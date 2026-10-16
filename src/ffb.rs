//! Typed access to CSP's force-feedback tuning file, `extension/config/ffb_tweaks.ini`.
//!
//! Only keys that are active (not commented out) in the files of the patch versions are exposed:
//! lights-patch v0.2.11 and v0.3.0-preview675. Defaults are the v0.3.0-preview675
//! values. Two keys (`STRUT_FFB_ADJUSTMENT`, `FADE_IN_AFTER_LOADING`) are active in
//! v0.2.11 only; v0.3.0 lists them commented out as `...2` with the same default 1,
//! so their default here is 1 for both. String keys (`IMPLEMENTATION`) and UI hints
//! are not exposed; they are preserved untouched on write.
//!
//! Every `ac_root` argument is the Assetto Corsa root, the folder that contains `extension/`.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

#[derive(Debug, Clone, PartialEq)]
pub struct FfbTweaks {
    /// [BASIC] ENABLED
    pub basic_enabled: bool,
    /// [GYRO2] ENABLED
    pub gyro2_enabled: bool,
    /// [POSTPROCESSING] RANGE_COMPRESSION, 0.5 to 4.0
    pub range_compression: f64,
    /// [POSTPROCESSING] RANGE_COMPRESSION_ASSIST
    pub range_compression_assist: bool,
    /// [POSTPROCESSING] CLEAN_FFB
    pub clean_ffb: bool,
    /// [POSTPROCESSING] DIRT_VIBRATION, 0 to 1.5
    pub dirt_vibration: f64,
    /// [POSTPROCESSING_SCRIPT] ENABLED
    pub postprocessing_script_enabled: bool,
    /// [CUSTOM_SOFT_LOCK] ENABLED
    pub soft_lock_enabled: bool,
    /// [CUSTOM_SOFT_LOCK] PADDING, 4 to 20 degrees
    pub soft_lock_padding: f64,
    /// [CUSTOM_SOFT_LOCK] SHIFT_PADDING, 0 to 1
    pub soft_lock_shift_padding: f64,
    /// [CUSTOM_SOFT_LOCK] FORCE_FACTOR, 0 to 2
    pub soft_lock_force_factor: f64,
    /// [CUSTOM_SOFT_LOCK] SPEED_FACTOR, 0 to 2
    pub soft_lock_speed_factor: f64,
    /// [CUSTOM_SOFT_LOCK] DAMPER, 0 to 1
    pub soft_lock_damper: f64,
    /// [REAL_FEEL] ENABLED
    pub real_feel_enabled: bool,
    /// [REAL_FEEL] WHEEL_MAX_NM, 2.0 to 35.0
    pub real_feel_wheel_max_nm: f64,
    /// [REAL_FEEL] ENABLE_FOR_ALL
    pub real_feel_enable_for_all: bool,
    /// [REAL_FEEL_BRAKE] ENABLED
    pub real_feel_brake_enabled: bool,
    /// [REAL_FEEL_BRAKE] BRAKE_MAX_KG, 0.0 to 200.0
    pub brake_max_kg: f64,
    /// [REAL_FEEL_BRAKE] BRAKE_MAX_INACTIVE_PERC, 1.0 to 100.0
    pub brake_max_inactive_perc: f64,
    /// [REAL_FEEL_BRAKE] CLAMP_FORCES
    pub brake_clamp_forces: bool,
    /// [TWEAKS] ADAPTIVE_FFB, v0.3.0 only: 0 to 3
    pub adaptive_ffb: f64,
    /// [TWEAKS] AVERAGE_SKIPPED_FFB
    pub average_skipped_ffb: bool,
    /// [TWEAKS] SKIP_DAMPING_FRAMES, whole number 0 to 5 (v0.3.0 comment)
    pub skip_damping_frames: f64,
    /// [TWEAKS] STRUT_FFB_ADJUSTMENT, v0.2.11 key
    pub strut_ffb_adjustment: bool,
    /// [TWEAKS] FADE_IN_AFTER_LOADING, v0.2.11 key
    pub fade_in_after_loading: bool,
}

impl Default for FfbTweaks {
    /// The values used by lights-patch v0.3.0-preview675 (see module docs).
    fn default() -> Self {
        FfbTweaks {
            basic_enabled: true,
            gyro2_enabled: false,
            range_compression: 1.0,
            range_compression_assist: false,
            clean_ffb: true,
            dirt_vibration: 0.0,
            postprocessing_script_enabled: false,
            soft_lock_enabled: true,
            soft_lock_padding: 10.0,
            soft_lock_shift_padding: 0.8,
            soft_lock_force_factor: 1.0,
            soft_lock_speed_factor: 1.0,
            soft_lock_damper: 0.0,
            real_feel_enabled: false,
            real_feel_wheel_max_nm: 35.0,
            real_feel_enable_for_all: false,
            real_feel_brake_enabled: false,
            brake_max_kg: 0.0,
            brake_max_inactive_perc: 100.0,
            brake_clamp_forces: true,
            adaptive_ffb: 0.0,
            average_skipped_ffb: true,
            skip_damping_frames: 3.0,
            strut_ffb_adjustment: true,
            fade_in_after_loading: true,
        }
    }
}

/// One exposed key, pointing at its field. Used for both reading and writing so the
/// key list lives in one place.
enum Slot<'a> {
    Float { section: &'static str, key: &'static str, value: &'a mut f64 },
    Flag { section: &'static str, key: &'static str, value: &'a mut bool },
}

fn slots(t: &mut FfbTweaks) -> Vec<Slot<'_>> {
    use Slot::{Flag, Float};
    vec![
        Flag { section: "BASIC", key: "ENABLED", value: &mut t.basic_enabled },
        Flag { section: "GYRO2", key: "ENABLED", value: &mut t.gyro2_enabled },
        Float { section: "POSTPROCESSING", key: "RANGE_COMPRESSION", value: &mut t.range_compression },
        Flag { section: "POSTPROCESSING", key: "RANGE_COMPRESSION_ASSIST", value: &mut t.range_compression_assist },
        Flag { section: "POSTPROCESSING", key: "CLEAN_FFB", value: &mut t.clean_ffb },
        Float { section: "POSTPROCESSING", key: "DIRT_VIBRATION", value: &mut t.dirt_vibration },
        Flag { section: "POSTPROCESSING_SCRIPT", key: "ENABLED", value: &mut t.postprocessing_script_enabled },
        Flag { section: "CUSTOM_SOFT_LOCK", key: "ENABLED", value: &mut t.soft_lock_enabled },
        Float { section: "CUSTOM_SOFT_LOCK", key: "PADDING", value: &mut t.soft_lock_padding },
        Float { section: "CUSTOM_SOFT_LOCK", key: "SHIFT_PADDING", value: &mut t.soft_lock_shift_padding },
        Float { section: "CUSTOM_SOFT_LOCK", key: "FORCE_FACTOR", value: &mut t.soft_lock_force_factor },
        Float { section: "CUSTOM_SOFT_LOCK", key: "SPEED_FACTOR", value: &mut t.soft_lock_speed_factor },
        Float { section: "CUSTOM_SOFT_LOCK", key: "DAMPER", value: &mut t.soft_lock_damper },
        Flag { section: "REAL_FEEL", key: "ENABLED", value: &mut t.real_feel_enabled },
        Float { section: "REAL_FEEL", key: "WHEEL_MAX_NM", value: &mut t.real_feel_wheel_max_nm },
        Flag { section: "REAL_FEEL", key: "ENABLE_FOR_ALL", value: &mut t.real_feel_enable_for_all },
        Flag { section: "REAL_FEEL_BRAKE", key: "ENABLED", value: &mut t.real_feel_brake_enabled },
        Float { section: "REAL_FEEL_BRAKE", key: "BRAKE_MAX_KG", value: &mut t.brake_max_kg },
        Float { section: "REAL_FEEL_BRAKE", key: "BRAKE_MAX_INACTIVE_PERC", value: &mut t.brake_max_inactive_perc },
        Flag { section: "REAL_FEEL_BRAKE", key: "CLAMP_FORCES", value: &mut t.brake_clamp_forces },
        Float { section: "TWEAKS", key: "ADAPTIVE_FFB", value: &mut t.adaptive_ffb },
        Flag { section: "TWEAKS", key: "AVERAGE_SKIPPED_FFB", value: &mut t.average_skipped_ffb },
        Float { section: "TWEAKS", key: "SKIP_DAMPING_FRAMES", value: &mut t.skip_damping_frames },
        Flag { section: "TWEAKS", key: "STRUT_FFB_ADJUSTMENT", value: &mut t.strut_ffb_adjustment },
        Flag { section: "TWEAKS", key: "FADE_IN_AFTER_LOADING", value: &mut t.fade_in_after_loading },
    ]
}

/// Path of the tuning file under an Assetto Corsa root.
pub fn config_path(ac_root: &Path) -> PathBuf {
    ac_root.join("extension").join("config").join("ffb_tweaks.ini")
}

/// Read the tuning file. A missing file yields `FfbTweaks::default()`. Keys that are
/// absent or unparsable also fall back to their default.
pub fn read(ac_root: &Path) -> io::Result<FfbTweaks> {
    match Ini::load(&config_path(ac_root)) {
        Ok(ini) => Ok(from_ini(&ini)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(FfbTweaks::default()),
        Err(e) => Err(e),
    }
}

/// Reads the values CSP uses: the tuning file under `ac_root` overlaid with the
/// user's override file (`Documents/Assetto Corsa/cfg/extension/ffb_tweaks.ini`)
/// when `user` is given.
pub fn read_with_user(ac_root: &Path, user: Option<&Path>) -> io::Result<FfbTweaks> {
    match crate::csp_config::load_effective(&config_path(ac_root), user) {
        Ok(ini) => Ok(from_ini(&ini)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(FfbTweaks::default()),
        Err(e) => Err(e),
    }
}

/// Writes the values that differ from what CSP currently uses into the user's
/// override file `user`, so the defaults under `ac_root` stay as shipped. With
/// `user` set to `None` this is [`write`].
pub fn write_with_user(ac_root: &Path, user: Option<&Path>, tweaks: &FfbTweaks) -> io::Result<()> {
    let Some(user) = user else {
        return write(ac_root, tweaks);
    };
    let mut current = read_with_user(ac_root, Some(user))?;
    let mut want = tweaks.clone();
    let mut ini = match Ini::load(user) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ini::default(),
        Err(e) => return Err(e),
    };
    let mut changed = false;
    for (w, c) in slots(&mut want).into_iter().zip(slots(&mut current)) {
        match (w, c) {
            (Slot::Float { section, key, value: w }, Slot::Float { value: c, .. }) => {
                if !w.is_finite() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{section}.{key} is not a finite number"),
                    ));
                }
                if *w != *c {
                    ini.set(section, key, (*w).to_string());
                    changed = true;
                }
            }
            (Slot::Flag { section, key, value: w }, Slot::Flag { value: c, .. }) => {
                if *w != *c {
                    ini.set(section, key, if *w { "1" } else { "0" });
                    changed = true;
                }
            }
            _ => unreachable!("slots() yields the same order for every instance"),
        }
    }
    if !changed {
        return Ok(());
    }
    ini.save_with_backup(user)
}

/// Write the tuning file under `ac_root`, and nothing else on disk.
///
/// Only keys whose value differs from what is in the file are touched, so a file that
/// already holds these values is not rewritten. Unknown keys, sections and comments
/// are kept. Inline comments on edited keys are kept too. If the file exists and
/// changes, it is first renamed to a `.acxs-backup-<stamp>` sibling. Nothing is deleted.
/// A missing file is created with every exposed key. Non-finite numbers are rejected.
pub fn write(ac_root: &Path, tweaks: &FfbTweaks) -> io::Result<()> {
    let path = config_path(ac_root);
    let existed = path.exists();
    let mut ini = if existed { Ini::load(&path)? } else { Ini::default() };
    let mut want = tweaks.clone();
    let mut defaults = FfbTweaks::default();
    let mut changed = false;

    for (w, d) in slots(&mut want).into_iter().zip(slots(&mut defaults)) {
        match (w, d) {
            (Slot::Float { section, key, value: w }, Slot::Float { value: d, .. }) => {
                if !w.is_finite() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{section}.{key} is not a finite number"),
                    ));
                }
                let raw = ini.get(section, key).map(str::to_owned);
                let needs = match raw.as_deref() {
                    Some(r) => parse_float(r) != Some(*w),
                    None => !existed || *w != *d,
                };
                if needs {
                    ini.set(section, key, keep_comment(raw.as_deref(), &(*w).to_string()));
                    changed = true;
                }
            }
            (Slot::Flag { section, key, value: w }, Slot::Flag { value: d, .. }) => {
                let raw = ini.get(section, key).map(str::to_owned);
                let needs = match raw.as_deref() {
                    Some(r) => parse_flag(r) != Some(*w),
                    None => !existed || *w != *d,
                };
                if needs {
                    let text = if *w { "1" } else { "0" };
                    ini.set(section, key, keep_comment(raw.as_deref(), text));
                    changed = true;
                }
            }
            _ => unreachable!("slots() yields the same order for every instance"),
        }
    }

    if existed && !changed {
        return Ok(());
    }
    ini.save_with_backup(&path)
}

/// Problems with values outside the ranges given in the file's own comments.
/// Ranges: `RANGE_COMPRESSION` 0.5 to 4.0, `DIRT_VIBRATION` 0 to 1.5, `PADDING` 4 to 20,
/// `SHIFT_PADDING`, `DAMPER` 0 to 1, `FORCE_FACTOR` and `SPEED_FACTOR` 0 to 2,
/// `WHEEL_MAX_NM` 2.0 to 35.0, `BRAKE_MAX_KG` 0.0 to 200.0, `BRAKE_MAX_INACTIVE_PERC`
/// 1.0 to 100.0, `ADAPTIVE_FFB` whole 0 to 3 (its comment lists 0 to 3),
/// `SKIP_DAMPING_FRAMES` whole 0 to 5 (the v0.3.0 comment; v0.2.11 says 15).
/// Booleans are not checked. Returns an empty list when all values are in range.
pub fn validate(t: &FfbTweaks) -> Vec<String> {
    let mut out = Vec::new();
    check(&mut out, "POSTPROCESSING.RANGE_COMPRESSION", t.range_compression, 0.5, 4.0, false);
    check(&mut out, "POSTPROCESSING.DIRT_VIBRATION", t.dirt_vibration, 0.0, 1.5, false);
    check(&mut out, "CUSTOM_SOFT_LOCK.PADDING", t.soft_lock_padding, 4.0, 20.0, false);
    check(&mut out, "CUSTOM_SOFT_LOCK.SHIFT_PADDING", t.soft_lock_shift_padding, 0.0, 1.0, false);
    check(&mut out, "CUSTOM_SOFT_LOCK.FORCE_FACTOR", t.soft_lock_force_factor, 0.0, 2.0, false);
    check(&mut out, "CUSTOM_SOFT_LOCK.SPEED_FACTOR", t.soft_lock_speed_factor, 0.0, 2.0, false);
    check(&mut out, "CUSTOM_SOFT_LOCK.DAMPER", t.soft_lock_damper, 0.0, 1.0, false);
    check(&mut out, "REAL_FEEL.WHEEL_MAX_NM", t.real_feel_wheel_max_nm, 2.0, 35.0, false);
    check(&mut out, "REAL_FEEL_BRAKE.BRAKE_MAX_KG", t.brake_max_kg, 0.0, 200.0, false);
    check(&mut out, "REAL_FEEL_BRAKE.BRAKE_MAX_INACTIVE_PERC", t.brake_max_inactive_perc, 1.0, 100.0, false);
    check(&mut out, "TWEAKS.ADAPTIVE_FFB", t.adaptive_ffb, 0.0, 3.0, true);
    check(&mut out, "TWEAKS.SKIP_DAMPING_FRAMES", t.skip_damping_frames, 0.0, 5.0, true);
    out
}

/// Compare the tuning file of two AC roots (for example the lights-patch v0.2.11 and
/// v0.3.0 folders). Returns `(section, key, v02 value, v03 value)` for each key whose
/// value differs. A key missing from one side gives `None` there. Values are compared
/// without inline comments, and numbers numerically (`0` equals `0.0`). The `[ℹ]`
/// metadata section and `__` hint keys are skipped. A missing or unreadable file counts
/// as having no keys. Results are sorted by section, then key.
pub fn version_diff(
    v02_dir: &Path,
    v03_dir: &Path,
) -> Vec<(String, String, Option<String>, Option<String>)> {
    let a = settings_map(v02_dir);
    let b = settings_map(v03_dir);
    let keys: BTreeSet<(String, String)> = a.keys().chain(b.keys()).cloned().collect();
    let mut out = Vec::new();
    for k in keys {
        let (va, vb) = (a.get(&k), b.get(&k));
        let same = match (va, vb) {
            (Some(x), Some(y)) => values_equal(&x.2, &y.2),
            (None, None) => true,
            _ => false,
        };
        if same {
            continue;
        }
        let Some(x) = va.or(vb) else { continue };
        out.push((x.0.clone(), x.1.clone(), va.map(|v| v.2.clone()), vb.map(|v| v.2.clone())));
    }
    out
}

/// Typed values from a parsed file, falling back to defaults.
fn from_ini(ini: &Ini) -> FfbTweaks {
    let mut t = FfbTweaks::default();
    for slot in slots(&mut t) {
        match slot {
            Slot::Float { section, key, value } => {
                if let Some(v) = ini.get(section, key).and_then(parse_float) {
                    *value = v;
                }
            }
            Slot::Flag { section, key, value } => {
                if let Some(v) = ini.get(section, key).and_then(parse_flag) {
                    *value = v;
                }
            }
        }
    }
    t
}

/// Value text with its inline `; comment` removed, trimmed.
fn strip_comment(raw: &str) -> &str {
    raw.split(';').next().unwrap_or("").trim()
}

fn parse_float(raw: &str) -> Option<f64> {
    strip_comment(raw).parse().ok()
}

fn parse_flag(raw: &str) -> Option<bool> {
    match strip_comment(raw) {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// New value text, followed by the old value's inline comment (with its spacing) if it had one.
fn keep_comment(old: Option<&str>, new_value: &str) -> String {
    let Some(old) = old else { return new_value.to_string() };
    let Some(idx) = old.find(';') else { return new_value.to_string() };
    let head = &old[..idx];
    let gap = &head[head.trim_end().len()..];
    format!("{new_value}{gap}{}", &old[idx..])
}

fn check(out: &mut Vec<String>, name: &str, v: f64, lo: f64, hi: f64, whole: bool) {
    if !v.is_finite() {
        out.push(format!("{name} is not a finite number"));
        return;
    }
    if whole && v.fract() != 0.0 {
        out.push(format!("{name} = {v} must be a whole number"));
    }
    if v < lo || v > hi {
        out.push(format!("{name} = {v} is outside {lo}..={hi}"));
    }
}

/// Normalized (section, key) -> (section as written, key as written, value without comment).
type SettingsMap = BTreeMap<(String, String), (String, String, String)>;

fn settings_map(ac_root: &Path) -> SettingsMap {
    let mut map = SettingsMap::new();
    let Ok(ini) = Ini::load(&config_path(ac_root)) else { return map };
    for section in ini.section_names() {
        if section.eq_ignore_ascii_case("ℹ") {
            continue;
        }
        for (key, raw) in ini.entries(section) {
            if key.starts_with("__") {
                continue;
            }
            map.entry((section.to_uppercase(), key.to_uppercase()))
                .or_insert_with(|| (section.to_string(), key.to_string(), strip_comment(raw).to_string()));
        }
    }
    map
}

fn values_equal(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Active keys of the v0.3.0-preview675 file, with short comments.
    const SAMPLE_V03: &str = "[ℹ]\nFULLNAME=Input: Wheel (FFB Tweaks)\nORDER=-1.91\n\n\
        [BASIC]\nENABLED=1 ; Active; 1 or 0\n\n\
        [CUSTOM_SOFT_LOCK]\nENABLED=1 ; Active\nPADDING=10 ; Lock area\nSHIFT_PADDING=0.8 ; Shift\n\
        FORCE_FACTOR=1 ; Force\nSPEED_FACTOR=1 ; Speed\nDAMPER=0 ; Damper\n\n\
        [POSTPROCESSING]\nRANGE_COMPRESSION=1.0 ; Range\nRANGE_COMPRESSION_ASSIST=0 ; Assist\n\
        CLEAN_FFB=1 ; Clean\nDIRT_VIBRATION=0.0 ; Dirt\n\n\
        [POSTPROCESSING_SCRIPT]\nENABLED=0 ; Active\nIMPLEMENTATION=alternative ; Script\n\n\
        [REAL_FEEL]\nENABLED=0 ; Active\nWHEEL_MAX_NM=35.0 ; Torque\nENABLE_FOR_ALL=0 ; Override\n\n\
        [REAL_FEEL_BRAKE]\nENABLED=0 ; Active\nBRAKE_MAX_KG=0.0 ; Max pedal\n\
        BRAKE_MAX_INACTIVE_PERC=100.0; Max percentage\nCLAMP_FORCES=1 ; Clamp\n\n\
        [GYRO2]\nENABLED=0 ; Active\n\n\
        [TWEAKS]\nADAPTIVE_FFB=0 ; Adaptive\nAVERAGE_SKIPPED_FFB=1 ; Average\nSKIP_DAMPING_FRAMES=3 ; Skip\n";

    /// Active keys of the v0.2.11 file that differ from v0.3.0.
    const SAMPLE_V02_TWEAKS: &str = "[TWEAKS]\nAVERAGE_SKIPPED_FFB=0 ; Average\n\
        SKIP_DAMPING_FRAMES=5 ; Skip\nSTRUT_FFB_ADJUSTMENT=1 ; Strut\nFADE_IN_AFTER_LOADING=1 ; Fade\n\
        [REAL_FEEL_BRAKE]\nCLAMP_FORCES=1 ; Clamp\n";

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("acxs-ffb-{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn write_file(root: &Path, text: &str) {
        let path = config_path(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn defaults_match_sample_files() {
        assert_eq!(from_ini(&Ini::parse(SAMPLE_V03)), FfbTweaks::default());
        let v02 = from_ini(&Ini::parse(SAMPLE_V02_TWEAKS));
        assert!(!v02.average_skipped_ffb);
        assert_eq!(v02.skip_damping_frames, 5.0);
        assert!(v02.strut_ffb_adjustment && v02.fade_in_after_loading);
    }

    #[test]
    fn missing_file_reads_defaults() {
        let root = temp_root("missing");
        assert_eq!(read(&root).unwrap(), FfbTweaks::default());
    }

    #[test]
    fn round_trip_on_temp_dir() {
        let root = temp_root("roundtrip");
        let mut t = FfbTweaks {
            range_compression: 2.5,
            clean_ffb: false,
            soft_lock_padding: 12.0,
            skip_damping_frames: 2.0,
            real_feel_enabled: true,
            ..FfbTweaks::default()
        };
        write(&root, &t).unwrap();
        assert_eq!(read(&root).unwrap(), t);

        t.dirt_vibration = 0.5;
        write(&root, &t).unwrap();
        assert_eq!(read(&root).unwrap(), t);
        // The first version was moved aside, not deleted.
        let files = fs::read_dir(config_path(&root).parent().unwrap()).unwrap().count();
        assert_eq!(files, 2);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unknown_keys_and_comments_are_preserved() {
        let root = temp_root("unknown");
        write_file(
            &root,
            "; my notes\r\n[BASIC]\r\nENABLED=1 ; on\r\nMY_KEY=hello\r\n\
             [MY_SECTION]\r\nX=7\r\n[TWEAKS]\r\nFOO=bar\r\nSKIP_DAMPING_FRAMES=5 ; comment kept\r\n",
        );
        let mut t = read(&root).unwrap();
        assert_eq!(t.skip_damping_frames, 5.0);
        t.skip_damping_frames = 4.0;
        t.basic_enabled = false;
        write(&root, &t).unwrap();

        let text = fs::read_to_string(config_path(&root)).unwrap();
        assert!(text.contains("; my notes"));
        assert!(text.contains("MY_KEY=hello"));
        assert!(text.contains("[MY_SECTION]\r\nX=7"));
        assert!(text.contains("FOO=bar"));
        assert!(text.contains("ENABLED=0 ; on"));
        assert!(text.contains("SKIP_DAMPING_FRAMES=4 ; comment kept"));
        // Keys left at their default are not added to an existing file.
        assert!(!text.contains("GYRO2"));
        assert_eq!(read(&root).unwrap().skip_damping_frames, 4.0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unchanged_write_does_not_rewrite() {
        let root = temp_root("unchanged");
        write(&root, &FfbTweaks::default()).unwrap();
        write(&root, &FfbTweaks::default()).unwrap();
        let files = fs::read_dir(config_path(&root).parent().unwrap()).unwrap().count();
        assert_eq!(files, 1);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn non_finite_values_are_rejected() {
        let root = temp_root("nan");
        let t = FfbTweaks {
            dirt_vibration: f64::NAN,
            ..FfbTweaks::default()
        };
        let err = write(&root, &t).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!config_path(&root).exists());
    }

    #[test]
    fn validate_defaults_are_clean() {
        assert!(validate(&FfbTweaks::default()).is_empty());
    }

    #[test]
    fn validate_flags_out_of_range() {
        let t = FfbTweaks {
            range_compression: 5.0,
            skip_damping_frames: 9.0,
            adaptive_ffb: 4.0,
            soft_lock_damper: f64::INFINITY,
            ..FfbTweaks::default()
        };
        let problems = validate(&t);
        assert_eq!(problems.len(), 4);
        assert!(problems[0].contains("RANGE_COMPRESSION"));
        assert!(problems.iter().any(|p| p.contains("SKIP_DAMPING_FRAMES")));
        assert!(problems.iter().any(|p| p.contains("ADAPTIVE_FFB")));
        assert!(problems.iter().any(|p| p.contains("DAMPER")));
    }

    #[test]
    fn version_diff_on_synthetic_dirs() {
        let a = temp_root("diff-a");
        let b = temp_root("diff-b");
        write_file(
            &a,
            "[ℹ]\nFULLNAME=Old\n[BASIC]\nENABLED=1 ; on\n[POSTPROCESSING]\nCLEAN_FFB=1 ; new\n\
             DIRT_VIBRATION=0.0\nRANGE_COMPRESSION=1.0\n[TWEAKS]\nAVERAGE_SKIPPED_FFB=0\n\
             SKIP_DAMPING_FRAMES=5\nSTRUT_FFB_ADJUSTMENT=1\n",
        );
        write_file(
            &b,
            "[ℹ]\nFULLNAME=New\n[BASIC]\nENABLED=1\n[POSTPROCESSING]\nCLEAN_FFB=1\n\
             DIRT_VIBRATION=0\nRANGE_COMPRESSION=2.0\n[TWEAKS]\nADAPTIVE_FFB=0\n\
             AVERAGE_SKIPPED_FFB=1\nSKIP_DAMPING_FRAMES=3\n",
        );
        let diff = version_diff(&a, &b);
        let find = |sec: &str, key: &str| {
            diff.iter()
                .find(|(s, k, _, _)| s == sec && k == key)
                .map(|(_, _, x, y)| (x.clone(), y.clone()))
        };
        assert_eq!(diff.len(), 5, "{diff:?}");
        assert_eq!(find("POSTPROCESSING", "RANGE_COMPRESSION"), Some((Some("1.0".into()), Some("2.0".into()))));
        assert_eq!(find("TWEAKS", "AVERAGE_SKIPPED_FFB"), Some((Some("0".into()), Some("1".into()))));
        assert_eq!(find("TWEAKS", "SKIP_DAMPING_FRAMES"), Some((Some("5".into()), Some("3".into()))));
        assert_eq!(find("TWEAKS", "STRUT_FFB_ADJUSTMENT"), Some((Some("1".into()), None)));
        assert_eq!(find("TWEAKS", "ADAPTIVE_FFB"), Some((None, Some("0".into()))));
        // Comment-only and numerically equal values are not reported, nor metadata.
        assert!(find("POSTPROCESSING", "CLEAN_FFB").is_none());
        assert!(find("POSTPROCESSING", "DIRT_VIBRATION").is_none());
        assert!(find("ℹ", "FULLNAME").is_none());
        fs::remove_dir_all(&a).unwrap();
        fs::remove_dir_all(&b).unwrap();
    }
}
