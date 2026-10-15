//! Replay Viewer tool page: opens an `.acreplay` file, shows its summary and
//! exports the lap table as CSV. Parsing and writing run on background threads.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::replay::{format_lap_time, read_replay, ReplayInfo};
use acxs_manager::telemetry::LapInfo;
use acxs_manager::telemetry_csv::{laps_to_csv, write_csv};
use eframe::egui::{self, RichText};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub fn tool() -> Box<dyn Tool> {
    Box::new(ReplayViewer::new())
}

/// Results sent back from background threads.
enum Msg {
    Parsed {
        path: PathBuf,
        result: Result<ReplayInfo, String>,
    },
    Exported(Result<PathBuf, String>),
}

struct ReplayViewer {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// True while a background parse or export is running.
    busy: bool,
    /// The replay currently shown, with the path it was read from.
    loaded: Option<(PathBuf, ReplayInfo)>,
}

impl ReplayViewer {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        ReplayViewer {
            tx,
            rx,
            busy: false,
            loaded: None,
        }
    }

    /// Parse `path` on a background thread.
    fn start_parse(&mut self, path: PathBuf, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = read_replay(&path).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Parsed { path, result });
            ctx.request_repaint();
        });
    }

    /// Write the lap table of the loaded replay to `path` on a background thread.
    fn start_export(&mut self, path: PathBuf, laps: Vec<LapInfo>, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let csv = laps_to_csv(&laps);
            let result = match write_csv(&path, &csv) {
                Ok(()) => Ok(path),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(format!(
                    "{} already exists. Choose another file name.",
                    path.display()
                )),
                Err(e) => Err(format!("Could not write {}: {e}", path.display())),
            };
            let _ = tx.send(Msg::Exported(result));
            ctx.request_repaint();
        });
    }
}

impl Tool for ReplayViewer {
    fn name(&self) -> &'static str {
        "Replay Viewer"
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            match msg {
                Msg::Parsed { path, result: Ok(info) } => {
                    notices.push(Notice {
                        text: format!("Opened {}", file_name(&path)),
                        ok: true,
                    });
                    self.loaded = Some((path, info));
                }
                Msg::Parsed { path, result: Err(e) } => {
                    self.loaded = None;
                    notices.push(Notice {
                        text: format!("Could not open {}: {e}", file_name(&path)),
                        ok: false,
                    });
                }
                Msg::Exported(Ok(path)) => notices.push(Notice {
                    text: format!("Wrote lap CSV to {}", path.display()),
                    ok: true,
                }),
                Msg::Exported(Err(e)) => notices.push(Notice { text: e, ok: false }),
            }
        }

        widgets::heading(
            ui,
            "Replay Viewer",
            "Open an Assetto Corsa .acreplay file to see its summary and export the lap times.",
        );

        ui.horizontal(|ui| {
            let open = ui.add_enabled(!self.busy, widgets::primary_button("Open replay"));
            if open.clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter("acreplay", &["acreplay"])
                    .pick_file();
                if let Some(path) = picked {
                    self.start_parse(path, ui.ctx().clone());
                }
            }

            let can_export = !self.busy && self.loaded.is_some();
            let export = ui.add_enabled(can_export, egui::Button::new("Export laps CSV"));
            if export.clicked() {
                let laps = self
                    .loaded
                    .as_ref()
                    .map(|(_, info)| lap_rows(info))
                    .unwrap_or_default();
                let picked = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_file_name("laps.csv")
                    .save_file();
                if let Some(path) = picked {
                    self.start_export(path, laps, ui.ctx().clone());
                }
            }

            if self.busy {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Working...").color(theme::MUTED));
            }
        });
        ui.add_space(12.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match &self.loaded {
                None => widgets::empty_state(ui, "No replay loaded."),
                Some((path, info)) => summary(ui, path, info),
            });
    }
}

/// Converts the replay's lap times into the rows the telemetry CSV writer expects.
/// Replay files carry no numeric car ID, so `car_id` is 0. Only the first car is covered.
fn lap_rows(info: &ReplayInfo) -> Vec<LapInfo> {
    info.lap_times_ms
        .iter()
        .enumerate()
        .map(|(i, &lap_time_ms)| LapInfo {
            car_id: 0,
            lap_time_ms,
            laps: u16::try_from(i + 1).unwrap_or(u16::MAX),
            driver: info.driver.clone(),
            car: info.car.clone(),
        })
        .collect()
}

fn summary(ui: &mut egui::Ui, path: &Path, info: &ReplayInfo) {
    let track = match &info.config_track {
        Some(config) => format!("{} ({config})", info.track),
        None => info.track.clone(),
    };
    let weather = if info.weather.is_empty() { "-" } else { info.weather.as_str() };
    let best = info.lap_times_ms.iter().min().copied();

    theme::card().show(ui, |ui| {
        ui.label(RichText::new(file_name(path)).strong());
        ui.add_space(6.0);
        widgets::key_value(ui, "Track", &track);
        widgets::key_value(ui, "Car", &info.car);
        widgets::key_value(ui, "Driver", &info.driver);
        widgets::key_value(ui, "Laps", &info.lap_count.to_string());
        widgets::key_value(ui, "Duration", &format_duration_ms(info.duration_s));
        widgets::key_value(ui, "Samples", &info.sample_count.to_string());
        widgets::key_value(ui, "Weather", weather);
        widgets::key_value(ui, "Cars in replay", &info.num_cars.to_string());
    });
    ui.add_space(12.0);

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Lap times");
        if info.lap_times_ms.is_empty() {
            ui.label(RichText::new("No completed laps recorded.").color(theme::MUTED));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("replay_lap_table")
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("replay_laps")
                    .num_columns(2)
                    .striped(true)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Lap").color(theme::MUTED));
                        ui.label(RichText::new("Time").color(theme::MUTED));
                        ui.end_row();
                        for (i, &ms) in info.lap_times_ms.iter().enumerate() {
                            ui.label((i + 1).to_string());
                            let color = if Some(ms) == best { theme::OK } else { theme::TEXT };
                            ui.label(RichText::new(format_lap_time(ms)).monospace().color(color));
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Export writes this lap table only (first car). The replay's telemetry \
                 samples are not exported, and car_id is 0 because replays carry no numeric car ID.",
            )
            .small()
            .color(theme::MUTED),
        );
    });
}

/// Formats a duration in seconds as `M:SS.mmm`, the same style as lap times.
fn format_duration_ms(seconds: f64) -> String {
    let ms = (seconds * 1000.0).round().clamp(0.0, f64::from(u32::MAX));
    format_lap_time(ms as u32)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
