//! Dedicated Server page: edits `server_cfg.ini` and `entry_list.ini` in the server cfg folder.
//!
//! All file IO runs on background threads. Results come back through a channel that is
//! polled at the start of each frame, and the worker calls `request_repaint` when it finishes.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, INFO, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::ini::Ini;
use acxs_manager::server::{
    preset, preset_names, read_server_config, validate, write_entry_list, write_server_config, Entry,
    ServerConfig,
};
use acxs_manager::server_process::{
    find_server_executable, parse_log_line, LaunchArgs, LogKind, ServerProcess, Status,
};
use acxs_manager::server_status::{fetch, ServerStatus, StatusError};
use eframe::egui::{self, Color32, RichText};
use std::io;
use std::path::{Path, PathBuf};
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

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    let (live_tx, live_rx) = mpsc::channel();
    let cfg = ServerConfig::default();
    Box::new(ServerPage {
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
    })
}

struct ServerPage {
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
    /// Fills the server form from a preset. Nothing is written until Save.
    Preset(&'static str),
}

impl Tool for ServerPage {
    fn name(&self) -> &'static str {
        "Dedicated Server"
    }

    fn group(&self) -> &'static str {
        "Server"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.follow_ac_root(ctx.ac_root.as_deref());
        self.poll(notices);
        self.poll_live();
        self.refresh_process();

        let egui_ctx = ui.ctx().clone();
        if self.status == Status::Running {
            egui_ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.live_tick(&egui_ctx);
        let mut action = Action::None;
        widgets::heading(
            ui,
            "Dedicated Server",
            "Edit server_cfg.ini and entry_list.ini in the server cfg folder.",
        );
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.folder_card(ui);
            self.process_card(ui, &mut action);
            self.live_card(ui, &egui_ctx);
            self.server_card(ui, &mut action);
            self.problem_list(ui);
            ui.add_space(8.0);
            self.action_bar(ui, &mut action);
            ui.add_space(16.0);
            self.entries_card(ui, &mut action);
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
                .max_height(280.0)
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
                        ui.add(egui::TextEdit::singleline(&mut cfg.name).desired_width(FIELD_WIDTH));
                    });
                    row(ui, "Password", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut cfg.password)
                                .desired_width(FIELD_WIDTH)
                                .hint_text("Empty means no password"),
                        );
                    });
                    row(ui, "Admin password", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut cfg.admin_password)
                                .password(true)
                                .desired_width(FIELD_WIDTH)
                                .hint_text("Empty lets anyone join as admin"),
                        );
                    });
                    row(ui, "Track", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut cfg.track).desired_width(FIELD_WIDTH));
                    });
                    row(ui, "Track layout", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut cfg.config_track)
                                .desired_width(FIELD_WIDTH)
                                .hint_text("Empty for tracks without layouts"),
                        );
                    });
                    row(ui, "Cars", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(cars)
                                .desired_width(FIELD_WIDTH)
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
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.busy, egui::Button::new("Load")).clicked() {
                *action = Action::Load;
            }
            if ui.button("Validate").clicked() {
                *action = Action::Validate;
            }
            if ui.add_enabled(!self.busy, widgets::primary_button("Save server config")).clicked() {
                *action = Action::Save;
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
