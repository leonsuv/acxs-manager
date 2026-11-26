//! Car Specs tool page: pick an installed car to see its specs and a shift-light
//! preview, then compare it with a second car.
//!
//! Every file read happens on a background thread. Results come back through a
//! channel that the UI drains with `try_recv` each frame, and the worker wakes
//! the UI with `request_repaint`.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::car_compare::{self, IniChange, SpecDiff};
use acxs_manager::badges;
use acxs_manager::cardata::{self, AeroData, CarSpecs, DrivetrainData, DynoCurve, TyreCompound};
use acxs_manager::content::{self, CarEntry};
use acxs_manager::game;
use acxs_manager::sfx::{self, SfxInfo};
use acxs_manager::shift_lights::{self, ShiftProfile};
use acxs_manager::thumbnails;
use eframe::egui::{self, Align2, Color32, FontId, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

const ROW_HEIGHT: f32 = 56.0;
/// Edge length in points of a thumbnail in the car lists.
const LIST_THUMB: f32 = 40.0;
/// Edge length in points of the thumbnail shown in the car details.
const DETAIL_THUMB: f32 = 160.0;
/// Edge length in pixels of the square thumbnail made by the "Make thumbnail" button.
const THUMB_SIZE: u32 = 512;
/// Edge length in pixels of the square badge made by the "Make badge" button.
const BADGE_SIZE: u32 = 256;
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
        badge_busy: false,
        detail_tab: 0,
        donor_car: None,
        aero_speed_kmh: 200.0,
        tx,
        rx,
    })
}

/// Result of a background job.
enum Msg {
    Cars { root: PathBuf, cars: Vec<CarEntry> },
    Specs {
        id: String,
        specs: CarSpecs,
        rev_limit: Option<f64>,
        dyno: Option<DynoCurve>,
        drivetrain: Option<DrivetrainData>,
        aero: Option<AeroData>,
        tyres: Vec<TyreCompound>,
        sfx: SfxInfo,
    },
    Compare { left: String, right: String, diffs: Vec<SpecDiff>, changes: Result<Vec<IniChange>, String> },
    /// A thumbnail was made or found in the cache: its file path and size in bytes.
    Thumbnail { result: Result<(PathBuf, usize), String> },
    /// A make badge was written: its file path.
    Badge { result: Result<PathBuf, String> },
}

/// Specs of the selected car, once read.
struct Loaded {
    id: String,
    specs: CarSpecs,
    rev_limit: Option<f64>,
    dyno: Option<DynoCurve>,
    drivetrain: Option<DrivetrainData>,
    aero: Option<AeroData>,
    tyres: Vec<TyreCompound>,
    sfx: SfxInfo,
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
    /// A make badge is being written for the selected car.
    badge_busy: bool,
    /// Detail sub-tab: 0 = Overview & Compare, 1 = Dyno/Gearing/Aero/Tyres, 2 = SFX & Sound Swap.
    detail_tab: usize,
    donor_car: Option<String>,
    aero_speed_kmh: f64,
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
            let dyno = cardata::read_dyno(&path);
            let drivetrain = cardata::read_drivetrain(&path);
            let aero = cardata::read_aero(&path);
            let tyres = cardata::read_tyre_curves(&path);
            let sfx = sfx::inspect_car_sfx(&path);
            Msg::Specs {
                id,
                specs,
                rev_limit,
                dyno,
                drivetrain,
                aero,
                tyres,
                sfx,
            }
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
    fn poll(&mut self, egui_ctx: &egui::Context, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Thumbnail { result } => {
                    self.thumb_busy = false;
                    match result {
                        Ok((path, size)) => {
                            // The file was overwritten, so drop the decoded copy egui kept.
                            egui_ctx.forget_image(&thumb_uri(&path));
                            notices.push(Notice {
                            text: format!(
                                "Thumbnail saved to {} ({THUMB_SIZE}x{THUMB_SIZE}, {size} bytes)",
                                path.display()
                            ),
                            ok: true,
                        });
                        }
                        Err(e) => notices.push(Notice {
                            text: format!("Thumbnail failed: {e}"),
                            ok: false,
                        }),
                    }
                }
                Msg::Badge { result } => {
                    self.badge_busy = false;
                    match result {
                        Ok(path) => notices.push(Notice {
                            text: format!("Badge saved to {}", path.display()),
                            ok: true,
                        }),
                        Err(e) => notices.push(Notice {
                            text: format!("Badge failed: {e}"),
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
                Msg::Specs {
                    id,
                    specs,
                    rev_limit,
                    dyno,
                    drivetrain,
                    aero,
                    tyres,
                    sfx,
                } => {
                    if self.pending_specs.as_deref() == Some(id.as_str()) {
                        self.pending_specs = None;
                    }
                    if self.selected.as_deref() == Some(id.as_str()) {
                        if let Some(limit) = rev_limit {
                            self.rpm = self.rpm.min(limit);
                        }
                        self.loaded = Some(Loaded {
                            id,
                            specs,
                            rev_limit,
                            dyno,
                            drivetrain,
                            aero,
                            tyres,
                            sfx,
                        });
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
        if let (Some(left), Some(right)) = (self.selected.clone(), self.compare_id.clone())
            && left != right
        {
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

    /// Thumbnail button, the cached thumbnail path and a button to open its folder.
    fn thumbnail_section(&mut self, ui: &mut egui::Ui, car: &CarEntry, notices: &mut Vec<Notice>) {
        let Some(root) = self.root.clone() else { return };
        let badge_root = root.clone();
        let id = car.id.as_str();
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
            car_thumb(ui, Some(root.as_path()), car, DETAIL_THUMB);
            ui.add_space(8.0);
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
            ui.horizontal(|ui| {
                let badge = ui.add_enabled(!self.badge_busy, egui::Button::new("Make badge"));
                if badge.clicked() {
                    self.badge_busy = true;
                    let id = id.to_string();
                    let brand = self
                        .cars
                        .iter()
                        .find(|c| c.id == id)
                        .map(|c| if c.brand.trim().is_empty() { c.name.clone() } else { c.brand.clone() })
                        .unwrap_or_else(|| id.clone());
                    let root = badge_root.clone();
                    spawn_job(ui.ctx(), &self.tx, move || {
                        let result = badges::write_badge(&root, &id, &brand, BADGE_SIZE)
                            .map_err(|e| e.to_string());
                        Msg::Badge { result }
                    });
                }
                if self.badge_busy {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new("Working...").color(theme::MUTED));
                }
            });
            ui.add_space(8.0);
            widgets::key_value(ui, "Cached thumbnail", &shown);
            ui.horizontal(|ui| {
                let open = ui.add_enabled(cache.is_dir(), egui::Button::new("Open folder"));
                if open.clicked()
                    && let Err(e) = open_folder(&cache)
                {
                    notices.push(Notice {
                        text: format!("Could not open {}: {e}", cache.display()),
                        ok: false,
                    });
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
        ui.add_space(8.0);
        widgets::animated_tabs(
            ui,
            "car_specs_detail_tabs",
            &mut self.detail_tab,
            &[
                (0usize, "Specs & Compare", None),
                (1usize, "Dyno / Aero / Tyres", None),
                (2usize, "SFX & Sound Swap", None),
            ],
        );
        ui.add_space(10.0);

        if self.detail_tab == 1 {
            self.draw_curves_tab(ui, &selected);
            return None;
        }
        if self.detail_tab == 2 {
            self.draw_sfx_tab(ui, &car, notices);
            return None;
        }

        self.thumbnail_section(ui, &car, notices);

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
            self.root.as_deref(),
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

    fn draw_curves_tab(&mut self, ui: &mut egui::Ui, selected: &str) {
        let Some(loaded) = self.loaded.as_ref().filter(|l| l.id == selected) else {
            ui.label(RichText::new("Reading physics curves from data...").color(theme::MUTED));
            return;
        };

        widgets::section_title(ui, "Engine Dyno (Power & Torque)");
        theme::card_elevated().show(ui, |ui| {
            if let Some(dyno) = &loaded.dyno {
                ui.horizontal_wrapped(|ui| {
                    widgets::pill(
                        ui,
                        &format!("Peak {:.0} kW / {:.0} hp", dyno.peak_power_kw, dyno.peak_power_kw * 1.341_022_1),
                        theme::CH_SPEED,
                    );
                    widgets::pill(ui, &format!("Peak {:.0} Nm", dyno.peak_torque_nm), theme::CH_THROTTLE);
                    if let Some(lim) = dyno.limiter_rpm {
                        widgets::pill(ui, &format!("Limiter {:.0} rpm", lim), theme::WARN);
                    }
                });
                ui.add_space(6.0);
                draw_dyno_plot(ui, dyno);
            } else {
                ui.label(RichText::new("No power.lut / engine.ini dyno curve found.").color(theme::MUTED));
            }
        });

        ui.add_space(12.0);
        widgets::section_title(ui, "Drivetrain & Gear Speeds");
        theme::card().show(ui, |ui| {
            if let Some(dt) = &loaded.drivetrain {
                let top_speeds = dt.gear_top_speeds_kmh();
                widgets::key_value(ui, "Traction", dt.traction.as_deref().unwrap_or("—"));
                widgets::key_value(ui, "Final drive", &format!("{:.3}", dt.final_ratio));
                ui.add_space(4.0);
                for (i, (ratio, speed)) in dt.gears.iter().zip(top_speeds.iter()).enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("G{}", i + 1)).font(theme::mono(12.5)).strong());
                        ui.label(RichText::new(format!("ratio {:.3}", ratio)).font(theme::mono(12.0)).color(theme::MUTED));
                        ui.label(RichText::new(format!("-> {:.0} km/h @ {:.0} rpm", speed, dt.limiter_rpm)).font(theme::mono(12.0)).color(theme::CH_SPEED));
                    });
                }
            } else {
                ui.label(RichText::new("No drivetrain.ini gear data found.").color(theme::MUTED));
            }
        });

        ui.add_space(12.0);
        widgets::section_title(ui, "Aerodynamics");
        theme::card().show(ui, |ui| {
            if let Some(aero) = &loaded.aero {
                ui.horizontal(|ui| {
                    ui.label("Airspeed (km/h)");
                    ui.add(egui::Slider::new(&mut self.aero_speed_kmh, 50.0..=380.0).fixed_decimals(0));
                });
                let f = aero.forces_at_speed(self.aero_speed_kmh);
                widgets::key_value(ui, "Total downforce", &format!("{:.0} N ({:.0} kgf)", f.total_downforce_n, f.total_downforce_n / 9.80665));
                widgets::key_value(ui, "Front / Rear", &format!("{:.0} N / {:.0} N ({:.1}% front)", f.front_downforce_n, f.rear_downforce_n, f.front_balance_pct));
                widgets::key_value(ui, "Total drag", &format!("{:.0} N", f.total_drag_n));
                widgets::key_value(ui, "Wings / elements", &aero.wings.len().to_string());
            } else {
                ui.label(RichText::new("No aero.ini wing data found.").color(theme::MUTED));
            }
        });

        ui.add_space(12.0);
        widgets::section_title(ui, "Tyre Compounds");
        theme::card().show(ui, |ui| {
            if loaded.tyres.is_empty() {
                ui.label(RichText::new("No tyre compound curves found in tyres.ini.").color(theme::MUTED));
            } else {
                for comp in &loaded.tyres {
                    ui.horizontal_wrapped(|ui| {
                        widgets::pill(ui, &format!("#{}", comp.index), theme::INFO);
                        ui.label(RichText::new(format!("F: {} / R: {}", comp.front_name, comp.rear_name)).strong());
                        if let (Some(fp), Some(rp)) = (comp.front_ideal_psi, comp.rear_ideal_psi) {
                            ui.label(
                                RichText::new(format!("Ideal {:.0}/{:.0} psi", fp, rp))
                                    .font(theme::mono(12.0))
                                    .color(theme::MUTED),
                            );
                        }
                    });
                }
            }
        });
    }

    fn draw_sfx_tab(&mut self, ui: &mut egui::Ui, car: &CarEntry, notices: &mut Vec<Notice>) {
        let Some(root) = self.root.clone() else { return };
        let Some(info) = self
            .loaded
            .as_ref()
            .filter(|l| l.id == car.id)
            .map(|l| l.sfx.clone())
        else {
            ui.label(RichText::new("Inspecting sfx/...").color(theme::MUTED));
            return;
        };

        widgets::section_title(ui, "SFX Audit");
        theme::card_elevated().show(ui, |ui| {
            widgets::key_value(ui, "Bank files", &info.bank_files.len().to_string());
            widgets::key_value(
                ui,
                "GUIDs.txt",
                if info.guids_path.is_some() { "present" } else { "missing" },
            );
            if !info.referenced_car_ids.is_empty() {
                widgets::key_value(ui, "Referenced car IDs", &info.referenced_car_ids.join(", "));
            }
            ui.add_space(6.0);
            if info.issues.is_empty() {
                ui.label(RichText::new("SFX bank and GUIDs.txt look healthy.").color(theme::OK));
            } else {
                for issue in &info.issues {
                    ui.label(RichText::new(format!("• {issue}")).color(theme::WARN));
                }
                ui.add_space(6.0);
                if ui.button("Auto-fix GUIDs.txt car ID mismatch").clicked() {
                    let running = game::is_game_running();
                    match sfx::fix_guids_mismatch(&car.path, running) {
                        Ok(Some(backup)) => {
                            self.loaded = None;
                            notices.push(Notice {
                                text: format!("Fixed GUIDs.txt (backup: {}).", backup.display()),
                                ok: true,
                            });
                        }
                        Ok(None) => notices.push(Notice {
                            text: "No GUIDs.txt mismatch needed fixing.".into(),
                            ok: true,
                        }),
                        Err(e) => notices.push(Notice {
                            text: format!("Could not fix GUIDs.txt: {e}"),
                            ok: false,
                        }),
                    }
                }
            }
        });

        ui.add_space(12.0);
        widgets::section_title(ui, "Sound Swapper");
        theme::card().show(ui, |ui| {
            ui.label(
                RichText::new(
                    "Copy a donor car's .bank and rewritten GUIDs.txt into this car. \
                     The current sfx/ folder is archived to sfx.acxs-backup-<stamp>/ first.",
                )
                .small()
                .color(theme::MUTED),
            );
            ui.add_space(8.0);
            let donor_label = self.donor_car.as_deref().unwrap_or("Choose donor car...");
            egui::ComboBox::from_id_salt("sfx_donor_picker")
                .selected_text(donor_label)
                .width(320.0)
                .show_ui(ui, |ui| {
                    for c in &self.cars {
                        if c.id == car.id {
                            continue;
                        }
                        let selected = self.donor_car.as_deref() == Some(c.id.as_str());
                        if ui
                            .selectable_label(selected, format!("{} ({})", c.name, c.id))
                            .clicked()
                        {
                            self.donor_car = Some(c.id.clone());
                        }
                    }
                });
            ui.add_space(8.0);
            let can_swap = self.donor_car.as_deref().is_some_and(|d| d != car.id);
            if ui
                .add_enabled(can_swap, widgets::primary_button("Replace Sound from Donor"))
                .clicked()
                && let Some(donor) = self.donor_car.clone()
            {
                let running = game::is_game_running();
                match sfx::swap_car_sound(&root, &car.id, &donor, running) {
                    Ok(report) => {
                        self.loaded = None;
                        notices.push(Notice {
                            text: format!(
                                "Replaced {} sound with {} (previous sfx backed up).",
                                report.target_car_id, report.donor_car_id
                            ),
                            ok: true,
                        });
                    }
                    Err(e) => notices.push(Notice {
                        text: format!("Sound swap failed: {e}"),
                        ok: false,
                    }),
                }
            }
        });
    }
}

fn draw_dyno_plot(ui: &mut egui::Ui, dyno: &DynoCurve) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(200.0), 140.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::SURFACE_BASE);
    painter.rect_stroke(rect, 8.0, egui::Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
    if dyno.points.len() < 2 {
        return;
    }
    let min_rpm = dyno.points.first().map(|p| p.rpm).unwrap_or(0.0);
    let max_rpm = dyno.points.last().map(|p| p.rpm).unwrap_or(1.0).max(min_rpm + 1.0);
    let max_kw = dyno.peak_power_kw.max(1.0);
    let max_nm = dyno.peak_torque_nm.max(1.0);
    let pad = 10.0;
    let inner = rect.shrink(pad);

    let to_pt = |rpm: f64, norm_y: f64| {
        let x = inner.left() + ((rpm - min_rpm) / (max_rpm - min_rpm)) as f32 * inner.width();
        let y = inner.bottom() - (norm_y.clamp(0.0, 1.0) as f32) * inner.height();
        egui::pos2(x, y)
    };
    let power_pts: Vec<egui::Pos2> = dyno.points.iter().map(|p| to_pt(p.rpm, p.power_kw / max_kw)).collect();
    let torque_pts: Vec<egui::Pos2> = dyno.points.iter().map(|p| to_pt(p.rpm, p.torque_nm / max_nm)).collect();
    painter.add(egui::Shape::line(power_pts, egui::Stroke::new(2.0, theme::CH_SPEED)));
    painter.add(egui::Shape::line(torque_pts, egui::Stroke::new(2.0, theme::CH_THROTTLE)));
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
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to see car specs.");
            return;
        };
        let egui_ctx = ui.ctx().clone();
        install_loaders_once(&egui_ctx);

        self.poll(&egui_ctx, notices);
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
                    self.root.as_deref(),
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
        if let Some(id) = picked_car
            && self.selected.as_deref() != Some(id.as_str())
        {
            self.selected = Some(id);
            self.loaded = None;
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
    root: Option<&Path>,
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
            if widgets::list_row(ui, selected == Some(car.id.as_str()), |ui| car_row(ui, root, car)) {
                picked = Some(car.id.clone());
            }
        }
    });
    picked
}

fn car_row(ui: &mut egui::Ui, root: Option<&Path>, car: &CarEntry) {
    ui.horizontal(|ui| {
        car_thumb(ui, root, car, LIST_THUMB);
        ui.vertical(|ui| {
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
        });
    });
}

/// Loads egui's image loaders the first time the page draws. The flag lives in egui memory.
fn install_loaders_once(egui_ctx: &egui::Context) {
    let flag = egui::Id::unique("car_specs_image_loaders");
    if !egui_ctx.data(|d| d.get_temp::<bool>(flag).unwrap_or(false)) {
        egui_extras::install_image_loaders(egui_ctx);
        egui_ctx.data_mut(|d| d.insert_temp(flag, true));
    }
}

/// The `file://` URI egui uses to load a thumbnail file.
fn thumb_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// The car's cached thumbnail, drawn `size` points square. Without one, a grey square
/// with the brand initials.
fn car_thumb(ui: &mut egui::Ui, root: Option<&Path>, car: &CarEntry, size: f32) {
    let thumb = root.map(|root| thumbnails::cache_path(root, &car.id).join(format!("{THUMB_SIZE}.png")));
    let side = egui::vec2(size, size);
    match thumb.filter(|path| path.is_file()) {
        Some(path) => {
            ui.add(egui::Image::from_uri(thumb_uri(&path)).fit_to_exact_size(side));
        }
        None => {
            let (rect, _) = ui.allocate_exact_size(side, egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 6.0, theme::CARD_HOVER);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                brand_initials(car),
                FontId::proportional(size * 0.36),
                theme::MUTED,
            );
        }
    }
}

/// Up to two initials from the brand, or from the name when there is no brand.
fn brand_initials(car: &CarEntry) -> String {
    let source = if car.brand.trim().is_empty() { &car.name } else { &car.brand };
    let words: Vec<&str> = source
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let out: String = match words.as_slice() {
        [] => "?".to_string(),
        [first, second, ..] => [first, second].iter().filter_map(|w| w.chars().next()).collect(),
        [only] => only.chars().take(2).collect(),
    };
    out.to_uppercase()
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
