//! Replay Viewer tool page: opens an `.acreplay` file, plays back its telemetry,
//! shows its summary and exports the lap table and the telemetry samples as CSV.
//! Parsing and writing run on background threads.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::replay::{
    format_lap_time, read_replay_samples, ReplayInfo, ReplaySamples, Sample,
};
use acxs_manager::replay_analysis;
use acxs_manager::replay_library::{self, ReplayFile, ReplayPlace};
use acxs_manager::replay_player::Player;
use acxs_manager::telemetry::LapInfo;
use acxs_manager::telemetry_csv::{laps_to_csv, replay_samples_to_csv, write_csv};
use eframe::egui::{self, RichText};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Playback speeds offered by the speed selector. The player clamps them to its own range.
const SPEEDS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

pub fn tool() -> Box<dyn Tool> {
    Box::new(ReplayViewer::new())
}

/// Results sent back from background threads.
enum Msg {
    Parsed {
        path: PathBuf,
        result: Result<Box<ReplaySamples>, String>,
    },
    Exported(Result<PathBuf, String>),
    /// Result of the samples export: the path written and the number of data rows.
    SamplesExported(Result<(PathBuf, usize), String>),
}

/// A replay shown in the viewer: its summary plus the playback model over its samples.
struct Loaded {
    path: PathBuf,
    info: ReplayInfo,
    player: Player,
    /// True when the file had more frames than were stored.
    capped: bool,
}

struct ReplayViewer {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// True while a background parse or export is running.
    busy: bool,
    /// The replay currently shown, with its playback state.
    loaded: Option<Loaded>,
    /// The replays on this PC; read when the page opens.
    library: Option<Vec<ReplayFile>>,
}

impl ReplayViewer {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        ReplayViewer {
            tx,
            rx,
            busy: false,
            loaded: None,
            library: None,
        }
    }

    /// Parse `path` and decode its samples on a background thread.
    fn start_parse(&mut self, path: PathBuf, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = read_replay_samples(&path).map(Box::new).map_err(|e| e.to_string());
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
            let result = write_csv(&path, &csv)
                .map(|()| path.clone())
                .map_err(|e| write_failure(&path, &e));
            let _ = tx.send(Msg::Exported(result));
            ctx.request_repaint();
        });
    }

    /// Write the telemetry samples of the loaded replay to `path` on a background thread.
    fn start_samples_export(&mut self, path: PathBuf, samples: Vec<Sample>, ctx: egui::Context) {
        self.busy = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let rows = samples.len();
            let csv = replay_samples_to_csv(&samples);
            let result = write_csv(&path, &csv)
                .map(|()| (path.clone(), rows))
                .map_err(|e| write_failure(&path, &e));
            let _ = tx.send(Msg::SamplesExported(result));
            ctx.request_repaint();
        });
    }
}

/// User-facing message for a failed write to `path`.
fn write_failure(path: &Path, e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::AlreadyExists {
        format!("{} already exists. Choose another file name.", path.display())
    } else {
        format!("Could not write {}: {e}", path.display())
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
                Msg::Parsed { path, result: Ok(replay) } => {
                    notices.push(Notice {
                        text: format!("Opened {}", file_name(&path)),
                        ok: true,
                    });
                    let capped = replay.is_capped();
                    let info = replay.info().clone();
                    let player = Player::new(replay.into_samples());
                    self.loaded = Some(Loaded {
                        path,
                        info,
                        player,
                        capped,
                    });
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
                Msg::SamplesExported(Ok((path, rows))) => notices.push(Notice {
                    text: format!("Wrote {rows} sample rows to {}", path.display()),
                    ok: true,
                }),
                Msg::SamplesExported(Err(e)) => notices.push(Notice { text: e, ok: false }),
            }
        }

        // Move the playhead by the frame time while playing, and keep repainting.
        if let Some(loaded) = &mut self.loaded
            && loaded.player.is_playing()
        {
            let dt = f64::from(ui.input(|i| i.stable_dt));
            loaded.player.advance(dt);
            ui.ctx().request_repaint();
        }

        widgets::heading(
            ui,
            "Replay Viewer",
            "Open an Assetto Corsa .acreplay file to play back its telemetry, see its summary and export the lap times.",
        );

        let files = self.library.get_or_insert_with(read_library);
        let mut reread = false;
        let mut open = None;
        egui::CollapsingHeader::new(format!("Replay library ({})", files.len()))
            .default_open(self.loaded.is_none())
            .show(ui, |ui| open = library_rows(ui, files, notices, &mut reread));
        if reread {
            self.library = None;
        }
        if let Some(path) = open
            && !self.busy
        {
            self.start_parse(path, ui.ctx().clone());
        }
        ui.add_space(6.0);

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
                    .map(|loaded| lap_rows(&loaded.info))
                    .unwrap_or_default();
                let picked = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_file_name("laps.csv")
                    .save_file();
                if let Some(path) = picked {
                    self.start_export(path, laps, ui.ctx().clone());
                }
            }

            let export_samples =
                ui.add_enabled(can_export, egui::Button::new("Export samples CSV"));
            if export_samples.clicked() {
                let picked = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_file_name("samples.csv")
                    .save_file();
                if let Some(path) = picked {
                    let samples = self
                        .loaded
                        .as_ref()
                        .map(|loaded| loaded.player.samples().to_vec())
                        .unwrap_or_default();
                    self.start_samples_export(path, samples, ui.ctx().clone());
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
            .show(ui, |ui| match &mut self.loaded {
                None => widgets::empty_state(ui, "No replay loaded."),
                Some(loaded) => {
                    playback(ui, loaded);
                    ui.add_space(12.0);
                    summary(ui, &loaded.path, &loaded.info, loaded.player.samples());
                }
            });
    }
}

/// The game's saved and autosave folders.
fn replay_dir() -> Option<PathBuf> {
    acxs_manager::settings::ac_documents_dir().map(|docs| docs.join("replay"))
}

fn read_library() -> Vec<ReplayFile> {
    replay_dir()
        .map(|dir| replay_library::list(&dir, &acxs_manager::session_recorder::replay_archive_dir()))
        .unwrap_or_default()
}

/// One row per replay. Returns the replay to open.
fn library_rows(ui: &mut egui::Ui, files: &[ReplayFile], notices: &mut Vec<Notice>, reread: &mut bool) -> Option<PathBuf> {
    let mut open = None;
    egui::ScrollArea::vertical().id_salt("replay_library").max_height(280.0).auto_shrink([false, true]).show(ui, |ui| {
        for file in files {
            ui.horizontal(|ui| {
                widgets::pill(ui, file.place.label(), if file.place == ReplayPlace::Autosave { theme::WARN } else { theme::MUTED });
                if file.online {
                    widgets::pill(ui, "Online", theme::INFO);
                }
                ui.add(egui::Label::new(file.title.as_str()).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Open").clicked() {
                        open = Some(file.path.clone());
                    }
                    if file.place != ReplayPlace::Saved
                        && ui.small_button("Keep").on_hover_text("Copy it to the saved folder, where the game leaves it alone.").clicked()
                        && let Some(dir) = replay_dir()
                    {
                        match replay_library::keep(file, &dir) {
                            Ok(_) => notices.push(Notice { text: "Replay kept.".into(), ok: true }),
                            Err(err) => notices.push(Notice { text: format!("Could not keep the replay: {err}"), ok: false }),
                        }
                        *reread = true;
                    }
                    let when: chrono::DateTime<chrono::Local> = file.modified.into();
                    ui.label(RichText::new(format!("{}  \u{00B7}  {} MB", when.format("%-d %b %H:%M"), file.bytes / 1_000_000)).size(12.0).color(theme::MUTED));
                });
            });
        }
    });
    open
}

/// Play/pause, speed selector, seek slider and the readout of the sample at the playhead.
fn playback(ui: &mut egui::Ui, loaded: &mut Loaded) {
    let capped = loaded.capped;
    let player = &mut loaded.player;
    let duration = player.duration_s();

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Playback");
        if player.samples().is_empty() {
            ui.label(RichText::new("No telemetry samples in this replay.").color(theme::MUTED));
            return;
        }

        ui.horizontal(|ui| {
            let label = if player.is_playing() { "Pause" } else { "Play" };
            if ui.add(widgets::primary_button(label)).clicked() {
                if player.is_playing() {
                    player.pause();
                } else {
                    player.play();
                }
            }
            ui.add_space(12.0);
            ui.label(RichText::new("Speed").color(theme::MUTED));
            let mut speed = player.speed();
            egui::ComboBox::from_id_salt("replay_speed")
                .selected_text(format_speed(speed))
                .show_ui(ui, |ui| {
                    for choice in SPEEDS {
                        ui.selectable_value(&mut speed, choice, format_speed(choice));
                    }
                });
            player.set_speed(speed);
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let seekable = duration > 0.0;
            let max = if seekable { duration } else { 1.0 };
            let mut position = player.position_s();
            ui.label(RichText::new(format_duration_ms(position)).monospace());
            ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(120.0);
            let slider = egui::Slider::new(&mut position, 0.0..=max).show_value(false);
            if ui.add_enabled(seekable, slider).changed() {
                player.seek(position);
            }
            ui.label(RichText::new(format_duration_ms(duration)).monospace());
        });

        ui.add_space(8.0);
        match player.current_sample() {
            Some(sample) => sample_readout(ui, sample),
            None => {
                ui.label(RichText::new("No sample at the playhead.").color(theme::MUTED));
            }
        }

        if capped {
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "This file has more frames than are stored, so playback ends at the last stored sample.",
                )
                .small()
                .color(theme::MUTED),
            );
        }
    });
}

/// Rows for the telemetry values of one sample.
fn sample_readout(ui: &mut egui::Ui, sample: &Sample) {
    widgets::key_value(ui, "Speed", &format!("{:.1} km/h", sample.speed_kmh));
    widgets::key_value(ui, "Gear", &gear_label(sample.gear));
    widgets::key_value(ui, "RPM", &format!("{:.0}", sample.rpm));
    widgets::key_value(ui, "Throttle", &format!("{:.0}%", sample.throttle * 100.0));
    widgets::key_value(ui, "Brake", &format!("{:.0}%", sample.brake * 100.0));
    widgets::key_value(
        ui,
        "Position",
        &format!("x {:.1}, y {:.1}, z {:.1} m", sample.x, sample.y, sample.z),
    );
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

fn summary(ui: &mut egui::Ui, path: &Path, info: &ReplayInfo, samples: &[Sample]) {
    let found = replay_analysis::analyze(samples);
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
        widgets::key_value(ui, "Distance", &format!("{:.1} km", found.distance_km));
        widgets::key_value(ui, "Top / average speed", &format!("{:.0} / {:.0} km/h", found.max_speed, found.average_speed));
        widgets::key_value(ui, "Full throttle", &format!("{:.0}%", found.full_throttle_share * 100.0));
        widgets::key_value(ui, "Hard braking", &format!("{:.0} s", found.hard_braking_s));
        widgets::key_value(ui, "Impacts / drops", &format!("{} / {}", found.impacts, found.drops));
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
                "Export laps CSV writes this lap table, and Export samples CSV writes one row \
                 per telemetry sample. Both cover the first car only. car_id is 0 because \
                 replays carry no numeric car ID.",
            )
            .small()
            .color(theme::MUTED),
        );
    });
}

/// Speed multiplier as shown in the selector, e.g. `0.5x`.
fn format_speed(multiplier: f64) -> String {
    format!("{multiplier}x")
}

/// Gear label from the raw gear code: 0 = reverse, 1 = neutral, 2 and up = forward gears.
fn gear_label(gear: u8) -> String {
    match gear {
        0 => "R".to_owned(),
        1 => "N".to_owned(),
        n => (n - 1).to_string(),
    }
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
