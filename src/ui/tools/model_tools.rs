//! Model Tools tool page: inspects an Assetto Corsa `.kn5` model, exports its
//! mesh geometry to OBJ, and lists the entries of a `data.acd` archive. Each
//! operation runs on a background thread and reports back over a channel that
//! is polled once per frame.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::acd::{self, AcdEntry};
use acxs_manager::kn5::{self, Kn5Info};
use acxs_manager::kn5_export::{self, ObjStats};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Longest list shown for textures, nodes and archive entries.
const LIST_CAP: usize = 500;
/// Height of each scrollable list box.
const LIST_HEIGHT: f32 = 240.0;

pub fn tool() -> Box<dyn Tool> {
    Box::new(ModelTools::new())
}

/// Results sent back from background threads.
enum Msg {
    Inspected {
        path: PathBuf,
        result: Result<Kn5Info, String>,
    },
    Exported {
        obj: PathBuf,
        result: Result<ObjStats, String>,
    },
    Listed {
        path: PathBuf,
        result: Result<Vec<AcdEntry>, String>,
    },
}

struct ModelTools {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The KN5 file chosen for inspection and export.
    kn5_path: Option<PathBuf>,
    inspect_busy: bool,
    export_busy: bool,
    /// Last successful inspection, with the file it came from.
    inspected: Option<(PathBuf, Kn5Info)>,
    /// Last successful export: output path and counts.
    exported: Option<(PathBuf, ObjStats)>,
    acd_busy: bool,
    /// Last successful listing of a `data.acd` file.
    acd: Option<(PathBuf, Vec<AcdEntry>)>,
}

impl ModelTools {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        ModelTools {
            tx,
            rx,
            kn5_path: None,
            inspect_busy: false,
            export_busy: false,
            inspected: None,
            exported: None,
            acd_busy: false,
            acd: None,
        }
    }

    /// Runs `job` on a background thread, sends its result back, and wakes the UI.
    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }

    /// Applies finished background results: updates state and pushes notices.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Inspected { path, result } => {
                    self.inspect_busy = false;
                    match result {
                        Ok(info) => {
                            notices.push(notice(format!("Inspected {}", file_name(&path)), true));
                            self.inspected = Some((path, info));
                        }
                        Err(e) => {
                            self.inspected = None;
                            notices.push(notice(
                                format!("Could not inspect {}: {e}", file_name(&path)),
                                false,
                            ));
                        }
                    }
                }
                Msg::Exported { obj, result } => {
                    self.export_busy = false;
                    match result {
                        Ok(stats) => {
                            notices.push(notice(
                                format!(
                                    "Exported {}: {} meshes, {} vertices, {} triangles",
                                    file_name(&obj),
                                    stats.meshes,
                                    stats.vertices,
                                    stats.triangles
                                ),
                                true,
                            ));
                            self.exported = Some((obj, stats));
                        }
                        Err(e) => {
                            self.exported = None;
                            notices.push(notice(format!("OBJ export failed: {e}"), false));
                        }
                    }
                }
                Msg::Listed { path, result } => {
                    self.acd_busy = false;
                    match result {
                        Ok(entries) => {
                            notices.push(notice(
                                format!("Listed {} entries in {}", entries.len(), file_name(&path)),
                                true,
                            ));
                            self.acd = Some((path, entries));
                        }
                        Err(e) => {
                            self.acd = None;
                            notices.push(notice(
                                format!("Could not read {}: {e}", file_name(&path)),
                                false,
                            ));
                        }
                    }
                }
            }
        }
    }

    fn kn5_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "KN5 MODEL");
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Choose .kn5").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("kn5", &["kn5"])
                        .pick_file()
                    {
                        self.kn5_path = Some(path);
                    }
                }
                let label = self
                    .kn5_path
                    .as_deref()
                    .map(file_name)
                    .unwrap_or_else(|| "No file chosen".to_string());
                ui.label(RichText::new(label).color(theme::MUTED));
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let has_file = self.kn5_path.is_some();

                let inspect = ui.add_enabled(
                    has_file && !self.inspect_busy,
                    widgets::primary_button("Inspect"),
                );
                if inspect.clicked() {
                    if let Some(path) = self.kn5_path.clone() {
                        self.inspect_busy = true;
                        self.spawn(ui.ctx(), move || {
                            let result = kn5::inspect(&path).map_err(|e| e.to_string());
                            Msg::Inspected { path, result }
                        });
                    }
                }

                let export = ui.add_enabled(
                    has_file && !self.export_busy,
                    egui::Button::new("Export OBJ"),
                );
                if export.clicked() {
                    if let Some(kn5_file) = self.kn5_path.clone() {
                        let stem = kn5_file
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "model".to_string());
                        if let Some(obj) = rfd::FileDialog::new()
                            .add_filter("obj", &["obj"])
                            .set_file_name(format!("{stem}.obj"))
                            .save_file()
                        {
                            self.export_busy = true;
                            self.spawn(ui.ctx(), move || {
                                // export_obj refuses to overwrite an existing file.
                                let result = kn5_export::export_obj(&kn5_file, &obj, None)
                                    .map_err(|e| e.to_string());
                                Msg::Exported { obj, result }
                            });
                        }
                    }
                }

                if self.inspect_busy || self.export_busy {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new("Working...").color(theme::MUTED));
                }
            });
        });

        if let Some((path, info)) = &self.inspected {
            inspect_view(ui, path, info);
        }
        if let Some((obj, stats)) = &self.exported {
            export_view(ui, obj, stats);
        }
    }

    fn acd_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "DATA.ACD ARCHIVE");
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                let pick = ui.add_enabled(
                    !self.acd_busy,
                    egui::Button::new("Choose .acd file"),
                );
                if pick.clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("acd", &["acd"])
                        .pick_file()
                    {
                        self.acd_busy = true;
                        self.spawn(ui.ctx(), move || {
                            let result = acd::read_file(&path).map_err(|e| e.to_string());
                            Msg::Listed { path, result }
                        });
                    }
                }
                if self.acd_busy {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new("Listing...").color(theme::MUTED));
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Shows entry names and sizes only. Entry contents are not decoded here: \
                     the key comes from the car or track folder name, which this page does not \
                     have, so read_entry returns Unsupported.",
                )
                .small()
                .color(theme::MUTED),
            );
        });

        if let Some((path, entries)) = &self.acd {
            acd_view(ui, path, entries);
        }
    }
}

impl Tool for ModelTools {
    fn name(&self) -> &'static str {
        "Model Tools"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);

        widgets::heading(
            ui,
            "Model Tools",
            "Inspect KN5 models, export their geometry to OBJ, and list data.acd archives.",
        );

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.kn5_section(ui);
                ui.add_space(20.0);
                self.acd_section(ui);
            });
    }
}

fn inspect_view(ui: &mut egui::Ui, path: &Path, info: &Kn5Info) {
    ui.add_space(12.0);
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(file_name(path)).strong());
        ui.add_space(6.0);
        widgets::key_value(ui, "KN5 version", &info.version.to_string());
        widgets::key_value(ui, "File size", &fmt_bytes(info.file_size));
        widgets::key_value(ui, "Textures", &info.texture_names.len().to_string());
        widgets::key_value(ui, "Nodes", &info.node_count.to_string());
    });

    if !info.notes.is_empty() {
        ui.add_space(8.0);
        theme::card().show(ui, |ui| {
            ui.label(RichText::new("Notes").strong().color(theme::WARN));
            for note in &info.notes {
                ui.label(RichText::new(note.as_str()).color(theme::WARN));
            }
        });
    }

    ui.add_space(12.0);
    name_list(ui, "Textures", &info.texture_names);
    ui.add_space(12.0);
    name_list(ui, "Nodes", &info.node_names);
}

fn export_view(ui: &mut egui::Ui, obj: &Path, stats: &ObjStats) {
    ui.add_space(12.0);
    theme::card().show(ui, |ui| {
        ui.label(RichText::new("Last OBJ export").strong().color(theme::OK));
        ui.add_space(6.0);
        widgets::key_value(ui, "File", &obj.display().to_string());
        widgets::key_value(ui, "Meshes", &stats.meshes.to_string());
        widgets::key_value(ui, "Vertices", &stats.vertices.to_string());
        widgets::key_value(ui, "Triangles", &stats.triangles.to_string());
    });
}

fn acd_view(ui: &mut egui::Ui, path: &Path, entries: &[AcdEntry]) {
    ui.add_space(12.0);
    widgets::section_title(
        ui,
        &format!("{} ({} entries)", file_name(path), entries.len()),
    );
    if entries.len() > LIST_CAP {
        ui.label(
            RichText::new(format!(
                "Showing the first {LIST_CAP} of {} entries; the rest are not listed.",
                entries.len()
            ))
            .small()
            .color(theme::MUTED),
        );
    }
    if entries.is_empty() {
        ui.label(RichText::new("No entries.").color(theme::MUTED));
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("acd_entries")
        .max_height(LIST_HEIGHT)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("acd_entries_grid")
                .striped(true)
                .show(ui, |ui| {
                    for entry in entries.iter().take(LIST_CAP) {
                        ui.label(RichText::new(entry.name.as_str()).monospace());
                        ui.label(RichText::new(fmt_bytes(entry.size)).color(theme::MUTED));
                        ui.end_row();
                    }
                });
        });
}

/// A capped, scrollable list of names with a heading and count.
fn name_list(ui: &mut egui::Ui, title: &str, names: &[String]) {
    widgets::section_title(ui, &format!("{title} ({})", names.len()));
    if names.len() > LIST_CAP {
        ui.label(
            RichText::new(format!(
                "Showing the first {LIST_CAP} of {} {}; the rest are not listed.",
                names.len(),
                title.to_lowercase()
            ))
            .small()
            .color(theme::MUTED),
        );
    }
    if names.is_empty() {
        ui.label(RichText::new("None").color(theme::MUTED));
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt(title)
        .max_height(LIST_HEIGHT)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for name in names.iter().take(LIST_CAP) {
                ui.label(RichText::new(name.as_str()).monospace());
            }
        });
}

fn notice(text: String, ok: bool) -> Notice {
    Notice { text, ok }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
