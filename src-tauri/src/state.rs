use std::collections::HashMap;
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

/// Shared application state managed by Tauri across all IPC commands.
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub cars: Mutex<Vec<CarEntry>>,
    pub tracks: Mutex<Vec<TrackEntry>>,
    pub lobby_servers: Mutex<Vec<LobbyServer>>,
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
        Self {
            settings: Mutex::new(loaded_settings),
            cars: Mutex::new(Vec::new()),
            tracks: Mutex::new(Vec::new()),
            lobby_servers: Mutex::new(Vec::new()),
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
