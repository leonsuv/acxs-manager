//! Reusable pieces: pills, stat tiles, section headers and clickable list cards.

use super::theme::{self, BG, CARD_HOVER, CARD_SELECTED, MUTED, TEXT};
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Sense, Stroke};

/// A small rounded label, e.g. a status or a tag.
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(color));
        });
}

/// Big number with a caption, used in the home dashboard.
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).small().color(MUTED));
        ui.label(RichText::new(value).size(20.0).strong().color(color));
    });
}

pub fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).size(26.0).strong());
    ui.label(RichText::new(subtitle).color(MUTED));
    ui.add_space(12.0);
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).strong().color(MUTED));
    ui.add_space(4.0);
}

pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong().color(BG))
        .fill(theme::ACCENT)
        .corner_radius(CornerRadius::same(9))
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
    response.clicked()
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
