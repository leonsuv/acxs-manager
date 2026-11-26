//! A car's physical specs, read from its own `content/cars/<id>/data/` folder.
//!
//! Every value is optional: a missing file or key gives `None`, never a panic
//! or an error. Section and key names follow the layout of the game's files:
//!
//! | File              | Section        | Key            | Meaning                        |
//! |-------------------|----------------|----------------|--------------------------------|
//! | `car.ini`         | `[BASIC]`      | `TOTALMASS`    | mass with driver, kg           |
//! | `car.ini`         | `[FUEL]`       | `MAX_FUEL`     | tank size, litres              |
//! | `engine.ini`      | `[HEADER]`     | `POWER_CURVE`  | torque curve file (`power.lut`)|
//! | `engine.ini`      | `[ENGINE_DATA]`| `LIMITER`      | rev limiter, rpm               |
//! | `drivetrain.ini`  | `[TRACTION]`   | `TYPE`         | e.g. `RWD`                     |
//! | `drivetrain.ini`  | `[GEARS]`      | `COUNT`        | forward gear count             |
//! | `suspensions.ini` | `[BASIC]`      | `CG_LOCATION`  | share of weight on front axle  |
//! | `tyres.ini`       | `[FRONT]`/`[REAR]` | `NAME`     | default tyre name per axle     |
//!
//! The curve file holds `rpm|torque` lines (torque in Nm). Power in watts at
//! each point is `torque * rpm / 9.5488`, where 9.5488 is a close approximation
//! of 30 / pi.

use crate::ini::Ini;
use std::fs;
use std::io;
use std::path::Path;

/// Specs of one installed car. `car_id` is the folder name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CarSpecs {
    pub car_id: String,
    pub mass_kg: Option<f64>,
    pub fuel_tank_l: Option<f64>,
    pub max_power_w: Option<f64>,
    pub max_torque_nm: Option<f64>,
    pub max_rpm: Option<f64>,
    pub gear_count: Option<u32>,
    pub drive_type: Option<String>,
    pub front_weight_distribution: Option<f64>,
    pub tyres_front: Option<String>,
    pub tyres_rear: Option<String>,
}

/// File name of a car's packed data archive.
pub const PACKED_DATA: &str = "data.acd";

/// True when the car ships its data packed as `data.acd`. The game reads the
/// archive in that case and ignores a `data/` folder next to it.
pub fn uses_packed_data(car_dir: &Path) -> bool {
    car_dir.join(PACKED_DATA).is_file()
}

/// Text of one data file of a car, from `data.acd` when the car has one (as the
/// game does), otherwise from `data/<name>`. Matching of `name` ignores case in
/// the archive. Bytes that are not UTF-8 are read as Windows ANSI (Latin-1).
pub fn read_data_text(car_dir: &Path, name: &str) -> io::Result<String> {
    let bytes = if uses_packed_data(car_dir) {
        let archive = fs::read(car_dir.join(PACKED_DATA))?;
        let folder = car_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        crate::acd::read_entry_in_folder(&archive, &folder, name).map_err(|e| match e {
            crate::acd::AcdError::Io(e) => e,
            crate::acd::AcdError::NotFound(n) => io::Error::new(io::ErrorKind::NotFound, format!("{n} is not in {PACKED_DATA}")),
            other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
        })?
    } else {
        fs::read(car_dir.join("data").join(name))?
    };
    Ok(match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => err.into_bytes().iter().map(|&b| char::from(b)).collect(),
    })
}

/// Names of the files in a car's data, from `data.acd` or the `data/` folder.
/// A car with neither gives an empty list.
pub fn data_file_names(car_dir: &Path) -> io::Result<Vec<String>> {
    if uses_packed_data(car_dir) {
        let archive = fs::read(car_dir.join(PACKED_DATA))?;
        let entries = crate::acd::list_entries(&archive)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        return Ok(entries.into_iter().map(|e| e.name).collect());
    }
    match fs::read_dir(car_dir.join("data")) {
        Ok(read) => Ok(read
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// One data file of a car parsed as INI; see [`read_data_text`].
pub fn load_data_ini(car_dir: &Path, name: &str) -> io::Result<Ini> {
    Ok(Ini::parse(&read_data_text(car_dir, name)?))
}

/// The error for edits to a car whose data is packed: the game would ignore them.
pub fn packed_data_error(car_dir: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{} uses a packed {PACKED_DATA}; the game reads that archive and ignores data/, so edits there would have no effect",
            car_dir.display()
        ),
    )
}

/// `torque[Nm] * rpm / 9.5488` gives watts (see module docs).
const RPM_NM_TO_WATTS_DIVISOR: f64 = 9.5488;

/// Value with any trailing `;` or `//` comment removed and surrounding space trimmed.
fn strip_comment(raw: &str) -> &str {
    let before_semicolon = raw.split(';').next().unwrap_or("");
    before_semicolon.split("//").next().unwrap_or("").trim()
}

fn text(ini: &Ini, section: &str, key: &str) -> Option<String> {
    let value = strip_comment(ini.get(section, key)?);
    (!value.is_empty()).then(|| value.to_string())
}

fn number(ini: &Ini, section: &str, key: &str) -> Option<f64> {
    strip_comment(ini.get(section, key)?)
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn count(ini: &Ini, section: &str, key: &str) -> Option<u32> {
    strip_comment(ini.get(section, key)?).parse::<u32>().ok()
}

/// Parse a `rpm|torque` curve file. Comment and malformed lines are skipped.
fn read_curve(car_dir: &Path, name: &str) -> Option<Vec<(f64, f64)>> {
    let text = read_data_text(car_dir, name).ok()?;
    let points: Vec<(f64, f64)> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with("//") || line.starts_with('#') {
                return None;
            }
            let (rpm, torque) = strip_comment(line).split_once('|')?;
            let rpm = rpm.trim().parse::<f64>().ok()?;
            let torque = torque.trim().parse::<f64>().ok()?;
            (rpm.is_finite() && torque.is_finite()).then_some((rpm, torque))
        })
        .collect();
    (points.len() >= 2).then_some(points)
}

/// `(max power in W, max torque in Nm)` over a curve, or `None` if there are no points.
fn curve_maxima(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let max_torque = points.iter().map(|&(_, torque)| torque).fold(f64::NEG_INFINITY, f64::max);
    let max_power = points
        .iter()
        .map(|&(rpm, torque)| torque * rpm / RPM_NM_TO_WATTS_DIVISOR)
        .fold(f64::NEG_INFINITY, f64::max);
    Some((max_power, max_torque))
}

/// Read a car's specs from `data.acd` or `<car_dir>/data/`. Missing files or keys give `None`.
pub fn read_specs(car_dir: &Path) -> CarSpecs {
    let mut specs = CarSpecs {
        car_id: car_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        ..CarSpecs::default()
    };

    if let Ok(car) = load_data_ini(car_dir, "car.ini") {
        specs.mass_kg = number(&car, "BASIC", "TOTALMASS");
        specs.fuel_tank_l = number(&car, "FUEL", "MAX_FUEL");
    }

    if let Ok(engine) = load_data_ini(car_dir, "engine.ini") {
        specs.max_rpm = number(&engine, "ENGINE_DATA", "LIMITER");
        let curve_name = text(&engine, "HEADER", "POWER_CURVE").unwrap_or_else(|| "power.lut".to_string());
        if let Some((power, torque)) = read_curve(car_dir, &curve_name).as_deref().and_then(curve_maxima) {
            specs.max_power_w = Some(power);
            specs.max_torque_nm = Some(torque);
        }
    }

    if let Ok(drivetrain) = load_data_ini(car_dir, "drivetrain.ini") {
        specs.drive_type = text(&drivetrain, "TRACTION", "TYPE");
        specs.gear_count = count(&drivetrain, "GEARS", "COUNT");
    }

    if let Ok(suspensions) = load_data_ini(car_dir, "suspensions.ini") {
        specs.front_weight_distribution = number(&suspensions, "BASIC", "CG_LOCATION")
            .filter(|share| (0.0..=1.0).contains(share));
    }

    if let Ok(tyres) = load_data_ini(car_dir, "tyres.ini") {
        specs.tyres_front = text(&tyres, "FRONT", "NAME");
        specs.tyres_rear = text(&tyres, "REAR", "NAME");
    }

    specs
}

/// Label and value pairs for display. Fields that are `None` are skipped.
pub fn summary_lines(specs: &CarSpecs) -> Vec<(String, String)> {
    let candidates: Vec<(&str, Option<String>)> = vec![
        ("Mass", specs.mass_kg.map(|kg| format!("{kg:.0} kg"))),
        ("Fuel tank", specs.fuel_tank_l.map(|l| format!("{l:.0} L"))),
        ("Max power", specs.max_power_w.map(|w| format!("{:.0} kW", w / 1000.0))),
        ("Max torque", specs.max_torque_nm.map(|nm| format!("{nm:.0} Nm"))),
        ("Rev limit", specs.max_rpm.map(|rpm| format!("{rpm:.0} rpm"))),
        ("Gears", specs.gear_count.map(|n| n.to_string())),
        ("Drive", specs.drive_type.clone()),
        (
            "Front weight",
            specs.front_weight_distribution.map(|share| format!("{:.1} %", share * 100.0)),
        ),
        ("Front tyre", specs.tyres_front.clone()),
        ("Rear tyre", specs.tyres_rear.clone()),
    ];
    candidates
        .into_iter()
        .filter_map(|(label, value)| value.map(|v| (label.to_string(), v)))
        .collect()
}

/// Set `[BASIC] TOTALMASS` in `<car_dir>/data/car.ini`. The old file is kept as a backup.
///
/// Returns an error, and changes nothing, when the file is missing, the value
/// is not a positive finite number, or the key is absent.
pub fn set_mass(car_dir: &Path, kg: f64) -> io::Result<()> {
    if !kg.is_finite() || kg <= 0.0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "mass must be a positive number"));
    }
    if uses_packed_data(car_dir) {
        return Err(packed_data_error(car_dir));
    }
    let path = car_dir.join("data").join("car.ini");
    let mut car = Ini::load(&path)?;
    if car.get("BASIC", "TOTALMASS").is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "TOTALMASS is not set in [BASIC] of car.ini",
        ));
    }
    car.set("BASIC", "TOTALMASS", kg);
    car.save_with_backup(&path)
}

// ---------------------------------------------------------------------------
// Extended physics visualizers (dyno, drivetrain, aero, tyres)
// ---------------------------------------------------------------------------

/// Reads any `x|y` numeric LUT file from `data.acd` or `data/`.
pub fn read_lut(car_dir: &Path, name: &str) -> Option<Vec<(f64, f64)>> {
    read_curve(car_dir, name)
}

/// Linearly interpolates `x` on a sorted `(x, y)` LUT, clamping at the ends.
pub fn eval_lut(points: &[(f64, f64)], x: f64) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[points.len() - 1].0 {
        return points[points.len() - 1].1;
    }
    let hi = points.partition_point(|&(px, _)| px < x);
    let (x0, y0) = points[hi - 1];
    let (x1, y1) = points[hi];
    if (x1 - x0).abs() < 1e-12 {
        y0
    } else {
        y0 + (y1 - y0) * ((x - x0) / (x1 - x0))
    }
}

/// One point on a car's dyno chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynoPoint {
    pub rpm: f64,
    pub torque_nm: f64,
    pub power_kw: f64,
    pub power_hp: f64,
}

/// Full engine dyno curve (including turbo boost when `[TURBO_0..N]` is present).
#[derive(Debug, Clone, PartialEq)]
pub struct DynoCurve {
    pub limiter_rpm: Option<f64>,
    pub points: Vec<DynoPoint>,
    pub peak_power_kw: f64,
    pub peak_torque_nm: f64,
}

/// Reads the engine dyno curve from `engine.ini` + `power.lut` (+ optional turbos).
pub fn read_dyno(car_dir: &Path) -> Option<DynoCurve> {
    let engine = load_data_ini(car_dir, "engine.ini").ok()?;
    let limiter_rpm = number(&engine, "ENGINE_DATA", "LIMITER");
    let curve_name = text(&engine, "HEADER", "POWER_CURVE").unwrap_or_else(|| "power.lut".into());
    let base = read_curve(car_dir, &curve_name)?;

    // Collect optional `[TURBO_0..N]` boost parameters.
    let mut turbos = Vec::new();
    for i in 0u32.. {
        let sec = format!("TURBO_{i}");
        let Some(max_boost) = number(&engine, &sec, "MAX_BOOST") else {
            break;
        };
        let ref_rpm = number(&engine, &sec, "REFERENCE_RPM").unwrap_or(4000.0).max(1.0);
        let gamma = number(&engine, &sec, "GAMMA").unwrap_or(1.0);
        let wastegate = number(&engine, &sec, "WASTEGATE").unwrap_or(max_boost);
        turbos.push((max_boost, ref_rpm, gamma, wastegate));
    }

    let mut points = Vec::with_capacity(base.len());
    let mut peak_power_kw = 0.0f64;
    let mut peak_torque_nm = 0.0f64;
    for (rpm, base_torque) in base {
        let boost: f64 = turbos
            .iter()
            .map(|&(max_b, ref_r, gamma, wg)| {
                let raw = max_b * (rpm / ref_r).clamp(0.0, 2.0).powf(gamma);
                if wg > 0.0 { raw.min(wg) } else { raw }
            })
            .sum();
        let torque_nm = base_torque * (1.0 + boost);
        let power_kw = torque_nm * rpm / RPM_NM_TO_WATTS_DIVISOR / 1000.0;
        let power_hp = power_kw * 1.341_022_1;
        peak_power_kw = peak_power_kw.max(power_kw);
        peak_torque_nm = peak_torque_nm.max(torque_nm);
        points.push(DynoPoint {
            rpm,
            torque_nm,
            power_kw,
            power_hp,
        });
    }

    Some(DynoCurve {
        limiter_rpm,
        points,
        peak_power_kw,
        peak_torque_nm,
    })
}

/// Drivetrain gearing and speed-per-gear calculations.
#[derive(Debug, Clone, PartialEq)]
pub struct DrivetrainData {
    pub traction: Option<String>,
    pub gears: Vec<f64>,
    pub final_ratio: f64,
    pub reverse_ratio: Option<f64>,
    pub limiter_rpm: f64,
    pub rear_tyre_radius_m: f64,
}

impl DrivetrainData {
    /// Theoretical wheel speed in km/h for 1-based forward `gear_index` at `rpm`.
    pub fn speed_kmh_at(&self, gear_index: usize, rpm: f64) -> Option<f64> {
        let ratio = *self.gears.get(gear_index.checked_sub(1)?)?;
        let total = ratio * self.final_ratio;
        if total <= 0.0 || !total.is_finite() {
            return None;
        }
        let wheel_rps = (rpm / 60.0) / total;
        let circum = std::f64::consts::TAU * self.rear_tyre_radius_m;
        Some(wheel_rps * circum * 3.6)
    }

    /// Top speed in km/h at the rev limiter for each forward gear.
    pub fn gear_top_speeds_kmh(&self) -> Vec<f64> {
        (1..=self.gears.len())
            .filter_map(|g| self.speed_kmh_at(g, self.limiter_rpm))
            .collect()
    }
}

/// Reads gearing from `drivetrain.ini`, rev limiter from `engine.ini`, and rear tyre
/// radius from `tyres.ini`.
pub fn read_drivetrain(car_dir: &Path) -> Option<DrivetrainData> {
    let dt = load_data_ini(car_dir, "drivetrain.ini").ok()?;
    let count = count(&dt, "GEARS", "COUNT")? as usize;
    let mut gears = Vec::with_capacity(count);
    for i in 1..=count {
        gears.push(number(&dt, "GEARS", &format!("GEAR_{i}"))?);
    }
    let final_ratio = number(&dt, "GEARS", "FINAL")?;
    let reverse_ratio = number(&dt, "GEARS", "GEAR_R");
    let traction = text(&dt, "TRACTION", "TYPE");
    let limiter_rpm = load_data_ini(car_dir, "engine.ini")
        .ok()
        .and_then(|e| number(&e, "ENGINE_DATA", "LIMITER"))
        .unwrap_or(7500.0);
    let rear_tyre_radius_m = load_data_ini(car_dir, "tyres.ini")
        .ok()
        .and_then(|t| number(&t, "REAR", "RADIUS"))
        .filter(|&r| r > 0.1 && r < 1.5)
        .unwrap_or(0.33);

    Some(DrivetrainData {
        traction,
        gears,
        final_ratio,
        reverse_ratio,
        limiter_rpm,
        rear_tyre_radius_m,
    })
}

/// One aerodynamic wing/body element from `aero.ini`.
#[derive(Debug, Clone, PartialEq)]
pub struct WingSpec {
    pub name: String,
    pub chord: f64,
    pub span: f64,
    pub position_z: f64,
    pub angle_deg: f64,
    pub cl_gain: f64,
    pub cd_gain: f64,
    pub lut_aoa_cl: Vec<(f64, f64)>,
    pub lut_aoa_cd: Vec<(f64, f64)>,
}

/// Aerodynamic model parsed from `aero.ini` + wing LUTs.
#[derive(Debug, Clone, PartialEq)]
pub struct AeroData {
    pub wings: Vec<WingSpec>,
}

/// Computed aerodynamic forces at a given airspeed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AeroForces {
    pub speed_kmh: f64,
    pub front_downforce_n: f64,
    pub rear_downforce_n: f64,
    pub total_downforce_n: f64,
    pub total_drag_n: f64,
    /// Front share of total downforce in `0.0..=100.0`%.
    pub front_balance_pct: f64,
}

impl AeroData {
    /// Computes downforce and drag at `speed_kmh` (air density `1.225 kg/m^3`).
    pub fn forces_at_speed(&self, speed_kmh: f64) -> AeroForces {
        let v = speed_kmh / 3.6;
        let q = 0.5 * 1.225 * v * v;
        let mut front_df = 0.0;
        let mut rear_df = 0.0;
        let mut drag = 0.0;
        for w in &self.wings {
            let area = (w.chord * w.span).max(0.0);
            let cl = eval_lut(&w.lut_aoa_cl, w.angle_deg) * w.cl_gain;
            let cd = eval_lut(&w.lut_aoa_cd, w.angle_deg) * w.cd_gain;
            let df = q * area * cl;
            let dr = q * area * cd;
            if w.position_z >= 0.0 {
                front_df += df;
            } else {
                rear_df += df;
            }
            drag += dr;
        }
        let total_df = front_df + rear_df;
        let front_balance_pct = if total_df.abs() > 1e-6 {
            (front_df / total_df * 100.0).clamp(0.0, 100.0)
        } else {
            50.0
        };
        AeroForces {
            speed_kmh,
            front_downforce_n: front_df,
            rear_downforce_n: rear_df,
            total_downforce_n: total_df,
            total_drag_n: drag,
            front_balance_pct,
        }
    }
}

/// Reads `aero.ini` and its referenced AoA CL/CD LUTs.
pub fn read_aero(car_dir: &Path) -> Option<AeroData> {
    let aero = load_data_ini(car_dir, "aero.ini").ok()?;
    let mut wings = Vec::new();
    for i in 0u32.. {
        let sec = format!("WING_{i}");
        let Some(chord) = number(&aero, &sec, "CHORD") else {
            break;
        };
        let span = number(&aero, &sec, "SPAN").unwrap_or(1.0);
        let name = text(&aero, &sec, "NAME").unwrap_or_else(|| sec.clone());
        let position_z = text(&aero, &sec, "POSITION")
            .and_then(|s| s.split(',').nth(2)?.trim().parse::<f64>().ok())
            .unwrap_or(0.0);
        let angle_deg = number(&aero, &sec, "ANGLE").unwrap_or(0.0);
        let cl_gain = number(&aero, &sec, "CL_GAIN").unwrap_or(1.0);
        let cd_gain = number(&aero, &sec, "CD_GAIN").unwrap_or(1.0);
        let lut_aoa_cl = text(&aero, &sec, "LUT_AOA_CL")
            .and_then(|f| read_lut(car_dir, &f))
            .unwrap_or_default();
        let lut_aoa_cd = text(&aero, &sec, "LUT_AOA_CD")
            .and_then(|f| read_lut(car_dir, &f))
            .unwrap_or_default();
        wings.push(WingSpec {
            name,
            chord,
            span,
            position_z,
            angle_deg,
            cl_gain,
            cd_gain,
            lut_aoa_cl,
            lut_aoa_cd,
        });
    }
    (!wings.is_empty()).then_some(AeroData { wings })
}

/// Tyre thermal & wear curves for one compound set.
#[derive(Debug, Clone, PartialEq)]
pub struct TyreCompound {
    pub index: u32,
    pub front_name: String,
    pub rear_name: String,
    pub front_ideal_psi: Option<f64>,
    pub rear_ideal_psi: Option<f64>,
    pub front_thermal_lut: Vec<(f64, f64)>,
    pub rear_thermal_lut: Vec<(f64, f64)>,
    pub front_wear_lut: Vec<(f64, f64)>,
    pub rear_wear_lut: Vec<(f64, f64)>,
}

/// Reads all tyre compounds and their thermal/wear LUTs from `tyres.ini`.
pub fn read_tyre_curves(car_dir: &Path) -> Vec<TyreCompound> {
    let Ok(ini) = load_data_ini(car_dir, "tyres.ini") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for idx in 0u32..16 {
        let f_sec = if idx == 0 { "FRONT".to_string() } else { format!("FRONT_{idx}") };
        let r_sec = if idx == 0 { "REAR".to_string() } else { format!("REAR_{idx}") };
        let Some(front_name) = text(&ini, &f_sec, "NAME") else {
            if idx == 0 { continue; } else { break; }
        };
        let rear_name = text(&ini, &r_sec, "NAME").unwrap_or_else(|| front_name.clone());
        let tf_sec = if idx == 0 { "THERMAL_FRONT".to_string() } else { format!("THERMAL_FRONT_{idx}") };
        let tr_sec = if idx == 0 { "THERMAL_REAR".to_string() } else { format!("THERMAL_REAR_{idx}") };
        out.push(TyreCompound {
            index: idx,
            front_name,
            rear_name,
            front_ideal_psi: number(&ini, &f_sec, "PRESSURE_IDEAL"),
            rear_ideal_psi: number(&ini, &r_sec, "PRESSURE_IDEAL"),
            front_thermal_lut: text(&ini, &tf_sec, "PERFORMANCE_CURVE")
                .and_then(|f| read_lut(car_dir, &f))
                .unwrap_or_default(),
            rear_thermal_lut: text(&ini, &tr_sec, "PERFORMANCE_CURVE")
                .and_then(|f| read_lut(car_dir, &f))
                .unwrap_or_default(),
            front_wear_lut: text(&ini, &f_sec, "WEAR_CURVE")
                .and_then(|f| read_lut(car_dir, &f))
                .unwrap_or_default(),
            rear_wear_lut: text(&ini, &r_sec, "WEAR_CURVE")
                .and_then(|f| read_lut(car_dir, &f))
                .unwrap_or_default(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A car folder under the temp dir, removed on drop even when a test panics.
    struct TempCar(std::path::PathBuf);

    impl TempCar {
        fn new(name: &str) -> TempCar {
            let dir = std::env::temp_dir().join(format!("acxs-cardata-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("data")).unwrap();
            TempCar(dir)
        }

        fn write(&self, file: &str, text: &str) {
            fs::write(self.0.join("data").join(file), text).unwrap();
        }
    }

    impl Drop for TempCar {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_full_car(car: &TempCar) {
        car.write(
            "car.ini",
            "[BASIC]\r\nTOTALMASS=1180.5 ; kg with driver\r\n\r\n[FUEL]\r\nMAX_FUEL=55\r\n",
        );
        car.write(
            "engine.ini",
            "[HEADER]\r\nPOWER_CURVE=power.lut\r\n\r\n[ENGINE_DATA]\r\nLIMITER=7800\r\n",
        );
        car.write(
            "power.lut",
            "; rpm|torque\r\n2000|100\r\n4000|200\r\n6000|250\r\n8000|220\r\n",
        );
        car.write("drivetrain.ini", "[TRACTION]\r\nTYPE=RWD\r\n\r\n[GEARS]\r\nCOUNT=6\r\n");
        car.write("suspensions.ini", "[BASIC]\r\nCG_LOCATION=0.55\r\n");
        car.write(
            "tyres.ini",
            "[FRONT]\r\nNAME=Slick Medium\r\nPRESSURE_IDEAL=21.5\r\n\r\n[REAR]\r\nNAME=Slick Soft\r\n",
        );
    }

    #[test]
    fn reads_every_spec_from_the_data_folder() {
        let car = TempCar::new("full");
        write_full_car(&car);
        let specs = read_specs(&car.0);

        assert_eq!(specs.car_id, format!("acxs-cardata-{}-full", std::process::id()));
        assert_eq!(specs.mass_kg, Some(1180.5));
        assert_eq!(specs.fuel_tank_l, Some(55.0));
        assert_eq!(specs.max_rpm, Some(7800.0));
        assert_eq!(specs.max_torque_nm, Some(250.0));
        // 220 Nm at 8000 rpm beats 250 Nm at 6000 rpm, so the peak power is the last point.
        let expected_power = 220.0 * 8000.0 / RPM_NM_TO_WATTS_DIVISOR;
        assert!((specs.max_power_w.unwrap() - expected_power).abs() < 1e-6);
        assert_eq!(specs.gear_count, Some(6));
        assert_eq!(specs.drive_type.as_deref(), Some("RWD"));
        assert_eq!(specs.front_weight_distribution, Some(0.55));
        assert_eq!(specs.tyres_front.as_deref(), Some("Slick Medium"));
        assert_eq!(specs.tyres_rear.as_deref(), Some("Slick Soft"));
    }

    #[test]
    fn missing_files_give_none_and_never_panic() {
        let car = TempCar::new("empty");
        let specs = read_specs(&car.0);
        assert_eq!(specs.mass_kg, None);
        assert_eq!(specs.max_power_w, None);
        assert_eq!(specs.gear_count, None);
        assert_eq!(specs.tyres_front, None);

        // A folder that does not exist at all is also just empty specs.
        let missing = std::env::temp_dir().join(format!("acxs-cardata-{}-nowhere", std::process::id()));
        let specs = read_specs(&missing);
        assert_eq!(specs, CarSpecs { car_id: specs.car_id.clone(), ..CarSpecs::default() });
    }

    #[test]
    fn a_curve_without_usable_points_gives_no_power_or_torque() {
        let car = TempCar::new("badcurve");
        car.write("engine.ini", "[HEADER]\nPOWER_CURVE=power.lut\n");
        car.write("power.lut", "garbage\n1000|x\n");
        let specs = read_specs(&car.0);
        assert_eq!(specs.max_power_w, None);
        assert_eq!(specs.max_torque_nm, None);
    }

    #[test]
    fn set_mass_backs_up_the_old_file_and_changes_the_value() {
        let car = TempCar::new("setmass");
        write_full_car(&car);

        set_mass(&car.0, 1200.0).unwrap();

        assert_eq!(read_specs(&car.0).mass_kg, Some(1200.0));
        let files: Vec<_> = fs::read_dir(car.0.join("data"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.iter().filter(|n| n.starts_with("car.ini.acxs-backup-")).count(), 1);
    }

    #[test]
    fn set_mass_errors_when_the_key_is_absent() {
        let car = TempCar::new("nokey");
        car.write("car.ini", "[BASIC]\nNAME=Test\n");
        assert!(set_mass(&car.0, 1000.0).is_err());
        assert_eq!(fs::read_to_string(car.0.join("data").join("car.ini")).unwrap(), "[BASIC]\nNAME=Test\n");
    }

    #[test]
    fn summary_lines_skip_missing_fields() {
        let specs = CarSpecs {
            car_id: "x".into(),
            mass_kg: Some(1100.0),
            gear_count: Some(5),
            ..CarSpecs::default()
        };
        let lines = summary_lines(&specs);
        assert_eq!(
            lines,
            vec![
                ("Mass".to_string(), "1100 kg".to_string()),
                ("Gears".to_string(), "5".to_string()),
            ]
        );
    }

    #[test]
    fn reads_dyno_drivetrain_aero_and_tyre_curves() {
        let car = TempCar::new("physics_lab");
        write_full_car(&car);
        car.write(
            "drivetrain.ini",
            "[TRACTION]\r\nTYPE=RWD\r\n\r\n[GEARS]\r\nCOUNT=3\r\nGEAR_1=3.0\r\nGEAR_2=2.0\r\nGEAR_3=1.2\r\nFINAL=3.5\r\nGEAR_R=-3.2\r\n",
        );
        car.write(
            "aero.ini",
            "[WING_0]\r\nNAME=FRONT\r\nCHORD=1.0\r\nSPAN=1.5\r\nPOSITION=0,0.2,1.5\r\nLUT_AOA_CL=wing_f_cl.lut\r\nLUT_AOA_CD=wing_f_cd.lut\r\nCL_GAIN=1.0\r\nCD_GAIN=1.0\r\nANGLE=4\r\n\r\n\
             [WING_1]\r\nNAME=REAR\r\nCHORD=1.0\r\nSPAN=1.5\r\nPOSITION=0,0.8,-1.8\r\nLUT_AOA_CL=wing_f_cl.lut\r\nLUT_AOA_CD=wing_f_cd.lut\r\nCL_GAIN=1.2\r\nCD_GAIN=1.0\r\nANGLE=4\r\n",
        );
        car.write("wing_f_cl.lut", "0|0.4\r\n8|1.2\r\n");
        car.write("wing_f_cd.lut", "0|0.1\r\n8|0.3\r\n");
        car.write(
            "tyres.ini",
            "[FRONT]\r\nNAME=Slick\r\nRADIUS=0.33\r\nPRESSURE_IDEAL=22\r\nWEAR_CURVE=wear.lut\r\n\r\n\
             [REAR]\r\nNAME=Slick\r\nRADIUS=0.34\r\nPRESSURE_IDEAL=23\r\n\r\n\
             [THERMAL_FRONT]\r\nPERFORMANCE_CURVE=thermal.lut\r\n",
        );
        car.write("wear.lut", "0|100\r\n100|80\r\n");
        car.write("thermal.lut", "20|0.90\r\n80|1.00\r\n120|0.92\r\n");

        let dyno = read_dyno(&car.0).unwrap();
        assert_eq!(dyno.points.len(), 4);
        assert!(dyno.peak_power_kw > 180.0);

        let dt = read_drivetrain(&car.0).unwrap();
        let tops = dt.gear_top_speeds_kmh();
        assert_eq!(tops.len(), 3);
        assert!(tops[0] < tops[1] && tops[1] < tops[2]);

        let aero = read_aero(&car.0).unwrap();
        let forces = aero.forces_at_speed(200.0);
        assert!(forces.total_downforce_n > 1000.0);
        assert!(forces.front_balance_pct > 40.0 && forces.front_balance_pct < 50.0);

        let tyres = read_tyre_curves(&car.0);
        assert_eq!(tyres.len(), 1);
        assert_eq!(tyres[0].front_thermal_lut.len(), 3);
        assert_eq!(tyres[0].front_wear_lut.len(), 2);
    }
}
