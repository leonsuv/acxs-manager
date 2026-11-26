//! Trajectory & Racing-Line Studio: overlays recorded 30 Hz laps (`LapTrace`) or
//! `.acreplay` laps against `fast_lane.ai` with braking/apex/throttle/delta analysis,
//! interactive 2D corridor view, optional 3D ribbon viewport, synced telemetry traces,
//! and a corner-by-corner time-loss table.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED};
use crate::ui::viewport3d::{self, Camera3D, DrawLines, LineVertex, RenderMode, SceneFrame};
use crate::ui::{widgets, Notice};
use acxs_manager::ai_spline::{self, AiSpline};
use acxs_manager::content::{self, TrackEntry};
use acxs_manager::replay::{self, MultiCarReplay};
use acxs_manager::session_recorder::{self, LapTrace};
use acxs_manager::track_files::{self, TrackSection};
use acxs_manager::trajectory::{self, TrajectoryAnalysis, TrajectoryLap, TrajectoryPoint};
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use glam::Vec3;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(TrajectoryStudio {
        tx,
        rx,
        root: None,
        tracks: Vec::new(),
        selected_track: None,
        spline: None,
        sections: Vec::new(),
        laps: Vec::new(),
        target_lap: None,
        ref_lap: None,
        analysis: None,
        view_mode: ViewMode::Corridor2D,
        color_mode: ColorMode::Pedals,
        zoom_2d: 1.0,
        pan_2d: egui::Vec2::ZERO,
        camera_3d: Camera3D::default(),
        busy: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Corridor2D,
    Ribbon3D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorMode {
    Pedals,
    Speed,
    Delta,
}

enum Msg {
    Tracks(Vec<TrackEntry>),
    TrackLoaded {
        spline: Result<AiSpline, String>,
        sections: Vec<TrackSection>,
    },
    LapsLoaded(Result<Vec<TrajectoryLap>, String>),
}

struct TrajectoryStudio {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    root: Option<PathBuf>,
    tracks: Vec<TrackEntry>,
    selected_track: Option<usize>,
    spline: Option<AiSpline>,
    sections: Vec<TrackSection>,
    laps: Vec<TrajectoryLap>,
    target_lap: Option<usize>,
    ref_lap: Option<usize>,
    analysis: Option<TrajectoryAnalysis>,
    view_mode: ViewMode,
    color_mode: ColorMode,
    zoom_2d: f32,
    pan_2d: egui::Vec2,
    camera_3d: Camera3D,
    busy: bool,
}

impl TrajectoryStudio {
    fn recompute(&mut self) {
        let (Some(spline), Some(t_idx)) = (&self.spline, self.target_lap) else {
            self.analysis = None;
            return;
        };
        let Some(target) = self.laps.get(t_idx) else {
            self.analysis = None;
            return;
        };
        let reference = self.ref_lap.and_then(|i| self.laps.get(i));
        self.analysis = Some(trajectory::analyze_trajectory(
            spline,
            target,
            reference,
            &self.sections,
        ));
    }

    fn load_track(&mut self, track_dir: PathBuf, ctx: &egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let spline_path = track_dir.join("ai").join("fast_lane.ai");
            let spline = ai_spline::read_ai_spline(&spline_path).map_err(|e| e.to_string());
            let sections = track_files::read_sections(&track_dir.join("data").join("sections.ini"))
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| track_files::read_sections(&track_dir.join("ui").join("sections.ini")).ok())
                .unwrap_or_default();
            let _ = tx.send(Msg::TrackLoaded { spline, sections });
            repaint.request_repaint();
        });
    }

    fn load_recorded_traces(&mut self, ctx: &egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let res = read_all_recorded_traces();
            let _ = tx.send(Msg::LapsLoaded(res));
            repaint.request_repaint();
        });
    }

    fn load_replay_laps(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let res = replay::read_replay_multicar(&path, 250_000)
                .map(|mc| laps_from_replay(&mc))
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::LapsLoaded(res));
            repaint.request_repaint();
        });
    }

    fn poll(&mut self, notices: &mut Vec<Notice>) {
        let mut need_recompute = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Tracks(tracks) => self.tracks = tracks,
                Msg::TrackLoaded { spline, sections } => {
                    self.busy = false;
                    self.sections = sections;
                    match spline {
                        Ok(sp) => {
                            if let Some(mid) = sp.points.first() {
                                self.camera_3d = Camera3D::Orbit {
                                    target: Vec3::new(mid.x, mid.y, mid.z),
                                    yaw: 0.7,
                                    pitch: 0.85,
                                    dist: 240.0,
                                };
                            }
                            self.spline = Some(sp);
                            self.zoom_2d = 1.0;
                            self.pan_2d = egui::Vec2::ZERO;
                            need_recompute = true;
                        }
                        Err(e) => {
                            self.spline = None;
                            notices.push(Notice {
                                text: format!("Could not read fast_lane.ai: {e}"),
                                ok: false,
                            });
                        }
                    }
                }
                Msg::LapsLoaded(Ok(new_laps)) => {
                    self.busy = false;
                    if new_laps.is_empty() {
                        notices.push(Notice {
                            text: "No laps with position samples found.".into(),
                            ok: false,
                        });
                    } else {
                        let count = new_laps.len();
                        self.laps = new_laps;
                        self.target_lap = Some(0);
                        self.ref_lap = (count > 1).then_some(1);
                        need_recompute = true;
                        notices.push(Notice {
                            text: format!("Loaded {count} lap(s) for trajectory comparison."),
                            ok: true,
                        });
                    }
                }
                Msg::LapsLoaded(Err(e)) => {
                    self.busy = false;
                    notices.push(Notice {
                        text: format!("Could not load laps: {e}"),
                        ok: false,
                    });
                }
            }
        }
        if need_recompute {
            self.recompute();
        }
    }
}

impl Tool for TrajectoryStudio {
    fn name(&self) -> &'static str {
        "Trajectory Studio"
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn content_width(&self) -> f32 {
        f32::INFINITY
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        if let Some(root) = ctx.ac_root.clone().filter(|_| ctx.ac_valid)
            && self.root.as_ref() != Some(&root)
        {
            self.root = Some(root.clone());
            let tx = self.tx.clone();
            let repaint = ui.ctx().clone();
            thread::spawn(move || {
                let _ = tx.send(Msg::Tracks(content::scan_tracks(&root)));
                repaint.request_repaint();
            });
        }

        widgets::heading(
            ui,
            "Trajectory Studio",
            "Overlay recorded 30 Hz laps or .acreplay traces against fast_lane.ai with corner-by-corner deltas.",
        );

        // Controls bar
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                // Track picker
                let track_label = self
                    .selected_track
                    .and_then(|i| self.tracks.get(i))
                    .map(|t| t.name.as_str())
                    .unwrap_or("Select track...");
                let mut picked_track = None;
                egui::ComboBox::from_id_salt("traj_track")
                    .selected_text(track_label)
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (i, t) in self.tracks.iter().enumerate() {
                            if ui.selectable_label(self.selected_track == Some(i), &t.name).clicked() {
                                picked_track = Some(i);
                            }
                        }
                    });
                if let Some(idx) = picked_track {
                    self.selected_track = Some(idx);
                    let dir = self.tracks[idx].path.clone();
                    self.load_track(dir, ui.ctx());
                }

                if ui.add_enabled(!self.busy, egui::Button::new("Load Recorded Traces")).clicked() {
                    self.load_recorded_traces(ui.ctx());
                }
                if ui.add_enabled(!self.busy, egui::Button::new("Open .acreplay...")).clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("AC Replay", &["acreplay"])
                        .pick_file()
                {
                    self.load_replay_laps(path, ui.ctx());
                }

                ui.separator();
                widgets::animated_tabs(
                    ui,
                    "traj_view",
                    &mut self.view_mode,
                    &[
                        (ViewMode::Corridor2D, "2D Corridor", None),
                        (ViewMode::Ribbon3D, "3D Ribbon", None),
                    ],
                );
                ui.separator();
                for (m, lbl) in [
                    (ColorMode::Pedals, "Pedals"),
                    (ColorMode::Speed, "Speed"),
                    (ColorMode::Delta, "Delta"),
                ] {
                    ui.selectable_value(&mut self.color_mode, m, lbl);
                }
                if self.busy {
                    ui.spinner();
                }
            });

            if !self.laps.is_empty() {
                ui.add_space(6.0);
                let mut changed = false;
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Target lap:").color(MUTED));
                    let t_label = self
                        .target_lap
                        .and_then(|i| self.laps.get(i))
                        .map(|l| l.label.as_str())
                        .unwrap_or("None");
                    egui::ComboBox::from_id_salt("traj_target_lap")
                        .selected_text(t_label)
                        .show_ui(ui, |ui| {
                            for (i, lap) in self.laps.iter().enumerate() {
                                if ui.selectable_label(self.target_lap == Some(i), &lap.label).clicked() {
                                    self.target_lap = Some(i);
                                    changed = true;
                                }
                            }
                        });

                    ui.add_space(8.0);
                    ui.label(RichText::new("Reference lap:").color(MUTED));
                    let r_label = self
                        .ref_lap
                        .and_then(|i| self.laps.get(i))
                        .map(|l| l.label.as_str())
                        .unwrap_or("None (AI line only)");
                    egui::ComboBox::from_id_salt("traj_ref_lap")
                        .selected_text(r_label)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(self.ref_lap.is_none(), "None (AI line only)").clicked() {
                                self.ref_lap = None;
                                changed = true;
                            }
                            for (i, lap) in self.laps.iter().enumerate() {
                                if ui.selectable_label(self.ref_lap == Some(i), &lap.label).clicked() {
                                    self.ref_lap = Some(i);
                                    changed = true;
                                }
                            }
                        });

                    if let Some(total) = self.analysis.as_ref().and_then(|a| a.total_delta_ms) {
                        let col = if total <= 0.0 { theme::OK } else { theme::CH_BRAKE };
                        widgets::pill(ui, &format!("Total Δ {:+.3} s", total / 1000.0), col);
                    }
                });
                if changed {
                    self.recompute();
                }
            }
        });

        ui.add_space(8.0);

        // Main viewport (2D corridor or 3D ribbon)
        let avail_w = ui.available_width().max(320.0);
        let vp_h = 380.0;
        let color_mode = self.color_mode;
        if self.view_mode == ViewMode::Ribbon3D {
            let mut frame = SceneFrame {
                camera: self.camera_3d,
                render_mode: RenderMode::Shaded,
                ..SceneFrame::default()
            };
            if let Some(sp) = &self.spline {
                frame.lines.extend(build_3d_ribbons(sp, self.analysis.as_ref(), color_mode));
            }
            let dt = ui.input(|i| i.stable_dt);
            let gl = ctx.gl.clone();
            widgets::viewport_3d_frame(
                ui,
                vec2(avail_w, vp_h),
                |ui| {
                    ui.horizontal(|ui| {
                        widgets::pill(ui, "3D RIBBON", theme::CH_SPEED);
                        let badge = if ctx.gl.is_some() { "GPU" } else { "2D FALLBACK" };
                        widgets::pill(ui, badge, MUTED);
                    });
                },
                |ui, rect| {
                    let resp = ui.interact(rect, ui.scope_id().with("traj_3d"), Sense::click_and_drag());
                    ui.input(|i| {
                        if self.camera_3d.handle_input(&resp, i, dt) {
                            ui.ctx().request_repaint();
                        }
                    });
                    frame.camera = self.camera_3d;
                    let sp_ref = self.spline.as_ref();
                    let an_ref = self.analysis.as_ref();
                    viewport3d::paint_viewport_3d(ui, rect, gl, frame, |ui, r| {
                        paint_corridor_2d(ui, r, sp_ref, an_ref, color_mode, 1.0, egui::Vec2::ZERO);
                    });
                },
            );
        } else {
            let mut reset_zoom = false;
            let mut zoom_2d = self.zoom_2d;
            let mut pan_2d = self.pan_2d;
            widgets::viewport_3d_frame(
                ui,
                vec2(avail_w, vp_h),
                |ui| {
                    ui.horizontal(|ui| {
                        widgets::pill(ui, "2D CORRIDOR", theme::CH_SPEED);
                        if ui.small_button("Reset zoom").clicked() {
                            reset_zoom = true;
                        }
                    });
                },
                |ui, rect| {
                    let resp = ui.interact(rect, ui.scope_id().with("traj_2d"), Sense::click_and_drag());
                    if resp.dragged() {
                        pan_2d += resp.drag_delta();
                    }
                    if resp.hovered() {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll.abs() > 0.01 {
                            zoom_2d = (zoom_2d * (1.0 + scroll * 0.002)).clamp(0.5, 20.0);
                        }
                    }
                    paint_corridor_2d(
                        ui,
                        rect,
                        self.spline.as_ref(),
                        self.analysis.as_ref(),
                        color_mode,
                        zoom_2d,
                        pan_2d,
                    );
                },
            );
            if reset_zoom {
                self.zoom_2d = 1.0;
                self.pan_2d = egui::Vec2::ZERO;
            } else {
                self.zoom_2d = zoom_2d;
                self.pan_2d = pan_2d;
            }
        }

        // Synced telemetry traces & corner table
        if let Some(analysis) = &self.analysis {
            ui.add_space(10.0);
            paint_telemetry_traces(ui, analysis);
            ui.add_space(10.0);
            paint_corner_table(ui, analysis);
        }
    }
}

fn read_all_recorded_traces() -> Result<Vec<TrajectoryLap>, String> {
    let dir = session_recorder::traces_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(trace) = serde_json::from_str::<LapTrace>(&text) else { continue };
        let points: Vec<TrajectoryPoint> = trace
            .bins
            .iter()
            .filter_map(|b| *b)
            .map(|b| TrajectoryPoint {
                x: b.x,
                z: b.z,
                speed_kmh: b.speed_kmh,
                throttle: b.throttle,
                brake: b.brake,
                gear: b.gear,
                elapsed_ms: f64::from(b.elapsed_ms),
            })
            .collect();
        if points.len() >= 10 {
            let secs = trace.lap_time_ms as f64 / 1000.0;
            out.push(TrajectoryLap {
                label: format!("{} @ {} ({secs:.3}s)", trace.car_id, trace.track_id),
                points,
            });
        }
    }
    Ok(out)
}

fn laps_from_replay(mc: &MultiCarReplay) -> Vec<TrajectoryLap> {
    let mut out = Vec::new();
    for (ci, car) in mc.cars.iter().enumerate() {
        let mut by_lap: std::collections::BTreeMap<u8, Vec<TrajectoryPoint>> =
            std::collections::BTreeMap::new();
        for f in &car.frames {
            by_lap.entry(f.current_lap).or_default().push(TrajectoryPoint {
                x: f.sample.x,
                z: f.sample.z,
                speed_kmh: f.sample.speed_kmh,
                throttle: f.sample.throttle,
                brake: f.sample.brake,
                gear: f.sample.gear,
                elapsed_ms: f.sample.time_s * 1000.0,
            });
        }
        for (lap_num, mut pts) in by_lap {
            if pts.len() < 15 {
                continue;
            }
            let t0 = pts[0].elapsed_ms;
            for p in &mut pts {
                p.elapsed_ms -= t0;
            }
            let dur = pts.last().map_or(0.0, |p| p.elapsed_ms) / 1000.0;
            out.push(TrajectoryLap {
                label: format!("Car {} ({}) Lap {} ({dur:.2}s)", ci + 1, car.driver, lap_num + 1),
                points: pts,
            });
        }
    }
    out
}

fn sample_color(p: &trajectory::ProjectedPoint, mode: ColorMode) -> Color32 {
    match mode {
        ColorMode::Pedals => {
            if p.point.brake > 0.2 {
                theme::CH_BRAKE
            } else if p.point.throttle > 0.75 {
                theme::CH_THROTTLE
            } else {
                theme::CH_STEER
            }
        }
        ColorMode::Speed => {
            let t = (p.point.speed_kmh / 260.0).clamp(0.0, 1.0);
            Color32::from_rgb((255.0 * (1.0 - t)) as u8, (210.0 * t) as u8, 255)
        }
        ColorMode::Delta => match p.delta_ms {
            Some(d) if d <= -50.0 => theme::OK,
            Some(d) if d >= 50.0 => theme::CH_BRAKE,
            _ => theme::CH_SPEED,
        },
    }
}

fn color32_to_rgba(c: Color32) -> [f32; 4] {
    [
        f32::from(c.r()) / 255.0,
        f32::from(c.g()) / 255.0,
        f32::from(c.b()) / 255.0,
        1.0,
    ]
}

fn build_3d_ribbons(
    spline: &AiSpline,
    analysis: Option<&TrajectoryAnalysis>,
    mode: ColorMode,
) -> Vec<DrawLines> {
    let mut lines = Vec::new();
    let (left, right) = spline.corridor_edges();
    let edge_col = [0.35, 0.38, 0.45, 0.9];
    lines.push(DrawLines {
        vertices: left.iter().map(|&pos| LineVertex { pos, color: edge_col }).collect(),
        strip: true,
    });
    lines.push(DrawLines {
        vertices: right.iter().map(|&pos| LineVertex { pos, color: edge_col }).collect(),
        strip: true,
    });
    if let Some(an) = analysis {
        let verts: Vec<LineVertex> = an
            .projected
            .iter()
            .map(|p| {
                let y = spline
                    .project_world(p.point.x, p.point.z)
                    .and_then(|pr| spline.points.get(pr.index))
                    .map_or(0.5, |sp| sp.y + 0.6);
                LineVertex {
                    pos: [p.point.x, y, p.point.z],
                    color: color32_to_rgba(sample_color(p, mode)),
                }
            })
            .collect();
        if verts.len() >= 2 {
            lines.push(DrawLines { vertices: verts, strip: true });
        }
    }
    lines
}

fn paint_corridor_2d(
    ui: &mut egui::Ui,
    rect: Rect,
    spline: Option<&AiSpline>,
    analysis: Option<&TrajectoryAnalysis>,
    mode: ColorMode,
    zoom: f32,
    pan: egui::Vec2,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, theme::SURFACE_VOID);
    let Some(sp) = spline else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Select a track with fast_lane.ai and load a recorded trace or .acreplay",
            egui::FontId::proportional(14.0),
            MUTED,
        );
        return;
    };
    if sp.points.len() < 2 {
        return;
    }

    let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut min_z, mut max_z) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in &sp.points {
        min_x = min_x.min(p.x);
        max_x = max_x.max(p.x);
        min_z = min_z.min(p.z);
        max_z = max_z.max(p.z);
    }
    let span = (max_x - min_x).max(max_z - min_z).max(1.0);
    let pad = 28.0;
    let base_scale = ((rect.width() - pad * 2.0).min(rect.height() - pad * 2.0) / span).max(1e-4);
    let scale = base_scale * zoom;
    let cx = 0.5 * (min_x + max_x);
    let cz = 0.5 * (min_z + max_z);
    let to_screen = |x: f32, z: f32| -> Pos2 {
        pos2(
            rect.center().x + (x - cx) * scale + pan.x,
            rect.center().y + (z - cz) * scale + pan.y,
        )
    };

    let (left, right) = sp.corridor_edges();
    let left_pts: Vec<Pos2> = left.iter().map(|p| to_screen(p[0], p[2])).collect();
    let right_pts: Vec<Pos2> = right.iter().map(|p| to_screen(p[0], p[2])).collect();
    let ideal_pts: Vec<Pos2> = sp.points.iter().map(|p| to_screen(p.x, p.z)).collect();
    painter.add(egui::Shape::line(left_pts, Stroke::new(1.2, Color32::from_gray(95))));
    painter.add(egui::Shape::line(right_pts, Stroke::new(1.2, Color32::from_gray(95))));
    painter.add(egui::Shape::line(ideal_pts, Stroke::new(1.0, Color32::from_rgba_unmultiplied(160, 175, 200, 90))));

    if let Some(an) = analysis {
        if an.reference_projected.len() >= 2 {
            let ref_pts: Vec<Pos2> = an
                .reference_projected
                .iter()
                .map(|p| to_screen(p.point.x, p.point.z))
                .collect();
            painter.add(egui::Shape::line(
                ref_pts,
                Stroke::new(1.6, Color32::from_rgba_unmultiplied(255, 255, 255, 110)),
            ));
        }
        for pair in an.projected.windows(2) {
            let a = to_screen(pair[0].point.x, pair[0].point.z);
            let b = to_screen(pair[1].point.x, pair[1].point.z);
            painter.line_segment([a, b], Stroke::new(2.6, sample_color(&pair[1], mode)));
        }
    }
}

fn paint_telemetry_traces(ui: &mut egui::Ui, analysis: &TrajectoryAnalysis) {
    if analysis.projected.len() < 2 {
        return;
    }
    theme::card_elevated().show(ui, |ui| {
        widgets::section_title(ui, "Synced Telemetry Traces");
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, CornerRadius::same(6), theme::SURFACE_VOID);
        let n = analysis.projected.len();
        let max_spd = analysis
            .projected
            .iter()
            .map(|s| s.point.speed_kmh)
            .fold(100.0f32, f32::max);
        let map_pt = |i: usize, val: f32| -> Pos2 {
            let x = rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
            let y = rect.bottom() - val.clamp(0.0, 1.0) * (rect.height() - 12.0) - 6.0;
            pos2(x, y)
        };
        let spd: Vec<Pos2> = analysis
            .projected
            .iter()
            .enumerate()
            .map(|(i, s)| map_pt(i, s.point.speed_kmh / max_spd))
            .collect();
        let thr: Vec<Pos2> = analysis
            .projected
            .iter()
            .enumerate()
            .map(|(i, s)| map_pt(i, s.point.throttle))
            .collect();
        let brk: Vec<Pos2> = analysis
            .projected
            .iter()
            .enumerate()
            .map(|(i, s)| map_pt(i, s.point.brake))
            .collect();
        p.add(egui::Shape::line(spd, Stroke::new(1.8, theme::CH_SPEED)));
        p.add(egui::Shape::line(thr, Stroke::new(1.4, theme::CH_THROTTLE)));
        p.add(egui::Shape::line(brk, Stroke::new(1.4, theme::CH_BRAKE)));
        ui.horizontal(|ui| {
            widgets::pill(ui, "SPEED", theme::CH_SPEED);
            widgets::pill(ui, "THROTTLE", theme::CH_THROTTLE);
            widgets::pill(ui, "BRAKE", theme::CH_BRAKE);
        });
    });
}

fn paint_corner_table(ui: &mut egui::Ui, analysis: &TrajectoryAnalysis) {
    theme::card_elevated().show(ui, |ui| {
        widgets::section_title(ui, &format!("Corner-by-Corner Breakdown ({})", analysis.corners.len()));
        if analysis.corners.is_empty() {
            ui.label(RichText::new("No corners segmented on this track.").color(MUTED));
            return;
        }
        egui::Grid::new("traj_corners_grid")
            .num_columns(6)
            .striped(true)
            .spacing([18.0, 6.0])
            .show(ui, |ui| {
                for h in ["Corner", "Brake In", "Min Speed", "Apex Margin", "Throttle On", "Delta"] {
                    ui.label(RichText::new(h).font(theme::display(12.5)).color(MUTED));
                }
                ui.end_row();
                for c in &analysis.corners {
                    ui.label(RichText::new(&c.name).strong());
                    ui.label(
                        RichText::new(
                            c.braking_norm
                                .map(|v| format!("{:.1}%", v * 100.0))
                                .unwrap_or_else(|| "—".into()),
                        )
                        .font(theme::mono(12.5)),
                    );
                    ui.label(
                        RichText::new(format!("{:.1} km/h", c.min_speed_kmh))
                            .font(theme::mono(12.5)),
                    );
                    ui.label(
                        RichText::new(
                            c.apex_margin_m
                                .map(|m| format!("{m:.2} m"))
                                .unwrap_or_else(|| "—".into()),
                        )
                        .font(theme::mono(12.5)),
                    );
                    ui.label(
                        RichText::new(
                            c.throttle_on_norm
                                .map(|v| format!("{:.1}%", v * 100.0))
                                .unwrap_or_else(|| "—".into()),
                        )
                        .font(theme::mono(12.5)),
                    );
                    let (txt, col) = match c.time_delta_ms {
                        Some(ms) => (
                            format!("{:+.3} s", ms / 1000.0),
                            if ms <= 0.0 { theme::OK } else { theme::CH_BRAKE },
                        ),
                        None => ("—".into(), MUTED),
                    };
                    ui.label(RichText::new(txt).font(theme::mono(12.5)).color(col));
                    ui.end_row();
                }
            });
    });
}
