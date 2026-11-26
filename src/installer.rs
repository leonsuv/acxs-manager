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
    /// An external tool (e.g. 7-Zip / UnRAR) is required to unpack this format.
    ExternalToolRequired { format: String, hint: String },
    /// Uncompressed payload exceeds the safety limit (zip-bomb protection).
    ArchiveTooLarge { bytes: u64, limit: u64 },
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
            InstallError::ExternalToolRequired { format, hint } => {
                write!(f, "{format} archives require an external extractor: {hint}")
            }
            InstallError::ArchiveTooLarge { bytes, limit } => {
                write!(f, "archive uncompressed size ({bytes} bytes) exceeds limit ({limit} bytes)")
            }
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
    // Folder names are matched ignoring case, as Windows does (`Content/Cars/...`).
    let mut found: Vec<(ContentKind, String)> = Vec::new();
    let mut spelling: Option<(String, String)> = None;
    for entry in entries {
        let parts = &entry.parts;
        if parts.len() < 3 || !parts[0].eq_ignore_ascii_case("content") {
            continue;
        }
        let kind = match parts[1].to_ascii_lowercase().as_str() {
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
            spelling = Some((parts[0].clone(), parts[1].clone()));
        }
    }
    if found.len() > 1 {
        return Err(InstallError::AmbiguousContent(format!(
            "{} content folders under content/",
            found.len()
        )));
    }
    if let Some((kind, id)) = found.pop() {
        let (content, folder) = spelling.unwrap_or_else(|| ("content".to_string(), kind.folder().to_string()));
        let prefix = vec![content, folder, id.clone()];
        for entry in entries {
            if entry.parts.is_empty() || is_loose_root_file(entry) {
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
    for entry in entries.iter().filter(|e| !e.parts.is_empty() && !is_loose_root_file(e)) {
        let first = &entry.parts[0];
        match &root {
            None => root = Some(first.clone()),
            Some(existing) if existing != first => {
                return Err(InstallError::UnexpectedEntry(entry.name.clone()));
            }
            Some(_) => {}
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
        let names: Vec<String> = rest.iter().map(|n| n.to_ascii_lowercase()).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
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

/// A plain file at the archive root, such as `readme.txt` or a licence. Mod
/// archives often carry one next to the content folder; it is not installed.
fn is_loose_root_file(entry: &Entry) -> bool {
    entry.parts.len() == 1 && !entry.is_dir
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

// ---------------------------------------------------------------------------
// Universal Drag-and-Drop Multi-Format & Multi-Item Package Installer
// ---------------------------------------------------------------------------

/// Uncompressed size cap per package (4 GiB) to defend against archive bombs.
pub const MAX_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Backward-compatible alias for [`install_archive`].
pub fn install_zip(
    ac_root: &Path,
    archive_path: &Path,
    game_running: bool,
) -> Result<InstallReport, InstallError> {
    install_archive(ac_root, archive_path, game_running)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    SevenZ,
    TarGz,
    Rar,
    Folder,
    SingleFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetKind {
    Car,
    Track,
    CarSkin { target_car: Option<String> },
    TrackSkin { target_track: Option<String> },
    PpFilter,
    Weather,
    PythonApp,
    LuaApp,
    CspConfig,
    Font,
    Driver,
    Helmet,
    Crew,
    Sound { target_car: Option<String> },
    Showroom,
    GenericMod,
}

impl AssetKind {
    pub fn label(&self) -> &'static str {
        match self {
            AssetKind::Car => "Car",
            AssetKind::Track => "Track",
            AssetKind::CarSkin { .. } => "Car Skin",
            AssetKind::TrackSkin { .. } => "Track Skin",
            AssetKind::PpFilter => "PP Filter",
            AssetKind::Weather => "Weather",
            AssetKind::PythonApp => "Python App",
            AssetKind::LuaApp => "CSP Lua App",
            AssetKind::CspConfig => "CSP Config",
            AssetKind::Font => "Font",
            AssetKind::Driver => "Driver Model",
            AssetKind::Helmet => "Helmet",
            AssetKind::Crew => "Crew",
            AssetKind::Sound { .. } => "Sound",
            AssetKind::Showroom => "Showroom",
            AssetKind::GenericMod => "Generic Mod",
        }
    }

    fn archive_bucket(&self) -> &'static str {
        match self {
            AssetKind::Car => "cars",
            AssetKind::Track => "tracks",
            AssetKind::CarSkin { .. } => "skins",
            AssetKind::TrackSkin { .. } => "track_skins",
            AssetKind::PpFilter => "ppfilters",
            AssetKind::Weather => "weather",
            AssetKind::PythonApp => "python_apps",
            AssetKind::LuaApp => "lua_apps",
            AssetKind::CspConfig => "csp_configs",
            AssetKind::Font => "fonts",
            AssetKind::Driver => "drivers",
            AssetKind::Helmet => "helmets",
            AssetKind::Crew => "crew",
            AssetKind::Sound { .. } => "sounds",
            AssetKind::Showroom => "showrooms",
            AssetKind::GenericMod => "generic_mods",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    ExistingDir(PathBuf),
    ExistingFiles(Vec<PathBuf>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedItem {
    pub kind: AssetKind,
    pub id: String,
    pub archive_prefix: Vec<String>,
    /// Relative destination path under `ac_root`.
    pub dest_rel: PathBuf,
    pub file_count: usize,
    pub total_size: u64,
    pub conflict: Option<ConflictKind>,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePlan {
    pub source: PathBuf,
    pub format: ArchiveFormat,
    pub items: Vec<PlannedItem>,
    pub skipped_root_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledItemReport {
    pub kind: AssetKind,
    pub id: String,
    pub installed_to: PathBuf,
    pub file_count: usize,
    pub moved_existing_to: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchInstallReport {
    pub installed: Vec<InstalledItemReport>,
}

/// Detects the package format from path type, extension, or magic bytes.
pub fn detect_format(path: &Path) -> Result<ArchiveFormat, InstallError> {
    if path.is_dir() {
        return Ok(ArchiveFormat::Folder);
    }
    if !path.is_file() {
        return Err(InstallError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("package not found: {}", path.display()),
        )));
    }
    let name_lower = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name_lower.ends_with(".tar.gz") || name_lower.ends_with(".tgz") {
        return Ok(ArchiveFormat::TarGz);
    }
    if name_lower.ends_with(".7z") {
        return Ok(ArchiveFormat::SevenZ);
    }
    if name_lower.ends_with(".rar") {
        return Ok(ArchiveFormat::Rar);
    }
    if name_lower.ends_with(".zip") {
        return Ok(ArchiveFormat::Zip);
    }
    if name_lower.ends_with(".ini") {
        return Ok(ArchiveFormat::SingleFile);
    }
    // Sniff magic bytes as fallback.
    let mut magic = [0u8; 8];
    let n = File::open(path)?.read(&mut magic)?;
    if n >= 4 && &magic[..4] == b"PK\x03\x04" {
        return Ok(ArchiveFormat::Zip);
    }
    if n >= 6 && &magic[..6] == b"7z\xBC\xAF\x27\x1C" {
        return Ok(ArchiveFormat::SevenZ);
    }
    if n >= 7 && &magic[..7] == b"Rar!\x1A\x07\x00" || n >= 8 && &magic[..8] == b"Rar!\x1A\x07\x01\x00" {
        return Ok(ArchiveFormat::Rar);
    }
    if n >= 2 && magic[0] == 0x1F && magic[1] == 0x8B {
        return Ok(ArchiveFormat::TarGz);
    }
    Err(InstallError::NotContent)
}

/// Inspects any supported package (`.zip`, `.7z`, `.tar.gz`/`.tgz`, `.rar`, folder, or `.ini` PP
/// filter) and classifies every Assetto Corsa asset inside it without writing to `ac_root`.
pub fn inspect_package(
    path: &Path,
    ac_root: Option<&Path>,
    seven_zip_path: Option<&Path>,
) -> Result<PackagePlan, InstallError> {
    let format = detect_format(path)?;
    if format == ArchiveFormat::SingleFile {
        let meta = fs::metadata(path)?;
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(InstallError::NotContent)?
            .to_string();
        let text = fs::read_to_string(path).unwrap_or_default();
        let upper = text.to_ascii_uppercase();
        if upper.contains("[TONEMAPPING]") || upper.contains("[GLARE]") || upper.contains("[AUTO_EXPOSURE]") {
            let dest_rel = PathBuf::from("system/cfg/ppfilters").join(&fname);
            let conflict = ac_root
                .map(|r| r.join(&dest_rel))
                .filter(|p| p.exists())
                .map(|p| ConflictKind::ExistingFiles(vec![p]));
            return Ok(PackagePlan {
                source: path.to_path_buf(),
                format,
                items: vec![PlannedItem {
                    kind: AssetKind::PpFilter,
                    id: fname,
                    archive_prefix: Vec::new(),
                    dest_rel,
                    file_count: 1,
                    total_size: meta.len(),
                    conflict,
                    selected: true,
                }],
                skipped_root_files: Vec::new(),
            });
        }
        return Err(InstallError::NotContent);
    }

    let entries = match format {
        ArchiveFormat::Zip => {
            let mut archive = ZipArchive::new(File::open(path)?)?;
            read_entries(&mut archive)?
        }
        ArchiveFormat::SevenZ => read_7z_entries(path)?,
        ArchiveFormat::TarGz => read_targz_entries(path)?,
        ArchiveFormat::Folder => read_folder_entries(path)?,
        ArchiveFormat::Rar => {
            let _tool = find_rar_extractor(seven_zip_path)?;
            // RAR inspection uses external extraction into a temporary dir.
            let tmp = std::env::temp_dir().join(format!("acxs-rar-inspect-{}", unique_name()));
            fs::create_dir_all(&tmp)?;
            let res = extract_rar_external(&_tool, path, &tmp).and_then(|()| read_folder_entries(&tmp));
            let _ = fs::remove_dir_all(&tmp);
            res?
        }
        ArchiveFormat::SingleFile => unreachable!(),
    };

    let total_bytes: u64 = entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
    if total_bytes > MAX_UNCOMPRESSED_BYTES {
        return Err(InstallError::ArchiveTooLarge {
            bytes: total_bytes,
            limit: MAX_UNCOMPRESSED_BYTES,
        });
    }

    let (mut items, skipped_root_files) = classify_entries(&entries)?;
    if let Some(root) = ac_root {
        for item in &mut items {
            let dest = root.join(&item.dest_rel);
            if dest.is_dir() {
                item.conflict = Some(ConflictKind::ExistingDir(dest));
            } else if dest.is_file() {
                item.conflict = Some(ConflictKind::ExistingFiles(vec![dest]));
            }
        }
    }

    Ok(PackagePlan {
        source: path.to_path_buf(),
        format,
        items,
        skipped_root_files,
    })
}

fn read_7z_entries(path: &Path) -> Result<Vec<Entry>, InstallError> {
    let mut entries = Vec::new();
    let file = File::open(path)?;
    let reader = sevenz_rust2::ArchiveReader::new(file, sevenz_rust2::Password::empty())
        .map_err(|e| InstallError::Io(io::Error::new(io::ErrorKind::InvalidData, e.to_string())))?;
    for entry in &reader.archive().files {
        let name = entry.name().replace('\\', "/");
        if name.is_empty() {
            continue;
        }
        let parts = safe_parts(&name)?;
        entries.push(Entry {
            name,
            parts,
            is_dir: entry.is_directory(),
            size: entry.size(),
        });
    }
    Ok(entries)
}

fn read_targz_entries(path: &Path) -> Result<Vec<Entry>, InstallError> {
    let gz = flate2::read::GzDecoder::new(File::open(path)?);
    let mut archive = tar::Archive::new(gz);
    let mut entries = Vec::new();
    for item in archive.entries()? {
        let entry = item?;
        if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
            let raw = entry.path().map(|p| p.display().to_string()).unwrap_or_default();
            return Err(InstallError::UnsafeEntry(raw));
        }
        let raw_path = entry.path()?;
        let name = raw_path.to_string_lossy().replace('\\', "/");
        let parts = safe_parts(&name)?;
        entries.push(Entry {
            is_dir: entry.header().entry_type().is_dir(),
            size: entry.header().size()?,
            name,
            parts,
        });
    }
    Ok(entries)
}

fn read_folder_entries(root: &Path) -> Result<Vec<Entry>, InstallError> {
    let mut out = Vec::new();
    let root_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("package")
        .to_string();
    read_folder_entries_rec(root, root, &root_name, &mut out)?;
    Ok(out)
}

fn read_folder_entries_rec(
    root: &Path,
    dir: &Path,
    root_name: &str,
    out: &mut Vec<Entry>,
) -> Result<(), InstallError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            return Err(InstallError::UnsafeEntry(path.display().to_string()));
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| InstallError::UnsafeEntry(path.display().to_string()))?;
        let mut parts = vec![root_name.to_string()];
        for c in rel.components() {
            parts.push(c.as_os_str().to_string_lossy().into_owned());
        }
        let name = parts.join("/");
        let parts = safe_parts(&name)?;
        if ft.is_dir() {
            out.push(Entry {
                name,
                parts,
                is_dir: true,
                size: 0,
            });
            read_folder_entries_rec(root, &path, root_name, out)?;
        } else if ft.is_file() {
            let size = entry.metadata()?.len();
            out.push(Entry {
                name,
                parts,
                is_dir: false,
                size,
            });
        }
    }
    Ok(())
}

fn find_rar_extractor(configured: Option<&Path>) -> Result<PathBuf, InstallError> {
    if let Some(p) = configured
        && p.is_file()
    {
        return Ok(p.to_path_buf());
    }
    for candidate in [
        r"C:\Program Files\7-Zip\7z.exe",
        r"C:\Program Files (x86)\7-Zip\7z.exe",
        r"C:\Program Files\WinRAR\UnRAR.exe",
        "/usr/local/bin/7z",
        "/opt/homebrew/bin/7z",
        "/usr/bin/7z",
    ] {
        let p = Path::new(candidate);
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
    }
    Err(InstallError::ExternalToolRequired {
        format: "RAR".into(),
        hint: "Install 7-Zip or WinRAR and set its path in Settings.".into(),
    })
}

fn extract_rar_external(tool: &Path, archive: &Path, dest: &Path) -> Result<(), InstallError> {
    let status = process::Command::new(tool)
        .arg("x")
        .arg("-y")
        .arg(archive)
        .arg(format!("-o{}", dest.display()))
        .status()?;
    if !status.success() {
        return Err(InstallError::Io(io::Error::other(format!(
            "external extractor exited with {status}"
        ))));
    }
    Ok(())
}

/// Strips a single wrapper folder if every non-loose entry sits under `<wrapper>/` and `<wrapper>`
/// is not itself a canonical AC root folder (`content`, `apps`, `extension`, `system`).
fn strip_wrapper_prefix(entries: &[Entry]) -> Vec<String> {
    const AC_ROOTS: [&str; 4] = ["content", "apps", "extension", "system"];
    let mut prefix: Vec<String> = Vec::new();
    let mut offset = 0usize;
    loop {
        let mut first_dir: Option<String> = None;
        let mut uniform = true;
        let mut saw_any = false;
        for e in entries {
            if e.parts.len() <= offset {
                continue;
            }
            if e.parts.len() == offset + 1 && !e.is_dir {
                continue; // loose readme/licence file
            }
            saw_any = true;
            let seg = &e.parts[offset];
            match &first_dir {
                None => first_dir = Some(seg.clone()),
                Some(prev) if prev.eq_ignore_ascii_case(seg) => {}
                Some(_) => {
                    uniform = false;
                    break;
                }
            }
        }
        let Some(dir) = first_dir.filter(|_| saw_any && uniform) else {
            break;
        };
        if AC_ROOTS.iter().any(|r| r.eq_ignore_ascii_case(&dir)) {
            break;
        }
        // Stop if stripping this dir would hide a standalone car/track/skin/app root.
        let has_direct_marker = entries.iter().any(|e| {
            e.parts.len() > offset + 1
                && e.parts[offset].eq_ignore_ascii_case(&dir)
                && matches!(
                    e.parts[offset + 1].to_ascii_lowercase().as_str(),
                    "ui" | "data" | "skins" | "sfx" | "ui_skin.json" | "weather.ini" | "manifest.ini"
                )
        });
        if has_direct_marker {
            break;
        }
        prefix.push(dir);
        offset += 1;
    }
    prefix
}

/// Classifies all entries in a package into one or more [`PlannedItem`]s.
fn classify_entries(entries: &[Entry]) -> Result<(Vec<PlannedItem>, Vec<String>), InstallError> {
    let wrapper = strip_wrapper_prefix(entries);
    let wlen = wrapper.len();
    let mut skipped_root_files = Vec::new();

    // Collect canonical AC-root items keyed by (bucket_key) -> (AssetKind, id, archive_prefix, dest_rel).
    let mut canonical: Vec<(AssetKind, String, Vec<String>, PathBuf)> = Vec::new();
    let mut seen_dest: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for entry in entries {
        if entry.parts.len() <= wlen {
            if entry.parts.len() == 1 && !entry.is_dir {
                skipped_root_files.push(entry.name.clone());
            }
            continue;
        }
        if !wrapper
            .iter()
            .zip(&entry.parts)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            if entry.parts.len() == 1 && !entry.is_dir {
                skipped_root_files.push(entry.name.clone());
            }
            continue;
        }
        let rel = &entry.parts[wlen..];
        if rel.len() == 1 && !entry.is_dir {
            skipped_root_files.push(entry.name.clone());
            continue;
        }
        let l0 = rel[0].to_ascii_lowercase();
        let l1 = rel.get(1).map(|s| s.to_ascii_lowercase());
        let l2 = rel.get(2).cloned();
        let l3 = rel.get(3).map(|s| s.to_ascii_lowercase());
        let l4 = rel.get(4).cloned();

        let matched: Option<(AssetKind, String, usize, PathBuf)> = match (l0.as_str(), l1.as_deref()) {
            ("content", Some("cars")) => match (l2.as_deref(), l3.as_deref(), l4.as_deref()) {
                (Some(car), Some("skins"), Some(skin)) if !entry.is_dir || rel.len() >= 5 => {
                    // Only treat as CarSkin if there is NO ui_car.json for this car in the archive.
                    let has_car_ui = entries.iter().any(|e| {
                        e.parts.len() >= wlen + 5
                            && e.parts[wlen].eq_ignore_ascii_case("content")
                            && e.parts[wlen + 1].eq_ignore_ascii_case("cars")
                            && e.parts[wlen + 2].eq_ignore_ascii_case(car)
                            && e.parts[wlen + 3].eq_ignore_ascii_case("ui")
                            && e.parts[wlen + 4].eq_ignore_ascii_case("ui_car.json")
                    });
                    if has_car_ui {
                        Some((
                            AssetKind::Car,
                            car.to_string(),
                            3,
                            PathBuf::from("content/cars").join(car),
                        ))
                    } else {
                        Some((
                            AssetKind::CarSkin {
                                target_car: Some(car.to_string()),
                            },
                            skin.to_string(),
                            5,
                            PathBuf::from("content/cars").join(car).join("skins").join(skin),
                        ))
                    }
                }
                (Some(car), Some("sfx"), _) if !entries.iter().any(|e| {
                    e.parts.len() >= wlen + 5
                        && e.parts[wlen].eq_ignore_ascii_case("content")
                        && e.parts[wlen + 1].eq_ignore_ascii_case("cars")
                        && e.parts[wlen + 2].eq_ignore_ascii_case(car)
                        && e.parts[wlen + 3].eq_ignore_ascii_case("ui")
                        && e.parts[wlen + 4].eq_ignore_ascii_case("ui_car.json")
                }) => Some((
                    AssetKind::Sound {
                        target_car: Some(car.to_string()),
                    },
                    car.to_string(),
                    4,
                    PathBuf::from("content/cars").join(car).join("sfx"),
                )),
                (Some(car), _, _) if rel.len() > 3 || entry.is_dir => Some((
                    AssetKind::Car,
                    car.to_string(),
                    3,
                    PathBuf::from("content/cars").join(car),
                )),
                _ => None,
            },
            ("content", Some("tracks")) => match (l2.as_deref(), l3.as_deref(), l4.as_deref()) {
                (Some(track), Some("skins"), Some(sub)) if sub.eq_ignore_ascii_case("cm_skins") => {
                    rel.get(5).map(|skin| {
                        (
                            AssetKind::TrackSkin {
                                target_track: Some(track.to_string()),
                            },
                            skin.clone(),
                            6,
                            PathBuf::from("content/tracks")
                                .join(track)
                                .join("skins/cm_skins")
                                .join(skin),
                        )
                    })
                }
                (Some(track), _, _) if rel.len() > 3 || entry.is_dir => Some((
                    AssetKind::Track,
                    track.to_string(),
                    3,
                    PathBuf::from("content/tracks").join(track),
                )),
                _ => None,
            },
            ("content", Some("weather")) => l2.map(|id| {
                (
                    AssetKind::Weather,
                    id.clone(),
                    3,
                    PathBuf::from("content/weather").join(id),
                )
            }),
            ("content", Some("showroom")) => l2.map(|id| {
                (
                    AssetKind::Showroom,
                    id.clone(),
                    3,
                    PathBuf::from("content/showroom").join(id),
                )
            }),
            ("content", Some("fonts")) if !entry.is_dir && rel.len() >= 3 => Some((
                AssetKind::Font,
                rel[2].clone(),
                3,
                PathBuf::from("content/fonts").join(&rel[2]),
            )),
            ("content", Some("driver")) if !entry.is_dir && rel.len() >= 3 => Some((
                AssetKind::Driver,
                rel[2].clone(),
                3,
                PathBuf::from("content/driver").join(&rel[2]),
            )),
            ("content", Some("texture")) if !entry.is_dir && rel.len() >= 3 => {
                let f = rel[2].to_ascii_lowercase();
                let kind = if f.contains("helmet") {
                    AssetKind::Helmet
                } else if f.contains("crew") {
                    AssetKind::Crew
                } else {
                    AssetKind::GenericMod
                };
                Some((kind, rel[2].clone(), 3, PathBuf::from("content/texture").join(&rel[2])))
            }
            ("system", Some("cfg")) if rel.get(2).is_some_and(|s| s.eq_ignore_ascii_case("ppfilters")) && !entry.is_dir && rel.len() >= 4 => {
                Some((
                    AssetKind::PpFilter,
                    rel[3].clone(),
                    4,
                    PathBuf::from("system/cfg/ppfilters").join(&rel[3]),
                ))
            }
            ("apps", Some("python")) => l2.map(|id| {
                (
                    AssetKind::PythonApp,
                    id.clone(),
                    3,
                    PathBuf::from("apps/python").join(id),
                )
            }),
            ("apps", Some("lua")) => l2.map(|id| {
                (
                    AssetKind::LuaApp,
                    id.clone(),
                    3,
                    PathBuf::from("apps/lua").join(id),
                )
            }),
            ("extension", Some("config")) if !entry.is_dir && rel.len() >= 3 => {
                let dest: PathBuf = rel.iter().collect();
                let id = rel.last().cloned().unwrap_or_default();
                Some((AssetKind::CspConfig, id, rel.len(), dest))
            }
            _ => None,
        };

        if let Some((kind, id, rel_prefix_len, dest_rel)) = matched {
            if seen_dest.insert(dest_rel.clone()) {
                let mut archive_prefix = wrapper.clone();
                archive_prefix.extend(rel[..rel_prefix_len].iter().cloned());
                canonical.push((kind, id, archive_prefix, dest_rel));
            }
        }
    }

    if !canonical.is_empty() {
        let items = canonical
            .into_iter()
            .map(|(kind, id, archive_prefix, dest_rel)| {
                let (file_count, total_size) = entries
                    .iter()
                    .filter(|e| !e.is_dir && starts_with_ci(&e.parts, &archive_prefix))
                    .fold((0usize, 0u64), |(c, s), e| (c + 1, s + e.size));
                PlannedItem {
                    kind,
                    id,
                    archive_prefix,
                    dest_rel,
                    file_count,
                    total_size,
                    conflict: None,
                    selected: true,
                }
            })
            .filter(|item| item.file_count > 0)
            .collect::<Vec<_>>();
        if !items.is_empty() {
            return Ok((items, skipped_root_files));
        }
    }

    // Fallback: standalone folder(s) at archive root (after stripping wrapper).
    let mut top_dirs: Vec<String> = Vec::new();
    for e in entries {
        if e.parts.len() <= wlen || (e.parts.len() == wlen + 1 && !e.is_dir) {
            continue;
        }
        let d = &e.parts[wlen];
        if !top_dirs.iter().any(|x| x.eq_ignore_ascii_case(d)) {
            top_dirs.push(d.clone());
        }
    }

    let mut fallback_items = Vec::new();
    for dir in top_dirs {
        let mut prefix = wrapper.clone();
        prefix.push(dir.clone());
        let sub_entries: Vec<&Entry> = entries
            .iter()
            .filter(|e| !e.is_dir && starts_with_ci(&e.parts, &prefix))
            .collect();
        if sub_entries.is_empty() {
            continue;
        }
        let has_rel = |target: &[&str]| -> bool {
            sub_entries.iter().any(|e| {
                let tail = &e.parts[prefix.len()..];
                tail.len() == target.len()
                    && tail
                        .iter()
                        .zip(target)
                        .all(|(a, b)| *b == "*" || a.eq_ignore_ascii_case(b))
            })
        };
        let has_ext_top = |ext: &str| -> bool {
            sub_entries.iter().any(|e| {
                let tail = &e.parts[prefix.len()..];
                tail.len() == 1
                    && Path::new(&tail[0])
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| x.eq_ignore_ascii_case(ext))
            })
        };

        let (kind, dest_rel) = if has_rel(&["ui", "ui_car.json"]) {
            (AssetKind::Car, PathBuf::from("content/cars").join(&dir))
        } else if has_rel(&["ui", "ui_track.json"]) || has_rel(&["ui", "*", "ui_track.json"]) {
            (AssetKind::Track, PathBuf::from("content/tracks").join(&dir))
        } else if has_rel(&["weather.ini"]) {
            (AssetKind::Weather, PathBuf::from("content/weather").join(&dir))
        } else if has_rel(&["manifest.ini"]) && has_ext_top("lua") {
            (AssetKind::LuaApp, PathBuf::from("apps/lua").join(&dir))
        } else if has_ext_top("py") {
            (AssetKind::PythonApp, PathBuf::from("apps/python").join(&dir))
        } else if has_rel(&["ui_skin.json"]) || has_ext_top("dds") {
            (
                AssetKind::CarSkin { target_car: None },
                PathBuf::from("content/cars/_unassigned/skins").join(&dir),
            )
        } else if has_ext_top("bank") {
            (
                AssetKind::Sound { target_car: None },
                PathBuf::from("content/cars/_unassigned/sfx"),
            )
        } else {
            continue;
        };

        let file_count = sub_entries.len();
        let total_size = sub_entries.iter().map(|e| e.size).sum();
        fallback_items.push(PlannedItem {
            kind,
            id: dir,
            archive_prefix: prefix,
            dest_rel,
            file_count,
            total_size,
            conflict: None,
            selected: true,
        });
    }

    if fallback_items.is_empty() {
        return Err(InstallError::NotContent);
    }
    Ok((fallback_items, skipped_root_files))
}

fn starts_with_ci(parts: &[String], prefix: &[String]) -> bool {
    parts.len() >= prefix.len()
        && parts
            .iter()
            .zip(prefix)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Installs all `selected` items from `plan` into `ac_root` with atomic staging and rollback.
///
/// - Refuses while `game_running` is true.
/// - Archives any existing conflicting folder/file into `<ac_root>/acxs_archive/<bucket>/<id>-<n>`.
/// - If any item fails mid-batch, rolls back every previously placed item in reverse order.
pub fn install_plan(
    ac_root: &Path,
    plan: &PackagePlan,
    game_running: bool,
    seven_zip_path: Option<&Path>,
) -> Result<BatchInstallReport, InstallError> {
    if game_running {
        return Err(InstallError::GameRunning);
    }
    if !ac_root.is_dir() {
        return Err(InstallError::AcRootMissing(ac_root.to_path_buf()));
    }

    let selected: Vec<&PlannedItem> = plan.items.iter().filter(|i| i.selected).collect();
    if selected.is_empty() {
        return Ok(BatchInstallReport::default());
    }

    let staging_parent = ac_root.join(STAGING_DIR);
    fs::create_dir_all(&staging_parent)?;
    let work = staging_parent.join(unique_name());
    fs::create_dir(&work)?;

    let extract_dir = work.join("extracted");
    let extract_res = extract_package_to(&plan.source, plan.format, &extract_dir, seven_zip_path);
    if let Err(e) = extract_res {
        let _ = fs::remove_dir_all(&work);
        let _ = fs::remove_dir(&staging_parent);
        return Err(e);
    }

    let mut installed = Vec::new();
    let mut tx_log: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();

    for (idx, item) in selected.iter().enumerate() {
        let target = ac_root.join(&item.dest_rel);
        let src = if plan.format == ArchiveFormat::SingleFile {
            plan.source.clone()
        } else {
            let mut p = extract_dir.clone();
            for seg in &item.archive_prefix {
                p.push(seg);
            }
            p
        };

        let step = place_item(ac_root, item, &src, &target, idx);
        match step {
            Ok(moved) => {
                tx_log.push((target.clone(), moved.clone()));
                installed.push(InstalledItemReport {
                    kind: item.kind.clone(),
                    id: item.id.clone(),
                    installed_to: target,
                    file_count: item.file_count,
                    moved_existing_to: moved,
                });
            }
            Err(e) => {
                // Reverse rollback of every item already placed in this batch.
                for (placed_target, archived_prev) in tx_log.into_iter().rev() {
                    if placed_target.is_dir() {
                        let _ = fs::remove_dir_all(&placed_target);
                    } else if placed_target.is_file() {
                        let _ = fs::remove_file(&placed_target);
                    }
                    if let Some(prev) = archived_prev {
                        let _ = fs::rename(prev, placed_target);
                    }
                }
                let _ = fs::remove_dir_all(&work);
                let _ = fs::remove_dir(&staging_parent);
                return Err(e);
            }
        }
    }

    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir(&staging_parent);
    Ok(BatchInstallReport { installed })
}

fn place_item(
    ac_root: &Path,
    item: &PlannedItem,
    src: &Path,
    target: &Path,
    _idx: usize,
) -> Result<Option<PathBuf>, InstallError> {
    if !src.exists() {
        return Err(InstallError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("staged source missing: {}", src.display()),
        )));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let moved = if fs::symlink_metadata(target).is_ok() {
        let archive_dir = ac_root.join(ARCHIVE_DIR).join(item.kind.archive_bucket());
        fs::create_dir_all(&archive_dir)?;
        let dest = free_archive_name(&archive_dir, &item.id);
        fs::rename(target, &dest)?;
        Some(dest)
    } else {
        None
    };

    let res = if src.is_dir() {
        fs::rename(src, target).or_else(|_| crate::util::copy_dir_all(src, target))
    } else {
        fs::copy(src, target).map(|_| ())
    };
    if let Err(e) = res {
        if let Some(prev) = &moved {
            let _ = fs::rename(prev, target);
        }
        return Err(e.into());
    }
    Ok(moved)
}

fn extract_package_to(
    source: &Path,
    format: ArchiveFormat,
    dest: &Path,
    seven_zip_path: Option<&Path>,
) -> Result<(), InstallError> {
    fs::create_dir_all(dest)?;
    match format {
        ArchiveFormat::SingleFile => Ok(()),
        ArchiveFormat::Folder => {
            let root_name = source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("package");
            crate::util::copy_dir_all(source, &dest.join(root_name))?;
            Ok(())
        }
        ArchiveFormat::Zip => {
            let mut archive = ZipArchive::new(File::open(source)?)?;
            let entries = read_entries(&mut archive)?;
            let mut written_bytes = 0u64;
            for (index, entry) in entries.iter().enumerate() {
                if entry.parts.is_empty() {
                    continue;
                }
                let mut target = dest.to_path_buf();
                for seg in &entry.parts {
                    target.push(seg);
                }
                if entry.is_dir {
                    fs::create_dir_all(&target)?;
                } else {
                    written_bytes = written_bytes.saturating_add(entry.size);
                    if written_bytes > MAX_UNCOMPRESSED_BYTES {
                        return Err(InstallError::ArchiveTooLarge {
                            bytes: written_bytes,
                            limit: MAX_UNCOMPRESSED_BYTES,
                        });
                    }
                    if let Some(p) = target.parent() {
                        fs::create_dir_all(p)?;
                    }
                    let mut file = archive.by_index(index)?;
                    let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&target)?;
                    io::copy(&mut file, &mut out)?;
                }
            }
            Ok(())
        }
        ArchiveFormat::SevenZ => {
            // Validate every entry path first.
            let _ = read_7z_entries(source)?;
            sevenz_rust2::decompress_file(source, dest)
                .map_err(|e| InstallError::Io(io::Error::new(io::ErrorKind::InvalidData, e.to_string())))?;
            Ok(())
        }
        ArchiveFormat::TarGz => {
            // Validate every entry path first.
            let _ = read_targz_entries(source)?;
            let gz = flate2::read::GzDecoder::new(File::open(source)?);
            let mut archive = tar::Archive::new(gz);
            for item in archive.entries()? {
                let mut entry = item?;
                let raw = entry.path()?.to_string_lossy().replace('\\', "/");
                let parts = safe_parts(&raw)?;
                if parts.is_empty() {
                    continue;
                }
                let mut target = dest.to_path_buf();
                for seg in &parts {
                    target.push(seg);
                }
                if entry.header().entry_type().is_dir() {
                    fs::create_dir_all(&target)?;
                } else {
                    if let Some(p) = target.parent() {
                        fs::create_dir_all(p)?;
                    }
                    let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&target)?;
                    io::copy(&mut entry, &mut out)?;
                }
            }
            Ok(())
        }
        ArchiveFormat::Rar => {
            let tool = find_rar_extractor(seven_zip_path)?;
            extract_rar_external(&tool, source, dest)?;
            // Post-extraction safety check.
            let _ = read_folder_entries(dest)?;
            Ok(())
        }
    }
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
        assert_eq!(info.total_size, 2 + 2);

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
        files.push(("extras/readme.txt", "hi"));
        make_zip(&zip, &files);

        assert!(matches!(inspect_archive(&zip), Err(InstallError::UnexpectedEntry(_))));
    }

    #[test]
    fn loose_root_files_are_skipped_and_case_is_ignored() {
        let dir = TempDir::new("loose");
        let ac = ac_root(&dir);
        let zip = dir.0.join("car.zip");
        make_zip(
            &zip,
            &[
                ("readme.txt", "hi"),
                ("Content/Cars/abc/UI/ui_car.json", "{}"),
                ("Content/Cars/abc/abc.kn5", "kn5"),
            ],
        );
        let report = install_archive(&ac, &zip, false).unwrap();
        assert_eq!(report.kind, ContentKind::Car);
        assert_eq!(report.file_count, 2);
        assert!(ac.join("content/cars/abc/abc.kn5").is_file());
        assert!(!ac.join("readme.txt").exists());

        let root_zip = dir.0.join("root.zip");
        make_zip(&root_zip, &[("license.txt", "x"), ("xyz/UI/UI_CAR.JSON", "{}")]);
        assert_eq!(inspect_archive(&root_zip).unwrap().kind, ContentKind::Car);
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

    #[test]
    fn classifies_and_installs_multi_item_zip_with_car_track_skin_ppfilter_lua_and_csp_config() {
        let dir = TempDir::new("multi-item");
        let ac = ac_root(&dir);
        let zip = dir.0.join("bundle.zip");
        make_zip(
            &zip,
            &[
                ("Pack_v1/content/cars/car_a/ui/ui_car.json", r#"{"name":"Car A"}"#),
                ("Pack_v1/content/cars/car_a/car_a.kn5", "kn5"),
                ("Pack_v1/content/tracks/trk_a/ui/ui_track.json", r#"{"name":"Trk A"}"#),
                ("Pack_v1/content/tracks/trk_a/trk_a.kn5", "kn5"),
                ("Pack_v1/content/cars/other_car/skins/01_red/ui_skin.json", "{}"),
                ("Pack_v1/content/cars/other_car/skins/01_red/body.dds", "dds"),
                ("Pack_v1/system/cfg/ppfilters/custom_pp.ini", "[TONEMAPPING]\nEXPOSURE=1\n"),
                ("Pack_v1/apps/lua/my_hud/manifest.ini", "[ABOUT]\nNAME=MyHUD\n"),
                ("Pack_v1/apps/lua/my_hud/my_hud.lua", "function script.update() end"),
                ("Pack_v1/extension/config/cars/loaded/car_a.ini", "[LIGHT_0]\nCOLOR=1,1,1\n"),
                ("Pack_v1/readme.txt", "hello"),
            ],
        );

        let plan = inspect_package(&zip, Some(&ac), None).unwrap();
        assert_eq!(plan.format, ArchiveFormat::Zip);
        assert_eq!(plan.items.len(), 6);
        assert!(plan.items.iter().any(|i| i.kind == AssetKind::Car && i.id == "car_a"));
        assert!(plan.items.iter().any(|i| i.kind == AssetKind::Track && i.id == "trk_a"));
        assert!(plan.items.iter().any(|i| matches!(i.kind, AssetKind::CarSkin { target_car: Some(ref c) } if c == "other_car")));
        assert!(plan.items.iter().any(|i| i.kind == AssetKind::PpFilter && i.id == "custom_pp.ini"));
        assert!(plan.items.iter().any(|i| i.kind == AssetKind::LuaApp && i.id == "my_hud"));
        assert!(plan.items.iter().any(|i| i.kind == AssetKind::CspConfig && i.id == "car_a.ini"));

        // Game-running guard.
        assert!(matches!(
            install_plan(&ac, &plan, true, None),
            Err(InstallError::GameRunning)
        ));

        let report = install_plan(&ac, &plan, false, None).unwrap();
        assert_eq!(report.installed.len(), 6);
        assert!(ac.join("content/cars/car_a/car_a.kn5").is_file());
        assert!(ac.join("content/tracks/trk_a/trk_a.kn5").is_file());
        assert!(ac.join("content/cars/other_car/skins/01_red/body.dds").is_file());
        assert!(ac.join("system/cfg/ppfilters/custom_pp.ini").is_file());
        assert!(ac.join("apps/lua/my_hud/my_hud.lua").is_file());
        assert!(ac.join("extension/config/cars/loaded/car_a.ini").is_file());
    }

    #[test]
    fn extracts_tar_gz_and_single_ppfilter_packages_cleanly() {
        let dir = TempDir::new("targz-and-ini");
        let ac = ac_root(&dir);

        // Build a .tar.gz with a car inside.
        let tgz_path = dir.0.join("car_mod.tar.gz");
        {
            let file = File::create(&tgz_path).unwrap();
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            let data = br#"{"name":"Tar Car"}"#;
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "content/cars/tar_car/ui/ui_car.json", &data[..]).unwrap();
            tar.finish().unwrap();
        }

        let plan = inspect_package(&tgz_path, Some(&ac), None).unwrap();
        assert_eq!(plan.format, ArchiveFormat::TarGz);
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].kind, AssetKind::Car);
        install_plan(&ac, &plan, false, None).unwrap();
        assert!(ac.join("content/cars/tar_car/ui/ui_car.json").is_file());

        // Single .ini PP filter drop.
        let pp_path = dir.0.join("natural.ini");
        fs::write(&pp_path, "[TONEMAPPING]\nEXPOSURE=1.0\n").unwrap();
        let pp_plan = inspect_package(&pp_path, Some(&ac), None).unwrap();
        assert_eq!(pp_plan.format, ArchiveFormat::SingleFile);
        assert_eq!(pp_plan.items[0].kind, AssetKind::PpFilter);
        install_plan(&ac, &pp_plan, false, None).unwrap();
        assert!(ac.join("system/cfg/ppfilters/natural.ini").is_file());
    }

    #[test]
    fn rolls_back_all_prior_items_when_second_item_fails_mid_install() {
        let dir = TempDir::new("batch-rollback");
        let ac = ac_root(&dir);
        let zip = dir.0.join("two.zip");
        make_zip(
            &zip,
            &[
                ("content/cars/first_car/ui/ui_car.json", r#"{"name":"First"}"#),
                ("content/cars/first_car/first_car.kn5", "new"),
            ],
        );

        // Pre-existing car that must be restored on rollback.
        fs::create_dir_all(ac.join("content/cars/first_car")).unwrap();
        fs::write(ac.join("content/cars/first_car/original.txt"), "keep-me").unwrap();

        let mut plan = inspect_package(&zip, Some(&ac), None).unwrap();
        // Inject a second item pointing to a missing archive prefix so step 2 fails.
        plan.items.push(PlannedItem {
            kind: AssetKind::Track,
            id: "broken_track".into(),
            archive_prefix: vec!["does_not_exist".into()],
            dest_rel: PathBuf::from("content/tracks/broken_track"),
            file_count: 1,
            total_size: 10,
            conflict: None,
            selected: true,
        });

        assert!(install_plan(&ac, &plan, false, None).is_err());
        // First car must be rolled back to its original state!
        assert_eq!(
            fs::read_to_string(ac.join("content/cars/first_car/original.txt")).unwrap(),
            "keep-me"
        );
        assert!(!ac.join("content/cars/first_car/first_car.kn5").exists());
    }
}
