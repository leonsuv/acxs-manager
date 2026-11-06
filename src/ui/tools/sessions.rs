//! Sessions tool page: recorded drives on the left, the chosen drive in full on the right.

use super::names::Names;
use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::collision_app;
use acxs_manager::driving_advice;
use acxs_manager::laptimes::format_lap;
use acxs_manager::session_recorder::{self, EventKind, RecordedSession};
use acxs_manager::track_map::project_lap;
use eframe::egui::{self, RichText};
use std::sync::{Arc, Mutex};

/// Laps shorter than this are the game's placeholders, not laps.
const MIN_LAP_MS: i32 = 10_000;
const LIST_WIDTH: f32 = 340.0;

pub fn tool() -> Box<dyn Tool> {
    Box::new(Sessions {
        sessions: Arc::new(Mutex::new(None)),
        loading: false,
        goto: None,
        last_seen: None,
        seen_saved: 0,
        selected: None,
        confirm_delete: false,
    })
}

struct Sessions {
    /// Newest first; `None` until the first load finishes.
    sessions: Arc<Mutex<Option<Vec<RecordedSession>>>>,
    loading: bool,
    goto: Option<&'static str>,
    /// When the page was last drawn; coming back after a pause reloads the list.
    last_seen: Option<std::time::Instant>,
    seen_saved: usize,
    /// `started_at` of the session on show.
    selected: Option<String>,
    confirm_delete: bool,
}

impl Sessions {
    fn reload(&mut self, ctx: &egui::Context) {
        self.loading = true;
        let slot = Arc::clone(&self.sessions);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut list = session_recorder::load_all(&session_recorder::sessions_dir());
            list.reverse();
            if let Ok(mut guard) = slot.lock() {
                *guard = Some(list);
            }
            ctx.request_repaint();
        });
    }

    /// Page-wide actions, out of the way of the data.
    fn actions_menu(ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>, reload: &mut bool) {
        ui.menu_button("More", |ui| {
            if ui.button("Reload").clicked() {
                *reload = true;
                ui.close();
            }
            if ui.button("Open sessions folder").clicked() {
                let dir = session_recorder::sessions_dir();
                let _ = std::fs::create_dir_all(&dir);
                crate::ui::pages::open_folder(&dir);
                ui.close();
            }
            if ui.button("Keep more game replays").on_hover_text("Raises the autosave limits of the game. The old file is backed up.").clicked() {
                match session_recorder::keep_more_replays() {
                    Ok(_) => notices.push(Notice { text: "The game now keeps more replays.".into(), ok: true }),
                    Err(err) => notices.push(Notice { text: format!("Could not change the replay limits: {err}"), ok: false }),
                }
                ui.close();
            }
            if let Some(root) = &ctx.ac_root {
                let label = if collision_app::is_installed(root) { "Reinstall collision app" } else { "Install collision app" };
                let response = ui
                    .add_enabled(!ctx.game_running, egui::Button::new(label))
                    .on_hover_text("A small CSP app that tells wall hits from car hits. The game must be closed.");
                if response.clicked() {
                    match collision_app::install(root, ctx.game_running) {
                        Ok(_) => notices.push(Notice { text: "Installed. It runs by itself from the next start of the game.".into(), ok: true }),
                        Err(err) => notices.push(Notice { text: format!("Could not install the collision app: {err}"), ok: false }),
                    }
                    ui.close();
                }
            }
        });
    }
}

impl Tool for Sessions {
    fn name(&self) -> &'static str {
        "Sessions"
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn content_width(&self) -> f32 {
        1560.0
    }

    fn take_goto(&mut self) -> Option<&'static str> {
        self.goto.take()
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        let fresh = ctx.sessions_saved != self.seen_saved;
        self.seen_saved = ctx.sessions_saved;
        let away = fresh || self.last_seen.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(1));
        self.last_seen = Some(std::time::Instant::now());
        let ready = self.sessions.lock().ok().is_some_and(|g| g.is_some());
        if (!ready || away) && !self.loading {
            self.reload(ui.ctx());
        }

        let mut reload = false;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Sessions").font(theme::bold(26.0)));
                ui.label(RichText::new("Your recorded drives. ACXS records while it is open and the game is running.").color(theme::MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                Self::actions_menu(ui, ctx, notices, &mut reload);
            });
        });
        ui.add_space(12.0);

        let snapshot = self.sessions.lock().ok().and_then(|g| g.clone());
        let Some(list) = snapshot else {
            ui.spinner();
            return;
        };
        self.loading = false;
        if list.is_empty() {
            widgets::empty_state(ui, "Nothing recorded yet. Keep ACXS Manager open while you drive and your sessions appear here.");
            return;
        }
        let names = ctx.ac_root.as_deref().map(|root| super::names::names(root, ui.ctx())).unwrap_or_default();

        let selected = self
            .selected
            .as_deref()
            .and_then(|id| list.iter().position(|s| s.started_at == id))
            .unwrap_or(0);

        let mut picked = None;
        egui::Panel::left("session_list")
            .exact_size(LIST_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("session_rows").auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(LIST_WIDTH - 16.0);
                    for (index, session) in list.iter().enumerate() {
                        if session_row(ui, session, &names, index == selected) {
                            picked = Some(session.started_at.clone());
                        }
                    }
                });
            });
        if let Some(id) = picked {
            self.selected = Some(id);
            self.confirm_delete = false;
        }

        let session = &list[selected];
        let mut delete = false;
        egui::ScrollArea::vertical().id_salt("session_detail").auto_shrink([false, false]).show(ui, |ui| {
            detail(ui, session, &list, &names, &mut self.confirm_delete, &mut delete, &mut self.goto);
        });
        if delete {
            match session_recorder::delete(&session_recorder::sessions_dir(), session) {
                Ok(()) => notices.push(Notice { text: "Session deleted.".into(), ok: true }),
                Err(err) => notices.push(Notice { text: format!("Could not delete the session: {err}"), ok: false }),
            }
            self.selected = None;
            reload = true;
        }
        if reload {
            self.reload(ui.ctx());
        }
    }
}

/// `2026-11-01T14:34:14` as "1 Nov 2026, 14:34".
fn pretty_date(stamp: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%dT%H:%M:%S")
        .map_or_else(|_| stamp.replace('T', " "), |t| t.format("%-d %b %Y, %H:%M").to_string())
}

fn track_title(s: &RecordedSession, names: &Names) -> String {
    let track = names.track(&s.track_id);
    if s.track_layout.is_empty() { track } else { format!("{track}, {}", names.layout(&s.track_layout)) }
}

fn clock(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match (seconds < 60.0, minutes / 60) {
        (true, _) => format!("{seconds:.0} s"),
        (_, 0) => format!("{minutes} min"),
        (_, hours) => format!("{hours} h {} min", minutes % 60),
    }
}

/// The session's best real lap.
fn best_lap(s: &RecordedSession) -> Option<i32> {
    let timed = s.laps.iter().map(|lap| lap.time_ms).filter(|ms| *ms >= MIN_LAP_MS).min();
    timed.or(s.best_lap_ms.filter(|ms| *ms >= MIN_LAP_MS))
}

/// One row of the list. Returns true when clicked.
fn session_row(ui: &mut egui::Ui, s: &RecordedSession, names: &Names, selected: bool) -> bool {
    let response = egui::Frame::new()
        .fill(if selected { theme::CARD_SELECTED } else { theme::CARD })
        .stroke(egui::Stroke::new(1.0, if selected { theme::ACCENT.gamma_multiply(0.6) } else { theme::BORDER }))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_SMALL))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(egui::Label::new(RichText::new(names.car(&s.car_id)).font(theme::bold(14.0))).truncate());
            ui.add(egui::Label::new(RichText::new(track_title(s, names)).size(12.5).color(theme::MUTED)).truncate());
            ui.horizontal(|ui| {
                ui.label(RichText::new(pretty_date(&s.started_at)).size(11.5).color(theme::FAINT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(clock(s.seconds)).size(11.5).color(theme::FAINT));
                });
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.add_space(6.0);
    response.clicked()
}

fn stat_tile(ui: &mut egui::Ui, label: &str, value: &str) {
    theme::card().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(label).size(11.5).color(theme::MUTED));
        ui.add(egui::Label::new(RichText::new(value).font(theme::bold(18.0))).truncate());
    });
}

fn section_card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(theme::bold(14.0)));
        ui.add_space(8.0);
        add(ui);
    });
}

fn detail(
    ui: &mut egui::Ui,
    s: &RecordedSession,
    all: &[RecordedSession],
    names: &Names,
    confirm: &mut bool,
    delete: &mut bool,
    goto: &mut Option<&'static str>,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(names.car(&s.car_id)).font(theme::bold(22.0)));
            let mut line = vec![track_title(s, names), pretty_date(&s.started_at)];
            if !s.session_type.is_empty() {
                line.push(s.session_type.clone());
            }
            ui.label(RichText::new(line.join("  \u{00B7}  ")).color(theme::MUTED));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if *confirm {
                if ui.button("Keep").clicked() {
                    *confirm = false;
                }
                if ui.button(RichText::new("Delete for good").color(theme::DANGER)).clicked() {
                    *confirm = false;
                    *delete = true;
                }
            } else if ui.button("Delete").clicked() {
                *confirm = true;
            }
        });
    });
    ui.add_space(12.0);

    let best = best_lap(s).map_or("-".to_string(), |ms| format_lap(ms as u32));
    let hits = if s.wall_hits + s.car_hits > 0 { format!("{} walls, {} cars", s.wall_hits, s.car_hits) } else { "-".to_string() };
    let tiles = [
        ("Drive time", clock(s.seconds)),
        ("Distance", format!("{:.1} km", s.distance_m / 1000.0)),
        ("Top speed", format!("{:.0} km/h", s.max_speed)),
        ("Best lap", best),
        ("Laps", s.laps.iter().filter(|lap| lap.time_ms >= MIN_LAP_MS).count().to_string()),
        ("Crashes", s.crashes.to_string()),
        ("Off track", s.offroad.to_string()),
        ("Jumps", s.jumps.to_string()),
        ("Lock-ups", (s.front_lockups + s.rear_lockups).to_string()),
        ("Wheelspin", s.wheelspins.to_string()),
        ("Longest jump", format!("{:.1} s", s.longest_airborne)),
        ("Hits", hits),
    ];
    widgets::tile_grid(ui, 150.0, tiles.len(), |ui, i| stat_tile(ui, tiles[i].0, &tiles[i].1));
    ui.add_space(12.0);

    let has_map = s.path.len() >= 20 || s.trace.len() >= 20;
    ui.columns(2, |cols| {
        if has_map {
            section_card(&mut cols[0], "Track", |ui| {
                track_map(ui, s, ui.available_width().min(520.0));
                legend(ui, s);
            });
        } else {
            section_card(&mut cols[0], "Track", |ui| {
                ui.label(RichText::new("This session was recorded before ACXS saved the driven path.").color(theme::MUTED));
            });
        }
        section_card(&mut cols[1], "Handling in hard corners (estimate)", |ui| balance_bar(ui, s));
        cols[1].add_space(10.0);
        section_card(&mut cols[1], "Trouble spots", |ui| trouble_spots(ui, s));
    });
    ui.add_space(12.0);

    if s.laps.iter().any(|lap| lap.time_ms >= MIN_LAP_MS) {
        section_card(ui, "Laps", |ui| lap_table(ui, s));
        ui.add_space(12.0);
    }

    let same_car: Vec<RecordedSession> = all.iter().filter(|o| o.car_id == s.car_id).cloned().collect();
    let advice = driving_advice::advise(&same_car);
    if !advice.is_empty() {
        section_card(ui, "Suggestions for this car", |ui| {
            for item in &advice {
                ui.label(RichText::new(item.finding.as_str()).size(13.0));
                ui.label(RichText::new(item.suggestion.as_str()).size(12.5).color(theme::MUTED));
                ui.add_space(6.0);
            }
            if ui.button("Open Setups").clicked() {
                *goto = Some("Setups");
            }
        });
    }
}

/// How the car handled in hard corners: one bar split into understeer, neutral and oversteer.
fn balance_bar(ui: &mut egui::Ui, s: &RecordedSession) {
    if s.cornering_seconds < 3.0 {
        ui.label(RichText::new("This session had too little steady cornering to measure.").color(theme::MUTED));
        return;
    }
    let under = (s.understeer_seconds / s.cornering_seconds).clamp(0.0, 1.0) as f32;
    let over = (s.oversteer_seconds / s.cornering_seconds).clamp(0.0, 1.0) as f32;
    let neutral = (1.0 - under - over).max(0.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
    let painter = ui.painter();
    let mut x = rect.left();
    for (share, color) in [
        (under, kind_color(EventKind::Understeer)),
        (neutral, theme::FAINT),
        (over, kind_color(EventKind::Oversteer)),
    ] {
        let width = share * rect.width();
        if width > 0.5 {
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, rect.height())), egui::CornerRadius::same(2), color);
            x += width;
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        for (label, share) in [("Understeer", under), ("Neutral", neutral), ("Oversteer", over)] {
            ui.label(RichText::new(format!("{label} {:.0}%", share * 100.0)).size(12.5).color(theme::MUTED));
            ui.add_space(14.0);
        }
    });
}

fn trouble_spots(ui: &mut egui::Ui, s: &RecordedSession) {
    let spots = s.worst_spots(6);
    if spots.is_empty() {
        ui.label(RichText::new("A clean drive.").color(theme::MUTED));
        return;
    }
    let placed = s.path.len() >= 20;
    egui::Grid::new(("session-spots", &s.started_at)).num_columns(3).spacing([14.0, 6.0]).show(ui, |ui| {
        for spot in spots {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.5, kind_color(spot.kind));
            let amount = if spot.kind.is_timed() {
                format!("{:.1} s", f64::from(spot.count) / 10.0)
            } else {
                format!("{}\u{00D7}", spot.count)
            };
            let mut label = spot.kind.label().to_string();
            if let Some(first) = label.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            ui.label(RichText::new(label).size(13.0));
            let place = if !placed {
                String::new()
            } else if spot.end - spot.start > 0.015 {
                format!("  at {:.0}\u{2013}{:.0}%", spot.start * 100.0, spot.end * 100.0)
            } else {
                format!("  at {:.0}%", spot.start * 100.0)
            };
            ui.label(RichText::new(format!("{amount}{place}")).size(12.5).color(theme::MUTED));
            ui.end_row();
        }
    });
}

/// Every real lap with its gap to the session's best.
fn lap_table(ui: &mut egui::Ui, s: &RecordedSession) {
    let laps: Vec<_> = s.laps.iter().filter(|lap| lap.time_ms >= MIN_LAP_MS).collect();
    let best = laps.iter().map(|lap| lap.time_ms).min().unwrap_or(0);
    egui::Grid::new(("session-lap-rows", &s.started_at)).num_columns(3).spacing([32.0, 4.0]).show(ui, |ui| {
        for lap in laps {
            ui.label(RichText::new(format!("Lap {}", lap.number)).size(13.0).color(theme::MUTED));
            let text = RichText::new(format_lap(lap.time_ms as u32)).size(13.0);
            if lap.time_ms == best {
                ui.label(text.color(theme::OK));
                ui.label(RichText::new("best").size(12.5).color(theme::OK));
            } else {
                ui.label(text);
                ui.label(RichText::new(format!("+{:.3}", f64::from(lap.time_ms - best) / 1000.0)).size(12.5).color(theme::MUTED));
            }
            ui.end_row();
        }
    });
}

/// The kinds that appear on the map.
fn legend(ui: &mut egui::Ui, s: &RecordedSession) {
    let mut kinds: Vec<EventKind> = if s.path.len() >= 20 {
        s.hotspots.iter().map(|h| h.kind).collect()
    } else {
        s.map_points(40).iter().map(|p| p.kind).collect()
    };
    kinds.sort();
    kinds.dedup();
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 4.0);
        for kind in kinds {
            // One group, so a wrapped line never parts a dot from its name.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, kind_color(kind));
                ui.label(RichText::new(kind.label()).size(11.5).color(theme::MUTED));
            });
        }
    });
}

/// Colour of an event kind on the map.
fn kind_color(kind: EventKind) -> egui::Color32 {
    match kind {
        EventKind::Crash => egui::Color32::from_rgb(255, 92, 108),
        EventKind::OffTrack => egui::Color32::from_rgb(245, 160, 66),
        EventKind::FrontLockup | EventKind::RearLockup => egui::Color32::from_rgb(245, 215, 70),
        EventKind::Wheelspin => egui::Color32::from_rgb(61, 214, 140),
        EventKind::Jump => egui::Color32::from_rgb(110, 205, 240),
        EventKind::Understeer => egui::Color32::from_rgb(91, 140, 255),
        EventKind::Oversteer => egui::Color32::from_rgb(200, 110, 240),
        EventKind::WallHit => egui::Color32::from_rgb(240, 240, 245),
        EventKind::CarHit => egui::Color32::from_rgb(255, 120, 175),
    }
}

fn amount_text(kind: EventKind, count: u32) -> String {
    if kind.is_timed() { format!("{:.1} s", f64::from(count) / 10.0) } else { format!("{count}\u{00D7}") }
}

/// The track outline with a dot for every trouble spot; hovering a dot names it.
///
/// Tracks with a lap position (the game's spline) are drawn from 100 slices of the lap. Tracks
/// without one, such as drift pads and freeroam maps, are drawn from the driven trace and the
/// events keep their own coordinates.
fn track_map(ui: &mut egui::Ui, s: &RecordedSession, size: f32) {
    // World points: first the line, then the dots. They are projected together so both share one scale.
    let sliced = s.path.len() >= 20;
    let mut world: Vec<[f32; 2]> = Vec::new();
    let mut breaks: Vec<bool> = Vec::new();
    let mut dots_meta: Vec<(EventKind, String)> = Vec::new();
    if sliced {
        let mut last: Option<u8> = None;
        for &(segment, x, z) in &s.path {
            breaks.push(last.is_none_or(|l| segment.saturating_sub(l) > 2));
            world.push([x, z]);
            last = Some(segment);
        }
    } else {
        breaks = session_recorder::trace_breaks(&s.trace);
        world.extend(s.trace.iter().map(|&(x, z)| [x, z]));
    }
    let line_len = world.len();
    if sliced {
        for spot in s.worst_spots(40) {
            let mid = ((spot.start + spot.end) * 50.0) as u8;
            let Some(i) = s.path.iter().position(|&(seg, _, _)| seg >= mid.saturating_sub(1)) else { continue };
            world.push(world[i]);
            dots_meta.push((
                spot.kind,
                format!("{} {} at {:.0}% of the lap", spot.kind.label(), amount_text(spot.kind, spot.count), spot.start * 100.0),
            ));
        }
    } else {
        for p in s.map_points(40) {
            world.push([p.x, p.z]);
            dots_meta.push((p.kind, format!("{} {}", p.kind.label(), amount_text(p.kind, p.count))));
        }
    }
    let unit = project_lap(&world);
    if unit.len() != world.len() || line_len < 2 {
        return;
    }

    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size * 0.8), egui::Sense::hover());
    let pad = 24.0;
    let side = (rect.width().min(rect.height()) - 2.0 * pad).max(10.0);
    let origin = rect.center() - egui::vec2(side, side) / 2.0;
    let to_screen = |p: [f32; 2]| egui::pos2(origin.x + p[0] * side, origin.y + p[1] * side);
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(theme::RADIUS_SMALL), theme::BG);

    let mut runs: Vec<Vec<egui::Pos2>> = Vec::new();
    for (point, &new_run) in unit[..line_len].iter().zip(&breaks) {
        if new_run {
            runs.push(Vec::new());
        }
        if let Some(run) = runs.last_mut() {
            run.push(to_screen(*point));
        }
    }
    // A full lap closes the loop.
    if sliced && runs.len() == 1 && s.path.len() >= 90 {
        if let Some(first) = runs[0].first().copied() {
            runs[0].push(first);
        }
    }
    for run in runs.iter().filter(|r| r.len() > 1) {
        painter.add(egui::Shape::line(run.clone(), egui::Stroke::new(9.0, theme::BORDER)));
    }
    for run in runs.iter().filter(|r| r.len() > 1) {
        painter.add(egui::Shape::line(run.clone(), egui::Stroke::new(3.0, theme::BORDER_STRONG)));
    }
    // Start marker.
    if !sliced || s.path.first().is_some_and(|&(seg, _, _)| seg <= 2) {
        let p = to_screen(unit[0]);
        painter.circle_filled(p, 5.0, egui::Color32::WHITE);
        painter.circle_stroke(p, 5.0, egui::Stroke::new(1.5, theme::BG));
    }

    let hover = response.hover_pos();
    let mut tip: Option<String> = None;
    for (point, (kind, text)) in unit[line_len..].iter().zip(&dots_meta) {
        let p = to_screen(*point);
        let color = kind_color(*kind);
        painter.circle_filled(p, 8.0, color.gamma_multiply(0.25));
        painter.circle_filled(p, 4.5, color);
        painter.circle_stroke(p, 4.5, egui::Stroke::new(1.0, theme::BG));
        if hover.is_some_and(|h| h.distance(p) < 10.0) {
            tip = Some(text.clone());
        }
    }
    if let Some(text) = tip {
        response.on_hover_text(text);
    }
}
