//! HUD Layout tool page: place and style the HUD widgets, preview the result
//! with sample telemetry, and load or save the layout file.
//!
//! With a valid Assetto Corsa folder the layout file defaults to
//! `acxs-hud.json` in that folder. Without one only the file pickers are
//! offered. File dialogs, reads and writes run on a background thread and
//! report back over a channel that is polled once per frame.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::overlay_app;
use acxs_manager::hud::{
    self, Anchor, Color, DrawItem, Layout, Rect as HudRect, SpeedUnit, Telemetry, Widget, WidgetKind,
};
use eframe::egui::{
    self, vec2, Align2, Color32, CornerRadius, FontId, Pos2, RichText, Sense, Stroke, StrokeKind,
    TextEdit,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const DEFAULT_FILE: &str = "acxs-hud.json";
const CANVAS_MAX_WIDTH: f32 = 640.0;
const SAMPLE_MAX_RPM: f32 = 8_500.0;
const SAMPLE_LAST_LAP_MS: u32 = 83_456;

const KINDS: [WidgetKind; 10] = [
    WidgetKind::Speed,
    WidgetKind::Gear,
    WidgetKind::RpmBar,
    WidgetKind::ShiftLights,
    WidgetKind::LapTime,
    WidgetKind::Delta,
    WidgetKind::Fuel,
    WidgetKind::TyreTemps,
    WidgetKind::Position,
    WidgetKind::CustomText,
];

const ANCHORS: [Anchor; 9] = [
    Anchor::TopLeft,
    Anchor::TopCenter,
    Anchor::TopRight,
    Anchor::CenterLeft,
    Anchor::Center,
    Anchor::CenterRight,
    Anchor::BottomLeft,
    Anchor::BottomCenter,
    Anchor::BottomRight,
];

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(HudEditor {
        tx,
        rx,
        root: None,
        path: None,
        layout: Layout::default_layout(),
        selected: None,
        new_kind: WidgetKind::Speed,
        sample: Sample::default(),
        busy: false,
    })
}

/// Sample telemetry controlled by the sliders on the page.
struct Sample {
    speed_kmh: f32,
    gear: i32,
    rpm: f32,
    lap_ms: u32,
}

impl Default for Sample {
    fn default() -> Self {
        Sample {
            speed_kmh: 180.0,
            gear: 4,
            rpm: 6_200.0,
            lap_ms: 83_456,
        }
    }
}

/// A file operation. `None` paths open a dialog first.
enum Job {
    Load {
        path: Option<PathBuf>,
        dir: Option<PathBuf>,
    },
    Save {
        path: Option<PathBuf>,
        dir: Option<PathBuf>,
    },
}

/// Results sent back by the background thread.
enum Msg {
    Loaded {
        path: PathBuf,
        result: Result<Layout, String>,
    },
    Saved {
        path: PathBuf,
        result: Result<Option<PathBuf>, String>,
    },
    Cancelled,
}

struct HudEditor {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// The valid AC folder the page was last shown for.
    root: Option<PathBuf>,
    path: Option<PathBuf>,
    layout: Layout,
    selected: Option<usize>,
    new_kind: WidgetKind,
    sample: Sample,
    busy: bool,
}

impl Tool for HudEditor {
    fn name(&self) -> &'static str {
        "HUD Layout"
    }

    fn group(&self) -> &'static str {
        "Graphics"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        let root = ctx.ac_root.clone().filter(|_| ctx.ac_valid);
        if self.root != root {
            self.path = root.as_ref().map(|r| r.join(DEFAULT_FILE));
            self.root = root;
            self.selected = None;
        }

        widgets::heading(
            ui,
            "HUD Layout",
            "Place and style the HUD widgets, preview them with sample telemetry, and save the layout file.",
        );
        if let Some(root) = self.root.clone() {
            ui.horizontal(|ui| {
                let label = if overlay_app::is_installed(&root) { "Update in-game overlay" } else { "Install in-game overlay" };
                let button = ui.add_enabled(!ctx.game_running, widgets::primary_button(label)).on_hover_text(
                    "Puts this layout in the game as a CSP app, with a delta bar and a session panel with a speed trap.",
                );
                if button.clicked() {
                    match overlay_app::install(&root, &self.layout, ctx.game_running) {
                        Ok(_) => notices.push(Notice {
                            text: "Overlay installed. Open ACXS Dash, ACXS Delta or ACXS Session from the app bar in the game.".into(),
                            ok: true,
                        }),
                        Err(err) => notices.push(Notice { text: format!("Could not install the overlay: {err}"), ok: false }),
                    }
                }
                if ctx.game_running {
                    ui.label(RichText::new("Close the game to install it.").color(theme::MUTED));
                }
            });
            ui.add_space(8.0);
        }
        let egui_ctx = ui.ctx().clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.body(ui, &egui_ctx, notices));
    }
}

impl HudEditor {
    /// Apply results from the background thread.
    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded { path, result } => {
                    self.busy = false;
                    match result {
                        Ok(layout) => {
                            notices.push(Notice {
                                text: format!("Loaded HUD layout from {}", path.display()),
                                ok: true,
                            });
                            self.layout = layout;
                            self.selected = None;
                            self.path = Some(path);
                        }
                        Err(err) => notices.push(Notice {
                            text: format!("Could not load HUD layout: {err}"),
                            ok: false,
                        }),
                    }
                }
                Msg::Saved { path, result } => {
                    self.busy = false;
                    match result {
                        Ok(backup) => {
                            let mut text = format!("Saved HUD layout to {}", path.display());
                            if let Some(backup) = backup {
                                text.push_str(&format!(". Previous version kept as {}", backup.display()));
                            }
                            notices.push(Notice { text, ok: true });
                            self.path = Some(path);
                        }
                        Err(err) => notices.push(Notice {
                            text: format!("Could not save HUD layout: {err}"),
                            ok: false,
                        }),
                    }
                }
                Msg::Cancelled => self.busy = false,
            }
        }
    }

    fn spawn(&mut self, ctx: &egui::Context, job: Job) {
        self.busy = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let layout = self.layout.clone();
        thread::spawn(move || {
            let msg = match job {
                Job::Load { path, dir } => match path.or_else(|| pick_open(dir)) {
                    Some(path) => {
                        let result = Layout::load(&path).map_err(|e| e.to_string());
                        Msg::Loaded { path, result }
                    }
                    None => Msg::Cancelled,
                },
                Job::Save { path, dir } => match path.or_else(|| pick_save(dir)) {
                    Some(path) => {
                        let result = layout.save(&path).map_err(|e| e.to_string());
                        Msg::Saved { path, result }
                    }
                    None => Msg::Cancelled,
                },
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    fn body(&mut self, ui: &mut egui::Ui, egui_ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let errors = self.layout.validate().err().unwrap_or_default();

        let mut job: Option<Job> = None;
        theme::card().show(ui, |ui| self.file_bar(ui, &mut job));
        if let Some(job) = job {
            let is_save = matches!(job, Job::Save { .. });
            if is_save && !errors.is_empty() {
                notices.push(Notice {
                    text: "HUD layout not saved: fix the problems listed below first.".to_string(),
                    ok: false,
                });
            } else {
                self.spawn(egui_ctx, job);
            }
        }

        ui.add_space(12.0);
        self.preview(ui);
        ui.add_space(12.0);
        self.widget_list(ui);
        ui.add_space(12.0);
        self.validation(ui, &errors);
        ui.add_space(12.0);
    }

    fn file_bar(&mut self, ui: &mut egui::Ui, job: &mut Option<Job>) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Layout file").color(theme::MUTED));
            match &self.path {
                Some(path) => ui.label(RichText::new(path.display().to_string()).color(theme::TEXT)),
                None => ui.label(RichText::new("No file selected").color(theme::MUTED)),
            };
        });
        ui.horizontal_wrapped(|ui| {
            let idle = !self.busy;
            if let Some(path) = self.path.clone() {
                if ui.add_enabled(idle, egui::Button::new("Load")).clicked() {
                    *job = Some(Job::Load { path: Some(path.clone()), dir: None });
                }
                if ui.add_enabled(idle, egui::Button::new("Save")).clicked() {
                    *job = Some(Job::Save { path: Some(path), dir: None });
                }
            }
            if ui.add_enabled(idle, egui::Button::new("Open file...")).clicked() {
                *job = Some(Job::Load { path: None, dir: self.root.clone() });
            }
            if ui.add_enabled(idle, egui::Button::new("Save as...")).clicked() {
                *job = Some(Job::Save { path: None, dir: self.root.clone() });
            }
            if self.busy {
                ui.spinner();
            }
        });
    }

    fn telemetry(&self) -> Telemetry {
        Telemetry {
            speed_kmh: self.sample.speed_kmh,
            gear: self.sample.gear,
            rpm: self.sample.rpm,
            max_rpm: SAMPLE_MAX_RPM,
            lap_time_ms: Some(self.sample.lap_ms),
            last_lap_ms: Some(SAMPLE_LAST_LAP_MS),
            delta_ms: Some(-420),
            fuel_litres: 42.0,
            fuel_capacity_litres: 60.0,
            tyre_temps_c: [88.0, 91.0, 84.0, 86.0],
            position: Some(3),
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        widgets::section_title(ui, "PREVIEW");
        let width = ui.available_width().min(CANVAS_MAX_WIDTH);
        let (rect, response) = ui.allocate_exact_size(vec2(width, width * 9.0 / 16.0), Sense::click());
        let painter = ui.painter();
        let radius = CornerRadius::same(theme::RADIUS);
        painter.rect_filled(rect, radius, theme::BG);
        painter.rect_stroke(rect, radius, Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);

        let to_screen = |r: HudRect| {
            egui::Rect::from_min_size(
                Pos2::new(rect.left() + r.x * rect.width(), rect.top() + r.y * rect.height()),
                vec2(r.w * rect.width(), r.h * rect.height()),
            )
        };

        for (i, w) in self.layout.widgets.iter().enumerate() {
            let r = to_screen(w.rect());
            let selected = self.selected == Some(i);
            let stroke = if selected {
                Stroke::new(2.0, theme::ACCENT)
            } else {
                Stroke::new(1.0, theme::BORDER)
            };
            painter.rect_filled(r, CornerRadius::same(3), theme::CARD_HOVER.gamma_multiply(0.6));
            painter.rect_stroke(r, CornerRadius::same(3), stroke, StrokeKind::Outside);
            painter.text(
                r.left_top() + vec2(4.0, 2.0),
                Align2::LEFT_TOP,
                kind_label(w.kind),
                FontId::proportional(11.0),
                theme::MUTED,
            );
        }

        for item in hud::draw_items(&self.layout, &self.telemetry()) {
            match item {
                DrawItem::Bar { rect: r, fill, color, background, .. } => {
                    let r = to_screen(r);
                    if let Some(bg) = background {
                        painter.rect_filled(r, CornerRadius::same(2), to_color32(bg));
                    }
                    let filled = egui::Rect::from_min_size(r.min, vec2(r.width() * fill, r.height()));
                    painter.rect_filled(filled, CornerRadius::same(2), to_color32(color));
                }
                DrawItem::Text { rect: r, text, color, .. } => {
                    let r = to_screen(r);
                    let size = (r.height() * 0.55).clamp(9.0, 40.0);
                    painter.text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        text,
                        FontId::proportional(size),
                        to_color32(color),
                    );
                }
            }
        }

        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selected = self
                .layout
                .widgets
                .iter()
                .enumerate()
                .rev()
                .find(|(_, w)| to_screen(w.rect()).contains(pos))
                .map(|(i, _)| i);
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Slider::new(&mut self.sample.speed_kmh, 0.0..=330.0).text("Speed km/h"));
            ui.add(egui::Slider::new(&mut self.sample.gear, -1..=8).text("Gear"));
            ui.add(egui::Slider::new(&mut self.sample.rpm, 0.0..=SAMPLE_MAX_RPM).text("RPM"));
            ui.add(egui::Slider::new(&mut self.sample.lap_ms, 0..=600_000).text("Lap time ms"));
        });
    }

    fn widget_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Speed unit").color(theme::MUTED));
            egui::ComboBox::from_id_salt("hud_speed_unit")
                .selected_text(match self.layout.speed_unit {
                    SpeedUnit::Kmh => "km/h",
                    SpeedUnit::Mph => "mph",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.layout.speed_unit, SpeedUnit::Kmh, "km/h");
                    ui.selectable_value(&mut self.layout.speed_unit, SpeedUnit::Mph, "mph");
                });
        });
        ui.add_space(8.0);

        widgets::section_title(ui, "WIDGETS");
        let mut clicked = None;
        for (i, w) in self.layout.widgets.iter().enumerate() {
            let selected = self.selected == Some(i);
            ui.push_id(i, |ui| {
                if widgets::list_row(ui, selected, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&w.id).strong());
                        ui.label(RichText::new(kind_label(w.kind)).color(theme::MUTED));
                    });
                }) {
                    clicked = Some(i);
                }
            });
        }
        if let Some(i) = clicked {
            self.selected = Some(i);
        }
        if self.layout.widgets.is_empty() {
            widgets::empty_state(ui, "No widgets. Add one below.");
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("hud_new_kind")
                .selected_text(kind_label(self.new_kind))
                .show_ui(ui, |ui| {
                    for kind in KINDS {
                        ui.selectable_value(&mut self.new_kind, kind, kind_label(kind));
                    }
                });
            if ui.add(widgets::primary_button("Add widget")).clicked() {
                self.add_widget();
            }
            if ui
                .add_enabled(self.selected.is_some(), egui::Button::new("Remove selected"))
                .clicked()
            {
                self.remove_selected();
            }
        });

        ui.add_space(12.0);
        match self.selected.filter(|&i| i < self.layout.widgets.len()) {
            Some(i) => self.widget_editor(ui, i),
            None => widgets::empty_state(ui, "Select a widget to edit its position, size and colours."),
        }
    }

    fn widget_editor(&mut self, ui: &mut egui::Ui, i: usize) {
        let w = &mut self.layout.widgets[i];
        theme::card().show(ui, |ui| {
            egui::Grid::new(("hud_fields", i))
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Id");
                    ui.add(TextEdit::singleline(&mut w.id).desired_width(180.0));
                    ui.end_row();

                    ui.label("Kind");
                    let previous = w.kind;
                    egui::ComboBox::from_id_salt(("hud_kind", i))
                        .selected_text(kind_label(w.kind))
                        .show_ui(ui, |ui| {
                            for kind in KINDS {
                                ui.selectable_value(&mut w.kind, kind, kind_label(kind));
                            }
                        });
                    if w.kind != previous {
                        w.source = w.kind.default_source().to_string();
                    }
                    ui.end_row();

                    ui.label("Value source");
                    let kind = w.kind;
                    egui::ComboBox::from_id_salt(("hud_source", i))
                        .selected_text(w.source.clone())
                        .show_ui(ui, |ui| {
                            for source in kind.sources() {
                                ui.selectable_value(&mut w.source, source.to_string(), *source);
                            }
                        });
                    ui.end_row();

                    ui.label("Anchor");
                    egui::ComboBox::from_id_salt(("hud_anchor", i))
                        .selected_text(anchor_label(w.anchor))
                        .show_ui(ui, |ui| {
                            for anchor in ANCHORS {
                                ui.selectable_value(&mut w.anchor, anchor, anchor_label(anchor));
                            }
                        });
                    ui.end_row();

                    for (label, value) in [
                        ("X", &mut w.x),
                        ("Y", &mut w.y),
                        ("Width", &mut w.width),
                        ("Height", &mut w.height),
                    ] {
                        ui.label(label);
                        ui.add(
                            egui::DragValue::new(value)
                                .range(0.0..=1.0)
                                .speed(0.001)
                                .max_decimals(3),
                        );
                        ui.end_row();
                    }

                    ui.label("Colour");
                    let mut arr = to_array(w.color);
                    ui.color_edit_button_srgba_unmultiplied(&mut arr);
                    w.color = from_array(arr);
                    ui.end_row();

                    if w.kind == WidgetKind::ShiftLights {
                        ui.label("Lit colour");
                        let mut arr = to_array(w.accent);
                        ui.color_edit_button_srgba_unmultiplied(&mut arr);
                        w.accent = from_array(arr);
                        ui.end_row();
                    }

                    ui.label("Background");
                    ui.horizontal(|ui| {
                        let mut has_bg = w.background.is_some();
                        if ui.checkbox(&mut has_bg, "").changed() {
                            w.background = has_bg.then(|| w.background.unwrap_or(Color::rgb(0x20, 0x20, 0x20)));
                        }
                        if let Some(bg) = w.background.as_mut() {
                            let mut arr = to_array(*bg);
                            ui.color_edit_button_srgba_unmultiplied(&mut arr);
                            *bg = from_array(arr);
                        }
                    });
                    ui.end_row();

                    if w.kind == WidgetKind::CustomText {
                        ui.label("Text");
                        ui.add(TextEdit::singleline(&mut w.text).desired_width(240.0));
                        ui.end_row();
                    }
                });
        });
    }

    fn add_widget(&mut self) {
        let base = kind_label(self.new_kind).to_lowercase().replace(' ', "_");
        let mut n = 1;
        let id = loop {
            let candidate = format!("{base}_{n}");
            if !self.layout.widgets.iter().any(|w| w.id == candidate) {
                break candidate;
            }
            n += 1;
        };
        let (width, height) = match self.new_kind {
            WidgetKind::RpmBar | WidgetKind::ShiftLights => (0.30, 0.02),
            WidgetKind::LapTime | WidgetKind::Delta | WidgetKind::CustomText => (0.16, 0.05),
            _ => (0.12, 0.07),
        };
        self.layout
            .widgets
            .push(Widget::new(&id, self.new_kind, Anchor::Center, 0.5, 0.5, width, height));
        self.selected = Some(self.layout.widgets.len() - 1);
    }

    fn remove_selected(&mut self) {
        if let Some(i) = self.selected.filter(|&i| i < self.layout.widgets.len()) {
            self.layout.widgets.remove(i);
        }
        self.selected = None;
    }

    fn validation(&self, ui: &mut egui::Ui, errors: &[acxs_manager::hud::ValidationError]) {
        if errors.is_empty() {
            ui.label(RichText::new("Layout is valid.").color(theme::OK));
            return;
        }
        ui.label(RichText::new("Layout problems").strong().color(theme::WARN));
        for error in errors {
            ui.label(RichText::new(format!("- {error}")).color(theme::WARN));
        }
    }
}

/// Opens the layout file picker. Runs on the background thread.
fn pick_open(dir: Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().add_filter("HUD layout", &["json"]);
    if let Some(dir) = dir {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}

/// Opens the save picker and ensures a `.json` extension. Runs on the background thread.
fn pick_save(dir: Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_file_name(DEFAULT_FILE)
        .add_filter("HUD layout", &["json"]);
    if let Some(dir) = dir {
        dialog = dialog.set_directory(dir);
    }
    let path = dialog.save_file()?;
    Some(if path.extension().is_none() {
        path.with_extension("json")
    } else {
        path
    })
}

fn kind_label(kind: WidgetKind) -> &'static str {
    match kind {
        WidgetKind::Speed => "Speed",
        WidgetKind::Gear => "Gear",
        WidgetKind::RpmBar => "RPM bar",
        WidgetKind::ShiftLights => "Shift lights",
        WidgetKind::LapTime => "Lap time",
        WidgetKind::Delta => "Delta",
        WidgetKind::Fuel => "Fuel",
        WidgetKind::TyreTemps => "Tyre temps",
        WidgetKind::Position => "Position",
        WidgetKind::CustomText => "Custom text",
    }
}

fn anchor_label(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::TopLeft => "Top left",
        Anchor::TopCenter => "Top centre",
        Anchor::TopRight => "Top right",
        Anchor::CenterLeft => "Centre left",
        Anchor::Center => "Centre",
        Anchor::CenterRight => "Centre right",
        Anchor::BottomLeft => "Bottom left",
        Anchor::BottomCenter => "Bottom centre",
        Anchor::BottomRight => "Bottom right",
    }
}

fn to_array(c: Color) -> [u8; 4] {
    [c.r, c.g, c.b, c.a]
}

fn from_array(a: [u8; 4]) -> Color {
    Color { r: a[0], g: a[1], b: a[2], a: a[3] }
}

fn to_color32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}
