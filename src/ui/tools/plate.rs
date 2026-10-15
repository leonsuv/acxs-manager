//! Plate Designer tool page: pick a built-in plate style, type the plate text
//! and preview the rendered plate. Rendering happens only when the style, text
//! or scale changes. Writing the PNG runs on a background thread, and the
//! file is created with `create_new` so an existing file is never overwritten.

use super::{Tool, ToolContext};
use crate::ui::{theme, Notice};
use acxs_manager::license_plate::{self, CharRule, PlateStyle};
use acxs_manager::png;
use eframe::egui::{self, ColorImage, RichText, TextureHandle, TextureOptions};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(PlateDesigner {
        styles: license_plate::builtin_styles(),
        style_index: 0,
        text: String::new(),
        scale: 2,
        inputs: None,
        preview: None,
        error: None,
        saving: false,
        tx,
        rx,
    })
}

/// The values a preview is built from. A new preview is made only when these change.
#[derive(Clone, PartialEq, Eq)]
struct Inputs {
    style: usize,
    text: String,
    scale: u32,
}

struct Preview {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    texture: TextureHandle,
}

struct PlateDesigner {
    styles: Vec<PlateStyle>,
    style_index: usize,
    text: String,
    scale: u32,
    /// Inputs the current preview or error was produced from.
    inputs: Option<Inputs>,
    preview: Option<Preview>,
    error: Option<String>,
    saving: bool,
    tx: Sender<Notice>,
    rx: Receiver<Notice>,
}

impl PlateDesigner {
    /// Re-renders the preview if any input changed since the last call.
    fn refresh(&mut self, egui_ctx: &egui::Context) {
        let inputs = Inputs { style: self.style_index, text: self.text.clone(), scale: self.scale };
        if self.inputs.as_ref() == Some(&inputs) {
            return;
        }
        self.inputs = Some(inputs);
        self.preview = None;
        self.error = None;

        let style = &self.styles[self.style_index];
        match license_plate::render_rgba(style, &self.text, self.scale) {
            Ok((width, height, pixels)) => {
                let image = ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &pixels,
                );
                let texture = egui_ctx.load_texture("plate-preview", image, TextureOptions::NEAREST);
                self.preview = Some(Preview { width, height, pixels, texture });
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(notice) = self.rx.try_recv() {
            self.saving = false;
            notices.push(notice);
        }
    }

    /// Ask for a file, then encode and write the preview on a background thread.
    fn save(&mut self, egui_ctx: &egui::Context) {
        let Some(preview) = &self.preview else { return };
        let style_name = self.styles[self.style_index].name.clone();
        let Some(mut path) = rfd::FileDialog::new()
            .set_file_name(format!("plate-{style_name}.png"))
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("png");
        }

        self.saving = true;
        let (width, height) = (preview.width, preview.height);
        let pixels = preview.pixels.clone();
        let tx = self.tx.clone();
        let ctx = egui_ctx.clone();
        std::thread::spawn(move || {
            let notice = match write_png(&path, width, height, &pixels) {
                Ok(()) => Notice { text: format!("Saved plate to {}", path.display()), ok: true },
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Notice {
                    text: format!("{} already exists. Choose another file name.", path.display()),
                    ok: false,
                },
                Err(e) => Notice { text: format!("Save failed: {e}"), ok: false },
            };
            let _ = tx.send(notice);
            ctx.request_repaint();
        });
    }
}

/// Encodes the RGBA buffer as PNG and creates the file, failing if it exists.
fn write_png(path: &Path, width: u32, height: u32, pixels: &[u8]) -> io::Result<()> {
    let bytes = png::encode_rgba(width, height, pixels).map_err(|e| io::Error::other(e.to_string()))?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)
}

/// Shows the shape of valid text for a style, e.g. "AA 9999 AA".
fn format_hint(style: &PlateStyle) -> String {
    style
        .fields
        .iter()
        .map(|field| {
            let symbol = match &field.rule {
                CharRule::Letters => 'A',
                CharRule::Digits => '9',
                CharRule::Alphanumeric => 'X',
                CharRule::Custom(_) => '*',
            };
            symbol.to_string().repeat(field.max_len)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Tool for PlateDesigner {
    fn name(&self) -> &'static str {
        "Plate Designer"
    }

    fn group(&self) -> &'static str {
        "Content"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        self.refresh(ui.ctx());

        theme::card().show(ui, |ui| {
            ui.label(RichText::new("Style").color(theme::MUTED));
            let current = self.styles[self.style_index].name.clone();
            egui::ComboBox::from_id_salt("plate-style")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (index, style) in self.styles.iter().enumerate() {
                        ui.selectable_value(&mut self.style_index, index, &style.name);
                    }
                });

            ui.add_space(8.0);
            ui.label(RichText::new("Plate text").color(theme::MUTED));
            let hint = format_hint(&self.styles[self.style_index]);
            ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .hint_text(hint.clone())
                    .desired_width(f32::INFINITY),
            );
            ui.label(RichText::new(format!("Format: {hint}")).small().color(theme::MUTED));

            if self.text.trim().is_empty() {
                ui.label(RichText::new("Enter the plate text.").small().color(theme::MUTED));
            } else if let Some(error) = &self.error {
                ui.label(RichText::new(error).small().color(theme::WARN));
            }

            ui.add_space(8.0);
            ui.add(egui::Slider::new(&mut self.scale, 1..=8).text("Scale"));
        });

        ui.add_space(12.0);

        let save_clicked = theme::card()
            .show(ui, |ui| {
                match &self.preview {
                    Some(preview) => {
                        let size = preview.texture.size_vec2();
                        let width = size.x.min(ui.available_width());
                        let height = width * size.y / size.x;
                        ui.add(
                            egui::Image::new(&preview.texture)
                                .fit_to_exact_size(egui::vec2(width, height)),
                        );
                    }
                    None => {
                        ui.label(RichText::new("The preview appears here once the text is valid.").color(theme::MUTED));
                    }
                }

                ui.add_space(8.0);
                let label = if self.saving { "Saving..." } else { "Save PNG" };
                let can_save = self.preview.is_some() && !self.saving;
                ui.add_enabled(can_save, egui::Button::new(label)).clicked()
            })
            .inner;

        if save_clicked {
            self.save(ui.ctx());
        }
    }
}
