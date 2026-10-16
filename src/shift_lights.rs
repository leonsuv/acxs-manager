//! Shift-light thresholds for a car.
//!
//! The LED thresholds are fractions of the rev limit. The default layout has
//! five LEDs, the first lighting at 20 % of the rev limit and the last at 90 %.
//! The intermediate LEDs are spaced evenly between those two points. An LED
//! lights when the RPM is strictly above its threshold.
//!
//! The rev limit is read from `data/engine.ini`, `[ENGINE_DATA]`, key `LIMITER`.

use std::fmt;
use std::path::Path;


/// Largest LED count accepted by [`profile_for`].
pub const MAX_LED_COUNT: usize = 64;
/// Default number of LEDs.
pub const DEFAULT_LED_COUNT: usize = 5;
/// Default first LED threshold as a fraction of the rev limit.
pub const DEFAULT_START_FRACTION: f64 = 0.2;
/// Default last LED threshold (the shift point) as a fraction of the rev limit.
pub const DEFAULT_END_FRACTION: f64 = 0.9;

/// LED thresholds for one car.
#[derive(Debug, Clone, PartialEq)]
pub struct ShiftProfile {
    /// Rev limit in RPM.
    pub rev_limit: f64,
    /// RPM threshold of each LED, ascending. LED `i` is lit when `rpm > leds[i]`.
    pub leds: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShiftError {
    /// Rev limit is not a finite number above zero.
    InvalidRevLimit(f64),
    /// LED count is outside `1..=MAX_LED_COUNT`.
    InvalidLedCount(usize),
    /// A fraction is outside `0..=1` (or is NaN).
    InvalidFraction(f64),
    /// The start fraction is not below the end fraction.
    InvalidRange { start: f64, end: f64 },
}

impl fmt::Display for ShiftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShiftError::InvalidRevLimit(v) => {
                write!(f, "rev limit must be a finite number above zero, got {v}")
            }
            ShiftError::InvalidLedCount(n) => {
                write!(f, "LED count must be between 1 and {MAX_LED_COUNT}, got {n}")
            }
            ShiftError::InvalidFraction(v) => {
                write!(f, "fraction must be between 0 and 1, got {v}")
            }
            ShiftError::InvalidRange { start, end } => {
                write!(f, "start fraction {start} must be below end fraction {end}")
            }
        }
    }
}

impl std::error::Error for ShiftError {}

/// Builds a profile with `led_count` LEDs spaced evenly from `start_fraction`
/// to `end_fraction` of `rev_limit_rpm`. With one LED, it sits at `end_fraction`
/// (the shift point).
pub fn profile_for(
    rev_limit_rpm: f64,
    led_count: usize,
    start_fraction: f64,
    end_fraction: f64,
) -> Result<ShiftProfile, ShiftError> {
    if !(rev_limit_rpm.is_finite() && rev_limit_rpm > 0.0) {
        return Err(ShiftError::InvalidRevLimit(rev_limit_rpm));
    }
    if !(1..=MAX_LED_COUNT).contains(&led_count) {
        return Err(ShiftError::InvalidLedCount(led_count));
    }
    for fraction in [start_fraction, end_fraction] {
        if !(0.0..=1.0).contains(&fraction) {
            return Err(ShiftError::InvalidFraction(fraction));
        }
    }
    if start_fraction >= end_fraction {
        return Err(ShiftError::InvalidRange {
            start: start_fraction,
            end: end_fraction,
        });
    }

    let leds = if led_count == 1 {
        vec![end_fraction * rev_limit_rpm]
    } else {
        let step = (end_fraction - start_fraction) / (led_count - 1) as f64;
        (0..led_count)
            .map(|i| {
                // The last LED is set exactly to the end fraction, avoiding float drift.
                let fraction = if i == led_count - 1 {
                    end_fraction
                } else {
                    start_fraction + step * i as f64
                };
                fraction * rev_limit_rpm
            })
            .collect()
    };

    Ok(ShiftProfile {
        rev_limit: rev_limit_rpm,
        leds,
    })
}

/// Which LEDs are lit at `rpm`. LED `i` is lit when `rpm > leds[i]`, so an LED
/// is still off exactly at its threshold.
pub fn led_states(profile: &ShiftProfile, rpm: f64) -> Vec<bool> {
    profile.leds.iter().map(|&threshold| rpm > threshold).collect()
}

/// RPM at which the last LED lights: the suggested shift point.
pub fn shift_point(profile: &ShiftProfile) -> f64 {
    profile.leds.last().copied().unwrap_or(profile.rev_limit)
}

/// Reads the rev limit from `<car_dir>/data/engine.ini`, `[ENGINE_DATA]`
/// `LIMITER`. Returns `None` if the file, the key or the value is missing or
/// not a finite number above zero.
pub fn read_rev_limit(car_dir: &Path) -> Option<f64> {
    let ini = crate::cardata::load_data_ini(car_dir, "engine.ini").ok()?;
    let limit = ini.get_f64("ENGINE_DATA", "LIMITER")?;
    (limit.is_finite() && limit > 0.0).then_some(limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Temporary car folder, unique per process and test, removed on drop.
    struct TempCar(PathBuf);

    impl TempCar {
        fn new(tag: &str) -> TempCar {
            let dir = std::env::temp_dir().join(format!(
                "acxs_shift_lights_{}_{}",
                std::process::id(),
                tag
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("data")).unwrap();
            TempCar(dir)
        }

        fn write_engine_ini(&self, text: &str) {
            fs::write(self.0.join("data").join("engine.ini"), text).unwrap();
        }
    }

    impl Drop for TempCar {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn profile_spacing_is_even_between_fractions() {
        // Step is (0.9 - 0.2) / 4 = 0.175 of 8000 = 1400 RPM.
        let p = profile_for(8000.0, 5, 0.2, 0.9).unwrap();
        assert_eq!(p.rev_limit, 8000.0);
        let expected = [1600.0, 3000.0, 4400.0, 5800.0, 7200.0];
        assert_eq!(p.leds.len(), expected.len());
        for (got, want) in p.leds.iter().zip(expected) {
            assert!(close(*got, want), "got {got}, want {want}");
        }
        for pair in p.leds.windows(2) {
            assert!(close(pair[1] - pair[0], 1400.0), "gap {}", pair[1] - pair[0]);
        }
    }

    #[test]
    fn profile_leds_are_ascending_and_last_is_end_fraction() {
        let p = profile_for(7777.0, 12, 0.25, 0.95).unwrap();
        assert!(p.leds.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(*p.leds.last().unwrap(), 0.95 * 7777.0);
        assert_eq!(p.leds[0], 0.25 * 7777.0);
    }

    #[test]
    fn single_led_sits_at_end_fraction() {
        let p = profile_for(6000.0, 1, 0.2, 0.9).unwrap();
        assert_eq!(p.leds.len(), 1);
        assert!(close(p.leds[0], 5400.0));
    }

    #[test]
    fn led_states_respect_strict_thresholds() {
        let p = profile_for(8000.0, 5, 0.2, 0.9).unwrap();
        // Below the first threshold: all off.
        assert_eq!(led_states(&p, 0.0), vec![false; 5]);
        assert_eq!(led_states(&p, 1599.0), vec![false; 5]);
        // Exactly at the first threshold: still off (strict `>`).
        assert_eq!(led_states(&p, 1600.0), vec![false; 5]);
        // Just above it: only the first LED.
        assert_eq!(led_states(&p, 1600.5), vec![true, false, false, false, false]);
        // Exactly at the second threshold (3000): still only the first LED.
        assert_eq!(led_states(&p, 3000.0), vec![true, false, false, false, false]);
        // Exactly at the last threshold: the last LED is still off.
        assert_eq!(led_states(&p, 7200.0), vec![true, true, true, true, false]);
        // Above the last threshold: all on.
        assert_eq!(led_states(&p, 7200.5), vec![true; 5]);
        assert_eq!(led_states(&p, 8000.0), vec![true; 5]);
    }

    #[test]
    fn led_states_are_a_lit_prefix() {
        let p = profile_for(8000.0, 8, 0.2, 0.9).unwrap();
        for rpm in [0.0, 2000.0, 3500.0, 5000.0, 6999.0, 9000.0] {
            let states = led_states(&p, rpm);
            let first_off = states.iter().position(|on| !on).unwrap_or(states.len());
            assert!(states[first_off..].iter().all(|on| !on), "rpm {rpm}: {states:?}");
        }
    }

    #[test]
    fn shift_point_is_last_led_threshold() {
        let p = profile_for(8000.0, 5, 0.2, 0.9).unwrap();
        assert!(close(shift_point(&p), 7200.0));
        let empty = ShiftProfile {
            rev_limit: 8000.0,
            leds: Vec::new(),
        };
        assert_eq!(shift_point(&empty), 8000.0);
    }

    #[test]
    fn default_constants_give_expected_shift_point() {
        assert_eq!(DEFAULT_LED_COUNT, 5);
        let p = profile_for(7000.0, DEFAULT_LED_COUNT, DEFAULT_START_FRACTION, DEFAULT_END_FRACTION)
            .unwrap();
        assert!(close(shift_point(&p), 6300.0));
    }

    #[test]
    fn validation_rejects_bad_led_counts() {
        assert_eq!(
            profile_for(8000.0, 0, 0.2, 0.9),
            Err(ShiftError::InvalidLedCount(0))
        );
        assert_eq!(
            profile_for(8000.0, 65, 0.2, 0.9),
            Err(ShiftError::InvalidLedCount(65))
        );
        assert!(profile_for(8000.0, 1, 0.2, 0.9).is_ok());
        assert!(profile_for(8000.0, 64, 0.2, 0.9).is_ok());
    }

    #[test]
    fn validation_rejects_bad_fractions() {
        assert!(matches!(
            profile_for(8000.0, 5, -0.1, 0.9),
            Err(ShiftError::InvalidFraction(_))
        ));
        assert!(matches!(
            profile_for(8000.0, 5, 0.2, 1.1),
            Err(ShiftError::InvalidFraction(_))
        ));
        assert!(matches!(
            profile_for(8000.0, 5, f64::NAN, 0.9),
            Err(ShiftError::InvalidFraction(_))
        ));
        // Boundary values 0 and 1 are allowed.
        assert!(profile_for(8000.0, 5, 0.0, 1.0).is_ok());
    }

    #[test]
    fn validation_rejects_inverted_or_equal_range() {
        assert_eq!(
            profile_for(8000.0, 5, 0.9, 0.2),
            Err(ShiftError::InvalidRange { start: 0.9, end: 0.2 })
        );
        assert_eq!(
            profile_for(8000.0, 5, 0.5, 0.5),
            Err(ShiftError::InvalidRange { start: 0.5, end: 0.5 })
        );
    }

    #[test]
    fn validation_rejects_bad_rev_limit() {
        assert_eq!(
            profile_for(0.0, 5, 0.2, 0.9),
            Err(ShiftError::InvalidRevLimit(0.0))
        );
        assert_eq!(
            profile_for(-100.0, 5, 0.2, 0.9),
            Err(ShiftError::InvalidRevLimit(-100.0))
        );
        assert!(matches!(
            profile_for(f64::NAN, 5, 0.2, 0.9),
            Err(ShiftError::InvalidRevLimit(_))
        ));
    }

    #[test]
    fn errors_have_display_messages() {
        let msg = ShiftError::InvalidLedCount(99).to_string();
        assert!(msg.contains("99"));
        let _: &dyn std::error::Error = &ShiftError::InvalidRevLimit(0.0);
    }

    #[test]
    fn read_rev_limit_reads_limiter() {
        let car = TempCar::new("reads_limiter");
        car.write_engine_ini("[HEADER]\r\nPOWER_CURVE=power.lut\r\n\r\n[ENGINE_DATA]\r\nMINIMUM=900\r\nLIMITER=7500\r\n");
        assert_eq!(read_rev_limit(&car.0), Some(7500.0));
    }

    #[test]
    fn read_rev_limit_key_is_case_insensitive() {
        let car = TempCar::new("case_insensitive");
        car.write_engine_ini("[engine_data]\nlimiter = 6800.5\n");
        assert_eq!(read_rev_limit(&car.0), Some(6800.5));
    }

    #[test]
    fn read_rev_limit_missing_file_is_none() {
        let car = TempCar::new("missing_file");
        assert_eq!(read_rev_limit(&car.0), None);
        let absent = std::env::temp_dir().join(format!(
            "acxs_shift_lights_{}_no_such_car",
            std::process::id()
        ));
        assert_eq!(read_rev_limit(&absent), None);
    }

    #[test]
    fn read_rev_limit_missing_or_bad_value_is_none() {
        let car = TempCar::new("missing_key");
        car.write_engine_ini("[ENGINE_DATA]\nMINIMUM=900\n");
        assert_eq!(read_rev_limit(&car.0), None);

        car.write_engine_ini("[ENGINE_DATA]\nLIMITER=0\n");
        assert_eq!(read_rev_limit(&car.0), None);

        car.write_engine_ini("[ENGINE_DATA]\nLIMITER=abc\n");
        assert_eq!(read_rev_limit(&car.0), None);

        car.write_engine_ini("[OTHER]\nLIMITER=7000\n");
        assert_eq!(read_rev_limit(&car.0), None);
    }
}
