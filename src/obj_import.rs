//! Reads Wavefront OBJ text into [`Kn5Mesh`] values, ready for
//! [`crate::kn5_writer::write_kn5_with_textures`].
//!
//! Handled statements: `v`, `vt`, `vn`, `f`, `o`, `g` and `usemtl`. Everything
//! else, including comments and `mtllib`, is ignored. A face corner may be
//! `v`, `v/vt`, `v//vn` or `v/vt/vn`. Negative indices count back from the most
//! recently defined element. Faces with more than three corners are fan
//! triangulated, which is exact for convex faces.
//!
//! Each distinct pair of group name (the last `o` or `g` seen) and texture name
//! (from the last `usemtl`) becomes one mesh, in order of first use. A texture
//! name comes from the caller's map. A material missing from the map is used as
//! its own texture name. A corner without `vt` gets the UV `(0, 0)`. A corner
//! without `vn` gets the flat normal of its face, and such corners are never
//! shared between faces. Meshes built from `v`-only faces therefore hold more
//! vertices, and reach [`MAX_VERTICES`] sooner.
//!
//! The input is read as UTF-8, with invalid bytes replaced.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use crate::kn5_writer::{Kn5Mesh, Kn5Vertex, MAX_NAME_BYTES, MAX_VERTICES};

/// Largest OBJ file [`read_obj`] will load, in bytes.
pub const MAX_OBJ_BYTES: u64 = 128 * 1024 * 1024;
/// Most entries in each of the position, normal and texture-coordinate lists.
pub const MAX_POOL_ENTRIES: usize = 4_000_000;
/// Most meshes one file may produce.
pub const MAX_MESHES: usize = 1_024;
/// Mesh name used for faces that come before any `o` or `g` statement.
const DEFAULT_NAME: &str = "default";

/// Why an OBJ file could not be read.
#[derive(Debug)]
pub enum ObjError {
    /// Reading the file failed.
    Io(io::Error),
    /// The file is larger than [`MAX_OBJ_BYTES`].
    FileTooLarge { bytes: u64, limit: u64 },
    /// A statement could not be parsed.
    Syntax { line: usize, message: String },
    /// A face refers to a position, texture coordinate or normal that is not defined.
    IndexOutOfRange { line: usize, index: i64, available: usize },
    /// The data is over a limit of the KN5 format or of this reader.
    Limit { what: &'static str, limit: usize },
    /// The text has no faces, so there is no geometry to import.
    NoFaces,
}

impl fmt::Display for ObjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjError::Io(e) => write!(f, "I/O error reading OBJ: {e}"),
            ObjError::FileTooLarge { bytes, limit } => {
                write!(f, "OBJ file is {bytes} bytes, the limit is {limit}")
            }
            ObjError::Syntax { line, message } => write!(f, "OBJ line {line}: {message}"),
            ObjError::IndexOutOfRange {
                line,
                index,
                available,
            } => write!(
                f,
                "OBJ line {line}: index {index} is outside the {available} defined entries"
            ),
            ObjError::Limit { what, limit } => write!(f, "{what} is over the limit of {limit}"),
            ObjError::NoFaces => write!(f, "OBJ data contains no faces"),
        }
    }
}

impl Error for ObjError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ObjError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ObjError {
    fn from(e: io::Error) -> Self {
        ObjError::Io(e)
    }
}

/// Reads and parses the OBJ file at `path`. See [`parse_obj`] for the rules.
///
/// The file size is checked against [`MAX_OBJ_BYTES`] before and while reading.
pub fn read_obj(
    path: &Path,
    textures: &HashMap<String, String>,
) -> Result<Vec<Kn5Mesh>, ObjError> {
    let size = fs::metadata(path)?.len();
    if size > MAX_OBJ_BYTES {
        return Err(ObjError::FileTooLarge {
            bytes: size,
            limit: MAX_OBJ_BYTES,
        });
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_OBJ_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_OBJ_BYTES {
        return Err(ObjError::FileTooLarge {
            bytes: bytes.len() as u64,
            limit: MAX_OBJ_BYTES,
        });
    }
    parse_obj(&String::from_utf8_lossy(&bytes), textures)
}

/// Parses OBJ text into one [`Kn5Mesh`] per group and texture pair.
///
/// `textures` maps `usemtl` names to texture names. Meshes come out in order of
/// first use, and each mesh's vertices are shared between its faces wherever
/// the same corner appears again. Fails with [`ObjError::NoFaces`] if there are
/// no faces.
pub fn parse_obj(
    source: &str,
    textures: &HashMap<String, String>,
) -> Result<Vec<Kn5Mesh>, ObjError> {
    let mut p = Parser {
        textures,
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        parts: Vec::new(),
        part_of: HashMap::new(),
        group: DEFAULT_NAME.to_string(),
        texture: String::new(),
        faces: 0,
    };

    for (n, raw) in source.lines().enumerate() {
        let line = n + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let (keyword, rest) = content.split_once(char::is_whitespace).unwrap_or((content, ""));
        let rest = rest.trim();
        match keyword {
            "v" => {
                let c = numbers(line, rest, 3, 4)?;
                push_pool(&mut p.positions, [c[0], c[1], c[2]], "the position count")?;
            }
            "vn" => {
                let c = numbers(line, rest, 3, 3)?;
                push_pool(&mut p.normals, [c[0], c[1], c[2]], "the normal count")?;
            }
            "vt" => {
                let c = numbers(line, rest, 2, 3)?;
                push_pool(&mut p.uvs, [c[0], c[1]], "the texture coordinate count")?;
            }
            "f" => p.face(line, rest)?,
            "o" | "g" => {
                p.group = if rest.is_empty() {
                    DEFAULT_NAME.to_string()
                } else {
                    rest.to_string()
                };
            }
            "usemtl" => p.use_material(line, rest)?,
            _ => {}
        }
    }

    if p.faces == 0 {
        return Err(ObjError::NoFaces);
    }
    Ok(p.parts
        .into_iter()
        .map(|part| Kn5Mesh {
            name: part.name,
            vertices: part.vertices,
            indices: part.indices,
            texture: part.texture,
        })
        .collect())
}

/// One face corner after its indices are resolved to zero-based positions.
/// A corner without a normal also records its face, so that it gets that
/// face's flat normal and no other corner shares its vertex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Corner {
    position: usize,
    uv: Option<usize>,
    normal: Option<usize>,
    face: Option<usize>,
}

/// A mesh under construction.
struct Part {
    name: String,
    texture: String,
    vertices: Vec<Kn5Vertex>,
    indices: Vec<u32>,
    /// Vertex index for each corner already emitted into this part.
    slots: HashMap<Corner, u32>,
}

struct Parser<'a> {
    textures: &'a HashMap<String, String>,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    parts: Vec<Part>,
    part_of: HashMap<(String, String), usize>,
    group: String,
    texture: String,
    faces: usize,
}

impl Parser<'_> {
    fn use_material(&mut self, line: usize, name: &str) -> Result<(), ObjError> {
        if name.is_empty() {
            return Err(syntax(line, "usemtl needs a material name"));
        }
        self.texture = self
            .textures
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string());
        check_name("the length of a texture name in bytes", &self.texture)
    }

    fn face(&mut self, line: usize, rest: &str) -> Result<(), ObjError> {
        let face = self.faces;
        self.faces += 1;

        let mut corners = Vec::new();
        for token in rest.split_whitespace() {
            corners.push(self.corner(line, token, face)?);
        }
        if corners.len() < 3 {
            return Err(syntax(
                line,
                format!("a face needs at least three vertices, found {}", corners.len()),
            ));
        }

        let part = self.part_for()?;
        let flat = if corners.iter().any(|c| c.normal.is_none()) {
            let points: Vec<[f32; 3]> = corners
                .iter()
                .map(|c| self.positions[c.position])
                .collect();
            polygon_normal(&points)
        } else {
            [0.0; 3]
        };

        for i in 1..corners.len() - 1 {
            for corner in [corners[0], corners[i], corners[i + 1]] {
                let index = self.vertex(part, corner, flat)?;
                self.parts[part].indices.push(index);
            }
        }
        Ok(())
    }

    /// Resolves one `v`, `v/vt`, `v//vn` or `v/vt/vn` token.
    fn corner(&self, line: usize, token: &str, face: usize) -> Result<Corner, ObjError> {
        let mut fields = token.split('/');
        let position = resolve(line, fields.next().unwrap_or(""), self.positions.len(), "position")?;
        let uv = match fields.next() {
            None | Some("") => None,
            Some(text) => Some(resolve(line, text, self.uvs.len(), "texture coordinate")?),
        };
        let normal = match fields.next() {
            None | Some("") => None,
            Some(text) => Some(resolve(line, text, self.normals.len(), "normal")?),
        };
        if fields.next().is_some() {
            return Err(syntax(
                line,
                format!("'{token}' has more than three '/' separated fields"),
            ));
        }
        Ok(Corner {
            position,
            uv,
            normal,
            face: normal.is_none().then_some(face),
        })
    }

    /// The index of the mesh for the current group and texture, creating it if needed.
    fn part_for(&mut self) -> Result<usize, ObjError> {
        let key = (self.group.clone(), self.texture.clone());
        if let Some(&index) = self.part_of.get(&key) {
            return Ok(index);
        }
        if self.parts.len() >= MAX_MESHES {
            return Err(ObjError::Limit {
                what: "the mesh count",
                limit: MAX_MESHES,
            });
        }
        check_name("the length of a mesh name in bytes", &self.group)?;
        self.parts.push(Part {
            name: key.0.clone(),
            texture: key.1.clone(),
            vertices: Vec::new(),
            indices: Vec::new(),
            slots: HashMap::new(),
        });
        let index = self.parts.len() - 1;
        self.part_of.insert(key, index);
        Ok(index)
    }

    /// The vertex index for `corner` in `part`, adding the vertex if it is new.
    fn vertex(&mut self, part: usize, corner: Corner, flat: [f32; 3]) -> Result<u32, ObjError> {
        if let Some(&index) = self.parts[part].slots.get(&corner) {
            return Ok(index);
        }
        let vertex = Kn5Vertex {
            position: self.positions[corner.position],
            normal: corner.normal.map_or(flat, |n| self.normals[n]),
            uv: corner.uv.map_or([0.0; 2], |t| self.uvs[t]),
        };
        let target = &mut self.parts[part];
        if target.vertices.len() >= MAX_VERTICES {
            return Err(ObjError::Limit {
                what: "the vertex count of a mesh",
                limit: MAX_VERTICES,
            });
        }
        let index = target.vertices.len() as u32;
        target.vertices.push(vertex);
        target.slots.insert(corner, index);
        Ok(index)
    }
}

fn syntax(line: usize, message: impl Into<String>) -> ObjError {
    ObjError::Syntax {
        line,
        message: message.into(),
    }
}

/// Parses between `min` and `max` finite numbers separated by whitespace.
fn numbers(line: usize, text: &str, min: usize, max: usize) -> Result<Vec<f32>, ObjError> {
    let values = text
        .split_whitespace()
        .map(|token| {
            token
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| syntax(line, format!("'{token}' is not a finite number")))
        })
        .collect::<Result<Vec<f32>, ObjError>>()?;
    if values.len() < min || values.len() > max {
        return Err(syntax(
            line,
            format!("expected {min} to {max} numbers, found {}", values.len()),
        ));
    }
    Ok(values)
}

/// Turns a one-based or negative OBJ index into a zero-based index into a list of `count` entries.
fn resolve(line: usize, text: &str, count: usize, kind: &str) -> Result<usize, ObjError> {
    let index: i64 = text
        .parse()
        .map_err(|_| syntax(line, format!("'{text}' is not a {kind} index")))?;
    let count = count as i64;
    let zero_based = if index > 0 {
        index - 1
    } else if index < 0 {
        count + index
    } else {
        -1
    };
    if zero_based < 0 || zero_based >= count {
        return Err(ObjError::IndexOutOfRange {
            line,
            index,
            available: count as usize,
        });
    }
    Ok(zero_based as usize)
}

fn push_pool<T>(pool: &mut Vec<T>, value: T, what: &'static str) -> Result<(), ObjError> {
    if pool.len() >= MAX_POOL_ENTRIES {
        return Err(ObjError::Limit {
            what,
            limit: MAX_POOL_ENTRIES,
        });
    }
    pool.push(value);
    Ok(())
}

fn check_name(what: &'static str, name: &str) -> Result<(), ObjError> {
    if name.len() > MAX_NAME_BYTES {
        return Err(ObjError::Limit {
            what,
            limit: MAX_NAME_BYTES,
        });
    }
    Ok(())
}

/// Unit normal of a polygon, from Newell's method. Zero for a degenerate polygon.
fn polygon_normal(points: &[[f32; 3]]) -> [f32; 3] {
    let mut n = [0.0f32; 3];
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 && len.is_finite() {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0; 3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kn5;
    use crate::kn5_writer::{Kn5Texture, write_kn5_with_textures};
    use std::fs;

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "acxs_obj_import_{}_{tag}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn parse(source: &str) -> Vec<Kn5Mesh> {
        parse_obj(source, &HashMap::new()).expect("parse")
    }

    fn parse_err(source: &str) -> ObjError {
        parse_obj(source, &HashMap::new()).expect_err("parse should fail")
    }

    const TRIANGLE: &str = "\
v 0 0 0
v 1 0 0
v 0 1 0
vt 0 0
vt 1 0
vt 0 1
vn 0 0 1
f 1/1/1 2/2/1 3/3/1
";

    #[test]
    fn triangle_with_all_three_indices() {
        let meshes = parse(TRIANGLE);
        assert_eq!(meshes.len(), 1);
        let m = &meshes[0];
        assert_eq!(m.name, "default");
        assert_eq!(m.texture, "");
        assert_eq!(m.indices, [0, 1, 2]);
        assert_eq!(m.vertices.len(), 3);
        assert_eq!(m.vertices[1], Kn5Vertex {
            position: [1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        });
    }

    #[test]
    fn quad_is_fan_triangulated_with_flat_normals_per_face() {
        let src = "\
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
f 1 2 3 4
f 1 3 4
";
        let m = &parse(src)[0];
        // The second face has its own corners, because its normal is per face.
        assert_eq!(m.indices, [0, 1, 2, 0, 2, 3, 4, 5, 6]);
        assert_eq!(m.vertices.len(), 7);
        // Without vn, every corner gets the flat normal of the counter-clockwise square.
        for v in &m.vertices {
            assert!((v.normal[2] - 1.0).abs() < 1e-6, "normal {:?}", v.normal);
            assert_eq!(v.uv, [0.0, 0.0]);
        }
    }

    #[test]
    fn every_face_corner_form_is_accepted() {
        let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
vt 0.25 0.5
vn 0 1 0
f 1//1 2//1 3//1
f 1/1 2/1 3/1
";
        let m = parse(src);
        assert_eq!(m[0].vertices[0].normal, [0.0, 1.0, 0.0]);
        assert_eq!(m[0].vertices[0].uv, [0.0, 0.0]);
        // Second face: uv given, normal absent, so the flat normal (perpendicular to XY) is used.
        let second = &m[0].vertices[3..];
        assert_eq!(second.len(), 3);
        assert_eq!(second[0].uv, [0.25, 0.5]);
        assert!((second[0].normal[2].abs() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn negative_indices_count_back_from_the_last_definition() {
        let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
vn 0 0 1
f -3//-1 -2//-1 -1//-1
";
        let m = &parse(src)[0];
        assert_eq!(m.indices, [0, 1, 2]);
        assert_eq!(m.vertices[1].position, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn groups_and_materials_split_meshes_in_first_use_order() {
        let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
v 1 1 0
o body
usemtl paint
f 1 2 3
usemtl glass
f 2 4 3
o wheel
usemtl tyre
f 1 2 4
g body
usemtl paint
f 2 3 1
";
        let meshes = parse(src);
        let names: Vec<(&str, &str)> = meshes
            .iter()
            .map(|m| (m.name.as_str(), m.texture.as_str()))
            .collect();
        assert_eq!(
            names,
            [("body", "paint"), ("body", "glass"), ("wheel", "tyre")]
        );
        // The last face joins the first mesh, appending its corners to the same list.
        assert_eq!(meshes[0].indices, [0, 1, 2, 3, 4, 5]);
        assert_eq!(meshes[0].vertices.len(), 6);
    }

    #[test]
    fn usemtl_map_renames_materials_and_unmapped_names_are_kept() {
        let src = "\
v 0 0 0
v 1 0 0
v 0 1 0
usemtl paint
f 1 2 3
usemtl chrome
f 3 2 1
";
        let mut map = HashMap::new();
        map.insert("paint".to_string(), "body_dif.dds".to_string());
        let meshes = parse_obj(src, &map).expect("parse");
        assert_eq!(meshes[0].texture, "body_dif.dds");
        assert_eq!(meshes[1].texture, "chrome");
    }

    #[test]
    fn comments_and_unknown_statements_are_ignored() {
        let src = "\
# exported by hand
mtllib car.mtl
s off
v 0 0 0 # origin
v 1 0 0
v 0 1 0
f 1 2 3 # one face
";
        assert_eq!(parse(src)[0].indices, [0, 1, 2]);
    }

    #[test]
    fn invalid_input_is_reported_with_its_line() {
        type Case = (&'static str, fn(&ObjError) -> bool);
        let cases: [Case; 11] = [
            ("v 1 2\nf 1 1 1", |e| matches!(e, ObjError::Syntax { line: 1, .. })),
            ("v 1 nan 3\nf 1 1 1", |e| matches!(e, ObjError::Syntax { line: 1, .. })),
            ("f 1 2 3", |e| matches!(e, ObjError::IndexOutOfRange { line: 1, index: 1, available: 0 })),
            ("v 0 0 0\nf 0 0 0", |e| matches!(e, ObjError::IndexOutOfRange { index: 0, .. })),
            ("v 0 0 0\nv 1 0 0\nf 1 2", |e| matches!(e, ObjError::Syntax { line: 3, .. })),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\nf 1/1/1/1 2 3", |e| matches!(e, ObjError::Syntax { line: 6, .. })),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/9 2 3", |e| matches!(e, ObjError::IndexOutOfRange { index: 9, available: 0, .. })),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl\nf 1 2 3", |e| matches!(e, ObjError::Syntax { line: 4, .. })),
            ("v 0 0 0", |e| matches!(e, ObjError::NoFaces)),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf -4 -3 -2", |e| matches!(e, ObjError::IndexOutOfRange { index: -4, available: 3, .. })),
            ("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 x", |e| matches!(e, ObjError::Syntax { line: 4, .. })),
        ];
        for (i, (src, check)) in cases.iter().enumerate() {
            let err = parse_err(src);
            assert!(check(&err), "case {i}: {err:?}");
        }
    }

    #[test]
    fn overlong_group_name_is_refused() {
        let src = format!("o {}\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", "x".repeat(MAX_NAME_BYTES + 1));
        assert!(matches!(parse_err(&src), ObjError::Limit { .. }));
    }

    #[test]
    fn too_many_vertices_for_one_mesh_is_refused() {
        // One face with MAX_VERTICES + 1 corners needs that many vertices in one mesh.
        let count = MAX_VERTICES + 1;
        let mut src = String::new();
        for i in 0..count {
            src.push_str(&format!("v {} 0 0\n", i));
        }
        src.push('f');
        for i in 1..=count {
            src.push_str(&format!(" {i}"));
        }
        src.push('\n');
        assert!(matches!(
            parse_err(&src),
            ObjError::Limit { limit, .. } if limit == MAX_VERTICES
        ));
    }

    #[test]
    fn read_obj_reads_a_file_and_reports_missing_and_oversized_files() {
        let dir = scratch_dir("read");
        let path = dir.join("tri.obj");
        fs::write(&path, TRIANGLE).expect("write obj");
        let meshes = read_obj(&path, &HashMap::new()).expect("read obj");
        assert_eq!(meshes[0].indices, [0, 1, 2]);

        let missing = dir.join("missing.obj");
        assert!(matches!(
            read_obj(&missing, &HashMap::new()),
            Err(ObjError::Io(_))
        ));

        // A sparse file over the cap is rejected by its size, without being read.
        let big = dir.join("big.obj");
        let file = File::create(&big).expect("create");
        file.set_len(MAX_OBJ_BYTES + 1).expect("set length");
        assert!(matches!(
            read_obj(&big, &HashMap::new()),
            Err(ObjError::FileTooLarge { .. })
        ));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn imported_meshes_round_trip_through_the_kn5_writer() {
        let dir = scratch_dir("kn5");
        let path = dir.join("car.kn5");
        let src = "\
o body
usemtl paint
v 0 0 0
v 1 0 0
v 0 1 0
vt 0 0
vt 1 0
vt 0 1
vn 0 0 1
f 1/1/1 2/2/1 3/3/1
o wheel
usemtl tyre
v 0 0 1
v 1 0 1
v 0 1 1
f 4 5 6
";
        let mut map = HashMap::new();
        map.insert("paint".to_string(), "body_dif.dds".to_string());
        map.insert("tyre".to_string(), "tyre_nm.png".to_string());
        let meshes = parse_obj(src, &map).expect("parse");

        let textures = [
            Kn5Texture {
                name: "body_dif.dds".into(),
                data: b"DDS \x00\x00\x00\x00".to_vec(),
            },
            Kn5Texture {
                name: "tyre_nm.png".into(),
                data: b"\x89PNG\r\n\x1a\n".to_vec(),
            },
        ];
        write_kn5_with_textures(&path, &meshes, &textures).expect("write kn5");

        let info = kn5::inspect(&path).expect("inspect");
        assert!(info.notes.is_empty(), "notes: {:?}", info.notes);
        assert_eq!(info.node_names, ["root", "body", "wheel"]);
        assert_eq!(info.texture_names, ["body_dif.dds", "tyre_nm.png"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn polygon_normal_follows_winding() {
        let square = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let n = polygon_normal(&square);
        assert!(n[0].abs() < 1e-6 && n[1].abs() < 1e-6 && (n[2] - 1.0).abs() < 1e-6);
        let reversed: Vec<_> = square.iter().rev().copied().collect();
        assert!((polygon_normal(&reversed)[2] + 1.0).abs() < 1e-6);
        assert_eq!(polygon_normal(&[[0.0; 3]; 3]), [0.0; 3]);
    }
}
