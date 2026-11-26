//! Track Info tool page: installed tracks, their details and surfaces, and a
//! two-track comparison. Scans and file reads run on background threads.

use super::{Tool, ToolContext};
use crate::ui::viewport3d::{self, Camera3D, CpuSubMesh, DrawItem, DrawLines, LineVertex, MeshVertex, RenderMode, SceneFrame};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::ai_spline::{self, AiSpline};
use acxs_manager::content::{filter_tracks, scan_tracks, TrackEntry};
use acxs_manager::kn5_scene::{self, LoadOptions, NodeRole};
use acxs_manager::thumbnails;
use acxs_manager::track_compare::{compare_tracks, surface_changes, TrackDiff};
use acxs_manager::track_files::{self, AiHint, DrsZone, TrackCamera, TrackSection};
use acxs_manager::track_surfaces::{
    read_surfaces, set_friction, summarize, Surface, FRICTION_MAX, FRICTION_MIN,
};
use eframe::egui::{self, Align2, Color32, Pos2, RichText, Sense, Stroke, pos2, vec2};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorView {
    Map2D,
    Mesh3D,
}

struct InspectorData {
    fast_lane: Option<AiSpline>,
    pit_lane: Option<AiSpline>,
    drs_zones: Vec<DrsZone>,
    cameras: Vec<TrackCamera>,
    sections: Vec<TrackSection>,
    ai_hints: Vec<AiHint>,
}

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
        batch_busy: false,
        friction_owner: None,
        friction_draft: HashMap::new(),
        friction_busy: false,
        inspector_view: InspectorView::Map2D,
        inspector_key: None,
        inspector_seq: 0,
        inspector: Load::Idle,
        mesh_3d: None,
        uploaded_3d: None,
        cam_3d: Camera3D::default(),
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
    /// A batch thumbnail run finished: how many tracks have a thumbnail, and one line per failure.
    Batch { made: usize, failures: Vec<String> },
    /// A friction save finished: the surface name and the saved value or the error.
    Friction { surface: String, result: Result<f64, String> },
    Inspector { seq: u64, data: Box<InspectorData> },
    Mesh3D { key: (PathBuf, u64), result: Result<(Vec<CpuSubMesh>, Vec3), String> },
    HintsSaved(Result<(), String>),
    MapGenerated(Result<PathBuf, String>),
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
    /// The all-tracks thumbnail job is running.
    batch_busy: bool,
    /// The surfaces read the friction drafts belong to. Drafts reset when it changes.
    friction_owner: Option<SurfaceKey>,
    /// Friction values being edited in the surfaces table, by surface name.
    friction_draft: HashMap<String, f64>,
    /// A friction save is running.
    friction_busy: bool,
    inspector_view: InspectorView,
    inspector_key: Option<SurfaceKey>,
    inspector_seq: u64,
    inspector: Load<InspectorData>,
    mesh_3d: Option<((PathBuf, u64), Vec<CpuSubMesh>)>,
    uploaded_3d: Option<(PathBuf, u64)>,
    cam_3d: Camera3D,
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
                Msg::Batch { made, failures } => {
                    self.batch_busy = false;
                    let text = if failures.is_empty() {
                        format!("{made} track thumbnails ready ({THUMB_SIZE}x{THUMB_SIZE})")
                    } else {
                        format!(
                            "{made} track thumbnails ready, {} failed. First: {}",
                            failures.len(),
                            failures[0]
                        )
                    };
                    notices.push(Notice { text, ok: failures.is_empty() });
                }
                Msg::Friction { surface, result } => {
                    self.friction_busy = false;
                    match result {
                        Ok(value) => {
                            notices.push(Notice {
                                text: format!("Saved FRICTION {value:.2} on {surface}"),
                                ok: true,
                            });
                            // Re-read the file so the table shows what was written.
                            self.surfaces_key = None;
                        }
                        Err(e) => notices.push(Notice {
                            text: format!("Could not save friction on {surface}: {e}"),
                            ok: false,
                        }),
                    }
                }
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
                Msg::Inspector { seq, data } if seq == self.inspector_seq => {
                    if let Some(sp) = &data.fast_lane
                        && let Some(p) = sp.points.first()
                    {
                        self.cam_3d = Camera3D::Orbit {
                            target: Vec3::new(p.x, p.y, p.z),
                            yaw: 0.7,
                            pitch: 0.75,
                            dist: 260.0,
                        };
                    }
                    self.inspector = Load::Ready(*data);
                }
                Msg::Mesh3D { key, result } => match result {
                    Ok((subs, center)) => {
                        self.cam_3d = Camera3D::Orbit {
                            target: center,
                            yaw: 0.7,
                            pitch: 0.65,
                            dist: 280.0,
                        };
                        self.mesh_3d = Some((key, subs));
                        self.uploaded_3d = None;
                    }
                    Err(e) => notices.push(Notice {
                        text: format!("Could not load 3D track mesh: {e}"),
                        ok: false,
                    }),
                },
                Msg::HintsSaved(Ok(())) => notices.push(Notice {
                    text: "Saved ai_hints.ini (backup created).".into(),
                    ok: true,
                }),
                Msg::HintsSaved(Err(e)) => notices.push(Notice {
                    text: format!("Could not save ai_hints.ini: {e}"),
                    ok: false,
                }),
                Msg::MapGenerated(Ok(path)) => notices.push(Notice {
                    text: format!("Generated map.png + map.ini in {}", path.display()),
                    ok: true,
                }),
                Msg::MapGenerated(Err(e)) => notices.push(Notice {
                    text: format!("Could not generate track map: {e}"),
                    ok: false,
                }),
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
                    self.surfaces_key = Some(key.clone());
                    self.surfaces = Load::Loading;
                    let surf_dir = data_dir.clone();
                    self.spawn(egui_ctx, move || Msg::Surfaces {
                        seq,
                        result: read_surfaces(&surf_dir).map_err(|e| e.to_string()),
                    });
                }
                if self.inspector_key.as_ref() != Some(&key) {
                    self.inspector_seq += 1;
                    let iseq = self.inspector_seq;
                    self.inspector_key = Some(key);
                    self.inspector = Load::Loading;
                    self.mesh_3d = None;
                    self.uploaded_3d = None;
                    let base_dir = data_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| data_dir.clone());
                    let data_dir2 = data_dir.clone();
                    self.spawn(egui_ctx, move || {
                        let fast_lane = ai_spline::read_ai_spline(&base_dir.join("ai").join("fast_lane.ai")).ok();
                        let pit_lane = ai_spline::read_ai_spline(&base_dir.join("ai").join("pit_lane.ai")).ok();
                        let drs_zones = track_files::read_drs_zones(&data_dir2).unwrap_or_default();
                        let cameras = track_files::read_cameras(&data_dir2.join("cameras.ini")).unwrap_or_default();
                        let sections = track_files::read_sections(&data_dir2.join("sections.ini"))
                            .ok()
                            .filter(|v| !v.is_empty())
                            .or_else(|| track_files::read_sections(&base_dir.join("ui").join("sections.ini")).ok())
                            .unwrap_or_default();
                        let ai_hints = track_files::read_ai_hints(&data_dir2).unwrap_or_default();
                        Msg::Inspector {
                            seq: iseq,
                            data: Box::new(InspectorData {
                                fast_lane,
                                pit_lane,
                                drs_zones,
                                cameras,
                                sections,
                                ai_hints,
                            }),
                        }
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
        let data_dir = match &self.layout {
            Some(name) => track.path.join(name),
            None => track.path.clone(),
        }
        .join("data");
        if self.friction_owner != self.surfaces_key {
            self.friction_owner = self.surfaces_key.clone();
            self.friction_draft.clear();
        }
        let busy = self.friction_busy || self.thumb_busy || self.batch_busy;
        let surfaces = &self.surfaces;
        let drafts = &mut self.friction_draft;
        let mut save: Option<(String, f64)> = None;
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
                        .num_columns(5)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Name").color(theme::MUTED));
                            ui.label(RichText::new("Friction").color(theme::MUTED));
                            ui.label("");
                            ui.label(RichText::new("Pit").color(theme::MUTED));
                            ui.label(RichText::new("Valid track").color(theme::MUTED));
                            ui.end_row();
                            for s in list {
                                ui.label(s.name.as_str());
                                let mut value = *drafts
                                    .entry(s.name.clone())
                                    .or_insert(s.friction.unwrap_or(0.0));
                                ui.add(
                                    egui::DragValue::new(&mut value)
                                        .range(FRICTION_MIN..=FRICTION_MAX)
                                        .speed(0.01)
                                        .fixed_decimals(2),
                                );
                                drafts.insert(s.name.clone(), value);
                                let changed = s.friction.is_none_or(|f| (f - value).abs() > 1e-9);
                                let save_btn = ui.add_enabled(
                                    !busy && changed,
                                    widgets::primary_button("Save"),
                                );
                                if save_btn.clicked() {
                                    save = Some((s.name.clone(), value));
                                }
                                ui.label(yes_no(s.is_pit));
                                ui.label(yes_no(s.is_valid_track));
                                ui.end_row();
                            }
                        });
                }
            }
        }

        if let Some((surface, value)) = save {
            self.friction_busy = true;
            self.spawn(ui.ctx(), move || Msg::Friction {
                result: set_friction(&data_dir, &surface, value)
                    .map(|()| value)
                    .map_err(|e| e.to_string()),
                surface,
            });
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
                let busy = self.thumb_busy || self.batch_busy;
                let make = ui.add_enabled(!busy, widgets::primary_button("Make thumbnail"));
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

    /// Button that makes a thumbnail for every track with a model, on a background thread.
    fn batch_thumbnail_row(&mut self, ui: &mut egui::Ui, root: &Path) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let enabled = !(self.thumb_busy || self.batch_busy);
            if ui
                .add_enabled(enabled, widgets::primary_button("Make all track thumbnails"))
                .clicked()
            {
                self.batch_busy = true;
                let root = root.to_path_buf();
                self.spawn(ui.ctx(), move || {
                    let mut made = 0;
                    let mut failures = Vec::new();
                    for (id, result) in thumbnails::batch_track_thumbnails(&root, THUMB_SIZE) {
                        match result {
                            Ok(_) => made += 1,
                            Err(e) => failures.push(format!("{id}: {e}")),
                        }
                    }
                    Msg::Batch { made, failures }
                });
            }
            if self.batch_busy {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Working...").color(theme::MUTED));
            }
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
                self.inspector_section(ui, ctx);
                ui.add_space(12.0);
                self.details_section(ui);
                if let Some(root) = root {
                    self.thumbnail_section(ui, root);
                    self.batch_thumbnail_row(ui, root);
                }
                ui.add_space(12.0);
                self.compare_section(ui);
            });

        // Selection and layout changes made above take effect this frame, not the next.
        self.sync(&egui_ctx, root);
    }
}

impl TrackInfo {
    fn inspector_section(&mut self, ui: &mut egui::Ui, ctx: &ToolContext) {
        let Some(track) = find(&self.tracks, self.selected.as_deref()).cloned() else {
            return;
        };
        let base_dir = match &self.layout {
            Some(l) => track.path.join(l),
            None => track.path.clone(),
        };
        let data_dir = base_dir.join("data");

        theme::card_elevated().show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::section_title(ui, "Track & Map Inspector");
                ui.add_space(12.0);
                widgets::animated_tabs(
                    ui,
                    "track_inspector_tabs",
                    &mut self.inspector_view,
                    &[
                        (InspectorView::Map2D, "2D Map", None),
                        (InspectorView::Mesh3D, "3D Track", None),
                    ],
                );
            });

            match &mut self.inspector {
                Load::Idle | Load::Loading => {
                    ui.spinner();
                    return;
                }
                Load::Failed(e) => {
                    ui.label(RichText::new(e.as_str()).color(theme::CH_BRAKE));
                    return;
                }
                Load::Ready(data) => {
                    ui.horizontal_wrapped(|ui| {
                        widgets::pill(
                            ui,
                            if data.fast_lane.is_some() { "fast_lane.ai OK" } else { "No fast_lane.ai" },
                            if data.fast_lane.is_some() { theme::OK } else { theme::WARN },
                        );
                        widgets::pill(ui, &format!("{} DRS", data.drs_zones.len()), theme::CH_THROTTLE);
                        widgets::pill(ui, &format!("{} TV Cams", data.cameras.len()), theme::CH_SPEED);
                        widgets::pill(ui, &format!("{} Sections", data.sections.len()), theme::MUTED);
                        if let Some(sp) = data.fast_lane.clone()
                            && ui
                                .add_enabled(!ctx.game_running, egui::Button::new("Generate map.png + map.ini"))
                                .clicked()
                        {
                            let out_base = base_dir.clone();
                            let out_data = data_dir.clone();
                            let tx = self.tx.clone();
                            let repaint = ui.ctx().clone();
                            thread::spawn(move || {
                                let res = (|| -> Result<PathBuf, String> {
                                    let (png_bytes, map) =
                                        track_files::generate_track_map_png(&sp, 1024, 24)
                                            .map_err(|e| e.to_string())?;
                                    let png_path = out_base.join("map.png");
                                    if png_path.is_file() {
                                        let _ = std::fs::rename(&png_path, out_base.join("map.png.acxs-backup"));
                                    }
                                    std::fs::write(&png_path, png_bytes).map_err(|e| e.to_string())?;
                                    std::fs::create_dir_all(&out_data).map_err(|e| e.to_string())?;
                                    let ini_path = out_data.join("map.ini");
                                    if ini_path.is_file() {
                                        let _ = std::fs::rename(&ini_path, out_data.join("map.ini.acxs-backup"));
                                    }
                                    std::fs::write(&ini_path, track_files::format_map_ini(&map))
                                        .map_err(|e| e.to_string())?;
                                    Ok(out_base)
                                })();
                                let _ = tx.send(Msg::MapGenerated(res));
                                repaint.request_repaint();
                            });
                        }
                    });
                    ui.add_space(6.0);

                    if self.inspector_view == InspectorView::Map2D {
                        paint_track_2d(ui, data);
                    } else {
                        if self.mesh_3d.is_none()
                            && ui.button("Load 3D KN5 Track Mesh").clicked()
                        {
                            let tdir = track.path.clone();
                            let lay = self.layout.clone();
                            let tx = self.tx.clone();
                            let repaint = ui.ctx().clone();
                            thread::spawn(move || {
                                let res = load_track_3d_meshes(&tdir, lay.as_deref());
                                let _ = tx.send(Msg::Mesh3D { key: (tdir, 0), result: res });
                                repaint.request_repaint();
                            });
                        }
                        if let (Some((key, subs)), Some(gl)) = (&self.mesh_3d, &ctx.gl)
                            && self.uploaded_3d.as_ref() != Some(key)
                            && let Ok(mut guard) = gl.lock()
                        {
                            guard.upload_mesh(key.clone(), subs);
                            self.uploaded_3d = Some(key.clone());
                        }
                        let mut frame = SceneFrame {
                            camera: self.cam_3d,
                            render_mode: RenderMode::Shaded,
                            ..SceneFrame::default()
                        };
                        if let Some((key, _)) = &self.mesh_3d {
                            frame.items.push(DrawItem {
                                mesh_key: key.clone(),
                                model: Mat4::IDENTITY,
                                tint_override: None,
                                texture_override: None,
                            });
                        }
                        if let Some(sp) = &data.fast_lane {
                            frame.lines.push(DrawLines {
                                vertices: sp
                                    .points
                                    .iter()
                                    .map(|p| LineVertex {
                                        pos: [p.x, p.y + 0.5, p.z],
                                        color: [0.2, 0.85, 1.0, 1.0],
                                    })
                                    .collect(),
                                strip: true,
                            });
                        }
                        let dt = ui.input(|i| i.stable_dt);
                        let gl = ctx.gl.clone();
                        widgets::viewport_3d_frame(
                            ui,
                            vec2(ui.available_width(), 300.0),
                            |ui| {
                                widgets::pill(ui, "3D TRACK INSPECTOR", theme::CH_SPEED);
                            },
                            |ui, rect| {
                                let resp = ui.interact(rect, ui.scope_id().with("ti_3d"), Sense::click_and_drag());
                                ui.input(|i| {
                                    if self.cam_3d.handle_input(&resp, i, dt) {
                                        ui.ctx().request_repaint();
                                    }
                                });
                                frame.camera = self.cam_3d;
                                viewport3d::paint_viewport_3d(ui, rect, gl, frame, |ui, r| {
                                    ui.painter().rect_filled(r, 0, theme::SURFACE_VOID);
                                    ui.painter().text(
                                        r.center(),
                                        Align2::CENTER_CENTER,
                                        "3D Track Inspector (2D fallback in headless mode)",
                                        egui::FontId::proportional(13.0),
                                        theme::MUTED,
                                    );
                                });
                            },
                        );
                    }

                    // Inline ai_hints.ini editor
                    if !data.ai_hints.is_empty() {
                        ui.add_space(8.0);
                        egui::CollapsingHeader::new(format!("AI Hints ({})", data.ai_hints.len()))
                            .show(ui, |ui| {
                                for h in &mut data.ai_hints {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&h.section).font(theme::mono(12.0)));
                                        ui.add(egui::DragValue::new(&mut h.start).speed(0.005).range(0.0..=1.0).prefix("Start "));
                                        ui.add(egui::DragValue::new(&mut h.end).speed(0.005).range(0.0..=1.0).prefix("End "));
                                        ui.add(egui::DragValue::new(&mut h.value).speed(0.01).range(0.0..=2.0).prefix("Val "));
                                    });
                                }
                                if ui
                                    .add_enabled(!ctx.game_running, egui::Button::new("Save ai_hints.ini"))
                                    .clicked()
                                {
                                    let ddir = data_dir.clone();
                                    let hints = data.ai_hints.clone();
                                    let tx = self.tx.clone();
                                    let repaint = ui.ctx().clone();
                                    thread::spawn(move || {
                                        let res = track_files::save_ai_hints(&ddir, &hints).map_err(|e| e.to_string());
                                        let _ = tx.send(Msg::HintsSaved(res));
                                        repaint.request_repaint();
                                    });
                                }
                            });
                    }
                }
            }
        });
    }
}

fn paint_track_2d(ui: &mut egui::Ui, data: &InspectorData) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 260.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6, theme::SURFACE_VOID);
    let Some(sp) = &data.fast_lane else {
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No ai/fast_lane.ai found for this layout.",
            egui::FontId::proportional(13.0),
            theme::MUTED,
        );
        return;
    };
    if sp.points.len() < 2 {
        return;
    }
    let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut min_z, mut max_z) = (f32::INFINITY, f32::NEG_INFINITY);
    for pt in &sp.points {
        min_x = min_x.min(pt.x);
        max_x = max_x.max(pt.x);
        min_z = min_z.min(pt.z);
        max_z = max_z.max(pt.z);
    }
    let span = (max_x - min_x).max(max_z - min_z).max(1.0);
    let scale = ((rect.width() - 32.0).min(rect.height() - 32.0) / span).max(1e-4);
    let cx = 0.5 * (min_x + max_x);
    let cz = 0.5 * (min_z + max_z);
    let to_scr = |x: f32, z: f32| -> Pos2 {
        pos2(rect.center().x + (x - cx) * scale, rect.center().y + (z - cz) * scale)
    };

    let (left, right) = sp.corridor_edges();
    p.add(egui::Shape::line(
        left.iter().map(|q| to_scr(q[0], q[2])).collect(),
        Stroke::new(1.2, Color32::from_gray(90)),
    ));
    p.add(egui::Shape::line(
        right.iter().map(|q| to_scr(q[0], q[2])).collect(),
        Stroke::new(1.2, Color32::from_gray(90)),
    ));
    p.add(egui::Shape::line(
        sp.points.iter().map(|q| to_scr(q.x, q.z)).collect(),
        Stroke::new(2.0, theme::CH_SPEED),
    ));
    if let Some(pit) = &data.pit_lane {
        p.add(egui::Shape::line(
            pit.points.iter().map(|q| to_scr(q.x, q.z)).collect(),
            Stroke::new(1.5, theme::WARN),
        ));
    }
    for cam in &data.cameras {
        let pos = to_scr(cam.position[0], cam.position[2]);
        p.circle_filled(pos, 3.0, theme::CH_GEAR);
    }
}

fn load_track_3d_meshes(track_dir: &Path, layout: Option<&str>) -> Result<(Vec<CpuSubMesh>, Vec3), String> {
    let opts = LoadOptions {
        load_textures: false,
        skip_ac_helpers: true,
        max_vertices: 2_000_000,
        max_triangles: 2_000_000,
    };
    let scene = kn5_scene::load_track_layout(track_dir, layout, &opts).map_err(|e| e.to_string())?;
    let mut subs = Vec::new();
    let mut min_p = Vec3::splat(f32::INFINITY);
    let mut max_p = Vec3::splat(f32::NEG_INFINITY);
    for (_, model) in &scene.models {
        for m in &model.meshes {
            if m.name.starts_with("AC_") || m.indices.len() < 3 {
                continue;
            }
            let tint = match &m.role {
                NodeRole::SurfaceMesh { surface_key, .. } => match surface_key.as_str() {
                    "ROAD" => [0.28, 0.30, 0.34, 1.0],
                    "KERB" => [0.85, 0.22, 0.22, 1.0],
                    "GRASS" => [0.22, 0.48, 0.24, 1.0],
                    "SAND" | "GRAVEL" => [0.76, 0.68, 0.48, 1.0],
                    _ => [0.55, 0.58, 0.62, 1.0],
                },
                _ => [0.65, 0.68, 0.72, 1.0],
            };
            let vertices: Vec<MeshVertex> = (0..m.positions.len())
                .map(|i| {
                    let pos = kn5_scene::mat4_transform(&m.transform, &m.positions[i], true);
                    let v = Vec3::from_array(pos);
                    min_p = min_p.min(v);
                    max_p = max_p.max(v);
                    MeshVertex {
                        pos,
                        normal: kn5_scene::mat4_transform(&m.transform, &m.normals[i], false),
                        uv: m.uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                    }
                })
                .collect();
            let indices = m.indices.iter().map(|&idx| u32::from(idx)).collect();
            subs.push(CpuSubMesh {
                vertices,
                indices,
                texture_key: None,
                tint,
            });
        }
    }
    let center = if min_p.is_finite() && max_p.is_finite() {
        (min_p + max_p) * 0.5
    } else {
        Vec3::ZERO
    };
    Ok((subs, center))
}
