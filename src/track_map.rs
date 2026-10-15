//! Geometry for the track map and the friction circle, kept free of UI types.
//!
//! The map turns a lap's world positions into points in the unit square. The
//! friction circle turns a lateral and longitudinal g-force pair into points in
//! the unit circle. Callers scale both to their own drawing rectangle.

/// Number of straight segments that make up the friction circle outline.
const CIRCLE_SEGMENTS: usize = 64;

/// Projects a lap's world positions onto the unit square `[0, 1] x [0, 1]`.
///
/// Each input is `[x, z]` in world units. The bounding box keeps its aspect
/// ratio: its longer side spans the full unit, and the shorter side is centred.
/// Order is preserved, so the result can be drawn as a polyline. Non-finite
/// positions are skipped. A single point, or points that all coincide, map to
/// the centre `[0.5, 0.5]`. Returns an empty vector when no position is finite.
pub fn project_lap(positions: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let finite: Vec<[f32; 2]> = positions
        .iter()
        .copied()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
        .collect();
    let Some(&first) = finite.first() else {
        return Vec::new();
    };

    let (mut min_x, mut max_x) = (first[0], first[0]);
    let (mut min_z, mut max_z) = (first[1], first[1]);
    for p in &finite[1..] {
        min_x = min_x.min(p[0]);
        max_x = max_x.max(p[0]);
        min_z = min_z.min(p[1]);
        max_z = max_z.max(p[1]);
    }

    let span = (max_x - min_x).max(max_z - min_z);
    if !span.is_finite() || span <= 0.0 {
        return vec![[0.5, 0.5]; finite.len()];
    }

    let scale = 1.0 / span;
    let offset_x = (1.0 - (max_x - min_x) * scale) * 0.5;
    let offset_z = (1.0 - (max_z - min_z) * scale) * 0.5;
    finite
        .iter()
        .map(|p| {
            [
                offset_x + (p[0] - min_x) * scale,
                offset_z + (p[1] - min_z) * scale,
            ]
        })
        .collect()
}

/// Outline of the friction circle at radius 1, as a closed polyline.
///
/// The first and last points are equal, so the outline can be drawn with a
/// single line call.
pub fn friction_circle_outline() -> Vec<[f32; 2]> {
    let mut points: Vec<[f32; 2]> = (0..CIRCLE_SEGMENTS)
        .map(|i| {
            let angle = std::f32::consts::TAU * i as f32 / CIRCLE_SEGMENTS as f32;
            [angle.cos(), angle.sin()]
        })
        .collect();
    // Copy the first point rather than recomputing it, so the loop closes exactly.
    points.push(points[0]);
    points
}

/// Points for drawing a friction circle with a current-state marker.
///
/// `lat_g` is lateral and `long_g` is longitudinal acceleration in g. `max_g` is
/// the radius of the circle in g. The result is the outline from
/// [`friction_circle_outline`], followed by the marker as the last element.
/// The marker is `(lat_g, long_g) / max_g`. If that lies outside the circle, it
/// is pulled back onto the circle edge.
///
/// Returns an empty vector when any input is not finite, or when `max_g` is not
/// greater than zero.
pub fn friction_circle_points(lat_g: f32, long_g: f32, max_g: f32) -> Vec<[f32; 2]> {
    if !lat_g.is_finite() || !long_g.is_finite() || !max_g.is_finite() || max_g <= 0.0 {
        return Vec::new();
    }

    let mut x = lat_g / max_g;
    let mut y = long_g / max_g;
    let magnitude = x.hypot(y);
    if magnitude > 1.0 {
        x /= magnitude;
        y /= magnitude;
    }

    let mut points = friction_circle_outline();
    points.push([x, y]);
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: [f32; 2], b: [f32; 2]) {
        assert!(
            (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn project_empty_or_all_non_finite_is_empty() {
        assert!(project_lap(&[]).is_empty());
        assert!(project_lap(&[[f32::NAN, 0.0], [1.0, f32::INFINITY]]).is_empty());
    }

    #[test]
    fn project_single_or_stationary_point_is_centred() {
        assert_eq!(project_lap(&[[7.0, -3.0]]), vec![[0.5, 0.5]]);
        assert_eq!(
            project_lap(&[[2.0, 2.0], [2.0, 2.0]]),
            vec![[0.5, 0.5], [0.5, 0.5]]
        );
    }

    #[test]
    fn project_keeps_aspect_ratio_and_centres_short_side() {
        // Extent is 10 wide and 5 tall, so x spans 0..1 and z spans 0.25..0.75.
        let out = project_lap(&[[0.0, 0.0], [10.0, 5.0]]);
        assert_close(out[0], [0.0, 0.25]);
        assert_close(out[1], [1.0, 0.75]);
    }

    #[test]
    fn project_output_is_inside_unit_square_and_ordered() {
        let input: Vec<[f32; 2]> = (0..50)
            .map(|i| {
                let a = i as f32 * 0.3;
                [-400.0 + 120.0 * a.cos(), 80.0 + 40.0 * a.sin()]
            })
            .collect();
        let out = project_lap(&input);
        assert_eq!(out.len(), input.len());
        for p in &out {
            assert!((0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1]), "{p:?}");
        }
        // The projection is affine, so the first and last input points keep their
        // positions relative to each other in both axes.
        let (a, b) = (out[0], out[out.len() - 1]);
        let (ia, ib) = (input[0], input[input.len() - 1]);
        assert_eq!(a[0] < b[0], ia[0] < ib[0]);
        assert_eq!(a[1] < b[1], ia[1] < ib[1]);
    }

    #[test]
    fn project_skips_non_finite_points_without_shifting_the_rest() {
        let with_gap = project_lap(&[[0.0, 0.0], [f32::NAN, 9.0], [10.0, 0.0]]);
        let without = project_lap(&[[0.0, 0.0], [10.0, 0.0]]);
        assert_eq!(with_gap, without);
    }

    #[test]
    fn friction_circle_invalid_input_is_empty() {
        assert!(friction_circle_points(f32::NAN, 0.0, 1.5).is_empty());
        assert!(friction_circle_points(0.0, f32::INFINITY, 1.5).is_empty());
        assert!(friction_circle_points(0.0, 0.0, 0.0).is_empty());
        assert!(friction_circle_points(0.0, 0.0, -2.0).is_empty());
        assert!(friction_circle_points(0.0, 0.0, f32::NAN).is_empty());
    }

    #[test]
    fn friction_outline_is_closed_on_the_unit_circle() {
        let outline = friction_circle_outline();
        assert_eq!(outline.first(), outline.last());
        for p in &outline {
            let r = p[0].hypot(p[1]);
            assert!((r - 1.0).abs() < 1e-5, "{p:?}");
        }
    }

    #[test]
    fn friction_marker_is_scaled_by_max_g_and_comes_last() {
        let points = friction_circle_points(0.5, -0.25, 1.0);
        assert_eq!(points.len(), friction_circle_outline().len() + 1);
        assert_close(*points.last().unwrap(), [0.5, -0.25]);
    }

    #[test]
    fn friction_marker_outside_the_circle_is_pulled_to_the_edge() {
        let points = friction_circle_points(3.0, 4.0, 1.0);
        let marker = *points.last().unwrap();
        assert!((marker[0].hypot(marker[1]) - 1.0).abs() < 1e-5);
        assert_close(marker, [0.6, 0.8]);
    }
}
