//! Plugin manifests: a small JSON description of one plugin module file and the
//! SHA-256 digest it must have before it is loaded.
//!
//! A plugin folder holds a `plugin.json` next to the module file it names. The
//! manifest is validated when parsed, and [`verify`] hashes the module on disk
//! and compares the digest against the one the manifest declares.

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::csp::sha256_file;

/// File name of the manifest inside a plugin folder.
const MANIFEST_FILE: &str = "plugin.json";

/// Description of one plugin module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Human-readable plugin name. Must not be blank.
    pub name: String,
    /// Free-form version string.
    pub version: String,
    /// File name of the module inside the plugin folder. Must be a single
    /// path component.
    pub module: String,
    /// Expected SHA-256 of the module file, as 64 lowercase hex digits.
    pub sha256: String,
}

/// Why a manifest could not be parsed, verified or loaded.
#[derive(Debug)]
pub enum ManifestError {
    /// The text is not valid JSON or does not match the manifest shape.
    Json(serde_json::Error),
    /// The plugin name is empty or blank.
    EmptyName,
    /// The module name is not a plain file name (empty, `.`, `..`, or it
    /// contains a path separator).
    InvalidModuleName(String),
    /// The digest is not 64 lowercase hex digits.
    InvalidSha256(String),
    /// The module file hashes to something other than the manifest declares.
    ChecksumMismatch { expected: String, actual: String },
    /// Reading the manifest or the module file failed.
    Io(io::Error),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Json(e) => write!(f, "invalid plugin manifest: {e}"),
            ManifestError::EmptyName => write!(f, "plugin name is empty"),
            ManifestError::InvalidModuleName(name) => write!(
                f,
                "invalid module file name {name:?}: must be a single file name without path separators"
            ),
            ManifestError::InvalidSha256(digest) => write!(
                f,
                "invalid sha256 {digest:?}: expected 64 lowercase hex digits"
            ),
            ManifestError::ChecksumMismatch { expected, actual } => write!(
                f,
                "module checksum mismatch: manifest declares {expected}, file hashes to {actual}"
            ),
            ManifestError::Io(e) => write!(f, "plugin I/O error: {e}"),
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ManifestError::Json(e) => Some(e),
            ManifestError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for ManifestError {
    fn from(e: serde_json::Error) -> Self {
        ManifestError::Json(e)
    }
}

impl From<io::Error> for ManifestError {
    fn from(e: io::Error) -> Self {
        ManifestError::Io(e)
    }
}

/// Parse and validate a manifest from JSON text.
pub fn parse(json: &str) -> Result<PluginManifest, ManifestError> {
    let manifest: PluginManifest = serde_json::from_str(json)?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Hash `plugin_dir/<module>` and check it against the manifest's digest.
pub fn verify(manifest: &PluginManifest, plugin_dir: &Path) -> Result<(), ManifestError> {
    // Re-check here too: a manifest may have been built in code, not parsed.
    validate(manifest)?;
    let actual = sha256_file(&plugin_dir.join(&manifest.module))?;
    if actual == manifest.sha256 {
        Ok(())
    } else {
        Err(ManifestError::ChecksumMismatch {
            expected: manifest.sha256.clone(),
            actual,
        })
    }
}

/// Read and parse `plugin_dir/plugin.json`.
pub fn load_from_dir(plugin_dir: &Path) -> Result<PluginManifest, ManifestError> {
    let text = fs::read_to_string(plugin_dir.join(MANIFEST_FILE))?;
    parse(&text)
}

/// Load the manifest of every immediate subfolder of `root` that contains a
/// `plugin.json`. Results are sorted by folder path. A broken manifest shows up
/// as an `Err` entry rather than stopping the scan. A missing or unreadable
/// `root` yields an empty list.
pub fn list_plugins(root: &Path) -> Vec<(PathBuf, Result<PluginManifest, ManifestError>)> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(MANIFEST_FILE).is_file())
        .collect();
    folders.sort();
    folders
        .into_iter()
        .map(|folder| {
            let result = load_from_dir(&folder);
            (folder, result)
        })
        .collect()
}

fn validate(manifest: &PluginManifest) -> Result<(), ManifestError> {
    if manifest.name.trim().is_empty() {
        return Err(ManifestError::EmptyName);
    }
    validate_module_name(&manifest.module)?;
    validate_sha256(&manifest.sha256)
}

/// Accept only a single normal path component: no separators, and not `.`,
/// `..` or an empty string.
fn validate_module_name(module: &str) -> Result<(), ManifestError> {
    let plain = !module.is_empty()
        && !module.contains(['/', '\\'])
        && Path::new(module).file_name() == Some(OsStr::new(module));
    if plain {
        Ok(())
    } else {
        Err(ManifestError::InvalidModuleName(module.to_string()))
    }
}

fn validate_sha256(digest: &str) -> Result<(), ManifestError> {
    let well_formed = digest.len() == 64
        && digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if well_formed {
        Ok(())
    } else {
        Err(ManifestError::InvalidSha256(digest.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of the bytes `hello`.
    const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn manifest_text(module: &str, sha256: &str) -> String {
        serde_json::json!({
            "name": "Test Plugin",
            "version": "1.2.3",
            "module": module,
            "sha256": sha256,
        })
        .to_string()
    }

    /// A scratch folder under the system temp dir, removed on drop.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "acxs_plugin_manifest_{}_{}",
                std::process::id(),
                tag
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create scratch dir");
            ScratchDir(path)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parse_accepts_valid_manifest() {
        let m = parse(&manifest_text("plugin.dll", HELLO_SHA256)).expect("valid manifest");
        assert_eq!(m.name, "Test Plugin");
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.module, "plugin.dll");
        assert_eq!(m.sha256, HELLO_SHA256);
    }

    #[test]
    fn parse_rejects_invalid_manifests() {
        let cases = [
            serde_json::json!({"name": "  ", "version": "1", "module": "a.dll", "sha256": HELLO_SHA256}).to_string(),
            serde_json::json!({"name": "x", "version": "1", "module": "", "sha256": HELLO_SHA256}).to_string(),
            serde_json::json!({"name": "x", "version": "1", "module": ".", "sha256": HELLO_SHA256}).to_string(),
            serde_json::json!({"name": "x", "version": "1", "module": "a.dll", "sha256": HELLO_SHA256.to_uppercase()}).to_string(),
            serde_json::json!({"name": "x", "version": "1", "module": "a.dll", "sha256": "abc"}).to_string(),
            serde_json::json!({"name": "x", "version": "1", "module": "a.dll", "sha256": "g".repeat(64)}).to_string(),
            serde_json::json!({"name": "x", "version": "1", "module": "a.dll"}).to_string(),
            "{ not json".to_string(),
        ];
        for text in &cases {
            assert!(parse(text).is_err(), "should reject: {text}");
        }
        assert!(matches!(
            parse(&cases[0]),
            Err(ManifestError::EmptyName)
        ));
        assert!(matches!(
            parse(&cases[3]),
            Err(ManifestError::InvalidSha256(_))
        ));
        assert!(matches!(parse(&cases[7]), Err(ManifestError::Json(_))));
    }

    #[test]
    fn parse_rejects_traversal_in_module_name() {
        for module in ["../evil.dll", "..", "sub/x.dll", "..\\..\\evil.dll", "/etc/passwd"] {
            let text = manifest_text(module, HELLO_SHA256);
            assert!(
                matches!(parse(&text), Err(ManifestError::InvalidModuleName(_))),
                "should reject module {module:?}"
            );
        }
    }

    #[test]
    fn verify_accepts_matching_checksum() {
        let dir = ScratchDir::new("verify_ok");
        fs::write(dir.0.join("plugin.dll"), b"hello").unwrap();
        let m = parse(&manifest_text("plugin.dll", HELLO_SHA256)).unwrap();
        assert!(verify(&m, &dir.0).is_ok());
    }

    #[test]
    fn verify_reports_mismatch() {
        let dir = ScratchDir::new("verify_mismatch");
        fs::write(dir.0.join("plugin.dll"), b"hello").unwrap();
        let wrong = "0".repeat(64);
        let m = parse(&manifest_text("plugin.dll", &wrong)).unwrap();
        match verify(&m, &dir.0) {
            Err(ManifestError::ChecksumMismatch { expected, actual }) => {
                assert_eq!(expected, wrong);
                assert_eq!(actual, HELLO_SHA256);
            }
            other => panic!("expected checksum mismatch, got {other:?}"),
        }
    }

    #[test]
    fn verify_missing_module_is_io_error() {
        let dir = ScratchDir::new("verify_missing");
        let m = parse(&manifest_text("absent.dll", HELLO_SHA256)).unwrap();
        assert!(matches!(verify(&m, &dir.0), Err(ManifestError::Io(_))));
    }

    #[test]
    fn load_from_dir_reads_plugin_json() {
        let dir = ScratchDir::new("load");
        fs::write(dir.0.join("plugin.json"), manifest_text("plugin.dll", HELLO_SHA256)).unwrap();
        let m = load_from_dir(&dir.0).expect("manifest loads");
        assert_eq!(m.module, "plugin.dll");
    }

    #[test]
    fn list_plugins_reports_broken_manifest() {
        let root = ScratchDir::new("list");
        let good = root.0.join("good");
        let broken = root.0.join("broken");
        let empty = root.0.join("no_manifest");
        for d in [&good, &broken, &empty] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(good.join("plugin.json"), manifest_text("plugin.dll", HELLO_SHA256)).unwrap();
        fs::write(broken.join("plugin.json"), "{ broken").unwrap();

        let listed = list_plugins(&root.0);
        assert_eq!(listed.len(), 2, "folder without manifest is skipped");

        let (path, result) = &listed[0];
        assert_eq!(path, &broken);
        assert!(matches!(result, Err(ManifestError::Json(_))));

        let (path, result) = &listed[1];
        assert_eq!(path, &good);
        assert!(result.is_ok());
    }

    #[test]
    fn list_plugins_missing_root_is_empty() {
        let missing = std::env::temp_dir().join(format!(
            "acxs_plugin_manifest_{}_missing_root",
            std::process::id()
        ));
        assert!(list_plugins(&missing).is_empty());
    }
}
