//! Full Assetto Corsa `controls.ini` mapper and controller preset store.
//!
//! Reads and writes wheel, pedal, handbrake, H-shifter, paddle, action, and CSP extra bindings
//! in `Documents/Assetto Corsa/cfg/controls.ini` while preserving all unknown keys/comments
//! and creating rolling `.acxs-backup-*` backups.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

pub const CONTROLS_FILE: &str = "controls.ini";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum InputMethod {
    #[default]
    Wheel,
    X360,
    Keyboard,
}

impl InputMethod {
    pub fn as_ini_str(self) -> &'static str {
        match self {
            InputMethod::Wheel => "WHEEL",
            InputMethod::X360 => "X360",
            InputMethod::Keyboard => "KEYBOARD",
        }
    }

    pub fn from_ini_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "X360" => InputMethod::X360,
            "KEYBOARD" => InputMethod::Keyboard,
            _ => InputMethod::Wheel,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AxisBinding {
    pub joy: i32,
    pub axle: i32,
    pub min: f64,
    pub max: f64,
    pub gamma: f64,
    pub range_from: f64,
    pub range_to: f64,
}

impl Default for AxisBinding {
    fn default() -> Self {
        Self {
            joy: -1,
            axle: -1,
            min: -1.0,
            max: 1.0,
            gamma: 1.0,
            range_from: 0.0,
            range_to: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SteerConfig {
    pub joy: i32,
    pub axle: i32,
    pub scale: i32,
    pub lock_deg: u32,
    pub ff_gain: f64,
    pub filter_ff: f64,
    pub steer_gamma: f64,
    pub steer_filter: f64,
    pub speed_sensitivity: f64,
    pub debouncing_ms: u32,
}

impl Default for SteerConfig {
    fn default() -> Self {
        Self {
            joy: 0,
            axle: 0,
            scale: 1,
            lock_deg: 900,
            ff_gain: 1.0,
            filter_ff: 0.0,
            steer_gamma: 1.0,
            steer_filter: 0.0,
            speed_sensitivity: 0.0,
            debouncing_ms: 50,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ButtonBinding {
    pub joy: i32,
    pub button: i32,
    /// Optional Win32 virtual key hex string (e.g. `"0x48"`).
    pub key: String,
}

impl Default for ButtonBinding {
    fn default() -> Self {
        Self {
            joy: -1,
            button: -1,
            key: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShifterConfig {
    pub active: bool,
    pub joy: i32,
    pub gears: [i32; 7],
    pub gear_r: i32,
}

impl Default for ShifterConfig {
    fn default() -> Self {
        Self {
            active: false,
            joy: -1,
            gears: [-1; 7],
            gear_r: -1,
        }
    }
}

/// Standard AC and CSP action section names mapped in `controls.ini`.
pub const ACTION_SECTIONS: &[&str] = &[
    "GEARUP",
    "GEARDN",
    "ACTION_HEADLIGHTS",
    "ACTION_HORN",
    "GLANCELEFT",
    "GLANCERIGHT",
    "GLANCEBACK",
    "VR_RECENTER",
    "__EXT_WIPER_UP",
    "__EXT_WIPER_DOWN",
    "__EXT_TURNSIGNAL_LEFT",
    "__EXT_TURNSIGNAL_RIGHT",
    "__EXT_HAZARDS",
    "__EXT_EXTRA_A",
    "__EXT_EXTRA_B",
    "__EXT_EXTRA_C",
    "__EXT_EXTRA_D",
    "__EXT_EXTRA_E",
    "__EXT_EXTRA_F",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlsConfig {
    pub input_method: InputMethod,
    /// Registered devices: `(name, product_guid)` written as `CON0`/`PGUID0`..`CONn`/`PGUIDn`.
    pub controllers: Vec<(String, String)>,
    pub steer: SteerConfig,
    pub throttle: AxisBinding,
    pub brakes: AxisBinding,
    pub clutch: AxisBinding,
    pub handbrake: AxisBinding,
    pub shifter: ShifterConfig,
    pub actions: BTreeMap<String, ButtonBinding>,
}

impl Default for ControlsConfig {
    fn default() -> Self {
        Self {
            input_method: InputMethod::Wheel,
            controllers: Vec::new(),
            steer: SteerConfig::default(),
            throttle: AxisBinding::default(),
            brakes: AxisBinding::default(),
            clutch: AxisBinding::default(),
            handbrake: AxisBinding::default(),
            shifter: ShifterConfig::default(),
            actions: BTreeMap::new(),
        }
    }
}

fn read_axis(ini: &Ini, section: &str) -> AxisBinding {
    let d = AxisBinding::default();
    AxisBinding {
        joy: ini.get_i64(section, "JOY").unwrap_or(d.joy as i64) as i32,
        axle: ini.get_i64(section, "AXLE").unwrap_or(d.axle as i64) as i32,
        min: ini.get_f64(section, "MIN").unwrap_or(d.min),
        max: ini.get_f64(section, "MAX").unwrap_or(d.max),
        gamma: ini.get_f64(section, "GAMMA").unwrap_or(d.gamma),
        range_from: ini.get_f64(section, "RANGE_FROM").unwrap_or(d.range_from),
        range_to: ini.get_f64(section, "RANGE_TO").unwrap_or(d.range_to),
    }
}

fn write_axis(ini: &mut Ini, section: &str, a: &AxisBinding) {
    ini.set(section, "JOY", a.joy);
    ini.set(section, "AXLE", a.axle);
    ini.set(section, "MIN", a.min);
    ini.set(section, "MAX", a.max);
    ini.set(section, "GAMMA", a.gamma);
    ini.set(section, "RANGE_FROM", a.range_from);
    ini.set(section, "RANGE_TO", a.range_to);
}

pub fn read_controls(cfg_dir: &Path) -> io::Result<ControlsConfig> {
    let path = cfg_dir.join(CONTROLS_FILE);
    let ini = match Ini::load(&path) {
        Ok(i) => i,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ControlsConfig::default()),
        Err(e) => return Err(e),
    };
    Ok(from_ini(&ini))
}

pub fn from_ini(ini: &Ini) -> ControlsConfig {
    let d = ControlsConfig::default();
    let input_method = ini
        .get("HEADER", "INPUT_METHOD")
        .map(InputMethod::from_ini_str)
        .unwrap_or(d.input_method);

    let mut controllers = Vec::new();
    for idx in 0..16 {
        let Some(name) = ini.get("CONTROLLERS", &format!("CON{idx}")) else {
            break;
        };
        let pguid = ini
            .get("CONTROLLERS", &format!("PGUID{idx}"))
            .unwrap_or_default()
            .trim()
            .to_string();
        controllers.push((name.trim().to_string(), pguid));
    }

    let ds = SteerConfig::default();
    let steer = SteerConfig {
        joy: ini.get_i64("STEER", "JOY").unwrap_or(ds.joy as i64) as i32,
        axle: ini.get_i64("STEER", "AXLE").unwrap_or(ds.axle as i64) as i32,
        scale: ini.get_i64("STEER", "SCALE").unwrap_or(ds.scale as i64) as i32,
        lock_deg: ini.get_i64("STEER", "LOCK").unwrap_or(ds.lock_deg as i64).clamp(180, 1800) as u32,
        ff_gain: ini.get_f64("STEER", "FF_GAIN").unwrap_or(ds.ff_gain),
        filter_ff: ini.get_f64("STEER", "FILTER_FF").unwrap_or(ds.filter_ff),
        steer_gamma: ini.get_f64("STEER", "STEER_GAMMA").unwrap_or(ds.steer_gamma),
        steer_filter: ini.get_f64("STEER", "STEER_FILTER").unwrap_or(ds.steer_filter),
        speed_sensitivity: ini.get_f64("STEER", "SPEED_SENSITIVITY").unwrap_or(ds.speed_sensitivity),
        debouncing_ms: ini.get_i64("STEER", "DEBOUNCING_MS").unwrap_or(ds.debouncing_ms as i64).clamp(0, 200) as u32,
    };

    let mut gears = [-1i32; 7];
    for (i, slot) in gears.iter_mut().enumerate() {
        *slot = ini.get_i64("SHIFTER", &format!("GEAR_{}", i + 1)).unwrap_or(-1) as i32;
    }
    let shifter = ShifterConfig {
        active: ini.get_i64("SHIFTER", "ACTIVE") == Some(1),
        joy: ini.get_i64("SHIFTER", "JOY").unwrap_or(-1) as i32,
        gears,
        gear_r: ini.get_i64("SHIFTER", "GEAR_R").unwrap_or(-1) as i32,
    };

    let mut actions = BTreeMap::new();
    for &sec in ACTION_SECTIONS {
        let joy = ini.get_i64(sec, "JOY");
        let btn = ini.get_i64(sec, "BUTTON");
        let key = ini.get(sec, "KEY").map(|s| s.trim().to_string());
        if joy.is_some() || btn.is_some() || key.is_some() {
            actions.insert(
                sec.to_string(),
                ButtonBinding {
                    joy: joy.unwrap_or(-1) as i32,
                    button: btn.unwrap_or(-1) as i32,
                    key: key.unwrap_or_default(),
                },
            );
        }
    }

    ControlsConfig {
        input_method,
        controllers,
        steer,
        throttle: read_axis(ini, "THROTTLE"),
        brakes: read_axis(ini, "BRAKES"),
        clutch: read_axis(ini, "CLUTCH"),
        handbrake: read_axis(ini, "HANDBRAKE"),
        shifter,
        actions,
    }
}

pub fn apply_to_ini(ini: &mut Ini, cfg: &ControlsConfig) {
    ini.set("HEADER", "INPUT_METHOD", cfg.input_method.as_ini_str());
    for (idx, (name, pguid)) in cfg.controllers.iter().enumerate() {
        ini.set("CONTROLLERS", &format!("CON{idx}"), name);
        if !pguid.is_empty() {
            ini.set("CONTROLLERS", &format!("PGUID{idx}"), pguid);
        }
    }
    ini.set("STEER", "JOY", cfg.steer.joy);
    ini.set("STEER", "AXLE", cfg.steer.axle);
    ini.set("STEER", "SCALE", if cfg.steer.scale < 0 { -1 } else { 1 });
    ini.set("STEER", "LOCK", cfg.steer.lock_deg.clamp(180, 1800));
    ini.set("STEER", "FF_GAIN", cfg.steer.ff_gain);
    ini.set("STEER", "FILTER_FF", cfg.steer.filter_ff);
    ini.set("STEER", "STEER_GAMMA", cfg.steer.steer_gamma);
    ini.set("STEER", "STEER_FILTER", cfg.steer.steer_filter);
    ini.set("STEER", "SPEED_SENSITIVITY", cfg.steer.speed_sensitivity);
    ini.set("STEER", "DEBOUNCING_MS", cfg.steer.debouncing_ms.clamp(0, 200));

    write_axis(ini, "THROTTLE", &cfg.throttle);
    write_axis(ini, "BRAKES", &cfg.brakes);
    write_axis(ini, "CLUTCH", &cfg.clutch);
    write_axis(ini, "HANDBRAKE", &cfg.handbrake);

    ini.set("SHIFTER", "ACTIVE", i32::from(cfg.shifter.active));
    ini.set("SHIFTER", "JOY", cfg.shifter.joy);
    for (i, &btn) in cfg.shifter.gears.iter().enumerate() {
        ini.set("SHIFTER", &format!("GEAR_{}", i + 1), btn);
    }
    ini.set("SHIFTER", "GEAR_R", cfg.shifter.gear_r);

    for (sec, bind) in &cfg.actions {
        ini.set(sec, "JOY", bind.joy);
        ini.set(sec, "BUTTON", bind.button);
        if !bind.key.is_empty() {
            ini.set(sec, "KEY", &bind.key);
        }
    }
}

pub fn write_controls(cfg_dir: &Path, cfg: &ControlsConfig) -> io::Result<()> {
    if crate::game::is_game_running() {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "Assetto Corsa is running; close it before saving controls.",
        ));
    }
    let path = cfg_dir.join(CONTROLS_FILE);
    let mut ini = match Ini::load(&path) {
        Ok(i) => i,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    apply_to_ini(&mut ini, cfg);
    ini.save_with_backup(&path)
}

fn presets_dir(cfg_dir: &Path) -> PathBuf {
    cfg_dir.join("controllers").join("savedsetups")
}

/// Lists saved controller presets under `cfg/controllers/savedsetups/*.ini`.
pub fn list_saved_presets(cfg_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(presets_dir(cfg_dir)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Saves a controller preset to `cfg/controllers/savedsetups/<name>.ini` with backup.
pub fn save_preset(cfg_dir: &Path, name: &str, cfg: &ControlsConfig) -> io::Result<PathBuf> {
    let clean = name.trim();
    if clean.is_empty() || clean.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid controller preset name"));
    }
    let dir = presets_dir(cfg_dir);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{clean}.ini"));
    let mut ini = Ini::default();
    apply_to_ini(&mut ini, cfg);
    ini.save_with_backup(&path)?;
    Ok(path)
}

/// Loads a saved controller preset from `cfg/controllers/savedsetups/<name>.ini`.
pub fn load_preset(cfg_dir: &Path, name: &str) -> io::Result<ControlsConfig> {
    let path = presets_dir(cfg_dir).join(format!("{}.ini", name.trim()));
    let ini = Ini::load(&path)?;
    Ok(from_ini(&ini))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_ini_preserves_unrelated_bindings_and_creates_backup() {
        let dir = std::env::temp_dir().join(format!("acxs-controls-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONTROLS_FILE);
        fs::write(
            &path,
            "[HEADER]\r\nINPUT_METHOD=WHEEL\r\n\r\n[CUSTOM_MOD]\r\nKEEP_ME=42\r\n",
        )
        .unwrap();

        let mut cfg = read_controls(&dir).unwrap();
        cfg.steer.lock_deg = 540;
        cfg.brakes.gamma = 1.8;
        cfg.shifter.active = true;
        cfg.shifter.gears[0] = 12;
        cfg.actions.insert(
            "__EXT_EXTRA_A".into(),
            ButtonBinding {
                joy: 0,
                button: 7,
                key: String::new(),
            },
        );
        write_controls(&dir, &cfg).unwrap();

        let raw = Ini::load(&path).unwrap();
        assert_eq!(raw.get("CUSTOM_MOD", "KEEP_ME"), Some("42"));
        let reloaded = read_controls(&dir).unwrap();
        assert_eq!(reloaded.steer.lock_deg, 540);
        assert_eq!(reloaded.brakes.gamma, 1.8);
        assert!(reloaded.shifter.active);
        assert_eq!(reloaded.shifter.gears[0], 12);
        assert_eq!(reloaded.actions["__EXT_EXTRA_A"].button, 7);

        let backups = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".acxs-backup-"))
            .count();
        assert_eq!(backups, 1);

        save_preset(&dir, "GT3 Wheel", &cfg).unwrap();
        assert_eq!(list_saved_presets(&dir), vec!["GT3 Wheel"]);
        assert_eq!(load_preset(&dir, "GT3 Wheel").unwrap().steer.lock_deg, 540);
        let _ = fs::remove_dir_all(&dir);
    }
}
