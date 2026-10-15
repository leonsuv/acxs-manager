//! Switches the graphics injector that is active in an Assetto Corsa folder.
//!
//! Three kinds of injector can sit in the AC root: the CSP lights patch, a
//! ReShade runtime and the RTX Remix runtime. Only one should be active at a
//! time. [`switch`] removes the active injector through the owning module's
//! uninstall, which moves its files into that module's archive folder, and then
//! installs the target through the same module. If a step fails, the files moved
//! out earlier in the switch are put back. Nothing is deleted.

use crate::csp::{self, LightsPatch};
use crate::graphics_check::{self, ProxyFile, Role};
use crate::remix;
use crate::reshade;
use crate::settings::is_ac_root;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Runtime DLL that a Remix build folder provides.
const REMIX_RUNTIME_DLL: &str = "d3d9.dll";
/// Sub-folder of a Remix build folder that is searched before the folder itself.
const REMIX_SIDECAR_DIR: &str = ".trex";

/// The graphics setup to switch to. Each folder is the build the user picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Profile {
    /// No graphics injector at all.
    None,
    /// A lights-patch folder (`lights-patch-v*`, holding `dwrite.dll` and `extension/`).
    Csp(PathBuf),
    /// A ReShade build folder holding `ReShade64.dll`.
    ReShade(PathBuf),
    /// A Remix build folder holding `d3d9.dll`, also looked up in `.trex/`.
    Remix(PathBuf),
}

impl Profile {
    /// Name shown to the user.
    pub fn label(&self) -> &'static str {
        match self {
            Profile::None => "None",
            Profile::Csp(_) => "CSP lights patch",
            Profile::ReShade(_) => "ReShade",
            Profile::Remix(_) => "RTX Remix",
        }
    }

    /// The build folder this profile was made from, if it has one.
    pub fn build_folder(&self) -> Option<&Path> {
        match self {
            Profile::None => None,
            Profile::Csp(folder) | Profile::ReShade(folder) | Profile::Remix(folder) => Some(folder),
        }
    }
}

/// Graphics proxy files in the AC root and the clashes between them.
#[derive(Debug, Clone, Default)]
pub struct ActiveStatus {
    /// Proxy DLLs found in the AC root. Roles come from `graphics_check`, except
    /// that a file whose name a module installs is attributed to that module.
    pub proxies: Vec<ProxyFile>,
    /// Clashes between the proxies, from `graphics_check::conflicts`.
    pub warnings: Vec<String>,
}

impl ActiveStatus {
    /// Distinct roles present, in lights patch, ReShade, Remix, unidentified order.
    pub fn roles(&self) -> Vec<Role> {
        [Role::LightsPatch, Role::ReShade, Role::RemixRuntime, Role::Other]
            .into_iter()
            .filter(|role| self.proxies.iter().any(|p| p.role == *role))
            .collect()
    }

    /// Short description such as `ReShade` or `None`.
    pub fn label(&self) -> String {
        let roles = self.roles();
        if roles.is_empty() {
            return "None".to_string();
        }
        roles.into_iter().map(role_label).collect::<Vec<_>>().join(" + ")
    }
}

/// Name of a role as shown to the user.
pub fn role_label(role: Role) -> &'static str {
    match role {
        Role::LightsPatch => "CSP lights patch",
        Role::ReShade => "ReShade",
        Role::RemixRuntime => "RTX Remix",
        Role::Other => "unidentified proxy",
    }
}

/// Which stage of a switch failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Removing the injector that was active.
    Uninstall,
    /// Installing the target profile.
    Install,
}

impl Stage {
    fn describe(self) -> &'static str {
        match self {
            Stage::Uninstall => "Removing the active graphics profile",
            Stage::Install => "Installing the new graphics profile",
        }
    }
}

#[derive(Debug)]
pub enum ProfileError {
    GameRunning,
    NotAcRoot(PathBuf),
    /// The chosen build folder does not contain what its profile needs.
    InvalidBuild(String),
    /// Proxy DLLs with no known owner are present. Switching would leave them loaded.
    UnknownProxies(Vec<PathBuf>),
    /// Reading the AC root failed before anything was changed.
    Io(io::Error),
    /// A step failed. `rollback_issues` lists archived files that could not be
    /// put back; it is empty when the previous files are all back in place.
    Failed {
        stage: Stage,
        message: String,
        rollback_issues: Vec<String>,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::GameRunning => {
                write!(f, "Close Assetto Corsa before switching the graphics profile.")
            }
            ProfileError::NotAcRoot(path) => {
                write!(f, "{} is not an Assetto Corsa folder (acs.exe not found).", path.display())
            }
            ProfileError::InvalidBuild(message) => write!(f, "{message}."),
            ProfileError::UnknownProxies(paths) => {
                let list: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
                write!(
                    f,
                    "Graphics DLLs with no known owner are in the Assetto Corsa folder: {}. They would stay loaded, so remove them by hand before switching.",
                    list.join(", ")
                )
            }
            ProfileError::Io(e) => write!(f, "File error: {e}"),
            ProfileError::Failed { stage, message, rollback_issues } => {
                write!(f, "{} failed: {message}.", stage.describe())?;
                if rollback_issues.is_empty() {
                    write!(f, " The previous files were put back.")
                } else {
                    write!(f, " Some files could not be put back: {}.", rollback_issues.join("; "))
                }
            }
        }
    }
}

impl std::error::Error for ProfileError {}

impl From<io::Error> for ProfileError {
    fn from(e: io::Error) -> Self {
        ProfileError::Io(e)
    }
}

/// What a successful switch did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwitchReport {
    /// Injectors removed from the AC root, by role.
    pub removed: Vec<Role>,
    /// Archive folders that received files moved out by this switch.
    pub archived: Vec<PathBuf>,
    /// Names of the files the target profile placed in the AC root.
    pub installed: Vec<String>,
}

/// Reads the graphics proxies in the AC root and the warnings about them.
pub fn active(ac_root: &Path) -> io::Result<ActiveStatus> {
    let mut proxies = graphics_check::scan(ac_root)?;
    for proxy in &mut proxies {
        let role = owner_role(proxy);
        proxy.role = role;
    }
    let warnings = graphics_check::conflicts(&proxies);
    Ok(ActiveStatus { proxies, warnings })
}

/// Switches the AC root to `target`: removes the active injector, then installs
/// the target. Refused while the game runs.
pub fn switch(ac_root: &Path, target: &Profile, game_running: bool) -> Result<SwitchReport, ProfileError> {
    if game_running {
        return Err(ProfileError::GameRunning);
    }
    if !is_ac_root(ac_root) {
        return Err(ProfileError::NotAcRoot(ac_root.to_path_buf()));
    }
    let patch = check_build(target)?;

    let status = active(ac_root)?;
    let unknown: Vec<PathBuf> = status
        .proxies
        .iter()
        .filter(|p| p.role == Role::Other)
        .map(|p| p.path.clone())
        .collect();
    if !unknown.is_empty() {
        return Err(ProfileError::UnknownProxies(unknown));
    }

    let mut report = SwitchReport::default();
    if let Err(message) = remove_active(ac_root, &status.proxies, &mut report) {
        return Err(fail(ac_root, Stage::Uninstall, message, &report.archived));
    }
    if let Err(message) = ensure_clear(ac_root) {
        return Err(fail(ac_root, Stage::Uninstall, message, &report.archived));
    }

    match install_target(ac_root, target, patch.as_ref()) {
        Ok((names, archived)) => {
            report.installed = names;
            report.archived.extend(archived);
            Ok(report)
        }
        Err(message) => {
            // Take back whatever the failed install placed, then restore the old files.
            let mut issues = clear_partial(ac_root, target);
            issues.extend(restore(ac_root, &report.archived));
            Err(ProfileError::Failed {
                stage: Stage::Install,
                message,
                rollback_issues: issues,
            })
        }
    }
}

/// Role of a proxy file. A name that a module installs belongs to that module,
/// even when `graphics_check` has no sibling file to prove it.
fn owner_role(file: &ProxyFile) -> Role {
    if file.role != Role::Other {
        return file.role;
    }
    let lower = file.name.to_ascii_lowercase();
    if lower == REMIX_RUNTIME_DLL {
        Role::RemixRuntime
    } else if reshade::PROXY_NAMES.contains(&lower.as_str()) {
        Role::ReShade
    } else {
        Role::Other
    }
}

/// Confirms the build folder holds what the profile needs, before anything moves.
/// Returns the loaded lights patch for a CSP profile.
fn check_build(target: &Profile) -> Result<Option<LightsPatch>, ProfileError> {
    match target {
        Profile::None => Ok(None),
        Profile::Csp(folder) => match csp::load_patch(folder)? {
            Some(patch) => Ok(Some(patch)),
            None => Err(ProfileError::InvalidBuild(format!(
                "{} is not a complete lights-patch folder",
                folder.display()
            ))),
        },
        Profile::ReShade(build) => {
            if build.join(reshade::BUILD_MODULE).is_file() {
                Ok(None)
            } else {
                Err(ProfileError::InvalidBuild(format!(
                    "{} has no {}",
                    build.display(),
                    reshade::BUILD_MODULE
                )))
            }
        }
        Profile::Remix(build) => {
            let found = [
                build.join(REMIX_SIDECAR_DIR).join(REMIX_RUNTIME_DLL),
                build.join(REMIX_RUNTIME_DLL),
            ]
            .iter()
            .any(|path| path.is_file());
            if found {
                Ok(None)
            } else {
                Err(ProfileError::InvalidBuild(format!(
                    "{} has no {REMIX_RUNTIME_DLL}",
                    build.display()
                )))
            }
        }
    }
}

/// Removes every injector in `proxies` through the module that owns it. Files
/// moved out are recorded in `report`, so a later failure can put them back.
fn remove_active(ac_root: &Path, proxies: &[ProxyFile], report: &mut SwitchReport) -> Result<(), String> {
    let owned = |role: Role| proxies.iter().filter(|p| p.role == role).count();

    if owned(Role::LightsPatch) > 0 {
        let archived = csp::uninstall_patch(ac_root, false).map_err(|e| e.to_string())?;
        record(report, Role::LightsPatch, archived);
    }
    // ReShade's uninstall moves one proxy per call. Stop early when it finds
    // nothing it manages, which leaves the file to the final check.
    for _ in 0..owned(Role::ReShade) {
        match reshade::uninstall(ac_root, false).map_err(|e| e.to_string())? {
            Some(folder) => record(report, Role::ReShade, Some(folder)),
            None => break,
        }
    }
    if owned(Role::RemixRuntime) > 0 {
        let archived = remix::uninstall(ac_root, false).map_err(|e| e.to_string())?;
        record(report, Role::RemixRuntime, archived);
    }
    Ok(())
}

fn record(report: &mut SwitchReport, role: Role, archived: Option<PathBuf>) {
    if let Some(folder) = archived {
        if !report.removed.contains(&role) {
            report.removed.push(role);
        }
        report.archived.push(folder);
    }
}

/// Fails if any graphics proxy is still in the AC root after removal.
fn ensure_clear(ac_root: &Path) -> Result<(), String> {
    let left: Vec<String> = graphics_check::scan(ac_root)
        .map_err(|e| e.to_string())?
        .iter()
        .map(|p| p.path.display().to_string())
        .collect();
    if left.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "these files are still in the Assetto Corsa folder: {}. Remove them by hand, then switch again",
            left.join(", ")
        ))
    }
}

/// Installs the target through its module. Returns the file names placed and
/// any archive folder the module used.
fn install_target(
    ac_root: &Path,
    target: &Profile,
    patch: Option<&LightsPatch>,
) -> Result<(Vec<String>, Option<PathBuf>), String> {
    match target {
        Profile::None => Ok((Vec::new(), None)),
        Profile::Csp(_) => {
            let Some(patch) = patch else {
                return Err("the lights patch was not loaded".to_string());
            };
            let result = csp::install_patch(ac_root, patch, false).map_err(|e| e.to_string())?;
            Ok((vec![csp::DLL_NAME.to_string(), csp::EXTENSION_NAME.to_string()], result.archived_to))
        }
        Profile::ReShade(build) => {
            let report = reshade::install(ac_root, build, false).map_err(|e| e.to_string())?;
            Ok((vec![report.proxy_name], report.archived_to))
        }
        Profile::Remix(build) => {
            let report = remix::install(ac_root, build, false).map_err(|e| e.to_string())?;
            Ok((report.files, report.archived_to))
        }
    }
}

/// Takes back whatever a failed install left in the AC root, through the
/// target's own uninstall so that the files are archived, not deleted.
fn clear_partial(ac_root: &Path, target: &Profile) -> Vec<String> {
    let result: Result<(), String> = match target {
        Profile::None => Ok(()),
        Profile::Csp(_) => csp::uninstall_patch(ac_root, false).map(|_| ()).map_err(|e| e.to_string()),
        Profile::ReShade(_) => reshade::uninstall(ac_root, false).map(|_| ()).map_err(|e| e.to_string()),
        Profile::Remix(_) => remix::uninstall(ac_root, false).map(|_| ()).map_err(|e| e.to_string()),
    };
    result.err().map(|e| format!("cleanup of the failed install: {e}")).into_iter().collect()
}

/// Moves every file in the given archive folders back into the AC root.
/// Returns one line for each file that could not be put back.
fn restore(ac_root: &Path, archives: &[PathBuf]) -> Vec<String> {
    let mut issues = Vec::new();
    for folder in archives.iter().rev() {
        let entries = match fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(e) => {
                issues.push(format!("{}: {e}", folder.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let from = entry.path();
            let to = ac_root.join(entry.file_name());
            if to.exists() {
                issues.push(format!("{} was not put back, {} is already there", from.display(), to.display()));
                continue;
            }
            if let Err(e) = fs::rename(&from, &to) {
                issues.push(format!("{} was not put back: {e}", from.display()));
            }
        }
    }
    issues
}

/// Builds the error for a failed step and puts back the files it had moved out.
fn fail(ac_root: &Path, stage: Stage, message: String, archived: &[PathBuf]) -> ProfileError {
    ProfileError::Failed {
        stage,
        message,
        rollback_issues: restore(ac_root, archived),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// Scratch folder under the temp dir, removed on drop (also when a test panics).
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acxs-profiles-{}-{tag}-{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create scratch");
            Scratch(path)
        }

        /// A fake AC root: the folder with the game executable marker.
        fn ac_root(&self) -> PathBuf {
            let root = self.0.join("ac");
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join(crate::settings::AC_EXECUTABLE), b"exe").unwrap();
            root
        }

        fn reshade_build(&self, tag: &str) -> PathBuf {
            let dir = self.0.join(format!("reshade-{tag}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(reshade::BUILD_MODULE), format!("reshade {tag}")).unwrap();
            dir
        }

        fn remix_build(&self, tag: &str) -> PathBuf {
            let dir = self.0.join(format!("remix-{tag}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(REMIX_RUNTIME_DLL), format!("remix {tag}")).unwrap();
            dir
        }

        fn patch_folder(&self, version: &str) -> PathBuf {
            let dir = self.0.join(format!("lights-patch-v{version}"));
            let config = dir.join(csp::EXTENSION_NAME).join("config");
            fs::create_dir_all(&config).unwrap();
            fs::write(dir.join(csp::DLL_NAME), format!("patch {version}")).unwrap();
            fs::write(config.join("lights.ini"), "[main]").unwrap();
            dir
        }

        fn empty_dir(&self, name: &str) -> PathBuf {
            let dir = self.0.join(name);
            fs::create_dir_all(&dir).unwrap();
            dir
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn archives_in(ac: &Path) -> Vec<PathBuf> {
        fs::read_dir(ac.join(reshade::ARCHIVE_DIR))
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn reshade_switch_installs_and_reports() {
        let s = Scratch::new("install");
        let ac = s.ac_root();
        let report = switch(&ac, &Profile::ReShade(s.reshade_build("a")), false).unwrap();
        assert_eq!(report.installed, vec!["dxgi.dll".to_string()]);
        assert!(report.archived.is_empty());
        assert_eq!(fs::read(ac.join("dxgi.dll")).unwrap(), b"reshade a".to_vec());
        let status = active(&ac).unwrap();
        assert_eq!(status.roles(), vec![Role::ReShade]);
        assert_eq!(status.label(), "ReShade");
    }

    #[test]
    fn none_archives_the_active_injector() {
        let s = Scratch::new("none");
        let ac = s.ac_root();
        switch(&ac, &Profile::ReShade(s.reshade_build("a")), false).unwrap();
        let report = switch(&ac, &Profile::None, false).unwrap();
        assert_eq!(report.removed, vec![Role::ReShade]);
        assert!(report.installed.is_empty());
        assert!(!ac.join("dxgi.dll").exists());
        let archives = archives_in(&ac);
        assert_eq!(archives.len(), 1);
        assert_eq!(fs::read(archives[0].join("dxgi.dll")).unwrap(), b"reshade a".to_vec());
        assert!(active(&ac).unwrap().roles().is_empty());
    }

    #[test]
    fn renderer_switch_leaves_one_injector() {
        let s = Scratch::new("renderer");
        let ac = s.ac_root();
        switch(&ac, &Profile::ReShade(s.reshade_build("a")), false).unwrap();
        let report = switch(&ac, &Profile::Remix(s.remix_build("b")), false).unwrap();
        assert_eq!(report.removed, vec![Role::ReShade]);
        assert_eq!(report.installed, vec![REMIX_RUNTIME_DLL.to_string()]);
        assert!(!ac.join("dxgi.dll").exists());
        assert_eq!(fs::read(ac.join(REMIX_RUNTIME_DLL)).unwrap(), b"remix b".to_vec());
        // The Remix runtime is recognised without a sidecar folder.
        assert_eq!(active(&ac).unwrap().roles(), vec![Role::RemixRuntime]);
        // And it can be removed again by the next switch.
        switch(&ac, &Profile::None, false).unwrap();
        assert!(active(&ac).unwrap().roles().is_empty());
    }

    #[test]
    fn csp_patch_installs_and_archives_on_removal() {
        let s = Scratch::new("csp");
        let ac = s.ac_root();
        let patch = s.patch_folder("0.2.11");
        let report = switch(&ac, &Profile::Csp(patch), false).unwrap();
        assert_eq!(report.installed, vec!["dwrite.dll".to_string(), "extension".to_string()]);
        assert!(ac.join("dwrite.dll").is_file());
        assert!(ac.join("extension/config/lights.ini").is_file());
        assert_eq!(active(&ac).unwrap().roles(), vec![Role::LightsPatch]);

        let removed = switch(&ac, &Profile::None, false).unwrap();
        assert_eq!(removed.removed, vec![Role::LightsPatch]);
        assert!(!ac.join("dwrite.dll").exists());
        assert!(!ac.join("extension").exists());
        let archives = fs::read_dir(csp::archive_dir(&ac)).unwrap().flatten().collect::<Vec<_>>();
        assert_eq!(archives.len(), 1);
        assert!(archives[0].path().join("dwrite.dll").is_file());
        assert!(archives[0].path().join("extension").is_dir());
    }

    #[test]
    fn game_running_refuses_without_touching_files() {
        let s = Scratch::new("running");
        let ac = s.ac_root();
        switch(&ac, &Profile::ReShade(s.reshade_build("a")), false).unwrap();
        assert!(matches!(
            switch(&ac, &Profile::None, true),
            Err(ProfileError::GameRunning)
        ));
        assert!(ac.join("dxgi.dll").is_file());
        assert!(archives_in(&ac).is_empty());
    }

    #[test]
    fn invalid_build_is_refused_before_anything_moves() {
        let s = Scratch::new("invalid");
        let ac = s.ac_root();
        switch(&ac, &Profile::ReShade(s.reshade_build("a")), false).unwrap();
        let empty = s.empty_dir("empty-build");
        assert!(matches!(
            switch(&ac, &Profile::Remix(empty), false),
            Err(ProfileError::InvalidBuild(_))
        ));
        assert_eq!(fs::read(ac.join("dxgi.dll")).unwrap(), b"reshade a".to_vec());
        assert!(archives_in(&ac).is_empty());
    }

    #[test]
    fn unknown_proxy_is_refused() {
        let s = Scratch::new("unknown");
        let ac = s.ac_root();
        fs::write(ac.join("ddraw.dll"), b"unknown").unwrap();
        match switch(&ac, &Profile::None, false) {
            Err(ProfileError::UnknownProxies(paths)) => assert_eq!(paths, vec![ac.join("ddraw.dll")]),
            other => panic!("unexpected result: {other:?}"),
        }
        assert!(ac.join("ddraw.dll").is_file());
    }

    #[test]
    fn plain_folder_is_refused_and_clean_root_is_a_no_op() {
        let s = Scratch::new("plain");
        let plain = s.empty_dir("plain");
        assert!(matches!(
            switch(&plain, &Profile::None, false),
            Err(ProfileError::NotAcRoot(_))
        ));
        let ac = s.ac_root();
        let report = switch(&ac, &Profile::None, false).unwrap();
        assert_eq!(report, SwitchReport::default());
    }

    #[cfg(unix)]
    #[test]
    fn failed_install_puts_previous_files_back() {
        use std::os::unix::fs::PermissionsExt;

        let s = Scratch::new("rollback");
        let ac = s.ac_root();
        switch(&ac, &Profile::ReShade(s.reshade_build("good")), false).unwrap();

        // The module is listed but cannot be read, so the copy step fails.
        let bad = s.reshade_build("bad");
        fs::set_permissions(bad.join(reshade::BUILD_MODULE), fs::Permissions::from_mode(0o000)).unwrap();

        match switch(&ac, &Profile::ReShade(bad), false) {
            Err(ProfileError::Failed { stage: Stage::Install, rollback_issues, .. }) => {
                assert!(rollback_issues.is_empty(), "{rollback_issues:?}");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert_eq!(fs::read(ac.join("dxgi.dll")).unwrap(), b"reshade good".to_vec());
        assert_eq!(active(&ac).unwrap().roles(), vec![Role::ReShade]);
    }

    #[test]
    fn leftover_injector_fails_removal_and_restores_archived_files() {
        let s = Scratch::new("leftover");
        let ac = s.ac_root();
        switch(&ac, &Profile::ReShade(s.reshade_build("a")), false).unwrap();
        // A ReShade runtime that the ReShade module does not manage.
        fs::write(ac.join("ReShade64.dll"), b"manual").unwrap();

        match switch(&ac, &Profile::None, false) {
            Err(ProfileError::Failed { stage: Stage::Uninstall, rollback_issues, .. }) => {
                assert!(rollback_issues.is_empty(), "{rollback_issues:?}");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert_eq!(fs::read(ac.join("dxgi.dll")).unwrap(), b"reshade a".to_vec());
        assert!(ac.join("ReShade64.dll").is_file());
    }
}
