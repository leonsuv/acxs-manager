//! The showroom: one car at full size, with its skins, specs and description.
//!
//! Left/Right switch skins, PageUp/PageDown switch cars, F toggles the favourite.

use super::theme::{self, ACCENT, FAINT, MUTED, TEXT};
use super::{media, widgets, App};
use crate::content::{self, CarEntry};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind, vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

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
                ui.label(RichText::new("SPECS").font(theme::bold(11.0)).color(FAINT));
                ui.add_space(6.0);
                if info.specs.is_empty() {
                    ui.label(RichText::new("This car ships no spec sheet.").color(MUTED));
                }
                for spec in &info.specs {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(spec.label).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(&spec.value).font(theme::bold(14.0)));
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                    ui.painter().rect_filled(rect, CornerRadius::same(3), theme::INPUT);
                    let fill = Rect::from_min_size(rect.min, vec2(rect.width() * spec.fill, rect.height()));
                    ui.painter().rect_filled(fill, CornerRadius::same(3), ACCENT.gamma_multiply(0.55 + 0.45 * spec.fill));
                    ui.add_space(6.0);
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
                    ui.horizontal_wrapped(|ui| {
                        for tag in &car.tags {
                            widgets::pill(ui, tag, MUTED);
                        }
                    });
                }

                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.add(widgets::primary_button("Race this car")).clicked() {
                        race_car = true;
                    }
                    if ui.button("Open folder").clicked() {
                        open_folder = true;
                    }
                    if ui.button("Edit metadata").clicked() {
                        goto_tool = Some("Metadata & Checks");
                    }
                    if ui.button("All cars").clicked() {
                        jump_cars = true;
                    }
                });
            });
        });
    });
    ui.allocate_ui_with_layout(vec2(ui.available_width(), area.y), egui::Layout::top_down(egui::Align::Min), |ui| {

    // Hero, sized to leave room for the skin strip.
    let strip_height = if skins.len() > 1 { 104.0 } else { 0.0 };
    let avail = ui.available_size();
    let hero_h = (avail.y - strip_height - 8.0).max(200.0).min(avail.x * 9.0 / 16.0);
    let hero_w = (hero_h * 16.0 / 9.0).min(avail.x);
    let (slot, _) = ui.allocate_exact_size(vec2(avail.x, hero_h), Sense::hover());
    let hero = Rect::from_center_size(slot.center(), vec2(hero_w, hero_h));
    let round = CornerRadius::same(16);
    let current = skins.get(skin_pos);
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
