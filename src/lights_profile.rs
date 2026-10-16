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
}
