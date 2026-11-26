//! Servers: a watch list of Assetto Corsa servers with live status.
//!
//! Each server is polled over its HTTP port (`/INFO` and the entry list, see
//! `acxs_manager::server_status`). The game's own lobby is not queried, so the list is
//! yours: add the servers you race on and see who is on them, on which track, which cars
//! are free, and whether you have that content installed.
//!
//! The list is a compact, virtualised table so a few thousand rows scroll smoothly. The
//! panel on the right shows the selected server: a car chooser built from its entry list,
//! the drivers, and the raw server replies in collapsible sections.

mod detail;

use super::theme::{self, ACCENT, DANGER, FAINT, INFO, MUTED, OK, TEXT, WARN};
use super::{media, widgets, App, Msg, Notice};
use acxs_manager::content::{CarEntry, TrackEntry};
use acxs_manager::content_dl;
use acxs_manager::directory::{self, RecentServer, RecentServers};
use acxs_manager::lan_discovery::{self, DiscoveredServer};
use acxs_manager::lobby::{self, LobbyServer};
use acxs_manager::server_status::{self, BookingRequest, ServerCar, ServerStatus};
use acxs_manager::uri;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_HTTP_PORT: u16 = 8081;
/// A server is asked again after this long.
const STALE_AFTER: Duration = Duration::from_secs(20);
const TIMEOUT: Duration = Duration::from_secs(4);
const ROW_HEIGHT: f32 = 34.0;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub favorite: bool,
}

impl Entry {
    pub fn key(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Clone, Default)]
pub struct Live {
    pub status: Option<ServerStatus>,
    pub latency_ms: Option<u128>,
    pub error: Option<String>,
    pub checking: bool,
    pub checked: Option<Instant>,
    /// True once the server itself answered (or failed), as opposed to the lobby's summary.
    pub detailed: bool,
}

#[derive(Default)]
enum LobbyState {
    #[default]
    Idle,
    Loading,
    /// A background refresh is running; the list on screen stays usable.
    Updating { got: usize, of: usize },
    Failed(String),
    Loaded(Instant),
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ServerTab {
    #[default]
    Watchlist,
    Online,
    Recent,
    Lan,
}

struct SlotWatch {
    entry: Entry,
    model: String,
    skin: Option<String>,
    last_poll: Instant,
}

struct DownloadUi {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<String>>,
    done: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Servers {
    pub entries: Vec<Entry>,
    pub live: HashMap<String, Live>,
    /// Active sub-tab: Watchlist | Online | Recent | LAN.
    tab: ServerTab,
    lobby: HashMap<String, LobbyServer>,
    lobby_state: LobbyState,
    /// Servers seen by the refresh in progress.
    refresh_seen: std::collections::HashSet<String>,
    /// The list on screen came from the file of the last session and is not refreshed yet.
    from_cache: bool,
    /// A refresh failed but the older list is still shown.
    refresh_note: Option<String>,
    lobby_host: String,
    /// 0 the Kunos lobby, 1 a custom list address.
    lobby_source: usize,
    custom_url: String,
    /// Text for searching the whole directory.
    dir_query: String,
    hide_full: bool,
    hide_empty: bool,
    hide_password: bool,
    joinable_only: bool,
    selected: Option<String>,
    add_input: String,
    filter: String,
    online_only: bool,
    /// Name to race under, and the password for password-protected servers.
    driver: String,
    password: String,
    /// Column the table is sorted by: 0 name, 1 track, 2 players, 3 ping.
    sort_col: usize,
    sort_desc: bool,
    /// Car model and skin chosen in the detail panel.
    pick_model: Option<String>,
    pick_skin: Option<String>,
    /// Car search text and the "mine only" switch in the join panel.
    car_query: String,
    installed_only: bool,
    /// The car and skin last chosen on each server, by `host:port`.
    picks: HashMap<String, (String, String)>,
    /// Recently joined servers persisted in `recent-servers.json`.
    recent: RecentServers,
    /// Discovered LAN servers and background scan receiver.
    lan_servers: Vec<DiscoveredServer>,
    lan_scanning: bool,
    lan_rx: Option<Receiver<Vec<DiscoveredServer>>>,
    /// Active Slot-Watch auto-join subscription.
    slot_watch: Option<SlotWatch>,
    pending_auto_join: Option<JoinRequest>,
    /// Active missing-content download state.
    download_ui: Option<DownloadUi>,
    /// Last `/SUB` booking status per server key.
    booking_status: HashMap<String, String>,
}

fn recent_path() -> PathBuf {
    file_path().with_file_name("recent-servers.json")
}

fn file_path() -> PathBuf {
    let settings_file = acxs_manager::settings::settings_path();
    settings_file.parent().unwrap_or(std::path::Path::new(".")).join("servers.json")
}

impl Servers {
    pub fn load() -> Self {
        let entries = std::fs::read_to_string(file_path())
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Entry>>(&text).ok())
            .unwrap_or_default();
        let saved: serde_json::Value = std::fs::read_to_string(Self::source_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        // A first-time user has no servers of their own: open on the public list.
        let no_own_servers = entries.is_empty();
        let recent = RecentServers::load(&recent_path());
        let mut servers = Servers {
            entries,
            sort_col: 2,
            sort_desc: true,
            lobby_host: lobby::DEFAULT_HOST.to_string(),
            lobby_source: saved["source"].as_u64().unwrap_or(0).min(2) as usize,
            custom_url: saved["url"].as_str().unwrap_or("").to_string(),
            hide_empty: true,
            installed_only: true,
            tab: if no_own_servers { ServerTab::Online } else { ServerTab::Watchlist },
            driver: previous_driver_name(),
            recent,
            ..Servers::default()
        };
        // The list of the last session is there at once; a refresh then updates it in place.
        if servers.lobby_source == 0
            && let Some((list, saved)) = load_cache()
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(saved);
            let age = Duration::from_secs(now.saturating_sub(saved));
            for server in list {
                servers.upsert_lobby(server);
            }
            servers.lobby_state = LobbyState::Loaded(Instant::now().checked_sub(age).unwrap_or_else(Instant::now));
            servers.from_cache = true;
        }
        servers
    }

    /// Servers whose name, track or address match `query`: (key, name, hint). Used by the
    /// command palette so public servers can be found from anywhere.
    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, String, String)> {
        let q = query.trim().to_lowercase();
        if q.len() < 2 {
            return Vec::new();
        }
        let mut hits: Vec<(String, String, String)> = Vec::new();
        for (key, s) in &self.lobby {
            if s.name.to_lowercase().contains(&q) || s.track.to_lowercase().contains(&q) || key.contains(&q) {
                hits.push((key.clone(), s.name.clone(), format!("{}/{} players", s.clients, s.capacity)));
            }
        }
        for entry in &self.entries {
            let key = entry.key();
            if hits.iter().any(|(k, _, _)| *k == key) {
                continue;
            }
            let name = self.live.get(&key).and_then(|l| l.status.as_ref()).and_then(|s| s.name.clone());
            if name.as_deref().is_some_and(|n| n.to_lowercase().contains(&q)) || key.contains(&q) {
                hits.push((key.clone(), name.unwrap_or_else(|| key.clone()), "my server".to_string()));
            }
        }
        // Fullest first, so busy servers lead.
        hits.sort_by_key(|(key, _, _)| std::cmp::Reverse(self.lobby.get(key).map_or(0, |s| s.clients)));
        hits.truncate(limit);
        hits
    }

    /// Show `key` on the Servers page, in the list it belongs to.
    pub fn show(&mut self, key: String) {
        self.tab = if self.lobby.contains_key(&key) && !self.entries.iter().any(|e| e.key() == key) {
            ServerTab::Online
        } else {
            ServerTab::Watchlist
        };
        self.select(key);
    }

    /// The entry for `key`: from the watch list, lobby list, recent list, or LAN scan.
    fn entry_for(&self, key: &str) -> Option<Entry> {
        self.entries
            .iter()
            .find(|e| e.key() == key)
            .cloned()
            .or_else(|| {
                self.lobby
                    .get(key)
                    .map(|s| Entry { host: s.ip.clone(), port: s.http_port, favorite: false })
            })
            .or_else(|| {
                self.recent
                    .items
                    .iter()
                    .find(|r| r.key() == key)
                    .map(|r| Entry { host: r.host.clone(), port: r.port, favorite: false })
            })
            .or_else(|| {
                self.lan_servers
                    .iter()
                    .find(|s| format!("{}:{}", s.host, s.http_port) == key)
                    .map(|s| Entry { host: s.host.clone(), port: s.http_port, favorite: false })
            })
    }

    fn start_lan_scan(&mut self, ctx: &egui::Context) {
        if self.lan_scanning {
            return;
        }
        self.lan_scanning = true;
        let (tx, rx) = mpsc::channel();
        self.lan_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let found = lan_discovery::scan_lan(Duration::from_millis(1200));
            let _ = tx.send(found);
            ctx.request_repaint();
        });
    }

    fn poll_lan_scan(&mut self) {
        if let Some(rx) = &self.lan_rx
            && let Ok(found) = rx.try_recv()
        {
            self.lan_scanning = false;
            for srv in &found {
                let key = format!("{}:{}", srv.host, srv.http_port);
                if let Some(status) = &srv.status {
                    let live = self.live.entry(key).or_default();
                    live.status = Some(status.clone());
                    live.latency_ms = srv.latency_ms;
                    live.detailed = true;
                    live.checked = Some(Instant::now());
                }
            }
            self.lan_servers = found;
            self.lan_rx = None;
        }
    }

    /// Download the public server list. The Steam id is read from the Steam client on this PC
    /// every time, so it always belongs to whoever is signed in now.
    fn load_lobby(&mut self, tx: &Sender<Msg>, ctx: &egui::Context) {
        if matches!(self.lobby_state, LobbyState::Loading | LobbyState::Updating { .. }) {
            return;
        }
        self.save_source();
        // The directory refreshes in the background and keeps the list usable meanwhile.
        if self.lobby_source == 0 {
            self.start_refresh(tx, ctx);
            return;
        }
        self.lobby_state = LobbyState::Loading;
        let (tx, ctx, host) = (tx.clone(), ctx.clone(), self.lobby_host.trim().to_string());
        let source = self.lobby_source;
        let url = self.custom_url.trim().to_string();
        std::thread::spawn(move || {
            let result = match source {
                // The community directory: every server that has players right now.
                0 => directory::fetch_all(None, true, 60, Duration::from_secs(20), |_| {}),
                2 => lobby::fetch_url(&url, Duration::from_secs(45)),
                _ => match lobby::current_steam_id64(&acxs_manager::steam::default_steam_roots()) {
                    Some(id) => lobby::fetch(&host, &id, Duration::from_secs(45)),
                    None => Err("Steam is not signed in on this PC, so there is no Steam id to ask the lobby with.".into()),
                },
            };
            let _ = tx.send(Msg::Lobby(result));
            ctx.request_repaint();
        });
    }

    /// Search every server the directory lists (not only those with players) by name.
    fn search_directory(&mut self, tx: &Sender<Msg>, ctx: &egui::Context) {
        if matches!(self.lobby_state, LobbyState::Loading | LobbyState::Updating { .. }) || self.dir_query.trim().len() < 2 {
            return;
        }
        self.lobby_state = LobbyState::Loading;
        let (tx, ctx, query) = (tx.clone(), ctx.clone(), self.dir_query.trim().to_string());
        std::thread::spawn(move || {
            let result = directory::fetch_all(Some(&query), false, 10, Duration::from_secs(20), |_| {});
            let _ = tx.send(Msg::Lobby(result));
            ctx.request_repaint();
        });
    }

    fn source_path() -> PathBuf {
        file_path().with_file_name("lobby-source.json")
    }

    /// Remember which list source was used and the custom address.
    fn save_source(&self) {
        let value = serde_json::json!({ "source": self.lobby_source, "url": self.custom_url });
        let _ = std::fs::write(Self::source_path(), value.to_string());
    }

    /// Put one lobby entry in the list: its summary fills the table at once, and the server's own
    /// answer replaces the summary when the server is opened.
    fn upsert_lobby(&mut self, server: LobbyServer) {
        let key = server.key();
        let live = self.live.entry(key.clone()).or_default();
        if !live.detailed {
            live.status = Some(ServerStatus {
                online: true,
                name: Some(server.name.clone()),
                // Directory lists name the track for people instead of giving its folder.
                track: (!server.track.is_empty()).then(|| server.track.clone()),
                clients: Some(server.clients),
                max_clients: Some(server.capacity),
                session: (!server.track.is_empty()).then(|| server.session.to_string()),
                ..ServerStatus::default()
            });
            live.checked = Some(Instant::now());
        }
        self.lobby.insert(key, server);
    }

    /// A finished download that replaces the whole list (a search, a custom list, the Kunos lobby).
    pub fn apply_lobby(&mut self, result: Result<Vec<LobbyServer>, String>) {
        match result {
            // A failed refresh keeps what is on screen.
            Err(e) if !self.lobby.is_empty() => {
                self.refresh_note = Some(e);
                self.lobby_state = LobbyState::Loaded(Instant::now());
            }
            Err(e) => self.lobby_state = LobbyState::Failed(e),
            Ok(list) => {
                self.lobby_state = LobbyState::Loaded(Instant::now());
                self.refresh_note = None;
                self.lobby.clear();
                for server in list {
                    self.upsert_lobby(server);
                }
            }
        }
    }

    /// One page of the background refresh. Servers in the page are updated in place, new ones
    /// appear, and when the last page is in, servers that did not come back are dropped. The list
    /// stays usable the whole time.
    pub fn apply_part(&mut self, servers: Vec<LobbyServer>, total: u64, last: bool) {
        if let LobbyState::Updating { got, of } = &mut self.lobby_state {
            *got += servers.len();
            *of = total as usize;
        }
        for server in servers {
            self.refresh_seen.insert(server.key());
            self.upsert_lobby(server);
        }
        if last {
            let seen = std::mem::take(&mut self.refresh_seen);
            self.lobby.retain(|key, _| seen.contains(key));
            self.lobby_state = LobbyState::Loaded(Instant::now());
            self.from_cache = false;
            self.refresh_note = None;
            save_cache(self.lobby.values().cloned().collect());
        }
    }

    /// Start (or restart) the background refresh of the directory's populated servers.
    pub fn start_refresh(&mut self, tx: &Sender<Msg>, ctx: &egui::Context) {
        if matches!(self.lobby_state, LobbyState::Loading | LobbyState::Updating { .. }) || self.lobby_source != 0 {
            return;
        }
        self.lobby_state = LobbyState::Updating { got: 0, of: 0 };
        self.refresh_seen.clear();
        let (tx, ctx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let sender = tx.clone();
            let repaint = ctx.clone();
            let result = directory::fetch_stream(None, true, 60, Duration::from_secs(20), move |servers, total| {
                let _ = sender.send(Msg::LobbyPart { servers, total, last: false });
                repaint.request_repaint();
            });
            let _ = match result {
                Ok(()) => tx.send(Msg::LobbyPart { servers: Vec::new(), total: 0, last: true }),
                Err(e) => tx.send(Msg::Lobby(Err(e))),
            };
            ctx.request_repaint();
        });
    }

    /// Ask one server for its full status now.
    fn refresh_one(&mut self, entry: &Entry, tx: &Sender<Msg>, ctx: &egui::Context) {
        let key = entry.key();
        let live = self.live.entry(key.clone()).or_default();
        if live.checking {
            return;
        }
        live.checking = true;
        let (tx, ctx, host, port) = (tx.clone(), ctx.clone(), entry.host.clone(), entry.port);
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = server_status::fetch(&host, port, TIMEOUT)
                .map(|status| (status, started.elapsed().as_millis()))
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Server(key, result));
            ctx.request_repaint();
        });
    }

    fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(&self.entries) {
            let _ = std::fs::write(file_path(), text);
        }
    }

    pub fn select(&mut self, key: String) {
        if self.selected.as_deref() != Some(key.as_str()) {
            // Come back to the car chosen on this server last time.
            match self.picks.get(&key) {
                Some((model, skin)) => {
                    self.pick_model = Some(model.clone());
                    self.pick_skin = (!skin.is_empty()).then(|| skin.clone());
                }
                None => {
                    self.pick_model = None;
                    self.pick_skin = None;
                }
            }
        }
        self.selected = Some(key);
    }

    /// Ask every server that has no fresh answer (or all of them with `force`).
    /// At most a few dozen requests run at once so a long list does not flood the network.
    fn refresh(&mut self, tx: &Sender<Msg>, ctx: &egui::Context, force: bool) {
        const MAX_IN_FLIGHT: usize = 24;
        // Reserve 1 polling slot for active Slot-Watch auto-join so it never starves.
        let budget = if self.slot_watch.is_some() { MAX_IN_FLIGHT - 1 } else { MAX_IN_FLIGHT };
        let mut in_flight = self.live.values().filter(|l| l.checking).count();
        for entry in &self.entries {
            if in_flight >= budget {
                break;
            }
            let key = entry.key();
            let live = self.live.entry(key.clone()).or_default();
            let stale = live.checked.is_none_or(|at| at.elapsed() > STALE_AFTER);
            if live.checking || !(force || stale) {
                continue;
            }
            live.checking = true;
            in_flight += 1;
            let (tx, ctx, host, port) = (tx.clone(), ctx.clone(), entry.host.clone(), entry.port);
            std::thread::spawn(move || {
                let started = Instant::now();
                let result = server_status::fetch(&host, port, TIMEOUT)
                    .map(|status| (status, started.elapsed().as_millis()))
                    .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Server(key, result));
                ctx.request_repaint();
            });
        }
    }

    /// Record the answer of one poll.
    pub fn apply(&mut self, key: String, result: Result<(ServerStatus, u128), String>) {
        let live = self.live.entry(key.clone()).or_default();
        live.checking = false;
        live.detailed = true;
        live.checked = Some(Instant::now());
        match result {
            Ok((status, latency)) => {
                live.error = None;
                live.latency_ms = Some(latency);
                // Check if an active Slot-Watch just opened up.
                if let Some(watch) = &self.slot_watch
                    && watch.entry.key() == key
                    && status.cars.iter().any(|c| {
                        !c.connected
                            && c.model == watch.model
                            && watch.skin.as_deref().is_none_or(|s| s == c.skin)
                    })
                {
                    let watch = self.slot_watch.take().unwrap();
                    self.pending_auto_join = Some(JoinRequest {
                        entry: watch.entry,
                        model: Some(watch.model),
                        skin: watch.skin,
                    });
                }
                live.status = Some(status);
            }
            Err(error) => {
                live.error = Some(error);
                live.latency_ms = None;
                // A server the lobby listed keeps the lobby's summary when it does not
                // answer on its HTTP port; only a watch-list server turns offline.
                if !self.lobby.contains_key(&key) {
                    live.status = None;
                }
            }
        }
    }

    /// Add every address in `text`, separated by spaces, commas, semicolons or lines.
    /// Returns the last key added, how many were new, and the first problem found.
    fn add_many(&mut self, text: &str) -> (Option<String>, usize, Option<String>) {
        let (mut last, mut added, mut problem) = (None, 0, None);
        for part in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|p| !p.is_empty()) {
            match self.add(part) {
                Ok(key) => {
                    last = Some(key);
                    added += 1;
                }
                Err(e) => problem = problem.or(Some(format!("{part}: {e}"))),
            }
        }
        (last, added, problem)
    }

    fn add(&mut self, text: &str) -> Result<String, String> {
        let trimmed = text.trim();
        // Accept pasted acmanager:// or acxs:// invite links directly.
        if let Ok(parsed) = uri::parse_join_uri(trimmed) {
            let entry = Entry { host: parsed.host, port: parsed.http_port, favorite: false };
            let key = entry.key();
            if !self.entries.iter().any(|e| e.key() == key) {
                self.entries.push(entry);
                self.save();
            }
            if let Some(pw) = parsed.password {
                self.password = pw;
            }
            if let Some(car) = parsed.car {
                self.pick_model = Some(car);
            }
            if let Some(skin) = parsed.skin {
                self.pick_skin = Some(skin);
            }
            return Ok(key);
        }
        let text = trimmed.trim_start_matches("http://").trim_end_matches('/');
        if text.is_empty() || text.contains(char::is_whitespace) {
            return Err("Enter an address like 192.0.2.10:8081 or an acmanager:// link".into());
        }
        let (host, port) = match text.rsplit_once(':') {
            Some((host, port)) => (host, port.parse::<u16>().map_err(|_| "The port must be a number")?),
            None => (text, DEFAULT_HTTP_PORT),
        };
        let entry = Entry { host: host.to_string(), port, favorite: false };
        if self.entries.contains(&entry) {
            return Err("That server is already in the list".into());
        }
        let key = entry.key();
        self.entries.push(entry);
        self.save();
        Ok(key)
    }
}

/// Session codes as the server reports them.
fn session_name(code: &str) -> &str {
    match code {
        "0" => "Booking",
        "1" => "Practice",
        "2" => "Qualifying",
        "3" => "Race",
        other => other,
    }
}

/// What the user asked to join: a server, and optionally a car and skin.
struct JoinRequest {
    entry: Entry,
    model: Option<String>,
    skin: Option<String>,
}

/// The first free slot of `cars`, optionally restricted to one model: (model, skin).
fn first_free(cars: &[ServerCar], model: Option<&str>) -> Option<(String, String)> {
    cars.iter()
        .find(|c| !c.connected && model.is_none_or(|m| c.model == m))
        .map(|c| (c.model.clone(), c.skin.clone()))
}

/// Start the game on a server directly: write `race.ini` with the server in `[REMOTE]`, then run
/// `acs.exe`. Content Manager is not involved. Everything slow runs on a thread.
fn start_join(app: &mut App, sv: &Servers, request: JoinRequest, notices: &mut Vec<Notice>) {
    let fail = |notices: &mut Vec<Notice>, text: String| notices.push(Notice { text, ok: false });
    let Some(ac_root) = app.settings.ac_root_path() else {
        return fail(notices, "Set your Assetto Corsa folder in Settings first.".into());
    };
    let key = request.entry.key();
    let live_status = sv.live.get(&key).and_then(|l| l.status.as_ref());
    let listed = sv.lobby.get(&key);

    // Track: the server's own answer, else the lobby's. Layouts come after a dash.
    let raw_track = live_status
        .and_then(|s| s.track.clone())
        .or_else(|| listed.map(|s| s.track.clone()))
        .unwrap_or_default();
    let (found_track, track, layout) = resolve_track(&raw_track, &app.snapshot.tracks);
    if !track.is_empty() && found_track.is_none() {
        return fail(notices, format!("You do not have the track {track} installed."));
    }

    // Car: the one chosen, else the first free slot, else the first car the lobby lists.
    let cars: &[ServerCar] = live_status.map_or(&[], |s| s.cars.as_slice());
    let (model, skin) = match request.model.as_deref() {
        Some(model) => {
            let skin = request
                .skin
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| first_free(cars, Some(model)).map(|(_, skin)| skin))
                .unwrap_or_default();
            (model.to_string(), skin)
        }
        None => first_free(cars, None)
            .or_else(|| listed.and_then(|s| s.cars.first()).map(|m| (m.clone(), String::new())))
            .unwrap_or_default(),
    };
    if !model.is_empty() && !app.snapshot.cars.iter().any(|c| c.id == model) {
        return fail(notices, format!("You do not have the car {model} installed."));
    }

    let join = acxs_manager::race::OnlineJoin {
        ip: request.entry.host.clone(),
        port: live_status.and_then(|s| s.port).or_else(|| listed.map(|s| s.race_port)).unwrap_or(0),
        http_port: request.entry.port,
        server_name: live_status.and_then(|s| s.name.clone()).or_else(|| listed.map(|s| s.name.clone())).unwrap_or_else(|| key.clone()),
        track,
        track_layout: layout,
        car: model,
        skin,
        driver: if sv.driver.trim().is_empty() { "Driver".into() } else { sv.driver.trim().to_string() },
        password: sv.password.clone(),
        steam_id: String::new(),
        features: live_status.map(|s| s.features.clone()).unwrap_or_default(),
    };
    if let Err(problems) = join.validate() {
        return fail(notices, problems.join(" "));
    }

    let tx = app.tx.clone();
    let ctx = app.ctx.clone();
    let server_name = join.server_name.clone();
    std::thread::spawn(move || {
        let mut join = join;
        let notice = (|| -> Result<String, String> {
            if acxs_manager::game::is_game_running() {
                return Err("Assetto Corsa is already running. Close it before joining a server.".into());
            }
            // Whoever is signed in to Steam right now, as the game itself would send.
            join.steam_id = lobby::current_steam_id64(&acxs_manager::steam::default_steam_roots())
                .ok_or("Steam is not signed in. Start Steam, then join again.")?;
            // The panel may not have heard from the server yet; a server with features needs them in the file.
            if join.features.is_empty()
                && let Ok(status) = acxs_manager::server_status::fetch(&join.ip, join.http_port, std::time::Duration::from_secs(4))
            {
                join.features = status.features;
            }
            let cfg = acxs_manager::settings::ac_user_cfg_dir().ok_or("The Documents folder was not found.")?;
            acxs_manager::race::write_online_ini(&cfg, &join).map_err(|e| format!("Could not write race.ini: {e}"))?;
            acxs_manager::game::launch_game(&ac_root).map_err(|e| format!("Assetto Corsa did not start: {e}"))?;
            Ok(format!("Joining {server_name}…"))
        })();
        let notice = match notice {
            Ok(text) => Notice { text, ok: true },
            Err(text) => Notice { text, ok: false },
        };
        let _ = tx.send(Msg::Notice(notice));
        ctx.request_repaint();
    });
}

/// Cars on a server grouped by model: (model, total slots, free slots, skins with free count).
fn group_cars(cars: &[ServerCar]) -> Vec<(String, usize, usize, Vec<(String, usize)>)> {
    let mut groups: Vec<(String, usize, usize, Vec<(String, usize)>)> = Vec::new();
    for car in cars {
        let free = usize::from(!car.connected);
        let group = match groups.iter_mut().position(|g| g.0 == car.model) {
            Some(i) => &mut groups[i],
            None => {
                groups.push((car.model.clone(), 0, 0, Vec::new()));
                groups.last_mut().expect("just pushed")
            }
        };
        group.1 += 1;
        group.2 += free;
        match group.3.iter_mut().find(|s| s.0 == car.skin) {
            Some(skin) => skin.1 += free,
            None => group.3.push((car.skin.clone(), free)),
        }
    }
    groups
}

pub fn page(app: &mut App, ui: &mut egui::Ui) {
    let mut sv = std::mem::take(&mut app.servers);
    let mut notices: Vec<Notice> = Vec::new();
    let ctx = ui.ctx().clone();

    sv.poll_lan_scan();
    if sv.download_ui.as_ref().is_some_and(|d| d.done.load(Ordering::Acquire)) {
        sv.download_ui = None;
        app.refresh();
    }

    widgets::heading(ui, "Servers", "Live players, free cars and your installed content, for your servers and the public lobby.");

    // AAA animated sub-tabs: Watchlist | Online | Recent | LAN.
    let online_badge = (!sv.lobby.is_empty()).then(|| sv.lobby.len().to_string());
    let recent_badge = (!sv.recent.items.is_empty()).then(|| sv.recent.items.len().to_string());
    let lan_badge = (!sv.lan_servers.is_empty()).then(|| sv.lan_servers.len().to_string());
    widgets::animated_tabs(
        ui,
        "online_paddock_tabs",
        &mut sv.tab,
        &[
            (ServerTab::Watchlist, "Watchlist", None),
            (ServerTab::Online, "Online", online_badge.as_deref()),
            (ServerTab::Recent, "Recent", recent_badge.as_deref()),
            (ServerTab::Lan, "LAN", lan_badge.as_deref()),
        ],
    );
    ui.add_space(6.0);

    // An open Online tab keeps itself fresh: a list older than five minutes updates in the background.
    if sv.tab == ServerTab::Online
        && sv.lobby_source == 0
        && let LobbyState::Loaded(at) = &sv.lobby_state
        && at.elapsed() > Duration::from_secs(300)
    {
        sv.start_refresh(&app.tx, &ctx);
    }
    // Poll the watch list, active Slot-Watch, and the selected server in whichever list it is.
    sv.refresh(&app.tx, &ctx, false);
    if let Some(watch) = &mut sv.slot_watch
        && watch.last_poll.elapsed() >= Duration::from_secs(5)
    {
        let entry = watch.entry.clone();
        watch.last_poll = Instant::now();
        sv.refresh_one(&entry, &app.tx, &ctx);
    }
    if let Some(entry) = sv.selected.clone().and_then(|key| sv.entry_for(&key))
        && sv.live.get(&entry.key()).is_none_or(|l| !l.detailed && !l.checking)
    {
        sv.refresh_one(&entry, &app.tx, &ctx);
    }
    ctx.request_repaint_after(Duration::from_secs(5));

    let mut force = false;
    match sv.tab {
        ServerTab::Watchlist => {
            // Add row.
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 520.0).max(180.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut sv.add_input)
                        .hint_text("Address (192.0.2.10:8081) or acmanager:// / acxs:// invite link")
                        .desired_width(width)
                        .margin(egui::Margin::symmetric(10, 7)),
                );
                let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.add(widgets::primary_button("Add server")).clicked() || submit {
                    let text = std::mem::take(&mut sv.add_input);
                    let (last, added, problem) = sv.add_many(&text);
                    if let Some(key) = last {
                        sv.select(key);
                        sv.refresh(&app.tx, &ctx, true);
                        if added > 1 {
                            notices.push(Notice { text: format!("Added {added} servers."), ok: true });
                        }
                    }
                    if let Some(problem) = problem {
                        if added == 0 {
                            sv.add_input = text;
                        }
                        notices.push(Notice { text: problem, ok: false });
                    }
                }
                if ui.button("My local server").on_hover_text("127.0.0.1 on the default HTTP port").clicked()
                    && let Err(e) = sv.add("127.0.0.1")
                {
                    notices.push(Notice { text: e, ok: false });
                }
                if ui.button("Import file").on_hover_text("A text file with one server per line: ip:port").clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("Text", &["txt", "csv", "list"]).pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => {
                            let (last, added, problem) = sv.add_many(&text);
                            if last.is_some() {
                                sv.refresh(&app.tx, &ctx, true);
                            }
                            notices.push(Notice { text: format!("Imported {added} server(s) from {}.", path.display()), ok: added > 0 });
                            if let Some(problem) = problem {
                                notices.push(Notice { text: problem, ok: false });
                            }
                        }
                        Err(e) => notices.push(Notice { text: format!("Could not read the file: {e}"), ok: false }),
                    }
                }
                if ui.button("Refresh").clicked() {
                    force = true;
                }
            });
        }
        ServerTab::Recent => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Servers you have joined recently (persisted in recent-servers.json).").color(MUTED));
                if !sv.recent.items.is_empty() && ui.button("Clear recent").clicked() {
                    sv.recent.items.clear();
                    let _ = sv.recent.save(&recent_path());
                }
            });
        }
        ServerTab::Lan => {
            ui.horizontal(|ui| {
                if ui.add_enabled(!sv.lan_scanning, widgets::primary_button("Scan LAN")).clicked() {
                    sv.start_lan_scan(&ctx);
                }
                if sv.lan_scanning {
                    ui.spinner();
                    ui.label(RichText::new("Listening for UDP beacons (50960) & probing localhost…").color(MUTED));
                } else {
                    ui.label(RichText::new("Discovers local servers (127.0.0.1:8081-8090) and ACXS LAN beacons.").color(MUTED));
                }
            });
        }
        ServerTab::Online => {
        // The lobby needs a click first: it sends the Steam id of whoever is signed in.
        let mut load = false;
        let mut search = false;
        match &sv.lobby_state {
            LobbyState::Idle | LobbyState::Failed(_) => {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Public servers").font(theme::bold(17.0)));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Source").color(MUTED));
                        ui.selectable_value(&mut sv.lobby_source, 0, "AC Servers directory");
                        ui.selectable_value(&mut sv.lobby_source, 1, "Kunos lobby");
                        ui.selectable_value(&mut sv.lobby_source, 2, "Custom list");
                    });
                    ui.add_space(4.0);
                    match sv.lobby_source {
                        0 => {
                            ui.label(
                                RichText::new(
                                    "A community directory of public Assetto Corsa servers (acservers.online, about 10,000 listed). \
                                     This loads the servers that have players right now; the search finds any server by name.",
                                )
                                .color(MUTED),
                            );
                        }
                        1 => {
                            ui.label(
                                RichText::new(
                                    "The official Kunos lobby, asked with the Steam id of the account signed in on this PC. It answers \
                                     only the game's own client and Content Manager, so a request from any other program is turned away.",
                                )
                                .color(MUTED),
                            );
                        }
                        _ => {
                            ui.label(
                                RichText::new(
                                    "Any web address that serves a server list in the lobby's JSON format (plain or gzip).",
                                )
                                .color(MUTED),
                            );
                        }
                    }
                    if let LobbyState::Failed(e) = &sv.lobby_state {
                        ui.add_space(4.0);
                        ui.label(RichText::new(e).color(DANGER));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(widgets::primary_button("Load public servers")).clicked() {
                            load = true;
                        }
                        match sv.lobby_source {
                            1 => {
                                ui.label(RichText::new("Lobby").size(12.5).color(FAINT));
                                ui.add(egui::TextEdit::singleline(&mut sv.lobby_host).desired_width(140.0));
                            }
                            2 => {
                                ui.add(
                                    egui::TextEdit::singleline(&mut sv.custom_url)
                                        .hint_text("https://example.com/servers.json")
                                        .desired_width((ui.available_width() - 8.0).max(200.0))
                                        .margin(egui::Margin::symmetric(8, 5)),
                                );
                            }
                            _ => {}
                        }
                    });
                    if sv.lobby_source == 0 {
                        ui.add_space(6.0);
                        let (s, p) = directory_search_row(ui, &mut sv.dir_query);
                        search |= s;
                        load |= p;
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("You can always add a server by address on My servers, and star it there.")
                            .size(12.0)
                            .color(FAINT),
                    );
                });
            }
            LobbyState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Asking the lobby…").color(MUTED));
                });
            }
            LobbyState::Loaded(_) | LobbyState::Updating { .. } => {
                let status_text = match &sv.lobby_state {
                    LobbyState::Updating { got, of } if *of > 0 => format!("updating… {got} of {of}"),
                    LobbyState::Updating { .. } => "updating…".to_string(),
                    LobbyState::Loaded(at) => {
                        let age = at.elapsed().as_secs();
                        let when = if age < 90 { format!("{age} s ago") } else { format!("{} min ago", age / 60) };
                        if sv.from_cache { format!("saved list from {when}") } else { format!("updated {when}") }
                    }
                    _ => String::new(),
                };
                let updating = matches!(sv.lobby_state, LobbyState::Updating { .. });
                ui.horizontal(|ui| {
                    if ui.add_enabled(!updating, egui::Button::new("Reload")).clicked() {
                        load = true;
                    }
                    widgets::toggle(ui, &mut sv.hide_empty);
                    ui.label(RichText::new("Hide empty").color(MUTED));
                    widgets::toggle(ui, &mut sv.hide_full);
                    ui.label(RichText::new("Hide full").color(MUTED));
                    widgets::toggle(ui, &mut sv.hide_password);
                    ui.label(RichText::new("Hide password").color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(status_text).size(12.5).color(FAINT));
                        if updating {
                            ui.add(egui::Spinner::new().size(14.0));
                        }
                    });
                });
                if let Some(note) = &sv.refresh_note {
                    ui.label(RichText::new(format!("The last refresh failed, showing the older list: {note}")).size(12.0).color(WARN));
                }
                if sv.lobby_source == 0 {
                    let (s, p) = directory_search_row(ui, &mut sv.dir_query);
                    search |= s;
                    load |= p;
                }
            }
        }
        if search {
            sv.search_directory(&app.tx, &ctx);
        }
        if load {
            sv.load_lobby(&app.tx, &ctx);
        }
        }
    }
    if force {
        sv.refresh(&app.tx, &ctx, true);
    }
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut sv.filter)
                .hint_text("Filter by name, address, track or car")
                .desired_width(300.0)
                .margin(egui::Margin::symmetric(10, 6)),
        );
        widgets::toggle(ui, &mut sv.joinable_only);
        ui.label(RichText::new("100% Joinable").color(MUTED))
            .on_hover_text("Only show servers where you own the track, have a free owned car, and meet the CSP build requirement.");
        if sv.tab == ServerTab::Watchlist {
            widgets::toggle(ui, &mut sv.online_only);
            ui.label(RichText::new("Online only").color(MUTED));
        }
        let (up, total) = match sv.tab {
            ServerTab::Online => (sv.lobby.len(), sv.lobby.len()),
            ServerTab::Recent => (sv.recent.items.len(), sv.recent.items.len()),
            ServerTab::Lan => (sv.lan_servers.len(), sv.lan_servers.len()),
            ServerTab::Watchlist => (
                sv.entries.iter().filter(|e| sv.live.get(&e.key()).is_some_and(|l| l.status.is_some())).count(),
                sv.entries.len(),
            ),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = if sv.tab == ServerTab::Watchlist {
                format!("{up} of {total} online")
            } else {
                format!("{total} servers")
            };
            ui.label(RichText::new(text).font(theme::mono(12.0)).color(MUTED));
        });
    });
    ui.add_space(8.0);

    if sv.tab == ServerTab::Watchlist && sv.entries.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No servers yet").font(theme::bold(20.0)));
            ui.add_space(4.0);
            ui.label(RichText::new("Add an address or acmanager:// invite link above, or star one from the Online tab.").color(MUTED));
            ui.label(RichText::new("Hosting your own? Use the Dedicated Server tool, then add it with My local server.").color(FAINT));
        });
        app.servers = sv;
        for n in notices {
            app.push_notice(n.text, n.ok);
        }
        return;
    }
    if sv.tab == ServerTab::Online && sv.lobby.is_empty() {
        app.servers = sv;
        for n in notices {
            app.push_notice(n.text, n.ok);
        }
        return;
    }

    let tracks = app.snapshot.tracks.clone();
    let cars_local = app.snapshot.cars.clone();
    let installed_csp_build = app.snapshot.installed_csp_build;
    let track_for = |status: &ServerStatus| {
        status.track.as_deref().and_then(|raw| resolve_track(raw, &tracks).0)
    };

    // Rows to show: filtered, favourites first, then by the chosen column. Sort keys are
    // computed once per row, not per comparison, so thousands of servers stay cheap.
    let filter = sv.filter.trim().to_lowercase();
    let candidates: Vec<Entry> = match sv.tab {
        ServerTab::Online => sv
            .lobby
            .values()
            .map(|s| Entry {
                host: s.ip.clone(),
                port: s.http_port,
                favorite: sv.entries.iter().any(|e| e.host == s.ip && e.port == s.http_port && e.favorite),
            })
            .collect(),
        ServerTab::Recent => sv
            .recent
            .items
            .iter()
            .map(|r| Entry {
                host: r.host.clone(),
                port: r.port,
                favorite: sv.entries.iter().any(|e| e.host == r.host && e.port == r.port && e.favorite),
            })
            .collect(),
        ServerTab::Lan => sv
            .lan_servers
            .iter()
            .map(|s| Entry {
                host: s.host.clone(),
                port: s.http_port,
                favorite: sv.entries.iter().any(|e| e.host == s.host && e.port == s.http_port && e.favorite),
            })
            .collect(),
        ServerTab::Watchlist => sv.entries.clone(),
    };
    let track_ids: Vec<&str> = tracks.iter().map(|t| t.id.as_str()).collect();
    let mut keyed: Vec<(Entry, (String, u32, u128))> = candidates
        .into_iter()
        .filter(|e| {
            let key = e.key();
            let status = sv.live.get(&key).and_then(|l| l.status.as_ref());
            if sv.joinable_only {
                let Some(s) = status else {
                    return false;
                };
                if let Some(raw_track) = s.track.as_deref() {
                    let res = server_status::resolve_track_id(raw_track, &track_ids);
                    if !tracks.iter().any(|t| t.id == res.track_id) {
                        return false;
                    }
                    if let Some(req) = res.min_csp_build
                        && installed_csp_build.is_none_or(|cur| cur < req)
                    {
                        return false;
                    }
                }
                if !s.cars.is_empty()
                    && !s.cars.iter().any(|c| !c.connected && cars_local.iter().any(|l| l.id == c.model))
                {
                    return false;
                }
            }
            if sv.tab == ServerTab::Online {
                if let Some(s) = sv.lobby.get(&key) {
                    if (sv.hide_empty && s.clients == 0)
                        || (sv.hide_full && s.capacity > 0 && s.clients >= s.capacity)
                        || (sv.hide_password && s.password)
                    {
                        return false;
                    }
                    if !filter.is_empty() && s.cars.iter().any(|c| c.to_lowercase().contains(&filter)) {
                        return true;
                    }
                }
            } else if sv.tab == ServerTab::Watchlist && sv.online_only && status.is_none() {
                return false;
            }
            filter.is_empty()
                || e.host.to_lowercase().contains(&filter)
                || status.is_some_and(|s| {
                    s.name.as_deref().is_some_and(|n| n.to_lowercase().contains(&filter))
                        || s.track.as_deref().is_some_and(|t| t.to_lowercase().contains(&filter))
                        || s.cars.iter().any(|c| c.model.to_lowercase().contains(&filter))
                })
        })
        .map(|e| {
            let live = sv.live.get(&e.key());
            let status = live.and_then(|l| l.status.as_ref());
            let text = match sv.sort_col {
                0 => status.and_then(|s| s.name.clone()).unwrap_or_default().to_lowercase(),
                1 => status.and_then(|s| s.track.clone()).unwrap_or_default(),
                _ => String::new(),
            };
            let key = (text, status.and_then(|s| s.clients).unwrap_or(0), live.and_then(|l| l.latency_ms).unwrap_or(u128::MAX));
            (e, key)
        })
        .collect();
    let (col, desc) = (sv.sort_col, sv.sort_desc);
    keyed.sort_by(|(a, ka), (b, kb)| {
        let ord = match col {
            0 | 1 => ka.0.cmp(&kb.0),
            3 => ka.2.cmp(&kb.2),
            _ => ka.1.cmp(&kb.1),
        };
        b.favorite.cmp(&a.favorite).then(if desc { ord.reverse() } else { ord })
    });
    let shown: Vec<Entry> = keyed.into_iter().map(|(e, _)| e).collect();

    let mut remove: Option<String> = None;
    let mut check: Option<String> = None;
    let mut star: Option<String> = None;
    let mut chosen: Option<String> = None;
    let mut sort_click: Option<usize> = None;
    let mut join_request: Option<JoinRequest> = None;

    // Detail panel for the selected server — rendered FIRST so egui reserves the exact right-pane
    // width before the server list table lays out in the remaining space.
    let selected = sv.selected.clone();
    let mut panel_edit: Option<(String, Option<String>, Option<String>, String, String, String, bool)> = None;
    let mut panel_actions: Vec<detail::Action> = Vec::new();
    let mut panel_key = String::new();
    let detail_resp = egui::Panel::right("server_detail_panel")
        .resizable(true)
        .default_size(420.0)
        .size_range(340.0..=540.0)
        .frame(
            egui::Frame::new()
                .fill(theme::SIDEBAR)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(14)),
        )
        .show(ui, |ui| {
            let Some(entry) = selected.as_ref().and_then(|key| sv.entry_for(key)) else {
                widgets::empty_state(ui, "Select a server to see its cars and join it.");
                return;
            };
            let key = entry.key();
            let watching_slot = sv.slot_watch.as_ref().is_some_and(|w| w.entry.key() == key);
            let download_progress = sv
                .download_ui
                .as_ref()
                .and_then(|d| d.progress.lock().ok().map(|s| s.clone()));
            let booking_status = sv.booking_status.get(&key).cloned();
            let input = detail::DetailIn {
                entry: &entry,
                live: sv.live.get(&key).cloned().unwrap_or_default(),
                lobby: sv.lobby.get(&key),
                in_list: sv.entries.iter().any(|e| e.key() == key),
                tracks: &tracks,
                cars: &cars_local,
                installed_csp_build,
                game_running: app.snapshot.game_running,
                watching_slot,
                download_progress,
                booking_status,
            };
            let (mut pick_model, mut pick_skin) = (sv.pick_model.clone(), sv.pick_skin.clone());
            let (mut driver, mut password, mut car_query) = (sv.driver.clone(), sv.password.clone(), sv.car_query.clone());
            let mut installed_only = sv.installed_only;
            let mut edit = detail::Edit {
                pick_model: &mut pick_model,
                pick_skin: &mut pick_skin,
                driver: &mut driver,
                password: &mut password,
                car_query: &mut car_query,
                installed_only: &mut installed_only,
            };
            panel_actions = detail::show(ui, &input, &mut edit);
            panel_key = key;
            panel_edit = Some((String::new(), pick_model, pick_skin, driver, password, car_query, installed_only));
        });
    let detail_rect = detail_resp.response.rect;
    if let Some((_, pick_model, pick_skin, driver, password, car_query, installed_only)) = panel_edit {
        if let Some(model) = &pick_model {
            sv.picks.insert(panel_key.clone(), (model.clone(), pick_skin.clone().unwrap_or_default()));
        }
        sv.pick_model = pick_model;
        sv.pick_skin = pick_skin;
        sv.driver = driver;
        sv.password = password;
        sv.car_query = car_query;
        sv.installed_only = installed_only;
    }
    for action in panel_actions {
        match action {
            detail::Action::Join(request) => join_request = Some(request),
            detail::Action::BookSlot(request) => {
                let host = request.entry.host.clone();
                let port = request.entry.port;
                let key = request.entry.key();
                let model = request.model.clone().unwrap_or_default();
                let skin = request.skin.clone().unwrap_or_default();
                let driver = if sv.driver.trim().is_empty() { "Driver".into() } else { sv.driver.trim().to_string() };
                let password = sv.password.clone();
                sv.booking_status.insert(key.clone(), "Booking (/SUB)…".into());
                let tx = app.tx.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let res = (|| -> Result<(), String> {
                        let steam_guid = lobby::current_steam_id64(&acxs_manager::steam::default_steam_roots())
                            .ok_or("Steam is not signed in.")?;
                        let req = BookingRequest {
                            driver_name: driver,
                            steam_guid,
                            car_model: model,
                            car_skin: skin,
                            password,
                        };
                        server_status::book(&host, port, &req, Duration::from_secs(5))
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })();
                    let _ = tx.send(Msg::BookingReady(key, res));
                    ctx.request_repaint();
                });
            }
            detail::Action::ToggleSlotWatch { entry, model, skin } => {
                if sv.slot_watch.as_ref().is_some_and(|w| w.entry.key() == entry.key()) {
                    sv.slot_watch = None;
                    notices.push(Notice { text: "Stopped Slot-Watch auto-join.".into(), ok: true });
                } else {
                    notices.push(Notice { text: format!("Watching {model} on {} — will auto-join when free.", entry.key()), ok: true });
                    sv.slot_watch = Some(SlotWatch {
                        entry,
                        model,
                        skin,
                        last_poll: Instant::now() - Duration::from_secs(5),
                    });
                }
            }
            detail::Action::DownloadMissing(plan) => {
                if let Some(ac_root) = app.settings.ac_root_path() {
                    let cancel = Arc::new(AtomicBool::new(false));
                    let progress = Arc::new(Mutex::new(format!("Starting download ({} item(s))…", plan.items.len())));
                    let done = Arc::new(AtomicBool::new(false));
                    sv.download_ui = Some(DownloadUi {
                        cancel: Arc::clone(&cancel),
                        progress: Arc::clone(&progress),
                        done: Arc::clone(&done),
                    });
                    let tx = app.tx.clone();
                    let ctx = ctx.clone();
                    let seven_zip = app.settings.seven_zip_path.clone().map(PathBuf::from);
                    std::thread::spawn(move || {
                        let res = content_dl::download_and_install_plan(
                            &ac_root,
                            &plan,
                            acxs_manager::game::is_game_running(),
                            seven_zip.as_deref(),
                            &cancel,
                            |p| {
                                if let Ok(mut text) = progress.lock() {
                                    let mb = p.bytes_done as f64 / (1024.0 * 1024.0);
                                    *text = match p.bytes_total {
                                        Some(t) if t > 0 => format!("Downloading {} ({mb:.1} / {:.1} MB)…", p.item_id, t as f64 / (1024.0 * 1024.0)),
                                        _ => format!("Downloading {} ({mb:.1} MB)…", p.item_id),
                                    };
                                }
                                ctx.request_repaint();
                            },
                        );
                        done.store(true, Ordering::Release);
                        let notice = match res {
                            Ok(reports) => Notice {
                                text: format!("Installed {} missing content package(s).", reports.len()),
                                ok: true,
                            },
                            Err(e) => Notice { text: format!("Missing content install failed: {e}"), ok: false },
                        };
                        let _ = tx.send(Msg::DownloadFinished(notice));
                        ctx.request_repaint();
                    });
                } else {
                    notices.push(Notice { text: "Set your Assetto Corsa folder in Settings first.".into(), ok: false });
                }
            }
            detail::Action::CancelDownload => {
                if let Some(dl) = &sv.download_ui {
                    dl.cancel.store(true, Ordering::Release);
                }
            }
            detail::Action::CheckNow => check = Some(panel_key.clone()),
            detail::Action::CopyAddress => {
                ctx.copy_text(panel_key.clone());
                notices.push(Notice { text: "Address copied.".into(), ok: true });
            }
            detail::Action::CopyInviteLink => {
                if let Some(entry) = sv.entry_for(&panel_key) {
                    let link = uri::format_acmanager_uri(
                        &entry.host,
                        entry.port,
                        (!sv.password.is_empty()).then_some(sv.password.as_str()),
                    );
                    ctx.copy_text(link);
                    notices.push(Notice { text: "acmanager:// invite link copied.".into(), ok: true });
                }
            }
            detail::Action::Remove => remove = Some(panel_key.clone()),
            detail::Action::AddToList | detail::Action::ToggleFavorite => star = Some(panel_key.clone()),
        }
    }

    // The table — strictly isolated to the remaining area left of `detail_rect` and clipped so
    // PLAYERS / PING can NEVER bleed over the right-hand detail panel at any window width.
    let current = sv.selected.clone();
    let live_map = &sv.live;
    let live_map_lobby = &sv.lobby;
    let avail = ui.available_rect_before_wrap();
    let right_limit = (detail_rect.left() - 10.0).min(avail.max.x);
    let table_rect = Rect::from_min_max(avail.min, egui::pos2(right_limit.max(avail.min.x + 1.0), avail.max.y));

    ui.scope_builder(egui::UiBuilder::new().max_rect(table_rect), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(table_rect));
        let width = table_rect.width().max(1.0);
        let cols = compute_table_columns(width);

        // Header.
        let (head, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
        let header_specs = [
            ("Server", cols.x[0], cols.widths[0]),
            ("Track", cols.x[1], cols.widths[1]),
            ("Players", cols.x[2], cols.widths[2]),
            ("Ping", cols.x[3], cols.widths[3]),
        ];
        for (i, (label, x, col_w)) in header_specs.iter().enumerate() {
            let cell = Rect::from_min_size(head.min + vec2(*x - 4.0, 0.0), vec2((*col_w).max(16.0), 28.0));
            let response = ui.interact(cell, ui.next_auto_id().with(("srv_head", i)), Sense::click());
            let active = sv.sort_col == i;
            let arrow = if active { if sv.sort_desc { " ▼" } else { " ▲" } } else { "" };
            let text_color = if active { ACCENT } else if response.hovered() { TEXT } else { FAINT };
            let galley = media::clipped(
                ui,
                &format!("{}{arrow}", label.to_uppercase()),
                theme::bold(11.0),
                text_color,
                (*col_w - 4.0).max(8.0),
            );
            let size = galley.size();
            ui.painter().galley(head.min + vec2(*x, 14.0 - size.y * 0.5), galley, text_color);
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                sort_click = Some(i);
            }
        }
        ui.painter().hline(head.x_range(), head.bottom(), egui::Stroke::new(1.0, theme::BORDER));

        if shown.is_empty() {
            widgets::empty_state(ui, "No server matches.");
        } else {
            ui.spacing_mut().item_spacing.y = 2.0;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_HEIGHT, shown.len(), |ui, range| {
                let row_w = ui.available_width().min(width).max(1.0);
                let row_cols = compute_table_columns(row_w);
                for index in range {
                    let entry = &shown[index];
                    let live = live_map.get(&entry.key());
                    let status = live.and_then(|l| l.status.as_ref());
                    let (rect, response) = ui.allocate_exact_size(vec2(row_w, ROW_HEIGHT), Sense::click());
                    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
                    let selected = current.as_deref() == Some(entry.key().as_str());
                    let hover = ui.ctx().animate_bool_with_time(response.id, response.hovered(), 0.1);
                    let fill = if selected {
                        theme::CARD_SELECTED
                    } else {
                        Color32::TRANSPARENT.lerp_to_gamma(theme::CARD_HOVER, hover)
                    };
                    painter.rect_filled(rect, CornerRadius::same(8), fill);
                    if selected {
                        painter.rect_filled(
                            Rect::from_center_size(rect.left_center() + vec2(2.0, 0.0), vec2(3.0, 18.0)),
                            CornerRadius::same(2),
                            ACCENT,
                        );
                    }

                    // Star.
                    let star_rect = Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(26.0, 26.0));
                    let star_resp = ui.interact(star_rect, response.id.with("star"), Sense::click());
                    let (glyph, color) = if entry.favorite { ("★", WARN) } else { ("☆", if star_resp.hovered() { TEXT } else { FAINT }) };
                    painter.text(star_rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(15.0), color);
                    let star_clicked = star_resp.on_hover_text("Favorite").clicked();
                    if star_clicked {
                        star = Some(entry.key());
                    }

                    // Status dot, name.
                    let dot = if status.is_some() { OK } else if live.is_some_and(|l| l.checking) { WARN } else { FAINT };
                    painter.circle_filled(rect.left_center() + vec2(row_cols.x[0] + 4.0, 0.0), 4.0, dot);
                    let title = status.and_then(|s| s.name.clone()).unwrap_or_else(|| entry.key());
                    let name_max = (row_cols.widths[0] - 24.0).max(8.0);
                    let title_color = if status.is_some() { TEXT } else { MUTED };
                    let galley = media::clipped(ui, &title, FontId::proportional(14.0), title_color, name_max);
                    let size = galley.size();
                    painter.galley(rect.left_center() + vec2(row_cols.x[0] + 16.0, -size.y / 2.0), galley, title_color);

                    // Track.
                    if let Some(s) = status {
                        let track_name = track_for(s)
                            .map(|t| t.name.clone())
                            .or_else(|| s.track.as_deref().map(|raw| resolve_track(raw, &tracks).1))
                            .or_else(|| live_map_lobby.get(&entry.key()).map(|l| l.track_name.clone()))
                            .unwrap_or_default();
                        let missing = s.track.is_some() && track_for(s).is_none();
                        let track_color = if missing { WARN } else { MUTED };
                        let track_max = (row_cols.widths[1] - 10.0).max(8.0);
                        let galley = media::clipped(ui, &track_name, FontId::proportional(13.0), track_color, track_max);
                        let size = galley.size();
                        painter.galley(rect.left_center() + vec2(row_cols.x[1], -size.y / 2.0), galley, track_color);

                        // Players with a small fill bar.
                        let (n, max) = (s.clients.unwrap_or(0), s.max_clients.unwrap_or(0));
                        let bar_w = (row_cols.widths[2] - 16.0).max(16.0);
                        let bar = Rect::from_min_size(rect.left_center() + vec2(row_cols.x[2], 6.0), vec2(bar_w, 4.0));
                        painter.rect_filled(bar, CornerRadius::same(2), theme::INPUT);
                        if max > 0 {
                            let share = (n as f32 / max as f32).clamp(0.0, 1.0);
                            let tint = if share > 0.9 { WARN } else { OK };
                            painter.rect_filled(
                                Rect::from_min_size(bar.min, vec2(bar.width() * share, 4.0)),
                                CornerRadius::same(2),
                                tint,
                            );
                        }
                        let players_galley = media::clipped(
                            ui,
                            &format!("{n}/{max}"),
                            FontId::proportional(12.5),
                            TEXT,
                            (row_cols.widths[2] - 6.0).max(8.0),
                        );
                        let psize = players_galley.size();
                        painter.galley(rect.left_center() + vec2(row_cols.x[2], -6.0 - psize.y * 0.5), players_galley, TEXT);
                    }
                    if let Some(ms) = live.and_then(|l| l.latency_ms) {
                        let color = if ms < 60 { OK } else if ms < 140 { WARN } else { DANGER };
                        let ping_galley = media::clipped(
                            ui,
                            &format!("{ms} ms"),
                            FontId::proportional(12.5),
                            color,
                            (row_cols.widths[3] - 4.0).max(8.0),
                        );
                        let psize = ping_galley.size();
                        painter.galley(rect.left_center() + vec2(row_cols.x[3], -psize.y * 0.5), ping_galley, color);
                    }

                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() && !star_clicked {
                        chosen = Some(entry.key());
                    }
                    if response.double_clicked() {
                        join_request = Some(JoinRequest { entry: entry.clone(), model: None, skin: None });
                    }
                    response.context_menu(|ui| {
                        if ui.button("Join").clicked() {
                            join_request = Some(JoinRequest { entry: entry.clone(), model: None, skin: None });
                            ui.close();
                        }
                        if ui.button(if entry.favorite { "Remove from favorites" } else { "Add to favorites" }).clicked() {
                            star = Some(entry.key());
                            ui.close();
                        }
                        if ui.button("Copy address").clicked() {
                            ui.ctx().copy_text(entry.key());
                            ui.close();
                        }
                        if ui.button("Remove").clicked() {
                            remove = Some(entry.key());
                            ui.close();
                        }
                    });
                }
            });
        }
    });

    if let Some(col) = sort_click {
        if sv.sort_col == col {
            sv.sort_desc = !sv.sort_desc;
        } else {
            sv.sort_col = col;
            sv.sort_desc = col == 2;
        }
    }
    if let Some(key) = chosen {
        sv.select(key);
    }
    if let Some(key) = star {
        match sv.entries.iter_mut().find(|e| e.key() == key) {
            Some(entry) => entry.favorite = !entry.favorite,
            // Starring a public server puts it on the watch list.
            None => {
                if let Some(mut entry) = sv.entry_for(&key) {
                    entry.favorite = true;
                    sv.entries.push(entry);
                }
            }
        }
        sv.save();
    }
    if let Some(key) = check
        && let Some(entry) = sv.entry_for(&key)
    {
        sv.refresh_one(&entry, &app.tx, &ctx);
    }
    if let Some(key) = remove {
        sv.entries.retain(|e| e.key() != key);
        sv.live.remove(&key);
        if sv.selected.as_deref() == Some(key.as_str()) {
            sv.selected = None;
        }
        sv.save();
    }

    if join_request.is_none()
        && let Some(auto_req) = sv.pending_auto_join.take()
    {
        notices.push(Notice { text: format!("Slot opened on {} — joining!", auto_req.entry.key()), ok: true });
        join_request = Some(auto_req);
    }
    if let Some(request) = join_request {
        // Record in Recent servers and keep in Watchlist.
        let key = request.entry.key();
        let live_status = sv.live.get(&key).and_then(|l| l.status.as_ref());
        let listed = sv.lobby.get(&key);
        let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        sv.recent.record(RecentServer {
            host: request.entry.host.clone(),
            port: request.entry.port,
            name: live_status
                .and_then(|s| s.name.clone())
                .or_else(|| listed.map(|s| s.name.clone()))
                .unwrap_or_else(|| key.clone()),
            track: live_status
                .and_then(|s| s.track.clone())
                .or_else(|| listed.map(|s| s.track.clone()))
                .unwrap_or_default(),
            last_car: request.model.clone().unwrap_or_default(),
            last_skin: request.skin.clone().unwrap_or_default(),
            last_joined_unix: now_unix,
            join_count: 1,
        });
        let _ = sv.recent.save(&recent_path());

        if !sv.entries.iter().any(|entry| entry.key() == key) {
            sv.entries.push(Entry { favorite: false, ..request.entry.clone() });
            sv.save();
        }
        start_join(app, &sv, request, &mut notices);
    }
    app.servers = sv;
    for n in notices {
        app.push_notice(n.text, n.ok);
    }
}

/// Column geometry for the server list table.
/// Guarantees `x[3] + widths[3] <= width` for EVERY positive table width.
#[derive(Debug, Clone, Copy)]
struct TableColumns {
    x: [f32; 4],
    widths: [f32; 4],
}

fn compute_table_columns(width: f32) -> TableColumns {
    let star_w = 36.0_f32.min(width * 0.12);
    let pad_r = 6.0_f32.min(width * 0.02);
    let usable = (width - star_w - pad_r).max(0.0);

    let ping_w = (usable * 0.14).clamp(44.0, 64.0).min(usable * 0.22);
    let rem_after_ping = (usable - ping_w).max(0.0);
    let players_w = (usable * 0.20).clamp(64.0, 108.0).min(rem_after_ping * 0.34);
    let rem_after_players = (rem_after_ping - players_w).max(0.0);
    let track_w = (usable * 0.25).clamp(52.0, 175.0).min(rem_after_players * 0.42);
    let name_w = (rem_after_players - track_w).max(0.0);

    let x0 = star_w;
    let x1 = x0 + name_w;
    let x2 = x1 + track_w;
    let x3 = x2 + players_w;
    TableColumns {
        x: [x0, x1, x2, x3],
        widths: [name_w, track_w, players_w, ping_w],
    }
}

/// Pretty-printed JSON in a monospace, scrollable box with a copy button.
fn raw_json(ui: &mut egui::Ui, raw: &str) {
    let pretty = serde_json::from_str::<serde_json::Value>(raw)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| raw.to_string());
    if ui.small_button("Copy").clicked() {
        ui.ctx().copy_text(pretty.clone());
    }
    theme::card_flat().show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(pretty).monospace().size(11.5).color(MUTED)).wrap_mode(egui::TextWrapMode::Extend));
        });
    });
}

/// The driver name of the last race, read from the game's `race.ini`.
fn previous_driver_name() -> String {
    acxs_manager::settings::ac_user_cfg_dir()
        .and_then(|cfg| std::fs::read_to_string(cfg.join("race.ini")).ok())
        .and_then(|text| {
            text.lines().find_map(|line| {
                let name = line.trim().strip_prefix("DRIVER_NAME=")?.trim();
                (!name.is_empty() && !name.starts_with("AI ")).then(|| name.to_string())
            })
        })
        .unwrap_or_else(|| "Driver".to_string())
}

/// Splits a server's track id into the installed track and its layout.
///
/// Servers that need a Custom Shaders Patch build name their track like
/// `csp/2651/../la_canyons-freeroam`: a version prefix, the track folder, a dash, the layout
/// folder. Returns the installed track (when there is one), its folder name and the layout.
fn resolve_track<'a>(raw: &str, installed: &'a [TrackEntry]) -> (Option<&'a TrackEntry>, String, Option<String>) {
    let ids: Vec<&str> = installed.iter().map(|t| t.id.as_str()).collect();
    let res = server_status::resolve_track_id(raw, &ids);
    let found = installed.iter().find(|t| t.id == res.track_id);
    (found, res.track_id, res.layout)
}

#[cfg(test)]
mod track_tests {
    use super::*;

    fn track(id: &str) -> TrackEntry {
        TrackEntry {
            id: id.into(),
            name: id.into(),
            country: String::new(),
            city: String::new(),
            length_m: None,
            pitboxes: None,
            layouts: Vec::new(),
            tags: Vec::new(),
            path: std::path::PathBuf::new(),
        }
    }

    #[test]
    fn splits_a_csp_track_id_into_track_and_layout() {
        let installed = vec![track("la_canyons"), track("ks_barcelona")];
        let (found, id, layout) = resolve_track("csp/2651/../la_canyons-freeroam", &installed);
        assert_eq!(found.map(|t| t.id.as_str()), Some("la_canyons"));
        assert_eq!(id, "la_canyons");
        assert_eq!(layout.as_deref(), Some("freeroam"));
    }

    #[test]
    fn plain_ids_and_layouts_work() {
        let installed = vec![track("ks_barcelona")];
        assert_eq!(resolve_track("ks_barcelona", &installed).2, None);
        let (found, _, layout) = resolve_track("ks_barcelona-layout_gp", &installed);
        assert!(found.is_some());
        assert_eq!(layout.as_deref(), Some("layout_gp"));
    }

    #[test]
    fn a_track_with_a_dash_in_its_name_is_found_whole() {
        let installed = vec![track("my-track")];
        assert!(resolve_track("my-track", &installed).0.is_some());
    }

    #[test]
    fn missing_tracks_still_give_a_clean_name() {
        let (found, id, layout) = resolve_track("csp/2651/../other-gp", &[]);
        assert!(found.is_none());
        assert_eq!((id.as_str(), layout.as_deref()), ("other", Some("gp")));
    }
}


/// The directory's search box. Returns (search pressed, "servers with players" pressed).
fn directory_search_row(ui: &mut egui::Ui, query: &mut String) -> (bool, bool) {
    let (mut search, mut popular) = (false, false);
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(query)
                .hint_text("Search every listed server by name…")
                .desired_width(340.0)
                .margin(egui::Margin::symmetric(10, 6)),
        );
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("Search").clicked() || enter {
            search = true;
        }
        if ui.button("Servers with players").clicked() {
            popular = true;
        }
        ui.label(RichText::new(format!("list: {}", directory::SITE)).size(12.0).color(FAINT));
    });
    (search, popular)
}

fn cache_path() -> PathBuf {
    file_path().with_file_name("directory-cache.json")
}

/// The servers of the last refresh and when they were saved (seconds since 1970).
fn load_cache() -> Option<(Vec<LobbyServer>, u64)> {
    let text = std::fs::read_to_string(cache_path()).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let saved = value.get("saved")?.as_u64()?;
    let servers = serde_json::from_value(value.get("servers")?.clone()).ok()?;
    Some((servers, saved))
}

/// Write the list for the next start, off the UI thread.
fn save_cache(servers: Vec<LobbyServer>) {
    std::thread::spawn(move || {
        let saved = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(text) = serde_json::to_string(&serde_json::json!({ "saved": saved, "servers": servers })) {
            let _ = std::fs::write(cache_path(), text);
        }
    });
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    fn server(ip: &str, clients: u32) -> LobbyServer {
        LobbyServer { ip: ip.into(), http_port: 8081, name: format!("S {ip}"), clients, capacity: 20, ..LobbyServer::default() }
    }

    #[test]
    fn a_refresh_updates_in_place_and_drops_what_did_not_come_back() {
        let mut sv = Servers::default();
        // The list of the last session.
        for (ip, n) in [("10.0.0.1", 5), ("10.0.0.2", 7), ("10.0.0.3", 9)] {
            sv.upsert_lobby(server(ip, n));
        }
        sv.lobby_state = LobbyState::Updating { got: 0, of: 0 };

        // Page one: .1 has new numbers and .4 is new. The old list stays visible meanwhile.
        sv.apply_part(vec![server("10.0.0.1", 11), server("10.0.0.4", 2)], 4, false);
        assert_eq!(sv.lobby.len(), 4);
        assert_eq!(sv.lobby["10.0.0.1:8081"].clients, 11);
        assert!(matches!(sv.lobby_state, LobbyState::Updating { got: 2, of: 4 }));

        // Last page: .2 and .3 did not come back, so they are gone.
        sv.apply_part(vec![server("10.0.0.2", 8)], 4, false);
        sv.apply_part(Vec::new(), 4, true);
        let mut keys: Vec<_> = sv.lobby.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["10.0.0.1:8081", "10.0.0.2:8081", "10.0.0.4:8081"]);
        assert!(matches!(sv.lobby_state, LobbyState::Loaded(_)));
    }

    #[test]
    fn a_failed_refresh_keeps_the_older_list() {
        let mut sv = Servers::default();
        sv.upsert_lobby(server("10.0.0.1", 5));
        sv.apply_lobby(Err("offline".into()));
        assert_eq!(sv.lobby.len(), 1);
        assert!(sv.refresh_note.is_some());
        assert!(matches!(sv.lobby_state, LobbyState::Loaded(_)));
    }

    #[test]
    fn a_servers_own_answer_survives_a_refresh() {
        let mut sv = Servers::default();
        sv.upsert_lobby(server("10.0.0.1", 5));
        let live = sv.live.get_mut("10.0.0.1:8081").unwrap();
        live.detailed = true;
        live.status.as_mut().unwrap().name = Some("Real name".into());
        sv.upsert_lobby(server("10.0.0.1", 6));
        assert_eq!(sv.live["10.0.0.1:8081"].status.as_ref().unwrap().name.as_deref(), Some("Real name"));
    }

    #[test]
    fn table_columns_never_overflow_at_any_viewport_width() {
        // Test every window-derived table width from ultra-narrow (200px) to 2560px ultrawide.
        for width in [200.0_f32, 320.0, 480.0, 580.0, 720.0, 920.0, 1280.0, 1600.0, 2100.0, 2560.0] {
            let cols = compute_table_columns(width);
            let right_edge = cols.x[3] + cols.widths[3];
            assert!(
                right_edge <= width + 0.01,
                "columns overflowed at width={width}: right_edge={right_edge}, cols={cols:?}"
            );
            for i in 0..3 {
                assert!(
                    cols.x[i] + cols.widths[i] <= cols.x[i + 1] + 0.01,
                    "column {i} overlaps column {} at width={width}: cols={cols:?}",
                    i + 1
                );
            }
        }
    }
}
