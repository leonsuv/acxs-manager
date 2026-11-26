//! The AAA 6-Pillar egui front end. Heavy work (scanning, hashing, installing) runs on
//! background threads and reports back through a channel, so the window stays responsive.

mod media;
pub mod nav;
mod pages;
mod servers;
mod showroom;
mod theme;
mod tools;
pub mod viewport3d;
mod widgets;
pub mod window_state;

use crate::content::{scan_cars, scan_tracks, CarEntry, TrackEntry};
use crate::csp::{self, LightsPatch};
use crate::game;
use crate::settings::{self, AccentPreset, ArmedSession, LaunchTarget, Settings};
use acxs_manager::ipc::{self, IpcListener, IpcMessage};
use acxs_manager::uri::JoinUri;
use eframe::egui::{self, Align2, Color32, CornerRadius, Margin, Rect, RichText, Sense, Stroke, StrokeKind, vec2};
use nav::{
    CarSubView, CspView, DriveTab, FocusNav, GameSettingsView, GarageTab, LibraryView, NavState, OnlineTab,
    PaddockTab, Pillar, Route, SetupTab, SetupsView, StudioView, SystemView, TelemetryTab, TrackSubView,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

/// Legacy page enum retained in sync with [`Route`] for backward compatibility with Wave 3 pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Showroom,
    Servers,
    Cars,
    Tracks,
    Patches,
    Settings,
    /// Index into [`App::tools`].
    Tool(usize),
}

#[derive(Debug, Clone)]
struct Toast {
    text: String,
    ok: bool,
    born: Instant,
}

#[derive(Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub ok: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub ac_valid: bool,
    pub cars: Vec<CarEntry>,
    pub tracks: Vec<TrackEntry>,
    pub installed: Option<String>,
    /// Numeric CSP build parsed from `extension/config/data_manifest.ini` or known patches.
    pub installed_csp_build: Option<u32>,
    /// sha256 of the live dwrite.dll, used to tell which patch is active.
    pub installed_sha: Option<String>,
    pub patches: Vec<LightsPatch>,
    pub game_running: bool,
}

#[allow(dead_code)]
pub(crate) enum Msg {
    Notice(Notice),
    Snapshot(u64, Snapshot),
    GameRunning(bool),
    Server(String, Result<(acxs_manager::server_status::ServerStatus, u128), String>),
    Lobby(Result<Vec<acxs_manager::lobby::LobbyServer>, String>),
    LobbyPart { servers: Vec<acxs_manager::lobby::LobbyServer>, total: u64, last: bool },
    OpenUri(JoinUri),
    FocusWindow,
    DownloadFinished(Notice),
    BookingReady(String, Result<(), String>),
}

const GAME_POLL: std::time::Duration = std::time::Duration::from_secs(2);

pub struct App {
    settings: Settings,
    recorder: Arc<acxs_manager::session_recorder::RecorderControl>,
    page: Page,
    last_synced_page: Page,
    pub route: Route,
    pub nav_state: NavState,
    pub focus: FocusNav,
    ac_input: String,
    library_input: String,
    car_query: String,
    track_query: String,
    car_skin: Option<String>,
    track_layout: Option<String>,
    selected_car: Option<String>,
    selected_track: Option<String>,
    confirm_uninstall: bool,
    snapshot: Snapshot,
    busy: bool,
    pending: usize,
    generation: u64,
    ctx: egui::Context,
    tools: Vec<Box<dyn tools::Tool>>,
    favorites: HashSet<String>,
    showroom_car: Option<String>,
    home_pick: Option<String>,
    car_filter: String,
    car_sort: usize,
    palette_open: bool,
    palette_query: String,
    palette_sel: usize,
    toasts: Vec<Toast>,
    servers: servers::Servers,
    grid_columns: usize,
    race_request: Option<pages::RaceAction>,
    picker: Option<tools::PickKind>,
    window: window_state::Tracker,
    /// Legacy pinned entries migrated read-only from `sidebar.json` into `Ctrl+K`.
    pinned: Vec<String>,
    gl_renderer: Option<Arc<Mutex<viewport3d::GlRenderer>>>,
    _ipc: Option<IpcListener>,
    quick_switches_open: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

fn gather(ac_root: Option<PathBuf>, library: PathBuf) -> Snapshot {
    let game_running = game::is_game_running();
    let mut snapshot = Snapshot { game_running, ..Snapshot::default() };
    let mut roots = vec![library];
    if let Some(root) = ac_root.filter(|r| settings::is_ac_root(r)) {
        snapshot.ac_valid = true;
        snapshot.cars = scan_cars(&root);
        snapshot.tracks = scan_tracks(&root);
        snapshot.installed = csp::installed_version(&root).ok().flatten();
        snapshot.installed_csp_build = csp::installed_build(&root);
        snapshot.installed_sha = csp::sha256_file(&root.join(csp::DLL_NAME)).ok();
        roots.push(csp::archive_dir(&root));
    }
    snapshot.patches = csp::discover_patches(&roots);
    snapshot
}

fn start_game_watch(tx: Sender<Msg>, ctx: egui::Context) {
    thread::spawn(move || {
        let mut last = None;
        loop {
            thread::sleep(GAME_POLL);
            let running = game::is_game_running();
            if last != Some(running) {
                if tx.send(Msg::GameRunning(running)).is_err() {
                    return;
                }
                last = Some(running);
                ctx.request_repaint();
            }
        }
    });
}

impl App {
    /// Standard constructor (delegates to [`App::new_with_uri`] with `None`).
    #[allow(dead_code)]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::new_with_uri(cc, None)
    }

    /// Construct the app with an optional initial `acmanager://` / `acxs://` join URI.
    pub fn new_with_uri(cc: &eframe::CreationContext<'_>, initial_uri: Option<JoinUri>) -> Self {
        let mut settings = Settings::load(&settings::settings_path());
        theme::apply_with_accent(&cc.egui_ctx, settings.accent_preset);

        if settings.ac_root_path().is_none()
            && let Some(found) = settings::detect_ac_root()
        {
            settings.ac_root = found.display().to_string();
            let _ = settings.save(&settings::settings_path());
        }
        if settings.crash_reports
            && let Ok(dsn) = acxs_manager::crash_report::Dsn::parse(acxs_manager::crash_report::DEFAULT_DSN)
        {
            let settings_file = settings::settings_path();
            let dir = acxs_manager::crash_report::pending_dir(settings_file.parent().unwrap_or(Path::new(".")));
            thread::spawn(move || {
                let _ = acxs_manager::crash_report::send_pending(&dsn, &dir, std::time::Duration::from_secs(10));
            });
        }

        let recorder = Arc::new(acxs_manager::session_recorder::RecorderControl::default());
        {
            let ctx = cc.egui_ctx.clone();
            acxs_manager::session_recorder::spawn(Arc::clone(&recorder), move || ctx.request_repaint());
        }

        let gl_renderer = cc
            .wgpu_render_state
            .as_ref()
            .and_then(|rs| viewport3d::GlRenderer::new(rs).ok())
            .map(|r| Arc::new(Mutex::new(r)));

        let (tx, rx) = mpsc::channel();

        // Start single-instance IPC listener (no-op on non-Windows).
        let ipc_tx = tx.clone();
        let ipc_ctx = cc.egui_ctx.clone();
        let ipc_listener = ipc::start_listener(move |msg| {
            let sent = match msg {
                IpcMessage::Focus => ipc_tx.send(Msg::FocusWindow),
                IpcMessage::OpenUri(uri) => ipc_tx.send(Msg::OpenUri(uri)),
            };
            if sent.is_ok() {
                ipc_ctx.request_repaint();
            }
        });

        let pinned = App::load_legacy_pins();
        let mut app = App {
            ac_input: settings.ac_root.clone(),
            library_input: settings.patch_library.clone(),
            settings,
            recorder: Arc::clone(&recorder),
            page: Page::Home,
            last_synced_page: Page::Home,
            route: Route::Paddock(PaddockTab::Hub),
            nav_state: NavState::default(),
            focus: FocusNav::default(),
            car_query: String::new(),
            track_query: String::new(),
            car_skin: None,
            track_layout: None,
            selected_car: None,
            selected_track: None,
            confirm_uninstall: false,
            snapshot: Snapshot::default(),
            busy: false,
            pending: 0,
            generation: 0,
            ctx: cc.egui_ctx.clone(),
            tools: tools::registry(),
            favorites: App::load_favorites(),
            showroom_car: None,
            home_pick: None,
            car_filter: "All".to_string(),
            car_sort: 0,
            palette_open: false,
            palette_query: String::new(),
            palette_sel: 0,
            toasts: Vec::new(),
            servers: servers::Servers::load(),
            grid_columns: 1,
            race_request: None,
            picker: None,
            window: window_state::Tracker::new(),
            pinned,
            gl_renderer,
            _ipc: Some(ipc_listener),
            quick_switches_open: false,
            tx,
            rx,
        };

        if let Some(uri) = initial_uri {
            let _ = app.tx.send(Msg::OpenUri(uri));
        }

        app.refresh();
        app.servers.start_refresh(&app.tx, &app.ctx);
        start_game_watch(app.tx.clone(), app.ctx.clone());
        app
    }

    fn library(&self) -> PathBuf {
        self.settings.library_path()
    }

    fn push_notice(&mut self, text: impl Into<String>, ok: bool) {
        self.toasts.push(Toast { text: text.into(), ok, born: Instant::now() });
    }

    fn spawn(&mut self, work: impl FnOnce(Option<PathBuf>, PathBuf) -> Option<Notice> + Send + 'static) {
        self.busy = true;
        self.pending += 1;
        self.generation += 1;
        let generation = self.generation;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let ac_root = self.settings.ac_root_path();
        let library = self.library();
        thread::spawn(move || {
            if let Some(notice) = work(ac_root.clone(), library.clone()) {
                let _ = tx.send(Msg::Notice(notice));
            }
            let _ = tx.send(Msg::Snapshot(generation, gather(ac_root, library)));
            ctx.request_repaint();
        });
    }

    pub fn refresh(&mut self) {
        self.spawn(|_, _| None);
    }

    /// Navigate to a [`Route`], remembering the pillar's active tab and syncing `self.page`.
    pub fn set_route(&mut self, route: Route) {
        self.route = route;
        self.nav_state.remember(route);
        self.page = self.route_to_page(route);
        self.last_synced_page = self.page;
    }

    fn route_to_page(&self, route: Route) -> Page {
        match route {
            Route::Paddock(PaddockTab::Hub) => Page::Home,
            Route::Garage(GarageTab::Showroom) => Page::Showroom,
            Route::Garage(GarageTab::Cars(CarSubView::Browser)) => Page::Cars,
            Route::Garage(GarageTab::Tracks(TrackSubView::Browser)) => Page::Tracks,
            Route::Online(OnlineTab::Browser) => Page::Servers,
            Route::Setup(SetupTab::Csp(CspView::Installer)) => Page::Patches,
            Route::Setup(SetupTab::App) => Page::Settings,
            other => match other.tool_name() {
                Some(name) => Page::Tool(self.tool_index(name)),
                None => Page::Home,
            },
        }
    }

    fn sync_route_from_page(&mut self) {
        if self.page == self.last_synced_page {
            return;
        }
        let next = match self.page {
            Page::Home => Route::Paddock(PaddockTab::Hub),
            Page::Showroom => Route::Garage(GarageTab::Showroom),
            Page::Servers => Route::Online(OnlineTab::Browser),
            Page::Cars => Route::Garage(GarageTab::Cars(CarSubView::Browser)),
            Page::Tracks => Route::Garage(GarageTab::Tracks(TrackSubView::Browser)),
            Page::Patches => Route::Setup(SetupTab::Csp(CspView::Installer)),
            Page::Settings => Route::Setup(SetupTab::App),
            Page::Tool(i) => self
                .tools
                .get(i)
                .and_then(|t| Route::from_tool_name(t.name()))
                .unwrap_or(self.route),
        };
        self.route = next;
        self.nav_state.remember(next);
        self.last_synced_page = self.page;
    }

    fn handle_open_uri(&mut self, uri: JoinUri) {
        let key = format!("{}:{}", uri.host, uri.http_port);
        if !self.servers.entries.iter().any(|e| e.key() == key) {
            self.servers.entries.push(servers::Entry {
                host: uri.host.clone(),
                port: uri.http_port,
                favorite: false,
            });
        }
        self.servers.show(key.clone());
        self.set_route(Route::Online(OnlineTab::Browser));
        self.push_notice(format!("Opened server invite {key}"), true);
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Notice(notice) | Msg::DownloadFinished(notice) => self.push_notice(notice.text, notice.ok),
                Msg::GameRunning(running) => self.snapshot.game_running = running,
                Msg::Server(key, result) => self.servers.apply(key, result),
                Msg::Lobby(result) => self.servers.apply_lobby(result),
                Msg::LobbyPart { servers, total, last } => self.servers.apply_part(servers, total, last),
                Msg::OpenUri(uri) => self.handle_open_uri(uri),
                Msg::FocusWindow => self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
                Msg::BookingReady(key, outcome) => match outcome {
                    Ok(()) => self.push_notice(format!("Booked slot on {key}"), true),
                    Err(err) => self.push_notice(format!("Booking failed on {key}: {err}"), false),
                },
                Msg::Snapshot(generation, snapshot) => {
                    self.pending = self.pending.saturating_sub(1);
                    self.busy = self.pending > 0;
                    if generation < self.generation {
                        continue;
                    }
                    self.snapshot = snapshot;
                    let car_ids: Vec<&str> = self.snapshot.cars.iter().map(|c| c.id.as_str()).collect();
                    if self.selected_car.as_deref().is_some_and(|id| !car_ids.contains(&id)) {
                        self.selected_car = None;
                    }
                    let track_ids: Vec<&str> = self.snapshot.tracks.iter().map(|t| t.id.as_str()).collect();
                    if self.selected_track.as_deref().is_some_and(|id| !track_ids.contains(&id)) {
                        self.selected_track = None;
                    }
                    self.ensure_armed_session();
                }
            }
        }
    }

    fn ensure_armed_session(&mut self) {
        let armed = self.settings.armed_session.get_or_insert_with(ArmedSession::default);
        if armed.car_id.is_empty()
            && let Some(first) = self.snapshot.cars.first()
        {
            armed.car_id = first.id.clone();
        }
        if armed.track_id.is_empty()
            && let Some(first) = self.snapshot.tracks.first()
        {
            armed.track_id = first.id.clone();
        }
        if armed.mode.is_empty() {
            armed.mode = "Quick Race".into();
        }
    }

    fn save_paths(&mut self) {
        self.settings.ac_root = self.ac_input.trim().to_string();
        self.settings.patch_library = self.library_input.trim().to_string();
        match self.settings.save(&settings::settings_path()) {
            Ok(()) => self.push_notice("Settings saved.", true),
            Err(e) => self.push_notice(format!("Could not save settings: {e}"), false),
        }
        self.refresh();
    }

    fn save_settings_quiet(&self) {
        let _ = self.settings.save(&settings::settings_path());
    }

    fn install_patch(&mut self, patch: LightsPatch) {
        let label = format!("{} {}", patch.version, patch.family);
        self.spawn(move |ac_root, _| {
            let Some(ac_root) = ac_root else {
                return Some(Notice { text: "Set your Assetto Corsa folder first.".into(), ok: false });
            };
            let game_running = game::is_game_running();
            Some(match csp::install_patch(&ac_root, &patch, game_running) {
                Ok(result) if result.already_installed => {
                    Notice { text: format!("Lights patch {} is already installed.", result.version), ok: true }
                }
                Ok(result) => {
                    let archived = result
                        .archived_to
                        .map(|p| format!(" Previous files archived to {}.", p.display()))
                        .unwrap_or_default();
                    Notice { text: format!("Installed lights patch {}.{archived}", result.version), ok: true }
                }
                Err(e) => Notice { text: format!("Install failed: {e}"), ok: false },
            })
        });
        self.push_notice(format!("Installing {label}…"), true);
    }

    fn uninstall_patch(&mut self) {
        self.confirm_uninstall = false;
        self.spawn(|ac_root, _| {
            let Some(ac_root) = ac_root else {
                return Some(Notice { text: "Set your Assetto Corsa folder first.".into(), ok: false });
            };
            Some(match csp::uninstall_patch(&ac_root, game::is_game_running()) {
                Ok(Some(archive)) => Notice {
                    text: format!("Lights patch removed. Files archived to {}.", archive.display()),
                    ok: true,
                },
                Ok(None) => Notice { text: "No lights patch was installed.".into(), ok: true },
                Err(e) => Notice { text: format!("Uninstall failed: {e}"), ok: false },
            })
        });
    }

    fn launch_game(&mut self) {
        let Some(root) = self.settings.ac_root_path() else {
            self.push_notice("Set your Assetto Corsa folder first.", false);
            return;
        };
        match game::launch_game(&root) {
            Ok(_) => self.push_notice("Launching Assetto Corsa…", true),
            Err(e) => self.push_notice(format!("Could not launch: {e}"), false),
        }
    }

    fn tool_index(&self, name: &str) -> usize {
        self.tools.iter().position(|t| t.name() == name).unwrap_or(0)
    }

    fn favorites_path() -> PathBuf {
        let settings_file = settings::settings_path();
        settings_file.parent().unwrap_or(Path::new(".")).join("favorites.json")
    }

    fn load_favorites() -> HashSet<String> {
        std::fs::read_to_string(Self::favorites_path())
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .map(|list| list.into_iter().collect())
            .unwrap_or_default()
    }

    pub fn toggle_favorite(&mut self, id: &str) {
        if !self.favorites.remove(id) {
            self.favorites.insert(id.to_string());
        }
        let mut list: Vec<&String> = self.favorites.iter().collect();
        list.sort();
        if let Ok(text) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::write(Self::favorites_path(), text);
        }
    }

    pub fn pick_random_car(&mut self) {
        let len = self.snapshot.cars.len();
        if len == 0 {
            return;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        for step in 0..40 {
            let car = &self.snapshot.cars[(nanos / 7 + step * 31) % len];
            if media::car_preview(&car.path, None).is_some() {
                self.home_pick = Some(car.id.clone());
                return;
            }
        }
        self.home_pick = Some(self.snapshot.cars[nanos % len].id.clone());
    }

    /// Read-only migration of legacy `sidebar.json` pinned entries into `Ctrl+K`.
    fn load_legacy_pins() -> Vec<String> {
        let settings_file = settings::settings_path();
        let path = settings_file.parent().unwrap_or(Path::new(".")).join("sidebar.json");
        let value: serde_json::Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        value["pinned"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // AAA 6-Pillar Top Bar (56 px)
    // -----------------------------------------------------------------------
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let accent = theme::active_accent(ui.ctx());
        let mut current_pillar = self.route.pillar();
        let mut switched = false;

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;

            // Minimal ACXS motorsport emblem.
            let (badge_rect, badge_resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
            ui.painter().rect_filled(badge_rect, CornerRadius::same(8), accent);
            ui.painter().text(badge_rect.center(), Align2::CENTER_CENTER, "A", theme::display(20.0), theme::SURFACE_VOID);
            if badge_resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.set_route(Route::Paddock(PaddockTab::Hub));
            }
            ui.label(RichText::new("ACXS").font(theme::display(20.0)).color(Color32::WHITE));
            ui.add_space(6.0);

            // 6 Primary Pillars with animated sliding underline.
            let tabs: [(Pillar, &str, Option<&str>); 6] = [
                (Pillar::Paddock, "Paddock", None),
                (Pillar::Drive, "Drive", None),
                (Pillar::Garage, "Garage", None),
                (Pillar::Online, "Online", None),
                (Pillar::Telemetry, "Telemetry", None),
                (Pillar::Setup, "Setup", None),
            ];
            if widgets::animated_tabs(ui, "aaa_pillars", &mut current_pillar, &tabs) {
                switched = true;
            }

            // Right utility cluster: Quick Switches, Live Status dots, Rescan, Ctrl+K.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                // Quick Switches toggle button.
                let qs_btn = ui
                    .add(egui::Button::new(RichText::new("⚡").size(14.0)).min_size(vec2(32.0, 30.0)))
                    .on_hover_text("Quick Switches & Accent");
                if qs_btn.clicked() {
                    self.quick_switches_open = !self.quick_switches_open;
                }

                // Compact Ctrl+K command palette trigger.
                let (k_rect, k_resp) = ui.allocate_exact_size(vec2(138.0, 30.0), Sense::click());
                let k_hov = k_resp.hovered();
                ui.painter().rect_filled(k_rect, CornerRadius::same(8), theme::SURFACE_VOID);
                ui.painter().rect_stroke(
                    k_rect,
                    CornerRadius::same(8),
                    Stroke::new(1.0, if k_hov { accent.gamma_multiply(0.75) } else { theme::BORDER_SUBTLE }),
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    k_rect.left_center() + vec2(10.0, 0.0),
                    Align2::LEFT_CENTER,
                    "Search…",
                    egui::FontId::proportional(12.5),
                    theme::MUTED,
                );
                let chip = Rect::from_center_size(k_rect.right_center() - vec2(26.0, 0.0), vec2(40.0, 18.0));
                ui.painter().rect_filled(chip, CornerRadius::same(4), theme::SURFACE_RAISED);
                ui.painter().text(chip.center(), Align2::CENTER_CENTER, "Ctrl K", theme::mono(10.0), theme::MUTED);
                if k_resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.palette_open = true;
                    self.palette_query.clear();
                    self.palette_sel = 0;
                }

                // Rescan icon button.
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Rescan").min_size(vec2(0.0, 30.0)))
                    .clicked()
                {
                    self.refresh();
                }

                // Compact live status dot cluster (Game / CSP / Recording).
                let recording = self.recorder.recording.load(std::sync::atomic::Ordering::Relaxed);
                if recording {
                    widgets::status_pill(ui, "REC", theme::DANGER);
                }
                let (csp_text, csp_color) = match (&self.snapshot.installed, self.snapshot.installed_csp_build) {
                    (None, _) => ("CSP off".to_string(), theme::FAINT),
                    (Some(v), Some(b)) if v == crate::csp::UNRECOGNIZED => (format!("CSP #{b}"), theme::INFO),
                    (Some(v), _) if v == crate::csp::UNRECOGNIZED => ("CSP custom".to_string(), theme::WARN),
                    (Some(v), _) => (format!("CSP {v}"), theme::INFO),
                };
                widgets::status_pill(ui, &csp_text, csp_color);
                let (game_text, game_color) =
                    if self.snapshot.game_running { ("LIVE", theme::OK) } else { ("IDLE", theme::FAINT) };
                widgets::status_pill(ui, game_text, game_color);
            });
        });

        if switched {
            let next = self.nav_state.route_for(current_pillar);
            self.set_route(next);
        }
    }

    /// Floating Quick Switches & Accent Preset popover anchored below the Top Bar.
    fn quick_switches_popover(&mut self, ctx: &egui::Context) {
        if !self.quick_switches_open {
            return;
        }
        let mut changed = false;
        let mut close = false;
        let area = egui::Area::new(egui::Id::unique("quick_switches_pop"))
            .anchor(Align2::RIGHT_TOP, vec2(-18.0, 62.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                theme::card_glass().show(ui, |ui| {
                    ui.set_width(250.0);
                    ui.label(theme::micro_label("Quick Switches"));
                    ui.add_space(6.0);
                    for (label, flag) in [
                        ("VR Mode", &mut self.settings.quick_switches.vr_mode),
                        ("Show FPS", &mut self.settings.quick_switches.show_fps),
                        ("Auto-record sessions", &mut self.settings.quick_switches.auto_record),
                        ("Skip intro", &mut self.settings.quick_switches.skip_intro),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(label).size(13.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if widgets::toggle(ui, flag).changed() {
                                    changed = true;
                                }
                            });
                        });
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(theme::micro_label("Accent Theme"));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        for (preset, name) in [
                            (AccentPreset::MotorsportRed, "Racing"),
                            (AccentPreset::TelemetryCyan, "Cyan"),
                            (AccentPreset::AmberGold, "Amber"),
                            (AccentPreset::Emerald, "Emerald"),
                        ] {
                            let c = theme::accent_color(preset);
                            let selected = self.settings.accent_preset == preset;
                            let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                            ui.painter().circle_filled(r.center(), 10.0, c);
                            if selected {
                                ui.painter().circle_stroke(r.center(), 12.5, Stroke::new(2.0, Color32::WHITE));
                            }
                            if resp.on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                self.settings.accent_preset = preset;
                                theme::apply_accent(ctx, preset);
                                changed = true;
                            }
                        }
                    });
                });
            });
        let clicked_outside = ctx.input(|i| i.pointer.any_click())
            && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p));
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || clicked_outside {
            close = true;
        }
        if changed {
            self.save_settings_quiet();
        }
        if close {
            self.quick_switches_open = false;
        }
    }

    // -----------------------------------------------------------------------
    // Contextual Sub-Nav Strip (38 px)
    // -----------------------------------------------------------------------
    fn sub_nav(&mut self, ui: &mut egui::Ui) {
        let mut next: Option<Route> = None;
        let cars_badge = self.snapshot.ac_valid.then(|| self.snapshot.cars.len().to_string());
        let tracks_badge = self.snapshot.ac_valid.then(|| self.snapshot.tracks.len().to_string());

        ui.horizontal_centered(|ui| {
            match self.route {
                Route::Paddock(_) => {}
                Route::Drive(mut tab) => {
                    let tabs = [
                        (DriveTab::QuickRace, "Quick Race", None),
                        (DriveTab::Events, "Events", None),
                        (DriveTab::Championships, "Championships", None),
                        (DriveTab::Launcher, "Launcher", None),
                    ];
                    if widgets::animated_tabs(ui, "sub_drive", &mut tab, &tabs) {
                        next = Some(Route::Drive(tab));
                    }
                }
                Route::Garage(tab) => {
                    #[derive(Clone, Copy, PartialEq, Eq)]
                    enum G { Showroom, Cars, Tracks, Setups, Studio, Library }
                    let mut g = match tab {
                        GarageTab::Showroom => G::Showroom,
                        GarageTab::Cars(_) => G::Cars,
                        GarageTab::Tracks(_) => G::Tracks,
                        GarageTab::Setups(_) => G::Setups,
                        GarageTab::Studio(_) => G::Studio,
                        GarageTab::Library(_) => G::Library,
                    };
                    let tabs = [
                        (G::Showroom, "Showroom", None),
                        (G::Cars, "Cars", cars_badge.as_deref()),
                        (G::Tracks, "Tracks", tracks_badge.as_deref()),
                        (G::Setups, "Setups", None),
                        (G::Studio, "Studio", None),
                        (G::Library, "Library", None),
                    ];
                    if widgets::animated_tabs(ui, "sub_garage", &mut g, &tabs) {
                        next = Some(Route::Garage(match g {
                            G::Showroom => GarageTab::Showroom,
                            G::Cars => GarageTab::Cars(CarSubView::Browser),
                            G::Tracks => GarageTab::Tracks(TrackSubView::Browser),
                            G::Setups => GarageTab::Setups(SetupsView::Editor),
                            G::Studio => GarageTab::Studio(StudioView::Skins),
                            G::Library => GarageTab::Library(LibraryView::Mods),
                        }));
                    }
                }
                Route::Online(mut tab) => {
                    let tabs = [
                        (OnlineTab::Browser, "Server Browser", None),
                        (OnlineTab::HostServer, "Dedicated Server", None),
                    ];
                    if widgets::animated_tabs(ui, "sub_online", &mut tab, &tabs) {
                        next = Some(Route::Online(tab));
                    }
                }
                Route::Telemetry(mut tab) => {
                    let tabs = [
                        (TelemetryTab::Sessions, "Sessions", None),
                        (TelemetryTab::Live, "Live", None),
                        (TelemetryTab::Replay, "Replay", None),
                        (TelemetryTab::Trajectory, "Trajectory", None),
                        (TelemetryTab::LapTimes, "Lap Times", None),
                        (TelemetryTab::Timing, "Timing", None),
                        (TelemetryTab::Strategy, "Strategy", None),
                        (TelemetryTab::Engineer, "Engineer", None),
                        (TelemetryTab::Media, "Media", None),
                    ];
                    if widgets::animated_tabs(ui, "sub_telemetry", &mut tab, &tabs) {
                        next = Some(Route::Telemetry(tab));
                    }
                }
                Route::Setup(tab) => {
                    #[derive(Clone, Copy, PartialEq, Eq)]
                    enum S { GameSettings, Csp, WeatherFx, System, App }
                    let mut s = match tab {
                        SetupTab::GameSettings(_) => S::GameSettings,
                        SetupTab::Csp(_) => S::Csp,
                        SetupTab::WeatherFx => S::WeatherFx,
                        SetupTab::System(_) => S::System,
                        SetupTab::App => S::App,
                    };
                    let tabs = [
                        (S::GameSettings, "Game Settings", None),
                        (S::Csp, "CSP & Lights", None),
                        (S::WeatherFx, "WeatherFX", None),
                        (S::System, "System", None),
                        (S::App, "App", None),
                    ];
                    if widgets::animated_tabs(ui, "sub_setup", &mut s, &tabs) {
                        next = Some(Route::Setup(match s {
                            S::GameSettings => SetupTab::GameSettings(GameSettingsView::Settings),
                            S::Csp => SetupTab::Csp(CspView::Config),
                            S::WeatherFx => SetupTab::WeatherFx,
                            S::System => SetupTab::System(SystemView::InstallFinder),
                            S::App => SetupTab::App,
                        }));
                    }
                }
            }
        });

        if let Some(route) = next {
            self.set_route(route);
        }
    }

    /// Inline segmented tool selector rendered inside the page canvas ONLY for
    /// multi-tool workspaces (`Studio`, `Library`, `CSP & Lights`, `System`) — never a 3rd top bar.
    fn workspace_switcher(&mut self, ui: &mut egui::Ui) {
        let mut next: Option<Route> = None;
        let items: Vec<(Route, &'static str)> = match self.route {
            Route::Garage(GarageTab::Studio(_)) => vec![
                (Route::Garage(GarageTab::Studio(StudioView::Skins)), "Skins"),
                (Route::Garage(GarageTab::Studio(StudioView::LiveryPainter)), "Livery Painter"),
                (Route::Garage(GarageTab::Studio(StudioView::PlateDesigner)), "Plates"),
                (Route::Garage(GarageTab::Studio(StudioView::ModelTools)), "3D / AO"),
            ],
            Route::Garage(GarageTab::Library(_)) => vec![
                (Route::Garage(GarageTab::Library(LibraryView::Mods)), "Mods"),
                (Route::Garage(GarageTab::Library(LibraryView::Installer)), "Installer"),
                (Route::Garage(GarageTab::Library(LibraryView::Collections)), "Collections"),
                (Route::Garage(GarageTab::Library(LibraryView::Metadata)), "Metadata"),
                (Route::Garage(GarageTab::Library(LibraryView::Exports)), "Exports"),
            ],
            Route::Setup(SetupTab::Csp(_)) => vec![
                (Route::Setup(SetupTab::Csp(CspView::Config)), "CSP Config"),
                (Route::Setup(SetupTab::Csp(CspView::Installer)), "Lights Patch"),
                (Route::Setup(SetupTab::Csp(CspView::Lighting)), "Lighting"),
            ],
            Route::Setup(SetupTab::System(_)) => vec![
                (Route::Setup(SetupTab::System(SystemView::InstallFinder)), "Installs"),
                (Route::Setup(SetupTab::System(SystemView::Logs)), "Logs"),
            ],
            _ => return,
        };

        let accent = theme::active_accent(ui.ctx());
        egui::Frame::new()
            .fill(theme::SURFACE_ELEVATED)
            .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    for (route, label) in items {
                        let active = self.route == route;
                        let btn = egui::Button::new(
                            RichText::new(label)
                                .font(if active { theme::bold(12.5) } else { egui::FontId::proportional(12.5) })
                                .color(if active { Color32::WHITE } else { theme::MUTED }),
                        )
                        .fill(if active { accent.gamma_multiply(0.24) } else { Color32::TRANSPARENT })
                        .stroke(if active {
                            Stroke::new(1.0, accent.gamma_multiply(0.65))
                        } else {
                            Stroke::NONE
                        })
                        .corner_radius(CornerRadius::same(6))
                        .min_size(vec2(0.0, 26.0));
                        if ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !active {
                            next = Some(route);
                        }
                    }
                });
            });
        ui.add_space(10.0);

        if let Some(route) = next {
            self.set_route(route);
        }
    }

    // -----------------------------------------------------------------------
    // Persistent Bottom Paddock Dock (62 px)
    // -----------------------------------------------------------------------
    fn paddock_dock(&mut self, ui: &mut egui::Ui) {
        let armed = self.settings.armed_session.clone().unwrap_or_default();
        let car_label = self
            .snapshot
            .cars
            .iter()
            .find(|c| c.id == armed.car_id)
            .map(|c| c.name.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(if armed.car_id.is_empty() { "Select car" } else { armed.car_id.as_str() });
        let track_label = self
            .snapshot
            .tracks
            .iter()
            .find(|t| t.id == armed.track_id)
            .map(|t| t.name.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(if armed.track_id.is_empty() { "Select track" } else { armed.track_id.as_str() });
        let mode_label = if armed.mode.is_empty() { "Quick Race" } else { armed.mode.as_str() };

        let mut goto: Option<Route> = None;
        let mut launch = false;

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;

            // Armed car pill (click → Garage Cars).
            let car_btn = ui
                .add(
                    egui::Button::new(
                        RichText::new(format!(
                            "CAR  {}",
                            if armed.skin_id.is_empty() {
                                car_label.to_string()
                            } else {
                                format!("{car_label} · {}", armed.skin_id)
                            }
                        ))
                        .font(theme::bold(13.0)),
                    )
                    .fill(theme::SURFACE_RAISED)
                    .min_size(vec2(0.0, 36.0)),
                )
                .on_hover_text("Change armed car in Garage");
            if car_btn.clicked() {
                goto = Some(Route::Garage(GarageTab::Cars(CarSubView::Browser)));
            }

            // Armed track pill (click → Drive Quick Race).
            let track_btn = ui
                .add(
                    egui::Button::new(
                        RichText::new(format!(
                            "TRACK  {}",
                            if armed.layout_id.is_empty() {
                                track_label.to_string()
                            } else {
                                format!("{track_label} · {}", armed.layout_id)
                            }
                        ))
                        .font(theme::bold(13.0)),
                    )
                    .fill(theme::SURFACE_RAISED)
                    .min_size(vec2(0.0, 36.0)),
                )
                .on_hover_text("Configure race & conditions in Drive");
            if track_btn.clicked() {
                goto = Some(Route::Drive(DriveTab::QuickRace));
            }

            egui::Frame::new()
                .fill(theme::SURFACE_RAISED)
                .stroke(Stroke::new(1.0, theme::BORDER_ACTIVE))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(mode_label.to_uppercase())
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    );
                });

            // Compact live busy indicator (notices live solely in toasts to prevent duplicates).
            if self.busy {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new("Working…").size(12.5).color(theme::MUTED));
            }

            // Right: Single unambiguous DRIVE ▶ CTA.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_launch = self.snapshot.ac_valid && !self.snapshot.game_running;
                let cta_text = if self.snapshot.game_running {
                    "GAME RUNNING"
                } else {
                    match &armed.target {
                        LaunchTarget::QuickRace => "DRIVE  ▶",
                        LaunchTarget::OnlineServer { .. } => "REJOIN  ▶",
                    }
                };
                if widgets::cta_button(ui, cta_text, vec2(148.0, 40.0), can_launch).clicked() {
                    launch = true;
                }
            });
        });

        if let Some(route) = goto {
            self.set_route(route);
        }
        if launch {
            self.launch_game();
        }
    }

    fn tool_page(&mut self, ui: &mut egui::Ui, index: usize) {
        let ctx = tools::ToolContext {
            ac_root: self.settings.ac_root_path(),
            ac_valid: self.snapshot.ac_valid,
            game_running: self.snapshot.game_running,
            sessions_saved: self.recorder.saved.load(std::sync::atomic::Ordering::Relaxed),
            gl: self.gl_renderer.clone(),
        };
        let mut notices = Vec::new();
        if let Some(tool) = self.tools.get_mut(index) {
            let full = ui.available_rect_before_wrap();
            let width = full.width().min(tool.content_width());
            let rect = Rect::from_center_size(full.center(), vec2(width, full.height()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| tool.ui(ui, &ctx, &mut notices));
        } else {
            widgets::empty_state(ui, "This tool is not available.");
        }
        if let Some(name) = self.tools.get_mut(index).and_then(|tool| tool.take_goto()) {
            if let Some(route) = Route::from_tool_name(name) {
                self.set_route(route);
            } else {
                self.page = Page::Tool(self.tool_index(name));
                self.last_synced_page = self.page;
            }
        }
        let picked = self.tools.get_mut(index).and_then(|tool| tool.take_pick());
        if let Some(kind) = picked {
            self.picker = Some(kind);
            match kind {
                tools::PickKind::Car => self.set_route(Route::Garage(GarageTab::Cars(CarSubView::Browser))),
                tools::PickKind::Track => self.set_route(Route::Garage(GarageTab::Tracks(TrackSubView::Browser))),
            }
        }
        for notice in notices {
            self.push_notice(notice.text, notice.ok);
        }
    }

    // -----------------------------------------------------------------------
    // Ctrl+K Command Palette (Pillars, SubTabs, Tools, Legacy Pins, Cars, Tracks, Servers)
    // -----------------------------------------------------------------------
    fn palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }
        enum Do {
            Go(Route),
            Car(String),
            Track(String),
            Server(String),
            Launch,
            Rescan,
        }
        let query = self.palette_query.trim().to_lowercase();
        let hit = |text: &str| query.is_empty() || text.to_lowercase().contains(&query);
        let mut items: Vec<(String, String, Do)> = Vec::new();

        // Legacy pinned entries from `sidebar.json` appear first when query is empty.
        if query.is_empty() {
            for pin in &self.pinned {
                if let Some(route) = Route::from_tool_name(pin) {
                    items.push((pin.clone(), "Pinned".into(), Do::Go(route)));
                }
            }
        }

        for pillar in Pillar::ALL {
            if hit(pillar.label()) {
                items.push((pillar.label().into(), "Pillar".into(), Do::Go(self.nav_state.route_for(pillar))));
            }
        }
        if hit("launch assetto corsa") || hit("drive") {
            items.push(("Launch Assetto Corsa".into(), "Action".into(), Do::Launch));
        }
        if hit("rescan") {
            items.push(("Rescan content".into(), "Action".into(), Do::Rescan));
        }
        for route in Route::all_routes() {
            if hit(route.title()) || hit(route.pillar().label()) {
                let title = route.title().to_string();
                if !items.iter().any(|(l, _, _)| *l == title) {
                    items.push((title, format!("{} · Workspace", route.pillar().label()), Do::Go(route)));
                }
            }
        }
        for tool in &self.tools {
            if (hit(tool.name()) || hit(tool.group()))
                && !items.iter().any(|(l, _, _)| l == tool.name())
                && let Some(route) = Route::from_tool_name(tool.name())
            {
                items.push((tool.name().into(), format!("Tool · {}", tool.group()), Do::Go(route)));
            }
        }
        if query.len() >= 2 {
            for car in self
                .snapshot
                .cars
                .iter()
                .filter(|c| hit(&c.name) || hit(&c.brand) || hit(&c.id))
                .take(8)
            {
                items.push((car.name.clone(), format!("Car · {}", car.brand), Do::Car(car.id.clone())));
            }
            for track in self.snapshot.tracks.iter().filter(|t| hit(&t.name) || hit(&t.country)).take(5) {
                items.push((track.name.clone(), format!("Track · {}", track.country), Do::Track(track.id.clone())));
            }
            for (key, name, hint) in self.servers.search(&query, 6) {
                items.push((name, format!("Server · {hint}"), Do::Server(key)));
            }
        }
        items.truncate(14);
        self.palette_sel = self.palette_sel.min(items.len().saturating_sub(1));

        let (down, up, enter, escape) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if down && !items.is_empty() {
            self.palette_sel = (self.palette_sel + 1) % items.len();
        }
        if up && !items.is_empty() {
            self.palette_sel = (self.palette_sel + items.len() - 1) % items.len();
        }

        let screen = ctx.content_rect();
        let dim = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("palette_dim")));
        dim.rect_filled(screen, 0, Color32::from_black_alpha(160));

        let accent = theme::active_accent(ctx);
        let mut chosen: Option<usize> = None;
        let area = egui::Area::new(egui::Id::unique("palette"))
            .anchor(Align2::CENTER_TOP, vec2(0.0, 84.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                theme::card_glass().inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(580.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.palette_query)
                            .hint_text("Jump to pillars, tools, cars, tracks, servers…")
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::proportional(16.5))
                            .margin(Margin::symmetric(12, 10)),
                    );
                    edit.request_focus();
                    if edit.changed() {
                        self.palette_sel = 0;
                    }
                    ui.add_space(6.0);
                    if items.is_empty() {
                        ui.add_space(14.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new("Nothing matches.").color(theme::MUTED)));
                        ui.add_space(14.0);
                    }
                    for (i, (label, hint, _)) in items.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
                        if response.hovered() && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            self.palette_sel = i;
                        }
                        let selected = i == self.palette_sel;
                        if selected {
                            ui.painter().rect_filled(rect, CornerRadius::same(9), accent.gamma_multiply(0.16));
                            let bar = Rect::from_center_size(rect.left_center() + vec2(3.0, 0.0), vec2(3.0, 18.0));
                            ui.painter().rect_filled(bar, CornerRadius::same(2), accent);
                        }
                        ui.painter().text(
                            rect.left_center() + vec2(16.0, 0.0),
                            Align2::LEFT_CENTER,
                            label,
                            if selected { theme::bold(14.0) } else { egui::FontId::proportional(14.0) },
                            theme::TEXT,
                        );
                        ui.painter().text(
                            rect.right_center() - vec2(14.0, 0.0),
                            Align2::RIGHT_CENTER,
                            hint,
                            theme::mono(11.5),
                            theme::FAINT,
                        );
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            chosen = Some(i);
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("↑ ↓ move   Enter open   Esc close   Alt+1–6 switch pillars")
                            .size(11.5)
                            .color(theme::FAINT),
                    );
                });
            });

        let clicked_outside = ctx.input(|i| i.pointer.any_click())
            && ctx.pointer_interact_pos().is_some_and(|p| !area.response.rect.contains(p));
        if enter && chosen.is_none() && !items.is_empty() {
            chosen = Some(self.palette_sel);
        }
        if escape || clicked_outside {
            self.palette_open = false;
        }
        if let Some(i) = chosen {
            self.palette_open = false;
            match items.swap_remove(i).2 {
                Do::Go(route) => self.set_route(route),
                Do::Car(id) => {
                    self.showroom_car = Some(id.clone());
                    self.selected_car = Some(id);
                    self.car_skin = None;
                    self.set_route(Route::Garage(GarageTab::Showroom));
                }
                Do::Server(key) => {
                    self.servers.show(key);
                    self.set_route(Route::Online(OnlineTab::Browser));
                }
                Do::Track(id) => {
                    self.selected_track = Some(id);
                    self.track_layout = None;
                    self.set_route(Route::Garage(GarageTab::Tracks(TrackSubView::Browser)));
                }
                Do::Launch => self.launch_game(),
                Do::Rescan => self.refresh(),
            }
        }
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        const LIFE: f32 = 4.5;
        self.toasts.retain(|t| t.born.elapsed().as_secs_f32() < LIFE);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-22.0, -76.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    for toast in self.toasts.iter().rev().take(3) {
                        let age = toast.born.elapsed().as_secs_f32();
                        let alpha = ((LIFE - age) / 0.6).clamp(0.0, 1.0) * (age / 0.12).clamp(0.0, 1.0);
                        let color = if toast.ok { theme::OK } else { theme::DANGER };
                        egui::Frame::new()
                            .fill(theme::SURFACE_GLASS.gamma_multiply(alpha))
                            .stroke(Stroke::new(1.0, color.gamma_multiply(0.7 * alpha)))
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(Margin::symmetric(16, 12))
                            .show(ui, |ui| {
                                ui.set_max_width(380.0);
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                                    ui.painter().circle_filled(rect.center(), 4.0, color.gamma_multiply(alpha));
                                    ui.add(egui::Label::new(
                                        RichText::new(&toast.text).size(13.0).color(theme::TEXT.gamma_multiply(alpha)),
                                    ));
                                });
                            });
                    }
                });
            });
    }

    /// Universal drag-and-drop handler: `.zip`, `.7z`, `.tar.gz`, `.tgz`, `.rar`, folders, and `.ini` PP filters.
    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            let taken = self.tools.iter_mut().position(|tool| tool.on_drop(ctx, &dropped));
            match taken {
                Some(_) => {
                    self.set_route(Route::Garage(GarageTab::Library(LibraryView::Installer)));
                    self.push_notice("Package received. Inspecting contents…", true);
                }
                None => {
                    let supported = dropped.iter().any(|p| is_droppable_mod_path(p));
                    if supported {
                        self.set_route(Route::Garage(GarageTab::Library(LibraryView::Installer)));
                        self.push_notice("Opened Installer for dropped content.", true);
                    } else {
                        self.push_notice(
                            "Drop a .zip, .7z, .tar.gz, .rar, mod folder, or PP filter .ini to install.",
                            false,
                        );
                    }
                }
            }
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let accent = theme::active_accent(ctx);
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drop_overlay")));
            let screen = ctx.content_rect();
            painter.rect_filled(screen, 0, Color32::from_black_alpha(195));
            let frame = screen.shrink(36.0);
            painter.rect_stroke(frame, CornerRadius::same(22), Stroke::new(2.5, accent), StrokeKind::Inside);
            painter.text(
                frame.center() - vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                "DROP TO INSTALL",
                theme::display(38.0),
                Color32::WHITE,
            );
            painter.text(
                frame.center() + vec2(0.0, 24.0),
                Align2::CENTER_CENTER,
                "Supports .zip, .7z, .tar.gz, .rar, mod folders & PP filter .ini — verified before install",
                egui::FontId::proportional(14.5),
                theme::MUTED,
            );
        }
    }
}

fn is_droppable_mod_path(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    let lower = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    lower.ends_with(".zip")
        || lower.ends_with(".7z")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".rar")
        || lower.ends_with(".ini")
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        for tool in &mut self.tools {
            tool.tick();
        }
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            self.palette_open = !self.palette_open;
            self.palette_query.clear();
            self.palette_sel = 0;
        }
        if !self.palette_open
            && let Some(pillar) = self.focus.handle_global_keys(&ctx, self.route.pillar())
        {
            let next = self.nav_state.route_for(pillar);
            self.set_route(next);
        }

        self.handle_drops(&ctx);
        self.window.update(&ctx);

        let edge = Stroke::new(1.0, theme::BORDER_SUBTLE);

        // 1. 56 px Translucent AAA Top Bar.
        egui::Panel::top("aaa_topbar")
            .exact_size(56.0)
            .frame(egui::Frame::new().fill(theme::SURFACE_GLASS).stroke(edge).inner_margin(Margin::symmetric(20, 0)))
            .show(ui, |ui| self.top_bar(ui));

        // 2. 38 px Contextual Sub-Nav Strip (hidden on Paddock Hub for full-bleed hero impact).
        if self.route.pillar() != Pillar::Paddock {
            egui::Panel::top("aaa_subnav")
                .exact_size(38.0)
                .frame(egui::Frame::new().fill(theme::SURFACE_BASE).stroke(edge).inner_margin(Margin::symmetric(20, 0)))
                .show(ui, |ui| self.sub_nav(ui));
        }

        // 3. 62 px Persistent Bottom Paddock Dock.
        egui::Panel::bottom("paddock_dock")
            .exact_size(62.0)
            .frame(egui::Frame::new().fill(theme::SURFACE_GLASS).stroke(edge).inner_margin(Margin::symmetric(20, 0)))
            .show(ui, |ui| self.paddock_dock(ui));

        // 4. Full-width Central Canvas.
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE_BASE)
                    .inner_margin(Margin { left: 24, right: 24, top: 18, bottom: 14 }),
            )
            .show(ui, |ui| {
                self.workspace_switcher(ui);
                match self.page {
                    Page::Home => pages::home(self, ui),
                    Page::Showroom => showroom::showroom(self, ui),
                    Page::Servers => servers::page(self, ui),
                    Page::Cars => pages::cars(self, ui),
                    Page::Tracks => pages::tracks(self, ui),
                    Page::Patches => pages::patches(self, ui),
                    Page::Settings => pages::settings(self, ui),
                    Page::Tool(i) => self.tool_page(ui, i),
                }
            });

        // Sync if a legacy page (`pages.rs` / `showroom.rs`) mutated `self.page` directly.
        self.sync_route_from_page();

        if let Some(action) = self.race_request.take() {
            // Keep ArmedSession synchronized with user's car/track choice.
            let armed = self.settings.armed_session.get_or_insert_with(ArmedSession::default);
            if let Some(car) = &action.car {
                armed.car_id = car.clone();
                armed.skin_id = action.skin.clone().unwrap_or_default();
            }
            if let Some(track) = &action.track {
                armed.track_id = track.clone();
                armed.layout_id = action.layout.clone().unwrap_or_default();
            }
            armed.target = LaunchTarget::QuickRace;
            self.save_settings_quiet();

            let index = self.tool_index("Quick Race");
            if let Some(tool) = self.tools.get_mut(index) {
                tool.set_race(action.track.as_deref(), action.layout.as_deref(), action.car.as_deref(), action.skin.as_deref());
            }
            self.picker = None;
            self.set_route(Route::Drive(DriveTab::QuickRace));
        }

        self.quick_switches_popover(&ctx);
        self.palette(&ctx);
        self.toasts(&ctx);
    }

    fn on_exit(&mut self) {
        if let Some(renderer) = &self.gl_renderer
            && let Ok(mut guard) = renderer.lock()
        {
            guard.cache.clear();
        }
    }
}

#[cfg(test)]
mod route_walk_tests {
    use super::*;

    #[test]
    fn headless_route_walk_draws_all_pillars_and_tools() {
        let ctx = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = App::new(&creation);
        let mut frame = eframe::Frame::_new_kittest();
        let mut time = 0.0;

        // Every registered tool must resolve to a canonical Route (zero orphaned tools).
        for tool in &app.tools {
            assert!(
                Route::from_tool_name(tool.name()).is_some(),
                "tool {:?} missing Route mapping in nav.rs",
                tool.name()
            );
        }

        // Walk all 6 pillars and every route in Route::all_routes().
        for route in Route::all_routes() {
            app.set_route(route);
            time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1320.0, 820.0))),
                time: Some(time),
                ..egui::RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| eframe::App::ui(&mut app, ui, &mut frame));
            output.textures_delta.clear();
        }
    }
}

/// Draws the whole app to PNG with no window:
/// `ACXS_RENDER_DIR=<dir> cargo test --bin acxs-manager render_app -- --ignored`.
#[cfg(test)]
mod app_render_tests {
    use super::tools::render_tests::{Canvas, HEIGHT, WIDTH};
    use super::*;

    #[test]
    #[ignore = "writes pictures; needs ACXS_RENDER_DIR"]
    fn render_app() {
        let dir = PathBuf::from(std::env::var("ACXS_RENDER_DIR").expect("ACXS_RENDER_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let wanted: Vec<String> = std::env::var("ACXS_RENDER_PAGES")
            .map(|list| list.split(',').map(str::to_string).collect())
            .unwrap_or_default();
        let ctx = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = App::new(&creation);
        let mut frame = eframe::Frame::_new_kittest();
        let mut canvas = Canvas::default();
        let mut time = 0.0;
        let mut step = |app: &mut App, count: u32, shoot: Option<&str>| {
            for i in 0..count {
                time += 0.1;
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(WIDTH as f32, HEIGHT as f32))),
                    time: Some(time),
                    ..egui::RawInput::default()
                };
                let mut output = ctx.run_ui(input, |ui| eframe::App::ui(app, ui, &mut frame));
                canvas.update(&mut output.textures_delta);
                if let Some(name) = shoot.filter(|_| i + 1 == count) {
                    canvas.draw(&ctx, output.shapes);
                    canvas.save(&dir.join(format!("{}.png", name.replace([' ', '&'], "_"))));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        };
        step(&mut app, 200, None);
        let mut pages: Vec<(String, Page)> = [
            ("Home", Page::Home),
            ("Servers", Page::Servers),
            ("Cars", Page::Cars),
            ("Tracks", Page::Tracks),
            ("Showroom", Page::Showroom),
            ("Lights Patch", Page::Patches),
            ("Settings", Page::Settings),
        ]
        .into_iter()
        .map(|(name, page)| (name.to_string(), page))
        .collect();
        let tools: Vec<&'static str> = app.tools.iter().map(|t| t.name()).collect();
        pages.extend(tools.iter().enumerate().map(|(i, name)| (name.to_string(), Page::Tool(i))));
        for (name, page) in pages {
            if !wanted.is_empty() && !wanted.contains(&name) {
                continue;
            }
            app.page = page;
            app.sync_route_from_page();
            step(&mut app, 40, Some(&name));
        }
    }
}
