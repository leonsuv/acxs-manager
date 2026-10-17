//! Persistent user settings: the Assetto Corsa folder and the patch library folder.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const AC_EXECUTABLE: &str = "acs.exe";

const DEFAULT_AC_ROOTS: &[&str] = &[
    r"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa",
    r"C:\Program Files\Steam\steamapps\common\assettocorsa",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ac_root: String,
    pub patch_library: String,
    /// Send stored crash reports to the developer's GlitchTip server on start.
    /// Off by default. Panics are stored locally either way.
    pub crash_reports: bool,
}

impl Settings {
    /// Load settings from `path`. A missing or corrupt file yields defaults.
    pub fn load(path: &Path) -> Settings {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Save atomically: write a temp file, then rename over the target.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)
    }

    pub fn ac_root_path(&self) -> Option<PathBuf> {
        (!self.ac_root.trim().is_empty()).then(|| PathBuf::from(self.ac_root.trim()))
    }

    pub fn library_path(&self) -> PathBuf {
        if self.patch_library.trim().is_empty() {
            default_library()
        } else {
            PathBuf::from(self.patch_library.trim())
        }
    }
}

/// Where settings live: `%APPDATA%\ACXS\settings.json` on Windows, `~/.config/acxs` elsewhere.
pub fn settings_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(|appdata| PathBuf::from(appdata).join("ACXS"))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("acxs")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("settings.json")
}

/// Creates the config directory that holds the settings file, if it is missing.
/// Existing directories and files are left alone; nothing is deleted.
///
/// The directory comes from [`settings_path`]. `_settings` is accepted so
/// callers can pass the value they are about to save, but it does not change
/// where the directory is.
pub fn ensure_directories(_settings: &Settings) -> io::Result<()> {
    ensure_parent_dir(&settings_path())
}

/// Creates the directory that will contain `file`, if it does not exist yet.
fn ensure_parent_dir(file: &Path) -> io::Result<()> {
    match file.parent() {
        Some(dir) => fs::create_dir_all(dir),
        None => Ok(()),
    }
}

/// Default patch library: the folder the program runs from.
pub fn default_library() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The user's Documents folder. On Windows this asks the shell for the known
/// folder, so a Documents folder moved to OneDrive or another drive is found.
/// Falls back to `<home>/OneDrive/Documents` or `<home>/Documents`, whichever
/// holds an `Assetto Corsa` folder, then to `<home>/Documents`.
pub fn documents_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(dir) = known_documents_dir() {
        return Some(dir);
    }
    let home = PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?);
    let candidates = [home.join("Documents"), home.join("OneDrive").join("Documents")];
    candidates
        .iter()
        .find(|dir| dir.join("Assetto Corsa").is_dir())
        .cloned()
        .or_else(|| Some(candidates[0].clone()))
}

#[cfg(windows)]
fn known_documents_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: the GUID lives for the call, `raw` receives a CoTaskMemAlloc'd,
    // NUL-terminated string that is freed below on every path.
    let hr = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT as u32, std::ptr::null_mut(), &mut raw) };
    let path = if hr >= 0 && !raw.is_null() {
        // SAFETY: `raw` is NUL-terminated, so the scan stops inside the allocation.
        let len = unsafe { (0..).take_while(|&i| *raw.add(i) != 0).count() };
        // SAFETY: `len` units were just read from `raw`.
        let units = unsafe { std::slice::from_raw_parts(raw, len) };
        Some(PathBuf::from(std::ffi::OsString::from_wide(units)))
    } else {
        None
    };
    // SAFETY: freeing null is allowed; otherwise `raw` came from the shell.
    unsafe { CoTaskMemFree(raw as *const _) };
    path.filter(|p| !p.as_os_str().is_empty())
}

/// `<Documents>/Assetto Corsa`, where the game keeps the user's config
/// (`cfg/`: `race.ini`, `video.ini`, `controls.ini`, CSP's `cfg/extension/`),
/// setups, logs, replays and screenshots.
pub fn ac_documents_dir() -> Option<PathBuf> {
    documents_dir().map(|docs| docs.join("Assetto Corsa"))
}

/// `<Documents>/Assetto Corsa/cfg`, the user config folder the game reads.
pub fn ac_user_cfg_dir() -> Option<PathBuf> {
    ac_documents_dir().map(|dir| dir.join("cfg"))
}

pub fn is_ac_root(path: &Path) -> bool {
    path.join(AC_EXECUTABLE).is_file()
}

/// The first Assetto Corsa install found: the usual Steam folders first, then
/// every Steam library listed in `libraryfolders.vdf` (installs on other drives).
pub fn detect_ac_root() -> Option<PathBuf> {
    DEFAULT_AC_ROOTS
        .iter()
        .map(PathBuf::from)
        .find(|path| is_ac_root(path))
        .or_else(|| crate::steam::find_ac_installs(&crate::steam::default_steam_roots()).into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("acxs-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let settings = Settings {
            ac_root: r"D:\Games\assettocorsa".into(),
            patch_library: r"D:\Patches".into(),
            crash_reports: true,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ensure_creates_missing_dir_and_keeps_existing_files() {
        let base = std::env::temp_dir().join(format!("acxs-ensure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let file = base.join("config").join("settings.json");

        ensure_parent_dir(&file).unwrap();
        assert!(base.join("config").is_dir());

        fs::write(base.join("config").join("keep.txt"), "still here").unwrap();
        ensure_parent_dir(&file).unwrap();
        assert_eq!(
            fs::read_to_string(base.join("config").join("keep.txt")).unwrap(),
            "still here"
        );

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("acxs-corrupt-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
