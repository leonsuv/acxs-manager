//! Hue recolouring of livery (skin) textures.
//!
//! A pixel is split into luma and chroma, with luma taken from the Rec. 601
//! weights. The chroma vector of a pixel lies in the plane where those weights
//! sum to zero, so rotating it inside that plane changes the hue and keeps the
//! luma. The chroma length (saturation) is kept too. If a rotated channel would
//! leave the 0..=255 range, the chroma is scaled down, which keeps the luma
//! exact and only reduces saturation.

use std::error::Error;
use std::fmt;

use crate::png::{self, PngError};

/// Rec. 601 luma weights for red, green and blue. They sum to one.
const LUMA: [f32; 3] = [0.299, 0.587, 0.114];

/// Chroma length below which a colour counts as neutral and has no hue. Units are 0..=255 sample values.
const MIN_CHROMA: f32 = 0.5;

/// Errors returned by [`recolor_png`].
#[derive(Debug, Clone, PartialEq)]
pub enum LiveryError {
    /// The skin, the mask or the output could not be decoded or encoded.
    Png(PngError),
    /// The strength was not a number between 0 and 1.
    InvalidStrength(f32),
    /// The target colour is black, white or grey, so it has no hue to move towards.
    NeutralTarget,
    /// The mask has different dimensions from the skin.
    MaskSize { skin: (u32, u32), mask: (u32, u32) },
}

impl fmt::Display for LiveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiveryError::Png(err) => write!(f, "{err}"),
            LiveryError::InvalidStrength(s) => {
                write!(f, "strength must be between 0 and 1, got {s}")
            }
            LiveryError::NeutralTarget => {
                write!(f, "target colour has no hue (it is black, white or grey)")
            }
            LiveryError::MaskSize { skin, mask } => write!(
                f,
                "mask is {}x{} but the skin is {}x{}",
                mask.0, mask.1, skin.0, skin.1
            ),
        }
    }
}

impl Error for LiveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            LiveryError::Png(err) => Some(err),
            _ => None,
        }
    }
}

impl From<PngError> for LiveryError {
    fn from(err: PngError) -> Self {
        LiveryError::Png(err)
    }
}

/// Recolours a skin texture so that its hues move towards `target_rgb`.
///
/// `input` and `mask` are PNG files. The skin may be greyscale, grey with
/// alpha, RGB or RGBA with 8-bit samples. The result is always RGBA with the
/// same alpha as the input.
///
/// `strength` is between 0 and 1. At 1 each hue reaches the target hue. At 0
/// the skin is returned unchanged. Each pixel rotates along the shorter arc
/// towards the target. Luma never changes, and neither does the saturation,
/// except where a channel would otherwise leave the 0..=255 range. Pixels
/// with no hue, such as greys, are left alone.
///
/// If `mask` is given, its luma (0 for black, 1 for white) scales the strength
/// per pixel. It must have the same dimensions as the skin.
pub fn recolor_png(
    input: &[u8],
    target_rgb: [u8; 3],
    strength: f32,
    mask: Option<&[u8]>,
) -> Result<Vec<u8>, LiveryError> {
    if !(0.0..=1.0).contains(&strength) {
        return Err(LiveryError::InvalidStrength(strength));
    }

    let plane = Plane::new();
    let target = plane.coords(target_rgb.map(f32::from));
    if target.0.hypot(target.1) < MIN_CHROMA {
        return Err(LiveryError::NeutralTarget);
    }

    let (width, height, mut pixels) = png::decode_rgba(input)?;
    let weights = match mask {
        None => None,
        Some(bytes) => {
            let (mw, mh, mask_pixels) = png::decode_rgba(bytes)?;
            if (mw, mh) != (width, height) {
                return Err(LiveryError::MaskSize {
                    skin: (width, height),
                    mask: (mw, mh),
                });
            }
            let weights: Vec<f32> = mask_pixels
                .as_chunks::<4>().0.iter()
                .map(|p| dot(LUMA, [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]) / 255.0)
                .collect();
            Some(weights)
        }
    };

    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let weight = weights.as_ref().map_or(1.0, |w| w[index]);
        let amount = strength * weight;
        if amount > 0.0 {
            recolor_pixel(px, amount, &plane, target);
        }
    }

    Ok(png::encode_rgba(width, height, &pixels)?)
}

/// Orthonormal basis of the plane in which the luma weights sum to zero.
struct Plane {
    e1: [f32; 3],
    e2: [f32; 3],
}

impl Plane {
    fn new() -> Self {
        // Project (1, -1, 0) onto the plane orthogonal to LUMA. The result is in the plane.
        let raw = [1.0, -1.0, 0.0];
        let k = dot(raw, LUMA) / dot(LUMA, LUMA);
        let projected = [
            raw[0] - k * LUMA[0],
            raw[1] - k * LUMA[1],
            raw[2] - k * LUMA[2],
        ];
        let e1 = scale(projected, 1.0 / dot(projected, projected).sqrt());
        // The cross product of the unit normal with e1 is a unit vector in the plane, orthogonal to e1.
        let normal = scale(LUMA, 1.0 / dot(LUMA, LUMA).sqrt());
        let e2 = cross(normal, e1);
        Plane { e1, e2 }
    }

    /// Coordinates of the chroma of `rgb` in this basis.
    fn coords(&self, rgb: [f32; 3]) -> (f32, f32) {
        let chroma = chroma(rgb);
        (dot(chroma, self.e1), dot(chroma, self.e2))
    }
}

/// Chroma of an RGB colour: each channel minus the luma.
fn chroma(rgb: [f32; 3]) -> [f32; 3] {
    let y = dot(LUMA, rgb);
    rgb.map(|c| c - y)
}

/// Rotates one pixel's chroma by `strength` times the angle to the target, then restores the gamut.
fn recolor_pixel(px: &mut [u8], strength: f32, plane: &Plane, target: (f32, f32)) {
    let rgb = [f32::from(px[0]), f32::from(px[1]), f32::from(px[2])];
    let y = dot(LUMA, rgb);
    let (x, v) = plane.coords(rgb);
    if x.hypot(v) < MIN_CHROMA {
        return;
    }

    // Signed angle from the pixel's chroma to the target's chroma, in (-pi, pi].
    let (tx, ty) = target;
    let turn = strength * (x * ty - v * tx).atan2(x * tx + v * ty);
    let (sin, cos) = turn.sin_cos();
    let (nx, nv) = (x * cos - v * sin, x * sin + v * cos);
    let rotated: [f32; 3] = std::array::from_fn(|i| nx * plane.e1[i] + nv * plane.e2[i]);

    // Largest factor in [0, 1] that keeps every channel within 0..=255.
    let mut fit = 1.0f32;
    for &c in &rotated {
        if c > 0.0 {
            fit = fit.min((255.0 - y) / c);
        } else if c < 0.0 {
            fit = fit.min(-y / c);
        }
    }
    let fit = fit.max(0.0);

    for (channel, &c) in px[..3].iter_mut().zip(&rotated) {
        *channel = (y + fit * c).round().clamp(0.0, 255.0) as u8;
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    a.map(|c| c * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(width: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
        let height = pixels.len() as u32 / width;
        let flat: Vec<u8> = pixels.iter().flatten().copied().collect();
        png::encode_rgba(width, height, &flat).unwrap()
    }

    fn decode(bytes: &[u8]) -> Vec<[u8; 4]> {
        let (_, _, rgba) = png::decode_rgba(bytes).unwrap();
        rgba.as_chunks::<4>().0.iter()
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect()
    }

    fn rgb_f(p: [u8; 4]) -> [f32; 3] {
        [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]
    }

    fn cosine(a: [f32; 3], b: [f32; 3]) -> f32 {
        dot(a, b) / (dot(a, a).sqrt() * dot(b, b).sqrt())
    }

    /// Deterministic colours, including varied alpha.
    fn sample_colours(count: usize) -> Vec<[u8; 4]> {
        let mut state: u32 = 0x1234_5678;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        (0..count).map(|_| [next(), next(), next(), next()]).collect()
    }

    #[test]
    fn luma_and_alpha_are_preserved() {
        let mut pixels = vec![
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 128],
            [0, 255, 255, 0],
            [255, 0, 255, 255],
            [1, 2, 3, 4],
        ];
        pixels.extend(sample_colours(500));
        let input = encode(pixels.len() as u32, &pixels);

        for target in [[30, 90, 200], [255, 128, 0], [0, 200, 40]] {
            for strength in [0.3, 1.0] {
                let out = decode(&recolor_png(&input, target, strength, None).unwrap());
                assert_eq!(out.len(), pixels.len());
                for (before, after) in pixels.iter().zip(&out) {
                    let dy = (dot(LUMA, rgb_f(*before)) - dot(LUMA, rgb_f(*after))).abs();
                    assert!(dy <= 1.0, "luma moved by {dy} for {before:?} -> {after:?}");
                    assert_eq!(before[3], after[3], "alpha changed");
                }
            }
        }
    }

    #[test]
    fn full_strength_turns_hue_to_target() {
        let input = encode(1, &[[200, 40, 40, 255]]);
        let target = [40u8, 40, 200];
        let out = decode(&recolor_png(&input, target, 1.0, None).unwrap())[0];

        let target_chroma = chroma(target.map(f32::from));
        let cos = cosine(chroma(rgb_f(out)), target_chroma);
        assert!(cos > 0.995, "hue is {cos} of the way to the target");
    }

    #[test]
    fn hue_moves_monotonically_with_strength() {
        let input = encode(1, &[[200, 40, 40, 255]]);
        let target = [40u8, 40, 200];
        let target_chroma = chroma(target.map(f32::from));
        let mut last = f32::MIN;
        for step in 0..=4 {
            let strength = step as f32 / 4.0;
            let out = decode(&recolor_png(&input, target, strength, None).unwrap())[0];
            let cos = cosine(chroma(rgb_f(out)), target_chroma);
            assert!(cos >= last - 0.01, "strength {strength} moved away from the target");
            last = cos;
        }
    }

    #[test]
    fn zero_strength_is_identity() {
        let pixels = sample_colours(64);
        let input = encode(8, &pixels);
        let out = decode(&recolor_png(&input, [0, 0, 255], 0.0, None).unwrap());
        assert_eq!(out, pixels);
    }

    #[test]
    fn greys_are_left_alone() {
        let pixels = [[0, 0, 0, 255], [128, 128, 128, 255], [255, 255, 255, 255], [60, 60, 60, 9]];
        let input = encode(4, &pixels);
        let out = decode(&recolor_png(&input, [200, 30, 30], 1.0, None).unwrap());
        assert_eq!(out, pixels);
    }

    #[test]
    fn mask_limits_where_the_recolour_applies() {
        let pixels = [[200, 40, 40, 255], [200, 40, 40, 255], [200, 40, 40, 255]];
        let input = encode(3, &pixels);
        // Black, mid grey and white mask pixels.
        let mask = encode(3, &[[0, 0, 0, 255], [128, 128, 128, 255], [255, 255, 255, 255]]);
        let target = [40, 40, 200];

        let out = decode(&recolor_png(&input, target, 1.0, Some(&mask)).unwrap());
        let full = decode(&recolor_png(&input, target, 1.0, None).unwrap());
        assert_eq!(out[0], pixels[0], "black mask must leave the pixel unchanged");
        assert_eq!(out[2], full[2], "white mask must apply full strength");
        assert_ne!(out[1], pixels[1]);
        assert_ne!(out[1], full[1], "mid-grey mask must apply partial strength");
    }

    #[test]
    fn mask_must_match_skin_size() {
        let input = encode(2, &[[200, 40, 40, 255], [200, 40, 40, 255]]);
        let mask = encode(1, &[[255, 255, 255, 255]]);
        assert_eq!(
            recolor_png(&input, [40, 40, 200], 1.0, Some(&mask)),
            Err(LiveryError::MaskSize {
                skin: (2, 1),
                mask: (1, 1)
            })
        );
    }

    #[test]
    fn neutral_target_is_rejected() {
        let input = encode(1, &[[200, 40, 40, 255]]);
        for target in [[0, 0, 0], [255, 255, 255], [120, 120, 120]] {
            assert_eq!(
                recolor_png(&input, target, 1.0, None),
                Err(LiveryError::NeutralTarget)
            );
        }
    }

    #[test]
    fn invalid_strength_is_rejected() {
        let input = encode(1, &[[200, 40, 40, 255]]);
        for strength in [f32::NAN, -0.1, 1.5, f32::INFINITY] {
            assert!(matches!(
                recolor_png(&input, [0, 0, 255], strength, None),
                Err(LiveryError::InvalidStrength(_))
            ));
        }
    }

    #[test]
    fn invalid_png_is_reported() {
        let input = encode(1, &[[200, 40, 40, 255]]);
        assert_eq!(
            recolor_png(b"definitely not a png", [0, 0, 255], 1.0, None),
            Err(LiveryError::Png(PngError::InvalidSignature))
        );
        assert_eq!(
            recolor_png(&input, [0, 0, 255], 1.0, Some(b"definitely not a png")),
            Err(LiveryError::Png(PngError::InvalidSignature))
        );
    }

    #[test]
    fn output_keeps_dimensions() {
        let pixels: Vec<[u8; 4]> = sample_colours(3 * 2);
        let input = encode(3, &pixels);
        let (w, h, _) = png::decode_rgba(&recolor_png(&input, [10, 200, 90], 0.5, None).unwrap())
            .unwrap();
        assert_eq!((w, h), (3, 2));
    }
}
