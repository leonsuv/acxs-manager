//! Import a skin folder into an installed car.
//!
//! The target is `<AC root>/content/cars/<car_id>/skins/<skin_id>/`, where `skin_id` is the
//! source folder's name. The layout and scan rules match `skins.rs`.
//!
//! Safety rules:
//! - Every import is refused while the game is running (the caller passes `game_running`,
//!   as `mods.rs` does).
//! - The car must exist (it has a `data/` or `ui/` folder). Car and skin ids may not hold
//!   separators, `.` or `..`, control characters or Windows-reserved characters.
//! - The source must be a folder with at least one texture (`.dds`, `.png`, `.jpg`) at its
//!   top level. `preview.*` and `livery.png` do not count. Symbolic links and unsafe file
//!   names are refused, not skipped, so nothing outside the source can be pulled in.
//! - An existing skin with the same id is never overwritten. It is moved to
//!   `<AC root>/acxs-skin-archive/<car_id>/<skin_id>-<stamp>/` and nothing is deleted.
//! - The copy goes to a staging folder that is renamed into place. If the copy fails,
//!   the staging folder is removed and the archived original is moved back.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;

use crate::util::copy_dir_all;

/// Folder inside the AC root that holds archived skins.
pub const ARCHIVE_DIR: &str = "acxs-skin-archive";

/// Image files that are not skin textures (same set as `skins.rs`).
const NON_TEXTURE_FILES: [&str; 3] = ["preview.jpg", "preview.png", "livery.png"];

/// Extensions that count as a skin texture for the import check.
const TEXTURE_EXTENSIONS: [&str; 3] = ["dds", "png", "jpg"];

#[derive(Debug)]
pub enum SkinImportError {
    GameRunning,
    InvalidCar(String),
    InvalidSource(Vec<String>),
    InvalidSkinId(String),
    Io(io::Error),
}

impl fmt::Display for SkinImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkinImportError::GameRunning => {
                write!(f, "Close Assetto Corsa before importing a skin.")
            }
            SkinImportError::InvalidCar(msg) => write!(f, "{msg}"),
            SkinImportError::InvalidSource(problems) => {
                write!(f, "the skin folder cannot be imported: {}", problems.join("; "))
            }
            SkinImportError::InvalidSkinId(id) => {
                write!(f, "{id:?} is not a valid skin folder name")
            }
            SkinImportError::Io(e) => write!(f, "file operation failed: {e}"),
        }
    }
}

impl std::error::Error for SkinImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SkinImportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for SkinImportError {
    fn from(e: io::Error) -> Self {
        SkinImportError::Io(e)
    }
}

/// Copy `source_dir` into `<AC root>/content/cars/<car_id>/skins/<folder name>` and
/// return the new skin folder. An existing skin with the same id is archived first.
pub fn import_skin(
    ac_root: &Path,
    car_id: &str,
    source_dir: &Path,
    game_running: bool,
) -> Result<PathBuf, SkinImportError> {
    if game_running {
        return Err(SkinImportError::GameRunning);
    }
    let car_dir = car_dir(ac_root, car_id)?;
    let skin_id = skin_id_from_source(source_dir)?;
    let problems = validate_source(source_dir);
    if !problems.is_empty() {
        return Err(SkinImportError::InvalidSource(problems));
    }

    let skins_dir = car_dir.join("skins");
    let target = skins_dir.join(&skin_id);
    if is_present(&target) && is_inside(source_dir, &target) {
        return Err(SkinImportError::InvalidSource(vec![format!(
            "{} is the installed skin or inside it; import from a copy instead",
            source_dir.display()
        )]));
    }

    fs::create_dir_all(&skins_dir)?;
    let car_archive = ac_root.join(ARCHIVE_DIR).join(car_id);
    fs::create_dir_all(&car_archive)?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S%3f").to_string();

    let archived = if is_present(&target) {
        Some(archive_existing(&car_archive, &skin_id, &stamp, &target)?)
    } else {
        None
    };

    // Unique per process and time, so the staging folder is ours alone.
    let staging = car_archive.join(format!(
        ".importing-{skin_id}-{}-{stamp}",
        std::process::id()
    ));
    let copied = copy_dir_all(source_dir, &staging).and_then(|()| fs::rename(&staging, &target));
    if let Err(err) = copied {
        let _ = fs::remove_dir_all(&staging);
        return Err(match restore_archived(archived.as_deref(), &target) {
            Ok(()) => err.into(),
            Err(restore_err) => {
                let archived = archived.map(|p| p.display().to_string()).unwrap_or_default();
                io::Error::new(
                    err.kind(),
                    format!(
                        "{err}; the original could not be moved back from {archived} ({restore_err})"
                    ),
                )
                .into()
            }
        });
    }
    Ok(target)
}

/// Check a source folder without changing anything on disk.
/// Returns one message per problem; an empty list means the folder can be imported.
pub fn validate_source(source_dir: &Path) -> Vec<String> {
    let shown = source_dir.display();
    match fs::metadata(source_dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return vec![format!("{shown} is not a folder")],
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return vec![format!("{shown} does not exist")];
        }
        Err(e) => return vec![format!("cannot read {shown}: {e}")],
    }

    let mut problems = Vec::new();
    let has_texture = scan_tree(source_dir, true, &mut problems);
    if !has_texture {
        problems.push(format!(
            "{shown} has no .dds, .png or .jpg texture at its top level"
        ));
    }
    problems
}

/// Walk `dir` without following symbolic links. Records unsafe names and links in
/// `problems`. Returns whether a skin texture was found, which only matters at the top level.
fn scan_tree(dir: &Path, top: bool, problems: &mut Vec<String>) -> bool {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            problems.push(format!("cannot read {}: {e}", dir.display()));
            return false;
        }
    };

    let mut has_texture = false;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                problems.push(format!("cannot read an entry in {}: {e}", dir.display()));
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if !is_safe_component(&name) {
            problems.push(format!("unsafe file name {name:?} in {}", dir.display()));
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(e) => {
                problems.push(format!("cannot read {}: {e}", path.display()));
                continue;
            }
        };
        if file_type.is_symlink() {
            problems.push(format!("{} is a symbolic link, which is not allowed", path.display()));
        } else if file_type.is_dir() {
            scan_tree(&path, false, problems);
        } else if file_type.is_file() && top && is_texture_file(&name) {
            has_texture = true;
        }
    }
    has_texture
}

/// Resolve `<AC root>/content/cars/<car_id>` and check that it is an installed car.
fn car_dir(ac_root: &Path, car_id: &str) -> Result<PathBuf, SkinImportError> {
    if !is_safe_component(car_id) {
        return Err(SkinImportError::InvalidCar(format!(
            "{car_id:?} is not a valid car id"
        )));
    }
    let dir = ac_root.join("content").join("cars").join(car_id);
    if dir.join("data").is_dir() || dir.join("ui").is_dir() {
        Ok(dir)
    } else {
        Err(SkinImportError::InvalidCar(format!(
            "{car_id} is not an installed car: {} has no data/ or ui/ folder",
            dir.display()
        )))
    }
}

/// The skin id is the source folder name, trimmed. Names that could escape the skins
/// folder are rejected rather than rewritten.
fn skin_id_from_source(source_dir: &Path) -> Result<String, SkinImportError> {
    let Some(raw) = source_dir.file_name().and_then(|n| n.to_str()) else {
        // Paths ending in `..` have no file name.
        return Err(SkinImportError::InvalidSkinId(source_dir.display().to_string()));
    };
    let id = raw.trim();
    if is_safe_component(id) {
        Ok(id.to_string())
    } else {
        Err(SkinImportError::InvalidSkinId(raw.to_string()))
    }
}

/// True for a single path component that cannot escape its parent folder.
fn is_safe_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}

/// Skin texture by name: right extension, and not a preview or livery image.
fn is_texture_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    if NON_TEXTURE_FILES.contains(&lower.as_str()) {
        return false;
    }
    Path::new(&lower)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| TEXTURE_EXTENSIONS.contains(&ext))
}

fn is_present(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// True when `source` is `target` or lies inside it. Copying in that case would
/// archive the source itself.
fn is_inside(source: &Path, target: &Path) -> bool {
    match (fs::canonicalize(source), fs::canonicalize(target)) {
        (Ok(source), Ok(target)) => source.starts_with(target),
        _ => false,
    }
}

/// Move the existing skin into the archive. Never overwrites an earlier archive.
fn archive_existing(
    car_archive: &Path,
    skin_id: &str,
    stamp: &str,
    target: &Path,
) -> io::Result<PathBuf> {
    let mut dest = car_archive.join(format!("{skin_id}-{stamp}"));
    let mut n = 2;
    while is_present(&dest) {
        dest = car_archive.join(format!("{skin_id}-{stamp}-{n}"));
        n += 1;
    }
    fs::rename(target, &dest)?;
    Ok(dest)
}

/// Move an archived original back to `target`, if there is one.
fn restore_archived(archived: Option<&Path>, target: &Path) -> io::Result<()> {
    match archived {
        Some(from) => fs::rename(from, target),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temp AC root that is removed on drop, even when a test panics.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "acxs_skin_import_test_{}_{}",
                std::process::id(),
                name
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            TempRoot(root)
        }

        fn ac(&self) -> &Path {
            &self.0
        }

        /// Create an installed car (with `data/`).
        fn car(&self, car_id: &str) {
            fs::create_dir_all(self.0.join("content").join("cars").join(car_id).join("data"))
                .unwrap();
        }

        /// A source skin folder at `incoming/<group>/<name>` holding the given files.
        fn source(&self, group: &str, name: &str, files: &[(&str, &str)]) -> PathBuf {
            let dir = self.0.join("incoming").join(group).join(name);
            fs::create_dir_all(&dir).unwrap();
            for (file, body) in files {
                fs::write(dir.join(file), body).unwrap();
            }
            dir
        }

        fn installed(&self, car_id: &str, skin_id: &str) -> PathBuf {
            self.0.join("content").join("cars").join(car_id).join("skins").join(skin_id)
        }

        /// Entries in `acxs-skin-archive/<car_id>/`, empty when the folder is missing.
        fn archived_entries(&self, car_id: &str) -> Vec<PathBuf> {
            fs::read_dir(self.0.join(ARCHIVE_DIR).join(car_id))
                .map(|rd| rd.flatten().map(|e| e.path()).collect())
                .unwrap_or_default()
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn imports_into_skins_folder() {
        let root = TempRoot::new("imports");
        root.car("ks_test");
        let src = root.source("first", "01_red", &[("body.dds", "old"), ("ui_skin.json", "{}")]);

        let target = import_skin(root.ac(), "ks_test", &src, false).unwrap();

        assert_eq!(target, root.installed("ks_test", "01_red"));
        assert_eq!(fs::read_to_string(target.join("body.dds")).unwrap(), "old");
        assert!(target.join("ui_skin.json").is_file());
        assert!(src.join("body.dds").is_file(), "source must be left alone");
        assert!(root.archived_entries("ks_test").is_empty());
    }

    #[test]
    fn second_import_archives_first() {
        let root = TempRoot::new("archives");
        root.car("ks_test");
        let first = root.source("first", "01_red", &[("body.dds", "old")]);
        import_skin(root.ac(), "ks_test", &first, false).unwrap();

        let second = root.source("second", "01_red", &[("body.dds", "new")]);
        let target = import_skin(root.ac(), "ks_test", &second, false).unwrap();

        assert_eq!(fs::read_to_string(target.join("body.dds")).unwrap(), "new");
        let archived = root.archived_entries("ks_test");
        assert_eq!(archived.len(), 1, "{archived:?}");
        let name = archived[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("01_red-"), "{name}");
        assert_eq!(fs::read_to_string(archived[0].join("body.dds")).unwrap(), "old");
    }

    #[test]
    fn game_running_is_refused() {
        let root = TempRoot::new("game_running");
        root.car("ks_test");
        let src = root.source("in", "01_red", &[("body.dds", "x")]);

        let result = import_skin(root.ac(), "ks_test", &src, true);

        assert!(matches!(result, Err(SkinImportError::GameRunning)), "{result:?}");
        assert!(!root.installed("ks_test", "01_red").exists());
    }

    #[test]
    fn traversal_ids_are_refused() {
        let root = TempRoot::new("traversal");
        root.car("ks_test");
        let src = root.source("in", "01_red", &[("body.dds", "x")]);

        for car in ["../ks_evil", "a/b", "", "."] {
            let result = import_skin(root.ac(), car, &src, false);
            assert!(matches!(result, Err(SkinImportError::InvalidCar(_))), "car {car:?}: {result:?}");
        }
        assert!(!root.ac().join("content").join("ks_evil").exists());

        // A source path ending in `..` has no folder name to use as the skin id.
        let dotdot = src.join("..");
        let result = import_skin(root.ac(), "ks_test", &dotdot, false);
        assert!(matches!(result, Err(SkinImportError::InvalidSkinId(_))), "{result:?}");
        assert!(root.archived_entries("ks_test").is_empty());
    }

    #[test]
    fn safe_component_rules() {
        assert!(is_safe_component("01_red"));
        assert!(is_safe_component("Blue Ice"));
        for bad in ["", ".", "..", "a/b", "a\\b", "c:d", "x\0y", "q*"] {
            assert!(!is_safe_component(bad), "{bad:?} should be unsafe");
        }
    }

    #[test]
    fn source_without_textures_is_refused() {
        let root = TempRoot::new("no_textures");
        root.car("ks_test");
        // preview and livery images are not textures, and neither are text files.
        let src = root.source(
            "in",
            "01_red",
            &[("ui_skin.json", "{}"), ("notes.txt", "x"), ("preview.jpg", "x"), ("livery.png", "x")],
        );

        let problems = validate_source(&src);
        assert!(problems.iter().any(|p| p.contains("texture")), "{problems:?}");

        let result = import_skin(root.ac(), "ks_test", &src, false);
        assert!(
            matches!(&result, Err(SkinImportError::InvalidSource(p)) if !p.is_empty()),
            "{result:?}"
        );
        assert!(!root.installed("ks_test", "01_red").exists());
    }

    #[test]
    fn invalid_car_is_refused() {
        let root = TempRoot::new("invalid_car");
        // A folder without data/ or ui/ is not an installed car.
        fs::create_dir_all(root.ac().join("content").join("cars").join("ks_empty")).unwrap();
        let src = root.source("in", "01_red", &[("body.dds", "x")]);

        for car in ["ks_empty", "ks_missing"] {
            let result = import_skin(root.ac(), car, &src, false);
            assert!(matches!(result, Err(SkinImportError::InvalidCar(_))), "{car}: {result:?}");
        }
    }

    #[test]
    fn importing_installed_skin_onto_itself_is_refused() {
        let root = TempRoot::new("self_import");
        root.car("ks_test");
        let src = root.source("in", "01_red", &[("body.dds", "x")]);
        let installed = import_skin(root.ac(), "ks_test", &src, false).unwrap();

        let result = import_skin(root.ac(), "ks_test", &installed, false);

        assert!(matches!(result, Err(SkinImportError::InvalidSource(_))), "{result:?}");
        assert!(installed.join("body.dds").is_file());
        assert!(root.archived_entries("ks_test").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_source_is_refused() {
        let root = TempRoot::new("symlink");
        root.car("ks_test");
        let secret = root.0.join("secret.txt");
        fs::write(&secret, "outside the skin").unwrap();
        let src = root.source("in", "01_red", &[("body.dds", "x")]);
        std::os::unix::fs::symlink(&secret, src.join("link.dds")).unwrap();

        let problems = validate_source(&src);
        assert!(problems.iter().any(|p| p.contains("symbolic link")), "{problems:?}");

        let result = import_skin(root.ac(), "ks_test", &src, false);
        assert!(matches!(result, Err(SkinImportError::InvalidSource(_))), "{result:?}");
        assert!(!root.installed("ks_test", "01_red").exists());
    }

    #[cfg(unix)]
    #[test]
    fn failed_copy_restores_original() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempRoot::new("rollback");
        root.car("ks_test");
        let original = root.installed("ks_test", "01_red");
        fs::create_dir_all(&original).unwrap();
        fs::write(original.join("body.dds"), "old").unwrap();

        // An unreadable texture makes the copy fail partway through.
        let src = root.source("in", "01_red", &[("body.dds", "new"), ("locked.dds", "x")]);
        let locked = src.join("locked.dds");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::File::open(&locked).is_ok() {
            // Running as root: permissions do not block the copy, so there is nothing to test.
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let result = import_skin(root.ac(), "ks_test", &src, false);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();

        assert!(matches!(result, Err(SkinImportError::Io(_))), "{result:?}");
        assert_eq!(fs::read_to_string(original.join("body.dds")).unwrap(), "old");
        assert!(root.archived_entries("ks_test").is_empty(), "archive must be empty again");
        let skins: Vec<_> = fs::read_dir(original.parent().unwrap()).unwrap().flatten().collect();
        assert_eq!(skins.len(), 1, "no partial copy may remain in skins/");
    }
}
