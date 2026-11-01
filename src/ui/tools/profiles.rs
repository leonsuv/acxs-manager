//! Graphics Profile page: shows which graphics injector is active in the
//! Assetto Corsa folder and switches to another one.
//!
//! Reading the folder, scanning a lights-patch library and switching all run on
//! background threads. Results come back over a channel that is drained with
//! `try_recv` on each frame, and the worker wakes the UI with `request_repaint`.
//! Results for an AC root that is no longer selected are dropped.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED, OK, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::csp::{self, LightsPatch};
use acxs_manager::graphics_check::Role;
use acxs_manager::graphics_profiles::{self, ActiveStatus, Profile, SwitchReport};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

const SUPPORT_WARNING: &str = "Graphics DLL switching only changes files. It does not check that the renderer is supported by the installed game version. Confirm that the renderer matches your Assetto Corsa version before switching.";

/// Whether the ReShade and RTX Remix choices are offered.
const SHOW_RESHADE_AND_REMIX: bool = false;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(ProfilesPage {
        tx,
        rx,
        root: None,
        status: None,
        status_busy: false,
        choice: Choice::None,
        csp_library: None,
        csp_patches: Vec::new(),
        csp_index: 0,
        patches_busy: false,
        reshade_build: None,
        remix_build: None,
        confirm: false,
        switching: false,
        outcome: None,
    })
}

/// The option picked in the profile selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    None,
    Csp,
    ReShade,
    Remix,
}

/// Result of a background job.
enum Msg {
    Status {
        root: PathBuf,
        result: Result<ActiveStatus, String>,
    },
    Patches {
        library: PathBuf,
        patches: Vec<LightsPatch>,
    },
    Switched {
        root: PathBuf,
        target: &'static str,
        result: Result<SwitchReport, String>,
        status: Result<ActiveStatus, String>,
    },
}

struct ProfilesPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The AC root that `status` and any switch refer to.
    root: Option<PathBuf>,
    status: Option<Result<ActiveStatus, String>>,
    status_busy: bool,
    choice: Choice,
    /// Folder that was searched for lights-patch folders.
    csp_library: Option<PathBuf>,
    csp_patches: Vec<LightsPatch>,
    csp_index: usize,
    patches_busy: bool,
    reshade_build: Option<PathBuf>,
    remix_build: Option<PathBuf>,
    confirm: bool,
    switching: bool,
    /// Text and success flag of the last switch, shown under the button.
    outcome: Option<(String, bool)>,
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

impl ProfilesPage {
    fn start_status(&mut self, egui_ctx: &egui::Context, root: PathBuf) {
        self.status_busy = true;
        spawn_job(egui_ctx, &self.tx, move || {
            let result = graphics_profiles::active(&root).map_err(|e| e.to_string());
            Msg::Status { root, result }
        });
    }

    fn start_patch_scan(&mut self, egui_ctx: &egui::Context, library: PathBuf) {
        self.patches_busy = true;
        spawn_job(egui_ctx, &self.tx, move || {
            let patches = csp::discover_patches(std::slice::from_ref(&library));
            Msg::Patches { library, patches }
        });
    }

    fn start_switch(&mut self, egui_ctx: &egui::Context, root: PathBuf, profile: Profile, game_running: bool) {
        self.switching = true;
        self.outcome = None;
        let target = profile.label();
        spawn_job(egui_ctx, &self.tx, move || {
            let result = graphics_profiles::switch(&root, &profile, game_running || acxs_manager::game::is_game_running()).map_err(|e| e.to_string());
            let status = graphics_profiles::active(&root).map_err(|e| e.to_string());
            Msg::Switched {
                root,
                target,
                result,
                status,
            }
        });
    }

    /// Apply finished jobs. Results for an AC root that is no longer selected are dropped.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Status { root, result } => {
                    if self.root.as_deref() == Some(root.as_path()) {
                        self.status = Some(result);
                        self.status_busy = false;
                    }
                }
                Msg::Patches { library, patches } => {
                    if self.csp_library.as_deref() == Some(library.as_path()) {
                        self.csp_patches = patches;
                        self.csp_index = 0;
                        self.patches_busy = false;
                    }
                }
                Msg::Switched {
                    root,
                    target,
                    result,
                    status,
                } => {
                    self.switching = false;
                    self.confirm = false;
                    if self.root.as_deref() == Some(root.as_path()) {
                        self.status = Some(status);
                        self.status_busy = false;
                    }
                    let (text, ok) = match result {
                        Ok(report) => (summary(target, &report), true),
                        Err(message) => (format!("Switch to {target} was not completed. {message}"), false),
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

    /// The profile the selector and pickers describe, or why it is incomplete.
    fn selected_profile(&self) -> Result<Profile, String> {
        match self.choice {
            Choice::None => Ok(Profile::None),
            Choice::Csp => self
                .csp_patches
                .get(self.csp_index)
                .map(|patch| Profile::Csp(patch.folder.clone()))
                .ok_or_else(|| "Choose a folder that contains a lights-patch folder.".to_string()),
            Choice::ReShade => self
                .reshade_build
                .clone()
                .map(Profile::ReShade)
                .ok_or_else(|| "Choose a ReShade build folder.".to_string()),
            Choice::Remix => self
                .remix_build
                .clone()
                .map(Profile::Remix)
                .ok_or_else(|| "Choose a Remix build folder.".to_string()),
        }
    }

    fn status_card(&self, ui: &mut egui::Ui, refresh: &mut bool, busy: bool) {
        theme::card().show(ui, |ui| {
            ui.label(RichText::new("Active in the Assetto Corsa folder").strong());
            ui.add_space(4.0);
            match &self.status {
                None => {
                    ui.label(RichText::new("Reading the folder...").color(MUTED));
                }
                Some(Err(err)) => {
                    ui.label(RichText::new(format!("Could not read the folder: {err}")).color(WARN));
                }
                Some(Ok(status)) => {
                    widgets::key_value(ui, "Active", &status.label());
                    if status.proxies.is_empty() {
                        ui.label(RichText::new("No graphics proxy DLLs in the folder.").color(MUTED));
                    }
                    for proxy in &status.proxies {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&proxy.name).monospace());
                            let color = if proxy.role == Role::Other { WARN } else { OK };
                            widgets::pill(ui, graphics_profiles::role_label(proxy.role), color);
                        });
                    }
                    for warning in &status.warnings {
                        ui.label(RichText::new(warning).color(WARN));
                    }
                }
            }
            ui.add_space(6.0);
            if ui.add_enabled(!busy, egui::Button::new("Refresh")).clicked() {
                *refresh = true;
            }
        });
    }

    fn profile_card(&mut self, ui: &mut egui::Ui, library_pick: &mut Option<PathBuf>) {
        theme::card().show(ui, |ui| {
            ui.radio_value(&mut self.choice, Choice::None, "None: no graphics injector");

            ui.radio_value(&mut self.choice, Choice::Csp, "CSP lights patch");
            if self.choice == Choice::Csp {
                ui.indent("csp_options", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Choose library folder...").clicked()
                            && let Some(picked) = rfd::FileDialog::new().pick_folder()
                        {
                            *library_pick = Some(picked);
                        }
                        match &self.csp_library {
                            Some(library) => {
                                ui.label(RichText::new(library.display().to_string()).monospace());
                            }
                            None => {
                                ui.label(RichText::new("No folder chosen").color(MUTED));
                            }
                        }
                    });
                    if self.patches_busy {
                        ui.label(RichText::new("Looking for lights-patch folders...").color(MUTED));
                    } else if self.csp_patches.is_empty() {
                        if self.csp_library.is_some() {
                            ui.label(RichText::new("No lights-patch folders in this library.").color(MUTED));
                        }
                    } else {
                        let labels: Vec<String> = self.csp_patches.iter().map(patch_label).collect();
                        let selected = labels.get(self.csp_index).cloned().unwrap_or_default();
                        egui::ComboBox::from_id_salt("csp_patch_choice")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                for (index, label) in labels.iter().enumerate() {
                                    ui.selectable_value(&mut self.csp_index, index, label.as_str());
                                }
                            });
                    }
                });
            }

            // ReShade and RTX Remix are switched off while they do not work (see the RTX tool).
            if SHOW_RESHADE_AND_REMIX {
                ui.radio_value(&mut self.choice, Choice::ReShade, "ReShade");
                if self.choice == Choice::ReShade {
                    ui.indent("reshade_options", |ui| {
                        folder_row(ui, &mut self.reshade_build, "Choose build folder...");
                    });
                }

                ui.radio_value(&mut self.choice, Choice::Remix, "RTX Remix");
                if self.choice == Choice::Remix {
                    ui.indent("remix_options", |ui| {
                        folder_row(ui, &mut self.remix_build, "Choose build folder...");
                    });
                }
            }
        });
    }
}

impl Tool for ProfilesPage {
    fn name(&self) -> &'static str {
        "Graphics Profile"
    }

    fn group(&self) -> &'static str {
        "Graphics"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        let egui_ctx = ui.ctx().clone();

        widgets::heading(
            ui,
            "Graphics Profile",
            "Keep one graphics injector active in the Assetto Corsa folder.",
        );

        let Some(root) = ctx.ac_root.clone().filter(|_| ctx.ac_valid) else {
            widgets::empty_state(ui, "Choose a valid Assetto Corsa folder first.");
            return;
        };
        if self.root.as_deref() != Some(root.as_path()) {
            self.root = Some(root.clone());
            self.status = None;
            self.status_busy = false;
            self.outcome = None;
        }
        if self.status.is_none() && !self.status_busy {
            self.start_status(&egui_ctx, root.clone());
        }

        let mut refresh = false;
        let mut library_pick = None;
        self.status_card(ui, &mut refresh, self.status_busy || self.switching);
        if refresh {
            self.status = None;
        }

        widgets::section_title(ui, "Profile");
        self.profile_card(ui, &mut library_pick);

        widgets::section_title(ui, "Switch");
        let profile = self.selected_profile();
        let mut switch_now = None;
        theme::card().show(ui, |ui| {
            ui.label(RichText::new(SUPPORT_WARNING).color(WARN));
            ui.add_space(6.0);
            ui.checkbox(
                &mut self.confirm,
                "I understand the files are moved to an archive folder and the renderer must match the game version.",
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let enabled = !self.switching && !ctx.game_running && self.confirm && profile.is_ok();
                if ui.add_enabled(enabled, widgets::primary_button("Switch")).clicked()
                    && let Ok(chosen) = &profile
                {
                    switch_now = Some(chosen.clone());
                }
                if self.switching {
                    ui.label(RichText::new("Switching...").color(MUTED));
                } else if ctx.game_running {
                    ui.label(RichText::new("Close Assetto Corsa to switch.").color(WARN));
                } else if let Err(reason) = &profile {
                    ui.label(RichText::new(reason).color(MUTED));
                }
            });
            if let Some((text, ok)) = &self.outcome {
                ui.add_space(6.0);
                ui.label(RichText::new(text).color(if *ok { OK } else { WARN }));
            }
        });

        if let Some(picked) = library_pick {
            self.csp_library = Some(picked.clone());
            self.csp_patches.clear();
            self.csp_index = 0;
            self.start_patch_scan(&egui_ctx, picked);
        }
        if let Some(chosen) = switch_now {
            self.start_switch(&egui_ctx, root, chosen, ctx.game_running);
        }
    }
}

/// Label for a lights patch in the selector: its folder name.
fn patch_label(patch: &LightsPatch) -> String {
    patch
        .folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| patch.version.clone())
}

/// A button that picks a build folder, followed by the folder it holds.
fn folder_row(ui: &mut egui::Ui, folder: &mut Option<PathBuf>, button: &str) {
    ui.horizontal_wrapped(|ui| {
        if ui.button(button).clicked()
            && let Some(picked) = rfd::FileDialog::new().pick_folder()
        {
            *folder = Some(picked);
        }
        match folder {
            Some(path) => {
                ui.label(RichText::new(path.display().to_string()).monospace());
            }
            None => {
                ui.label(RichText::new("No folder chosen").color(MUTED));
            }
        }
    });
}

/// One-line summary of a finished switch.
fn summary(target: &str, report: &SwitchReport) -> String {
    let removed = if report.removed.is_empty() {
        "nothing".to_string()
    } else {
        report
            .removed
            .iter()
            .map(|role| graphics_profiles::role_label(*role))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let installed = if report.installed.is_empty() {
        "nothing".to_string()
    } else {
        report.installed.join(", ")
    };
    let archived = if report.archived.is_empty() {
        "No files were archived.".to_string()
    } else {
        let folders: Vec<String> = report.archived.iter().map(|p| p.display().to_string()).collect();
        format!("Archived to {}.", folders.join(", "))
    };
    format!("Switched to {target}. Removed: {removed}. Installed: {installed}. {archived}")
}
