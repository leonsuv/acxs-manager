#![allow(dead_code)]
//! Reusable 3D viewport built on `eframe::egui_wgpu` + `eframe::wgpu` (native default in
//! `eframe 0.37`) + `glam`.
//!
//! Designed for the Garage 3D Showroom, 3D Track Inspector, Trajectory Studio and Standalone
//! 3D Replay Viewer. Falls back cleanly to a 2D renderer when `render_state` is `None`
//! (headless tests or software contexts).

use eframe::egui::{self, Key, PaintCallbackInfo, PointerButton, Rect, Response};
use eframe::egui_wgpu::{Callback, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use eframe::wgpu;
use glam::{Mat4, Vec3};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MAX_CACHED_MESHES: usize = 32;

const MESH_WGSL: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    tint: vec4<f32>,
    mode: vec4<u32>, // x = 0 shaded | 1 wireframe | 2 normals, y = has_tex
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = u.mvp * vec4<f32>(in.pos, 1.0);
    let n = (u.model * vec4<f32>(in.normal, 0.0)).xyz;
    out.normal = normalize(n);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    if (u.mode.x == 2u) {
        return vec4<f32>(n * 0.5 + vec3<f32>(0.5), 1.0);
    }
    let sun = normalize(vec3<f32>(0.45, 0.82, 0.35));
    let ndl = max(dot(n, sun), 0.0);
    let hemi = mix(vec3<f32>(0.10, 0.12, 0.16), vec3<f32>(0.36, 0.42, 0.52), n.y * 0.5 + 0.5);
    let lit = u.tint.rgb * (hemi + vec3<f32>(0.95, 0.92, 0.88) * ndl * 0.85);
    return vec4<f32>(lit, u.tint.a);
}
"#;

const LINE_WGSL: &str = r#"
struct LineUniforms {
    vp: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> u: LineUniforms;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = u.vp * vec4<f32>(in.pos, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

/// Shading mode for 3D mesh passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    #[default]
    Shaded,
    Wireframe,
    Normals,
}

/// Interactive camera modes shared across Showroom, Track Inspector and Replay Viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Camera3D {
    Orbit { target: Vec3, yaw: f32, pitch: f32, dist: f32 },
    FreeFly { pos: Vec3, yaw: f32, pitch: f32, speed: f32 },
    Chase { pos: Vec3, target: Vec3, up: Vec3, fov_deg: f32 },
    TracksideTv { pos: Vec3, target: Vec3, fov_deg: f32 },
}

impl Default for Camera3D {
    fn default() -> Self {
        Self::Orbit { target: Vec3::ZERO, yaw: 0.75, pitch: 0.32, dist: 5.5 }
    }
}

impl Camera3D {
    pub fn eye_pos(&self) -> Vec3 {
        match *self {
            Self::Orbit { target, yaw, pitch, dist } => {
                let cp = pitch.cos();
                target + Vec3::new(yaw.cos() * cp, pitch.sin(), yaw.sin() * cp) * dist.max(0.2)
            }
            Self::FreeFly { pos, .. } | Self::Chase { pos, .. } | Self::TracksideTv { pos, .. } => pos,
        }
    }

    pub fn fov_radians(&self) -> f32 {
        match *self {
            Self::Chase { fov_deg, .. } | Self::TracksideTv { fov_deg, .. } => fov_deg.clamp(15.0, 110.0).to_radians(),
            Self::Orbit { .. } | Self::FreeFly { .. } => 45.0_f32.to_radians(),
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        match *self {
            Self::Orbit { target, .. } => Mat4::look_at_rh(self.eye_pos(), target, Vec3::Y),
            Self::FreeFly { pos, yaw, pitch, .. } => {
                let cp = pitch.cos();
                let dir = Vec3::new(yaw.cos() * cp, pitch.sin(), yaw.sin() * cp).normalize_or_zero();
                Mat4::look_at_rh(pos, pos + dir, Vec3::Y)
            }
            Self::Chase { pos, target, up, .. } => Mat4::look_at_rh(pos, target, up.normalize_or(Vec3::Y)),
            Self::TracksideTv { pos, target, .. } => Mat4::look_at_rh(pos, target, Vec3::Y),
        }
    }

    pub fn proj_matrix(&self, aspect: f32) -> Mat4 {
        let safe_aspect = if aspect.is_finite() && aspect > 0.01 { aspect } else { 16.0 / 9.0 };
        Mat4::perspective_rh(self.fov_radians(), safe_aspect, 0.05, 8000.0)
    }

    /// Update camera from pointer drag, scroll wheel and WASD+QE keys.
    pub fn handle_input(&mut self, resp: &Response, input: &egui::InputState, dt: f32) -> bool {
        let mut moved = false;
        let drag = resp.drag_delta();
        let lmb = resp.dragged_by(PointerButton::Primary);
        let rmb = resp.dragged_by(PointerButton::Secondary);
        let scroll = if resp.hovered() { input.smooth_scroll_delta.y } else { 0.0 };

        match self {
            Self::Orbit { target, yaw, pitch, dist } => {
                if lmb && drag != egui::Vec2::ZERO {
                    *yaw += drag.x * 0.008;
                    *pitch = (*pitch + drag.y * 0.008).clamp(-1.48, 1.48);
                    moved = true;
                }
                if rmb && drag != egui::Vec2::ZERO {
                    let right = Vec3::new(-yaw.sin(), 0.0, yaw.cos()).normalize_or(Vec3::X);
                    let scale = *dist * 0.0022;
                    *target -= right * drag.x * scale;
                    *target += Vec3::Y * drag.y * scale;
                    moved = true;
                }
                if scroll.abs() > 0.01 {
                    *dist = (*dist * (1.0 - scroll * 0.0015)).clamp(0.4, 2500.0);
                    moved = true;
                }
            }
            Self::FreeFly { pos, yaw, pitch, speed } => {
                if (lmb || rmb) && drag != egui::Vec2::ZERO {
                    *yaw += drag.x * 0.006;
                    *pitch = (*pitch - drag.y * 0.006).clamp(-1.48, 1.48);
                    moved = true;
                }
                if scroll.abs() > 0.01 {
                    *speed = (*speed * (1.0 + scroll * 0.002)).clamp(1.0, 500.0);
                    moved = true;
                }
                if resp.hovered() || resp.has_focus() {
                    let cp = pitch.cos();
                    let fwd = Vec3::new(yaw.cos() * cp, pitch.sin(), yaw.sin() * cp).normalize_or(Vec3::Z);
                    let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
                    let mut delta = Vec3::ZERO;
                    if input.key_down(Key::W) { delta += fwd; }
                    if input.key_down(Key::S) { delta -= fwd; }
                    if input.key_down(Key::D) { delta += right; }
                    if input.key_down(Key::A) { delta -= right; }
                    if input.key_down(Key::E) { delta += Vec3::Y; }
                    if input.key_down(Key::Q) { delta -= Vec3::Y; }
                    if delta.length_squared() > 0.0 {
                        let boost = if input.modifiers.shift { 3.0 } else { 1.0 };
                        *pos += delta.normalize() * (*speed * boost * dt.clamp(0.0, 0.1));
                        moved = true;
                    }
                }
            }
            Self::Chase { .. } | Self::TracksideTv { .. } => {}
        }
        moved
    }
}

/// Interleaved vertex layout uploaded to the mesh vertex buffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MeshVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// Colored vertex layout for 3D polylines, trajectories and track corridors.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MeshUniforms {
    mvp: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    tint: [f32; 4],
    mode: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LineUniforms {
    vp: [[f32; 4]; 4],
}

/// CPU-side sub-mesh ready to upload to [`GlRenderer`].
#[derive(Debug, Clone)]
pub struct CpuSubMesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub texture_key: Option<String>,
    pub tint: [f32; 4],
}

/// One mesh instance drawn in a [`SceneFrame`].
#[derive(Debug, Clone)]
pub struct DrawItem {
    pub mesh_key: (PathBuf, u64),
    pub model: Mat4,
    pub tint_override: Option<[f32; 4]>,
    pub texture_override: Option<String>,
}

/// One 3D polyline strip drawn in a [`SceneFrame`].
#[derive(Debug, Clone)]
pub struct DrawLines {
    pub vertices: Vec<LineVertex>,
    pub strip: bool,
}

/// Immutable per-frame draw description handed to [`paint_viewport_3d`].
#[derive(Debug, Clone)]
pub struct SceneFrame {
    pub camera: Camera3D,
    pub clear_color: [f32; 4],
    pub render_mode: RenderMode,
    pub items: Vec<DrawItem>,
    pub lines: Vec<DrawLines>,
}

impl Default for SceneFrame {
    fn default() -> Self {
        Self {
            camera: Camera3D::default(),
            clear_color: [0.027, 0.035, 0.051, 1.0],
            render_mode: RenderMode::Shaded,
            items: Vec::new(),
            lines: Vec::new(),
        }
    }
}

struct GpuSubMesh {
    vbo: wgpu::Buffer,
    ibo: wgpu::Buffer,
    index_count: u32,
    tint: [f32; 4],
}

/// LRU-bounded GPU mesh cache.
#[derive(Default)]
pub struct GpuMeshCache {
    meshes: HashMap<(PathBuf, u64), Vec<GpuSubMesh>>,
    order: VecDeque<(PathBuf, u64)>,
}

impl GpuMeshCache {
    pub fn contains_mesh(&self, key: &(PathBuf, u64)) -> bool {
        self.meshes.contains_key(key)
    }

    pub fn clear(&mut self) {
        self.meshes.clear();
        self.order.clear();
    }
}

struct PreparedLine {
    vbo: wgpu::Buffer,
    count: u32,
    strip: bool,
}

/// Shared 3D renderer initialized once from `eframe::CreationContext::wgpu_render_state`.
/// Named `GlRenderer` for API stability with Wave 3D callers.
pub struct GlRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    mesh_pipeline: wgpu::RenderPipeline,
    line_list_pipeline: wgpu::RenderPipeline,
    line_strip_pipeline: wgpu::RenderPipeline,
    mesh_uniform_buf: wgpu::Buffer,
    mesh_bind_group: wgpu::BindGroup,
    line_uniform_buf: wgpu::Buffer,
    line_bind_group: wgpu::BindGroup,
    prepared_lines: Vec<PreparedLine>,
    pub cache: GpuMeshCache,
}

impl GlRenderer {
    pub fn new(render_state: &RenderState) -> Result<Self, String> {
        let device = render_state.device.clone();
        let queue = render_state.queue.clone();
        let target_format = render_state.target_format;

        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("acxs_mesh_shader"),
            source: wgpu::ShaderSource::Wgsl(MESH_WGSL.into()),
        });
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("acxs_line_shader"),
            source: wgpu::ShaderSource::Wgsl(LINE_WGSL.into()),
        });

        let mesh_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("acxs_mesh_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let line_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("acxs_line_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let mesh_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acxs_mesh_uniforms"),
            size: std::mem::size_of::<MeshUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let line_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acxs_line_uniforms"),
            size: std::mem::size_of::<LineUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mesh_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acxs_mesh_bg"),
            layout: &mesh_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: mesh_uniform_buf.as_entire_binding() }],
        });
        let line_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acxs_line_bg"),
            layout: &line_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: line_uniform_buf.as_entire_binding() }],
        });

        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("acxs_mesh_pl"),
            bind_group_layouts: &[Some(&mesh_bgl)],
            immediate_size: 0,
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("acxs_line_pl"),
            bind_group_layouts: &[Some(&line_bgl)],
            immediate_size: 0,
        });

        let mesh_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
            ],
        };
        let line_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
            ],
        };

        let color_target = Some(wgpu::ColorTargetState {
            format: target_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        });

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("acxs_mesh_pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vertex_layout)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target.clone()],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let make_line_pipeline = |topology: wgpu::PrimitiveTopology, label: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&line_layout),
                vertex: wgpu::VertexState {
                    module: &line_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(line_vertex_layout.clone())],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &line_shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[color_target.clone()],
                }),
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        let line_list_pipeline = make_line_pipeline(wgpu::PrimitiveTopology::LineList, "acxs_line_list_pipeline");
        let line_strip_pipeline = make_line_pipeline(wgpu::PrimitiveTopology::LineStrip, "acxs_line_strip_pipeline");

        Ok(Self {
            device,
            queue,
            mesh_pipeline,
            line_list_pipeline,
            line_strip_pipeline,
            mesh_uniform_buf,
            mesh_bind_group,
            line_uniform_buf,
            line_bind_group,
            prepared_lines: Vec::new(),
            cache: GpuMeshCache::default(),
        })
    }

    /// Upload a group of CPU sub-meshes into the LRU cache.
    pub fn upload_mesh(&mut self, key: (PathBuf, u64), meshes: &[CpuSubMesh]) {
        self.cache.meshes.remove(&key);
        self.cache.order.retain(|k| k != &key);
        while self.cache.order.len() >= MAX_CACHED_MESHES {
            if let Some(oldest) = self.cache.order.pop_front() {
                self.cache.meshes.remove(&oldest);
            }
        }
        let mut uploaded = Vec::with_capacity(meshes.len());
        for sm in meshes {
            if sm.vertices.is_empty() || sm.indices.is_empty() {
                continue;
            }
            let vbytes = slice_as_bytes(&sm.vertices);
            let ibytes = slice_as_bytes(&sm.indices);
            let vbo = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("acxs_mesh_vbo"),
                size: vbytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let ibo = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("acxs_mesh_ibo"),
                size: ibytes.len() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&vbo, 0, vbytes);
            self.queue.write_buffer(&ibo, 0, ibytes);
            uploaded.push(GpuSubMesh { vbo, ibo, index_count: sm.indices.len() as u32, tint: sm.tint });
        }
        self.cache.meshes.insert(key.clone(), uploaded);
        self.cache.order.push_back(key);
    }

    fn prepare_scene(&mut self, scene: &SceneFrame, aspect: f32) {
        let vp = scene.camera.proj_matrix(aspect) * scene.camera.view_matrix();
        let line_u = LineUniforms { vp: vp.to_cols_array_2d() };
        self.queue.write_buffer(&self.line_uniform_buf, 0, slice_as_bytes(std::slice::from_ref(&line_u)));

        if let Some(first) = scene.items.first() {
            let mvp = vp * first.model;
            let mode = match scene.render_mode {
                RenderMode::Shaded => 0,
                RenderMode::Wireframe => 1,
                RenderMode::Normals => 2,
            };
            let tint = first.tint_override.unwrap_or([0.85, 0.87, 0.90, 1.0]);
            let mu = MeshUniforms {
                mvp: mvp.to_cols_array_2d(),
                model: first.model.to_cols_array_2d(),
                tint,
                mode: [mode, 0, 0, 0],
            };
            self.queue.write_buffer(&self.mesh_uniform_buf, 0, slice_as_bytes(std::slice::from_ref(&mu)));
        }

        self.prepared_lines.clear();
        for line in &scene.lines {
            if line.vertices.len() < 2 {
                continue;
            }
            let bytes = slice_as_bytes(&line.vertices);
            let vbo = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("acxs_line_vbo"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&vbo, 0, bytes);
            self.prepared_lines.push(PreparedLine { vbo, count: line.vertices.len() as u32, strip: line.strip });
        }
    }

    fn paint_pass<'rp>(&'rp self, render_pass: &mut wgpu::RenderPass<'rp>, scene: &SceneFrame) {
        if !scene.items.is_empty() {
            render_pass.set_pipeline(&self.mesh_pipeline);
            render_pass.set_bind_group(0, &self.mesh_bind_group, &[]);
            for item in &scene.items {
                let Some(submeshes) = self.cache.meshes.get(&item.mesh_key) else { continue };
                for sm in submeshes {
                    render_pass.set_vertex_buffer(0, sm.vbo.slice(..));
                    render_pass.set_index_buffer(sm.ibo.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..sm.index_count, 0, 0..1);
                }
            }
        }
        for line in &self.prepared_lines {
            let pipe = if line.strip { &self.line_strip_pipeline } else { &self.line_list_pipeline };
            render_pass.set_pipeline(pipe);
            render_pass.set_bind_group(0, &self.line_bind_group, &[]);
            render_pass.set_vertex_buffer(0, line.vbo.slice(..));
            render_pass.draw(0..line.count, 0..1);
        }
    }
}

struct ViewportCallback {
    renderer: Arc<Mutex<GlRenderer>>,
    scene: SceneFrame,
    aspect: f32,
}

impl CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Ok(mut guard) = self.renderer.lock() {
            guard.prepare_scene(&self.scene, self.aspect);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &CallbackResources,
    ) {
        if let Ok(guard) = self.renderer.lock() {
            // SAFETY: `render_pass` is dropped before `guard` releases the mutex at the end of `paint`.
            let renderer_ref: &'static GlRenderer = unsafe { &*(&*guard as *const GlRenderer) };
            renderer_ref.paint_pass(render_pass, &self.scene);
        }
    }
}

/// Paint a 3D scene into `rect` via `egui_wgpu::Callback` when `gl_renderer` is `Some`,
/// or cleanly invoke `fallback_2d` in headless tests / software contexts.
pub fn paint_viewport_3d(
    ui: &mut egui::Ui,
    rect: Rect,
    gl_renderer: Option<Arc<Mutex<GlRenderer>>>,
    scene: SceneFrame,
    fallback_2d: impl FnOnce(&mut egui::Ui, Rect),
) {
    let Some(renderer) = gl_renderer else {
        fallback_2d(ui, rect);
        return;
    };
    let aspect = (rect.width() / rect.height().max(1.0)).max(0.01);
    let cb = Callback::new_paint_callback(rect, ViewportCallback { renderer, scene, aspect });
    ui.painter().add(cb);
}

fn slice_as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), std::mem::size_of_val(slice)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_matrices_are_finite_and_fallback_runs_headlessly() {
        let orbit = Camera3D::default();
        assert!(orbit.eye_pos().is_finite());
        assert!(orbit.view_matrix().is_finite());
        assert!(orbit.proj_matrix(16.0 / 9.0).is_finite());
        assert!(orbit.proj_matrix(0.0).is_finite());

        let free = Camera3D::FreeFly { pos: Vec3::new(1.0, 2.0, 3.0), yaw: 0.4, pitch: -0.2, speed: 12.0 };
        assert!(free.view_matrix().is_finite());

        let chase = Camera3D::Chase { pos: Vec3::new(0.0, 2.0, -5.0), target: Vec3::ZERO, up: Vec3::Y, fov_deg: 60.0 };
        assert!(chase.view_matrix().is_finite());

        let tv = Camera3D::TracksideTv { pos: Vec3::new(20.0, 8.0, 15.0), target: Vec3::ZERO, fov_deg: 35.0 };
        assert!(tv.proj_matrix(1.5).is_finite());

        let ctx = egui::Context::default();
        let mut fallback_called = false;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 180.0));
            paint_viewport_3d(ui, rect, None, SceneFrame::default(), |_ui, _r| {
                fallback_called = true;
            });
        });
        out.textures_delta.clear();
        assert!(fallback_called);
    }
}
