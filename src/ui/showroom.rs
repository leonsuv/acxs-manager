//! The showroom: one car at full size, with its skins, specs and description.
//!
//! Left/Right switch skins, PageUp/PageDown switch cars, F toggles the favourite.

use super::theme::{self, ACCENT, FAINT, MUTED, TEXT};
use super::viewport3d::{self, Camera3D, CpuSubMesh, DrawItem, MeshVertex, RenderMode, SceneFrame};
use super::{media, widgets, App};
use crate::content::{self, CarEntry};
use acxs_manager::kn5_scene::{self, LoadOptions};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind, vec2};
use glam::Mat4;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShowroomView {
    Preview2D,
    Showroom3D,
}

struct LoadedCar3D {
    key: (PathBuf, u64),
    submeshes: Vec<CpuSubMesh>,
    tri_count: usize,
    center: glam::Vec3,
    radius: f32,
}

struct Showroom3DState {
    view: ShowroomView,
    camera: Camera3D,
    render_mode: RenderMode,
    requested: Option<(PathBuf, Option<String>)>,
    loading: bool,
    loaded: Option<LoadedCar3D>,
    uploaded_key: Option<(PathBuf, u64)>,
    error: Option<String>,
    tx: Sender<((PathBuf, Option<String>), Result<LoadedCar3D, String>)>,
    rx: Receiver<((PathBuf, Option<String>), Result<LoadedCar3D, String>)>,
}

impl Showroom3DState {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            view: ShowroomView::Preview2D,
            camera: Camera3D::default(),
            render_mode: RenderMode::Shaded,
            requested: None,
            loading: false,
            loaded: None,
            uploaded_key: None,
            error: None,
            tx,
            rx,
        }
    }
}

fn state_3d() -> &'static Mutex<Showroom3DState> {
    static STATE: OnceLock<Mutex<Showroom3DState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(Showroom3DState::new()))
}

fn build_car_3d(car_dir: &Path, skin: Option<&str>) -> Result<LoadedCar3D, String> {
    let opts = LoadOptions {
        load_textures: false,
        skip_ac_helpers: true,
        max_vertices: 1_500_000,
        max_triangles: 1_500_000,
    };
    let car_scene = kn5_scene::load_car_lod(car_dir, skin, 0, &opts).map_err(|e| e.to_string())?;
    let mut submeshes = Vec::new();
    let mut tri_count = 0usize;
    let mut min_p = glam::Vec3::splat(f32::INFINITY);
    let mut max_p = glam::Vec3::splat(f32::NEG_INFINITY);

    for mesh in &car_scene.scene.meshes {
        if mesh.name.starts_with("AC_") || mesh.indices.len() < 3 {
            continue;
        }
        let mut vertices = Vec::with_capacity(mesh.positions.len());
        for i in 0..mesh.positions.len() {
            let p = kn5_scene::mat4_transform(&mesh.transform, &mesh.positions[i], true);
            let n = kn5_scene::mat4_transform(&mesh.transform, &mesh.normals[i], false);
            let uv = mesh.uvs.get(i).copied().unwrap_or([0.0, 0.0]);
            let v = glam::Vec3::from_array(p);
            min_p = min_p.min(v);
            max_p = max_p.max(v);
            vertices.push(MeshVertex { pos: p, normal: n, uv });
        }
        let indices: Vec<u32> = mesh.indices.iter().map(|&idx| u32::from(idx)).collect();
        tri_count += indices.len() / 3;
        submeshes.push(CpuSubMesh {
            vertices,
            indices,
            texture_key: None,
            tint: [0.84, 0.87, 0.91, 1.0],
        });
    }
    let (center, radius) = if min_p.is_finite() && max_p.is_finite() {
        ((min_p + max_p) * 0.5, ((max_p - min_p).length() * 0.5).max(1.5))
    } else {
        (glam::Vec3::ZERO, 2.5)
    };
    let hash = skin.map_or(0u64, |s| s.bytes().fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(b))));
    Ok(LoadedCar3D {
        key: (car_dir.join(&car_scene.lod_file), hash),
        submeshes,
        tri_count,
        center,
        radius,
    })
}

/// One line of the spec sheet.
struct Spec {
    label: &'static str,
    value: String,
    /// 0..=1, how far along the bar is.
    fill: f32,
}

/// What `ui_car.json` says about a car, ready to show.
struct CarInfo {
    specs: Vec<Spec>,
    description: String,
    country: String,
    author: String,
}

/// (json key, label, unit, bar maximum, lower is better)
const SPECS: [(&str, &str, &str, f64, bool); 6] = [
    ("bhp", "Power", "hp", 1000.0, false),
    ("torque", "Torque", "Nm", 1200.0, false),
    ("weight", "Weight", "kg", 2200.0, true),
    ("topspeed", "Top speed", "km/h", 400.0, false),
    ("acceleration", "0–100 km/h", "s", 10.0, true),
    ("pwratio", "Weight / power", "kg/hp", 8.0, true),
];

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<CarInfo>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<CarInfo>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The first number in a spec string such as "1,250 kg" or "3.2s".
fn first_number(text: &str) -> Option<f64> {
    let cleaned = text.replace(',', "");
    let start = cleaned.find(|c: char| c.is_ascii_digit())?;
    let tail = &cleaned[start..];
    let end = tail.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(tail.len());
    tail[..end].parse().ok()
}

/// Remove the HTML the game's descriptions carry.
fn plain_text(html: &str) -> String {
    let html = html.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n").replace("</p>", "\n");
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&nbsp;", " ").trim().to_string()
}

fn car_info(car_path: &Path) -> Arc<CarInfo> {
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(car_path).cloned()) {
        return hit;
    }
    let json = content::read_ui_json(&car_path.join("ui").join("ui_car.json"));
    let specs_json = json.get("specs").and_then(|v| v.as_object());
    let specs = SPECS
        .iter()
        .filter_map(|(key, label, unit, max, lower_better)| {
            let raw = specs_json?.get(*key)?.as_str()?.trim().to_string();
            let number = first_number(&raw).filter(|n| *n > 0.0)?;
            let share = (number / max).clamp(0.04, 1.0) as f32;
            let value = if raw.chars().any(|c| c.is_alphabetic()) { raw } else { format!("{raw} {unit}") };
            Some(Spec { label, value, fill: if *lower_better { 1.04 - share } else { share }.clamp(0.04, 1.0) })
        })
        .collect();
    let text = |key: &str| json.get(key).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let info = Arc::new(CarInfo {
        specs,
        description: plain_text(&text("description")),
        country: text("country"),
        author: text("author"),
    });
    if let Ok(mut c) = cache().lock() {
        c.insert(car_path.to_path_buf(), Arc::clone(&info));
    }
    info
}

pub fn showroom(app: &mut App, ui: &mut egui::Ui) {
    let valid = |app: &App, id: &str| app.snapshot.cars.iter().any(|c| c.id == id);
    if app.showroom_car.as_deref().is_none_or(|id| !valid(app, id)) {
        app.showroom_car = app
            .selected_car
            .clone()
            .filter(|id| valid(app, id))
            .or_else(|| app.favorites.iter().find(|id| valid(app, id)).cloned())
            .or_else(|| app.snapshot.cars.first().map(|c| c.id.clone()));
    }
    let Some(index) = app
        .showroom_car
        .as_deref()
        .and_then(|id| app.snapshot.cars.iter().position(|c| c.id == id))
    else {
        widgets::heading(ui, "Showroom", "Look at your cars up close.");
        widgets::empty_state(ui, "No cars found. Set your Assetto Corsa folder in Settings.");
        return;
    };
    let total = app.snapshot.cars.len();
    let car: CarEntry = app.snapshot.cars[index].clone();
    let info = car_info(&car.path);
    let skins = media::car_skins(&car.path);
    let skin_pos = app
        .car_skin
        .as_deref()
        .and_then(|name| skins.iter().position(|(n, _)| n == name))
        .unwrap_or(0);
    let favorite = app.favorites.contains(&car.id);

    // Keyboard.
    let (left, right, prev, next, fav) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::PageUp),
            i.key_pressed(egui::Key::PageDown),
            i.key_pressed(egui::Key::F) && !i.modifiers.command,
        )
    });
    let typing = ui.ctx().egui_wants_keyboard_input();
    let mut skin_step: i32 = 0;
    let mut car_step: i32 = 0;
    let mut toggle_fav = false;
    if !typing {
        skin_step += right as i32 - left as i32;
        car_step += next as i32 - prev as i32;
        toggle_fav |= fav;
    }
    let mut open_folder = false;
    let mut jump_cars = false;
    let mut race_car = false;
    let mut game_showroom = false;
    let mut goto_tool: Option<&str> = None;

    let area = ui.available_size();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
    ui.spacing_mut().item_spacing.x = 16.0;
    ui.allocate_ui_with_layout(vec2(340.0, area.y), egui::Layout::top_down(egui::Align::Min), |ui| {
    egui::Frame::new()
        .fill(theme::SIDEBAR)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(304.0);
            ui.set_min_height(area.y - 38.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.label(RichText::new("SPECS").font(theme::display(13.0)).color(FAINT));
                ui.add_space(6.0);
                if info.specs.is_empty() {
                    ui.label(RichText::new("This car ships no spec sheet.").color(MUTED));
                }
                for spec in &info.specs {
                    widgets::spec_bar_animated(ui, &car.id, spec.label, &spec.value, spec.fill, ACCENT);
                }
                if info.specs.len() >= 3 {
                    let axes: Vec<(&str, f32)> = info
                        .specs
                        .iter()
                        .take(6)
                        .map(|s| (s.label, s.fill))
                        .collect();
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(((ui.available_width() - 180.0) * 0.5).max(0.0));
                        widgets::radar_chart(ui, 180.0, &axes, ACCENT);
                    });
                }

                if !info.description.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("ABOUT").font(theme::bold(11.0)).color(FAINT));
                    ui.add_space(4.0);
                    ui.label(RichText::new(&info.description).size(13.0).color(TEXT));
                }
                let credits = [info.country.as_str(), info.author.as_str()];
                if credits.iter().any(|c| !c.is_empty()) {
                    ui.add_space(8.0);
                    let text = credits.iter().filter(|c| !c.is_empty()).copied().collect::<Vec<_>>().join("  ·  ");
                    ui.label(RichText::new(text).size(12.0).color(FAINT));
                }
                if !car.tags.is_empty() {
                    ui.add_space(10.0);
                    widgets::pill_rows(ui, &car.tags, MUTED);
                }

                ui.add_space(16.0);
                let full = ui.available_width();
                if ui.add_sized([full, 38.0], widgets::primary_button("Race this car")).clicked() {
                    race_car = true;
                }
                ui.add_space(4.0);
                let half = (full - ui.spacing().item_spacing.x) / 2.0;
                ui.horizontal(|ui| {
                    if ui.add_sized([half, 32.0], egui::Button::new("Open in the game")).on_hover_text("Starts the showroom of Assetto Corsa with this car.").clicked() {
                        game_showroom = true;
                    }
                    if ui.add_sized([half, 32.0], egui::Button::new("Open folder")).clicked() {
                        open_folder = true;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_sized([half, 32.0], egui::Button::new("Edit details")).clicked() {
                        goto_tool = Some("Metadata & Checks");
                    }
                    if ui.add_sized([half, 32.0], egui::Button::new("All cars")).clicked() {
                        jump_cars = true;
                    }
                });
            });
        });
    });
    ui.allocate_ui_with_layout(vec2(ui.available_width(), area.y), egui::Layout::top_down(egui::Align::Min), |ui| {

    // Mode toggle bar: 2D Preview | 3D Showroom.
    let mut show_3d = false;
    if let Ok(mut st) = state_3d().lock() {
        while let Ok((req, res)) = st.rx.try_recv() {
            if st.requested.as_ref() == Some(&req) {
                st.loading = false;
                match res {
                    Ok(loaded) => {
                        st.camera = Camera3D::Orbit {
                            target: loaded.center,
                            yaw: 0.75,
                            pitch: 0.30,
                            dist: loaded.radius * 2.4,
                        };
                        st.loaded = Some(loaded);
                        st.uploaded_key = None;
                        st.error = None;
                    }
                    Err(e) => {
                        st.loaded = None;
                        st.error = Some(e);
                    }
                }
            }
        }
        ui.horizontal(|ui| {
            widgets::animated_tabs(
                ui,
                "showroom_view_mode",
                &mut st.view,
                &[
                    (ShowroomView::Preview2D, "2D Preview", None),
                    (ShowroomView::Showroom3D, "3D Showroom", None),
                ],
            );
            if st.view == ShowroomView::Showroom3D {
                ui.add_space(8.0);
                for (m, lbl) in [
                    (RenderMode::Shaded, "Shaded"),
                    (RenderMode::Wireframe, "Wireframe"),
                    (RenderMode::Normals, "Normals"),
                ] {
                    ui.selectable_value(&mut st.render_mode, m, lbl);
                }
            }
        });
        ui.add_space(6.0);
        show_3d = st.view == ShowroomView::Showroom3D;
    }

    // Hero, sized to leave room for the skin strip.
    let strip_height = if skins.len() > 1 { 104.0 } else { 0.0 };
    let avail = ui.available_size();
    let hero_h = (avail.y - strip_height - 8.0).max(200.0).min(avail.x * 9.0 / 16.0);
    let hero_w = (hero_h * 16.0 / 9.0).min(avail.x);
    let current = skins.get(skin_pos);

    if show_3d {
        let skin_name = current.map(|(n, _)| n.clone());
        if let Ok(mut st) = state_3d().lock() {
            let req = (car.path.clone(), skin_name.clone());
            if st.requested.as_ref() != Some(&req) {
                st.requested = Some(req.clone());
                st.loading = true;
                st.error = None;
                let tx = st.tx.clone();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let res = build_car_3d(&req.0, req.1.as_deref());
                    let _ = tx.send((req, res));
                    ctx.request_repaint();
                });
            }
            if let (Some(loaded), Some(gl)) = (&st.loaded, &app.gl_renderer)
                && st.uploaded_key.as_ref() != Some(&loaded.key)
                && let Ok(mut guard) = gl.lock()
            {
                guard.upload_mesh(loaded.key.clone(), &loaded.submeshes);
                st.uploaded_key = Some(loaded.key.clone());
            }
            let tri_label = st
                .loaded
                .as_ref()
                .map(|l| format!("{} tris", l.tri_count))
                .unwrap_or_else(|| if st.loading { "Loading KN5...".into() } else { "No mesh".into() });
            let err_msg = st.error.clone();
            let mut frame = SceneFrame {
                camera: st.camera,
                render_mode: st.render_mode,
                ..SceneFrame::default()
            };
            if let Some(loaded) = &st.loaded {
                frame.items.push(DrawItem {
                    mesh_key: loaded.key.clone(),
                    model: Mat4::IDENTITY,
                    tint_override: None,
                    texture_override: None,
                });
            }
            let gl = app.gl_renderer.clone();
            let dt = ui.input(|i| i.stable_dt);
            let mut reset_cam = false;
            let is_loading = st.loading;
            let mut cam = st.camera;
            widgets::viewport_3d_frame(
                ui,
                vec2(avail.x, hero_h),
                |ui| {
                    ui.horizontal(|ui| {
                        widgets::pill(ui, "3D SHOWROOM", theme::CH_SPEED);
                        widgets::pill(ui, &tri_label, theme::MUTED);
                        if ui.small_button("Reset cam").clicked() {
                            reset_cam = true;
                        }
                    });
                },
                |ui, rect| {
                    let resp = ui.interact(rect, ui.scope_id().with("sr_3d_drag"), Sense::click_and_drag());
                    ui.input(|i| {
                        if cam.handle_input(&resp, i, dt) {
                            ui.ctx().request_repaint();
                        }
                    });
                    frame.camera = cam;
                    viewport3d::paint_viewport_3d(ui, rect, gl, frame, |ui, r| {
                        ui.painter().rect_filled(r, 0, theme::SURFACE_VOID);
                        let msg = err_msg.as_deref().unwrap_or(if is_loading {
                            "Loading car KN5 geometry..."
                        } else {
                            "3D Viewport ready (2D fallback in headless mode)"
                        });
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            msg,
                            egui::FontId::proportional(13.5),
                            MUTED,
                        );
                    });
                },
            );
            st.camera = cam;
            if reset_cam
                && let Some(loaded) = &st.loaded
            {
                st.camera = Camera3D::Orbit {
                    target: loaded.center,
                    yaw: 0.75,
                    pitch: 0.30,
                    dist: loaded.radius * 2.4,
                };
            }
        }
    } else {
    let (slot, _) = ui.allocate_exact_size(vec2(avail.x, hero_h), Sense::hover());
    let hero = Rect::from_center_size(slot.center(), vec2(hero_w, hero_h));
    let round = CornerRadius::same(16);
    media::paint_image(ui, hero, current.map(|(_, p)| p.as_path()), &car.name, theme::INFO, round);

    // Dark gradient under the title.
    let shade = Rect::from_min_max(hero.left_bottom() - vec2(0.0, hero_h * 0.45), hero.right_bottom());
    let mut mesh = egui::Mesh::default();
    let clear = Color32::from_black_alpha(0);
    let dark = Color32::from_black_alpha(215);
    mesh.colored_vertex(shade.left_top(), clear);
    mesh.colored_vertex(shade.right_top(), clear);
    mesh.colored_vertex(shade.right_bottom(), dark);
    mesh.colored_vertex(shade.left_bottom(), dark);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
    ui.painter().rect_stroke(hero, round, Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);

    let title = ui.painter().layout_no_wrap(car.name.clone(), theme::bold(32.0), Color32::WHITE);
    ui.painter().galley(hero.left_bottom() + vec2(24.0, -30.0 - title.size().y), title, Color32::WHITE);
    let sub = content_subtitle(&car);
    if !sub.is_empty() {
        ui.painter().text(hero.left_bottom() + vec2(24.0, -22.0), Align2::LEFT_CENTER, sub, egui::FontId::proportional(14.5), Color32::from_gray(205));
    }
    if !car.car_class.is_empty() {
        media::chip(ui, hero.left_top() + vec2(16.0, 16.0), Align2::LEFT_TOP, &car.car_class, media::class_color(&car.car_class));
    }
    if let Some((name, _)) = current {
        let text = format!("{name}   {}/{}", skin_pos + 1, skins.len());
        media::chip(ui, hero.right_bottom() - vec2(16.0, 16.0), Align2::RIGHT_BOTTOM, &text, Color32::WHITE);
    }

    // Overlay buttons: favourite, previous and next skin.
    let star = Rect::from_center_size(hero.right_top() + vec2(-34.0, 34.0), vec2(40.0, 40.0));
    let star_resp = ui.interact(star, ui.scope_id().with("sr_star"), Sense::click());
    ui.painter().circle_filled(star.center(), 20.0, Color32::from_black_alpha(if star_resp.hovered() { 210 } else { 150 }));
    let (glyph, color) = if favorite { ("★", theme::WARN) } else { ("☆", Color32::WHITE) };
    ui.painter().text(star.center(), Align2::CENTER_CENTER, glyph, egui::FontId::proportional(22.0), color);
    if star_resp.on_hover_text("Favorite (F)").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        toggle_fav = true;
    }
    if skins.len() > 1 {
        for (dir, glyph, x) in [(-1, "‹", 34.0), (1, "›", hero.width() - 34.0)] {
            let rect = Rect::from_center_size(hero.left_center() + vec2(x, 0.0), vec2(44.0, 44.0));
            let resp = ui.interact(rect, ui.scope_id().with(("sr_arrow", dir)), Sense::click());
            let alpha = if resp.hovered() { 210 } else { 110 };
            ui.painter().circle_filled(rect.center(), 22.0, Color32::from_black_alpha(alpha));
            ui.painter().text(rect.center() - vec2(0.0, 2.0), Align2::CENTER_CENTER, glyph, egui::FontId::proportional(28.0), Color32::WHITE);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                skin_step = dir;
            }
        }
    }
    }

    // Skin strip.
    if skins.len() > 1 {
        ui.add_space(10.0);
        egui::ScrollArea::horizontal().id_salt("showroom_skins").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (i, (name, path)) in skins.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(124.0, 70.0), Sense::click());
                    let selected = i == skin_pos;
                    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || selected, 0.1);
                    let grown = rect.expand(2.0 * hover);
                    media::paint_image(ui, grown, Some(path), name, theme::INFO, CornerRadius::same(8));
                    let stroke = if selected { Stroke::new(2.0, ACCENT) } else { Stroke::new(1.0, theme::BORDER_STRONG) };
                    ui.painter().rect_stroke(grown, CornerRadius::same(8), stroke, StrokeKind::Inside);
                    if resp.on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        skin_step = i as i32 - skin_pos as i32;
                    }
                }
            });
            ui.add_space(6.0);
        });
    }
    });
    });

    // Apply what happened this frame.
    if skin_step != 0 && !skins.is_empty() {
        let len = skins.len() as i32;
        let next = (skin_pos as i32 + skin_step).rem_euclid(len) as usize;
        app.car_skin = Some(skins[next].0.clone());
    }
    if car_step != 0 && total > 0 {
        let next = (index as i32 + car_step).rem_euclid(total as i32) as usize;
        app.showroom_car = Some(app.snapshot.cars[next].id.clone());
        app.selected_car = app.showroom_car.clone();
        app.car_skin = None;
    }
    if toggle_fav {
        app.toggle_favorite(&car.id);
    }
    if open_folder {
        super::pages::open_folder(&car.path);
    }
    if game_showroom {
        let skin = skins.get(skin_pos).map(|(name, _)| name.clone()).unwrap_or_default();
        let result = match (app.settings.ac_root_path(), crate::settings::ac_user_cfg_dir()) {
            (Some(root), Some(cfg)) => crate::game::launch_showroom(&root, &cfg, &car.id, &skin).map(|_| ()),
            _ => Err(std::io::Error::other("the game folder is not set")),
        };
        if let Err(err) = result {
            app.push_notice(format!("Could not open the showroom: {err}"), false);
        }
    }
    if race_car {
        app.race_request = Some(super::pages::RaceAction {
            car: Some(car.id.clone()),
            skin: skins.get(skin_pos).map(|(name, _)| name.clone()),
            ..super::pages::RaceAction::default()
        });
    }
    if jump_cars {
        app.selected_car = Some(car.id.clone());
        app.page = super::Page::Cars;
    }
    if let Some(name) = goto_tool
        && let Some(i) = app.tools.iter().position(|t| t.name() == name)
    {
        app.page = super::Page::Tool(i);
    }
    // Keep the showroom animating when a key could change the picture.
    let _ = FAINT;
}

fn content_subtitle(car: &CarEntry) -> String {
    [car.brand.as_str(), car.year.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("  ·  ")
}
