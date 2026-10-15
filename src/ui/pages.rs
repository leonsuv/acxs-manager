//! The four pages of the main area: Home, Cars, Tracks and the CSP lights patch.
//!
//! Pages only read `App` and report user actions by calling its methods, which
//! run the real work on background threads.

use super::theme::{self, DANGER, INFO, MUTED, OK, TEXT, WARN};
use super::widgets;
use super::App;
use crate::content::{self, CarEntry, TrackEntry};
use crate::csp::{self, LightsPatch, UNRECOGNIZED};
use acxs_manager::dashboard_stats::{self, ContentStats, CountEntry};
use acxs_manager::skins;
use acxs_manager::update::{self, Release, Version};
use eframe::egui::{self, RichText};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Row height for `show_rows` (without item spacing). Matches the two-line list rows.
const ROW_HEIGHT: f32 = 60.0;
/// Most entries shown in each home bar list.
const TOP_N: usize = 5;
/// egui memory id of the cached home statistics.
const STATS_ID: &str = "home_content_stats";
/// egui memory id of the update check card state.
const UPDATE_STATE_ID: &str = "home_update_check";
/// Upper bound for one update check, connect through reply.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn home(app: &mut App, ui: &mut egui::Ui) {
    widgets::heading(ui, "Home", "Point the manager at your Assetto Corsa install, then launch.");

    let ac_valid = app.snapshot.ac_valid;
    let game_running = app.snapshot.game_running;
    let busy = app.busy;
    let installed = app.snapshot.installed.clone();
    let stats = if ac_valid { content_stats(app, ui) } else { None };

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let (ac_text, ac_color) = if ac_valid { ("found", OK) } else { ("missing", DANGER) };
            tile(ui, |ui| widgets::stat(ui, "Assetto Corsa", ac_text, ac_color));

            let game_text = if game_running { "running" } else { "not running" };
            tile(ui, |ui| widgets::stat(ui, "Game", game_text, theme::pill_color(game_text)));

            let (csp_text, csp_color) = match installed.as_deref() {
                None => ("none", WARN),
                Some(UNRECOGNIZED) => ("unrecognized", WARN),
                Some(version) => (version, OK),
            };
            tile(ui, |ui| widgets::stat(ui, "CSP lights patch", csp_text, csp_color));
        });

        ui.add_space(16.0);
        content_section(ui, ac_valid, stats);

        ui.add_space(16.0);
        widgets::section_title(ui, "Folders");
        theme::card().show(ui, |ui| {
            let field_width = (ui.available_width() - 120.0).max(160.0);

            ui.label(RichText::new("Assetto Corsa folder (the one containing acs.exe)").color(MUTED));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.ac_input)
                        .desired_width(field_width)
                        .hint_text("Not set"),
                );
                if ui.button("Browse").clicked() {
                    if let Some(path) = app.pick_folder() {
                        app.ac_input = path.display().to_string();
                    }
                }
            });
            ui.add_space(8.0);

            ui.label(RichText::new("Patch library (where lights patch folders live)").color(MUTED));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.library_input)
                        .desired_width(field_width)
                        .hint_text("Default: the folder the program runs from"),
                );
                if ui.button("Browse").clicked() {
                    if let Some(path) = app.pick_folder() {
                        app.library_input = path.display().to_string();
                    }
                }
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.button("Save folders").clicked() {
                    app.save_paths();
                }
                if !ac_valid {
                    ui.label(RichText::new("Choose the folder that contains acs.exe, then save.").color(WARN));
                }
            });
        });

        ui.add_space(16.0);
        update_card(app, ui);

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let launch = ui.add_enabled(
                ac_valid && !game_running,
                widgets::primary_button("Launch Assetto Corsa"),
            );
            if launch.clicked() {
                app.launch_game();
            }
            if ui.add_enabled(!busy, egui::Button::new("Rescan")).clicked() {
                app.refresh();
            }
        });
    });
}

pub fn cars(app: &mut App, ui: &mut egui::Ui) {
    let App { snapshot, car_query, selected_car, .. } = app;
    widgets::heading(ui, "Cars", "Installed cars with their names, classes and tags.");
    if !snapshot.ac_valid {
        widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to see cars.");
        return;
    }

    egui::Panel::right("car_detail")
        .resizable(true)
        .default_size(340.0)
        .show(ui, |ui| {
            let car = selected_car
                .as_deref()
                .and_then(|id| snapshot.cars.iter().find(|c| c.id == id));
            car_detail(ui, car);
        });

    ui.add(
        egui::TextEdit::singleline(&mut *car_query)
            .hint_text("Search name, brand, class or tag")
            .desired_width(f32::INFINITY),
    );
    let filtered = content::filter_cars(&snapshot.cars, car_query.as_str());
    ui.label(
        RichText::new(format!("{} of {} cars", filtered.len(), snapshot.cars.len()))
            .small()
            .color(MUTED),
    );
    ui.add_space(6.0);
    if filtered.is_empty() {
        let text = if snapshot.cars.is_empty() {
            "No cars found in content/cars."
        } else {
            "No cars match your search."
        };
        widgets::empty_state(ui, text);
        return;
    }

    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
        ui,
        ROW_HEIGHT,
        filtered.len(),
        |ui, range| {
            for car in &filtered[range] {
                let selected = selected_car.as_deref() == Some(car.id.as_str());
                if widgets::list_row(ui, selected, |ui| car_row(ui, car)) {
                    *selected_car = Some(car.id.clone());
                }
            }
        },
    );
}

pub fn tracks(app: &mut App, ui: &mut egui::Ui) {
    let App { snapshot, track_query, selected_track, .. } = app;
    widgets::heading(ui, "Tracks", "Installed tracks with their layouts and sizes.");
    if !snapshot.ac_valid {
        widgets::empty_state(ui, "Set your Assetto Corsa folder on the Home page to see tracks.");
        return;
    }

    egui::Panel::right("track_detail")
        .resizable(true)
        .default_size(340.0)
        .show(ui, |ui| {
            let track = selected_track
                .as_deref()
                .and_then(|id| snapshot.tracks.iter().find(|t| t.id == id));
            track_detail(ui, track);
        });

    ui.add(
        egui::TextEdit::singleline(&mut *track_query)
            .hint_text("Search name, country or city")
            .desired_width(f32::INFINITY),
    );
    let filtered = content::filter_tracks(&snapshot.tracks, track_query.as_str());
    ui.label(
        RichText::new(format!("{} of {} tracks", filtered.len(), snapshot.tracks.len()))
            .small()
            .color(MUTED),
    );
    ui.add_space(6.0);
    if filtered.is_empty() {
        let text = if snapshot.tracks.is_empty() {
            "No tracks found in content/tracks."
        } else {
            "No tracks match your search."
        };
        widgets::empty_state(ui, text);
        return;
    }

    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
        ui,
        ROW_HEIGHT,
        filtered.len(),
        |ui, range| {
            for track in &filtered[range] {
                let selected = selected_track.as_deref() == Some(track.id.as_str());
                if widgets::list_row(ui, selected, |ui| track_row(ui, track)) {
                    *selected_track = Some(track.id.clone());
                }
            }
        },
    );
}

pub fn patches(app: &mut App, ui: &mut egui::Ui) {
    widgets::heading(ui, "CSP Lights Patch", "Install a lights patch from your library, or archive the active one.");

    let ac_valid = app.snapshot.ac_valid;
    let game_running = app.snapshot.game_running;
    let busy = app.busy;
    let installed = app.snapshot.installed.clone();
    let installed_sha = app.snapshot.installed_sha.clone();
    let patches = app.snapshot.patches.clone();
    let archive = app.settings.ac_root_path().map(|root| csp::archive_dir(&root));
    let library = app.library().display().to_string();
    let archive_text = archive
        .as_ref()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| "Set your Assetto Corsa folder first".to_string());
    if installed.is_none() {
        app.confirm_uninstall = false;
    }

    let mut to_install: Option<LightsPatch> = None;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        theme::card().show(ui, |ui| {
            let (pill_text, pill_color) = match installed.as_deref() {
                None => ("not installed", WARN),
                Some(UNRECOGNIZED) => ("unrecognized", WARN),
                Some(_) => ("installed", OK),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(csp::status_text(&installed)).strong());
                widgets::pill(ui, pill_text, pill_color);
            });
            ui.add_space(8.0);
            widgets::key_value(ui, "Library", &library);
            widgets::key_value(ui, "Archive", &archive_text);

            if !ac_valid {
                ui.label(RichText::new("Set your Assetto Corsa folder on the Home page before installing.").color(WARN));
            } else if game_running {
                ui.label(RichText::new("Close Assetto Corsa before installing or removing a patch.").color(WARN));
            }

            if installed.is_some() {
                ui.add_space(8.0);
                if app.confirm_uninstall {
                    ui.horizontal(|ui| {
                        ui.label("Move the active lights patch into the archive?");
                        if ui.button(RichText::new("Archive and remove").color(DANGER)).clicked() {
                            app.uninstall_patch();
                        }
                        if ui.button("Cancel").clicked() {
                            app.confirm_uninstall = false;
                        }
                    });
                    ui.label(RichText::new("Files are moved to the archive folder, not deleted.").small().color(MUTED));
                } else if ui
                    .add_enabled(ac_valid && !game_running && !busy, egui::Button::new("Uninstall"))
                    .clicked()
                {
                    app.confirm_uninstall = true;
                }
            }
        });

        ui.add_space(16.0);
        widgets::section_title(ui, "Discovered patches");
        if patches.is_empty() {
            widgets::empty_state(ui, "No lights patches found. Add patch folders to the library.");
        }

        for patch in &patches {
            let from_archive = archive.as_ref().is_some_and(|dir| patch.folder.starts_with(dir));
            let source = if from_archive { "Archive" } else { "Library" };
            let is_installed = installed_sha.as_deref() == Some(patch.sha256.as_str());
            let known_text = if patch.known { "known" } else { UNRECOGNIZED };
            let short_sha = patch.sha256.get(..12).unwrap_or(patch.sha256.as_str());

            theme::card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Lights patch {}", patch.version)).strong());
                    widgets::pill(ui, &format!("family {}", patch.family), INFO);
                    widgets::pill(ui, known_text, theme::pill_color(known_text));
                    widgets::pill(ui, source, MUTED);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_installed {
                            widgets::pill(ui, "installed", OK);
                        } else if ui
                            .add_enabled(
                                ac_valid && !game_running && !busy,
                                egui::Button::new("Install"),
                            )
                            .clicked()
                        {
                            to_install = Some(patch.clone());
                        }
                    });
                });
                ui.label(RichText::new(format!("sha256 {short_sha}")).monospace().small().color(MUTED));
            });
            ui.add_space(4.0);
        }
    });

    if let Some(patch) = to_install {
        app.install_patch(patch);
    }
}

/// Result of one update check: a newer release, no newer release, or an error text.
type UpdateOutcome = Result<Option<Release>, String>;

/// Update check state kept in egui memory between frames.
#[derive(Clone, Default)]
struct UpdateCheck {
    /// Nothing is requested until this is ticked.
    opt_in: bool,
    feed: String,
    /// Filled by the worker thread while a check runs.
    pending: Option<Arc<Mutex<Option<UpdateOutcome>>>>,
}

/// Opt-in update check. The request runs on a worker thread and its result is
/// shown once as a notice.
fn update_card(app: &mut App, ui: &mut egui::Ui) {
    let id = egui::Id::unique(UPDATE_STATE_ID);
    let mut state: UpdateCheck = ui.ctx().data_mut(|data| data.get_temp(id)).unwrap_or_default();

    if let Some(slot) = state.pending.clone() {
        let done = slot.lock().ok().and_then(|mut guard| guard.take());
        if let Some(outcome) = done {
            state.pending = None;
            match outcome {
                Ok(Some(release)) => app.push_notice(
                    format!("Version {} is available: {}", release.version, release.url),
                    true,
                ),
                Ok(None) => app.push_notice(
                    format!("You are on the latest version ({}).", env!("CARGO_PKG_VERSION")),
                    true,
                ),
                Err(e) => app.push_notice(format!("Update check failed: {e}"), false),
            }
        }
    }

    let feed_ok = state.feed.trim().starts_with("http://");
    let checking = state.pending.is_some();
    let can_check = state.opt_in && feed_ok && !checking;

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Update check");
        ui.checkbox(&mut state.opt_in, "Check for updates when I press Check");
        ui.label(
            RichText::new("Off by default. Nothing is contacted until this is ticked and you press Check.")
                .small()
                .color(MUTED),
        );
        ui.add_space(8.0);
        ui.label(RichText::new("Feed URL (http:// only)").color(MUTED));
        ui.add_enabled(
            state.opt_in,
            egui::TextEdit::singleline(&mut state.feed)
                .desired_width(f32::INFINITY)
                .hint_text("http://updates.example.com/latest.json"),
        );
        if !state.feed.trim().is_empty() && !feed_ok {
            ui.label(RichText::new("The feed URL must start with http://. https:// is not supported.").color(DANGER));
        }
        ui.label(
            RichText::new(
                "HTTPS is not supported, so the request and the reply travel unencrypted. The feed \
                 must return JSON with version, url and notes fields.",
            )
            .small()
            .color(WARN),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(can_check, egui::Button::new("Check")).clicked() {
                state.pending = Some(start_update_check(state.feed.trim().to_string(), ui.ctx()));
            }
            if checking {
                ui.spinner();
                ui.label(RichText::new("Checking…").color(MUTED));
            }
        });
    });

    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}

/// Runs `update::check` for the running version on a worker thread.
fn start_update_check(feed: String, ctx: &egui::Context) -> Arc<Mutex<Option<UpdateOutcome>>> {
    let slot = Arc::new(Mutex::new(None));
    let worker = Arc::clone(&slot);
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let outcome = Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|e| e.to_string())
            .and_then(|current| {
                update::check(&feed, UPDATE_TIMEOUT, &current).map_err(|e| e.to_string())
            });
        if let Ok(mut guard) = worker.lock() {
            *guard = Some(outcome);
        }
        ctx.request_repaint();
    });
    slot
}

/// A fixed-width status tile on the home dashboard.
fn tile(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    theme::card().show(ui, |ui| {
        ui.set_width(170.0);
        add_contents(ui);
    });
}

/// Statistics for the id-keyed cache in egui memory. The worker fills `result`.
#[derive(Clone)]
struct StatsSlot {
    /// Car and track counts of the snapshot these statistics were computed for.
    key: (usize, usize),
    result: Arc<Mutex<Option<ContentStats>>>,
}

/// Statistics for the current snapshot, or `None` while the worker is still running.
///
/// The worker starts when the car or track count changes. It reads skin folders
/// and runs the aggregation off the UI thread, then wakes the UI with a repaint.
fn content_stats(app: &App, ui: &egui::Ui) -> Option<ContentStats> {
    let key = (app.snapshot.cars.len(), app.snapshot.tracks.len());
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(STATS_ID);

    let cached = ctx.data_mut(|data| data.get_temp::<StatsSlot>(id));
    if let Some(slot) = cached.filter(|slot| slot.key == key) {
        return slot.result.lock().ok().and_then(|guard| guard.as_ref().cloned());
    }

    let result = Arc::new(Mutex::new(None));
    ctx.data_mut(|data| {
        data.insert_temp(id, StatsSlot { key, result: Arc::clone(&result) });
    });

    let cars = app.snapshot.cars.clone();
    let tracks = app.snapshot.tracks.clone();
    let root = app.settings.ac_root_path();
    std::thread::spawn(move || {
        let skin_counts = root.map(|root| skins::scan_all_skin_counts(&root)).unwrap_or_default();
        let stats = dashboard_stats::compute(&cars, &tracks, &skin_counts);
        if let Ok(mut guard) = result.lock() {
            *guard = Some(stats);
        }
        ctx.request_repaint();
    });
    None
}

/// Content statistics: stat tiles, then bar lists by class, brand and country.
fn content_section(ui: &mut egui::Ui, ac_valid: bool, stats: Option<ContentStats>) {
    widgets::section_title(ui, "Content");
    if !ac_valid {
        ui.label(RichText::new("Set your Assetto Corsa folder to see content statistics.").color(MUTED));
        return;
    }
    let Some(stats) = stats else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Counting content…").color(MUTED));
        });
        return;
    };

    let untagged_color = if stats.untagged_cars > 0 { WARN } else { TEXT };
    ui.horizontal_wrapped(|ui| {
        tile(ui, |ui| widgets::stat(ui, "Cars", &stats.car_total.to_string(), TEXT));
        tile(ui, |ui| widgets::stat(ui, "Tracks", &stats.track_total.to_string(), TEXT));
        tile(ui, |ui| widgets::stat(ui, "Skins", &stats.skin_total.to_string(), TEXT));
        tile(ui, |ui| widgets::stat(ui, "Track layouts", &stats.layout_total.to_string(), TEXT));
        tile(ui, |ui| widgets::stat(ui, "Pitboxes", &stats.total_pitboxes.to_string(), TEXT));
        tile(ui, |ui| widgets::stat(ui, "Untagged cars", &stats.untagged_cars.to_string(), untagged_color));
        tile(ui, |ui| widgets::stat(ui, "Longest track", &metres(stats.longest_track_m), TEXT));
        tile(ui, |ui| widgets::stat(ui, "Shortest track", &metres(stats.shortest_track_m), TEXT));
    });

    ui.add_space(12.0);
    ui.columns(2, |cols| {
        bar_card(&mut cols[0], "Cars by class", &stats.cars_by_class);
        bar_card(&mut cols[1], "Cars by brand", &stats.cars_by_brand);
    });
    ui.add_space(12.0);
    bar_card(ui, "Tracks by country", &stats.tracks_by_country);
}

/// A card with the top entries of a count as horizontal bars, scaled to the largest.
fn bar_card(ui: &mut egui::Ui, title: &str, entries: &[CountEntry]) {
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(title).strong());
        ui.add_space(6.0);
        if entries.is_empty() {
            ui.label(RichText::new("Nothing to show yet.").small().color(MUTED));
            return;
        }

        let shown = dashboard_stats::top_n(entries, TOP_N);
        let largest = shown.first().map_or(1, |entry| entry.count).max(1) as f32;
        for entry in shown {
            ui.horizontal(|ui| {
                ui.add_sized([130.0, 18.0], egui::Label::new(RichText::new(entry.label.as_str())).truncate());
                let width = (ui.available_width() - 44.0).max(40.0);
                ui.add(
                    egui::ProgressBar::new(entry.count as f32 / largest)
                        .desired_width(width)
                        .fill(theme::ACCENT.gamma_multiply(0.8)),
                );
                ui.label(RichText::new(entry.count.to_string()).strong());
            });
        }

        let hidden = entries.len().saturating_sub(shown.len());
        if hidden > 0 {
            ui.label(RichText::new(format!("and {hidden} more")).small().color(MUTED));
        }
    });
}

/// Track length in metres, or a dash when no track has one.
fn metres(length: Option<f64>) -> String {
    length.map(|m| format!("{m:.0} m")).unwrap_or_else(|| "—".to_string())
}

/// Join the non-empty parts with a middle dot, for list subtitles.
fn join_parts(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("  ·  ")
}

fn dash_if_empty(text: &str) -> &str {
    if text.is_empty() { "—" } else { text }
}

fn car_row(ui: &mut egui::Ui, car: &CarEntry) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(car.name.as_str()).strong());
        if !car.car_class.is_empty() {
            widgets::pill(ui, &car.car_class, INFO);
        }
    });
    let subtitle = join_parts(&[car.brand.as_str(), car.year.as_str()]);
    ui.label(RichText::new(subtitle).small().color(MUTED));
}

fn car_detail(ui: &mut egui::Ui, car: Option<&CarEntry>) {
    let Some(car) = car else {
        widgets::empty_state(ui, "Select a car to see its details.");
        return;
    };
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(car.name.as_str()).size(20.0).strong());
        ui.add_space(6.0);
        if !car.tags.is_empty() || !car.car_class.is_empty() {
            ui.horizontal_wrapped(|ui| {
                if !car.car_class.is_empty() {
                    widgets::pill(ui, &car.car_class, INFO);
                }
                for tag in &car.tags {
                    widgets::pill(ui, tag, MUTED);
                }
            });
            ui.add_space(6.0);
        }
        widgets::key_value(ui, "Brand", dash_if_empty(&car.brand));
        widgets::key_value(ui, "Class", dash_if_empty(&car.car_class));
        widgets::key_value(ui, "Year", dash_if_empty(&car.year));
        widgets::key_value(ui, "Folder", &car.id);
        widgets::key_value(ui, "Path", &car.path.display().to_string());
    });
}

fn track_row(ui: &mut egui::Ui, track: &TrackEntry) {
    ui.label(RichText::new(track.name.as_str()).strong());
    let length = track.length_m.map(|m| format!("{m:.0} m")).unwrap_or_default();
    let subtitle = join_parts(&[track.country.as_str(), track.city.as_str(), length.as_str()]);
    ui.label(RichText::new(subtitle).small().color(MUTED));
}

fn track_detail(ui: &mut egui::Ui, track: Option<&TrackEntry>) {
    let Some(track) = track else {
        widgets::empty_state(ui, "Select a track to see its details.");
        return;
    };
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(track.name.as_str()).size(20.0).strong());
        let place = join_parts(&[track.country.as_str(), track.city.as_str()]);
        ui.label(RichText::new(place).color(MUTED));
        ui.add_space(10.0);

        let length = track.length_m.map(|m| format!("{m:.0} m")).unwrap_or_else(|| "—".into());
        let pitboxes = track.pitboxes.map(|p| p.to_string()).unwrap_or_else(|| "—".into());
        let layouts = if track.layouts.is_empty() {
            "Main layout only".to_string()
        } else {
            track.layouts.join(", ")
        };
        widgets::key_value(ui, "Length", &length);
        widgets::key_value(ui, "Pitboxes", &pitboxes);
        widgets::key_value(ui, "Layouts", &layouts);
        widgets::key_value(ui, "Country", dash_if_empty(&track.country));
        widgets::key_value(ui, "City", dash_if_empty(&track.city));
        widgets::key_value(ui, "Folder", &track.id);
        widgets::key_value(ui, "Path", &track.path.display().to_string());
    });
}
