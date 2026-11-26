//! Reusable UI pieces: legacy widgets + AAA motorsport widget kit.

use super::theme::{self, BG, CARD_HOVER, CARD_SELECTED, FAINT, MUTED, TEXT};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Margin, Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind, pos2, vec2,
};

// ---------------------------------------------------------------------------
// Legacy widgets (preserved with exact signatures for zero breakage)
// ---------------------------------------------------------------------------

/// A small rounded label, e.g. a status or a tag.
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.14))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).small().color(color)).wrap_mode(egui::TextWrapMode::Extend));
        });
}

/// Pills in rows that fit the width. A wrapping layout cannot do this: a pill's frame does not
/// wrap, so one long row would push the whole panel past its edge.
pub fn pill_rows(ui: &mut egui::Ui, tags: &[String], color: Color32) {
    let width = ui.available_width();
    let font = egui::FontId::proportional(11.0);
    let gap = ui.spacing().item_spacing.x;
    let measured: Vec<f32> = tags
        .iter()
        .map(|tag| ui.painter().layout_no_wrap(tag.clone(), font.clone(), color).size().x + 22.0 + gap)
        .collect();
    let mut start = 0;
    while start < tags.len() {
        let mut end = start;
        let mut used = 0.0;
        while end < tags.len() && (end == start || used + measured[end] <= width) {
            used += measured[end];
            end += 1;
        }
        ui.horizontal(|ui| {
            for tag in &tags[start..end] {
                pill(ui, tag, color);
            }
        });
        start = end;
    }
}

/// A pill with a leading status dot, used for live state (game running, CSP version).
pub fn status_pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.10))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, color);
                ui.label(RichText::new(text).size(12.5).color(TEXT));
            });
        });
}

/// Big number with a caption, used in dashboards.
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(label).size(12.0).color(MUTED));
        ui.label(RichText::new(value).font(theme::mono(20.0)).color(color));
    });
}

pub fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).font(theme::display(28.0)));
    ui.label(RichText::new(subtitle).color(MUTED));
    ui.add_space(14.0);
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text.to_uppercase()).font(theme::display(13.5)).color(MUTED));
    ui.add_space(4.0);
}

pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).font(theme::bold(14.5)).color(BG))
        .fill(theme::ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(9))
        .min_size(vec2(0.0, 34.0))
}

/// A full-width clickable card. Returns true when clicked.
pub fn list_row(
    ui: &mut egui::Ui,
    selected: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> bool {
    let fill = if selected { CARD_SELECTED } else { theme::SURFACE_ELEVATED };
    let frame = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(
            1.0,
            if selected { theme::ACCENT.gamma_multiply(0.7) } else { theme::BORDER_SUBTLE },
        ))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10));
    let response = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        })
        .response
        .interact(Sense::click());
    if response.hovered() && !selected {
        ui.painter().rect_filled(
            response.rect,
            CornerRadius::same(10),
            CARD_HOVER.gamma_multiply(0.35),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

pub fn key_value(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(key).color(MUTED));
        ui.label(RichText::new(value).color(TEXT));
    });
}

pub fn empty_state(ui: &mut egui::Ui, text: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).color(MUTED));
    });
}

/// Small uppercase label above a group of sidebar entries.
#[allow(dead_code)]
pub fn nav_section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(9.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(text.to_uppercase()).font(theme::bold(11.0)).color(FAINT));
    });
    ui.add_space(2.0);
}

/// One sidebar entry. `indent` pushes it right for entries under a group header.
pub fn nav_item(ui: &mut egui::Ui, selected: bool, label: &str, badge: Option<&str>, indent: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 33.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, CornerRadius::same(8), theme::ACCENT.gamma_multiply(0.13));
            let bar = Rect::from_center_size(rect.left_center() + vec2(2.0, 0.0), vec2(3.0, 18.0));
            painter.rect_filled(bar, CornerRadius::same(2), theme::ACCENT);
        } else if hovered {
            painter.rect_filled(rect, CornerRadius::same(8), CARD_HOVER);
        }
        let color = if selected || hovered { TEXT } else { MUTED };
        let font = if selected { theme::bold(14.5) } else { FontId::proportional(14.5) };
        painter.text(rect.left_center() + vec2(14.0 + indent, 0.0), Align2::LEFT_CENTER, label, font, color);
        if let Some(badge) = badge {
            painter.text(rect.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, badge, theme::mono(11.5), FAINT);
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A collapsible group header for the sidebar tool list. Returns true when toggled.
#[allow(dead_code)]
pub fn nav_group(ui: &mut egui::Ui, open: bool, label: &str, count: usize, holds_active: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 31.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(8), CARD_HOVER.gamma_multiply(0.6));
        }
        let color = if holds_active { theme::ACCENT } else if response.hovered() { TEXT } else { MUTED };
        let c = rect.left_center() + vec2(16.0, 0.0);
        let points = if open {
            vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
        } else {
            vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
        };
        painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        painter.text(rect.left_center() + vec2(30.0, 0.0), Align2::LEFT_CENTER, label, theme::bold(14.0), color);
        painter.text(
            rect.right_center() - vec2(12.0, 0.0),
            Align2::RIGHT_CENTER,
            count.to_string(),
            theme::mono(11.5),
            FAINT,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Lay `items` out in equal-width columns that adapt to the available width.
pub fn tile_grid(ui: &mut egui::Ui, min_width: f32, items: usize, mut draw: impl FnMut(&mut egui::Ui, usize)) {
    let gap = ui.spacing().item_spacing.x;
    let available = ui.available_width();
    let columns = (((available + gap) / (min_width + gap)).floor() as usize).clamp(1, items.max(1));
    let width = ((available - gap * (columns as f32 - 1.0)) / columns as f32).floor();
    for row in (0..items).step_by(columns) {
        ui.horizontal_top(|ui| {
            for index in row..(row + columns).min(items) {
                ui.allocate_ui(vec2(width, 0.0), |ui| {
                    ui.set_width(width);
                    draw(ui, index);
                });
            }
        });
    }
}

/// A label above a value that wraps anywhere, for long paths and ids.
pub fn key_value_block(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.label(RichText::new(key).size(12.0).color(MUTED));
    ui.add(egui::Label::new(RichText::new(value).size(12.5).color(TEXT)).wrap_mode(egui::TextWrapMode::Wrap));
    ui.add_space(4.0);
}

/// An on/off switch. Returns the response; `changed()` is set when it flips.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = vec2(38.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let accent = theme::active_accent(ui.ctx());
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    let track = if *on { accent } else { Color32::from_rgb(0x2A, 0x31, 0x3D) };
    ui.painter().rect_filled(rect, CornerRadius::same(255), track.gamma_multiply(0.35 + 0.65 * t));
    let knob_x = rect.left() + 10.0 + t * (size.x - 20.0);
    ui.painter().circle_filled(pos2(knob_x, rect.center().y), 7.0, if *on { Color32::WHITE } else { MUTED });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small square icon-style button with a text glyph; returns true when clicked.
pub fn glyph_button(ui: &mut egui::Ui, glyph: &str, tip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    let fill = if response.hovered() { CARD_HOVER } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(16.0),
        if response.hovered() { TEXT } else { MUTED },
    );
    response.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The small drawn icons used in nav & headers.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Home,
    Play,
    Globe,
    Car,
    Track,
    Camera,
    Paint,
    Layers,
    Sliders,
    Gear,
    Bulb,
    Clock,
    Chart,
    Replay,
    Grid,
    Pin,
}

/// Draw `icon` centred in `rect` with strokes of `color`.
#[allow(dead_code)]
pub fn draw_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let u = rect.width() / 20.0;
    let p = |x: f32, y: f32| c + vec2(x * u, y * u);
    let stroke = Stroke::new((1.6 * u).max(1.2), color);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], stroke);
    match icon {
        Icon::Home => {
            let roof = [p(-8.0, -0.5), p(0.0, -8.0), p(8.0, -0.5)];
            painter.add(Shape::line(roof.to_vec(), stroke));
            painter.add(Shape::line(vec![p(-5.5, 0.5), p(-5.5, 7.5), p(5.5, 7.5), p(5.5, 0.5)], stroke));
            painter.add(Shape::line(vec![p(-1.8, 7.5), p(-1.8, 3.0), p(1.8, 3.0), p(1.8, 7.5)], stroke));
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(vec![p(-4.0, -7.0), p(7.5, 0.0), p(-4.0, 7.0)], color, Stroke::NONE));
        }
        Icon::Globe => {
            painter.circle_stroke(c, 8.0 * u, stroke);
            painter.add(Shape::line(
                (0..=16)
                    .map(|i| {
                        let t = i as f32 / 16.0 * std::f32::consts::TAU;
                        p(3.6 * t.cos(), 8.0 * t.sin())
                    })
                    .collect(),
                stroke,
            ));
            line(p(-8.0, 0.0), p(8.0, 0.0));
        }
        Icon::Car => {
            painter.add(Shape::line(
                vec![
                    p(-9.0, 3.0),
                    p(-8.0, -1.0),
                    p(-4.5, -2.0),
                    p(-2.5, -6.0),
                    p(3.0, -6.0),
                    p(5.5, -2.0),
                    p(8.5, -1.0),
                    p(9.0, 3.0),
                    p(-9.0, 3.0),
                ],
                stroke,
            ));
            painter.circle_filled(p(-4.8, 4.5), 2.2 * u, color);
            painter.circle_filled(p(4.8, 4.5), 2.2 * u, color);
        }
        Icon::Track => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0 * std::f32::consts::TAU;
                    p(9.0 * t.cos(), 5.5 * t.sin() + 2.2 * (2.0 * t).sin())
                })
                .collect();
            painter.add(Shape::line(pts, stroke));
            line(p(-1.0, -7.5), p(-1.0, -3.5));
        }
        Icon::Camera => {
            painter.rect_stroke(
                Rect::from_center_size(c + vec2(0.0, 1.0 * u), vec2(18.0 * u, 12.0 * u)),
                CornerRadius::same((2.5 * u) as u8),
                stroke,
                StrokeKind::Middle,
            );
            painter.circle_stroke(p(0.0, 1.0), 3.4 * u, stroke);
            line(p(-4.0, -5.5), p(-1.0, -5.5));
        }
        Icon::Paint => {
            painter.add(Shape::line(
                vec![
                    p(0.0, -8.5),
                    p(5.5, -1.0),
                    p(5.5, 3.0),
                    p(0.0, 8.0),
                    p(-5.5, 3.0),
                    p(-5.5, -1.0),
                    p(0.0, -8.5),
                ],
                stroke,
            ));
            painter.circle_filled(p(0.0, 2.0), 2.0 * u, color);
        }
        Icon::Layers => {
            for dy in [-3.5, 0.5, 4.5] {
                painter.add(Shape::line(
                    vec![p(-8.0, dy), p(0.0, dy - 3.5), p(8.0, dy), p(0.0, dy + 3.5), p(-8.0, dy)],
                    stroke,
                ));
            }
        }
        Icon::Sliders => {
            for (y, x) in [(-5.5, -3.0), (0.0, 3.5), (5.5, -1.0)] {
                line(p(-8.5, y), p(8.5, y));
                painter.circle_filled(p(x, y), 2.4 * u, color);
            }
        }
        Icon::Gear => {
            painter.circle_stroke(c, 3.0 * u, stroke);
            painter.circle_stroke(c, 7.0 * u, stroke);
            for i in 0..8 {
                let t = i as f32 / 8.0 * std::f32::consts::TAU;
                line(p(7.0 * t.cos(), 7.0 * t.sin()), p(9.2 * t.cos(), 9.2 * t.sin()));
            }
        }
        Icon::Bulb => {
            painter.circle_stroke(p(0.0, -2.5), 5.5 * u, stroke);
            line(p(-2.5, 4.5), p(2.5, 4.5));
            line(p(-1.5, 7.5), p(1.5, 7.5));
            line(p(-2.5, 3.0), p(-2.5, 4.5));
            line(p(2.5, 3.0), p(2.5, 4.5));
        }
        Icon::Clock => {
            painter.circle_stroke(c, 8.5 * u, stroke);
            line(c, p(0.0, -5.0));
            line(c, p(4.0, 2.0));
        }
        Icon::Chart => {
            painter.add(Shape::line(vec![p(-8.5, 7.0), p(-8.5, -7.5)], stroke));
            painter.add(Shape::line(vec![p(-8.5, 7.0), p(8.5, 7.0)], stroke));
            painter.add(Shape::line(vec![p(-6.0, 3.0), p(-2.0, -2.0), p(1.5, 1.0), p(7.0, -5.5)], stroke));
        }
        Icon::Replay => {
            painter.circle_stroke(c, 8.5 * u, stroke);
            painter.add(Shape::convex_polygon(vec![p(-2.5, -4.0), p(4.5, 0.0), p(-2.5, 4.0)], color, Stroke::NONE));
        }
        Icon::Grid => {
            for (x, y) in [(-4.5, -4.5), (4.5, -4.5), (-4.5, 4.5), (4.5, 4.5)] {
                painter.rect_filled(
                    Rect::from_center_size(p(x, y), vec2(6.0 * u, 6.0 * u)),
                    CornerRadius::same((1.5 * u) as u8),
                    color,
                );
            }
        }
        Icon::Pin => {
            painter.circle_filled(p(0.0, -3.5), 3.8 * u, color);
            line(p(0.0, 0.0), p(0.0, 8.5));
        }
    }
}

/// A primary sidebar entry: a drawn icon, the label and an optional count on the right.
#[allow(dead_code)]
pub fn nav_icon_item(ui: &mut egui::Ui, selected: bool, icon: Icon, label: &str, badge: Option<&str>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 31.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, CornerRadius::same(9), theme::ACCENT.gamma_multiply(0.14));
            let bar = Rect::from_center_size(rect.left_center() + vec2(2.0, 0.0), vec2(3.0, 18.0));
            painter.rect_filled(bar, CornerRadius::same(2), theme::ACCENT);
        } else if hovered {
            painter.rect_filled(rect, CornerRadius::same(9), CARD_HOVER);
        }
        let color = if selected { theme::ACCENT } else if hovered { TEXT } else { MUTED };
        let icon_rect = Rect::from_center_size(rect.left_center() + vec2(24.0, 0.0), vec2(20.0, 20.0));
        draw_icon(painter, icon_rect, icon, color);
        let text_color = if selected || hovered { TEXT } else { MUTED };
        let font = if selected { theme::bold(14.5) } else { FontId::proportional(14.5) };
        painter.text(rect.left_center() + vec2(46.0, 0.0), Align2::LEFT_CENTER, label, font, text_color);
        if let Some(badge) = badge {
            painter.text(rect.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, badge, theme::mono(11.5), FAINT);
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// AAA Motorsport Widget Kit (Spec 1)
// ---------------------------------------------------------------------------

/// Horizontal motorsport tabs with a smoothly sliding accent underline.
/// Returns `true` when the active tab changed.
pub fn animated_tabs<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id_salt: &str,
    current: &mut T,
    tabs: &[(T, &str, Option<&str>)],
) -> bool {
    let mut changed = false;
    let accent = theme::active_accent(ui.ctx());
    let base_id = ui.make_persistent_id(id_salt);
    let mut active_rect: Option<Rect> = None;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (value, label, badge) in tabs {
            let selected = *current == *value;
            let label_font = theme::display(15.5);
            let label_galley = ui.painter().layout_no_wrap(label.to_uppercase(), label_font, Color32::WHITE);
            let badge_galley = badge.map(|b| ui.painter().layout_no_wrap(b.to_string(), theme::mono(11.0), MUTED));

            let pad_x = 12.0;
            let badge_w = badge_galley.as_ref().map_or(0.0, |g| g.size().x + 14.0);
            let width = label_galley.size().x + pad_x * 2.0 + badge_w;
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());

            let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
            if hover > 0.0 && !selected {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(7),
                    theme::SURFACE_RAISED.gamma_multiply(0.55 * hover),
                );
            }

            let text_color = if selected {
                Color32::WHITE
            } else {
                MUTED.lerp_to_gamma(TEXT, hover)
            };
            let text_pos = pos2(rect.left() + pad_x, rect.center().y - label_galley.size().y * 0.5);
            ui.painter().galley(text_pos, label_galley, text_color);

            if let Some(bg) = badge_galley {
                let badge_rect = Rect::from_center_size(
                    pos2(rect.right() - pad_x - bg.size().x * 0.5, rect.center().y),
                    vec2(bg.size().x + 8.0, 17.0),
                );
                ui.painter().rect_filled(
                    badge_rect,
                    CornerRadius::same(255),
                    if selected { accent.gamma_multiply(0.22) } else { theme::SURFACE_RAISED },
                );
                ui.painter().galley(
                    badge_rect.center() - bg.size() * 0.5,
                    bg,
                    if selected { accent } else { FAINT },
                );
            }

            if selected {
                active_rect = Some(rect);
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !selected {
                *current = *value;
                changed = true;
            }
        }
    });

    if let Some(target) = active_rect {
        let x = ui.ctx().animate_value_with_time(base_id.with("x"), target.left(), 0.16);
        let w = ui.ctx().animate_value_with_time(base_id.with("w"), target.width(), 0.16);
        let bar = Rect::from_min_size(pos2(x + 8.0, target.bottom() - 2.5), vec2((w - 16.0).max(8.0), 2.5));
        // Soft glow + crisp bar.
        ui.painter().rect_filled(bar.expand2(vec2(2.0, 1.5)), CornerRadius::same(3), accent.gamma_multiply(0.28));
        ui.painter().rect_filled(bar, CornerRadius::same(2), accent);
    }

    changed
}

/// Primary motorsport CTA button with subtle 2-layer glow, hover sheen and crisp disabled state.
pub fn cta_button(ui: &mut egui::Ui, label: &str, size: egui::Vec2, enabled: bool) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let desired = vec2(size.x.max(96.0), size.y.max(36.0));
    let (rect, response) = ui.allocate_exact_size(desired, sense);
    if ui.is_rect_visible(rect) {
        let accent = theme::active_accent(ui.ctx());
        let hover = ui.ctx().animate_bool_with_time(response.id, enabled && response.hovered(), 0.12);
        let down = enabled && response.is_pointer_button_down_on();
        let round = CornerRadius::same(10);
        let painter = ui.painter();

        if enabled {
            // Subtle outer glow.
            painter.rect_filled(
                rect.expand(3.0 + 1.5 * hover),
                CornerRadius::same(13),
                accent.gamma_multiply(0.14 + 0.12 * hover),
            );
            let fill = if down {
                accent.gamma_multiply(0.85)
            } else {
                accent.lerp_to_gamma(Color32::WHITE, 0.10 * hover)
            };
            painter.rect_filled(rect, round, fill);
            theme::paint_top_specular(painter, rect, 10.0);
        } else {
            painter.rect_filled(rect, round, theme::SURFACE_RAISED);
            painter.rect_stroke(rect, round, Stroke::new(1.0, theme::BORDER_SUBTLE), StrokeKind::Inside);
        }

        let text_color = if enabled { theme::SURFACE_VOID } else { FAINT };
        painter.text(rect.center(), Align2::CENTER_CENTER, label.to_uppercase(), theme::display(16.5), text_color);
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Animated horizontal spec bar whose fill smoothly interpolates when switching cars.
pub fn spec_bar_animated(
    ui: &mut egui::Ui,
    id_salt: &str,
    label: &str,
    value_text: &str,
    target_fill: f32,
    accent: Color32,
) {
    let clamped = target_fill.clamp(0.0, 1.0);
    let anim_id = ui.make_persistent_id(("spec_bar", id_salt, label));
    let fill = ui.ctx().animate_value_with_time(anim_id, clamped, 0.22);

    ui.horizontal(|ui| {
        ui.label(RichText::new(label.to_uppercase()).font(theme::display(12.5)).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value_text).font(theme::mono(13.0)).color(TEXT));
        });
    });

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(3), theme::SURFACE_VOID);
        let fill_w = (rect.width() * fill).max(if fill > 0.01 { 4.0 } else { 0.0 });
        if fill_w > 0.0 {
            let bar = Rect::from_min_size(rect.min, vec2(fill_w, rect.height()));
            painter.rect_filled(bar, CornerRadius::same(3), accent.gamma_multiply(0.55 + 0.45 * fill));
            // Bright leading tip.
            let tip = Rect::from_min_size(pos2(bar.right() - 3.0, bar.top()), vec2(3.0, bar.height()));
            painter.rect_filled(tip, CornerRadius::same(2), Color32::WHITE.gamma_multiply(0.75));
        }
    }
    ui.add_space(6.0);
}

/// N-axis (typically 6-axis) motorsport performance radar chart painted with `egui::Painter`.
pub fn radar_chart(ui: &mut egui::Ui, size: f32, axes: &[(&str, f32)], accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    if !ui.is_rect_visible(rect) || axes.len() < 3 {
        return;
    }
    let painter = ui.painter();
    let center = rect.center();
    let radius = (size * 0.5 - 26.0).max(20.0);
    let n = axes.len();

    let point_at = |index: usize, r: f32| -> Pos2 {
        let angle = -std::f32::consts::FRAC_PI_2 + (index as f32 / n as f32) * std::f32::consts::TAU;
        center + vec2(angle.cos() * r, angle.sin() * r)
    };

    // Concentric web rings.
    for step in 1..=4 {
        let r = radius * (step as f32 / 4.0);
        let mut ring: Vec<Pos2> = (0..n).map(|i| point_at(i, r)).collect();
        ring.push(ring[0]);
        painter.add(Shape::line(ring, Stroke::new(1.0, theme::BORDER_SUBTLE)));
    }
    // Axis spokes + micro-labels.
    for (i, (label, _)) in axes.iter().enumerate() {
        let outer = point_at(i, radius);
        painter.line_segment([center, outer], Stroke::new(1.0, theme::BORDER_SUBTLE));
        let label_pos = point_at(i, radius + 15.0);
        painter.text(label_pos, Align2::CENTER_CENTER, label.to_uppercase(), theme::display(11.0), MUTED);
    }

    // Data polygon.
    let poly: Vec<Pos2> = axes
        .iter()
        .enumerate()
        .map(|(i, (_, v))| point_at(i, radius * v.clamp(0.05, 1.0)))
        .collect();
    painter.add(Shape::convex_polygon(
        poly.clone(),
        accent.gamma_multiply(0.22),
        Stroke::new(1.8, accent),
    ));
    for pt in poly {
        painter.circle_filled(pt, 3.0, Color32::WHITE);
        painter.circle_stroke(pt, 3.0, Stroke::new(1.2, accent));
    }
}

/// Interactive semicircular solar arc time-of-day dial (08:00..=18:00 default AC range,
/// supports full 00:00..=23:59). Returns `true` when `minutes_from_midnight` changed.
pub fn solar_dial(ui: &mut egui::Ui, minutes_from_midnight: &mut u32) -> bool {
    let size = vec2(ui.available_width().clamp(180.0, 260.0), 112.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let mut changed = false;

    let center = pos2(rect.center().x, rect.bottom() - 18.0);
    let radius = (rect.width() * 0.42).min(rect.height() - 28.0).max(40.0);

    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let dx = pointer.x - center.x;
        let dy = (center.y - pointer.y).max(0.0);
        // Angle from left horizon (PI) across zenith (PI/2) to right horizon (0).
        let angle = dy.atan2(dx).clamp(0.0, std::f32::consts::PI);
        let frac = 1.0 - (angle / std::f32::consts::PI);
        // Map 0.0..=1.0 to 06:00 (360) ..= 20:00 (1200).
        let next = (360.0 + frac * 840.0).round() as u32;
        let snapped = (next / 15) * 15;
        if snapped != *minutes_from_midnight {
            *minutes_from_midnight = snapped;
            changed = true;
        }
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let accent = theme::active_accent(ui.ctx());
        let frac = ((*minutes_from_midnight as f32 - 360.0) / 840.0).clamp(0.0, 1.0);

        // Background sky-tinted card.
        let sky = if frac < 0.2 || frac > 0.8 {
            Color32::from_rgb(0x28, 0x1A, 0x24) // dawn/dusk
        } else {
            Color32::from_rgb(0x13, 0x1E, 0x2E) // daylight
        };
        painter.rect_filled(rect, CornerRadius::same(12), sky);
        painter.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, theme::BORDER_SUBTLE), StrokeKind::Inside);

        // Horizon baseline.
        painter.line_segment(
            [pos2(center.x - radius - 10.0, center.y), pos2(center.x + radius + 10.0, center.y)],
            Stroke::new(1.0, theme::BORDER_ACTIVE),
        );

        // Semicircular track.
        let arc_pts: Vec<Pos2> = (0..=32)
            .map(|i| {
                let t = i as f32 / 32.0;
                let a = std::f32::consts::PI * (1.0 - t);
                pos2(center.x + a.cos() * radius, center.y - a.sin() * radius)
            })
            .collect();
        painter.add(Shape::line(arc_pts, Stroke::new(2.0, theme::BORDER_ACTIVE)));

        // Sun knob.
        let angle = std::f32::consts::PI * (1.0 - frac);
        let sun = pos2(center.x + angle.cos() * radius, center.y - angle.sin() * radius);
        painter.circle_filled(sun, 12.0, accent.gamma_multiply(0.25));
        painter.circle_filled(sun, 6.5, Color32::from_rgb(0xFF, 0xD1, 0x66));
        painter.circle_stroke(sun, 6.5, Stroke::new(1.5, Color32::WHITE));

        // Tabular HH:MM readout.
        let h = (*minutes_from_midnight / 60) % 24;
        let m = *minutes_from_midnight % 60;
        painter.text(
            pos2(center.x, center.y - radius * 0.36),
            Align2::CENTER_CENTER,
            format!("{h:02}:{m:02}"),
            theme::mono(20.0),
            Color32::WHITE,
        );
        painter.text(pos2(center.x - radius, center.y + 10.0), Align2::CENTER_CENTER, "06:00", theme::mono(10.0), FAINT);
        painter.text(pos2(center.x + radius, center.y + 10.0), Align2::CENTER_CENTER, "20:00", theme::mono(10.0), FAINT);
    }

    response.on_hover_cursor(egui::CursorIcon::Grab);
    changed
}

/// Pulsing dark-carbon shimmer placeholder while images or async data load.
pub fn skeleton_rect(ui: &mut egui::Ui, size: egui::Vec2, radius: u8) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let t = ui.input(|i| i.time) as f32;
        let pulse = 0.5 + 0.5 * (t * 3.0).sin();
        let fill = theme::SURFACE_ELEVATED.lerp_to_gamma(theme::SURFACE_RAISED, 0.35 + 0.65 * pulse);
        ui.painter().rect_filled(rect, CornerRadius::same(radius), fill);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
}

/// Multi-stop mesh scrim for crisp typography over full-bleed hero car/track art.
pub fn paint_hero_scrim(painter: &egui::Painter, rect: Rect, bottom_alpha: u8, left_alpha: u8) {
    // Bottom-up vignette (bottom 55% of hero).
    let mid_y = rect.bottom() - rect.height() * 0.55;
    let mut bottom_mesh = Mesh::default();
    let clear = Color32::from_black_alpha(0);
    let dark_b = Color32::from_black_alpha(bottom_alpha);
    bottom_mesh.colored_vertex(pos2(rect.left(), mid_y), clear);
    bottom_mesh.colored_vertex(pos2(rect.right(), mid_y), clear);
    bottom_mesh.colored_vertex(rect.right_bottom(), dark_b);
    bottom_mesh.colored_vertex(rect.left_bottom(), dark_b);
    bottom_mesh.add_triangle(0, 1, 2);
    bottom_mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(bottom_mesh));

    // Left-to-right readability scrim (left 65% of hero).
    let mid_x = rect.left() + rect.width() * 0.65;
    let mut left_mesh = Mesh::default();
    let dark_l = Color32::from_black_alpha(left_alpha);
    left_mesh.colored_vertex(rect.left_top(), dark_l);
    left_mesh.colored_vertex(rect.left_bottom(), dark_l);
    left_mesh.colored_vertex(pos2(mid_x, rect.bottom()), clear);
    left_mesh.colored_vertex(pos2(mid_x, rect.top()), clear);
    left_mesh.add_triangle(0, 1, 2);
    left_mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(left_mesh));
}

/// Shared sticky bottom bar state for every INI/config editor (`Game Settings`, `CSP Config`, `Setups`, `Controls`).
#[derive(Debug, Clone, Default)]
pub struct PendingChangesBar {
    pub changed_count: usize,
    pub summary: String,
    pub can_save: bool,
    pub blocked_reason: Option<String>,
}

/// User action returned by [`pending_changes_bar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    None,
    Save,
    RevertAll,
    ToggleDiff,
}

/// Shared sticky pending-changes action bar. Renders nothing when `changed_count == 0`.
pub fn pending_changes_bar(ui: &mut egui::Ui, state: &PendingChangesBar) -> PendingAction {
    if state.changed_count == 0 {
        return PendingAction::None;
    }
    let mut action = PendingAction::None;
    let accent = theme::active_accent(ui.ctx());
    theme::card_glass()
        .stroke(Stroke::new(1.0, accent.gamma_multiply(0.65)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                pill(ui, &format!("{} modified", state.changed_count), accent);
                let text = if state.summary.is_empty() {
                    "Unsaved configuration changes"
                } else {
                    state.summary.as_str()
                };
                ui.label(RichText::new(text).color(TEXT));
                if let Some(reason) = &state.blocked_reason {
                    ui.label(RichText::new(format!("·  {reason}")).size(12.5).color(theme::WARN));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if cta_button(ui, "Save changes", vec2(128.0, 34.0), state.can_save).clicked() {
                        action = PendingAction::Save;
                    }
                    if ui.add(egui::Button::new("Revert all").min_size(vec2(86.0, 34.0))).clicked() {
                        action = PendingAction::RevertAll;
                    }
                    if ui.add(egui::Button::new("Diff").min_size(vec2(64.0, 34.0))).clicked() {
                        action = PendingAction::ToggleDiff;
                    }
                });
            });
        });
    action
}

/// Standardized obsidian container slot for 3D viewports (`Viewport3D` / `PaintCallback`)
/// with studio corner frame, top-left glass overlay slot, and inner rect handed to `render_stage`.
pub fn viewport_3d_frame(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    overlay_top_left: impl FnOnce(&mut egui::Ui),
    render_stage: impl FnOnce(&mut egui::Ui, Rect),
) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let round = CornerRadius::same(theme::RADIUS);
    ui.painter().rect_filled(rect, round, theme::SURFACE_VOID);

    let inner = rect.shrink(1.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        render_stage(ui, inner);
    });

    // Crisp outer hairline + top specular edge over the viewport border.
    ui.painter().rect_stroke(rect, round, Stroke::new(1.0, theme::BORDER_ACTIVE), StrokeKind::Inside);
    theme::paint_top_specular(ui.painter(), rect, f32::from(theme::RADIUS));

    // Top-left floating HUD overlay slot.
    let overlay_rect = Rect::from_min_max(rect.left_top() + vec2(12.0, 12.0), rect.right_bottom() - vec2(12.0, 12.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(overlay_rect), |ui| {
        overlay_top_left(ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aaa_widgets_render_headlessly() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut tab = 0usize;
        let mut mins = 870u32; // 14:30
        let pending = PendingChangesBar {
            changed_count: 3,
            summary: "3 video settings changed".into(),
            can_save: true,
            blocked_reason: None,
        };

        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            animated_tabs(ui, "test_tabs", &mut tab, &[(0, "Showroom", None), (1, "Cars", Some("178"))]);
            let _ = cta_button(ui, "DRIVE", vec2(140.0, 40.0), true);
            spec_bar_animated(ui, "car1", "Power", "570 bhp", 0.57, theme::ACCENT);
            radar_chart(
                ui,
                180.0,
                &[("PWR", 0.7), ("TRQ", 0.65), ("WGT", 0.8), ("SPD", 0.75), ("ACC", 0.82), ("P/W", 0.78)],
                theme::ACCENT,
            );
            let _ = solar_dial(ui, &mut mins);
            skeleton_rect(ui, vec2(200.0, 40.0), 8);
            paint_hero_scrim(ui.painter(), Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0)), 210, 180);
            let act = pending_changes_bar(ui, &pending);
            assert_eq!(act, PendingAction::None);
            viewport_3d_frame(
                ui,
                vec2(320.0, 180.0),
                |ui| {
                    pill(ui, "ORBIT CAM", theme::INFO);
                },
                |ui, r| {
                    ui.painter().rect_filled(r, 0, theme::SURFACE_VOID);
                },
            );
        });
        out.textures_delta.clear();
    }
}
