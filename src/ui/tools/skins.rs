//! Skins tool page: pick an installed car, browse its skins, and import a skin folder.
//!
//! Car and skin scans, the source folder check and the import run on background
//! threads. They report back through a channel that is polled once per frame.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use super::{Tool, ToolContext};
use crate::ui::theme::{self, BORDER, CARD, CARD_SELECTED, DANGER, INFO, MUTED, OK, TEXT, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::content::{self, CarEntry};
use acxs_manager::skin_import;
use acxs_manager::skins::{self, SkinEntry};
use eframe::egui::{self, CornerRadius, Margin, RichText, Sense, Stroke};

/// Row height for the car list (two lines, matches the other list pages).
const CAR_ROW_HEIGHT: f32 = 60.0;
/// Inner width of a skin card.
const CARD_WIDTH: f32 = 210.0;

/// Results sent back from background threads.
enum Msg {
    Notice(Notice),
    /// Car list and skin counts for the AC root `root`.
    Cars { root: PathBuf, cars: Vec<CarEntry>, counts: BTreeMap<String, usize> },
    /// Skin counts only, after an import.
    Counts { root: PathBuf, counts: BTreeMap<String, usize> },
    Skins { car_id: String, skins: Vec<SkinEntry> },
    /// Result of `validate_source` for `source`.
    Checked { source: String, problems: Vec<String> },
    ImportFinished,
}

pub fn tool() -> Box<dyn Tool> {
    Box::new(SkinsPage::new())
}

struct SkinsPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The AC root the car list was requested for.
    root: Option<PathBuf>,
    cars: Vec<CarEntry>,
    counts: BTreeMap<String, usize>,
    cars_loading: bool,
    car_query: String,
    selected_car: Option<String>,
    /// The car whose skins are loaded, or loading.
    skins_car: Option<String>,
    skins: Vec<SkinEntry>,
    skins_loading: bool,
    skin_query: String,
    selected_skin: Option<String>,
    source_input: String,
    /// The last finished source check: the source it ran for and its problems.
    checked: Option<(String, Vec<String>)>,
    checking: bool,
    importing: bool,
}

impl SkinsPage {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            root: None,
            cars: Vec::new(),
            counts: BTreeMap::new(),
            cars_loading: false,
            car_query: String::new(),
            selected_car: None,
            skins_car: None,
            skins: Vec::new(),
            skins_loading: false,
            skin_query: String::new(),
            selected_skin: None,
            source_input: String::new(),
            checked: None,
            checking: false,
            importing: false,
        }
    }

    /// Apply results from background threads. Stale results are dropped.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Notice(notice) => notices.push(notice),
                Msg::Cars { root, cars, counts } => {
                    if self.root.as_ref() == Some(&root) {
                        self.cars = cars;
                        self.counts = counts;
                        self.cars_loading = false;
                    }
                }
                Msg::Counts { root, counts } => {
                    if self.root.as_ref() == Some(&root) {
                        self.counts = counts;
                    }
                }
                Msg::Skins { car_id, skins } => {
                    if self.skins_car.as_deref() == Some(car_id.as_str()) {
                        self.skins = skins;
                        self.skins_loading = false;
                    }
                }
                Msg::Checked { source, problems } => {
                    self.checking = false;
                    self.checked = Some((source, problems));
                }
                Msg::ImportFinished => self.importing = false,
            }
        }
    }

    fn spawn_cars(&mut self, egui_ctx: &egui::Context, root: PathBuf) {
        self.root = Some(root.clone());
        self.cars.clear();
        self.counts.clear();
        self.cars_loading = true;
        self.selected_car = None;
        self.clear_skins();
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            let cars = content::scan_cars(&root);
            let counts = skins::scan_all_skin_counts(&root);
            let _ = tx.send(Msg::Cars { root, cars, counts });
            ctx.request_repaint();
        });
    }

    fn clear_skins(&mut self) {
        self.skins_car = None;
        self.skins.clear();
        self.skins_loading = false;
        self.selected_skin = None;
    }

    fn spawn_skins(&mut self, egui_ctx: &egui::Context, root: PathBuf, car_id: String) {
        self.clear_skins();
        self.skins_car = Some(car_id.clone());
        self.skins_loading = true;
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            let found = skins::scan_skins(&root, &car_id);
            let _ = tx.send(Msg::Skins { car_id, skins: found });
            ctx.request_repaint();
        });
    }

    fn spawn_check(&mut self, egui_ctx: &egui::Context, source: String) {
        self.checking = true;
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            let problems = skin_import::validate_source(Path::new(&source));
            let _ = tx.send(Msg::Checked { source, problems });
            ctx.request_repaint();
        });
    }

    fn start_import(
        &mut self,
        egui_ctx: &egui::Context,
        root: PathBuf,
        car_id: String,
        source: PathBuf,
        game_running: bool,
    ) {
        self.importing = true;
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            let notice = match skin_import::import_skin(&root, &car_id, &source, game_running) {
                Ok(target) => {
                    let folder = target
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    Notice { text: format!("Imported skin {folder} into {car_id}."), ok: true }
                }
                Err(err) => Notice { text: format!("Import failed: {err}"), ok: false },
            };
            let _ = tx.send(Msg::Notice(notice));
            let found = skins::scan_skins(&root, &car_id);
            let counts = skins::scan_all_skin_counts(&root);
            let _ = tx.send(Msg::Skins { car_id, skins: found });
            let _ = tx.send(Msg::Counts { root, counts });
            let _ = tx.send(Msg::ImportFinished);
            ctx.request_repaint();
        });
    }

    fn car_list(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::TextEdit::singleline(&mut self.car_query)
                .hint_text("Search cars")
                .desired_width(f32::INFINITY),
        );
        let filtered = content::filter_cars(&self.cars, &self.car_query);
        ui.label(
            RichText::new(format!("{} of {} cars", filtered.len(), self.cars.len()))
                .small()
                .color(MUTED),
        );
        ui.add_space(6.0);
        if self.cars_loading && self.cars.is_empty() {
            widgets::empty_state(ui, "Scanning cars...");
            return;
        }
        if filtered.is_empty() {
            let text = if self.cars.is_empty() {
                "No cars found in content/cars."
            } else {
                "No cars match your search."
            };
            widgets::empty_state(ui, text);
            return;
        }

        let selected_id = self.selected_car.clone();
        let counts = &self.counts;
        let mut clicked: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            ui,
            CAR_ROW_HEIGHT,
            filtered.len(),
            |ui, range| {
                for car in &filtered[range] {
                    let selected = selected_id.as_deref() == Some(car.id.as_str());
                    let count = counts.get(&car.id).copied().unwrap_or(0);
                    let picked = widgets::list_row(ui, selected, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(car_title(car)).strong());
                            widgets::pill(
                                ui,
                                &format!("{count} skins"),
                                if count > 0 { INFO } else { MUTED },
                            );
                        });
                        ui.label(RichText::new(car_subtitle(car)).small().color(MUTED));
                    });
                    if picked {
                        clicked = Some(car.id.clone());
                    }
                }
            },
        );
        if let Some(id) = clicked {
            self.selected_car = Some(id);
        }
    }

    fn detail(&self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Details");
        let Some(skin) = self
            .selected_skin
            .as_deref()
            .and_then(|id| self.skins.iter().find(|s| s.skin_id == id))
        else {
            ui.label(RichText::new("Select a skin to see its details.").color(MUTED));
            return;
        };

        ui.label(RichText::new(skin.name.as_str()).size(18.0).strong());
        ui.add_space(6.0);
        widgets::key_value(ui, "Folder", &skin.skin_id);
        widgets::key_value(ui, "Driver", or_dash(&skin.driver));
        widgets::key_value(ui, "Team", or_dash(&skin.team));
        widgets::key_value(ui, "Number", or_dash(&skin.number));
        widgets::key_value(ui, "Textures", &skin.texture_count.to_string());
        widgets::key_value(ui, "Path", &skin.path.display().to_string());

        ui.add_space(12.0);
        widgets::section_title(ui, "Preview");
        match skins::preview_for(&skin.path) {
            Some(path) => {
                ui.label(
                    RichText::new("Image previews are not available in this build. File:")
                        .color(MUTED),
                );
                ui.label(RichText::new(path.display().to_string()).color(TEXT));
            }
            None => {
                ui.label(RichText::new("No preview image.").color(MUTED));
            }
        }
    }

    fn import_bar(&mut self, ui: &mut egui::Ui, root: &Path, game_running: bool, egui_ctx: &egui::Context) {
        egui::CollapsingHeader::new("Import a skin folder")
            .default_open(false)
            .show(ui, |ui| {
                let hint = match &self.selected_car {
                    Some(car) => format!(
                        "Imports into {car}. The folder needs .dds, .png or .jpg textures at its top level."
                    ),
                    None => "Select a car to import into it.".to_string(),
                };
                ui.label(RichText::new(hint).color(MUTED));
                if game_running {
                    ui.label(RichText::new("Close Assetto Corsa before importing a skin.").color(WARN));
                }

                let source = self.source_input.trim().to_string();
                ui.horizontal(|ui| {
                    let field_width = (ui.available_width() - 200.0).max(160.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.source_input)
                            .desired_width(field_width)
                            .hint_text("Skin folder"),
                    );
                    if ui.button("Browse").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.source_input = path.display().to_string();
                        }
                    }
                    let ready = self
                        .checked
                        .as_ref()
                        .is_some_and(|(s, problems)| *s == source && problems.is_empty());
                    let can_import =
                        ready && !self.importing && !game_running && self.selected_car.is_some();
                    let label = if self.importing { "Importing..." } else { "Import" };
                    if ui.add_enabled(can_import, widgets::primary_button(label)).clicked() {
                        if let Some(car) = self.selected_car.clone() {
                            self.start_import(
                                egui_ctx,
                                root.to_path_buf(),
                                car,
                                PathBuf::from(&source),
                                game_running,
                            );
                        }
                    }
                });

                match &self.checked {
                    Some((s, problems)) if !source.is_empty() && *s == source => {
                        if problems.is_empty() {
                            ui.label(RichText::new("Folder is ready to import.").color(OK));
                        } else {
                            for problem in problems {
                                ui.label(RichText::new(problem).color(DANGER));
                            }
                        }
                    }
                    _ if !source.is_empty() => {
                        ui.label(RichText::new("Checking folder...").color(MUTED));
                    }
                    _ => {}
                }
            });
    }

    fn skin_grid(&mut self, ui: &mut egui::Ui) {
        let Some(car_id) = self.selected_car.clone() else {
            widgets::empty_state(ui, "Pick a car on the left to see its skins.");
            return;
        };
        let title = self
            .cars
            .iter()
            .find(|c| c.id == car_id)
            .map(car_title)
            .unwrap_or_else(|| car_id.clone());

        ui.add_space(8.0);
        ui.label(RichText::new(format!("Skins of {title}")).size(16.0).strong());
        ui.add(
            egui::TextEdit::singleline(&mut self.skin_query)
                .hint_text("Search name, driver, team or number")
                .desired_width(f32::INFINITY),
        );
        if self.skins_loading {
            widgets::empty_state(ui, "Scanning skins...");
            return;
        }
        let filtered = skins::filter_skins(&self.skins, &self.skin_query);
        ui.label(
            RichText::new(format!("{} of {} skins", filtered.len(), self.skins.len()))
                .small()
                .color(MUTED),
        );
        ui.add_space(6.0);
        if filtered.is_empty() {
            let text = if self.skins.is_empty() {
                "This car has no skins."
            } else {
                "No skins match your search."
            };
            widgets::empty_state(ui, text);
            return;
        }

        let selected_id = self.selected_skin.clone();
        let mut clicked: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for skin in &filtered {
                    let selected = selected_id.as_deref() == Some(skin.skin_id.as_str());
                    if skin_card(ui, skin, selected) {
                        clicked = Some(skin.skin_id.clone());
                    }
                }
            });
        });
        if let Some(id) = clicked {
            self.selected_skin = Some(id);
        }
    }
}

impl Tool for SkinsPage {
    fn name(&self) -> &'static str {
        "Skins"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        widgets::heading(ui, "Skins", "Browse the skins of installed cars and import new ones.");

        let root = match (ctx.ac_valid, &ctx.ac_root) {
            (true, Some(root)) => root.clone(),
            _ => {
                widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to see skins.");
                return;
            }
        };
        let egui_ctx = ui.ctx().clone();

        if self.root.as_ref() != Some(&root) {
            self.spawn_cars(&egui_ctx, root.clone());
        }
        if let Some(car) = self.selected_car.clone() {
            if self.skins_car.as_deref() != Some(car.as_str()) {
                self.spawn_skins(&egui_ctx, root.clone(), car);
            }
        }
        let source = self.source_input.trim().to_string();
        if !source.is_empty()
            && !self.checking
            && self.checked.as_ref().map(|(s, _)| s.as_str()) != Some(source.as_str())
        {
            self.spawn_check(&egui_ctx, source);
        }

        egui::Panel::left("skins_cars")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui| self.car_list(ui));
        egui::Panel::right("skins_detail")
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| self.detail(ui));

        self.import_bar(ui, &root, ctx.game_running, &egui_ctx);
        self.skin_grid(ui);
    }
}

/// One skin as a clickable card. Returns true when clicked.
fn skin_card(ui: &mut egui::Ui, skin: &SkinEntry, selected: bool) -> bool {
    let response = egui::Frame::new()
        .fill(if selected { CARD_SELECTED } else { CARD })
        .stroke(Stroke::new(
            1.0,
            if selected { theme::ACCENT.gamma_multiply(0.7) } else { BORDER },
        ))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(CARD_WIDTH);
            ui.label(RichText::new(skin.name.as_str()).strong());
            ui.label(RichText::new(skin.skin_id.as_str()).small().color(MUTED));
            ui.add_space(4.0);
            widgets::key_value(ui, "Driver", or_dash(&skin.driver));
            widgets::key_value(ui, "Team", or_dash(&skin.team));
            widgets::key_value(ui, "Number", or_dash(&skin.number));
        })
        .response
        .interact(Sense::click());
    response.clicked()
}

fn car_title(car: &CarEntry) -> String {
    if car.name.is_empty() {
        car.id.clone()
    } else {
        car.name.clone()
    }
}

fn car_subtitle(car: &CarEntry) -> String {
    [car.brand.as_str(), car.id.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn or_dash(text: &str) -> &str {
    if text.trim().is_empty() {
        "-"
    } else {
        text
    }
}
