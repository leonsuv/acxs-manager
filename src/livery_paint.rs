//! Painting on skin textures: brush, flood fill, rectangles, stamping a picture, and undo.
//!
//! The game's own skins stay as they are. [`save_as_new_skin`] copies the skin folder to a new one
//! and writes the painted texture there, so a mistake costs nothing.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};

use crate::dds::{self, Rgba};

/// Undo steps kept. Each is a full picture, so the count stays small.
const MAX_UNDO: usize = 12;

/// A picture being painted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pub image: Rgba,
    undo: Vec<Vec<u8>>,
}

impl Canvas {
    pub fn new(image: Rgba) -> Self {
        Canvas { image, undo: Vec::new() }
    }

    /// Remembers the picture before an edit.
    pub fn checkpoint(&mut self) {
        if self.undo.len() == MAX_UNDO {
            self.undo.remove(0);
        }
        self.undo.push(self.image.pixels.clone());
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(pixels) = self.undo.pop() {
            self.image.pixels = pixels;
        }
    }

    fn at(&self, x: i32, y: i32) -> Option<usize> {
        let (w, h) = (self.image.width as i32, self.image.height as i32);
        ((0..w).contains(&x) && (0..h).contains(&y)).then(|| (y as usize * w as usize + x as usize) * 4)
    }

    /// Paints `color` over a pixel by its alpha.
    fn blend(&mut self, x: i32, y: i32, color: [u8; 4]) {
        let Some(at) = self.at(x, y) else { return };
        let alpha = u32::from(color[3]);
        for i in 0..3 {
            let old = u32::from(self.image.pixels[at + i]);
            self.image.pixels[at + i] = ((u32::from(color[i]) * alpha + old * (255 - alpha)) / 255) as u8;
        }
    }

    /// A filled round dab.
    pub fn dab(&mut self, x: f32, y: f32, radius: f32, color: [u8; 4]) {
        let r = radius.max(0.5);
        let reach = r.ceil() as i32;
        let (cx, cy) = (x.round() as i32, y.round() as i32);
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                if ((dx * dx + dy * dy) as f32) <= r * r {
                    self.blend(cx + dx, cy + dy, color);
                }
            }
        }
    }

    /// A brush stroke between two points, with dabs close enough to leave no gaps.
    pub fn stroke(&mut self, from: (f32, f32), to: (f32, f32), radius: f32, color: [u8; 4]) {
        let length = (to.0 - from.0).hypot(to.1 - from.1);
        let steps = (length / (radius / 2.0).max(1.0)).ceil().max(1.0) as u32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            self.dab(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t, radius, color);
        }
    }

    pub fn rectangle(&mut self, a: (i32, i32), b: (i32, i32), color: [u8; 4]) {
        for y in a.1.min(b.1)..=a.1.max(b.1) {
            for x in a.0.min(b.0)..=a.0.max(b.0) {
                self.blend(x, y, color);
            }
        }
    }

    /// Fills the connected area around a point whose colour is within `tolerance` of it.
    pub fn fill(&mut self, x: i32, y: i32, color: [u8; 4], tolerance: u8) {
        let Some(start) = self.at(x, y) else { return };
        let target: [u8; 3] = [self.image.pixels[start], self.image.pixels[start + 1], self.image.pixels[start + 2]];
        let close = |pixel: &[u8]| (0..3).all(|i| pixel[i].abs_diff(target[i]) <= tolerance);
        let (w, h) = (self.image.width as usize, self.image.height as usize);
        let mut seen = vec![false; w * h];
        let mut queue = VecDeque::from([(x, y)]);
        seen[y as usize * w + x as usize] = true;
        while let Some((px, py)) = queue.pop_front() {
            let at = (py as usize * w + px as usize) * 4;
            self.image.pixels[at..at + 3].copy_from_slice(&color[..3]);
            for (nx, ny) in [(px - 1, py), (px + 1, py), (px, py - 1), (px, py + 1)] {
                if (0..w as i32).contains(&nx) && (0..h as i32).contains(&ny) {
                    let index = ny as usize * w + nx as usize;
                    if !seen[index] {
                        seen[index] = true;
                        // Compare against what the picture was before this fill reached it.
                        let pixel = &self.image.pixels[index * 4..index * 4 + 3];
                        if close(pixel) {
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }
        }
    }

    /// Draws a picture with its top left corner at a point, scaled by `scale`, keeping its alpha.
    pub fn stamp(&mut self, decal: &Rgba, x: i32, y: i32, scale: f32) {
        let scale = scale.max(0.01);
        let (w, h) = ((decal.width as f32 * scale) as i32, (decal.height as f32 * scale) as i32);
        for dy in 0..h {
            for dx in 0..w {
                let (sx, sy) = ((dx as f32 / scale) as usize, (dy as f32 / scale) as usize);
                let at = (sy.min(decal.height as usize - 1) * decal.width as usize + sx.min(decal.width as usize - 1)) * 4;
                let pixel = &decal.pixels[at..at + 4];
                self.blend(x + dx, y + dy, [pixel[0], pixel[1], pixel[2], pixel[3]]);
            }
        }
    }
}

/// The `.dds` textures of a skin folder, sorted by name.
pub fn textures(skin_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(skin_dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dds")))
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files
}

/// Reads a texture to paint on.
pub fn load(path: &Path) -> Result<Rgba, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    dds::decode(&bytes).map_err(|e| e.to_string())
}

/// Reads a PNG or JPEG for stamping.
pub fn load_decal(path: &Path) -> Result<Rgba, String> {
    let picture = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    Ok(Rgba { width: picture.width(), height: picture.height(), pixels: picture.into_raw() })
}

/// A folder name free next to `skin_dir`: `<skin>_custom`, then `<skin>_custom_2` and on.
fn free_name(skin_dir: &Path) -> io::Result<PathBuf> {
    let name = skin_dir.file_name().ok_or_else(|| io::Error::other("the skin has no folder name"))?.to_string_lossy().into_owned();
    let parent = skin_dir.parent().ok_or_else(|| io::Error::other("the skin has no parent folder"))?;
    let stem = name.split("_custom").next().unwrap_or(&name).to_string();
    let mut candidate = parent.join(format!("{stem}_custom"));
    let mut n = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{stem}_custom_{n}"));
        n += 1;
    }
    Ok(candidate)
}

/// Copies the skin to a new folder and writes the painted texture into it. A skin of ours, one
/// ending in `_custom`, is updated in place instead. Returns the folder written.
pub fn save_as_new_skin(skin_dir: &Path, texture_name: &str, image: &Rgba) -> io::Result<PathBuf> {
    let ours = skin_dir.file_name().is_some_and(|n| n.to_string_lossy().contains("_custom"));
    let target = if ours {
        skin_dir.to_path_buf()
    } else {
        let target = free_name(skin_dir)?;
        crate::util::copy_dir_all(skin_dir, &target)?;
        target
    };
    std::fs::write(target.join(texture_name), dds::encode(image))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];

    fn blank(size: u32) -> Canvas {
        Canvas::new(Rgba { width: size, height: size, pixels: WHITE.repeat((size * size) as usize) })
    }

    fn pixel(canvas: &Canvas, x: usize, y: usize) -> [u8; 3] {
        let at = (y * canvas.image.width as usize + x) * 4;
        [canvas.image.pixels[at], canvas.image.pixels[at + 1], canvas.image.pixels[at + 2]]
    }

    #[test]
    fn a_stroke_has_no_gaps_and_undo_brings_it_back() {
        let mut canvas = blank(64);
        canvas.checkpoint();
        canvas.stroke((2.0, 10.0), (60.0, 10.0), 2.0, RED);
        for x in 2..=60 {
            assert_eq!(pixel(&canvas, x, 10), [255, 0, 0], "gap at {x}");
        }
        assert_eq!(pixel(&canvas, 30, 20), [255, 255, 255]);
        canvas.undo();
        assert_eq!(pixel(&canvas, 30, 10), [255, 255, 255]);
        assert!(!canvas.can_undo());
    }

    #[test]
    fn painting_past_the_edge_is_harmless_and_alpha_blends() {
        let mut canvas = blank(8);
        canvas.stroke((-50.0, -50.0), (100.0, 100.0), 3.0, RED);
        let mut half = blank(2);
        half.dab(0.0, 0.0, 0.5, [0, 0, 0, 128]);
        assert_eq!(pixel(&half, 0, 0), [127, 127, 127]);
    }

    #[test]
    fn fill_stops_at_a_line() {
        let mut canvas = blank(16);
        canvas.rectangle((8, 0), (8, 15), RED);
        canvas.fill(2, 2, [0, 0, 255, 255], 8);
        assert_eq!(pixel(&canvas, 7, 15), [0, 0, 255]);
        assert_eq!(pixel(&canvas, 8, 3), [255, 0, 0]);
        assert_eq!(pixel(&canvas, 9, 3), [255, 255, 255]);
    }

    #[test]
    fn a_decal_lands_scaled_with_its_clear_parts_left_alone() {
        let mut canvas = blank(16);
        let decal = Rgba { width: 2, height: 1, pixels: vec![255, 0, 0, 255, 0, 0, 0, 0] };
        canvas.stamp(&decal, 1, 1, 2.0);
        assert_eq!(pixel(&canvas, 2, 2), [255, 0, 0]);
        assert_eq!(pixel(&canvas, 1, 1), [255, 0, 0]);
        assert_eq!(pixel(&canvas, 3, 2), [255, 255, 255]);
    }

    #[test]
    fn the_original_skin_is_never_written() {
        let root = std::env::temp_dir().join(format!("acxs-paint-{}", std::process::id()));
        let skin = root.join("red_01");
        std::fs::create_dir_all(&skin).unwrap();
        std::fs::write(skin.join("body.dds"), b"original").unwrap();
        std::fs::write(skin.join("preview.jpg"), b"p").unwrap();
        let image = blank(4).image;
        let first = save_as_new_skin(&skin, "body.dds", &image).unwrap();
        assert_eq!(first, root.join("red_01_custom"));
        assert_eq!(std::fs::read(skin.join("body.dds")).unwrap(), b"original");
        assert!(first.join("preview.jpg").is_file());
        assert_eq!(load(&first.join("body.dds")).unwrap(), image);
        assert_eq!(save_as_new_skin(&first, "body.dds", &image).unwrap(), first, "our own copy is updated");
        assert_eq!(save_as_new_skin(&skin, "body.dds", &image).unwrap(), root.join("red_01_custom_2"));
        std::fs::remove_dir_all(&root).ok();
    }
}
