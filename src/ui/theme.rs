//! Colors, spacing and widget styling for the whole app.

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

pub const BG: Color32 = Color32::from_rgb(0x0C, 0x0E, 0x12);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x11, 0x14, 0x1A);
pub const CARD: Color32 = Color32::from_rgb(0x17, 0x1B, 0x23);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x1E, 0x23, 0x2E);
pub const CARD_SELECTED: Color32 = Color32::from_rgb(0x2A, 0x20, 0x18);
pub const BORDER: Color32 = Color32::from_rgb(0x2A, 0x30, 0x3C);
pub const TEXT: Color32 = Color32::from_rgb(0xE9, 0xEB, 0xF1);
pub const MUTED: Color32 = Color32::from_rgb(0x8B, 0x94, 0xA8);
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x7A, 0x1A);
pub const OK: Color32 = Color32::from_rgb(0x3D, 0xD6, 0x8C);
pub const WARN: Color32 = Color32::from_rgb(0xF5, 0xB8, 0x42);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x6C);
pub const INFO: Color32 = Color32::from_rgb(0x5B, 0xA8, 0xFF);

pub const RADIUS: u8 = 12;

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.extreme_bg_color = Color32::from_rgb(0x08, 0x0A, 0x0D);
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_corner_radius = CornerRadius::same(RADIUS);

    let round = CornerRadius::same(8);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = round;
    }
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x20, 0x25, 0x30);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x20, 0x25, 0x30);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2A, 0x30, 0x3E);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x2A, 0x30, 0x3E);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
    visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.5);
    visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.5);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    ctx.set_global_style(style);
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(16))
}

pub fn pill_color(text: &str) -> Color32 {
    match text {
        "running" | "installed" | "known" | "OK" => OK,
        "not running" | "not installed" | "unrecognized" | "invalid" => WARN,
        _ => INFO,
    }
}
