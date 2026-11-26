//! Colors, typography, surface elevation, accent presets and widget styling for the whole app.

use acxs_manager::settings::AccentPreset;
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Rect, RichText, Shadow,
    Stroke, TextStyle, pos2,
};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Legacy surface & signal constants (preserved for zero breakage across pages)
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// AAA 5-level obsidian/carbon surface elevation & motorsport telemetry tokens
// ---------------------------------------------------------------------------
/// Deepest backdrop behind hero canvases, 3D viewports and telemetry plots.
pub const SURFACE_VOID: Color32 = Color32::from_rgb(0x07, 0x09, 0x0D);
/// Primary application canvas.
pub const SURFACE_BASE: Color32 = Color32::from_rgb(0x0C, 0x0F, 0x14);
/// Primary cards, drawers and inspector decks.
pub const SURFACE_ELEVATED: Color32 = Color32::from_rgb(0x13, 0x17, 0x20);
/// Interactive tiles, hover states and active pill surfaces.
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(0x1A, 0x20, 0x2C);
/// Translucent obsidian glass for top bar, floating overlays and paddock dock.
pub const SURFACE_GLASS: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x10, 0x17, 218);

/// Subtle hairline border for elevated carbon cards.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);
/// Stronger hairline border for interactive/focused elements.
pub const BORDER_ACTIVE: Color32 = Color32::from_rgba_premultiplied(40, 40, 40, 40);
/// 1 px top specular highlight simulating a machined bevel on elevated cards.
pub const TOP_HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 22);

/// Overall-best / purple sector signal color.
pub const SIGNAL_PURPLE: Color32 = Color32::from_rgb(0xC0, 0x6E, 0xFF);

// Pit-wall telemetry channel colors.
pub const CH_SPEED: Color32 = Color32::from_rgb(0x00, 0xD1, 0xFF);
pub const CH_THROTTLE: Color32 = Color32::from_rgb(0x2E, 0xE5, 0x8D);
pub const CH_BRAKE: Color32 = Color32::from_rgb(0xFF, 0x45, 0x57);
pub const CH_STEER: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x20);
pub const CH_GEAR: Color32 = Color32::from_rgb(0xC0, 0x6E, 0xFF);

// Bundled SIL-OFL fonts (zero runtime disk I/O, identical rendering on Windows & macOS).
const FONT_DISPLAY: &[u8] = include_bytes!("../../assets/fonts/BarlowCondensed-Bold.ttf");
const FONT_SANS_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const FONT_SANS_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
const FONT_MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Medium.ttf");

const ACTIVE_ACCENT_ID: &str = "acxs_active_accent";

/// Resolve the primary accent color for a user-selected [`AccentPreset`].
pub fn accent_color(preset: AccentPreset) -> Color32 {
    match preset {
        AccentPreset::MotorsportRed => Color32::from_rgb(0xFF, 0x5A, 0x1F),
        AccentPreset::TelemetryCyan => Color32::from_rgb(0x00, 0xC8, 0xFF),
        AccentPreset::AmberGold => Color32::from_rgb(0xFF, 0xB0, 0x20),
        AccentPreset::Emerald => Color32::from_rgb(0x2E, 0xE5, 0x8D),
    }
}

/// Read the active accent color stored on the egui context (falls back to `ACCENT`).
pub fn active_accent(ctx: &egui::Context) -> Color32 {
    ctx.data(|d| d.get_temp::<Color32>(egui::Id::unique(ACTIVE_ACCENT_ID))).unwrap_or(ACCENT)
}

/// Install bundled Barlow Condensed (display), Inter (UI sans) and JetBrains Mono (telemetry)
/// in front of egui's built-in fallback fonts so Unicode symbols never render as tofu.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert("barlow-display".into(), Arc::new(FontData::from_static(FONT_DISPLAY)));
    fonts.font_data.insert("inter-regular".into(), Arc::new(FontData::from_static(FONT_SANS_REGULAR)));
    fonts.font_data.insert("inter-semibold".into(), Arc::new(FontData::from_static(FONT_SANS_SEMIBOLD)));
    fonts.font_data.insert("jetbrains-mono".into(), Arc::new(FontData::from_static(FONT_MONO)));

    let base_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    // Proportional: Inter Regular -> built-in fallbacks.
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "inter-regular".into());

    // Bold: Inter SemiBold -> Inter Regular -> built-in fallbacks.
    let mut bold_chain = vec!["inter-semibold".to_string(), "inter-regular".to_string()];
    bold_chain.extend(base_prop.iter().cloned());
    fonts.families.insert(FontFamily::Name("bold".into()), bold_chain);

    // Display: Barlow Condensed Bold -> Inter SemiBold -> Inter Regular -> built-in fallbacks.
    let mut display_chain = vec![
        "barlow-display".to_string(),
        "inter-semibold".to_string(),
        "inter-regular".to_string(),
    ];
    display_chain.extend(base_prop);
    fonts.families.insert(FontFamily::Name("display".into()), display_chain);

    // Monospace: JetBrains Mono Medium -> built-in fallbacks.
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.insert(0, "jetbrains-mono".into());

    ctx.set_fonts(fonts);
}

fn build_visuals(accent: Color32) -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = SURFACE_BASE;
    visuals.window_fill = SURFACE_ELEVATED;
    visuals.window_stroke = Stroke::new(1.0, BORDER_ACTIVE);
    visuals.window_shadow = Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(165) };
    visuals.popup_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(150) };
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.extreme_bg_color = SURFACE_VOID;
    visuals.faint_bg_color = Color32::from_rgb(0x10, 0x14, 0x1B);
    visuals.code_bg_color = SURFACE_VOID;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = accent;
    visuals.selection.bg_fill = accent.gamma_multiply(0.30);
    visuals.selection.stroke = Stroke::new(1.0, accent);
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
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);

    let button = SURFACE_RAISED;
    visuals.widgets.inactive.weak_bg_fill = button;
    visuals.widgets.inactive.bg_fill = button;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    let button_hover = Color32::from_rgb(0x24, 0x2B, 0x3A);
    visuals.widgets.hovered.weak_bg_fill = button_hover;
    visuals.widgets.hovered.bg_fill = button_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.75));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);

    visuals.widgets.active.weak_bg_fill = accent.gamma_multiply(0.45);
    visuals.widgets.active.bg_fill = accent.gamma_multiply(0.45);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);

    visuals.widgets.open.weak_bg_fill = button_hover;
    visuals.widgets.open.bg_fill = button_hover;

    visuals
}

/// Apply the full theme (fonts, visuals, spacing, image loaders) using the default accent.
pub fn apply(ctx: &egui::Context) {
    apply_with_accent(ctx, AccentPreset::default());
}

/// Apply the full theme with a specific [`AccentPreset`].
pub fn apply_with_accent(ctx: &egui::Context, preset: AccentPreset) {
    install_fonts(ctx);
    // The design is dark only; without this a light OS theme picks the light style slot.
    ctx.set_theme(egui::ThemePreference::Dark);

    let accent = accent_color(preset);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ACTIVE_ACCENT_ID), accent));

    let mut style = (*ctx.global_style()).clone();
    style.visuals = build_visuals(accent);

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.scroll.bar_width = 7.0;
    style.spacing.scroll.floating = true;
    style.spacing.scroll.foreground_color = false;

    // Typography scale.
    style.text_styles.insert(TextStyle::Heading, display(26.0));
    style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
    style.text_styles.insert(TextStyle::Button, bold(14.0));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Monospace, mono(13.0));

    egui_extras::install_image_loaders(ctx);
    ctx.set_global_style(style);
}

/// Live-switch the active [`AccentPreset`] without reloading fonts or image loaders.
pub fn apply_accent(ctx: &egui::Context, preset: AccentPreset) {
    let accent = accent_color(preset);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ACTIVE_ACCENT_ID), accent));
    let mut style = (*ctx.global_style()).clone();
    style.visuals = build_visuals(accent);
    ctx.set_global_style(style);
}

/// Condensed motorsport display face (Barlow Condensed Bold).
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

/// SemiBold UI face (Inter SemiBold).
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Tabular-numeral telemetry monospace face (JetBrains Mono Medium).
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Small uppercase motorsport section label.
pub fn micro_label(text: &str) -> RichText {
    RichText::new(text.to_uppercase()).font(display(12.5)).color(MUTED)
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(18))
}

/// A flatter, tighter card for use inside other cards or side panels.
pub fn card_flat() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_VOID.gamma_multiply(0.75))
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
}

/// Elevated carbon surface card with subtle shadow.
pub fn card_elevated() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_ELEVATED)
        .stroke(Stroke::new(1.0, BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(RADIUS))
        .shadow(Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(95) })
        .inner_margin(Margin::same(18))
}

/// Translucent obsidian glass card for floating HUD panels, drawers and docks.
pub fn card_glass() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_GLASS)
        .stroke(Stroke::new(1.0, BORDER_ACTIVE))
        .corner_radius(CornerRadius::same(RADIUS))
        .shadow(Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(135) })
        .inner_margin(Margin::symmetric(16, 12))
}

/// Paint a subtle 1 px top specular highlight inside a card rect to give machined depth.
pub fn paint_top_specular(painter: &egui::Painter, rect: Rect, radius: f32) {
    let inset = radius.min(rect.width() * 0.25);
    let y = rect.top() + 1.0;
    painter.line_segment(
        [pos2(rect.left() + inset, y), pos2(rect.right() - inset, y)],
        Stroke::new(1.0, TOP_HIGHLIGHT),
    );
}

pub fn pill_color(text: &str) -> Color32 {
    match text {
        "running" | "installed" | "known" | "OK" => OK,
        "not running" | "not installed" | "unrecognized" | "invalid" => WARN,
        _ => INFO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_and_accents_apply_headlessly() {
        let ctx = egui::Context::default();
        apply(&ctx);
        assert_eq!(active_accent(&ctx), accent_color(AccentPreset::MotorsportRed));

        for preset in [
            AccentPreset::MotorsportRed,
            AccentPreset::TelemetryCyan,
            AccentPreset::AmberGold,
            AccentPreset::Emerald,
        ] {
            apply_accent(&ctx, preset);
            assert_eq!(active_accent(&ctx), accent_color(preset));
        }

        // Verify all custom font families render a frame without panicking.
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label(RichText::new("FERRARI 488 GT3").font(display(32.0)));
            ui.label(RichText::new("Section Title").font(bold(16.0)));
            ui.label(RichText::new("1:47.824").font(mono(14.0)));
            ui.label(micro_label("TELEMETRY"));
        });
        out.textures_delta.clear();
    }
}
