//! Setup Advice tool page: opens an `.acreplay` file, runs the heuristic setup
//! advisor over its samples and shows the estimated balance with notes. Reading and
//! analysis run on a background thread.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::replay::read_replay_samples;
use acxs_manager::setup_advice::{advise, Balance, SetupAdvice};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    Box::new(SetupAdviceTool::new())
}

/// Result sent back from the background thread.
struct Msg {
    path: PathBuf,
    /// True when the file had more frames than were stored and only the stored ones were analysed.
    capped: bool,
    result: Result<SetupAdvice, String>,
}

struct SetupAdviceTool {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// True while a replay is being read and analysed.
    busy: bool,
    /// The last result, with the file it came from.
    shown: Option<(PathBuf, bool, SetupAdvice)>,
}

impl SetupAdviceTool {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        SetupAdviceTool {
            tx,
            rx,
            busy: false,
            shown: None,
        }
    }

    /// Read `path` and run the advisor on its samples on a background thread.
    fn start_analysis(&mut self, path: PathBuf, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let msg = match read_replay_samples(&path) {
                Ok(replay) => Msg {
                    capped: replay.is_capped(),
                    result: advise(replay.samples()).map_err(|e| e.to_string()),
                    path,
                },
                Err(e) => Msg {
                    capped: false,
                    result: Err(e.to_string()),
                    path,
                },
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }
}

impl Tool for SetupAdviceTool {
    fn name(&self) -> &'static str {
        "Setup Advice"
    }

    fn group(&self) -> &'static str {
        "Setup"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            match msg.result {
                Ok(advice) => {
                    notices.push(Notice {
                        text: format!("Analysed {}", file_name(&msg.path)),
                        ok: true,
                    });
                    self.shown = Some((msg.path, msg.capped, advice));
                }
                Err(e) => {
                    self.shown = None;
                    notices.push(Notice {
                        text: format!("Could not analyse {}: {e}", file_name(&msg.path)),
                        ok: false,
                    });
                }
            }
        }

        widgets::heading(
            ui,
            "Setup Advice",
            "Open an Assetto Corsa .acreplay file to get a rough read on the car's balance in corners.",
        );

        ui.label(
            RichText::new(
                "Heuristic only. This tool estimates balance from the replay's position, speed, \
                 throttle and brake. It is not a measurement, and its suggestions are things to \
                 try, not fixes.",
            )
            .color(theme::WARN),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let open = ui.add_enabled(!self.busy, widgets::primary_button("Open replay"));
            if open.clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter("acreplay", &["acreplay"])
                    .pick_file();
                if let Some(path) = picked {
                    self.start_analysis(path, ui.ctx().clone());
                }
            }

            if self.busy {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Analysing...").color(theme::MUTED));
            }
        });
        ui.add_space(12.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match &self.shown {
                None => widgets::empty_state(ui, "No replay analysed yet."),
                Some((path, capped, advice)) => {
                    result(ui, path, *capped, advice);
                }
            });
    }
}

fn result(ui: &mut egui::Ui, path: &Path, capped: bool, advice: &SetupAdvice) {
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(file_name(path)).color(theme::MUTED));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Estimated balance").color(theme::MUTED));
            let (text, color) = balance_label(advice.balance);
            widgets::pill(ui, text, color);
        });
        if capped {
            ui.add_space(4.0);
            ui.label(
                RichText::new("The replay is longer than the stored samples. Only the stored part was analysed.")
                    .color(theme::WARN),
            );
        }
    });
    ui.add_space(12.0);

    widgets::section_title(ui, "Notes");
    for note in &advice.notes {
        theme::card().show(ui, |ui| {
            ui.label(note);
        });
        ui.add_space(6.0);
    }
}

fn balance_label(balance: Balance) -> (&'static str, egui::Color32) {
    match balance {
        Balance::Neutral => ("Neutral", theme::OK),
        Balance::Understeer => ("Understeer", theme::WARN),
        Balance::Oversteer => ("Oversteer", theme::DANGER),
        Balance::Unknown => ("Unclear", theme::MUTED),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
