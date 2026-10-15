//! Collections tool page: named groups of installed cars and tracks, saved in
//! `acxs-collections.json` in the Assetto Corsa folder. The file is read and
//! written, and the content is scanned, on background threads. Results are
//! polled from a channel each frame, so the window never waits on the disk.

use super::{Tool, ToolContext};
use crate::ui::theme::{ACCENT, DANGER, INFO, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::collection::{self, Collection, CollectionError, CollectionStore, ItemKind};
use acxs_manager::content::{self, CarEntry, TrackEntry};
use acxs_manager::query::{self, QueryError, Record};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    Box::new(CollectionsPage::new())
}

/// Row height of the picker results, without item spacing.
const PICKER_ROW_HEIGHT: f32 = 40.0;
/// The item list scrolls once it is taller than this.
const ITEMS_MAX_HEIGHT: f32 = 240.0;

/// A change to the store. It runs on a background thread, against the store as
/// read from disk, and returns the message for the notice bar.
type Op = Box<dyn FnOnce(&mut CollectionStore) -> Result<String, CollectionError> + Send>;

/// What to do with the selection after a change succeeds.
enum Followup {
    Keep,
    Select(String),
    Clear,
}

/// Results sent back from background threads.
enum Msg {
    Loaded {
        root: PathBuf,
        store: Result<CollectionStore, String>,
        cars: Vec<CarEntry>,
        tracks: Vec<TrackEntry>,
    },
    Changed {
        root: PathBuf,
        followup: Followup,
        result: Result<(CollectionStore, String), String>,
    },
}

/// A user action collected while drawing. Applied after the frame is drawn.
enum Action {
    Select(String),
    Reload,
    Create(String),
    Rename { from: String, to: String },
    Delete(String),
    Add { collection: String, items: Vec<(ItemKind, String)> },
    Remove { collection: String, kind: ItemKind, id: String, label: String },
}

/// Installed content, as scanned from the AC root.
#[derive(Default)]
struct Catalog {
    cars: Vec<CarEntry>,
    tracks: Vec<TrackEntry>,
    /// False until the first scan has finished. Missing markers wait for it.
    scanned: bool,
}

/// A picker result: an index into [`Catalog`].
#[derive(Debug, Clone, Copy)]
enum Hit {
    Car(usize),
    Track(usize),
}

type Hits = Result<Vec<Hit>, QueryError>;

#[derive(Default)]
struct Picker {
    query: String,
    /// The query the hits were computed for. Cleared when the catalog changes.
    cache: Option<(String, Hits)>,
}

struct CollectionsPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// True while a background load or change is running.
    busy: bool,
    /// The usable AC root this frame, or `None` when the folder is not set up.
    root: Option<PathBuf>,
    /// The root the store was last loaded for.
    loaded_root: Option<PathBuf>,
    store: Option<CollectionStore>,
    load_error: Option<String>,
    catalog: Catalog,
    new_name: String,
    rename_to: String,
    selected: Option<String>,
    picker: Picker,
}

impl CollectionsPage {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        CollectionsPage {
            tx,
            rx,
            busy: false,
            root: None,
            loaded_root: None,
            store: None,
            load_error: None,
            catalog: Catalog::default(),
            new_name: String::new(),
            rename_to: String::new(),
            selected: None,
            picker: Picker::default(),
        }
    }

    /// Runs `job` on a background thread and sends its result back.
    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Msg + Send + 'static) {
        self.busy = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }

    /// Reads the store and scans the content of `root`.
    fn start_load(&mut self, root: PathBuf, ctx: &egui::Context) {
        self.spawn(ctx, move || {
            let store = CollectionStore::load(&collection::default_path(&root))
                .map_err(|e| e.to_string());
            let cars = content::scan_cars(&root);
            let tracks = content::scan_tracks(&root);
            Msg::Loaded { root, store, cars, tracks }
        });
    }

    /// Applies `op` to the store on disk, in the background.
    fn change(&mut self, ctx: &egui::Context, followup: Followup, op: Op) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let path = collection::default_path(&root);
        self.spawn(ctx, move || {
            let result = run_change(&path, op);
            Msg::Changed { root, followup, result }
        });
    }

    /// Clears all state tied to the AC root when the root changes.
    fn sync_root(&mut self, ctx: &ToolContext) {
        let root = ctx.ac_root.as_deref().filter(|_| ctx.ac_valid);
        if self.root.as_deref() == root {
            return;
        }
        self.root = root.map(Path::to_path_buf);
        self.store = None;
        self.load_error = None;
        self.catalog = Catalog::default();
        self.selected = None;
        self.rename_to.clear();
        self.picker.cache = None;
        self.loaded_root = None;
    }

    /// Takes every finished background result. Never blocks.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            match msg {
                Msg::Loaded { root, store, cars, tracks } => {
                    if self.root.as_ref() != Some(&root) {
                        continue;
                    }
                    self.loaded_root = Some(root);
                    self.catalog = Catalog { cars, tracks, scanned: true };
                    self.picker.cache = None;
                    match store {
                        Ok(store) => {
                            self.store = Some(store);
                            self.load_error = None;
                        }
                        Err(error) => {
                            self.store = None;
                            self.load_error = Some(error);
                        }
                    }
                }
                Msg::Changed { root, followup, result } => {
                    if self.root.as_ref() != Some(&root) {
                        continue;
                    }
                    match result {
                        Ok((store, message)) => {
                            self.store = Some(store);
                            match followup {
                                Followup::Keep => {}
                                Followup::Select(name) => self.select(name),
                                Followup::Clear => self.selected = None,
                            }
                            notices.push(Notice { text: message, ok: true });
                        }
                        Err(text) => notices.push(Notice { text, ok: false }),
                    }
                }
            }
        }
    }

    fn select(&mut self, name: String) {
        self.rename_to = name.clone();
        self.selected = Some(name);
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Select(name) => self.select(name),
            Action::Reload => {
                if let Some(root) = self.root.clone()
                    && !self.busy
                {
                    self.start_load(root, ctx);
                }
            }
            Action::Create(name) => {
                let selected = name.clone();
                let op: Op = Box::new(move |store: &mut CollectionStore| {
                    store.create(&name)?;
                    Ok(format!("Created \"{name}\""))
                });
                self.change(ctx, Followup::Select(selected), op);
                self.new_name.clear();
            }
            Action::Rename { from, to } => {
                let selected = to.clone();
                let op: Op = Box::new(move |store: &mut CollectionStore| {
                    store.rename(&from, &to)?;
                    Ok(format!("Renamed to \"{to}\""))
                });
                self.change(ctx, Followup::Select(selected), op);
            }
            Action::Delete(name) => {
                let op: Op = Box::new(move |store: &mut CollectionStore| {
                    store.delete_collection(&name)?;
                    Ok(format!(
                        "Moved \"{name}\" to the deleted list. Its items are kept there."
                    ))
                });
                self.change(ctx, Followup::Clear, op);
            }
            Action::Add { collection, items } => {
                let op: Op = Box::new(move |store: &mut CollectionStore| {
                    let before = item_count(store, &collection);
                    for (kind, id) in &items {
                        store.add_item(&collection, *kind, id)?;
                    }
                    let added = item_count(store, &collection) - before;
                    let noun = if added == 1 { "item" } else { "items" };
                    Ok(format!("Added {added} {noun} to \"{collection}\""))
                });
                self.change(ctx, Followup::Keep, op);
            }
            Action::Remove { collection, kind, id, label } => {
                let op: Op = Box::new(move |store: &mut CollectionStore| {
                    store.remove_item(&collection, kind, &id)?;
                    Ok(format!("Removed {label} from \"{collection}\""))
                });
                self.change(ctx, Followup::Keep, op);
            }
        }
    }
}

impl Tool for CollectionsPage {
    fn name(&self) -> &'static str {
        "Collections"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        widgets::heading(
            ui,
            "Collections",
            "Named groups of installed cars and tracks, saved in acxs-collections.json.",
        );
        self.sync_root(ctx);
        self.poll(notices);

        if self.root.is_none() {
            widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to use collections.");
            return;
        }
        if !self.busy && self.loaded_root != self.root
            && let Some(root) = self.root.clone()
        {
            self.start_load(root, ui.ctx());
        }

        let busy = self.busy;
        let mut actions = Vec::new();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Reload")).clicked() {
                actions.push(Action::Reload);
            }
            if busy {
                ui.spinner();
                ui.label(RichText::new("Working...").color(MUTED));
            }
            if self.catalog.scanned {
                ui.label(
                    RichText::new(format!(
                        "{} cars and {} tracks found",
                        self.catalog.cars.len(),
                        self.catalog.tracks.len()
                    ))
                    .small()
                    .color(MUTED),
                );
            }
        });
        if let Some(error) = &self.load_error {
            ui.label(RichText::new(error.as_str()).color(DANGER));
        }

        if let Some(store) = self.store.as_ref() {
            let selected = self.selected.as_deref();
            egui::Panel::left("collections_list")
                .resizable(true)
                .default_size(280.0)
                .show(ui, |ui| {
                    list_panel(ui, store, selected, &mut self.new_name, busy, &mut actions);
                });
            detail_panel(
                ui,
                store,
                selected,
                &mut self.rename_to,
                &self.catalog,
                &mut self.picker,
                busy,
                &mut actions,
            );
        } else if self.load_error.is_none() {
            ui.label(RichText::new("Loading collections...").color(MUTED));
        }

        for action in actions {
            self.apply(action, ui.ctx());
        }
    }
}

/// Create box, deleted collections and the list of active collections.
fn list_panel(
    ui: &mut egui::Ui,
    store: &CollectionStore,
    selected: Option<&str>,
    new_name: &mut String,
    busy: bool,
    actions: &mut Vec<Action>,
) {
    widgets::section_title(ui, "Collections");
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 90.0).max(90.0);
        let edit = ui.add(
            egui::TextEdit::singleline(&mut *new_name)
                .hint_text("New collection")
                .desired_width(width),
        );
        let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let name = new_name.trim().to_string();
        let can_create = !busy && !name.is_empty();
        let clicked = ui
            .add_enabled(can_create, widgets::primary_button("Create"))
            .clicked();
        if can_create && (clicked || submitted) {
            actions.push(Action::Create(name));
        }
    });

    let deleted = store.deleted();
    if !deleted.is_empty() {
        ui.add_space(6.0);
        egui::CollapsingHeader::new(format!("Deleted ({})", deleted.len()))
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Deleted collections keep their items. To restore one, edit acxs-collections.json by hand.",
                    )
                    .small()
                    .color(MUTED),
                );
                for old in deleted {
                    ui.label(format!("{} ({})", old.name, count_text(old.items.len())));
                }
            });
    }

    ui.add_space(6.0);
    let collections = store.list();
    if collections.is_empty() {
        ui.label(RichText::new("No collections yet.").color(MUTED));
    }
    let mut picked = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for collection in collections {
                let is_selected = selected == Some(collection.name.as_str());
                let clicked = widgets::list_row(ui, is_selected, |ui| {
                    ui.label(RichText::new(collection.name.as_str()).strong());
                    ui.label(
                        RichText::new(count_text(collection.items.len()))
                            .small()
                            .color(MUTED),
                    );
                });
                if clicked {
                    picked = Some(collection.name.clone());
                }
            }
        });
    if let Some(name) = picked {
        actions.push(Action::Select(name));
    }
}

/// Header, rename, items of the selected collection, and the picker.
#[allow(clippy::too_many_arguments)]
fn detail_panel(
    ui: &mut egui::Ui,
    store: &CollectionStore,
    selected: Option<&str>,
    rename_to: &mut String,
    catalog: &Catalog,
    picker: &mut Picker,
    busy: bool,
    actions: &mut Vec<Action>,
) {
    let Some(name) = selected else {
        widgets::empty_state(ui, "Select a collection on the left, or create one.");
        return;
    };
    let Some(collection) = store.list().into_iter().find(|c| c.name == name) else {
        widgets::empty_state(ui, "This collection no longer exists.");
        return;
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(collection.name.as_str()).size(20.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!busy, egui::Button::new("Delete")).clicked() {
                actions.push(Action::Delete(collection.name.clone()));
            }
        });
    });
    ui.label(RichText::new(count_text(collection.items.len())).color(MUTED));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        let width = (ui.available_width() - 110.0).max(120.0);
        ui.add(egui::TextEdit::singleline(&mut *rename_to).desired_width(width));
        let new_name = rename_to.trim();
        let can_rename =
            !busy && !new_name.is_empty() && new_name != collection.name.as_str();
        if ui.add_enabled(can_rename, egui::Button::new("Rename")).clicked() {
            actions.push(Action::Rename {
                from: collection.name.clone(),
                to: new_name.to_string(),
            });
        }
    });

    ui.add_space(10.0);
    widgets::section_title(ui, "Items");
    if collection.items.is_empty() {
        ui.label(RichText::new("No items yet. Search below and add some.").color(MUTED));
    }
    egui::ScrollArea::vertical()
        .id_salt("collection_items")
        .max_height(ITEMS_MAX_HEIGHT)
        .show(ui, |ui| {
            for (kind, id) in &collection.items {
                let kind = *kind;
                let id = id.as_str();
                ui.horizontal(|ui| {
                    let (label, color) = kind_pill(kind);
                    widgets::pill(ui, label, color);
                    let name = catalog.name_of(kind, id);
                    ui.label(RichText::new(name.unwrap_or(id)).strong());
                    ui.label(RichText::new(id).small().color(MUTED));
                    if catalog.scanned && name.is_none() {
                        widgets::pill(ui, "missing", WARN);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                            actions.push(Action::Remove {
                                collection: collection.name.clone(),
                                kind,
                                id: id.to_string(),
                                label: name.unwrap_or(id).to_string(),
                            });
                        }
                    });
                });
            }
        });

    ui.add_space(10.0);
    widgets::section_title(ui, "Add cars and tracks");
    ui.add(
        egui::TextEdit::singleline(&mut picker.query)
            .hint_text("Search, e.g. country:italy & !(year<1990)")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    if !catalog.scanned {
        ui.label(RichText::new("Reading installed content...").color(MUTED));
        return;
    }

    if picker.cache.as_ref().is_none_or(|(text, _)| *text != picker.query) {
        picker.cache = Some((picker.query.clone(), compute_hits(&picker.query, catalog)));
    }
    let Some((_, hits)) = picker.cache.as_ref() else {
        return;
    };
    match hits {
        Err(error) => {
            ui.label(RichText::new(error.to_string()).color(WARN));
        }
        Ok(hits) => picker_results(ui, store, collection, catalog, hits, busy, actions),
    }
}

/// The "Add all" line and the rows of picker matches.
fn picker_results(
    ui: &mut egui::Ui,
    store: &CollectionStore,
    collection: &Collection,
    catalog: &Catalog,
    hits: &[Hit],
    busy: bool,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} found", hits.len())).color(MUTED));
        if ui
            .add_enabled(!busy && !hits.is_empty(), egui::Button::new("Add all"))
            .clicked()
        {
            let items: Vec<(ItemKind, String)> = hits
                .iter()
                .map(|hit| {
                    let (kind, id, _) = catalog.item_of(*hit);
                    (kind, id.to_string())
                })
                .collect();
            actions.push(Action::Add {
                collection: collection.name.clone(),
                items,
            });
        }
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .id_salt("picker_results")
        .auto_shrink([false, false])
        .show_rows(ui, PICKER_ROW_HEIGHT, hits.len(), |ui, range| {
            for hit in &hits[range] {
                let (kind, id, name) = catalog.item_of(*hit);
                let in_collection = collection
                    .items
                    .iter()
                    .any(|(k, i)| *k == kind && i.as_str() == id);
                let containing: Vec<&str> = store
                    .collections_containing(kind, id)
                    .into_iter()
                    .map(|c| c.name.as_str())
                    .collect();
                ui.horizontal(|ui| {
                    let (label, color) = kind_pill(kind);
                    widgets::pill(ui, label, color);
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(id).small().color(MUTED));
                    if !containing.is_empty() {
                        ui.label(
                            RichText::new(format!("in {}", containing.join(", ")))
                                .small()
                                .color(MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if in_collection {
                            ui.label(RichText::new("Added").color(OK));
                        } else if ui.add_enabled(!busy, egui::Button::new("Add")).clicked() {
                            actions.push(Action::Add {
                                collection: collection.name.clone(),
                                items: vec![(kind, id.to_string())],
                            });
                        }
                    });
                });
            }
        });
}

/// Runs a change on the store read from `path`, then writes it back.
/// A corrupt file is reported and left untouched by `load`.
fn run_change(path: &Path, op: Op) -> Result<(CollectionStore, String), String> {
    let mut store = CollectionStore::load(path).map_err(|e| e.to_string())?;
    let message = op(&mut store).map_err(|e| e.to_string())?;
    store.save(path).map_err(|e| e.to_string())?;
    Ok((store, message))
}

fn item_count(store: &CollectionStore, name: &str) -> usize {
    store
        .list()
        .into_iter()
        .find(|c| c.name == name)
        .map_or(0, |c| c.items.len())
}

fn count_text(n: usize) -> String {
    if n == 1 {
        "1 item".to_string()
    } else {
        format!("{n} items")
    }
}

fn kind_pill(kind: ItemKind) -> (&'static str, egui::Color32) {
    match kind {
        ItemKind::Car => ("car", INFO),
        ItemKind::Track => ("track", ACCENT),
    }
}

impl Catalog {
    /// Display name of an installed item, or `None` when it is not installed.
    fn name_of(&self, kind: ItemKind, id: &str) -> Option<&str> {
        match kind {
            ItemKind::Car => self.cars.iter().find(|c| c.id == id).map(|c| c.name.as_str()),
            ItemKind::Track => self.tracks.iter().find(|t| t.id == id).map(|t| t.name.as_str()),
        }
    }

    /// Kind, folder id and display name of a picker hit.
    fn item_of(&self, hit: Hit) -> (ItemKind, &str, &str) {
        match hit {
            Hit::Car(i) => {
                let car = &self.cars[i];
                (ItemKind::Car, car.id.as_str(), car.name.as_str())
            }
            Hit::Track(i) => {
                let track = &self.tracks[i];
                (ItemKind::Track, track.id.as_str(), track.name.as_str())
            }
        }
    }
}

/// Installed cars and tracks that match the query text.
fn compute_hits(text: &str, catalog: &Catalog) -> Hits {
    let parsed = query::parse(text)?;
    let mut hits = Vec::new();
    for (index, car) in catalog.cars.iter().enumerate() {
        if query::matches(&parsed, &CarRecord(car)) {
            hits.push(Hit::Car(index));
        }
    }
    for (index, track) in catalog.tracks.iter().enumerate() {
        if query::matches(&parsed, &TrackRecord(track)) {
            hits.push(Hit::Track(index));
        }
    }
    Ok(hits)
}

/// Local wrappers. The orphan rule does not allow this crate to implement
/// `acxs_manager::query::Record` directly for the library's own types.
struct CarRecord<'a>(&'a CarEntry);

impl Record for CarRecord<'_> {
    fn text_fields(&self) -> Vec<String> {
        let car = self.0;
        let mut fields = vec![
            car.name.clone(),
            car.id.clone(),
            car.brand.clone(),
            car.car_class.clone(),
        ];
        fields.extend(car.tags.iter().cloned());
        fields
    }

    fn field(&self, name: &str) -> Option<String> {
        let car = self.0;
        match name {
            "name" => Some(car.name.clone()),
            "id" => Some(car.id.clone()),
            "brand" => Some(car.brand.clone()),
            "class" => Some(car.car_class.clone()),
            "year" => Some(car.year.clone()),
            "tag" | "tags" => Some(car.tags.join(" ")),
            _ => None,
        }
    }
}

struct TrackRecord<'a>(&'a TrackEntry);

impl Record for TrackRecord<'_> {
    fn text_fields(&self) -> Vec<String> {
        let track = self.0;
        let mut fields = vec![
            track.name.clone(),
            track.id.clone(),
            track.country.clone(),
            track.city.clone(),
        ];
        fields.extend(track.layouts.iter().cloned());
        fields
    }

    fn field(&self, name: &str) -> Option<String> {
        let track = self.0;
        match name {
            "name" => Some(track.name.clone()),
            "id" => Some(track.id.clone()),
            "country" => Some(track.country.clone()),
            "city" => Some(track.city.clone()),
            "length" => track.length_m.map(|m| m.to_string()),
            "pitboxes" => track.pitboxes.map(|n| n.to_string()),
            "layout" | "layouts" => Some(track.layouts.join(" ")),
            _ => None,
        }
    }
}
