//! CSP Config tool page: edits the Custom Shaders Patch config files in
//! `extension/config` and shows the FFB, weather, gamepad and car class settings.
//!
//! Every file read and write runs on a background thread. Results come back
//! over a channel that is polled with `try_recv` on each frame, and the thread
//! requests a repaint when it is done.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, ACCENT, DANGER, INFO, MUTED, OK, TEXT, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::car_classes::{self, CarClass, CarYear};
use acxs_manager::csp_config::{self, ConfigFile, Setting};
use acxs_manager::ffb::{self, FfbTweaks};
use acxs_manager::gamepad::{self, PadSetup};
use acxs_manager::weather::{self, WeatherPreset};
use eframe::egui::{self, RichText};
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Backup names carry whole seconds, so two writes in the same second would
/// overwrite each other's backup. Writes in one batch are spaced by this gap.
const WRITE_GAP: Duration = Duration::from_millis(1100);
const CLASS_ROW_HEIGHT: f32 = 20.0;

pub fn tool() -> Box<dyn Tool> {
    Box::new(CspConfig::new())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Tab {
    #[default]
    Settings,
    Ffb,
    Weather,
    Gamepad,
    Classes,
}

const TABS: [(Tab, &str); 5] = [
    (Tab::Settings, "Settings"),
    (Tab::Ffb, "FFB"),
    (Tab::Weather, "Weather"),
    (Tab::Gamepad, "Gamepad"),
    (Tab::Classes, "Car classes"),
];

/// Messages from background jobs. Every job ends by sending `Finished`.
/// Data messages carry the AC root they were read from, so results that
/// arrive after the root changed are ignored.
enum Msg {
    Notice(Notice),
    Finished,
    Files { root: PathBuf, files: Vec<ConfigFile> },
    Settings { root: PathBuf, path: PathBuf, result: Result<Vec<Setting>, String> },
    Ffb { root: PathBuf, result: Result<FfbTweaks, String> },
    Weather {
        root: PathBuf,
        presets: Vec<WeatherPreset>,
        controllers: Vec<String>,
        fx: Result<Vec<(String, String, String)>, String>,
    },
    PresetValues {
        root: PathBuf,
        preset: String,
        result: Result<Vec<(String, String, String)>, String>,
    },
    Gamepad { root: PathBuf, result: Result<PadSetup, String> },
    Classes { root: PathBuf, result: Result<(Vec<CarClass>, Vec<CarYear>), String> },
    Diff { root: PathBuf, tab: Tab, lines: Vec<String> },
}

/// A write from the Car classes tab's edit form.
enum ClassEdit {
    AddToClass { car: String, class: String },
    SetYear { car: String, year: i64 },
}

/// One car in the car classes table.
struct CarRow {
    id: String,
    class: String,
    year: Option<i64>,
}

impl CarRow {
    fn matches(&self, query: &str) -> bool {
        self.id.to_lowercase().contains(query)
            || self.class.to_lowercase().contains(query)
            || self.year.is_some_and(|y| y.to_string().contains(query))
    }
}

/// Everything read from one AC install. Dropped when the AC root changes.
#[derive(Default)]
struct Data {
    files_requested: bool,
    files: Vec<ConfigFile>,
    selected: Option<PathBuf>,
    /// The file whose settings were requested last.
    settings_for: Option<PathBuf>,
    settings: Option<Vec<Setting>>,
    /// Edited values by (section, key). A value equal to the file's value is not a change.
    edits: HashMap<(String, String), String>,
    new_section: String,
    new_key: String,
    new_value: String,

    ffb_requested: bool,
    /// Working copy, edited in the form.
    ffb: Option<FfbTweaks>,
    /// Values as last read from disk.
    ffb_disk: Option<FfbTweaks>,

    gamepad_requested: bool,
    gamepad: Option<PadSetup>,
    gamepad_disk: Option<PadSetup>,

    weather_requested: bool,
    presets: Vec<WeatherPreset>,
    controllers: Vec<String>,
    fx: Vec<(String, String, String)>,
    /// Edited weather_fx values by (section, key). Only values that differ from `fx` are changes.
    weather_edits: HashMap<(String, String), String>,
    /// Preset picked in the Weather tab.
    selected_preset: Option<String>,
    /// The preset whose settings.ini values were requested last.
    preset_values_for: Option<String>,
    preset_values: Option<Vec<(String, String, String)>>,
    /// Edited preset values by (section, key), as for `weather_edits`.
    preset_edits: HashMap<(String, String), String>,
    preset_error: Option<String>,
    /// Name typed for a new or duplicated preset.
    preset_name: String,

    classes_requested: bool,
    classes: Vec<CarClass>,
    car_rows: Vec<CarRow>,
    class_query: String,
    /// Car edit form on the Car classes tab.
    class_car: String,
    class_target: String,
    class_year: String,

    errors: HashMap<Tab, String>,
    diff: Option<(Tab, Vec<String>)>,
}

pub struct CspConfig {
    tab: Tab,
    root: Option<PathBuf>,
    data: Data,
    allow_new_key: bool,
    file_filter: String,
    patch_v02: String,
    patch_v03: String,
    /// Jobs started and not yet finished.
    pending: usize,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl CspConfig {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        CspConfig {
            tab: Tab::default(),
            root: None,
            data: Data::default(),
            allow_new_key: false,
            file_filter: String::new(),
            patch_v02: String::new(),
            patch_v03: String::new(),
            pending: 0,
            tx,
            rx,
        }
    }

    fn is_current(&self, root: &Path) -> bool {
        self.root.as_deref() == Some(root)
    }

    fn patches_set(&self) -> bool {
        !self.patch_v02.trim().is_empty() && !self.patch_v03.trim().is_empty()
    }

    /// Run `work` on a background thread. `Finished` is sent after it returns.
    fn spawn(&mut self, egui_ctx: &egui::Context, work: impl FnOnce(&Sender<Msg>) + Send + 'static) {
        self.pending += 1;
        let tx = self.tx.clone();
        let egui_ctx = egui_ctx.clone();
        thread::spawn(move || {
            work(&tx);
            let _ = tx.send(Msg::Finished);
            egui_ctx.request_repaint();
        });
    }

    /// Apply everything the background jobs sent since the last frame.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Finished => self.pending = self.pending.saturating_sub(1),
                Msg::Notice(notice) => notices.push(notice),
                Msg::Files { root, files } => {
                    if self.is_current(&root) {
                        self.data.files = files;
                        if self.data.selected.is_none() {
                            self.data.selected = self.data.files.first().map(|f| f.path.clone());
                        }
                    }
                }
                Msg::Settings { root, path, result } => {
                    if self.is_current(&root) && self.data.selected.as_deref() == Some(path.as_path()) {
                        match result {
                            Ok(list) => {
                                self.data.settings = Some(list);
                                self.data.edits.clear();
                                self.data.errors.remove(&Tab::Settings);
                            }
                            Err(e) => {
                                self.data.settings = Some(Vec::new());
                                self.data.errors.insert(Tab::Settings, e);
                            }
                        }
                    }
                }
                Msg::Ffb { root, result } => {
                    if self.is_current(&root) {
                        match result {
                            Ok(t) => {
                                self.data.ffb = Some(t.clone());
                                self.data.ffb_disk = Some(t);
                                self.data.errors.remove(&Tab::Ffb);
                            }
                            Err(e) => {
                                self.data.errors.insert(Tab::Ffb, e);
                            }
                        }
                    }
                }
                Msg::Weather { root, presets, controllers, fx } => {
                    if self.is_current(&root) {
                        self.data.presets = presets;
                        self.data.controllers = controllers;
                        let selected_gone = self
                            .data
                            .selected_preset
                            .as_ref()
                            .is_some_and(|name| !self.data.presets.iter().any(|p| &p.name == name));
                        if selected_gone {
                            self.data.selected_preset = None;
                            self.data.preset_values_for = None;
                            self.data.preset_values = None;
                            self.data.preset_edits.clear();
                        }
                        match fx {
                            Ok(rows) => {
                                self.data.fx = rows;
                                self.data.errors.remove(&Tab::Weather);
                            }
                            Err(e) => {
                                self.data.fx.clear();
                                self.data.errors.insert(Tab::Weather, format!("weather_fx.ini: {e}"));
                            }
                        }
                    }
                }
                Msg::PresetValues { root, preset, result } => {
                    if self.is_current(&root) && self.data.selected_preset.as_deref() == Some(preset.as_str()) {
                        match result {
                            Ok(values) => {
                                self.data.preset_values = Some(values);
                                self.data.preset_error = None;
                            }
                            Err(e) => {
                                self.data.preset_values = Some(Vec::new());
                                self.data.preset_error = Some(e);
                            }
                        }
                    }
                }
                Msg::Gamepad { root, result } => {
                    if self.is_current(&root) {
                        match result {
                            Ok(s) => {
                                self.data.gamepad = Some(s.clone());
                                self.data.gamepad_disk = Some(s);
                                self.data.errors.remove(&Tab::Gamepad);
                            }
                            Err(e) => {
                                self.data.errors.insert(Tab::Gamepad, e);
                            }
                        }
                    }
                }
                Msg::Classes { root, result } => {
                    if self.is_current(&root) {
                        match result {
                            Ok((classes, years)) => {
                                self.data.car_rows = build_car_rows(&classes, &years);
                                self.data.classes = classes;
                                self.data.errors.remove(&Tab::Classes);
                            }
                            Err(e) => {
                                self.data.errors.insert(Tab::Classes, e);
                            }
                        }
                    }
                }
                Msg::Diff { root, tab, lines } => {
                    if self.is_current(&root) {
                        self.data.diff = Some((tab, lines));
                    }
                }
            }
        }
    }

    /// Start the reads the current tab needs, once per AC root (or after a reload).
    fn ensure(&mut self, egui_ctx: &egui::Context, root: &Path) {
        match self.tab {
            Tab::Settings => {
                if !self.data.files_requested {
                    self.data.files_requested = true;
                    let root_buf = root.to_path_buf();
                    let dir = config_dir(root);
                    self.spawn(egui_ctx, move |tx| {
                        let files = csp_config::list_config_files(&dir);
                        let _ = tx.send(Msg::Files { root: root_buf, files });
                    });
                }
                if let Some(path) = self.data.selected.clone()
                    && self.data.settings_for.as_deref() != Some(path.as_path())
                {
                    self.data.settings_for = Some(path.clone());
                    self.data.settings = None;
                    let root_buf = root.to_path_buf();
                    self.spawn(egui_ctx, move |tx| {
                        let user = csp_config::user_config_path(&path);
                        let result = csp_config::read_effective_settings(&path, user.as_deref()).map_err(|e| e.to_string());
                        let _ = tx.send(Msg::Settings { root: root_buf, path, result });
                    });
                }
            }
            Tab::Ffb => {
                if !self.data.ffb_requested {
                    self.data.ffb_requested = true;
                    let root_buf = root.to_path_buf();
                    let ac = root_buf.clone();
                    self.spawn(egui_ctx, move |tx| {
                        let user = csp_config::user_config_path(&ffb::config_path(&ac));
                        let result = ffb::read_with_user(&ac, user.as_deref()).map_err(|e| e.to_string());
                        let _ = tx.send(Msg::Ffb { root: root_buf, result });
                    });
                }
            }
            Tab::Weather => {
                if !self.data.weather_requested {
                    self.data.weather_requested = true;
                    let root_buf = root.to_path_buf();
                    self.spawn(egui_ctx, move |tx| {
                        let _ = tx.send(weather_msg(root_buf));
                    });
                }
                if let Some(preset) = self.data.selected_preset.clone()
                    && self.data.preset_values_for.as_deref() != Some(preset.as_str())
                {
                    self.data.preset_values_for = Some(preset.clone());
                    self.data.preset_values = None;
                    let root_buf = root.to_path_buf();
                    self.spawn(egui_ctx, move |tx| {
                        let result = weather::read_preset_values(&root_buf, &preset).map_err(|e| e.to_string());
                        let _ = tx.send(Msg::PresetValues { root: root_buf, preset, result });
                    });
                }
            }
            Tab::Gamepad => {
                if !self.data.gamepad_requested {
                    self.data.gamepad_requested = true;
                    let root_buf = root.to_path_buf();
                    let path = gamepad::config_path(root);
                    self.spawn(egui_ctx, move |tx| {
                        let user = csp_config::user_config_path(&path);
                        let controls = gamepad::controls_path();
                        let result = gamepad::read_setup(&path, user.as_deref(), controls.as_deref())
                            .map_err(|e| e.to_string());
                        let _ = tx.send(Msg::Gamepad { root: root_buf, result });
                    });
                }
            }
            Tab::Classes => {
                if !self.data.classes_requested {
                    self.data.classes_requested = true;
                    let root_buf = root.to_path_buf();
                    self.spawn(egui_ctx, move |tx| {
                        let _ = tx.send(classes_msg(root_buf));
                    });
                }
            }
        }
    }

    /// Forget what the tab shows and read it again on the next frame.
    fn reload(&mut self, tab: Tab) {
        let d = &mut self.data;
        d.errors.remove(&tab);
        match tab {
            Tab::Settings => {
                d.files_requested = false;
                d.settings_for = None;
                d.settings = None;
                d.edits.clear();
            }
            Tab::Ffb => d.ffb_requested = false,
            Tab::Weather => {
                d.weather_requested = false;
                d.weather_edits.clear();
                d.preset_values_for = None;
                d.preset_values = None;
                d.preset_edits.clear();
                d.preset_error = None;
            }
            Tab::Gamepad => d.gamepad_requested = false,
            Tab::Classes => d.classes_requested = false,
        }
    }

    fn select_file(&mut self, egui_ctx: &egui::Context, root: &Path, path: PathBuf) {
        self.data.selected = Some(path);
        self.data.edits.clear();
        self.ensure(egui_ctx, root);
    }

    fn save_settings(
        &mut self,
        egui_ctx: &egui::Context,
        root: &Path,
        path: PathBuf,
        changes: Vec<(String, String, String)>,
    ) {
        let allow_new = self.allow_new_key;
        let root_buf = root.to_path_buf();
        self.spawn(egui_ctx, move |tx| {
            let user = csp_config::user_config_path(&path);
            let total = changes.len();
            let mut failures = Vec::new();
            for (i, (section, key, value)) in changes.iter().enumerate() {
                if i > 0 {
                    thread::sleep(WRITE_GAP);
                }
                // CSP reads the user's values from Documents/Assetto Corsa/cfg/extension,
                // which override the defaults in the install; write there when it is known.
                let written = match &user {
                    Some(user) => csp_config::write_user_setting(&path, user, section, key, value, allow_new),
                    None => csp_config::write_setting(&path, section, key, value, allow_new),
                };
                if let Err(e) = written {
                    failures.push(format!("[{section}] {key}: {e}"));
                }
            }
            let file = user
                .as_deref()
                .unwrap_or(&path)
                .display()
                .to_string();
            if failures.is_empty() {
                note(tx, format!("Saved {total} setting(s) to {file}."), true);
            } else {
                note(tx, format!("Not all settings were saved: {}", failures.join("; ")), false);
            }
            let result = csp_config::read_effective_settings(&path, user.as_deref()).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Settings { root: root_buf, path, result });
        });
    }

    fn save_ffb(&mut self, egui_ctx: &egui::Context, root: &Path, tweaks: FfbTweaks) {
        let root_buf = root.to_path_buf();
        let ac = root_buf.clone();
        let user = csp_config::user_config_path(&ffb::config_path(&ac));
        self.spawn(egui_ctx, move |tx| match ffb::write_with_user(&ac, user.as_deref(), &tweaks) {
            Ok(()) => {
                note(tx, "FFB tweaks saved.", true);
                let result = ffb::read_with_user(&ac, user.as_deref()).map_err(|e| e.to_string());
                let _ = tx.send(Msg::Ffb { root: root_buf, result });
            }
            Err(e) => note(tx, format!("FFB tweaks not saved: {e}"), false),
        });
    }

    fn save_gamepad(&mut self, egui_ctx: &egui::Context, root: &Path, settings: PadSetup) {
        let root_buf = root.to_path_buf();
        let path = gamepad::config_path(root);
        let user = csp_config::user_config_path(&path);
        let controls = gamepad::controls_path();
        self.spawn(egui_ctx, move |tx| {
            let (Some(user), Some(controls)) = (user, controls) else {
                note(tx, "Gamepad settings not saved: the Documents folder was not found.", false);
                return;
            };
            match gamepad::write_setup(&user, &controls, &settings) {
                Ok(()) => {
                    note(tx, format!("Gamepad settings saved to {} and {}.", user.display(), controls.display()), true);
                    let result = gamepad::read_setup(&path, Some(&user), Some(&controls)).map_err(|e| e.to_string());
                    let _ = tx.send(Msg::Gamepad { root: root_buf, result });
                }
                Err(e) => note(tx, format!("Gamepad settings not saved: {e}"), false),
            }
        });
    }

    /// Write one weather_fx.ini value on a background thread, then read the tab again.
    fn save_weather_value(&mut self, egui_ctx: &egui::Context, root: &Path, section: String, key: String, value: String) {
        let root_buf = root.to_path_buf();
        let allow_new = self.allow_new_key;
        self.spawn(egui_ctx, move |tx| {
            match weather::set_weather_value_user(
                &root_buf,
                csp_config::user_config_path(&weather::weather_fx_path(&root_buf)).as_deref(),
                &section,
                &key,
                &value,
                allow_new,
            ) {
                Ok(()) => note(tx, format!("Saved [{section}] {key}."), true),
                Err(e) => note(tx, format!("Not saved: [{section}] {key}: {e}"), false),
            }
            let _ = tx.send(weather_msg(root_buf));
        });
    }

    /// Pick a preset in the Weather tab and read its values.
    fn select_preset(&mut self, egui_ctx: &egui::Context, root: &Path, name: String) {
        let d = &mut self.data;
        d.selected_preset = Some(name);
        d.preset_values_for = None;
        d.preset_values = None;
        d.preset_edits.clear();
        d.preset_error = None;
        self.ensure(egui_ctx, root);
    }

    /// Write one preset settings.ini value on a background thread, then read the preset again.
    fn save_preset_value(
        &mut self,
        egui_ctx: &egui::Context,
        root: &Path,
        preset: String,
        section: String,
        key: String,
        value: String,
    ) {
        let root_buf = root.to_path_buf();
        let allow_new = self.allow_new_key;
        self.spawn(egui_ctx, move |tx| {
            match weather::set_preset_value(&root_buf, &preset, &section, &key, &value, allow_new) {
                Ok(()) => note(tx, format!("Saved {preset} [{section}] {key}."), true),
                Err(e) => note(tx, format!("Not saved: {preset} [{section}] {key}: {e}"), false),
            }
            let result = weather::read_preset_values(&root_buf, &preset).map_err(|e| e.to_string());
            let _ = tx.send(Msg::PresetValues { root: root_buf, preset, result });
        });
    }

    /// Create a preset from the default parameter file, on a background thread.
    fn new_preset(&mut self, egui_ctx: &egui::Context, root: &Path, name: String) {
        let root_buf = root.to_path_buf();
        self.spawn(egui_ctx, move |tx| {
            match weather::create_preset(&root_buf, &name) {
                Ok(_) => note(tx, format!("Created preset {name}."), true),
                Err(e) => note(tx, format!("Preset not created: {e}"), false),
            }
            let _ = tx.send(weather_msg(root_buf));
        });
    }

    /// Copy the preset `from` to a new preset `to`, on a background thread.
    fn duplicate_preset(&mut self, egui_ctx: &egui::Context, root: &Path, from: String, to: String) {
        let root_buf = root.to_path_buf();
        self.spawn(egui_ctx, move |tx| {
            match weather::copy_preset(&root_buf, &from, &to) {
                Ok(()) => note(tx, format!("Copied {from} to {to}."), true),
                Err(e) => note(tx, format!("Preset not copied: {e}"), false),
            }
            let _ = tx.send(weather_msg(root_buf));
        });
    }

    /// Apply one car classes edit on a background thread, then read the tab again.
    fn save_classes(&mut self, egui_ctx: &egui::Context, root: &Path, edit: ClassEdit) {
        let root_buf = root.to_path_buf();
        self.spawn(egui_ctx, move |tx| {
            let dir = config_dir(&root_buf);
            let outcome = match &edit {
                ClassEdit::AddToClass { car, class } => {
                    car_classes::add_car_to_class(&dir, car, class).map(|()| format!("Added {car} to class {class}."))
                }
                ClassEdit::SetYear { car, year } => {
                    car_classes::set_car_year(&dir, car, *year).map(|()| format!("Set {car} year to {year}."))
                }
            };
            match outcome {
                Ok(text) => note(tx, text, true),
                Err(e) => note(tx, format!("Car classes not saved: {e}"), false),
            }
            let _ = tx.send(classes_msg(root_buf));
        });
    }

    /// Compare the two patch folders for the current tab.
    fn start_diff(&mut self, egui_ctx: &egui::Context, root: &Path, tab: Tab) {
        let v02 = PathBuf::from(self.patch_v02.trim());
        let v03 = PathBuf::from(self.patch_v03.trim());
        let file = self
            .data
            .selected
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        let root_buf = root.to_path_buf();
        self.spawn(egui_ctx, move |tx| {
            let lines: Vec<String> = match tab {
                Tab::Settings => match file {
                    Some(name) => csp_config::version_diff(&name, &config_dir(&v02), &config_dir(&v03))
                        .into_iter()
                        .map(|(s, k, a, b)| change_line(&s, &k, a, b))
                        .collect(),
                    None => vec!["Select a config file first.".to_string()],
                },
                Tab::Ffb => ffb::version_diff(&v02, &v03)
                    .into_iter()
                    .map(|(s, k, a, b)| change_line(&s, &k, a, b))
                    .collect(),
                Tab::Weather => weather::compare_versions(&v02, &v03),
                Tab::Classes => car_classes::version_diff(&config_dir(&v02), &config_dir(&v03)),
                Tab::Gamepad => Vec::new(),
            };
            let lines = if lines.is_empty() { vec!["No differences found.".to_string()] } else { lines };
            let _ = tx.send(Msg::Diff { root: root_buf, tab, lines });
        });
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context, root: &Path, busy: bool) {
        let mut reload = false;
        let mut compare = false;
        let mut select: Option<PathBuf> = None;
        let mut save: Option<Vec<(String, String, String)>> = None;
        let can_compare = !busy && self.data.selected.is_some() && self.patches_set();

        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Reload")).clicked() {
                reload = true;
            }
            if ui
                .add_enabled(can_compare, egui::Button::new("Compare patch versions"))
                .clicked()
            {
                compare = true;
            }
            ui.add_space(8.0);
            widgets::toggle(ui, &mut self.allow_new_key);
            ui.label(RichText::new("Allow creating new keys").color(MUTED));
        });
        show_error(ui, &self.data, Tab::Settings);
        diff_card(ui, &self.data.diff, Tab::Settings);
        ui.add_space(6.0);

        // Left: the config files, searchable. Right: the selected file's settings.
        egui::Panel::left("csp_files")
            .exact_size(250.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::SIDEBAR)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.file_filter)
                        .hint_text("Filter files")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(10, 6)),
                );
                ui.add_space(4.0);
                let filter = self.file_filter.trim().to_lowercase();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.data.files.is_empty() {
                        ui.label(RichText::new("No .ini files found in extension/config.").color(MUTED));
                    }
                    for file in &self.data.files {
                        let nice = friendly_name(&file.name);
                        if !filter.is_empty()
                            && !nice.to_lowercase().contains(&filter)
                            && !file.name.to_lowercase().contains(&filter)
                        {
                            continue;
                        }
                        let is_selected = self.data.selected.as_deref() == Some(file.path.as_path());
                        let response = widgets::nav_item(ui, is_selected, &nice, None, 0.0).on_hover_text(file.name.as_str());
                        if response.clicked() {
                            select = Some(file.path.clone());
                        }
                    }
                });
            });

        if let Some(path) = self.data.selected.clone() {
            let allow_new = self.allow_new_key;
            let Data { settings, edits, new_section, new_key, new_value, .. } = &mut self.data;
            match settings {
                None => {
                    ui.label(RichText::new("Loading...").color(MUTED));
                }
                Some(list) => {
                    let changes: Vec<(String, String, String)> = list
                        .iter()
                        .filter_map(|s| {
                            edits
                                .get(&(s.section.clone(), s.key.clone()))
                                .filter(|v| v.as_str() != s.value.as_str())
                                .map(|v| (s.section.clone(), s.key.clone(), v.clone()))
                        })
                        .collect();
                    let dirty = !changes.is_empty();

                    // The save bar stays at the bottom so it is always in reach.
                    egui::Panel::bottom("csp_savebar")
                        .frame(
                            egui::Frame::new()
                                .fill(if dirty { theme::CARD_SELECTED } else { theme::CARD })
                                .stroke(egui::Stroke::new(1.0, if dirty { ACCENT.gamma_multiply(0.7) } else { theme::BORDER }))
                                .corner_radius(egui::CornerRadius::same(12))
                                .inner_margin(egui::Margin::symmetric(14, 8)),
                        )
                        .show_separator_line(false)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.add_enabled(!busy && dirty, widgets::primary_button("Save changes")).clicked() {
                                    save = Some(changes.clone());
                                }
                                if ui.add_enabled(!busy && dirty, egui::Button::new("Discard")).clicked() {
                                    edits.clear();
                                }
                                if dirty {
                                    ui.label(RichText::new(format!("{} unsaved change(s)", changes.len())).color(WARN));
                                } else {
                                    ui.label(RichText::new("No changes").color(MUTED));
                                }
                            });
                            if allow_new {
                                ui.horizontal(|ui| {
                                    ui.label("Add setting:");
                                    ui.add(egui::TextEdit::singleline(&mut *new_section).hint_text("section").desired_width(130.0));
                                    ui.add(egui::TextEdit::singleline(&mut *new_key).hint_text("key").desired_width(150.0));
                                    ui.add(egui::TextEdit::singleline(&mut *new_value).hint_text("value").desired_width(150.0));
                                    let ready = !busy && !new_section.trim().is_empty() && !new_key.trim().is_empty();
                                    if ui.add_enabled(ready, egui::Button::new("Add")).clicked() {
                                        save = Some(vec![(
                                            new_section.trim().to_string(),
                                            new_key.trim().to_string(),
                                            new_value.trim().to_string(),
                                        )]);
                                    }
                                });
                            }
                        });
                    ui.add_space(8.0);

                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_enabled_ui(!busy, |ui| {
                            let mut index = 0;
                            while index < list.len() {
                                // One card per [section].
                                let section = list[index].section.clone();
                                let end = list[index..].iter().position(|s| s.section != section).map_or(list.len(), |n| index + n);
                                let group = &list[index..end];
                                let edited = group
                                    .iter()
                                    .filter(|s| edits.get(&(s.section.clone(), s.key.clone())).is_some_and(|v| v != &s.value))
                                    .count();
                                theme::card().inner_margin(egui::Margin::symmetric(18, 12)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(humanize(&section)).font(theme::bold(15.0)));
                                        if edited > 0 {
                                            widgets::pill(ui, &format!("{edited} changed"), ACCENT);
                                        }
                                    });
                                    ui.add_space(6.0);
                                    for s in group {
                                        let value = edits
                                            .entry((s.section.clone(), s.key.clone()))
                                            .or_insert_with(|| s.value.clone());
                                        setting_row(ui, s, value);
                                    }
                                });
                                ui.add_space(10.0);
                                index = end;
                            }
                        });
                    });
                }
            }
            if let Some(changes) = save {
                self.save_settings(egui_ctx, root, path, changes);
            }
        } else {
            widgets::empty_state(ui, "Pick a config file on the left.");
        }

        if reload {
            self.reload(Tab::Settings);
        }
        if let Some(path) = select {
            self.select_file(egui_ctx, root, path);
        }
        if compare {
            self.start_diff(egui_ctx, root, Tab::Settings);
        }
    }

    fn ffb_tab(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context, root: &Path, busy: bool) {
        let mut reload = false;
        let mut save = false;
        let mut compare = false;
        let dirty = matches!((&self.data.ffb, &self.data.ffb_disk), (Some(a), Some(b)) if a != b);
        let problems = self.data.ffb.as_ref().map(ffb::validate).unwrap_or_default();
        let can_compare = !busy && self.patches_set();

        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Reload")).clicked() {
                reload = true;
            }
            if ui
                .add_enabled(!busy && dirty && problems.is_empty(), widgets::primary_button("Save changes"))
                .clicked()
            {
                save = true;
            }
            if ui.add_enabled(!busy && dirty, egui::Button::new("Discard")).clicked() {
                self.data.ffb = self.data.ffb_disk.clone();
            }
            if ui.add_enabled(can_compare, egui::Button::new("Compare patch versions")).clicked() {
                compare = true;
            }
            if dirty {
                ui.label(RichText::new("Unsaved changes").color(WARN));
            }
        });
        show_error(ui, &self.data, Tab::Ffb);
        for problem in &problems {
            ui.label(RichText::new(problem.as_str()).color(WARN));
        }
        diff_card(ui, &self.data.diff, Tab::Ffb);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| match self.data.ffb.as_mut() {
                Some(t) => ffb_form(ui, t),
                None => {
                    ui.label(RichText::new("Not loaded.").color(MUTED));
                }
            });
        });

        if reload {
            self.reload(Tab::Ffb);
        }
        if save
            && let Some(t) = self.data.ffb.clone()
        {
            self.save_ffb(egui_ctx, root, t);
        }
        if compare {
            self.start_diff(egui_ctx, root, Tab::Ffb);
        }
    }

    fn weather_tab(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context, root: &Path, busy: bool) {
        let mut reload = false;
        let mut compare = false;
        let mut save: Option<(String, String, String)> = None;
        let mut preset_save: Option<(String, String, String, String)> = None;
        let mut picked: Option<String> = None;
        let mut create: Option<String> = None;
        let mut duplicate: Option<(String, String)> = None;
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Reload")).clicked() {
                reload = true;
            }
            if ui
                .add_enabled(!busy && self.patches_set(), egui::Button::new("Compare patch versions"))
                .clicked()
            {
                compare = true;
            }
            ui.checkbox(&mut self.allow_new_key, "Allow creating new keys");
            ui.label(
                RichText::new("Select a preset to edit its settings.ini values. Selection values can be saved.")
                    .small()
                    .color(MUTED),
            );
        });
        show_error(ui, &self.data, Tab::Weather);
        diff_card(ui, &self.data.diff, Tab::Weather);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            widgets::section_title(ui, "Weather presets (extension/weather)");
            if self.data.presets.is_empty() {
                ui.label(RichText::new(if busy { "Loading..." } else { "No presets found." }).color(MUTED));
            }
            let selected_name = self.data.selected_preset.clone();
            for preset in &self.data.presets {
                ui.horizontal(|ui| {
                    let is_selected = selected_name.as_deref() == Some(preset.name.as_str());
                    if ui.selectable_label(is_selected, preset.name.as_str()).clicked() && !is_selected && !busy {
                        picked = Some(preset.name.clone());
                    }
                    if preset.has_controller {
                        widgets::pill(ui, "has controller", OK);
                    } else {
                        widgets::pill(ui, "no controller", MUTED);
                    }
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.data.preset_name)
                        .desired_width(200.0)
                        .hint_text("New preset name"),
                );
                let name = self.data.preset_name.trim().to_string();
                let name_ok = !name.is_empty();
                if ui
                    .add_enabled(!busy && name_ok, egui::Button::new("New preset"))
                    .on_hover_text("Creates the preset with a copy of the default settings.ini")
                    .clicked()
                {
                    create = Some(name.clone());
                }
                if ui
                    .add_enabled(!busy && name_ok && selected_name.is_some(), egui::Button::new("Duplicate"))
                    .on_hover_text("Copies every file of the selected preset to the name above")
                    .clicked()
                    && let Some(from) = selected_name.clone()
                {
                    duplicate = Some((from, name));
                }
            });
            ui.add_space(12.0);

            widgets::section_title(ui, "Preset values (settings.ini of the selected preset)");
            if let Some(err) = &self.data.preset_error {
                ui.label(RichText::new(err.as_str()).color(DANGER));
            }
            let Data { selected_preset, preset_values, preset_edits, .. } = &mut self.data;
            match (selected_preset.as_deref(), preset_values.as_ref()) {
                (None, _) => {
                    ui.label(RichText::new("Select a preset to edit its values.").color(MUTED));
                }
                (Some(_), None) => {
                    ui.label(RichText::new(if busy { "Loading..." } else { "Not loaded." }).color(MUTED));
                }
                (Some(preset), Some(values)) => {
                    egui::Grid::new("preset_values")
                        .num_columns(4)
                        .spacing([14.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Section").small().color(MUTED));
                            ui.label(RichText::new("Key").small().color(MUTED));
                            ui.label(RichText::new("Value").small().color(MUTED));
                            ui.label("");
                            ui.end_row();
                            for (section, key, value) in values.iter() {
                                ui.label(RichText::new(section.as_str()).color(MUTED));
                                ui.label(key.as_str());
                                let text = preset_edits
                                    .entry((section.clone(), key.clone()))
                                    .or_insert_with(|| value.clone());
                                ui.add(egui::TextEdit::singleline(&mut *text).desired_width(200.0));
                                let pending = text.trim().to_string();
                                let changed = pending != value.as_str();
                                if ui.add_enabled(!busy && changed, egui::Button::new("Save")).clicked() {
                                    preset_save = Some((preset.to_string(), section.clone(), key.clone(), pending));
                                }
                                ui.end_row();
                            }
                        });
                    if values.is_empty() {
                        ui.label(RichText::new("No values in this preset.").color(MUTED));
                    }
                }
            }
            ui.add_space(12.0);

            widgets::section_title(ui, "Weather controllers (extension/weather-controllers)");
            if self.data.controllers.is_empty() {
                ui.label(RichText::new("No controllers found.").color(MUTED));
            }
            for controller in &self.data.controllers {
                ui.label(controller.as_str());
            }
            ui.add_space(12.0);

            widgets::section_title(ui, "Selection (extension/config/weather_fx.ini)");
            let Data { fx, weather_edits, .. } = &mut self.data;
            egui::Grid::new("weather_fx")
                .num_columns(4)
                .spacing([14.0, 6.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("Section").small().color(MUTED));
                    ui.label(RichText::new("Key").small().color(MUTED));
                    ui.label(RichText::new("Value").small().color(MUTED));
                    ui.label("");
                    ui.end_row();
                    for (section, key, value) in fx.iter() {
                        ui.label(RichText::new(section.as_str()).color(MUTED));
                        ui.label(key.as_str());
                        let text = weather_edits
                            .entry((section.clone(), key.clone()))
                            .or_insert_with(|| value.clone());
                        ui.add(egui::TextEdit::singleline(&mut *text).desired_width(200.0));
                        let pending = text.trim().to_string();
                        let changed = pending != value.as_str();
                        if ui.add_enabled(!busy && changed, egui::Button::new("Save")).clicked() {
                            save = Some((section.clone(), key.clone(), pending));
                        }
                        ui.end_row();
                    }
                });
        });

        if reload {
            self.reload(Tab::Weather);
        }
        if let Some((section, key, value)) = save {
            self.save_weather_value(egui_ctx, root, section, key, value);
        }
        if let Some(name) = picked {
            self.select_preset(egui_ctx, root, name);
        }
        if let Some((preset, section, key, value)) = preset_save {
            self.save_preset_value(egui_ctx, root, preset, section, key, value);
        }
        if let Some(name) = create {
            self.new_preset(egui_ctx, root, name);
        }
        if let Some((from, to)) = duplicate {
            self.duplicate_preset(egui_ctx, root, from, to);
        }
        if compare {
            self.start_diff(egui_ctx, root, Tab::Weather);
        }
    }

    fn gamepad_tab(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context, root: &Path, busy: bool) {
        let mut reload = false;
        let mut save = false;
        let dirty = matches!((&self.data.gamepad, &self.data.gamepad_disk), (Some(a), Some(b)) if a != b);
        let problems = self.data.gamepad.as_ref().map(gamepad::validate).unwrap_or_default();

        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Reload")).clicked() {
                reload = true;
            }
            if ui
                .add_enabled(!busy && dirty && problems.is_empty(), widgets::primary_button("Save changes"))
                .clicked()
            {
                save = true;
            }
            if ui.add_enabled(!busy && dirty, egui::Button::new("Discard")).clicked() {
                self.data.gamepad = self.data.gamepad_disk.clone();
            }
            if dirty {
                ui.label(RichText::new("Unsaved changes").color(WARN));
            }
        });
        let user_fx = csp_config::user_config_path(&gamepad::config_path(root));
        widgets::key_value(
            ui,
            "Script selection",
            &user_fx.map(|p| p.display().to_string()).unwrap_or_else(|| "Documents folder not found".into()),
        );
        widgets::key_value(
            ui,
            "Controller",
            &gamepad::controls_path().map(|p| p.display().to_string()).unwrap_or_else(|| "Documents folder not found".into()),
        );
        ui.label(
            RichText::new(
                "Advanced Gamepad Assist stores its own tuning (steering rate, slip, self-steer, \
                 shifting) in CSP's script storage. Change those in its in-game app \
                 \"Advanced Gamepad Assist Config\".",
            )
            .small()
            .color(MUTED),
        );
        show_error(ui, &self.data, Tab::Gamepad);
        for problem in &problems {
            ui.label(RichText::new(problem.as_str()).color(WARN));
        }
        ui.add_space(6.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| match self.data.gamepad.as_mut() {
                Some(s) => gamepad_form(ui, s),
                None => {
                    ui.label(RichText::new("Not loaded.").color(MUTED));
                }
            });
        });

        if reload {
            self.reload(Tab::Gamepad);
        }
        if save
            && let Some(s) = self.data.gamepad.clone()
        {
            self.save_gamepad(egui_ctx, root, s);
        }
    }

    fn classes_tab(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context, root: &Path, busy: bool) {
        let mut reload = false;
        let mut compare = false;
        let mut write: Option<ClassEdit> = None;
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Reload")).clicked() {
                reload = true;
            }
            if ui
                .add_enabled(!busy && self.patches_set(), egui::Button::new("Compare patch versions"))
                .clicked()
            {
                compare = true;
            }
            ui.label(RichText::new("Edits are backed up before each save.").small().color(MUTED));
        });
        show_error(ui, &self.data, Tab::Classes);
        diff_card(ui, &self.data.diff, Tab::Classes);

        widgets::section_title(ui, "Classes (data_car_classes.ini)");
        if self.data.classes.is_empty() {
            ui.label(RichText::new("No classes found.").color(MUTED));
        }
        // The confirm checkbox state is kept in egui memory, one entry per class id.
        let mut remove: Option<String> = None;
        for class in &self.data.classes {
            ui.horizontal(|ui| {
                widgets::pill(ui, &format!("{} · {} cars", class.id, class.cars.len()), INFO);
                let confirm_id = ui.scope_id().with(("confirm_remove", class.id.as_str()));
                let mut confirmed = ui.data(|d| d.get_temp::<bool>(confirm_id)).unwrap_or(false);
                let label = match class.cars.len() {
                    0 => "Confirm".to_string(),
                    n => format!("Unassign {n} cars"),
                };
                ui.checkbox(&mut confirmed, label);
                if ui.add_enabled(!busy && confirmed, egui::Button::new("Remove class")).clicked() {
                    remove = Some(class.id.clone());
                    confirmed = false;
                }
                ui.data_mut(|d| d.insert_temp(confirm_id, confirmed));
            });
        }
        ui.add_space(8.0);

        widgets::section_title(ui, "New class");
        // The name is kept in egui memory between frames, like the checkbox above.
        let name_id = ui.scope_id().with("new_class_name");
        let mut new_class = ui.data(|d| d.get_temp::<String>(name_id)).unwrap_or_default();
        let mut create: Option<(String, String)> = None;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut new_class)
                    .hint_text("New class")
                    .desired_width(160.0),
            );
            let name = new_class.trim().to_string();
            let car = self.data.class_car.trim().to_string();
            let ready = !busy && !name.is_empty() && !car.is_empty();
            let button = egui::Button::new("Create class");
            if ui
                .add_enabled(ready, button)
                .on_hover_text("A class exists only through its cars, so the car in the Car field above goes into it.")
                .clicked()
            {
                create = Some((name, car));
                new_class.clear();
            }
        });
        ui.data_mut(|d| d.insert_temp(name_id, new_class));
        ui.add_space(8.0);

        widgets::section_title(ui, "Edit car");
        ui.horizontal(|ui| {
            ui.label("Car");
            ui.add(
                egui::TextEdit::singleline(&mut self.data.class_car)
                    .hint_text("car id")
                    .desired_width(200.0),
            );
            ui.label("Class");
            ui.add(
                egui::TextEdit::singleline(&mut self.data.class_target)
                    .hint_text("race or street")
                    .desired_width(120.0),
            );
            let car = self.data.class_car.trim().to_string();
            let class = self.data.class_target.trim().to_string();
            let ready = !busy && !car.is_empty() && !class.is_empty();
            if ui.add_enabled(ready, egui::Button::new("Add to class")).clicked() {
                write = Some(ClassEdit::AddToClass { car, class });
            }
        });
        ui.horizontal(|ui| {
            ui.label("Year");
            ui.add(
                egui::TextEdit::singleline(&mut self.data.class_year)
                    .hint_text("year")
                    .desired_width(80.0),
            );
            let text = self.data.class_year.trim();
            let year = text.parse::<i64>().ok().filter(|y| *y > 0);
            if !text.is_empty() && year.is_none() {
                ui.label(RichText::new("Enter a positive whole number.").color(WARN));
            }
            let car = self.data.class_car.trim().to_string();
            let ready = !busy && !car.is_empty() && year.is_some();
            if ui.add_enabled(ready, egui::Button::new("Set year")).clicked()
                && let Some(year) = year
            {
                write = Some(ClassEdit::SetYear { car, year });
            }
        });
        ui.add_space(8.0);

        ui.add(
            egui::TextEdit::singleline(&mut self.data.class_query)
                .hint_text("Search car id, class or year")
                .desired_width(f32::INFINITY),
        );
        let query = self.data.class_query.trim().to_lowercase();
        let filtered: Vec<&CarRow> = self
            .data
            .car_rows
            .iter()
            .filter(|row| query.is_empty() || row.matches(&query))
            .collect();
        ui.label(
            RichText::new(format!("{} of {} cars", filtered.len(), self.data.car_rows.len()))
                .small()
                .color(MUTED),
        );
        ui.add_space(4.0);

        if filtered.is_empty() {
            let text = if self.data.car_rows.is_empty() { "No car data found." } else { "No cars match your search." };
            widgets::empty_state(ui, text);
        } else {
            ui.horizontal(|ui| {
                ui.add_sized([300.0, CLASS_ROW_HEIGHT], egui::Label::new(RichText::new("Car id").small().color(MUTED)));
                ui.add_sized([120.0, CLASS_ROW_HEIGHT], egui::Label::new(RichText::new("Class").small().color(MUTED)));
                ui.add_sized([80.0, CLASS_ROW_HEIGHT], egui::Label::new(RichText::new("Year").small().color(MUTED)));
            });
            // Clicking a car id copies its id, class and year into the edit form.
            let mut picked: Option<(String, String, String)> = None;
            let selected = self.data.class_car.trim().to_string();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
                ui,
                CLASS_ROW_HEIGHT,
                filtered.len(),
                |ui, range| {
                    for row in &filtered[range] {
                        ui.horizontal(|ui| {
                            let is_selected = row.id == selected;
                            let id_color = if is_selected { ACCENT } else { TEXT };
                            let id = egui::Label::new(RichText::new(row.id.as_str()).color(id_color))
                                .sense(egui::Sense::click());
                            if ui.add_sized([300.0, CLASS_ROW_HEIGHT], id).clicked() {
                                let year = row.year.map(|y| y.to_string()).unwrap_or_default();
                                picked = Some((row.id.clone(), row.class.clone(), year));
                            }
                            let class = if row.class.is_empty() { "—" } else { row.class.as_str() };
                            ui.add_sized([120.0, CLASS_ROW_HEIGHT], egui::Label::new(class));
                            let year = row.year.map(|y| y.to_string()).unwrap_or_else(|| "—".to_string());
                            ui.add_sized([80.0, CLASS_ROW_HEIGHT], egui::Label::new(year));
                        });
                    }
                },
            );
            if let Some((id, class, year)) = picked {
                self.data.class_car = id;
                self.data.class_target = class;
                self.data.class_year = year;
            }
        }

        if reload {
            self.reload(Tab::Classes);
        }
        if let Some(edit) = write {
            self.save_classes(egui_ctx, root, edit);
        }
        if let Some((name, car)) = create {
            let root_buf = root.to_path_buf();
            self.spawn(egui_ctx, move |tx| {
                let dir = config_dir(&root_buf);
                match car_classes::create_class(&dir, &name, &car) {
                    Ok(()) => note(tx, format!("Created class {name} with {car}."), true),
                    Err(e) => note(tx, format!("Class not created: {e}"), false),
                }
                let _ = tx.send(classes_msg(root_buf));
            });
        }
        if let Some(class) = remove {
            let root_buf = root.to_path_buf();
            self.spawn(egui_ctx, move |tx| {
                let dir = config_dir(&root_buf);
                // The confirm checkbox is the user's agreement to unassign the cars.
                match car_classes::remove_class(&dir, &class, true) {
                    Ok(n) => note(tx, format!("Removed class {class}; {n} car(s) left without a class."), true),
                    Err(e) => note(tx, format!("Class not removed: {e}"), false),
                }
                let _ = tx.send(classes_msg(root_buf));
            });
        }
        if compare {
            self.start_diff(egui_ctx, root, Tab::Classes);
        }
    }
}

impl Tool for CspConfig {
    fn name(&self) -> &'static str {
        "CSP Config"
    }

    fn group(&self) -> &'static str {
        "Config"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let egui_ctx = ui.ctx().clone();
        self.poll(notices);
        if self.root != ctx.ac_root {
            self.root = ctx.ac_root.clone();
            self.data = Data::default();
        }

        widgets::heading(
            ui,
            "CSP Config",
            "Edit Custom Shaders Patch settings, and review FFB, weather, gamepad and car class config.",
        );
        let Some(root) = self.root.clone() else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to edit CSP config.");
            return;
        };
        self.ensure(&egui_ctx, &root);
        let busy = self.pending > 0;

        ui.horizontal(|ui| {
            for (tab, label) in TABS {
                ui.selectable_value(&mut self.tab, tab, label);
            }
            if busy {
                ui.spinner();
            }
        });
        ui.add_space(6.0);

        egui::CollapsingHeader::new("Patch folders for version compare")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Each folder is an install root that contains extension/. Compare buttons stay disabled until both are set.")
                        .small()
                        .color(MUTED),
                );
                egui::Grid::new("patch_folders").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Older (v0.2.11)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.patch_v02)
                            .desired_width(480.0)
                            .hint_text("Folder path"),
                    );
                    ui.end_row();
                    ui.label("Newer (v0.3.0)");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.patch_v03)
                            .desired_width(480.0)
                            .hint_text("Folder path"),
                    );
                    ui.end_row();
                });
            });
        ui.add_space(8.0);

        self.ensure(&egui_ctx, &root);
        match self.tab {
            Tab::Settings => self.settings_tab(ui, &egui_ctx, &root, busy),
            Tab::Ffb => self.ffb_tab(ui, &egui_ctx, &root, busy),
            Tab::Weather => self.weather_tab(ui, &egui_ctx, &root, busy),
            Tab::Gamepad => self.gamepad_tab(ui, &egui_ctx, &root, busy),
            Tab::Classes => self.classes_tab(ui, &egui_ctx, &root, busy),
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn config_dir(root: &Path) -> PathBuf {
    root.join("extension").join("config")
}

/// Everything the Weather tab shows, read from the AC root `root`.
fn weather_msg(root: PathBuf) -> Msg {
    let presets = weather::list_presets(&root);
    let controllers = weather::list_controllers(&root);
    let user = csp_config::user_config_path(&weather::weather_fx_path(&root));
    let fx = weather::read_weather_fx_effective(&root, user.as_deref()).map_err(|e| e.to_string());
    Msg::Weather { root, presets, controllers, fx }
}

/// Classes and years, read from the config folder of the AC root `root`.
fn classes_msg(root: PathBuf) -> Msg {
    let dir = config_dir(&root);
    let result = match (car_classes::read_classes(&dir), car_classes::read_years(&dir)) {
        (Ok(c), Ok(y)) => Ok((c, y)),
        (Err(e), _) | (_, Err(e)) => Err(e.to_string()),
    };
    Msg::Classes { root, result }
}

fn note(tx: &Sender<Msg>, text: impl Into<String>, ok: bool) {
    let _ = tx.send(Msg::Notice(Notice { text: text.into(), ok }));
}

fn change_line(section: &str, key: &str, a: Option<String>, b: Option<String>) -> String {
    let shown = |v: Option<String>| v.unwrap_or_else(|| "(missing)".to_string());
    format!("[{section}] {key}: {} -> {}", shown(a), shown(b))
}

fn build_car_rows(classes: &[CarClass], years: &[CarYear]) -> Vec<CarRow> {
    let mut seen = HashSet::new();
    let mut ids: Vec<String> = Vec::new();
    let all = classes
        .iter()
        .flat_map(|c| c.cars.iter())
        .chain(years.iter().map(|y| &y.car_id));
    for id in all {
        if seen.insert(id.to_lowercase()) {
            ids.push(id.clone());
        }
    }
    ids.sort_by_key(|id| id.to_lowercase());
    ids.into_iter()
        .map(|id| {
            let class = car_classes::class_of(classes, &id).map(|c| c.id.clone()).unwrap_or_default();
            let year = car_classes::year_of(years, &id);
            CarRow { id, class, year }
        })
        .collect()
}

fn show_error(ui: &mut egui::Ui, data: &Data, tab: Tab) {
    if let Some(error) = data.errors.get(&tab) {
        ui.label(RichText::new(error.as_str()).color(DANGER));
    }
}

/// The result of a version compare, shown under the buttons of its tab.
fn diff_card(ui: &mut egui::Ui, diff: &Option<(Tab, Vec<String>)>, tab: Tab) {
    let Some((shown, lines)) = diff else { return };
    if *shown != tab {
        return;
    }
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, &format!("What changed ({} lines)", lines.len()));
        egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            for line in lines {
                // Added lines green, removed red, so a change reads at a glance.
                let trimmed = line.trim_start();
                let (color, tint) = if trimmed.starts_with('+') {
                    (theme::OK, theme::OK.gamma_multiply(0.10))
                } else if trimmed.starts_with('-') {
                    (theme::DANGER, theme::DANGER.gamma_multiply(0.10))
                } else {
                    (theme::MUTED, egui::Color32::TRANSPARENT)
                };
                egui::Frame::new().fill(tint).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(line.as_str()).monospace().small().color(color));
                });
            }
        });
    });
    ui.add_space(8.0);
}

/// A titled card that groups related settings.
fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::card().inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(theme::bold(15.0)));
        ui.add_space(8.0);
        add(ui);
    });
    ui.add_space(10.0);
}

fn grid(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([14.0, 8.0]).show(ui, add);
}

/// One checkbox row. `key` is the file key, shown as hover text.
fn flag(ui: &mut egui::Ui, label: &str, key: &str, value: &mut bool) {
    ui.label(label).on_hover_text(key);
    widgets::toggle(ui, value).on_hover_text(key);
    ui.end_row();
}

/// One slider row. `key` is the file key, shown as hover text.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    key: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    whole: bool,
) {
    ui.label(label).on_hover_text(key);
    let slider = egui::Slider::new(value, range);
    ui.add(if whole { slider.integer() } else { slider });
    ui.end_row();
}

fn ffb_form(ui: &mut egui::Ui, t: &mut FfbTweaks) {
    section(ui, "Basic", |ui| {
        grid(ui, "ffb_basic", |ui| {
            flag(ui, "Tweaks enabled", "[BASIC] ENABLED", &mut t.basic_enabled);
            flag(ui, "Gyro 2 enabled", "[GYRO2] ENABLED", &mut t.gyro2_enabled);
            flag(
                ui,
                "Post-processing script",
                "[POSTPROCESSING_SCRIPT] ENABLED",
                &mut t.postprocessing_script_enabled,
            );
        });
    });
    section(ui, "Post-processing", |ui| {
        grid(ui, "ffb_post", |ui| {
            slider_row(ui, "Range compression", "[POSTPROCESSING] RANGE_COMPRESSION", &mut t.range_compression, 0.5..=4.0, false);
            flag(ui, "Range compression assist", "[POSTPROCESSING] RANGE_COMPRESSION_ASSIST", &mut t.range_compression_assist);
            flag(ui, "Clean FFB", "[POSTPROCESSING] CLEAN_FFB", &mut t.clean_ffb);
            slider_row(ui, "Dirt vibration", "[POSTPROCESSING] DIRT_VIBRATION", &mut t.dirt_vibration, 0.0..=1.5, false);
        });
    });
    section(ui, "Custom soft lock", |ui| {
        grid(ui, "ffb_softlock", |ui| {
            flag(ui, "Soft lock enabled", "[CUSTOM_SOFT_LOCK] ENABLED", &mut t.soft_lock_enabled);
            slider_row(ui, "Padding (degrees)", "[CUSTOM_SOFT_LOCK] PADDING", &mut t.soft_lock_padding, 4.0..=20.0, false);
            slider_row(ui, "Shift padding", "[CUSTOM_SOFT_LOCK] SHIFT_PADDING", &mut t.soft_lock_shift_padding, 0.0..=1.0, false);
            slider_row(ui, "Force factor", "[CUSTOM_SOFT_LOCK] FORCE_FACTOR", &mut t.soft_lock_force_factor, 0.0..=2.0, false);
            slider_row(ui, "Speed factor", "[CUSTOM_SOFT_LOCK] SPEED_FACTOR", &mut t.soft_lock_speed_factor, 0.0..=2.0, false);
            slider_row(ui, "Damper", "[CUSTOM_SOFT_LOCK] DAMPER", &mut t.soft_lock_damper, 0.0..=1.0, false);
        });
    });
    section(ui, "Real feel", |ui| {
        grid(ui, "ffb_realfeel", |ui| {
            flag(ui, "Real feel enabled", "[REAL_FEEL] ENABLED", &mut t.real_feel_enabled);
            slider_row(ui, "Wheel max (Nm)", "[REAL_FEEL] WHEEL_MAX_NM", &mut t.real_feel_wheel_max_nm, 2.0..=35.0, false);
            flag(ui, "Enable for all cars", "[REAL_FEEL] ENABLE_FOR_ALL", &mut t.real_feel_enable_for_all);
        });
    });
    section(ui, "Real feel brake", |ui| {
        grid(ui, "ffb_brake", |ui| {
            flag(ui, "Brake enabled", "[REAL_FEEL_BRAKE] ENABLED", &mut t.real_feel_brake_enabled);
            slider_row(ui, "Brake max (kg)", "[REAL_FEEL_BRAKE] BRAKE_MAX_KG", &mut t.brake_max_kg, 0.0..=200.0, false);
            slider_row(ui, "Brake max inactive (%)", "[REAL_FEEL_BRAKE] BRAKE_MAX_INACTIVE_PERC", &mut t.brake_max_inactive_perc, 1.0..=100.0, false);
            flag(ui, "Clamp forces", "[REAL_FEEL_BRAKE] CLAMP_FORCES", &mut t.brake_clamp_forces);
        });
    });
    section(ui, "Tweaks", |ui| {
        grid(ui, "ffb_tweaks", |ui| {
            slider_row(ui, "Adaptive FFB", "[TWEAKS] ADAPTIVE_FFB (v0.3.0)", &mut t.adaptive_ffb, 0.0..=3.0, true);
            flag(ui, "Average skipped FFB", "[TWEAKS] AVERAGE_SKIPPED_FFB", &mut t.average_skipped_ffb);
            slider_row(ui, "Skip damping frames", "[TWEAKS] SKIP_DAMPING_FRAMES", &mut t.skip_damping_frames, 0.0..=5.0, true);
            flag(ui, "Strut FFB adjustment", "[TWEAKS] STRUT_FFB_ADJUSTMENT (v0.2.11)", &mut t.strut_ffb_adjustment);
            flag(ui, "Fade in after loading", "[TWEAKS] FADE_IN_AFTER_LOADING (v0.2.11)", &mut t.fade_in_after_loading);
        });
    });
}

fn gamepad_form(ui: &mut egui::Ui, s: &mut PadSetup) {
    section(ui, "Gamepad script (CSP)", |ui| {
        let status = if s.aga_active() {
            "Advanced Gamepad Assist is active.".to_string()
        } else if s.script_enabled && !s.implementation.is_empty() {
            format!("Active script: {}", s.implementation)
        } else {
            "No gamepad script is active.".to_string()
        };
        ui.label(RichText::new(status).color(if s.aga_active() { OK } else { MUTED }));
        ui.horizontal(|ui| {
            if ui.add_enabled(!s.aga_active(), egui::Button::new("Use Advanced Gamepad Assist")).clicked() {
                s.select_aga();
            }
            if ui.add_enabled(s.script_enabled, egui::Button::new("Turn the script off")).clicked() {
                s.script_enabled = false;
            }
        });
        grid(ui, "gp_input", |ui| {
            flag(ui, "Gamepad input method (X360)", "INPUT_METHOD", &mut s.gamepad_input);
        });
    });
    section(ui, "Controller (controls.ini [X360])", |ui| {
        grid(ui, "gp_x360", |ui| {
            slider_row(ui, "Steering gamma", "STEER_GAMMA", &mut s.steer_gamma, gamepad::GAMMA_RANGE, false);
            slider_row(ui, "Steering deadzone", "STEER_DEADZONE", &mut s.steer_deadzone, gamepad::DEADZONE_RANGE, false);
            slider_row(ui, "Rumble", "RUMBLE_INTENSITY", &mut s.rumble, gamepad::RUMBLE_RANGE, false);
        });
    });
}

/// `rain_fx.ini` becomes `Rain fx`.
fn friendly_name(file: &str) -> String {
    humanize(file.strip_suffix(".ini").unwrap_or(file))
}

/// `SOME_KEY_NAME` / `some_key` becomes `Some key name`.
fn humanize(raw: &str) -> String {
    let spaced = raw.replace(['_', '.'], " ");
    let lower = spaced.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// One setting: readable name, a control that fits the value, the old value when changed.
fn setting_row(ui: &mut egui::Ui, setting: &acxs_manager::csp_config::Setting, value: &mut String) {
    let original = setting.value.clone();
    let changed = *value != original;
    ui.horizontal(|ui| {
        // Change marker.
        let (marker, _) = ui.allocate_exact_size(egui::vec2(4.0, 22.0), egui::Sense::hover());
        if changed {
            ui.painter().rect_filled(marker, egui::CornerRadius::same(2), ACCENT);
        }
        ui.allocate_ui_with_layout(
            egui::vec2(280.0, 28.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let label = ui.add(
                    egui::Label::new(RichText::new(humanize(&setting.key)).color(if changed { ACCENT } else { TEXT })).truncate(),
                );
                label.on_hover_text(match &setting.doc {
                    Some(doc) => format!("{}\n\n{doc}", setting.key),
                    None => setting.key.clone(),
                });
            },
        );

        let lower = value.trim().to_lowercase();
        let is_bool = matches!(lower.as_str(), "0" | "1" | "true" | "false" | "on" | "off");
        let number: Option<f64> = value.trim().parse().ok();
        if is_bool {
            let mut on = matches!(lower.as_str(), "1" | "true" | "on");
            if widgets::toggle(ui, &mut on).changed() {
                // Keep the file's own spelling of booleans.
                *value = match (lower.as_str(), on) {
                    ("true" | "false", _) => if on { "true" } else { "false" }.to_string(),
                    ("on" | "off", _) => if on { "on" } else { "off" }.to_string(),
                    _ => if on { "1" } else { "0" }.to_string(),
                };
            }
        } else if let Some(mut n) = number {
            let whole = !value.contains('.');
            let speed = if whole { 1.0 } else { (n.abs() * 0.01).max(0.001) };
            if ui.add(egui::DragValue::new(&mut n).speed(speed).max_decimals(if whole { 0 } else { 4 })).changed() {
                *value = if whole { format!("{}", n.round() as i64) } else { trim_float(n) };
            }
        } else {
            ui.add(egui::TextEdit::singleline(value).desired_width(260.0).margin(egui::Margin::symmetric(8, 5)));
        }

        if changed {
            ui.label(RichText::new(format!("was {}", if original.is_empty() { "empty" } else { original.as_str() })).size(12.0).color(MUTED));
            if widgets::glyph_button(ui, "↺", "Revert this setting") {
                *value = original;
            }
        }
    });
    if let Some(doc) = &setting.doc {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.add(egui::Label::new(RichText::new(doc.as_str()).size(11.5).color(theme::FAINT)).truncate());
        });
    }
}

fn trim_float(n: f64) -> String {
    let text = format!("{n:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}
