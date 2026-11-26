//! Corner-by-corner driving coach & braking-point analyzer.
//!
//! Segments corners deterministically from a 200-bin [`LapTrace`] (`|lat_g| >= 0.45`),
//! compares a current lap trace against a reference PB trace, and joins with
//! [`RecordedSession`] hotspots (front/rear lockups, wheelspins, understeer, oversteer)
//! to emit actionable corner-specific coaching tips.

use serde::{Deserialize, Serialize};

use crate::session_recorder::{EventKind, Hotspot, LapTrace, SEGMENTS};

/// Lateral-g threshold to classify a bin as cornering.
const CORNER_LAT_G: f32 = 0.45;
/// Minimum consecutive bins to count as a corner.
const MIN_CORNER_BINS: usize = 3;
/// Merge two corner segments separated by at most this many bins.
const MERGE_GAP_BINS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CornerSegment {
    /// 1-based corner number around the lap.
    pub number: usize,
    pub start_bin: usize,
    pub apex_bin: usize,
    pub end_bin: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CornerCoaching {
    pub corner: CornerSegment,
    /// Time lost (positive) or gained (negative) through this corner vs PB, in ms.
    pub time_delta_ms: f32,
    /// Braking point delta in meters: negative = braked earlier than PB, positive = braked later.
    pub brake_delta_m: Option<f32>,
    /// Minimum apex speed delta (current - PB) in km/h.
    pub apex_speed_delta_kmh: f32,
    /// Throttle pickup delta in meters: positive = picked up throttle later than PB.
    pub throttle_delta_m: Option<f32>,
    /// Correlated trouble-spot counts inside this corner's normalized track slice.
    pub front_lockups: u32,
    pub rear_lockups: u32,
    pub wheelspins: u32,
    pub understeer_tenths: u32,
    pub oversteer_tenths: u32,
    /// Concrete, human-readable coaching sentence.
    pub advice: String,
}

/// Detects distinct corners from a reference [`LapTrace`].
pub fn segment_corners(trace: &LapTrace) -> Vec<CornerSegment> {
    let n = trace.bins.len();
    let mut raw: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;

    for i in 0..n {
        let active = trace.bins[i]
            .as_ref()
            .is_some_and(|b| b.lat_g.abs() >= CORNER_LAT_G);
        match (start, active) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                if i - s >= MIN_CORNER_BINS {
                    raw.push((s, i - 1));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start && n - s >= MIN_CORNER_BINS {
        raw.push((s, n - 1));
    }

    // Merge close segments.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in raw {
        if let Some(last) = merged.last_mut() && s <= last.1 + MERGE_GAP_BINS + 1 {
            last.1 = e;
        } else {
            merged.push((s, e));
        }
    }

    merged
        .into_iter()
        .enumerate()
        .map(|(idx, (s, e))| {
            let apex_bin = (s..=e)
                .filter_map(|b| trace.bins[b].as_ref().map(|bin| (b, bin.speed_kmh)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map_or((s + e) / 2, |(b, _)| b);
            CornerSegment {
                number: idx + 1,
                start_bin: s,
                apex_bin,
                end_bin: e,
            }
        })
        .collect()
}

fn find_brake_onset(trace: &LapTrace, corner_start: usize, lookback: usize) -> Option<usize> {
    let from = corner_start.saturating_sub(lookback);
    (from..=corner_start).find(|&i| trace.bins.get(i).and_then(|b| b.as_ref()).is_some_and(|b| b.brake >= 0.25))
}

fn find_throttle_pickup(trace: &LapTrace, apex: usize, corner_end: usize) -> Option<usize> {
    let limit = (corner_end + 4).min(trace.bins.len().saturating_sub(1));
    (apex..=limit).find(|&i| trace.bins.get(i).and_then(|b| b.as_ref()).is_some_and(|b| b.throttle >= 0.6))
}

/// Compares `current` against `reference_pb` corner-by-corner and correlates `hotspots`.
pub fn analyze_corners(
    reference_pb: &LapTrace,
    current: &LapTrace,
    hotspots: &[Hotspot],
) -> Vec<CornerCoaching> {
    let corners = segment_corners(reference_pb);
    let n = reference_pb.bins.len().max(1);
    let meters_per_bin = if reference_pb.track_length_m > 0.0 {
        reference_pb.track_length_m / n as f32
    } else {
        25.0
    };

    let mut out = Vec::new();
    for corner in corners {
        let pb_start = reference_pb.bins.get(corner.start_bin).and_then(|b| b.as_ref());
        let pb_end = reference_pb.bins.get(corner.end_bin).and_then(|b| b.as_ref());
        let cur_start = current.bins.get(corner.start_bin).and_then(|b| b.as_ref());
        let cur_end = current.bins.get(corner.end_bin).and_then(|b| b.as_ref());

        let (Some(ps), Some(pe), Some(cs), Some(ce)) = (pb_start, pb_end, cur_start, cur_end) else {
            continue;
        };
        let pb_dur = pe.elapsed_ms - ps.elapsed_ms;
        let cur_dur = ce.elapsed_ms - cs.elapsed_ms;
        let time_delta_ms = cur_dur - pb_dur;

        let min_speed = |t: &LapTrace| -> f32 {
            (corner.start_bin..=corner.end_bin)
                .filter_map(|b| t.bins.get(b).and_then(|x| x.as_ref()).map(|x| x.speed_kmh))
                .fold(f32::INFINITY, f32::min)
        };
        let pb_min = min_speed(reference_pb);
        let cur_min = min_speed(current);
        let apex_speed_delta_kmh = if pb_min.is_finite() && cur_min.is_finite() {
            cur_min - pb_min
        } else {
            0.0
        };

        let pb_brake = find_brake_onset(reference_pb, corner.start_bin, 8);
        let cur_brake = find_brake_onset(current, corner.start_bin, 8);
        let brake_delta_m = match (pb_brake, cur_brake) {
            (Some(pb_b), Some(cur_b)) => Some((cur_b as f32 - pb_b as f32) * meters_per_bin),
            _ => None,
        };

        let pb_thr = find_throttle_pickup(reference_pb, corner.apex_bin, corner.end_bin);
        let cur_thr = find_throttle_pickup(current, corner.apex_bin, corner.end_bin);
        let throttle_delta_m = match (pb_thr, cur_thr) {
            (Some(pb_t), Some(cur_t)) => Some((cur_t as f32 - pb_t as f32) * meters_per_bin),
            _ => None,
        };

        // Map corner bin span to `SEGMENTS` (0..100) for hotspot correlation.
        let seg_lo = (corner.start_bin.saturating_sub(4) * SEGMENTS / n) as u8;
        let seg_hi = ((corner.end_bin + 4).min(n) * SEGMENTS / n) as u8;
        let mut front_lockups = 0;
        let mut rear_lockups = 0;
        let mut wheelspins = 0;
        let mut understeer_tenths = 0;
        let mut oversteer_tenths = 0;
        for h in hotspots {
            if h.segment >= seg_lo && h.segment <= seg_hi {
                match h.kind {
                    EventKind::FrontLockup => front_lockups += h.count,
                    EventKind::RearLockup => rear_lockups += h.count,
                    EventKind::Wheelspin => wheelspins += h.count,
                    EventKind::Understeer => understeer_tenths += h.count,
                    EventKind::Oversteer => oversteer_tenths += h.count,
                    _ => {}
                }
            }
        }

        let mut tips = Vec::new();
        if let Some(bd) = brake_delta_m && bd <= -5.0 {
            tips.push(format!("braking {:.0}m early vs PB", bd.abs()));
        } else if let Some(bd) = brake_delta_m && bd >= 5.0 {
            tips.push(format!("braking {:.0}m late vs PB", bd));
        }
        if apex_speed_delta_kmh <= -3.0 {
            tips.push(format!("apex {:.1} km/h slower", apex_speed_delta_kmh.abs()));
        }
        if let Some(td) = throttle_delta_m && td >= 5.0 {
            tips.push(format!("throttle {:.0}m late on exit", td));
        }
        if front_lockups > 0 {
            tips.push(format!("{front_lockups} front lockup(s) — try shifting brake bias rear"));
        } else if rear_lockups > 0 {
            tips.push(format!("{rear_lockups} rear lockup(s) — try shifting brake bias forward"));
        }
        if wheelspins > 0 {
            tips.push(format!("{wheelspins} wheelspin(s) on exit"));
        }
        if understeer_tenths >= 5 {
            tips.push("persistent understeer — trail brake deeper or soften front ARB".into());
        } else if oversteer_tenths >= 5 {
            tips.push("exit oversteer — smooth throttle or soften rear ARB".into());
        }

        let advice = if tips.is_empty() {
            format!("Turn {}: on pace ({:+.2}s vs PB).", corner.number, time_delta_ms / 1000.0)
        } else {
            format!(
                "Turn {} ({:+.2}s): {}.",
                corner.number,
                time_delta_ms / 1000.0,
                tips.join("; ")
            )
        };

        out.push(CornerCoaching {
            corner,
            time_delta_ms,
            brake_delta_m,
            apex_speed_delta_kmh,
            throttle_delta_m,
            front_lockups,
            rear_lockups,
            wheelspins,
            understeer_tenths,
            oversteer_tenths,
            advice,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_recorder::{LapTraceBin, TRACE_BINS};

    fn synthetic_trace(brake_bin: usize, apex_speed: f32, extra_corner_ms: f32) -> LapTrace {
        let mut bins = Vec::with_capacity(TRACE_BINS);
        let mut t = 0.0_f32;
        for i in 0..TRACE_BINS {
            let in_corner = (30..=40).contains(&i);
            let dt = if in_corner { 120.0 + extra_corner_ms / 11.0 } else { 100.0 };
            t += dt;
            bins.push(Some(LapTraceBin {
                elapsed_ms: t,
                speed_kmh: if i == 35 { apex_speed } else if in_corner { apex_speed + 15.0 } else { 200.0 },
                throttle: if i >= 37 { 0.9 } else if i >= brake_bin && i < 35 { 0.0 } else { 1.0 },
                brake: if i >= brake_bin && i < 34 { 0.85 } else { 0.0 },
                steer: if in_corner { 0.35 } else { 0.0 },
                lat_g: if in_corner { 1.4 } else { 0.05 },
                x: i as f32 * 20.0,
                z: 0.0,
                gear: 3,
            }));
        }
        LapTrace {
            car_id: "ks_ferrari_488_gt3".into(),
            track_id: "monza".into(),
            lap_time_ms: t as u32,
            track_length_m: 4000.0,
            bins,
        }
    }

    #[test]
    fn corner_segmentation_finds_braking_delta_and_joins_hotspots() {
        let pb = synthetic_trace(27, 110.0, 0.0);
        let cur = synthetic_trace(25, 102.0, 250.0); // braked 2 bins (40m) early, 8 km/h slower
        let hotspots = vec![Hotspot {
            segment: 16, // ~bin 32 out of 200
            kind: EventKind::FrontLockup,
            count: 3,
        }];
        let report = analyze_corners(&pb, &cur, &hotspots);
        assert_eq!(report.len(), 1);
        let c = &report[0];
        assert_eq!(c.corner.number, 1);
        assert!(c.brake_delta_m.unwrap() < -30.0);
        assert!((c.apex_speed_delta_kmh - (-8.0)).abs() < 0.1);
        assert_eq!(c.front_lockups, 3);
        assert!(c.advice.contains("braking 40m early"));
        assert!(c.advice.contains("3 front lockup"));
    }
}
