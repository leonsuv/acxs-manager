//! Parser and geometry utilities for Assetto Corsa `ai/fast_lane.ai` and
//! `ai/pit_lane.ai` binary spline files.
//!
//! File layout (little-endian):
//! - Header (16 bytes): `header_version: i32` (AC uses 7), `detail_count: i32`,
//!   `lap_time_ms: i32`, `sample_count: i32`.
//! - Ideal-line array (`detail_count * 20` bytes): per point `{ x: f32, y: f32,
//!   z: f32, length_m: f32, id: i32 }`.
//! - Optional extra-data array (`detail_count * 72` bytes = `18 * f32` per point),
//!   present in v7 files: includes curvature radius (`[4]`), `side_left` (`[5]`),
//!   `side_right` (`[6]`), camber (`[7]`), normal (`[9..12]`), normalized position
//!   (`[12]`), forward vector (`[13..16]`), and grade (`[17]`). Older/short files
//!   without the extra block still parse cleanly with `extra: None`.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// Maximum accepted header version.
const MAX_VERSION: i32 = 64;
/// Upper bound on spline point count to prevent unbounded allocations.
pub const MAX_POINTS: usize = 500_000;
/// Size of one ideal-line point in bytes (`4 * f32 + i32`).
const POINT_BYTES: usize = 20;
/// Size of one extra-data record in bytes (`18 * f32`).
const EXTRA_BYTES: usize = 72;

#[derive(Debug)]
pub enum SplineError {
    Io(io::Error),
    BadHeader(String),
    Truncated,
    NonFiniteData { index: usize },
}

impl fmt::Display for SplineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplineError::Io(e) => write!(f, "AI spline I/O error: {e}"),
            SplineError::BadHeader(msg) => write!(f, "invalid AI spline header: {msg}"),
            SplineError::Truncated => write!(f, "AI spline file is truncated"),
            SplineError::NonFiniteData { index } => {
                write!(f, "AI spline contains non-finite float at point {index}")
            }
        }
    }
}

impl Error for SplineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SplineError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for SplineError {
    fn from(e: io::Error) -> Self {
        SplineError::Io(e)
    }
}

/// One ideal-line vertex from `fast_lane.ai` / `pit_lane.ai`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplinePoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Cumulative distance from start/finish in metres.
    pub length_m: f32,
    pub id: i32,
}

/// Per-point v7 extra payload (`18 * f32`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplineExtra {
    pub speed_hint: f32,
    pub gas: f32,
    pub brake: f32,
    pub obsolete_lat_g: f32,
    /// Turning radius in metres (signed or unsigned; curvature is `1.0 / radius`).
    pub radius: f32,
    /// Distance from ideal line to left track edge, in metres.
    pub side_left: f32,
    /// Distance from ideal line to right track edge, in metres.
    pub side_right: f32,
    /// Track banking/camber in radians.
    pub camber: f32,
    pub direction: f32,
    pub normal: [f32; 3],
    /// Normalized spline coordinate `0.0..=1.0`.
    pub norm_pos: f32,
    pub forward: [f32; 3],
    pub tag: f32,
    pub grade: f32,
}

/// Parsed `fast_lane.ai` or `pit_lane.ai` spline.
#[derive(Debug, Clone, PartialEq)]
pub struct AiSpline {
    pub version: i32,
    pub lap_time_ms: i32,
    pub sample_count: i32,
    pub points: Vec<SplinePoint>,
    /// Present when the file carries the v7 `18 * f32` extra block; length matches `points`.
    pub extra: Option<Vec<SplineExtra>>,
}

/// Result of projecting a world `(x, z)` position onto an [`AiSpline`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplineProjection {
    /// Nearest spline point index.
    pub index: usize,
    /// Normalized track position in `0.0..=1.0`.
    pub norm_pos: f32,
    /// Cumulative distance along the spline in metres.
    pub distance_m: f32,
    /// Signed lateral offset from the ideal line in metres (positive = right, negative = left).
    pub lateral_offset_m: f32,
    /// Track edge width to the left at this point (metres), if extra data is present.
    pub side_left_m: Option<f32>,
    /// Track edge width to the right at this point (metres), if extra data is present.
    pub side_right_m: Option<f32>,
}

/// A corner automatically detected from spline curvature peaks.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerSegment {
    pub index: usize,
    pub start_idx: usize,
    pub apex_idx: usize,
    pub end_idx: usize,
    pub start_norm: f32,
    pub apex_norm: f32,
    pub end_norm: f32,
    pub peak_curvature: f32,
    /// True for a right-hand turn, false for left-hand.
    pub is_right: bool,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SplineError> {
        let end = self.pos.checked_add(n).ok_or(SplineError::Truncated)?;
        let slice = self.bytes.get(self.pos..end).ok_or(SplineError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn i32(&mut self) -> Result<i32, SplineError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, SplineError> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Parse an in-memory `fast_lane.ai` or `pit_lane.ai` buffer.
pub fn parse_ai_spline(bytes: &[u8]) -> Result<AiSpline, SplineError> {
    let mut cur = Cursor::new(bytes);
    let version = cur.i32()?;
    if version <= 0 || version > MAX_VERSION {
        return Err(SplineError::BadHeader(format!("unsupported version {version}")));
    }
    let detail_count_raw = cur.i32()?;
    if detail_count_raw <= 0 {
        return Err(SplineError::BadHeader(format!(
            "invalid detail_count {detail_count_raw}"
        )));
    }
    let detail_count = detail_count_raw as usize;
    if detail_count > MAX_POINTS {
        return Err(SplineError::BadHeader(format!(
            "detail_count {detail_count} exceeds cap {MAX_POINTS}"
        )));
    }
    let lap_time_ms = cur.i32()?;
    let sample_count = cur.i32()?;

    let needed_points = detail_count
        .checked_mul(POINT_BYTES)
        .ok_or(SplineError::Truncated)?;
    if cur.remaining() < needed_points {
        return Err(SplineError::Truncated);
    }

    let mut points = Vec::with_capacity(detail_count);
    for idx in 0..detail_count {
        let x = cur.f32()?;
        let y = cur.f32()?;
        let z = cur.f32()?;
        let length_m = cur.f32()?;
        let id = cur.i32()?;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !length_m.is_finite() {
            return Err(SplineError::NonFiniteData { index: idx });
        }
        points.push(SplinePoint {
            x,
            y,
            z,
            length_m,
            id,
        });
    }

    let needed_extra = detail_count
        .checked_mul(EXTRA_BYTES)
        .ok_or(SplineError::Truncated)?;
    let extra = if cur.remaining() >= needed_extra {
        let mut list = Vec::with_capacity(detail_count);
        for idx in 0..detail_count {
            let mut vals = [0.0f32; 18];
            for slot in &mut vals {
                *slot = cur.f32()?;
            }
            // Allow radius (vals[4]) to be infinite on dead-straight segments,
            // but require side widths and positions to be finite.
            if !vals[5].is_finite() || !vals[6].is_finite() || !vals[12].is_finite() {
                return Err(SplineError::NonFiniteData { index: idx });
            }
            list.push(SplineExtra {
                speed_hint: vals[0],
                gas: vals[1],
                brake: vals[2],
                obsolete_lat_g: vals[3],
                radius: vals[4],
                side_left: vals[5],
                side_right: vals[6],
                camber: vals[7],
                direction: vals[8],
                normal: [vals[9], vals[10], vals[11]],
                norm_pos: vals[12],
                forward: [vals[13], vals[14], vals[15]],
                tag: vals[16],
                grade: vals[17],
            });
        }
        Some(list)
    } else {
        None
    };

    Ok(AiSpline {
        version,
        lap_time_ms,
        sample_count,
        points,
        extra,
    })
}

/// Read and parse a `fast_lane.ai` or `pit_lane.ai` file from disk.
pub fn read_ai_spline(path: &Path) -> Result<AiSpline, SplineError> {
    let bytes = fs::read(path)?;
    parse_ai_spline(&bytes)
}

impl AiSpline {
    /// Total spline length in metres.
    pub fn total_length_m(&self) -> f32 {
        self.points.last().map_or(0.0, |p| p.length_m)
    }

    /// Unit horizontal tangent `(tx, tz)` at point `idx` in the XZ plane.
    pub fn tangent_xz(&self, idx: usize) -> [f32; 2] {
        let n = self.points.len();
        if n < 2 {
            return [1.0, 0.0];
        }
        let prev = &self.points[(idx + n - 1) % n];
        let next = &self.points[(idx + 1) % n];
        let dx = next.x - prev.x;
        let dz = next.z - prev.z;
        let len = dx.hypot(dz);
        if len > 1e-6 && len.is_finite() {
            [dx / len, dz / len]
        } else {
            [1.0, 0.0]
        }
    }

    /// Right-hand horizontal unit normal `(rx, rz)` in AC's Y-up right-handed XZ plane.
    /// With forward `(tx, tz)` and up `(0, 1, 0)`, `forward x up = (-tz, 0, tx)`.
    fn right_xz(&self, idx: usize) -> [f32; 2] {
        let [tx, tz] = self.tangent_xz(idx);
        [-tz, tx]
    }

    /// Computes `(left_edge, right_edge)` world-space 3D polylines using `side_left`
    /// and `side_right` from the v7 extra block (falling back to a default 5 m half-width
    /// when extra data is absent).
    pub fn corridor_edges(&self) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
        let n = self.points.len();
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        for (i, p) in self.points.iter().enumerate() {
            let [rx, rz] = self.right_xz(i);
            let (sl, sr) = match &self.extra {
                Some(ex) => (ex[i].side_left.max(0.0), ex[i].side_right.max(0.0)),
                None => (5.0, 5.0),
            };
            left.push([p.x - rx * sl, p.y, p.z - rz * sl]);
            right.push([p.x + rx * sr, p.y, p.z + rz * sr]);
        }
        (left, right)
    }

    /// Projects a world `(x, z)` coordinate onto the nearest spline point.
    pub fn project_world(&self, x: f32, z: f32) -> Option<SplineProjection> {
        if self.points.is_empty() || !x.is_finite() || !z.is_finite() {
            return None;
        }
        let mut best_idx = 0usize;
        let mut best_d2 = f32::INFINITY;
        for (i, p) in self.points.iter().enumerate() {
            let dx = x - p.x;
            let dz = z - p.z;
            let d2 = dx * dx + dz * dz;
            if d2 < best_d2 {
                best_d2 = d2;
                best_idx = i;
            }
        }
        let p = &self.points[best_idx];
        let [rx, rz] = self.right_xz(best_idx);
        let lateral_offset_m = (x - p.x) * rx + (z - p.z) * rz;
        let total = self.total_length_m();
        let norm_pos = match &self.extra {
            Some(ex) if ex[best_idx].norm_pos.is_finite() && ex[best_idx].norm_pos > 0.0 => {
                ex[best_idx].norm_pos.clamp(0.0, 1.0)
            }
            _ if total > 0.0 => (p.length_m / total).clamp(0.0, 1.0),
            _ => best_idx as f32 / self.points.len().max(1) as f32,
        };
        let (side_left_m, side_right_m) = match &self.extra {
            Some(ex) => (Some(ex[best_idx].side_left), Some(ex[best_idx].side_right)),
            None => (None, None),
        };
        Some(SplineProjection {
            index: best_idx,
            norm_pos,
            distance_m: p.length_m,
            lateral_offset_m,
            side_left_m,
            side_right_m,
        })
    }

    /// Computes signed horizontal curvature (1/m) at every point from consecutive tangents.
    pub fn signed_curvatures(&self) -> Vec<f32> {
        let n = self.points.len();
        if n < 3 {
            return vec![0.0; n];
        }
        let mut out = vec![0.0f32; n];
        for i in 0..n {
            let prev = &self.points[(i + n - 1) % n];
            let curr = &self.points[i];
            let next = &self.points[(i + 1) % n];
            let v1x = curr.x - prev.x;
            let v1z = curr.z - prev.z;
            let v2x = next.x - curr.x;
            let v2z = next.z - curr.z;
            let l1 = v1x.hypot(v1z);
            let l2 = v2x.hypot(v2z);
            if l1 < 1e-4 || l2 < 1e-4 {
                continue;
            }
            // Signed angle change divided by average segment length.
            let cross = (v1x * v2z - v1z * v2x) / (l1 * l2);
            let dot = ((v1x * v2x + v1z * v2z) / (l1 * l2)).clamp(-1.0, 1.0);
            let dtheta = cross.atan2(dot);
            let ds = 0.5 * (l1 + l2);
            out[i] = dtheta / ds;
        }
        out
    }

    /// Detects contiguous corners where `|curvature| >= min_curvature` over at least `min_len_m`.
    pub fn detect_corners(&self, min_curvature: f32, min_len_m: f32) -> Vec<CornerSegment> {
        let n = self.points.len();
        if n < 5 {
            return Vec::new();
        }
        let curv = self.signed_curvatures();
        let total = self.total_length_m().max(1.0);
        let mut corners = Vec::new();
        let mut i = 0usize;
        while i < n {
            if curv[i].abs() < min_curvature {
                i += 1;
                continue;
            }
            let start = i;
            let sign = curv[i].signum();
            let mut apex = i;
            let mut peak = curv[i].abs();
            while i < n && curv[i].abs() >= min_curvature && curv[i].signum() == sign {
                if curv[i].abs() > peak {
                    peak = curv[i].abs();
                    apex = i;
                }
                i += 1;
            }
            let end = i.saturating_sub(1);
            let arc_len = (self.points[end].length_m - self.points[start].length_m).abs();
            if arc_len >= min_len_m {
                corners.push(CornerSegment {
                    index: corners.len() + 1,
                    start_idx: start,
                    apex_idx: apex,
                    end_idx: end,
                    start_norm: (self.points[start].length_m / total).clamp(0.0, 1.0),
                    apex_norm: (self.points[apex].length_m / total).clamp(0.0, 1.0),
                    end_norm: (self.points[end].length_m / total).clamp(0.0, 1.0),
                    peak_curvature: peak,
                    is_right: sign > 0.0,
                });
            }
        }
        corners
    }
}

/// Builds a valid v7 `fast_lane.ai` binary buffer (useful for tests and generators).
pub fn encode_ai_spline(spline: &AiSpline) -> Vec<u8> {
    let n = spline.points.len();
    let extra_bytes = if spline.extra.is_some() {
        n * EXTRA_BYTES
    } else {
        0
    };
    let mut out = Vec::with_capacity(16 + n * POINT_BYTES + extra_bytes);
    out.extend_from_slice(&spline.version.to_le_bytes());
    out.extend_from_slice(&(n as i32).to_le_bytes());
    out.extend_from_slice(&spline.lap_time_ms.to_le_bytes());
    out.extend_from_slice(&spline.sample_count.to_le_bytes());
    for p in &spline.points {
        out.extend_from_slice(&p.x.to_le_bytes());
        out.extend_from_slice(&p.y.to_le_bytes());
        out.extend_from_slice(&p.z.to_le_bytes());
        out.extend_from_slice(&p.length_m.to_le_bytes());
        out.extend_from_slice(&p.id.to_le_bytes());
    }
    if let Some(extra) = &spline.extra {
        for e in extra {
            let vals = [
                e.speed_hint,
                e.gas,
                e.brake,
                e.obsolete_lat_g,
                e.radius,
                e.side_left,
                e.side_right,
                e.camber,
                e.direction,
                e.normal[0],
                e.normal[1],
                e.normal[2],
                e.norm_pos,
                e.forward[0],
                e.forward[1],
                e.forward[2],
                e.tag,
                e.grade,
            ];
            for v in vals {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oval_spline(with_extra: bool) -> AiSpline {
        let count = 120usize;
        let radius = 100.0f32;
        let mut points = Vec::with_capacity(count);
        let mut extra = Vec::with_capacity(count);
        let mut dist = 0.0f32;
        for i in 0..count {
            let t = std::f32::consts::TAU * (i as f32) / (count as f32);
            let x = radius * t.cos();
            let z = radius * t.sin();
            if i > 0 {
                let prev: &SplinePoint = &points[i - 1];
                dist += (x - prev.x).hypot(z - prev.z);
            }
            points.push(SplinePoint {
                x,
                y: 5.0 * t.sin(),
                z,
                length_m: dist,
                id: i as i32,
            });
            extra.push(SplineExtra {
                speed_hint: 50.0,
                gas: 1.0,
                brake: 0.0,
                obsolete_lat_g: 0.0,
                radius,
                side_left: 6.0,
                side_right: 4.0,
                camber: 0.02,
                direction: t,
                normal: [0.0, 1.0, 0.0],
                norm_pos: i as f32 / count as f32,
                forward: [-t.sin(), 0.0, t.cos()],
                tag: 0.0,
                grade: 0.0,
            });
        }
        AiSpline {
            version: if with_extra { 7 } else { 6 },
            lap_time_ms: 42_500,
            sample_count: count as i32,
            points,
            extra: with_extra.then_some(extra),
        }
    }

    #[test]
    fn v7_round_trip_and_corridor_width() {
        let orig = oval_spline(true);
        let bytes = encode_ai_spline(&orig);
        let parsed = parse_ai_spline(&bytes).unwrap();
        assert_eq!(parsed, orig);

        let (left, right) = parsed.corridor_edges();
        assert_eq!(left.len(), parsed.points.len());
        assert_eq!(right.len(), parsed.points.len());
        for i in 0..left.len() {
            let width = (right[i][0] - left[i][0]).hypot(right[i][2] - left[i][2]);
            assert!((width - 10.0).abs() < 1e-3, "corridor width at {i} was {width}");
        }
    }

    #[test]
    fn v6_without_extra_parses_cleanly() {
        let orig = oval_spline(false);
        let bytes = encode_ai_spline(&orig);
        let parsed = parse_ai_spline(&bytes).unwrap();
        assert_eq!(parsed.version, 6);
        assert!(parsed.extra.is_none());
        assert_eq!(parsed.points.len(), orig.points.len());
    }

    #[test]
    fn project_world_computes_signed_lateral_offset() {
        let spline = oval_spline(true);
        let (left, right) = spline.corridor_edges();
        let proj_l = spline.project_world(left[10][0], left[10][2]).unwrap();
        let proj_r = spline.project_world(right[10][0], right[10][2]).unwrap();
        assert!((proj_l.lateral_offset_m + 6.0).abs() < 0.25, "got {}", proj_l.lateral_offset_m);
        assert!((proj_r.lateral_offset_m - 4.0).abs() < 0.25, "got {}", proj_r.lateral_offset_m);
        assert_eq!(proj_l.side_left_m, Some(6.0));
        assert_eq!(proj_r.side_right_m, Some(4.0));
    }

    #[test]
    fn detect_corners_finds_hairpin_between_straights() {
        let mut points = Vec::new();
        let mut dist = 0.0f32;
        // 200m straight along +X
        for i in 0..40 {
            let x = i as f32 * 5.0;
            if i > 0 {
                dist += 5.0;
            }
            points.push(SplinePoint { x, y: 0.0, z: 0.0, length_m: dist, id: i });
        }
        // 180-degree hairpin of radius 20m (curvature 0.05)
        let cx = 200.0f32;
        let cz = 20.0f32;
        for step in 1..=20 {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * (step as f32 / 20.0);
            let x = cx + 20.0 * a.cos();
            let z = cz + 20.0 * a.sin();
            let prev = points.last().unwrap();
            dist += (x - prev.x).hypot(z - prev.z);
            points.push(SplinePoint { x, y: 0.0, z, length_m: dist, id: 40 + step });
        }
        // Return straight along -X
        for step in 1..=40 {
            let x = 200.0 - step as f32 * 5.0;
            dist += 5.0;
            points.push(SplinePoint { x, y: 0.0, z: 40.0, length_m: dist, id: 60 + step });
        }
        let spline = AiSpline {
            version: 7,
            lap_time_ms: 20_000,
            sample_count: points.len() as i32,
            points,
            extra: None,
        };
        let corners = spline.detect_corners(0.02, 20.0);
        assert_eq!(corners.len(), 1, "expected 1 hairpin, got {corners:?}");
        assert!(corners[0].peak_curvature > 0.04);
    }

    #[test]
    fn rejects_bad_headers_and_truncated_buffers() {
        assert!(matches!(parse_ai_spline(&[]), Err(SplineError::Truncated)));
        let mut bad = vec![0u8; 16];
        bad[0..4].copy_from_slice(&(-1i32).to_le_bytes());
        assert!(matches!(parse_ai_spline(&bad), Err(SplineError::BadHeader(_))));
        bad[0..4].copy_from_slice(&7i32.to_le_bytes());
        bad[4..8].copy_from_slice(&(-5i32).to_le_bytes());
        assert!(matches!(parse_ai_spline(&bad), Err(SplineError::BadHeader(_))));
    }
}
