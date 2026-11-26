//! App settings, video/VR/triple/controls/FFB configuration, rig profiles, CSP management,
//! URI protocols, updates, changelog, and config backup snapshots.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use acxs_manager::backup;
use acxs_manager::controls::{self, ControlsConfig};
use acxs_manager::csp;
use acxs_manager::csp_config;
use acxs_manager::csp_online;
use acxs_manager::ffb::{self, FfbPostProcess, FfbTweaks};
use acxs_manager::game;
use acxs_manager::game_cfg::{self, TripleScreenSettings, VideoSettings, VrSettings};
use acxs_manager::rig_profiles::{self, RigProfileStore};
use acxs_manager::settings::{self, AccentPreset, QuickSwitches, Settings};
use acxs_manager::update::{self, Version};
use acxs_manager::uri_register;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ac_root_from_state(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let s = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    s.ac_root_path()
        .ok_or_else(|| "Assetto Corsa root folder is not configured".to_string())
}

fn require_user_cfg_dir() -> Result<PathBuf, String> {
    settings::ac_user_cfg_dir()
        .ok_or_else(|| "Assetto Corsa user cfg directory could not be located".to_string())
}

fn rig_profiles_path() -> PathBuf {
    settings::settings_path()
        .parent()
        .map(|p| p.join("rig_profiles.json"))
        .unwrap_or_else(|| PathBuf::from("rig_profiles.json"))
}

fn validate_flat_filename(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        return Err(format!("Invalid config file name: {name}"));
    }
    Ok(trimmed)
}

fn accent_to_str(accent: AccentPreset) -> &'static str {
    match accent {
        AccentPreset::MotorsportRed => "motorsportRed",
        AccentPreset::TelemetryCyan => "telemetryCyan",
        AccentPreset::AmberGold => "amberGold",
        AccentPreset::Emerald => "emerald",
    }
}

fn str_to_accent(s: &str) -> AccentPreset {
    match s.to_ascii_lowercase().as_str() {
        "telemetrycyan" | "telemetry_cyan" => AccentPreset::TelemetryCyan,
        "ambergold" | "amber_gold" => AccentPreset::AmberGold,
        "emerald" => AccentPreset::Emerald,
        _ => AccentPreset::MotorsportRed,
    }
}

// ---------------------------------------------------------------------------
// 1. App Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickSwitchesDto {
    #[serde(default)]
    pub vr_mode: bool,
    #[serde(default)]
    pub show_fps: bool,
    #[serde(default)]
    pub auto_record: bool,
    #[serde(default)]
    pub skip_intro: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub ac_root: String,
    pub patch_library: String,
    pub crash_reports: bool,
    pub accent_preset: String,
    pub quick_switches: QuickSwitchesDto,
    pub active_rig_profile: Option<String>,
    pub adaptive_ai_enabled: bool,
    pub seven_zip_path: Option<String>,
    pub mods_dir: Option<String>,
    pub csp_catalog_url: Option<String>,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        Self {
            ac_root: s.ac_root.clone(),
            patch_library: s.patch_library.clone(),
            crash_reports: s.crash_reports,
            accent_preset: accent_to_str(s.accent_preset).to_string(),
            quick_switches: QuickSwitchesDto {
                vr_mode: s.quick_switches.vr_mode,
                show_fps: s.quick_switches.show_fps,
                auto_record: s.quick_switches.auto_record,
                skip_intro: s.quick_switches.skip_intro,
            },
            active_rig_profile: s.active_rig_profile.clone(),
            adaptive_ai_enabled: s.adaptive_ai_enabled,
            seven_zip_path: s
                .seven_zip_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            mods_dir: s.mods_dir.as_ref().map(|p| p.to_string_lossy().into_owned()),
            csp_catalog_url: s.csp_catalog_url.clone(),
        }
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<SettingsDto, String> {
    let s = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    Ok(SettingsDto::from(&*s))
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    settings: SettingsDto,
) -> Result<SettingsDto, String> {
    let mut guard = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;

    guard.ac_root = settings.ac_root;
    guard.patch_library = settings.patch_library;
    guard.crash_reports = settings.crash_reports;
    guard.accent_preset = str_to_accent(&settings.accent_preset);
    guard.quick_switches = QuickSwitches {
        vr_mode: settings.quick_switches.vr_mode,
        show_fps: settings.quick_switches.show_fps,
        auto_record: settings.quick_switches.auto_record,
        skip_intro: settings.quick_switches.skip_intro,
    };
    guard.active_rig_profile = settings.active_rig_profile;
    guard.adaptive_ai_enabled = settings.adaptive_ai_enabled;
    guard.seven_zip_path = settings
        .seven_zip_path
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    guard.mods_dir = settings
        .mods_dir
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    guard.csp_catalog_url = settings.csp_catalog_url.filter(|s| !s.trim().is_empty());

    guard
        .save(&settings::settings_path())
        .map_err(|e| e.to_string())?;
    Ok(SettingsDto::from(&*guard))
}

// ---------------------------------------------------------------------------
// 2. Video, VR & Triple Screen Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VrTripleSettingsDto {
    pub triple_screen: TripleScreenSettings,
    pub oculus: VrSettings,
    pub openvr: VrSettings,
}

#[tauri::command]
pub fn get_video_settings() -> Result<VideoSettings, String> {
    let cfg_dir = require_user_cfg_dir()?;
    game_cfg::read_video(&cfg_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_video_settings(cfg: VideoSettings) -> Result<(), String> {
    let errors = game_cfg::validate_video(&cfg);
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let cfg_dir = require_user_cfg_dir()?;
    game_cfg::write_video(&cfg_dir, &cfg).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_vr_triple_settings() -> Result<VrTripleSettingsDto, String> {
    let cfg_dir = require_user_cfg_dir()?;
    let triple_screen = game_cfg::read_triple_screen(&cfg_dir).map_err(|e| e.to_string())?;
    let oculus = game_cfg::read_vr(&cfg_dir, false).map_err(|e| e.to_string())?;
    let openvr = game_cfg::read_vr(&cfg_dir, true).map_err(|e| e.to_string())?;
    Ok(VrTripleSettingsDto {
        triple_screen,
        oculus,
        openvr,
    })
}

#[tauri::command]
pub fn save_vr_triple_settings(cfg: VrTripleSettingsDto) -> Result<(), String> {
    let mut errors = game_cfg::validate_vr(&cfg.oculus);
    errors.extend(game_cfg::validate_vr(&cfg.openvr));
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let cfg_dir = require_user_cfg_dir()?;
    game_cfg::write_triple_screen(&cfg_dir, &cfg.triple_screen).map_err(|e| e.to_string())?;
    game_cfg::write_vr(&cfg_dir, false, &cfg.oculus).map_err(|e| e.to_string())?;
    game_cfg::write_vr(&cfg_dir, true, &cfg.openvr).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Controls & FFB Configuration
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_controls_config() -> Result<ControlsConfig, String> {
    let cfg_dir = require_user_cfg_dir()?;
    controls::read_controls(&cfg_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_controls_config(cfg: ControlsConfig) -> Result<(), String> {
    if game::is_game_running() {
        return Err("Assetto Corsa is running; close it before saving controls.".into());
    }
    let cfg_dir = require_user_cfg_dir()?;
    controls::write_controls(&cfg_dir, &cfg).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfbTweaksDto {
    pub basic_enabled: bool,
    pub gyro2_enabled: bool,
    pub range_compression: f64,
    pub range_compression_assist: bool,
    pub clean_ffb: bool,
    pub dirt_vibration: f64,
    pub postprocessing_script_enabled: bool,
    pub soft_lock_enabled: bool,
    pub soft_lock_padding: f64,
    pub soft_lock_shift_padding: f64,
    pub soft_lock_force_factor: f64,
    pub soft_lock_speed_factor: f64,
    pub soft_lock_damper: f64,
    pub real_feel_enabled: bool,
    pub real_feel_wheel_max_nm: f64,
    pub real_feel_enable_for_all: bool,
    pub real_feel_brake_enabled: bool,
    pub brake_max_kg: f64,
    pub brake_max_inactive_perc: f64,
    pub brake_clamp_forces: bool,
    pub adaptive_ffb: f64,
    pub average_skipped_ffb: bool,
    pub skip_damping_frames: f64,
    pub strut_ffb_adjustment: bool,
    pub fade_in_after_loading: bool,
}

impl From<FfbTweaks> for FfbTweaksDto {
    fn from(t: FfbTweaks) -> Self {
        Self {
            basic_enabled: t.basic_enabled,
            gyro2_enabled: t.gyro2_enabled,
            range_compression: t.range_compression,
            range_compression_assist: t.range_compression_assist,
            clean_ffb: t.clean_ffb,
            dirt_vibration: t.dirt_vibration,
            postprocessing_script_enabled: t.postprocessing_script_enabled,
            soft_lock_enabled: t.soft_lock_enabled,
            soft_lock_padding: t.soft_lock_padding,
            soft_lock_shift_padding: t.soft_lock_shift_padding,
            soft_lock_force_factor: t.soft_lock_force_factor,
            soft_lock_speed_factor: t.soft_lock_speed_factor,
            soft_lock_damper: t.soft_lock_damper,
            real_feel_enabled: t.real_feel_enabled,
            real_feel_wheel_max_nm: t.real_feel_wheel_max_nm,
            real_feel_enable_for_all: t.real_feel_enable_for_all,
            real_feel_brake_enabled: t.real_feel_brake_enabled,
            brake_max_kg: t.brake_max_kg,
            brake_max_inactive_perc: t.brake_max_inactive_perc,
            brake_clamp_forces: t.brake_clamp_forces,
            adaptive_ffb: t.adaptive_ffb,
            average_skipped_ffb: t.average_skipped_ffb,
            skip_damping_frames: t.skip_damping_frames,
            strut_ffb_adjustment: t.strut_ffb_adjustment,
            fade_in_after_loading: t.fade_in_after_loading,
        }
    }
}

impl From<FfbTweaksDto> for FfbTweaks {
    fn from(t: FfbTweaksDto) -> Self {
        Self {
            basic_enabled: t.basic_enabled,
            gyro2_enabled: t.gyro2_enabled,
            range_compression: t.range_compression,
            range_compression_assist: t.range_compression_assist,
            clean_ffb: t.clean_ffb,
            dirt_vibration: t.dirt_vibration,
            postprocessing_script_enabled: t.postprocessing_script_enabled,
            soft_lock_enabled: t.soft_lock_enabled,
            soft_lock_padding: t.soft_lock_padding,
            soft_lock_shift_padding: t.soft_lock_shift_padding,
            soft_lock_force_factor: t.soft_lock_force_factor,
            soft_lock_speed_factor: t.soft_lock_speed_factor,
            soft_lock_damper: t.soft_lock_damper,
            real_feel_enabled: t.real_feel_enabled,
            real_feel_wheel_max_nm: t.real_feel_wheel_max_nm,
            real_feel_enable_for_all: t.real_feel_enable_for_all,
            real_feel_brake_enabled: t.real_feel_brake_enabled,
            brake_max_kg: t.brake_max_kg,
            brake_max_inactive_perc: t.brake_max_inactive_perc,
            brake_clamp_forces: t.brake_clamp_forces,
            adaptive_ffb: t.adaptive_ffb,
            average_skipped_ffb: t.average_skipped_ffb,
            skip_damping_frames: t.skip_damping_frames,
            strut_ffb_adjustment: t.strut_ffb_adjustment,
            fade_in_after_loading: t.fade_in_after_loading,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfbConfigDto {
    pub tweaks: FfbTweaksDto,
    pub post_process: FfbPostProcess,
    pub per_car_gain: BTreeMap<String, f64>,
}

#[tauri::command]
pub fn get_ffb_config(state: State<'_, AppState>) -> Result<FfbConfigDto, String> {
    let ac_root = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        s.ac_root_path()
    };
    let user_ext_dir = csp_config::user_config_dir();
    let user_ffb = user_ext_dir.as_ref().map(|d| d.join("ffb_tweaks.ini"));

    let tweaks = match ac_root.as_deref() {
        Some(root) => ffb::read_with_user(root, user_ffb.as_deref()).unwrap_or_default(),
        None => FfbTweaks::default(),
    };

    let (post_process, per_car_gain) = match settings::ac_user_cfg_dir() {
        Some(cfg_dir) => (
            ffb::read_post_process(&cfg_dir).unwrap_or_default(),
            ffb::read_per_car_ffb(&cfg_dir).unwrap_or_default(),
        ),
        None => (FfbPostProcess::default(), BTreeMap::new()),
    };

    Ok(FfbConfigDto {
        tweaks: tweaks.into(),
        post_process,
        per_car_gain,
    })
}

#[tauri::command]
pub fn save_ffb_config(state: State<'_, AppState>, cfg: FfbConfigDto) -> Result<(), String> {
    if game::is_game_running() {
        return Err("Assetto Corsa is running; close it before saving FFB settings.".into());
    }
    let tweaks: FfbTweaks = cfg.tweaks.into();
    let errors = ffb::validate(&tweaks);
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    if let Some(root) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        s.ac_root_path()
    } {
        let user_ffb = csp_config::user_config_dir().map(|d| d.join("ffb_tweaks.ini"));
        ffb::write_with_user(&root, user_ffb.as_deref(), &tweaks).map_err(|e| e.to_string())?;
    }

    if let Some(cfg_dir) = settings::ac_user_cfg_dir() {
        ffb::write_post_process(&cfg_dir, &cfg.post_process).map_err(|e| e.to_string())?;
        for (car_id, gain) in &cfg.per_car_gain {
            ffb::write_car_ffb(&cfg_dir, car_id, *gain).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Rig Profiles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigProfileSummaryDto {
    pub name: String,
    pub has_video: bool,
    pub has_triple_screen: bool,
    pub has_vr_oculus: bool,
    pub has_vr_openvr: bool,
    pub has_controls: bool,
    pub has_ffb_post_process: bool,
    pub active: bool,
}

#[tauri::command]
pub fn list_rig_profiles(state: State<'_, AppState>) -> Result<Vec<RigProfileSummaryDto>, String> {
    let active_name = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        s.active_rig_profile.clone()
    };
    let store = RigProfileStore::load(&rig_profiles_path()).map_err(|e| e.to_string())?;
    Ok(store
        .profiles
        .into_iter()
        .map(|p| {
            let active = active_name
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(&p.name));
            RigProfileSummaryDto {
                name: p.name,
                has_video: p.video.is_some(),
                has_triple_screen: p.triple_screen.is_some(),
                has_vr_oculus: p.vr_oculus.is_some(),
                has_vr_openvr: p.vr_openvr.is_some(),
                has_controls: p.controls.is_some(),
                has_ffb_post_process: p.ffb_post_process.is_some(),
                active,
            }
        })
        .collect())
}

#[tauri::command]
pub fn apply_rig_profile(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let cfg_dir = require_user_cfg_dir()?;
    let store = RigProfileStore::load(&rig_profiles_path()).map_err(|e| e.to_string())?;
    let profile = store
        .get(&name)
        .ok_or_else(|| format!("Rig profile not found: {name}"))?;

    rig_profiles::apply_rig_profile(&cfg_dir, profile).map_err(|e| e.to_string())?;

    let mut s = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    s.active_rig_profile = Some(profile.name.clone());
    let _ = s.save(&settings::settings_path());
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. CSP Patch Management & Config Editor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspPatchDto {
    pub version: String,
    pub family: String,
    pub folder: String,
    pub sha256: String,
    pub known: bool,
    pub build_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspPatchesOverviewDto {
    pub installed_version: Option<String>,
    pub installed_build: Option<u32>,
    pub status_text: String,
    pub patches: Vec<CspPatchDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspInstallResultDto {
    pub version: String,
    pub archived_to: Option<String>,
    pub already_installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspReleaseDto {
    pub version: String,
    pub build_id: u32,
    pub url: String,
    pub changelog: String,
    pub recommended: bool,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspCatalogDto {
    pub etag: Option<String>,
    pub releases: Vec<CspReleaseDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspConfigSyncStatusDto {
    pub tracked_entries: usize,
    pub written: Vec<String>,
    pub backed_up: Vec<String>,
    pub skipped_up_to_date: usize,
    pub skipped_uninstalled: usize,
}

#[tauri::command]
pub fn list_csp_patches(state: State<'_, AppState>) -> Result<CspPatchesOverviewDto, String> {
    let (ac_root, library) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        (s.ac_root_path(), s.library_path())
    };

    let installed_version = match ac_root.as_deref() {
        Some(root) => csp::installed_version(root).unwrap_or(None),
        None => None,
    };
    let installed_build = ac_root.as_deref().and_then(csp::installed_build);
    let status_text = csp::status_text(&installed_version);

    let patches = csp::discover_patches(&[library])
        .into_iter()
        .map(|p| {
            let build_id = csp::patch_build(&p);
            CspPatchDto {
                version: p.version,
                family: p.family,
                folder: p.folder.to_string_lossy().into_owned(),
                sha256: p.sha256,
                known: p.known,
                build_id,
            }
        })
        .collect();

    Ok(CspPatchesOverviewDto {
        installed_version,
        installed_build,
        status_text,
        patches,
    })
}

#[tauri::command]
pub fn install_csp_patch(
    state: State<'_, AppState>,
    name: String,
) -> Result<CspInstallResultDto, String> {
    let (ac_root, library) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        let root = s
            .ac_root_path()
            .ok_or_else(|| "Assetto Corsa root folder is not configured".to_string())?;
        (root, s.library_path())
    };

    let patches = csp::discover_patches(&[library]);
    let patch = patches
        .iter()
        .find(|p| {
            p.version.eq_ignore_ascii_case(&name)
                || p.folder
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(&name))
        })
        .ok_or_else(|| format!("CSP patch not found in library: {name}"))?;

    let result =
        csp::install_patch(&ac_root, patch, game::is_game_running()).map_err(|e| e.to_string())?;
    Ok(CspInstallResultDto {
        version: result.version,
        archived_to: result.archived_to.map(|p| p.to_string_lossy().into_owned()),
        already_installed: result.already_installed,
    })
}

#[tauri::command]
pub fn uninstall_csp_patch(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let ac_root = ac_root_from_state(&state)?;
    let archived =
        csp::uninstall_patch(&ac_root, game::is_game_running()).map_err(|e| e.to_string())?;
    Ok(archived.map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn fetch_csp_catalog(state: State<'_, AppState>) -> Result<CspCatalogDto, String> {
    let url = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        s.csp_catalog_url
            .clone()
            .unwrap_or_else(|| csp_online::DEFAULT_CATALOG_URL.to_string())
    };

    let catalog = csp_online::fetch_catalog(&url, None).map_err(|e| e.to_string())?;
    Ok(CspCatalogDto {
        etag: catalog.etag,
        releases: catalog
            .releases
            .into_iter()
            .map(|r| CspReleaseDto {
                version: r.version,
                build_id: r.build_id,
                url: r.url,
                changelog: r.changelog,
                recommended: r.recommended,
                sha256: r.sha256,
            })
            .collect(),
    })
}

#[tauri::command]
pub fn sync_csp_configs(state: State<'_, AppState>) -> Result<CspConfigSyncStatusDto, String> {
    let ac_root = ac_root_from_state(&state)?;
    let report = csp_online::apply_config_sync(&ac_root, &[], game::is_game_running(), |_| {
        Err(csp_online::CspOnlineError::BadFormat(
            "no remote items supplied".into(),
        ))
    })
    .map_err(|e| e.to_string())?;

    let index = csp_online::load_sync_index(&ac_root);
    Ok(CspConfigSyncStatusDto {
        tracked_entries: index.entries.len(),
        written: report
            .written
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        backed_up: report
            .backed_up
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        skipped_up_to_date: report.skipped_up_to_date,
        skipped_uninstalled: report.skipped_uninstalled,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspConfigFileDto {
    pub name: String,
    pub path: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspSettingDto {
    pub section: String,
    pub key: String,
    pub value: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CspSettingEditDto {
    pub section: String,
    pub key: String,
    pub value: String,
}

#[tauri::command]
pub fn list_csp_config_files(state: State<'_, AppState>) -> Result<Vec<CspConfigFileDto>, String> {
    let ac_root = ac_root_from_state(&state)?;
    let ext_cfg_dir = ac_root.join("extension").join("config");
    let files = csp_config::list_config_files(&ext_cfg_dir);
    Ok(files
        .into_iter()
        .map(|f| {
            let meta = csp_config::config_meta(&f.path);
            CspConfigFileDto {
                name: f.name,
                path: f.path.to_string_lossy().into_owned(),
                title: meta
                    .as_ref()
                    .map(|m| m.title.clone())
                    .filter(|t| !t.is_empty()),
                description: meta
                    .map(|m| m.description)
                    .filter(|d| !d.is_empty()),
            }
        })
        .collect())
}

#[tauri::command]
pub fn read_csp_config(
    state: State<'_, AppState>,
    file: String,
) -> Result<Vec<CspSettingDto>, String> {
    let fname = validate_flat_filename(&file)?;
    let ac_root = ac_root_from_state(&state)?;
    let default_path = ac_root.join("extension").join("config").join(fname);
    let user_path = csp_config::user_config_path(&default_path);

    let settings = csp_config::read_effective_settings(&default_path, user_path.as_deref())
        .map_err(|e| e.to_string())?;
    Ok(settings
        .into_iter()
        .map(|s| CspSettingDto {
            section: s.section,
            key: s.key,
            value: s.value,
            doc: s.doc,
        })
        .collect())
}

#[tauri::command]
pub fn save_csp_config(
    state: State<'_, AppState>,
    file: String,
    edits: Vec<CspSettingEditDto>,
) -> Result<usize, String> {
    if game::is_game_running() {
        return Err("Assetto Corsa is running; close it before saving CSP config.".into());
    }
    let fname = validate_flat_filename(&file)?;
    let ac_root = ac_root_from_state(&state)?;
    let default_path = ac_root.join("extension").join("config").join(fname);
    let user_path = csp_config::user_config_path(&default_path)
        .ok_or_else(|| "Could not determine CSP user config directory".to_string())?;

    for edit in &edits {
        csp_config::write_user_setting(
            &default_path,
            &user_path,
            &edit.section,
            &edit.key,
            &edit.value,
            true,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(edits.len())
}

// ---------------------------------------------------------------------------
// 6. URI Protocols, Updates, Changelog & Config Snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolStatusDto {
    pub registered: bool,
    pub exe_path: String,
}

#[tauri::command]
pub fn get_protocol_status() -> Result<ProtocolStatusDto, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let registered = uri_register::is_registered(&exe);
    Ok(ProtocolStatusDto {
        registered,
        exe_path: exe.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn register_uri_protocols(enable: bool) -> Result<ProtocolStatusDto, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if enable {
        uri_register::register_protocols(&exe)?;
    } else {
        uri_register::unregister_protocols()?;
    }
    Ok(ProtocolStatusDto {
        registered: uri_register::is_registered(&exe),
        exe_path: exe.to_string_lossy().into_owned(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResultDto {
    pub current_version: String,
    pub update_available: bool,
    pub latest_version: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
}

#[tauri::command]
pub fn check_for_updates(url: String) -> Result<UpdateCheckResultDto, String> {
    let current_str = env!("CARGO_PKG_VERSION");
    let current = Version::parse(current_str).map_err(|e| e.to_string())?;
    let maybe_release =
        update::check(&url, Duration::from_secs(10), &current).map_err(|e| e.to_string())?;

    match maybe_release {
        Some(rel) => Ok(UpdateCheckResultDto {
            current_version: current_str.to_string(),
            update_available: true,
            latest_version: Some(rel.version.to_string()),
            url: Some(rel.url),
            notes: Some(rel.notes),
        }),
        None => Ok(UpdateCheckResultDto {
            current_version: current_str.to_string(),
            update_available: false,
            latest_version: None,
            url: None,
            notes: None,
        }),
    }
}

#[tauri::command]
pub fn get_changelog() -> Result<String, String> {
    Ok(include_str!("../../../CHANGELOG.md").to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshotDto {
    pub id: String,
    pub path: String,
    pub created: String,
    pub files: usize,
    pub label: Option<String>,
}

#[tauri::command]
pub fn list_config_snapshots(state: State<'_, AppState>) -> Result<Vec<ConfigSnapshotDto>, String> {
    let ac_root = ac_root_from_state(&state)?;
    let snaps = backup::list_snapshots(&ac_root);
    Ok(snaps
        .into_iter()
        .map(|s| ConfigSnapshotDto {
            id: s.name,
            path: s.path.to_string_lossy().into_owned(),
            created: s.created,
            files: s.files,
            label: None,
        })
        .collect())
}

#[tauri::command]
pub fn create_config_snapshot(
    state: State<'_, AppState>,
    label: Option<String>,
) -> Result<ConfigSnapshotDto, String> {
    let ac_root = ac_root_from_state(&state)?;
    let mut extra_dirs = Vec::new();
    if let Some(cfg_dir) = settings::ac_user_cfg_dir() {
        if cfg_dir.is_dir() {
            extra_dirs.push(cfg_dir);
        }
    }
    let snap = backup::create_snapshot(&ac_root, &extra_dirs).map_err(|e| e.to_string())?;
    Ok(ConfigSnapshotDto {
        id: snap.name,
        path: snap.path.to_string_lossy().into_owned(),
        created: snap.created,
        files: snap.files,
        label: label.filter(|l| !l.trim().is_empty()),
    })
}

#[tauri::command]
pub fn restore_config_snapshot(state: State<'_, AppState>, id: String) -> Result<usize, String> {
    let ac_root = ac_root_from_state(&state)?;
    let snaps = backup::list_snapshots(&ac_root);
    let snap = snaps
        .into_iter()
        .find(|s| s.name == id)
        .ok_or_else(|| format!("Config snapshot not found: {id}"))?;
    backup::restore_snapshot(&ac_root, Path::new(&snap.path), game::is_game_running())
        .map_err(|e| e.to_string())
}
