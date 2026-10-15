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
}
