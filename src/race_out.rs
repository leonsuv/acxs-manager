//! Parser for Assetto Corsa's `Documents/Assetto Corsa/out/race_out.json`.
//!
//! AC writes `race_out.json` when an offline session ends. Parsing it lets Playable
//! Championships auto-record race results, Adaptive AI calibrate from finishing gaps,
//! and Special Events / Career evaluate earned medals.

use serde::Deserialize;
use std::fs;
use std::io;
use std::path::Path;

use crate::championship::RaceResult;

/// Parsed summary of the final race/session in `race_out.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct RaceOutSummary {
    pub track: String,
    pub players: Vec<RaceOutPlayer>,
    pub results: Vec<RaceResult>,
    /// Player's best lap time in milliseconds, if recorded.
    pub player_best_lap_ms: Option<u32>,
    /// Leading AI's average lap time minus player's average lap time (ms/lap), if available.
    pub gap_to_p1_ms_per_lap: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaceOutPlayer {
    pub name: String,
    pub car: String,
    pub skin: String,
}

#[derive(Debug, Deserialize)]
struct RawRaceOut {
    #[serde(default)]
    track: String,
    #[serde(default)]
    players: Vec<RawPlayer>,
    #[serde(default)]
    sessions: Vec<RawSession>,
}

#[derive(Debug, Deserialize)]
struct RawPlayer {
    #[serde(default)]
    name: String,
    #[serde(default)]
    car: String,
    #[serde(default)]
    skin: String,
}

#[derive(Debug, Deserialize)]
struct RawSession {
    #[serde(default)]
    r#type: i32,
    #[serde(default, rename = "raceResult")]
    race_result: Vec<usize>,
    #[serde(default, rename = "bestLaps")]
    best_laps: Vec<RawBestLap>,
    #[serde(default)]
    laps: Vec<RawLap>,
}

#[derive(Debug, Deserialize)]
struct RawBestLap {
    #[serde(default)]
    car: usize,
    #[serde(default)]
    time: u32,
}

#[derive(Debug, Deserialize)]
struct RawLap {
    #[serde(default)]
    car: usize,
    #[serde(default)]
    time: u32,
}

/// Parses `race_out.json` text into a [`RaceOutSummary`].
pub fn parse_race_out(text: &str) -> Result<RaceOutSummary, String> {
    let raw: RawRaceOut = serde_json::from_str(text).map_err(|e| format!("invalid race_out.json: {e}"))?;
    let players: Vec<RaceOutPlayer> = raw
        .players
        .iter()
        .map(|p| RaceOutPlayer {
            name: p.name.clone(),
            car: p.car.clone(),
            skin: p.skin.clone(),
        })
        .collect();

    // Prefer the last Race session (type == 3), otherwise fall back to the last session.
    let session = raw
        .sessions
        .iter()
        .rev()
        .find(|s| s.r#type == 3)
        .or_else(|| raw.sessions.last());

    let mut results = Vec::new();
    let mut player_best_lap_ms = None;
    let mut gap_to_p1_ms_per_lap = None;

    if let Some(sess) = session {
        // Determine fastest lap car index (minimum non-zero time).
        let fastest_car = sess
            .best_laps
            .iter()
            .filter(|b| b.time > 0)
            .min_by_key(|b| b.time)
            .map(|b| b.car);

        player_best_lap_ms = sess
            .best_laps
            .iter()
            .find(|b| b.car == 0 && b.time > 0)
            .map(|b| b.time);

        // Count completed laps per car to detect DNFs (cars with 0 laps when leader completed laps).
        let max_laps = (0..players.len())
            .map(|c| sess.laps.iter().filter(|l| l.car == c && l.time > 0).count())
            .max()
            .unwrap_or(0);

        let order: Vec<usize> = if !sess.race_result.is_empty() {
            sess.race_result.clone()
        } else {
            (0..players.len()).collect()
        };

        for (pos_idx, &car_idx) in order.iter().enumerate() {
            let Some(player) = players.get(car_idx) else {
                continue;
            };
            let driver = if player.name.trim().is_empty() {
                format!("Driver {}", car_idx + 1)
            } else {
                player.name.trim().to_string()
            };
            let car_laps = sess.laps.iter().filter(|l| l.car == car_idx && l.time > 0).count();
            let dnf = max_laps > 0 && car_laps == 0;
            results.push(RaceResult {
                driver,
                position: (pos_idx + 1) as u32,
                fastest_lap: fastest_car == Some(car_idx) && !dnf,
                dnf,
            });
        }

        // Compute average-lap gap between player (car 0) and leading AI (first car != 0).
        if let Some(&lead_ai) = order.iter().find(|&&c| c != 0) {
            let avg = |car: usize| -> Option<f64> {
                let times: Vec<u32> = sess
                    .laps
                    .iter()
                    .filter(|l| l.car == car && l.time > 0)
                    .map(|l| l.time)
                    .collect();
                if times.is_empty() {
                    None
                } else {
                    Some(times.iter().map(|&t| f64::from(t)).sum::<f64>() / times.len() as f64)
                }
            };
            if let (Some(p_avg), Some(ai_avg)) = (avg(0), avg(lead_ai)) {
                // Positive => player was slower than P1 AI; negative => player was faster.
                gap_to_p1_ms_per_lap = Some(p_avg - ai_avg);
            }
        }
    }

    Ok(RaceOutSummary {
        track: raw.track,
        players,
        results,
        player_best_lap_ms,
        gap_to_p1_ms_per_lap,
    })
}

/// Reads and parses `<ac_docs>/out/race_out.json`.
pub fn load_race_out(path: &Path) -> io::Result<RaceOutSummary> {
    let text = fs::read_to_string(path)?;
    parse_race_out(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RACE_OUT: &str = r#"{
        "track": "monza",
        "players": [
            {"name": "Leo", "car": "ks_ferrari_488_gt3", "skin": "red"},
            {"name": "AI 1", "car": "ks_porsche_911_gt3_r", "skin": "blue"},
            {"name": "AI 2", "car": "ks_audi_r8_lms", "skin": "white"}
        ],
        "sessions": [
            {
                "type": 3,
                "raceResult": [1, 0, 2],
                "bestLaps": [
                    {"car": 0, "time": 108500},
                    {"car": 1, "time": 107900}
                ],
                "laps": [
                    {"car": 0, "time": 109000},
                    {"car": 0, "time": 108500},
                    {"car": 1, "time": 108000},
                    {"car": 1, "time": 107900}
                ]
            }
        ]
    }"#;

    #[test]
    fn race_out_json_parses_positions_dnfs_and_fastest_lap() {
        let summary = parse_race_out(SAMPLE_RACE_OUT).unwrap();
        assert_eq!(summary.track, "monza");
        assert_eq!(summary.player_best_lap_ms, Some(108_500));
        assert_eq!(summary.results.len(), 3);
        assert_eq!(summary.results[0].driver, "AI 1");
        assert_eq!(summary.results[0].position, 1);
        assert!(summary.results[0].fastest_lap);
        assert!(!summary.results[0].dnf);
        assert_eq!(summary.results[1].driver, "Leo");
        assert_eq!(summary.results[1].position, 2);
        assert!(!summary.results[1].fastest_lap);
        assert_eq!(summary.results[2].driver, "AI 2");
        assert!(summary.results[2].dnf);
        let gap = summary.gap_to_p1_ms_per_lap.unwrap();
        assert!((gap - 800.0).abs() < 1e-3);
    }
}
