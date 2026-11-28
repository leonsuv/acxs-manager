//! Full scene loader for Assetto Corsa `.kn5` models, multi-KN5 track layouts
//! (`models.ini` / `models_<layout>.ini`), and car LODs (`data/lods.ini`) with
//! embedded texture retention, skin `.dds` overrides, and AC node classification.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::cardata;
use crate::ini::Ini;
use crate::kn5::Kn5Error;

const MAGIC: &[u8; 6] = b"sc6969";
const CSP_MAGIC: &[u8; 30] = b"__AC_SHADERS_PATCH_KN5ENC_v1__";
const MAX_STRING_BYTES: usize = 64 * 1024;
const MAX_NODES: usize = 1_000_000;
/// Default vertex cap (matches `kn5_export`).
pub const DEFAULT_MAX_VERTICES: usize = 8_000_000;
/// Default triangle cap (matches `kn5_export`).
pub const DEFAULT_MAX_TRIANGLES: usize = 8_000_000;
/// Default cap on a single embedded texture payload (64 MiB).
const MAX_TEXTURE_BYTES: u64 = 64 * 1024 * 1024;

/// Row-major 4x4 matrix. Vectors are row vectors: `v' = [x y z 1] * M`.
pub type Mat4 = [[f64; 4]; 4];

pub const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f64; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[i][k] * b[k][j]).sum::<f64>();
        }
    }
    out
}

pub fn mat4_transform(m: &Mat4, v: &[f32; 3], is_point: bool) -> [f32; 3] {
    let w = if is_point { 1.0 } else { 0.0 };
    let (x, y, z) = (f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let mut out = [0.0f32; 3];
    for (j, o) in out.iter_mut().enumerate() {
        *o = (x * m[0][j] + y * m[1][j] + z * m[2][j] + w * m[3][j]) as f32;
    }
    out
}

/// Translation vector stored in row 3 of a row-major KN5 matrix.
pub fn mat4_translation(m: &Mat4) -> [f32; 3] {
    [m[3][0] as f32, m[3][1] as f32, m[3][2] as f32]
}

/// Options controlling how much data [`load_kn5`] retains in memory.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    pub load_textures: bool,
    pub skip_ac_helpers: bool,
    pub max_vertices: usize,
    pub max_triangles: usize,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            load_textures: true,
            skip_ac_helpers: false,
            max_vertices: DEFAULT_MAX_VERTICES,
            max_triangles: DEFAULT_MAX_TRIANGLES,
        }
    }
}

/// An embedded texture from a KN5 file (or an external skin override).
#[derive(Debug, Clone)]
pub struct SceneTexture {
    pub name: String,
    pub active: bool,
    /// Raw DDS or PNG bytes. Empty when `LoadOptions::load_textures` is false.
    pub bytes: Vec<u8>,
}

/// Material metadata parsed from a KN5 file.
#[derive(Debug, Clone)]
pub struct SceneMaterial {
    pub name: String,
    pub shader: String,
    pub blend_mode: u8,
    pub alpha_tested: bool,
    pub depth_mode: i32,
    pub ks_ambient: f32,
    pub ks_diffuse: f32,
    pub ks_specular: f32,
    pub ks_specular_exp: f32,
    pub diffuse_mult: f32,
    pub use_detail: f32,
    pub detail_uv_multiplier: f32,
    pub tx_diffuse: String,
    pub tx_normal: String,
    pub tx_detail: String,
    pub tx_detail_a: String,
}

impl Default for SceneMaterial {
    fn default() -> Self {
        Self {
            name: String::new(),
            shader: String::new(),
            blend_mode: 0,
            alpha_tested: false,
            depth_mode: 0,
            ks_ambient: 0.6,
            ks_diffuse: 0.6,
            ks_specular: 0.9,
            ks_specular_exp: 1.0,
            diffuse_mult: 1.0,
            use_detail: 0.0,
            detail_uv_multiplier: 1.0,
            tx_diffuse: String::new(),
            tx_normal: String::new(),
            tx_detail: String::new(),
            tx_detail_a: String::new(),
        }
    }
}

/// Classification of an AC dummy helper node (`AC_PIT_0`, `AC_START_0`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperKind {
    Pit(u32),
    Start(u32),
    HotlapStart(u32),
    TimingGate { index: u32, side: char },
    AbStart { side: char },
    Crew(u32),
    Other(String),
}

/// Classification of common AC car sub-part nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarPartKind {
    WheelLf,
    WheelRf,
    WheelLr,
    WheelRr,
    Steer,
    Cockpit,
}

/// Semantic role inferred from AC's node naming conventions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    /// Track physics surface mesh (`1ROAD*`, `1KERB*`, `1GRASS*`, `1SAND*`, `1PIT*`, `1WALL*`).
    SurfaceMesh { prefix_num: u32, surface_key: String },
    /// Track or car helper dummy (`AC_*`).
    HelperDummy(HelperKind),
    /// Named car dynamic sub-part (`WHEEL_LF`, `STEER_HR`, etc.).
    CarPart(CarPartKind),
    /// Regular visual mesh.
    VisualMesh,
}

/// Classify a KN5 node name using Assetto Corsa naming conventions.
pub fn classify_node_name(name: &str) -> NodeRole {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("AC_") {
        return NodeRole::HelperDummy(classify_helper(&upper, name));
    }
    if upper.starts_with("WHEEL_LF") || upper.starts_with("HUB_LF") {
        return NodeRole::CarPart(CarPartKind::WheelLf);
    }
    if upper.starts_with("WHEEL_RF") || upper.starts_with("HUB_RF") {
        return NodeRole::CarPart(CarPartKind::WheelRf);
    }
    if upper.starts_with("WHEEL_LR") || upper.starts_with("HUB_LR") {
        return NodeRole::CarPart(CarPartKind::WheelLr);
    }
    if upper.starts_with("WHEEL_RR") || upper.starts_with("HUB_RR") {
        return NodeRole::CarPart(CarPartKind::WheelRr);
    }
    if upper.starts_with("STEER_") {
        return NodeRole::CarPart(CarPartKind::Steer);
    }
    if upper.starts_with("COCKPIT_") {
        return NodeRole::CarPart(CarPartKind::Cockpit);
    }

    // Track physics surface meshes begin with one or more digits followed by an
    // alphabetic surface key matching `surfaces.ini` (e.g. `1ROAD_main`, `2KERB_01`).
    let digit_end = name.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digit_end > 0 {
        if let Ok(prefix_num) = name[..digit_end].parse::<u32>() {
            let rest = &name[digit_end..];
            let key_end = rest
                .bytes()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            if key_end >= 2 {
                let surface_key = rest[..key_end].to_ascii_uppercase();
                return NodeRole::SurfaceMesh {
                    prefix_num,
                    surface_key,
                };
            }
        }
    }

    NodeRole::VisualMesh
}

fn classify_helper(upper: &str, original: &str) -> HelperKind {
    if let Some(rest) = upper.strip_prefix("AC_PIT_") {
        if let Ok(n) = rest.parse::<u32>() {
            return HelperKind::Pit(n);
        }
    }
    if let Some(rest) = upper.strip_prefix("AC_START_") {
        if let Ok(n) = rest.parse::<u32>() {
            return HelperKind::Start(n);
        }
    }
    if let Some(rest) = upper.strip_prefix("AC_HOTLAP_START_") {
        if let Ok(n) = rest.parse::<u32>() {
            return HelperKind::HotlapStart(n);
        }
    }
    if let Some(rest) = upper.strip_prefix("AC_TIME_") {
        if let Some((num, side)) = rest.rsplit_once('_') {
            if let (Ok(index), Some(ch)) = (num.parse::<u32>(), side.chars().next()) {
                return HelperKind::TimingGate { index, side: ch };
            }
        }
    }
    if let Some(rest) = upper.strip_prefix("AC_AB_START_") {
        if let Some(ch) = rest.chars().next() {
            return HelperKind::AbStart { side: ch };
        }
    }
    if let Some(rest) = upper.strip_prefix("AC_CREW_") {
        if let Ok(n) = rest.parse::<u32>() {
            return HelperKind::Crew(n);
        }
    }
    HelperKind::Other(original.to_string())
}

/// A helper dummy node extracted from the KN5 hierarchy.
#[derive(Debug, Clone)]
pub struct SceneHelper {
    pub name: String,
    pub kind: HelperKind,
    pub transform: Mat4,
    pub position: [f32; 3],
}

/// One mesh node from a KN5 file.
#[derive(Debug, Clone)]
pub struct SceneMesh {
    pub name: String,
    pub role: NodeRole,
    pub transform: Mat4,
    pub material_id: i32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// V is flipped (`1.0 - v`) to match standard UV / OBJ convention.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
}

/// Complete parsed KN5 scene.
#[derive(Debug, Clone)]
pub struct Kn5Scene {
    pub version: u32,
    pub textures: Vec<SceneTexture>,
    pub materials: Vec<SceneMaterial>,
    pub meshes: Vec<SceneMesh>,
    pub helpers: Vec<SceneHelper>,
}

/// Multi-KN5 track layout loaded via `models.ini` or `models_<layout>.ini`.
#[derive(Debug, Clone)]
pub struct TrackScene {
    pub track_dir: PathBuf,
    pub layout: Option<String>,
    pub models: Vec<(String, Kn5Scene)>,
}

impl TrackScene {
    /// All helper dummies across every loaded KN5 in the layout.
    pub fn all_helpers(&self) -> impl Iterator<Item = &SceneHelper> {
        self.models.iter().flat_map(|(_, s)| s.helpers.iter())
    }
}

/// A car LOD loaded via `data/lods.ini` with optional skin `.dds` texture overrides.
#[derive(Debug, Clone)]
pub struct CarScene {
    pub car_dir: PathBuf,
    pub lod_file: String,
    pub skin: Option<String>,
    pub scene: Kn5Scene,
    /// Lowercase texture name -> override raw DDS/PNG bytes from `skins/<skin>/`.
    pub skin_overrides: HashMap<String, Vec<u8>>,
}

impl CarScene {
    /// Returns texture bytes for `name`, preferring any skin override over the embedded KN5 texture.
    pub fn texture_bytes(&self, name: &str) -> Option<&[u8]> {
        let key = name.to_ascii_lowercase();
        if let Some(bytes) = self.skin_overrides.get(&key) {
            return Some(bytes.as_slice());
        }
        // Also match by file stem in case a skin provides `.png` for an embedded `.dds` (or vice versa).
        if let Some(stem) = Path::new(&key).file_stem().and_then(|s| s.to_str()) {
            for (ov_name, bytes) in &self.skin_overrides {
                if Path::new(ov_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s == stem)
                {
                    return Some(bytes.as_slice());
                }
            }
        }
        self.scene
            .textures
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .map(|t| t.bytes.as_slice())
    }
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = if chunk.len() > 1 { u32::from(chunk[1]) } else { 0 };
        let b2 = if chunk.len() > 2 { u32::from(chunk[2]) } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn decode_to_dynamic_image(bytes: &[u8]) -> Option<image::DynamicImage> {
    if bytes.is_empty() {
        return None;
    }
    if bytes.len() >= 4 && &bytes[..4] == b"DDS " {
        let rgba = crate::dds::decode(bytes).ok()?;
        let buf = image::RgbaImage::from_raw(rgba.width, rgba.height, rgba.pixels)?;
        Some(image::DynamicImage::ImageRgba8(buf))
    } else {
        image::load_from_memory(bytes).ok()
    }
}

fn encode_dynamic_image_data_url(
    resized: &image::DynamicImage,
    keep_alpha: bool,
) -> Option<String> {
    let has_translucent_alpha = if keep_alpha {
        let rgba = resized.to_rgba8();
        rgba.pixels().any(|p| p.0[3] < 250)
    } else {
        false
    };

    let mut encoded = Vec::new();
    let mime = if has_translucent_alpha {
        let mut cursor = io::Cursor::new(&mut encoded);
        resized.write_to(&mut cursor, image::ImageFormat::Png).ok()?;
        "image/png"
    } else {
        let rgb = resized.to_rgb8();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 86);
        enc.encode_image(&rgb).ok()?;
        "image/jpeg"
    };

    Some(format!("data:{mime};base64,{}", encode_base64(&encoded)))
}

/// Decodes raw DDS/PNG/JPEG texture bytes, optionally downscales to `max_side`,
/// and returns a browser-ready `data:image/...;base64,...` URL.
pub fn encode_texture_data_url(bytes: &[u8], max_side: u32, keep_alpha: bool) -> Option<String> {
    let dyn_img = decode_to_dynamic_image(bytes)?;
    let resized = if dyn_img.width() > max_side || dyn_img.height() > max_side {
        dyn_img.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        dyn_img
    };
    encode_dynamic_image_data_url(&resized, keep_alpha)
}

fn composite_car_paint_rgba(
    diff_rgba: &mut image::RgbaImage,
    det_rgba: &image::RgbaImage,
    detail_uv_multiplier: f32,
    keep_alpha: bool,
) -> bool {
    let (w, h) = diff_rgba.dimensions();
    let (dw, dh) = det_rgba.dimensions();
    if w == 0 || h == 0 || dw == 0 || dh == 0 {
        return false;
    }

    let mult = if detail_uv_multiplier.is_finite() && detail_uv_multiplier > 0.01 {
        detail_uv_multiplier
    } else {
        1.0
    };
    let w_f = w as f32;
    let h_f = h as f32;
    let dw_f = dw as f32;
    let dh_f = dh as f32;

    for (x, y, px) in diff_rgba.enumerate_pixels_mut() {
        let u = (x as f32 + 0.5) / w_f;
        let v = (y as f32 + 0.5) / h_f;
        let dx = (((u * mult).rem_euclid(1.0)) * dw_f) as u32 % dw;
        let dy = (((v * mult).rem_euclid(1.0)) * dh_f) as u32 % dh;
        let det = det_rgba.get_pixel(dx, dy);

        // diffuse.a == 0 -> full detail tint (e.g. carpaint body); diffuse.a == 255 -> keep diffuse decal.
        let alpha_mask = f32::from(px.0[3]) / 255.0;
        let one_minus_a = 1.0 - alpha_mask;
        for c in 0..3 {
            let det_factor = (f32::from(det.0[c]) / 255.0) * one_minus_a + alpha_mask;
            px.0[c] = (f32::from(px.0[c]) * det_factor).round().clamp(0.0, 255.0) as u8;
        }
        if !keep_alpha {
            px.0[3] = 255;
        }
    }
    true
}

/// Composites Assetto Corsa `ksPerPixelMultiMap*` car-paint textures where
/// `txDiffuse` (often `skin_00.dds`) holds AO/decals and its alpha channel
/// masks `txDetail` (often `metal_detail.dds` tiled by `detail_uv_multiplier`):
/// `rgb = diffuse.rgb * lerp(detail.rgb, 1.0, diffuse.a)`.
pub fn encode_composited_car_paint_data_url(
    diffuse_bytes: &[u8],
    detail_bytes: &[u8],
    detail_uv_multiplier: f32,
    max_side: u32,
    keep_alpha: bool,
) -> Option<String> {
    let diffuse_dyn = decode_to_dynamic_image(diffuse_bytes)?;
    let detail_dyn = decode_to_dynamic_image(detail_bytes)?;

    let resized_diffuse = if diffuse_dyn.width() > max_side || diffuse_dyn.height() > max_side {
        diffuse_dyn.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        diffuse_dyn
    };

    let mut diff_rgba = resized_diffuse.to_rgba8();
    let det_rgba = detail_dyn.to_rgba8();
    if !composite_car_paint_rgba(&mut diff_rgba, &det_rgba, detail_uv_multiplier, keep_alpha) {
        return None;
    }

    encode_dynamic_image_data_url(&image::DynamicImage::ImageRgba8(diff_rgba), keep_alpha)
}

// ---------------------------------------------------------------------------
// Bounds-checked binary reader
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Stop {
    Eof(u64),
    Unsupported(String),
    Io(io::Error),
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            Stop::Eof(0)
        } else {
            Stop::Io(e)
        }
    }
}

type Step<T> = Result<T, Stop>;

fn fatal(stop: Stop) -> Kn5Error {
    match stop {
        Stop::Eof(at) => Kn5Error::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("KN5 ends early at byte {at}"),
        )),
        Stop::Unsupported(what) => Kn5Error::Unsupported(what),
        Stop::Io(e) => Kn5Error::Io(e),
    }
}

struct Reader<R> {
    inner: BufReader<R>,
    pos: u64,
    size: u64,
}

impl<R: Read + Seek> Reader<R> {
    fn new(inner: R, size: u64) -> Self {
        Self {
            inner: BufReader::new(inner),
            pos: 0,
            size,
        }
    }

    fn ensure(&self, n: u64) -> Step<()> {
        if n > self.size.saturating_sub(self.pos) {
            Err(Stop::Eof(self.pos))
        } else {
            Ok(())
        }
    }

    fn array<const N: usize>(&mut self) -> Step<[u8; N]> {
        self.ensure(N as u64)?;
        let mut buf = [0u8; N];
        self.inner.read_exact(&mut buf)?;
        self.pos += N as u64;
        Ok(buf)
    }

    fn u8(&mut self) -> Step<u8> {
        Ok(self.array::<1>()?[0])
    }

    fn i32(&mut self) -> Step<i32> {
        Ok(i32::from_le_bytes(self.array::<4>()?))
    }

    fn u32(&mut self) -> Step<u32> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    fn u16(&mut self) -> Step<u16> {
        Ok(u16::from_le_bytes(self.array::<2>()?))
    }

    fn f32(&mut self) -> Step<f32> {
        Ok(f32::from_le_bytes(self.array::<4>()?))
    }

    fn count(&mut self) -> Step<usize> {
        let at = self.pos;
        let n = self.i32()?;
        usize::try_from(n)
            .map_err(|_| Stop::Unsupported(format!("negative count {n} at byte {at}")))
    }

    fn count_of(&mut self, unit: u64) -> Step<usize> {
        let n = self.count()?;
        self.ensure((n as u64).saturating_mul(unit))?;
        Ok(n)
    }

    fn skip(&mut self, n: u64) -> Step<()> {
        self.ensure(n)?;
        self.inner.seek(SeekFrom::Current(n as i64))?;
        self.pos += n;
        Ok(())
    }

    fn bytes(&mut self, n: usize) -> Step<Vec<u8>> {
        self.ensure(n as u64)?;
        let mut buf = vec![0u8; n];
        self.inner.read_exact(&mut buf)?;
        self.pos += n as u64;
        Ok(buf)
    }

    fn string(&mut self) -> Step<String> {
        let at = self.pos;
        let n = self.i32()?;
        let n = usize::try_from(n)
            .map_err(|_| Stop::Unsupported(format!("negative string length at byte {at}")))?;
        if n > MAX_STRING_BYTES {
            return Err(Stop::Unsupported(format!(
                "string of {n} bytes at byte {at}"
            )));
        }
        let buf = self.bytes(n)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn has_csp_envelope(&mut self) -> io::Result<bool> {
        let tail = CSP_MAGIC.len() as u64 + 8;
        if self.size < tail {
            return Ok(false);
        }
        self.inner.seek(SeekFrom::Start(self.size - tail))?;
        let mut marker = [0u8; 30];
        self.inner.read_exact(&mut marker)?;
        self.inner.seek(SeekFrom::Start(self.pos))?;
        Ok(&marker == CSP_MAGIC)
    }
}

/// Load a `.kn5` file from disk into a [`Kn5Scene`].
pub fn load_kn5(path: &Path, opts: &LoadOptions) -> Result<Kn5Scene, Kn5Error> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    load_kn5_reader(file, size, opts)
}

/// Load a `.kn5` from any seekable reader.
pub fn load_kn5_reader<R: Read + Seek>(
    source: R,
    size: u64,
    opts: &LoadOptions,
) -> Result<Kn5Scene, Kn5Error> {
    let mut r = Reader::new(source, size);
    if r.has_csp_envelope()? {
        return Err(Kn5Error::Encrypted);
    }
    match r.array::<6>() {
        Ok(magic) if &magic == MAGIC => {}
        Ok(_) | Err(Stop::Eof(_)) => return Err(Kn5Error::NotKn5),
        Err(other) => return Err(fatal(other)),
    }
    let version = r.u32().map_err(fatal)?;
    if version <= 4 {
        return Err(Kn5Error::Unsupported(format!(
            "KN5 version {version} is not supported"
        )));
    }
    if version > 5 {
        r.u32().map_err(fatal)?;
    }
    read_body(&mut r, version, opts).map_err(fatal)
}

fn read_body<R: Read + Seek>(
    r: &mut Reader<R>,
    version: u32,
    opts: &LoadOptions,
) -> Step<Kn5Scene> {
    let texture_count = r.count()?;
    let mut textures = Vec::with_capacity(texture_count.min(4096));
    for _ in 0..texture_count {
        let flag = r.i32()?;
        let name = r.string()?;
        let size = u64::from(r.u32()?);
        let bytes = if opts.load_textures {
            if size > MAX_TEXTURE_BYTES {
                return Err(Stop::Unsupported(format!(
                    "texture '{name}' size {size} exceeds cap {MAX_TEXTURE_BYTES}"
                )));
            }
            r.bytes(size as usize)?
        } else {
            r.skip(size)?;
            Vec::new()
        };
        textures.push(SceneTexture {
            name,
            active: flag != 0,
            bytes,
        });
    }

    let material_count = r.count()?;
    let mut materials = Vec::with_capacity(material_count.min(4096));
    for _ in 0..material_count {
        materials.push(read_material(r)?);
    }

    let mut walk = Walk {
        max_vertices: opts.max_vertices,
        max_triangles: opts.max_triangles,
        skip_ac_helpers: opts.skip_ac_helpers,
        ..Walk::default()
    };
    let (root_world, root_children) = read_node(r, &IDENTITY, &mut walk)?;
    let mut pending: Vec<(Mat4, usize)> = vec![(root_world, root_children)];
    while let Some(frame) = pending.last_mut() {
        if frame.1 == 0 {
            pending.pop();
            continue;
        }
        frame.1 -= 1;
        let parent = frame.0;
        let (world, children) = read_node(r, &parent, &mut walk)?;
        if children > 0 {
            pending.push((world, children));
        }
    }

    Ok(Kn5Scene {
        version,
        textures,
        materials,
        meshes: walk.meshes,
        helpers: walk.helpers,
    })
}

fn read_material<R: Read + Seek>(r: &mut Reader<R>) -> Step<SceneMaterial> {
    let mut m = SceneMaterial {
        name: r.string()?,
        shader: r.string()?,
        ..SceneMaterial::default()
    };
    m.blend_mode = r.u8()?;
    m.alpha_tested = r.u8()? != 0;
    m.depth_mode = r.i32()?;

    let property_count = r.count()?;
    for _ in 0..property_count {
        let name = r.string()?;
        let value = r.f32()?;
        r.skip(36)?;
        match name.as_str() {
            "ksAmbient" => m.ks_ambient = value,
            "ksDiffuse" => m.ks_diffuse = value,
            "ksSpecular" => m.ks_specular = value,
            "ksSpecularEXP" => m.ks_specular_exp = value,
            "diffuseMult" => m.diffuse_mult = value,
            "useDetail" => m.use_detail = value,
            "detailUVMultiplier" => m.detail_uv_multiplier = value,
            _ => {}
        }
    }

    let sampler_count = r.count()?;
    for _ in 0..sampler_count {
        let sample = r.string()?;
        let _slot = r.i32()?;
        let texture = r.string()?;
        match sample.as_str() {
            "txDiffuse" => m.tx_diffuse = texture,
            "txNormal" => m.tx_normal = texture,
            "txDetail" => m.tx_detail = texture,
            "txDetailA" => m.tx_detail_a = texture,
            _ => {}
        }
    }
    Ok(m)
}

#[derive(Default)]
struct Walk {
    meshes: Vec<SceneMesh>,
    helpers: Vec<SceneHelper>,
    nodes: usize,
    vertices: usize,
    triangles: usize,
    max_vertices: usize,
    max_triangles: usize,
    skip_ac_helpers: bool,
}

impl Walk {
    fn add_geometry(&mut self, vertices: usize, indices: usize) -> Step<()> {
        self.vertices = self.vertices.saturating_add(vertices);
        self.triangles = self.triangles.saturating_add(indices / 3);
        if self.vertices > self.max_vertices {
            return Err(Stop::Unsupported(format!(
                "more than {} vertices",
                self.max_vertices
            )));
        }
        if self.triangles > self.max_triangles {
            return Err(Stop::Unsupported(format!(
                "more than {} triangles",
                self.max_triangles
            )));
        }
        Ok(())
    }
}

struct Corners {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
}

fn read_vertices<R: Read + Seek>(r: &mut Reader<R>, count: usize, tail: u64) -> Step<Corners> {
    let mut c = Corners {
        positions: Vec::with_capacity(count),
        normals: Vec::with_capacity(count),
        uvs: Vec::with_capacity(count),
    };
    for _ in 0..count {
        let p = [r.f32()?, r.f32()?, r.f32()?];
        let n = [r.f32()?, r.f32()?, r.f32()?];
        let u = r.f32()?;
        let v = r.f32()?;
        r.skip(tail)?;
        if !p
            .iter()
            .chain(n.iter())
            .chain([u, v].iter())
            .all(|x| x.is_finite())
        {
            return Err(Stop::Unsupported("non-finite vertex value".to_string()));
        }
        c.positions.push(p);
        c.normals.push(n);
        c.uvs.push([u, 1.0 - v]);
    }
    Ok(c)
}

fn read_indices<R: Read + Seek>(r: &mut Reader<R>, name: &str, vertex_count: usize) -> Step<Vec<u16>> {
    let count = r.count_of(2)?;
    let mut indices = Vec::with_capacity(count);
    for _ in 0..count {
        let i = r.u16()?;
        if usize::from(i) >= vertex_count {
            return Err(Stop::Unsupported(format!(
                "mesh '{name}' has index {i} but only {vertex_count} vertices"
            )));
        }
        indices.push(i);
    }
    Ok(indices)
}

fn read_node<R: Read + Seek>(
    r: &mut Reader<R>,
    parent: &Mat4,
    walk: &mut Walk,
) -> Step<(Mat4, usize)> {
    walk.nodes += 1;
    if walk.nodes > MAX_NODES {
        return Err(Stop::Unsupported(format!("more than {MAX_NODES} nodes")));
    }

    let class_at = r.pos;
    let class = r.i32()?;
    let name = r.string()?;
    let children = r.count()?;
    r.skip(1)?; // active flag

    match class {
        1 => {
            let mut local = [[0.0f64; 4]; 4];
            for row in local.iter_mut() {
                for cell in row.iter_mut() {
                    *cell = f64::from(r.f32()?);
                }
            }
            let world = mat4_mul(&local, parent);
            if !walk.skip_ac_helpers {
                if let NodeRole::HelperDummy(kind) = classify_node_name(&name) {
                    walk.helpers.push(SceneHelper {
                        name,
                        kind,
                        transform: world,
                        position: mat4_translation(&world),
                    });
                }
            }
            Ok((world, children))
        }
        2 => {
            r.skip(3)?;
            let vertex_count = r.count_of(44)?;
            let corners = read_vertices(r, vertex_count, 12)?;
            let indices = read_indices(r, &name, vertex_count)?;
            let material_id = r.i32()?;
            r.skip(29)?;
            walk.add_geometry(vertex_count, indices.len())?;
            let role = classify_node_name(&name);
            walk.meshes.push(SceneMesh {
                name,
                role,
                transform: *parent,
                material_id,
                positions: corners.positions,
                normals: corners.normals,
                uvs: corners.uvs,
                indices,
            });
            Ok((*parent, children))
        }
        3 => {
            r.skip(3)?;
            let bone_count = r.count_of(68)?;
            for _ in 0..bone_count {
                let _bone = r.string()?;
                r.skip(64)?;
            }
            let vertex_count = r.count_of(76)?;
            let corners = read_vertices(r, vertex_count, 44)?;
            let indices = read_indices(r, &name, vertex_count)?;
            let material_id = r.i32()?;
            r.skip(12)?;
            walk.add_geometry(vertex_count, indices.len())?;
            let role = classify_node_name(&name);
            walk.meshes.push(SceneMesh {
                name,
                role,
                transform: *parent,
                material_id,
                positions: corners.positions,
                normals: corners.normals,
                uvs: corners.uvs,
                indices,
            });
            Ok((*parent, children))
        }
        other => Err(Stop::Unsupported(format!(
            "unknown node class {other} at byte {class_at}"
        ))),
    }
}

/// Parses `models.ini` (or `models_<layout>.ini`) and loads every referenced `.kn5`.
pub fn load_track_layout(
    track_dir: &Path,
    layout: Option<&str>,
    opts: &LoadOptions,
) -> Result<TrackScene, Kn5Error> {
    let ini_name = match layout.filter(|s| !s.trim().is_empty()) {
        Some(l) => format!("models_{}.ini", l.trim()),
        None => "models.ini".to_string(),
    };
    let ini_path = track_dir.join(&ini_name);
    let mut kn5_entries: Vec<(String, [f64; 3])> = Vec::new();
    if ini_path.is_file() {
        let ini = Ini::load(&ini_path)?;
        for idx in 0u32.. {
            let section = format!("MODEL_{idx}");
            let Some(file) = ini.get(&section, "FILE").map(str::trim).filter(|s| !s.is_empty())
            else {
                break;
            };
            let pos = ini
                .get(&section, "POSITION")
                .and_then(parse_vec3)
                .unwrap_or([0.0, 0.0, 0.0]);
            kn5_entries.push((file.to_string(), pos));
        }
    } else {
        // Fallback: single `<track_id>.kn5` in the track folder.
        if let Some(stem) = track_dir.file_name().and_then(|s| s.to_str()) {
            let fallback = format!("{stem}.kn5");
            if track_dir.join(&fallback).is_file() {
                kn5_entries.push((fallback, [0.0, 0.0, 0.0]));
            }
        }
    }

    let mut models = Vec::with_capacity(kn5_entries.len());
    for (file, offset) in kn5_entries {
        let path = track_dir.join(&file);
        if !path.is_file() {
            continue;
        }
        let mut scene = load_kn5(&path, opts)?;
        if offset != [0.0, 0.0, 0.0] {
            let shift = translation_mat4(offset);
            for m in &mut scene.meshes {
                m.transform = mat4_mul(&m.transform, &shift);
            }
            for h in &mut scene.helpers {
                h.transform = mat4_mul(&h.transform, &shift);
                h.position = mat4_translation(&h.transform);
            }
        }
        models.push((file, scene));
    }

    Ok(TrackScene {
        track_dir: track_dir.to_path_buf(),
        layout: layout.map(str::to_string),
        models,
    })
}

/// Loads a car's `.kn5` using `data/lods.ini` (preferring `lod_idx`, falling back to `LOD_0`
/// or the first `.kn5` in the car directory) and collects `skins/<skin>/*.dds` overrides.
pub fn load_car_lod(
    car_dir: &Path,
    skin: Option<&str>,
    lod_idx: usize,
    opts: &LoadOptions,
) -> Result<CarScene, Kn5Error> {
    let mut lod_file: Option<String> = None;
    if let Ok(ini) = cardata::load_data_ini(car_dir, "lods.ini") {
        // Try requested LOD, then walk down to LOD_0.
        for idx in (0..=lod_idx).rev() {
            let section = format!("LOD_{idx}");
            if let Some(f) = ini.get(&section, "FILE").map(str::trim).filter(|s| !s.is_empty()) {
                if car_dir.join(f).is_file() {
                    lod_file = Some(f.to_string());
                    break;
                }
            }
        }
    }
    let lod_file = match lod_file {
        Some(f) => f,
        None => {
            // Fallback: first `.kn5` in `car_dir`.
            let mut found = None;
            if let Ok(entries) = fs::read_dir(car_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("kn5"))
                    {
                        found = p.file_name().map(|n| n.to_string_lossy().into_owned());
                        break;
                    }
                }
            }
            found.ok_or_else(|| {
                Kn5Error::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no KN5 found in {}", car_dir.display()),
                ))
            })?
        }
    };

    let scene = load_kn5(&car_dir.join(&lod_file), opts)?;
    let mut skin_overrides = HashMap::new();
    if let Some(skin_id) = skin.filter(|s| !s.trim().is_empty()) {
        let skin_dir = car_dir.join("skins").join(skin_id);
        if let Ok(entries) = fs::read_dir(&skin_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let is_tex = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    e.eq_ignore_ascii_case("dds") || e.eq_ignore_ascii_case("png")
                });
                if !is_tex {
                    continue;
                }
                if let (Some(fname), Ok(bytes)) = (
                    path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()),
                    fs::read(&path),
                ) {
                    skin_overrides.insert(fname, bytes);
                }
            }
        }
    }

    Ok(CarScene {
        car_dir: car_dir.to_path_buf(),
        lod_file,
        skin: skin.map(str::to_string),
        scene,
        skin_overrides,
    })
}

fn parse_vec3(raw: &str) -> Option<[f64; 3]> {
    let parts: Vec<f64> = raw
        .split(',')
        .map(|s| s.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() == 3 && parts.iter().all(|v| v.is_finite()) {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

fn translation_mat4(t: [f64; 3]) -> Mat4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kn5_writer::{write_kn5_with_textures, Kn5Mesh, Kn5Texture, Kn5Vertex};

    fn sample_mesh(name: &str, texture: &str) -> Kn5Mesh {
        Kn5Mesh {
            name: name.to_string(),
            texture: texture.to_string(),
            vertices: vec![
                Kn5Vertex { position: [0.0, 0.0, 0.0], normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0] },
                Kn5Vertex { position: [1.0, 0.0, 0.0], normal: [0.0, 1.0, 0.0], uv: [1.0, 0.0] },
                Kn5Vertex { position: [0.0, 0.0, 1.0], normal: [0.0, 1.0, 0.0], uv: [0.0, 1.0] },
            ],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn classifies_surface_helpers_and_car_parts() {
        assert_eq!(
            classify_node_name("1ROAD_main_01"),
            NodeRole::SurfaceMesh {
                prefix_num: 1,
                surface_key: "ROAD".to_string()
            }
        );
        assert_eq!(
            classify_node_name("2KERB_turn3"),
            NodeRole::SurfaceMesh {
                prefix_num: 2,
                surface_key: "KERB".to_string()
            }
        );
        assert_eq!(
            classify_node_name("AC_PIT_0"),
            NodeRole::HelperDummy(HelperKind::Pit(0))
        );
        assert_eq!(
            classify_node_name("AC_START_12"),
            NodeRole::HelperDummy(HelperKind::Start(12))
        );
        assert_eq!(
            classify_node_name("AC_TIME_0_L"),
            NodeRole::HelperDummy(HelperKind::TimingGate { index: 0, side: 'L' })
        );
        assert_eq!(
            classify_node_name("WHEEL_LF"),
            NodeRole::CarPart(CarPartKind::WheelLf)
        );
        assert_eq!(classify_node_name("grandstand_roof"), NodeRole::VisualMesh);
    }

    #[test]
    fn loads_kn5_with_embedded_textures_and_track_and_car_loaders() {
        let dir = std::env::temp_dir().join(format!("acxs_kn5_scene_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("data")).unwrap();
        fs::create_dir_all(dir.join("skins").join("red")).unwrap();

        let fake_png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        let fake_dds = b"DDS override_bytes".to_vec();
        let kn5_path = dir.join("body.kn5");
        write_kn5_with_textures(
            &kn5_path,
            &[
                sample_mesh("1ROAD_main", "road.png"),
                sample_mesh("WHEEL_LF", "wheel.dds"),
            ],
            &[
                Kn5Texture { name: "road.png".into(), data: fake_png.clone() },
                Kn5Texture { name: "wheel.dds".into(), data: b"DDS original".to_vec() },
            ],
        )
        .unwrap();

        let scene = load_kn5(&kn5_path, &LoadOptions::default()).unwrap();
        assert_eq!(scene.textures.len(), 2);
        assert_eq!(scene.textures[0].bytes, fake_png);
        assert_eq!(scene.meshes.len(), 2);
        assert!(matches!(scene.meshes[0].role, NodeRole::SurfaceMesh { .. }));
        assert_eq!(scene.meshes[1].role, NodeRole::CarPart(CarPartKind::WheelLf));

        // Track layout loader via models.ini
        fs::write(
            dir.join("models.ini"),
            "[MODEL_0]\r\nFILE=body.kn5\r\nPOSITION=10,2,3\r\n",
        )
        .unwrap();
        let track = load_track_layout(&dir, None, &LoadOptions::default()).unwrap();
        assert_eq!(track.models.len(), 1);
        assert!((track.models[0].1.meshes[0].transform[3][0] - 10.0).abs() < 1e-6);

        // Car LOD + skin override loader
        fs::write(
            dir.join("data").join("lods.ini"),
            "[LOD_0]\r\nFILE=body.kn5\r\n",
        )
        .unwrap();
        fs::write(dir.join("skins").join("red").join("wheel.dds"), &fake_dds).unwrap();
        let car = load_car_lod(&dir, Some("red"), 0, &LoadOptions::default()).unwrap();
        assert_eq!(car.texture_bytes("wheel.dds"), Some(fake_dds.as_slice()));
        assert_eq!(car.texture_bytes("road.png"), Some(fake_png.as_slice()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn composites_detail_paint_over_diffuse_alpha_mask() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

        let encode_png = |img: &RgbaImage| -> Vec<u8> {
            let mut out = Vec::new();
            DynamicImage::ImageRgba8(img.clone())
                .write_to(&mut io::Cursor::new(&mut out), ImageFormat::Png)
                .unwrap();
            out
        };

        // Left pixel has alpha=0 (tinted by detail); right pixel has alpha=255 (keeps decal).
        let mut diff = RgbaImage::new(2, 1);
        diff.put_pixel(0, 0, Rgba([200, 200, 200, 0]));
        diff.put_pixel(1, 0, Rgba([240, 240, 240, 255]));

        let mut det = RgbaImage::new(1, 1);
        det.put_pixel(0, 0, Rgba([255, 102, 0, 64]));

        let url = encode_composited_car_paint_data_url(
            &encode_png(&diff),
            &encode_png(&det),
            35.0,
            256,
            false,
        )
        .expect("composited image");
        assert!(url.starts_with("data:image/jpeg;base64,") || url.starts_with("data:image/png;base64,"));

        // Verify production pixel compositor directly as well as end-to-end PNG/JPEG data URL.
        assert!(composite_car_paint_rgba(&mut diff, &det, 35.0, false));
        assert_eq!(diff.get_pixel(0, 0).0, [200, 80, 0, 255]);
        assert_eq!(diff.get_pixel(1, 0).0, [240, 240, 240, 255]);
    }
}
