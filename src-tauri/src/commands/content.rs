//! Content library commands (cars, tracks, skins, previews, and OS explorer integration).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use acxs_manager::content::{self, CarEntry, TrackEntry};
use acxs_manager::csp;
use acxs_manager::driving_stats::{self, DrivingStats};
use acxs_manager::game;
use acxs_manager::query;
use acxs_manager::settings;
use acxs_manager::skins;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// Car metadata and resolved preview image paths for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarDto {
    pub id: String,
    pub name: String,
    pub brand: String,
    pub car_class: String,
    pub year: String,
    pub tags: Vec<String>,
    pub parent: Option<String>,
    pub country: String,
    pub author: String,
    pub bhp: Option<f64>,
    pub weight_kg: Option<f64>,
    pub pwratio: Option<f64>,
    pub path: String,
    pub preview_path: Option<String>,
    pub badge_path: Option<String>,
}

/// Track metadata and resolved preview/outline image paths for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDto {
    pub id: String,
    pub name: String,
    pub country: String,
    pub city: String,
    pub length_m: Option<f64>,
    pub pitboxes: Option<u32>,
    pub layouts: Vec<String>,
    pub tags: Vec<String>,
    pub path: String,
    pub preview_path: Option<String>,
    pub outline_path: Option<String>,
}

/// Car skin entry with preview path and `ui_skin.json` metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinDto {
    pub skin_id: String,
    pub name: String,
    pub driver: String,
    pub team: String,
    pub number: String,
    pub country: String,
    pub priority: Option<i64>,
    pub preview_path: Option<String>,
    pub livery_path: Option<String>,
    pub texture_count: usize,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntryDto {
    pub id: String,
    pub sessions: usize,
    pub seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyHoursDto {
    pub week: String,
    pub label: String,
    pub hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrivingStatsSummaryDto {
    pub sessions: usize,
    pub seconds: f64,
    pub distance_km: f64,
    pub crashes: u32,
    pub offroad: u32,
    pub max_speed: f64,
    pub longest_airborne: f64,
    pub crashes_per_100km: Option<f64>,
    pub top_cars: Vec<UsageEntryDto>,
    pub top_tracks: Vec<UsageEntryDto>,
    pub weekly_hours: Vec<WeeklyHoursDto>,
}

/// Combined result returned by [`scan_content`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentScanResult {
    pub ac_root: String,
    pub cars: Vec<CarDto>,
    pub tracks: Vec<TrackDto>,
    pub game_running: bool,
    pub csp_build: Option<u32>,
    pub csp_version: Option<String>,
    pub driving_stats: DrivingStatsSummaryDto,
}

fn resolve_ac_root(state: &AppState) -> Result<PathBuf, String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    settings
        .ac_root_path()
        .or_else(settings::detect_ac_root)
        .ok_or_else(|| "Assetto Corsa folder is not configured.".to_string())
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Finds the first available `preview.jpg` or `preview.png` for a car folder.
fn resolve_car_preview(car_dir: &Path) -> Option<String> {
    let skins_dir = car_dir.join("skins");
    if let Ok(read) = fs::read_dir(&skins_dir) {
        let mut dirs: Vec<PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for skin_dir in dirs {
            if let Some(preview) = skins::preview_for(&skin_dir) {
                return Some(path_to_string(&preview));
            }
        }
    }
    for rel in [
        "ui/preview.jpg",
        "ui/preview.png",
        "preview.jpg",
        "preview.png",
    ] {
        let candidate = car_dir.join(rel);
        if candidate.is_file() {
            return Some(path_to_string(&candidate));
        }
    }
    None
}

fn resolve_car_badge(car_dir: &Path) -> Option<String> {
    let candidate = car_dir.join("ui").join("badge.png");
    candidate.is_file().then(|| path_to_string(&candidate))
}

fn resolve_track_image(track_dir: &Path, layouts: &[String], file_name: &str) -> Option<String> {
    let direct = track_dir.join("ui").join(file_name);
    if direct.is_file() {
        return Some(path_to_string(&direct));
    }
    for layout in layouts {
        let kunos_style = track_dir.join("ui").join(layout).join(file_name);
        if kunos_style.is_file() {
            return Some(path_to_string(&kunos_style));
        }
        let alt_style = track_dir.join(layout).join("ui").join(file_name);
        if alt_style.is_file() {
            return Some(path_to_string(&alt_style));
        }
    }
    None
}

fn car_to_dto(car: &CarEntry) -> CarDto {
    CarDto {
        id: car.id.clone(),
        name: car.name.clone(),
        brand: car.brand.clone(),
        car_class: car.car_class.clone(),
        year: car.year.clone(),
        tags: car.tags.clone(),
        parent: car.parent.clone(),
        country: car.country.clone(),
        author: car.author.clone(),
        bhp: car.bhp,
        weight_kg: car.weight_kg,
        pwratio: car.pwratio,
        preview_path: resolve_car_preview(&car.path),
        badge_path: resolve_car_badge(&car.path),
        path: path_to_string(&car.path),
    }
}

fn track_to_dto(track: &TrackEntry) -> TrackDto {
    TrackDto {
        id: track.id.clone(),
        name: track.name.clone(),
        country: track.country.clone(),
        city: track.city.clone(),
        length_m: track.length_m,
        pitboxes: track.pitboxes,
        layouts: track.layouts.clone(),
        tags: track.tags.clone(),
        preview_path: resolve_track_image(&track.path, &track.layouts, "preview.png")
            .or_else(|| resolve_track_image(&track.path, &track.layouts, "preview.jpg")),
        outline_path: resolve_track_image(&track.path, &track.layouts, "outline.png"),
        path: path_to_string(&track.path),
    }
}

fn stats_to_dto(stats: &DrivingStats) -> DrivingStatsSummaryDto {
    DrivingStatsSummaryDto {
        sessions: stats.sessions,
        seconds: stats.seconds,
        distance_km: stats.distance_km,
        crashes: stats.crashes,
        offroad: stats.offroad,
        max_speed: stats.max_speed,
        longest_airborne: stats.longest_airborne,
        crashes_per_100km: stats.crashes_per_100km,
        top_cars: stats
            .top_cars
            .iter()
            .take(10)
            .map(|u| UsageEntryDto {
                id: u.id.clone(),
                sessions: u.sessions,
                seconds: u.seconds,
            })
            .collect(),
        top_tracks: stats
            .top_tracks
            .iter()
            .take(10)
            .map(|u| UsageEntryDto {
                id: u.id.clone(),
                sessions: u.sessions,
                seconds: u.seconds,
            })
            .collect(),
        weekly_hours: stats
            .weekly_hours
            .iter()
            .map(|(week, hours)| WeeklyHoursDto {
                label: driving_stats::week_range(week),
                week: week.clone(),
                hours: *hours,
            })
            .collect(),
    }
}

fn count_skin_textures(skin_dir: &Path) -> usize {
    const NON_TEXTURE: [&str; 3] = ["preview.jpg", "preview.png", "livery.png"];
    const TEXTURE_EXTS: [&str; 5] = ["dds", "png", "jpg", "jpeg", "gif"];
    let Ok(read) = fs::read_dir(skin_dir) else {
        return 0;
    };
    read.flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            !NON_TEXTURE.contains(&name.as_str())
        })
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| TEXTURE_EXTS.contains(&ext.to_string_lossy().to_lowercase().as_str()))
        })
        .count()
}

fn skin_name_from_id(id: &str) -> String {
    let trimmed = id.trim_start_matches(|c: char| c.is_ascii_digit());
    let trimmed = trimmed.strip_prefix('_').unwrap_or(trimmed);
    let base = if trimmed.is_empty() { id } else { trimmed };
    base.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scans `ac_root` for cars and tracks, updates cached state, and returns game/CSP/driving stats.
#[tauri::command]
pub fn scan_content(state: State<'_, AppState>) -> Result<ContentScanResult, String> {
    let ac_root = resolve_ac_root(&state)?;
    let cars = content::scan_cars(&ac_root);
    let tracks = content::scan_tracks(&ac_root);

    let car_dtos: Vec<CarDto> = cars.iter().map(car_to_dto).collect();
    let track_dtos: Vec<TrackDto> = tracks.iter().map(track_to_dto).collect();

    {
        let mut cached_cars = state
            .cars
            .lock()
            .map_err(|_| "cars lock poisoned".to_string())?;
        *cached_cars = cars;
    }
    {
        let mut cached_tracks = state
            .tracks
            .lock()
            .map_err(|_| "tracks lock poisoned".to_string())?;
        *cached_tracks = tracks;
    }

    let game_running = game::is_game_running();
    let csp_build = csp::installed_build(&ac_root);
    let csp_version = csp::installed_version(&ac_root).ok().flatten();

    let sessions = driving_stats::default_profile_path()
        .map(|p| driving_stats::load_profile(&p))
        .unwrap_or_default();
    let stats = driving_stats::summarize(&sessions);

    Ok(ContentScanResult {
        ac_root: path_to_string(&ac_root),
        cars: car_dtos,
        tracks: track_dtos,
        game_running,
        csp_build,
        csp_version,
        driving_stats: stats_to_dto(&stats),
    })
}

/// Filters cached cars using the ACXS query language, falling back to substring matching on syntax error.
#[tauri::command]
pub fn search_cars(state: State<'_, AppState>, query_str: String) -> Result<Vec<CarDto>, String> {
    let cars = state
        .cars
        .lock()
        .map_err(|_| "cars lock poisoned".to_string())?;
    let trimmed = query_str.trim();
    if trimmed.is_empty() {
        return Ok(cars.iter().map(car_to_dto).collect());
    }
    match query::parse(trimmed) {
        Ok(parsed) => Ok(cars
            .iter()
            .filter(|car| query::matches(&parsed, *car))
            .map(car_to_dto)
            .collect()),
        Err(_) => Ok(content::filter_cars(&cars, trimmed)
            .into_iter()
            .map(car_to_dto)
            .collect()),
    }
}

/// Filters cached tracks using the ACXS query language, falling back to substring matching on syntax error.
#[tauri::command]
pub fn search_tracks(
    state: State<'_, AppState>,
    query_str: String,
) -> Result<Vec<TrackDto>, String> {
    let tracks = state
        .tracks
        .lock()
        .map_err(|_| "tracks lock poisoned".to_string())?;
    let trimmed = query_str.trim();
    if trimmed.is_empty() {
        return Ok(tracks.iter().map(track_to_dto).collect());
    }
    match query::parse(trimmed) {
        Ok(parsed) => Ok(tracks
            .iter()
            .filter(|track| query::matches(&parsed, *track))
            .map(track_to_dto)
            .collect()),
        Err(_) => Ok(content::filter_tracks(&tracks, trimmed)
            .into_iter()
            .map(track_to_dto)
            .collect()),
    }
}

/// Lists skins for a car directory along with preview image paths and `ui_skin.json` metadata.
#[tauri::command]
pub fn get_car_skins(car_path: String) -> Result<Vec<SkinDto>, String> {
    let car_dir = PathBuf::from(car_path.trim());
    if !car_dir.is_dir() {
        return Err(format!("Car directory not found: {}", car_dir.display()));
    }
    let skins_dir = car_dir.join("skins");
    let Ok(entries) = fs::read_dir(&skins_dir) else {
        return Ok(Vec::new());
    };

    let mut out: Vec<SkinDto> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|dir| {
            let skin_id = dir.file_name()?.to_string_lossy().into_owned();
            let meta = skins::read_skin_meta(&dir);
            let name = meta
                .skinname
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| skin_name_from_id(&skin_id));
            let preview_path = skins::preview_for(&dir).map(|p| path_to_string(&p));
            let livery = dir.join("livery.png");
            let livery_path = livery.is_file().then(|| path_to_string(&livery));
            Some(SkinDto {
                skin_id,
                name,
                driver: meta.drivername.unwrap_or_default(),
                team: meta.team.unwrap_or_default(),
                number: meta.number.unwrap_or_default(),
                country: meta.country.unwrap_or_default(),
                priority: meta.priority,
                preview_path,
                livery_path,
                texture_count: count_skin_textures(&dir),
                path: path_to_string(&dir),
            })
        })
        .collect();

    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.skin_id.cmp(&b.skin_id))
    });
    Ok(out)
}

/// Opens a file or folder in the operating system's native file manager.
#[tauri::command]
pub fn open_path_in_explorer(path: String) -> Result<(), String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() || !target.exists() {
        return Err(format!("Path does not exist: {}", target.display()));
    }
    let dir = if target.is_file() {
        target.parent().unwrap_or(&target)
    } else {
        target.as_path()
    };

    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(dir);
        c
    };

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("explorer");
        c.arg(dir);
        c
    };

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(dir);
        c
    };

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open {}: {e}", dir.display()))
}
