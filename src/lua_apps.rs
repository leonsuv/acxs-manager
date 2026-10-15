//! Lua apps: the apps that ship inside Custom Shaders Patch, and the user's own apps.
//!
//! Listing is read-only. Every app is a folder holding a `manifest.ini`; folders without one
//! are skipped.
//!
//! - CSP apps: `<AC root>/extension/internal/lua-apps/<AppName>/manifest.ini`
//! - user apps: `<AC root>/apps/lua/<AppName>/manifest.ini`
//!
//! Changes never delete anything. [`install_app`] copies a picked folder into the user
//! apps and [`remove_app`] moves an app out of its folder. Whatever gets replaced or removed
//! is moved to `<AC root>/acxs-app-archive/<stamp>/`. Both are refused while the game runs.
//!
//! Manifest keys read: `[ABOUT]` `NAME`, `VERSION`, `AUTHOR`, `DESCRIPTION`; the first
//! `[WINDOW_...]` `ICON` as a fallback icon. The app icon is `icon.png` in the app
//! folder. Also available: the window size and whether a `[SERVICE]` script runs in
//! the background, see [`read_manifest_runtime`].

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;

use crate::ini::Ini;
use crate::util::copy_dir_all;

const MANIFEST: &str = "manifest.ini";

#[derive(Debug, Clone, PartialEq)]
pub enum AppScope {
    /// Shipped inside Custom Shaders Patch.
    Csp,
    /// Installed by the user under `apps/lua`.
    User,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LuaApp {
    /// The folder name, which is also the app id used by Assetto Corsa.
    pub id: String,
    /// `[ABOUT] NAME`, or the folder name when the manifest has no name.
    pub name: String,
    /// `[ABOUT] VERSION`, `None` when missing or empty.
    pub version: Option<String>,
    /// `[ABOUT] AUTHOR`, `None` when missing or empty.
    pub author: Option<String>,
    /// `[ABOUT] DESCRIPTION`, `None` when missing or empty.
    pub description: Option<String>,
    /// `icon.png` in the app folder, else the first window's `ICON` if that file exists.
    pub icon: Option<PathBuf>,
    pub scope: AppScope,
    /// The app folder.
    pub path: PathBuf,
}

fn csp_apps_dir(ac_root: &Path) -> PathBuf {
    ac_root.join("extension").join("internal").join("lua-apps")
}

fn user_apps_dir(ac_root: &Path) -> PathBuf {
    ac_root.join("apps").join("lua")
}

/// Apps that ship with Custom Shaders Patch. Empty when the folder is missing.
pub fn list_csp_apps(ac_root: &Path) -> Vec<LuaApp> {
    list_in(&csp_apps_dir(ac_root), AppScope::Csp)
}

/// Apps installed by the user under `apps/lua`. Empty when the folder is missing.
pub fn list_user_apps(ac_root: &Path) -> Vec<LuaApp> {
    list_in(&user_apps_dir(ac_root), AppScope::User)
}

/// CSP and user apps together, sorted by name (case-insensitive).
pub fn list_all(ac_root: &Path) -> Vec<LuaApp> {
    let mut apps = list_csp_apps(ac_root);
    apps.extend(list_user_apps(ac_root));
    sort_apps(&mut apps);
    apps
}

/// Whether an app folder is enabled.
///
/// No enable/disable marker is known for Lua apps: there is no disabled folder and no
/// file or manifest key that turns an app off. So every app is reported as enabled,
/// whatever `app_dir` contains. Revisit this if a marker is found.
pub fn app_is_enabled(_app_dir: &Path) -> bool {
    true
}

fn list_in(dir: &Path, scope: AppScope) -> Vec<LuaApp> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut apps: Vec<LuaApp> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| read_app(&path, scope.clone()))
        .collect();
    sort_apps(&mut apps);
    apps
}

/// Reads one app folder. `None` when there is no readable manifest.
fn read_app(app_dir: &Path, scope: AppScope) -> Option<LuaApp> {
    let manifest = app_dir.join(MANIFEST);
    if !manifest.is_file() {
        return None;
    }
    let ini = Ini::load(&manifest).ok()?;
    let id = app_dir.file_name()?.to_string_lossy().into_owned();
    Some(LuaApp {
        name: about_value(&ini, "NAME").unwrap_or_else(|| id.clone()),
        version: about_value(&ini, "VERSION"),
        author: about_value(&ini, "AUTHOR"),
        description: about_value(&ini, "DESCRIPTION"),
        icon: icon_path(&ini, app_dir),
        id,
        scope,
        path: app_dir.to_path_buf(),
    })
}

/// A value from `[ABOUT]`, trimmed. Empty values count as missing.
fn about_value(ini: &Ini, key: &str) -> Option<String> {
    ini.get("ABOUT", key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn icon_path(ini: &Ini, app_dir: &Path) -> Option<PathBuf> {
    let own = app_dir.join("icon.png");
    if own.is_file() {
        return Some(own);
    }
    // The first [WINDOW_...] section is the only one `Ini` can address.
    let window_icon = ini.get("WINDOW_...", "ICON")?.trim();
    if window_icon.is_empty() {
        return None;
    }
    let path = app_dir.join(window_icon);
    path.is_file().then_some(path)
}

fn sort_apps(apps: &mut [LuaApp]) {
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.to_lowercase().cmp(&b.id.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
    });
}

/// What an app's manifest declares about its window and its background script.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestRuntime {
    /// `SIZE` of the first `[WINDOW_...]` section with a valid one, as (width, height).
    pub window_size: Option<(u32, u32)>,
    /// True when a `[SERVICE]` section has a non-empty `SCRIPT`: a script that runs in
    /// the background and has no window.
    pub background_script: bool,
}

/// Reads the window size and background-script declaration from `<app_dir>/manifest.ini`.
/// A missing manifest, or missing keys and sections, give the defaults (`None`, `false`).
/// Values that do not parse are skipped. Only a manifest that exists but cannot be read
/// is an error.
pub fn read_manifest_runtime(app_dir: &Path) -> io::Result<ManifestRuntime> {
    let text = match fs::read_to_string(app_dir.join(MANIFEST)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ManifestRuntime::default()),
        Err(e) => return Err(e),
    };
    Ok(runtime_from_text(&text))
}

/// Sections of a manifest in file order, repeated names included. Comments, blank lines
/// and key lines before the first section are skipped. Values lose their `;` comment.
fn sections_of(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            sections.push((name.trim().to_string(), Vec::new()));
            continue;
        }
        if let (Some((_, entries)), Some((key, value))) = (sections.last_mut(), line.split_once('=')) {
            let value = value.split(';').next().unwrap_or("").trim();
            entries.push((key.trim().to_string(), value.to_string()));
        }
    }
    sections
}

fn runtime_from_text(text: &str) -> ManifestRuntime {
    let sections = sections_of(text);
    let window_size = sections
        .iter()
        .filter(|(name, _)| name.to_ascii_uppercase().starts_with("WINDOW_"))
        .find_map(|(_, entries)| value_of(entries, "SIZE").and_then(parse_size));
    let background_script = sections
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("SERVICE"))
        .any(|(_, entries)| value_of(entries, "SCRIPT").is_some_and(|script| !script.is_empty()));
    ManifestRuntime { window_size, background_script }
}

fn value_of<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// `"240, 400"` gives `(240, 400)`. Anything other than exactly two whole numbers gives `None`.
fn parse_size(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split(',').map(|part| part.trim().parse::<u32>());
    let width = parts.next()?.ok()?;
    let height = parts.next()?.ok()?;
    parts.next().is_none().then_some((width, height))
}

/// Folder inside the AC root that receives every app folder this module moves out of the way.
/// Each operation gets its own stamped subfolder: `acxs-app-archive/<stamp>/<app id>`.
pub const ARCHIVE_DIR: &str = "acxs-app-archive";

/// Why an install or removal was refused or failed.
#[derive(Debug)]
pub enum AppError {
    /// Assetto Corsa is running, so apps are not changed.
    GameRunning,
    /// The name cannot be used as an app folder name.
    InvalidId(String),
    /// The picked folder cannot be installed. The text says why.
    InvalidSource(String),
    /// Installs only write into the user folder.
    NotUserScope,
    /// A CSP app was asked to be removed without `allow_csp`.
    CspAppProtected(String),
    /// No app folder with that id in the scope.
    NotFound(PathBuf),
    /// The source is the installed app, or lies inside it.
    SourceInsideTarget(PathBuf),
    Io(io::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::GameRunning => {
                write!(f, "Close Assetto Corsa before installing or removing apps.")
            }
            AppError::InvalidId(id) => write!(f, "{id:?} is not a valid app folder name"),
            AppError::InvalidSource(problem) => {
                write!(f, "the app folder cannot be installed: {problem}")
            }
            AppError::NotUserScope => write!(f, "apps can only be installed into apps/lua"),
            AppError::CspAppProtected(id) => write!(
                f,
                "{id} ships with Custom Shaders Patch and was not removed; removing CSP apps is not allowed here"
            ),
            AppError::NotFound(path) => write!(f, "{} does not exist", path.display()),
            AppError::SourceInsideTarget(path) => write!(
                f,
                "{} is the installed app or lies inside it; copy the app somewhere else first",
                path.display()
            ),
            AppError::Io(e) => write!(f, "file operation failed: {e}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for AppError {
    fn from(e: io::Error) -> Self {
        AppError::Io(e)
    }
}

/// Copy the app folder `source_dir` into `<AC root>/apps/lua/<folder name>` and return the new
/// app folder. The source must hold a `manifest.ini` with a non-empty `[ABOUT] NAME` at its top
/// level. An installed app with the same id is moved to the archive first. The source is left
/// untouched. Only [`AppScope::User`] can be installed.
pub fn install_app(
    ac_root: &Path,
    source_dir: &Path,
    scope: AppScope,
    game_running: bool,
) -> Result<PathBuf, AppError> {
    if game_running {
        return Err(AppError::GameRunning);
    }
    if scope != AppScope::User {
        return Err(AppError::NotUserScope);
    }
    if !source_dir.is_dir() {
        return Err(AppError::InvalidSource("it is not a folder".into()));
    }
    let app_id = source_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    validate_id(&app_id)?;
    check_manifest(source_dir)?;

    let target = user_apps_dir(ac_root).join(&app_id);
    if is_inside(source_dir, &target) {
        return Err(AppError::SourceInsideTarget(target));
    }
    fs::create_dir_all(user_apps_dir(ac_root))?;
    let archive_root = ac_root.join(ARCHIVE_DIR);
    fs::create_dir_all(&archive_root)?;
    let stamp = stamp();

    let archived = if is_present(&target) {
        Some(archive_app(ac_root, &stamp, &app_id, &target)?)
    } else {
        None
    };

    // Staging lives in the archive root, on the same filesystem, so the final rename is atomic.
    // It is unique per process and time, so it belongs to this call alone.
    let staging = archive_root.join(format!(".importing-{app_id}-{}-{stamp}", std::process::id()));
    let copied = copy_dir_all(source_dir, &staging).and_then(|()| fs::rename(&staging, &target));
    if let Err(err) = copied {
        let _ = fs::remove_dir_all(&staging);
        return Err(restore_archived(archived.as_deref(), &target, err));
    }
    Ok(target)
}

/// Move the app `app_id` in `scope` into the archive and return where it went. Nothing is
/// deleted. A CSP app is only moved when `allow_csp` is true.
pub fn remove_app(
    ac_root: &Path,
    app_id: &str,
    scope: AppScope,
    game_running: bool,
    allow_csp: bool,
) -> Result<PathBuf, AppError> {
    if game_running {
        return Err(AppError::GameRunning);
    }
    validate_id(app_id)?;
    if scope == AppScope::Csp && !allow_csp {
        return Err(AppError::CspAppProtected(app_id.to_string()));
    }
    let app_dir = scope_dir(ac_root, &scope).join(app_id);
    if !app_dir.is_dir() {
        return Err(AppError::NotFound(app_dir));
    }
    Ok(archive_app(ac_root, &stamp(), app_id, &app_dir)?)
}

fn scope_dir(ac_root: &Path, scope: &AppScope) -> PathBuf {
    match scope {
        AppScope::Csp => csp_apps_dir(ac_root),
        AppScope::User => user_apps_dir(ac_root),
    }
}

fn stamp() -> String {
    Local::now().format("%Y%m%d-%H%M%S%3f").to_string()
}

/// Rejects names that could leave the apps folder or are not valid on Windows.
fn validate_id(id: &str) -> Result<(), AppError> {
    let bad = id.is_empty()
        || id == "."
        || id == ".."
        || id.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        });
    if bad {
        Err(AppError::InvalidId(id.to_string()))
    } else {
        Ok(())
    }
}

/// The source needs a `manifest.ini` at its top level with a non-empty `[ABOUT] NAME`.
fn check_manifest(source_dir: &Path) -> Result<(), AppError> {
    let manifest = source_dir.join(MANIFEST);
    if !manifest.is_file() {
        return Err(AppError::InvalidSource(format!("there is no {MANIFEST} at its top level")));
    }
    let ini = Ini::load(&manifest)
        .map_err(|e| AppError::InvalidSource(format!("{MANIFEST} cannot be read: {e}")))?;
    if about_value(&ini, "NAME").is_none() {
        return Err(AppError::InvalidSource(format!("{MANIFEST} has no NAME under [ABOUT]")));
    }
    Ok(())
}

/// Move `app_dir` to `<AC root>/acxs-app-archive/<stamp>/<app_id>`. Never overwrites an earlier
/// archive: a taken name gets `-2`, `-3`, and so on.
fn archive_app(ac_root: &Path, stamp: &str, app_id: &str, app_dir: &Path) -> io::Result<PathBuf> {
    let dir = ac_root.join(ARCHIVE_DIR).join(stamp);
    fs::create_dir_all(&dir)?;
    let mut dest = dir.join(app_id);
    let mut n = 2;
    while is_present(&dest) {
        dest = dir.join(format!("{app_id}-{n}"));
        n += 1;
    }
    fs::rename(app_dir, &dest)?;
    Ok(dest)
}

/// After a failed install: put the archived app back and return the error to report.
fn restore_archived(archived: Option<&Path>, target: &Path, err: io::Error) -> AppError {
    let Some(from) = archived else {
        return AppError::Io(err);
    };
    match fs::rename(from, target) {
        Ok(()) => AppError::Io(err),
        Err(restore_err) => AppError::Io(io::Error::new(
            err.kind(),
            format!(
                "{err}; the old app could not be moved back from {} ({restore_err})",
                from.display()
            ),
        )),
    }
}

fn is_present(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// True when `source` is `target` or lies inside it. Copying in that case would archive the
/// source itself.
fn is_inside(source: &Path, target: &Path) -> bool {
    match (fs::canonicalize(source), fs::canonicalize(target)) {
        (Ok(source), Ok(target)) => source.starts_with(target),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp folder, removed again on drop (also when a test panics).
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> TempRoot {
            let path = std::env::temp_dir().join(format!("acxs-lua-apps-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempRoot(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn reads_full_csp_manifest() {
        let root = TempRoot::new("full");
        let dir = root.0.join("extension/internal/lua-apps/CspDebug");
        write(
            &dir.join(MANIFEST),
            "[ABOUT]\r\nNAME = CSP Debug\r\nAUTHOR = x4fab\r\nVERSION = 1.1\r\nDESCRIPTION = Show outlines.\r\n\r\n[WINDOW_...]\r\nID = main\r\nICON = icon.png\r\n",
        );
        write(&dir.join("icon.png"), "");

        let apps = list_csp_apps(&root.0);
        assert_eq!(apps.len(), 1);
        let app = &apps[0];
        assert_eq!(app.id, "CspDebug");
        assert_eq!(app.name, "CSP Debug");
        assert_eq!(app.version.as_deref(), Some("1.1"));
        assert_eq!(app.author.as_deref(), Some("x4fab"));
        assert_eq!(app.description.as_deref(), Some("Show outlines."));
        assert_eq!(app.icon, Some(dir.join("icon.png")));
        assert_eq!(app.scope, AppScope::Csp);
        assert_eq!(app.path, dir);
    }

    #[test]
    fn optional_keys_fall_back_to_none_and_folder_name() {
        let root = TempRoot::new("minimal");
        write(&root.0.join("apps/lua/Bare/manifest.ini"), "[ABOUT]\nVERSION =\nAUTHOR = \n");
        write(&root.0.join("apps/lua/Plain/manifest.ini"), "[ABOUT]\nNAME = Plain App\n");

        let apps = list_user_apps(&root.0);
        assert_eq!(apps.len(), 2);

        let bare = &apps[0];
        assert_eq!(bare.id, "Bare");
        assert_eq!(bare.name, "Bare");
        assert_eq!(bare.version, None);
        assert_eq!(bare.author, None);
        assert_eq!(bare.description, None);
        assert_eq!(bare.icon, None);
        assert_eq!(bare.scope, AppScope::User);

        let plain = &apps[1];
        assert_eq!(plain.name, "Plain App");
        assert_eq!(plain.version, None);
    }

    #[test]
    fn icon_falls_back_to_window_icon_only_when_the_file_exists() {
        let root = TempRoot::new("icon");
        let with = root.0.join("apps/lua/WithWindowIcon");
        write(&with.join(MANIFEST), "[ABOUT]\nNAME = A\n[WINDOW_...]\nICON = tool.png\n");
        write(&with.join("tool.png"), "");
        let without = root.0.join("apps/lua/MissingWindowIcon");
        write(&without.join(MANIFEST), "[ABOUT]\nNAME = B\n[WINDOW_...]\nICON = gone.png\n");

        let apps = list_user_apps(&root.0);
        let a = apps.iter().find(|app| app.id == "WithWindowIcon").unwrap();
        let b = apps.iter().find(|app| app.id == "MissingWindowIcon").unwrap();
        assert_eq!(a.icon, Some(with.join("tool.png")));
        assert_eq!(b.icon, None);
    }

    #[test]
    fn skips_folders_without_manifest_and_loose_files() {
        let root = TempRoot::new("skip");
        let dir = root.0.join("extension/internal/lua-apps");
        write(&dir.join("NoManifest/main.lua"), "-- no manifest here");
        write(&dir.join("Good/manifest.ini"), "[ABOUT]\nNAME = Good\n");
        write(&dir.join("readme.txt"), "not an app");

        let apps = list_csp_apps(&root.0);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "Good");
    }

    #[test]
    fn sorts_by_name_case_insensitively() {
        let root = TempRoot::new("sort");
        let dir = root.0.join("extension/internal/lua-apps");
        write(&dir.join("zeta/manifest.ini"), "[ABOUT]\nNAME = beta\n");
        write(&dir.join("mid/manifest.ini"), "[ABOUT]\nNAME = Alpha\n");
        write(&dir.join("last/manifest.ini"), "[ABOUT]\nNAME = gamma\n");

        let names: Vec<String> = list_csp_apps(&root.0).into_iter().map(|app| app.name).collect();
        assert_eq!(names, ["Alpha", "beta", "gamma"]);
    }

    #[test]
    fn missing_folders_give_empty_lists() {
        let root = TempRoot::new("empty");
        assert!(list_csp_apps(&root.0).is_empty());
        assert!(list_user_apps(&root.0).is_empty());
        assert!(list_all(&root.0).is_empty());

        let nowhere = root.0.join("does-not-exist");
        assert!(list_csp_apps(&nowhere).is_empty());
        assert!(list_user_apps(&nowhere).is_empty());
        assert!(list_all(&nowhere).is_empty());
    }

    #[test]
    fn list_all_combines_both_scopes_sorted_by_name() {
        let root = TempRoot::new("all");
        write(&root.0.join("extension/internal/lua-apps/Mumble/manifest.ini"), "[ABOUT]\nNAME = Mumble\n");
        write(&root.0.join("extension/internal/lua-apps/AppShelf/manifest.ini"), "[ABOUT]\nNAME = App Shelf\n");
        write(&root.0.join("apps/lua/Banana/manifest.ini"), "[ABOUT]\nNAME = banana\n");

        let all = list_all(&root.0);
        let order: Vec<(&str, AppScope)> = all.iter().map(|app| (app.name.as_str(), app.scope.clone())).collect();
        assert_eq!(
            order,
            [
                ("App Shelf", AppScope::Csp),
                ("banana", AppScope::User),
                ("Mumble", AppScope::Csp),
            ]
        );

        assert_eq!(list_csp_apps(&root.0).len(), 2);
        assert_eq!(list_user_apps(&root.0).len(), 1);
    }

    #[test]
    fn app_is_enabled_defaults_to_true() {
        let root = TempRoot::new("enabled");
        let dir = root.0.join("apps/lua/Anything");
        write(&dir.join(MANIFEST), "[ABOUT]\nNAME = Anything\n");
        assert!(app_is_enabled(&dir));
        assert!(app_is_enabled(&root.0.join("does-not-exist")));
    }

    #[test]
    fn manifest_runtime_reads_first_window_size_and_service_script() {
        let root = TempRoot::new("runtime");
        let dir = root.0.join("apps/lua/Tracer");
        write(
            &dir.join(MANIFEST),
            "\u{feff}[ABOUT]\r\nNAME = Tracer\r\n\r\n[CORE]\r\nLAZY = FULL  ; note\r\n\r\n[WINDOW_...]\r\nID = main\r\nSIZE = 240, 400\r\n\r\n[WINDOW_...]\r\nID = other\r\nSIZE = 10, 10\r\n\r\n[SERVICE]\r\nSCRIPT = Service\r\nPERIOD = 24h\r\n",
        );
        let info = read_manifest_runtime(&dir).unwrap();
        assert_eq!(info, ManifestRuntime { window_size: Some((240, 400)), background_script: true });
    }

    #[test]
    fn manifest_runtime_skips_bad_sizes_and_needs_a_service_script() {
        let root = TempRoot::new("runtime-bad");
        let dir = root.0.join("apps/lua/Odd");
        write(
            &dir.join(MANIFEST),
            "[WINDOW_...]\nSIZE = wide, 400\n[WINDOW_...]\nSIZE = 300, 200, 1\n[WINDOW_...]\nSIZE = 320,\n[WINDOW_...]\nSIZE = 500 , 260 ; spaced\n[CORE]\nSCRIPT = not_a_service\n[SERVICE]\nSCRIPT =\n",
        );
        let info = read_manifest_runtime(&dir).unwrap();
        assert_eq!(info, ManifestRuntime { window_size: Some((500, 260)), background_script: false });
    }

    #[test]
    fn manifest_runtime_defaults_when_missing_or_without_keys() {
        let root = TempRoot::new("runtime-missing");
        let missing = read_manifest_runtime(&root.0.join("nothing-here")).unwrap();
        assert_eq!(missing, ManifestRuntime::default());
        assert_eq!(missing.window_size, None);
        assert!(!missing.background_script);

        let bare = root.0.join("apps/lua/Bare");
        write(&bare.join(MANIFEST), "[ABOUT]\nNAME = Bare\n");
        assert_eq!(read_manifest_runtime(&bare).unwrap(), ManifestRuntime::default());
    }

    /// A picked app folder under `root` with a manifest and one script.
    fn picked_app(root: &TempRoot, name: &str, manifest: &str) -> PathBuf {
        let dir = root.0.join(name);
        write(&dir.join(MANIFEST), manifest);
        write(&dir.join("main.lua"), "-- script");
        dir
    }

    /// The stamped folders inside the archive, empty when there is none.
    fn archive_entries(ac_root: &Path) -> Vec<PathBuf> {
        fs::read_dir(ac_root.join(ARCHIVE_DIR))
            .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn install_copies_the_folder_and_keeps_the_source() {
        let ac = TempRoot::new("install-copy");
        let picked = TempRoot::new("install-copy-src");
        let source = picked_app(&picked, "Shelf", "[ABOUT]\nNAME = Shelf\n");

        let target = install_app(&ac.0, &source, AppScope::User, false).unwrap();
        assert_eq!(target, ac.0.join("apps/lua/Shelf"));
        assert!(target.join(MANIFEST).is_file());
        assert!(target.join("main.lua").is_file());
        assert!(source.join("main.lua").is_file());
        assert_eq!(list_user_apps(&ac.0).len(), 1);
    }

    #[test]
    fn install_archives_the_app_it_replaces() {
        let ac = TempRoot::new("install-replace");
        let picked = TempRoot::new("install-replace-src");
        write(&ac.0.join("apps/lua/Shelf").join(MANIFEST), "[ABOUT]\nNAME = Old\n");
        write(&ac.0.join("apps/lua/Shelf/old.txt"), "keep me");
        let source = picked_app(&picked, "Shelf", "[ABOUT]\nNAME = New\n");

        let target = install_app(&ac.0, &source, AppScope::User, false).unwrap();
        assert!(!target.join("old.txt").exists());
        assert_eq!(list_user_apps(&ac.0)[0].name, "New");

        let archives = archive_entries(&ac.0);
        assert_eq!(archives.len(), 1);
        assert!(archives[0].join("Shelf/old.txt").is_file());
    }

    #[test]
    fn install_refuses_a_folder_that_is_not_an_app() {
        let ac = TempRoot::new("install-bad");
        let picked = TempRoot::new("install-bad-src");
        let no_manifest = picked.0.join("NoManifest");
        write(&no_manifest.join("main.lua"), "-- script");
        let no_name = picked_app(&picked, "NoName", "[ABOUT]\nVERSION = 1\n");
        let blank_name = picked_app(&picked, "Blank", "[ABOUT]\nNAME =   \n");
        let loose_file = picked.0.join("loose.ini");
        write(&loose_file, "[ABOUT]\nNAME = Loose\n");

        for source in [&no_manifest, &no_name, &blank_name, &loose_file] {
            let err = install_app(&ac.0, source, AppScope::User, false).unwrap_err();
            assert!(matches!(err, AppError::InvalidSource(_)), "{source:?}: {err}");
        }
        assert!(!ac.0.join("apps").exists());
    }

    #[test]
    fn install_refuses_csp_scope_and_a_running_game() {
        let ac = TempRoot::new("install-refuse");
        let picked = TempRoot::new("install-refuse-src");
        let source = picked_app(&picked, "Shelf", "[ABOUT]\nNAME = Shelf\n");

        assert!(matches!(
            install_app(&ac.0, &source, AppScope::Csp, false),
            Err(AppError::NotUserScope)
        ));
        assert!(matches!(
            install_app(&ac.0, &source, AppScope::User, true),
            Err(AppError::GameRunning)
        ));
        assert!(!ac.0.join("apps").exists());
    }

    #[test]
    fn install_refuses_the_installed_app_as_its_own_source() {
        let ac = TempRoot::new("install-self");
        let installed = ac.0.join("apps/lua/Shelf");
        write(&installed.join(MANIFEST), "[ABOUT]\nNAME = Shelf\n");
        write(&installed.join("main.lua"), "-- script");

        let err = install_app(&ac.0, &installed, AppScope::User, false).unwrap_err();
        assert!(matches!(err, AppError::SourceInsideTarget(_)));
        assert!(installed.join("main.lua").is_file());
        assert!(archive_entries(&ac.0).is_empty());
    }

    #[test]
    fn remove_moves_the_user_app_to_the_archive() {
        let ac = TempRoot::new("remove-user");
        write(&ac.0.join("apps/lua/Gone").join(MANIFEST), "[ABOUT]\nNAME = Gone\n");
        write(&ac.0.join("apps/lua/Gone/main.lua"), "-- script");

        let archived = remove_app(&ac.0, "Gone", AppScope::User, false, false).unwrap();
        assert!(!ac.0.join("apps/lua/Gone").exists());
        assert!(archived.join("main.lua").is_file());
        assert!(archived.starts_with(ac.0.join(ARCHIVE_DIR)));
        assert_eq!(archive_entries(&ac.0), vec![archived.parent().unwrap().to_path_buf()]);
    }

    #[test]
    fn remove_refuses_csp_apps_unless_allowed() {
        let ac = TempRoot::new("remove-csp");
        let csp = ac.0.join("extension/internal/lua-apps/CspDebug");
        write(&csp.join(MANIFEST), "[ABOUT]\nNAME = CSP Debug\n");

        let err = remove_app(&ac.0, "CspDebug", AppScope::Csp, false, false).unwrap_err();
        assert!(matches!(err, AppError::CspAppProtected(_)));
        assert!(csp.join(MANIFEST).is_file());

        let archived = remove_app(&ac.0, "CspDebug", AppScope::Csp, false, true).unwrap();
        assert!(!csp.exists());
        assert!(archived.join(MANIFEST).is_file());
    }

    #[test]
    fn remove_refuses_missing_apps_a_running_game_and_bad_ids() {
        let ac = TempRoot::new("remove-refuse");
        write(&ac.0.join("apps/lua/Here").join(MANIFEST), "[ABOUT]\nNAME = Here\n");

        assert!(matches!(
            remove_app(&ac.0, "Here", AppScope::User, true, false),
            Err(AppError::GameRunning)
        ));
        assert!(matches!(
            remove_app(&ac.0, "Missing", AppScope::User, false, false),
            Err(AppError::NotFound(_))
        ));
        for id in ["", ".", "..", "a/b", "a\\b", "x:y"] {
            assert!(
                matches!(remove_app(&ac.0, id, AppScope::User, false, false), Err(AppError::InvalidId(_))),
                "{id:?}"
            );
        }
        assert!(ac.0.join("apps/lua/Here").join(MANIFEST).is_file());
        assert!(archive_entries(&ac.0).is_empty());
    }

    #[test]
    fn archive_never_overwrites_an_earlier_archive() {
        let ac = TempRoot::new("archive-unique");
        let stamp = "20260101-000000000";
        write(&ac.0.join(ARCHIVE_DIR).join(stamp).join("Shelf/old.txt"), "earlier");
        let app = ac.0.join("apps/lua/Shelf");
        write(&app.join("new.txt"), "later");

        let dest = archive_app(&ac.0, stamp, "Shelf", &app).unwrap();
        assert_eq!(dest, ac.0.join(ARCHIVE_DIR).join(stamp).join("Shelf-2"));
        assert!(dest.join("new.txt").is_file());
        assert!(ac.0.join(ARCHIVE_DIR).join(stamp).join("Shelf/old.txt").is_file());
    }
}
