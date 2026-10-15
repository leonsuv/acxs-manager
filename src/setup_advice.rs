//! Heuristic setup advisor. Reads the samples of one replay and estimates whether
//! the car leans towards understeer or oversteer in its corners, together with a
//! few setup directions to try. The result is an estimate, never a measurement.
//!
//! A replay sample carries position, speed, throttle and brake, but no steering or
//! g-force channel. Lateral load and yaw are therefore derived from the path the
//! car drew over time, and the signals below are built on those derived values.

use crate::replay::{Sample, MAX_SAMPLES};
use std::fmt;

/// Balance estimate for the analysed corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Balance {
    /// No clear understeer or oversteer pattern in the corners.
    Neutral,
    /// The car seems to push wide.
    Understeer,
    /// The rear seems to step out.
    Oversteer,
    /// Too little corner data, or both patterns are present and neither wins.
    Unknown,
}

/// The estimate and the notes explaining it.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupAdvice {
    pub balance: Balance,
    pub notes: Vec<String>,
}

/// Why the samples could not be analysed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdviceError {
    /// There were no samples.
    Empty,
    /// More samples than the analysis accepts.
    TooManySamples { count: usize, max: usize },
    /// A time, position, speed, throttle or brake value is NaN or infinite.
    NonFinite { index: usize },
    /// The sample time does not increase at this index.
    TimeNotIncreasing { index: usize },
}

impl fmt::Display for AdviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdviceError::Empty => write!(f, "no telemetry samples to analyse"),
            AdviceError::TooManySamples { count, max } => {
                write!(f, "{count} samples is more than the {max} the advisor accepts")
            }
            AdviceError::NonFinite { index } => {
                write!(f, "sample {index} has a NaN or infinite value")
            }
            AdviceError::TimeNotIncreasing { index } => {
                write!(f, "sample {index} is not later than the sample before it")
            }
        }
    }
}

impl std::error::Error for AdviceError {}

/// Speeds below this say little about balance and are ignored.
const MIN_SPEED_KMH: f64 = 30.0;
/// Lateral acceleration in m/s^2 that counts as cornering, about 0.25 g.
const CORNER_LAT_ACCEL: f64 = 2.5;
/// Change in rotation rate (rad/s) over `LAG_S` that counts as rotation building or fading.
const YAW_CHANGE: f64 = 0.2;
/// Window over which rotation, throttle and brake changes are measured.
const LAG_S: f64 = 0.4;
/// Time span of the chord used to estimate heading from positions. A longer span
/// averages out position rounding in the recording.
const CHORD_S: f64 = 0.1;
/// Shortest chord that gives a usable heading.
const MIN_CHORD_M: f64 = 0.5;
/// Half width of the window that smooths the derived rotation rate.
const SMOOTH_HALF_S: f64 = 0.1;
/// Gaps between samples longer than this are not counted as driving time.
const MAX_GAP_S: f64 = 0.5;
/// Cornering time needed before a balance is claimed.
const MIN_CORNER_S: f64 = 2.0;
/// Share of corner samples that must show a pattern before it counts.
const EVENT_SHARE: f64 = 0.08;
/// One pattern must outweigh the other by this factor to win.
const DOMINANCE: f64 = 1.5;
/// Throttle or brake level (0 to 1) that counts as being on power or braking.
const ON_POWER: f64 = 0.3;
const ON_BRAKE: f64 = 0.2;
/// Throttle change over `LAG_S` that counts as a lift or a pickup.
const THROTTLE_STEP: f64 = 0.2;
/// Share of corner braking samples with sharp rotation changes worth a note.
const BRAKE_SHARE: f64 = 0.1;
/// Rotation direction reversals per cornering second worth a note.
const CORRECTION_RATE: f64 = 0.3;
/// Rotation rates (rad/s) below this are too small to count as a reversal.
const REVERSAL_MIN_YAW: f64 = 0.1;

/// Analyse the samples of one car and return a balance estimate with notes.
pub fn advise(samples: &[Sample]) -> Result<SetupAdvice, AdviceError> {
    if samples.is_empty() {
        return Err(AdviceError::Empty);
    }
    if samples.len() > MAX_SAMPLES {
        return Err(AdviceError::TooManySamples {
            count: samples.len(),
            max: MAX_SAMPLES,
        });
    }
    validate(samples)?;

    let track = Track::new(samples);
    let corners = track.corner_stats();
    let braking = track.braking_stats();

    let balance = if corners.time_s < MIN_CORNER_S {
        Balance::Unknown
    } else {
        let under = corners.understeer as f64 / corners.samples as f64;
        let over = corners.oversteer as f64 / corners.samples as f64;
        decide(under, over)
    };

    let mut notes = Vec::new();
    match balance {
        Balance::Understeer => notes.push(
            "The car seems to push wide: rotation fades on throttle, under braking or during \
             lifts in corners. You could try softening the front anti-roll bar, or stiffening \
             the rear one, to free up front grip."
                .to_string(),
        ),
        Balance::Oversteer => notes.push(
            "The rear seems to step out: rotation builds when the throttle is lifted or picked \
             up in corners. You could try stiffening the front anti-roll bar, or softening the \
             rear one, to free up rear grip."
                .to_string(),
        ),
        Balance::Neutral => notes.push(
            "No clear understeer or oversteer pattern showed up in the corners analysed."
                .to_string(),
        ),
        Balance::Unknown => {
            if corners.time_s < MIN_CORNER_S {
                notes.push(format!(
                    "Only {:.1} s of cornering was found, and at least {:.0} s is needed for an \
                     estimate. Drive some corners above {:.0} km/h.",
                    corners.time_s, MIN_CORNER_S, MIN_SPEED_KMH
                ));
            } else {
                notes.push(
                    "Understeer and oversteer patterns both appear, so the balance is unclear. \
                     Compare individual corners before changing the setup."
                        .to_string(),
                );
            }
        }
    }

    if corners.samples > 0 {
        let under = 100.0 * corners.understeer as f64 / corners.samples as f64;
        let over = 100.0 * corners.oversteer as f64 / corners.samples as f64;
        notes.push(format!(
            "Analysed {:.1} s of cornering. Understeer-like samples: {:.0}%. \
             Oversteer-like samples: {:.0}%.",
            corners.time_s, under, over
        ));
    }

    if corners.time_s > 0.0 {
        let rate = corners.reversals as f64 / corners.time_s;
        if rate >= CORRECTION_RATE {
            notes.push(format!(
                "Rotation changed direction about {:.0} times per minute of cornering. That \
                 suggests frequent corrections, so check the same corners for a steady line \
                 before changing the setup.",
                rate * 60.0
            ));
        }
    }

    if braking.corner_samples > 0 {
        let share = braking.sharp as f64 / braking.corner_samples as f64;
        if share >= BRAKE_SHARE {
            notes.push(
                "Rotation changes sharply while braking in corners. The rear may be unsettled \
                 under braking. You could check the brake bias, and how quickly the brake is \
                 released into the corner."
                    .to_string(),
            );
        }
    }

    notes.push(
        "This is a heuristic estimate from position, speed, throttle and brake only. The \
         replay has no steering or g-force channel, so lateral load and yaw are derived from \
         the car's path. Change one setup item at a time and drive the same corners again."
            .to_string(),
    );

    Ok(SetupAdvice { balance, notes })
}

/// Pick the balance from the share of corner samples showing each pattern.
fn decide(under: f64, over: f64) -> Balance {
    match (under >= EVENT_SHARE, over >= EVENT_SHARE) {
        (false, false) => Balance::Neutral,
        (true, false) => Balance::Understeer,
        (false, true) => Balance::Oversteer,
        (true, true) => {
            if under >= DOMINANCE * over {
                Balance::Understeer
            } else if over >= DOMINANCE * under {
                Balance::Oversteer
            } else {
                Balance::Unknown
            }
        }
    }
}

fn validate(samples: &[Sample]) -> Result<(), AdviceError> {
    for (index, s) in samples.iter().enumerate() {
        let finite = s.time_s.is_finite()
            && s.speed_kmh.is_finite()
            && s.throttle.is_finite()
            && s.brake.is_finite()
            && s.x.is_finite()
            && s.z.is_finite();
        if !finite {
            return Err(AdviceError::NonFinite { index });
        }
        if index > 0 && s.time_s <= samples[index - 1].time_s {
            return Err(AdviceError::TimeNotIncreasing { index });
        }
    }
    Ok(())
}

/// Per-sample series derived from the replay, in f64, plus the derived rotation rate.
struct Track {
    t: Vec<f64>,
    /// Time since the previous sample, zero across gaps.
    dt: Vec<f64>,
    speed_kmh: Vec<f64>,
    /// Lateral acceleration estimate in m/s^2, NaN where the path gives no heading.
    lat_accel: Vec<f64>,
    /// Rotation rate in rad/s, smoothed in time. NaN where unknown.
    yaw: Vec<f64>,
    /// Index of the first sample at least `LAG_S` before each sample.
    lag_index: Vec<usize>,
    /// Smoothed throttle and brake.
    throttle_s: Vec<f64>,
    brake_s: Vec<f64>,
}

#[derive(Default)]
struct CornerStats {
    samples: usize,
    time_s: f64,
    understeer: usize,
    oversteer: usize,
    /// Direction reversals of the rotation rate inside corners.
    reversals: usize,
}

#[derive(Default)]
struct BrakingStats {
    corner_samples: usize,
    sharp: usize,
}

impl Track {
    fn new(samples: &[Sample]) -> Track {
        let n = samples.len();
        let t: Vec<f64> = samples.iter().map(|s| s.time_s).collect();
        let x: Vec<f64> = samples.iter().map(|s| f64::from(s.x)).collect();
        let z: Vec<f64> = samples.iter().map(|s| f64::from(s.z)).collect();

        let mut dt = vec![0.0; n];
        for k in 1..n {
            let step = t[k] - t[k - 1];
            dt[k] = if step <= MAX_GAP_S { step } else { 0.0 };
        }

        let heading = chord_headings(&t, &x, &z);
        let mut yaw_raw = vec![f64::NAN; n];
        for k in 1..n {
            if let (Some(prev), Some(cur)) = (heading[k - 1], heading[k]) && dt[k] > 0.0 {
                yaw_raw[k] = wrap_angle(cur - prev) / dt[k];
            }
        }
        let yaw = centred_mean(&t, &yaw_raw, SMOOTH_HALF_S);

        let speed_kmh: Vec<f64> = samples.iter().map(|s| f64::from(s.speed_kmh)).collect();
        let lat_accel: Vec<f64> = speed_kmh
            .iter()
            .zip(&yaw)
            .map(|(v, w)| (v / 3.6) * w.abs())
            .collect();

        let throttle: Vec<f64> = samples.iter().map(|s| f64::from(s.throttle)).collect();
        let brake: Vec<f64> = samples.iter().map(|s| f64::from(s.brake)).collect();
        let throttle_s = centred_mean(&t, &throttle, SMOOTH_HALF_S);
        let brake_s = centred_mean(&t, &brake, SMOOTH_HALF_S);

        let mut lag_index = vec![0; n];
        let mut j = 0;
        for k in 0..n {
            while j < k && t[j] < t[k] - LAG_S {
                j += 1;
            }
            lag_index[k] = j;
        }

        Track {
            t,
            dt,
            speed_kmh,
            lat_accel,
            yaw,
            lag_index,
            throttle_s,
            brake_s,
        }
    }

    fn fast_enough(&self, k: usize) -> bool {
        self.speed_kmh[k] >= MIN_SPEED_KMH
    }

    fn in_corner(&self, k: usize) -> bool {
        self.fast_enough(k)
            && self.yaw[k].is_finite()
            && self.lat_accel[k] >= CORNER_LAT_ACCEL
    }

    /// Change of rotation magnitude and throttle over the lag window ending at `k`.
    /// Returns None when the lag window has no rotation rate yet.
    fn changes(&self, k: usize) -> Option<(f64, f64)> {
        let b = self.lag_index[k];
        if b == k || !self.yaw[b].is_finite() || !self.yaw[k].is_finite() {
            return None;
        }
        let d_yaw = self.yaw[k].abs() - self.yaw[b].abs();
        let d_thr = self.throttle_s[k] - self.throttle_s[b];
        Some((d_yaw, d_thr))
    }

    fn corner_stats(&self) -> CornerStats {
        let mut stats = CornerStats::default();
        for k in 0..self.t.len() {
            if !self.in_corner(k) {
                continue;
            }
            stats.samples += 1;
            stats.time_s += self.dt[k];

            if let Some((d_yaw, d_thr)) = self.changes(k) {
                let grow = d_yaw >= YAW_CHANGE;
                let fade = d_yaw <= -YAW_CHANGE;
                let lift = d_thr <= -THROTTLE_STEP;
                let pickup = d_thr >= THROTTLE_STEP;
                let on_power = self.throttle_s[k] >= ON_POWER;
                let on_brake = self.brake_s[k] >= ON_BRAKE;

                // Rotation building after a lift or pickup: the rear is moving.
                if grow && (lift || pickup) {
                    stats.oversteer += 1;
                }
                // Rotation fading while still on power, braking or lifting: the car is
                // running wide.
                if fade && (on_power || on_brake || lift) {
                    stats.understeer += 1;
                }
            }

            if k > 0 && self.in_corner(k - 1) {
                let (a, b) = (self.yaw[k - 1], self.yaw[k]);
                if a.abs() >= REVERSAL_MIN_YAW && b.abs() >= REVERSAL_MIN_YAW && a.signum() != b.signum() {
                    stats.reversals += 1;
                }
            }
        }
        stats
    }

    fn braking_stats(&self) -> BrakingStats {
        let mut stats = BrakingStats::default();
        for k in 0..self.t.len() {
            if !self.in_corner(k) || self.brake_s[k] < ON_POWER {
                continue;
            }
            stats.corner_samples += 1;
            if let Some((d_yaw, _)) = self.changes(k) && d_yaw.abs() >= YAW_CHANGE {
                stats.sharp += 1;
            }
        }
        stats
    }
}

/// Heading of the chord from an earlier sample to each sample, over about `CHORD_S`.
/// None where no earlier sample is far enough away.
fn chord_headings(t: &[f64], x: &[f64], z: &[f64]) -> Vec<Option<f64>> {
    let n = t.len();
    let mut out = vec![None; n];
    let mut j = 0;
    for k in 0..n {
        while j < k && t[j] < t[k] - CHORD_S {
            j += 1;
        }
        if j < k {
            let dx = x[k] - x[j];
            let dz = z[k] - z[j];
            if dx.hypot(dz) >= MIN_CHORD_M {
                out[k] = Some(dz.atan2(dx));
            }
        }
    }
    out
}

/// Mean of the finite values within `half` seconds of each sample. NaN where there are none.
fn centred_mean(t: &[f64], values: &[f64], half: f64) -> Vec<f64> {
    let n = values.len();
    let mut sum = vec![0.0; n + 1];
    let mut count = vec![0usize; n + 1];
    for i in 0..n {
        let finite = values[i].is_finite();
        sum[i + 1] = sum[i] + if finite { values[i] } else { 0.0 };
        count[i + 1] = count[i] + usize::from(finite);
    }

    let mut out = vec![f64::NAN; n];
    let (mut lo, mut hi) = (0, 0);
    for k in 0..n {
        while lo < k && t[lo] < t[k] - half {
            lo += 1;
        }
        while hi < n && t[hi] <= t[k] + half {
            hi += 1;
        }
        let c = count[hi] - count[lo];
        if c > 0 {
            out[k] = (sum[hi] - sum[lo]) / c as f64;
        }
    }
    out
}

/// Wrap an angle to the range (-pi, pi].
fn wrap_angle(a: f64) -> f64 {
    use std::f64::consts::PI;
    (a + PI).rem_euclid(2.0 * PI) - PI
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 1.0 / 60.0;
    /// Constant forward speed, 43 km/h. Low enough that the yaw rates in the tests
    /// give a plausible lateral load.
    const SPEED_MS: f64 = 12.0;

    /// Integrate a scripted rotation rate (rad/s) into positions, at `SPEED_MS`.
    fn drive(
        seconds: f64,
        yaw: impl Fn(f64) -> f64,
        throttle: impl Fn(f64) -> f32,
        brake: impl Fn(f64) -> f32,
    ) -> Vec<Sample> {
        let n = (seconds / DT).round() as usize;
        let (mut x, mut z, mut heading) = (0.0f64, 0.0f64, 0.0f64);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 * DT;
            out.push(Sample {
                time_s: t,
                speed_kmh: (SPEED_MS * 3.6) as f32,
                rpm: 6000.0,
                gear: 4,
                throttle: throttle(t),
                brake: brake(t),
                x: x as f32,
                y: 0.0,
                z: z as f32,
            });
            heading += yaw(t) * DT;
            x += SPEED_MS * heading.cos() * DT;
            z += SPEED_MS * heading.sin() * DT;
        }
        out
    }

    /// Rotation rate that rises and falls in a 1.7 s cycle: it fades from 1.2 to 0.3
    /// rad/s over 1.5 s, then snaps back up over 0.2 s.
    fn fading_cycle(t: f64) -> f64 {
        let phase = t % 1.7;
        if phase < 1.5 {
            1.2 - 0.6 * phase
        } else {
            0.3 + 4.5 * (phase - 1.5)
        }
    }

    #[test]
    fn steady_corner_is_neutral() {
        let samples = drive(12.0, |_| 0.6, |_| 0.5, |_| 0.0);
        let advice = advise(&samples).unwrap();
        assert_eq!(advice.balance, Balance::Neutral);
    }

    #[test]
    fn rotation_fading_on_power_is_understeer() {
        // Full throttle all the time, and the car keeps losing rotation mid-corner.
        let samples = drive(12.0, fading_cycle, |_| 1.0, |_| 0.0);
        let advice = advise(&samples).unwrap();
        assert_eq!(advice.balance, Balance::Understeer);
        assert!(advice.notes.iter().any(|n| n.contains("front anti-roll bar")));
    }

    #[test]
    fn rotation_building_after_lift_is_oversteer() {
        // Throttle lands at 0.5 s and lifts at 1.5 s of each 2 s cycle, while the car
        // rotates more and more. Rotation is at rest with the throttle off in between.
        let cycle = |t: f64| t % 2.0;
        let yaw = move |t: f64| {
            let p = cycle(t);
            if p < 0.2 {
                1.2 - 4.5 * p
            } else if p < 0.5 {
                0.3
            } else {
                0.3 + 0.6 * (p - 0.5)
            }
        };
        let throttle = move |t: f64| if (0.5..1.5).contains(&cycle(t)) { 1.0 } else { 0.0 };
        let samples = drive(12.0, yaw, throttle, |_| 0.0);
        let advice = advise(&samples).unwrap();
        assert_eq!(advice.balance, Balance::Oversteer);
        assert!(advice.notes.iter().any(|n| n.contains("stiffening the front anti-roll bar")));
    }

    #[test]
    fn straight_line_is_unknown() {
        let samples = drive(12.0, |_| 0.0, |_| 1.0, |_| 0.0);
        let advice = advise(&samples).unwrap();
        assert_eq!(advice.balance, Balance::Unknown);
        assert!(advice.notes.iter().any(|n| n.contains("Only")));
    }

    #[test]
    fn both_patterns_of_similar_size_is_unknown() {
        assert_eq!(decide(0.2, 0.2), Balance::Unknown);
        assert_eq!(decide(0.2, 0.01), Balance::Understeer);
        assert_eq!(decide(0.01, 0.2), Balance::Oversteer);
        assert_eq!(decide(0.0, 0.0), Balance::Neutral);
        assert_eq!(decide(0.5, 0.2), Balance::Understeer);
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(advise(&[]), Err(AdviceError::Empty));
    }

    #[test]
    fn nan_input_is_rejected() {
        let mut samples = drive(1.0, |_| 0.5, |_| 0.5, |_| 0.0);
        samples[7].throttle = f32::NAN;
        assert_eq!(advise(&samples), Err(AdviceError::NonFinite { index: 7 }));
    }

    #[test]
    fn non_increasing_time_is_rejected() {
        let mut samples = drive(1.0, |_| 0.5, |_| 0.5, |_| 0.0);
        samples[10].time_s = samples[9].time_s;
        assert_eq!(advise(&samples), Err(AdviceError::TimeNotIncreasing { index: 10 }));
    }

    #[test]
    fn oversized_input_is_rejected() {
        let sample = Sample {
            time_s: 0.0,
            speed_kmh: 0.0,
            rpm: 0.0,
            gear: 0,
            throttle: 0.0,
            brake: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let samples = vec![sample; MAX_SAMPLES + 1];
        assert!(matches!(advise(&samples), Err(AdviceError::TooManySamples { .. })));
    }
}
