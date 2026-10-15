//! Read-only diagnostics for installed Assetto Corsa cars and tracks.
//!
//! The checks cover what the game needs from installed content: how a folder is
//! recognised as a car or a track, which data files and KN5 models each one must
//! have, how layout folders are found, and where skin previews live.
//!
//! The checks never write. Paths that cannot be inspected are reported as issues, never panics.
//!
//! The only writes are the repairs from `plan_repairs` / `apply_repairs`. They create a missing
//! `ui/ui_car.json` or `ui/ui_track.json` and nothing else. Existing files are never rewritten,
//! so nothing is moved aside and nothing is deleted.

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use crate::ini::Ini;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    pub path: PathBuf,
    pub message: String,
}

impl Issue {
    fn new(severity: Severity, path: &Path, message: impl Into<String>) -> Issue {
        Issue {
            severity,
            path: path.to_path_buf(),
            message: message.into(),
        }
    }
}

/// `Some(true)` when the path exists as the wanted kind (folder or file), `Some(false)` when it is
/// absent or of the other kind, `None` when it cannot be inspected (an issue is pushed then).
fn probe(path: &Path, want_dir: bool, issues: &mut Vec<Issue>) -> Option<bool> {
    match fs::metadata(path) {
        Ok(meta) => Some(meta.is_dir() == want_dir),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Some(false),
        Err(e) => {
            issues.push(Issue::new(Severity::Error, path, format!("cannot access this path: {e}")));
            None
        }
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// All entries of a folder, or `None` (with an issue) when the folder cannot be listed.
fn read_entries(dir: &Path, issues: &mut Vec<Issue>) -> Option<Vec<PathBuf>> {
    match fs::read_dir(dir) {
        Ok(read) => Some(read.filter_map(|e| e.ok().map(|e| e.path())).collect()),
        Err(e) => {
            issues.push(Issue::new(Severity::Error, dir, format!("cannot read folder: {e}")));
            None
        }
    }
}

/// Visible subfolders of a folder, sorted. An unreadable folder is reported and yields nothing.
fn subfolders(dir: &Path, issues: &mut Vec<Issue>) -> Vec<PathBuf> {
    let Some(entries) = read_entries(dir, issues) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .into_iter()
        .filter(|p| p.is_dir() && !is_hidden(p))
        .collect();
    dirs.sort();
    dirs
}

fn has_section(ini: &Ini, name: &str) -> bool {
    ini.section_names().iter().any(|s| s.eq_ignore_ascii_case(name))
}

/// Checks that the content folder itself is a usable folder. Returns false when it is not.
fn check_root(dir: &Path, kind: &str, issues: &mut Vec<Issue>) -> bool {
    match fs::metadata(dir) {
        Ok(meta) if meta.is_dir() => true,
        Ok(_) => {
            issues.push(Issue::new(Severity::Error, dir, format!("{kind} path is not a folder")));
            false
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            issues.push(Issue::new(Severity::Error, dir, format!("{kind} folder does not exist")));
            false
        }
        Err(e) => {
            issues.push(Issue::new(Severity::Error, dir, format!("cannot access {kind} folder: {e}")));
            false
        }
    }
}

/// Diagnostics for one car folder (`content/cars/<id>`).
pub fn check_car(car_dir: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    if !check_root(car_dir, "car", &mut issues) {
        return issues;
    }

    // The content scanner only treats a folder as a car when ui/ui_car.json exists.
    let ui_car = car_dir.join("ui").join("ui_car.json");
    if probe(&ui_car, false, &mut issues) == Some(false) {
        issues.push(Issue::new(
            Severity::Warning,
            &ui_car,
            "ui/ui_car.json is missing; content tools will not recognise this folder as a car",
        ));
    }

    // Data is either an unpacked data/ folder or a packed data.acd.
    let data = car_dir.join("data");
    let packed = car_dir.join("data.acd");
    match probe(&data, true, &mut issues) {
        Some(false) => {
            if probe(&packed, false, &mut issues) == Some(false) {
                issues.push(Issue::new(
                    Severity::Error,
                    &data,
                    "data/ folder is missing (and there is no packed data.acd)",
                ));
            }
        }
        Some(true) => check_car_ini(&data.join("car.ini"), &mut issues),
        None => {}
    }

    // The car model is any *.kn5 in the folder root.
    if let Some(entries) = read_entries(car_dir, &mut issues) && !entries.iter().any(|p| p.is_file() && has_ext(p, "kn5")) {
        issues.push(Issue::new(
            Severity::Error,
            car_dir,
            "no car model (*.kn5) in the folder root",
        ));
    }

    let skins = car_dir.join("skins");
    match probe(&skins, true, &mut issues) {
        Some(false) => issues.push(Issue::new(Severity::Warning, &skins, "skins/ folder is missing")),
        Some(true) => {
            for skin in subfolders(&skins, &mut issues) {
                let jpg = skin.join("preview.jpg");
                let png = skin.join("preview.png");
                let has_preview = probe(&jpg, false, &mut issues) == Some(true)
                    || probe(&png, false, &mut issues) == Some(true);
                if !has_preview {
                    issues.push(Issue::new(
                        Severity::Info,
                        &skin,
                        format!("skin {} has no preview image (preview.jpg or preview.png)", name_of(&skin)),
                    ));
                }
            }
        }
        None => {}
    }

    issues
}

fn check_car_ini(path: &Path, issues: &mut Vec<Issue>) {
    match probe(path, false, issues) {
        Some(false) => issues.push(Issue::new(Severity::Error, path, "data/car.ini is missing")),
        Some(true) => match Ini::load(path) {
            Ok(ini) if has_section(&ini, "BASIC") => {}
            Ok(_) => issues.push(Issue::new(Severity::Error, path, "data/car.ini has no [BASIC] section")),
            Err(e) => issues.push(Issue::new(Severity::Error, path, format!("data/car.ini cannot be read: {e}"))),
        },
        None => {}
    }
}

/// Diagnostics for one track folder (`content/tracks/<id>`), including its layouts.
///
/// Layouts are the folders under `ui/` that hold a `ui_track.json`: each one is a layout, and its
/// data lives in `<track>/<layout>/`. Without a root `ui/ui_track.json` the main layout is one of
/// those, so the root `data/` is then not required.
pub fn check_track(track_dir: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    if !check_root(track_dir, "track", &mut issues) {
        return issues;
    }
    let track_id = name_of(track_dir);

    let ui_dir = track_dir.join("ui");
    let ui_present = probe(&ui_dir, true, &mut issues);
    let main_ui = ui_dir.join("ui_track.json");
    let has_main_ui = probe(&main_ui, false, &mut issues);

    let mut layouts: Vec<String> = Vec::new();
    if ui_present == Some(true) {
        for dir in subfolders(&ui_dir, &mut issues) {
            if probe(&dir.join("ui_track.json"), false, &mut issues) == Some(true) {
                layouts.push(name_of(&dir));
            }
        }
    }

    if has_main_ui == Some(false) && layouts.is_empty() {
        issues.push(Issue::new(
            Severity::Warning,
            &main_ui,
            "ui/ui_track.json is missing and no layout has one",
        ));
    }

    // Each unit is a set of data: (layout name, folder holding its data/ and ai/).
    let mut units: Vec<(Option<String>, PathBuf)> = Vec::new();
    if has_main_ui == Some(true) {
        units.push((None, track_dir.to_path_buf()));
    }
    for layout in &layouts {
        units.push((Some(layout.clone()), track_dir.join(layout)));
    }

    for (layout, base) in &units {
        check_track_unit(track_dir, &track_id, layout.as_deref(), base, &mut issues);
    }

    // A folder next to ui/ with data or ai in it is a layout folder. Without the matching
    // ui/<name>/ui_track.json the game does not list it.
    const RESERVED: [&str; 6] = ["ui", "data", "ai", "skins", "sfx", "texture"];
    for dir in subfolders(track_dir, &mut issues) {
        let name = name_of(&dir);
        if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let has_data = probe(&dir.join("data"), true, &mut issues) == Some(true);
        let has_ai = probe(&dir.join("ai"), true, &mut issues) == Some(true);
        if !(has_data || has_ai) {
            continue;
        }
        let ui_layout = ui_dir.join(&name);
        match probe(&ui_layout, true, &mut issues) {
            Some(false) => issues.push(Issue::new(
                Severity::Info,
                &dir,
                format!("layout folder {name} has no ui/{name} folder, so it is not listed as a layout"),
            )),
            Some(true) if probe(&ui_layout.join("ui_track.json"), false, &mut issues) == Some(false) => {
                issues.push(Issue::new(
                    Severity::Info,
                    &dir,
                    format!("layout folder {name} has no ui/{name}/ui_track.json, so it is not listed as a layout"),
                ));
            }
            _ => {}
        }
    }

    issues
}

fn check_track_unit(
    track_dir: &Path,
    track_id: &str,
    layout: Option<&str>,
    base: &Path,
    issues: &mut Vec<Issue>,
) {
    let who = match layout {
        None => "track".to_string(),
        Some(l) => format!("layout {l}"),
    };

    let data = base.join("data");
    match probe(&data, true, issues) {
        Some(false) => issues.push(Issue::new(Severity::Error, &data, format!("{who}: data/ folder is missing"))),
        Some(true) => {
            let surfaces = data.join("surfaces.ini");
            if probe(&surfaces, false, issues) == Some(false) {
                issues.push(Issue::new(Severity::Error, &surfaces, format!("{who}: data/surfaces.ini is missing")));
            }
        }
        None => {}
    }

    let fast_lane = base.join("ai").join("fast_lane.ai");
    if probe(&fast_lane, false, issues) == Some(false) {
        issues.push(Issue::new(Severity::Warning, &fast_lane, format!("{who}: ai/fast_lane.ai is missing")));
    }

    // Models are listed in models.ini (basic track or main layout) or models_<layout>.ini, and
    // their KN5 files sit in the track folder. Without models.ini, a basic track needs <id>.kn5.
    let (models_name, is_basic) = match layout {
        None => ("models.ini".to_string(), true),
        Some(l) => (format!("models_{l}.ini"), false),
    };
    let models = track_dir.join(&models_name);
    match probe(&models, false, issues) {
        Some(true) => match Ini::load(&models) {
            Ok(ini) => {
                for file in referenced_models(&ini) {
                    let kn5 = track_dir.join(&file);
                    if probe(&kn5, false, issues) == Some(false) {
                        issues.push(Issue::new(
                            Severity::Error,
                            &kn5,
                            format!("{who}: {models_name} references missing model {file}"),
                        ));
                    }
                }
            }
            Err(e) => issues.push(Issue::new(
                Severity::Error,
                &models,
                format!("{who}: {models_name} cannot be read: {e}"),
            )),
        },
        Some(false) if is_basic && !track_id.is_empty() => {
            let kn5_name = format!("{track_id}.kn5");
            let kn5 = track_dir.join(&kn5_name);
            if probe(&kn5, false, issues) == Some(false) {
                issues.push(Issue::new(
                    Severity::Error,
                    &kn5,
                    format!("track model {kn5_name} is missing (there is no models.ini)"),
                ));
            }
        }
        _ => {}
    }
}

/// KN5 file names from `[MODEL_n]` and `[DYNAMIC_OBJECT_n]` sections. Each series is counted up
/// from 0 and stops at the first missing number.
fn referenced_models(ini: &Ini) -> Vec<String> {
    let mut files = Vec::new();
    for prefix in ["MODEL", "DYNAMIC_OBJECT"] {
        for i in 0usize.. {
            let section = format!("{prefix}_{i}");
            if !has_section(ini, &section) {
                break;
            }
            if let Some(file) = ini.get(&section, "FILE").map(str::trim).filter(|f| !f.is_empty()) {
                files.push(file.to_string());
            }
        }
    }
    files
}

/// Runs `check` over every visible subfolder of `parent`, or reports a missing `parent`.
fn check_each(parent: &Path, label: &str, check: fn(&Path) -> Vec<Issue>, issues: &mut Vec<Issue>) {
    match probe(parent, true, issues) {
        Some(true) => {
            for dir in subfolders(parent, issues) {
                issues.extend(check(&dir));
            }
        }
        Some(false) => issues.push(Issue::new(
            Severity::Error,
            parent,
            format!("content/{label} folder is missing"),
        )),
        None => {}
    }
}

/// Runs the car and track checks over every folder under `ac_root/content/cars` and
/// `ac_root/content/tracks`.
pub fn check_all(ac_root: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    let content = ac_root.join("content");
    check_each(&content.join("cars"), "cars", check_car, &mut issues);
    check_each(&content.join("tracks"), "tracks", check_track, &mut issues);
    issues
}

/// Counts of `(errors, warnings, infos)`.
pub fn summary(issues: &[Issue]) -> (usize, usize, usize) {
    issues.iter().fold((0, 0, 0), |(e, w, i), issue| match issue.severity {
        Severity::Error => (e + 1, w, i),
        Severity::Warning => (e, w + 1, i),
        Severity::Info => (e, w, i + 1),
    })
}

/// What a repair does. Every action only creates a file that does not exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairAction {
    /// Creates a missing `ui/ui_car.json` or `ui/ui_track.json` holding only `{"name": "<folder>"}`.
    CreateMissingUiJson,
}

/// A fix proposed by `plan_repairs`. Nothing is written until it is passed to `apply_repairs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repair {
    pub description: String,
    /// The file the repair creates.
    pub path: PathBuf,
    pub action: RepairAction,
}

/// What `apply_repairs` did with each repair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairReport {
    /// Files that were created.
    pub created: Vec<PathBuf>,
    /// Left alone because something already exists at the path (a file, a folder or a link).
    pub skipped: Vec<PathBuf>,
    /// Refused as unsafe, or stopped by an I/O error, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

/// Repairs for the warnings `check_all` reports that have a safe fix. Nothing is written.
///
/// A missing `ui/ui_car.json` or `ui/ui_track.json` gets a repair. Other issues, such as a
/// missing car data folder, are only left in the report.
pub fn plan_repairs(ac_root: &Path) -> Vec<Repair> {
    let mut repairs = Vec::new();
    for issue in check_all(ac_root) {
        if issue.severity != Severity::Warning {
            continue;
        }
        let Ok(folder) = ui_json_folder(ac_root, &issue.path) else {
            continue;
        };
        // The checks also warn when a folder sits where the file would go. Only a truly
        // missing file is repaired.
        let missing = matches!(
            fs::symlink_metadata(&issue.path),
            Err(e) if e.kind() == io::ErrorKind::NotFound
        );
        if !missing {
            continue;
        }
        let shown = issue.path.strip_prefix(ac_root).unwrap_or(&issue.path);
        repairs.push(Repair {
            description: format!("Create {} with name \"{}\"", shown.display(), name_of(&folder)),
            path: issue.path.clone(),
            action: RepairAction::CreateMissingUiJson,
        });
    }
    repairs
}

/// Runs each repair. A file is only ever created, never replaced: if something already exists
/// at a repair's path it is skipped. Existing files are not rewritten, so none are archived.
/// Paths that are not a `ui_car.json` / `ui_track.json` inside an existing car or track folder
/// are refused, whatever the caller passes in.
pub fn apply_repairs(ac_root: &Path, repairs: &[Repair]) -> RepairReport {
    let mut report = RepairReport::default();
    for repair in repairs {
        let result = match repair.action {
            RepairAction::CreateMissingUiJson => create_ui_json(ac_root, &repair.path),
        };
        match result {
            Ok(()) => report.created.push(repair.path.clone()),
            Err(CreateError::Exists) => report.skipped.push(repair.path.clone()),
            Err(CreateError::Failed(reason)) => report.failed.push((repair.path.clone(), reason)),
        }
    }
    report
}

enum CreateError {
    Exists,
    Failed(String),
}

fn create_ui_json(ac_root: &Path, path: &Path) -> Result<(), CreateError> {
    let folder = ui_json_folder(ac_root, path).map_err(CreateError::Failed)?;
    let ui_dir = path
        .parent()
        .ok_or_else(|| CreateError::Failed("path has no folder".to_string()))?;
    fs::create_dir_all(ui_dir)
        .map_err(|e| CreateError::Failed(format!("cannot create the ui folder: {e}")))?;

    let value = serde_json::json!({ "name": name_of(&folder) });
    let text = serde_json::to_string_pretty(&value).map_err(|e| CreateError::Failed(e.to_string()))?;

    // `create_new` fails if anything is already there, including a folder or a dangling link,
    // so the write can never replace existing content.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => CreateError::Exists,
            _ => CreateError::Failed(format!("cannot create the file: {e}")),
        })?;
    file.write_all(format!("{text}\n").as_bytes())
        .map_err(|e| CreateError::Failed(format!("cannot write the file: {e}")))
}

/// The car or track folder that `path` is a UI file of, if `path` is exactly
/// `<ac_root>/content/cars/<id>/ui/ui_car.json` or `<ac_root>/content/tracks/<id>/ui/ui_track.json`
/// and `<id>` is an existing folder. Anything else is refused with the reason.
fn ui_json_folder(ac_root: &Path, path: &Path) -> Result<PathBuf, String> {
    let rel = path
        .strip_prefix(ac_root)
        .map_err(|_| "path is outside the Assetto Corsa folder".to_string())?;

    let mut parts: Vec<&str> = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or("path is not valid text")?),
            _ => return Err("path is not a plain path inside the Assetto Corsa folder".to_string()),
        }
    }

    let [top, kind, id, ui, file] = parts.as_slice() else {
        return Err("path is not a car or track ui file".to_string());
    };
    let expected_file = match *kind {
        "cars" => "ui_car.json",
        "tracks" => "ui_track.json",
        _ => return Err("path is not inside content/cars or content/tracks".to_string()),
    };
    if *top != "content" || *ui != "ui" || id.starts_with('.') || *file != expected_file {
        return Err(format!("path is not content/{kind}/<id>/ui/{expected_file}"));
    }

    let folder = ac_root.join("content").join(kind).join(id);
    match fs::metadata(&folder) {
        Ok(meta) if meta.is_dir() => Ok(folder),
        Ok(_) => Err("the car or track path is not a folder".to_string()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err("the car or track folder does not exist, so nothing is created".to_string())
        }
        Err(e) => Err(format!("cannot access the car or track folder: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary AC root, removed when dropped.
    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn fixture(tag: &str) -> TempRoot {
        let root = std::env::temp_dir().join(format!("acxs-content-repair-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        let cars = root.join("content").join("cars");
        // Healthy car.
        write(&cars.join("good").join("ui").join("ui_car.json"), "{}");
        write(&cars.join("good").join("data").join("car.ini"), "[BASIC]\nTOTALMASS=1000\n");
        write(&cars.join("good").join("good.kn5"), "");
        write(&cars.join("good").join("skins").join("red").join("preview.jpg"), "");
        // Broken car: no ui_car.json, car.ini without [BASIC], no kn5, skin without preview.
        write(&cars.join("bad").join("data").join("car.ini"), "[ENGINE]\nTYPE=V\n");
        fs::create_dir_all(cars.join("bad").join("skins").join("blue")).unwrap();

        let tracks = root.join("content").join("tracks");
        // Healthy track.
        write(&tracks.join("good").join("ui").join("ui_track.json"), "{}");
        write(
            &tracks.join("good").join("models.ini"),
            "[MODEL_0]\nFILE=good.kn5\n[DYNAMIC_OBJECT_0]\nFILE=tree.kn5\n",
        );
        write(&tracks.join("good").join("good.kn5"), "");
        write(&tracks.join("good").join("tree.kn5"), "");
        write(&tracks.join("good").join("data").join("surfaces.ini"), "");
        write(&tracks.join("good").join("ai").join("fast_lane.ai"), "");
        // Broken track: no data/, models.ini names a missing KN5, no AI lane, and a layout
        // folder with no ui/gp folder.
        write(&tracks.join("broken").join("ui").join("ui_track.json"), "{}");
        write(&tracks.join("broken").join("models.ini"), "[MODEL_0]\nFILE=missing.kn5\n");
        write(&tracks.join("broken").join("gp").join("data").join("surfaces.ini"), "");
        // Track with no ui folder at all.
        write(&tracks.join("noui").join("data").join("surfaces.ini"), "");

        TempRoot(root)
    }

    fn assert_issues(issues: &[Issue], expected: &[(Severity, &str)]) {
        assert_eq!(issues.len(), expected.len(), "unexpected issues: {issues:#?}");
        for (severity, text) in expected {
            assert!(
                issues.iter().any(|i| i.severity == *severity && i.message.contains(text)),
                "missing {severity:?} containing {text:?} in {issues:#?}"
            );
        }
    }

    #[test]
    fn healthy_car_has_no_issues() {
        let root = fixture("car-ok");
        let issues = check_car(&root.0.join("content/cars/good"));
        assert_issues(&issues, &[]);
    }

    #[test]
    fn broken_car_reports_each_problem() {
        let root = fixture("car-bad");
        let issues = check_car(&root.0.join("content/cars/bad"));
        assert_issues(
            &issues,
            &[
                (Severity::Warning, "ui/ui_car.json is missing"),
                (Severity::Error, "data/car.ini has no [BASIC] section"),
                (Severity::Error, "no car model (*.kn5)"),
                (Severity::Info, "skin blue has no preview image"),
            ],
        );
        assert_eq!(summary(&issues), (2, 1, 1));
    }

    #[test]
    fn healthy_track_has_no_issues() {
        let root = fixture("track-ok");
        let issues = check_track(&root.0.join("content/tracks/good"));
        assert_issues(&issues, &[]);
    }

    #[test]
    fn broken_track_reports_each_problem() {
        let root = fixture("track-bad");
        let issues = check_track(&root.0.join("content/tracks/broken"));
        assert_issues(
            &issues,
            &[
                (Severity::Error, "track: data/ folder is missing"),
                (Severity::Error, "references missing model missing.kn5"),
                (Severity::Warning, "track: ai/fast_lane.ai is missing"),
                (Severity::Info, "layout folder gp has no ui/gp folder"),
            ],
        );
        assert_eq!(summary(&issues), (2, 1, 1));

        let no_ui = check_track(&root.0.join("content/tracks/noui"));
        assert_issues(&no_ui, &[(Severity::Warning, "ui/ui_track.json is missing")]);
    }

    #[test]
    fn check_all_counts_every_folder() {
        let root = fixture("all");
        let issues = check_all(&root.0);
        assert_eq!(summary(&issues), (4, 3, 2));
        assert_eq!(issues.len(), 9);
    }

    /// A temporary AC root that is not created yet, removed when dropped.
    fn fresh_root(tag: &str) -> TempRoot {
        let root = std::env::temp_dir().join(format!("acxs-content-repair-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        TempRoot(root)
    }

    /// Cars and tracks without their UI file, a car without a data folder, and a track whose
    /// layout has a ui_track.json (so the main UI file is not needed there).
    fn repair_fixture(tag: &str) -> TempRoot {
        let root = fresh_root(tag);
        let cars = root.0.join("content").join("cars");
        write(&cars.join("bare").join("data").join("car.ini"), "[BASIC]\nTOTALMASS=1000\n");
        write(&cars.join("bare").join("bare.kn5"), "");
        write(&cars.join("bare").join("skins").join("red").join("preview.jpg"), "");
        write(&cars.join("nodata").join("ui").join("ui_car.json"), "{}");
        write(&cars.join("nodata").join("nodata.kn5"), "");

        let tracks = root.0.join("content").join("tracks");
        write(&tracks.join("bare_track").join("data").join("surfaces.ini"), "");
        write(&tracks.join("bare_track").join("ai").join("fast_lane.ai"), "");
        write(&tracks.join("layouted").join("ui").join("gp").join("ui_track.json"), "{}");
        write(&tracks.join("layouted").join("gp").join("data").join("surfaces.ini"), "");
        write(&tracks.join("layouted").join("gp").join("ai").join("fast_lane.ai"), "");
        root
    }

    #[test]
    fn plan_covers_only_missing_ui_json() {
        let root = repair_fixture("plan");
        let repairs = plan_repairs(&root.0);

        let paths: Vec<PathBuf> = repairs.iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths.len(), 2, "{repairs:#?}");
        assert!(paths.contains(&root.0.join("content/cars/bare/ui/ui_car.json")));
        assert!(paths.contains(&root.0.join("content/tracks/bare_track/ui/ui_track.json")));
        assert!(repairs.iter().all(|r| r.action == RepairAction::CreateMissingUiJson));
        assert!(repairs.iter().any(|r| r.description.contains("\"bare_track\"")));

        // The car without a data folder is reported by the checks, but nothing is planned for it.
        assert!(check_all(&root.0).iter().any(|i| i.message.contains("data/ folder is missing")));
        assert!(!root.0.join("content/cars/nodata/data").exists());
        assert!(plan_repairs(&root.0).iter().all(|r| !r.path.starts_with(root.0.join("content/cars/nodata"))));
    }

    #[test]
    fn create_missing_ui_json_names_it_after_the_folder() {
        let root = repair_fixture("create");
        let repairs = plan_repairs(&root.0);
        let report = apply_repairs(&root.0, &repairs);

        assert_eq!(report.created.len(), 2, "{report:#?}");
        assert!(report.skipped.is_empty());
        assert!(report.failed.is_empty());
        for (path, name) in [
            (root.0.join("content/cars/bare/ui/ui_car.json"), "bare"),
            (root.0.join("content/tracks/bare_track/ui/ui_track.json"), "bare_track"),
        ] {
            let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(value["name"], name);
            assert_eq!(value.as_object().unwrap().len(), 1);
        }

        // Once created, the checks no longer ask for those files and nothing is left to plan.
        let after = check_all(&root.0);
        assert!(!after.iter().any(|i| i.message.contains("ui_car.json is missing")));
        assert!(!after.iter().any(|i| i.message.contains("ui_track.json is missing")));
        assert!(plan_repairs(&root.0).is_empty());
    }

    #[test]
    fn apply_never_overwrites_existing_files() {
        let root = repair_fixture("overwrite");
        let repairs = plan_repairs(&root.0);
        assert_eq!(repairs.len(), 2);

        // Created after the plan was made: a real file for the car.
        let car_json = root.0.join("content/cars/bare/ui/ui_car.json");
        write(&car_json, "{\"name\": \"Mine\"}");
        // A folder sits where the track file would go.
        let track_json = root.0.join("content/tracks/bare_track/ui/ui_track.json");
        fs::create_dir_all(&track_json).unwrap();

        let report = apply_repairs(&root.0, &repairs);
        assert!(report.created.is_empty(), "{report:#?}");
        assert_eq!(report.skipped, vec![car_json.clone(), track_json.clone()]);
        assert!(report.failed.is_empty(), "{report:#?}");
        assert_eq!(fs::read_to_string(&car_json).unwrap(), "{\"name\": \"Mine\"}");
        assert!(track_json.is_dir());
    }

    #[test]
    fn apply_refuses_paths_that_are_not_a_car_or_track_ui_file() {
        let root = repair_fixture("unsafe");
        let cars = root.0.join("content").join("cars");
        let outside = std::env::temp_dir().join(format!("acxs-content-repair-unsafe-outside-{}", std::process::id()));
        let repairs: Vec<Repair> = [
            cars.join("bare/ui/../../../escape/ui_car.json"),
            cars.join("bare/ui/other.json"),
            root.0.join("content/tracks/bare_track/ui/ui_car.json"),
            cars.join("ghost/ui/ui_car.json"),
            outside.join("ui_car.json"),
        ]
        .into_iter()
        .map(|path| Repair {
            description: "unsafe".to_string(),
            path,
            action: RepairAction::CreateMissingUiJson,
        })
        .collect();

        let report = apply_repairs(&root.0, &repairs);
        assert!(report.created.is_empty(), "{report:#?}");
        assert!(report.skipped.is_empty(), "{report:#?}");
        assert_eq!(report.failed.len(), repairs.len(), "{report:#?}");
        assert!(!root.0.join("content/escape").exists());
        assert!(!cars.join("ghost").exists());
        assert!(!outside.exists());
    }

    #[test]
    fn missing_root_plans_and_applies_nothing() {
        let root = fresh_root("repair-none");
        assert!(plan_repairs(&root.0).is_empty());
        assert_eq!(apply_repairs(&root.0, &[]), RepairReport::default());
        assert!(!root.0.exists());
    }

    #[test]
    fn missing_paths_are_reported_not_panicked() {
        let root = fixture("missing");
        let absent = root.0.join("does-not-exist");

        assert_eq!(summary(&check_car(&absent)), (1, 0, 0));
        assert_eq!(summary(&check_track(&absent)), (1, 0, 0));
        assert_eq!(summary(&check_all(&absent)), (2, 0, 0));
    }
}
