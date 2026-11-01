//! Opt-in crash reporting to the project's self-hosted GlitchTip server.
//!
//! Panics are always stored locally: [`install_panic_hook`] writes one JSON file
//! per panic into `<settings folder>/crash-reports/`. Nothing leaves the computer
//! at that point. Sending happens only when the user has ticked "Send crash
//! reports" in Settings; then [`send_pending`] posts each stored report to
//! the GlitchTip "store" API (Sentry-compatible) and deletes the file after the
//! server answers with a 2xx status.
//!
//! What is sent: the panic message, the panic location, a backtrace, the app
//! version, the OS name and architecture, and the time. The user's home folder
//! is replaced with `~` in all of these. Nothing else from the machine is read.
//!
//! The DSN (`https://<public_key>@<host>[:port][/<prefix>]/<project_id>`) comes
//! from [`DEFAULT_DSN`]. The placeholder value is refused, so a build without a
//! real DSN never sends anything.
//!
//! Limits: the send runs on the caller's thread with a timeout per request, and
//! it stops at the first failure, so unsent reports stay on disk for the next
//! start.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The DSN of the project's GlitchTip server (https://test6.azendix.de, project "ACXS Manager").
/// The public key in a DSN is not a secret: it only allows sending events.
pub const DEFAULT_DSN: &str = "https://ff2dc40af06f4236b1e2a4a6ff535ae6@test6.azendix.de/1";

/// Text that marks a DSN as not configured.
const PLACEHOLDER: &str = "__GLITCHTIP_DSN__";

/// Name of the folder, inside the settings folder, that holds unsent reports.
const PENDING_DIR_NAME: &str = "crash-reports";

/// How many names are tried when a report file with the same name already exists.
const MAX_NAME_TRIES: usize = 100;

/// A parsed GlitchTip DSN.
#[derive(Debug, Clone, PartialEq)]
pub struct Dsn {
    /// Always `https`. Other schemes are refused by [`Dsn::parse`].
    pub scheme: String,
    /// Public key sent in the `sentry_key` field of the auth header.
    pub public_key: String,
    /// Host name, with the port when one is given.
    pub host: String,
    /// Numeric project id, the last path segment of the DSN.
    pub project_id: String,
    /// Path before the project id, with a leading `/`, or empty.
    pub path_prefix: String,
}

impl Dsn {
    /// Parse `https://<public_key>@<host>[:port][/<prefix>]/<project_id>`.
    ///
    /// Refuses other schemes, a missing or malformed key or project id, and the
    /// [`DEFAULT_DSN`] placeholder.
    pub fn parse(dsn: &str) -> Result<Dsn, String> {
        let dsn = dsn.trim();
        if dsn.is_empty() || dsn.contains(PLACEHOLDER) {
            return Err("no crash report DSN is configured".into());
        }
        let rest = dsn.strip_prefix("https://").ok_or("the DSN must start with https://")?;
        let (userinfo, location) = rest.split_once('@').ok_or("the DSN has no public key")?;
        let public_key = userinfo.split(':').next().unwrap_or_default();
        if public_key.is_empty() || !public_key.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("the DSN public key is missing or malformed".into());
        }
        let (authority, path) = location.split_once('/').ok_or("the DSN has no project id")?;
        if authority.is_empty() || !authority.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._:".contains(&b)) {
            return Err("the DSN host is missing or malformed".into());
        }
        if let Some((_, port)) = authority.rsplit_once(':')
            && (port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err("the DSN port is malformed".into());
        }
        let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let project_id = segments.pop().ok_or("the DSN has no project id")?;
        if !project_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err("the DSN project id must be numeric".into());
        }
        let path_prefix = if segments.is_empty() {
            String::new()
        } else {
            format!("/{}", segments.join("/"))
        };
        Ok(Dsn {
            scheme: "https".into(),
            public_key: public_key.into(),
            host: authority.into(),
            project_id: project_id.into(),
            path_prefix,
        })
    }

    /// The store endpoint: `https://<host>[/<prefix>]/api/<project_id>/store/`.
    fn store_url(&self) -> String {
        format!("{}://{}{}/api/{}/store/", self.scheme, self.host, self.path_prefix, self.project_id)
    }

    /// Value of the `X-Sentry-Auth` header.
    fn auth_header(&self) -> String {
        format!(
            "Sentry sentry_version=7, sentry_client=acxs-manager/{}, sentry_key={}",
            env!("CARGO_PKG_VERSION"),
            self.public_key
        )
    }
}

/// One panic, as stored on disk and sent to the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrashReport {
    /// The panic payload as text.
    pub message: String,
    /// `file:line:column` of the panic, when known.
    pub location: Option<String>,
    /// Backtrace captured inside the panic hook.
    pub backtrace: String,
    /// Version of the app that panicked.
    pub app_version: String,
    /// OS name and CPU architecture, for example `windows x86_64`.
    pub os: String,
    /// When the panic happened, RFC 3339 in UTC.
    pub timestamp: String,
}

/// Build a Sentry event for `report`, ready to be posted to the store endpoint.
///
/// The user's home folder is replaced with `~` in the message, the location and
/// the backtrace.
pub fn event_json(report: &CrashReport) -> serde_json::Value {
    event_json_with_home(report, home_dir().as_deref())
}

fn event_json_with_home(report: &CrashReport, home: Option<&str>) -> serde_json::Value {
    let message = scrub(&report.message, home);
    let location = report.location.as_ref().map(|loc| scrub(loc, home));
    let backtrace = scrub(&report.backtrace, home);
    let event_id: String = Sha256::digest(format!("{message}\n{}", report.timestamp))
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect();
    serde_json::json!({
        "event_id": event_id,
        "timestamp": report.timestamp,
        "platform": "native",
        "level": "fatal",
        "logger": "panic",
        "release": format!("acxs-manager@{}", report.app_version),
        "tags": { "os": report.os },
        "exception": {
            "values": [ { "type": "panic", "value": message } ]
        },
        "extra": { "location": location, "backtrace": backtrace }
    })
}

/// Replace the home folder `home` in `text` with `~`.
///
/// Both separator styles are handled (`C:\Users\me` and `C:/Users/me`). A match
/// counts only when it ends at a separator or at the end of the text, so
/// `/home/me` does not change `/home/mev`. `None`, or an empty home, leaves
/// the text as it is.
pub fn scrub(text: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])).filter(|h| !h.is_empty()) else {
        return text.to_string();
    };
    let mut out = text.to_string();
    for variant in [home.to_string(), home.replace('\\', "/")] {
        out = replace_dir(&out, &variant);
    }
    out
}

/// Replace `dir` in `text`, ignoring ASCII case: Windows paths are not case
/// sensitive, so `C:\Users\Leon` must also match `c:\users\leon`.
fn replace_dir(text: &str, dir: &str) -> String {
    let lower_dir = dir.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // ASCII lowercasing keeps byte offsets, so positions found in the lowered
    // copy are valid in the original.
    while let Some(pos) = rest.to_ascii_lowercase().find(&lower_dir) {
        let after = &rest[pos + dir.len()..];
        out.push_str(&rest[..pos]);
        if after.is_empty() || after.starts_with(['/', '\\']) {
            out.push('~');
        } else {
            out.push_str(dir);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The current user's home folder, from `USERPROFILE` or `HOME`.
fn home_dir() -> Option<String> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| home.to_string_lossy().into_owned())
        .filter(|home| !home.is_empty())
}

/// Folder that holds unsent reports: `<settings_dir>/crash-reports`.
pub fn pending_dir(settings_dir: &Path) -> PathBuf {
    settings_dir.join(PENDING_DIR_NAME)
}

/// Store `report` as a new JSON file in `dir` and return its path.
///
/// The file is named after the report timestamp. An existing file is never
/// overwritten: a numeric suffix is added instead.
pub fn save_pending(dir: &Path, report: &CrashReport) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(report).map_err(io::Error::other)?;
    let stem: String = report
        .timestamp
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let stem = if stem.is_empty() { "crash".to_string() } else { stem };
    for attempt in 0..MAX_NAME_TRIES {
        let name = if attempt == 0 {
            format!("{stem}.json")
        } else {
            format!("{stem}-{attempt}.json")
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free crash report file name"))
}

/// Read every stored report in `dir`, sorted by file name.
///
/// Files that cannot be read or parsed are skipped. A missing folder gives an
/// empty list.
pub fn load_pending(dir: &Path) -> Vec<(PathBuf, CrashReport)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let report = serde_json::from_str(&text).ok()?;
            Some((path, report))
        })
        .collect()
}

/// Install a panic hook that stores each panic in [`pending_dir`]`(settings_dir)`.
///
/// The previous hook still runs first, so the usual message on stderr is kept.
/// Storing never panics: any failure is ignored.
pub fn install_panic_hook(settings_dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| previous(info)));
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let report = report_from_panic(info);
            let _ = save_pending(&pending_dir(&settings_dir), &report);
        }));
    }));
}

fn report_from_panic(info: &PanicHookInfo<'_>) -> CrashReport {
    let home = home_dir();
    let home = home.as_deref();
    let location = info
        .location()
        .map(|loc| scrub(&format!("{}:{}:{}", loc.file(), loc.line(), loc.column()), home));
    CrashReport {
        message: scrub(&panic_message(info), home),
        location,
        backtrace: scrub(&std::backtrace::Backtrace::force_capture().to_string(), home),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    }
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic with a non-text payload".to_string()
    }
}

/// Post every stored report in `dir` to the GlitchTip store endpoint of `dsn`.
///
/// Each report is deleted after a 2xx answer. Returns how many were sent. The
/// first network error or non-2xx answer stops the run and is returned as an
/// error; the reports not yet sent stay on disk.
pub fn send_pending(dsn: &Dsn, dir: &Path, timeout: Duration) -> Result<usize, String> {
    let pending = load_pending(dir);
    if pending.is_empty() {
        return Ok(0);
    }
    let agent = agent(timeout)?;
    let url = dsn.store_url();
    let auth = dsn.auth_header();
    let mut sent = 0;
    for (path, report) in pending {
        let body = event_json(&report).to_string();
        match agent
            .post(&url)
            .set("X-Sentry-Auth", &auth)
            .set("Content-Type", "application/json")
            .send_string(&body)
        {
            Ok(_) => {
                fs::remove_file(&path).map_err(|e| format!("could not delete {}: {e}", path.display()))?;
                sent += 1;
            }
            Err(ureq::Error::Status(code, _)) => return Err(format!("the crash report server answered HTTP {code}")),
            Err(ureq::Error::Transport(err)) => return Err(format!("could not reach the crash report server: {err}")),
        }
    }
    Ok(sent)
}

fn agent(timeout: Duration) -> Result<ureq::Agent, String> {
    let builder = ureq::AgentBuilder::new();
    // Windows builds use the system TLS stack (SChannel) and its certificate store.
    #[cfg(windows)]
    let builder = builder.tls_connector(std::sync::Arc::new(
        native_tls::TlsConnector::new().map_err(|e| e.to_string())?,
    ));
    Ok(builder
        .timeout(timeout)
        .user_agent(concat!("acxs-manager/", env!("CARGO_PKG_VERSION")))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_ignores_case_like_windows_paths() {
        let text = r"panicked at c:\users\leon\src\main.rs and C:\USERS\LEON\x";
        assert_eq!(scrub(text, Some(r"C:\Users\Leon")), r"panicked at ~\src\main.rs and ~\x");
    }

    #[test]
    fn default_dsn_is_configured() {
        let dsn = Dsn::parse(DEFAULT_DSN).expect("DEFAULT_DSN parses");
        assert_eq!(dsn.store_url(), "https://test6.azendix.de/api/1/store/");
    }

    /// A directory under the temp folder, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!("acxs-crash-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample_report() -> CrashReport {
        CrashReport {
            message: "index out of bounds".into(),
            location: Some("src/ui/pages.rs:10:5".into()),
            backtrace: "0: acxs_manager::foo".into(),
            app_version: "0.3.0".into(),
            os: "linux x86_64".into(),
            timestamp: "2026-10-17T12:00:00.000Z".into(),
        }
    }

    #[test]
    fn parses_a_plain_dsn() {
        let dsn = Dsn::parse("https://abc123@glitch.example.com/42").unwrap();
        assert_eq!(dsn.scheme, "https");
        assert_eq!(dsn.public_key, "abc123");
        assert_eq!(dsn.host, "glitch.example.com");
        assert_eq!(dsn.project_id, "42");
        assert_eq!(dsn.path_prefix, "");
    }

    #[test]
    fn parses_a_port() {
        let dsn = Dsn::parse("https://abc123@glitch.example.com:8443/7").unwrap();
        assert_eq!(dsn.host, "glitch.example.com:8443");
        assert_eq!(dsn.project_id, "7");
    }

    #[test]
    fn parses_a_path_prefix() {
        let dsn = Dsn::parse("https://abc123@example.com/glitchtip/5").unwrap();
        assert_eq!(dsn.path_prefix, "/glitchtip");
        assert_eq!(dsn.project_id, "5");
        assert_eq!(dsn.store_url(), "https://example.com/glitchtip/api/5/store/");
    }

    #[test]
    fn rejects_bad_dsns() {
        assert!(Dsn::parse("http://abc123@example.com/42").is_err());
        assert!(Dsn::parse("__GLITCHTIP_DSN__").is_err());
        assert!(Dsn::parse("https://example.com/42").is_err());
        assert!(Dsn::parse("https://@example.com/42").is_err());
        assert!(Dsn::parse("https://abc123@example.com/").is_err());
        assert!(Dsn::parse("https://abc123@example.com").is_err());
        assert!(Dsn::parse("https://abc123@example.com/abc").is_err());
        assert!(Dsn::parse("https://abc123@example.com:/42").is_err());
    }

    #[test]
    fn store_url_and_auth_header_text() {
        let dsn = Dsn::parse("https://abc123@glitch.example.com/42").unwrap();
        assert_eq!(dsn.store_url(), "https://glitch.example.com/api/42/store/");
        assert_eq!(
            dsn.auth_header(),
            format!(
                "Sentry sentry_version=7, sentry_client=acxs-manager/{}, sentry_key=abc123",
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    #[test]
    fn event_has_the_sentry_fields() {
        let event = event_json_with_home(&sample_report(), None);
        let event_id = event["event_id"].as_str().unwrap();
        assert_eq!(event_id.len(), 32);
        assert!(event_id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(event["level"], "fatal");
        assert_eq!(event["release"], "acxs-manager@0.3.0");
        assert_eq!(event["platform"], "native");
        assert_eq!(event["logger"], "panic");
        assert_eq!(event["timestamp"], "2026-10-17T12:00:00.000Z");
        assert_eq!(event["tags"]["os"], "linux x86_64");
        assert_eq!(event["exception"]["values"][0]["type"], "panic");
        assert_eq!(event["exception"]["values"][0]["value"], "index out of bounds");
        assert_eq!(event["extra"]["location"], "src/ui/pages.rs:10:5");
        assert_eq!(event["extra"]["backtrace"], "0: acxs_manager::foo");
    }

    #[test]
    fn event_id_depends_on_message_and_time() {
        let a = event_json_with_home(&sample_report(), None);
        let mut other = sample_report();
        other.timestamp = "2026-10-17T12:00:01.000Z".into();
        let b = event_json_with_home(&other, None);
        assert_ne!(a["event_id"], b["event_id"]);
    }

    #[test]
    fn event_scrubs_the_home_folder() {
        let mut report = sample_report();
        report.location = Some("/home/leon/acxs/src/main.rs:3:1".into());
        report.backtrace = "at /home/leon/acxs/src/x.rs\nat /usr/lib/y.rs".into();
        let event = event_json_with_home(&report, Some("/home/leon"));
        assert_eq!(event["extra"]["location"], "~/acxs/src/main.rs:3:1");
        assert_eq!(event["extra"]["backtrace"], "at ~/acxs/src/x.rs\nat /usr/lib/y.rs");
    }

    #[test]
    fn scrub_replaces_home_only_at_a_path_boundary() {
        assert_eq!(scrub("/home/me/x", Some("/home/me")), "~/x");
        assert_eq!(scrub("/home/mev/x", Some("/home/me")), "/home/mev/x");
        assert_eq!(scrub("C:\\Users\\me\\x.rs", Some("C:\\Users\\me")), "~\\x.rs");
        assert_eq!(scrub("C:/Users/me/x.rs", Some("C:\\Users\\me")), "~/x.rs");
        assert_eq!(scrub("in /home/me", Some("/home/me/")), "in ~");
        assert_eq!(scrub("/home/me/a /home/me/b", Some("/home/me")), "~/a ~/b");
    }

    #[test]
    fn scrub_without_a_home_changes_nothing() {
        assert_eq!(scrub("/home/me/x", None), "/home/me/x");
        assert_eq!(scrub("/home/me/x", Some("")), "/home/me/x");
        assert_eq!(scrub("/home/me/x", Some("/")), "/home/me/x");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = TempDir::new("round-trip");
        let report = sample_report();
        let first = save_pending(&dir.0.join("crash-reports"), &report).unwrap();
        let second = save_pending(&dir.0.join("crash-reports"), &report).unwrap();
        assert_ne!(first, second, "a second save must not overwrite the first");

        let loaded = load_pending(&dir.0.join("crash-reports"));
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().all(|(_, r)| *r == report));
    }

    #[test]
    fn load_skips_unparsable_files_and_missing_dirs() {
        let dir = TempDir::new("skip-bad");
        assert!(load_pending(&dir.0).is_empty());
        fs::create_dir_all(&dir.0).unwrap();
        fs::write(dir.0.join("broken.json"), "{ not json").unwrap();
        fs::write(dir.0.join("notes.txt"), "ignored").unwrap();
        assert!(load_pending(&dir.0).is_empty());
    }

    #[test]
    fn pending_dir_is_inside_settings_dir() {
        assert_eq!(
            pending_dir(Path::new("/cfg/acxs")),
            Path::new("/cfg/acxs").join("crash-reports")
        );
    }

    #[test]
    fn send_with_nothing_pending_sends_nothing() {
        let dir = TempDir::new("nothing-pending");
        let dsn = Dsn::parse("https://abc123@glitch.example.com/42").unwrap();
        assert_eq!(send_pending(&dsn, &dir.0, Duration::from_secs(1)), Ok(0));
    }
}
