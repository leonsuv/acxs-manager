//! Orthographic renderer for flat-shaded triangle meshes.
//!
//! The mesh is rotated by a yaw (about the vertical axis) followed by a pitch
//! (about the horizontal axis), then projected along the view direction with
//! no perspective. The projection is scaled uniformly to fit the image inside
//! the padding, and rasterised with a depth buffer. Each triangle gets a single
//! colour from its face normal, so the result is a simple shaded silhouette.

use crate::png::{self, PngError};
use std::error::Error;
use std::fmt;

/// Largest accepted image width or height, in pixels.
pub const MAX_DIMENSION: u32 = 4096;

/// Largest number of triangles accepted by one render call.
pub const MAX_TRIANGLES: usize = 1_000_000;

/// Largest accepted padding, as a fraction of each image dimension.
pub const MAX_PADDING: f32 = 0.45;

/// Colour of pixels that no triangle covers.
pub const BACKGROUND: [u8; 3] = [24, 26, 30];

const AMBIENT: f32 = 0.35;
const DIFFUSE: f32 = 0.65;

/// Light direction in view space, pointing from the surface toward the light.
/// It sits in front of the viewer and slightly up and to the right.
const LIGHT_DIR: [f32; 3] = [0.3, 0.5, 1.0];

/// Tolerance that accepts pixels lying exactly on a shared triangle edge.
const EDGE_EPS: f32 = 1e-6;

/// Doubled screen-space areas below this are treated as degenerate.
const MIN_AREA2: f32 = 1e-9;

/// A triangle in model space with a base colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub c: [f32; 3],
    pub color: [u8; 3],
}

/// Camera orientation and output image description.
///
/// `yaw_deg` turns the model about the vertical axis and `pitch_deg` tilts it
/// about the horizontal axis. `padding` is the margin on each side as a
/// fraction of the image size, and must lie in `0.0..=MAX_PADDING`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub width: u32,
    pub height: u32,
    pub padding: f32,
}

impl View {
    /// Checks that the view describes an image that can be rendered.
    pub fn validate(&self) -> Result<(), OrthoError> {
        if self.width == 0 || self.height == 0 {
            return Err(OrthoError::InvalidView("width and height must be non-zero"));
        }
        if self.width > MAX_DIMENSION || self.height > MAX_DIMENSION {
            return Err(OrthoError::InvalidView(
                "width and height must be at most 4096",
            ));
        }
        if !self.yaw_deg.is_finite() || !self.pitch_deg.is_finite() {
            return Err(OrthoError::InvalidView("yaw and pitch must be finite"));
        }
        if !(0.0..=MAX_PADDING).contains(&self.padding) {
            return Err(OrthoError::InvalidView(
                "padding must be between 0.0 and 0.45",
            ));
        }
        Ok(())
    }
}

/// Errors returned by [`render`] and [`render_png`].
#[derive(Debug, Clone, PartialEq)]
pub enum OrthoError {
    /// The view failed validation; the message says which rule was broken.
    InvalidView(&'static str),
    /// More triangles were supplied than the renderer accepts.
    TooManyTriangles { count: usize, max: usize },
    /// A vertex coordinate was NaN or infinite.
    NonFiniteVertex { index: usize },
    /// Encoding the rendered pixels as PNG failed.
    Png(PngError),
}

impl fmt::Display for OrthoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrthoError::InvalidView(reason) => write!(f, "invalid view: {reason}"),
            OrthoError::TooManyTriangles { count, max } => {
                write!(f, "{count} triangles exceeds the limit of {max}")
            }
            OrthoError::NonFiniteVertex { index } => {
                write!(f, "triangle {index} has a NaN or infinite vertex")
            }
            OrthoError::Png(err) => write!(f, "PNG encoding failed: {err}"),
        }
    }
}

impl Error for OrthoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            OrthoError::Png(err) => Some(err),
            _ => None,
        }
    }
}

impl From<PngError> for OrthoError {
    fn from(err: PngError) -> Self {
        OrthoError::Png(err)
    }
}

/// Renders the triangles to RGBA pixels, `width * height * 4` bytes, top row first.
///
/// Pixels with no coverage get [`BACKGROUND`] at full alpha. When triangles
/// overlap, the one nearest the viewer wins. Triangles with zero projected area
/// are skipped.
pub fn render(triangles: &[Triangle], view: &View) -> Result<Vec<u8>, OrthoError> {
    view.validate()?;
    if triangles.len() > MAX_TRIANGLES {
        return Err(OrthoError::TooManyTriangles {
            count: triangles.len(),
            max: MAX_TRIANGLES,
        });
    }
    for (index, tri) in triangles.iter().enumerate() {
        let finite = [tri.a, tri.b, tri.c]
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()));
        if !finite {
            return Err(OrthoError::NonFiniteVertex { index });
        }
    }

    let width = view.width as usize;
    let height = view.height as usize;
    let mut rgba = [BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 255].repeat(width * height);

    let rot = view_rotation(view);
    let light = normalize(LIGHT_DIR).unwrap_or([0.0, 0.0, 1.0]);

    // Move every triangle into view space: x and y are screen axes, and z
    // grows toward the viewer.
    let projected: Vec<Projected> = triangles
        .iter()
        .filter_map(|tri| {
            let pts = [tri.a, tri.b, tri.c].map(|p| apply(&rot, p));
            let normal = normalize(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])))?;
            Some(Projected {
                pts,
                normal,
                color: tri.color,
            })
        })
        .collect();
    if projected.is_empty() {
        return Ok(rgba);
    }

    // Uniform scale and centring so the whole mesh fits inside the padding.
    let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
    for tri in &projected {
        for p in &tri.pts {
            min_x = min_x.min(p[0]);
            max_x = max_x.max(p[0]);
            min_y = min_y.min(p[1]);
            max_y = max_y.max(p[1]);
        }
    }
    let usable = 1.0 - 2.0 * view.padding;
    let fit_w = width as f32 * usable;
    let fit_h = height as f32 * usable;
    let mut scale = f32::INFINITY;
    if max_x > min_x {
        scale = scale.min(fit_w / (max_x - min_x));
    }
    if max_y > min_y {
        scale = scale.min(fit_h / (max_y - min_y));
    }
    if !scale.is_finite() || scale <= 0.0 {
        scale = 1.0;
    }
    let cx = 0.5 * (min_x + max_x);
    let cy = 0.5 * (min_y + max_y);
    let half_w = width as f32 * 0.5;
    let half_h = height as f32 * 0.5;
    let to_screen = |p: [f32; 3]| -> [f32; 3] {
        [
            (p[0] - cx) * scale + half_w,
            half_h - (p[1] - cy) * scale,
            p[2],
        ]
    };

    let mut depth = vec![f32::NEG_INFINITY; width * height];
    for tri in &projected {
        let s = tri.pts.map(to_screen);
        let area2 = edge(s[0], s[1], s[2]);
        if area2.abs() < MIN_AREA2 {
            continue;
        }
        let color = shade(tri.normal, light, tri.color);

        let (x0, x1) = pixel_span(s.iter().map(|p| p[0]), width);
        let (y0, y1) = pixel_span(s.iter().map(|p| p[1]), height);
        for py in y0..=y1 {
            let sample_y = py as f32 + 0.5;
            for px in x0..=x1 {
                let p = [px as f32 + 0.5, sample_y, 0.0];
                let w0 = edge(s[1], s[2], p) / area2;
                let w1 = edge(s[2], s[0], p) / area2;
                let w2 = edge(s[0], s[1], p) / area2;
                if w0 < -EDGE_EPS || w1 < -EDGE_EPS || w2 < -EDGE_EPS {
                    continue;
                }
                let z = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let idx = py * width + px;
                if z > depth[idx] {
                    depth[idx] = z;
                    let o = idx * 4;
                    rgba[o..o + 3].copy_from_slice(&color);
                }
            }
        }
    }

    Ok(rgba)
}

/// Renders the triangles and encodes the result as a PNG file.
pub fn render_png(triangles: &[Triangle], view: &View) -> Result<Vec<u8>, OrthoError> {
    let rgba = render(triangles, view)?;
    Ok(png::encode_rgba(view.width, view.height, &rgba)?)
}

/// A triangle after rotation: view-space points, unit normal, base colour.
struct Projected {
    pts: [[f32; 3]; 3],
    normal: [f32; 3],
    color: [u8; 3],
}

type Mat3 = [[f32; 3]; 3];

/// Rotation for the view: yaw about Y first, then pitch about X.
fn view_rotation(view: &View) -> Mat3 {
    let (sy, cy) = view.yaw_deg.to_radians().sin_cos();
    let (sp, cp) = view.pitch_deg.to_radians().sin_cos();
    let yaw = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let pitch = [[1.0, 0.0, 0.0], [0.0, cp, -sp], [0.0, sp, cp]];
    mat_mul(&pitch, &yaw)
}

fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn apply(m: &Mat3, v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Unit vector in the same direction, or `None` if the input has no length.
fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = dot(v, v).sqrt();
    if !len.is_finite() || len <= 0.0 {
        return None;
    }
    Some([v[0] / len, v[1] / len, v[2] / len])
}

/// Signed doubled area of the screen-space triangle `(a, b, p)`, using x and y.
fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Flat shading: the face colour scaled by ambient plus diffuse light.
///
/// The absolute value of the dot product lights both faces of a surface, so
/// a triangle wound the other way still gets a sensible colour.
fn shade(normal: [f32; 3], light: [f32; 3], color: [u8; 3]) -> [u8; 3] {
    let k = AMBIENT + DIFFUSE * dot(normal, light).abs();
    color.map(|c| (f32::from(c) * k).round().clamp(0.0, 255.0) as u8)
}

/// Inclusive range of pixel indices that a set of coordinates touches.
fn pixel_span(coords: impl Iterator<Item = f32>, limit: usize) -> (usize, usize) {
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for c in coords {
        lo = lo.min(c);
        hi = hi.max(c);
    }
    let max_index = (limit - 1) as f32;
    let start = lo.floor().clamp(0.0, max_index) as usize;
    let end = hi.ceil().clamp(0.0, max_index) as usize;
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(size: u32) -> View {
        View {
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            width: size,
            height: size,
            padding: 0.1,
        }
    }

    fn pixel(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let o = (y * width + x) * 4;
        [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]
    }

    fn background() -> [u8; 4] {
        [BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 255]
    }

    /// Triangle in the plane `z = depth`, with its normal pointing at the viewer.
    fn front_triangle(color: [u8; 3], depth: f32) -> Triangle {
        Triangle {
            a: [-1.0, -1.0, depth],
            b: [1.0, -1.0, depth],
            c: [0.0, 1.0, depth],
            color,
        }
    }

    #[test]
    fn front_facing_triangle_fills_expected_region() {
        let rgba = render(&[front_triangle([200, 100, 50], 0.0)], &view(100)).unwrap();
        assert_eq!(rgba.len(), 100 * 100 * 4);

        // With 10% padding the triangle is 80 px on a side, so its area is about 3200 px.
        let covered: Vec<(usize, usize)> = (0..100)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(&rgba, 100, x, y) != background())
            .collect();
        assert!(
            (2900..=3500).contains(&covered.len()),
            "covered {} pixels",
            covered.len()
        );

        let min_x = covered.iter().map(|p| p.0).min().unwrap();
        let max_x = covered.iter().map(|p| p.0).max().unwrap();
        let min_y = covered.iter().map(|p| p.1).min().unwrap();
        let max_y = covered.iter().map(|p| p.1).max().unwrap();
        assert!(min_x >= 9 && max_x <= 91, "x span {min_x}..{max_x}");
        assert!(min_y >= 9 && max_y <= 91, "y span {min_y}..{max_y}");

        assert_eq!(pixel(&rgba, 100, 2, 2), background());
        assert_eq!(pixel(&rgba, 100, 50, 5), background());
        assert_ne!(pixel(&rgba, 100, 50, 85), background());
        assert_ne!(pixel(&rgba, 100, 50, 20), background());
    }

    #[test]
    fn front_facing_triangle_uses_flat_shaded_colour() {
        let rgba = render(&[front_triangle([200, 100, 50], 0.0)], &view(100)).unwrap();
        // A face normal of (0, 0, 1) gives a diffuse term of |light.z|.
        let light = normalize(LIGHT_DIR).unwrap();
        let k = AMBIENT + DIFFUSE * light[2];
        let p = pixel(&rgba, 100, 50, 85);
        let expected = [200.0, 100.0, 50.0].map(|c: f32| (c * k).round());
        for ch in 0..3 {
            assert!(
                (f32::from(p[ch]) - expected[ch]).abs() <= 1.0,
                "channel {ch}: got {}, expected {}",
                p[ch],
                expected[ch]
            );
        }
        assert_eq!(p[3], 255);
    }

    #[test]
    fn nearer_triangle_wins_regardless_of_draw_order() {
        let near = front_triangle([0, 255, 0], 1.0);
        let far = front_triangle([255, 0, 0], -1.0);
        for tris in [[far, near], [near, far]] {
            let rgba = render(&tris, &view(64)).unwrap();
            let p = pixel(&rgba, 64, 32, 40);
            assert!(
                p[1] > 200 && p[0] < 30,
                "pixel {p:?} is not the near colour"
            );
        }
    }

    #[test]
    fn edge_on_triangle_draws_nothing() {
        // A yaw of 90 degrees turns the flat triangle edge-on to the viewer.
        let v = View {
            yaw_deg: 90.0,
            ..view(64)
        };
        let rgba = render(&[front_triangle([200, 100, 50], 0.0)], &v).unwrap();
        assert!(rgba.as_chunks::<4>().0.iter().all(|px| *px == background()));
    }

    #[test]
    fn empty_mesh_is_background() {
        let rgba = render(&[], &view(4)).unwrap();
        assert_eq!(rgba.len(), 4 * 4 * 4);
        assert!(rgba.as_chunks::<4>().0.iter().all(|px| *px == background()));
    }

    #[test]
    fn invalid_views_are_rejected() {
        let base = view(16);
        let bad = [
            View { width: 0, ..base },
            View { height: 0, ..base },
            View {
                width: MAX_DIMENSION + 1,
                ..base
            },
            View {
                height: 5000,
                ..base
            },
            View {
                yaw_deg: f32::NAN,
                ..base
            },
            View {
                pitch_deg: f32::INFINITY,
                ..base
            },
            View {
                padding: -0.01,
                ..base
            },
            View {
                padding: 0.5,
                ..base
            },
            View {
                padding: f32::NAN,
                ..base
            },
        ];
        for v in bad {
            assert!(
                matches!(render(&[], &v), Err(OrthoError::InvalidView(_))),
                "render accepted {v:?}"
            );
            assert!(
                matches!(render_png(&[], &v), Err(OrthoError::InvalidView(_))),
                "render_png accepted {v:?}"
            );
        }
    }

    #[test]
    fn maximum_dimension_is_accepted() {
        let v = View {
            width: MAX_DIMENSION,
            height: 1,
            ..view(1)
        };
        let rgba = render(&[], &v).unwrap();
        assert_eq!(rgba.len(), MAX_DIMENSION as usize * 4);
    }

    #[test]
    fn too_many_triangles_are_rejected() {
        let tris = vec![front_triangle([1, 2, 3], 0.0); MAX_TRIANGLES + 1];
        assert!(matches!(
            render(&tris, &view(8)),
            Err(OrthoError::TooManyTriangles { .. })
        ));
    }

    #[test]
    fn non_finite_vertex_is_rejected() {
        let mut t = front_triangle([1, 2, 3], 0.0);
        t.b[0] = f32::NAN;
        assert_eq!(
            render(&[t], &view(8)),
            Err(OrthoError::NonFiniteVertex { index: 0 })
        );
    }

    #[test]
    fn render_png_produces_png_of_requested_size() {
        let v = View {
            width: 30,
            height: 20,
            ..view(1)
        };
        let bytes = render_png(&[front_triangle([200, 100, 50], 0.0)], &v).unwrap();
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        assert_eq!(&bytes[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 30);
        assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 20);
    }
}
