//! Preview images for cars and tracks, and the thumbnail card grid built on them.
//!
//! File lookups are cached: a grid asks for the same preview every frame, and a disk
//! probe per card per frame would stall scrolling.

use super::theme;
use crate::content::TrackEntry;
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, text::LayoutJob, vec2};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// The `file://` URI egui's loaders use for a local file.
pub fn uri(path: &Path) -> String {
    let p = path.display().to_string().replace('\\', "/").replace('%', "%25").replace(' ', "%20").replace('#', "%23").replace('?', "%3F");
    format!("file:///{p}")
}

type Skins = Arc<Vec<(String, PathBuf)>>;

fn skin_cache() -> &'static Mutex<HashMap<PathBuf, Skins>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Skins>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn file_cache() -> &'static Mutex<HashMap<PathBuf, Option<PathBuf>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// First of `candidates` that exists, remembered per candidate list head.
fn first_existing(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    let key = candidates.first()?.clone();
    if let Some(hit) = file_cache().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let found = candidates.into_iter().find(|p| p.is_file());
    if let Ok(mut cache) = file_cache().lock() {
        cache.insert(key, found.clone());
    }
    found
}

/// Skins of a car that have a preview image, sorted by folder name.
pub fn car_skins(car_path: &Path) -> Skins {
    if let Some(hit) = skin_cache().lock().ok().and_then(|c| c.get(car_path).cloned()) {
        return hit;
    }
    let mut skins: Vec<(String, PathBuf)> = fs::read_dir(car_path.join("skins"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let preview = entry.path().join("preview.jpg");
            preview.is_file().then_some((name, preview))
        })
        .collect();
    skins.sort_by_key(|(name, _)| name.to_lowercase());
    let skins = Arc::new(skins);
    if let Ok(mut cache) = skin_cache().lock() {
        cache.insert(car_path.to_path_buf(), Arc::clone(&skins));
    }
    skins
}

/// Preview of the named skin, or of the first skin.
pub fn car_preview(car_path: &Path, skin: Option<&str>) -> Option<PathBuf> {
    let skins = car_skins(car_path);
    skin.and_then(|name| skins.iter().find(|(n, _)| n == name))
        .or_else(|| skins.first())
        .map(|(_, path)| path.clone())
}

pub fn car_badge(car_path: &Path) -> Option<PathBuf> {
    first_existing(vec![car_path.join("ui").join("badge.png")])
}

fn track_file(track: &TrackEntry, layout: Option<&str>, file: &str) -> Option<PathBuf> {
    let ui = track.path.join("ui");
    let mut candidates = Vec::new();
    if let Some(layout) = layout {
        candidates.push(ui.join(layout).join(file));
    }
    candidates.push(ui.join(file));
    candidates.extend(track.layouts.iter().map(|l| ui.join(l).join(file)));
    first_existing(candidates)
}

pub fn track_preview(track: &TrackEntry, layout: Option<&str>) -> Option<PathBuf> {
    track_file(track, layout, "preview.png")
}

pub fn track_outline(track: &TrackEntry, layout: Option<&str>) -> Option<PathBuf> {
    track_file(track, layout, "outline.png")
}

/// Draw `path` into `rect`, or a tinted placeholder with the initials of `label`.
pub fn paint_image(ui: &egui::Ui, rect: Rect, path: Option<&Path>, label: &str, tint: Color32, rounding: CornerRadius) {
    // Pictures far out of view are not drawn, so long lists do not decode hundreds of images.
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, rounding, theme::INPUT);
    match path {
        Some(path) => {
            egui::Image::from_uri(uri(path))
                .maintain_aspect_ratio(false)
                .fit_to_exact_size(rect.size())
                .corner_radius(rounding)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, rounding, tint.gamma_multiply(0.12));
            let initials: String = label
                .split_whitespace()
                .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
                .take(2)
                .flat_map(|c| c.to_uppercase())
                .collect();
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, initials, theme::bold(rect.height() * 0.3), tint);
        }
    }
}

/// Single-line text clipped with an ellipsis to `width`.
pub fn clipped(ui: &egui::Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter().layout_job(job)
}

/// A small rounded chip drawn on top of an image.
pub fn chip(ui: &egui::Ui, anchor: egui::Pos2, align: Align2, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.5), color);
    let size = galley.size() + vec2(14.0, 6.0);
    let rect = align.anchor_size(anchor, size);
    ui.painter().rect_filled(rect, CornerRadius::same(255), Color32::from_black_alpha(170));
    ui.painter().rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0, color.gamma_multiply(0.6)), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// Colour used for a car class chip.
pub fn class_color(class: &str) -> Color32 {
    match class.to_lowercase().as_str() {
        "race" => theme::ACCENT,
        "street" => theme::INFO,
        "" => theme::MUTED,
        _ => theme::OK,
    }
}

/// What happened in a [`grid`] this frame.
#[derive(Default)]
pub struct GridOut {
    pub clicked: Option<usize>,
    /// Double-clicked: open the item.
    pub opened: Option<usize>,
    /// Cards per row this frame, for keyboard movement.
    pub columns: usize,
    /// The favourite star of this card was clicked.
    pub star: Option<usize>,
}

/// Options for a [`grid`].
pub struct GridSpec {
    pub min_width: f32,
    pub info_height: f32,
    /// Scroll so this card is in view (used after keyboard movement).
    pub scroll_to: Option<usize>,
    /// Draw a favourite star on each card.
    pub stars: bool,
}

/// A scrolling grid of image cards. `draw` paints one card's image and text, `menu` fills its
/// right-click menu. Cards lift on hover and show their star on hover or when favourited.
pub fn grid<T>(
    ui: &mut egui::Ui,
    items: &[T],
    spec: &GridSpec,
    is_selected: impl Fn(&T) -> bool,
    is_favorite: impl Fn(&T) -> bool,
    mut draw: impl FnMut(&egui::Ui, Rect, Rect, &T),
    mut menu: impl FnMut(&mut egui::Ui, usize, &T, &mut GridOut),
) -> GridOut {
    const GAP: f32 = 16.0;
    const ASPECT: f32 = 9.0 / 16.0;
    let available = ui.available_width() - 10.0;
    let columns = (((available + GAP) / (spec.min_width + GAP)).floor() as usize).max(1);
    let width = ((available - GAP * (columns as f32 - 1.0)) / columns as f32).floor();
    let image_height = (width * ASPECT).round();
    let card_height = image_height + spec.info_height;
    let rows = items.len().div_ceil(columns);
    let mut out = GridOut { columns, ..GridOut::default() };

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(index) = spec.scroll_to {
            let top = (index / columns) as f32 * (card_height + GAP);
            area = area.vertical_scroll_offset((top - 30.0).max(0.0));
        }
        area.show_rows(ui, card_height, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    for index in row * columns..((row + 1) * columns).min(items.len()) {
                        let item = &items[index];
                        let (slot, response) = ui.allocate_exact_size(vec2(width, card_height), Sense::click());
                        let selected = is_selected(item);
                        let favorite = is_favorite(item);
                        let hover = ui.ctx().animate_bool_with_time(response.id, response.hovered(), 0.12);
                        // Lift: the card grows 3 px on hover.
                        let rect = slot.expand(2.0 * hover);
                        let fill = if selected { theme::CARD_SELECTED } else { theme::CARD.lerp_to_gamma(theme::CARD_HOVER, hover) };
                        let round = CornerRadius::same(12);
                        if hover > 0.0 {
                            ui.painter().rect_filled(
                                rect.translate(vec2(0.0, 6.0 * hover)),
                                round,
                                Color32::from_black_alpha((70.0 * hover) as u8),
                            );
                        }
                        ui.painter().rect_filled(rect, round, fill);
                        let image_rect = Rect::from_min_size(rect.min, vec2(rect.width(), image_height + 2.0 * hover));
                        let info_rect = Rect::from_min_max(image_rect.left_bottom(), rect.right_bottom());
                        draw(ui, image_rect, info_rect, item);

                        if spec.stars && (favorite || hover > 0.0) {
                            let star = Rect::from_center_size(image_rect.right_top() + vec2(-20.0, 20.0), vec2(30.0, 30.0));
                            let star_resp = ui.interact(star, response.id.with("star"), Sense::click());
                            ui.painter().circle_filled(star.center(), 14.0, Color32::from_black_alpha(if star_resp.hovered() { 200 } else { 150 }));
                            let (glyph, color) = if favorite { ("★", theme::WARN) } else { ("☆", Color32::WHITE) };
                            ui.painter().text(star.center(), Align2::CENTER_CENTER, glyph, egui::FontId::proportional(16.0), color);
                            if star_resp.on_hover_text(if favorite { "Remove from favorites" } else { "Add to favorites" }).clicked() {
                                out.star = Some(index);
                            }
                        }

                        let stroke = if selected {
                            Stroke::new(1.5, theme::ACCENT)
                        } else {
                            Stroke::new(1.0, theme::BORDER.lerp_to_gamma(theme::BORDER_STRONG, hover))
                        };
                        ui.painter().rect_stroke(rect, round, stroke, StrokeKind::Inside);

                        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.double_clicked() {
                            out.opened = Some(index);
                        } else if response.clicked() && out.star.is_none() {
                            out.clicked = Some(index);
                        }
                        response.context_menu(|ui| menu(ui, index, item, &mut out));
                    }
                });
            }
        });
    });
    out
}

/// Top-corners-only rounding for an image that sits on a card.
pub fn top_round() -> CornerRadius {
    CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 }
}

/// Fill `rect` with a horizontal band of the image (cover fit), for banners.
pub fn paint_cover(ui: &egui::Ui, rect: Rect, path: &Path, rounding: CornerRadius) {
    let band = ((rect.height() / rect.width()) * (16.0 / 9.0)).clamp(0.05, 1.0);
    let top = (0.55 - band / 2.0).clamp(0.0, 1.0 - band);
    let uv = Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(1.0, top + band));
    ui.painter().rect_filled(rect, rounding, theme::INPUT);
    egui::Image::from_uri(uri(path))
        .uv(uv)
        .maintain_aspect_ratio(false)
        .fit_to_exact_size(rect.size())
        .corner_radius(rounding)
        .paint_at(ui, rect);
}

/// Top-corners-only rounding with the given radius.
pub fn top_round_by(radius: u8) -> CornerRadius {
    CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 }
}
