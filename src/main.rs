#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! ACXS Manager: eframe entry point. The core logic lives in the `acxs_manager`
//! library; the GUI lives in `ui`. The UI refers to the library modules through
//! `crate::`, so they are re-exported here.

mod ui;

use acxs_manager::{content, csp, game, settings};
use eframe::egui;

fn main() -> eframe::Result {
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
