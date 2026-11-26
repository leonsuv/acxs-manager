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
use acxs_manager::controls::{self, ControlsConfig, InputMethod};
use acxs_manager::ffb::{self, FfbPostProcess, FfbPostProcessMode};
use acxs_manager::game;
use acxs_manager::game_cfg::{
    self, AssettoCorsaSettings, CameraMode, CameraOnboardSettings, ReplaySettings, TripleScreenSettings, VideoSettings,
    VrSettings, ASSETTO_CORSA_FILE, VIDEO_FILE,
};
use acxs_manager::input_poll::{self, BindEvent, DeviceInfo, DeviceState, InputBackend};
use acxs_manager::rig_profiles::{self, RigProfile, RigProfileStore};
use acxs_manager::settings;
use std::collections::BTreeMap;
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
    let cfg_dir = settings::ac_user_cfg_dir();
    let triple = cfg_dir.as_deref().and_then(|d| game_cfg::read_triple_screen(d).ok()).unwrap_or_default();
    let vr_oculus = cfg_dir.as_deref().and_then(|d| game_cfg::read_vr(d, false).ok()).unwrap_or_default();
    let vr_openvr = cfg_dir.as_deref().and_then(|d| game_cfg::read_vr(d, true).ok()).unwrap_or_default();
    let cam_onboard = cfg_dir.as_deref().and_then(|d| game_cfg::read_camera_onboard(d).ok()).unwrap_or_default();
    let replay_cfg = cfg_dir.as_deref().and_then(|d| game_cfg::read_replay_settings(d).ok()).unwrap_or_default();
    let controls_cfg = cfg_dir.as_deref().and_then(|d| controls::read_controls(d).ok()).unwrap_or_default();
    let ffb_pp = cfg_dir.as_deref().and_then(|d| ffb::read_post_process(d).ok()).unwrap_or_default();
    let per_car_ffb = cfg_dir.as_deref().and_then(|d| ffb::read_per_car_ffb(d).ok()).unwrap_or_default();
    let rig_store = RigProfileStore::load(&rig_profiles_path()).unwrap_or_default();

    Box::new(GameSettings {
        tab: VIDEO_TAB,
        options: super::game_options::GameOptions::new(),
        tx,
        rx,
        video_dir: default_video_dir(),
        video_requested: false,
        video_loaded: false,
        video_file_present: false,
        video_pending: 0,
        form: VideoForm::from_settings(&VideoSettings::default()),
        saved_form: VideoForm::from_settings(&VideoSettings::default()),
        ac_dir: String::new(),
        ac_for_root: None,
        ac_loaded: false,
        ac_file_present: false,
        ac_pending: 0,
        ac_form: AcForm::from_settings(&AssettoCorsaSettings::default()),
        saved_ac_form: AcForm::from_settings(&AssettoCorsaSettings::default()),
        snapshots: Vec::new(),
        listed_root: None,
        snap_pending: 0,
        verify: HashMap::new(),
        restore_target: None,
        restore_confirmed: false,
        triple,
        vr_oculus,
        vr_openvr,
        cam_onboard,
        replay_cfg,
        controls_cfg,
        ffb_pp,
        per_car_ffb,
        new_car_ffb_id: String::new(),
        new_car_ffb_gain: 1.0,
        rig_store,
        new_rig_name: String::new(),
        input_backend: input_poll::default_backend(),
        devices: Vec::new(),
        listening_target: None,
        listen_baseline: None,
        last_bound_note: String::new(),
    })
}

fn rig_profiles_path() -> PathBuf {
    settings::settings_path()
        .parent()
        .map_or_else(|| PathBuf::from("rig-profiles.json"), |d| d.join("rig-profiles.json"))
}

/// `video.ini` lives in `Documents/Assetto Corsa/cfg/` (see `game_cfg` module docs).
fn default_video_dir() -> String {
    acxs_manager::settings::ac_user_cfg_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default()
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
#[derive(Clone, PartialEq)]
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
            ..VideoSettings::default()
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
#[derive(Clone, PartialEq)]
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

const VIDEO_TAB: &str = "Video";
const VR_TRIPLE_TAB: &str = "VR & Triple";
const CONTROLS_TAB: &str = "Controls";
const FFB_LUT_TAB: &str = "FFB LUT";
const RIG_PROFILES_TAB: &str = "Rig Profiles";
const CAMERA_REPLAY_TAB: &str = "Camera & Replay";
const ADVANCED_TAB: &str = "Advanced";

/// Grouped navigation rail categories: `(section_header, &[(display_label, internal_tab_key)])`.
const RAIL_GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "DISPLAY",
        &[
            ("Video", VIDEO_TAB),
            ("VR & Triple", VR_TRIPLE_TAB),
            ("Camera & Replay", CAMERA_REPLAY_TAB),
        ],
    ),
    (
        "CONTROLS",
        &[
            ("Controls", CONTROLS_TAB),
            ("FFB & LUT", FFB_LUT_TAB),
            ("Rig Profiles", RIG_PROFILES_TAB),
        ],
    ),
    (
        "SIMULATION",
        &[
            ("Audio", "Audio"),
            ("Force Feedback", "Force feedback"),
            ("Gameplay", "Gameplay"),
            ("Filters & Apps", "Filter and apps"),
            ("Advanced & Snapshots", ADVANCED_TAB),
        ],
    ),
];

struct GameSettings {
    tab: &'static str,
    options: super::game_options::GameOptions,
    tx: Sender<Event>,
    rx: Receiver<Event>,

    video_dir: String,
    video_requested: bool,
    video_loaded: bool,
    video_file_present: bool,
    video_pending: usize,
    form: VideoForm,
    /// The form as last loaded or saved: what the file holds right now.
    saved_form: VideoForm,

    ac_dir: String,
    /// The Assetto Corsa root the assetto_corsa.ini folder was last set from.
    ac_for_root: Option<PathBuf>,
    ac_loaded: bool,
    ac_file_present: bool,
    ac_pending: usize,
    ac_form: AcForm,
    saved_ac_form: AcForm,

    snapshots: Vec<Snapshot>,
    /// The Assetto Corsa root the snapshot list was requested for.
    listed_root: Option<PathBuf>,
    snap_pending: usize,
    verify: HashMap<PathBuf, Verify>,
    restore_target: Option<PathBuf>,
    restore_confirmed: bool,

    // Wave 3C Offline Settings: VR/Triple, Camera/Replay, Controls, FFB LUT, Rig Profiles
    triple: TripleScreenSettings,
    vr_oculus: VrSettings,
    vr_openvr: VrSettings,
    cam_onboard: CameraOnboardSettings,
    replay_cfg: ReplaySettings,
    controls_cfg: ControlsConfig,
    ffb_pp: FfbPostProcess,
    per_car_ffb: BTreeMap<String, f64>,
    new_car_ffb_id: String,
    new_car_ffb_gain: f64,
    rig_store: RigProfileStore,
    new_rig_name: String,
    input_backend: Box<dyn InputBackend>,
    devices: Vec<DeviceInfo>,
    listening_target: Option<&'static str>,
    listen_baseline: Option<(u32, DeviceState)>,
    last_bound_note: String,
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
            // The game keeps the user's own settings (video.ini, controls.ini, race.ini,
            // CSP's cfg/extension) in Documents, so that folder is part of every snapshot.
            let extra: Vec<PathBuf> = acxs_manager::settings::ac_user_cfg_dir().into_iter().collect();
            Event::SnapshotCreated(backup::create_snapshot(&root, &extra).map_err(|e| e.to_string()))
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
                            self.saved_form = self.form.clone();
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
                            self.saved_ac_form = self.ac_form.clone();
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
        let base = self.saved_form.clone();
        let changes = video_changes(&base, &self.form);
        let (quality_was, cost_was) = impact(&base);
        let (quality_now, cost_now) = impact(&self.form);
        let parsed = self.form.to_settings();
        let problems = match &parsed {
            Ok(settings) => game_cfg::validate_video(settings),
            Err(messages) => messages.clone(),
        };
        let can_save = idle && problems.is_empty() && !self.video_dir.trim().is_empty();
        let mut revert = false;
        let mut trigger_save = false;

        let avail = ui.available_width();
        if avail >= 820.0 {
            let gap = 16.0;
            let left_w = ((avail - gap) * 0.56).floor();
            let right_w = (avail - gap - left_w).max(300.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(left_w);
                        self.video_controls_card(ui, &base, &problems, can_save, &mut trigger_save);
                    },
                );
                ui.add_space(gap);
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(right_w);
                        video_impact_card(ui, quality_was, quality_now, cost_was, cost_now, &changes, &mut revert);
                    },
                );
            });
        } else {
            self.video_controls_card(ui, &base, &problems, can_save, &mut trigger_save);
            ui.add_space(12.0);
            video_impact_card(ui, quality_was, quality_now, cost_was, cost_now, &changes, &mut revert);
        }

        if revert {
            self.form = base.clone();
        }
        if trigger_save && let Ok(settings) = parsed.clone() {
            self.saved_form = self.form.clone();
            self.save_video(ctx, settings);
        }

        ui.add_space(10.0);
        let bar_action = widgets::pending_changes_bar(
            ui,
            &widgets::PendingChangesBar {
                changed_count: changes.len(),
                summary: format!("{} video setting(s) modified", changes.len()),
                can_save,
                blocked_reason: problems.first().cloned(),
            },
        );
        match bar_action {
            widgets::PendingAction::Save => {
                if let Ok(settings) = parsed {
                    self.saved_form = self.form.clone();
                    self.save_video(ctx, settings);
                }
            }
            widgets::PendingAction::RevertAll => {
                self.form = base;
            }
            _ => {}
        }
    }

    fn video_controls_card(
        &mut self,
        ui: &mut egui::Ui,
        base: &VideoForm,
        problems: &[String],
        can_save: bool,
        trigger_save: &mut bool,
    ) {
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Video Settings (video.ini)");
            if !self.video_loaded {
                ui.label(RichText::new("Loading...").small().color(MUTED));
            } else if !self.video_file_present {
                ui.label(RichText::new(format!("{VIDEO_FILE} not found: showing defaults.")).small().color(WARN));
            }
            ui.add_space(8.0);

            egui::Grid::new("game_settings_video")
                .num_columns(2)
                .spacing([18.0, 10.0])
                .show(ui, |ui| {
                    row_label(ui, "Resolution", base.width != self.form.width || base.height != self.form.height);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form.width)
                                .desired_width(72.0)
                                .hint_text("width"),
                        );
                        ui.label("×");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form.height)
                                .desired_width(72.0)
                                .hint_text("height"),
                        );
                        ui.label(RichText::new("empty keeps file value").small().color(MUTED));
                    });
                    ui.end_row();

                    row_label(ui, "Fullscreen", base.fullscreen != self.form.fullscreen);
                    widgets::toggle(ui, &mut self.form.fullscreen);
                    ui.end_row();

                    row_label(ui, "VSync", base.vsync != self.form.vsync);
                    widgets::toggle(ui, &mut self.form.vsync);
                    ui.end_row();

                    row_label(ui, "FPS cap", base.fps_cap != self.form.fps_cap);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.form.fps_cap)
                                .desired_width(72.0)
                                .hint_text("none"),
                        );
                        ui.label(RichText::new("frames per second").small().color(MUTED));
                    });
                    ui.end_row();

                    row_label(ui, "Anti-aliasing", base.aa_samples != self.form.aa_samples);
                    choice(ui, "game_settings_aa", &mut self.form.aa_samples, AA_CHOICES, |n: u32| {
                        if n == 1 { "Off".to_string() } else { format!("{n}x") }
                    });
                    ui.end_row();

                    row_label(ui, "Anisotropic filtering", base.anisotropic != self.form.anisotropic);
                    choice(
                        ui,
                        "game_settings_aniso",
                        &mut self.form.anisotropic,
                        ANISOTROPIC_CHOICES,
                        |n: u32| if n == 0 { "Off".to_string() } else { format!("{n}x") },
                    );
                    ui.end_row();

                    row_label(ui, "Shadow map size", base.shadow_map_size != self.form.shadow_map_size);
                    choice(
                        ui,
                        "game_settings_shadow",
                        &mut self.form.shadow_map_size,
                        SHADOW_CHOICES,
                        |n: i32| if n < 0 { "Off".to_string() } else { n.to_string() },
                    );
                    ui.end_row();

                    row_label(ui, "World detail", base.world_detail != self.form.world_detail);
                    choice(
                        ui,
                        "game_settings_detail",
                        &mut self.form.world_detail,
                        WORLD_DETAIL_CHOICES,
                        |n: u32| format!("{n} of 5"),
                    );
                    ui.end_row();

                    row_label(ui, "Post-processing", base.post_processing != self.form.post_processing);
                    widgets::toggle(ui, &mut self.form.post_processing);
                    ui.end_row();
                });

            if !problems.is_empty() {
                ui.add_space(8.0);
                for problem in problems {
                    ui.label(RichText::new(problem).color(WARN));
                }
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(can_save, widgets::primary_button("Save video settings")).clicked() {
                    *trigger_save = true;
                }
                ui.label(RichText::new("Previous file is kept as a backup.").small().color(MUTED));
            });
        });
    }

    fn ac_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let idle = self.ac_pending == 0;
        let base = self.saved_ac_form.clone();
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Advanced Game Options (assetto_corsa.ini)");
            if self.ac_dir.trim().is_empty() {
                ui.label(
                    RichText::new("Set the Assetto Corsa folder in Settings.").color(WARN),
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
                    row_label(ui, "Simulation value", base.simulation_value != self.ac_form.simulation_value);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ac_form.simulation_value)
                                .desired_width(72.0)
                                .hint_text("0-100"),
                        );
                        ui.label(RichText::new("percent").small().color(MUTED));
                    });
                    ui.end_row();

                    row_label(ui, "Logging", base.logging != self.ac_form.logging);
                    widgets::toggle(ui, &mut self.ac_form.logging);
                    ui.end_row();

                    row_label(ui, "Developer apps", base.developer_apps != self.ac_form.developer_apps);
                    widgets::toggle(ui, &mut self.ac_form.developer_apps);
                    ui.end_row();

                    row_label(ui, "Free camera", base.allow_free_camera != self.ac_form.allow_free_camera);
                    widgets::toggle(ui, &mut self.ac_form.allow_free_camera);
                    ui.end_row();

                    row_label(ui, "Hide driver", base.hide_driver != self.ac_form.hide_driver);
                    widgets::toggle(ui, &mut self.ac_form.hide_driver);
                    ui.end_row();

                    row_label(ui, "Screenshot format", base.screenshot_format != self.ac_form.screenshot_format);
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

                    row_label(ui, "Mirror field of view", base.mirrors_fov != self.ac_form.mirrors_fov);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ac_form.mirrors_fov)
                                .desired_width(72.0)
                                .hint_text("1-180"),
                        );
                    });
                    ui.end_row();

                    row_label(ui, "Mirror far plane", base.mirrors_far_plane != self.ac_form.mirrors_far_plane);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ac_form.mirrors_far_plane)
                                .desired_width(72.0)
                                .hint_text("10-2000"),
                        );
                    });
                    ui.end_row();
                });

            let changes = ac_changes(&base, &self.ac_form);
            let mut revert = false;
            ui.add_space(8.0);
            theme::card_flat().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("What this changes").font(theme::bold(13.5)));
                ui.add_space(4.0);
                revert = changes_list(ui, &changes);
            });
            if revert {
                self.ac_form = base.clone();
            }
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
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Configuration Snapshots");
            let Some(root) = root else {
                ui.label(RichText::new("Set the Assetto Corsa folder in Settings to make snapshots.").color(WARN));
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
    fn vr_triple_section(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        let cfg_dir = PathBuf::from(self.video_dir.trim());
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Display & VR Mode (video.ini [CAMERA] MODE)");
            let mut mode = game_cfg::read_video(&cfg_dir).map(|v| v.camera_mode).unwrap_or_default();
            let before = mode;
            ui.horizontal_wrapped(|ui| {
                for m in CameraMode::ALL {
                    ui.selectable_value(&mut mode, m, m.label());
                }
            });
            if mode != before {
                if let Ok(mut v) = game_cfg::read_video(&cfg_dir) {
                    v.camera_mode = mode;
                    match game_cfg::write_video(&cfg_dir, &v) {
                        Ok(()) => notices.push(ok_note(format!("Switched camera mode to {}.", mode.label()))),
                        Err(e) => notices.push(fail_note(format!("Could not switch camera mode: {e}"))),
                    }
                }
            }
        });
        ui.add_space(10.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Triple Screen Geometry (triple_screen.ini)");
            egui::Grid::new("gs_triple").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                ui.label("Screen width / height (mm)");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.triple.width_mm).range(100.0..=3000.0).suffix(" mm"));
                    ui.add(egui::DragValue::new(&mut self.triple.height_mm).range(100.0..=2000.0).suffix(" mm"));
                });
                ui.end_row();
                ui.label("Eye distance / Side angle");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.triple.distance_mm).range(100.0..=3000.0).suffix(" mm"));
                    ui.add(egui::DragValue::new(&mut self.triple.angle_deg).range(0.0..=89.0).suffix("°"));
                });
                ui.end_row();
                ui.label("Bezel width (mm) / Center px");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.triple.bezel_mm).range(0.0..=100.0).suffix(" mm"));
                    ui.add(egui::DragValue::new(&mut self.triple.sp_width_px).range(640..=7680).suffix(" px"));
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            if ui.add(widgets::primary_button("Save triple_screen.ini")).clicked() {
                match game_cfg::write_triple_screen(&cfg_dir, &self.triple) {
                    Ok(()) => notices.push(ok_note("Saved triple_screen.ini (backup kept).")),
                    Err(e) => notices.push(fail_note(format!("Save failed: {e}"))),
                }
            }
        });
        ui.add_space(10.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "VR Settings (oculus.ini & openvr.ini)");
            ui.horizontal(|ui| {
                ui.label("OpenVR / OpenXR PPD:");
                ui.add(egui::Slider::new(&mut self.vr_openvr.pixel_per_display, 0.5..=2.5).step_by(0.05));
                ui.checkbox(&mut self.vr_openvr.mirror_texture, "Mirror");
                if ui.button("Save openvr.ini").clicked() {
                    match game_cfg::write_vr(&cfg_dir, true, &self.vr_openvr) {
                        Ok(()) => notices.push(ok_note("Saved openvr.ini.")),
                        Err(e) => notices.push(fail_note(format!("Save failed: {e}"))),
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Oculus / Meta PPD:");
                ui.add(egui::Slider::new(&mut self.vr_oculus.pixel_per_display, 0.5..=2.5).step_by(0.05));
                ui.checkbox(&mut self.vr_oculus.autoselect_audio, "Auto Audio");
                if ui.button("Save oculus.ini").clicked() {
                    match game_cfg::write_vr(&cfg_dir, false, &self.vr_oculus) {
                        Ok(()) => notices.push(ok_note("Saved oculus.ini.")),
                        Err(e) => notices.push(fail_note(format!("Save failed: {e}"))),
                    }
                }
            });
        });
    }

    fn controls_section(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        let cfg_dir = PathBuf::from(self.video_dir.trim());
        // Poll live binding if listening.
        if let Some(target) = self.listening_target {
            if let Some((dev_idx, baseline)) = self.listen_baseline.clone()
                && let Some(now) = self.input_backend.poll(dev_idx)
                && let Some(ev) = input_poll::detect_binding(dev_idx, &baseline, &now, 0.25)
            {
                match (target, ev) {
                    ("STEER", BindEvent::Axis { device, axis, inverted }) => {
                        self.controls_cfg.steer.joy = device as i32;
                        self.controls_cfg.steer.axle = axis as i32;
                        self.controls_cfg.steer.scale = if inverted { -1 } else { 1 };
                        self.last_bound_note = format!("Bound Steer -> Joy {device} Axis {axis}");
                    }
                    ("THROTTLE", BindEvent::Axis { device, axis, .. }) => {
                        self.controls_cfg.throttle.joy = device as i32;
                        self.controls_cfg.throttle.axle = axis as i32;
                        self.last_bound_note = format!("Bound Throttle -> Joy {device} Axis {axis}");
                    }
                    ("BRAKES", BindEvent::Axis { device, axis, .. }) => {
                        self.controls_cfg.brakes.joy = device as i32;
                        self.controls_cfg.brakes.axle = axis as i32;
                        self.last_bound_note = format!("Bound Brakes -> Joy {device} Axis {axis}");
                    }
                    (sec, BindEvent::Button { device, button }) => {
                        self.controls_cfg.actions.insert(
                            sec.to_string(),
                            controls::ButtonBinding { joy: device as i32, button: button as i32, key: String::new() },
                        );
                        self.last_bound_note = format!("Bound {sec} -> Joy {device} Btn {button}");
                    }
                    _ => {}
                }
                self.listening_target = None;
                self.listen_baseline = None;
            }
            ui.ctx().request_repaint();
        }

        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Controls & Live Wheel/Pedal Binder (controls.ini)");
            ui.horizontal_wrapped(|ui| {
                ui.label("Input Method:");
                ui.selectable_value(&mut self.controls_cfg.input_method, InputMethod::Wheel, "Wheel");
                ui.selectable_value(&mut self.controls_cfg.input_method, InputMethod::X360, "Xbox / Gamepad");
                ui.selectable_value(&mut self.controls_cfg.input_method, InputMethod::Keyboard, "Keyboard");
                if ui.button("Scan Controllers").clicked() {
                    self.devices = self.input_backend.enumerate();
                    self.controls_cfg.controllers = self
                        .devices
                        .iter()
                        .map(|d| (d.name.clone(), d.product_guid.clone()))
                        .collect();
                }
                if !self.last_bound_note.is_empty() {
                    widgets::pill(ui, &self.last_bound_note, OK);
                }
            });
            ui.add_space(6.0);
            if !self.devices.is_empty() {
                for dev in &self.devices {
                    ui.label(RichText::new(format!("[{}] {}  {}", dev.index, dev.name, dev.product_guid)).font(theme::mono(11.5)).color(MUTED));
                }
                ui.add_space(6.0);
            }
            egui::Grid::new("gs_controls_axes").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("Steering Lock / FFB Gain");
                ui.add(egui::DragValue::new(&mut self.controls_cfg.steer.lock_deg).range(180..=1800).suffix("°"));
                ui.add(egui::Slider::new(&mut self.controls_cfg.steer.ff_gain, 0.0..=2.0).suffix(" gain"));
                if ui.small_button(if self.listening_target == Some("STEER") { "Move wheel..." } else { "Bind Steer" }).clicked() {
                    if let Some(st) = self.input_backend.poll(0) {
                        self.listening_target = Some("STEER");
                        self.listen_baseline = Some((0, st));
                    }
                }
                ui.end_row();

                ui.label("Throttle Gamma / Range");
                ui.add(egui::DragValue::new(&mut self.controls_cfg.throttle.gamma).range(0.5..=4.0).speed(0.05));
                ui.add(egui::DragValue::new(&mut self.controls_cfg.throttle.range_to).range(0.1..=1.0).speed(0.02));
                if ui.small_button("Bind Throttle").clicked() {
                    if let Some(st) = self.input_backend.poll(0) {
                        self.listening_target = Some("THROTTLE");
                        self.listen_baseline = Some((0, st));
                    }
                }
                ui.end_row();

                ui.label("Brake Gamma / Range");
                ui.add(egui::DragValue::new(&mut self.controls_cfg.brakes.gamma).range(0.5..=4.0).speed(0.05));
                ui.add(egui::DragValue::new(&mut self.controls_cfg.brakes.range_to).range(0.1..=1.0).speed(0.02));
                if ui.small_button("Bind Brake").clicked() {
                    if let Some(st) = self.input_backend.poll(0) {
                        self.listening_target = Some("BRAKES");
                        self.listen_baseline = Some((0, st));
                    }
                }
                ui.end_row();

                ui.label("H-Shifter Active");
                widgets::toggle(ui, &mut self.controls_cfg.shifter.active);
                ui.label(RichText::new("CSP Actions: Extra A–F, Wipers, VR Recenter").size(12.0).color(MUTED));
                ui.label("");
                ui.end_row();
            });
            ui.add_space(8.0);
            if ui.add(widgets::primary_button("Save controls.ini")).clicked() {
                match controls::write_controls(&cfg_dir, &self.controls_cfg) {
                    Ok(()) => notices.push(ok_note("Saved controls.ini (backup kept).")),
                    Err(e) => notices.push(fail_note(format!("Save failed: {e}"))),
                }
            }
        });
    }

    fn ffb_lut_section(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        let cfg_dir = PathBuf::from(self.video_dir.trim());
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "FFB Post-Processing (ff_post_process.ini)");
            ui.horizontal(|ui| {
                widgets::toggle(ui, &mut self.ffb_pp.enabled);
                ui.label("Enable FFB Post-Processing");
                ui.selectable_value(&mut self.ffb_pp.mode, FfbPostProcessMode::Gamma, "Gamma");
                ui.selectable_value(&mut self.ffb_pp.mode, FfbPostProcessMode::Lut, "LUT Curve");
            });
            if self.ffb_pp.mode == FfbPostProcessMode::Gamma {
                ui.add(egui::Slider::new(&mut self.ffb_pp.gamma, 0.1..=2.0).text("Gamma"));
            } else {
                ui.horizontal(|ui| {
                    ui.label("LUT filename in cfg/:");
                    ui.add(egui::TextEdit::singleline(&mut self.ffb_pp.lut_curve).hint_text("mywheel.lut").desired_width(220.0));
                });
            }
            if ui.add(widgets::primary_button("Save ff_post_process.ini")).clicked() {
                match ffb::write_post_process(&cfg_dir, &self.ffb_pp) {
                    Ok(()) => notices.push(ok_note("Saved ff_post_process.ini.")),
                    Err(e) => notices.push(fail_note(format!("Save failed: {e}"))),
                }
            }
        });
        ui.add_space(10.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Per-Car FFB Gain (native user_ff.ini)");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_car_ffb_id).hint_text("car id (e.g. ks_ferrari_488_gt3)").desired_width(220.0));
                ui.add(egui::DragValue::new(&mut self.new_car_ffb_gain).range(0.10..=3.00).speed(0.05).suffix("x"));
                if ui.button("Set Car FFB").clicked() {
                    match ffb::write_car_ffb(&cfg_dir, &self.new_car_ffb_id, self.new_car_ffb_gain) {
                        Ok(()) => {
                            self.per_car_ffb.insert(self.new_car_ffb_id.trim().to_string(), self.new_car_ffb_gain);
                            notices.push(ok_note(format!("Updated FFB for {}.", self.new_car_ffb_id.trim())));
                            self.new_car_ffb_id.clear();
                        }
                        Err(e) => notices.push(fail_note(format!("Could not save user_ff.ini: {e}"))),
                    }
                }
            });
            for (car, gain) in &self.per_car_ffb {
                ui.label(RichText::new(format!("{car:<36}  {:.0}%", gain * 100.0)).font(theme::mono(12.0)));
            }
        });
    }

    fn rig_profiles_section(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        let cfg_dir = PathBuf::from(self.video_dir.trim());
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "1-Click Atomic Rig Profile Switcher (VR ↔ Triple ↔ Flat)");
            ui.label(RichText::new("Captures Video, VR/Triple, Controls & FFB into one atomic profile with full rollback on error.").size(12.5).color(MUTED));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_rig_name).hint_text("Profile name (e.g. VR + Wheel)").desired_width(240.0));
                if ui.add(widgets::primary_button("Save Current Rig")).clicked() {
                    let profile = RigProfile {
                        name: self.new_rig_name.trim().to_string(),
                        video: self.form.to_settings().ok(),
                        triple_screen: Some(self.triple.clone()),
                        vr_oculus: Some(self.vr_oculus.clone()),
                        vr_openvr: Some(self.vr_openvr.clone()),
                        controls: Some(self.controls_cfg.clone()),
                        ffb_post_process: Some(self.ffb_pp.clone()),
                    };
                    match self.rig_store.upsert(profile).and_then(|()| self.rig_store.save(&rig_profiles_path())) {
                        Ok(()) => {
                            notices.push(ok_note(format!("Saved rig profile '{}'.", self.new_rig_name.trim())));
                            self.new_rig_name.clear();
                        }
                        Err(e) => notices.push(fail_note(format!("Could not save rig profile: {e}"))),
                    }
                }
            });
            ui.add_space(8.0);
            for prof in self.rig_store.profiles.clone() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&prof.name).font(theme::bold(14.0)));
                    if let Some(v) = &prof.video {
                        widgets::pill(ui, v.camera_mode.label(), INFO);
                    }
                    if ui.button("Apply Profile").clicked() {
                        match rig_profiles::apply_rig_profile(&cfg_dir, &prof) {
                            Ok(()) => notices.push(ok_note(format!("Applied rig profile '{}'.", prof.name))),
                            Err(e) => notices.push(fail_note(format!("Switch failed (rolled back): {e}"))),
                        }
                    }
                });
            }
        });
    }

    fn camera_replay_section(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        let cfg_dir = PathBuf::from(self.video_dir.trim());
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Cockpit Camera (camera_onboard_free.ini)");
            ui.add(egui::Slider::new(&mut self.cam_onboard.fov, 10.0..=120.0).text("FOV (°)"));
            ui.checkbox(&mut self.cam_onboard.lock_to_horizon, "Lock to horizon");
            ui.add(egui::Slider::new(&mut self.cam_onboard.look_ahead, 0.0..=1.0).text("Look ahead"));
            if ui.add(widgets::primary_button("Save camera_onboard_free.ini")).clicked() {
                match game_cfg::write_camera_onboard(&cfg_dir, &self.cam_onboard) {
                    Ok(()) => notices.push(ok_note("Saved camera_onboard_free.ini.")),
                    Err(e) => notices.push(fail_note(format!("Save failed: {e}"))),
                }
            }
        });
        ui.add_space(10.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Replay Recording (replay.ini)");
            ui.horizontal(|ui| {
                ui.label("FPS:");
                for fps in [10u32, 15, 20, 30, 60] {
                    ui.selectable_value(&mut self.replay_cfg.fps, fps, format!("{fps} Hz"));
                }
            });
            ui.add(egui::Slider::new(&mut self.replay_cfg.max_size_mb, 50..=2048).suffix(" MB").text("Max replay size"));
            ui.checkbox(&mut self.replay_cfg.autosave, "Autosave replays");
            if ui.add(widgets::primary_button("Save replay.ini")).clicked() {
                match game_cfg::write_replay_settings(&cfg_dir, &self.replay_cfg) {
                    Ok(()) => notices.push(ok_note("Saved replay.ini.")),
                    Err(e) => notices.push(fail_note(format!("Save failed: {e}"))),
                }
            }
        });
    }
}

impl Tool for GameSettings {
    fn name(&self) -> &'static str {
        "Game Settings"
    }

    fn content_width(&self) -> f32 {
        f32::INFINITY
    }

    fn group(&self) -> &'static str {
        "Config"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let egui_ctx = ui.ctx().clone();
        self.poll(notices);

        widgets::heading(ui, "Game Settings", "Video, VR/Triple Screen, Controls, FFB LUT, Atomic Rig Profiles & Snapshots.");

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
            .id_salt("gs_main_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail = ui.available_width();
                let rail_w = 210.0;
                let gap = 16.0;
                let content_w = (avail - rail_w - gap).max(360.0);

                ui.horizontal_top(|ui| {
                    // Left 210 px category rail.
                    ui.allocate_ui_with_layout(
                        egui::vec2(rail_w, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(rail_w);
                            theme::card_elevated().show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                for (g_idx, (group_title, items)) in RAIL_GROUPS.iter().enumerate() {
                                    if g_idx > 0 {
                                        ui.add_space(10.0);
                                    }
                                    ui.label(theme::micro_label(group_title));
                                    ui.add_space(4.0);
                                    for &(label, key) in *items {
                                        if widgets::nav_item(ui, self.tab == key, label, None, 2.0).clicked() {
                                            self.tab = key;
                                        }
                                    }
                                }
                            });
                        },
                    );

                    ui.add_space(gap);

                    // Right full-width content area (flows naturally inside outer ScrollArea; never clips).
                    ui.allocate_ui_with_layout(
                        egui::vec2(content_w, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(content_w);
                            match self.tab {
                                VIDEO_TAB => self.video_section(ui, &egui_ctx),
                                VR_TRIPLE_TAB => self.vr_triple_section(ui, notices),
                                CONTROLS_TAB => self.controls_section(ui, notices),
                                FFB_LUT_TAB => self.ffb_lut_section(ui, notices),
                                RIG_PROFILES_TAB => self.rig_profiles_section(ui, notices),
                                CAMERA_REPLAY_TAB => self.camera_replay_section(ui, notices),
                                ADVANCED_TAB => {
                                    let w = ui.available_width();
                                    if w >= 860.0 {
                                        let col_gap = 16.0;
                                        let half = ((w - col_gap) * 0.5).floor();
                                        ui.horizontal_top(|ui| {
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(half, 0.0),
                                                egui::Layout::top_down(egui::Align::Min),
                                                |ui| {
                                                    ui.set_width(half);
                                                    self.ac_section(ui, &egui_ctx);
                                                },
                                            );
                                            ui.add_space(col_gap);
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(half, 0.0),
                                                egui::Layout::top_down(egui::Align::Min),
                                                |ui| {
                                                    ui.set_width(half);
                                                    self.snapshot_section(ui, &egui_ctx, ctx.ac_root.clone(), ctx.game_running);
                                                },
                                            );
                                        });
                                    } else {
                                        self.ac_section(ui, &egui_ctx);
                                        ui.add_space(14.0);
                                        self.snapshot_section(ui, &egui_ctx, ctx.ac_root.clone(), ctx.game_running);
                                    }
                                }
                                tab => {
                                    theme::card_elevated().show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.set_min_height(520.0);
                                        widgets::section_title(ui, tab);
                                        self.options.show(ui, ctx, notices, tab);
                                    });
                                }
                            }
                        },
                    );
                });
            });
    }
}

fn video_impact_card(
    ui: &mut egui::Ui,
    quality_was: f32,
    quality_now: f32,
    cost_was: f32,
    cost_now: f32,
    changes: &[(&'static str, String, String)],
    revert: &mut bool,
) {
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, "Visual Quality vs Performance");
        ui.add_space(4.0);
        meter(ui, "Visual quality", quality_was, quality_now, true);
        ui.add_space(6.0);
        meter(ui, "Performance cost", cost_was, cost_now, false);
        ui.add_space(6.0);
        ui.label(
            RichText::new("The white tick marks the saved file value. Relative guide, not a benchmark.")
                .size(11.5)
                .color(theme::FAINT),
        );
    });
    ui.add_space(12.0);
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            widgets::section_title(ui, "Changed Values");
            widgets::pill(
                ui,
                &changes.len().to_string(),
                if changes.is_empty() { MUTED } else { theme::ACCENT },
            );
        });
        if changes.is_empty() {
            ui.label(RichText::new("No unsaved video changes.").color(MUTED));
        } else {
            for (name, was, now) in changes {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(*name).color(MUTED));
                    ui.label(RichText::new(was).color(theme::FAINT).strikethrough());
                    ui.label(RichText::new("→").color(MUTED));
                    ui.label(RichText::new(now).font(theme::bold(13.5)).color(theme::ACCENT));
                });
            }
            ui.add_space(8.0);
            if ui.button("Revert all changes").clicked() {
                *revert = true;
            }
        }
    });
}

/// A grid label that is marked and tinted when its value differs from the saved one.
fn row_label(ui: &mut egui::Ui, text: &str, changed: bool) {
    if changed {
        ui.label(RichText::new(format!("• {text}")).color(theme::ACCENT));
    } else {
        ui.label(text);
    }
}

/// Rough (visual quality, performance cost) of a video setup, both 0..=100. A guide for
/// comparing before and after, not a benchmark: it weighs each option by how much it usually
/// changes the picture and the frame time.
fn impact(form: &VideoForm) -> (f32, f32) {
    let log2 = |n: f32| n.max(1.0).log2();
    let pixels = form.width.parse::<f32>().unwrap_or(1920.0) * form.height.parse::<f32>().unwrap_or(1080.0);
    let res = (pixels / (3840.0 * 2160.0)).clamp(0.0, 1.0);
    let shadow = if form.shadow_map_size < 0 { 0.0 } else { ((log2(form.shadow_map_size as f32) - 5.0) / 8.0).clamp(0.0, 1.0) };
    let aa = log2(form.aa_samples as f32) / 3.0;
    let aniso = log2(form.anisotropic as f32) / 4.0;
    let detail = form.world_detail as f32 / 5.0;
    let post = if form.post_processing { 1.0 } else { 0.0 };
    let quality = 18.0 * aa + 10.0 * aniso + 24.0 * shadow + 20.0 * detail + 14.0 * post + 14.0 * res.sqrt();
    let cost = 24.0 * aa + 5.0 * aniso + 18.0 * shadow + 10.0 * detail + 10.0 * post + 33.0 * res;
    (quality.clamp(0.0, 100.0), cost.clamp(0.0, 100.0))
}

/// A bar with a marker at the saved value and the new value filled, plus the difference.
fn meter(ui: &mut egui::Ui, label: &str, saved: f32, now: f32, higher_is_better: bool) {
    ui.horizontal(|ui| {
        ui.add_sized([130.0, 18.0], egui::Label::new(RichText::new(label).color(MUTED)));
        let (rect, _) = ui.allocate_exact_size(egui::vec2((ui.available_width() - 70.0).max(120.0), 10.0), egui::Sense::hover());
        let round = egui::CornerRadius::same(5);
        ui.painter().rect_filled(rect, round, theme::INPUT);
        let fill_to = |v: f32| egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * (v / 100.0).clamp(0.0, 1.0), rect.height()));
        let delta = now - saved;
        let good = if higher_is_better { delta >= 0.0 } else { delta <= 0.0 };
        let color = if delta.abs() < 0.5 { theme::MUTED } else if good { OK } else { WARN };
        ui.painter().rect_filled(fill_to(now), round, color.gamma_multiply(0.85));
        // The saved value stays visible as a tick, so a change reads as a gap.
        let x = rect.left() + rect.width() * (saved / 100.0).clamp(0.0, 1.0);
        ui.painter().line_segment(
            [egui::pos2(x, rect.top() - 3.0), egui::pos2(x, rect.bottom() + 3.0)],
            egui::Stroke::new(2.0, egui::Color32::WHITE),
        );
        let text = if delta.abs() < 0.5 { "same".to_string() } else { format!("{delta:+.0}") };
        ui.label(RichText::new(text).font(theme::bold(13.0)).color(color));
    });
}

/// Everything that differs from the saved file, as (name, was, now).
fn video_changes(saved: &VideoForm, now: &VideoForm) -> Vec<(&'static str, String, String)> {
    let onoff = |b: bool| if b { "On" } else { "Off" }.to_string();
    let res = |f: &VideoForm| format!("{}x{}", f.width, f.height);
    let mut out = Vec::new();
    if saved.width != now.width || saved.height != now.height {
        out.push(("Resolution", res(saved), res(now)));
    }
    if saved.fullscreen != now.fullscreen {
        out.push(("Fullscreen", onoff(saved.fullscreen), onoff(now.fullscreen)));
    }
    if saved.vsync != now.vsync {
        out.push(("VSync", onoff(saved.vsync), onoff(now.vsync)));
    }
    if saved.fps_cap != now.fps_cap {
        let cap = |f: &VideoForm| if f.fps_cap.is_empty() { "none".to_string() } else { f.fps_cap.clone() };
        out.push(("FPS cap", cap(saved), cap(now)));
    }
    if saved.aa_samples != now.aa_samples {
        out.push(("Anti-aliasing", format!("{}x", saved.aa_samples), format!("{}x", now.aa_samples)));
    }
    if saved.anisotropic != now.anisotropic {
        out.push(("Anisotropic", format!("{}x", saved.anisotropic), format!("{}x", now.anisotropic)));
    }
    if saved.shadow_map_size != now.shadow_map_size {
        out.push(("Shadow map", saved.shadow_map_size.to_string(), now.shadow_map_size.to_string()));
    }
    if saved.world_detail != now.world_detail {
        out.push(("World detail", saved.world_detail.to_string(), now.world_detail.to_string()));
    }
    if saved.post_processing != now.post_processing {
        out.push(("Post-processing", onoff(saved.post_processing), onoff(now.post_processing)));
    }
    out
}

/// Changes of the assetto_corsa.ini form as (name, was, now).
fn ac_changes(saved: &AcForm, now: &AcForm) -> Vec<(&'static str, String, String)> {
    let onoff = |b: bool| if b { "On" } else { "Off" }.to_string();
    let mut out = Vec::new();
    if saved.simulation_value != now.simulation_value {
        out.push(("Simulation value", saved.simulation_value.clone(), now.simulation_value.clone()));
    }
    if saved.logging != now.logging {
        out.push(("Logging", onoff(saved.logging), onoff(now.logging)));
    }
    if saved.developer_apps != now.developer_apps {
        out.push(("Developer apps", onoff(saved.developer_apps), onoff(now.developer_apps)));
    }
    if saved.allow_free_camera != now.allow_free_camera {
        out.push(("Free camera", onoff(saved.allow_free_camera), onoff(now.allow_free_camera)));
    }
    if saved.hide_driver != now.hide_driver {
        out.push(("Hide driver", onoff(saved.hide_driver), onoff(now.hide_driver)));
    }
    if saved.screenshot_format != now.screenshot_format {
        out.push(("Screenshot format", saved.screenshot_format.clone(), now.screenshot_format.clone()));
    }
    if saved.mirrors_fov != now.mirrors_fov {
        out.push(("Mirror field of view", saved.mirrors_fov.clone(), now.mirrors_fov.clone()));
    }
    if saved.mirrors_far_plane != now.mirrors_far_plane {
        out.push(("Mirror far plane", saved.mirrors_far_plane.clone(), now.mirrors_far_plane.clone()));
    }
    out
}

/// The "was → now" list with a revert button. Returns true when revert was clicked.
fn changes_list(ui: &mut egui::Ui, changes: &[(&'static str, String, String)]) -> bool {
    if changes.is_empty() {
        ui.label(RichText::new("Nothing changed yet.").color(MUTED));
        return false;
    }
    for (name, was, now) in changes {
        ui.horizontal(|ui| {
            ui.label(RichText::new(*name).color(MUTED));
            ui.label(RichText::new(was).color(theme::FAINT).strikethrough());
            ui.label(RichText::new("→").color(MUTED));
            ui.label(RichText::new(now).font(theme::bold(13.5)).color(theme::ACCENT));
        });
    }
    ui.add_space(4.0);
    ui.button("Revert all changes").clicked()
}
