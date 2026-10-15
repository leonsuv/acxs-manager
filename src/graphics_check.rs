//! Finds graphics proxy DLLs in an Assetto Corsa root folder and reports clashes
//! between them. A proxy DLL sits next to the game executable and is loaded in
//! place of a system graphics or text library, so two of them can fight over
//! the same calls.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The part a proxy DLL plays in the setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Text-layout proxy that loads the custom lighting patch.
    LightsPatch,
    /// Renderer injector for the post-processing runtime (hooks the graphics APIs).
    ReShade,
    /// Renderer injector that forwards Direct3D 9 calls to a path-tracing runtime.
    RemixRuntime,
    /// Graphics proxy name with no identifiable owner in this folder.
    Other,
}

/// A graphics proxy DLL found in the AC root folder.
#[derive(Debug, Clone)]
pub struct ProxyFile {
    /// File name as it appears on disk.
    pub name: String,
    pub role: Role,
    pub path: PathBuf,
    pub size: u64,
}

/// Text-layout proxy used by the lighting patch.
const PATCH_DLLS: &[&str] = &["dwrite.dll"];

/// Runtime libraries that a renderer injector loads.
const RENDERER_RUNTIME_DLLS: &[&str] = &["reshade64.dll", "reshade32.dll"];

/// Files written by the post-processing runtime's configuration.
const RENDERER_CONFIG_FILES: &[&str] = &["reshade.ini", "reshadepreset.ini"];

/// Graphics API entry points that a proxy can stand in for.
const API_PROXY_DLLS: &[&str] = &[
    "d3d9.dll",
    "d3d10.dll",
    "d3d10_1.dll",
    "d3d11.dll",
    "d3d12.dll",
    "dxgi.dll",
    "opengl32.dll",
    "ddraw.dll",
];

/// Sidecar folder that the path-tracing bridge installs next to the executable.
const REMIX_SIDECAR_DIR: &str = ".trex";

/// Lists the graphics proxy DLLs directly inside `ac_root`.
///
/// Only known proxy names are returned; other DLLs (game libraries, codecs,
/// middleware) are ignored. An API proxy name is attributed to a role using
/// sibling files: the bridge's sidecar folder claims the Direct3D 9 proxy, and
/// the renderer's runtime or config files claim the API proxies. Anything else
/// with a known name is `Role::Other`.
pub fn scan(ac_root: &Path) -> io::Result<Vec<ProxyFile>> {
    let mut dir_names: HashSet<String> = HashSet::new();
    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();

    for entry in fs::read_dir(ac_root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            dir_names.insert(name.to_ascii_lowercase());
        } else if path.is_file() {
            let size = fs::metadata(&path)?.len();
            files.push((name, path, size));
        }
    }

    let has_sidecar = dir_names.contains(REMIX_SIDECAR_DIR);
    let has_renderer_files = files.iter().any(|(name, _, _)| {
        let lower = name.to_ascii_lowercase();
        RENDERER_RUNTIME_DLLS.contains(&lower.as_str())
            || RENDERER_CONFIG_FILES.contains(&lower.as_str())
    });

    let mut found: Vec<ProxyFile> = files
        .into_iter()
        .filter_map(|(name, path, size)| {
            let role = classify(&name.to_ascii_lowercase(), has_sidecar, has_renderer_files)?;
            Some(ProxyFile {
                name,
                role,
                path,
                size,
            })
        })
        .collect();

    found.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(found)
}

/// Maps a lowercase file name to a role, or `None` if it is not a graphics proxy.
fn classify(lower: &str, has_sidecar: bool, has_renderer_files: bool) -> Option<Role> {
    if PATCH_DLLS.contains(&lower) {
        return Some(Role::LightsPatch);
    }
    if RENDERER_RUNTIME_DLLS.contains(&lower) {
        return Some(Role::ReShade);
    }
    if API_PROXY_DLLS.contains(&lower) {
        return Some(if lower == "d3d9.dll" && has_sidecar {
            Role::RemixRuntime
        } else if has_renderer_files {
            Role::ReShade
        } else {
            Role::Other
        });
    }
    None
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::LightsPatch => "lights patch",
        Role::ReShade => "ReShade",
        Role::RemixRuntime => "Remix runtime",
        Role::Other => "unidentified proxy",
    }
}

fn describe(files: &[&ProxyFile]) -> String {
    files
        .iter()
        .map(|f| format!("{} [{}]", f.path.display(), role_label(f.role)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Human-readable warnings about the proxies in `files`.
///
/// Reports three things: the same DLL name claimed by more than one file, a
/// proxy with no identifiable owner, and a ReShade install together with a
/// Remix runtime install (both hook the same graphics APIs).
pub fn conflicts(files: &[ProxyFile]) -> Vec<String> {
    let mut out = Vec::new();

    let mut by_name: BTreeMap<String, Vec<&ProxyFile>> = BTreeMap::new();
    for f in files {
        by_name
            .entry(f.name.to_ascii_lowercase())
            .or_default()
            .push(f);
    }
    for (name, group) in &by_name {
        if group.len() > 1 {
            out.push(format!(
                "{name} is claimed by {} files: {}. Only one copy can be loaded; remove the others.",
                group.len(),
                describe(group)
            ));
        }
    }

    for f in files.iter().filter(|f| f.role == Role::Other) {
        out.push(format!(
            "{} is present but not expected: it is a graphics proxy with no known owner and can intercept rendering calls. Remove it unless you installed it on purpose.",
            f.path.display()
        ));
    }

    let reshade: Vec<&ProxyFile> = files.iter().filter(|f| f.role == Role::ReShade).collect();
    let remix: Vec<&ProxyFile> = files
        .iter()
        .filter(|f| f.role == Role::RemixRuntime)
        .collect();
    if !reshade.is_empty() && !remix.is_empty() {
        out.push(format!(
            "ReShade and the Remix runtime are both installed: {} and {}. Both hook the same graphics APIs, so the game may crash or render incorrectly. Keep one renderer and remove the other.",
            describe(&reshade),
            describe(&remix)
        ));
    }

    out
}

/// Setup steps that avoid the clashes in `files`, tailored to the roles present.
pub fn recommended_order(files: &[ProxyFile]) -> Vec<String> {
    if files.is_empty() {
        return vec![
            "No graphics proxy DLLs found in the AC root. Nothing to order.".to_string(),
        ];
    }

    let has = |role: Role| files.iter().any(|f| f.role == role);
    let mut steps = vec!["Back up the Assetto Corsa root folder before changing any graphics DLLs.".to_string()];

    let unknown: Vec<String> = files
        .iter()
        .filter(|f| f.role == Role::Other)
        .map(|f| f.name.clone())
        .collect();
    if !unknown.is_empty() {
        steps.push(format!(
            "Remove the unidentified proxy DLLs: {}.",
            unknown.join(", ")
        ));
    }

    if has(Role::ReShade) && has(Role::RemixRuntime) {
        steps.push(
            "Pick one renderer: either keep the ReShade files and delete the Remix runtime files, or keep the Remix runtime files and delete the ReShade files. Never run both."
                .to_string(),
        );
    }

    if has(Role::LightsPatch) {
        steps.push(
            "Keep the lights patch DLL in place; it does not hook the renderer, so it can stay with either renderer."
                .to_string(),
        );
    }

    if has(Role::ReShade) {
        steps.push(
            "ReShade: keep its runtime DLL and its API proxy in the same folder, and set its configuration once rather than replacing the files."
                .to_string(),
        );
    }
    if has(Role::RemixRuntime) {
        steps.push(
            "Remix runtime: keep the bridge DLL and its sidecar folder together, next to the game executable."
                .to_string(),
        );
    }

    steps.push("Start the game once with only the chosen renderer enabled and check that it launches.".to_string());
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Temporary AC root that is removed when dropped.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acxs_graphics_check_{}_{}_{}",
                std::process::id(),
                tag,
                n
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp root");
            TempRoot(path)
        }

        fn file(&self, name: &str, bytes: usize) {
            fs::write(self.0.join(name), vec![0u8; bytes]).expect("write file");
        }

        fn dir(&self, name: &str) {
            fs::create_dir_all(self.0.join(name)).expect("create dir");
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn proxy(name: &str, role: Role) -> ProxyFile {
        ProxyFile {
            name: name.to_string(),
            role,
            path: PathBuf::from(format!("/ac/{name}")),
            size: 1,
        }
    }

    fn role_of(files: &[ProxyFile], name: &str) -> Role {
        files
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("{name} not found"))
            .role
    }

    #[test]
    fn scan_assigns_roles_for_each_setup() {
        let root = TempRoot::new("roles");
        root.file("dwrite.dll", 11);
        root.file("ReShade64.dll", 22);
        root.file("dxgi.dll", 33);
        root.file("d3d9.dll", 44);
        root.dir(".trex");

        let found = scan(root.path()).expect("scan");
        assert_eq!(found.len(), 4);
        assert_eq!(role_of(&found, "dwrite.dll"), Role::LightsPatch);
        assert_eq!(role_of(&found, "ReShade64.dll"), Role::ReShade);
        assert_eq!(role_of(&found, "dxgi.dll"), Role::ReShade);
        assert_eq!(role_of(&found, "d3d9.dll"), Role::RemixRuntime);

        let patch = found.iter().find(|f| f.name == "dwrite.dll").unwrap();
        assert_eq!(patch.size, 11);
        assert_eq!(patch.path, root.path().join("dwrite.dll"));
    }

    #[test]
    fn scan_marks_proxies_without_owner_as_other() {
        let root = TempRoot::new("other");
        root.file("ddraw.dll", 5);
        root.file("d3d9.dll", 6);

        let found = scan(root.path()).expect("scan");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|f| f.role == Role::Other));
    }

    #[test]
    fn scan_ignores_non_graphics_dlls() {
        let root = TempRoot::new("ignore");
        root.file("steam_api64.dll", 1);
        root.file("tbb.dll", 1);
        root.file("libopus.dll", 1);
        root.file("game.exe", 1);
        root.dir("d3d11.dll");

        let found = scan(root.path()).expect("scan");
        assert!(found.is_empty(), "unexpected entries: {found:?}");
    }

    #[test]
    fn scan_missing_root_is_an_error() {
        let root = TempRoot::new("missing");
        let missing = root.path().join("does_not_exist");
        assert!(scan(&missing).is_err());
    }

    #[test]
    fn conflicts_empty_for_single_patch() {
        let files = vec![proxy("dwrite.dll", Role::LightsPatch)];
        assert!(conflicts(&files).is_empty());
    }

    #[test]
    fn conflicts_reports_duplicate_names_with_paths() {
        let files = vec![
            ProxyFile {
                name: "dxgi.dll".into(),
                role: Role::ReShade,
                path: PathBuf::from("/ac/a/dxgi.dll"),
                size: 1,
            },
            ProxyFile {
                name: "DXGI.dll".into(),
                role: Role::Other,
                path: PathBuf::from("/ac/b/DXGI.dll"),
                size: 1,
            },
        ];
        let warnings = conflicts(&files);
        let dup = warnings
            .iter()
            .find(|w| w.contains("claimed by 2 files"))
            .expect("duplicate warning");
        assert!(dup.contains("/ac/a/dxgi.dll"));
        assert!(dup.contains("/ac/b/DXGI.dll"));
    }

    #[test]
    fn conflicts_reports_unexpected_proxy_by_path() {
        let files = vec![proxy("ddraw.dll", Role::Other)];
        let warnings = conflicts(&files);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("/ac/ddraw.dll"));
        assert!(warnings[0].contains("present but not expected"));
    }

    #[test]
    fn conflicts_warns_when_reshade_and_remix_both_installed() {
        let files = vec![
            proxy("ReShade64.dll", Role::ReShade),
            proxy("d3d9.dll", Role::RemixRuntime),
        ];
        let warnings = conflicts(&files);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("/ac/ReShade64.dll"));
        assert!(warnings[0].contains("/ac/d3d9.dll"));
        assert!(warnings[0].contains("both installed"));
    }

    #[test]
    fn recommended_order_for_empty_folder() {
        let steps = recommended_order(&[]);
        assert_eq!(steps.len(), 1);
        assert!(steps[0].contains("Nothing to order"));
    }

    #[test]
    fn recommended_order_resolves_renderer_clash() {
        let files = vec![
            proxy("dwrite.dll", Role::LightsPatch),
            proxy("ReShade64.dll", Role::ReShade),
            proxy("d3d9.dll", Role::RemixRuntime),
            proxy("ddraw.dll", Role::Other),
        ];
        let steps = recommended_order(&files);
        assert!(steps[0].starts_with("Back up"));
        assert!(steps.iter().any(|s| s.contains("Remove the unidentified proxy DLLs: ddraw.dll")));
        assert!(steps.iter().any(|s| s.contains("Pick one renderer")));
        assert!(steps.iter().any(|s| s.contains("Keep the lights patch DLL")));
        assert!(steps.last().unwrap().contains("Start the game once"));
    }

    #[test]
    fn recommended_order_without_clash_has_no_renderer_choice() {
        let files = vec![proxy("ReShade64.dll", Role::ReShade)];
        let steps = recommended_order(&files);
        assert!(!steps.iter().any(|s| s.contains("Pick one renderer")));
        assert!(steps.iter().any(|s| s.starts_with("ReShade:")));
    }
}
