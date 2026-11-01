//! The pages of the main area: Home, Cars, Tracks, the CSP lights patch and Settings.
//!
//! Pages only read `App` and report user actions by calling its methods, which
//! run the real work on background threads.

use super::theme::{self, DANGER, INFO, MUTED, OK, TEXT, WARN};
use super::media;
use super::widgets;
use super::tools::PickKind;
use super::{App, Page};
use crate::content::{self, CarEntry, TrackEntry};
use crate::csp::{self, LightsPatch, UNRECOGNIZED};
use acxs_manager::dashboard_stats::{self, ContentStats, CountEntry};
use acxs_manager::driving_stats::{self, DrivingStats};
use acxs_manager::skins;
use acxs_manager::update::{self, Release, Version};
use acxs_manager::{crash_report, settings};
use eframe::egui::{self, RichText};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Most entries shown in each home bar list.
const TOP_N: usize = 5;
/// egui memory id of the cached home statistics.
const STATS_ID: &str = "home_content_stats";
/// egui memory id of the update check card state.
const UPDATE_STATE_ID: &str = "home_update_check";
/// Upper bound for one update check, connect through reply.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn home(app: &mut App, ui: &mut egui::Ui) {
    if !app.snapshot.ac_valid {
        // The first scan is still running: do not flash the setup screen at a valid install.
        if app.busy {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| ui.spinner());
        } else {
            onboarding(app, ui);
        }
        return;
    }

    let game_running = app.snapshot.game_running;
    let installed = app.snapshot.installed.clone();
    let stats = content_stats(app, ui);
    let featured = featured_car(app);
    let mut goto: Option<Page> = None;
    let mut surprise = false;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Banner: a car from the library behind the one thing you most likely want to do.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 230.0), egui::Sense::hover());
        let round = egui::CornerRadius::same(theme::RADIUS + 4);
        match featured.as_ref().and_then(|car| media::car_preview(&car.path, None)) {
            Some(path) => media::paint_cover(ui, rect, &path, round),
            None => {
                ui.painter().rect_filled(rect, round, theme::CARD);
            }
        }
        let mut mesh = egui::Mesh::default();
        let dark = egui::Color32::from_black_alpha(235);
        let clear = egui::Color32::from_black_alpha(20);
        mesh.colored_vertex(rect.left_top(), dark);
        mesh.colored_vertex(rect.left_bottom(), dark);
        mesh.colored_vertex(rect.right_bottom(), clear);
        mesh.colored_vertex(rect.right_top(), clear);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(rect, round, egui::Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);

        let inner = rect.shrink2(egui::vec2(30.0, 26.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let (title, color) = if game_running {
                ("Assetto Corsa is running", OK)
            } else {
                ("Ready to race", egui::Color32::WHITE)
            };
            ui.label(RichText::new(title).font(theme::bold(32.0)).color(color));
            let csp = match installed.as_deref() {
                None => "No CSP lights patch installed".to_string(),
                Some(UNRECOGNIZED) => "Custom CSP build installed".to_string(),
                Some(version) => format!("CSP lights patch {version}"),
            };
            if let Some(car) = &featured {
                let name = if car.brand.is_empty() { car.name.clone() } else { format!("{}  ·  {}", car.name, car.brand) };
                ui.label(RichText::new(format!("Featured: {name}")).size(14.0).color(egui::Color32::from_gray(210)));
            }
            ui.label(RichText::new(csp).size(13.0).color(egui::Color32::from_gray(160)));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let launch = ui.add_enabled(
                    !game_running,
                    widgets::primary_button("Launch Assetto Corsa").min_size(egui::vec2(190.0, 42.0)),
                );
                if launch.clicked() {
                    app.launch_game();
                }
                if featured.is_some() {
                    if ui.add(egui::Button::new("Open in showroom").min_size(egui::vec2(0.0, 42.0))).clicked() {
                        goto = Some(Page::Showroom);
                    }
                    if ui.add(egui::Button::new("Surprise me").min_size(egui::vec2(0.0, 42.0))).clicked() {
                        surprise = true;
                    }
                }
            });
        });

        // Leaving the banner's inner scope moves the cursor up into it; step back below the banner.
        let below = rect.bottom() - ui.cursor().min.y;
        if below > 0.0 {
            ui.add_space(below);
        }
        ui.add_space(18.0);
        // Jump-off cards.
        let actions: [(&str, &str, &str); 4] = [
            ("Showroom", "Cars up close, skins and specs", "showroom"),
            ("Servers", "Live status of your servers", "servers"),
            ("Quick Race", "Pick car, track and go", "quick"),
            ("Lights Patch", "Install or archive CSP lights", "patch"),
        ];
        widgets::tile_grid(ui, 200.0, actions.len(), |ui, i| {
            let (title, text, key) = actions[i];
            let response = theme::card()
                .inner_margin(egui::Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new(title).font(theme::bold(15.0)));
                        ui.add(egui::Label::new(RichText::new(text).size(12.5).color(MUTED)).truncate());
                    });
                })
                .response
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.hovered() {
                ui.painter().rect_stroke(
                    response.rect,
                    egui::CornerRadius::same(theme::RADIUS),
                    egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)),
                    egui::StrokeKind::Inside,
                );
            }
            if response.clicked() {
                goto = Some(match key {
                    "showroom" => Page::Showroom,
                    "patch" => Page::Patches,
                    "servers" => Page::Servers,
                    _ => Page::Tool(app.tool_index("Quick Race")),
                });
            }
        });

        ui.add_space(20.0);
        driving_section(app, ui);
        ui.add_space(20.0);
        content_section(ui, true, stats);
        ui.add_space(8.0);
    });

    if surprise {
        app.pick_random_car();
    }
    if let Some(page) = goto {
        if page == Page::Showroom {
            app.showroom_car = featured.map(|c| c.id);
            app.car_skin = None;
        }
        app.page = page;
    }
}

/// The car on the home banner: the picked one, else a favourite or the car of the day.
fn featured_car(app: &App) -> Option<CarEntry> {
    let cars = &app.snapshot.cars;
    if let Some(car) = app.home_pick.as_deref().and_then(|id| cars.iter().find(|c| c.id == id)) {
        return Some(car.clone());
    }
    if cars.is_empty() {
        return None;
    }
    let day = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as usize;
    let pool: Vec<&CarEntry> = {
        let favs: Vec<&CarEntry> = cars.iter().filter(|c| app.favorites.contains(&c.id)).collect();
        if favs.is_empty() { cars.iter().collect() } else { favs }
    };
    (0..pool.len().min(60))
        .map(|offset| pool[(day + offset) % pool.len()])
        .find(|car| media::car_preview(&car.path, None).is_some())
        .or_else(|| pool.get(day % pool.len()).copied())
        .cloned()
}

/// First-run screen: nothing else works until the install folder is known.
fn onboarding(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(560.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                ui.label(RichText::new("Welcome to ACXS Manager").font(theme::bold(26.0)));
                ui.add_space(4.0);
                ui.label(RichText::new("Tell me where Assetto Corsa lives and everything else lights up.").color(MUTED));
                ui.add_space(18.0);
                path_row(
                    ui,
                    "Assetto Corsa folder (contains acs.exe)",
                    r"e.g. C:\Program Files (x86)\Steam\steamapps\common\assettocorsa",
                    &mut app.ac_input,
                );
                ui.add_space(12.0);
                if !app.ac_input.trim().is_empty() {
                    if settings::is_ac_root(Path::new(app.ac_input.trim())) {
                        ui.label(RichText::new("acs.exe found.").color(OK));
                    } else {
                        ui.label(RichText::new("No acs.exe in that folder.").color(WARN));
                    }
                    ui.add_space(8.0);
                }
                if ui.add(widgets::primary_button("Save and scan")).clicked() {
                    app.save_paths();
                }
            });
        });
    });
}

pub fn settings(app: &mut App, ui: &mut egui::Ui) {
    widgets::heading(ui, "Settings", "Folders, updates and crash reporting.");
    let ac_valid = app.snapshot.ac_valid;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(820.0);
        update_card(app, ui);
        ui.add_space(16.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Folders");
            path_row(ui, "Assetto Corsa folder (the one containing acs.exe)", "Not set", &mut app.ac_input);
            ui.add_space(10.0);
            path_row(
                ui,
                "Patch library (where lights patch folders live)",
                "Default: the folder the program runs from",
                &mut app.library_input,
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add(widgets::primary_button("Save folders")).clicked() {
                    app.save_paths();
                }
                if !ac_valid {
                    ui.label(RichText::new("Choose the folder that contains acs.exe, then save.").color(WARN));
                }
            });
        });


        ui.add_space(16.0);
        crash_card(app, ui);

        ui.add_space(16.0);
        about_card(ui);
        ui.add_space(8.0);
    });
}

/// A labelled path field with a Browse button.
fn path_row(ui: &mut egui::Ui, label: &str, hint: &str, value: &mut String) {
    ui.label(RichText::new(label).color(MUTED));
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 90.0).max(160.0);
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(width)
                .hint_text(hint)
                .margin(egui::Margin::symmetric(10, 7)),
        );
        if ui.button("Browse").clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            *value = path.display().to_string();
        }
    });
}

/// Search field plus result count, shared by the Cars and Tracks pages.
fn list_toolbar(ui: &mut egui::Ui, query: &mut String, hint: &str, shown: usize, total: usize, noun: &str) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(query)
                .hint_text(hint)
                .desired_width((ui.available_width() - 130.0).max(160.0))
                .margin(egui::Margin::symmetric(10, 7)),
        );
        ui.label(RichText::new(format!("{shown} of {total} {noun}")).size(12.5).color(MUTED));
    });
    ui.add_space(4.0);
}

/// The panel that holds the selected item's details.
fn detail_panel(id: &'static str) -> egui::Panel {
    egui::Panel::right(id)
        .resizable(true)
        .default_size(360.0)
        .size_range(300.0..=560.0)
        .frame(
            egui::Frame::new()
                .fill(theme::SIDEBAR)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::same(18)),
        )
}

/// Open a folder in the system file browser.
pub(super) fn open_folder(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}


const CAR_FILTERS: [&str; 5] = ["All", "★ Favorites", "Race", "Street", "Other"];
const CAR_SORTS: [&str; 3] = ["Name", "Brand", "Newest"];

/// The page heading, or a banner with Cancel while this page is choosing for Quick Race.
fn heading_or_picker(app: &mut App, ui: &mut egui::Ui, kind: PickKind, title: &str, subtitle: &str) {
    if app.picker != Some(kind) {
        widgets::heading(ui, title, subtitle);
        return;
    }
    let what = if kind == PickKind::Car { "car" } else { "track" };
    let how = if kind == PickKind::Car {
        "Click a car, pick its skin on the right, then press Choose."
    } else {
        "Click a track, pick its layout on the right, then press Choose."
    };
    let mut cancel = false;
    egui::Frame::new()
        .fill(theme::CARD_SELECTED)
        .stroke(egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Choose a {what} for Quick Race")).font(theme::bold(20.0)));
                    ui.label(RichText::new(how).color(MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });
    ui.add_space(12.0);
    if cancel {
        app.picker = None;
        app.page = Page::Tool(app.tool_index("Quick Race"));
    }
}

pub fn cars(app: &mut App, ui: &mut egui::Ui) {
    heading_or_picker(app, ui, PickKind::Car, "Cars", "Everything installed. Click for details, double-click for the showroom, right-click for more.");
    if !app.snapshot.ac_valid {
        widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to see cars.");
        return;
    }
    let mut star: Option<String> = None;
    let mut open: Option<String> = None;
    {
        let App { snapshot, car_query, selected_car, car_skin, favorites, car_filter, car_sort, grid_columns, race_request, picker, .. } = &mut *app;

        detail_panel("car_detail").show(ui, |ui| {
            let car = selected_car
                .as_deref()
                .and_then(|id| snapshot.cars.iter().find(|c| c.id == id));
            car_detail(ui, car, car_skin, race_request, picker.is_some());
        });

        let mut filtered = content::filter_cars(&snapshot.cars, car_query.as_str());
        filtered.retain(|car| {
            let class = car.car_class.to_lowercase();
            match car_filter.as_str() {
                "★ Favorites" => favorites.contains(&car.id),
                "Race" => class == "race",
                "Street" => class == "street",
                "Other" => class != "race" && class != "street",
                _ => true,
            }
        });
        match *car_sort {
            1 => filtered.sort_by_key(|c| (c.brand.to_lowercase(), c.name.to_lowercase())),
            2 => filtered.sort_by(|a, b| b.year.cmp(&a.year).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))),
            _ => filtered.sort_by_key(|c| c.name.to_lowercase()),
        }

        list_toolbar(ui, car_query, "Search name, brand, class or tag", filtered.len(), snapshot.cars.len(), "cars");
        ui.horizontal_wrapped(|ui| {
            for label in CAR_FILTERS {
                if ui.selectable_label(car_filter == label, label).clicked() {
                    *car_filter = label.to_string();
                }
            }
            egui::ComboBox::from_id_salt("car_sort")
                .selected_text(format!("Sort: {}", CAR_SORTS[(*car_sort).min(2)]))
                .show_ui(ui, |ui| {
                    for (i, name) in CAR_SORTS.iter().enumerate() {
                        ui.selectable_value(car_sort, i, *name);
                    }
                });
        });
        ui.add_space(6.0);

        if filtered.is_empty() {
            let text = if snapshot.cars.is_empty() {
                "No cars found in content/cars."
            } else if car_filter == "★ Favorites" {
                "No favorites yet. Hover a car and click its star."
            } else {
                "No cars match your search."
            };
            widgets::empty_state(ui, text);
        } else {
            let cur_idx = selected_car.as_deref().and_then(|id| filtered.iter().position(|c| c.id == id));
            let (moved, enter) = grid_keys(ui, filtered.len(), cur_idx, *grid_columns);
            let mut scroll_to = None;
            if let Some(i) = moved {
                *selected_car = Some(filtered[i].id.clone());
                *car_skin = None;
                scroll_to = Some(i);
            }
            if enter {
                open = selected_car.clone();
            }
            let current = selected_car.clone();
            let favs = favorites.clone();
            let spec = media::GridSpec { min_width: 220.0, info_height: 58.0, stars: true, scroll_to };
            let out = media::grid(
                ui,
                &filtered,
                &spec,
                |car| current.as_deref() == Some(car.id.as_str()),
                |car| favs.contains(&car.id),
                |ui, image, info, car| {
                    let preview = media::car_preview(&car.path, None);
                    media::paint_image(ui, image, preview.as_deref(), &car.name, INFO, media::top_round());
                    if !car.car_class.is_empty() {
                        let color = media::class_color(&car.car_class);
                        media::chip(ui, image.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP, &car.car_class, color);
                    }
                    card_text(ui, info, &car.name, &join_parts(&[car.brand.as_str(), car.year.as_str()]));
                },
                |ui, index, car, out| {
                    if ui.button("Open in showroom").clicked() {
                        out.opened = Some(index);
                        ui.close();
                    }
                    let label = if favs.contains(&car.id) { "Remove from favorites" } else { "Add to favorites" };
                    if ui.button(label).clicked() {
                        out.star = Some(index);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Open folder").clicked() {
                        open_folder(&car.path);
                        ui.close();
                    }
                    if ui.button("Copy folder name").clicked() {
                        ui.ctx().copy_text(car.id.clone());
                        ui.close();
                    }
                },
            );
            *grid_columns = out.columns;
            if let Some(i) = out.clicked {
                *selected_car = Some(filtered[i].id.clone());
                *car_skin = None;
            }
            if let Some(i) = out.star {
                star = Some(filtered[i].id.clone());
            }
            if let Some(i) = out.opened {
                open = Some(filtered[i].id.clone());
            }
        }
    }
    if let Some(id) = star {
        app.toggle_favorite(&id);
    }
    if let Some(id) = open {
        if app.picker == Some(PickKind::Car) {
            app.race_request = Some(RaceAction { car: Some(id), ..RaceAction::default() });
        } else {
            app.showroom_car = Some(id.clone());
            app.selected_car = Some(id);
            app.car_skin = None;
            app.page = Page::Showroom;
        }
    }
}

pub fn tracks(app: &mut App, ui: &mut egui::Ui) {
    heading_or_picker(app, ui, PickKind::Track, "Tracks", "Every circuit with its layouts. Pick one for the map and details.");
    let App { snapshot, track_query, selected_track, track_layout, grid_columns, race_request, picker, .. } = app;
    if !snapshot.ac_valid {
        widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to see tracks.");
        return;
    }

    detail_panel("track_detail").show(ui, |ui| {
        let track = selected_track
            .as_deref()
            .and_then(|id| snapshot.tracks.iter().find(|t| t.id == id));
        track_detail(ui, track, track_layout, race_request, picker.is_some());
    });

    let filtered = content::filter_tracks(&snapshot.tracks, track_query.as_str());
    list_toolbar(ui, track_query, "Search name, country or city", filtered.len(), snapshot.tracks.len(), "tracks");
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

    let cur_idx = selected_track.as_deref().and_then(|id| filtered.iter().position(|t| t.id == id));
    let (moved, _) = grid_keys(ui, filtered.len(), cur_idx, *grid_columns);
    let mut scroll_to = None;
    if let Some(i) = moved {
        *selected_track = Some(filtered[i].id.clone());
        *track_layout = None;
        scroll_to = Some(i);
    }
    let current = selected_track.clone();
    let spec = media::GridSpec { min_width: 250.0, info_height: 58.0, stars: false, scroll_to };
    let out = media::grid(
        ui,
        &filtered,
        &spec,
        |track| current.as_deref() == Some(track.id.as_str()),
        |_| false,
        |ui, image, info, track| {
            let preview = media::track_preview(track, None);
            media::paint_image(ui, image, preview.as_deref(), &track.name, theme::ACCENT, media::top_round());
            if let Some(m) = track.length_m {
                let text = format!("{:.1} km", m / 1000.0);
                media::chip(ui, image.right_bottom() - egui::vec2(8.0, 8.0), egui::Align2::RIGHT_BOTTOM, &text, TEXT);
            }
            if track.layouts.len() > 1 {
                let text = format!("{} layouts", track.layouts.len());
                media::chip(ui, image.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP, &text, INFO);
            }
            card_text(ui, info, &track.name, &join_parts(&[track.country.as_str(), track.city.as_str()]));
        },
        |ui, _index, track, _out| {
            if ui.button("Open folder").clicked() {
                open_folder(&track.path);
                ui.close();
            }
            if ui.button("Copy folder name").clicked() {
                ui.ctx().copy_text(track.id.clone());
                ui.close();
            }
        },
    );
    if picker.is_some()
        && let Some(i) = out.opened
    {
        *race_request = Some(RaceAction { track: Some(filtered[i].id.clone()), ..RaceAction::default() });
    }
    *grid_columns = out.columns;
    if let Some(index) = out.clicked.or(out.opened) {
        *selected_track = Some(filtered[index].id.clone());
        *track_layout = None;
    }
}

/// Title and subtitle at the bottom of a grid card.
fn card_text(ui: &egui::Ui, info: egui::Rect, title: &str, subtitle: &str) {
    let width = info.width() - 20.0;
    let galley = media::clipped(ui, title, theme::bold(14.0), TEXT, width);
    ui.painter().galley(info.left_top() + egui::vec2(10.0, 9.0), galley, TEXT);
    if !subtitle.is_empty() {
        let galley = media::clipped(ui, subtitle, egui::FontId::proportional(12.0), MUTED, width);
        ui.painter().galley(info.left_top() + egui::vec2(10.0, 31.0), galley, MUTED);
    }
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
                ui.label(RichText::new("Set your Assetto Corsa folder in Settings before installing.").color(WARN));
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

/// What the last check found, kept to show on the card.
#[derive(Clone)]
enum UpdateResult {
    Newer { version: String, url: String, notes: String },
    Latest,
    Failed(String),
}

/// Update check state kept in egui memory between frames.
#[derive(Clone)]
struct UpdateCheck {
    feed: String,
    /// Filled by the worker thread while a check runs.
    pending: Option<Arc<Mutex<Option<UpdateOutcome>>>>,
    last: Option<UpdateResult>,
}

/// The project's own update feed, served over HTTPS.
const DEFAULT_FEED: &str = "https://test2.azendix.de/acxs/latest.json";

/// egui memory flag the sidebar sets to start a check as soon as Settings opens.
pub(super) const UPDATE_RUN_ID: &str = "update_run_request";

impl Default for UpdateCheck {
    fn default() -> Self {
        UpdateCheck { feed: DEFAULT_FEED.to_string(), pending: None, last: None }
    }
}

/// A link is opened only when it is a plain web address: no spaces, quotes or shell characters.
fn safe_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '|' | '^' | '`'))
}

fn open_url(url: &str) {
    if !safe_web_url(url) {
        return;
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", &url.replace('&', "^&")]).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Updates: the running version, a Check button, and what the last check found. The request
/// only happens when the button is pressed (or "Check for updates" is clicked in the sidebar),
/// and runs on a worker thread.
fn update_card(app: &mut App, ui: &mut egui::Ui) {
    let id = egui::Id::unique(UPDATE_STATE_ID);
    let mut state: UpdateCheck = ui.ctx().data_mut(|data| data.get_temp(id)).unwrap_or_default();
    let run_id = egui::Id::unique(UPDATE_RUN_ID);
    let run_requested: bool = ui.ctx().data_mut(|data| data.get_temp(run_id)).unwrap_or(false);
    if run_requested {
        ui.ctx().data_mut(|data| data.insert_temp(run_id, false));
    }

    if let Some(slot) = state.pending.clone() {
        let done = slot.lock().ok().and_then(|mut guard| guard.take());
        if let Some(outcome) = done {
            state.pending = None;
            state.last = Some(match outcome {
                Ok(Some(release)) => {
                    app.push_notice(format!("Version {} is available.", release.version), true);
                    UpdateResult::Newer {
                        version: release.version.to_string(),
                        url: release.url,
                        notes: release.notes,
                    }
                }
                Ok(None) => UpdateResult::Latest,
                Err(e) => {
                    app.push_notice(format!("Update check failed: {e}"), false);
                    UpdateResult::Failed(e)
                }
            });
        }
    }

    let feed = state.feed.trim().to_ascii_lowercase();
    let feed_ok = feed.starts_with("https://") || feed.starts_with("http://");
    let plain_http = feed.starts_with("http://");
    let checking = state.pending.is_some();
    let can_check = feed_ok && !checking;

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Updates").font(theme::bold(15.0)));
                ui.label(RichText::new(format!("You are on version {}", env!("CARGO_PKG_VERSION"))).color(MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = widgets::primary_button("Check for updates").min_size(egui::vec2(170.0, 36.0));
                if (ui.add_enabled(can_check, button).clicked() || (run_requested && can_check)) && !checking {
                    state.pending = Some(start_update_check(state.feed.trim().to_string(), ui.ctx()));
                }
                if checking {
                    ui.spinner();
                }
            });
        });
        ui.label(
            RichText::new("Nothing is contacted until you press the button.").size(12.0).color(theme::FAINT),
        );

        match &state.last {
            Some(UpdateResult::Newer { version, url, notes }) => {
                ui.add_space(10.0);
                theme::card_flat().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(format!("Version {version} is available")).font(theme::bold(15.0)).color(OK));
                    for line in notes.lines().filter(|l| !l.trim().is_empty()).take(6) {
                        ui.label(RichText::new(line.trim()).size(12.5).color(MUTED));
                    }
                    ui.add_space(6.0);
                    if safe_web_url(url) {
                        if ui.add(widgets::primary_button("Open the download page")).clicked() {
                            open_url(url);
                        }
                    } else {
                        ui.label(RichText::new("The feed's download link is not a plain web address, so it is not opened.").color(WARN));
                    }
                });
            }
            Some(UpdateResult::Latest) => {
                ui.add_space(8.0);
                ui.label(RichText::new("You have the latest version.").color(OK));
            }
            Some(UpdateResult::Failed(error)) => {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("The check failed: {error}")).color(DANGER));
            }
            None => {}
        }

        ui.add_space(6.0);
        egui::CollapsingHeader::new(RichText::new("Update feed").size(13.0)).show(ui, |ui| {
            ui.label(RichText::new("The feed must return JSON with version, url and notes fields.").size(12.0).color(MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut state.feed)
                    .desired_width(f32::INFINITY)
                    .hint_text("https://updates.example.com/latest.json"),
            );
            if !state.feed.trim().is_empty() && !feed_ok {
                ui.label(RichText::new("The feed URL must start with https:// or http://.").color(DANGER));
            }
            if plain_http {
                ui.label(
                    RichText::new(
                        "Plain http:// travels unencrypted, so the reply (and the download link in it) \
                         could be changed on the way. Use https:// when the server offers it.",
                    )
                    .small()
                    .color(WARN),
                );
            }
            if ui.small_button("Use the default feed").clicked() {
                state.feed = DEFAULT_FEED.to_string();
            }
        });
    });

    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}

const CRASH_COUNT_ID: &str = "home_crash_count";
/// How long the count of waiting crash reports is reused before the folder is read again.
const CRASH_COUNT_TTL: Duration = Duration::from_secs(5);

/// Opt-in crash report sending, and how many stored reports are waiting.
fn crash_card(app: &mut App, ui: &mut egui::Ui) {
    let id = egui::Id::unique(CRASH_COUNT_ID);
    let cached: Option<(Instant, usize)> = ui.ctx().data_mut(|data| data.get_temp(id));
    let waiting = match cached {
        Some((at, count)) if at.elapsed() < CRASH_COUNT_TTL => count,
        _ => {
            let settings_file = settings::settings_path();
            let dir = crash_report::pending_dir(settings_file.parent().unwrap_or(Path::new(".")));
            let count = crash_report::load_pending(&dir).len();
            ui.ctx().data_mut(|data| data.insert_temp(id, (Instant::now(), count)));
            count
        }
    };

    theme::card().show(ui, |ui| {
        widgets::section_title(ui, "Crash reports");
        let mut enabled = app.settings.crash_reports;
        if ui
            .checkbox(&mut enabled, "Send crash reports to the developer (anonymous, opt-in)")
            .changed()
        {
            app.settings.crash_reports = enabled;
            match app.settings.save(&settings::settings_path()) {
                Ok(()) if enabled => app.push_notice("Crash reports will be sent when the app starts.", true),
                Ok(()) => app.push_notice("Crash reports will not be sent.", true),
                Err(e) => app.push_notice(format!("Could not save settings: {e}"), false),
            }
        }
        ui.label(
            RichText::new(
                "Only the error message, where in the code it happened, the stack trace, app version and OS \
                 are sent to the developer's own GlitchTip server (test6.azendix.de). Paths in your user \
                 folder are removed. Off by default.",
            )
            .small()
            .color(MUTED),
        );
        if waiting > 0 {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{waiting} report(s) waiting")).color(MUTED));
        }
    });
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

/// A stat tile that fills its grid cell.
fn tile(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    theme::card().inner_margin(egui::Margin::symmetric(16, 14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
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

/// egui memory id of the imported driving history.
const DRIVING_ID: &str = "home_driving_stats";

type DrivingSlot = Arc<Mutex<Option<DrivingStats>>>;

/// Driving statistics: the Content Manager profile plus sessions recorded by ACXS, loaded on a
/// worker thread and reloaded when the recorder saves a session.
fn driving_stats(app: &App, ui: &egui::Ui) -> Option<DrivingStats> {
    let saved = app.recorder.saved.load(std::sync::atomic::Ordering::Relaxed);
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(DRIVING_ID);
    if let Some((key, slot)) = ctx.data_mut(|data| data.get_temp::<(usize, DrivingSlot)>(id))
        && key == saved
    {
        return slot.lock().ok().and_then(|guard| guard.as_ref().cloned());
    }
    let slot: DrivingSlot = Arc::new(Mutex::new(None));
    ctx.data_mut(|data| data.insert_temp(id, (saved, Arc::clone(&slot))));
    std::thread::spawn(move || {
        let imported = driving_stats::default_profile_path()
            .map(|path| driving_stats::load_profile(&path))
            .unwrap_or_default();
        let recorded_sessions = acxs_manager::session_recorder::load_all(&acxs_manager::session_recorder::sessions_dir());
        let mut rows = driving_stats::load_cm_laps();
        for s in &recorded_sessions {
            rows.extend(s.laps.iter().filter(|l| l.time_ms >= 10_000).map(|l| driving_stats::LapRow {
                car_id: s.car_id.clone(),
                track_key: s.full_track_id(),
                time_ms: l.time_ms as u32,
            }));
        }
        let recorded = recorded_sessions.iter().map(acxs_manager::session_recorder::RecordedSession::to_driven).collect();
        let all = driving_stats::merge(imported, recorded);
        if let Ok(mut guard) = slot.lock() {
            let mut stats = driving_stats::summarize(&all);
            stats.bests = driving_stats::personal_bests(&rows);
            *guard = Some(stats);
        }
        ctx.request_repaint();
    });
    None
}

/// Minutes per label, merging entries whose display names collide.
fn usage_bars(list: &[driving_stats::UsageEntry], name: &dyn Fn(&str) -> String) -> Vec<CountEntry> {
    let mut merged: Vec<CountEntry> = Vec::new();
    for entry in list {
        let label = name(&entry.id);
        let minutes = (entry.seconds / 60.0).round() as usize;
        match merged.iter_mut().find(|m| m.label == label) {
            Some(existing) => existing.count += minutes,
            None => merged.push(CountEntry { label, count: minutes }),
        }
    }
    merged.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    merged
}

/// "Your driving": totals, plus the most driven cars and tracks and the recent weeks.
fn driving_section(app: &App, ui: &mut egui::Ui) {
    let Some(stats) = driving_stats(app, ui) else { return };
    if stats.sessions == 0 {
        return;
    }
    widgets::section_title(ui, "Your driving");
    let crash_rate = stats.crashes_per_100km.map_or("-".to_string(), |rate| format!("{rate:.1}"));
    let tiles = [
        ("Drive time", format!("{:.1} h", stats.seconds / 3600.0)),
        ("Distance", format!("{:.0} km", stats.distance_km)),
        ("Sessions", stats.sessions.to_string()),
        ("Top speed", format!("{:.0} km/h", stats.max_speed)),
        ("Crashes", stats.crashes.to_string()),
        ("Crashes / 100 km", crash_rate),
        ("Off track", stats.offroad.to_string()),
        ("Longest jump", format!("{:.1} s", stats.longest_airborne)),
    ];
    let share = |value: Option<f64>| value.map_or("-".to_string(), |v| format!("{:.0}%", v * 100.0));
    let mut tiles = tiles.to_vec();
    if stats.understeer_share.is_some() {
        tiles.extend([
            ("Understeer", share(stats.understeer_share)),
            ("Oversteer", share(stats.oversteer_share)),
            ("Brake lock-ups", stats.lockups.to_string()),
            ("Wheelspins", stats.wheelspins.to_string()),
        ]);
    }
    widgets::tile_grid(ui, 150.0, tiles.len(), |ui, i| {
        let (label, value) = &tiles[i];
        tile(ui, |ui| widgets::stat(ui, label, value, TEXT));
    });
    ui.add_space(16.0);

    let car_name = |id: &str| {
        app.snapshot.cars.iter().find(|c| c.id == id).map_or_else(|| id.to_string(), |c| c.name.clone())
    };
    let track_name = |id: &str| {
        // Ids come as `track/layout` or `track-layout`; the longest installed id that fits wins.
        let key = id.replace('/', "-");
        let found = app
            .snapshot
            .tracks
            .iter()
            .filter(|t| key == t.id || key.starts_with(&format!("{}-", t.id)))
            .max_by_key(|t| t.id.len());
        found.map_or_else(|| id.split('/').next().unwrap_or(id).to_string(), |t| t.name.clone())
    };
    let weekly: Vec<CountEntry> = stats
        .weekly_hours
        .iter()
        .rev()
        .map(|(week, hours)| CountEntry { label: week.clone(), count: (hours * 60.0).round() as usize })
        .collect();
    ui.columns(3, |cols| {
        bar_card(&mut cols[0], "Most driven cars (minutes)", &usage_bars(&stats.top_cars, &car_name));
        bar_card(&mut cols[1], "Most driven tracks (minutes)", &usage_bars(&stats.top_tracks, &track_name));
        bar_card(&mut cols[2], "Recent weeks (minutes)", &weekly);
    });
    if !stats.bests.is_empty() {
        ui.add_space(16.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Best laps").font(theme::bold(14.5)));
            ui.add_space(6.0);
            for best in stats.bests.iter().take(8) {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(car_name(&best.car_id)).size(13.0)).truncate());
                    ui.label(RichText::new(track_name(&best.track_key)).size(13.0).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(acxs_manager::laptimes::format_lap(best.time_ms)).font(theme::bold(13.0)));
                        ui.label(RichText::new(format!("{} laps", best.laps)).size(12.0).color(MUTED));
                    });
                });
            }
        });
    }
}

/// Content statistics: stat tiles, then bar lists by class, brand and country.
fn content_section(ui: &mut egui::Ui, ac_valid: bool, stats: Option<ContentStats>) {
    widgets::section_title(ui, "Your content");
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
    let tiles = [
        ("Cars", stats.car_total.to_string(), TEXT),
        ("Tracks", stats.track_total.to_string(), TEXT),
        ("Skins", stats.skin_total.to_string(), TEXT),
        ("Track layouts", stats.layout_total.to_string(), TEXT),
        ("Pitboxes", stats.total_pitboxes.to_string(), TEXT),
        ("Untagged cars", stats.untagged_cars.to_string(), untagged_color),
        ("Longest track", metres(stats.longest_track_m), TEXT),
        ("Shortest track", metres(stats.shortest_track_m), TEXT),
    ];
    widgets::tile_grid(ui, 150.0, tiles.len(), |ui, i| {
        let (label, value, color) = &tiles[i];
        tile(ui, |ui| widgets::stat(ui, label, value, *color));
    });

    ui.add_space(16.0);
    ui.columns(3, |cols| {
        bar_card(&mut cols[0], "Cars by class", &stats.cars_by_class);
        bar_card(&mut cols[1], "Cars by brand", &stats.cars_by_brand);
        bar_card(&mut cols[2], "Tracks by country", &stats.tracks_by_country);
    });
}

/// A card with the top entries of a count as horizontal bars, scaled to the largest.
fn bar_card(ui: &mut egui::Ui, title: &str, entries: &[CountEntry]) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Same height for all cards in a row, whatever the entry count.
        ui.set_min_height(236.0);
        ui.label(RichText::new(title).font(theme::bold(14.5)));
        ui.add_space(8.0);
        if entries.is_empty() {
            ui.label(RichText::new("Nothing to show yet.").small().color(MUTED));
            return;
        }

        let shown = dashboard_stats::top_n(entries, TOP_N);
        let largest = shown.first().map_or(1, |entry| entry.count).max(1) as f32;
        for entry in shown {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(entry.label.as_str()).size(13.0)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(entry.count.to_string()).font(theme::bold(13.0)));
                    });
                });
                ui.add(
                    egui::ProgressBar::new(entry.count as f32 / largest)
                        .desired_width(ui.available_width())
                        .desired_height(6.0)
                        .corner_radius(egui::CornerRadius::same(3))
                        .fill(theme::ACCENT.gamma_multiply(0.85)),
                );
            });
            ui.add_space(2.0);
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
pub(crate) fn join_parts(parts: &[&str]) -> String {
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

/// A 16:9 hero image across the width of the panel.
fn hero_image(ui: &mut egui::Ui, path: Option<&Path>, label: &str, tint: egui::Color32) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, (width * 9.0 / 16.0).round()), egui::Sense::hover());
    media::paint_image(ui, rect, path, label, tint, egui::CornerRadius::same(10));
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(1.0, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    rect
}

fn car_detail(ui: &mut egui::Ui, car: Option<&CarEntry>, skin: &mut Option<String>, race: &mut Option<RaceAction>, picking: bool) {
    let Some(car) = car else {
        widgets::empty_state(ui, "Select a car to see it here.");
        return;
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let skins = media::car_skins(&car.path);
        let current = skin.as_deref().and_then(|name| skins.iter().find(|(n, _)| n == name)).or_else(|| skins.first());
        let preview = current.map(|(_, path)| path.clone());
        let hero = hero_image(ui, preview.as_deref(), &car.name, INFO);
        if !car.car_class.is_empty() {
            let color = media::class_color(&car.car_class);
            media::chip(ui, hero.left_top() + egui::vec2(8.0, 8.0), egui::Align2::LEFT_TOP, &car.car_class, color);
        }
        if let Some((name, _)) = current {
            media::chip(ui, hero.right_bottom() - egui::vec2(8.0, 8.0), egui::Align2::RIGHT_BOTTOM, name, TEXT);
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if let Some(badge) = media::car_badge(&car.path) {
                ui.add(egui::Image::from_uri(media::uri(&badge)).fit_to_exact_size(egui::vec2(40.0, 40.0)));
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(car.name.as_str()).font(theme::bold(19.0)));
                ui.label(RichText::new(join_parts(&[car.brand.as_str(), car.year.as_str()])).color(MUTED));
            });
        });
        if !car.tags.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for tag in &car.tags {
                    widgets::pill(ui, tag, MUTED);
                }
            });
        }

        if picking {
            ui.add_space(8.0);
            let choose = widgets::primary_button("Choose this car").min_size(egui::vec2(ui.available_width(), 38.0));
            if ui.add(choose).clicked() {
                *race = Some(RaceAction {
                    car: Some(car.id.clone()),
                    skin: current.map(|(name, _)| name.clone()),
                    ..RaceAction::default()
                });
            }
        }
        if skins.len() > 1 {
            ui.add_space(12.0);
            widgets::section_title(ui, &format!("Skins ({})", skins.len()));
            let thumb = egui::vec2(76.0, 43.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for (name, path) in skins.iter() {
                    let (rect, response) = ui.allocate_exact_size(thumb, egui::Sense::click());
                    let selected = current.is_some_and(|(n, _)| n == name);
                    media::paint_image(ui, rect, Some(path), name, INFO, egui::CornerRadius::same(6));
                    let stroke = if selected {
                        egui::Stroke::new(2.0, theme::ACCENT)
                    } else if response.hovered() {
                        egui::Stroke::new(1.0, theme::BORDER_STRONG)
                    } else {
                        egui::Stroke::new(1.0, theme::BORDER)
                    };
                    ui.painter().rect_stroke(rect, egui::CornerRadius::same(6), stroke, egui::StrokeKind::Inside);
                    if response.on_hover_text(name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        *skin = Some(name.clone());
                    }
                }
            });
        }

        ui.add_space(12.0);
        theme::card_flat().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::key_value(ui, "Class", dash_if_empty(&car.car_class));
            widgets::key_value_block(ui, "Folder", &car.id);
            widgets::key_value_block(ui, "Path", &car.path.display().to_string());
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(widgets::primary_button(if picking { "Choose this car" } else { "Race this car" })).clicked() {
                *race = Some(RaceAction {
                    car: Some(car.id.clone()),
                    skin: current.map(|(name, _)| name.clone()),
                    ..RaceAction::default()
                });
            }
            if ui.button("Open folder").clicked() {
                open_folder(&car.path);
            }
        });
    });
}

fn track_detail(ui: &mut egui::Ui, track: Option<&TrackEntry>, layout: &mut Option<String>, race: &mut Option<RaceAction>, picking: bool) {
    let Some(track) = track else {
        widgets::empty_state(ui, "Select a track to see it here.");
        return;
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let layout_owned = layout.clone().filter(|l| track.layouts.iter().any(|t| t == l));
        let layout_name = layout_owned.as_deref();
        let preview = media::track_preview(track, layout_name);
        hero_image(ui, preview.as_deref(), &track.name, theme::ACCENT);

        ui.add_space(10.0);
        ui.label(RichText::new(track.name.as_str()).font(theme::bold(19.0)));
        ui.label(RichText::new(join_parts(&[track.country.as_str(), track.city.as_str()])).color(MUTED));
        if picking {
            ui.add_space(8.0);
            let choose = widgets::primary_button("Choose this track").min_size(egui::vec2(ui.available_width(), 38.0));
            if ui.add(choose).clicked() {
                *race = Some(RaceAction {
                    track: Some(track.id.clone()),
                    layout: layout_name.map(str::to_string),
                    ..RaceAction::default()
                });
            }
        }

        if track.layouts.len() > 1 {
            ui.add_space(10.0);
            widgets::section_title(ui, "Layouts");
            ui.horizontal_wrapped(|ui| {
                for name in &track.layouts {
                    let selected = layout_name == Some(name.as_str());
                    let label = name.strip_prefix("layout_").unwrap_or(name);
                    if ui.selectable_label(selected, label).clicked() {
                        *layout = Some(name.clone());
                    }
                }
            });
        }

        if let Some(outline) = media::track_outline(track, layout_name) {
            ui.add_space(10.0);
            theme::card_flat().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::from_uri(media::uri(&outline)).max_height(190.0).fit_to_original_size(1.0).shrink_to_fit());
                });
            });
        }

        ui.add_space(10.0);
        let length = track.length_m.map(|m| format!("{m:.0} m")).unwrap_or_else(|| "—".into());
        let pitboxes = track.pitboxes.map(|p| p.to_string()).unwrap_or_else(|| "—".into());
        theme::card_flat().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::key_value(ui, "Length", &length);
            widgets::key_value(ui, "Pitboxes", &pitboxes);
            widgets::key_value_block(ui, "Folder", &track.id);
            widgets::key_value_block(ui, "Path", &track.path.display().to_string());
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(widgets::primary_button(if picking { "Choose this track" } else { "Race here" })).clicked() {
                *race = Some(RaceAction {
                    track: Some(track.id.clone()),
                    layout: layout_name.map(str::to_string),
                    ..RaceAction::default()
                });
            }
            if ui.button("Open folder").clicked() {
                open_folder(&track.path);
            }
        });
    });
}

/// Version, where the app keeps its data, and the keyboard shortcuts.
fn about_card(ui: &mut egui::Ui) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, "About");
        ui.label(RichText::new(format!("ACXS Manager {}", env!("CARGO_PKG_VERSION"))).font(theme::bold(15.0)));
        let data_dir = settings::settings_path().parent().map(|p| p.to_path_buf());
        if let Some(dir) = data_dir {
            ui.add_space(4.0);
            ui.label(RichText::new("Settings, favorites and your server list are stored in:").size(12.5).color(MUTED));
            ui.horizontal(|ui| {
                ui.label(RichText::new(dir.display().to_string()).size(12.5));
                if ui.small_button("Open").clicked() {
                    open_folder(&dir);
                }
            });
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new(RichText::new("Changelog").font(theme::bold(13.5))).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for line in include_str!("../../CHANGELOG.md").lines() {
                    let line = line.trim_end();
                    if let Some(title) = line.strip_prefix("## ") {
                        ui.add_space(8.0);
                        ui.label(RichText::new(title).font(theme::bold(14.5)).color(theme::ACCENT));
                    } else if let Some(item) = line.strip_prefix("- ") {
                        ui.add(egui::Label::new(RichText::new(format!("•  {item}")).size(12.5).color(MUTED)));
                    } else if !line.is_empty() && !line.starts_with("# ") {
                        ui.label(RichText::new(line).font(theme::bold(12.5)));
                    }
                }
            });
        });
        ui.add_space(10.0);
        widgets::section_title(ui, "Shortcuts");
        egui::Grid::new("shortcuts").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            for (keys, what) in [
                ("Ctrl K", "Search pages, tools, cars and tracks"),
                ("Double-click a car", "Open it in the showroom"),
                ("Right-click a card", "More actions"),
                ("Arrow keys, Enter  (Cars, Tracks)", "Move through the cards, open the car"),
                ("← →  (Showroom)", "Previous / next skin"),
                ("Page Up / Page Down  (Showroom)", "Previous / next car"),
                ("F  (Showroom)", "Favorite the car"),
                ("Drop a .zip on the window", "Install a car or track"),
            ] {
                ui.label(RichText::new(keys).font(theme::bold(13.0)));
                ui.label(RichText::new(what).color(MUTED));
                ui.end_row();
            }
        });
    });
}

/// Arrow keys move the selection through a card grid of `count` items laid out in `columns`,
/// and Enter opens it. Returns the new index (when it moved) and whether Enter was pressed.
/// Does nothing while a text field has the keyboard.
fn grid_keys(ui: &egui::Ui, count: usize, current: Option<usize>, columns: usize) -> (Option<usize>, bool) {
    if count == 0 || ui.ctx().egui_wants_keyboard_input() {
        return (None, false);
    }
    let columns = columns.max(1) as isize;
    let (step, enter) = ui.input(|i| {
        let mut step = 0isize;
        if i.key_pressed(egui::Key::ArrowRight) {
            step += 1;
        }
        if i.key_pressed(egui::Key::ArrowLeft) {
            step -= 1;
        }
        if i.key_pressed(egui::Key::ArrowDown) {
            step += columns;
        }
        if i.key_pressed(egui::Key::ArrowUp) {
            step -= columns;
        }
        (step, i.key_pressed(egui::Key::Enter))
    });
    if step == 0 {
        return (None, enter && current.is_some());
    }
    let next = current.map_or(0, |c| (c as isize + step).clamp(0, count as isize - 1) as usize);
    (Some(next), enter && current.is_some())
}

/// A request from a detail panel to start a quick race with this track and/or car.
#[derive(Default)]
pub struct RaceAction {
    pub track: Option<String>,
    pub layout: Option<String>,
    pub car: Option<String>,
    pub skin: Option<String>,
}
