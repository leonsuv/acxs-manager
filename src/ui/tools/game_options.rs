//! The Audio, Force feedback, Gameplay and Filter tabs of Game Settings: the post-processing filter and
//! Python apps, all in the game's own config files.

use super::ToolContext;
use crate::ui::{theme, widgets, Notice};
use acxs_manager::game_options::{self, Control, OPTIONS, PythonApp};
use acxs_manager::settings;
use eframe::egui::{self, RichText};



pub struct GameOptions {
    /// The values as last read or saved; `None` until the first read.
    loaded: Option<Vec<Option<f64>>>,
    values: Vec<Option<f64>>,
    filters: Vec<String>,
    filter: Option<String>,
    apps: Vec<PythonApp>,
}

impl GameOptions {
    pub fn new() -> Self {
        GameOptions { loaded: None, values: Vec::new(), filters: Vec::new(), filter: None, apps: Vec::new() }
    }

    fn read(&mut self, ctx: &ToolContext) {
        let Some(cfg) = settings::ac_user_cfg_dir() else { return };
        let values = game_options::load(&cfg);
        self.values.clone_from(&values);
        self.loaded = Some(values);
        self.filter = game_options::active_filter(&cfg);
        if let Some(root) = &ctx.ac_root {
            self.filters = game_options::pp_filters(root, &cfg);
            self.apps = game_options::python_apps(root, &cfg);
        }
    }

    fn option_rows(&mut self, ui: &mut egui::Ui, tab: &str) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut missing = 0;
            ui.spacing_mut().slider_width = 360.0;
            egui::Grid::new("game_options").num_columns(2).min_col_width(300.0).spacing([24.0, 12.0]).show(ui, |ui| {
                for (option, value) in OPTIONS.iter().zip(&mut self.values).filter(|(o, _)| o.group == tab) {
                    let Some(current) = value else {
                        missing += 1;
                        continue;
                    };
                    ui.label(option.label);
                    match option.control {
                        Control::Toggle => {
                            let mut on = *current != 0.0;
                            if widgets::toggle(ui, &mut on).changed() {
                                *current = if on { 1.0 } else { 0.0 };
                            }
                        }
                        Control::Slider(min, max, true) => {
                            let mut percent = (*current * 100.0).round();
                            let slider = egui::Slider::new(&mut percent, (min * 100.0)..=(max * 100.0)).suffix("%").max_decimals(0);
                            if ui.add(slider).changed() {
                                *current = percent / 100.0;
                            }
                        }
                        Control::Slider(min, max, false) => {
                            ui.add(egui::Slider::new(current, min..=max).integer());
                        }
                    }
                    ui.end_row();
                }
            });
            if missing > 0 {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("{missing} more appear once the game has written them.")).size(12.0).color(theme::MUTED));
            }
        });
    }

    fn filter_and_apps(&mut self, ui: &mut egui::Ui, notices: &mut Vec<Notice>) {
        let Some(cfg) = settings::ac_user_cfg_dir() else { return };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            widgets::section_title(ui, "Post-processing filter");
            let shown = self.filter.clone().unwrap_or_else(|| "none".to_string());
            let mut picked = None;
            egui::ComboBox::from_id_salt("pp_filter").selected_text(shown).width(260.0).show_ui(ui, |ui| {
                for name in &self.filters {
                    if ui.selectable_label(self.filter.as_deref() == Some(name), name).clicked() {
                        picked = Some(name.clone());
                    }
                }
            });
            if let Some(name) = picked {
                match game_options::set_filter(&cfg, &name) {
                    Ok(()) => self.filter = Some(name),
                    Err(err) => notices.push(Notice { text: format!("Could not save the filter: {err}"), ok: false }),
                }
            }
            ui.add_space(16.0);
            widgets::section_title(ui, "Python apps");
            if self.apps.is_empty() {
                ui.label(RichText::new("No Python apps are installed.").color(theme::MUTED));
            }
            for app in &mut self.apps {
                ui.horizontal(|ui| {
                    if widgets::toggle(ui, &mut app.active).changed()
                        && let Err(err) = game_options::set_python_app(&cfg, &app.id, app.active)
                    {
                        notices.push(Notice { text: format!("Could not change {}: {err}", app.id), ok: false });
                    }
                    ui.label(&app.id);
                });
            }
        });
    }
    /// Draws one tab of Game Settings.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>, tab: &str) {
        if self.loaded.is_none() {
            self.read(ctx);
        }
        if ctx.game_running {
            ui.label(RichText::new("The game rewrites these files when it closes, so saving waits until then.").color(theme::WARN));
            ui.add_space(6.0);
        }
        if tab == "Filter and apps" {
            self.filter_and_apps(ui, notices);
            return;
        }

        let old = self.loaded.clone().unwrap_or_default();
        let changed = old.iter().zip(&self.values).filter(|(a, b)| a != b).count();
        ui.horizontal(|ui| {
            let can_save = changed > 0 && !ctx.game_running;
            if ui.add_enabled(can_save, widgets::primary_button("Save")).clicked()
                && let Some(cfg) = settings::ac_user_cfg_dir()
            {
                match game_options::save(&cfg, &old, &self.values) {
                    Ok(count) => {
                        notices.push(Notice { text: format!("Saved {count} settings."), ok: true });
                        self.loaded = Some(self.values.clone());
                    }
                    Err(err) => notices.push(Notice { text: format!("Could not save: {err}"), ok: false }),
                }
            }
            if ui.add_enabled(changed > 0, egui::Button::new("Revert")).clicked() {
                self.values = old;
            }
            if changed > 0 {
                ui.label(RichText::new(format!("{changed} changed")).color(theme::WARN));
            }
        });
        ui.add_space(10.0);
        self.option_rows(ui, tab);
    }
}
