//! Model Tools tool page: inspects a `.kn5` model, exports its mesh geometry to
//! OBJ and FBX, imports OBJ meshes as a new KN5 file, bakes per-corner ambient
//! occlusion, and lists the entries of a `data.acd` archive. Each operation runs
//! on a background thread and reports back over a channel that is polled once
//! per frame.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::acd::{self, AcdEntry};
use acxs_manager::ao_bake::{self, AoSettings};
use acxs_manager::fbx::{self, FbxMesh};
use acxs_manager::kn5::{self, Kn5Info};
use acxs_manager::kn5_export::{self, ObjStats};
use acxs_manager::kn5_writer;
use acxs_manager::obj_import;
use acxs_manager::ortho::{self, View};
use eframe::egui::{self, RichText};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Longest list shown for textures, nodes and archive entries.
const LIST_CAP: usize = 500;
/// Height of each scrollable list box.
const LIST_HEIGHT: f32 = 240.0;
/// Size of the preview PNG written by "Render preview".
const PREVIEW_WIDTH: u32 = 800;
const PREVIEW_HEIGHT: u32 = 600;
/// Settings for "Bake AO".
const AO_RAYS: u32 = 64;
const AO_MAX_DISTANCE: f32 = 10.0;
const AO_BIAS: f32 = 0.01;

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
    /// A preview PNG was written; the result is its size in bytes.
    Rendered {
        path: PathBuf,
        result: Result<usize, String>,
    },
    /// An FBX file was written; the result is the triangle count.
    FbxExported {
        path: PathBuf,
        result: Result<usize, String>,
    },
    /// A KN5 file was written from an OBJ; the result is the mesh count.
    ObjImported {
        obj: PathBuf,
        kn5: PathBuf,
        result: Result<usize, String>,
    },
    /// AO was baked in memory; the result is the value count and the average.
    AoBaked {
        kn5: PathBuf,
        result: Result<(usize, f64), String>,
    },
}

struct ModelTools {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The KN5 file chosen for inspection and export.
    kn5_path: Option<PathBuf>,
    inspect_busy: bool,
    export_busy: bool,
    render_busy: bool,
    fbx_busy: bool,
    obj_busy: bool,
    ao_busy: bool,
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
            render_busy: false,
            fbx_busy: false,
            obj_busy: false,
            ao_busy: false,
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
                Msg::Rendered { path, result } => {
                    self.render_busy = false;
                    match result {
                        Ok(bytes) => notices.push(notice(
                            format!(
                                "Saved preview {} ({PREVIEW_WIDTH}x{PREVIEW_HEIGHT}, {bytes} bytes)",
                                path.display()
                            ),
                            true,
                        )),
                        Err(e) => notices.push(notice(format!("Render preview failed: {e}"), false)),
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
                Msg::FbxExported { path, result } => {
                    self.fbx_busy = false;
                    match result {
                        Ok(triangles) => notices.push(notice(
                            format!("Exported FBX {} ({triangles} triangles)", file_name(&path)),
                            true,
                        )),
                        Err(e) => notices.push(notice(format!("FBX export failed: {e}"), false)),
                    }
                }
                Msg::ObjImported { obj, kn5, result } => {
                    self.obj_busy = false;
                    match result {
                        Ok(meshes) => notices.push(notice(
                            format!(
                                "Imported {} as {meshes} KN5 meshes into {}",
                                file_name(&obj),
                                file_name(&kn5)
                            ),
                            true,
                        )),
                        Err(e) => notices.push(notice(format!("OBJ import failed: {e}"), false)),
                    }
                }
                Msg::AoBaked { kn5, result } => {
                    self.ao_busy = false;
                    match result {
                        Ok((values, average)) => notices.push(notice(
                            format!(
                                "Baked AO for {}: {values} values, average {average:.3} (not written to disk)",
                                file_name(&kn5)
                            ),
                            true,
                        )),
                        Err(e) => notices.push(notice(format!("AO bake failed: {e}"), false)),
                    }
                }
            }
        }
    }

    fn convert_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "CONVERT AND BAKE");
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let has_file = self.kn5_path.is_some();

                let fbx = ui.add_enabled(
                    has_file && !self.fbx_busy,
                    egui::Button::new("Export FBX"),
                );
                if fbx.clicked() {
                    if let Some(kn5_file) = self.kn5_path.clone() {
                        let stem = default_stem(&kn5_file);
                        if let Some(fbx_file) = rfd::FileDialog::new()
                            .add_filter("fbx", &["fbx"])
                            .set_file_name(format!("{stem}.fbx"))
                            .save_file()
                        {
                            self.fbx_busy = true;
                            self.spawn(ui.ctx(), move || {
                                let result = export_fbx(&kn5_file, &fbx_file);
                                Msg::FbxExported {
                                    path: fbx_file,
                                    result,
                                }
                            });
                        }
                    }
                }

                let import = ui.add_enabled(!self.obj_busy, egui::Button::new("Import OBJ"));
                if import.clicked() {
                    if let Some(obj_file) = rfd::FileDialog::new()
                        .add_filter("obj", &["obj"])
                        .pick_file()
                    {
                        let stem = default_stem(&obj_file);
                        if let Some(kn5_file) = rfd::FileDialog::new()
                            .add_filter("kn5", &["kn5"])
                            .set_file_name(format!("{stem}.kn5"))
                            .save_file()
                        {
                            self.obj_busy = true;
                            self.spawn(ui.ctx(), move || {
                                let result = import_obj(&obj_file, &kn5_file);
                                Msg::ObjImported {
                                    obj: obj_file,
                                    kn5: kn5_file,
                                    result,
                                }
                            });
                        }
                    }
                }

                let ao = ui.add_enabled(has_file && !self.ao_busy, egui::Button::new("Bake AO"));
                if ao.clicked() {
                    if let Some(kn5_file) = self.kn5_path.clone() {
                        self.ao_busy = true;
                        self.spawn(ui.ctx(), move || {
                            let result = bake_ao(&kn5_file);
                            Msg::AoBaked {
                                kn5: kn5_file,
                                result,
                            }
                        });
                    }
                }

                if self.fbx_busy || self.obj_busy || self.ao_busy {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new("Working...").color(theme::MUTED));
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Export FBX and Bake AO use the KN5 file chosen above. Export FBX writes one \
                     mesh with no normals. Bake AO only reports the values; they are not written \
                     to disk.",
                )
                .small()
                .color(theme::MUTED),
            );
        });
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

                let render = ui.add_enabled(
                    has_file && !self.render_busy,
                    egui::Button::new("Render preview"),
                );
                if render.clicked() {
                    if let Some(kn5_file) = self.kn5_path.clone() {
                        let stem = default_stem(&kn5_file);
                        if let Some(png) = rfd::FileDialog::new()
                            .add_filter("png", &["png"])
                            .set_file_name(format!("{stem}.png"))
                            .save_file()
                        {
                            self.render_busy = true;
                            self.spawn(ui.ctx(), move || {
                                let result = render_preview(&kn5_file, &png);
                                Msg::Rendered { path: png, result }
                            });
                        }
                    }
                }

                if self.inspect_busy || self.export_busy || self.render_busy {
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
            "Inspect KN5 models, export their geometry to OBJ and FBX, import OBJ as KN5, \
             bake ambient occlusion, and list data.acd archives.",
        );

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.kn5_section(ui);
                ui.add_space(20.0);
                self.convert_section(ui);
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

/// Loads the model, renders it from the fixed preview view and writes the PNG
/// as a new file. An existing file is never overwritten. Returns the byte count.
fn render_preview(kn5_file: &Path, png: &Path) -> Result<usize, String> {
    let triangles = kn5_export::load_triangles(kn5_file).map_err(|e| e.to_string())?;
    let view = View {
        yaw_deg: 45.0,
        pitch_deg: 20.0,
        width: PREVIEW_WIDTH,
        height: PREVIEW_HEIGHT,
        padding: 0.08,
    };
    let bytes = ortho::render_png(&triangles, &view).map_err(|e| e.to_string())?;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(png)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => format!("{} already exists, choose another name", file_name(png)),
            _ => format!("could not create {}: {e}", file_name(png)),
        })?;
    if let Err(e) = file.write_all(&bytes) {
        let _ = fs::remove_file(png);
        return Err(format!("could not write {}: {e}", file_name(png)));
    }
    Ok(bytes.len())
}

/// Loads the KN5 model and writes its triangles as one FBX mesh to a new file.
/// Returns the triangle count.
fn export_fbx(kn5_file: &Path, fbx_file: &Path) -> Result<usize, String> {
    let triangles = kn5_export::load_triangles(kn5_file).map_err(|e| e.to_string())?;
    if triangles.is_empty() {
        return Err(format!("{} has no triangles", file_name(kn5_file)));
    }
    let mut vertices = Vec::with_capacity(triangles.len() * 3);
    for tri in &triangles {
        vertices.extend_from_slice(&[tri.a, tri.b, tri.c]);
    }
    let count = vertices.len();
    let indices = (0..count)
        .map(|i| u32::try_from(i).map_err(|_| format!("{count} vertices is too many for FBX")))
        .collect::<Result<Vec<u32>, String>>()?;
    let mesh = FbxMesh {
        name: fbx_name(&default_stem(kn5_file)),
        vertices,
        indices,
        normals: None,
    };
    fbx::write_fbx(fbx_file, &[mesh]).map_err(|e| e.to_string())?;
    Ok(triangles.len())
}

/// Reads the OBJ file (no texture mapping) and writes its meshes as a new KN5 file.
/// Returns the mesh count.
fn import_obj(obj_file: &Path, kn5_file: &Path) -> Result<usize, String> {
    let meshes =
        obj_import::read_obj(obj_file, &HashMap::new()).map_err(|e| e.to_string())?;
    let count = meshes.len();
    kn5_writer::write_kn5(kn5_file, &meshes).map_err(|e| e.to_string())?;
    Ok(count)
}

/// Loads the KN5 model and bakes one AO value per triangle corner. The values are
/// not stored. Returns the value count and their average.
fn bake_ao(kn5_file: &Path) -> Result<(usize, f64), String> {
    let triangles = kn5_export::load_triangles(kn5_file).map_err(|e| e.to_string())?;
    if triangles.is_empty() {
        return Err(format!("{} has no triangles", file_name(kn5_file)));
    }
    let settings = AoSettings {
        rays: AO_RAYS,
        max_distance: AO_MAX_DISTANCE,
        bias: AO_BIAS,
    };
    let values = ao_bake::bake_vertex_ao(&triangles, &settings).map_err(|e| e.to_string())?;
    let count = values.len();
    let average = values.iter().map(|&v| f64::from(v)).sum::<f64>() / count as f64;
    Ok((count, average))
}

/// Turns a file stem into an FBX mesh name: quotes and control characters become
/// underscores, and an empty name becomes "model".
fn fbx_name(stem: &str) -> String {
    let name: String = stem
        .chars()
        .map(|c| if c == '"' || c.is_control() { '_' } else { c })
        .collect();
    if name.trim().is_empty() {
        "model".to_string()
    } else {
        name
    }
}

/// File stem of `path`, or "model" when it has none. Used as the default save name.
fn default_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_string())
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
