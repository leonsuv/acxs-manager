//! The egui front end. Heavy work (scanning, hashing, installing) runs on
//! background threads and reports back through a channel, so the window stays responsive.

mod pages;
mod tools;
mod theme;
mod widgets;

use crate::content::{scan_cars, scan_tracks, CarEntry, TrackEntry};
use crate::csp::{self, LightsPatch};
use crate::game;
use crate::settings::{self, Settings};
use eframe::egui::{self, Color32, RichText};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const MAX_NOTICES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Cars,
    Tracks,
    Patches,
    Tools,
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
}

/// How often the background thread checks whether the game is running.
const GAME_POLL: std::time::Duration = std::time::Duration::from_secs(2);

pub struct App {
    settings: Settings,
    page: Page,
    ac_input: String,
    library_input: String,
    car_query: String,
    track_query: String,
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
    active_tool: usize,
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
        let (tx, rx) = mpsc::channel();
        let mut app = App {
            ac_input: settings.ac_root.clone(),
            library_input: settings.patch_library.clone(),
            settings,
            page: Page::Home,
            car_query: String::new(),
            track_query: String::new(),
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
            active_tool: 0,
            tx,
            rx,
        };
        app.refresh();
        start_game_watch(app.tx.clone(), app.ctx.clone());
        app
    }

    fn library(&self) -> PathBuf {
        self.settings.library_path()
    }

    fn push_notice(&mut self, text: impl Into<String>, ok: bool) {
        self.notices.push_front(Notice { text: text.into(), ok });
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

    fn pick_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        ui.label(RichText::new("ACXS").size(26.0).strong().color(theme::ACCENT));
        ui.label(RichText::new("Assetto Corsa Manager").small().color(theme::MUTED));
        ui.add_space(20.0);

        let entries = [
            (Page::Home, "Home"),
            (Page::Cars, "Cars"),
            (Page::Tracks, "Tracks"),
            (Page::Patches, "CSP Lights Patch"),
            (Page::Tools, "Tools"),
        ];
        for (page, label) in entries {
            let selected = self.page == page;
            let text = RichText::new(label).size(15.0).color(if selected { theme::TEXT } else { theme::MUTED });
            let button = egui::Button::new(text)
                .fill(if selected { theme::ACCENT.gamma_multiply(0.18) } else { Color32::TRANSPARENT })
                .stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(ui.available_width(), 36.0));
            if ui.add(button).clicked() {
                self.page = page;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(12.0);
            let game_text = if self.snapshot.game_running { "running" } else { "not running" };
            widgets::pill(ui, &format!("Game: {game_text}"), theme::pill_color(game_text));
            let csp_text = match self.snapshot.installed.as_deref() {
                None => "CSP: none".to_string(),
                Some(crate::csp::UNRECOGNIZED) => "CSP: unrecognized".to_string(),
                Some(v) => format!("CSP: {v}"),
            };
            widgets::pill(ui, &csp_text, theme::INFO);
        });
    }

    fn tools_page(&mut self, ui: &mut egui::Ui) {
        if self.tools.is_empty() {
            widgets::heading(ui, "Tools", "Feature pages will appear here.");
            widgets::empty_state(ui, "No tool pages are registered yet.");
            return;
        }
        let ctx = tools::ToolContext {
            ac_root: self.settings.ac_root_path(),
            ac_valid: self.snapshot.ac_valid,
            game_running: self.snapshot.game_running,
        };
        let mut notices = Vec::new();
        egui::Panel::left("tool_list")
            .exact_size(200.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(10, 8)))
            .show(ui, |ui| {
                let mut group = "";
                for (i, tool) in self.tools.iter().enumerate() {
                    if tool.group() != group {
                        group = tool.group();
                        ui.add_space(8.0);
                        ui.label(RichText::new(group).small().color(theme::MUTED));
                    }
                    if ui.selectable_label(self.active_tool == i, tool.name()).clicked() {
                        self.active_tool = i;
                    }
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 0)))
            .show(ui, |ui| {
                if let Some(tool) = self.tools.get_mut(self.active_tool) {
                    tool.ui(ui, &ctx, &mut notices);
                }
            });
        for notice in notices {
            self.push_notice(notice.text, notice.ok);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.busy {
                ui.spinner();
                ui.label(RichText::new("Working…").color(theme::MUTED));
            }
            if let Some(notice) = self.notices.front().cloned() {
                let color = if notice.ok { theme::OK } else { theme::DANGER };
                ui.label(RichText::new(notice.text).color(color));
            } else {
                ui.label(RichText::new("Ready").color(theme::MUTED));
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        for tool in &mut self.tools {
            tool.tick();
        }

        egui::Panel::left("nav")
            .exact_size(210.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(14, 10)))
            .show(ui, |ui| self.nav(ui));

        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(egui::Margin::symmetric(18, 8)))
            .show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(22)))
            .show(ui, |ui| match self.page {
                Page::Home => pages::home(self, ui),
                Page::Cars => pages::cars(self, ui),
                Page::Tracks => pages::tracks(self, ui),
                Page::Patches => pages::patches(self, ui),
                Page::Tools => self.tools_page(ui),
            });
    }
}
