//! Logs tool page: reads an Assetto Corsa log on a background thread and shows
//! its error, warning and info counts, so a failed launch can be explained
//! without opening the raw file.

use super::{Tool, ToolContext};
use crate::ui::theme::{self, DANGER, INFO, MUTED, TEXT, WARN};
use crate::ui::widgets;
use crate::ui::Notice;
use acxs_manager::ac_log::{self, LogLevel, LogLine, LogSummary};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Height of one log line row. Matches a monospace label without wrapping.
const ROW_HEIGHT: f32 = 18.0;

pub fn tool() -> Box<dyn Tool> {
    Box::new(LogsTool::default())
}

/// Output of one background read: the summary and every classified line.
struct Loaded {
    path: PathBuf,
    summary: LogSummary,
    lines: Vec<LogLine>,
}

/// Which lines the list shows.
#[derive(Clone, Copy, Default, PartialEq)]
enum Filter {
    #[default]
    Errors,
    Warnings,
    Infos,
    All,
}

impl Filter {
    const ALL: [Filter; 4] = [Filter::Errors, Filter::Warnings, Filter::Infos, Filter::All];

    fn label(self) -> &'static str {
        match self {
            Filter::Errors => "Errors",
            Filter::Warnings => "Warnings",
            Filter::Infos => "Infos",
            Filter::All => "All lines",
        }
    }

    fn keeps(self, level: LogLevel) -> bool {
        match self {
            Filter::Errors => level == LogLevel::Error,
            Filter::Warnings => level == LogLevel::Warning,
            Filter::Infos => level == LogLevel::Info,
            Filter::All => true,
        }
    }
}

#[derive(Default)]
struct LogsTool {
    initialized: bool,
    path_input: String,
    /// Set while a read runs on a background thread.
    pending: Option<Receiver<Result<Loaded, String>>>,
    loaded: Option<Loaded>,
    error: Option<String>,
    filter: Filter,
}

impl Tool for LogsTool {
    fn name(&self) -> &'static str {
        "Logs"
    }

    fn group(&self) -> &'static str {
        "Diagnostics"
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>) {
        if !self.initialized {
            self.initialized = true;
            self.path_input = default_log_path(ctx.ac_root.as_deref());
        }
        self.poll();

        widgets::heading(ui, "Logs", "Summarize the Assetto Corsa log to find out why a launch failed.");

        let busy = self.pending.is_some();
        let mut start_read = false;
        theme::card().show(ui, |ui| {
            ui.label(RichText::new("Log file (Documents/Assetto Corsa/logs/log.txt)").color(MUTED));
            ui.horizontal(|ui| {
                let field_width = (ui.available_width() - 260.0).max(160.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .desired_width(field_width)
                        .hint_text("Choose the log file"),
                );
                if ui.button("Browse").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Log files", &["txt"])
                        .pick_file()
                    {
                        self.path_input = path.display().to_string();
                    }
                }
                let can_read = !busy && !self.path_input.trim().is_empty();
                if ui.add_enabled(can_read, widgets::primary_button("Read")).clicked() {
                    start_read = true;
                }
                if busy {
                    ui.spinner();
                    ui.label(RichText::new("Reading...").color(MUTED));
                }
            });
            if let Some(error) = &self.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(DANGER));
            }
        });

        if start_read {
            self.start_read(ui.ctx());
        }

        let Some(loaded) = &self.loaded else {
            ui.add_space(12.0);
            widgets::empty_state(ui, "Read a log file to see its summary.");
            return;
        };

        ui.add_space(12.0);
        ui.label(RichText::new(loaded.path.display().to_string()).small().color(MUTED));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let counts = [
                ("Errors", loaded.summary.errors, DANGER),
                ("Warnings", loaded.summary.warnings, WARN),
                ("Infos", loaded.summary.infos, MUTED),
            ];
            for (label, count, color) in counts {
                theme::card().show(ui, |ui| {
                    ui.set_width(150.0);
                    widgets::stat(ui, label, &count.to_string(), color);
                });
            }
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("logs_filter")
                .selected_text(self.filter.label())
                .show_ui(ui, |ui| {
                    for filter in Filter::ALL {
                        ui.selectable_value(&mut self.filter, filter, filter.label());
                    }
                });
            if ui.button("Copy summary").clicked() {
                ui.ctx().copy_text(summary_text(loaded));
                notices.push(Notice {
                    text: "Log summary copied to the clipboard.".to_string(),
                    ok: true,
                });
            }
        });
        ui.add_space(6.0);

        let filter = self.filter;
        let rows: Vec<&LogLine> = loaded.lines.iter().filter(|line| filter.keeps(line.level)).collect();
        if rows.is_empty() {
            widgets::empty_state(ui, "No lines match this filter.");
            return;
        }
        ui.label(
            RichText::new(format!("{} lines shown", rows.len()))
                .small()
                .color(MUTED),
        );
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            ui,
            ROW_HEIGHT,
            rows.len(),
            |ui, range| {
                for line in &rows[range] {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{:>6}  {}", line.line_no, line.text))
                                .monospace()
                                .color(level_color(line.level)),
                        )
                        .truncate(),
                    );
                }
            },
        );
    }
}

impl LogsTool {
    /// Takes a finished background read, if one is ready. Never blocks.
    fn poll(&mut self) {
        let Some(rx) = &self.pending else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The read stopped before it finished.".to_string()),
        };
        self.pending = None;
        match result {
            Ok(loaded) => {
                self.loaded = Some(loaded);
                self.error = None;
            }
            Err(message) => {
                self.loaded = None;
                self.error = Some(message);
            }
        }
    }

    /// Starts reading the path in the text field on a background thread.
    fn start_read(&mut self, ctx: &egui::Context) {
        let path = PathBuf::from(self.path_input.trim());
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.pending = Some(rx);
        self.error = None;
        std::thread::spawn(move || {
            let _ = tx.send(read_and_parse(&path));
            ctx.request_repaint();
        });
    }
}

fn read_and_parse(path: &Path) -> Result<Loaded, String> {
    let text = ac_log::read_log(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    Ok(Loaded {
        path: path.to_path_buf(),
        summary: ac_log::summarize(&text),
        lines: ac_log::parse_lines(&text),
    })
}

/// The AC log path to start with: a path the core library knows for the install,
/// else `Documents/Assetto Corsa/logs/log.txt` under the home folder, else empty.
fn default_log_path(ac_root: Option<&Path>) -> String {
    if let Some(path) = ac_root.and_then(|root| ac_log::known_log_paths(root).into_iter().next()) {
        return path.display().to_string();
    }
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| {
            PathBuf::from(home)
                .join("Documents")
                .join("Assetto Corsa")
                .join("logs")
                .join("log.txt")
                .display()
                .to_string()
        })
        .unwrap_or_default()
}

/// Plain text for the clipboard: the counts, then the first errors with line numbers.
fn summary_text(loaded: &Loaded) -> String {
    let summary = &loaded.summary;
    let mut out = format!(
        "Log: {}\nErrors: {}, warnings: {}, infos: {}\n",
        loaded.path.display(),
        summary.errors,
        summary.warnings,
        summary.infos
    );
    if !summary.first_errors.is_empty() {
        out.push_str("First errors:\n");
        for line in &summary.first_errors {
            out.push_str(&format!("Line {}: {}\n", line.line_no, line.text));
        }
    }
    out
}

fn level_color(level: LogLevel) -> Color32 {
    match level {
        LogLevel::Error => DANGER,
        LogLevel::Warning => WARN,
        LogLevel::Info => INFO,
        LogLevel::Other => TEXT,
    }
}
