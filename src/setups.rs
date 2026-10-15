//! Car setup file manager.
//!
//! Assetto Corsa keeps setups in `<Documents>/Assetto Corsa/setups/<car_id>/<name>.ini`.
//! The setups root is always passed in by the caller. Nothing here deletes a file:
//! anything that would be overwritten or removed is renamed aside instead.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::ini::Ini;

/// Folder under the setups root that holds archived setups, one subfolder per car.
const ARCHIVE_DIR: &str = "_archive";
/// Marker in the name of backups made when a setup is replaced.
const BACKUP_MARKER: &str = ".acxs-backup-";

#[derive(Debug, Clone)]
pub struct SetupFile {
    pub car_id: String,
    /// File name without the `.ini` extension.
    pub name: String,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetupDiff {
    pub section: String,
    pub key: String,
    /// `None` when the key is missing on the left side.
    pub left: Option<String>,
    /// `None` when the key is missing on the right side.
    pub right: Option<String>,
}

/// List `.ini` setups for one car, or for every car folder when `car_id` is `None`.
/// Results are sorted by car, then by setup name. A missing root or car folder gives an empty list.
pub fn list_setups(setups_root: &Path, car_id: Option<&str>) -> Vec<SetupFile> {
    let car_ids: Vec<String> = match car_id {
        Some(car) => {
            if validate_component(car).is_err() {
                return Vec::new();
            }
            vec![car.to_string()]
        }
        None => match fs::read_dir(setups_root) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.') && n != ARCHIVE_DIR)
                .collect(),
            Err(_) => Vec::new(),
        },
    };

    let mut out = Vec::new();
    for car in car_ids {
        let Ok(entries) = fs::read_dir(setups_root.join(&car)) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() || !has_ini_extension(&path) {
                continue;
            }
            let Some(name) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let modified = entry.metadata().and_then(|m| m.modified()).ok();
            out.push(SetupFile { car_id: car.clone(), name, path, modified });
        }
    }

    out.sort_by(|a, b| {
        a.car_id
            .to_lowercase()
            .cmp(&b.car_id.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Copy a `.ini` file into `<setups_root>/<car_id>/`. If a file with the same name already
/// exists, it is first renamed to `<name>.ini.acxs-backup-<stamp>`. Returns the new path.
pub fn import_setup(setups_root: &Path, car_id: &str, source: &Path) -> io::Result<PathBuf> {
    validate_component(car_id)?;
    if !has_ini_extension(source) {
        return Err(invalid("setup files must have a .ini extension"));
    }
    if !source.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("no such file: {}", source.display())));
    }
    let file_name = source.file_name().ok_or_else(|| invalid("source has no file name"))?;

    let car_dir = setups_root.join(car_id);
    fs::create_dir_all(&car_dir)?;
    let dest = car_dir.join(file_name);

    if dest.exists() {
        if same_file(source, &dest) {
            return Err(invalid("source is already in the car's setup folder"));
        }
        let base = format!("{}{}{}", file_name.to_string_lossy(), BACKUP_MARKER, stamp());
        let backup = free_path(&car_dir, &base, "");
        fs::rename(&dest, &backup)?;
    }

    fs::copy(source, &dest)?;
    Ok(dest)
}

/// Compare two setups. Sections and keys are matched case-insensitively. The result holds
/// only keys whose values differ, or that exist on one side only.
pub fn diff(left: &Ini, right: &Ini) -> Vec<SetupDiff> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::new();

    for section in left.section_names() {
        for (key, _) in left.entries(section) {
            if !seen.insert(normalized(section, key)) {
                continue;
            }
            let l = left.get(section, key).map(str::to_string);
            let r = right.get(section, key).map(str::to_string);
            if l != r {
                out.push(SetupDiff {
                    section: section.to_string(),
                    key: key.to_string(),
                    left: l,
                    right: r,
                });
            }
        }
    }

    for section in right.section_names() {
        // Prefer the left side's spelling of the section when both sides have it.
        let canonical = left
            .section_names()
            .into_iter()
            .find(|s| s.eq_ignore_ascii_case(section))
            .unwrap_or(section);
        for (key, _) in right.entries(section) {
            if !seen.insert(normalized(section, key)) {
                continue;
            }
            out.push(SetupDiff {
                section: canonical.to_string(),
                key: key.to_string(),
                left: None,
                right: right.get(section, key).map(str::to_string),
            });
        }
    }

    out
}

/// Move a setup into `<setups_root>/_archive/<car_id>/` with a timestamp in its name.
/// `name` may be given with or without the `.ini` extension. Returns the archived path.
pub fn archive_setup(setups_root: &Path, car_id: &str, name: &str) -> io::Result<PathBuf> {
    validate_component(car_id)?;
    validate_component(name)?;
    let file_name = if has_ini_extension(Path::new(name)) {
        name.to_string()
    } else {
        format!("{name}.ini")
    };

    let source = setups_root.join(car_id).join(&file_name);
    if !source.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no setup {file_name} for {car_id}"),
        ));
    }

    let archive_dir = setups_root.join(ARCHIVE_DIR).join(car_id);
    fs::create_dir_all(&archive_dir)?;
    let stem = Path::new(&file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.clone());
    let dest = free_path(&archive_dir, &format!("{stem}-{}", stamp()), ".ini");
    fs::rename(&source, &dest)?;
    Ok(dest)
}

/// Every `(section, key, value)` of a setup file, in file order. Comments are left out. A section
/// or key that repeats (ignoring case) is listed once, with the value that `Ini::get` returns.
pub fn values(path: &Path) -> io::Result<Vec<(String, String, String)>> {
    let ini = Ini::load(path)?;
    let mut out = Vec::new();
    let mut seen_sections: HashSet<String> = HashSet::new();
    for section in ini.section_names() {
        if !seen_sections.insert(section.to_lowercase()) {
            continue;
        }
        let mut seen_keys: HashSet<String> = HashSet::new();
        for (key, value) in ini.entries(section) {
            if seen_keys.insert(key.to_lowercase()) {
                out.push((section.to_string(), key.to_string(), value.to_string()));
            }
        }
    }
    Ok(out)
}

/// Set one value in a setup file and save the file, keeping a backup of the previous version.
/// Comments, blank lines and key order are kept. An unchanged value writes nothing.
/// A key or section missing from the file is refused unless `allow_new_key` is true.
pub fn set_value(path: &Path, section: &str, key: &str, value: &str, allow_new_key: bool) -> io::Result<()> {
    let section = section.trim();
    let key = key.trim();
    check_section_name(section)?;
    check_key_name(key)?;
    check_value(value)?;

    let mut ini = Ini::load(path)?;
    match ini.get(section, key) {
        Some(current) if current == value => return Ok(()),
        Some(_) => {}
        None if allow_new_key => {}
        None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{key} is not in [{section}] of {}, and adding keys is not allowed", path.display()),
            ));
        }
    }
    ini.set(section, key, value);
    ini.save_with_backup(path)
}

fn check_section_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.chars().any(|c| c == '[' || c == ']' || c.is_control()) {
        Err(invalid(format!("invalid section name {name:?}")))
    } else {
        Ok(())
    }
}

fn check_key_name(name: &str) -> io::Result<()> {
    let comment_start = name.starts_with(';') || name.starts_with('#');
    let bad_char = name.chars().any(|c| matches!(c, '=' | '[' | ']') || c.is_control());
    if name.is_empty() || comment_start || bad_char {
        Err(invalid(format!("invalid key name {name:?}")))
    } else {
        Ok(())
    }
}

/// Values are written as they are, so they cannot contain line breaks or start or end with spaces,
/// which the reader would drop.
fn check_value(value: &str) -> io::Result<()> {
    if value.chars().any(|c| matches!(c, '\r' | '\n' | '\0')) || value.trim() != value {
        Err(invalid(format!("invalid value {value:?}: no line breaks, and no spaces at either end")))
    } else {
        Ok(())
    }
}

fn normalized(section: &str, key: &str) -> (String, String) {
    (section.to_lowercase(), key.to_lowercase())
}

fn stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg.into())
}

fn has_ini_extension(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini"))
}

/// Reject names that could escape the setups folder (separators, `..`, drive prefixes).
fn validate_component(s: &str) -> io::Result<()> {
    if s.is_empty() || s == "." || s == ".." || s.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0')) {
        Err(invalid(format!("invalid name {s:?}")))
    } else {
        Ok(())
    }
}

/// A path in `dir` named `base` + `ext` that is not taken yet, adding `-1`, `-2`, ... if needed.
/// Checked with `symlink_metadata` so a dangling symlink also counts as taken.
fn free_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{base}{ext}"));
    let mut n = 1;
    while fs::symlink_metadata(&candidate).is_ok() {
        candidate = dir.join(format!("{base}-{n}{ext}"));
        n += 1;
    }
    candidate
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique scratch folder that is removed on drop, even if the test panics.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("acxs-setups-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
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

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn lists_setups_per_car_and_skips_archive_and_backups() {
        let t = TempDir::new("list");
        let root = t.path();
        write(&root.join("ks_ferrari/b.ini"), "[A]\n");
        write(&root.join("ks_ferrari/a.ini"), "[A]\n");
        write(&root.join("ks_ferrari/notes.txt"), "x");
        write(&root.join("ks_ferrari/a.ini.acxs-backup-20260101-000000"), "x");
        write(&root.join("ks_bmw/z.ini"), "[A]\n");
        write(&root.join("_archive/ks_bmw/old.ini"), "[A]\n");

        let all = list_setups(root, None);
        let names: Vec<(&str, &str)> = all.iter().map(|s| (s.car_id.as_str(), s.name.as_str())).collect();
        assert_eq!(names, vec![("ks_bmw", "z"), ("ks_ferrari", "a"), ("ks_ferrari", "b")]);
        assert_eq!(all[1].path, root.join("ks_ferrari/a.ini"));
        assert!(all.iter().all(|s| s.modified.is_some()));

        assert_eq!(list_setups(root, Some("ks_ferrari")).len(), 2);
        assert!(list_setups(root, Some("missing")).is_empty());
        assert!(list_setups(&root.join("no_such_root"), None).is_empty());
        assert!(list_setups(root, Some("../etc")).is_empty());
    }

    #[test]
    fn import_copies_and_backs_up_on_name_clash() {
        let t = TempDir::new("import");
        let root = t.path().join("setups");
        let src_dir = t.path().join("downloads");
        let src = src_dir.join("race.ini");

        write(&src, "[A]\nX=1\n");
        let dest = import_setup(&root, "ks_bmw", &src).unwrap();
        assert_eq!(dest, root.join("ks_bmw/race.ini"));
        assert_eq!(fs::read_to_string(&dest).unwrap(), "[A]\nX=1\n");

        // Name clash: the old file is kept as a backup, the new one takes its place.
        write(&src, "[A]\nX=2\n");
        import_setup(&root, "ks_bmw", &src).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "[A]\nX=2\n");

        // A second clash in the same second must not overwrite the first backup.
        write(&src, "[A]\nX=3\n");
        import_setup(&root, "ks_bmw", &src).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "[A]\nX=3\n");

        let mut backups: Vec<String> = fs::read_dir(root.join("ks_bmw"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("race.ini.acxs-backup-"))
            .map(|e| fs::read_to_string(e.path()).unwrap())
            .collect();
        backups.sort();
        assert_eq!(backups, vec!["[A]\nX=1\n".to_string(), "[A]\nX=2\n".to_string()]);

        // Non-ini and missing sources are refused.
        assert!(import_setup(&root, "ks_bmw", &src_dir.join("race.txt")).is_err());
        assert!(import_setup(&root, "ks_bmw", &src_dir.join("missing.ini")).is_err());
    }

    #[test]
    fn diff_reports_changed_added_and_removed_keys() {
        let left = Ini::parse("[BASIC]\nNAME=Ferrari\nMASS=1200\nGONE=1\n[TYRES]\nFRONT=26\n");
        let right = Ini::parse("[basic]\nname=Lotus\nMASS=1200\nADDED=5\n[tyres]\nfront=26\n[NEWSEC]\nA=1\n");

        let d = diff(&left, &right);
        assert_eq!(d.len(), 4, "got {d:?}");

        let find = |section: &str, key: &str| {
            d.iter()
                .find(|x| x.section == section && x.key == key)
                .unwrap_or_else(|| panic!("missing diff {section}/{key}"))
                .clone()
        };
        assert_eq!(
            find("BASIC", "NAME"),
            SetupDiff {
                section: "BASIC".into(),
                key: "NAME".into(),
                left: Some("Ferrari".into()),
                right: Some("Lotus".into()),
            }
        );
        assert_eq!(find("BASIC", "GONE").left.as_deref(), Some("1"));
        assert_eq!(find("BASIC", "GONE").right, None);
        assert_eq!(find("BASIC", "ADDED").left, None);
        assert_eq!(find("BASIC", "ADDED").right.as_deref(), Some("5"));
        assert_eq!(find("NEWSEC", "A").right.as_deref(), Some("1"));

        // Unchanged keys (MASS, FRONT) are not reported, and identical sides give nothing.
        assert!(d.iter().all(|x| x.key != "MASS" && x.key != "FRONT"));
        assert!(diff(&left, &left).is_empty());
    }

    #[test]
    fn archive_moves_setup_with_timestamp_and_never_overwrites() {
        let t = TempDir::new("archive");
        let root = t.path();
        write(&root.join("ks_bmw/old.ini"), "[A]\nX=1\n");

        let archived = archive_setup(root, "ks_bmw", "old").unwrap();
        assert!(!root.join("ks_bmw/old.ini").exists());
        assert!(archived.starts_with(root.join("_archive/ks_bmw")));
        assert!(archived.file_name().unwrap().to_string_lossy().starts_with("old-"));
        assert_eq!(fs::read_to_string(&archived).unwrap(), "[A]\nX=1\n");

        // Archiving a same-named setup again keeps the first archive intact.
        write(&root.join("ks_bmw/old.ini"), "[A]\nX=2\n");
        let second = archive_setup(root, "ks_bmw", "old.ini").unwrap();
        assert_ne!(second, archived);
        assert_eq!(fs::read_to_string(&archived).unwrap(), "[A]\nX=1\n");
        assert_eq!(fs::read_to_string(&second).unwrap(), "[A]\nX=2\n");

        assert!(list_setups(root, None).is_empty());
        assert!(archive_setup(root, "ks_bmw", "old").is_err());
        assert!(archive_setup(root, "../ks_bmw", "old").is_err());
    }

    const EDIT_SAMPLE: &str = "; tuned at Monza\r\n[BASIC]\r\nNAME=Ferrari\r\n\r\n[TYRES]\r\nFRONT=26\r\n; keep me\r\nREAR=27\r\n";

    fn count_files(dir: &Path) -> usize {
        fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn set_value_changes_one_value_and_keeps_the_rest_of_the_file() {
        let t = TempDir::new("set-value");
        let path = t.path().join("ks_bmw/race.ini");
        write(&path, EDIT_SAMPLE);

        set_value(&path, "basic", "name", "Lotus", false).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "; tuned at Monza\r\n[BASIC]\r\nNAME=Lotus\r\n\r\n[TYRES]\r\nFRONT=26\r\n; keep me\r\nREAR=27\r\n"
        );
        // The replaced file is kept next to the new one.
        let backups: Vec<String> = fs::read_dir(t.path().join("ks_bmw"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("race.ini.acxs-backup-"))
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = t.path().join("ks_bmw").join(&backups[0]);
        assert_eq!(fs::read_to_string(backup).unwrap(), EDIT_SAMPLE);
    }

    #[test]
    fn set_value_refuses_new_keys_and_sections_without_the_flag() {
        let t = TempDir::new("set-value-new");
        let path = t.path().join("ks_bmw/race.ini");
        write(&path, EDIT_SAMPLE);

        let err = set_value(&path, "TYRES", "HOT", "28", false).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(set_value(&path, "AERO", "WING", "3", false).is_err());

        assert_eq!(fs::read_to_string(&path).unwrap(), EDIT_SAMPLE);
        assert_eq!(count_files(&t.path().join("ks_bmw")), 1, "a refused edit must not write a backup");
    }

    #[test]
    fn set_value_adds_keys_and_sections_with_the_flag() {
        let t = TempDir::new("set-value-add");
        let path = t.path().join("ks_bmw/race.ini");
        write(&path, EDIT_SAMPLE);

        set_value(&path, "TYRES", "HOT", "28", true).unwrap();
        set_value(&path, "AERO", "WING", "3", true).unwrap();

        let ini = Ini::load(&path).unwrap();
        assert_eq!(ini.get("tyres", "HOT"), Some("28"));
        assert_eq!(ini.get("AERO", "WING"), Some("3"));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("REAR=27\r\nHOT=28\r\n"), "new key goes after the section's last key: {text}");
        assert!(text.ends_with("[AERO]\r\nWING=3\r\n"), "{text}");
    }

    #[test]
    fn set_value_with_the_same_value_writes_nothing() {
        let t = TempDir::new("set-value-same");
        let path = t.path().join("ks_bmw/race.ini");
        write(&path, EDIT_SAMPLE);

        set_value(&path, "BASIC", "NAME", "Ferrari", false).unwrap();
        assert_eq!(count_files(&t.path().join("ks_bmw")), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), EDIT_SAMPLE);
    }

    #[test]
    fn set_value_rejects_bad_names_and_values_before_touching_the_file() {
        let t = TempDir::new("set-value-bad");
        let path = t.path().join("ks_bmw/race.ini");
        write(&path, EDIT_SAMPLE);

        let kind = |r: io::Result<()>| r.unwrap_err().kind();
        assert_eq!(kind(set_value(&path, "BASIC", "NAME", "Lotus\r\nX=1", true)), io::ErrorKind::InvalidInput);
        assert_eq!(kind(set_value(&path, "BASIC", "NAME", " padded ", true)), io::ErrorKind::InvalidInput);
        assert_eq!(kind(set_value(&path, "BASIC", "A=B", "1", true)), io::ErrorKind::InvalidInput);
        assert_eq!(kind(set_value(&path, "BASIC", "; note", "1", true)), io::ErrorKind::InvalidInput);
        assert_eq!(kind(set_value(&path, "", "KEY", "1", true)), io::ErrorKind::InvalidInput);
        assert_eq!(kind(set_value(&path, "A]B", "KEY", "1", true)), io::ErrorKind::InvalidInput);
        assert_eq!(kind(set_value(&path, "BASIC", "  ", "1", true)), io::ErrorKind::InvalidInput);

        assert_eq!(fs::read_to_string(&path).unwrap(), EDIT_SAMPLE);
        assert_eq!(count_files(&t.path().join("ks_bmw")), 1);
        assert!(set_value(&t.path().join("ks_bmw/missing.ini"), "A", "B", "1", true).is_err());
    }

    #[test]
    fn values_lists_section_key_value_in_file_order() {
        let t = TempDir::new("values");
        let path = t.path().join("ks_bmw/race.ini");
        write(&path, EDIT_SAMPLE);

        let got = values(&path).unwrap();
        let expected = vec![
            ("BASIC".to_string(), "NAME".to_string(), "Ferrari".to_string()),
            ("TYRES".to_string(), "FRONT".to_string(), "26".to_string()),
            ("TYRES".to_string(), "REAR".to_string(), "27".to_string()),
        ];
        assert_eq!(got, expected);

        // Repeated sections and keys are listed once, using the first occurrence.
        write(&path, "[A]\nX=1\n[a]\nX=2\nY=3\n[B]\nx=4\nx=5\n");
        let got = values(&path).unwrap();
        let pairs: Vec<(&str, &str, &str)> = got.iter().map(|(s, k, v)| (s.as_str(), k.as_str(), v.as_str())).collect();
        assert_eq!(pairs, vec![("A", "X", "1"), ("B", "x", "4")]);

        assert!(values(&t.path().join("ks_bmw/missing.ini")).is_err());
    }

    #[test]
    fn values_reflects_a_saved_edit() {
        let t = TempDir::new("values-edit");
        let path = t.path().join("ks_bmw/race.ini");
        write(&path, EDIT_SAMPLE);

        set_value(&path, "TYRES", "REAR", "29", false).unwrap();
        let rear = values(&path).unwrap().into_iter().find(|(s, k, _)| s == "TYRES" && k == "REAR");
        assert_eq!(rear, Some(("TYRES".into(), "REAR".into(), "29".into())));
    }
}
