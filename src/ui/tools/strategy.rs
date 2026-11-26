//! Strategy tool page: tactile motorsport sliders and visual fuel/pit-window telemetry
//! bars that work out fuel coverage, pit window and stop plan live on every frame.
//!
//! The arithmetic lives in `acxs_manager::strategy` and runs on a handful of
//! numbers, so it is recalculated on each frame without a background thread.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, ACCENT, DANGER, INFO, MUTED, OK, TEXT, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::strategy::{self, FuelStatus, PitPlan, PitWindow};
use eframe::egui::{self, Align2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, pos2, vec2};

pub fn tool() -> Box<dyn Tool> {
    Box::new(StrategyTool::new())
}

struct StrategyTool {
    fuel_per_lap: f64,
    fuel_left: f64,
    race_laps: u32,
    completed_laps: u32,
    tank: f64,
    tyre_life: u32,
    min_stint: u32,
    /// Optional lap-by-lap tank reading calibration inside a collapsible drawer.
    readings: String,
    calibration_note: Option<(String, bool)>,
}

impl StrategyTool {
    fn new() -> Self {
        Self {
            fuel_per_lap: 2.4,
            fuel_left: 48.0,
            race_laps: 20,
            completed_laps: 0,
            tank: 60.0,
            tyre_life: 12,
            min_stint: 5,
            readings: String::new(),
            calibration_note: None,
        }
    }

    fn fuel_status(&self) -> Result<FuelStatus, String> {
        strategy::fuel_status(
            self.fuel_left,
            self.fuel_per_lap,
            self.race_laps,
            self.completed_laps.min(self.race_laps),
        )
        .map_err(|err| err.to_string())
    }

    fn pit_window(&self) -> Result<Option<PitWindow>, String> {
        strategy::pit_window(self.race_laps, self.tyre_life, self.min_stint)
            .map_err(|err| err.to_string())
    }

    fn pit_plan(&self) -> Result<PitPlan, String> {
        strategy::pit_plan(
            self.race_laps,
            self.tank,
            self.fuel_per_lap,
            self.tyre_life,
            self.min_stint,
        )
        .map_err(|err| err.to_string())
    }
}

impl Tool for StrategyTool {
    fn name(&self) -> &'static str {
        "Strategy"
    }

    fn content_width(&self) -> f32 {
        f32::INFINITY
    }

    fn group(&self) -> &'static str {
        "Race"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, _notices: &mut Vec<Notice>) {
        widgets::heading(
            ui,
            "Strategy",
            "Live race fuel calculator, pit window timeline and multi-stint stop planner.",
        );

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail = ui.available_width();
                if avail >= 880.0 {
                    let left_w = 400.0;
                    let gap = 16.0;
                    let right_w = (avail - left_w - gap).max(360.0);
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            vec2(left_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(left_w);
                                self.inputs_card(ui);
                            },
                        );
                        ui.add_space(gap);
                        ui.allocate_ui_with_layout(
                            vec2(right_w, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(right_w);
                                self.output_panels(ui, right_w);
                            },
                        );
                    });
                } else {
                    self.inputs_card(ui);
                    ui.add_space(14.0);
                    self.output_panels(ui, avail);
                }
            });
    }
}

impl StrategyTool {
    fn inputs_card(&mut self, ui: &mut egui::Ui) {
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Race & Fuel Controls");
            ui.spacing_mut().slider_width = (ui.available_width() - 160.0).clamp(110.0, 210.0);

            slider_row(ui, "Fuel burn rate", |ui| {
                ui.add(
                    egui::Slider::new(&mut self.fuel_per_lap, 0.2..=12.0)
                        .step_by(0.05)
                        .fixed_decimals(2)
                        .suffix(" L/lap"),
                );
            });

            slider_row(ui, "Fuel in tank", |ui| {
                let max_tank = self.tank.max(10.0);
                if self.fuel_left > max_tank {
                    self.fuel_left = max_tank;
                }
                ui.add(
                    egui::Slider::new(&mut self.fuel_left, 0.0..=max_tank)
                        .step_by(0.5)
                        .fixed_decimals(1)
                        .suffix(" L"),
                );
            });

            slider_row(ui, "Tank capacity", |ui| {
                ui.add(
                    egui::Slider::new(&mut self.tank, 10.0..=150.0)
                        .step_by(1.0)
                        .fixed_decimals(0)
                        .suffix(" L"),
                );
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            slider_row(ui, "Race distance", |ui| {
                ui.add(egui::Slider::new(&mut self.race_laps, 1..=200).suffix(" laps"));
            });

            if self.completed_laps > self.race_laps {
                self.completed_laps = self.race_laps;
            }
            slider_row(ui, "Laps completed", |ui| {
                ui.add(egui::Slider::new(&mut self.completed_laps, 0..=self.race_laps).suffix(" laps"));
            });

            slider_row(ui, "Tyre life", |ui| {
                ui.add(egui::Slider::new(&mut self.tyre_life, 1..=100).suffix(" laps"));
            });

            slider_row(ui, "Minimum stint", |ui| {
                let max_stint = self.tyre_life.max(1);
                if self.min_stint > max_stint {
                    self.min_stint = max_stint;
                }
                ui.add(egui::Slider::new(&mut self.min_stint, 1..=max_stint).suffix(" laps"));
            });

            ui.add_space(10.0);
            egui::CollapsingHeader::new(
                RichText::new("Calibrate from telemetry / readings")
                    .font(theme::bold(13.0))
                    .color(MUTED),
            )
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Enter consecutive lap tank readings (L, oldest first) to compute fuel/lap:")
                        .small()
                        .color(MUTED),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 96.0).max(120.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.readings)
                            .desired_width(w)
                            .hint_text("52.1, 50.0, 47.9"),
                    );
                    if ui.button("Apply rate").clicked() {
                        match parse_readings(&self.readings)
                            .and_then(|r| strategy::fuel_per_lap(&r).map_err(|e| e.to_string()))
                        {
                            Ok(rate) => {
                                self.fuel_per_lap = rate.clamp(0.2, 12.0);
                                self.calibration_note =
                                    Some((format!("Calibrated to {rate:.3} L/lap."), true));
                            }
                            Err(e) => {
                                self.calibration_note = Some((e, false));
                            }
                        }
                    }
                });
                if let Some((msg, ok)) = &self.calibration_note {
                    ui.add_space(4.0);
                    ui.label(RichText::new(msg).small().color(if *ok { OK } else { DANGER }));
                }
            });
        });
    }

    fn output_panels(&self, ui: &mut egui::Ui, width: f32) {
        if width >= 620.0 {
            let gap = 14.0;
            let col_w = ((width - gap) * 0.5).floor();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.allocate_ui_with_layout(
                    vec2(col_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(col_w);
                        fuel_card(ui, self);
                    },
                );
                ui.add_space(gap);
                ui.allocate_ui_with_layout(
                    vec2(col_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(col_w);
                        window_card(ui, self);
                    },
                );
            });
        } else {
            fuel_card(ui, self);
            ui.add_space(12.0);
            window_card(ui, self);
        }
        ui.add_space(14.0);
        plan_card(ui, self);
    }
}

fn slider_row(ui: &mut egui::Ui, label: &str, add_slider: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [116.0, 22.0],
            egui::Label::new(RichText::new(label).color(MUTED)),
        );
        add_slider(ui);
    });
    ui.add_space(4.0);
}

fn fuel_card(ui: &mut egui::Ui, tool: &StrategyTool) {
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, "Fuel Telemetry");
        match tool.fuel_status() {
            Err(text) => {
                ui.label(RichText::new(text).color(DANGER));
            }
            Ok(status) => {
                ui.horizontal_wrapped(|ui| {
                    widgets::stat(ui, "Burn Rate", &format!("{:.2} L", tool.fuel_per_lap), INFO);
                    ui.add_space(10.0);
                    widgets::stat(ui, "Covers", &format!("{} laps", status.laps_covered), TEXT);
                    ui.add_space(10.0);
                    widgets::stat(ui, "Needed", &format!("{:.1} L", status.fuel_needed), TEXT);
                });
                ui.add_space(10.0);
                paint_fuel_bar(ui, tool.fuel_left, status.fuel_needed, tool.tank);
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if status.surplus >= 0.0 {
                        widgets::pill(ui, &format!("+{:.2} L SPARE", status.surplus), OK);
                    } else {
                        widgets::pill(ui, &format!("-{:.2} L SHORT", -status.surplus), DANGER);
                    }
                    if status.laps_short > 0 {
                        widgets::pill(ui, &format!("SHORT {} LAPS", status.laps_short), DANGER);
                    } else {
                        widgets::pill(ui, &format!("{} LAPS TO GO", status.laps_to_go), INFO);
                    }
                });
            }
        }
    });
}

/// Painted horizontal fuel gauge comparing current tank load against fuel needed to finish.
fn paint_fuel_bar(ui: &mut egui::Ui, fuel_left: f64, fuel_needed: f64, tank: f64) {
    let scale = tank.max(fuel_needed).max(1.0);
    let fill_frac = (fuel_left / scale).clamp(0.0, 1.0) as f32;
    let need_frac = (fuel_needed / scale).clamp(0.0, 1.0) as f32;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let round = CornerRadius::same(6);
    painter.rect_filled(rect, round, theme::SURFACE_VOID);

    let bar_color = if fuel_left >= fuel_needed { OK } else { DANGER };
    let fill_w = (rect.width() * fill_frac).max(if fill_frac > 0.0 { 4.0 } else { 0.0 });
    if fill_w > 0.0 {
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(fill_w, rect.height())),
            round,
            bar_color.gamma_multiply(0.80),
        );
    }

    // Target fuel marker tick.
    let need_x = rect.left() + rect.width() * need_frac;
    painter.line_segment(
        [pos2(need_x, rect.top() - 2.0), pos2(need_x, rect.bottom() + 2.0)],
        Stroke::new(2.0, Color32::WHITE),
    );
    painter.rect_stroke(rect, round, Stroke::new(1.0, theme::BORDER_ACTIVE), egui::StrokeKind::Inside);

    painter.text(
        rect.left_center() + vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        format!("TANK {fuel_left:.1} L"),
        theme::mono(11.0),
        Color32::WHITE,
    );
    painter.text(
        rect.right_center() - vec2(8.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("REQ {fuel_needed:.1} L"),
        theme::mono(11.0),
        MUTED,
    );
}

fn window_card(ui: &mut egui::Ui, tool: &StrategyTool) {
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, "Pit Window Timeline");
        let plan_stops = tool.pit_plan().ok().map(|p| p.stop_laps()).unwrap_or_default();
        match tool.pit_window() {
            Ok(Some(window)) => {
                widgets::stat(
                    ui,
                    "Viable Stop Window",
                    &format!("Lap {} – {}", window.first_lap, window.last_lap),
                    OK,
                );
                ui.add_space(10.0);
                paint_pit_timeline(
                    ui,
                    tool.race_laps,
                    tool.completed_laps,
                    Some((window.first_lap, window.last_lap)),
                    &plan_stops,
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Green band = valid pit window · Orange ticks = planned stop laps.")
                        .small()
                        .color(MUTED),
                );
            }
            Ok(None) => {
                widgets::pill(ui, "NO SINGLE-STOP WINDOW", WARN);
                ui.add_space(8.0);
                paint_pit_timeline(ui, tool.race_laps, tool.completed_laps, None, &plan_stops);
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Tyre life and minimum stint require multiple stops or wider limits.")
                        .small()
                        .color(WARN),
                );
            }
            Err(text) => {
                ui.label(RichText::new(text).color(DANGER));
            }
        }
    });
}

/// Painted lap timeline bar showing completed laps, valid pit window band, and planned stops.
fn paint_pit_timeline(
    ui: &mut egui::Ui,
    race_laps: u32,
    completed: u32,
    window: Option<(u32, u32)>,
    stops: &[u32],
) {
    let total = race_laps.max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let round = CornerRadius::same(6);
    painter.rect_filled(rect, round, theme::SURFACE_VOID);

    // Completed laps progress shading.
    if completed > 0 {
        let done_w = rect.width() * (completed as f32 / total).clamp(0.0, 1.0);
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(done_w, rect.height())),
            round,
            INFO.gamma_multiply(0.28),
        );
    }

    // Viable pit window highlight band.
    if let Some((first, last)) = window {
        let x0 = rect.left() + rect.width() * ((first as f32 - 0.5) / total).clamp(0.0, 1.0);
        let x1 = rect.left() + rect.width() * ((last as f32 + 0.5) / total).clamp(0.0, 1.0);
        let band = Rect::from_min_max(pos2(x0, rect.top() + 2.0), pos2(x1.max(x0 + 4.0), rect.bottom() - 2.0));
        painter.rect_filled(band, CornerRadius::same(4), OK.gamma_multiply(0.35));
        painter.rect_stroke(band, CornerRadius::same(4), Stroke::new(1.0, OK), egui::StrokeKind::Inside);
    }

    // Planned stop lap markers.
    for &lap in stops {
        let x = rect.left() + rect.width() * (lap as f32 / total).clamp(0.0, 1.0);
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(2.5, ACCENT),
        );
        painter.circle_filled(pos2(x, rect.center().y), 3.5, ACCENT);
    }

    painter.rect_stroke(rect, round, Stroke::new(1.0, theme::BORDER_ACTIVE), egui::StrokeKind::Inside);
    painter.text(
        rect.left_center() + vec2(6.0, 0.0),
        Align2::LEFT_CENTER,
        "L1",
        theme::mono(10.5),
        MUTED,
    );
    painter.text(
        rect.right_center() - vec2(6.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("L{race_laps}"),
        theme::mono(10.5),
        MUTED,
    );
}

fn plan_card(ui: &mut egui::Ui, tool: &StrategyTool) {
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, "Stop Plan");
        match tool.pit_plan() {
            Ok(plan) => {
                let stops = plan.stop_laps();
                ui.horizontal_wrapped(|ui| {
                    let badge = if stops.is_empty() {
                        "0 STOPS".to_string()
                    } else {
                        format!("{} STOP{}", plan.stops(), if plan.stops() == 1 { "" } else { "S" })
                    };
                    widgets::pill(ui, &badge, if stops.is_empty() { OK } else { ACCENT });
                    let summary = if stops.is_empty() {
                        "No stop needed — single stint covers the race.".to_string()
                    } else {
                        let laps: Vec<String> = stops.iter().map(|lap| lap.to_string()).collect();
                        format!(
                            "Box after lap{} {}.",
                            if stops.len() == 1 { "" } else { "s" },
                            laps.join(", ")
                        )
                    };
                    ui.label(RichText::new(summary).color(TEXT));
                });
                ui.add_space(10.0);
                stint_table(ui, &plan);
            }
            Err(text) => {
                ui.label(RichText::new(text).color(DANGER));
            }
        }
    });
}

fn stint_table(ui: &mut egui::Ui, plan: &PitPlan) {
    let col_min = ((ui.available_width() - 60.0) / 4.0).max(80.0);
    egui::Grid::new("strategy_plan")
        .striped(true)
        .num_columns(4)
        .min_col_width(col_min)
        .spacing([18.0, 8.0])
        .show(ui, |ui| {
            for heading in ["STINT", "LAPS", "RACE LAPS", "FUEL LOAD"] {
                ui.label(RichText::new(heading).font(theme::display(12.5)).color(MUTED));
            }
            ui.end_row();
            for stint in &plan.stints {
                ui.label(RichText::new(format!("Stint {}", stint.number)).font(theme::bold(13.5)).color(TEXT));
                ui.label(RichText::new(stint.laps.to_string()).font(theme::mono(13.5)).color(TEXT));
                ui.label(RichText::new(format!("Lap {} – {}", stint.first_lap, stint.last_lap)).font(theme::mono(13.5)).color(MUTED));
                ui.label(RichText::new(format!("{:.2} L", stint.fuel_load)).font(theme::mono(13.5)).color(INFO));
                ui.end_row();
            }
        });
}

/// Splits fuel readings on commas or spaces.
fn parse_readings(text: &str) -> Result<Vec<f64>, String> {
    let vals: Vec<f64> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<f64>()
                .map_err(|_| format!("'{part}' is not a fuel reading."))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if vals.len() < 2 {
        return Err("Enter at least two consecutive tank readings (e.g. 52.1, 50.0).".to_string());
    }
    Ok(vals)
}
