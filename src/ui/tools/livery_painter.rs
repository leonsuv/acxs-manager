//! Livery Painter tool page: paint on a skin's textures and save the result as a new skin.
//!
//! The game's skin is never written: saving copies the skin folder first.

use super::{Tool, ToolContext};
use crate::ui::viewport3d::{self, Camera3D, CpuSubMesh, DrawItem, MeshVertex, RenderMode, SceneFrame};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::content::{self, CarEntry};
use acxs_manager::dds::{self, Rgba};
use acxs_manager::kn5_scene::{self, LoadOptions};
use acxs_manager::livery_paint::{self, Canvas};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, pos2, vec2};
use glam::Mat4;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy, PartialEq, Eq)]
enum DdsExportFormat {
    Dxt1,
    Dxt5,
}

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(Painter {
        tx,
        rx,
        root: None,
        cars: Vec::new(),
        car: None,
        skin: None,
        skins: Vec::new(),
        textures: Vec::new(),
        texture: None,
        canvas: None,
        view: None,
        dirty: false,
        changed: false,
        tool: Brush::Paint,
        color: Color32::from_rgb(255, 122, 26),
        radius: 6.0,
        zoom: 1.0,
        decal: None,
        decal_scale: 1.0,
        last: None,
        drag_start: None,
        busy: false,
        export_fmt: DdsExportFormat::Dxt5,
        show_3d: false,
        cam_3d: Camera3D::default(),
        mesh_3d: None,
        uploaded_3d: None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Brush {
    Paint,
    Fill,
    Rectangle,
    Stamp,
}

enum Msg {
    Cars(Vec<CarEntry>),
    Texture(PathBuf, Result<Rgba, String>),
    Decal(Option<Result<Rgba, String>>),
    Mesh3D(PathBuf, Result<Vec<CpuSubMesh>, String>),
}

struct Painter {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    root: Option<PathBuf>,
    cars: Vec<CarEntry>,
    car: Option<usize>,
    skin: Option<String>,
    skins: Vec<String>,
    textures: Vec<PathBuf>,
    texture: Option<PathBuf>,
    canvas: Option<Canvas>,
    view: Option<TextureHandle>,
    /// The picture changed since it was last sent to the screen.
    dirty: bool,
    /// The picture has edits that are not saved.
    changed: bool,
    tool: Brush,
    color: Color32,
    radius: f32,
    zoom: f32,
    decal: Option<Rgba>,
    decal_scale: f32,
    /// The pointer's last position in a stroke, in picture pixels.
    last: Option<(f32, f32)>,
    drag_start: Option<(i32, i32)>,
    busy: bool,
    export_fmt: DdsExportFormat,
    show_3d: bool,
    cam_3d: Camera3D,
    mesh_3d: Option<((PathBuf, u64), Vec<CpuSubMesh>)>,
    uploaded_3d: Option<(PathBuf, u64)>,
}

fn folders(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> =
        entries.filter_map(Result::ok).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

impl Painter {
    fn skin_dir(&self) -> Option<PathBuf> {
        let car = &self.cars[self.car?];
        Some(car.path.join("skins").join(self.skin.as_ref()?))
    }

    fn pick_car(&mut self, index: usize) {
        self.car = Some(index);
        self.skins = folders(&self.cars[index].path.join("skins"));
        let first = self.skins.first().cloned();
        self.pick_skin(first);
    }

    fn pick_skin(&mut self, skin: Option<String>) {
        self.skin = skin;
        self.textures = self.skin_dir().map(|dir| livery_paint::textures(&dir)).unwrap_or_default();
        self.texture = None;
        self.canvas = None;
        self.view = None;
        self.changed = false;
    }

    fn open_texture(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.busy = true;
        self.texture = Some(path.clone());
        self.canvas = None;
        self.view = None;
        self.changed = false;
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = livery_paint::load(&path);
            let _ = tx.send(Msg::Texture(path, result));
            repaint.request_repaint();
        });
    }

    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Msg::Cars(cars) => self.cars = cars,
                Msg::Texture(path, result) => {
                    self.busy = false;
                    if self.texture.as_ref() != Some(&path) {
                        continue;
                    }
                    match result {
                        Ok(image) => {
                            self.canvas = Some(Canvas::new(image));
                            self.dirty = true;
                        }
                        Err(err) => notices.push(Notice { text: format!("Could not open the texture: {err}"), ok: false }),
                    }
                }
                Msg::Decal(Some(Ok(image))) => {
                    self.decal = Some(image);
                    self.tool = Brush::Stamp;
                }
                Msg::Decal(Some(Err(err))) => notices.push(Notice { text: format!("Could not open the picture: {err}"), ok: false }),
                Msg::Mesh3D(car_path, Ok(submeshes)) => {
                    self.mesh_3d = Some(((car_path, 0), submeshes));
                    self.uploaded_3d = None;
                }
                Msg::Mesh3D(_, Err(err)) => {
                    notices.push(Notice { text: format!("Could not load 3D car mesh: {err}"), ok: false });
                }
                Msg::Decal(None) => {}
            }
        }
    }

    fn pickers(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let car_name = self.car.map_or("Choose a car".to_string(), |i| self.cars[i].name.clone());
            let mut picked_car = None;
            egui::ComboBox::from_id_salt("paint_car").selected_text(car_name).width(240.0).height(420.0).show_ui(ui, |ui| {
                for (i, car) in self.cars.iter().enumerate() {
                    if ui.selectable_label(self.car == Some(i), &car.name).clicked() {
                        picked_car = Some(i);
                    }
                }
            });
            if let Some(i) = picked_car {
                self.pick_car(i);
            }
            let mut picked_skin = None;
            egui::ComboBox::from_id_salt("paint_skin")
                .selected_text(self.skin.clone().unwrap_or_else(|| "Skin".to_string()))
                .width(220.0)
                .show_ui(ui, |ui| {
                    for skin in &self.skins {
                        if ui.selectable_label(self.skin.as_ref() == Some(skin), skin).clicked() {
                            picked_skin = Some(skin.clone());
                        }
                    }
                });
            if let Some(skin) = picked_skin {
                self.pick_skin(Some(skin));
            }
            let shown = self.texture.as_ref().and_then(|p| p.file_name()).map_or("Texture".to_string(), |n| n.to_string_lossy().into_owned());
            let mut picked_texture = None;
            egui::ComboBox::from_id_salt("paint_texture").selected_text(shown).width(240.0).height(420.0).show_ui(ui, |ui| {
                for path in &self.textures {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if ui.selectable_label(self.texture.as_ref() == Some(path), name.as_ref()).clicked() {
                        picked_texture = Some(path.clone());
                    }
                }
            });
            if let Some(path) = picked_texture {
                self.open_texture(path, ctx);
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        ui.horizontal(|ui| {
            for (brush, label) in [(Brush::Paint, "Brush"), (Brush::Fill, "Fill"), (Brush::Rectangle, "Rectangle"), (Brush::Stamp, "Stamp")] {
                ui.selectable_value(&mut self.tool, brush, label);
            }
            ui.add_space(8.0);
            ui.color_edit_button_srgba(&mut self.color);
            ui.add(egui::Slider::new(&mut self.radius, 1.0..=120.0).text("Size"));
            ui.add(egui::Slider::new(&mut self.zoom, 0.1..=4.0).text("Zoom"));
        });
        ui.horizontal(|ui| {
            if ui.button("Load a picture to stamp").clicked() {
                let tx = self.tx.clone();
                let repaint = ui.ctx().clone();
                std::thread::spawn(move || {
                    let path = rfd::FileDialog::new().add_filter("Picture", &["png", "jpg", "jpeg"]).pick_file();
                    let _ = tx.send(Msg::Decal(path.map(|p| livery_paint::load_decal(&p))));
                    repaint.request_repaint();
                });
            }
            if self.decal.is_some() {
                ui.add(egui::Slider::new(&mut self.decal_scale, 0.05..=4.0).text("Stamp size"));
            }
            let can_undo = self.canvas.as_ref().is_some_and(Canvas::can_undo);
            if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked()
                && let Some(canvas) = &mut self.canvas
            {
                canvas.undo();
                self.dirty = true;
            }
            let can_save = self.changed && self.canvas.is_some();
            if ui
                .add_enabled(can_save, widgets::primary_button("Save as new skin"))
                .on_hover_text("Copies this skin and writes the painted texture there. The original stays.")
                .clicked()
            {
                self.save(notices);
            }
            ui.add_space(6.0);
            ui.selectable_value(&mut self.export_fmt, DdsExportFormat::Dxt1, "DXT1");
            ui.selectable_value(&mut self.export_fmt, DdsExportFormat::Dxt5, "DXT5");
            if ui
                .add_enabled(self.canvas.is_some(), egui::Button::new("Export DDS..."))
                .on_hover_text("Write a compressed DXT1 or DXT5 DDS texture with full mip chain.")
                .clicked()
            {
                self.export_compressed_dds(notices);
            }
            ui.add_space(6.0);
            if ui.selectable_label(self.show_3d, "3D Preview").clicked() {
                self.show_3d = !self.show_3d;
                if self.show_3d
                    && let Some(idx) = self.car
                    && let Some(car) = self.cars.get(idx)
                {
                    let car_path = car.path.clone();
                    let tx = self.tx.clone();
                    let repaint = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let res = load_painter_3d(&car_path);
                        let _ = tx.send(Msg::Mesh3D(car_path, res));
                        repaint.request_repaint();
                    });
                }
            }
            if self.changed {
                ui.label(RichText::new("Not saved").color(theme::WARN));
            }
        });
    }

    fn export_compressed_dds(&self, notices: &mut Vec<Notice>) {
        let Some(canvas) = &self.canvas else { return };
        let default_name = self
            .texture
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| format!("{}_dxt.dds", s.to_string_lossy()))
            .unwrap_or_else(|| "livery.dds".to_string());
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DDS", &["dds"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        if path.exists() {
            notices.push(Notice {
                text: format!("{} already exists. Choose another name.", path.display()),
                ok: false,
            });
            return;
        }
        let bytes = match self.export_fmt {
            DdsExportFormat::Dxt1 => dds::encode_dxt1(&canvas.image),
            DdsExportFormat::Dxt5 => dds::encode_dxt5(&canvas.image),
        };
        let label = match self.export_fmt {
            DdsExportFormat::Dxt1 => "DXT1",
            DdsExportFormat::Dxt5 => "DXT5",
        };
        match std::fs::write(&path, &bytes) {
            Ok(()) => notices.push(Notice {
                text: format!("Exported {label} DDS ({} KB) to {}", bytes.len() / 1024, path.display()),
                ok: true,
            }),
            Err(e) => notices.push(Notice {
                text: format!("Could not export DDS: {e}"),
                ok: false,
            }),
        }
    }

    fn save(&mut self, notices: &mut Vec<Notice>) {
        let (Some(dir), Some(texture), Some(canvas)) = (self.skin_dir(), self.texture.clone(), &self.canvas) else { return };
        let name = texture.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match livery_paint::save_as_new_skin(&dir, &name, &canvas.image) {
            Ok(target) => {
                let skin = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
                notices.push(Notice { text: format!("Saved as skin {skin}. Pick it in Quick Race or the Showroom."), ok: true });
                self.changed = false;
                let index = self.car;
                if let Some(index) = index {
                    self.skins = folders(&self.cars[index].path.join("skins"));
                }
                self.skin = Some(skin);
                self.textures = self.skin_dir().map(|d| livery_paint::textures(&d)).unwrap_or_default();
                self.texture = Some(target.join(&name));
            }
            Err(err) => notices.push(Notice { text: format!("Could not save the skin: {err}"), ok: false }),
        }
    }

    fn sync_view(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        let Some(canvas) = &self.canvas else { return };
        let image = &canvas.image;
        let picture = egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.pixels);
        match &mut self.view {
            Some(view) => view.set(picture, TextureOptions::NEAREST),
            None => self.view = Some(ctx.load_texture("livery_painter", picture, TextureOptions::NEAREST)),
        }
        self.dirty = false;
    }

    fn paint_area(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.view.clone() else { return };
        let size = view.size_vec2() * self.zoom;
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_gray(40));
            painter.image(view.id(), rect, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            let [r, g, b, a] = self.color.to_srgba_unmultiplied();
            let color = [r, g, b, a];
            let to_picture = |p: egui::Pos2| ((p.x - rect.left()) / self.zoom, (p.y - rect.top()) / self.zoom);
            let hover = response.hover_pos().or(response.interact_pointer_pos());
            if self.tool == Brush::Paint
                && let Some(p) = hover
            {
                painter.circle_stroke(p, self.radius * self.zoom, Stroke::new(1.0, Color32::WHITE));
            }
            let Some(canvas) = &mut self.canvas else { return };
            let pointer = response.interact_pointer_pos().map(to_picture);
            match (self.tool, pointer) {
                (Brush::Paint, Some(at)) => {
                    if response.drag_started() || self.last.is_none() {
                        canvas.checkpoint();
                    }
                    canvas.stroke(self.last.unwrap_or(at), at, self.radius, color);
                    self.last = Some(at);
                    self.dirty = true;
                    self.changed = true;
                }
                (Brush::Fill, Some(at)) if response.drag_started() => {
                    canvas.checkpoint();
                    canvas.fill(at.0 as i32, at.1 as i32, color, 24);
                    self.dirty = true;
                    self.changed = true;
                }
                (Brush::Rectangle, Some(at)) => {
                    let here = (at.0 as i32, at.1 as i32);
                    let start = *self.drag_start.get_or_insert(here);
                    if let Some(p) = response.interact_pointer_pos() {
                        let from = pos2(rect.left() + start.0 as f32 * self.zoom, rect.top() + start.1 as f32 * self.zoom);
                        painter.rect_stroke(egui::Rect::from_two_pos(from, p), 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
                    }
                    if response.drag_stopped() {
                        canvas.checkpoint();
                        canvas.rectangle(start, here, color);
                        self.drag_start = None;
                        self.dirty = true;
                        self.changed = true;
                    }
                }
                (Brush::Stamp, Some(at)) if response.drag_started() => {
                    if let Some(decal) = &self.decal {
                        canvas.checkpoint();
                        canvas.stamp(decal, at.0 as i32, at.1 as i32, self.decal_scale);
                        self.dirty = true;
                        self.changed = true;
                    }
                }
                _ => {}
            }
            if !response.dragged() && !response.drag_started() {
                self.last = None;
            }
        });
    }
}

impl Tool for Painter {
    fn name(&self) -> &'static str {
        "Livery Painter"
    }

    fn content_width(&self) -> f32 {
        1900.0
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        widgets::heading(ui, "Livery Painter", "Paint on a skin's textures. Saving makes a new skin; the original stays as it is.");
        self.poll(notices);
        let Some(root) = ctx.ac_root.clone().filter(|_| ctx.ac_valid) else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to paint liveries.");
            return;
        };
        if self.root.as_ref() != Some(&root) {
            self.root = Some(root.clone());
            self.cars.clear();
            self.car = None;
            let tx = self.tx.clone();
            let repaint = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Cars(content::scan_cars(&root)));
                repaint.request_repaint();
            });
        }
        let egui_ctx = ui.ctx().clone();
        self.pickers(ui, &egui_ctx);
        ui.add_space(6.0);
        self.toolbar(ui, notices);
        ui.add_space(6.0);
        self.sync_view(&egui_ctx);
        if self.busy {
            ui.spinner();
        } else if self.view.is_none() {
            widgets::empty_state(ui, "Choose a car, a skin and a texture to start painting.");
        } else if self.show_3d {
            if let (Some((key, subs)), Some(gl)) = (&self.mesh_3d, &ctx.gl)
                && self.uploaded_3d.as_ref() != Some(key)
                && let Ok(mut guard) = gl.lock()
            {
                guard.upload_mesh(key.clone(), subs);
                self.uploaded_3d = Some(key.clone());
            }
            let avail = ui.available_size();
            let half_w = (avail.x * 0.5 - 8.0).max(240.0);
            let h = avail.y.max(320.0);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(vec2(half_w, h), egui::Layout::top_down(egui::Align::Min), |ui| {
                    self.paint_area(ui);
                });
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
                let dt = ui.input(|i| i.stable_dt);
                let gl = ctx.gl.clone();
                widgets::viewport_3d_frame(
                    ui,
                    vec2(half_w, h),
                    |ui| {
                        widgets::pill(ui, "3D CAR PREVIEW", theme::CH_SPEED);
                    },
                    |ui, rect| {
                        let resp = ui.interact(rect, ui.scope_id().with("lp_3d"), Sense::click_and_drag());
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
                                egui::Align2::CENTER_CENTER,
                                "3D Livery Preview (2D fallback in headless mode)",
                                egui::FontId::proportional(13.0),
                                theme::MUTED,
                            );
                        });
                    },
                );
            });
        } else {
            ui.allocate_space(vec2(0.0, 0.0));
            self.paint_area(ui);
        }
    }
}

fn load_painter_3d(car_path: &Path) -> Result<Vec<CpuSubMesh>, String> {
    let opts = LoadOptions {
        load_textures: false,
        skip_ac_helpers: true,
        max_vertices: 1_000_000,
        max_triangles: 1_000_000,
    };
    let car = kn5_scene::load_car_lod(car_path, None, 0, &opts).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for m in &car.scene.meshes {
        if m.name.starts_with("AC_") || m.indices.len() < 3 {
            continue;
        }
        let vertices: Vec<MeshVertex> = (0..m.positions.len())
            .map(|i| MeshVertex {
                pos: kn5_scene::mat4_transform(&m.transform, &m.positions[i], true),
                normal: kn5_scene::mat4_transform(&m.transform, &m.normals[i], false),
                uv: m.uvs.get(i).copied().unwrap_or([0.0, 0.0]),
            })
            .collect();
        let indices = m.indices.iter().map(|&idx| u32::from(idx)).collect();
        out.push(CpuSubMesh {
            vertices,
            indices,
            texture_key: None,
            tint: [0.85, 0.88, 0.92, 1.0],
        });
    }
    Ok(out)
}
