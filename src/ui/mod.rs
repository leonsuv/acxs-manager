//! The egui front end. Heavy work (scanning, hashing, installing) runs on
//! background threads and reports back through a channel, so the window stays responsive.

mod media;
mod pages;
mod servers;
mod showroom;
mod tools;
mod theme;
mod widgets;
pub mod window_state;

use crate::content::{scan_cars, scan_tracks, CarEntry, TrackEntry};
use crate::csp::{self, LightsPatch};
use crate::game;
use crate::settings::{self, Settings};
use eframe::egui::{self, RichText};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

const MAX_NOTICES: usize = 8;

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
/// A notice that is also shown briefly in the corner.
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
    /// sha256 of the live dwrite.dll, used to tell which patch is active.
    pub installed_sha: Option<String>,
    pub patches: Vec<LightsPatch>,
    pub game_running: bool,
}

enum Msg {
    Notice(Notice),
    /// A rescan result, tagged with the generation of the request that made it.
    Snapshot(u64, Snapshot),
    /// The current answer of the periodic game check.
    GameRunning(bool),
    /// The answer of one server poll, keyed by `host:port`.
    Server(String, Result<(acxs_manager::server_status::ServerStatus, u128), String>),
    /// The public server list from the lobby.
    Lobby(Result<Vec<acxs_manager::lobby::LobbyServer>, String>),
    /// One page of the background refresh of the public list; `last` closes the refresh.
    LobbyPart { servers: Vec<acxs_manager::lobby::LobbyServer>, total: u64, last: bool },
}

/// How often the background thread checks whether the game is running.
const GAME_POLL: std::time::Duration = std::time::Duration::from_secs(2);

pub struct App {
    settings: Settings,
    recorder: Arc<acxs_manager::session_recorder::RecorderControl>,
    page: Page,
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
    /// Number of background jobs started by [`App::spawn`] that have not reported yet.
    pending: usize,
    /// Generation of the newest rescan request; older snapshots are dropped.
    generation: u64,
    notices: VecDeque<Notice>,
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
    /// Cards per row in the last grid drawn, for arrow-key movement.
    grid_columns: usize,
    /// Set by a detail panel; the Quick Race tool is filled in and opened after the frame.
    race_request: Option<pages::RaceAction>,
    /// Set while the Cars or Tracks page is choosing something for Quick Race.
    picker: Option<tools::PickKind>,
    window: window_state::Tracker,
    /// Entries pinned to the top of the sidebar, by name.
    pinned: Vec<String>,
    /// "More tools" is unfolded.
    more_open: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

/// Collect everything the UI shows. Runs on a background thread.
fn gather(ac_root: Option<PathBuf>, library: PathBuf) -> Snapshot {
    let game_running = game::is_game_running();
    let mut snapshot = Snapshot { game_running, ..Snapshot::default() };
    let mut roots = vec![library];
    if let Some(root) = ac_root.filter(|r| settings::is_ac_root(r)) {
        snapshot.ac_valid = true;
        snapshot.cars = scan_cars(&root);
        snapshot.tracks = scan_tracks(&root);
        snapshot.installed = csp::installed_version(&root).ok().flatten();
        snapshot.installed_sha = csp::sha256_file(&root.join(csp::DLL_NAME)).ok();
        roots.push(csp::archive_dir(&root));
    }
    snapshot.patches = csp::discover_patches(&roots);
    snapshot
}

/// Checks every [`GAME_POLL`] whether the game runs and reports changes, so the
/// "running" state the pages and tools rely on never goes stale.
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
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut settings = Settings::load(&settings::settings_path());
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
        let sidebar = App::load_sidebar();
        let (tx, rx) = mpsc::channel();
        let mut app = App {
            ac_input: settings.ac_root.clone(),
            library_input: settings.patch_library.clone(),
            settings,
            recorder: Arc::clone(&recorder),
            page: Page::Home,
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
            notices: VecDeque::new(),
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
            pinned: sidebar.0,
            more_open: sidebar.1,
            tx,
            rx,
        };
        app.refresh();
        // The public server list: the saved one is already loaded, this brings it up to date.
        app.servers.start_refresh(&app.tx, &app.ctx);

        start_game_watch(app.tx.clone(), app.ctx.clone());
        app
    }

    fn library(&self) -> PathBuf {
        self.settings.library_path()
    }

    fn push_notice(&mut self, text: impl Into<String>, ok: bool) {
        let text = text.into();
        self.toasts.push(Toast { text: text.clone(), ok, born: Instant::now() });
        self.notices.push_front(Notice { text, ok });
        self.notices.truncate(MAX_NOTICES);
    }

    /// Run `work` on a background thread, then rescan and send the new snapshot.
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

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Notice(notice) => self.push_notice(notice.text, notice.ok),
                Msg::GameRunning(running) => self.snapshot.game_running = running,
                Msg::Server(key, result) => self.servers.apply(key, result),
                Msg::Lobby(result) => self.servers.apply_lobby(result),
                Msg::LobbyPart { servers, total, last } => self.servers.apply_part(servers, total, last),
                Msg::Snapshot(generation, snapshot) => {
                    self.pending = self.pending.saturating_sub(1);
                    self.busy = self.pending > 0;
                    // A slower, older rescan must not replace a newer result.
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
                }
            }
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

    /// Tool indices grouped for the sidebar, in a fixed group order.
    fn tool_groups(&self) -> Vec<(&'static str, Vec<usize>)> {
        const ORDER: [&str; 10] =
            ["Content", "Car", "Setup", "Race", "Graphics", "Telemetry", "Config", "Launch", "Server", "Diagnostics"];
        let mut groups: Vec<(&'static str, Vec<usize>)> = Vec::new();
        for (i, tool) in self.tools.iter().enumerate() {
            match groups.iter_mut().find(|(g, _)| *g == tool.group()) {
                Some((_, list)) => list.push(i),
                None => groups.push((tool.group(), vec![i])),
            }
        }
        groups.sort_by_key(|(g, _)| ORDER.iter().position(|o| o == g).unwrap_or(ORDER.len()));
        groups
    }

    /// Index of the tool called `name`, or 0 when there is none.
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

    /// Pick a random car for the home banner.
    pub fn pick_random_car(&mut self) {
        let len = self.snapshot.cars.len();
        if len == 0 {
            return;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        // A few tries to land on a car that has a preview.
        for step in 0..40 {
            let car = &self.snapshot.cars[(nanos / 7 + step * 31) % len];
            if media::car_preview(&car.path, None).is_some() {
                self.home_pick = Some(car.id.clone());
                return;
            }
        }
        self.home_pick = Some(self.snapshot.cars[nanos % len].id.clone());
    }

    fn page_title(&self) -> (Option<&'static str>, String) {
        match self.page {
            Page::Home => (None, "Home".into()),
            Page::Showroom => (None, "Showroom".into()),
            Page::Servers => (None, "Servers".into()),
            Page::Cars => (None, "Cars".into()),
            Page::Tracks => (None, "Tracks".into()),
            Page::Patches => (None, "CSP Lights Patch".into()),
            Page::Settings => (None, "Settings".into()),
            Page::Tool(i) => match self.tools.get(i) {
                Some(tool) => (Some(tool.group()), tool.name().to_string()),
                None => (None, "Tools".into()),
            },
        }
    }

    fn sidebar_path() -> PathBuf {
        let settings_file = settings::settings_path();
        settings_file.parent().unwrap_or(Path::new(".")).join("sidebar.json")
    }

    /// The pinned entries and whether "More tools" was left open, from the last session.
    fn load_sidebar() -> (Vec<String>, bool) {
        let value: serde_json::Value = std::fs::read_to_string(Self::sidebar_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let pinned = value["pinned"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        (pinned, value["more_open"].as_bool().unwrap_or(false))
    }

    fn save_sidebar(&self) {
        let value = serde_json::json!({ "pinned": self.pinned, "more_open": self.more_open });
        let _ = std::fs::write(Self::sidebar_path(), value.to_string());
    }

    /// The Everyday sections of the sidebar. Everything else lives under "More tools".
    fn nav_sections() -> Vec<(&'static str, Vec<NavEntry>)> {
        use widgets::Icon;
        let page = |key, icon, page| NavEntry { key, icon, page: Some(page) };
        let tool = |key, icon| NavEntry { key, icon, page: None };
        vec![
            (
                "Play",
                vec![
                    page("Home", Icon::Home, Page::Home),
                    tool("Quick Race", Icon::Play),
                    tool("Events", Icon::Clock),
                    page("Servers", Icon::Globe, Page::Servers),
                ],
            ),
            (
                "Garage",
                vec![
                    page("Cars", Icon::Car, Page::Cars),
                    page("Tracks", Icon::Track, Page::Tracks),
                    page("Showroom", Icon::Camera, Page::Showroom),
                    tool("Skins", Icon::Paint),
                    tool("Setups", Icon::Sliders),
                    tool("Mods", Icon::Grid),
                ],
            ),
            (
                "Tuning",
                vec![
                    tool("CSP Config", Icon::Layers),
                    tool("Game Settings", Icon::Gear),
                    tool("Game Options", Icon::Layers),
                    page("Lights Patch", Icon::Bulb, Page::Patches),
                ],
            ),
            (
                "Driving data",
                vec![
                    tool("Lap Times", Icon::Clock),
                    tool("Sessions", Icon::Chart),
                    tool("Replay Viewer", Icon::Replay),
                    tool("Screenshots", Icon::Paint),
                    tool("Live Telemetry", Icon::Chart),
                ],
            ),
        ]
    }

    /// Where a sidebar entry leads, when it exists in this build.
    fn nav_target(&self, entry: &NavEntry) -> Option<Page> {
        entry
            .page
            .or_else(|| self.tools.iter().position(|t| t.name() == entry.key).map(Page::Tool))
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        // Brand.
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, egui::CornerRadius::same(10), theme::ACCENT);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "A", theme::bold(20.0), theme::BG);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("ACXS Manager").font(theme::bold(16.0)));
                ui.label(RichText::new("Assetto Corsa").size(11.5).color(theme::FAINT));
            });
        });
        ui.add_space(14.0);

        // Looks like a search field, opens the command palette.
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());
        let hovered = response.hovered();
        ui.painter().rect_filled(rect, egui::CornerRadius::same(9), theme::INPUT);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(9),
            egui::Stroke::new(1.0, if hovered { theme::ACCENT.gamma_multiply(0.7) } else { theme::BORDER }),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.left_center() + egui::vec2(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "Search or jump to…",
            egui::FontId::proportional(13.5),
            theme::MUTED,
        );
        let key = egui::Rect::from_center_size(rect.right_center() - egui::vec2(32.0, 0.0), egui::vec2(46.0, 20.0));
        ui.painter().rect_filled(key, egui::CornerRadius::same(5), theme::CARD_HOVER);
        ui.painter().text(key.center(), egui::Align2::CENTER_CENTER, "Ctrl K", egui::FontId::proportional(11.0), theme::MUTED);
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.palette_open = true;
            self.palette_query.clear();
            self.palette_sel = 0;
        }
        ui.add_space(4.0);

        let mut goto: Option<Page> = None;
        let mut toggle_pin: Option<String> = None;
        let mut toggle_more = false;

        // Settings and the version stay at the bottom, outside the scrolling list.
        egui::Panel::bottom("nav_footer")
            .frame(egui::Frame::new())
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.add_space(6.0);
                if widgets::nav_icon_item(ui, self.page == Page::Settings, widgets::Icon::Gear, "Settings", None).clicked() {
                    goto = Some(Page::Settings);
                }
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    let version = format!("v{}  ·  Check for updates", env!("CARGO_PKG_VERSION"));
                    let link = ui.add(egui::Label::new(RichText::new(version).size(11.5).color(theme::FAINT)).sense(egui::Sense::click()));
                    if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::unique(pages::UPDATE_RUN_ID), true));
                        goto = Some(Page::Settings);
                    }
                });
            });

        let sections = Self::nav_sections();
        let curated_tools: HashSet<&str> = sections
            .iter()
            .flat_map(|(_, entries)| entries.iter().filter(|e| e.page.is_none()).map(|e| e.key))
            .collect();
        let have = self.snapshot.ac_valid;
        let counts = [("Cars", self.snapshot.cars.len().to_string()), ("Tracks", self.snapshot.tracks.len().to_string())];
        let active_tool = match self.page {
            Page::Tool(i) => self.tools.get(i).map(|t| t.name()),
            _ => None,
        };

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // One entry: icon, label, count; right-click pins or unpins it.
            let mut entry_ui = |ui: &mut egui::Ui, this: &Self, entry: &NavEntry| {
                let Some(target) = this.nav_target(entry) else { return };
                let badge = counts.iter().find(|(k, _)| *k == entry.key && have).map(|(_, n)| n.as_str());
                let response = widgets::nav_icon_item(ui, this.page == target, entry.icon, entry.key, badge);
                let pinned = this.pinned.iter().any(|p| p == entry.key);
                let response = response.on_hover_text("Right-click to pin or unpin");
                if response.clicked() {
                    goto = Some(target);
                }
                response.context_menu(|ui| {
                    if ui.button(if pinned { "Unpin from the top" } else { "Pin to the top" }).clicked() {
                        toggle_pin = Some(entry.key.to_string());
                        ui.close();
                    }
                });
            };

            // Pinned entries, in the order they were pinned.
            let everyday: Vec<&NavEntry> = sections.iter().flat_map(|(_, e)| e.iter()).collect();
            let pinned_entries: Vec<NavEntry> = self
                .pinned
                .iter()
                .filter_map(|key| {
                    everyday.iter().find(|e| e.key == key.as_str()).map(|e| NavEntry { key: e.key, icon: e.icon, page: e.page }).or_else(|| {
                        self.tools.iter().find(|t| t.name() == key).map(|t| NavEntry { key: t.name(), icon: widgets::Icon::Pin, page: None })
                    })
                })
                .collect();
            if !pinned_entries.is_empty() {
                widgets::nav_section(ui, "Pinned");
                for entry in &pinned_entries {
                    entry_ui(ui, self, entry);
                }
            }

            for (title, entries) in &sections {
                widgets::nav_section(ui, title);
                for entry in entries {
                    entry_ui(ui, self, entry);
                }
            }

            // Everything else, folded away until wanted.
            let rest: Vec<(&'static str, Vec<usize>)> = self
                .tool_groups()
                .into_iter()
                .map(|(group, indices)| (group, indices.into_iter().filter(|i| !curated_tools.contains(self.tools[*i].name())).collect::<Vec<_>>()))
                .filter(|(_, indices)| !indices.is_empty())
                .collect();
            let rest_count: usize = rest.iter().map(|(_, i)| i.len()).sum();
            let holds_active = active_tool.is_some_and(|name| rest.iter().any(|(_, idx)| idx.iter().any(|i| self.tools[*i].name() == name)));
            let open = self.more_open || holds_active;
            ui.add_space(10.0);
            if widgets::nav_group(ui, open, "More tools", rest_count, holds_active) && !holds_active {
                toggle_more = true;
            }
            if open {
                for (group, indices) in &rest {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(RichText::new(group.to_uppercase()).font(theme::bold(10.5)).color(theme::FAINT));
                    });
                    for &i in indices {
                        let name = self.tools[i].name();
                        let response = widgets::nav_item(ui, active_tool == Some(name), name, None, 14.0)
                            .on_hover_text("Right-click to pin or unpin");
                        if response.clicked() {
                            goto = Some(Page::Tool(i));
                        }
                        let pinned = self.pinned.iter().any(|p| p == name);
                        response.context_menu(|ui| {
                            if ui.button(if pinned { "Unpin from the top" } else { "Pin to the top" }).clicked() {
                                toggle_pin = Some(name.to_string());
                                ui.close();
                            }
                        });
                    }
                }
            }
            ui.add_space(8.0);
        });

        if toggle_more {
            self.more_open = !self.more_open;
            self.save_sidebar();
        }
        if let Some(key) = toggle_pin {
            match self.pinned.iter().position(|p| *p == key) {
                Some(i) => {
                    self.pinned.remove(i);
                }
                None => self.pinned.push(key),
            }
            self.save_sidebar();
        }
        if let Some(page) = goto {
            self.page = page;
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let (group, title) = self.page_title();
        ui.horizontal_centered(|ui| {
            if let Some(group) = group {
                ui.label(RichText::new(format!("Tools  /  {group}  /")).color(theme::FAINT));
            }
            ui.label(RichText::new(title).font(theme::bold(15.0)));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Home has its own, larger launch button.
                if self.page != Page::Home {
                    let can_launch = self.snapshot.ac_valid && !self.snapshot.game_running;
                    if ui.add_enabled(can_launch, widgets::primary_button("Launch Assetto Corsa")).clicked() {
                        self.launch_game();
                    }
                }
                if ui.add_enabled(!self.busy, egui::Button::new("Rescan")).clicked() {
                    self.refresh();
                }
                ui.add_space(6.0);
                let (csp_text, csp_color) = match self.snapshot.installed.as_deref() {
                    None => ("CSP: none".to_string(), theme::FAINT),
                    Some(crate::csp::UNRECOGNIZED) => ("CSP: unrecognized".to_string(), theme::WARN),
                    Some(v) => (format!("CSP {v}"), theme::INFO),
                };
                widgets::status_pill(ui, &csp_text, csp_color);
                let (game_text, game_color) =
                    if self.snapshot.game_running { ("Game running", theme::OK) } else { ("Game closed", theme::FAINT) };
                widgets::status_pill(ui, game_text, game_color);
                if self.recorder.recording.load(std::sync::atomic::Ordering::Relaxed) {
                    widgets::status_pill(ui, "Recording session", theme::INFO);
                }
            });
        });
    }

    fn tool_page(&mut self, ui: &mut egui::Ui, index: usize) {
        let ctx = tools::ToolContext {
            ac_root: self.settings.ac_root_path(),
            ac_valid: self.snapshot.ac_valid,
            game_running: self.snapshot.game_running,
            sessions_saved: self.recorder.saved.load(std::sync::atomic::Ordering::Relaxed),
        };
        let mut notices = Vec::new();
        if let Some(tool) = self.tools.get_mut(index) {
            tool.ui(ui, &ctx, &mut notices);
        } else {
            widgets::empty_state(ui, "This tool is not available.");
        }
        if let Some(name) = self.tools.get_mut(index).and_then(|tool| tool.take_goto()) {
            self.page = Page::Tool(self.tool_index(name));
        }
        let picked = self.tools.get_mut(index).and_then(|tool| tool.take_pick());
        if let Some(kind) = picked {
            self.picker = Some(kind);
            self.page = match kind {
                tools::PickKind::Car => Page::Cars,
                tools::PickKind::Track => Page::Tracks,
            };
        }
        for notice in notices {
            self.push_notice(notice.text, notice.ok);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            if self.busy {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new("Working…").size(13.0).color(theme::MUTED));
            }
            let response = if let Some(notice) = self.notices.front().cloned() {
                let color = if notice.ok { theme::OK } else { theme::DANGER };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.5, color);
                ui.add(egui::Label::new(RichText::new(notice.text).size(13.0)).truncate())
            } else if self.busy {
                ui.label("")
            } else {
                ui.label(RichText::new("Ready").size(13.0).color(theme::MUTED))
            };
            if self.notices.len() > 1 {
                response.on_hover_ui(|ui| {
                    ui.label(RichText::new("Recent messages").font(theme::bold(12.5)).color(theme::MUTED));
                    for notice in &self.notices {
                        let color = if notice.ok { theme::TEXT } else { theme::DANGER };
                        ui.label(RichText::new(&notice.text).size(12.5).color(color));
                    }
                });
            }
        });
    }

    /// The Ctrl+K command palette: pages, tools, actions, cars and tracks in one list.
    #[allow(deprecated)]
    fn palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }
        enum Do {
            Go(Page),
            Car(String),
            Track(String),
            Server(String),
            Launch,
            Rescan,
        }
        let query = self.palette_query.trim().to_lowercase();
        let hit = |text: &str| query.is_empty() || text.to_lowercase().contains(&query);
        let mut items: Vec<(String, String, Do)> = Vec::new();
        for (page, name) in [
            (Page::Home, "Home"),
            (Page::Showroom, "Showroom"),
            (Page::Servers, "Servers"),
            (Page::Cars, "Cars"),
            (Page::Tracks, "Tracks"),
            (Page::Patches, "CSP Lights Patch"),
            (Page::Settings, "Settings"),
        ] {
            if hit(name) {
                items.push((name.into(), "Go to".into(), Do::Go(page)));
            }
        }
        if hit("launch assetto corsa") {
            items.push(("Launch Assetto Corsa".into(), "Action".into(), Do::Launch));
        }
        if hit("rescan") {
            items.push(("Rescan content".into(), "Action".into(), Do::Rescan));
        }
        for (i, tool) in self.tools.iter().enumerate() {
            if hit(tool.name()) || hit(tool.group()) {
                items.push((tool.name().into(), format!("Tool  ·  {}", tool.group()), Do::Go(Page::Tool(i))));
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
                items.push((car.name.clone(), format!("Car  ·  {}", car.brand), Do::Car(car.id.clone())));
            }
            for track in self.snapshot.tracks.iter().filter(|t| hit(&t.name) || hit(&t.country)).take(5) {
                items.push((track.name.clone(), format!("Track  ·  {}", track.country), Do::Track(track.id.clone())));
            }
            for (key, name, hint) in self.servers.search(&query, 6) {
                items.push((name, format!("Server  ·  {hint}"), Do::Server(key)));
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

        // Dim everything behind.
        let screen = ctx.content_rect();
        let dim = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("palette_dim")));
        dim.rect_filled(screen, 0, egui::Color32::from_black_alpha(150));

        let mut chosen: Option<usize> = None;
        let area = egui::Area::new(egui::Id::new("palette"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::CARD)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER_STRONG))
                    .corner_radius(egui::CornerRadius::same(16))
                    .shadow(ctx.global_style().visuals.popup_shadow)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(560.0);
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.palette_query)
                                .hint_text("Type to search tools, cars, tracks, servers…")
                                .desired_width(f32::INFINITY)
                                .font(egui::FontId::proportional(17.0))
                                .margin(egui::Margin::symmetric(12, 10)),
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
                            let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::click());
                            if response.hovered() && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                                self.palette_sel = i;
                            }
                            let selected = i == self.palette_sel;
                            if selected {
                                ui.painter().rect_filled(rect, egui::CornerRadius::same(9), theme::ACCENT.gamma_multiply(0.16));
                                let bar = egui::Rect::from_center_size(rect.left_center() + egui::vec2(3.0, 0.0), egui::vec2(3.0, 18.0));
                                ui.painter().rect_filled(bar, egui::CornerRadius::same(2), theme::ACCENT);
                            }
                            ui.painter().text(
                                rect.left_center() + egui::vec2(16.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                label,
                                if selected { theme::bold(14.5) } else { egui::FontId::proportional(14.5) },
                                theme::TEXT,
                            );
                            ui.painter().text(
                                rect.right_center() - egui::vec2(14.0, 0.0),
                                egui::Align2::RIGHT_CENTER,
                                hint,
                                egui::FontId::proportional(12.0),
                                theme::FAINT,
                            );
                            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                chosen = Some(i);
                            }
                        }
                        ui.add_space(4.0);
                        ui.label(RichText::new("↑ ↓ to move   Enter to open   Esc to close").size(11.5).color(theme::FAINT));
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
                Do::Go(page) => self.page = page,
                Do::Car(id) => {
                    self.showroom_car = Some(id.clone());
                    self.selected_car = Some(id);
                    self.car_skin = None;
                    self.page = Page::Showroom;
                }
                Do::Server(key) => {
                    self.servers.show(key);
                    self.page = Page::Servers;
                }
                Do::Track(id) => {
                    self.selected_track = Some(id);
                    self.track_layout = None;
                    self.page = Page::Tracks;
                }
                Do::Launch => self.launch_game(),
                Do::Rescan => self.refresh(),
            }
        }
    }

    /// Short-lived messages in the bottom-right corner. They fade out on their own.
    #[allow(deprecated)]
    fn toasts(&mut self, ctx: &egui::Context) {
        const LIFE: f32 = 4.5;
        self.toasts.retain(|t| t.born.elapsed().as_secs_f32() < LIFE);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-22.0, -46.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                    for toast in self.toasts.iter().rev().take(3) {
                        let age = toast.born.elapsed().as_secs_f32();
                        let alpha = ((LIFE - age) / 0.6).clamp(0.0, 1.0) * (age / 0.12).clamp(0.0, 1.0);
                        let color = if toast.ok { theme::OK } else { theme::DANGER };
                        egui::Frame::new()
                            .fill(theme::CARD.gamma_multiply(alpha))
                            .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7 * alpha)))
                            .corner_radius(egui::CornerRadius::same(12))
                            .inner_margin(egui::Margin::symmetric(16, 12))
                            .show(ui, |ui| {
                                ui.set_max_width(380.0);
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                    ui.painter().circle_filled(rect.center(), 4.0, color.gamma_multiply(alpha));
                                    ui.add(egui::Label::new(
                                        RichText::new(&toast.text).size(13.5).color(theme::TEXT.gamma_multiply(alpha)),
                                    ));
                                });
                            });
                    }
                });
            });
    }

    /// Dropped `.zip` packages go to the installer; a hint shows while files hover over the window.
    #[allow(deprecated)]
    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            let taken = self.tools.iter_mut().position(|tool| tool.on_drop(ctx, &dropped));
            match taken {
                Some(i) => {
                    self.page = Page::Tool(i);
                    self.push_notice("Package received. Checking it now…", true);
                }
                None => self.push_notice("Drop a car or track .zip here to install it.", false),
            }
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drop_overlay")));
            let screen = ctx.content_rect();
            painter.rect_filled(screen, 0, egui::Color32::from_black_alpha(190));
            let frame = screen.shrink(36.0);
            painter.rect_stroke(
                frame,
                egui::CornerRadius::same(22),
                egui::Stroke::new(2.5, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            painter.text(
                frame.center() - egui::vec2(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                "Drop to install",
                theme::bold(34.0),
                egui::Color32::WHITE,
            );
            painter.text(
                frame.center() + egui::vec2(0.0, 24.0),
                egui::Align2::CENTER_CENTER,
                "Car and track .zip packages are checked before anything is installed",
                egui::FontId::proportional(15.0),
                theme::MUTED,
            );
        }
    }
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
        self.handle_drops(&ctx);
        self.window.update(&ctx);

        let edge = egui::Stroke::new(1.0, theme::BORDER);
        egui::Panel::left("nav")
            .exact_size(244.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::SIDEBAR).stroke(edge).inner_margin(egui::Margin::symmetric(12, 14)))
            .show(ui, |ui| self.nav(ui));

        egui::Panel::top("header")
            .exact_size(54.0)
            .frame(egui::Frame::new().fill(theme::BG).stroke(edge).inner_margin(egui::Margin::symmetric(24, 0)))
            .show(ui, |ui| self.header(ui));

        egui::Panel::bottom("status")
            .exact_size(30.0)
            .frame(egui::Frame::new().fill(theme::SIDEBAR).stroke(edge).inner_margin(egui::Margin::symmetric(24, 0)))
            .show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin { left: 28, right: 28, top: 22, bottom: 14 }))
            .show(ui, |ui| match self.page {
                Page::Home => pages::home(self, ui),
                Page::Showroom => showroom::showroom(self, ui),
                Page::Servers => servers::page(self, ui),
                Page::Cars => pages::cars(self, ui),
                Page::Tracks => pages::tracks(self, ui),
                Page::Patches => pages::patches(self, ui),
                Page::Settings => pages::settings(self, ui),
                Page::Tool(i) => self.tool_page(ui, i),
            });

        if let Some(action) = self.race_request.take() {
            let index = self.tool_index("Quick Race");
            if let Some(tool) = self.tools.get_mut(index) {
                tool.set_race(action.track.as_deref(), action.layout.as_deref(), action.car.as_deref(), action.skin.as_deref());
            }
            self.picker = None;
            self.page = Page::Tool(index);
        }

        self.palette(&ctx);
        self.toasts(&ctx);
    }
}

/// One entry of the sidebar: its name, icon and where it goes (a tool when `page` is `None`).
struct NavEntry {
    key: &'static str,
    icon: widgets::Icon,
    page: Option<Page>,
}
