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
}
