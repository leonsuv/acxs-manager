//! Remembers the window's size, position and maximised state between runs.
//!
//! Stored as `window.json` next to the settings. A broken or missing file means defaults, and a
//! position that would put the window off every screen is dropped.

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_SIZE: [f32; 2] = [1320.0, 820.0];
const MIN_SIZE: [f32; 2] = [920.0, 600.0];
/// How often the window is looked at for changes.
const CHECK_EVERY: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState { width: DEFAULT_SIZE[0], height: DEFAULT_SIZE[1], x: None, y: None, maximized: false }
    }
}

fn path() -> PathBuf {
    let settings_file = acxs_manager::settings::settings_path();
    settings_file.parent().unwrap_or(std::path::Path::new(".")).join("window.json")
}

/// Sanitize a deserialized [`WindowState`] into safe screen bounds.
fn sanitize(mut state: WindowState) -> WindowState {
    state.width = state.width.clamp(MIN_SIZE[0], 7680.0);
    state.height = state.height.clamp(MIN_SIZE[1], 4320.0);
    // A position far outside any sensible desktop, e.g. a monitor that is gone now.
    let sane = |v: f32| (-200.0..=7000.0).contains(&v);
    if !state.x.is_some_and(sane) || !state.y.is_some_and(sane) {
        state.x = None;
        state.y = None;
    }
    state
}

/// The saved state, made safe to apply.
pub fn load() -> WindowState {
    let state: WindowState = std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    sanitize(state)
}

/// Watches the window and writes the state when it changes.
pub struct Tracker {
    last: WindowState,
    checked: Instant,
}

impl Tracker {
    pub fn new() -> Self {
        Tracker { last: load(), checked: Instant::now() }
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        if self.checked.elapsed() < CHECK_EVERY {
            return;
        }
        self.checked = Instant::now();
        let (inner, outer, maximized, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.maximized.unwrap_or(false), v.minimized.unwrap_or(false))
        });
        if minimized {
            return;
        }
        let mut now = self.last;
        now.maximized = maximized;
        // While maximised the rectangles are the whole screen: keep the normal size.
        if !maximized && let (Some(inner), Some(outer)) = (inner, outer) {
            now.width = inner.width().round();
            now.height = inner.height().round();
            now.x = Some(outer.min.x.round());
            now.y = Some(outer.min.y.round());
        }
        if now != self.last {
            self.last = now;
            if let Ok(text) = serde_json::to_string(&now) {
                let _ = std::fs::write(path(), text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_legacy_and_partial_window_json() {
        let partial: WindowState = serde_json::from_str(r#"{"width": 1440, "height": 900}"#).unwrap();
        let clean = sanitize(partial);
        assert_eq!(clean.width, 1440.0);
        assert_eq!(clean.height, 900.0);
        assert_eq!(clean.x, None);
        assert!(!clean.maximized);

        let offscreen = sanitize(WindowState { width: 400.0, height: 300.0, x: Some(99999.0), y: Some(10.0), maximized: false });
        assert_eq!(offscreen.width, MIN_SIZE[0]);
        assert_eq!(offscreen.height, MIN_SIZE[1]);
        assert_eq!(offscreen.x, None);
    }
}
