//! Display names of installed cars and tracks, for pages that hold only folder ids.
//!
//! The first request scans on a background thread; until it lands, ids are made readable.

use acxs_manager::content;
use eframe::egui;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default)]
pub struct Names {
    root: Option<PathBuf>,
    cars: BTreeMap<String, String>,
    tracks: BTreeMap<String, String>,
}

static NAMES: OnceLock<Mutex<Arc<Names>>> = OnceLock::new();

/// `ks_porsche_911_gt3_r` as `Porsche 911 Gt3 R`.
fn readable(id: &str) -> String {
    let id = id.strip_prefix("ks_").unwrap_or(id);
    id.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Names {
    pub fn car(&self, id: &str) -> String {
        self.cars.get(id).cloned().unwrap_or_else(|| readable(id))
    }

    pub fn track(&self, id: &str) -> String {
        self.tracks.get(id).cloned().unwrap_or_else(|| readable(id))
    }

    pub fn layout(&self, layout: &str) -> String {
        readable(layout)
    }
}

/// The names for an install, scanning them once.
pub fn names(root: &Path, ctx: &egui::Context) -> Arc<Names> {
    let slot = NAMES.get_or_init(|| Mutex::new(Arc::new(Names::default())));
    let Ok(mut guard) = slot.lock() else { return Arc::new(Names::default()) };
    if guard.root.as_deref() != Some(root) {
        // Claim the install now, so frames before the scan lands do not start another.
        *guard = Arc::new(Names { root: Some(root.to_path_buf()), ..Names::default() });
        let (root, ctx) = (root.to_path_buf(), ctx.clone());
        std::thread::spawn(move || {
            let cars = content::scan_cars(&root).into_iter().map(|c| (c.id, c.name)).collect();
            let tracks = content::scan_tracks(&root).into_iter().map(|t| (t.id, t.name)).collect();
            if let Some(slot) = NAMES.get()
                && let Ok(mut guard) = slot.lock()
            {
                *guard = Arc::new(Names { root: Some(root), cars, tracks });
            }
            ctx.request_repaint();
        });
    }
    Arc::clone(&guard)
}
