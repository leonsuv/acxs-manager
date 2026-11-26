//! Compare two installed track folders at the level of their metadata and surfaces.
//!
//! Read-only. Metadata comes from `ui/ui_track.json` through `content::read_ui_json`
//! and `content_meta::TrackMeta`. Layouts are sub-folders that contain a `ui` folder,
//! as in `content::scan_tracks`. Surfaces come from the track's own `data/surfaces.ini`
//! (layout surfaces are not compared).
//!
//! Missing data is treated as empty, never as an error:
//! * a track folder that does not exist has no metadata, no layouts and no surfaces;
//! * a missing `ui_track.json`, `data` folder or `surfaces.ini` contributes nothing.
//!
//! An empty text value (or `None` for length and pitboxes) means "not set" and is
//! shown as an empty string. As in `scan_tracks`, a track without a name uses its
//! folder name. Only a surfaces file that exists but cannot be read (for example
//! invalid UTF-8) makes the functions return an error.
//!
//! Surfaces are matched by name, ignoring ASCII case, as `track_surfaces::find_surface` does.

use std::io;
use std::path::Path;

use crate::content::{read_ui_json, TrackEntry};
use crate::content_meta::TrackMeta;
use crate::track_surfaces::{find_surface, read_surfaces, summarize, Surface};

/// One differing value between two tracks. `left` and `right` are the displayed values
/// for each side, and an empty string means "not set" or "not present".
#[derive(Debug, Clone, PartialEq)]
pub struct TrackDiff {
    pub label: String,
    pub left: String,
    pub right: String,
}

/// Compares the metadata and surfaces of two track folders.
///
/// Returns one row per value that differs. Rows, in order: `name`, `city`, `country`,
/// `length (m)`, `pitboxes`, `layouts`, `surfaces`, `valid-track surfaces`, then
/// `surfaces only on left` and `surfaces only on right` (names joined with ", ",
/// empty side left blank). The two surface-membership rows appear only when non-empty.
pub fn compare_tracks(left_track: &Path, right_track: &Path) -> io::Result<Vec<TrackDiff>> {
    let (left, left_surfaces) = load_track(left_track)?;
    let (right, right_surfaces) = load_track(right_track)?;

    let mut rows = Vec::new();
    push_if_changed(&mut rows, "name", left.name.clone(), right.name.clone());
    push_if_changed(&mut rows, "city", left.city.clone(), right.city.clone());
    push_if_changed(&mut rows, "country", left.country.clone(), right.country.clone());
    push_if_changed(
        &mut rows,
        "length (m)",
        fmt_opt(left.length_m),
        fmt_opt(right.length_m),
    );
    push_if_changed(
        &mut rows,
        "pitboxes",
        fmt_opt(left.pitboxes),
        fmt_opt(right.pitboxes),
    );
    push_if_changed(
        &mut rows,
        "layouts",
        left.layouts.join(", "),
        right.layouts.join(", "),
    );

    let (left_count, left_valid) = summarize(&left_surfaces);
    let (right_count, right_valid) = summarize(&right_surfaces);
    push_if_changed(&mut rows, "surfaces", left_count.to_string(), right_count.to_string());
    push_if_changed(
        &mut rows,
        "valid-track surfaces",
        left_valid.to_string(),
        right_valid.to_string(),
    );

    let only_left = names_missing_from(&left_surfaces, &right_surfaces);
    if !only_left.is_empty() {
        rows.push(TrackDiff {
            label: "surfaces only on left".to_string(),
            left: only_left.join(", "),
            right: String::new(),
        });
    }
    let only_right = names_missing_from(&right_surfaces, &left_surfaces);
    if !only_right.is_empty() {
        rows.push(TrackDiff {
            label: "surfaces only on right".to_string(),
            left: String::new(),
            right: only_right.join(", "),
        });
    }

    Ok(rows)
}

/// Lists surface differences, one line each:
/// * `NAME: only in left` or `NAME: only in right` for a surface on one side only;
/// * `NAME: friction A -> B` when both sides have the surface and the friction differs
///   (`none` when the value is missing).
///
/// Lines follow the left surfaces in file order, then the right-only surfaces.
pub fn surface_changes(left_track: &Path, right_track: &Path) -> io::Result<Vec<String>> {
    let left = read_surfaces(&left_track.join("data"))?;
    let right = read_surfaces(&right_track.join("data"))?;

    let mut lines = Vec::new();
    for surface in &left {
        match find_surface(&right, &surface.name) {
            None => lines.push(format!("{}: only in left", surface.name)),
            Some(other) if other.friction != surface.friction => lines.push(format!(
                "{}: friction {} -> {}",
                surface.name,
                fmt_friction(surface.friction),
                fmt_friction(other.friction)
            )),
            Some(_) => {}
        }
    }
    for surface in &right {
        if find_surface(&left, &surface.name).is_none() {
            lines.push(format!("{}: only in right", surface.name));
        }
    }
    Ok(lines)
}

/// Reads one track folder. Returns its entry and its track-level surfaces.
fn load_track(track_dir: &Path) -> io::Result<(TrackEntry, Vec<Surface>)> {
    let ui = read_ui_json(&track_dir.join("ui").join("ui_track.json"));
    let meta = TrackMeta::from_map(&ui);
    let id = track_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let entry = TrackEntry {
        name: trimmed(meta.name)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.clone()),
        city: trimmed(meta.city).unwrap_or_default(),
        country: trimmed(meta.country).unwrap_or_default(),
        length_m: meta.length_m,
        pitboxes: meta.pitboxes,
        layouts: crate::content::track_layouts(track_dir),
        tags: meta.tags.unwrap_or_default(),
        id,
        path: track_dir.to_path_buf(),
    };
    let surfaces = read_surfaces(&track_dir.join("data"))?;
    Ok((entry, surfaces))
}

/// Names of surfaces in `from` that have no match in `other` (by name, ignoring ASCII case).
fn names_missing_from(from: &[Surface], other: &[Surface]) -> Vec<String> {
    from.iter()
        .filter(|s| find_surface(other, &s.name).is_none())
        .map(|s| s.name.clone())
        .collect()
}

fn push_if_changed(rows: &mut Vec<TrackDiff>, label: &str, left: String, right: String) {
    if left != right {
        rows.push(TrackDiff {
            label: label.to_string(),
            left,
            right,
        });
    }
}

fn trimmed(value: Option<String>) -> Option<String> {
    value.map(|s| s.trim().to_string())
}

fn fmt_opt<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn fmt_friction(value: Option<f64>) -> String {
    value.map(|f| f.to_string()).unwrap_or_else(|| "none".to_string())
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
                "acxs_track_compare_{}_{}",
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

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn row(label: &str, left: &str, right: &str) -> TrackDiff {
        TrackDiff {
            label: label.to_string(),
            left: left.to_string(),
            right: right.to_string(),
        }
    }

    const LEFT_SURFACES: &str = "[HEADER]\n\
        VERSION=1\n\
        [SURFACE_0]\nKEY=PIT_LANE\nFRICTION=0.85\nIS_PITLANE=1\nIS_VALID_TRACK=1\n\
        [SURFACE_1]\nKEY=ASPHALT\nFRICTION=0.97\nIS_VALID_TRACK=1\n\
        [SURFACE_2]\nKEY=GRASS\nFRICTION=0.5\nIS_VALID_TRACK=0\n";

    const RIGHT_SURFACES: &str = "[SURFACE_0]\nKEY=PIT_LANE\nFRICTION=0.85\nIS_PITLANE=1\nIS_VALID_TRACK=1\n\
        [SURFACE_1]\nKEY=ASPHALT\nFRICTION=0.95\nIS_VALID_TRACK=1\n\
        [SURFACE_2]\nKEY=KERB\nFRICTION=0.8\nIS_VALID_TRACK=1\n\
        [SURFACE_3]\nKEY=GRAVEL\nIS_VALID_TRACK=0\n";

    /// `alpha`: name, pitboxes, one layout, three surfaces (two valid).
    fn make_alpha(root: &TempDir) -> PathBuf {
        let dir = root.0.join("alpha");
        write(
            &dir.join("ui/ui_track.json"),
            r#"{"name":"Alpha Ring","city":"Turin","country":"Italy","length":"5000","pitboxes":"20"}"#,
        );
        write(&dir.join("gp/ui/ui_track.json"), "{}");
        write(&dir.join("data/surfaces.ini"), LEFT_SURFACES);
        dir
    }

    #[test]
    fn diff_rows_for_two_tracks() {
        let root = TempDir::new("rows");
        let left = make_alpha(&root);

        // `beta`: different name and pitboxes, two layouts, different surface set.
        let right = root.0.join("beta");
        write(
            &right.join("ui/ui_track.json"),
            r#"{"name":"Beta Ring","city":"Turin","country":"Italy","length":5000,"pitboxes":24}"#,
        );
        write(&right.join("gp/ui/ui_track.json"), "{}");
        write(&right.join("short/ui/ui_track.json"), "{}");
        write(&right.join("data/surfaces.ini"), RIGHT_SURFACES);

        let rows = compare_tracks(&left, &right).unwrap();
        assert_eq!(
            rows,
            vec![
                row("name", "Alpha Ring", "Beta Ring"),
                row("pitboxes", "20", "24"),
                row("layouts", "gp", "gp, short"),
                row("surfaces", "3", "4"),
                row("valid-track surfaces", "2", "3"),
                row("surfaces only on left", "GRASS", ""),
                row("surfaces only on right", "", "KERB, GRAVEL"),
            ]
        );

        assert_eq!(
            surface_changes(&left, &right).unwrap(),
            vec![
                "ASPHALT: friction 0.97 -> 0.95".to_string(),
                "GRASS: only in left".to_string(),
                "KERB: only in right".to_string(),
                "GRAVEL: only in right".to_string(),
            ]
        );
    }

    #[test]
    fn identical_tracks_have_no_rows() {
        let root = TempDir::new("same");
        let left = make_alpha(&root);
        assert_eq!(compare_tracks(&left, &left).unwrap(), Vec::new());
        assert_eq!(surface_changes(&left, &left).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn missing_surfaces_file_is_treated_as_empty() {
        let root = TempDir::new("missing");
        let left = make_alpha(&root);

        // `gamma` has ui metadata only: no data folder, no surfaces file, no layouts.
        let right = root.0.join("gamma");
        write(
            &right.join("ui/ui_track.json"),
            r#"{"name":"Gamma","city":"Turin","country":"Italy","length":5000}"#,
        );

        assert_eq!(
            compare_tracks(&left, &right).unwrap(),
            vec![
                row("name", "Alpha Ring", "Gamma"),
                row("pitboxes", "20", ""),
                row("layouts", "gp", ""),
                row("surfaces", "3", "0"),
                row("valid-track surfaces", "2", "0"),
                row("surfaces only on left", "PIT_LANE, ASPHALT, GRASS", ""),
            ]
        );
        assert_eq!(
            surface_changes(&left, &right).unwrap(),
            vec![
                "PIT_LANE: only in left".to_string(),
                "ASPHALT: only in left".to_string(),
                "GRASS: only in left".to_string(),
            ]
        );

        // A folder that does not exist at all has no metadata and no surfaces.
        let absent = root.0.join("does_not_exist");
        assert!(surface_changes(&absent, &absent).unwrap().is_empty());
        let rows = compare_tracks(&absent, &absent).unwrap();
        assert!(rows.is_empty());
    }
}
