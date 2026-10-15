//! Race Engineer tool page: opens an `.acreplay` file, runs the rule-based
//! engineer over its first car and lists the advice by time, coloured by
//! severity. Reading and analysis run on a background thread.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::engineer::{analyse, Advice, EngineerLimits, Severity};
use acxs_manager::replay::read_replay_samples;
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    Box::new(RaceEngineer::new())
}

/// The outcome of analysing one replay.
struct Report {
    file: String,
    sample_count: usize,
    capped: bool,
    advice: Vec<Advice>,
}

/// Results sent back from the background thread.
enum Msg {
    Done {
        path: PathBuf,
        result: Result<Report, String>,
    },
}

struct RaceEngineer {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// True while a background load is running.
    busy: bool,
    /// The most recent analysis, if it succeeded.
    report: Option<Report>,
}

impl RaceEngineer {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        RaceEngineer {
            tx,
            rx,
            busy: false,
            report: None,
        }
    }

    /// Read and analyse `path` on a background thread.
    fn start_load(&mut self, path: PathBuf, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = load_and_analyse(&path);
            let _ = tx.send(Msg::Done { path, result });
            ctx.request_repaint();
        });
    }
}

impl Tool for RaceEngineer {
    fn name(&self) -> &'static str {
        "Race Engineer"
    }

    fn group(&self) -> &'static str {
        "Race"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            match msg {
                Msg::Done {
                    result: Ok(report), ..
                } => {
                    notices.push(Notice {
                        text: format!(
                            "Analysed {}: {} piece(s) of advice",
                            report.file,
                            report.advice.len()
                        ),
                        ok: true,
                    });
                    self.report = Some(report);
                }
                Msg::Done {
                    path,
                    result: Err(e),
                } => {
                    self.report = None;
                    notices.push(Notice {
                        text: format!("Could not analyse {}: {e}", file_name(&path)),
                        ok: false,
                    });
                }
            }
        }

        widgets::heading(
            ui,
            "Race Engineer",
            "Open an Assetto Corsa .acreplay file for rule-based advice on shifts, braking and traction.",
        );

        ui.horizontal(|ui| {
            let open = ui.add_enabled(!self.busy, widgets::primary_button("Open replay"));
            if open.clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter("acreplay", &["acreplay"])
                    .pick_file();
                if let Some(path) = picked {
                    self.start_load(path, ui.ctx().clone());
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
            .show(ui, |ui| match &self.report {
                None => widgets::empty_state(ui, "No replay analysed."),
                Some(report) => show_report(ui, report),
            });
    }
}

/// Reads the replay and runs the engineer with the default limits.
fn load_and_analyse(path: &Path) -> Result<Report, String> {
    let replay = read_replay_samples(path).map_err(|e| e.to_string())?;
    let samples = replay.samples();
    Ok(Report {
        file: file_name(path),
        sample_count: samples.len(),
        capped: replay.is_capped(),
        advice: analyse(samples, &EngineerLimits::default()),
    })
}

fn show_report(ui: &mut egui::Ui, report: &Report) {
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(&report.file).strong());
        ui.add_space(6.0);
        widgets::key_value(ui, "Samples", &report.sample_count.to_string());
        widgets::key_value(
            ui,
            "Critical",
            &count(report, Severity::Critical).to_string(),
        );
        widgets::key_value(ui, "Warnings", &count(report, Severity::Warn).to_string());
        widgets::key_value(ui, "Info", &count(report, Severity::Info).to_string());
        if report.capped {
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "The replay is longer than the sample cap, so only its first part was analysed.",
                )
                .small()
                .color(theme::MUTED),
            );
        }
    });
    ui.add_space(12.0);

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Advice");
        if report.advice.is_empty() {
            ui.label(RichText::new("No advice for this replay.").color(theme::MUTED));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("engineer_advice")
            .max_height(480.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for advice in &report.advice {
                    advice_row(ui, advice);
                }
            });
    });
}

fn advice_row(ui: &mut egui::Ui, advice: &Advice) {
    let color = severity_color(advice.severity());
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("{:>8.2} s", advice.t_s()))
                .monospace()
                .color(color),
        );
        widgets::pill(ui, severity_label(advice.severity()), color);
        ui.label(RichText::new(advice.text()).color(theme::TEXT));
    });
    ui.add_space(4.0);
}

fn count(report: &Report, severity: Severity) -> usize {
    report
        .advice
        .iter()
        .filter(|a| a.severity() == severity)
        .count()
}

fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Info => theme::INFO,
        Severity::Warn => theme::WARN,
        Severity::Critical => theme::DANGER,
    }
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "INFO",
        Severity::Warn => "WARN",
        Severity::Critical => "CRITICAL",
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
