//! Quick Race tool page: pick a track, car and rules, write `cfg/race.ini`
//! and start Assetto Corsa. Named presets of the form live next to the game.
//!
//! Scanning, preset file access, the "is the game running" check and the
//! launch all run on background threads. Results come back through a channel
//! and are applied in [`QuickRacePage::poll`] on each frame.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::content::{self, scan_cars, scan_tracks, CarEntry, TrackEntry};
use acxs_manager::game;
use acxs_manager::presets::{self, PresetStore};
use acxs_manager::race::{self, QuickRace};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Highest AI count the game accepts (the player plus 39 AI cars).
const MAX_AI_COUNT: u32 = 39;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(QuickRacePage {
        form: QuickRace::default(),
        check_requested: false,
        track_query: String::new(),
        car_query: String::new(),
        loaded_root: None,
        cars: Vec::new(),
        tracks: Vec::new(),
        scanned: false,
        presets: None,
        presets_error: None,
        preset_name: String::new(),
        selected_preset: None,
        jobs: 0,
        tx,
        rx,
    })
}

struct QuickRacePage {
    /// The race being edited.
    form: QuickRace,
    /// Set once Validate (or a launch attempt) was pressed; problems are shown from then on.
    check_requested: bool,
    track_query: String,
    car_query: String,
    /// The AC root this page currently reflects. `None` when there is no valid install.
    loaded_root: Option<PathBuf>,
    cars: Vec<CarEntry>,
    tracks: Vec<TrackEntry>,
    /// True once the installed content for `loaded_root` has been scanned.
    scanned: bool,
    /// Presets as last saved or loaded. `None` until loaded, or when the file could not be read.
    presets: Option<PresetStore>,
    presets_error: Option<String>,
    preset_name: String,
    selected_preset: Option<String>,
    /// Background jobs still running. While non-zero the page disables its actions.
    jobs: usize,
    tx: Sender<Done>,
    rx: Receiver<Done>,
}

/// Results of background work.
enum Done {
    Library {
        root: PathBuf,
        cars: Vec<CarEntry>,
        tracks: Vec<TrackEntry>,
    },
    PresetsLoaded {
        root: PathBuf,
        result: Result<PresetStore, String>,
    },
    PresetsSaved {
        root: PathBuf,
        /// On success: the stored presets and the notice text to show.
        outcome: Result<(PresetStore, String), String>,
    },
    Launched(Notice),
}

/// A user action collected while drawing, applied after the frame's layout.
enum Action {
    Validate,
    Launch,
    Rescan,
    Load(String),
    Delete(String),
    Save,
}

impl Tool for QuickRacePage {
    fn name(&self) -> &'static str {
        "Quick Race"
    }

    fn group(&self) -> &'static str {
        "Launch"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let egui_ctx = ui.ctx().clone();
        self.sync_root(ctx, &egui_ctx);
        self.poll(notices);

        widgets::heading(
            ui,
            "Quick Race",
            "Pick a track and car, set the rules, then write race.ini and launch.",
        );

        let Some(root) = self.loaded_root.clone() else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to use Quick Race.");
            return;
        };

        let busy = self.jobs > 0;
        let mut action = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.columns(2, |cols| {
                    if let [left, right] = cols {
                        let from_form = form_card(left, self, busy);
                        let from_presets = presets_card(right, self, busy);
                        action = from_form.or(from_presets);
                    }
                });
            });

        if let Some(action) = action {
            self.apply(action, &root, notices, &egui_ctx);
        }
    }
}

impl QuickRacePage {
    /// Starts a scan and a preset load whenever the usable AC root changes.
    fn sync_root(&mut self, ctx: &ToolContext, egui_ctx: &egui::Context) {
        let root = if ctx.ac_valid { ctx.ac_root.clone() } else { None };
        if root == self.loaded_root {
            return;
        }
        self.loaded_root = root.clone();
        self.cars.clear();
        self.tracks.clear();
        self.scanned = false;
        self.presets = None;
        self.presets_error = None;
        self.selected_preset = None;
        if let Some(root) = root {
            self.start_scan(&root, egui_ctx);
            self.start_presets_load(&root, egui_ctx);
        }
    }

    /// Runs `work` on a background thread and wakes the UI when it finishes.
    fn spawn(&mut self, egui_ctx: &egui::Context, work: impl FnOnce() -> Done + Send + 'static) {
        self.jobs += 1;
        let tx = self.tx.clone();
        let egui_ctx = egui_ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(work());
            egui_ctx.request_repaint();
        });
    }

    fn start_scan(&mut self, root: &Path, egui_ctx: &egui::Context) {
        let root = root.to_path_buf();
        self.spawn(egui_ctx, move || Done::Library {
            cars: scan_cars(&root),
            tracks: scan_tracks(&root),
            root,
        });
    }

    fn start_presets_load(&mut self, root: &Path, egui_ctx: &egui::Context) {
        let root = root.to_path_buf();
        self.spawn(egui_ctx, move || {
            let path = presets::default_path(&root);
            let result = PresetStore::load(&path).map_err(|err| err.to_string());
            Done::PresetsLoaded { root, result }
        });
    }

    /// Writes the presets file in the background. The in-memory store is only
    /// replaced once the file has been saved.
    fn save_presets(
        &mut self,
        root: PathBuf,
        next: PresetStore,
        success: String,
        egui_ctx: &egui::Context,
    ) {
        self.spawn(egui_ctx, move || {
            let path = presets::default_path(&root);
            let outcome = match next.save(&path) {
                Ok(()) => Ok((next, success)),
                Err(err) => Err(format!("Could not save presets: {err}")),
            };
            Done::PresetsSaved { root, outcome }
        });
    }

    /// Writes race.ini and starts the game, unless the game is already running.
    fn launch(&mut self, root: PathBuf, notices: &mut Vec<Notice>, egui_ctx: &egui::Context) {
        self.check_requested = true;
        let problems = self.problems();
        if !problems.is_empty() {
            notices.push(Notice {
                text: format!("Not launched. {}", problems.join(" ")),
                ok: false,
            });
            return;
        }

        let form = self.form.clone();
        self.spawn(egui_ctx, move || {
            // The tasklist check blocks, so it happens here and not on the UI thread.
            let notice = if game::is_game_running() {
                Notice {
                    text: "Assetto Corsa is already running. Close it before writing race.ini."
                        .to_string(),
                    ok: false,
                }
            } else {
                match race::write_race_ini(&root, &form) {
                    Err(err) => Notice {
                        text: format!("Could not write race.ini. {err}"),
                        ok: false,
                    },
                    Ok(path) => match game::launch_game(&root) {
                        Ok(_child) => Notice {
                            text: format!("Wrote {} and launched Assetto Corsa.", path.display()),
                            ok: true,
                        },
                        Err(err) => Notice {
                            text: format!(
                                "Wrote {} but Assetto Corsa did not start. {err}",
                                path.display()
                            ),
                            ok: false,
                        },
                    },
                }
            };
            Done::Launched(notice)
        });
    }

    /// Validation problems for the current form: the pure `race::validate` checks,
    /// plus track, layout and car names that are not installed (once scanned).
    fn problems(&self) -> Vec<String> {
        let mut problems = self.form.validate().err().unwrap_or_default();
        if self.scanned {
            if !self.form.track.trim().is_empty() {
                match self.tracks.iter().find(|t| t.id == self.form.track) {
                    None => problems.push(format!("Track \"{}\" is not installed.", self.form.track)),
                    Some(track) => {
                        if let Some(layout) = &self.form.track_layout
                            && !track.layouts.contains(layout)
                        {
                            problems.push(format!(
                                "Layout \"{layout}\" is not installed for {}.",
                                track.id
                            ));
                        }
                    }
                }
            }
            if !self.form.car.trim().is_empty()
                && !self.cars.iter().any(|c| c.id == self.form.car)
            {
                problems.push(format!("Car \"{}\" is not installed.", self.form.car));
            }
        }
        problems
    }

    /// Applies results from background jobs. Results for an AC root that is no
    /// longer loaded are dropped.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(done) = self.rx.try_recv() {
            self.jobs = self.jobs.saturating_sub(1);
            match done {
                Done::Library { root, cars, tracks } => {
                    if self.loaded_root.as_ref() == Some(&root) {
                        self.cars = cars;
                        self.tracks = tracks;
                        self.scanned = true;
                    }
                }
                Done::PresetsLoaded { root, result } => {
                    if self.loaded_root.as_ref() == Some(&root) {
                        match result {
                            Ok(store) => {
                                self.presets = Some(store);
                                self.presets_error = None;
                            }
                            Err(err) => {
                                self.presets = None;
                                self.presets_error = Some(format!(
                                    "{err}. Presets are unavailable; the file was left unchanged."
                                ));
                            }
                        }
                    }
                }
                Done::PresetsSaved { root, outcome } => {
                    if self.loaded_root.as_ref() == Some(&root) {
                        match outcome {
                            Ok((store, text)) => {
                                self.presets = Some(store);
                                notices.push(Notice { text, ok: true });
                            }
                            Err(text) => notices.push(Notice { text, ok: false }),
                        }
                    }
                }
                Done::Launched(notice) => notices.push(notice),
            }
        }
    }

    fn apply(&mut self, action: Action, root: &Path, notices: &mut Vec<Notice>, egui_ctx: &egui::Context) {
        match action {
            Action::Validate => self.check_requested = true,
            Action::Launch => self.launch(root.to_path_buf(), notices, egui_ctx),
            Action::Rescan => self.start_scan(root, egui_ctx),
            Action::Load(name) => {
                if let Some(preset) = self.presets.as_ref().and_then(|store| store.get(&name)) {
                    self.form = preset.race.clone();
                    self.preset_name = preset.name.clone();
                    notices.push(Notice {
                        text: format!("Loaded preset {}.", preset.name),
                        ok: true,
                    });
                }
            }
            Action::Delete(name) => {
                let Some(mut next) = self.presets.clone() else {
                    return;
                };
                if next.remove(&name) {
                    self.selected_preset = None;
                    let text = format!("Deleted preset {name}. It is kept in the removed list in the file.");
                    self.save_presets(root.to_path_buf(), next, text, egui_ctx);
                }
            }
            Action::Save => {
                let name = self.preset_name.trim().to_string();
                let Some(mut next) = self.presets.clone() else {
                    return;
                };
                match next.upsert(&name, self.form.clone()) {
                    Ok(()) => {
                        self.selected_preset = Some(name.clone());
                        let text = format!("Saved preset {name}.");
                        self.save_presets(root.to_path_buf(), next, text, egui_ctx);
                    }
                    Err(err) => notices.push(Notice {
                        text: format!("Preset not saved. {err}"),
                        ok: false,
                    }),
                }
            }
        }
    }
}

/// The race settings and the Validate / launch buttons.
fn form_card(ui: &mut egui::Ui, page: &mut QuickRacePage, busy: bool) -> Option<Action> {
    let problems = if page.check_requested { page.problems() } else { Vec::new() };
    let QuickRacePage {
        form,
        check_requested,
        track_query,
        car_query,
        tracks,
        cars,
        scanned,
        ..
    } = page;
    let mut action = None;

    theme::card().show(ui, |ui| {
        ui.spacing_mut().slider_width = 200.0;

        widgets::section_title(ui, "Track");
        ui.add(
            egui::TextEdit::singleline(&mut *track_query)
                .hint_text("Filter tracks")
                .desired_width(f32::INFINITY),
        );
        let shown_tracks = content::filter_tracks(tracks, track_query.as_str());
        let current_track = tracks.iter().find(|t| t.id == form.track);
        let track_label = match current_track {
            Some(track) => track.name.clone(),
            None if form.track.trim().is_empty() => "Choose a track".to_string(),
            None if *scanned => format!("{} (not installed)", form.track),
            None => form.track.clone(),
        };
        let mut picked_track = None;
        egui::ComboBox::from_id_salt("quick_race_track")
            .width(ui.available_width())
            .height(320.0)
            .selected_text(track_label)
            .show_ui(ui, |ui| {
                for track in &shown_tracks {
                    if ui.selectable_label(form.track == track.id, &track.name).clicked() {
                        picked_track = Some(track.id.clone());
                    }
                }
            });
        if let Some(id) = picked_track
            && id != form.track
        {
            form.track = id;
            form.track_layout = None;
        }

        if let Some(track) = current_track
            && !track.layouts.is_empty()
        {
            ui.add_space(4.0);
            ui.label(RichText::new("Layout").color(MUTED));
            let layout_label = form
                .track_layout
                .clone()
                .unwrap_or_else(|| "Default layout".to_string());
            egui::ComboBox::from_id_salt("quick_race_layout")
                .selected_text(layout_label)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(form.track_layout.is_none(), "Default layout").clicked() {
                        form.track_layout = None;
                    }
                    for layout in &track.layouts {
                        let selected = form.track_layout.as_deref() == Some(layout.as_str());
                        if ui.selectable_label(selected, layout).clicked() {
                            form.track_layout = Some(layout.clone());
                        }
                    }
                });
        }
        ui.add_space(10.0);

        widgets::section_title(ui, "Car");
        ui.add(
            egui::TextEdit::singleline(&mut *car_query)
                .hint_text("Filter cars")
                .desired_width(f32::INFINITY),
        );
        let shown_cars = content::filter_cars(cars, car_query.as_str());
        let car_label = match cars.iter().find(|c| c.id == form.car) {
            Some(car) => car.name.clone(),
            None if form.car.trim().is_empty() => "Choose a car".to_string(),
            None if *scanned => format!("{} (not installed)", form.car),
            None => form.car.clone(),
        };
        let mut picked_car = None;
        egui::ComboBox::from_id_salt("quick_race_car")
            .width(ui.available_width())
            .height(320.0)
            .selected_text(car_label)
            .show_ui(ui, |ui| {
                for car in &shown_cars {
                    if ui.selectable_label(form.car == car.id, &car.name).clicked() {
                        picked_car = Some(car.id.clone());
                    }
                }
            });
        if let Some(id) = picked_car {
            form.car = id;
        }
        ui.add_space(12.0);

        widgets::section_title(ui, "Rules");
        let max_grid = form.ai_count.min(MAX_AI_COUNT);
        form.grid_position = form.grid_position.min(form.ai_count);
        egui::Grid::new("quick_race_fields")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                ui.label("Player name");
                ui.add(egui::TextEdit::singleline(&mut form.player_name).desired_width(220.0));
                ui.end_row();

                ui.label("Skin");
                ui.add(
                    egui::TextEdit::singleline(&mut form.skin)
                        .hint_text("Empty for the game default")
                        .desired_width(220.0),
                );
                ui.end_row();

                ui.label("AI cars");
                ui.add(egui::Slider::new(&mut form.ai_count, 0..=MAX_AI_COUNT));
                ui.end_row();

                ui.label("Grid position");
                ui.add(egui::Slider::new(&mut form.grid_position, 0..=max_grid));
                ui.end_row();

                ui.label("Laps");
                ui.add(egui::DragValue::new(&mut form.laps).range(0_u32..=999));
                ui.end_row();

                ui.label("Weather");
                ui.add(egui::TextEdit::singleline(&mut form.weather).desired_width(220.0));
                ui.end_row();

                ui.label("Time of day");
                ui.add(egui::Slider::new(&mut form.time_of_day_hour, 0..=23).suffix(":00"));
                ui.end_row();

                ui.label("AI level");
                ui.add(egui::Slider::new(&mut form.ai_level, 0..=100));
                ui.end_row();

                ui.label("AI aggression");
                ui.horizontal(|ui| {
                    let mut on = form.ai_aggression.is_some();
                    if ui.checkbox(&mut on, "").changed() {
                        form.ai_aggression = on.then_some(50);
                    }
                    match form.ai_aggression.as_mut() {
                        Some(value) => {
                            ui.add(egui::Slider::new(value, 0..=100));
                        }
                        None => {
                            ui.label(RichText::new("Game default").color(MUTED));
                        }
                    }
                });
                ui.end_row();
            });
        ui.add_space(12.0);

        if *check_requested {
            if problems.is_empty() {
                ui.label(RichText::new("Race settings are valid.").color(OK));
            } else {
                for problem in &problems {
                    ui.label(RichText::new(problem).color(WARN));
                }
            }
            ui.add_space(6.0);
        }

        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Validate")).clicked() {
                action = Some(Action::Validate);
            }
            if ui.add_enabled(!busy, egui::Button::new("Rescan content")).clicked() {
                action = Some(Action::Rescan);
            }
            let launch = widgets::primary_button("Write race.ini and launch");
            if ui.add_enabled(!busy && *scanned, launch).clicked() {
                action = Some(Action::Launch);
            }
            if busy {
                ui.spinner();
            }
        });
    });

    action
}

/// The preset list, load / delete, and saving the current form under a name.
fn presets_card(ui: &mut egui::Ui, page: &mut QuickRacePage, busy: bool) -> Option<Action> {
    let QuickRacePage {
        form: _,
        presets,
        presets_error,
        preset_name,
        selected_preset,
        ..
    } = page;
    let mut action = None;
    let mut clicked = None;

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Presets");
        if let Some(err) = presets_error.as_deref() {
            ui.label(RichText::new(err).color(DANGER));
            return;
        }
        let Some(store) = presets.as_ref() else {
            ui.label(RichText::new("Loading presets...").color(MUTED));
            return;
        };

        if store.list().is_empty() {
            ui.label(RichText::new("No presets saved yet.").color(MUTED));
        } else {
            egui::ScrollArea::vertical()
                .id_salt("quick_race_presets")
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for preset in store.list() {
                        let selected = selected_preset.as_deref() == Some(preset.name.as_str());
                        let summary = format!(
                            "{} | {} | {} laps | {} AI",
                            preset.race.track, preset.race.car, preset.race.laps, preset.race.ai_count
                        );
                        let picked = widgets::list_row(ui, selected, |ui| {
                            ui.label(RichText::new(&preset.name).strong());
                            ui.label(RichText::new(summary).small().color(MUTED));
                        });
                        if picked {
                            clicked = Some(preset.name.clone());
                        }
                    }
                });
        }
        if let Some(name) = clicked {
            *selected_preset = Some(name);
        }
        ui.add_space(6.0);

        let selected_name = selected_preset.clone();
        let has_selection = selected_name.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!busy && has_selection, widgets::primary_button("Load into form"))
                .clicked()
                && let Some(name) = &selected_name
            {
                action = Some(Action::Load(name.clone()));
            }
            if ui
                .add_enabled(!busy && has_selection, egui::Button::new("Delete"))
                .clicked()
                && let Some(name) = &selected_name
            {
                action = Some(Action::Delete(name.clone()));
            }
        });
        ui.add_space(12.0);

        widgets::section_title(ui, "Save current race as");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut *preset_name)
                    .hint_text("Preset name")
                    .desired_width(200.0),
            );
            let can_save = !busy && !preset_name.trim().is_empty();
            if ui.add_enabled(can_save, egui::Button::new("Save")).clicked() {
                action = Some(Action::Save);
            }
        });

        let removed = store.removed().len();
        if removed > 0 {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{removed} removed preset(s) are kept in the presets file."))
                    .small()
                    .color(MUTED),
            );
        }
    });

    action
}
