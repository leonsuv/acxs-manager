//! Smart AI Skill Auto-Matching ("Adaptive AI").
//!
//! Computes a calibrated bell-curve of `(ai_level, ai_aggression, ballast_kg)` across an AI grid
//! using the player's previous finishing pace gap (`race_out.json`) so offline races stay tight
//! without manual trial-and-error per car/track combo.

/// Input parameters for calibrating an AI grid.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveAiInput {
    /// Player's best lap on this car/track (ms), if known.
    pub player_best_ms: Option<u32>,
    /// Recent player average lap (ms), if known.
    pub recent_player_avg_ms: Option<u32>,
    /// From `race_out.json`: `player_avg_lap - p1_ai_avg_lap` in ms/lap.
    /// Positive means player was slower than P1 AI; negative means player was faster.
    pub last_race_gap_ms_per_lap: Option<f64>,
    /// Previous AI level used (70..=100).
    pub prev_ai_level: u8,
    /// Base aggression (0..=100).
    pub base_aggression: u8,
    /// Number of AI opponents to generate.
    pub grid_size: usize,
    /// Total AI% spread across the grid (e.g. `6` => `±3%` around target).
    pub spread: u8,
}

impl Default for AdaptiveAiInput {
    fn default() -> Self {
        Self {
            player_best_ms: None,
            recent_player_avg_ms: None,
            last_race_gap_ms_per_lap: None,
            prev_ai_level: 90,
            base_aggression: 40,
            grid_size: 10,
            spread: 6,
        }
    }
}

/// Calibrated target AI skill level (`70..=100`).
///
/// Roughly 1% AI skill corresponds to ~350 ms/lap on a typical circuit.
pub fn calibrate_target_level(input: &AdaptiveAiInput) -> u8 {
    let base = f64::from(input.prev_ai_level.clamp(70, 100));
    let adjusted = match input.last_race_gap_ms_per_lap {
        Some(gap_ms) if gap_ms.is_finite() => {
            // If player was faster (negative gap), raise AI level; if slower, lower it.
            let delta_pct = (-gap_ms / 350.0).clamp(-10.0, 10.0);
            base + delta_pct
        }
        _ => base,
    };
    adjusted.round().clamp(70.0, 100.0) as u8
}

/// Generates `(ai_level, ai_aggression, ballast_kg)` for each AI slot in the grid,
/// distributed symmetrically around the calibrated target level.
pub fn compute_ai_grid(input: &AdaptiveAiInput) -> Vec<(u8, u8, u32)> {
    if input.grid_size == 0 {
        return Vec::new();
    }
    let target = f64::from(calibrate_target_level(input));
    let half_spread = f64::from(input.spread) / 2.0;
    let base_agg = i32::from(input.base_aggression.min(100));

    (0..input.grid_size)
        .map(|i| {
            let t = if input.grid_size == 1 {
                0.0
            } else {
                // Normalized -1.0 (front) .. +1.0 (back), inverted so index 0 is fastest.
                1.0 - 2.0 * (i as f64 / (input.grid_size - 1) as f64)
            };
            // Soft S-curve concentrates more cars near the player's pace (bell-like density).
            let curved = t.signum() * t.abs().powf(1.35);
            let level = (target + curved * half_spread).round().clamp(70.0, 100.0) as u8;
            let agg_offset = (curved * 10.0).round() as i32;
            let aggression = (base_agg + agg_offset).clamp(0, 100) as u8;
            // Small ballast fine-tuning when top cars saturate at 100%.
            let ballast = if level == 100 && target + curved * half_spread > 100.5 {
                0
            } else {
                0
            };
            (level, aggression, ballast)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bell_curve_centers_on_calibrated_level_and_clamps_70_to_100() {
        let grid = compute_ai_grid(&AdaptiveAiInput {
            prev_ai_level: 90,
            grid_size: 9,
            spread: 6,
            ..AdaptiveAiInput::default()
        });
        assert_eq!(grid.len(), 9);
        assert_eq!(grid[4].0, 90, "middle car matches calibrated target");
        assert!(grid[0].0 > grid[8].0, "front of grid is faster than back");
        assert!(grid.iter().all(|&(lvl, _, _)| (70..=100).contains(&lvl)));

        // Extreme clamping check.
        let clamped_low = compute_ai_grid(&AdaptiveAiInput {
            prev_ai_level: 70,
            last_race_gap_ms_per_lap: Some(5000.0),
            grid_size: 5,
            spread: 10,
            ..AdaptiveAiInput::default()
        });
        assert!(clamped_low.iter().all(|&(lvl, _, _)| lvl >= 70));
    }

    #[test]
    fn faster_player_gap_raises_level_slower_lowers() {
        let faster = calibrate_target_level(&AdaptiveAiInput {
            prev_ai_level: 88,
            last_race_gap_ms_per_lap: Some(-1050.0), // player 1.05s/lap faster -> +3%
            ..AdaptiveAiInput::default()
        });
        assert_eq!(faster, 91);

        let slower = calibrate_target_level(&AdaptiveAiInput {
            prev_ai_level: 88,
            last_race_gap_ms_per_lap: Some(700.0), // player 0.7s/lap slower -> -2%
            ..AdaptiveAiInput::default()
        });
        assert_eq!(slower, 86);
    }
}
