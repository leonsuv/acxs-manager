//! Sessions tool page: the sessions ACXS recorded, newest first, with what happened in each.

use super::{Tool, ToolContext};
use crate::ui::{theme, widgets, Notice};
use acxs_manager::driving_advice;
use acxs_manager::laptimes::format_lap;
use acxs_manager::session_recorder::{self, EventKind, RecordedSession};
use acxs_manager::track_map::project_lap;
use eframe::egui::{self, RichText};
use std::sync::{Arc, Mutex};

pub fn tool() -> Box<dyn Tool> {
    Box::new(Sessions { sessions: Arc::new(Mutex::new(None)), loading: false, goto: None })
}

struct Sessions {
    /// Newest first; `None` until the first load finishes.
    sessions: Arc<Mutex<Option<Vec<RecordedSession>>>>,
    loading: bool,
    goto: Option<&'static str>,
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

fn percent(part: f64, whole: f64) -> String {
    if whole < 10.0 { "-".to_string() } else { format!("{:.0}%", part / whole * 100.0) }
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

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        widgets::heading(ui, "Sessions", "Sessions recorded by ACXS while Assetto Corsa was running.");
        let ready = self.sessions.lock().ok().is_some_and(|g| g.is_some());
        if !ready && !self.loading {
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
                theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&s.car_id).font(theme::bold(14.5)));
                        ui.label(RichText::new(s.full_track_id()).color(theme::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(s.started_at.replace('T', "  ")).color(theme::MUTED));
                        });
                    });
                    ui.add_space(4.0);
                    let best = s.best_lap_ms.filter(|ms| *ms > 0).map_or("-".to_string(), |ms| format_lap(ms as u32));
                    let facts = [
                        format!("{:.0} min", s.seconds / 60.0),
                        format!("{:.1} km", s.distance_m / 1000.0),
                        format!("top {:.0} km/h", s.max_speed),
                        format!("{} laps, best {best}", s.laps.len()),
                        format!("{} crashes", s.crashes),
                        format!("{} off track", s.offroad),
                        format!("{} jumps (longest {:.1} s)", s.jumps, s.longest_airborne),
                        format!("lock-ups {} front / {} rear", s.front_lockups, s.rear_lockups),
                        format!("{} wheelspins", s.wheelspins),
                        format!(
                            "understeer {} / oversteer {}",
                            percent(s.understeer_seconds, s.cornering_seconds),
                            percent(s.oversteer_seconds, s.cornering_seconds)
                        ),
                    ];
                    ui.label(RichText::new(facts.join("   ·   ")).size(12.5));
                    if s.path.len() >= 20 {
                        ui.add_space(6.0);
                        track_map(ui, s);
                    }
                    let spots = s.worst_spots(5);
                    if !spots.is_empty() {
                        ui.add_space(4.0);
                        let text: Vec<String> = spots
                            .iter()
                            .map(|(pos, kind, count)| {
                                let amount = match kind {
                                    EventKind::Understeer | EventKind::Oversteer => format!("{:.1} s", f64::from(*count) / 10.0),
                                    _ => count.to_string(),
                                };
                                format!("{:.0}% of the lap: {amount} {}", pos * 100.0, kind.label())
                            })
                            .collect();
                        ui.label(RichText::new(format!("Trouble spots: {}", text.join("   ·   "))).size(12.0).color(theme::MUTED));
                    }
                });
                ui.add_space(6.0);
            }
        });
    }
}

/// Colour of an event kind on the map.
fn kind_color(kind: EventKind) -> egui::Color32 {
    match kind {
        EventKind::Crash => egui::Color32::from_rgb(235, 70, 70),
        EventKind::OffTrack => egui::Color32::from_rgb(235, 150, 60),
        EventKind::FrontLockup | EventKind::RearLockup => egui::Color32::from_rgb(240, 215, 70),
        EventKind::Wheelspin => egui::Color32::from_rgb(120, 200, 120),
        EventKind::Jump => egui::Color32::from_rgb(110, 200, 235),
        EventKind::Understeer => egui::Color32::from_rgb(90, 130, 245),
        EventKind::Oversteer => egui::Color32::from_rgb(200, 100, 235),
    }
}

/// The track outline from the recorded path with one dot per trouble spot.
fn track_map(ui: &mut egui::Ui, s: &RecordedSession) {
    const SIZE: f32 = 190.0;
    let world: Vec<[f32; 2]> = s.path.iter().map(|&(_, x, z)| [x, z]).collect();
    let unit = project_lap(&world);
    if unit.len() != s.path.len() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
    let to_screen = |p: [f32; 2]| egui::pos2(rect.left() + 10.0 + p[0] * (SIZE - 20.0), rect.top() + 10.0 + p[1] * (SIZE - 20.0));
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(theme::RADIUS), theme::BG);
    // Break the line where slices are missing so gaps do not draw false shortcuts.
    let mut run: Vec<egui::Pos2> = Vec::new();
    let mut last_segment: Option<u8> = None;
    let stroke = egui::Stroke::new(2.0, theme::FAINT);
    for (point, &(segment, _, _)) in unit.iter().zip(&s.path) {
        if last_segment.is_some_and(|l| segment.saturating_sub(l) > 2) && run.len() > 1 {
            painter.add(egui::Shape::line(std::mem::take(&mut run), stroke));
        }
        run.push(to_screen(*point));
        last_segment = Some(segment);
    }
    if run.len() > 1 {
        painter.add(egui::Shape::line(run, stroke));
    }
    for h in &s.hotspots {
        if let Some(i) = s.path.iter().position(|&(seg, _, _)| seg == h.segment) {
            painter.circle_filled(to_screen(unit[i]), 4.0, kind_color(h.kind));
        }
    }
}
