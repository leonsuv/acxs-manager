//! Online multiplayer & dedicated server commands (directory, live status, watchlist, LAN, URIs,
//! join/booking, missing-content auto-download, dedicated server config/process, CSP extras, RCON).

use std::collections::HashSet;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use acxs_manager::content_dl::{self, ContentDownloadPlan};
use acxs_manager::csp;
use acxs_manager::directory::{self, RecentServer, RecentServers};
use acxs_manager::game;
use acxs_manager::ini::Ini;
use acxs_manager::lan_discovery;
use acxs_manager::lobby::{self, LobbyServer};
use acxs_manager::race::{self, OnlineJoin};
use acxs_manager::server::{
    self, DynamicTrack, Entry, RaceSessionCfg, RealismConfig, ServerConfig, SessionCfg,
    VotingPluginsConfig, WeatherSlot,
};
use acxs_manager::server_csp::{self, CspExtraOptions, ExtraCfgYml};
use acxs_manager::server_process::{self, LaunchArgs, ServerProcess, Status};
use acxs_manager::server_rcon::{AdminCmd, RconClient, RconError, ServerEvent};
use acxs_manager::server_status::{self, BookingRequest, ServerCar, ServerStatus};
use acxs_manager::settings;
use acxs_manager::steam;
use acxs_manager::uri;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

/// Serializable summary of a public directory / lobby server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbyServerDto {
    pub key: String,
    pub ip: String,
    pub http_port: u16,
    pub race_port: u16,
    pub name: String,
    pub clients: u32,
    pub capacity: u32,
    pub track: String,
    pub track_name: String,
    pub cars: Vec<String>,
    pub car_names: Vec<String>,
    pub player_names: Vec<String>,
    pub session: u32,
    pub countries: Vec<String>,
    pub password: bool,
    pub csp_required: bool,
    pub region: String,
}

impl From<&LobbyServer> for LobbyServerDto {
    fn from(s: &LobbyServer) -> Self {
        Self {
            key: s.key(),
            ip: s.ip.clone(),
            http_port: s.http_port,
            race_port: s.race_port,
            name: s.name.clone(),
            clients: s.clients,
            capacity: s.capacity,
            track: s.track.clone(),
            track_name: s.track_name.clone(),
            cars: s.cars.clone(),
            car_names: s.car_names.clone(),
            player_names: s.player_names.clone(),
            session: s.session,
            countries: s.countries.clone(),
            password: s.password,
            csp_required: s.csp_required,
            region: s.region.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCarDto {
    pub model: String,
    pub skin: String,
    pub driver: String,
    pub connected: bool,
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDetailDto {
    pub host: String,
    pub http_port: u16,
    pub race_port: Option<u16>,
    pub online: bool,
    pub latency_ms: u64,
    pub name: Option<String>,
    pub raw_track: Option<String>,
    pub track_id: Option<String>,
    pub track_layout: Option<String>,
    pub track_owned: bool,
    pub min_csp_build: Option<u32>,
    pub installed_csp_build: Option<u32>,
    pub csp_satisfied: bool,
    pub clients: Option<u32>,
    pub max_clients: Option<u32>,
    pub session: Option<String>,
    pub players: Vec<String>,
    pub cars: Vec<ServerCarDto>,
    pub features: Vec<String>,
    pub missing_cars: Vec<String>,
    pub download_plan: ContentDownloadPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchlistEntryDto {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentServerDto {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub track: String,
    pub last_car: String,
    pub last_skin: String,
    pub last_joined_unix: u64,
    pub join_count: u32,
}

impl From<&RecentServer> for RecentServerDto {
    fn from(r: &RecentServer) -> Self {
        Self {
            host: r.host.clone(),
            port: r.port,
            name: r.name.clone(),
            track: r.track.clone(),
            last_car: r.last_car.clone(),
            last_skin: r.last_skin.clone(),
            last_joined_unix: r.last_joined_unix,
            join_count: r.join_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServerDto {
    pub host: String,
    pub http_port: u16,
    pub race_port: Option<u16>,
    pub name: String,
    pub latency_ms: Option<u64>,
    pub track: Option<String>,
    pub clients: Option<u32>,
    pub max_clients: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinUriDto {
    pub host: String,
    pub http_port: u16,
    pub password: Option<String>,
    pub car: Option<String>,
    pub skin: Option<String>,
    pub min_csp_build: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineJoinRequest {
    pub ip: String,
    pub port: u16,
    pub http_port: u16,
    #[serde(default)]
    pub server_name: String,
    pub track: String,
    #[serde(default)]
    pub track_layout: Option<String>,
    pub car: String,
    #[serde(default)]
    pub skin: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub steam_id: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinResultDto {
    pub pid: u32,
    pub race_ini_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineBookRequest {
    pub host: String,
    pub http_port: u16,
    pub driver_name: String,
    #[serde(default)]
    pub steam_guid: Option<String>,
    pub car_model: String,
    #[serde(default)]
    pub car_skin: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingReplyDto {
    pub accepted: bool,
    pub message: String,
    pub booking_time_sec: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledItemReportDto {
    pub kind: String,
    pub id: String,
    pub installed_to: String,
    pub file_count: usize,
    pub moved_existing_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchInstallReportDto {
    pub installed: Vec<InstalledItemReportDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicTrackDto {
    pub session_start: u32,
    pub randomness: u32,
    pub session_transfer: u32,
    pub lap_gain: u32,
}

impl From<&DynamicTrack> for DynamicTrackDto {
    fn from(dt: &DynamicTrack) -> Self {
        Self {
            session_start: dt.session_start,
            randomness: dt.randomness,
            session_transfer: dt.session_transfer,
            lap_gain: dt.lap_gain,
        }
    }
}

impl From<&DynamicTrackDto> for DynamicTrack {
    fn from(dt: &DynamicTrackDto) -> Self {
        Self {
            session_start: dt.session_start,
            randomness: dt.randomness,
            session_transfer: dt.session_transfer,
            lap_gain: dt.lap_gain,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfigDto {
    pub name: String,
    pub password: String,
    pub admin_password: String,
    pub cars: Vec<String>,
    pub track: String,
    pub config_track: String,
    pub max_clients: u32,
    pub udp_port: u16,
    pub tcp_port: u16,
    pub http_port: u16,
    pub loop_mode: bool,
    pub sun_angle: i32,
    pub pickup_mode: bool,
    pub dynamic_track: Option<DynamicTrackDto>,
    pub booking: Option<SessionCfg>,
    pub practice: Option<SessionCfg>,
    pub qualify: Option<SessionCfg>,
    pub race: Option<RaceSessionCfg>,
    pub weather: Vec<WeatherSlot>,
    pub realism: Option<RealismConfig>,
    pub voting_plugins: Option<VotingPluginsConfig>,
}

impl From<&ServerConfig> for ServerConfigDto {
    fn from(c: &ServerConfig) -> Self {
        Self {
            name: c.name.clone(),
            password: c.password.clone(),
            admin_password: c.admin_password.clone(),
            cars: c.cars.clone(),
            track: c.track.clone(),
            config_track: c.config_track.clone(),
            max_clients: c.max_clients,
            udp_port: c.udp_port,
            tcp_port: c.tcp_port,
            http_port: c.http_port,
            loop_mode: c.loop_mode,
            sun_angle: c.sun_angle,
            pickup_mode: c.pickup_mode,
            dynamic_track: c.dynamic_track.as_ref().map(DynamicTrackDto::from),
            booking: c.booking.clone(),
            practice: c.practice.clone(),
            qualify: c.qualify.clone(),
            race: c.race.clone(),
            weather: c.weather.clone(),
            realism: c.realism.clone(),
            voting_plugins: c.voting_plugins.clone(),
        }
    }
}

impl From<&ServerConfigDto> for ServerConfig {
    fn from(c: &ServerConfigDto) -> Self {
        Self {
            name: c.name.clone(),
            password: c.password.clone(),
            admin_password: c.admin_password.clone(),
            cars: c.cars.clone(),
            track: c.track.clone(),
            config_track: c.config_track.clone(),
            max_clients: c.max_clients,
            udp_port: c.udp_port,
            tcp_port: c.tcp_port,
            http_port: c.http_port,
            loop_mode: c.loop_mode,
            sun_angle: c.sun_angle,
            pickup_mode: c.pickup_mode,
            dynamic_track: c.dynamic_track.as_ref().map(DynamicTrack::from),
            booking: c.booking.clone(),
            practice: c.practice.clone(),
            qualify: c.qualify.clone(),
            race: c.race.clone(),
            weather: c.weather.clone(),
            realism: c.realism.clone(),
            voting_plugins: c.voting_plugins.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerEntryDto {
    pub model: String,
    pub skin: String,
    pub guid: String,
    pub driver_name: String,
    pub spectator: bool,
    pub ballast_kg: u32,
    pub restrictor: u32,
}

impl Default for ServerEntryDto {
    fn default() -> Self {
        Self {
            model: String::new(),
            skin: String::new(),
            guid: String::new(),
            driver_name: String::new(),
            spectator: false,
            ballast_kg: 0,
            restrictor: 0,
        }
    }
}

impl From<&Entry> for ServerEntryDto {
    fn from(e: &Entry) -> Self {
        Self {
            model: e.model.clone(),
            skin: e.skin.clone(),
            guid: e.guid.clone(),
            driver_name: e.driver_name.clone(),
            spectator: e.spectator,
            ballast_kg: e.ballast_kg,
            restrictor: e.restrictor,
        }
    }
}

impl From<&ServerEntryDto> for Entry {
    fn from(e: &ServerEntryDto) -> Self {
        Self {
            model: e.model.clone(),
            skin: e.skin.clone(),
            guid: e.guid.clone(),
            driver_name: e.driver_name.clone(),
            spectator: e.spectator,
            ballast_kg: e.ballast_kg,
            restrictor: e.restrictor,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedicatedServerBundleDto {
    pub cfg: ServerConfigDto,
    pub entries: Vec<ServerEntryDto>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedicatedServerStatusDto {
    pub status: String,
    pub exit_code: Option<i32>,
    pub recent_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CspExtraOptionsDto {
    pub allow_wrong_way: bool,
    pub enforce_back_to_pits_penalty: bool,
    pub no_collisions: bool,
    pub pits_keep_collisions: bool,
    pub pits_non_lethal: bool,
    pub pits_speed_kmh: u32,
    pub use_custom_weather: bool,
    pub force_rain: bool,
    pub content_url: String,
    pub minimum_csp_build: u32,
}

impl Default for CspExtraOptionsDto {
    fn default() -> Self {
        Self::from(&CspExtraOptions::default())
    }
}

impl From<&CspExtraOptions> for CspExtraOptionsDto {
    fn from(o: &CspExtraOptions) -> Self {
        Self {
            allow_wrong_way: o.allow_wrong_way,
            enforce_back_to_pits_penalty: o.enforce_back_to_pits_penalty,
            no_collisions: o.no_collisions,
            pits_keep_collisions: o.pits_keep_collisions,
            pits_non_lethal: o.pits_non_lethal,
            pits_speed_kmh: o.pits_speed_kmh,
            use_custom_weather: o.use_custom_weather,
            force_rain: o.force_rain,
            content_url: o.content_url.clone(),
            minimum_csp_build: o.minimum_csp_build,
        }
    }
}

impl From<&CspExtraOptionsDto> for CspExtraOptions {
    fn from(o: &CspExtraOptionsDto) -> Self {
        Self {
            allow_wrong_way: o.allow_wrong_way,
            enforce_back_to_pits_penalty: o.enforce_back_to_pits_penalty,
            no_collisions: o.no_collisions,
            pits_keep_collisions: o.pits_keep_collisions,
            pits_non_lethal: o.pits_non_lethal,
            pits_speed_kmh: o.pits_speed_kmh,
            use_custom_weather: o.use_custom_weather,
            force_rain: o.force_rain,
            content_url: o.content_url.clone(),
            minimum_csp_build: o.minimum_csp_build,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtraCfgYmlDto {
    pub enable_ai: bool,
    pub enable_client_messages: bool,
    pub enable_real_penalty: bool,
    pub content_url: String,
    pub minimum_csp_version: u32,
    pub ai_max_density: u32,
    pub ai_max_speed_kph: u32,
}

impl Default for ExtraCfgYmlDto {
    fn default() -> Self {
        Self::from(&ExtraCfgYml::default())
    }
}

impl From<&ExtraCfgYml> for ExtraCfgYmlDto {
    fn from(c: &ExtraCfgYml) -> Self {
        Self {
            enable_ai: c.enable_ai,
            enable_client_messages: c.enable_client_messages,
            enable_real_penalty: c.enable_real_penalty,
            content_url: c.content_url.clone(),
            minimum_csp_version: c.minimum_csp_version,
            ai_max_density: c.ai_max_density,
            ai_max_speed_kph: c.ai_max_speed_kph,
        }
    }
}

impl From<&ExtraCfgYmlDto> for ExtraCfgYml {
    fn from(c: &ExtraCfgYmlDto) -> Self {
        Self {
            enable_ai: c.enable_ai,
            enable_client_messages: c.enable_client_messages,
            enable_real_penalty: c.enable_real_penalty,
            content_url: c.content_url.clone(),
            minimum_csp_version: c.minimum_csp_version,
            ai_max_density: c.ai_max_density,
            ai_max_speed_kph: c.ai_max_speed_kph,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCspBundleDto {
    pub csp_extra_options: CspExtraOptionsDto,
    pub extra_cfg_yml: Option<ExtraCfgYmlDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RconConnectResultDto {
    pub local_addr: String,
    pub server_addr: String,
}

/// RCON command input: accepts either a structured command object or a raw admin command string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RconCommandInput {
    Structured(RconCommandDto),
    Raw(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RconCommandDto {
    SetRealtimePosInterval { interval_ms: u16 },
    GetCarInfo { car_id: u8 },
    SendChat { car_id: u8, message: String },
    BroadcastChat { message: String },
    GetSessionInfo { session_index: i16 },
    KickUser { car_id: u8 },
    NextSession,
    RestartSession,
    AdminCommand { command: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RconReplyDto {
    pub sent: bool,
    pub event: Option<String>,
}

fn acxs_config_dir() -> PathBuf {
    settings::settings_path()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn watchlist_path() -> PathBuf {
    acxs_config_dir().join("servers.json")
}

fn recent_servers_path() -> PathBuf {
    acxs_config_dir().join("recent-servers.json")
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

fn read_entry_list(cfg_dir: &Path) -> Vec<Entry> {
    let path = cfg_dir.join(server::ENTRY_LIST);
    let Ok(ini) = Ini::load(&path) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for i in 0..255 {
        let sec = format!("CAR_{i}");
        let has_sec = ini.section_names().iter().any(|s| s.eq_ignore_ascii_case(&sec));
        if !has_sec {
            break;
        }
        entries.push(Entry {
            model: ini.get(&sec, "MODEL").unwrap_or_default().to_string(),
            skin: ini.get(&sec, "SKIN").unwrap_or_default().to_string(),
            guid: ini.get(&sec, "GUID").unwrap_or_default().to_string(),
            driver_name: ini.get(&sec, "DRIVERNAME").unwrap_or_default().to_string(),
            spectator: ini.get_i64(&sec, "SPECTATOR_MODE").unwrap_or(0) != 0,
            ballast_kg: ini
                .get_i64(&sec, "BALLAST")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            restrictor: ini
                .get_i64(&sec, "RESTRICTOR")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
        });
    }
    entries
}

fn process_status_dto(proc: &mut ServerProcess) -> DedicatedServerStatusDto {
    let (status, exit_code) = match proc.status() {
        Status::Stopped => ("stopped".to_string(), None),
        Status::Running => ("running".to_string(), None),
        Status::Exited(code) => ("exited".to_string(), code),
    };
    DedicatedServerStatusDto {
        status,
        exit_code,
        recent_lines: proc.recent_lines(),
    }
}

/// Fetches live public servers from `acservers.online` in a blocking thread and caches them in state.
#[tauri::command]
pub async fn fetch_directory_servers(
    state: State<'_, AppState>,
    query: Option<String>,
    has_players: bool,
) -> Result<Vec<LobbyServerDto>, String> {
    let query_owned = query.filter(|q| !q.trim().is_empty());
    let servers = tauri::async_runtime::spawn_blocking(move || {
        directory::fetch_all(
            query_owned.as_deref(),
            has_players,
            10,
            Duration::from_secs(12),
            |_| {},
        )
    })
    .await
    .map_err(|e| format!("Directory worker failed: {e}"))??;

    let dtos: Vec<LobbyServerDto> = servers.iter().map(LobbyServerDto::from).collect();
    {
        let mut cached = state
            .lobby_servers
            .lock()
            .map_err(|_| "lobby_servers lock poisoned".to_string())?;
        *cached = servers;
    }
    Ok(dtos)
}

/// Fetches `/INFO` + `/JSON|-1` for a server, resolves track & CSP requirement, and computes missing content URLs.
#[tauri::command]
pub async fn fetch_server_detail(
    state: State<'_, AppState>,
    host: String,
    port: u16,
) -> Result<ServerDetailDto, String> {
    let host_clone = host.clone();
    let (status, latency_ms): (ServerStatus, u64) =
        tauri::async_runtime::spawn_blocking(move || {
            let started = Instant::now();
            let status = server_status::fetch(&host_clone, port, Duration::from_secs(6))
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((status, started.elapsed().as_millis() as u64))
        })
        .await
        .map_err(|e| format!("Server status worker failed: {e}"))??;

    let key = format!("{host}:{port}");
    {
        let mut cache = state
            .server_statuses
            .lock()
            .map_err(|_| "server_statuses lock poisoned".to_string())?;
        cache.insert(key, status.clone());
    }

    let owned_car_ids: HashSet<String> = state
        .cars
        .lock()
        .map_err(|_| "cars lock poisoned".to_string())?
        .iter()
        .map(|c| c.id.to_ascii_lowercase())
        .collect();
    let installed_tracks: Vec<String> = state
        .tracks
        .lock()
        .map_err(|_| "tracks lock poisoned".to_string())?
        .iter()
        .map(|t| t.id.clone())
        .collect();
    let installed_track_refs: Vec<&str> = installed_tracks.iter().map(String::as_str).collect();
    let owned_track_ids: HashSet<String> = installed_tracks
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();

    let resolved = status
        .track
        .as_deref()
        .map(|raw| server_status::resolve_track_id(raw, &installed_track_refs));
    let track_id = resolved.as_ref().map(|r| r.track_id.clone());
    let track_layout = resolved.as_ref().and_then(|r| r.layout.clone());
    let min_csp_build = resolved.as_ref().and_then(|r| r.min_csp_build);
    let track_owned = track_id
        .as_deref()
        .is_some_and(|id| owned_track_ids.contains(&id.to_ascii_lowercase()));

    let installed_csp_build = resolve_ac_root(&state)
        .ok()
        .and_then(|root| csp::installed_build(&root));
    let csp_satisfied = match (min_csp_build, installed_csp_build) {
        (Some(req), Some(inst)) => inst >= req,
        (Some(_), None) => false,
        (None, _) => true,
    };

    let mut missing_set = HashSet::new();
    let mut missing_cars = Vec::new();
    let cars: Vec<ServerCarDto> = status
        .cars
        .iter()
        .map(|c: &ServerCar| {
            let owned = owned_car_ids.contains(&c.model.to_ascii_lowercase());
            if !owned && !c.model.is_empty() && missing_set.insert(c.model.clone()) {
                missing_cars.push(c.model.clone());
            }
            ServerCarDto {
                model: c.model.clone(),
                skin: c.skin.clone(),
                driver: c.driver.clone(),
                connected: c.connected,
                owned,
            }
        })
        .collect();

    let missing_track = if track_owned {
        None
    } else {
        track_id.as_deref()
    };
    let download_plan = content_dl::parse_server_content_urls(
        &status.raw_info,
        &status.raw_entries,
        missing_track,
        &missing_cars,
    );

    Ok(ServerDetailDto {
        host,
        http_port: port,
        race_port: status.port,
        online: status.online,
        latency_ms,
        name: status.name,
        raw_track: status.track,
        track_id,
        track_layout,
        track_owned,
        min_csp_build,
        installed_csp_build,
        csp_satisfied,
        clients: status.clients,
        max_clients: status.max_clients,
        session: status.session,
        players: status.players,
        cars,
        features: status.features,
        missing_cars,
        download_plan,
    })
}

/// Lists saved watchlist servers from `servers.json`.
#[tauri::command]
pub fn list_watchlist() -> Result<Vec<WatchlistEntryDto>, String> {
    let path = watchlist_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<WatchlistEntryDto>>(&text)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))
}

/// Saves the server watchlist atomically to `servers.json`.
#[tauri::command]
pub fn save_watchlist(entries: Vec<WatchlistEntryDto>) -> Result<(), String> {
    let path = watchlist_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    fs::write(&tmp, text).map_err(|e| format!("Failed to write watchlist: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Failed to save watchlist: {e}"))
}

/// Lists recently joined servers from `recent-servers.json`.
#[tauri::command]
pub fn list_recent_servers() -> Result<Vec<RecentServerDto>, String> {
    let store = RecentServers::load(&recent_servers_path());
    Ok(store.items.iter().map(RecentServerDto::from).collect())
}

/// Scans the local network (UDP beacons + localhost HTTP probe) for AC dedicated servers.
#[tauri::command]
pub async fn scan_lan_servers() -> Result<Vec<DiscoveredServerDto>, String> {
    let found = tauri::async_runtime::spawn_blocking(|| {
        lan_discovery::scan_lan(Duration::from_millis(800))
    })
    .await
    .map_err(|e| format!("LAN scan worker failed: {e}"))?;

    Ok(found
        .into_iter()
        .map(|s| DiscoveredServerDto {
            host: s.host,
            http_port: s.http_port,
            race_port: s.race_port,
            name: s.name,
            latency_ms: s.latency_ms.map(|ms| ms as u64),
            track: s.status.as_ref().and_then(|st| st.track.clone()),
            clients: s.status.as_ref().and_then(|st| st.clients),
            max_clients: s.status.as_ref().and_then(|st| st.max_clients),
        })
        .collect())
}

/// Parses an `acmanager://` or `acxs://` server join URI.
#[tauri::command]
pub fn parse_join_uri(raw: String) -> Result<JoinUriDto, String> {
    let parsed = uri::parse_join_uri(&raw).map_err(|e| e.to_string())?;
    Ok(JoinUriDto {
        host: parsed.host,
        http_port: parsed.http_port,
        password: parsed.password,
        car: parsed.car,
        skin: parsed.skin,
        min_csp_build: parsed.min_csp_build,
    })
}

/// Formats a Content Manager compatible `acmanager://` invite link.
#[tauri::command]
pub fn format_invite_uri(
    host: String,
    port: u16,
    password: Option<String>,
) -> Result<String, String> {
    let trimmed = host.trim();
    if !uri::is_valid_host(trimmed) {
        return Err(format!("Invalid host: {trimmed:?}"));
    }
    if port == 0 {
        return Err("HTTP port must not be 0.".to_string());
    }
    Ok(uri::format_acmanager_uri(trimmed, port, password.as_deref()))
}

/// Writes an online `race.ini`, records the server in `recent-servers.json`, and launches `acs.exe`.
#[tauri::command]
pub fn join_online_server(
    state: State<'_, AppState>,
    req: OnlineJoinRequest,
) -> Result<JoinResultDto, String> {
    if game::is_game_running() {
        return Err("Assetto Corsa is already running. Close it first.".to_string());
    }
    let ac_root = resolve_ac_root(&state)?;
    let cfg_dir = settings::ac_user_cfg_dir()
        .ok_or_else(|| "Could not locate Documents/Assetto Corsa/cfg.".to_string())?;

    let steam_id = req
        .steam_id
        .filter(|s| !s.trim().is_empty())
        .or_else(|| lobby::current_steam_id64(&steam::default_steam_roots()))
        .unwrap_or_default();

    let join = OnlineJoin {
        ip: req.ip.clone(),
        port: req.port,
        http_port: req.http_port,
        server_name: req.server_name.clone(),
        track: req.track.clone(),
        track_layout: req.track_layout.clone().filter(|s| !s.trim().is_empty()),
        car: req.car.clone(),
        skin: req.skin.clone(),
        driver: if req.driver.trim().is_empty() {
            "Driver".to_string()
        } else {
            req.driver.clone()
        },
        password: req.password.clone(),
        steam_id,
        features: req.features.clone(),
    };

    let ini_path = race::write_online_ini(&cfg_dir, &join)
        .map_err(|e| format!("Failed to write online race.ini: {e}"))?;

    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let recent_path = recent_servers_path();
    let mut recent = RecentServers::load(&recent_path);
    recent.record(RecentServer {
        host: req.ip,
        port: req.http_port,
        name: req.server_name,
        track: req.track,
        last_car: req.car,
        last_skin: req.skin,
        last_joined_unix: now_unix,
        join_count: 1,
    });
    let _ = recent.save(&recent_path);

    let child =
        game::launch_game(&ac_root).map_err(|e| format!("Failed to launch Assetto Corsa: {e}"))?;
    Ok(JoinResultDto {
        pid: child.id(),
        race_ini_path: ini_path.to_string_lossy().into_owned(),
    })
}

/// Sends an HTTP `/SUB` booking registration request to an online server.
#[tauri::command]
pub async fn book_online_server(
    _state: State<'_, AppState>,
    req: OnlineBookRequest,
) -> Result<BookingReplyDto, String> {
    let steam_guid = req
        .steam_guid
        .filter(|s| !s.trim().is_empty())
        .or_else(|| lobby::current_steam_id64(&steam::default_steam_roots()))
        .unwrap_or_default();

    let host = req.host.clone();
    let http_port = req.http_port;
    let booking_req = BookingRequest {
        driver_name: req.driver_name,
        steam_guid,
        car_model: req.car_model,
        car_skin: req.car_skin,
        password: req.password,
    };

    let reply = tauri::async_runtime::spawn_blocking(move || {
        server_status::book(&host, http_port, &booking_req, Duration::from_secs(6))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Booking worker failed: {e}"))??;

    Ok(BookingReplyDto {
        accepted: reply.accepted,
        message: reply.message,
        booking_time_sec: reply.booking_time_sec,
    })
}

/// Downloads and installs missing server content from a [`ContentDownloadPlan`].
#[tauri::command]
pub async fn download_missing_content(
    state: State<'_, AppState>,
    plan: ContentDownloadPlan,
) -> Result<Vec<BatchInstallReportDto>, String> {
    let (ac_root, seven_zip_path) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        let root = s
            .ac_root_path()
            .or_else(settings::detect_ac_root)
            .ok_or_else(|| "Assetto Corsa folder is not configured.".to_string())?;
        (root, s.seven_zip_path.clone())
    };

    let reports = tauri::async_runtime::spawn_blocking(move || {
        let cancel = AtomicBool::new(false);
        content_dl::download_and_install_plan(
            &ac_root,
            &plan,
            game::is_game_running(),
            seven_zip_path.as_deref(),
            &cancel,
            |_| {},
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Content download worker failed: {e}"))??;

    Ok(reports
        .into_iter()
        .map(|batch| BatchInstallReportDto {
            installed: batch
                .installed
                .into_iter()
                .map(|item| InstalledItemReportDto {
                    kind: format!("{:?}", item.kind),
                    id: item.id,
                    installed_to: item.installed_to.to_string_lossy().into_owned(),
                    file_count: item.file_count,
                    moved_existing_to: item
                        .moved_existing_to
                        .map(|p| p.to_string_lossy().into_owned()),
                })
                .collect(),
        })
        .collect())
}

/// Loads `server_cfg.ini` and `entry_list.ini` from a dedicated server cfg directory.
#[tauri::command]
pub fn load_dedicated_server(dir: String) -> Result<DedicatedServerBundleDto, String> {
    let cfg_dir = PathBuf::from(dir.trim());
    let cfg = match server::read_server_config(&cfg_dir) {
        Ok(cfg) => cfg,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ServerConfig::default(),
        Err(e) => return Err(format!("Failed to read server_cfg.ini: {e}")),
    };
    let warnings = server::validate(&cfg);
    let entries = read_entry_list(&cfg_dir)
        .iter()
        .map(ServerEntryDto::from)
        .collect();
    Ok(DedicatedServerBundleDto {
        cfg: ServerConfigDto::from(&cfg),
        entries,
        warnings,
    })
}

/// Saves `server_cfg.ini` and `entry_list.ini` (with rolling backups) and returns validation warnings.
#[tauri::command]
pub fn save_dedicated_server(
    dir: String,
    cfg: ServerConfigDto,
    entries: Vec<ServerEntryDto>,
) -> Result<Vec<String>, String> {
    let cfg_dir = PathBuf::from(dir.trim());
    fs::create_dir_all(&cfg_dir).map_err(|e| format!("Failed to create server cfg dir: {e}"))?;
    let domain_cfg = ServerConfig::from(&cfg);
    server::write_server_config(&cfg_dir, &domain_cfg)
        .map_err(|e| format!("Failed to write server_cfg.ini: {e}"))?;
    if !entries.is_empty() {
        let domain_entries: Vec<Entry> = entries.iter().map(Entry::from).collect();
        server::write_entry_list(&cfg_dir, &domain_entries)
            .map_err(|e| format!("Failed to write entry_list.ini: {e}"))?;
    }
    Ok(server::validate(&domain_cfg))
}

/// Starts `acServer` using the configuration in `dir`.
#[tauri::command]
pub fn start_dedicated_server(
    state: State<'_, AppState>,
    dir: String,
) -> Result<DedicatedServerStatusDto, String> {
    let cfg_dir = PathBuf::from(dir.trim());
    let exe = resolve_ac_root(&state)
        .and_then(|root| server_process::find_server_executable(&root).map_err(|e| e.to_string()))
        .or_else(|_| {
            let bin = server_process::server_binary_name();
            let beside = cfg_dir.parent().map(|p| p.join(bin));
            if let Some(p) = beside
                && p.is_file()
            {
                return Ok(p);
            }
            let inside = cfg_dir.join(bin);
            if inside.is_file() {
                return Ok(inside);
            }
            Err(format!("Server executable ({bin}) not found."))
        })?;

    let mut guard = state
        .server_process
        .lock()
        .map_err(|_| "server_process lock poisoned".to_string())?;
    let proc = guard.get_or_insert_with(ServerProcess::new);
    proc.start(&exe, &cfg_dir, &LaunchArgs::default())
        .map_err(|e| format!("Failed to start dedicated server: {e}"))?;
    Ok(process_status_dto(proc))
}

/// Stops the running dedicated server child process.
#[tauri::command]
pub fn stop_dedicated_server(
    state: State<'_, AppState>,
) -> Result<DedicatedServerStatusDto, String> {
    let mut guard = state
        .server_process
        .lock()
        .map_err(|_| "server_process lock poisoned".to_string())?;
    let Some(proc) = guard.as_mut() else {
        return Ok(DedicatedServerStatusDto {
            status: "stopped".to_string(),
            exit_code: None,
            recent_lines: Vec::new(),
        });
    };
    proc.stop()
        .map_err(|e| format!("Failed to stop dedicated server: {e}"))?;
    Ok(process_status_dto(proc))
}

/// Returns the current dedicated server process status and recent log lines.
#[tauri::command]
pub fn get_dedicated_server_status(
    state: State<'_, AppState>,
) -> Result<DedicatedServerStatusDto, String> {
    let mut guard = state
        .server_process
        .lock()
        .map_err(|_| "server_process lock poisoned".to_string())?;
    let Some(proc) = guard.as_mut() else {
        return Ok(DedicatedServerStatusDto {
            status: "stopped".to_string(),
            exit_code: None,
            recent_lines: Vec::new(),
        });
    };
    Ok(process_status_dto(proc))
}

/// Loads `csp_extra_options.ini` and optional `extra_cfg.yml` from a dedicated server cfg directory.
#[tauri::command]
pub fn load_server_csp(dir: String) -> Result<ServerCspBundleDto, String> {
    let cfg_dir = PathBuf::from(dir.trim());
    let csp_extra_options = match server_csp::read_csp_extra_options(&cfg_dir) {
        Ok(opts) => CspExtraOptionsDto::from(&opts),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CspExtraOptionsDto::default(),
        Err(e) => return Err(format!("Failed to read csp_extra_options.ini: {e}")),
    };
    let extra_cfg_yml = match server_csp::read_extra_cfg_yml(&cfg_dir) {
        Ok(yml) => Some(ExtraCfgYmlDto::from(&yml)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Failed to read extra_cfg.yml: {e}")),
    };
    Ok(ServerCspBundleDto {
        csp_extra_options,
        extra_cfg_yml,
    })
}

/// Saves `csp_extra_options.ini` and optional `extra_cfg.yml` to a dedicated server cfg directory.
#[tauri::command]
pub fn save_server_csp(dir: String, csp: ServerCspBundleDto) -> Result<(), String> {
    let cfg_dir = PathBuf::from(dir.trim());
    fs::create_dir_all(&cfg_dir).map_err(|e| format!("Failed to create server cfg dir: {e}"))?;
    let opts = CspExtraOptions::from(&csp.csp_extra_options);
    server_csp::write_csp_extra_options(&cfg_dir, &opts)
        .map_err(|e| format!("Failed to write csp_extra_options.ini: {e}"))?;
    if let Some(yml) = &csp.extra_cfg_yml {
        let domain_yml = ExtraCfgYml::from(yml);
        server_csp::write_extra_cfg_yml(&cfg_dir, &domain_yml)
            .map_err(|e| format!("Failed to write extra_cfg.yml: {e}"))?;
    }
    Ok(())
}

/// Connects a UDP plugin RCON client to a dedicated server.
#[tauri::command]
pub fn rcon_connect(
    state: State<'_, AppState>,
    host: String,
    port: u16,
    local_port: Option<u16>,
) -> Result<RconConnectResultDto, String> {
    let server_addr = (host.trim(), port)
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve {host}:{port}: {e}"))?
        .next()
        .ok_or_else(|| format!("Host {host} resolved to no addresses"))?;

    let local_bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), local_port.unwrap_or(0));
    let client = RconClient::bind(local_bind, server_addr, Duration::from_millis(400))
        .map_err(|e| format!("Failed to bind RCON UDP socket: {e}"))?;
    let local_addr = client
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| local_bind.to_string());

    let mut guard = state
        .rcon_client
        .lock()
        .map_err(|_| "rcon_client lock poisoned".to_string())?;
    *guard = Some(client);

    Ok(RconConnectResultDto {
        local_addr,
        server_addr: server_addr.to_string(),
    })
}

/// Sends an admin command over the active RCON UDP client and returns any immediate event reply.
#[tauri::command]
pub fn rcon_send_command(
    state: State<'_, AppState>,
    cmd: RconCommandInput,
) -> Result<RconReplyDto, String> {
    let admin_cmd = match cmd {
        RconCommandInput::Raw(text) => AdminCmd::AdminCommand(text),
        RconCommandInput::Structured(structured) => match structured {
            RconCommandDto::SetRealtimePosInterval { interval_ms } => {
                AdminCmd::SetRealtimePosInterval(interval_ms)
            }
            RconCommandDto::GetCarInfo { car_id } => AdminCmd::GetCarInfo(car_id),
            RconCommandDto::SendChat { car_id, message } => AdminCmd::SendChat { car_id, message },
            RconCommandDto::BroadcastChat { message } => AdminCmd::BroadcastChat(message),
            RconCommandDto::GetSessionInfo { session_index } => {
                AdminCmd::GetSessionInfo(session_index)
            }
            RconCommandDto::KickUser { car_id } => AdminCmd::KickUser(car_id),
            RconCommandDto::NextSession => AdminCmd::NextSession,
            RconCommandDto::RestartSession => AdminCmd::RestartSession,
            RconCommandDto::AdminCommand { command } => AdminCmd::AdminCommand(command),
        },
    };

    let guard = state
        .rcon_client
        .lock()
        .map_err(|_| "rcon_client lock poisoned".to_string())?;
    let client = guard
        .as_ref()
        .ok_or_else(|| "RCON client is not connected. Call rcon_connect first.".to_string())?;

    client
        .send_cmd(&admin_cmd)
        .map_err(|e| format!("Failed to send RCON command: {e}"))?;

    let event = match client.recv_event() {
        Ok(ev) => Some(format_server_event(&ev)),
        Err(RconError::Timeout) => None,
        Err(e) => return Err(format!("Failed to receive RCON reply: {e}")),
    };

    Ok(RconReplyDto { sent: true, event })
}

fn format_server_event(ev: &ServerEvent) -> String {
    match ev {
        ServerEvent::Version(v) => format!("Protocol v{v}"),
        ServerEvent::Chat { car_id, message } => format!("[car {car_id}] {message}"),
        ServerEvent::Error(msg) => format!("Error: {msg}"),
        ServerEvent::NewSession(s) | ServerEvent::SessionInfo(s) => {
            format!("{} ({}) on {}", s.name, s.server_name, s.track)
        }
        ServerEvent::NewConnection(c) => {
            format!("Connected: {} ({})", c.driver_name, c.car_model)
        }
        ServerEvent::ConnectionClosed(c) => format!("Disconnected: {}", c.driver_name),
        ServerEvent::LapCompleted {
            car_id,
            lap_time_ms,
            cuts,
            ..
        } => format!("Car {car_id} lap {lap_time_ms}ms (cuts: {cuts})"),
        other => format!("{other:?}"),
    }
}
