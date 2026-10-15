//! Car classes and car years as CSP reads them from `extension/config`:
//!
//! - `data_car_classes.ini`, section `[CLASSES]`: one `car_id=class` per line.
//!   Both 0.2.11 and 0.3.0-preview675 only use `race` and `street` as values.
//!   The file's own comment says other classes are "tags".
//! - `data_car_years.ini`, section `[YEARS]`: one `car_id=year` per line.
//!
//! The readers never write. A missing file is not an error: they return an empty
//! `Vec`. Other I/O errors, and files that are not valid UTF-8, are returned as
//! `Err` (for `version_diff`, they are reported as lines in its output).
//! Car ids are matched case-insensitively, the same way the INI reader matches keys.
//!
//! The writers ([`write_classes`], [`write_years`], [`add_car_to_class`],
//! [`set_car_year`]) only upsert entries. They never remove a car, keep comments
//! and key order, and back up the previous file before saving. A file that has
//! no change is not saved at all.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::Path;

use crate::ini::Ini;

const CLASSES_FILE: &str = "data_car_classes.ini";
const YEARS_FILE: &str = "data_car_years.ini";
const CLASSES_SECTION: &str = "CLASSES";
const YEARS_SECTION: &str = "YEARS";

#[derive(Debug, Clone)]
pub struct CarClass {
    /// Class value as written in the file, for example `race` or `street`.
    pub id: String,
    /// Display name. The file has no name key, so this is always `None` for now.
    pub name: Option<String>,
    /// Car ids in file order.
    pub cars: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CarYear {
    pub car_id: String,
    /// `None` when the value is empty or not a whole number. The car is still listed.
    pub year: Option<i64>,
}

/// Reads the INI file, or `None` when it does not exist.
fn load_ini(config_dir: &Path, file: &str) -> io::Result<Option<Ini>> {
    match Ini::load(&config_dir.join(file)) {
        Ok(ini) => Ok(Some(ini)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Reads `[CLASSES]` from `data_car_classes.ini` and groups the cars by class.
///
/// Classes come out in order of first appearance, and cars in file order.
/// Entries with an empty class are skipped. If a car is listed twice, the first
/// entry wins. Returns an empty `Vec` if the file does not exist.
pub fn read_classes(config_dir: &Path) -> io::Result<Vec<CarClass>> {
    let Some(ini) = load_ini(config_dir, CLASSES_FILE)? else {
        return Ok(Vec::new());
    };
    let mut classes: Vec<CarClass> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (car, class) in ini.entries(CLASSES_SECTION) {
        if class.is_empty() || !seen.insert(car.to_lowercase()) {
            continue;
        }
        match classes.iter_mut().find(|c| c.id.eq_ignore_ascii_case(class)) {
            Some(c) => c.cars.push(car.to_string()),
            None => classes.push(CarClass {
                id: class.to_string(),
                name: None,
                cars: vec![car.to_string()],
            }),
        }
    }
    Ok(classes)
}

/// Reads `[YEARS]` from `data_car_years.ini`, in file order.
///
/// Values that are empty or not numbers give `year: None`. The car is kept
/// anyway. Returns an empty `Vec` if the file does not exist.
pub fn read_years(config_dir: &Path) -> io::Result<Vec<CarYear>> {
    let Some(ini) = load_ini(config_dir, YEARS_FILE)? else {
        return Ok(Vec::new());
    };
    Ok(ini
        .entries(YEARS_SECTION)
        .into_iter()
        .map(|(car, value)| CarYear {
            car_id: car.to_string(),
            year: value.parse().ok(),
        })
        .collect())
}

/// The class that lists `car_id`, if any.
pub fn class_of<'a>(classes: &'a [CarClass], car_id: &str) -> Option<&'a CarClass> {
    classes
        .iter()
        .find(|c| c.cars.iter().any(|car| car.eq_ignore_ascii_case(car_id)))
}

/// The year recorded for `car_id`, if it has one.
pub fn year_of(years: &[CarYear], car_id: &str) -> Option<i64> {
    years
        .iter()
        .find(|y| y.car_id.eq_ignore_ascii_case(car_id))?
        .year
}

/// Sets `car=class` in `[CLASSES]` of `data_car_classes.ini` for every car of every
/// class in `classes`. Existing entries are updated in place and new ones are added
/// at the end of the section. Cars not in `classes` are left alone.
pub fn write_classes(config_dir: &Path, classes: &[CarClass]) -> io::Result<()> {
    for class in classes {
        check_token("class", &class.id)?;
        for car in &class.cars {
            check_token("car id", car)?;
        }
    }
    edit_file(config_dir, CLASSES_FILE, |ini| {
        for class in classes {
            for car in &class.cars {
                ini.set(CLASSES_SECTION, car, &class.id);
            }
        }
        Ok(())
    })
}

/// Sets `car=year` in `[YEARS]` of `data_car_years.ini` for every entry of `years`
/// whose year is known. Entries with `year: None` are skipped, so their current
/// value is kept. Other cars are left alone, as in [`write_classes`].
pub fn write_years(config_dir: &Path, years: &[CarYear]) -> io::Result<()> {
    for entry in years {
        check_token("car id", &entry.car_id)?;
        if let Some(year) = entry.year {
            check_year(year)?;
        }
    }
    edit_file(config_dir, YEARS_FILE, |ini| {
        for entry in years {
            if let Some(year) = entry.year {
                ini.set(YEARS_SECTION, &entry.car_id, year);
            }
        }
        Ok(())
    })
}

/// Puts `car_id` into `class_id`, adding its entry to `[CLASSES]` when it has none.
///
/// A car already listed under a different class is not moved. The call fails with
/// `ErrorKind::AlreadyExists` and writes nothing. Adding a car to the class it is
/// already in does nothing.
pub fn add_car_to_class(config_dir: &Path, car_id: &str, class_id: &str) -> io::Result<()> {
    check_token("car id", car_id)?;
    check_token("class", class_id)?;
    edit_file(config_dir, CLASSES_FILE, |ini| {
        if let Some(current) = ini.get(CLASSES_SECTION, car_id) {
            if current.eq_ignore_ascii_case(class_id) {
                return Ok(());
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{car_id} is already in class {current}; it was not moved"),
            ));
        }
        ini.set(CLASSES_SECTION, car_id, class_id);
        Ok(())
    })
}

/// Sets the year of `car_id` in `[YEARS]`, adding the entry when it is missing.
/// The year must be a positive number.
pub fn set_car_year(config_dir: &Path, car_id: &str, year: i64) -> io::Result<()> {
    check_token("car id", car_id)?;
    check_year(year)?;
    edit_file(config_dir, YEARS_FILE, |ini| {
        ini.set(YEARS_SECTION, car_id, year);
        Ok(())
    })
}

/// Loads `file` in `config_dir` (a missing file starts empty), applies `edit`, and
/// saves with a backup only when the INI text changed.
fn edit_file(config_dir: &Path, file: &str, edit: impl FnOnce(&mut Ini) -> io::Result<()>) -> io::Result<()> {
    let path = config_dir.join(file);
    let before = match Ini::load(&path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    let mut ini = before.clone();
    edit(&mut ini)?;
    if ini == before {
        return Ok(());
    }
    ini.save_with_backup(&path)
}

/// Car ids and class ids become `key=value` lines. They must be non-empty, have no
/// surrounding spaces, and contain none of the characters the INI reader treats as syntax.
fn check_token(what: &str, text: &str) -> io::Result<()> {
    let bad = text.is_empty()
        || text != text.trim()
        || text.chars().any(|c| c.is_control() || matches!(c, '=' | ';' | '[' | ']'));
    if bad {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid {what}: {text:?}")));
    }
    Ok(())
}

fn check_year(year: i64) -> io::Result<()> {
    if year <= 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("invalid year: {year}")));
    }
    Ok(())
}

/// Lists the classes and cars that exist in only one of the two config directories.
///
/// Each line reads like `class race only in <dir>` or `car <id> only in <dir>`.
/// Classes come first, then cars, both sorted by id. Read problems other than
/// a missing file come first as `could not read ...` lines, and that side is
/// then treated as empty. Two identical sets give an empty `Vec`.
pub fn version_diff(v02_dir: &Path, v03_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let side_a = read_side(v02_dir, &mut out);
    let side_b = read_side(v03_dir, &mut out);

    let classes_a = class_index(&side_a);
    let classes_b = class_index(&side_b);
    let cars_a = car_index(&side_a);
    let cars_b = car_index(&side_b);

    let a = v02_dir.display().to_string();
    let b = v03_dir.display().to_string();
    push_only_in(&mut out, "class", &classes_a, &classes_b, &a);
    push_only_in(&mut out, "class", &classes_b, &classes_a, &b);
    push_only_in(&mut out, "car", &cars_a, &cars_b, &a);
    push_only_in(&mut out, "car", &cars_b, &cars_a, &b);
    out
}

/// Reads one side of `version_diff`. Unreadable files are reported in `out`.
fn read_side(dir: &Path, out: &mut Vec<String>) -> Vec<CarClass> {
    match read_classes(dir) {
        Ok(classes) => classes,
        Err(e) => {
            out.push(format!("could not read {CLASSES_FILE} in {}: {e}", dir.display()));
            Vec::new()
        }
    }
}

/// Lowercase id -> original id, for classes.
fn class_index(classes: &[CarClass]) -> BTreeMap<String, String> {
    classes
        .iter()
        .map(|c| (c.id.to_lowercase(), c.id.clone()))
        .collect()
}

/// Lowercase id -> original id, for all cars across all classes.
fn car_index(classes: &[CarClass]) -> BTreeMap<String, String> {
    classes
        .iter()
        .flat_map(|c| c.cars.iter())
        .map(|car| (car.to_lowercase(), car.clone()))
        .collect()
}

/// Pushes a line for each key of `mine` that is missing from `other`.
fn push_only_in(
    out: &mut Vec<String>,
    kind: &str,
    mine: &BTreeMap<String, String>,
    other: &BTreeMap<String, String>,
    dir: &str,
) {
    for (key, id) in mine {
        if !other.contains_key(key) {
            out.push(format!("{kind} {id} only in {dir}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A temp dir under `std::env::temp_dir()`, removed on drop (also after a panic).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "acxs_car_classes_{}_{}",
                std::process::id(),
                tag
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, file: &str, text: &str) {
            fs::write(self.0.join(file), text).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const CLASSES_SAMPLE: &str = "; in AC, there are two classes, street and race. for anything else,\r\n\
        ; there are tags. let’s keep it tidy\r\n\
        [CLASSES]\r\n\
        alpha=race\r\n\
        beta=street\r\n\
        gamma=race\r\n";

    #[test]
    fn read_classes_groups_cars_by_class_in_file_order() {
        let dir = TempDir::new("group");
        dir.write(CLASSES_FILE, CLASSES_SAMPLE);

        let classes = read_classes(dir.path()).unwrap();
        assert_eq!(classes.len(), 2);
        assert_eq!(classes[0].id, "race");
        assert_eq!(classes[0].name, None);
        assert_eq!(classes[0].cars, vec!["alpha", "gamma"]);
        assert_eq!(classes[1].id, "street");
        assert_eq!(classes[1].cars, vec!["beta"]);
    }

    #[test]
    fn read_classes_skips_empty_and_duplicate_entries_and_other_sections() {
        let dir = TempDir::new("skip");
        dir.write(
            CLASSES_FILE,
            "[OTHER]\r\nignored=race\r\n[CLASSES]\r\nempty=\r\nalpha=race\r\nalpha=street\r\nbeta=drift\r\n",
        );

        let classes = read_classes(dir.path()).unwrap();
        assert_eq!(classes.len(), 2);
        assert_eq!(classes[0].id, "race");
        assert_eq!(classes[0].cars, vec!["alpha"]); // duplicate kept under first class
        assert_eq!(classes[1].id, "drift");
        assert_eq!(classes[1].cars, vec!["beta"]);
    }

    #[test]
    fn read_years_parses_numbers_and_keeps_bad_values() {
        let dir = TempDir::new("years");
        dir.write(
            YEARS_FILE,
            "; comment\r\n[YEARS]\r\ngood=2004\r\nempty=\r\nword=abc\r\nfloat=1.5\r\nspaced key = 1999\r\n",
        );

        let years = read_years(dir.path()).unwrap();
        let got: Vec<(&str, Option<i64>)> = years.iter().map(|y| (y.car_id.as_str(), y.year)).collect();
        assert_eq!(
            got,
            vec![
                ("good", Some(2004)),
                ("empty", None),
                ("word", None),
                ("float", None),
                ("spaced key", Some(1999)),
            ]
        );
    }

    #[test]
    fn missing_files_and_missing_dir_give_empty_vectors() {
        let dir = TempDir::new("missing");
        assert!(read_classes(dir.path()).unwrap().is_empty());
        assert!(read_years(dir.path()).unwrap().is_empty());

        let nowhere = dir.path().join("does_not_exist");
        assert!(read_classes(&nowhere).unwrap().is_empty());
        assert!(read_years(&nowhere).unwrap().is_empty());
    }

    #[test]
    fn lookups_are_case_insensitive_and_return_none_when_absent() {
        let dir = TempDir::new("lookup");
        dir.write(CLASSES_FILE, CLASSES_SAMPLE);
        dir.write(YEARS_FILE, "[YEARS]\r\nalpha=2010\r\nbeta=\r\n");

        let classes = read_classes(dir.path()).unwrap();
        let years = read_years(dir.path()).unwrap();

        assert_eq!(class_of(&classes, "ALPHA").map(|c| c.id.as_str()), Some("race"));
        assert_eq!(class_of(&classes, "beta").map(|c| c.id.as_str()), Some("street"));
        assert!(class_of(&classes, "nope").is_none());

        assert_eq!(year_of(&years, "Alpha"), Some(2010));
        assert_eq!(year_of(&years, "beta"), None); // present, but no year
        assert_eq!(year_of(&years, "gamma"), None); // absent
        assert_eq!(year_of(&[], "alpha"), None);
    }

    #[test]
    fn version_diff_identical_dirs_give_nothing() {
        let a = TempDir::new("same_a");
        let b = TempDir::new("same_b");
        a.write(CLASSES_FILE, CLASSES_SAMPLE);
        b.write(CLASSES_FILE, CLASSES_SAMPLE);
        assert!(version_diff(a.path(), b.path()).is_empty());
    }

    #[test]
    fn version_diff_lists_classes_and_cars_present_in_only_one_version() {
        let old = TempDir::new("diff_old");
        let new = TempDir::new("diff_new");
        old.write(CLASSES_FILE, "[CLASSES]\r\nalpha=race\r\nbeta=race\r\nretired=street\r\n");
        new.write(CLASSES_FILE, "[CLASSES]\r\nalpha=race\r\nbeta=race\r\nnewcar=race\r\nnewclass_car=drift\r\n");

        let diff = version_diff(old.path(), new.path());
        let old_s = old.path().display().to_string();
        let new_s = new.path().display().to_string();
        assert_eq!(
            diff,
            vec![
                format!("class street only in {old_s}"),
                format!("class drift only in {new_s}"),
                format!("car retired only in {old_s}"),
                format!("car newcar only in {new_s}"),
                format!("car newclass_car only in {new_s}"),
            ]
        );
    }

    #[test]
    fn version_diff_with_missing_side_lists_everything_from_the_other() {
        let old = TempDir::new("missing_old");
        let new = TempDir::new("missing_new");
        new.write(CLASSES_FILE, CLASSES_SAMPLE);
        let nowhere = old.path().join("absent");

        let diff = version_diff(&nowhere, new.path());
        let new_s = new.path().display().to_string();
        assert_eq!(
            diff,
            vec![
                format!("class race only in {new_s}"),
                format!("class street only in {new_s}"),
                format!("car alpha only in {new_s}"),
                format!("car beta only in {new_s}"),
                format!("car gamma only in {new_s}"),
            ]
        );
    }

    #[test]
    fn version_diff_reports_unreadable_file_instead_of_failing() {
        let old = TempDir::new("bad_old");
        let new = TempDir::new("bad_new");
        fs::write(old.path().join(CLASSES_FILE), [0xff_u8, 0xfe, 0x00]).unwrap();
        new.write(CLASSES_FILE, "[CLASSES]\r\nalpha=race\r\n");

        let diff = version_diff(old.path(), new.path());
        assert!(diff[0].starts_with("could not read data_car_classes.ini in "), "{diff:?}");
        assert!(diff.contains(&format!("car alpha only in {}", new.path().display())));
    }

    fn class(id: &str, cars: &[&str]) -> CarClass {
        CarClass { id: id.to_string(), name: None, cars: cars.iter().map(|c| c.to_string()).collect() }
    }

    fn backups(dir: &TempDir) -> usize {
        fs::read_dir(dir.path()).unwrap().count()
    }

    #[test]
    fn write_classes_updates_in_place_keeps_comments_and_backs_up() {
        let dir = TempDir::new("write_classes");
        dir.write(CLASSES_FILE, CLASSES_SAMPLE);

        write_classes(dir.path(), &[class("race", &["beta", "newcar"])]).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(CLASSES_FILE)).unwrap(),
            "; in AC, there are two classes, street and race. for anything else,\r\n\
             ; there are tags. let’s keep it tidy\r\n\
             [CLASSES]\r\nalpha=race\r\nbeta=race\r\ngamma=race\r\nnewcar=race\r\n"
        );
        assert_eq!(backups(&dir), 2);
    }

    #[test]
    fn write_classes_with_unchanged_data_does_not_save() {
        let dir = TempDir::new("write_same");
        dir.write(CLASSES_FILE, CLASSES_SAMPLE);

        let classes = read_classes(dir.path()).unwrap();
        write_classes(dir.path(), &classes).unwrap();
        assert_eq!(backups(&dir), 1);
    }

    #[test]
    fn write_classes_rejects_bad_ids_before_touching_the_file() {
        let dir = TempDir::new("write_bad");
        let err = write_classes(dir.path(), &[class("race", &["bad=id"])]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(backups(&dir), 0);
    }

    #[test]
    fn add_car_to_class_adds_missing_car_and_refuses_to_move_one() {
        let dir = TempDir::new("add_car");
        dir.write(CLASSES_FILE, CLASSES_SAMPLE);

        add_car_to_class(dir.path(), "delta", "street").unwrap();
        let classes = read_classes(dir.path()).unwrap();
        assert_eq!(class_of(&classes, "delta").map(|c| c.id.as_str()), Some("street"));

        // Same class again: no error and no new backup.
        add_car_to_class(dir.path(), "ALPHA", "race").unwrap();
        assert_eq!(backups(&dir), 2);

        let err = add_car_to_class(dir.path(), "alpha", "street").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(class_of(&read_classes(dir.path()).unwrap(), "alpha").map(|c| c.id.as_str()), Some("race"));
    }

    #[test]
    fn add_car_to_class_creates_a_missing_file() {
        let dir = TempDir::new("add_new_file");
        add_car_to_class(dir.path(), "alpha", "race").unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(CLASSES_FILE)).unwrap(),
            "[CLASSES]\r\nalpha=race\r\n"
        );
    }

    #[test]
    fn set_car_year_updates_adds_and_validates() {
        let dir = TempDir::new("set_year");
        dir.write(YEARS_FILE, "[YEARS]\r\nalpha=2010\r\nbeta=\r\n");

        set_car_year(dir.path(), "alpha", 2004).unwrap();
        set_car_year(dir.path(), "gamma", 1999).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(YEARS_FILE)).unwrap(),
            "[YEARS]\r\nalpha=2004\r\nbeta=\r\ngamma=1999\r\n"
        );
        assert_eq!(year_of(&read_years(dir.path()).unwrap(), "beta"), None);

        assert_eq!(set_car_year(dir.path(), "alpha", 0).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(set_car_year(dir.path(), "a=b", 2000).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(backups(&dir), 3);
    }

    #[test]
    fn write_years_skips_entries_without_a_year() {
        let dir = TempDir::new("write_years");
        dir.write(YEARS_FILE, "[YEARS]\r\nalpha=2010\r\nbeta=abc\r\n");

        let years = vec![
            CarYear { car_id: "beta".to_string(), year: None },
            CarYear { car_id: "alpha".to_string(), year: Some(2001) },
        ];
        write_years(dir.path(), &years).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(YEARS_FILE)).unwrap(),
            "[YEARS]\r\nalpha=2001\r\nbeta=abc\r\n"
        );
    }
}
