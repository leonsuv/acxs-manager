//! Reusable pieces: pills, stat tiles, section headers, nav items and clickable list cards.

use super::theme::{self, BG, CARD_HOVER, CARD_SELECTED, FAINT, MUTED, TEXT};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Margin, Rect, RichText, Sense, Shape, Stroke, vec2};

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

/// Big number with a caption, used in the home dashboard.
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(label).size(12.0).color(MUTED));
        ui.label(RichText::new(value).font(theme::bold(22.0)).color(color));
    });
}

pub fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).font(theme::bold(26.0)));
    ui.label(RichText::new(subtitle).color(MUTED));
    ui.add_space(14.0);
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).font(theme::bold(13.5)).color(MUTED));
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
    let fill = if selected { CARD_SELECTED } else { theme::CARD };
    let frame = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(
            1.0,
            if selected { theme::ACCENT.gamma_multiply(0.7) } else { theme::BORDER },
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
pub fn nav_section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.0);
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
            painter.text(rect.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, badge, FontId::proportional(12.0), FAINT);
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A collapsible group header for the sidebar tool list. Returns true when toggled.
pub fn nav_group(ui: &mut egui::Ui, open: bool, label: &str, count: usize, holds_active: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 31.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(8), CARD_HOVER.gamma_multiply(0.6));
        }
        let color = if holds_active { theme::ACCENT } else if response.hovered() { TEXT } else { MUTED };
        // Chevron: points right when closed, down when open.
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
            FontId::proportional(12.0),
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
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    let track = if *on { theme::ACCENT } else { Color32::from_rgb(0x2A, 0x31, 0x3D) };
    ui.painter().rect_filled(rect, CornerRadius::same(255), track.gamma_multiply(0.35 + 0.65 * t));
    let knob_x = rect.left() + 10.0 + t * (size.x - 20.0);
    ui.painter().circle_filled(egui::pos2(knob_x, rect.center().y), 7.0, if *on { Color32::WHITE } else { MUTED });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small square icon-style button with a text glyph; returns true when clicked.
pub fn glyph_button(ui: &mut egui::Ui, glyph: &str, tip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    let fill = if response.hovered() { CARD_HOVER } else { Color32::TRANSPARENT };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(16.0), if response.hovered() { TEXT } else { MUTED });
    response.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The small drawn icons of the sidebar.
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
pub fn draw_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let u = rect.width() / 20.0; // 20 units across
    let p = |x: f32, y: f32| c + vec2(x * u, y * u);
    let stroke = Stroke::new((1.6 * u).max(1.2), color);
    let line = |a: egui::Pos2, b: egui::Pos2| painter.line_segment([a, b], stroke);
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
            painter.add(Shape::line((0..=16).map(|i| {
                let t = i as f32 / 16.0 * std::f32::consts::TAU;
                p(3.6 * t.cos(), 8.0 * t.sin())
            }).collect(), stroke));
            line(p(-8.0, 0.0), p(8.0, 0.0));
        }
        Icon::Car => {
            painter.add(Shape::line(
                vec![p(-9.0, 3.0), p(-8.0, -1.0), p(-4.5, -2.0), p(-2.5, -6.0), p(3.0, -6.0), p(5.5, -2.0), p(8.5, -1.0), p(9.0, 3.0), p(-9.0, 3.0)],
                stroke,
            ));
            painter.circle_filled(p(-4.8, 4.5), 2.2 * u, color);
            painter.circle_filled(p(4.8, 4.5), 2.2 * u, color);
        }
        Icon::Track => {
            let pts: Vec<egui::Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0 * std::f32::consts::TAU;
                    p(9.0 * t.cos(), 5.5 * t.sin() + 2.2 * (2.0 * t).sin())
                })
                .collect();
            painter.add(Shape::line(pts, stroke));
            line(p(-1.0, -7.5), p(-1.0, -3.5));
        }
        Icon::Camera => {
            painter.rect_stroke(Rect::from_center_size(c + vec2(0.0, 1.0 * u), vec2(18.0 * u, 12.0 * u)), CornerRadius::same((2.5 * u) as u8), stroke, egui::StrokeKind::Middle);
            painter.circle_stroke(p(0.0, 1.0), 3.4 * u, stroke);
            line(p(-4.0, -5.5), p(-1.0, -5.5));
        }
        Icon::Paint => {
            painter.add(Shape::line(vec![p(0.0, -8.5), p(5.5, -1.0), p(5.5, 3.0), p(0.0, 8.0), p(-5.5, 3.0), p(-5.5, -1.0), p(0.0, -8.5)], stroke));
            painter.circle_filled(p(0.0, 2.0), 2.0 * u, color);
        }
        Icon::Layers => {
            for dy in [-3.5, 0.5, 4.5] {
                painter.add(Shape::line(vec![p(-8.0, dy), p(0.0, dy - 3.5), p(8.0, dy), p(0.0, dy + 3.5), p(-8.0, dy)], stroke));
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
                painter.rect_filled(Rect::from_center_size(p(x, y), vec2(6.0 * u, 6.0 * u)), CornerRadius::same((1.5 * u) as u8), color);
            }
        }
        Icon::Pin => {
            painter.circle_filled(p(0.0, -3.5), 3.8 * u, color);
            line(p(0.0, 0.0), p(0.0, 8.5));
        }
    }
}

/// A primary sidebar entry: a drawn icon, the label and an optional count on the right.
pub fn nav_icon_item(ui: &mut egui::Ui, selected: bool, icon: Icon, label: &str, badge: Option<&str>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
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
            painter.text(rect.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, badge, FontId::proportional(12.0), FAINT);
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
