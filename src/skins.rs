//! Car skin scanning.
//!
//! Layout: `<AC root>/content/cars/<car_id>/skins/<skin_id>/`, where each skin
//! folder may hold `ui_skin.json`, `preview.jpg` / `preview.png`, `livery.png`
//! and texture files. The manifest fields read are `skinname`, `drivername`,
//! `team` and `number`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::util::matches;

/// One car skin folder.
#[derive(Debug, Clone)]
pub struct SkinEntry {
    pub car_id: String,
    /// The skin folder name. `ui_skin.json` has no id field, so the folder name is the id.
    pub skin_id: String,
    /// `skinname` from `ui_skin.json`, or a name derived from the folder name.
    pub name: String,
    /// `drivername` from `ui_skin.json`.
    pub driver: String,
    /// `team` from `ui_skin.json`.
    pub team: String,
    /// `number` from `ui_skin.json` (accepted as string or number).
    pub number: String,
    pub preview: Option<PathBuf>,
    pub texture_count: usize,
    pub path: PathBuf,
}

/// Image files that are not skin textures.
const NON_TEXTURE_FILES: [&str; 3] = ["preview.jpg", "preview.png", "livery.png"];

/// Extensions counted as skin textures.
const TEXTURE_EXTENSIONS: [&str; 5] = ["dds", "png", "jpg", "jpeg", "gif"];

/// Scan all skins of one car, sorted by name (case-insensitive).
/// Missing directories yield an empty list.
pub fn scan_skins(ac_root: &Path, car_id: &str) -> Vec<SkinEntry> {
    let skins_dir = ac_root.join("content").join("cars").join(car_id).join("skins");
    let Ok(read_dir) = fs::read_dir(&skins_dir) else {
        return Vec::new();
    };

    let mut skins: Vec<SkinEntry> = read_dir
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let dir = entry.path();
            let skin_id = entry.file_name().to_string_lossy().into_owned();
            let manifest = read_manifest(&dir.join("ui_skin.json"));

            let name = manifest_str(manifest.as_ref(), "skinname")
                .unwrap_or_else(|| name_from_id(&skin_id));

            SkinEntry {
                car_id: car_id.to_string(),
                name,
                driver: manifest_str(manifest.as_ref(), "drivername").unwrap_or_default(),
                team: manifest_str(manifest.as_ref(), "team").unwrap_or_default(),
                number: manifest_str(manifest.as_ref(), "number").unwrap_or_default(),
                preview: preview_for(&dir),
                texture_count: count_textures(&dir),
                skin_id,
                path: dir,
            }
        })
        .collect();

    skins.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.skin_id.cmp(&b.skin_id))
    });
    skins
}

/// Map of car id to number of skin folders, for every car directory under
/// `content/cars`. Cars without a `skins` folder map to 0. Does not parse JSON.
pub fn scan_all_skin_counts(ac_root: &Path) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let Ok(read_dir) = fs::read_dir(ac_root.join("content").join("cars")) else {
        return counts;
    };
    for entry in read_dir.flatten() {
        let car_dir = entry.path();
        if !car_dir.is_dir() {
            continue;
        }
        let car_id = entry.file_name().to_string_lossy().into_owned();
        let count = fs::read_dir(car_dir.join("skins"))
            .map(|skins| skins.flatten().filter(|s| s.path().is_dir()).count())
            .unwrap_or(0);
        counts.insert(car_id, count);
    }
    counts
}

/// Skins whose name, skin id, driver, team or number match every term of `query`.
pub fn filter_skins<'a>(skins: &'a [SkinEntry], query: &str) -> Vec<&'a SkinEntry> {
    skins
        .iter()
        .filter(|s| {
            matches(
                query,
                &[&s.name, &s.skin_id, &s.driver, &s.team, &s.number],
            )
        })
        .collect()
}

/// Total texture files across `skins`, summing each skin's `texture_count`.
pub fn total_texture_count(skins: &[SkinEntry]) -> usize {
    skins.iter().map(|skin| skin.texture_count).sum()
}

/// One-line description of a skin for lists. The name comes first, then the
/// driver, team and number when they are set, then the texture count, all
/// joined with " · ". For example: `Racing Red · J. Doe · Team A · #7 · 2 textures`.
pub fn skin_summary(entry: &SkinEntry) -> String {
    let mut parts = vec![entry.name.clone()];
    if !entry.driver.is_empty() {
        parts.push(entry.driver.clone());
    }
    if !entry.team.is_empty() {
        parts.push(entry.team.clone());
    }
    if !entry.number.is_empty() {
        parts.push(format!("#{}", entry.number));
    }
    let textures = entry.texture_count;
    let noun = if textures == 1 { "texture" } else { "textures" };
    parts.push(format!("{textures} {noun}"));
    parts.join(" · ")
}

/// Preview image of a skin folder: `preview.jpg` first, then `preview.png`.
pub fn preview_for(skin_dir: &Path) -> Option<PathBuf> {
    ["preview.jpg", "preview.png"]
        .iter()
        .map(|name| skin_dir.join(name))
        .find(|path| path.is_file())
}

/// Count texture files directly inside a skin folder, excluding preview and livery images.
fn count_textures(skin_dir: &Path) -> usize {
    let Ok(read_dir) = fs::read_dir(skin_dir) else {
        return 0;
    };
    read_dir
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_lowercase();
            !NON_TEXTURE_FILES.contains(&file_name.as_str())
        })
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| {
                    let ext = ext.to_string_lossy().to_lowercase();
                    TEXTURE_EXTENSIONS.contains(&ext.as_str())
                })
                .unwrap_or(false)
        })
        .count()
}

/// Parse `ui_skin.json`, tolerating a UTF-8 BOM and trailing commas.
/// Returns `None` when the file is missing or still invalid after repair.
fn read_manifest(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&strip_trailing_commas(text)))
        .ok()
}

/// Remove commas that directly precede `}` or `]` (ignoring whitespace),
/// leaving string contents untouched.
fn strip_trailing_commas(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut in_string = false;
    let mut escaped = false;

    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            ',' => {
                let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
                if !matches!(next, Some('}') | Some(']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// String value of a manifest field. Numbers are accepted and stringified.
fn manifest_str(manifest: Option<&Value>, key: &str) -> Option<String> {
    match manifest?.get(key)? {
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Display name from a folder id, e.g. `01_ferrari_red` -> `Ferrari Red`.
fn name_from_id(id: &str) -> String {
    let trimmed = id.trim_start_matches(|c: char| c.is_ascii_digit());
    let trimmed = trimmed.strip_prefix('_').unwrap_or(trimmed);
    let base = if trimmed.is_empty() { id } else { trimmed };
    base.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temp AC root that is removed on drop, even when a test panics.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "acxs_skins_test_{}_{}",
                std::process::id(),
                name
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            TempRoot(root)
        }

        fn skin_dir(&self, car: &str, skin: &str) -> PathBuf {
            let dir = self.0.join("content").join("cars").join(car).join("skins").join(skin);
            fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_ui_skin_json_fields() {
        let root = TempRoot::new("json_present");
        let dir = root.skin_dir("ks_audi", "01_red");
        fs::write(
            dir.join("ui_skin.json"),
            r#"{"skinname": "Racing Red", "drivername": "J. Doe", "team": "Team A", "number": 7, "priority": 2}"#,
        )
        .unwrap();
        fs::write(dir.join("preview.jpg"), b"x").unwrap();
        fs::write(dir.join("livery.png"), b"x").unwrap();
        fs::write(dir.join("body.dds"), b"x").unwrap();
        fs::write(dir.join("body_ext.DDS"), b"x").unwrap();
        fs::write(dir.join("notes.txt"), b"x").unwrap();

        let skins = scan_skins(&root.0, "ks_audi");
        assert_eq!(skins.len(), 1);
        let s = &skins[0];
        assert_eq!(s.car_id, "ks_audi");
        assert_eq!(s.skin_id, "01_red");
        assert_eq!(s.name, "Racing Red");
        assert_eq!(s.driver, "J. Doe");
        assert_eq!(s.team, "Team A");
        assert_eq!(s.number, "7");
        assert_eq!(s.preview, Some(dir.join("preview.jpg")));
        // preview.jpg and livery.png are excluded; notes.txt is not a texture.
        assert_eq!(s.texture_count, 2);
        assert_eq!(s.path, dir);
    }

    #[test]
    fn missing_json_falls_back_to_folder_name() {
        let root = TempRoot::new("json_missing");
        root.skin_dir("ks_audi", "03_blue_ice");
        let skins = scan_skins(&root.0, "ks_audi");
        assert_eq!(skins.len(), 1);
        assert_eq!(skins[0].skin_id, "03_blue_ice");
        assert_eq!(skins[0].name, "Blue Ice");
        assert_eq!(skins[0].driver, "");
        assert_eq!(skins[0].preview, None);
    }

    #[test]
    fn invalid_json_with_trailing_commas_is_tolerated() {
        let root = TempRoot::new("json_invalid");
        let dir = root.skin_dir("ks_audi", "02_black");
        fs::write(
            dir.join("ui_skin.json"),
            "\u{feff}{\n  \"skinname\": \"Stealth, Black\",\n  \"team\": \"Crew\",\n  \"tags\": [\"a\", \"b\",],\n}",
        )
        .unwrap();
        let skins = scan_skins(&root.0, "ks_audi");
        assert_eq!(skins[0].name, "Stealth, Black");
        assert_eq!(skins[0].team, "Crew");
    }

    #[test]
    fn totally_broken_json_falls_back() {
        let root = TempRoot::new("json_broken");
        let dir = root.skin_dir("ks_audi", "04_green");
        fs::write(dir.join("ui_skin.json"), "{ not json at all").unwrap();
        let skins = scan_skins(&root.0, "ks_audi");
        assert_eq!(skins[0].name, "Green");
        assert_eq!(skins[0].driver, "");
    }

    #[test]
    fn preview_prefers_jpg_then_png() {
        let root = TempRoot::new("preview");
        let dir = root.skin_dir("ks_audi", "05_only_png");
        assert_eq!(preview_for(&dir), None);
        fs::write(dir.join("preview.png"), b"x").unwrap();
        assert_eq!(preview_for(&dir), Some(dir.join("preview.png")));
        fs::write(dir.join("preview.jpg"), b"x").unwrap();
        assert_eq!(preview_for(&dir), Some(dir.join("preview.jpg")));
    }

    #[test]
    fn counts_skins_per_car_without_parsing() {
        let root = TempRoot::new("counts");
        root.skin_dir("ks_audi", "a");
        root.skin_dir("ks_audi", "b");
        root.skin_dir("ks_bmw", "only");
        fs::create_dir_all(root.0.join("content").join("cars").join("ks_empty")).unwrap();
        // A stray file in skins/ is not a skin.
        fs::write(
            root.0.join("content").join("cars").join("ks_audi").join("skins").join("readme.txt"),
            b"x",
        )
        .unwrap();

        let counts = scan_all_skin_counts(&root.0);
        assert_eq!(counts.get("ks_audi"), Some(&2));
        assert_eq!(counts.get("ks_bmw"), Some(&1));
        assert_eq!(counts.get("ks_empty"), Some(&0));
        assert_eq!(counts.len(), 3);
    }

    #[test]
    fn missing_dirs_are_tolerated() {
        let root = TempRoot::new("missing");
        assert!(scan_skins(&root.0, "nope").is_empty());
        assert!(scan_all_skin_counts(&root.0).is_empty());
    }

    #[test]
    fn scan_is_sorted_by_name() {
        let root = TempRoot::new("sorted");
        let zed = root.skin_dir("ks_audi", "z");
        let alpha = root.skin_dir("ks_audi", "a");
        fs::write(zed.join("ui_skin.json"), r#"{"skinname": "Zebra"}"#).unwrap();
        fs::write(alpha.join("ui_skin.json"), r#"{"skinname": "alpha"}"#).unwrap();
        let names: Vec<String> = scan_skins(&root.0, "ks_audi").into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["alpha", "Zebra"]);
    }

    #[test]
    fn filter_matches_all_terms() {
        let root = TempRoot::new("filter");
        let red = root.skin_dir("ks_audi", "01_red");
        let blue = root.skin_dir("ks_audi", "02_blue");
        fs::write(
            red.join("ui_skin.json"),
            r#"{"skinname": "Racing Red", "drivername": "Jane Doe", "team": "Apex"}"#,
        )
        .unwrap();
        fs::write(blue.join("ui_skin.json"), r#"{"skinname": "Night Blue", "team": "Bolt"}"#).unwrap();
        let skins = scan_skins(&root.0, "ks_audi");

        assert_eq!(filter_skins(&skins, "").len(), 2);
        let by_driver = filter_skins(&skins, "jane");
        assert_eq!(by_driver.len(), 1);
        assert_eq!(by_driver[0].skin_id, "01_red");
        assert_eq!(filter_skins(&skins, "APEX racing").len(), 1);
        assert_eq!(filter_skins(&skins, "apex bolt").len(), 0);
    }

    #[test]
    fn total_texture_count_sums_per_skin_counts() {
        let root = TempRoot::new("totals");
        let red = root.skin_dir("ks_audi", "01_red");
        let blue = root.skin_dir("ks_audi", "02_blue");
        root.skin_dir("ks_audi", "03_empty");
        fs::write(red.join("body.dds"), b"x").unwrap();
        fs::write(red.join("wheel.png"), b"x").unwrap();
        fs::write(red.join("preview.jpg"), b"x").unwrap();
        fs::write(blue.join("body.dds"), b"x").unwrap();
        let skins = scan_skins(&root.0, "ks_audi");

        let count_of = |id: &str| skins.iter().find(|s| s.skin_id == id).unwrap().texture_count;
        assert_eq!(count_of("01_red"), 2);
        assert_eq!(count_of("02_blue"), 1);
        assert_eq!(count_of("03_empty"), 0);
        assert_eq!(total_texture_count(&skins), 3);
        assert_eq!(total_texture_count(&[]), 0);
    }

    #[test]
    fn summary_lists_set_fields_and_texture_count() {
        let root = TempRoot::new("summary");
        let dir = root.skin_dir("ks_audi", "01_red");
        fs::write(
            dir.join("ui_skin.json"),
            r#"{"skinname": "Racing Red", "drivername": "J. Doe", "team": "Team A", "number": 7}"#,
        )
        .unwrap();
        fs::write(dir.join("body.dds"), b"x").unwrap();
        fs::write(dir.join("trim.dds"), b"x").unwrap();
        let skins = scan_skins(&root.0, "ks_audi");
        assert_eq!(
            skin_summary(&skins[0]),
            "Racing Red · J. Doe · Team A · #7 · 2 textures"
        );
    }

    #[test]
    fn summary_skips_empty_fields_and_singular_noun() {
        let root = TempRoot::new("summary_bare");
        let dir = root.skin_dir("ks_audi", "04_green");
        fs::write(dir.join("body.dds"), b"x").unwrap();
        let skins = scan_skins(&root.0, "ks_audi");
        assert_eq!(skin_summary(&skins[0]), "Green · 1 texture");

        root.skin_dir("ks_audi", "05_none");
        let skins = scan_skins(&root.0, "ks_audi");
        let bare = skins.iter().find(|s| s.skin_id == "05_none").unwrap();
        assert_eq!(skin_summary(bare), "None · 0 textures");
    }
}
