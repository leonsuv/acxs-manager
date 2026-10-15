//! Car Specs tool page: pick an installed car to see its specs and a shift-light
//! preview, then compare it with a second car.
//!
//! Every file read happens on a background thread. Results come back through a
//! channel that the UI drains with `try_recv` each frame, and the worker wakes
//! the UI with `request_repaint`.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::car_compare::{self, IniChange, SpecDiff};
use acxs_manager::cardata::{self, CarSpecs};
use acxs_manager::content::{self, CarEntry};
use acxs_manager::shift_lights::{self, ShiftProfile};
use acxs_manager::thumbnails;
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

const ROW_HEIGHT: f32 = 56.0;
/// Edge length in pixels of the square thumbnail made by the "Make thumbnail" button.
const THUMB_SIZE: u32 = 512;
const COMPARE_ROW_HEIGHT: f32 = 44.0;
const COMPARE_LIST_HEIGHT: f32 = 220.0;
/// Most data-file changes drawn at once. The rest are counted, not drawn.
const MAX_CHANGE_ROWS: usize = 200;
const LED_COUNT: usize = shift_lights::DEFAULT_LED_COUNT;
const LED_RADIUS: f32 = 13.0;
const LED_GAP: f32 = 10.0;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(CarSpecsPage {
        root: None,
        cars: Vec::new(),
        scanning: false,
        car_query: String::new(),
        selected: None,
        loaded: None,
        pending_specs: None,
        rpm: 0.0,
        compare_query: String::new(),
        compare_id: None,
        compare: None,
        pending_compare: None,
        thumb_busy: false,
        tx,
        rx,
    })
}

/// Result of a background job.
enum Msg {
    Cars { root: PathBuf, cars: Vec<CarEntry> },
    Specs { id: String, specs: CarSpecs, rev_limit: Option<f64> },
    Compare { left: String, right: String, diffs: Vec<SpecDiff>, changes: Result<Vec<IniChange>, String> },
    /// A thumbnail was made or found in the cache: its file path and size in bytes.
    Thumbnail { result: Result<(PathBuf, usize), String> },
}

/// Specs of the selected car, once read.
struct Loaded {
    id: String,
    specs: CarSpecs,
    rev_limit: Option<f64>,
}

/// Comparison of two cars, once read.
struct Comparison {
    left: String,
    right: String,
    diffs: Vec<SpecDiff>,
    changes: Result<Vec<IniChange>, String>,
}

struct CarSpecsPage {
    /// The AC folder the car list was scanned for.
    root: Option<PathBuf>,
    cars: Vec<CarEntry>,
    scanning: bool,
    car_query: String,
    /// Car id (folder name) of the main car.
    selected: Option<String>,
    loaded: Option<Loaded>,
    pending_specs: Option<String>,
    rpm: f64,
    compare_query: String,
    /// Car id of the car to compare with.
    compare_id: Option<String>,
    compare: Option<Comparison>,
    pending_compare: Option<(String, String)>,
    /// A thumbnail is being made for the selected car.
    thumb_busy: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

/// Run `job` on a background thread and send its result back, waking the UI.
fn spawn_job(egui_ctx: &egui::Context, tx: &Sender<Msg>, job: impl FnOnce() -> Msg + Send + 'static) {
    let tx = tx.clone();
    let egui_ctx = egui_ctx.clone();
    std::thread::spawn(move || {
        let msg = job();
        if tx.send(msg).is_ok() {
            egui_ctx.request_repaint();
        }
    });
}

impl CarSpecsPage {
    fn car_path(&self, id: &str) -> Option<PathBuf> {
        self.cars.iter().find(|c| c.id == id).map(|c| c.path.clone())
    }

    fn start_scan(&mut self, egui_ctx: &egui::Context) {
        let Some(root) = self.root.clone() else { return };
        self.scanning = true;
        spawn_job(egui_ctx, &self.tx, move || {
            let cars = content::scan_cars(&root);
            Msg::Cars { root, cars }
        });
    }

    fn request_specs(&mut self, egui_ctx: &egui::Context, id: String) {
        let Some(path) = self.car_path(&id) else { return };
        self.pending_specs = Some(id.clone());
        spawn_job(egui_ctx, &self.tx, move || {
            let specs = cardata::read_specs(&path);
            let rev_limit = shift_lights::read_rev_limit(&path);
            Msg::Specs { id, specs, rev_limit }
        });
    }

    fn request_compare(&mut self, egui_ctx: &egui::Context, left: String, right: String) {
        let (Some(left_path), Some(right_path)) = (self.car_path(&left), self.car_path(&right)) else {
            return;
        };
        self.pending_compare = Some((left.clone(), right.clone()));
        spawn_job(egui_ctx, &self.tx, move || {
            let diffs = car_compare::spec_diff(&left_path, &right_path);
            let changes = car_compare::compare_data_files(&left_path, &right_path).map_err(|e| e.to_string());
            Msg::Compare { left, right, diffs, changes }
        });
    }

    /// Apply finished jobs. Results for a car that is no longer selected are dropped.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Thumbnail { result } => {
                    self.thumb_busy = false;
                    match result {
                        Ok((path, size)) => notices.push(Notice {
                            text: format!(
                                "Thumbnail saved to {} ({THUMB_SIZE}x{THUMB_SIZE}, {size} bytes)",
                                path.display()
                            ),
                            ok: true,
                        }),
                        Err(e) => notices.push(Notice {
                            text: format!("Thumbnail failed: {e}"),
                            ok: false,
                        }),
                    }
                }
                Msg::Cars { root, cars } => {
                    if self.root.as_deref() == Some(root.as_path()) {
                        self.cars = cars;
                        self.scanning = false;
                    }
                }
                Msg::Specs { id, specs, rev_limit } => {
                    if self.pending_specs.as_deref() == Some(id.as_str()) {
                        self.pending_specs = None;
                    }
                    if self.selected.as_deref() == Some(id.as_str()) {
                        if let Some(limit) = rev_limit {
                            self.rpm = self.rpm.min(limit);
                        }
                        self.loaded = Some(Loaded { id, specs, rev_limit });
                    }
                }
                Msg::Compare { left, right, diffs, changes } => {
                    if self.pending_compare.as_ref() == Some(&(left.clone(), right.clone())) {
                        self.pending_compare = None;
                    }
                    let shown = self.selected.as_deref() == Some(left.as_str())
                        && self.compare_id.as_deref() == Some(right.as_str());
                    if shown {
                        self.compare = Some(Comparison { left, right, diffs, changes });
                    }
                }
            }
        }
    }

    /// Start whatever the current selection needs and is not already loading.
    fn ensure_jobs(&mut self, egui_ctx: &egui::Context) {
        if let Some(id) = self.selected.clone() {
            let loaded = self.loaded.as_ref().is_some_and(|l| l.id == id);
            let pending = self.pending_specs.as_deref() == Some(id.as_str());
            if !loaded && !pending {
                self.request_specs(egui_ctx, id);
            }
        }
        if let (Some(left), Some(right)) = (self.selected.clone(), self.compare_id.clone()) {
            if left != right {
                let done = self
                    .compare
                    .as_ref()
                    .is_some_and(|c| c.left == left && c.right == right);
                let pending = self
                    .pending_compare
                    .as_ref()
                    .is_some_and(|(l, r)| *l == left && *r == right);
                if !done && !pending {
                    self.request_compare(egui_ctx, left, right);
                }
            }
        }
    }

    /// Thumbnail button, the cached thumbnail path and a button to open its folder.
    fn thumbnail_section(&mut self, ui: &mut egui::Ui, id: &str, notices: &mut Vec<Notice>) {
        let Some(root) = self.root.clone() else { return };
        let cache = thumbnails::cache_path(&root, id);
        let thumb_file = cache.join(format!("{THUMB_SIZE}.png"));
        let shown = if thumb_file.is_file() {
            thumb_file.display().to_string()
        } else {
            "not made yet".to_string()
        };

        ui.add_space(16.0);
        widgets::section_title(ui, "Thumbnail");
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                let make = ui.add_enabled(!self.thumb_busy, widgets::primary_button("Make thumbnail"));
                if make.clicked() {
                    self.thumb_busy = true;
                    let id = id.to_string();
                    spawn_job(ui.ctx(), &self.tx, move || {
                        let saved = thumbnails::cache_path(&root, &id).join(format!("{THUMB_SIZE}.png"));
                        let result = thumbnails::car_thumbnail(&root, &id, THUMB_SIZE)
                            .map(|bytes| (saved, bytes.len()))
                            .map_err(|e| e.to_string());
                        Msg::Thumbnail { result }
                    });
                }
                if self.thumb_busy {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new("Working...").color(theme::MUTED));
                }
            });
            ui.add_space(8.0);
            widgets::key_value(ui, "Cached thumbnail", &shown);
            ui.horizontal(|ui| {
                let open = ui.add_enabled(cache.is_dir(), egui::Button::new("Open folder"));
                if open.clicked() {
                    if let Err(e) = open_folder(&cache) {
                        notices.push(Notice {
                            text: format!("Could not open {}: {e}", cache.display()),
                            ok: false,
                        });
                    }
                }
            });
        });
    }

    /// Right-hand column: the selected car, its specs, shift lights and the comparison.
    /// Returns a car id when the compare list was clicked this frame.
    fn detail(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) -> Option<String> {
        let Some(selected) = self.selected.clone() else {
            widgets::empty_state(ui, "Pick a car from the list to see its specs.");
            return None;
        };
        let Some(car) = self.cars.iter().find(|c| c.id == selected).cloned() else {
            widgets::empty_state(ui, "This car is no longer installed.");
            return None;
        };
        car_header(ui, &car);
        self.thumbnail_section(ui, &selected, notices);

        let loaded = self.loaded.as_ref().filter(|l| l.id == selected);
        ui.add_space(16.0);
        widgets::section_title(ui, "Specs");
        if let Some(loaded) = loaded {
            theme::card().show(ui, |ui| {
                let lines = cardata::summary_lines(&loaded.specs);
                if lines.is_empty() {
                    ui.label(RichText::new("No spec values found in data/.").color(theme::MUTED));
                }
                for (label, value) in &lines {
                    widgets::key_value(ui, label, value);
                }
            });
        } else {
            ui.label(RichText::new("Reading specs...").color(theme::MUTED));
        }

        ui.add_space(16.0);
        widgets::section_title(ui, "Shift lights");
        let rev_limit = loaded.and_then(|l| l.rev_limit);
        let mut rpm = self.rpm;
        theme::card().show(ui, |ui| {
            let profile = rev_limit.and_then(|limit| {
                shift_lights::profile_for(
                    limit,
                    LED_COUNT,
                    shift_lights::DEFAULT_START_FRACTION,
                    shift_lights::DEFAULT_END_FRACTION,
                )
                .ok()
            });
            match profile {
                Some(profile) => shift_preview(ui, &profile, &mut rpm),
                None => {
                    ui.label(
                        RichText::new("No rev limit in data/engine.ini, so the shift lights cannot be shown.")
                            .color(theme::WARN),
                    );
                }
            }
        });
        self.rpm = rpm;

        ui.add_space(16.0);
        widgets::section_title(ui, "Compare with");
        let picked = car_list(
            ui,
            &self.cars,
            &mut self.compare_query,
            self.compare_id.as_deref(),
            COMPARE_ROW_HEIGHT,
            Some(COMPARE_LIST_HEIGHT),
            "car_specs_compare",
        );
        ui.add_space(12.0);

        match self.compare_id.clone() {
            None => {
                ui.label(RichText::new("Pick a car above to compare its specs and data files.").color(theme::MUTED));
            }
            Some(other) if other == selected => {
                ui.label(RichText::new("Pick a different car to compare.").color(theme::MUTED));
            }
            Some(other) => {
                let other_name = self
                    .cars
                    .iter()
                    .find(|c| c.id == other)
                    .map_or_else(|| other.clone(), |c| c.name.clone());
                match self
                    .compare
                    .as_ref()
                    .filter(|c| c.left == selected && c.right == other)
                {
                    Some(cmp) => show_comparison(ui, cmp, &car.name, &other_name),
                    None => {
                        ui.label(RichText::new("Comparing...").color(theme::MUTED));
                    }
                }
            }
        }
        picked
    }
}

impl Tool for CarSpecsPage {
    fn name(&self) -> &'static str {
        "Car Specs"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        widgets::heading(
            ui,
            "Car Specs",
            "Physical specs and shift lights of an installed car, and how it differs from another.",
        );
        let Some(root) = ctx.ac_root.as_deref().filter(|_| ctx.ac_valid) else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to see car specs.");
            return;
        };
        let egui_ctx = ui.ctx().clone();

        self.poll(notices);
        if self.root.as_deref() != Some(root) {
            self.root = Some(root.to_path_buf());
            self.cars.clear();
            self.selected = None;
            self.loaded = None;
            self.pending_specs = None;
            self.compare_id = None;
            self.compare = None;
            self.pending_compare = None;
            self.start_scan(&egui_ctx);
        }

        let mut rescan = false;
        let mut picked_car: Option<String> = None;
        let mut picked_compare: Option<String> = None;
        ui.columns(2, |cols| {
            if let [left, right] = cols {
                left.horizontal(|ui| {
                    let status = if self.scanning {
                        "Scanning cars...".to_string()
                    } else {
                        format!("{} cars installed", self.cars.len())
                    };
                    ui.label(RichText::new(status).small().color(theme::MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!self.scanning, egui::Button::new("Rescan")).clicked() {
                            rescan = true;
                        }
                    });
                });
                left.add_space(6.0);
                picked_car = car_list(
                    left,
                    &self.cars,
                    &mut self.car_query,
                    self.selected.as_deref(),
                    ROW_HEIGHT,
                    None,
                    "car_specs_main",
                );

                egui::ScrollArea::vertical()
                    .id_salt("car_specs_detail")
                    .auto_shrink([false, false])
                    .show(right, |ui| {
                        picked_compare = self.detail(ui, notices);
                    });
            }
        });

        if rescan {
            self.start_scan(&egui_ctx);
        }
        if let Some(id) = picked_car {
            if self.selected.as_deref() != Some(id.as_str()) {
                self.selected = Some(id);
                self.loaded = None;
            }
        }
        if let Some(id) = picked_compare {
            self.compare_id = Some(id);
        }
        self.ensure_jobs(&egui_ctx);
    }
}

/// Searchable list of cars. Returns the id of the car clicked this frame.
fn car_list(
    ui: &mut egui::Ui,
    cars: &[CarEntry],
    query: &mut String,
    selected: Option<&str>,
    row_height: f32,
    max_height: Option<f32>,
    salt: &str,
) -> Option<String> {
    ui.add(
        egui::TextEdit::singleline(&mut *query)
            .hint_text("Search name, brand, class or tag")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(6.0);
    let filtered = content::filter_cars(cars, query.as_str());
    if filtered.is_empty() {
        let text = if cars.is_empty() { "No cars found in content/cars." } else { "No cars match your search." };
        ui.label(RichText::new(text).color(theme::MUTED));
        return None;
    }

    let mut picked = None;
    let mut scroll = egui::ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]);
    if let Some(height) = max_height {
        scroll = scroll.max_height(height);
    }
    scroll.show_rows(ui, row_height, filtered.len(), |ui, range| {
        for car in &filtered[range] {
            if widgets::list_row(ui, selected == Some(car.id.as_str()), |ui| car_row(ui, car)) {
                picked = Some(car.id.clone());
            }
        }
    });
    picked
}

fn car_row(ui: &mut egui::Ui, car: &CarEntry) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(car.name.as_str()).strong());
        if !car.car_class.is_empty() {
            widgets::pill(ui, &car.car_class, theme::INFO);
        }
    });
    let subtitle = [car.brand.as_str(), car.year.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  ·  ");
    ui.label(RichText::new(subtitle).small().color(theme::MUTED));
}

fn car_header(ui: &mut egui::Ui, car: &CarEntry) {
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(car.name.as_str()).size(20.0).strong());
        if !car.car_class.is_empty() || !car.tags.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if !car.car_class.is_empty() {
                    widgets::pill(ui, &car.car_class, theme::INFO);
                }
                for tag in &car.tags {
                    widgets::pill(ui, tag, theme::MUTED);
                }
            });
        }
        ui.add_space(6.0);
        widgets::key_value(ui, "Brand", dash_if_empty(&car.brand));
        widgets::key_value(ui, "Year", dash_if_empty(&car.year));
        widgets::key_value(ui, "Folder", &car.id);
    });
}

fn dash_if_empty(text: &str) -> &str {
    if text.is_empty() { "—" } else { text }
}

/// Opens a folder in the platform file manager.
#[cfg(windows)]
fn open_folder(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("explorer").arg(path).spawn().map(|_| ())
}

/// Opens a folder in the platform file manager.
#[cfg(not(windows))]
fn open_folder(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open").arg(path).spawn().map(|_| ())
}

/// LED row, the rpm slider that drives it, and the thresholds behind it.
fn shift_preview(ui: &mut egui::Ui, profile: &ShiftProfile, rpm: &mut f64) {
    ui.add(
        egui::Slider::new(rpm, 0.0..=profile.rev_limit)
            .suffix(" rpm")
            .fixed_decimals(0)
            .step_by(10.0),
    );
    ui.add_space(8.0);
    let lit = shift_lights::led_states(profile, *rpm);
    draw_leds(ui, &lit);
    ui.add_space(8.0);

    let thresholds = profile
        .leds
        .iter()
        .map(|rpm| format!("{rpm:.0}"))
        .collect::<Vec<_>>()
        .join(", ");
    let lit_count = lit.iter().filter(|on| **on).count();
    widgets::key_value(ui, "Rev limit", &format!("{:.0} rpm", profile.rev_limit));
    widgets::key_value(ui, "Shift point", &format!("{:.0} rpm", shift_lights::shift_point(profile)));
    widgets::key_value(ui, "LED thresholds", &format!("{thresholds} rpm"));
    widgets::key_value(ui, "LEDs lit", &format!("{lit_count} of {}", lit.len()));
}

fn draw_leds(ui: &mut egui::Ui, lit: &[bool]) {
    let count = lit.len();
    let diameter = LED_RADIUS * 2.0;
    let width = count as f32 * diameter + count.saturating_sub(1) as f32 * LED_GAP;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, diameter), egui::Sense::hover());
    let painter = ui.painter();
    for (i, &on) in lit.iter().enumerate() {
        let center = egui::pos2(
            rect.left() + LED_RADIUS + i as f32 * (diameter + LED_GAP),
            rect.center().y,
        );
        let color = if on { led_color(i, count) } else { theme::BORDER };
        painter.circle_filled(center, LED_RADIUS, color);
    }
}

/// Lit LEDs take a colour from their position: green low, amber mid, red near the shift point.
fn led_color(index: usize, count: usize) -> Color32 {
    let position = index as f32 / count as f32;
    if position < 0.6 {
        theme::OK
    } else if position < 0.8 {
        theme::WARN
    } else {
        theme::DANGER
    }
}

fn show_comparison(ui: &mut egui::Ui, cmp: &Comparison, left_name: &str, right_name: &str) {
    theme::card().show(ui, |ui| {
        ui.label(RichText::new("Spec differences").strong());
        ui.add_space(6.0);
        if cmp.diffs.is_empty() {
            ui.label(RichText::new("No headline spec differences.").color(theme::MUTED));
        } else {
            egui::Grid::new("car_specs_diffs").striped(true).num_columns(3).show(ui, |ui| {
                ui.label(RichText::new("Spec").color(theme::MUTED));
                ui.label(RichText::new(left_name).color(theme::MUTED));
                ui.label(RichText::new(right_name).color(theme::MUTED));
                ui.end_row();
                for diff in &cmp.diffs {
                    ui.label(diff.label.as_str());
                    ui.label(diff.left.as_str());
                    ui.label(diff.right.as_str());
                    ui.end_row();
                }
            });
        }
    });

    ui.add_space(12.0);
    match &cmp.changes {
        Err(err) => {
            ui.label(RichText::new(format!("Could not read the data files: {err}")).color(theme::DANGER));
        }
        Ok(changes) => {
            theme::card().show(ui, |ui| {
                ui.label(RichText::new(format!("Data file changes ({})", changes.len())).strong());
                ui.add_space(6.0);
                if changes.is_empty() {
                    ui.label(RichText::new("No data file keys differ.").color(theme::MUTED));
                    return;
                }
                if changes.len() > MAX_CHANGE_ROWS {
                    ui.label(
                        RichText::new(format!(
                            "Showing the first {MAX_CHANGE_ROWS} of {} changes.",
                            changes.len()
                        ))
                        .small()
                        .color(theme::WARN),
                    );
                    ui.add_space(4.0);
                }
                egui::Grid::new("car_specs_changes").striped(true).num_columns(5).show(ui, |ui| {
                    for header in ["File", "Section", "Key", left_name, right_name] {
                        ui.label(RichText::new(header).color(theme::MUTED));
                    }
                    ui.end_row();
                    for change in changes.iter().take(MAX_CHANGE_ROWS) {
                        ui.label(change.file.as_str());
                        ui.label(change.section.as_str());
                        ui.label(change.key.as_str());
                        ui.label(change.left.as_deref().unwrap_or("-"));
                        ui.label(change.right.as_deref().unwrap_or("-"));
                        ui.end_row();
                    }
                });
            });
        }
    }
}
