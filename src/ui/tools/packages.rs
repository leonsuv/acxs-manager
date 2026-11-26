//! Install packages page: universal drag-and-drop installer for cars, tracks, skins,
//! PP filters, weather, Python/Lua apps, CSP configs, fonts, drivers, sounds and more
//! from `.zip`, `.7z`, `.tar.gz`/`.tgz`, `.rar`, folders, or standalone `.ini` PP filters.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, INFO, MUTED, OK, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::content;
use acxs_manager::game;
use acxs_manager::installer::{
    self, ArchiveFormat, BatchInstallReport, ConflictKind, ContentKind, PackagePlan,
};
use acxs_manager::util::{self, human_size};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

pub fn tool() -> Box<dyn Tool> {
    Box::new(PackagesPage::default())
}

const GAME_CHECK_EVERY: Duration = Duration::from_secs(2);

struct Picked {
    path: PathBuf,
    plan: Result<PackagePlan, String>,
}

struct InstalledRow {
    name: String,
    id: String,
    kind: ContentKind,
}

#[derive(Default)]
struct PackagesPage {
    picked: Option<Picked>,
    inspecting: bool,
    inspect_rx: Option<Receiver<(PathBuf, Result<PackagePlan, String>)>>,

    game_running: bool,
    game_checked_at: Option<Instant>,
    game_rx: Option<Receiver<bool>>,

    installing: bool,
    install_rx: Option<Receiver<Result<BatchInstallReport, String>>>,

    installed: Vec<InstalledRow>,
    installed_filter: String,
    scanned_root: Option<PathBuf>,
    scanning: bool,
    scan_rx: Option<Receiver<Vec<InstalledRow>>>,
    needs_scan: bool,
}

fn is_supported_drop(path: &PathBuf) -> bool {
    if path.is_dir() {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.ends_with(".zip")
        || name.ends_with(".7z")
        || name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".rar")
        || name.ends_with(".ini")
}

impl Tool for PackagesPage {
    fn on_drop(&mut self, ctx: &egui::Context, paths: &[PathBuf]) -> bool {
        let candidate = paths.iter().find(|p| is_supported_drop(p));
        match candidate {
            Some(path) => {
                self.start_inspect(ctx, path.clone(), self.scanned_root.clone());
                true
            }
            None => false,
        }
    }

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
            "Drop or choose a .zip, .7z, .tar.gz, .rar, folder, or PP filter .ini to classify and install AC content.",
        );

        let Some(root) = ctx.ac_root.as_deref() else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to install packages.");
            return;
        };

        let root_changed = self.scanned_root.as_deref() != Some(root);
        if (self.needs_scan || root_changed) && !self.scanning {
            self.start_scan(&egui_ctx, root.to_path_buf());
        }

        let busy = self.inspecting || self.installing;
        let game_running = self.game_running || ctx.game_running;
        let root_valid = ctx.ac_valid;

        let selected_count = self
            .picked
            .as_ref()
            .and_then(|p| p.plan.as_ref().ok())
            .map(|plan| plan.items.iter().filter(|i| i.selected).count())
            .unwrap_or(0);
        let can_install = root_valid && !game_running && !busy && selected_count > 0;

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            widgets::section_title(ui, "Package");
            theme::card_elevated().show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Supports cars, tracks, skins, PP filters, weather, Python & CSP Lua apps, \
                         CSP configs, fonts, drivers, helmets, crew, sounds, and showrooms.",
                    )
                    .color(MUTED),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Choose archive / .ini")).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "AC Packages",
                                &["zip", "7z", "gz", "tgz", "rar", "ini"],
                            )
                            .pick_file()
                    {
                        self.start_inspect(&egui_ctx, path, Some(root.to_path_buf()));
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Choose folder")).clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        self.start_inspect(&egui_ctx, path, Some(root.to_path_buf()));
                    }
                    if self.inspecting {
                        ui.label(RichText::new("Inspecting package...").color(MUTED));
                    }
                });

                if let Some(picked) = &mut self.picked {
                    ui.add_space(10.0);
                    widgets::key_value(ui, "Source", &picked.path.display().to_string());
                    match &mut picked.plan {
                        Ok(plan) => {
                            ui.horizontal(|ui| {
                                widgets::pill(ui, format_label(plan.format), INFO);
                                ui.label(
                                    RichText::new(format!("{} item(s) detected", plan.items.len()))
                                        .font(theme::bold(13.0)),
                                );
                            });
                            ui.add_space(6.0);
                            for item in &mut plan.items {
                                theme::card().show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.checkbox(&mut item.selected, "");
                                        widgets::pill(ui, item.kind.label(), OK);
                                        ui.label(RichText::new(&item.id).font(theme::bold(13.5)));
                                        ui.label(
                                            RichText::new(format!(
                                                "{} files · {}",
                                                item.file_count,
                                                human_size(item.total_size)
                                            ))
                                            .font(theme::mono(12.0))
                                            .color(MUTED),
                                        );
                                        if let Some(conflict) = &item.conflict {
                                            let badge = match conflict {
                                                ConflictKind::ExistingDir(_) => "Exists -> will archive",
                                                ConflictKind::ExistingFiles(_) => "Overwrite -> will archive",
                                            };
                                            widgets::pill(ui, badge, WARN);
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format!("-> {}", item.dest_rel.display()))
                                            .font(theme::mono(11.5))
                                            .color(MUTED),
                                    );
                                });
                                ui.add_space(4.0);
                            }
                            if !plan.skipped_root_files.is_empty() {
                                ui.label(
                                    RichText::new(format!(
                                        "Skipped loose root files: {}",
                                        plan.skipped_root_files.join(", ")
                                    ))
                                    .small()
                                    .color(MUTED),
                                );
                            }
                        }
                        Err(message) => {
                            ui.label(
                                RichText::new(format!("This package cannot be installed: {message}"))
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
                        "Atomic staging with rollback: existing content is moved to acxs_archive/ \
                         before replacement, and any mid-batch error rolls back earlier items.",
                    )
                    .color(MUTED),
                );
                ui.add_space(8.0);
                if !root_valid {
                    ui.label(
                        RichText::new("The Assetto Corsa folder is not valid. Fix it in Settings first.")
                            .color(WARN),
                    );
                } else if game_running {
                    ui.label(
                        RichText::new("Assetto Corsa is running. Close it before installing content.")
                            .color(WARN),
                    );
                }
                ui.horizontal(|ui| {
                    let btn_label = if selected_count > 1 {
                        format!("Install {selected_count} items")
                    } else {
                        "Install".to_string()
                    };
                    if ui
                        .add_enabled(can_install, widgets::primary_button(&btn_label))
                        .clicked()
                        && let Some(Ok(plan)) = self.picked.as_ref().map(|p| &p.plan)
                    {
                        self.start_install(&egui_ctx, root.to_path_buf(), plan.clone());
                    }
                    if self.installing {
                        ui.label(RichText::new("Installing...").color(MUTED));
                    }
                });
            });

            ui.add_space(16.0);
            widgets::section_title(ui, "Installed");
            theme::card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Filter").color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.installed_filter)
                            .hint_text("Name, id or kind")
                            .desired_width(260.0),
                    );
                    if self.scanning {
                        ui.label(RichText::new("Reading folders...").color(MUTED));
                    }
                });
                ui.add_space(8.0);

                let filter = self.installed_filter.trim().to_string();
                let shown: Vec<&InstalledRow> = self
                    .installed
                    .iter()
                    .filter(|row| {
                        util::matches(&filter, &[row.name.as_str(), row.id.as_str(), kind_name(row.kind)])
                    })
                    .collect();

                if self.installed.is_empty() {
                    let text = if self.scanning {
                        "Reading the content folder..."
                    } else {
                        "No cars or tracks found in the content folder."
                    };
                    ui.label(RichText::new(text).color(MUTED));
                } else {
                    ui.label(
                        RichText::new(format!("{} of {} shown", shown.len(), self.installed.len()))
                            .color(MUTED),
                    );
                    ui.add_space(4.0);
                    for row in shown {
                        ui.horizontal(|ui| {
                            widgets::pill(ui, kind_name(row.kind), MUTED);
                            ui.label(RichText::new(&row.name).strong());
                            ui.label(RichText::new(&row.id).color(MUTED));
                        });
                    }
                }
            });
        });
    }
}

impl PackagesPage {
    fn poll(&mut self, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let outcome = self.inspect_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok((path, plan))) => {
                self.picked = Some(Picked { path, plan });
                self.inspecting = false;
                self.inspect_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.inspecting = false;
                self.inspect_rx = None;
                notices.push(Notice {
                    text: "Inspecting the package stopped unexpectedly.".into(),
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
                self.needs_scan = true;
                notices.push(Notice {
                    text: batch_install_notice(&report),
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

        let outcome = self.scan_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok(rows)) => {
                self.installed = rows;
                self.scanning = false;
                self.scan_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.scanning = false;
                self.scan_rx = None;
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

    fn start_inspect(&mut self, ctx: &egui::Context, path: PathBuf, ac_root: Option<PathBuf>) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let plan = installer::inspect_package(&path, ac_root.as_deref(), None)
                .map_err(|e| e.to_string());
            let _ = tx.send((path, plan));
            repaint.request_repaint();
        });
        self.inspect_rx = Some(rx);
        self.inspecting = true;
    }

    fn start_install(&mut self, ctx: &egui::Context, root: PathBuf, plan: PackagePlan) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let running = game::is_game_running();
            let result = installer::install_plan(&root, &plan, running, None)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.install_rx = Some(rx);
        self.installing = true;
    }

    fn start_scan(&mut self, ctx: &egui::Context, root: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        self.scanned_root = Some(root.clone());
        thread::spawn(move || {
            let cars = content::scan_cars(&root)
                .into_iter()
                .map(|car| InstalledRow {
                    name: car.name,
                    id: car.id,
                    kind: ContentKind::Car,
                });
            let tracks = content::scan_tracks(&root)
                .into_iter()
                .map(|track| InstalledRow {
                    name: track.name,
                    id: track.id,
                    kind: ContentKind::Track,
                });
            let rows: Vec<InstalledRow> = cars.chain(tracks).collect();
            let _ = tx.send(rows);
            repaint.request_repaint();
        });
        self.scan_rx = Some(rx);
        self.scanning = true;
        self.needs_scan = false;
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

fn format_label(fmt: ArchiveFormat) -> &'static str {
    match fmt {
        ArchiveFormat::Zip => "ZIP",
        ArchiveFormat::SevenZ => "7Z",
        ArchiveFormat::TarGz => "TAR.GZ",
        ArchiveFormat::Rar => "RAR",
        ArchiveFormat::Folder => "FOLDER",
        ArchiveFormat::SingleFile => "INI",
    }
}

fn kind_name(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Car => "Car",
        ContentKind::Track => "Track",
    }
}

fn batch_install_notice(report: &BatchInstallReport) -> String {
    if report.installed.len() == 1 {
        let item = &report.installed[0];
        let mut text = format!(
            "Installed {} {} ({} files) to {}.",
            item.kind.label().to_lowercase(),
            item.id,
            item.file_count,
            item.installed_to.display()
        );
        if let Some(old) = &item.moved_existing_to {
            text.push_str(&format!(" Previous copy archived to {}.", old.display()));
        }
        return text;
    }
    let archived = report
        .installed
        .iter()
        .filter(|i| i.moved_existing_to.is_some())
        .count();
    let mut text = format!("Installed {} package items.", report.installed.len());
    if archived > 0 {
        text.push_str(&format!(" {archived} replaced item(s) archived safely."));
    }
    text
}
