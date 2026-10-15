//! Configuration of the Rules and Race Enhancements mod (RARE).
//!
//! RARE is a Lua app. The mod's installer copies its `RARE` folder to
//! `<AC root>/apps/lua/RARE`. The app keeps its settings in
//! `settings/settings.ini` inside that folder. When that file is missing, the
//! app copies `settings/default_settings.ini` over it. The format is plain INI,
//! read and written with [`crate::ini::Ini`].
//!
//! Defaults: [`RareSettings::default`] uses the values of `default_settings.ini`,
//! which is what a fresh install gets. Keys the app reads but the shipped file
//! does not contain (`VSC_*`, `DRIVER.TANK_FILL`, `AI_RELATIVE_SCALING`) use the
//! fallbacks in the app's Lua code. Keys the app never reads
//! (`MISC.PHYSICS_REBOOT`) are not modelled, and are kept as they are on write.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// Values at or above this magnitude are rejected by the app's own
/// config-setting sanity check.
const SANITY_LIMIT: f64 = 1e9;

#[derive(Debug, Clone, PartialEq)]
pub struct RareSettings {
    // [RULES]
    /// `DRS_RULES`: the app controls DRS.
    pub drs_rules: bool,
    /// `DRS_ACTIVATION_LAP`: first lap DRS may be used (UI range 1..=5; some presets write 0).
    pub drs_activation_lap: f64,
    /// `DRS_GAP_DELTA`: max gap to the car ahead in milliseconds (UI range 100..=2000; some presets write 0).
    pub drs_gap_delta_ms: f64,
    /// `DRS_WET_DISABLE`: disable DRS when the track is wet.
    pub drs_wet_disable: bool,
    /// `RESTRICT_COMPOUNDS`: restrict tyre choice to the per-car compound config.
    pub restrict_compounds: bool,
    /// `CORRECT_COMPOUNDS_COLORS`: recolour compounds to soft red, medium yellow, hard white.
    pub correct_compounds_colors: bool,
    /// `RACE_REFUELING`: allow refuelling during a race.
    pub race_refueling: bool,
    /// `VSC_RULES`: enable the virtual safety car.
    pub vsc_rules: bool,
    /// `VSC_INIT_TIME`: fallback value from the app's Lua code, not in the shipped defaults file.
    pub vsc_init_time: f64,
    /// `VSC_DEPLOY_TIME`: fallback value from the app's Lua code, not in the shipped defaults file.
    pub vsc_deploy_time: f64,

    // [DRIVER]
    /// `TANK_FILL`: fill the player's fuel tank for the whole race.
    pub driver_tank_fill: bool,

    // [AI]
    /// `AI_FORCE_PIT_TYRES`: AI pits for new tyres when worn.
    pub ai_force_pit_tyres: bool,
    /// `AI_AVG_TYRE_LIFE`: average tyre life (%) below which AI pits (UI range 0..=100).
    pub ai_avg_tyre_life: f64,
    /// `AI_AVG_TYRE_LIFE_RANGE`: per-AI variability of the average limit (UI range 0..=15).
    pub ai_avg_tyre_life_range: f64,
    /// `AI_SINGLE_TYRE_LIFE`: single tyre life (%) below which AI pits. Must not exceed `ai_avg_tyre_life`.
    pub ai_single_tyre_life: f64,
    /// `AI_SINGLE_TYRE_LIFE_RANGE`: per-AI variability of the single-tyre limit (UI range 0..=15).
    pub ai_single_tyre_life_range: f64,
    /// `AI_ALTERNATE_LEVEL`: use a more competitive default AI level.
    pub ai_alternate_level: bool,
    /// `AI_RELATIVE_SCALING`: enable relative AI scaling.
    pub ai_relative_scaling: bool,
    /// `AI_RELATIVE_LEVEL`: relative AI level in percent (UI range 70..=100).
    pub ai_relative_level: f64,
    /// `AI_MGUK_CONTROL`: AI makes MGU-K changes during the race.
    pub ai_mguk_control: bool,
    /// `AI_TANK_FILL`: fill AI fuel tanks for the whole race.
    pub ai_tank_fill: bool,

    // [AUDIO]
    /// `MASTER`: master volume of RARE sounds in percent (0..=100).
    pub audio_master: f64,
    /// `DRS_BEEP`: DRS beep volume in percent (0..=100).
    pub audio_drs_beep: f64,
    /// `DRS_FLAP`: DRS flap volume in percent (0..=100).
    pub audio_drs_flap: f64,

    // [NOTIFICATIONS]
    /// `X_POS`: race control banner X position on screen, in pixels.
    pub notifications_x_pos: f64,
    /// `Y_POS`: race control banner Y position on screen, in pixels.
    pub notifications_y_pos: f64,
    /// `SCALE`: race control banner scale (UI range 0.1..=5).
    pub notifications_scale: f64,
    /// `DURATION`: seconds the race control banner stays on screen (UI range 0..=10).
    pub notifications_duration: f64,
}

impl Default for RareSettings {
    fn default() -> Self {
        RareSettings {
            drs_rules: true,
            drs_activation_lap: 3.0,
            drs_gap_delta_ms: 1000.0,
            drs_wet_disable: true,
            restrict_compounds: true,
            correct_compounds_colors: true,
            race_refueling: false,
            vsc_rules: false,
            vsc_init_time: 300.0,
            vsc_deploy_time: 300.0,
            driver_tank_fill: true,
            ai_force_pit_tyres: true,
            ai_avg_tyre_life: 62.0,
            ai_avg_tyre_life_range: 5.0,
            ai_single_tyre_life: 45.0,
            ai_single_tyre_life_range: 5.0,
            ai_alternate_level: false,
            ai_relative_scaling: false,
            ai_relative_level: 100.0,
            ai_mguk_control: true,
            ai_tank_fill: true,
            audio_master: 100.0,
            audio_drs_beep: 50.0,
            audio_drs_flap: 50.0,
            notifications_x_pos: 0.0,
            notifications_y_pos: 0.0,
            notifications_scale: 1.0,
            notifications_duration: 5.0,
        }
    }
}

/// `<ac_root>/apps/lua/RARE`, the folder the mod's installer copies the mod into.
fn rare_dir(ac_root: &Path) -> PathBuf {
    ac_root.join("apps").join("lua").join("RARE")
}

/// Where the app reads and writes its settings:
/// `<ac_root>/apps/lua/RARE/settings/settings.ini`.
pub fn config_path(ac_root: &Path) -> PathBuf {
    rare_dir(ac_root).join("settings").join("settings.ini")
}

/// True when the mod's files are in place under `<ac_root>/apps/lua/RARE`
/// (`rare.lua` and `manifest.ini`, which Assetto Corsa needs to load the app).
pub fn is_installed(ac_root: &Path) -> bool {
    let dir = rare_dir(ac_root);
    dir.join("rare.lua").is_file() && dir.join("manifest.ini").is_file()
}

/// Reads the settings. A missing settings file returns [`RareSettings::default`].
/// A missing key in an existing file also takes its default. A value that is not
/// a number is an `InvalidData` error. Out-of-range values are read as they are;
/// use [`validate`] to check them.
pub fn read_settings(ac_root: &Path) -> io::Result<RareSettings> {
    let path = config_path(ac_root);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(RareSettings::default()),
        Err(e) => return Err(e),
    };
    from_ini(&Ini::parse(&text))
}

/// Writes the settings. Keys and sections the manager does not model are kept,
/// comments and key order too. Refuses values [`validate`] rejects. An existing
/// file is renamed to a `.acxs-backup-<stamp>` copy first, never deleted.
pub fn write_settings(ac_root: &Path, settings: &RareSettings) -> io::Result<()> {
    let problems = validate(settings);
    if !problems.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, problems.join("; ")));
    }
    let path = config_path(ac_root);
    let mut ini = match fs::read_to_string(&path) {
        Ok(text) => Ini::parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    apply(settings, &mut ini);
    ini.save_with_backup(&path)
}

/// Returns one message per value the app would reject or that breaks an
/// invariant the settings UI enforces. An empty list means the settings are valid.
/// Pushes a message when `value` is not finite, too large for the app, or outside `min..=max`.
fn check(problems: &mut Vec<String>, name: &str, value: f64, min: f64, max: f64) {
    if !value.is_finite() || value.abs() >= SANITY_LIMIT {
        problems.push(format!("{name} = {value}: not a finite number the app accepts"));
    } else if value < min || value > max {
        problems.push(format!("{name} = {value}: must be between {min} and {max}"));
    }
}

pub fn validate(s: &RareSettings) -> Vec<String> {
    let mut problems = Vec::new();

    check(&mut problems, "DRS_ACTIVATION_LAP", s.drs_activation_lap, 0.0, 5.0);
    if s.drs_activation_lap.is_finite() && s.drs_activation_lap.fract() != 0.0 {
        problems.push(format!("DRS_ACTIVATION_LAP = {}: must be a whole lap number", s.drs_activation_lap));
    }
    check(&mut problems, "DRS_GAP_DELTA", s.drs_gap_delta_ms, 0.0, 2000.0);
    check(&mut problems, "VSC_INIT_TIME", s.vsc_init_time, 0.0, SANITY_LIMIT);
    check(&mut problems, "VSC_DEPLOY_TIME", s.vsc_deploy_time, 0.0, SANITY_LIMIT);
    check(&mut problems, "AI_AVG_TYRE_LIFE", s.ai_avg_tyre_life, 0.0, 100.0);
    check(&mut problems, "AI_AVG_TYRE_LIFE_RANGE", s.ai_avg_tyre_life_range, 0.0, 15.0);
    check(&mut problems, "AI_SINGLE_TYRE_LIFE", s.ai_single_tyre_life, 0.0, 100.0);
    check(&mut problems, "AI_SINGLE_TYRE_LIFE_RANGE", s.ai_single_tyre_life_range, 0.0, 15.0);
    check(&mut problems, "AI_RELATIVE_LEVEL", s.ai_relative_level, 70.0, 100.0);
    if s.ai_single_tyre_life.is_finite() && s.ai_avg_tyre_life.is_finite() && s.ai_single_tyre_life > s.ai_avg_tyre_life {
        problems.push(format!(
            "AI_SINGLE_TYRE_LIFE = {} must not exceed AI_AVG_TYRE_LIFE = {}",
            s.ai_single_tyre_life, s.ai_avg_tyre_life
        ));
    }
    check(&mut problems, "MASTER", s.audio_master, 0.0, 100.0);
    check(&mut problems, "DRS_BEEP", s.audio_drs_beep, 0.0, 100.0);
    check(&mut problems, "DRS_FLAP", s.audio_drs_flap, 0.0, 100.0);
    check(&mut problems, "X_POS", s.notifications_x_pos, 0.0, SANITY_LIMIT);
    check(&mut problems, "Y_POS", s.notifications_y_pos, 0.0, SANITY_LIMIT);
    check(&mut problems, "SCALE", s.notifications_scale, 0.1, 5.0);
    check(&mut problems, "DURATION", s.notifications_duration, 0.0, 10.0);
    problems
}

fn from_ini(ini: &Ini) -> io::Result<RareSettings> {
    let d = RareSettings::default();
    Ok(RareSettings {
        drs_rules: flag(ini, "RULES", "DRS_RULES", d.drs_rules)?,
        drs_activation_lap: number(ini, "RULES", "DRS_ACTIVATION_LAP", d.drs_activation_lap)?,
        drs_gap_delta_ms: number(ini, "RULES", "DRS_GAP_DELTA", d.drs_gap_delta_ms)?,
        drs_wet_disable: flag(ini, "RULES", "DRS_WET_DISABLE", d.drs_wet_disable)?,
        restrict_compounds: flag(ini, "RULES", "RESTRICT_COMPOUNDS", d.restrict_compounds)?,
        correct_compounds_colors: flag(ini, "RULES", "CORRECT_COMPOUNDS_COLORS", d.correct_compounds_colors)?,
        race_refueling: flag(ini, "RULES", "RACE_REFUELING", d.race_refueling)?,
        vsc_rules: flag(ini, "RULES", "VSC_RULES", d.vsc_rules)?,
        vsc_init_time: number(ini, "RULES", "VSC_INIT_TIME", d.vsc_init_time)?,
        vsc_deploy_time: number(ini, "RULES", "VSC_DEPLOY_TIME", d.vsc_deploy_time)?,
        driver_tank_fill: flag(ini, "DRIVER", "TANK_FILL", d.driver_tank_fill)?,
        ai_force_pit_tyres: flag(ini, "AI", "AI_FORCE_PIT_TYRES", d.ai_force_pit_tyres)?,
        ai_avg_tyre_life: number(ini, "AI", "AI_AVG_TYRE_LIFE", d.ai_avg_tyre_life)?,
        ai_avg_tyre_life_range: number(ini, "AI", "AI_AVG_TYRE_LIFE_RANGE", d.ai_avg_tyre_life_range)?,
        ai_single_tyre_life: number(ini, "AI", "AI_SINGLE_TYRE_LIFE", d.ai_single_tyre_life)?,
        ai_single_tyre_life_range: number(ini, "AI", "AI_SINGLE_TYRE_LIFE_RANGE", d.ai_single_tyre_life_range)?,
        ai_alternate_level: flag(ini, "AI", "AI_ALTERNATE_LEVEL", d.ai_alternate_level)?,
        ai_relative_scaling: flag(ini, "AI", "AI_RELATIVE_SCALING", d.ai_relative_scaling)?,
        ai_relative_level: number(ini, "AI", "AI_RELATIVE_LEVEL", d.ai_relative_level)?,
        ai_mguk_control: flag(ini, "AI", "AI_MGUK_CONTROL", d.ai_mguk_control)?,
        ai_tank_fill: flag(ini, "AI", "AI_TANK_FILL", d.ai_tank_fill)?,
        audio_master: number(ini, "AUDIO", "MASTER", d.audio_master)?,
        audio_drs_beep: number(ini, "AUDIO", "DRS_BEEP", d.audio_drs_beep)?,
        audio_drs_flap: number(ini, "AUDIO", "DRS_FLAP", d.audio_drs_flap)?,
        notifications_x_pos: number(ini, "NOTIFICATIONS", "X_POS", d.notifications_x_pos)?,
        notifications_y_pos: number(ini, "NOTIFICATIONS", "Y_POS", d.notifications_y_pos)?,
        notifications_scale: number(ini, "NOTIFICATIONS", "SCALE", d.notifications_scale)?,
        notifications_duration: number(ini, "NOTIFICATIONS", "DURATION", d.notifications_duration)?,
    })
}

fn apply(s: &RareSettings, ini: &mut Ini) {
    set_flag(ini, "RULES", "DRS_RULES", s.drs_rules);
    set_number(ini, "RULES", "DRS_ACTIVATION_LAP", s.drs_activation_lap);
    set_number(ini, "RULES", "DRS_GAP_DELTA", s.drs_gap_delta_ms);
    set_flag(ini, "RULES", "DRS_WET_DISABLE", s.drs_wet_disable);
    set_flag(ini, "RULES", "RESTRICT_COMPOUNDS", s.restrict_compounds);
    set_flag(ini, "RULES", "CORRECT_COMPOUNDS_COLORS", s.correct_compounds_colors);
    set_flag(ini, "RULES", "RACE_REFUELING", s.race_refueling);
    set_flag(ini, "RULES", "VSC_RULES", s.vsc_rules);
    set_number(ini, "RULES", "VSC_INIT_TIME", s.vsc_init_time);
    set_number(ini, "RULES", "VSC_DEPLOY_TIME", s.vsc_deploy_time);
    set_flag(ini, "DRIVER", "TANK_FILL", s.driver_tank_fill);
    set_flag(ini, "AI", "AI_FORCE_PIT_TYRES", s.ai_force_pit_tyres);
    set_number(ini, "AI", "AI_AVG_TYRE_LIFE", s.ai_avg_tyre_life);
    set_number(ini, "AI", "AI_AVG_TYRE_LIFE_RANGE", s.ai_avg_tyre_life_range);
    set_number(ini, "AI", "AI_SINGLE_TYRE_LIFE", s.ai_single_tyre_life);
    set_number(ini, "AI", "AI_SINGLE_TYRE_LIFE_RANGE", s.ai_single_tyre_life_range);
    set_flag(ini, "AI", "AI_ALTERNATE_LEVEL", s.ai_alternate_level);
    set_flag(ini, "AI", "AI_RELATIVE_SCALING", s.ai_relative_scaling);
    set_number(ini, "AI", "AI_RELATIVE_LEVEL", s.ai_relative_level);
    set_flag(ini, "AI", "AI_MGUK_CONTROL", s.ai_mguk_control);
    set_flag(ini, "AI", "AI_TANK_FILL", s.ai_tank_fill);
    set_number(ini, "AUDIO", "MASTER", s.audio_master);
    set_number(ini, "AUDIO", "DRS_BEEP", s.audio_drs_beep);
    set_number(ini, "AUDIO", "DRS_FLAP", s.audio_drs_flap);
    set_number(ini, "NOTIFICATIONS", "X_POS", s.notifications_x_pos);
    set_number(ini, "NOTIFICATIONS", "Y_POS", s.notifications_y_pos);
    set_number(ini, "NOTIFICATIONS", "SCALE", s.notifications_scale);
    set_number(ini, "NOTIFICATIONS", "DURATION", s.notifications_duration);
}

/// Raw value of a key, without an inline `;` comment. `None` when the key is
/// missing or empty, so the default applies.
fn raw_value<'a>(ini: &'a Ini, section: &str, key: &str) -> Option<&'a str> {
    let value = ini.get(section, key)?.split(';').next()?.trim();
    (!value.is_empty()).then_some(value)
}

fn number(ini: &Ini, section: &str, key: &str, default: f64) -> io::Result<f64> {
    match raw_value(ini, section, key) {
        None => Ok(default),
        Some(raw) => raw.parse::<f64>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("[{section}] {key}: '{raw}' is not a number"),
            )
        }),
    }
}

/// The app treats a flag as on only when its value is exactly 1.
fn flag(ini: &Ini, section: &str, key: &str, default: bool) -> io::Result<bool> {
    Ok(number(ini, section, key, if default { 1.0 } else { 0.0 })? == 1.0)
}

fn set_number(ini: &mut Ini, section: &str, key: &str, value: f64) {
    let text = if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    };
    ini.set(section, key, text);
}

fn set_flag(ini: &mut Ini, section: &str, key: &str, value: bool) {
    ini.set(section, key, if value { "1" } else { "0" });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique scratch AC root, removed on drop.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> TempRoot {
            let dir = std::env::temp_dir().join(format!("acxs-rare-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempRoot(dir)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_raw(root: &Path, text: &str) {
        let path = config_path(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn path_matches_install_location() {
        let root = Path::new("/ac");
        assert_eq!(config_path(root), Path::new("/ac/apps/lua/RARE/settings/settings.ini"));
    }

    #[test]
    fn missing_file_gives_defaults() {
        let root = TempRoot::new("missing");
        assert_eq!(read_settings(&root.0).unwrap(), RareSettings::default());
        assert!(validate(&RareSettings::default()).is_empty());
    }

    #[test]
    fn missing_keys_take_defaults_and_inline_comments_are_ignored() {
        let root = TempRoot::new("partial");
        write_raw(&root.0, "[AUDIO]\r\nMASTER=40 ; quieter\r\n");
        let s = read_settings(&root.0).unwrap();
        assert_eq!(s.audio_master, 40.0);
        assert_eq!(s.audio_drs_beep, 50.0);
        assert_eq!(s, RareSettings { audio_master: 40.0, ..RareSettings::default() });
    }

    #[test]
    fn non_numeric_value_is_an_error() {
        let root = TempRoot::new("bad-number");
        write_raw(&root.0, "[RULES]\r\nDRS_GAP_DELTA=soon\r\n");
        let err = read_settings(&root.0).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("DRS_GAP_DELTA"));
    }

    #[test]
    fn round_trip_on_temp_root() {
        let root = TempRoot::new("round-trip");
        let changed = RareSettings {
            drs_rules: false,
            drs_activation_lap: 5.0,
            drs_gap_delta_ms: 1500.0,
            race_refueling: true,
            vsc_rules: true,
            ai_relative_scaling: true,
            ai_relative_level: 85.0,
            ai_avg_tyre_life: 50.5,
            ai_single_tyre_life: 40.0,
            audio_master: 80.0,
            notifications_scale: 1.5,
            notifications_x_pos: 640.0,
            ..RareSettings::default()
        };
        write_settings(&root.0, &changed).unwrap();
        assert_eq!(read_settings(&root.0).unwrap(), changed);
        write_settings(&root.0, &changed).unwrap();
        assert_eq!(read_settings(&root.0).unwrap(), changed);
    }

    #[test]
    fn unknown_keys_and_comments_are_preserved_and_backed_up() {
        let root = TempRoot::new("unknown");
        let original = "; my note\r\n[RULES]\r\nDRS_RULES=0\r\nMY_CUSTOM=hello\r\n[CUSTOM_SECTION]\r\nFOO=bar\r\n";
        write_raw(&root.0, original);

        let mut s = read_settings(&root.0).unwrap();
        assert!(!s.drs_rules);
        s.drs_rules = true;
        write_settings(&root.0, &s).unwrap();

        let ini = Ini::load(&config_path(&root.0)).unwrap();
        assert_eq!(ini.get("RULES", "DRS_RULES"), Some("1"));
        assert_eq!(ini.get("RULES", "MY_CUSTOM"), Some("hello"));
        assert_eq!(ini.get("CUSTOM_SECTION", "FOO"), Some("bar"));
        let text = fs::read_to_string(config_path(&root.0)).unwrap();
        assert!(text.starts_with("; my note\r\n"));

        let dir = config_path(&root.0).parent().unwrap().to_path_buf();
        let backups: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("settings.ini.acxs-backup-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(&backups[0]).unwrap(), original);
    }

    #[test]
    fn validate_catches_bad_values_and_write_refuses_them() {
        let root = TempRoot::new("validate");
        let bad = RareSettings {
            audio_master: 150.0,
            drs_activation_lap: 2.5,
            ai_single_tyre_life: 70.0,
            ai_avg_tyre_life: 62.0,
            notifications_duration: -1.0,
            ai_relative_level: f64::NAN,
            ..RareSettings::default()
        };
        let problems = validate(&bad);
        assert!(problems.iter().any(|p| p.starts_with("MASTER")), "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("DRS_ACTIVATION_LAP") && p.contains("whole")));
        assert!(problems.iter().any(|p| p.contains("must not exceed AI_AVG_TYRE_LIFE")));
        assert!(problems.iter().any(|p| p.starts_with("DURATION")));
        assert!(problems.iter().any(|p| p.starts_with("AI_RELATIVE_LEVEL") && p.contains("finite")));

        let err = write_settings(&root.0, &bad).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!config_path(&root.0).exists());
    }

    #[test]
    fn is_installed_checks_mod_files() {
        let root = TempRoot::new("installed");
        assert!(!is_installed(&root.0));
        let dir = root.0.join("apps").join("lua").join("RARE");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("rare.lua"), "").unwrap();
        assert!(!is_installed(&root.0));
        fs::write(dir.join("manifest.ini"), "").unwrap();
        assert!(is_installed(&root.0));
    }
}
