//! Gamepad setup for Advanced Gamepad Assist (AGA), a CSP gamepad script for
//! Assetto Corsa, and the game's own controller settings.
//!
//! AGA keeps its tuning (steering rate, target slip, self-steer, shifting and so
//! on) in CSP's script storage (`ac.storage`, keys prefixed `AGA_`) and changes it
//! only through its own in-game app, "Advanced Gamepad Assist Config". It does not
//! read those values from any INI file, so they are not edited here.
//!
//! What can be set from outside, and what AGA's own app writes itself:
//!
//! - CSP's gamepad script selection in `gamepad_fx.ini`, section `[JOYPAD_ASSIST]`:
//!   `ENABLED=1` and `IMPLEMENTATION=Advanced Gamepad Assist`. The defaults live in
//!   `<AC root>/extension/config/gamepad_fx.ini`; the user's choice is written to
//!   `Documents/Assetto Corsa/cfg/extension/gamepad_fx.ini`, which CSP reads over them.
//! - AC's controller settings in `Documents/Assetto Corsa/cfg/controls.ini`:
//!   `[HEADER] INPUT_METHOD=X360` (AGA needs the gamepad input method) and the
//!   `[X360]` keys `STEER_GAMMA`, `STEER_DEADZONE` and `RUMBLE_INTENSITY`, which
//!   AGA's app also shows as its Gamma, Deadzone and Rumble sliders.
//!
//! Writes keep every other key, section and comment, and back up the previous
//! file (renamed, never deleted).

use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

const CONFIG_FILE: &[&str] = &["extension", "config", "gamepad_fx.ini"];
/// Section of `gamepad_fx.ini` that selects the gamepad script.
const SECTION: &str = "JOYPAD_ASSIST";
/// `IMPLEMENTATION` value that selects AGA.
pub const AGA_IMPLEMENTATION: &str = "Advanced Gamepad Assist";

/// Allowed `[X360] STEER_GAMMA`.
pub const GAMMA_RANGE: RangeInclusive<f64> = 0.5..=4.0;
/// Allowed `[X360] STEER_DEADZONE`, as a fraction of the stick travel.
pub const DEADZONE_RANGE: RangeInclusive<f64> = 0.0..=0.5;
/// Allowed `[X360] RUMBLE_INTENSITY`, 0 (off) to 1.
pub const RUMBLE_RANGE: RangeInclusive<f64> = 0.0..=1.0;

/// The gamepad settings that live in files.
#[derive(Debug, Clone, PartialEq)]
pub struct PadSetup {
    /// `[JOYPAD_ASSIST] ENABLED` of `gamepad_fx.ini`.
    pub script_enabled: bool,
    /// `[JOYPAD_ASSIST] IMPLEMENTATION`, the selected gamepad script.
    pub implementation: String,
    /// `[HEADER] INPUT_METHOD` of `controls.ini` is `X360`.
    pub gamepad_input: bool,
    /// `[X360] STEER_GAMMA`.
    pub steer_gamma: f64,
    /// `[X360] STEER_DEADZONE`.
    pub steer_deadzone: f64,
    /// `[X360] RUMBLE_INTENSITY`.
    pub rumble: f64,
}

impl Default for PadSetup {
    /// What a fresh install has: no script selected, the game's gamepad defaults.
    fn default() -> Self {
        PadSetup {
            script_enabled: false,
            implementation: String::new(),
            gamepad_input: false,
            steer_gamma: 1.0,
            steer_deadzone: 0.0,
            rumble: 0.5,
        }
    }
}

impl PadSetup {
    /// True when CSP runs AGA as its gamepad script.
    pub fn aga_active(&self) -> bool {
        self.script_enabled && self.implementation.trim().eq_ignore_ascii_case(AGA_IMPLEMENTATION)
    }

    /// Selects AGA and the gamepad input method, as AGA's own app does.
    pub fn select_aga(&mut self) {
        self.script_enabled = true;
        self.implementation = AGA_IMPLEMENTATION.to_string();
        self.gamepad_input = true;
    }
}

/// Path of the default `gamepad_fx.ini` under an Assetto Corsa root folder.
pub fn config_path(ac_root: &Path) -> PathBuf {
    CONFIG_FILE.iter().fold(ac_root.to_path_buf(), |path, part| path.join(part))
}

/// `Documents/Assetto Corsa/cfg/controls.ini`, when the Documents folder is known.
pub fn controls_path() -> Option<PathBuf> {
    crate::settings::ac_user_cfg_dir().map(|cfg| cfg.join("controls.ini"))
}

/// Reads the setup. `fx_default` is the patch's `gamepad_fx.ini`, `fx_user` the
/// user's override (read over the defaults), `controls` the game's `controls.ini`.
/// Missing files and keys give the defaults; unparsable numbers are an error.
pub fn read_setup(fx_default: &Path, fx_user: Option<&Path>, controls: Option<&Path>) -> io::Result<PadSetup> {
    let d = PadSetup::default();
    let fx = match crate::csp_config::load_effective(fx_default, fx_user) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    let controls = match controls.map(Ini::load) {
        Some(Ok(ini)) => ini,
        Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => Ini::default(),
    };
    Ok(PadSetup {
        script_enabled: number(&fx, SECTION, "ENABLED")?.map_or(d.script_enabled, |v| v != 0.0),
        implementation: fx.get(SECTION, "IMPLEMENTATION").map(strip_comment).unwrap_or_default().to_string(),
        gamepad_input: controls
            .get("HEADER", "INPUT_METHOD")
            .is_some_and(|v| strip_comment(v).eq_ignore_ascii_case("X360")),
        steer_gamma: number(&controls, "X360", "STEER_GAMMA")?.unwrap_or(d.steer_gamma),
        steer_deadzone: number(&controls, "X360", "STEER_DEADZONE")?.unwrap_or(d.steer_deadzone),
        rumble: number(&controls, "X360", "RUMBLE_INTENSITY")?.unwrap_or(d.rumble),
    })
}

/// Writes the script selection to `fx_user` (the user's `gamepad_fx.ini`) and the
/// controller values to `controls`. A file whose values do not change is not saved.
/// Values that fail [`validate`] are refused before anything is written.
pub fn write_setup(fx_user: &Path, controls: &Path, setup: &PadSetup) -> io::Result<()> {
    let problems = validate(setup);
    if !problems.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, problems.join("; ")));
    }
    if setup.implementation.contains(['\r', '\n', ';']) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the script name must be one line without ';'"));
    }

    let mut fx = load_or_empty(fx_user)?;
    let before = fx.clone();
    fx.set(SECTION, "ENABLED", u8::from(setup.script_enabled));
    if !setup.implementation.trim().is_empty() {
        fx.set(SECTION, "IMPLEMENTATION", setup.implementation.trim());
    }
    if fx != before {
        fx.save_with_backup(fx_user)?;
    }

    let mut ini = load_or_empty(controls)?;
    let before = ini.clone();
    if setup.gamepad_input {
        ini.set("HEADER", "INPUT_METHOD", "X360");
    }
    ini.set("X360", "STEER_GAMMA", setup.steer_gamma);
    ini.set("X360", "STEER_DEADZONE", setup.steer_deadzone);
    ini.set("X360", "RUMBLE_INTENSITY", setup.rumble);
    if ini != before {
        ini.save_with_backup(controls)?;
    }
    Ok(())
}

/// Problems with values outside their ranges. Empty when everything is fine.
pub fn validate(s: &PadSetup) -> Vec<String> {
    let mut out = Vec::new();
    for (name, value, range) in [
        ("STEER_GAMMA", s.steer_gamma, GAMMA_RANGE),
        ("STEER_DEADZONE", s.steer_deadzone, DEADZONE_RANGE),
        ("RUMBLE_INTENSITY", s.rumble, RUMBLE_RANGE),
    ] {
        if !value.is_finite() || !range.contains(&value) {
            out.push(format!("{name} is {value}, allowed {} to {}", range.start(), range.end()));
        }
    }
    out
}

fn load_or_empty(path: &Path) -> io::Result<Ini> {
    match Ini::load(path) {
        Ok(ini) => Ok(ini),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Ini::default()),
        Err(e) => Err(e),
    }
}

fn strip_comment(raw: &str) -> &str {
    raw.split(';').next().unwrap_or("").trim()
}

/// A number, `None` when the key is missing, an error when it is not a number.
fn number(ini: &Ini, section: &str, key: &str) -> io::Result<Option<f64>> {
    let Some(raw) = ini.get(section, key) else {
        return Ok(None);
    };
    let text = strip_comment(raw);
    text.parse::<f64>().map(Some).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, format!("[{section}] {key} is not a number: {text:?}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("acxs-gamepad-{}-{name}", std::process::id()));
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
    fn config_path_is_under_extension_config() {
        let root = Path::new("ac");
        assert_eq!(config_path(root), root.join("extension").join("config").join("gamepad_fx.ini"));
    }

    #[test]
    fn missing_files_give_defaults() {
        let dir = TempDir::new("missing");
        let setup = read_setup(&dir.0.join("a.ini"), Some(&dir.0.join("b.ini")), Some(&dir.0.join("c.ini"))).unwrap();
        assert_eq!(setup, PadSetup::default());
        assert!(!setup.aga_active());
    }

    #[test]
    fn user_override_and_controls_are_read() {
        let dir = TempDir::new("read");
        let fx_default = dir.0.join("default.ini");
        let fx_user = dir.0.join("user.ini");
        let controls = dir.0.join("controls.ini");
        fs::write(&fx_default, "[JOYPAD_ASSIST]\nENABLED=0 ; off\nIMPLEMENTATION=Default\n").unwrap();
        fs::write(&fx_user, "[JOYPAD_ASSIST]\nENABLED=1\nIMPLEMENTATION=Advanced Gamepad Assist\n").unwrap();
        fs::write(&controls, "[HEADER]\nINPUT_METHOD=X360\n[X360]\nSTEER_GAMMA=1.4\nSTEER_DEADZONE=0.12\n").unwrap();
        let setup = read_setup(&fx_default, Some(&fx_user), Some(&controls)).unwrap();
        assert!(setup.aga_active());
        assert!(setup.gamepad_input);
        assert_eq!(setup.steer_gamma, 1.4);
        assert_eq!(setup.steer_deadzone, 0.12);
        assert_eq!(setup.rumble, PadSetup::default().rumble);
    }

    #[test]
    fn select_aga_writes_both_files_and_keeps_other_keys() {
        let dir = TempDir::new("write");
        let fx_user = dir.0.join("user.ini");
        let controls = dir.0.join("controls.ini");
        fs::write(&controls, "[HEADER]\nINPUT_METHOD=WHEEL\n[KEYBOARD]\nGAS=0x57\n").unwrap();
        let mut setup = PadSetup::default();
        setup.select_aga();
        setup.steer_gamma = 1.4;
        write_setup(&fx_user, &controls, &setup).unwrap();

        let fx = Ini::load(&fx_user).unwrap();
        assert_eq!(fx.get("JOYPAD_ASSIST", "ENABLED"), Some("1"));
        assert_eq!(fx.get("JOYPAD_ASSIST", "IMPLEMENTATION"), Some(AGA_IMPLEMENTATION));
        let ini = Ini::load(&controls).unwrap();
        assert_eq!(ini.get("HEADER", "INPUT_METHOD"), Some("X360"));
        assert_eq!(ini.get("KEYBOARD", "GAS"), Some("0x57"));
        assert_eq!(ini.get("X360", "STEER_GAMMA"), Some("1.4"));
        assert_eq!(read_setup(&dir.0.join("none.ini"), Some(&fx_user), Some(&controls)).unwrap(), setup);
    }

    #[test]
    fn out_of_range_values_are_refused_before_writing() {
        let dir = TempDir::new("range");
        let setup = PadSetup { steer_deadzone: 0.9, ..PadSetup::default() };
        assert_eq!(validate(&setup).len(), 1);
        let err = write_setup(&dir.0.join("u.ini"), &dir.0.join("c.ini"), &setup).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!dir.0.join("u.ini").exists());
    }

    #[test]
    fn unparsable_number_is_an_error() {
        let dir = TempDir::new("bad");
        let controls = dir.0.join("controls.ini");
        fs::write(&controls, "[X360]\nSTEER_GAMMA=abc\n").unwrap();
        let err = read_setup(&dir.0.join("none.ini"), None, Some(&controls)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
