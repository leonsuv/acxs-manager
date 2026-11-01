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

    let window = ui::window_state::load();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("ACXS Manager")
        .with_inner_size([window.width, window.height])
        .with_min_inner_size([920.0, 600.0])
        .with_maximized(window.maximized)
        .with_icon(app_icon());
    if let (Some(x), Some(y)) = (window.x, window.y) {
        viewport = viewport.with_position([x, y]);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };

    eframe::run_native(
        "ACXS Manager",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc)))),
    )
}

/// The window icon: an orange rounded square with a white "A", drawn here so no image file ships.
fn app_icon() -> egui::IconData {
    const SIZE: u32 = 64;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let size = SIZE as f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded square.
            let radius = 14.0;
            let dx = (fx - size / 2.0).abs() - (size / 2.0 - radius);
            let dy = (fy - size / 2.0).abs() - (size / 2.0 - radius);
            let inside = dx.max(0.0).hypot(dy.max(0.0)) <= radius && dx.max(dy) <= radius;
            if !inside {
                continue;
            }
            // The A: two slanted strokes from the apex plus a crossbar.
            let apex = (size / 2.0, 14.0);
            let stroke = 4.5;
            let t = ((fy - apex.1) / (size - 14.0 - apex.1)).clamp(0.0, 1.0);
            let half = 2.0 + t * 17.0;
            let on_leg = (fy >= apex.1 - 1.0 && fy <= size - 14.0)
                && (((fx - (size / 2.0 - half)).abs() < stroke / 2.0 + 0.5) || ((fx - (size / 2.0 + half)).abs() < stroke / 2.0 + 0.5));
            let bar = fy > 38.0 && fy < 42.5 && (fx - size / 2.0).abs() < half;
            let letter = on_leg || bar;
            let i = ((y * SIZE + x) * 4) as usize;
            let color = if letter { [255, 255, 255, 255] } else { [0xFF, 0x7A, 0x1A, 255] };
            rgba[i..i + 4].copy_from_slice(&color);
        }
    }
    egui::IconData { rgba, width: SIZE, height: SIZE }
}
