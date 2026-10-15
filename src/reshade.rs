//! Manage a ReShade build folder for Assetto Corsa.
//!
//! The user points the app at a folder that holds a built `ReShade64.dll`.
//! Installing copies it into the AC root under the name of a graphics-API
//! proxy (`dxgi.dll` by default), which is how the game loads it. Any file
//! replaced or removed is moved into `acxs-reshade-archive/<stamp>/` inside the
//! AC root. Nothing is deleted.
//!
//! Settings live in `ReShade.ini` in the AC root and are read and written
//! through [`crate::ini::Ini`], so keys this module does not know about survive
//! a save. Presets are `.ini` files in `reshade-presets/` inside the AC root.

use crate::ini::Ini;
use crate::settings::is_ac_root;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Module file in the build folder.
pub const BUILD_MODULE: &str = "ReShade64.dll";
/// Proxy name used when installing. DXGI is the proxy for DirectX 10 to 12.
pub const DEFAULT_PROXY: &str = "dxgi.dll";
/// Names a proxy can take in the AC root, in detection order.
pub const PROXY_NAMES: [&str; 7] = [
    "dxgi.dll",
    "d3d11.dll",
    "d3d12.dll",
    "d3d10.dll",
    "d3d9.dll",
    "opengl32.dll",
    "dinput8.dll",
];
/// Settings file in the AC root.
pub const CONFIG_NAME: &str = "ReShade.ini";
/// Preset folder inside the AC root.
pub const PRESET_DIR: &str = "reshade-presets";
/// Folder that receives replaced or removed files.
pub const ARCHIVE_DIR: &str = "acxs-reshade-archive";

#[derive(Debug, Clone)]
pub struct ReshadeInstall {
    pub proxy_dll: PathBuf,
    pub proxy_name: String,
    pub config: Option<PathBuf>,
    pub presets: Vec<PathBuf>,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub proxy_name: String,
    /// Archive folder that received the file this install replaced, if any.
    pub archived_to: Option<PathBuf>,
}

#[derive(Debug)]
pub enum ReshadeError {
    GameRunning,
    NotAcRoot(PathBuf),
    NoProxyInBuild(PathBuf),
    Io(io::Error),
}

impl fmt::Display for ReshadeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReshadeError::GameRunning => {
                write!(f, "Assetto Corsa is running. Close the game before changing ReShade.")
            }
            ReshadeError::NotAcRoot(path) => {
                write!(f, "{} is not an Assetto Corsa folder (acs.exe not found).", path.display())
            }
            ReshadeError::NoProxyInBuild(path) => {
                write!(f, "{} has no {BUILD_MODULE}. Point to a ReShade build folder.", path.display())
            }
            ReshadeError::Io(e) => write!(f, "File error: {e}"),
        }
    }
}

impl std::error::Error for ReshadeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReshadeError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ReshadeError {
    fn from(e: io::Error) -> Self {
        ReshadeError::Io(e)
    }
}

/// Settings the app understands. `None` means the key is left as it is on write.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReshadeConfig {
    /// Active preset (`GENERAL`, `PresetPath`). Relative paths are resolved against the config folder.
    pub preset_path: Option<PathBuf>,
    /// Screenshot folder (`SCREENSHOT`, `SavePath`).
    pub screenshot_path: Option<PathBuf>,
    /// Performance mode flag (`GENERAL`, `PerformanceMode`).
    pub performance_mode: Option<bool>,
}

/// Finds a ReShade proxy DLL in the AC root by its file name.
pub fn detect(ac_root: &Path) -> Option<ReshadeInstall> {
    let (proxy_name, proxy_dll) = find_proxy(ac_root)?;
    let config = ac_root.join(CONFIG_NAME);
    Some(ReshadeInstall {
        version: read_product_version(&proxy_dll),
        proxy_dll,
        proxy_name: proxy_name.to_string(),
        config: config.is_file().then_some(config),
        presets: list_presets(ac_root),
    })
}

/// Copy the build folder's module into the AC root as the default proxy.
///
/// An existing file with that name is moved to the archive first. Refused while
/// the game runs.
pub fn install(ac_root: &Path, build_dir: &Path, game_running: bool) -> Result<InstallReport, ReshadeError> {
    if game_running {
        return Err(ReshadeError::GameRunning);
    }
    ensure_ac_root(ac_root)?;
    let source = build_dir.join(BUILD_MODULE);
    if !source.is_file() {
        return Err(ReshadeError::NoProxyInBuild(build_dir.to_path_buf()));
    }

    let target = ac_root.join(DEFAULT_PROXY);
    let archived_to = if target.exists() {
        Some(archive_file(ac_root, &target)?)
    } else {
        None
    };

    if let Err(e) = fs::copy(&source, &target) {
        // Put the previous file back so a failed install changes nothing.
        let _ = fs::remove_file(&target);
        if let Some(folder) = &archived_to {
            let _ = fs::rename(folder.join(DEFAULT_PROXY), &target);
            let _ = fs::remove_dir(folder);
        }
        return Err(e.into());
    }

    Ok(InstallReport {
        proxy_name: DEFAULT_PROXY.to_string(),
        archived_to,
    })
}

/// Move the installed proxy DLL into the archive. Returns the archive folder, or
/// None if no proxy was installed. Refused while the game runs.
pub fn uninstall(ac_root: &Path, game_running: bool) -> Result<Option<PathBuf>, ReshadeError> {
    if game_running {
        return Err(ReshadeError::GameRunning);
    }
    ensure_ac_root(ac_root)?;
    match find_proxy(ac_root) {
        Some((_, proxy)) => Ok(Some(archive_file(ac_root, &proxy)?)),
        None => Ok(None),
    }
}

/// Read the settings this module knows. Missing file is an error.
pub fn read_config(path: &Path) -> io::Result<ReshadeConfig> {
    let ini = Ini::load(path)?;
    let base = config_base(path);
    Ok(ReshadeConfig {
        preset_path: non_empty(ini.get("GENERAL", "PresetPath")).map(|raw| resolve_ini_path(base, raw)),
        screenshot_path: non_empty(ini.get("SCREENSHOT", "SavePath")).map(|raw| resolve_ini_path(base, raw)),
        performance_mode: ini.get("GENERAL", "PerformanceMode").and_then(parse_bool),
    })
}

/// Write the settings in `cfg` into the file, keeping every other key and comment.
/// The previous file is moved to a backup first.
pub fn write_config(path: &Path, cfg: &ReshadeConfig) -> io::Result<()> {
    let mut ini = if path.exists() { Ini::load(path)? } else { Ini::default() };
    let base = config_base(path);
    if let Some(preset) = &cfg.preset_path {
        ini.set("GENERAL", "PresetPath", to_ini_path(base, preset));
    }
    if let Some(folder) = &cfg.screenshot_path {
        ini.set("SCREENSHOT", "SavePath", to_ini_path(base, folder));
    }
    if let Some(on) = cfg.performance_mode {
        ini.set("GENERAL", "PerformanceMode", if on { "1" } else { "0" });
    }
    ini.save_with_backup(path)
}

/// `.ini` files in the preset folder, sorted. Empty if the folder is missing.
pub fn list_presets(ac_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(ac_root.join(PRESET_DIR)) else {
        return Vec::new();
    };
    let mut presets: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
        })
        .collect();
    presets.sort();
    presets
}

/// Entries of the `Techniques` list in a preset file, in file order. Each entry
/// is as written, `Name@Effect.fx`. Only keys above the first section are read,
/// as the preset format places them there.
pub fn preset_techniques(path: &Path) -> io::Result<Vec<String>> {
    let text = fs::read_to_string(path)?;
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            break;
        }
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') || line.starts_with('/') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') && key.trim().eq_ignore_ascii_case("Techniques") {
            return Ok(split_list(value.trim()));
        }
    }
    Ok(Vec::new())
}

// ---------------------------------------------------------------------------
// helpers

fn ensure_ac_root(ac_root: &Path) -> Result<(), ReshadeError> {
    if is_ac_root(ac_root) {
        Ok(())
    } else {
        Err(ReshadeError::NotAcRoot(ac_root.to_path_buf()))
    }
}

/// First known proxy name present in the AC root, with its path.
fn find_proxy(ac_root: &Path) -> Option<(&'static str, PathBuf)> {
    PROXY_NAMES
        .iter()
        .map(|&name| (name, ac_root.join(name)))
        .find(|(_, path)| path.is_file())
}

/// Move `file` into a fresh archive folder under the AC root and return that folder.
fn archive_file(ac_root: &Path, file: &Path) -> io::Result<PathBuf> {
    let name = file
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file has no name"))?;
    let folder = new_archive_folder(ac_root)?;
    if let Err(e) = fs::rename(file, folder.join(name)) {
        let _ = fs::remove_dir(&folder);
        return Err(e);
    }
    Ok(folder)
}

/// Create `<AC root>/acxs-reshade-archive/<stamp>/`. Never reuses a folder: the
/// stamp moves forward a second until the name is free.
fn new_archive_folder(ac_root: &Path) -> io::Result<PathBuf> {
    let root = ac_root.join(ARCHIVE_DIR);
    fs::create_dir_all(&root)?;
    let mut when = chrono::Local::now();
    loop {
        let candidate = root.join(when.format("%Y%m%d-%H%M%S").to_string());
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                when += chrono::TimeDelta::seconds(1);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Read `ProductVersion` from the version resource of a PE file. The resource
/// stores its strings as UTF-16, so the key is searched for in that encoding.
fn read_product_version(dll: &Path) -> Option<String> {
    let bytes = fs::read(dll).ok()?;
    let needle: Vec<u8> = "ProductVersion"
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    let mut pos = find_bytes(&bytes, &needle)? + needle.len();
    // Skip the zero padding that aligns the value.
    while pos + 1 < bytes.len() && bytes[pos] == 0 && bytes[pos + 1] == 0 {
        pos += 2;
    }
    let mut units = Vec::new();
    while pos + 1 < bytes.len() && units.len() < 64 {
        let unit = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
        if unit == 0 {
            break;
        }
        units.push(unit);
        pos += 2;
    }
    let version = String::from_utf16(&units).ok()?.trim().to_string();
    (!version.is_empty()).then_some(version)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Directory that relative paths in the config are taken from.
fn config_base(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        v if v == "1" || v.eq_ignore_ascii_case("true") => Some(true),
        v if v == "0" || v.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// Turn a config path value into a full path. Both separators are accepted.
fn resolve_ini_path(base: &Path, raw: &str) -> PathBuf {
    let normalized = raw.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Inverse of [`resolve_ini_path`]: paths under `base` are written relative with
/// backslashes, the form the settings file uses.
fn to_ini_path(base: &Path, path: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => format!(
            ".\\{}",
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\\")
        ),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Split a comma list. `,,` is an escaped comma inside an item.
fn split_list(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ',' {
            if chars.peek() == Some(&',') {
                chars.next();
                current.push(',');
            } else {
                items.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    items.push(current);
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scratch folder that is removed on drop, also when a test panics.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Scratch {
            let path = std::env::temp_dir().join(format!("acxs-reshade-{}-{label}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn ac_root(&self) -> PathBuf {
            let root = self.0.join("ac");
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join(crate::settings::AC_EXECUTABLE), b"exe").unwrap();
            root
        }

        fn build_dir(&self, version: &str, tag: &str) -> PathBuf {
            let dir = self.0.join("build");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(BUILD_MODULE), module_bytes(version, tag)).unwrap();
            dir
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn utf16z(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(|u| u.to_le_bytes())
            .collect()
    }

    /// Bytes shaped like a module with a version resource, plus a tag to tell builds apart.
    fn module_bytes(version: &str, tag: &str) -> Vec<u8> {
        let mut bytes = b"MZ".to_vec();
        bytes.extend(utf16z("ProductVersion"));
        bytes.extend([0, 0]); // padding
        bytes.extend(utf16z(version));
        bytes.extend(tag.as_bytes());
        bytes
    }

    fn write_text(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn detect_finds_proxy_config_presets_and_version() {
        let scratch = Scratch::new("detect");
        let ac = scratch.ac_root();
        fs::write(ac.join("dxgi.dll"), module_bytes("5.9.2", "x")).unwrap();
        write_text(&ac.join(CONFIG_NAME), "[GENERAL]\r\n");
        write_text(&ac.join(PRESET_DIR).join("b.ini"), "");
        write_text(&ac.join(PRESET_DIR).join("a.ini"), "");

        let found = detect(&ac).expect("proxy should be detected");
        assert_eq!(found.proxy_name, "dxgi.dll");
        assert_eq!(found.proxy_dll, ac.join("dxgi.dll"));
        assert_eq!(found.config, Some(ac.join(CONFIG_NAME)));
        assert_eq!(found.presets, vec![ac.join(PRESET_DIR).join("a.ini"), ac.join(PRESET_DIR).join("b.ini")]);
        assert_eq!(found.version.as_deref(), Some("5.9.2"));
    }

    #[test]
    fn detect_returns_none_without_proxy() {
        let scratch = Scratch::new("detect-none");
        let ac = scratch.ac_root();
        fs::write(ac.join("d3d11.dll.txt"), b"not a proxy").unwrap();
        assert!(detect(&ac).is_none());
    }

    #[test]
    fn install_copies_module_as_proxy() {
        let scratch = Scratch::new("install");
        let ac = scratch.ac_root();
        let build = scratch.build_dir("5.9.2", "new");

        let report = install(&ac, &build, false).unwrap();
        assert_eq!(report.proxy_name, "dxgi.dll");
        assert_eq!(report.archived_to, None);
        assert_eq!(fs::read(ac.join("dxgi.dll")).unwrap(), module_bytes("5.9.2", "new"));
    }

    #[test]
    fn reinstall_archives_previous_proxy_each_time() {
        let scratch = Scratch::new("reinstall");
        let ac = scratch.ac_root();
        let build = scratch.build_dir("5.9.2", "new");
        fs::write(ac.join("dxgi.dll"), b"old proxy").unwrap();

        let first = install(&ac, &build, false).unwrap().archived_to.expect("old file archived");
        assert_eq!(fs::read(first.join("dxgi.dll")).unwrap(), b"old proxy");
        assert!(first.starts_with(ac.join(ARCHIVE_DIR)));

        let second = install(&ac, &build, false).unwrap().archived_to.expect("previous install archived");
        assert_ne!(first, second);
        assert_eq!(fs::read(second.join("dxgi.dll")).unwrap(), module_bytes("5.9.2", "new"));
        assert_eq!(fs::read(ac.join("dxgi.dll")).unwrap(), module_bytes("5.9.2", "new"));
    }

    #[test]
    fn install_refuses_while_game_runs() {
        let scratch = Scratch::new("install-running");
        let ac = scratch.ac_root();
        let build = scratch.build_dir("5.9.2", "new");
        assert!(matches!(install(&ac, &build, true), Err(ReshadeError::GameRunning)));
        assert!(!ac.join("dxgi.dll").exists());
    }

    #[test]
    fn install_rejects_non_ac_root_and_empty_build() {
        let scratch = Scratch::new("install-errors");
        let plain = scratch.0.join("plain");
        fs::create_dir_all(&plain).unwrap();
        let build = scratch.build_dir("5.9.2", "new");
        assert!(matches!(install(&plain, &build, false), Err(ReshadeError::NotAcRoot(_))));

        let ac = scratch.ac_root();
        let empty_build = scratch.0.join("empty-build");
        fs::create_dir_all(&empty_build).unwrap();
        assert!(matches!(install(&ac, &empty_build, false), Err(ReshadeError::NoProxyInBuild(_))));
    }

    #[test]
    fn uninstall_archives_proxy_and_never_deletes() {
        let scratch = Scratch::new("uninstall");
        let ac = scratch.ac_root();
        let build = scratch.build_dir("5.9.2", "new");
        install(&ac, &build, false).unwrap();

        let archive = uninstall(&ac, false).unwrap().expect("proxy archived");
        assert!(!ac.join("dxgi.dll").exists());
        assert_eq!(fs::read(archive.join("dxgi.dll")).unwrap(), module_bytes("5.9.2", "new"));

        assert_eq!(uninstall(&ac, false).unwrap(), None);
    }

    #[test]
    fn uninstall_refuses_while_game_runs() {
        let scratch = Scratch::new("uninstall-running");
        let ac = scratch.ac_root();
        fs::write(ac.join("dxgi.dll"), b"proxy").unwrap();
        assert!(matches!(uninstall(&ac, true), Err(ReshadeError::GameRunning)));
        assert!(ac.join("dxgi.dll").exists());
    }

    #[test]
    fn config_round_trip_keeps_unknown_keys() {
        let scratch = Scratch::new("config");
        let ac = scratch.ac_root();
        let path = ac.join(CONFIG_NAME);
        write_text(
            &path,
            "; kept comment\r\n[GENERAL]\r\nPresetPath=.\\reshade-presets\\main.ini\r\nUnknownKey=keepme\r\n[INPUT]\r\nKeyScreenshot=44,0,0,0,0\r\n",
        );

        let cfg = read_config(&path).unwrap();
        assert_eq!(cfg.preset_path, Some(ac.join("reshade-presets").join("main.ini")));
        assert_eq!(cfg.screenshot_path, None);
        assert_eq!(cfg.performance_mode, None);

        let updated = ReshadeConfig {
            preset_path: Some(ac.join("reshade-presets").join("night.ini")),
            screenshot_path: Some(ac.join("shots")),
            performance_mode: Some(true),
        };
        write_config(&path, &updated).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("; kept comment\r\n"));
        assert!(text.contains("UnknownKey=keepme\r\n"));
        assert!(text.contains("KeyScreenshot=44,0,0,0,0\r\n"));
        assert!(text.contains("PresetPath=.\\reshade-presets\\night.ini\r\n"));
        assert!(text.contains("PerformanceMode=1\r\n"));
        assert!(text.contains("[SCREENSHOT]\r\nSavePath=.\\shots\r\n"));
        // The previous file is kept as a backup.
        let backups = fs::read_dir(&ac)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".acxs-backup-"))
            .count();
        assert_eq!(backups, 1);

        assert_eq!(read_config(&path).unwrap(), updated);
    }

    #[test]
    fn list_presets_returns_sorted_ini_files_only() {
        let scratch = Scratch::new("presets");
        let ac = scratch.ac_root();
        let dir = ac.join(PRESET_DIR);
        write_text(&dir.join("zeta.ini"), "");
        write_text(&dir.join("alpha.ini"), "");
        write_text(&dir.join("notes.txt"), "");
        assert_eq!(list_presets(&ac), vec![dir.join("alpha.ini"), dir.join("zeta.ini")]);
        assert!(list_presets(&scratch.0.join("missing")).is_empty());
    }

    #[test]
    fn preset_techniques_reads_global_list_only() {
        let scratch = Scratch::new("techniques");
        let path = scratch.0.join("preset.ini");
        write_text(
            &path,
            "Techniques=Sharpen@Sharpen.fx,Lut@Lut.fx,Odd@Odd,,Name.fx\r\nTechniqueSorting=Lut@Lut.fx\r\n[OTHER]\r\nTechniques=Ignored@X.fx\r\n",
        );
        assert_eq!(
            preset_techniques(&path).unwrap(),
            vec!["Sharpen@Sharpen.fx", "Lut@Lut.fx", "Odd@Odd,Name.fx"]
        );

        let empty = scratch.0.join("empty.ini");
        write_text(&empty, "[OTHER]\r\nTechniques=Ignored@X.fx\r\n");
        assert!(preset_techniques(&empty).unwrap().is_empty());
    }
}
