//! Assetto Corsa user configuration: `video.ini` and `assetto_corsa.ini`.
//!
//! Section names, keys, defaults and value lists are the ones the game reads and
//! writes. `video.ini` lives in the user Documents folder (`Documents/Assetto Corsa/cfg/`),
//! `assetto_corsa.ini` in `<AC root>/system/cfg/`. The functions here take the
//! directory that holds the file, so the caller picks the location.
//!
//! Reading is lenient: a missing file gives the defaults, and a value the game would
//! not accept falls back to its default, as the game does. Writing goes through
//! [`Ini::set`], so unknown keys, comments and key order survive, and the previous
//! file is renamed to a backup (never deleted) by [`Ini::save_with_backup`].
//! Fields that are `None` are not written, so the existing value in the file stays.

use std::io;
use std::ops::RangeInclusive;
use std::path::Path;

use crate::ini::Ini;

pub const VIDEO_FILE: &str = "video.ini";
pub const ASSETTO_CORSA_FILE: &str = "assetto_corsa.ini";

/// `[VIDEO] AASAMPLES`. 1 means off.
const AA_SAMPLES: &[u32] = &[1, 2, 4, 8];
/// `[VIDEO] ANISOTROPIC`. 0 means off.
const ANISOTROPIC: &[u32] = &[0, 2, 4, 8, 16];
/// `[VIDEO] SHADOW_MAP_SIZE`. -1 means off.
const SHADOW_MAP_SIZES: &[i32] = &[-1, 32, 64, 128, 256, 512, 1024, 2048, 3072, 4096, 6144, 8192];
/// `[ASSETTOCORSA] WORLD_DETAIL`, 0 (minimum) to 5 (maximum).
const WORLD_DETAILS: &[u32] = &[0, 1, 2, 3, 4, 5];
/// `[SCREENSHOT] FORMAT`.
const SCREENSHOT_FORMATS: &[&str] = &["JPG", "BMP", "PNG"];

const MIRRORS_FOV_RANGE: RangeInclusive<u32> = 1..=180;
const MIRRORS_FAR_PLANE_RANGE: RangeInclusive<u32> = 10..=2000;

// ---------------------------------------------------------------------------
// video.ini
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct VideoSettings {
    /// `[VIDEO] WIDTH`. `None` when the file has no usable resolution.
    pub width: Option<u32>,
    /// `[VIDEO] HEIGHT`. `None` when the file has no usable resolution.
    pub height: Option<u32>,
    /// `[VIDEO] FULLSCREEN`. Default `true`.
    pub fullscreen: bool,
    /// `[VIDEO] VSYNC`. Default `false`.
    pub vsync: bool,
    /// Frame rate cap in frames per second. The file stores `[VIDEO] FPS_CAP_MS`
    /// (milliseconds per frame, 0 = no cap); this field converts. `None` = no cap.
    pub fps_cap: Option<i32>,
    /// `[VIDEO] AASAMPLES`, one of 1, 2, 4, 8 (1 = off). Default 1.
    pub aa_samples: u32,
    /// `[VIDEO] ANISOTROPIC`, one of 0, 2, 4, 8, 16 (0 = off). Default 8.
    pub anisotropic: u32,
    /// `[VIDEO] SHADOW_MAP_SIZE`, -1 (off) or a power of two up to 8192. Default 2048.
    pub shadow_map_size: i32,
    /// `[ASSETTOCORSA] WORLD_DETAIL`, 0 to 5. Default 4.
    pub world_detail: u32,
    /// `[POST_PROCESS] ENABLED`. Default `true`.
    pub post_processing: bool,
}

impl Default for VideoSettings {
    fn default() -> Self {
        VideoSettings {
            width: None,
            height: None,
            fullscreen: true,
            vsync: false,
            fps_cap: None,
            aa_samples: 1,
            anisotropic: 8,
            shadow_map_size: 2048,
            world_detail: 4,
            post_processing: true,
        }
    }
}

/// Read `video.ini` from `cfg_dir`. A missing file gives [`VideoSettings::default`].
pub fn read_video(cfg_dir: &Path) -> io::Result<VideoSettings> {
    let ini = load_or_empty(&cfg_dir.join(VIDEO_FILE))?;
    Ok(video_from_ini(&ini))
}

/// Write `settings` into `video.ini` in `cfg_dir`, keeping every other key. The old
/// file is renamed to a backup first. Invalid settings are refused and nothing is written.
pub fn write_video(cfg_dir: &Path, settings: &VideoSettings) -> io::Result<()> {
    let errors = validate_video(settings);
    if !errors.is_empty() {
        return Err(invalid_input(errors));
    }
    let path = cfg_dir.join(VIDEO_FILE);
    let mut ini = load_or_empty(&path)?;
    apply_video(&mut ini, settings);
    ini.save_with_backup(&path)
}

/// Problems that make the settings impossible to apply. Empty when valid.
pub fn validate_video(settings: &VideoSettings) -> Vec<String> {
    let mut errors = Vec::new();
    if settings.width == Some(0) {
        errors.push("width must be greater than 0".to_string());
    }
    if settings.height == Some(0) {
        errors.push("height must be greater than 0".to_string());
    }
    if settings.width.is_some() != settings.height.is_some() {
        errors.push("width and height must be set together".to_string());
    }
    if let Some(fps) = settings.fps_cap {
        if fps <= 0 {
            errors.push(format!("fps cap must be positive, got {fps} (use None for no cap)"));
        }
    }
    if !AA_SAMPLES.contains(&settings.aa_samples) {
        errors.push(format!("anti-aliasing samples must be one of {AA_SAMPLES:?}, got {}", settings.aa_samples));
    }
    if !ANISOTROPIC.contains(&settings.anisotropic) {
        errors.push(format!("anisotropic level must be one of {ANISOTROPIC:?}, got {}", settings.anisotropic));
    }
    if !SHADOW_MAP_SIZES.contains(&settings.shadow_map_size) {
        errors.push(format!(
            "shadow map size must be one of {SHADOW_MAP_SIZES:?}, got {}",
            settings.shadow_map_size
        ));
    }
    if !WORLD_DETAILS.contains(&settings.world_detail) {
        errors.push(format!("world detail must be 0 to 5, got {}", settings.world_detail));
    }
    errors
}

fn video_from_ini(ini: &Ini) -> VideoSettings {
    let d = VideoSettings::default();

    // Width and height only count as a pair, as the game reads them.
    let (width, height) = match (read_u32(ini, "VIDEO", "WIDTH"), read_u32(ini, "VIDEO", "HEIGHT")) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (Some(w), Some(h)),
        _ => (None, None),
    };

    // The game stores the cap as milliseconds per frame; anything under 0.1 ms means no cap.
    let fps_cap = ini.get_f64("VIDEO", "FPS_CAP_MS").and_then(fps_from_ms);

    // Shadow patching mods keep the original shadow size under this key. Prefer it when present.
    let shadow_key = if ini.get("VIDEO", "__CM_ORIGINAL_SHADOW_MAP_SIZE").is_some() {
        "__CM_ORIGINAL_SHADOW_MAP_SIZE"
    } else {
        "SHADOW_MAP_SIZE"
    };

    VideoSettings {
        width,
        height,
        fullscreen: read_flag(ini, "VIDEO", "FULLSCREEN", d.fullscreen),
        vsync: read_flag(ini, "VIDEO", "VSYNC", d.vsync),
        fps_cap,
        aa_samples: pick(read_u32(ini, "VIDEO", "AASAMPLES"), AA_SAMPLES, d.aa_samples),
        anisotropic: pick(read_u32(ini, "VIDEO", "ANISOTROPIC"), ANISOTROPIC, d.anisotropic),
        shadow_map_size: pick(read_i32(ini, "VIDEO", shadow_key), SHADOW_MAP_SIZES, d.shadow_map_size),
        world_detail: pick(read_u32(ini, "ASSETTOCORSA", "WORLD_DETAIL"), WORLD_DETAILS, d.world_detail),
        post_processing: read_flag(ini, "POST_PROCESS", "ENABLED", d.post_processing),
    }
}

fn apply_video(ini: &mut Ini, s: &VideoSettings) {
    if let (Some(width), Some(height)) = (s.width, s.height) {
        ini.set("VIDEO", "WIDTH", width);
        ini.set("VIDEO", "HEIGHT", height);
    }
    ini.set("VIDEO", "FULLSCREEN", flag_str(s.fullscreen));
    ini.set("VIDEO", "VSYNC", flag_str(s.vsync));
    ini.set("VIDEO", "FPS_CAP_MS", fps_to_ms(s.fps_cap));
    ini.set("VIDEO", "AASAMPLES", s.aa_samples);
    ini.set("VIDEO", "ANISOTROPIC", s.anisotropic);
    ini.set("VIDEO", "SHADOW_MAP_SIZE", s.shadow_map_size);
    if ini.get("VIDEO", "__CM_ORIGINAL_SHADOW_MAP_SIZE").is_some() {
        ini.set("VIDEO", "__CM_ORIGINAL_SHADOW_MAP_SIZE", s.shadow_map_size);
    }
    ini.set("ASSETTOCORSA", "WORLD_DETAIL", s.world_detail);
    ini.set("POST_PROCESS", "ENABLED", flag_str(s.post_processing));
}

// ---------------------------------------------------------------------------
// assetto_corsa.ini (in system/cfg)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct AssettoCorsaSettings {
    /// `[ASSETTO_CORSA] SIMULATION_VALUE`, shown as a percentage 0 to 100. Default 0.
    pub simulation_value: u32,
    /// Logging on or off. The file stores `[LOG] SUPPRESS`, which is inverted. Default `true`.
    pub logging: bool,
    /// `[AC_APPS] ENABLE_DEV_APPS`. Default `false`.
    pub developer_apps: bool,
    /// `[CAMERA] ALLOW_FREE_CAMERA`. Default `false`.
    pub allow_free_camera: bool,
    /// `[DRIVER] HIDE`. Default `false`.
    pub hide_driver: bool,
    /// `[SCREENSHOT] FORMAT`, one of `JPG`, `BMP`, `PNG` (PNG needs CSP). Default `JPG`.
    pub screenshot_format: String,
    /// `[MIRRORS] FOV`, 1 to 180. Default 10.
    pub mirrors_fov: u32,
    /// `[MIRRORS] FAR_PLANE`, 10 to 2000. Default 400.
    pub mirrors_far_plane: u32,
}

impl Default for AssettoCorsaSettings {
    fn default() -> Self {
        AssettoCorsaSettings {
            simulation_value: 0,
            logging: true,
            developer_apps: false,
            allow_free_camera: false,
            hide_driver: false,
            screenshot_format: "JPG".to_string(),
            mirrors_fov: 10,
            mirrors_far_plane: 400,
        }
    }
}

/// Read `assetto_corsa.ini` from `cfg_dir` (normally `<AC root>/system/cfg`). A missing
/// file gives [`AssettoCorsaSettings::default`].
pub fn read_assetto_corsa_ini(cfg_dir: &Path) -> io::Result<AssettoCorsaSettings> {
    let ini = load_or_empty(&cfg_dir.join(ASSETTO_CORSA_FILE))?;
    Ok(assetto_corsa_from_ini(&ini))
}

/// Write `settings` into `assetto_corsa.ini` in `cfg_dir`, keeping every other key
/// (including the force feedback keys this struct does not model). The old file is
/// renamed to a backup first. Invalid settings are refused and nothing is written.
pub fn write_assetto_corsa_ini(cfg_dir: &Path, settings: &AssettoCorsaSettings) -> io::Result<()> {
    let errors = validate_assetto_corsa(settings);
    if !errors.is_empty() {
        return Err(invalid_input(errors));
    }
    let path = cfg_dir.join(ASSETTO_CORSA_FILE);
    let mut ini = load_or_empty(&path)?;
    apply_assetto_corsa(&mut ini, settings);
    ini.save_with_backup(&path)
}

/// Problems that make the settings impossible to apply. Empty when valid.
pub fn validate_assetto_corsa(settings: &AssettoCorsaSettings) -> Vec<String> {
    let mut errors = Vec::new();
    if settings.simulation_value > 100 {
        errors.push(format!("simulation value must be 0 to 100, got {}", settings.simulation_value));
    }
    if !MIRRORS_FOV_RANGE.contains(&settings.mirrors_fov) {
        errors.push(format!("mirrors field of view must be 1 to 180, got {}", settings.mirrors_fov));
    }
    if !MIRRORS_FAR_PLANE_RANGE.contains(&settings.mirrors_far_plane) {
        errors.push(format!("mirrors far plane must be 10 to 2000, got {}", settings.mirrors_far_plane));
    }
    if !SCREENSHOT_FORMATS.contains(&settings.screenshot_format.as_str()) {
        errors.push(format!(
            "screenshot format must be one of {SCREENSHOT_FORMATS:?}, got {:?}",
            settings.screenshot_format
        ));
    }
    errors
}

fn assetto_corsa_from_ini(ini: &Ini) -> AssettoCorsaSettings {
    let d = AssettoCorsaSettings::default();
    AssettoCorsaSettings {
        simulation_value: ini
            .get_f64("ASSETTO_CORSA", "SIMULATION_VALUE")
            .filter(|v| v.is_finite())
            .map(|v| (100.0 * v).round().clamp(0.0, 100.0) as u32)
            .unwrap_or(d.simulation_value),
        // The game writes SUPPRESS=1 to turn logging off; a missing key means logging is on.
        logging: !read_flag(ini, "LOG", "SUPPRESS", false),
        developer_apps: read_flag(ini, "AC_APPS", "ENABLE_DEV_APPS", d.developer_apps),
        allow_free_camera: read_flag(ini, "CAMERA", "ALLOW_FREE_CAMERA", d.allow_free_camera),
        hide_driver: read_flag(ini, "DRIVER", "HIDE", d.hide_driver),
        screenshot_format: ini
            .get("SCREENSHOT", "FORMAT")
            .map(|v| v.trim().to_ascii_uppercase())
            .filter(|v| SCREENSHOT_FORMATS.contains(&v.as_str()))
            .unwrap_or(d.screenshot_format),
        mirrors_fov: in_range(read_u32(ini, "MIRRORS", "FOV"), MIRRORS_FOV_RANGE, d.mirrors_fov),
        mirrors_far_plane: in_range(
            read_u32(ini, "MIRRORS", "FAR_PLANE"),
            MIRRORS_FAR_PLANE_RANGE,
            d.mirrors_far_plane,
        ),
    }
}

fn apply_assetto_corsa(ini: &mut Ini, s: &AssettoCorsaSettings) {
    ini.set("ASSETTO_CORSA", "SIMULATION_VALUE", s.simulation_value as f64 / 100.0);
    ini.set("LOG", "SUPPRESS", flag_str(!s.logging));
    ini.set("AC_APPS", "ENABLE_DEV_APPS", flag_str(s.developer_apps));
    ini.set("CAMERA", "ALLOW_FREE_CAMERA", flag_str(s.allow_free_camera));
    ini.set("DRIVER", "HIDE", flag_str(s.hide_driver));
    ini.set("SCREENSHOT", "FORMAT", s.screenshot_format.as_str());
    ini.set("MIRRORS", "FOV", s.mirrors_fov);
    ini.set("MIRRORS", "FAR_PLANE", s.mirrors_far_plane);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A missing file is an empty config; any other error is reported.
fn load_or_empty(path: &Path) -> io::Result<Ini> {
    match Ini::load(path) {
        Ok(ini) => Ok(ini),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Ini::default()),
        Err(e) => Err(e),
    }
}

/// The game reads flags as exactly `1`.
fn read_flag(ini: &Ini, section: &str, key: &str, default: bool) -> bool {
    match ini.get(section, key) {
        Some(value) => value == "1",
        None => default,
    }
}

fn flag_str(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

fn read_u32(ini: &Ini, section: &str, key: &str) -> Option<u32> {
    ini.get_i64(section, key).and_then(|v| u32::try_from(v).ok())
}

fn read_i32(ini: &Ini, section: &str, key: &str) -> Option<i32> {
    ini.get_i64(section, key).and_then(|v| i32::try_from(v).ok())
}

/// The value if the game would accept it, otherwise the default.
fn pick<T: Copy + PartialEq>(value: Option<T>, allowed: &[T], default: T) -> T {
    match value {
        Some(v) if allowed.contains(&v) => v,
        _ => default,
    }
}

fn in_range(value: Option<u32>, range: RangeInclusive<u32>, default: u32) -> u32 {
    match value {
        Some(v) if range.contains(&v) => v,
        _ => default,
    }
}

/// `FPS_CAP_MS` to frames per second. `None` when there is no cap.
fn fps_from_ms(ms: f64) -> Option<i32> {
    let ms = ms.abs();
    if !(ms >= 0.1) {
        return None;
    }
    let fps = (1000.0 / ms).round();
    if fps >= 1.0 && fps <= i32::MAX as f64 { Some(fps as i32) } else { None }
}

/// Frames per second to `FPS_CAP_MS`. `None` (no cap) is written as 0.
fn fps_to_ms(fps: Option<i32>) -> f64 {
    match fps {
        Some(fps) if fps > 0 => 1000.0 / fps as f64,
        _ => 0.0,
    }
}

fn invalid_input(errors: Vec<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, errors.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A unique temp directory, removed on drop (also when a test panics).
    struct TempCfg(PathBuf);

    impl TempCfg {
        fn new(name: &str) -> TempCfg {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("acxs-game-cfg-{}-{n}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempCfg(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempCfg {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn backups(dir: &Path, file: &str) -> Vec<PathBuf> {
        let prefix = format!("{file}.acxs-backup-");
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(&prefix))
            .collect()
    }

    #[test]
    fn video_defaults_when_missing() {
        let dir = TempCfg::new("video-missing");
        let read = read_video(dir.path()).unwrap();
        assert_eq!(read, VideoSettings::default());
        assert!(read.fullscreen);
        assert!(!read.vsync);
        assert_eq!(read.fps_cap, None);
        assert_eq!(read.anisotropic, 8);
        assert_eq!(read.shadow_map_size, 2048);
        assert_eq!(read.world_detail, 4);
        assert!(read.post_processing);
        assert!(validate_video(&read).is_empty());
    }

    #[test]
    fn video_round_trip() {
        let dir = TempCfg::new("video-round-trip");
        let settings = VideoSettings {
            width: Some(2560),
            height: Some(1440),
            fullscreen: false,
            vsync: true,
            fps_cap: Some(60),
            aa_samples: 4,
            anisotropic: 16,
            shadow_map_size: -1,
            world_detail: 5,
            post_processing: false,
        };
        write_video(dir.path(), &settings).unwrap();
        assert_eq!(read_video(dir.path()).unwrap(), settings);
    }

    #[test]
    fn video_writes_game_key_names_and_values() {
        let dir = TempCfg::new("video-keys");
        let settings = VideoSettings { width: Some(1920), height: Some(1080), fps_cap: Some(50), ..Default::default() };
        write_video(dir.path(), &settings).unwrap();
        let text = fs::read_to_string(dir.path().join(VIDEO_FILE)).unwrap();
        assert!(text.contains("[VIDEO]\r\n"));
        assert!(text.contains("WIDTH=1920\r\n"));
        assert!(text.contains("HEIGHT=1080\r\n"));
        assert!(text.contains("FULLSCREEN=1\r\n"));
        assert!(text.contains("VSYNC=0\r\n"));
        assert!(text.contains("FPS_CAP_MS=20\r\n"));
        assert!(text.contains("[ASSETTOCORSA]\r\n"));
        assert!(text.contains("WORLD_DETAIL=4\r\n"));
        assert!(text.contains("[POST_PROCESS]\r\n"));
        assert!(text.contains("ENABLED=1\r\n"));
    }

    #[test]
    fn video_preserves_unknown_keys_and_backs_up_old_file() {
        let dir = TempCfg::new("video-preserve");
        let original = "; my comment\r\n[VIDEO]\r\nWIDTH=1280\r\nHEIGHT=720\r\nREFRESH=144\r\nINDEX=3\r\n\r\n[CUBEMAP]\r\nSIZE=1024\r\n[MYSTERY]\r\nFOO=bar\r\n";
        fs::write(dir.path().join(VIDEO_FILE), original).unwrap();

        let mut settings = read_video(dir.path()).unwrap();
        assert_eq!((settings.width, settings.height), (Some(1280), Some(720)));
        settings.width = Some(1600);
        settings.height = Some(900);
        write_video(dir.path(), &settings).unwrap();

        let text = fs::read_to_string(dir.path().join(VIDEO_FILE)).unwrap();
        assert!(text.starts_with("; my comment\r\n"));
        assert!(text.contains("WIDTH=1600\r\n"));
        assert!(text.contains("REFRESH=144\r\n"));
        assert!(text.contains("INDEX=3\r\n"));
        assert!(text.contains("[CUBEMAP]\r\nSIZE=1024\r\n"));
        assert!(text.contains("[MYSTERY]\r\nFOO=bar\r\n"));

        let old = backups(dir.path(), VIDEO_FILE);
        assert_eq!(old.len(), 1);
        assert_eq!(fs::read_to_string(&old[0]).unwrap(), original);
    }

    #[test]
    fn video_none_resolution_keeps_file_value() {
        let dir = TempCfg::new("video-none-res");
        fs::write(dir.path().join(VIDEO_FILE), "[VIDEO]\r\nWIDTH=1280\r\nHEIGHT=720\r\n").unwrap();
        let settings = VideoSettings { width: None, height: None, ..Default::default() };
        write_video(dir.path(), &settings).unwrap();
        let text = fs::read_to_string(dir.path().join(VIDEO_FILE)).unwrap();
        assert!(text.contains("WIDTH=1280\r\n"));
        assert!(text.contains("HEIGHT=720\r\n"));
    }

    #[test]
    fn video_fps_cap_converts_from_frame_time() {
        let dir = TempCfg::new("video-fps");
        fs::write(dir.path().join(VIDEO_FILE), "[VIDEO]\r\nFPS_CAP_MS=16.666666\r\n").unwrap();
        assert_eq!(read_video(dir.path()).unwrap().fps_cap, Some(60));
        fs::write(dir.path().join(VIDEO_FILE), "[VIDEO]\r\nFPS_CAP_MS=0\r\n").unwrap();
        assert_eq!(read_video(dir.path()).unwrap().fps_cap, None);
    }

    #[test]
    fn video_invalid_file_values_fall_back_to_defaults() {
        let dir = TempCfg::new("video-sanitize");
        let text = "[VIDEO]\r\nAASAMPLES=3\r\nANISOTROPIC=7\r\nSHADOW_MAP_SIZE=1000\r\nWIDTH=0\r\nHEIGHT=720\r\n[ASSETTOCORSA]\r\nWORLD_DETAIL=9\r\n";
        fs::write(dir.path().join(VIDEO_FILE), text).unwrap();
        let read = read_video(dir.path()).unwrap();
        assert_eq!(read.aa_samples, 1);
        assert_eq!(read.anisotropic, 8);
        assert_eq!(read.shadow_map_size, 2048);
        assert_eq!(read.world_detail, 4);
        assert_eq!((read.width, read.height), (None, None));
        assert!(validate_video(&read).is_empty());
    }

    #[test]
    fn video_validation_flags_impossible_values() {
        assert!(validate_video(&VideoSettings::default()).is_empty());

        let zero_width = VideoSettings { width: Some(0), height: Some(1080), ..Default::default() };
        assert!(validate_video(&zero_width).iter().any(|e| e.contains("width")));

        let zero_height = VideoSettings { width: Some(1920), height: Some(0), ..Default::default() };
        assert!(validate_video(&zero_height).iter().any(|e| e.contains("height")));

        let half_set = VideoSettings { width: Some(1920), height: None, ..Default::default() };
        assert!(validate_video(&half_set).iter().any(|e| e.contains("together")));

        let negative_fps = VideoSettings { fps_cap: Some(-30), ..Default::default() };
        assert!(validate_video(&negative_fps).iter().any(|e| e.contains("fps")));

        let zero_fps = VideoSettings { fps_cap: Some(0), ..Default::default() };
        assert_eq!(validate_video(&zero_fps).len(), 1);

        let bad_aa = VideoSettings { aa_samples: 3, ..Default::default() };
        assert!(validate_video(&bad_aa).iter().any(|e| e.contains("anti-aliasing")));

        let bad_world = VideoSettings { world_detail: 6, ..Default::default() };
        assert!(validate_video(&bad_world).iter().any(|e| e.contains("world detail")));
    }

    #[test]
    fn write_video_refuses_invalid_settings_and_writes_nothing() {
        let dir = TempCfg::new("video-refuse");
        let settings = VideoSettings { fps_cap: Some(-1), ..Default::default() };
        let err = write_video(dir.path(), &settings).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!dir.path().join(VIDEO_FILE).exists());
    }

    #[test]
    fn assetto_corsa_defaults_when_missing() {
        let dir = TempCfg::new("ac-missing");
        let read = read_assetto_corsa_ini(dir.path()).unwrap();
        assert_eq!(read, AssettoCorsaSettings::default());
        assert!(read.logging);
        assert_eq!(read.screenshot_format, "JPG");
        assert_eq!(read.mirrors_fov, 10);
        assert_eq!(read.mirrors_far_plane, 400);
        assert!(validate_assetto_corsa(&read).is_empty());
    }

    #[test]
    fn assetto_corsa_round_trip_and_keeps_other_keys() {
        let dir = TempCfg::new("ac-round-trip");
        let original = "[SOFT_LOCK]\r\nENABLED=1\r\n[FF_EXPERIMENTAL]\r\nDAMPER_GAIN=0.5\r\n[LOG]\r\nSUPPRESS=1\r\n";
        fs::write(dir.path().join(ASSETTO_CORSA_FILE), original).unwrap();

        let mut settings = read_assetto_corsa_ini(dir.path()).unwrap();
        assert!(!settings.logging);
        settings.logging = true;
        settings.simulation_value = 50;
        settings.screenshot_format = "PNG".to_string();
        settings.mirrors_fov = 45;
        settings.hide_driver = true;
        write_assetto_corsa_ini(dir.path(), &settings).unwrap();

        assert_eq!(read_assetto_corsa_ini(dir.path()).unwrap(), settings);
        let text = fs::read_to_string(dir.path().join(ASSETTO_CORSA_FILE)).unwrap();
        assert!(text.contains("[SOFT_LOCK]\r\nENABLED=1\r\n"));
        assert!(text.contains("[FF_EXPERIMENTAL]\r\nDAMPER_GAIN=0.5\r\n"));
        assert!(text.contains("SUPPRESS=0\r\n"));
        assert_eq!(backups(dir.path(), ASSETTO_CORSA_FILE).len(), 1);
    }

    #[test]
    fn assetto_corsa_reads_game_encoding() {
        let dir = TempCfg::new("ac-encoding");
        let text = "[ASSETTO_CORSA]\r\nSIMULATION_VALUE=0.25\r\n[LOG]\r\nSUPPRESS=0\r\n[SCREENSHOT]\r\nFORMAT=TGA\r\n[MIRRORS]\r\nFOV=0\r\nFAR_PLANE=400\r\n";
        fs::write(dir.path().join(ASSETTO_CORSA_FILE), text).unwrap();
        let read = read_assetto_corsa_ini(dir.path()).unwrap();
        assert_eq!(read.simulation_value, 25);
        assert!(read.logging);
        assert_eq!(read.screenshot_format, "JPG");
        assert_eq!(read.mirrors_fov, 10);
        assert_eq!(read.mirrors_far_plane, 400);
    }

    #[test]
    fn assetto_corsa_validation_flags_impossible_values() {
        let bad_sim = AssettoCorsaSettings { simulation_value: 101, ..Default::default() };
        assert_eq!(validate_assetto_corsa(&bad_sim).len(), 1);

        let bad_fov = AssettoCorsaSettings { mirrors_fov: 0, ..Default::default() };
        assert!(validate_assetto_corsa(&bad_fov).iter().any(|e| e.contains("field of view")));

        let bad_far = AssettoCorsaSettings { mirrors_far_plane: 5, ..Default::default() };
        assert!(validate_assetto_corsa(&bad_far).iter().any(|e| e.contains("far plane")));

        let bad_format = AssettoCorsaSettings { screenshot_format: "TGA".to_string(), ..Default::default() };
        assert!(validate_assetto_corsa(&bad_format).iter().any(|e| e.contains("screenshot")));

        let dir = TempCfg::new("ac-refuse");
        assert!(write_assetto_corsa_ini(dir.path(), &bad_fov).is_err());
        assert!(!dir.path().join(ASSETTO_CORSA_FILE).exists());
    }
}
