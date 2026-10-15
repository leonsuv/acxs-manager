//! Install packages page: installs one car or track from a `.zip` archive into the
//! Assetto Corsa folder. The archive is inspected first, then installed on a thread.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, MUTED, OK, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::game;
use acxs_manager::installer::{self, ArchiveInfo, ContentKind, InstallReport};
use acxs_manager::util::human_size;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

pub fn tool() -> Box<dyn Tool> {
    Box::new(PackagesPage::default())
}

/// How long a game-running check stays fresh before the page asks again.
const GAME_CHECK_EVERY: Duration = Duration::from_secs(2);

/// The archive the user picked, with the result of inspecting it.
struct Picked {
    path: PathBuf,
    info: Result<ArchiveInfo, String>,
}

#[derive(Default)]
struct PackagesPage {
    picked: Option<Picked>,
    inspecting: bool,
    inspect_rx: Option<Receiver<(PathBuf, Result<ArchiveInfo, String>)>>,

    game_running: bool,
    game_checked_at: Option<Instant>,
    game_rx: Option<Receiver<bool>>,

    installing: bool,
    install_rx: Option<Receiver<Result<InstallReport, String>>>,
}

impl Tool for PackagesPage {
    fn name(&self) -> &'static str {
        "Install Packages"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let egui_ctx = ui.ctx().clone();
        self.poll(&egui_ctx, notices);

        widgets::heading(
            ui,
            "Install Packages",
            "Install one car or track from a .zip archive into your Assetto Corsa folder.",
        );

        let Some(root) = ctx.ac_root.as_deref() else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to install packages.");
            return;
        };

        let busy = self.inspecting || self.installing;
        let game_running = self.game_running;
        let root_valid = ctx.ac_valid;
        let install_target = self
            .picked
            .as_ref()
            .filter(|picked| picked.info.is_ok())
            .map(|picked| picked.path.clone());
        let can_install = root_valid && !game_running && !busy && install_target.is_some();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            widgets::section_title(ui, "Archive");
            theme::card().show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Choose a .zip that holds one car or one track, either in the game's content \
                         layout or at the archive root.",
                    )
                    .color(MUTED),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let browse = ui.add_enabled(!busy, egui::Button::new("Choose .zip"));
                    if browse.clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Zip archive", &["zip"])
                            .pick_file()
                    {
                        self.start_inspect(&egui_ctx, path);
                    }
                    if self.inspecting {
                        ui.label(RichText::new("Reading archive...").color(MUTED));
                    }
                });

                if let Some(picked) = &self.picked {
                    ui.add_space(8.0);
                    widgets::key_value(ui, "File", &picked.path.display().to_string());
                    match &picked.info {
                        Ok(info) => {
                            ui.horizontal(|ui| {
                                widgets::pill(ui, kind_name(info.kind), OK);
                                ui.label(RichText::new(&info.id).strong());
                            });
                            widgets::key_value(ui, "Files", &info.file_count.to_string());
                            widgets::key_value(ui, "Size", &human_size(info.total_size));
                        }
                        Err(message) => {
                            ui.label(
                                RichText::new(format!("This archive cannot be installed: {message}"))
                                    .color(DANGER),
                            );
                        }
                    }
                }
            });

            ui.add_space(16.0);
            widgets::section_title(ui, "Install");
            theme::card().show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Installing over existing content does not delete it. The old folder is moved \
                         to the archive folder under your Assetto Corsa folder first.",
                    )
                    .color(MUTED),
                );
                ui.add_space(8.0);
                if !root_valid {
                    ui.label(
                        RichText::new("The Assetto Corsa folder is not valid. Fix it on the Home page first.")
                            .color(WARN),
                    );
                } else if game_running {
                    ui.label(
                        RichText::new("Assetto Corsa is running. Close it before installing content.")
                            .color(WARN),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(can_install, widgets::primary_button("Install"))
                        .clicked()
                        && let Some(archive) = install_target.clone()
                    {
                        self.start_install(&egui_ctx, root.to_path_buf(), archive);
                    }
                    if self.installing {
                        ui.label(RichText::new("Installing...").color(MUTED));
                    }
                });
            });
        });
    }
}

impl PackagesPage {
    /// Pick up finished background work without blocking, and keep the game-running state fresh.
    fn poll(&mut self, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let outcome = self.inspect_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok((path, info))) => {
                self.picked = Some(Picked { path, info });
                self.inspecting = false;
                self.inspect_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.inspecting = false;
                self.inspect_rx = None;
                notices.push(Notice {
                    text: "Reading the archive stopped unexpectedly.".into(),
                    ok: false,
                });
            }
            _ => {}
        }

        let outcome = self.install_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok(Ok(report))) => {
                self.installing = false;
                self.install_rx = None;
                notices.push(Notice {
                    text: install_notice(&report),
                    ok: true,
                });
            }
            Some(Ok(Err(message))) => {
                self.installing = false;
                self.install_rx = None;
                notices.push(Notice {
                    text: format!("Install failed: {message}"),
                    ok: false,
                });
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.installing = false;
                self.install_rx = None;
                notices.push(Notice {
                    text: "The install stopped unexpectedly.".into(),
                    ok: false,
                });
            }
            _ => {}
        }

        let outcome = self.game_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok(running)) => {
                self.game_running = running;
                self.game_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => self.game_rx = None,
            _ => {}
        }
        let due = self
            .game_checked_at
            .is_none_or(|at| at.elapsed() >= GAME_CHECK_EVERY);
        if self.game_rx.is_none() && due {
            self.start_game_check(ctx);
        }
        ctx.request_repaint_after(GAME_CHECK_EVERY);
    }

    fn start_inspect(&mut self, ctx: &egui::Context, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let info = installer::inspect_archive(&path).map_err(|e| e.to_string());
            let _ = tx.send((path, info));
            repaint.request_repaint();
        });
        self.inspect_rx = Some(rx);
        self.inspecting = true;
    }

    fn start_install(&mut self, ctx: &egui::Context, root: PathBuf, archive: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            // Checked again here, right before anything is written.
            let running = game::is_game_running();
            let result = installer::install_archive(&root, &archive, running).map_err(|e| e.to_string());
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.install_rx = Some(rx);
        self.installing = true;
    }

    fn start_game_check(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(game::is_game_running());
            repaint.request_repaint();
        });
        self.game_rx = Some(rx);
        self.game_checked_at = Some(Instant::now());
    }
}

fn kind_name(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Car => "Car",
        ContentKind::Track => "Track",
    }
}

/// Notice text for a finished install, including where a replaced folder went.
fn install_notice(report: &InstallReport) -> String {
    let mut text = format!(
        "Installed {} {} ({} files, {}) to {}.",
        kind_name(report.kind).to_lowercase(),
        report.id,
        report.file_count,
        human_size(report.total_size),
        report.installed_to.display()
    );
    if let Some(old) = &report.moved_existing_to {
        text.push_str(&format!(" The previous copy was moved to {}.", old.display()));
    }
    text
}
