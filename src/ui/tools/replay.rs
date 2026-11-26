//! Replay Viewer tool page: opens an `.acreplay` file, plays back its telemetry,
//! shows its summary and exports the lap table and the telemetry samples as CSV.
//! Parsing and writing run on background threads.

use super::{Tool, ToolContext};
use crate::ui::viewport3d::{self, Camera3D, DrawLines, LineVertex, RenderMode, SceneFrame};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::ai_spline::{self, AiSpline};
use acxs_manager::game;
use acxs_manager::race;
use acxs_manager::replay::{
    format_lap_time, read_replay_multicar, trim_replay_file, MultiCarReplay, ReplayInfo, Sample,
};
use acxs_manager::replay_analysis;
use acxs_manager::replay_library::{self, ReplayFile, ReplayPlace};
use acxs_manager::replay_player::Player;
use acxs_manager::settings;
use acxs_manager::telemetry::LapInfo;
use acxs_manager::telemetry_csv::{laps_to_csv, replay_samples_to_csv, write_csv};
use acxs_manager::track_files::{self, TrackCamera};
use eframe::egui::{self, Align2, Color32, Pos2, RichText, Sense, Stroke, pos2, vec2};
use glam::Vec3;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Playback speeds offered by the speed selector. The player clamps them to its own range.
const SPEEDS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayView {
    Telemetry,
    Map2D,
    View3D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayCamMode {
    Chase,
    FreeFly,
    Tv,
}

pub fn tool() -> Box<dyn Tool> {
    Box::new(ReplayViewer::new())
}

/// Results sent back from background threads.
enum Msg {
    Parsed {
        path: PathBuf,
        result: Result<Box<MultiCarReplay>, String>,
        spline: Option<AiSpline>,
        cameras: Vec<TrackCamera>,
    },
    Exported(Result<PathBuf, String>),
    /// Result of the samples export: the path written and the number of data rows.
    SamplesExported(Result<(PathBuf, usize), String>),
    Trimmed(Result<(PathBuf, usize), String>),
}

/// A replay shown in the viewer: its multi-car data plus the playback model.
struct Loaded {
    path: PathBuf,
    info: ReplayInfo,
    multicar: MultiCarReplay,
    car_index: usize,
    player: Player,
    /// True when the file had more frames than were stored.
    capped: bool,
    spline: Option<AiSpline>,
    cameras: Vec<TrackCamera>,
    view: ReplayView,
    cam_mode: ReplayCamMode,
    camera_3d: Camera3D,
    trim_start: u32,
    trim_end: u32,
}

struct ReplayViewer {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// True while a background parse or export is running.
    busy: bool,
    /// The replay currently shown, with its playback state.
    loaded: Option<Loaded>,
    /// The replays on this PC; read when the page opens.
    library: Option<Vec<ReplayFile>>,
}

impl ReplayViewer {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        ReplayViewer {
            tx,
            rx,
            busy: false,
            loaded: None,
            library: None,
        }
    }

    /// Parse `path` (all cars) and load optional track `fast_lane.ai` + `cameras.ini` on a background thread.
    fn start_parse(&mut self, path: PathBuf, ac_root: Option<PathBuf>, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = read_replay_multicar(&path, 500_000)
                .map(Box::new)
                .map_err(|e| e.to_string());
            let (spline, cameras) = match (&result, &ac_root) {
                (Ok(mc), Some(root)) => {
                    let mut track_dir = root.join("content").join("tracks").join(&mc.info.track);
                    if let Some(cfg) = &mc.info.config_track {
                        let layout_dir = track_dir.join(cfg);
                        if layout_dir.is_dir() {
                            track_dir = layout_dir;
                        }
                    }
                    let sp = ai_spline::read_ai_spline(&track_dir.join("ai").join("fast_lane.ai")).ok();
                    let cams = track_files::read_cameras(&track_dir.join("data").join("cameras.ini"))
                        .unwrap_or_default();
                    (sp, cams)
                }
                _ => (None, Vec::new()),
            };
            let _ = tx.send(Msg::Parsed {
                path,
                result,
                spline,
                cameras,
            });
            ctx.request_repaint();
        });
    }

    /// Write the lap table of the loaded replay to `path` on a background thread.
    fn start_export(&mut self, path: PathBuf, laps: Vec<LapInfo>, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let csv = laps_to_csv(&laps);
            let result = write_csv(&path, &csv)
                .map(|()| path.clone())
                .map_err(|e| write_failure(&path, &e));
            let _ = tx.send(Msg::Exported(result));
            ctx.request_repaint();
        });
    }

    /// Write the telemetry samples of the loaded replay to `path` on a background thread.
    fn start_samples_export(&mut self, path: PathBuf, samples: Vec<Sample>, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let rows = samples.len();
            let csv = replay_samples_to_csv(&samples);
            let result = write_csv(&path, &csv)
                .map(|()| (path.clone(), rows))
                .map_err(|e| write_failure(&path, &e));
            let _ = tx.send(Msg::SamplesExported(result));
            ctx.request_repaint();
        });
    }
}

/// User-facing message for a failed write to `path`.
fn write_failure(path: &Path, e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::AlreadyExists {
        format!("{} already exists. Choose another file name.", path.display())
    } else {
        format!("Could not write {}: {e}", path.display())
    }
}

impl Tool for ReplayViewer {
    fn name(&self) -> &'static str {
        "Replay Viewer"
    }

    fn content_width(&self) -> f32 {
        1400.0
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            match msg {
                Msg::Parsed {
                    path,
                    result: Ok(mc),
                    spline,
                    cameras,
                } => {
                    notices.push(Notice {
                        text: format!("Opened {} ({} car(s))", file_name(&path), mc.cars.len()),
                        ok: true,
                    });
                    let capped = mc.capped;
                    let info = mc.info.clone();
                    let samples: Vec<Sample> = mc
                        .cars
                        .first()
                        .map(|c| c.frames.iter().map(|f| f.sample).collect())
                        .unwrap_or_default();
                    let total_frames = samples.len() as u32;
                    let player = Player::new(samples);
                    self.loaded = Some(Loaded {
                        path,
                        info,
                        multicar: *mc,
                        car_index: 0,
                        player,
                        capped,
                        spline,
                        cameras,
                        view: ReplayView::Telemetry,
                        cam_mode: ReplayCamMode::Chase,
                        camera_3d: Camera3D::default(),
                        trim_start: 0,
                        trim_end: total_frames.max(1),
                    });
                }
                Msg::Parsed { path, result: Err(e), .. } => {
                    self.loaded = None;
                    notices.push(Notice {
                        text: format!("Could not open {}: {e}", file_name(&path)),
                        ok: false,
                    });
                }
                Msg::Exported(Ok(path)) => notices.push(Notice {
                    text: format!("Wrote lap CSV to {}", path.display()),
                    ok: true,
                }),
                Msg::Exported(Err(e)) => notices.push(Notice { text: e, ok: false }),
                Msg::SamplesExported(Ok((path, rows))) => notices.push(Notice {
                    text: format!("Wrote {rows} sample rows to {}", path.display()),
                    ok: true,
                }),
                Msg::SamplesExported(Err(e)) => notices.push(Notice { text: e, ok: false }),
                Msg::Trimmed(Ok((path, bytes))) => notices.push(Notice {
                    text: format!("Exported clipped replay ({} KB) to {}", bytes / 1024, path.display()),
                    ok: true,
                }),
                Msg::Trimmed(Err(e)) => notices.push(Notice {
                    text: format!("Could not trim replay: {e}"),
                    ok: false,
                }),
            }
        }

        // Move the playhead by the frame time while playing, and keep repainting.
        if let Some(loaded) = &mut self.loaded
            && loaded.player.is_playing()
        {
            let dt = f64::from(ui.input(|i| i.stable_dt));
            loaded.player.advance(dt);
            ui.ctx().request_repaint();
        }

        widgets::heading(
            ui,
            "Replay Viewer",
            "Play back multi-car .acreplay telemetry in 2D/3D, trim shareable clips, or launch directly in AC.",
        );

        let files = self.library.get_or_insert_with(read_library);
        let mut reread = false;
        let mut open = None;
        egui::CollapsingHeader::new(format!("Replay library ({})", files.len()))
            .default_open(self.loaded.is_none())
            .show(ui, |ui| open = library_rows(ui, files, notices, &mut reread));
        if reread {
            self.library = None;
        }
        if let Some(path) = open
            && !self.busy
        {
            self.start_parse(path, ctx.ac_root.clone(), ui.ctx().clone());
        }
        ui.add_space(6.0);

        ui.horizontal_wrapped(|ui| {
            let open = ui.add_enabled(!self.busy, widgets::primary_button("Open replay"));
            if open.clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter("acreplay", &["acreplay"])
                    .pick_file();
                if let Some(path) = picked {
                    self.start_parse(path, ctx.ac_root.clone(), ui.ctx().clone());
                }
            }

            let can_export = !self.busy && self.loaded.is_some();
            if ui
                .add_enabled(can_export && !ctx.game_running && ctx.ac_valid, egui::Button::new("Watch in AC"))
                .on_hover_text("Writes [REPLAY] ACTIVE=1 to race.ini and launches Assetto Corsa.")
                .clicked()
                && let (Some(loaded), Some(root), Some(cfg)) =
                    (&self.loaded, &ctx.ac_root, settings::ac_user_cfg_dir())
            {
                let fname = file_name(&loaded.path);
                match race::write_replay_ini(&cfg, &fname).and_then(|_| game::launch_game(root)) {
                    Ok(_) => notices.push(Notice {
                        text: format!("Launched AC replay playback for {fname}."),
                        ok: true,
                    }),
                    Err(e) => notices.push(Notice {
                        text: format!("Could not launch replay in AC: {e}"),
                        ok: false,
                    }),
                }
            }

            let export = ui.add_enabled(can_export, egui::Button::new("Export laps CSV"));
            if export.clicked() {
                let laps = self
                    .loaded
                    .as_ref()
                    .map(|loaded| lap_rows(&loaded.info))
                    .unwrap_or_default();
                let picked = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_file_name("laps.csv")
                    .save_file();
                if let Some(path) = picked {
                    self.start_export(path, laps, ui.ctx().clone());
                }
            }

            let export_samples =
                ui.add_enabled(can_export, egui::Button::new("Export samples CSV"));
            if export_samples.clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_file_name("samples.csv")
                    .save_file();
                if let Some(path) = picked {
                    let samples = self
                        .loaded
                        .as_ref()
                        .map(|loaded| loaded.player.samples().to_vec())
                        .unwrap_or_default();
                    self.start_samples_export(path, samples, ui.ctx().clone());
                }
            }

            if self.busy {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Working...").color(theme::MUTED));
            }
        });
        ui.add_space(12.0);

        let mut trim_req: Option<(PathBuf, PathBuf, u32, u32)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match &mut self.loaded {
                None => widgets::empty_state(ui, "No replay loaded."),
                Some(loaded) => {
                    ui.horizontal(|ui| {
                        widgets::animated_tabs(
                            ui,
                            "replay_view_tabs",
                            &mut loaded.view,
                            &[
                                (ReplayView::Telemetry, "Telemetry", None),
                                (ReplayView::Map2D, "2D Multi-Car", None),
                                (ReplayView::View3D, "3D Viewer", None),
                            ],
                        );
                        if loaded.multicar.cars.len() > 1 {
                            ui.add_space(10.0);
                            let cur_name = loaded
                                .multicar
                                .cars
                                .get(loaded.car_index)
                                .map(|c| format!("Car {}: {}", loaded.car_index + 1, c.driver))
                                .unwrap_or_default();
                            egui::ComboBox::from_id_salt("replay_car_pick")
                                .selected_text(cur_name)
                                .show_ui(ui, |ui| {
                                    for (i, c) in loaded.multicar.cars.iter().enumerate() {
                                        if ui
                                            .selectable_label(
                                                loaded.car_index == i,
                                                format!("Car {}: {} ({})", i + 1, c.driver, c.car_id),
                                            )
                                            .clicked()
                                        {
                                            loaded.car_index = i;
                                            let samples: Vec<Sample> =
                                                c.frames.iter().map(|f| f.sample).collect();
                                            let pos = loaded.player.position_s();
                                            loaded.player = Player::new(samples);
                                            loaded.player.seek(pos);
                                        }
                                    }
                                });
                        }
                    });
                    ui.add_space(8.0);
                    playback(ui, loaded);
                    ui.add_space(10.0);
                    match loaded.view {
                        ReplayView::Telemetry => {}
                        ReplayView::Map2D => replay_map_2d(ui, loaded),
                        ReplayView::View3D => replay_view_3d(ui, loaded, ctx),
                    }
                    ui.add_space(10.0);
                    if let Some(req) = trim_clip_card(ui, loaded, self.busy) {
                        trim_req = Some(req);
                    }
                    ui.add_space(12.0);
                    summary(ui, &loaded.path, &loaded.info, loaded.player.samples());
                }
            });

        if let Some((src, dst, start, end)) = trim_req {
            self.busy = true;
            let tx = self.tx.clone();
            let repaint = ui.ctx().clone();
            thread::spawn(move || {
                let res = trim_replay_file(&src, &dst, start, end)
                    .map(|n| (dst, n))
                    .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Trimmed(res));
                repaint.request_repaint();
            });
        }
    }
}

fn current_frame_idx(loaded: &Loaded) -> usize {
    let dt = (loaded.multicar.recording_interval_ms / 1000.0).max(1e-4);
    (loaded.player.position_s() / dt).floor().max(0.0) as usize
}

fn replay_map_2d(ui: &mut egui::Ui, loaded: &Loaded) {
    let idx = current_frame_idx(loaded);
    theme::card_elevated().show(ui, |ui| {
        widgets::section_title(ui, "2D Multi-Car Track View");
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 6, theme::SURFACE_VOID);

        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_z = f32::INFINITY;
        let mut max_z = f32::NEG_INFINITY;
        if let Some(sp) = &loaded.spline {
            for pt in &sp.points {
                min_x = min_x.min(pt.x);
                max_x = max_x.max(pt.x);
                min_z = min_z.min(pt.z);
                max_z = max_z.max(pt.z);
            }
        }
        for c in &loaded.multicar.cars {
            for f in c.frames.iter().step_by(8) {
                min_x = min_x.min(f.sample.x);
                max_x = max_x.max(f.sample.x);
                min_z = min_z.min(f.sample.z);
                max_z = max_z.max(f.sample.z);
            }
        }
        if !min_x.is_finite() {
            return;
        }
        let span = (max_x - min_x).max(max_z - min_z).max(1.0);
        let scale = ((rect.width() - 36.0).min(rect.height() - 36.0) / span).max(1e-4);
        let cx = 0.5 * (min_x + max_x);
        let cz = 0.5 * (min_z + max_z);
        let to_scr = |x: f32, z: f32| -> Pos2 {
            pos2(rect.center().x + (x - cx) * scale, rect.center().y + (z - cz) * scale)
        };

        if let Some(sp) = &loaded.spline {
            let pts: Vec<Pos2> = sp.points.iter().map(|pt| to_scr(pt.x, pt.z)).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_gray(85))));
        } else if let Some(first) = loaded.multicar.cars.first() {
            let pts: Vec<Pos2> = first
                .frames
                .iter()
                .step_by(4)
                .map(|f| to_scr(f.sample.x, f.sample.z))
                .collect();
            p.add(egui::Shape::line(pts, Stroke::new(1.5, Color32::from_gray(75))));
        }

        for (ci, car) in loaded.multicar.cars.iter().enumerate() {
            if let Some(f) = car.frames.get(idx).or_else(|| car.frames.last()) {
                let pos = to_scr(f.sample.x, f.sample.z);
                let col = if ci == loaded.car_index { theme::ACCENT } else { theme::CH_SPEED };
                p.circle_filled(pos, if ci == loaded.car_index { 6.0 } else { 4.0 }, col);
                p.text(
                    pos + vec2(8.0, -6.0),
                    Align2::LEFT_CENTER,
                    &car.driver,
                    egui::FontId::proportional(11.0),
                    Color32::WHITE,
                );
            }
        }
    });
}

fn replay_view_3d(ui: &mut egui::Ui, loaded: &mut Loaded, ctx: &ToolContext) {
    let idx = current_frame_idx(loaded);
    let focus_pos = loaded
        .multicar
        .cars
        .get(loaded.car_index)
        .and_then(|c| c.frames.get(idx).or_else(|| c.frames.last()))
        .map(|f| Vec3::new(f.sample.x, f.sample.y, f.sample.z))
        .unwrap_or(Vec3::ZERO);

    let mut lines = Vec::new();
    if let Some(sp) = &loaded.spline {
        let (l, r) = sp.corridor_edges();
        let col = [0.40, 0.44, 0.52, 0.9];
        lines.push(DrawLines {
            vertices: l.iter().map(|&pos| LineVertex { pos, color: col }).collect(),
            strip: true,
        });
        lines.push(DrawLines {
            vertices: r.iter().map(|&pos| LineVertex { pos, color: col }).collect(),
            strip: true,
        });
    }
    // Render car bounding markers in 3D space.
    for (ci, car) in loaded.multicar.cars.iter().enumerate() {
        if let Some(f) = car.frames.get(idx).or_else(|| car.frames.last()) {
            let p = [f.sample.x, f.sample.y + 0.6, f.sample.z];
            let col = if ci == loaded.car_index {
                [1.0, 0.45, 0.15, 1.0]
            } else {
                [0.2, 0.85, 1.0, 1.0]
            };
            lines.push(DrawLines {
                vertices: vec![
                    LineVertex { pos: [p[0] - 1.5, p[1], p[2]], color: col },
                    LineVertex { pos: [p[0] + 1.5, p[1], p[2]], color: col },
                    LineVertex { pos: [p[0], p[1], p[2] - 2.2], color: col },
                    LineVertex { pos: [p[0], p[1], p[2] + 2.2], color: col },
                ],
                strip: false,
            });
        }
    }

    match loaded.cam_mode {
        ReplayCamMode::Chase => {
            loaded.camera_3d = Camera3D::Chase {
                pos: focus_pos + Vec3::new(0.0, 5.5, -12.0),
                target: focus_pos + Vec3::new(0.0, 1.0, 0.0),
                up: Vec3::Y,
                fov_deg: 55.0,
            };
        }
        ReplayCamMode::Tv => {
            if let Some(cam) = loaded.cameras.first() {
                loaded.camera_3d = Camera3D::TracksideTv {
                    pos: Vec3::from_array(cam.position),
                    target: focus_pos,
                    fov_deg: cam.max_fov,
                };
            } else {
                loaded.camera_3d = Camera3D::TracksideTv {
                    pos: focus_pos + Vec3::new(25.0, 12.0, 25.0),
                    target: focus_pos,
                    fov_deg: 40.0,
                };
            }
        }
        ReplayCamMode::FreeFly => {
            if !matches!(loaded.camera_3d, Camera3D::FreeFly { .. }) {
                loaded.camera_3d = Camera3D::FreeFly {
                    pos: focus_pos + Vec3::new(0.0, 8.0, -16.0),
                    yaw: 1.57,
                    pitch: -0.25,
                    speed: 25.0,
                };
            }
        }
    }

    let dt = ui.input(|i| i.stable_dt);
    let gl = ctx.gl.clone();
    widgets::viewport_3d_frame(
        ui,
        vec2(ui.available_width(), 320.0),
        |ui| {
            ui.horizontal(|ui| {
                widgets::pill(ui, "3D REPLAY", theme::CH_SPEED);
                for (m, lbl) in [
                    (ReplayCamMode::Chase, "Chase"),
                    (ReplayCamMode::FreeFly, "FreeFly"),
                    (ReplayCamMode::Tv, "TV"),
                ] {
                    ui.selectable_value(&mut loaded.cam_mode, m, lbl);
                }
            });
        },
        |ui, rect| {
            let resp = ui.interact(rect, ui.scope_id().with("replay_3d_vp"), Sense::click_and_drag());
            ui.input(|i| {
                if loaded.camera_3d.handle_input(&resp, i, dt) {
                    ui.ctx().request_repaint();
                }
            });
            let frame = SceneFrame {
                camera: loaded.camera_3d,
                render_mode: RenderMode::Shaded,
                lines,
                ..SceneFrame::default()
            };
            viewport3d::paint_viewport_3d(ui, rect, gl, frame, |ui, r| {
                ui.painter().rect_filled(r, 0, theme::SURFACE_VOID);
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "3D Replay Viewport (2D fallback in headless mode)",
                    egui::FontId::proportional(13.5),
                    theme::MUTED,
                );
            });
        },
    );
}

fn trim_clip_card(
    ui: &mut egui::Ui,
    loaded: &mut Loaded,
    busy: bool,
) -> Option<(PathBuf, PathBuf, u32, u32)> {
    let total = loaded.player.samples().len() as u32;
    if total < 2 {
        return None;
    }
    let mut request = None;
    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Trim & Export Clip");
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Slider::new(&mut loaded.trim_start, 0..=total.saturating_sub(2)).text("Start frame"));
            if loaded.trim_end <= loaded.trim_start {
                loaded.trim_end = (loaded.trim_start + 1).min(total);
            }
            ui.add(egui::Slider::new(&mut loaded.trim_end, (loaded.trim_start + 1)..=total).text("End frame"));
            if ui
                .add_enabled(!busy && loaded.trim_start < loaded.trim_end, egui::Button::new("Export Clip (.acreplay)"))
                .clicked()
            {
                let stem = loaded
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "clip".into());
                if let Some(dst) = rfd::FileDialog::new()
                    .add_filter("AC Replay", &["acreplay"])
                    .set_file_name(format!("{stem}_clip.acreplay"))
                    .save_file()
                {
                    request = Some((loaded.path.clone(), dst, loaded.trim_start, loaded.trim_end));
                }
            }
        });
    });
    request
}

/// The game's saved and autosave folders.
fn replay_dir() -> Option<PathBuf> {
    acxs_manager::settings::ac_documents_dir().map(|docs| docs.join("replay"))
}

fn read_library() -> Vec<ReplayFile> {
    replay_dir()
        .map(|dir| replay_library::list(&dir, &acxs_manager::session_recorder::replay_archive_dir()))
        .unwrap_or_default()
}

/// One row per replay. Returns the replay to open.
fn library_rows(ui: &mut egui::Ui, files: &[ReplayFile], notices: &mut Vec<Notice>, reread: &mut bool) -> Option<PathBuf> {
    let mut open = None;
    egui::ScrollArea::vertical().id_salt("replay_library").max_height(280.0).auto_shrink([false, true]).show(ui, |ui| {
        for file in files {
            ui.horizontal(|ui| {
                widgets::pill(ui, file.place.label(), if file.place == ReplayPlace::Autosave { theme::WARN } else { theme::MUTED });
                if file.online {
                    widgets::pill(ui, "Online", theme::INFO);
                }
                ui.add(egui::Label::new(file.title.as_str()).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Open").clicked() {
                        open = Some(file.path.clone());
                    }
                    if file.place != ReplayPlace::Saved
                        && ui.small_button("Keep").on_hover_text("Copy it to the saved folder, where the game leaves it alone.").clicked()
                        && let Some(dir) = replay_dir()
                    {
                        match replay_library::keep(file, &dir) {
                            Ok(_) => notices.push(Notice { text: "Replay kept.".into(), ok: true }),
                            Err(err) => notices.push(Notice { text: format!("Could not keep the replay: {err}"), ok: false }),
                        }
                        *reread = true;
                    }
                    let when: chrono::DateTime<chrono::Local> = file.modified.into();
                    ui.label(RichText::new(format!("{}  \u{00B7}  {} MB", when.format("%-d %b %H:%M"), file.bytes / 1_000_000)).size(12.0).color(theme::MUTED));
                });
            });
        }
    });
    open
}

/// Play/pause, speed selector, seek slider and the readout of the sample at the playhead.
fn playback(ui: &mut egui::Ui, loaded: &mut Loaded) {
    let capped = loaded.capped;
    let player = &mut loaded.player;
    let duration = player.duration_s();

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Playback");
        if player.samples().is_empty() {
            ui.label(RichText::new("No telemetry samples in this replay.").color(theme::MUTED));
            return;
        }

        ui.horizontal(|ui| {
            let label = if player.is_playing() { "Pause" } else { "Play" };
            if ui.add(widgets::primary_button(label)).clicked() {
                if player.is_playing() {
                    player.pause();
                } else {
                    player.play();
                }
            }
            ui.add_space(12.0);
            ui.label(RichText::new("Speed").color(theme::MUTED));
            let mut speed = player.speed();
            egui::ComboBox::from_id_salt("replay_speed")
                .selected_text(format_speed(speed))
                .show_ui(ui, |ui| {
                    for choice in SPEEDS {
                        ui.selectable_value(&mut speed, choice, format_speed(choice));
                    }
                });
            player.set_speed(speed);
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let seekable = duration > 0.0;
            let max = if seekable { duration } else { 1.0 };
            let mut position = player.position_s();
            ui.label(RichText::new(format_duration_ms(position)).monospace());
            ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(120.0);
            let slider = egui::Slider::new(&mut position, 0.0..=max).show_value(false);
            if ui.add_enabled(seekable, slider).changed() {
                player.seek(position);
            }
            ui.label(RichText::new(format_duration_ms(duration)).monospace());
        });

        ui.add_space(8.0);
        match player.current_sample() {
            Some(sample) => sample_readout(ui, sample),
            None => {
                ui.label(RichText::new("No sample at the playhead.").color(theme::MUTED));
            }
        }

        if capped {
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "This file has more frames than are stored, so playback ends at the last stored sample.",
                )
                .small()
                .color(theme::MUTED),
            );
        }
    });
}

/// Rows for the telemetry values of one sample.
fn sample_readout(ui: &mut egui::Ui, sample: &Sample) {
    widgets::key_value(ui, "Speed", &format!("{:.1} km/h", sample.speed_kmh));
    widgets::key_value(ui, "Gear", &gear_label(sample.gear));
    widgets::key_value(ui, "RPM", &format!("{:.0}", sample.rpm));
    widgets::key_value(ui, "Throttle", &format!("{:.0}%", sample.throttle * 100.0));
    widgets::key_value(ui, "Brake", &format!("{:.0}%", sample.brake * 100.0));
    widgets::key_value(
        ui,
        "Position",
        &format!("x {:.1}, y {:.1}, z {:.1} m", sample.x, sample.y, sample.z),
    );
}

/// Converts the replay's lap times into the rows the telemetry CSV writer expects.
/// Replay files carry no numeric car ID, so `car_id` is 0. Only the first car is covered.
fn lap_rows(info: &ReplayInfo) -> Vec<LapInfo> {
    info.lap_times_ms
        .iter()
        .enumerate()
        .map(|(i, &lap_time_ms)| LapInfo {
            car_id: 0,
            lap_time_ms,
            laps: u16::try_from(i + 1).unwrap_or(u16::MAX),
            driver: info.driver.clone(),
            car: info.car.clone(),
        })
        .collect()
}

fn summary(ui: &mut egui::Ui, path: &Path, info: &ReplayInfo, samples: &[Sample]) {
    let found = replay_analysis::analyze(samples);
    let track = match &info.config_track {
        Some(config) => format!("{} ({config})", info.track),
        None => info.track.clone(),
    };
    let weather = if info.weather.is_empty() { "-" } else { info.weather.as_str() };
    let best = info.lap_times_ms.iter().min().copied();

    theme::card().show(ui, |ui| {
        ui.label(RichText::new(file_name(path)).strong());
        ui.add_space(6.0);
        widgets::key_value(ui, "Track", &track);
        widgets::key_value(ui, "Car", &info.car);
        widgets::key_value(ui, "Driver", &info.driver);
        widgets::key_value(ui, "Laps", &info.lap_count.to_string());
        widgets::key_value(ui, "Duration", &format_duration_ms(info.duration_s));
        widgets::key_value(ui, "Samples", &info.sample_count.to_string());
        widgets::key_value(ui, "Weather", weather);
        widgets::key_value(ui, "Cars in replay", &info.num_cars.to_string());
        widgets::key_value(ui, "Distance", &format!("{:.1} km", found.distance_km));
        widgets::key_value(ui, "Top / average speed", &format!("{:.0} / {:.0} km/h", found.max_speed, found.average_speed));
        widgets::key_value(ui, "Full throttle", &format!("{:.0}%", found.full_throttle_share * 100.0));
        widgets::key_value(ui, "Hard braking", &format!("{:.0} s", found.hard_braking_s));
        widgets::key_value(ui, "Impacts / drops", &format!("{} / {}", found.impacts, found.drops));
    });
    ui.add_space(12.0);

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Lap times");
        if info.lap_times_ms.is_empty() {
            ui.label(RichText::new("No completed laps recorded.").color(theme::MUTED));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("replay_lap_table")
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("replay_laps")
                    .num_columns(2)
                    .striped(true)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Lap").color(theme::MUTED));
                        ui.label(RichText::new("Time").color(theme::MUTED));
                        ui.end_row();
                        for (i, &ms) in info.lap_times_ms.iter().enumerate() {
                            ui.label((i + 1).to_string());
                            let color = if Some(ms) == best { theme::OK } else { theme::TEXT };
                            ui.label(RichText::new(format_lap_time(ms)).monospace().color(color));
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Export laps CSV writes this lap table, and Export samples CSV writes one row \
                 per telemetry sample. Both cover the first car only. car_id is 0 because \
                 replays carry no numeric car ID.",
            )
            .small()
            .color(theme::MUTED),
        );
    });
}

/// Speed multiplier as shown in the selector, e.g. `0.5x`.
fn format_speed(multiplier: f64) -> String {
    format!("{multiplier}x")
}

/// Gear label from the raw gear code: 0 = reverse, 1 = neutral, 2 and up = forward gears.
fn gear_label(gear: u8) -> String {
    match gear {
        0 => "R".to_owned(),
        1 => "N".to_owned(),
        n => (n - 1).to_string(),
    }
}

/// Formats a duration in seconds as `M:SS.mmm`, the same style as lap times.
fn format_duration_ms(seconds: f64) -> String {
    let ms = (seconds * 1000.0).round().clamp(0.0, f64::from(u32::MAX));
    format_lap_time(ms as u32)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
