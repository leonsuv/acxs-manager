//! Playback model over a replay's per-frame samples.
//!
//! The player owns a playhead in seconds. Time moves only through [`Player::advance`],
//! so the model has no clock of its own and can be driven by any frame loop.

use crate::replay::Sample;

/// Slowest playback multiplier accepted by [`Player::set_speed`].
pub const MIN_SPEED: f64 = 0.25;
/// Fastest playback multiplier accepted by [`Player::set_speed`].
pub const MAX_SPEED: f64 = 4.0;

/// Playhead, play state and speed over a list of samples.
#[derive(Debug, Clone)]
pub struct Player {
    samples: Vec<Sample>,
    duration_s: f64,
    position_s: f64,
    speed: f64,
    playing: bool,
}

impl Player {
    /// Create a paused player at time 0 with speed 1.0.
    ///
    /// Samples must be in recording order (non-decreasing `time_s`), as the replay
    /// parser produces them. The duration is the time of the last sample.
    pub fn new(samples: Vec<Sample>) -> Self {
        let duration_s = samples.last().map_or(0.0, |s| s.time_s);
        Player {
            samples,
            duration_s,
            position_s: 0.0,
            speed: 1.0,
            playing: false,
        }
    }

    /// All samples, in recording order.
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// Start playback. If the playhead is already at the end, it restarts from 0.
    pub fn play(&mut self) {
        if self.position_s >= self.duration_s {
            self.position_s = 0.0;
        }
        self.playing = true;
    }

    /// Pause playback. The playhead stays where it is.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// True while playback is running.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Move the playhead to `seconds`, clamped to `0.0..=duration_s`.
    /// Non-finite input is ignored.
    pub fn seek(&mut self, seconds: f64) {
        if seconds.is_finite() {
            self.position_s = seconds.clamp(0.0, self.duration_s);
        }
    }

    /// Current playhead position in seconds.
    pub fn position_s(&self) -> f64 {
        self.position_s
    }

    /// Time of the last sample in seconds.
    pub fn duration_s(&self) -> f64 {
        self.duration_s
    }

    /// Set the playback speed multiplier, clamped to [`MIN_SPEED`]..=[`MAX_SPEED`].
    /// Non-finite input is ignored.
    pub fn set_speed(&mut self, multiplier: f64) {
        if multiplier.is_finite() {
            self.speed = multiplier.clamp(MIN_SPEED, MAX_SPEED);
        }
    }

    /// Current playback speed multiplier.
    pub fn speed(&self) -> f64 {
        self.speed
    }

    /// Move the playhead forward by `real_dt_s` wall-clock seconds, scaled by the speed.
    ///
    /// Does nothing while paused, or when `real_dt_s` is negative or non-finite.
    /// Reaching the end stops the playhead at `duration_s` and pauses playback.
    pub fn advance(&mut self, real_dt_s: f64) {
        if !self.playing || !real_dt_s.is_finite() || real_dt_s < 0.0 {
            return;
        }
        self.position_s = (self.position_s + real_dt_s * self.speed).min(self.duration_s);
        if self.position_s >= self.duration_s {
            self.playing = false;
        }
    }

    /// The latest sample recorded at or before the playhead, or `None` when there are no samples.
    pub fn current_sample(&self) -> Option<&Sample> {
        let reached = self.samples.partition_point(|s| s.time_s <= self.position_s);
        reached.checked_sub(1).and_then(|i| self.samples.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `count` samples spaced `step` seconds apart, starting at 0. Speed encodes the index.
    fn track(count: usize, step: f64) -> Vec<Sample> {
        (0..count)
            .map(|i| Sample {
                time_s: i as f64 * step,
                speed_kmh: i as f32,
                rpm: 0.0,
                gear: 0,
                throttle: 0.0,
                brake: 0.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })
            .collect()
    }

    #[test]
    fn seek_clamps_to_replay_bounds() {
        let mut p = Player::new(track(5, 0.5)); // duration 2.0
        assert_eq!(p.duration_s(), 2.0);

        p.seek(-3.0);
        assert_eq!(p.position_s(), 0.0);
        p.seek(99.0);
        assert_eq!(p.position_s(), 2.0);
        p.seek(1.25);
        assert_eq!(p.position_s(), 1.25);

        p.seek(f64::NAN);
        assert_eq!(p.position_s(), 1.25);
        p.seek(f64::INFINITY);
        assert_eq!(p.position_s(), 1.25);
    }

    #[test]
    fn speed_is_clamped_to_multiplier_range() {
        let mut p = Player::new(track(2, 1.0));
        assert_eq!(p.speed(), 1.0);

        p.set_speed(0.1);
        assert_eq!(p.speed(), MIN_SPEED);
        p.set_speed(10.0);
        assert_eq!(p.speed(), MAX_SPEED);
        p.set_speed(2.0);
        assert_eq!(p.speed(), 2.0);
        p.set_speed(f64::NAN);
        assert_eq!(p.speed(), 2.0);
    }

    #[test]
    fn advance_scales_by_speed_and_ignores_paused_or_bad_dt() {
        let mut p = Player::new(track(10, 1.0)); // duration 9.0

        p.advance(1.0);
        assert_eq!(p.position_s(), 0.0, "paused player must not move");

        p.play();
        p.set_speed(2.0);
        p.advance(0.5);
        assert!((p.position_s() - 1.0).abs() < 1e-12);

        p.advance(-1.0);
        p.advance(f64::NAN);
        assert!((p.position_s() - 1.0).abs() < 1e-12);

        p.pause();
        p.advance(1.0);
        assert!((p.position_s() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn advance_across_end_clamps_and_pauses() {
        let mut p = Player::new(track(4, 1.0)); // duration 3.0
        p.play();
        p.set_speed(4.0);

        p.advance(0.5); // 2.0 s
        assert!(p.is_playing());

        p.advance(10.0); // would be 42 s
        assert_eq!(p.position_s(), 3.0);
        assert!(!p.is_playing());
        assert_eq!(p.current_sample().map(|s| s.time_s), Some(3.0));

        p.advance(1.0);
        assert_eq!(p.position_s(), 3.0);

        p.play(); // at the end, play restarts from the beginning
        assert_eq!(p.position_s(), 0.0);
        assert!(p.is_playing());
    }

    #[test]
    fn current_sample_is_latest_at_or_before_playhead() {
        let mut p = Player::new(track(3, 0.5)); // times 0.0, 0.5, 1.0
        let time_at = |p: &Player| p.current_sample().map(|s| s.time_s);

        assert_eq!(time_at(&p), Some(0.0));
        p.seek(0.49);
        assert_eq!(time_at(&p), Some(0.0));
        p.seek(0.5);
        assert_eq!(time_at(&p), Some(0.5));
        p.seek(0.99);
        assert_eq!(time_at(&p), Some(0.5));
        p.seek(1.0);
        assert_eq!(time_at(&p), Some(1.0));
    }

    #[test]
    fn empty_player_is_inert() {
        let mut p = Player::new(Vec::new());
        assert!(p.current_sample().is_none());

        p.play();
        p.advance(5.0);
        p.seek(3.0);
        assert_eq!(p.position_s(), 0.0);
        assert!(!p.is_playing());
        assert!(p.current_sample().is_none());
    }
}
