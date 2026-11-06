//! Reader and editor for Custom Shaders Patch config files.
//!
//! CSP keeps its settings in `<AC root>/extension/config/*.ini`. Values are
//! plain `KEY=value` lines. Most keys carry a trailing `; description` comment
//! on the same line (for example `ENABLED=1 ; Active; 1 or 0`), which is shown
//! as the setting's doc when no `_doc_config_<name>.ini` entry exists for it.
//!
//! Those files are the patch's defaults. CSP reads the user's own values from
//! `<Documents>/Assetto Corsa/cfg/extension/<same name>.ini`, which override the
//! defaults and survive patch updates (CSP's own settings UI writes there). The
//! `*_effective` readers overlay that user file, and [`write_user_setting`]
//! writes into it, so an edit takes effect and is not hidden by an override.
//!
//! Editing goes through `crate::ini::Ini`, so comments, key order and the
//! trailing inline comment of an edited line are kept, and every save first
//! moves the previous file to a `.acxs-backup-<stamp>` copy.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// Prefix of the documentation files, `_doc_config_<config name>.ini`.
const DOC_PREFIX: &str = "_doc_config_";

/// A CSP config file found in the extension config directory.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigFile {
    /// File name including the extension, for example `general.ini`.
    pub name: String,
    pub path: PathBuf,
}

/// One `KEY=value` entry of a config file.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    pub section: String,
    pub key: String,
    /// The value without its trailing inline comment.
    pub value: String,
    /// Description from a `_doc_config_*.ini` entry if one exists, otherwise
    /// the trailing `; comment` of the line in the config file.
    pub doc: Option<String>,
}

/// CSP's user config folder, `<Documents>/Assetto Corsa/cfg/extension`.
pub fn user_config_dir() -> Option<PathBuf> {
    crate::settings::ac_user_cfg_dir().map(|cfg| cfg.join("extension"))
}

/// The user override file for the default config file `default_path`
/// (same file name, inside [`user_config_dir`]).
pub fn user_config_path(default_path: &Path) -> Option<PathBuf> {
    Some(user_config_dir()?.join(default_path.file_name()?))
}

/// `defaults` with every value of `user` laid over it: keys present in both take
/// the user's value (keeping the default's inline comment), keys only in `user`
/// are added.
pub fn overlay(defaults: &Ini, user: &Ini) -> Ini {
    let mut out = defaults.clone();
    for section in unique_sections(user) {
        for (key, raw) in user.entries(section) {
            let value = split_inline(raw).0;
            let rendered = render_value(out.get(section, key), value);
            out.set(section, key, rendered);
        }
    }
    out
}

/// The default file at `default_path` overlaid with the user file `user_path`.
/// Fails with `NotFound` only when neither file exists.
pub fn load_effective(default_path: &Path, user_path: Option<&Path>) -> io::Result<Ini> {
    let defaults = match Ini::load(default_path) {
        Ok(ini) => Some(ini),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let user = match user_path.map(Ini::load) {
        Some(Ok(ini)) => Some(ini),
        Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => None,
    };
    match (defaults, user) {
        (Some(d), Some(u)) => Ok(overlay(&d, &u)),
        (Some(d), None) => Ok(d),
        (None, Some(u)) => Ok(u),
        (None, None) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} not found", default_path.display()),
        )),
    }
}

/// Settings of `default_path` as CSP sees them: defaults overlaid with the
/// user file. Docs come from the default file's folder.
pub fn read_effective_settings(default_path: &Path, user_path: Option<&Path>) -> io::Result<Vec<Setting>> {
    let mut settings = match read_settings(default_path) {
        Ok(list) => list,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let Some(user_path) = user_path else {
        return Ok(settings);
    };
    let user = match Ini::load(user_path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(settings),
        Err(e) => return Err(e),
    };
    for section in unique_sections(&user) {
        for (key, raw) in user.entries(section) {
            let value = split_inline(raw).0.to_string();
            match settings
                .iter_mut()
                .find(|s| s.section.eq_ignore_ascii_case(section) && s.key.eq_ignore_ascii_case(key))
            {
                Some(existing) => existing.value = value,
                None => settings.push(Setting {
                    section: section.to_string(),
                    key: key.to_string(),
                    value,
                    doc: None,
                }),
            }
        }
    }
    Ok(settings)
}

/// Sets one value in the user override file `user_path` for the default file
/// `default_path`, creating the override file when needed. The key must exist in
/// the defaults or the override unless `allow_new_key` is true. Setting the value
/// CSP already uses saves nothing. The previous override file is backed up.
pub fn write_user_setting(
    default_path: &Path,
    user_path: &Path,
    section: &str,
    key: &str,
    value: &str,
    allow_new_key: bool,
) -> io::Result<()> {
    let (section, key, value) = (section.trim(), key.trim(), value.trim());
    validate(section, key, value)?;
    let effective = match load_effective(default_path, Some(user_path)) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    match effective.get(section, key) {
        Some(raw) if split_inline(raw).0 == value => return Ok(()),
        Some(_) => {}
        None if !allow_new_key => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{key} is not set in [{section}] of {}; refusing to create a new key", default_path.display()),
            ));
        }
        None => {}
    }
    let mut user = match Ini::load(user_path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    user.set(section, key, value);
    user.save_with_backup(user_path)
}

/// The `.ini` files directly inside `extension_config_dir`, sorted by name.
///
/// `_doc_config_*.ini` files are documentation, not settings, so they are
/// skipped. A missing or unreadable directory gives an empty list.
pub fn list_config_files(extension_config_dir: &Path) -> Vec<ConfigFile> {
    let Ok(entries) = fs::read_dir(extension_config_dir) else {
        return Vec::new();
    };
    let mut files: Vec<ConfigFile> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_file())
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ini")))
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let is_doc = name.to_ascii_lowercase().starts_with(DOC_PREFIX);
            (!is_doc).then_some(ConfigFile { name, path })
        })
        .collect();
    files.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    files
}

/// All settings of a config file, in file order.
///
/// Documentation is looked up in the same directory as `path`, see
/// [`read_settings_with_docs`].
pub fn read_settings(path: &Path) -> io::Result<Vec<Setting>> {
    read_settings_with_docs(path, path.parent())
}

/// Like [`read_settings`], but looks for `_doc_config_<stem>.ini` in `docs_dir`.
///
/// `docs_dir` is `None` when no documentation directory is known; the inline
/// comments are still used then.
pub fn read_settings_with_docs(path: &Path, docs_dir: Option<&Path>) -> io::Result<Vec<Setting>> {
    let ini = Ini::load(path)?;
    let docs = load_docs(docs_dir, path);
    let mut settings = Vec::new();
    for section in unique_sections(&ini) {
        for (key, raw) in ini.entries(section) {
            let (value, comment) = split_inline(raw);
            let doc = docs
                .get(&doc_key(section, key))
                .cloned()
                .or_else(|| comment.map(str::to_owned));
            settings.push(Setting {
                section: section.to_string(),
                key: key.to_string(),
                value: value.to_string(),
                doc,
            });
        }
    }
    Ok(settings)
}

/// Set one value and save the file (with a backup of the previous version).
///
/// If the key does not exist yet, the write is refused with
/// `ErrorKind::NotFound` unless `allow_new_key` is true. Nothing is written in
/// that case. Setting a key to the value it already has does not save at all.
/// The trailing inline comment of an existing line is kept.
pub fn write_setting(
    path: &Path,
    section: &str,
    key: &str,
    value: &str,
    allow_new_key: bool,
) -> io::Result<()> {
    let (section, key, value) = (section.trim(), key.trim(), value.trim());
    validate(section, key, value)?;

    let mut ini = match Ini::load(path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    let existing = ini.get(section, key);
    match existing {
        Some(raw) if split_inline(raw).0 == value => return Ok(()),
        Some(_) => {}
        None if !allow_new_key => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{key} is not set in [{section}] of {}; refusing to create a new key",
                    path.display()
                ),
            ));
        }
        None => {}
    }
    let rendered = render_value(existing, value);
    ini.set(section, key, rendered);
    ini.save_with_backup(path)
}

/// Values that differ between two versions of one config file.
///
/// Returns `(section, key, value_in_02, value_in_03)` for every key whose value
/// differs, or that exists in only one of the two. A side that does not have
/// the file counts as having no keys. Inline comments are ignored, so a changed
/// description alone does not show up. Sorted by section, then key, ignoring
/// case.
pub fn version_diff(
    file_name: &str,
    v02_dir: &Path,
    v03_dir: &Path,
) -> Vec<(String, String, Option<String>, Option<String>)> {
    let old = load_or_empty(&v02_dir.join(file_name));
    let new = load_or_empty(&v03_dir.join(file_name));

    let mut names: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    for ini in [&old, &new] {
        for section in unique_sections(ini) {
            for (key, _) in ini.entries(section) {
                names
                    .entry((section.to_ascii_lowercase(), key.to_ascii_lowercase()))
                    .or_insert_with(|| (section.to_string(), key.to_string()));
            }
        }
    }

    names
        .into_values()
        .filter_map(|(section, key)| {
            let in_02 = old.get(&section, &key).map(|raw| split_inline(raw).0.to_string());
            let in_03 = new.get(&section, &key).map(|raw| split_inline(raw).0.to_string());
            (in_02 != in_03).then_some((section, key, in_02, in_03))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn load_or_empty(path: &Path) -> Ini {
    Ini::load(path).unwrap_or_default()
}

/// Section names of `ini`, first spelling kept, duplicates (ignoring case) dropped.
fn unique_sections(ini: &Ini) -> Vec<&str> {
    let mut seen = HashSet::new();
    ini.section_names()
        .into_iter()
        .filter(|name| seen.insert(name.to_ascii_lowercase()))
        .collect()
}

/// Splits a raw value into the value and the trailing `; comment`.
/// The comment is `None` when it is missing or empty.
fn split_inline(raw: &str) -> (&str, Option<&str>) {
    match raw.find(';') {
        Some(i) => {
            let comment = raw[i + 1..].trim();
            (raw[..i].trim(), (!comment.is_empty()).then_some(comment))
        }
        None => (raw.trim(), None),
    }
}

/// New raw value for a line: `new_value` followed by the old trailing comment,
/// with the original spacing before the `;`.
fn render_value(existing_raw: Option<&str>, new_value: &str) -> String {
    let Some(raw) = existing_raw else {
        return new_value.to_string();
    };
    match raw.find(';') {
        Some(i) => {
            let before = &raw[..i];
            let gap = &before[before.trim_end().len()..];
            format!("{new_value}{gap}{}", &raw[i..])
        }
        None => new_value.to_string(),
    }
}

fn validate(section: &str, key: &str, value: &str) -> io::Result<()> {
    let invalid = |what: &str| io::Error::new(io::ErrorKind::InvalidInput, what.to_string());
    if section.is_empty() || section.contains(['[', ']', '\r', '\n']) {
        return Err(invalid("invalid section name"));
    }
    if key.is_empty() || key.contains(['=', ';', '\r', '\n']) {
        return Err(invalid("invalid key name"));
    }
    if value.contains(['\r', '\n', ';']) {
        return Err(invalid("value cannot contain line breaks or ';'"));
    }
    Ok(())
}

/// Matching key for documentation. Numbered parts match the `_X` placeholders
/// used in the docs, so `CONDITION_3` finds `CONDITION_X`. Case is ignored.
fn doc_key(section: &str, key: &str) -> (String, String) {
    (normalize(section), normalize(key))
}

fn normalize(name: &str) -> String {
    name.trim()
        .split('_')
        .map(|part| {
            if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
                "X".to_string()
            } else {
                part.to_ascii_uppercase()
            }
        })
        .collect::<Vec<_>>()
        .join("_")
}

fn load_docs(docs_dir: Option<&Path>, config_file: &Path) -> HashMap<(String, String), String> {
    let (Some(dir), Some(stem)) = (docs_dir, config_file.file_stem()) else {
        return HashMap::new();
    };
    let doc_path = dir.join(format!("{DOC_PREFIX}{}.ini", stem.to_string_lossy()));
    fs::read_to_string(doc_path)
        .map(|text| parse_docs(&text))
        .unwrap_or_default()
}

/// Parses doc lines of the form `[SECTION] - KEY = example ; description`.
/// Entries without a description are skipped. The first entry for a key wins.
fn parse_docs(text: &str) -> HashMap<(String, String), String> {
    let mut docs = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((section, rest)) = rest.split_once(']') else { continue };
        let Some(rest) = rest.trim_start().strip_prefix('-') else { continue };
        let Some((key, rest)) = rest.split_once('=') else { continue };
        if key.trim().is_empty() {
            continue;
        }
        let (_, comment) = split_inline(rest);
        if let Some(comment) = comment {
            docs.entry(doc_key(section, key)).or_insert_with(|| comment.to_string());
        }
    }
    docs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_override_wins_and_receives_writes() {
        let dir = std::env::temp_dir().join(format!("acxs-csp-user-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("defaults")).unwrap();
        fs::create_dir_all(dir.join("user")).unwrap();
        let default_path = dir.join("defaults").join("general.ini");
        let user_path = dir.join("user").join("general.ini");
        fs::write(&default_path, "[BASIC]\nENABLED=1 ; Active\nOTHER=5\n").unwrap();
        fs::write(&user_path, "[BASIC]\nENABLED=0\n").unwrap();

        let settings = read_effective_settings(&default_path, Some(&user_path)).unwrap();
        let enabled = settings.iter().find(|s| s.key == "ENABLED").unwrap();
        assert_eq!(enabled.value, "0");
        assert_eq!(enabled.doc.as_deref(), Some("Active"));

        write_user_setting(&default_path, &user_path, "BASIC", "OTHER", "7", false).unwrap();
        let user = Ini::load(&user_path).unwrap();
        assert_eq!(user.get("BASIC", "OTHER"), Some("7"));
        assert_eq!(user.get("BASIC", "ENABLED"), Some("0"));
        // The defaults stay untouched.
        assert_eq!(fs::read_to_string(&default_path).unwrap(), "[BASIC]\nENABLED=1 ; Active\nOTHER=5\n");
        assert!(write_user_setting(&default_path, &user_path, "BASIC", "NOPE", "1", false).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    const GENERAL: &str = "[BASIC]\r\nENABLED=1 ; Active (set to 0 to disable); 1 or 0\r\nNAME=Lights\r\n\r\n[CONFIGURATIONS]\r\nADAPTER_NAME= ; Internet adapter\r\n";

    /// Temporary directory under the system temp dir, removed on drop.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> TestDir {
            let path = std::env::temp_dir().join(format!(
                "acxs-csp-config-{}-{}",
                std::process::id(),
                label
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TestDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn backups(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("general.ini.acxs-backup-")
            })
            .collect()
    }

    fn row(
        section: &str,
        key: &str,
        in_02: Option<&str>,
        in_03: Option<&str>,
    ) -> (String, String, Option<String>, Option<String>) {
        (
            section.to_string(),
            key.to_string(),
            in_02.map(str::to_string),
            in_03.map(str::to_string),
        )
    }

    #[test]
    fn lists_only_ini_files_sorted() {
        let dir = TestDir::new("list");
        for name in [
            "lighting_fx.ini",
            "General.ini",
            "readme.txt",
            "_doc_config_general.ini",
            "general.ini.acxs-backup-20260101-000000",
        ] {
            write(&dir.path().join(name), "");
        }
        fs::create_dir(dir.path().join("folder.ini")).unwrap();

        let files = list_config_files(dir.path());
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["General.ini", "lighting_fx.ini"]);
        assert_eq!(files[0].path, dir.path().join("General.ini"));

        assert!(list_config_files(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn reads_values_without_inline_comments() {
        let dir = TestDir::new("read");
        let path = dir.path().join("general.ini");
        write(&path, GENERAL);

        let settings = read_settings(&path).unwrap();
        let find = |key: &str| settings.iter().find(|s| s.key == key).unwrap();

        let enabled = find("ENABLED");
        assert_eq!(enabled.section, "BASIC");
        assert_eq!(enabled.value, "1");
        assert_eq!(enabled.doc.as_deref(), Some("Active (set to 0 to disable); 1 or 0"));

        let name = find("NAME");
        assert_eq!(name.value, "Lights");
        assert_eq!(name.doc, None);

        let adapter = find("ADAPTER_NAME");
        assert_eq!(adapter.section, "CONFIGURATIONS");
        assert_eq!(adapter.value, "");
        assert_eq!(adapter.doc.as_deref(), Some("Internet adapter"));

        assert_eq!(settings.len(), 3);
    }

    #[test]
    fn doc_file_entries_win_and_match_numbered_names() {
        let dir = TestDir::new("docs");
        let path = dir.path().join("lighting_fx.ini");
        write(&path, "[BRIGHTNESS_3]\r\nSTRENGTH_7=2 ; inline text\r\nMULT=1\r\n");
        write(
            &dir.path().join("_doc_config_lighting_fx.ini"),
            "; a comment line\r\n[BRIGHTNESS_X]  - STRENGTH_X = 2.0   ; from doc file\r\n",
        );

        let settings = read_settings(&path).unwrap();
        assert_eq!(settings[0].section, "BRIGHTNESS_3");
        assert_eq!(settings[0].key, "STRENGTH_7");
        assert_eq!(settings[0].doc.as_deref(), Some("from doc file"));
        assert_eq!(settings[1].key, "MULT");
        assert_eq!(settings[1].doc, None);
    }

    #[test]
    fn write_updates_value_keeps_comment_and_backs_up() {
        let dir = TestDir::new("write");
        let path = dir.path().join("general.ini");
        write(&path, GENERAL);

        write_setting(&path, "BASIC", "ENABLED", "0", false).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("ENABLED=0 ; Active (set to 0 to disable); 1 or 0\r\n"));
        let settings = read_settings(&path).unwrap();
        let enabled = settings.iter().find(|s| s.key == "ENABLED").unwrap();
        assert_eq!(enabled.value, "0");
        assert_eq!(enabled.doc.as_deref(), Some("Active (set to 0 to disable); 1 or 0"));

        let saved = backups(dir.path());
        assert_eq!(saved.len(), 1);
        assert_eq!(fs::read_to_string(&saved[0]).unwrap(), GENERAL);

        // Same value again: nothing is written, so no second backup.
        write_setting(&path, "BASIC", "ENABLED", "0", false).unwrap();
        assert_eq!(backups(dir.path()).len(), 1);
    }

    #[test]
    fn refuses_new_key_without_flag() {
        let dir = TestDir::new("refuse");
        let path = dir.path().join("general.ini");
        write(&path, GENERAL);

        let err = write_setting(&path, "BASIC", "NEW_KEY", "v", false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let err = write_setting(&path, "NO_SUCH_SECTION", "X", "1", false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);

        assert_eq!(fs::read_to_string(&path).unwrap(), GENERAL);
        assert!(backups(dir.path()).is_empty());

        write_setting(&path, "BASIC", "NEW_KEY", "v", true).unwrap();
        let settings = read_settings(&path).unwrap();
        assert!(settings.iter().any(|s| s.key == "NEW_KEY" && s.value == "v"));
    }

    #[test]
    fn version_diff_lists_changed_and_one_sided_keys() {
        let dir = TestDir::new("diff");
        let v02 = dir.path().join("v02");
        let v03 = dir.path().join("v03");
        write(
            &v02.join("general.ini"),
            "[BASIC]\nENABLED=1 ; old comment\nFPS=30\n[OLD_ONLY]\nX=1\n",
        );
        write(
            &v03.join("general.ini"),
            "[BASIC]\nENABLED=1 ; new comment\nFPS=60\nNEW_KEY=5\n[NEW_ONLY]\nY=1\n",
        );

        let diff = version_diff("general.ini", &v02, &v03);
        assert_eq!(
            diff,
            vec![
                row("BASIC", "FPS", Some("30"), Some("60")),
                row("BASIC", "NEW_KEY", None, Some("5")),
                row("NEW_ONLY", "Y", None, Some("1")),
                row("OLD_ONLY", "X", Some("1"), None),
            ]
        );

        assert!(version_diff("missing.ini", &v02, &v03).is_empty());
    }
}

/// A config file's own description, from its `[ℹ]` section. Files without one are CSP's internal
/// data tables, not settings for people.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigMeta {
    /// Text after the group, such as `Lighting FX`.
    pub title: String,
    /// Text before the colon of the full name, such as `Graphics`.
    pub group: String,
    pub summary: String,
    pub description: String,
    pub order: f64,
}

/// Reads a config file's description. `None` for internal files and the version page.
pub fn config_meta(path: &Path) -> Option<ConfigMeta> {
    let bytes = fs::read(path).ok()?;
    let ini = Ini::parse(&String::from_utf8_lossy(&bytes));
    let section = "\u{2139}";
    let full = ini.get(section, "FULLNAME")?.trim();
    if full == "Information" {
        return None;
    }
    let (group, title) = match full.split_once(':') {
        Some((group, title)) => (group.trim().to_string(), title.trim().to_string()),
        None => ("Other".to_string(), full.to_string()),
    };
    let text = |key: &str| ini.get(section, key).unwrap_or_default().trim().to_string();
    Some(ConfigMeta {
        title,
        group,
        summary: text("SHORT_DESCRIPTION"),
        description: text("DESCRIPTION"),
        order: ini.get_f64(section, "ORDER").unwrap_or(0.0),
    })
}

/// What control an option wants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptionKind {
    Toggle,
    Range { min: f64, max: f64, percent: bool, step: Option<f64> },
    /// Not stated: the value decides.
    Unknown,
}

/// An option as CSP describes it in its comment: `Label (detail); type; note`.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionDoc {
    pub label: String,
    pub hint: Option<String>,
    pub kind: OptionKind,
    /// Marked hidden, or undocumented: not for the normal view.
    pub hidden: bool,
}

/// `SOME_KEY` as `Some key`.
fn key_label(key: &str) -> String {
    let lower = key.replace(['_', '.'], " ").trim().to_lowercase();
    let mut chars = lower.chars();
    chars.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + chars.as_str())
}

/// The first number in a text, skipping any unit around it.
fn first_number(text: &str) -> Option<f64> {
    let start = text.find(|c: char| c.is_ascii_digit() || c == '-')?;
    let tail = &text[start..];
    let end = tail.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(tail.len());
    tail[..end].parse().ok()
}

/// `from 0.5 s to 4.0, round to 0.1` or `from 0 to 2, perc`.
fn parse_range(text: &str) -> Option<OptionKind> {
    let body = text.strip_prefix("from ")?;
    let (low, high) = body.split_once(" to ")?;
    let step = high.split_once("round to ").and_then(|(_, step)| first_number(step));
    Some(OptionKind::Range { min: first_number(low)?, max: first_number(high)?, percent: text.contains("perc"), step })
}

/// Reads an option's comment.
pub fn parse_option_doc(key: &str, doc: Option<&str>) -> OptionDoc {
    let parts: Vec<&str> = doc.unwrap_or_default().split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
    let hidden = parts.is_empty() || parts.iter().any(|p| p.eq_ignore_ascii_case("hidden"));
    let mut kind = OptionKind::Unknown;
    let mut notes = Vec::new();
    for part in parts.iter().skip(1) {
        if part.eq_ignore_ascii_case("hidden") || part.starts_with("only with") {
            continue;
        }
        if *part == "1 or 0" {
            kind = OptionKind::Toggle;
        } else if let Some(range) = parse_range(part) {
            kind = range;
        } else {
            notes.push(*part);
        }
    }
    let first = parts.first().copied().filter(|p| !p.eq_ignore_ascii_case("hidden")).unwrap_or_default();
    let (label, detail) = match first.split_once(" (") {
        Some((label, detail)) if detail.ends_with(')') => (label, Some(detail.trim_end_matches(')'))),
        _ => (first, None),
    };
    let label = if label.is_empty() { key_label(key) } else { label.to_string() };
    let hint = detail.into_iter().chain(notes).collect::<Vec<_>>().join(". ");
    OptionDoc { label, hint: Some(hint).filter(|h| !h.is_empty()), kind, hidden }
}

#[cfg(test)]
mod doc_tests {
    use super::*;

    #[test]
    fn csp_comments_become_labels_controls_and_hints() {
        let toggle = parse_option_doc("X", Some("Use compatible sky shader (on newer systems, disabling this might help); 1 or 0"));
        assert_eq!(toggle.label, "Use compatible sky shader");
        assert_eq!(toggle.hint.as_deref(), Some("on newer systems, disabling this might help"));
        assert_eq!(toggle.kind, OptionKind::Toggle);
        assert!(!toggle.hidden);

        let delay = parse_option_doc("X", Some("Delay between updates; from 0.5 s to 4.0, round to 0.1"));
        assert_eq!(delay.kind, OptionKind::Range { min: 0.5, max: 4.0, percent: false, step: Some(0.1) });

        let bright = parse_option_doc("X", Some("Brightness (to adjust for PP-filter); from 0 to 2, perc"));
        assert_eq!(bright.kind, OptionKind::Range { min: 0.0, max: 2.0, percent: true, step: None });

        let script = parse_option_doc("X", Some("Script; look for assist.lua in extension/lua/joypad-assist; only with ENABLED"));
        assert_eq!(script.kind, OptionKind::Unknown);
        assert_eq!(script.hint.as_deref(), Some("look for assist.lua in extension/lua/joypad-assist"));
    }

    #[test]
    fn hidden_and_undocumented_options_stay_out_of_the_normal_view() {
        assert!(parse_option_doc("SPECULAR_MULT", Some("; hidden")).hidden);
        assert_eq!(parse_option_doc("SPECULAR_MULT", Some("; hidden")).label, "Specular mult");
        assert!(parse_option_doc("ENABLED", Some("this option does not do anything; hidden")).hidden);
        assert!(parse_option_doc("SOME_KEY", None).hidden);
    }

    #[test]
    fn a_file_describes_itself_and_internal_files_do_not() {
        let dir = std::env::temp_dir().join(format!("acxs-meta-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.ini"), "[\u{2139}]\nFULLNAME=Graphics: Lighting FX\nSHORT_DESCRIPTION=Dynamic lights\nORDER=-1.6\n\n[BASIC]\nENABLED=1\n").unwrap();
        fs::write(dir.join("b.ini"), "[DEFAULTS]\nX=1\n").unwrap();
        let meta = config_meta(&dir.join("a.ini")).unwrap();
        assert_eq!((meta.group.as_str(), meta.title.as_str()), ("Graphics", "Lighting FX"));
        assert_eq!(meta.order, -1.6);
        assert!(config_meta(&dir.join("b.ini")).is_none());
        fs::remove_dir_all(&dir).ok();
    }
}
