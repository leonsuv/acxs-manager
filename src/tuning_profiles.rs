//! Named tuning profiles: an ordered list of tuning steps that can be saved as
//! JSON, loaded back and applied to a car folder through [`car_tuning`].
//!
//! Each step is a [`Tuning`] kind plus the value it uses. The number inside the
//! `kind` is replaced by `value`, so `value` alone decides the amount and the
//! kind decides the operation and axle. Values are checked against the bounds in
//! [`car_tuning`] when a profile is loaded or saved, and again by `car_tuning`
//! when a step runs.
//!
//! [`apply_profile`] runs the steps in order. It stops at the first failing step
//! and reports the changes made by the steps before it. Nothing is deleted:
//! every file write goes through `car_tuning`, which keeps a backup.
//! Profiles are saved as new files only, under `<AC root>/acxs-tuning-profiles/`.

use crate::car_tuning::{
    self, Axle, Change, MAX_BASE_HEIGHT_M, MAX_CAMBER_DEG, MAX_FACTOR, MAX_FUEL_L, MAX_MASS_KG,
    MAX_PRESSURE_PSI, MIN_BASE_HEIGHT_M, MIN_CAMBER_DEG, MIN_FACTOR, MIN_FUEL_L, MIN_MASS_KG,
    MIN_PRESSURE_PSI, TuneError, Tuning,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Folder under the AC root where profiles are saved.
pub const PROFILE_DIR: &str = "acxs-tuning-profiles";

/// One tuning operation and the value it uses, applied in profile order.
#[derive(Debug, Clone, PartialEq)]
pub struct TuningStep {
    pub kind: Tuning,
    pub value: f64,
}

/// A named list of tuning steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningProfile {
    pub name: String,
    pub steps: Vec<TuningStep>,
}

/// A profile run that stopped at a failing step.
#[derive(Debug)]
pub struct ApplyProfileError {
    /// 1-based position of the step that failed.
    pub step: usize,
    /// Changes written by the steps before the failing one.
    pub changes: Vec<Change>,
    pub error: TuneError,
}

impl fmt::Display for ApplyProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "step {} failed: {} ({} change(s) were written before it)",
            self.step,
            self.error,
            self.changes.len()
        )
    }
}

impl std::error::Error for ApplyProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Serialized form of a step: a flat object with `kind`, an optional `axle` and `value`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepRecord {
    kind: StepKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    axle: Option<StepAxle>,
    value: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepKind {
    TorqueCurve,
    TotalMass,
    FuelTank,
    GearRatios,
    FinalDrive,
    IdealPressure,
    StaticPressure,
    Camber,
    BaseHeight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StepAxle {
    Front,
    Rear,
}

impl TryFrom<StepRecord> for TuningStep {
    type Error = String;

    fn try_from(record: StepRecord) -> Result<Self, String> {
        let kind = build(record.kind, record.axle, record.value)?;
        check_value(kind).map_err(|e| e.to_string())?;
        Ok(TuningStep {
            kind,
            value: record.value,
        })
    }
}

impl From<TuningStep> for StepRecord {
    fn from(step: TuningStep) -> StepRecord {
        let (kind, axle) = split(step.kind);
        StepRecord {
            kind,
            axle,
            value: step.value,
        }
    }
}

// `TuningStep` serializes through `StepRecord`, so the flat JSON form is the only one.
impl Serialize for TuningStep {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StepRecord::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TuningStep {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let record = StepRecord::deserialize(deserializer)?;
        TuningStep::try_from(record).map_err(serde::de::Error::custom)
    }
}

/// Saves the profile as `<name>.json` in `<ac_root>/acxs-tuning-profiles/`.
///
/// The file is created new, so an existing profile is never overwritten. The
/// name is reduced to letters, digits, spaces, `-` and `_`. Every step is checked
/// against the bounds first. Returns the path written.
pub fn save_profile(ac_root: &Path, profile: &TuningProfile) -> io::Result<PathBuf> {
    let stem = file_stem(&profile.name)?;
    for step in &profile.steps {
        check_value(with_value(step.kind, step.value)).map_err(invalid)?;
    }
    let dir = ac_root.join(PROFILE_DIR);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{stem}.json"));
    let json = serde_json::to_string_pretty(profile).map_err(io::Error::other)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    file.write_all(json.as_bytes())?;
    Ok(path)
}

/// Reads a profile JSON file. Every step is checked against the bounds in
/// [`car_tuning`]; a bad step or an unknown field is an `InvalidData` error.
pub fn load_profile(path: &Path) -> io::Result<TuningProfile> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Runs the steps on the car in `car_dir`, in order.
///
/// Stops at the first step that fails. The error holds that step's number, the
/// error itself and the changes the earlier steps made, so the caller can report
/// what was written.
pub fn apply_profile(car_dir: &Path, profile: &TuningProfile) -> Result<Vec<Change>, ApplyProfileError> {
    let mut changes = Vec::new();
    for (index, step) in profile.steps.iter().enumerate() {
        match car_tuning::apply(car_dir, with_value(step.kind, step.value)) {
            Ok(mut done) => changes.append(&mut done),
            Err(error) => {
                return Err(ApplyProfileError {
                    step: index + 1,
                    changes,
                    error,
                });
            }
        }
    }
    Ok(changes)
}

/// Splits a tuning into its serialized kind and axle. The number is not kept.
fn split(kind: Tuning) -> (StepKind, Option<StepAxle>) {
    match kind {
        Tuning::TorqueCurve(_) => (StepKind::TorqueCurve, None),
        Tuning::TotalMass(_) => (StepKind::TotalMass, None),
        Tuning::FuelTank(_) => (StepKind::FuelTank, None),
        Tuning::GearRatios(_) => (StepKind::GearRatios, None),
        Tuning::FinalDrive(_) => (StepKind::FinalDrive, None),
        Tuning::IdealPressure { axle, .. } => (StepKind::IdealPressure, Some(step_axle(axle))),
        Tuning::StaticPressure { axle, .. } => (StepKind::StaticPressure, Some(step_axle(axle))),
        Tuning::Camber { axle, .. } => (StepKind::Camber, Some(step_axle(axle))),
        Tuning::BaseHeight { axle, .. } => (StepKind::BaseHeight, Some(step_axle(axle))),
    }
}

/// Builds a tuning from a serialized kind. Per-axle kinds need an axle and the
/// others must not have one, so a mistake in the file is an error.
fn build(kind: StepKind, axle: Option<StepAxle>, value: f64) -> Result<Tuning, String> {
    let per_axle = matches!(
        kind,
        StepKind::IdealPressure | StepKind::StaticPressure | StepKind::Camber | StepKind::BaseHeight
    );
    match (per_axle, axle) {
        (true, None) => return Err(format!("{kind:?} needs an axle of front or rear")),
        (false, Some(_)) => return Err(format!("{kind:?} takes no axle")),
        _ => {}
    }
    let axle = axle.map_or(Axle::Front, to_axle);
    Ok(match kind {
        StepKind::TorqueCurve => Tuning::TorqueCurve(value),
        StepKind::TotalMass => Tuning::TotalMass(value),
        StepKind::FuelTank => Tuning::FuelTank(value),
        StepKind::GearRatios => Tuning::GearRatios(value),
        StepKind::FinalDrive => Tuning::FinalDrive(value),
        StepKind::IdealPressure => Tuning::IdealPressure { axle, psi: value },
        StepKind::StaticPressure => Tuning::StaticPressure { axle, psi: value },
        StepKind::Camber => Tuning::Camber {
            axle,
            degrees: value,
        },
        StepKind::BaseHeight => Tuning::BaseHeight {
            axle,
            metres: value,
        },
    })
}

/// The kind with its number replaced by `value`.
fn with_value(kind: Tuning, value: f64) -> Tuning {
    match kind {
        Tuning::TorqueCurve(_) => Tuning::TorqueCurve(value),
        Tuning::TotalMass(_) => Tuning::TotalMass(value),
        Tuning::FuelTank(_) => Tuning::FuelTank(value),
        Tuning::GearRatios(_) => Tuning::GearRatios(value),
        Tuning::FinalDrive(_) => Tuning::FinalDrive(value),
        Tuning::IdealPressure { axle, .. } => Tuning::IdealPressure { axle, psi: value },
        Tuning::StaticPressure { axle, .. } => Tuning::StaticPressure { axle, psi: value },
        Tuning::Camber { axle, .. } => Tuning::Camber {
            axle,
            degrees: value,
        },
        Tuning::BaseHeight { axle, .. } => Tuning::BaseHeight {
            axle,
            metres: value,
        },
    }
}

/// Checks a value against the bounds that [`car_tuning`] enforces.
fn check_value(kind: Tuning) -> Result<(), TuneError> {
    let (what, value, min, max) = match kind {
        Tuning::TorqueCurve(v) | Tuning::GearRatios(v) | Tuning::FinalDrive(v) => {
            return if v.is_finite() && (MIN_FACTOR..=MAX_FACTOR).contains(&v) {
                Ok(())
            } else {
                Err(TuneError::InvalidFactor(v))
            };
        }
        Tuning::TotalMass(v) => ("total mass (kg)", v, MIN_MASS_KG, MAX_MASS_KG),
        Tuning::FuelTank(v) => ("fuel tank (L)", v, MIN_FUEL_L, MAX_FUEL_L),
        Tuning::IdealPressure { psi: v, .. } | Tuning::StaticPressure { psi: v, .. } => {
            ("tyre pressure (psi)", v, MIN_PRESSURE_PSI, MAX_PRESSURE_PSI)
        }
        Tuning::Camber { degrees: v, .. } => ("camber (deg)", v, MIN_CAMBER_DEG, MAX_CAMBER_DEG),
        Tuning::BaseHeight { metres: v, .. } => {
            ("base height (m)", v, MIN_BASE_HEIGHT_M, MAX_BASE_HEIGHT_M)
        }
    };
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(TuneError::OutOfRange { what, value, min, max })
    }
}

fn to_axle(axle: StepAxle) -> Axle {
    match axle {
        StepAxle::Front => Axle::Front,
        StepAxle::Rear => Axle::Rear,
    }
}

fn step_axle(axle: Axle) -> StepAxle {
    match axle {
        Axle::Front => StepAxle::Front,
        Axle::Rear => StepAxle::Rear,
    }
}

fn invalid(error: TuneError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

/// Reduces a profile name to a file name. Other characters become `_`.
fn file_stem(name: &str) -> io::Result<String> {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' })
        .collect();
    if stem.is_empty() {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "the profile needs a name"))
    } else {
        Ok(stem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAR_INI: &str = "[BASIC]\r\nTOTALMASS=1180.5 ; kg with driver\r\n\r\n[FUEL]\r\nMAX_FUEL=55\r\n";

    /// A temp folder unique per process and tag, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("acxs-tuning-profiles-{}-{tag}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        /// A car folder with `data/car.ini`.
        fn car(&self) -> PathBuf {
            let car = self.0.join("car");
            fs::create_dir_all(car.join("data")).unwrap();
            fs::write(car.join("data").join("car.ini"), CAR_INI).unwrap();
            car
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn step(kind: Tuning, value: f64) -> TuningStep {
        TuningStep { kind, value }
    }

    fn profile(name: &str, steps: Vec<TuningStep>) -> TuningProfile {
        TuningProfile {
            name: name.to_string(),
            steps,
        }
    }

    #[test]
    fn profile_round_trips_through_save_and_load() {
        let dir = TempDir::new("round-trip");
        let original = profile(
            "Wet setup",
            vec![
                step(Tuning::TotalMass(1200.0), 1200.0),
                step(Tuning::Camber { axle: Axle::Rear, degrees: -1.5 }, -1.5),
                step(Tuning::TorqueCurve(0.9), 0.9),
            ],
        );
        let path = save_profile(&dir.0, &original).unwrap();
        assert_eq!(path, dir.0.join(PROFILE_DIR).join("Wet setup.json"));
        assert_eq!(load_profile(&path).unwrap(), original);
    }

    #[test]
    fn saved_steps_are_flat_json_with_axle_only_where_it_applies() {
        let dir = TempDir::new("flat-json");
        let path = save_profile(
            &dir.0,
            &profile(
                "Flat",
                vec![
                    step(Tuning::StaticPressure { axle: Axle::Front, psi: 28.0 }, 28.0),
                    step(Tuning::FuelTank(40.0), 40.0),
                ],
            ),
        )
        .unwrap();
        let text = fs::read_to_string(path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["steps"][0]["kind"], "static_pressure");
        assert_eq!(json["steps"][0]["axle"], "front");
        assert_eq!(json["steps"][0]["value"], 28.0);
        assert_eq!(json["steps"][1]["kind"], "fuel_tank");
        assert!(json["steps"][1].get("axle").is_none());
    }

    #[test]
    fn save_never_overwrites_an_existing_profile() {
        let dir = TempDir::new("create-new");
        let first = profile("Same", vec![step(Tuning::TotalMass(1100.0), 1100.0)]);
        save_profile(&dir.0, &first).unwrap();
        let second = profile("Same", vec![step(Tuning::TotalMass(1300.0), 1300.0)]);
        let err = save_profile(&dir.0, &second).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        let loaded = load_profile(&dir.0.join(PROFILE_DIR).join("Same.json")).unwrap();
        assert_eq!(loaded, first);
    }

    #[test]
    fn save_rejects_an_empty_name_and_out_of_range_steps() {
        let dir = TempDir::new("save-rejects");
        let empty = profile("   ", vec![]);
        assert_eq!(save_profile(&dir.0, &empty).unwrap_err().kind(), io::ErrorKind::InvalidInput);

        let too_heavy = profile("Heavy", vec![step(Tuning::TotalMass(9000.0), 9000.0)]);
        assert_eq!(save_profile(&dir.0, &too_heavy).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(!dir.0.join(PROFILE_DIR).join("Heavy.json").exists());
    }

    #[test]
    fn name_is_reduced_to_a_safe_file_name() {
        let dir = TempDir::new("name");
        let path = save_profile(
            &dir.0,
            &profile("../escape: dry/lap", vec![step(Tuning::FinalDrive(1.0), 1.0)]),
        )
        .unwrap();
        assert_eq!(path.parent().unwrap(), dir.0.join(PROFILE_DIR));
        assert_eq!(path.file_name().unwrap(), "___escape_ dry_lap.json");
    }

    #[test]
    fn load_rejects_out_of_range_values() {
        let dir = TempDir::new("load-range");
        let path = dir.0.join("bad.json");
        fs::write(&path, r#"{"name":"Bad","steps":[{"kind":"fuel_tank","value":900}]}"#).unwrap();
        let err = load_profile(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("fuel tank"), "{err}");

        fs::write(&path, r#"{"name":"Bad","steps":[{"kind":"torque_curve","value":5}]}"#).unwrap();
        assert_eq!(load_profile(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn load_rejects_unknown_kinds_fields_and_mismatched_axles() {
        let dir = TempDir::new("load-shape");
        let path = dir.0.join("shape.json");
        let cases = [
            r#"{"name":"A","steps":[{"kind":"horsepower","value":1}]}"#,
            r#"{"name":"A","steps":[{"kind":"camber","value":-2}]}"#,
            r#"{"name":"A","steps":[{"kind":"total_mass","axle":"front","value":1200}]}"#,
            r#"{"name":"A","steps":[{"kind":"total_mass","value":1200,"note":"x"}]}"#,
            r#"{"name":"A","steps":[],"extra":1}"#,
        ];
        for text in cases {
            fs::write(&path, text).unwrap();
            assert_eq!(load_profile(&path).unwrap_err().kind(), io::ErrorKind::InvalidData, "{text}");
        }
    }

    #[test]
    fn missing_profile_file_is_an_io_error() {
        let dir = TempDir::new("missing");
        let err = load_profile(&dir.0.join("nope.json")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn apply_runs_steps_in_order_and_reports_each_change() {
        let dir = TempDir::new("apply-order");
        let car = dir.car();
        let profile = profile(
            "Mass then tank",
            vec![
                step(Tuning::TotalMass(1200.0), 1200.0),
                step(Tuning::FuelTank(60.0), 60.0),
            ],
        );
        let changes = apply_profile(&car, &profile).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].key, "TOTALMASS");
        assert_eq!(changes[0].after, "1200");
        assert_eq!(changes[1].key, "MAX_FUEL");
        assert_eq!(changes[1].after, "60");

        let text = fs::read_to_string(car.join("data").join("car.ini")).unwrap();
        assert!(text.contains("TOTALMASS=1200"), "{text}");
        assert!(text.contains("MAX_FUEL=60"), "{text}");
    }

    #[test]
    fn apply_stops_at_the_first_error_and_reports_earlier_changes() {
        let dir = TempDir::new("apply-stop");
        let car = dir.car();
        // The car has no drivetrain.ini, so step 2 fails and step 3 never runs.
        let profile = profile(
            "Stops",
            vec![
                step(Tuning::TotalMass(1250.0), 1250.0),
                step(Tuning::FinalDrive(1.1), 1.1),
                step(Tuning::FuelTank(70.0), 70.0),
            ],
        );
        let err = apply_profile(&car, &profile).unwrap_err();
        assert_eq!(err.step, 2);
        assert_eq!(err.changes.len(), 1);
        assert_eq!(err.changes[0].key, "TOTALMASS");
        assert!(matches!(err.error, TuneError::MissingFile(_)));
        assert!(err.to_string().starts_with("step 2 failed"), "{err}");

        let text = fs::read_to_string(car.join("data").join("car.ini")).unwrap();
        assert!(text.contains("TOTALMASS=1250"), "{text}");
        assert!(text.contains("MAX_FUEL=55"), "step 3 must not run: {text}");
    }

    #[test]
    fn apply_uses_the_value_field_for_the_amount() {
        let dir = TempDir::new("apply-value");
        let car = dir.car();
        // The number inside the kind is ignored. Only `value` changes the file.
        let mismatched = profile("Mismatch", vec![step(Tuning::TotalMass(1.0), 1300.0)]);
        let changes = apply_profile(&car, &mismatched).unwrap();
        assert_eq!(changes[0].after, "1300");
    }

    #[test]
    fn empty_profile_applies_nothing() {
        let dir = TempDir::new("apply-empty");
        let car = dir.car();
        let changes = apply_profile(&car, &profile("Empty", vec![])).unwrap();
        assert!(changes.is_empty());
    }
}
