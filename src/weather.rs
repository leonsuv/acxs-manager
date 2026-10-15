//! Read-only view of CSP weather configuration.
//!
//! Layout used by the lights-patch v0.2.11 and v0.3.0-preview675 folders:
//!
//! - `extension/weather/<name>/` holds weather implementations (`weather.lua`,
//!   `manifest.ini`, `settings.ini`, `shaders/`, `src/`).
//! - `extension/weather-controllers/<name>/` holds controllers (`controller.lua`,
//!   `manifest.ini`, optionally `settings.ini`).
//! - `extension/config/weather_fx.ini` selects them through `[BASIC] CONTROLLER=`
//!   and `IMPLEMENTATION=`.
//!
//! Reads never write to disk. The writes are [`set_weather_value`] (`weather_fx.ini`),
//! [`set_preset_value`] (a preset's `settings.ini`), and [`create_preset`] and
//! [`copy_preset`], which only add new preset folders. Edits to existing files go
//! through [`Ini::save_with_backup`]. Missing folders are treated as empty.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// One folder under `extension/weather`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherPreset {
    pub name: String,
    pub path: PathBuf,
    /// True when `extension/weather-controllers` has a folder with the same name
    /// (compared case-insensitively). Folders in the patch layouts are paired this way.
    pub has_controller: bool,
}

/// Subfolder names of `dir`, sorted case-insensitively. Files, hidden folders
/// and anything unreadable are skipped. A missing `dir` gives an empty list.
fn subfolders(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            (!name.starts_with('.')).then_some((name, path))
        })
        .collect();
    folders.sort_by_key(|(name, _)| name.to_lowercase());
    folders
}

fn weather_dir(ac_root: &Path) -> PathBuf {
    ac_root.join("extension").join("weather")
}

fn controllers_dir(ac_root: &Path) -> PathBuf {
    ac_root.join("extension").join("weather-controllers")
}

/// Folders under `extension/weather`, sorted by name.
pub fn list_presets(ac_root: &Path) -> Vec<WeatherPreset> {
    let controllers: HashSet<String> = list_controllers(ac_root)
        .into_iter()
        .map(|name| name.to_lowercase())
        .collect();
    subfolders(&weather_dir(ac_root))
        .into_iter()
        .map(|(name, path)| WeatherPreset {
            has_controller: controllers.contains(&name.to_lowercase()),
            name,
            path,
        })
        .collect()
}

/// Folder names under `extension/weather-controllers`, sorted by name.
pub fn list_controllers(ac_root: &Path) -> Vec<String> {
    subfolders(&controllers_dir(ac_root))
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// Every `(section, key, value)` of `extension/config/weather_fx.ini`, in file order.
///
/// Values have a trailing inline comment (` ; ...` or ` ;; ...`) removed, since
/// the CSP files put their explanations there. A missing or unreadable file is
/// returned as the io error. Nothing is written.
pub fn read_weather_fx(ac_root: &Path) -> io::Result<Vec<(String, String, String)>> {
    read_entries(&ac_root.join("extension").join("config").join("weather_fx.ini"))
}

/// Every `(section, key, value)` of the ini file at `path`, in file order, with inline comments cut.
fn read_entries(path: &Path) -> io::Result<Vec<(String, String, String)>> {
    let ini = Ini::load(path)?;

    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for section in ini.section_names() {
        // Ini keeps duplicate headers as separate sections, but lookups hit the first one.
        if !seen.insert(section.to_lowercase()) {
            continue;
        }
        for (key, value) in ini.entries(section) {
            out.push((section.to_string(), key.to_string(), strip_inline_comment(value)));
        }
    }
    Ok(out)
}

/// Byte index where a trailing ` ;...` (or tab-prefixed) comment starts, if there is one.
fn inline_comment_start(raw: &str) -> Option<usize> {
    [" ;", "\t;"]
        .iter()
        .filter_map(|marker| raw.find(marker))
        .min()
}

/// Cut a trailing ` ;...` comment from a value. A `;` with no space before it stays.
fn strip_inline_comment(value: &str) -> String {
    let cut = inline_comment_start(value).unwrap_or(value.len());
    value[..cut].trim().to_string()
}

/// Set one value in `extension/config/weather_fx.ini` and save the file.
///
/// Comments, key order and the trailing inline comment of the edited line are
/// kept. The previous file is moved to a `.acxs-backup-<stamp>` copy before the
/// new one is written, and nothing is deleted. Section and key are matched
/// case-insensitively, as the game does. An unchanged value does not save.
///
/// A key that is not in the file yet is refused with `ErrorKind::NotFound`
/// unless `allow_new_key` is true. A missing `weather_fx.ini` is an error.
/// Section, key and value must be single-line. The value also may not contain
/// a ` ;` comment marker, because the reader would cut it off again.
pub fn set_weather_value(
    ac_root: &Path,
    section: &str,
    key: &str,
    value: &str,
    allow_new_key: bool,
) -> io::Result<()> {
    let path = ac_root.join("extension").join("config").join("weather_fx.ini");
    edit_ini_value(&path, section, key, value, allow_new_key)
}

/// Checks one value edit and applies it to the ini file at `path`, saving through
/// [`Ini::save_with_backup`]. Used by the `weather_fx.ini` and preset writers.
fn edit_ini_value(path: &Path, section: &str, key: &str, value: &str, allow_new_key: bool) -> io::Result<()> {
    let (section, key, value) = (section.trim(), key.trim(), value.trim());
    let invalid = |msg: String| io::Error::new(io::ErrorKind::InvalidInput, msg);
    if section.is_empty() || section.chars().any(|c| c.is_control() || c == '[' || c == ']') {
        return Err(invalid(format!("invalid section name {section:?}")));
    }
    if key.is_empty() || key.starts_with([';', '#']) || key.chars().any(|c| c.is_control() || c == '=') {
        return Err(invalid(format!("invalid key name {key:?}")));
    }
    if value.chars().any(char::is_control) || value.contains(" ;") || value.contains("\t;") {
        return Err(invalid(format!("value for {key} must be one line without a ';' comment")));
    }

    let before = Ini::load(path)?;
    let mut ini = before.clone();
    match ini.get(section, key).map(str::to_owned) {
        Some(raw) => ini.set(section, key, keep_inline_comment(&raw, value)),
        None if allow_new_key => ini.set(section, key, value),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{key} is not set in [{section}] of {}; refusing to create a new key", path.display()),
            ));
        }
    }
    if ini == before {
        return Ok(());
    }
    ini.save_with_backup(path)
}

/// `new_value` followed by the trailing comment of `raw`, when `raw` has one.
fn keep_inline_comment(raw: &str, new_value: &str) -> String {
    match inline_comment_start(raw) {
        Some(cut) => format!("{new_value}{}", &raw[cut..]),
        None => new_value.to_string(),
    }
}

/// File in each preset folder that holds the preset's parameters.
const PARAMETER_FILE: &str = "settings.ini";

/// New preset names become folder names, so they use only ASCII letters, digits, `-` and `_`.
fn validate_new_preset_name(name: &str) -> io::Result<()> {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid preset name {name:?}: use letters, digits, '-' and '_' only"),
    ))
}

/// Existing preset names are used as paths, so they only need to be one plain path component.
fn check_folder_name(name: &str) -> io::Result<()> {
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'));
    if plain {
        return Ok(());
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid preset name {name:?}")))
}

fn preset_folder(ac_root: &Path, name: &str) -> PathBuf {
    weather_dir(ac_root).join(name)
}

/// True when the weather folder already has a folder with this name, ignoring case.
fn preset_name_taken(ac_root: &Path, name: &str) -> bool {
    subfolders(&weather_dir(ac_root))
        .iter()
        .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
}

/// Writes `bytes` to a file that must not exist yet.
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)
}

/// Copies the folder `from` to the not yet existing folder `to`, file by file, subfolders included.
/// Symbolic links are skipped.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            write_new(&target, &fs::read(entry.path())?)?;
        }
    }
    Ok(())
}

/// Creates `extension/weather/<name>` with a copy of the parameter file (`settings.ini`) of the
/// first preset, in name order, that has one. Returns the new folder.
///
/// The name must use only ASCII letters, digits, `-` and `_` (`InvalidInput` otherwise). A name
/// already taken, ignoring case, gives `AlreadyExists`. With no preset to copy from, `NotFound`.
/// Nothing is overwritten or deleted.
pub fn create_preset(ac_root: &Path, name: &str) -> io::Result<PathBuf> {
    validate_new_preset_name(name)?;
    if preset_name_taken(ac_root, name) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("preset {name} already exists")));
    }
    let weather = weather_dir(ac_root);
    let source = subfolders(&weather)
        .into_iter()
        .map(|(_, path)| path.join(PARAMETER_FILE))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no preset under {} has a {PARAMETER_FILE} to copy", weather.display()),
            )
        })?;
    let bytes = fs::read(&source)?;
    let folder = preset_folder(ac_root, name);
    fs::create_dir(&folder)?;
    write_new(&folder.join(PARAMETER_FILE), &bytes)?;
    Ok(folder)
}

/// Copies the preset folder `from` to a new preset `to`, every file and subfolder included.
///
/// Each file is created new, so nothing in the target is overwritten, and the source is only read.
/// `from` must exist (`NotFound`), `to` must be a valid new name (`InvalidInput`) that is not taken,
/// ignoring case (`AlreadyExists`). If a copy fails part way, the files already written stay.
pub fn copy_preset(ac_root: &Path, from: &str, to: &str) -> io::Result<()> {
    check_folder_name(from)?;
    validate_new_preset_name(to)?;
    let source = preset_folder(ac_root, from);
    if !source.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("no preset named {from}")));
    }
    if preset_name_taken(ac_root, to) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("preset {to} already exists")));
    }
    copy_tree(&source, &preset_folder(ac_root, to))
}

/// Every `(section, key, value)` of a preset's `settings.ini`, in file order.
/// Values have their inline comment removed, as in [`read_weather_fx`]. Nothing is written.
pub fn read_preset_values(ac_root: &Path, preset: &str) -> io::Result<Vec<(String, String, String)>> {
    check_folder_name(preset)?;
    read_entries(&preset_folder(ac_root, preset).join(PARAMETER_FILE))
}

/// Sets one value in a preset's `settings.ini` and saves it through [`Ini::save_with_backup`].
///
/// The rules are those of [`set_weather_value`]: the key must exist unless `allow_new_key` is
/// true, and an unchanged value does not save. A preset with no `settings.ini` gives `NotFound`.
pub fn set_preset_value(
    ac_root: &Path,
    preset: &str,
    section: &str,
    key: &str,
    value: &str,
    allow_new_key: bool,
) -> io::Result<()> {
    check_folder_name(preset)?;
    let path = preset_folder(ac_root, preset).join(PARAMETER_FILE);
    if !path.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} has no {PARAMETER_FILE}", path.display())));
    }
    edit_ini_value(&path, section, key, value, allow_new_key)
}

/// Lines describing presets and controllers present in only one of the two roots.
///
/// Each line reads `preset <name>: only in v02` or `controller <name>: only in v03`.
/// Names are compared case-insensitively, as Windows folders are. The v02 entries
/// come before the v03 entries within each kind.
pub fn compare_versions(v02_root: &Path, v03_root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let presets_02: Vec<String> = list_presets(v02_root).into_iter().map(|p| p.name).collect();
    let presets_03: Vec<String> = list_presets(v03_root).into_iter().map(|p| p.name).collect();
    push_one_sided(&mut out, "preset", &presets_02, &presets_03);

    let controllers_02 = list_controllers(v02_root);
    let controllers_03 = list_controllers(v03_root);
    push_one_sided(&mut out, "controller", &controllers_02, &controllers_03);
    out
}

fn push_one_sided(out: &mut Vec<String>, kind: &str, v02: &[String], v03: &[String]) {
    let lower_02: HashSet<String> = v02.iter().map(|n| n.to_lowercase()).collect();
    let lower_03: HashSet<String> = v03.iter().map(|n| n.to_lowercase()).collect();
    for name in v02.iter().filter(|n| !lower_03.contains(&n.to_lowercase())) {
        out.push(format!("{kind} {name}: only in v02"));
    }
    for name in v03.iter().filter(|n| !lower_02.contains(&n.to_lowercase())) {
        out.push(format!("{kind} {name}: only in v03"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp root per test. `std::process::id()` keeps parallel runs apart.
    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("acxs-weather-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn mkdirs(root: &Path, rel: &[&str]) {
        for r in rel {
            fs::create_dir_all(root.join(r)).unwrap();
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn lists_presets_and_controllers_sorted_with_controller_flag() {
        let root = temp_root("listing");
        mkdirs(&root, &[
            "extension/weather/base",
            "extension/weather/Zeta",
            "extension/weather/alpha",
            "extension/weather-controllers/base",
            "extension/weather-controllers/base-schedule",
        ]);
        // Files and hidden folders are not presets.
        write(&root.join("extension/weather/readme.txt"), "x");
        mkdirs(&root, &["extension/weather/.hidden"]);

        let presets = list_presets(&root);
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["alpha", "base", "Zeta"]);
        assert!(presets[1].has_controller); // base has controller base
        assert!(!presets[0].has_controller);
        assert_eq!(presets[1].path, root.join("extension/weather/base"));

        assert_eq!(list_controllers(&root), ["base", "base-schedule"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_folders_give_empty_lists() {
        let root = temp_root("missing");
        assert!(list_presets(&root).is_empty());
        assert!(list_controllers(&root).is_empty());
        assert!(read_weather_fx(&root).is_err());
        assert!(compare_versions(&root, &root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_weather_fx_entries_in_order_and_strips_inline_comments() {
        let root = temp_root("fx");
        write(
            &root.join("extension/config/weather_fx.ini"),
            "\u{feff}[ℹ]\r\nFULLNAME=Weather FX\r\n\r\n[BASIC]\r\n; Extension\r\n\
             ENABLED=1 ; Active; 1 or 0\r\nCONTROLLER=sol ; Controller script\r\n\
             IMPLEMENTATION=base ;; hidden\r\nMODE=a;b\r\n\r\n[PERFORMANCE]\r\nHIGH_QUALITY_STARS=1\r\n",
        );
        let fx = read_weather_fx(&root).unwrap();
        let got: Vec<(&str, &str, &str)> = fx
            .iter()
            .map(|(s, k, v)| (s.as_str(), k.as_str(), v.as_str()))
            .collect();
        assert_eq!(got, [
            ("ℹ", "FULLNAME", "Weather FX"),
            ("BASIC", "ENABLED", "1"),
            ("BASIC", "CONTROLLER", "sol"),
            ("BASIC", "IMPLEMENTATION", "base"),
            ("BASIC", "MODE", "a;b"),
            ("PERFORMANCE", "HIGH_QUALITY_STARS", "1"),
        ]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_sections_are_listed_once() {
        let root = temp_root("fxdup");
        write(
            &root.join("extension/config/weather_fx.ini"),
            "[BASIC]\nA=1\n[basic]\nB=2\n",
        );
        let fx = read_weather_fx(&root).unwrap();
        assert_eq!(fx.len(), 1);
        assert_eq!(fx[0], ("BASIC".to_string(), "A".to_string(), "1".to_string()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compares_presets_and_controllers_present_in_one_version_only() {
        let v02 = temp_root("cmp-02");
        let v03 = temp_root("cmp-03");
        mkdirs(&v02, &[
            "extension/weather/base",
            "extension/weather/old_only",
            "extension/weather-controllers/base",
            "extension/weather-controllers/base-live",
        ]);
        mkdirs(&v03, &[
            "extension/weather/BASE",
            "extension/weather/new_only",
            "extension/weather-controllers/base",
            "extension/weather-controllers/base-live",
            "extension/weather-controllers/base-schedule",
        ]);

        assert_eq!(compare_versions(&v02, &v03), [
            "preset old_only: only in v02",
            "preset new_only: only in v03",
            "controller base-schedule: only in v03",
        ]);
        fs::remove_dir_all(v02).unwrap();
        fs::remove_dir_all(v03).unwrap();
    }

    #[test]
    fn set_value_keeps_inline_comment_order_and_backs_up_the_old_file() {
        let root = temp_root("set");
        let fx = root.join("extension/config/weather_fx.ini");
        write(&fx, "[BASIC]\r\nENABLED=1 ; Active; 1 or 0\r\nCONTROLLER=sol\r\n");

        set_weather_value(&root, "basic", "enabled", "0", false).unwrap();
        assert_eq!(
            fs::read_to_string(&fx).unwrap(),
            "[BASIC]\r\nENABLED=0 ; Active; 1 or 0\r\nCONTROLLER=sol\r\n"
        );
        assert_eq!(fs::read_dir(fx.parent().unwrap()).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn set_value_refuses_new_keys_unless_allowed() {
        let root = temp_root("set-new");
        let fx = root.join("extension/config/weather_fx.ini");
        write(&fx, "[BASIC]\r\nENABLED=1\r\n");

        let err = set_weather_value(&root, "BASIC", "MODE", "a", false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read_to_string(&fx).unwrap(), "[BASIC]\r\nENABLED=1\r\n");

        set_weather_value(&root, "BASIC", "MODE", "a", true).unwrap();
        assert_eq!(
            read_weather_fx(&root).unwrap().last(),
            Some(&("BASIC".to_string(), "MODE".to_string(), "a".to_string()))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn set_value_with_the_same_value_does_not_save() {
        let root = temp_root("set-same");
        let fx = root.join("extension/config/weather_fx.ini");
        write(&fx, "[BASIC]\r\nENABLED=1 ; x\r\n");

        set_weather_value(&root, "BASIC", "ENABLED", "1", false).unwrap();
        assert_eq!(fs::read_dir(fx.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn set_value_rejects_bad_input_and_missing_file() {
        let root = temp_root("set-bad");
        assert!(set_weather_value(&root, "BASIC", "ENABLED", "1", true).is_err());

        let fx = root.join("extension/config/weather_fx.ini");
        write(&fx, "[BASIC]\r\nENABLED=1\r\n");
        let bad = [
            ("BASIC", "ENABLED", "1\r\nX=2"),
            ("BASIC", "ENABLED", "1 ; note"),
            ("BASIC", "", "1"),
            ("BA]SIC", "ENABLED", "1"),
            ("BASIC", "A=B", "1"),
        ];
        for (section, key, value) in bad {
            let err = set_weather_value(&root, section, key, value, true).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{section} {key} {value:?}");
        }
        assert_eq!(fs::read_to_string(&fx).unwrap(), "[BASIC]\r\nENABLED=1\r\n");
        assert_eq!(fs::read_dir(fx.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    /// `base` has a settings.ini, a manifest and a shader; `empty` has no settings.ini.
    fn preset_root(tag: &str) -> PathBuf {
        let root = temp_root(tag);
        write(
            &root.join("extension/weather/base/settings.ini"),
            "[PARAMETERS]\r\nSUN_ANGLE=12 ; degrees\r\nHUMIDITY=0.5\r\n",
        );
        write(&root.join("extension/weather/base/manifest.ini"), "[ABOUT]\r\nNAME=Base\r\n");
        write(&root.join("extension/weather/base/shaders/sky.fx"), "float x;");
        mkdirs(&root, &["extension/weather/empty"]);
        root
    }

    fn backups_in(dir: &Path, file: &str) -> usize {
        let prefix = format!("{file}.acxs-backup-");
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .count()
    }

    #[test]
    fn create_preset_copies_the_parameter_file_and_refuses_taken_or_bad_names() {
        let root = preset_root("create");
        let folder = create_preset(&root, "Storm_2-b").unwrap();
        assert_eq!(folder, root.join("extension/weather/Storm_2-b"));
        assert_eq!(
            fs::read_to_string(folder.join("settings.ini")).unwrap(),
            "[PARAMETERS]\r\nSUN_ANGLE=12 ; degrees\r\nHUMIDITY=0.5\r\n"
        );
        assert!(!folder.join("manifest.ini").exists());

        assert_eq!(create_preset(&root, "BASE").unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        for bad in ["", "two words", "../up", "dot.name", "sl/ash"] {
            let err = create_preset(&root, bad).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad:?}");
        }
        assert_eq!(list_presets(&root).len(), 3); // base, empty, Storm_2-b
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn create_preset_needs_a_parameter_file_to_copy() {
        let root = temp_root("create-none");
        mkdirs(&root, &["extension/weather/bare"]);
        let err = create_preset(&root, "fresh").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(!root.join("extension/weather/fresh").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copy_preset_copies_every_file_and_never_overwrites() {
        let root = preset_root("copy");
        copy_preset(&root, "base", "wet").unwrap();
        let wet = root.join("extension/weather/wet");
        assert_eq!(fs::read_to_string(wet.join("manifest.ini")).unwrap(), "[ABOUT]\r\nNAME=Base\r\n");
        assert_eq!(fs::read_to_string(wet.join("shaders/sky.fx")).unwrap(), "float x;");
        assert_eq!(
            fs::read_to_string(wet.join("settings.ini")).unwrap(),
            fs::read_to_string(root.join("extension/weather/base/settings.ini")).unwrap()
        );

        assert_eq!(copy_preset(&root, "base", "Wet").unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(copy_preset(&root, "missing", "other").unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(copy_preset(&root, "base", "bad name").unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(root.join("extension/weather/base/shaders/sky.fx").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn set_preset_value_edits_settings_ini_and_keeps_a_backup() {
        let root = preset_root("preset-set");
        let dir = root.join("extension/weather/base");
        assert_eq!(
            read_preset_values(&root, "base").unwrap()[0],
            ("PARAMETERS".to_string(), "SUN_ANGLE".to_string(), "12".to_string())
        );

        set_preset_value(&root, "base", "parameters", "sun_angle", "30", false).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("settings.ini")).unwrap(),
            "[PARAMETERS]\r\nSUN_ANGLE=30 ; degrees\r\nHUMIDITY=0.5\r\n"
        );
        assert_eq!(backups_in(&dir, "settings.ini"), 1);

        let err = set_preset_value(&root, "base", "PARAMETERS", "NEW", "1", false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        set_preset_value(&root, "base", "PARAMETERS", "NEW", "1", true).unwrap();
        assert!(read_preset_values(&root, "base")
            .unwrap()
            .iter()
            .any(|(s, k, v)| s == "PARAMETERS" && k == "NEW" && v == "1"));

        assert_eq!(
            set_preset_value(&root, "empty", "A", "B", "1", true).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(set_preset_value(&root, "../base", "PARAMETERS", "HUMIDITY", "1", false).is_err());
        assert!(read_preset_values(&root, "").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
