//! Championships tool page: build a series from rounds of pasted CSV results and read
//! its standings. Championships are kept in memory and saved to or loaded from JSON
//! files. File dialogs and file I/O run on background threads, and their results come
//! back over a channel that is polled once per frame.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED};
use crate::ui::{widgets, Notice};
use acxs_manager::championship::{self, Championship, Round, Standing};
use acxs_manager::race::{self, AiOpponent};
use acxs_manager::{game, race_out, settings};
use eframe::egui::{self, RichText, TextEdit};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const DEFAULT_POINTS: &str = "25, 18, 15, 12, 10, 8, 6, 4, 2, 1";

pub fn tool() -> Box<dyn Tool> {
    let (tx, rx) = mpsc::channel();
    Box::new(ChampionshipsPage {
        tx,
        rx,
        championships: Vec::new(),
        selected: None,
        create: CreateForm { points: DEFAULT_POINTS.to_string(), ..CreateForm::default() },
        round: RoundForm::default(),
        ballast_input: "30, 20, 10".to_string(),
        player_name: "Driver".to_string(),
        new_opp_car: "ks_ferrari_488_gt3".to_string(),
        new_opp_name: String::new(),
        new_opp_ai: 92,
        ac_root: None,
        dialog_open: false,
    })
}

/// Sent back by the file-dialog threads once they finish.
enum Msg {
    /// A server result file was read into the round form.
    Results(championship::ServerRace),
    Done {
        loaded: Option<Championship>,
        notice: Option<Notice>,
    },
}

/// A user action collected while drawing. Applied after the frame is drawn.
enum Action {
    Select(usize),
    Create,
    AddRound,
    AddScheduledRound,
    LaunchRound(usize),
    ImportRaceOut(usize),
    AddOpponent,
    RemoveOpponent(usize),
    ApplyBallast,
    LoadServerResults,
    SetCustomPoints,
    OpenFile,
    SaveSelected,
}

#[derive(Default)]
struct CreateForm {
    name: String,
    /// The points table the new championship will use, as typed or filled from a preset.
    points: String,
    /// Staging text for a custom points table, applied by the "Use custom table" button.
    custom: String,
}

#[derive(Default)]
struct RoundForm {
    track: String,
    layout: String,
    csv: String,
    laps: u32,
}

struct ChampionshipsPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    championships: Vec<Championship>,
    /// Index into `championships`. Championships are never removed, so it stays valid.
    selected: Option<usize>,
    create: CreateForm,
    round: RoundForm,
    ballast_input: String,
    player_name: String,
    new_opp_car: String,
    new_opp_name: String,
    new_opp_ai: u8,
    ac_root: Option<PathBuf>,
    /// True while a file dialog is open on a background thread.
    dialog_open: bool,
}

impl Tool for ChampionshipsPage {
    fn name(&self) -> &'static str {
        "Championships"
    }

    fn group(&self) -> &'static str {
        "Race"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.ac_root = if ctx.ac_valid { ctx.ac_root.clone() } else { None };
        self.poll(notices);
        widgets::heading(
            ui,
            "Championships",
            "Build playable AI seasons with success ballast, launch rounds, or import server/race_out.json results.",
        );

        let mut actions = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.draw(ui, &mut actions));

        let ctx = ui.ctx().clone();
        for action in actions {
            self.apply(action, &ctx, notices);
        }
    }
}

impl ChampionshipsPage {
    fn draw(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let busy = self.dialog_open;

        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("New championship").color(MUTED));
                ui.add(
                    TextEdit::singleline(&mut self.create.name)
                        .hint_text("Name")
                        .desired_width(220.0),
                );
                ui.add(
                    TextEdit::singleline(&mut self.create.points)
                        .hint_text("Points per position")
                        .desired_width(260.0),
                );
                if ui.add(widgets::primary_button("Create")).clicked() {
                    actions.push(Action::Create);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Preset").color(MUTED));
                let current = parse_points(&self.create.points).ok();
                let label = championship::preset_names()
                    .into_iter()
                    .find(|name| championship::points_preset(name) == current)
                    .unwrap_or("Custom");
                egui::ComboBox::from_id_salt("championship_points_preset")
                    .width(220.0)
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for name in championship::preset_names() {
                            if ui.selectable_label(label == name, name).clicked() {
                                if let Some(table) = championship::points_preset(name) {
                                    self.create.points = join_points(&table);
                                }
                            }
                        }
                    });
                ui.add(
                    TextEdit::singleline(&mut self.create.custom)
                        .hint_text("Custom, e.g. 12, 9, 7, 5")
                        .desired_width(220.0),
                );
                if ui.button("Use custom table").clicked() {
                    actions.push(Action::SetCustomPoints);
                }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Open file...")).clicked() {
                    actions.push(Action::OpenFile);
                }
                let can_save = !busy && self.selected.is_some();
                if ui.add_enabled(can_save, egui::Button::new("Save selected...")).clicked() {
                    actions.push(Action::SaveSelected);
                }
                if busy {
                    ui.spinner();
                    ui.label(RichText::new("Waiting for the file dialog...").color(MUTED));
                }
            });
        });

        ui.add_space(14.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(220.0);
                widgets::section_title(ui, "Championships");
                if self.championships.is_empty() {
                    widgets::empty_state(ui, "None yet. Create one above or open a file.");
                }
                for (i, champ) in self.championships.iter().enumerate() {
                    if ui.selectable_label(self.selected == Some(i), champ.name.as_str()).clicked() {
                        actions.push(Action::Select(i));
                    }
                }
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                match self.selected.and_then(|i| self.championships.get_mut(i)) {
                    Some(champ) => detail(
                        ui,
                        champ,
                        &mut self.round,
                        &mut self.ballast_input,
                        &mut self.player_name,
                        &mut self.new_opp_car,
                        &mut self.new_opp_name,
                        &mut self.new_opp_ai,
                        actions,
                    ),
                    None => widgets::empty_state(ui, "Select a championship to see its standings."),
                }
            });
        });
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context, notices: &mut Vec<Notice>) {
        match action {
            Action::Select(i) => self.selected = Some(i),
            Action::Create => report(notices, self.create_championship()),
            Action::AddRound => report(notices, self.add_round()),
            Action::AddScheduledRound => report(notices, self.add_scheduled_round()),
            Action::LaunchRound(round_idx) => report(notices, self.launch_round(round_idx)),
            Action::ImportRaceOut(round_idx) => report(notices, self.import_race_out(round_idx)),
            Action::AddOpponent => report(notices, self.add_opponent()),
            Action::RemoveOpponent(idx) => {
                if let Some(c) = self.selected.and_then(|i| self.championships.get_mut(i))
                    && idx < c.opponents.len()
                {
                    c.opponents.remove(idx);
                }
            }
            Action::ApplyBallast => report(notices, self.apply_ballast()),
            Action::SetCustomPoints => report(notices, self.set_custom_points()),
            Action::LoadServerResults => self.spawn_server_results(ctx),
            Action::OpenFile => self.spawn_open(ctx),
            Action::SaveSelected => {
                if let Some(champ) = self.selected.and_then(|i| self.championships.get(i)).cloned() {
                    self.spawn_save(ctx, champ);
                }
            }
        }
    }

    fn add_scheduled_round(&mut self) -> Result<String, String> {
        let index = self.selected.ok_or("Select a championship first.")?;
        let track = self.round.track.trim().to_string();
        if track.is_empty() {
            return Err("Enter a track id for the scheduled round.".into());
        }
        let champ = &mut self.championships[index];
        champ.rounds.push(Round {
            track: track.clone(),
            layout: Some(self.round.layout.trim().to_string()).filter(|s| !s.is_empty()),
            results: Vec::new(),
            laps: if self.round.laps == 0 { 5 } else { self.round.laps },
            weather: "3_clear".into(),
            time_of_day_hour: 13,
            completed: false,
        });
        self.round.track.clear();
        self.round.layout.clear();
        Ok(format!("Scheduled round {} at {track}.", champ.rounds.len()))
    }

    fn launch_round(&self, round_idx: usize) -> Result<String, String> {
        let index = self.selected.ok_or("Select a championship first.")?;
        let root = self.ac_root.as_ref().ok_or("Set your Assetto Corsa folder in Settings first.")?;
        if game::is_game_running() {
            return Err("Assetto Corsa is already running. Close it first.".into());
        }
        let cfg = settings::ac_user_cfg_dir().ok_or("Could not find Documents/Assetto Corsa/cfg.")?;
        let champ = &self.championships[index];
        let qrace = champ
            .build_round_race(round_idx, &self.player_name)
            .map_err(|e| e.to_string())?;
        race::write_race_ini(&cfg, &qrace).map_err(|e| e.to_string())?;
        game::launch_game(root).map_err(|e| e.to_string())?;
        Ok(format!("Launched Round {} at {} (ballast {} kg).", round_idx + 1, qrace.track, qrace.player_ballast))
    }

    fn import_race_out(&mut self, round_idx: usize) -> Result<String, String> {
        let index = self.selected.ok_or("Select a championship first.")?;
        let out_path = settings::ac_documents_dir()
            .map(|d| d.join("out").join("race_out.json"))
            .ok_or("Could not find Documents/Assetto Corsa.")?;
        let summary = race_out::load_race_out(&out_path).map_err(|e| e.to_string())?;
        let count = summary.results.len();
        self.championships[index]
            .record_round_results(round_idx, summary.results)
            .map_err(|e| e.to_string())?;
        Ok(format!("Recorded {count} results from race_out.json into Round {}.", round_idx + 1))
    }

    fn add_opponent(&mut self) -> Result<String, String> {
        let index = self.selected.ok_or("Select a championship first.")?;
        let car = self.new_opp_car.trim().to_string();
        if car.is_empty() {
            return Err("Enter an opponent car id.".into());
        }
        let champ = &mut self.championships[index];
        let name = if self.new_opp_name.trim().is_empty() {
            format!("AI {}", champ.opponents.len() + 1)
        } else {
            self.new_opp_name.trim().to_string()
        };
        champ.opponents.push(AiOpponent {
            car,
            driver_name: name.clone(),
            ai_level: self.new_opp_ai,
            ..Default::default()
        });
        self.new_opp_name.clear();
        Ok(format!("Added opponent {name}."))
    }

    fn apply_ballast(&mut self) -> Result<String, String> {
        let index = self.selected.ok_or("Select a championship first.")?;
        let table = parse_points(&self.ballast_input)?;
        self.championships[index].success_ballast_kg = table;
        Ok("Updated success ballast table.".into())
    }

    fn set_custom_points(&mut self) -> Result<String, String> {
        let table = parse_points(&self.create.custom)?;
        self.create.points = join_points(&table);
        Ok(format!("Points set to {}.", self.create.points))
    }

    fn create_championship(&mut self) -> Result<String, String> {
        let name = self.create.name.trim().to_string();
        if name.is_empty() {
            return Err("Give the championship a name.".into());
        }
        let points = parse_points(&self.create.points)?;
        let champ = Championship::new(name.clone(), points);
        champ.validate().map_err(|e| e.to_string())?;
        self.championships.push(champ);
        self.selected = Some(self.championships.len() - 1);
        self.create.name.clear();
        Ok(format!("Created {name}."))
    }

    fn add_round(&mut self) -> Result<String, String> {
        let Some(index) = self.selected else {
            return Err("Select a championship before adding a round.".into());
        };
        let track = self.round.track.trim().to_string();
        if track.is_empty() {
            return Err("Enter the track for this round.".into());
        }
        let mut round = championship::import_results_csv(&self.round.csv).map_err(|e| e.to_string())?;
        round.track = track.clone();
        round.layout = Some(self.round.layout.trim().to_string()).filter(|s| !s.is_empty());

        let champ = &mut self.championships[index];
        champ.rounds.push(round);
        if let Err(e) = champ.validate() {
            champ.rounds.pop();
            return Err(e.to_string());
        }
        let number = champ.rounds.len();
        self.round = RoundForm::default();
        Ok(format!("Added round {number} at {track}."))
    }

    fn spawn_server_results(&mut self, ctx: &egui::Context) {
        self.dialog_open = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let message = match rfd::FileDialog::new().add_filter("Server result", &["json"]).pick_file() {
                None => Msg::Done { loaded: None, notice: None },
                Some(path) => {
                    let race = std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|text| championship::import_server_results(&text).map_err(|e| e.to_string()));
                    match race {
                        Ok(race) => Msg::Results(race),
                        Err(e) => {
                            let text = format!("Could not read {}: {e}", path.display());
                            Msg::Done { loaded: None, notice: Some(Notice { text, ok: false }) }
                        }
                    }
                }
            };
            let _ = tx.send(message);
            ctx.request_repaint();
        });
    }

    fn spawn_open(&mut self, ctx: &egui::Context) {
        self.dialog_open = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .add_filter("Championship", &["json"])
                .pick_file();
            let (loaded, notice) = match picked {
                None => (None, None),
                Some(path) => match Championship::load(&path) {
                    Ok(champ) => {
                        let text = format!("Opened {} from {}.", champ.name, path.display());
                        (Some(champ), Some(Notice { text, ok: true }))
                    }
                    Err(e) => (None, Some(Notice { text: format!("Open failed: {e}"), ok: false })),
                },
            };
            let _ = tx.send(Msg::Done { loaded, notice });
            ctx.request_repaint();
        });
    }

    fn spawn_save(&mut self, ctx: &egui::Context, champ: Championship) {
        self.dialog_open = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_file_name(format!("{}.json", file_stem(&champ.name)))
                .add_filter("Championship", &["json"])
                .save_file();
            let notice = picked.map(|path| match champ.save(&path) {
                Ok(()) => Notice {
                    text: format!("Saved {} to {}.", champ.name, path.display()),
                    ok: true,
                },
                Err(e) => Notice { text: format!("Save failed: {e}"), ok: false },
            });
            let _ = tx.send(Msg::Done { loaded: None, notice });
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, notices: &mut Vec<Notice>) {
        while let Ok(message) = self.rx.try_recv() {
            self.dialog_open = false;
            let (loaded, notice) = match message {
                Msg::Done { loaded, notice } => (loaded, notice),
                Msg::Results(race) => {
                    let text = format!("Read the race at {}. Check it, then add the round.", race.track);
                    self.round = RoundForm { track: race.track, layout: race.layout, csv: race.csv, laps: 5 };
                    (None, Some(Notice { text, ok: true }))
                }
            };
            if let Some(champ) = loaded {
                self.championships.push(champ);
                self.selected = Some(self.championships.len() - 1);
            }
            notices.extend(notice);
        }
    }
}

fn report(notices: &mut Vec<Notice>, result: Result<String, String>) {
    match result {
        Ok(text) => notices.push(Notice { text, ok: true }),
        Err(text) => notices.push(Notice { text, ok: false }),
    }
}

/// Reads "25, 18, 15" or "25 18 15". Commas and spaces both separate values.
fn parse_points(text: &str) -> Result<Vec<u32>, String> {
    let points = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().map_err(|_| format!("\"{s}\" is not a point value")))
        .collect::<Result<Vec<u32>, String>>()?;
    if points.is_empty() {
        return Err("Enter the points for each position, such as 25, 18, 15.".into());
    }
    Ok(points)
}

/// Formats a points table as "25, 18, 15", the form `parse_points` reads back.
fn join_points(table: &[u32]) -> String {
    table.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

/// A file name derived from the championship name, with characters that are awkward in paths replaced.
fn file_stem(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if stem.is_empty() { "championship".to_string() } else { stem }
}

/// The details of one championship: standings, playable AI season config, rounds and add-round form.
#[allow(clippy::too_many_arguments)]
fn detail(
    ui: &mut egui::Ui,
    champ: &mut Championship,
    form: &mut RoundForm,
    ballast_input: &mut String,
    player_name: &mut String,
    new_opp_car: &mut String,
    new_opp_name: &mut String,
    new_opp_ai: &mut u8,
    actions: &mut Vec<Action>,
) {
    ui.label(RichText::new(champ.name.as_str()).font(theme::display(22.0)));
    let table = if champ.points_table.is_empty() {
        "none".to_string()
    } else {
        join_points(&champ.points_table)
    };
    ui.label(RichText::new(format!("Points: {table}  ·  {} rounds", champ.rounds.len())).color(MUTED));

    ui.add_space(8.0);
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            widgets::toggle(ui, &mut champ.playable);
            ui.label("Playable AI Season");
            ui.add_space(12.0);
            ui.label(RichText::new("Fastest lap bonus:").color(MUTED));
            ui.add(egui::DragValue::new(&mut champ.scoring.fastest_lap_bonus).range(0..=5).suffix(" pts"));
            ui.add_space(12.0);
            ui.label(RichText::new("Success ballast (kg):").color(MUTED));
            ui.add(TextEdit::singleline(ballast_input).desired_width(110.0));
            if ui.small_button("Set").clicked() {
                actions.push(Action::ApplyBallast);
            }
        });
        if champ.playable {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Driver:").color(MUTED));
                ui.add(TextEdit::singleline(player_name).desired_width(120.0));
                ui.label(RichText::new("Car ID:").color(MUTED));
                ui.add(TextEdit::singleline(&mut champ.player_car).hint_text("ks_ferrari_488_gt3").desired_width(180.0));
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Add AI:").color(MUTED));
                ui.add(TextEdit::singleline(new_opp_car).hint_text("car id").desired_width(150.0));
                ui.add(TextEdit::singleline(new_opp_name).hint_text("driver name").desired_width(110.0));
                ui.add(egui::DragValue::new(new_opp_ai).range(70..=100).suffix("%"));
                if ui.small_button("+ Add").clicked() {
                    actions.push(Action::AddOpponent);
                }
            });
            if !champ.opponents.is_empty() {
                ui.add_space(4.0);
                for (idx, opp) in champ.opponents.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} · {} · AI {}%", opp.driver_name, opp.car, opp.ai_level)).font(theme::mono(12.0)));
                        if ui.small_button("×").clicked() {
                            actions.push(Action::RemoveOpponent(idx));
                        }
                    });
                }
            }
        }
    });

    ui.add_space(10.0);
    widgets::section_title(ui, "Standings");
    let rows = championship::standings_with(champ, champ.scoring);
    if rows.is_empty() {
        widgets::empty_state(ui, "No completed rounds yet.");
    } else {
        standings_grid(ui, &rows, &champ.success_ballast_kg);
    }

    ui.add_space(12.0);
    widgets::section_title(ui, "Rounds");
    if champ.rounds.is_empty() {
        ui.label(RichText::new("None.").color(MUTED));
    }
    for (i, round) in champ.rounds.iter().enumerate() {
        let place = match round.layout.as_deref() {
            Some(layout) => format!("{} ({layout})", round.track),
            None => round.track.clone(),
        };
        let winner = round
            .results
            .iter()
            .find(|r| r.position == 1 && !r.dnf)
            .map_or("scheduled", |r| r.driver.as_str());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "R{}  {place}  ·  {winner}  ·  {} results",
                    i + 1,
                    round.results.len()
                ))
                .font(theme::mono(12.5)),
            );
            if champ.playable {
                if ui.small_button("Launch Round").clicked() {
                    actions.push(Action::LaunchRound(i));
                }
                if ui.small_button("Import race_out.json").clicked() {
                    actions.push(Action::ImportRaceOut(i));
                }
            }
        });
    }

    ui.add_space(12.0);
    widgets::section_title(ui, "Add a round");
    ui.label(
        RichText::new("Schedule a playable round or paste CSV results (driver,position,fastest_lap,dnf).")
            .small()
            .color(MUTED),
    );
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut form.track)
                .hint_text("Track")
                .desired_width(220.0),
        );
        ui.add(
            TextEdit::singleline(&mut form.layout)
                .hint_text("Layout (optional)")
                .desired_width(160.0),
        );
        ui.add(egui::DragValue::new(&mut form.laps).range(0..=200).suffix(" laps"));
        if champ.playable && ui.button("Schedule round").clicked() {
            actions.push(Action::AddScheduledRound);
        }
    });
    ui.add(
        TextEdit::multiline(&mut form.csv)
            .hint_text("driver,position,fastest_lap,dnf\nAlice,1,yes,no\nBob,2,no,no")
            .desired_rows(5)
            .desired_width(520.0),
    );
    ui.horizontal(|ui| {
        if ui.add(widgets::primary_button("Add round from CSV")).clicked() {
            actions.push(Action::AddRound);
        }
        if ui
            .button("Load server result file")
            .on_hover_text("Fills the form from a file in the results folder of a dedicated server.")
            .clicked()
        {
            actions.push(Action::LoadServerResults);
        }
    });
}

fn standings_grid(ui: &mut egui::Ui, rows: &[Standing], ballast: &[u32]) {
    egui::Grid::new("championship_standings")
        .striped(true)
        .num_columns(8)
        .show(ui, |ui| {
            for header in ["Pos", "Driver", "Points", "Wins", "Fastest laps", "Best", "Rounds", "Next Ballast"] {
                ui.label(RichText::new(header).color(MUTED));
            }
            ui.end_row();
            for (i, row) in rows.iter().enumerate() {
                ui.label(RichText::new((i + 1).to_string()).font(theme::mono(12.5)));
                ui.label(row.driver.as_str());
                ui.label(RichText::new(row.points.to_string()).font(theme::mono(12.5)));
                ui.label(RichText::new(row.wins.to_string()).font(theme::mono(12.5)));
                ui.label(RichText::new(row.fastest_laps.to_string()).font(theme::mono(12.5)));
                ui.label(RichText::new(row.best_position.map_or("-".to_string(), |p| format!("P{p}"))).font(theme::mono(12.5)));
                ui.label(RichText::new(row.rounds_entered.to_string()).font(theme::mono(12.5)));
                let kg = ballast.get(i).copied().unwrap_or(0);
                ui.label(RichText::new(if kg > 0 { format!("+{kg} kg") } else { "-".into() }).font(theme::mono(12.5)));
                ui.end_row();
            }
        });
}
