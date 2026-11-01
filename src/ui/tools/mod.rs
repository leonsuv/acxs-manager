//! Tool pages: one file per feature, each implementing [`Tool`].
//!
//! To add a feature page, write `src/ui/tools/<name>.rs` with a
//! `pub fn tool() -> Box<dyn Tool>`, add `mod <name>;` below, and list it in
//! [`registry`]. Tools call the core library through `acxs_manager::...` and
//! must never block the UI thread on long work: run it on a thread and report
//! back through the notices vector.

use crate::ui::Notice;
use eframe::egui;
use std::path::PathBuf;

/// What a tool page can read from the app on each frame.
pub struct ToolContext {
    pub ac_root: Option<PathBuf>,
    pub ac_valid: bool,
    pub game_running: bool,
}

mod content_editor;
mod car_specs;
mod skins;
mod track_info;
mod mods_manager;
mod setups;
mod server;
mod replay;
mod laptimes;
mod sessions;
mod live_telemetry;
mod csp_config;
mod game_settings;
mod quick_race;
mod collections;
mod install;
mod model_tools;
mod logs;
#[allow(dead_code)]
mod rtx;
mod packages;
mod setup_advice;
mod plate;
mod hud_editor;
mod launcher;
mod profiles;
mod timing;
mod engineer;
mod championship;
mod exports;
mod car_tuning;
mod strategy;
mod lights_profile;

/// What a tool asks the user to choose on the Cars or Tracks page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Car,
    Track,
}

pub trait Tool {
    /// Label in the tool list.
    fn name(&self) -> &'static str;
    /// Group heading, e.g. "Content", "Setup", "Server", "Telemetry", "Config".
    fn group(&self) -> &'static str;
    /// Draw the page. Push status messages into `notices`.
    fn ui(&mut self, ui: &mut egui::Ui, ctx: &ToolContext, notices: &mut Vec<Notice>);
    /// Called every frame for every tool, also while another page is shown, so
    /// background work (live telemetry, the web dashboard) keeps flowing.
    fn tick(&mut self) {}
    /// Fill in a race: other pages offer "Race this car / on this track" and hand the choice here.
    fn set_race(&mut self, _track: Option<&str>, _layout: Option<&str>, _car: Option<&str>, _skin: Option<&str>) {}
    /// The tool wants the user to pick a car or track on its page; taken once by the app.
    fn take_pick(&mut self) -> Option<PickKind> {
        None
    }
    /// The tool wants to open another tool page, by name; taken once by the app.
    fn take_goto(&mut self) -> Option<&'static str> {
        None
    }
    /// Files dropped onto the window. Return true when this tool took them.
    fn on_drop(&mut self, _ctx: &egui::Context, _paths: &[PathBuf]) -> bool {
        false
    }
}

/// Every tool page, in display order.
pub fn registry() -> Vec<Box<dyn Tool>> {
    vec![
        content_editor::tool(),
        car_specs::tool(),
        skins::tool(),
        track_info::tool(),
        mods_manager::tool(),
        setups::tool(),
        server::tool(),
        replay::tool(),
        laptimes::tool(),
        sessions::tool(),
        live_telemetry::tool(),
        csp_config::tool(),
        game_settings::tool(),
        quick_race::tool(),
        collections::tool(),
        install::tool(),
        model_tools::tool(),
        logs::tool(),
        // RTX & ReShade is switched off for now: it does not work yet.
        // rtx::tool(),
        packages::tool(),
        setup_advice::tool(),
        plate::tool(),
        hud_editor::tool(),
        launcher::tool(),
        profiles::tool(),
        timing::tool(),
        engineer::tool(),
        championship::tool(),
        exports::tool(),
        car_tuning::tool(),
        strategy::tool(),
        lights_profile::tool(),
    ]
}
