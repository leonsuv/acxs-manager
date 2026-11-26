//! Dedicated Server page: edits `server_cfg.ini` and `entry_list.ini` in the server cfg folder.
//!
//! All file IO runs on background threads. Results come back through a channel that is
//! polled at the start of each frame, and the worker calls `request_repaint` when it finishes.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, INFO, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::ini::Ini;
use acxs_manager::server::{
    Entry, RaceSessionCfg, RealismConfig, ServerConfig, SessionCfg, VotingPluginsConfig,
    WeatherSlot, list_preset_dirs, preset, preset_names, read_server_config, save_preset_dir,
    validate, validate_multi_instance_ports, write_entry_list, write_server_config,
};
use acxs_manager::server_csp::{
    CspExtraOptions, ExtraCfgYml, read_csp_extra_options, read_extra_cfg_yml,
    write_csp_extra_options, write_extra_cfg_yml,
};
use acxs_manager::server_process::{
    LaunchArgs, LogKind, ServerProcess, Status, find_server_executable, parse_log_line,
};
use acxs_manager::server_rcon::{AdminCmd, RconClient, ServerEvent};
use acxs_manager::server_status::{ServerStatus, StatusError, fetch};
use acxs_manager::web_dashboard::{Dashboard, RaceState, SpectatorCar};
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

const ENTRY_LIST: &str = "entry_list.ini";
/// The server reads `[CAR_0]` to `[CAR_254]`.
const MAX_ENTRIES: usize = 255;
const FIELD_WIDTH: f32 = 320.0;
const DEFAULT_STATUS_HOST: &str = "127.0.0.1";
/// Budget for one live status query, covering both HTTP requests.
const LIVE_TIMEOUT: Duration = Duration::from_secs(3);
const AUTO_REFRESH: Duration = Duration::from_secs(10);

/// Result of one live status query.
type LiveResult = Result<ServerStatus, StatusError>;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ServerPageTab {
    #[default]
    Config,
    CspExtra,
    Rcon,
}

#[derive(Clone, Default)]
struct RconDriver {
    car_id: u8,
    driver: String,
    model: String,
    guid: String,
    lap: u32,
    best_lap_ms: u32,
    last_lap_ms: u32,
    speed_kmh: f32,
    gear: i32,
    spline_pos: f32,
    connected: bool,
}

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    let (live_tx, live_rx) = mpsc::channel();
    let cfg = ServerConfig::default();
    Box::new(ServerPage {
        tab: ServerPageTab::Config,
        dir_input: String::new(),
        default_dir: None,
        cars_text: join_cars(&cfg.cars),
        cfg,
        entries: Vec::new(),
        problems: Vec::new(),
        busy: false,
        process: Arc::new(Mutex::new(ServerProcess::new())),
        status: Status::Stopped,
        log: Vec::new(),
        auto_scroll: true,
        confirm_stop: false,
        tx,
        rx,
        status_host: DEFAULT_STATUS_HOST.to_string(),
        status_port: None,
        live_auto: false,
        live_pending: false,
        live_at: None,
        live: None,
        live_tx,
        live_rx,
        preset_slug: String::new(),
        saved_presets: Vec::new(),
        csp_opts: CspExtraOptions::default(),
        extra_yml: ExtraCfgYml::default(),
        rcon_local_port: 12000,
        rcon_server_addr: "127.0.0.1:11000".to_string(),
        rcon_client: None,
        rcon_stop: Arc::new(AtomicBool::new(false)),
        rcon_rx: None,
        rcon_drivers: BTreeMap::new(),
        rcon_chat_log: Vec::new(),
        rcon_broadcast: String::new(),
        spectator_dash: None,
        spectator_port: 8787,
    })
}

struct ServerPage {
    tab: ServerPageTab,
    /// The server cfg folder, as typed by the user.
    dir_input: String,
    /// `<AC root>/server/cfg` for the current AC root, when one is set.
    default_dir: Option<PathBuf>,
    /// Values shown in the server form. `cfg.cars` is kept in sync through `cars_text`.
    cfg: ServerConfig,
    cars_text: String,
    entries: Vec<Entry>,
    /// Problems from the last Validate or refused Save, shown in WARN.
    problems: Vec<String>,
    busy: bool,
    /// The server child. Shared with the worker threads that start and stop it.
    process: Arc<Mutex<ServerProcess>>,
    /// Last status read from `process`. Kept for frames where a worker holds the lock.
    status: Status,
    /// Last output lines read from `process`, oldest first.
    log: Vec<String>,
    auto_scroll: bool,
    /// Must be ticked before the Stop button is enabled.
    confirm_stop: bool,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    /// Host for the live status query.
    status_host: String,
    /// Port typed by the user. `None` follows the HTTP port of the loaded config.
    status_port: Option<u16>,
    /// Re-runs the live status query every `AUTO_REFRESH`.
    live_auto: bool,
    /// True while a live status query is on its worker thread.
    live_pending: bool,
    /// When the last live status query started.
    live_at: Option<Instant>,
    /// Outcome of the last live status query.
    live: Option<LiveResult>,
    live_tx: Sender<LiveResult>,
    live_rx: Receiver<LiveResult>,
    /// Multi-instance preset slug input & discovered presets.
    preset_slug: String,
    saved_presets: Vec<String>,
    /// CSP `csp_extra_options.ini` and AssettoServer `extra_cfg.yml`.
    csp_opts: CspExtraOptions,
    extra_yml: ExtraCfgYml,
    /// Live RCON / AC Server UDP Plugin state.
    rcon_local_port: u16,
    rcon_server_addr: String,
    rcon_client: Option<Arc<RconClient>>,
    rcon_stop: Arc<AtomicBool>,
    rcon_rx: Option<Receiver<ServerEvent>>,
    rcon_drivers: BTreeMap<u8, RconDriver>,
    rcon_chat_log: Vec<String>,
    rcon_broadcast: String,
    /// Live Spectator Web Dashboard (`/spectator`).
    spectator_dash: Option<Dashboard>,
    spectator_port: u16,
}

/// Result of a background job, sent back to the page.
enum Done {
    Loaded(Result<Loaded, String>),
    Saved(Result<String, String>),
    Server(Result<String, String>),
}

struct Loaded {
    cfg: ServerConfig,
    entries: Vec<Entry>,
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    None,
    Start,
    Stop,
    Load,
    Validate,
    Save,
    SaveEntries,
    CopyContent,
    /// Fills the server form from a preset. Nothing is written until Save.
    Preset(&'static str),
}

impl Tool for ServerPage {
    fn name(&self) -> &'static str {
        "Dedicated Server"
    }

    fn content_width(&self) -> f32 {
        2400.0
    }

    fn group(&self) -> &'static str {
        "Server"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.follow_ac_root(ctx.ac_root.as_deref());
        self.poll(notices);
        self.poll_live();
        self.poll_rcon();
        self.refresh_process();

        let egui_ctx = ui.ctx().clone();
        if self.status == Status::Running || self.rcon_client.is_some() {
            egui_ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.live_tick(&egui_ctx);
        let mut action = Action::None;
        widgets::heading(
            ui,
            "Dedicated Server",
            "Configure acServer, CSP extra_cfg, multi-instance presets, and live RCON / Spectator.",
        );
        let rcon_badge = self.rcon_client.is_some().then_some("LIVE");
        widgets::animated_tabs(
            ui,
            "server_page_tabs",
            &mut self.tab,
            &[
                (ServerPageTab::Config, "Config & Process", None),
                (ServerPageTab::CspExtra, "CSP extra_cfg", None),
                (ServerPageTab::Rcon, "Live Admin / RCON", rcon_badge),
            ],
        );
        ui.add_space(8.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let wide = ui.available_width() >= 960.0;
            match self.tab {
                ServerPageTab::Config => {
                    self.folder_card(ui);
                    ui.add_space(10.0);
                    if wide {
                        let (left_w, right_w) = split_widths(ui, 0.52);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 16.0;
                            ui.allocate_ui_with_layout(
                                egui::vec2(left_w, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(left_w);
                                    self.server_card(ui, &mut action);
                                    self.extended_config_cards(ui, notices);
                                    self.problem_list(ui);
                                    ui.add_space(8.0);
                                    self.action_bar(ui, &mut action);
                                },
                            );
                            ui.allocate_ui_with_layout(
                                egui::vec2(right_w, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_width(right_w);
                                    self.process_card(ui, &mut action);
                                    ui.add_space(10.0);
                                    self.live_card(ui, &egui_ctx);
                                },
                            );
                        });
                    } else {
                        self.process_card(ui, &mut action);
                        ui.add_space(10.0);
                        self.live_card(ui, &egui_ctx);
                        ui.add_space(10.0);
                        self.server_card(ui, &mut action);
                        self.extended_config_cards(ui, notices);
                        self.problem_list(ui);
                        ui.add_space(8.0);
                        self.action_bar(ui, &mut action);
                    }
                    ui.add_space(14.0);
                    self.entries_card(ui, &mut action);
                }
                ServerPageTab::CspExtra => {
                    self.folder_card(ui);
                    ui.add_space(10.0);
                    self.csp_extra_tab(ui, wide, notices);
                }
                ServerPageTab::Rcon => {
                    self.rcon_tab(ui, wide, &egui_ctx, notices);
                }
            }
        });

        match action {
            Action::None => {}
            Action::Start => match ctx.ac_root.as_deref() {
                None => notices.push(fail("Set your Assetto Corsa folder in Settings first.")),
                Some(root) => match find_server_executable(root) {
                    Err(e) => notices.push(fail(format!("Cannot start the server: {e}."))),
                    Ok(exe) => match self.dir() {
                        None => notices.push(fail("Set the server folder first.")),
                        Some(dir) => {
                            let process = Arc::clone(&self.process);
                            self.spawn(&egui_ctx, move || Done::Server(start_server(&process, &exe, &dir)));
                        }
                    },
                },
            },
            Action::Stop => {
                self.confirm_stop = false;
                let process = Arc::clone(&self.process);
                self.spawn(&egui_ctx, move || Done::Server(stop_server(&process)));
            }
            Action::Load => match self.dir() {
                None => notices.push(fail("Set the server folder first.")),
                Some(dir) => self.spawn(&egui_ctx, move || Done::Loaded(load(&dir))),
            },
            Action::Preset(name) => self.apply_preset(name, notices),
            Action::Validate => {
                self.problems = validate(&self.current_config());
                if self.problems.is_empty() {
                    notices.push(ok("No problems found."));
                } else {
                    notices.push(fail(format!("{} problem(s) found.", self.problems.len())));
                }
            }
            Action::Save => match self.dir() {
                None => notices.push(fail("Set the server folder first.")),
                Some(dir) => {
                    let cfg = self.current_config();
                    let problems = validate(&cfg);
                    if problems.is_empty() {
                        self.problems.clear();
                        self.spawn(&egui_ctx, move || Done::Saved(save_config(&dir, &cfg)));
                    } else {
                        self.problems = problems;
                        notices.push(fail("Not saved: fix the problems listed above."));
                    }
                }
            },
            Action::CopyContent => match ctx.ac_root.as_deref() {
                None => notices.push(fail("Set your Assetto Corsa folder in Settings first.")),
                Some(root) => {
                    let cfg = self.current_config();
                    match acxs_manager::server::copy_content(root, &cfg.cars, &cfg.track, &cfg.config_track) {
                        Ok(report) if report.missing.is_empty() => {
                            notices.push(ok(format!("Copied {} files to the server content folder.", report.copied)));
                        }
                        Ok(report) => notices.push(fail(format!(
                            "Copied {} files. Not found: {}.",
                            report.copied,
                            report.missing.join(", ")
                        ))),
                        Err(err) => notices.push(fail(format!("Could not copy the content: {err}"))),
                    }
                }
            },
            Action::SaveEntries => match self.dir() {
                None => notices.push(fail("Set the server folder first.")),
                Some(_) if self.entries.is_empty() => {
                    notices.push(fail("Add at least one entry before saving the entry list."));
                }
                Some(dir) => {
                    if let Some(i) = self.entries.iter().position(|e| e.model.trim().is_empty()) {
                        notices.push(fail(format!("Entry {} has no model.", i + 1)));
                    } else {
                        let entries = self.entries.clone();
                        self.spawn(&egui_ctx, move || Done::Saved(save_entries(&dir, &entries)));
                    }
                }
            },
        }
    }
}

impl ServerPage {
    /// Keeps the folder field on `<AC root>/server/cfg` while the user has not typed their own path.
    fn follow_ac_root(&mut self, ac_root: Option<&Path>) {
        let default = ac_root.map(|root| root.join("server").join("cfg"));
        if default == self.default_dir {
            return;
        }
        let old_default = self.default_dir.as_ref().map(|d| d.display().to_string());
        if (self.dir_input.trim().is_empty() || old_default.as_deref() == Some(self.dir_input.as_str()))
            && let Some(dir) = &default
        {
            self.dir_input = dir.display().to_string();
        }
        self.default_dir = default;
    }

    /// The typed folder, or `None` when the field is empty.
    fn dir(&self) -> Option<PathBuf> {
        let typed = self.dir_input.trim();
        (!typed.is_empty()).then(|| PathBuf::from(typed))
    }

    /// The form values as they will be written: names trimmed, cars parsed from the text field.
    fn current_config(&self) -> ServerConfig {
        let mut cfg = self.cfg.clone();
        cfg.name = cfg.name.trim().to_string();
        cfg.track = cfg.track.trim().to_string();
        cfg.config_track = cfg.config_track.trim().to_string();
        cfg.cars = parse_cars(&self.cars_text);
        cfg
    }

    /// Copies a preset into the form. The typed passwords are kept, and nothing is saved.
    fn apply_preset(&mut self, name: &str, notices: &mut Vec<Notice>) {
        let Some(mut cfg) = preset(name) else {
            notices.push(fail(format!("Unknown preset {name}.")));
            return;
        };
        cfg.password = self.cfg.password.clone();
        cfg.admin_password = self.cfg.admin_password.clone();
        self.cars_text = join_cars(&cfg.cars);
        self.cfg = cfg;
        self.problems.clear();
        notices.push(ok(format!("Filled in the {name} preset. Save server config to write it.")));
    }

    /// Applies finished background jobs. Each one clears the busy flag and reports a notice.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(done) = self.rx.try_recv() {
            self.busy = false;
            match done {
                Done::Loaded(Ok(loaded)) => {
                    let count = loaded.entries.len();
                    self.cars_text = join_cars(&loaded.cfg.cars);
                    self.cfg = loaded.cfg;
                    self.entries = loaded.entries;
                    self.problems.clear();
                    notices.push(ok(format!("Loaded server_cfg.ini and {count} entries.")));
                }
                Done::Loaded(Err(e)) => notices.push(fail(e)),
                Done::Server(Ok(message)) => notices.push(ok(message)),
                Done::Server(Err(e)) => notices.push(fail(e)),
                Done::Saved(Ok(message)) => notices.push(ok(message)),
                Done::Saved(Err(e)) => notices.push(fail(e)),
            }
        }
    }

    /// Applies a finished live status query. Does not touch `busy`, so the form stays usable.
    fn poll_live(&mut self) {
        while let Ok(result) = self.live_rx.try_recv() {
            self.live_pending = false;
            self.live = Some(result);
        }
    }

    /// Starts a live status query when auto-refresh is on and the last one is at least
    /// `AUTO_REFRESH` old. Otherwise asks for a repaint when the next query is due.
    fn live_tick(&mut self, ctx: &egui::Context) {
        if !self.live_auto || self.live_pending {
            return;
        }
        let elapsed = self.live_at.map(|at| at.elapsed());
        match elapsed {
            Some(e) if e < AUTO_REFRESH => ctx.request_repaint_after(AUTO_REFRESH - e),
            _ => self.start_live(ctx),
        }
    }

    /// Runs one live status query on a worker thread. The result arrives through `self.live_rx`.
    fn start_live(&mut self, ctx: &egui::Context) {
        let host = self.status_host.trim().to_string();
        let port = self.status_port.unwrap_or(self.cfg.http_port);
        self.live_pending = true;
        self.live_at = Some(Instant::now());
        let tx = self.live_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(fetch(&host, port, LIVE_TIMEOUT));
            ctx.request_repaint();
        });
    }

    /// Runs `job` on a worker thread. The result arrives through `self.rx` on a later frame.
    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Done + Send + 'static) {
        self.busy = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }

    /// Copies the status and output from the server process. A frame where a worker holds the
    /// lock keeps the previous copy, so the UI never waits on the lock.
    fn refresh_process(&mut self) {
        let guard = match self.process.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        };
        if let Some(mut sp) = guard {
            self.status = sp.status();
            self.log = sp.recent_lines();
        }
        if self.status != Status::Running {
            self.confirm_stop = false;
        }
    }

    fn process_card(&mut self, ui: &mut egui::Ui, action: &mut Action) {
        widgets::section_title(ui, "Server process");
        let running = self.status == Status::Running;
        let busy = self.busy;
        let label = status_text(self.status);
        let pill_tint = status_color(self.status);
        let auto_scroll = self.auto_scroll;
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                widgets::pill(ui, &label, pill_tint);
                if busy {
                    ui.spinner();
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!busy && !running, widgets::primary_button("Start server")).clicked() {
                    *action = Action::Start;
                }
                ui.add_enabled(
                    running && !busy,
                    egui::Checkbox::new(&mut self.confirm_stop, "I want to stop the server"),
                );
                let stop = egui::Button::new(RichText::new("Stop server").color(DANGER));
                if ui.add_enabled(running && !busy && self.confirm_stop, stop).clicked() {
                    *action = Action::Stop;
                }
                ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            });
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("server_log")
                .max_height(320.0)
                .auto_shrink([false, false])
                .stick_to_bottom(auto_scroll)
                .show(ui, |ui| {
                    if self.log.is_empty() {
                        ui.label(RichText::new("No output yet.").color(MUTED));
                    }
                    for line in &self.log {
                        let tint = log_color(parse_log_line(line));
                        ui.label(RichText::new(line.as_str()).monospace().color(tint));
                    }
                });
        });
    }

    fn live_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        widgets::section_title(ui, "Live status");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut refresh = false;
            ui.horizontal_wrapped(|ui| {
                ui.label("Host");
                ui.add(egui::TextEdit::singleline(&mut self.status_host).desired_width(140.0));
                ui.label("Port");
                let mut port = self.status_port.unwrap_or(self.cfg.http_port);
                if ui.add(egui::DragValue::new(&mut port).range(1..=65535)).changed() {
                    self.status_port = Some(port);
                }
                if ui.add_enabled(!self.live_pending, egui::Button::new("Refresh")).clicked() {
                    refresh = true;
                }
                ui.checkbox(&mut self.live_auto, "Auto-refresh every 10 s");
                if self.live_pending {
                    ui.spinner();
                }
            });
            if refresh {
                self.start_live(ctx);
            }
            ui.add_space(8.0);
            match &self.live {
                None => {
                    ui.label(RichText::new("Not checked yet. Press Refresh or turn on auto-refresh.").color(MUTED));
                }
                Some(Err(e)) => {
                    widgets::pill(ui, "offline", MUTED);
                    ui.add_space(4.0);
                    ui.label(RichText::new(e.to_string()).small().color(WARN));
                }
                Some(Ok(status)) => {
                    let (label, tint) = if status.online { ("online", OK) } else { ("offline", MUTED) };
                    widgets::pill(ui, label, tint);
                    ui.add_space(6.0);
                    egui::Grid::new("live_fields")
                        .num_columns(2)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            row(ui, "Server name", |ui| {
                                ui.label(status.name.as_deref().unwrap_or("-"));
                            });
                            row(ui, "Track", |ui| {
                                ui.label(status.track.as_deref().unwrap_or("-"));
                            });
                            row(ui, "Clients", |ui| {
                                ui.label(status.clients.map_or("-".to_string(), |n| n.to_string()));
                            });
                            row(ui, "Max clients", |ui| {
                                ui.label(status.max_clients.map_or("-".to_string(), |n| n.to_string()));
                            });
                            row(ui, "Session", |ui| {
                                ui.label(status.session.as_deref().unwrap_or("-"));
                            });
                        });
                    ui.add_space(8.0);
                    if status.players.is_empty() {
                        ui.label(RichText::new("No players connected.").color(MUTED));
                    } else {
                        ui.label(RichText::new(format!("Players ({})", status.players.len())).color(MUTED));
                        egui::ScrollArea::vertical()
                            .id_salt("live_players")
                            .max_height(160.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for name in &status.players {
                                    ui.label(name.as_str());
                                }
                            });
                    }
                }
            }
        });
    }

    fn folder_card(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Server folder");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("The folder with server_cfg.ini and entry_list.ini. Default: <AC root>/server/cfg.")
                    .color(MUTED),
            );
            if self.default_dir.is_none() {
                ui.label(
                    RichText::new("Set your Assetto Corsa folder in Settings to get the default.").color(WARN),
                );
            }
            ui.horizontal_wrapped(|ui| {
                let width = (ui.available_width() - 130.0).max(160.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.dir_input)
                        .desired_width(width)
                        .hint_text("Not set"),
                );
                let has_default = self.default_dir.is_some();
                if ui.add_enabled(has_default, egui::Button::new("Use default")).clicked()
                    && let Some(dir) = &self.default_dir
                {
                    self.dir_input = dir.display().to_string();
                }
            });
        });
    }

    fn server_card(&mut self, ui: &mut egui::Ui, action: &mut Action) {
        widgets::section_title(ui, "Server settings");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let field_w = (ui.available_width() - 150.0).clamp(180.0, FIELD_WIDTH.max(440.0));
            let cfg = &mut self.cfg;
            let cars = &mut self.cars_text;
            egui::Grid::new("server_form")
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    row(ui, "Preset", |ui| {
                        egui::ComboBox::from_id_salt("server_preset")
                            .selected_text("Choose a starting point")
                            .show_ui(ui, |ui| {
                                for name in preset_names() {
                                    if ui.selectable_label(false, *name).clicked() {
                                        *action = Action::Preset(name);
                                    }
                                }
                            });
                    });
                    row(ui, "Server name", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut cfg.name).desired_width(field_w));
                    });
                    row(ui, "Password", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut cfg.password)
                                .desired_width(field_w)
                                .hint_text("Empty means no password"),
                        );
                    });
                    row(ui, "Admin password", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut cfg.admin_password)
                                .password(true)
                                .desired_width(field_w)
                                .hint_text("Empty lets anyone join as admin"),
                        );
                    });
                    row(ui, "Track", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut cfg.track).desired_width(field_w));
                    });
                    row(ui, "Track layout", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut cfg.config_track)
                                .desired_width(field_w)
                                .hint_text("Empty for tracks without layouts"),
                        );
                    });
                    row(ui, "Cars", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(cars)
                                .desired_width(field_w)
                                .hint_text("Comma-separated model ids"),
                        );
                    });
                    row(ui, "Max clients", |ui| {
                        ui.add(egui::DragValue::new(&mut cfg.max_clients).range(1..=255));
                    });
                    row(ui, "UDP port", |ui| {
                        ui.add(egui::DragValue::new(&mut cfg.udp_port).range(1..=65535));
                    });
                    row(ui, "TCP port", |ui| {
                        ui.add(egui::DragValue::new(&mut cfg.tcp_port).range(1..=65535));
                    });
                    row(ui, "HTTP port", |ui| {
                        ui.add(egui::DragValue::new(&mut cfg.http_port).range(1..=65535));
                    });
                    row(ui, "Sun angle", |ui| {
                        ui.add(egui::DragValue::new(&mut cfg.sun_angle));
                    });
                    row(ui, "Loop mode", |ui| {
                        ui.checkbox(&mut cfg.loop_mode, "Restart from the first session after the last");
                    });
                    row(ui, "Pickup mode", |ui| {
                        ui.checkbox(&mut cfg.pickup_mode, "Enabled");
                    });
                });
            if cfg.dynamic_track.is_some() {
                ui.add_space(6.0);
                ui.label(RichText::new("Dynamic track settings in the file are kept unchanged.").small().color(MUTED));
            }
        });
    }

    fn problem_list(&self, ui: &mut egui::Ui) {
        if self.problems.is_empty() {
            return;
        }
        ui.add_space(8.0);
        for problem in &self.problems {
            ui.label(RichText::new(format!("- {problem}")).color(WARN));
        }
    }

    fn action_bar(&self, ui: &mut egui::Ui, action: &mut Action) {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!self.busy, egui::Button::new("Load")).clicked() {
                *action = Action::Load;
            }
            if ui.button("Validate").clicked() {
                *action = Action::Validate;
            }
            if ui.add_enabled(!self.busy, widgets::primary_button("Save server config")).clicked() {
                *action = Action::Save;
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Copy content to server"))
                .on_hover_text("Copies the data the server checks clients against for these cars and this track.")
                .clicked()
            {
                *action = Action::CopyContent;
            }
            if self.busy {
                ui.spinner();
                ui.label(RichText::new("Working...").color(MUTED));
            }
        });
    }

    fn entries_card(&mut self, ui: &mut egui::Ui, action: &mut Action) {
        widgets::section_title(ui, "Entry list");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Each row becomes a [CAR_n] section. Save entry list replaces the file and keeps a backup.")
                    .color(MUTED),
            );
            if self.entries.is_empty() {
                ui.label(RichText::new("No entries. Add one, or Load to read entry_list.ini.").color(MUTED));
            } else {
                self.entry_rows(ui);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Add entry").clicked() {
                    self.entries.push(Entry::default());
                }
                if ui.add_enabled(!self.busy, widgets::primary_button("Save entry list")).clicked() {
                    *action = Action::SaveEntries;
                }
            });
        });
    }

    fn entry_rows(&mut self, ui: &mut egui::Ui) {
        let mut remove = None;
        egui::ScrollArea::horizontal()
            .id_salt("entry_rows")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("entry_grid")
                    .num_columns(8)
                    .spacing([8.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for head in ["Model", "Skin", "GUID", "Driver name", "Spectator", "Ballast", "Restrictor", ""] {
                            ui.label(RichText::new(head).small().color(MUTED));
                        }
                        ui.end_row();
                        for (i, entry) in self.entries.iter_mut().enumerate() {
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.model)
                                    .desired_width(150.0)
                                    .hint_text("bmw_m3_e30"),
                            );
                            ui.add(egui::TextEdit::singleline(&mut entry.skin).desired_width(90.0));
                            ui.add(egui::TextEdit::singleline(&mut entry.guid).desired_width(150.0));
                            ui.add(egui::TextEdit::singleline(&mut entry.driver_name).desired_width(140.0));
                            ui.checkbox(&mut entry.spectator, "");
                            ui.add(egui::DragValue::new(&mut entry.ballast_kg).range(0..=500).suffix(" kg"));
                            ui.add(egui::DragValue::new(&mut entry.restrictor).range(0..=100).suffix(" %"));
                            if ui.button("Remove").clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(i) = remove {
            self.entries.remove(i);
        }
    }

    fn presets_root() -> PathBuf {
        acxs_manager::settings::settings_path()
            .parent()
            .unwrap_or(Path::new("."))
            .join("server-presets")
    }

    fn extended_config_cards(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        ui.add_space(8.0);
        egui::CollapsingHeader::new(RichText::new("Sessions ([BOOKING] / [PRACTICE] / [QUALIFY] / [RACE])").font(theme::bold(14.0)))
            .id_salt("srv_sessions")
            .show(ui, |ui| {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut has_b = self.cfg.booking.is_some();
                    if ui.checkbox(&mut has_b, "Enable [BOOKING]").changed() {
                        self.cfg.booking = has_b.then(|| SessionCfg { name: "Booking".into(), time_min: 10, is_open: 1 });
                    }
                    if let Some(b) = &mut self.cfg.booking {
                        ui.horizontal(|ui| {
                            ui.label("Minutes");
                            ui.add(egui::DragValue::new(&mut b.time_min).range(1..=180));
                        });
                    }
                    let mut has_p = self.cfg.practice.is_some();
                    if ui.checkbox(&mut has_p, "Enable [PRACTICE]").changed() {
                        self.cfg.practice = has_p.then(|| SessionCfg { name: "Practice".into(), time_min: 20, is_open: 1 });
                    }
                    if let Some(p) = &mut self.cfg.practice {
                        ui.horizontal(|ui| {
                            ui.label("Minutes");
                            ui.add(egui::DragValue::new(&mut p.time_min).range(1..=720));
                        });
                    }
                    let mut has_q = self.cfg.qualify.is_some();
                    if ui.checkbox(&mut has_q, "Enable [QUALIFY]").changed() {
                        self.cfg.qualify = has_q.then(|| SessionCfg { name: "Qualify".into(), time_min: 15, is_open: 1 });
                    }
                    if let Some(q) = &mut self.cfg.qualify {
                        ui.horizontal(|ui| {
                            ui.label("Minutes");
                            ui.add(egui::DragValue::new(&mut q.time_min).range(1..=180));
                        });
                    }
                    let mut has_r = self.cfg.race.is_some();
                    if ui.checkbox(&mut has_r, "Enable [RACE]").changed() {
                        self.cfg.race = has_r.then(|| RaceSessionCfg {
                            name: "Race".into(),
                            laps: 10,
                            time_min: 0,
                            wait_time_sec: 60,
                            is_open: 1,
                            reversed_grid_positions: 0,
                        });
                    }
                    if let Some(r) = &mut self.cfg.race {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Laps");
                            ui.add(egui::DragValue::new(&mut r.laps).range(0..=999));
                            ui.label("Minutes");
                            ui.add(egui::DragValue::new(&mut r.time_min).range(0..=1440));
                            ui.label("Wait (s)");
                            ui.add(egui::DragValue::new(&mut r.wait_time_sec).range(0..=600));
                            ui.label("Reverse grid");
                            ui.add(egui::DragValue::new(&mut r.reversed_grid_positions).range(-1..=64));
                        });
                    }
                });
            });

        egui::CollapsingHeader::new(RichText::new(format!("Weather slots ({})", self.cfg.weather.len())).font(theme::bold(14.0)))
            .id_salt("srv_weather")
            .show(ui, |ui| {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if ui.button("Add [WEATHER_n] slot").clicked() {
                        self.cfg.weather.push(WeatherSlot::default());
                    }
                    let mut remove_idx = None;
                    for (i, w) in self.cfg.weather.iter_mut().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("WEATHER_{i}")).font(theme::mono(12.0)));
                            ui.add(egui::TextEdit::singleline(&mut w.graphics).desired_width(130.0));
                            ui.label("Amb °C");
                            ui.add(egui::DragValue::new(&mut w.base_temp_ambient).range(-10..=50));
                            ui.label("Road Δ°C");
                            ui.add(egui::DragValue::new(&mut w.base_temp_road).range(-10..=30));
                            if ui.small_button("Remove").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove_idx {
                        self.cfg.weather.remove(i);
                    }
                });
            });

        egui::CollapsingHeader::new(RichText::new("Realism, Assists & Voting / UDP Plugins").font(theme::bold(14.0)))
            .id_salt("srv_realism_voting")
            .show(ui, |ui| {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut has_realism = self.cfg.realism.is_some();
                    if ui.checkbox(&mut has_realism, "Manage Realism & Assists keys").changed() {
                        self.cfg.realism = has_realism.then(RealismConfig::default);
                    }
                    if let Some(r) = &mut self.cfg.realism {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("ABS (0/1/2)");
                            ui.add(egui::DragValue::new(&mut r.abs_allowed).range(0..=2));
                            ui.label("TC (0/1/2)");
                            ui.add(egui::DragValue::new(&mut r.tc_allowed).range(0..=2));
                            ui.label("Fuel %");
                            ui.add(egui::DragValue::new(&mut r.fuel_rate).range(0..=500));
                            ui.label("Damage %");
                            ui.add(egui::DragValue::new(&mut r.damage_multiplier).range(0..=100));
                            ui.label("Tyre wear %");
                            ui.add(egui::DragValue::new(&mut r.tyre_wear_rate).range(0..=500));
                            ui.checkbox(&mut r.tyre_blankets_allowed, "Tyre blankets");
                            ui.checkbox(&mut r.force_virtual_mirror, "Force mirror");
                        });
                    }
                    ui.add_space(6.0);
                    let mut has_vp = self.cfg.voting_plugins.is_some();
                    if ui.checkbox(&mut has_vp, "Manage Voting & UDP Plugin ports").changed() {
                        self.cfg.voting_plugins = has_vp.then(VotingPluginsConfig::default);
                    }
                    if let Some(vp) = &mut self.cfg.voting_plugins {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("UDP Plugin Local Port");
                            ui.add(egui::DragValue::new(&mut vp.udp_plugin_local_port).range(0..=65535));
                            ui.label("UDP Plugin Address");
                            ui.add(egui::TextEdit::singleline(&mut vp.udp_plugin_address).desired_width(160.0).hint_text("127.0.0.1:12000"));
                            ui.checkbox(&mut vp.register_to_lobby, "Register to lobby");
                        });
                    }
                });
            });

        egui::CollapsingHeader::new(RichText::new("Multi-Instance Presets & Port Validator").font(theme::bold(14.0)))
            .id_salt("srv_presets_multi")
            .show(ui, |ui| {
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let root = Self::presets_root();
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.preset_slug).hint_text("preset-name").desired_width(180.0));
                        if ui.button("Save Preset").clicked() {
                            let cfg = self.current_config();
                            match save_preset_dir(&root, &self.preset_slug, &cfg, &self.entries) {
                                Ok(path) => {
                                    self.saved_presets = list_preset_dirs(&root);
                                    notices.push(ok(format!("Saved preset to {}", path.display())));
                                }
                                Err(e) => notices.push(fail(format!("Could not save preset: {e}"))),
                            }
                        }
                        if ui.button("Refresh Presets").clicked() {
                            self.saved_presets = list_preset_dirs(&root);
                        }
                        if ui.button("Check Port Collisions").clicked() {
                            let presets = list_preset_dirs(&root);
                            let mut loaded_cfgs: Vec<(String, ServerConfig)> = Vec::new();
                            for slug in presets {
                                if let Ok(c) = read_server_config(&root.join(&slug)) {
                                    loaded_cfgs.push((slug, c));
                                }
                            }
                            let refs: Vec<(&str, &ServerConfig)> = loaded_cfgs.iter().map(|(s, c)| (s.as_str(), c)).collect();
                            let issues = validate_multi_instance_ports(&refs);
                            if issues.is_empty() {
                                notices.push(ok(format!("No port collisions across {} saved preset(s).", refs.len())));
                            } else {
                                for issue in issues {
                                    notices.push(fail(issue));
                                }
                            }
                        }
                    });
                    if !self.saved_presets.is_empty() {
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for slug in self.saved_presets.clone() {
                                if ui.button(format!("Load {slug}")).clicked() {
                                    let pdir = root.join(&slug);
                                    match load(&pdir) {
                                        Ok(loaded) => {
                                            self.cars_text = join_cars(&loaded.cfg.cars);
                                            self.cfg = loaded.cfg;
                                            self.entries = loaded.entries;
                                            notices.push(ok(format!("Loaded preset {slug}.")));
                                        }
                                        Err(e) => notices.push(fail(e)),
                                    }
                                }
                            }
                        });
                    }
                });
            });
    }

    fn csp_extra_tab(&mut self, ui: &mut egui::Ui, wide: bool, notices: &mut Vec<Notice>) {
        if wide {
            let (left_w, right_w) = split_widths(ui, 0.5);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(left_w);
                        self.csp_options_card(ui, notices);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(right_w);
                        self.extra_cfg_yml_card(ui, notices);
                    },
                );
            });
        } else {
            self.csp_options_card(ui, notices);
            ui.add_space(12.0);
            self.extra_cfg_yml_card(ui, notices);
        }
    }

    fn csp_options_card(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        widgets::section_title(ui, "CSP csp_extra_options.ini");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Load csp_extra_options.ini").clicked()
                    && let Some(dir) = self.dir()
                {
                    match read_csp_extra_options(&dir) {
                        Ok(o) => {
                            self.csp_opts = o;
                            notices.push(ok("Loaded csp_extra_options.ini."));
                        }
                        Err(e) => notices.push(fail(format!("Could not load csp_extra_options.ini: {e}"))),
                    }
                }
                if ui.add(widgets::primary_button("Save csp_extra_options.ini")).clicked()
                    && let Some(dir) = self.dir()
                {
                    match write_csp_extra_options(&dir, &self.csp_opts) {
                        Ok(()) => notices.push(ok("Saved csp_extra_options.ini (backup kept).")),
                        Err(e) => notices.push(fail(format!("Save failed: {e}"))),
                    }
                }
            });
            ui.add_space(6.0);
            ui.checkbox(&mut self.csp_opts.allow_wrong_way, "Allow wrong way");
            ui.checkbox(&mut self.csp_opts.enforce_back_to_pits_penalty, "Enforce back-to-pits penalty");
            ui.checkbox(&mut self.csp_opts.no_collisions, "Disable car-to-car collisions");
            ui.checkbox(&mut self.csp_opts.use_custom_weather, "Use custom WeatherFX");
            ui.checkbox(&mut self.csp_opts.force_rain, "Force rain");
            ui.horizontal_wrapped(|ui| {
                ui.label("Pit speed limit (km/h)");
                ui.add(egui::DragValue::new(&mut self.csp_opts.pits_speed_kmh).range(30..=160));
                ui.label("Min CSP build");
                ui.add(egui::DragValue::new(&mut self.csp_opts.minimum_csp_build).range(0..=99999));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Content URL");
                let url_w = (ui.available_width() - 100.0).clamp(180.0, 420.0);
                ui.add(egui::TextEdit::singleline(&mut self.csp_opts.content_url).desired_width(url_w));
            });
        });
    }

    fn extra_cfg_yml_card(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        widgets::section_title(ui, "AssettoServer extra_cfg.yml");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Load extra_cfg.yml").clicked()
                    && let Some(dir) = self.dir()
                {
                    match read_extra_cfg_yml(&dir) {
                        Ok(y) => {
                            self.extra_yml = y;
                            notices.push(ok("Loaded extra_cfg.yml."));
                        }
                        Err(e) => notices.push(fail(format!("Could not load extra_cfg.yml: {e}"))),
                    }
                }
                if ui.add(widgets::primary_button("Save extra_cfg.yml")).clicked()
                    && let Some(dir) = self.dir()
                {
                    match write_extra_cfg_yml(&dir, &self.extra_yml) {
                        Ok(()) => notices.push(ok("Saved extra_cfg.yml (backup kept).")),
                        Err(e) => notices.push(fail(format!("Save failed: {e}"))),
                    }
                }
            });
            ui.add_space(6.0);
            ui.checkbox(&mut self.extra_yml.enable_ai, "Enable AI traffic");
            ui.checkbox(&mut self.extra_yml.enable_client_messages, "Enable client messages");
            ui.checkbox(&mut self.extra_yml.enable_real_penalty, "Enable RealPenalty");
            ui.horizontal_wrapped(|ui| {
                ui.label("AI Max Density");
                ui.add(egui::DragValue::new(&mut self.extra_yml.ai_max_density).range(1..=200));
                ui.label("AI Max Speed (km/h)");
                ui.add(egui::DragValue::new(&mut self.extra_yml.ai_max_speed_kph).range(30..=350));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Content URL");
                let url_w = (ui.available_width() - 100.0).clamp(180.0, 420.0);
                ui.add(egui::TextEdit::singleline(&mut self.extra_yml.content_url).desired_width(url_w));
            });
        });
    }

    fn poll_rcon(&mut self) {
        let mut changed = false;
        if let Some(rx) = &self.rcon_rx {
            while let Ok(ev) = rx.try_recv() {
                changed = true;
                match ev {
                    ServerEvent::NewConnection(c) => {
                        self.rcon_drivers.insert(
                            c.car_id,
                            RconDriver {
                                car_id: c.car_id,
                                driver: c.driver_name.clone(),
                                model: c.car_model,
                                guid: c.driver_guid,
                                connected: true,
                                ..RconDriver::default()
                            },
                        );
                        self.rcon_chat_log.push(format!("+ {} joined (#{})", c.driver_name, c.car_id));
                    }
                    ServerEvent::ConnectionClosed(c) => {
                        if let Some(d) = self.rcon_drivers.get_mut(&c.car_id) {
                            d.connected = false;
                        }
                        self.rcon_chat_log.push(format!("- {} left (#{})", c.driver_name, c.car_id));
                    }
                    ServerEvent::CarInfo { car_id, is_connected, model, driver_name, driver_guid, .. } => {
                        let d = self.rcon_drivers.entry(car_id).or_default();
                        d.car_id = car_id;
                        d.connected = is_connected;
                        d.model = model;
                        d.driver = driver_name;
                        d.guid = driver_guid;
                    }
                    ServerEvent::CarUpdate { car_id, vel, gear, normalized_spline_pos, .. } => {
                        let d = self.rcon_drivers.entry(car_id).or_default();
                        d.car_id = car_id;
                        d.connected = true;
                        d.gear = i32::from(gear);
                        d.spline_pos = normalized_spline_pos;
                        d.speed_kmh = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt() * 3.6;
                    }
                    ServerEvent::LapCompleted { car_id, lap_time_ms, .. } => {
                        let d = self.rcon_drivers.entry(car_id).or_default();
                        d.lap = d.lap.saturating_add(1);
                        d.last_lap_ms = lap_time_ms;
                        if d.best_lap_ms == 0 || lap_time_ms < d.best_lap_ms {
                            d.best_lap_ms = lap_time_ms;
                        }
                    }
                    ServerEvent::Chat { car_id, message } => {
                        self.rcon_chat_log.push(format!("[#{car_id}] {message}"));
                    }
                    ServerEvent::CollisionWithCar { car_id, other_car_id, impact_speed, .. } => {
                        self.rcon_chat_log.push(format!("Collision #{car_id} <-> #{other_car_id} ({impact_speed:.1} km/h)"));
                    }
                    ServerEvent::Error(e) => {
                        self.rcon_chat_log.push(format!("Server error: {e}"));
                    }
                    _ => {}
                }
                if self.rcon_chat_log.len() > 100 {
                    self.rcon_chat_log.drain(0..self.rcon_chat_log.len() - 100);
                }
            }
        }
        if changed && let Some(dash) = &self.spectator_dash {
            let cars = self
                .rcon_drivers
                .values()
                .map(|d| SpectatorCar {
                    car_id: d.car_id,
                    driver: d.driver.clone(),
                    model: d.model.clone(),
                    x: d.spline_pos,
                    z: 0.5,
                    spline_pos: d.spline_pos,
                    speed_kmh: d.speed_kmh,
                    gear: d.gear,
                    lap: d.lap,
                    best_lap_ms: d.best_lap_ms,
                    last_lap_ms: d.last_lap_ms,
                    gap_ms: 0,
                    connected: d.connected,
                })
                .collect();
            dash.publish_race(RaceState {
                track: self.cfg.track.clone(),
                session: "Live".into(),
                elapsed_ms: 0,
                track_outline: Vec::new(),
                cars,
                recent_events: self.rcon_chat_log.iter().rev().take(8).cloned().collect(),
            });
        }
    }

    fn rcon_tab(&mut self, ui: &mut egui::Ui, wide: bool, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        if wide {
            let (left_w, right_w) = split_widths(ui, 0.56);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(left_w);
                        self.rcon_control_card(ui, ctx, notices);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(right_w);
                        self.spectator_card(ui, notices);
                        ui.add_space(10.0);
                        self.rcon_log_card(ui);
                    },
                );
            });
        } else {
            self.rcon_control_card(ui, ctx, notices);
            ui.add_space(12.0);
            self.spectator_card(ui, notices);
            ui.add_space(12.0);
            self.rcon_log_card(ui);
        }
    }

    fn rcon_control_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        widgets::section_title(ui, "Live Admin / RCON (AC Server UDP Plugin v4)");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let connected = self.rcon_client.is_some();
            ui.horizontal_wrapped(|ui| {
                ui.label("Local UDP Port");
                ui.add_enabled(!connected, egui::DragValue::new(&mut self.rcon_local_port).range(1024..=65535));
                ui.label("Server Plugin Addr");
                ui.add_enabled(!connected, egui::TextEdit::singleline(&mut self.rcon_server_addr).desired_width(160.0));
                if !connected {
                    if ui.add(widgets::primary_button("Connect RCON")).clicked() {
                        match self.rcon_server_addr.parse::<SocketAddr>() {
                            Ok(server_addr) => {
                                let local: SocketAddr = ([127, 0, 0, 1], self.rcon_local_port).into();
                                match RconClient::bind(local, server_addr, Duration::from_millis(250)) {
                                    Ok(client) => {
                                        let client = Arc::new(client);
                                        let _ = client.send_cmd(&AdminCmd::SetRealtimePosInterval(250));
                                        let _ = client.send_cmd(&AdminCmd::GetSessionInfo(-1));
                                        let stop = Arc::new(AtomicBool::new(false));
                                        let (tx, rx) = mpsc::channel();
                                        let c_bg = Arc::clone(&client);
                                        let s_bg = Arc::clone(&stop);
                                        let ctx_bg = ctx.clone();
                                        thread::spawn(move || {
                                            while !s_bg.load(Ordering::Acquire) {
                                                if let Ok(ev) = c_bg.recv_event() {
                                                    if tx.send(ev).is_err() {
                                                        break;
                                                    }
                                                    ctx_bg.request_repaint();
                                                }
                                            }
                                        });
                                        self.rcon_stop = stop;
                                        self.rcon_client = Some(client);
                                        self.rcon_rx = Some(rx);
                                        notices.push(ok("Connected RCON UDP plugin."));
                                    }
                                    Err(e) => notices.push(fail(format!("RCON bind failed: {e}"))),
                                }
                            }
                            Err(_) => notices.push(fail("Invalid server address (expected ip:port).")),
                        }
                    }
                } else if ui.button("Disconnect").clicked() {
                    self.rcon_stop.store(true, Ordering::Release);
                    self.rcon_client = None;
                    self.rcon_rx = None;
                }
            });

            if let Some(client) = self.rcon_client.clone() {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.rcon_broadcast).hint_text("Broadcast message to all drivers…").desired_width(280.0));
                    if ui.button("Broadcast").clicked() && !self.rcon_broadcast.trim().is_empty() {
                        let msg = std::mem::take(&mut self.rcon_broadcast);
                        let _ = client.send_cmd(&AdminCmd::BroadcastChat(msg));
                    }
                    if ui.button("Next Session").clicked() {
                        let _ = client.send_cmd(&AdminCmd::NextSession);
                    }
                    if ui.button("Restart Session").clicked() {
                        let _ = client.send_cmd(&AdminCmd::RestartSession);
                    }
                });
                ui.add_space(8.0);
                egui::Grid::new("rcon_drivers_grid").striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
                    for h in ["ID", "Driver", "Car", "Lap", "Speed", "Actions"] {
                        ui.label(RichText::new(h).font(theme::mono(11.5)).color(MUTED));
                    }
                    ui.end_row();
                    for d in self.rcon_drivers.values().filter(|d| d.connected) {
                        ui.label(RichText::new(format!("#{}", d.car_id)).font(theme::mono(12.0)));
                        ui.label(&d.driver);
                        ui.label(&d.model);
                        ui.label(format!("{}", d.lap));
                        ui.label(format!("{:.0} km/h", d.speed_kmh));
                        ui.horizontal(|ui| {
                            if ui.small_button("Kick").clicked() {
                                let _ = client.send_cmd(&AdminCmd::KickUser(d.car_id));
                            }
                            if ui.small_button("Ban").clicked() {
                                let _ = client.send_cmd(&AdminCmd::AdminCommand(format!("/ban_id {}", d.car_id)));
                            }
                            if ui.small_button("+20 kg").clicked() {
                                let _ = client.send_cmd(&AdminCmd::AdminCommand(format!("/ballast {} 20", d.car_id)));
                            }
                        });
                        ui.end_row();
                    }
                });
            }
        });
    }

    fn spectator_card(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        widgets::section_title(ui, "Live Spectator Web Dashboard (/spectator)");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if let Some(dash) = &self.spectator_dash {
                    let url = format!("http://{}/spectator", dash.local_addr());
                    widgets::pill(ui, "RUNNING", OK);
                    ui.label(RichText::new(&url).font(theme::mono(12.5)).color(INFO));
                    if ui.button("Copy URL").clicked() {
                        ui.ctx().copy_text(url);
                        notices.push(ok("Spectator URL copied."));
                    }
                    if ui.button("Stop Dashboard").clicked() {
                        self.spectator_dash = None;
                    }
                } else {
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut self.spectator_port).range(1024..=65535));
                    if ui.add(widgets::primary_button("Start Spectator Dashboard")).clicked() {
                        match Dashboard::start(acxs_manager::web_dashboard::DEFAULT_BIND, self.spectator_port) {
                            Ok(dash) => {
                                notices.push(ok(format!("Spectator live at http://{}/spectator", dash.local_addr())));
                                self.spectator_dash = Some(dash);
                            }
                            Err(e) => notices.push(fail(format!("Could not start dashboard: {e}"))),
                        }
                    }
                }
            });
        });
    }

    fn rcon_log_card(&self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Live RCON events & chat");
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("rcon_event_log")
                .max_height(260.0)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.rcon_chat_log.is_empty() {
                        ui.label(RichText::new("Connect RCON to stream driver joins, chat, and incidents.").color(MUTED));
                    }
                    for line in &self.rcon_chat_log {
                        ui.label(RichText::new(line.as_str()).font(theme::mono(12.0)).color(MUTED));
                    }
                });
        });
    }
}

/// Compute left and right column widths for a full-width 2-column split with a 16px gutter.
fn split_widths(ui: &egui::Ui, left_fraction: f32) -> (f32, f32) {
    let gap = 16.0;
    let total = ui.available_width();
    let left_w = ((total - gap) * left_fraction).floor().max(260.0);
    let right_w = (total - gap - left_w).max(260.0);
    (left_w, right_w)
}

/// One label/field pair in a two-column grid.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(label);
    add(ui);
    ui.end_row();
}

fn ok(text: impl Into<String>) -> Notice {
    Notice { text: text.into(), ok: true }
}

fn fail(text: impl Into<String>) -> Notice {
    Notice { text: text.into(), ok: false }
}

/// Text for the status pill.
fn status_text(status: Status) -> String {
    match status {
        Status::Running => "running".to_string(),
        Status::Stopped => "stopped".to_string(),
        Status::Exited(Some(code)) => format!("exited (code {code})"),
        Status::Exited(None) => "exited (signal)".to_string(),
    }
}

fn status_color(status: Status) -> Color32 {
    match status {
        Status::Running => OK,
        Status::Stopped => MUTED,
        Status::Exited(_) => DANGER,
    }
}

/// Error lines are red, warnings yellow, joins, leaves and laps blue. Other lines are muted.
fn log_color(kind: Option<LogKind>) -> Color32 {
    match kind {
        Some(LogKind::Error) => DANGER,
        Some(LogKind::Warning) => WARN,
        Some(LogKind::PlayerJoined | LogKind::PlayerLeft | LogKind::LapCompleted) => INFO,
        None => MUTED,
    }
}

/// Locks the server process, recovering the guard if an earlier holder panicked.
fn lock_process(process: &Mutex<ServerProcess>) -> MutexGuard<'_, ServerProcess> {
    process.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn start_server(process: &Mutex<ServerProcess>, exe: &Path, cfg_dir: &Path) -> Result<String, String> {
    lock_process(process)
        .start(exe, cfg_dir, &LaunchArgs::default())
        .map(|()| "Server started.".to_string())
        .map_err(|e| format!("Could not start the server: {e}"))
}

fn stop_server(process: &Mutex<ServerProcess>) -> Result<String, String> {
    lock_process(process)
        .stop()
        .map(|_| "Server stopped.".to_string())
        .map_err(|e| format!("Could not stop the server: {e}"))
}

fn join_cars(cars: &[String]) -> String {
    cars.join(", ")
}

/// Splits on commas or semicolons, trimming and dropping empty items.
fn parse_cars(text: &str) -> Vec<String> {
    text.split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Reads `server_cfg.ini` and `entry_list.ini`. A missing entry list is an empty list.
fn load(dir: &Path) -> Result<Loaded, String> {
    let cfg = read_server_config(dir).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            format!("No server_cfg.ini in {}.", dir.display())
        } else {
            format!("Could not read server_cfg.ini: {e}")
        }
    })?;
    let entries = read_entries(dir).map_err(|e| format!("Could not read entry_list.ini: {e}"))?;
    Ok(Loaded { cfg, entries })
}

fn read_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let path = dir.join(ENTRY_LIST);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let ini = Ini::load(&path)?;
    let sections = ini.section_names();
    let mut entries = Vec::new();
    for i in 0..MAX_ENTRIES {
        let section = format!("CAR_{i}");
        if !sections.iter().any(|s| s.eq_ignore_ascii_case(&section)) {
            continue;
        }
        let text = |key: &str| ini.get(&section, key).unwrap_or("").to_string();
        let number = |key: &str| {
            ini.get_i64(&section, key)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0)
        };
        entries.push(Entry {
            model: text("MODEL"),
            skin: text("SKIN"),
            guid: text("GUID"),
            driver_name: text("DRIVERNAME"),
            spectator: ini.get_i64(&section, "SPECTATOR_MODE").unwrap_or(0) != 0,
            ballast_kg: number("BALLAST"),
            restrictor: number("RESTRICTOR"),
        });
    }
    Ok(entries)
}

fn save_config(dir: &Path, cfg: &ServerConfig) -> Result<String, String> {
    write_server_config(dir, cfg)
        .map(|()| "Saved server_cfg.ini. The previous file is kept as a backup.".to_string())
        .map_err(|e| format!("Could not save server_cfg.ini: {e}"))
}

fn save_entries(dir: &Path, entries: &[Entry]) -> Result<String, String> {
    write_entry_list(dir, entries)
        .map(|()| format!("Saved {} entries to entry_list.ini.", entries.len()))
        .map_err(|e| format!("Could not save entry_list.ini: {e}"))
}
