//! Install, switch and remove CSP lights patches (`lights-patch-v*` folders).
//!
//! A lights patch folder holds a `dwrite.dll` proxy and an `extension/` tree.
//! Both go into the Assetto Corsa root. Nothing is deleted: whatever is replaced
//! or removed is moved into `acxs-patch-archive/` inside the AC root, where it can
//! be installed again later. A failed install rolls back to the previous state.
//!
//! Known patch versions are identified by the sha256 of their `dwrite.dll`.

use crate::util::copy_dir_all;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const ARCHIVE_DIR: &str = "acxs-patch-archive";
pub const DLL_NAME: &str = "dwrite.dll";
pub const EXTENSION_NAME: &str = "extension";
pub const UNRECOGNIZED: &str = "unrecognized";

/// sha256 of `dwrite.dll` -> patch version.
pub const KNOWN_PATCHES: &[(&str, &str)] = &[
    (
        "6546fdf7854213dcc87a0b2bcb68155bebcc0d7892d3becb5795f310cbf24c6e",
        "0.2.11",
    ),
    (
        "178e3fef2cc08ade04ba37624f277da07edb0c633b2f4af3d7695202c2e4c645",
        "0.3.0-preview675",
    ),
];

/// sha256 of `dwrite.dll` -> CSP numeric build number (fallback when `data_manifest.ini` is absent).
pub const KNOWN_PATCH_BUILDS: &[(&str, u32)] = &[
    (
        "6546fdf7854213dcc87a0b2bcb68155bebcc0d7892d3becb5795f310cbf24c6e",
        3334,
    ),
    (
        "178e3fef2cc08ade04ba37624f277da07edb0c633b2f4af3d7695202c2e4c645",
        3675,
    ),
];

fn known_build(sha256: &str) -> Option<u32> {
    KNOWN_PATCH_BUILDS
        .iter()
        .find(|(hash, _)| *hash == sha256)
        .map(|(_, build)| *build)
}

/// Reads the numeric CSP build from `<root>/extension/config/data_manifest.ini`
/// (`[VERSION]` `SHADERS_PATCH_BUILD`, `BUILD`, or `VERSION_ID`).
pub fn manifest_build(root: &Path) -> Option<u32> {
    let text = fs::read_to_string(root.join(EXTENSION_NAME).join("config").join("data_manifest.ini")).ok()?;
    let mut in_version = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        if let Some(sec) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_version = sec.trim().eq_ignore_ascii_case("VERSION");
            continue;
        }
        if !in_version {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            if (key.eq_ignore_ascii_case("SHADERS_PATCH_BUILD")
                || key.eq_ignore_ascii_case("BUILD")
                || key.eq_ignore_ascii_case("VERSION_ID"))
                && let Ok(build) = v.trim().parse::<u32>()
                && build > 0
            {
                return Some(build);
            }
        }
    }
    None
}

/// Returns the installed CSP build number in `ac_root`, or `None` when CSP is not installed
/// or its build number cannot be determined.
pub fn installed_build(ac_root: &Path) -> Option<u32> {
    let dll = ac_root.join(DLL_NAME);
    if !dll.is_file() {
        return None;
    }
    if let Some(build) = manifest_build(ac_root) {
        return Some(build);
    }
    let sha = sha256_file(&dll).ok()?;
    known_build(&sha)
}

/// Returns the numeric CSP build number of a discovered patch folder, if available.
pub fn patch_build(patch: &LightsPatch) -> Option<u32> {
    manifest_build(&patch.folder).or_else(|| known_build(&patch.sha256))
}

#[derive(Debug)]
pub enum CspError {
    GameRunning,
    NotAcRoot(PathBuf),
    InvalidPatch(PathBuf),
    Io(io::Error),
}

impl fmt::Display for CspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CspError::GameRunning => {
                write!(f, "Close Assetto Corsa before changing the CSP lights patch.")
            }
            CspError::NotAcRoot(p) => write!(f, "acs.exe not found in {}", p.display()),
            CspError::InvalidPatch(p) => {
                write!(f, "{} is not a complete lights-patch folder", p.display())
            }
            CspError::Io(e) => write!(f, "file operation failed: {e}"),
        }
    }
}

impl std::error::Error for CspError {}

impl From<io::Error> for CspError {
    fn from(e: io::Error) -> Self {
        CspError::Io(e)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LightsPatch {
    pub version: String,
    /// `0.2` or `0.3` (or `unknown`), used to group patches in the UI.
    pub family: String,
    pub folder: PathBuf,
    pub dll: PathBuf,
    pub extension: PathBuf,
    pub sha256: String,
    pub known: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallResult {
    pub version: String,
    pub archived_to: Option<PathBuf>,
    pub already_installed: bool,
}

/// Streamed sha256 of a file, as lowercase hex.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn known_version(sha256: &str) -> Option<&'static str> {
    KNOWN_PATCHES
        .iter()
        .find(|(hash, _)| *hash == sha256)
        .map(|(_, version)| *version)
}

/// `0.2` from `0.2.11`, `0.3` from `0.3.0-preview675`.
pub fn family_of(version: &str) -> String {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor))
            if !major.is_empty()
                && !minor.is_empty()
                && major.chars().all(|c| c.is_ascii_digit())
                && minor.chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("{major}.{minor}")
        }
        _ => "unknown".to_string(),
    }
}

/// Version from a folder name such as `lights-patch-v0.2.11` or an archive
/// name `lights-patch-v0.2.11-20260101-101010`.
pub fn folder_version(name: &str) -> Option<String> {
    let rest = name.strip_prefix("lights-patch-v")?;
    if rest.is_empty() {
        return None;
    }
    // Archive names end with "-YYYYMMDD-HHMMSS" (16 ASCII bytes).
    const STAMP_LEN: usize = "-YYYYMMDD-HHMMSS".len();
    let bytes = rest.as_bytes();
    if bytes.len() > STAMP_LEN {
        let split = bytes.len() - STAMP_LEN;
        let stamp = &bytes[split..];
        let ok = stamp[0] == b'-'
            && stamp[1..9].iter().all(u8::is_ascii_digit)
            && stamp[9] == b'-'
            && stamp[10..].iter().all(u8::is_ascii_digit);
        if ok {
            return Some(rest[..split].to_string());
        }
    }
    Some(rest.to_string())
}

/// Load a complete lights-patch folder, or None if it is not one.
pub fn load_patch(folder: &Path) -> io::Result<Option<LightsPatch>> {
    let Some(name) = folder.file_name().and_then(|n| n.to_str()) else {
        return Ok(None);
    };
    let Some(folder_ver) = folder_version(name) else {
        return Ok(None);
    };
    let dll = folder.join(DLL_NAME);
    let extension = folder.join(EXTENSION_NAME);
    if !dll.is_file() || !extension.is_dir() {
        return Ok(None);
    }
    let sha256 = sha256_file(&dll)?;
    let known = known_version(&sha256);
    let version = known.map(str::to_string).unwrap_or(folder_ver);
    Ok(Some(LightsPatch {
        family: family_of(&version),
        version,
        folder: folder.to_path_buf(),
        dll,
        extension,
        sha256,
        known: known.is_some(),
    }))
}

/// Find lights-patch folders directly inside each root (library folders or archives).
pub fn discover_patches(roots: &[PathBuf]) -> Vec<LightsPatch> {
    let mut found = Vec::new();
    for root in roots {
        let Ok(read) = fs::read_dir(root) else {
            continue;
        };
        for entry in read.flatten() {
            if let Ok(Some(patch)) = load_patch(&entry.path()) {
                found.push(patch);
            }
        }
    }
    found.sort_by(|a, b| (&a.family, &a.version, &a.folder).cmp(&(&b.family, &b.version, &b.folder)));
    found
}

pub fn archive_dir(ac_root: &Path) -> PathBuf {
    ac_root.join(ARCHIVE_DIR)
}

/// Version of the installed patch: what its own manifest says, else a known `dwrite.dll` by hash,
/// else `unrecognized`. `None` if there is no `dwrite.dll`. The manifest comes first because
/// hashing the 140 MB file slows every scan and only ever finds versions on our list.
pub fn installed_version(ac_root: &Path) -> io::Result<Option<String>> {
    let dll = ac_root.join(DLL_NAME);
    if !dll.is_file() {
        return Ok(None);
    }
    if let Some(version) = manifest_version(ac_root) {
        return Ok(Some(version));
    }
    let sha = sha256_file(&dll)?;
    Ok(Some(known_version(&sha).unwrap_or(UNRECOGNIZED).to_string()))
}

/// `[VERSION] SHADERS_PATCH` from `extension/config/data_manifest.ini`, the file CSP keeps for
/// launchers.
fn manifest_version(ac_root: &Path) -> Option<String> {
    let path = ac_root.join(EXTENSION_NAME).join("config").join("data_manifest.ini");
    let text = String::from_utf8_lossy(&fs::read(path).ok()?).into_owned();
    let ini = crate::ini::Ini::parse(&text);
    ini.get("VERSION", "SHADERS_PATCH").map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// Move the live `dwrite.dll` and `extension/` into a new archive folder.
/// Returns that folder, or None if neither exists.
fn archive_current(ac_root: &Path, label: &str) -> io::Result<Option<PathBuf>> {
    let names: Vec<&str> = [DLL_NAME, EXTENSION_NAME]
        .into_iter()
        .filter(|n| ac_root.join(n).exists())
        .collect();
    if names.is_empty() {
        return Ok(None);
    }
    fs::create_dir_all(archive_dir(ac_root))?;
    // Never reuse an archive folder: step forward a second until the name is free.
    let mut when = chrono::Local::now();
    let target = loop {
        let candidate = archive_dir(ac_root).join(format!(
            "lights-patch-v{label}-{}",
            when.format("%Y%m%d-%H%M%S")
        ));
        if !candidate.exists() {
            break candidate;
        }
        when += chrono::TimeDelta::seconds(1);
    };
    fs::create_dir(&target)?;
    let mut moved: Vec<&str> = Vec::new();
    for &name in &names {
        if let Err(e) = fs::rename(ac_root.join(name), target.join(name)) {
            for &done in &moved {
                let _ = fs::rename(target.join(done), ac_root.join(done));
            }
            let _ = fs::remove_dir(&target);
            return Err(e);
        }
        moved.push(name);
    }
    Ok(Some(target))
}

fn restore_from_archive(ac_root: &Path, archive: Option<&Path>) {
    let Some(archive) = archive else {
        return;
    };
    for name in [DLL_NAME, EXTENSION_NAME] {
        if archive.join(name).exists() {
            let _ = fs::rename(archive.join(name), ac_root.join(name));
        }
    }
}

/// Install `patch` into the AC root.
pub fn install_patch(
    ac_root: &Path,
    patch: &LightsPatch,
    game_running: bool,
) -> Result<InstallResult, CspError> {
    if game_running {
        return Err(CspError::GameRunning);
    }
    if !ac_root.join("acs.exe").is_file() {
        return Err(CspError::NotAcRoot(ac_root.to_path_buf()));
    }
    if !patch.dll.is_file() || !patch.extension.is_dir() {
        return Err(CspError::InvalidPatch(patch.folder.clone()));
    }

    let live_dll = ac_root.join(DLL_NAME);
    if live_dll.is_file()
        && ac_root.join(EXTENSION_NAME).is_dir()
        && sha256_file(&live_dll)? == patch.sha256
    {
        return Ok(InstallResult {
            version: patch.version.clone(),
            archived_to: None,
            already_installed: true,
        });
    }

    let previous = installed_version(ac_root)?.unwrap_or_else(|| "unknown".into());
    let label = if previous == UNRECOGNIZED { "unknown".to_string() } else { previous };
    let archive = archive_current(ac_root, &label)?;

    let copy = fs::copy(&patch.dll, ac_root.join(DLL_NAME))
        .map(|_| ())
        .and_then(|_| copy_dir_all(&patch.extension, &ac_root.join(EXTENSION_NAME)));
    if let Err(e) = copy {
        // Remove our partial copies, then put the originals back.
        let _ = fs::remove_file(ac_root.join(DLL_NAME));
        let _ = fs::remove_dir_all(ac_root.join(EXTENSION_NAME));
        restore_from_archive(ac_root, archive.as_deref());
        return Err(CspError::Io(e));
    }

    Ok(InstallResult {
        version: patch.version.clone(),
        archived_to: archive,
        already_installed: false,
    })
}

/// Archive the active lights patch. Returns the archive folder, or None if nothing was installed.
pub fn uninstall_patch(ac_root: &Path, game_running: bool) -> Result<Option<PathBuf>, CspError> {
    if game_running {
        return Err(CspError::GameRunning);
    }
    let label = match installed_version(ac_root)? {
        Some(v) if v != UNRECOGNIZED => v,
        _ => "unknown".to_string(),
    };
    Ok(archive_current(ac_root, &label)?)
}

/// Number of files and their total size that installing `patch` would place in the AC root:
/// the `dwrite.dll` plus everything under `extension/`. Only file metadata is read.
/// Unreadable entries are skipped. Symlinks are not followed.
fn install_footprint(patch: &LightsPatch) -> (u64, u64) {
    let mut count = 0u64;
    let mut bytes = 0u64;
    if let Ok(meta) = fs::metadata(&patch.dll) && meta.is_file() {
        count += 1;
        bytes += meta.len();
    }
    let mut pending = vec![patch.extension.clone()];
    while let Some(dir) = pending.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && let Ok(meta) = entry.metadata() {
                count += 1;
                bytes += meta.len();
            }
        }
    }
    (count, bytes)
}

/// Human-readable lines describing how patch `a` differs from patch `b`: the version,
/// the family, the installed file count and the total installed size. Lines appear in
/// that order and only for differences. Two patches that match give an empty list.
pub fn describe_patch_differences(a: &LightsPatch, b: &LightsPatch) -> Vec<String> {
    let mut lines = Vec::new();
    if a.version != b.version {
        lines.push(format!("version: {} -> {}", a.version, b.version));
    }
    if a.family != b.family {
        lines.push(format!("family: {} -> {}", a.family, b.family));
    }
    let (count_a, bytes_a) = install_footprint(a);
    let (count_b, bytes_b) = install_footprint(b);
    if count_a != count_b {
        let delta = count_b as i128 - count_a as i128;
        lines.push(format!("file count: {count_a} -> {count_b} ({delta:+})"));
    }
    if bytes_a != bytes_b {
        let delta = bytes_b as i128 - bytes_a as i128;
        lines.push(format!("total size: {bytes_a} -> {bytes_b} bytes ({delta:+})"));
    }
    lines
}

pub fn status_text(installed: &Option<String>) -> String {
    match installed.as_deref() {
        None => "No CSP lights patch installed.".to_string(),
        Some(UNRECOGNIZED) => "A dwrite.dll is installed but it is not a known lights patch.".to_string(),
        Some(version) => format!("CSP lights patch {version} is installed."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("acxs-csp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Build a fake lights-patch folder (not the real DLL) with the given dll bytes.
    fn fake_patch(root: &Path, name: &str, dll: &[u8]) -> PathBuf {
        let folder = root.join(name);
        fs::create_dir_all(folder.join("extension/config")).unwrap();
        fs::write(folder.join(DLL_NAME), dll).unwrap();
        fs::write(folder.join("extension/config/general.ini"), format!("[{name}]")).unwrap();
        folder
    }

    fn fake_ac_root(root: &Path) -> PathBuf {
        let ac = root.join("assettocorsa");
        fs::create_dir_all(&ac).unwrap();
        fs::write(ac.join("acs.exe"), b"exe").unwrap();
        ac
    }

    #[test]
    fn folder_version_strips_archive_stamp() {
        assert_eq!(folder_version("lights-patch-v0.2.11").as_deref(), Some("0.2.11"));
        assert_eq!(
            folder_version("lights-patch-v0.3.0-preview675").as_deref(),
            Some("0.3.0-preview675")
        );
        assert_eq!(
            folder_version("lights-patch-v0.2.11-20260101-101010").as_deref(),
            Some("0.2.11")
        );
        assert_eq!(folder_version("other-folder"), None);
    }

    #[test]
    fn families_group_by_major_minor() {
        assert_eq!(family_of("0.2.11"), "0.2");
        assert_eq!(family_of("0.3.0-preview675"), "0.3");
        assert_eq!(family_of("weird"), "unknown");
    }

    #[test]
    fn unknown_patch_takes_version_from_folder_name() {
        let root = temp("discover");
        fake_patch(&root, "lights-patch-v0.3.0-preview999", b"not-real");
        fake_patch(&root, "lights-patch-v0.2.5", b"also-fake");
        let found = discover_patches(std::slice::from_ref(&root));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].family, "0.2");
        assert!(found.iter().all(|p| !p.known));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn install_archives_previous_and_uninstall_keeps_files() {
        let root = temp("install");
        let ac = fake_ac_root(&root);
        let library = root.join("library");
        let v2 = fake_patch(&library, "lights-patch-v0.2.11", b"patch-two");
        let v3 = fake_patch(&library, "lights-patch-v0.3.0-preview675", b"patch-three");
        let p2 = load_patch(&v2).unwrap().unwrap();
        let p3 = load_patch(&v3).unwrap().unwrap();

        let first = install_patch(&ac, &p2, false).unwrap();
        assert!(first.archived_to.is_none());
        assert_eq!(fs::read(ac.join(DLL_NAME)).unwrap(), b"patch-two");

        let again = install_patch(&ac, &p2, false).unwrap();
        assert!(again.already_installed);

        let switch = install_patch(&ac, &p3, false).unwrap();
        let archive = switch.archived_to.expect("previous patch archived");
        assert!(archive.join(DLL_NAME).is_file());
        assert!(archive.join(EXTENSION_NAME).is_dir());
        assert_eq!(fs::read(ac.join(DLL_NAME)).unwrap(), b"patch-three");

        let archived = discover_patches(&[archive_dir(&ac)]);
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].version, "unknown");

        let removed = uninstall_patch(&ac, false).unwrap();
        assert!(removed.is_some());
        assert!(!ac.join(DLL_NAME).exists());
        assert!(!ac.join(EXTENSION_NAME).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_while_game_runs() {
        let root = temp("running");
        let ac = fake_ac_root(&root);
        let folder = fake_patch(&root.join("lib"), "lights-patch-v0.2.11", b"x");
        let patch = load_patch(&folder).unwrap().unwrap();
        assert!(matches!(install_patch(&ac, &patch, true), Err(CspError::GameRunning)));
        assert!(matches!(uninstall_patch(&ac, true), Err(CspError::GameRunning)));
        assert!(!ac.join(DLL_NAME).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn patch_differences_cover_version_family_count_and_size() {
        let root = temp("differences");
        let library = root.join("library");
        let v2 = fake_patch(&library, "lights-patch-v0.2.11", b"12345");
        let v3 = fake_patch(&library, "lights-patch-v0.3.0-preview675", b"1234567890");
        fs::write(v3.join("extension/config/extra.ini"), b"0123456789").unwrap();
        let p2 = load_patch(&v2).unwrap().unwrap();
        let p3 = load_patch(&v3).unwrap().unwrap();

        // Each folder's general.ini embeds its folder name, so the sizes differ.
        let general_a = fs::metadata(v2.join("extension/config/general.ini")).unwrap().len();
        let general_b = fs::metadata(v3.join("extension/config/general.ini")).unwrap().len();
        let bytes_a = 5 + general_a;
        let bytes_b = 10 + general_b + 10;
        assert_eq!(
            describe_patch_differences(&p2, &p3),
            [
                "version: 0.2.11 -> 0.3.0-preview675".to_string(),
                "family: 0.2 -> 0.3".to_string(),
                "file count: 2 -> 3 (+1)".to_string(),
                format!("total size: {bytes_a} -> {bytes_b} bytes ({:+})", bytes_b as i128 - bytes_a as i128),
            ]
        );
        // The comparison is symmetric in what it reports, with the sign flipped.
        let reverse = describe_patch_differences(&p3, &p2);
        assert_eq!(reverse[2], "file count: 3 -> 2 (-1)");

        assert!(describe_patch_differences(&p2, &p2).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_family_reports_only_version_when_sizes_match() {
        let root = temp("same-family");
        let library = root.join("library");
        // Same-length folder names and dll bytes give identical footprints.
        let a = fake_patch(&library, "lights-patch-v0.2.11", b"aaaa");
        let b = fake_patch(&library, "lights-patch-v0.2.12", b"bbbb");
        let pa = load_patch(&a).unwrap().unwrap();
        let pb = load_patch(&b).unwrap().unwrap();
        assert_eq!(describe_patch_differences(&pa, &pb), ["version: 0.2.11 -> 0.2.12"]);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn the_patch_names_its_own_version_and_an_unknown_dll_stays_unrecognized() {
        let root = std::env::temp_dir().join(format!("acxs-csp-manifest-{}", std::process::id()));
        fs::create_dir_all(root.join("extension").join("config")).unwrap();
        fs::write(root.join(DLL_NAME), b"not a known build").unwrap();
        assert_eq!(installed_version(&root).unwrap().as_deref(), Some(UNRECOGNIZED));
        let manifest = "; comment\n[\u{2139}]\nNAME=x\n\n[VERSION]\nSHADERS_PATCH=0.3.0-preview649\nSHADERS_PATCH_BUILD=4184\n";
        fs::write(root.join("extension").join("config").join("data_manifest.ini"), manifest).unwrap();
        assert_eq!(installed_version(&root).unwrap().as_deref(), Some("0.3.0-preview649"));
        assert_eq!(installed_build(&root), Some(4184));
        fs::remove_file(root.join(DLL_NAME)).unwrap();
        assert_eq!(installed_version(&root).unwrap(), None);
        assert_eq!(installed_build(&root), None);
        fs::remove_dir_all(&root).ok();
    }
}
