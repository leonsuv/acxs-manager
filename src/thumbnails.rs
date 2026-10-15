//! Car thumbnails. A car's main KN5 model is loaded, drawn from a fixed
//! three-quarter view, and saved as a PNG in the car folder's `acxs-thumbnails`
//! subfolder. That folder doubles as the cache.
//!
//! Thumbnail files are only ever created, never overwritten. A thumbnail that
//! already exists for the requested size is returned as it is.

use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::kn5_export::{self, ExportError};
use crate::ortho::{self, OrthoError, View};

/// Name of the cache subfolder inside each car folder.
const CACHE_DIR: &str = "acxs-thumbnails";

/// Fixed three-quarter view: 45 degrees of yaw and 20 degrees of pitch.
const VIEW_YAW_DEG: f32 = 45.0;
const VIEW_PITCH_DEG: f32 = 20.0;
const VIEW_PADDING: f32 = 0.08;

/// Why a thumbnail could not be produced.
#[derive(Debug)]
pub enum ThumbError {
    /// The car id is empty or would leave the cars folder.
    InvalidCarId(String),
    /// The requested size is zero or above the renderer's limit.
    InvalidSize(u32),
    /// The car folder does not exist.
    CarNotFound(PathBuf),
    /// The car folder has no `.kn5` file at its root.
    NoModel(PathBuf),
    /// The model file could not be read.
    Model(ExportError),
    /// The triangles could not be rendered or encoded.
    Render(OrthoError),
    /// Reading or writing the cache failed.
    Io(io::Error),
}

impl fmt::Display for ThumbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThumbError::InvalidCarId(id) => write!(f, "invalid car id {id:?}"),
            ThumbError::InvalidSize(size) => write!(
                f,
                "thumbnail size {size} must be between 1 and {}",
                ortho::MAX_DIMENSION
            ),
            ThumbError::CarNotFound(path) => {
                write!(f, "car folder not found: {}", path.display())
            }
            ThumbError::NoModel(path) => {
                write!(f, "no .kn5 model in {}", path.display())
            }
            ThumbError::Model(e) => write!(f, "could not read car model: {e}"),
            ThumbError::Render(e) => write!(f, "could not render thumbnail: {e}"),
            ThumbError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl Error for ThumbError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ThumbError::Model(e) => Some(e),
            ThumbError::Render(e) => Some(e),
            ThumbError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ExportError> for ThumbError {
    fn from(e: ExportError) -> Self {
        ThumbError::Model(e)
    }
}

impl From<OrthoError> for ThumbError {
    fn from(e: OrthoError) -> Self {
        ThumbError::Render(e)
    }
}

impl From<io::Error> for ThumbError {
    fn from(e: io::Error) -> Self {
        ThumbError::Io(e)
    }
}

/// The folder that holds a car's thumbnails: `<ac_root>/content/cars/<car_id>/acxs-thumbnails`.
/// Each size is stored as `<size>.png` inside it.
pub fn cache_path(ac_root: &Path, car_id: &str) -> PathBuf {
    ac_root
        .join("content")
        .join("cars")
        .join(car_id)
        .join(CACHE_DIR)
}

/// Returns a PNG thumbnail of `size` by `size` pixels for the car.
///
/// The main model is the `.kn5` at the car folder's root whose name matches the
/// folder, or the largest `.kn5` there when none matches. The PNG is written to
/// [`cache_path`] as `<size>.png`. If that file already exists, its bytes are
/// returned and nothing is rendered or written.
pub fn car_thumbnail(ac_root: &Path, car_id: &str, size: u32) -> Result<Vec<u8>, ThumbError> {
    if !is_safe_car_id(car_id) {
        return Err(ThumbError::InvalidCarId(car_id.to_string()));
    }
    if size == 0 || size > ortho::MAX_DIMENSION {
        return Err(ThumbError::InvalidSize(size));
    }

    let car_dir = ac_root.join("content").join("cars").join(car_id);
    if !car_dir.is_dir() {
        return Err(ThumbError::CarNotFound(car_dir));
    }

    let cache_dir = cache_path(ac_root, car_id);
    let target = cache_dir.join(format!("{size}.png"));
    match fs::read(&target) {
        Ok(bytes) => return Ok(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    let model = find_main_model(&car_dir)?;
    let triangles = kn5_export::load_triangles(&model)?;
    let view = View {
        yaw_deg: VIEW_YAW_DEG,
        pitch_deg: VIEW_PITCH_DEG,
        width: size,
        height: size,
        padding: VIEW_PADDING,
    };
    let png = ortho::render_png(&triangles, &view)?;

    fs::create_dir_all(&cache_dir)?;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(&png) {
                drop(file);
                // Do not leave a truncated thumbnail behind for the cache to return.
                let _ = fs::remove_file(&target);
                return Err(e.into());
            }
            Ok(png)
        }
        // Another writer created the file first. Return theirs.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(fs::read(&target)?),
        Err(e) => Err(e.into()),
    }
}

/// A car id is one plain folder name: no separators, no `.`/`..`, no drive colons.
fn is_safe_car_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\', '\0', ':'])
}

/// Picks the car's model: a root-level `.kn5` whose stem matches the folder name,
/// otherwise the largest root-level `.kn5` (ties broken by path).
fn find_main_model(car_dir: &Path) -> Result<PathBuf, ThumbError> {
    let car_name = car_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut largest: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(car_dir)? {
        let entry = entry?;
        let path = entry.path();
        let is_kn5 = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("kn5"));
        if !is_kn5 || !entry.file_type()?.is_file() {
            continue;
        }

        let stem_matches = path
            .file_stem()
            .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case(&car_name));
        if stem_matches {
            return Ok(path);
        }

        let len = entry.metadata()?.len();
        let replaces = match &largest {
            None => true,
            Some((best_len, best_path)) => {
                len > *best_len || (len == *best_len && path < *best_path)
            }
        };
        if replaces {
            largest = Some((len, path));
        }
    }

    largest
        .map(|(_, path)| path)
        .ok_or_else(|| ThumbError::NoModel(car_dir.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kn5::Kn5Error;
    use crate::kn5_export::tests::{kn5_with_triangle, triangle_kn5};

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    /// A fresh AC root under the temp dir, unique per test tag.
    fn scratch_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "acxs_thumb_{}_{}",
            std::process::id(),
            tag
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create scratch root");
        root
    }

    /// Creates `<root>/content/cars/<car>` and returns it.
    fn make_car(root: &Path, car: &str) -> PathBuf {
        let dir = root.join("content").join("cars").join(car);
        fs::create_dir_all(&dir).expect("create car dir");
        dir
    }

    #[test]
    fn cache_path_is_thumbnail_folder_under_car() {
        let expected = PathBuf::from("/ac")
            .join("content")
            .join("cars")
            .join("abc")
            .join("acxs-thumbnails");
        assert_eq!(cache_path(Path::new("/ac"), "abc"), expected);
    }

    #[test]
    fn renders_png_and_caches_it() {
        let root = scratch_root("render_cache");
        let car = make_car(&root, "abc");
        fs::write(car.join("abc.kn5"), triangle_kn5()).expect("write model");

        let png = car_thumbnail(&root, "abc", 64).expect("thumbnail");
        assert!(png.starts_with(&PNG_SIGNATURE));

        let cached = cache_path(&root, "abc").join("64.png");
        assert_eq!(fs::read(&cached).expect("cached file"), png);

        let again = car_thumbnail(&root, "abc", 64).expect("cached thumbnail");
        assert_eq!(again, png);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn existing_thumbnail_is_never_overwritten() {
        let root = scratch_root("no_overwrite");
        make_car(&root, "abc");
        let cache = cache_path(&root, "abc");
        fs::create_dir_all(&cache).expect("create cache");
        fs::write(cache.join("64.png"), b"kept").expect("seed thumbnail");

        let bytes = car_thumbnail(&root, "abc", 64).expect("existing thumbnail");
        assert_eq!(bytes, b"kept");
        assert_eq!(fs::read(cache.join("64.png")).expect("read"), b"kept");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn name_matching_model_wins_over_a_larger_file() {
        let root = scratch_root("name_match");
        let car = make_car(&root, "abc");
        fs::write(car.join("abc.kn5"), triangle_kn5()).expect("write model");
        // Larger, but not a KN5. Picking it would fail the thumbnail.
        fs::write(car.join("other.kn5"), vec![0u8; 10_000]).expect("write decoy");

        assert!(car_thumbnail(&root, "abc", 32).is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn largest_model_is_used_when_no_name_matches() {
        let root = scratch_root("largest");
        let car = make_car(&root, "abc");
        fs::write(car.join("x.kn5"), triangle_kn5()).expect("write model");
        fs::write(car.join("y.kn5"), vec![1u8; 16]).expect("write smaller decoy");

        assert!(car_thumbnail(&root, "abc", 32).is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_model_is_reported() {
        let root = scratch_root("no_model");
        let car = make_car(&root, "abc");
        fs::write(car.join("readme.txt"), b"no model here").expect("write file");

        assert!(matches!(
            car_thumbnail(&root, "abc", 32),
            Err(ThumbError::NoModel(_))
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_car_is_reported() {
        let root = scratch_root("no_car");
        assert!(matches!(
            car_thumbnail(&root, "nope", 32),
            Err(ThumbError::CarNotFound(_))
        ));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unsafe_car_ids_and_bad_sizes_are_rejected() {
        let root = scratch_root("bad_args");
        make_car(&root, "abc");
        for id in ["", ".", "..", "../abc", "a/b", "a\\b", "c:x"] {
            assert!(
                matches!(car_thumbnail(&root, id, 32), Err(ThumbError::InvalidCarId(_))),
                "id {id:?} accepted"
            );
        }
        for size in [0u32, ortho::MAX_DIMENSION + 1] {
            assert!(matches!(
                car_thumbnail(&root, "abc", size),
                Err(ThumbError::InvalidSize(s)) if s == size
            ));
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unreadable_model_leaves_no_thumbnail() {
        let root = scratch_root("bad_model");
        let car = make_car(&root, "abc");
        fs::write(car.join("abc.kn5"), b"junk, not a kn5").expect("write model");

        assert!(matches!(
            car_thumbnail(&root, "abc", 32),
            Err(ThumbError::Model(ExportError::Kn5(Kn5Error::NotKn5)))
        ));
        assert!(!cache_path(&root, "abc").join("32.png").exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn model_with_only_ac_nodes_still_renders() {
        let root = scratch_root("ac_only");
        let car = make_car(&root, "abc");
        fs::write(car.join("abc.kn5"), kn5_with_triangle("AC_collider", [0, 1, 2]))
            .expect("write model");

        let png = car_thumbnail(&root, "abc", 16).expect("thumbnail");
        assert!(png.starts_with(&PNG_SIGNATURE));
        fs::remove_dir_all(&root).ok();
    }
}
