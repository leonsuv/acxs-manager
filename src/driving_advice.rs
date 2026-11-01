//! Setup suggestions from recorded sessions.
//!
//! Rule based and cautious: a car needs enough recorded driving before anything is said, and
//! every suggestion carries the number that triggered it.

use std::collections::BTreeMap;

use crate::session_recorder::RecordedSession;

/// Hard-cornering seconds needed per car before balance advice is given.
const MIN_CORNERING_S: f64 = 60.0;
/// Driving minutes needed per car before lock-up and wheelspin advice is given.
const MIN_MINUTES: f64 = 15.0;
/// Share of judged cornering time in under- or oversteer that triggers advice.
const BALANCE_SHARE: f64 = 0.25;
/// Events per 10 driving minutes that trigger advice.
const LOCKUPS_PER_10_MIN: f64 = 3.0;
const WHEELSPINS_PER_10_MIN: f64 = 4.0;

/// One suggestion for one car.
#[derive(Debug, Clone, PartialEq)]
pub struct Advice {
    pub car_id: String,
    /// What was seen, with the number.
    pub finding: String,
    /// What to try.
    pub suggestion: String,
}

#[derive(Default)]
struct Totals {
    seconds: f64,
    cornering: f64,
    understeer: f64,
    oversteer: f64,
    front: u32,
    rear: u32,
    spins: u32,
}

/// Suggestions for every car with enough data, ordered by car.
pub fn advise(sessions: &[RecordedSession]) -> Vec<Advice> {
    let mut by_car: BTreeMap<&str, Totals> = BTreeMap::new();
    for s in sessions {
        let t = by_car.entry(s.car_id.as_str()).or_default();
        t.seconds += s.seconds;
        t.cornering += s.cornering_seconds;
        t.understeer += s.understeer_seconds;
        t.oversteer += s.oversteer_seconds;
        t.front += s.front_lockups;
        t.rear += s.rear_lockups;
        t.spins += s.wheelspins;
    }
    let mut out = Vec::new();
    for (car, t) in by_car {
        let mut add = |finding: String, suggestion: &str| {
            out.push(Advice { car_id: car.to_string(), finding, suggestion: suggestion.to_string() });
        };
        if t.cornering >= MIN_CORNERING_S {
            let under = t.understeer / t.cornering;
            let over = t.oversteer / t.cornering;
            if under >= BALANCE_SHARE && under >= over {
                add(
                    format!("Understeer in {:.0}% of hard cornering", under * 100.0),
                    "Soften the front anti-roll bar or springs, add front wing or front toe-out, or lower the differential preload.",
                );
            } else if over >= BALANCE_SHARE {
                add(
                    format!("Oversteer in {:.0}% of hard cornering", over * 100.0),
                    "Stiffen the front anti-roll bar, soften the rear one, add rear wing, raise traction control, or raise the differential preload.",
                );
            }
        }
        let minutes = t.seconds / 60.0;
        if minutes >= MIN_MINUTES {
            let per10 = |n: u32| f64::from(n) / minutes * 10.0;
            if per10(t.front) >= LOCKUPS_PER_10_MIN && t.front >= t.rear {
                add(
                    format!("{:.1} front brake lock-ups per 10 minutes", per10(t.front)),
                    "Move brake bias towards the rear, lower brake pressure, or raise ABS.",
                );
            } else if per10(t.rear) >= LOCKUPS_PER_10_MIN {
                add(
                    format!("{:.1} rear brake lock-ups per 10 minutes", per10(t.rear)),
                    "Move brake bias towards the front or lower engine braking.",
                );
            }
            if per10(t.spins) >= WHEELSPINS_PER_10_MIN {
                add(
                    format!("{:.1} wheelspin events per 10 minutes", per10(t.spins)),
                    "Raise traction control, be gentler on the throttle at corner exit, or check the differential power setting.",
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(car: &str) -> RecordedSession {
        RecordedSession { car_id: car.into(), seconds: 1200.0, ..RecordedSession::default() }
    }

    #[test]
    fn too_little_data_gives_no_advice() {
        let mut s = session("a");
        s.seconds = 300.0;
        s.cornering_seconds = 30.0;
        s.understeer_seconds = 30.0;
        s.front_lockups = 50;
        assert!(advise(&[s]).is_empty());
    }

    #[test]
    fn understeer_and_lockups_are_reported_with_numbers() {
        let mut s = session("a");
        s.cornering_seconds = 100.0;
        s.understeer_seconds = 40.0;
        s.oversteer_seconds = 5.0;
        s.front_lockups = 8;
        let advice = advise(&[s]);
        assert_eq!(advice.len(), 2);
        assert!(advice[0].finding.contains("40%"));
        assert!(advice[1].finding.contains("front brake lock-ups"));
    }

    #[test]
    fn cars_are_judged_separately_and_sessions_pooled() {
        let mut a = session("a");
        a.cornering_seconds = 40.0;
        a.oversteer_seconds = 20.0;
        let b = a.clone();
        let mut other = session("b");
        other.cornering_seconds = 100.0;
        let advice = advise(&[a, b, other]);
        assert_eq!(advice.len(), 1);
        assert_eq!(advice[0].car_id, "a");
        assert!(advice[0].finding.starts_with("Oversteer in 50%"));
    }
}
