//! Car Tuning tool page: pick an installed car, choose one physical value and
//! change it. Writes run on a background thread. Results come back through a
//! channel that the UI drains with `try_recv` each frame, and the worker wakes
//! the UI with `request_repaint`. Each change is reported as notices.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::car_tuning::{
    self, Axle, Change, Tuning, MAX_BASE_HEIGHT_M, MAX_CAMBER_DEG, MAX_FACTOR, MAX_FUEL_L, MAX_MASS_KG,
    MAX_PRESSURE_PSI, MIN_BASE_HEIGHT_M, MIN_CAMBER_DEG, MIN_FACTOR, MIN_FUEL_L, MIN_MASS_KG, MIN_PRESSURE_PSI,
};
use acxs_manager::content::{self, CarEntry};
use acxs_manager::tuning_profiles::{self, ApplyProfileError, PROFILE_DIR, TuningProfile, TuningStep};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

/// Height of the scrollable car list.
const LIST_HEIGHT: f32 = 240.0;
const WARNING: &str = "Apply writes the edited file in the car's data folder. The previous file is kept \
as a backup next to it, named <file>.acxs-backup-<stamp>. To undo a change, copy that backup over the \
edited file.";

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(CarTuningPage {
        root: None,
        cars: Vec::new(),
        scanning: false,
        query: String::new(),
        selected: None,
        kind: Kind::default(),
        axle: Axle::Front,
        factor: 1.0,
        mass_kg: 1200.0,
        fuel_l: 60.0,
        pressure_psi: 30.0,
        camber_deg: -2.5,
        base_height_m: 0.25,
        profile_name: String::new(),
        steps: Vec::new(),
        busy: false,
        tx,
        rx,
    })
}

/// Result of a background job.
enum Msg {
    Cars { root: PathBuf, cars: Vec<CarEntry> },
    Applied { car: String, result: Result<Vec<Change>, String> },
    /// A profile file was picked and read. Nothing is sent when the pick is cancelled.
    Loaded(Result<TuningProfile, String>),
    ProfileApplied { car: String, result: Result<Vec<Change>, ApplyProfileError> },
}

/// Which value the page changes.
#[derive(Clone, Copy, Default, PartialEq)]
enum Kind {
    #[default]
    TorqueCurve,
    TotalMass,
    FuelTank,
    GearRatios,
    FinalDrive,
    IdealPressure,
    StaticPressure,
    Camber,
    BaseHeight,
}

impl Kind {
    const ALL: [Kind; 9] = [
        Kind::TorqueCurve,
        Kind::TotalMass,
        Kind::FuelTank,
        Kind::GearRatios,
        Kind::FinalDrive,
        Kind::IdealPressure,
        Kind::StaticPressure,
        Kind::Camber,
        Kind::BaseHeight,
    ];

    fn label(self) -> &'static str {
        match self {
            Kind::TorqueCurve => "Torque curve",
            Kind::TotalMass => "Total mass",
            Kind::FuelTank => "Fuel tank",
            Kind::GearRatios => "Gear ratios",
            Kind::FinalDrive => "Final drive",
            Kind::IdealPressure => "Ideal pressure",
            Kind::StaticPressure => "Static pressure",
            Kind::Camber => "Camber",
            Kind::BaseHeight => "Base height",
        }
    }

    /// Factors multiply the current value. Everything else is set to a new value.
    fn is_factor(self) -> bool {
        matches!(self, Kind::TorqueCurve | Kind::GearRatios | Kind::FinalDrive)
    }

    /// Per-axle kinds need the front or rear axle picked.
    fn has_axle(self) -> bool {
        matches!(
            self,
            Kind::IdealPressure | Kind::StaticPressure | Kind::Camber | Kind::BaseHeight
        )
    }
}

fn axle_label(axle: Axle) -> &'static str {
    match axle {
        Axle::Front => "Front axle",
        Axle::Rear => "Rear axle",
    }
}

struct CarTuningPage {
    /// The AC folder the car list was scanned for.
    root: Option<PathBuf>,
    cars: Vec<CarEntry>,
    scanning: bool,
    query: String,
    /// Car id (folder name) of the car to tune.
    selected: Option<String>,
    kind: Kind,
    /// Axle used by the per-axle kinds.
    axle: Axle,
    /// Multiplier used by the factor kinds.
    factor: f64,
    /// Value used by the total mass kind, in kg.
    mass_kg: f64,
    /// Value used by the fuel tank kind, in litres.
    fuel_l: f64,
    /// Value used by the tyre pressure kinds, in psi.
    pressure_psi: f64,
    /// Value used by the camber kind, in degrees.
    camber_deg: f64,
    /// Value used by the base height kind, in metres.
    base_height_m: f64,
    /// Name of the profile on the page. Also the saved file's name.
    profile_name: String,
    /// Steps of the profile on the page, applied in this order.
    steps: Vec<TuningStep>,
    /// Set while a change runs on a background thread.
    busy: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Tool for CarTuningPage {
    fn name(&self) -> &'static str {
        "Car Tuning"
    }

    fn group(&self) -> &'static str {
        "Car"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        widgets::heading(
            ui,
            "Car Tuning",
            "Scale or set the torque, mass, fuel tank, gears, final drive, tyre pressures, camber or base height of an installed car.",
        );
        self.poll(notices);

        let Some(root) = ctx.ac_root.as_deref().filter(|_| ctx.ac_valid) else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to tune cars.");
            return;
        };
        let egui_ctx = ui.ctx().clone();
        if self.root.as_deref() != Some(root) {
            self.root = Some(root.to_path_buf());
            self.cars.clear();
            self.selected = None;
            self.start_scan(&egui_ctx);
        }

        let mut picked: Option<String> = None;
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Car");
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search cars")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);
            if self.scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Scanning cars...").color(MUTED));
                });
                return;
            }
            if self.cars.is_empty() {
                ui.label(RichText::new("No cars found in content/cars.").color(MUTED));
                return;
            }
            let matches = content::filter_cars(&self.cars, &self.query);
            if matches.is_empty() {
                ui.label(RichText::new("No cars match your search.").color(MUTED));
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("car_tuning_list")
                .max_height(LIST_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for car in matches {
                        let selected = self.selected.as_deref() == Some(car.id.as_str());
                        if widgets::list_row(ui, selected, |ui| {
                            ui.label(RichText::new(&car.name).strong());
                            ui.label(RichText::new(&car.id).small().color(MUTED));
                        }) {
                            picked = Some(car.id.clone());
                        }
                    }
                });
        });
        if let Some(id) = picked {
            self.selected = Some(id);
        }

        ui.add_space(12.0);
        let mut apply_clicked = false;
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Change");
            ui.horizontal_wrapped(|ui| {
                for kind in Kind::ALL {
                    ui.selectable_value(&mut self.kind, kind, kind.label());
                }
            });
            if self.kind.has_axle() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for axle in [Axle::Front, Axle::Rear] {
                        ui.selectable_value(&mut self.axle, axle, axle_label(axle));
                    }
                });
            }
            ui.add_space(8.0);
            self.value_slider(ui);
            ui.add_space(4.0);
            let hint = if self.kind.is_factor() {
                "Multiplies the current value."
            } else {
                "Sets a new value."
            };
            ui.label(RichText::new(hint).small().color(MUTED));
            ui.add_space(10.0);
            ui.label(RichText::new(WARNING).small().color(WARN));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let can_apply = self.selected.is_some() && !self.busy;
                if ui.add_enabled(can_apply, widgets::primary_button("Apply")).clicked() {
                    apply_clicked = true;
                }
                if self.busy {
                    ui.spinner();
                    ui.label(RichText::new("Writing...").color(MUTED));
                }
            });
        });

        ui.add_space(12.0);
        let mut add_step = false;
        let mut remove_step: Option<usize> = None;
        let mut save_clicked = false;
        let mut load_clicked = false;
        let mut apply_profile_clicked = false;
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Profile");
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.profile_name)
                        .hint_text("Profile name")
                        .desired_width(240.0),
                );
                if ui.button("Add current step").clicked() {
                    add_step = true;
                }
            });
            ui.add_space(6.0);
            if self.steps.is_empty() {
                ui.label(
                    RichText::new("No steps yet. Set a change in the Change card, then add it as a step.")
                        .small()
                        .color(MUTED),
                );
            }
            for (index, step) in self.steps.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}. {}", index + 1, step_label(step)));
                    if ui.small_button("Remove").clicked() {
                        remove_step = Some(index);
                    }
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save profile").clicked() {
                    save_clicked = true;
                }
                if ui.button("Load profile").clicked() {
                    load_clicked = true;
                }
                let can_apply = self.selected.is_some() && !self.steps.is_empty() && !self.busy;
                if ui.add_enabled(can_apply, widgets::primary_button("Apply profile")).clicked() {
                    apply_profile_clicked = true;
                }
            });
        });

        if add_step {
            self.steps.push(TuningStep {
                kind: self.tuning(),
                value: self.current_value(),
            });
        }
        if let Some(index) = remove_step {
            self.steps.remove(index);
        }
        if save_clicked {
            self.save_current(root, notices);
        }
        if load_clicked {
            self.start_load(root, &egui_ctx);
        }
        if apply_profile_clicked {
            self.start_apply_profile(&egui_ctx);
        }
        if apply_clicked {
            self.start_apply(&egui_ctx);
        }
    }
}

impl CarTuningPage {
    /// Draws the slider for the current kind, bounded by the core library's limits.
    fn value_slider(&mut self, ui: &mut egui::Ui) {
        match self.kind {
            Kind::TorqueCurve | Kind::GearRatios | Kind::FinalDrive => {
                ui.add(
                    egui::Slider::new(&mut self.factor, MIN_FACTOR..=MAX_FACTOR)
                        .fixed_decimals(2)
                        .suffix(" x")
                        .step_by(0.01),
                );
            }
            Kind::TotalMass => {
                ui.add(
                    egui::Slider::new(&mut self.mass_kg, MIN_MASS_KG..=MAX_MASS_KG)
                        .fixed_decimals(0)
                        .suffix(" kg")
                        .step_by(1.0),
                );
            }
            Kind::FuelTank => {
                ui.add(
                    egui::Slider::new(&mut self.fuel_l, MIN_FUEL_L..=MAX_FUEL_L)
                        .fixed_decimals(1)
                        .suffix(" L")
                        .step_by(0.5),
                );
            }
            Kind::IdealPressure | Kind::StaticPressure => {
                ui.add(
                    egui::Slider::new(&mut self.pressure_psi, MIN_PRESSURE_PSI..=MAX_PRESSURE_PSI)
                        .fixed_decimals(1)
                        .suffix(" psi")
                        .step_by(0.1),
                );
            }
            Kind::Camber => {
                ui.add(
                    egui::Slider::new(&mut self.camber_deg, MIN_CAMBER_DEG..=MAX_CAMBER_DEG)
                        .fixed_decimals(2)
                        .suffix(" deg")
                        .step_by(0.05),
                );
            }
            Kind::BaseHeight => {
                ui.add(
                    egui::Slider::new(&mut self.base_height_m, MIN_BASE_HEIGHT_M..=MAX_BASE_HEIGHT_M)
                        .fixed_decimals(3)
                        .suffix(" m")
                        .step_by(0.001),
                );
            }
        }
    }

    /// The core library operation for the current kind and value.
    fn tuning(&self) -> Tuning {
        match self.kind {
            Kind::TorqueCurve => Tuning::TorqueCurve(self.factor),
            Kind::TotalMass => Tuning::TotalMass(self.mass_kg),
            Kind::FuelTank => Tuning::FuelTank(self.fuel_l),
            Kind::GearRatios => Tuning::GearRatios(self.factor),
            Kind::FinalDrive => Tuning::FinalDrive(self.factor),
            Kind::IdealPressure => Tuning::IdealPressure {
                axle: self.axle,
                psi: self.pressure_psi,
            },
            Kind::StaticPressure => Tuning::StaticPressure {
                axle: self.axle,
                psi: self.pressure_psi,
            },
            Kind::Camber => Tuning::Camber {
                axle: self.axle,
                degrees: self.camber_deg,
            },
            Kind::BaseHeight => Tuning::BaseHeight {
                axle: self.axle,
                metres: self.base_height_m,
            },
        }
    }

    /// Takes the finished background jobs, if any. Never blocks.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Cars { root, cars } => {
                    if self.root.as_deref() == Some(root.as_path()) {
                        self.cars = cars;
                        self.scanning = false;
                    }
                }
                Msg::Loaded(result) => match result {
                    Ok(profile) => {
                        notices.push(Notice {
                            text: format!("Loaded profile {} with {} step(s).", profile.name, profile.steps.len()),
                            ok: true,
                        });
                        self.profile_name = profile.name;
                        self.steps = profile.steps;
                    }
                    Err(message) => notices.push(Notice {
                        text: format!("Could not load profile. {message}"),
                        ok: false,
                    }),
                },
                Msg::ProfileApplied { car, result } => {
                    self.busy = false;
                    match result {
                        Ok(changes) if changes.is_empty() => notices.push(Notice {
                            text: format!("{car}: profile ran, no value changed, nothing was written."),
                            ok: true,
                        }),
                        Ok(changes) => {
                            notices.push(Notice {
                                text: format!(
                                    "{car}: profile applied, {} change(s) written. The previous files are kept as backups.",
                                    changes.len()
                                ),
                                ok: true,
                            });
                            for change in &changes {
                                notices.push(Notice {
                                    text: change_line(change),
                                    ok: true,
                                });
                            }
                        }
                        Err(err) => {
                            notices.push(Notice {
                                text: format!("{car}: profile stopped. {err}"),
                                ok: false,
                            });
                            for change in &err.changes {
                                notices.push(Notice {
                                    text: change_line(change),
                                    ok: true,
                                });
                            }
                        }
                    }
                }
                Msg::Applied { car, result } => {
                    self.busy = false;
                    match result {
                        Ok(changes) if changes.is_empty() => notices.push(Notice {
                            text: format!("{car}: no value changed, nothing was written."),
                            ok: true,
                        }),
                        Ok(changes) => {
                            notices.push(Notice {
                                text: format!(
                                    "{car}: {} change(s) written. The previous file is kept as a backup.",
                                    changes.len()
                                ),
                                ok: true,
                            });
                            for change in &changes {
                                notices.push(Notice {
                                    text: change_line(change),
                                    ok: true,
                                });
                            }
                        }
                        Err(message) => notices.push(Notice {
                            text: format!("{car}: tuning failed. {message}"),
                            ok: false,
                        }),
                    }
                }
            }
        }
    }

    /// Scans the car folders on a background thread.
    fn start_scan(&mut self, egui_ctx: &egui::Context) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.scanning = true;
        let tx = self.tx.clone();
        let egui_ctx = egui_ctx.clone();
        std::thread::spawn(move || {
            let cars = content::scan_cars(&root);
            if tx.send(Msg::Cars { root, cars }).is_ok() {
                egui_ctx.request_repaint();
            }
        });
    }

    /// The folder and name of the selected car.
    fn selected_car(&self) -> Option<(PathBuf, String)> {
        let id = self.selected.as_deref()?;
        let car = self.cars.iter().find(|c| c.id == id)?;
        Some((car.path.clone(), car.name.clone()))
    }

    /// The number the page holds for the selected kind. It becomes the step's value.
    fn current_value(&self) -> f64 {
        match self.kind {
            Kind::TorqueCurve | Kind::GearRatios | Kind::FinalDrive => self.factor,
            Kind::TotalMass => self.mass_kg,
            Kind::FuelTank => self.fuel_l,
            Kind::IdealPressure | Kind::StaticPressure => self.pressure_psi,
            Kind::Camber => self.camber_deg,
            Kind::BaseHeight => self.base_height_m,
        }
    }

    /// The profile as it stands on the page.
    fn profile(&self) -> TuningProfile {
        TuningProfile {
            name: self.profile_name.trim().to_string(),
            steps: self.steps.clone(),
        }
    }

    /// Saves the page's profile under the AC root. Always a new file.
    fn save_current(&self, root: &Path, notices: &mut Vec<Notice>) {
        let profile = self.profile();
        match tuning_profiles::save_profile(root, &profile) {
            Ok(path) => notices.push(Notice {
                text: format!("Saved profile {} to {}.", profile.name, path.display()),
                ok: true,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => notices.push(Notice {
                text: format!("A profile named {} already exists. Choose another name.", profile.name),
                ok: false,
            }),
            Err(e) => notices.push(Notice {
                text: format!("Could not save profile. {e}"),
                ok: false,
            }),
        }
    }

    /// Opens the profile picker and reads the chosen file. Runs on a background thread.
    fn start_load(&self, root: &Path, egui_ctx: &egui::Context) {
        let dir = root.join(PROFILE_DIR);
        let tx = self.tx.clone();
        let egui_ctx = egui_ctx.clone();
        std::thread::spawn(move || {
            let Some(path) = pick_profile(Some(dir)) else {
                return;
            };
            let result = tuning_profiles::load_profile(&path).map_err(|e| format!("{}: {e}", path.display()));
            if tx.send(Msg::Loaded(result)).is_ok() {
                egui_ctx.request_repaint();
            }
        });
    }

    /// Runs the page's profile on the selected car on a background thread.
    fn start_apply_profile(&mut self, egui_ctx: &egui::Context) {
        let Some((path, name)) = self.selected_car() else {
            return;
        };
        let profile = self.profile();
        self.busy = true;
        let tx = self.tx.clone();
        let egui_ctx = egui_ctx.clone();
        std::thread::spawn(move || {
            let result = tuning_profiles::apply_profile(&path, &profile);
            if tx.send(Msg::ProfileApplied { car: name, result }).is_ok() {
                egui_ctx.request_repaint();
            }
        });
    }

    /// Runs the selected change on a background thread.
    fn start_apply(&mut self, egui_ctx: &egui::Context) {
        let Some((path, name)) = self.selected_car() else {
            return;
        };
        let tuning = self.tuning();
        self.busy = true;
        let tx = self.tx.clone();
        let egui_ctx = egui_ctx.clone();
        std::thread::spawn(move || {
            let result = car_tuning::apply(&path, tuning).map_err(|e| e.to_string());
            if tx.send(Msg::Applied { car: name, result }).is_ok() {
                egui_ctx.request_repaint();
            }
        });
    }
}

/// One notice line for a changed key, e.g. `car.ini [BASIC] TOTALMASS: 1180.5 -> 1200`.
fn change_line(change: &Change) -> String {
    let place = if change.section.is_empty() {
        change.file.clone()
    } else {
        format!("{} [{}]", change.file, change.section)
    };
    format!("{place} {}: {} -> {}", change.key, change.before, change.after)
}

/// One line for a profile step, e.g. `Front axle ideal pressure 30 psi`.
fn step_label(step: &TuningStep) -> String {
    let v = step.value;
    match step.kind {
        Tuning::TorqueCurve(_) => format!("Torque curve x {v}"),
        Tuning::TotalMass(_) => format!("Total mass {v} kg"),
        Tuning::FuelTank(_) => format!("Fuel tank {v} L"),
        Tuning::GearRatios(_) => format!("Gear ratios x {v}"),
        Tuning::FinalDrive(_) => format!("Final drive x {v}"),
        Tuning::IdealPressure { axle, .. } => format!("{} ideal pressure {v} psi", axle_label(axle)),
        Tuning::StaticPressure { axle, .. } => format!("{} static pressure {v} psi", axle_label(axle)),
        Tuning::Camber { axle, .. } => format!("{} camber {v} deg", axle_label(axle)),
        Tuning::BaseHeight { axle, .. } => format!("{} base height {v} m", axle_label(axle)),
    }
}

/// Opens the profile picker. Runs on the background thread.
fn pick_profile(dir: Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().add_filter("Tuning profile", &["json"]);
    if let Some(dir) = dir.filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}
