//! Screenshots tool page: the pictures in the game's screenshot folder.

use super::{Tool, ToolContext};
use crate::ui::{media, theme, widgets, Notice};
use acxs_manager::{replay_library, settings};
use eframe::egui::{self, RichText};
use std::path::PathBuf;

/// Pictures drawn at once, newest first.
const MAX_SHOWN: usize = 60;

pub fn tool() -> Box<dyn Tool> {
    Box::new(Screenshots { shots: None })
}

struct Screenshots {
    shots: Option<Vec<PathBuf>>,
}

fn folder() -> Option<PathBuf> {
    settings::ac_documents_dir().map(|docs| docs.join("screens"))
}

impl Tool for Screenshots {
    fn name(&self) -> &'static str {
        "Screenshots"
    }

    fn content_width(&self) -> f32 {
        1640.0
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, _notices: &mut Vec<Notice>) {
        widgets::heading(ui, "Screenshots", "What you shot in the game. Click a picture to open it.");
        ui.horizontal(|ui| {
            if ui.button("Reload").clicked() {
                self.shots = None;
            }
            if ui.button("Open folder").clicked()
                && let Some(dir) = folder()
            {
                let _ = std::fs::create_dir_all(&dir);
                crate::ui::pages::open_folder(&dir);
            }
        });
        ui.add_space(8.0);
        let shots = self.shots.get_or_insert_with(|| folder().map(|dir| replay_library::screenshots(&dir)).unwrap_or_default());
        if shots.is_empty() {
            widgets::empty_state(ui, "No screenshots yet. Press F8 in the game to take one.");
            return;
        }
        let shown = shots.len().min(MAX_SHOWN);
        let shots = &shots[..shown];
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            widgets::tile_grid(ui, 280.0, shown, |ui, i| {
                let width = ui.available_width();
                let (rect, response) = ui.allocate_exact_size(egui::vec2(width, width * 9.0 / 16.0), egui::Sense::click());
                media::paint_image(ui, rect, Some(&shots[i]), "", egui::Color32::WHITE, egui::CornerRadius::same(theme::RADIUS_SMALL));
                if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    crate::ui::pages::open_folder(&shots[i]);
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Newest {shown} pictures")).size(12.0).color(theme::MUTED));
        });
    }
}
