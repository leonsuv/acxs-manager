//! Driving figures worked out from a replay's samples.
//!
//! A replay holds speed, pedals and position but no slip or steering, so only what those show
//! is reported: distance, top speed, hard stops, sudden speed loss (likely impacts) and drops
//! in height (jumps and falls).

use crate::replay::Sample;

/// Speed in km/h lost within [`IMPACT_WINDOW_S`] that counts as an impact, from at least [`IMPACT_MIN_SPEED`].
const IMPACT_LOSS: f32 = 18.0;
const IMPACT_MIN_SPEED: f32 = 25.0;
const IMPACT_WINDOW_S: f64 = 0.4;
/// Seconds between two impacts that count separately.
const EVENT_GAP_S: f64 = 3.0;
/// Brake level and speed loss over the same window for a hard stop (not an impact).
const HARD_BRAKE: f32 = 0.9;
/// An impact only counts with the brake below this across the whole window, else the driver may
/// simply be stopping or just letting off after a stop.
const IMPACT_BRAKE: f32 = 0.5;
/// Height lost in metres within a second that counts as a drop.
const DROP_M: f32 = 3.0;

/// What a replay shows about the drive.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReplayAnalysis {
    pub distance_km: f64,
    pub max_speed: f32,
    pub average_speed: f32,
    /// Seconds with the brake above 90%.
    pub hard_braking_s: f64,
    /// Sudden speed losses with no brake applied, such as hitting a wall.
    pub impacts: u32,
    pub drops: u32,
    pub full_throttle_share: f32,
}

/// Works through the samples once.
pub fn analyze(samples: &[Sample]) -> ReplayAnalysis {
    let mut out = ReplayAnalysis::default();
    if samples.len() < 2 {
        return out;
    }
    let mut metres = 0.0_f64;
    let mut full = 0usize;
    let mut last_impact = f64::NEG_INFINITY;
    let mut last_drop = f64::NEG_INFINITY;
    for (i, pair) in samples.windows(2).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        let dt = (b.time_s - a.time_s).max(0.0);
        metres += f64::from(b.speed_kmh) / 3.6 * dt;
        out.max_speed = out.max_speed.max(b.speed_kmh);
        if b.brake > HARD_BRAKE {
            out.hard_braking_s += dt;
        }
        if b.throttle > 0.95 {
            full += 1;
        }
        // Look back over the window for the fastest speed and the height a second ago.
        let mut top = b.speed_kmh;
        let mut braked = b.brake >= IMPACT_BRAKE;
        let mut j = i;
        loop {
            let s = &samples[j];
            if b.time_s - s.time_s > IMPACT_WINDOW_S {
                break;
            }
            top = top.max(s.speed_kmh);
            braked |= s.brake >= IMPACT_BRAKE;
            if j == 0 {
                break;
            }
            j -= 1;
        }
        if top >= IMPACT_MIN_SPEED
            && top - b.speed_kmh >= IMPACT_LOSS
            && !braked
            && b.time_s - last_impact > EVENT_GAP_S
        {
            out.impacts += 1;
            last_impact = b.time_s;
        }
        let mut k = i;
        while k > 0 && b.time_s - samples[k].time_s < 1.0 {
            k -= 1;
        }
        if samples[k].y - b.y >= DROP_M && b.time_s - last_drop > EVENT_GAP_S {
            out.drops += 1;
            last_drop = b.time_s;
        }
    }
    let total = samples.last().map_or(0.0, |s| s.time_s) - samples[0].time_s;
    out.distance_km = metres / 1000.0;
    if total > 0.0 {
        out.average_speed = (metres / total * 3.6) as f32;
    }
    out.full_throttle_share = full as f32 / (samples.len() - 1) as f32;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(time_s: f64, speed_kmh: f32, brake: f32, y: f32) -> Sample {
        Sample { time_s, speed_kmh, rpm: 0.0, gear: 3, throttle: 1.0, brake, x: 0.0, y, z: 0.0 }
    }

    #[test]
    fn distance_and_top_speed() {
        let samples: Vec<Sample> = (0..=10).map(|i| s(f64::from(i), 36.0, 0.0, 0.0)).collect();
        let a = analyze(&samples);
        assert!((a.distance_km - 0.1).abs() < 1e-9);
        assert_eq!(a.max_speed, 36.0);
        assert!((a.average_speed - 36.0).abs() < 1e-3);
        assert_eq!(a.full_throttle_share, 1.0);
    }

    #[test]
    fn impact_needs_a_sudden_loss_without_braking() {
        let mut crash: Vec<Sample> = (0..20).map(|i| s(f64::from(i) * 0.1, 150.0, 0.0, 0.0)).collect();
        crash.extend((20..30).map(|i| s(f64::from(i) * 0.1, 20.0, 0.0, 0.0)));
        assert_eq!(analyze(&crash).impacts, 1);
        let mut stop: Vec<Sample> = (0..20).map(|i| s(f64::from(i) * 0.1, 150.0, 1.0, 0.0)).collect();
        stop.extend((20..30).map(|i| s(f64::from(i) * 0.1, 20.0, 1.0, 0.0)));
        let a = analyze(&stop);
        assert_eq!(a.impacts, 0);
        assert!(a.hard_braking_s > 2.0);
    }

    #[test]
    fn drops_count_height_lost_in_a_second() {
        let samples: Vec<Sample> = (0..40).map(|i| s(f64::from(i) * 0.1, 100.0, 0.0, if i < 20 { 10.0 } else { 2.0 })).collect();
        assert_eq!(analyze(&samples).drops, 1);
        assert_eq!(analyze(&[]), ReplayAnalysis::default());
    }
}

#[cfg(test)]
mod real_replay_tests {
    use super::*;

    /// The two crashes in a real Ferrari replay: 29 to 8 and 180 to 75 km/h, no brake. A brake
    /// zone from the same drive, 150 to 130 at 94% brake, stays out.
    #[test]
    fn figures_from_a_real_drive() {
        let s = |t: f64, speed: f32, brake: f32| Sample { time_s: t, speed_kmh: speed, rpm: 0.0, gear: 3, throttle: 0.0, brake, x: 0.0, y: 0.0, z: 0.0 };
        let crash = |top: f32, low: f32| vec![s(0.0, top, 0.0), s(0.2, (top + low) / 2.0, 0.0), s(0.36, low, 0.0)];
        assert_eq!(analyze(&crash(29.0, 8.0)).impacts, 1);
        assert_eq!(analyze(&crash(180.0, 75.0)).impacts, 1);
        let braking = vec![s(0.0, 150.0, 0.94), s(0.2, 140.0, 0.94), s(0.36, 130.0, 0.94)];
        assert_eq!(analyze(&braking).impacts, 0);
    }
}

#[cfg(test)]
mod brake_release_tests {
    use super::*;

    /// Found on a real replay: the moment the brake comes up, the window still holds the stop.
    #[test]
    fn letting_off_the_brake_is_not_an_impact() {
        let s = |t: f64, speed: f32, brake: f32| Sample { time_s: t, speed_kmh: speed, rpm: 0.0, gear: 3, throttle: 0.0, brake, x: 0.0, y: 0.0, z: 0.0 };
        let samples = vec![s(0.0, 150.0, 1.0), s(0.1, 142.0, 1.0), s(0.2, 134.0, 1.0), s(0.3, 126.0, 0.8), s(0.36, 124.0, 0.0)];
        assert_eq!(analyze(&samples).impacts, 0);
    }
}
