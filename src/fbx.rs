//! ASCII FBX 7.4 writer for triangle meshes.
//!
//! Each [`FbxMesh`] becomes a `Geometry` object holding its positions and a
//! polygon index list, plus a `Model` object that is parented to the scene
//! root. Triangles are stored in the FBX convention where the last vertex of
//! every polygon is written as `-(index + 1)`, which tells readers where the
//! polygon ends.

use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// Number of values written on one line inside a numeric array.
const VALUES_PER_LINE: usize = 12;

/// A triangle mesh ready for export.
#[derive(Debug, Clone, PartialEq)]
pub struct FbxMesh {
    /// Scene name. Must be non-empty and free of `"` and control characters.
    pub name: String,
    /// Vertex positions.
    pub vertices: Vec<[f32; 3]>,
    /// Triangle list: three entries per triangle, each below `vertices.len()`.
    pub indices: Vec<u32>,
    /// Optional per-vertex normals. When present there must be exactly one per vertex.
    pub normals: Option<Vec<[f32; 3]>>,
}

/// Errors returned while validating or writing FBX data.
#[derive(Debug)]
pub enum FbxError {
    /// The mesh name is empty or contains a quote or control character.
    InvalidName(String),
    /// A vertex or normal has a NaN or infinite component.
    NonFinite {
        mesh: String,
        kind: &'static str,
        position: usize,
    },
    /// The index list is not a whole number of triangles.
    IndexCount { mesh: String, len: usize },
    /// An index refers past the end of the vertex list.
    IndexOutOfRange {
        mesh: String,
        index: u32,
        vertex_count: usize,
    },
    /// The normal count does not match the vertex count.
    NormalCount {
        mesh: String,
        normals: usize,
        vertices: usize,
    },
    /// The destination file already exists.
    Exists(PathBuf),
    /// An I/O failure while writing the file.
    Io(io::Error),
}

impl fmt::Display for FbxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FbxError::InvalidName(name) => write!(
                f,
                "invalid mesh name {name:?}: names must be non-empty and contain no quotes or control characters"
            ),
            FbxError::NonFinite {
                mesh,
                kind,
                position,
            } => write!(f, "mesh '{mesh}': {kind} {position} is NaN or infinite"),
            FbxError::IndexCount { mesh, len } => {
                write!(f, "mesh '{mesh}': {len} indices is not a multiple of 3")
            }
            FbxError::IndexOutOfRange {
                mesh,
                index,
                vertex_count,
            } => write!(
                f,
                "mesh '{mesh}': index {index} is out of range for {vertex_count} vertices"
            ),
            FbxError::NormalCount {
                mesh,
                normals,
                vertices,
            } => write!(
                f,
                "mesh '{mesh}': {normals} normals given for {vertices} vertices"
            ),
            FbxError::Exists(path) => {
                write!(f, "refusing to overwrite existing file {}", path.display())
            }
            FbxError::Io(e) => write!(f, "I/O error writing FBX: {e}"),
        }
    }
}

impl Error for FbxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            FbxError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for FbxError {
    fn from(e: io::Error) -> Self {
        FbxError::Io(e)
    }
}

/// Renders the meshes as an ASCII FBX 7.4 document.
pub fn to_fbx_string(meshes: &[FbxMesh]) -> Result<String, FbxError> {
    for mesh in meshes {
        validate(mesh)?;
    }
    let mut out = String::from(HEADER);
    // Writing into a String cannot fail, so the fmt results are unreachable errors.
    write_objects(&mut out, meshes).expect("formatting into a String cannot fail");
    write_connections(&mut out, meshes.len()).expect("formatting into a String cannot fail");
    Ok(out)
}

/// Writes the meshes to a new FBX file. An existing file is never replaced.
///
/// The meshes are validated before the file is created, so invalid input
/// leaves no file behind. If writing fails after the file was created, the
/// partial file is removed.
pub fn write_fbx(path: &Path, meshes: &[FbxMesh]) -> Result<(), FbxError> {
    let text = to_fbx_string(meshes)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                FbxError::Exists(path.to_path_buf())
            } else {
                FbxError::Io(e)
            }
        })?;
    if let Err(e) = file.write_all(text.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(FbxError::Io(e));
    }
    Ok(())
}

fn validate(mesh: &FbxMesh) -> Result<(), FbxError> {
    if mesh.name.is_empty() || mesh.name.chars().any(|c| c == '"' || c.is_control()) {
        return Err(FbxError::InvalidName(mesh.name.clone()));
    }
    if mesh.indices.len() % 3 != 0 {
        return Err(FbxError::IndexCount {
            mesh: mesh.name.clone(),
            len: mesh.indices.len(),
        });
    }
    let vertex_count = mesh.vertices.len();
    if let Some(&index) = mesh.indices.iter().find(|&&i| i as usize >= vertex_count) {
        return Err(FbxError::IndexOutOfRange {
            mesh: mesh.name.clone(),
            index,
            vertex_count,
        });
    }
    check_finite(&mesh.name, "vertex", &mesh.vertices)?;
    if let Some(normals) = &mesh.normals {
        if normals.len() != vertex_count {
            return Err(FbxError::NormalCount {
                mesh: mesh.name.clone(),
                normals: normals.len(),
                vertices: vertex_count,
            });
        }
        check_finite(&mesh.name, "normal", normals)?;
    }
    Ok(())
}

fn check_finite(mesh: &str, kind: &'static str, points: &[[f32; 3]]) -> Result<(), FbxError> {
    match points.iter().position(|p| !p.iter().all(|c| c.is_finite())) {
        Some(position) => Err(FbxError::NonFinite {
            mesh: mesh.to_string(),
            kind,
            position,
        }),
        None => Ok(()),
    }
}

const HEADER: &str = "; FBX 7.4.0 project file\n\
; Generated by acxs-manager\n\
; ----------------------------------------------------\n\
\n\
FBXHeaderExtension:  {\n\
\tFBXHeaderVersion: 1003\n\
\tFBXVersion: 7400\n\
\tCreator: \"acxs-manager\"\n\
}\n\
\n\
GlobalSettings:  {\n\
\tVersion: 1000\n\
\tProperties70:  {\n\
\t\tP: \"UpAxis\", \"int\", \"Integer\", \"\",1\n\
\t\tP: \"UpAxisSign\", \"int\", \"Integer\", \"\",1\n\
\t\tP: \"FrontAxis\", \"int\", \"Integer\", \"\",2\n\
\t\tP: \"FrontAxisSign\", \"int\", \"Integer\", \"\",1\n\
\t\tP: \"CoordAxis\", \"int\", \"Integer\", \"\",0\n\
\t\tP: \"CoordAxisSign\", \"int\", \"Integer\", \"\",1\n\
\t\tP: \"UnitScaleFactor\", \"double\", \"Number\", \"\",1\n\
\t}\n\
}\n\
\n";

/// Object ids for the mesh at position `i`: (geometry, model). Both are positive,
/// so they never collide with the implicit scene root (id 0).
fn ids(i: usize) -> (i64, i64) {
    let base = 2 * (i as i64);
    (base + 1, base + 2)
}

fn write_objects(out: &mut String, meshes: &[FbxMesh]) -> fmt::Result {
    out.push_str("Objects:  {\n");
    for (i, mesh) in meshes.iter().enumerate() {
        let (geometry_id, model_id) = ids(i);
        write_geometry(out, geometry_id, mesh)?;
        write_model(out, model_id, &mesh.name)?;
    }
    out.push_str("}\n\n");
    Ok(())
}

fn write_geometry(out: &mut String, id: i64, mesh: &FbxMesh) -> fmt::Result {
    writeln!(
        out,
        "\tGeometry: {id}, \"Geometry::{}\", \"Mesh\" {{",
        mesh.name
    )?;
    out.push_str("\t\tGeometryVersion: 124\n");
    push_array(
        out,
        "\t\t",
        "Vertices",
        mesh.vertices.len() * 3,
        mesh.vertices
            .iter()
            .flat_map(|v| v.iter().map(|&c| float_text(c))),
    )?;
    push_array(
        out,
        "\t\t",
        "PolygonVertexIndex",
        mesh.indices.len(),
        polygon_indices(&mesh.indices).map(|i| i.to_string()),
    )?;
    if let Some(normals) = &mesh.normals {
        out.push_str("\t\tLayerElementNormal: 0 {\n");
        out.push_str("\t\t\tVersion: 101\n\t\t\tName: \"\"\n");
        out.push_str("\t\t\tMappingInformationType: \"ByVertice\"\n");
        out.push_str("\t\t\tReferenceInformationType: \"Direct\"\n");
        push_array(
            out,
            "\t\t\t",
            "Normals",
            normals.len() * 3,
            normals
                .iter()
                .flat_map(|n| n.iter().map(|&c| float_text(c))),
        )?;
        out.push_str("\t\t}\n");
        out.push_str("\t\tLayer: 0 {\n\t\t\tVersion: 100\n");
        out.push_str("\t\t\tLayerElement:  {\n");
        out.push_str("\t\t\t\tType: \"LayerElementNormal\"\n\t\t\t\tTypedIndex: 0\n");
        out.push_str("\t\t\t}\n\t\t}\n");
    }
    out.push_str("\t}\n");
    Ok(())
}

fn write_model(out: &mut String, id: i64, name: &str) -> fmt::Result {
    writeln!(out, "\tModel: {id}, \"Model::{name}\", \"Mesh\" {{")?;
    out.push_str("\t\tVersion: 232\n");
    out.push_str("\t\tProperties70:  {\n");
    out.push_str("\t\t\tP: \"Lcl Translation\", \"Lcl Translation\", \"\", \"A\",0,0,0\n");
    out.push_str("\t\t\tP: \"Lcl Rotation\", \"Lcl Rotation\", \"\", \"A\",0,0,0\n");
    out.push_str("\t\t\tP: \"Lcl Scaling\", \"Lcl Scaling\", \"\", \"A\",1,1,1\n");
    out.push_str("\t\t}\n");
    out.push_str("\t\tShading: T\n");
    out.push_str("\t\tCulling: \"CullingOff\"\n");
    out.push_str("\t}\n");
    Ok(())
}

fn write_connections(out: &mut String, mesh_count: usize) -> fmt::Result {
    out.push_str("Connections:  {\n");
    for i in 0..mesh_count {
        let (geometry_id, model_id) = ids(i);
        writeln!(out, "\tC: \"OO\", {geometry_id}, {model_id}")?;
        writeln!(out, "\tC: \"OO\", {model_id}, 0")?;
    }
    out.push_str("}\n");
    Ok(())
}

/// Writes `key: *count { a: ... }` with the key line at `indent`.
fn push_array(
    out: &mut String,
    indent: &str,
    key: &str,
    count: usize,
    items: impl Iterator<Item = String>,
) -> fmt::Result {
    let inner = format!("{indent}\t");
    writeln!(out, "{indent}{key}: *{count} {{")?;
    write!(out, "{inner}a: ")?;
    push_joined(out, &inner, items);
    writeln!(out, "\n{indent}}}")
}

fn push_joined(out: &mut String, continuation: &str, items: impl Iterator<Item = String>) {
    for (i, item) in items.enumerate() {
        if i > 0 {
            out.push(',');
            if i % VALUES_PER_LINE == 0 {
                out.push('\n');
                out.push_str(continuation);
            }
        }
        out.push_str(&item);
    }
}

/// Encodes each triangle as `a, b, -(c + 1)`.
fn polygon_indices(indices: &[u32]) -> impl Iterator<Item = i64> + '_ {
    indices
        .chunks_exact(3)
        .flat_map(|t| [i64::from(t[0]), i64::from(t[1]), -i64::from(t[2]) - 1])
}

/// Formats a float so it always reads as a real number (`1.0`, not `1`).
fn float_text(v: f32) -> String {
    let s = v.to_string();
    if s.contains('.') { s } else { s + ".0" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit quad split into two triangles: (0,1,2) and (2,1,3).
    fn quad(name: &str) -> FbxMesh {
        FbxMesh {
            name: name.to_string(),
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            indices: vec![0, 1, 2, 2, 1, 3],
            normals: None,
        }
    }

    /// A file path that is removed on creation and on drop.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("acxs_fbx_test_{}_{tag}.fbx", std::process::id()));
            let _ = fs::remove_file(&path);
            TempFile(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn document_contains_all_top_level_sections() {
        let text = to_fbx_string(&[quad("Quad")]).unwrap();
        assert!(text.starts_with("; FBX 7.4.0 project file"));
        for marker in [
            "FBXHeaderExtension:  {",
            "FBXVersion: 7400",
            "GlobalSettings:  {",
            "Objects:  {",
            "\tGeometry: 1, \"Geometry::Quad\", \"Mesh\" {",
            "\tModel: 2, \"Model::Quad\", \"Mesh\" {",
            "Connections:  {",
        ] {
            assert!(text.contains(marker), "missing {marker:?} in:\n{text}");
        }
    }

    #[test]
    fn vertices_are_flat_and_always_have_a_decimal_point() {
        let text = to_fbx_string(&[quad("Quad")]).unwrap();
        assert!(text.contains(
            "Vertices: *12 {\n\t\t\ta: 0.0,0.0,0.0,1.0,0.0,0.0,0.0,1.0,0.0,1.0,1.0,0.0\n\t\t}\n"
        ));
    }

    #[test]
    fn last_index_of_each_triangle_is_negated_minus_one() {
        let text = to_fbx_string(&[quad("Quad")]).unwrap();
        // (0,1,2) -> 0,1,-3 and (2,1,3) -> 2,1,-4
        assert!(text.contains("PolygonVertexIndex: *6 {\n\t\t\ta: 0,1,-3,2,1,-4\n\t\t}\n"));
    }

    #[test]
    fn long_index_lists_wrap_and_decode_back_to_the_original() {
        let mut mesh = quad("Long");
        mesh.vertices = vec![[0.0; 3]; 15];
        mesh.indices = (0..15).collect();
        let text = to_fbx_string(&[mesh]).unwrap();

        let start = text.find("PolygonVertexIndex").unwrap();
        let body = &text[start..];
        let values = &body[body.find("a: ").unwrap() + 3..];
        let values = &values[..values.find("\n\t\t}").unwrap()];
        assert!(
            values.contains(",\n\t\t\t"),
            "expected a wrapped line in {values:?}"
        );

        let encoded: Vec<i64> = values
            .split(',')
            .map(|s| s.trim().parse().unwrap())
            .collect();
        assert_eq!(encoded.len(), 15);
        let decoded: Vec<i64> = encoded
            .chunks(3)
            .flat_map(|t| {
                assert!(t[2] < 0, "last entry of a polygon must be negative");
                [t[0], t[1], -t[2] - 1]
            })
            .collect();
        assert_eq!(decoded, (0..15).collect::<Vec<i64>>());
    }

    #[test]
    fn normals_are_written_only_when_given() {
        let plain = to_fbx_string(&[quad("Plain")]).unwrap();
        assert!(!plain.contains("LayerElementNormal"));

        let mut shaded = quad("Shaded");
        shaded.normals = Some(vec![[0.0, 0.0, 1.0]; 4]);
        let text = to_fbx_string(&[shaded]).unwrap();
        assert!(text.contains("LayerElementNormal: 0 {"));
        assert!(text.contains("MappingInformationType: \"ByVertice\""));
        assert!(text.contains("ReferenceInformationType: \"Direct\""));
        assert!(text.contains("Normals: *12 {"));
        assert!(text.contains("Type: \"LayerElementNormal\""));
    }

    #[test]
    fn each_geometry_connects_to_its_model_and_each_model_to_root() {
        let text = to_fbx_string(&[quad("A"), quad("B")]).unwrap();
        assert!(text.contains("\tC: \"OO\", 1, 2\n"));
        assert!(text.contains("\tC: \"OO\", 2, 0\n"));
        assert!(text.contains("\tC: \"OO\", 3, 4\n"));
        assert!(text.contains("\tC: \"OO\", 4, 0\n"));
    }

    #[test]
    fn empty_scene_is_a_valid_document_without_objects() {
        let text = to_fbx_string(&[]).unwrap();
        assert!(text.contains("Objects:  {\n}\n"));
        assert!(text.contains("Connections:  {\n}\n"));
        assert!(!text.contains("Geometry:"));
    }

    #[test]
    fn rejects_index_count_that_is_not_a_multiple_of_three() {
        let mut mesh = quad("Bad");
        mesh.indices.truncate(4);
        assert!(matches!(
            to_fbx_string(&[mesh]),
            Err(FbxError::IndexCount { len: 4, .. })
        ));
    }

    #[test]
    fn rejects_index_past_the_vertex_list() {
        let mut mesh = quad("Bad");
        mesh.indices[5] = 4;
        assert!(matches!(
            to_fbx_string(&[mesh]),
            Err(FbxError::IndexOutOfRange {
                index: 4,
                vertex_count: 4,
                ..
            })
        ));
    }

    #[test]
    fn rejects_normal_count_that_differs_from_vertex_count() {
        let mut mesh = quad("Bad");
        mesh.normals = Some(vec![[0.0, 0.0, 1.0]; 3]);
        assert!(matches!(
            to_fbx_string(&[mesh]),
            Err(FbxError::NormalCount {
                normals: 3,
                vertices: 4,
                ..
            })
        ));
    }

    #[test]
    fn rejects_non_finite_coordinates() {
        let mut mesh = quad("Bad");
        mesh.vertices[2][1] = f32::NAN;
        assert!(matches!(
            to_fbx_string(&[mesh]),
            Err(FbxError::NonFinite {
                kind: "vertex",
                position: 2,
                ..
            })
        ));

        let mut mesh = quad("Bad");
        mesh.normals = Some(vec![[0.0, 0.0, f32::INFINITY]; 4]);
        assert!(matches!(
            to_fbx_string(&[mesh]),
            Err(FbxError::NonFinite {
                kind: "normal",
                position: 0,
                ..
            })
        ));
    }

    #[test]
    fn rejects_names_that_would_break_the_file_syntax() {
        for name in ["", "with\"quote", "line\nbreak"] {
            assert!(
                matches!(to_fbx_string(&[quad(name)]), Err(FbxError::InvalidName(_))),
                "name {name:?} should be rejected"
            );
        }
    }

    #[test]
    fn write_fbx_writes_the_same_text_as_to_fbx_string() {
        let file = TempFile::new("roundtrip");
        let meshes = [quad("Quad")];
        write_fbx(&file.0, &meshes).unwrap();
        assert_eq!(
            fs::read_to_string(&file.0).unwrap(),
            to_fbx_string(&meshes).unwrap()
        );
    }

    #[test]
    fn write_fbx_refuses_to_overwrite_an_existing_file() {
        let file = TempFile::new("overwrite");
        write_fbx(&file.0, &[quad("First")]).unwrap();
        let before = fs::read_to_string(&file.0).unwrap();

        let err = write_fbx(&file.0, &[quad("Second")]).unwrap_err();
        assert!(matches!(err, FbxError::Exists(ref p) if *p == file.0));
        assert_eq!(fs::read_to_string(&file.0).unwrap(), before);
    }

    #[test]
    fn write_fbx_validates_before_creating_the_file() {
        let file = TempFile::new("invalid");
        let mut mesh = quad("Bad");
        mesh.indices.push(0);
        assert!(matches!(
            write_fbx(&file.0, &[mesh]),
            Err(FbxError::IndexCount { .. })
        ));
        assert!(!file.0.exists());
    }
}
