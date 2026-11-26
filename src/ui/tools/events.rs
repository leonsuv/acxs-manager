//! Events tool page: the game's Special Events and Career races, ready to start.

use super::{Tool, ToolContext};
use crate::ui::{media, theme, widgets, Notice};
use acxs_manager::events::{self, EventProgressStore, GameEvent, Medal};
use acxs_manager::{game, race, race_out, settings};
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

/// Rows drawn at once; the search narrows the rest down.
const MAX_ROWS: usize = 120;

fn progress_store_path() -> PathBuf {
    settings::settings_path()
        .parent()
        .map_or_else(|| PathBuf::from("event-progress.json"), |d| d.join("event-progress.json"))
}

fn medal_color(medal: Medal) -> Color32 {
    match medal {
        Medal::Gold => Color32::from_rgb(0xFF, 0xC8, 0x3D),
        Medal::Silver => Color32::from_rgb(0xC0, 0xCA, 0xD8),
        Medal::Bronze => Color32::from_rgb(0xCD, 0x7F, 0x32),
        Medal::None => theme::MUTED,
    }
}

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(Events {
        tx,
        rx,
        root: None,
        events: None,
        medals: BTreeMap::new(),
        last_launched: None,
        query: String::new(),
        kind: "All".to_string(),
        career: false,
        playable_only: true,
        busy: false,
    })
}

enum Msg {
    Scanned(Vec<(GameEvent, bool)>, BTreeMap<String, Medal>),
    Started(Notice, GameEvent),
}

struct Events {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    root: Option<PathBuf>,
    /// Each event with whether its car and track are installed.
    events: Option<Vec<(GameEvent, bool)>>,
    medals: BTreeMap<String, Medal>,
    last_launched: Option<GameEvent>,
    query: String,
    kind: String,
    career: bool,
    playable_only: bool,
    busy: bool,
}

fn load_medals() -> BTreeMap<String, Medal> {
    let mut merged = settings::ac_user_cfg_dir()
        .map(|cfg| events::read_progress_ini(&cfg))
        .unwrap_or_default();
    let store = EventProgressStore::load(&progress_store_path());
    for (id, medal) in store.medals {
        let entry = merged.entry(id).or_default();
        if medal > *entry {
            *entry = medal;
        }
    }
    merged
}

fn scan(root: &Path) -> Vec<(GameEvent, bool)> {
    events::special_events(root)
        .into_iter()
        .chain(events::career_events(root))
        .map(|event| {
            let playable = event.is_playable(root);
            (event, playable)
        })
        .collect()
}

/// Writes the event as `race.ini` and starts the game.
fn start(root: &Path, event: &GameEvent) -> Notice {
    let result = (|| -> Result<(), String> {
        if game::is_game_running() {
            return Err("Assetto Corsa is already running. Close it before starting an event.".into());
        }
        let cfg = settings::ac_user_cfg_dir().ok_or("The Documents folder was not found.")?;
        let bytes = std::fs::read(&event.ini).map_err(|e| e.to_string())?;
        let driver = std::fs::read(cfg.join("race.ini"))
            .ok()
            .map(|old| String::from_utf8_lossy(&old).into_owned())
            .and_then(|old| {
                old.lines().find_map(|l| l.trim().strip_prefix("DRIVER_NAME=").map(|n| n.trim().to_string()))
            })
            .unwrap_or_default();
        let text = events::to_race_ini(&String::from_utf8_lossy(&bytes), &driver);
        race::write_event_ini(&cfg, &text).map_err(|e| format!("Could not write race.ini: {e}"))?;
        game::launch_game(root).map_err(|e| format!("Assetto Corsa did not start: {e}"))?;
        Ok(())
    })();
    match result {
        Ok(()) => Notice { text: format!("Starting {}.", event.name), ok: true },
        Err(text) => Notice { text, ok: false },
    }
}

impl Tool for Events {
    fn name(&self) -> &'static str {
        "Events"
    }

    fn content_width(&self) -> f32 {
        1200.0
    }

    fn group(&self) -> &'static str {
        "Launch"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        widgets::heading(ui, "Events", "The game's Special Events and Career races. Pick one and drive.");
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Msg::Scanned(list, medals) => {
                    self.events = Some(list);
                    self.medals = medals;
                }
                Msg::Started(notice, ev) => {
                    self.busy = false;
                    if notice.ok {
                        self.last_launched = Some(ev);
                    }
                    notices.push(notice);
                }
            }
        }
        let Some(root) = ctx.ac_root.clone().filter(|_| ctx.ac_valid) else {
            widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to see its events.");
            return;
        };
        if self.root.as_ref() != Some(&root) {
            self.root = Some(root.clone());
            self.events = None;
            let tx = self.tx.clone();
            let repaint = ui.ctx().clone();
            let scan_root = root.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Scanned(scan(&scan_root), load_medals()));
                repaint.request_repaint();
            });
        }
        let Some(list) = &self.events else {
            ui.spinner();
            return;
        };

        let names = super::names::names(&root, ui.ctx());
        let gold = self.medals.values().filter(|&&m| m == Medal::Gold).count();
        let silver = self.medals.values().filter(|&&m| m == Medal::Silver).count();
        let bronze = self.medals.values().filter(|&&m| m == Medal::Bronze).count();
        ui.horizontal_wrapped(|ui| {
            widgets::pill(ui, &format!("{gold} Gold"), medal_color(Medal::Gold));
            widgets::pill(ui, &format!("{silver} Silver"), medal_color(Medal::Silver));
            widgets::pill(ui, &format!("{bronze} Bronze"), medal_color(Medal::Bronze));
            if let Some(last) = self.last_launched.clone() {
                if ui.button(format!("Check result for {}", last.name)).clicked() {
                    if let Some(out_path) = settings::ac_documents_dir().map(|d| d.join("out").join("race_out.json"))
                        && let Ok(summary) = race_out::load_race_out(&out_path)
                        && let Ok(bytes) = std::fs::read(&last.ini)
                    {
                        let finish_pos = summary.results.iter().find(|r| r.position == 1).map(|r| r.position);
                        let earned = events::evaluate_event_medal(
                            &String::from_utf8_lossy(&bytes),
                            summary.player_best_lap_ms,
                            finish_pos,
                        );
                        if earned != Medal::None {
                            let mut store = EventProgressStore::load(&progress_store_path());
                            store.record_medal(&last.id, earned);
                            let _ = store.save(&progress_store_path());
                            self.medals = load_medals();
                            notices.push(Notice {
                                text: format!("Recorded {} medal for {}!", earned.label(), last.name),
                                ok: true,
                            });
                        } else {
                            notices.push(Notice {
                                text: format!("No medal threshold met yet for {}.", last.name),
                                ok: false,
                            });
                        }
                    } else {
                        notices.push(Notice {
                            text: "Could not read Documents/Assetto Corsa/out/race_out.json.".into(),
                            ok: false,
                        });
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.career, false, "Special Events");
            ui.selectable_value(&mut self.career, true, "Career");
            ui.add_space(12.0);
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Search name, car or track").desired_width(300.0));
            ui.add_space(12.0);
            widgets::toggle(ui, &mut self.playable_only);
            ui.label("Installed only");
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for kind in ["All", "Hotlap", "Race", "Practice", "Drift", "Time attack", "Drag"] {
                ui.selectable_value(&mut self.kind, kind.to_string(), kind);
            }
        });
        ui.add_space(8.0);

        let query = self.query.trim().to_lowercase();
        let shown: Vec<&(GameEvent, bool)> = list
            .iter()
            .filter(|(e, playable)| {
                (self.career != e.series.is_empty())
                    && (self.kind == "All" || e.kind == self.kind)
                    && (*playable || !self.playable_only)
                    && (query.is_empty()
                        || [&e.name, &e.car, &e.track, &e.series].iter().any(|t| t.to_lowercase().contains(&query)))
            })
            .collect();
        ui.label(RichText::new(format!("{} events", shown.len())).size(12.5).color(theme::MUTED));
        ui.add_space(4.0);
        if shown.is_empty() {
            widgets::empty_state(ui, "No event fits. Clear the search or show events that need other content.");
            return;
        }

        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (event, playable) in shown.iter().take(MAX_ROWS).map(|pair| (&pair.0, pair.1)) {
                let medal = self.medals.get(&event.id).copied().unwrap_or(Medal::None);
                theme::card_elevated().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(150.0, 72.0), egui::Sense::hover());
                        media::paint_image(
                            ui,
                            rect,
                            event.preview.as_deref(),
                            &event.track,
                            egui::Color32::WHITE,
                            egui::CornerRadius::same(theme::RADIUS_SMALL),
                        );
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&event.name).font(theme::bold(15.0)));
                                widgets::pill(ui, &event.kind, theme::INFO);
                                if medal != Medal::None {
                                    widgets::pill(ui, medal.label(), medal_color(medal));
                                }
                            });
                            let track = names.track(&event.track);
                            let place = if event.layout.is_empty() { track } else { format!("{track}, {}", names.layout(&event.layout)) };
                            ui.label(RichText::new(format!("{}  \u{00B7}  {place}", names.car(&event.car))).size(12.5).color(theme::MUTED));
                            if !event.series.is_empty() {
                                ui.label(RichText::new(&event.series).size(12.0).color(theme::MUTED));
                            }
                            if !event.goals.is_empty() {
                                ui.label(RichText::new(format!("Goals: {}", event.goals.join("  /  "))).size(12.0).color(theme::MUTED));
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let open = playable && !self.busy && !ctx.game_running;
                            let button = ui.add_enabled(open, widgets::primary_button("Start"));
                            let button = if playable {
                                button
                            } else {
                                button.on_disabled_hover_text("Needs a car or track that is not installed.")
                            };
                            if button.clicked() {
                                chosen = Some(event.clone());
                            }
                        });
                    });
                });
                ui.add_space(6.0);
            }
            if shown.len() > MAX_ROWS {
                ui.label(RichText::new(format!("{} more. Narrow the search.", shown.len() - MAX_ROWS)).color(theme::MUTED));
            }
        });

        if let Some(event) = chosen {
            self.busy = true;
            let tx = self.tx.clone();
            let repaint = ui.ctx().clone();
            std::thread::spawn(move || {
                let notice = start(&root, &event);
                let _ = tx.send(Msg::Started(notice, event));
                repaint.request_repaint();
            });
        }
    }
}
