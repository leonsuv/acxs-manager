//! Lap Times tool page: best laps per car, manual lap entry and a lap delta chart.
//!
//! The lap store lives at `<ac_root>/acxs-laptimes.json` by default; the path is
//! editable on the page. Reading and writing the store and scanning the content
//! folders run on background threads. Their results come back over a channel
//! that is polled once per frame.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::content::{scan_cars, scan_tracks, CarEntry, TrackEntry};
use acxs_manager::delta::{self, Lap, Sample};
use acxs_manager::laptimes::{format_lap, now_recorded_at, LapRecord, LapStore, MAX_LAP_TIME_MS};
use eframe::egui::{
    self, Align2, CornerRadius, FontId, Pos2, RichText, Sense, Stroke, StrokeKind, TextEdit, vec2,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const STORE_FILE: &str = "acxs-laptimes.json";
/// Maximum rows on the leaderboard.
const BOARD_LIMIT: usize = 200;
const CHART_MAX_WIDTH: f32 = 720.0;
const CHART_HEIGHT: f32 = 240.0;
const STEP_MIN_M: f64 = 0.5;
const STEP_MAX_M: f64 = 1000.0;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(LapTimes {
        tx,
        rx,
        root: None,
        tracks: Vec::new(),
        cars: Vec::new(),
        scanning: false,
        store_path: String::new(),
        store: None,
        store_error: None,
        loading: false,
        saving: false,
        view: View::Leaderboard,
        track: None,
        layout: None,
        board_car: None,
        add_car: None,
        add_driver: String::new(),
        add_time: String::new(),
        delta_car: None,
        ref_pick: None,
        cur_pick: None,
        ref_csv: String::new(),
        cur_csv: String::new(),
        step_text: "5".to_string(),
        delta: None,
        delta_error: None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Leaderboard,
    Add,
    Delta,
}

/// Results sent back by background threads.
enum Msg {
    Content {
        root: PathBuf,
        tracks: Vec<TrackEntry>,
        cars: Vec<CarEntry>,
    },
    Loaded {
        path: PathBuf,
        result: Result<LapStore, String>,
    },
    Added {
        path: PathBuf,
        label: String,
        result: Result<LapStore, String>,
    },
}

struct DeltaView {
    reference_label: String,
    current_label: String,
    trace: Vec<(f64, f64)>,
    final_ms: Option<f64>,
}

struct LapTimes {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    root: Option<PathBuf>,
    tracks: Vec<TrackEntry>,
    cars: Vec<CarEntry>,
    scanning: bool,
    store_path: String,
    store: Option<LapStore>,
    store_error: Option<String>,
    loading: bool,
    saving: bool,
    view: View,
    track: Option<String>,
    layout: Option<String>,
    board_car: Option<String>,
    add_car: Option<String>,
    add_driver: String,
    add_time: String,
    delta_car: Option<String>,
    ref_pick: Option<usize>,
    cur_pick: Option<usize>,
    ref_csv: String,
    cur_csv: String,
    step_text: String,
    delta: Option<DeltaView>,
    delta_error: Option<String>,
}

impl Tool for LapTimes {
    fn name(&self) -> &'static str {
        "Lap Times"
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        if self.root != ctx.ac_root {
            let egui_ctx = ui.ctx().clone();
            self.reset_for_root(&egui_ctx, ctx.ac_root.clone());
        }

        widgets::heading(ui, "Lap Times", "Best laps per car, manual lap entry and lap delta comparison.");
        if self.root.is_none() {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to use lap times.");
            return;
        }

        let egui_ctx = ui.ctx().clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.body(ui, &egui_ctx, notices));
    }
}

impl LapTimes {
    /// Apply results from background threads. Stale results (for an older
    /// folder or store path) are dropped.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Content { root, tracks, cars } => {
                    if self.root.as_ref() != Some(&root) {
                        continue;
                    }
                    self.scanning = false;
                    self.tracks = tracks;
                    self.cars = cars;
                    let track_known = self
                        .track
                        .as_ref()
                        .is_some_and(|id| self.tracks.iter().any(|t| &t.id == id));
                    if !track_known {
                        self.track = None;
                        self.layout = None;
                        self.clear_picks();
                    }
                }
                Msg::Loaded { path, result } => {
                    if store_file(&self.store_path).as_ref() != Some(&path) {
                        continue;
                    }
                    self.loading = false;
                    match result {
                        Ok(store) => {
                            self.store = Some(store);
                            self.store_error = None;
                        }
                        Err(err) => {
                            notices.push(Notice {
                                text: format!("Could not read lap store: {err}"),
                                ok: false,
                            });
                            self.store = None;
                            self.store_error = Some(err);
                        }
                    }
                }
                Msg::Added { path, label, result } => {
                    self.saving = false;
                    match result {
                        Ok(store) => {
                            if store_file(&self.store_path).as_ref() == Some(&path) {
                                self.store = Some(store);
                                self.store_error = None;
                            }
                            self.add_time.clear();
                            notices.push(Notice {
                                text: format!("Saved lap: {label}"),
                                ok: true,
                            });
                        }
                        Err(err) => notices.push(Notice {
                            text: format!("Could not save lap: {err}"),
                            ok: false,
                        }),
                    }
                }
            }
        }
    }

    fn reset_for_root(&mut self, ctx: &egui::Context, root: Option<PathBuf>) {
        self.root = root.clone();
        self.tracks.clear();
        self.cars.clear();
        self.scanning = false;
        self.loading = false;
        self.store = None;
        self.store_error = None;
        self.track = None;
        self.layout = None;
        self.board_car = None;
        self.add_car = None;
        self.delta_car = None;
        self.clear_picks();
        match root {
            Some(root) => {
                self.store_path = default_store_path(&root).display().to_string();
                self.spawn_scan(ctx, root);
                self.refresh_store(ctx);
            }
            None => self.store_path.clear(),
        }
    }

    fn clear_picks(&mut self) {
        self.ref_pick = None;
        self.cur_pick = None;
        self.delta = None;
        self.delta_error = None;
    }

    fn spawn_scan(&mut self, ctx: &egui::Context, root: PathBuf) {
        self.scanning = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let tracks = scan_tracks(&root);
            let cars = scan_cars(&root);
            let _ = tx.send(Msg::Content { root, tracks, cars });
            ctx.request_repaint();
        });
    }

    fn refresh_store(&mut self, ctx: &egui::Context) {
        match store_file(&self.store_path) {
            Some(path) => self.spawn_load(ctx, path),
            None => {
                self.loading = false;
                self.store = None;
                self.store_error = Some("Set a path for the lap store file.".to_string());
            }
        }
    }

    fn spawn_load(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.loading = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = LapStore::load(&path).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Loaded { path, result });
            ctx.request_repaint();
        });
    }

    fn spawn_add(&mut self, ctx: &egui::Context, path: PathBuf, record: LapRecord, label: String) {
        self.saving = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = add_and_save(&path, record);
            let _ = tx.send(Msg::Added { path, label, result });
            ctx.request_repaint();
        });
    }

    fn body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let mut reload = false;
        let mut rescan = false;
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Lap store").color(theme::MUTED));
                let edit = ui.add(
                    TextEdit::singleline(&mut self.store_path)
                        .desired_width(360.0)
                        .hint_text("Not set"),
                );
                reload |= edit.lost_focus();
                if ui.add_enabled(!self.loading, egui::Button::new("Reload")).clicked() {
                    reload = true;
                }
                let can_rescan = !self.scanning && self.root.is_some();
                if ui.add_enabled(can_rescan, egui::Button::new("Rescan content")).clicked() {
                    rescan = true;
                }
                if self.loading || self.scanning {
                    ui.spinner();
                }
            });
            let status = match (&self.store, &self.store_error) {
                (_, Some(err)) => RichText::new(err.as_str()).color(theme::DANGER),
                (Some(store), None) => RichText::new(format!("{} laps stored", store.records().len()))
                    .color(theme::MUTED),
                (None, None) => RichText::new("No lap store loaded").color(theme::MUTED),
            };
            ui.label(status);
        });
        if reload {
            self.refresh_store(ctx);
        }
        if rescan
            && let Some(root) = self.root.clone()
        {
            self.spawn_scan(ctx, root);
        }

        ui.add_space(12.0);
        let mut picks_reset = false;
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Track").color(theme::MUTED));
                let before = self.track.clone();
                let track_text = match self.track.as_deref() {
                    Some(id) => track_label(&self.tracks, id),
                    None => "Choose a track".to_string(),
                };
                egui::ComboBox::from_id_salt("lt_track")
                    .selected_text(track_text)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for track in &self.tracks {
                            ui.selectable_value(&mut self.track, Some(track.id.clone()), track.name.as_str());
                        }
                    });
                if self.track != before {
                    self.layout = None;
                    picks_reset = true;
                }

                ui.label(RichText::new("Layout").color(theme::MUTED));
                let layouts: Vec<String> = self
                    .tracks
                    .iter()
                    .find(|t| self.track.as_ref() == Some(&t.id))
                    .map(|t| t.layouts.clone())
                    .unwrap_or_default();
                let layout_before = self.layout.clone();
                let layout_text = self.layout.clone().unwrap_or_else(|| "Default".to_string());
                egui::ComboBox::from_id_salt("lt_layout")
                    .selected_text(layout_text)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.layout, None, "Default");
                        for layout in &layouts {
                            ui.selectable_value(&mut self.layout, Some(layout.clone()), layout.as_str());
                        }
                    });
                if self.layout != layout_before {
                    picks_reset = true;
                }
            });
        });
        if picks_reset {
            self.clear_picks();
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.view, View::Leaderboard, "Leaderboard");
            ui.selectable_value(&mut self.view, View::Add, "Add lap");
            ui.selectable_value(&mut self.view, View::Delta, "Delta");
        });
        ui.add_space(8.0);

        if self.store.is_none() {
            let text = if self.loading {
                "Loading lap store..."
            } else {
                "Fix the lap store path above to use lap times."
            };
            widgets::empty_state(ui, text);
            return;
        }
        if self.track.is_none() {
            widgets::empty_state(ui, "Pick a track above.");
            return;
        }

        match self.view {
            View::Leaderboard => self.leaderboard(ui),
            View::Add => self.add_view(ui, ctx, notices),
            View::Delta => self.delta_view(ui),
        }
    }

    fn leaderboard(&mut self, ui: &mut egui::Ui) {
        let Some(store) = self.store.as_ref() else { return };
        let Some(track) = self.track.as_deref() else { return };

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Car").color(theme::MUTED));
            car_combo(ui, "lt_board_car", &self.cars, &mut self.board_car, Some("All cars"));
        });
        ui.add_space(8.0);

        let rows: Vec<&LapRecord> = store
            .leaderboard(track, self.layout.as_deref(), usize::MAX)
            .into_iter()
            .filter(|r| self.board_car.as_deref().is_none_or(|c| r.car.eq_ignore_ascii_case(c)))
            .take(BOARD_LIMIT)
            .collect();
        if rows.is_empty() {
            widgets::empty_state(ui, "No laps recorded for this track and layout yet.");
            return;
        }

        egui::Grid::new("lt_board")
            .striped(true)
            .num_columns(5)
            .spacing([18.0, 6.0])
            .show(ui, |ui| {
                for header in ["#", "Car", "Driver", "Lap time", "Recorded"] {
                    ui.label(RichText::new(header).small().color(theme::MUTED));
                }
                ui.end_row();
                for (i, rec) in rows.iter().enumerate() {
                    ui.label((i + 1).to_string());
                    ui.label(car_label(&self.cars, &rec.car));
                    ui.label(driver_label(rec));
                    let time_color = if i == 0 { theme::OK } else { theme::TEXT };
                    ui.label(RichText::new(format_lap(rec.lap_time_ms)).monospace().color(time_color));
                    ui.label(RichText::new(short_date(&rec.recorded_at)).color(theme::MUTED));
                    ui.end_row();
                }
            });
    }

    fn add_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let Some(track) = self.track.clone() else { return };
        let layout_text = self.layout.clone().unwrap_or_else(|| "default layout".to_string());
        let track_name = track_label(&self.tracks, &track);

        let mut submit = false;
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Add a lap");
            ui.label(RichText::new(format!("{track_name}, {layout_text}")).color(theme::MUTED));
            ui.add_space(6.0);
            egui::Grid::new("lt_add_form")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Car");
                    car_combo(ui, "lt_add_car", &self.cars, &mut self.add_car, None);
                    ui.end_row();

                    ui.label("Driver");
                    ui.add(
                        TextEdit::singleline(&mut self.add_driver)
                            .desired_width(260.0)
                            .hint_text("Driver name"),
                    );
                    ui.end_row();

                    ui.label("Lap time");
                    ui.add(
                        TextEdit::singleline(&mut self.add_time)
                            .desired_width(260.0)
                            .hint_text("m:ss.mmm, e.g. 1:23.456"),
                    );
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = self.store.is_some() && !self.saving;
                if ui.add_enabled(ready, widgets::primary_button("Add lap")).clicked() {
                    submit = true;
                }
                if self.saving {
                    ui.spinner();
                }
            });
        });
        if submit {
            self.submit_lap(ctx, notices);
        }
    }

    fn submit_lap(&mut self, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let Some(path) = store_file(&self.store_path) else {
            return warn(notices, "Set a path for the lap store file first.");
        };
        let Some(track) = self.track.clone() else {
            return warn(notices, "Pick a track first.");
        };
        let Some(car) = self.add_car.clone() else {
            return warn(notices, "Pick a car first.");
        };
        let driver = self.add_driver.trim().to_string();
        if driver.is_empty() {
            return warn(notices, "Enter the driver name.");
        }
        let lap_time_ms = match parse_lap_time(&self.add_time) {
            Ok(ms) => ms,
            Err(err) => return warn(notices, err),
        };

        let label = format!(
            "{} by {} in {}",
            format_lap(lap_time_ms),
            driver,
            car_label(&self.cars, &car)
        );
        let record = LapRecord {
            track,
            layout: self.layout.clone(),
            car,
            driver,
            lap_time_ms,
            sectors_ms: None,
            recorded_at: now_recorded_at(),
        };
        self.spawn_add(ctx, path, record, label);
    }

    fn delta_view(&mut self, ui: &mut egui::Ui) {
        let Some(track) = self.track.clone() else { return };
        let mut picks_reset = false;
        let mut compare = false;

        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Car").color(theme::MUTED));
                if car_combo(ui, "lt_delta_car", &self.cars, &mut self.delta_car, None) {
                    picks_reset = true;
                }
                ui.label(RichText::new("Step (m)").color(theme::MUTED));
                ui.add(TextEdit::singleline(&mut self.step_text).desired_width(60.0));
            });
        });
        ui.add_space(8.0);

        let history: Vec<&LapRecord> = match (self.store.as_ref(), self.delta_car.as_deref()) {
            (Some(store), Some(car)) => store.history(&track, self.layout.as_deref(), car),
            _ => Vec::new(),
        };
        if history.is_empty() {
            ui.label(
                RichText::new("No laps for this car on this track and layout yet. Add one first.")
                    .color(theme::MUTED),
            );
        }
        lap_side(ui, "lt_ref", "Reference lap", &history, &mut self.ref_pick, &mut self.ref_csv);
        ui.add_space(8.0);
        lap_side(ui, "lt_cur", "Compared lap", &history, &mut self.cur_pick, &mut self.cur_csv);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let ready = self.store.is_some();
            if ui.add_enabled(ready, widgets::primary_button("Compare laps")).clicked() {
                compare = true;
            }
        });
        drop(history);

        if picks_reset {
            self.clear_picks();
        }
        if compare {
            self.compare();
        }
        if let Some(err) = self.delta_error.as_deref() {
            ui.label(RichText::new(err).color(theme::DANGER));
        }
        if let Some(view) = self.delta.as_ref() {
            ui.add_space(8.0);
            show_delta(ui, view);
        }
    }

    fn compare(&mut self) {
        self.delta = None;
        self.delta_error = None;
        match self.build_delta() {
            Ok(view) => self.delta = Some(view),
            Err(err) => self.delta_error = Some(err),
        }
    }

    fn build_delta(&self) -> Result<DeltaView, String> {
        let step_m = parse_step(&self.step_text)?;
        let store = self.store.as_ref().ok_or("The lap store is not loaded.")?;
        let track = self.track.as_deref().ok_or("Pick a track first.")?;
        let car = self.delta_car.as_deref().ok_or("Pick a car first.")?;

        let history = store.history(track, self.layout.as_deref(), car);
        let reference_rec = self
            .ref_pick
            .and_then(|i| history.get(i).copied())
            .ok_or("Pick a reference lap.")?;
        let current_rec = self
            .cur_pick
            .and_then(|i| history.get(i).copied())
            .ok_or("Pick a compared lap.")?;

        let reference = parse_lap_csv(&self.ref_csv, "Reference")?;
        let current = parse_lap_csv(&self.cur_csv, "Compared")?;
        let trace = delta::delta_trace(&reference, &current, step_m);
        if trace.is_empty() {
            return Err("The two laps do not overlap in distance.".to_string());
        }
        Ok(DeltaView {
            reference_label: lap_label(reference_rec),
            current_label: lap_label(current_rec),
            trace,
            final_ms: delta::final_delta_ms(&reference, &current),
        })
    }
}

/// Load the store, add one lap and save. Runs on a background thread. The store is
/// read fresh from disk so a concurrent edit is not overwritten by a stale copy.
fn add_and_save(path: &Path, record: LapRecord) -> Result<LapStore, String> {
    let mut store = LapStore::load(path).map_err(|e| e.to_string())?;
    store.add(record).map_err(|e| e.to_string())?;
    store.save(path).map_err(|e| e.to_string())?;
    Ok(store)
}

fn store_file(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

fn default_store_path(root: &Path) -> PathBuf {
    root.join(STORE_FILE)
}

fn warn(notices: &mut Vec<Notice>, text: impl Into<String>) {
    notices.push(Notice {
        text: text.into(),
        ok: false,
    });
}

/// Digits only, no sign, and it must fit in a `u32`.
fn number(text: &str) -> Option<u32> {
    let text = text.trim();
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse::<u32>().ok())
        .flatten()
}

/// Parse `m:ss.mmm` (the milliseconds part is optional, 1 to 3 digits) into milliseconds.
fn parse_lap_time(text: &str) -> Result<u32, String> {
    let trimmed = text.trim();
    let bad = || format!("\"{trimmed}\" is not a lap time. Use m:ss.mmm, for example 1:23.456.");
    let (minutes, rest) = trimmed.split_once(':').ok_or_else(bad)?;
    let minutes = number(minutes).ok_or_else(bad)?;
    let (seconds, fraction) = match rest.split_once('.') {
        Some((s, f)) => (s, Some(f)),
        None => (rest, None),
    };
    let seconds = number(seconds).filter(|&s| s < 60).ok_or_else(bad)?;
    let millis = match fraction {
        None => 0,
        Some(f) if (1..=3).contains(&f.len()) => {
            number(f).ok_or_else(bad)? * 10u32.pow(3 - f.len() as u32)
        }
        Some(_) => return Err(bad()),
    };
    minutes
        .checked_mul(60_000)
        .and_then(|m| m.checked_add(seconds * 1_000 + millis))
        .filter(|&total| total > 0 && total <= MAX_LAP_TIME_MS)
        .ok_or_else(|| format!("Lap time must be above 0 and at most one hour (got \"{trimmed}\")."))
}

fn parse_step(text: &str) -> Result<f64, String> {
    match text.trim().parse::<f64>() {
        Ok(v) if v.is_finite() && (STEP_MIN_M..=STEP_MAX_M).contains(&v) => Ok(v),
        _ => Err(format!(
            "Step must be a number from {STEP_MIN_M} to {STEP_MAX_M} metres."
        )),
    }
}

/// Parse `distance_m,time_ms` lines into a lap. A header line and blank lines are skipped.
fn parse_lap_csv(text: &str, which: &str) -> Result<Lap, String> {
    let mut samples = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || (samples.is_empty() && line.eq_ignore_ascii_case("distance_m,time_ms")) {
            continue;
        }
        let line_no = index + 1;
        let (distance, time) = line
            .split_once(',')
            .ok_or_else(|| format!("{which} lap, line {line_no}: expected distance_m,time_ms"))?;
        let distance_m: f64 = distance.trim().parse().map_err(|_| {
            format!("{which} lap, line {line_no}: \"{}\" is not a distance in metres", distance.trim())
        })?;
        let time_ms: f64 = time.trim().parse().map_err(|_| {
            format!("{which} lap, line {line_no}: \"{}\" is not a time in ms", time.trim())
        })?;
        samples.push(Sample { distance_m, time_ms });
    }
    Lap::new(samples).map_err(|e| format!("{which} lap: {e}"))
}

fn track_label(tracks: &[TrackEntry], id: &str) -> String {
    tracks
        .iter()
        .find(|t| t.id == id)
        .map_or_else(|| id.to_string(), |t| t.name.clone())
}

fn car_label(cars: &[CarEntry], id: &str) -> String {
    cars.iter()
        .find(|c| c.id == id)
        .map_or_else(|| id.to_string(), |c| c.name.clone())
}

fn driver_label(rec: &LapRecord) -> String {
    if rec.driver.trim().is_empty() {
        "-".to_string()
    } else {
        rec.driver.clone()
    }
}

/// The date part of an ISO-8601 timestamp.
fn short_date(recorded_at: &str) -> &str {
    recorded_at.get(..10).unwrap_or(recorded_at)
}

fn lap_label(rec: &LapRecord) -> String {
    format!(
        "{}  {}  {}",
        format_lap(rec.lap_time_ms),
        driver_label(rec),
        short_date(&rec.recorded_at)
    )
}

/// Car picker. `all_label` adds a "no filter" entry that maps to `None`.
/// Returns true when the selection changed.
fn car_combo(
    ui: &mut egui::Ui,
    id: &str,
    cars: &[CarEntry],
    pick: &mut Option<String>,
    all_label: Option<&str>,
) -> bool {
    let before = pick.clone();
    let text = match (pick.as_deref(), all_label) {
        (Some(car), _) => car_label(cars, car),
        (None, Some(all)) => all.to_string(),
        (None, None) => "Choose a car".to_string(),
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(260.0)
        .show_ui(ui, |ui| {
            if let Some(all) = all_label {
                ui.selectable_value(pick, None, all);
            }
            for car in cars {
                ui.selectable_value(pick, Some(car.id.clone()), car.name.as_str());
            }
        });
    *pick != before
}

/// One side of the delta comparison: a lap from history plus its pasted samples.
fn lap_side(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    history: &[&LapRecord],
    pick: &mut Option<usize>,
    csv: &mut String,
) {
    theme::card().show(ui, |ui| {
        widgets::section_title(ui, title);
        let text = (*pick)
            .and_then(|i| history.get(i))
            .map(|rec| lap_label(rec))
            .unwrap_or_else(|| "Choose a lap".to_string());
        let width = ui.available_width().min(420.0);
        egui::ComboBox::from_id_salt(id)
            .selected_text(text)
            .width(width)
            .show_ui(ui, |ui| {
                for (i, rec) in history.iter().enumerate() {
                    ui.selectable_value(pick, Some(i), lap_label(rec));
                }
            });
        ui.add_space(4.0);
        ui.label(
            RichText::new("Samples as CSV, one per line: distance_m,time_ms")
                .small()
                .color(theme::MUTED),
        );
        ui.add(
            TextEdit::multiline(csv)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Monospace)
                .hint_text("0,0\n120,4310\n..."),
        );
    });
}

fn format_delta(ms: f64) -> String {
    format!("{:+.3} s", ms / 1000.0)
}

fn show_delta(ui: &mut egui::Ui, view: &DeltaView) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new(format!("{} vs {}", view.current_label, view.reference_label))
                .color(theme::MUTED),
        );
        let (text, color) = match view.final_ms {
            Some(ms) => (
                format!("Final delta {}", format_delta(ms)),
                if ms <= 0.0 { theme::OK } else { theme::DANGER },
            ),
            None => ("No final delta".to_string(), theme::MUTED),
        };
        ui.label(RichText::new(text).size(20.0).strong().color(color));
        ui.label(
            RichText::new("Red: the compared lap is behind at that distance. Green: ahead.")
                .small()
                .color(theme::MUTED),
        );
        ui.add_space(8.0);
        draw_delta_chart(ui, &view.trace);
    });
}

/// Line chart of delta (ms) against distance (m). Negative (faster) is green, positive
/// (slower) is red, and the zero line is drawn through the middle.
fn draw_delta_chart(ui: &mut egui::Ui, trace: &[(f64, f64)]) {
    let (Some(&first), Some(&last)) = (trace.first(), trace.last()) else {
        return;
    };
    let width = ui.available_width().clamp(200.0, CHART_MAX_WIDTH);
    let (rect, _) = ui.allocate_exact_size(vec2(width, CHART_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), theme::BG);
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let plot = rect.shrink2(vec2(56.0, 22.0));
    let (x_min, x_max) = (first.0, last.0);
    let x_span = (x_max - x_min).max(1e-9);

    let raw_lo = trace.iter().map(|p| p.1).fold(0.0, f64::min);
    let raw_hi = trace.iter().map(|p| p.1).fold(0.0, f64::max);
    let hi_base = if raw_hi - raw_lo < 1.0 { raw_lo + 1.0 } else { raw_hi };
    let pad = (hi_base - raw_lo) * 0.08;
    let (lo, hi) = (raw_lo - pad, hi_base + pad);

    let to_screen = |x: f64, y: f64| {
        let fx = ((x - x_min) / x_span) as f32;
        let fy = ((y - lo) / (hi - lo)) as f32;
        Pos2::new(plot.left() + fx * plot.width(), plot.bottom() - fy * plot.height())
    };

    let zero_y = to_screen(x_min, 0.0).y;
    painter.line_segment(
        [Pos2::new(plot.left(), zero_y), Pos2::new(plot.right(), zero_y)],
        Stroke::new(1.0, theme::MUTED),
    );

    if trace.len() == 1 {
        let color = if first.1 > 0.0 { theme::DANGER } else { theme::OK };
        painter.circle_filled(to_screen(first.0, first.1), 3.0, color);
    }
    for pair in trace.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let color = if a.1 + b.1 > 0.0 { theme::DANGER } else { theme::OK };
        painter.line_segment(
            [to_screen(a.0, a.1), to_screen(b.0, b.1)],
            Stroke::new(2.0, color),
        );
    }

    let font = FontId::proportional(11.0);
    painter.text(
        Pos2::new(rect.left() + 8.0, plot.top()),
        Align2::LEFT_TOP,
        format!("{raw_hi:+.0} ms"),
        font.clone(),
        theme::MUTED,
    );
    painter.text(
        Pos2::new(rect.left() + 8.0, plot.bottom()),
        Align2::LEFT_BOTTOM,
        format!("{raw_lo:+.0} ms"),
        font.clone(),
        theme::MUTED,
    );
    painter.text(
        Pos2::new(plot.left(), rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        format!("{x_min:.0} m"),
        font.clone(),
        theme::MUTED,
    );
    painter.text(
        Pos2::new(plot.right(), rect.bottom() - 4.0),
        Align2::RIGHT_BOTTOM,
        format!("{x_max:.0} m"),
        font,
        theme::MUTED,
    );
}
