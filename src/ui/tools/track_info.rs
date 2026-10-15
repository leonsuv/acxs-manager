//! Track Info tool page: installed tracks, their details and surfaces, and a
//! two-track comparison. Scans and file reads run on background threads.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::content::{filter_tracks, scan_tracks, TrackEntry};
use acxs_manager::thumbnails;
use acxs_manager::track_compare::{compare_tracks, surface_changes, TrackDiff};
use acxs_manager::track_surfaces::{read_surfaces, summarize, Surface};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Upper bound for compare rows and surface-change lines shown at once.
const MAX_LINES: usize = 200;
const BASE_LAYOUT: &str = "Track (base)";
/// Edge length in pixels of the square thumbnail made by the "Make thumbnail" button.
const THUMB_SIZE: u32 = 512;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(TrackInfo {
        tx,
        rx,
        tracks_root: None,
        tracks_loading: false,
        tracks: Vec::new(),
        query: String::new(),
        selected: None,
        layout: None,
        surfaces_key: None,
        surfaces_seq: 0,
        surfaces: Load::Idle,
        compare_with: None,
        compare_query: String::new(),
        compare_key: None,
        compare_seq: 0,
        compare: Load::Idle,
        thumb_busy: false,
    })
}

/// State of a background read: nothing requested, running, or finished.
#[derive(Default)]
enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

struct Comparison {
    rows: Vec<TrackDiff>,
    changes: Vec<String>,
}

enum Msg {
    Tracks { root: PathBuf, tracks: Vec<TrackEntry> },
    Surfaces { seq: u64, result: Result<Vec<Surface>, String> },
    Compare { seq: u64, result: Result<Comparison, String> },
    /// A thumbnail was made or found in the cache: its file path and size in bytes.
    Thumbnail { result: Result<(PathBuf, usize), String> },
}

/// What the surfaces read was requested for.
#[derive(Clone, PartialEq)]
struct SurfaceKey {
    track: String,
    layout: Option<String>,
}

struct TrackInfo {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    tracks_root: Option<PathBuf>,
    tracks_loading: bool,
    tracks: Vec<TrackEntry>,
    query: String,
    /// Track folder id of the selected track.
    selected: Option<String>,
    /// `None` is the track's own `data` folder, `Some` is a layout folder.
    layout: Option<String>,
    surfaces_key: Option<SurfaceKey>,
    surfaces_seq: u64,
    surfaces: Load<Vec<Surface>>,
    /// Track folder id of the comparison target.
    compare_with: Option<String>,
    compare_query: String,
    /// (left id, right id) the comparison was requested for.
    compare_key: Option<(String, String)>,
    compare_seq: u64,
    compare: Load<Comparison>,
    /// A thumbnail job is running.
    thumb_busy: bool,
}

fn find<'a>(tracks: &'a [TrackEntry], id: Option<&str>) -> Option<&'a TrackEntry> {
    let id = id?;
    tracks.iter().find(|t| t.id == id)
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() { "-" } else { text }
}

fn location(t: &TrackEntry) -> String {
    [t.city.as_str(), t.country.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn yes_no(on: bool) -> &'static str {
    if on { "yes" } else { "no" }
}

impl TrackInfo {
    /// Runs `work` on a background thread and sends its message back.
    fn spawn(&self, egui_ctx: &egui::Context, work: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.tx.clone();
        let repaint = egui_ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(work());
            repaint.request_repaint();
        });
    }

    /// Applies finished background work. Results for superseded requests are dropped.
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
                Msg::Tracks { root, tracks } => {
                    if self.tracks_root.as_ref() == Some(&root) {
                        self.tracks_loading = false;
                        self.tracks = tracks;
                        if find(&self.tracks, self.selected.as_deref()).is_none() {
                            self.selected = None;
                            self.layout = None;
                        }
                        if find(&self.tracks, self.compare_with.as_deref()).is_none() {
                            self.compare_with = None;
                        }
                    }
                }
                Msg::Surfaces { seq, result } if seq == self.surfaces_seq => {
                    self.surfaces = match result {
                        Ok(list) => Load::Ready(list),
                        Err(e) => Load::Failed(e),
                    };
                }
                Msg::Compare { seq, result } if seq == self.compare_seq => {
                    self.compare = match result {
                        Ok(c) => Load::Ready(c),
                        Err(e) => Load::Failed(e),
                    };
                }
                _ => {}
            }
        }
    }

    /// Starts whatever the current state needs but does not have yet:
    /// a track scan for the install root, a surfaces read for the selection,
    /// and a comparison when a second track is chosen.
    fn sync(&mut self, egui_ctx: &egui::Context, root: Option<&PathBuf>) {
        match root {
            Some(root) if self.tracks_root.as_ref() != Some(root) => {
                self.tracks_root = Some(root.clone());
                self.tracks_loading = true;
                let root = root.clone();
                self.spawn(egui_ctx, move || Msg::Tracks {
                    tracks: scan_tracks(&root),
                    root,
                });
            }
            Some(_) => {}
            None => {
                self.tracks_root = None;
                self.tracks_loading = false;
                self.tracks.clear();
            }
        }

        let desired = find(&self.tracks, self.selected.as_deref()).map(|t| {
            let base = match &self.layout {
                Some(layout) => t.path.join(layout),
                None => t.path.clone(),
            };
            let key = SurfaceKey {
                track: t.id.clone(),
                layout: self.layout.clone(),
            };
            (key, base.join("data"))
        });
        match desired {
            Some((key, data_dir)) => {
                if self.surfaces_key.as_ref() != Some(&key) {
                    self.surfaces_seq += 1;
                    let seq = self.surfaces_seq;
                    self.surfaces_key = Some(key);
                    self.surfaces = Load::Loading;
                    self.spawn(egui_ctx, move || Msg::Surfaces {
                        seq,
                        result: read_surfaces(&data_dir).map_err(|e| e.to_string()),
                    });
                }
            }
            None => {
                if self.surfaces_key.take().is_some() {
                    self.surfaces_seq += 1;
                    self.surfaces = Load::Idle;
                }
            }
        }

        let desired = match (
            find(&self.tracks, self.selected.as_deref()),
            find(&self.tracks, self.compare_with.as_deref()),
        ) {
            (Some(left), Some(right)) => Some((
                (left.id.clone(), right.id.clone()),
                left.path.clone(),
                right.path.clone(),
            )),
            _ => None,
        };
        match desired {
            Some((key, left_dir, right_dir)) => {
                if self.compare_key.as_ref() != Some(&key) {
                    self.compare_seq += 1;
                    let seq = self.compare_seq;
                    self.compare_key = Some(key);
                    self.compare = Load::Loading;
                    self.spawn(egui_ctx, move || {
                        let result = match (
                            compare_tracks(&left_dir, &right_dir),
                            surface_changes(&left_dir, &right_dir),
                        ) {
                            (Ok(rows), Ok(changes)) => Ok(Comparison { rows, changes }),
                            (Err(e), _) | (_, Err(e)) => Err(e.to_string()),
                        };
                        Msg::Compare { seq, result }
                    });
                }
            }
            None => {
                if self.compare_key.take().is_some() {
                    self.compare_seq += 1;
                    self.compare = Load::Idle;
                }
            }
        }
    }

    fn list_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Installed tracks");

        let mut rescan = false;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 96.0).max(120.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search name, id, city or country")
                    .desired_width(width),
            );
            rescan = ui.button("Rescan").clicked();
        });
        if rescan {
            self.tracks_root = None;
        }

        if self.tracks.is_empty() {
            let text = if self.tracks_loading {
                "Scanning tracks..."
            } else {
                "No tracks found in content/tracks."
            };
            ui.label(RichText::new(text).color(theme::MUTED));
            return;
        }

        let filtered = filter_tracks(&self.tracks, &self.query);
        ui.label(
            RichText::new(format!("{} of {} tracks", filtered.len(), self.tracks.len()))
                .small()
                .color(theme::MUTED),
        );
        if filtered.is_empty() {
            ui.label(RichText::new("No tracks match the search.").color(theme::MUTED));
        }

        let selected = self.selected.as_deref();
        let mut clicked: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("track_info_list")
            .max_height(260.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for t in &filtered {
                    let is_selected = selected == Some(t.id.as_str());
                    let row_clicked = widgets::list_row(ui, is_selected, |ui| {
                        ui.label(RichText::new(t.name.as_str()).strong());
                        let loc = location(t);
                        let meta = if loc.is_empty() {
                            t.id.clone()
                        } else {
                            format!("{}  |  {}", t.id, loc)
                        };
                        ui.label(RichText::new(meta).small().color(theme::MUTED));
                    });
                    if row_clicked {
                        clicked = Some(t.id.clone());
                    }
                }
            });

        if let Some(id) = clicked
            && self.selected.as_deref() != Some(id.as_str())
        {
            self.selected = Some(id);
            self.layout = None;
        }
    }

    fn details_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Details");
        let Some(track) = find(&self.tracks, self.selected.as_deref()) else {
            ui.label(RichText::new("Select a track to see its details.").color(theme::MUTED));
            return;
        };
        let surfaces = &self.surfaces;
        let mut layout = self.layout.clone();

        theme::card().show(ui, |ui| {
            ui.label(RichText::new(track.name.as_str()).size(18.0).strong());
            widgets::key_value(ui, "City", or_dash(&track.city));
            widgets::key_value(ui, "Country", or_dash(&track.country));
            let length = track
                .length_m
                .map(|m| format!("{m:.0} m"))
                .unwrap_or_else(|| "-".to_string());
            widgets::key_value(ui, "Length", &length);
            let pitboxes = track
                .pitboxes
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string());
            widgets::key_value(ui, "Pitboxes", &pitboxes);
            let layouts = if track.layouts.is_empty() {
                "none".to_string()
            } else {
                track.layouts.join(", ")
            };
            widgets::key_value(ui, "Layouts", &layouts);
        });

        ui.add_space(12.0);
        widgets::section_title(ui, "Surfaces");
        if !track.layouts.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Layout").color(theme::MUTED));
                let current = layout.clone().unwrap_or_else(|| BASE_LAYOUT.to_string());
                egui::ComboBox::from_id_salt("track_info_layout")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut layout, None, BASE_LAYOUT);
                        for name in &track.layouts {
                            ui.selectable_value(&mut layout, Some(name.clone()), name.as_str());
                        }
                    });
            });
        }

        match surfaces {
            Load::Idle => {}
            Load::Loading => {
                ui.label(RichText::new("Reading surfaces...").color(theme::MUTED));
            }
            Load::Failed(e) => {
                ui.label(
                    RichText::new(format!("Could not read surfaces.ini: {e}")).color(theme::DANGER),
                );
            }
            Load::Ready(list) => {
                let (count, valid) = summarize(list);
                ui.label(format!("{count} surfaces, {valid} valid-track"));
                if list.is_empty() {
                    ui.label(RichText::new("No surfaces.ini in this folder.").color(theme::MUTED));
                } else {
                    egui::Grid::new("track_info_surfaces")
                        .striped(true)
                        .num_columns(4)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Name").color(theme::MUTED));
                            ui.label(RichText::new("Friction").color(theme::MUTED));
                            ui.label(RichText::new("Pit").color(theme::MUTED));
                            ui.label(RichText::new("Valid track").color(theme::MUTED));
                            ui.end_row();
                            for s in list {
                                ui.label(s.name.as_str());
                                let friction = s
                                    .friction
                                    .map(|f| f.to_string())
                                    .unwrap_or_else(|| "-".to_string());
                                ui.label(friction);
                                ui.label(yes_no(s.is_pit));
                                ui.label(yes_no(s.is_valid_track));
                                ui.end_row();
                            }
                        });
                }
            }
        }

        if layout != self.layout {
            self.layout = layout;
        }
    }

    /// Thumbnail button for the selected track and the cached thumbnail path.
    fn thumbnail_section(&mut self, ui: &mut egui::Ui, root: &Path) {
        let Some(track) = find(&self.tracks, self.selected.as_deref()) else {
            return;
        };
        let id = track.id.clone();
        let cache = thumbnails::track_cache_path(root, &id);
        let thumb_file = cache.join(format!("{THUMB_SIZE}.png"));
        let shown = if thumb_file.is_file() {
            thumb_file.display().to_string()
        } else {
            "not made yet".to_string()
        };

        ui.add_space(12.0);
        widgets::section_title(ui, "Thumbnail");
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                let make = ui.add_enabled(!self.thumb_busy, widgets::primary_button("Make thumbnail"));
                if make.clicked() {
                    self.thumb_busy = true;
                    let root = root.to_path_buf();
                    let id = id.clone();
                    self.spawn(ui.ctx(), move || Msg::Thumbnail {
                        result: thumbnails::track_thumbnail(&root, &id, THUMB_SIZE)
                            .map(|bytes| {
                                let saved = thumbnails::track_cache_path(&root, &id)
                                    .join(format!("{THUMB_SIZE}.png"));
                                (saved, bytes.len())
                            })
                            .map_err(|e| e.to_string()),
                    });
                }
                if self.thumb_busy {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new("Working...").color(theme::MUTED));
                }
            });
            ui.add_space(8.0);
            widgets::key_value(ui, "Cached thumbnail", &shown);
        });
    }

    fn compare_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "Compare");
        let Some(left) = find(&self.tracks, self.selected.as_deref()) else {
            ui.label(RichText::new("Select a track to compare it with another.").color(theme::MUTED));
            return;
        };
        let left_id = left.id.clone();
        let left_name = left.name.clone();
        let tracks = &self.tracks;
        let right_name = find(tracks, self.compare_with.as_deref()).map(|t| t.name.clone());
        let mut compare_with = self.compare_with.clone();
        let mut query = self.compare_query.clone();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Compare with").color(theme::MUTED));
            let current = right_name.clone().unwrap_or_else(|| "Choose a track".to_string());
            egui::ComboBox::from_id_salt("track_info_compare")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .hint_text("Filter tracks")
                            .desired_width(240.0),
                    );
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("track_info_compare_list")
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for t in filter_tracks(tracks, &query)
                                .into_iter()
                                .filter(|t| t.id != left_id)
                            {
                                ui.selectable_value(
                                    &mut compare_with,
                                    Some(t.id.clone()),
                                    t.name.as_str(),
                                );
                            }
                        });
                });
        });
        if query != self.compare_query {
            self.compare_query = query;
        }
        if compare_with != self.compare_with {
            self.compare_with = compare_with;
        }

        let Some(right_name) = right_name else {
            ui.label(RichText::new("Pick a second track to see what differs.").color(theme::MUTED));
            return;
        };

        match &self.compare {
            Load::Idle => {}
            Load::Loading => {
                ui.label(RichText::new("Comparing...").color(theme::MUTED));
            }
            Load::Failed(e) => {
                ui.label(RichText::new(format!("Comparison failed: {e}")).color(theme::DANGER));
            }
            Load::Ready(c) => {
                if c.rows.is_empty() {
                    ui.label(RichText::new("Metadata and surfaces match.").color(theme::MUTED));
                } else {
                    egui::Grid::new("track_info_diff")
                        .striped(true)
                        .num_columns(3)
                        .show(ui, |ui| {
                            ui.label("");
                            ui.label(RichText::new(left_name.as_str()).strong());
                            ui.label(RichText::new(right_name.as_str()).strong());
                            ui.end_row();
                            for row in c.rows.iter().take(MAX_LINES) {
                                ui.label(RichText::new(row.label.as_str()).color(theme::MUTED));
                                ui.label(or_dash(&row.left));
                                ui.label(or_dash(&row.right));
                                ui.end_row();
                            }
                        });
                    if c.rows.len() > MAX_LINES {
                        ui.label(
                            RichText::new(format!("Showing first {MAX_LINES} of {} rows.", c.rows.len()))
                                .small()
                                .color(theme::MUTED),
                        );
                    }
                }

                ui.add_space(8.0);
                widgets::section_title(ui, &format!("Surface changes ({})", c.changes.len()));
                if c.changes.is_empty() {
                    ui.label(RichText::new("No surface differences.").color(theme::MUTED));
                }
                for line in c.changes.iter().take(MAX_LINES) {
                    ui.label(RichText::new(line.as_str()).monospace());
                }
                if c.changes.len() > MAX_LINES {
                    ui.label(
                        RichText::new(format!(
                            "Showing first {MAX_LINES} of {} changes.",
                            c.changes.len()
                        ))
                        .small()
                        .color(theme::MUTED),
                    );
                }
            }
        }
    }
}

impl Tool for TrackInfo {
    fn name(&self) -> &'static str {
        "Track Info"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let egui_ctx = ui.ctx().clone();
        let root = ctx.ac_root.as_ref().filter(|_| ctx.ac_valid);
        self.poll(notices);
        self.sync(&egui_ctx, root);

        widgets::heading(ui, "Track Info", "Details, surfaces and comparisons for installed tracks");
        if root.is_none() {
            widgets::empty_state(ui, "No valid Assetto Corsa folder. Set it first to browse tracks.");
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.list_section(ui);
                ui.add_space(12.0);
                self.details_section(ui);
                if let Some(root) = root {
                    self.thumbnail_section(ui, root);
                }
                ui.add_space(12.0);
                self.compare_section(ui);
            });

        // Selection and layout changes made above take effect this frame, not the next.
        self.sync(&egui_ctx, root);
    }
}
