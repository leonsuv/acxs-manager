//! Opt-in online CSP lights-patch catalog, verified downloader, and GitHub
//! `acc-extension-config` car/track/VAO config synchronization.
//!
//! Nothing here runs automatically. All network calls go through `ureq` with TLS
//! certificate verification (SChannel on Windows, `rustls` elsewhere). Downloads
//! verify SHA-256 digests when supplied, extract safely via staging, and never
//! overwrite user-edited configs without creating a `.acxs-backup-<stamp>` copy.

use chrono::Local;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Default opt-in catalog endpoint. Overridable via `Settings::csp_catalog_url`.
pub const DEFAULT_CATALOG_URL: &str = "https://acstuff.ru/patch/?get=list";

/// Sync index file inside `<ac_root>/extension/config/`.
pub const SYNC_INDEX_FILE: &str = "acxs-config-sync.json";

/// Maximum catalog payload size (256 KiB).
const MAX_CATALOG_BYTES: usize = 256 * 1024;
/// Maximum single CSP zip download size (512 MiB).
const MAX_PATCH_BYTES: u64 = 512 * 1024 * 1024;
/// Maximum single config/VAO file size (32 MiB).
const MAX_CONFIG_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CspRelease {
    pub version: String,
    #[serde(default)]
    pub build_id: u32,
    pub url: String,
    #[serde(default)]
    pub changelog: String,
    #[serde(default)]
    pub recommended: bool,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CspCatalog {
    #[serde(default)]
    pub etag: Option<String>,
    pub releases: Vec<CspRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSyncEntry {
    /// Relative path under `<ac_root>/extension/` (e.g. `config/cars/loaded/ks_ferrari.ini`).
    pub rel_path: String,
    pub sha256: String,
    #[serde(default)]
    pub source_commit: String,
    #[serde(default)]
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSyncIndex {
    pub entries: BTreeMap<String, ConfigSyncEntry>,
}

/// One remote config or VAO patch offered by a manifest/repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteConfigItem {
    /// Relative path under `<ac_root>/extension/`, e.g. `config/cars/loaded/ks_audi.ini`
    /// or `vao-patches-tracks/spa.vao-patch`.
    pub rel_path: String,
    /// Download URL (or local fixture path in tests).
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub source_commit: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigSyncReport {
    pub written: Vec<PathBuf>,
    pub backed_up: Vec<PathBuf>,
    pub skipped_up_to_date: usize,
    pub skipped_uninstalled: usize,
}

#[derive(Debug)]
pub enum CspOnlineError {
    GameRunning,
    InvalidUrl(String),
    UnsafePath(String),
    Network(String),
    BadFormat(String),
    Sha256Mismatch { expected: String, actual: String },
    TooLarge { limit: u64 },
    Io(io::Error),
}

impl fmt::Display for CspOnlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CspOnlineError::GameRunning => {
                write!(f, "Close Assetto Corsa before installing or syncing CSP files.")
            }
            CspOnlineError::InvalidUrl(u) => write!(f, "invalid or non-HTTPS URL: {u}"),
            CspOnlineError::UnsafePath(p) => write!(f, "unsafe relative path: {p:?}"),
            CspOnlineError::Network(e) => write!(f, "network error: {e}"),
            CspOnlineError::BadFormat(e) => write!(f, "invalid catalog format: {e}"),
            CspOnlineError::Sha256Mismatch { expected, actual } => {
                write!(f, "SHA-256 mismatch: expected {expected}, got {actual}")
            }
            CspOnlineError::TooLarge { limit } => {
                write!(f, "download exceeded safety limit of {limit} bytes")
            }
            CspOnlineError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for CspOnlineError {}

impl From<io::Error> for CspOnlineError {
    fn from(e: io::Error) -> Self {
        CspOnlineError::Io(e)
    }
}

/// Computes lowercase hex SHA-256 of bytes in memory.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Verifies `bytes` against an expected hex SHA-256 digest (case-insensitive).
pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), CspOnlineError> {
    let actual = sha256_bytes(bytes);
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(CspOnlineError::Sha256Mismatch { expected, actual });
    }
    Ok(())
}

/// Parses a CSP catalog JSON payload. Accepts either `{"releases": [...]}` or a bare JSON array `[...]`.
pub fn parse_catalog_json(text: &str) -> Result<CspCatalog, CspOnlineError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if trimmed.starts_with('[') {
        let releases: Vec<CspRelease> = serde_json::from_str(trimmed)
            .map_err(|e| CspOnlineError::BadFormat(e.to_string()))?;
        return Ok(CspCatalog {
            etag: None,
            releases,
        });
    }
    serde_json::from_str::<CspCatalog>(trimmed)
        .map_err(|e| CspOnlineError::BadFormat(e.to_string()))
}

fn require_https(url: &str) -> Result<(), CspOnlineError> {
    if !url.starts_with("https://") {
        return Err(CspOnlineError::InvalidUrl(url.to_string()));
    }
    Ok(())
}

/// Fetches the CSP release catalog over HTTPS, honoring an optional `cached` ETag.
pub fn fetch_catalog(
    url: &str,
    cached: Option<&CspCatalog>,
) -> Result<CspCatalog, CspOnlineError> {
    require_https(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .build();
    let mut req = agent.get(url);
    if let Some(etag) = cached.and_then(|c| c.etag.as_deref()) {
        req = req.set("If-None-Match", etag);
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(304, _)) => {
            if let Some(c) = cached {
                return Ok(c.clone());
            }
            return Err(CspOnlineError::Network("HTTP 304 without cached catalog".into()));
        }
        Err(e) => return Err(CspOnlineError::Network(e.to_string())),
    };
    let etag = resp.header("ETag").map(str::to_string);
    let mut body = Vec::new();
    resp.into_reader()
        .take((MAX_CATALOG_BYTES + 1) as u64)
        .read_to_end(&mut body)?;
    if body.len() > MAX_CATALOG_BYTES {
        return Err(CspOnlineError::TooLarge {
            limit: MAX_CATALOG_BYTES as u64,
        });
    }
    let text = String::from_utf8(body).map_err(|e| CspOnlineError::BadFormat(e.to_string()))?;
    let mut catalog = parse_catalog_json(&text)?;
    catalog.etag = etag;
    Ok(catalog)
}

/// Downloads a CSP release zip over HTTPS, verifies its SHA-256 if provided, and extracts it
/// into `<patch_library>/lights-patch-v<version>/`.
pub fn download_and_extract_patch(
    release: &CspRelease,
    patch_library: &Path,
    game_running: bool,
) -> Result<PathBuf, CspOnlineError> {
    if game_running {
        return Err(CspOnlineError::GameRunning);
    }
    require_https(&release.url)?;
    if !is_safe_version(&release.version) {
        return Err(CspOnlineError::UnsafePath(release.version.clone()));
    }

    fs::create_dir_all(patch_library)?;
    let part_path = patch_library.join(format!("lights-patch-v{}.zip.part", release.version));

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .build();
    let resp = agent
        .get(&release.url)
        .call()
        .map_err(|e| CspOnlineError::Network(e.to_string()))?;

    let mut reader = resp.into_reader().take(MAX_PATCH_BYTES + 1);
    let mut out = File::create(&part_path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_PATCH_BYTES {
            drop(out);
            let _ = fs::remove_file(&part_path);
            return Err(CspOnlineError::TooLarge {
                limit: MAX_PATCH_BYTES,
            });
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    drop(out);

    let actual_sha: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if let Some(expected) = &release.sha256 {
        let exp = expected.trim().to_ascii_lowercase();
        if actual_sha != exp {
            let _ = fs::remove_file(&part_path);
            return Err(CspOnlineError::Sha256Mismatch {
                expected: exp,
                actual: actual_sha,
            });
        }
    }

    let dest_dir = patch_library.join(format!("lights-patch-v{}", release.version));
    let extract_res = extract_patch_zip(&part_path, &dest_dir);
    let _ = fs::remove_file(&part_path);
    extract_res?;
    Ok(dest_dir)
}

fn extract_patch_zip(zip_path: &Path, dest_dir: &Path) -> Result<(), CspOnlineError> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)
        .map_err(|e| CspOnlineError::BadFormat(e.to_string()))?;
    let staging = dest_dir.with_extension(format!("staging-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| CspOnlineError::BadFormat(e.to_string()))?;
        let name = file
            .name()
            .map_err(|e| CspOnlineError::BadFormat(e.to_string()))?
            .into_owned();
        if file.is_symlink() {
            let _ = fs::remove_dir_all(&staging);
            return Err(CspOnlineError::UnsafePath(name));
        }
        let rel = validate_rel_path(&name)?;
        let target = staging.join(&rel);
        if file.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(p) = target.parent() {
                fs::create_dir_all(p)?;
            }
            let mut out = File::create(&target)?;
            io::copy(&mut file, &mut out)?;
        }
    }

    if dest_dir.exists() {
        let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
        let backup = dest_dir.with_file_name(format!(
            "{}.acxs-backup-{stamp}",
            dest_dir.file_name().unwrap_or_default().to_string_lossy()
        ));
        fs::rename(dest_dir, &backup)?;
    }
    fs::rename(&staging, dest_dir)?;
    Ok(())
}

/// Loads `<ac_root>/extension/config/acxs-config-sync.json`, returning empty on missing file.
pub fn load_sync_index(ac_root: &Path) -> ConfigSyncIndex {
    let path = ac_root.join("extension").join("config").join(SYNC_INDEX_FILE);
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Saves `<ac_root>/extension/config/acxs-config-sync.json` atomically.
pub fn save_sync_index(ac_root: &Path, index: &ConfigSyncIndex) -> io::Result<()> {
    let dir = ac_root.join("extension").join("config");
    fs::create_dir_all(&dir)?;
    let path = dir.join(SYNC_INDEX_FILE);
    let tmp = dir.join(format!("{SYNC_INDEX_FILE}.tmp"));
    let text = serde_json::to_string_pretty(index).map_err(io::Error::other)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &path)
}

/// Filters `remote_items` down to configs and VAO patches matching installed cars or tracks.
pub fn plan_config_sync(
    ac_root: &Path,
    remote_items: &[RemoteConfigItem],
) -> Vec<RemoteConfigItem> {
    let installed_cars = folder_names(&ac_root.join("content").join("cars"));
    let installed_tracks = folder_names(&ac_root.join("content").join("tracks"));

    remote_items
        .iter()
        .filter(|item| matches_installed(&item.rel_path, &installed_cars, &installed_tracks))
        .cloned()
        .collect()
}

/// Applies synced config files provided by `fetch_bytes` into `<ac_root>/extension/<rel_path>`.
///
/// - Refuses while `game_running` is true.
/// - Skips items not matching installed cars/tracks and items already up-to-date.
/// - Verifies SHA-256 of fetched bytes.
/// - If an existing file on disk differs from the last synced SHA-256 (user-edited), backs it
///   up to `<file>.acxs-backup-<stamp>` before writing.
pub fn apply_config_sync<F>(
    ac_root: &Path,
    remote_items: &[RemoteConfigItem],
    game_running: bool,
    mut fetch_bytes: F,
) -> Result<ConfigSyncReport, CspOnlineError>
where
    F: FnMut(&RemoteConfigItem) -> Result<Vec<u8>, CspOnlineError>,
{
    if game_running {
        return Err(CspOnlineError::GameRunning);
    }

    let installed_cars = folder_names(&ac_root.join("content").join("cars"));
    let installed_tracks = folder_names(&ac_root.join("content").join("tracks"));
    let mut index = load_sync_index(ac_root);
    let mut report = ConfigSyncReport::default();

    for item in remote_items {
        if !matches_installed(&item.rel_path, &installed_cars, &installed_tracks) {
            report.skipped_uninstalled += 1;
            continue;
        }
        let rel = validate_rel_path(&item.rel_path)?;
        let dest = ac_root.join("extension").join(&rel);

        let existing_sha = fs::read(&dest).ok().map(|b| sha256_bytes(&b));
        if existing_sha
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(&item.sha256))
        {
            report.skipped_up_to_date += 1;
            continue;
        }

        let bytes = fetch_bytes(item)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(CspOnlineError::TooLarge {
                limit: MAX_CONFIG_BYTES as u64,
            });
        }
        verify_sha256(&bytes, &item.sha256)?;

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        // Back up if the file exists and was not produced by our last sync (i.e. user-edited).
        if let Some(disk_sha) = existing_sha {
            let last_synced = index.entries.get(&item.rel_path).map(|e| e.sha256.as_str());
            if last_synced != Some(disk_sha.as_str()) {
                let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
                let fname = dest
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "config".into());
                let dir = dest.parent().unwrap_or(ac_root);
                let mut backup = dir.join(format!("{fname}.acxs-backup-{stamp}"));
                let mut n = 1u32;
                while backup.exists() {
                    backup = dir.join(format!("{fname}.acxs-backup-{stamp}-{n}"));
                    n += 1;
                }
                fs::rename(&dest, &backup)?;
                report.backed_up.push(backup);
            }
        }

        fs::write(&dest, &bytes)?;
        report.written.push(dest);
        index.entries.insert(
            item.rel_path.clone(),
            ConfigSyncEntry {
                rel_path: item.rel_path.clone(),
                sha256: item.sha256.to_ascii_lowercase(),
                source_commit: item.source_commit.clone(),
                etag: None,
            },
        );
    }

    save_sync_index(ac_root, &index)?;
    Ok(report)
}

fn folder_names(dir: &Path) -> HashSet<String> {
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_ascii_lowercase())
        .collect()
}

fn matches_installed(
    rel_path: &str,
    installed_cars: &HashSet<String>,
    installed_tracks: &HashSet<String>,
) -> bool {
    let lower = rel_path.replace('\\', "/").to_ascii_lowercase();
    let stem = Path::new(&lower)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if stem.is_empty() {
        return false;
    }
    if lower.starts_with("config/cars/") || lower.starts_with("vao-patches-cars/") {
        installed_cars.contains(stem)
    } else if lower.starts_with("config/tracks/") || lower.starts_with("vao-patches-tracks/") {
        installed_tracks.contains(stem)
    } else {
        false
    }
}

fn is_safe_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn validate_rel_path(rel: &str) -> Result<PathBuf, CspOnlineError> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') || rel.contains('\0') || rel.contains(':') {
        return Err(CspOnlineError::UnsafePath(rel.to_string()));
    }
    let mut out = PathBuf::new();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(p) => out.push(p),
            _ => return Err(CspOnlineError::UnsafePath(rel.to_string())),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(CspOnlineError::UnsafePath(rel.to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);
    impl TempRoot {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("acxs-csp-online-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempRoot(p)
        }
    }
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_catalog_json_and_verifies_sha256_mismatch_rejection() {
        let json = r#"{
            "releases": [
                {
                    "version": "0.2.11",
                    "build_id": 3440,
                    "url": "https://example.com/lights-patch-v0.2.11.zip",
                    "changelog": "Fixes",
                    "recommended": true,
                    "sha256": "deadbeef"
                }
            ]
        }"#;
        let catalog = parse_catalog_json(json).unwrap();
        assert_eq!(catalog.releases.len(), 1);
        assert_eq!(catalog.releases[0].version, "0.2.11");
        assert!(catalog.releases[0].recommended);

        let payload = b"hello csp";
        let good = sha256_bytes(payload);
        assert!(verify_sha256(payload, &good).is_ok());
        assert!(matches!(
            verify_sha256(payload, "0000"),
            Err(CspOnlineError::Sha256Mismatch { .. })
        ));
    }

    #[test]
    fn sync_only_downloads_configs_matching_installed_cars_and_tracks() {
        let root = TempRoot::new("sync-filter");
        fs::create_dir_all(root.0.join("content/cars/ks_ferrari")).unwrap();
        fs::create_dir_all(root.0.join("content/tracks/spa")).unwrap();

        let car_bytes = b"[LIGHT_0]\nCOLOR=1,0,0\n";
        let track_bytes = b"VAO_DATA";
        let items = vec![
            RemoteConfigItem {
                rel_path: "config/cars/loaded/ks_ferrari.ini".into(),
                url: "https://example.com/ks_ferrari.ini".into(),
                sha256: sha256_bytes(car_bytes),
                source_commit: "abc123".into(),
            },
            RemoteConfigItem {
                rel_path: "config/cars/loaded/not_installed_car.ini".into(),
                url: "https://example.com/missing.ini".into(),
                sha256: sha256_bytes(b"x"),
                source_commit: "abc123".into(),
            },
            RemoteConfigItem {
                rel_path: "vao-patches-tracks/spa.vao-patch".into(),
                url: "https://example.com/spa.vao-patch".into(),
                sha256: sha256_bytes(track_bytes),
                source_commit: "abc123".into(),
            },
        ];

        let planned = plan_config_sync(&root.0, &items);
        assert_eq!(planned.len(), 2);

        let report = apply_config_sync(&root.0, &items, false, |item| {
            if item.rel_path.contains("ferrari") {
                Ok(car_bytes.to_vec())
            } else {
                Ok(track_bytes.to_vec())
            }
        })
        .unwrap();

        assert_eq!(report.written.len(), 2);
        assert_eq!(report.skipped_uninstalled, 1);
        assert!(root.0.join("extension/config/cars/loaded/ks_ferrari.ini").is_file());
        assert!(root.0.join("extension/vao-patches-tracks/spa.vao-patch").is_file());
        assert!(!root.0.join("extension/config/cars/loaded/not_installed_car.ini").exists());

        // Second sync skips already up-to-date items.
        let report2 = apply_config_sync(&root.0, &items, false, |_| {
            panic!("should not re-fetch up-to-date files")
        })
        .unwrap();
        assert_eq!(report2.skipped_up_to_date, 2);
        assert_eq!(report2.written.len(), 0);
    }

    #[test]
    fn sync_backs_up_user_modified_config_before_overwriting() {
        let root = TempRoot::new("sync-backup");
        fs::create_dir_all(root.0.join("content/cars/ks_bmw")).unwrap();
        let cfg = root.0.join("extension/config/cars/loaded/ks_bmw.ini");
        fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs::write(&cfg, b"; my custom edit\n").unwrap();

        let new_bytes = b"; upstream config\n";
        let items = vec![RemoteConfigItem {
            rel_path: "config/cars/loaded/ks_bmw.ini".into(),
            url: "https://example.com/ks_bmw.ini".into(),
            sha256: sha256_bytes(new_bytes),
            source_commit: "def456".into(),
        }];

        assert!(matches!(
            apply_config_sync(&root.0, &items, true, |_| Ok(new_bytes.to_vec())),
            Err(CspOnlineError::GameRunning)
        ));

        let report = apply_config_sync(&root.0, &items, false, |_| Ok(new_bytes.to_vec())).unwrap();
        assert_eq!(report.written.len(), 1);
        assert_eq!(report.backed_up.len(), 1);
        assert_eq!(fs::read(&report.backed_up[0]).unwrap(), b"; my custom edit\n");
        assert_eq!(fs::read(&cfg).unwrap(), new_bytes);
    }
}
