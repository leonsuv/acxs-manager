//! Read-only reader for a track's `data/surfaces.ini`.
//!
//! Sections are `[SURFACE_0]`, `[SURFACE_1]`, ... counted up from 0 until the
//! first missing number. Sections without a non-empty `KEY` are skipped.
//!
//! Keys read: `KEY`, `WAV`, `FRICTION`, `IS_PITLANE`, `IS_VALID_TRACK`.
//! A flag counts as set only when its value is exactly `1`.
//! Other surface keys (damping, wall penalty, sin height and length) are not read.
//!
//! A layout's surfaces are a separate call: pass the layout's own data directory
//! to `read_surfaces`. Merging track and layout lists is up to the caller.

use std::io;
use std::path::Path;

use crate::ini::Ini;

#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// Value of `KEY`. Always non-empty.
    pub name: String,
    /// Value of `WAV`, when present and non-empty.
    pub wav_path: Option<String>,
    /// Value of `FRICTION`, when present and numeric. Left as `None` when missing so the
    /// caller can choose its own default.
    pub friction: Option<f64>,
    /// `IS_PITLANE == 1`. Missing counts as false.
    pub is_pit: bool,
    /// `IS_VALID_TRACK == 1`. Missing counts as false.
    pub is_valid_track: bool,
}

/// Reads `surfaces.ini` from `data_dir`.
///
/// A missing file returns `Ok(vec![])`: a track with no surfaces file is valid
/// content for the caller to report, not an error. Any other I/O error (including
/// invalid UTF-8) is returned.
pub fn read_surfaces(data_dir: &Path) -> io::Result<Vec<Surface>> {
    let ini = match Ini::load(&data_dir.join("surfaces.ini")) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let section_names: Vec<String> = ini.section_names().iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut surfaces = Vec::new();
    for index in 0u32.. {
        let section = format!("SURFACE_{index}");
        if !section_names.contains(&section.to_ascii_lowercase()) {
            break;
        }
        let Some(name) = non_empty(ini.get(&section, "KEY")) else {
            continue;
        };
        surfaces.push(Surface {
            name: name.to_string(),
            wav_path: non_empty(ini.get(&section, "WAV")).map(str::to_string),
            friction: ini.get_f64(&section, "FRICTION"),
            is_pit: flag(&ini, &section, "IS_PITLANE"),
            is_valid_track: flag(&ini, &section, "IS_VALID_TRACK"),
        });
    }
    Ok(surfaces)
}

/// Returns `(surface count, count of valid-track surfaces)`.
pub fn summarize(surfaces: &[Surface]) -> (usize, usize) {
    (surfaces.len(), surfaces.iter().filter(|s| s.is_valid_track).count())
}

/// Finds a surface by `name`, ignoring ASCII case. Returns the first match.
pub fn find_surface<'a>(surfaces: &'a [Surface], name: &str) -> Option<&'a Surface> {
    surfaces.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// Lowest `FRICTION` value accepted by [`set_friction`].
pub const FRICTION_MIN: f64 = 0.0;
/// Highest `FRICTION` value accepted by [`set_friction`].
pub const FRICTION_MAX: f64 = 2.0;

/// Sets `FRICTION` on the surface named `surface_name` in `data_dir/surfaces.ini`.
///
/// The name matches ASCII case-insensitively, as [`find_surface`] does. The file is
/// saved through [`Ini::save_with_backup`], so the previous version is kept. Other
/// surfaces and keys are left as they were.
///
/// Fails with `NotFound` when the file is missing or no surface has that name, and with
/// `InvalidInput` when `friction` is not in `FRICTION_MIN..=FRICTION_MAX` (NaN included).
/// Nothing is written on failure.
pub fn set_friction(data_dir: &Path, surface_name: &str, friction: f64) -> io::Result<()> {
    if !(FRICTION_MIN..=FRICTION_MAX).contains(&friction) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("friction {friction} is outside {FRICTION_MIN}..={FRICTION_MAX}"),
        ));
    }
    let path = data_dir.join("surfaces.ini");
    let mut ini = Ini::load(&path)?;
    let Some(section) = find_surface_section(&ini, surface_name) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no surface named {:?} in {}", surface_name.trim(), path.display()),
        ));
    };
    ini.set(&section, "FRICTION", friction);
    ini.save_with_backup(&path)
}

/// Section name (`SURFACE_<n>`) of the first surface whose `KEY` matches `name`.
/// Scans the same numbered sections, in the same order, as [`read_surfaces`].
fn find_surface_section(ini: &Ini, name: &str) -> Option<String> {
    let name = name.trim();
    let section_names: Vec<String> = ini.section_names().iter().map(|s| s.to_ascii_lowercase()).collect();
    for index in 0u32.. {
        let section = format!("SURFACE_{index}");
        if !section_names.contains(&section.to_ascii_lowercase()) {
            return None;
        }
        if non_empty(ini.get(&section, "KEY")).is_some_and(|key| key.eq_ignore_ascii_case(name)) {
            return Some(section);
        }
    }
    None
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn flag(ini: &Ini, section: &str, key: &str) -> bool {
    ini.get(section, key).map(str::trim) == Some("1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Temp directory that is removed on drop, even when an assertion fails.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!(
                "acxs_track_surfaces_{}_{}",
                std::process::id(),
                tag
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const SAMPLE: &str = "[HEADER]\r\nVERSION=1\r\n\r\n\
        [SURFACE_0]\r\nKEY=PIT_LANE\r\nFRICTION=0.85\r\nWAV=pit.wav\r\nIS_PITLANE=1\r\nIS_VALID_TRACK=0\r\n\r\n\
        [SURFACE_1]\r\nKEY=grass_side\r\n";

    #[test]
    fn reads_values_and_missing_keys() {
        let dir = TempDir::new("values");
        fs::write(dir.0.join("surfaces.ini"), SAMPLE).unwrap();

        let surfaces = read_surfaces(&dir.0).unwrap();
        assert_eq!(surfaces.len(), 2);

        assert_eq!(
            surfaces[0],
            Surface {
                name: "PIT_LANE".to_string(),
                wav_path: Some("pit.wav".to_string()),
                friction: Some(0.85),
                is_pit: true,
                is_valid_track: false,
            }
        );

        assert_eq!(
            surfaces[1],
            Surface {
                name: "grass_side".to_string(),
                wav_path: None,
                friction: None,
                is_pit: false,
                is_valid_track: false,
            }
        );
    }

    #[test]
    fn missing_file_is_empty_and_stops_at_gap() {
        let dir = TempDir::new("missing");
        assert_eq!(read_surfaces(&dir.0).unwrap(), Vec::new());

        // SURFACE_1 is unreachable because SURFACE_0 is missing: the scan stops at the gap.
        fs::write(
            dir.0.join("surfaces.ini"),
            "[SURFACE_1]\r\nKEY=ORPHAN\r\n",
        )
        .unwrap();
        assert_eq!(read_surfaces(&dir.0).unwrap(), Vec::new());
    }

    #[test]
    fn summarize_counts_valid_track() {
        let surfaces = vec![
            Surface {
                name: "A".into(),
                wav_path: None,
                friction: None,
                is_pit: false,
                is_valid_track: true,
            },
            Surface {
                name: "B".into(),
                wav_path: None,
                friction: None,
                is_pit: true,
                is_valid_track: false,
            },
        ];
        assert_eq!(summarize(&surfaces), (2, 1));
        assert_eq!(summarize(&[]), (0, 0));
    }

    #[test]
    fn find_is_case_insensitive() {
        let dir = TempDir::new("find");
        fs::write(dir.0.join("surfaces.ini"), SAMPLE).unwrap();
        let surfaces = read_surfaces(&dir.0).unwrap();

        assert_eq!(find_surface(&surfaces, "pit_lane").map(|s| s.name.as_str()), Some("PIT_LANE"));
        assert_eq!(find_surface(&surfaces, "GRASS_SIDE").map(|s| s.name.as_str()), Some("grass_side"));
        assert!(find_surface(&surfaces, "kerb").is_none());
    }

    fn backup_count(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("surfaces.ini.acxs-backup-"))
            .count()
    }

    #[test]
    fn set_friction_edits_one_surface_and_keeps_a_backup() {
        let dir = TempDir::new("set_ok");
        fs::write(dir.0.join("surfaces.ini"), SAMPLE).unwrap();

        set_friction(&dir.0, "grass_side", 0.6).unwrap();

        let ini = Ini::load(&dir.0.join("surfaces.ini")).unwrap();
        assert_eq!(ini.get_f64("SURFACE_1", "FRICTION"), Some(0.6));
        // The other surface keeps its value and its keys.
        assert_eq!(ini.get_f64("SURFACE_0", "FRICTION"), Some(0.85));
        assert_eq!(ini.get("SURFACE_0", "WAV"), Some("pit.wav"));
        assert_eq!(backup_count(&dir.0), 1);

        // The old file is kept as the backup, unchanged.
        let backup = fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("surfaces.ini.acxs-backup-"))
            .unwrap();
        assert_eq!(fs::read_to_string(backup).unwrap(), SAMPLE);
    }

    #[test]
    fn set_friction_matches_name_ignoring_case_and_accepts_bounds() {
        let dir = TempDir::new("set_bounds");
        fs::write(dir.0.join("surfaces.ini"), SAMPLE).unwrap();

        set_friction(&dir.0, "pit_lane", 0.0).unwrap();
        let ini = Ini::load(&dir.0.join("surfaces.ini")).unwrap();
        assert_eq!(ini.get_f64("SURFACE_0", "FRICTION"), Some(0.0));

        set_friction(&dir.0, "PIT_LANE", FRICTION_MAX).unwrap();
        let ini = Ini::load(&dir.0.join("surfaces.ini")).unwrap();
        assert_eq!(ini.get_f64("SURFACE_0", "FRICTION"), Some(2.0));
    }

    #[test]
    fn set_friction_refuses_unknown_surface_without_writing() {
        let dir = TempDir::new("set_unknown");
        fs::write(dir.0.join("surfaces.ini"), SAMPLE).unwrap();

        let err = set_friction(&dir.0, "kerb", 1.0).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read_to_string(dir.0.join("surfaces.ini")).unwrap(), SAMPLE);
        assert_eq!(backup_count(&dir.0), 0);
    }

    #[test]
    fn set_friction_refuses_out_of_range_values_without_writing() {
        let dir = TempDir::new("set_range");
        fs::write(dir.0.join("surfaces.ini"), SAMPLE).unwrap();

        for bad in [-0.01, 2.01, f64::NAN, f64::INFINITY] {
            let err = set_friction(&dir.0, "grass_side", bad).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "value {bad}");
        }
        assert_eq!(fs::read_to_string(dir.0.join("surfaces.ini")).unwrap(), SAMPLE);
        assert_eq!(backup_count(&dir.0), 0);
    }

    #[test]
    fn set_friction_fails_when_file_is_missing() {
        let dir = TempDir::new("set_missing");
        let err = set_friction(&dir.0, "grass_side", 1.0).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(!dir.0.join("surfaces.ini").exists());
    }
}
