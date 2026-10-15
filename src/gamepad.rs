//! Settings of Advanced Gamepad Assist (AGA), a CSP input helper for Assetto Corsa.
//!
//! Option names and meanings follow the mod's own user guide. The ranges and
//! defaults are the ones its settings UI uses (its slider limits and the storage
//! defaults its script declares), since the guide does not state them. Values are
//! stored the way the mod stores them: percentages
//! as fractions (`0.5` = 50%), except `maxDynamicLimitReduction` (0..10, shown
//! as 0..100%) and `maxSelfSteerAngle` (degrees).
//!
//! The mod does not document where its settings are stored. It keeps them with
//! `ac.storage` (keys prefixed `AGA_`). This module writes them to
//! `extension/config/gamepad_fx.ini`, section `JOYPAD_ASSIST`, which is the CSP
//! config file the mod's settings UI writes. Check this against a real install
//! before writing settings for a user.
//!
//! Keys this module does not own (anything not listed in [`GamepadSettings`],
//! and any other section) are kept by [`write_settings`].
//!
//! AC's own steering gamma, deadzone and rumble settings (`controls.ini`,
//! `[X360]`) live in another file, so they are not here.

use std::io;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// Path of the CSP config file, relative to the Assetto Corsa root.
const CONFIG_FILE: &[&str] = &["extension", "config", "gamepad_fx.ini"];
/// Section holding the AGA settings.
const SECTION: &str = "JOYPAD_ASSIST";
/// Prefix of every AGA setting key, as set by `ac.storage(..., "AGA_")`.
const KEY_PREFIX: &str = "AGA_";

/// Graphs shown next to the settings UI (Extra settings > Graphs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphMode {
    None,
    Static,
    Live,
}

impl GraphMode {
    fn code(self) -> i64 {
        match self {
            GraphMode::None => 1,
            GraphMode::Static => 2,
            GraphMode::Live => 3,
        }
    }

    fn from_code(code: i64) -> Option<Self> {
        match code {
            1 => Some(GraphMode::None),
            2 => Some(GraphMode::Static),
            3 => Some(GraphMode::Live),
            _ => None,
        }
    }
}

/// Keyboard driving (Extra settings > Keyboard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardMode {
    Off,
    On,
    BrakeHelp,
    GasAndBrakeHelp,
}

impl KeyboardMode {
    fn code(self) -> i64 {
        match self {
            KeyboardMode::Off => 0,
            KeyboardMode::On => 1,
            KeyboardMode::BrakeHelp => 2,
            KeyboardMode::GasAndBrakeHelp => 3,
        }
    }

    fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(KeyboardMode::Off),
            1 => Some(KeyboardMode::On),
            2 => Some(KeyboardMode::BrakeHelp),
            3 => Some(KeyboardMode::GasAndBrakeHelp),
            _ => None,
        }
    }
}

/// Gear shifting (Extra settings > Shifting mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftingMode {
    Default,
    Manual,
    Automatic,
}

impl ShiftingMode {
    fn code(self) -> i64 {
        match self {
            ShiftingMode::Default => 0,
            ShiftingMode::Manual => 1,
            ShiftingMode::Automatic => 2,
        }
    }

    fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(ShiftingMode::Default),
            1 => Some(ShiftingMode::Manual),
            2 => Some(ShiftingMode::Automatic),
            _ => None,
        }
    }
}

/// All documented AGA options, with their stored units.
#[derive(Debug, Clone, PartialEq)]
pub struct GamepadSettings {
    /// General > Enable Advanced Gamepad Assist (`assistEnabled`).
    pub assist_enabled: bool,
    /// General > Simplified settings (`useFilter`).
    pub simplified: bool,
    /// Simplified mode's single slider, "Steering assistance" (`filterSetting`), 0..=1.
    pub simplified_strength: f64,
    /// Steering input > Steering rate (`steeringRate`), 0..=1.
    pub steering_rate: f64,
    /// Steering input > Steering rate at speed (`rateIncreaseWithSpeed`), -0.5..=0.5.
    pub rate_increase_with_speed: f64,
    /// Steering input > Target slip angle (`targetSlip`), 0.9..=1.2.
    pub target_slip: f64,
    /// Steering input > Countersteer response (`countersteerResponse`), 0..=1.
    pub countersteer_response: f64,
    /// Steering input > Dynamic steering limit (`maxDynamicLimitReduction`), 0..=10 (shown as 0..100%).
    pub dynamic_limit_reduction: f64,
    /// Self-steer force > Response (`selfSteerResponse`), 0..=1.
    pub self_steer_response: f64,
    /// Self-steer force > Max angle (`maxSelfSteerAngle`), degrees, 0..=90.
    pub max_self_steer_angle: f64,
    /// Self-steer force > Damping (`dampingStrength`), 0..=1.
    pub damping: f64,
    /// Extra > Graphs (`graphSelection`).
    pub graphs: GraphMode,
    /// Extra > Keyboard (`keyboardMode`).
    pub keyboard: KeyboardMode,
    /// Extra > Photo mode (`photoMode`).
    pub photo_mode: bool,
    /// Extra > Automatic clutch (`autoClutch`).
    pub auto_clutch: bool,
    /// Extra > Shifting mode (`autoShiftingMode`).
    pub shifting_mode: ShiftingMode,
    /// Extra > Shifting mode > Auto-switch into cruise mode (`autoShiftingCruise`).
    pub auto_shifting_cruise: bool,
    /// Extra > Shifting mode > Downshift bias (`autoShiftingDownBias`), 0.1..=0.9.
    pub downshift_bias: f64,
    /// Extra > Left trigger feedback (`triggerFeedbackL`), 0..=1.
    pub trigger_feedback_left: f64,
    /// Extra > Right trigger feedback (`triggerFeedbackR`), 0..=1.
    pub trigger_feedback_right: f64,
    /// Extra > Trigger feedback with ABS/TCS (`triggerFeedbackAlwaysOn`).
    pub trigger_feedback_with_abs_tcs: bool,
}

impl Default for GamepadSettings {
    fn default() -> Self {
        GamepadSettings {
            assist_enabled: true,
            simplified: true,
            simplified_strength: 0.5,
            steering_rate: 0.5,
            rate_increase_with_speed: 0.0,
            target_slip: 0.95,
            countersteer_response: 0.2,
            dynamic_limit_reduction: 5.0,
            self_steer_response: 0.37,
            max_self_steer_angle: 90.0,
            damping: 0.37,
            graphs: GraphMode::None,
            keyboard: KeyboardMode::Off,
            photo_mode: false,
            auto_clutch: true,
            shifting_mode: ShiftingMode::Default,
            auto_shifting_cruise: true,
            downshift_bias: 0.9,
            trigger_feedback_left: 0.4,
            trigger_feedback_right: 0.4,
            trigger_feedback_with_abs_tcs: false,
        }
    }
}

/// Path of the AGA settings file under an Assetto Corsa root folder.
pub fn config_path(ac_root: &Path) -> PathBuf {
    CONFIG_FILE.iter().fold(ac_root.to_path_buf(), |path, part| path.join(part))
}

/// Reads the settings from `path`. A missing file gives the defaults. Missing
/// keys also take their defaults. A value that cannot be parsed is an error.
/// Out-of-range values are read as they are; use [`validate`] to check them.
pub fn read_settings(path: &Path) -> io::Result<GamepadSettings> {
    let ini = match Ini::load(path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(GamepadSettings::default()),
        Err(e) => return Err(e),
    };
    let d = GamepadSettings::default();
    Ok(GamepadSettings {
        assist_enabled: get_bool(&ini, "assistEnabled", d.assist_enabled)?,
        simplified: get_bool(&ini, "useFilter", d.simplified)?,
        simplified_strength: get_f64(&ini, "filterSetting", d.simplified_strength)?,
        steering_rate: get_f64(&ini, "steeringRate", d.steering_rate)?,
        rate_increase_with_speed: get_f64(&ini, "rateIncreaseWithSpeed", d.rate_increase_with_speed)?,
        target_slip: get_f64(&ini, "targetSlip", d.target_slip)?,
        countersteer_response: get_f64(&ini, "countersteerResponse", d.countersteer_response)?,
        dynamic_limit_reduction: get_f64(&ini, "maxDynamicLimitReduction", d.dynamic_limit_reduction)?,
        self_steer_response: get_f64(&ini, "selfSteerResponse", d.self_steer_response)?,
        max_self_steer_angle: get_f64(&ini, "maxSelfSteerAngle", d.max_self_steer_angle)?,
        damping: get_f64(&ini, "dampingStrength", d.damping)?,
        graphs: get_enum(&ini, "graphSelection", d.graphs.code(), GraphMode::from_code)?,
        keyboard: get_enum(&ini, "keyboardMode", d.keyboard.code(), KeyboardMode::from_code)?,
        photo_mode: get_bool(&ini, "photoMode", d.photo_mode)?,
        auto_clutch: get_bool(&ini, "autoClutch", d.auto_clutch)?,
        shifting_mode: get_enum(&ini, "autoShiftingMode", d.shifting_mode.code(), ShiftingMode::from_code)?,
        auto_shifting_cruise: get_bool(&ini, "autoShiftingCruise", d.auto_shifting_cruise)?,
        downshift_bias: get_f64(&ini, "autoShiftingDownBias", d.downshift_bias)?,
        trigger_feedback_left: get_f64(&ini, "triggerFeedbackL", d.trigger_feedback_left)?,
        trigger_feedback_right: get_f64(&ini, "triggerFeedbackR", d.trigger_feedback_right)?,
        trigger_feedback_with_abs_tcs: get_bool(&ini, "triggerFeedbackAlwaysOn", d.trigger_feedback_with_abs_tcs)?,
    })
}

/// Writes the settings to `path`. The existing file is kept as a backup
/// (renamed, never deleted), and its other sections, keys and comments are
/// kept as they are. Settings that fail [`validate`] are not written.
pub fn write_settings(path: &Path, settings: &GamepadSettings) -> io::Result<()> {
    let problems = validate(settings);
    if !problems.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, problems.join("; ")));
    }
    let mut ini = match Ini::load(path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    let s = settings;
    set_bool(&mut ini, "assistEnabled", s.assist_enabled);
    set_bool(&mut ini, "useFilter", s.simplified);
    set_f64(&mut ini, "filterSetting", s.simplified_strength);
    set_f64(&mut ini, "steeringRate", s.steering_rate);
    set_f64(&mut ini, "rateIncreaseWithSpeed", s.rate_increase_with_speed);
    set_f64(&mut ini, "targetSlip", s.target_slip);
    set_f64(&mut ini, "countersteerResponse", s.countersteer_response);
    set_f64(&mut ini, "maxDynamicLimitReduction", s.dynamic_limit_reduction);
    set_f64(&mut ini, "selfSteerResponse", s.self_steer_response);
    set_f64(&mut ini, "maxSelfSteerAngle", s.max_self_steer_angle);
    set_f64(&mut ini, "dampingStrength", s.damping);
    set_int(&mut ini, "graphSelection", s.graphs.code());
    set_int(&mut ini, "keyboardMode", s.keyboard.code());
    set_bool(&mut ini, "photoMode", s.photo_mode);
    set_bool(&mut ini, "autoClutch", s.auto_clutch);
    set_int(&mut ini, "autoShiftingMode", s.shifting_mode.code());
    set_bool(&mut ini, "autoShiftingCruise", s.auto_shifting_cruise);
    set_f64(&mut ini, "autoShiftingDownBias", s.downshift_bias);
    set_f64(&mut ini, "triggerFeedbackL", s.trigger_feedback_left);
    set_f64(&mut ini, "triggerFeedbackR", s.trigger_feedback_right);
    set_bool(&mut ini, "triggerFeedbackAlwaysOn", s.trigger_feedback_with_abs_tcs);
    ini.save_with_backup(path)
}

/// Checks every value against the documented ranges. Returns one message per
/// problem, empty when all is fine.
pub fn validate(s: &GamepadSettings) -> Vec<String> {
    let mut problems = Vec::new();
    let mut range = |name: &str, value: f64, min: f64, max: f64| {
        // `contains` is false for NaN, so non-finite values are reported too.
        if !(min..=max).contains(&value) {
            problems.push(format!("{name} must be between {min} and {max} (got {value})"));
        }
    };
    range("Steering assistance", s.simplified_strength, 0.0, 1.0);
    range("Steering rate", s.steering_rate, 0.0, 1.0);
    range("Steering rate at speed", s.rate_increase_with_speed, -0.5, 0.5);
    range("Target slip angle", s.target_slip, 0.9, 1.2);
    range("Countersteer response", s.countersteer_response, 0.0, 1.0);
    range("Dynamic steering limit", s.dynamic_limit_reduction, 0.0, 10.0);
    range("Self-steer response", s.self_steer_response, 0.0, 1.0);
    range("Self-steer max angle", s.max_self_steer_angle, 0.0, 90.0);
    range("Self-steer damping", s.damping, 0.0, 1.0);
    range("Downshift bias", s.downshift_bias, 0.1, 0.9);
    range("Left trigger feedback", s.trigger_feedback_left, 0.0, 1.0);
    range("Right trigger feedback", s.trigger_feedback_right, 0.0, 1.0);
    problems
}

fn storage_key(key: &str) -> String {
    format!("{KEY_PREFIX}{key}")
}

fn invalid_data(key: &str, raw: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("bad value for {key}: {raw:?}"))
}

fn get_f64(ini: &Ini, key: &str, default: f64) -> io::Result<f64> {
    match ini.get(SECTION, &storage_key(key)) {
        None => Ok(default),
        Some(raw) => raw.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| invalid_data(key, raw)),
    }
}

fn get_bool(ini: &Ini, key: &str, default: bool) -> io::Result<bool> {
    match ini.get(SECTION, &storage_key(key)) {
        None => Ok(default),
        Some(raw) => match raw.to_ascii_lowercase().as_str() {
            "1" | "true" => Ok(true),
            "0" | "false" => Ok(false),
            _ => Err(invalid_data(key, raw)),
        },
    }
}

fn get_enum<T>(ini: &Ini, key: &str, default_code: i64, from_code: fn(i64) -> Option<T>) -> io::Result<T> {
    let code = match ini.get(SECTION, &storage_key(key)) {
        None => default_code,
        Some(raw) => raw.parse::<i64>().map_err(|_| invalid_data(key, raw))?,
    };
    from_code(code).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("bad value for {key}: {code}")))
}

fn set_f64(ini: &mut Ini, key: &str, value: f64) {
    ini.set(SECTION, &storage_key(key), value);
}

fn set_bool(ini: &mut Ini, key: &str, value: bool) {
    // The mod's own config writes flags as 1 and 0.
    ini.set(SECTION, &storage_key(key), if value { 1 } else { 0 });
}

fn set_int(ini: &mut Ini, key: &str, value: i64) {
    ini.set(SECTION, &storage_key(key), value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Temp folder unique to this test run, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("acxs-gamepad-{}-{}", std::process::id(), name));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn defaults_match_documented_values() {
        let d = GamepadSettings::default();
        assert!(d.assist_enabled);
        assert_eq!(d.steering_rate, 0.5);
        assert_eq!(d.rate_increase_with_speed, 0.0);
        assert_eq!(d.max_self_steer_angle, 90.0);
        assert_eq!(d.graphs, GraphMode::None);
        assert_eq!(d.keyboard, KeyboardMode::Off);
        assert_eq!(d.shifting_mode, ShiftingMode::Default);
        assert!(validate(&d).is_empty());
    }

    #[test]
    fn config_path_is_under_extension_config() {
        let p = config_path(Path::new("/games/AC"));
        assert_eq!(p, Path::new("/games/AC").join("extension").join("config").join("gamepad_fx.ini"));
    }

    #[test]
    fn missing_file_reads_as_defaults() {
        let dir = TempDir::new("missing");
        let got = read_settings(&dir.0.join("nope.ini")).unwrap();
        assert_eq!(got, GamepadSettings::default());
    }

    #[test]
    fn round_trip_through_a_file() {
        let dir = TempDir::new("roundtrip");
        let path = dir.0.join("gamepad_fx.ini");
        let s = GamepadSettings {
            assist_enabled: false,
            simplified: false,
            simplified_strength: 0.8,
            steering_rate: 0.35,
            rate_increase_with_speed: -0.2,
            target_slip: 1.1,
            countersteer_response: 0.45,
            dynamic_limit_reduction: 7.5,
            self_steer_response: 0.6,
            max_self_steer_angle: 5.0,
            damping: 0.25,
            graphs: GraphMode::Live,
            keyboard: KeyboardMode::GasAndBrakeHelp,
            photo_mode: true,
            auto_clutch: false,
            shifting_mode: ShiftingMode::Automatic,
            auto_shifting_cruise: false,
            downshift_bias: 0.5,
            trigger_feedback_left: 0.1,
            trigger_feedback_right: 0.9,
            trigger_feedback_with_abs_tcs: true,
        };
        write_settings(&path, &s).unwrap();
        assert_eq!(read_settings(&path).unwrap(), s);
    }

    #[test]
    fn unknown_keys_and_sections_are_preserved() {
        let dir = TempDir::new("unknown");
        let path = dir.0.join("gamepad_fx.ini");
        let original = "; my comment\r\n[OTHER]\r\nFOO=bar\r\n[JOYPAD_ASSIST]\r\nENABLED=1\r\nAGA_mystery=42\r\nAGA_steeringRate=0.3\r\n";
        fs::write(&path, original).unwrap();

        let mut s = read_settings(&path).unwrap();
        assert_eq!(s.steering_rate, 0.3);
        s.steering_rate = 0.4;
        write_settings(&path, &s).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("; my comment\r\n"));
        assert!(text.contains("[OTHER]\r\nFOO=bar\r\n"));
        assert!(text.contains("ENABLED=1\r\n"));
        assert!(text.contains("AGA_mystery=42\r\n"));
        assert!(text.contains("AGA_steeringRate=0.4\r\n"));
        assert_eq!(read_settings(&path).unwrap().steering_rate, 0.4);

        // Older backup is renamed, not deleted.
        let backups = fs::read_dir(&dir.0)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains("acxs-backup"))
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn unparseable_value_is_an_error() {
        let dir = TempDir::new("badvalue");
        let path = dir.0.join("gamepad_fx.ini");
        fs::write(&path, "[JOYPAD_ASSIST]\r\nAGA_steeringRate=fast\r\n").unwrap();
        let err = read_settings(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn unknown_enum_code_is_an_error() {
        let dir = TempDir::new("badenum");
        let path = dir.0.join("gamepad_fx.ini");
        fs::write(&path, "[JOYPAD_ASSIST]\r\nAGA_keyboardMode=7\r\n").unwrap();
        assert_eq!(read_settings(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn validate_reports_out_of_range_values() {
        let s = GamepadSettings {
            steering_rate: 1.5,
            target_slip: 0.5,
            max_self_steer_angle: 120.0,
            downshift_bias: 0.05,
            damping: f64::NAN,
            ..GamepadSettings::default()
        };
        let problems = validate(&s);
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("Steering rate ")));
        assert!(problems.iter().any(|p| p.starts_with("Target slip angle ")));
        assert!(problems.iter().any(|p| p.starts_with("Self-steer max angle ")));
        assert!(problems.iter().any(|p| p.starts_with("Downshift bias ")));
        assert!(problems.iter().any(|p| p.starts_with("Self-steer damping ")));
    }

    #[test]
    fn validate_accepts_range_edges() {
        let s = GamepadSettings {
            rate_increase_with_speed: -0.5,
            target_slip: 1.2,
            dynamic_limit_reduction: 10.0,
            max_self_steer_angle: 0.0,
            downshift_bias: 0.9,
            ..GamepadSettings::default()
        };
        assert!(validate(&s).is_empty());
    }

    #[test]
    fn write_refuses_out_of_range_values() {
        let dir = TempDir::new("refuse");
        let path = dir.0.join("gamepad_fx.ini");
        let s = GamepadSettings { steering_rate: 2.0, ..GamepadSettings::default() };
        assert_eq!(write_settings(&path, &s).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(!path.exists());
    }
}
