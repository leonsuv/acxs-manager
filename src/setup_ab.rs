//! Automated Setup A/B Testing across recorded stints.
//!
//! Groups [`RecordedSession`]s for `(car_id, track_id)` by `setup_hash`, pairs them with
//! `crate::setups::diff()` when both setup INI files are available, and builds a side-by-side
//! scorecard comparing pace, lap consistency (std-dev), lockups/wheelspins per 10 min, and
//! understeer/oversteer balance.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::session_recorder::RecordedSession;
use crate::setups::{self, SetupDiff};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupStintMetrics {
    pub setup_hash: String,
    pub setup_name: String,
    pub session_count: usize,
    pub valid_laps: usize,
    pub best_lap_ms: Option<u32>,
    pub mean_lap_ms: Option<f64>,
    pub lap_stddev_ms: Option<f64>,
    pub lockups_per_10min: f64,
    pub wheelspins_per_10min: f64,
    pub understeer_pct: f64,
    pub oversteer_pct: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetupAbScorecard {
    pub car_id: String,
    pub track_id: String,
    pub a: SetupStintMetrics,
    pub b: SetupStintMetrics,
    /// `b.mean_lap_ms - a.mean_lap_ms` (negative means Setup B is faster on average).
    pub mean_delta_ms: Option<f64>,
    /// `b.best_lap_ms - a.best_lap_ms` (negative means Setup B has a faster best lap).
    pub best_delta_ms: Option<i64>,
    /// Exact INI parameter differences between Setup A and Setup B.
    pub setup_diffs: Vec<SetupDiff>,
}

fn aggregate_stint(hash: &str, sessions: &[&RecordedSession]) -> Option<SetupStintMetrics> {
    if sessions.is_empty() {
        return None;
    }
    let setup_name = sessions
        .iter()
        .find_map(|s| s.setup_name.clone())
        .unwrap_or_else(|| hash.chars().take(8).collect());

    let mut lap_times: Vec<f64> = Vec::new();
    let mut best_lap_ms: Option<u32> = None;
    let mut total_secs = 0.0_f64;
    let mut lockups = 0u32;
    let mut wheelspins = 0u32;
    let mut cornering = 0.0_f64;
    let mut under = 0.0_f64;
    let mut over = 0.0_f64;

    for s in sessions {
        total_secs += s.seconds;
        lockups += s.front_lockups + s.rear_lockups;
        wheelspins += s.wheelspins;
        cornering += s.cornering_seconds;
        under += s.understeer_seconds;
        over += s.oversteer_seconds;
        for lap in &s.laps {
            if lap.time_ms > 0 {
                let ms = lap.time_ms as u32;
                lap_times.push(f64::from(ms));
                best_lap_ms = Some(best_lap_ms.map_or(ms, |b| b.min(ms)));
            }
        }
    }

    let (mean_lap_ms, lap_stddev_ms) = if lap_times.is_empty() {
        (None, None)
    } else {
        let mean = lap_times.iter().sum::<f64>() / lap_times.len() as f64;
        let var = lap_times
            .iter()
            .map(|t| {
                let d = t - mean;
                d * d
            })
            .sum::<f64>()
            / lap_times.len() as f64;
        (Some(mean), Some(var.sqrt()))
    };

    let minutes = (total_secs / 60.0).max(1e-6);
    let (understeer_pct, oversteer_pct) = if cornering > 0.0 {
        (100.0 * under / cornering, 100.0 * over / cornering)
    } else {
        (0.0, 0.0)
    };

    Some(SetupStintMetrics {
        setup_hash: hash.to_string(),
        setup_name,
        session_count: sessions.len(),
        valid_laps: lap_times.len(),
        best_lap_ms,
        mean_lap_ms,
        lap_stddev_ms,
        lockups_per_10min: f64::from(lockups) / minutes * 10.0,
        wheelspins_per_10min: f64::from(wheelspins) / minutes * 10.0,
        understeer_pct,
        oversteer_pct,
    })
}

/// Builds a Setup A vs Setup B scorecard from recorded sessions and optional setup INI files.
pub fn compare_stints(
    sessions: &[RecordedSession],
    car_id: &str,
    track_id: &str,
    hash_a: &str,
    hash_b: &str,
    setup_a_path: Option<&Path>,
    setup_b_path: Option<&Path>,
) -> Option<SetupAbScorecard> {
    let matching = |hash: &str| -> Vec<&RecordedSession> {
        sessions
            .iter()
            .filter(|s| {
                s.car_id.eq_ignore_ascii_case(car_id)
                    && s.full_track_id().eq_ignore_ascii_case(track_id)
                    && s.setup_hash.as_deref() == Some(hash)
            })
            .collect()
    };

    let a = aggregate_stint(hash_a, &matching(hash_a))?;
    let b = aggregate_stint(hash_b, &matching(hash_b))?;
    let mean_delta_ms = match (a.mean_lap_ms, b.mean_lap_ms) {
        (Some(ma), Some(mb)) => Some(mb - ma),
        _ => None,
    };
    let best_delta_ms = match (a.best_lap_ms, b.best_lap_ms) {
        (Some(ba), Some(bb)) => Some(i64::from(bb) - i64::from(ba)),
        _ => None,
    };
    let setup_diffs = match (setup_a_path, setup_b_path) {
        (Some(pa), Some(pb)) => match (crate::ini::Ini::load(pa), crate::ini::Ini::load(pb)) {
            (Ok(ia), Ok(ib)) => setups::diff(&ia, &ib),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    Some(SetupAbScorecard {
        car_id: car_id.to_string(),
        track_id: track_id.to_string(),
        a,
        b,
        mean_delta_ms,
        best_delta_ms,
        setup_diffs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_recorder::RecordedLap;

    fn make_session(hash: &str, name: &str, laps: &[i32], under: f64) -> RecordedSession {
        RecordedSession {
            car_id: "ks_ferrari_488_gt3".into(),
            track_id: "spa".into(),
            seconds: 600.0,
            cornering_seconds: 100.0,
            understeer_seconds: under,
            laps: laps
                .iter()
                .enumerate()
                .map(|(i, &ms)| RecordedLap {
                    number: (i + 1) as i32,
                    time_ms: ms,
                })
                .collect(),
            setup_name: Some(name.into()),
            setup_hash: Some(hash.into()),
            ..RecordedSession::default()
        }
    }

    #[test]
    fn setup_ab_scorecard_pairs_setup_diff_with_stint_metrics() {
        let dir = std::env::temp_dir().join(format!("acxs-setup-ab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pa = dir.join("a.ini");
        let pb = dir.join("b.ini");
        std::fs::write(&pa, "[ARB_FRONT]\nVALUE=5\n").unwrap();
        std::fs::write(&pb, "[ARB_FRONT]\nVALUE=3\n").unwrap();

        let sessions = vec![
            make_session("hashA", "Baseline", &[140_000, 140_400], 45.0),
            make_session("hashB", "SoftFront", &[139_200, 139_400], 22.0),
        ];

        let card = compare_stints(&sessions, "ks_ferrari_488_gt3", "spa", "hashA", "hashB", Some(&pa), Some(&pb))
            .unwrap();
        assert_eq!(card.best_delta_ms, Some(-800));
        assert!((card.mean_delta_ms.unwrap() - (-900.0)).abs() < 1e-3);
        assert!(card.b.understeer_pct < card.a.understeer_pct);
        assert_eq!(card.setup_diffs.len(), 1);
        assert_eq!(card.setup_diffs[0].section, "ARB_FRONT");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
