//! Installs packaged content (one car or one track) from a `.zip` archive into an
//! Assetto Corsa folder.
//!
//! Two archive layouts are accepted:
//! - `content/cars/<id>/...` or `content/tracks/<id>/...`, the layout the game itself uses, and
//! - `<id>/...` at the archive root. The kind is then read from the files: a car has
//!   `ui/ui_car.json`, a track has `ui/ui_track.json` or a layout's `ui/<layout>/ui_track.json`.
//!
//! Every entry name is checked before anything is written. Absolute names, drive prefixes,
//! `..` components, backslashes, NUL bytes and symbolic links are refused, and so is any entry
//! outside the content folder.
//!
//! Extraction goes to a staging folder under `<ac_root>/.acxs_staging/`. The finished folder is
//! renamed into `<ac_root>/content/<cars|tracks>/<id>`, so a failed extraction leaves the content
//! folder untouched. Installing over an existing folder never deletes it: the old folder is moved
//! to `<ac_root>/acxs_archive/<cars|tracks>/<id>-<n>` first, and the report names that location.
//! Installation is refused while the game is running; the caller passes that state in.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use zip::ZipArchive;
use zip::result::ZipError;

/// Hidden folder under the AC root where extraction happens before the final rename.
const STAGING_DIR: &str = ".acxs_staging";
/// Folder under the AC root that receives previously installed content with the same id.
const ARCHIVE_DIR: &str = "acxs_archive";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Car,
    Track,
}

impl ContentKind {
    /// Name of the folder under `content/` that holds this kind.
    fn folder(self) -> &'static str {
        match self {
            ContentKind::Car => "cars",
            ContentKind::Track => "tracks",
        }
    }
}

/// What an archive holds, found without extracting anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveInfo {
    pub kind: ContentKind,
    /// Name of the car or track folder.
    pub id: String,
    /// Number of files that would be installed (directories are not counted).
    pub file_count: usize,
    /// Uncompressed size in bytes of those files.
    pub total_size: u64,
}

/// Result of a successful installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub kind: ContentKind,
    pub id: String,
    /// Where the content now lives: `<ac_root>/content/<cars|tracks>/<id>`.
    pub installed_to: PathBuf,
    pub file_count: usize,
    pub total_size: u64,
    /// Where a previously installed folder with the same id was moved, if there was one.
    pub moved_existing_to: Option<PathBuf>,
}

#[derive(Debug)]
pub enum InstallError {
    /// The game is running, so content must not change.
    GameRunning,
    /// The AC root is not an existing folder.
    AcRootMissing(PathBuf),
    /// The archive could not be read as a zip file.
    Zip(ZipError),
    /// An entry name is unsafe (absolute, a drive prefix, contains `..`, a backslash, NUL)
    /// or the entry is a symbolic link.
    UnsafeEntry(String),
    /// An entry lies outside the car or track folder, or a file sits where a folder must be.
    UnexpectedEntry(String),
    /// The archive holds neither a car nor a track.
    NotContent,
    /// The archive matches more than one car, track or folder.
    AmbiguousContent(String),
    /// Reading the archive or writing the content folder failed.
    Io(io::Error),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::GameRunning => write!(f, "the game is running; close it before installing content"),
            InstallError::AcRootMissing(path) => {
                write!(f, "Assetto Corsa folder not found: {}", path.display())
            }
            InstallError::Zip(e) => write!(f, "archive cannot be read: {e}"),
            InstallError::UnsafeEntry(name) => write!(f, "refusing unsafe archive entry {name:?}"),
            InstallError::UnexpectedEntry(name) => {
                write!(f, "archive entry {name:?} is outside the car or track folder")
            }
            InstallError::NotContent => write!(f, "archive holds neither a car nor a track"),
            InstallError::AmbiguousContent(detail) => write!(f, "archive layout is ambiguous: {detail}"),
            InstallError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InstallError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for InstallError {
    fn from(e: io::Error) -> Self {
        InstallError::Io(e)
    }
}

impl From<ZipError> for InstallError {
    fn from(e: ZipError) -> Self {
        InstallError::Zip(e)
    }
}

/// One archive entry, with its name already checked.
struct Entry {
    name: String,
    /// Path components with `.` and empty parts removed.
    parts: Vec<String>,
    is_dir: bool,
    size: u64,
}

/// Where the car or track sits inside the archive.
struct Layout {
    kind: ContentKind,
    id: String,
    /// Components leading to the content folder, e.g. `["content", "cars", "<id>"]` or `["<id>"]`.
    prefix: Vec<String>,
}

/// Reads an archive and reports what it holds: the kind, the folder name, the file count and the
/// total uncompressed size. Nothing is extracted. Unsafe or unexpected entries are an error.
pub fn inspect_archive(path: &Path) -> Result<ArchiveInfo, InstallError> {
    let mut archive = ZipArchive::new(File::open(path)?)?;
    let entries = read_entries(&mut archive)?;
    let layout = analyse(&entries)?;
    let (file_count, total_size) = totals(&entries, &layout);
    Ok(ArchiveInfo {
        kind: layout.kind,
        id: layout.id,
        file_count,
        total_size,
    })
}

/// Installs the car or track in `archive_path` into `ac_root/content`.
///
/// Refuses with [`InstallError::GameRunning`] when `game_running` is true, before reading the
/// archive. Any failure removes the staging folder and leaves existing content in place.
pub fn install_archive(
    ac_root: &Path,
    archive_path: &Path,
    game_running: bool,
) -> Result<InstallReport, InstallError> {
    if game_running {
        return Err(InstallError::GameRunning);
    }
    if !ac_root.is_dir() {
        return Err(InstallError::AcRootMissing(ac_root.to_path_buf()));
    }

    let mut archive = ZipArchive::new(File::open(archive_path)?)?;
    let entries = read_entries(&mut archive)?;
    let layout = analyse(&entries)?;
    let (file_count, total_size) = totals(&entries, &layout);

    let staging_parent = ac_root.join(STAGING_DIR);
    fs::create_dir_all(&staging_parent)?;
    let work = staging_parent.join(unique_name());
    fs::create_dir(&work)?;
    let staged = work.join(&layout.id);

    let outcome = extract(&mut archive, &entries, &layout, &staged)
        .and_then(|()| place(ac_root, &layout, &staged));

    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir(&staging_parent);
    let (installed_to, moved_existing_to) = outcome?;

    Ok(InstallReport {
        kind: layout.kind,
        id: layout.id,
        installed_to,
        file_count,
        total_size,
        moved_existing_to,
    })
}

/// Splits an entry name into safe components. Rejects anything that could leave the
/// destination folder: absolute names, drive prefixes, `..`, backslashes and NUL bytes.
fn safe_parts(name: &str) -> Result<Vec<String>, InstallError> {
    let unsafe_name = || InstallError::UnsafeEntry(name.to_string());
    if name.starts_with('/') || name.contains('\\') || name.contains('\0') {
        return Err(unsafe_name());
    }
    let mut parts = Vec::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        // A colon starts a drive prefix on Windows or an alternate stream name. Parts made only of
        // dots cover `..` and the Windows variants that get trimmed to it.
        if part.contains(':') || part.bytes().all(|b| b == b'.') {
            return Err(unsafe_name());
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

fn read_entries<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<Vec<Entry>, InstallError> {
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let name = file.name()?.into_owned();
        let parts = safe_parts(&name)?;
        if file.is_symlink() {
            return Err(InstallError::UnsafeEntry(name));
        }
        entries.push(Entry {
            is_dir: file.is_dir(),
            size: file.size(),
            name,
            parts,
        });
    }
    Ok(entries)
}

/// Works out the kind, id and prefix of the content folder, and checks that every entry lies
/// inside it. Folders leading to it (`content/`, `content/cars/`) may be listed as well.
fn analyse(entries: &[Entry]) -> Result<Layout, InstallError> {
    // Form 1: content/cars/<id>/... or content/tracks/<id>/...
    let mut found: Vec<(ContentKind, String)> = Vec::new();
    for entry in entries {
        let parts = &entry.parts;
        if parts.len() < 3 || parts[0] != "content" {
            continue;
        }
        let kind = match parts[1].as_str() {
            "cars" => ContentKind::Car,
            "tracks" => ContentKind::Track,
            _ => continue,
        };
        // A plain file directly under content/cars is not a content folder.
        if parts.len() == 3 && !entry.is_dir {
            continue;
        }
        let candidate = (kind, parts[2].clone());
        if !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    if found.len() > 1 {
        return Err(InstallError::AmbiguousContent(format!(
            "{} content folders under content/",
            found.len()
        )));
    }
    if let Some((kind, id)) = found.pop() {
        let prefix = vec!["content".to_string(), kind.folder().to_string(), id.clone()];
        for entry in entries {
            if entry.parts.is_empty() {
                continue;
            }
            if entry.parts == prefix && !entry.is_dir {
                return Err(InstallError::UnexpectedEntry(entry.name.clone()));
            }
            if entry.parts.starts_with(&prefix) {
                continue;
            }
            if entry.is_dir && prefix.starts_with(&entry.parts) {
                continue;
            }
            return Err(InstallError::UnexpectedEntry(entry.name.clone()));
        }
        return Ok(Layout { kind, id, prefix });
    }

    // Form 2: <id>/... at the archive root, with the kind read from the files inside.
    let mut root: Option<String> = None;
    for entry in entries.iter().filter(|e| !e.parts.is_empty()) {
        let first = &entry.parts[0];
        match &root {
            None => root = Some(first.clone()),
            Some(existing) if existing != first => {
                return Err(InstallError::UnexpectedEntry(entry.name.clone()));
            }
            Some(_) => {}
        }
        if entry.parts.len() == 1 && !entry.is_dir {
            return Err(InstallError::UnexpectedEntry(entry.name.clone()));
        }
    }
    let Some(id) = root else {
        return Err(InstallError::NotContent);
    };

    let (mut has_car, mut has_track) = (false, false);
    for entry in entries {
        let Some((first, rest)) = entry.parts.split_first() else {
            continue;
        };
        if *first != id {
            continue;
        }
        let names: Vec<&str> = rest.iter().map(String::as_str).collect();
        match names.as_slice() {
            ["ui", "ui_car.json"] => has_car = true,
            ["ui", "ui_track.json"] => has_track = true,
            ["ui", _, "ui_track.json"] => has_track = true,
            _ => {}
        }
    }
    let kind = match (has_car, has_track) {
        (true, false) => ContentKind::Car,
        (false, true) => ContentKind::Track,
        (true, true) => {
            return Err(InstallError::AmbiguousContent(format!(
                "{id} has both car and track markers"
            )));
        }
        (false, false) => return Err(InstallError::NotContent),
    };
    let prefix = vec![id.clone()];
    Ok(Layout { kind, id, prefix })
}

/// File count and total uncompressed size of the entries that will be installed.
fn totals(entries: &[Entry], layout: &Layout) -> (usize, u64) {
    entries
        .iter()
        .filter(|e| !e.is_dir && e.parts.starts_with(&layout.prefix))
        .fold((0, 0u64), |(count, size), e| (count + 1, size + e.size))
}

/// Writes the content folder into `staged`. Every path is built from checked components, and
/// files are created with `create_new`, so an entry can never overwrite something outside.
fn extract<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    entries: &[Entry],
    layout: &Layout,
    staged: &Path,
) -> Result<(), InstallError> {
    fs::create_dir_all(staged)?;
    for (index, entry) in entries.iter().enumerate() {
        if !entry.parts.starts_with(&layout.prefix) {
            continue;
        }
        let rel = &entry.parts[layout.prefix.len()..];
        if rel.is_empty() {
            continue;
        }
        let mut target = staged.to_path_buf();
        for part in rel {
            target.push(part);
        }
        if entry.is_dir {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = archive.by_index(index)?;
            let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&target)?;
            io::copy(&mut file, &mut out)?;
        }
    }
    Ok(())
}

/// Moves the finished folder into `content/<cars|tracks>/<id>`. An existing folder with that id
/// is moved to the archive folder first and never deleted. Returns the target and the archive
/// location of the replaced folder, if any.
fn place(ac_root: &Path, layout: &Layout, staged: &Path) -> Result<(PathBuf, Option<PathBuf>), InstallError> {
    let parent = ac_root.join("content").join(layout.kind.folder());
    fs::create_dir_all(&parent)?;
    let target = parent.join(&layout.id);

    let moved = if fs::symlink_metadata(&target).is_ok() {
        let archive_dir = ac_root.join(ARCHIVE_DIR).join(layout.kind.folder());
        fs::create_dir_all(&archive_dir)?;
        let dest = free_archive_name(&archive_dir, &layout.id);
        fs::rename(&target, &dest)?;
        Some(dest)
    } else {
        None
    };

    if let Err(e) = fs::rename(staged, &target) {
        // Put the old folder back so a failed install changes nothing.
        if let Some(dest) = &moved {
            let _ = fs::rename(dest, &target);
        }
        return Err(e.into());
    }
    Ok((target, moved))
}

/// First `<dir>/<id>-<n>` (n from 1) that does not exist yet.
fn free_archive_name(dir: &Path, id: &str) -> PathBuf {
    let mut n: u32 = 1;
    loop {
        let candidate = dir.join(format!("{id}-{n}"));
        if fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
        n += 1;
    }
}

fn unique_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Temp folder that is removed on drop, even when an assertion fails.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!("acxs-installer-{tag}-{}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Writes a zip at `path`. A name ending in `/` becomes a directory entry.
    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for &(name, text) in files {
            let options = zip::write::SimpleFileOptions::default();
            if name.ends_with('/') {
                zip.add_directory(name, options).unwrap();
            } else {
                zip.start_file(name, options).unwrap();
                zip.write_all(text.as_bytes()).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    const CAR: &[(&str, &str)] = &[
        ("content/", ""),
        ("content/cars/abc/", ""),
        ("content/cars/abc/ui/ui_car.json", "{}"),
        ("content/cars/abc/data/car.ini", "[BASIC]\n"),
        ("content/cars/abc/abc.kn5", "kn5"),
    ];

    /// Track with the folder at the archive root and a layout's UI file.
    const TRACK_ROOT: &[(&str, &str)] = &[
        ("trk/", ""),
        ("trk/ui/ui_track.json", "{}"),
        ("trk/data/surfaces.ini", ""),
        ("trk/ai/fast_lane.ai", "ai"),
    ];

    fn ac_root(dir: &TempDir) -> PathBuf {
        let ac = dir.0.join("ac");
        fs::create_dir_all(ac.join("content")).unwrap();
        ac
    }

    #[test]
    fn inspects_car_archive() {
        let dir = TempDir::new("inspect-car");
        let zip = dir.0.join("car.zip");
        make_zip(&zip, CAR);

        let info = inspect_archive(&zip).unwrap();
        assert_eq!(
            info,
            ArchiveInfo {
                kind: ContentKind::Car,
                id: "abc".to_string(),
                file_count: 3,
                total_size: 2 + 8 + 3,
            }
        );
    }

    #[test]
    fn installs_car_from_content_layout() {
        let dir = TempDir::new("install-car");
        let ac = ac_root(&dir);
        let zip = dir.0.join("car.zip");
        make_zip(&zip, CAR);

        let report = install_archive(&ac, &zip, false).unwrap();
        assert_eq!(report.kind, ContentKind::Car);
        assert_eq!(report.id, "abc");
        assert_eq!(report.installed_to, ac.join("content/cars/abc"));
        assert_eq!(report.file_count, 3);
        assert!(report.moved_existing_to.is_none());
        assert_eq!(fs::read_to_string(ac.join("content/cars/abc/abc.kn5")).unwrap(), "kn5");
        assert_eq!(fs::read_to_string(ac.join("content/cars/abc/ui/ui_car.json")).unwrap(), "{}");
        assert!(!ac.join(STAGING_DIR).exists(), "staging folder must be cleaned up");
    }

    #[test]
    fn installs_track_from_root_folder() {
        let dir = TempDir::new("install-track");
        let ac = ac_root(&dir);
        let zip = dir.0.join("track.zip");
        make_zip(&zip, TRACK_ROOT);

        let info = inspect_archive(&zip).unwrap();
        assert_eq!(info.kind, ContentKind::Track);
        assert_eq!(info.id, "trk");
        assert_eq!(info.file_count, 3);
        assert_eq!(info.total_size, 2 + 0 + 2);

        let report = install_archive(&ac, &zip, false).unwrap();
        assert_eq!(report.installed_to, ac.join("content/tracks/trk"));
        assert!(ac.join("content/tracks/trk/data/surfaces.ini").is_file());
        assert!(ac.join("content/tracks/trk/ai/fast_lane.ai").is_file());
    }

    #[test]
    fn refuses_unsafe_entry_names() {
        let dir = TempDir::new("traversal");
        let ac = ac_root(&dir);
        let zip = dir.0.join("evil.zip");

        let bad_names = [
            "content/cars/abc/../../../../escape.txt",
            "content/cars/abc/../x.txt",
            "/etc/escape.txt",
            "C:/escape.txt",
            "content/cars/abc/..\\escape.txt",
            "content/cars/abc/.../x.txt",
        ];
        for bad in bad_names {
            let mut files = CAR.to_vec();
            files.push((bad, "pwned"));
            make_zip(&zip, &files);

            assert!(
                matches!(inspect_archive(&zip), Err(InstallError::UnsafeEntry(_))),
                "inspect accepted {bad}"
            );
            assert!(
                matches!(install_archive(&ac, &zip, false), Err(InstallError::UnsafeEntry(_))),
                "install accepted {bad}"
            );
        }

        assert!(!dir.0.join("escape.txt").exists());
        assert!(!ac.join("content/cars/abc").exists());
        assert!(!ac.join(STAGING_DIR).exists());
    }

    #[test]
    fn refuses_entries_outside_the_content_folder() {
        let dir = TempDir::new("outside");
        let zip = dir.0.join("mixed.zip");
        let mut files = CAR.to_vec();
        files.push(("readme.txt", "hi"));
        make_zip(&zip, &files);

        assert!(matches!(inspect_archive(&zip), Err(InstallError::UnexpectedEntry(_))));
    }

    #[test]
    fn refuses_archive_without_car_or_track() {
        let dir = TempDir::new("no-content");
        let plain = dir.0.join("plain.zip");
        make_zip(&plain, &[("docs/readme.txt", "hi")]);
        assert!(matches!(inspect_archive(&plain), Err(InstallError::NotContent)));

        let both = dir.0.join("both.zip");
        make_zip(
            &both,
            &[
                ("content/cars/abc/ui/ui_car.json", "{}"),
                ("content/tracks/x/ui/ui_track.json", "{}"),
            ],
        );
        assert!(matches!(inspect_archive(&both), Err(InstallError::AmbiguousContent(_))));
    }

    #[test]
    fn refuses_while_game_is_running() {
        let dir = TempDir::new("game-running");
        let ac = ac_root(&dir);
        let zip = dir.0.join("car.zip");
        make_zip(&zip, CAR);

        let err = install_archive(&ac, &zip, true).unwrap_err();
        assert!(matches!(err, InstallError::GameRunning));
        assert!(!ac.join("content/cars/abc").exists());
        assert!(!ac.join(STAGING_DIR).exists());
    }

    #[test]
    fn reinstall_moves_existing_folder_aside() {
        let dir = TempDir::new("reinstall");
        let ac = ac_root(&dir);
        let zip = dir.0.join("car.zip");
        make_zip(&zip, CAR);
        install_archive(&ac, &zip, false).unwrap();
        fs::write(ac.join("content/cars/abc/old-only.txt"), "keep me").unwrap();

        let report = install_archive(&ac, &zip, false).unwrap();
        let moved = report.moved_existing_to.expect("old folder should be moved aside");
        assert_eq!(moved, ac.join("acxs_archive/cars/abc-1"));
        assert_eq!(fs::read_to_string(moved.join("old-only.txt")).unwrap(), "keep me");
        assert!(!ac.join("content/cars/abc/old-only.txt").exists());
        assert!(ac.join("content/cars/abc/abc.kn5").is_file());
    }

    #[test]
    fn failed_extraction_leaves_nothing_behind() {
        let dir = TempDir::new("failed-extract");
        let ac = ac_root(&dir);
        let zip = dir.0.join("dup.zip");
        // abc.kn5 is written as a file, then the entry below needs it to be a folder, so
        // extraction fails part way through.
        let mut files = CAR.to_vec();
        files.push(("content/cars/abc/abc.kn5/inner.txt", "again"));
        make_zip(&zip, &files);

        assert!(install_archive(&ac, &zip, false).is_err());
        assert!(!ac.join("content/cars/abc").exists());
        assert!(!ac.join(STAGING_DIR).exists());
    }
}
