//! Snapshot and restore of the user's Assetto Corsa configuration.
//!
//! A snapshot is a plain folder copy under `<AC root>/acxs-backups/<stamp>/`
//! holding `cfg/`, `extension/config/` and any extra directories the caller
//! passes in (e.g. the setups folder), plus a `manifest.txt` listing every file
//! with its size and SHA-256. Restore verifies the manifest first, then moves
//! every file it is about to overwrite into `acxs-backups/pre-restore-<stamp>/`.
//!
//! Manifest format (tab separated, `#` lines are headers):
//! `<sha256>\t<size>\t<path inside snapshot>\t<target>`
//! where `<target>` is relative to the AC root, or `abs:<absolute path>` for
//! extra directories that live outside the AC root.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::csp::sha256_file;

const BACKUP_DIR: &str = "acxs-backups";
const MANIFEST: &str = "manifest.txt";
const PRE_RESTORE_PREFIX: &str = "pre-restore-";
const ABS_PREFIX: &str = "abs:";
const SCOPED_DIRS: [&str; 2] = ["cfg", "extension/config"];

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub name: String,
    pub path: PathBuf,
    pub created: String,
    pub files: usize,
}

#[derive(Debug)]
pub enum BackupError {
    GameRunning,
    Io(io::Error),
    Corrupt(Vec<String>),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackupError::GameRunning => {
                write!(f, "Assetto Corsa is running; close it before restoring a backup")
            }
            BackupError::Io(e) => write!(f, "backup I/O error: {e}"),
            BackupError::Corrupt(problems) => write!(
                f,
                "snapshot failed verification ({} problem(s)): {}",
                problems.len(),
                problems.join("; ")
            ),
        }
    }
}

impl Error for BackupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            BackupError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for BackupError {
    fn from(e: io::Error) -> Self {
        BackupError::Io(e)
    }
}

/// One validated manifest line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    sha256: String,
    size: u64,
    /// `/`-separated path relative to the snapshot folder.
    snap_rel: String,
    /// Raw target string from the manifest (`rel` or `abs:...`).
    target: String,
}

/// Where a manifest entry goes back to.
enum Target {
    /// Relative to the AC root.
    Inside(PathBuf),
    /// Absolute path outside the AC root.
    Outside(PathBuf),
}

impl Target {
    fn parse(raw: &str) -> Option<Target> {
        match raw.strip_prefix(ABS_PREFIX) {
            Some(abs) => {
                let path = PathBuf::from(abs);
                let ok = path.is_absolute()
                    && path
                        .components()
                        .all(|c| !matches!(c, Component::ParentDir | Component::CurDir));
                ok.then_some(Target::Outside(path))
            }
            None => safe_rel(raw).map(Target::Inside),
        }
    }

    fn destination(&self, ac_root: &Path) -> PathBuf {
        match self {
            Target::Inside(rel) => ac_root.join(rel),
            Target::Outside(abs) => abs.clone(),
        }
    }

    /// Location of the displaced file inside a pre-restore archive.
    fn archive_path(&self, archive: &Path) -> PathBuf {
        match self {
            Target::Inside(rel) => archive.join(rel),
            Target::Outside(abs) => {
                let mut out = archive.join("external");
                for c in abs.components() {
                    if let Component::Normal(part) = c {
                        out.push(part);
                    }
                }
                out
            }
        }
    }
}

/// Converts a `/`-separated relative path into a `PathBuf`, rejecting anything
/// that could escape its base directory.
fn safe_rel(s: &str) -> Option<PathBuf> {
    if s.is_empty() || s.contains('\\') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in s.split('/') {
        if part.is_empty() {
            return None;
        }
        out.push(part);
    }
    out.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(out)
}

/// Renders a relative path with `/` separators.
fn manifest_path(rel: &Path) -> io::Result<String> {
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 file name"))?
                    .to_string(),
            ),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected path component",
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn manifest_line(sha256: &str, size: u64, snap_rel: &str, target: &str) -> io::Result<String> {
    if [snap_rel, target].iter().any(|s| s.contains(['\t', '\n', '\r'])) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("path contains a tab or newline: {snap_rel}"),
        ));
    }
    Ok(format!("{sha256}\t{size}\t{snap_rel}\t{target}"))
}

fn parse_entry(line: &str) -> Option<Entry> {
    let mut parts = line.splitn(4, '\t');
    let sha256 = parts.next()?;
    let size = parts.next()?.parse::<u64>().ok()?;
    let snap_rel = parts.next()?;
    let target = parts.next()?;
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    safe_rel(snap_rel)?;
    Target::parse(target)?;
    Some(Entry {
        sha256: sha256.to_string(),
        size,
        snap_rel: snap_rel.to_string(),
        target: target.to_string(),
    })
}

/// Returns the `created` header (if any) and one result per data line.
fn parse_manifest(text: &str) -> (Option<String>, Vec<Result<Entry, String>>) {
    let mut created = None;
    let mut entries = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if let Some(header) = line.strip_prefix('#') {
            if let Some(value) = header.trim().strip_prefix("created:") {
                created = Some(value.trim().to_string());
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        entries.push(
            parse_entry(line).ok_or_else(|| format!("malformed or unsafe manifest line {}", idx + 1)),
        );
    }
    (created, entries)
}

fn canonical_or(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Creates `parent/<base>` or, if taken, `parent/<base>-2`, `-3`, ...
/// Never reuses an existing folder.
fn create_unique_dir(parent: &Path, base: &str) -> io::Result<(String, PathBuf)> {
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base}-{n}")
        };
        let path = parent.join(&name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok((name, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Collects every regular file under `base` into `out`, keyed by snapshot path.
/// Symlinks are skipped, and the backup folder itself is never included.
fn gather(
    base: &Path,
    root: &Path,
    backups: &Path,
    external_index: Option<usize>,
    out: &mut BTreeMap<String, (PathBuf, String)>,
) -> io::Result<()> {
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() || path.starts_with(backups) {
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let (snap_rel, target) = match path.strip_prefix(root) {
                Ok(rel) => {
                    let rel = manifest_path(rel)?;
                    (rel.clone(), rel)
                }
                Err(_) => {
                    let index = external_index.ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "file outside AC root")
                    })?;
                    let rel = manifest_path(path.strip_prefix(base).unwrap_or(&path))?;
                    let abs = path
                        .to_str()
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 path"))?;
                    (format!("external/{index}/{rel}"), format!("{ABS_PREFIX}{abs}"))
                }
            };
            out.entry(snap_rel).or_insert((path, target));
        }
    }
    Ok(())
}

/// Copies the gathered files into `dir` and writes the manifest.
fn write_snapshot(
    dir: &Path,
    created: &str,
    sources: &BTreeMap<String, (PathBuf, String)>,
) -> io::Result<usize> {
    let mut lines = Vec::with_capacity(sources.len());
    for (snap_rel, (src, target)) in sources {
        let dst = dir.join(snap_rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, &dst)?;
        let sha256 = sha256_file(&dst)?;
        let size = fs::metadata(&dst)?.len();
        lines.push(manifest_line(&sha256, size, snap_rel, target)?);
    }
    let mut text = format!("# acxs backup manifest\n# created: {created}\n");
    for line in &lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(dir.join(MANIFEST), text)?;
    Ok(lines.len())
}

/// Copies `cfg/`, `extension/config/` and `extra_dirs` into a new snapshot folder
/// under `<ac_root>/acxs-backups/`. Missing scoped folders are skipped.
pub fn create_snapshot(ac_root: &Path, extra_dirs: &[PathBuf]) -> io::Result<Snapshot> {
    let root = canonical_or(ac_root);
    let backups = root.join(BACKUP_DIR);
    fs::create_dir_all(&backups)?;

    let mut sources = BTreeMap::new();
    for scoped in SCOPED_DIRS {
        let base = root.join(scoped);
        if base.is_dir() {
            gather(&base, &root, &backups, None, &mut sources)?;
        }
    }
    for (index, extra) in extra_dirs.iter().enumerate() {
        if extra.is_dir() {
            let base = canonical_or(extra);
            gather(&base, &root, &backups, Some(index), &mut sources)?;
        }
    }

    let now = chrono::Local::now();
    let created = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let (name, dir) = create_unique_dir(&backups, &now.format("%Y%m%d-%H%M%S").to_string())?;
    match write_snapshot(&dir, &created, &sources) {
        Ok(files) => Ok(Snapshot {
            name,
            path: dir,
            created,
            files,
        }),
        Err(e) => {
            // Remove only the partial folder this call just created.
            let _ = fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// Lists snapshots under `<ac_root>/acxs-backups/`, newest first. Folders without
/// a readable manifest (including `pre-restore-*` archives) are ignored.
pub fn list_snapshots(ac_root: &Path) -> Vec<Snapshot> {
    let Ok(read) = fs::read_dir(ac_root.join(BACKUP_DIR)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !path.is_dir() || name.starts_with(PRE_RESTORE_PREFIX) {
            continue;
        }
        let Ok(text) = fs::read_to_string(path.join(MANIFEST)) else {
            continue;
        };
        let (created, entries) = parse_manifest(&text);
        let files = entries.iter().filter(|e| e.is_ok()).count();
        out.push(Snapshot {
            created: created.unwrap_or_else(|| name.clone()),
            name,
            path,
            files,
        });
    }
    out.sort_by(|a, b| {
        b.created
            .cmp(&a.created)
            .then_with(|| b.name.cmp(&a.name))
    });
    out
}

fn check_entry(snapshot_path: &Path, entry: &Entry) -> Option<String> {
    let rel = &entry.snap_rel;
    let path = snapshot_path.join(rel);
    let meta = match fs::metadata(&path) {
        Ok(m) if m.is_file() => m,
        _ => return Some(format!("{rel}: missing")),
    };
    if meta.len() != entry.size {
        return Some(format!(
            "{rel}: size mismatch (expected {}, found {})",
            entry.size,
            meta.len()
        ));
    }
    match sha256_file(&path) {
        Ok(hash) if hash.eq_ignore_ascii_case(&entry.sha256) => None,
        Ok(_) => Some(format!("{rel}: sha256 mismatch")),
        Err(e) => Some(format!("{rel}: unreadable ({e})")),
    }
}

/// Re-hashes every file listed in the manifest. Returns the mismatched, missing
/// or malformed entries; an empty list means the snapshot is intact. Errors only
/// when the manifest itself cannot be read.
pub fn verify_snapshot(snapshot_path: &Path) -> io::Result<Vec<String>> {
    let text = fs::read_to_string(snapshot_path.join(MANIFEST))?;
    let (_, entries) = parse_manifest(&text);
    let mut problems = Vec::new();
    for entry in entries {
        match entry {
            Err(problem) => problems.push(problem),
            Ok(entry) => problems.extend(check_entry(snapshot_path, &entry)),
        }
    }
    Ok(problems)
}

/// Moves `from` to `to`, falling back to copy-then-remove across filesystems.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    fs::remove_file(from)
}

/// Restores a snapshot into `ac_root`. Refuses while the game runs and when the
/// snapshot fails verification. Any existing file that would be overwritten is
/// first moved (never deleted) into `acxs-backups/pre-restore-<stamp>/`.
/// Returns the number of files restored.
pub fn restore_snapshot(
    ac_root: &Path,
    snapshot_path: &Path,
    game_running: bool,
) -> Result<usize, BackupError> {
    if game_running {
        return Err(BackupError::GameRunning);
    }
    let problems = verify_snapshot(snapshot_path)?;
    if !problems.is_empty() {
        return Err(BackupError::Corrupt(problems));
    }
    let text = fs::read_to_string(snapshot_path.join(MANIFEST))?;
    let (_, parsed) = parse_manifest(&text);
    let entries: Vec<Entry> = parsed.into_iter().filter_map(Result::ok).collect();

    let backups = ac_root.join(BACKUP_DIR);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut archive: Option<PathBuf> = None;
    let mut restored = 0;

    for entry in &entries {
        let Some(target) = Target::parse(&entry.target) else {
            return Err(BackupError::Corrupt(vec![format!(
                "{}: invalid target",
                entry.snap_rel
            )]));
        };
        let dest = target.destination(ac_root);
        let src = snapshot_path.join(&entry.snap_rel);

        match fs::symlink_metadata(&dest) {
            Ok(meta) if meta.is_dir() => {
                return Err(BackupError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("restore target is a directory: {}", dest.display()),
                )));
            }
            Ok(_) => {
                let dir = match &archive {
                    Some(dir) => dir.clone(),
                    None => {
                        fs::create_dir_all(&backups)?;
                        let (_, dir) = create_unique_dir(
                            &backups,
                            &format!("{PRE_RESTORE_PREFIX}{stamp}"),
                        )?;
                        archive = Some(dir.clone());
                        dir
                    }
                };
                move_file(&dest, &target.archive_path(&dir))?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&src, &dest)?;
        restored += 1;
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp directory that is removed on drop, even when a test panics.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "acxs-backup-test-{}-{}",
                std::process::id(),
                label
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempRoot(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    const VIDEO: &str = "[VIDEO]\nFULLSCREEN=1\n";

    fn seed(ac: &Path) {
        write(&ac.join("cfg/video.ini"), VIDEO);
        write(&ac.join("cfg/controls.ini"), "[KEYS]\n");
        write(&ac.join("extension/config/csp.ini"), "[CSP]\n");
        write(&ac.join("content/cars/x/data.txt"), "out of scope");
    }

    fn pre_restore_dirs(ac: &Path) -> Vec<PathBuf> {
        fs::read_dir(ac.join(BACKUP_DIR))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(PRE_RESTORE_PREFIX)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn create_copies_scope_and_writes_manifest() {
        let t = TempRoot::new("create");
        let ac = t.0.join("ac");
        seed(&ac);
        write(&ac.join("setups/car/a.ini"), "[SETUP]\n");

        let snap = create_snapshot(&ac, &[ac.join("setups")]).unwrap();
        assert_eq!(snap.files, 4);
        assert!(snap.path.starts_with(fs::canonicalize(&ac).unwrap().join(BACKUP_DIR)));
        assert_eq!(read(&snap.path.join("cfg/video.ini")), VIDEO);
        assert_eq!(read(&snap.path.join("setups/car/a.ini")), "[SETUP]\n");
        assert!(!snap.path.join("content").exists());

        let manifest = read(&snap.path.join(MANIFEST));
        assert_eq!(manifest.lines().filter(|l| !l.starts_with('#')).count(), 4);
        assert!(manifest.contains("\tcfg/video.ini\tcfg/video.ini"));
    }

    #[test]
    fn create_never_overwrites_existing_snapshot() {
        let t = TempRoot::new("collide");
        let ac = t.0.join("ac");
        seed(&ac);
        let a = create_snapshot(&ac, &[]).unwrap();
        let b = create_snapshot(&ac, &[]).unwrap();
        assert_ne!(a.path, b.path);
        assert!(a.path.join(MANIFEST).is_file());
        assert!(b.path.join(MANIFEST).is_file());
    }

    #[test]
    fn list_is_newest_first_and_tolerates_missing_dirs() {
        let t = TempRoot::new("list");
        let ac = t.0.join("ac");
        assert!(list_snapshots(&ac).is_empty());

        seed(&ac);
        let _first = create_snapshot(&ac, &[]).unwrap();
        let second = create_snapshot(&ac, &[]).unwrap();
        let listed = list_snapshots(&ac);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, second.name);
        assert_eq!(listed[0].files, 3);
    }

    #[test]
    fn verify_good_snapshot_is_clean() {
        let t = TempRoot::new("verify-good");
        let ac = t.0.join("ac");
        seed(&ac);
        let snap = create_snapshot(&ac, &[]).unwrap();
        assert!(verify_snapshot(&snap.path).unwrap().is_empty());
    }

    #[test]
    fn verify_detects_tampered_and_missing_files() {
        let t = TempRoot::new("verify-bad");
        let ac = t.0.join("ac");
        seed(&ac);
        let snap = create_snapshot(&ac, &[]).unwrap();

        write(&snap.path.join("cfg/video.ini"), "tampered");
        fs::remove_file(snap.path.join("cfg/controls.ini")).unwrap();

        let problems = verify_snapshot(&snap.path).unwrap();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("cfg/video.ini")));
        assert!(problems.iter().any(|p| p.starts_with("cfg/controls.ini")));
    }

    #[test]
    fn restore_moves_overwritten_files_to_pre_restore() {
        let t = TempRoot::new("restore");
        let ac = t.0.join("ac");
        seed(&ac);
        let snap = create_snapshot(&ac, &[]).unwrap();

        write(&ac.join("cfg/video.ini"), "CHANGED");
        let restored = restore_snapshot(&ac, &snap.path, false).unwrap();
        assert_eq!(restored, 3);
        assert_eq!(read(&ac.join("cfg/video.ini")), VIDEO);

        let archives = pre_restore_dirs(&ac);
        assert_eq!(archives.len(), 1);
        assert_eq!(read(&archives[0].join("cfg/video.ini")), "CHANGED");
        // The snapshot itself is never consumed.
        assert!(verify_snapshot(&snap.path).unwrap().is_empty());
    }

    #[test]
    fn restore_refuses_while_game_running() {
        let t = TempRoot::new("running");
        let ac = t.0.join("ac");
        seed(&ac);
        let snap = create_snapshot(&ac, &[]).unwrap();

        write(&ac.join("cfg/video.ini"), "CHANGED");
        let err = restore_snapshot(&ac, &snap.path, true).unwrap_err();
        assert!(matches!(err, BackupError::GameRunning));
        assert_eq!(read(&ac.join("cfg/video.ini")), "CHANGED");
        assert!(pre_restore_dirs(&ac).is_empty());
    }

    #[test]
    fn restore_refuses_corrupt_snapshot() {
        let t = TempRoot::new("corrupt");
        let ac = t.0.join("ac");
        seed(&ac);
        let snap = create_snapshot(&ac, &[]).unwrap();

        write(&snap.path.join("cfg/video.ini"), "tampered");
        write(&ac.join("cfg/video.ini"), "CHANGED");
        match restore_snapshot(&ac, &snap.path, false) {
            Err(BackupError::Corrupt(problems)) => assert!(!problems.is_empty()),
            other => panic!("expected Corrupt, got {other:?}"),
        }
        assert_eq!(read(&ac.join("cfg/video.ini")), "CHANGED");
        assert!(pre_restore_dirs(&ac).is_empty());
    }

    #[test]
    fn external_extra_dir_round_trips() {
        let t = TempRoot::new("external");
        let ac = t.0.join("ac");
        let ext = t.0.join("external-setups");
        seed(&ac);
        write(&ext.join("a.ini"), "orig");

        let snap = create_snapshot(&ac, std::slice::from_ref(&ext)).unwrap();
        assert_eq!(snap.files, 4);
        assert!(read(&snap.path.join(MANIFEST)).contains("\tabs:"));

        write(&ext.join("a.ini"), "changed");
        assert_eq!(restore_snapshot(&ac, &snap.path, false).unwrap(), 4);
        assert_eq!(read(&ext.join("a.ini")), "orig");

        // The displaced file lands under pre-restore/external/<absolute path>.
        let mut displaced = pre_restore_dirs(&ac)[0].join("external");
        for c in fs::canonicalize(&ext).unwrap().components() {
            if let Component::Normal(part) = c {
                displaced.push(part);
            }
        }
        assert_eq!(read(&displaced.join("a.ini")), "changed");
    }

    #[test]
    fn temp_root_is_removed_after_use() {
        let path;
        {
            let t = TempRoot::new("cleanup");
            path = t.0.clone();
            assert!(path.exists());
        }
        assert!(!path.exists());
    }
}
