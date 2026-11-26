//! Missing-content manifest parser and safe streaming auto-installer for online servers.
//!
//! Reads server-advertised `content_url` / `content.json` manifests (from `/INFO`,
//! `/JSON|-1`, or CSP/AssettoServer HTTP endpoints), streams archives into
//! `<ac_root>/.acxs_staging/downloads/<unique>.part` with progress reporting,
//! cancellation, size caps, and optional SHA-256 verification, then hands off to
//! [`crate::installer::inspect_package`] and [`crate::installer::install_plan`].

use crate::installer::{BatchInstallReport, InstallError, inspect_package, install_plan};
use crate::lobby::web_agent;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default maximum download size per archive (2 GiB).
pub const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const CHUNK_BYTES: usize = 64 * 1024;
const STAGING_DOWNLOADS: &str = ".acxs_staging/downloads";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Kind of missing content item advertised by a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DownloadAssetKind {
    Car,
    Track,
    Pack,
}

/// One downloadable content item resolved from a server manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DownloadItem {
    pub kind: DownloadAssetKind,
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// Resolved list of missing items to download and install.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContentDownloadPlan {
    pub items: Vec<DownloadItem>,
    pub total_bytes: Option<u64>,
}

/// Streaming progress update emitted during download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub item_id: String,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
}

/// Errors returned while fetching manifests or downloading/installing content.
#[derive(Debug)]
pub enum DownloadError {
    InvalidUrl(String),
    GameRunning,
    Http(String),
    TooLarge { limit: u64 },
    ChecksumMismatch { expected: String, actual: String },
    Cancelled,
    Install(InstallError),
    Io(io::Error),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadError::InvalidUrl(u) => write!(f, "invalid download URL {u:?}"),
            DownloadError::GameRunning => {
                write!(f, "the game is running; close it before installing content")
            }
            DownloadError::Http(msg) => write!(f, "download failed: {msg}"),
            DownloadError::TooLarge { limit } => {
                write!(f, "download exceeds safety limit of {limit} bytes")
            }
            DownloadError::ChecksumMismatch { expected, actual } => {
                write!(f, "SHA-256 mismatch: expected {expected}, got {actual}")
            }
            DownloadError::Cancelled => write!(f, "download cancelled"),
            DownloadError::Install(e) => write!(f, "install failed: {e}"),
            DownloadError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<io::Error> for DownloadError {
    fn from(e: io::Error) -> Self {
        DownloadError::Io(e)
    }
}

impl From<InstallError> for DownloadError {
    fn from(e: InstallError) -> Self {
        DownloadError::Install(e)
    }
}

/// Validates that `url` is a plain `http://` or `https://` address with no whitespace/control chars.
pub fn validate_url(url: &str) -> Result<String, DownloadError> {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://"))
        || trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(DownloadError::InvalidUrl(trimmed.to_string()));
    }
    Ok(trimmed.to_string())
}

/// Extracts server-advertised download URLs directly from `/INFO` and `/JSON|-1` replies.
pub fn parse_server_content_urls(
    raw_info: &str,
    raw_entries: &str,
    missing_track: Option<&str>,
    missing_cars: &[String],
) -> ContentDownloadPlan {
    let mut items = Vec::new();
    let mut seen_urls: HashSet<String> = HashSet::new();
    let wanted_cars: HashSet<&str> = missing_cars.iter().map(String::as_str).collect();

    let push_unique = |items: &mut Vec<DownloadItem>,
                       seen: &mut HashSet<String>,
                       kind: DownloadAssetKind,
                       id: String,
                       url: &str| {
        if let Ok(clean) = validate_url(url)
            && seen.insert(clean.clone())
        {
            items.push(DownloadItem {
                kind,
                id,
                url: clean,
                sha256: None,
                size_bytes: None,
            });
        }
    };

    if let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(raw_entries) {
        if let Some(Value::Array(cars)) = entries.get("Cars") {
            for car in cars.iter().filter_map(Value::as_object) {
                let model = car.get("Model").and_then(Value::as_str).unwrap_or("");
                if !wanted_cars.contains(model) {
                    continue;
                }
                if let Some(url) = car
                    .get("DownloadUrl")
                    .or_else(|| car.get("downloadUrl"))
                    .or_else(|| car.get("ContentUrl"))
                    .or_else(|| car.get("content_url"))
                    .and_then(Value::as_str)
                {
                    push_unique(&mut items, &mut seen_urls, DownloadAssetKind::Car, model.to_string(), url);
                }
            }
        }
        if let Some(url) = entries
            .get("ContentUrl")
            .or_else(|| entries.get("content_url"))
            .or_else(|| entries.get("downloadUrl"))
            .and_then(Value::as_str)
        {
            let kind = if missing_track.is_some() && !missing_cars.is_empty() {
                DownloadAssetKind::Pack
            } else if missing_track.is_some() {
                DownloadAssetKind::Track
            } else {
                DownloadAssetKind::Pack
            };
            let id = missing_track.unwrap_or("server-pack").to_string();
            push_unique(&mut items, &mut seen_urls, kind, id, url);
        }
    }

    if let Ok(Value::Object(info)) = serde_json::from_str::<Value>(raw_info)
        && let Some(url) = info
            .get("contentUrl")
            .or_else(|| info.get("content_url"))
            .or_else(|| info.get("downloadUrl"))
            .and_then(Value::as_str)
    {
        let id = missing_track.unwrap_or("server-pack").to_string();
        push_unique(&mut items, &mut seen_urls, DownloadAssetKind::Pack, id, url);
    }

    finalize_plan(items)
}

/// Parses a CSP/ACXS JSON content manifest and selects only the requested missing track/cars
/// (or a server-wide pack when individual items are not listed).
pub fn parse_manifest_json(
    json: &str,
    missing_track: Option<&str>,
    missing_cars: &[String],
) -> Result<ContentDownloadPlan, String> {
    let val: Value =
        serde_json::from_str(json).map_err(|e| format!("invalid content manifest JSON: {e}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| "content manifest root must be a JSON object".to_string())?;

    let mut items = Vec::new();
    let wanted_cars: HashSet<&str> = missing_cars.iter().map(String::as_str).collect();

    if let Some(track_id) = missing_track
        && let Some(track_val) = obj.get("track")
        && let Some(item) = parse_manifest_entry(DownloadAssetKind::Track, track_id, track_val)
    {
        items.push(item);
    }

    if let Some(Value::Object(cars_map)) = obj.get("cars") {
        for (car_id, entry) in cars_map {
            if wanted_cars.contains(car_id.as_str())
                && let Some(item) = parse_manifest_entry(DownloadAssetKind::Car, car_id, entry)
            {
                items.push(item);
            }
        }
    }

    if let Some(Value::Array(list)) = obj.get("items") {
        for entry in list.iter().filter_map(Value::as_object) {
            let kind_str = entry.get("kind").and_then(Value::as_str).unwrap_or("pack");
            let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
            let kind = match kind_str.to_ascii_lowercase().as_str() {
                "car" => {
                    if !wanted_cars.contains(id) {
                        continue;
                    }
                    DownloadAssetKind::Car
                }
                "track" => {
                    if missing_track != Some(id) {
                        continue;
                    }
                    DownloadAssetKind::Track
                }
                _ => DownloadAssetKind::Pack,
            };
            if let Some(item) = parse_manifest_entry(kind, id, &Value::Object(entry.clone())) {
                items.push(item);
            }
        }
    }

    if items.is_empty()
        && let Some(pack_val) = obj.get("pack")
        && let Some(item) = parse_manifest_entry(DownloadAssetKind::Pack, "server-pack", pack_val)
    {
        items.push(item);
    }

    Ok(finalize_plan(items))
}

fn parse_manifest_entry(
    kind: DownloadAssetKind,
    default_id: &str,
    val: &Value,
) -> Option<DownloadItem> {
    match val {
        Value::String(url) => validate_url(url).ok().map(|clean| DownloadItem {
            kind,
            id: default_id.to_string(),
            url: clean,
            sha256: None,
            size_bytes: None,
        }),
        Value::Object(map) => {
            let url = map.get("url").and_then(Value::as_str)?;
            let clean = validate_url(url).ok()?;
            let id = map
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(default_id)
                .to_string();
            let sha256 = map
                .get("sha256")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty());
            let size_bytes = map
                .get("size_bytes")
                .or_else(|| map.get("size"))
                .and_then(Value::as_u64);
            Some(DownloadItem {
                kind,
                id,
                url: clean,
                sha256,
                size_bytes,
            })
        }
        _ => None,
    }
}

fn finalize_plan(items: Vec<DownloadItem>) -> ContentDownloadPlan {
    let total_bytes = if !items.is_empty() && items.iter().all(|i| i.size_bytes.is_some()) {
        Some(items.iter().filter_map(|i| i.size_bytes).sum())
    } else {
        None
    };
    ContentDownloadPlan { items, total_bytes }
}

/// Fetches a JSON content manifest over HTTP(S) and resolves missing items.
pub fn fetch_manifest(
    url: &str,
    missing_track: Option<&str>,
    missing_cars: &[String],
    timeout: Duration,
) -> Result<ContentDownloadPlan, DownloadError> {
    let clean = validate_url(url)?;
    let response = web_agent(timeout, true)
        .map_err(DownloadError::Http)?
        .get(&clean)
        .call()
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_MANIFEST_BYTES)
        .read_to_string(&mut body)
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    parse_manifest_json(&body, missing_track, missing_cars).map_err(DownloadError::Http)
}

/// Streams `item` into `<ac_root>/.acxs_staging/downloads/`, verifies optional SHA-256,
/// and installs via [`inspect_package`] -> [`install_plan`].
///
/// Any cancellation, checksum mismatch, or oversize transfer removes the `.part` file
/// immediately and never mutates `content/`.
pub fn download_and_install_item(
    ac_root: &Path,
    item: &DownloadItem,
    game_running: bool,
    seven_zip_path: Option<&Path>,
    cancel: &AtomicBool,
    max_bytes: u64,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<BatchInstallReport, DownloadError> {
    if game_running {
        return Err(DownloadError::GameRunning);
    }
    if cancel.load(Ordering::Acquire) {
        return Err(DownloadError::Cancelled);
    }
    let clean_url = validate_url(&item.url)?;

    let dl_dir = ac_root.join(STAGING_DOWNLOADS);
    fs::create_dir_all(&dl_dir)?;
    let stem = unique_stem(&item.id);
    let part_path = dl_dir.join(format!("{stem}.part"));

    let response = web_agent(Duration::from_secs(30), true)
        .map_err(DownloadError::Http)?
        .get(&clean_url)
        .call()
        .map_err(|e| DownloadError::Http(e.to_string()))?;

    let bytes_total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .or(item.size_bytes);
    if let Some(total) = bytes_total
        && total > max_bytes
    {
        return Err(DownloadError::TooLarge { limit: max_bytes });
    }

    let mut reader = response.into_reader();
    let stream_res = (|| -> Result<String, DownloadError> {
        let mut out = File::create(&part_path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK_BYTES];
        let mut done: u64 = 0;

        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(DownloadError::Cancelled);
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            done = done.saturating_add(n as u64);
            if done > max_bytes {
                return Err(DownloadError::TooLarge { limit: max_bytes });
            }
            out.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            on_progress(DownloadProgress {
                item_id: item.id.clone(),
                bytes_done: done,
                bytes_total,
            });
        }
        out.flush()?;
        Ok(hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    })();

    let actual_sha = match stream_res {
        Ok(sha) => sha,
        Err(e) => {
            let _ = fs::remove_file(&part_path);
            return Err(e);
        }
    };

    if let Some(expected) = item.sha256.as_deref()
        && !actual_sha.eq_ignore_ascii_case(expected.trim())
    {
        let _ = fs::remove_file(&part_path);
        return Err(DownloadError::ChecksumMismatch {
            expected: expected.trim().to_ascii_lowercase(),
            actual: actual_sha,
        });
    }

    if cancel.load(Ordering::Acquire) {
        let _ = fs::remove_file(&part_path);
        return Err(DownloadError::Cancelled);
    }

    let ext = archive_extension_from_url(&clean_url);
    let archive_path = dl_dir.join(format!("{stem}{ext}"));
    fs::rename(&part_path, &archive_path)?;

    let install_res = (|| -> Result<BatchInstallReport, DownloadError> {
        let plan = inspect_package(&archive_path, Some(ac_root), seven_zip_path)?;
        let report = install_plan(ac_root, &plan, game_running, seven_zip_path)?;
        Ok(report)
    })();

    let _ = fs::remove_file(&archive_path);
    install_res
}

/// Downloads and installs every item in `plan` sequentially.
pub fn download_and_install_plan(
    ac_root: &Path,
    plan: &ContentDownloadPlan,
    game_running: bool,
    seven_zip_path: Option<&Path>,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<Vec<BatchInstallReport>, DownloadError> {
    let mut reports = Vec::with_capacity(plan.items.len());
    for item in &plan.items {
        let report = download_and_install_item(
            ac_root,
            item,
            game_running,
            seven_zip_path,
            cancel,
            MAX_DOWNLOAD_BYTES,
            &mut on_progress,
        )?;
        reports.push(report);
    }
    Ok(reports)
}

fn unique_stem(id: &str) -> String {
    let clean: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(32)
        .collect();
    let prefix = if clean.is_empty() { "item" } else { &clean };
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{ts}-{n}", process::id())
}

fn archive_extension_from_url(url: &str) -> &'static str {
    let path = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".tar.gz") {
        ".tar.gz"
    } else if path.ends_with(".tgz") {
        ".tgz"
    } else if path.ends_with(".7z") {
        ".7z"
    } else if path.ends_with(".rar") {
        ".rar"
    } else {
        ".zip"
    }
}

/// Returns the staging downloads directory under `ac_root`.
pub fn staging_downloads_dir(ac_root: &Path) -> PathBuf {
    ac_root.join(STAGING_DOWNLOADS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Cursor};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use zip::write::SimpleFileOptions;

    fn make_car_zip(car_id: &str) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let opts = SimpleFileOptions::default();
            zip.start_file(format!("content/cars/{car_id}/ui/ui_car.json"), opts)
                .unwrap();
            zip.write_all(br#"{"name":"Test Car"}"#).unwrap();
            zip.start_file(format!("content/cars/{car_id}/data.acd"), opts)
                .unwrap();
            zip.write_all(b"dummy-acd-payload").unwrap();
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn serve_bytes(body: Vec<u8>, chunk_delay: Option<Duration>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                loop {
                    let mut h = String::new();
                    match reader.read_line(&mut h) {
                        Ok(n) if n > 2 => continue,
                        _ => break,
                    }
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                if let Some(delay) = chunk_delay {
                    for chunk in body.chunks(1024) {
                        if stream.write_all(chunk).is_err() {
                            break;
                        }
                        let _ = stream.flush();
                        thread::sleep(delay);
                    }
                } else {
                    let _ = stream.write_all(&body);
                }
            }
        });
        port
    }

    fn temp_ac(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("acxs-dl-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("content/cars")).unwrap();
        fs::create_dir_all(dir.join("content/tracks")).unwrap();
        dir
    }

    #[test]
    fn parses_server_entries_and_manifest_json_for_missing_content() {
        let entries = r#"{
            "Cars": [
                {"Model": "car_a", "DownloadUrl": "https://cdn.example.com/car_a.zip"},
                {"Model": "car_b", "DownloadUrl": "https://cdn.example.com/car_b.zip"}
            ]
        }"#;
        let plan = parse_server_content_urls("{}", entries, None, &["car_b".into()]);
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].id, "car_b");
        assert_eq!(plan.items[0].url, "https://cdn.example.com/car_b.zip");

        let manifest = r#"{
            "track": {"id": "la_canyons", "url": "https://cdn.example.com/la.zip", "size": 500},
            "cars": {
                "car_a": {"url": "https://cdn.example.com/a.zip", "sha256": "abc", "size": 200},
                "car_b": {"url": "https://cdn.example.com/b.zip", "size": 300}
            }
        }"#;
        let mplan = parse_manifest_json(manifest, Some("la_canyons"), &["car_a".into()]).unwrap();
        assert_eq!(mplan.items.len(), 2);
        assert_eq!(mplan.total_bytes, Some(700));
    }

    #[test]
    fn mock_http_download_verifies_sha256_and_installs_via_installer() {
        let ac = temp_ac("ok");
        let zip_bytes = make_car_zip("test_gt3");
        let sha = sha256_hex(&zip_bytes);
        let port = serve_bytes(zip_bytes.clone(), None);

        let item = DownloadItem {
            kind: DownloadAssetKind::Car,
            id: "test_gt3".into(),
            url: format!("http://127.0.0.1:{port}/test_gt3.zip"),
            sha256: Some(sha),
            size_bytes: Some(zip_bytes.len() as u64),
        };
        let cancel = AtomicBool::new(false);
        let mut saw_progress = false;
        let report = download_and_install_item(
            &ac,
            &item,
            false,
            None,
            &cancel,
            MAX_DOWNLOAD_BYTES,
            |p| {
                if p.bytes_done > 0 {
                    saw_progress = true;
                }
            },
        )
        .unwrap();

        assert!(saw_progress);
        assert_eq!(report.installed.len(), 1);
        assert!(ac.join("content/cars/test_gt3/ui/ui_car.json").is_file());
        // Temp staging download is cleaned up.
        assert_eq!(fs::read_dir(staging_downloads_dir(&ac)).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&ac);
    }

    #[test]
    fn mock_http_checksum_mismatch_deletes_part_and_leaves_content_untouched() {
        let ac = temp_ac("bad-sha");
        let zip_bytes = make_car_zip("bad_car");
        let port = serve_bytes(zip_bytes, None);

        let item = DownloadItem {
            kind: DownloadAssetKind::Car,
            id: "bad_car".into(),
            url: format!("http://127.0.0.1:{port}/bad_car.zip"),
            sha256: Some("0".repeat(64)),
            size_bytes: None,
        };
        let cancel = AtomicBool::new(false);
        let err = download_and_install_item(
            &ac,
            &item,
            false,
            None,
            &cancel,
            MAX_DOWNLOAD_BYTES,
            |_| {},
        )
        .unwrap_err();

        assert!(matches!(err, DownloadError::ChecksumMismatch { .. }));
        assert!(!ac.join("content/cars/bad_car").exists());
        assert_eq!(fs::read_dir(staging_downloads_dir(&ac)).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&ac);
    }

    #[test]
    fn download_cancellation_aborts_cleanly_and_cleans_staging() {
        let ac = temp_ac("cancel");
        let payload = vec![b'A'; 8192];
        let port = serve_bytes(payload, Some(Duration::from_millis(25)));

        let item = DownloadItem {
            kind: DownloadAssetKind::Pack,
            id: "slow_pack".into(),
            url: format!("http://127.0.0.1:{port}/slow.zip"),
            sha256: None,
            size_bytes: None,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);
        let err = download_and_install_item(
            &ac,
            &item,
            false,
            None,
            &cancel,
            MAX_DOWNLOAD_BYTES,
            move |_| {
                cancel_clone.store(true, Ordering::Release);
            },
        )
        .unwrap_err();

        assert!(matches!(err, DownloadError::Cancelled));
        assert_eq!(fs::read_dir(staging_downloads_dir(&ac)).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&ac);
    }

    #[test]
    fn refuses_download_while_game_is_running_and_rejects_bad_urls() {
        let ac = temp_ac("guard");
        let item = DownloadItem {
            kind: DownloadAssetKind::Car,
            id: "x".into(),
            url: "http://127.0.0.1:1/x.zip".into(),
            sha256: None,
            size_bytes: None,
        };
        let cancel = AtomicBool::new(false);
        assert!(matches!(
            download_and_install_item(&ac, &item, true, None, &cancel, MAX_DOWNLOAD_BYTES, |_| {}),
            Err(DownloadError::GameRunning)
        ));
        assert!(matches!(
            validate_url("file:///etc/passwd"),
            Err(DownloadError::InvalidUrl(_))
        ));
        let _ = fs::remove_dir_all(&ac);
    }
}
