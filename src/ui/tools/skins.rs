//! Skins tool page: pick an installed car, browse its skins, import a skin folder,
//! and recolour a texture of a skin into a new file.
//!
//! Car and skin scans, the source folder check, the PNG listing and the import run on
//! background threads. They report back through a channel that is polled once per frame.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use super::{Tool, ToolContext};
use crate::ui::theme::{self, BORDER, CARD, CARD_SELECTED, DANGER, INFO, MUTED, OK, TEXT, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::content::{self, CarEntry};
use acxs_manager::livery;
use acxs_manager::skin_import;
use acxs_manager::skins::{self, SkinEntry};
use eframe::egui::{self, CornerRadius, Margin, RichText, Sense, Stroke};

/// Row height for the car list (two lines, matches the other list pages).
const CAR_ROW_HEIGHT: f32 = 60.0;
/// Inner width of a skin card.
const CARD_WIDTH: f32 = 210.0;
/// Target colour the recolour section starts with (a blue).
const RECOLOUR_DEFAULT_TARGET: [u8; 3] = [0x2F, 0x80, 0xED];

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
    /// PNG file names directly inside the skin folder `dir`.
    PngFiles { dir: PathBuf, files: Vec<String> },
    /// A recolour save has finished, successfully or not.
    RecolourFinished,
}

/// State of the Recolour section for the selected skin.
struct RecolourForm {
    /// The skin folder that `files` belongs to.
    dir: Option<PathBuf>,
    /// PNG file names in `dir`, or None while they are being scanned.
    files: Option<Vec<String>>,
    /// The chosen source file name. Falls back to the first file when unset or gone.
    source: Option<String>,
    target: [u8; 3],
    /// Recolour strength, 0 to 1.
    strength: f32,
    /// Optional greyscale mask. Its brightness scales the strength per pixel.
    mask: Option<PathBuf>,
    saving: bool,
}

impl Default for RecolourForm {
    fn default() -> Self {
        Self {
            dir: None,
            files: None,
            source: None,
            target: RECOLOUR_DEFAULT_TARGET,
            strength: 1.0,
            mask: None,
            saving: false,
        }
    }
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
    recolour: RecolourForm,
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
            recolour: RecolourForm::default(),
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
                Msg::PngFiles { dir, files } => {
                    if self.recolour.dir.as_deref() == Some(dir.as_path()) {
                        self.recolour.files = Some(files);
                    }
                }
                Msg::RecolourFinished => self.recolour.saving = false,
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

    /// Start a scan of the PNG files in `dir`, unless `dir` is already the listed folder.
    fn ensure_png_list(&mut self, egui_ctx: &egui::Context, dir: &Path) {
        if self.recolour.dir.as_deref() == Some(dir) {
            return;
        }
        self.recolour.dir = Some(dir.to_path_buf());
        self.recolour.files = None;
        self.recolour.source = None;
        let dir = dir.to_path_buf();
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            let files = list_pngs(&dir);
            let _ = tx.send(Msg::PngFiles { dir, files });
            ctx.request_repaint();
        });
    }

    /// Recolour `source` with the current settings and save it as a new file in `dir`.
    fn start_recolour(&mut self, egui_ctx: &egui::Context, dir: PathBuf, source: PathBuf) {
        self.recolour.saving = true;
        let target = self.recolour.target;
        let strength = self.recolour.strength;
        let mask = self.recolour.mask.clone();
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        thread::spawn(move || {
            let notice = match recolour_to_new_file(&dir, &source, target, strength, mask.as_deref()) {
                Ok(path) => Notice {
                    text: format!("Saved recoloured texture as {}", path.display()),
                    ok: true,
                },
                Err(err) => Notice { text: format!("Recolour failed: {err}"), ok: false },
            };
            let _ = tx.send(Msg::Notice(notice));
            let files = list_pngs(&dir);
            let _ = tx.send(Msg::PngFiles { dir, files });
            let _ = tx.send(Msg::RecolourFinished);
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

    fn detail(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Details");
        let Some(skin) = self
            .selected_skin
            .as_deref()
            .and_then(|id| self.skins.iter().find(|s| s.skin_id == id))
            .cloned()
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

        self.recolour_section(ui, &skin);
    }

    /// Recolour one PNG of the skin into a new file in the skin folder.
    fn recolour_section(&mut self, ui: &mut egui::Ui, skin: &SkinEntry) {
        ui.add_space(12.0);
        widgets::section_title(ui, "Recolour");
        let egui_ctx = ui.ctx().clone();
        self.ensure_png_list(&egui_ctx, &skin.path);

        let Some(files) = self.recolour.files.clone() else {
            ui.label(RichText::new("Scanning PNG files...").color(MUTED));
            return;
        };
        if files.is_empty() {
            ui.label(RichText::new("This skin folder has no PNG files.").color(MUTED));
            return;
        }
        let source = match &self.recolour.source {
            Some(name) if files.contains(name) => name.clone(),
            _ => files[0].clone(),
        };

        ui.label(RichText::new("Source texture").color(MUTED));
        let mut picked = None;
        egui::ComboBox::from_id_salt("recolour_source")
            .selected_text(source.as_str())
            .show_ui(ui, |ui| {
                for name in &files {
                    if ui.selectable_label(*name == source, name.as_str()).clicked() {
                        picked = Some(name.clone());
                    }
                }
            });
        if let Some(name) = picked {
            self.recolour.source = Some(name);
        }

        ui.horizontal(|ui| {
            ui.label("Target colour");
            ui.color_edit_button_srgb(&mut self.recolour.target);
        });
        ui.add(egui::Slider::new(&mut self.recolour.strength, 0.0..=1.0).text("Strength"));

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Mask");
            let mask_name = self
                .recolour
                .mask
                .as_deref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "none".to_string());
            ui.label(RichText::new(mask_name).color(MUTED));
        });
        ui.horizontal(|ui| {
            if ui.button("Choose mask").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).pick_file() {
                    self.recolour.mask = Some(path);
                }
            }
            if self.recolour.mask.is_some() && ui.button("Clear mask").clicked() {
                self.recolour.mask = None;
            }
        });
        ui.label(
            RichText::new("Optional. White areas of the mask take the full strength, black areas none.")
                .small()
                .color(MUTED),
        );

        ui.add_space(6.0);
        let saving = self.recolour.saving;
        let label = if saving { "Saving..." } else { "Save as new file" };
        if ui.add_enabled(!saving, widgets::primary_button(label)).clicked() {
            self.start_recolour(&egui_ctx, skin.path.clone(), skin.path.join(&source));
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
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.detail(ui));
            });

        self.import_bar(ui, &root, ctx.game_running, &egui_ctx);
        self.skin_grid(ui);
    }
}

/// File names of the PNG files directly inside `dir`, sorted case-insensitively.
/// Symbolic links are skipped. A folder that cannot be read yields an empty list.
fn list_pngs(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.to_ascii_lowercase().ends_with(".png"))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

/// Recolour the PNG at `source` and write the result to a new file in `dir`.
///
/// The name is `<source stem>-recoloured.png`, with a counter added while that name
/// is taken. An existing file is never replaced. Errors are returned as text for the notice.
fn recolour_to_new_file(
    dir: &Path,
    source: &Path,
    target: [u8; 3],
    strength: f32,
    mask: Option<&Path>,
) -> Result<PathBuf, String> {
    let input = fs::read(source).map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    let mask_bytes = mask
        .map(|path| fs::read(path).map_err(|e| format!("cannot read mask {}: {e}", path.display())))
        .transpose()?;
    let output = livery::recolor_png(&input, target, strength, mask_bytes.as_deref())
        .map_err(|err| err.to_string())?;

    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "texture".to_string());
    let mut counter = 1u32;
    loop {
        let name = if counter == 1 {
            format!("{stem}-recoloured.png")
        } else {
            format!("{stem}-recoloured-{counter}.png")
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(&output) {
                    let _ = fs::remove_file(&path);
                    return Err(format!("cannot write {}: {e}", path.display()));
                }
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => counter += 1,
            Err(e) => return Err(format!("cannot create {}: {e}", path.display())),
        }
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
