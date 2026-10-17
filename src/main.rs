#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! ACXS Manager: eframe entry point. The core logic lives in the `acxs_manager`
//! library; the GUI lives in `ui`. The UI refers to the library modules through
//! `crate::`, so they are re-exported here.

mod ui;

use acxs_manager::{content, crash_report, csp, game, settings};
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    // Panics are stored next to settings.json, whether or not reports are sent.
    let settings_dir = settings::settings_path()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    crash_report::install_panic_hook(settings_dir);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ACXS Manager")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([920.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ACXS Manager",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc)))),
    )
}
