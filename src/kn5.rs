//! Read-only inspection of Assetto Corsa `.kn5` model files.
//!
//! Only the header, the texture table, the material table and the node tree
//! are read. Texture payloads and mesh vertex/index data are skipped with
//! seeks, so a large model is never decompressed or loaded into memory.
//!
//! File layout, all little-endian:
//! - Header: the signature `sc6969`, a `u32` version, an extra `u32` word from
//!   version 6 on, then an `i32` texture count.
//! - Texture table: per entry an `i32` active flag, a length-prefixed name, a
//!   `u32` payload size and the payload.
//! - Material table: per material a name, a shader name, a blend-mode byte, an
//!   alpha-tested byte and an `i32` depth mode, then a list of float properties
//!   and a list of texture mappings.
//! - Node tree: depth-first. Each node has an `i32` class (1 = dummy with a 4x4
//!   matrix, 2 = mesh, 3 = skinned mesh), a name, an `i32` child count and an
//!   active byte, followed by a class-specific payload.
//! - The CSP envelope is a 30-byte marker at the end of the file, followed by a
//!   `u32` sidecar offset and a `u32` encryption version.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// File signature at offset 0, followed by a little-endian `u32` version.
const MAGIC: &[u8; 6] = b"sc6969";

/// Tail marker of a CSP-encrypted KN5. The file ends with this 30-byte marker,
/// then a `u32` sidecar offset, then a `u32` encryption version.
const CSP_MAGIC: &[u8; 30] = b"__AC_SHADERS_PATCH_KN5ENC_v1__";

/// Summary of a KN5 file, read without decompressing textures.
#[derive(Debug, Clone, PartialEq)]
pub struct Kn5Info {
    /// Format version from the header (AC models use 5 or 6).
    pub version: u32,
    /// Every entry of the texture table, in file order.
    pub texture_names: Vec<String>,
    /// Names of all nodes, depth-first in file order, root first.
    pub node_names: Vec<String>,
    /// Number of nodes read; equals `node_names.len()`.
    pub node_count: usize,
    /// Size of the file on disk in bytes.
    pub file_size: u64,
    /// Reasons the walk stopped early (truncation or a feature that cannot be
    /// parsed). Empty when the whole file was read.
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub enum Kn5Error {
    /// The file could not be read, or ended inside the header.
    Io(io::Error),
    /// The file does not start with the `sc6969` signature.
    NotKn5,
    /// The file carries the CSP encryption envelope and cannot be read as KN5.
    Encrypted,
    /// The file uses a structure this reader does not support.
    Unsupported(String),
}

impl fmt::Display for Kn5Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kn5Error::Io(e) => write!(f, "I/O error: {e}"),
            Kn5Error::NotKn5 => write!(f, "not a KN5 file (missing sc6969 header)"),
            Kn5Error::Encrypted => write!(
                f,
                "KN5 file is CSP-encrypted (AC shaders patch envelope) and cannot be inspected without decryption"
            ),
            Kn5Error::Unsupported(what) => write!(f, "unsupported KN5 data: {what}"),
        }
    }
}

impl Error for Kn5Error {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Kn5Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Kn5Error {
    fn from(e: io::Error) -> Self {
        Kn5Error::Io(e)
    }
}

/// Reads the header, texture names and node names of a KN5 file.
///
/// Textures and mesh payloads are skipped, never loaded. If the file ends
/// early or contains a structure this reader does not handle, the result is
/// `Ok` with whatever was read so far and an explanation in `notes`.
pub fn inspect(path: &Path) -> Result<Kn5Info, Kn5Error> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    inspect_reader(file, size)
}

/// Why a read step stopped. `Eof` covers any truncated input.
#[derive(Debug)]
enum Stop {
    Eof,
    Unsupported(String),
    Io(io::Error),
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            Stop::Eof
        } else {
            Stop::Io(e)
        }
    }
}

type Step<T> = Result<T, Stop>;

/// Bounds-checked little-endian reader that tracks its own position.
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
            Err(Stop::Eof)
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

    /// Reads an `i32` element count. Negative counts are malformed.
    fn count(&mut self) -> Step<usize> {
        let at = self.pos;
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| Stop::Unsupported(format!("negative count {n} at byte {at}")))
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
        self.ensure(n as u64)?;
        let mut buf = vec![0u8; n];
        self.inner.read_exact(&mut buf)?;
        self.pos += n as u64;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn skip_string(&mut self) -> Step<()> {
        let at = self.pos;
        let n = self.i32()?;
        let n = usize::try_from(n)
            .map_err(|_| Stop::Unsupported(format!("negative string length at byte {at}")))?;
        self.skip(n as u64)
    }

    /// True when the file ends with the CSP envelope marker. Restores the
    /// read position afterwards.
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

fn inspect_reader<R: Read + Seek>(source: R, size: u64) -> Result<Kn5Info, Kn5Error> {
    let mut r = Reader::new(source, size);

    if r.has_csp_envelope()? {
        return Err(Kn5Error::Encrypted);
    }

    match r.array::<6>() {
        Ok(magic) if &magic == MAGIC => {}
        Ok(_) | Err(Stop::Eof) => return Err(Kn5Error::NotKn5),
        Err(other) => return Err(fatal(other)),
    }

    let version = r.u32().map_err(fatal)?;
    if version > 5 {
        // Extra header word, present from version 6 on. Its value is not used.
        r.u32().map_err(fatal)?;
    }
    let texture_count = r.count().map_err(fatal)?;

    let mut info = Kn5Info {
        version,
        texture_names: Vec::new(),
        node_names: Vec::new(),
        node_count: 0,
        file_size: size,
        notes: Vec::new(),
    };

    match walk_body(&mut r, &mut info, texture_count) {
        Ok(()) => {}
        Err(Stop::Eof) => info.notes.push(format!(
            "file ends early at byte {}; texture, material or node data is incomplete",
            r.pos
        )),
        Err(Stop::Unsupported(note)) => info.notes.push(note),
        Err(Stop::Io(e)) => return Err(Kn5Error::Io(e)),
    }

    info.node_count = info.node_names.len();
    Ok(info)
}

/// Maps a failure in the fixed header to an error. Truncation there is an I/O error.
fn fatal(stop: Stop) -> Kn5Error {
    match stop {
        Stop::Eof => Kn5Error::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "KN5 header is truncated",
        )),
        Stop::Unsupported(what) => Kn5Error::Unsupported(what),
        Stop::Io(e) => Kn5Error::Io(e),
    }
}

/// Reads the texture table, the material table and the node tree in file order.
fn walk_body<R: Read + Seek>(r: &mut Reader<R>, info: &mut Kn5Info, texture_count: usize) -> Step<()> {
    for _ in 0..texture_count {
        // The name and payload size are read for every entry, active or not.
        let _active = r.i32()?;
        let name = r.string()?;
        let length = r.u32()? as u64;
        info.texture_names.push(name);
        r.skip(length)?;
    }

    if info.version <= 4 {
        return Err(Stop::Unsupported(format!(
            "KN5 version {} uses a material layout this reader does not parse; node tree not read",
            info.version
        )));
    }

    let material_count = r.count()?;
    for _ in 0..material_count {
        skip_material(r)?;
    }

    // Depth-first walk with an explicit stack, so a deep tree cannot overflow the call stack.
    let (root_name, root_children) = read_node_header(r)?;
    info.node_names.push(root_name);
    let mut pending = vec![root_children];
    while !pending.is_empty() {
        let top = pending.len() - 1;
        if pending[top] == 0 {
            pending.pop();
            continue;
        }
        pending[top] -= 1;
        let (name, children) = read_node_header(r)?;
        info.node_names.push(name);
        pending.push(children);
    }
    Ok(())
}

fn skip_material<R: Read + Seek>(r: &mut Reader<R>) -> Step<()> {
    r.skip_string()?; // name
    r.skip_string()?; // shader name
    r.skip(6)?; // blend mode (1 byte), alpha tested (1 byte), depth mode (i32)

    let properties = r.count()?;
    for _ in 0..properties {
        r.skip_string()?; // property name
        r.skip(40)?; // float + vec2 + vec3 + vec4
    }

    let mappings = r.count()?;
    for _ in 0..mappings {
        r.skip_string()?; // sample name
        r.skip(4)?; // slot
        r.skip_string()?; // texture name
    }
    Ok(())
}

/// Reads one node's header and class-specific payload, leaving the reader at
/// its first child. Returns the node name and its child count.
fn read_node_header<R: Read + Seek>(r: &mut Reader<R>) -> Step<(String, usize)> {
    let class_at = r.pos;
    let class = r.i32()?;
    let name = r.string()?;
    let children = r.count()?;
    r.u8()?; // active flag

    match class {
        // Base (dummy): 4x4 transform, 16 floats.
        1 => r.skip(64)?,
        // Mesh: flags, vertices (44 bytes each), u16 indices, material, layer,
        // LOD in/out, bounding sphere (centre + radius), renderable flag.
        2 => {
            r.skip(3)?; // cast shadows, visible, transparent
            let vertices = r.u32()? as u64;
            r.skip(vertices.saturating_mul(44))?;
            let indices = r.u32()? as u64;
            r.skip(indices.saturating_mul(2))?;
            r.skip(4 + 4 + 4 + 4 + 12 + 4 + 1)?;
        }
        // Skinned mesh: flags, bones (name + 4x4 matrix each), vertices (76
        // bytes each), u16 indices, material, layer, LOD in/out.
        3 => {
            r.skip(3)?;
            let bones = r.u32()?;
            for _ in 0..bones {
                r.skip_string()?;
                r.skip(64)?;
            }
            let vertices = r.u32()? as u64;
            r.skip(vertices.saturating_mul(76))?;
            let indices = r.u32()? as u64;
            r.skip(indices.saturating_mul(2))?;
            r.skip(4 + 4 + 4 + 4)?;
        }
        other => {
            return Err(Stop::Unsupported(format!(
                "unknown node class {other} at byte {class_at}; node tree stops here"
            )));
        }
    }
    Ok((name, children as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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

    fn put_identity(v: &mut Vec<u8>) {
        for i in 0..16 {
            put_f32(v, if i % 5 == 0 { 1.0 } else { 0.0 });
        }
    }

    /// Magic, version, optional extra word, and a texture table with two
    /// entries and one material. Stops before the node tree.
    fn header(version: u32) -> Vec<u8> {
        let mut v = MAGIC.to_vec();
        put_u32(&mut v, version);
        if version > 5 {
            put_u32(&mut v, 673425);
        }
        put_i32(&mut v, 2);
        // texture 1: active, 4 bytes of payload
        put_i32(&mut v, 1);
        put_str(&mut v, "body_dif.dds");
        put_u32(&mut v, 4);
        v.extend_from_slice(b"DDS ");
        // texture 2: active, 3 bytes of payload
        put_i32(&mut v, 1);
        put_str(&mut v, "body_nm.dds");
        put_u32(&mut v, 3);
        v.extend_from_slice(b"abc");
        // one material with one property and one texture mapping
        put_i32(&mut v, 1);
        put_str(&mut v, "body");
        put_str(&mut v, "ksPBR");
        v.extend_from_slice(&[0, 0]);
        put_i32(&mut v, 0); // depth mode
        put_i32(&mut v, 1);
        put_str(&mut v, "ksAmbient");
        put_f32(&mut v, 0.5);
        v.extend_from_slice(&[0u8; 36]);
        put_i32(&mut v, 1);
        put_str(&mut v, "txDiffuse");
        put_i32(&mut v, 0);
        put_str(&mut v, "body_dif.dds");
        v
    }

    fn base_node(v: &mut Vec<u8>, name: &str, children: i32) {
        put_i32(v, 1);
        put_str(v, name);
        put_i32(v, children);
        v.push(1);
        put_identity(v);
    }

    fn mesh_node(v: &mut Vec<u8>, name: &str, children: i32) {
        put_i32(v, 2);
        put_str(v, name);
        put_i32(v, children);
        v.push(1);
        v.extend_from_slice(&[1, 1, 0]); // cast shadows, visible, transparent
        put_u32(v, 1); // one vertex
        v.extend_from_slice(&[0u8; 44]);
        put_u32(v, 3); // three indices
        for i in [0u16, 1, 2] {
            v.extend_from_slice(&i.to_le_bytes());
        }
        put_u32(v, 0); // material id
        put_u32(v, 0); // layer
        put_f32(v, 0.0); // lod in
        put_f32(v, 100.0); // lod out
        v.extend_from_slice(&[0u8; 12]); // bounding sphere centre
        put_f32(v, 1.5); // bounding sphere radius
        v.push(1); // renderable
    }

    fn skinned_node(v: &mut Vec<u8>, name: &str, children: i32) {
        put_i32(v, 3);
        put_str(v, name);
        put_i32(v, children);
        v.push(1);
        v.extend_from_slice(&[1, 1, 1]);
        put_u32(v, 1); // one bone
        put_str(v, "bone_arm");
        put_identity(v);
        put_u32(v, 1); // one vertex
        v.extend_from_slice(&[0u8; 76]);
        put_u32(v, 3);
        for i in [0u16, 0, 0] {
            v.extend_from_slice(&i.to_le_bytes());
        }
        put_u32(v, 0); // material id
        put_u32(v, 0); // layer
        put_f32(v, 0.0);
        put_f32(v, 100.0);
    }

    fn sample(version: u32) -> Vec<u8> {
        let mut v = header(version);
        base_node(&mut v, "root", 2);
        mesh_node(&mut v, "body", 0);
        base_node(&mut v, "wheel", 0);
        v
    }

    fn inspect_bytes(bytes: &[u8]) -> Result<Kn5Info, Kn5Error> {
        inspect_reader(Cursor::new(bytes), bytes.len() as u64)
    }

    #[test]
    fn parses_v6_header_textures_and_node_tree() {
        let bytes = sample(6);
        let info = inspect_bytes(&bytes).expect("valid v6 file");
        assert_eq!(info.version, 6);
        assert_eq!(info.texture_names, ["body_dif.dds", "body_nm.dds"]);
        assert_eq!(info.node_names, ["root", "body", "wheel"]);
        assert_eq!(info.node_count, 3);
        assert_eq!(info.file_size, bytes.len() as u64);
        assert!(info.notes.is_empty());
    }

    #[test]
    fn v5_has_no_extra_header_word() {
        let info = inspect_bytes(&sample(5)).expect("valid v5 file");
        assert_eq!(info.version, 5);
        assert_eq!(info.texture_names, ["body_dif.dds", "body_nm.dds"]);
        assert_eq!(info.node_names, ["root", "body", "wheel"]);
        assert!(info.notes.is_empty());
    }

    #[test]
    fn skinned_mesh_and_nested_children() {
        let mut v = header(6);
        base_node(&mut v, "root", 1);
        base_node(&mut v, "arm", 1);
        skinned_node(&mut v, "skin", 0);
        let info = inspect_bytes(&v).expect("valid nested file");
        assert_eq!(info.node_names, ["root", "arm", "skin"]);
        assert_eq!(info.node_count, 3);
        assert!(info.notes.is_empty());
    }

    #[test]
    fn garbage_bytes_are_not_kn5() {
        assert!(matches!(
            inspect_bytes(b"this is definitely not a kn5 file"),
            Err(Kn5Error::NotKn5)
        ));
        assert!(matches!(inspect_bytes(b""), Err(Kn5Error::NotKn5)));
        assert!(matches!(inspect_bytes(b"sc69"), Err(Kn5Error::NotKn5)));
    }

    #[test]
    fn csp_envelope_is_reported_as_encrypted() {
        let mut v = sample(6);
        let body_len = v.len() as u32;
        v.extend_from_slice(CSP_MAGIC);
        put_u32(&mut v, body_len);
        put_u32(&mut v, 1);
        assert!(matches!(inspect_bytes(&v), Err(Kn5Error::Encrypted)));
    }

    #[test]
    fn truncated_file_does_not_panic() {
        let full = sample(6);
        for n in 0..full.len() {
            let prefix = &full[..n];
            let result = inspect_reader(Cursor::new(prefix), n as u64);
            if n < 18 {
                // magic + version + extra word + texture count is the fixed header
                assert!(result.is_err(), "prefix of {n} bytes should fail");
            }
            match result {
                Err(_) => {}
                Ok(info) => assert!(!info.notes.is_empty(), "prefix of {n} bytes parsed silently"),
            }
        }
    }

    #[test]
    fn unknown_node_class_stops_with_note() {
        let mut v = header(6);
        base_node(&mut v, "root", 1);
        put_i32(&mut v, 9); // not a valid node class
        put_str(&mut v, "mystery");
        put_i32(&mut v, 0);
        v.push(1);
        let info = inspect_bytes(&v).expect("header is fine, body stops");
        assert_eq!(info.node_names, ["root"]);
        assert_eq!(info.notes.len(), 1);
        assert!(info.notes[0].contains("unknown node class 9"));
    }

    #[test]
    fn version_four_reads_textures_but_not_nodes() {
        let mut v = header(4);
        base_node(&mut v, "root", 0);
        let info = inspect_bytes(&v).expect("header is fine");
        assert_eq!(info.version, 4);
        assert_eq!(info.texture_names.len(), 2);
        assert!(info.node_names.is_empty());
        assert!(info.notes[0].contains("version 4"));
    }

    #[test]
    fn inspect_reads_a_file_on_disk_and_missing_file_is_io() {
        let path = std::env::temp_dir().join(format!("acxs_kn5_test_{}.kn5", std::process::id()));
        let bytes = sample(6);
        std::fs::write(&path, &bytes).expect("write temp kn5");
        let info = inspect(&path);
        std::fs::remove_file(&path).ok();
        let info = info.expect("valid file on disk");
        assert_eq!(info.node_names, ["root", "body", "wheel"]);
        assert_eq!(info.file_size, bytes.len() as u64);

        let missing = std::env::temp_dir().join("acxs_kn5_definitely_missing.kn5");
        assert!(matches!(inspect(&missing), Err(Kn5Error::Io(_))));
    }
}
