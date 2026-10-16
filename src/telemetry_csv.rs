//! CSV export of decoded telemetry, for spreadsheets and analysis tools.
//!
//! Three kinds of CSV are produced:
//!   * one row per [`CarUpdate`] sample, see [`car_updates_to_csv`];
//!   * one row per completed [`LapInfo`], see [`laps_to_csv`];
//!   * one row per [`Sample`] of a replay, see [`replay_samples_to_csv`].
//!
//! Output follows RFC 4180: every line, including the header and the last
//! row, ends in CRLF, and a field that contains a comma, a double quote or a
//! line break is wrapped in double quotes with embedded quotes doubled.
//! Floats use fixed precision, so the same input always yields the same bytes.
//!
//! [`CarUpdate`] carries no timestamp, so `time_s` is derived from the sample
//! index as `index / sample_hz`. The caller supplies the rate at which the
//! samples were received or recorded. Non-finite floats are written as Rust
//! formats them (`NaN`, `inf`).

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use crate::replay::Sample;
use crate::telemetry::{CarUpdate, LapInfo};

const NEWLINE: &str = "\r\n";
const CAR_UPDATE_HEADER: &str =
    "time_s,speed_kmh,gear,rpm,position_x,position_y,position_z,lap_time_ms";
const LAP_HEADER: &str = "car_id,laps,lap_time_ms,lap_time,driver,car";
const REPLAY_SAMPLE_HEADER: &str = "time_s,speed_kmh,rpm,gear,throttle,brake,x,y,z";

/// Errors returned by [`car_updates_to_csv`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CsvError {
    /// `sample_hz` was not finite, or was not greater than zero.
    InvalidRate(f64),
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CsvError::InvalidRate(hz) => {
                write!(f, "invalid sample rate {hz}: must be finite and greater than zero")
            }
        }
    }
}

impl std::error::Error for CsvError {}

/// Renders car update samples as CSV with a header row.
///
/// Columns: `time_s` (sample index divided by `sample_hz`, 6 decimals),
/// `speed_kmh` (2 decimals), `gear`, `rpm`, `position_x`, `position_y`,
/// `position_z` (3 decimals), `lap_time_ms`.
///
/// Returns [`CsvError::InvalidRate`] if `sample_hz` is NaN, infinite, zero or
/// negative. An empty slice yields the header alone.
pub fn car_updates_to_csv(samples: &[CarUpdate], sample_hz: f64) -> Result<String, CsvError> {
    if !sample_hz.is_finite() || sample_hz <= 0.0 {
        return Err(CsvError::InvalidRate(sample_hz));
    }
    let mut out = String::with_capacity(CAR_UPDATE_HEADER.len() + samples.len() * 64);
    out.push_str(CAR_UPDATE_HEADER);
    out.push_str(NEWLINE);
    for (index, sample) in samples.iter().enumerate() {
        let time_s = index as f64 / sample_hz;
        let (x, y, z) = sample.position;
        let fields = [
            format!("{time_s:.6}"),
            format!("{:.2}", sample.speed_kmh),
            sample.gear.to_string(),
            sample.rpm.to_string(),
            format!("{x:.3}"),
            format!("{y:.3}"),
            format!("{z:.3}"),
            sample.lap_time_ms.to_string(),
        ];
        out.push_str(&fields.join(","));
        out.push_str(NEWLINE);
    }
    Ok(out)
}

/// Renders completed laps as CSV with a header row, one row per lap.
///
/// Columns: `car_id`, `laps`, `lap_time_ms`, `lap_time` (`m:ss.mmm`), `driver`,
/// `car`. Driver and car names are escaped per RFC 4180. An empty slice yields
/// the header alone.
pub fn laps_to_csv(laps: &[LapInfo]) -> String {
    let mut out = String::with_capacity(LAP_HEADER.len() + laps.len() * 48);
    out.push_str(LAP_HEADER);
    out.push_str(NEWLINE);
    for lap in laps {
        let fields = [
            lap.car_id.to_string(),
            lap.laps.to_string(),
            lap.lap_time_ms.to_string(),
            format_lap_time(lap.lap_time_ms),
            escape_field(&lap.driver),
            escape_field(&lap.car),
        ];
        out.push_str(&fields.join(","));
        out.push_str(NEWLINE);
    }
    out
}

/// Renders replay samples as CSV with a header row, one row per sample.
///
/// Columns: `time_s` (taken from [`Sample::time_s`], 6 decimals), `speed_kmh`
/// (2 decimals), `rpm` (whole numbers), `gear` (raw gear code, see
/// [`Sample::gear`]), `throttle` and `brake` (3 decimals, 0.0 to 1.0), `x`, `y`
/// and `z` (3 decimals, metres). An empty slice yields the header alone.
pub fn replay_samples_to_csv(samples: &[Sample]) -> String {
    let mut out = String::with_capacity(REPLAY_SAMPLE_HEADER.len() + samples.len() * 80);
    out.push_str(REPLAY_SAMPLE_HEADER);
    out.push_str(NEWLINE);
    for sample in samples {
        let fields = [
            format!("{:.6}", sample.time_s),
            format!("{:.2}", sample.speed_kmh),
            format!("{:.0}", sample.rpm),
            sample.gear.to_string(),
            format!("{:.3}", sample.throttle),
            format!("{:.3}", sample.brake),
            format!("{:.3}", sample.x),
            format!("{:.3}", sample.y),
            format!("{:.3}", sample.z),
        ];
        out.push_str(&fields.join(","));
        out.push_str(NEWLINE);
    }
    out
}

/// Writes `content` to a new file at `path`, creating parent directories.
///
/// Never overwrites: if `path` already exists the call fails with
/// [`io::ErrorKind::AlreadyExists`] and the existing file is left untouched.
/// Nothing is deleted. If the write itself fails part way, the partial file is
/// kept for inspection.
pub fn write_csv(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(content.as_bytes())
}

/// Formats milliseconds as `m:ss.mmm`. Minutes are not capped at 59.
fn format_lap_time(ms: u32) -> String {
    let minutes = ms / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

/// Quotes a field if it contains a comma, double quote, CR or LF (RFC 4180).
fn escape_field(field: &str) -> String {
    if !field.contains([',', '"', '\r', '\n']) {
        return field.to_string();
    }
    let mut out = String::with_capacity(field.len() + 2);
    out.push('"');
    for c in field.chars() {
        if c == '"' {
            out.push_str("\"\"");
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A uniquely named directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("acxs_telemetry_csv_{}_{test}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn car(speed_kmh: f32, gear: i8, rpm: u16, position: (f32, f32, f32), lap_time_ms: u32) -> CarUpdate {
        CarUpdate {
            speed_kmh,
            gear,
            rpm,
            position,
            lap_time_ms,
            throttle: 0.0,
            brake: 0.0,
            steer_angle: 0.0,
            g_lateral: 0.0,
            g_longitudinal: 0.0,
            g_vertical: 0.0,
        }
    }

    fn lap(car_id: u8, laps: u16, lap_time_ms: u32, driver: &str, car: &str) -> LapInfo {
        LapInfo {
            car_id,
            lap_time_ms,
            laps,
            driver: driver.to_string(),
            car: car.to_string(),
        }
    }

    fn three_samples() -> Vec<CarUpdate> {
        vec![
            car(187.5, 4, 7413, (10.5, -2.25, 300.0), 61_234),
            car(190.0, 5, 7800, (11.0, -2.0, 300.5), 61_300),
            car(0.0, 0, 900, (0.0, 0.0, 0.0), 0),
        ]
    }

    // ---- car updates ----

    #[test]
    fn car_csv_has_header_and_crlf_lines() {
        let out = car_updates_to_csv(&three_samples(), 10.0).expect("valid rate");
        assert!(out.starts_with(&format!("{CAR_UPDATE_HEADER}\r\n")));
        assert!(out.ends_with("\r\n"));
        assert!(!out.replace("\r\n", "").contains('\n'), "bare LF found");
    }

    #[test]
    fn car_csv_row_count_matches_samples() {
        let out = car_updates_to_csv(&three_samples(), 10.0).expect("valid rate");
        // one header line plus one line per sample
        assert_eq!(out.split_terminator("\r\n").count(), 4);
    }

    #[test]
    fn car_csv_empty_is_header_only() {
        let out = car_updates_to_csv(&[], 60.0).expect("valid rate");
        assert_eq!(out, format!("{CAR_UPDATE_HEADER}\r\n"));
    }

    #[test]
    fn car_csv_is_deterministic() {
        let samples = three_samples();
        let a = car_updates_to_csv(&samples, 59.94).expect("valid rate");
        let b = car_updates_to_csv(&samples, 59.94).expect("valid rate");
        assert_eq!(a, b);
    }

    #[test]
    fn car_csv_row_values_and_time_column() {
        let out = car_updates_to_csv(&three_samples(), 10.0).expect("valid rate");
        let rows: Vec<&str> = out.split_terminator("\r\n").collect();
        assert_eq!(rows[1], "0.000000,187.50,4,7413,10.500,-2.250,300.000,61234");
        assert_eq!(rows[2], "0.100000,190.00,5,7800,11.000,-2.000,300.500,61300");
        assert_eq!(rows[3], "0.200000,0.00,0,900,0.000,0.000,0.000,0");
    }

    #[test]
    fn car_csv_time_uses_sample_rate() {
        let out = car_updates_to_csv(&three_samples(), 4.0).expect("valid rate");
        let last = out.split_terminator("\r\n").last().expect("row");
        assert!(last.starts_with("0.500000,"), "got {last}");
    }

    #[test]
    fn car_csv_rejects_invalid_rates() {
        let samples = three_samples();
        for hz in [0.0, -60.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let result = car_updates_to_csv(&samples, hz);
            assert!(
                matches!(result, Err(CsvError::InvalidRate(_))),
                "rate {hz} should be rejected, got {result:?}"
            );
        }
    }

    #[test]
    fn csv_error_display_is_informative() {
        let msg = CsvError::InvalidRate(-1.0).to_string();
        assert!(msg.contains("-1"), "message was {msg}");
    }

    // ---- laps ----

    #[test]
    fn lap_csv_has_header_and_rows() {
        let laps = [lap(3, 1, 92_500, "Max", "ks_mazda_mx5"), lap(3, 2, 91_000, "Max", "ks_mazda_mx5")];
        let out = laps_to_csv(&laps);
        let rows: Vec<&str> = out.split_terminator("\r\n").collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], LAP_HEADER);
        assert_eq!(rows[1], "3,1,92500,1:32.500,Max,ks_mazda_mx5");
        assert_eq!(rows[2], "3,2,91000,1:31.000,Max,ks_mazda_mx5");
    }

    #[test]
    fn lap_csv_empty_is_header_only() {
        assert_eq!(laps_to_csv(&[]), format!("{LAP_HEADER}\r\n"));
    }

    #[test]
    fn lap_time_formatting() {
        assert_eq!(format_lap_time(0), "0:00.000");
        assert_eq!(format_lap_time(59_999), "0:59.999");
        assert_eq!(format_lap_time(92_500), "1:32.500");
        assert_eq!(format_lap_time(3_600_000), "60:00.000");
    }

    #[test]
    fn driver_with_comma_and_quote_is_escaped() {
        let laps = [lap(1, 1, 90_000, "Doe, \"Jane\"", "ks_bmw_m3")];
        let out = laps_to_csv(&laps);
        let row = out.split_terminator("\r\n").nth(1).expect("row");
        assert_eq!(row, "1,1,90000,1:30.000,\"Doe, \"\"Jane\"\"\",ks_bmw_m3");
    }

    #[test]
    fn car_name_with_newline_is_quoted() {
        let laps = [lap(1, 1, 90_000, "Max", "line one\nline two")];
        let out = laps_to_csv(&laps);
        assert!(out.ends_with("Max,\"line one\nline two\"\r\n"), "got {out:?}");
    }

    #[test]
    fn plain_fields_are_not_quoted() {
        assert_eq!(escape_field("ks_ferrari_488_gt3"), "ks_ferrari_488_gt3");
        assert_eq!(escape_field(""), "");
    }

    // ---- replay samples ----

    fn replay_sample(
        time_s: f64,
        speed_kmh: f32,
        rpm: f32,
        gear: u8,
        throttle: f32,
        brake: f32,
        position: (f32, f32, f32),
    ) -> Sample {
        Sample {
            time_s,
            speed_kmh,
            rpm,
            gear,
            throttle,
            brake,
            x: position.0,
            y: position.1,
            z: position.2,
        }
    }

    fn three_replay_samples() -> Vec<Sample> {
        vec![
            replay_sample(0.0, 187.5, 7413.0, 4, 0.5, 0.0, (10.5, -2.25, 300.0)),
            replay_sample(0.1, 190.0, 7800.0, 5, 1.0, 0.0, (11.0, -2.0, 300.5)),
            replay_sample(0.2, 0.0, 900.0, 1, 0.0, 1.0, (0.0, 0.0, 0.0)),
        ]
    }

    #[test]
    fn replay_csv_has_exact_header_and_crlf_lines() {
        let out = replay_samples_to_csv(&three_replay_samples());
        let header = out.split("\r\n").next().expect("header");
        assert_eq!(header, "time_s,speed_kmh,rpm,gear,throttle,brake,x,y,z");
        assert!(out.ends_with("\r\n"));
        assert!(!out.replace("\r\n", "").contains('\n'), "bare LF found");
    }

    #[test]
    fn replay_csv_row_count_matches_samples() {
        let out = replay_samples_to_csv(&three_replay_samples());
        // one header line plus one line per sample
        assert_eq!(out.split_terminator("\r\n").count(), 4);
    }

    #[test]
    fn replay_csv_empty_is_header_only() {
        assert_eq!(
            replay_samples_to_csv(&[]),
            format!("{REPLAY_SAMPLE_HEADER}\r\n")
        );
    }

    #[test]
    fn replay_csv_is_deterministic() {
        let samples = three_replay_samples();
        assert_eq!(replay_samples_to_csv(&samples), replay_samples_to_csv(&samples));
    }

    #[test]
    fn replay_csv_row_values_use_fixed_precision() {
        let out = replay_samples_to_csv(&three_replay_samples());
        let rows: Vec<&str> = out.split_terminator("\r\n").collect();
        assert_eq!(rows[1], "0.000000,187.50,7413,4,0.500,0.000,10.500,-2.250,300.000");
        assert_eq!(rows[2], "0.100000,190.00,7800,5,1.000,0.000,11.000,-2.000,300.500");
        assert_eq!(rows[3], "0.200000,0.00,900,1,0.000,1.000,0.000,0.000,0.000");
    }

    // ---- write_csv ----

    #[test]
    fn write_csv_creates_parent_dirs_and_writes_content() {
        let tmp = TempDir::new("write_creates");
        let path = tmp.path().join("nested").join("deeper").join("laps.csv");
        let content = laps_to_csv(&[lap(1, 1, 90_000, "Max", "mx5")]);
        write_csv(&path, &content).expect("first write");
        assert_eq!(fs::read_to_string(&path).expect("read back"), content);
    }

    #[test]
    fn write_csv_refuses_to_overwrite() {
        let tmp = TempDir::new("write_refuse");
        let path = tmp.path().join("telemetry.csv");
        write_csv(&path, "first\r\n").expect("first write");

        let err = write_csv(&path, "second\r\n").expect_err("second write must fail");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).expect("read back"), "first\r\n");
    }
}
