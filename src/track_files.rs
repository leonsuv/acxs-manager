//! Readers, non-destructive writers, and map generators for Assetto Corsa track
//! data files: `drs_zones.ini`, `cameras*.ini`, `sections.ini`, `ai_hints.ini`,
//! `map.ini`, and `map.png`.

use std::io;
use std::path::Path;

use crate::ai_spline::AiSpline;
use crate::ini::Ini;
use crate::png::{self, PngError};

/// One DRS detection/activation zone from `data/drs_zones.ini`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrsZone {
    pub detection: f32,
    pub start: f32,
    pub end: f32,
}

/// One trackside TV camera from `data/cameras*.ini`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackCamera {
    pub name: String,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub min_fov: f32,
    pub max_fov: f32,
    pub in_point: f32,
    pub out_point: f32,
}

/// One named corner/sector from `ui/sections.ini` (or `data/sections.ini`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSection {
    pub in_point: f32,
    pub out_point: f32,
    pub name: String,
}

/// One AI hint zone from `data/ai_hints.ini`.
#[derive(Debug, Clone, PartialEq)]
pub struct AiHint {
    pub section: String,
    pub start: f32,
    pub end: f32,
    pub value: f32,
}

/// World-to-image transform stored in `data/map.ini`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapIni {
    pub width: u32,
    pub height: u32,
    pub margin: u32,
    pub scale_factor: f32,
    pub x_offset: f32,
    pub z_offset: f32,
    pub drawing_size: u32,
}

fn parse_f32(ini: &Ini, section: &str, key: &str) -> Option<f32> {
    ini.get(section, key)?
        .split(';')
        .next()?
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
}

fn parse_vec3(ini: &Ini, section: &str, key: &str) -> Option<[f32; 3]> {
    let raw = ini.get(section, key)?.split(';').next()?.trim();
    let parts: Vec<f32> = raw
        .split(',')
        .map(|s| s.trim().parse::<f32>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() == 3 && parts.iter().all(|v| v.is_finite()) {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

/// Reads `data/drs_zones.ini`. Missing file returns an empty list.
pub fn read_drs_zones(data_dir: &Path) -> io::Result<Vec<DrsZone>> {
    let ini = match Ini::load(&data_dir.join("drs_zones.ini")) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut zones = Vec::new();
    for idx in 0u32.. {
        let sec = format!("ZONE_{idx}");
        let (Some(det), Some(start), Some(end)) = (
            parse_f32(&ini, &sec, "DETECTION"),
            parse_f32(&ini, &sec, "START"),
            parse_f32(&ini, &sec, "END"),
        ) else {
            break;
        };
        zones.push(DrsZone {
            detection: det,
            start,
            end,
        });
    }
    Ok(zones)
}

/// Reads `cameras.ini` (or any `cameras_*.ini` file). Missing file returns empty.
pub fn read_cameras(path: &Path) -> io::Result<Vec<TrackCamera>> {
    let ini = match Ini::load(path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut cams = Vec::new();
    for idx in 0u32.. {
        let sec = format!("CAMERA_{idx}");
        let Some(position) = parse_vec3(&ini, &sec, "POSITION") else {
            break;
        };
        let name = ini
            .get(&sec, "NAME")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Camera {idx}"));
        cams.push(TrackCamera {
            name,
            position,
            forward: parse_vec3(&ini, &sec, "FORWARD").unwrap_or([0.0, 0.0, 1.0]),
            up: parse_vec3(&ini, &sec, "UP").unwrap_or([0.0, 1.0, 0.0]),
            min_fov: parse_f32(&ini, &sec, "MIN_FOV").unwrap_or(20.0),
            max_fov: parse_f32(&ini, &sec, "MAX_FOV").unwrap_or(60.0),
            in_point: parse_f32(&ini, &sec, "IN_POINT").unwrap_or(0.0),
            out_point: parse_f32(&ini, &sec, "OUT_POINT").unwrap_or(1.0),
        });
    }
    Ok(cams)
}

/// Reads `sections.ini` (checking `data/sections.ini` or `ui/sections.ini`).
pub fn read_sections(path: &Path) -> io::Result<Vec<TrackSection>> {
    let ini = match Ini::load(path) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for idx in 0u32.. {
        let sec = format!("SECTION_{idx}");
        let Some(name) = ini
            .get(&sec, "TEXT")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        else {
            break;
        };
        let in_point = parse_f32(&ini, &sec, "IN").unwrap_or(0.0);
        let out_point = parse_f32(&ini, &sec, "OUT").unwrap_or(1.0);
        out.push(TrackSection {
            in_point,
            out_point,
            name,
        });
    }
    Ok(out)
}

/// Reads `data/ai_hints.ini` (`[HINT_0..N]`, `[BRAKEHINT_0..N]`, etc.).
pub fn read_ai_hints(data_dir: &Path) -> io::Result<Vec<AiHint>> {
    let ini = match Ini::load(&data_dir.join("ai_hints.ini")) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut hints = Vec::new();
    for section in ini.section_names() {
        let (Some(start), Some(end), Some(value)) = (
            parse_f32(&ini, section, "START"),
            parse_f32(&ini, section, "END"),
            parse_f32(&ini, section, "VALUE"),
        ) else {
            continue;
        };
        hints.push(AiHint {
            section: section.to_string(),
            start,
            end,
            value,
        });
    }
    Ok(hints)
}

/// Saves `ai_hints.ini` non-destructively (creates `.acxs-backup-<stamp>` if it exists).
pub fn save_ai_hints(data_dir: &Path, hints: &[AiHint]) -> io::Result<()> {
    let path = data_dir.join("ai_hints.ini");
    let mut ini = if path.is_file() {
        Ini::load(&path)?
    } else {
        Ini::parse("")
    };
    for hint in hints {
        ini.set(&hint.section, "START", format!("{:.4}", hint.start));
        ini.set(&hint.section, "END", format!("{:.4}", hint.end));
        ini.set(&hint.section, "VALUE", format!("{:.4}", hint.value));
    }
    ini.save_with_backup(&path)
}

/// Reads `data/map.ini`.
pub fn read_map_ini(data_dir: &Path) -> io::Result<Option<MapIni>> {
    let ini = match Ini::load(&data_dir.join("map.ini")) {
        Ok(ini) => ini,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let Some(width) = parse_f32(&ini, "PARAMETERS", "WIDTH") else {
        return Ok(None);
    };
    let Some(height) = parse_f32(&ini, "PARAMETERS", "HEIGHT") else {
        return Ok(None);
    };
    Ok(Some(MapIni {
        width: width.max(1.0) as u32,
        height: height.max(1.0) as u32,
        margin: parse_f32(&ini, "PARAMETERS", "MARGIN").unwrap_or(20.0) as u32,
        scale_factor: parse_f32(&ini, "PARAMETERS", "SCALE_FACTOR").unwrap_or(1.0),
        x_offset: parse_f32(&ini, "PARAMETERS", "X_OFFSET").unwrap_or(0.0),
        z_offset: parse_f32(&ini, "PARAMETERS", "Z_OFFSET").unwrap_or(0.0),
        drawing_size: parse_f32(&ini, "PARAMETERS", "DRAWING_SIZE").unwrap_or(10.0) as u32,
    }))
}

/// Formats a [`MapIni`] into Assetto Corsa's `data/map.ini` text format.
pub fn format_map_ini(map: &MapIni) -> String {
    format!(
        "[PARAMETERS]\r\nWIDTH={}\r\nHEIGHT={}\r\nMARGIN={}\r\nSCALE_FACTOR={:.4}\r\nX_OFFSET={:.4}\r\nZ_OFFSET={:.4}\r\nDRAWING_SIZE={}\r\n",
        map.width,
        map.height,
        map.margin,
        map.scale_factor,
        map.x_offset,
        map.z_offset,
        map.drawing_size
    )
}

/// Renders a clean transparent-background track `map.png` and matching [`MapIni`]
/// directly from an [`AiSpline`].
pub fn generate_track_map_png(
    spline: &AiSpline,
    max_dim: u32,
    margin: u32,
) -> Result<(Vec<u8>, MapIni), PngError> {
    let max_dim = max_dim.clamp(64, 4096);
    let margin = margin.min(max_dim / 4);
    let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut min_z, mut max_z) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in &spline.points {
        min_x = min_x.min(p.x);
        max_x = max_x.max(p.x);
        min_z = min_z.min(p.z);
        max_z = max_z.max(p.z);
    }
    if !min_x.is_finite() || !max_x.is_finite() {
        let map = MapIni {
            width: 64,
            height: 64,
            margin: 8,
            scale_factor: 1.0,
            x_offset: 0.0,
            z_offset: 0.0,
            drawing_size: 10,
        };
        let png_bytes = png::encode_rgba(64, 64, &vec![0u8; 64 * 64 * 4])?;
        return Ok((png_bytes, map));
    }

    let span_x = (max_x - min_x).max(1.0);
    let span_z = (max_z - min_z).max(1.0);
    let usable = (max_dim - 2 * margin) as f32;
    let scale_factor = (span_x.max(span_z) / usable).max(1e-4);
    let width = ((span_x / scale_factor).ceil() as u32 + 2 * margin).clamp(64, max_dim);
    let height = ((span_z / scale_factor).ceil() as u32 + 2 * margin).clamp(64, max_dim);
    let x_offset = -min_x + margin as f32 * scale_factor;
    let z_offset = -min_z + margin as f32 * scale_factor;

    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    let to_px = |x: f32, z: f32| -> (f32, f32) {
        ((x + x_offset) / scale_factor, (z + z_offset) / scale_factor)
    };

    let n = spline.points.len();
    for i in 0..n {
        let a = &spline.points[i];
        let b = &spline.points[(i + 1) % n];
        let (x0, y0) = to_px(a.x, a.z);
        let (x1, y1) = to_px(b.x, b.z);
        draw_thick_segment(&mut rgba, width, height, (x0, y0), (x1, y1), 3.0, [245, 245, 245, 255]);
    }

    let map = MapIni {
        width,
        height,
        margin,
        scale_factor,
        x_offset,
        z_offset,
        drawing_size: 10,
    };
    let png_bytes = png::encode_rgba(width, height, &rgba)?;
    Ok((png_bytes, map))
}

fn draw_thick_segment(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    a: (f32, f32),
    b: (f32, f32),
    radius: f32,
    color: [u8; 4],
) {
    let dist = (b.0 - a.0).hypot(b.1 - a.1);
    let steps = (dist * 2.0).ceil().max(1.0) as u32;
    let r_i = radius.ceil() as i32;
    let r2 = radius * radius;
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let cx = a.0 + (b.0 - a.0) * t;
        let cy = a.1 + (b.1 - a.1) * t;
        let ix = cx.round() as i32;
        let iy = cy.round() as i32;
        for dy in -r_i..=r_i {
            for dx in -r_i..=r_i {
                if (dx * dx + dy * dy) as f32 <= r2 {
                    let px = ix + dx;
                    let py = iy + dy;
                    if px >= 0 && py >= 0 && (px as u32) < width && (py as u32) < height {
                        let off = (py as usize * width as usize + px as usize) * 4;
                        rgba[off..off + 4].copy_from_slice(&color);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_spline::SplinePoint;
    use std::fs;

    #[test]
    fn parses_track_data_files_and_saves_ai_hints_with_backup() {
        let dir = std::env::temp_dir().join(format!("acxs_track_files_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("drs_zones.ini"),
            "[ZONE_0]\r\nDETECTION=0.72\r\nSTART=0.78\r\nEND=0.95\r\n",
        )
        .unwrap();
        fs::write(
            dir.join("cameras.ini"),
            "[CAMERA_0]\r\nNAME=Main Straight\r\nPOSITION=10,5,20\r\nFORWARD=0,0,1\r\nUP=0,1,0\r\nMIN_FOV=15\r\nMAX_FOV=55\r\nIN_POINT=0.0\r\nOUT_POINT=0.25\r\n",
        )
        .unwrap();
        fs::write(
            dir.join("sections.ini"),
            "[SECTION_0]\r\nIN=0.1\r\nOUT=0.2\r\nTEXT=Turn 1\r\n",
        )
        .unwrap();
        fs::write(
            dir.join("ai_hints.ini"),
            "[HINT_0]\r\nSTART=0.10\r\nEND=0.15\r\nVALUE=0.85\r\n",
        )
        .unwrap();

        let drs = read_drs_zones(&dir).unwrap();
        assert_eq!(drs.len(), 1);
        assert!((drs[0].start - 0.78).abs() < 1e-5);

        let cams = read_cameras(&dir.join("cameras.ini")).unwrap();
        assert_eq!(cams.len(), 1);
        assert_eq!(cams[0].name, "Main Straight");
        assert_eq!(cams[0].position, [10.0, 5.0, 20.0]);

        let secs = read_sections(&dir.join("sections.ini")).unwrap();
        assert_eq!(secs.len(), 1);
        assert_eq!(secs[0].name, "Turn 1");

        let mut hints = read_ai_hints(&dir).unwrap();
        assert_eq!(hints.len(), 1);
        hints[0].value = 0.75;
        save_ai_hints(&dir, &hints).unwrap();
        let reloaded = read_ai_hints(&dir).unwrap();
        assert!((reloaded[0].value - 0.75).abs() < 1e-4);
        let backups = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("ai_hints.ini.acxs-backup-"))
            .count();
        assert_eq!(backups, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generates_valid_map_png_and_map_ini() {
        let mut points = Vec::new();
        for i in 0..64 {
            let a = std::f32::consts::TAU * i as f32 / 64.0;
            points.push(SplinePoint {
                x: 200.0 * a.cos(),
                y: 0.0,
                z: 100.0 * a.sin(),
                length_m: i as f32 * 10.0,
                id: i,
            });
        }
        let spline = AiSpline {
            version: 7,
            lap_time_ms: 30_000,
            sample_count: 64,
            points,
            extra: None,
        };
        let (png_bytes, map) = generate_track_map_png(&spline, 512, 20).unwrap();
        assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(map.width <= 512 && map.height <= 512);
        assert!(map.scale_factor > 0.0);
        let ini_text = format_map_ini(&map);
        assert!(ini_text.contains("[PARAMETERS]"));
        assert!(ini_text.contains("SCALE_FACTOR="));
    }
}
