//! Flagship pages of ACXS Manager: Paddock Hub (Home), Garage Cars & Tracks,
//! CSP Lights Patch & Online Catalog, and App Settings.
//!
//! Pages only read `App` and report user actions by calling its methods, which
//! run the real work on background threads.

use super::media;
use super::theme::{self, DANGER, FAINT, INFO, MUTED, OK, TEXT, WARN};
use super::tools::PickKind;
use super::widgets;
use super::{App, Page};
use crate::content::{self, CarEntry, TrackEntry};
use crate::csp::{self, LightsPatch, UNRECOGNIZED};
use acxs_manager::content_meta;
use acxs_manager::csp_online::{self, CspCatalog, CspRelease};
use acxs_manager::dashboard_stats::{self, ContentStats, CountEntry};
use acxs_manager::driving_stats::{self, DrivingStats};
use acxs_manager::mods;
use acxs_manager::query;
use acxs_manager::settings::{self, AccentPreset};
use acxs_manager::skins;
use acxs_manager::update::{self, Release, Version};
use acxs_manager::{crash_report, uri_register};
use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke, StrokeKind, vec2};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TOP_N: usize = 5;
const STATS_ID: &str = "home_content_stats";
const UPDATE_STATE_ID: &str = "home_update_check";
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// 1. PADDOCK HUB (Home)
// ---------------------------------------------------------------------------

pub fn home(app: &mut App, ui: &mut egui::Ui) {
    if !app.snapshot.ac_valid {
        if app.busy {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| widgets::skeleton_rect(ui, vec2(420.0, 120.0), 14));
        } else {
            onboarding(app, ui);
        }
        return;
    }

    let game_running = app.snapshot.game_running;
    let installed = app.snapshot.installed.clone();
    let csp_build = app.snapshot.installed_csp_build;
    let stats = content_stats(app, ui);
    let driving = driving_stats(app, ui);
    let featured = featured_car(app);
    let accent = theme::active_accent(ui.ctx());
    let mut goto: Option<Page> = None;
    let mut surprise = false;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Full-bleed cinematic hero backdrop (no cramped 230 px clipping).
        let avail_w = ui.available_width();
        let hero_h = (avail_w * 0.30).clamp(290.0, 390.0).round();
        let (rect, _) = ui.allocate_exact_size(vec2(avail_w, hero_h), egui::Sense::hover());
        let round = CornerRadius::same(theme::RADIUS + 4);
        match featured.as_ref().and_then(|car| media::car_preview(&car.path, None)) {
            Some(path) => media::paint_cover(ui, rect, &path, round),
            None => {
                ui.painter().rect_filled(rect, round, theme::SURFACE_ELEVATED);
            }
        }
        widgets::paint_hero_scrim(ui.painter(), rect, 238, 225);
        ui.painter().rect_stroke(rect, round, Stroke::new(1.0, theme::BORDER_ACTIVE), StrokeKind::Inside);
        theme::paint_top_specular(ui.painter(), rect, f32::from(theme::RADIUS + 4));

        let inner = rect.shrink2(vec2(32.0, 26.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            // Top micro-badge row.
            ui.horizontal(|ui| {
                if game_running {
                    widgets::pill(ui, "SESSION LIVE", OK);
                } else {
                    widgets::pill(ui, "PADDOCK READY", accent);
                }
                if let Some(car) = &featured
                    && !car.car_class.is_empty()
                {
                    widgets::pill(ui, &car.car_class.to_uppercase(), media::class_color(&car.car_class));
                }
                let csp_label = match (installed.as_deref(), csp_build) {
                    (None, _) => "CSP NOT INSTALLED".to_string(),
                    (Some(UNRECOGNIZED), Some(b)) => format!("CSP BUILD #{b}"),
                    (Some(UNRECOGNIZED), None) => "CSP CUSTOM".to_string(),
                    (Some(v), _) => format!("CSP {v}"),
                };
                widgets::pill(ui, &csp_label, INFO);
            });

            ui.add_space(6.0);
            let title = if game_running {
                "ASSETTO CORSA IS RUNNING".to_string()
            } else if let Some(car) = &featured {
                car.name.to_uppercase()
            } else {
                "READY TO RACE".to_string()
            };
            ui.label(
                RichText::new(title)
                    .font(theme::display(42.0))
                    .color(if game_running { OK } else { egui::Color32::WHITE }),
            );

            // Tabular mono telemetry subtitle.
            let mut spec_bits: Vec<String> = Vec::new();
            if let Some(car) = &featured {
                if !car.brand.is_empty() {
                    spec_bits.push(car.brand.clone());
                }
                if let Some(bhp) = car.bhp {
                    spec_bits.push(format!("{bhp:.0} BHP"));
                }
                if let Some(kg) = car.weight_kg {
                    spec_bits.push(format!("{kg:.0} KG"));
                }
                if let Some(pw) = car.pwratio {
                    spec_bits.push(format!("{pw:.2} KG/HP"));
                }
            }
            spec_bits.push(format!("{} CARS  ·  {} TRACKS", app.snapshot.cars.len(), app.snapshot.tracks.len()));
            ui.label(RichText::new(spec_bits.join("   ·   ")).font(theme::mono(12.5)).color(egui::Color32::from_gray(205)));

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if widgets::cta_button(ui, "LAUNCH ASSETTO CORSA  ▶", vec2(228.0, 44.0), !game_running).clicked() {
                    app.launch_game();
                }
                if featured.is_some() {
                    if ui
                        .add(egui::Button::new(RichText::new("Open Showroom").font(theme::bold(13.5))).min_size(vec2(0.0, 44.0)))
                        .clicked()
                    {
                        goto = Some(Page::Showroom);
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Surprise Me").font(theme::bold(13.5))).min_size(vec2(0.0, 44.0)))
                        .clicked()
                    {
                        surprise = true;
                    }
                }
            });
        });

        let below = rect.bottom() - ui.cursor().min.y;
        if below > 0.0 {
            ui.add_space(below);
        }
        ui.add_space(18.0);

        // Live KPI telemetry strip (4 monospace metric tiles).
        let kpis: Vec<(&str, String, egui::Color32)> = if let Some(d) = &driving
            && d.sessions > 0
        {
            vec![
                ("DRIVE TIME", format!("{:.1} hrs", d.seconds / 3600.0), TEXT),
                ("DISTANCE", format!("{:.0} km", d.distance_km), theme::CH_SPEED),
                ("SESSIONS", d.sessions.to_string(), TEXT),
                ("TOP SPEED", format!("{:.0} km/h", d.max_speed), accent),
            ]
        } else if let Some(s) = &stats {
            vec![
                ("CARS INSTALLED", s.car_total.to_string(), TEXT),
                ("TRACKS", s.track_total.to_string(), theme::CH_SPEED),
                ("LIVERIES", s.skin_total.to_string(), TEXT),
                ("LAYOUTS", s.layout_total.to_string(), accent),
            ]
        } else {
            Vec::new()
        };

        if kpis.is_empty() {
            widgets::tile_grid(ui, 180.0, 4, |ui, _| {
                widgets::skeleton_rect(ui, vec2(ui.available_width(), 68.0), 12);
            });
        } else {
            widgets::tile_grid(ui, 180.0, kpis.len(), |ui, i| {
                let (label, value, color) = &kpis[i];
                let resp = theme::card_elevated()
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(theme::micro_label(label));
                        ui.add_space(2.0);
                        ui.label(RichText::new(value).font(theme::mono(22.0)).color(*color));
                    })
                    .response;
                theme::paint_top_specular(ui.painter(), resp.rect, f32::from(theme::RADIUS));
            });
        }

        ui.add_space(18.0);

        // Visual Jump-In Cards.
        let actions: [(&str, &str, &str, egui::Color32); 4] = [
            ("QUICK RACE", "Configure grid, weather & rules", "quick", accent),
            ("ONLINE PADDOCK", "Live lobbies, slots & direct join", "servers", theme::CH_SPEED),
            ("SPECIAL EVENTS", "Medals, challenges & career", "events", theme::WARN),
            ("TELEMETRY SESSIONS", "30 Hz drives, laps & corner coach", "sessions", theme::CH_THROTTLE),
        ];
        widgets::tile_grid(ui, 210.0, actions.len(), |ui, i| {
            let (title, text, key, bar_color) = actions[i];
            let response = theme::card_elevated()
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(RichText::new(title).font(theme::display(18.0)).color(TEXT));
                        ui.add(egui::Label::new(RichText::new(text).size(12.5).color(MUTED)).truncate());
                    });
                })
                .response
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            // Left motorsport accent stripe.
            let stripe = egui::Rect::from_min_size(
                response.rect.left_top() + vec2(0.0, 10.0),
                vec2(3.0, (response.rect.height() - 20.0).max(8.0)),
            );
            ui.painter().rect_filled(stripe, CornerRadius::same(2), bar_color);
            if response.hovered() {
                ui.painter().rect_stroke(
                    response.rect,
                    CornerRadius::same(theme::RADIUS),
                    Stroke::new(1.2, bar_color.gamma_multiply(0.8)),
                    StrokeKind::Inside,
                );
            }
            if response.clicked() {
                goto = Some(match key {
                    "events" => Page::Tool(app.tool_index("Events")),
                    "sessions" => Page::Tool(app.tool_index("Sessions")),
                    "servers" => Page::Servers,
                    _ => Page::Tool(app.tool_index("Quick Race")),
                });
            }
        });

        ui.add_space(20.0);
        driving_section(app, ui, driving);
        ui.add_space(16.0);
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

fn onboarding(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(36.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(580.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                ui.label(RichText::new("WELCOME TO ACXS MANAGER").font(theme::display(30.0)));
                ui.add_space(4.0);
                ui.label(RichText::new("Point ACXS at your Assetto Corsa folder and everything lights up.").color(MUTED));
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
                if widgets::cta_button(ui, "SAVE & SCAN", vec2(160.0, 38.0), true).clicked() {
                    app.save_paths();
                }
            });
        });
    });
}

// ---------------------------------------------------------------------------
// 2. APP SETTINGS (Accent Preset, Protocol Handler, Paths, Updates, Crash, About)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct ExtraPathsState {
    initialized: bool,
    seven_zip: String,
    mods_dir: String,
    csp_catalog: String,
}

pub fn settings(app: &mut App, ui: &mut egui::Ui) {
    widgets::heading(
        ui,
        "App Settings",
        "Appearance, OS protocol handlers, folders, updates and diagnostics. Sim options live under Setup → Video & Audio / Controls.",
    );
    let ac_valid = app.snapshot.ac_valid;

    let extra_id = egui::Id::unique("settings_extra_paths");
    let mut extra: ExtraPathsState = ui.ctx().data_mut(|d| d.get_temp(extra_id)).unwrap_or_default();
    if !extra.initialized {
        extra.seven_zip = app.settings.seven_zip_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        extra.mods_dir = app.settings.mods_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        extra.csp_catalog = app.settings.csp_catalog_url.clone().unwrap_or_default();
        extra.initialized = true;
    }

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(860.0);

        // Accent Theme Card.
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Appearance & Motorsport Accent");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
                for (preset, name) in [
                    (AccentPreset::MotorsportRed, "Motorsport Red"),
                    (AccentPreset::TelemetryCyan, "Telemetry Cyan"),
                    (AccentPreset::AmberGold, "Amber Gold"),
                    (AccentPreset::Emerald, "Emerald"),
                ] {
                    let color = theme::accent_color(preset);
                    let active = app.settings.accent_preset == preset;
                    let fill = if active { color.gamma_multiply(0.20) } else { theme::SURFACE_RAISED };
                    let stroke = if active { Stroke::new(1.5, color) } else { Stroke::new(1.0, theme::BORDER_SUBTLE) };
                    let btn = egui::Frame::new()
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (dot, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
                                ui.painter().circle_filled(dot.center(), 6.0, color);
                                ui.label(RichText::new(name).font(if active { theme::bold(13.5) } else { egui::FontId::proportional(13.5) }));
                            });
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if btn.clicked() && !active {
                        app.settings.accent_preset = preset;
                        theme::apply_with_accent(ui.ctx(), preset);
                        let _ = app.settings.save(&settings::settings_path());
                        app.push_notice(format!("Switched accent to {name}."), true);
                    }
                }
            });
        });

        ui.add_space(14.0);

        // Protocol Registration (`acmanager://` & `acxs://`).
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Invite Links (acmanager:// & acxs://)");
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("acxs-manager.exe"));
            let registered = uri_register::is_registered(&exe);
            ui.horizontal(|ui| {
                widgets::status_pill(
                    ui,
                    if registered { "Registered" } else { "Not registered" },
                    if registered { OK } else { MUTED },
                );
                ui.label(
                    RichText::new("Open Discord & web server invite links directly in ACXS Manager.")
                        .size(13.0)
                        .color(MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if registered {
                        if ui.button("Unregister").clicked() {
                            match uri_register::unregister_protocols() {
                                Ok(()) => app.push_notice("Removed acmanager:// and acxs:// protocol handlers.", true),
                                Err(e) => app.push_notice(format!("Protocol unregister failed: {e}"), false),
                            }
                        }
                    } else if ui.add(widgets::primary_button("Register protocol")).clicked() {
                        match uri_register::register_protocols(&exe) {
                            Ok(()) => app.push_notice("Registered acmanager:// and acxs:// handlers.", true),
                            Err(e) => app.push_notice(format!("Protocol registration failed: {e}"), false),
                        }
                    }
                });
            });
        });

        ui.add_space(14.0);
        update_card(app, ui);
        ui.add_space(14.0);

        // Folders & Advanced Paths.
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Folders & Sources");
            path_row(ui, "Assetto Corsa folder", "Not set", &mut app.ac_input);
            ui.add_space(10.0);
            path_row(ui, "Lights patch library", "Next to this program", &mut app.library_input);
            ui.add_space(10.0);
            path_row(ui, "JSGME MODS folder (optional override)", "<AC root>/MODS", &mut extra.mods_dir);
            ui.add_space(10.0);
            file_row(ui, "7-Zip executable (optional, for .rar/.7z fallback)", "Auto-detect 7z.exe", &mut extra.seven_zip);
            ui.add_space(10.0);
            ui.label(RichText::new("CSP online catalog URL (optional HTTPS override)").color(MUTED));
            ui.add_space(2.0);
            ui.add(
                egui::TextEdit::singleline(&mut extra.csp_catalog)
                    .desired_width(f32::INFINITY)
                    .hint_text(csp_online::DEFAULT_CATALOG_URL)
                    .margin(Margin::symmetric(10, 7)),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if widgets::cta_button(ui, "SAVE SETTINGS", vec2(150.0, 36.0), true).clicked() {
                    app.settings.seven_zip_path = (!extra.seven_zip.trim().is_empty()).then(|| PathBuf::from(extra.seven_zip.trim()));
                    app.settings.mods_dir = (!extra.mods_dir.trim().is_empty()).then(|| PathBuf::from(extra.mods_dir.trim()));
                    app.settings.csp_catalog_url = (!extra.csp_catalog.trim().is_empty()).then(|| extra.csp_catalog.trim().to_string());
                    app.save_paths();
                }
                if !ac_valid {
                    ui.label(RichText::new("Choose the folder that contains acs.exe, then save.").color(WARN));
                }
            });
        });

        ui.add_space(14.0);
        crash_card(app, ui);
        ui.add_space(14.0);
        about_card(ui);
        ui.add_space(8.0);
    });

    ui.ctx().data_mut(|d| d.insert_temp(extra_id, extra));
}

fn path_row(ui: &mut egui::Ui, label: &str, hint: &str, value: &mut String) {
    ui.label(RichText::new(label).color(MUTED));
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 90.0).max(160.0);
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(width)
                .hint_text(hint)
                .margin(Margin::symmetric(10, 7)),
        );
        if ui.button("Browse").clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            *value = path.display().to_string();
        }
    });
}

fn file_row(ui: &mut egui::Ui, label: &str, hint: &str, value: &mut String) {
    ui.label(RichText::new(label).color(MUTED));
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 90.0).max(160.0);
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(width)
                .hint_text(hint)
                .margin(Margin::symmetric(10, 7)),
        );
        if ui.button("Browse").clicked()
            && let Some(path) = rfd::FileDialog::new().pick_file()
        {
            *value = path.display().to_string();
        }
    });
}

fn list_toolbar(
    ui: &mut egui::Ui,
    query_text: &mut String,
    hint: &str,
    shown: usize,
    total: usize,
    noun: &str,
    query_active: bool,
) {
    ui.horizontal(|ui| {
        let reserve = if query_active { 195.0 } else { 135.0 };
        ui.add(
            egui::TextEdit::singleline(query_text)
                .hint_text(hint)
                .desired_width((ui.available_width() - reserve).max(160.0))
                .margin(Margin::symmetric(10, 7)),
        );
        if query_active {
            widgets::pill(ui, "QUERY", theme::CH_SPEED);
        }
        ui.label(RichText::new(format!("{shown} / {total} {noun}")).font(theme::mono(12.0)).color(MUTED));
    });
    ui.add_space(4.0);
}

fn detail_panel(id: &'static str) -> egui::Panel {
    egui::Panel::right(id)
        .resizable(true)
        .default_size(370.0)
        .size_range(310.0..=560.0)
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE_ELEVATED)
                .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(Margin::same(18)),
        )
}

pub(super) fn open_folder(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

// ---------------------------------------------------------------------------
// 3. GARAGE: CARS & TRACKS (Query Language, Parent Grouping, Batch Toolbar)
// ---------------------------------------------------------------------------

const CAR_FILTERS: [&str; 5] = ["All", "★ Favorites", "Race", "Street", "Other"];
const CAR_SORTS: [&str; 4] = ["Name", "Brand", "Newest", "Power (BHP)"];

#[derive(Clone, Default)]
struct BatchState {
    enabled: bool,
    group_upgrades: bool,
    selected: HashSet<String>,
    tag_input: String,
    class_input: String,
}

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
        .stroke(Stroke::new(1.0, theme::active_accent(ui.ctx()).gamma_multiply(0.75)))
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("CHOOSE A {} FOR QUICK RACE", what.to_uppercase())).font(theme::display(22.0)));
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
    heading_or_picker(
        app,
        ui,
        PickKind::Car,
        "Garage · Cars",
        "Query by brand, class, power or tags (e.g. brand:ferrari bhp>500). Double-click for Showroom, right-click for actions.",
    );
    if !app.snapshot.ac_valid {
        widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to see cars.");
        return;
    }

    let batch_id = egui::Id::unique("cars_batch_state");
    let mut batch: BatchState = ui.ctx().data_mut(|d| d.get_temp(batch_id)).unwrap_or_default();

    let mut star: Option<String> = None;
    let mut open: Option<String> = None;
    let mut batch_disable: Vec<String> = Vec::new();
    let mut batch_tag: Option<(Vec<String>, String)> = None;
    let mut batch_class: Option<(Vec<String>, String)> = None;
    let game_running = app.snapshot.game_running;

    {
        let App { snapshot, car_query, selected_car, car_skin, favorites, car_filter, car_sort, grid_columns, race_request, picker, .. } = &mut *app;

        detail_panel("car_detail").show(ui, |ui| {
            let car = selected_car
                .as_deref()
                .and_then(|id| snapshot.cars.iter().find(|c| c.id == id));
            car_detail(ui, car, car_skin, race_request, picker.is_some());
        });

        // Use CM-style query language when query parses and contains operators/field syntax;
        // otherwise fall back gracefully to substring filter.
        let trimmed = car_query.trim();
        let use_query_lang = !trimmed.is_empty()
            && (trimmed.contains(':') || trimmed.contains('>') || trimmed.contains('<') || trimmed.contains('&') || trimmed.contains('|') || trimmed.starts_with('!'));
        let parsed_query = if use_query_lang { query::parse(trimmed).ok() } else { None };
        let query_active = parsed_query.is_some();

        let mut filtered: Vec<CarEntry> = if let Some(q) = &parsed_query {
            snapshot.cars.iter().filter(|c| query::matches(q, *c)).cloned().collect()
        } else {
            content::filter_cars(&snapshot.cars, car_query.as_str()).into_iter().cloned().collect()
        };

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
            3 => filtered.sort_by(|a, b| {
                b.bhp
                    .unwrap_or(0.0)
                    .partial_cmp(&a.bhp.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
            _ => filtered.sort_by_key(|c| c.name.to_lowercase()),
        }

        // Optional parent/upgrade grouping: place upgrades right under their parent id.
        if batch.group_upgrades {
            filtered.sort_by_key(|c| {
                let root = c.parent.as_deref().unwrap_or(c.id.as_str()).to_lowercase();
                let is_child = c.parent.is_some();
                (root, is_child, c.name.to_lowercase())
            });
        }

        list_toolbar(
            ui,
            car_query,
            "Search or query (e.g. brand:ferrari bhp>500 & !street)",
            filtered.len(),
            snapshot.cars.len(),
            "cars",
            query_active,
        );

        ui.horizontal_wrapped(|ui| {
            for label in CAR_FILTERS {
                if ui.selectable_label(car_filter == label, label).clicked() {
                    *car_filter = label.to_string();
                }
            }
            egui::ComboBox::from_id_salt("car_sort")
                .selected_text(format!("Sort: {}", CAR_SORTS[(*car_sort).min(CAR_SORTS.len() - 1)]))
                .show_ui(ui, |ui| {
                    for (i, name) in CAR_SORTS.iter().enumerate() {
                        ui.selectable_value(car_sort, i, *name);
                    }
                });
            let next_group = !batch.group_upgrades;
            ui.selectable_value(&mut batch.group_upgrades, next_group, "Group upgrades");
            if ui.selectable_label(batch.enabled, format!("Batch select ({})", batch.selected.len())).clicked() {
                batch.enabled = !batch.enabled;
                if !batch.enabled {
                    batch.selected.clear();
                }
            }
        });

        // Multi-select batch toolbar when active.
        if batch.enabled {
            ui.add_space(6.0);
            theme::card_flat().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    widgets::pill(ui, &format!("{} selected", batch.selected.len()), theme::active_accent(ui.ctx()));
                    if ui.small_button("Select shown").clicked() {
                        for c in &filtered {
                            batch.selected.insert(c.id.clone());
                        }
                    }
                    if ui.small_button("Clear").clicked() {
                        batch.selected.clear();
                    }
                    let has_sel = !batch.selected.is_empty();
                    if ui
                        .add_enabled(has_sel && !game_running, egui::Button::new("Disable selected"))
                        .on_hover_text("Moves selected cars safely into acxs-disabled/")
                        .clicked()
                    {
                        batch_disable = batch.selected.iter().cloned().collect();
                    }
                    ui.add(egui::TextEdit::singleline(&mut batch.tag_input).desired_width(110.0).hint_text("tag"));
                    if ui.add_enabled(has_sel && !batch.tag_input.trim().is_empty(), egui::Button::new("+ Tag")).clicked() {
                        batch_tag = Some((batch.selected.iter().cloned().collect(), batch.tag_input.trim().to_string()));
                    }
                    ui.add(egui::TextEdit::singleline(&mut batch.class_input).desired_width(90.0).hint_text("class"));
                    if ui.add_enabled(has_sel && !batch.class_input.trim().is_empty(), egui::Button::new("Set class")).clicked() {
                        batch_class = Some((batch.selected.iter().cloned().collect(), batch.class_input.trim().to_string()));
                    }
                });
            });
        }
        ui.add_space(6.0);

        if filtered.is_empty() {
            let text = if snapshot.cars.is_empty() {
                "No cars found in content/cars."
            } else if car_filter == "★ Favorites" {
                "No favorites yet. Hover a car and click its star."
            } else {
                "No cars match your search or query."
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
            let batch_on = batch.enabled;
            let batch_sel = batch.selected.clone();
            let spec = media::GridSpec { min_width: 220.0, info_height: 58.0, stars: true, scroll_to };
            let out = media::grid(
                ui,
                &filtered,
                &spec,
                |car| {
                    if batch_on {
                        batch_sel.contains(&car.id)
                    } else {
                        current.as_deref() == Some(car.id.as_str())
                    }
                },
                |car| favs.contains(&car.id),
                |ui, image, info, car| {
                    let preview = media::car_preview(&car.path, None);
                    media::paint_image(ui, image, preview.as_deref(), &car.name, INFO, media::top_round());
                    if !car.car_class.is_empty() {
                        let color = media::class_color(&car.car_class);
                        media::chip(ui, image.left_top() + vec2(8.0, 8.0), egui::Align2::LEFT_TOP, &car.car_class, color);
                    }
                    if car.parent.is_some() {
                        media::chip(ui, image.left_bottom() + vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, "UPGRADE", theme::SIGNAL_PURPLE);
                    }
                    if let Some(bhp) = car.bhp {
                        media::chip(ui, image.right_bottom() - vec2(8.0, 8.0), egui::Align2::RIGHT_BOTTOM, &format!("{bhp:.0} hp"), TEXT);
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
                let id = filtered[i].id.clone();
                if batch.enabled {
                    if !batch.selected.remove(&id) {
                        batch.selected.insert(id.clone());
                    }
                }
                *selected_car = Some(id);
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

    // Apply batch operations safely off the selection list.
    if !batch_disable.is_empty()
        && let Some(root) = app.settings.ac_root_path()
    {
        let mut ok_count = 0usize;
        for id in &batch_disable {
            if mods::disable_car(&root, id, game_running).is_ok() {
                ok_count += 1;
                batch.selected.remove(id);
            }
        }
        app.push_notice(format!("Disabled {ok_count} of {} car(s).", batch_disable.len()), ok_count > 0);
        app.refresh();
    }
    if let Some((ids, tag)) = batch_tag
        && let Some(root) = app.settings.ac_root_path()
    {
        let mut ok_count = 0usize;
        for id in &ids {
            let dir = root.join("content").join("cars").join(id);
            if let Ok(mut map) = content_meta::read_car_meta(&dir) {
                let tags = map.entry("tags").or_insert_with(|| serde_json::Value::Array(Vec::new()));
                if let Some(arr) = tags.as_array_mut()
                    && !arr.iter().any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&tag)))
                {
                    arr.push(serde_json::Value::String(tag.clone()));
                    if content_meta::save_car_meta(&dir, &map).is_ok() {
                        ok_count += 1;
                    }
                }
            }
        }
        batch.tag_input.clear();
        app.push_notice(format!("Added tag '{tag}' to {ok_count} car(s)."), true);
        app.refresh();
    }
    if let Some((ids, class)) = batch_class
        && let Some(root) = app.settings.ac_root_path()
    {
        let mut ok_count = 0usize;
        for id in &ids {
            let dir = root.join("content").join("cars").join(id);
            if let Ok(mut map) = content_meta::read_car_meta(&dir) {
                map.insert("class".into(), serde_json::Value::String(class.clone()));
                if content_meta::save_car_meta(&dir, &map).is_ok() {
                    ok_count += 1;
                }
            }
        }
        batch.class_input.clear();
        app.push_notice(format!("Set class '{class}' on {ok_count} car(s)."), true);
        app.refresh();
    }

    ui.ctx().data_mut(|d| d.insert_temp(batch_id, batch));

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
    heading_or_picker(
        app,
        ui,
        PickKind::Track,
        "Garage · Tracks",
        "Every circuit with its layouts and outline map. Supports query filters (e.g. country:italy length>4000).",
    );
    if !app.snapshot.ac_valid {
        widgets::empty_state(ui, "Set your Assetto Corsa folder in Settings to see tracks.");
        return;
    }

    let batch_id = egui::Id::unique("tracks_batch_state");
    let mut batch: BatchState = ui.ctx().data_mut(|d| d.get_temp(batch_id)).unwrap_or_default();
    let mut batch_disable: Vec<String> = Vec::new();
    let game_running = app.snapshot.game_running;

    {
        let App { snapshot, track_query, selected_track, track_layout, grid_columns, race_request, picker, .. } = app;

        detail_panel("track_detail").show(ui, |ui| {
            let track = selected_track
                .as_deref()
                .and_then(|id| snapshot.tracks.iter().find(|t| t.id == id));
            track_detail(ui, track, track_layout, race_request, picker.is_some());
        });

        let trimmed = track_query.trim();
        let use_query_lang = !trimmed.is_empty()
            && (trimmed.contains(':') || trimmed.contains('>') || trimmed.contains('<') || trimmed.contains('&') || trimmed.contains('|') || trimmed.starts_with('!'));
        let parsed_query = if use_query_lang { query::parse(trimmed).ok() } else { None };
        let query_active = parsed_query.is_some();

        let filtered: Vec<TrackEntry> = if let Some(q) = &parsed_query {
            snapshot.tracks.iter().filter(|t| query::matches(q, *t)).cloned().collect()
        } else {
            content::filter_tracks(&snapshot.tracks, track_query.as_str()).into_iter().cloned().collect()
        };

        list_toolbar(
            ui,
            track_query,
            "Search or query (e.g. country:italy pitboxes>=24)",
            filtered.len(),
            snapshot.tracks.len(),
            "tracks",
            query_active,
        );
        ui.horizontal(|ui| {
            if ui.selectable_label(batch.enabled, format!("Batch select ({})", batch.selected.len())).clicked() {
                batch.enabled = !batch.enabled;
                if !batch.enabled {
                    batch.selected.clear();
                }
            }
            if batch.enabled {
                if ui.small_button("Select shown").clicked() {
                    for t in &filtered {
                        batch.selected.insert(t.id.clone());
                    }
                }
                if ui.small_button("Clear").clicked() {
                    batch.selected.clear();
                }
                if ui
                    .add_enabled(!batch.selected.is_empty() && !game_running, egui::Button::new("Disable selected"))
                    .clicked()
                {
                    batch_disable = batch.selected.iter().cloned().collect();
                }
            }
        });
        ui.add_space(6.0);

        if filtered.is_empty() {
            let text = if snapshot.tracks.is_empty() {
                "No tracks found in content/tracks."
            } else {
                "No tracks match your search or query."
            };
            widgets::empty_state(ui, text);
        } else {
            let cur_idx = selected_track.as_deref().and_then(|id| filtered.iter().position(|t| t.id == id));
            let (moved, _) = grid_keys(ui, filtered.len(), cur_idx, *grid_columns);
            let mut scroll_to = None;
            if let Some(i) = moved {
                *selected_track = Some(filtered[i].id.clone());
                *track_layout = None;
                scroll_to = Some(i);
            }
            let current = selected_track.clone();
            let batch_on = batch.enabled;
            let batch_sel = batch.selected.clone();
            let spec = media::GridSpec { min_width: 250.0, info_height: 58.0, stars: false, scroll_to };
            let out = media::grid(
                ui,
                &filtered,
                &spec,
                |track| {
                    if batch_on {
                        batch_sel.contains(&track.id)
                    } else {
                        current.as_deref() == Some(track.id.as_str())
                    }
                },
                |_| false,
                |ui, image, info, track| {
                    let preview = media::track_preview(track, None);
                    media::paint_image(ui, image, preview.as_deref(), &track.name, theme::ACCENT, media::top_round());
                    if let Some(m) = track.length_m {
                        let text = format!("{:.1} km", m / 1000.0);
                        media::chip(ui, image.right_bottom() - vec2(8.0, 8.0), egui::Align2::RIGHT_BOTTOM, &text, TEXT);
                    }
                    if track.layouts.len() > 1 {
                        let text = format!("{} layouts", track.layouts.len());
                        media::chip(ui, image.left_top() + vec2(8.0, 8.0), egui::Align2::LEFT_TOP, &text, INFO);
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
                let id = filtered[index].id.clone();
                if batch.enabled && out.clicked.is_some() {
                    if !batch.selected.remove(&id) {
                        batch.selected.insert(id.clone());
                    }
                }
                *selected_track = Some(id);
                *track_layout = None;
            }
        }
    }

    if !batch_disable.is_empty()
        && let Some(root) = app.settings.ac_root_path()
    {
        let mut ok_count = 0usize;
        for id in &batch_disable {
            if mods::disable_track(&root, id, game_running).is_ok() {
                ok_count += 1;
                batch.selected.remove(id);
            }
        }
        app.push_notice(format!("Disabled {ok_count} of {} track(s).", batch_disable.len()), ok_count > 0);
        app.refresh();
    }

    ui.ctx().data_mut(|d| d.insert_temp(batch_id, batch));
}

fn card_text(ui: &egui::Ui, info: egui::Rect, title: &str, subtitle: &str) {
    let width = info.width() - 20.0;
    let galley = media::clipped(ui, title, theme::bold(14.0), TEXT, width);
    ui.painter().galley(info.left_top() + vec2(10.0, 9.0), galley, TEXT);
    if !subtitle.is_empty() {
        let galley = media::clipped(ui, subtitle, egui::FontId::proportional(12.0), MUTED, width);
        ui.painter().galley(info.left_top() + vec2(10.0, 31.0), galley, MUTED);
    }
}

// ---------------------------------------------------------------------------
// 4. CSP LIGHTS PATCH + ONLINE CATALOG & CONFIG SYNC
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct CspOnlineState {
    catalog: Option<CspCatalog>,
    catalog_error: Option<String>,
    pending_catalog: Option<Arc<Mutex<Option<Result<CspCatalog, String>>>>>,
    pending_download: Option<Arc<Mutex<Option<Result<PathBuf, String>>>>>,
    downloading_version: Option<String>,
}

pub fn patches(app: &mut App, ui: &mut egui::Ui) {
    widgets::heading(
        ui,
        "CSP Lights Patch & Online Catalog",
        "Install local or online Custom Shaders Patch builds, archive active patches, and inspect synced car/track configs.",
    );

    let ac_valid = app.snapshot.ac_valid;
    let game_running = app.snapshot.game_running;
    let busy = app.busy;
    let installed = app.snapshot.installed.clone();
    let installed_build = app.snapshot.installed_csp_build;
    let installed_sha = app.snapshot.installed_sha.clone();
    let patches = app.snapshot.patches.clone();
    let ac_root = app.settings.ac_root_path();
    let archive = ac_root.as_ref().map(|root| csp::archive_dir(root));
    let library_path = app.library();
    let library = library_path.display().to_string();
    let catalog_url = app
        .settings
        .csp_catalog_url
        .clone()
        .unwrap_or_else(|| csp_online::DEFAULT_CATALOG_URL.to_string());
    let archive_text = archive
        .as_ref()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| "Set your Assetto Corsa folder first".to_string());
    if installed.is_none() {
        app.confirm_uninstall = false;
    }

    let online_id = egui::Id::unique("csp_online_ui_state");
    let mut online: CspOnlineState = ui.ctx().data_mut(|d| d.get_temp(online_id)).unwrap_or_default();

    // Poll async catalog & download results.
    if let Some(slot) = online.pending_catalog.clone()
        && let Some(outcome) = slot.lock().ok().and_then(|mut g| g.take())
    {
        online.pending_catalog = None;
        match outcome {
            Ok(cat) => {
                online.catalog_error = None;
                app.push_notice(format!("Loaded {} CSP release(s) from online catalog.", cat.releases.len()), true);
                online.catalog = Some(cat);
            }
            Err(e) => {
                online.catalog_error = Some(e.clone());
                app.push_notice(format!("CSP catalog fetch failed: {e}"), false);
            }
        }
    }
    if let Some(slot) = online.pending_download.clone()
        && let Some(outcome) = slot.lock().ok().and_then(|mut g| g.take())
    {
        online.pending_download = None;
        let ver = online.downloading_version.take().unwrap_or_default();
        match outcome {
            Ok(dest) => {
                app.push_notice(format!("Downloaded & verified CSP {ver} to {}.", dest.display()), true);
                app.refresh();
            }
            Err(e) => app.push_notice(format!("CSP {ver} download failed: {e}"), false),
        }
    }

    let mut to_install: Option<LightsPatch> = None;
    let mut to_download: Option<CspRelease> = None;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // Active CSP status card.
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (pill_text, pill_color) = match installed.as_deref() {
                None => ("not installed", WARN),
                Some(UNRECOGNIZED) => ("unrecognized", WARN),
                Some(_) => ("installed", OK),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(csp::status_text(&installed)).font(theme::bold(15.5)));
                widgets::pill(ui, pill_text, pill_color);
                if let Some(b) = installed_build {
                    widgets::pill(ui, &format!("BUILD #{b}"), theme::CH_SPEED);
                }
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

        // Online Catalog & Config Sync card (`acxs_manager::csp_online`).
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    widgets::section_title(ui, "Online CSP Catalog & Config Sync");
                    ui.label(
                        RichText::new("Opt-in HTTPS catalog fetch with SHA-256 verification. Nothing connects until you click.")
                            .size(12.5)
                            .color(MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let fetching = online.pending_catalog.is_some();
                    if ui.add_enabled(!fetching, widgets::primary_button("Fetch catalog")).clicked() {
                        let slot = Arc::new(Mutex::new(None));
                        let worker = Arc::clone(&slot);
                        let url = catalog_url.clone();
                        let cached = online.catalog.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            let res = csp_online::fetch_catalog(&url, cached.as_ref()).map_err(|e| e.to_string());
                            if let Ok(mut g) = worker.lock() {
                                *g = Some(res);
                            }
                            ctx.request_repaint();
                        });
                        online.pending_catalog = Some(slot);
                    }
                    if fetching || online.pending_download.is_some() {
                        ui.spinner();
                    }
                });
            });

            if let Some(root) = &ac_root {
                let idx = csp_online::load_sync_index(root);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Synced car/track/VAO configs tracked: {}   ·   Catalog: {}",
                        idx.entries.len(),
                        catalog_url
                    ))
                    .font(theme::mono(11.5))
                    .color(FAINT),
                );
            }
            if let Some(err) = &online.catalog_error {
                ui.add_space(4.0);
                ui.label(RichText::new(err).size(12.5).color(DANGER));
            }
            if let Some(cat) = &online.catalog {
                ui.add_space(8.0);
                for rel in cat.releases.iter().take(8) {
                    theme::card_flat().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("CSP {}", rel.version)).font(theme::bold(14.0)));
                            if rel.build_id > 0 {
                                widgets::pill(ui, &format!("#{}", rel.build_id), INFO);
                            }
                            if rel.recommended {
                                widgets::pill(ui, "recommended", OK);
                            }
                            if rel.sha256.is_some() {
                                widgets::pill(ui, "SHA-256", MUTED);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let can_dl = !game_running && online.pending_download.is_none();
                                if ui.add_enabled(can_dl, egui::Button::new("Download & Extract")).clicked() {
                                    to_download = Some(rel.clone());
                                }
                            });
                        });
                        if !rel.changelog.is_empty() {
                            ui.label(RichText::new(&rel.changelog).size(12.0).color(MUTED));
                        }
                    });
                    ui.add_space(4.0);
                }
            }
        });

        ui.add_space(16.0);
        widgets::section_title(ui, "Discovered local patches");
        if patches.is_empty() {
            widgets::empty_state(ui, "No local lights patches found. Fetch from the online catalog above or add folders to your library.");
        }

        for patch in &patches {
            let from_archive = archive.as_ref().is_some_and(|dir| patch.folder.starts_with(dir));
            let source = if from_archive { "Archive" } else { "Library" };
            let is_installed = installed_sha.as_deref() == Some(patch.sha256.as_str());
            let known_text = if patch.known { "known" } else { UNRECOGNIZED };
            let short_sha = patch.sha256.get(..12).unwrap_or(patch.sha256.as_str());

            theme::card_elevated().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Lights patch {}", patch.version)).font(theme::bold(14.5)));
                    widgets::pill(ui, &format!("family {}", patch.family), INFO);
                    widgets::pill(ui, known_text, theme::pill_color(known_text));
                    widgets::pill(ui, source, MUTED);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_installed {
                            widgets::pill(ui, "installed", OK);
                        } else if ui
                            .add_enabled(ac_valid && !game_running && !busy, egui::Button::new("Install"))
                            .clicked()
                        {
                            to_install = Some(patch.clone());
                        }
                    });
                });
                ui.label(RichText::new(format!("sha256 {short_sha}")).font(theme::mono(11.5)).color(MUTED));
            });
            ui.add_space(4.0);
        }
    });

    if let Some(rel) = to_download {
        let slot = Arc::new(Mutex::new(None));
        let worker = Arc::clone(&slot);
        let lib = library_path;
        let ctx = ui.ctx().clone();
        online.downloading_version = Some(rel.version.clone());
        std::thread::spawn(move || {
            let res = csp_online::download_and_extract_patch(&rel, &lib, crate::game::is_game_running())
                .map_err(|e| e.to_string());
            if let Ok(mut g) = worker.lock() {
                *g = Some(res);
            }
            ctx.request_repaint();
        });
        online.pending_download = Some(slot);
    }

    ui.ctx().data_mut(|d| d.insert_temp(online_id, online));

    if let Some(patch) = to_install {
        app.install_patch(patch);
    }
}

// ---------------------------------------------------------------------------
// Updates, Crash Reports, Stats, Detail Panels, About & Grid Keyboard Helpers
// ---------------------------------------------------------------------------

type UpdateOutcome = Result<Option<Release>, String>;

#[derive(Clone)]
enum UpdateResult {
    Newer { version: String, url: String, notes: String },
    Latest,
    Failed(String),
}

#[derive(Clone)]
struct UpdateCheck {
    feed: String,
    pending: Option<Arc<Mutex<Option<UpdateOutcome>>>>,
    last: Option<UpdateResult>,
}

const DEFAULT_FEED: &str = "https://test2.azendix.de/acxs/latest.json";
pub(super) const UPDATE_RUN_ID: &str = "update_run_request";

impl Default for UpdateCheck {
    fn default() -> Self {
        UpdateCheck { feed: DEFAULT_FEED.to_string(), pending: None, last: None }
    }
}

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

    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Updates").font(theme::bold(15.0)));
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(theme::mono(12.5)).color(MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = widgets::primary_button("Check for updates").min_size(vec2(170.0, 36.0));
                if (ui.add_enabled(can_check, button).clicked() || (run_requested && can_check)) && !checking {
                    state.pending = Some(start_update_check(state.feed.trim().to_string(), ui.ctx()));
                }
                if checking {
                    ui.spinner();
                }
            });
        });
        ui.label(RichText::new("Nothing is contacted until you press the button.").size(12.0).color(FAINT));

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
                    RichText::new("Plain http:// travels unencrypted. Use https:// when the server offers it.")
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
const CRASH_COUNT_TTL: Duration = Duration::from_secs(5);

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

    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, "Crash reports");
        let mut enabled = app.settings.crash_reports;
        let changed = ui
            .horizontal(|ui| {
                let response = widgets::toggle(ui, &mut enabled);
                ui.label("Send crash reports to the developer");
                response.changed()
            })
            .inner;
        if changed {
            app.settings.crash_reports = enabled;
            match app.settings.save(&settings::settings_path()) {
                Ok(()) if enabled => app.push_notice("Crash reports will be sent when the app starts.", true),
                Ok(()) => app.push_notice("Crash reports will not be sent.", true),
                Err(e) => app.push_notice(format!("Could not save settings: {e}"), false),
            }
        }
        ui.label(
            RichText::new(
                "Only the error message, stack trace, app version and OS are sent to the developer's GlitchTip server. \
                 Paths in your user folder are removed. Off by default.",
            )
            .small()
            .color(MUTED),
        );
        if waiting > 0 {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{waiting} report(s) waiting")).font(theme::mono(12.0)).color(MUTED));
        }
    });
}

fn start_update_check(feed: String, ctx: &egui::Context) -> Arc<Mutex<Option<UpdateOutcome>>> {
    let slot = Arc::new(Mutex::new(None));
    let worker = Arc::clone(&slot);
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let outcome = Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|e| e.to_string())
            .and_then(|current| update::check(&feed, UPDATE_TIMEOUT, &current).map_err(|e| e.to_string()));
        if let Ok(mut guard) = worker.lock() {
            *guard = Some(outcome);
        }
        ctx.request_repaint();
    });
    slot
}

fn tile(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    theme::card_elevated().inner_margin(Margin::symmetric(16, 14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add_contents(ui);
    });
}

#[derive(Clone)]
struct StatsSlot {
    key: (usize, usize),
    result: Arc<Mutex<Option<ContentStats>>>,
}

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

const DRIVING_ID: &str = "home_driving_stats";
type DrivingSlot = Arc<Mutex<Option<DrivingStats>>>;

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

fn driving_section(app: &App, ui: &mut egui::Ui, stats: Option<DrivingStats>) {
    let Some(stats) = stats else { return };
    if stats.sessions == 0 {
        return;
    }
    widgets::section_title(ui, "Driving Telemetry");
    let car_name = |id: &str| {
        app.snapshot.cars.iter().find(|c| c.id == id).map_or_else(|| id.to_string(), |c| c.name.clone())
    };
    let track_name = |id: &str| {
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
        .map(|(week, hours)| CountEntry { label: driving_stats::week_range(week), count: (hours * 60.0).round() as usize })
        .collect();
    ui.columns(3, |cols| {
        bar_card(&mut cols[0], "Most driven cars (min)", &usage_bars(&stats.top_cars, &car_name));
        bar_card(&mut cols[1], "Most driven tracks (min)", &usage_bars(&stats.top_tracks, &track_name));
        bar_card(&mut cols[2], "Recent weeks (min)", &weekly);
    });
    if !stats.bests.is_empty() {
        ui.add_space(14.0);
        theme::card_elevated().show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::section_title(ui, "Personal Best Laps");
            for best in stats.bests.iter().take(8) {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(car_name(&best.car_id)).size(13.0)).truncate());
                    ui.label(RichText::new(track_name(&best.track_key)).size(12.5).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(acxs_manager::laptimes::format_lap(best.time_ms))
                                .font(theme::mono(13.0))
                                .color(theme::SIGNAL_PURPLE),
                        );
                        ui.label(RichText::new(format!("{} laps", best.laps)).font(theme::mono(11.5)).color(MUTED));
                    });
                });
            }
        });
    }
}

fn content_section(ui: &mut egui::Ui, ac_valid: bool, stats: Option<ContentStats>) {
    if !ac_valid {
        return;
    }
    let Some(stats) = stats else { return };
    egui::CollapsingHeader::new(theme::micro_label("Library Breakdown"))
        .default_open(false)
        .show(ui, |ui| {
            ui.add_space(6.0);
            let untagged_color = if stats.untagged_cars > 0 { WARN } else { TEXT };
            let tiles = [
                ("Cars", stats.car_total.to_string(), TEXT),
                ("Tracks", stats.track_total.to_string(), TEXT),
                ("Skins", stats.skin_total.to_string(), TEXT),
                ("Layouts", stats.layout_total.to_string(), TEXT),
                ("Pitboxes", stats.total_pitboxes.to_string(), TEXT),
                ("Untagged cars", stats.untagged_cars.to_string(), untagged_color),
                ("Longest track", metres(stats.longest_track_m), TEXT),
                ("Shortest track", metres(stats.shortest_track_m), TEXT),
            ];
            widgets::tile_grid(ui, 150.0, tiles.len(), |ui, i| {
                let (label, value, color) = &tiles[i];
                tile(ui, |ui| widgets::stat(ui, label, value, *color));
            });
            ui.add_space(12.0);
            ui.columns(3, |cols| {
                bar_card(&mut cols[0], "Cars by class", &stats.cars_by_class);
                bar_card(&mut cols[1], "Cars by brand", &stats.cars_by_brand);
                bar_card(&mut cols[2], "Tracks by country", &stats.tracks_by_country);
            });
        });
}

fn bar_card(ui: &mut egui::Ui, title: &str, entries: &[CountEntry]) {
    let accent = theme::active_accent(ui.ctx());
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(228.0);
        widgets::section_title(ui, title);
        if entries.is_empty() {
            ui.label(RichText::new("Nothing to show yet.").small().color(MUTED));
            return;
        }
        let shown = dashboard_stats::top_n(entries, TOP_N);
        let largest = shown.first().map_or(1, |entry| entry.count).max(1) as f32;
        for entry in shown {
            widgets::spec_bar_animated(
                ui,
                title,
                &entry.label,
                &entry.count.to_string(),
                entry.count as f32 / largest,
                accent,
            );
        }
        let hidden = entries.len().saturating_sub(shown.len());
        if hidden > 0 {
            ui.label(RichText::new(format!("+{hidden} more")).font(theme::mono(11.0)).color(FAINT));
        }
    });
}

fn metres(length: Option<f64>) -> String {
    length.map(|m| format!("{m:.0} m")).unwrap_or_else(|| "—".to_string())
}

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

fn hero_image(ui: &mut egui::Ui, path: Option<&Path>, label: &str, tint: egui::Color32) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, (width * 9.0 / 16.0).round()), egui::Sense::hover());
    media::paint_image(ui, rect, path, label, tint, CornerRadius::same(10));
    ui.painter().rect_stroke(rect, CornerRadius::same(10), Stroke::new(1.0, theme::BORDER_SUBTLE), StrokeKind::Inside);
    rect
}

fn car_detail(ui: &mut egui::Ui, car: Option<&CarEntry>, skin: &mut Option<String>, race: &mut Option<RaceAction>, picking: bool) {
    let Some(car) = car else {
        widgets::empty_state(ui, "Select a car to inspect its liveries and specs.");
        return;
    };
    let accent = theme::active_accent(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let skins = media::car_skins(&car.path);
        let current = skin.as_deref().and_then(|name| skins.iter().find(|(n, _)| n == name)).or_else(|| skins.first());
        let preview = current.map(|(_, path)| path.clone());
        let hero = hero_image(ui, preview.as_deref(), &car.name, INFO);
        if !car.car_class.is_empty() {
            let color = media::class_color(&car.car_class);
            media::chip(ui, hero.left_top() + vec2(8.0, 8.0), egui::Align2::LEFT_TOP, &car.car_class, color);
        }
        if let Some((name, _)) = current {
            media::chip(ui, hero.right_bottom() - vec2(8.0, 8.0), egui::Align2::RIGHT_BOTTOM, name, TEXT);
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if let Some(badge) = media::car_badge(&car.path) {
                ui.add(egui::Image::from_uri(media::uri(&badge)).fit_to_exact_size(vec2(40.0, 40.0)));
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(car.name.as_str()).font(theme::display(22.0)));
                ui.label(RichText::new(join_parts(&[car.brand.as_str(), car.year.as_str(), car.country.as_str()])).color(MUTED));
            });
        });

        if let Some(parent) = &car.parent {
            ui.add_space(4.0);
            widgets::pill(ui, &format!("UPGRADE OF {parent}"), theme::SIGNAL_PURPLE);
        }

        // Animated telemetry spec bars when numeric specs are present.
        if car.bhp.is_some() || car.weight_kg.is_some() || car.pwratio.is_some() {
            ui.add_space(10.0);
            if let Some(bhp) = car.bhp {
                widgets::spec_bar_animated(ui, &car.id, "Power", &format!("{bhp:.0} bhp"), (bhp / 1000.0) as f32, accent);
            }
            if let Some(kg) = car.weight_kg {
                widgets::spec_bar_animated(ui, &car.id, "Weight", &format!("{kg:.0} kg"), (1.0 - (kg / 2200.0)) as f32, theme::CH_SPEED);
            }
            if let Some(pw) = car.pwratio {
                widgets::spec_bar_animated(ui, &car.id, "Weight / Power", &format!("{pw:.2} kg/hp"), (1.0 - (pw / 8.0)) as f32, theme::CH_THROTTLE);
            }
        }

        if !car.tags.is_empty() {
            ui.add_space(4.0);
            widgets::pill_rows(ui, &car.tags, MUTED);
        }

        if picking {
            ui.add_space(8.0);
            if widgets::cta_button(ui, "CHOOSE THIS CAR", vec2(ui.available_width(), 38.0), true).clicked() {
                *race = Some(RaceAction {
                    car: Some(car.id.clone()),
                    skin: current.map(|(name, _)| name.clone()),
                    ..RaceAction::default()
                });
            }
        }
        if skins.len() > 1 {
            ui.add_space(12.0);
            widgets::section_title(ui, &format!("Liveries ({})", skins.len()));
            let thumb = vec2(76.0, 43.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (name, path) in skins.iter() {
                    let (rect, response) = ui.allocate_exact_size(thumb, egui::Sense::click());
                    let selected = current.is_some_and(|(n, _)| n == name);
                    media::paint_image(ui, rect, Some(path), name, INFO, CornerRadius::same(6));
                    let stroke = if selected {
                        Stroke::new(2.0, accent)
                    } else if response.hovered() {
                        Stroke::new(1.0, theme::BORDER_ACTIVE)
                    } else {
                        Stroke::new(1.0, theme::BORDER_SUBTLE)
                    };
                    ui.painter().rect_stroke(rect, CornerRadius::same(6), stroke, StrokeKind::Inside);
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
            if !car.author.is_empty() {
                widgets::key_value(ui, "Author", &car.author);
            }
            widgets::key_value_block(ui, "Folder", &car.id);
            widgets::key_value_block(ui, "Path", &car.path.display().to_string());
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if widgets::cta_button(ui, if picking { "CHOOSE CAR" } else { "RACE THIS CAR" }, vec2(150.0, 36.0), true).clicked() {
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
        widgets::empty_state(ui, "Select a track to see its layouts and outline.");
        return;
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let layout_owned = layout.clone().filter(|l| track.layouts.iter().any(|t| t == l));
        let layout_name = layout_owned.as_deref();
        let preview = media::track_preview(track, layout_name);
        hero_image(ui, preview.as_deref(), &track.name, theme::ACCENT);

        ui.add_space(10.0);
        ui.label(RichText::new(track.name.as_str()).font(theme::display(22.0)));
        ui.label(RichText::new(join_parts(&[track.country.as_str(), track.city.as_str()])).color(MUTED));
        if picking {
            ui.add_space(8.0);
            if widgets::cta_button(ui, "CHOOSE THIS TRACK", vec2(ui.available_width(), 38.0), true).clicked() {
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
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if widgets::cta_button(ui, if picking { "CHOOSE TRACK" } else { "RACE HERE" }, vec2(150.0, 36.0), true).clicked() {
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

fn about_card(ui: &mut egui::Ui) {
    theme::card_elevated().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_title(ui, "About");
        ui.label(RichText::new(format!("ACXS Manager {}", env!("CARGO_PKG_VERSION"))).font(theme::bold(15.0)));
        let data_dir = settings::settings_path().parent().map(|p| p.to_path_buf());
        if let Some(dir) = data_dir {
            ui.add_space(4.0);
            ui.label(RichText::new("Settings, favorites and your server list are stored in:").size(12.5).color(MUTED));
            ui.horizontal(|ui| {
                ui.label(RichText::new(dir.display().to_string()).font(theme::mono(12.0)));
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
                        ui.label(RichText::new(title).font(theme::bold(14.5)).color(theme::active_accent(ui.ctx())));
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
                ("Ctrl K", "Command palette (pillars, tools, cars, tracks, servers)"),
                ("Alt 1–6 / Ctrl Tab", "Switch between the 6 top pillars"),
                ("Double-click a car", "Open it in the Showroom"),
                ("Right-click a card", "Context actions"),
                ("Arrow keys, Enter (Cars, Tracks)", "Navigate grid and open selection"),
                ("← → / PgUp PgDn / F (Showroom)", "Switch livery, switch car, toggle favorite"),
                ("Drop .zip / .7z / .tar.gz / .rar / folder", "Universal mod installer"),
            ] {
                ui.label(RichText::new(keys).font(theme::mono(12.5)));
                ui.label(RichText::new(what).color(MUTED));
                ui.end_row();
            }
        });
    });
}

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

#[derive(Default)]
pub struct RaceAction {
    pub track: Option<String>,
    pub layout: Option<String>,
    pub car: Option<String>,
    pub skin: Option<String>,
}
