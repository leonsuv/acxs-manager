//! Strategy tool page: from fuel readings and race details, works out the fuel
//! per lap, whether the tank covers the race, the pit window and a stop plan.
//!
//! The arithmetic lives in `acxs_manager::strategy` and runs on a handful of
//! numbers, so it is recalculated on each frame without a background thread.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::strategy::{self, FuelStatus, PitPlan, PitWindow};
use eframe::egui::{self, RichText};

pub fn tool() -> Box<dyn Tool> {
    Box::new(StrategyTool::new())
}

struct StrategyTool {
    readings: String,
    fuel_per_lap: String,
    fuel_left: String,
    race_laps: String,
    completed_laps: String,
    tank: String,
    tyre_life: String,
    min_stint: String,
}

impl StrategyTool {
    fn new() -> Self {
        Self {
            readings: String::new(),
            fuel_per_lap: String::new(),
            fuel_left: String::new(),
            race_laps: "20".to_string(),
            completed_laps: "0".to_string(),
            tank: "60".to_string(),
            tyre_life: "12".to_string(),
            min_stint: "5".to_string(),
        }
    }

    /// Fuel per lap: the typed override if there is one, else worked out from
    /// the readings.
    fn per_lap(&self) -> Result<f64, String> {
        if !self.fuel_per_lap.trim().is_empty() {
            return parse_f64("fuel per lap", &self.fuel_per_lap);
        }
        let readings = parse_readings(&self.readings)?;
        if readings.is_empty() {
            return Err("Enter fuel readings or a fuel per lap.".to_string());
        }
        strategy::fuel_per_lap(&readings).map_err(|err| err.to_string())
    }

    fn fuel_status(&self) -> Result<FuelStatus, String> {
        let per_lap = self.per_lap()?;
        strategy::fuel_status(
            parse_f64("fuel in the tank", &self.fuel_left)?,
            per_lap,
            parse_u32("race length", &self.race_laps)?,
            parse_u32("laps completed", &self.completed_laps)?,
        )
        .map_err(|err| err.to_string())
    }

    fn pit_window(&self) -> Result<Option<PitWindow>, String> {
        strategy::pit_window(
            parse_u32("race length", &self.race_laps)?,
            parse_u32("tyre life", &self.tyre_life)?,
            parse_u32("minimum stint", &self.min_stint)?,
        )
        .map_err(|err| err.to_string())
    }

    fn pit_plan(&self) -> Result<PitPlan, String> {
        strategy::pit_plan(
            parse_u32("race length", &self.race_laps)?,
            parse_f64("tank capacity", &self.tank)?,
            self.per_lap()?,
            parse_u32("tyre life", &self.tyre_life)?,
            parse_u32("minimum stint", &self.min_stint)?,
        )
        .map_err(|err| err.to_string())
    }
}

impl Tool for StrategyTool {
    fn name(&self) -> &'static str {
        "Strategy"
    }

    fn group(&self) -> &'static str {
        "Race"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, _notices: &mut Vec<Notice>) {
        widgets::heading(
            ui,
            "Strategy",
            "Fuel, pit window and stop plan from your own readings and race details.",
        );

        theme::card().show(ui, |ui| {
            egui::Grid::new("strategy_inputs")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    field(ui, "Fuel readings (L, oldest first)", &mut self.readings, "52.1, 50.0, 47.9");
                    field(ui, "Fuel per lap override (L)", &mut self.fuel_per_lap, "blank = from readings");
                    field(ui, "Fuel in the tank now (L)", &mut self.fuel_left, "");
                    field(ui, "Race length (laps)", &mut self.race_laps, "");
                    field(ui, "Laps completed", &mut self.completed_laps, "");
                    field(ui, "Tank capacity (L)", &mut self.tank, "");
                    field(ui, "Tyre life (laps)", &mut self.tyre_life, "");
                    field(ui, "Minimum stint (laps)", &mut self.min_stint, "");
                });
        });

        ui.add_space(12.0);
        fuel_card(ui, self);
        ui.add_space(12.0);
        window_card(ui, self);
        ui.add_space(12.0);
        plan_card(ui, self);
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(RichText::new(label).color(MUTED));
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(220.0)
            .hint_text(hint),
    );
    ui.end_row();
}

fn error_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(DANGER));
}

fn fuel_card(ui: &mut egui::Ui, tool: &StrategyTool) {
    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Fuel");
        match tool.per_lap() {
            Ok(per_lap) => widgets::key_value(ui, "Fuel per lap", &format!("{per_lap:.3} L")),
            Err(text) => error_label(ui, &text),
        }
        match tool.fuel_status() {
            Ok(status) => {
                widgets::key_value(ui, "Laps the fuel covers", &status.laps_covered.to_string());
                widgets::key_value(ui, "Laps to go", &status.laps_to_go.to_string());
                widgets::key_value(ui, "Fuel needed", &format!("{:.2} L", status.fuel_needed));
                let (text, color) = if status.surplus >= 0.0 {
                    (format!("{:.2} L spare", status.surplus), OK)
                } else {
                    (format!("{:.2} L short", -status.surplus), DANGER)
                };
                ui.label(RichText::new(text).color(color));
                if status.laps_short > 0 {
                    ui.label(
                        RichText::new(format!("Short by {} laps.", status.laps_short)).color(DANGER),
                    );
                }
            }
            Err(text) => error_label(ui, &text),
        }
    });
}

fn window_card(ui: &mut egui::Ui, tool: &StrategyTool) {
    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Pit window");
        match tool.pit_window() {
            Ok(Some(window)) => {
                widgets::key_value(
                    ui,
                    "Stop viable after lap",
                    &format!("{} to {}", window.first_lap, window.last_lap),
                );
            }
            Ok(None) => {
                ui.label(RichText::new("No lap allows a stop with these limits.").color(WARN));
            }
            Err(text) => error_label(ui, &text),
        }
    });
}

fn plan_card(ui: &mut egui::Ui, tool: &StrategyTool) {
    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Stop plan");
        match tool.pit_plan() {
            Ok(plan) => {
                let stops = plan.stop_laps();
                let summary = if stops.is_empty() {
                    "No stop needed.".to_string()
                } else {
                    let laps: Vec<String> = stops.iter().map(|lap| lap.to_string()).collect();
                    format!(
                        "{} stop{} after lap{} {}.",
                        plan.stops(),
                        if plan.stops() == 1 { "" } else { "s" },
                        if stops.len() == 1 { "" } else { "s" },
                        laps.join(", ")
                    )
                };
                ui.label(summary);
                ui.add_space(6.0);
                stint_table(ui, &plan);
            }
            Err(text) => error_label(ui, &text),
        }
    });
}

fn stint_table(ui: &mut egui::Ui, plan: &PitPlan) {
    egui::Grid::new("strategy_plan")
        .striped(true)
        .num_columns(4)
        .spacing([18.0, 6.0])
        .show(ui, |ui| {
            for heading in ["Stint", "Laps", "Race laps", "Fuel load"] {
                ui.label(RichText::new(heading).color(MUTED));
            }
            ui.end_row();
            for stint in &plan.stints {
                ui.label(stint.number.to_string());
                ui.label(stint.laps.to_string());
                ui.label(format!("{} to {}", stint.first_lap, stint.last_lap));
                ui.label(format!("{:.2} L", stint.fuel_load));
                ui.end_row();
            }
        });
}

fn parse_f64(label: &str, text: &str) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .map_err(|_| format!("Enter a number for the {label}."))
}

fn parse_u32(label: &str, text: &str) -> Result<u32, String> {
    text.trim()
        .parse::<u32>()
        .map_err(|_| format!("Enter a whole number for the {label}."))
}

/// Splits fuel readings on commas or spaces.
fn parse_readings(text: &str) -> Result<Vec<f64>, String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<f64>()
                .map_err(|_| format!("'{part}' is not a fuel reading."))
        })
        .collect()
}
