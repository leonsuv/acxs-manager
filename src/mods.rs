//! Enable and disable installed cars and tracks without deleting anything.
//!
//! Disabling moves `content/<cars|tracks>/<id>` into `acxs-disabled/<cars|tracks>/<id>`
//! inside the AC root. Enabling moves it back. A move is refused when the destination
//! already exists, so nothing is ever overwritten, and every operation is refused while
//! the game is running (the caller passes `game_running`, as `csp.rs` does).
//!
//! Content is never destroyed: a move only renames the folder, and an existing target is
//! refused rather than replaced. Some mod managers copy files into the game root and
//! record them in an install log. Here the whole content folder is the unit, so the
//! folder being moved is itself the state.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Folder inside the AC root that holds disabled content.
pub const DISABLED_DIR: &str = "acxs-disabled";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentKind {
    Car,
    Track,
}

impl ContentKind {
    /// Folder name under `content/` and under the disabled store.
    fn folder(self) -> &'static str {
        match self {
            ContentKind::Car => "cars",
            ContentKind::Track => "tracks",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisabledItem {
    pub kind: ContentKind,
    pub id: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum ModError {
    GameRunning,
    InvalidId(String),
    NotInstalled(PathBuf),
    AlreadyExists(PathBuf),
    /// A JSGME mod cannot be disabled because later-enabled mods overwrote some of its files.
    BlockedByDependent {
        mod_name: String,
        dependents: Vec<String>,
        files: Vec<String>,
    },
    /// A JSGME mod contains an unsafe path or symbolic link.
    UnsafeModEntry(String),
    Io(io::Error),
}

impl fmt::Display for ModError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModError::GameRunning => {
                write!(f, "Close Assetto Corsa before enabling or disabling content.")
            }
            ModError::InvalidId(id) => write!(f, "{id:?} is not a valid content id"),
            ModError::NotInstalled(p) => write!(f, "{} does not exist", p.display()),
            ModError::AlreadyExists(p) => {
                write!(f, "{} already exists, nothing was moved", p.display())
            }
            ModError::BlockedByDependent {
                mod_name,
                dependents,
                files,
            } => write!(
                f,
                "cannot disable {mod_name:?} because later mod(s) ({}) overwrote {} file(s); disable them first",
                dependents.join(", "),
                files.len()
            ),
            ModError::UnsafeModEntry(entry) => write!(f, "refusing unsafe mod entry {entry:?}"),
            ModError::Io(e) => write!(f, "file operation failed: {e}"),
        }
    }
}

impl std::error::Error for ModError {}

impl From<io::Error> for ModError {
    fn from(e: io::Error) -> Self {
        ModError::Io(e)
    }
}

/// Reject ids that could escape `content/<kind>/` or the disabled store.
pub fn validate_id(id: &str) -> Result<(), ModError> {
    let bad = id.is_empty()
        || id == "."
        || id == ".."
        || id.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0'));
    if bad {
        Err(ModError::InvalidId(id.to_string()))
    } else {
        Ok(())
    }
}

/// `<AC root>/acxs-disabled/`, where disabled content is moved.
pub fn disabled_root(ac_root: &Path) -> PathBuf {
    ac_root.join(DISABLED_DIR)
}

fn live_dir(ac_root: &Path, kind: ContentKind, id: &str) -> PathBuf {
    ac_root.join("content").join(kind.folder()).join(id)
}

fn stored_dir(ac_root: &Path, kind: ContentKind, id: &str) -> PathBuf {
    disabled_root(ac_root).join(kind.folder()).join(id)
}

/// Move `from` to `to`. Refuses if `to` exists (including a dangling symlink).
/// The existence check is not atomic with the rename, so a concurrent writer
/// could still race it; the game is required to be closed for this reason.
fn move_dir_no_overwrite(from: &Path, to: &Path) -> Result<PathBuf, ModError> {
    match fs::symlink_metadata(from) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(ModError::NotInstalled(from.to_path_buf()));
        }
        Err(e) => return Err(ModError::Io(e)),
    }
    if fs::symlink_metadata(to).is_ok() {
        return Err(ModError::AlreadyExists(to.to_path_buf()));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(from, to)?;
    Ok(to.to_path_buf())
}

fn move_item(
    ac_root: &Path,
    kind: ContentKind,
    id: &str,
    game_running: bool,
    to_disabled: bool,
) -> Result<PathBuf, ModError> {
    if game_running {
        return Err(ModError::GameRunning);
    }
    validate_id(id)?;
    let live = live_dir(ac_root, kind, id);
    let stored = stored_dir(ac_root, kind, id);
    if to_disabled {
        move_dir_no_overwrite(&live, &stored)
    } else {
        move_dir_no_overwrite(&stored, &live)
    }
}

/// Move `content/cars/<car_id>` into the disabled store. Returns the new location.
pub fn disable_car(ac_root: &Path, car_id: &str, game_running: bool) -> Result<PathBuf, ModError> {
    move_item(ac_root, ContentKind::Car, car_id, game_running, true)
}

/// Move `content/tracks/<track_id>` into the disabled store. Returns the new location.
pub fn disable_track(
    ac_root: &Path,
    track_id: &str,
    game_running: bool,
) -> Result<PathBuf, ModError> {
    move_item(ac_root, ContentKind::Track, track_id, game_running, true)
}

/// Move a disabled car back to `content/cars/<car_id>`. Returns the restored location.
pub fn enable_car(ac_root: &Path, car_id: &str, game_running: bool) -> Result<PathBuf, ModError> {
    move_item(ac_root, ContentKind::Car, car_id, game_running, false)
}

/// Move a disabled track back to `content/tracks/<track_id>`. Returns the restored location.
pub fn enable_track(
    ac_root: &Path,
    track_id: &str,
    game_running: bool,
) -> Result<PathBuf, ModError> {
    move_item(ac_root, ContentKind::Track, track_id, game_running, false)
}

/// Everything currently in the disabled store, cars first, each sorted by id.
/// A missing store or unreadable folder yields an empty list.
pub fn list_disabled(ac_root: &Path) -> Vec<DisabledItem> {
    let mut items = Vec::new();
    for kind in [ContentKind::Car, ContentKind::Track] {
        let Ok(entries) = fs::read_dir(disabled_root(ac_root).join(kind.folder())) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(id) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let path = entry.path();
            if validate_id(&id).is_err() || !path.is_dir() {
                continue;
            }
            items.push(DisabledItem { kind, id, path });
        }
    }
    items.sort_by(|a, b| {
        a.kind
            .folder()
            .cmp(b.kind.folder())
            .then_with(|| a.id.cmp(&b.id))
    });
    items
}

/// A disabled item with the total size of its files.
#[derive(Debug, Clone, PartialEq)]
pub struct DisabledItemSize {
    pub item: DisabledItem,
    /// Sum of the sizes of every regular file under the item's folder, in bytes.
    /// Symbolic links are not followed and do not count.
    pub bytes: u64,
}

/// Every disabled item (same order as [`list_disabled`]) with its size on disk.
/// Only metadata is read, so no file is moved, changed or created. A missing store gives
/// an empty list. An unreadable file or folder inside an item is an error, so a size is
/// never reported short.
pub fn list_disabled_sizes(ac_root: &Path) -> io::Result<Vec<DisabledItemSize>> {
    list_disabled(ac_root)
        .into_iter()
        .map(|item| {
            let meta = fs::symlink_metadata(&item.path)?;
            let bytes = if meta.is_dir() { tree_bytes(&item.path)? } else { 0 };
            Ok(DisabledItemSize { item, bytes })
        })
        .collect()
}

/// Total size of the regular files under `root`, without following symbolic links.
fn tree_bytes(root: &Path) -> io::Result<u64> {
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// JSGME-style generic mod manager (`MODS/` overlay with backup & rollback)
// ---------------------------------------------------------------------------

/// Default folder inside the AC root holding JSGME mods.
pub const JSGME_MODS_DIR: &str = "MODS";
/// Folder inside the AC root holding backed-up stock/prior files displaced by JSGME mods.
pub const JSGME_BACKUP_DIR: &str = "acxs-jsgme-backup";
/// Persistent JSON state file inside the AC root recording load order and installed files.
pub const JSGME_STATE_FILE: &str = "acxs-jsgme-state.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JsgmeState {
    pub version: u32,
    /// Enabled mods in exact activation (load) order.
    pub enabled: Vec<EnabledMod>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnabledMod {
    pub name: String,
    pub enabled_at: String,
    /// Forward-slash relative paths under `ac_root` installed by this mod.
    pub files: Vec<String>,
    /// Subset of `files` that existed prior to activation and were moved to `acxs-jsgme-backup/<name>/`.
    pub backed_up: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModConflict {
    pub rel_path: String,
    pub existing_mod: String,
}

/// Resolves the JSGME mods directory (`custom_mods_dir` or `<ac_root>/MODS`).
pub fn jsgme_mods_dir(ac_root: &Path, custom_mods_dir: Option<&Path>) -> PathBuf {
    match custom_mods_dir {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => ac_root.join(JSGME_MODS_DIR),
    }
}

/// Loads `<ac_root>/acxs-jsgme-state.json`. Missing file returns an empty v1 state.
pub fn load_jsgme_state(ac_root: &Path) -> io::Result<JsgmeState> {
    let path = ac_root.join(JSGME_STATE_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(JsgmeState {
            version: 1,
            enabled: Vec::new(),
        }),
        Err(e) => Err(e),
    }
}

fn save_jsgme_state(ac_root: &Path, state: &JsgmeState) -> io::Result<()> {
    let path = ac_root.join(JSGME_STATE_FILE);
    let tmp = ac_root.join(format!("{JSGME_STATE_FILE}.tmp"));
    let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &path)
}

/// Lists all mod folder names present in the JSGME `MODS/` directory, sorted by name.
pub fn list_jsgme_mods(ac_root: &Path, custom_mods_dir: Option<&Path>) -> Vec<String> {
    let dir = jsgme_mods_dir(ac_root, custom_mods_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| !n.starts_with('.') && validate_id(n).is_ok())
        .collect();
    names.sort();
    names
}

fn collect_mod_files(mod_root: &Path) -> Result<Vec<String>, ModError> {
    let mut out = Vec::new();
    collect_mod_files_rec(mod_root, mod_root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_mod_files_rec(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), ModError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            return Err(ModError::UnsafeModEntry(path.display().to_string()));
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains(['/', '\\', ':', '\0']) || name == "." || name == ".." {
            return Err(ModError::UnsafeModEntry(name));
        }
        if ft.is_dir() {
            collect_mod_files_rec(root, &path, out)?;
        } else if ft.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| ModError::UnsafeModEntry(path.display().to_string()))?;
            let rel_str = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push(rel_str);
        }
    }
    Ok(())
}

/// Checks whether enabling `mod_name` would overwrite files owned by already-enabled JSGME mods.
pub fn check_jsgme_conflicts(
    ac_root: &Path,
    custom_mods_dir: Option<&Path>,
    mod_name: &str,
) -> Result<Vec<ModConflict>, ModError> {
    validate_id(mod_name)?;
    let mod_dir = jsgme_mods_dir(ac_root, custom_mods_dir).join(mod_name);
    if !mod_dir.is_dir() {
        return Err(ModError::NotInstalled(mod_dir));
    }
    let state = load_jsgme_state(ac_root)?;
    let mod_files = collect_mod_files(&mod_dir)?;
    let mut conflicts = Vec::new();
    for rel in &mod_files {
        for enabled in &state.enabled {
            if enabled.name.eq_ignore_ascii_case(mod_name) {
                continue;
            }
            if enabled.files.iter().any(|f| f.eq_ignore_ascii_case(rel)) {
                conflicts.push(ModConflict {
                    rel_path: rel.clone(),
                    existing_mod: enabled.name.clone(),
                });
            }
        }
    }
    Ok(conflicts)
}

/// Enables a JSGME mod from `MODS/<mod_name>/` into `ac_root`, backing up any displaced files
/// into `acxs-jsgme-backup/<mod_name>/`. Rolls back cleanly on any I/O failure.
pub fn enable_jsgme_mod(
    ac_root: &Path,
    custom_mods_dir: Option<&Path>,
    mod_name: &str,
    game_running: bool,
) -> Result<EnabledMod, ModError> {
    if game_running {
        return Err(ModError::GameRunning);
    }
    validate_id(mod_name)?;
    let mod_dir = jsgme_mods_dir(ac_root, custom_mods_dir).join(mod_name);
    if !mod_dir.is_dir() {
        return Err(ModError::NotInstalled(mod_dir));
    }
    let mut state = load_jsgme_state(ac_root)?;
    if state.enabled.iter().any(|m| m.name.eq_ignore_ascii_case(mod_name)) {
        return Err(ModError::AlreadyExists(mod_dir));
    }

    let files = collect_mod_files(&mod_dir)?;
    let backup_root = ac_root.join(JSGME_BACKUP_DIR).join(mod_name);
    let mut backed_up: Vec<String> = Vec::new();
    let mut applied: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();

    for rel in &files {
        let src = mod_dir.join(rel);
        let dest = ac_root.join(rel);
        let backup_dest = backup_root.join(rel);

        let moved_backup = if fs::symlink_metadata(&dest).is_ok() {
            if let Some(p) = backup_dest.parent() {
                if let Err(e) = fs::create_dir_all(p) {
                    rollback_jsgme_enable(&applied);
                    return Err(e.into());
                }
            }
            if let Err(e) = fs::rename(&dest, &backup_dest) {
                rollback_jsgme_enable(&applied);
                return Err(e.into());
            }
            backed_up.push(rel.clone());
            Some(backup_dest)
        } else {
            None
        };

        if let Some(p) = dest.parent() {
            if let Err(e) = fs::create_dir_all(p) {
                if let Some(b) = &moved_backup {
                    let _ = fs::rename(b, &dest);
                }
                rollback_jsgme_enable(&applied);
                return Err(e.into());
            }
        }
        if let Err(e) = fs::copy(&src, &dest) {
            if let Some(b) = &moved_backup {
                let _ = fs::rename(b, &dest);
            }
            rollback_jsgme_enable(&applied);
            return Err(e.into());
        }
        applied.push((dest, moved_backup));
    }

    let entry = EnabledMod {
        name: mod_name.to_string(),
        enabled_at: chrono::Local::now().to_rfc3339(),
        files,
        backed_up,
    };
    state.version = 1;
    state.enabled.push(entry.clone());
    if let Err(e) = save_jsgme_state(ac_root, &state) {
        rollback_jsgme_enable(&applied);
        return Err(e.into());
    }
    Ok(entry)
}

fn rollback_jsgme_enable(applied: &[(PathBuf, Option<PathBuf>)]) {
    for (dest, backup) in applied.iter().rev() {
        let _ = fs::remove_file(dest);
        if let Some(b) = backup {
            let _ = fs::rename(b, dest);
        }
    }
}

/// Disables an enabled JSGME mod, restoring any backed-up files and pruning empty directories.
/// Refuses with [`ModError::BlockedByDependent`] if a later-enabled mod overwrote any of its files.
pub fn disable_jsgme_mod(
    ac_root: &Path,
    mod_name: &str,
    game_running: bool,
) -> Result<(), ModError> {
    if game_running {
        return Err(ModError::GameRunning);
    }
    validate_id(mod_name)?;
    let mut state = load_jsgme_state(ac_root)?;
    let Some(idx) = state
        .enabled
        .iter()
        .position(|m| m.name.eq_ignore_ascii_case(mod_name))
    else {
        return Err(ModError::NotInstalled(ac_root.join(JSGME_MODS_DIR).join(mod_name)));
    };

    // Check LIFO conflict against any mod enabled after `idx`.
    let target = &state.enabled[idx];
    let mut dependents = Vec::new();
    let mut conflict_files = Vec::new();
    for later in &state.enabled[idx + 1..] {
        for f in &target.files {
            if later.files.iter().any(|lf| lf.eq_ignore_ascii_case(f)) {
                if !dependents.contains(&later.name) {
                    dependents.push(later.name.clone());
                }
                if !conflict_files.contains(f) {
                    conflict_files.push(f.clone());
                }
            }
        }
    }
    if !dependents.is_empty() {
        return Err(ModError::BlockedByDependent {
            mod_name: target.name.clone(),
            dependents,
            files: conflict_files,
        });
    }

    let removed = state.enabled.remove(idx);
    let backup_root = ac_root.join(JSGME_BACKUP_DIR).join(&removed.name);
    for rel in &removed.files {
        let dest = ac_root.join(rel);
        if dest.is_file() {
            let _ = fs::remove_file(&dest);
        }
        if removed.backed_up.iter().any(|b| b == rel) {
            let backup = backup_root.join(rel);
            if backup.is_file() {
                if let Some(p) = dest.parent() {
                    let _ = fs::create_dir_all(p);
                }
                fs::rename(&backup, &dest)?;
            }
        } else {
            prune_empty_parents(dest.parent(), ac_root);
        }
    }
    let _ = fs::remove_dir_all(&backup_root);
    save_jsgme_state(ac_root, &state)?;
    Ok(())
}

fn prune_empty_parents(mut dir: Option<&Path>, stop_at: &Path) {
    while let Some(d) = dir {
        if d == stop_at || !d.starts_with(stop_at) {
            break;
        }
        if fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temp AC root, removed on drop so cleanup also happens when a test panics.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("acxs-mods-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempRoot(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn put(&self, rel: &str, body: &str) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn disable_and_enable_car_round_trip() {
        let root = TempRoot::new("car-round-trip");
        let ac = root.path();
        root.put("content/cars/ks_abc/data/car.ini", "live-ini");
        root.put("content/cars/ks_abc/skins/s1/livery.txt", "livery");

        let stored = disable_car(ac, "ks_abc", false).unwrap();
        assert_eq!(stored, disabled_root(ac).join("cars").join("ks_abc"));
        assert!(!ac.join("content/cars/ks_abc").exists());
        assert_eq!(
            fs::read_to_string(stored.join("data/car.ini")).unwrap(),
            "live-ini"
        );

        let listed = list_disabled(ac);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, ContentKind::Car);
        assert_eq!(listed[0].id, "ks_abc");
        assert_eq!(listed[0].path, stored);

        let restored = enable_car(ac, "ks_abc", false).unwrap();
        assert_eq!(restored, ac.join("content/cars/ks_abc"));
        assert!(!stored.exists());
        assert!(list_disabled(ac).is_empty());
        assert_eq!(
            fs::read_to_string(restored.join("data/car.ini")).unwrap(),
            "live-ini"
        );
        assert_eq!(
            fs::read_to_string(restored.join("skins/s1/livery.txt")).unwrap(),
            "livery"
        );
    }

    #[test]
    fn disable_and_enable_track_round_trip() {
        let root = TempRoot::new("track-round-trip");
        let ac = root.path();
        root.put("content/tracks/ks_track/models.ini", "models");

        let stored = disable_track(ac, "ks_track", false).unwrap();
        assert_eq!(stored, disabled_root(ac).join("tracks").join("ks_track"));
        assert!(!ac.join("content/tracks/ks_track").exists());
        assert_eq!(list_disabled(ac)[0].kind, ContentKind::Track);

        enable_track(ac, "ks_track", false).unwrap();
        assert_eq!(
            fs::read_to_string(ac.join("content/tracks/ks_track/models.ini")).unwrap(),
            "models"
        );
        assert!(list_disabled(ac).is_empty());
    }

    #[test]
    fn refuses_to_overwrite_existing_target() {
        let root = TempRoot::new("no-overwrite");
        let ac = root.path();

        // Disable side: live car and a stored copy with the same id.
        root.put("content/cars/ks_dup/live.txt", "live");
        root.put("acxs-disabled/cars/ks_dup/stored.txt", "stored");
        let err = disable_car(ac, "ks_dup", false).unwrap_err();
        assert!(matches!(err, ModError::AlreadyExists(_)), "{err:?}");
        assert_eq!(fs::read_to_string(ac.join("content/cars/ks_dup/live.txt")).unwrap(), "live");
        assert_eq!(
            fs::read_to_string(ac.join("acxs-disabled/cars/ks_dup/stored.txt")).unwrap(),
            "stored"
        );

        // Enable side: stored track and a live track with the same id.
        root.put("acxs-disabled/tracks/t1/stored.txt", "stored");
        root.put("content/tracks/t1/live.txt", "live");
        let err = enable_track(ac, "t1", false).unwrap_err();
        assert!(matches!(err, ModError::AlreadyExists(_)), "{err:?}");
        assert_eq!(
            fs::read_to_string(ac.join("acxs-disabled/tracks/t1/stored.txt")).unwrap(),
            "stored"
        );
        assert_eq!(fs::read_to_string(ac.join("content/tracks/t1/live.txt")).unwrap(), "live");
    }

    #[test]
    fn missing_source_is_reported() {
        let root = TempRoot::new("missing");
        let err = disable_car(root.path(), "ks_none", false).unwrap_err();
        assert!(matches!(err, ModError::NotInstalled(_)), "{err:?}");
        assert!(!disabled_root(root.path()).exists());
    }

    #[test]
    fn traversal_ids_are_rejected() {
        let root = TempRoot::new("traversal");
        let ac = root.path();
        root.put("content/cars/keep/k.txt", "keep");

        let bad = [
            "", ".", "..", "../cars/keep", "a/b", "a\\b", "..\\cars\\keep", "c:evil", "x\0y",
        ];
        for id in bad {
            assert!(
                matches!(disable_car(ac, id, false), Err(ModError::InvalidId(_))),
                "disable_car accepted {id:?}"
            );
            assert!(
                matches!(enable_track(ac, id, false), Err(ModError::InvalidId(_))),
                "enable_track accepted {id:?}"
            );
        }

        assert_eq!(fs::read_to_string(ac.join("content/cars/keep/k.txt")).unwrap(), "keep");
        assert!(!disabled_root(ac).exists());
    }

    #[test]
    fn game_running_refuses_all_moves() {
        let root = TempRoot::new("game-running");
        let ac = root.path();
        root.put("content/cars/ks_abc/data.txt", "live");
        root.put("acxs-disabled/tracks/t1/data.txt", "stored");

        assert!(matches!(disable_car(ac, "ks_abc", true), Err(ModError::GameRunning)));
        assert!(matches!(enable_track(ac, "t1", true), Err(ModError::GameRunning)));

        assert_eq!(fs::read_to_string(ac.join("content/cars/ks_abc/data.txt")).unwrap(), "live");
        assert_eq!(
            fs::read_to_string(ac.join("acxs-disabled/tracks/t1/data.txt")).unwrap(),
            "stored"
        );
        assert!(!disabled_root(ac).join("cars").exists());
    }

    #[test]
    fn disabled_sizes_sum_files_and_leave_disk_alone() {
        let root = TempRoot::new("sizes");
        let ac = root.path();
        root.put("acxs-disabled/cars/ks_big/data/car.ini", &"x".repeat(100));
        root.put("acxs-disabled/cars/ks_big/skins/s1/livery.txt", "12345");
        root.put("acxs-disabled/tracks/t1/models.ini", "");
        root.put("content/cars/live/data.txt", "live-content");

        let sizes = list_disabled_sizes(ac).unwrap();
        let summary: Vec<(&str, u64)> = sizes.iter().map(|s| (s.item.id.as_str(), s.bytes)).collect();
        assert_eq!(summary, [("ks_big", 105), ("t1", 0)]);
        assert_eq!(sizes[0].item.kind, ContentKind::Car);
        assert_eq!(sizes[1].item.kind, ContentKind::Track);

        assert!(ac.join("acxs-disabled/cars/ks_big/data/car.ini").is_file());
        assert!(!ac.join("content/cars/ks_big").exists());
        assert_eq!(fs::read_to_string(ac.join("content/cars/live/data.txt")).unwrap(), "live-content");
    }

    #[test]
    fn disabled_sizes_empty_when_nothing_is_disabled() {
        let root = TempRoot::new("sizes-empty");
        assert!(list_disabled_sizes(root.path()).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn disabled_sizes_do_not_follow_symlinks() {
        let root = TempRoot::new("sizes-link");
        let ac = root.path();
        root.put("outside/big.bin", &"y".repeat(500));
        root.put("acxs-disabled/cars/ks_link/small.txt", "abc");
        std::os::unix::fs::symlink(
            ac.join("outside/big.bin"),
            ac.join("acxs-disabled/cars/ks_link/link.bin"),
        )
        .unwrap();

        let sizes = list_disabled_sizes(ac).unwrap();
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].bytes, 3);
    }

    #[test]
    fn jsgme_enable_disable_round_trip_restores_stock_files_and_prunes_dirs() {
        let root = TempRoot::new("jsgme-rt");
        let ac = root.path();
        root.put("system/cfg/graphics.ini", "stock-graphics");
        root.put("MODS/WeatherMod/system/cfg/graphics.ini", "mod-graphics");
        root.put("MODS/WeatherMod/extension/extra/new_file.ini", "added-by-mod");

        assert_eq!(list_jsgme_mods(ac, None), vec!["WeatherMod".to_string()]);
        assert!(check_jsgme_conflicts(ac, None, "WeatherMod").unwrap().is_empty());

        let enabled = enable_jsgme_mod(ac, None, "WeatherMod", false).unwrap();
        assert_eq!(enabled.files.len(), 2);
        assert_eq!(enabled.backed_up, vec!["system/cfg/graphics.ini".to_string()]);
        assert_eq!(fs::read_to_string(ac.join("system/cfg/graphics.ini")).unwrap(), "mod-graphics");
        assert_eq!(fs::read_to_string(ac.join("extension/extra/new_file.ini")).unwrap(), "added-by-mod");

        disable_jsgme_mod(ac, "WeatherMod", false).unwrap();
        assert_eq!(fs::read_to_string(ac.join("system/cfg/graphics.ini")).unwrap(), "stock-graphics");
        assert!(!ac.join("extension/extra/new_file.ini").exists());
        assert!(!ac.join("extension/extra").exists());
        assert!(load_jsgme_state(ac).unwrap().enabled.is_empty());
    }

    #[test]
    fn jsgme_detects_conflicts_blocks_out_of_order_disable_and_guards_game_running() {
        let root = TempRoot::new("jsgme-conflicts");
        let ac = root.path();
        root.put("system/cfg/a.ini", "stock");
        root.put("MODS/ModA/system/cfg/a.ini", "from-a");
        root.put("MODS/ModB/system/cfg/a.ini", "from-b");

        assert!(matches!(
            enable_jsgme_mod(ac, None, "ModA", true),
            Err(ModError::GameRunning)
        ));

        enable_jsgme_mod(ac, None, "ModA", false).unwrap();
        let conflicts = check_jsgme_conflicts(ac, None, "ModB").unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].existing_mod, "ModA");
        assert_eq!(conflicts[0].rel_path, "system/cfg/a.ini");

        enable_jsgme_mod(ac, None, "ModB", false).unwrap();
        assert_eq!(fs::read_to_string(ac.join("system/cfg/a.ini")).unwrap(), "from-b");

        assert!(matches!(
            disable_jsgme_mod(ac, "ModA", true),
            Err(ModError::GameRunning)
        ));
        // Out-of-order disable is refused because ModB overwrote a.ini.
        assert!(matches!(
            disable_jsgme_mod(ac, "ModA", false),
            Err(ModError::BlockedByDependent { .. })
        ));

        // Disabling in LIFO order restores ModA then stock.
        disable_jsgme_mod(ac, "ModB", false).unwrap();
        assert_eq!(fs::read_to_string(ac.join("system/cfg/a.ini")).unwrap(), "from-a");
        disable_jsgme_mod(ac, "ModA", false).unwrap();
        assert_eq!(fs::read_to_string(ac.join("system/cfg/a.ini")).unwrap(), "stock");
    }
}
