//! RTX & ReShade page: shows which graphics runtimes are in the Assetto Corsa
//! folder, installs them from build folders the user picks, archives them,
//! lists ReShade presets with their techniques, and sets the Remix quality level.
//!
//! Every file read and write runs on a background thread. The thread sends one
//! `Finished` message with the outcome and a fresh status snapshot, and the UI
//! picks it up with `try_recv` on each frame. Results from an older AC root are
//! dropped by comparing a generation counter.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED, OK, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::game;
use acxs_manager::graphics_check::{self, ProxyFile, Role};
use acxs_manager::remix::{self, QualityPreset, RemixInstall};
use acxs_manager::reshade::{self, ReshadeConfig, ReshadeInstall};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    Box::new(RtxPage::new())
}

/// Quality levels offered for the Remix config, in button order.
const QUALITY_LEVELS: [(QualityPreset, &str); 3] = [
    (QualityPreset::Performance, "Performance"),
    (QualityPreset::Balanced, "Balanced"),
    (QualityPreset::Quality, "Quality"),
];

/// Which runtime an archive button refers to. Archiving needs a second click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    ReShade,
    Remix,
}

/// Everything the page shows about the Assetto Corsa folder.
struct Status {
    game_running: bool,
    reshade: Option<ReshadeInstall>,
    /// `Ok(None)` when there is no `ReShade.ini` yet.
    reshade_config: Result<Option<ReshadeConfig>, String>,
    presets: Vec<Preset>,
    remix: Option<RemixInstall>,
    /// `Ok(None)` when there is no `rtx.conf`; otherwise the validation problems.
    rtx: Result<Option<Vec<String>>, String>,
    proxies: Result<Vec<ProxyFile>, String>,
    conflicts: Vec<String>,
    order: Vec<String>,
}

struct Preset {
    name: String,
    active: bool,
    techniques: Result<Vec<String>, String>,
}

/// A user intention produced while drawing. It is applied after drawing ends,
/// so the drawing code never needs mutable access to the page.
enum Action {
    Refresh,
    Arm(Target),
    Archive(Target),
    InstallReshade(PathBuf),
    InstallRemix(PathBuf),
    Quality(QualityPreset),
    SavePerformance(bool),
}

/// Sent once per background job, after the work and the status re-read.
struct Finished {
    generation: u64,
    notice: Option<Notice>,
    status: Box<Status>,
}

struct RtxPage {
    root: Option<PathBuf>,
    /// Bumped whenever the AC root changes, so late results are ignored.
    generation: u64,
    status: Option<Status>,
    busy: bool,
    armed: Option<Target>,
    perf_draft: bool,
    tx: Sender<Finished>,
    rx: Receiver<Finished>,
}

impl RtxPage {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            root: None,
            generation: 0,
            status: None,
            busy: false,
            armed: None,
            perf_draft: false,
            tx,
            rx,
        }
    }

    /// Run `job` and then re-read the folder, both on a background thread.
    fn run(
        &mut self,
        egui_ctx: &egui::Context,
        root: PathBuf,
        job: impl FnOnce(&Path) -> Option<Notice> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        let generation = self.generation;
        let tx = self.tx.clone();
        let repaint = egui_ctx.clone();
        thread::spawn(move || {
            let notice = job(&root);
            let status = gather(&root);
            let _ = tx.send(Finished { generation, notice, status: Box::new(status) });
            repaint.request_repaint();
        });
    }

    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(done) = self.rx.try_recv() {
            if done.generation != self.generation {
                continue;
            }
            self.busy = false;
            if let Some(notice) = done.notice {
                notices.push(notice);
            }
            let status = *done.status;
            self.perf_draft = status
                .reshade_config
                .as_ref()
                .ok()
                .and_then(|cfg| cfg.as_ref())
                .and_then(|cfg| cfg.performance_mode)
                .unwrap_or(false);
            self.status = Some(status);
        }
    }

    fn apply(&mut self, egui_ctx: &egui::Context, root: &Path, action: Action) {
        match action {
            Action::Refresh => self.run(egui_ctx, root.to_path_buf(), |_: &Path| None),
            Action::Arm(target) => self.armed = Some(target),
            Action::Archive(target) => {
                self.armed = None;
                match target {
                    Target::ReShade => self.run(egui_ctx, root.to_path_buf(), |root: &Path| {
                        Some(archive_reshade(root))
                    }),
                    Target::Remix => self.run(egui_ctx, root.to_path_buf(), |root: &Path| {
                        Some(archive_remix(root))
                    }),
                }
            }
            Action::InstallReshade(build) => {
                self.run(egui_ctx, root.to_path_buf(), move |root: &Path| {
                    Some(install_reshade(root, &build))
                })
            }
            Action::InstallRemix(build) => {
                self.run(egui_ctx, root.to_path_buf(), move |root: &Path| {
                    Some(install_remix(root, &build))
                })
            }
            Action::Quality(preset) => {
                self.run(egui_ctx, root.to_path_buf(), move |root: &Path| {
                    Some(apply_quality(root, preset))
                })
            }
            Action::SavePerformance(on) => {
                self.perf_draft = on;
                self.run(egui_ctx, root.to_path_buf(), move |root: &Path| {
                    Some(save_performance(root, on))
                })
            }
        }
    }
}

impl Tool for RtxPage {
    fn name(&self) -> &'static str {
        "RTX & ReShade"
    }

    fn group(&self) -> &'static str {
        "Graphics"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        let egui_ctx = ui.ctx().clone();

        widgets::heading(
            ui,
            "RTX & ReShade",
            "Install and manage the RTX Remix runtime and ReShade for Assetto Corsa.",
        );

        let root = ctx.ac_root.clone().filter(|_| ctx.ac_valid);
        if self.root != root {
            self.root = root.clone();
            self.generation += 1;
            self.status = None;
            self.busy = false;
            self.armed = None;
            if let Some(root) = root.clone() {
                self.run(&egui_ctx, root, |_: &Path| None);
            }
        }

        let Some(root) = root else {
            widgets::empty_state(
                ui,
                "Set your Assetto Corsa folder on the Home page to manage RTX and ReShade.",
            );
            return;
        };

        let busy = self.busy;
        let armed = self.armed;
        let perf_draft = self.perf_draft;
        let action = match &self.status {
            Some(status) => egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| draw(ui, status, busy, armed, perf_draft))
                .inner,
            None => {
                ui.label(RichText::new("Reading the Assetto Corsa folder...").color(MUTED));
                None
            }
        };
        if let Some(action) = action {
            self.apply(&egui_ctx, &root, action);
        }
    }
}

/// Draws every section and returns the one action the user asked for, if any.
fn draw(
    ui: &mut egui::Ui,
    status: &Status,
    busy: bool,
    armed: Option<Target>,
    perf_draft: bool,
) -> Option<Action> {
    let mut action = None;
    let game_running = status.game_running;

    theme::card().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Assetto Corsa").strong());
            let label = if game_running { "running" } else { "not running" };
            widgets::pill(ui, label, theme::pill_color(label));
            if ui.add_enabled(!busy, egui::Button::new("Refresh")).clicked() {
                action = Some(Action::Refresh);
            }
            if busy {
                ui.label(RichText::new("Working...").color(MUTED));
            }
        });
        if game_running {
            ui.label(
                RichText::new("Close Assetto Corsa before installing, archiving or changing a runtime.")
                    .small()
                    .color(MUTED),
            );
        }
    });

    ui.add_space(16.0);
    widgets::section_title(ui, "Graphics DLLs");
    theme::card().show(ui, |ui| match &status.proxies {
        Err(err) => {
            ui.label(RichText::new(format!("Could not read the Assetto Corsa folder: {err}")).color(WARN));
        }
        Ok(files) if files.is_empty() => {
            ui.label(RichText::new("No graphics proxy DLLs in the Assetto Corsa folder.").color(MUTED));
        }
        Ok(files) => {
            for file in files {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&file.name).monospace());
                    let label = role_label(file.role);
                    widgets::pill(ui, label, theme::INFO);
                });
            }
        }
    });

    if !status.conflicts.is_empty() {
        ui.add_space(16.0);
        widgets::section_title(ui, "Conflict warnings");
        theme::card().show(ui, |ui| {
            for conflict in &status.conflicts {
                ui.label(RichText::new(conflict).color(WARN));
                ui.add_space(4.0);
            }
            if !status.order.is_empty() {
                ui.label(RichText::new("Suggested setup order").strong());
                for (index, step) in status.order.iter().enumerate() {
                    ui.label(format!("{}. {step}", index + 1));
                }
            }
        });
    }

    ui.add_space(16.0);
    widgets::section_title(ui, "ReShade");
    theme::card().show(ui, |ui| {
        match &status.reshade {
            Some(install) => {
                ui.horizontal(|ui| widgets::pill(ui, "installed", OK));
                widgets::key_value(ui, "Proxy file", &install.proxy_name);
                widgets::key_value(ui, "Version", install.version.as_deref().unwrap_or("unknown"));
                widgets::key_value(ui, "Presets", &status.presets.len().to_string());
            }
            None => {
                ui.horizontal(|ui| widgets::pill(ui, "not installed", WARN));
            }
        }

        ui.add_space(8.0);
        match &status.reshade_config {
            Err(err) => {
                ui.label(RichText::new(format!("ReShade.ini could not be read: {err}")).color(WARN));
            }
            Ok(None) => {
                ui.label(RichText::new("No ReShade.ini in the Assetto Corsa folder yet.").color(MUTED));
            }
            Ok(Some(_)) => {
                let mut draft = perf_draft;
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!busy, egui::Checkbox::new(&mut draft, "Performance mode"))
                        .changed()
                    {
                        action = Some(Action::SavePerformance(draft));
                    }
                });
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy && !game_running,
                    widgets::primary_button("Install from build folder..."),
                )
                .clicked()
                && let Some(build) = rfd::FileDialog::new().pick_folder()
            {
                action = Some(Action::InstallReshade(build));
            }
            archive_button(
                ui,
                &mut action,
                Target::ReShade,
                armed,
                busy || game_running || status.reshade.is_none(),
                "Archive ReShade",
            );
        });
    });

    ui.add_space(16.0);
    widgets::section_title(ui, "ReShade presets");
    theme::card().show(ui, |ui| {
        if status.presets.is_empty() {
            ui.label(RichText::new("No presets in the reshade-presets folder.").color(MUTED));
        }
        for preset in &status.presets {
            let title = if preset.active {
                format!("{}  (active)", preset.name)
            } else {
                preset.name.clone()
            };
            ui.collapsing(title, |ui| match &preset.techniques {
                Err(err) => {
                    ui.label(RichText::new(format!("Could not read this preset: {err}")).color(WARN));
                }
                Ok(list) if list.is_empty() => {
                    ui.label(RichText::new("No techniques listed.").color(MUTED));
                }
                Ok(list) => {
                    for technique in list {
                        ui.label(RichText::new(technique).monospace());
                    }
                }
            });
        }
    });

    ui.add_space(16.0);
    widgets::section_title(ui, "RTX Remix");
    theme::card().show(ui, |ui| {
        match &status.remix {
            Some(install) => {
                ui.horizontal(|ui| widgets::pill(ui, "installed", OK));
                let version = install.version.as_deref().unwrap_or("unknown");
                widgets::key_value(ui, "Version", version);
                let config = if install.config.is_some() { "found" } else { "missing" };
                widgets::key_value(ui, "rtx.conf", config);
                let bridge = if install.bridge.is_some() { "found" } else { "not found" };
                widgets::key_value(ui, "Bridge", bridge);
            }
            None => {
                ui.horizontal(|ui| widgets::pill(ui, "not installed", WARN));
            }
        }

        match &status.rtx {
            Err(err) => {
                ui.label(RichText::new(format!("rtx.conf could not be read: {err}")).color(WARN));
            }
            Ok(Some(problems)) => {
                for problem in problems {
                    ui.label(RichText::new(format!("rtx.conf: {problem}")).color(WARN));
                }
            }
            Ok(None) => {}
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy && !game_running,
                    widgets::primary_button("Install from build folder..."),
                )
                .clicked()
                && let Some(build) = rfd::FileDialog::new().pick_folder()
            {
                action = Some(Action::InstallRemix(build));
            }
            archive_button(
                ui,
                &mut action,
                Target::Remix,
                armed,
                busy || game_running || status.remix.is_none(),
                "Archive Remix",
            );
        });

        ui.add_space(8.0);
        widgets::section_title(ui, "Quality preset (rtx.conf)");
        let has_config = status.remix.as_ref().is_some_and(|r| r.config.is_some());
        ui.horizontal(|ui| {
            for (preset, label) in QUALITY_LEVELS {
                if ui
                    .add_enabled(!busy && has_config, egui::Button::new(label))
                    .clicked()
                {
                    action = Some(Action::Quality(preset));
                }
            }
        });
        if !has_config {
            ui.label(
                RichText::new("Quality presets need an rtx.conf in the Assetto Corsa folder.")
                    .small()
                    .color(MUTED),
            );
        }
    });

    action
}

/// Archive button with a confirm step: the first click arms it, the second runs it.
fn archive_button(
    ui: &mut egui::Ui,
    action: &mut Option<Action>,
    target: Target,
    armed: Option<Target>,
    disabled: bool,
    text: &str,
) {
    let confirming = armed == Some(target);
    let label = if confirming { "Confirm archive" } else { text };
    let button = egui::Button::new(label);
    if ui.add_enabled(!disabled, button).clicked() {
        *action = Some(if confirming {
            Action::Archive(target)
        } else {
            Action::Arm(target)
        });
    }
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::LightsPatch => "lighting patch",
        Role::ReShade => "ReShade",
        Role::RemixRuntime => "RTX Remix",
        Role::Other => "unknown owner",
    }
}

fn note(text: impl Into<String>, ok: bool) -> Notice {
    Notice { text: text.into(), ok }
}

/// Reads every file the page shows. Runs on a background thread.
fn gather(root: &Path) -> Status {
    let game_running = game::is_game_running();

    let config_path = root.join(reshade::CONFIG_NAME);
    let reshade_config = if config_path.is_file() {
        reshade::read_config(&config_path)
            .map(Some)
            .map_err(|err| err.to_string())
    } else {
        Ok(None)
    };
    let active_name = reshade_config
        .as_ref()
        .ok()
        .and_then(|cfg| cfg.as_ref())
        .and_then(|cfg| cfg.preset_path.as_ref())
        .and_then(|path| path.file_name().map(|name| name.to_os_string()));
    let presets = reshade::list_presets(root)
        .into_iter()
        .map(|path| Preset {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            active: active_name.as_deref() == path.file_name(),
            techniques: reshade::preset_techniques(&path).map_err(|err| err.to_string()),
        })
        .collect();

    let remix = remix::detect(root);
    let rtx = match remix.as_ref().and_then(|install| install.config.as_ref()) {
        Some(path) => remix::read_rtx(path)
            .map(|cfg| Some(remix::validate(&cfg)))
            .map_err(|err| err.to_string()),
        None => Ok(None),
    };

    let proxies = graphics_check::scan(root).map_err(|err| err.to_string());
    let (conflicts, order) = match &proxies {
        Ok(files) => (graphics_check::conflicts(files), graphics_check::recommended_order(files)),
        Err(_) => (Vec::new(), Vec::new()),
    };

    Status {
        game_running,
        reshade: reshade::detect(root),
        reshade_config,
        presets,
        remix,
        rtx,
        proxies,
        conflicts,
        order,
    }
}

fn install_reshade(root: &Path, build: &Path) -> Notice {
    let running = game::is_game_running();
    match reshade::install(root, build, running) {
        Ok(report) => {
            let mut text = format!("ReShade installed as {}.", report.proxy_name);
            if let Some(dir) = report.archived_to {
                text.push_str(&format!(" The file it replaced was moved to {}.", dir.display()));
            }
            note(text, true)
        }
        Err(err) => note(err.to_string(), false),
    }
}

fn archive_reshade(root: &Path) -> Notice {
    let running = game::is_game_running();
    match reshade::uninstall(root, running) {
        Ok(Some(dir)) => note(format!("ReShade archived to {}.", dir.display()), true),
        Ok(None) => note("No ReShade file was installed, so nothing was archived.", true),
        Err(err) => note(err.to_string(), false),
    }
}

fn install_remix(root: &Path, build: &Path) -> Notice {
    let running = game::is_game_running();
    match remix::install(root, build, running) {
        Ok(report) if report.files.is_empty() => note("Nothing was copied from the build folder.", false),
        Ok(report) => {
            let mut text = format!("RTX Remix installed: {}.", report.files.join(", "));
            if let Some(dir) = report.archived_to {
                text.push_str(&format!(" Replaced files were moved to {}.", dir.display()));
            }
            note(text, true)
        }
        Err(err) => note(err.to_string(), false),
    }
}

fn archive_remix(root: &Path) -> Notice {
    let running = game::is_game_running();
    match remix::uninstall(root, running) {
        Ok(Some(dir)) => note(format!("RTX Remix archived to {}.", dir.display()), true),
        Ok(None) => note("No RTX Remix files were installed, so nothing was archived.", true),
        Err(err) => note(err.to_string(), false),
    }
}

fn apply_quality(root: &Path, preset: QualityPreset) -> Notice {
    let label = QUALITY_LEVELS
        .iter()
        .find(|(level, _)| *level == preset)
        .map(|(_, label)| *label)
        .unwrap_or("selected");
    let Some(path) = remix::detect(root).and_then(|install| install.config) else {
        return note("No rtx.conf found in the Assetto Corsa folder.", false);
    };
    let mut cfg = match remix::read_rtx(&path) {
        Ok(cfg) => cfg,
        Err(err) => return note(format!("rtx.conf could not be read: {err}"), false),
    };
    remix::apply_quality(&mut cfg, preset);
    let problems = remix::validate(&cfg);
    if let Err(err) = remix::write_rtx(&path, &cfg) {
        return note(format!("rtx.conf could not be saved: {err}"), false);
    }
    if problems.is_empty() {
        note(format!("RTX quality set to {label}."), true)
    } else {
        note(
            format!(
                "RTX quality set to {label}. The config still has {} issue(s) listed on the RTX Remix card.",
                problems.len()
            ),
            false,
        )
    }
}

fn save_performance(root: &Path, on: bool) -> Notice {
    let path = root.join(reshade::CONFIG_NAME);
    let cfg = ReshadeConfig {
        performance_mode: Some(on),
        ..ReshadeConfig::default()
    };
    match reshade::write_config(&path, &cfg) {
        Ok(()) => note(
            format!("ReShade performance mode {}.", if on { "on" } else { "off" }),
            true,
        ),
        Err(err) => note(format!("ReShade.ini could not be saved: {err}"), false),
    }
}
