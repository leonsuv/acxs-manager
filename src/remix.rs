//! RTX Remix runtime management for an Assetto Corsa folder.
//!
//! The runtime is a Direct3D 9 DLL that sits next to the game executable,
//! optionally with a bridge executable and an `rtx.conf` file of
//! `key = value` lines. This module finds those files, installs them from a
//! user supplied build folder, removes them again, and reads, edits and
//! validates `rtx.conf`.
//!
//! Nothing is ever deleted. Files that an install or uninstall would replace
//! or remove are renamed into `<AC root>/acxs-remix-archive/<stamp>/`, and
//! `rtx.conf` backups are renamed beside the original.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;

use crate::settings::AC_EXECUTABLE;

const RUNTIME_DLL: &str = "d3d9.dll";
const BRIDGE_EXE: &str = "NvRemixBridge.exe";
const CONFIG_FILE: &str = "rtx.conf";
const TREX_DIR: &str = ".trex";
const ARCHIVE_DIR: &str = "acxs-remix-archive";
const STAMP_FORMAT: &str = "%Y%m%d-%H%M%S";

/// A Remix runtime found in an Assetto Corsa folder.
#[derive(Debug, Clone)]
pub struct RemixInstall {
    pub runtime_dll: PathBuf,
    pub config: Option<PathBuf>,
    pub bridge: Option<PathBuf>,
    pub version: Option<String>,
}

/// What an install copied and what it moved out of the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    /// File names copied into the Assetto Corsa folder.
    pub files: Vec<String>,
    /// The archive folder holding replaced files, if any file was replaced.
    pub archived_to: Option<PathBuf>,
}

#[derive(Debug)]
pub enum RemixError {
    GameRunning,
    NotAcRoot(PathBuf),
    NoRuntimeInBuild(PathBuf),
    Io(io::Error),
}

impl fmt::Display for RemixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemixError::GameRunning => {
                write!(f, "Assetto Corsa is running; close it before changing the Remix runtime")
            }
            RemixError::NotAcRoot(path) => {
                write!(f, "{} is not an Assetto Corsa folder ({AC_EXECUTABLE} not found)", path.display())
            }
            RemixError::NoRuntimeInBuild(path) => {
                write!(f, "no Remix runtime ({RUNTIME_DLL}) found in {}", path.display())
            }
            RemixError::Io(err) => write!(f, "file error: {err}"),
        }
    }
}

impl Error for RemixError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RemixError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for RemixError {
    fn from(err: io::Error) -> Self {
        RemixError::Io(err)
    }
}

/// Finds the runtime DLL, `rtx.conf` and bridge executable in an AC folder.
/// Returns None when there is no runtime DLL; the other two are optional.
pub fn detect(ac_root: &Path) -> Option<RemixInstall> {
    let runtime_dll = ac_root.join(RUNTIME_DLL);
    if !runtime_dll.is_file() {
        return None;
    }
    let version = file_version(&runtime_dll);
    Some(RemixInstall {
        runtime_dll,
        config: existing_file(ac_root.join(CONFIG_FILE)),
        bridge: existing_file(ac_root.join(BRIDGE_EXE)),
        version,
    })
}

fn existing_file(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

/// Copies the runtime DLL, and the bridge executable when the build folder has
/// one, into the AC folder. The runtime is looked up in `.trex/` first and then
/// in the folder itself. Files already in the AC folder are archived first.
/// `rtx.conf` and the user's other files are never touched.
pub fn install(ac_root: &Path, build_dir: &Path, game_running: bool) -> Result<InstallReport, RemixError> {
    if game_running {
        return Err(RemixError::GameRunning);
    }
    require_ac_root(ac_root)?;

    let runtime = find_in_build(build_dir, RUNTIME_DLL)
        .ok_or_else(|| RemixError::NoRuntimeInBuild(build_dir.to_path_buf()))?;
    let mut sources = vec![(RUNTIME_DLL, runtime)];
    if let Some(bridge) = find_in_build(build_dir, BRIDGE_EXE) {
        sources.push((BRIDGE_EXE, bridge));
    }

    let mut archive = Archive::new(ac_root);
    let mut files = Vec::new();
    for (name, src) in sources {
        let dst = ac_root.join(name);
        if same_file(&src, &dst) {
            continue;
        }
        archive.put_aside(&dst)?;
        fs::copy(&src, &dst)?;
        files.push(name.to_string());
    }
    Ok(InstallReport {
        files,
        archived_to: archive.dir,
    })
}

/// Moves the runtime DLL and bridge out of the AC folder into an archive folder.
/// Returns the archive folder, or None when neither file was present.
pub fn uninstall(ac_root: &Path, game_running: bool) -> Result<Option<PathBuf>, RemixError> {
    if game_running {
        return Err(RemixError::GameRunning);
    }
    require_ac_root(ac_root)?;

    let mut archive = Archive::new(ac_root);
    for name in [RUNTIME_DLL, BRIDGE_EXE] {
        archive.put_aside(&ac_root.join(name))?;
    }
    Ok(archive.dir)
}

fn require_ac_root(ac_root: &Path) -> Result<(), RemixError> {
    if ac_root.join(AC_EXECUTABLE).is_file() {
        Ok(())
    } else {
        Err(RemixError::NotAcRoot(ac_root.to_path_buf()))
    }
}

fn find_in_build(build_dir: &Path, name: &str) -> Option<PathBuf> {
    [build_dir.join(TREX_DIR).join(name), build_dir.join(name)]
        .into_iter()
        .find(|path| path.is_file())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Moves files into one archive folder per run. The folder is created only
/// when the first file is actually moved.
struct Archive {
    base: PathBuf,
    stamp: String,
    dir: Option<PathBuf>,
}

impl Archive {
    fn new(ac_root: &Path) -> Self {
        Self {
            base: ac_root.join(ARCHIVE_DIR),
            stamp: Local::now().format(STAMP_FORMAT).to_string(),
            dir: None,
        }
    }

    /// Renames `path` into the archive. Returns the new location, or None when
    /// `path` does not exist.
    fn put_aside(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        if !path.is_file() {
            return Ok(None);
        }
        let dir = match &self.dir {
            Some(dir) => dir.clone(),
            None => {
                let dir = self.base.join(&self.stamp);
                fs::create_dir_all(&dir)?;
                self.dir = Some(dir.clone());
                dir
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        let target = unique_path(&dir, &name);
        fs::rename(path, &target)?;
        Ok(Some(target))
    }
}

/// `dir/name` if free, otherwise `dir/stem.N.ext` for the first free N.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let mut n = 1u32;
    loop {
        let candidate = dir.join(format!("{stem}.{n}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Reads the file version from the fixed version block of a PE file.
fn file_version(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    file_version_from_bytes(&bytes)
}

/// Scans for the VS_FIXEDFILEINFO signature (0xFEEF04BD, little endian) and
/// reads the file version that follows it as `a.b.c.d`.
fn file_version_from_bytes(bytes: &[u8]) -> Option<String> {
    const SIGNATURE: [u8; 4] = [0xBD, 0x04, 0xEF, 0xFE];
    (0..bytes.len().saturating_sub(3))
        .filter(|&at| bytes[at..at + 4] == SIGNATURE)
        .find_map(|at| {
            let word = |offset: usize| -> Option<u32> {
                let b = bytes.get(at + offset..at + offset + 4)?;
                Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            };
            if word(4)? != 0x0001_0000 {
                return None;
            }
            let (ms, ls) = (word(8)?, word(12)?);
            if ms == 0 && ls == 0 {
                return None;
            }
            Some(format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF))
        })
}

/// One line of `rtx.conf`. Comments, blank lines and section headers are kept
/// as text so that rewriting the file does not disturb them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Text(String),
    Entry {
        key: String,
        value: String,
        /// The line as it should be written out.
        text: String,
    },
}

/// An ordered `rtx.conf` that keeps comments, blank lines and key order.
///
/// Lines are `key = value`. A line starting with `#` is a comment. Keys are
/// case-sensitive, and when a key appears more than once the last entry wins,
/// as in the runtime's own reader.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RtxConfig {
    lines: Vec<Line>,
    crlf: bool,
    bom: bool,
}

impl RtxConfig {
    /// Parses config text. Unchanged lines are written back exactly as read.
    pub fn parse(text: &str) -> Self {
        let bom = text.starts_with('\u{feff}');
        let body = text.strip_prefix('\u{feff}').unwrap_or(text);
        Self {
            lines: body.lines().map(parse_line).collect(),
            crlf: body.contains("\r\n"),
            bom,
        }
    }

    /// The text to write to disk.
    pub fn render(&self) -> String {
        let newline = if self.crlf { "\r\n" } else { "\n" };
        let mut out = String::new();
        if self.bom {
            out.push('\u{feff}');
        }
        for line in &self.lines {
            match line {
                Line::Text(text) | Line::Entry { text, .. } => out.push_str(text),
            }
            out.push_str(newline);
        }
        out
    }

    /// The value of the last entry for `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().rev().find_map(|line| match line {
            Line::Entry { key: k, value, .. } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Sets `key` to `value`. Every existing entry for the key is updated;
    /// a new key is appended at the end. Line breaks in the value become spaces.
    pub fn set(&mut self, key: &str, value: &str) {
        let value: String = value
            .chars()
            .map(|c| if matches!(c, '\r' | '\n') { ' ' } else { c })
            .collect();
        let text = format!("{key} = {value}");
        let mut found = false;
        for line in &mut self.lines {
            if let Line::Entry {
                key: existing,
                value: current,
                text: current_text,
            } = line
            {
                if existing.as_str() == key {
                    *current = value.clone();
                    *current_text = text.clone();
                    found = true;
                }
            }
        }
        if !found {
            self.lines.push(Line::Entry {
                key: key.to_string(),
                value,
                text,
            });
        }
    }

    /// Every `key = value` entry in file order, including duplicates.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.lines.iter().filter_map(|line| match line {
            Line::Entry { key, value, .. } => Some((key.as_str(), value.as_str())),
            Line::Text(_) => None,
        })
    }
}

fn parse_line(raw: &str) -> Line {
    let line = raw.trim_end_matches('\r');
    let trimmed = line.trim_start_matches([' ', '\t']);
    if trimmed.starts_with('#') || trimmed.starts_with('[') {
        return Line::Text(line.to_string());
    }
    let key_len = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
        .unwrap_or(trimmed.len());
    let key = &trimmed[..key_len];
    if key.is_empty() {
        return Line::Text(line.to_string());
    }
    let rest = trimmed[key_len..].trim_start_matches([' ', '\t', '\r']);
    let Some(raw_value) = rest.strip_prefix('=') else {
        return Line::Text(line.to_string());
    };
    // The runtime drops double quotes from values, so do the same.
    let value: String = raw_value.chars().filter(|&c| c != '"').collect();
    Line::Entry {
        key: key.to_string(),
        value: value.trim().to_string(),
        text: line.to_string(),
    }
}

/// Reads `rtx.conf` from `path`.
pub fn read_rtx(path: &Path) -> io::Result<RtxConfig> {
    let text = fs::read_to_string(path)?;
    Ok(RtxConfig::parse(&text))
}

/// Writes `cfg` to `path`. An existing file is renamed to a `.bak` name beside
/// it first, and the new text is staged under a separate name, so a failure
/// leaves the original in place or in the backup.
pub fn write_rtx(path: &Path, cfg: &RtxConfig) -> io::Result<()> {
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let stamp = Local::now().format(STAMP_FORMAT).to_string();

    let staged = unique_path(dir, &format!("{name}.{stamp}.new"));
    fs::write(&staged, cfg.render())?;

    let backup = if path.is_file() {
        let backup = unique_path(dir, &format!("{name}.{stamp}.bak"));
        fs::rename(path, &backup)?;
        Some(backup)
    } else {
        None
    };

    if let Err(err) = fs::rename(&staged, path) {
        if let Some(backup) = &backup {
            let _ = fs::rename(backup, path);
        }
        return Err(err);
    }
    Ok(())
}

/// A quality level, named after the runtime's upscaler presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    Performance,
    Balanced,
    Quality,
}

/// Sets the keys that express a quality level. Other keys are left alone.
///
/// | Key | Performance | Balanced | Quality |
/// |-----|-------------|----------|---------|
/// | `rtx.nisPreset` | 0 | 1 | 2 |
/// | `rtx.taauPreset` | 1 | 2 | 3 |
/// | `rtx.resolutionScale` | 0.5 | 0.66 | 0.75 |
/// | `rtx.neuralRadianceCache.qualityPreset` | 0 (Medium) | 1 (High) | 2 (Ultra) |
///
/// The NIS and TAA-U values are the upscaler preset numbers, and the
/// resolution scale is what each of those presets applies. The neural radiance
/// cache levels come from the runtime's option documentation.
pub fn apply_quality(cfg: &mut RtxConfig, preset: QualityPreset) {
    let (nis, taau, scale, nrc) = match preset {
        QualityPreset::Performance => (0, 1, "0.5", 0),
        QualityPreset::Balanced => (1, 2, "0.66", 1),
        QualityPreset::Quality => (2, 3, "0.75", 2),
    };
    cfg.set("rtx.nisPreset", &nis.to_string());
    cfg.set("rtx.taauPreset", &taau.to_string());
    cfg.set("rtx.resolutionScale", scale);
    cfg.set("rtx.neuralRadianceCache.qualityPreset", &nrc.to_string());
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Bool,
    /// Integer in the inclusive range `min..=max`.
    Int { min: i64, max: i64 },
    /// Finite number greater than zero.
    PositiveFloat,
    /// Three comma separated finite numbers.
    Float3,
    /// Comma separated hashes such as `0x8DD6F568BD126398`, optionally with a leading `-`.
    HashSet,
}

/// Keys whose type and range come from the runtime's option documentation.
const KNOWN_KEYS: &[(&str, Kind)] = &[
    ("rtx.enableRaytracing", Kind::Bool),
    ("rtx.lowMemoryGpu", Kind::Bool),
    ("rtx.autoExposure.enabled", Kind::Bool),
    ("rtx.graphicsPreset", Kind::Int { min: 0, max: 5 }),
    ("rtx.dlssPreset", Kind::Int { min: 0, max: 2 }),
    ("rtx.nisPreset", Kind::Int { min: 0, max: 3 }),
    ("rtx.taauPreset", Kind::Int { min: 0, max: 4 }),
    ("rtx.neuralRadianceCache.qualityPreset", Kind::Int { min: 0, max: 2 }),
    ("rtx.resolutionScale", Kind::PositiveFloat),
    ("rtx.fallbackLightDirection", Kind::Float3),
    ("rtx.fallbackLightPrimaryAxis", Kind::Float3),
    ("rtx.effectLightColor", Kind::Float3),
    ("rtx.camera.freeCameraPosition", Kind::Float3),
    ("rtx.ignoreTextures", Kind::HashSet),
];

/// Lists problems with known keys: empty values, values of the wrong type,
/// and values outside the documented range. Unknown keys are not checked.
/// An empty list means no problems were found.
pub fn validate(cfg: &RtxConfig) -> Vec<String> {
    cfg.entries()
        .filter_map(|(key, value)| {
            let (_, kind) = KNOWN_KEYS.iter().find(|(known, _)| *known == key)?;
            check_value(*kind, value).map(|problem| format!("{key}: {problem}"))
        })
        .collect()
}

fn check_value(kind: Kind, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return Some("value is empty".to_string());
    }
    match kind {
        Kind::Bool => {
            let lower = value.to_ascii_lowercase();
            (!matches!(lower.as_str(), "true" | "false" | "1" | "0"))
                .then(|| format!("\"{value}\" is not a bool (True or False)"))
        }
        Kind::Int { min, max } => match value.parse::<i64>() {
            Ok(n) if (min..=max).contains(&n) => None,
            Ok(n) => Some(format!("{n} is outside the range {min}..={max}")),
            Err(_) => Some(format!("\"{value}\" is not an integer")),
        },
        Kind::PositiveFloat => match value.parse::<f32>() {
            Ok(v) if v.is_finite() && v > 0.0 => None,
            Ok(_) => Some(format!("{value} must be a positive number")),
            Err(_) => Some(format!("\"{value}\" is not a number")),
        },
        Kind::Float3 => {
            let parts: Vec<&str> = value.split(',').map(str::trim).collect();
            if parts.len() != 3 {
                Some(format!("expected 3 comma separated numbers, found {}", parts.len()))
            } else if parts.iter().all(|p| p.parse::<f32>().is_ok_and(|v| v.is_finite())) {
                None
            } else {
                Some(format!("\"{value}\" contains a value that is not a finite number"))
            }
        }
        Kind::HashSet => value.split(',').map(str::trim).find_map(|item| {
            let unsigned = item.strip_prefix('-').unwrap_or(item);
            let hex = unsigned
                .strip_prefix("0x")
                .or_else(|| unsigned.strip_prefix("0X"));
            match hex {
                Some(h) if !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()) => None,
                _ => Some(format!("\"{item}\" is not a 0x hash")),
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp folder that is removed on drop, even when a test panics.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "acxs-remix-test-{}-{}",
                std::process::id(),
                label
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn make_ac_root(root: &Path) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        write(&root.join(AC_EXECUTABLE), b"exe");
        root.to_path_buf()
    }

    /// Bytes that contain a version block, enough for the version reader.
    fn fake_version_block(ms: u32, ls: u32, struct_version: u32) -> Vec<u8> {
        let mut bytes = b"MZ fake header ".to_vec();
        bytes.extend_from_slice(&[0xBD, 0x04, 0xEF, 0xFE]);
        bytes.extend_from_slice(&struct_version.to_le_bytes());
        bytes.extend_from_slice(&ms.to_le_bytes());
        bytes.extend_from_slice(&ls.to_le_bytes());
        bytes
    }

    #[test]
    fn detect_finds_runtime_config_and_bridge() {
        let tmp = TempDir::new("detect");
        let ac = make_ac_root(&tmp.0.join("ac"));
        assert!(detect(&ac).is_none());

        write(&ac.join(RUNTIME_DLL), &fake_version_block((1 << 16) | 2, (3 << 16) | 4, 0x0001_0000));
        let found = detect(&ac).expect("runtime present");
        assert_eq!(found.runtime_dll, ac.join(RUNTIME_DLL));
        assert_eq!(found.config, None);
        assert_eq!(found.bridge, None);
        assert_eq!(found.version.as_deref(), Some("1.2.3.4"));

        write(&ac.join(CONFIG_FILE), b"rtx.enableRaytracing = True\n");
        write(&ac.join(BRIDGE_EXE), b"bridge");
        let found = detect(&ac).expect("runtime present");
        assert_eq!(found.config, Some(ac.join(CONFIG_FILE)));
        assert_eq!(found.bridge, Some(ac.join(BRIDGE_EXE)));
    }

    #[test]
    fn version_reader_rejects_wrong_struct_version() {
        let good = fake_version_block(0x0002_0000, 0x0000_0001, 0x0001_0000);
        assert_eq!(file_version_from_bytes(&good).as_deref(), Some("2.0.0.1"));
        let bad = fake_version_block(0x0002_0000, 0x0000_0001, 0x0002_0000);
        assert_eq!(file_version_from_bytes(&bad), None);
        assert_eq!(file_version_from_bytes(b"no block here"), None);
    }

    #[test]
    fn install_copies_runtime_and_bridge() {
        let tmp = TempDir::new("install");
        let ac = make_ac_root(&tmp.0.join("ac"));
        let build = tmp.0.join("build");
        write(&build.join(TREX_DIR).join(RUNTIME_DLL), b"runtime v1");
        write(&build.join(TREX_DIR).join(BRIDGE_EXE), b"bridge v1");

        let report = install(&ac, &build, false).unwrap();
        assert_eq!(report.files, vec![RUNTIME_DLL.to_string(), BRIDGE_EXE.to_string()]);
        assert_eq!(report.archived_to, None);
        assert_eq!(fs::read(ac.join(RUNTIME_DLL)).unwrap(), b"runtime v1");
        assert_eq!(fs::read(ac.join(BRIDGE_EXE)).unwrap(), b"bridge v1");
    }

    #[test]
    fn install_without_bridge_copies_only_runtime() {
        let tmp = TempDir::new("install-no-bridge");
        let ac = make_ac_root(&tmp.0.join("ac"));
        let build = tmp.0.join("build");
        write(&build.join(RUNTIME_DLL), b"runtime");

        let report = install(&ac, &build, false).unwrap();
        assert_eq!(report.files, vec![RUNTIME_DLL.to_string()]);
        assert!(!ac.join(BRIDGE_EXE).exists());
    }

    #[test]
    fn reinstall_archives_replaced_files() {
        let tmp = TempDir::new("reinstall");
        let ac = make_ac_root(&tmp.0.join("ac"));
        let build = tmp.0.join("build");
        write(&build.join(TREX_DIR).join(RUNTIME_DLL), b"runtime v1");
        write(&build.join(TREX_DIR).join(BRIDGE_EXE), b"bridge v1");
        install(&ac, &build, false).unwrap();

        write(&build.join(TREX_DIR).join(RUNTIME_DLL), b"runtime v2");
        let second = install(&ac, &build, false).unwrap();
        let archive = second.archived_to.expect("replaced files are archived");
        assert!(archive.starts_with(ac.join(ARCHIVE_DIR)));
        assert_eq!(fs::read(archive.join(RUNTIME_DLL)).unwrap(), b"runtime v1");
        assert_eq!(fs::read(archive.join(BRIDGE_EXE)).unwrap(), b"bridge v1");
        assert_eq!(fs::read(ac.join(RUNTIME_DLL)).unwrap(), b"runtime v2");
    }

    #[test]
    fn install_keeps_the_user_config() {
        let tmp = TempDir::new("keep-config");
        let ac = make_ac_root(&tmp.0.join("ac"));
        write(&ac.join(CONFIG_FILE), b"rtx.exposure = 2\n");
        let build = tmp.0.join("build");
        write(&build.join(RUNTIME_DLL), b"runtime");

        install(&ac, &build, false).unwrap();
        assert_eq!(fs::read(ac.join(CONFIG_FILE)).unwrap(), b"rtx.exposure = 2\n");
    }

    #[test]
    fn install_refuses_while_game_runs() {
        let tmp = TempDir::new("game-running");
        let ac = make_ac_root(&tmp.0.join("ac"));
        let build = tmp.0.join("build");
        write(&build.join(RUNTIME_DLL), b"runtime");
        write(&ac.join(RUNTIME_DLL), b"keep me");

        assert!(matches!(install(&ac, &build, true), Err(RemixError::GameRunning)));
        assert!(matches!(uninstall(&ac, true), Err(RemixError::GameRunning)));
        assert_eq!(fs::read(ac.join(RUNTIME_DLL)).unwrap(), b"keep me");
        assert!(!ac.join(ARCHIVE_DIR).exists());
    }

    #[test]
    fn install_checks_ac_root_and_build_contents() {
        let tmp = TempDir::new("refusals");
        let ac = make_ac_root(&tmp.0.join("ac"));
        let plain = tmp.0.join("plain");
        fs::create_dir_all(&plain).unwrap();
        let empty_build = tmp.0.join("empty");
        fs::create_dir_all(&empty_build).unwrap();

        let err = install(&plain, &empty_build, false).unwrap_err();
        assert!(matches!(err, RemixError::NotAcRoot(_)));
        assert!(!err.to_string().is_empty());
        assert!(matches!(
            install(&ac, &empty_build, false),
            Err(RemixError::NoRuntimeInBuild(_))
        ));
    }

    #[test]
    fn uninstall_archives_runtime_and_keeps_config() {
        let tmp = TempDir::new("uninstall");
        let ac = make_ac_root(&tmp.0.join("ac"));
        write(&ac.join(RUNTIME_DLL), b"runtime");
        write(&ac.join(BRIDGE_EXE), b"bridge");
        write(&ac.join(CONFIG_FILE), b"rtx.enableRaytracing = True\n");

        let archive = uninstall(&ac, false).unwrap().expect("files were archived");
        assert!(!ac.join(RUNTIME_DLL).exists());
        assert!(!ac.join(BRIDGE_EXE).exists());
        assert_eq!(fs::read(archive.join(RUNTIME_DLL)).unwrap(), b"runtime");
        assert_eq!(fs::read(archive.join(BRIDGE_EXE)).unwrap(), b"bridge");
        assert!(ac.join(CONFIG_FILE).is_file());

        assert_eq!(uninstall(&ac, false).unwrap(), None);
    }

    #[test]
    fn rtx_round_trip_keeps_comments_and_order() {
        let tmp = TempDir::new("rtx-roundtrip");
        let path = tmp.0.join(CONFIG_FILE);
        let original = "# Mod settings\n\n[acs.exe]\nrtx.enableRaytracing = True\n# keep me\nrtx.exposure = 1.5\n";
        write(&path, original.as_bytes());

        let mut cfg = read_rtx(&path).unwrap();
        assert_eq!(cfg.render(), original);
        assert_eq!(cfg.get("rtx.exposure"), Some("1.5"));
        assert_eq!(cfg.get("rtx.enableRaytracing"), Some("True"));
        assert_eq!(cfg.get("RTX.enableRaytracing"), None);

        cfg.set("rtx.exposure", "2.0");
        cfg.set("rtx.newKey", "7");
        write_rtx(&path, &cfg).unwrap();

        let saved = fs::read_to_string(&path).unwrap();
        assert_eq!(
            saved,
            "# Mod settings\n\n[acs.exe]\nrtx.enableRaytracing = True\n# keep me\nrtx.exposure = 2.0\nrtx.newKey = 7\n"
        );
        assert_eq!(read_rtx(&path).unwrap().get("rtx.exposure"), Some("2.0"));

        let backups: Vec<PathBuf> = fs::read_dir(&tmp.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "bak"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(&backups[0]).unwrap(), original);
    }

    #[test]
    fn rtx_write_without_existing_file_makes_no_backup() {
        let tmp = TempDir::new("rtx-fresh");
        let path = tmp.0.join(CONFIG_FILE);
        let mut cfg = RtxConfig::default();
        cfg.set("rtx.enableRaytracing", "True");
        write_rtx(&path, &cfg).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "rtx.enableRaytracing = True\n");
        let count = fs::read_dir(&tmp.0).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    fn rtx_crlf_and_bom_survive_round_trip() {
        let text = "\u{feff}# first\r\nrtx.exposure = 1\r\n";
        let mut cfg = RtxConfig::parse(text);
        assert_eq!(cfg.render(), text);
        cfg.set("rtx.exposure", "3");
        assert_eq!(cfg.render(), "\u{feff}# first\r\nrtx.exposure = 3\r\n");
    }

    #[test]
    fn rtx_last_duplicate_wins_and_set_updates_all() {
        let mut cfg = RtxConfig::parse("rtx.exposure = 1\nrtx.exposure = 4\n");
        assert_eq!(cfg.get("rtx.exposure"), Some("4"));
        cfg.set("rtx.exposure", "5");
        assert_eq!(cfg.render(), "rtx.exposure = 5\nrtx.exposure = 5\n");
    }

    #[test]
    fn apply_quality_sets_documented_keys() {
        let mut cfg = RtxConfig::parse("# tuned by hand\nrtx.nisPreset = 3\nrtx.exposure = 1\n");

        apply_quality(&mut cfg, QualityPreset::Performance);
        assert_eq!(cfg.get("rtx.nisPreset"), Some("0"));
        assert_eq!(cfg.get("rtx.taauPreset"), Some("1"));
        assert_eq!(cfg.get("rtx.resolutionScale"), Some("0.5"));
        assert_eq!(cfg.get("rtx.neuralRadianceCache.qualityPreset"), Some("0"));

        apply_quality(&mut cfg, QualityPreset::Balanced);
        assert_eq!(cfg.get("rtx.nisPreset"), Some("1"));
        assert_eq!(cfg.get("rtx.taauPreset"), Some("2"));
        assert_eq!(cfg.get("rtx.resolutionScale"), Some("0.66"));
        assert_eq!(cfg.get("rtx.neuralRadianceCache.qualityPreset"), Some("1"));

        apply_quality(&mut cfg, QualityPreset::Quality);
        assert_eq!(cfg.get("rtx.nisPreset"), Some("2"));
        assert_eq!(cfg.get("rtx.taauPreset"), Some("3"));
        assert_eq!(cfg.get("rtx.resolutionScale"), Some("0.75"));
        assert_eq!(cfg.get("rtx.neuralRadianceCache.qualityPreset"), Some("2"));

        assert_eq!(cfg.get("rtx.exposure"), Some("1"));
        assert!(cfg.render().starts_with("# tuned by hand\n"));
        assert!(validate(&cfg).is_empty());
    }

    #[test]
    fn validate_flags_bad_values() {
        let cfg = RtxConfig::parse(
            "rtx.enableRaytracing = maybe\n\
             rtx.nisPreset = 9\n\
             rtx.resolutionScale =\n\
             rtx.fallbackLightDirection = 1, 2\n\
             rtx.ignoreTextures = 0x12AB, nope\n\
             rtx.unknownThing = whatever\n",
        );
        let problems = validate(&cfg);
        assert_eq!(problems.len(), 5, "{problems:?}");
        for key in [
            "rtx.enableRaytracing",
            "rtx.nisPreset",
            "rtx.resolutionScale",
            "rtx.fallbackLightDirection",
            "rtx.ignoreTextures",
        ] {
            assert!(problems.iter().any(|p| p.starts_with(key)), "missing {key}");
        }
    }

    #[test]
    fn validate_accepts_good_values() {
        let cfg = RtxConfig::parse(
            "rtx.enableRaytracing = True\n\
             rtx.lowMemoryGpu = 0\n\
             rtx.nisPreset = 1\n\
             rtx.resolutionScale = 0.75\n\
             rtx.fallbackLightDirection = -0.2, -1, 0.4\n\
             rtx.ignoreTextures = 0x8DD6F568BD126398, -0xEEF8EFD4B8A1B2A5\n",
        );
        assert!(validate(&cfg).is_empty());
    }
}
