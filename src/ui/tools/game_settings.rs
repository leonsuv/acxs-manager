//! Game Settings tool page: the `video.ini` and `assetto_corsa.ini` editors, and configuration snapshots.
//!
//! All file IO and the game check run on background threads. Results come back
//! over a channel and are applied in [`GameSettings::poll`] at the start of each frame.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use acxs_manager::backup::{self, Snapshot};
use acxs_manager::game;
use acxs_manager::game_cfg::{self, AssettoCorsaSettings, VideoSettings, ASSETTO_CORSA_FILE, VIDEO_FILE};
use eframe::egui::{self, RichText};

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, INFO, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};

/// Folder that holds the snapshots, under the Assetto Corsa root (see `acxs_manager::backup`).
const BACKUP_DIR: &str = "acxs-backups";
/// How many verification problems to list under a snapshot before summarizing.
const MAX_PROBLEMS_SHOWN: usize = 5;

/// Choices for the video drop-downs. Same lists as `acxs_manager::game_cfg` accepts.
const AA_CHOICES: &[u32] = &[1, 2, 4, 8];
const ANISOTROPIC_CHOICES: &[u32] = &[0, 2, 4, 8, 16];
const SHADOW_CHOICES: &[i32] = &[-1, 32, 64, 128, 256, 512, 1024, 2048, 3072, 4096, 6144, 8192];
const WORLD_DETAIL_CHOICES: &[u32] = &[0, 1, 2, 3, 4, 5];
/// Choices for `[SCREENSHOT] FORMAT`. Same list as `acxs_manager::game_cfg` accepts.
const SCREENSHOT_CHOICES: &[&str] = &["JPG", "BMP", "PNG"];

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(GameSettings {
        tx,
        rx,
        video_dir: default_video_dir(),
        video_requested: false,
        video_loaded: false,
        video_file_present: false,
        video_pending: 0,
        form: VideoForm::from_settings(&VideoSettings::default()),
        ac_dir: String::new(),
        ac_for_root: None,
        ac_loaded: false,
        ac_file_present: false,
        ac_pending: 0,
        ac_form: AcForm::from_settings(&AssettoCorsaSettings::default()),
        snapshots: Vec::new(),
        listed_root: None,
        snap_pending: 0,
        verify: HashMap::new(),
        restore_target: None,
        restore_confirmed: false,
    })
}

/// `video.ini` lives in `Documents/Assetto Corsa/cfg/` (see `game_cfg` module docs).
fn default_video_dir() -> String {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Documents").join("Assetto Corsa").join("cfg").display().to_string()
}

/// `assetto_corsa.ini` lives in `<AC root>/system/cfg/` (see `game_cfg` module docs).
fn default_ac_dir(ac_root: &Path) -> String {
    ac_root.join("system").join("cfg").display().to_string()
}

fn ok_note(text: impl Into<String>) -> Notice {
    Notice { text: text.into(), ok: true }
}

fn fail_note(text: impl Into<String>) -> Notice {
    Notice { text: text.into(), ok: false }
}

/// Results sent back from background threads.
enum Event {
    VideoLoaded { dir: String, present: bool, result: Result<VideoSettings, String> },
    VideoSaved(Result<(), String>),
    AcLoaded { dir: String, present: bool, result: Result<AssettoCorsaSettings, String> },
    AcSaved(Result<(), String>),
    SnapshotsListed(Vec<Snapshot>),
    SnapshotCreated(Result<Snapshot, String>),
    Verified { path: PathBuf, result: Result<Vec<String>, String> },
    Restored { name: String, result: Result<usize, String> },
}

#[derive(Clone)]
enum Verify {
    Checking,
    Done(Vec<String>),
    Failed(String),
}

/// Text and checkbox state of the video form.
#[derive(Clone)]
struct VideoForm {
    /// Empty means not set, which leaves the file's current value alone on save.
    width: String,
    height: String,
    fullscreen: bool,
    vsync: bool,
    /// Empty means no cap.
    fps_cap: String,
    aa_samples: u32,
    anisotropic: u32,
    shadow_map_size: i32,
    world_detail: u32,
    post_processing: bool,
}

impl VideoForm {
    fn from_settings(s: &VideoSettings) -> Self {
        VideoForm {
            width: s.width.map(|v| v.to_string()).unwrap_or_default(),
            height: s.height.map(|v| v.to_string()).unwrap_or_default(),
            fullscreen: s.fullscreen,
            vsync: s.vsync,
            fps_cap: s.fps_cap.map(|v| v.to_string()).unwrap_or_default(),
            aa_samples: s.aa_samples,
            anisotropic: s.anisotropic,
            shadow_map_size: s.shadow_map_size,
            world_detail: s.world_detail,
            post_processing: s.post_processing,
        }
    }

    /// The settings to save, or a message for each text field that does not parse.
    fn to_settings(&self) -> Result<VideoSettings, Vec<String>> {
        let mut problems = Vec::new();
        let width = parse_optional::<u32>(&self.width, "width", &mut problems);
        let height = parse_optional::<u32>(&self.height, "height", &mut problems);
        let fps_cap = parse_optional::<i32>(&self.fps_cap, "fps cap", &mut problems);
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(VideoSettings {
            width,
            height,
            fullscreen: self.fullscreen,
            vsync: self.vsync,
            fps_cap,
            aa_samples: self.aa_samples,
            anisotropic: self.anisotropic,
            shadow_map_size: self.shadow_map_size,
            world_detail: self.world_detail,
            post_processing: self.post_processing,
        })
    }
}

fn parse_optional<T: FromStr>(text: &str, label: &str, problems: &mut Vec<String>) -> Option<T> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    match text.parse::<T>() {
        Ok(value) => Some(value),
        Err(_) => {
            problems.push(format!("{label} must be a whole number, got \"{text}\""));
            None
        }
    }
}

/// Like [`parse_optional`], but an empty field is a problem too.
fn parse_required<T: FromStr>(text: &str, label: &str, problems: &mut Vec<String>) -> Option<T> {
    if text.trim().is_empty() {
        problems.push(format!("{label} is required"));
        return None;
    }
    parse_optional(text, label, problems)
}

/// Text and checkbox state of the assetto_corsa.ini form.
#[derive(Clone)]
struct AcForm {
    simulation_value: String,
    logging: bool,
    developer_apps: bool,
    allow_free_camera: bool,
    hide_driver: bool,
    screenshot_format: String,
    mirrors_fov: String,
    mirrors_far_plane: String,
}

impl AcForm {
    fn from_settings(s: &AssettoCorsaSettings) -> Self {
        AcForm {
            simulation_value: s.simulation_value.to_string(),
            logging: s.logging,
            developer_apps: s.developer_apps,
            allow_free_camera: s.allow_free_camera,
            hide_driver: s.hide_driver,
            screenshot_format: s.screenshot_format.clone(),
            mirrors_fov: s.mirrors_fov.to_string(),
            mirrors_far_plane: s.mirrors_far_plane.to_string(),
        }
    }

    /// The settings to save, or a message for each text field that is missing or does not parse.
    fn to_settings(&self) -> Result<AssettoCorsaSettings, Vec<String>> {
        let mut problems = Vec::new();
        let simulation_value = parse_required::<u32>(&self.simulation_value, "simulation value", &mut problems);
        let mirrors_fov = parse_required::<u32>(&self.mirrors_fov, "mirrors field of view", &mut problems);
        let mirrors_far_plane = parse_required::<u32>(&self.mirrors_far_plane, "mirrors far plane", &mut problems);
        match (simulation_value, mirrors_fov, mirrors_far_plane) {
            (Some(simulation_value), Some(mirrors_fov), Some(mirrors_far_plane)) => Ok(AssettoCorsaSettings {
                simulation_value,
                logging: self.logging,
                developer_apps: self.developer_apps,
                allow_free_camera: self.allow_free_camera,
                hide_driver: self.hide_driver,
                screenshot_format: self.screenshot_format.clone(),
                mirrors_fov,
                mirrors_far_plane,
            }),
            // Every missing value above added a problem, so the list is never empty here.
            _ => Err(problems),
        }
    }
}

/// A drop-down over a fixed list of values.
fn choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[T],
    label: impl Fn(T) -> String,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for &option in options {
                ui.selectable_value(&mut *value, option, label(option));
            }
        });
}

struct GameSettings {
    tx: Sender<Event>,
    rx: Receiver<Event>,

    video_dir: String,
    video_requested: bool,
    video_loaded: bool,
    video_file_present: bool,
    video_pending: usize,
    form: VideoForm,

    ac_dir: String,
    /// The Assetto Corsa root the assetto_corsa.ini folder was last set from.
    ac_for_root: Option<PathBuf>,
    ac_loaded: bool,
    ac_file_present: bool,
    ac_pending: usize,
    ac_form: AcForm,

    snapshots: Vec<Snapshot>,
    /// The Assetto Corsa root the snapshot list was requested for.
    listed_root: Option<PathBuf>,
    snap_pending: usize,
    verify: HashMap<PathBuf, Verify>,
    restore_target: Option<PathBuf>,
    restore_confirmed: bool,
}

impl GameSettings {
    /// Run `work` on a background thread and send its result back. Wakes the UI when done.
    fn spawn(&self, ctx: &egui::Context, work: impl FnOnce() -> Event + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    fn load_video(&mut self, ctx: &egui::Context) {
        let dir = PathBuf::from(self.video_dir.trim());
        if dir.as_os_str().is_empty() {
            return;
        }
        self.video_pending += 1;
        self.spawn(ctx, move || {
            let present = dir.join(VIDEO_FILE).is_file();
            let result = game_cfg::read_video(&dir).map_err(|e| e.to_string());
            Event::VideoLoaded { dir: dir.display().to_string(), present, result }
        });
    }

    fn save_video(&mut self, ctx: &egui::Context, settings: VideoSettings) {
        let dir = PathBuf::from(self.video_dir.trim());
        self.video_pending += 1;
        self.spawn(ctx, move || {
            Event::VideoSaved(game_cfg::write_video(&dir, &settings).map_err(|e| e.to_string()))
        });
    }

    fn load_ac(&mut self, ctx: &egui::Context) {
        let dir = PathBuf::from(self.ac_dir.trim());
        if dir.as_os_str().is_empty() {
            return;
        }
        self.ac_pending += 1;
        self.spawn(ctx, move || {
            let present = dir.join(ASSETTO_CORSA_FILE).is_file();
            let result = game_cfg::read_assetto_corsa_ini(&dir).map_err(|e| e.to_string());
            Event::AcLoaded { dir: dir.display().to_string(), present, result }
        });
    }

    /// Goes through `write_assetto_corsa_ini`, which keeps the previous file as a backup.
    fn save_ac(&mut self, ctx: &egui::Context, settings: AssettoCorsaSettings) {
        let dir = PathBuf::from(self.ac_dir.trim());
        self.ac_pending += 1;
        self.spawn(ctx, move || {
            Event::AcSaved(game_cfg::write_assetto_corsa_ini(&dir, &settings).map_err(|e| e.to_string()))
        });
    }

    fn list_snapshots(&mut self, ctx: &egui::Context, root: PathBuf) {
        self.listed_root = Some(root.clone());
        self.snap_pending += 1;
        self.spawn(ctx, move || Event::SnapshotsListed(backup::list_snapshots(&root)));
    }

    fn create_snapshot(&mut self, ctx: &egui::Context, root: PathBuf) {
        self.snap_pending += 1;
        self.spawn(ctx, move || {
            Event::SnapshotCreated(backup::create_snapshot(&root, &[]).map_err(|e| e.to_string()))
        });
    }

    fn verify_snapshot(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.verify.insert(path.clone(), Verify::Checking);
        self.spawn(ctx, move || {
            let result = backup::verify_snapshot(&path).map_err(|e| e.to_string());
            Event::Verified { path, result }
        });
    }

    /// The live game check runs on the same thread as the restore, so it is current.
    fn restore(&mut self, ctx: &egui::Context, root: PathBuf, path: PathBuf, name: String) {
        self.snap_pending += 1;
        self.spawn(ctx, move || {
            let running = game::is_game_running();
            let result = backup::restore_snapshot(&root, &path, running)
                .map_err(|e| e.to_string());
            Event::Restored { name, result }
        });
    }

    /// Apply finished background work to the page state and report it as notices.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::VideoLoaded { dir, present, result } => {
                    self.video_pending = self.video_pending.saturating_sub(1);
                    match result {
                        Ok(settings) => {
                            self.form = VideoForm::from_settings(&settings);
                            self.video_loaded = true;
                            self.video_file_present = present;
                            if !present {
                                notices.push(fail_note(format!(
                                    "{VIDEO_FILE} not found in {dir}; showing defaults. Save creates it."
                                )));
                            }
                        }
                        Err(e) => notices.push(fail_note(format!("Could not read {VIDEO_FILE}: {e}"))),
                    }
                }
                Event::VideoSaved(result) => {
                    self.video_pending = self.video_pending.saturating_sub(1);
                    match result {
                        Ok(()) => {
                            self.video_file_present = true;
                            notices.push(ok_note(format!(
                                "Saved {VIDEO_FILE}. The previous file was kept as a backup."
                            )));
                        }
                        Err(e) => notices.push(fail_note(format!("Could not save {VIDEO_FILE}: {e}"))),
                    }
                }
                Event::AcLoaded { dir, present, result } => {
                    self.ac_pending = self.ac_pending.saturating_sub(1);
                    match result {
                        Ok(settings) => {
                            self.ac_form = AcForm::from_settings(&settings);
                            self.ac_loaded = true;
                            self.ac_file_present = present;
                            if !present {
                                notices.push(fail_note(format!(
                                    "{ASSETTO_CORSA_FILE} not found in {dir}; showing defaults. Save creates it."
                                )));
                            }
                        }
                        Err(e) => {
                            notices.push(fail_note(format!("Could not read {ASSETTO_CORSA_FILE}: {e}")))
                        }
                    }
                }
                Event::AcSaved(result) => {
                    self.ac_pending = self.ac_pending.saturating_sub(1);
                    match result {
                        Ok(()) => {
                            self.ac_file_present = true;
                            notices.push(ok_note(format!(
                                "Saved {ASSETTO_CORSA_FILE}. The previous file was kept as a backup."
                            )));
                        }
                        Err(e) => {
                            notices.push(fail_note(format!("Could not save {ASSETTO_CORSA_FILE}: {e}")))
                        }
                    }
                }
                Event::SnapshotsListed(list) => {
                    self.snap_pending = self.snap_pending.saturating_sub(1);
                    self.snapshots = list;
                }
                Event::SnapshotCreated(result) => {
                    self.snap_pending = self.snap_pending.saturating_sub(1);
                    match result {
                        Ok(snap) => {
                            notices.push(ok_note(format!(
                                "Created snapshot {} ({} files).",
                                snap.name, snap.files
                            )));
                            // Pull the new folder into the list on the next frame.
                            self.listed_root = None;
                        }
                        Err(e) => notices.push(fail_note(format!("Could not create snapshot: {e}"))),
                    }
                }
                Event::Verified { path, result } => {
                    let state = match result {
                        Ok(problems) => Verify::Done(problems),
                        Err(e) => Verify::Failed(e),
                    };
                    self.verify.insert(path, state);
                }
                Event::Restored { name, result } => {
                    self.snap_pending = self.snap_pending.saturating_sub(1);
                    self.restore_target = None;
                    self.restore_confirmed = false;
                    match result {
                        Ok(count) => notices.push(ok_note(format!(
                            "Restored {count} file(s) from {name}. Files they replaced are in {BACKUP_DIR}/pre-restore-*."
                        ))),
                        Err(e) => notices.push(fail_note(format!("Restore failed: {e}"))),
                    }
                }
            }
        }
    }

    fn video_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let idle = self.video_pending == 0;
        theme::card().show(ui, |ui| {
            ui.label(RichText::new("Folder with video.ini (Documents/Assetto Corsa/cfg by default)").color(MUTED));
            ui.horizontal(|ui| {
                let field = (ui.available_width() - 110.0).max(160.0);
                ui.add(egui::TextEdit::singleline(&mut self.video_dir).desired_width(field));
                let can_reload = idle && !self.video_dir.trim().is_empty();
                if ui.add_enabled(can_reload, egui::Button::new("Reload")).clicked() {
                    self.load_video(ctx);
                }
            });
            if !self.video_loaded {
                ui.label(RichText::new("Loading...").small().color(MUTED));
            } else if !self.video_file_present {
                ui.label(RichText::new(format!("{VIDEO_FILE} not found: showing defaults.")).small().color(WARN));
            }
            ui.add_space(6.0);

            egui::Grid::new("game_settings_video")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Resolution");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form.width)
                                .desired_width(72.0)
                                .hint_text("width"),
                        );
                        ui.label("x");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form.height)
                                .desired_width(72.0)
                                .hint_text("height"),
                        );
                        ui.label(RichText::new("empty keeps the file's value").small().color(MUTED));
                    });
                    ui.end_row();

                    ui.label("Fullscreen");
                    ui.checkbox(&mut self.form.fullscreen, "On");
                    ui.end_row();

                    ui.label("VSync");
                    ui.checkbox(&mut self.form.vsync, "On");
                    ui.end_row();

                    ui.label("FPS cap");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form.fps_cap)
                                .desired_width(72.0)
                                .hint_text("none"),
                        );
                        ui.label(RichText::new("frames per second").small().color(MUTED));
                    });
                    ui.end_row();

                    ui.label("Anti-aliasing");
                    choice(ui, "game_settings_aa", &mut self.form.aa_samples, AA_CHOICES, |n: u32| {
                        if n == 1 { "Off".to_string() } else { format!("{n}x") }
                    });
                    ui.end_row();

                    ui.label("Anisotropic filtering");
                    choice(
                        ui,
                        "game_settings_aniso",
                        &mut self.form.anisotropic,
                        ANISOTROPIC_CHOICES,
                        |n: u32| if n == 0 { "Off".to_string() } else { format!("{n}x") },
                    );
                    ui.end_row();

                    ui.label("Shadow map size");
                    choice(
                        ui,
                        "game_settings_shadow",
                        &mut self.form.shadow_map_size,
                        SHADOW_CHOICES,
                        |n: i32| if n < 0 { "Off".to_string() } else { n.to_string() },
                    );
                    ui.end_row();

                    ui.label("World detail");
                    choice(
                        ui,
                        "game_settings_detail",
                        &mut self.form.world_detail,
                        WORLD_DETAIL_CHOICES,
                        |n: u32| format!("{n} of 5"),
                    );
                    ui.end_row();

                    ui.label("Post-processing");
                    ui.checkbox(&mut self.form.post_processing, "On");
                    ui.end_row();
                });

            ui.add_space(6.0);
            let parsed = self.form.to_settings();
            let problems = match &parsed {
                Ok(settings) => game_cfg::validate_video(settings),
                Err(messages) => messages.clone(),
            };
            for problem in &problems {
                ui.label(RichText::new(problem).color(WARN));
            }
            ui.horizontal(|ui| {
                let can_save = idle && problems.is_empty() && !self.video_dir.trim().is_empty();
                if ui
                    .add_enabled(can_save, widgets::primary_button("Save video settings"))
                    .clicked()
                    && let Ok(settings) = parsed
                {
                    self.save_video(ctx, settings);
                }
                ui.label(RichText::new("The previous file is kept as a backup.").small().color(MUTED));
            });
        });
    }

    fn ac_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let idle = self.ac_pending == 0;
        theme::card().show(ui, |ui| {
            ui.label(
                RichText::new(format!(
                    "Folder with {ASSETTO_CORSA_FILE} (system/cfg under the Assetto Corsa folder by default)"
                ))
                .color(MUTED),
            );
            ui.horizontal(|ui| {
                let field = (ui.available_width() - 110.0).max(160.0);
                ui.add(egui::TextEdit::singleline(&mut self.ac_dir).desired_width(field));
                let can_reload = idle && !self.ac_dir.trim().is_empty();
                if ui.add_enabled(can_reload, egui::Button::new("Reload")).clicked() {
                    self.load_ac(ctx);
                }
            });
            if self.ac_dir.trim().is_empty() {
                ui.label(
                    RichText::new("Set the Assetto Corsa folder on the Home page, or type a folder above.").color(WARN),
                );
            } else if !self.ac_loaded {
                ui.label(RichText::new("Loading...").small().color(MUTED));
            } else if !self.ac_file_present {
                ui.label(RichText::new(format!("{ASSETTO_CORSA_FILE} not found: showing defaults.")).small().color(WARN));
            }
            ui.add_space(6.0);

            egui::Grid::new("game_settings_ac")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Simulation value");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ac_form.simulation_value)
                                .desired_width(72.0)
                                .hint_text("0-100"),
                        );
                        ui.label(RichText::new("percent").small().color(MUTED));
                    });
                    ui.end_row();

                    ui.label("Logging");
                    ui.checkbox(&mut self.ac_form.logging, "On");
                    ui.end_row();

                    ui.label("Developer apps");
                    ui.checkbox(&mut self.ac_form.developer_apps, "On");
                    ui.end_row();

                    ui.label("Free camera");
                    ui.checkbox(&mut self.ac_form.allow_free_camera, "Allowed");
                    ui.end_row();

                    ui.label("Hide driver");
                    ui.checkbox(&mut self.ac_form.hide_driver, "On");
                    ui.end_row();

                    ui.label("Screenshot format");
                    egui::ComboBox::from_id_salt("game_settings_ac_screenshot")
                        .selected_text(self.ac_form.screenshot_format.clone())
                        .show_ui(ui, |ui| {
                            for &option in SCREENSHOT_CHOICES {
                                ui.selectable_value(
                                    &mut self.ac_form.screenshot_format,
                                    option.to_string(),
                                    option,
                                );
                            }
                        });
                    ui.end_row();

                    ui.label("Mirror field of view");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ac_form.mirrors_fov)
                                .desired_width(72.0)
                                .hint_text("1-180"),
                        );
                    });
                    ui.end_row();

                    ui.label("Mirror far plane");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ac_form.mirrors_far_plane)
                                .desired_width(72.0)
                                .hint_text("10-2000"),
                        );
                    });
                    ui.end_row();
                });

            ui.add_space(6.0);
            let parsed = self.ac_form.to_settings();
            let problems = match &parsed {
                Ok(settings) => game_cfg::validate_assetto_corsa(settings),
                Err(messages) => messages.clone(),
            };
            for problem in &problems {
                ui.label(RichText::new(problem).color(WARN));
            }
            ui.horizontal(|ui| {
                let can_save = idle && problems.is_empty() && !self.ac_dir.trim().is_empty();
                if ui
                    .add_enabled(can_save, widgets::primary_button("Save assetto_corsa settings"))
                    .clicked()
                    && let Ok(settings) = parsed
                {
                    self.save_ac(ctx, settings);
                }
                ui.label(RichText::new("The previous file is kept as a backup.").small().color(MUTED));
            });
        });
    }

    fn snapshot_section(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        root: Option<PathBuf>,
        game_running: bool,
    ) {
        theme::card().show(ui, |ui| {
            let Some(root) = root else {
                ui.label(RichText::new("Set the Assetto Corsa folder on the Home page to make snapshots.").color(WARN));
                return;
            };
            ui.label(
                RichText::new(format!("Snapshots are stored in {}", root.join(BACKUP_DIR).display()))
                    .small()
                    .color(MUTED),
            );
            ui.add_space(6.0);

            let idle = self.snap_pending == 0;
            ui.horizontal(|ui| {
                if ui.add_enabled(idle, widgets::primary_button("Create snapshot")).clicked() {
                    self.create_snapshot(ctx, root.clone());
                }
                if ui.add_enabled(idle, egui::Button::new("Refresh list")).clicked() {
                    self.listed_root = None;
                }
                if !idle {
                    ui.spinner();
                    ui.label(RichText::new("Working...").color(MUTED));
                }
            });
            if game_running {
                ui.label(
                    RichText::new("Assetto Corsa is running. Restores are refused until it closes.").color(WARN),
                );
            }
            ui.add_space(8.0);

            if self.snapshots.is_empty() {
                ui.label(RichText::new("No snapshots yet.").color(MUTED));
            }
            let snapshots = self.snapshots.clone();
            for (index, snap) in snapshots.iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                self.snapshot_row(ui, ctx, &root, snap);
            }
        });
    }

    fn snapshot_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, root: &Path, snap: &Snapshot) {
        let idle = self.snap_pending == 0;
        let state = self.verify.get(&snap.path).cloned();
        let verifying = matches!(state, Some(Verify::Checking));
        let open = self.restore_target.as_deref() == Some(snap.path.as_path());

        ui.horizontal(|ui| {
            ui.label(RichText::new(&snap.created).strong());
            ui.label(RichText::new(format!("{} files", snap.files)).color(MUTED));
            match &state {
                None => {}
                Some(Verify::Checking) => widgets::pill(ui, "checking", INFO),
                Some(Verify::Done(problems)) if problems.is_empty() => widgets::pill(ui, "intact", OK),
                Some(Verify::Done(problems)) => {
                    widgets::pill(ui, &format!("{} problem(s)", problems.len()), DANGER)
                }
                Some(Verify::Failed(_)) => widgets::pill(ui, "unreadable", DANGER),
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(&snap.name).small().color(MUTED));
            if ui.add_enabled(!verifying, egui::Button::new("Verify")).clicked() {
                self.verify_snapshot(ctx, snap.path.clone());
            }
            let label = if open { "Cancel restore" } else { "Restore" };
            if ui.add_enabled(idle, egui::Button::new(label)).clicked() {
                self.restore_target = if open { None } else { Some(snap.path.clone()) };
                self.restore_confirmed = false;
            }
        });

        match &state {
            Some(Verify::Done(problems)) if !problems.is_empty() => {
                for problem in problems.iter().take(MAX_PROBLEMS_SHOWN) {
                    ui.label(RichText::new(problem).small().color(DANGER));
                }
                if problems.len() > MAX_PROBLEMS_SHOWN {
                    let more = problems.len() - MAX_PROBLEMS_SHOWN;
                    ui.label(RichText::new(format!("...and {more} more")).small().color(MUTED));
                }
            }
            Some(Verify::Failed(error)) => {
                ui.label(RichText::new(error).small().color(DANGER));
            }
            _ => {}
        }

        if open {
            ui.checkbox(
                &mut self.restore_confirmed,
                "I understand this replaces the current cfg and extension/config files. \
                 Files they replace are moved, not deleted, into acxs-backups/pre-restore-*.",
            );
            ui.horizontal(|ui| {
                let can_restore = self.restore_confirmed && idle;
                let button = egui::Button::new(RichText::new("Restore now").color(DANGER));
                if ui.add_enabled(can_restore, button).clicked() {
                    self.restore(ctx, root.to_path_buf(), snap.path.clone(), snap.name.clone());
                }
            });
        }
    }
}

impl Tool for GameSettings {
    fn name(&self) -> &'static str {
        "Game Settings"
    }

    fn group(&self) -> &'static str {
        "Config"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let egui_ctx = ui.ctx().clone();
        self.poll(notices);

        widgets::heading(
            ui,
            "Game Settings",
            "Video and assetto_corsa.ini options, and snapshots of your Assetto Corsa configuration.",
        );

        if !self.video_requested {
            self.video_requested = true;
            self.load_video(&egui_ctx);
        }
        if let Some(root) = &ctx.ac_root
            && self.ac_for_root.as_ref() != Some(root) && self.ac_pending == 0
        {
            self.ac_for_root = Some(root.clone());
            self.ac_dir = default_ac_dir(root);
            self.load_ac(&egui_ctx);
        }
        if let Some(root) = &ctx.ac_root
            && self.listed_root.as_ref() != Some(root) && self.snap_pending == 0
        {
            self.list_snapshots(&egui_ctx, root.clone());
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                widgets::section_title(ui, "Video");
                self.video_section(ui, &egui_ctx);
                ui.add_space(16.0);
                widgets::section_title(ui, "assetto_corsa.ini");
                self.ac_section(ui, &egui_ctx);
                ui.add_space(16.0);
                widgets::section_title(ui, "Snapshots");
                self.snapshot_section(ui, &egui_ctx, ctx.ac_root.clone(), ctx.game_running);
            });
    }
}
