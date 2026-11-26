//! Setups tool page: browse car setups, edit single values, import and archive
//! them, compare two setups and run a three-way merge. All file IO runs on background threads;
//! results come back over a channel that is drained once per frame.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::SystemTime;

use acxs_manager::content::{self, CarEntry, TrackEntry};
use acxs_manager::ini::Ini;
use acxs_manager::ini_merge::{self, MergeConflict, MergeResult};
use acxs_manager::laptimes::format_lap;
use acxs_manager::session_recorder::{self, RecordedSession};
use acxs_manager::setup_ab;
use acxs_manager::setup_schema::{self, SetupSchema};
use acxs_manager::setups::{self, SetupDiff, SetupFile};
use eframe::egui::{self, RichText};

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, INFO, MUTED, OK, TEXT, WARN};
use crate::ui::widgets::{self, PendingAction, PendingChangesBar};
use crate::ui::Notice;

/// Folder under the setups root that holds archived setups. It is not a car.
const ARCHIVE_DIR: &str = "_archive";
/// Conflicts listed in full before the rest are summarized.
const MAX_CONFLICTS_SHOWN: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspaceTab {
    Editor,
    CompareMerge,
    AbTesting,
}

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(SetupsPage {
        ac_root: None,
        root_input: default_root(),
        import_track: "generic".to_string(),
        tab: WorkspaceTab::Editor,
        car_query: String::new(),
        installed_cars: Vec::new(),
        installed_tracks: Vec::new(),
        catalog_scanned_for: None,
        started: false,
        error: None,
        cars: Vec::new(),
        car: None,
        requested_car: None,
        setups: Vec::new(),
        selected: None,
        left: None,
        right: None,
        compare: None,
        base: None,
        ours: None,
        theirs: None,
        merge: None,
        allow_conflicts: false,
        editor_for: None,
        editor: None,
        schema_for: None,
        schema: None,
        ab_sessions: None,
        ab_track: String::new(),
        ab_hash_a: String::new(),
        ab_hash_b: String::new(),
        busy: 0,
        tx,
        rx,
    })
}

struct SetupsPage {
    ac_root: Option<PathBuf>,
    /// Setups root, edited as text or picked with Browse.
    root_input: String,
    /// Track folder (inside the car folder) that imports go into. The game lists
    /// setups per track; `generic` is offered on every track.
    import_track: String,
    /// Active tab in the right workspace.
    tab: WorkspaceTab,
    /// Search filter for the visual car selector list.
    car_query: String,
    /// Installed cars scanned from `ac_root` for rich brand/name display.
    installed_cars: Vec<CarEntry>,
    /// Installed tracks scanned from `ac_root` for the track pill selector.
    installed_tracks: Vec<TrackEntry>,
    /// `ac_root` for which installed cars/tracks were last scanned.
    catalog_scanned_for: Option<PathBuf>,
    /// The first scan happens on the first frame the page is drawn.
    started: bool,
    /// Why the setups root could not be read, shown in the page.
    error: Option<String>,
    cars: Vec<String>,
    car: Option<String>,
    /// The car whose setups were last requested. The setup list reloads when `car` differs.
    requested_car: Option<String>,
    setups: Vec<SetupFile>,
    selected: Option<PathBuf>,
    left: Option<PathBuf>,
    right: Option<PathBuf>,
    compare: Option<Comparison>,
    base: Option<PathBuf>,
    ours: Option<PathBuf>,
    theirs: Option<PathBuf>,
    merge: Option<MergePreview>,
    allow_conflicts: bool,
    /// The setup whose values were last requested. The editor reloads when `selected` differs.
    editor_for: Option<PathBuf>,
    /// Editable values of `editor_for`, once they have been read.
    editor: Option<ValueEditor>,
    /// Car id whose `setup.ini` schema is cached.
    schema_for: Option<String>,
    schema: Option<SetupSchema>,
    /// Cached recorded sessions for Setup A/B testing.
    ab_sessions: Option<Vec<RecordedSession>>,
    ab_track: String,
    ab_hash_a: String,
    ab_hash_b: String,
    /// Number of background jobs still running. Actions are disabled while this is above zero.
    busy: usize,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

/// Editable copy of one setup file's values.
struct ValueEditor {
    path: PathBuf,
    rows: Vec<ValueRow>,
    /// Adding keys is refused by the library unless this is ticked.
    new_key_enabled: bool,
    new_section: String,
    new_key: String,
    new_value: String,
}

struct ValueRow {
    section: String,
    key: String,
    /// The value as read from disk. The row counts as edited while `text` differs from it.
    original: String,
    text: String,
}

impl ValueEditor {
    fn new(path: PathBuf, values: Vec<(String, String, String)>) -> ValueEditor {
        let rows = values
            .into_iter()
            .map(|(section, key, value)| ValueRow { section, key, original: value.clone(), text: value })
            .collect();
        ValueEditor {
            path,
            rows,
            new_key_enabled: false,
            new_section: String::new(),
            new_key: String::new(),
            new_value: String::new(),
        }
    }

    /// The writes Save would make, as `(section, key, value, allow_new_key)`.
    fn pending(&self) -> Vec<(String, String, String, bool)> {
        let mut out: Vec<_> = self
            .rows
            .iter()
            .filter(|row| row.text != row.original)
            .map(|row| (row.section.clone(), row.key.clone(), row.text.clone(), false))
            .collect();
        if self.new_key_enabled && !self.new_key.trim().is_empty() {
            out.push((self.new_section.clone(), self.new_key.clone(), self.new_value.clone(), true));
        }
        out
    }
}

struct Comparison {
    title: String,
    diffs: Vec<SetupDiff>,
}

struct MergePreview {
    title: String,
    /// The file that `Apply` writes.
    ours: PathBuf,
    result: MergeResult,
}

/// Results sent back from background jobs. Every job sends exactly one message.
enum Msg {
    Cars(Result<Vec<String>, String>),
    Catalog { cars: Vec<CarEntry>, tracks: Vec<TrackEntry> },
    Setups { car: String, setups: Vec<SetupFile> },
    Compared(Comparison),
    Merged(MergePreview),
    /// A file operation finished. Carries the reloaded setup list for `car`.
    Done { car: String, setups: Vec<SetupFile>, notice: Notice },
    /// The values of `path` were read, or a save to it finished. `result` is what is on disk now.
    Values {
        path: PathBuf,
        result: Result<Vec<(String, String, String)>, String>,
        notice: Option<Notice>,
    },
    Notice(Notice),
}

/// Button presses, collected while drawing and acted on after drawing.
enum Intent {
    Rescan,
    Import,
    Archive,
    Compare,
    Merge,
    Apply,
    SaveValues,
}

impl Tool for SetupsPage {
    fn name(&self) -> &'static str {
        "Setups"
    }

    fn content_width(&self) -> f32 {
        f32::INFINITY
    }

    fn group(&self) -> &'static str {
        "Setup"
    }

    fn ui(&mut self, ui: &mut egui::Ui, tool_ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.ac_root = tool_ctx.ac_root.clone();
        let ctx = ui.ctx().clone();
        if !self.started {
            self.started = true;
            self.rescan(&ctx);
        }
        if self.ac_root != self.catalog_scanned_for {
            self.catalog_scanned_for = self.ac_root.clone();
            if let Some(root) = self.ac_root.clone() {
                self.spawn(&ctx, move || Msg::Catalog {
                    cars: content::scan_cars(&root),
                    tracks: content::scan_tracks(&root),
                });
            } else {
                self.installed_cars.clear();
                self.installed_tracks.clear();
            }
        }

        self.drain(notices);
        if self.car != self.requested_car {
            self.request_car(&ctx);
        }
        self.sync_values(&ctx);

        let mut intent = None;
        self.draw(ui, &mut intent);
        match intent {
            Some(Intent::Rescan) => self.rescan(&ctx),
            Some(Intent::Import) => {
                if let Some(source) = rfd::FileDialog::new().add_filter("ini", &["ini"]).pick_file() {
                    self.import(&ctx, source);
                }
            }
            Some(Intent::Archive) => self.archive(&ctx),
            Some(Intent::Compare) => self.compare(&ctx),
            Some(Intent::Merge) => self.merge(&ctx),
            Some(Intent::Apply) => self.apply(&ctx),
            Some(Intent::SaveValues) => self.save_values(&ctx),
            None => {}
        }
    }
}

impl SetupsPage {
    fn root(&self) -> PathBuf {
        PathBuf::from(self.root_input.trim())
    }

    /// Run `job` on a thread. The job's message is sent back and the UI is woken up.
    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Msg + Send + 'static) {
        self.busy += 1;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Msg::Notice(Notice { text: "A setup task failed unexpectedly.".into(), ok: false }));
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// Apply finished background results to the page.
    fn drain(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = self.busy.saturating_sub(1);
            match msg {
                Msg::Cars(Ok(cars)) => {
                    self.error = None;
                    self.cars = cars;
                    if !self.car.as_ref().is_some_and(|c| self.cars.contains(c)) {
                        self.car = self.cars.first().cloned();
                    }
                }
                Msg::Cars(Err(e)) => {
                    self.error = Some(e);
                    self.cars.clear();
                    self.car = None;
                }
                Msg::Catalog { cars, tracks } => {
                    self.installed_cars = cars;
                    self.installed_tracks = tracks;
                }
                Msg::Setups { car, setups } => {
                    if self.car.as_deref() == Some(car.as_str()) {
                        self.set_setups(setups);
                    }
                }
                Msg::Compared(comparison) => self.compare = Some(comparison),
                Msg::Merged(preview) => self.merge = Some(preview),
                Msg::Done { car, setups, notice } => {
                    notices.push(notice);
                    if self.car.as_deref() == Some(car.as_str()) {
                        self.set_setups(setups);
                    }
                }
                Msg::Values { path, result, notice } => {
                    notices.extend(notice);
                    if self.editor_for.as_ref() == Some(&path) {
                        match result {
                            Ok(values) => self.editor = Some(ValueEditor::new(path, values)),
                            Err(e) => {
                                self.editor = None;
                                notices.push(Notice { text: format!("Could not read the values: {e}"), ok: false });
                            }
                        }
                    }
                }
                Msg::Notice(notice) => notices.push(notice),
            }
        }
    }

    /// Replace the setup list and clear any selection that no longer exists in it.
    fn set_setups(&mut self, setups: Vec<SetupFile>) {
        prune(&mut self.selected, &setups);
        prune(&mut self.left, &setups);
        prune(&mut self.right, &setups);
        prune(&mut self.base, &setups);
        prune(&mut self.ours, &setups);
        prune(&mut self.theirs, &setups);
        self.setups = setups;
    }

    /// Scan the setups root for car folders.
    fn rescan(&mut self, ctx: &egui::Context) {
        // Force the setup list to reload for the car in the (possibly new) root.
        self.requested_car = None;
        let root = self.root();
        self.spawn(ctx, move || {
            if root.as_os_str().is_empty() {
                return Msg::Cars(Err("Set the setups folder above.".into()));
            }
            Msg::Cars(scan_cars(&root).map_err(|e| format!("Could not read {}: {e}", root.display())))
        });
    }

    /// Called when `car` changed: drop everything tied to the old car and load the new one.
    fn request_car(&mut self, ctx: &egui::Context) {
        self.requested_car = self.car.clone();
        self.setups.clear();
        self.selected = None;
        self.left = None;
        self.right = None;
        self.compare = None;
        self.base = None;
        self.ours = None;
        self.theirs = None;
        self.merge = None;

        if let Some(car) = self.car.clone() {
            let root = self.root();
            self.spawn(ctx, move || Msg::Setups { setups: sorted_setups(&root, &car), car });
        }
    }

    /// Keep the value editor in step with the selected setup. Reads the file when the selection changes.
    fn sync_values(&mut self, ctx: &egui::Context) {
        if self.selected == self.editor_for {
            return;
        }
        self.editor_for = self.selected.clone();
        self.editor = None;
        if let Some(path) = self.selected.clone() {
            self.spawn(ctx, move || Msg::Values {
                result: setups::values(&path).map_err(|e| e.to_string()),
                path,
                notice: None,
            });
        }
    }

    /// Write every changed value, one `set_value` call each, then read the file back.
    fn save_values(&mut self, ctx: &egui::Context) {
        let Some(editor) = &self.editor else { return };
        let path = editor.path.clone();
        let edits = editor.pending();
        if edits.is_empty() {
            return;
        }
        let total = edits.len();
        self.spawn(ctx, move || {
            let mut saved = 0;
            let mut failure = None;
            for (section, key, value, allow_new) in &edits {
                match setups::set_value(&path, section, key, value, *allow_new) {
                    Ok(()) => saved += 1,
                    Err(e) => {
                        failure = Some(format!("{section}/{key}: {e}"));
                        break;
                    }
                }
            }
            let notice = match failure {
                None => Notice {
                    text: format!("Saved {saved} value(s) to {}.ini. Previous versions were kept as backups.", stem(&path)),
                    ok: true,
                },
                Some(e) => Notice { text: format!("Saved {saved} of {total} value(s), then stopped at {e}"), ok: false },
            };
            Msg::Values {
                result: setups::values(&path).map_err(|e| e.to_string()),
                path,
                notice: Some(notice),
            }
        });
    }

    fn import(&mut self, ctx: &egui::Context, source: PathBuf) {
        let Some(car) = self.car.clone() else { return };
        let root = self.root();
        let track = match self.import_track.trim() {
            "" => "generic".to_string(),
            other => other.to_string(),
        };
        self.spawn(ctx, move || {
            let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let clash = root.join(&car).join(&track).join(&name).is_file();
            let notice = match setups::import_setup_for_track(&root, &car, Some(&track), &source) {
                Ok(_) => Notice {
                    text: format!(
                        "Imported {name} into {car}/{track}.{}",
                        if clash { " The old file with that name was kept as a backup." } else { "" }
                    ),
                    ok: true,
                },
                Err(e) => Notice { text: format!("Import failed: {e}"), ok: false },
            };
            Msg::Done { setups: sorted_setups(&root, &car), car, notice }
        });
    }

    fn archive(&mut self, ctx: &egui::Context) {
        let (Some(car), Some(selected)) = (self.car.clone(), self.selected.clone()) else { return };
        let root = self.root();
        let name = stem(&selected);
        let track = self.setups.iter().find(|s| s.path == selected).and_then(|s| s.track.clone());
        self.spawn(ctx, move || {
            let notice = match setups::archive_setup_in(&root, &car, track.as_deref(), &name) {
                Ok(dest) => Notice {
                    text: format!("Archived {name}.ini to {}. Nothing was deleted.", dest.display()),
                    ok: true,
                },
                Err(e) => Notice { text: format!("Archive failed: {e}"), ok: false },
            };
            Msg::Done { setups: sorted_setups(&root, &car), car, notice }
        });
    }

    fn compare(&mut self, ctx: &egui::Context) {
        let (Some(left), Some(right)) = (self.left.clone(), self.right.clone()) else { return };
        let title = format!("{} vs {}", stem(&left), stem(&right));
        self.spawn(ctx, move || match (Ini::load(&left), Ini::load(&right)) {
            (Ok(l), Ok(r)) => Msg::Compared(Comparison { title, diffs: setups::diff(&l, &r) }),
            (Err(e), _) | (_, Err(e)) => Msg::Notice(Notice { text: format!("Compare failed: {e}"), ok: false }),
        });
    }

    fn merge(&mut self, ctx: &egui::Context) {
        let (Some(base), Some(ours), Some(theirs)) = (self.base.clone(), self.ours.clone(), self.theirs.clone()) else {
            return;
        };
        let title = format!("Base {} / Ours {} / Theirs {}", stem(&base), stem(&ours), stem(&theirs));
        self.spawn(ctx, move || match load_all(&[&base, &ours, &theirs]) {
            Ok(inis) => {
                let result = ini_merge::three_way(&inis[0], &inis[1], &inis[2]);
                Msg::Merged(MergePreview { title, ours, result })
            }
            Err(e) => Msg::Notice(Notice { text: format!("Merge failed: {e}"), ok: false }),
        });
    }

    fn apply(&mut self, ctx: &egui::Context) {
        let (Some(preview), Some(car)) = (self.merge.take(), self.car.clone()) else { return };
        let root = self.root();
        let allow = self.allow_conflicts;
        self.spawn(ctx, move || {
            let name = stem(&preview.ours);
            let notice = match ini_merge::apply_merge(&preview.ours, &preview.result, allow) {
                Ok(()) => Notice {
                    text: format!("Wrote {name}.ini. The previous version was kept as a backup."),
                    ok: true,
                },
                Err(e) => Notice { text: format!("Merge not applied: {e}"), ok: false },
            };
            Msg::Done { setups: sorted_setups(&root, &car), car, notice }
        });
    }

    fn draw(&mut self, ui: &mut egui::Ui, intent: &mut Option<Intent>) {
        widgets::heading(
            ui,
            "Setups",
            "Browse car setups, edit values with car schemas, compare, 3-way merge, or run stint A/B tests.",
        );
        let busy = self.busy > 0;
        let car_names = self.ac_root.clone().map(|root| super::names::names(&root, ui.ctx()));

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            if avail >= 880.0 {
                let left_w = 360.0;
                let gap = 16.0;
                let right_w = (avail - left_w - gap).max(400.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_w, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(left_w);
                            self.draw_left_browser(ui, busy, car_names.as_deref(), intent);
                        },
                    );
                    ui.add_space(gap);
                    ui.allocate_ui_with_layout(
                        egui::vec2(right_w, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(right_w);
                            self.draw_right_workspace(ui, busy, intent);
                        },
                    );
                });
            } else {
                self.draw_left_browser(ui, busy, car_names.as_deref(), intent);
                ui.add_space(14.0);
                self.draw_right_workspace(ui, busy, intent);
            }
        });
    }

    fn draw_left_browser(
        &mut self,
        ui: &mut egui::Ui,
        busy: bool,
        car_names: Option<&super::names::Names>,
        intent: &mut Option<Intent>,
    ) {
        // Build merged car list: existing setup folders first, plus installed cars.
        let mut all_car_ids: Vec<String> = self.cars.clone();
        for c in &self.installed_cars {
            if !all_car_ids.iter().any(|id| id == &c.id) {
                all_car_ids.push(c.id.clone());
            }
        }

        // Visual searchable car selector card.
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                widgets::section_title(ui, "Cars");
                widgets::pill(ui, &all_car_ids.len().to_string(), INFO);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Rescan")).clicked() {
                        *intent = Some(Intent::Rescan);
                    }
                    if busy {
                        ui.spinner();
                    }
                });
            });

            if all_car_ids.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("No setups or installed cars found yet.")
                        .color(MUTED),
                );
                ui.label(
                    RichText::new("Set your Assetto Corsa folder in Settings or import a setup .ini file.")
                        .small()
                        .color(theme::FAINT),
                );
                ui.add_space(8.0);
                if ui.add_enabled(!busy, widgets::primary_button("Rescan Setups")).clicked() {
                    *intent = Some(Intent::Rescan);
                }
            } else {
                ui.add(
                    egui::TextEdit::singleline(&mut self.car_query)
                        .desired_width(ui.available_width())
                        .hint_text("Search cars by name or brand..."),
                );
                ui.add_space(6.0);
                let q = self.car_query.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt("setups_car_list")
                    .max_height(210.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for car_id in &all_car_ids {
                            let meta = self.installed_cars.iter().find(|c| &c.id == car_id);
                            let display_name = meta
                                .map(|m| m.name.clone())
                                .unwrap_or_else(|| car_names.map_or(car_id.clone(), |n| n.car(car_id)));
                            let brand = meta.map(|m| m.brand.as_str()).unwrap_or("");
                            if !q.is_empty()
                                && !display_name.to_lowercase().contains(&q)
                                && !brand.to_lowercase().contains(&q)
                                && !car_id.to_lowercase().contains(&q)
                            {
                                continue;
                            }
                            let selected = self.car.as_deref() == Some(car_id.as_str());
                            let has_folder = self.cars.iter().any(|c| c == car_id);
                            if widgets::list_row(ui, selected, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 1.0;
                                        ui.label(RichText::new(&display_name).font(theme::bold(13.0)).color(TEXT));
                                        let sub = if brand.is_empty() { car_id.as_str() } else { brand };
                                        ui.label(RichText::new(sub).small().color(MUTED));
                                    });
                                    if has_folder {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            widgets::pill(ui, "SAVED", OK);
                                        });
                                    }
                                });
                            }) {
                                self.car = Some(car_id.clone());
                            }
                            ui.add_space(3.0);
                        }
                    });
            }

            // Track pill selector + Import / Archive actions.
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);
            ui.label(RichText::new("IMPORT TARGET TRACK").font(theme::display(12.0)).color(MUTED));
            ui.add_space(4.0);
            egui::ScrollArea::horizontal()
                .id_salt("setups_track_pills")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(self.import_track == "generic", "Generic (All)").clicked() {
                            self.import_track = "generic".to_string();
                        }
                        for track in self.installed_tracks.iter().take(12) {
                            let active = self.import_track == track.id;
                            if ui.selectable_label(active, &track.name).clicked() {
                                self.import_track = track.id.clone();
                            }
                        }
                    });
                });

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!busy && self.car.is_some(), widgets::primary_button("Import .ini..."))
                    .clicked()
                {
                    *intent = Some(Intent::Import);
                }
                if ui
                    .add_enabled(!busy && self.selected.is_some(), egui::Button::new("Archive selected"))
                    .clicked()
                {
                    *intent = Some(Intent::Archive);
                }
            });
        });

        ui.add_space(14.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let car_title = self
                .car
                .as_deref()
                .map(|id| car_names.map_or(id.to_string(), |n| n.car(id)))
                .unwrap_or_else(|| "No Car".into());
            ui.horizontal(|ui| {
                widgets::section_title(ui, &format!("Setups · {car_title}"));
                widgets::pill(ui, &self.setups.len().to_string(), INFO);
            });
            if self.car.is_none() {
                widgets::empty_state(ui, "Pick a car above to browse its setups.");
                ui.add_space(12.0);
            } else if self.setups.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("No setups saved for this car yet.").color(MUTED));
                ui.add_space(8.0);
                if ui.add_enabled(!busy, widgets::primary_button("Import .ini Setup...")).clicked() {
                    *intent = Some(Intent::Import);
                }
                ui.add_space(6.0);
            } else {
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("setups_left_list")
                    .max_height(380.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for setup in &self.setups {
                            let selected = self.selected.as_ref() == Some(&setup.path);
                            if widgets::list_row(ui, selected, |ui| setup_row(ui, setup)) {
                                self.selected = Some(setup.path.clone());
                            }
                            ui.add_space(4.0);
                        }
                    });
            }
        });
    }

    fn draw_right_workspace(&mut self, ui: &mut egui::Ui, busy: bool, intent: &mut Option<Intent>) {
        let pending_badge = self
            .editor
            .as_ref()
            .map(|e| e.pending().len())
            .filter(|&n| n > 0)
            .map(|n| n.to_string());
        widgets::animated_tabs(
            ui,
            "setups_workspace_tabs",
            &mut self.tab,
            &[
                (WorkspaceTab::Editor, "Editor", pending_badge.as_deref()),
                (WorkspaceTab::CompareMerge, "Compare & 3-Way Merge", None),
                (WorkspaceTab::AbTesting, "A/B Testing", None),
            ],
        );
        ui.add_space(10.0);

        match self.tab {
            WorkspaceTab::Editor => self.draw_values(ui, busy, intent),
            WorkspaceTab::CompareMerge => self.draw_compare_merge(ui, busy, intent),
            WorkspaceTab::AbTesting => self.draw_ab_testing(ui),
        }
    }

    fn draw_compare_merge(&mut self, ui: &mut egui::Ui, busy: bool, intent: &mut Option<Intent>) {
        // Compare two setups.
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Compare Two Setups");
            ui.horizontal_wrapped(|ui| {
                setup_combo(ui, "setups_left", "Left", &self.setups, &mut self.left);
                setup_combo(ui, "setups_right", "Right", &self.setups, &mut self.right);
                let ready = self.left.is_some() && self.right.is_some() && self.left != self.right;
                if ui.add_enabled(!busy && ready, widgets::primary_button("Compare")).clicked() {
                    *intent = Some(Intent::Compare);
                }
            });
            if let Some(comparison) = &self.compare {
                show_comparison(ui, comparison);
            }
        });

        // Three-way merge.
        ui.add_space(14.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Three-Way Merge");
            ui.label(
                RichText::new(
                    "Changes from Theirs (relative to Base) are applied onto Ours. Ours is the file that gets written.",
                )
                .small()
                .color(MUTED),
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                setup_combo(ui, "setups_base", "Base", &self.setups, &mut self.base);
                setup_combo(ui, "setups_ours", "Ours", &self.setups, &mut self.ours);
                setup_combo(ui, "setups_theirs", "Theirs", &self.setups, &mut self.theirs);
                let ready = self.base.is_some() && self.ours.is_some() && self.theirs.is_some();
                if ui.add_enabled(!busy && ready, widgets::primary_button("Merge")).clicked() {
                    *intent = Some(Intent::Merge);
                }
            });

            if let Some(preview) = &self.merge {
                let result = &preview.result;
                let conflicts = !result.conflicts.is_empty();
                ui.add_space(8.0);
                ui.label(RichText::new(preview.title.as_str()).color(MUTED));
                ui.horizontal_wrapped(|ui| {
                    widgets::pill(ui, &format!("{} taken from Theirs", result.applied_from_theirs), INFO);
                    widgets::pill(ui, &format!("{} kept from Ours", result.applied_from_ours), INFO);
                    widgets::pill(
                        ui,
                        &format!("{} conflicts", result.conflicts.len()),
                        if conflicts { DANGER } else { OK },
                    );
                });
                for conflict in result.conflicts.iter().take(MAX_CONFLICTS_SHOWN) {
                    let MergeConflict::Both { section, key, base, ours, theirs } = conflict;
                    ui.label(
                        RichText::new(format!(
                            "[{section}] {key}: base {} / ours {} / theirs {}",
                            opt(base),
                            opt(ours),
                            opt(theirs)
                        ))
                        .font(theme::mono(12.0))
                        .color(WARN),
                    );
                }
                if result.conflicts.len() > MAX_CONFLICTS_SHOWN {
                    ui.label(
                        RichText::new(format!(
                            "...and {} more",
                            result.conflicts.len() - MAX_CONFLICTS_SHOWN
                        ))
                        .small()
                        .color(MUTED),
                    );
                }

                ui.checkbox(
                    &mut self.allow_conflicts,
                    "Write even with conflicts (conflicting keys keep the Ours value)",
                );
                let can_apply = !busy && (!conflicts || self.allow_conflicts);
                ui.horizontal(|ui| {
                    if ui.add_enabled(can_apply, widgets::primary_button("Apply to Ours")).clicked() {
                        *intent = Some(Intent::Apply);
                    }
                    ui.label(
                        RichText::new("A backup of the current Ours file is kept.").small().color(MUTED),
                    );
                });
            }
        });
    }

    /// Editor for the values of the selected setup. Nothing is written until Save is pressed.
    fn draw_values(&mut self, ui: &mut egui::Ui, busy: bool, intent: &mut Option<Intent>) {
        // Load car-specific setup.ini schema whenever car changes.
        if self.schema_for != self.car {
            self.schema_for = self.car.clone();
            self.schema = match (&self.ac_root, &self.car) {
                (Some(root), Some(car_id)) => {
                    setup_schema::read_setup_schema(&root.join("content/cars").join(car_id))
                }
                _ => None,
            };
        }

        let Some(selected) = self.selected.clone() else {
            theme::card_elevated().show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::empty_state(
                    ui,
                    "Select a setup on the left to inspect and edit its values.",
                );
                ui.add_space(32.0);
            });
            return;
        };

        let schema = self.schema.as_ref();
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, &format!("Values in {}.ini", stem(&selected)));
            let Some(editor) = self.editor.as_mut() else {
                ui.label(RichText::new("Reading the setup...").color(MUTED));
                return;
            };
            let pending_count = editor.pending().len();

            ui.add_enabled_ui(!busy, |ui| {
                if editor.rows.is_empty() {
                    widgets::empty_state(ui, "This setup has no values yet.");
                } else {
                    draw_value_groups(ui, &mut editor.rows, schema);
                }

                ui.add_space(10.0);
                ui.checkbox(&mut editor.new_key_enabled, "New key (allow adding keys to this setup)");
                ui.add_enabled_ui(editor.new_key_enabled, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.new_section)
                                .hint_text("Section")
                                .desired_width(140.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.new_key)
                                .hint_text("Key")
                                .desired_width(160.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.new_value)
                                .hint_text("Value")
                                .desired_width(220.0),
                        );
                    });
                });
            });

            ui.add_space(10.0);
            let bar = PendingChangesBar {
                changed_count: pending_count,
                summary: format!("{pending_count} pending setup value(s) — backup created on save"),
                can_save: !busy,
                blocked_reason: busy.then(|| "Saving...".into()),
            };
            match widgets::pending_changes_bar(ui, &bar) {
                PendingAction::Save => *intent = Some(Intent::SaveValues),
                PendingAction::RevertAll => {
                    for row in &mut editor.rows {
                        row.text = row.original.clone();
                    }
                    editor.new_section.clear();
                    editor.new_key.clear();
                    editor.new_value.clear();
                }
                PendingAction::ToggleDiff | PendingAction::None => {}
            }
        });
    }

    fn draw_ab_testing(&mut self, ui: &mut egui::Ui) {
        let Some(car_id) = self.car.clone() else {
            theme::card_elevated().show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::empty_state(ui, "Select a car on the left to compare recorded stints.");
                ui.add_space(32.0);
            });
            return;
        };
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Setup A/B Testing (Recorded Stints)");
            ui.horizontal(|ui| {
                if ui.button("Load recorded stints").clicked() {
                    let all = session_recorder::load_all(&session_recorder::sessions_dir());
                    self.ab_sessions = Some(all);
                }
                ui.label(
                    RichText::new("Compare pace, consistency, and balance across stints tagged with different setup hashes.")
                        .small()
                        .color(MUTED),
                );
            });
            let Some(sessions) = &self.ab_sessions else {
                return;
            };
            let car_sessions: Vec<&RecordedSession> = sessions
                .iter()
                .filter(|s| s.car_id.eq_ignore_ascii_case(&car_id) && s.setup_hash.is_some())
                .collect();
            if car_sessions.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("No recorded sessions with setup hashes found for this car yet.").color(MUTED));
                return;
            }
            let mut tracks: Vec<String> = car_sessions.iter().map(|s| s.full_track_id()).collect();
            tracks.sort();
            tracks.dedup();
            if !tracks.contains(&self.ab_track) {
                self.ab_track = tracks[0].clone();
            }
            let mut hashes: Vec<(String, String)> = car_sessions
                .iter()
                .filter(|s| s.full_track_id() == self.ab_track)
                .filter_map(|s| {
                    let h = s.setup_hash.clone()?;
                    let n = s.setup_name.clone().unwrap_or_else(|| h.chars().take(8).collect());
                    Some((h, n))
                })
                .collect();
            hashes.sort();
            hashes.dedup_by(|a, b| a.0 == b.0);
            if !hashes.iter().any(|(h, _)| h == &self.ab_hash_a) {
                self.ab_hash_a = hashes.first().map(|x| x.0.clone()).unwrap_or_default();
            }
            if !hashes.iter().any(|(h, _)| h == &self.ab_hash_b) {
                self.ab_hash_b = hashes.get(1).or(hashes.first()).map(|x| x.0.clone()).unwrap_or_default();
            }

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("ab_track")
                    .selected_text(&self.ab_track)
                    .show_ui(ui, |ui| {
                        for t in &tracks {
                            ui.selectable_value(&mut self.ab_track, t.clone(), t);
                        }
                    });
                egui::ComboBox::from_id_salt("ab_hash_a")
                    .selected_text(format!("A: {}", self.ab_hash_a.chars().take(8).collect::<String>()))
                    .show_ui(ui, |ui| {
                        for (h, n) in &hashes {
                            ui.selectable_value(&mut self.ab_hash_a, h.clone(), format!("{n} ({})", &h[..h.len().min(8)]));
                        }
                    });
                egui::ComboBox::from_id_salt("ab_hash_b")
                    .selected_text(format!("B: {}", self.ab_hash_b.chars().take(8).collect::<String>()))
                    .show_ui(ui, |ui| {
                        for (h, n) in &hashes {
                            ui.selectable_value(&mut self.ab_hash_b, h.clone(), format!("{n} ({})", &h[..h.len().min(8)]));
                        }
                    });
            });

            if let Some(card) = setup_ab::compare_stints(
                sessions,
                &car_id,
                &self.ab_track,
                &self.ab_hash_a,
                &self.ab_hash_b,
                self.left.as_deref(),
                self.right.as_deref(),
            ) {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if let Some(d) = card.mean_delta_ms {
                        let color = if d < 0.0 { OK } else { WARN };
                        widgets::pill(ui, &format!("Mean delta (B-A): {d:+.0} ms"), color);
                    }
                    if let Some(d) = card.best_delta_ms {
                        let color = if d < 0 { OK } else { WARN };
                        widgets::pill(ui, &format!("Best delta (B-A): {d:+} ms"), color);
                    }
                });
                ui.add_space(6.0);
                ui.columns(2, |cols| {
                    let (left, right) = cols.split_at_mut(1);
                    for (col, label, m) in [(&mut left[0], "Setup A", &card.a), (&mut right[0], "Setup B", &card.b)] {
                        theme::card_elevated().show(col, |ui| {
                            ui.label(RichText::new(format!("{label}: {}", m.setup_name)).strong());
                            widgets::key_value(ui, "Laps / sessions", &format!("{} / {}", m.valid_laps, m.session_count));
                            widgets::key_value(
                                ui,
                                "Best lap",
                                &m.best_lap_ms.map_or("—".into(), format_lap),
                            );
                            widgets::key_value(
                                ui,
                                "Consistency (stddev)",
                                &m.lap_stddev_ms.map_or("—".into(), |s| format!("{s:.0} ms")),
                            );
                            widgets::key_value(
                                ui,
                                "Lockups / Wheelspin (10m)",
                                &format!("{:.1} / {:.1}", m.lockups_per_10min, m.wheelspins_per_10min),
                            );
                            widgets::key_value(
                                ui,
                                "Under / Oversteer",
                                &format!("{:.0}% / {:.0}%", m.understeer_pct, m.oversteer_pct),
                            );
                        });
                    }
                });
            }
        });
    }
}

/// Rows grouped by section, enriched with car-specific `setup.ini` labels and sliders when available.
fn draw_value_groups(ui: &mut egui::Ui, rows: &mut [ValueRow], schema: Option<&SetupSchema>) {
    for (index, group) in rows.chunk_by_mut(|a, b| a.section == b.section).enumerate() {
        let sec = &group[0].section;
        let spec = schema.and_then(|s| s.find(sec));
        ui.add_space(4.0);
        let heading = match spec {
            Some(sp) => format!("{}  [{} · {}]", sp.name, sp.tab, sec),
            None => format!("[{sec}]"),
        };
        ui.label(RichText::new(heading).strong().color(TEXT));
        egui::Grid::new(("setups_values", index))
            .num_columns(3)
            .spacing([16.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                for row in group.iter_mut() {
                    ui.label(RichText::new(row.key.as_str()).font(theme::mono(12.0)));
                    if row.key.eq_ignore_ascii_case("VALUE")
                        && let Some(sp) = spec
                        && sp.max > sp.min
                        && let Ok(mut num) = row.text.trim().parse::<f64>()
                    {
                        let step = if sp.step > 0.0 { sp.step } else { 1.0 };
                        let resp = ui.add(
                            egui::Slider::new(&mut num, sp.min..=sp.max)
                                .step_by(step)
                                .show_value(true),
                        );
                        if resp.changed() {
                            row.text = if step.fract() == 0.0 {
                                format!("{num:.0}")
                            } else {
                                format!("{num:.2}")
                            };
                        }
                    } else {
                        ui.add(egui::TextEdit::singleline(&mut row.text).desired_width(240.0));
                    }
                    if row.text != row.original {
                        ui.label(RichText::new("edited").small().color(WARN));
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
    }
}

fn setup_row(ui: &mut egui::Ui, setup: &SetupFile) {
    ui.horizontal(|ui| {
        let label = match &setup.track {
            Some(track) => format!("{track} / {}.ini", setup.name),
            None => format!("{}.ini", setup.name),
        };
        ui.label(RichText::new(label).strong().color(TEXT));
        ui.label(RichText::new(modified_text(setup.modified)).small().color(MUTED));
    });
}

fn setup_combo(ui: &mut egui::Ui, id: &str, label: &str, setups: &[SetupFile], slot: &mut Option<PathBuf>) {
    let current = slot
        .as_ref()
        .and_then(|p| setups.iter().find(|s| &s.path == p))
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "Pick...".into());
    ui.label(RichText::new(label).color(MUTED));
    egui::ComboBox::from_id_salt(id)
        .selected_text(current)
        .width(170.0)
        .show_ui(ui, |ui| {
            for setup in setups {
                ui.selectable_value(&mut *slot, Some(setup.path.clone()), setup.name.as_str());
            }
        });
}

fn show_comparison(ui: &mut egui::Ui, comparison: &Comparison) {
    ui.add_space(8.0);
    ui.label(RichText::new(format!("{}: {} differing keys", comparison.title, comparison.diffs.len())).color(MUTED));
    if comparison.diffs.is_empty() {
        ui.label(RichText::new("No differences.").color(OK));
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("setups_diff")
        .max_height(320.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("setups_diff_grid")
                .striped(true)
                .num_columns(4)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for header in ["Section", "Key", "Left", "Right"] {
                        ui.label(RichText::new(header).strong().color(MUTED));
                    }
                    ui.end_row();
                    for diff in &comparison.diffs {
                        ui.label(diff.section.as_str());
                        ui.label(diff.key.as_str());
                        value_label(ui, diff.left.as_deref());
                        value_label(ui, diff.right.as_deref());
                        ui.end_row();
                    }
                });
        });
}

fn value_label(ui: &mut egui::Ui, value: Option<&str>) {
    match value {
        Some(v) => ui.label(RichText::new(v).color(TEXT)),
        None => ui.label(RichText::new("missing").italics().color(MUTED)),
    };
}

fn opt(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("missing")
}

fn modified_text(modified: Option<SystemTime>) -> String {
    match modified {
        Some(time) => chrono::DateTime::<chrono::Local>::from(time).format("%Y-%m-%d %H:%M").to_string(),
        None => "unknown date".into(),
    }
}

/// `<Documents>/Assetto Corsa/setups`, from USERPROFILE or HOME. Empty when neither is set.
fn default_root() -> String {
    acxs_manager::settings::ac_documents_dir()
        .map(|dir| dir.join("setups").display().to_string())
        .unwrap_or_default()
}

/// Car folders under the setups root, sorted by name. Hidden folders and the archive are skipped.
fn scan_cars(root: &Path) -> io::Result<Vec<String>> {
    let mut cars: Vec<String> = fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.') && name != ARCHIVE_DIR)
        .collect();
    cars.sort_by_key(|c| c.to_lowercase());
    Ok(cars)
}

/// Setups for one car, newest first. Setups without a modified time go last.
fn sorted_setups(root: &Path, car: &str) -> Vec<SetupFile> {
    let mut list = setups::list_setups(root, Some(car));
    list.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    list
}

fn load_all(paths: &[&PathBuf]) -> io::Result<Vec<Ini>> {
    paths.iter().map(|p| Ini::load(p)).collect()
}

/// Forget a selection whose file is no longer in the list.
fn prune(slot: &mut Option<PathBuf>, setups: &[SetupFile]) {
    if slot.as_ref().is_some_and(|p| !setups.iter().any(|s| &s.path == p)) {
        *slot = None;
    }
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}
