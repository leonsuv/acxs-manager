//! Car Tuning tool page: pick an installed car, choose one physical value and
//! change it. Writes run on a background thread. Results come back through a
//! channel that the UI drains with `try_recv` each frame, and the worker wakes
//! the UI with `request_repaint`. Each change is reported as notices.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::car_tuning::{
    self, Change, Tuning, MAX_FACTOR, MAX_FUEL_L, MAX_MASS_KG, MIN_FACTOR, MIN_FUEL_L, MIN_MASS_KG,
};
use acxs_manager::content::{self, CarEntry};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
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
        factor: 1.0,
        mass_kg: 1200.0,
        fuel_l: 60.0,
        busy: false,
        tx,
        rx,
    })
}

/// Result of a background job.
enum Msg {
    Cars { root: PathBuf, cars: Vec<CarEntry> },
    Applied { car: String, result: Result<Vec<Change>, String> },
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
}

impl Kind {
    const ALL: [Kind; 5] = [Kind::TorqueCurve, Kind::TotalMass, Kind::FuelTank, Kind::GearRatios, Kind::FinalDrive];

    fn label(self) -> &'static str {
        match self {
            Kind::TorqueCurve => "Torque curve",
            Kind::TotalMass => "Total mass",
            Kind::FuelTank => "Fuel tank",
            Kind::GearRatios => "Gear ratios",
            Kind::FinalDrive => "Final drive",
        }
    }

    /// Factors multiply the current value. Mass and tank size are set to a new value.
    fn is_factor(self) -> bool {
        !matches!(self, Kind::TotalMass | Kind::FuelTank)
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
    /// Multiplier used by the factor kinds.
    factor: f64,
    /// Value used by the total mass kind, in kg.
    mass_kg: f64,
    /// Value used by the fuel tank kind, in litres.
    fuel_l: f64,
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
            "Scale or set the torque, mass, fuel tank, gears or final drive of an installed car.",
        );
        self.poll(notices);

        let Some(root) = ctx.ac_root.as_deref().filter(|_| ctx.ac_valid) else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to tune cars.");
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

    /// Runs the selected change on a background thread.
    fn start_apply(&mut self, egui_ctx: &egui::Context) {
        let Some(id) = self.selected.as_deref() else {
            return;
        };
        let Some(car) = self.cars.iter().find(|c| c.id == id) else {
            return;
        };
        let path = car.path.clone();
        let name = car.name.clone();
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
