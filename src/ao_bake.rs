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
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Largest accepted number of rays per corner.
pub const MAX_RAYS: u32 = 512;

/// Largest number of triangles accepted by one bake call.
pub const MAX_TRIANGLES: usize = 2_000_000;

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
    /// The number of values is not three per triangle.
    ValueCount { expected: usize, actual: usize },
    /// The output file already exists and was left untouched.
    FileExists(PathBuf),
    /// Creating the parent folder, creating the file or writing it failed.
    Io(String),
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
            AoError::ValueCount { expected, actual } => {
                write!(f, "expected {expected} AO values, got {actual}")
            }
            AoError::FileExists(path) => {
                write!(f, "{} already exists, choose another name", path.display())
            }
            AoError::Io(message) => write!(f, "{message}"),
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
    let bvh = Bvh::build(triangles);

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(triangles.len().max(1));

    if thread_count <= 1 || triangles.len() < 128 {
        let mut out = Vec::with_capacity(triangles.len() * 3);
        bake_range(triangles, &bvh, &local_dirs, rays, settings, 0..triangles.len(), &mut out);
        return Ok(out);
    }

    let chunk_size = triangles.len().div_ceil(thread_count);
    let mut chunks: Vec<Vec<f32>> = vec![Vec::new(); thread_count];
    std::thread::scope(|s| {
        for (t_idx, slot) in chunks.iter_mut().enumerate() {
            let start = t_idx * chunk_size;
            if start >= triangles.len() {
                break;
            }
            let end = (start + chunk_size).min(triangles.len());
            let bvh_ref = &bvh;
            let dirs_ref = &local_dirs;
            s.spawn(move || {
                slot.reserve((end - start) * 3);
                bake_range(triangles, bvh_ref, dirs_ref, rays, settings, start..end, slot);
            });
        }
    });

    let mut out = Vec::with_capacity(triangles.len() * 3);
    for c in chunks {
        out.extend(c);
    }
    Ok(out)
}

fn bake_range(
    triangles: &[Triangle],
    bvh: &Bvh,
    local_dirs: &[[f32; 3]],
    rays: f32,
    settings: &AoSettings,
    range: std::ops::Range<usize>,
    out: &mut Vec<f32>,
) {
    for index in range {
        let tri = &triangles[index];
        let Some(normal) = face_normal(tri) else {
            out.extend([1.0; 3]);
            continue;
        };
        let (tangent, bitangent) = orthonormal_basis(normal);
        for corner in [tri.a, tri.b, tri.c] {
            let origin = add(corner, scale(normal, settings.bias));
            let mut open = 0u32;
            for local in local_dirs {
                let dir = add(
                    add(scale(tangent, local[0]), scale(bitangent, local[1])),
                    scale(normal, local[2]),
                );
                if !bvh.is_occluded(origin, dir, settings.max_distance, triangles, index) {
                    open += 1;
                }
            }
            out.push(open as f32 / rays);
        }
    }
}

/// UV-mapped triangle for baking an ambient occlusion texture map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvTriangle {
    pub tri: Triangle,
    pub uv_a: [f32; 2],
    pub uv_b: [f32; 2],
    pub uv_c: [f32; 2],
}

/// Bakes ambient occlusion into an RGBA texture (`width * height * 4` bytes) in UV space.
///
/// Uncovered texels stay white `(255, 255, 255, 255)`. Covered texels are rasterised
/// barycentrically and raycast against the full mesh BVH.
pub fn bake_uv_ao_rgba(
    uv_tris: &[UvTriangle],
    width: u32,
    height: u32,
    settings: &AoSettings,
) -> Result<Vec<u8>, AoError> {
    settings.validate()?;
    if uv_tris.len() > MAX_TRIANGLES {
        return Err(AoError::TooManyTriangles {
            count: uv_tris.len(),
            max: MAX_TRIANGLES,
        });
    }
    let tris: Vec<Triangle> = uv_tris.iter().map(|u| u.tri).collect();
    for (index, tri) in tris.iter().enumerate() {
        let finite = [tri.a, tri.b, tri.c]
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()));
        if !finite {
            return Err(AoError::NonFiniteVertex { index });
        }
    }

    let bvh = Bvh::build(&tris);
    let local_dirs = hemisphere_directions(settings.rays);
    let rays = settings.rays as f32;
    let w = width.max(1) as usize;
    let h = height.max(1) as usize;
    let mut rgba = vec![255u8; w * h * 4];

    for (tri_idx, uvt) in uv_tris.iter().enumerate() {
        let Some(normal) = face_normal(&uvt.tri) else {
            continue;
        };
        let (tangent, bitangent) = orthonormal_basis(normal);
        let p0 = [uvt.uv_a[0].clamp(0.0, 1.0) * (w as f32), uvt.uv_a[1].clamp(0.0, 1.0) * (h as f32)];
        let p1 = [uvt.uv_b[0].clamp(0.0, 1.0) * (w as f32), uvt.uv_b[1].clamp(0.0, 1.0) * (h as f32)];
        let p2 = [uvt.uv_c[0].clamp(0.0, 1.0) * (w as f32), uvt.uv_c[1].clamp(0.0, 1.0) * (h as f32)];
        let area = edge2d(p0, p1, p2);
        if area.abs() < 1e-6 {
            continue;
        }
        let x0 = p0[0].min(p1[0]).min(p2[0]).floor().max(0.0) as usize;
        let x1 = (p0[0].max(p1[0]).max(p2[0]).ceil() as usize).min(w - 1);
        let y0 = p0[1].min(p1[1]).min(p2[1]).floor().max(0.0) as usize;
        let y1 = (p0[1].max(p1[1]).max(p2[1]).ceil() as usize).min(h - 1);

        for py in y0..=y1 {
            for px in x0..=x1 {
                let pt = [px as f32 + 0.5, py as f32 + 0.5];
                let w0 = edge2d(p1, p2, pt) / area;
                let w1 = edge2d(p2, p0, pt) / area;
                let w2 = edge2d(p0, p1, pt) / area;
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let pos = [
                    w0 * uvt.tri.a[0] + w1 * uvt.tri.b[0] + w2 * uvt.tri.c[0],
                    w0 * uvt.tri.a[1] + w1 * uvt.tri.b[1] + w2 * uvt.tri.c[1],
                    w0 * uvt.tri.a[2] + w1 * uvt.tri.b[2] + w2 * uvt.tri.c[2],
                ];
                let origin = add(pos, scale(normal, settings.bias));
                let mut open = 0u32;
                for local in &local_dirs {
                    let dir = add(
                        add(scale(tangent, local[0]), scale(bitangent, local[1])),
                        scale(normal, local[2]),
                    );
                    if !bvh.is_occluded(origin, dir, settings.max_distance, &tris, tri_idx) {
                        open += 1;
                    }
                }
                let val = ((open as f32 / rays).clamp(0.0, 1.0) * 255.0).round() as u8;
                let off = (py * w + px) * 4;
                rgba[off..off + 4].copy_from_slice(&[val, val, val, 255]);
            }
        }
    }
    Ok(rgba)
}

fn edge2d(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
}

// ---------------------------------------------------------------------------
// Median-split Bounding Volume Hierarchy (BVH)
// ---------------------------------------------------------------------------

const BVH_LEAF_SIZE: usize = 8;

#[derive(Clone, Copy)]
struct Aabb {
    min: [f32; 3],
    max: [f32; 3],
}

impl Aabb {
    fn empty() -> Self {
        Self {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        }
    }

    fn of_tri(t: &Triangle) -> Self {
        let mut b = Self::empty();
        for p in [t.a, t.b, t.c] {
            for axis in 0..3 {
                b.min[axis] = b.min[axis].min(p[axis]);
                b.max[axis] = b.max[axis].max(p[axis]);
            }
        }
        b
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: [
                self.min[0].min(other.min[0]),
                self.min[1].min(other.min[1]),
                self.min[2].min(other.min[2]),
            ],
            max: [
                self.max[0].max(other.max[0]),
                self.max[1].max(other.max[1]),
                self.max[2].max(other.max[2]),
            ],
        }
    }

    fn hits_ray(&self, origin: [f32; 3], inv_dir: [f32; 3], max_t: f32) -> bool {
        let mut tmin = HIT_EPSILON;
        let mut tmax = max_t;
        for axis in 0..3 {
            let t0 = (self.min[axis] - origin[axis]) * inv_dir[axis];
            let t1 = (self.max[axis] - origin[axis]) * inv_dir[axis];
            let (near, far) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
            tmin = tmin.max(near);
            tmax = tmax.min(far);
            if tmax < tmin {
                return false;
            }
        }
        true
    }
}

struct BvhNode {
    bounds: Aabb,
    /// When `count > 0`, leaf covering `indices[first..first + count]`.
    /// When `count == 0`, internal node with children `first` and `first + 1`.
    first: u32,
    count: u32,
}

struct Bvh {
    nodes: Vec<BvhNode>,
    indices: Vec<usize>,
}

impl Bvh {
    fn build(triangles: &[Triangle]) -> Self {
        if triangles.is_empty() {
            return Self {
                nodes: Vec::new(),
                indices: Vec::new(),
            };
        }
        let tri_bounds: Vec<Aabb> = triangles.iter().map(Aabb::of_tri).collect();
        let centroids: Vec<[f32; 3]> = triangles
            .iter()
            .map(|t| {
                [
                    (t.a[0] + t.b[0] + t.c[0]) / 3.0,
                    (t.a[1] + t.b[1] + t.c[1]) / 3.0,
                    (t.a[2] + t.b[2] + t.c[2]) / 3.0,
                ]
            })
            .collect();
        let mut indices: Vec<usize> = (0..triangles.len()).collect();
        let mut nodes = Vec::with_capacity(triangles.len() / 2 + 1);
        nodes.push(BvhNode {
            bounds: Aabb::empty(),
            first: 0,
            count: triangles.len() as u32,
        });
        Self::subdivide(0, 0, triangles.len(), &tri_bounds, &centroids, &mut indices, &mut nodes);
        Self { nodes, indices }
    }

    fn subdivide(
        node_idx: usize,
        start: usize,
        count: usize,
        tri_bounds: &[Aabb],
        centroids: &[[f32; 3]],
        indices: &mut [usize],
        nodes: &mut Vec<BvhNode>,
    ) {
        let mut bounds = Aabb::empty();
        for &idx in &indices[start..start + count] {
            bounds = bounds.union(tri_bounds[idx]);
        }
        nodes[node_idx].bounds = bounds;
        if count <= BVH_LEAF_SIZE {
            nodes[node_idx].first = start as u32;
            nodes[node_idx].count = count as u32;
            return;
        }
        let extent = [
            bounds.max[0] - bounds.min[0],
            bounds.max[1] - bounds.min[1],
            bounds.max[2] - bounds.min[2],
        ];
        let axis = if extent[1] > extent[0] && extent[1] >= extent[2] {
            1
        } else if extent[2] > extent[0] {
            2
        } else {
            0
        };
        let mid = count / 2;
        indices[start..start + count].select_nth_unstable_by(mid, |&a, &b| {
            centroids[a][axis].total_cmp(&centroids[b][axis])
        });
        let left_idx = nodes.len();
        nodes.push(BvhNode { bounds: Aabb::empty(), first: 0, count: 0 });
        nodes.push(BvhNode { bounds: Aabb::empty(), first: 0, count: 0 });
        nodes[node_idx].first = left_idx as u32;
        nodes[node_idx].count = 0;
        Self::subdivide(left_idx, start, mid, tri_bounds, centroids, indices, nodes);
        Self::subdivide(left_idx + 1, start + mid, count - mid, tri_bounds, centroids, indices, nodes);
    }

    fn is_occluded(
        &self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_t: f32,
        triangles: &[Triangle],
        skip: usize,
    ) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let inv_dir = [1.0 / dir[0], 1.0 / dir[1], 1.0 / dir[2]];
        let mut stack = [0usize; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp]];
            if !node.bounds.hits_ray(origin, inv_dir, max_t) {
                continue;
            }
            if node.count > 0 {
                let s = node.first as usize;
                let e = s + node.count as usize;
                for &tri_idx in &self.indices[s..e] {
                    if tri_idx != skip && ray_hits(origin, dir, &triangles[tri_idx], max_t) {
                        return true;
                    }
                }
            } else if sp + 2 <= stack.len() {
                let first = node.first as usize;
                stack[sp] = first;
                stack[sp + 1] = first + 1;
                sp += 2;
            }
        }
        false
    }
}

/// Writes baked values to a new CSV file at `path`, one row per triangle corner.
///
/// The file starts with a `triangle,corner,ao` header. Each following row holds
/// the triangle index, the corner (0, 1 or 2) and the value with four decimals.
/// `values` must hold three entries per triangle, in the order returned by
/// [`bake_vertex_ao`]. Parent folders are created only when they are missing.
/// An existing file is never overwritten: the call fails with
/// [`AoError::FileExists`] and leaves it untouched.
pub fn write_ao_csv(path: &Path, triangles_count: usize, values: &[f32]) -> Result<(), AoError> {
    let expected = triangles_count.saturating_mul(3);
    if values.len() != expected {
        return Err(AoError::ValueCount {
            expected,
            actual: values.len(),
        });
    }

    let mut csv = String::from("triangle,corner,ao\r\n");
    for (i, value) in values.iter().enumerate() {
        csv.push_str(&format!("{},{},{:.4}\r\n", i / 3, i % 3, value));
    }

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| AoError::Io(format!("could not create {}: {e}", parent.display())))?;
    }
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(AoError::FileExists(path.to_path_buf()));
        }
        Err(e) => {
            return Err(AoError::Io(format!("could not create {}: {e}", path.display())));
        }
    };
    if let Err(e) = file.write_all(csv.as_bytes()) {
        let _ = fs::remove_file(path);
        return Err(AoError::Io(format!("could not write {}: {e}", path.display())));
    }
    Ok(())
}

/// Returns true if any triangle other than `skip` is hit by the ray within `max_t`.
#[allow(dead_code)]
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
    fn write_ao_csv_writes_corner_rows_and_refuses_overwrite() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "ao_csv_test_{}_{nanos}",
            std::process::id()
        ));
        let path = dir.join("nested").join("ao.csv");

        write_ao_csv(&path, 2, &[1.0, 0.5, 0.25, 0.0, 0.123_456, 0.9999]).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "triangle,corner,ao\r\n\
             0,0,1.0000\r\n\
             0,1,0.5000\r\n\
             0,2,0.2500\r\n\
             1,0,0.0000\r\n\
             1,1,0.1235\r\n\
             1,2,0.9999\r\n"
        );

        let err = write_ao_csv(&path, 2, &[0.0; 6]).unwrap_err();
        assert_eq!(err, AoError::FileExists(path.clone()));
        assert!(fs::read_to_string(&path).unwrap().starts_with("triangle,corner,ao\r\n0,0,1.0000"));

        let short = dir.join("short.csv");
        assert_eq!(
            write_ao_csv(&short, 2, &[0.0; 5]),
            Err(AoError::ValueCount { expected: 6, actual: 5 })
        );
        assert!(!short.exists());

        fs::remove_dir_all(&dir).unwrap();
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

    #[test]
    fn bvh_matches_brute_force_on_500_triangle_mesh() {
        let mut tris = Vec::with_capacity(500);
        for i in 0..250 {
            let row = (i / 15) as f32;
            let col = (i % 15) as f32;
            let z = ((i * 7) % 11) as f32 * 0.05;
            tris.push(tri([col, row, z], [col + 1.0, row, z], [col + 1.0, row + 1.0, z + 0.1]));
            tris.push(tri([col, row, z], [col + 1.0, row + 1.0, z + 0.1], [col, row + 1.0, z]));
        }
        let cfg = settings(32, 2.0);
        let bvh_ao = bake_vertex_ao(&tris, &cfg).unwrap();
        let dirs = hemisphere_directions(cfg.rays);
        // Spot-check first 30 triangles against brute-force `is_occluded`.
        for (idx, t) in tris.iter().enumerate().take(30) {
            let normal = face_normal(t).unwrap();
            let (tan, bitan) = orthonormal_basis(normal);
            for (ci, corner) in [t.a, t.b, t.c].iter().enumerate() {
                let origin = add(*corner, scale(normal, cfg.bias));
                let mut open = 0u32;
                for local in &dirs {
                    let dir = add(
                        add(scale(tan, local[0]), scale(bitan, local[1])),
                        scale(normal, local[2]),
                    );
                    if !is_occluded(origin, dir, cfg.max_distance, &tris, idx) {
                        open += 1;
                    }
                }
                let expected = open as f32 / cfg.rays as f32;
                let actual = bvh_ao[idx * 3 + ci];
                assert!(
                    (actual - expected).abs() < 1e-4,
                    "tri {idx} corner {ci}: bvh={actual}, brute={expected}"
                );
            }
        }
    }

    #[test]
    fn uv_ao_baker_produces_rgba_texture() {
        let uv_tris = vec![UvTriangle {
            tri: flat_plane()[0],
            uv_a: [0.0, 0.0],
            uv_b: [1.0, 0.0],
            uv_c: [1.0, 1.0],
        }];
        let rgba = bake_uv_ao_rgba(&uv_tris, 16, 16, &settings(16, 1.0)).unwrap();
        assert_eq!(rgba.len(), 16 * 16 * 4);
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
    }
}
