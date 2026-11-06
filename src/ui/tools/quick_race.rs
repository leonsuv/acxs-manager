//! Quick Race tool page: pick a track, car and rules, write `cfg/race.ini`
//! and start Assetto Corsa. Named presets of the form live next to the game.
//!
//! Scanning, preset file access, the "is the game running" check and the
//! launch all run on background threads. Results come back through a channel
//! and are applied in [`QuickRacePage::poll`] on each frame.

use super::{PickKind, Tool, ToolContext};
use crate::ui::theme::{self, DANGER, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::content::{scan_cars, scan_tracks, CarEntry, TrackEntry};
use acxs_manager::game;
use acxs_manager::presets::{self, PresetStore};
use acxs_manager::race::{self, QuickRace};
use acxs_manager::race_options::RaceMode;
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
        loaded_root: None,
        cars: Vec::new(),
        tracks: Vec::new(),
        scanned: false,
        weathers: Vec::new(),
        pick: None,
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
    /// The AC root this page currently reflects. `None` when there is no valid install.
    loaded_root: Option<PathBuf>,
    cars: Vec<CarEntry>,
    tracks: Vec<TrackEntry>,
    /// True once the installed content for `loaded_root` has been scanned.
    scanned: bool,
    /// Weather types installed in `content/weather`.
    weathers: Vec<String>,
    /// Set when the user pressed "Choose track" or "Choose car"; taken by the app.
    pick: Option<super::PickKind>,
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
    fn take_pick(&mut self) -> Option<PickKind> {
        self.pick.take()
    }

    fn set_race(&mut self, track: Option<&str>, layout: Option<&str>, car: Option<&str>, skin: Option<&str>) {
        if let Some(track) = track {
            self.form.track = track.to_string();
            self.form.track_layout = layout.map(str::to_string);
        }
        if let Some(car) = car {
            self.form.car = car.to_string();
            self.form.skin = skin.unwrap_or("").to_string();
        }
    }

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
            "Choose a track and car, set up the weekend and your aids, then start.",
        );

        let Some(root) = self.loaded_root.clone() else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to use Quick Race.");
            return;
        };

        let busy = self.jobs > 0;
        let mut from_presets = None;
        egui::Panel::right("quick_race_presets")
            .exact_size(340.0)
            .resizable(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 0, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                from_presets = presets_card(ui, self, busy);
            });
        let mut from_form = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                from_form = form_card(ui, self, busy);
            });
        let action = from_form.or(from_presets);

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
        self.weathers = root.as_deref().map(list_weather).unwrap_or_default();
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
                let written = acxs_manager::settings::ac_user_cfg_dir()
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "the Documents folder was not found")
                    })
                    .and_then(|cfg| {
                        let path = race::write_race_ini(&cfg, &form)?;
                        if form.extras.write_assists {
                            race::write_assists_ini(&cfg, &form.extras.assists)?;
                        }
                        Ok(path)
                    });
                match written {
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
            if let Some(root) = &self.loaded_root
                && self.form.extras.mode.needs_racing_line()
                && !race::has_racing_line(root, &self.form.track, self.form.track_layout.as_deref())
            {
                problems.push("Time attack needs a track with a racing line. This layout has none.".to_string());
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
/// A titled card with the page's usual spacing.
fn card(ui: &mut egui::Ui, title: &str, hint: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::card().inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(theme::bold(16.0)));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).size(12.5).color(MUTED));
        }
        ui.add_space(8.0);
        add(ui);
    });
    ui.add_space(12.0);
}

/// One labelled setting: a fixed-width label, then the control.
fn setting(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(160.0, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(RichText::new(label).color(MUTED));
        });
        add(ui);
    });
}

/// A drop-down over `(value, label)` pairs.
fn pick_value<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[(T, &str)]) {
    let current = options.iter().find(|(v, _)| v == value).map_or("Custom", |(_, label)| *label);
    egui::ComboBox::from_id_salt(id).selected_text(current).show_ui(ui, |ui| {
        for (option, label) in options {
            ui.selectable_value(value, *option, *label);
        }
    });
}

/// A picture card for the track or the car with its "Choose" button. True when the button was pressed.
fn pick_card(ui: &mut egui::Ui, width: f32, kind: &str, title: &str, subtitle: &str, image: Option<PathBuf>, button: &str) -> bool {
    use crate::ui::media;
    let mut pressed = false;
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        let height = (width * 9.0 / 16.0).min(200.0).round();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        media::paint_image(ui, rect, image.as_deref(), title, theme::INFO, egui::CornerRadius::same(12));
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(12),
            egui::Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );
        media::chip(ui, rect.left_top() + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, kind, theme::TEXT);
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(title).font(theme::bold(16.0))).truncate());
        ui.add(egui::Label::new(RichText::new(subtitle).size(12.5).color(MUTED)).truncate());
        ui.add_space(4.0);
        if ui.add(widgets::primary_button(button).min_size(egui::vec2(width, 34.0))).clicked() {
            pressed = true;
        }
    });
    pressed
}

fn humanize_weather(id: &str) -> String {
    // "3_clear" becomes "Clear", "7_heavy_clouds" "Heavy clouds".
    let name = id.split_once('_').map_or(id, |(_, rest)| rest).replace('_', " ");
    let mut chars = name.chars();
    chars.next().map_or(String::new(), |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

fn form_card(ui: &mut egui::Ui, page: &mut QuickRacePage, busy: bool) -> Option<Action> {
    let problems = if page.check_requested { page.problems() } else { Vec::new() };
    let QuickRacePage { form, check_requested, tracks, cars, scanned, weathers, pick, .. } = page;
    let mut action = None;
    ui.spacing_mut().slider_width = 220.0;

    // Track and car: pictures, and a button that opens the Tracks or Cars page to choose.
    {
        use crate::ui::media;
        let track = tracks.iter().find(|t| t.id == form.track);
        let track_title = match track {
            Some(t) => t.name.clone(),
            None if form.track.trim().is_empty() => "No track chosen".to_string(),
            None if *scanned => format!("{} (not installed)", form.track),
            None => form.track.clone(),
        };
        let layout = form.track_layout.as_deref().map(|l| l.strip_prefix("layout_").unwrap_or(l).to_string());
        let track_sub = match (&layout, track) {
            (Some(l), _) => format!("Layout: {l}"),
            (None, Some(t)) => crate::ui::pages::join_parts(&[t.country.as_str(), t.city.as_str()]),
            _ => String::new(),
        };
        let track_img = track.and_then(|t| media::track_preview(t, form.track_layout.as_deref()));

        let car = cars.iter().find(|c| c.id == form.car);
        let car_title = match car {
            Some(c) => c.name.clone(),
            None if form.car.trim().is_empty() => "No car chosen".to_string(),
            None if *scanned => format!("{} (not installed)", form.car),
            None => form.car.clone(),
        };
        let skin = Some(form.skin.trim()).filter(|s| !s.is_empty());
        let car_sub = match (car, skin) {
            (Some(c), Some(s)) => format!("{}  ·  skin {s}", c.brand),
            (Some(c), None) => format!("{}  ·  default skin", c.brand),
            _ => String::new(),
        };
        let car_img = car.and_then(|c| media::car_preview(&c.path, skin));

        let width = ((ui.available_width() - 14.0) / 2.0).floor();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            if pick_card(ui, width, "Track", &track_title, &track_sub, track_img, "Choose track") {
                *pick = Some(PickKind::Track);
            }
            if pick_card(ui, width, "Car", &car_title, &car_sub, car_img, "Choose car and skin") {
                *pick = Some(PickKind::Car);
            }
        });
        ui.add_space(14.0);
    }

    let x = &mut form.extras;

    card(ui, "Mode", "A race weekend has opponents. The other modes are driven alone.", |ui| {
        ui.horizontal(|ui| {
            for mode in RaceMode::ALL {
                ui.selectable_value(&mut x.mode, mode, mode.label());
            }
        });
    });
    ui.add_space(14.0);

    let weekend = x.mode == RaceMode::Weekend;

    if weekend {
    card(ui, "Sessions", "Played in this order. Switch off what you do not want.", |ui| {
        let mut practice_on = x.practice_minutes > 0;
        setting(ui, "Practice", |ui| {
            if widgets::toggle(ui, &mut practice_on).changed() {
                x.practice_minutes = if practice_on { 20 } else { 0 };
            }
            if practice_on {
                ui.add(egui::DragValue::new(&mut x.practice_minutes).range(1_u32..=600).suffix(" min"));
            }
        });
        let mut qualify_on = x.qualify_minutes > 0;
        setting(ui, "Qualifying", |ui| {
            if widgets::toggle(ui, &mut qualify_on).changed() {
                x.qualify_minutes = if qualify_on { 10 } else { 0 };
            }
            if qualify_on {
                ui.add(egui::DragValue::new(&mut x.qualify_minutes).range(1_u32..=600).suffix(" min"));
            }
        });
        setting(ui, "Race", |ui| {
            widgets::toggle(ui, &mut x.race_enabled);
            if x.race_enabled {
                let mut timed = x.race_minutes > 0;
                if ui.selectable_label(!timed, "Laps").clicked() {
                    timed = false;
                    x.race_minutes = 0;
                }
                if ui.selectable_label(timed, "Minutes").clicked() && !timed {
                    x.race_minutes = 30;
                }
                if x.race_minutes > 0 {
                    ui.add(egui::DragValue::new(&mut x.race_minutes).range(1_u32..=1440).suffix(" min"));
                } else {
                    ui.add(egui::DragValue::new(&mut form.laps).range(1_u32..=999).suffix(" laps"));
                }
            }
        });
        if x.race_enabled {
            setting(ui, "Start from the pits", |ui| {
                widgets::toggle(ui, &mut x.pit_start);
            });
        }
        let mut parts = Vec::new();
        if x.practice_minutes > 0 {
            parts.push(format!("Practice {} min", x.practice_minutes));
        }
        if x.qualify_minutes > 0 {
            parts.push(format!("Qualifying {} min", x.qualify_minutes));
        }
        if x.race_enabled {
            parts.push(if x.race_minutes > 0 { format!("Race {} min", x.race_minutes) } else { format!("Race {} laps", form.laps) });
        }
        ui.add_space(4.0);
        ui.label(RichText::new(parts.join("   →   ")).size(12.5).color(theme::ACCENT));
    });
    }

    card(ui, "Conditions", "Weather, light and the track surface.", |ui| {
        setting(ui, "Weather", |ui| {
            if weathers.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut form.weather).desired_width(200.0));
            } else {
                let current = humanize_weather(&form.weather);
                egui::ComboBox::from_id_salt("quick_race_weather").selected_text(current).height(320.0).show_ui(ui, |ui| {
                    for id in weathers.iter() {
                        ui.selectable_value(&mut form.weather, id.clone(), humanize_weather(id));
                    }
                });
            }
        });
        setting(ui, "Time of day", |ui| {
            ui.add(egui::Slider::new(&mut form.time_of_day_hour, 0..=23).suffix(":00"));
        });
        setting(ui, "Time speed", |ui| {
            ui.add(egui::DragValue::new(&mut x.time_multiplier).range(1_u32..=60).prefix("x"));
            ui.label(RichText::new("1 is real time").size(12.0).color(theme::FAINT));
        });
        setting(ui, "Air temperature", |ui| {
            ui.add(egui::Slider::new(&mut x.ambient_temp, -10..=45).suffix(" °C"));
        });
        setting(ui, "Road temperature", |ui| {
            ui.add(egui::Slider::new(&mut x.road_temp, -10..=70).suffix(" °C"));
            if ui.small_button("Match air").clicked() {
                x.road_temp = x.ambient_temp + 6;
            }
        });
        setting(ui, "Wind speed", |ui| {
            ui.add(egui::Slider::new(&mut x.wind_min_kmh, 0..=60).suffix(" min"));
            ui.add(egui::Slider::new(&mut x.wind_max_kmh, 0..=60).suffix(" max"));
            if x.wind_min_kmh > x.wind_max_kmh {
                x.wind_max_kmh = x.wind_min_kmh;
            }
        });
        setting(ui, "Wind direction", |ui| {
            ui.add(egui::Slider::new(&mut x.wind_direction, 0..=359).suffix("°"));
        });
        setting(ui, "Track grip at the start", |ui| {
            ui.add(egui::Slider::new(&mut x.track_grip, 70..=100).suffix(" %"));
        });
    });

    if weekend {
    card(ui, "Opponents", "AI drivers on the grid.", |ui| {
        let max_grid = form.ai_count.min(MAX_AI_COUNT);
        form.grid_position = form.grid_position.min(form.ai_count);
        setting(ui, "AI cars", |ui| {
            ui.add(egui::Slider::new(&mut form.ai_count, 0..=MAX_AI_COUNT));
        });
        setting(ui, "Your grid position", |ui| {
            ui.add(egui::Slider::new(&mut form.grid_position, 0..=max_grid));
        });
        setting(ui, "AI strength", |ui| {
            ui.add(egui::Slider::new(&mut form.ai_level, 0..=100).suffix(" %"));
        });
        setting(ui, "AI aggression", |ui| {
            let mut on = form.ai_aggression.is_some();
            if widgets::toggle(ui, &mut on).changed() {
                form.ai_aggression = on.then_some(50);
            }
            match form.ai_aggression.as_mut() {
                Some(value) => {
                    ui.add(egui::Slider::new(value, 0..=100).suffix(" %"));
                }
                None => {
                    ui.label(RichText::new("Game default").color(MUTED));
                }
            }
        });
    });
    }

    card(ui, "Rules", "", |ui| {
        setting(ui, "Driver name", |ui| {
            ui.add(egui::TextEdit::singleline(&mut form.player_name).desired_width(220.0));
        });
        setting(ui, "Penalties", |ui| {
            widgets::toggle(ui, &mut x.penalties);
        });
        setting(ui, "Jump start", |ui| {
            pick_value(ui, "qr_jump", &mut x.jump_start_penalty, &[(0, "No penalty"), (1, "Drive-through"), (2, "Pit lane start")]);
        });
        setting(ui, "Wheels off track", |ui| {
            pick_value(
                ui,
                "qr_tyres_out",
                &mut x.allowed_tyres_out,
                &[(-1, "Laps never invalid"), (0, "None allowed"), (1, "One allowed"), (2, "Two allowed"), (3, "Three allowed")],
            );
        });
        setting(ui, "Fixed setup", |ui| {
            widgets::toggle(ui, &mut x.fixed_setup);
            ui.label(RichText::new("everyone drives the same setup").size(12.0).color(theme::FAINT));
        });
    });

    card(ui, "Driver aids", "Written to assists.ini when you start; your old file is kept as a backup.", |ui| {
        setting(ui, "Use these aids", |ui| {
            widgets::toggle(ui, &mut x.write_assists);
            if !x.write_assists {
                ui.label(RichText::new("off: the game keeps your current aids").size(12.0).color(theme::FAINT));
            }
        });
        ui.add_enabled_ui(x.write_assists, |ui| {
            let a = &mut x.assists;
            setting(ui, "Quick setups", |ui| {
                use acxs_manager::race_options::Assists;
                if ui.button("Beginner").clicked() {
                    *a = Assists {
                        ideal_line: true,
                        auto_shifter: true,
                        stability_control: 60,
                        traction_control: 2,
                        abs: 2,
                        auto_clutch: true,
                        damage: 0,
                        visual_damage: false,
                        ..Assists::default()
                    };
                }
                if ui.button("Relaxed").clicked() {
                    *a = Assists { traction_control: 1, abs: 1, damage: 50, ..Assists::default() };
                }
                if ui.button("Realistic").clicked() {
                    *a = Assists::default();
                }
                if ui.button("Hardcore").clicked() {
                    *a = Assists { traction_control: 0, abs: 0, auto_blip: false, damage: 100, fuel_rate: 1, tyre_wear: 1, ..Assists::default() };
                }
            });
            let tri = [(0u32, "Off"), (1, "As the car has it"), (2, "Always on")];
            setting(ui, "Traction control", |ui| pick_value(ui, "qr_tc", &mut a.traction_control, &tri));
            setting(ui, "ABS", |ui| pick_value(ui, "qr_abs", &mut a.abs, &tri));
            setting(ui, "Stability control", |ui| {
                ui.add(egui::Slider::new(&mut a.stability_control, 0..=100).suffix(" %"));
            });
            setting(ui, "Automatic gears", |ui| {
                widgets::toggle(ui, &mut a.auto_shifter);
            });
            setting(ui, "Throttle blip", |ui| {
                widgets::toggle(ui, &mut a.auto_blip);
                ui.label(RichText::new("matches revs on downshifts").size(12.0).color(theme::FAINT));
            });
            setting(ui, "Automatic clutch", |ui| {
                widgets::toggle(ui, &mut a.auto_clutch);
            });
            setting(ui, "Racing line", |ui| {
                widgets::toggle(ui, &mut a.ideal_line);
            });
            setting(ui, "Damage", |ui| {
                ui.add(egui::Slider::new(&mut a.damage, 0..=100).suffix(" %"));
            });
            setting(ui, "Damage on the body", |ui| {
                widgets::toggle(ui, &mut a.visual_damage);
            });
            let times = [(0u32, "Off"), (1, "Normal"), (2, "2x"), (3, "3x"), (5, "5x"), (10, "10x")];
            setting(ui, "Fuel use", |ui| pick_value(ui, "qr_fuel", &mut a.fuel_rate, &times));
            setting(ui, "Tyre wear", |ui| pick_value(ui, "qr_wear", &mut a.tyre_wear, &times));
            setting(ui, "Tyre blankets", |ui| {
                widgets::toggle(ui, &mut a.tyre_blankets);
                ui.label(RichText::new("tyres start warm").size(12.0).color(theme::FAINT));
            });
            setting(ui, "Slipstream", |ui| pick_value(ui, "qr_slip", &mut a.slipstream, &times));
        });
    });

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
        let launch = widgets::primary_button("Start race").min_size(egui::vec2(160.0, 38.0));
        if ui.add_enabled(!busy && *scanned, launch).clicked() {
            action = Some(Action::Launch);
        }
        if ui.add_enabled(!busy, egui::Button::new("Validate")).clicked() {
            action = Some(Action::Validate);
        }
        if ui.add_enabled(!busy, egui::Button::new("Rescan content")).clicked() {
            action = Some(Action::Rescan);
        }
        if busy {
            ui.spinner();
        }
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
                .add_enabled(!busy && has_selection, widgets::primary_button("Load"))
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

/// Weather types installed in `content/weather`, sorted by their number prefix.
fn list_weather(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("content").join("weather"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by_key(|name| {
        let number: u32 = name.split('_').next().and_then(|n| n.parse().ok()).unwrap_or(u32::MAX);
        (number, name.clone())
    });
    names
}
