//! Exports tool page: pick cars or tracks, narrow them with a search query and
//! an optional collection, then save the list as CSV or JSON.
//!
//! Scanning the install and reading the collections file run on a background
//! thread, and so does writing the chosen file. Results come back through a
//! channel that the UI drains with `try_recv` each frame, and the worker wakes
//! the UI with `request_repaint`.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::collection::{self, Collection, CollectionStore, ItemKind};
use acxs_manager::content::{self, CarEntry, TrackEntry};
use acxs_manager::content_export;
use acxs_manager::query::{self, Query, Record};
use eframe::egui::{self, RichText};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(ExportsPage {
        root: None,
        scanning: false,
        writing: false,
        cars: Vec::new(),
        tracks: Vec::new(),
        collections: Vec::new(),
        collections_error: None,
        kind: ItemKind::Car,
        search: String::new(),
        collection: None,
        format: Format::Csv,
        tx,
        rx,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Csv,
    Json,
}

impl Format {
    fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// Result of a background job.
enum Msg {
    /// Install scan and collections file for the AC folder `root`.
    Scanned {
        root: PathBuf,
        cars: Vec<CarEntry>,
        tracks: Vec<TrackEntry>,
        collections: Result<Vec<Collection>, String>,
    },
    /// A finished file write, reported as a notice.
    Written(Notice),
}

struct ExportsPage {
    /// The AC folder the lists were scanned for. `None` while a rescan is queued.
    root: Option<PathBuf>,
    scanning: bool,
    writing: bool,
    cars: Vec<CarEntry>,
    tracks: Vec<TrackEntry>,
    collections: Vec<Collection>,
    collections_error: Option<String>,
    kind: ItemKind,
    search: String,
    /// Name of the collection to restrict to. `None` means no restriction.
    collection: Option<String>,
    format: Format,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl ExportsPage {
    /// Scan the install and read the collections file on a background thread.
    fn start_scan(&mut self, egui_ctx: &egui::Context, root: PathBuf) {
        self.scanning = true;
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        std::thread::spawn(move || {
            let cars = content::scan_cars(&root);
            let tracks = content::scan_tracks(&root);
            let collections: Result<Vec<Collection>, String> =
                CollectionStore::load(&collection::default_path(&root))
                    .map(|store| store.list().into_iter().cloned().collect())
                    .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Scanned { root, cars, tracks, collections });
            ctx.request_repaint();
        });
    }

    /// Apply finished jobs. Scan results for an older folder are dropped.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Scanned { root, cars, tracks, collections } => {
                    if self.root.as_ref() != Some(&root) {
                        continue;
                    }
                    self.scanning = false;
                    self.cars = cars;
                    self.tracks = tracks;
                    match collections {
                        Ok(list) => {
                            self.collections = list;
                            self.collections_error = None;
                        }
                        Err(error) => {
                            self.collections.clear();
                            self.collections_error = Some(error);
                        }
                    }
                    let still_exists = |name: &String| self.collections.iter().any(|c| &c.name == name);
                    if self.collection.as_ref().is_some_and(|name| !still_exists(name)) {
                        self.collection = None;
                    }
                }
                Msg::Written(notice) => {
                    self.writing = false;
                    notices.push(notice);
                }
            }
        }
    }

    /// Ids of the selected collection for the chosen kind. `None` when no collection is selected.
    fn member_ids(&self) -> Option<Vec<&str>> {
        let name = self.collection.as_deref()?;
        let collection = self.collections.iter().find(|c| c.name == name)?;
        Some(
            collection
                .items
                .iter()
                .filter(|(kind, _)| *kind == self.kind)
                .map(|(_, id)| id.as_str())
                .collect(),
        )
    }

    /// Number of items that pass the collection and search filters, and the total of that kind.
    fn counts(&self, query: &Query, members: Option<&[&str]>) -> (usize, usize) {
        match self.kind {
            ItemKind::Car => (matching_cars(&self.cars, members, query).len(), self.cars.len()),
            ItemKind::Track => (matching_tracks(&self.tracks, members, query).len(), self.tracks.len()),
        }
    }

    /// Ask for a file, then write the filtered list on a background thread.
    fn export(&mut self, egui_ctx: &egui::Context, query: &Query) {
        let format = self.format;
        let noun = match self.kind {
            ItemKind::Car => "cars",
            ItemKind::Track => "tracks",
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("acxs-{noun}.{}", format.extension()))
            .add_filter(format.label(), &[format.extension()])
            .save_file()
        else {
            return;
        };
        let path = if path.extension().is_none() {
            path.with_extension(format.extension())
        } else {
            path
        };

        let members = self.member_ids();
        let (content, count) = match self.kind {
            ItemKind::Car => {
                let cars: Vec<CarEntry> = matching_cars(&self.cars, members.as_deref(), query)
                    .into_iter()
                    .cloned()
                    .collect();
                let text = match format {
                    Format::Csv => content_export::cars_csv(&cars),
                    Format::Json => content_export::cars_json(&cars),
                };
                (text, cars.len())
            }
            ItemKind::Track => {
                let tracks: Vec<TrackEntry> = matching_tracks(&self.tracks, members.as_deref(), query)
                    .into_iter()
                    .cloned()
                    .collect();
                let text = match format {
                    Format::Csv => content_export::tracks_csv(&tracks),
                    Format::Json => content_export::tracks_json(&tracks),
                };
                (text, tracks.len())
            }
        };
        drop(members);

        self.writing = true;
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        std::thread::spawn(move || {
            let notice = match content_export::write_export(&path, &content) {
                Ok(()) => Notice {
                    text: format!("Exported {count} {noun} to {}", path.display()),
                    ok: true,
                },
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Notice {
                    text: format!("{} already exists. Choose another file name.", path.display()),
                    ok: false,
                },
                Err(e) => Notice { text: format!("Export failed: {e}"), ok: false },
            };
            let _ = tx.send(Msg::Written(notice));
            ctx.request_repaint();
        });
    }
}

impl Tool for ExportsPage {
    fn name(&self) -> &'static str {
        "Exports"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);

        // Rescan when the AC folder changes. Reload clears `root` to force this.
        let root = ctx.ac_root.clone().filter(|_| ctx.ac_valid);
        if self.root != root {
            self.root = root.clone();
            if let Some(root) = root {
                self.start_scan(ui.ctx(), root);
            }
        }

        widgets::heading(ui, "Exports", "Save installed cars or tracks to a CSV or JSON file.");
        if !ctx.ac_valid {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to export content.");
            return;
        }

        let busy = self.scanning || self.writing;
        let mut reload_clicked = false;

        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.kind, ItemKind::Car, "Cars");
                ui.selectable_value(&mut self.kind, ItemKind::Track, "Tracks");
                ui.separator();
                ui.selectable_value(&mut self.format, Format::Csv, "CSV");
                ui.selectable_value(&mut self.format, Format::Json, "JSON");
            });
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search, e.g. class:GT3 brand:bmw  (empty for all)")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Collection");
                egui::ComboBox::from_id_salt("exports_collection")
                    .selected_text(self.collection.as_deref().unwrap_or("Any"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.collection, None, "Any");
                        for collection in &self.collections {
                            ui.selectable_value(
                                &mut self.collection,
                                Some(collection.name.clone()),
                                collection.name.as_str(),
                            );
                        }
                    });
            });
            if let Some(error) = &self.collections_error {
                ui.label(RichText::new(format!("Collections could not be read: {error}")).small().color(theme::WARN));
            }
        });

        ui.add_space(12.0);
        let parsed = query::parse(&self.search);
        let members = self.member_ids();
        let noun = match self.kind {
            ItemKind::Car => "cars",
            ItemKind::Track => "tracks",
        };
        let (matched, total) = match &parsed {
            Ok(query) => self.counts(query, members.as_deref()),
            Err(_) => (0, self.counts(&Query::All, None).1),
        };
        let mut export_clicked = false;

        theme::card().show(ui, |ui| {
            match &parsed {
                Ok(_) => {
                    ui.label(RichText::new(format!("{matched} of {total} {noun} match")).strong());
                }
                Err(error) => {
                    ui.label(RichText::new(format!("Search: {error}")).color(theme::DANGER));
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can_export = !busy && matched > 0 && parsed.is_ok();
                if ui.add_enabled(can_export, widgets::primary_button("Export…")).clicked() {
                    export_clicked = true;
                }
                if ui.add_enabled(!busy, egui::Button::new("Reload")).clicked() {
                    reload_clicked = true;
                }
                if self.scanning {
                    ui.spinner();
                    ui.label(RichText::new("Scanning…").color(theme::MUTED));
                } else if self.writing {
                    ui.spinner();
                    ui.label(RichText::new("Writing file…").color(theme::MUTED));
                }
            });
        });
        drop(members);

        if reload_clicked {
            self.root = None;
        }
        if export_clicked
            && let Ok(query) = &parsed
        {
            self.export(ui.ctx(), query);
        }
    }
}

/// Cars that pass the collection filter (when `members` is set) and the query, in
/// their scanned order.
fn matching_cars<'a>(cars: &'a [CarEntry], members: Option<&[&str]>, query: &Query) -> Vec<&'a CarEntry> {
    let base: Vec<&CarEntry> = match members {
        Some(ids) => content_export::filter_by_ids(cars, ids, |car| car.id.as_str()),
        None => cars.iter().collect(),
    };
    base.into_iter()
        .filter(|car| query::matches(query, &CarRecord(car)))
        .collect()
}

/// Tracks that pass the collection filter (when `members` is set) and the query, in
/// their scanned order.
fn matching_tracks<'a>(tracks: &'a [TrackEntry], members: Option<&[&str]>, query: &Query) -> Vec<&'a TrackEntry> {
    let base: Vec<&TrackEntry> = match members {
        Some(ids) => content_export::filter_by_ids(tracks, ids, |track| track.id.as_str()),
        None => tracks.iter().collect(),
    };
    base.into_iter()
        .filter(|track| query::matches(query, &TrackRecord(track)))
        .collect()
}

/// Lets the query language read car fields. The trait and the entry type both
/// come from the library, so the impl needs this local wrapper.
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

/// Lets the query language read track fields. See [`CarRecord`] for why this wrapper exists.
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
