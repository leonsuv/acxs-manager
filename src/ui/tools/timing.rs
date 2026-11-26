//! Timing Tower tool page: full-width broadcast motorsport timing tower that
//! auto-loads `Documents/Assetto Corsa/out/race_out.json` when available, loads
//! CSV files via dialog, or accepts a pasted CSV feed from a collapsible drawer.
//!
//! The fastest lap of the session is shown in purple (`SIGNAL_PURPLE`), and each
//! driver's own best lap in the success colour.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::timing::{self, Entry};
use acxs_manager::{race_out, settings};
use eframe::egui::{self, Align2, Color32, CornerRadius, RichText, Sense, vec2};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const HEADER: &str = "driver,laps,last_ms,best_ms,position";

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(TimingTower {
        tx,
        rx,
        text: String::new(),
        tower: Vec::new(),
        best: None,
        source_label: None,
        error: None,
        reading: false,
        auto_loaded: false,
        show_csv_drawer: false,
    })
}

/// Results sent back by the file dialog thread.
enum Msg {
    Cancelled,
    Read {
        path: PathBuf,
        result: Result<String, String>,
    },
}

struct TimingTower {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    text: String,
    tower: Vec<Entry>,
    /// Session fastest lap in milliseconds, with the driver who set it.
    best: Option<(String, u32)>,
    /// Human-readable label of the loaded feed (e.g. `race_out.json · monza`).
    source_label: Option<String>,
    error: Option<String>,
    reading: bool,
    auto_loaded: bool,
    show_csv_drawer: bool,
}

impl Tool for TimingTower {
    fn name(&self) -> &'static str {
        "Timing Tower"
    }

    fn content_width(&self) -> f32 {
        f32::INFINITY
    }

    fn group(&self) -> &'static str {
        "Race"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        if !self.auto_loaded {
            self.auto_loaded = true;
            let _ = self.load_latest_race_out();
        }

        widgets::heading(
            ui,
            "Timing Tower",
            "Broadcast motorsport race order, live gaps and purple overall fastest lap.",
        );
        let egui_ctx = ui.ctx().clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.body(ui, &egui_ctx, notices));
    }
}

impl TimingTower {
    fn body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        // Sleek top toolbar card.
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                if ui.add(widgets::primary_button("Load race_out.json")).clicked() {
                    match self.load_latest_race_out() {
                        Ok(n) => notices.push(Notice {
                            text: format!("Loaded latest race_out.json ({n} drivers)."),
                            ok: true,
                        }),
                        Err(e) => notices.push(Notice { text: e, ok: false }),
                    }
                }
                let load = ui.add_enabled(!self.reading, egui::Button::new("Load CSV File"));
                if load.clicked() {
                    self.spawn_load(ctx);
                }
                let drawer_label = if self.show_csv_drawer { "Hide Paste CSV" } else { "Paste CSV" };
                if ui.selectable_label(self.show_csv_drawer, drawer_label).clicked() {
                    self.show_csv_drawer = !self.show_csv_drawer;
                }
                if self.reading {
                    ui.spinner();
                    ui.label(RichText::new("Waiting for a file...").small().color(theme::MUTED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(src) = &self.source_label {
                        widgets::pill(ui, src, theme::INFO);
                    } else {
                        widgets::pill(ui, "NO FEED LOADED", theme::MUTED);
                    }
                });
            });

            if let Some(error) = &self.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(theme::DANGER));
            }

            if self.show_csv_drawer {
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Format:").small().color(theme::MUTED));
                    ui.label(RichText::new(HEADER).font(theme::mono(11.5)).color(theme::INFO));
                    ui.label(
                        RichText::new("(times in ms, empty or - for none)")
                            .small()
                            .color(theme::MUTED),
                    );
                });
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .desired_rows(6)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace)
                        .hint_text(format!("{HEADER}\nAna,12,91000,89500,2\nBen,12,90100,89000,1")),
                );
                ui.add_space(8.0);
                if ui.add(widgets::primary_button("Build Tower from CSV")).clicked() {
                    match self.build("Pasted CSV") {
                        Ok(n) => {
                            self.show_csv_drawer = false;
                            notices.push(Notice {
                                text: format!("Built the tower with {n} drivers."),
                                ok: true,
                            });
                        }
                        Err(e) => notices.push(Notice {
                            text: format!("Timing feed rejected: {e}"),
                            ok: false,
                        }),
                    }
                }
            }
        });

        ui.add_space(14.0);
        // Full-width Broadcast Motorsport Timing Tower hero.
        self.tower_card(ui, ctx, notices);
    }

    fn tower_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let session_ms = self.best.as_ref().map(|(_, ms)| *ms);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                widgets::section_title(ui, "Broadcast Timing Tower");
                if !self.tower.is_empty() {
                    widgets::pill(ui, &format!("{} DRIVERS", self.tower.len()), theme::INFO);
                }
                if let Some((name, ms)) = &self.best {
                    widgets::pill(
                        ui,
                        &format!("FASTEST LAP  {} · {name}", timing::format_lap(*ms)),
                        theme::SIGNAL_PURPLE,
                    );
                }
            });

            if self.tower.is_empty() {
                ui.add_space(36.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("NO TIMING SESSION LOADED")
                            .font(theme::display(20.0))
                            .color(theme::TEXT),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Finish an Assetto Corsa race to auto-load race_out.json, or load a CSV timing sheet.",
                        )
                        .color(theme::MUTED),
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 320.0).max(0.0) * 0.5);
                        if ui.add(widgets::primary_button("Load race_out.json")).clicked() {
                            match self.load_latest_race_out() {
                                Ok(n) => notices.push(Notice {
                                    text: format!("Loaded latest race_out.json ({n} drivers)."),
                                    ok: true,
                                }),
                                Err(e) => notices.push(Notice { text: e, ok: false }),
                            }
                        }
                        if ui.add_enabled(!self.reading, egui::Button::new("Load CSV File")).clicked() {
                            self.spawn_load(ctx);
                        }
                    });
                });
                ui.add_space(40.0);
                return;
            }

            ui.add_space(10.0);
            let avail_w = ui.available_width();
            let driver_w = (avail_w - 430.0).max(160.0);
            egui::ScrollArea::horizontal()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Grid::new("timing_tower_grid")
                        .num_columns(6)
                        .spacing([18.0, 9.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.add_sized([52.0, 20.0], egui::Label::new(RichText::new("POS").font(theme::display(12.5)).color(theme::MUTED)));
                            ui.add_sized([driver_w, 20.0], egui::Label::new(RichText::new("DRIVER").font(theme::display(12.5)).color(theme::MUTED)));
                            ui.add_sized([56.0, 20.0], egui::Label::new(RichText::new("LAPS").font(theme::display(12.5)).color(theme::MUTED)));
                            ui.add_sized([96.0, 20.0], egui::Label::new(RichText::new("LAST LAP").font(theme::display(12.5)).color(theme::MUTED)));
                            ui.add_sized([96.0, 20.0], egui::Label::new(RichText::new("BEST LAP").font(theme::display(12.5)).color(theme::MUTED)));
                            ui.add_sized([94.0, 20.0], egui::Label::new(RichText::new("GAP").font(theme::display(12.5)).color(theme::MUTED)));
                            ui.end_row();

                            for (index, row) in self.tower.iter().enumerate() {
                                position_badge(ui, row.position);
                                ui.label(RichText::new(&row.driver).font(theme::bold(14.5)).color(theme::TEXT));
                                ui.label(RichText::new(row.laps.to_string()).font(theme::mono(13.5)).color(theme::MUTED));

                                // Purple for overall session fastest lap, green for driver personal best.
                                let last_color = match row.last_ms {
                                    Some(ms) if Some(ms) == session_ms => theme::SIGNAL_PURPLE,
                                    Some(ms) if Some(ms) == row.best_ms => theme::OK,
                                    _ => theme::TEXT,
                                };
                                ui.label(lap_text(row.last_ms, last_color));

                                let best_color = match row.best_ms {
                                    Some(ms) if Some(ms) == session_ms => theme::SIGNAL_PURPLE,
                                    Some(_) => theme::OK,
                                    None => theme::TEXT,
                                };
                                ui.label(lap_text(row.best_ms, best_color));

                                match (index, row.gap_ms) {
                                    (0, _) => {
                                        widgets::pill(ui, "LEADER", theme::ACCENT);
                                    }
                                    (_, Some(gap)) => {
                                        ui.label(
                                            RichText::new(timing::format_gap(gap))
                                                .font(theme::mono(13.5))
                                                .color(theme::TEXT),
                                        );
                                    }
                                    (_, None) => {
                                        ui.label(RichText::new("—").font(theme::mono(13.5)).color(theme::MUTED));
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
        });
    }

    fn load_latest_race_out(&mut self) -> Result<usize, String> {
        let path = settings::ac_documents_dir()
            .map(|d| d.join("out").join("race_out.json"))
            .ok_or_else(|| "Documents/Assetto Corsa folder not found.".to_string())?;
        if !path.is_file() {
            return Err("No race_out.json found yet in Documents/Assetto Corsa/out.".to_string());
        }
        let summary = race_out::load_race_out(&path).map_err(|e| e.to_string())?;
        if summary.results.is_empty() {
            return Err("race_out.json contains no race results.".to_string());
        }
        let entries: Vec<Entry> = summary
            .results
            .iter()
            .map(|r| {
                let best_ms = if r.position == 1 || r.fastest_lap {
                    summary.player_best_lap_ms
                } else {
                    None
                };
                Entry {
                    driver: r.driver.clone(),
                    position: r.position,
                    laps: 0,
                    last_ms: None,
                    best_ms,
                    gap_ms: None,
                }
            })
            .collect();
        let tower = timing::build_tower(entries).map_err(|e| e.to_string())?;
        let count = tower.len();
        self.best = timing::session_best(&tower);
        self.tower = tower;
        self.error = None;
        self.source_label = Some(if summary.track.is_empty() {
            "RACE_OUT.JSON".to_string()
        } else {
            format!("RACE_OUT.JSON · {}", summary.track.to_uppercase())
        });
        Ok(count)
    }

    /// Parse the text box into the tower. On failure the previous tower stays on screen.
    fn build(&mut self, source: &str) -> Result<usize, String> {
        match timing::parse_csv(&self.text) {
            Ok(tower) => {
                self.best = timing::session_best(&tower);
                let count = tower.len();
                self.tower = tower;
                self.source_label = Some(source.to_string());
                self.error = None;
                Ok(count)
            }
            Err(e) => {
                let message = e.to_string();
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    fn spawn_load(&mut self, ctx: &egui::Context) {
        self.reading = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let msg = match rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file() {
                None => Msg::Cancelled,
                Some(path) => {
                    let result = std::fs::read_to_string(&path)
                        .map_err(|e| format!("could not read {}: {e}", path.display()));
                    Msg::Read { path, result }
                }
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.reading = false;
            let Msg::Read { path, result } = msg else {
                continue;
            };
            let file = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            match result.and_then(|text| {
                self.text = text;
                self.build(&file)
            }) {
                Ok(count) => notices.push(Notice {
                    text: format!("Loaded {file} with {count} drivers."),
                    ok: true,
                }),
                Err(e) => notices.push(Notice {
                    text: format!("{file} could not be loaded: {e}"),
                    ok: false,
                }),
            }
        }
    }
}

fn position_badge(ui: &mut egui::Ui, pos: u32) {
    let (fill, fg) = match pos {
        1 => (Color32::from_rgb(0xF5, 0xB8, 0x42), theme::SURFACE_VOID),
        2 => (Color32::from_rgb(0xC8, 0xD0, 0xDC), theme::SURFACE_VOID),
        3 => (Color32::from_rgb(0xCD, 0x7F, 0x45), theme::SURFACE_VOID),
        _ => (theme::SURFACE_RAISED, theme::TEXT),
    };
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 22.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            format!("P{pos}"),
            theme::mono(12.0),
            fg,
        );
    }
}

fn lap_text(ms: Option<u32>, color: egui::Color32) -> RichText {
    match ms {
        Some(ms) => RichText::new(timing::format_lap(ms)).font(theme::mono(13.5)).color(color),
        None => RichText::new("—").font(theme::mono(13.5)).color(theme::MUTED),
    }
}
