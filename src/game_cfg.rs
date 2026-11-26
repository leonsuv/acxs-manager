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

use serde::{Deserialize, Serialize};
use std::io;
use std::ops::RangeInclusive;
use std::path::Path;

use crate::ini::Ini;

pub const VIDEO_FILE: &str = "video.ini";
pub const ASSETTO_CORSA_FILE: &str = "assetto_corsa.ini";
pub const TRIPLE_SCREEN_FILE: &str = "triple_screen.ini";
pub const OCULUS_FILE: &str = "oculus.ini";
pub const OPENVR_FILE: &str = "openvr.ini";
pub const CAMERA_ONBOARD_FREE_FILE: &str = "camera_onboard_free.ini";
pub const REPLAY_FILE: &str = "replay.ini";

/// `[CAMERA] MODE` in `video.ini`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CameraMode {
    #[default]
    Default,
    Triple,
    Oculus,
    OpenVr,
}

impl CameraMode {
    pub const ALL: [CameraMode; 4] = [
        CameraMode::Default,
        CameraMode::Triple,
        CameraMode::Oculus,
        CameraMode::OpenVr,
    ];

    pub fn as_ini_str(self) -> &'static str {
        match self {
            CameraMode::Default => "DEFAULT",
            CameraMode::Triple => "TRIPLE",
            CameraMode::Oculus => "OCULUS",
            CameraMode::OpenVr => "OPENVR",
        }
    }

    pub fn from_ini_str(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "TRIPLE" => CameraMode::Triple,
            "OCULUS" => CameraMode::Oculus,
            "OPENVR" => CameraMode::OpenVr,
            _ => CameraMode::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CameraMode::Default => "Single Screen",
            CameraMode::Triple => "Triple Screen",
            CameraMode::Oculus => "Oculus Rift / Meta",
            CameraMode::OpenVr => "OpenVR / OpenXR",
        }
    }

    pub fn is_vr(self) -> bool {
        matches!(self, CameraMode::Oculus | CameraMode::OpenVr)
    }
}

const CUBEMAP_SIZES: &[u32] = &[0, 256, 512, 1024, 2048];
const MIRROR_SIZES: &[u32] = &[256, 512, 1024, 2048];
const REPLAY_FPS_VALUES: &[u32] = &[10, 15, 20, 30, 60];

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    /// `[VIDEO] WIDTH`. `None` when the file has no usable resolution.
    pub width: Option<u32>,
    /// `[VIDEO] HEIGHT`. `None` when the file has no usable resolution.
    pub height: Option<u32>,
    /// `[VIDEO] REFRESH` in Hz. `None` leaves the existing key untouched.
    pub refresh_hz: Option<u32>,
    /// `[VIDEO] INDEX` (display index). `None` leaves the existing key untouched.
    pub display_index: Option<u32>,
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
    /// `[CAMERA] MODE` (Single / Triple / Oculus / OpenVR). Default `Default`.
    pub camera_mode: CameraMode,
    /// `[EFFECTS] FXAA`. Default `false`.
    pub fxaa: bool,
    /// `[EFFECTS] MOTION_BLUR`, 0 to 12. Default `0`.
    pub motion_blur: u32,
    /// `[EFFECTS] SMOKE`, 0 to 5. Default `3`.
    pub smoke: u32,
    /// `[EFFECTS] RENDER_SMOKE_IN_MIRROR`. Default `false`.
    pub render_smoke_in_mirror: bool,
    /// `[CUBEMAP] SIZE`, one of 0, 256, 512, 1024, 2048. Default `512`.
    pub cubemap_size: u32,
    /// `[CUBEMAP] FACES_PER_FRAME`, 0 to 6. Default `2`.
    pub cubemap_faces_per_frame: u32,
    /// `[CUBEMAP] FARPLANE`, 100 to 1500. Default `500`.
    pub cubemap_farplane: u32,
    /// `[MIRROR] SIZE`, one of 256, 512, 1024, 2048. Default `512`.
    pub mirror_size: u32,
    /// `[MIRROR] HQ`. Default `false`.
    pub mirror_hq: bool,
    /// `[POST_PROCESS] GLARE`, 0 to 5. Default `2`.
    pub glare: u32,
    /// `[POST_PROCESS] DOF`, 0 to 5. Default `2`.
    pub dof: u32,
    /// `[POST_PROCESS] RAYS_OF_GOD`. Default `true`.
    pub rays_of_god: bool,
    /// `[POST_PROCESS] HEAT_SHIMMER`. Default `true`.
    pub heat_shimmer: bool,
    /// `[POST_PROCESS] SATURATION`, 0 to 200. Default `100`.
    pub saturation: u32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        VideoSettings {
            width: None,
            height: None,
            refresh_hz: None,
            display_index: None,
            fullscreen: true,
            vsync: false,
            fps_cap: None,
            aa_samples: 1,
            anisotropic: 8,
            shadow_map_size: 2048,
            world_detail: 4,
            post_processing: true,
            camera_mode: CameraMode::Default,
            fxaa: false,
            motion_blur: 0,
            smoke: 3,
            render_smoke_in_mirror: false,
            cubemap_size: 512,
            cubemap_faces_per_frame: 2,
            cubemap_farplane: 500,
            mirror_size: 512,
            mirror_hq: false,
            glare: 2,
            dof: 2,
            rays_of_god: true,
            heat_shimmer: true,
            saturation: 100,
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
    if let Some(fps) = settings.fps_cap && fps <= 0 {
        errors.push(format!("fps cap must be positive, got {fps} (use None for no cap)"));
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
        refresh_hz: read_u32(ini, "VIDEO", "REFRESH").filter(|&v| v > 0),
        display_index: read_u32(ini, "VIDEO", "INDEX"),
        fullscreen: read_flag(ini, "VIDEO", "FULLSCREEN", d.fullscreen),
        vsync: read_flag(ini, "VIDEO", "VSYNC", d.vsync),
        fps_cap,
        aa_samples: pick(read_u32(ini, "VIDEO", "AASAMPLES"), AA_SAMPLES, d.aa_samples),
        anisotropic: pick(read_u32(ini, "VIDEO", "ANISOTROPIC"), ANISOTROPIC, d.anisotropic),
        shadow_map_size: pick(read_i32(ini, "VIDEO", shadow_key), SHADOW_MAP_SIZES, d.shadow_map_size),
        world_detail: pick(read_u32(ini, "ASSETTOCORSA", "WORLD_DETAIL"), WORLD_DETAILS, d.world_detail),
        post_processing: read_flag(ini, "POST_PROCESS", "ENABLED", d.post_processing),
        camera_mode: ini
            .get("CAMERA", "MODE")
            .map(CameraMode::from_ini_str)
            .unwrap_or(d.camera_mode),
        fxaa: read_flag(ini, "EFFECTS", "FXAA", d.fxaa),
        motion_blur: in_range(read_u32(ini, "EFFECTS", "MOTION_BLUR"), 0..=12, d.motion_blur),
        smoke: in_range(read_u32(ini, "EFFECTS", "SMOKE"), 0..=5, d.smoke),
        render_smoke_in_mirror: read_flag(ini, "EFFECTS", "RENDER_SMOKE_IN_MIRROR", d.render_smoke_in_mirror),
        cubemap_size: pick(read_u32(ini, "CUBEMAP", "SIZE"), CUBEMAP_SIZES, d.cubemap_size),
        cubemap_faces_per_frame: in_range(read_u32(ini, "CUBEMAP", "FACES_PER_FRAME"), 0..=6, d.cubemap_faces_per_frame),
        cubemap_farplane: in_range(read_u32(ini, "CUBEMAP", "FARPLANE"), 100..=1500, d.cubemap_farplane),
        mirror_size: pick(read_u32(ini, "MIRROR", "SIZE"), MIRROR_SIZES, d.mirror_size),
        mirror_hq: read_flag(ini, "MIRROR", "HQ", d.mirror_hq),
        glare: in_range(read_u32(ini, "POST_PROCESS", "GLARE"), 0..=5, d.glare),
        dof: in_range(read_u32(ini, "POST_PROCESS", "DOF"), 0..=5, d.dof),
        rays_of_god: read_flag(ini, "POST_PROCESS", "RAYS_OF_GOD", d.rays_of_god),
        heat_shimmer: read_flag(ini, "POST_PROCESS", "HEAT_SHIMMER", d.heat_shimmer),
        saturation: in_range(read_u32(ini, "POST_PROCESS", "SATURATION"), 0..=200, d.saturation),
    }
}

fn apply_video(ini: &mut Ini, s: &VideoSettings) {
    if let (Some(width), Some(height)) = (s.width, s.height) {
        ini.set("VIDEO", "WIDTH", width);
        ini.set("VIDEO", "HEIGHT", height);
    }
    if let Some(hz) = s.refresh_hz {
        ini.set("VIDEO", "REFRESH", hz);
    }
    if let Some(idx) = s.display_index {
        ini.set("VIDEO", "INDEX", idx);
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
    ini.set("CAMERA", "MODE", s.camera_mode.as_ini_str());
    ini.set("EFFECTS", "FXAA", flag_str(s.fxaa));
    ini.set("EFFECTS", "MOTION_BLUR", s.motion_blur);
    ini.set("EFFECTS", "SMOKE", s.smoke);
    ini.set("EFFECTS", "RENDER_SMOKE_IN_MIRROR", flag_str(s.render_smoke_in_mirror));
    ini.set("CUBEMAP", "SIZE", s.cubemap_size);
    ini.set("CUBEMAP", "FACES_PER_FRAME", s.cubemap_faces_per_frame);
    ini.set("CUBEMAP", "FARPLANE", s.cubemap_farplane);
    ini.set("MIRROR", "SIZE", s.mirror_size);
    ini.set("MIRROR", "HQ", flag_str(s.mirror_hq));
    ini.set("POST_PROCESS", "ENABLED", flag_str(s.post_processing));
    ini.set("POST_PROCESS", "GLARE", s.glare);
    ini.set("POST_PROCESS", "DOF", s.dof);
    ini.set("POST_PROCESS", "RAYS_OF_GOD", flag_str(s.rays_of_god));
    ini.set("POST_PROCESS", "HEAT_SHIMMER", flag_str(s.heat_shimmer));
    ini.set("POST_PROCESS", "SATURATION", s.saturation);
}

// ---------------------------------------------------------------------------
// triple_screen.ini, oculus.ini / openvr.ini, camera_onboard_free.ini, replay.ini
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TripleScreenSettings {
    pub width_mm: f64,
    pub height_mm: f64,
    pub distance_mm: f64,
    pub angle_deg: f64,
    pub bezel_mm: f64,
    pub margin_l_mm: f64,
    pub margin_r_mm: f64,
    pub rotation_deg: f64,
    pub sp_width_px: u32,
}

impl Default for TripleScreenSettings {
    fn default() -> Self {
        Self {
            width_mm: 597.0,
            height_mm: 336.0,
            distance_mm: 650.0,
            angle_deg: 45.0,
            bezel_mm: 10.0,
            margin_l_mm: 0.0,
            margin_r_mm: 0.0,
            rotation_deg: 0.0,
            sp_width_px: 1920,
        }
    }
}

pub fn read_triple_screen(cfg_dir: &Path) -> io::Result<TripleScreenSettings> {
    let ini = load_or_empty(&cfg_dir.join(TRIPLE_SCREEN_FILE))?;
    let d = TripleScreenSettings::default();
    Ok(TripleScreenSettings {
        width_mm: ini.get_f64("TRIPLE_SCREEN", "WIDTH").unwrap_or(d.width_mm),
        height_mm: ini.get_f64("TRIPLE_SCREEN", "HEIGHT").unwrap_or(d.height_mm),
        distance_mm: ini.get_f64("TRIPLE_SCREEN", "DISTANCE").unwrap_or(d.distance_mm),
        angle_deg: ini.get_f64("TRIPLE_SCREEN", "ANGLE").unwrap_or(d.angle_deg),
        bezel_mm: ini.get_f64("TRIPLE_SCREEN", "BEZEL").unwrap_or(d.bezel_mm),
        margin_l_mm: ini.get_f64("TRIPLE_SCREEN", "MARGIN_L").unwrap_or(d.margin_l_mm),
        margin_r_mm: ini.get_f64("TRIPLE_SCREEN", "MARGIN_R").unwrap_or(d.margin_r_mm),
        rotation_deg: ini.get_f64("TRIPLE_SCREEN", "ROTATION").unwrap_or(d.rotation_deg),
        sp_width_px: read_u32(&ini, "TRIPLE_SCREEN", "SP_WIDTH").unwrap_or(d.sp_width_px),
    })
}

pub fn write_triple_screen(cfg_dir: &Path, s: &TripleScreenSettings) -> io::Result<()> {
    if crate::game::is_game_running() {
        return Err(io::Error::new(io::ErrorKind::ResourceBusy, "Assetto Corsa is running"));
    }
    if !(100.0..=3000.0).contains(&s.width_mm)
        || !(100.0..=3000.0).contains(&s.distance_mm)
        || !(0.0..=89.0).contains(&s.angle_deg)
    {
        return Err(invalid_input(vec!["Triple screen geometry values are out of range".into()]));
    }
    let path = cfg_dir.join(TRIPLE_SCREEN_FILE);
    let mut ini = load_or_empty(&path)?;
    ini.set("TRIPLE_SCREEN", "WIDTH", s.width_mm);
    ini.set("TRIPLE_SCREEN", "HEIGHT", s.height_mm);
    ini.set("TRIPLE_SCREEN", "DISTANCE", s.distance_mm);
    ini.set("TRIPLE_SCREEN", "ANGLE", s.angle_deg);
    ini.set("TRIPLE_SCREEN", "BEZEL", s.bezel_mm);
    ini.set("TRIPLE_SCREEN", "MARGIN_L", s.margin_l_mm);
    ini.set("TRIPLE_SCREEN", "MARGIN_R", s.margin_r_mm);
    ini.set("TRIPLE_SCREEN", "ROTATION", s.rotation_deg);
    ini.set("TRIPLE_SCREEN", "SP_WIDTH", s.sp_width_px);
    ini.save_with_backup(&path)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VrSettings {
    /// `[SETTINGS] PIXEL_PER_DISPLAY`, 0.5 to 2.5.
    pub pixel_per_display: f64,
    /// `[SETTINGS] MIRROR_TEXTURE`.
    pub mirror_texture: bool,
    /// `[SETTINGS] AUTOSELECT_RIFT_AUDIO_DEVICE`.
    pub autoselect_audio: bool,
    /// `[MIRROR] ENABLE`.
    pub mirror_enable: bool,
}

impl Default for VrSettings {
    fn default() -> Self {
        Self {
            pixel_per_display: 1.0,
            mirror_texture: true,
            autoselect_audio: true,
            mirror_enable: true,
        }
    }
}

pub fn validate_vr(s: &VrSettings) -> Vec<String> {
    let mut errors = Vec::new();
    if !s.pixel_per_display.is_finite() || !(0.5..=2.5).contains(&s.pixel_per_display) {
        errors.push(format!("pixel_per_display must be 0.5 to 2.5, got {}", s.pixel_per_display));
    }
    errors
}

pub fn read_vr(cfg_dir: &Path, openvr: bool) -> io::Result<VrSettings> {
    let file = if openvr { OPENVR_FILE } else { OCULUS_FILE };
    let ini = load_or_empty(&cfg_dir.join(file))?;
    let d = VrSettings::default();
    Ok(VrSettings {
        pixel_per_display: ini
            .get_f64("SETTINGS", "PIXEL_PER_DISPLAY")
            .filter(|v| (0.5..=2.5).contains(v))
            .unwrap_or(d.pixel_per_display),
        mirror_texture: read_flag(&ini, "SETTINGS", "MIRROR_TEXTURE", d.mirror_texture),
        autoselect_audio: read_flag(&ini, "SETTINGS", "AUTOSELECT_RIFT_AUDIO_DEVICE", d.autoselect_audio),
        mirror_enable: read_flag(&ini, "MIRROR", "ENABLE", d.mirror_enable),
    })
}

pub fn write_vr(cfg_dir: &Path, openvr: bool, s: &VrSettings) -> io::Result<()> {
    if crate::game::is_game_running() {
        return Err(io::Error::new(io::ErrorKind::ResourceBusy, "Assetto Corsa is running"));
    }
    let errors = validate_vr(s);
    if !errors.is_empty() {
        return Err(invalid_input(errors));
    }
    let file = if openvr { OPENVR_FILE } else { OCULUS_FILE };
    let path = cfg_dir.join(file);
    let mut ini = load_or_empty(&path)?;
    ini.set("SETTINGS", "PIXEL_PER_DISPLAY", format!("{:.2}", s.pixel_per_display));
    ini.set("SETTINGS", "MIRROR_TEXTURE", flag_str(s.mirror_texture));
    ini.set("SETTINGS", "AUTOSELECT_RIFT_AUDIO_DEVICE", flag_str(s.autoselect_audio));
    ini.set("MIRROR", "ENABLE", flag_str(s.mirror_enable));
    ini.save_with_backup(&path)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraOnboardSettings {
    pub fov: f64,
    pub lock_to_horizon: bool,
    pub gforce_x: f64,
    pub gforce_y: f64,
    pub gforce_z: f64,
    pub look_ahead: f64,
}

impl Default for CameraOnboardSettings {
    fn default() -> Self {
        Self {
            fov: 54.0,
            lock_to_horizon: false,
            gforce_x: 0.25,
            gforce_y: 0.15,
            gforce_z: 0.50,
            look_ahead: 0.0,
        }
    }
}

pub fn read_camera_onboard(cfg_dir: &Path) -> io::Result<CameraOnboardSettings> {
    let ini = load_or_empty(&cfg_dir.join(CAMERA_ONBOARD_FREE_FILE))?;
    let d = CameraOnboardSettings::default();
    Ok(CameraOnboardSettings {
        fov: ini.get_f64("CAMERA_SETTINGS", "FOV").filter(|v| (10.0..=120.0).contains(v)).unwrap_or(d.fov),
        lock_to_horizon: read_flag(&ini, "CAMERA_SETTINGS", "LOCK_TO_HORIZON", d.lock_to_horizon),
        gforce_x: ini.get_f64("GFORCE", "GFORCE_X").unwrap_or(d.gforce_x),
        gforce_y: ini.get_f64("GFORCE", "GFORCE_Y").unwrap_or(d.gforce_y),
        gforce_z: ini.get_f64("GFORCE", "GFORCE_Z").unwrap_or(d.gforce_z),
        look_ahead: ini.get_f64("CAMERA_SETTINGS", "LOOK_AHEAD").unwrap_or(d.look_ahead),
    })
}

pub fn write_camera_onboard(cfg_dir: &Path, s: &CameraOnboardSettings) -> io::Result<()> {
    if crate::game::is_game_running() {
        return Err(io::Error::new(io::ErrorKind::ResourceBusy, "Assetto Corsa is running"));
    }
    if !(10.0..=120.0).contains(&s.fov) {
        return Err(invalid_input(vec![format!("FOV must be 10 to 120, got {}", s.fov)]));
    }
    let path = cfg_dir.join(CAMERA_ONBOARD_FREE_FILE);
    let mut ini = load_or_empty(&path)?;
    ini.set("CAMERA_SETTINGS", "FOV", s.fov);
    ini.set("CAMERA_SETTINGS", "LOCK_TO_HORIZON", flag_str(s.lock_to_horizon));
    ini.set("CAMERA_SETTINGS", "LOOK_AHEAD", s.look_ahead);
    ini.set("GFORCE", "GFORCE_X", s.gforce_x);
    ini.set("GFORCE", "GFORCE_Y", s.gforce_y);
    ini.set("GFORCE", "GFORCE_Z", s.gforce_z);
    ini.save_with_backup(&path)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplaySettings {
    /// `[QUALITY] FPS`, one of 10, 15, 20, 30, 60.
    pub fps: u32,
    /// `[REPLAY] MAX_SIZE_MB`, 50 to 2048.
    pub max_size_mb: u32,
    /// `[REPLAY] AUTOSAVE`.
    pub autosave: bool,
}

impl Default for ReplaySettings {
    fn default() -> Self {
        Self {
            fps: 30,
            max_size_mb: 500,
            autosave: true,
        }
    }
}

pub fn read_replay_settings(cfg_dir: &Path) -> io::Result<ReplaySettings> {
    let ini = load_or_empty(&cfg_dir.join(REPLAY_FILE))?;
    let d = ReplaySettings::default();
    Ok(ReplaySettings {
        fps: pick(read_u32(&ini, "QUALITY", "FPS"), REPLAY_FPS_VALUES, d.fps),
        max_size_mb: in_range(read_u32(&ini, "REPLAY", "MAX_SIZE_MB"), 50..=2048, d.max_size_mb),
        autosave: read_flag(&ini, "REPLAY", "AUTOSAVE", d.autosave),
    })
}

pub fn write_replay_settings(cfg_dir: &Path, s: &ReplaySettings) -> io::Result<()> {
    if crate::game::is_game_running() {
        return Err(io::Error::new(io::ErrorKind::ResourceBusy, "Assetto Corsa is running"));
    }
    if !REPLAY_FPS_VALUES.contains(&s.fps) || !(50..=2048).contains(&s.max_size_mb) {
        return Err(invalid_input(vec!["Replay FPS or max_size_mb out of range".into()]));
    }
    let path = cfg_dir.join(REPLAY_FILE);
    let mut ini = load_or_empty(&path)?;
    ini.set("QUALITY", "FPS", s.fps);
    ini.set("REPLAY", "MAX_SIZE_MB", s.max_size_mb);
    ini.set("REPLAY", "AUTOSAVE", flag_str(s.autosave));
    ini.save_with_backup(&path)
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
    if ms.is_nan() || ms < 0.1 {
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
            ..VideoSettings::default()
        };
        write_video(dir.path(), &settings).unwrap();
        assert_eq!(read_video(dir.path()).unwrap(), settings);
    }

    #[test]
    fn vr_triple_camera_and_replay_round_trip() {
        let dir = TempCfg::new("vr-triple-replay");
        let vid = VideoSettings {
            camera_mode: CameraMode::OpenVr,
            fxaa: true,
            cubemap_size: 1024,
            mirror_hq: true,
            ..VideoSettings::default()
        };
        write_video(dir.path(), &vid).unwrap();
        let got_vid = read_video(dir.path()).unwrap();
        assert_eq!(got_vid.camera_mode, CameraMode::OpenVr);
        assert!(got_vid.fxaa);
        assert_eq!(got_vid.cubemap_size, 1024);
        assert!(got_vid.mirror_hq);

        let ts = TripleScreenSettings {
            angle_deg: 55.0,
            bezel_mm: 12.0,
            ..TripleScreenSettings::default()
        };
        write_triple_screen(dir.path(), &ts).unwrap();
        assert_eq!(read_triple_screen(dir.path()).unwrap(), ts);

        let vr = VrSettings {
            pixel_per_display: 1.35,
            mirror_texture: false,
            ..VrSettings::default()
        };
        write_vr(dir.path(), true, &vr).unwrap();
        assert!((read_vr(dir.path(), true).unwrap().pixel_per_display - 1.35).abs() < 1e-6);
        assert!(write_vr(dir.path(), true, &VrSettings { pixel_per_display: 4.0, ..VrSettings::default() }).is_err());

        let cam = CameraOnboardSettings {
            fov: 48.0,
            lock_to_horizon: true,
            ..CameraOnboardSettings::default()
        };
        write_camera_onboard(dir.path(), &cam).unwrap();
        assert_eq!(read_camera_onboard(dir.path()).unwrap(), cam);

        let rep = ReplaySettings {
            fps: 60,
            max_size_mb: 1024,
            autosave: false,
        };
        write_replay_settings(dir.path(), &rep).unwrap();
        assert_eq!(read_replay_settings(dir.path()).unwrap(), rep);
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
