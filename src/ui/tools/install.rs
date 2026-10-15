//! Install Finder: finds Assetto Corsa installs through Steam, opens or copies
//! their paths, and checks a folder the user picks by hand.
//!
//! This tool cannot change app settings. The Home page's Assetto Corsa folder
//! field is where a chosen path is saved.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED, OK, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::{settings, steam};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    Box::new(InstallFinder::default())
}

/// Result of checking a folder for `acs.exe`.
struct ManualCheck {
    path: PathBuf,
    is_root: bool,
}

#[derive(Default)]
struct InstallFinder {
    found: Vec<PathBuf>,
    scanned: bool,
    scanning: bool,
    scan_rx: Option<Receiver<Vec<PathBuf>>>,
    manual_input: String,
    checking: bool,
    check_rx: Option<Receiver<ManualCheck>>,
    manual: Option<ManualCheck>,
}

impl Tool for InstallFinder {
    fn name(&self) -> &'static str {
        "Install Finder"
    }

    fn group(&self) -> &'static str {
        "Launch"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        let ctx = ui.ctx().clone();

        widgets::heading(
            ui,
            "Install Finder",
            "Find Assetto Corsa installs on this computer, or check a folder you choose.",
        );

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            theme::card().show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "The Home page's Assetto Corsa folder field is where a chosen path is saved. \
                         This tool cannot change app settings: copy the path, paste it into that field, \
                         then press Save folders.",
                    )
                    .color(WARN),
                );
            });

            ui.add_space(16.0);
            widgets::section_title(ui, "Steam scan");
            theme::card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.scanning, widgets::primary_button("Scan"))
                        .clicked()
                    {
                        self.start_scan(&ctx);
                    }
                    if self.scanning {
                        ui.label(RichText::new("Scanning Steam libraries...").color(MUTED));
                    }
                });
                if steam::default_steam_roots().is_empty() {
                    ui.label(
                        RichText::new(
                            "The automatic scan looks for Steam on Windows only. Use the folder check below elsewhere.",
                        )
                        .small()
                        .color(MUTED),
                    );
                }
            });

            ui.add_space(16.0);
            widgets::section_title(ui, "Found installs");
            if !self.scanned && !self.scanning {
                ui.label(RichText::new("Press Scan to search your Steam libraries.").color(MUTED));
            } else if self.scanned && self.found.is_empty() {
                ui.label(
                    RichText::new("No Assetto Corsa installs were found in the Steam libraries.")
                        .color(MUTED),
                );
            }
            for path in &self.found {
                theme::card().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        widgets::pill(ui, "acs.exe found", OK);
                    });
                    ui.label(RichText::new(path.display().to_string()).monospace());
                    path_actions(ui, notices, path);
                });
                ui.add_space(4.0);
            }

            ui.add_space(16.0);
            widgets::section_title(ui, "Check a folder");
            theme::card().show(ui, |ui| {
                ui.label(
                    RichText::new("Assetto Corsa folder (the one containing acs.exe)").color(MUTED),
                );
                let field_width = (ui.available_width() - 260.0).max(160.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.manual_input)
                            .desired_width(field_width)
                            .hint_text("Paste a path or browse"),
                    );
                    if ui
                        .add_enabled(!self.checking, egui::Button::new("Check"))
                        .clicked()
                    {
                        let input = self.manual_input.trim().to_string();
                        if input.is_empty() {
                            notices.push(Notice {
                                text: "Enter or browse to a folder first.".into(),
                                ok: false,
                            });
                        } else {
                            self.start_check(&ctx, PathBuf::from(input));
                        }
                    }
                    if ui
                        .add_enabled(!self.checking, egui::Button::new("Browse"))
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        self.manual_input = path.display().to_string();
                        self.start_check(&ctx, path);
                    }
                });
                ui.add_space(8.0);

                if self.checking {
                    ui.label(RichText::new("Checking folder...").color(MUTED));
                } else if let Some(check) = &self.manual {
                    widgets::key_value(ui, "Folder", &check.path.display().to_string());
                    ui.horizontal(|ui| {
                        if check.is_root {
                            widgets::pill(ui, "acs.exe found", OK);
                            ui.label("This is a valid Assetto Corsa folder.");
                        } else {
                            widgets::pill(ui, "acs.exe missing", WARN);
                            ui.label(
                                RichText::new(
                                    "acs.exe was not found here. Choose the folder that contains it.",
                                )
                                .color(WARN),
                            );
                        }
                    });
                    if check.is_root {
                        path_actions(ui, notices, &check.path);
                    }
                }
            });
        });
    }
}

impl InstallFinder {
    /// Pick up finished background work without blocking.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        let outcome = self.scan_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok(found)) => {
                self.found = found;
                self.scanned = true;
                self.scanning = false;
                self.scan_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.scanning = false;
                self.scan_rx = None;
                notices.push(Notice {
                    text: "The Steam scan stopped unexpectedly.".into(),
                    ok: false,
                });
            }
            _ => {}
        }

        let outcome = self.check_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok(check)) => {
                self.manual = Some(check);
                self.checking = false;
                self.check_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.checking = false;
                self.check_rx = None;
                notices.push(Notice {
                    text: "The folder check stopped unexpectedly.".into(),
                    ok: false,
                });
            }
            _ => {}
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let found = steam::find_ac_installs(&steam::default_steam_roots());
            let _ = tx.send(found);
            repaint.request_repaint();
        });
        self.scan_rx = Some(rx);
        self.scanning = true;
    }

    fn start_check(&mut self, ctx: &egui::Context, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let is_root = settings::is_ac_root(&path);
            let _ = tx.send(ManualCheck { path, is_root });
            repaint.request_repaint();
        });
        self.check_rx = Some(rx);
        self.checking = true;
    }
}

/// The "Open folder" and "Copy path" buttons for one install path.
fn path_actions(ui: &mut egui::Ui, notices: &mut Vec<Notice>, path: &Path) {
    ui.horizontal(|ui| {
        if ui.button("Open folder").clicked()
            && let Err(err) = open_folder(path)
        {
            notices.push(Notice {
                text: format!("Could not open {}: {err}", path.display()),
                ok: false,
            });
        }
        if ui.button("Copy path").clicked() {
            ui.ctx().copy_text(path.display().to_string());
            notices.push(Notice {
                text: format!(
                    "Copied path: {}. Paste it into the Assetto Corsa folder field on the Home page, then press Save folders.",
                    path.display()
                ),
                ok: true,
            });
        }
    });
}

/// Open the folder in the system file browser. Spawns and returns at once.
fn open_folder(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let child = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let child = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let child: std::io::Result<std::process::Child> = {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "opening folders is supported on Windows and macOS only",
        ))
    };
    child.map(|_| ())
}
