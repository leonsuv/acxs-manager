//! Tuning edits to a car folder's `data/` files: engine power and torque,
//! total mass, fuel tank size, forward gear ratios and final drive, plus per-axle
//! tyre pressures (`tyres.ini`) and per-axle camber and base height (`suspensions.ini`).
//!
//! Each change is a pure function from file text to new file text, plus a list
//! of [`Change`]s for the keys whose value actually changed. [`apply`] runs one
//! of them on a car folder and saves the edited file with [`Ini::save_with_backup`],
//! which keeps the old file as a backup. Nothing is deleted. A car folder without
//! `data/car.ini` is refused before any file is read for writing.
//!
//! Factors multiply the current value and must lie within [`MIN_FACTOR`] and
//! [`MAX_FACTOR`]. Values such as masses, tank sizes, pressures and angles must lie
//! within their `MIN_*` / `MAX_*` bounds. Out-of-range input is an error, not a clamp.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use crate::ini::Ini;

const CAR_FILE: &str = "car.ini";
const ENGINE_FILE: &str = "engine.ini";
const DRIVETRAIN_FILE: &str = "drivetrain.ini";
const TYRES_FILE: &str = "tyres.ini";
const SUSPENSIONS_FILE: &str = "suspensions.ini";
/// Curve file name used when `engine.ini` does not name one.
const DEFAULT_POWER_CURVE: &str = "power.lut";
/// Watts = torque (Nm) * rpm / this divisor. The value is close to 30 / pi.
const WATT_DIVISOR: f64 = 9.5488;

/// Smallest allowed multiplier for any factor.
pub const MIN_FACTOR: f64 = 0.1;
/// Largest allowed multiplier for any factor.
pub const MAX_FACTOR: f64 = 3.0;
/// Smallest allowed `TOTALMASS`, kg.
pub const MIN_MASS_KG: f64 = 100.0;
/// Largest allowed `TOTALMASS`, kg.
pub const MAX_MASS_KG: f64 = 5000.0;
/// Smallest allowed `MAX_FUEL`, litres.
pub const MIN_FUEL_L: f64 = 1.0;
/// Largest allowed `MAX_FUEL`, litres.
pub const MAX_FUEL_L: f64 = 500.0;
/// Smallest allowed tyre pressure, psi.
pub const MIN_PRESSURE_PSI: f64 = 10.0;
/// Largest allowed tyre pressure, psi.
pub const MAX_PRESSURE_PSI: f64 = 50.0;
/// Smallest allowed `STATIC_CAMBER`, degrees. Negative camber leans the top of the wheel inward.
pub const MIN_CAMBER_DEG: f64 = -10.0;
/// Largest allowed `STATIC_CAMBER`, degrees.
pub const MAX_CAMBER_DEG: f64 = 10.0;
/// Smallest allowed `BASEY`, metres.
pub const MIN_BASE_HEIGHT_M: f64 = 0.05;
/// Largest allowed `BASEY`, metres.
pub const MAX_BASE_HEIGHT_M: f64 = 1.0;

/// One key whose value changed. Curve files have no sections, so `section` is empty for them.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// File name inside `data/`, for example `car.ini`.
    pub file: String,
    pub section: String,
    pub key: String,
    /// Value before the edit, formatted for display.
    pub before: String,
    /// Value after the edit, formatted for display.
    pub after: String,
}

/// Result of a pure edit: the complete new file text and the keys that changed.
#[derive(Debug, Clone, PartialEq)]
pub struct TunedFile {
    pub text: String,
    pub changes: Vec<Change>,
}

/// One axle of the car. Its values live in the `[FRONT]` or `[REAR]` section of a data file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axle {
    Front,
    Rear,
}

impl Axle {
    /// Section name in `tyres.ini` and `suspensions.ini`.
    pub fn section(self) -> &'static str {
        match self {
            Axle::Front => "FRONT",
            Axle::Rear => "REAR",
        }
    }
}

/// One tuning operation to apply to a car folder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tuning {
    /// Multiply every torque point of the power curve. Power at each point scales too.
    TorqueCurve(f64),
    /// Set `[BASIC] TOTALMASS` in `car.ini`, in kg.
    TotalMass(f64),
    /// Set `[FUEL] MAX_FUEL` in `car.ini`, in litres.
    FuelTank(f64),
    /// Multiply each forward gear ratio (`GEAR_1`, `GEAR_2`, ...). Reverse is left alone.
    GearRatios(f64),
    /// Multiply `[GEARS] FINAL` in `drivetrain.ini`.
    FinalDrive(f64),
    /// Set `PRESSURE_IDEAL` for one axle in `tyres.ini`, in psi. The pressure the tyre works at hot.
    IdealPressure { axle: Axle, psi: f64 },
    /// Set `PRESSURE_STATIC` for one axle in `tyres.ini`, in psi. The cold pressure.
    StaticPressure { axle: Axle, psi: f64 },
    /// Set `STATIC_CAMBER` for one axle in `suspensions.ini`, in degrees.
    Camber { axle: Axle, degrees: f64 },
    /// Set `BASEY` for one axle in `suspensions.ini`, in metres. It is the height of the suspension base point.
    BaseHeight { axle: Axle, metres: f64 },
}

#[derive(Debug)]
pub enum TuneError {
    /// Factor is not finite or is outside `MIN_FACTOR..=MAX_FACTOR`.
    InvalidFactor(f64),
    /// A value is not finite or is outside its allowed range.
    OutOfRange {
        what: &'static str,
        value: f64,
        min: f64,
        max: f64,
    },
    /// A file the operation needs is not in `data/`.
    MissingFile(String),
    /// A key the operation needs is not set.
    MissingKey { section: String, key: String },
    /// A key exists but its value is not a usable number, or names an unusable file.
    BadValue {
        section: String,
        key: String,
        value: String,
    },
    /// `drivetrain.ini` has no `GEAR_<n>` keys.
    NoGears,
    /// The power curve has fewer than two `rpm|torque` points.
    NoCurve,
    Io(io::Error),
}

impl fmt::Display for TuneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TuneError::InvalidFactor(v) => {
                write!(f, "factor must be between {MIN_FACTOR} and {MAX_FACTOR}, got {v}")
            }
            TuneError::OutOfRange { what, value, min, max } => {
                write!(f, "{what} must be between {min} and {max}, got {value}")
            }
            TuneError::MissingFile(name) => write!(f, "data/{name} is missing"),
            TuneError::MissingKey { section, key } => write!(f, "[{section}] {key} is not set"),
            TuneError::BadValue { section, key, value } => {
                write!(f, "[{section}] {key} has an unusable value {value:?}")
            }
            TuneError::NoGears => write!(f, "drivetrain.ini has no GEAR_<n> keys"),
            TuneError::NoCurve => write!(f, "the power curve needs at least two rpm|torque points"),
            TuneError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TuneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TuneError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for TuneError {
    fn from(e: io::Error) -> Self {
        TuneError::Io(e)
    }
}

/// Scales every `rpm|torque` point of a power curve file by `factor`.
///
/// Comment lines and other text are kept. Power follows from torque, so it
/// scales by the same factor. The summary lists the peak torque and peak power
/// when they change.
pub fn scale_torque_curve(curve: &str, factor: f64) -> Result<TunedFile, TuneError> {
    check_factor(factor)?;
    let mut lines = Vec::new();
    let mut before = Vec::new();
    let mut after = Vec::new();
    for line in curve.trim_start_matches('\u{feff}').lines() {
        match parse_point(line.trim()) {
            Some(point) => {
                let scaled = point.torque * factor;
                before.push((point.rpm, point.torque));
                after.push((point.rpm, scaled));
                lines.push(format!("{}|{}{}", point.rpm_text, fmt_num(scaled), point.tail));
            }
            None => lines.push(line.to_string()),
        }
    }
    if before.len() < 2 {
        return Err(TuneError::NoCurve);
    }
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    Ok(TunedFile {
        text,
        changes: peak_changes(&before, &after),
    })
}

/// Sets `[BASIC] TOTALMASS` to `kg`. Inline comments on the line are kept.
pub fn set_total_mass(car_ini: &str, kg: f64) -> Result<TunedFile, TuneError> {
    check_range("mass", kg, MIN_MASS_KG, MAX_MASS_KG)?;
    let mut ini = Ini::parse(car_ini);
    let changes = update_number(&mut ini, CAR_FILE, "BASIC", "TOTALMASS", |_| kg)?.into_iter().collect();
    Ok(finish(&ini, changes))
}

/// Sets `[FUEL] MAX_FUEL` to `litres`. Inline comments on the line are kept.
pub fn set_fuel_tank(car_ini: &str, litres: f64) -> Result<TunedFile, TuneError> {
    check_range("fuel tank", litres, MIN_FUEL_L, MAX_FUEL_L)?;
    let mut ini = Ini::parse(car_ini);
    let changes = update_number(&mut ini, CAR_FILE, "FUEL", "MAX_FUEL", |_| litres)?.into_iter().collect();
    Ok(finish(&ini, changes))
}

/// Multiplies every forward gear ratio (`GEAR_1`, `GEAR_2`, ...) in `[GEARS]` by `factor`.
/// `GEAR_R` and `FINAL` are not touched.
pub fn scale_gear_ratios(drivetrain_ini: &str, factor: f64) -> Result<TunedFile, TuneError> {
    check_factor(factor)?;
    let mut ini = Ini::parse(drivetrain_ini);
    let gear_keys: Vec<String> = ini
        .entries("GEARS")
        .into_iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| is_forward_gear_key(key))
        .collect();
    if gear_keys.is_empty() {
        return Err(TuneError::NoGears);
    }
    let mut changes = Vec::new();
    for key in &gear_keys {
        changes.extend(update_number(&mut ini, DRIVETRAIN_FILE, "GEARS", key, |old| old * factor)?);
    }
    Ok(finish(&ini, changes))
}

/// Multiplies `[GEARS] FINAL` in `drivetrain_ini` by `factor`.
pub fn scale_final_drive(drivetrain_ini: &str, factor: f64) -> Result<TunedFile, TuneError> {
    check_factor(factor)?;
    let mut ini = Ini::parse(drivetrain_ini);
    let changes = update_number(&mut ini, DRIVETRAIN_FILE, "GEARS", "FINAL", |old| old * factor)?
        .into_iter()
        .collect();
    Ok(finish(&ini, changes))
}

/// Sets `PRESSURE_IDEAL` in the `axle` section of `tyres_ini` to `psi`.
pub fn set_ideal_pressure(tyres_ini: &str, axle: Axle, psi: f64) -> Result<TunedFile, TuneError> {
    set_tyre_pressure(tyres_ini, axle, "PRESSURE_IDEAL", psi)
}

/// Sets `PRESSURE_STATIC` (cold pressure) in the `axle` section of `tyres_ini` to `psi`.
pub fn set_static_pressure(tyres_ini: &str, axle: Axle, psi: f64) -> Result<TunedFile, TuneError> {
    set_tyre_pressure(tyres_ini, axle, "PRESSURE_STATIC", psi)
}

fn set_tyre_pressure(tyres_ini: &str, axle: Axle, key: &str, psi: f64) -> Result<TunedFile, TuneError> {
    check_range("tyre pressure", psi, MIN_PRESSURE_PSI, MAX_PRESSURE_PSI)?;
    let mut ini = Ini::parse(tyres_ini);
    let changes = update_number(&mut ini, TYRES_FILE, axle.section(), key, |_| psi)?
        .into_iter()
        .collect();
    Ok(finish(&ini, changes))
}

/// Sets `STATIC_CAMBER` in the `axle` section of `suspensions_ini` to `degrees`.
pub fn set_camber(suspensions_ini: &str, axle: Axle, degrees: f64) -> Result<TunedFile, TuneError> {
    check_range("camber", degrees, MIN_CAMBER_DEG, MAX_CAMBER_DEG)?;
    let mut ini = Ini::parse(suspensions_ini);
    let changes = update_number(&mut ini, SUSPENSIONS_FILE, axle.section(), "STATIC_CAMBER", |_| degrees)?
        .into_iter()
        .collect();
    Ok(finish(&ini, changes))
}

/// Sets `BASEY` in the `axle` section of `suspensions_ini` to `metres`.
pub fn set_base_height(suspensions_ini: &str, axle: Axle, metres: f64) -> Result<TunedFile, TuneError> {
    check_range("base height", metres, MIN_BASE_HEIGHT_M, MAX_BASE_HEIGHT_M)?;
    let mut ini = Ini::parse(suspensions_ini);
    let changes = update_number(&mut ini, SUSPENSIONS_FILE, axle.section(), "BASEY", |_| metres)?
        .into_iter()
        .collect();
    Ok(finish(&ini, changes))
}

/// Applies one tuning operation to the car in `car_dir` and saves the edited file.
///
/// The previous file is kept as `<name>.acxs-backup-<stamp>` next to it. Returns
/// the keys that changed. When none changed, nothing is written and the list is empty.
/// A car folder without `data/car.ini` is refused, with nothing written. So is one
/// whose operation needs a data file that is missing (`tyres.ini` or `suspensions.ini`).
pub fn apply(car_dir: &Path, tuning: Tuning) -> Result<Vec<Change>, TuneError> {
    let data = car_dir.join("data");
    if !data.join(CAR_FILE).is_file() {
        return Err(TuneError::MissingFile(CAR_FILE.to_string()));
    }
    let (file, tuned) = match tuning {
        Tuning::TorqueCurve(factor) => {
            let engine = read_data_file(&data, ENGINE_FILE)?;
            let name = power_curve_name(&engine)?;
            let curve = read_data_file(&data, &name)?;
            (name, scale_torque_curve(&curve, factor)?)
        }
        Tuning::TotalMass(kg) => {
            let car = read_data_file(&data, CAR_FILE)?;
            (CAR_FILE.to_string(), set_total_mass(&car, kg)?)
        }
        Tuning::FuelTank(litres) => {
            let car = read_data_file(&data, CAR_FILE)?;
            (CAR_FILE.to_string(), set_fuel_tank(&car, litres)?)
        }
        Tuning::GearRatios(factor) => {
            let drivetrain = read_data_file(&data, DRIVETRAIN_FILE)?;
            (DRIVETRAIN_FILE.to_string(), scale_gear_ratios(&drivetrain, factor)?)
        }
        Tuning::FinalDrive(factor) => {
            let drivetrain = read_data_file(&data, DRIVETRAIN_FILE)?;
            (DRIVETRAIN_FILE.to_string(), scale_final_drive(&drivetrain, factor)?)
        }
        Tuning::IdealPressure { axle, psi } => {
            let tyres = read_data_file(&data, TYRES_FILE)?;
            (TYRES_FILE.to_string(), set_ideal_pressure(&tyres, axle, psi)?)
        }
        Tuning::StaticPressure { axle, psi } => {
            let tyres = read_data_file(&data, TYRES_FILE)?;
            (TYRES_FILE.to_string(), set_static_pressure(&tyres, axle, psi)?)
        }
        Tuning::Camber { axle, degrees } => {
            let suspensions = read_data_file(&data, SUSPENSIONS_FILE)?;
            (SUSPENSIONS_FILE.to_string(), set_camber(&suspensions, axle, degrees)?)
        }
        Tuning::BaseHeight { axle, metres } => {
            let suspensions = read_data_file(&data, SUSPENSIONS_FILE)?;
            (SUSPENSIONS_FILE.to_string(), set_base_height(&suspensions, axle, metres)?)
        }
    };
    if tuned.changes.is_empty() {
        return Ok(Vec::new());
    }
    // The curve file has no sections. The INI writer keeps lines that have no `=` verbatim,
    // so the same save path works for it.
    Ini::parse(&tuned.text).save_with_backup(&data.join(&file))?;
    Ok(tuned
        .changes
        .into_iter()
        .map(|change| Change {
            file: file.clone(),
            ..change
        })
        .collect())
}

/// Sets `key` in `section` to `new_value(old)`. Returns the change, or `None` when the
/// displayed value would not change. The value's inline comment is kept.
fn update_number(
    ini: &mut Ini,
    file: &str,
    section: &str,
    key: &str,
    new_value: impl FnOnce(f64) -> f64,
) -> Result<Option<Change>, TuneError> {
    let raw = ini.get(section, key).ok_or_else(|| TuneError::MissingKey {
        section: section.to_string(),
        key: key.to_string(),
    })?;
    let (value, tail) = split_comment(raw);
    let old = value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| TuneError::BadValue {
            section: section.to_string(),
            key: key.to_string(),
            value: raw.to_string(),
        })?;
    let tail = tail.to_string();
    let before = fmt_num(old);
    let after = fmt_num(new_value(old));
    if before == after {
        return Ok(None);
    }
    ini.set(section, key, format!("{after}{tail}"));
    Ok(Some(Change {
        file: file.to_string(),
        section: section.to_string(),
        key: key.to_string(),
        before,
        after,
    }))
}

fn finish(ini: &Ini, changes: Vec<Change>) -> TunedFile {
    TunedFile {
        text: ini.to_string_crlf(),
        changes,
    }
}

/// Splits a value into the value and its trailing `;` or `//` comment, with the
/// whitespace in front of the comment kept in the tail. `"3.2 ; final"` gives `("3.2", " ; final")`.
fn split_comment(raw: &str) -> (&str, &str) {
    let cut = [raw.find(';'), raw.find("//")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(raw.len());
    let value_end = raw[..cut].trim_end().len();
    (&raw[..value_end], &raw[value_end..])
}

/// `GEAR_` followed by one or more digits. `GEAR_R` does not match.
fn is_forward_gear_key(key: &str) -> bool {
    key.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("GEAR_"))
        && key
            .get(5..)
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Reads a file from `data/`. A missing file becomes [`TuneError::MissingFile`].
fn read_data_file(data: &Path, name: &str) -> Result<String, TuneError> {
    fs::read_to_string(data.join(name)).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            TuneError::MissingFile(name.to_string())
        } else {
            TuneError::Io(e)
        }
    })
}

/// Name of the power curve file, from `[HEADER] POWER_CURVE`. The name must stay inside `data/`.
fn power_curve_name(engine: &str) -> Result<String, TuneError> {
    let ini = Ini::parse(engine);
    let Some(raw) = ini.get("HEADER", "POWER_CURVE") else {
        return Ok(DEFAULT_POWER_CURVE.to_string());
    };
    let name = split_comment(raw).0.trim();
    if name.is_empty() {
        return Ok(DEFAULT_POWER_CURVE.to_string());
    }
    if name == ".." || name.contains(['/', '\\']) {
        return Err(TuneError::BadValue {
            section: "HEADER".to_string(),
            key: "POWER_CURVE".to_string(),
            value: raw.to_string(),
        });
    }
    Ok(name.to_string())
}

struct CurvePoint<'a> {
    rpm_text: &'a str,
    rpm: f64,
    torque: f64,
    /// Trailing comment and its leading whitespace, kept on rewrite.
    tail: &'a str,
}

/// Parses an `rpm|torque` line. Returns `None` for comments and anything else.
fn parse_point(line: &str) -> Option<CurvePoint<'_>> {
    if line.starts_with('#') {
        return None;
    }
    let (body, tail) = split_comment(line);
    let (rpm_text, torque_text) = body.split_once('|')?;
    let rpm_text = rpm_text.trim();
    let rpm = rpm_text.parse::<f64>().ok().filter(|v| v.is_finite())?;
    let torque = torque_text.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some(CurvePoint {
        rpm_text,
        rpm,
        torque,
        tail,
    })
}

/// Changes in peak torque (Nm) and peak power (kW) between two curves, listing only what differs.
fn peak_changes(before: &[(f64, f64)], after: &[(f64, f64)]) -> Vec<Change> {
    let peak_torque = |points: &[(f64, f64)]| points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let peak_power_kw = |points: &[(f64, f64)]| {
        points
            .iter()
            .map(|&(rpm, torque)| torque * rpm / WATT_DIVISOR / 1000.0)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    [
        ("peak torque (Nm)", peak_torque(before), peak_torque(after)),
        ("peak power (kW)", peak_power_kw(before), peak_power_kw(after)),
    ]
    .into_iter()
    .filter_map(|(key, old, new)| {
        let (before, after) = (fmt_num(old), fmt_num(new));
        (before != after).then(|| Change {
            file: DEFAULT_POWER_CURVE.to_string(),
            section: String::new(),
            key: key.to_string(),
            before,
            after,
        })
    })
    .collect()
}

fn check_factor(factor: f64) -> Result<(), TuneError> {
    if factor.is_finite() && (MIN_FACTOR..=MAX_FACTOR).contains(&factor) {
        Ok(())
    } else {
        Err(TuneError::InvalidFactor(factor))
    }
}

fn check_range(what: &'static str, value: f64, min: f64, max: f64) -> Result<(), TuneError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(TuneError::OutOfRange { what, value, min, max })
    }
}

/// Four decimals at most, trailing zeros dropped: `1180.5`, `2.88`, `250`.
fn fmt_num(value: f64) -> String {
    let text = format!("{value:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const CAR_INI: &str = "[BASIC]\r\nTOTALMASS=1180.5 ; kg with driver\r\n\r\n[FUEL]\r\nMAX_FUEL=55\r\n";
    const DRIVETRAIN_INI: &str = "[TRACTION]\r\nTYPE=RWD\r\n\r\n[GEARS]\r\nCOUNT=3\r\nGEAR_R=2.9\r\nGEAR_1=3.2\r\nGEAR_2=2.1 ; second\r\nGEAR_3=1.5\r\nFINAL=3.8\r\n";
    const CURVE: &str = "; rpm|torque\r\n2000|100\r\n4000|200\r\n6000|250\r\n8000|220\r\n";

    /// A car folder under the temp dir, unique per process and test, removed on drop.
    struct TempCar(PathBuf);

    impl TempCar {
        fn new(name: &str) -> TempCar {
            let dir = std::env::temp_dir().join(format!("acxs-car-tuning-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("data")).unwrap();
            TempCar(dir)
        }

        fn write(&self, file: &str, text: &str) {
            fs::write(self.0.join("data").join(file), text).unwrap();
        }

        fn read(&self, file: &str) -> String {
            fs::read_to_string(self.0.join("data").join(file)).unwrap()
        }

        /// Contents of every backup of `file`, in directory order.
        fn backups(&self, file: &str) -> Vec<String> {
            let prefix = format!("{file}.acxs-backup-");
            fs::read_dir(self.0.join("data"))
                .unwrap()
                .map(|e| e.unwrap())
                .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                .map(|e| fs::read_to_string(e.path()).unwrap())
                .collect()
        }
    }

    impl Drop for TempCar {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn pairs(changes: &[Change]) -> Vec<(&str, &str, &str)> {
        changes
            .iter()
            .map(|c| (c.key.as_str(), c.before.as_str(), c.after.as_str()))
            .collect()
    }

    #[test]
    fn curve_scaling_multiplies_torque_and_keeps_comments() {
        let out = scale_torque_curve(CURVE, 1.1).unwrap();
        assert!(out.text.starts_with("; rpm|torque\r\n"));
        assert!(out.text.contains("2000|110\r\n"));
        assert!(out.text.contains("8000|242\r\n"));
        assert_eq!(out.changes.len(), 2);
        assert_eq!((out.changes[0].key.as_str(), out.changes[0].before.as_str(), out.changes[0].after.as_str()), ("peak torque (Nm)", "250", "275"));
        // Peak power is 220 Nm at 8000 rpm before scaling; after scaling it is 10 % higher.
        let power_before: f64 = out.changes[1].before.parse().unwrap();
        let power_after: f64 = out.changes[1].after.parse().unwrap();
        assert_eq!(out.changes[1].key, "peak power (kW)");
        assert!((power_before - 220.0 * 8000.0 / 9.5488 / 1000.0).abs() < 1e-3);
        assert!((power_after - power_before * 1.1).abs() < 1e-3);
    }

    #[test]
    fn curve_scaling_keeps_the_rpm_text_and_trailing_comments() {
        let out = scale_torque_curve("2000.0|100 ; idle\r\n4000|200\r\n", 0.5).unwrap();
        assert_eq!(out.text, "2000.0|50 ; idle\r\n4000|100\r\n");
    }

    #[test]
    fn unit_factor_gives_no_changes() {
        let out = scale_torque_curve(CURVE, 1.0).unwrap();
        assert!(out.changes.is_empty());
        assert_eq!(scale_gear_ratios(DRIVETRAIN_INI, 1.0).unwrap().changes, Vec::new());
    }

    #[test]
    fn bad_factors_are_refused() {
        for factor in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.05, 3.5] {
            assert!(matches!(scale_torque_curve(CURVE, factor), Err(TuneError::InvalidFactor(_))));
            assert!(matches!(scale_gear_ratios(DRIVETRAIN_INI, factor), Err(TuneError::InvalidFactor(_))));
            assert!(matches!(scale_final_drive(DRIVETRAIN_INI, factor), Err(TuneError::InvalidFactor(_))));
        }
    }

    #[test]
    fn curve_needs_two_points() {
        assert!(matches!(scale_torque_curve("1000|100\r\n", 1.1), Err(TuneError::NoCurve)));
        assert!(matches!(scale_torque_curve("garbage\r\n", 1.1), Err(TuneError::NoCurve)));
    }

    #[test]
    fn mass_change_keeps_the_inline_comment() {
        let out = set_total_mass(CAR_INI, 1200.0).unwrap();
        assert!(out.text.contains("TOTALMASS=1200 ; kg with driver\r\n"));
        assert_eq!(out.changes.len(), 1);
        assert_eq!(out.changes[0].file, "car.ini");
        assert_eq!(out.changes[0].section, "BASIC");
        assert_eq!(pairs(&out.changes), vec![("TOTALMASS", "1180.5", "1200")]);
    }

    #[test]
    fn mass_out_of_range_is_refused() {
        for kg in [50.0, 6000.0, f64::NAN] {
            assert!(matches!(set_total_mass(CAR_INI, kg), Err(TuneError::OutOfRange { .. })));
        }
    }

    #[test]
    fn missing_keys_are_errors() {
        assert!(matches!(
            set_total_mass("[BASIC]\nNAME=x\n", 1000.0),
            Err(TuneError::MissingKey { .. })
        ));
        assert!(matches!(
            set_fuel_tank("[BASIC]\nTOTALMASS=1000\n", 60.0),
            Err(TuneError::MissingKey { .. })
        ));
    }

    #[test]
    fn fuel_tank_changes_max_fuel() {
        let out = set_fuel_tank(CAR_INI, 60.0).unwrap();
        assert!(out.text.contains("MAX_FUEL=60\r\n"));
        assert_eq!(pairs(&out.changes), vec![("MAX_FUEL", "55", "60")]);
        assert!(matches!(set_fuel_tank(CAR_INI, 0.0), Err(TuneError::OutOfRange { .. })));
        assert!(matches!(set_fuel_tank(CAR_INI, 900.0), Err(TuneError::OutOfRange { .. })));
    }

    #[test]
    fn gear_scaling_touches_only_forward_gears() {
        let out = scale_gear_ratios(DRIVETRAIN_INI, 0.9).unwrap();
        assert!(out.text.contains("GEAR_1=2.88\r\n"));
        assert!(out.text.contains("GEAR_2=1.89 ; second\r\n"));
        assert!(out.text.contains("GEAR_3=1.35\r\n"));
        assert!(out.text.contains("GEAR_R=2.9\r\n"));
        assert!(out.text.contains("FINAL=3.8\r\n"));
        assert_eq!(
            pairs(&out.changes),
            vec![("GEAR_1", "3.2", "2.88"), ("GEAR_2", "2.1", "1.89"), ("GEAR_3", "1.5", "1.35")]
        );
    }

    #[test]
    fn final_drive_scaling_touches_only_final() {
        let out = scale_final_drive(DRIVETRAIN_INI, 1.25).unwrap();
        assert!(out.text.contains("FINAL=4.75\r\n"));
        assert!(out.text.contains("GEAR_1=3.2\r\n"));
        assert_eq!(pairs(&out.changes), vec![("FINAL", "3.8", "4.75")]);
    }

    #[test]
    fn drivetrain_without_forward_gears_is_an_error() {
        assert!(matches!(
            scale_gear_ratios("[GEARS]\nCOUNT=0\nGEAR_R=2.9\n", 1.1),
            Err(TuneError::NoGears)
        ));
    }

    #[test]
    fn unusable_gear_value_is_an_error() {
        assert!(matches!(
            scale_gear_ratios("[GEARS]\nGEAR_1=abc\n", 1.1),
            Err(TuneError::BadValue { .. })
        ));
    }

    #[test]
    fn apply_mass_writes_the_file_and_keeps_a_backup() {
        let car = TempCar::new("mass");
        car.write("car.ini", CAR_INI);

        let changes = apply(&car.0, Tuning::TotalMass(1200.0)).unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].file, "car.ini");
        assert!(car.read("car.ini").contains("TOTALMASS=1200 ; kg with driver"));
        let backups = car.backups("car.ini");
        assert_eq!(backups.len(), 1);
        assert!(backups[0].contains("TOTALMASS=1180.5"));
    }

    #[test]
    fn apply_refuses_a_car_without_car_ini() {
        let car = TempCar::new("nocar");
        car.write("engine.ini", "[HEADER]\r\nPOWER_CURVE=power.lut\r\n");
        car.write("power.lut", CURVE);

        let result = apply(&car.0, Tuning::TorqueCurve(1.1));

        assert!(matches!(result, Err(TuneError::MissingFile(ref name)) if name == "car.ini"));
        assert_eq!(car.read("power.lut"), CURVE);
        assert!(car.backups("power.lut").is_empty());
    }

    #[test]
    fn apply_refuses_a_folder_that_does_not_exist() {
        let absent = std::env::temp_dir().join(format!("acxs-car-tuning-{}-absent", std::process::id()));
        let _ = fs::remove_dir_all(&absent);

        assert!(matches!(apply(&absent, Tuning::FuelTank(60.0)), Err(TuneError::MissingFile(_))));
        assert!(!absent.exists());
    }

    #[test]
    fn apply_curve_uses_the_curve_name_from_engine_ini() {
        let car = TempCar::new("curve");
        car.write("car.ini", CAR_INI);
        car.write("engine.ini", "[HEADER]\r\nPOWER_CURVE=curve.lut\r\n");
        car.write("curve.lut", CURVE);

        let changes = apply(&car.0, Tuning::TorqueCurve(0.5)).unwrap();

        assert!(!changes.is_empty());
        assert!(changes.iter().all(|c| c.file == "curve.lut"));
        assert!(car.read("curve.lut").contains("8000|110\r\n"));
        assert_eq!(car.backups("curve.lut").len(), 1);
        assert!(car.backups("power.lut").is_empty());
    }

    #[test]
    fn apply_rejects_a_curve_name_outside_data() {
        let car = TempCar::new("escape");
        car.write("car.ini", CAR_INI);
        car.write("engine.ini", "[HEADER]\r\nPOWER_CURVE=../secret.lut\r\n");

        assert!(matches!(apply(&car.0, Tuning::TorqueCurve(1.1)), Err(TuneError::BadValue { .. })));
    }

    #[test]
    fn apply_with_no_effective_change_writes_nothing() {
        let car = TempCar::new("noop");
        car.write("car.ini", CAR_INI);
        car.write("drivetrain.ini", DRIVETRAIN_INI);

        let changes = apply(&car.0, Tuning::GearRatios(1.0)).unwrap();

        assert!(changes.is_empty());
        assert!(car.backups("drivetrain.ini").is_empty());
        assert_eq!(car.read("drivetrain.ini"), DRIVETRAIN_INI);
    }

    #[test]
    fn apply_final_drive_changes_only_final() {
        let car = TempCar::new("final");
        car.write("car.ini", CAR_INI);
        car.write("drivetrain.ini", DRIVETRAIN_INI);

        let changes = apply(&car.0, Tuning::FinalDrive(0.5)).unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].file, "drivetrain.ini");
        assert_eq!((changes[0].before.as_str(), changes[0].after.as_str()), ("3.8", "1.9"));
        let text = car.read("drivetrain.ini");
        assert!(text.contains("FINAL=1.9\r\n"));
        assert!(text.contains("GEAR_1=3.2\r\n"));
    }

    const TYRES_INI: &str = "[FRONT]\r\nSHORT_NAME=F\r\nPRESSURE_STATIC=26 ; cold\r\nPRESSURE_IDEAL=28\r\n\r\n[REAR]\r\nSHORT_NAME=R\r\nPRESSURE_STATIC=25\r\nPRESSURE_IDEAL=27.5\r\n";
    const SUSPENSIONS_INI: &str = "[BASIC]\r\nCG_LOCATION=0.55\r\n\r\n[FRONT]\r\nSTATIC_CAMBER=-2.5 ; deg\r\nBASEY=0.25\r\n\r\n[REAR]\r\nSTATIC_CAMBER=-1\r\nBASEY=0.3\r\n";

    #[test]
    fn ideal_pressure_changes_only_its_axle() {
        let out = set_ideal_pressure(TYRES_INI, Axle::Front, 30.0).unwrap();
        assert!(out.text.contains("PRESSURE_IDEAL=30\r\n"));
        assert!(out.text.contains("PRESSURE_IDEAL=27.5\r\n"));
        assert!(out.text.contains("PRESSURE_STATIC=26 ; cold\r\n"));
        assert_eq!(pairs(&out.changes), vec![("PRESSURE_IDEAL", "28", "30")]);
        assert_eq!(out.changes[0].file, "tyres.ini");
        assert_eq!(out.changes[0].section, "FRONT");
    }

    #[test]
    fn static_pressure_keeps_the_ideal_value() {
        let out = set_static_pressure(TYRES_INI, Axle::Rear, 24.0).unwrap();
        assert!(out.text.contains("PRESSURE_STATIC=24\r\n"));
        assert!(out.text.contains("PRESSURE_IDEAL=27.5\r\n"));
        assert!(out.text.contains("PRESSURE_STATIC=26 ; cold\r\n"));
        assert_eq!(pairs(&out.changes), vec![("PRESSURE_STATIC", "25", "24")]);
        assert_eq!(out.changes[0].section, "REAR");
    }

    #[test]
    fn pressure_out_of_range_is_refused() {
        for psi in [5.0, 60.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                set_ideal_pressure(TYRES_INI, Axle::Front, psi),
                Err(TuneError::OutOfRange { .. })
            ));
            assert!(matches!(
                set_static_pressure(TYRES_INI, Axle::Rear, psi),
                Err(TuneError::OutOfRange { .. })
            ));
        }
    }

    #[test]
    fn same_pressure_gives_no_changes() {
        assert!(set_ideal_pressure(TYRES_INI, Axle::Front, 28.0).unwrap().changes.is_empty());
    }

    #[test]
    fn missing_pressure_key_or_section_is_an_error() {
        assert!(matches!(
            set_ideal_pressure("[FRONT]\r\nPRESSURE_STATIC=26\r\n", Axle::Front, 28.0),
            Err(TuneError::MissingKey { .. })
        ));
        assert!(matches!(
            set_ideal_pressure("[REAR]\r\nPRESSURE_IDEAL=27\r\n", Axle::Front, 28.0),
            Err(TuneError::MissingKey { .. })
        ));
    }

    #[test]
    fn unusable_pressure_value_is_an_error() {
        assert!(matches!(
            set_ideal_pressure("[FRONT]\r\nPRESSURE_IDEAL=high\r\n", Axle::Front, 28.0),
            Err(TuneError::BadValue { .. })
        ));
    }

    #[test]
    fn camber_changes_only_its_axle_and_keeps_the_comment() {
        let out = set_camber(SUSPENSIONS_INI, Axle::Front, -3.2).unwrap();
        assert!(out.text.contains("STATIC_CAMBER=-3.2 ; deg\r\n"));
        assert!(out.text.contains("STATIC_CAMBER=-1\r\n"));
        assert!(out.text.contains("CG_LOCATION=0.55\r\n"));
        assert_eq!(pairs(&out.changes), vec![("STATIC_CAMBER", "-2.5", "-3.2")]);
        assert_eq!(out.changes[0].file, "suspensions.ini");
        assert_eq!(out.changes[0].section, "FRONT");
    }

    #[test]
    fn camber_out_of_range_is_refused() {
        for degrees in [-10.5, 11.0, f64::NAN] {
            assert!(matches!(
                set_camber(SUSPENSIONS_INI, Axle::Rear, degrees),
                Err(TuneError::OutOfRange { .. })
            ));
        }
    }

    #[test]
    fn camber_without_the_key_is_an_error() {
        assert!(matches!(
            set_camber("[FRONT]\r\nBASEY=0.25\r\n", Axle::Front, -2.0),
            Err(TuneError::MissingKey { .. })
        ));
    }

    #[test]
    fn base_height_changes_only_its_axle() {
        let out = set_base_height(SUSPENSIONS_INI, Axle::Rear, 0.32).unwrap();
        assert!(out.text.contains("BASEY=0.32\r\n"));
        assert!(out.text.contains("BASEY=0.25\r\n"));
        assert_eq!(pairs(&out.changes), vec![("BASEY", "0.3", "0.32")]);
        assert_eq!(out.changes[0].file, "suspensions.ini");
    }

    #[test]
    fn base_height_out_of_range_is_refused() {
        for metres in [0.01, 1.5, f64::NAN] {
            assert!(matches!(
                set_base_height(SUSPENSIONS_INI, Axle::Front, metres),
                Err(TuneError::OutOfRange { .. })
            ));
        }
    }

    #[test]
    fn apply_ideal_pressure_writes_tyres_ini_and_keeps_a_backup() {
        let car = TempCar::new("tyre-ideal");
        car.write("car.ini", CAR_INI);
        car.write("tyres.ini", TYRES_INI);

        let changes = apply(&car.0, Tuning::IdealPressure { axle: Axle::Rear, psi: 29.0 }).unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].file, "tyres.ini");
        assert!(car.read("tyres.ini").contains("PRESSURE_IDEAL=29\r\n"));
        let backups = car.backups("tyres.ini");
        assert_eq!(backups.len(), 1);
        assert!(backups[0].contains("PRESSURE_IDEAL=27.5"));
        assert!(car.backups("car.ini").is_empty());
    }

    #[test]
    fn apply_static_pressure_writes_tyres_ini() {
        let car = TempCar::new("tyre-static");
        car.write("car.ini", CAR_INI);
        car.write("tyres.ini", TYRES_INI);

        let changes = apply(&car.0, Tuning::StaticPressure { axle: Axle::Front, psi: 24.5 }).unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key, "PRESSURE_STATIC");
        assert!(car.read("tyres.ini").contains("PRESSURE_STATIC=24.5 ; cold\r\n"));
        assert_eq!(car.backups("tyres.ini").len(), 1);
    }

    #[test]
    fn apply_camber_writes_suspensions_ini_and_keeps_a_backup() {
        let car = TempCar::new("camber");
        car.write("car.ini", CAR_INI);
        car.write("suspensions.ini", SUSPENSIONS_INI);

        let changes = apply(&car.0, Tuning::Camber { axle: Axle::Front, degrees: -3.0 }).unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].file, "suspensions.ini");
        assert!(car.read("suspensions.ini").contains("STATIC_CAMBER=-3 ; deg\r\n"));
        let backups = car.backups("suspensions.ini");
        assert_eq!(backups.len(), 1);
        assert!(backups[0].contains("STATIC_CAMBER=-2.5"));
    }

    #[test]
    fn apply_base_height_writes_suspensions_ini() {
        let car = TempCar::new("base-height");
        car.write("car.ini", CAR_INI);
        car.write("suspensions.ini", SUSPENSIONS_INI);

        let changes = apply(&car.0, Tuning::BaseHeight { axle: Axle::Front, metres: 0.2 }).unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key, "BASEY");
        assert!(car.read("suspensions.ini").contains("BASEY=0.2\r\n"));
        assert_eq!(car.backups("suspensions.ini").len(), 1);
    }

    #[test]
    fn apply_tyre_tuning_refuses_a_car_without_car_ini() {
        let car = TempCar::new("tyre-nocar");
        car.write("tyres.ini", TYRES_INI);

        let result = apply(&car.0, Tuning::IdealPressure { axle: Axle::Front, psi: 30.0 });

        assert!(matches!(result, Err(TuneError::MissingFile(ref name)) if name == "car.ini"));
        assert_eq!(car.read("tyres.ini"), TYRES_INI);
        assert!(car.backups("tyres.ini").is_empty());
    }

    #[test]
    fn apply_refuses_when_the_data_file_it_edits_is_missing() {
        let car = TempCar::new("no-tyres");
        car.write("car.ini", CAR_INI);
        car.write("suspensions.ini", SUSPENSIONS_INI);

        let result = apply(&car.0, Tuning::StaticPressure { axle: Axle::Front, psi: 30.0 });
        assert!(matches!(result, Err(TuneError::MissingFile(ref name)) if name == "tyres.ini"));
        assert!(car.backups("suspensions.ini").is_empty());
        assert_eq!(car.read("suspensions.ini"), SUSPENSIONS_INI);

        let result = apply(&car.0, Tuning::IdealPressure { axle: Axle::Rear, psi: 30.0 });
        assert!(matches!(result, Err(TuneError::MissingFile(ref name)) if name == "tyres.ini"));
    }

    #[test]
    fn apply_camber_refuses_when_suspensions_ini_is_missing() {
        let car = TempCar::new("no-suspensions");
        car.write("car.ini", CAR_INI);
        car.write("tyres.ini", TYRES_INI);

        let result = apply(&car.0, Tuning::Camber { axle: Axle::Rear, degrees: -1.0 });

        assert!(matches!(result, Err(TuneError::MissingFile(ref name)) if name == "suspensions.ini"));
        assert!(car.backups("tyres.ini").is_empty());
        assert!(!car.0.join("data").join("suspensions.ini").exists());
    }
}
