//! Championships tool page: build a series from rounds of pasted CSV results and read
//! its standings. Championships are kept in memory and saved to or loaded from JSON
//! files. File dialogs and file I/O run on background threads, and their results come
//! back over a channel that is polled once per frame.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, MUTED};
use crate::ui::{widgets, Notice};
use acxs_manager::championship::{self, Championship, Standing};
use eframe::egui::{self, RichText, TextEdit};
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
}

struct ChampionshipsPage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    championships: Vec<Championship>,
    /// Index into `championships`. Championships are never removed, so it stays valid.
    selected: Option<usize>,
    create: CreateForm,
    round: RoundForm,
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

    fn ui(&mut self, ui: &mut egui::Ui, _ctx: &ToolContext, notices: &mut Vec<Notice>) {
        self.poll(notices);
        widgets::heading(
            ui,
            "Championships",
            "Build a series from round results and read the standings. Save and open files with the buttons.",
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
                match self.selected.and_then(|i| self.championships.get(i)) {
                    Some(champ) => detail(ui, champ, &mut self.round, actions),
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
                    self.round = RoundForm { track: race.track, layout: race.layout, csv: race.csv };
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

/// The details of one championship: standings, rounds and the form to add a round.
fn detail(ui: &mut egui::Ui, champ: &Championship, form: &mut RoundForm, actions: &mut Vec<Action>) {
    ui.label(RichText::new(champ.name.as_str()).size(20.0).strong());
    let table = if champ.points_table.is_empty() {
        "none".to_string()
    } else {
        join_points(&champ.points_table)
    };
    ui.label(RichText::new(format!("Points: {table}  ·  {} rounds", champ.rounds.len())).color(MUTED));

    ui.add_space(10.0);
    widgets::section_title(ui, "Standings");
    if champ.rounds.is_empty() {
        widgets::empty_state(ui, "No rounds yet. Add the first round below.");
    } else {
        standings_grid(ui, &championship::standings(champ));
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
            .map_or("no winner listed", |r| r.driver.as_str());
        ui.label(format!(
            "R{}  {place}  -  winner: {winner}  -  {} results",
            i + 1,
            round.results.len()
        ));
    }

    ui.add_space(12.0);
    widgets::section_title(ui, "Add a round");
    ui.label(
        RichText::new("Paste results as CSV: driver,position,fastest_lap,dnf. The last two columns are optional.")
            .small()
            .color(MUTED),
    );
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut form.track)
                .hint_text("Track")
                .desired_width(240.0),
        );
        ui.add(
            TextEdit::singleline(&mut form.layout)
                .hint_text("Layout (optional)")
                .desired_width(180.0),
        );
    });
    ui.add(
        TextEdit::multiline(&mut form.csv)
            .hint_text("driver,position,fastest_lap,dnf\nAlice,1,yes,no\nBob,2,no,no")
            .desired_rows(6)
            .desired_width(520.0),
    );
    ui.horizontal(|ui| {
        if ui.add(widgets::primary_button("Add round")).clicked() {
            actions.push(Action::AddRound);
        }
        if ui
            .button("Load server result file")
            .on_hover_text("Fills the form from a results\*.json of a dedicated server.")
            .clicked()
        {
            actions.push(Action::LoadServerResults);
        }
    });
}

fn standings_grid(ui: &mut egui::Ui, rows: &[Standing]) {
    egui::Grid::new("championship_standings")
        .striped(true)
        .num_columns(7)
        .show(ui, |ui| {
            for header in ["Pos", "Driver", "Points", "Wins", "Fastest laps", "Best finish", "Rounds"] {
                ui.label(RichText::new(header).color(MUTED));
            }
            ui.end_row();
            for (i, row) in rows.iter().enumerate() {
                ui.label((i + 1).to_string());
                ui.label(row.driver.as_str());
                ui.label(row.points.to_string());
                ui.label(row.wins.to_string());
                ui.label(row.fastest_laps.to_string());
                ui.label(row.best_position.map_or("-".to_string(), |p| format!("P{p}")));
                ui.label(row.rounds_entered.to_string());
                ui.end_row();
            }
        });
}
