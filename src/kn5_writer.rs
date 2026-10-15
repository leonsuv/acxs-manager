//! Writes Assetto Corsa `.kn5` model files from plain mesh data.
//!
//! The output is a version 5 file, which [`crate::kn5::inspect`] reads back in
//! full. It holds a dummy root node at the identity transform, followed by one
//! mesh node per [`Kn5Mesh`], in order. Each mesh gets its own material, which
//! maps the `txDiffuse` sampler to the mesh's texture name.
//!
//! Each distinct texture name used by a mesh appears once in the texture table.
//! Its payload is the data supplied under that name in a [`Kn5Texture`], or
//! empty if none was supplied. Supplied textures that no mesh uses are stored
//! as well. Payloads must be DDS or PNG files. Tangents are derived from the UV
//! layout, and bounding spheres from the vertex positions.
//!
//! All input is checked before anything is created on disk. An existing file is
//! never overwritten or deleted.

use std::error::Error;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// File signature at offset 0.
const MAGIC: &[u8; 6] = b"sc6969";
/// Format version written. Version 5 has no extra header word.
const VERSION: u32 = 5;
/// Name of the dummy node that holds every mesh.
const ROOT_NAME: &str = "root";
/// Shader given to every generated material.
const SHADER: &str = "ksPerPixel";
/// Material properties written for each material: name and float value.
/// The remaining 36 bytes of each property (vec2, vec3, vec4) are zero.
const PROPERTIES: [(&str, f32); 4] = [
    ("ksAmbient", 0.6),
    ("ksDiffuse", 0.6),
    ("ksSpecular", 0.9),
    ("ksSpecularEXP", 20.0),
];
/// Level-of-detail distance written for every mesh, so it stays visible at any range.
const LOD_OUT: f32 = 1_000_000.0;
/// Longest name or texture name accepted, in bytes.
pub const MAX_NAME_BYTES: usize = 1024;
/// Most vertices one mesh can have: `u16` indices address `0..=65535`.
pub const MAX_VERTICES: usize = 65_536;
/// Leading bytes of a DDS file.
const DDS_MAGIC: &[u8] = b"DDS ";
/// Leading bytes of a PNG file.
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// One vertex of a [`Kn5Mesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kn5Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Texture coordinates, stored unchanged.
    pub uv: [f32; 2],
}

/// A triangle mesh to write as one KN5 mesh node.
#[derive(Debug, Clone, PartialEq)]
pub struct Kn5Mesh {
    /// Node name. Must be at most 1024 bytes.
    pub name: String,
    pub vertices: Vec<Kn5Vertex>,
    /// Triangle list: three vertex indices per triangle, each below `vertices.len()`.
    pub indices: Vec<u32>,
    /// Diffuse texture name, for example `body_dif.dds`. Empty for no texture.
    pub texture: String,
}

/// A texture payload to store in a KN5 file under `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Kn5Texture {
    /// Name that meshes refer to, for example `body_dif.dds`. Must be non-empty
    /// and unique, and at most 1024 bytes.
    pub name: String,
    /// The whole DDS or PNG file, stored as is.
    pub data: Vec<u8>,
}

/// Why a KN5 file could not be written.
#[derive(Debug)]
pub enum Kn5WriteError {
    /// The output path already exists. Nothing was written.
    Exists(PathBuf),
    /// The mesh data cannot be represented in the format.
    Invalid(String),
    /// Creating or writing the file failed.
    Io(io::Error),
}

impl fmt::Display for Kn5WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kn5WriteError::Exists(path) => {
                write!(f, "refusing to overwrite existing file {}", path.display())
            }
            Kn5WriteError::Invalid(what) => write!(f, "invalid KN5 mesh data: {what}"),
            Kn5WriteError::Io(e) => write!(f, "I/O error writing KN5: {e}"),
        }
    }
}

impl Error for Kn5WriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Kn5WriteError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Kn5WriteError {
    fn from(e: io::Error) -> Self {
        Kn5WriteError::Io(e)
    }
}

/// Writes `meshes` as a new KN5 file at `path`.
///
/// The file must not exist yet, and it is never overwritten or deleted. The
/// input is validated and encoded before the file is created, so an invalid
/// input leaves nothing on disk. If the write itself fails after the file was
/// created, the partial file is left in place.
pub fn write_kn5(path: &Path, meshes: &[Kn5Mesh]) -> Result<(), Kn5WriteError> {
    write_kn5_with_textures(path, meshes, &[])
}

/// Writes `meshes` and the texture payloads in `textures` as a new KN5 file at `path`.
///
/// Follows the same rules as [`write_kn5`]. Each texture is also checked
/// before anything is written: names must be non-empty and unique, and each
/// payload must start with the DDS or PNG signature.
pub fn write_kn5_with_textures(
    path: &Path,
    meshes: &[Kn5Mesh],
    textures: &[Kn5Texture],
) -> Result<(), Kn5WriteError> {
    let bytes = encode(meshes, textures)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                Kn5WriteError::Exists(path.to_path_buf())
            } else {
                Kn5WriteError::Io(e)
            }
        })?;
    file.write_all(&bytes)?;
    Ok(())
}

/// Builds the complete file image in memory.
fn encode(meshes: &[Kn5Mesh], textures: &[Kn5Texture]) -> Result<Vec<u8>, Kn5WriteError> {
    if i32::try_from(meshes.len()).is_err() {
        return Err(Kn5WriteError::Invalid(format!(
            "{} meshes is more than the format allows",
            meshes.len()
        )));
    }
    for mesh in meshes {
        check_mesh(mesh)?;
    }
    check_textures(textures)?;
    let table = texture_table(meshes, textures);

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    put_u32(&mut out, VERSION);

    // Texture table: active flag, name, payload size and payload.
    put_i32(&mut out, table.len() as i32);
    for &(name, data) in &table {
        put_i32(&mut out, 1);
        put_str(&mut out, name);
        put_u32(&mut out, data.len() as u32);
        out.extend_from_slice(data);
    }

    // Material table: one material per mesh, with the same index as the mesh.
    put_i32(&mut out, meshes.len() as i32);
    for mesh in meshes {
        put_material(&mut out, mesh);
    }

    // Node tree: the root dummy, then each mesh as its child.
    put_i32(&mut out, 1);
    put_str(&mut out, ROOT_NAME);
    put_i32(&mut out, meshes.len() as i32);
    out.push(1);
    for i in 0..16 {
        put_f32(&mut out, if i % 5 == 0 { 1.0 } else { 0.0 });
    }
    for (material_id, mesh) in meshes.iter().enumerate() {
        put_mesh(&mut out, mesh, material_id as i32);
    }
    Ok(out)
}

/// Checks one mesh against the limits of the format. Textures and names are checked too.
fn check_mesh(mesh: &Kn5Mesh) -> Result<(), Kn5WriteError> {
    check_name("mesh name", &mesh.name)?;
    check_name("texture name", &mesh.texture)?;
    let invalid = |what: String| Err(Kn5WriteError::Invalid(format!("mesh '{}': {what}", mesh.name)));

    if mesh.vertices.len() > MAX_VERTICES {
        return invalid(format!(
            "{} vertices, the limit is {MAX_VERTICES}",
            mesh.vertices.len()
        ));
    }
    if !mesh.indices.len().is_multiple_of(3) {
        return invalid(format!(
            "{} indices, which is not a multiple of three",
            mesh.indices.len()
        ));
    }
    if u32::try_from(mesh.indices.len()).is_err() {
        return invalid("too many indices".to_string());
    }
    if let Some(bad) = mesh
        .indices
        .iter()
        .find(|&&i| i as usize >= mesh.vertices.len())
    {
        return invalid(format!(
            "index {bad} is outside the {} vertices",
            mesh.vertices.len()
        ));
    }
    for (k, v) in mesh.vertices.iter().enumerate() {
        let finite = v
            .position
            .iter()
            .chain(v.normal.iter())
            .chain(v.uv.iter())
            .all(|x| x.is_finite());
        if !finite {
            return invalid(format!("vertex {k} has a value that is not finite"));
        }
    }
    Ok(())
}

fn check_name(what: &str, name: &str) -> Result<(), Kn5WriteError> {
    if name.len() > MAX_NAME_BYTES {
        return Err(Kn5WriteError::Invalid(format!(
            "{what} is {} bytes, the limit is {MAX_NAME_BYTES}",
            name.len()
        )));
    }
    Ok(())
}

/// Checks the supplied textures: names must be non-empty, unique and short
/// enough, and each payload must be a DDS or PNG file that fits the size field.
fn check_textures(textures: &[Kn5Texture]) -> Result<(), Kn5WriteError> {
    for (k, t) in textures.iter().enumerate() {
        let invalid = |what: &str| Err(Kn5WriteError::Invalid(format!("texture '{}': {what}", t.name)));
        check_name("texture name", &t.name)?;
        if t.name.is_empty() {
            return invalid("the name is empty");
        }
        if textures[..k].iter().any(|other| other.name == t.name) {
            return invalid("the name is used by an earlier texture");
        }
        if u32::try_from(t.data.len()).is_err() {
            return invalid("the payload is too large for the format");
        }
        if !(t.data.starts_with(DDS_MAGIC) || t.data.starts_with(PNG_MAGIC)) {
            return invalid("the payload is neither a DDS nor a PNG file");
        }
    }
    Ok(())
}

/// Texture table entries in file order: each name used by a mesh, in order of
/// first use, then each supplied texture that no mesh uses. A mesh texture
/// without a supplied payload gets an empty one.
fn texture_table<'a>(meshes: &'a [Kn5Mesh], textures: &'a [Kn5Texture]) -> Vec<(&'a str, &'a [u8])> {
    let mut table: Vec<(&str, &[u8])> = Vec::new();
    for mesh in meshes {
        let name = mesh.texture.as_str();
        if !name.is_empty() && !table.iter().any(|&(n, _)| n == name) {
            table.push((name, supplied_payload(textures, name)));
        }
    }
    for t in textures {
        if !table.iter().any(|&(n, _)| n == t.name) {
            table.push((t.name.as_str(), t.data.as_slice()));
        }
    }
    table
}

/// The payload supplied under `name`, or an empty slice.
fn supplied_payload<'a>(textures: &'a [Kn5Texture], name: &str) -> &'a [u8] {
    textures
        .iter()
        .find(|t| t.name == name)
        .map_or(&[][..], |t| t.data.as_slice())
}

fn put_material(out: &mut Vec<u8>, mesh: &Kn5Mesh) {
    put_str(out, &mesh.name);
    put_str(out, SHADER);
    out.extend_from_slice(&[0, 0]); // blend mode, alpha tested
    put_i32(out, 0); // depth mode

    put_i32(out, PROPERTIES.len() as i32);
    for (name, value) in PROPERTIES {
        put_str(out, name);
        put_f32(out, value);
        out.extend_from_slice(&[0u8; 36]);
    }

    if mesh.texture.is_empty() {
        put_i32(out, 0);
    } else {
        put_i32(out, 1);
        put_str(out, "txDiffuse");
        put_i32(out, 0); // slot
        put_str(out, &mesh.texture);
    }
}

fn put_mesh(out: &mut Vec<u8>, mesh: &Kn5Mesh, material_id: i32) {
    put_i32(out, 2); // class: mesh
    put_str(out, &mesh.name);
    put_i32(out, 0); // child count
    out.push(1); // active
    out.extend_from_slice(&[1, 1, 0]); // cast shadows, visible, not transparent

    // Vertices: position, normal, uv, tangent. 44 bytes each.
    let tangents = tangents(&mesh.vertices, &mesh.indices);
    put_u32(out, mesh.vertices.len() as u32);
    for (v, t) in mesh.vertices.iter().zip(&tangents) {
        put_f32s(out, &v.position);
        put_f32s(out, &v.normal);
        put_f32s(out, &v.uv);
        put_f32s(out, t);
    }

    put_u32(out, mesh.indices.len() as u32);
    for &i in &mesh.indices {
        // Checked by `check_mesh`: every index is below the vertex count, which fits u16.
        out.extend_from_slice(&(i as u16).to_le_bytes());
    }

    put_i32(out, material_id);
    put_u32(out, 0); // layer
    put_f32(out, 0.0); // lod in
    put_f32(out, LOD_OUT); // lod out
    let (centre, radius) = bounding_sphere(&mesh.vertices);
    put_f32s(out, &centre);
    put_f32(out, radius);
    out.push(1); // renderable
}

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The vector scaled to unit length, or `None` if it is (nearly) zero.
fn unit(a: V3) -> Option<V3> {
    let len = dot(a, a).sqrt();
    if len.is_finite() && len > 1e-12 {
        Some(scale(a, 1.0 / len))
    } else {
        None
    }
}

/// A unit vector perpendicular to `n`. Falls back to the X axis for a zero `n`.
fn perpendicular(n: V3) -> V3 {
    let axis = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    unit(cross(n, axis)).unwrap_or([1.0, 0.0, 0.0])
}

/// Per-vertex tangents from the UV layout. Each triangle contributes the
/// direction in which U increases. The sums are then made orthogonal to the
/// vertex normal and normalised. Vertices with no usable UV direction get a
/// direction perpendicular to their normal.
fn tangents(vertices: &[Kn5Vertex], indices: &[u32]) -> Vec<V3> {
    let mut sums: Vec<V3> = vec![[0.0; 3]; vertices.len()];
    for tri in indices.as_chunks::<3>().0 {
        let ids = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        let [a, b, c] = ids.map(|i| &vertices[i]);
        let e1 = sub(b.position, a.position);
        let e2 = sub(c.position, a.position);
        let du1 = b.uv[0] - a.uv[0];
        let dv1 = b.uv[1] - a.uv[1];
        let du2 = c.uv[0] - a.uv[0];
        let dv2 = c.uv[1] - a.uv[1];
        let det = du1 * dv2 - du2 * dv1;
        if !det.is_finite() || det.abs() < 1e-12 {
            continue;
        }
        let t = scale(sub(scale(e1, dv2), scale(e2, dv1)), 1.0 / det);
        for i in ids {
            sums[i] = add(sums[i], t);
        }
    }

    vertices
        .iter()
        .zip(sums)
        .map(|(v, t)| {
            let ortho = match unit(v.normal) {
                Some(n) => sub(t, scale(n, dot(n, t))),
                None => t,
            };
            unit(ortho).unwrap_or_else(|| perpendicular(v.normal))
        })
        .collect()
}

/// Centre and radius of a sphere that contains every vertex. The centre is the
/// centre of the bounding box.
fn bounding_sphere(vertices: &[Kn5Vertex]) -> (V3, f32) {
    let Some(first) = vertices.first() else {
        return ([0.0; 3], 0.0);
    };
    let mut lo = first.position;
    let mut hi = first.position;
    for v in vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(v.position[k]);
            hi[k] = hi[k].max(v.position[k]);
        }
    }
    let centre = scale(add(lo, hi), 0.5);
    let radius = vertices
        .iter()
        .map(|v| dot(sub(v.position, centre), sub(v.position, centre)).sqrt())
        .fold(0.0f32, f32::max);
    (centre, radius)
}

fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32s(out: &mut Vec<u8>, v: &[f32]) {
    for &x in v {
        put_f32(out, x);
    }
}

/// Writes a length-prefixed (`i32`) string. Callers have already checked the length.
fn put_str(out: &mut Vec<u8>, s: &str) {
    put_i32(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kn5;
    use crate::kn5_export;
    use std::fs;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "acxs_kn5_writer_{}_{tag}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// A unit square in the XY plane, made of two triangles, with a flat normal.
    fn quad(name: &str, texture: &str) -> Kn5Mesh {
        let n = [0.0, 0.0, 1.0];
        let vertex = |x: f32, y: f32| Kn5Vertex {
            position: [x, y, 0.0],
            normal: n,
            uv: [x, y],
        };
        Kn5Mesh {
            name: name.to_string(),
            vertices: vec![vertex(0.0, 0.0), vertex(1.0, 0.0), vertex(1.0, 1.0), vertex(0.0, 1.0)],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: texture.to_string(),
        }
    }

    #[test]
    fn round_trip_through_inspect() {
        let dir = scratch_dir("round_trip");
        let path = dir.join("model.kn5");
        let meshes = [
            quad("body", "body_dif.dds"),
            quad("wheel", "body_nm.dds"),
            quad("glass", "body_dif.dds"),
        ];
        write_kn5(&path, &meshes).expect("write kn5");

        let info = kn5::inspect(&path).expect("inspect written file");
        assert_eq!(info.version, 5);
        assert!(info.notes.is_empty(), "notes: {:?}", info.notes);
        assert_eq!(info.texture_names, ["body_dif.dds", "body_nm.dds"]);
        assert_eq!(info.node_names, ["root", "body", "wheel", "glass"]);
        assert_eq!(info.node_count, 4);
        assert_eq!(info.file_size, fs::metadata(&path).expect("stat").len());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_mesh_list_writes_only_the_root() {
        let dir = scratch_dir("empty");
        let path = dir.join("empty.kn5");
        write_kn5(&path, &[]).expect("write empty kn5");
        let info = kn5::inspect(&path).expect("inspect");
        assert_eq!(info.node_names, ["root"]);
        assert!(info.texture_names.is_empty());
        assert!(info.notes.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refuses_to_overwrite_and_keeps_the_file() {
        let dir = scratch_dir("overwrite");
        let path = dir.join("model.kn5");
        fs::write(&path, "keep me").expect("seed file");
        let err = write_kn5(&path, &[quad("body", "")]).expect_err("path exists");
        assert!(matches!(err, Kn5WriteError::Exists(ref p) if p == &path));
        assert_eq!(fs::read_to_string(&path).expect("read back"), "keep me");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_input_is_refused_and_creates_nothing() {
        let dir = scratch_dir("invalid");

        let mut out_of_range = quad("body", "");
        out_of_range.indices[5] = 4;

        let mut partial_triangle = quad("body", "");
        partial_triangle.indices.pop();

        let mut not_finite = quad("body", "");
        not_finite.vertices[2].normal[1] = f32::NAN;

        let long_name = quad(&"x".repeat(MAX_NAME_BYTES + 1), "");

        let mut too_many_vertices = quad("body", "");
        too_many_vertices.vertices = vec![too_many_vertices.vertices[0]; MAX_VERTICES + 1];

        let cases = [
            ("out_of_range", out_of_range),
            ("partial_triangle", partial_triangle),
            ("not_finite", not_finite),
            ("long_name", long_name),
            ("too_many_vertices", too_many_vertices),
        ];
        for (label, mesh) in cases {
            let path = dir.join(format!("{label}.kn5"));
            let result = write_kn5(&path, &[mesh]);
            assert!(
                matches!(&result, Err(Kn5WriteError::Invalid(_))),
                "{label}: {result:?}"
            );
            assert!(!path.exists(), "{label} created a file");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exporter_reads_the_written_meshes() {
        let dir = scratch_dir("exporter");
        let kn5_path = dir.join("model.kn5");
        let obj_path = dir.join("model.obj");
        let mtl_path = dir.join("model.mtl");
        write_kn5(
            &kn5_path,
            &[quad("body", "body_dif.dds"), quad("wheel", "")],
        )
        .expect("write kn5");

        let stats = kn5_export::export_obj(&kn5_path, &obj_path, Some(&mtl_path))
            .expect("export written file");
        assert_eq!(stats.meshes, 2);
        assert_eq!(stats.vertices, 8);
        assert_eq!(stats.triangles, 4);

        let mtl = fs::read_to_string(&mtl_path).expect("read mtl");
        assert!(mtl.contains("newmtl body"));
        assert!(mtl.contains("map_Kd texture\\body_dif.dds"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tangents_follow_the_u_direction() {
        let q = quad("q", "");
        for t in tangents(&q.vertices, &q.indices) {
            let d = sub(t, [1.0, 0.0, 0.0]);
            assert!(dot(d, d).sqrt() < 1e-5, "tangent {t:?}");
        }
    }

    #[test]
    fn bounding_sphere_contains_the_square() {
        let q = quad("q", "");
        let (centre, radius) = bounding_sphere(&q.vertices);
        assert_eq!(centre, [0.5, 0.5, 0.0]);
        assert!((radius - 0.5f32.sqrt()).abs() < 1e-6);
    }

    fn read_i32(bytes: &[u8], at: &mut usize) -> i32 {
        let v = i32::from_le_bytes(bytes[*at..*at + 4].try_into().expect("4 bytes"));
        *at += 4;
        v
    }

    /// Walks the texture table of a version 5 file and returns each entry's name and payload.
    fn texture_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut at = MAGIC.len() + 4;
        let count = read_i32(bytes, &mut at);
        let mut entries = Vec::new();
        for _ in 0..count {
            let _active = read_i32(bytes, &mut at);
            let len = read_i32(bytes, &mut at) as usize;
            let name = String::from_utf8(bytes[at..at + len].to_vec()).expect("utf-8 name");
            at += len;
            let size = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes")) as usize;
            at += 4;
            entries.push((name, bytes[at..at + size].to_vec()));
            at += size;
        }
        entries
    }

    fn dds_bytes(fill: u8) -> Vec<u8> {
        let mut v = b"DDS ".to_vec();
        v.extend(vec![fill; 60]);
        v
    }

    fn png_bytes(fill: u8) -> Vec<u8> {
        let mut v = PNG_MAGIC.to_vec();
        v.extend(vec![fill; 40]);
        v
    }

    #[test]
    fn texture_payloads_round_trip_through_inspect() {
        let dir = scratch_dir("payloads");
        let path = dir.join("model.kn5");
        let body = dds_bytes(7);
        let spare = png_bytes(9);
        let textures = [
            Kn5Texture {
                name: "body_dif.dds".to_string(),
                data: body.clone(),
            },
            Kn5Texture {
                name: "unused.png".to_string(),
                data: spare.clone(),
            },
        ];
        let meshes = [quad("body", "body_dif.dds"), quad("wheel", "wheel_dif.dds")];
        write_kn5_with_textures(&path, &meshes, &textures).expect("write kn5");

        let bytes = fs::read(&path).expect("read back");
        let info = kn5::inspect(&path).expect("inspect written file");
        assert!(info.notes.is_empty(), "notes: {:?}", info.notes);
        assert_eq!(info.texture_names, ["body_dif.dds", "wheel_dif.dds", "unused.png"]);
        assert_eq!(info.node_names, ["root", "body", "wheel"]);
        assert_eq!(info.file_size, bytes.len() as u64);

        // The payloads are stored byte for byte. The wheel texture has none.
        assert_eq!(
            texture_entries(&bytes),
            [
                ("body_dif.dds".to_string(), body),
                ("wheel_dif.dds".to_string(), Vec::new()),
                ("unused.png".to_string(), spare),
            ]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_texture_is_refused_and_creates_nothing() {
        let dir = scratch_dir("bad_texture");
        let mesh = quad("body", "body_dif.dds");
        let texture = |name: &str, data: Vec<u8>| Kn5Texture {
            name: name.to_string(),
            data,
        };
        let cases = [
            ("not_an_image", vec![texture("body_dif.dds", b"GIF89a".to_vec())]),
            ("empty_payload", vec![texture("body_dif.dds", Vec::new())]),
            ("empty_name", vec![texture("", dds_bytes(1))]),
            (
                "duplicate_name",
                vec![texture("body_dif.dds", dds_bytes(1)), texture("body_dif.dds", dds_bytes(2))],
            ),
            ("long_name", vec![texture(&"x".repeat(MAX_NAME_BYTES + 1), dds_bytes(1))]),
        ];
        for (label, supplied) in cases {
            let path = dir.join(format!("{label}.kn5"));
            let result = write_kn5_with_textures(&path, std::slice::from_ref(&mesh), &supplied);
            assert!(
                matches!(&result, Err(Kn5WriteError::Invalid(_))),
                "{label}: {result:?}"
            );
            assert!(!path.exists(), "{label} created a file");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn textures_do_not_bypass_refusing_to_overwrite() {
        let dir = scratch_dir("textures_overwrite");
        let path = dir.join("model.kn5");
        fs::write(&path, "keep me").expect("seed file");
        let textures = [Kn5Texture {
            name: "body_dif.dds".to_string(),
            data: dds_bytes(3),
        }];
        let err = write_kn5_with_textures(&path, &[quad("body", "body_dif.dds")], &textures)
            .expect_err("path exists");
        assert!(matches!(err, Kn5WriteError::Exists(ref p) if p == &path));
        assert_eq!(fs::read_to_string(&path).expect("read back"), "keep me");
        fs::remove_dir_all(&dir).ok();
    }
}
