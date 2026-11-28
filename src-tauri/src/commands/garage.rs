//! Garage, car tuning, showroom, KN5, setups, packages, mods, and content health commands.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use acxs_manager::acd;
use acxs_manager::ai_spline;
use acxs_manager::ao_bake::{self, AoSettings};
use acxs_manager::car_tuning::{self, Axle, Tuning};
use acxs_manager::cardata;
use acxs_manager::content_meta::{self, CarMeta, TrackMeta};
use acxs_manager::content_repair::{self, RepairAction, Severity};
use acxs_manager::game;
use acxs_manager::ini::Ini;
use acxs_manager::installer::{self, ArchiveFormat, AssetKind, ConflictKind};
use acxs_manager::kn5_scene::{self, LoadOptions};
use acxs_manager::mods;
use acxs_manager::ortho::Triangle;
use acxs_manager::session_recorder;
use acxs_manager::settings;
use acxs_manager::setup_ab;
use acxs_manager::setup_schema;
use acxs_manager::setups;
use acxs_manager::sfx;
use acxs_manager::skins::{self, SkinMeta};
use acxs_manager::track_files;
use acxs_manager::vao_patch::{self, VaoMeshPatch, VaoPatch};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ac_root_from_state(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    settings
        .ac_root_path()
        .ok_or_else(|| "Assetto Corsa root folder is not configured".to_string())
}

fn ensure_game_not_running() -> Result<(), String> {
    if game::is_game_running() {
        return Err("Assetto Corsa is currently running; close the game before modifying files.".into());
    }
    Ok(())
}

fn normalize_vec3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 && len.is_finite() {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn asset_kind_str(kind: &AssetKind) -> &'static str {
    match kind {
        AssetKind::Car => "car",
        AssetKind::Track => "track",
        AssetKind::CarSkin { .. } => "carSkin",
        AssetKind::TrackSkin { .. } => "trackSkin",
        AssetKind::PpFilter => "ppFilter",
        AssetKind::Weather => "weather",
        AssetKind::PythonApp => "pythonApp",
        AssetKind::LuaApp => "luaApp",
        AssetKind::CspConfig => "cspConfig",
        AssetKind::Font => "font",
        AssetKind::Driver => "driver",
        AssetKind::Helmet => "helmet",
        AssetKind::Crew => "crew",
        AssetKind::Sound { .. } => "sound",
        AssetKind::Showroom => "showroom",
        AssetKind::GenericMod => "genericMod",
    }
}

fn archive_format_str(fmt: ArchiveFormat) -> &'static str {
    match fmt {
        ArchiveFormat::Zip => "zip",
        ArchiveFormat::SevenZ => "7z",
        ArchiveFormat::TarGz => "tar.gz",
        ArchiveFormat::Rar => "rar",
        ArchiveFormat::Folder => "folder",
        ArchiveFormat::SingleFile => "singleFile",
    }
}

// ---------------------------------------------------------------------------
// 1. 3D Showroom & Track Scenes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds3DDto {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub center: [f32; 3],
    pub radius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarMaterialDto {
    pub name: String,
    pub shader: String,
    pub blend_mode: u8,
    pub alpha_tested: bool,
    pub ks_diffuse: f32,
    pub ks_specular: f32,
    pub ks_specular_exp: f32,
    pub diffuse_texture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarMeshPartDto {
    pub material_index: usize,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub uvs: Vec<f32>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Car3DSceneDto {
    pub lod_file: String,
    pub mesh_count: usize,
    pub vertex_count: usize,
    pub triangle_count: usize,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub parts: Vec<CarMeshPartDto>,
    pub materials: Vec<CarMaterialDto>,
    pub textures: BTreeMap<String, String>,
    pub bounds: Bounds3DDto,
}

#[tauri::command]
pub async fn load_car_3d_scene(
    car_path: String,
    skin: Option<String>,
) -> Result<Car3DSceneDto, String> {
    tauri::async_runtime::spawn_blocking(move || load_car_3d_scene_sync(&car_path, skin.as_deref()))
        .await
        .map_err(|e| format!("3D car loader worker failed: {e}"))?
}

fn load_car_3d_scene_sync(
    car_path: &str,
    skin: Option<&str>,
) -> Result<Car3DSceneDto, String> {
    let opts = LoadOptions {
        load_textures: true,
        skip_ac_helpers: true,
        ..LoadOptions::default()
    };
    let car_scene = kn5_scene::load_car_lod(Path::new(car_path), skin, 0, &opts)
        .map_err(|e| e.to_string())?;

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut mesh_count = 0usize;
    let mut vertex_count = 0usize;
    let mut triangle_count = 0usize;

    // Group meshes by KN5 material index so each material renders with its own texture & shader params.
    let mut part_by_mat: BTreeMap<usize, CarMeshPartDto> = BTreeMap::new();
    let fallback_mat_idx = car_scene.scene.materials.len();

    for mesh in &car_scene.scene.meshes {
        if mesh.positions.is_empty() || mesh.indices.is_empty() {
            continue;
        }
        let upper_name = mesh.name.to_ascii_uppercase();
        if upper_name.contains("COLLIDER")
            || upper_name.contains("RIM_BLUR")
            || upper_name.contains("TYRE_BLUR")
            || upper_name.contains("TIRE_BLUR")
            || upper_name.contains("AMBIENT_SHADOW")
        {
            continue;
        }

        let mat_idx = usize::try_from(mesh.material_id)
            .ok()
            .filter(|&i| i < car_scene.scene.materials.len())
            .unwrap_or(fallback_mat_idx);

        let part = part_by_mat.entry(mat_idx).or_insert_with(|| CarMeshPartDto {
            material_index: mat_idx,
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        });

        let base_vertex = (part.positions.len() / 3) as u32;
        mesh_count += 1;

        for (i, pos) in mesh.positions.iter().enumerate() {
            let wp = kn5_scene::mat4_transform(&mesh.transform, pos, true);
            for axis in 0..3 {
                if wp[axis].is_finite() {
                    if wp[axis] < min[axis] {
                        min[axis] = wp[axis];
                    }
                    if wp[axis] > max[axis] {
                        max[axis] = wp[axis];
                    }
                }
            }
            part.positions.extend_from_slice(&wp);

            let raw_n = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            let wn = normalize_vec3(kn5_scene::mat4_transform(&mesh.transform, &raw_n, false));
            part.normals.extend_from_slice(&wn);

            let uv = mesh.uvs.get(i).copied().unwrap_or([0.0, 0.0]);
            part.uvs.extend_from_slice(&uv);
        }

        for &idx in &mesh.indices {
            part.indices.push(base_vertex + u32::from(idx));
        }

        vertex_count += mesh.positions.len();
        triangle_count += mesh.indices.len() / 3;
    }

    // Build compact material list & decode only textures actually used by rendered parts.
    let mut compact_mat_map: BTreeMap<usize, usize> = BTreeMap::new();
    let mut materials: Vec<CarMaterialDto> = Vec::new();
    let mut textures: BTreeMap<String, String> = BTreeMap::new();
    let mut parts: Vec<CarMeshPartDto> = Vec::with_capacity(part_by_mat.len());

    for (orig_mat_idx, mut part) in part_by_mat {
        let new_idx = if let Some(&existing) = compact_mat_map.get(&orig_mat_idx) {
            existing
        } else {
            let dto = if let Some(m) = car_scene.scene.materials.get(orig_mat_idx) {
                let is_valid_tex = |name: &str| {
                    !name.is_empty() && !name.eq_ignore_ascii_case("null.dds")
                };
                let diff_name = is_valid_tex(&m.tx_diffuse).then(|| m.tx_diffuse.clone());
                let detail_name = (m.use_detail > 0.5 && is_valid_tex(&m.tx_detail))
                    .then(|| m.tx_detail.clone());
                let fallback_detail_name = is_valid_tex(&m.tx_detail).then(|| m.tx_detail.clone());

                let keep_alpha = m.blend_mode != 0
                    || m.alpha_tested
                    || m.shader.to_ascii_lowercase().contains("alpha")
                    || m.shader.to_ascii_lowercase().contains("glass")
                    || m.shader.to_ascii_lowercase().contains("windscreen");

                let mut resolved_tex_key = None;
                if let (Some(diff), Some(det)) = (&diff_name, &detail_name) {
                    if let (Some(diff_bytes), Some(det_bytes)) =
                        (car_scene.texture_bytes(diff), car_scene.texture_bytes(det))
                    {
                        let mult = if m.detail_uv_multiplier.is_finite()
                            && m.detail_uv_multiplier > 0.01
                        {
                            m.detail_uv_multiplier
                        } else {
                            1.0
                        };
                        let key = format!(
                            "{}::detail::{}::{mult:.1}::{keep_alpha}",
                            diff.to_ascii_lowercase(),
                            det.to_ascii_lowercase()
                        );
                        if !textures.contains_key(&key) {
                            if let Some(data_url) = kn5_scene::encode_composited_car_paint_data_url(
                                diff_bytes, det_bytes, mult, 1024, keep_alpha,
                            ) {
                                textures.insert(key.clone(), data_url);
                            }
                        }
                        if textures.contains_key(&key) {
                            resolved_tex_key = Some(key);
                        }
                    }
                }

                if resolved_tex_key.is_none() {
                    if let Some(name) = diff_name.or(fallback_detail_name) {
                        let key = format!("{}::{keep_alpha}", name.to_ascii_lowercase());
                        if !textures.contains_key(&key) {
                            if let Some(raw_bytes) = car_scene.texture_bytes(&name) {
                                if let Some(data_url) =
                                    kn5_scene::encode_texture_data_url(raw_bytes, 1024, keep_alpha)
                                {
                                    textures.insert(key.clone(), data_url);
                                }
                            }
                        }
                        if textures.contains_key(&key) {
                            resolved_tex_key = Some(key);
                        }
                    }
                }

                CarMaterialDto {
                    name: m.name.clone(),
                    shader: m.shader.clone(),
                    blend_mode: m.blend_mode,
                    alpha_tested: m.alpha_tested,
                    ks_diffuse: m.ks_diffuse,
                    ks_specular: m.ks_specular,
                    ks_specular_exp: m.ks_specular_exp,
                    diffuse_texture: resolved_tex_key,
                }
            } else {
                CarMaterialDto {
                    name: "Default".to_string(),
                    shader: String::new(),
                    blend_mode: 0,
                    alpha_tested: false,
                    ks_diffuse: 0.6,
                    ks_specular: 0.5,
                    ks_specular_exp: 10.0,
                    diffuse_texture: None,
                }
            };
            let idx = materials.len();
            materials.push(dto);
            compact_mat_map.insert(orig_mat_idx, idx);
            idx
        };
        part.material_index = new_idx;
        parts.push(part);
    }

    let bounds = if min[0].is_finite() && max[0].is_finite() {
        let center = [
            0.5 * (min[0] + max[0]),
            0.5 * (min[1] + max[1]),
            0.5 * (min[2] + max[2]),
        ];
        let dx = max[0] - center[0];
        let dy = max[1] - center[1];
        let dz = max[2] - center[2];
        let radius = (dx * dx + dy * dy + dz * dz).sqrt();
        Bounds3DDto {
            min,
            max,
            center,
            radius,
        }
    } else {
        Bounds3DDto {
            min: [0.0; 3],
            max: [0.0; 3],
            center: [0.0; 3],
            radius: 1.0,
        }
    };

    Ok(Car3DSceneDto {
        lod_file: car_scene.lod_file,
        mesh_count,
        vertex_count,
        triangle_count,
        positions: Vec::new(),
        normals: Vec::new(),
        indices: Vec::new(),
        parts,
        materials,
        textures,
        bounds,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplinePointDto {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub length_m: f32,
    pub norm_pos: f32,
    pub side_left: f32,
    pub side_right: f32,
    pub speed_hint: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackModelSummaryDto {
    pub file: String,
    pub mesh_count: usize,
    pub helper_count: usize,
    pub triangle_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrsZoneDto {
    pub detection: f32,
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackCameraDto {
    pub name: String,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub min_fov: f32,
    pub max_fov: f32,
    pub in_point: f32,
    pub out_point: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSectionDto {
    pub in_point: f32,
    pub out_point: f32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHintDto {
    pub section: String,
    pub start: f32,
    pub end: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track3DSceneDto {
    pub layout: Option<String>,
    pub spline_length_m: f32,
    pub spline_points: Vec<SplinePointDto>,
    pub models: Vec<TrackModelSummaryDto>,
    pub drs_zones: Vec<DrsZoneDto>,
    pub cameras: Vec<TrackCameraDto>,
    pub sections: Vec<TrackSectionDto>,
    pub ai_hints: Vec<AiHintDto>,
    pub bounds: Bounds3DDto,
}

#[tauri::command]
pub async fn load_track_3d_scene(
    track_path: String,
    layout: Option<String>,
) -> Result<Track3DSceneDto, String> {
    tauri::async_runtime::spawn_blocking(move || load_track_3d_scene_sync(&track_path, layout))
        .await
        .map_err(|e| format!("3D track loader worker failed: {e}"))?
}

fn load_track_3d_scene_sync(
    track_path: &str,
    layout: Option<String>,
) -> Result<Track3DSceneDto, String> {
    let track_dir = Path::new(track_path);
    if !track_dir.is_dir() {
        return Err(format!("Track directory not found: {track_path}"));
    }

    let layout_trimmed = layout
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let layout_dir = match &layout_trimmed {
        Some(l) if track_dir.join(l).is_dir() => track_dir.join(l),
        _ => track_dir.to_path_buf(),
    };
    let data_dir = if layout_dir.join("data").is_dir() {
        layout_dir.join("data")
    } else {
        track_dir.join("data")
    };
    let ai_dir = if layout_dir.join("ai").is_dir() {
        layout_dir.join("ai")
    } else {
        track_dir.join("ai")
    };

    // 1. AI spline corridor
    let mut spline_points = Vec::new();
    let mut spline_length_m = 0.0f32;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    let spline_path = ai_dir.join("fast_lane.ai");
    if spline_path.is_file() {
        if let Ok(spline) = ai_spline::read_ai_spline(&spline_path) {
            spline_length_m = spline.points.last().map(|p| p.length_m).unwrap_or(0.0);
            let total = spline.points.len().max(1) as f32;
            for (idx, pt) in spline.points.iter().enumerate() {
                let extra = spline.extra.as_ref().and_then(|e| e.get(idx));
                let norm_pos = extra.map(|e| e.norm_pos).unwrap_or(idx as f32 / total);
                let side_left = extra.map(|e| e.side_left).unwrap_or(6.0);
                let side_right = extra.map(|e| e.side_right).unwrap_or(6.0);
                let speed_hint = extra.map(|e| e.speed_hint).unwrap_or(0.0);

                for (axis, val) in [pt.x, pt.y, pt.z].iter().copied().enumerate() {
                    if val.is_finite() {
                        if val < min[axis] {
                            min[axis] = val;
                        }
                        if val > max[axis] {
                            max[axis] = val;
                        }
                    }
                }
                spline_points.push(SplinePointDto {
                    x: pt.x,
                    y: pt.y,
                    z: pt.z,
                    length_m: pt.length_m,
                    norm_pos,
                    side_left,
                    side_right,
                    speed_hint,
                });
            }
        }
    }

    // 2. Skip decoding full multi-hundred-MB track KN5 geometry here so the
    // Track Inspector opens in milliseconds instead of hanging for 30+ seconds.
    let models = Vec::new();

    // 3. Track data files (DRS, cameras, sections, ai_hints)
    let drs_zones = track_files::read_drs_zones(&data_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|z| DrsZoneDto {
            detection: z.detection,
            start: z.start,
            end: z.end,
        })
        .collect();

    let cameras_path = data_dir.join("cameras.ini");
    let cameras = if cameras_path.is_file() {
        track_files::read_cameras(&cameras_path)
            .unwrap_or_default()
            .into_iter()
            .map(|c| TrackCameraDto {
                name: c.name,
                position: c.position,
                forward: c.forward,
                up: c.up,
                min_fov: c.min_fov,
                max_fov: c.max_fov,
                in_point: c.in_point,
                out_point: c.out_point,
            })
            .collect()
    } else {
        Vec::new()
    };

    let sections_candidates = [
        layout_trimmed
            .as_ref()
            .map(|l| track_dir.join("ui").join(l).join("sections.ini")),
        Some(track_dir.join("ui").join("sections.ini")),
        Some(data_dir.join("sections.ini")),
    ];
    let sections = sections_candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .and_then(|p| track_files::read_sections(&p).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|s| TrackSectionDto {
            in_point: s.in_point,
            out_point: s.out_point,
            name: s.name,
        })
        .collect();

    let ai_hints = track_files::read_ai_hints(&data_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|h| AiHintDto {
            section: h.section,
            start: h.start,
            end: h.end,
            value: h.value,
        })
        .collect();

    let bounds = if min[0].is_finite() && max[0].is_finite() {
        let center = [
            0.5 * (min[0] + max[0]),
            0.5 * (min[1] + max[1]),
            0.5 * (min[2] + max[2]),
        ];
        let dx = max[0] - center[0];
        let dy = max[1] - center[1];
        let dz = max[2] - center[2];
        Bounds3DDto {
            min,
            max,
            center,
            radius: (dx * dx + dy * dy + dz * dz).sqrt(),
        }
    } else {
        Bounds3DDto {
            min: [0.0; 3],
            max: [0.0; 3],
            center: [0.0; 3],
            radius: 100.0,
        }
    };

    Ok(Track3DSceneDto {
        layout: layout_trimmed,
        spline_length_m,
        spline_points,
        models,
        drs_zones,
        cameras,
        sections,
        ai_hints,
        bounds,
    })
}

// ---------------------------------------------------------------------------
// 2. Car Physics & Tuning & ACD
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynoPointDto {
    pub rpm: f64,
    pub torque_nm: f64,
    pub power_kw: f64,
    pub power_hp: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynoCurveDto {
    pub limiter_rpm: Option<f64>,
    pub peak_power_kw: f64,
    pub peak_torque_nm: f64,
    pub points: Vec<DynoPointDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrivetrainDto {
    pub traction: Option<String>,
    pub gears: Vec<f64>,
    pub final_ratio: f64,
    pub reverse_ratio: Option<f64>,
    pub limiter_rpm: f64,
    pub rear_tyre_radius_m: f64,
    pub gear_top_speeds_kmh: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WingSpecDto {
    pub name: String,
    pub chord: f64,
    pub span: f64,
    pub position_z: f64,
    pub angle_deg: f64,
    pub cl_gain: f64,
    pub cd_gain: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AeroForcePointDto {
    pub speed_kmh: f64,
    pub front_downforce_n: f64,
    pub rear_downforce_n: f64,
    pub total_downforce_n: f64,
    pub total_drag_n: f64,
    pub front_balance_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AeroDto {
    pub wings: Vec<WingSpecDto>,
    pub force_curve: Vec<AeroForcePointDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TyreCompoundDto {
    pub index: u32,
    pub front_name: String,
    pub rear_name: String,
    pub front_ideal_psi: Option<f64>,
    pub rear_ideal_psi: Option<f64>,
    pub front_thermal_lut: Vec<[f64; 2]>,
    pub rear_thermal_lut: Vec<[f64; 2]>,
    pub front_wear_lut: Vec<[f64; 2]>,
    pub rear_wear_lut: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarSpecsDto {
    pub car_id: String,
    pub mass_kg: Option<f64>,
    pub fuel_tank_l: Option<f64>,
    pub max_power_w: Option<f64>,
    pub max_torque_nm: Option<f64>,
    pub max_rpm: Option<f64>,
    pub gear_count: Option<u32>,
    pub drive_type: Option<String>,
    pub front_weight_distribution: Option<f64>,
    pub tyres_front: Option<String>,
    pub tyres_rear: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarPhysicsDto {
    pub uses_packed_data: bool,
    pub specs: CarSpecsDto,
    pub dyno: Option<DynoCurveDto>,
    pub drivetrain: Option<DrivetrainDto>,
    pub aero: Option<AeroDto>,
    pub tyres: Vec<TyreCompoundDto>,
}

#[tauri::command]
pub fn get_car_physics(car_path: String) -> Result<CarPhysicsDto, String> {
    let dir = Path::new(&car_path);
    if !dir.is_dir() {
        return Err(format!("Car directory not found: {car_path}"));
    }

    let uses_packed_data = cardata::uses_packed_data(dir);
    let specs = cardata::read_specs(dir);
    let specs_dto = CarSpecsDto {
        car_id: specs.car_id,
        mass_kg: specs.mass_kg,
        fuel_tank_l: specs.fuel_tank_l,
        max_power_w: specs.max_power_w,
        max_torque_nm: specs.max_torque_nm,
        max_rpm: specs.max_rpm,
        gear_count: specs.gear_count,
        drive_type: specs.drive_type,
        front_weight_distribution: specs.front_weight_distribution,
        tyres_front: specs.tyres_front,
        tyres_rear: specs.tyres_rear,
    };

    let dyno = cardata::read_dyno(dir).map(|d| DynoCurveDto {
        limiter_rpm: d.limiter_rpm,
        peak_power_kw: d.peak_power_kw,
        peak_torque_nm: d.peak_torque_nm,
        points: d
            .points
            .into_iter()
            .map(|p| DynoPointDto {
                rpm: p.rpm,
                torque_nm: p.torque_nm,
                power_kw: p.power_kw,
                power_hp: p.power_hp,
            })
            .collect(),
    });

    let drivetrain = cardata::read_drivetrain(dir).map(|dt| {
        let gear_top_speeds_kmh = dt.gear_top_speeds_kmh();
        DrivetrainDto {
            traction: dt.traction,
            gears: dt.gears,
            final_ratio: dt.final_ratio,
            reverse_ratio: dt.reverse_ratio,
            limiter_rpm: dt.limiter_rpm,
            rear_tyre_radius_m: dt.rear_tyre_radius_m,
            gear_top_speeds_kmh,
        }
    });

    let aero = cardata::read_aero(dir).map(|a| {
        let sample_speeds = [80.0, 120.0, 160.0, 200.0, 250.0, 300.0];
        let force_curve = sample_speeds
            .iter()
            .map(|&v| {
                let f = a.forces_at_speed(v);
                AeroForcePointDto {
                    speed_kmh: f.speed_kmh,
                    front_downforce_n: f.front_downforce_n,
                    rear_downforce_n: f.rear_downforce_n,
                    total_downforce_n: f.total_downforce_n,
                    total_drag_n: f.total_drag_n,
                    front_balance_pct: f.front_balance_pct,
                }
            })
            .collect();
        let wings = a
            .wings
            .into_iter()
            .map(|w| WingSpecDto {
                name: w.name,
                chord: w.chord,
                span: w.span,
                position_z: w.position_z,
                angle_deg: w.angle_deg,
                cl_gain: w.cl_gain,
                cd_gain: w.cd_gain,
            })
            .collect();
        AeroDto { wings, force_curve }
    });

    let tyres = cardata::read_tyre_curves(dir)
        .into_iter()
        .map(|t| TyreCompoundDto {
            index: t.index,
            front_name: t.front_name,
            rear_name: t.rear_name,
            front_ideal_psi: t.front_ideal_psi,
            rear_ideal_psi: t.rear_ideal_psi,
            front_thermal_lut: t.front_thermal_lut.into_iter().map(|(x, y)| [x, y]).collect(),
            rear_thermal_lut: t.rear_thermal_lut.into_iter().map(|(x, y)| [x, y]).collect(),
            front_wear_lut: t.front_wear_lut.into_iter().map(|(x, y)| [x, y]).collect(),
            rear_wear_lut: t.rear_wear_lut.into_iter().map(|(x, y)| [x, y]).collect(),
        })
        .collect();

    Ok(CarPhysicsDto {
        uses_packed_data,
        specs: specs_dto,
        dyno,
        drivetrain,
        aero,
        tyres,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarTuningStateDto {
    pub uses_packed_data: bool,
    pub has_unpacked_data: bool,
    pub mass_kg: Option<f64>,
    pub fuel_tank_l: Option<f64>,
    pub final_drive: Option<f64>,
    pub front_ideal_psi: Option<f64>,
    pub rear_ideal_psi: Option<f64>,
    pub front_static_psi: Option<f64>,
    pub rear_static_psi: Option<f64>,
    pub front_camber_deg: Option<f64>,
    pub rear_camber_deg: Option<f64>,
    pub front_base_height_m: Option<f64>,
    pub rear_base_height_m: Option<f64>,
}

fn ini_f64(ini: &Ini, section: &str, key: &str) -> Option<f64> {
    ini.get(section, key)?
        .split(';')
        .next()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

#[tauri::command]
pub fn get_car_tuning(car_path: String) -> Result<CarTuningStateDto, String> {
    let dir = Path::new(&car_path);
    if !dir.is_dir() {
        return Err(format!("Car directory not found: {car_path}"));
    }
    let uses_packed_data = cardata::uses_packed_data(dir);
    let has_unpacked_data = dir.join("data").is_dir();

    let car_ini = cardata::load_data_ini(dir, "car.ini").ok();
    let dt_ini = cardata::load_data_ini(dir, "drivetrain.ini").ok();
    let tyres_ini = cardata::load_data_ini(dir, "tyres.ini").ok();
    let susp_ini = cardata::load_data_ini(dir, "suspensions.ini").ok();

    Ok(CarTuningStateDto {
        uses_packed_data,
        has_unpacked_data,
        mass_kg: car_ini.as_ref().and_then(|i| ini_f64(i, "BASIC", "TOTALMASS")),
        fuel_tank_l: car_ini.as_ref().and_then(|i| ini_f64(i, "FUEL", "MAX_FUEL")),
        final_drive: dt_ini.as_ref().and_then(|i| ini_f64(i, "GEARS", "FINAL")),
        front_ideal_psi: tyres_ini.as_ref().and_then(|i| ini_f64(i, "FRONT", "PRESSURE_IDEAL")),
        rear_ideal_psi: tyres_ini.as_ref().and_then(|i| ini_f64(i, "REAR", "PRESSURE_IDEAL")),
        front_static_psi: tyres_ini.as_ref().and_then(|i| ini_f64(i, "FRONT", "PRESSURE_STATIC")),
        rear_static_psi: tyres_ini.as_ref().and_then(|i| ini_f64(i, "REAR", "PRESSURE_STATIC")),
        front_camber_deg: susp_ini.as_ref().and_then(|i| ini_f64(i, "FRONT", "STATIC_CAMBER")),
        rear_camber_deg: susp_ini.as_ref().and_then(|i| ini_f64(i, "REAR", "STATIC_CAMBER")),
        front_base_height_m: susp_ini.as_ref().and_then(|i| ini_f64(i, "FRONT", "BASEY")),
        rear_base_height_m: susp_ini.as_ref().and_then(|i| ini_f64(i, "REAR", "BASEY")),
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AxleDto {
    Front,
    Rear,
}

impl From<AxleDto> for Axle {
    fn from(a: AxleDto) -> Self {
        match a {
            AxleDto::Front => Axle::Front,
            AxleDto::Rear => Axle::Rear,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TuningRequestDto {
    TorqueCurve { factor: f64 },
    TotalMass { kg: f64 },
    FuelTank { litres: f64 },
    GearRatios { factor: f64 },
    FinalDrive { factor: f64 },
    IdealPressure { axle: AxleDto, psi: f64 },
    StaticPressure { axle: AxleDto, psi: f64 },
    Camber { axle: AxleDto, degrees: f64 },
    BaseHeight { axle: AxleDto, metres: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningChangeDto {
    pub file: String,
    pub section: String,
    pub key: String,
    pub before: String,
    pub after: String,
}

#[tauri::command]
pub fn apply_car_tuning(
    car_path: String,
    tuning: TuningRequestDto,
) -> Result<Vec<TuningChangeDto>, String> {
    ensure_game_not_running()?;
    let op = match tuning {
        TuningRequestDto::TorqueCurve { factor } => Tuning::TorqueCurve(factor),
        TuningRequestDto::TotalMass { kg } => Tuning::TotalMass(kg),
        TuningRequestDto::FuelTank { litres } => Tuning::FuelTank(litres),
        TuningRequestDto::GearRatios { factor } => Tuning::GearRatios(factor),
        TuningRequestDto::FinalDrive { factor } => Tuning::FinalDrive(factor),
        TuningRequestDto::IdealPressure { axle, psi } => Tuning::IdealPressure {
            axle: axle.into(),
            psi,
        },
        TuningRequestDto::StaticPressure { axle, psi } => Tuning::StaticPressure {
            axle: axle.into(),
            psi,
        },
        TuningRequestDto::Camber { axle, degrees } => Tuning::Camber {
            axle: axle.into(),
            degrees,
        },
        TuningRequestDto::BaseHeight { axle, metres } => Tuning::BaseHeight {
            axle: axle.into(),
            metres,
        },
    };

    let changes = car_tuning::apply(Path::new(&car_path), op).map_err(|e| e.to_string())?;
    Ok(changes
        .into_iter()
        .map(|c| TuningChangeDto {
            file: c.file,
            section: c.section,
            key: c.key,
            before: c.before,
            after: c.after,
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcdOperationResultDto {
    pub file_count: usize,
    pub backup_path: Option<String>,
}

#[tauri::command]
pub fn unpack_car_acd(car_path: String) -> Result<AcdOperationResultDto, String> {
    let running = game::is_game_running();
    let (file_count, backup) =
        acd::unpack_car_data(Path::new(&car_path), running).map_err(|e| e.to_string())?;
    Ok(AcdOperationResultDto {
        file_count,
        backup_path: Some(backup.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub fn pack_car_acd(car_path: String) -> Result<AcdOperationResultDto, String> {
    let dir = Path::new(&car_path);
    let folder = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid car folder path".to_string())?;
    let data_dir = dir.join("data");
    if !data_dir.is_dir() {
        return Err(format!("Unpacked data/ folder not found in {car_path}"));
    }
    let out_acd = dir.join("data.acd");
    let running = game::is_game_running();
    let file_count =
        acd::pack_acd(&data_dir, folder, &out_acd, running).map_err(|e| e.to_string())?;
    Ok(AcdOperationResultDto {
        file_count,
        backup_path: None,
    })
}

// ---------------------------------------------------------------------------
// 3. SFX Inspector & Swapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SfxInfoDto {
    pub car_id: String,
    pub bank_files: Vec<String>,
    pub bank_size_bytes: u64,
    pub guids_present: bool,
    pub guids_path: Option<String>,
    pub referenced_car_ids: Vec<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SfxSwapReportDto {
    pub target_car_id: String,
    pub donor_car_id: String,
    pub bank_written: String,
    pub guids_written: String,
    pub backup_dir: Option<String>,
}

#[tauri::command]
pub fn inspect_car_sfx(car_path: String) -> Result<SfxInfoDto, String> {
    let dir = Path::new(&car_path);
    if !dir.is_dir() {
        return Err(format!("Car directory not found: {car_path}"));
    }
    let info = sfx::inspect_car_sfx(dir);
    let bank_size_bytes: u64 = info
        .bank_files
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let guids_present = info.guids_path.is_some();
    Ok(SfxInfoDto {
        car_id: info.car_id,
        bank_files: info
            .bank_files
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        bank_size_bytes,
        guids_present,
        guids_path: info.guids_path.map(|p| p.to_string_lossy().into_owned()),
        referenced_car_ids: info.referenced_car_ids,
        issues: info.issues,
    })
}

fn car_id_from_arg(arg: &str) -> String {
    let trimmed = arg.trim();
    if trimmed.contains('/') || trimmed.contains('\\') {
        Path::new(trimmed)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| trimmed.to_string())
    } else {
        trimmed.to_string()
    }
}

#[tauri::command]
pub fn swap_car_sfx(
    state: State<'_, AppState>,
    target_car: String,
    donor_car: String,
) -> Result<SfxSwapReportDto, String> {
    let ac_root = ac_root_from_state(&state)?;
    let target_id = car_id_from_arg(&target_car);
    let donor_id = car_id_from_arg(&donor_car);
    let report = sfx::swap_car_sound(&ac_root, &target_id, &donor_id, game::is_game_running())
        .map_err(|e| e.to_string())?;
    Ok(SfxSwapReportDto {
        target_car_id: report.target_car_id,
        donor_car_id: report.donor_car_id,
        bank_written: report.bank_written.to_string_lossy().into_owned(),
        guids_written: report.guids_written.to_string_lossy().into_owned(),
        backup_dir: report.backup_dir.map(|p| p.to_string_lossy().into_owned()),
    })
}

// ---------------------------------------------------------------------------
// 4. Car Setups & A/B Comparison
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupFileDto {
    pub car_id: String,
    pub track: Option<String>,
    pub name: String,
    pub path: String,
    pub modified_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupValueDto {
    pub section: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupParamSpecDto {
    pub section: String,
    pub tab: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub show_clicks: bool,
    pub help: Option<String>,
    pub lut: Option<Vec<(f64, String)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupDocumentDto {
    pub path: String,
    pub values: Vec<SetupValueDto>,
    pub schema: Vec<SetupParamSpecDto>,
    pub tabs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupDiffDto {
    pub section: String,
    pub key: String,
    pub left: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStintMetricsDto {
    pub setup_hash: String,
    pub setup_name: String,
    pub session_count: usize,
    pub valid_laps: usize,
    pub best_lap_ms: Option<u32>,
    pub mean_lap_ms: Option<f64>,
    pub lap_stddev_ms: Option<f64>,
    pub lockups_per_10min: f64,
    pub wheelspins_per_10min: f64,
    pub understeer_pct: f64,
    pub oversteer_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupAbScorecardDto {
    pub car_id: String,
    pub track_id: String,
    pub available_hashes: Vec<String>,
    pub a: Option<SetupStintMetricsDto>,
    pub b: Option<SetupStintMetricsDto>,
    pub mean_delta_ms: Option<f64>,
    pub best_delta_ms: Option<i64>,
    pub setup_diffs: Vec<SetupDiffDto>,
}

fn resolve_setup_target_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Setup path is empty".to_string());
    }

    let virtual_rel = trimmed
        .get(..8)
        .filter(|prefix| {
            prefix.eq_ignore_ascii_case("/setups/")
                || prefix.eq_ignore_ascii_case("/setups\\")
                || prefix.eq_ignore_ascii_case("\\setups\\")
                || prefix.eq_ignore_ascii_case("\\setups/")
        })
        .map(|_| &trimmed[8..]);

    let rel_str = if let Some(rel) = virtual_rel {
        Some(rel)
    } else {
        let p = Path::new(trimmed);
        if !p.has_root() && p.is_relative() {
            Some(trimmed)
        } else {
            None
        }
    };

    if let Some(rel) = rel_str {
        let docs = settings::ac_documents_dir()
            .ok_or_else(|| "Assetto Corsa documents folder could not be located".to_string())?;
        let mut resolved = docs.join("setups");
        let parts: Vec<&str> = rel.split(['/', '\\']).collect();
        if parts.is_empty() || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.contains(':')
                || part.contains('\0')
        }) {
            return Err(format!("Invalid relative setup path: {trimmed}"));
        }
        for part in parts {
            resolved.push(part);
        }
        return Ok(resolved);
    }

    Ok(PathBuf::from(trimmed))
}

#[tauri::command]
pub fn list_setups() -> Result<Vec<SetupFileDto>, String> {
    let Some(docs) = settings::ac_documents_dir() else {
        return Ok(Vec::new());
    };
    let setups_root = docs.join("setups");
    let files = setups::list_setups(&setups_root, None);
    Ok(files
        .into_iter()
        .map(|f| SetupFileDto {
            car_id: f.car_id,
            track: f.track,
            name: f.name,
            path: f.path.to_string_lossy().into_owned(),
            modified_unix_ms: f
                .modified
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64),
        })
        .collect())
}

#[tauri::command]
pub fn read_setup(path: String, car_path: Option<String>) -> Result<SetupDocumentDto, String> {
    let resolved = resolve_setup_target_path(&path)?;
    let raw_values = setups::values(&resolved).map_err(|e| e.to_string())?;
    let values = raw_values
        .into_iter()
        .map(|(section, key, value)| SetupValueDto {
            section,
            key,
            value,
        })
        .collect();

    let (schema, tabs) = match car_path.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(cp) => match setup_schema::read_setup_schema(Path::new(cp)) {
            Some(s) => {
                let tabs = s.tabs().into_iter().map(str::to_string).collect();
                let params = s
                    .params
                    .into_iter()
                    .map(|p| SetupParamSpecDto {
                        section: p.section,
                        tab: p.tab,
                        name: p.name,
                        min: p.min,
                        max: p.max,
                        step: p.step,
                        show_clicks: p.show_clicks,
                        help: p.help,
                        lut: p.lut,
                    })
                    .collect();
                (params, tabs)
            }
            None => (Vec::new(), Vec::new()),
        },
        None => (Vec::new(), Vec::new()),
    };

    Ok(SetupDocumentDto {
        path,
        values,
        schema,
        tabs,
    })
}

#[tauri::command]
pub fn save_setup(path: String, values: Vec<SetupValueDto>) -> Result<usize, String> {
    let target = resolve_setup_target_path(&path)?;
    let entries: Vec<(&str, &str, &str)> = values
        .iter()
        .map(|entry| (entry.section.as_str(), entry.key.as_str(), entry.value.as_str()))
        .collect();
    setups::set_values(&target, &entries, true, true).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn diff_setups(left: String, right: String) -> Result<Vec<SetupDiffDto>, String> {
    let left_path = resolve_setup_target_path(&left)?;
    let right_path = resolve_setup_target_path(&right)?;
    let ini_l = Ini::load(&left_path).map_err(|e| format!("Failed to read {left}: {e}"))?;
    let ini_r = Ini::load(&right_path).map_err(|e| format!("Failed to read {right}: {e}"))?;
    Ok(setups::diff(&ini_l, &ini_r)
        .into_iter()
        .map(|d| SetupDiffDto {
            section: d.section,
            key: d.key,
            left: d.left,
            right: d.right,
        })
        .collect())
}

#[tauri::command]
pub fn compare_setup_ab(car_id: String, track_id: String) -> Result<SetupAbScorecardDto, String> {
    let sessions = session_recorder::load_all(&session_recorder::sessions_dir());
    let mut hashes: Vec<String> = Vec::new();
    for s in &sessions {
        if s.car_id.eq_ignore_ascii_case(&car_id) && s.full_track_id().eq_ignore_ascii_case(&track_id)
        {
            if let Some(h) = &s.setup_hash {
                if !hashes.iter().any(|existing| existing == h) {
                    hashes.push(h.clone());
                }
            }
        }
    }

    if hashes.len() < 2 {
        return Ok(SetupAbScorecardDto {
            car_id,
            track_id,
            available_hashes: hashes,
            a: None,
            b: None,
            mean_delta_ms: None,
            best_delta_ms: None,
            setup_diffs: Vec::new(),
        });
    }

    let scorecard = setup_ab::compare_stints(
        &sessions,
        &car_id,
        &track_id,
        &hashes[0],
        &hashes[1],
        None,
        None,
    );

    let to_dto = |m: setup_ab::SetupStintMetrics| SetupStintMetricsDto {
        setup_hash: m.setup_hash,
        setup_name: m.setup_name,
        session_count: m.session_count,
        valid_laps: m.valid_laps,
        best_lap_ms: m.best_lap_ms,
        mean_lap_ms: m.mean_lap_ms,
        lap_stddev_ms: m.lap_stddev_ms,
        lockups_per_10min: m.lockups_per_10min,
        wheelspins_per_10min: m.wheelspins_per_10min,
        understeer_pct: m.understeer_pct,
        oversteer_pct: m.oversteer_pct,
    };

    match scorecard {
        Some(sc) => Ok(SetupAbScorecardDto {
            car_id: sc.car_id,
            track_id: sc.track_id,
            available_hashes: hashes,
            a: Some(to_dto(sc.a)),
            b: Some(to_dto(sc.b)),
            mean_delta_ms: sc.mean_delta_ms,
            best_delta_ms: sc.best_delta_ms,
            setup_diffs: sc
                .setup_diffs
                .into_iter()
                .map(|d| SetupDiffDto {
                    section: d.section,
                    key: d.key,
                    left: d.left,
                    right: d.right,
                })
                .collect(),
        }),
        None => Ok(SetupAbScorecardDto {
            car_id,
            track_id,
            available_hashes: hashes,
            a: None,
            b: None,
            mean_delta_ms: None,
            best_delta_ms: None,
            setup_diffs: Vec::new(),
        }),
    }
}

// ---------------------------------------------------------------------------
// 5. Universal Package Installer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedItemDto {
    pub kind: String,
    pub id: String,
    pub dest_rel: String,
    pub file_count: usize,
    pub total_size: u64,
    pub conflict: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagePlanDto {
    pub source: String,
    pub format: String,
    pub items: Vec<PlannedItemDto>,
    pub skipped_root_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledItemReportDto {
    pub kind: String,
    pub id: String,
    pub installed_to: String,
    pub file_count: usize,
    pub moved_existing_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchInstallReportDto {
    pub installed: Vec<InstalledItemReportDto>,
}

#[tauri::command]
pub fn inspect_package(
    state: State<'_, AppState>,
    path: String,
) -> Result<PackagePlanDto, String> {
    let (ac_root, seven_zip) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        (s.ac_root_path(), s.seven_zip_path.clone())
    };
    let plan = installer::inspect_package(
        Path::new(&path),
        ac_root.as_deref(),
        seven_zip.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    Ok(PackagePlanDto {
        source: plan.source.to_string_lossy().into_owned(),
        format: archive_format_str(plan.format).to_string(),
        items: plan
            .items
            .into_iter()
            .map(|item| {
                let conflict = item.conflict.map(|c| match c {
                    ConflictKind::ExistingDir(p) => format!("Existing folder: {}", p.display()),
                    ConflictKind::ExistingFiles(fs) => {
                        format!("{} existing file(s) would be replaced", fs.len())
                    }
                });
                PlannedItemDto {
                    kind: asset_kind_str(&item.kind).to_string(),
                    id: item.id,
                    dest_rel: item.dest_rel.to_string_lossy().into_owned(),
                    file_count: item.file_count,
                    total_size: item.total_size,
                    conflict,
                    selected: item.selected,
                }
            })
            .collect(),
        skipped_root_files: plan.skipped_root_files,
    })
}

#[tauri::command]
pub fn install_package(
    state: State<'_, AppState>,
    path: String,
    selected_ids: Vec<String>,
) -> Result<BatchInstallReportDto, String> {
    let (ac_root, seven_zip) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        let root = s
            .ac_root_path()
            .ok_or_else(|| "Assetto Corsa root folder is not configured".to_string())?;
        (root, s.seven_zip_path.clone())
    };

    let mut plan = installer::inspect_package(
        Path::new(&path),
        Some(ac_root.as_path()),
        seven_zip.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    if !selected_ids.is_empty() {
        let set: HashSet<&str> = selected_ids.iter().map(String::as_str).collect();
        for item in &mut plan.items {
            item.selected = set.contains(item.id.as_str());
        }
    }

    let report = installer::install_plan(
        &ac_root,
        &plan,
        game::is_game_running(),
        seven_zip.as_deref(),
    )
    .map_err(|e| e.to_string())?;

    Ok(BatchInstallReportDto {
        installed: report
            .installed
            .into_iter()
            .map(|i| InstalledItemReportDto {
                kind: asset_kind_str(&i.kind).to_string(),
                id: i.id,
                installed_to: i.installed_to.to_string_lossy().into_owned(),
                file_count: i.file_count,
                moved_existing_to: i
                    .moved_existing_to
                    .map(|p| p.to_string_lossy().into_owned()),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// 6. JSGME Mods & Content Enable/Disable
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsgmeModDto {
    pub name: String,
    pub enabled: bool,
    pub enabled_at: Option<String>,
    pub file_count: usize,
}

#[tauri::command]
pub fn list_jsgme_mods(state: State<'_, AppState>) -> Result<Vec<JsgmeModDto>, String> {
    let (ac_root, custom_mods_dir) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        let root = s
            .ac_root_path()
            .ok_or_else(|| "Assetto Corsa root folder is not configured".to_string())?;
        (root, s.mods_dir.clone())
    };

    let available = mods::list_jsgme_mods(&ac_root, custom_mods_dir.as_deref());
    let jsgme_state = mods::load_jsgme_state(&ac_root).unwrap_or_default();

    let mut out = Vec::with_capacity(available.len());
    for name in available {
        let active = jsgme_state
            .enabled
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(&name));
        out.push(JsgmeModDto {
            name,
            enabled: active.is_some(),
            enabled_at: active.map(|m| m.enabled_at.clone()),
            file_count: active.map(|m| m.files.len()).unwrap_or(0),
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn enable_jsgme_mod(state: State<'_, AppState>, name: String) -> Result<JsgmeModDto, String> {
    let (ac_root, custom_mods_dir) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        let root = s
            .ac_root_path()
            .ok_or_else(|| "Assetto Corsa root folder is not configured".to_string())?;
        (root, s.mods_dir.clone())
    };

    let enabled = mods::enable_jsgme_mod(
        &ac_root,
        custom_mods_dir.as_deref(),
        &name,
        game::is_game_running(),
    )
    .map_err(|e| e.to_string())?;

    Ok(JsgmeModDto {
        name: enabled.name,
        enabled: true,
        enabled_at: Some(enabled.enabled_at),
        file_count: enabled.files.len(),
    })
}

#[tauri::command]
pub fn disable_jsgme_mod(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let ac_root = ac_root_from_state(&state)?;
    mods::disable_jsgme_mod(&ac_root, &name, game::is_game_running()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn toggle_content_enabled(
    state: State<'_, AppState>,
    kind: String,
    id: String,
    enable: bool,
) -> Result<String, String> {
    let ac_root = ac_root_from_state(&state)?;
    let running = game::is_game_running();
    let dest = match (kind.to_ascii_lowercase().as_str(), enable) {
        ("car", true) => mods::enable_car(&ac_root, &id, running),
        ("car", false) => mods::disable_car(&ac_root, &id, running),
        ("track", true) => mods::enable_track(&ac_root, &id, running),
        ("track", false) => mods::disable_track(&ac_root, &id, running),
        _ => return Err(format!("Unsupported content kind: {kind} (expected 'car' or 'track')")),
    }
    .map_err(|e| e.to_string())?;

    Ok(dest.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// 7. Content Health, Repairs, Metadata & VAO Baking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthIssueDto {
    pub severity: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedRepairDto {
    pub description: String,
    pub path: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentHealthReportDto {
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub issues: Vec<HealthIssueDto>,
    pub planned_repairs: Vec<PlannedRepairDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairFailureDto {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReportDto {
    pub created: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<RepairFailureDto>,
}

fn repair_action_str(action: RepairAction) -> &'static str {
    match action {
        RepairAction::CreateMissingUiJson => "createMissingUiJson",
        RepairAction::CreateMissingSkinJson => "createMissingSkinJson",
        RepairAction::NormalizeCarTags => "normalizeCarTags",
        RepairAction::RecalculateCarSpecs => "recalculateCarSpecs",
        RepairAction::FixSfxGuidsMismatch => "fixSfxGuidsMismatch",
    }
}

#[tauri::command]
pub fn run_content_health(state: State<'_, AppState>) -> Result<ContentHealthReportDto, String> {
    let ac_root = ac_root_from_state(&state)?;
    let issues = content_repair::check_all(&ac_root);
    let (error_count, warning_count, info_count) = content_repair::summary(&issues);
    let repairs = content_repair::plan_extended_repairs(&ac_root);

    Ok(ContentHealthReportDto {
        error_count,
        warning_count,
        info_count,
        issues: issues
            .into_iter()
            .map(|i| HealthIssueDto {
                severity: match i.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "info",
                }
                .to_string(),
                path: i.path.to_string_lossy().into_owned(),
                message: i.message,
            })
            .collect(),
        planned_repairs: repairs
            .into_iter()
            .map(|r| PlannedRepairDto {
                description: r.description,
                path: r.path.to_string_lossy().into_owned(),
                action: repair_action_str(r.action).to_string(),
            })
            .collect(),
    })
}

#[tauri::command]
pub fn apply_content_repairs(state: State<'_, AppState>) -> Result<RepairReportDto, String> {
    ensure_game_not_running()?;
    let ac_root = ac_root_from_state(&state)?;
    let repairs = content_repair::plan_extended_repairs(&ac_root);
    let report = content_repair::apply_repairs(&ac_root, &repairs);

    Ok(RepairReportDto {
        created: report
            .created
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        skipped: report
            .skipped
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        failed: report
            .failed
            .into_iter()
            .map(|(p, reason)| RepairFailureDto {
                path: p.to_string_lossy().into_owned(),
                reason,
            })
            .collect(),
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarMetaPatchDto {
    pub name: Option<String>,
    pub brand: Option<String>,
    pub class: Option<String>,
    pub year: Option<i64>,
    pub tags: Option<Vec<String>>,
    pub description: Option<String>,
    pub specs: Option<BTreeMap<String, String>>,
}

#[tauri::command]
pub fn save_car_meta(car_path: String, meta: CarMetaPatchDto) -> Result<(), String> {
    let patch = CarMeta {
        name: meta.name,
        brand: meta.brand,
        class: meta.class,
        year: meta.year,
        tags: meta.tags,
        description: meta.description,
        specs: meta.specs,
    };
    content_meta::patch_car_meta(Path::new(&car_path), &patch).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackMetaPatchDto {
    pub name: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub length_m: Option<f64>,
    pub pitboxes: Option<u32>,
    pub author: Option<String>,
    pub version: Option<String>,
}

#[tauri::command]
pub fn save_track_meta(track_path: String, meta: TrackMetaPatchDto) -> Result<(), String> {
    let patch = TrackMeta {
        name: meta.name,
        city: meta.city,
        country: meta.country,
        description: meta.description,
        tags: meta.tags,
        length_m: meta.length_m,
        pitboxes: meta.pitboxes,
        author: meta.author,
        version: meta.version,
    };
    content_meta::patch_track_meta(Path::new(&track_path), &patch).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinMetaPatchDto {
    pub skinname: Option<String>,
    pub drivername: Option<String>,
    pub team: Option<String>,
    pub number: Option<String>,
    pub country: Option<String>,
    pub priority: Option<i64>,
}

#[tauri::command]
pub fn save_skin_meta(skin_path: String, meta: SkinMetaPatchDto) -> Result<Option<String>, String> {
    let patch = SkinMeta {
        skinname: meta.skinname,
        drivername: meta.drivername,
        team: meta.team,
        number: meta.number,
        country: meta.country,
        priority: meta.priority,
    };
    let backup = skins::save_skin_meta(Path::new(&skin_path), &patch).map_err(|e| e.to_string())?;
    Ok(backup.map(|p| p.to_string_lossy().into_owned()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaoBakeReportDto {
    pub out_path: String,
    pub bytes_written: usize,
    pub mesh_count: usize,
    pub triangle_count: usize,
}

#[tauri::command]
pub fn bake_vao_patch(kn5_path: String, out_path: String) -> Result<VaoBakeReportDto, String> {
    let opts = LoadOptions {
        load_textures: false,
        skip_ac_helpers: true,
        ..LoadOptions::default()
    };
    let scene = kn5_scene::load_kn5(Path::new(&kn5_path), &opts).map_err(|e| e.to_string())?;

    let mut all_triangles: Vec<Triangle> = Vec::new();
    // Track (mesh_index, vertex_count, triangle_corner_vertex_indices)
    let mut mesh_slices: Vec<(String, usize, Vec<[usize; 3]>)> = Vec::new();

    for mesh in &scene.meshes {
        if mesh.positions.is_empty() || mesh.indices.len() < 3 {
            continue;
        }
        let world_verts: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .map(|p| kn5_scene::mat4_transform(&mesh.transform, p, true))
            .collect();
        let mut tri_verts = Vec::with_capacity(mesh.indices.len() / 3);
        for chunk in mesh.indices.chunks_exact(3) {
            let (ia, ib, ic) = (usize::from(chunk[0]), usize::from(chunk[1]), usize::from(chunk[2]));
            if let (Some(&a), Some(&b), Some(&c)) = (
                world_verts.get(ia),
                world_verts.get(ib),
                world_verts.get(ic),
            ) {
                all_triangles.push(Triangle {
                    a,
                    b,
                    c,
                    color: [255, 255, 255],
                });
                tri_verts.push([ia, ib, ic]);
            }
        }
        if !tri_verts.is_empty() {
            mesh_slices.push((mesh.name.clone(), mesh.positions.len(), tri_verts));
        }
    }

    if all_triangles.is_empty() {
        return Err("No valid triangles found in KN5 file to bake VAO.".into());
    }

    let ao_settings = AoSettings {
        rays: 32,
        max_distance: 2.5,
        bias: 0.005,
    };
    let corner_ao =
        ao_bake::bake_vertex_ao(&all_triangles, &ao_settings).map_err(|e| e.to_string())?;

    let mut vao_meshes = Vec::with_capacity(mesh_slices.len());
    let mut cursor = 0usize;
    for (name, vert_count, tri_verts) in &mesh_slices {
        let mut sums = vec![0.0f32; *vert_count];
        let mut counts = vec![0u32; *vert_count];
        for &[ia, ib, ic] in tri_verts {
            if cursor + 2 < corner_ao.len() {
                sums[ia] += corner_ao[cursor];
                counts[ia] += 1;
                sums[ib] += corner_ao[cursor + 1];
                counts[ib] += 1;
                sums[ic] += corner_ao[cursor + 2];
                counts[ic] += 1;
            }
            cursor += 3;
        }
        let avg: Vec<f32> = sums
            .into_iter()
            .zip(counts)
            .map(|(s, c)| if c > 0 { s / c as f32 } else { 1.0 })
            .collect();
        vao_meshes.push(VaoMeshPatch::from_scalar_ao(name.clone(), &avg));
    }

    let mesh_count = vao_meshes.len();
    let patch = VaoPatch {
        version: 5,
        meshes: vao_meshes,
    };
    let bytes_written =
        vao_patch::write_vao_patch(Path::new(&out_path), &patch).map_err(|e| e.to_string())?;

    Ok(VaoBakeReportDto {
        out_path,
        bytes_written,
        mesh_count,
        triangle_count: all_triangles.len(),
    })
}
