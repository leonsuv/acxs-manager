//! Export the installed car and track lists to CSV and JSON.
//!
//! CSV follows RFC 4180: CRLF line endings, and fields containing a comma,
//! quote or line break are quoted with embedded quotes doubled. Output is
//! deterministic and keeps the order of the input slice. Multi-value fields
//! (tags, layouts) are joined with `;` in CSV and kept as arrays in JSON.

use crate::content::{CarEntry, TrackEntry};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

const NEWLINE: &str = "\r\n";
const LIST_SEPARATOR: &str = ";";
const CAR_HEADER: [&str; 6] = ["id", "name", "brand", "class", "year", "tags"];
const TRACK_HEADER: [&str; 7] = [
    "id",
    "name",
    "country",
    "city",
    "length_m",
    "pitboxes",
    "layouts",
];

/// Installed cars as CSV: header row, then one row per car.
pub fn cars_csv(cars: &[CarEntry]) -> String {
    let mut out = csv_line(CAR_HEADER);
    for car in cars {
        let tags = car.tags.join(LIST_SEPARATOR);
        out.push_str(&csv_line([
            car.id.as_str(),
            car.name.as_str(),
            car.brand.as_str(),
            car.car_class.as_str(),
            car.year.as_str(),
            tags.as_str(),
        ]));
    }
    out
}

/// Installed tracks as CSV: header row, then one row per track.
///
/// Missing length or pitbox counts are written as empty fields.
pub fn tracks_csv(tracks: &[TrackEntry]) -> String {
    let mut out = csv_line(TRACK_HEADER);
    for track in tracks {
        let length = track.length_m.map(|v| v.to_string()).unwrap_or_default();
        let pitboxes = track.pitboxes.map(|v| v.to_string()).unwrap_or_default();
        let layouts = track.layouts.join(LIST_SEPARATOR);
        out.push_str(&csv_line([
            track.id.as_str(),
            track.name.as_str(),
            track.country.as_str(),
            track.city.as_str(),
            length.as_str(),
            pitboxes.as_str(),
            layouts.as_str(),
        ]));
    }
    out
}

/// Installed cars as a pretty-printed JSON array of objects.
pub fn cars_json(cars: &[CarEntry]) -> String {
    let rows: Vec<Value> = cars
        .iter()
        .map(|car| {
            json!({
                "id": car.id,
                "name": car.name,
                "brand": car.brand,
                "class": car.car_class,
                "year": car.year,
                "tags": car.tags,
            })
        })
        .collect();
    to_pretty(&rows)
}

/// Installed tracks as a pretty-printed JSON array of objects.
///
/// Missing length or pitbox counts become `null`.
pub fn tracks_json(tracks: &[TrackEntry]) -> String {
    let rows: Vec<Value> = tracks
        .iter()
        .map(|track| {
            json!({
                "id": track.id,
                "name": track.name,
                "country": track.country,
                "city": track.city,
                "length_m": track.length_m,
                "pitboxes": track.pitboxes,
                "layouts": track.layouts,
            })
        })
        .collect();
    to_pretty(&rows)
}

/// Keeps only the items whose id appears in `ids`, in input order.
///
/// Ids in `ids` that match nothing are ignored.
pub fn filter_by_ids<'a, T>(
    items: &'a [T],
    ids: &[&str],
    id_of: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    let wanted: HashSet<&str> = ids.iter().copied().collect();
    items
        .iter()
        .filter(|item| wanted.contains(id_of(item)))
        .collect()
}

/// Writes `content` to a new file at `path`, creating parent directories.
///
/// Never overwrites: if `path` already exists the call fails with
/// [`io::ErrorKind::AlreadyExists`] and the existing file is left untouched.
/// Nothing is deleted.
pub fn write_export(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(content.as_bytes())
}

/// Joins fields into one CRLF-terminated CSV record.
fn csv_line<'a>(fields: impl IntoIterator<Item = &'a str>) -> String {
    let escaped: Vec<String> = fields.into_iter().map(escape_field).collect();
    let mut line = escaped.join(",");
    line.push_str(NEWLINE);
    line
}

/// Quotes a field if it contains a comma, double quote, CR or LF (RFC 4180).
fn escape_field(field: &str) -> String {
    if !field.contains([',', '"', '\r', '\n']) {
        return field.to_string();
    }
    let mut out = String::with_capacity(field.len() + 2);
    out.push('"');
    for c in field.chars() {
        if c == '"' {
            out.push_str("\"\"");
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

fn to_pretty(rows: &[Value]) -> String {
    // Serializing a `Value` cannot fail.
    serde_json::to_string_pretty(rows).expect("serializing JSON values cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn car(id: &str, name: &str, tags: &[&str]) -> CarEntry {
        CarEntry {
            id: id.to_string(),
            name: name.to_string(),
            brand: "Ferrari".to_string(),
            car_class: "gt3".to_string(),
            year: "2015".to_string(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            path: PathBuf::from("/ac/content/cars").join(id),
            ..CarEntry::default()
        }
    }

    fn track(id: &str, length_m: Option<f64>, pitboxes: Option<u32>) -> TrackEntry {
        TrackEntry {
            id: id.to_string(),
            name: "Monza".to_string(),
            country: "Italy".to_string(),
            city: "Monza".to_string(),
            length_m,
            pitboxes,
            layouts: vec!["gp".to_string(), "short".to_string()],
            path: PathBuf::from("/ac/content/tracks").join(id),
            ..TrackEntry::default()
        }
    }

    #[test]
    fn cars_csv_has_header_and_escapes_name() {
        let cars = vec![car("ks_ferrari", "Ferrari, \"488\" GT3", &["gt3"])];
        let csv = cars_csv(&cars);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines[0], "id,name,brand,class,year,tags");
        assert_eq!(
            lines[1],
            "ks_ferrari,\"Ferrari, \"\"488\"\" GT3\",Ferrari,gt3,2015,gt3"
        );
        assert_eq!(lines.len(), 3, "two records plus trailing empty split item");
        assert!(csv.ends_with("\r\n"));
        assert!(!csv.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn cars_csv_joins_tags_with_semicolons() {
        let cars = vec![car("a", "A", &["gt3", "2015", "hot"])];
        let csv = cars_csv(&cars);
        assert!(csv.ends_with(",gt3;2015;hot\r\n"), "got {csv:?}");
    }

    #[test]
    fn cars_csv_empty_is_header_only() {
        assert_eq!(cars_csv(&[]), "id,name,brand,class,year,tags\r\n");
    }

    #[test]
    fn tracks_csv_has_header_and_empty_optional_fields() {
        let tracks = vec![track("monza", None, Some(20)), track("spa", Some(7004.5), None)];
        let csv = tracks_csv(&tracks);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines[0], "id,name,country,city,length_m,pitboxes,layouts");
        assert_eq!(lines[1], "monza,Monza,Italy,Monza,,20,gp;short");
        assert_eq!(lines[2], "spa,Monza,Italy,Monza,7004.5,,gp;short");
    }

    #[test]
    fn cars_json_round_trips() {
        let cars = vec![
            car("ks_ferrari", "Ferrari, \"488\"", &["gt3", "hot"]),
            car("abarth", "Abarth", &[]),
        ];
        let text = cars_json(&cars);
        let parsed: Value = serde_json::from_str(&text).expect("valid JSON");
        let rows = parsed.as_array().expect("top-level array");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "ks_ferrari");
        assert_eq!(rows[0]["name"], "Ferrari, \"488\"");
        assert_eq!(rows[0]["class"], "gt3");
        assert_eq!(rows[0]["year"], "2015");
        assert_eq!(rows[0]["tags"], json!(["gt3", "hot"]));
        assert_eq!(rows[1]["tags"], json!([]));
    }

    #[test]
    fn tracks_json_uses_null_for_missing_numbers() {
        let tracks = vec![track("monza", None, Some(20))];
        let parsed: Value = serde_json::from_str(&tracks_json(&tracks)).expect("valid JSON");
        assert_eq!(parsed[0]["length_m"], Value::Null);
        assert_eq!(parsed[0]["pitboxes"], 20);
        assert_eq!(parsed[0]["layouts"], json!(["gp", "short"]));
    }

    #[test]
    fn filter_by_ids_keeps_input_order_and_ignores_unknown_ids() {
        let cars = vec![
            car("b", "B", &[]),
            car("a", "A", &[]),
            car("c", "C", &[]),
        ];
        let kept = filter_by_ids(&cars, &["c", "a", "missing"], |c| c.id.as_str());
        let ids: Vec<&str> = kept.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c"]);
    }

    #[test]
    fn write_export_creates_parents_and_refuses_overwrite() {
        let root = std::env::temp_dir().join(format!(
            "acxs-content-export-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("nested").join("cars.csv");

        write_export(&path, "first\r\n").expect("first write succeeds");
        let err = write_export(&path, "second\r\n").expect_err("overwrite refused");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\r\n");

        fs::remove_dir_all(root).unwrap();
    }
}
