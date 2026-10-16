//! Compare two installed car folders: their `data/*.ini` files key by key, and
//! the headline specs from `cardata`.
//!
//! Used to see what a modified car changed against its stock copy. Read-only:
//! nothing here writes to either folder.
//!
//! Matching follows the game: file names, section names and key names are
//! compared case-insensitively. A `data/` folder that is missing on one side
//! counts as empty, so every key on the other side shows up as added or removed.

use crate::cardata;
use crate::ini::Ini;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// One key that differs between the two cars. `None` means the key is absent on that side.
#[derive(Debug, Clone, PartialEq)]
pub struct IniChange {
    pub file: String,
    pub section: String,
    pub key: String,
    pub left: Option<String>,
    pub right: Option<String>,
}

/// One headline spec that differs between the two cars. A value that is not known shows as `-`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecDiff {
    pub label: String,
    pub left: String,
    pub right: String,
}

const UNKNOWN: &str = "-";

/// Lower-cased file name to the file name as written on disk, for every `*.ini`
/// directly inside `<car_dir>/data/`. A missing `data/` folder gives an empty map.
fn ini_files(car_dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    for name in crate::cardata::data_file_names(car_dir)? {
        if name.to_ascii_lowercase().ends_with(".ini") {
            files.entry(name.to_lowercase()).or_insert(name);
        }
    }
    Ok(files)
}

/// Parse one file of a car, or an empty INI when the file is not there.
fn load_side(car_dir: &Path, file: Option<&String>) -> io::Result<Ini> {
    match file {
        Some(name) => crate::cardata::load_data_ini(car_dir, name),
        None => Ok(Ini::default()),
    }
}

/// `(lower-case section, lower-case key)` to `(section, key, value)` as written in the file.
/// When a key repeats, the first occurrence wins, as `Ini::get` does.
type KeyMap = BTreeMap<(String, String), (String, String, String)>;

fn keys_of(ini: &Ini) -> KeyMap {
    let mut map = KeyMap::new();
    for section in ini.section_names() {
        for (key, value) in ini.entries(section) {
            map.entry((section.to_lowercase(), key.to_lowercase()))
                .or_insert_with(|| (section.to_string(), key.to_string(), value.to_string()));
        }
    }
    map
}

/// Every key in `data/*.ini` that is added, removed or changed between the two cars,
/// sorted by file, section and key (case-insensitive).
///
/// Returns an error only when a file exists but cannot be read or parsed as text.
pub fn compare_data_files(left_car: &Path, right_car: &Path) -> io::Result<Vec<IniChange>> {
    let left_files = ini_files(left_car)?;
    let right_files = ini_files(right_car)?;
    let file_keys: BTreeSet<String> = left_files.keys().chain(right_files.keys()).cloned().collect();

    let mut changes = Vec::new();
    for lower in &file_keys {
        let file = left_files
            .get(lower)
            .or_else(|| right_files.get(lower))
            .cloned()
            .unwrap_or_default();
        let left_ini = load_side(left_car, left_files.get(lower))?;
        let right_ini = load_side(right_car, right_files.get(lower))?;
        let left = keys_of(&left_ini);
        let right = keys_of(&right_ini);

        // Section names are shown as the left side spells them, else the right side's.
        let mut section_spelling: BTreeMap<String, String> = BTreeMap::new();
        for name in left_ini.section_names().into_iter().chain(right_ini.section_names()) {
            section_spelling.entry(name.to_lowercase()).or_insert_with(|| name.to_string());
        }

        let ids: BTreeSet<(String, String)> = left.keys().chain(right.keys()).cloned().collect();
        for id in &ids {
            let (l, r) = (left.get(id), right.get(id));
            let left_value = l.map(|(_, _, v)| v.clone());
            let right_value = r.map(|(_, _, v)| v.clone());
            if left_value == right_value {
                continue;
            }
            let Some((_, key, _)) = l.or(r) else { continue };
            changes.push(IniChange {
                file: file.clone(),
                section: section_spelling.get(&id.0).cloned().unwrap_or_default(),
                key: key.clone(),
                left: left_value,
                right: right_value,
            });
        }
    }

    changes.sort_by_key(|c| (c.file.to_lowercase(), c.section.to_lowercase(), c.key.to_lowercase()));
    Ok(changes)
}

/// Value of `label` in a summary, or `-` when the summary does not have it.
fn value_for(rows: &[(String, String)], label: &str) -> String {
    rows.iter()
        .find(|(l, _)| l == label)
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| UNKNOWN.to_string())
}

/// Headline specs (mass, power, gears, tyres, ...) that differ between the two cars.
/// Rows follow the order of `cardata::summary_lines`. A spec that one car does not
/// have shows as `-` and counts as a difference.
pub fn spec_diff(left_car: &Path, right_car: &Path) -> Vec<SpecDiff> {
    let left = cardata::summary_lines(&cardata::read_specs(left_car));
    let right = cardata::summary_lines(&cardata::read_specs(right_car));

    let mut labels: Vec<&str> = left.iter().map(|(label, _)| label.as_str()).collect();
    for (label, _) in &right {
        if !labels.contains(&label.as_str()) {
            labels.push(label.as_str());
        }
    }

    labels
        .into_iter()
        .filter_map(|label| {
            let (l, r) = (value_for(&left, label), value_for(&right, label));
            (l != r).then(|| SpecDiff { label: label.to_string(), left: l, right: r })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Two car folders (`stock` and `mod`) under the temp dir, removed on drop
    /// even when a test panics.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let root = std::env::temp_dir().join(format!("acxs-car-compare-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Scratch(root)
        }

        fn stock(&self) -> PathBuf {
            self.0.join("stock")
        }

        fn modded(&self) -> PathBuf {
            self.0.join("mod")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(car: &Path, file: &str, text: &str) {
        let data = car.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join(file), text).unwrap();
    }

    fn change(file: &str, section: &str, key: &str, left: Option<&str>, right: Option<&str>) -> IniChange {
        IniChange {
            file: file.into(),
            section: section.into(),
            key: key.into(),
            left: left.map(Into::into),
            right: right.map(Into::into),
        }
    }

    #[test]
    fn reports_changed_added_and_removed_keys_in_sorted_order() {
        let scratch = Scratch::new("changes");
        let (stock, modded) = (scratch.stock(), scratch.modded());

        write(
            &stock,
            "car.ini",
            "[BASIC]\r\nTOTALMASS=1100\r\nNAME=Stock\r\n[FUEL]\r\nMAX_FUEL=55\r\n",
        );
        write(
            &stock,
            "engine.ini",
            "[HEADER]\r\nPOWER_CURVE=power.lut\r\n[ENGINE_DATA]\r\nLIMITER=7800\r\nCOAST_REF_RPM=3000\r\n[TURBO_0]\r\nMAX_BOOST=0.8\r\n",
        );
        let engine_before = fs::read_to_string(stock.join("data").join("engine.ini")).unwrap();

        // Right side: section and key names in other case, one key changed, one
        // added, one removed, one section added, one whole file added.
        write(
            &modded,
            "car.ini",
            "[basic]\r\ntotalmass=1180\r\nname=Stock\r\nSPORT=1\r\n[FUEL]\r\nMAX_FUEL=55\r\n[WING_0]\r\nANGLE=5\r\n",
        );
        write(
            &modded,
            "engine.ini",
            "[header]\r\nPOWER_CURVE=power.lut\r\n[ENGINE_DATA]\r\nlimiter=8200\r\nADDED_KEY=1\r\n[TURBO_0]\r\nMAX_BOOST=1.2\r\n",
        );
        write(&modded, "tyres.ini", "[FRONT]\r\nNAME=Soft\r\n");

        let changes = compare_data_files(&stock, &modded).unwrap();

        assert_eq!(
            changes,
            vec![
                change("car.ini", "BASIC", "SPORT", None, Some("1")),
                change("car.ini", "BASIC", "TOTALMASS", Some("1100"), Some("1180")),
                change("car.ini", "WING_0", "ANGLE", None, Some("5")),
                change("engine.ini", "ENGINE_DATA", "ADDED_KEY", None, Some("1")),
                change("engine.ini", "ENGINE_DATA", "COAST_REF_RPM", Some("3000"), None),
                change("engine.ini", "ENGINE_DATA", "LIMITER", Some("7800"), Some("8200")),
                change("engine.ini", "TURBO_0", "MAX_BOOST", Some("0.8"), Some("1.2")),
                change("tyres.ini", "FRONT", "NAME", None, Some("Soft")),
            ]
        );

        // Comparing never writes: the stock file is byte-for-byte what it was.
        assert_eq!(fs::read_to_string(stock.join("data").join("engine.ini")).unwrap(), engine_before);
    }

    #[test]
    fn identical_cars_have_no_changes_and_no_spec_rows() {
        let scratch = Scratch::new("identical");
        write(&scratch.stock(), "car.ini", "[BASIC]\nTOTALMASS=1100\n");
        assert!(compare_data_files(&scratch.stock(), &scratch.stock()).unwrap().is_empty());
        assert!(spec_diff(&scratch.stock(), &scratch.stock()).is_empty());
    }

    #[test]
    fn spec_diff_lists_only_rows_that_differ() {
        let scratch = Scratch::new("specs");
        let (stock, modded) = (scratch.stock(), scratch.modded());

        write(&stock, "car.ini", "[BASIC]\nTOTALMASS=1100\n[FUEL]\nMAX_FUEL=55\n");
        write(&stock, "drivetrain.ini", "[TRACTION]\nTYPE=RWD\n[GEARS]\nCOUNT=6\n");
        write(&stock, "tyres.ini", "[FRONT]\nNAME=Slick Medium\n[REAR]\nNAME=Slick Soft\n");

        write(&modded, "car.ini", "[BASIC]\nTOTALMASS=1180\n[FUEL]\nMAX_FUEL=55\n");
        write(&modded, "drivetrain.ini", "[TRACTION]\nTYPE=RWD\n[GEARS]\nCOUNT=7\n");
        write(&modded, "tyres.ini", "[FRONT]\nNAME=Slick Medium\n[REAR]\nNAME=Slick Hard\n");

        let rows = spec_diff(&stock, &modded);
        let row = |label: &str, left: &str, right: &str| SpecDiff {
            label: label.into(),
            left: left.into(),
            right: right.into(),
        };
        assert_eq!(
            rows,
            vec![
                row("Mass", "1100 kg", "1180 kg"),
                row("Gears", "6", "7"),
                row("Rear tyre", "Slick Soft", "Slick Hard"),
            ]
        );
    }

    #[test]
    fn missing_data_folder_counts_as_empty() {
        let scratch = Scratch::new("missing");
        let (stock, modded) = (scratch.stock(), scratch.modded());
        write(&stock, "car.ini", "[BASIC]\nTOTALMASS=1100\nNAME=Stock\n[FUEL]\nMAX_FUEL=55\n");
        fs::create_dir_all(&modded).unwrap(); // car folder exists, data/ does not

        // Every key on the side that has data is reported as removed.
        let changes = compare_data_files(&stock, &modded).unwrap();
        assert_eq!(
            changes,
            vec![
                change("car.ini", "BASIC", "NAME", Some("Stock"), None),
                change("car.ini", "BASIC", "TOTALMASS", Some("1100"), None),
                change("car.ini", "FUEL", "MAX_FUEL", Some("55"), None),
            ]
        );

        // The reverse direction reports them as added.
        let reversed = compare_data_files(&modded, &stock).unwrap();
        assert_eq!(reversed.len(), 3);
        assert!(reversed.iter().all(|c| c.left.is_none() && c.right.is_some()));

        // A car folder that does not exist at all is also empty, not an error.
        let nowhere = scratch.0.join("does-not-exist");
        assert_eq!(compare_data_files(&nowhere, &nowhere).unwrap(), vec![]);

        let rows = spec_diff(&stock, &modded);
        assert!(rows.contains(&SpecDiff { label: "Mass".into(), left: "1100 kg".into(), right: "-".into() }));
        assert!(rows.contains(&SpecDiff { label: "Fuel tank".into(), left: "55 L".into(), right: "-".into() }));
    }
}
