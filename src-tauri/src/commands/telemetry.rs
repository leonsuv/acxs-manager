//! Live telemetry, UDP client, web dashboard, session analysis, replays, trajectory, strategy, and timing commands.

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use acxs_manager::ai_spline;
use acxs_manager::corner_coach;
use acxs_manager::game;
use acxs_manager::laptimes::{self, LapStore};
use acxs_manager::race;
use acxs_manager::race_out;
use acxs_manager::replay::{self, Sample};
use acxs_manager::replay_library;
use acxs_manager::session_recorder::{self, EventKind, RecordedSession};
use acxs_manager::settings;
use acxs_manager::strategy;
use acxs_manager::telemetry::Packet;
use acxs_manager::timing;
use acxs_manager::track_files;
use acxs_manager::trajectory::{self, TrajectoryLap, TrajectoryPoint};
use acxs_manager::udp_client::{ClientError, TelemetryClient};
use acxs_manager::web_dashboard::Dashboard;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn session_id_of(s: &RecordedSession) -> String {
    let stamp: String = s.started_at.chars().filter(char::is_ascii_digit).collect();
    let car: String = s
        .car_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    format!("{stamp}-{car}")
}

fn event_kind_str(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Crash => "crash",
        EventKind::OffTrack => "offTrack",
        EventKind::Jump => "jump",
        EventKind::FrontLockup => "frontLockup",
        EventKind::RearLockup => "rearLockup",
        EventKind::Wheelspin => "wheelspin",
        EventKind::Understeer => "understeer",
        EventKind::Oversteer => "oversteer",
        EventKind::WallHit => "wallHit",
        EventKind::CarHit => "carHit",
    }
}

fn samples_to_trajectory_lap(label: String, samples: &[Sample]) -> TrajectoryLap {
    let points = samples
        .iter()
        .map(|s| TrajectoryPoint {
            x: s.x,
            z: s.z,
            speed_kmh: s.speed_kmh,
            throttle: s.throttle,
            brake: s.brake,
            gear: s.gear,
            elapsed_ms: s.time_s * 1000.0,
        })
        .collect();
    TrajectoryLap { label, points }
}

// ---------------------------------------------------------------------------
// 1. Session History & Corner Coach Analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub id: String,
    pub car_id: String,
    pub skin: String,
    pub track_id: String,
    pub track_layout: String,
    pub full_track_id: String,
    pub session_type: String,
    pub started_at: String,
    pub seconds: f64,
    pub distance_m: f64,
    pub max_speed: f64,
    pub lap_count: usize,
    pub best_lap_ms: Option<i32>,
    pub crashes: u32,
    pub offroad: u32,
    pub front_lockups: u32,
    pub rear_lockups: u32,
    pub wheelspins: u32,
    pub setup_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotspotDto {
    pub segment: u8,
    pub kind: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPointDto {
    pub kind: String,
    pub x: f32,
    pub z: f32,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerCoachingDto {
    pub number: usize,
    pub start_bin: usize,
    pub apex_bin: usize,
    pub end_bin: usize,
    pub time_delta_ms: f32,
    pub brake_delta_m: Option<f32>,
    pub apex_speed_delta_kmh: f32,
    pub throttle_delta_m: Option<f32>,
    pub front_lockups: u32,
    pub rear_lockups: u32,
    pub wheelspins: u32,
    pub understeer_tenths: u32,
    pub oversteer_tenths: u32,
    pub advice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedLapDto {
    pub number: i32,
    pub time_ms: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAnalysisDto {
    pub summary: SessionSummaryDto,
    pub laps: Vec<RecordedLapDto>,
    pub hotspots: Vec<HotspotDto>,
    pub event_points: Vec<EventPointDto>,
    pub path: Vec<(u8, f32, f32)>,
    pub trace: Vec<(f32, f32)>,
    pub cornering_seconds: f64,
    pub understeer_seconds: f64,
    pub oversteer_seconds: f64,
    pub wall_hits: u32,
    pub car_hits: u32,
    pub corners: Vec<CornerCoachingDto>,
}

fn to_session_summary(s: &RecordedSession) -> SessionSummaryDto {
    SessionSummaryDto {
        id: session_id_of(s),
        car_id: s.car_id.clone(),
        skin: s.skin.clone(),
        track_id: s.track_id.clone(),
        track_layout: s.track_layout.clone(),
        full_track_id: s.full_track_id(),
        session_type: s.session_type.clone(),
        started_at: s.started_at.clone(),
        seconds: s.seconds,
        distance_m: s.distance_m,
        max_speed: s.max_speed,
        lap_count: s.laps.len(),
        best_lap_ms: s.best_lap_ms,
        crashes: s.crashes,
        offroad: s.offroad,
        front_lockups: s.front_lockups,
        rear_lockups: s.rear_lockups,
        wheelspins: s.wheelspins,
        setup_name: s.setup_name.clone(),
    }
}

#[tauri::command]
pub fn list_sessions() -> Result<Vec<SessionSummaryDto>, String> {
    let mut sessions = session_recorder::load_all(&session_recorder::sessions_dir());
    sessions.reverse();
    Ok(sessions.iter().map(to_session_summary).collect())
}

#[tauri::command]
pub fn get_session_analysis(id: String) -> Result<SessionAnalysisDto, String> {
    let sessions = session_recorder::load_all(&session_recorder::sessions_dir());
    let session = sessions
        .into_iter()
        .find(|s| session_id_of(s) == id || s.started_at == id)
        .ok_or_else(|| format!("Session not found: {id}"))?;

    let traces_dir = session_recorder::traces_dir();
    let full_track = session.full_track_id();
    let pb_trace = session_recorder::load_trace(&traces_dir, &session.car_id, &full_track, "pb");
    let last_trace = session_recorder::load_trace(&traces_dir, &session.car_id, &full_track, "last")
        .or_else(|| pb_trace.clone());

    let corners = match (pb_trace.as_ref(), last_trace.as_ref()) {
        (Some(pb), Some(cur)) => corner_coach::analyze_corners(pb, cur, &session.hotspots)
            .into_iter()
            .map(|c| CornerCoachingDto {
                number: c.corner.number,
                start_bin: c.corner.start_bin,
                apex_bin: c.corner.apex_bin,
                end_bin: c.corner.end_bin,
                time_delta_ms: c.time_delta_ms,
                brake_delta_m: c.brake_delta_m,
                apex_speed_delta_kmh: c.apex_speed_delta_kmh,
                throttle_delta_m: c.throttle_delta_m,
                front_lockups: c.front_lockups,
                rear_lockups: c.rear_lockups,
                wheelspins: c.wheelspins,
                understeer_tenths: c.understeer_tenths,
                oversteer_tenths: c.oversteer_tenths,
                advice: c.advice,
            })
            .collect(),
        _ => Vec::new(),
    };

    let summary = to_session_summary(&session);
    let laps = session
        .laps
        .iter()
        .map(|l| RecordedLapDto {
            number: l.number,
            time_ms: l.time_ms,
        })
        .collect();
    let hotspots = session
        .hotspots
        .iter()
        .map(|h| HotspotDto {
            segment: h.segment,
            kind: event_kind_str(h.kind).to_string(),
            count: h.count,
        })
        .collect();
    let event_points = session
        .event_points
        .iter()
        .map(|ep| EventPointDto {
            kind: event_kind_str(ep.kind).to_string(),
            x: ep.x,
            z: ep.z,
            count: ep.count,
        })
        .collect();

    Ok(SessionAnalysisDto {
        summary,
        laps,
        hotspots,
        event_points,
        path: session.path,
        trace: session.trace,
        cornering_seconds: session.cornering_seconds,
        understeer_seconds: session.understeer_seconds,
        oversteer_seconds: session.oversteer_seconds,
        wall_hits: session.wall_hits,
        car_hits: session.car_hits,
        corners,
    })
}

// ---------------------------------------------------------------------------
// 2. Replays: Library, Multi-Car 3D, Trimmer, AC Launcher
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayFileDto {
    pub path: String,
    pub filename: String,
    pub place: String,
    pub title: String,
    pub online: bool,
    pub bytes: u64,
    pub modified_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayFrameDto {
    pub time_s: f64,
    pub speed_kmh: f32,
    pub rpm: f32,
    pub gear: u8,
    pub throttle: f32,
    pub brake: f32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub lights_flags: u8,
    pub current_lap: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarReplayDto {
    pub car_id: String,
    pub driver: String,
    pub nation: String,
    pub team: String,
    pub skin_id: String,
    pub lap_times_ms: Vec<u32>,
    pub frames: Vec<ReplayFrameDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiCarReplayDto {
    pub track: String,
    pub config_track: Option<String>,
    pub weather: String,
    pub duration_s: f64,
    pub recording_interval_ms: f64,
    pub num_cars: usize,
    pub capped: bool,
    pub cars: Vec<CarReplayDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayTrimResultDto {
    pub out_path: String,
    pub bytes_written: usize,
}

#[tauri::command]
pub fn list_replays() -> Result<Vec<ReplayFileDto>, String> {
    let replay_dir = settings::ac_documents_dir()
        .map(|d| d.join("replay"))
        .unwrap_or_default();
    let archive_dir = session_recorder::replay_archive_dir();
    let files = replay_library::list(&replay_dir, &archive_dir);
    Ok(files
        .into_iter()
        .map(|f| {
            let filename = f
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let modified_unix_ms = f
                .modified
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            ReplayFileDto {
                path: f.path.to_string_lossy().into_owned(),
                filename,
                place: f.place.label().to_string(),
                title: f.title,
                online: f.online,
                bytes: f.bytes,
                modified_unix_ms,
            }
        })
        .collect())
}

#[tauri::command]
pub fn load_replay_multicar(path: String) -> Result<MultiCarReplayDto, String> {
    const MAX_FRAMES: usize = 12_000;
    let mc = replay::read_replay_multicar(Path::new(&path), MAX_FRAMES)
        .map_err(|e| e.to_string())?;
    Ok(MultiCarReplayDto {
        track: mc.info.track,
        config_track: mc.info.config_track,
        weather: mc.info.weather,
        duration_s: mc.info.duration_s,
        recording_interval_ms: mc.recording_interval_ms,
        num_cars: mc.info.num_cars,
        capped: mc.capped,
        cars: mc
            .cars
            .into_iter()
            .map(|c| CarReplayDto {
                car_id: c.car_id,
                driver: c.driver,
                nation: c.nation,
                team: c.team,
                skin_id: c.skin_id,
                lap_times_ms: c.lap_times_ms,
                frames: c
                    .frames
                    .into_iter()
                    .map(|f| ReplayFrameDto {
                        time_s: f.sample.time_s,
                        speed_kmh: f.sample.speed_kmh,
                        rpm: f.sample.rpm,
                        gear: f.sample.gear,
                        throttle: f.sample.throttle,
                        brake: f.sample.brake,
                        position: [f.sample.x, f.sample.y, f.sample.z],
                        rotation: f.rotation,
                        lights_flags: f.lights_flags,
                        current_lap: f.current_lap,
                    })
                    .collect(),
            })
            .collect(),
    })
}

#[tauri::command]
pub fn trim_replay_clip(
    path: String,
    start_frame: u32,
    end_frame: u32,
    out_path: String,
) -> Result<ReplayTrimResultDto, String> {
    let bytes_written =
        replay::trim_replay_file(Path::new(&path), Path::new(&out_path), start_frame, end_frame)
            .map_err(|e| e.to_string())?;
    Ok(ReplayTrimResultDto {
        out_path,
        bytes_written,
    })
}

#[tauri::command]
pub fn launch_replay_in_ac(state: State<'_, AppState>, filename: String) -> Result<(), String> {
    let ac_root = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        s.ac_root_path()
            .ok_or_else(|| "Assetto Corsa root folder is not configured".to_string())?
    };
    let cfg_dir = settings::ac_user_cfg_dir()
        .ok_or_else(|| "Assetto Corsa user cfg directory could not be located".to_string())?;
    race::write_replay_ini(&cfg_dir, &filename).map_err(|e| e.to_string())?;
    game::launch_game(&ac_root).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. Trajectory Analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectedPointDto {
    pub x: f32,
    pub z: f32,
    pub speed_kmh: f32,
    pub throttle: f32,
    pub brake: f32,
    pub gear: u8,
    pub elapsed_ms: f64,
    pub norm_pos: f32,
    pub distance_m: f32,
    pub lateral_offset_m: f32,
    pub delta_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerMetricsDto {
    pub name: String,
    pub start_norm: f32,
    pub end_norm: f32,
    pub braking_norm: Option<f32>,
    pub min_speed_kmh: f32,
    pub apex_margin_m: Option<f32>,
    pub throttle_on_norm: Option<f32>,
    pub time_delta_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryAnalysisDto {
    pub projected: Vec<ProjectedPointDto>,
    pub reference_projected: Vec<ProjectedPointDto>,
    pub corners: Vec<CornerMetricsDto>,
    pub total_delta_ms: Option<f64>,
}

#[tauri::command]
pub fn analyze_trajectory(
    track_path: String,
    layout: Option<String>,
    replay_a: String,
    replay_b: Option<String>,
) -> Result<TrajectoryAnalysisDto, String> {
    let track_dir = Path::new(&track_path);
    let layout_trimmed = layout.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let layout_dir = match layout_trimmed {
        Some(l) if track_dir.join(l).is_dir() => track_dir.join(l),
        _ => track_dir.to_path_buf(),
    };
    let ai_path = if layout_dir.join("ai").join("fast_lane.ai").is_file() {
        layout_dir.join("ai").join("fast_lane.ai")
    } else {
        track_dir.join("ai").join("fast_lane.ai")
    };
    let spline = ai_spline::read_ai_spline(&ai_path).map_err(|e| e.to_string())?;

    let sections_candidates = [
        layout_trimmed.map(|l| track_dir.join("ui").join(l).join("sections.ini")),
        Some(track_dir.join("ui").join("sections.ini")),
        Some(layout_dir.join("data").join("sections.ini")),
        Some(track_dir.join("data").join("sections.ini")),
    ];
    let sections = sections_candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .and_then(|p| track_files::read_sections(&p).ok())
        .unwrap_or_default();

    let samples_a =
        replay::read_replay_samples(Path::new(&replay_a)).map_err(|e| e.to_string())?;
    let lap_a = samples_to_trajectory_lap(samples_a.info().driver.clone(), samples_a.samples());

    let lap_b = match replay_b.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(path_b) => {
            let sb = replay::read_replay_samples(Path::new(path_b)).map_err(|e| e.to_string())?;
            Some(samples_to_trajectory_lap(sb.info().driver.clone(), sb.samples()))
        }
        None => None,
    };

    let analysis = trajectory::analyze_trajectory(&spline, &lap_a, lap_b.as_ref(), &sections);

    let map_pts = |pts: Vec<trajectory::ProjectedPoint>| -> Vec<ProjectedPointDto> {
        pts.into_iter()
            .map(|p| ProjectedPointDto {
                x: p.point.x,
                z: p.point.z,
                speed_kmh: p.point.speed_kmh,
                throttle: p.point.throttle,
                brake: p.point.brake,
                gear: p.point.gear,
                elapsed_ms: p.point.elapsed_ms,
                norm_pos: p.norm_pos,
                distance_m: p.distance_m,
                lateral_offset_m: p.lateral_offset_m,
                delta_ms: p.delta_ms,
            })
            .collect()
    };

    Ok(TrajectoryAnalysisDto {
        projected: map_pts(analysis.projected),
        reference_projected: map_pts(analysis.reference_projected),
        corners: analysis
            .corners
            .into_iter()
            .map(|c| CornerMetricsDto {
                name: c.name,
                start_norm: c.start_norm,
                end_norm: c.end_norm,
                braking_norm: c.braking_norm,
                min_speed_kmh: c.min_speed_kmh,
                apex_margin_m: c.apex_margin_m,
                throttle_on_norm: c.throttle_on_norm,
                time_delta_ms: c.time_delta_ms,
            })
            .collect(),
        total_delta_ms: analysis.total_delta_ms,
    })
}

// ---------------------------------------------------------------------------
// 4. Race Strategy Calculator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyRequestDto {
    pub fuel_left: f64,
    pub fuel_capacity: f64,
    pub fuel_per_lap: f64,
    pub race_laps: u32,
    pub completed_laps: u32,
    pub tyre_life_laps: u32,
    pub min_stint_laps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuelStatusDto {
    pub laps_to_go: u32,
    pub laps_covered: u32,
    pub laps_short: u32,
    pub fuel_needed: f64,
    pub surplus: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PitWindowDto {
    pub first_lap: u32,
    pub last_lap: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StintDto {
    pub number: u32,
    pub first_lap: u32,
    pub last_lap: u32,
    pub laps: u32,
    pub fuel_load: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyResultDto {
    pub fuel: FuelStatusDto,
    pub pit_window: Option<PitWindowDto>,
    pub stops: u32,
    pub stop_laps: Vec<u32>,
    pub stints: Vec<StintDto>,
}

#[tauri::command]
pub fn compute_strategy(req: StrategyRequestDto) -> Result<StrategyResultDto, String> {
    let fuel = strategy::fuel_status(
        req.fuel_left,
        req.fuel_per_lap,
        req.race_laps,
        req.completed_laps,
    )
    .map_err(|e| e.to_string())?;

    let window = strategy::pit_window(req.race_laps, req.tyre_life_laps, req.min_stint_laps)
        .map_err(|e| e.to_string())?;

    let plan = strategy::pit_plan(
        req.race_laps,
        req.fuel_capacity,
        req.fuel_per_lap,
        req.tyre_life_laps,
        req.min_stint_laps,
    )
    .map_err(|e| e.to_string())?;

    let stops = plan.stops();
    let stop_laps = plan.stop_laps();

    Ok(StrategyResultDto {
        fuel: FuelStatusDto {
            laps_to_go: fuel.laps_to_go,
            laps_covered: fuel.laps_covered,
            laps_short: fuel.laps_short,
            fuel_needed: fuel.fuel_needed,
            surplus: fuel.surplus,
        },
        pit_window: window.map(|w| PitWindowDto {
            first_lap: w.first_lap,
            last_lap: w.last_lap,
        }),
        stops,
        stop_laps,
        stints: plan
            .stints
            .into_iter()
            .map(|s| StintDto {
                number: s.number,
                first_lap: s.first_lap,
                last_lap: s.last_lap,
                laps: s.laps,
                fuel_load: s.fuel_load,
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// 5. Timing Tower & Lap Times
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingEntryDto {
    pub driver: String,
    pub position: u32,
    pub laps: u32,
    pub last_ms: Option<u32>,
    pub best_ms: Option<u32>,
    pub gap_ms: Option<i64>,
    pub best_formatted: Option<String>,
    pub gap_formatted: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingTowerDto {
    pub source: String,
    pub track: Option<String>,
    pub session_best_driver: Option<String>,
    pub session_best_ms: Option<u32>,
    pub entries: Vec<TimingEntryDto>,
}

#[tauri::command]
pub fn load_timing_tower(csv_or_none: Option<String>) -> Result<TimingTowerDto, String> {
    let (source, track, entries) = match csv_or_none.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(csv) => {
            let parsed = timing::parse_csv(csv).map_err(|e| e.to_string())?;
            let tower = timing::build_tower(parsed).map_err(|e| e.to_string())?;
            ("csv".to_string(), None, tower)
        }
        None => {
            let docs = settings::ac_documents_dir()
                .ok_or_else(|| "Assetto Corsa Documents directory not found".to_string())?;
            let race_out_path = docs.join("out").join("race_out.json");
            let summary = race_out::load_race_out(&race_out_path)
                .map_err(|e| format!("Failed to load {}: {e}", race_out_path.display()))?;
            let raw_entries: Vec<timing::Entry> = summary
                .results
                .iter()
                .map(|r| timing::Entry {
                    driver: r.driver.clone(),
                    position: r.position,
                    laps: 0,
                    last_ms: None,
                    best_ms: if r.fastest_lap {
                        summary.player_best_lap_ms
                    } else {
                        None
                    },
                    gap_ms: None,
                })
                .collect();
            let tower = timing::build_tower(raw_entries).map_err(|e| e.to_string())?;
            ("race_out.json".to_string(), Some(summary.track), tower)
        }
    };

    let best = timing::session_best(&entries);
    Ok(TimingTowerDto {
        source,
        track,
        session_best_driver: best.as_ref().map(|(d, _)| d.clone()),
        session_best_ms: best.map(|(_, ms)| ms),
        entries: entries
            .into_iter()
            .map(|e| TimingEntryDto {
                driver: e.driver,
                position: e.position,
                laps: e.laps,
                last_ms: e.last_ms,
                best_ms: e.best_ms,
                gap_ms: e.gap_ms,
                best_formatted: e.best_ms.map(timing::format_lap),
                gap_formatted: e.gap_ms.map(timing::format_gap),
            })
            .collect(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LapRecordDto {
    pub track: String,
    pub layout: Option<String>,
    pub car: String,
    pub driver: String,
    pub lap_time_ms: u32,
    pub lap_time_formatted: String,
    pub sectors_ms: Option<Vec<u32>>,
    pub recorded_at: String,
}

#[tauri::command]
pub fn list_laptimes(state: State<'_, AppState>) -> Result<Vec<LapRecordDto>, String> {
    let store_path = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        if let Some(root) = s.ac_root_path() {
            root.join("acxs-laptimes.json")
        } else {
            settings::settings_path()
                .parent()
                .map(|p| p.join("acxs-laptimes.json"))
                .unwrap_or_else(|| Path::new("acxs-laptimes.json").to_path_buf())
        }
    };

    let store = LapStore::load(&store_path).map_err(|e| e.to_string())?;
    let mut records: Vec<LapRecordDto> = store
        .records()
        .iter()
        .map(|r| LapRecordDto {
            track: r.track.clone(),
            layout: r.layout.clone(),
            car: r.car.clone(),
            driver: r.driver.clone(),
            lap_time_ms: r.lap_time_ms,
            lap_time_formatted: laptimes::format_lap(r.lap_time_ms),
            sectors_ms: r.sectors_ms.clone(),
            recorded_at: r.recorded_at.clone(),
        })
        .collect();
    records.reverse();
    Ok(records)
}

// ---------------------------------------------------------------------------
// 6. Live UDP Telemetry & Web Dashboard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeDto {
    pub car: String,
    pub driver: String,
    pub track: String,
    pub track_config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarUpdateDto {
    pub speed_kmh: f32,
    pub gear: i8,
    pub rpm: u16,
    pub position: [f32; 3],
    pub lap_time_ms: u32,
    pub throttle: f32,
    pub brake: f32,
    pub steer_angle: f32,
    pub g_lateral: f32,
    pub g_longitudinal: f32,
    pub g_vertical: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveLapDto {
    pub car_id: u8,
    pub lap_time_ms: u32,
    pub laps: u16,
    pub driver: String,
    pub car: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTelemetryDto {
    pub connected: bool,
    pub handshake: Option<HandshakeDto>,
    pub car_update: Option<CarUpdateDto>,
    pub last_lap: Option<LiveLapDto>,
    pub dashboard_running: bool,
    pub dashboard_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDashboardStatusDto {
    pub running: bool,
    pub url: Option<String>,
}

#[tauri::command]
pub fn udp_connect(
    state: State<'_, AppState>,
    host: String,
    port: u16,
) -> Result<String, String> {
    let addr_str = format!("{}:{}", host.trim(), port);
    let addr: SocketAddr = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("Invalid address '{addr_str}': {e}"))?
        .next()
        .ok_or_else(|| format!("Could not resolve '{addr_str}'"))?;

    let client = TelemetryClient::connect(addr, Duration::from_millis(20))
        .map_err(|e| format!("UDP connect failed: {e}"))?;
    let _ = client.subscribe_updates(33);

    let mut guard = state
        .udp_client
        .lock()
        .map_err(|_| "udp_client lock poisoned".to_string())?;
    *guard = Some(client);

    Ok(addr.to_string())
}

#[tauri::command]
pub fn udp_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state
        .udp_client
        .lock()
        .map_err(|_| "udp_client lock poisoned".to_string())?;
    if let Some(client) = guard.take() {
        let _ = client.send_dismiss();
    }
    Ok(())
}

#[tauri::command]
pub fn get_live_telemetry(state: State<'_, AppState>) -> Result<LiveTelemetryDto, String> {
    let (dashboard_running, dashboard_url) = {
        let dash = state
            .dashboard
            .lock()
            .map_err(|_| "dashboard lock poisoned".to_string())?;
        match dash.as_ref() {
            Some(d) => (true, Some(format!("http://{}", d.local_addr()))),
            None => (false, None),
        }
    };

    let guard = state
        .udp_client
        .lock()
        .map_err(|_| "udp_client lock poisoned".to_string())?;
    let Some(client) = guard.as_ref() else {
        return Ok(LiveTelemetryDto {
            connected: false,
            handshake: None,
            car_update: None,
            last_lap: None,
            dashboard_running,
            dashboard_url,
        });
    };

    let mut handshake = None;
    let mut car_update = None;
    let mut last_lap = None;

    for _ in 0..16 {
        match client.recv_packet() {
            Ok(Packet::Handshake(h)) => {
                handshake = Some(HandshakeDto {
                    car: h.car,
                    driver: h.driver,
                    track: h.track,
                    track_config: h.track_config,
                });
            }
            Ok(Packet::CarUpdate(u)) => {
                car_update = Some(CarUpdateDto {
                    speed_kmh: u.speed_kmh,
                    gear: u.gear,
                    rpm: u.rpm,
                    position: [u.position.0, u.position.1, u.position.2],
                    lap_time_ms: u.lap_time_ms,
                    throttle: u.throttle,
                    brake: u.brake,
                    steer_angle: u.steer_angle,
                    g_lateral: u.g_lateral,
                    g_longitudinal: u.g_longitudinal,
                    g_vertical: u.g_vertical,
                });
            }
            Ok(Packet::LapCompleted(l)) => {
                last_lap = Some(LiveLapDto {
                    car_id: l.car_id,
                    lap_time_ms: l.lap_time_ms,
                    laps: l.laps,
                    driver: l.driver,
                    car: l.car,
                });
            }
            Ok(Packet::Other(_)) => {}
            Err(ClientError::Timeout) => break,
            Err(ClientError::Io(_)) | Err(ClientError::Decode(_)) => break,
        }
    }

    Ok(LiveTelemetryDto {
        connected: true,
        handshake,
        car_update,
        last_lap,
        dashboard_running,
        dashboard_url,
    })
}

#[tauri::command]
pub fn toggle_web_dashboard(
    state: State<'_, AppState>,
    port: u16,
    lan: bool,
    start: bool,
) -> Result<WebDashboardStatusDto, String> {
    let mut guard = state
        .dashboard
        .lock()
        .map_err(|_| "dashboard lock poisoned".to_string())?;

    if !start {
        if let Some(d) = guard.take() {
            d.stop();
        }
        return Ok(WebDashboardStatusDto {
            running: false,
            url: None,
        });
    }

    if let Some(old) = guard.take() {
        old.stop();
    }

    let bind = if lan { "0.0.0.0" } else { "127.0.0.1" };
    let dash = Dashboard::start(bind, port).map_err(|e| e.to_string())?;
    let url = format!("http://{}", dash.local_addr());
    *guard = Some(dash);

    Ok(WebDashboardStatusDto {
        running: true,
        url: Some(url),
    })
}
