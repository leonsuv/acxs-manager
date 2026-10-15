//! Exports the mesh geometry of an Assetto Corsa `.kn5` model to Wavefront OBJ,
//! with an optional minimal MTL.
//!
//! The byte layout matches the KN5 reader in `kn5.rs` on every offset used here.
//! Mesh vertices and node transforms are read in full, and the result is written
//! as OBJ groups, with an MTL file that describes the materials.
//!
//! Node parsing lives in this module because `kn5.rs` exposes only node names.
//! Nothing is written until the whole file has parsed, so a corrupt file leaves no
//! partial output behind. Existing output files are never overwritten or deleted.

use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::kn5::Kn5Error;

/// File signature at offset 0.
const MAGIC: &[u8; 6] = b"sc6969";

/// CSP envelope marker at the end of an encrypted KN5.
const CSP_MAGIC: &[u8; 30] = b"__AC_SHADERS_PATCH_KN5ENC_v1__";

/// Longest string accepted. Real names are far shorter.
const MAX_STRING_BYTES: usize = 64 * 1024;
/// Upper bound on nodes walked, so a corrupt tree cannot run on indefinitely.
const MAX_NODES: usize = 1_000_000;
/// Upper bound on mesh vertices kept in memory and written.
const MAX_VERTICES: usize = 8_000_000;
/// Upper bound on triangles written.
const MAX_TRIANGLES: usize = 8_000_000;

/// Counts of what was written to the OBJ file.
#[derive(Debug, Clone)]
pub struct ObjStats {
    /// Mesh nodes written as OBJ groups.
    pub meshes: usize,
    /// Vertices written (`v` lines).
    pub vertices: usize,
    /// Triangles written (`f` lines).
    pub triangles: usize,
}

/// Why an export did not complete.
#[derive(Debug)]
pub enum ExportError {
    /// The KN5 input could not be read or is not a supported KN5 file.
    Kn5(Kn5Error),
    /// The request or the file uses something this exporter does not handle.
    Unsupported(String),
    /// Reading or writing a file failed.
    Io(io::Error),
    /// The output path already exists. Nothing was written to it.
    Exists(PathBuf),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Kn5(e) => write!(f, "{e}"),
            ExportError::Unsupported(what) => write!(f, "unsupported export: {what}"),
            ExportError::Io(e) => write!(f, "I/O error: {e}"),
            ExportError::Exists(path) => {
                write!(f, "refusing to overwrite existing file {}", path.display())
            }
        }
    }
}

impl Error for ExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ExportError::Kn5(e) => Some(e),
            ExportError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Kn5Error> for ExportError {
    fn from(e: Kn5Error) -> Self {
        ExportError::Kn5(e)
    }
}

impl From<io::Error> for ExportError {
    fn from(e: io::Error) -> Self {
        ExportError::Io(e)
    }
}

/// Exports every mesh node of `kn5_path` to a new OBJ file at `obj_path`, and a
/// minimal MTL at `mtl_path` when given.
///
/// Both output paths must not exist. Mesh nodes whose names start with `AC_` are
/// skipped. Their children are still exported.
pub fn export_obj(
    kn5_path: &Path,
    obj_path: &Path,
    mtl_path: Option<&Path>,
) -> Result<ObjStats, ExportError> {
    if let Some(mtl) = mtl_path {
        if mtl == obj_path {
            return Err(ExportError::Unsupported(
                "the MTL and OBJ paths must differ".to_string(),
            ));
        }
        refuse_existing(mtl)?;
    }
    refuse_existing(obj_path)?;

    let model = load_model(kn5_path)?;

    for mesh in &model.meshes {
        if mesh.material_id >= 0 && mesh.material_id as usize >= model.materials.len() {
            return Err(ExportError::Kn5(Kn5Error::Unsupported(format!(
                "mesh '{}' uses material {} but the file has {}",
                mesh.name,
                mesh.material_id,
                model.materials.len()
            ))));
        }
    }

    let source_name = file_name_of(kn5_path);
    let mtl_name = mtl_path.map(file_name_of);

    if let Some(mtl) = mtl_path {
        let mut w = BufWriter::new(create_new(mtl)?);
        write_mtl(&mut w, &model.materials)?;
        w.flush()?;
    }
    let mut w = BufWriter::new(create_new(obj_path)?);
    let stats = write_obj(&mut w, &model, &source_name, mtl_name.as_deref())?;
    w.flush()?;
    Ok(stats)
}

/// Largest triangle count [`load_triangles`] returns. It matches the renderer's
/// own limit, so every loaded model can be drawn.
const MAX_LOAD_TRIANGLES: usize = crate::ortho::MAX_TRIANGLES;

/// Base colour given to every loaded triangle.
const LOAD_COLOR: [u8; 3] = [200, 200, 200];

/// Reads the mesh geometry of a KN5 file as world-space triangles for the
/// orthographic renderer.
///
/// Each mesh is placed by the product of its ancestor dummies. Nodes whose names
/// start with `AC_` are left out, as in [`export_obj`]. Fails when the file holds
/// more than [`crate::ortho::MAX_TRIANGLES`] triangles.
pub fn load_triangles(path: &Path) -> Result<Vec<crate::ortho::Triangle>, ExportError> {
    load_triangles_capped(path, MAX_LOAD_TRIANGLES)
}

fn load_triangles_capped(
    path: &Path,
    max: usize,
) -> Result<Vec<crate::ortho::Triangle>, ExportError> {
    let model = load_model(path)?;
    let visible = || model.meshes.iter().filter(|m| !m.name.starts_with("AC_"));

    let total: usize = visible().map(|m| m.indices.len() / 3).sum();
    if total > max {
        return Err(ExportError::Unsupported(format!(
            "{total} triangles exceeds the limit of {max}"
        )));
    }

    let mut out = Vec::with_capacity(total);
    for mesh in visible() {
        // Indices were checked against the vertex count when the mesh was read.
        let world: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .map(|p| transform(&mesh.transform, p, true))
            .collect();
        for tri in mesh.indices.as_chunks::<3>().0 {
            out.push(crate::ortho::Triangle {
                a: world[usize::from(tri[0])],
                b: world[usize::from(tri[1])],
                c: world[usize::from(tri[2])],
                color: LOAD_COLOR,
            });
        }
    }
    Ok(out)
}

/// Opens and parses a KN5 file. Nothing is written by this function.
fn load_model(path: &Path) -> Result<Model, ExportError> {
    let file = File::open(path).map_err(|e| ExportError::Kn5(Kn5Error::Io(e)))?;
    let size = file
        .metadata()
        .map_err(|e| ExportError::Kn5(Kn5Error::Io(e)))?
        .len();
    Ok(read_model(file, size)?)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn refuse_existing(path: &Path) -> Result<(), ExportError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(ExportError::Exists(path.to_path_buf())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ExportError::Io(e)),
    }
}

/// Creates a file that must not exist yet. Never truncates or replaces one.
fn create_new(path: &Path) -> Result<File, ExportError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                ExportError::Exists(path.to_path_buf())
            } else {
                ExportError::Io(e)
            }
        })
}

// ---------------------------------------------------------------------------
// Parsed model
// ---------------------------------------------------------------------------

/// Row-major 4x4 matrix. Vectors are row vectors: `v' = [x y z 1] * M`.
type Mat4 = [[f64; 4]; 4];

const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f64; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[i][k] * b[k][j]).sum::<f64>();
        }
    }
    out
}

/// Transforms a point (with translation) or a direction (without) by a node's
/// accumulated matrix.
fn transform(m: &Mat4, v: &[f32; 3], is_point: bool) -> [f32; 3] {
    let w = if is_point { 1.0 } else { 0.0 };
    let (x, y, z) = (f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let mut out = [0.0f32; 3];
    for (j, o) in out.iter_mut().enumerate() {
        *o = (x * m[0][j] + y * m[1][j] + z * m[2][j] + w * m[3][j]) as f32;
    }
    out
}

/// Material fields the exporter uses. Defaults apply when a material omits a property.
#[derive(Debug, Clone)]
struct Material {
    name: String,
    shader: String,
    ks_ambient: f32,
    ks_diffuse: f32,
    ks_specular: f32,
    ks_specular_exp: f32,
    diffuse_mult: f32,
    use_detail: f32,
    detail_uv_multiplier: f32,
    tx_diffuse: String,
    tx_normal: String,
    tx_detail: String,
    tx_detail_a: String,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: String::new(),
            shader: String::new(),
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

/// One mesh node (class 2 or 3) with the transform of its ancestors applied later.
#[derive(Debug, Clone)]
struct Mesh {
    name: String,
    /// World transform: the product of all ancestor dummies. Mesh nodes carry no
    /// transform of their own.
    transform: Mat4,
    material_id: i32,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    /// Stored as `(u, 1 - v)`, so V is flipped on read.
    uvs: Vec<[f32; 2]>,
    indices: Vec<u16>,
}

#[derive(Debug)]
struct Model {
    materials: Vec<Material>,
    meshes: Vec<Mesh>,
}

// ---------------------------------------------------------------------------
// Bounds-checked reader
// ---------------------------------------------------------------------------

/// Why a read step stopped.
#[derive(Debug)]
enum Stop {
    /// The data ends before the field is complete. Carries the byte offset.
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

/// Little-endian reader that tracks its offset and checks every read against
/// the file size before it allocates or seeks.
struct Reader<R> {
    inner: BufReader<R>,
    pos: u64,
    size: u64,
}

impl<R: Read + Seek> Reader<R> {
    fn new(inner: R, size: u64) -> Self {
        Reader {
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

    /// Reads an `i32` element count. Negative counts are malformed.
    fn count(&mut self) -> Step<usize> {
        let at = self.pos;
        let n = self.i32()?;
        usize::try_from(n)
            .map_err(|_| Stop::Unsupported(format!("negative count {n} at byte {at}")))
    }

    /// Reads a count and checks that `count * unit` bytes remain, so the caller
    /// can allocate for it safely.
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

    /// Reads a length-prefixed (`i32`) UTF-8 string.
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
        self.ensure(n as u64)?;
        let mut buf = vec![0u8; n];
        self.inner.read_exact(&mut buf)?;
        self.pos += n as u64;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// True when the file ends with the CSP envelope marker. Leaves the read
    /// position where it was.
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

// ---------------------------------------------------------------------------
// KN5 walk
// ---------------------------------------------------------------------------

/// Parses the whole file into memory. Textures are skipped with seeks, never loaded.
fn read_model<R: Read + Seek>(source: R, size: u64) -> Result<Model, Kn5Error> {
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
        // Version 4 stores a shorter material block than version 5 and later,
        // and the node tree is not parsed for it.
        return Err(Kn5Error::Unsupported(format!(
            "KN5 version {version} is not supported by the OBJ exporter"
        )));
    }
    if version > 5 {
        // Extra header word, present from version 6 on. Its value is not used.
        r.u32().map_err(fatal)?;
    }

    read_body(&mut r).map_err(fatal)
}

fn read_body<R: Read + Seek>(r: &mut Reader<R>) -> Step<Model> {
    // Texture table: flag (i32), name (length-prefixed string), payload size (u32), payload.
    let texture_count = r.count()?;
    for _ in 0..texture_count {
        let _flag = r.i32()?;
        let _name = r.string()?;
        let size = u64::from(r.u32()?);
        r.skip(size)?;
    }

    // Material table.
    let material_count = r.count()?;
    let mut materials = Vec::new();
    for _ in 0..material_count {
        materials.push(read_material(r)?);
    }

    // Node tree, depth first, in file order.
    let mut walk = Walk::default();
    let (root_world, root_children) = read_node(r, &IDENTITY, &mut walk)?;

    // Each frame is (world matrix of the owning node, children still to read).
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

    Ok(Model {
        materials,
        meshes: walk.meshes,
    })
}

fn read_material<R: Read + Seek>(r: &mut Reader<R>) -> Step<Material> {
    let mut m = Material {
        name: r.string()?,
        shader: r.string()?,
        ..Material::default()
    };
    // Blend mode (1 byte), alpha tested (1 byte), depth mode (i32).
    r.skip(6)?;

    let property_count = r.count()?;
    for _ in 0..property_count {
        let name = r.string()?;
        let value = r.f32()?;
        // The float is followed by a vec2, vec3 and vec4 (36 bytes), which are unused here.
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
    meshes: Vec<Mesh>,
    nodes: usize,
    vertices: usize,
    triangles: usize,
}

impl Walk {
    fn add_geometry(&mut self, vertices: usize, indices: usize) -> Step<()> {
        self.vertices = self.vertices.saturating_add(vertices);
        self.triangles = self.triangles.saturating_add(indices / 3);
        if self.vertices > MAX_VERTICES {
            return Err(Stop::Unsupported(format!(
                "more than {MAX_VERTICES} vertices"
            )));
        }
        if self.triangles > MAX_TRIANGLES {
            return Err(Stop::Unsupported(format!(
                "more than {MAX_TRIANGLES} triangles"
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

/// Reads `count` vertices: position (12 bytes), normal (12), uv (8), then `tail`
/// bytes the exporter does not use. The tail is 12 for a static vertex (tangent)
/// and 44 for a skinned one (tangent and weights).
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
        if !p.iter().chain(n.iter()).chain([u, v].iter()).all(|x| x.is_finite()) {
            return Err(Stop::Unsupported("non-finite vertex value".to_string()));
        }
        c.positions.push(p);
        c.normals.push(n);
        c.uvs.push([u, 1.0 - v]);
    }
    Ok(c)
}

/// Reads a u16 index list. Each index must address a vertex of the same node.
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

/// Reads one node header and its class payload, leaving the reader at the first
/// child. Returns the node's world matrix and its child count.
///
/// Header: class (i32), name (string), child count (i32), active flag (1 byte).
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
        // Base (dummy). A 4x4 float matrix, read row by row, so the translation is
        // in row 3.
        1 => {
            let mut local = [[0.0f64; 4]; 4];
            for row in local.iter_mut() {
                for cell in row.iter_mut() {
                    *cell = f64::from(r.f32()?);
                }
            }
            Ok((mul(&local, parent), children))
        }
        // Mesh. Three flag bytes, then the vertex block, u16 indices, material id,
        // and 29 bytes of layer, LOD and bounding data.
        2 => {
            r.skip(3)?;
            let vertex_count = r.count_of(44)?;
            let corners = read_vertices(r, vertex_count, 12)?;
            let indices = read_indices(r, &name, vertex_count)?;
            let material_id = r.i32()?;
            r.skip(29)?;
            walk.add_geometry(vertex_count, indices.len())?;
            walk.meshes.push(Mesh {
                name,
                transform: *parent,
                material_id,
                positions: corners.positions,
                normals: corners.normals,
                uvs: corners.uvs,
                indices,
            });
            Ok((*parent, children))
        }
        // Skinned mesh. Three flag bytes, bones (name plus a 4x4 matrix each), the
        // vertex block with 44 tail bytes, u16 indices, material id, and 12 bytes.
        // Bone matrices are read past and not applied.
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
            walk.meshes.push(Mesh {
                name,
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

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// OBJ and MTL names: whitespace and control characters become `_`, so each
/// name stays a single token.
fn obj_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

/// Shaders treated as transparent. They get `d 0.9999` and an alpha map.
fn is_transparent_shader(shader: &str) -> bool {
    shader.starts_with("ksPerPixelAT")
        || matches!(
            shader,
            "ksPerPixelAlpha" | "ksSkidMark" | "ksTree" | "ksGrass" | "ksFlags"
        )
}

/// Minimal MTL: colour terms, specular exponent, and texture maps.
fn write_mtl<W: Write>(w: &mut W, materials: &[Material]) -> io::Result<()> {
    for m in materials {
        writeln!(w, "newmtl {}", obj_name(&m.name))?;
        writeln!(w, "Ka {0} {0} {0}", m.ks_ambient)?;
        writeln!(w, "Kd {0} {0} {0}", m.ks_diffuse)?;
        writeln!(w, "Ks {0} {0} {0}", m.ks_specular)?;
        writeln!(w, "Ns {}", m.ks_specular_exp)?;
        writeln!(w, "illum 2")?;

        let transparent = is_transparent_shader(&m.shader);
        if transparent {
            writeln!(w, "d 0.9999")?;
        }
        // Texture paths use `texture\name`.
        if m.use_detail == 1.0 && !m.tx_detail.is_empty() {
            writeln!(w, "map_Kd texture\\{}", m.tx_detail)?;
            if !m.tx_diffuse.is_empty() {
                writeln!(w, "map_Ks texture\\{}", m.tx_diffuse)?;
            }
            if transparent && !m.tx_detail_a.is_empty() {
                writeln!(w, "map_d texture\\{}", m.tx_detail_a)?;
            }
        } else if !m.tx_diffuse.is_empty() {
            writeln!(w, "map_Kd texture\\{}", m.tx_diffuse)?;
            if transparent {
                writeln!(w, "map_d texture\\{}", m.tx_diffuse)?;
            }
        }
        if !m.tx_normal.is_empty() {
            writeln!(w, "bump texture\\{}", m.tx_normal)?;
        }
        writeln!(w)?;
    }
    Ok(())
}

/// Writes the OBJ body. Mesh nodes are written in file order. Vertex indices are
/// global across the file, so each group continues the numbering of the last.
fn write_obj<W: Write>(
    w: &mut W,
    model: &Model,
    source_name: &str,
    mtl_name: Option<&str>,
) -> Result<ObjStats, ExportError> {
    writeln!(w, "# Assetto Corsa model")?;
    writeln!(w, "# Exported by ACXS Manager from {source_name}")?;
    if let Some(mtl) = mtl_name {
        writeln!(w, "mtllib {mtl}")?;
    }

    let mut stats = ObjStats {
        meshes: 0,
        vertices: 0,
        triangles: 0,
    };
    // OBJ indices are 1-based and count across all groups.
    let mut first = 1usize;

    for mesh in &model.meshes {
        if mesh.name.starts_with("AC_") {
            continue;
        }
        let material = usize::try_from(mesh.material_id)
            .ok()
            .map(|i| &model.materials[i]);
        // The UV multiplier is diffuseMult, or detailUVMultiplier when useDetail is set.
        let uv_mult = match material {
            None => 1.0,
            Some(m) if m.use_detail == 0.0 => m.diffuse_mult,
            Some(m) => m.detail_uv_multiplier,
        };

        writeln!(w, "g {}", obj_name(&mesh.name))?;
        for p in &mesh.positions {
            let [x, y, z] = transform(&mesh.transform, p, true);
            writeln!(w, "v {x} {y} {z}")?;
        }
        for n in &mesh.normals {
            let [x, y, z] = transform(&mesh.transform, n, false);
            writeln!(w, "vn {x} {y} {z}")?;
        }
        for uv in &mesh.uvs {
            writeln!(w, "vt {} {}", uv[0] * uv_mult, uv[1] * uv_mult)?;
        }
        if mtl_name.is_some() {
            let name = material.map_or_else(|| "Default".to_string(), |m| obj_name(&m.name));
            writeln!(w, "usemtl {name}")?;
        }
        for tri in mesh.indices.as_chunks::<3>().0 {
            let a = first + usize::from(tri[0]);
            let b = first + usize::from(tri[1]);
            let c = first + usize::from(tri[2]);
            writeln!(w, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
        }

        first += mesh.positions.len();
        stats.meshes += 1;
        stats.vertices += mesh.positions.len();
        stats.triangles += mesh.indices.len() / 3;
    }
    Ok(stats)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn put_i32(v: &mut Vec<u8>, x: i32) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    fn put_u32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    fn put_f32(v: &mut Vec<u8>, x: f32) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    fn put_str(v: &mut Vec<u8>, s: &str) {
        put_i32(v, s.len() as i32);
        v.extend_from_slice(s.as_bytes());
    }

    /// A v6 KN5 with one texture, one material, a root dummy translated by
    /// (10, 0, 0), and one mesh child named `mesh_name` with a triangle whose
    /// indices are `indices`.
    pub(crate) fn kn5_with_triangle(mesh_name: &str, indices: [u16; 3]) -> Vec<u8> {
        let mut v = MAGIC.to_vec();
        put_u32(&mut v, 6);
        put_u32(&mut v, 673425);

        // Texture table: one entry with a 4-byte payload.
        put_i32(&mut v, 1);
        put_i32(&mut v, 1);
        put_str(&mut v, "tri_dif.dds");
        put_u32(&mut v, 4);
        v.extend_from_slice(b"DDS ");

        // Material table: one material with ksSpecularEXP = 20 and txDiffuse.
        put_i32(&mut v, 1);
        put_str(&mut v, "tri_mat");
        put_str(&mut v, "ksPerPixel");
        v.extend_from_slice(&[0, 0]);
        put_i32(&mut v, 0);
        put_i32(&mut v, 1);
        put_str(&mut v, "ksSpecularEXP");
        put_f32(&mut v, 20.0);
        v.extend_from_slice(&[0u8; 36]);
        put_i32(&mut v, 1);
        put_str(&mut v, "txDiffuse");
        put_i32(&mut v, 0);
        put_str(&mut v, "tri_dif.dds");

        // Root dummy: identity with translation in row 3 (element 12 = x).
        put_i32(&mut v, 1);
        put_str(&mut v, "root");
        put_i32(&mut v, 1);
        v.push(1);
        for i in 0..16 {
            let value = if i == 12 {
                10.0
            } else if i % 5 == 0 {
                1.0
            } else {
                0.0
            };
            put_f32(&mut v, value);
        }

        // Mesh child: class 2, flags, three vertices, indices, material, 29 bytes.
        put_i32(&mut v, 2);
        put_str(&mut v, mesh_name);
        put_i32(&mut v, 0);
        v.push(1);
        v.extend_from_slice(&[1, 1, 0]);
        put_u32(&mut v, 3);
        for (p, uv) in [
            ([0.0f32, 0.0, 0.0], [0.0f32, 0.0]),
            ([1.0, 0.0, 0.0], [1.0, 0.0]),
            ([0.0, 1.0, 0.0], [0.0, 1.0]),
        ] {
            for x in p {
                put_f32(&mut v, x);
            }
            put_f32(&mut v, 0.0);
            put_f32(&mut v, 0.0);
            put_f32(&mut v, 1.0);
            for x in uv {
                put_f32(&mut v, x);
            }
            v.extend_from_slice(&[0u8; 12]); // tangent
        }
        put_u32(&mut v, 3);
        for i in indices {
            v.extend_from_slice(&i.to_le_bytes());
        }
        put_i32(&mut v, 0); // material id
        v.extend_from_slice(&[0u8; 29]);
        v
    }

    pub(crate) fn triangle_kn5() -> Vec<u8> {
        kn5_with_triangle("tri", [0, 1, 2])
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "acxs_kn5_export_{}_{}",
            std::process::id(),
            tag
        ));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn write_kn5(dir: &Path, bytes: &[u8]) -> PathBuf {
        let path = dir.join("model.kn5");
        fs::write(&path, bytes).expect("write kn5");
        path
    }

    #[test]
    fn exports_triangle_to_obj_and_mtl() {
        let dir = scratch_dir("triangle");
        let kn5 = write_kn5(&dir, &triangle_kn5());
        let obj = dir.join("tri.obj");
        let mtl = dir.join("tri.mtl");

        let stats = export_obj(&kn5, &obj, Some(&mtl)).expect("export");
        assert_eq!(stats.meshes, 1);
        assert_eq!(stats.vertices, 3);
        assert_eq!(stats.triangles, 1);

        let text = fs::read_to_string(&obj).expect("read obj");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.iter().filter(|l| l.starts_with("v ")).count(), 3);
        assert_eq!(lines.iter().filter(|l| l.starts_with("vn ")).count(), 3);
        assert_eq!(lines.iter().filter(|l| l.starts_with("vt ")).count(), 3);
        assert_eq!(lines.iter().filter(|l| l.starts_with("f ")).count(), 1);
        assert!(lines.contains(&"mtllib tri.mtl"));
        // The root dummy's translation (10, 0, 0) is applied to every vertex.
        assert!(lines.contains(&"v 10 0 0"));
        assert!(lines.contains(&"v 11 0 0"));
        assert!(lines.contains(&"v 10 1 0"));
        assert!(lines.contains(&"vn 0 0 1"));
        // V is flipped on read (1 - v), so the stored uv (0, 0) becomes (0, 1).
        assert!(lines.contains(&"vt 0 1"));
        assert!(lines.contains(&"usemtl tri_mat"));
        assert!(lines.contains(&"f 1/1/1 2/2/2 3/3/3"));

        let mtl_text = fs::read_to_string(&mtl).expect("read mtl");
        assert!(mtl_text.contains("newmtl tri_mat"));
        assert!(mtl_text.contains("Ns 20"));
        assert!(mtl_text.contains("map_Kd texture\\tri_dif.dds"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refuses_to_overwrite_existing_outputs() {
        let dir = scratch_dir("overwrite");
        let kn5 = write_kn5(&dir, &triangle_kn5());
        let obj = dir.join("tri.obj");
        let mtl = dir.join("tri.mtl");

        fs::write(&obj, "keep me").expect("seed obj");
        let err = export_obj(&kn5, &obj, Some(&mtl)).expect_err("obj exists");
        assert!(matches!(err, ExportError::Exists(ref p) if p == &obj));
        assert_eq!(fs::read_to_string(&obj).expect("obj intact"), "keep me");
        assert!(!mtl.exists(), "no MTL is written when the OBJ is refused");

        fs::remove_file(&obj).expect("remove seed");
        fs::write(&mtl, "keep me too").expect("seed mtl");
        let err = export_obj(&kn5, &obj, Some(&mtl)).expect_err("mtl exists");
        assert!(matches!(err, ExportError::Exists(ref p) if p == &mtl));
        assert_eq!(fs::read_to_string(&mtl).expect("mtl intact"), "keep me too");
        assert!(!obj.exists(), "no OBJ is written when the MTL is refused");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn truncated_file_is_an_error_not_a_panic() {
        let dir = scratch_dir("truncated");
        let full = triangle_kn5();
        let kn5 = dir.join("model.kn5");
        let obj = dir.join("out.obj");
        for n in 0..full.len() {
            fs::write(&kn5, &full[..n]).expect("write prefix");
            let result = export_obj(&kn5, &obj, None);
            assert!(result.is_err(), "prefix of {n} bytes exported");
            assert!(!obj.exists(), "prefix of {n} bytes left an OBJ behind");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_bytes_never_panic() {
        let dir = scratch_dir("corrupt");
        let full = triangle_kn5();
        let kn5 = dir.join("model.kn5");
        let obj = dir.join("out.obj");
        let mtl = dir.join("out.mtl");
        for pos in 0..full.len() {
            for value in [0x00u8, 0x7f, 0xff] {
                let mut bytes = full.clone();
                bytes[pos] = value;
                fs::write(&kn5, &bytes).expect("write corrupt");
                // Test-owned scratch outputs only, cleared before each run.
                fs::remove_file(&obj).ok();
                fs::remove_file(&mtl).ok();
                let _ = export_obj(&kn5, &obj, Some(&mtl));
            }
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn index_outside_vertex_range_is_rejected() {
        let dir = scratch_dir("index");
        let kn5 = write_kn5(&dir, &kn5_with_triangle("tri", [0, 1, 3]));
        let obj = dir.join("out.obj");
        let err = export_obj(&kn5, &obj, None).expect_err("bad index");
        assert!(matches!(err, ExportError::Kn5(Kn5Error::Unsupported(_))));
        assert!(!obj.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ac_nodes_are_skipped() {
        let dir = scratch_dir("ac_skip");
        let kn5 = write_kn5(&dir, &kn5_with_triangle("AC_collider", [0, 1, 2]));
        let obj = dir.join("out.obj");
        let stats = export_obj(&kn5, &obj, None).expect("export");
        assert_eq!(stats.meshes, 0);
        assert_eq!(stats.vertices, 0);
        assert_eq!(stats.triangles, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn encrypted_and_non_kn5_inputs_are_reported() {
        let dir = scratch_dir("kinds");
        let obj = dir.join("out.obj");

        let mut encrypted = triangle_kn5();
        let body_len = encrypted.len() as u32;
        encrypted.extend_from_slice(CSP_MAGIC);
        put_u32(&mut encrypted, body_len);
        put_u32(&mut encrypted, 1);
        let kn5 = write_kn5(&dir, &encrypted);
        assert!(matches!(
            export_obj(&kn5, &obj, None),
            Err(ExportError::Kn5(Kn5Error::Encrypted))
        ));

        let kn5 = write_kn5(&dir, b"not a kn5 file at all");
        assert!(matches!(
            export_obj(&kn5, &obj, None),
            Err(ExportError::Kn5(Kn5Error::NotKn5))
        ));
        assert!(!obj.exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_triangles_applies_world_transform() {
        let dir = scratch_dir("load_world");
        let kn5 = write_kn5(&dir, &triangle_kn5());
        let triangles = load_triangles(&kn5).expect("load");
        assert_eq!(triangles.len(), 1);
        let t = triangles[0];
        // The root dummy translates by (10, 0, 0).
        assert_eq!(t.a, [10.0, 0.0, 0.0]);
        assert_eq!(t.b, [11.0, 0.0, 0.0]);
        assert_eq!(t.c, [10.0, 1.0, 0.0]);
        assert_eq!(t.color, LOAD_COLOR);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_triangles_skips_ac_nodes() {
        let dir = scratch_dir("load_ac");
        let kn5 = write_kn5(&dir, &kn5_with_triangle("AC_collider", [0, 1, 2]));
        assert!(load_triangles(&kn5).expect("load").is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_triangles_enforces_cap() {
        let dir = scratch_dir("load_cap");
        let kn5 = write_kn5(&dir, &triangle_kn5());
        assert!(load_triangles_capped(&kn5, 1).is_ok());
        assert!(matches!(
            load_triangles_capped(&kn5, 0),
            Err(ExportError::Unsupported(_))
        ));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_triangles_reports_bad_input() {
        let dir = scratch_dir("load_bad");
        let kn5 = write_kn5(&dir, b"not a kn5 file at all");
        assert!(matches!(
            load_triangles(&kn5),
            Err(ExportError::Kn5(Kn5Error::NotKn5))
        ));
        let missing = dir.join("missing.kn5");
        assert!(matches!(
            load_triangles(&missing),
            Err(ExportError::Kn5(Kn5Error::Io(_)))
        ));
        fs::remove_dir_all(&dir).ok();
    }
}
