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
    /// Sessions the recorder has saved since the app started.
    pub sessions_saved: usize,
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
mod game_options;
mod quick_race;
mod events;
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
        game_options::tool(),
        quick_race::tool(),
        events::tool(),
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

#[cfg(test)]
mod headless_tests {
    use super::*;

    fn draw_every_tool(root: Option<PathBuf>, game_running: bool) {
        let mut tools = registry();
        for index in 0..tools.len() {
            let ctx = egui::Context::default();
            crate::ui::theme::apply(&ctx);
            let tool_ctx = ToolContext {
                ac_valid: root.is_some(),
                ac_root: root.clone(),
                game_running,
                sessions_saved: 0,
            };
            let mut notices = Vec::new();
            for frame in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                    time: Some(f64::from(frame) * 0.05),
                    ..egui::RawInput::default()
                };
                let tool = &mut tools[index];
                let mut output = ctx.run_ui(input, |ui| {
                    tool.ui(ui, &tool_ctx, &mut notices);
                    tool.tick();
                });
                output.textures_delta.clear();
                // Background scans finish between frames in the app too.
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
    }

    #[test]
    fn every_page_draws_without_a_game_folder() {
        draw_every_tool(None, false);
    }

    #[test]
    fn every_page_draws_with_an_empty_game_folder_while_the_game_runs() {
        let root = std::env::temp_dir().join(format!("acxs-headless-{}", std::process::id()));
        std::fs::create_dir_all(root.join("content").join("cars")).unwrap();
        std::fs::create_dir_all(root.join("content").join("tracks")).unwrap();
        draw_every_tool(Some(root.clone()), true);
        std::fs::remove_dir_all(&root).ok();
    }
}

/// Draws pages to PNG with no window, to look at them while the app may not run:
/// `ACXS_RENDER_DIR=<dir> cargo test --bin acxs-manager render_pages -- --ignored`.
/// `ACXS_RENDER_PAGES` limits it to a comma-separated list. Pictures from disk stay blank.
#[cfg(test)]
mod render_tests {
    use super::*;
    use eframe::egui::epaint::{ImageDelta, Primitive, Vertex, textures::TexturesDelta};
    use std::collections::HashMap;

    const WIDTH: usize = 1400;
    const HEIGHT: usize = 1000;

    #[derive(Default)]
    struct Canvas {
        textures: HashMap<egui::TextureId, (usize, usize, Vec<egui::Color32>)>,
        pixels: Vec<[f32; 3]>,
    }

    fn apply_image(slot: &mut (usize, usize, Vec<egui::Color32>), delta: &ImageDelta) {
        let egui::ImageData::Color(image) = &delta.image;
        let (w, h) = (image.size[0], image.size[1]);
        match delta.pos {
            None => *slot = (w, h, image.pixels.clone()),
            Some([x0, y0]) => {
                for y in 0..h {
                    for x in 0..w {
                        slot.2[(y0 + y) * slot.0 + x0 + x] = image.pixels[y * w + x];
                    }
                }
            }
        }
    }

    impl Canvas {
        fn update(&mut self, delta: &mut TexturesDelta) {
            for (id, images) in &delta.set {
                for image in images.iter() {
                    apply_image(self.textures.entry(*id).or_default(), image);
                }
            }
            delta.clear();
        }

        fn triangle(&mut self, a: &Vertex, b: &Vertex, c: &Vertex, clip: egui::Rect, texture: egui::TextureId) {
            let texture = self.textures.get(&texture);
            let area = (b.pos - a.pos).x * (c.pos - a.pos).y - (b.pos - a.pos).y * (c.pos - a.pos).x;
            if area.abs() < 1e-6 {
                return;
            }
            let left = a.pos.x.min(b.pos.x).min(c.pos.x).max(clip.left()).max(0.0).floor() as usize;
            let right = (a.pos.x.max(b.pos.x).max(c.pos.x).min(clip.right()).ceil() as usize).min(WIDTH);
            let top = a.pos.y.min(b.pos.y).min(c.pos.y).max(clip.top()).max(0.0).floor() as usize;
            let bottom = (a.pos.y.max(b.pos.y).max(c.pos.y).min(clip.bottom()).ceil() as usize).min(HEIGHT);
            for y in top..bottom {
                for x in left..right {
                    let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                    let w1 = ((p - a.pos).x * (c.pos - a.pos).y - (p - a.pos).y * (c.pos - a.pos).x) / area;
                    let w2 = ((b.pos - a.pos).x * (p - a.pos).y - (b.pos - a.pos).y * (p - a.pos).x) / area;
                    let w0 = 1.0 - w1 - w2;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let mix = |pick: fn(&Vertex) -> f32| w0 * pick(a) + w1 * pick(b) + w2 * pick(c);
                    let mut rgba = [
                        mix(|v| f32::from(v.color.r())),
                        mix(|v| f32::from(v.color.g())),
                        mix(|v| f32::from(v.color.b())),
                        mix(|v| f32::from(v.color.a())),
                    ];
                    if let Some((tw, th, texels)) = texture {
                        let u = ((mix(|v| v.uv.x) * *tw as f32) as usize).min(tw - 1);
                        let v = ((mix(|v| v.uv.y) * *th as f32) as usize).min(th - 1);
                        let t = texels[v * tw + u];
                        for (i, value) in [t.r(), t.g(), t.b(), t.a()].into_iter().enumerate() {
                            rgba[i] *= f32::from(value) / 255.0;
                        }
                    }
                    let out = &mut self.pixels[y * WIDTH + x];
                    for i in 0..3 {
                        out[i] = rgba[i] + out[i] * (1.0 - rgba[3] / 255.0);
                    }
                }
            }
        }

        fn draw(&mut self, ctx: &egui::Context, shapes: Vec<egui::epaint::ClippedShape>) {
            self.pixels = vec![[0x0B as f32, 0x0D as f32, 0x11 as f32]; WIDTH * HEIGHT];
            for primitive in ctx.tessellate(shapes, 1.0) {
                let Primitive::Mesh(mesh) = primitive.primitive else { continue };
                for tri in mesh.indices.chunks_exact(3) {
                    let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices[i as usize]);
                    self.triangle(&a, &b, &c, primitive.clip_rect, mesh.texture_id);
                }
            }
        }

        fn save(&self, path: &std::path::Path) {
            let bytes: Vec<u8> = self.pixels.iter().flat_map(|p| [p[0] as u8, p[1] as u8, p[2] as u8, 255]).collect();
            std::fs::write(path, acxs_manager::png::encode_rgba(WIDTH as u32, HEIGHT as u32, &bytes).unwrap()).unwrap();
        }
    }

    #[test]
    #[ignore = "writes pictures; needs ACXS_RENDER_DIR"]
    fn render_pages() {
        let dir = PathBuf::from(std::env::var("ACXS_RENDER_DIR").expect("ACXS_RENDER_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let wanted: Vec<String> = std::env::var("ACXS_RENDER_PAGES")
            .map(|list| list.split(',').map(str::to_string).collect())
            .unwrap_or_default();
        let root = acxs_manager::settings::detect_ac_root();
        let mut tools = registry();
        for tool in &mut tools {
            let name = tool.name();
            if !wanted.is_empty() && !wanted.iter().any(|w| w == name) {
                continue;
            }
            let ctx = egui::Context::default();
            crate::ui::theme::apply(&ctx);
            let tool_ctx = ToolContext { ac_valid: root.is_some(), ac_root: root.clone(), game_running: false, sessions_saved: 0 };
            let mut canvas = Canvas::default();
            let mut notices = Vec::new();
            // Enough frames for background loads to land.
            for frame in 0..40 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH as f32, HEIGHT as f32))),
                    time: Some(f64::from(frame) * 0.1),
                    ..egui::RawInput::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    egui::Frame::central_panel(ui.style()).show(ui, |ui| tool.ui(ui, &tool_ctx, &mut notices));
                });
                canvas.update(&mut output.textures_delta);
                if frame == 39 {
                    canvas.draw(&ctx, output.shapes);
                }
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            canvas.save(&dir.join(format!("{}.png", name.replace([' ', '&'], "_"))));
        }
    }
}
