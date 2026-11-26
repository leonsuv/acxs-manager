//! Metadata & Checks page: edit the `ui_car.json` / `ui_track.json` fields the game
//! shows, run the read-only content checks, and apply the safe repairs the checks offer.
//!
//! Scans, metadata reads, saves, checks, repair planning and repair runs happen on
//! background threads. They report back through a channel, which is drained at the start
//! of each frame.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, INFO, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::content::{filter_cars, filter_tracks, scan_cars, scan_tracks, CarEntry, TrackEntry};
use acxs_manager::content_meta::{
    apply_track_patch, patch_car_meta, patch_track_meta, read_track_layout_meta,
    save_track_layout_meta, CarMeta, TrackMeta,
};
use acxs_manager::content_repair::{
    apply_repairs, check_all, plan_extended_repairs, summary, Issue, Repair, RepairAction,
    RepairReport, Severity,
};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const LIST_WIDTH: f32 = 320.0;
const LIST_HEIGHT: f32 = 420.0;
const ISSUES_HEIGHT: f32 = 320.0;
const ISSUE_ROW_HEIGHT: f32 = 34.0;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(MetadataChecks {
        mode: Mode::Cars,
        query: String::new(),
        cars: Vec::new(),
        tracks: Vec::new(),
        list_root: None,
        scanning: false,
        selected: None,
        form: Form::default(),
        saving: false,
        checking: false,
        issues: None,
        planning: false,
        applying: false,
        plan: None,
        tx,
        rx,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Cars,
    Tracks,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Cars => "Cars",
            Mode::Tracks => "Tracks",
        }
    }
}

/// Editable copy of the metadata. Exposes full CarMeta / TrackMeta fields.
#[derive(Clone, Default)]
struct Form {
    name: String,
    brand: String,
    class: String,
    year: String,
    tags: String,
    spec_bhp: String,
    spec_torque: String,
    spec_weight: String,
    spec_topspeed: String,
    spec_accel: String,
    spec_pwratio: String,
    city: String,
    country: String,
    author: String,
    version: String,
    length_m: String,
    pitboxes: String,
    description: String,
    /// Selected track layout (None = root `ui/ui_track.json`).
    layout: Option<String>,
    layouts: Vec<String>,
}

#[derive(Clone)]
struct Selection {
    mode: Mode,
    path: PathBuf,
    id: String,
    /// True once the metadata file has been read into the form.
    ready: bool,
    error: Option<String>,
}

/// One row of the searchable list.
#[derive(Clone)]
struct Row {
    path: PathBuf,
    id: String,
    title: String,
    subtitle: String,
}

enum Patch {
    Car(CarMeta),
    Track(TrackMeta),
}

enum Msg {
    Listed {
        root: PathBuf,
        cars: Vec<CarEntry>,
        tracks: Vec<TrackEntry>,
    },
    Loaded {
        mode: Mode,
        path: PathBuf,
        result: Result<Form, String>,
    },
    Saved {
        id: String,
        result: Result<(), String>,
    },
    Checked(Vec<Issue>),
    Planned {
        root: PathBuf,
        repairs: Vec<Repair>,
    },
    Repaired(RepairReport),
}

/// A repair plan for one AC root. Every repair starts selected.
struct RepairPlan {
    root: PathBuf,
    items: Vec<PlanItem>,
}

struct PlanItem {
    repair: Repair,
    selected: bool,
}

struct MetadataChecks {
    mode: Mode,
    query: String,
    cars: Vec<CarEntry>,
    tracks: Vec<TrackEntry>,
    /// The AC root the lists were scanned (or are being scanned) for.
    list_root: Option<PathBuf>,
    scanning: bool,
    selected: Option<Selection>,
    form: Form,
    saving: bool,
    checking: bool,
    issues: Option<Vec<Issue>>,
    planning: bool,
    applying: bool,
    plan: Option<RepairPlan>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Tool for MetadataChecks {
    fn name(&self) -> &'static str {
        "Metadata & Checks"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let ectx = ui.ctx().clone();
        self.drain(notices);
        widgets::heading(
            ui,
            "Metadata & Checks",
            "Edit the names and details the game shows for cars and tracks, and check the installed content for problems.",
        );

        let root = match (&ctx.ac_root, ctx.ac_valid) {
            (Some(root), true) => root.clone(),
            _ => {
                widgets::empty_state(
                    ui,
                    "No valid Assetto Corsa folder is set. Set it in Settings to edit metadata and run checks.",
                );
                return;
            }
        };
        self.ensure_list(&ectx, &root);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.mode_row(ui);
                ui.horizontal_top(|ui| {
                    self.list_column(ui, &ectx);
                    ui.add_space(16.0);
                    self.editor_column(ui, &ectx, notices);
                });
                ui.add_space(16.0);
                self.checks_section(ui, &ectx, &root);
            });
    }
}

impl MetadataChecks {
    /// Applies finished background work to the state. Runs once per frame.
    fn drain(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Listed { root, cars, tracks } => {
                    self.scanning = false;
                    if self.list_root.as_deref() == Some(root.as_path()) {
                        self.cars = cars;
                        self.tracks = tracks;
                    }
                }
                Msg::Loaded { mode, path, result } => {
                    if let Some(sel) = self
                        .selected
                        .as_mut()
                        .filter(|s| s.mode == mode && s.path == path)
                    {
                        match result {
                            Ok(form) => {
                                sel.ready = true;
                                sel.error = None;
                                self.form = form;
                            }
                            Err(e) => sel.error = Some(e),
                        }
                    }
                }
                Msg::Saved { id, result } => {
                    self.saving = false;
                    // Rescan so renamed entries show their new names.
                    self.list_root = None;
                    match result {
                        Ok(()) => notices.push(Notice {
                            text: format!("Saved metadata for {id}. The previous file was kept as a backup."),
                            ok: true,
                        }),
                        Err(e) => notices.push(Notice {
                            text: format!("Could not save {id}: {e}"),
                            ok: false,
                        }),
                    }
                }
                Msg::Checked(issues) => {
                    self.checking = false;
                    let (e, w, i) = summary(&issues);
                    notices.push(Notice {
                        text: format!("Checks finished: {e} errors, {w} warnings, {i} info."),
                        ok: e == 0,
                    });
                    self.issues = Some(issues);
                }
                Msg::Planned { root, repairs } => {
                    self.planning = false;
                    if self.list_root.as_deref() == Some(root.as_path()) {
                        let items = repairs
                            .into_iter()
                            .map(|repair| PlanItem { repair, selected: true })
                            .collect();
                        self.plan = Some(RepairPlan { root, items });
                    }
                }
                Msg::Repaired(report) => {
                    self.applying = false;
                    self.plan = None;
                    // The checks ran before the repairs, so their results are stale now.
                    self.issues = None;
                    notices.push(repair_notice(&report));
                }
            }
        }
    }

    /// Starts a scan when the AC root differs from the one last scanned.
    fn ensure_list(&mut self, ectx: &egui::Context, root: &Path) {
        if self.scanning || self.list_root.as_deref() == Some(root) {
            return;
        }
        self.scanning = true;
        self.list_root = Some(root.to_path_buf());
        let root = root.to_path_buf();
        spawn(&self.tx, ectx, move || Msg::Listed {
            cars: scan_cars(&root),
            tracks: scan_tracks(&root),
            root,
        });
    }

    fn rows(&self) -> Vec<Row> {
        match self.mode {
            Mode::Cars => filter_cars(&self.cars, &self.query)
                .into_iter()
                .map(|c| Row {
                    path: c.path.clone(),
                    id: c.id.clone(),
                    title: c.name.clone(),
                    subtitle: join_parts(&[
                        c.id.as_str(),
                        c.brand.as_str(),
                        c.car_class.as_str(),
                        c.year.as_str(),
                    ]),
                })
                .collect(),
            Mode::Tracks => filter_tracks(&self.tracks, &self.query)
                .into_iter()
                .map(|t| Row {
                    path: t.path.clone(),
                    id: t.id.clone(),
                    title: t.name.clone(),
                    subtitle: join_parts(&[t.id.as_str(), t.city.as_str(), t.country.as_str()]),
                })
                .collect(),
        }
    }

    fn mode_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for mode in [Mode::Cars, Mode::Tracks] {
                let count = match mode {
                    Mode::Cars => self.cars.len(),
                    Mode::Tracks => self.tracks.len(),
                };
                let text = format!("{} ({count})", mode.label());
                if ui.selectable_label(self.mode == mode, text).clicked() && self.mode != mode {
                    self.mode = mode;
                    self.selected = None;
                    self.form = Form::default();
                }
            }
        });
        ui.add_space(8.0);
    }

    fn list_column(&mut self, ui: &mut egui::Ui, ectx: &egui::Context) {
        ui.vertical(|ui| {
            ui.set_width(LIST_WIDTH);
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(f32::INFINITY)
                    .hint_text("Search"),
            );
            ui.add_space(8.0);

            let rows = self.rows();
            if rows.is_empty() {
                let total = match self.mode {
                    Mode::Cars => self.cars.len(),
                    Mode::Tracks => self.tracks.len(),
                };
                let text = if self.scanning {
                    "Scanning..."
                } else if total == 0 {
                    match self.mode {
                        Mode::Cars => "No cars found in this install.",
                        Mode::Tracks => "No tracks found in this install.",
                    }
                } else {
                    "No matches."
                };
                ui.label(RichText::new(text).color(MUTED));
            }

            let selected = self.selected.as_ref().map(|s| s.path.clone());
            let mut clicked: Option<Row> = None;
            egui::ScrollArea::vertical()
                .id_salt("meta_list")
                .max_height(LIST_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for row in &rows {
                        let is_selected = selected.as_deref() == Some(row.path.as_path());
                        let hit = widgets::list_row(ui, is_selected, |ui| {
                            ui.label(RichText::new(row.title.as_str()).strong());
                            ui.label(RichText::new(row.subtitle.as_str()).small().color(MUTED));
                        });
                        if hit {
                            clicked = Some(row.clone());
                        }
                    }
                });
            if let Some(row) = clicked {
                self.select(ectx, row);
            }
        });
    }

    /// Selects an entry and reads its metadata on a background thread.
    fn select(&mut self, ectx: &egui::Context, row: Row) {
        if self.selected.as_ref().is_some_and(|s| s.path == row.path) {
            return;
        }
        let mode = self.mode;
        self.selected = Some(Selection {
            mode,
            path: row.path.clone(),
            id: row.id,
            ready: false,
            error: None,
        });
        self.form = Form::default();
        let path = row.path;
        spawn(&self.tx, ectx, move || {
            let result = load(mode, &path);
            Msg::Loaded { mode, path, result }
        });
    }

    fn editor_column(&mut self, ui: &mut egui::Ui, ectx: &egui::Context, notices: &mut Vec<Notice>) {
        ui.vertical(|ui| {
            let Some(sel) = self.selected.clone() else {
                widgets::empty_state(ui, "Select a car or track on the left to edit its metadata.");
                return;
            };
            widgets::section_title(ui, &format!("{} \u{b7} {}", sel.mode.label(), sel.id));

            let saving = self.saving;
            let mut save_clicked = false;
            let form = &mut self.form;
            theme::card().show(ui, |ui| {
                widgets::key_value(ui, "Folder", &sel.path.display().to_string());
                ui.add_space(8.0);
                if let Some(err) = &sel.error {
                    ui.label(RichText::new(err.as_str()).color(DANGER));
                } else if !sel.ready {
                    ui.label(RichText::new("Loading metadata...").color(MUTED));
                }

                ui.add_enabled_ui(sel.ready, |ui| match sel.mode {
                    Mode::Cars => {
                        field(ui, "Name", &mut form.name);
                        field(ui, "Brand", &mut form.brand);
                        field(ui, "Class", &mut form.class);
                        field(ui, "Year", &mut form.year);
                        field(ui, "Tags (comma-separated)", &mut form.tags);
                        text_area(ui, "Description", &mut form.description);
                        ui.add_space(4.0);
                        ui.label(RichText::new("Specs").small().strong().color(MUTED));
                        ui.columns(2, |cols| {
                            field(&mut cols[0], "Power (bhp)", &mut form.spec_bhp);
                            field(&mut cols[1], "Torque (Nm)", &mut form.spec_torque);
                            field(&mut cols[0], "Weight (kg)", &mut form.spec_weight);
                            field(&mut cols[1], "Top speed", &mut form.spec_topspeed);
                            field(&mut cols[0], "0-100 km/h", &mut form.spec_accel);
                            field(&mut cols[1], "Weight/power", &mut form.spec_pwratio);
                        });
                    }
                    Mode::Tracks => {
                        if !form.layouts.is_empty() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Layout").small().color(MUTED));
                                let current = form.layout.as_deref().unwrap_or("(main)");
                                egui::ComboBox::from_id_salt("meta_track_layout")
                                    .selected_text(current)
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_label(form.layout.is_none(), "(main)").clicked() {
                                            form.layout = None;
                                        }
                                        for l in &form.layouts {
                                            let selected = form.layout.as_deref() == Some(l.as_str());
                                            if ui.selectable_label(selected, l).clicked() {
                                                form.layout = Some(l.clone());
                                            }
                                        }
                                    });
                            });
                            ui.add_space(6.0);
                        }
                        field(ui, "Name", &mut form.name);
                        ui.columns(2, |cols| {
                            field(&mut cols[0], "City", &mut form.city);
                            field(&mut cols[1], "Country", &mut form.country);
                            field(&mut cols[0], "Author", &mut form.author);
                            field(&mut cols[1], "Version", &mut form.version);
                            field(&mut cols[0], "Length (m)", &mut form.length_m);
                            field(&mut cols[1], "Pitboxes", &mut form.pitboxes);
                        });
                        field(ui, "Tags (comma-separated)", &mut form.tags);
                        text_area(ui, "Description", &mut form.description);
                    }
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(sel.ready && !saving, widgets::primary_button("Save"))
                        .clicked()
                    {
                        save_clicked = true;
                    }
                    if sel.mode == Mode::Cars
                        && ui
                            .add_enabled(sel.ready && !saving, egui::Button::new("Recalculate specs from data/"))
                            .clicked()
                    {
                        let repair = Repair {
                            description: format!("Recalculate specs for {}", sel.id),
                            path: sel.path.join("ui").join("ui_car.json"),
                            action: RepairAction::RecalculateCarSpecs,
                        };
                        if let Some(root) = sel.path.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
                            let rep = apply_repairs(root, &[repair]);
                            notices.push(repair_notice(&rep));
                            if rep.failed.is_empty()
                                && let Ok(reloaded) = load(Mode::Cars, &sel.path)
                            {
                                *form = reloaded;
                            }
                        }
                    }
                    if saving {
                        ui.label(RichText::new("Saving...").color(MUTED));
                    }
                });
            });

            if save_clicked {
                self.save(ectx, notices);
            }
        });
    }

    /// Builds the patch from the form and writes it on a background thread.
    /// `patch_*_meta` keeps a backup of the previous file.
    fn save(&mut self, ectx: &egui::Context, notices: &mut Vec<Notice>) {
        let Some(sel) = self.selected.clone() else {
            return;
        };
        if !sel.ready || self.saving {
            return;
        }
        let form = self.form.clone();
        let patch = match sel.mode {
            Mode::Cars => {
                let year_text = form.year.trim();
                let year = if year_text.is_empty() {
                    None
                } else {
                    match year_text.parse::<i64>() {
                        Ok(y) => Some(y),
                        Err(_) => {
                            notices.push(Notice {
                                text: format!("Year must be a whole number, got \"{year_text}\"."),
                                ok: false,
                            });
                            return;
                        }
                    }
                };
                let tags: Vec<String> = form
                    .tags
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut specs = std::collections::BTreeMap::new();
                for (k, v) in [
                    ("bhp", &form.spec_bhp),
                    ("torque", &form.spec_torque),
                    ("weight", &form.spec_weight),
                    ("topspeed", &form.spec_topspeed),
                    ("acceleration", &form.spec_accel),
                    ("pwratio", &form.spec_pwratio),
                ] {
                    if !v.trim().is_empty() {
                        specs.insert(k.to_string(), v.trim().to_string());
                    }
                }
                Patch::Car(CarMeta {
                    name: Some(form.name.trim().to_string()),
                    brand: Some(form.brand.trim().to_string()),
                    class: Some(form.class.trim().to_string()),
                    year,
                    tags: Some(tags),
                    description: Some(form.description.trim().to_string()),
                    specs: (!specs.is_empty()).then_some(specs),
                })
            }
            Mode::Tracks => {
                let tags: Vec<String> = form
                    .tags
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let length_m = form.length_m.trim().parse::<f64>().ok();
                let pitboxes = form.pitboxes.trim().parse::<u32>().ok();
                Patch::Track(TrackMeta {
                    name: Some(form.name.trim().to_string()),
                    city: Some(form.city.trim().to_string()),
                    country: Some(form.country.trim().to_string()),
                    author: (!form.author.trim().is_empty()).then(|| form.author.trim().to_string()),
                    version: (!form.version.trim().is_empty()).then(|| form.version.trim().to_string()),
                    length_m,
                    pitboxes,
                    tags: Some(tags),
                    description: Some(form.description.trim().to_string()),
                })
            }
        };

        self.saving = true;
        let path = sel.path;
        let id = sel.id;
        let layout = form.layout;
        spawn(&self.tx, ectx, move || {
            let result = match patch {
                Patch::Car(p) => patch_car_meta(&path, &p),
                Patch::Track(p) => match layout {
                    Some(l) => {
                        let layout_ui = path.join("ui").join(l);
                        read_track_layout_meta(&layout_ui).and_then(|mut map| {
                            apply_track_patch(&mut map, &p);
                            save_track_layout_meta(&layout_ui, &map)
                        })
                    }
                    None => patch_track_meta(&path, &p),
                },
            };
            Msg::Saved {
                id,
                result: result.map_err(|e| e.to_string()),
            }
        });
    }

    fn checks_section(&mut self, ui: &mut egui::Ui, ectx: &egui::Context, root: &Path) {
        widgets::section_title(ui, "Checks");
        let checking = self.checking;
        let planning = self.planning;
        let applying = self.applying;
        let mut run = false;
        let mut plan = false;
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!checking, widgets::primary_button("Run checks"))
                    .clicked()
                {
                    run = true;
                }
                if ui
                    .add_enabled(
                        !checking && !planning && !applying,
                        egui::Button::new("Plan repairs"),
                    )
                    .on_hover_text("Lists the safe fixes for the problems the checks report. Nothing is written until you apply them.")
                    .clicked()
                {
                    plan = true;
                }
                if checking {
                    ui.label(RichText::new("Checking...").color(MUTED));
                }
                if planning {
                    ui.label(RichText::new("Planning...").color(MUTED));
                }
            });
            ui.add_space(8.0);

            match &self.issues {
                None => {
                    ui.label(
                        RichText::new("Not run yet. Checks read the car and track folders and change nothing.")
                            .color(MUTED),
                    );
                }
                Some(issues) => {
                    let (e, w, i) = summary(issues);
                    ui.horizontal(|ui| {
                        widgets::pill(ui, &format!("{e} errors"), DANGER);
                        widgets::pill(ui, &format!("{w} warnings"), WARN);
                        widgets::pill(ui, &format!("{i} info"), INFO);
                    });
                    ui.add_space(8.0);
                    if issues.is_empty() {
                        ui.label(RichText::new("No problems found.").color(OK));
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("issues")
                            .max_height(ISSUES_HEIGHT)
                            .auto_shrink([false, false])
                            .show_rows(ui, ISSUE_ROW_HEIGHT, issues.len(), |ui, range| {
                                for issue in &issues[range] {
                                    issue_row(ui, issue, Some(root));
                                }
                            });
                    }
                }
            }
        });

        if run {
            self.run_checks(ectx, root);
        }
        if plan {
            self.start_plan(ectx, root);
        }
        self.repair_card(ui, ectx, root);
    }

    fn run_checks(&mut self, ectx: &egui::Context, root: &Path) {
        self.checking = true;
        let root = root.to_path_buf();
        spawn(&self.tx, ectx, move || Msg::Checked(check_all(&root)));
    }

    /// Builds the repair plan on a background thread. Nothing is written by planning.
    fn start_plan(&mut self, ectx: &egui::Context, root: &Path) {
        self.planning = true;
        let root = root.to_path_buf();
        spawn(&self.tx, ectx, move || {
            let repairs = plan_extended_repairs(&root);
            Msg::Planned { root, repairs }
        });
    }

    /// Runs the chosen repairs on a background thread.
    fn start_apply(&mut self, ectx: &egui::Context, root: PathBuf, repairs: Vec<Repair>) {
        self.applying = true;
        spawn(&self.tx, ectx, move || Msg::Repaired(apply_repairs(&root, &repairs)));
    }

    /// The plan with a checkbox per repair, and the "Apply selected" button.
    fn repair_card(&mut self, ui: &mut egui::Ui, ectx: &egui::Context, root: &Path) {
        let applying = self.applying;
        let Some(plan) = self.plan.as_mut().filter(|p| p.root == root) else {
            return;
        };

        widgets::section_title(ui, "Repair plan");
        let mut apply: Option<(PathBuf, Vec<Repair>)> = None;
        let mut close = false;
        theme::card().show(ui, |ui| {
            if plan.items.is_empty() {
                ui.label(RichText::new("No safe repairs found. Nothing to apply.").color(MUTED));
            } else {
                ui.label(
                    RichText::new("Each repair only creates a missing ui file. Existing files are never changed.")
                        .color(MUTED),
                );
                ui.add_space(8.0);
                for item in &mut plan.items {
                    ui.checkbox(&mut item.selected, item.repair.description.as_str())
                        .on_hover_text(item.repair.path.display().to_string());
                }
                ui.add_space(8.0);
            }

            let chosen: Vec<Repair> = plan
                .items
                .iter()
                .filter(|item| item.selected)
                .map(|item| item.repair.clone())
                .collect();
            ui.horizontal(|ui| {
                let can_apply = !applying && !chosen.is_empty();
                if ui
                    .add_enabled(can_apply, widgets::primary_button("Apply selected"))
                    .clicked()
                {
                    apply = Some((plan.root.clone(), chosen));
                }
                if ui.add_enabled(!applying, egui::Button::new("Close")).clicked() {
                    close = true;
                }
                if applying {
                    ui.label(RichText::new("Applying...").color(MUTED));
                }
            });
        });

        if let Some((root, repairs)) = apply {
            self.start_apply(ectx, root, repairs);
        }
        if close {
            self.plan = None;
        }
    }
}

/// Summary of a repair run, shown as a notice.
fn repair_notice(report: &RepairReport) -> Notice {
    let mut text = format!(
        "Repairs finished: {} created, {} skipped, {} failed.",
        report.created.len(),
        report.skipped.len(),
        report.failed.len()
    );
    if let Some((path, reason)) = report.failed.first() {
        text = format!("{text} {}: {reason}", path.display());
    }
    text = format!("{text} Run checks again to see the result.");
    Notice {
        text,
        ok: report.failed.is_empty(),
    }
}

/// Runs `work` on a background thread and sends its result to the page.
fn spawn(tx: &Sender<Msg>, ectx: &egui::Context, work: impl FnOnce() -> Msg + Send + 'static) {
    let tx = tx.clone();
    let ectx = ectx.clone();
    thread::spawn(move || {
        let _ = tx.send(work());
        ectx.request_repaint();
    });
}

fn load(mode: Mode, dir: &Path) -> Result<Form, String> {
    match mode {
        Mode::Cars => CarMeta::read(dir)
            .map(|m| {
                let spec = |k: &str| {
                    m.specs
                        .as_ref()
                        .and_then(|s| s.get(k))
                        .cloned()
                        .unwrap_or_default()
                };
                Form {
                    name: m.name.unwrap_or_default(),
                    brand: m.brand.unwrap_or_default(),
                    class: m.class.unwrap_or_default(),
                    year: m.year.map(|y| y.to_string()).unwrap_or_default(),
                    tags: m.tags.unwrap_or_default().join(", "),
                    description: m.description.unwrap_or_default(),
                    spec_bhp: spec("bhp"),
                    spec_torque: spec("torque"),
                    spec_weight: spec("weight"),
                    spec_topspeed: spec("topspeed"),
                    spec_accel: spec("acceleration"),
                    spec_pwratio: spec("pwratio"),
                    ..Form::default()
                }
            })
            .map_err(|e| format!("Could not read ui/ui_car.json: {e}")),
        Mode::Tracks => {
            let layouts = acxs_manager::content::track_layouts(dir);
            TrackMeta::read(dir)
                .map(|m| Form {
                    name: m.name.unwrap_or_default(),
                    city: m.city.unwrap_or_default(),
                    country: m.country.unwrap_or_default(),
                    author: m.author.unwrap_or_default(),
                    version: m.version.unwrap_or_default(),
                    length_m: m.length_m.map(|v| format!("{v:.0}")).unwrap_or_default(),
                    pitboxes: m.pitboxes.map(|v| v.to_string()).unwrap_or_default(),
                    tags: m.tags.unwrap_or_default().join(", "),
                    description: m.description.unwrap_or_default(),
                    layout: None,
                    layouts,
                    ..Form::default()
                })
                .map_err(|e| format!("Could not read ui/ui_track.json: {e}"))
        }
    }
}

fn join_parts(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String) {
    ui.label(RichText::new(label).small().color(MUTED));
    let width = ui.available_width();
    ui.add(egui::TextEdit::singleline(text).desired_width(width));
    ui.add_space(6.0);
}

fn text_area(ui: &mut egui::Ui, label: &str, text: &mut String) {
    ui.label(RichText::new(label).small().color(MUTED));
    let width = ui.available_width();
    ui.add(egui::TextEdit::multiline(text).desired_rows(4).desired_width(width));
    ui.add_space(6.0);
}

fn severity_style(severity: Severity) -> (&'static str, Color32) {
    match severity {
        Severity::Error => ("error", DANGER),
        Severity::Warning => ("warning", WARN),
        Severity::Info => ("info", MUTED),
    }
}

fn issue_row(ui: &mut egui::Ui, issue: &Issue, root: Option<&Path>) {
    let (label, color) = severity_style(issue.severity);
    let shown = root
        .and_then(|r| issue.path.strip_prefix(r).ok())
        .unwrap_or(issue.path.as_path());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).small().strong().color(color));
            ui.add(
                egui::Label::new(RichText::new(shown.display().to_string()).small().color(MUTED))
                    .truncate(),
            );
        });
        ui.add(egui::Label::new(RichText::new(issue.message.as_str())).truncate())
            .on_hover_text(issue.message.as_str());
    });
}
