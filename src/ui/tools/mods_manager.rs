//! Mods page: disable and enable installed cars and tracks without deleting them,
//! list, install and archive user Lua apps, and edit the RARE settings.
//!
//! Every file operation and the game check run on a background thread. Results come
//! back over a channel that is drained at the start of each frame.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::content::{scan_cars, scan_tracks, CarEntry, TrackEntry};
use acxs_manager::game;
use acxs_manager::lua_apps::{self, AppScope, LuaApp};
use acxs_manager::mods::{self, ContentKind, DisabledItem, ModError};
use acxs_manager::rare::{self, RareSettings};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(ModsPage {
        tx,
        rx,
        pending: 0,
        section: Section::Content,
        loaded_for: None,
        cars: Vec::new(),
        tracks: Vec::new(),
        disabled: Vec::new(),
        apps: Vec::new(),
        remove_confirmed: false,
        rare_installed: None,
        rare_error: None,
        rare_disk: None,
        rare_edit: None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    Content,
    Lua,
    Rare,
}

/// Results sent from background threads to the UI.
enum Msg {
    Content {
        cars: Vec<CarEntry>,
        tracks: Vec<TrackEntry>,
        disabled: Vec<DisabledItem>,
    },
    Apps(Vec<LuaApp>),
    Rare {
        installed: bool,
        settings: Result<RareSettings, String>,
    },
    Notice(Notice),
    Finished,
}

/// What a button asked for. Built while drawing, run after the frame so the
/// drawing code never needs to borrow the page mutably for a spawn.
enum Action {
    Move { kind: ContentKind, id: String, disable: bool },
    SaveRare(RareSettings),
    InstallApp(PathBuf),
    RemoveApp(String),
}

struct ModsPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Background jobs still running. Each job sends `Finished` when it ends, even on panic.
    pending: usize,
    section: Section,
    /// The AC root the lists were last loaded for.
    loaded_for: Option<PathBuf>,
    cars: Vec<CarEntry>,
    tracks: Vec<TrackEntry>,
    disabled: Vec<DisabledItem>,
    apps: Vec<LuaApp>,
    /// The user must tick this before any user app can be removed.
    remove_confirmed: bool,
    /// `None` until the first RARE check has finished.
    rare_installed: Option<bool>,
    rare_error: Option<String>,
    /// The settings as last read from or written to disk.
    rare_disk: Option<RareSettings>,
    /// The settings shown in the form, possibly with unsaved edits.
    rare_edit: Option<RareSettings>,
}

/// Sends `Finished` when dropped, so a job always ends the busy state.
struct Finish {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Drop for Finish {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Finished);
        self.ctx.request_repaint();
    }
}

fn load_content(root: &Path, tx: &Sender<Msg>) {
    let _ = tx.send(Msg::Content {
        cars: scan_cars(root),
        tracks: scan_tracks(root),
        disabled: mods::list_disabled(root),
    });
}

fn load_apps(root: &Path, tx: &Sender<Msg>) {
    let _ = tx.send(Msg::Apps(lua_apps::list_all(root)));
}

fn load_rare(root: &Path, tx: &Sender<Msg>) {
    let installed = rare::is_installed(root);
    let settings = if installed {
        rare::read_settings(root).map_err(|e| e.to_string())
    } else {
        Ok(RareSettings::default())
    };
    let _ = tx.send(Msg::Rare { installed, settings });
}

/// Move one car or track into or out of the disabled store.
fn change_content(
    root: &Path,
    kind: ContentKind,
    id: &str,
    disable: bool,
    game_running: bool,
) -> Result<(), ModError> {
    match (kind, disable) {
        (ContentKind::Car, true) => mods::disable_car(root, id, game_running).map(|_| ()),
        (ContentKind::Track, true) => mods::disable_track(root, id, game_running).map(|_| ()),
        (ContentKind::Car, false) => mods::enable_car(root, id, game_running).map(|_| ()),
        (ContentKind::Track, false) => mods::enable_track(root, id, game_running).map(|_| ()),
    }
}

impl ModsPage {
    /// Run `work` on a background thread. The job is counted as busy until it ends.
    fn spawn(&mut self, ctx: &egui::Context, work: impl FnOnce(&Sender<Msg>) + Send + 'static) {
        self.pending += 1;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _finish = Finish { tx: tx.clone(), ctx };
            work(&tx);
        });
    }

    fn refresh(&mut self, ctx: &egui::Context, root: &Path) {
        self.loaded_for = Some(root.to_path_buf());
        let dir = root.to_path_buf();
        self.spawn(ctx, move |tx| load_content(&dir, tx));
        let dir = root.to_path_buf();
        self.spawn(ctx, move |tx| load_apps(&dir, tx));
        let dir = root.to_path_buf();
        self.spawn(ctx, move |tx| load_rare(&dir, tx));
    }

    /// Apply results that background jobs sent since the last frame.
    fn drain(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Content { cars, tracks, disabled } => {
                    self.cars = cars;
                    self.tracks = tracks;
                    self.disabled = disabled;
                }
                Msg::Apps(apps) => self.apps = apps,
                Msg::Rare { installed, settings } => {
                    self.rare_installed = Some(installed);
                    match settings {
                        Ok(s) => {
                            self.rare_error = None;
                            self.rare_disk = Some(s.clone());
                            self.rare_edit = Some(s);
                        }
                        Err(e) => {
                            self.rare_error = Some(e);
                            self.rare_disk = None;
                            self.rare_edit = None;
                        }
                    }
                }
                Msg::Notice(notice) => notices.push(notice),
                Msg::Finished => self.pending = self.pending.saturating_sub(1),
            }
        }
    }

    fn perform(&mut self, action: Action, ctx: &egui::Context, root: &Path) {
        let root = root.to_path_buf();
        match action {
            Action::Move { kind, id, disable } => {
                self.spawn(ctx, move |tx| {
                    let game_running = game::is_game_running();
                    let verb = if disable { "Disabled" } else { "Enabled" };
                    let noun = match kind {
                        ContentKind::Car => "car",
                        ContentKind::Track => "track",
                    };
                    let notice = match change_content(&root, kind, &id, disable, game_running) {
                        Ok(()) => Notice { text: format!("{verb} {noun} {id}."), ok: true },
                        Err(e) => Notice { text: e.to_string(), ok: false },
                    };
                    let _ = tx.send(Msg::Notice(notice));
                    load_content(&root, tx);
                });
            }
            Action::SaveRare(settings) => {
                self.spawn(ctx, move |tx| {
                    let notice = match rare::write_settings(&root, &settings) {
                        Ok(()) => Notice { text: "Saved RARE settings.".into(), ok: true },
                        Err(e) => Notice {
                            text: format!("Could not save RARE settings: {e}"),
                            ok: false,
                        },
                    };
                    let _ = tx.send(Msg::Notice(notice));
                    load_rare(&root, tx);
                });
            }
            Action::InstallApp(source) => {
                self.spawn(ctx, move |tx| {
                    let game_running = game::is_game_running();
                    let notice = match lua_apps::install_app(&root, &source, AppScope::User, game_running) {
                        Ok(target) => Notice { text: format!("Installed app to {}.", target.display()), ok: true },
                        Err(e) => Notice { text: e.to_string(), ok: false },
                    };
                    let _ = tx.send(Msg::Notice(notice));
                    load_apps(&root, tx);
                });
            }
            Action::RemoveApp(id) => {
                self.remove_confirmed = false;
                self.spawn(ctx, move |tx| {
                    let game_running = game::is_game_running();
                    let notice = match lua_apps::remove_app(&root, &id, AppScope::User, game_running, false) {
                        Ok(archived) => Notice {
                            text: format!("Archived app {id} to {}.", archived.display()),
                            ok: true,
                        },
                        Err(e) => Notice { text: e.to_string(), ok: false },
                    };
                    let _ = tx.send(Msg::Notice(notice));
                    load_apps(&root, tx);
                });
            }
        }
    }

    fn draw_content(&self, ui: &mut egui::Ui, game_running: bool, root: &Path) -> Option<Action> {
        let mut action = None;
        let locked = self.pending > 0 || game_running;

        if game_running {
            ui.label(
                RichText::new("Assetto Corsa is running. Close it before disabling or enabling content.")
                    .color(theme::WARN),
            );
            ui.add_space(8.0);
        }

        widgets::section_title(ui, &format!("Installed cars ({})", self.cars.len()));
        if self.cars.is_empty() {
            ui.label(RichText::new("No cars found in content/cars.").color(theme::MUTED));
        }
        for car in &self.cars {
            let detail = join(&[car.id.clone(), car.brand.clone(), car.car_class.clone()]);
            item_row(ui, &car.name, &detail, |ui| {
                if ui.add_enabled(!locked, egui::Button::new("Disable")).clicked() {
                    action = Some(Action::Move {
                        kind: ContentKind::Car,
                        id: car.id.clone(),
                        disable: true,
                    });
                }
            });
        }

        ui.add_space(16.0);
        widgets::section_title(ui, &format!("Installed tracks ({})", self.tracks.len()));
        if self.tracks.is_empty() {
            ui.label(RichText::new("No tracks found in content/tracks.").color(theme::MUTED));
        }
        for track in &self.tracks {
            let detail = join(&[track.id.clone(), track.city.clone(), track.country.clone()]);
            item_row(ui, &track.name, &detail, |ui| {
                if ui.add_enabled(!locked, egui::Button::new("Disable")).clicked() {
                    action = Some(Action::Move {
                        kind: ContentKind::Track,
                        id: track.id.clone(),
                        disable: true,
                    });
                }
            });
        }

        ui.add_space(16.0);
        widgets::section_title(ui, &format!("Disabled ({})", self.disabled.len()));
        ui.label(
            RichText::new(format!(
                "Moved to {}. Nothing is deleted.",
                mods::disabled_root(root).display()
            ))
            .small()
            .color(theme::MUTED),
        );
        ui.add_space(4.0);
        if self.disabled.is_empty() {
            ui.label(RichText::new("Nothing is disabled.").color(theme::MUTED));
        }
        for item in &self.disabled {
            let (label, color) = match item.kind {
                ContentKind::Car => ("Car", theme::INFO),
                ContentKind::Track => ("Track", theme::ACCENT),
            };
            item_row(ui, &item.id, "", |ui| {
                if ui.add_enabled(!locked, egui::Button::new("Enable")).clicked() {
                    action = Some(Action::Move {
                        kind: item.kind,
                        id: item.id.clone(),
                        disable: false,
                    });
                }
                widgets::pill(ui, label, color);
            });
        }

        action
    }

    fn draw_apps(&mut self, ui: &mut egui::Ui, busy: bool, game_running: bool) -> Option<Action> {
        let locked = busy || game_running;
        let mut action = None;

        if game_running {
            ui.label(
                RichText::new("Assetto Corsa is running. Close it before installing or removing apps.")
                    .color(theme::WARN),
            );
            ui.add_space(8.0);
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!locked, widgets::primary_button("Install app...")).clicked()
                && let Some(folder) = rfd::FileDialog::new().pick_folder()
            {
                action = Some(Action::InstallApp(folder));
            }
            ui.label(
                RichText::new("Copies a folder with a manifest.ini into apps/lua.")
                    .small()
                    .color(theme::MUTED),
            );
        });
        ui.add_space(6.0);
        ui.add_enabled(
            !locked,
            egui::Checkbox::new(
                &mut self.remove_confirmed,
                "Allow removing user apps. A removed app is moved to the archive, not deleted.",
            ),
        );
        ui.add_space(8.0);

        if self.apps.is_empty() {
            widgets::empty_state(ui, "No Lua apps found in the CSP folder or apps/lua.");
            return action;
        }
        let confirmed = self.remove_confirmed;
        for app in &self.apps {
            let version = app.version.as_ref().map(|v| format!("v{v}")).unwrap_or_default();
            let author = app.author.as_ref().map(|a| format!("by {a}")).unwrap_or_default();
            let detail = join(&[version, author]);
            let (label, color) = match app.scope {
                AppScope::Csp => ("CSP", theme::INFO),
                AppScope::User => ("User", theme::ACCENT),
            };
            item_row(ui, &app.name, &detail, |ui| {
                widgets::pill(ui, label, color);
                if app.scope == AppScope::User
                    && ui.add_enabled(!locked && confirmed, egui::Button::new("Remove")).clicked()
                {
                    action = Some(Action::RemoveApp(app.id.clone()));
                }
            });
        }
        action
    }

    fn draw_rare(&mut self, ui: &mut egui::Ui, locked: bool) -> Option<Action> {
        let Self { rare_installed, rare_error, rare_disk, rare_edit, .. } = self;

        match rare_installed {
            None => {
                widgets::empty_state(ui, "Checking for RARE...");
                return None;
            }
            Some(false) => {
                widgets::empty_state(
                    ui,
                    "RARE is not installed. Install it under apps/lua/RARE, then rescan.",
                );
                return None;
            }
            Some(true) => {}
        }
        if let Some(err) = rare_error.as_ref() {
            ui.label(RichText::new(format!("Could not read RARE settings: {err}")).color(theme::DANGER));
            return None;
        }
        let edit = rare_edit.as_mut()?;

        let problems = rare::validate(edit);
        let dirty = rare_disk.as_ref() != Some(&*edit);
        let mut action = None;
        let mut revert = false;

        ui.add_enabled_ui(!locked, |ui| rare_form(ui, &mut *edit));

        for problem in &problems {
            ui.label(RichText::new(problem).color(theme::WARN));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let can_save = !locked && dirty && problems.is_empty();
            if ui.add_enabled(can_save, widgets::primary_button("Save RARE settings")).clicked() {
                action = Some(Action::SaveRare((*edit).clone()));
            }
            if ui.add_enabled(!locked && dirty, egui::Button::new("Revert")).clicked() {
                revert = true;
            }
            ui.label(
                RichText::new("Writes settings.ini. An existing file is kept as a backup.")
                    .small()
                    .color(theme::MUTED),
            );
        });
        if revert
            && let Some(disk) = rare_disk.as_ref()
        {
            *edit = disk.clone();
        }
        action
    }
}

impl Tool for ModsPage {
    fn name(&self) -> &'static str {
        "Mods"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let ectx = ui.ctx().clone();
        self.drain(notices);

        widgets::heading(
            ui,
            "Mods",
            "Turn cars and tracks off without deleting them. Review Lua apps and RARE settings.",
        );
        let Some(root) = ctx.ac_root.clone().filter(|_| ctx.ac_valid) else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to manage mods.");
            return;
        };
        if self.loaded_for.as_deref() != Some(root.as_path()) {
            self.refresh(&ectx, &root);
        }

        let busy = self.pending > 0;
        let mut section = self.section;
        let mut rescan = false;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut section, Section::Content, "Content");
            ui.selectable_value(&mut section, Section::Lua, "Lua apps");
            ui.selectable_value(&mut section, Section::Rare, "RARE");
            ui.add_space(12.0);
            if busy {
                ui.spinner();
                ui.label(RichText::new("Working...").color(theme::MUTED));
            }
            if ui.add_enabled(!busy, egui::Button::new("Rescan")).clicked() {
                rescan = true;
            }
        });
        self.section = section;
        if rescan {
            self.refresh(&ectx, &root);
        }
        ui.add_space(10.0);

        let game_running = ctx.game_running;
        let action = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match section {
                Section::Content => self.draw_content(ui, game_running, &root),
                Section::Lua => self.draw_apps(ui, busy, game_running),
                Section::Rare => self.draw_rare(ui, busy || game_running),
            })
            .inner;
        if let Some(action) = action {
            self.perform(action, &ectx, &root);
        }
    }
}

/// Join non-empty parts with a middle dot.
fn join(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("  ·  ")
}

/// A full-width card with a title and detail line on the left and actions on the right.
fn item_row(ui: &mut egui::Ui, title: &str, detail: &str, actions: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).strong());
                    if !detail.is_empty() {
                        ui.label(RichText::new(detail).small().color(theme::MUTED));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
            });
        });
}

fn rare_group(ui: &mut egui::Ui, title: &str, rows: impl FnOnce(&mut egui::Ui)) {
    widgets::section_title(ui, title);
    egui::Grid::new(title)
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, rows);
}

fn flag_row(ui: &mut egui::Ui, label: &str, key: &str, value: &mut bool) {
    ui.label(label).on_hover_text(key);
    ui.checkbox(value, "");
    ui.end_row();
}

fn number_row(ui: &mut egui::Ui, label: &str, key: &str, value: &mut f64, speed: f64) {
    ui.label(label).on_hover_text(key);
    ui.add(egui::DragValue::new(value).speed(speed));
    ui.end_row();
}

fn rare_form(ui: &mut egui::Ui, s: &mut RareSettings) {
    rare_group(ui, "Rules", |ui| {
        flag_row(ui, "DRS rules", "DRS_RULES", &mut s.drs_rules);
        number_row(ui, "DRS activation lap", "DRS_ACTIVATION_LAP", &mut s.drs_activation_lap, 1.0);
        number_row(ui, "DRS gap (ms)", "DRS_GAP_DELTA", &mut s.drs_gap_delta_ms, 10.0);
        flag_row(ui, "Disable DRS in the wet", "DRS_WET_DISABLE", &mut s.drs_wet_disable);
        flag_row(ui, "Restrict tyre compounds", "RESTRICT_COMPOUNDS", &mut s.restrict_compounds);
        flag_row(ui, "Correct compound colours", "CORRECT_COMPOUNDS_COLORS", &mut s.correct_compounds_colors);
        flag_row(ui, "Race refuelling", "RACE_REFUELING", &mut s.race_refueling);
        flag_row(ui, "Virtual safety car", "VSC_RULES", &mut s.vsc_rules);
        number_row(ui, "VSC init time (s)", "VSC_INIT_TIME", &mut s.vsc_init_time, 1.0);
        number_row(ui, "VSC deploy time (s)", "VSC_DEPLOY_TIME", &mut s.vsc_deploy_time, 1.0);
    });
    ui.add_space(12.0);
    rare_group(ui, "Driver", |ui| {
        flag_row(ui, "Fill player fuel tank", "TANK_FILL", &mut s.driver_tank_fill);
    });
    ui.add_space(12.0);
    rare_group(ui, "AI", |ui| {
        flag_row(ui, "AI pits for new tyres", "AI_FORCE_PIT_TYRES", &mut s.ai_force_pit_tyres);
        number_row(ui, "AI average tyre life (%)", "AI_AVG_TYRE_LIFE", &mut s.ai_avg_tyre_life, 0.5);
        number_row(ui, "AI average life variability (%)", "AI_AVG_TYRE_LIFE_RANGE", &mut s.ai_avg_tyre_life_range, 0.5);
        number_row(ui, "AI single tyre life (%)", "AI_SINGLE_TYRE_LIFE", &mut s.ai_single_tyre_life, 0.5);
        number_row(ui, "AI single tyre variability (%)", "AI_SINGLE_TYRE_LIFE_RANGE", &mut s.ai_single_tyre_life_range, 0.5);
        flag_row(ui, "More competitive AI level", "AI_ALTERNATE_LEVEL", &mut s.ai_alternate_level);
        flag_row(ui, "Relative AI scaling", "AI_RELATIVE_SCALING", &mut s.ai_relative_scaling);
        number_row(ui, "Relative AI level (%)", "AI_RELATIVE_LEVEL", &mut s.ai_relative_level, 0.5);
        flag_row(ui, "AI MGU-K control", "AI_MGUK_CONTROL", &mut s.ai_mguk_control);
        flag_row(ui, "Fill AI fuel tanks", "AI_TANK_FILL", &mut s.ai_tank_fill);
    });
    ui.add_space(12.0);
    rare_group(ui, "Audio", |ui| {
        number_row(ui, "Master volume (%)", "MASTER", &mut s.audio_master, 0.5);
        number_row(ui, "DRS beep volume (%)", "DRS_BEEP", &mut s.audio_drs_beep, 0.5);
        number_row(ui, "DRS flap volume (%)", "DRS_FLAP", &mut s.audio_drs_flap, 0.5);
    });
    ui.add_space(12.0);
    rare_group(ui, "Race control banner", |ui| {
        number_row(ui, "X position (px)", "X_POS", &mut s.notifications_x_pos, 1.0);
        number_row(ui, "Y position (px)", "Y_POS", &mut s.notifications_y_pos, 1.0);
        number_row(ui, "Scale", "SCALE", &mut s.notifications_scale, 0.05);
        number_row(ui, "Duration (s)", "DURATION", &mut s.notifications_duration, 0.1);
    });
    ui.add_space(12.0);
}
