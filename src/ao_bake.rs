//! CPU ambient occlusion baking for triangle meshes.
//!
//! Every triangle corner gets one value in `0.0..=1.0`: the fraction of
//! cosine-weighted directions over the hemisphere around the triangle's face
//! normal that travel `max_distance` without hitting another triangle. `0.0`
//! means the corner is fully occluded and `1.0` means it is open.
//!
//! Directions come from a fixed stratified pattern, so the same input always
//! bakes to the same output. Each ray is tested against every triangle, which
//! costs time proportional to corners times rays times triangles; the triangle
//! count is therefore capped at [`MAX_TRIANGLES`].
//!
//! The face normal follows the winding order (right-hand rule), so the
//! hemisphere is taken on the side the triangle is wound to face. Degenerate
//! triangles have no normal: their corners are reported as open, and they never
//! occlude anything.

use crate::ortho::Triangle;
use std::error::Error;
use std::fmt;

/// Largest accepted number of rays per corner.
pub const MAX_RAYS: u32 = 512;

/// Largest number of triangles accepted by one bake call.
pub const MAX_TRIANGLES: usize = 20_000;

/// Smallest ray parameter counted as a hit, so numerical noise at the start of
/// a ray is not mistaken for occlusion.
const HIT_EPSILON: f32 = 1e-6;

/// Determinant magnitude below which a ray is treated as parallel to a triangle.
const PARALLEL_EPSILON: f32 = 1e-12;

/// Fractional part of the golden ratio, used to spread azimuth samples evenly.
const GOLDEN_FRACTION: f32 = 0.618_034;

/// Sampling and occlusion parameters for [`bake_vertex_ao`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AoSettings {
    /// Rays cast per corner. Must lie in `1..=MAX_RAYS`.
    pub rays: u32,
    /// Occluders further than this from a corner are ignored. Must be finite and greater than zero.
    pub max_distance: f32,
    /// Distance the ray origin is lifted along the face normal, so a corner does
    /// not occlude itself. Must be finite and non-negative.
    pub bias: f32,
}

impl AoSettings {
    /// Checks every field against its allowed range.
    pub fn validate(&self) -> Result<(), AoError> {
        if self.rays == 0 || self.rays > MAX_RAYS {
            return Err(AoError::InvalidRays(self.rays));
        }
        if !(self.max_distance.is_finite() && self.max_distance > 0.0) {
            return Err(AoError::InvalidMaxDistance(self.max_distance));
        }
        if !(self.bias.is_finite() && self.bias >= 0.0) {
            return Err(AoError::InvalidBias(self.bias));
        }
        Ok(())
    }
}

/// Errors returned by [`bake_vertex_ao`].
#[derive(Debug, Clone, PartialEq)]
pub enum AoError {
    /// The ray count was outside `1..=MAX_RAYS`.
    InvalidRays(u32),
    /// The maximum distance was not a finite, positive number.
    InvalidMaxDistance(f32),
    /// The bias was not a finite, non-negative number.
    InvalidBias(f32),
    /// More triangles were supplied than the baker accepts.
    TooManyTriangles { count: usize, max: usize },
    /// A vertex coordinate was NaN or infinite.
    NonFiniteVertex { index: usize },
}

impl fmt::Display for AoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AoError::InvalidRays(rays) => {
                write!(f, "ray count {rays} is outside 1..={MAX_RAYS}")
            }
            AoError::InvalidMaxDistance(d) => {
                write!(f, "max distance {d} must be finite and greater than zero")
            }
            AoError::InvalidBias(b) => {
                write!(f, "bias {b} must be finite and non-negative")
            }
            AoError::TooManyTriangles { count, max } => {
                write!(f, "{count} triangles exceeds the limit of {max}")
            }
            AoError::NonFiniteVertex { index } => {
                write!(f, "triangle {index} has a NaN or infinite vertex")
            }
        }
    }
}

impl Error for AoError {}

/// Bakes one ambient occlusion value per triangle corner.
///
/// The output holds three values per triangle, in the order `a`, `b`, `c` for
/// each triangle in `triangles`. Each value is the share of unblocked rays in a
/// cosine-weighted hemisphere around the triangle's face normal, where a ray is
/// blocked by any other triangle within `settings.max_distance`.
pub fn bake_vertex_ao(
    triangles: &[Triangle],
    settings: &AoSettings,
) -> Result<Vec<f32>, AoError> {
    settings.validate()?;
    if triangles.len() > MAX_TRIANGLES {
        return Err(AoError::TooManyTriangles {
            count: triangles.len(),
            max: MAX_TRIANGLES,
        });
    }
    for (index, tri) in triangles.iter().enumerate() {
        let finite = [tri.a, tri.b, tri.c]
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()));
        if !finite {
            return Err(AoError::NonFiniteVertex { index });
        }
    }

    let local_dirs = hemisphere_directions(settings.rays);
    let rays = settings.rays as f32;
    let mut out = Vec::with_capacity(triangles.len() * 3);

    for (index, tri) in triangles.iter().enumerate() {
        let Some(normal) = face_normal(tri) else {
            out.extend([1.0; 3]);
            continue;
        };
        let (tangent, bitangent) = orthonormal_basis(normal);
        for corner in [tri.a, tri.b, tri.c] {
            let origin = add(corner, scale(normal, settings.bias));
            let mut open = 0u32;
            for local in &local_dirs {
                let dir = add(
                    add(scale(tangent, local[0]), scale(bitangent, local[1])),
                    scale(normal, local[2]),
                );
                if !is_occluded(origin, dir, settings.max_distance, triangles, index) {
                    open += 1;
                }
            }
            out.push(open as f32 / rays);
        }
    }

    Ok(out)
}

/// Returns true if any triangle other than `skip` is hit by the ray within `max_t`.
fn is_occluded(
    origin: [f32; 3],
    dir: [f32; 3],
    max_t: f32,
    triangles: &[Triangle],
    skip: usize,
) -> bool {
    triangles
        .iter()
        .enumerate()
        .any(|(j, tri)| j != skip && ray_hits(origin, dir, tri, max_t))
}

/// Möller–Trumbore ray-triangle intersection. `dir` must be unit length.
fn ray_hits(origin: [f32; 3], dir: [f32; 3], tri: &Triangle, max_t: f32) -> bool {
    let e1 = sub(tri.b, tri.a);
    let e2 = sub(tri.c, tri.a);
    let p = cross(dir, e2);
    let det = dot(e1, p);
    if det.abs() < PARALLEL_EPSILON {
        return false;
    }
    let inv_det = 1.0 / det;
    let s = sub(origin, tri.a);
    let u = dot(s, p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = dot(dir, q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = dot(e2, q) * inv_det;
    t > HIT_EPSILON && t <= max_t
}

/// Unit face normal from the winding order, or `None` for a zero-area triangle.
fn face_normal(tri: &Triangle) -> Option<[f32; 3]> {
    let n = cross(sub(tri.b, tri.a), sub(tri.c, tri.a));
    let len = dot(n, n).sqrt();
    if len > 0.0 && len.is_finite() {
        Some(scale(n, 1.0 / len))
    } else {
        None
    }
}

/// Two unit vectors perpendicular to `n` and to each other.
fn orthonormal_basis(n: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let helper = if n[0].abs() > 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t = cross(helper, n);
    let t = scale(t, 1.0 / dot(t, t).sqrt());
    let b = cross(n, t);
    (t, b)
}

/// Deterministic cosine-weighted directions in a frame whose z axis is the normal.
///
/// The radial coordinate is stratified over the disk and the azimuth follows a
/// golden-angle sequence. Lifting each disk point onto the hemisphere gives a
/// cosine-weighted distribution, so the mean visibility is the occlusion term.
fn hemisphere_directions(count: u32) -> Vec<[f32; 3]> {
    let n = count as f32;
    (0..count)
        .map(|i| {
            let k = i as f32 + 0.5;
            let u = k / n;
            let phi = std::f32::consts::TAU * (k * GOLDEN_FRACTION).fract();
            let r = u.sqrt();
            [r * phi.cos(), r * phi.sin(), (1.0 - u).sqrt()]
        })
        .collect()
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Triangle {
        Triangle {
            a,
            b,
            c,
            color: [255, 255, 255],
        }
    }

    fn settings(rays: u32, max_distance: f32) -> AoSettings {
        AoSettings {
            rays,
            max_distance,
            bias: 1e-4,
        }
    }

    /// Two triangles forming a unit square in the z = 0 plane, normal +z.
    fn flat_plane() -> Vec<Triangle> {
        vec![
            tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            tri([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]),
        ]
    }

    /// A floor patch in the z = 0 plane meeting two walls (y = 0 and x = 0) at
    /// a concave corner, with the walls' normals pointing into the room.
    ///
    /// Layout: walls A, A, B, B, then floor-near (next to the crease) and
    /// floor-far (well away from both walls).
    fn room_corner() -> Vec<Triangle> {
        vec![
            // Wall A in y = 0, normal +y.
            tri([0.0, 0.0, 0.0], [0.0, 0.0, 4.0], [4.0, 0.0, 0.0]),
            tri([4.0, 0.0, 0.0], [0.0, 0.0, 4.0], [4.0, 0.0, 4.0]),
            // Wall B in x = 0, normal +x.
            tri([0.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]),
            tri([0.0, 4.0, 0.0], [0.0, 4.0, 4.0], [0.0, 0.0, 4.0]),
            // Floor near the crease, normal +z.
            tri([0.1, 0.1, 0.0], [0.5, 0.1, 0.0], [0.1, 0.5, 0.0]),
            // Floor far from both walls, normal +z.
            tri([1.5, 1.5, 0.0], [3.0, 1.5, 0.0], [1.5, 3.0, 0.0]),
        ]
    }

    #[test]
    fn flat_plane_is_fully_open() {
        let ao = bake_vertex_ao(&flat_plane(), &settings(64, 1.0)).unwrap();
        assert_eq!(ao.len(), 6);
        for (i, v) in ao.iter().enumerate() {
            assert!(*v >= 0.999 && *v <= 1.0, "corner {i} was {v}");
        }
    }

    #[test]
    fn corner_between_walls_is_darker_than_open_floor() {
        let ao = bake_vertex_ao(&room_corner(), &settings(128, 1.0)).unwrap();
        assert_eq!(ao.len(), 18);

        // Floor-near triangle is index 4; its first corner sits 0.1 from both walls.
        let near = ao[12];
        assert!(near < 0.5, "crease corner should be occluded, got {near}");

        // Floor-far triangle is index 5; every corner is out of reach of the walls.
        for v in &ao[15..18] {
            assert!(*v > 0.99, "open floor corner was {v}");
        }
    }

    #[test]
    fn baking_is_deterministic() {
        let scene = room_corner();
        let first = bake_vertex_ao(&scene, &settings(64, 1.0)).unwrap();
        let second = bake_vertex_ao(&scene, &settings(64, 1.0)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let tris = flat_plane();
        assert_eq!(
            bake_vertex_ao(&tris, &settings(0, 1.0)),
            Err(AoError::InvalidRays(0))
        );
        assert_eq!(
            bake_vertex_ao(&tris, &settings(MAX_RAYS + 1, 1.0)),
            Err(AoError::InvalidRays(MAX_RAYS + 1))
        );
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let err = bake_vertex_ao(&tris, &settings(16, bad)).unwrap_err();
            assert!(matches!(err, AoError::InvalidMaxDistance(_)), "{bad}");
        }
        let negative_bias = AoSettings {
            rays: 16,
            max_distance: 1.0,
            bias: -0.1,
        };
        assert!(matches!(
            bake_vertex_ao(&tris, &negative_bias),
            Err(AoError::InvalidBias(_))
        ));

        // The boundary values themselves are accepted.
        assert!(bake_vertex_ao(&tris, &settings(1, 1.0)).is_ok());
        assert!(bake_vertex_ao(&tris, &settings(MAX_RAYS, 1.0)).is_ok());
    }

    #[test]
    fn triangle_cap_is_enforced() {
        let tris = vec![flat_plane()[0]; MAX_TRIANGLES + 1];
        assert_eq!(
            bake_vertex_ao(&tris, &settings(1, 1.0)),
            Err(AoError::TooManyTriangles {
                count: MAX_TRIANGLES + 1,
                max: MAX_TRIANGLES,
            })
        );
    }

    #[test]
    fn non_finite_vertex_is_rejected() {
        let mut tris = flat_plane();
        tris[1].b[0] = f32::NAN;
        assert_eq!(
            bake_vertex_ao(&tris, &settings(16, 1.0)),
            Err(AoError::NonFiniteVertex { index: 1 })
        );
    }
}
