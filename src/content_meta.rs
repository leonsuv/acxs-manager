//! Read and patch the metadata Assetto Corsa shows in its own UI.
//!
//! * Cars: `<car>/ui/ui_car.json`
//! * Tracks: `<track>/ui/ui_track.json`
//! * Track layouts: `<track>/ui/<layout>/ui_track.json`, via the `*_track_layout_meta` functions
//!
//! The files are hand-edited, so reading accepts a UTF-8 BOM and trailing commas.
//! Saving writes normal pretty JSON (2-space indent, no BOM). serde_json is built
//! without `preserve_order`, so `Map` is sorted; to keep the file's own key order,
//! the existing file is scanned again on save and its keys are emitted in that
//! order. New keys follow, sorted. Fields this module does not know are kept as-is.
//!
//! Saving renames the existing file to `<name>.acxs-backup-<stamp>` first and never
//! deletes anything.

use chrono::Local;
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Typed view of the car fields in `ui_car.json`. `None` means "not set" when patching.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CarMeta {
    pub name: Option<String>,
    pub brand: Option<String>,
    pub class: Option<String>,
    pub year: Option<i64>,
    pub tags: Option<Vec<String>>,
    pub description: Option<String>,
    /// Merged into the existing `specs` object: keys given here are set, other spec keys are kept.
    pub specs: Option<BTreeMap<String, String>>,
}

/// Typed view of the track fields in `ui_track.json`. `None` means "not set" when patching.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMeta {
    pub name: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Stored under the JSON key `length` (metres; AC usually writes it as a string).
    pub length_m: Option<f64>,
    /// Stored under the JSON key `pitboxes` (AC usually writes it as a string).
    pub pitboxes: Option<u32>,
    pub author: Option<String>,
    pub version: Option<String>,
}

impl CarMeta {
    /// Reads the typed fields out of a raw `ui_car.json` map. Missing or mistyped fields become `None`.
    pub fn from_map(map: &Map<String, Value>) -> Self {
        Self {
            name: get_str(map, "name"),
            brand: get_str(map, "brand"),
            class: get_str(map, "class"),
            year: get_i64(map, "year"),
            tags: get_str_list(map, "tags"),
            description: get_str(map, "description"),
            specs: get_specs(map, "specs"),
        }
    }

    /// Reads `<car_dir>/ui/ui_car.json` into typed fields.
    pub fn read(car_dir: &Path) -> io::Result<Self> {
        read_car_meta(car_dir).map(|map| Self::from_map(&map))
    }
}

impl TrackMeta {
    /// Reads the typed fields out of a raw `ui_track.json` map. Missing or mistyped fields become `None`.
    pub fn from_map(map: &Map<String, Value>) -> Self {
        Self {
            name: get_str(map, "name"),
            city: get_str(map, "city"),
            country: get_str(map, "country"),
            description: get_str(map, "description"),
            tags: get_str_list(map, "tags"),
            length_m: get_f64(map, "length"),
            pitboxes: get_i64(map, "pitboxes").and_then(|v| u32::try_from(v).ok()),
            author: get_str(map, "author"),
            version: get_str(map, "version"),
        }
    }

    /// Reads `<track_dir>/ui/ui_track.json` into typed fields.
    pub fn read(track_dir: &Path) -> io::Result<Self> {
        read_track_meta(track_dir).map(|map| Self::from_map(&map))
    }
}

// ---------------------------------------------------------------------------
// Public file API
// ---------------------------------------------------------------------------

/// Reads `<car_dir>/ui/ui_car.json`. Returns `NotFound` if the file is missing.
pub fn read_car_meta(car_dir: &Path) -> io::Result<Map<String, Value>> {
    read_ui_map(&car_ui_path(car_dir))
}

/// Writes `<car_dir>/ui/ui_car.json`, keeping the old file as a `.acxs-backup-<stamp>` copy.
pub fn save_car_meta(car_dir: &Path, map: &Map<String, Value>) -> io::Result<()> {
    write_ui_map(&car_ui_path(car_dir), map)
}

/// Reads `<track_dir>/ui/ui_track.json`. Returns `NotFound` if the file is missing.
pub fn read_track_meta(track_dir: &Path) -> io::Result<Map<String, Value>> {
    read_ui_map(&track_ui_path(track_dir))
}

/// Writes `<track_dir>/ui/ui_track.json`, keeping the old file as a `.acxs-backup-<stamp>` copy.
pub fn save_track_meta(track_dir: &Path, map: &Map<String, Value>) -> io::Result<()> {
    write_ui_map(&track_ui_path(track_dir), map)
}

/// Reads `<layout_ui_dir>/ui_track.json`, where `layout_ui_dir` is `<track>/ui/<layout>`.
pub fn read_track_layout_meta(layout_ui_dir: &Path) -> io::Result<Map<String, Value>> {
    read_ui_map(&layout_ui_dir.join("ui_track.json"))
}

/// Writes `<layout_ui_dir>/ui_track.json` with the same backup behaviour as `save_track_meta`.
pub fn save_track_layout_meta(layout_ui_dir: &Path, map: &Map<String, Value>) -> io::Result<()> {
    write_ui_map(&layout_ui_dir.join("ui_track.json"), map)
}

/// Read-modify-save helper: applies `patch` to the car's `ui_car.json` and saves it.
pub fn patch_car_meta(car_dir: &Path, patch: &CarMeta) -> io::Result<()> {
    let mut map = read_car_meta(car_dir)?;
    apply_car_patch(&mut map, patch);
    save_car_meta(car_dir, &map)
}

/// Read-modify-save helper: applies `patch` to the track's `ui_track.json` and saves it.
pub fn patch_track_meta(track_dir: &Path, patch: &TrackMeta) -> io::Result<()> {
    let mut map = read_track_meta(track_dir)?;
    apply_track_patch(&mut map, patch);
    save_track_meta(track_dir, &map)
}

/// Sets only the `Some` fields of `patch` in `map`. Everything else is left untouched.
/// Numeric fields that were JSON strings in the file are written back as strings.
/// Non-finite floats are ignored.
pub fn apply_car_patch(map: &mut Map<String, Value>, patch: &CarMeta) {
    if let Some(v) = &patch.name {
        put_str(map, "name", v);
    }
    if let Some(v) = &patch.brand {
        put_str(map, "brand", v);
    }
    if let Some(v) = &patch.class {
        put_str(map, "class", v);
    }
    if let Some(v) = patch.year {
        put_int(map, "year", v);
    }
    if let Some(v) = &patch.tags {
        put_str_list(map, "tags", v);
    }
    if let Some(v) = &patch.description {
        put_str(map, "description", v);
    }
    if let Some(specs) = &patch.specs {
        put_specs(map, "specs", specs);
    }
}

/// Sets only the `Some` fields of `patch` in `map`. Everything else is left untouched.
/// `length_m` is written to `length` and `pitboxes` to `pitboxes`; string-typed values stay strings.
/// Non-finite floats are ignored.
pub fn apply_track_patch(map: &mut Map<String, Value>, patch: &TrackMeta) {
    if let Some(v) = &patch.name {
        put_str(map, "name", v);
    }
    if let Some(v) = &patch.city {
        put_str(map, "city", v);
    }
    if let Some(v) = &patch.country {
        put_str(map, "country", v);
    }
    if let Some(v) = &patch.description {
        put_str(map, "description", v);
    }
    if let Some(v) = &patch.tags {
        put_str_list(map, "tags", v);
    }
    if let Some(v) = patch.length_m {
        put_f64(map, "length", v);
    }
    if let Some(v) = patch.pitboxes {
        put_int(map, "pitboxes", i64::from(v));
    }
    if let Some(v) = &patch.author {
        put_str(map, "author", v);
    }
    if let Some(v) = &patch.version {
        put_str(map, "version", v);
    }
}

// ---------------------------------------------------------------------------
// Paths, reading and writing
// ---------------------------------------------------------------------------

fn car_ui_path(car_dir: &Path) -> PathBuf {
    car_dir.join("ui").join("ui_car.json")
}

fn track_ui_path(track_dir: &Path) -> PathBuf {
    track_dir.join("ui").join("ui_track.json")
}

fn read_ui_map(ui_json: &Path) -> io::Result<Map<String, Value>> {
    let raw = fs::read(ui_json)?;
    parse_document(&raw).map(|(map, _)| map).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {e}", ui_json.display()),
        )
    })
}

fn write_ui_map(ui_json: &Path, map: &Map<String, Value>) -> io::Result<()> {
    // Key order comes from the file as it is now. An unparseable file just gives no order.
    let (existed, order) = match fs::read(ui_json) {
        Ok(raw) => (true, parse_document(&raw).ok().map(|(_, order)| order)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (false, None),
        Err(e) => return Err(e),
    };

    let mut text = String::new();
    write_map(&mut text, map, order.as_ref(), 0);
    text.push('\n');

    if let Some(dir) = ui_json.parent() {
        fs::create_dir_all(dir)?;
    }

    let backup = if existed {
        let backup = backup_path(ui_json);
        fs::rename(ui_json, &backup)?;
        Some(backup)
    } else {
        None
    };

    if let Err(e) = fs::write(ui_json, text.as_bytes()) {
        // Put the original back so a failed save does not lose it.
        if let Some(backup) = &backup {
            let _ = fs::rename(backup, ui_json);
        }
        return Err(e);
    }
    Ok(())
}

/// `<dir>/<file>.acxs-backup-<YYYYmmdd-HHMMSS>`, with `-N` appended if that name is taken.
fn backup_path(ui_json: &Path) -> PathBuf {
    let dir = ui_json.parent().unwrap_or_else(|| Path::new("."));
    let name = ui_json
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ui.json".to_string());
    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();

    let mut candidate = dir.join(format!("{name}.acxs-backup-{stamp}"));
    let mut n = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{name}.acxs-backup-{stamp}-{n}"));
        n += 1;
    }
    candidate
}

// ---------------------------------------------------------------------------
// Lenient parser that also records key order
// ---------------------------------------------------------------------------

/// Key-order skeleton of a parsed document. Objects keep their key order, arrays their items.
enum Order {
    Object(Vec<(String, Order)>),
    Array(Vec<Order>),
    Leaf,
}

/// Parses a `ui_*.json` file. Accepts a UTF-8 BOM and trailing commas.
/// Returns the top-level object and its key order.
fn parse_document(raw: &[u8]) -> Result<(Map<String, Value>, Order), String> {
    let text = String::from_utf8_lossy(raw);
    let text: &str = text.trim_start_matches('\u{feff}');
    let mut parser = Parser { src: text, pos: 0 };
    let (value, order) = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos != text.len() {
        return parser.fail("trailing data after the document");
    }
    match value {
        Value::Object(map) => Ok((map, order)),
        _ => Err("top level is not a JSON object".to_string()),
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() && matches!(bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn fail<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.pos))
    }

    fn parse_value(&mut self) -> Result<(Value, Order), String> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => {
                let token = self.scan_string()?;
                let value = serde_json::from_str::<Value>(token).map_err(|e| e.to_string())?;
                Ok((value, Order::Leaf))
            }
            Some(_) => {
                let token = self.scan_token();
                if token.is_empty() {
                    return self.fail("expected a value");
                }
                let value = serde_json::from_str::<Value>(token)
                    .map_err(|e| format!("invalid value {token:?}: {e}"))?;
                Ok((value, Order::Leaf))
            }
            None => self.fail("unexpected end of file"),
        }
    }

    fn parse_object(&mut self) -> Result<(Value, Order), String> {
        self.pos += 1; // '{'
        let mut map = Map::new();
        let mut order = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                Some(b'"') => {}
                _ => return self.fail("expected a key or '}'"),
            }
            let token = self.scan_string()?;
            let key: String = serde_json::from_str(token).map_err(|e| e.to_string())?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return self.fail("expected ':'");
            }
            self.pos += 1;
            let (value, value_order) = self.parse_value()?;
            map.insert(key.clone(), value);
            order.push((key, value_order));
            self.skip_ws();
            match self.peek() {
                // A trailing comma is fine: the loop then sees '}' and stops.
                Some(b',') => self.pos += 1,
                Some(b'}') => {}
                _ => return self.fail("expected ',' or '}'"),
            }
        }
        Ok((Value::Object(map), Order::Object(order)))
    }

    fn parse_array(&mut self) -> Result<(Value, Order), String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        let mut orders = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b']') {
                self.pos += 1;
                break;
            }
            let (value, order) = self.parse_value()?;
            items.push(value);
            orders.push(order);
            self.skip_ws();
            match self.peek() {
                // A trailing comma is fine: the loop then sees ']' and stops.
                Some(b',') => self.pos += 1,
                Some(b']') => {}
                _ => return self.fail("expected ',' or ']'"),
            }
        }
        Ok((Value::Array(items), Order::Array(orders)))
    }

    /// Returns the raw text of a string literal, quotes included.
    fn scan_string(&mut self) -> Result<&'a str, String> {
        let start = self.pos;
        let bytes = self.src.as_bytes();
        self.pos += 1; // opening quote
        while self.pos < bytes.len() {
            match bytes[self.pos] {
                b'\\' => self.pos += 2,
                b'"' => {
                    self.pos += 1;
                    return Ok(&self.src[start..self.pos]);
                }
                _ => self.pos += 1,
            }
        }
        self.fail("unterminated string")
    }

    /// Returns a number, `true`, `false` or `null` token.
    fn scan_token(&mut self) -> &'a str {
        let start = self.pos;
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len()
            && !matches!(
                bytes[self.pos],
                b',' | b'}' | b']' | b':' | b'{' | b'[' | b'"' | b' ' | b'\t' | b'\r' | b'\n'
            )
        {
            self.pos += 1;
        }
        &self.src[start..self.pos]
    }
}

// ---------------------------------------------------------------------------
// Pretty writer that follows a key order
// ---------------------------------------------------------------------------

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(out: &mut String, value: &Value, order: Option<&Order>, depth: usize) {
    match value {
        Value::Object(map) => write_map(out, map, order, depth),
        Value::Array(items) if !items.is_empty() => {
            let orders: &[Order] = match order {
                Some(Order::Array(orders)) => orders.as_slice(),
                _ => &[],
            };
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                push_indent(out, depth + 1);
                write_value(out, item, orders.get(i), depth + 1);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, depth);
            out.push(']');
        }
        other => {
            out.push_str(&serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()));
        }
    }
}

fn write_map(out: &mut String, map: &Map<String, Value>, order: Option<&Order>, depth: usize) {
    if map.is_empty() {
        out.push_str("{}");
        return;
    }
    let entries: &[(String, Order)] = match order {
        Some(Order::Object(entries)) => entries.as_slice(),
        _ => &[],
    };

    // Keys from the original file first, in their original order; then the rest, sorted.
    let mut keys: Vec<&str> = Vec::with_capacity(map.len());
    for (key, _) in entries {
        if map.contains_key(key) && !keys.contains(&key.as_str()) {
            keys.push(key.as_str());
        }
    }
    for key in map.keys() {
        if !keys.contains(&key.as_str()) {
            keys.push(key.as_str());
        }
    }

    out.push_str("{\n");
    for (i, key) in keys.iter().enumerate() {
        push_indent(out, depth + 1);
        out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string()));
        out.push_str(": ");
        let child = entries
            .iter()
            .find(|(k, _)| k.as_str() == *key)
            .map(|(_, order)| order);
        write_value(out, &map[*key], child, depth + 1);
        if i + 1 < keys.len() {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(out, depth);
    out.push('}');
}

// ---------------------------------------------------------------------------
// Typed getters and setters
// ---------------------------------------------------------------------------

fn get_str(map: &Map<String, Value>, key: &str) -> Option<String> {
    match map.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn get_str_list(map: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    match map.get(key)? {
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// Whole number from a JSON number or a numeric string.
fn get_i64(map: &Map<String, Value>, key: &str) -> Option<i64> {
    match map.get(key)? {
        Value::Number(n) => whole_number(n),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn whole_number(n: &Number) -> Option<i64> {
    n.as_i64().or_else(|| {
        n.as_f64()
            .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
            .map(|f| f as i64)
    })
}

/// Decimal from a JSON number or a numeric string such as "5793".
fn get_f64(map: &Map<String, Value>, key: &str) -> Option<f64> {
    match map.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

fn get_specs(map: &Map<String, Value>, key: &str) -> Option<BTreeMap<String, String>> {
    match map.get(key)? {
        Value::Object(obj) => Some(
            obj.iter()
                .filter_map(|(k, v)| match v {
                    Value::String(s) => Some((k.clone(), s.clone())),
                    Value::Number(n) => Some((k.clone(), n.to_string())),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

fn put_str(map: &mut Map<String, Value>, key: &str, value: &str) {
    map.insert(key.to_string(), Value::String(value.to_string()));
}

fn put_str_list(map: &mut Map<String, Value>, key: &str, items: &[String]) {
    let array = items.iter().map(|s| Value::String(s.clone())).collect();
    map.insert(key.to_string(), Value::Array(array));
}

/// Writes an integer. If the file had this field as a string, it stays a string.
fn put_int(map: &mut Map<String, Value>, key: &str, value: i64) {
    let was_string = matches!(map.get(key), Some(Value::String(_)));
    let value = if was_string {
        Value::String(value.to_string())
    } else {
        Value::from(value)
    };
    map.insert(key.to_string(), value);
}

/// Writes a decimal. Whole values are written without a fraction ("5793", not "5793.0").
/// If the file had this field as a string, it stays a string.
fn put_f64(map: &mut Map<String, Value>, key: &str, value: f64) {
    if !value.is_finite() {
        return;
    }
    let was_string = matches!(map.get(key), Some(Value::String(_)));
    let (text, number) = if value.fract() == 0.0 && value.abs() < 9.0e15 {
        let whole = value as i64;
        (whole.to_string(), Value::from(whole))
    } else {
        (value.to_string(), Value::from(value))
    };
    let value = if was_string {
        Value::String(text)
    } else {
        number
    };
    map.insert(key.to_string(), value);
}

/// Merges `specs` into the existing object under `key`. Other spec keys are kept.
fn put_specs(map: &mut Map<String, Value>, key: &str, specs: &BTreeMap<String, String>) {
    let entry = map
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(obj) = entry {
        for (spec_key, spec_value) in specs {
            put_spec_value(obj, spec_key, spec_value);
        }
    }
}

/// A spec value stays a number when the file had a number and the new text is numeric.
fn put_spec_value(obj: &mut Map<String, Value>, key: &str, text: &str) {
    let was_number = matches!(obj.get(key), Some(Value::Number(_)));
    let parsed = if was_number {
        text.trim().parse::<f64>().ok().filter(|f| f.is_finite())
    } else {
        None
    };
    let value = match parsed {
        Some(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Value::from(f as i64),
        Some(f) => Value::from(f),
        None => Value::String(text.to_string()),
    };
    obj.insert(key.to_string(), value);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp folder that is removed when the test ends, even on panic.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "acxs-content-meta-{}-{}",
                std::process::id(),
                tag
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Writes `<root>/ui/<name>` and returns the file path.
        fn write_ui(&self, name: &str, text: &str) -> PathBuf {
            let ui = self.0.join("ui");
            fs::create_dir_all(&ui).unwrap();
            let file = ui.join(name);
            fs::write(&file, text).unwrap();
            file
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn backups_in(ui_dir: &Path, prefix: &str) -> Vec<PathBuf> {
        fs::read_dir(ui_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(prefix))
                    .unwrap_or(false)
            })
            .collect()
    }

    const CAR_JSON: &str = "\u{feff}{\n  \"name\": \"Test Car\",\n  \"brand\": \"Old Brand\",\n  \"class\": \"gt3\",\n  \"year\": \"1990\",\n  \"tags\": [\"road\", \"fast\",],\n  \"description\": \"A car.\",\n  \"specs\": {\n    \"bhp\": \"300\",\n    \"torque\": \"250\",\n  },\n}\n";

    #[test]
    fn reads_bom_and_trailing_commas() {
        let root = TempRoot::new("read-lenient");
        root.write_ui("ui_car.json", CAR_JSON);

        let map = read_car_meta(root.path()).unwrap();
        assert_eq!(map.get("name").and_then(Value::as_str), Some("Test Car"));

        let typed = CarMeta::read(root.path()).unwrap();
        assert_eq!(typed.name.as_deref(), Some("Test Car"));
        assert_eq!(typed.year, Some(1990));
        assert_eq!(
            typed.tags,
            Some(vec!["road".to_string(), "fast".to_string()])
        );
        let specs = typed.specs.unwrap();
        assert_eq!(specs.get("bhp").map(String::as_str), Some("300"));
        assert_eq!(specs.get("torque").map(String::as_str), Some("250"));
    }

    #[test]
    fn missing_file_is_not_found() {
        let root = TempRoot::new("missing");
        let err = read_car_meta(root.path()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn patch_sets_only_some_fields() {
        let root = TempRoot::new("patch-some");
        root.write_ui("ui_car.json", CAR_JSON);

        let patch = CarMeta {
            brand: Some("New Brand".to_string()),
            year: Some(1995),
            ..Default::default()
        };
        patch_car_meta(root.path(), &patch).unwrap();

        let typed = CarMeta::read(root.path()).unwrap();
        assert_eq!(typed.brand.as_deref(), Some("New Brand"));
        assert_eq!(typed.year, Some(1995));
        // Untouched fields keep their values.
        assert_eq!(typed.name.as_deref(), Some("Test Car"));
        assert_eq!(typed.class.as_deref(), Some("gt3"));
        assert_eq!(typed.description.as_deref(), Some("A car."));
        assert_eq!(
            typed.tags,
            Some(vec!["road".to_string(), "fast".to_string()])
        );
        assert_eq!(typed.specs.unwrap().get("bhp").map(String::as_str), Some("300"));
    }

    #[test]
    fn unknown_fields_survive_patch() {
        let root = TempRoot::new("unknown");
        root.write_ui(
            "ui_car.json",
            "{\n  \"name\": \"X\",\n  \"torqueCurve\": [[0, 1.5], [1, 2]],\n  \"x_custom\": {\"nested\": [true, null, \"s\"]},\n  \"specs\": {\"bhp\": \"300\", \"extra\": \"1\"}\n}\n",
        );

        let patch = CarMeta {
            specs: Some(BTreeMap::from([("bhp".to_string(), "350".to_string())])),
            ..Default::default()
        };
        patch_car_meta(root.path(), &patch).unwrap();

        let map = read_car_meta(root.path()).unwrap();
        assert_eq!(
            map.get("torqueCurve"),
            Some(&serde_json::json!([[0, 1.5], [1, 2]]))
        );
        assert_eq!(
            map.get("x_custom"),
            Some(&serde_json::json!({"nested": [true, null, "s"]}))
        );
        let specs = map.get("specs").and_then(Value::as_object).unwrap();
        assert_eq!(specs.get("bhp").and_then(Value::as_str), Some("350"));
        assert_eq!(specs.get("extra").and_then(Value::as_str), Some("1"));
    }

    #[test]
    fn backup_created_on_save() {
        let root = TempRoot::new("backup");
        let original = root.write_ui("ui_car.json", CAR_JSON);
        let ui_dir = original.parent().unwrap().to_path_buf();

        patch_car_meta(
            root.path(),
            &CarMeta {
                name: Some("Renamed".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let backups = backups_in(&ui_dir, "ui_car.json.acxs-backup-");
        assert_eq!(backups.len(), 1, "expected exactly one backup");
        // The backup is the untouched original.
        assert_eq!(fs::read_to_string(&backups[0]).unwrap(), CAR_JSON);
        // The new file is clean pretty JSON without a BOM.
        let written = fs::read(&original).unwrap();
        assert_ne!(&written[..3], &[0xEF, 0xBB, 0xBF]);
        assert!(written.starts_with(b"{\n  \"name\": \"Renamed\""));
        assert!(written.ends_with(b"}\n"));
    }

    #[test]
    fn save_without_existing_file_creates_it_without_backup() {
        let root = TempRoot::new("fresh");
        let map = {
            let mut m = Map::new();
            m.insert("name".to_string(), Value::String("Fresh".to_string()));
            m
        };
        save_track_meta(root.path(), &map).unwrap();

        let ui_dir = root.path().join("ui");
        assert!(ui_dir.join("ui_track.json").exists());
        assert!(backups_in(&ui_dir, "ui_track.json.acxs-backup-").is_empty());
        assert_eq!(
            read_track_meta(root.path()).unwrap().get("name").and_then(Value::as_str),
            Some("Fresh")
        );
    }

    #[test]
    fn string_numbers_stay_strings() {
        let root = TempRoot::new("string-numbers");
        root.write_ui(
            "ui_track.json",
            "{\"name\": \"T\", \"length\": \"5793\", \"pitboxes\": \"10\", \"width\": 12}",
        );

        patch_track_meta(
            root.path(),
            &TrackMeta {
                length_m: Some(5800.0),
                pitboxes: Some(12),
                ..Default::default()
            },
        )
        .unwrap();

        let map = read_track_meta(root.path()).unwrap();
        assert_eq!(map.get("length"), Some(&Value::String("5800".to_string())));
        assert_eq!(map.get("pitboxes"), Some(&Value::String("12".to_string())));
        // A field that was a number stays a number.
        assert_eq!(map.get("width"), Some(&Value::from(12)));
    }

    #[test]
    fn numeric_fields_stay_numbers_when_they_were_numbers() {
        let root = TempRoot::new("numbers");
        root.write_ui("ui_car.json", "{\"name\": \"N\", \"year\": 2010}");

        patch_car_meta(
            root.path(),
            &CarMeta {
                year: Some(2011),
                ..Default::default()
            },
        )
        .unwrap();

        let map = read_car_meta(root.path()).unwrap();
        assert_eq!(map.get("year"), Some(&Value::from(2011)));
    }

    #[test]
    fn track_patch_writes_new_fields_and_tags() {
        let root = TempRoot::new("track-fields");
        root.write_ui("ui_track.json", "{\"name\": \"T\", \"city\": \"Old\"}");

        patch_track_meta(
            root.path(),
            &TrackMeta {
                city: Some("Monza".to_string()),
                country: Some("Italy".to_string()),
                tags: Some(vec!["gp".to_string()]),
                author: Some("Me".to_string()),
                version: Some("1.2".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let typed = TrackMeta::read(root.path()).unwrap();
        assert_eq!(typed.name.as_deref(), Some("T"));
        assert_eq!(typed.city.as_deref(), Some("Monza"));
        assert_eq!(typed.country.as_deref(), Some("Italy"));
        assert_eq!(typed.tags, Some(vec!["gp".to_string()]));
        assert_eq!(typed.author.as_deref(), Some("Me"));
        assert_eq!(typed.version.as_deref(), Some("1.2"));
        assert_eq!(typed.length_m, None);
        assert_eq!(typed.pitboxes, None);
    }

    #[test]
    fn key_order_of_existing_keys_is_kept() {
        let root = TempRoot::new("order");
        let file = root.write_ui(
            "ui_car.json",
            "{\"zeta\": 1, \"name\": \"Z\", \"alpha\": {\"b\": 2, \"a\": 1}}",
        );

        patch_car_meta(
            root.path(),
            &CarMeta {
                brand: Some("B".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        let text = fs::read_to_string(&file).unwrap();
        let zeta = text.find("\"zeta\"").unwrap();
        let name = text.find("\"name\"").unwrap();
        let alpha = text.find("\"alpha\"").unwrap();
        let b = text.find("\"b\"").unwrap();
        let a = text.find("\"a\"").unwrap();
        assert!(zeta < name && name < alpha, "top-level order changed: {text}");
        assert!(b < a, "nested order changed: {text}");
        // New keys go after the existing ones.
        assert!(alpha < text.find("\"brand\"").unwrap(), "{text}");
    }

    #[test]
    fn layout_meta_reads_and_writes_layout_folder() {
        let root = TempRoot::new("layout");
        let layout_ui = root.path().join("ui").join("short");
        fs::create_dir_all(&layout_ui).unwrap();
        fs::write(layout_ui.join("ui_track.json"), "{\"name\": \"Short\",}").unwrap();

        let mut map = read_track_layout_meta(&layout_ui).unwrap();
        assert_eq!(map.get("name").and_then(Value::as_str), Some("Short"));
        apply_track_patch(
            &mut map,
            &TrackMeta {
                length_m: Some(1200.5),
                ..Default::default()
            },
        );
        save_track_layout_meta(&layout_ui, &map).unwrap();

        let again = read_track_layout_meta(&layout_ui).unwrap();
        assert_eq!(again.get("length"), Some(&Value::from(1200.5)));
        assert_eq!(backups_in(&layout_ui, "ui_track.json.acxs-backup-").len(), 1);
    }
}
