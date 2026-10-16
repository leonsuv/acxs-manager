//! Lighting profiles: named sets of lighting values that can be captured from a
//! lights patch configuration file, edited, saved as JSON and applied back.
//!
//! A profile only carries values. The rendering itself is done by the installed
//! lights patch, which reads the file this module writes. Writes go through
//! [`Ini::save_with_backup`], so the previous file is kept next to the new one
//! and nothing is deleted. Only the keys in a profile are touched; every other
//! line of the file is left as it was.

use crate::ini::Ini;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Sections of the lighting configuration that a profile covers.
pub const PROFILE_SECTIONS: [&str; 4] = ["BASIC", "RANGES", "SHADOWS", "PERFORMANCE"];

/// File name of the lighting configuration inside its folder.
const CONFIG_FILE: &str = "lighting_fx.ini";

/// `(section, key)` that only the 0.2 lighting configuration has.
const ONLY_IN_02: [(&str, &str); 1] = [("BASIC", "BRIGHTNESS_MULT")];

/// `(section, key)` that only the 0.3 lighting configuration has.
const ONLY_IN_03: [(&str, &str); 1] = [("ℹ", "ORDER")];

/// A named set of lighting values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightsProfile {
    pub name: String,
    /// `(section, key, value)` in the order they are shown and written.
    pub values: Vec<(String, String, String)>,
}

/// What [`apply_profile`] did to the configuration file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Values that differed from the file and were written.
    pub changed: usize,
    /// Values that already matched the file and were left alone.
    pub unchanged: usize,
}

/// Captures the lighting values of a configuration file.
///
/// Only the sections in [`PROFILE_SECTIONS`] are read. Inline comments are
/// dropped from the captured values. The returned profile has a placeholder
/// name that the caller is expected to replace.
pub fn profile_from_config(config_path: &Path) -> io::Result<LightsProfile> {
    let ini = Ini::load(config_path)?;
    let mut values = Vec::new();
    for section in PROFILE_SECTIONS {
        for (key, value) in ini.entries(section) {
            values.push((section.to_string(), key.to_string(), bare(value).to_string()));
        }
    }
    if values.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no lighting values found in {}", config_path.display()),
        ));
    }
    Ok(LightsProfile {
        name: "Current values".to_string(),
        values,
    })
}

/// Writes the profile's values into the configuration file.
///
/// Every key is checked before anything is written. A key that is not in the
/// file, or that lies outside [`PROFILE_SECTIONS`], is refused unless
/// `allow_new_keys` is set. Values equal to the file are left alone. When at
/// least one value changes, the file is saved with [`Ini::save_with_backup`].
pub fn apply_profile(
    config_path: &Path,
    profile: &LightsProfile,
    allow_new_keys: bool,
) -> io::Result<ApplyReport> {
    let problems = validate_profile(profile);
    if !problems.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, problems.join("; ")));
    }
    let mut ini = Ini::load(config_path)?;

    for (section, key, _) in &profile.values {
        let covered = PROFILE_SECTIONS.iter().any(|s| s.eq_ignore_ascii_case(section));
        let known = ini.get(section, key).is_some();
        if !allow_new_keys && (!known || !covered) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("[{section}] {key} is not a known lighting key"),
            ));
        }
    }

    let mut report = ApplyReport::default();
    let mut writes = Vec::new();
    for (section, key, value) in &profile.values {
        if ini.get(section, key).map(bare) == Some(value.as_str()) {
            report.unchanged += 1;
        } else {
            report.changed += 1;
            writes.push((section, key, value));
        }
    }

    if report.changed > 0 {
        for (section, key, value) in writes {
            ini.set(section, key, value);
        }
        ini.save_with_backup(config_path)?;
    }
    Ok(report)
}

/// Problems that would stop a profile from being applied or saved.
///
/// A value is checked as a number when its key is a multiplier or its text
/// starts like a number. Numbers may be a comma separated list. Any other text
/// is accepted. Multipliers may not be negative. Keys may appear only once.
pub fn validate_profile(profile: &LightsProfile) -> Vec<String> {
    let mut problems = Vec::new();
    if profile.values.is_empty() {
        problems.push("the profile has no values".to_string());
    }
    let mut seen = HashSet::new();
    for (section, key, value) in &profile.values {
        let label = format!("[{section}] {key}");
        if !seen.insert((section.to_ascii_uppercase(), key.to_ascii_uppercase())) {
            problems.push(format!("{label} appears more than once"));
        }
        let text = bare(value);
        let multiplier = is_multiplier(key);
        if !multiplier && !looks_numeric(text) {
            continue;
        }
        match numbers(text) {
            None => problems.push(format!("{label}: \"{value}\" is not a number")),
            Some(nums) if multiplier && nums.iter().any(|n| *n < 0.0) => {
                problems.push(format!("{label}: multiplier is negative"));
            }
            Some(_) => {}
        }
    }
    problems
}

/// Saves the profile as `<name>.json` in `profiles_dir`, creating the folder if needed.
///
/// The file is created new, so an existing profile is never overwritten. The
/// name is reduced to letters, digits, spaces, `-` and `_`. Returns the path written.
pub fn save_profile(profiles_dir: &Path, profile: &LightsProfile) -> io::Result<PathBuf> {
    let problems = validate_profile(profile);
    if !problems.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, problems.join("; ")));
    }
    let stem = file_stem(&profile.name)?;
    fs::create_dir_all(profiles_dir)?;
    let path = profiles_dir.join(format!("{stem}.json"));
    let json = serde_json::to_string_pretty(profile).map_err(io::Error::other)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    file.write_all(json.as_bytes())?;
    Ok(path)
}

/// Reads a profile JSON file and checks it with [`validate_profile`].
pub fn load_profile(path: &Path) -> io::Result<LightsProfile> {
    let text = fs::read_to_string(path)?;
    let profile: LightsProfile = serde_json::from_str(&text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let problems = validate_profile(&profile);
    if problems.is_empty() {
        Ok(profile)
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, problems.join("; ")))
    }
}

/// Lighting configuration version of `config_dir`, as `"0.2"` or `"0.3"`.
///
/// Read from the keys in the `lighting_fx.ini` of that folder. `None` when the
/// file cannot be read or its keys match neither version or both.
pub fn detect_version(config_dir: &Path) -> Option<String> {
    let ini = Ini::load(&config_dir.join(CONFIG_FILE)).ok()?;
    let has = |keys: &[(&str, &str)]| keys.iter().any(|(s, k)| ini.get(s, k).is_some());
    match (has(&ONLY_IN_02[..]), has(&ONLY_IN_03[..])) {
        (true, false) => Some("0.2".to_string()),
        (false, true) => Some("0.3".to_string()),
        _ => None,
    }
}

/// Differences between two lighting configuration files.
///
/// Lists the sections and keys that only one file has, and the values that
/// differ for keys present in both. Values are compared without inline
/// comments. Lines say "first" or "second" for the file, so no paths appear.
pub fn key_differences(a: &Path, b: &Path) -> io::Result<Vec<String>> {
    let first = Ini::load(a)?;
    let second = Ini::load(b)?;
    let mut out = Vec::new();
    for (side, here, there) in [("first", &first, &second), ("second", &second, &first)] {
        for section in here.section_names() {
            if !has_section(there, section) {
                out.push(format!("section [{section}] only in the {side} file"));
                continue;
            }
            for (key, _) in here.entries(section) {
                if there.get(section, key).is_none() {
                    out.push(format!("[{section}] {key} only in the {side} file"));
                }
            }
        }
    }
    for section in first.section_names() {
        if !has_section(&second, section) {
            continue;
        }
        for (key, value) in first.entries(section) {
            if let Some(other) = second.get(section, key)
                && bare(value) != bare(other)
            {
                out.push(format!(
                    "[{section}] {key}: \"{}\" in the first file, \"{}\" in the second",
                    bare(value),
                    bare(other)
                ));
            }
        }
    }
    Ok(out)
}

/// Keys that differ between the 0.2 and 0.3 lighting configurations, from the
/// keys each version has. Used when only one folder is at hand.
pub fn version_differences() -> Vec<String> {
    ONLY_IN_02
        .iter()
        .map(|(s, k)| format!("[{s}] {k} only in 0.2"))
        .chain(ONLY_IN_03.iter().map(|(s, k)| format!("[{s}] {k} only in 0.3")))
        .collect()
}

/// Keys of `profile` that the lighting configuration of `version` does not have,
/// as `[section] key`. Empty for a version that is not known.
pub fn unknown_keys(profile: &LightsProfile, version: &str) -> Vec<String> {
    let missing: &[(&str, &str)] = match version {
        "0.2" => &ONLY_IN_03,
        "0.3" => &ONLY_IN_02,
        _ => return Vec::new(),
    };
    profile
        .values
        .iter()
        .filter(|(s, k, _)| {
            missing
                .iter()
                .any(|(ms, mk)| s.eq_ignore_ascii_case(ms) && k.eq_ignore_ascii_case(mk))
        })
        .map(|(s, k, _)| format!("[{s}] {k}"))
        .collect()
}

fn has_section(ini: &Ini, name: &str) -> bool {
    ini.section_names().iter().any(|s| s.eq_ignore_ascii_case(name))
}

/// Value with any inline `;` comment removed and surrounding space trimmed.
fn bare(value: &str) -> &str {
    value.split(';').next().unwrap_or("").trim()
}

fn looks_numeric(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))
}

fn is_multiplier(key: &str) -> bool {
    key.to_ascii_uppercase().contains("MULT")
}

/// One number, or a comma separated list of numbers. `None` when any part fails to parse.
fn numbers(text: &str) -> Option<Vec<f64>> {
    if text.is_empty() {
        return None;
    }
    text.split(',')
        .map(|part| part.trim().parse::<f64>().ok().filter(|n| n.is_finite()))
        .collect()
}

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

    const CONFIG: &str = "[BASIC]\r\nNAME=Test\r\nBRIGHTNESS=1.5 ; inline\r\n\
        [RANGES]\r\nMIN=0.1\r\nMAX=4\r\n\
        [SHADOWS]\r\nSTRENGTH=0.8\r\nSHADOW_MULT=1.0\r\n\
        [PERFORMANCE]\r\nLEVEL=2\r\n\
        [OTHER]\r\nIGNORED=7\r\n";

    /// Temporary folder that is removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let path = std::env::temp_dir().join(format!(
                "acxs-lights-profile-{}-{tag}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn config(&self) -> PathBuf {
            let path = self.0.join("lighting_fx.ini");
            fs::write(&path, CONFIG).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn profile(values: &[(&str, &str, &str)]) -> LightsProfile {
        LightsProfile {
            name: "Test".to_string(),
            values: values
                .iter()
                .map(|(s, k, v)| (s.to_string(), k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn lights_profile_captures_only_profile_sections() {
        let scratch = Scratch::new("capture");
        let captured = profile_from_config(&scratch.config()).unwrap();
        assert!(captured.values.iter().any(|(s, k, v)| s == "BASIC" && k == "BRIGHTNESS" && v == "1.5"));
        assert!(captured.values.iter().any(|(s, k, _)| s == "SHADOWS" && k == "SHADOW_MULT"));
        assert!(!captured.values.iter().any(|(_, k, _)| k == "IGNORED"));
        assert_eq!(captured.values.len(), 7);
    }

    #[test]
    fn lights_profile_apply_writes_only_profile_keys_and_backs_up() {
        let scratch = Scratch::new("apply");
        let config = scratch.config();
        let edit = profile(&[
            ("BASIC", "BRIGHTNESS", "2.0"),
            ("SHADOWS", "STRENGTH", "0.8"),
        ]);
        let report = apply_profile(&config, &edit, false).unwrap();
        assert_eq!(report, ApplyReport { changed: 1, unchanged: 1 });

        let ini = Ini::load(&config).unwrap();
        assert_eq!(ini.get("BASIC", "BRIGHTNESS"), Some("2.0"));
        assert_eq!(ini.get("PERFORMANCE", "LEVEL"), Some("2"));
        assert_eq!(ini.get("OTHER", "IGNORED"), Some("7"));
        // The config and its backup are the only entries in the folder.
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 2);
    }

    #[test]
    fn lights_profile_apply_unchanged_values_writes_nothing() {
        let scratch = Scratch::new("noop");
        let config = scratch.config();
        let same = profile_from_config(&config).unwrap();
        let report = apply_profile(&config, &same, false).unwrap();
        assert_eq!(report.changed, 0);
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
    }

    #[test]
    fn lights_profile_apply_refuses_unknown_keys_without_writing() {
        let scratch = Scratch::new("refuse");
        let config = scratch.config();
        let before = fs::read_to_string(&config).unwrap();

        let unknown = profile(&[("BASIC", "BRIGHTNESS", "3"), ("SHADOWS", "NEW_KEY", "1")]);
        assert!(apply_profile(&config, &unknown, false).is_err());
        let outside = profile(&[("OTHER", "IGNORED", "1")]);
        assert!(apply_profile(&config, &outside, false).is_err());

        assert_eq!(fs::read_to_string(&config).unwrap(), before);
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
    }

    #[test]
    fn lights_profile_apply_adds_new_keys_when_allowed() {
        let scratch = Scratch::new("newkeys");
        let config = scratch.config();
        let add = profile(&[("SHADOWS", "NEW_KEY", "1")]);
        let report = apply_profile(&config, &add, true).unwrap();
        assert_eq!(report.changed, 1);
        assert_eq!(Ini::load(&config).unwrap().get("SHADOWS", "NEW_KEY"), Some("1"));
    }

    #[test]
    fn lights_profile_validate_reports_bad_values() {
        assert!(validate_profile(&profile(&[("BASIC", "BRIGHTNESS", "1.5")])).is_empty());
        // Text values are accepted; numeric-looking ones must parse.
        assert!(validate_profile(&profile(&[("BASIC", "NAME", "Track day")])).is_empty());
        assert!(validate_profile(&profile(&[("RANGES", "LIST", "0.1, 0.2")])).is_empty());

        let problems = validate_profile(&profile(&[
            ("RANGES", "MAX", "1.2.3"),
            ("SHADOWS", "SHADOW_MULT", "-0.5"),
            ("SHADOWS", "shadow_mult", "1"),
            ("BASIC", "EMPTY_MULT", ""),
        ]));
        assert_eq!(problems.len(), 4);
        assert!(problems.iter().any(|p| p.contains("MAX") && p.contains("not a number")));
        assert!(problems.iter().any(|p| p.contains("negative")));
        assert!(problems.iter().any(|p| p.contains("more than once")));
        assert!(validate_profile(&profile(&[])).len() == 1);
    }

    #[test]
    fn lights_profile_save_and_load_round_trip_never_overwrites() {
        let scratch = Scratch::new("files");
        let dir = scratch.0.join("profiles");
        let mut saved = profile(&[("BASIC", "BRIGHTNESS", "1.5")]);
        saved.name = "Night/Rain".to_string();

        let path = save_profile(&dir, &saved).unwrap();
        assert_eq!(path.file_name().unwrap(), "Night_Rain.json");
        assert_eq!(load_profile(&path).unwrap(), saved);

        let again = save_profile(&dir, &saved);
        assert_eq!(again.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn lights_profile_load_rejects_invalid_files() {
        let scratch = Scratch::new("badload");
        let path = scratch.0.join("bad.json");
        fs::write(
            &path,
            r#"{"name":"bad","values":[["SHADOWS","SHADOW_MULT","-2"]]}"#,
        )
        .unwrap();
        assert_eq!(load_profile(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);

        fs::write(&path, "not json").unwrap();
        assert!(load_profile(&path).is_err());
    }

    const OLD_STYLE: &str = "[BASIC]\r\nENABLED=1\r\nBRIGHTNESS_MULT=1\r\n[PERFORMANCE]\r\nLEVEL=2\r\n";
    const NEW_STYLE: &str = "[ℹ]\r\nFULLNAME=Changed name\r\nORDER=-1.6\r\n[BASIC]\r\nENABLED=1\r\n";

    /// Writes `text` as the configuration file of a new folder inside `scratch`.
    fn config_dir(scratch: &Scratch, tag: &str, text: &str) -> PathBuf {
        let dir = scratch.0.join(tag);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("lighting_fx.ini"), text).unwrap();
        dir
    }

    #[test]
    fn lights_profile_detect_version_from_keys() {
        let scratch = Scratch::new("version");
        let old = config_dir(&scratch, "old", OLD_STYLE);
        let new = config_dir(&scratch, "new", NEW_STYLE);
        let neither = config_dir(&scratch, "neither", "[BASIC]\r\nENABLED=1\r\n");
        let both = config_dir(&scratch, "both", "[ℹ]\r\nORDER=1\r\n[BASIC]\r\nBRIGHTNESS_MULT=1\r\n");
        assert_eq!(detect_version(&old).as_deref(), Some("0.2"));
        assert_eq!(detect_version(&new).as_deref(), Some("0.3"));
        assert_eq!(detect_version(&neither), None);
        assert_eq!(detect_version(&both), None);
        assert_eq!(detect_version(&scratch.0.join("missing")), None);
    }

    #[test]
    fn lights_profile_key_differences_lists_one_sided_keys_and_changed_values() {
        let scratch = Scratch::new("diff");
        let a = config_dir(&scratch, "a", OLD_STYLE).join("lighting_fx.ini");
        let b = config_dir(&scratch, "b", NEW_STYLE).join("lighting_fx.ini");
        let changed = config_dir(
            &scratch,
            "changed",
            "[BASIC]\r\nENABLED=0 ; off\r\nBRIGHTNESS_MULT=1\r\n[PERFORMANCE]\r\nLEVEL=2\r\n",
        )
        .join("lighting_fx.ini");

        let diffs = key_differences(&a, &b).unwrap();
        assert_eq!(diffs.len(), 3, "{diffs:?}");
        assert!(diffs.contains(&"[BASIC] BRIGHTNESS_MULT only in the first file".to_string()));
        assert!(diffs.contains(&"section [PERFORMANCE] only in the first file".to_string()));
        assert!(diffs.contains(&"section [ℹ] only in the second file".to_string()));

        let value_diffs = key_differences(&a, &changed).unwrap();
        assert_eq!(value_diffs.len(), 1, "{value_diffs:?}");
        assert!(value_diffs[0].starts_with("[BASIC] ENABLED: \"1\" in the first file, \"0\""));

        assert!(key_differences(&a, &a).unwrap().is_empty());
        assert!(key_differences(&scratch.0.join("missing.ini"), &a).is_err());
    }

    #[test]
    fn lights_profile_version_differences_lists_both_directions() {
        let diffs = version_differences();
        assert_eq!(diffs.len(), 2);
        assert!(diffs.iter().any(|d| d.contains("BRIGHTNESS_MULT") && d.ends_with("only in 0.2")));
        assert!(diffs.iter().any(|d| d.contains("ORDER") && d.ends_with("only in 0.3")));
    }

    #[test]
    fn lights_profile_unknown_keys_follow_version() {
        let p = profile(&[("BASIC", "BRIGHTNESS_MULT", "1"), ("PERFORMANCE", "LEVEL", "2")]);
        assert_eq!(unknown_keys(&p, "0.3"), vec!["[BASIC] BRIGHTNESS_MULT".to_string()]);
        assert!(unknown_keys(&p, "0.2").is_empty());
        assert!(unknown_keys(&p, "9.9").is_empty());
    }
}
