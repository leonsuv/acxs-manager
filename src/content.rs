//! Scan an Assetto Corsa install for cars and tracks.
//!
//! Reads `ui/ui_car.json` and `ui/ui_track.json`. Those files are often
//! hand-edited and sometimes not strict JSON (trailing commas, BOMs), so the
//! reader is lenient and falls back to the folder name when a field is missing.

use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CarEntry {
    pub id: String,
    pub name: String,
    pub brand: String,
    pub car_class: String,
    pub year: String,
    pub tags: Vec<String>,
    pub parent: Option<String>,
    pub country: String,
    pub author: String,
    pub bhp: Option<f64>,
    pub weight_kg: Option<f64>,
    pub pwratio: Option<f64>,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackEntry {
    pub id: String,
    pub name: String,
    pub country: String,
    pub city: String,
    pub length_m: Option<f64>,
    pub pitboxes: Option<u32>,
    pub layouts: Vec<String>,
    pub tags: Vec<String>,
    pub path: PathBuf,
}

/// Parse a ui JSON file. Returns an empty map when missing or unreadable.
pub fn read_ui_json(path: &Path) -> Map<String, Value> {
    let Ok(raw) = fs::read(path) else {
        return Map::new();
    };
    let text = String::from_utf8_lossy(&raw);
    let text = text.trim_start_matches('\u{feff}');
    let parsed = serde_json::from_str::<Value>(text)
        .or_else(|_| serde_json::from_str::<Value>(&strip_trailing_commas(text)))
        .or_else(|_| serde_json::from_str::<Value>(&strip_trailing_commas(&escape_control_chars(text))));
    match parsed {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Escape raw newlines and tabs inside strings. The game accepts them, strict JSON does not,
/// and many stock `ui_car.json` files contain them.
fn escape_control_chars(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut escaped) = (false, false);
    for c in text.chars() {
        if in_string {
            match c {
                _ if escaped => {
                    escaped = false;
                    out.push(c);
                }
                '\\' => {
                    escaped = true;
                    out.push(c);
                }
                '"' => {
                    in_string = false;
                    out.push(c);
                }
                '\n' => out.push_str("\\n"),
                '\r' => {}
                '\t' => out.push_str("\\t"),
                c => out.push(c),
            }
        } else {
            if c == '"' {
                in_string = true;
            }
            out.push(c);
        }
    }
    out
}

/// Remove commas that directly precede `}` or `]` (outside of strings).
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            out.push(c);
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|ch| !ch.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn text_field(map: &Map<String, Value>, key: &str) -> String {
    match map.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn number_field(map: &Map<String, Value>, key: &str) -> Option<f64> {
    match map.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => parse_lenient_f64(s),
        _ => None,
    }
}

/// Leniently parse the first numeric value in a string such as `"570 bhp"`,
/// `"1,350 kg"`, `"2.37 kg/hp"`, or `"~450bhp"`.
pub fn parse_lenient_f64(s: &str) -> Option<f64> {
    let bytes = s.as_bytes();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_digit()
            || (b == b'-' && bytes.get(i + 1).is_some_and(|n| n.is_ascii_digit()))
        {
            start = Some(i);
            break;
        }
    }
    let start = start?;
    let mut token = String::new();
    let mut seen_dot = false;
    for (offset, &b) in bytes[start..].iter().enumerate() {
        if offset == 0 && b == b'-' {
            token.push('-');
        } else if b.is_ascii_digit() {
            token.push(b as char);
        } else if b == b',' {
            // Thousands separator inside a number, e.g. "1,350 kg"
            if bytes
                .get(start + offset + 1)
                .is_some_and(|n| n.is_ascii_digit())
            {
                continue;
            }
            break;
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
            token.push('.');
        } else {
            break;
        }
    }
    token.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn spec_number_field(map: &Map<String, Value>, key: &str) -> Option<f64> {
    map.get("specs")
        .and_then(|v| v.as_object())
        .and_then(|specs| number_field(specs, key))
        .or_else(|| number_field(map, key))
}

fn tag_field(map: &Map<String, Value>) -> Vec<String> {
    match map.get("tags") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        _ => Vec::new(),
    }
}

fn is_visible_dir(path: &Path) -> bool {
    path.is_dir()
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| !n.starts_with('.'))
}

fn sorted_children(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut children: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    children.sort();
    children
}

pub fn scan_cars(ac_root: &Path) -> Vec<CarEntry> {
    let mut cars: Vec<CarEntry> = sorted_children(&ac_root.join("content").join("cars"))
        .into_iter()
        .filter(|p| is_visible_dir(p))
        .filter_map(|folder| {
            let ui = read_ui_json(&folder.join("ui").join("ui_car.json"));
            if ui.is_empty() && !folder.join("data").is_dir() && !folder.join("data.acd").is_file() {
                return None;
            }
            let id = folder.file_name()?.to_string_lossy().into_owned();
            Some(CarEntry {
                name: Some(text_field(&ui, "name"))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| id.clone()),
                brand: text_field(&ui, "brand"),
                car_class: text_field(&ui, "class"),
                year: text_field(&ui, "year"),
                tags: tag_field(&ui),
                parent: Some(text_field(&ui, "parent")).filter(|s| !s.is_empty()),
                country: text_field(&ui, "country"),
                author: text_field(&ui, "author"),
                bhp: spec_number_field(&ui, "bhp"),
                weight_kg: spec_number_field(&ui, "weight"),
                pwratio: spec_number_field(&ui, "pwratio"),
                id,
                path: folder,
            })
        })
        .collect();
    cars.sort_by_key(|c| (c.name.to_lowercase(), c.id.clone()));
    cars
}

/// Layout folder names of a track, sorted and without duplicates.
///
/// Assetto Corsa keeps one `ui/<layout>/ui_track.json` per layout (the layout's
/// data sits in `<track>/<layout>/data`). A `<track>/<layout>/ui` folder is
/// accepted too, for hand-made tracks that use that shape.
pub fn track_layouts(track_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = sorted_children(&track_dir.join("ui"))
        .into_iter()
        .filter(|sub| is_visible_dir(sub) && sub.join("ui_track.json").is_file())
        .chain(
            sorted_children(track_dir)
                .into_iter()
                .filter(|sub| is_visible_dir(sub) && sub.join("ui").is_dir()),
        )
        .filter_map(|sub| sub.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|name| !name.eq_ignore_ascii_case("ui"))
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn scan_tracks(ac_root: &Path) -> Vec<TrackEntry> {
    let mut tracks: Vec<TrackEntry> = sorted_children(&ac_root.join("content").join("tracks"))
        .into_iter()
        .filter(|p| is_visible_dir(p))
        .filter_map(|folder| {
            let layouts = track_layouts(&folder);
            let mut ui = read_ui_json(&folder.join("ui").join("ui_track.json"));
            // Tracks with layouts usually have no top-level ui_track.json: the
            // name, country and length then come from the first layout.
            if ui.is_empty() {
                ui = layouts
                    .iter()
                    .map(|layout| read_ui_json(&folder.join("ui").join(layout).join("ui_track.json")))
                    .find(|map| !map.is_empty())
                    .unwrap_or_default();
            }
            if ui.is_empty() && !folder.join("data").is_dir() && layouts.is_empty() {
                return None;
            }
            let id = folder.file_name()?.to_string_lossy().into_owned();
            Some(TrackEntry {
                name: Some(text_field(&ui, "name"))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| id.clone()),
                country: text_field(&ui, "country"),
                city: text_field(&ui, "city"),
                length_m: number_field(&ui, "length"),
                pitboxes: number_field(&ui, "pitboxes").map(|v| v as u32),
                layouts,
                tags: tag_field(&ui),
                id,
                path: folder,
            })
        })
        .collect();
    tracks.sort_by_key(|t| (t.name.to_lowercase(), t.id.clone()));
    tracks
}

pub fn filter_cars<'a>(cars: &'a [CarEntry], query: &str) -> Vec<&'a CarEntry> {
    cars.iter()
        .filter(|c| {
            crate::util::matches(
                query,
                &[&c.name, &c.id, &c.brand, &c.car_class, &c.tags.join(" ")],
            )
        })
        .collect()
}

pub fn filter_tracks<'a>(tracks: &'a [TrackEntry], query: &str) -> Vec<&'a TrackEntry> {
    tracks
        .iter()
        .filter(|t| crate::util::matches(query, &[&t.name, &t.id, &t.country, &t.city]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("acxs-content-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn parses_bom_and_trailing_commas() {
        let root = fixture("json");
        let file = root.join("ui.json");
        write(&file, "\u{feff}{\"name\": \"Ferrari, 488\", \"tags\": [\"gt3\", ], }");
        let map = read_ui_json(&file);
        assert_eq!(text_field(&map, "name"), "Ferrari, 488");
        assert_eq!(tag_field(&map), vec!["gt3".to_string()]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scans_cars_and_falls_back_to_folder_name() {
        let root = fixture("cars");
        write(
            &root.join("content/cars/ks_ferrari/ui/ui_car.json"),
            r#"{"name":"Ferrari 488 GT3","brand":"Ferrari","class":"gt3","tags":["gt3"]}"#,
        );
        fs::create_dir_all(root.join("content/cars/unnamed/data")).unwrap();
        fs::create_dir_all(root.join("content/cars/junk")).unwrap();
        let cars = scan_cars(&root);
        assert_eq!(cars.len(), 2);
        assert_eq!(cars[0].name, "Ferrari 488 GT3");
        assert_eq!(cars[1].name, "unnamed");
        assert_eq!(filter_cars(&cars, "ferrari gt3").len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scans_kunos_style_layouts_from_ui_folder() {
        let root = fixture("kunos_layouts");
        write(
            &root.join("content/tracks/ks_vallelunga/ui/club_circuit/ui_track.json"),
            r#"{"name":"Vallelunga Club","country":"Italy","length":"1750"}"#,
        );
        write(
            &root.join("content/tracks/ks_vallelunga/ui/extended_circuit/ui_track.json"),
            r#"{"name":"Vallelunga Extended","country":"Italy"}"#,
        );
        fs::create_dir_all(root.join("content/tracks/ks_vallelunga/club_circuit/data")).unwrap();
        let tracks = scan_tracks(&root);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].layouts, vec!["club_circuit".to_string(), "extended_circuit".to_string()]);
        assert_eq!(tracks[0].name, "Vallelunga Club");
        assert_eq!(tracks[0].country, "Italy");
        assert_eq!(tracks[0].length_m, Some(1750.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scans_tracks_with_layouts() {
        let root = fixture("tracks");
        write(
            &root.join("content/tracks/monza/ui/ui_track.json"),
            r#"{"name":"Monza","country":"Italy","city":"Monza","length":"5793","pitboxes":"20"}"#,
        );
        fs::create_dir_all(root.join("content/tracks/monza/gp/ui")).unwrap();
        let tracks = scan_tracks(&root);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].length_m, Some(5793.0));
        assert_eq!(tracks[0].pitboxes, Some(20));
        assert_eq!(tracks[0].layouts, vec!["gp".to_string()]);
        assert_eq!(filter_tracks(&tracks, "italy").len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_car_specs_and_track_tags_leniently() {
        let root = fixture("specs_and_tags");
        write(
            &root.join("content/cars/ferrari_488_s1/ui/ui_car.json"),
            r##"{
                "name": "Ferrari 488 GT3 S1",
                "brand": "Ferrari",
                "class": "race",
                "country": "Italy",
                "author": "Kunos",
                "parent": "ks_ferrari_488_gt3",
                "specs": {
                    "bhp": "570 bhp",
                    "weight": "1,260 kg",
                    "pwratio": "2.21 kg/hp"
                },
                "tags": ["#GT3", "italy"]
            }"##,
        );
        write(
            &root.join("content/cars/no_specs/ui/ui_car.json"),
            r#"{
                "name": "Mystery Car",
                "specs": { "bhp": "N/A", "weight": "--" }
            }"#,
        );
        write(
            &root.join("content/tracks/spa/ui/ui_track.json"),
            r#"{
                "name": "Spa-Francorchamps",
                "country": "Belgium",
                "tags": ["circuit", "gp", "belgium"]
            }"#,
        );

        let cars = scan_cars(&root);
        let ferrari = cars.iter().find(|c| c.id == "ferrari_488_s1").unwrap();
        assert_eq!(ferrari.parent.as_deref(), Some("ks_ferrari_488_gt3"));
        assert_eq!(ferrari.country, "Italy");
        assert_eq!(ferrari.author, "Kunos");
        assert_eq!(ferrari.bhp, Some(570.0));
        assert_eq!(ferrari.weight_kg, Some(1260.0));
        assert_eq!(ferrari.pwratio, Some(2.21));

        let mystery = cars.iter().find(|c| c.id == "no_specs").unwrap();
        assert_eq!(mystery.parent, None);
        assert_eq!(mystery.bhp, None);
        assert_eq!(mystery.weight_kg, None);
        assert_eq!(mystery.pwratio, None);

        let tracks = scan_tracks(&root);
        assert_eq!(tracks.len(), 1);
        assert_eq!(
            tracks[0].tags,
            vec!["circuit".to_string(), "gp".to_string(), "belgium".to_string()]
        );

        fs::remove_dir_all(root).unwrap();
    }
}
