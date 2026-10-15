//! HUD layout model: widget descriptions, validation, JSON persistence and the
//! pure mapping from telemetry values to drawable items.
//!
//! Coordinates are normalised to the screen: `(0, 0)` is the top-left corner
//! and `(1, 1)` the bottom-right. A widget's `x` and `y` give the screen point
//! where the part of the widget named by its `anchor` sits. For example
//! `TopRight` at `x = 0.98` puts the widget's top-right corner at 98% across.
//!
//! Nothing here draws anything. A renderer consumes the [`DrawItem`] list
//! returned by [`draw_items`].

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

/// Tolerance used when comparing normalised coordinates.
const EPS: f32 = 1e-6;

/// Kilometres per hour to miles per hour.
const KMH_TO_MPH: f32 = 0.621_371;

const LAP_TIME_PLACEHOLDER: &str = "--:--.---";
const DELTA_PLACEHOLDER: &str = "--.---";

pub const SOURCE_SPEED: &str = "speed";
pub const SOURCE_GEAR: &str = "gear";
pub const SOURCE_RPM: &str = "rpm";
pub const SOURCE_LAP_TIME: &str = "lap_time";
pub const SOURCE_LAST_LAP: &str = "last_lap";
pub const SOURCE_DELTA: &str = "delta";
pub const SOURCE_FUEL: &str = "fuel";
pub const SOURCE_TYRE_TEMPS: &str = "tyre_temps";
pub const SOURCE_POSITION: &str = "position";
pub const SOURCE_TEXT: &str = "text";

/// Every value source id a widget may reference.
pub const VALUE_SOURCES: &[&str] = &[
    SOURCE_SPEED,
    SOURCE_GEAR,
    SOURCE_RPM,
    SOURCE_LAP_TIME,
    SOURCE_LAST_LAP,
    SOURCE_DELTA,
    SOURCE_FUEL,
    SOURCE_TYRE_TEMPS,
    SOURCE_POSITION,
    SOURCE_TEXT,
];

/// Number of lights drawn by a shift light widget.
pub const SHIFT_LIGHT_COUNT: usize = 8;
/// RPM fraction at which the first shift light comes on.
pub const SHIFT_LIGHT_START: f32 = 0.80;
/// RPM fraction at which the last shift light comes on.
pub const SHIFT_LIGHT_END: f32 = 0.98;

/// An RGBA colour. Serialised as a hex string: `#RRGGBB` or `#RRGGBBAA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    /// Parses `#RRGGBB` or `#RRGGBBAA`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| format!("colour '{s}' must start with '#'"))?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("colour '{s}' must be #RRGGBB or #RRGGBBAA"));
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string());
        let a = if hex.len() == 8 { byte(6)? } else { 255 };
        Ok(Color {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a,
        })
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Color::parse(&s)
    }
}

impl From<Color> for String {
    fn from(c: Color) -> String {
        c.to_hex()
    }
}

fn default_color() -> Color {
    Color::rgb(255, 255, 255)
}

fn default_accent() -> Color {
    Color::rgb(0, 255, 136)
}

/// Which point of a widget is placed at the widget's `(x, y)` position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Anchor {
    /// The anchor point as a fraction of the widget's width and height.
    fn pivot(self) -> (f32, f32) {
        match self {
            Anchor::TopLeft => (0.0, 0.0),
            Anchor::TopCenter => (0.5, 0.0),
            Anchor::TopRight => (1.0, 0.0),
            Anchor::CenterLeft => (0.0, 0.5),
            Anchor::Center => (0.5, 0.5),
            Anchor::CenterRight => (1.0, 0.5),
            Anchor::BottomLeft => (0.0, 1.0),
            Anchor::BottomCenter => (0.5, 1.0),
            Anchor::BottomRight => (1.0, 1.0),
        }
    }
}

/// Unit used for the speed readout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedUnit {
    #[default]
    Kmh,
    Mph,
}

/// The kinds of HUD element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    Speed,
    Gear,
    RpmBar,
    ShiftLights,
    LapTime,
    Delta,
    Fuel,
    TyreTemps,
    Position,
    CustomText,
}

impl WidgetKind {
    /// The value source ids this kind can read. The first is the default.
    pub fn sources(self) -> &'static [&'static str] {
        match self {
            WidgetKind::Speed => &[SOURCE_SPEED],
            WidgetKind::Gear => &[SOURCE_GEAR],
            WidgetKind::RpmBar | WidgetKind::ShiftLights => &[SOURCE_RPM],
            WidgetKind::LapTime => &[SOURCE_LAP_TIME, SOURCE_LAST_LAP],
            WidgetKind::Delta => &[SOURCE_DELTA],
            WidgetKind::Fuel => &[SOURCE_FUEL],
            WidgetKind::TyreTemps => &[SOURCE_TYRE_TEMPS],
            WidgetKind::Position => &[SOURCE_POSITION],
            WidgetKind::CustomText => &[SOURCE_TEXT],
        }
    }

    pub fn default_source(self) -> &'static str {
        self.sources()[0]
    }
}

/// One placed element of the HUD.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    pub kind: WidgetKind,
    #[serde(default)]
    pub anchor: Anchor,
    /// Screen position of the anchor point, in `0..=1`.
    pub x: f32,
    /// Screen position of the anchor point, in `0..=1`.
    pub y: f32,
    /// Size as a fraction of the screen, in `(0, 1]`.
    pub width: f32,
    pub height: f32,
    /// Id of the telemetry value this widget shows.
    pub source: String,
    /// Literal text, used by `CustomText` widgets only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default = "default_color")]
    pub color: Color,
    /// Highlight colour, used by shift lights when lit.
    #[serde(default = "default_accent")]
    pub accent: Color,
    #[serde(default)]
    pub background: Option<Color>,
}

impl Widget {
    /// A widget with the kind's default value source and default colours.
    pub fn new(
        id: &str,
        kind: WidgetKind,
        anchor: Anchor,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Self {
        Widget {
            id: id.to_string(),
            kind,
            anchor,
            x,
            y,
            width,
            height,
            source: kind.default_source().to_string(),
            text: String::new(),
            color: default_color(),
            accent: default_accent(),
            background: None,
        }
    }

    /// The widget's top-left corner and size on the screen.
    pub fn rect(&self) -> Rect {
        let (px, py) = self.anchor.pivot();
        Rect {
            x: self.x - px * self.width,
            y: self.y - py * self.height,
            w: self.width,
            h: self.height,
        }
    }
}

/// An axis-aligned rectangle in normalised screen coordinates, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// True when the two rectangles share a region of positive area.
    /// Rectangles that only touch along an edge do not overlap.
    pub fn overlaps(&self, other: &Rect) -> bool {
        let x0 = self.x.max(other.x);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y0 = self.y.max(other.y);
        let y1 = (self.y + self.h).min(other.y + other.h);
        x1 - x0 > EPS && y1 - y0 > EPS
    }
}

/// A complete HUD layout.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default)]
    pub speed_unit: SpeedUnit,
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

/// A problem found by [`Layout::validate`].
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    EmptyId,
    DuplicateId(String),
    UnknownSource { widget: String, source: String },
    SourceMismatch { widget: String, kind: WidgetKind, source: String },
    PositionOutOfRange { widget: String, x: f32, y: f32 },
    InvalidSize { widget: String, width: f32, height: f32 },
    OutsideScreen { widget: String },
    Overlap { first: String, second: String },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::EmptyId => write!(f, "a widget has an empty id"),
            ValidationError::DuplicateId(id) => write!(f, "duplicate widget id '{id}'"),
            ValidationError::UnknownSource { widget, source } => {
                write!(f, "widget '{widget}' uses unknown value source '{source}'")
            }
            ValidationError::SourceMismatch { widget, kind, source } => {
                write!(f, "widget '{widget}' ({kind:?}) cannot read value source '{source}'")
            }
            ValidationError::PositionOutOfRange { widget, x, y } => {
                write!(f, "widget '{widget}' position ({x}, {y}) is outside 0..=1")
            }
            ValidationError::InvalidSize { widget, width, height } => {
                write!(f, "widget '{widget}' size {width}x{height} must be within (0, 1]")
            }
            ValidationError::OutsideScreen { widget } => {
                write!(f, "widget '{widget}' extends beyond the screen")
            }
            ValidationError::Overlap { first, second } => {
                write!(f, "widgets '{first}' and '{second}' overlap")
            }
        }
    }
}

impl Error for ValidationError {}

/// Errors from loading, parsing or saving a layout.
#[derive(Debug)]
pub enum HudError {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(Vec<ValidationError>),
}

impl fmt::Display for HudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HudError::Io(e) => write!(f, "hud layout I/O error: {e}"),
            HudError::Json(e) => write!(f, "hud layout JSON error: {e}"),
            HudError::Invalid(errors) => {
                let parts: Vec<String> = errors.iter().map(ToString::to_string).collect();
                write!(f, "invalid hud layout: {}", parts.join("; "))
            }
        }
    }
}

impl Error for HudError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            HudError::Io(e) => Some(e),
            HudError::Json(e) => Some(e),
            HudError::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for HudError {
    fn from(e: io::Error) -> Self {
        HudError::Io(e)
    }
}

impl From<serde_json::Error> for HudError {
    fn from(e: serde_json::Error) -> Self {
        HudError::Json(e)
    }
}

fn is_known_source(source: &str) -> bool {
    VALUE_SOURCES.contains(&source)
}

fn kind_accepts(kind: WidgetKind, source: &str) -> bool {
    kind.sources().contains(&source)
}

fn valid_size(v: f32) -> bool {
    v > 0.0 && v <= 1.0
}

impl Layout {
    /// A starting layout with one of each element kind, none overlapping.
    pub fn default_layout() -> Self {
        use crate::hud::Anchor::*;
        use crate::hud::WidgetKind::*;
        Layout {
            speed_unit: SpeedUnit::Kmh,
            widgets: vec![
                Widget::new("speed", Speed, BottomLeft, 0.02, 0.98, 0.14, 0.08),
                Widget::new("gear", Gear, BottomCenter, 0.5, 0.98, 0.06, 0.10),
                Widget::new("rpm", RpmBar, BottomCenter, 0.5, 0.80, 0.30, 0.02),
                Widget::new("shift", ShiftLights, TopCenter, 0.5, 0.03, 0.30, 0.03),
                Widget::new("lap", LapTime, TopLeft, 0.02, 0.03, 0.16, 0.05),
                Widget::new("delta", Delta, TopLeft, 0.02, 0.09, 0.16, 0.05),
                Widget::new("position", Position, TopRight, 0.98, 0.03, 0.08, 0.05),
                Widget::new("tyres", TyreTemps, BottomRight, 0.98, 0.90, 0.20, 0.04),
                Widget::new("fuel", Fuel, BottomRight, 0.98, 0.98, 0.12, 0.05),
            ],
        }
    }

    /// Checks ids, value sources, positions, sizes and overlaps. Returns every
    /// problem found, not just the first.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        let mut ids: Vec<&str> = Vec::new();
        let mut rects: Vec<(&str, Rect)> = Vec::new();

        for w in &self.widgets {
            let id = w.id.as_str();
            if id.trim().is_empty() {
                errors.push(ValidationError::EmptyId);
            } else if ids.contains(&id) {
                errors.push(ValidationError::DuplicateId(w.id.clone()));
            } else {
                ids.push(id);
            }

            if !kind_accepts(w.kind, &w.source) {
                if is_known_source(&w.source) {
                    errors.push(ValidationError::SourceMismatch {
                        widget: w.id.clone(),
                        kind: w.kind,
                        source: w.source.clone(),
                    });
                } else {
                    errors.push(ValidationError::UnknownSource {
                        widget: w.id.clone(),
                        source: w.source.clone(),
                    });
                }
            }

            let position_ok = (0.0..=1.0).contains(&w.x) && (0.0..=1.0).contains(&w.y);
            if !position_ok {
                errors.push(ValidationError::PositionOutOfRange {
                    widget: w.id.clone(),
                    x: w.x,
                    y: w.y,
                });
            }
            let size_ok = valid_size(w.width) && valid_size(w.height);
            if !size_ok {
                errors.push(ValidationError::InvalidSize {
                    widget: w.id.clone(),
                    width: w.width,
                    height: w.height,
                });
            }

            if position_ok && size_ok {
                let r = w.rect();
                if r.x < -EPS || r.y < -EPS || r.x + r.w > 1.0 + EPS || r.y + r.h > 1.0 + EPS {
                    errors.push(ValidationError::OutsideScreen { widget: w.id.clone() });
                } else {
                    rects.push((id, r));
                }
            }
        }

        for (i, (a_id, a)) in rects.iter().enumerate() {
            for (b_id, b) in &rects[i + 1..] {
                if a.overlaps(b) {
                    errors.push(ValidationError::Overlap {
                        first: a_id.to_string(),
                        second: b_id.to_string(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Pretty-printed JSON. Does not validate.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parses JSON and validates the result.
    pub fn from_json(text: &str) -> Result<Self, HudError> {
        let layout: Layout = serde_json::from_str(text)?;
        layout.validate().map_err(HudError::Invalid)?;
        Ok(layout)
    }

    /// Reads and validates a layout file.
    pub fn load(path: &Path) -> Result<Self, HudError> {
        let text = fs::read_to_string(path)?;
        Layout::from_json(&text)
    }

    /// Validates and writes the layout to `path`.
    ///
    /// If the file exists and its bytes differ from the new content, the old
    /// file is copied to a timestamped `.bak` sibling first. The original is
    /// never removed. Returns the backup path, or `None` when no backup was
    /// needed (new file, or unchanged content).
    pub fn save(&self, path: &Path) -> Result<Option<PathBuf>, HudError> {
        self.validate().map_err(HudError::Invalid)?;
        let content = format!("{}\n", self.to_json()?);

        let mut backup = None;
        match fs::read(path) {
            Ok(existing) => {
                if existing == content.as_bytes() {
                    return Ok(None);
                }
                let stamp = Local::now().format("%Y%m%d-%H%M%S%3f").to_string();
                let target = next_backup_path(path, &stamp);
                fs::copy(path, &target)?;
                backup = Some(target);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }

        if let Some(parent) = path.parent() && !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
        let tmp = tmp_path(path);
        fs::write(&tmp, content.as_bytes())?;
        fs::rename(&tmp, path)?;
        Ok(backup)
    }
}

/// `<dir>/<stem>.<stamp>[-n].bak.<ext>`, with `ext` defaulting to `json`.
fn backup_candidate(path: &Path, stamp: &str, n: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("hud");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("json");
    let suffix = if n == 0 { String::new() } else { format!("-{n}") };
    let name = format!("{stem}.{stamp}{suffix}.bak.{ext}");
    match path.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

/// The first backup name for `stamp` that is not already taken.
fn next_backup_path(path: &Path, stamp: &str) -> PathBuf {
    let mut n = 0;
    loop {
        let candidate = backup_candidate(path, stamp, n);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| "hud.json".into());
    name.push(".tmp");
    path.with_file_name(name)
}

/// Gear as shown to the driver: `R`, `N`, then the gear number.
pub fn format_gear(gear: i32) -> String {
    match gear {
        -1 => "R".to_string(),
        0 => "N".to_string(),
        g if g > 0 => g.to_string(),
        _ => "?".to_string(),
    }
}

/// Speed with its unit label, rounded to whole units. Negative values are
/// shown as zero.
pub fn format_speed(kmh: f32, unit: SpeedUnit) -> String {
    let (label, value) = match unit {
        SpeedUnit::Kmh => ("km/h", kmh),
        SpeedUnit::Mph => ("mph", kmh * KMH_TO_MPH),
    };
    if value.is_finite() {
        format!("{:.0} {label}", value.max(0.0))
    } else {
        format!("--- {label}")
    }
}

/// Lap time as `m:ss.mmm`. Minutes are not padded.
pub fn format_lap_time(ms: u32) -> String {
    let minutes = ms / 60_000;
    let seconds = (ms / 1_000) % 60;
    let millis = ms % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

/// Signed delta as seconds with milliseconds, such as `+1.234` or `-0.056`.
/// Zero is shown as `+0.000`.
pub fn format_delta(ms: i32) -> String {
    let sign = if ms < 0 { '-' } else { '+' };
    let abs = i64::from(ms).unsigned_abs();
    format!("{sign}{}.{:03}", abs / 1_000, abs % 1_000)
}

/// Fuel in litres with one decimal place.
pub fn format_fuel(litres: f32) -> String {
    if litres.is_finite() {
        format!("{:.1} L", litres.max(0.0))
    } else {
        "--.- L".to_string()
    }
}

/// Tyre temperatures in whole degrees, in the order front-left, front-right,
/// rear-left, rear-right. Missing readings show as `--`.
pub fn format_tyre_temps(temps: [f32; 4]) -> String {
    const LABELS: [&str; 4] = ["FL", "FR", "RL", "RR"];
    let parts: Vec<String> = LABELS
        .iter()
        .zip(temps.iter())
        .map(|(label, t)| {
            if t.is_finite() {
                format!("{label} {:.0}", t.round())
            } else {
                format!("{label} --")
            }
        })
        .collect();
    parts.join(" ")
}

/// Race position as `P3`, or `P--` when unknown.
pub fn format_position(position: Option<u32>) -> String {
    match position {
        Some(p) => format!("P{p}"),
        None => "P--".to_string(),
    }
}

/// Telemetry values for one frame. Missing optional readings are `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Telemetry {
    pub speed_kmh: f32,
    /// -1 reverse, 0 neutral, 1..n forward gears.
    pub gear: i32,
    pub rpm: f32,
    /// Reference RPM for the bar and shift lights.
    pub max_rpm: f32,
    pub lap_time_ms: Option<u32>,
    pub last_lap_ms: Option<u32>,
    pub delta_ms: Option<i32>,
    pub fuel_litres: f32,
    pub fuel_capacity_litres: f32,
    /// Front-left, front-right, rear-left, rear-right, in degrees Celsius.
    pub tyre_temps_c: [f32; 4],
    pub position: Option<u32>,
}

/// One thing for a renderer to draw. Rectangles are normalised to the screen.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawItem {
    Text {
        widget: String,
        rect: Rect,
        text: String,
        color: Color,
    },
    /// A horizontal bar filled from the left. `fill` is in `0..=1`.
    Bar {
        widget: String,
        rect: Rect,
        fill: f32,
        color: Color,
        background: Option<Color>,
    },
}

/// `value / max` clamped to `0..=1`. Non-finite inputs or a non-positive
/// maximum give `0`.
fn fraction(value: f32, max: f32) -> f32 {
    if !value.is_finite() || !max.is_finite() || max <= 0.0 {
        0.0
    } else {
        (value / max).clamp(0.0, 1.0)
    }
}

/// RPM fraction at which shift light `index` (0-based) comes on.
pub fn shift_light_threshold(index: usize) -> f32 {
    let step = index as f32 / (SHIFT_LIGHT_COUNT - 1) as f32;
    SHIFT_LIGHT_START + (SHIFT_LIGHT_END - SHIFT_LIGHT_START) * step
}

fn text_item(w: &Widget, rect: Rect, text: String) -> DrawItem {
    DrawItem::Text {
        widget: w.id.clone(),
        rect,
        text,
        color: w.color,
    }
}

fn bar_item(w: &Widget, rect: Rect, fill: f32) -> DrawItem {
    DrawItem::Bar {
        widget: w.id.clone(),
        rect,
        fill,
        color: w.color,
        background: w.background,
    }
}

/// Turns telemetry into the items to draw, in layout order. Widgets whose
/// source does not suit their kind are skipped; run [`Layout::validate`]
/// first to catch them.
pub fn draw_items(layout: &Layout, t: &Telemetry) -> Vec<DrawItem> {
    let mut out = Vec::new();
    for w in &layout.widgets {
        if !kind_accepts(w.kind, &w.source) {
            continue;
        }
        let rect = w.rect();
        match w.kind {
            WidgetKind::Speed => {
                out.push(text_item(w, rect, format_speed(t.speed_kmh, layout.speed_unit)));
            }
            WidgetKind::Gear => {
                out.push(text_item(w, rect, format_gear(t.gear)));
            }
            WidgetKind::RpmBar => {
                out.push(bar_item(w, rect, fraction(t.rpm, t.max_rpm)));
            }
            WidgetKind::ShiftLights => {
                let frac = fraction(t.rpm, t.max_rpm);
                let cell = rect.w / SHIFT_LIGHT_COUNT as f32;
                let gap = cell * 0.15;
                for i in 0..SHIFT_LIGHT_COUNT {
                    let lit = frac >= shift_light_threshold(i);
                    let light = Rect {
                        x: rect.x + cell * i as f32 + gap / 2.0,
                        y: rect.y,
                        w: cell - gap,
                        h: rect.h,
                    };
                    out.push(DrawItem::Bar {
                        widget: w.id.clone(),
                        rect: light,
                        fill: if lit { 1.0 } else { 0.0 },
                        color: w.accent,
                        background: w.background,
                    });
                }
            }
            WidgetKind::LapTime => {
                let ms = if w.source == SOURCE_LAST_LAP {
                    t.last_lap_ms
                } else {
                    t.lap_time_ms
                };
                let text = ms.map_or_else(|| LAP_TIME_PLACEHOLDER.to_string(), format_lap_time);
                out.push(text_item(w, rect, text));
            }
            WidgetKind::Delta => {
                let text = t
                    .delta_ms
                    .map_or_else(|| DELTA_PLACEHOLDER.to_string(), format_delta);
                out.push(text_item(w, rect, text));
            }
            WidgetKind::Fuel => {
                out.push(bar_item(w, rect, fraction(t.fuel_litres, t.fuel_capacity_litres)));
                out.push(text_item(w, rect, format_fuel(t.fuel_litres)));
            }
            WidgetKind::TyreTemps => {
                out.push(text_item(w, rect, format_tyre_temps(t.tyre_temps_c)));
            }
            WidgetKind::Position => {
                out.push(text_item(w, rect, format_position(t.position)));
            }
            WidgetKind::CustomText => {
                out.push(text_item(w, rect, w.text.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "acxs_hud_{tag}_{}_{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn single(kind: WidgetKind, anchor: Anchor, x: f32, y: f32, w: f32, h: f32) -> Layout {
        Layout {
            speed_unit: SpeedUnit::Kmh,
            widgets: vec![Widget::new("w", kind, anchor, x, y, w, h)],
        }
    }

    #[test]
    fn colour_parses_six_and_eight_digit_hex() {
        assert_eq!(
            Color::parse("#10ff80").unwrap(),
            Color { r: 16, g: 255, b: 128, a: 255 }
        );
        assert_eq!(Color::parse("#10ff8040").unwrap().a, 0x40);
        assert!(Color::parse("10ff80").is_err());
        assert!(Color::parse("#10ff8").is_err());
        assert!(Color::parse("#zz0000").is_err());
    }

    #[test]
    fn colour_round_trips_as_hex_string() {
        let json = serde_json::to_string(&Color::rgb(1, 2, 3)).unwrap();
        assert_eq!(json, "\"#010203\"");
        let back: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Color::rgb(1, 2, 3));
        assert!(serde_json::from_str::<Color>("\"#12\"").is_err());
    }

    #[test]
    fn gear_names() {
        assert_eq!(format_gear(-1), "R");
        assert_eq!(format_gear(0), "N");
        assert_eq!(format_gear(1), "1");
        assert_eq!(format_gear(8), "8");
        assert_eq!(format_gear(-5), "?");
    }

    #[test]
    fn lap_time_is_m_ss_mmm() {
        assert_eq!(format_lap_time(0), "0:00.000");
        assert_eq!(format_lap_time(83_456), "1:23.456");
        assert_eq!(format_lap_time(3_600_005), "60:00.005");
    }

    #[test]
    fn delta_carries_sign() {
        assert_eq!(format_delta(1_234), "+1.234");
        assert_eq!(format_delta(-56), "-0.056");
        assert_eq!(format_delta(0), "+0.000");
        assert_eq!(format_delta(-90_500), "-90.500");
        assert_eq!(format_delta(i32::MIN), "-2147483.648");
    }

    #[test]
    fn speed_in_km_h_and_mph() {
        assert_eq!(format_speed(100.0, SpeedUnit::Kmh), "100 km/h");
        assert_eq!(format_speed(100.0, SpeedUnit::Mph), "62 mph");
        assert_eq!(format_speed(-3.0, SpeedUnit::Kmh), "0 km/h");
        assert_eq!(format_speed(f32::NAN, SpeedUnit::Kmh), "--- km/h");
    }

    #[test]
    fn fuel_tyre_and_position_text() {
        assert_eq!(format_fuel(42.46), "42.5 L");
        assert_eq!(format_fuel(f32::NAN), "--.- L");
        assert_eq!(
            format_tyre_temps([85.4, 84.6, 82.0, f32::NAN]),
            "FL 85 FR 85 RL 82 RR --"
        );
        assert_eq!(format_position(Some(3)), "P3");
        assert_eq!(format_position(None), "P--");
    }

    #[test]
    fn anchor_selects_the_point_placed_at_xy() {
        let centred = Widget::new("a", WidgetKind::Speed, Anchor::Center, 0.5, 0.5, 0.2, 0.1).rect();
        assert!(approx(centred.x, 0.4) && approx(centred.y, 0.45));
        assert!(approx(centred.w, 0.2) && approx(centred.h, 0.1));

        let top_right = Widget::new("b", WidgetKind::Speed, Anchor::TopRight, 0.98, 0.03, 0.08, 0.05).rect();
        assert!(approx(top_right.x, 0.90) && approx(top_right.y, 0.03));
    }

    #[test]
    fn default_layout_is_valid() {
        assert!(Layout::default_layout().validate().is_ok());
    }

    #[test]
    fn overlapping_widgets_are_reported() {
        let layout = Layout {
            speed_unit: SpeedUnit::Kmh,
            widgets: vec![
                Widget::new("a", WidgetKind::Speed, Anchor::TopLeft, 0.1, 0.1, 0.2, 0.2),
                Widget::new("b", WidgetKind::Gear, Anchor::TopLeft, 0.2, 0.2, 0.2, 0.2),
            ],
        };
        let errors = layout.validate().unwrap_err();
        assert!(errors.contains(&ValidationError::Overlap {
            first: "a".into(),
            second: "b".into()
        }));
    }

    #[test]
    fn touching_widgets_do_not_overlap() {
        let layout = Layout {
            speed_unit: SpeedUnit::Kmh,
            widgets: vec![
                Widget::new("a", WidgetKind::Speed, Anchor::TopLeft, 0.0, 0.0, 0.5, 0.5),
                Widget::new("b", WidgetKind::Gear, Anchor::TopLeft, 0.5, 0.0, 0.5, 0.5),
            ],
        };
        assert!(layout.validate().is_ok());
    }

    #[test]
    fn position_out_of_range_is_reported() {
        let layout = single(WidgetKind::Speed, Anchor::TopLeft, 1.5, 0.1, 0.1, 0.1);
        let errors = layout.validate().unwrap_err();
        assert!(matches!(errors[0], ValidationError::PositionOutOfRange { .. }));
    }

    #[test]
    fn widget_past_the_edge_is_reported() {
        let layout = single(WidgetKind::Speed, Anchor::TopLeft, 0.95, 0.1, 0.2, 0.1);
        let errors = layout.validate().unwrap_err();
        assert_eq!(errors, vec![ValidationError::OutsideScreen { widget: "w".into() }]);
    }

    #[test]
    fn invalid_size_is_reported() {
        let zero = single(WidgetKind::Speed, Anchor::TopLeft, 0.1, 0.1, 0.0, 0.1);
        assert!(matches!(zero.validate().unwrap_err()[0], ValidationError::InvalidSize { .. }));
        let big = single(WidgetKind::Speed, Anchor::TopLeft, 0.1, 0.1, 1.5, 0.1);
        assert!(matches!(big.validate().unwrap_err()[0], ValidationError::InvalidSize { .. }));
    }

    #[test]
    fn unknown_source_is_reported() {
        let mut layout = single(WidgetKind::Speed, Anchor::TopLeft, 0.1, 0.1, 0.1, 0.1);
        layout.widgets[0].source = "nope".into();
        let errors = layout.validate().unwrap_err();
        assert_eq!(
            errors,
            vec![ValidationError::UnknownSource {
                widget: "w".into(),
                source: "nope".into()
            }]
        );
    }

    #[test]
    fn source_that_does_not_suit_kind_is_reported() {
        let mut layout = single(WidgetKind::Speed, Anchor::TopLeft, 0.1, 0.1, 0.1, 0.1);
        layout.widgets[0].source = SOURCE_RPM.into();
        let errors = layout.validate().unwrap_err();
        assert!(matches!(errors[0], ValidationError::SourceMismatch { .. }));
    }

    #[test]
    fn duplicate_and_empty_ids_are_reported() {
        let layout = Layout {
            speed_unit: SpeedUnit::Kmh,
            widgets: vec![
                Widget::new("a", WidgetKind::Speed, Anchor::TopLeft, 0.1, 0.1, 0.1, 0.1),
                Widget::new("a", WidgetKind::Gear, Anchor::TopLeft, 0.5, 0.5, 0.1, 0.1),
                Widget::new("", WidgetKind::Fuel, Anchor::TopLeft, 0.8, 0.8, 0.1, 0.1),
            ],
        };
        let errors = layout.validate().unwrap_err();
        assert!(errors.contains(&ValidationError::DuplicateId("a".into())));
        assert!(errors.contains(&ValidationError::EmptyId));
    }

    #[test]
    fn json_round_trip_preserves_layout() {
        let layout = Layout::default_layout();
        let json = layout.to_json().unwrap();
        assert_eq!(Layout::from_json(&json).unwrap(), layout);
    }

    #[test]
    fn from_json_rejects_invalid_layout() {
        let mut layout = Layout::default_layout();
        let copy = layout.widgets[0].clone();
        layout.widgets.push(copy);
        let json = layout.to_json().unwrap();
        assert!(matches!(Layout::from_json(&json), Err(HudError::Invalid(_))));
    }

    #[test]
    fn json_defaults_fill_optional_fields() {
        let json = r#"{"widgets":[{"id":"s","kind":"speed","x":0.1,"y":0.1,
            "width":0.1,"height":0.1,"source":"speed"}]}"#;
        let layout = Layout::from_json(json).unwrap();
        assert_eq!(layout.speed_unit, SpeedUnit::Kmh);
        assert_eq!(layout.widgets[0].anchor, Anchor::TopLeft);
        assert_eq!(layout.widgets[0].color, default_color());
        assert_eq!(layout.widgets[0].background, None);
    }

    #[test]
    fn from_json_rejects_unknown_source() {
        let json = r#"{"widgets":[{"id":"s","kind":"speed","x":0.1,"y":0.1,
            "width":0.1,"height":0.1,"source":"bogus"}]}"#;
        assert!(matches!(Layout::from_json(json), Err(HudError::Invalid(_))));
    }

    #[test]
    fn rpm_bar_fill_is_ratio_clamped_to_one() {
        let layout = single(WidgetKind::RpmBar, Anchor::TopLeft, 0.1, 0.1, 0.3, 0.02);
        let t = Telemetry { rpm: 6_000.0, max_rpm: 8_000.0, ..Default::default() };
        match &draw_items(&layout, &t)[..] {
            [DrawItem::Bar { fill, .. }] => assert!(approx(*fill, 0.75)),
            other => panic!("unexpected items: {other:?}"),
        }
        let over = Telemetry { rpm: 9_000.0, max_rpm: 8_000.0, ..Default::default() };
        match &draw_items(&layout, &over)[..] {
            [DrawItem::Bar { fill, .. }] => assert!(approx(*fill, 1.0)),
            other => panic!("unexpected items: {other:?}"),
        }
        let no_max = Telemetry { rpm: 6_000.0, max_rpm: 0.0, ..Default::default() };
        match &draw_items(&layout, &no_max)[..] {
            [DrawItem::Bar { fill, .. }] => assert_eq!(*fill, 0.0),
            other => panic!("unexpected items: {other:?}"),
        }
    }

    #[test]
    fn shift_lights_follow_rpm() {
        let layout = single(WidgetKind::ShiftLights, Anchor::TopLeft, 0.1, 0.1, 0.4, 0.03);
        let lit_count = |rpm: f32| {
            let t = Telemetry { rpm, max_rpm: 1.0, ..Default::default() };
            let items = draw_items(&layout, &t);
            assert_eq!(items.len(), SHIFT_LIGHT_COUNT);
            items
                .iter()
                .filter(|i| matches!(i, DrawItem::Bar { fill, .. } if *fill == 1.0))
                .count()
        };
        assert_eq!(lit_count(0.0), 0);
        assert_eq!(lit_count(0.9), 4);
        assert_eq!(lit_count(1.0), SHIFT_LIGHT_COUNT);
    }

    #[test]
    fn missing_lap_and_delta_show_placeholders() {
        let layout = Layout {
            speed_unit: SpeedUnit::Kmh,
            widgets: vec![
                Widget::new("lap", WidgetKind::LapTime, Anchor::TopLeft, 0.1, 0.1, 0.2, 0.05),
                Widget::new("delta", WidgetKind::Delta, Anchor::TopLeft, 0.1, 0.2, 0.2, 0.05),
            ],
        };
        let items = draw_items(&layout, &Telemetry::default());
        assert!(matches!(&items[0], DrawItem::Text { text, .. } if text == "--:--.---"));
        assert!(matches!(&items[1], DrawItem::Text { text, .. } if text == "--.---"));
    }

    #[test]
    fn last_lap_source_reads_last_lap() {
        let mut layout = single(WidgetKind::LapTime, Anchor::TopLeft, 0.1, 0.1, 0.2, 0.05);
        layout.widgets[0].source = SOURCE_LAST_LAP.into();
        let t = Telemetry {
            lap_time_ms: Some(5_000),
            last_lap_ms: Some(61_000),
            ..Default::default()
        };
        assert!(matches!(&draw_items(&layout, &t)[0], DrawItem::Text { text, .. } if text == "1:01.000"));
    }

    #[test]
    fn fuel_draws_bar_then_text() {
        let layout = single(WidgetKind::Fuel, Anchor::TopLeft, 0.1, 0.1, 0.2, 0.05);
        let t = Telemetry {
            fuel_litres: 30.0,
            fuel_capacity_litres: 60.0,
            ..Default::default()
        };
        let items = draw_items(&layout, &t);
        assert!(matches!(&items[0], DrawItem::Bar { fill, .. } if approx(*fill, 0.5)));
        assert!(matches!(&items[1], DrawItem::Text { text, .. } if text == "30.0 L"));
    }

    #[test]
    fn gear_and_custom_text_draw_their_values() {
        let mut layout = Layout {
            speed_unit: SpeedUnit::Kmh,
            widgets: vec![
                Widget::new("gear", WidgetKind::Gear, Anchor::TopLeft, 0.1, 0.1, 0.1, 0.1),
                Widget::new("note", WidgetKind::CustomText, Anchor::TopLeft, 0.1, 0.3, 0.1, 0.1),
            ],
        };
        layout.widgets[1].text = "ACXS".into();
        let items = draw_items(&layout, &Telemetry::default());
        assert!(matches!(&items[0], DrawItem::Text { text, .. } if text == "N"));
        assert!(matches!(&items[1], DrawItem::Text { text, .. } if text == "ACXS"));
    }

    #[test]
    fn mismatched_source_widgets_are_skipped_in_drawing() {
        let mut layout = single(WidgetKind::Speed, Anchor::TopLeft, 0.1, 0.1, 0.1, 0.1);
        layout.widgets[0].source = SOURCE_RPM.into();
        assert!(draw_items(&layout, &Telemetry::default()).is_empty());
    }

    #[test]
    fn backup_names_are_timestamped_and_never_reuse_a_name() {
        let path = Path::new("/x/hud.json");
        let first = backup_candidate(path, "20261015-120000123", 0);
        assert_eq!(first.file_name().unwrap(), "hud.20261015-120000123.bak.json");

        let dir = temp_dir("names");
        let json = dir.join("hud.json");
        let taken = backup_candidate(&json, "20261015-120000123", 0);
        fs::write(&taken, b"old").unwrap();
        let next = next_backup_path(&json, "20261015-120000123");
        assert_eq!(next.file_name().unwrap(), "hud.20261015-120000123-1.bak.json");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_backs_up_previous_file_and_keeps_it() {
        let dir = temp_dir("save");
        let path = dir.join("hud.json");

        let first = Layout::default_layout();
        assert_eq!(first.save(&path).unwrap(), None);

        let mut second = Layout::default_layout();
        second.speed_unit = SpeedUnit::Mph;
        let backup = second.save(&path).unwrap().expect("a backup is made");
        assert_eq!(backup.parent(), Some(dir.as_path()));
        let name = backup.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("hud.") && name.ends_with(".bak.json"), "{name}");
        assert_eq!(fs::read_to_string(&backup).unwrap(), format!("{}\n", first.to_json().unwrap()));
        assert_eq!(Layout::load(&path).unwrap(), second);

        // Identical content: nothing written, no new backup.
        assert_eq!(second.save(&path).unwrap(), None);

        let mut third = second.clone();
        third.speed_unit = SpeedUnit::Kmh;
        let backup2 = third.save(&path).unwrap().expect("a second backup is made");
        assert_ne!(backup2, backup);
        assert!(backup.exists() && backup2.exists());
        assert_eq!(Layout::load(&path).unwrap(), third);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_refuses_invalid_layout() {
        let dir = temp_dir("invalid");
        let path = dir.join("hud.json");
        let mut layout = Layout::default_layout();
        let copy = layout.widgets[0].clone();
        layout.widgets.push(copy);
        assert!(matches!(layout.save(&path), Err(HudError::Invalid(_))));
        assert!(!path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn load_missing_file_is_io_error() {
        let dir = temp_dir("missing");
        let result = Layout::load(&dir.join("nope.json"));
        assert!(matches!(result, Err(HudError::Io(_))));
        fs::remove_dir_all(&dir).unwrap();
    }
}
