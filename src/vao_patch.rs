//! Reader and writer for CSP `.vao-patch` (Vertex Ambient Occlusion) binary files.
//!
//! Layout (little-endian):
//! - Magic: `b"VAO\0"` (4 bytes)
//! - Version: `u32` (`5`)
//! - Compressed body (`flate2` zlib stream):
//!   - `mesh_count: u32`
//!   - For each mesh:
//!     - `name_len: u32`, `name: [u8; name_len]` (UTF-8)
//!     - `vertex_count: u32`
//!     - `values: [u8; vertex_count * 4]` (per-vertex `(ao_r, ao_g, ao_b, ao_a)` in `0..=255`)

use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

const MAGIC: &[u8; 4] = b"VAO\0";
pub const VAO_PATCH_VERSION: u32 = 5;
const MAX_MESHES: usize = 65_536;
const MAX_VERTICES_PER_MESH: usize = 4_000_000;
const MAX_NAME_LEN: usize = 4096;

#[derive(Debug)]
pub enum VaoError {
    NotVaoPatch,
    UnsupportedVersion(u32),
    Truncated,
    Decompress(io::Error),
    InvalidData(String),
    FileExists(PathBuf),
    Io(io::Error),
}

impl fmt::Display for VaoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaoError::NotVaoPatch => write!(f, "not a CSP .vao-patch file"),
            VaoError::UnsupportedVersion(v) => write!(f, "unsupported .vao-patch version {v}"),
            VaoError::Truncated => write!(f, ".vao-patch file is truncated"),
            VaoError::Decompress(e) => write!(f, ".vao-patch zlib error: {e}"),
            VaoError::InvalidData(msg) => write!(f, "invalid .vao-patch data: {msg}"),
            VaoError::FileExists(p) => write!(f, "{} already exists", p.display()),
            VaoError::Io(e) => write!(f, ".vao-patch I/O error: {e}"),
        }
    }
}

impl Error for VaoError {}

impl From<io::Error> for VaoError {
    fn from(e: io::Error) -> Self {
        VaoError::Io(e)
    }
}

/// Per-mesh vertex AO payload stored inside a `.vao-patch` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaoMeshPatch {
    pub mesh_name: String,
    /// Four bytes `[ao_r, ao_g, ao_b, ao_a]` per vertex (`0..=255`).
    pub vertex_rgba: Vec<[u8; 4]>,
}

impl VaoMeshPatch {
    /// Convenience constructor from scalar `0.0..=1.0` per-vertex AO values.
    pub fn from_scalar_ao(mesh_name: impl Into<String>, ao: &[f32]) -> Self {
        let vertex_rgba = ao
            .iter()
            .map(|&v| {
                let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                [b, b, b, 255]
            })
            .collect();
        Self {
            mesh_name: mesh_name.into(),
            vertex_rgba,
        }
    }
}

/// Complete `.vao-patch` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaoPatch {
    pub version: u32,
    pub meshes: Vec<VaoMeshPatch>,
}

/// Serializes `patch` to a zlib-compressed `.vao-patch` byte vector.
pub fn encode_vao_patch(patch: &VaoPatch) -> Result<Vec<u8>, VaoError> {
    let mut raw = Vec::new();
    raw.extend_from_slice(&(patch.meshes.len() as u32).to_le_bytes());
    for mesh in &patch.meshes {
        let name_bytes = mesh.mesh_name.as_bytes();
        if name_bytes.len() > MAX_NAME_LEN {
            return Err(VaoError::InvalidData("mesh name exceeds limit".into()));
        }
        raw.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        raw.extend_from_slice(name_bytes);
        raw.extend_from_slice(&(mesh.vertex_rgba.len() as u32).to_le_bytes());
        for rgba in &mesh.vertex_rgba {
            raw.extend_from_slice(rgba);
        }
    }

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&raw).map_err(VaoError::Decompress)?;
    let compressed = encoder.finish().map_err(VaoError::Decompress)?;

    let mut out = Vec::with_capacity(8 + compressed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&patch.version.to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

/// Parses a `.vao-patch` byte buffer.
pub fn decode_vao_patch(bytes: &[u8]) -> Result<VaoPatch, VaoError> {
    if bytes.len() < 8 {
        return Err(VaoError::Truncated);
    }
    if &bytes[..4] != MAGIC {
        return Err(VaoError::NotVaoPatch);
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != 4 && version != 5 {
        return Err(VaoError::UnsupportedVersion(version));
    }

    let mut decoder = ZlibDecoder::new(&bytes[8..]);
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw).map_err(VaoError::Decompress)?;

    let mut pos = 0usize;
    let take = |pos: &mut usize, n: usize| -> Result<&[u8], VaoError> {
        let end = pos.checked_add(n).ok_or(VaoError::Truncated)?;
        let slice = raw.get(*pos..end).ok_or(VaoError::Truncated)?;
        *pos = end;
        Ok(slice)
    };
    let read_u32 = |pos: &mut usize| -> Result<u32, VaoError> {
        let b = take(pos, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };

    let mesh_count = read_u32(&mut pos)? as usize;
    if mesh_count > MAX_MESHES {
        return Err(VaoError::InvalidData(format!(
            "mesh count {mesh_count} exceeds cap"
        )));
    }
    let mut meshes = Vec::with_capacity(mesh_count);
    for _ in 0..mesh_count {
        let name_len = read_u32(&mut pos)? as usize;
        if name_len > MAX_NAME_LEN {
            return Err(VaoError::InvalidData("mesh name exceeds cap".into()));
        }
        let mesh_name = String::from_utf8_lossy(take(&mut pos, name_len)?).into_owned();
        let vert_count = read_u32(&mut pos)? as usize;
        if vert_count > MAX_VERTICES_PER_MESH {
            return Err(VaoError::InvalidData("vertex count exceeds cap".into()));
        }
        let payload = take(&mut pos, vert_count.checked_mul(4).ok_or(VaoError::Truncated)?)?;
        let mut vertex_rgba = Vec::with_capacity(vert_count);
        for chunk in payload.chunks_exact(4) {
            vertex_rgba.push([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        meshes.push(VaoMeshPatch {
            mesh_name,
            vertex_rgba,
        });
    }

    Ok(VaoPatch { version, meshes })
}

/// Writes a `.vao-patch` file non-destructively (refuses to overwrite existing files).
pub fn write_vao_patch(path: &Path, patch: &VaoPatch) -> Result<usize, VaoError> {
    let encoded = encode_vao_patch(patch)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(VaoError::FileExists(path.to_path_buf()));
        }
        Err(e) => return Err(VaoError::Io(e)),
    };
    file.write_all(&encoded)?;
    Ok(encoded.len())
}

/// Reads a `.vao-patch` file from disk.
pub fn read_vao_patch(path: &Path) -> Result<VaoPatch, VaoError> {
    let bytes = fs::read(path)?;
    decode_vao_patch(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vao_patch_round_trips_through_zlib_and_refuses_overwrite() {
        let patch = VaoPatch {
            version: VAO_PATCH_VERSION,
            meshes: vec![
                VaoMeshPatch::from_scalar_ao("BODY_SUB0", &[0.0, 0.5, 1.0]),
                VaoMeshPatch::from_scalar_ao("INTERIOR", &[0.25, 0.75]),
            ],
        };
        let encoded = encode_vao_patch(&patch).unwrap();
        let decoded = decode_vao_patch(&encoded).unwrap();
        assert_eq!(decoded, patch);

        let dir = std::env::temp_dir().join(format!("acxs_vao_patch_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("car.vao-patch");
        let n = write_vao_patch(&path, &patch).unwrap();
        assert_eq!(n, encoded.len());
        assert_eq!(read_vao_patch(&path).unwrap(), patch);
        assert!(matches!(write_vao_patch(&path, &patch), Err(VaoError::FileExists(_))));
        let _ = fs::remove_dir_all(&dir);
    }
}
