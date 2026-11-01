//! Colors, typography, spacing and widget styling for the whole app.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use std::sync::Arc;

// Surfaces, darkest to lightest.
pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x11);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x0F, 0x12, 0x17);
pub const CARD: Color32 = Color32::from_rgb(0x15, 0x19, 0x20);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x1C, 0x21, 0x2B);
pub const CARD_SELECTED: Color32 = Color32::from_rgb(0x24, 0x1C, 0x15);
pub const INPUT: Color32 = Color32::from_rgb(0x0D, 0x10, 0x15);
pub const BORDER: Color32 = Color32::from_rgb(0x25, 0x2B, 0x36);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x36, 0x3E, 0x4D);

// Text.
pub const TEXT: Color32 = Color32::from_rgb(0xEC, 0xEE, 0xF3);
pub const MUTED: Color32 = Color32::from_rgb(0x8E, 0x97, 0xAA);
pub const FAINT: Color32 = Color32::from_rgb(0x5F, 0x68, 0x7A);

// Signal colors.
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x7A, 0x1A);
pub const OK: Color32 = Color32::from_rgb(0x3D, 0xD6, 0x8C);
pub const WARN: Color32 = Color32::from_rgb(0xF5, 0xB8, 0x42);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x6C);
pub const INFO: Color32 = Color32::from_rgb(0x5B, 0xA8, 0xFF);

pub const RADIUS: u8 = 14;
pub const RADIUS_SMALL: u8 = 8;

/// Segoe UI looks far better than egui's default font and ships with Windows.
/// Loaded at runtime in front of the built-in fonts, which stay as fallback for symbols.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, file, family) in [
        ("segoe", "segoeui.ttf", FontFamily::Proportional),
        ("segoe-bold", "segoeuib.ttf", FontFamily::Name("bold".into())),
    ] {
        let path = std::path::Path::new(r"C:\Windows\Fonts").join(file);
        let Ok(bytes) = std::fs::read(path) else { continue };
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
        let list = fonts.families.entry(family.clone()).or_default();
        list.insert(0, name.to_owned());
        if family != FontFamily::Proportional {
            // Fallbacks for glyphs the bold face lacks.
            list.push("segoe".to_owned());
        }
    }
    // Symbols (stars, arrows) that Segoe UI lacks.
    if let Ok(bytes) = std::fs::read(std::path::Path::new(r"C:\Windows\Fonts").join("seguisym.ttf")) {
        fonts.font_data.insert("segoe-sym".to_owned(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Name("bold".into())] {
            fonts.families.entry(family).or_default().push("segoe-sym".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    // The design is dark only; without this a light Windows theme picks the light style slot,
    // which leaves text fields white and implicit text dark gray.
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut style = (*ctx.global_style()).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.window_shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(140) };
    visuals.popup_shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(130) };
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.extreme_bg_color = INPUT;
    visuals.faint_bg_color = Color32::from_rgb(0x12, 0x16, 0x1C);
    visuals.code_bg_color = INPUT;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.30);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_corner_radius = CornerRadius::same(RADIUS);
    visuals.striped = false;

    let round = CornerRadius::same(RADIUS_SMALL);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = round;
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);

    let button = Color32::from_rgb(0x1D, 0x22, 0x2C);
    visuals.widgets.inactive.weak_bg_fill = button;
    visuals.widgets.inactive.bg_fill = button;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    let button_hover = Color32::from_rgb(0x27, 0x2E, 0x3B);
    visuals.widgets.hovered.weak_bg_fill = button_hover;
    visuals.widgets.hovered.bg_fill = button_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.7));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);

    visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.45);
    visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.45);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);

    visuals.widgets.open.weak_bg_fill = button_hover;
    visuals.widgets.open.bg_fill = button_hover;

    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;
    style.spacing.scroll.foreground_color = false;

    // Typography scale.
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(24.0));
    style.text_styles.insert(TextStyle::Body, FontId::proportional(14.5));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(14.5));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.0));
    egui_extras::install_image_loaders(ctx);
    ctx.set_global_style(style);
}

/// Bold text in the bold face when it is installed (falls back to the regular face).
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(18))
}

/// A flatter, tighter card for use inside other cards or side panels.
pub fn card_flat() -> egui::Frame {
    egui::Frame::new()
        .fill(BG.gamma_multiply(0.6))
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
}

pub fn pill_color(text: &str) -> Color32 {
    match text {
        "running" | "installed" | "known" | "OK" => OK,
        "not running" | "not installed" | "unrecognized" | "invalid" => WARN,
        _ => INFO,
    }
}
