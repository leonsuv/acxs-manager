//! Sessions tool page: the sessions ACXS recorded, newest first, with what happened in each.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::collision_app;
use acxs_manager::driving_advice;
use acxs_manager::laptimes::format_lap;
use acxs_manager::session_recorder::{self, EventKind, RecordedSession};
use acxs_manager::track_map::project_lap;
use eframe::egui::{self, RichText};
use std::sync::{Arc, Mutex};

pub fn tool() -> Box<dyn Tool> {
    Box::new(Sessions { sessions: Arc::new(Mutex::new(None)), loading: false, goto: None, last_seen: None, seen_saved: 0 })
}

struct Sessions {
    /// Newest first; `None` until the first load finishes.
    sessions: Arc<Mutex<Option<Vec<RecordedSession>>>>,
    loading: bool,
    goto: Option<&'static str>,
    /// When the page was last drawn; coming back after a pause reloads the list.
    last_seen: Option<std::time::Instant>,
    seen_saved: usize,
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
}


impl Tool for Sessions {
    fn name(&self) -> &'static str {
        "Sessions"
    }

    fn group(&self) -> &'static str {
        "Telemetry"
    }

    fn take_goto(&mut self) -> Option<&'static str> {
        self.goto.take()
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        widgets::heading(ui, "Sessions", "Sessions recorded by ACXS while Assetto Corsa was running.");
        let fresh = ctx.sessions_saved != self.seen_saved;
        self.seen_saved = ctx.sessions_saved;
        let away = fresh || self.last_seen.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(1));
        self.last_seen = Some(std::time::Instant::now());
        let ready = self.sessions.lock().ok().is_some_and(|g| g.is_some());
        if (!ready || away) && !self.loading {
            self.reload(ui.ctx());
        }
        ui.horizontal(|ui| {
            if ui.button("Reload").clicked() {
                self.reload(ui.ctx());
            }
            if ui
                .button("Keep more game replays")
                .on_hover_text("Raises the autosave limits in the game's replay.ini (the old file is backed up).")
                .clicked()
            {
                match session_recorder::keep_more_replays() {
                    Ok(path) => notices.push(Notice { text: format!("Updated {}", path.display()), ok: true }),
                    Err(err) => notices.push(Notice { text: format!("Could not update replay.ini: {err}"), ok: false }),
                }
            }
            if let Some(root) = &ctx.ac_root {
                let installed = collision_app::is_installed(root);
                let label = if installed { "Reinstall collision app" } else { "Install collision app" };
                let response = ui.add_enabled(!ctx.game_running, egui::Button::new(label)).on_hover_text(
                    "A small CSP Lua app that tells wall hits from car hits. After installing, switch on \"ACXS Collisions\" once in the in-game app bar. The game must be closed.",
                );
                if response.clicked() {
                    match collision_app::install(root, ctx.game_running) {
                        Ok(_) => notices.push(Notice { text: "Installed ACXS Collisions. Enable it once in the in-game app bar.".into(), ok: true }),
                        Err(err) => notices.push(Notice { text: format!("Could not install the collision app: {err}"), ok: false }),
                    }
                }
            }
            if ui.button("Open folder").clicked() {
                let dir = session_recorder::sessions_dir();
                let _ = std::fs::create_dir_all(&dir);
                crate::ui::pages::open_folder(&dir);
            }
        });
        ui.add_space(8.0);
        let guard = self.sessions.lock().ok();
        let Some(list) = guard.as_deref().and_then(|g| g.as_ref()) else {
            ui.spinner();
            return;
        };
        self.loading = false;
        if list.is_empty() {
            widgets::empty_state(ui, "Nothing recorded yet. Drive a session with ACXS Manager open and it shows up here.");
            return;
        }
        let advice = driving_advice::advise(list);
        if !advice.is_empty() {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Suggestions").font(theme::bold(14.5)));
                    if ui.small_button("Open Setups").clicked() {
                        self.goto = Some("Setups");
                    }
                });
                for a in &advice {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("{}: {}", a.car_id, a.finding)).size(13.0));
                    ui.label(RichText::new(&a.suggestion).size(12.5).color(theme::MUTED));
                }
            });
            ui.add_space(8.0);
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for s in list {
                session_card(ui, s);
                ui.add_space(10.0);
            }
        });
    }
}

/// `ks_porsche_911_gt3_r_2016` as "Porsche 911 Gt3 R 2016": readable when the car is not installed.
fn pretty_id(id: &str) -> String {
    let id = id.strip_prefix("ks_").unwrap_or(id);
    id.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `2026-11-01T14:34:14` as "1 Nov 2026, 14:34".
fn pretty_date(stamp: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%dT%H:%M:%S")
        .map_or_else(|_| stamp.replace('T', " "), |t| t.format("%-d %b %Y, %H:%M").to_string())
}

fn session_card(ui: &mut egui::Ui, s: &RecordedSession) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            if s.path.len() >= 20 {
                ui.vertical(|ui| {
                    track_map(ui, s);
                    legend(ui, s);
                });
                ui.add_space(14.0);
            }
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(pretty_id(&s.car_id)).font(theme::bold(18.0)));
                    widgets::pill(ui, &pretty_id(&s.full_track_id()), theme::MUTED);
                });
                ui.label(RichText::new(pretty_date(&s.started_at)).size(12.5).color(theme::MUTED));
                ui.add_space(10.0);
                stat_grid(ui, s);
                ui.add_space(10.0);
                balance_bar(ui, s);
                trouble_spots(ui, s);
            });
        });
    });
}

fn stat_grid(ui: &mut egui::Ui, s: &RecordedSession) {
    let best = s.best_lap_ms.filter(|ms| *ms > 0).map_or("-".to_string(), |ms| format_lap(ms as u32));
    let minutes = s.seconds / 60.0;
    let mut stats = vec![
        ("Time", if minutes >= 1.0 { format!("{minutes:.0} min") } else { format!("{:.0} s", s.seconds) }),
        ("Distance", format!("{:.1} km", s.distance_m / 1000.0)),
        ("Top speed", format!("{:.0} km/h", s.max_speed)),
        ("Laps", if s.laps.is_empty() { "0".to_string() } else { format!("{}  \u{00B7}  best {best}", s.laps.len()) }),
        ("Crashes", s.crashes.to_string()),
        ("Off track", s.offroad.to_string()),
        ("Jumps", if s.jumps > 0 { format!("{} (longest {:.1} s)", s.jumps, s.longest_airborne) } else { "0".to_string() }),
        ("Lock-ups", format!("{} front, {} rear", s.front_lockups, s.rear_lockups)),
        ("Wheelspin", s.wheelspins.to_string()),
    ];
    if s.wall_hits + s.car_hits > 0 {
        stats.push(("Hits", format!("{} wall, {} car", s.wall_hits, s.car_hits)));
    }
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    egui::Grid::new(("session-stats", &s.started_at)).num_columns(3).min_col_width(120.0).spacing([28.0, 12.0]).show(ui, |ui| {
        for (i, (label, value)) in stats.iter().enumerate() {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(RichText::new(*label).size(11.5).color(theme::MUTED));
                ui.label(RichText::new(value).font(theme::bold(15.0)));
            });
            if (i + 1) % 3 == 0 {
                ui.end_row();
            }
        }
    });
}

/// How the car handled in hard corners: one bar split into understeer, neutral and oversteer.
fn balance_bar(ui: &mut egui::Ui, s: &RecordedSession) {
    if s.cornering_seconds < 3.0 {
        return;
    }
    let under = (s.understeer_seconds / s.cornering_seconds).clamp(0.0, 1.0) as f32;
    let over = (s.oversteer_seconds / s.cornering_seconds).clamp(0.0, 1.0) as f32;
    let neutral = (1.0 - under - over).max(0.0);
    ui.label(RichText::new("Handling in hard corners").size(11.5).color(theme::MUTED));
    let width = ui.available_width().min(420.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 10.0), egui::Sense::hover());
    let painter = ui.painter();
    let round = egui::CornerRadius::same(5);
    painter.rect_filled(rect, round, theme::BORDER);
    let mut x = rect.left();
    for (share, color) in [
        (under, kind_color(EventKind::Understeer)),
        (neutral, theme::FAINT),
        (over, kind_color(EventKind::Oversteer)),
    ] {
        let w = share * rect.width();
        if w > 0.5 {
            let part = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, rect.height()));
            painter.rect_filled(part, egui::CornerRadius::ZERO, color);
            x += w;
        }
    }
    painter.rect_stroke(rect, round, egui::Stroke::new(1.5, theme::CARD), egui::StrokeKind::Inside);
    ui.add_space(3.0);
    ui.label(
        RichText::new(format!(
            "Understeer {:.0}%   \u{00B7}   Neutral {:.0}%   \u{00B7}   Oversteer {:.0}%",
            under * 100.0,
            neutral * 100.0,
            over * 100.0
        ))
        .size(12.0)
        .color(theme::MUTED),
    );
    ui.add_space(8.0);
}

fn trouble_spots(ui: &mut egui::Ui, s: &RecordedSession) {
    let spots = s.worst_spots(5);
    if spots.is_empty() {
        return;
    }
    ui.label(RichText::new("Trouble spots").size(11.5).color(theme::MUTED));
    for spot in spots {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.5, kind_color(spot.kind));
            let amount = if spot.kind.is_timed() {
                format!("{:.1} s", f64::from(spot.count) / 10.0)
            } else {
                format!("{}\u{00D7}", spot.count)
            };
            let place = if spot.end - spot.start > 0.015 {
                format!("{:.0}\u{2013}{:.0}%", spot.start * 100.0, spot.end * 100.0)
            } else {
                format!("{:.0}%", spot.start * 100.0)
            };
            let mut label = spot.kind.label().to_string();
            if let Some(first) = label.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            ui.label(RichText::new(format!("{label}  {amount}")).font(theme::bold(13.0)));
            ui.label(RichText::new(format!("at {place} of the lap")).size(12.5).color(theme::MUTED));
        });
    }
}

/// The kinds that appear in this session, as coloured chips under the map.
fn legend(ui: &mut egui::Ui, s: &RecordedSession) {
    let mut kinds: Vec<EventKind> = s.hotspots.iter().map(|h| h.kind).collect();
    kinds.sort();
    kinds.dedup();
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(MAP_SIZE);
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
        for kind in kinds {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, kind_color(kind));
            ui.label(RichText::new(kind.label()).size(11.5).color(theme::MUTED));
            ui.add_space(4.0);
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

const MAP_SIZE: f32 = 250.0;

/// The track outline from the recorded path with a dot for every trouble spot. Hovering a dot
/// names it.
fn track_map(ui: &mut egui::Ui, s: &RecordedSession) {
    let world: Vec<[f32; 2]> = s.path.iter().map(|&(_, x, z)| [x, z]).collect();
    let unit = project_lap(&world);
    if unit.len() != s.path.len() {
        return;
    }
    let (rect, response) = ui.allocate_exact_size(egui::vec2(MAP_SIZE, MAP_SIZE), egui::Sense::hover());
    let pad = 22.0;
    let to_screen = |p: [f32; 2]| {
        egui::pos2(rect.left() + pad + p[0] * (MAP_SIZE - 2.0 * pad), rect.top() + pad + p[1] * (MAP_SIZE - 2.0 * pad))
    };
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(theme::RADIUS_SMALL), theme::BG);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(theme::RADIUS_SMALL),
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );

    // Runs of consecutive slices; a gap in the data ends a run so no false shortcut is drawn.
    let mut runs: Vec<Vec<egui::Pos2>> = Vec::new();
    let mut last_segment: Option<u8> = None;
    for (point, &(segment, _, _)) in unit.iter().zip(&s.path) {
        if last_segment.is_none_or(|l| segment.saturating_sub(l) > 2) {
            runs.push(Vec::new());
        }
        if let Some(run) = runs.last_mut() {
            run.push(to_screen(*point));
        }
        last_segment = Some(segment);
    }
    // A full lap closes the loop.
    if runs.len() == 1 && s.path.len() >= 90 {
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
    // Start and finish.
    if s.path.first().is_some_and(|&(seg, _, _)| seg <= 2) {
        let p = to_screen(unit[0]);
        painter.circle_filled(p, 5.0, egui::Color32::WHITE);
        painter.circle_stroke(p, 5.0, egui::Stroke::new(1.5, theme::BG));
    }

    let hover = response.hover_pos();
    let mut tip: Option<String> = None;
    for spot in s.worst_spots(40) {
        let mid = ((spot.start + spot.end) * 50.0) as u8;
        let Some(i) = s.path.iter().position(|&(seg, _, _)| seg >= mid.saturating_sub(1)) else { continue };
        let p = to_screen(unit[i]);
        let color = kind_color(spot.kind);
        painter.circle_filled(p, 8.0, color.gamma_multiply(0.25));
        painter.circle_filled(p, 4.5, color);
        painter.circle_stroke(p, 4.5, egui::Stroke::new(1.0, theme::BG));
        if hover.is_some_and(|h| h.distance(p) < 10.0) {
            let amount = if spot.kind.is_timed() {
                format!("{:.1} s", f64::from(spot.count) / 10.0)
            } else {
                format!("{}\u{00D7}", spot.count)
            };
            tip = Some(format!("{} {amount} at {:.0}% of the lap", spot.kind.label(), spot.start * 100.0));
        }
    }
    if let Some(text) = tip {
        response.on_hover_text(text);
    }
}
