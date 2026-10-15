//! External Launcher tool page: keep a list of third-party launch profiles in the
//! Assetto Corsa folder, edit them, and start one with the race file of the session
//! filled in. Profile reads and writes, the file dialog and process start run on
//! background threads. Results come back over a channel that is polled once per frame.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED, OK, WARN};
use crate::ui::{widgets, Notice};
use acxs_manager::launcher::{self, LaunchProfile, AC_ROOT_TOKEN, RACE_INI_TOKEN};
use eframe::egui::{self, RichText, TextEdit};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const PROFILES_FILE: &str = "acxs-launchers.json";

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(LauncherPage {
        tx,
        rx,
        root: None,
        profiles: Vec::new(),
        ready: false,
        loading: false,
        saving: false,
        picking: false,
        revision: 0,
        saved_revision: 0,
        editor: None,
    })
}

/// Sent back by the background threads once they finish.
enum Msg {
    Loaded {
        path: PathBuf,
        result: Result<Vec<LaunchProfile>, String>,
    },
    Saved {
        path: PathBuf,
        revision: u64,
        result: Result<(), String>,
    },
    Picked(Option<PathBuf>),
    Launched(Notice),
}

/// A user action collected while drawing. Applied after the frame is drawn.
enum Action {
    Add,
    Edit(usize),
    Remove(usize),
    Launch(usize),
    Save,
    Browse,
    Apply,
    Cancel,
}

/// The form for one profile. `index` is `None` for a new profile.
struct Editor {
    index: Option<usize>,
    /// The profile being edited. Its working folder and environment are kept as they are.
    base: Option<LaunchProfile>,
    name: String,
    program: String,
    args: String,
}

impl Editor {
    fn blank() -> Self {
        Self {
            index: None,
            base: None,
            name: String::new(),
            program: String::new(),
            args: String::new(),
        }
    }

    fn of(index: usize, profile: &LaunchProfile) -> Self {
        Self {
            index: Some(index),
            base: Some(profile.clone()),
            name: profile.name.clone(),
            program: profile.program.display().to_string(),
            args: profile.args.join("\n"),
        }
    }
}

struct LauncherPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The Assetto Corsa folder the profiles belong to.
    root: Option<PathBuf>,
    profiles: Vec<LaunchProfile>,
    /// True once the profiles file for `root` was read. Saving is refused before that,
    /// so an unreadable file is never overwritten with an empty list.
    ready: bool,
    loading: bool,
    saving: bool,
    /// True while a file dialog is open on a background thread.
    picking: bool,
    /// Bumped on every change. `saved_revision` is the revision last written to disk.
    revision: u64,
    saved_revision: u64,
    editor: Option<Editor>,
}

impl Tool for LauncherPage {
    fn name(&self) -> &'static str {
        "External Launcher"
    }

    fn group(&self) -> &'static str {
        "Launch"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let egui_ctx = ui.ctx().clone();
        self.poll(notices);
        self.sync_root(ctx.ac_root.as_deref(), &egui_ctx, notices);

        widgets::heading(
            ui,
            "External Launcher",
            "Start Assetto Corsa through third-party launchers. Each profile runs its program with the race file of the session.",
        );

        let Some(root) = self.root.clone() else {
            widgets::empty_state(ui, "Set the Assetto Corsa folder to manage launch profiles.");
            return;
        };

        let mut actions = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.draw(ui, &root, &mut actions));

        for action in actions {
            self.apply(action, &egui_ctx, notices);
        }
    }
}

impl LauncherPage {
    fn draw(&mut self, ui: &mut egui::Ui, root: &Path, actions: &mut Vec<Action>) {
        self.toolbar(ui, root, actions);
        if let Some(editor) = self.editor.as_mut() {
            editor_card(ui, editor, self.picking, actions);
            ui.add_space(12.0);
        }
        self.profile_list(ui, actions);
    }

    fn toolbar(&self, ui: &mut egui::Ui, root: &Path, actions: &mut Vec<Action>) {
        theme::card().show(ui, |ui| {
            ui.label(
                RichText::new(format!("File: {}", profiles_path(root).display()))
                    .small()
                    .color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.can_save(), widgets::primary_button("Save profiles"))
                    .clicked()
                {
                    actions.push(Action::Save);
                }
                let can_add = self.ready && self.editor.is_none();
                if ui.add_enabled(can_add, egui::Button::new("Add profile")).clicked() {
                    actions.push(Action::Add);
                }
                if self.loading {
                    ui.spinner();
                    ui.label(RichText::new("Loading...").color(MUTED));
                } else if self.saving {
                    ui.spinner();
                    ui.label(RichText::new("Saving...").color(MUTED));
                } else if !self.ready {
                    ui.label(RichText::new("The file could not be read.").color(WARN));
                } else if self.dirty() {
                    ui.label(RichText::new("Unsaved changes").color(WARN));
                } else {
                    ui.label(RichText::new("All changes saved").color(OK));
                }
            });
        });
    }

    fn profile_list(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        widgets::section_title(ui, "Profiles");
        if self.loading {
            return;
        }
        if !self.ready {
            widgets::empty_state(
                ui,
                "The profiles file could not be read. Fix or move it, then reopen this page. See the notice for details.",
            );
            return;
        }
        if self.profiles.is_empty() {
            widgets::empty_state(ui, "No launch profiles yet. Add one to start Assetto Corsa through a launcher.");
            return;
        }

        let editing = self.editor.is_some();
        for (i, profile) in self.profiles.iter().enumerate() {
            theme::card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(profile.name.as_str()).strong());
                        ui.label(
                            RichText::new(profile.program.display().to_string())
                                .small()
                                .color(MUTED),
                        );
                        if !profile.args.is_empty() {
                            ui.label(RichText::new(profile.args.join("  ")).small().color(MUTED));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(widgets::primary_button("Launch")).clicked() {
                            actions.push(Action::Launch(i));
                        }
                        if ui.add_enabled(!editing, egui::Button::new("Remove")).clicked() {
                            actions.push(Action::Remove(i));
                        }
                        if ui.add_enabled(!editing, egui::Button::new("Edit")).clicked() {
                            actions.push(Action::Edit(i));
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        match action {
            Action::Add => self.editor = Some(Editor::blank()),
            Action::Edit(i) => {
                if let Some(profile) = self.profiles.get(i) {
                    self.editor = Some(Editor::of(i, profile));
                }
            }
            Action::Remove(i) => {
                if i < self.profiles.len() {
                    let removed = self.profiles.remove(i);
                    self.revision += 1;
                    notices.push(Notice {
                        text: format!("Removed {}.", removed.name),
                        ok: true,
                    });
                }
            }
            Action::Launch(i) => {
                if let (Some(profile), Some(root)) = (self.profiles.get(i).cloned(), self.root.clone()) {
                    self.spawn_launch(ctx, profile, root);
                }
            }
            Action::Save => self.spawn_save(ctx),
            Action::Browse => self.spawn_browse(ctx),
            Action::Apply => report(notices, self.apply_editor()),
            Action::Cancel => self.editor = None,
        }
    }

    fn apply_editor(&mut self) -> Result<String, String> {
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "No profile is open for editing.".to_string())?;

        let name = editor.name.trim().to_string();
        if name.is_empty() {
            return Err("Give the profile a name.".into());
        }
        let program = editor.program.trim();
        if program.is_empty() {
            return Err("Choose the program to start.".into());
        }
        let key = name.to_lowercase();
        let clash = self
            .profiles
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != editor.index && p.name.trim().to_lowercase() == key);
        if clash {
            return Err(format!("Another profile is already named \"{name}\"."));
        }

        let args = editor
            .args
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        let profile = LaunchProfile {
            name: name.clone(),
            program: PathBuf::from(program),
            args,
            working_dir: editor.base.as_ref().and_then(|b| b.working_dir.clone()),
            env: editor.base.as_ref().map(|b| b.env.clone()).unwrap_or_default(),
        };
        let index = editor.index;

        self.editor = None;
        self.revision += 1;
        match index {
            Some(i) => {
                self.profiles[i] = profile;
                Ok(format!("Updated {name}."))
            }
            None => {
                self.profiles.push(profile);
                Ok(format!("Added {name}. Save profiles to keep it."))
            }
        }
    }

    fn can_save(&self) -> bool {
        self.ready && !self.loading && !self.saving && self.dirty()
    }

    fn dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// True when `path` is the profiles file of the current folder.
    fn is_current(&self, path: &Path) -> bool {
        self.root
            .as_deref()
            .is_some_and(|root| profiles_path(root).as_path() == path)
    }

    /// Switches to the profiles of a new folder and loads them. Unsaved changes to the
    /// old folder's profiles are dropped and reported.
    fn sync_root(&mut self, root: Option<&Path>, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        if self.root.as_deref() == root {
            return;
        }
        if self.dirty() {
            notices.push(Notice {
                text: "Unsaved launch profile changes were discarded because the Assetto Corsa folder changed.".into(),
                ok: false,
            });
        }
        self.root = root.map(Path::to_path_buf);
        self.profiles.clear();
        self.editor = None;
        self.ready = false;
        self.loading = false;
        self.saving = false;
        self.revision = 0;
        self.saved_revision = 0;

        if let Some(root) = self.root.clone() {
            self.loading = true;
            self.spawn_load(ctx, profiles_path(&root));
        }
    }

    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded { path, result } => {
                    if !self.is_current(&path) {
                        continue;
                    }
                    self.loading = false;
                    match result {
                        Ok(profiles) => {
                            self.profiles = profiles;
                            self.ready = true;
                        }
                        Err(text) => {
                            self.ready = false;
                            notices.push(Notice {
                                text: format!("Could not read launch profiles: {text}"),
                                ok: false,
                            });
                        }
                    }
                }
                Msg::Saved { path, revision, result } => {
                    if !self.is_current(&path) {
                        continue;
                    }
                    self.saving = false;
                    match result {
                        Ok(()) => {
                            self.saved_revision = revision;
                            notices.push(Notice {
                                text: "Launch profiles saved.".into(),
                                ok: true,
                            });
                        }
                        Err(text) => notices.push(Notice {
                            text: format!("Could not save launch profiles: {text}"),
                            ok: false,
                        }),
                    }
                }
                Msg::Picked(picked) => {
                    self.picking = false;
                    if let (Some(path), Some(editor)) = (picked, self.editor.as_mut()) {
                        match path.to_str() {
                            Some(text) => editor.program = text.to_string(),
                            None => notices.push(Notice {
                                text: "That file name is not valid text and cannot be used in a launch profile.".into(),
                                ok: false,
                            }),
                        }
                    }
                }
                Msg::Launched(notice) => notices.push(notice),
            }
        }
    }

    fn spawn_load(&self, ctx: &egui::Context, path: PathBuf) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = launcher::load_profiles(&path).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Loaded { path, result });
            ctx.request_repaint();
        });
    }

    fn spawn_save(&mut self, ctx: &egui::Context) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.saving = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let profiles = self.profiles.clone();
        let revision = self.revision;
        thread::spawn(move || {
            let path = profiles_path(&root);
            let result = launcher::save_profiles(&path, &profiles).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Saved { path, revision, result });
            ctx.request_repaint();
        });
    }

    fn spawn_browse(&mut self, ctx: &egui::Context) {
        self.picking = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let start = self.root.clone();
        thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title("Choose the program to start");
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            let picked = dialog.pick_file();
            let _ = tx.send(Msg::Picked(picked));
            ctx.request_repaint();
        });
    }

    /// Expands the placeholders and starts the program on a thread. The result is
    /// reported as a notice, and the thread then waits for the process so it does not
    /// stay a zombie after it exits.
    fn spawn_launch(&self, ctx: &egui::Context, profile: LaunchProfile, root: PathBuf) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let name = profile.name.clone();
            let race_ini = race_ini_path(&root);
            let started = launcher::expand(&profile, &root, &race_ini)
                .map_err(|e| e.to_string())
                .and_then(|spec| launcher::spawn(&spec).map_err(|e| e.to_string()));
            match started {
                Ok(mut child) => {
                    let notice = Notice {
                        text: format!("Started {name} (process {}).", child.id()),
                        ok: true,
                    };
                    let _ = tx.send(Msg::Launched(notice));
                    ctx.request_repaint();
                    let _ = child.wait();
                }
                Err(text) => {
                    let notice = Notice {
                        text: format!("Could not start {name}: {text}"),
                        ok: false,
                    };
                    let _ = tx.send(Msg::Launched(notice));
                    ctx.request_repaint();
                }
            }
        });
    }
}

fn editor_card(ui: &mut egui::Ui, editor: &mut Editor, picking: bool, actions: &mut Vec<Action>) {
    theme::card().show(ui, |ui| {
        widgets::section_title(ui, if editor.index.is_some() { "Edit profile" } else { "New profile" });
        ui.add(
            TextEdit::singleline(&mut editor.name)
                .hint_text("Name")
                .desired_width(320.0),
        );
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut editor.program)
                    .hint_text("Program")
                    .desired_width(420.0),
            );
            if ui.add_enabled(!picking, egui::Button::new("Browse...")).clicked() {
                actions.push(Action::Browse);
            }
            if picking {
                ui.spinner();
            }
        });
        ui.label(
            RichText::new(format!(
                "Arguments, one per line. Blank lines are ignored. Use {AC_ROOT_TOKEN} for the Assetto Corsa folder and {RACE_INI_TOKEN} for the race file."
            ))
            .small()
            .color(MUTED),
        );
        ui.add(
            TextEdit::multiline(&mut editor.args)
                .hint_text("-config\n{race_ini}")
                .desired_rows(4)
                .desired_width(520.0),
        );
        ui.horizontal(|ui| {
            if ui.add(widgets::primary_button("Apply")).clicked() {
                actions.push(Action::Apply);
            }
            if ui.button("Cancel").clicked() {
                actions.push(Action::Cancel);
            }
        });
    });
}

fn report(notices: &mut Vec<Notice>, result: Result<String, String>) {
    match result {
        Ok(text) => notices.push(Notice { text, ok: true }),
        Err(text) => notices.push(Notice { text, ok: false }),
    }
}

fn profiles_path(root: &Path) -> PathBuf {
    root.join(PROFILES_FILE)
}

fn race_ini_path(root: &Path) -> PathBuf {
    root.join("settings").join("cfg").join("race.ini")
}
