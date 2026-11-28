use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use acxs_manager::content::{CarEntry, TrackEntry};
use acxs_manager::lobby::LobbyServer;
use acxs_manager::server_process::ServerProcess;
use acxs_manager::server_rcon::RconClient;
use acxs_manager::server_status::ServerStatus;
use acxs_manager::session_recorder::{RecorderControl, SessionAggregator};
use acxs_manager::settings::{self, Settings};
use acxs_manager::udp_client::TelemetryClient;
use acxs_manager::web_dashboard::Dashboard;

/// Path to the persisted public directory cache (`directory-cache.json` beside `settings.json`).
pub fn directory_cache_path() -> PathBuf {
    settings::settings_path().with_file_name("directory-cache.json")
}

/// Loads previously cached public directory servers and save timestamp from disk.
pub fn load_directory_cache() -> (Vec<LobbyServer>, Option<u64>) {
    let Ok(text) = std::fs::read_to_string(directory_cache_path()) else {
        return (Vec::new(), None);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return (Vec::new(), None);
    };
    let saved = value.get("saved").and_then(|v| v.as_u64());
    let servers = value
        .get("servers")
        .cloned()
        .and_then(|v| serde_json::from_value::<Vec<LobbyServer>>(v).ok())
        .unwrap_or_default();
    if servers.is_empty() {
        (Vec::new(), None)
    } else {
        (servers, saved)
    }
}

/// Persists `directory-cache.json` off the caller thread.
pub fn save_directory_cache_async(servers: Vec<LobbyServer>, saved_unix: u64) {
    std::thread::spawn(move || {
        let path = directory_cache_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) =
            serde_json::to_string(&serde_json::json!({ "saved": saved_unix, "servers": servers }))
        {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, &text).is_ok() {
                if std::fs::rename(&tmp, &path).is_err() {
                    let _ = std::fs::write(&path, text);
                }
            } else {
                let _ = std::fs::write(&path, text);
            }
        }
    });
}

/// Shared application state managed by Tauri across all IPC commands.
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub cars: Mutex<Vec<CarEntry>>,
    pub tracks: Mutex<Vec<TrackEntry>>,
    pub lobby_servers: Mutex<Vec<LobbyServer>>,
    pub lobby_cache_saved_unix: Mutex<Option<u64>>,
    pub directory_refresh_seq: Arc<AtomicU64>,
    pub directory_search_seq: Arc<AtomicU64>,
    pub server_statuses: Mutex<HashMap<String, ServerStatus>>,
    pub server_process: Mutex<Option<ServerProcess>>,
    pub rcon_client: Mutex<Option<RconClient>>,
    pub dashboard: Mutex<Option<Dashboard>>,
    pub udp_client: Mutex<Option<TelemetryClient>>,
    pub session_recorder: Mutex<SessionAggregator>,
    pub recorder_control: Arc<RecorderControl>,
}

impl AppState {
    pub fn new() -> Self {
        let loaded_settings = Settings::load(&settings::settings_path());
        let (cached_servers, cached_saved_unix) = load_directory_cache();
        Self {
            settings: Mutex::new(loaded_settings),
            cars: Mutex::new(Vec::new()),
            tracks: Mutex::new(Vec::new()),
            lobby_servers: Mutex::new(cached_servers),
            lobby_cache_saved_unix: Mutex::new(cached_saved_unix),
            directory_refresh_seq: Arc::new(AtomicU64::new(0)),
            directory_search_seq: Arc::new(AtomicU64::new(0)),
            server_statuses: Mutex::new(HashMap::new()),
            server_process: Mutex::new(None),
            rcon_client: Mutex::new(None),
            dashboard: Mutex::new(None),
            udp_client: Mutex::new(None),
            session_recorder: Mutex::new(SessionAggregator::default()),
            recorder_control: Arc::new(RecorderControl::default()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
