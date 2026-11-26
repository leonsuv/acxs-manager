//! Drive & race launch commands (quick race, presets, adaptive AI, events, championships, weather).

use std::fs;
use std::path::PathBuf;

use acxs_manager::adaptive_ai::{self, AdaptiveAiInput};
use acxs_manager::championship::{self, Championship, RaceResult, Standing};
use acxs_manager::driving_stats;
use acxs_manager::events::{self, EventProgressStore, GameEvent, Medal};
use acxs_manager::game;
use acxs_manager::presets::{self, Preset, PresetStore};
use acxs_manager::race::{self, AiOpponent, QuickRace};
use acxs_manager::race_options::{Assists, DynamicTrackConfig, GhostCarConfig, RaceExtras, RaceMode};
use acxs_manager::race_out;
use acxs_manager::settings;
use acxs_manager::weather;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// Serializable quick-race payload shared between `launch_quick_race` and presets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QuickRaceDto {
    pub track: String,
    pub track_layout: Option<String>,
    pub car: String,
    pub skin: String,
    pub ai_count: u32,
    pub laps: u32,
    pub weather: String,
    pub time_of_day_hour: u8,
    pub time_of_day_minute: u8,
    pub ai_level: u8,
    pub ai_aggression: Option<u8>,
    pub player_name: String,
    pub grid_position: u32,
    pub mode: RaceMode,
    pub practice_minutes: u32,
    pub qualify_minutes: u32,
    pub race_enabled: bool,
    pub race_minutes: u32,
    pub pit_start: bool,
    pub ambient_temp: i32,
    pub road_temp: i32,
    pub wind_min_kmh: u32,
    pub wind_max_kmh: u32,
    pub wind_direction: u32,
    pub time_multiplier: u32,
    pub track_grip: u32,
    pub penalties: bool,
    pub jump_start_penalty: u32,
    pub fixed_setup: bool,
    pub allowed_tyres_out: i32,
    pub write_assists: bool,
    pub assists: Assists,
    pub track_day_minutes: u32,
    pub drag_matches: u32,
    pub ai_flood: bool,
    pub opponents: Vec<AiOpponent>,
    pub player_setup: String,
    pub player_ballast: u32,
    pub player_restrictor: u32,
    pub date_unix: Option<i64>,
    pub weather_type: Option<i32>,
    pub weather_controller: Option<String>,
    pub weather_implementation: Option<String>,
    pub ghost_car: GhostCarConfig,
    pub dynamic_track: DynamicTrackConfig,
}

impl Default for QuickRaceDto {
    fn default() -> Self {
        Self::from(&QuickRace::default())
    }
}

impl From<&QuickRace> for QuickRaceDto {
    fn from(r: &QuickRace) -> Self {
        Self {
            track: r.track.clone(),
            track_layout: r.track_layout.clone(),
            car: r.car.clone(),
            skin: r.skin.clone(),
            ai_count: r.ai_count,
            laps: r.laps,
            weather: r.weather.clone(),
            time_of_day_hour: r.time_of_day_hour,
            time_of_day_minute: r.time_of_day_minute,
            ai_level: r.ai_level,
            ai_aggression: r.ai_aggression,
            player_name: r.player_name.clone(),
            grid_position: r.grid_position,
            mode: r.extras.mode,
            practice_minutes: r.extras.practice_minutes,
            qualify_minutes: r.extras.qualify_minutes,
            race_enabled: r.extras.race_enabled,
            race_minutes: r.extras.race_minutes,
            pit_start: r.extras.pit_start,
            ambient_temp: r.extras.ambient_temp,
            road_temp: r.extras.road_temp,
            wind_min_kmh: r.extras.wind_min_kmh,
            wind_max_kmh: r.extras.wind_max_kmh,
            wind_direction: r.extras.wind_direction,
            time_multiplier: r.extras.time_multiplier,
            track_grip: r.extras.track_grip,
            penalties: r.extras.penalties,
            jump_start_penalty: r.extras.jump_start_penalty,
            fixed_setup: r.extras.fixed_setup,
            allowed_tyres_out: r.extras.allowed_tyres_out,
            write_assists: r.extras.write_assists,
            assists: r.extras.assists,
            track_day_minutes: r.extras.track_day_minutes,
            drag_matches: r.extras.drag_matches,
            ai_flood: r.extras.ai_flood,
            opponents: r.opponents.clone(),
            player_setup: r.player_setup.clone(),
            player_ballast: r.player_ballast,
            player_restrictor: r.player_restrictor,
            date_unix: r.date_unix,
            weather_type: r.weather_type,
            weather_controller: r.weather_controller.clone(),
            weather_implementation: None,
            ghost_car: r.ghost_car.clone(),
            dynamic_track: r.dynamic_track,
        }
    }
}

impl From<&QuickRaceDto> for QuickRace {
    fn from(d: &QuickRaceDto) -> Self {
        Self {
            track: d.track.clone(),
            track_layout: d.track_layout.clone().filter(|s| !s.trim().is_empty()),
            car: d.car.clone(),
            skin: d.skin.clone(),
            ai_count: d.ai_count,
            laps: d.laps,
            weather: d.weather.clone(),
            time_of_day_hour: d.time_of_day_hour,
            ai_level: d.ai_level,
            ai_aggression: d.ai_aggression,
            player_name: d.player_name.clone(),
            grid_position: d.grid_position,
            extras: RaceExtras {
                mode: d.mode,
                practice_minutes: d.practice_minutes,
                qualify_minutes: d.qualify_minutes,
                race_enabled: d.race_enabled,
                race_minutes: d.race_minutes,
                pit_start: d.pit_start,
                ambient_temp: d.ambient_temp,
                road_temp: d.road_temp,
                wind_min_kmh: d.wind_min_kmh,
                wind_max_kmh: d.wind_max_kmh,
                wind_direction: d.wind_direction,
                time_multiplier: d.time_multiplier,
                track_grip: d.track_grip,
                penalties: d.penalties,
                jump_start_penalty: d.jump_start_penalty,
                fixed_setup: d.fixed_setup,
                allowed_tyres_out: d.allowed_tyres_out,
                write_assists: d.write_assists,
                assists: d.assists,
                track_day_minutes: d.track_day_minutes,
                drag_matches: d.drag_matches,
                ai_flood: d.ai_flood,
            },
            opponents: d.opponents.clone(),
            player_setup: d.player_setup.clone(),
            player_ballast: d.player_ballast,
            player_restrictor: d.player_restrictor,
            time_of_day_minute: d.time_of_day_minute,
            date_unix: d.date_unix,
            weather_type: d.weather_type,
            weather_controller: d.weather_controller.clone().filter(|s| !s.trim().is_empty()),
            ghost_car: d.ghost_car.clone(),
            dynamic_track: d.dynamic_track,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetDto {
    pub name: String,
    pub race: QuickRaceDto,
}

impl From<&Preset> for PresetDto {
    fn from(p: &Preset) -> Self {
        Self {
            name: p.name.clone(),
            race: QuickRaceDto::from(&p.race),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResultDto {
    pub pid: u32,
    pub race_ini_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveAiSlotDto {
    pub slot_index: usize,
    pub ai_level: u8,
    pub ai_aggression: u8,
    pub ballast_kg: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveAiResultDto {
    pub target_level: u8,
    pub player_best_ms: Option<u32>,
    pub last_race_gap_ms_per_lap: Option<f64>,
    pub slots: Vec<AdaptiveAiSlotDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEventDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub series: String,
    pub track: String,
    pub layout: String,
    pub car: String,
    pub skin: String,
    pub goals: Vec<String>,
    pub preview_path: Option<String>,
    pub playable: bool,
    pub medal: Medal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingDto {
    pub driver: String,
    pub points: u32,
    pub wins: u32,
    pub fastest_laps: u32,
    pub best_position: Option<u32>,
    pub rounds_entered: u32,
}

impl From<&Standing> for StandingDto {
    fn from(s: &Standing) -> Self {
        Self {
            driver: s.driver.clone(),
            points: s.points,
            wins: s.wins,
            fastest_laps: s.fastest_laps,
            best_position: s.best_position,
            rounds_entered: s.rounds_entered,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionshipBundleDto {
    pub championship: Championship,
    pub standings: Vec<StandingDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaceOutPlayerDto {
    pub name: String,
    pub car: String,
    pub skin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaceOutSummaryDto {
    pub track: String,
    pub players: Vec<RaceOutPlayerDto>,
    pub results: Vec<RaceResult>,
    pub player_best_lap_ms: Option<u32>,
    pub gap_to_p1_ms_per_lap: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherPresetDto {
    pub name: String,
    pub path: String,
    pub has_controller: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherOverviewDto {
    pub controllers: Vec<String>,
    pub presets: Vec<WeatherPresetDto>,
    pub active_controller: Option<String>,
    pub active_implementation: Option<String>,
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

fn resolve_cfg_dir() -> Result<PathBuf, String> {
    settings::ac_user_cfg_dir()
        .ok_or_else(|| "Could not locate Documents/Assetto Corsa/cfg.".to_string())
}

fn acxs_config_dir() -> PathBuf {
    settings::settings_path()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn presets_file_path(state: &AppState) -> PathBuf {
    match resolve_ac_root(state) {
        Ok(ac_root) => presets::default_path(&ac_root),
        Err(_) => acxs_config_dir().join("acxs-presets.json"),
    }
}

fn event_progress_path() -> PathBuf {
    acxs_config_dir().join("event-progress.json")
}

fn championships_dir() -> PathBuf {
    acxs_config_dir().join("championships")
}

fn championship_slug(name: &str) -> String {
    let slug: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('_');
    if trimmed.is_empty() {
        "championship".to_string()
    } else {
        trimmed.to_string()
    }
}

fn ensure_game_not_running() -> Result<(), String> {
    if game::is_game_running() {
        return Err("Assetto Corsa is already running. Close it first.".to_string());
    }
    Ok(())
}

/// Writes `race.ini` (plus optional assists, WeatherFX controller, and AI Flood override) and launches `acs.exe`.
#[tauri::command]
pub fn launch_quick_race(
    state: State<'_, AppState>,
    req: QuickRaceDto,
) -> Result<LaunchResultDto, String> {
    ensure_game_not_running()?;
    let ac_root = resolve_ac_root(&state)?;
    let cfg_dir = resolve_cfg_dir()?;
    let quick_race = QuickRace::from(&req);

    if let Some(ctrl) = req
        .weather_controller
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && weather::weather_fx_path(&ac_root).is_file()
    {
        let user_fx = cfg_dir.join("extension").join("weather_fx.ini");
        let impl_name = req.weather_implementation.as_deref().unwrap_or("");
        weather::select_controller_user(&ac_root, &user_fx, impl_name, ctrl)
            .map_err(|e| format!("Failed to apply WeatherFX controller: {e}"))?;
    }

    if req.ai_flood {
        let user_ext = cfg_dir.join("extension");
        race::write_ai_flood_override(&user_ext, true)
            .map_err(|e| format!("Failed to enable AI Flood: {e}"))?;
    }

    if req.write_assists {
        race::write_assists_ini(&cfg_dir, &req.assists)
            .map_err(|e| format!("Failed to write assists.ini: {e}"))?;
    }

    let ini_path = race::write_race_ini(&cfg_dir, &quick_race)
        .map_err(|e| format!("Failed to write race.ini: {e}"))?;
    let child =
        game::launch_game(&ac_root).map_err(|e| format!("Failed to launch Assetto Corsa: {e}"))?;

    Ok(LaunchResultDto {
        pid: child.id(),
        race_ini_path: Some(ini_path.to_string_lossy().into_owned()),
    })
}

/// Launches `acs.exe` directly without modifying `race.ini`.
#[tauri::command]
pub fn launch_game_direct(state: State<'_, AppState>) -> Result<LaunchResultDto, String> {
    ensure_game_not_running()?;
    let ac_root = resolve_ac_root(&state)?;
    let child =
        game::launch_game(&ac_root).map_err(|e| format!("Failed to launch Assetto Corsa: {e}"))?;
    Ok(LaunchResultDto {
        pid: child.id(),
        race_ini_path: None,
    })
}

/// Returns all saved quick-race presets.
#[tauri::command]
pub fn get_presets(state: State<'_, AppState>) -> Result<Vec<PresetDto>, String> {
    let path = presets_file_path(&state);
    let store = PresetStore::load(&path).map_err(|e| format!("Failed to load presets: {e}"))?;
    Ok(store.list().iter().map(PresetDto::from).collect())
}

/// Creates or updates a named quick-race preset and returns the updated list.
#[tauri::command]
pub fn save_preset(
    state: State<'_, AppState>,
    preset: PresetDto,
) -> Result<Vec<PresetDto>, String> {
    let path = presets_file_path(&state);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create preset dir: {e}"))?;
    }
    let mut store =
        PresetStore::load(&path).map_err(|e| format!("Failed to load presets: {e}"))?;
    store
        .upsert(&preset.name, QuickRace::from(&preset.race))
        .map_err(|e| format!("Invalid preset: {e}"))?;
    store
        .save(&path)
        .map_err(|e| format!("Failed to save presets: {e}"))?;
    Ok(store.list().iter().map(PresetDto::from).collect())
}

/// Soft-deletes a preset by name (moves it into `removed` in the preset store).
#[tauri::command]
pub fn delete_preset(state: State<'_, AppState>, name: String) -> Result<bool, String> {
    let path = presets_file_path(&state);
    let mut store =
        PresetStore::load(&path).map_err(|e| format!("Failed to load presets: {e}"))?;
    let removed = store.remove(&name);
    if removed {
        store
            .save(&path)
            .map_err(|e| format!("Failed to save presets: {e}"))?;
    }
    Ok(removed)
}

/// Computes a calibrated Adaptive AI grid using local personal bests and `race_out.json` gap data.
#[tauri::command]
pub fn compute_adaptive_ai(
    car_id: String,
    track_id: String,
    count: usize,
    spread: u8,
    prev_ai_level: Option<u8>,
    base_aggression: Option<u8>,
) -> Result<AdaptiveAiResultDto, String> {
    let cm_laps = driving_stats::load_cm_laps();
    let bests = driving_stats::personal_bests(&cm_laps);
    let normalized_track = track_id.replace('/', "-");
    let player_best_ms = bests
        .iter()
        .find(|b| b.car_id == car_id && b.track_key.starts_with(&normalized_track))
        .map(|b| b.time_ms);

    let last_race_gap_ms_per_lap = settings::ac_documents_dir()
        .map(|d| d.join("out").join("race_out.json"))
        .and_then(|p| race_out::load_race_out(&p).ok())
        .filter(|summary| {
            summary.track.eq_ignore_ascii_case(&track_id)
                || summary.track.eq_ignore_ascii_case(&normalized_track)
        })
        .and_then(|summary| summary.gap_to_p1_ms_per_lap);

    let input = AdaptiveAiInput {
        player_best_ms,
        recent_player_avg_ms: None,
        last_race_gap_ms_per_lap,
        prev_ai_level: prev_ai_level.unwrap_or(90),
        base_aggression: base_aggression.unwrap_or(40),
        grid_size: count,
        spread,
    };

    let target_level = adaptive_ai::calibrate_target_level(&input);
    let slots = adaptive_ai::compute_ai_grid(&input)
        .into_iter()
        .enumerate()
        .map(|(slot_index, (ai_level, ai_aggression, ballast_kg))| AdaptiveAiSlotDto {
            slot_index,
            ai_level,
            ai_aggression,
            ballast_kg,
        })
        .collect();

    Ok(AdaptiveAiResultDto {
        target_level,
        player_best_ms,
        last_race_gap_ms_per_lap,
        slots,
    })
}

/// Lists all Special Events and Career events with earned medal progress.
#[tauri::command]
pub fn list_events(state: State<'_, AppState>) -> Result<Vec<GameEventDto>, String> {
    let ac_root = resolve_ac_root(&state)?;
    let ac_progress = settings::ac_user_cfg_dir()
        .map(|cfg| events::read_progress_ini(&cfg))
        .unwrap_or_default();
    let local_progress = EventProgressStore::load(&event_progress_path());

    let mut all_events: Vec<GameEvent> = events::special_events(&ac_root);
    all_events.extend(events::career_events(&ac_root));

    Ok(all_events
        .into_iter()
        .map(|ev| {
            let ac_medal = ac_progress.get(&ev.id).copied().unwrap_or(Medal::None);
            let local_medal = local_progress.medals.get(&ev.id).copied().unwrap_or(Medal::None);
            let medal = ac_medal.max(local_medal);
            GameEventDto {
                playable: ev.is_playable(&ac_root),
                preview_path: ev.preview.map(|p| p.to_string_lossy().into_owned()),
                id: ev.id,
                name: ev.name,
                kind: ev.kind,
                series: ev.series,
                track: ev.track,
                layout: ev.layout,
                car: ev.car,
                skin: ev.skin,
                goals: ev.goals,
                medal,
            }
        })
        .collect())
}

/// Launches a Special Event or Career event by its folder id.
#[tauri::command]
pub fn launch_event(
    state: State<'_, AppState>,
    event_id: String,
    player_name: Option<String>,
) -> Result<LaunchResultDto, String> {
    ensure_game_not_running()?;
    let ac_root = resolve_ac_root(&state)?;
    let cfg_dir = resolve_cfg_dir()?;

    let mut all_events = events::special_events(&ac_root);
    all_events.extend(events::career_events(&ac_root));
    let event = all_events
        .into_iter()
        .find(|ev| ev.id == event_id)
        .ok_or_else(|| format!("Event not found: {event_id}"))?;

    let raw = fs::read(&event.ini)
        .map_err(|e| format!("Failed to read {}: {e}", event.ini.display()))?;
    let event_text = String::from_utf8_lossy(&raw);
    let driver = player_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Driver");
    let race_ini = events::to_race_ini(&event_text, driver);
    let ini_path = race::write_event_ini(&cfg_dir, &race_ini)
        .map_err(|e| format!("Failed to write event race.ini: {e}"))?;
    let child =
        game::launch_game(&ac_root).map_err(|e| format!("Failed to launch Assetto Corsa: {e}"))?;

    Ok(LaunchResultDto {
        pid: child.id(),
        race_ini_path: Some(ini_path.to_string_lossy().into_owned()),
    })
}

/// Lists all saved championships along with computed standings.
#[tauri::command]
pub fn list_championships() -> Result<Vec<ChampionshipBundleDto>, String> {
    let dir = championships_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut bundles = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        if let Ok(champ) = Championship::load(&path) {
            let standings = championship::standings_with(&champ, champ.scoring)
                .iter()
                .map(StandingDto::from)
                .collect();
            bundles.push(ChampionshipBundleDto {
                championship: champ,
                standings,
            });
        }
    }
    bundles.sort_by(|a, b| {
        a.championship
            .name
            .to_lowercase()
            .cmp(&b.championship.name.to_lowercase())
    });
    Ok(bundles)
}

/// Validates and saves a championship to disk, returning the updated bundle with standings.
#[tauri::command]
pub fn save_championship(champ: Championship) -> Result<ChampionshipBundleDto, String> {
    champ.validate().map_err(|e| e.to_string())?;
    let dir = championships_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create championships folder: {e}"))?;
    let path = dir.join(format!("{}.json", championship_slug(&champ.name)));
    champ.save(&path).map_err(|e| e.to_string())?;
    let standings = championship::standings_with(&champ, champ.scoring)
        .iter()
        .map(StandingDto::from)
        .collect();
    Ok(ChampionshipBundleDto {
        championship: champ,
        standings,
    })
}

/// Builds and launches a championship round (with success ballast applied) in Assetto Corsa.
#[tauri::command]
pub fn launch_championship_round(
    state: State<'_, AppState>,
    champ_name: String,
    round_idx: usize,
    player_name: Option<String>,
) -> Result<LaunchResultDto, String> {
    ensure_game_not_running()?;
    let ac_root = resolve_ac_root(&state)?;
    let cfg_dir = resolve_cfg_dir()?;

    let dir = championships_dir();
    let target_path = dir.join(format!("{}.json", championship_slug(&champ_name)));
    let champ = if target_path.is_file() {
        Championship::load(&target_path).map_err(|e| e.to_string())?
    } else {
        let mut found = None;
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(c) = Championship::load(&entry.path())
                    && c.name.eq_ignore_ascii_case(champ_name.trim())
                {
                    found = Some(c);
                    break;
                }
            }
        }
        found.ok_or_else(|| format!("Championship not found: {champ_name}"))?
    };

    let driver = player_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Driver");
    let quick_race = champ
        .build_round_race(round_idx, driver)
        .map_err(|e| e.to_string())?;
    let ini_path = race::write_race_ini(&cfg_dir, &quick_race)
        .map_err(|e| format!("Failed to write championship race.ini: {e}"))?;
    let child =
        game::launch_game(&ac_root).map_err(|e| format!("Failed to launch Assetto Corsa: {e}"))?;

    Ok(LaunchResultDto {
        pid: child.id(),
        race_ini_path: Some(ini_path.to_string_lossy().into_owned()),
    })
}

/// Imports results from Assetto Corsa's `Documents/Assetto Corsa/out/race_out.json`.
#[tauri::command]
pub fn import_race_out_results() -> Result<RaceOutSummaryDto, String> {
    let docs = settings::ac_documents_dir()
        .ok_or_else(|| "Could not locate Documents/Assetto Corsa.".to_string())?;
    let path = docs.join("out").join("race_out.json");
    let summary = race_out::load_race_out(&path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    Ok(RaceOutSummaryDto {
        track: summary.track,
        players: summary
            .players
            .into_iter()
            .map(|p| RaceOutPlayerDto {
                name: p.name,
                car: p.car,
                skin: p.skin,
            })
            .collect(),
        results: summary.results,
        player_best_lap_ms: summary.player_best_lap_ms,
        gap_to_p1_ms_per_lap: summary.gap_to_p1_ms_per_lap,
    })
}

/// Lists installed CSP WeatherFX controllers, weather implementations, and active selection.
#[tauri::command]
pub fn list_weather_controllers(state: State<'_, AppState>) -> Result<WeatherOverviewDto, String> {
    let ac_root = resolve_ac_root(&state)?;
    let controllers = weather::list_controllers(&ac_root);
    let presets = weather::list_presets(&ac_root)
        .into_iter()
        .map(|p| WeatherPresetDto {
            name: p.name,
            path: p.path.to_string_lossy().into_owned(),
            has_controller: p.has_controller,
        })
        .collect();

    let user_fx = settings::ac_user_cfg_dir().map(|d| d.join("extension").join("weather_fx.ini"));
    let entries = weather::read_weather_fx_effective(&ac_root, user_fx.as_deref()).unwrap_or_default();
    let find_basic = |key: &str| -> Option<String> {
        entries
            .iter()
            .find(|(sec, k, _)| sec.eq_ignore_ascii_case("BASIC") && k.eq_ignore_ascii_case(key))
            .map(|(_, _, v)| v.clone())
            .filter(|v| !v.is_empty())
    };

    Ok(WeatherOverviewDto {
        controllers,
        presets,
        active_controller: find_basic("CONTROLLER"),
        active_implementation: find_basic("IMPLEMENTATION"),
    })
}
