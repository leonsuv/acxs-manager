//! Timing Tower tool page: paste a CSV feed or load one from a file and see the race order.
//!
//! The fastest lap of the session is shown in the accent colour, and each
//! driver's own best lap in the success colour. A file is read on a background
//! thread and its result comes back over a channel that is polled once per frame.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::timing::{self, Entry};
use eframe::egui::{self, RichText};
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
        error: None,
        reading: false,
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
    error: Option<String>,
    reading: bool,
}

impl Tool for TimingTower {
    fn name(&self) -> &'static str {
        "Timing Tower"
    }

    fn group(&self) -> &'static str {
        "Race"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        widgets::heading(
            ui,
            "Timing Tower",
            "Paste a CSV feed or load one from a file to see the race order, gaps and the session's fastest lap.",
        );
        let egui_ctx = ui.ctx().clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.body(ui, &egui_ctx, notices));
    }
}

impl TimingTower {
    fn body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "CSV feed");
            ui.label(RichText::new(format!("{HEADER}  (times in milliseconds, empty or - for none)")).small().color(theme::MUTED));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .desired_rows(8)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(format!("{HEADER}\nAna,12,91000,89500,2\nBen,12,90100,89000,1")),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(widgets::primary_button("Build tower")).clicked() {
                    match self.build() {
                        Ok(n) => notices.push(Notice {
                            text: format!("Built the tower with {n} drivers."),
                            ok: true,
                        }),
                        Err(e) => notices.push(Notice {
                            text: format!("Timing feed rejected: {e}"),
                            ok: false,
                        }),
                    }
                }
                let load = ui.add_enabled(!self.reading, egui::Button::new("Load CSV file"));
                if load.clicked() {
                    self.spawn_load(ctx);
                }
                if self.reading {
                    ui.spinner();
                    ui.label(RichText::new("Waiting for a file...").color(theme::MUTED));
                }
            });
            if let Some(error) = &self.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(theme::DANGER));
            }
        });

        ui.add_space(16.0);
        if self.tower.is_empty() {
            widgets::empty_state(ui, "No timing rows yet. Paste a feed above or load a CSV file.");
            return;
        }
        self.tower_card(ui);
    }

    fn tower_card(&self, ui: &mut egui::Ui) {
        let session_ms = self.best.as_ref().map(|(_, ms)| *ms);
        theme::card().show(ui, |ui| {
            if let Some((name, ms)) = &self.best {
                ui.label(
                    RichText::new(format!("Session best {} by {name}", timing::format_lap(*ms)))
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.add_space(8.0);
            }
            egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("timing_tower_grid")
                    .num_columns(6)
                    .spacing([18.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for head in ["Pos", "Driver", "Laps", "Last", "Best", "Gap"] {
                            ui.label(RichText::new(head).small().color(theme::MUTED));
                        }
                        ui.end_row();

                        for (index, row) in self.tower.iter().enumerate() {
                            ui.label(RichText::new(row.position.to_string()).strong());
                            ui.label(RichText::new(&row.driver).color(theme::TEXT));
                            ui.label(row.laps.to_string());

                            // The last lap is coloured when it is the session best, or when it is the driver's own best.
                            let last_color = match row.last_ms {
                                Some(ms) if Some(ms) == session_ms => theme::ACCENT,
                                Some(ms) if Some(ms) == row.best_ms => theme::OK,
                                _ => theme::TEXT,
                            };
                            ui.label(lap_text(row.last_ms, last_color));

                            let best_color = match row.best_ms {
                                Some(ms) if Some(ms) == session_ms => theme::ACCENT,
                                Some(_) => theme::OK,
                                None => theme::TEXT,
                            };
                            ui.label(lap_text(row.best_ms, best_color));

                            let gap = match (index, row.gap_ms) {
                                (0, _) => RichText::new("Leader").color(theme::MUTED),
                                (_, Some(gap)) => RichText::new(timing::format_gap(gap)).color(theme::TEXT),
                                (_, None) => RichText::new("-").color(theme::MUTED),
                            };
                            ui.label(gap);
                            ui.end_row();
                        }
                    });
            });
        });
    }

    /// Parse the text box into the tower. On failure the previous tower stays on screen.
    fn build(&mut self) -> Result<usize, String> {
        match timing::parse_csv(&self.text) {
            Ok(tower) => {
                self.best = timing::session_best(&tower);
                let count = tower.len();
                self.tower = tower;
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
                self.build()
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

fn lap_text(ms: Option<u32>, color: egui::Color32) -> RichText {
    match ms {
        Some(ms) => RichText::new(timing::format_lap(ms)).monospace().color(color),
        None => RichText::new("-").color(theme::MUTED),
    }
}
