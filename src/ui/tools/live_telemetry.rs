//! Live Telemetry tool page: connects to the built-in UDP telemetry of a running
//! Assetto Corsa and shows the handshake, the latest car sample and completed laps.
//!
//! The socket lives on a background thread. It forwards each decoded packet
//! over a channel and wakes the UI with `request_repaint`. The UI only drains
//! the channel, so it never waits on the network or on the save dialog.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, ACCENT, BG, BORDER, INFO, MUTED, OK, TEXT, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::charts::{Channel, ChannelBuffers, Ring, Sample};
use acxs_manager::telemetry::{CarUpdate, HandshakeInfo, LapInfo, Packet, summarize_laps};
use acxs_manager::telemetry_csv;
use acxs_manager::track_map::{friction_circle_outline, friction_circle_points, project_lap};
use acxs_manager::udp_client::{ClientError, DEFAULT_GAME_PORT, TelemetryClient};
use eframe::egui::{self, RichText};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// How long one receive waits before the thread checks the stop flag again.
const POLL_TIMEOUT: Duration = Duration::from_millis(250);

/// Samples kept per chart channel. At 60 car updates a second this is about
/// two minutes of history.
const CHART_CAPACITY: usize = 8192;

/// Seconds of history drawn in each line plot.
const PLOT_WINDOW_S: f64 = 30.0;

/// Radius of the friction circle in g. Points beyond it are pulled onto the edge.
const FRICTION_MAX_G: f32 = 2.0;

/// Positions kept for the lap trail. Recording stops at this count until the
/// car starts a new lap.
const TRAIL_CAPACITY: usize = 20_000;

pub fn tool() -> Box<dyn Tool> {
    Box::new(LiveTelemetry::default())
}

/// Message from the socket thread to the UI.
enum Msg {
    Handshake(HandshakeInfo),
    Car(CarUpdate),
    Lap(LapInfo),
    /// A datagram that did not decode, or a packet the decoder does not model.
    Ignored,
    /// The session is over. The text goes to the status line.
    Ended(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    Idle,
    Running,
    Stopping,
}

struct LiveTelemetry {
    host: String,
    port: String,
    link: Link,
    status: String,
    /// Set to true to end the current socket thread.
    stop: Option<Arc<AtomicBool>>,
    rx: Option<mpsc::Receiver<Msg>>,
    handshake: Option<HandshakeInfo>,
    car: Option<CarUpdate>,
    laps: Vec<LapInfo>,
    car_updates: u64,
    ignored: u64,
    /// Result of a laps CSV export, present while the save dialog is open or
    /// the file is being written.
    export_rx: Option<mpsc::Receiver<Notice>>,
    charts: LiveCharts,
}

/// Chart and map state built from the car updates of one connection.
struct LiveCharts {
    /// Time zero for every sample in this connection.
    clock: Instant,
    channels: ChannelBuffers,
    /// `[x, z]` world positions since the car last started a lap.
    trail: Vec<[f32; 2]>,
    last_lap_ms: Option<u32>,
}

impl LiveCharts {
    fn new() -> Self {
        Self {
            clock: Instant::now(),
            channels: ChannelBuffers::new(CHART_CAPACITY),
            trail: Vec::new(),
            last_lap_ms: None,
        }
    }

    fn reset(&mut self) {
        self.clock = Instant::now();
        self.channels.clear();
        self.trail.clear();
        self.last_lap_ms = None;
    }

    /// Records one car update. The lap trail restarts when the lap time drops.
    fn record(&mut self, car: &CarUpdate) {
        let t = self.clock.elapsed().as_secs_f64();
        // Pedals are plotted as percentages of full travel.
        self.channels.push(Channel::Throttle, t, car.throttle * 100.0);
        self.channels.push(Channel::Brake, t, car.brake * 100.0);
        self.channels.push(Channel::Steering, t, car.steer_angle);
        self.channels.push(Channel::Speed, t, car.speed_kmh);
        self.channels.push(Channel::Gear, t, f32::from(car.gear));

        if self.last_lap_ms.is_some_and(|prev| car.lap_time_ms < prev) {
            self.trail.clear();
        }
        self.last_lap_ms = Some(car.lap_time_ms);
        if self.trail.len() < TRAIL_CAPACITY {
            let (x, _, z) = car.position;
            self.trail.push([x, z]);
        }
    }
}

impl Default for LiveTelemetry {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: DEFAULT_GAME_PORT.to_string(),
            link: Link::Idle,
            status: String::new(),
            stop: None,
            rx: None,
            handshake: None,
            car: None,
            laps: Vec::new(),
            car_updates: 0,
            ignored: 0,
            export_rx: None,
            charts: LiveCharts::new(),
        }
    }
}

impl Drop for LiveTelemetry {
    fn drop(&mut self) {
        self.stop_session();
    }
}

impl Tool for LiveTelemetry {
    fn name(&self) -> &'static str {
        "Live Telemetry"
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll_session();
        self.poll_export(notices);
        let ctx = ui.ctx().clone();

        widgets::heading(
            ui,
            "Live Telemetry",
            "Reads what a running Assetto Corsa reports over its built-in UDP telemetry.",
        );
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.connection_card(ui, &ctx, notices);
            ui.add_space(16.0);
            self.session_card(ui);
            ui.add_space(16.0);
            self.live_card(ui);
            ui.add_space(16.0);
            self.charts_card(ui);
            ui.add_space(16.0);
            self.track_card(ui);
            ui.add_space(16.0);
            self.friction_card(ui);
            ui.add_space(16.0);
            self.laps_card(ui, &ctx);
        });
    }
}

impl LiveTelemetry {
    fn stop_session(&self) {
        if let Some(stop) = &self.stop {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Drains packets from the socket thread. Never waits.
    fn poll_session(&mut self) {
        let Some(rx) = self.rx.as_ref() else {
            return;
        };
        let mut ended: Option<String> = None;
        loop {
            match rx.try_recv() {
                Ok(Msg::Handshake(h)) => self.handshake = Some(h),
                Ok(Msg::Car(c)) => {
                    self.charts.record(&c);
                    self.car = Some(c);
                    self.car_updates += 1;
                }
                Ok(Msg::Lap(lap)) => self.laps.push(lap),
                Ok(Msg::Ignored) => self.ignored += 1,
                Ok(Msg::Ended(reason)) => {
                    ended = Some(reason);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    ended = Some("Telemetry thread stopped unexpectedly.".to_string());
                    break;
                }
            }
        }
        if let Some(reason) = ended {
            self.rx = None;
            self.stop = None;
            self.link = Link::Idle;
            self.status = reason;
        }
    }

    fn poll_export(&mut self, notices: &mut Vec<Notice>) {
        let Some(rx) = self.export_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(notice) => {
                notices.push(notice);
                self.export_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            // The thread ends without a message when the dialog is cancelled.
            Err(mpsc::TryRecvError::Disconnected) => self.export_rx = None,
        }
    }

    fn connect(&mut self, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let addr = match parse_target(&self.host, &self.port) {
            Ok(addr) => addr,
            Err(e) => {
                notices.push(Notice { text: e, ok: false });
                return;
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let thread_stop = Arc::clone(&stop);
        let thread_ctx = ctx.clone();
        thread::spawn(move || run_session(addr, thread_stop, tx, thread_ctx));

        self.stop = Some(stop);
        self.rx = Some(rx);
        self.link = Link::Running;
        self.status = format!("Connecting to {addr}...");
        self.handshake = None;
        self.car = None;
        self.laps.clear();
        self.car_updates = 0;
        self.ignored = 0;
        self.charts.reset();
    }

    fn disconnect(&mut self) {
        self.stop_session();
        self.link = Link::Stopping;
        self.status = "Disconnecting...".to_string();
    }

    fn export_laps(&mut self, ctx: &egui::Context) {
        let csv = telemetry_csv::laps_to_csv(&self.laps);
        let count = self.laps.len();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        // rfd runs the dialog on the main thread on macOS, so this thread waits
        // for the user while the UI keeps running.
        thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_file_name("laps.csv")
                .add_filter("CSV", &["csv"])
                .save_file()
            else {
                return;
            };
            let notice = match telemetry_csv::write_csv(&path, &csv) {
                Ok(()) => Notice {
                    text: format!("Saved {count} laps to {}", path.display()),
                    ok: true,
                },
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Notice {
                    text: format!(
                        "{} already exists and was not overwritten. Choose another name.",
                        path.display()
                    ),
                    ok: false,
                },
                Err(e) => Notice {
                    text: format!("Could not save {}: {e}", path.display()),
                    ok: false,
                },
            };
            let _ = tx.send(notice);
            ctx.request_repaint();
        });
        self.export_rx = Some(rx);
    }

    fn link_label(&self) -> (&'static str, egui::Color32) {
        match self.link {
            Link::Idle => ("not connected", MUTED),
            Link::Stopping => ("disconnecting", WARN),
            Link::Running if self.handshake.is_some() => ("game answered", OK),
            Link::Running => ("waiting for game", WARN),
        }
    }

    fn connection_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        let idle = self.link == Link::Idle;
        let running = self.link == Link::Running;
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Connection");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Host").color(MUTED));
                ui.add_enabled(
                    idle,
                    egui::TextEdit::singleline(&mut self.host)
                        .desired_width(150.0)
                        .hint_text("IP address"),
                );
                ui.label(RichText::new("Port").color(MUTED));
                ui.add_enabled(idle, egui::TextEdit::singleline(&mut self.port).desired_width(70.0));
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(idle, widgets::primary_button("Connect")).clicked() {
                    self.connect(ctx, notices);
                }
                if ui.add_enabled(running, egui::Button::new("Disconnect")).clicked() {
                    self.disconnect();
                }
                let (text, color) = self.link_label();
                widgets::pill(ui, text, color);
            });
            if !self.status.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(&self.status).color(TEXT));
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "The game must have UDP telemetry enabled, or nothing will answer. \
                     Host must be an IP address.",
                )
                .color(MUTED),
            );
            ui.label(
                RichText::new(
                    "Nothing is sent to the game except one handshake request when you connect \
                     and one dismiss request when you disconnect.",
                )
                .color(MUTED),
            );
            ui.label(
                RichText::new(
                    "This build does not send the car-update subscription request, because the \
                     telemetry module does not provide one. Car updates and laps may never \
                     arrive. Only the handshake result is expected.",
                )
                .color(WARN),
            );
        });
    }

    fn session_card(&self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Session");
            match &self.handshake {
                Some(h) => {
                    widgets::key_value(ui, "Car", &h.car);
                    widgets::key_value(ui, "Driver", &h.driver);
                    widgets::key_value(ui, "Track", &h.track);
                    let layout = if h.track_config.is_empty() {
                        "default"
                    } else {
                        h.track_config.as_str()
                    };
                    widgets::key_value(ui, "Layout", layout);
                }
                None => {
                    let text = if self.link == Link::Idle {
                        "Not connected."
                    } else {
                        "Waiting for the game to answer the handshake."
                    };
                    ui.label(RichText::new(text).color(MUTED));
                }
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "Car updates received: {}   Ignored datagrams: {}",
                    self.car_updates, self.ignored
                ))
                .small()
                .color(MUTED),
            );
        });
    }

    fn live_card(&self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Live car");
            if let Some(car) = &self.car {
                ui.horizontal_wrapped(|ui| {
                    widgets::stat(ui, "Speed", &format!("{:.0} km/h", car.speed_kmh), TEXT);
                    widgets::stat(ui, "Gear", &gear_label(car.gear), TEXT);
                    widgets::stat(ui, "RPM", &car.rpm.to_string(), TEXT);
                    widgets::stat(ui, "Lap time", &format_lap(car.lap_time_ms), TEXT);
                });
            } else {
                ui.label(RichText::new("No car updates yet.").color(MUTED));
            }
        });
    }

    fn charts_card(&self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Charts");
            ui.label(
                RichText::new(format!("The last {PLOT_WINDOW_S:.0} seconds of samples."))
                    .small()
                    .color(MUTED),
            );
            ui.add_space(8.0);
            let mut drawn = false;
            for channel in Channel::ALL {
                let ring = self.charts.channels.ring(channel);
                if ring.is_empty() {
                    continue;
                }
                draw_plot(ui, channel, ring);
                ui.add_space(8.0);
                drawn = true;
            }
            if !drawn {
                ui.label(RichText::new("No samples yet.").color(MUTED));
            }
            ui.label(
                RichText::new("Throttle and brake are shown as percent of pedal travel.")
                    .small()
                    .color(MUTED),
            );
        });
    }

    fn track_card(&self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Track map");
            let side = ui.available_width().clamp(160.0, 360.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 6.0, BG);

            let points = project_lap(&self.charts.trail);
            if points.len() < 2 {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drive a lap to draw the track.",
                    egui::FontId::proportional(13.0),
                    MUTED,
                );
            } else {
                let inner = rect.shrink(16.0);
                let to_screen = |p: &[f32; 2]| {
                    egui::pos2(
                        inner.left() + p[0] * inner.width(),
                        inner.top() + p[1] * inner.height(),
                    )
                };
                let line: Vec<egui::Pos2> = points.iter().map(to_screen).collect();
                if let Some(&head) = line.last() {
                    painter.add(egui::Shape::line(line, egui::Stroke::new(2.0, OK)));
                    painter.circle_filled(head, 4.0, ACCENT);
                }
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{} positions this lap", self.charts.trail.len()))
                    .small()
                    .color(MUTED),
            );
        });
    }

    fn friction_card(&self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Friction circle");
            let side = ui.available_width().clamp(160.0, 360.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 6.0, BG);

            let center = rect.center();
            let radius = rect.width() * 0.5 - 16.0;
            let to_screen = |p: &[f32; 2]| {
                egui::pos2(center.x + p[0] * radius, center.y - p[1] * radius)
            };

            let axis = egui::Stroke::new(1.0, BORDER);
            painter.line_segment(
                [egui::pos2(center.x - radius, center.y), egui::pos2(center.x + radius, center.y)],
                axis,
            );
            painter.line_segment(
                [egui::pos2(center.x, center.y - radius), egui::pos2(center.x, center.y + radius)],
                axis,
            );

            let points = match &self.car {
                Some(car) => friction_circle_points(car.g_lateral, car.g_longitudinal, FRICTION_MAX_G),
                None => Vec::new(),
            };
            match points.split_last() {
                // The last point is the current g-force; the rest is the circle.
                Some((marker, outline)) if outline.len() > 1 => {
                    let outline: Vec<egui::Pos2> = outline.iter().map(to_screen).collect();
                    painter.add(egui::Shape::line(outline, egui::Stroke::new(1.0, BORDER)));
                    painter.circle_filled(to_screen(marker), 5.0, ACCENT);
                }
                _ => {
                    let outline: Vec<egui::Pos2> = friction_circle_outline().iter().map(to_screen).collect();
                    painter.add(egui::Shape::line(outline, egui::Stroke::new(1.0, BORDER)));
                    painter.text(
                        rect.center_bottom() - egui::vec2(0.0, 8.0),
                        egui::Align2::CENTER_BOTTOM,
                        "No g-force data yet.",
                        egui::FontId::proportional(12.0),
                        MUTED,
                    );
                }
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "Lateral is across, longitudinal is up. Circle radius is {FRICTION_MAX_G:.0} g."
                ))
                .small()
                .color(MUTED),
            );
        });
    }

    fn laps_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        theme::card().show(ui, |ui| {
            widgets::section_title(ui, "Laps this session");
            let summary = summarize_laps(&self.laps);
            let best = summary.best_ms.map(format_lap).unwrap_or_else(|| "-".to_string());
            let average = summary.average_ms.map(format_lap).unwrap_or_else(|| "-".to_string());
            let valid = summary.count.to_string();
            ui.horizontal_wrapped(|ui| {
                widgets::stat(ui, "Best", &best, OK);
                widgets::stat(ui, "Average", &average, TEXT);
                widgets::stat(ui, "Valid laps", &valid, TEXT);
            });
            ui.label(
                RichText::new(format!(
                    "{} laps received. Laps under 10 seconds are left out of the summary.",
                    self.laps.len()
                ))
                .small()
                .color(MUTED),
            );
            ui.add_space(8.0);
            let can_export = !self.laps.is_empty() && self.export_rx.is_none();
            if ui.add_enabled(can_export, egui::Button::new("Export laps CSV")).clicked() {
                self.export_laps(ctx);
            }
            ui.add_space(8.0);

            if self.laps.is_empty() {
                ui.label(RichText::new("No completed laps yet.").color(MUTED));
            } else {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new("live_telemetry_laps")
                            .striped(true)
                            .num_columns(4)
                            .show(ui, |ui| {
                                for head in ["Lap", "Time", "Car", "Driver"] {
                                    ui.label(RichText::new(head).color(MUTED));
                                }
                                ui.end_row();
                                for lap in self.laps.iter().rev() {
                                    ui.label(lap.laps.to_string());
                                    ui.label(format_lap(lap.lap_time_ms));
                                    ui.label(lap.car.as_str());
                                    ui.label(lap.driver.as_str());
                                    ui.end_row();
                                }
                            });
                    });
            }
        });
    }
}

/// Parses the host (an IP address, so nothing blocks on DNS) and port fields.
fn parse_target(host: &str, port: &str) -> Result<SocketAddr, String> {
    let ip: IpAddr = host
        .trim()
        .parse()
        .map_err(|_| "Host must be an IP address, for example 127.0.0.1.".to_string())?;
    let port: u16 = port
        .trim()
        .parse()
        .ok()
        .filter(|&p| p != 0)
        .ok_or_else(|| "Port must be a number from 1 to 65535.".to_string())?;
    Ok(SocketAddr::new(ip, port))
}

/// Body of the socket thread. Owns the client, forwards packets, and sends a
/// dismiss request when it exits.
fn run_session(addr: SocketAddr, stop: Arc<AtomicBool>, tx: mpsc::Sender<Msg>, ctx: egui::Context) {
    let send = |msg: Msg| {
        let _ = tx.send(msg);
        ctx.request_repaint();
    };

    let client = match TelemetryClient::connect(addr, POLL_TIMEOUT) {
        Ok(client) => client,
        Err(e) => {
            send(Msg::Ended(format!("Could not open the telemetry socket: {e}")));
            return;
        }
    };

    let reason = loop {
        if stop.load(Ordering::Relaxed) {
            break "Disconnected.".to_string();
        }
        match client.recv_packet() {
            Ok(Packet::Handshake(h)) => send(Msg::Handshake(h)),
            Ok(Packet::CarUpdate(c)) => send(Msg::Car(c)),
            Ok(Packet::LapCompleted(lap)) => send(Msg::Lap(lap)),
            Ok(Packet::Other(_)) | Err(ClientError::Decode(_)) => send(Msg::Ignored),
            Err(ClientError::Timeout) => {}
            Err(ClientError::Io(e)) => break format!("Telemetry socket error: {e}"),
        }
    };

    let _ = client.send_dismiss();
    send(Msg::Ended(reason));
}

/// One line plot of the last `PLOT_WINDOW_S` seconds of a channel.
fn draw_plot(ui: &mut egui::Ui, channel: Channel, ring: &Ring) {
    let Some(last) = ring.last() else {
        return;
    };
    let from = last.t - PLOT_WINDOW_S;
    let Some((lo, hi)) = ring.min_max(from, last.t) else {
        return;
    };
    let (mut lo, mut hi) = (f64::from(lo), f64::from(hi));
    if hi - lo < 1e-3 {
        // A flat line still needs a band, or every point lands on the same edge.
        lo -= 1.0;
        hi += 1.0;
    }

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 96.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, BG);
    let plot = rect.shrink2(egui::vec2(8.0, 22.0));

    let to_screen = |s: &Sample| {
        let fx = ((s.t - from) / PLOT_WINDOW_S).clamp(0.0, 1.0) as f32;
        let fy = ((f64::from(s.value) - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
        egui::pos2(plot.left() + fx * plot.width(), plot.bottom() - fy * plot.height())
    };
    let points = ring.downsample(from, last.t, plot.width().max(2.0) as usize);
    let line: Vec<egui::Pos2> = points.iter().map(to_screen).collect();
    if line.len() >= 2 {
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, INFO)));
    }

    painter.text(
        rect.left_top() + egui::vec2(8.0, 4.0),
        egui::Align2::LEFT_TOP,
        format!("{}  {:.1}", channel.label(), last.value),
        egui::FontId::proportional(12.0),
        TEXT,
    );
    painter.text(
        rect.right_top() + egui::vec2(-8.0, 4.0),
        egui::Align2::RIGHT_TOP,
        format!("{lo:.1} to {hi:.1}"),
        egui::FontId::proportional(11.0),
        MUTED,
    );
}

/// Formats milliseconds as `m:ss.mmm`.
fn format_lap(ms: u32) -> String {
    format!("{}:{:02}.{:03}", ms / 60_000, (ms % 60_000) / 1_000, ms % 1_000)
}

/// Gear as shown on the dash. Shared-memory gear values use
/// 0 for reverse, 1 for neutral and 2 for first gear.
fn gear_label(gear: i8) -> String {
    match gear {
        0 => "R".to_string(),
        1 => "N".to_string(),
        g if g >= 2 => (g - 1).to_string(),
        _ => "?".to_string(),
    }
}
