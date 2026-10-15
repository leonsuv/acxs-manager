//! Licence plate core: a style model, text validation, glyph layout and a pure
//! RGBA rasteriser. Nothing here touches the filesystem or an image encoder.
//!
//! Plate text is a list of fields separated by single spaces. Each style
//! declares its fields, and every field states which characters it accepts
//! and how many of them it takes. Text is drawn with a small built-in 5x7
//! bitmap font that covers A-Z, 0-9 and space.

use std::fmt;

/// Largest integer scale accepted by [`render_rgba`].
pub const MAX_SCALE: u32 = 16;

/// Largest RGBA buffer, in bytes, that [`render_rgba`] will allocate.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Glyph height in font cells.
const GLYPH_ROWS: u32 = 7;
/// Glyph width in font cells.
const GLYPH_COLS: u32 = 5;

/// Colour as `[red, green, blue, alpha]`.
pub type Rgba = [u8; 4];

/// Everything that can go wrong while validating, laying out or drawing a plate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlateError {
    /// The style itself is malformed.
    InvalidStyle(&'static str),
    /// The text has a different number of space-separated fields than the style.
    FieldCount { expected: usize, found: usize },
    /// A field has fewer or more characters than its range allows.
    Length { field: usize, len: usize, min: usize, max: usize },
    /// A character is outside the field's allowed set, or is forbidden in it.
    NotAllowed { field: usize, ch: char },
    /// The character is allowed by the style but the built-in font has no glyph for it.
    UnsupportedChar(char),
    /// The content does not fit inside the plate.
    DoesNotFit { needed: u32, available: u32 },
    /// The render scale is outside `1..=MAX_SCALE`.
    InvalidScale(u32),
    /// The output image would exceed [`MAX_BYTES`].
    TooLarge { width: u64, height: u64 },
}

impl fmt::Display for PlateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlateError::InvalidStyle(why) => write!(f, "invalid plate style: {why}"),
            PlateError::FieldCount { expected, found } => {
                write!(f, "expected {expected} space-separated fields, found {found}")
            }
            PlateError::Length { field, len, min, max } => write!(
                f,
                "field {} has {len} characters, it needs between {min} and {max}",
                field + 1
            ),
            PlateError::NotAllowed { field, ch } => {
                write!(f, "character {ch:?} is not allowed in field {}", field + 1)
            }
            PlateError::UnsupportedChar(ch) => {
                write!(f, "character {ch:?} has no glyph in the built-in font")
            }
            PlateError::DoesNotFit { needed, available } => {
                write!(f, "content needs {needed} px but only {available} px are available")
            }
            PlateError::InvalidScale(scale) => {
                write!(f, "scale {scale} is outside 1..={MAX_SCALE}")
            }
            PlateError::TooLarge { width, height } => {
                write!(f, "{width}x{height} image is too large to render")
            }
        }
    }
}

impl std::error::Error for PlateError {}

/// Which characters a field accepts. Text is upper-cased before it is checked,
/// so the letter and digit rules only deal with `A-Z` and `0-9`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharRule {
    Letters,
    Digits,
    Alphanumeric,
    /// Exactly the characters in the given set (compared after upper-casing).
    Custom(String),
}

impl CharRule {
    fn accepts(&self, c: char) -> bool {
        match self {
            CharRule::Letters => c.is_ascii_uppercase(),
            CharRule::Digits => c.is_ascii_digit(),
            CharRule::Alphanumeric => c.is_ascii_uppercase() || c.is_ascii_digit(),
            CharRule::Custom(set) => set.to_ascii_uppercase().contains(c),
        }
    }
}

/// One space-separated part of the plate text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub rule: CharRule,
    /// Characters removed from the rule's set, upper-cased.
    pub forbidden: String,
    pub min_len: usize,
    pub max_len: usize,
}

impl Field {
    pub fn new(rule: CharRule, min_len: usize, max_len: usize) -> Self {
        Field { rule, forbidden: String::new(), min_len, max_len }
    }

    /// Forbids the given characters even if the rule would accept them.
    pub fn forbidding(mut self, chars: &str) -> Self {
        self.forbidden = chars.to_ascii_uppercase();
        self
    }

    /// Whether this field accepts `c` (already upper-cased).
    pub fn accepts(&self, c: char) -> bool {
        self.rule.accepts(c) && !self.forbidden.contains(c)
    }
}

/// Visual and structural description of one plate design.
///
/// All measurements are in plate pixels at scale 1. Rendering multiplies them
/// by the requested scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlateStyle {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub background: Rgba,
    pub text_color: Rgba,
    pub border_width: u32,
    pub border_color: Rgba,
    /// Target character height. The font stroke is `char_height / 7`.
    pub char_height: u32,
    /// Gap between two characters of the same field.
    pub char_spacing: u32,
    /// Gap that replaces a space between two fields.
    pub group_gap: u32,
    pub fields: Vec<Field>,
}

impl PlateStyle {
    /// Size of one font cell in plate pixels.
    pub fn stroke(&self) -> u32 {
        (self.char_height / GLYPH_ROWS).max(1)
    }

    /// Checks that the style describes a plate that can hold text.
    pub fn check(&self) -> Result<(), PlateError> {
        if self.width == 0 || self.height == 0 {
            return Err(PlateError::InvalidStyle("plate size must be non-zero"));
        }
        if self.fields.is_empty() {
            return Err(PlateError::InvalidStyle("style needs at least one field"));
        }
        if self.fields.iter().any(|f| f.max_len == 0 || f.min_len > f.max_len) {
            return Err(PlateError::InvalidStyle("field length range is empty"));
        }
        if self.char_height < GLYPH_ROWS {
            return Err(PlateError::InvalidStyle("character height must be at least 7 px"));
        }
        let border2 = self.border_width.saturating_mul(2);
        if border2 >= self.width || border2 >= self.height {
            return Err(PlateError::InvalidStyle("border leaves no room for text"));
        }
        let needed = GLYPH_ROWS * self.stroke() + border2;
        if needed > self.height {
            return Err(PlateError::DoesNotFit { needed, available: self.height });
        }
        Ok(())
    }

    /// Looks up a built-in style by name.
    pub fn builtin(name: &str) -> Option<PlateStyle> {
        builtin_styles().into_iter().find(|s| s.name == name)
    }
}

/// The styles that ship with the crate.
pub fn builtin_styles() -> Vec<PlateStyle> {
    vec![eu_strip(), night_black(), regional_blue(), compact_single()]
}

fn eu_strip() -> PlateStyle {
    PlateStyle {
        name: "eu-strip".into(),
        width: 520,
        height: 112,
        background: [245, 242, 230, 255],
        text_color: [30, 30, 30, 255],
        border_width: 5,
        border_color: [30, 30, 30, 255],
        char_height: 56,
        char_spacing: 10,
        group_gap: 30,
        fields: vec![
            Field::new(CharRule::Letters, 1, 2),
            Field::new(CharRule::Digits, 1, 4),
            Field::new(CharRule::Letters, 1, 2),
        ],
    }
}

fn night_black() -> PlateStyle {
    PlateStyle {
        name: "night-black".into(),
        width: 480,
        height: 120,
        background: [16, 16, 20, 255],
        text_color: [242, 242, 242, 255],
        border_width: 4,
        border_color: [242, 242, 242, 255],
        char_height: 72,
        char_spacing: 10,
        group_gap: 40,
        fields: vec![
            Field::new(CharRule::Alphanumeric, 2, 3).forbidding("IO"),
            Field::new(CharRule::Digits, 1, 4),
        ],
    }
}

fn regional_blue() -> PlateStyle {
    PlateStyle {
        name: "regional-blue".into(),
        width: 560,
        height: 136,
        background: [31, 78, 156, 255],
        text_color: [255, 255, 255, 255],
        border_width: 6,
        border_color: [255, 212, 0, 255],
        char_height: 72,
        char_spacing: 8,
        group_gap: 40,
        fields: vec![
            Field::new(CharRule::Digits, 2, 2),
            Field::new(CharRule::Letters, 1, 3).forbidding("IOQ"),
            Field::new(CharRule::Digits, 1, 3),
        ],
    }
}

fn compact_single() -> PlateStyle {
    PlateStyle {
        name: "compact-single".into(),
        width: 400,
        height: 90,
        background: [232, 232, 232, 255],
        text_color: [32, 32, 32, 255],
        border_width: 0,
        border_color: [0, 0, 0, 255],
        char_height: 60,
        char_spacing: 6,
        group_gap: 0,
        fields: vec![Field::new(CharRule::Alphanumeric, 4, 8)],
    }
}

/// Checks `text` against `style` and returns it normalised: trimmed and upper-cased.
pub fn validate(style: &PlateStyle, text: &str) -> Result<String, PlateError> {
    style.check()?;
    let normalised = text.trim().to_ascii_uppercase();
    let parts: Vec<&str> = normalised.split(' ').collect();
    if parts.len() != style.fields.len() {
        return Err(PlateError::FieldCount { expected: style.fields.len(), found: parts.len() });
    }
    for (index, (field, part)) in style.fields.iter().zip(&parts).enumerate() {
        let len = part.chars().count();
        if len < field.min_len || len > field.max_len {
            return Err(PlateError::Length {
                field: index,
                len,
                min: field.min_len,
                max: field.max_len,
            });
        }
        for ch in part.chars() {
            if !field.accepts(ch) {
                return Err(PlateError::NotAllowed { field: index, ch });
            }
            if font::glyph(ch).is_none() {
                return Err(PlateError::UnsupportedChar(ch));
            }
        }
    }
    Ok(normalised)
}

/// One character placed on the plate, in plate pixels at scale 1.
/// Spaces get a zero-sized box; they only mark the gap between fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphBox {
    pub ch: char,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Placement of all characters on a plate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlateLayout {
    pub width: u32,
    pub height: u32,
    /// Font cell size in plate pixels.
    pub stroke: u32,
    pub glyphs: Vec<GlyphBox>,
}

/// Places every character of `text` on the plate, centred horizontally and
/// vertically inside the border. Validates the text first.
pub fn layout(style: &PlateStyle, text: &str) -> Result<PlateLayout, PlateError> {
    let normalised = validate(style, text)?;
    let stroke = style.stroke();
    let glyph_w = GLYPH_COLS * stroke;
    let glyph_h = GLYPH_ROWS * stroke;
    let chars: Vec<char> = normalised.chars().collect();

    // Offset of each character from the start of the content.
    let mut offsets = Vec::with_capacity(chars.len());
    let mut cursor = 0u32;
    for (i, &ch) in chars.iter().enumerate() {
        offsets.push(cursor);
        if ch == ' ' {
            cursor += style.group_gap;
            continue;
        }
        cursor += glyph_w;
        if chars.get(i + 1).is_some_and(|&next| next != ' ') {
            cursor += style.char_spacing;
        }
    }
    let content_w = cursor;

    let border = style.border_width;
    let inner_w = style.width - 2 * border;
    let inner_h = style.height - 2 * border;
    if content_w > inner_w {
        return Err(PlateError::DoesNotFit {
            needed: content_w + 2 * border,
            available: style.width,
        });
    }
    let origin_x = border + (inner_w - content_w) / 2;
    let origin_y = border + (inner_h - glyph_h) / 2;

    let glyphs = chars
        .iter()
        .zip(offsets)
        .map(|(&ch, offset)| {
            let (width, height) = if ch == ' ' { (0, 0) } else { (glyph_w, glyph_h) };
            GlyphBox { ch, x: origin_x + offset, y: origin_y, width, height }
        })
        .collect();

    Ok(PlateLayout { width: style.width, height: style.height, stroke, glyphs })
}

/// Draws the plate for `text` into a tightly packed RGBA buffer.
///
/// Returns `(width, height, pixels)` where `pixels.len() == width * height * 4`.
/// Rows run top to bottom. The output is `scale` times the style size in each
/// direction.
pub fn render_rgba(
    style: &PlateStyle,
    text: &str,
    scale: u32,
) -> Result<(u32, u32, Vec<u8>), PlateError> {
    if scale == 0 || scale > MAX_SCALE {
        return Err(PlateError::InvalidScale(scale));
    }
    let plate = layout(style, text)?;

    let out_w = u64::from(style.width) * u64::from(scale);
    let out_h = u64::from(style.height) * u64::from(scale);
    let bytes = out_w
        .checked_mul(out_h)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&bytes| bytes <= MAX_BYTES)
        .ok_or(PlateError::TooLarge { width: out_w, height: out_h })?;
    // The byte cap keeps both dimensions well inside u32.
    let (w, h) = (out_w as u32, out_h as u32);

    let mut buf = vec![0u8; bytes as usize];
    let border = style.border_width * scale;
    fill_rect(&mut buf, w, h, (0, 0, w, h), style.border_color);
    fill_rect(&mut buf, w, h, (border, border, w - border, h - border), style.background);

    let cell = plate.stroke * scale;
    for glyph in &plate.glyphs {
        let Some(rows) = font::glyph(glyph.ch) else {
            continue;
        };
        for (row, line) in rows.iter().enumerate() {
            for (col, px) in line.bytes().enumerate() {
                if px != b'#' {
                    continue;
                }
                let x0 = (glyph.x + col as u32 * plate.stroke) * scale;
                let y0 = (glyph.y + row as u32 * plate.stroke) * scale;
                fill_rect(&mut buf, w, h, (x0, y0, x0 + cell, y0 + cell), style.text_color);
            }
        }
    }

    Ok((w, h, buf))
}

/// Fills `rect = (x0, y0, x1, y1)`, end-exclusive, clipped to the image.
fn fill_rect(buf: &mut [u8], w: u32, h: u32, rect: (u32, u32, u32, u32), color: Rgba) {
    let (x0, y0, x1, y1) = rect;
    let x1 = x1.min(w);
    let y1 = y1.min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y as usize * w as usize + x as usize) * 4;
            buf[i..i + 4].copy_from_slice(&color);
        }
    }
}

/// Built-in 5x7 font. Each glyph is seven rows of five cells, `#` is ink.
mod font {
    pub fn glyph(ch: char) -> Option<&'static [&'static str; 7]> {
        Some(match ch {
            'A' => &[".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
            'B' => &["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
            'C' => &[".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."],
            'D' => &["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
            'E' => &["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
            'F' => &["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
            'G' => &[".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"],
            'H' => &["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
            'I' => &[".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."],
            'J' => &["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."],
            'K' => &["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
            'L' => &["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
            'M' => &["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
            'N' => &["#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#"],
            'O' => &[".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
            'P' => &["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
            'Q' => &[".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
            'R' => &["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
            'S' => &[".####", "#....", "#....", ".###.", "....#", "....#", "####."],
            'T' => &["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
            'U' => &["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
            'V' => &["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
            'W' => &["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "#.#.#", ".#.#."],
            'X' => &["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
            'Y' => &["#...#", "#...#", "#...#", ".#.#.", "..#..", "..#..", "..#.."],
            'Z' => &["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"],
            '0' => &[".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
            '1' => &["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
            '2' => &[".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
            '3' => &["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
            '4' => &["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
            '5' => &["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
            '6' => &["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
            '7' => &["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
            '8' => &[".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
            '9' => &[".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
            ' ' => &[".....", ".....", ".....", ".....", ".....", ".....", "....."],
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: [(&str, &str); 4] = [
        ("eu-strip", "AB 1234 CD"),
        ("night-black", "K7M 4821"),
        ("regional-blue", "42 KLM 731"),
        ("compact-single", "AB12CD"),
    ];

    fn eu() -> PlateStyle {
        PlateStyle::builtin("eu-strip").unwrap()
    }

    fn count_pixels(w: u32, h: u32, buf: &[u8], color: Rgba) -> usize {
        assert_eq!(buf.len(), (w * h * 4) as usize);
        buf.chunks_exact(4).filter(|px| *px == color).count()
    }

    #[test]
    fn font_covers_required_characters() {
        for ch in ('A'..='Z').chain('0'..='9') {
            let rows = font::glyph(ch).unwrap_or_else(|| panic!("missing {ch}"));
            assert_eq!(rows.len(), 7);
            for row in rows.iter() {
                assert_eq!(row.len(), 5, "bad row width in {ch}");
            }
            assert!(rows.iter().any(|r| r.contains('#')), "{ch} is blank");
        }
        let space = font::glyph(' ').unwrap();
        assert!(space.iter().all(|r| !r.contains('#')));
        assert!(font::glyph('a').is_none());
        assert!(font::glyph('-').is_none());
    }

    #[test]
    fn validate_normalises_case_and_whitespace() {
        assert_eq!(validate(&eu(), "  ab 1234 cd ").unwrap(), "AB 1234 CD");
    }

    #[test]
    fn validate_rejects_wrong_field_count() {
        assert_eq!(
            validate(&eu(), "AB1234CD"),
            Err(PlateError::FieldCount { expected: 3, found: 1 })
        );
    }

    #[test]
    fn validate_rejects_bad_lengths() {
        assert_eq!(
            validate(&eu(), "ABC 1234 CD"),
            Err(PlateError::Length { field: 0, len: 3, min: 1, max: 2 })
        );
        assert_eq!(
            validate(&eu(), "AB 12345 CD"),
            Err(PlateError::Length { field: 1, len: 5, min: 1, max: 4 })
        );
        assert_eq!(
            validate(&eu(), "AB  CD"),
            Err(PlateError::Length { field: 1, len: 0, min: 1, max: 4 })
        );
    }

    #[test]
    fn validate_rejects_characters_outside_the_rule() {
        assert_eq!(
            validate(&eu(), "A1 1234 CD"),
            Err(PlateError::NotAllowed { field: 0, ch: '1' })
        );
    }

    #[test]
    fn validate_rejects_forbidden_characters() {
        let style = PlateStyle::builtin("night-black").unwrap();
        assert_eq!(
            validate(&style, "IO 1234"),
            Err(PlateError::NotAllowed { field: 0, ch: 'I' })
        );
        assert!(validate(&style, "AB 1234").is_ok());
    }

    #[test]
    fn validate_rejects_chars_missing_from_the_font() {
        let mut style = eu();
        style.fields[0].rule = CharRule::Custom("-#".into());
        assert_eq!(validate(&style, "-# 1234 CD"), Err(PlateError::UnsupportedChar('-')));
    }

    #[test]
    fn layout_places_one_box_per_character_inside_the_plate() {
        let style = eu();
        let plate = layout(&style, "AB 1234 CD").unwrap();
        assert_eq!(plate.glyphs.len(), 10);
        assert_eq!((plate.width, plate.height), (style.width, style.height));
        let visible: Vec<&GlyphBox> = plate.glyphs.iter().filter(|g| g.ch != ' ').collect();
        assert_eq!(visible.len(), 8);
        for pair in visible.windows(2) {
            assert!(pair[0].x + pair[0].width <= pair[1].x, "boxes overlap");
        }
        for g in &plate.glyphs {
            assert!(g.x + g.width <= plate.width);
            assert!(g.y + g.height <= plate.height);
        }
    }

    #[test]
    fn layout_uses_stroke_sized_glyph_boxes() {
        let style = eu();
        let plate = layout(&style, "AB 1234 CD").unwrap();
        assert_eq!(plate.stroke, style.char_height / 7);
        let a = &plate.glyphs[0];
        assert_eq!((a.width, a.height), (5 * plate.stroke, 7 * plate.stroke));
    }

    #[test]
    fn layout_uses_group_gap_between_fields_and_spacing_within_them() {
        let style = eu();
        let plate = layout(&style, "AB 1234 CD").unwrap();
        let b = &plate.glyphs[1];
        let one = &plate.glyphs[3];
        let first = &plate.glyphs[0];
        assert_eq!(one.x - (b.x + b.width), style.group_gap);
        assert_eq!(b.x - (first.x + first.width), style.char_spacing);
    }

    #[test]
    fn layout_is_centred_horizontally() {
        let style = eu();
        let plate = layout(&style, "AB 1234 CD").unwrap();
        let first = plate.glyphs.first().unwrap();
        let last = plate.glyphs.last().unwrap();
        let left = first.x;
        let right = plate.width - (last.x + last.width);
        assert!(left.abs_diff(right) <= 1, "left {left} right {right}");
    }

    #[test]
    fn layout_reports_text_that_is_too_wide() {
        let mut style = eu();
        style.width = 100;
        assert!(matches!(
            layout(&style, "AB 1234 CD"),
            Err(PlateError::DoesNotFit { .. })
        ));
    }

    #[test]
    fn check_rejects_malformed_styles() {
        let mut style = eu();
        style.width = 0;
        assert!(matches!(style.check(), Err(PlateError::InvalidStyle(_))));

        let mut style = eu();
        style.fields.clear();
        assert!(matches!(style.check(), Err(PlateError::InvalidStyle(_))));

        let mut style = eu();
        style.fields[0].min_len = 3;
        assert!(matches!(style.check(), Err(PlateError::InvalidStyle(_))));

        let mut style = eu();
        style.char_height = 6;
        assert!(matches!(style.check(), Err(PlateError::InvalidStyle(_))));

        let mut style = eu();
        style.border_width = 60;
        assert!(matches!(style.check(), Err(PlateError::InvalidStyle(_))));
    }

    #[test]
    fn builtin_styles_are_valid_and_distinct() {
        let styles = builtin_styles();
        assert!(styles.len() >= 3);
        for style in &styles {
            style.check().unwrap();
        }
        let mut names: Vec<&str> = styles.iter().map(|s| s.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), styles.len());
    }

    #[test]
    fn every_builtin_renders_its_sample() {
        for (name, sample) in SAMPLES {
            let style = PlateStyle::builtin(name).unwrap();
            let (w, h, buf) = render_rgba(&style, sample, 1).unwrap();
            assert_eq!((w, h), (style.width, style.height));
            assert_eq!(buf.len(), (w * h * 4) as usize);
        }
    }

    #[test]
    fn render_output_size_scales_with_scale() {
        let style = eu();
        for scale in [1, 3] {
            let (w, h, buf) = render_rgba(&style, "AB 1234 CD", scale).unwrap();
            assert_eq!(w, style.width * scale);
            assert_eq!(h, style.height * scale);
            assert_eq!(buf.len(), (w * h * 4) as usize);
        }
    }

    #[test]
    fn render_paints_border_background_and_text() {
        let style = eu();
        let (w, _h, buf) = render_rgba(&style, "AB 1234 CD", 1).unwrap();
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        };
        assert_eq!(px(0, 0), style.border_color);
        let b = style.border_width;
        assert_eq!(px(b, b), style.background);
        assert!(count_pixels(w, style.height, &buf, style.text_color) > 0);
    }

    #[test]
    fn render_ink_scales_with_square_of_scale() {
        let style = eu();
        let (w1, h1, b1) = render_rgba(&style, "AB 1234 CD", 1).unwrap();
        let (w2, h2, b2) = render_rgba(&style, "AB 1234 CD", 2).unwrap();
        let ink1 = count_pixels(w1, h1, &b1, style.text_color);
        let ink2 = count_pixels(w2, h2, &b2, style.text_color);
        assert_eq!(ink2, ink1 * 4);
    }

    #[test]
    fn render_is_deterministic() {
        let style = eu();
        let a = render_rgba(&style, "AB 1234 CD", 2).unwrap();
        let b = render_rgba(&style, "AB 1234 CD", 2).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn render_rejects_bad_scale() {
        assert_eq!(render_rgba(&eu(), "AB 1234 CD", 0), Err(PlateError::InvalidScale(0)));
        assert_eq!(
            render_rgba(&eu(), "AB 1234 CD", MAX_SCALE + 1),
            Err(PlateError::InvalidScale(MAX_SCALE + 1))
        );
    }

    #[test]
    fn render_propagates_validation_errors() {
        assert_eq!(
            render_rgba(&eu(), "AB", 1),
            Err(PlateError::FieldCount { expected: 3, found: 1 })
        );
    }

    #[test]
    fn render_rejects_oversized_output() {
        let mut style = eu();
        style.width = 20_000;
        style.height = 4_000;
        assert!(matches!(
            render_rgba(&style, "AB 1234 CD", MAX_SCALE),
            Err(PlateError::TooLarge { .. })
        ));
    }

    #[test]
    fn full_alphabet_and_digits_render_in_one_field() {
        let mut style = eu();
        style.width = 4_000;
        style.fields = vec![Field::new(CharRule::Alphanumeric, 1, 36)];
        let text = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        assert!(render_rgba(&style, text, 1).is_ok());
    }

    #[test]
    fn errors_have_readable_messages() {
        let errors = [
            PlateError::InvalidStyle("x"),
            PlateError::FieldCount { expected: 2, found: 1 },
            PlateError::Length { field: 0, len: 5, min: 1, max: 2 },
            PlateError::NotAllowed { field: 1, ch: 'Z' },
            PlateError::UnsupportedChar('-'),
            PlateError::DoesNotFit { needed: 9, available: 5 },
            PlateError::InvalidScale(0),
            PlateError::TooLarge { width: 1, height: 1 },
        ];
        for err in errors {
            assert!(!err.to_string().is_empty());
        }
    }
}
