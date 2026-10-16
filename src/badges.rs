//! Make badges. A badge is a rounded square filled with a colour derived from
//! the brand name, with the brand's initials drawn in a 5x7 bitmap font.
//!
//! Badges are generated entirely from the brand name. No brand artwork is used
//! or embedded. Badge files are only ever created, never overwritten.

use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::png::{self, PngError};

/// Name of the cache subfolder inside each car folder. Shared with the thumbnails.
const CACHE_DIR: &str = "acxs-thumbnails";
/// File name of the badge inside the cache folder.
const BADGE_FILE: &str = "badge.png";

/// Smallest badge edge in pixels. Below this the initials do not fit.
pub const MIN_SIZE: u32 = 16;
/// Largest badge edge in pixels.
pub const MAX_SIZE: u32 = 1024;

const GLYPH_H: u32 = 7;
/// Blank columns between two glyphs, in glyph cells.
const GLYPH_GAP: u32 = 1;
/// Glyph cells per character, including the gap after it.
const GLYPH_ADVANCE: u32 = 5 + GLYPH_GAP;

/// Badge colour: the hue comes from the brand hash, saturation and lightness are fixed
/// so that white initials read well on every badge.
const SATURATION: f32 = 0.55;
const LIGHTNESS: f32 = 0.40;
const TEXT_RGB: [u8; 3] = [255, 255, 255];

/// Why a badge could not be made.
#[derive(Debug)]
pub enum BadgeError {
    /// The brand name has no letters or digits to make initials from.
    EmptyBrand,
    /// The car id is empty or would leave the cars folder.
    InvalidCarId(String),
    /// The requested size is outside `MIN_SIZE..=MAX_SIZE`.
    InvalidSize { size: u32, min: u32, max: u32 },
    /// The car folder does not exist.
    CarNotFound(PathBuf),
    /// A badge file already exists at the target path. It is left unchanged.
    AlreadyExists(PathBuf),
    /// The pixels could not be encoded as PNG.
    Encode(PngError),
    /// Reading or writing the file failed.
    Io(io::Error),
}

impl fmt::Display for BadgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BadgeError::EmptyBrand => write!(f, "brand name has no letters or digits"),
            BadgeError::InvalidCarId(id) => write!(f, "invalid car id {id:?}"),
            BadgeError::InvalidSize { size, min, max } => {
                write!(f, "badge size {size} must be between {min} and {max}")
            }
            BadgeError::CarNotFound(path) => write!(f, "car folder not found: {}", path.display()),
            BadgeError::AlreadyExists(path) => write!(f, "badge already exists: {}", path.display()),
            BadgeError::Encode(e) => write!(f, "could not encode badge: {e}"),
            BadgeError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl Error for BadgeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            BadgeError::Encode(e) => Some(e),
            BadgeError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<PngError> for BadgeError {
    fn from(e: PngError) -> Self {
        BadgeError::Encode(e)
    }
}

impl From<io::Error> for BadgeError {
    fn from(e: io::Error) -> Self {
        BadgeError::Io(e)
    }
}

/// Returns the badge as `(width, height, rgba)`. The image is `size` by `size` pixels,
/// four bytes per pixel, top row first. Pixels outside the rounded corners are transparent.
pub fn badge_rgba(brand: &str, size: u32) -> Result<(u32, u32, Vec<u8>), BadgeError> {
    check_size(size)?;
    let letters = initials(brand)?;
    let fill = colour_for(brand);

    let mut rgba = vec![0u8; (size as usize) * (size as usize) * 4];
    let radius = size as f32 / 5.0;
    for y in 0..size {
        for x in 0..size {
            if inside_rounded_square(x, y, size, radius) {
                put(&mut rgba, size, x, y, [fill[0], fill[1], fill[2], 255]);
            }
        }
    }

    let chars: Vec<char> = letters.chars().collect();
    let scale = text_scale(size, chars.len());
    let text_w = (GLYPH_ADVANCE * chars.len() as u32 - GLYPH_GAP) * scale;
    let text_h = GLYPH_H * scale;
    let x0 = (size - text_w) / 2;
    let y0 = (size - text_h) / 2;
    for (i, &ch) in chars.iter().enumerate() {
        let rows = glyph(ch);
        let gx = x0 + i as u32 * GLYPH_ADVANCE * scale;
        for (row, bits) in rows.iter().enumerate() {
            for (col, bit) in bits.bytes().enumerate() {
                if bit != b'#' {
                    continue;
                }
                let px = gx + col as u32 * scale;
                let py = y0 + row as u32 * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        put(&mut rgba, size, px + dx, py + dy, [TEXT_RGB[0], TEXT_RGB[1], TEXT_RGB[2], 255]);
                    }
                }
            }
        }
    }

    Ok((size, size, rgba))
}

/// Returns the badge as a PNG file.
pub fn badge_png(brand: &str, size: u32) -> Result<Vec<u8>, BadgeError> {
    let (width, height, rgba) = badge_rgba(brand, size)?;
    Ok(png::encode_rgba(width, height, &rgba)?)
}

/// The badge file for a car: `<ac_root>/content/cars/<car_id>/acxs-thumbnails/badge.png`.
/// The path is not checked. [`write_badge`] rejects unsafe car ids.
pub fn badge_path(ac_root: &Path, car_id: &str) -> PathBuf {
    ac_root
        .join("content")
        .join("cars")
        .join(car_id)
        .join(CACHE_DIR)
        .join(BADGE_FILE)
}

/// Makes the badge for a car and writes it to [`badge_path`].
///
/// The file is created new. An existing badge is never overwritten and gives
/// [`BadgeError::AlreadyExists`]. Returns the path of the written file.
pub fn write_badge(ac_root: &Path, car_id: &str, brand: &str, size: u32) -> Result<PathBuf, BadgeError> {
    if !is_safe_id(car_id) {
        return Err(BadgeError::InvalidCarId(car_id.to_string()));
    }
    let car_dir = ac_root.join("content").join("cars").join(car_id);
    if !car_dir.is_dir() {
        return Err(BadgeError::CarNotFound(car_dir));
    }
    let png = badge_png(brand, size)?;

    let target = badge_path(ac_root, car_id);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    match OpenOptions::new().write(true).create_new(true).open(&target) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(&png) {
                drop(file);
                // Do not leave a truncated badge behind.
                let _ = fs::remove_file(&target);
                return Err(e.into());
            }
            Ok(target)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(BadgeError::AlreadyExists(target)),
        Err(e) => Err(e.into()),
    }
}

fn check_size(size: u32) -> Result<(), BadgeError> {
    if (MIN_SIZE..=MAX_SIZE).contains(&size) {
        Ok(())
    } else {
        Err(BadgeError::InvalidSize {
            size,
            min: MIN_SIZE,
            max: MAX_SIZE,
        })
    }
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', '\0', ':'])
}

/// Up to two initials: the first letter of the first two words, or the first two
/// letters of a single word. Characters with no glyph are drawn as `?`.
fn initials(brand: &str) -> Result<String, BadgeError> {
    let words: Vec<&str> = brand
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = String::new();
    match words.as_slice() {
        [] => return Err(BadgeError::EmptyBrand),
        [first, second, ..] => {
            out.extend(first.chars().next());
            out.extend(second.chars().next());
        }
        [only] => out.extend(only.chars().take(2)),
    }
    Ok(out.chars().map(glyph_char).collect())
}

fn glyph_char(c: char) -> char {
    let upper = c.to_ascii_uppercase();
    if upper.is_ascii_alphanumeric() { upper } else { '?' }
}

/// Fill colour from a hash of the brand name. The name is trimmed and lowercased first,
/// so the same brand always gets the same colour. FNV-1a is used because its output
/// does not change between Rust releases.
fn colour_for(brand: &str) -> [u8; 3] {
    let normalised = brand.trim().to_lowercase();
    let hue = (fnv1a(normalised.as_bytes()) % 360) as f32;
    hsl_to_rgb(hue, SATURATION, LIGHTNESS)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> [u8; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let m = lightness - chroma / 2.0;
    [r, g, b].map(|v| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Whether the pixel centre lies inside a square with rounded corners.
fn inside_rounded_square(x: u32, y: u32, size: u32, radius: f32) -> bool {
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let far = size as f32 - radius;
    let cx = px.clamp(radius, far);
    let cy = py.clamp(radius, far);
    let (dx, dy) = (px - cx, py - cy);
    dx * dx + dy * dy <= radius * radius
}

/// Largest whole-number scale at which the initials fit in about 60% of the width
/// and 40% of the height. Never below 1.
fn text_scale(size: u32, chars: usize) -> u32 {
    let cells_w = GLYPH_ADVANCE * chars as u32 - GLYPH_GAP;
    let by_width = (size * 3 / 5) / cells_w.max(1);
    let by_height = (size * 2 / 5) / GLYPH_H;
    by_width.min(by_height).max(1)
}

fn put(rgba: &mut [u8], size: u32, x: u32, y: u32, px: [u8; 4]) {
    let i = ((y * size + x) as usize) * 4;
    rgba[i..i + 4].copy_from_slice(&px);
}

/// 5x7 glyph rows, top first. `#` is an ink pixel and `.` is background.
/// Characters without a glyph are drawn as `?`.
fn glyph(c: char) -> [&'static str; 7] {
    match c {
        'A' => [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'B' => ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
        'C' => [".###.", "#...#", "#....", "#....", "#....", "#...#", ".###."],
        'D' => ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
        'E' => ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
        'F' => ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
        'G' => [".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"],
        'H' => ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'I' => [".###.", "..#..", "..#..", "..#..", "..#..", "..#..", ".###."],
        'J' => ["..###", "...#.", "...#.", "...#.", "...#.", "#..#.", ".##.."],
        'K' => ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
        'L' => ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
        'M' => ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
        'N' => ["#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#"],
        'O' => [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'P' => ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
        'Q' => [".###.", "#...#", "#...#", "#...#", "#.#.#", "#..#.", ".##.#"],
        'R' => ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
        'S' => [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
        'U' => ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'V' => ["#...#", "#...#", "#...#", "#...#", "#...#", ".#.#.", "..#.."],
        'W' => ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"],
        'X' => ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
        'Y' => ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
        'Z' => ["#####", "....#", "...#.", "..#..", ".#...", "#....", "#####"],
        '0' => [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
        '1' => ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
        '2' => [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
        '3' => ["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
        '4' => ["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
        '5' => ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
        '6' => ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
        '7' => ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
        '8' => [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
        '9' => [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
        _ => [".###.", "#...#", "....#", "...#.", "..#..", ".....", "..#.."],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    /// A scratch AC folder with one car, `abc`.
    fn scratch_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("acxs_badge_{}_{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("content").join("cars").join("abc")).expect("create car folder");
        root
    }

    /// Colour of a pixel on the top edge, away from the corners and the initials.
    fn background_pixel(brand: &str, size: u32) -> [u8; 4] {
        let (_, _, rgba) = badge_rgba(brand, size).expect("badge");
        let i = ((2 * size + size / 2) as usize) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn same_brand_gives_same_badge() {
        assert_eq!(badge_rgba("Ferrari", 64).unwrap(), badge_rgba("Ferrari", 64).unwrap());
        assert_eq!(badge_png("Ferrari", 64).unwrap(), badge_png("Ferrari", 64).unwrap());
    }

    #[test]
    fn different_brands_get_different_colours() {
        let brands = ["Ferrari", "Lotus", "Porsche", "Alfa Romeo", "BMW", "Ford"];
        let colours: Vec<[u8; 3]> = brands.iter().map(|b| colour_for(b)).collect();
        for i in 0..colours.len() {
            for j in i + 1..colours.len() {
                assert_ne!(colours[i], colours[j], "{} and {}", brands[i], brands[j]);
            }
        }
    }

    #[test]
    fn fill_colour_matches_colour_for() {
        for brand in ["Ferrari", "Lotus"] {
            let px = background_pixel(brand, 64);
            let fill = colour_for(brand);
            assert_eq!(&px[..3], &fill[..]);
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn corners_are_transparent_and_centre_is_opaque() {
        let (w, h, rgba) = badge_rgba("Ferrari", 64).unwrap();
        assert_eq!((w, h), (64, 64));
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert_eq!(rgba[3], 0, "top-left corner pixel");
        let centre = ((32 * 64 + 32) as usize) * 4;
        assert_eq!(rgba[centre + 3], 255);
    }

    #[test]
    fn empty_brand_is_rejected() {
        for brand in ["", "   ", "!!!", " - "] {
            assert!(matches!(badge_rgba(brand, 64), Err(BadgeError::EmptyBrand)), "{brand:?}");
            assert!(matches!(badge_png(brand, 64), Err(BadgeError::EmptyBrand)), "{brand:?}");
        }
    }

    #[test]
    fn initials_come_from_the_name() {
        assert_eq!(initials("Alfa Romeo").unwrap(), "AR");
        assert_eq!(initials("BMW").unwrap(), "BM");
        assert_eq!(initials("  ferrari ").unwrap(), "FE");
        assert_eq!(initials("Ford-GT").unwrap(), "FG");
        assert_eq!(initials("Ü7").unwrap(), "?7");
    }

    #[test]
    fn size_caps_are_enforced() {
        for size in [0, 1, MIN_SIZE - 1, MAX_SIZE + 1, u32::MAX] {
            assert!(
                matches!(badge_rgba("Ferrari", size), Err(BadgeError::InvalidSize { .. })),
                "size {size}"
            );
        }
        assert!(badge_rgba("Ferrari", MIN_SIZE).is_ok());
        let (w, h, rgba) = badge_rgba("Ferrari", MAX_SIZE).unwrap();
        assert_eq!((w, h), (MAX_SIZE, MAX_SIZE));
        assert_eq!(rgba.len(), (MAX_SIZE * MAX_SIZE * 4) as usize);
    }

    #[test]
    fn every_supported_character_has_a_glyph() {
        for c in ('A'..='Z').chain('0'..='9') {
            let rows = glyph(c);
            assert_eq!(rows.len(), 7, "{c}");
            for row in rows {
                assert_eq!(row.len(), 5, "{c}");
                assert!(row.bytes().all(|b| b == b'#' || b == b'.'), "{c}");
            }
            assert!(rows.iter().any(|r| r.contains('#')), "{c} has no ink");
        }
    }

    #[test]
    fn png_has_signature() {
        let png = badge_png("Lotus", 32).unwrap();
        assert!(png.starts_with(&PNG_SIGNATURE));
    }

    #[test]
    fn badge_path_is_in_car_thumbnail_folder() {
        let expected = PathBuf::from("/ac")
            .join("content")
            .join("cars")
            .join("abc")
            .join("acxs-thumbnails")
            .join("badge.png");
        assert_eq!(badge_path(Path::new("/ac"), "abc"), expected);
    }

    #[test]
    fn write_badge_creates_file_and_never_overwrites() {
        let root = scratch_root("write");
        let path = write_badge(&root, "abc", "Ferrari", 64).expect("first write");
        assert_eq!(path, badge_path(&root, "abc"));
        let first = fs::read(&path).expect("read badge");
        assert_eq!(first, badge_png("Ferrari", 64).unwrap());

        match write_badge(&root, "abc", "Lotus", 128) {
            Err(BadgeError::AlreadyExists(p)) => assert_eq!(p, path),
            other => panic!("expected AlreadyExists, got {other:?}"),
        }
        assert_eq!(fs::read(&path).expect("read badge"), first);
    }

    #[test]
    fn write_badge_checks_car_and_inputs() {
        let root = scratch_root("checks");
        assert!(matches!(
            write_badge(&root, "missing", "Ferrari", 64),
            Err(BadgeError::CarNotFound(_))
        ));
        assert!(matches!(
            write_badge(&root, "../abc", "Ferrari", 64),
            Err(BadgeError::InvalidCarId(_))
        ));
        assert!(matches!(
            write_badge(&root, "abc", "  ", 64),
            Err(BadgeError::EmptyBrand)
        ));
        assert!(!badge_path(&root, "abc").exists());
    }
}
