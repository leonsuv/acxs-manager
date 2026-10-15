//! Race strategy helpers: fuel use, laps covered by a tank, the pit window and
//! a stop plan.
//!
//! Everything here is a pure function over typed inputs. Fuel is in litres and
//! laps are whole numbers. Zero, negative, non-finite or contradictory input
//! comes back as a [`StrategyError`]; nothing panics.

use std::fmt;

/// Slack added before flooring a lap count, so that float noise such as
/// `0.3 / 0.1 = 2.9999999999999996` still counts as three laps.
const LAP_EPSILON: f64 = 1e-9;

/// Why a strategy calculation was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum StrategyError {
    /// The named value must be greater than zero.
    NotPositive(&'static str),
    /// The named value must be zero or greater.
    Negative(&'static str),
    /// The named value must be a finite number.
    NotFinite(&'static str),
    /// Fewer fuel readings than the calculation needs.
    TooFewReadings { needed: usize, got: usize },
    /// The readings show no fuel being used, so a per-lap rate cannot be found.
    NoFuelUsed,
    /// More laps completed than the race has.
    CompletedPastEnd { completed: u32, race_laps: u32 },
    /// A full tank does not cover one lap at this consumption.
    TankTooSmall { capacity: f64, fuel_per_lap: f64 },
    /// The plan would need a stint shorter than the minimum.
    StintTooShort { shortest: u32, min_stint: u32 },
}

impl fmt::Display for StrategyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrategyError::NotPositive(name) => write!(f, "{name} must be greater than zero."),
            StrategyError::Negative(name) => write!(f, "{name} cannot be negative."),
            StrategyError::NotFinite(name) => write!(f, "{name} must be a finite number."),
            StrategyError::TooFewReadings { needed, got } => write!(
                f,
                "Need at least {needed} fuel readings after the last refuel, got {got}."
            ),
            StrategyError::NoFuelUsed => write!(
                f,
                "The readings show no fuel used, so fuel per lap cannot be worked out."
            ),
            StrategyError::CompletedPastEnd { completed, race_laps } => write!(
                f,
                "Laps completed ({completed}) is more than the race length ({race_laps})."
            ),
            StrategyError::TankTooSmall { capacity, fuel_per_lap } => write!(
                f,
                "A tank of {capacity:.2} L does not cover one lap at {fuel_per_lap:.2} L."
            ),
            StrategyError::StintTooShort { shortest, min_stint } => write!(
                f,
                "A stint would be {shortest} laps, below the minimum of {min_stint}."
            ),
        }
    }
}

impl std::error::Error for StrategyError {}

fn positive(name: &'static str, value: f64) -> Result<f64, StrategyError> {
    if !value.is_finite() {
        return Err(StrategyError::NotFinite(name));
    }
    if value <= 0.0 {
        return Err(StrategyError::NotPositive(name));
    }
    Ok(value)
}

fn non_negative(name: &'static str, value: f64) -> Result<f64, StrategyError> {
    if !value.is_finite() {
        return Err(StrategyError::NotFinite(name));
    }
    if value < 0.0 {
        return Err(StrategyError::Negative(name));
    }
    Ok(value)
}

fn positive_laps(name: &'static str, laps: u32) -> Result<u32, StrategyError> {
    if laps == 0 {
        return Err(StrategyError::NotPositive(name));
    }
    Ok(laps)
}

/// Whole laps in a (possibly fractional) lap count, rounded down.
fn whole_laps(laps: f64) -> u32 {
    (laps + LAP_EPSILON).floor() as u32
}

/// Average fuel used per lap, from fuel levels read at the end of consecutive
/// laps, oldest first.
///
/// A rise between two readings means the car was refuelled. Only the readings
/// from the rise onwards are used, so a refuel in the middle of the window does
/// not hide the consumption before or after it.
pub fn fuel_per_lap(levels: &[f64]) -> Result<f64, StrategyError> {
    for &level in levels {
        non_negative("fuel reading", level)?;
    }
    let start = levels
        .windows(2)
        .rposition(|pair| pair[1] > pair[0])
        .map_or(0, |i| i + 1);
    let run = &levels[start..];
    if run.len() < 2 {
        return Err(StrategyError::TooFewReadings {
            needed: 2,
            got: run.len(),
        });
    }
    let used = run[0] - run[run.len() - 1];
    let per_lap = used / (run.len() - 1) as f64;
    if per_lap <= 0.0 {
        return Err(StrategyError::NoFuelUsed);
    }
    Ok(per_lap)
}

/// Whole laps that `fuel_left` litres cover at `fuel_per_lap` litres a lap.
pub fn fuel_laps(fuel_left: f64, fuel_per_lap: f64) -> Result<u32, StrategyError> {
    let fuel_left = non_negative("fuel left", fuel_left)?;
    let fuel_per_lap = positive("fuel per lap", fuel_per_lap)?;
    Ok(whole_laps(fuel_left / fuel_per_lap))
}

/// How the fuel in the tank compares with the laps still to run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuelStatus {
    /// Laps still to run in the race.
    pub laps_to_go: u32,
    /// Whole laps the fuel in the tank covers.
    pub laps_covered: u32,
    /// Laps the fuel does not cover. Zero when the fuel is enough.
    pub laps_short: u32,
    /// Fuel the remaining laps need, in litres.
    pub fuel_needed: f64,
    /// Fuel in the tank minus fuel needed, in litres. Negative when short.
    pub surplus: f64,
}

/// Compares `fuel_left` with the laps left in a race of `race_laps` laps,
/// of which `completed_laps` are done.
pub fn fuel_status(
    fuel_left: f64,
    fuel_per_lap: f64,
    race_laps: u32,
    completed_laps: u32,
) -> Result<FuelStatus, StrategyError> {
    let fuel_left = non_negative("fuel left", fuel_left)?;
    let fuel_per_lap = positive("fuel per lap", fuel_per_lap)?;
    let race_laps = positive_laps("race laps", race_laps)?;
    if completed_laps > race_laps {
        return Err(StrategyError::CompletedPastEnd {
            completed: completed_laps,
            race_laps,
        });
    }
    let laps_to_go = race_laps - completed_laps;
    let laps_covered = whole_laps(fuel_left / fuel_per_lap);
    let fuel_needed = laps_to_go as f64 * fuel_per_lap;
    Ok(FuelStatus {
        laps_to_go,
        laps_covered,
        laps_short: laps_to_go.saturating_sub(laps_covered),
        fuel_needed,
        surplus: fuel_left - fuel_needed,
    })
}

/// Laps on which a pit stop is viable, counted as laps completed when the car
/// enters the pit lane. Both ends are inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitWindow {
    pub first_lap: u32,
    pub last_lap: u32,
}

/// The laps where a single stop is viable.
///
/// A stop after lap `L` is viable when the stint before it is at least
/// `min_stint_laps` long, the tyres last until `L` (`L <= tyre_life_laps`),
/// and the stint after it is also at least `min_stint_laps` long. Returns
/// `Ok(None)` when no lap meets all three.
pub fn pit_window(
    race_laps: u32,
    tyre_life_laps: u32,
    min_stint_laps: u32,
) -> Result<Option<PitWindow>, StrategyError> {
    let race_laps = positive_laps("race laps", race_laps)?;
    let tyre_life_laps = positive_laps("tyre life laps", tyre_life_laps)?;
    let min_stint = positive_laps("minimum stint laps", min_stint_laps)?;
    let Some(latest_by_race) = race_laps.checked_sub(min_stint) else {
        return Ok(None);
    };
    let first_lap = min_stint;
    let last_lap = tyre_life_laps.min(latest_by_race);
    Ok((first_lap <= last_lap).then_some(PitWindow {
        first_lap,
        last_lap,
    }))
}

/// One stint of a plan. Laps are race lap numbers, counted from 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Stint {
    /// 1 for the stint from the start, and so on.
    pub number: u32,
    pub first_lap: u32,
    pub last_lap: u32,
    pub laps: u32,
    /// Fuel to load for this stint, in litres. Never more than the tank holds.
    pub fuel_load: f64,
}

/// A full-race stop plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PitPlan {
    pub stints: Vec<Stint>,
}

impl PitPlan {
    /// Number of stops. One stint means no stops.
    pub fn stops(&self) -> u32 {
        self.stints.len().saturating_sub(1) as u32
    }

    /// The lap after which the car stops for each stop, in order.
    pub fn stop_laps(&self) -> Vec<u32> {
        let stops = self.stints.len().saturating_sub(1);
        self.stints.iter().take(stops).map(|s| s.last_lap).collect()
    }
}

/// Plans the fewest stops that cover `race_laps` laps.
///
/// A stint can be no longer than the tyres last (`tyre_life_laps`) or the
/// tank's worth of fuel at `fuel_per_lap`, whichever is shorter. Stints are
/// split as evenly as the lap count allows. When the plan has stops, every
/// stint must be at least `min_stint_laps` long; a race that needs no stop
/// is not held to that minimum.
pub fn pit_plan(
    race_laps: u32,
    fuel_capacity: f64,
    fuel_per_lap: f64,
    tyre_life_laps: u32,
    min_stint_laps: u32,
) -> Result<PitPlan, StrategyError> {
    let race_laps = positive_laps("race laps", race_laps)?;
    let fuel_capacity = positive("fuel capacity", fuel_capacity)?;
    let fuel_per_lap = positive("fuel per lap", fuel_per_lap)?;
    let tyre_life_laps = positive_laps("tyre life laps", tyre_life_laps)?;
    let min_stint = positive_laps("minimum stint laps", min_stint_laps)?;

    let fuel_limit = whole_laps(fuel_capacity / fuel_per_lap);
    if fuel_limit == 0 {
        return Err(StrategyError::TankTooSmall {
            capacity: fuel_capacity,
            fuel_per_lap,
        });
    }
    let longest_stint = fuel_limit.min(tyre_life_laps);
    let count = race_laps.div_ceil(longest_stint);
    let base = race_laps / count;
    let extra = race_laps % count;
    if count > 1 && base < min_stint {
        return Err(StrategyError::StintTooShort {
            shortest: base,
            min_stint,
        });
    }

    let mut stints = Vec::with_capacity(count as usize);
    let mut first_lap = 1;
    for number in 1..=count {
        let laps = if number <= extra { base + 1 } else { base };
        let last_lap = first_lap + laps - 1;
        stints.push(Stint {
            number,
            first_lap,
            last_lap,
            laps,
            fuel_load: (laps as f64 * fuel_per_lap).min(fuel_capacity),
        });
        first_lap = last_lap + 1;
    }
    Ok(PitPlan { stints })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn strategy_fuel_per_lap_averages_consumption() {
        let per_lap = fuel_per_lap(&[50.0, 48.0, 46.0, 44.0]).unwrap();
        assert!(close(per_lap, 2.0));
    }

    #[test]
    fn strategy_fuel_per_lap_ignores_readings_before_refuel() {
        let per_lap = fuel_per_lap(&[50.0, 48.0, 46.0, 70.0, 68.0, 66.0]).unwrap();
        assert!(close(per_lap, 2.0));
    }

    #[test]
    fn strategy_fuel_per_lap_needs_two_readings_after_refuel() {
        assert_eq!(
            fuel_per_lap(&[]),
            Err(StrategyError::TooFewReadings { needed: 2, got: 0 })
        );
        assert_eq!(
            fuel_per_lap(&[40.0]),
            Err(StrategyError::TooFewReadings { needed: 2, got: 1 })
        );
        assert_eq!(
            fuel_per_lap(&[40.0, 70.0]),
            Err(StrategyError::TooFewReadings { needed: 2, got: 1 })
        );
    }

    #[test]
    fn strategy_fuel_per_lap_rejects_no_consumption() {
        assert_eq!(fuel_per_lap(&[40.0, 40.0, 40.0]), Err(StrategyError::NoFuelUsed));
    }

    #[test]
    fn strategy_fuel_per_lap_rejects_bad_readings() {
        assert_eq!(
            fuel_per_lap(&[40.0, -1.0]),
            Err(StrategyError::Negative("fuel reading"))
        );
        assert_eq!(
            fuel_per_lap(&[40.0, f64::NAN]),
            Err(StrategyError::NotFinite("fuel reading"))
        );
    }

    #[test]
    fn strategy_fuel_laps_rounds_down() {
        assert_eq!(fuel_laps(10.0, 2.5), Ok(4));
        assert_eq!(fuel_laps(9.9, 2.5), Ok(3));
        assert_eq!(fuel_laps(0.0, 2.5), Ok(0));
        // Float noise must not cost a lap.
        assert_eq!(fuel_laps(0.3, 0.1), Ok(3));
    }

    #[test]
    fn strategy_fuel_laps_rejects_invalid_values() {
        assert_eq!(fuel_laps(10.0, 0.0), Err(StrategyError::NotPositive("fuel per lap")));
        assert_eq!(fuel_laps(10.0, -2.0), Err(StrategyError::NotPositive("fuel per lap")));
        assert_eq!(fuel_laps(-1.0, 2.0), Err(StrategyError::Negative("fuel left")));
        assert_eq!(fuel_laps(f64::INFINITY, 2.0), Err(StrategyError::NotFinite("fuel left")));
    }

    #[test]
    fn strategy_fuel_status_exact_fit() {
        let status = fuel_status(20.0, 2.0, 15, 5).unwrap();
        assert_eq!(status.laps_to_go, 10);
        assert_eq!(status.laps_covered, 10);
        assert_eq!(status.laps_short, 0);
        assert!(close(status.fuel_needed, 20.0));
        assert!(close(status.surplus, 0.0));
    }

    #[test]
    fn strategy_fuel_status_short() {
        let status = fuel_status(15.0, 2.0, 15, 0).unwrap();
        assert_eq!(status.laps_covered, 7);
        assert_eq!(status.laps_short, 8);
        assert!(close(status.surplus, -15.0));
    }

    #[test]
    fn strategy_fuel_status_race_finished() {
        let status = fuel_status(1.0, 2.0, 15, 15).unwrap();
        assert_eq!(status.laps_to_go, 0);
        assert_eq!(status.laps_short, 0);
        assert!(close(status.fuel_needed, 0.0));
    }

    #[test]
    fn strategy_fuel_status_rejects_invalid_values() {
        assert_eq!(
            fuel_status(10.0, 2.0, 0, 0),
            Err(StrategyError::NotPositive("race laps"))
        );
        assert_eq!(
            fuel_status(10.0, 2.0, 15, 16),
            Err(StrategyError::CompletedPastEnd {
                completed: 16,
                race_laps: 15
            })
        );
        assert_eq!(
            fuel_status(10.0, 0.0, 15, 0),
            Err(StrategyError::NotPositive("fuel per lap"))
        );
    }

    #[test]
    fn strategy_pit_window_between_min_stint_and_tyre_life() {
        let window = pit_window(30, 12, 5).unwrap();
        assert_eq!(
            window,
            Some(PitWindow {
                first_lap: 5,
                last_lap: 12
            })
        );
    }

    #[test]
    fn strategy_pit_window_capped_by_final_stint() {
        // Tyres would last to lap 30, but the last stint must be 5 laps.
        let window = pit_window(20, 30, 5).unwrap();
        assert_eq!(
            window,
            Some(PitWindow {
                first_lap: 5,
                last_lap: 15
            })
        );
    }

    #[test]
    fn strategy_pit_window_empty_when_race_too_short() {
        assert_eq!(pit_window(8, 20, 5).unwrap(), None);
        assert_eq!(pit_window(5, 20, 5).unwrap(), None);
        assert_eq!(pit_window(3, 20, 5).unwrap(), None);
    }

    #[test]
    fn strategy_pit_window_rejects_zero() {
        assert_eq!(pit_window(0, 10, 5), Err(StrategyError::NotPositive("race laps")));
        assert_eq!(
            pit_window(30, 0, 5),
            Err(StrategyError::NotPositive("tyre life laps"))
        );
        assert_eq!(
            pit_window(30, 10, 0),
            Err(StrategyError::NotPositive("minimum stint laps"))
        );
    }

    #[test]
    fn strategy_pit_plan_one_stop_split_evenly() {
        let plan = pit_plan(30, 60.0, 3.0, 15, 5).unwrap();
        assert_eq!(plan.stops(), 1);
        assert_eq!(plan.stop_laps(), vec![15]);
        assert_eq!(plan.stints.len(), 2);
        assert_eq!(plan.stints[0].first_lap, 1);
        assert_eq!(plan.stints[0].last_lap, 15);
        assert!(close(plan.stints[0].fuel_load, 45.0));
        assert_eq!(plan.stints[1].first_lap, 16);
        assert_eq!(plan.stints[1].last_lap, 30);
    }

    #[test]
    fn strategy_pit_plan_no_stop_when_tank_and_tyres_last() {
        let plan = pit_plan(20, 50.0, 2.5, 30, 5).unwrap();
        assert_eq!(plan.stops(), 0);
        assert!(plan.stop_laps().is_empty());
        assert_eq!(plan.stints[0].laps, 20);
    }

    #[test]
    fn strategy_pit_plan_spreads_remainder_over_first_stints() {
        // 31 laps, stints of at most 12: three stints of 11, 10 and 10.
        let plan = pit_plan(31, 100.0, 1.0, 12, 5).unwrap();
        let laps: Vec<u32> = plan.stints.iter().map(|s| s.laps).collect();
        assert_eq!(laps, vec![11, 10, 10]);
        assert_eq!(plan.stop_laps(), vec![11, 21]);
        assert_eq!(plan.stops(), 2);
    }

    #[test]
    fn strategy_pit_plan_fuel_limit_shortens_stints() {
        // Tank covers 9 laps at 4 L a lap, tyres last 20: 27 laps needs 3 stints of 9.
        let plan = pit_plan(27, 36.0, 4.0, 20, 5).unwrap();
        assert_eq!(plan.stops(), 2);
        assert!(plan.stints.iter().all(|s| s.laps == 9));
        assert!(plan.stints.iter().all(|s| close(s.fuel_load, 36.0)));
    }

    #[test]
    fn strategy_pit_plan_rejects_stints_below_minimum() {
        // 10 laps with a 9-lap longest stint forces stints of 5 and 5.
        assert_eq!(
            pit_plan(10, 100.0, 1.0, 9, 6),
            Err(StrategyError::StintTooShort {
                shortest: 5,
                min_stint: 6
            })
        );
    }

    #[test]
    fn strategy_pit_plan_rejects_tank_below_one_lap() {
        assert_eq!(
            pit_plan(10, 2.0, 3.0, 10, 1),
            Err(StrategyError::TankTooSmall {
                capacity: 2.0,
                fuel_per_lap: 3.0
            })
        );
    }

    #[test]
    fn strategy_pit_plan_rejects_invalid_values() {
        assert_eq!(
            pit_plan(0, 60.0, 3.0, 10, 5),
            Err(StrategyError::NotPositive("race laps"))
        );
        assert_eq!(
            pit_plan(30, 0.0, 3.0, 10, 5),
            Err(StrategyError::NotPositive("fuel capacity"))
        );
        assert_eq!(
            pit_plan(30, 60.0, -3.0, 10, 5),
            Err(StrategyError::NotPositive("fuel per lap"))
        );
        assert_eq!(
            pit_plan(30, f64::NAN, 3.0, 10, 5),
            Err(StrategyError::NotFinite("fuel capacity"))
        );
    }

    #[test]
    fn strategy_pit_plan_covers_every_lap_within_limits() {
        for race in 1..=80u32 {
            let plan = match pit_plan(race, 63.0, 7.0, 7, 2) {
                Ok(plan) => plan,
                Err(StrategyError::StintTooShort { .. }) => continue,
                Err(other) => panic!("unexpected error for {race} laps: {other}"),
            };
            let total: u32 = plan.stints.iter().map(|s| s.laps).sum();
            assert_eq!(total, race);
            let mut expected_first = 1;
            for stint in &plan.stints {
                assert_eq!(stint.first_lap, expected_first);
                assert!(stint.laps <= 7, "stint longer than tyres for {race} laps");
                assert!(stint.fuel_load <= 63.0 + 1e-9);
                if plan.stops() > 0 {
                    assert!(stint.laps >= 2, "stint below minimum for {race} laps");
                }
                expected_first = stint.last_lap + 1;
            }
        }
    }

    #[test]
    fn strategy_errors_have_messages() {
        let errors = [
            StrategyError::NotPositive("race laps"),
            StrategyError::Negative("fuel left"),
            StrategyError::NotFinite("fuel reading"),
            StrategyError::TooFewReadings { needed: 2, got: 1 },
            StrategyError::NoFuelUsed,
            StrategyError::CompletedPastEnd {
                completed: 3,
                race_laps: 2,
            },
            StrategyError::TankTooSmall {
                capacity: 1.0,
                fuel_per_lap: 2.0,
            },
            StrategyError::StintTooShort {
                shortest: 2,
                min_stint: 5,
            },
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
        }
    }
}
