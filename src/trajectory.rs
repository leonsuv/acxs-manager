//! Trajectory & racing-line analysis over an [`AiSpline`] track corridor.
//!
//! Accepts self-contained [`TrajectoryLap`] samples so callers can feed laps
//! from `.acreplay` files or 30 Hz recorded sessions without tight coupling.

use crate::ai_spline::{AiSpline, CornerSegment};
use crate::delta;
use crate::track_files::TrackSection;

/// One sample point along a driven lap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrajectoryPoint {
    pub x: f32,
    pub z: f32,
    pub speed_kmh: f32,
    pub throttle: f32,
    pub brake: f32,
    pub gear: u8,
    pub elapsed_ms: f64,
}

/// A complete or partial driven lap.
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryLap {
    pub label: String,
    pub points: Vec<TrajectoryPoint>,
}

/// A [`TrajectoryPoint`] projected onto an [`AiSpline`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPoint {
    pub point: TrajectoryPoint,
    pub norm_pos: f32,
    pub distance_m: f32,
    pub lateral_offset_m: f32,
    /// Time delta vs the reference lap at this distance (ms; positive = slower).
    pub delta_ms: Option<f64>,
}

/// Driving metrics for one corner segment.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerMetrics {
    pub name: String,
    pub start_norm: f32,
    pub end_norm: f32,
    /// Normalized position where braking first exceeds 25% in the corner entry.
    pub braking_norm: Option<f32>,
    /// Minimum speed reached inside the corner (km/h).
    pub min_speed_kmh: f32,
    /// Closest lateral distance to the inside track edge at the apex (metres, smaller = tighter).
    pub apex_margin_m: Option<f32>,
    /// Normalized position where throttle exceeds 80% on corner exit.
    pub throttle_on_norm: Option<f32>,
    /// Time lost (+) or gained (-) in this corner vs the reference lap, in ms.
    pub time_delta_ms: Option<f64>,
}

/// Full comparison of a target lap against an optional reference lap over an [`AiSpline`].
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryAnalysis {
    pub projected: Vec<ProjectedPoint>,
    pub reference_projected: Vec<ProjectedPoint>,
    pub corners: Vec<CornerMetrics>,
    pub total_delta_ms: Option<f64>,
}

/// Builds a monotonic [`delta::Lap`] from projected points for distance interpolation.
fn to_delta_lap(projected: &[ProjectedPoint]) -> Option<delta::Lap> {
    let mut samples = Vec::with_capacity(projected.len());
    let mut last_dist = f64::NEG_INFINITY;
    for p in projected {
        let d = f64::from(p.distance_m);
        if d.is_finite() && p.point.elapsed_ms.is_finite() && d >= last_dist {
            samples.push(delta::Sample {
                distance_m: d,
                time_ms: p.point.elapsed_ms,
            });
            last_dist = d;
        }
    }
    delta::Lap::new(samples).ok()
}

/// Projects every point of `lap` onto `spline`.
pub fn project_lap(spline: &AiSpline, lap: &TrajectoryLap) -> Vec<ProjectedPoint> {
    lap.points
        .iter()
        .filter_map(|&point| {
            let proj = spline.project_world(point.x, point.z)?;
            Some(ProjectedPoint {
                point,
                norm_pos: proj.norm_pos,
                distance_m: proj.distance_m,
                lateral_offset_m: proj.lateral_offset_m,
                delta_ms: None,
            })
        })
        .collect()
}

/// Compares `target` against `reference` (optional) across `spline`, segmenting corners
/// from `sections` (if non-empty) or automatically via [`AiSpline::detect_corners`].
pub fn analyze_trajectory(
    spline: &AiSpline,
    target: &TrajectoryLap,
    reference: Option<&TrajectoryLap>,
    sections: &[TrackSection],
) -> TrajectoryAnalysis {
    let mut projected = project_lap(spline, target);
    let reference_projected = reference
        .map(|r| project_lap(spline, r))
        .unwrap_or_default();
    let ref_delta_lap = to_delta_lap(&reference_projected);

    if let Some(ref_lap) = &ref_delta_lap {
        for p in &mut projected {
            if let Some(ref_t) = delta::time_at_distance(ref_lap, f64::from(p.distance_m)) {
                p.delta_ms = Some(p.point.elapsed_ms - ref_t);
            }
        }
    }

    let total_delta_ms = projected.last().and_then(|p| p.delta_ms);

    // Build corner ranges either from `sections.ini` or auto-detected curvature peaks.
    let ranges: Vec<(String, f32, f32, Option<bool>)> = if !sections.is_empty() {
        sections
            .iter()
            .map(|s| (s.name.clone(), s.in_point, s.out_point, None))
            .collect()
    } else {
        spline
            .detect_corners(0.008, 15.0)
            .into_iter()
            .map(|c: CornerSegment| {
                (
                    format!("Turn {}", c.index),
                    c.start_norm,
                    c.end_norm,
                    Some(c.is_right),
                )
            })
            .collect()
    };

    let mut corners = Vec::with_capacity(ranges.len());
    for (name, start_norm, end_norm, is_right) in ranges {
        let slice: Vec<&ProjectedPoint> = projected
            .iter()
            .filter(|p| in_norm_range(p.norm_pos, start_norm, end_norm))
            .collect();
        if slice.is_empty() {
            continue;
        }
        let braking_norm = slice
            .iter()
            .find(|p| p.point.brake >= 0.25)
            .map(|p| p.norm_pos);
        let mut min_speed = f32::INFINITY;
        let mut apex_idx = 0usize;
        for (i, p) in slice.iter().enumerate() {
            if p.point.speed_kmh < min_speed {
                min_speed = p.point.speed_kmh;
                apex_idx = i;
            }
        }
        let throttle_on_norm = slice[apex_idx..]
            .iter()
            .find(|p| p.point.throttle >= 0.80)
            .map(|p| p.norm_pos);
        let apex_margin_m = spline
            .project_world(slice[apex_idx].point.x, slice[apex_idx].point.z)
            .and_then(|proj| match (is_right, proj.side_left_m, proj.side_right_m) {
                (Some(true), _, Some(sr)) => Some((sr - proj.lateral_offset_m).max(0.0)),
                (Some(false), Some(sl), _) => Some((sl + proj.lateral_offset_m).max(0.0)),
                (_, Some(sl), Some(sr)) => {
                    Some((sr - proj.lateral_offset_m).min(sl + proj.lateral_offset_m).max(0.0))
                }
                _ => None,
            });
        let time_delta_ms = match (slice.first().and_then(|p| p.delta_ms), slice.last().and_then(|p| p.delta_ms)) {
            (Some(d0), Some(d1)) => Some(d1 - d0),
            _ => None,
        };
        corners.push(CornerMetrics {
            name,
            start_norm,
            end_norm,
            braking_norm,
            min_speed_kmh: if min_speed.is_finite() { min_speed } else { 0.0 },
            apex_margin_m,
            throttle_on_norm,
            time_delta_ms,
        });
    }

    TrajectoryAnalysis {
        projected,
        reference_projected,
        corners,
        total_delta_ms,
    }
}

fn in_norm_range(pos: f32, start: f32, end: f32) -> bool {
    if start <= end {
        pos >= start && pos <= end
    } else {
        pos >= start || pos <= end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_spline::{SplineExtra, SplinePoint};

    fn test_spline() -> AiSpline {
        let mut points = Vec::new();
        let mut extra = Vec::new();
        for i in 0..100 {
            let x = i as f32 * 5.0;
            points.push(SplinePoint {
                x,
                y: 0.0,
                z: 0.0,
                length_m: x,
                id: i,
            });
            extra.push(SplineExtra {
                speed_hint: 50.0,
                gas: 1.0,
                brake: 0.0,
                obsolete_lat_g: 0.0,
                radius: 1000.0,
                side_left: 5.0,
                side_right: 5.0,
                camber: 0.0,
                direction: 0.0,
                normal: [0.0, 1.0, 0.0],
                norm_pos: i as f32 / 100.0,
                forward: [1.0, 0.0, 0.0],
                tag: 0.0,
                grade: 0.0,
            });
        }
        AiSpline {
            version: 7,
            lap_time_ms: 20_000,
            sample_count: 100,
            points,
            extra: Some(extra),
        }
    }

    #[test]
    fn computes_delta_and_corner_metrics_against_reference() {
        let spline = test_spline();
        let make_lap = |label: &str, slow_factor: f64| TrajectoryLap {
            label: label.into(),
            points: (0..100)
                .map(|i| TrajectoryPoint {
                    x: i as f32 * 5.0,
                    z: 1.0,
                    speed_kmh: if (40..60).contains(&i) { 80.0 } else { 180.0 },
                    throttle: if (35..50).contains(&i) { 0.0 } else { 1.0 },
                    brake: if (35..45).contains(&i) { 0.9 } else { 0.0 },
                    gear: 3,
                    elapsed_ms: i as f64 * 100.0 * slow_factor,
                })
                .collect(),
        };
        let sections = vec![TrackSection {
            in_point: 0.30,
            out_point: 0.65,
            name: "Chicane".into(),
        }];
        let analysis = analyze_trajectory(
            &spline,
            &make_lap("Current", 1.1),
            Some(&make_lap("PB", 1.0)),
            &sections,
        );
        assert_eq!(analysis.corners.len(), 1);
        let c = &analysis.corners[0];
        assert_eq!(c.name, "Chicane");
        assert!(c.braking_norm.is_some());
        assert_eq!(c.min_speed_kmh, 80.0);
        assert!(c.throttle_on_norm.is_some());
        assert!(c.time_delta_ms.unwrap() > 300.0);
        assert!(analysis.total_delta_ms.unwrap() > 900.0);
    }
}
