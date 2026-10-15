//! Lap delta calculation: how far ahead or behind a live lap is compared with a
//! reference lap at the same track position.
//!
//! The delta at a position is `current_elapsed(position) - reference_elapsed(position)`.
//! A positive delta means the current lap is slower (behind the reference) at
//! that point; a negative delta means it is faster (ahead).
//!
//! Here the position is distance along the lap in metres. Times between
//! recorded samples are linearly interpolated.

use std::fmt;

/// One recorded point on a lap: distance travelled and elapsed lap time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub distance_m: f64,
    pub time_ms: f64,
}

/// A lap as a sequence of samples ordered by distance.
#[derive(Debug, Clone, PartialEq)]
pub struct Lap {
    pub samples: Vec<Sample>,
}

/// Errors produced when building a [`Lap`].
#[derive(Debug, Clone, PartialEq)]
pub enum DeltaError {
    /// The sample list was empty.
    EmptyLap,
    /// A sample had a NaN or infinite distance or time.
    NonFiniteSample { index: usize },
    /// A sample's distance was smaller than the previous sample's.
    NonMonotonicDistance { index: usize },
}

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeltaError::EmptyLap => write!(f, "lap has no samples"),
            DeltaError::NonFiniteSample { index } => {
                write!(f, "sample {index} has a non-finite distance or time")
            }
            DeltaError::NonMonotonicDistance { index } => {
                write!(f, "sample {index} has a distance smaller than the previous sample")
            }
        }
    }
}

impl std::error::Error for DeltaError {}

impl Lap {
    /// Validates and builds a lap. Samples must be non-empty, all values
    /// finite, and distances non-decreasing.
    pub fn new(samples: Vec<Sample>) -> Result<Lap, DeltaError> {
        if samples.is_empty() {
            return Err(DeltaError::EmptyLap);
        }
        for (index, s) in samples.iter().enumerate() {
            if !s.distance_m.is_finite() || !s.time_ms.is_finite() {
                return Err(DeltaError::NonFiniteSample { index });
            }
            if index > 0 && s.distance_m < samples[index - 1].distance_m {
                return Err(DeltaError::NonMonotonicDistance { index });
            }
        }
        Ok(Lap { samples })
    }

    fn start_distance(&self) -> Option<f64> {
        self.samples.first().map(|s| s.distance_m)
    }

    fn end_distance(&self) -> Option<f64> {
        self.samples.last().map(|s| s.distance_m)
    }
}

/// Elapsed time (ms) at `distance_m` on `lap`, linearly interpolated between
/// the surrounding samples. Returns `None` if the distance is not finite or
/// lies outside the lap's recorded range.
pub fn time_at_distance(lap: &Lap, distance_m: f64) -> Option<f64> {
    if !distance_m.is_finite() {
        return None;
    }
    let (first, last) = (lap.start_distance()?, lap.end_distance()?);
    if distance_m < first || distance_m > last {
        return None;
    }

    // First sample with distance >= target.
    let hi_idx = lap
        .samples
        .partition_point(|s| s.distance_m < distance_m);
    let hi = lap.samples.get(hi_idx)?;
    if hi.distance_m == distance_m || hi_idx == 0 {
        return Some(hi.time_ms);
    }

    // Here lo.distance < distance_m <= hi.distance, so the span is positive.
    let lo = lap.samples[hi_idx - 1];
    let fraction = (distance_m - lo.distance_m) / (hi.distance_m - lo.distance_m);
    Some(lo.time_ms + fraction * (hi.time_ms - lo.time_ms))
}

/// Delta in ms at `distance_m`: `current - reference`. Positive means the
/// current lap is slower than the reference at this point, negative means
/// faster. Returns `None` if either lap does not cover the distance.
pub fn delta_ms(reference: &Lap, current: &Lap, distance_m: f64) -> Option<f64> {
    let cur = time_at_distance(current, distance_m)?;
    let reference_time = time_at_distance(reference, distance_m)?;
    Some(cur - reference_time)
}

/// Distance range covered by both laps, or `None` if they do not overlap.
fn overlap(a: &Lap, b: &Lap) -> Option<(f64, f64)> {
    let start = a.start_distance()?.max(b.start_distance()?);
    let end = a.end_distance()?.min(b.end_distance()?);
    if start <= end {
        Some((start, end))
    } else {
        None
    }
}

/// Samples the delta every `step_m` metres across the overlap of the two laps,
/// starting at the overlap start. Returns `(distance_m, delta_ms)` pairs. The
/// result is empty if either lap is empty, the laps do not overlap, or
/// `step_m` is not a finite positive number.
pub fn delta_trace(reference: &Lap, current: &Lap, step_m: f64) -> Vec<(f64, f64)> {
    if !step_m.is_finite() || step_m <= 0.0 {
        return Vec::new();
    }
    let Some((start, end)) = overlap(reference, current) else {
        return Vec::new();
    };

    // Small tolerance so an end that lands on a step boundary is included.
    let count = ((end - start) / step_m + 1e-9).floor() as usize + 1;
    (0..count)
        .filter_map(|k| {
            let distance = (start + k as f64 * step_m).min(end);
            delta_ms(reference, current, distance).map(|d| (distance, d))
        })
        .collect()
}

/// Delta at the end of the overlap between the two laps (the final distance
/// both laps cover). `None` if the laps do not overlap.
pub fn final_delta_ms(reference: &Lap, current: &Lap) -> Option<f64> {
    let (_, end) = overlap(reference, current)?;
    delta_ms(reference, current, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(points: &[(f64, f64)]) -> Lap {
        Lap::new(
            points
                .iter()
                .map(|&(distance_m, time_ms)| Sample { distance_m, time_ms })
                .collect(),
        )
        .expect("valid test lap")
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    fn reference() -> Lap {
        lap(&[(0.0, 0.0), (100.0, 10_000.0), (200.0, 20_000.0)])
    }

    #[test]
    fn interpolation_at_exact_samples() {
        let r = reference();
        approx(time_at_distance(&r, 0.0).unwrap(), 0.0);
        approx(time_at_distance(&r, 100.0).unwrap(), 10_000.0);
        approx(time_at_distance(&r, 200.0).unwrap(), 20_000.0);
    }

    #[test]
    fn interpolation_between_samples() {
        let r = reference();
        approx(time_at_distance(&r, 50.0).unwrap(), 5_000.0);
        approx(time_at_distance(&r, 150.0).unwrap(), 15_000.0);
        let uneven = lap(&[(0.0, 0.0), (10.0, 1_000.0), (40.0, 1_500.0)]);
        approx(time_at_distance(&uneven, 25.0).unwrap(), 1_250.0);
    }

    #[test]
    fn out_of_range_is_none() {
        let r = reference();
        assert_eq!(time_at_distance(&r, -0.1), None);
        assert_eq!(time_at_distance(&r, 200.1), None);
        assert_eq!(time_at_distance(&r, f64::NAN), None);
        assert_eq!(time_at_distance(&r, f64::INFINITY), None);
    }

    #[test]
    fn delta_sign_faster_is_negative_slower_is_positive() {
        let r = reference();
        let faster = lap(&[(0.0, 0.0), (100.0, 9_500.0), (200.0, 19_000.0)]);
        let slower = lap(&[(0.0, 0.0), (100.0, 10_500.0), (200.0, 21_000.0)]);

        approx(delta_ms(&r, &faster, 100.0).unwrap(), -500.0);
        approx(delta_ms(&r, &slower, 100.0).unwrap(), 500.0);
        approx(delta_ms(&r, &faster, 150.0).unwrap(), -750.0);
        approx(delta_ms(&r, &slower, 150.0).unwrap(), 750.0);
        assert_eq!(delta_ms(&r, &faster, 300.0), None);
    }

    #[test]
    fn trace_length_and_step() {
        let r = reference();
        let trace = delta_trace(&r, &r, 50.0);
        let distances: Vec<f64> = trace.iter().map(|&(d, _)| d).collect();
        assert_eq!(distances, vec![0.0, 50.0, 100.0, 150.0, 200.0]);
        assert!(trace.iter().all(|&(_, d)| d.abs() < 1e-9));

        // Overlap of [20, 180] with step 50 gives 20, 70, 120, 170.
        let short = lap(&[(20.0, 2_000.0), (180.0, 18_000.0)]);
        let trace = delta_trace(&r, &short, 50.0);
        let distances: Vec<f64> = trace.iter().map(|&(d, _)| d).collect();
        assert_eq!(distances, vec![20.0, 70.0, 120.0, 170.0]);
    }

    #[test]
    fn trace_rejects_invalid_step_and_no_overlap() {
        let r = reference();
        assert!(delta_trace(&r, &r, 0.0).is_empty());
        assert!(delta_trace(&r, &r, -1.0).is_empty());
        assert!(delta_trace(&r, &r, f64::NAN).is_empty());
        assert!(delta_trace(&r, &r, f64::INFINITY).is_empty());

        let far = lap(&[(500.0, 0.0), (600.0, 1_000.0)]);
        assert!(delta_trace(&r, &far, 10.0).is_empty());
    }

    #[test]
    fn final_delta_uses_end_of_overlap() {
        let r = reference();
        let slower = lap(&[(0.0, 0.0), (100.0, 10_500.0), (200.0, 21_000.0)]);
        approx(final_delta_ms(&r, &slower).unwrap(), 1_000.0);

        // Current lap stops at 150 m, so the final delta is taken there.
        let partial = lap(&[(0.0, 0.0), (150.0, 15_250.0)]);
        approx(final_delta_ms(&r, &partial).unwrap(), 250.0);

        let far = lap(&[(500.0, 0.0), (600.0, 1_000.0)]);
        assert_eq!(final_delta_ms(&r, &far), None);
    }

    #[test]
    fn new_rejects_empty_input() {
        assert_eq!(Lap::new(Vec::new()), Err(DeltaError::EmptyLap));
    }

    #[test]
    fn new_rejects_non_monotonic_distance() {
        let samples = vec![
            Sample { distance_m: 0.0, time_ms: 0.0 },
            Sample { distance_m: 100.0, time_ms: 1_000.0 },
            Sample { distance_m: 50.0, time_ms: 2_000.0 },
        ];
        assert_eq!(
            Lap::new(samples),
            Err(DeltaError::NonMonotonicDistance { index: 2 })
        );
    }

    #[test]
    fn new_accepts_equal_distances() {
        let samples = vec![
            Sample { distance_m: 0.0, time_ms: 0.0 },
            Sample { distance_m: 0.0, time_ms: 10.0 },
            Sample { distance_m: 5.0, time_ms: 20.0 },
        ];
        assert!(Lap::new(samples).is_ok());
    }

    #[test]
    fn new_rejects_non_finite_values() {
        let nan_distance = vec![Sample { distance_m: f64::NAN, time_ms: 0.0 }];
        assert_eq!(
            Lap::new(nan_distance),
            Err(DeltaError::NonFiniteSample { index: 0 })
        );

        let inf_time = vec![
            Sample { distance_m: 0.0, time_ms: 0.0 },
            Sample { distance_m: 1.0, time_ms: f64::INFINITY },
        ];
        assert_eq!(
            Lap::new(inf_time),
            Err(DeltaError::NonFiniteSample { index: 1 })
        );
    }

    #[test]
    fn errors_have_display_and_implement_error() {
        let err: Box<dyn std::error::Error> = Box::new(DeltaError::EmptyLap);
        assert!(!err.to_string().is_empty());
        assert!(DeltaError::NonMonotonicDistance { index: 3 }
            .to_string()
            .contains('3'));
    }

    #[test]
    fn directly_constructed_empty_lap_does_not_panic() {
        let empty = Lap { samples: Vec::new() };
        let r = reference();
        assert_eq!(time_at_distance(&empty, 0.0), None);
        assert_eq!(delta_ms(&r, &empty, 0.0), None);
        assert!(delta_trace(&r, &empty, 10.0).is_empty());
        assert_eq!(final_delta_ms(&r, &empty), None);
    }
}
