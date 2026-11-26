//! Trajectory & Racing-Line Studio: overlays recorded 30 Hz laps or `.acreplay` traces
//! against `fast_lane.ai` with braking/apex/throttle analysis and 2D/3D viewport playback.
//!
//! Wave 2A mounts the compiling shell and `viewport_3d_frame` slot; Wave 3D wires the full
//! `ai_spline` + `trajectory` + `corner_coach` interactive pit-wall analysis.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED};
use crate::ui::viewport3d::{self, SceneFrame};
use crate::ui::{widgets, Notice};
use eframe::egui::{self, RichText, vec2};

pub fn tool() -> Box<dyn Tool> {
    Box::new(TrajectoryStudio)
}

struct TrajectoryStudio;

impl Tool for TrajectoryStudio {
    fn name(&self) -> &'static str {
        "Trajectory Studio"
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn content_width(&self) -> f32 {
        f32::INFINITY
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, _notices: &mut Vec<Notice>) {
        widgets::heading(
            ui,
            "Trajectory Studio",
            "Overlay recorded laps and replays against the AI racing line with corner-by-corner deltas.",
        );

        let avail = ui.available_size();
        let height = (avail.y - 16.0).max(260.0);
        widgets::viewport_3d_frame(
            ui,
            vec2(avail.x.max(320.0), height),
            |ui| {
                ui.horizontal(|ui| {
                    widgets::pill(ui, "TRAJECTORY", theme::CH_SPEED);
                    let mode = if ctx.gl.is_some() { "3D GL" } else { "2D FALLBACK" };
                    widgets::pill(ui, mode, theme::MUTED);
                });
            },
            |ui, rect| {
                viewport3d::paint_viewport_3d(ui, rect, ctx.gl.clone(), SceneFrame::default(), |ui, r| {
                    ui.painter().rect_filled(r, 0, theme::SURFACE_VOID);
                    ui.painter().text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        "Select a recorded session or replay to inspect the racing line",
                        egui::FontId::proportional(14.0),
                        MUTED,
                    );
                });
            },
        );
        let _ = RichText::new("");
    }
}
