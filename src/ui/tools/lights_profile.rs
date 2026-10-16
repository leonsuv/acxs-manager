//! Lighting Profiles page: captures the lighting values of a lights patch
//! configuration file into a named profile, edits them, checks them and applies
//! them back with a backup. Only the values change. The rendering itself is done
//! by the installed lights patch.
//!
//! Capturing and applying run on background threads. Results come back over a
//! channel that is drained with `try_recv` on each frame, and the worker wakes
//! the UI with `request_repaint`. Profile files are small and are read and
//! written directly when the user picks them.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED, OK, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::lights_profile::{self, ApplyReport, LightsProfile};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

const INTRO: &str = "These settings only change the values in the lighting configuration file. The rendering itself is done by the installed lights patch. Applying a profile keeps the previous file as a backup next to it.";

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(LightsPage {
        tx,
        rx,
        folder: None,
        config: None,
        name: String::new(),
        profile: None,
        allow_new_keys: false,
        busy: false,
        outcome: None,
        version: None,
        differences: lights_profile::version_differences(),
    })
}

/// Result of a background job.
enum Msg {
    Captured(Result<LightsProfile, String>),
    Applied(Result<ApplyReport, String>),
}

struct LightsPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The lights patch folder the configuration file was taken from.
    folder: Option<PathBuf>,
    /// The configuration file that capture and apply read and write.
    config: Option<PathBuf>,
    /// Name typed for the profile. Used when saving.
    name: String,
    /// The profile being edited.
    profile: Option<LightsProfile>,
    /// Whether apply may add keys that the configuration file does not have.
    allow_new_keys: bool,
    busy: bool,
    /// Text and success flag of the last action, shown under the apply card.
    outcome: Option<(String, bool)>,
    /// Version of the chosen configuration file, when its keys show one.
    version: Option<String>,
    /// Differences between the 0.2 and 0.3 configurations.
    differences: Vec<String>,
}

/// The configuration file inside a lights patch folder.
fn default_config(folder: &Path) -> PathBuf {
    folder.join("extension").join("config").join("lighting_fx.ini")
}

/// Version of a chosen configuration file. Only a file named as the
/// configuration file is read, so another file is never mistaken for it.
fn detect(config: &Path) -> Option<String> {
    if config.file_name().and_then(|n| n.to_str()) != Some("lighting_fx.ini") {
        return None;
    }
    lights_profile::detect_version(config.parent()?)
}

/// Run `job` on a background thread and send its result back, waking the UI.
fn spawn_job(egui_ctx: &egui::Context, tx: &Sender<Msg>, job: impl FnOnce() -> Msg + Send + 'static) {
    let tx = tx.clone();
    let egui_ctx = egui_ctx.clone();
    std::thread::spawn(move || {
        if tx.send(job()).is_ok() {
            egui_ctx.request_repaint();
        }
    });
}

impl LightsPage {
    fn start_capture(&mut self, egui_ctx: &egui::Context, config: PathBuf) {
        self.busy = true;
        self.outcome = None;
        spawn_job(egui_ctx, &self.tx, move || {
            Msg::Captured(lights_profile::profile_from_config(&config).map_err(|e| e.to_string()))
        });
    }

    fn start_apply(&mut self, egui_ctx: &egui::Context, config: PathBuf, profile: LightsProfile, allow_new_keys: bool) {
        self.busy = true;
        self.outcome = None;
        spawn_job(egui_ctx, &self.tx, move || {
            Msg::Applied(
                lights_profile::apply_profile(&config, &profile, allow_new_keys).map_err(|e| e.to_string()),
            )
        });
    }

    /// Apply finished jobs.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            match msg {
                Msg::Captured(Ok(profile)) => {
                    if self.name.trim().is_empty() {
                        self.name = profile.name.clone();
                    }
                    self.profile = Some(profile);
                }
                Msg::Captured(Err(message)) => {
                    self.outcome = Some((format!("Capture was not completed. {message}"), false));
                }
                Msg::Applied(result) => {
                    self.refresh_version();
                    let (text, ok) = match result {
                        Ok(report) if report.changed == 0 => {
                            ("No values differed from the file, so nothing was written.".to_string(), true)
                        }
                        Ok(report) => (
                            format!(
                                "Applied. {} value(s) changed. The previous file was kept as a backup next to it.",
                                report.changed
                            ),
                            true,
                        ),
                        Err(message) => (format!("Apply was not completed. {message}"), false),
                    };
                    notices.push(Notice {
                        text: text.clone(),
                        ok,
                    });
                    self.outcome = Some((text, ok));
                }
            }
        }
    }

    /// Reads the version of the chosen configuration file again.
    fn refresh_version(&mut self) {
        self.version = self.config.as_deref().and_then(detect);
    }

    fn save_to(&mut self, dir: &Path) {
        let Some(profile) = self.profile.as_mut() else {
            return;
        };
        profile.name = self.name.trim().to_string();
        self.outcome = Some(match lights_profile::save_profile(dir, profile) {
            Ok(path) => (format!("Saved the profile to {}.", path.display()), true),
            Err(err) => (format!("Could not save the profile. {err}"), false),
        });
    }

    fn config_card(&mut self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Choose lights patch folder...").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    self.config = Some(default_config(&folder));
                    self.folder = Some(folder);
                    self.outcome = None;
                    self.refresh_version();
                }
                if ui.button("Choose file...").clicked()
                    && let Some(file) = rfd::FileDialog::new()
                        .add_filter("Lighting configuration", &["ini"])
                        .pick_file()
                {
                    self.config = Some(file);
                    self.outcome = None;
                    self.refresh_version();
                }
            });
            ui.add_space(6.0);
            match &self.config {
                Some(path) => {
                    ui.label(RichText::new(path.display().to_string()).monospace());
                }
                None => {
                    ui.label(RichText::new("No configuration file chosen").color(MUTED));
                }
            }
            if self.config.is_some() {
                ui.add_space(6.0);
                match &self.version {
                    Some(version) => {
                        ui.label(format!("Detected version: {version}"));
                    }
                    None => {
                        ui.label(RichText::new("Version could not be detected from the keys in this file.").color(WARN));
                    }
                }
            }
            ui.add_space(6.0);
            egui::CollapsingHeader::new("Differences between versions 0.2 and 0.3")
                .id_salt("lights_profile_versions")
                .show(ui, |ui| {
                    for line in &self.differences {
                        ui.label(RichText::new(line).monospace());
                    }
                });
        });
    }

    fn profile_card(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context) {
        let capture_enabled = !self.busy && self.config.is_some();
        let has_profile = self.profile.is_some();
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .hint_text("Profile name")
                        .desired_width(220.0),
                );
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(capture_enabled, egui::Button::new("Capture current values"))
                    .clicked()
                    && let Some(config) = self.config.clone()
                {
                    self.start_capture(egui_ctx, config);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Load profile file..."))
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Lighting profile", &["json"])
                        .pick_file()
                {
                    match lights_profile::load_profile(&path) {
                        Ok(profile) => {
                            self.name = profile.name.clone();
                            self.profile = Some(profile);
                            self.outcome = None;
                        }
                        Err(err) => self.outcome = Some((format!("Could not load the profile. {err}"), false)),
                    }
                }
                if ui
                    .add_enabled(has_profile && !self.busy, egui::Button::new("Save profile file..."))
                    .clicked()
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_title("Folder for profile files")
                        .pick_folder()
                {
                    self.save_to(&dir);
                }
            });
        });
    }

    fn values_card(&mut self, ui: &mut egui::Ui) {
        let Some(profile) = self.profile.as_mut() else {
            widgets::empty_state(ui, "Capture the current values, or load a profile file.");
            return;
        };
        let problems = lights_profile::validate_profile(profile);
        theme::card().show(ui, |ui| {
            if problems.is_empty() {
                ui.label(RichText::new("All values are valid.").color(OK));
            } else {
                for problem in &problems {
                    ui.label(RichText::new(problem).color(WARN));
                }
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("lights_profile_values")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Section").color(MUTED));
                        ui.label(RichText::new("Key").color(MUTED));
                        ui.label(RichText::new("Value").color(MUTED));
                        ui.end_row();
                        for (section, key, value) in profile.values.iter_mut() {
                            ui.label(section.as_str());
                            ui.label(RichText::new(key.as_str()).monospace());
                            ui.add(egui::TextEdit::singleline(value).desired_width(180.0));
                            ui.end_row();
                        }
                    });
            });
        });
    }

    fn apply_card(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context, game_running: bool) {
        let valid = self
            .profile
            .as_ref()
            .is_some_and(|p| lights_profile::validate_profile(p).is_empty());
        let ready = self.config.is_some() && self.profile.is_some() && valid;
        let can_apply = ready && !self.busy && !game_running;
        let reason = if self.config.is_none() {
            "Choose a configuration file first."
        } else if self.profile.is_none() {
            "Capture the current values or load a profile file first."
        } else {
            "Fix the values listed above first."
        };
        theme::card().show(ui, |ui| {
            ui.checkbox(
                &mut self.allow_new_keys,
                "Allow keys that are not in the file (adds them to their section)",
            );
            ui.add_space(6.0);
            if let (Some(version), Some(profile)) = (&self.version, &self.profile) {
                let unknown = lights_profile::unknown_keys(profile, version);
                if !unknown.is_empty() {
                    ui.label(
                        RichText::new(format!(
                            "Version {version} does not have these keys: {}. Apply refuses this profile unless \"Allow keys that are not in the file\" is checked, and then these keys are added to the file.",
                            unknown.join(", ")
                        ))
                        .color(WARN),
                    );
                    ui.add_space(6.0);
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(can_apply, widgets::primary_button("Apply to configuration"))
                    .clicked()
                    && let Some(config) = self.config.clone()
                    && let Some(profile) = self.profile.clone()
                {
                    let allow = self.allow_new_keys;
                    self.start_apply(egui_ctx, config, profile, allow);
                }
                if self.busy {
                    ui.label(RichText::new("Working...").color(MUTED));
                } else if game_running {
                    ui.label(RichText::new("Close Assetto Corsa to apply.").color(WARN));
                } else if !ready {
                    ui.label(RichText::new(reason).color(MUTED));
                }
            });
            if let Some((text, ok)) = &self.outcome {
                ui.add_space(6.0);
                ui.label(RichText::new(text).color(if *ok { OK } else { WARN }));
            }
        });
    }
}

impl Tool for LightsPage {
    fn name(&self) -> &'static str {
        "Lighting Profiles"
    }

    fn group(&self) -> &'static str {
        "Graphics"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        let egui_ctx = ui.ctx().clone();

        widgets::heading(
            ui,
            "Lighting Profiles",
            "Capture, edit and apply lighting values from a lights patch configuration file.",
        );
        theme::card().show(ui, |ui| {
            ui.label(RichText::new(INTRO).color(MUTED));
        });

        widgets::section_title(ui, "Configuration file");
        self.config_card(ui);

        widgets::section_title(ui, "Profile");
        self.profile_card(ui, &egui_ctx);

        widgets::section_title(ui, "Values");
        self.values_card(ui);

        widgets::section_title(ui, "Apply");
        self.apply_card(ui, &egui_ctx, ctx.game_running);
    }
}
