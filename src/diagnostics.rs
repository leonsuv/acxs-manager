//! Builds a zip file of diagnostic information to attach to a bug report.
//!
//! The bundle holds three entries and nothing else:
//! - `summary.txt`: app version, OS family, whether the AC root looks valid, graphics
//!   proxy DLL names and sizes, lights-patch state and content counts.
//! - `ac-log-tail.txt`: the last [`LOG_TAIL_BYTES`] of the game log, when it can be found.
//! - `settings.json`: the app's own settings file with every path replaced by a placeholder.
//!
//! Save files, replays, car and track content, and DLL files are never read into the
//! bundle. The bundle is written with `create_new`, so an existing file is never overwritten.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::{ac_log, content, csp, graphics_check, settings};

/// Number of trailing bytes of the AC log that go into the bundle.
pub const LOG_TAIL_BYTES: usize = 200 * 1024;

const SUMMARY_NAME: &str = "summary.txt";
const LOG_NAME: &str = "ac-log-tail.txt";
const SETTINGS_NAME: &str = "settings.json";
const BUNDLE_PREFIX: &str = "acxs-diagnostics-";

const AC_ROOT_PLACEHOLDER: &str = "<AC_ROOT>";
const PATCH_LIBRARY_PLACEHOLDER: &str = "<PATCH_LIBRARY>";
const PATH_PLACEHOLDER: &str = "<PATH>";

/// Result of a successful [`create_bundle`] call.
#[derive(Debug, Clone)]
pub struct BundleReport {
    /// The zip file that was written.
    pub path: PathBuf,
    /// Names of the entries inside the zip, in order.
    pub files: Vec<String>,
}

/// Errors from building a diagnostics bundle.
#[derive(Debug)]
pub enum DiagnosticsError {
    /// Creating or writing the output file failed.
    Io(io::Error),
    /// The zip container could not be produced.
    Zip(zip::result::ZipError),
    /// A bundle with the chosen name already exists. It was left untouched.
    AlreadyExists(PathBuf),
}

impl fmt::Display for DiagnosticsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosticsError::Io(e) => write!(f, "diagnostics file operation failed: {e}"),
            DiagnosticsError::Zip(e) => write!(f, "could not build the diagnostics zip: {e}"),
            DiagnosticsError::AlreadyExists(p) => {
                write!(f, "{} already exists and was not overwritten", p.display())
            }
        }
    }
}

impl std::error::Error for DiagnosticsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DiagnosticsError::Io(e) => Some(e),
            DiagnosticsError::Zip(e) => Some(e),
            DiagnosticsError::AlreadyExists(_) => None,
        }
    }
}

impl From<io::Error> for DiagnosticsError {
    fn from(e: io::Error) -> Self {
        DiagnosticsError::Io(e)
    }
}

impl From<zip::result::ZipError> for DiagnosticsError {
    fn from(e: zip::result::ZipError) -> Self {
        DiagnosticsError::Zip(e)
    }
}

/// Writes a diagnostics zip for a bug report into `out_dir`.
///
/// The file is named `acxs-diagnostics-<local time>.zip`. An existing file with that
/// name is not overwritten; the call returns [`DiagnosticsError::AlreadyExists`] instead.
pub fn create_bundle(
    ac_root: &Path,
    out_dir: &Path,
    app_version: &str,
) -> Result<BundleReport, DiagnosticsError> {
    let settings_file = settings::settings_path();
    let log_file = locate_ac_log();
    let inputs = Inputs {
        ac_root,
        app_version,
        settings_file: &settings_file,
        log_file: log_file.as_deref(),
    };
    build_bundle(&inputs, out_dir, Local::now())
}

/// Everything the bundle is built from. Paths are explicit so tests can use fakes.
struct Inputs<'a> {
    ac_root: &'a Path,
    app_version: &'a str,
    settings_file: &'a Path,
    log_file: Option<&'a Path>,
}

fn build_bundle(
    inputs: &Inputs<'_>,
    out_dir: &Path,
    now: DateTime<Local>,
) -> Result<BundleReport, DiagnosticsError> {
    let mut summary = String::new();
    summary.push_str("ACXS diagnostics\n");
    summary.push_str(&format!("created: {}\n", now.format("%Y-%m-%d %H:%M:%S")));
    summary.push_str(&format!("app version: {}\n", inputs.app_version));
    summary.push_str(&format!("os family: {}\n", std::env::consts::FAMILY));
    summary.push_str(&format!("ac root: {}\n", ac_root_status(inputs.ac_root)));
    summary.push_str(&graphics_section(inputs.ac_root));
    summary.push_str(&format!("lights patch: {}\n", lights_patch_status(inputs.ac_root)));
    summary.push_str(&content_section(inputs.ac_root));

    let mut entries: Vec<(&str, Vec<u8>)> = Vec::new();

    match read_log_tail(inputs.log_file) {
        Ok(Some(tail)) => {
            summary.push_str(&format!("ac log: included, last {} bytes\n", tail.len()));
            entries.push((LOG_NAME, tail.into_bytes()));
        }
        Ok(None) => summary.push_str("ac log: not located\n"),
        Err(e) => summary.push_str(&format!("ac log: could not be read: {e}\n")),
    }

    match read_settings(inputs.settings_file) {
        Ok(Some(text)) => {
            summary.push_str("app settings: included with paths replaced\n");
            entries.push((SETTINGS_NAME, text.into_bytes()));
        }
        Ok(None) => summary.push_str("app settings: not found\n"),
        Err(e) => summary.push_str(&format!("app settings: could not be read: {e}\n")),
    }

    // The summary is written first so it is the first entry a reader sees.
    entries.insert(0, (SUMMARY_NAME, summary.into_bytes()));

    let zip_bytes = write_zip(&entries)?;

    fs::create_dir_all(out_dir)?;
    let name = format!("{BUNDLE_PREFIX}{}.zip", now.format("%Y%m%d-%H%M%S"));
    let path = out_dir.join(name);
    write_new_file(&path, &zip_bytes)?;

    Ok(BundleReport {
        path,
        files: entries.iter().map(|(name, _)| name.to_string()).collect(),
    })
}

/// Finds the game log in the user's Documents folder. Checks the OneDrive-redirected
/// Documents folder too, since Windows often uses it.
fn locate_ac_log() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let home = PathBuf::from(home);
    settings::documents_dir()
        .into_iter()
        .chain([home.join("Documents"), home.join("OneDrive").join("Documents")])
        .map(|docs| docs.join("Assetto Corsa").join("logs").join("log.txt"))
        .find(|path| path.is_file())
}

fn ac_root_status(ac_root: &Path) -> &'static str {
    if !ac_root.is_dir() {
        "folder not found"
    } else if settings::is_ac_root(ac_root) {
        "found (acs.exe present)"
    } else {
        "folder present, acs.exe not found"
    }
}

fn graphics_section(ac_root: &Path) -> String {
    match graphics_check::scan(ac_root) {
        Ok(files) if files.is_empty() => "graphics proxy dlls: none\n".to_string(),
        Ok(files) => {
            let mut out = String::from("graphics proxy dlls:\n");
            for file in files {
                let role = match file.role {
                    graphics_check::Role::LightsPatch => "lights patch",
                    graphics_check::Role::ReShade => "reshade",
                    graphics_check::Role::RemixRuntime => "remix runtime",
                    graphics_check::Role::Other => "other",
                };
                out.push_str(&format!("  {} ({} bytes, {role})\n", file.name, file.size));
            }
            out
        }
        Err(e) => format!("graphics proxy dlls: could not scan: {e}\n"),
    }
}

fn lights_patch_status(ac_root: &Path) -> String {
    match csp::installed_version(ac_root) {
        Ok(Some(version)) => format!("installed ({version})"),
        Ok(None) => "not installed".to_string(),
        Err(e) => format!("could not check: {e}"),
    }
}

fn content_section(ac_root: &Path) -> String {
    format!(
        "content: {} car(s), {} track(s)\n",
        content::scan_cars(ac_root).len(),
        content::scan_tracks(ac_root).len()
    )
}

/// Reads the last [`LOG_TAIL_BYTES`] of the log. `Ok(None)` if no log was located.
fn read_log_tail(log_file: Option<&Path>) -> io::Result<Option<String>> {
    let Some(path) = log_file else {
        return Ok(None);
    };
    let text = ac_log::read_log(path)?;
    Ok(Some(tail_text(&text, LOG_TAIL_BYTES).to_string()))
}

/// Returns the last `max_bytes` of `text`, starting at a line boundary when one exists
/// inside the window. The cut never splits a UTF-8 character.
fn tail_text(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let window = &text[start..];
    match window.find('\n') {
        Some(i) => &window[i + 1..],
        None => window,
    }
}

/// Reads the app settings file with every path replaced by a placeholder.
/// `Ok(None)` if the file does not exist.
fn read_settings(settings_file: &Path) -> io::Result<Option<String>> {
    let text = match fs::read_to_string(settings_file) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let scrubbed = match serde_json::from_str::<Value>(&text) {
        Ok(mut value) => {
            scrub_paths(&mut value);
            value
        }
        // Unparseable text could hold paths in any shape, so none of it is kept.
        Err(_) => serde_json::json!({
            "note": "settings file is not valid JSON and its contents were left out"
        }),
    };
    let pretty = serde_json::to_string_pretty(&scrubbed).map_err(io::Error::other)?;
    Ok(Some(pretty))
}

fn scrub_paths(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match child {
                    Value::String(s) => {
                        if let Some(placeholder) = placeholder_for(key, s) {
                            *s = placeholder.to_string();
                        }
                    }
                    other => scrub_paths(other),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub_paths),
        _ => {}
    }
}

/// Placeholder for a string value, or None if the value is kept as it is.
fn placeholder_for(key: &str, value: &str) -> Option<&'static str> {
    if value.trim().is_empty() {
        return None;
    }
    match key {
        "ac_root" => Some(AC_ROOT_PLACEHOLDER),
        "patch_library" => Some(PATCH_LIBRARY_PLACEHOLDER),
        _ if looks_absolute(value) => Some(PATH_PLACEHOLDER),
        _ => None,
    }
}

/// True for POSIX absolute or home-relative paths, UNC paths, and `X:\` or `X:/` drive paths.
fn looks_absolute(value: &str) -> bool {
    let v = value.trim();
    let bytes = v.as_bytes();
    v.starts_with('/')
        || v.starts_with('~')
        || v.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/'))
}

/// Builds the zip in memory, so a failure cannot leave a partial file behind.
fn write_zip(entries: &[(&str, Vec<u8>)]) -> Result<Vec<u8>, DiagnosticsError> {
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, options)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), DiagnosticsError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(DiagnosticsError::AlreadyExists(path.to_path_buf()));
        }
        Err(e) => return Err(e.into()),
    };
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use zip::ZipArchive;

    /// A directory under the system temp dir, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!(
                "acxs_diag_test_{}_{}",
                std::process::id(),
                name
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Fake AC install with an executable, a lights-patch DLL, a reshade DLL,
    /// car and track content, a save file and a replay.
    fn fake_ac_root(base: &Path) -> PathBuf {
        let root = base.join("assettocorsa");
        let car = root.join("content").join("cars").join("fake_car");
        fs::create_dir_all(car.join("ui")).unwrap();
        fs::create_dir_all(car.join("data")).unwrap();
        fs::write(car.join("ui").join("ui_car.json"), r#"{"name": "Fake"}"#).unwrap();
        let track = root.join("content").join("tracks").join("fake_track");
        fs::create_dir_all(track.join("ui")).unwrap();
        fs::write(track.join("ui").join("ui_track.json"), r#"{"name": "Fake"}"#).unwrap();
        fs::create_dir_all(root.join("savedsetups")).unwrap();
        fs::write(root.join("savedsetups").join("save.txt"), b"save data").unwrap();
        fs::create_dir_all(root.join("replay")).unwrap();
        fs::write(root.join("replay").join("replay.mpr"), b"replay data").unwrap();
        fs::write(root.join(settings::AC_EXECUTABLE), b"exe").unwrap();
        fs::write(root.join("dwrite.dll"), b"fake dll bytes").unwrap();
        fs::write(root.join("d3d11.dll"), b"other dll bytes").unwrap();
        root
    }

    #[test]
    fn bundle_has_expected_entries_and_sanitized_settings() {
        let dir = TempDir::new("contents");
        let root = fake_ac_root(dir.path());
        let settings_file = dir.path().join("settings.json");
        let patch_dir = dir.path().join("patch_library");
        let settings_text = serde_json::json!({
            "ac_root": root.display().to_string(),
            "patch_library": patch_dir.display().to_string(),
            "theme": "dark",
        });
        fs::write(&settings_file, settings_text.to_string()).unwrap();

        // A log larger than the tail limit, so only its end should be bundled.
        let log_file = dir.path().join("log.txt");
        let mut log = String::new();
        for i in 0..40_000 {
            log.push_str(&format!("line {i:06} some log text here\n"));
        }
        log.push_str("LAST LINE MARKER\n");
        fs::write(&log_file, &log).unwrap();

        let out_dir = dir.path().join("out");
        let inputs = Inputs {
            ac_root: &root,
            app_version: "9.9.9-test",
            settings_file: &settings_file,
            log_file: Some(&log_file),
        };
        let report = build_bundle(&inputs, &out_dir, Local::now()).unwrap();

        assert_eq!(report.files, vec!["summary.txt", "ac-log-tail.txt", "settings.json"]);
        assert!(report.path.starts_with(&out_dir));
        assert!(report.path.file_name().unwrap().to_string_lossy().starts_with("acxs-diagnostics-"));

        let mut archive = ZipArchive::new(File::open(&report.path).unwrap()).unwrap();
        let names: Vec<String> = archive
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        assert_eq!(names, report.files);

        let read_entry = |archive: &mut ZipArchive<File>, name: &str| -> String {
            let mut entry = archive.by_name(name).unwrap();
            let mut text = String::new();
            io::Read::read_to_string(&mut entry, &mut text).unwrap();
            text
        };

        let summary = read_entry(&mut archive, "summary.txt");
        assert!(summary.contains("app version: 9.9.9-test"));
        assert!(summary.contains("found (acs.exe present)"));
        assert!(summary.contains("dwrite.dll"));
        assert!(summary.contains("content: 1 car(s), 1 track(s)"));
        assert!(!summary.contains(&root.display().to_string()));

        let settings_entry = read_entry(&mut archive, "settings.json");
        assert!(settings_entry.contains("<AC_ROOT>"));
        assert!(settings_entry.contains("<PATCH_LIBRARY>"));
        assert!(settings_entry.contains("dark"));
        assert!(!settings_entry.contains(&dir.path().display().to_string()));
        assert!(!settings_entry.contains("\"/"));
        assert!(!settings_entry.contains(":\\"));

        let log_entry = read_entry(&mut archive, "ac-log-tail.txt");
        assert!(log_entry.len() <= LOG_TAIL_BYTES);
        assert!(log_entry.trim_end().ends_with("LAST LINE MARKER"));
        assert!(!log_entry.contains("line 000000 "));
    }

    #[test]
    fn refuses_to_overwrite_existing_bundle() {
        let dir = TempDir::new("overwrite");
        let root = fake_ac_root(dir.path());
        let settings_file = dir.path().join("missing_settings.json");
        let out_dir = dir.path().join("out");
        let inputs = Inputs {
            ac_root: &root,
            app_version: "1.0.0",
            settings_file: &settings_file,
            log_file: None,
        };
        let now = Local::now();
        let first = build_bundle(&inputs, &out_dir, now).unwrap();
        let before = fs::read(&first.path).unwrap();

        let second = build_bundle(&inputs, &out_dir, now);
        match second {
            Err(DiagnosticsError::AlreadyExists(path)) => assert_eq!(path, first.path),
            other => panic!("expected AlreadyExists, got {other:?}"),
        }
        assert_eq!(fs::read(&first.path).unwrap(), before);
    }

    #[test]
    fn tail_text_keeps_whole_lines_and_valid_utf8() {
        assert_eq!(tail_text("short", 100), "short");
        let text = "first\nsecond\nthird\n";
        assert_eq!(tail_text(text, 10), "third\n");
        // Cut lands inside a multi-byte character; the function must not panic.
        let accented = "ééééééé\nend";
        let tail = tail_text(accented, 6);
        assert!(tail.ends_with("end"));
    }
}
