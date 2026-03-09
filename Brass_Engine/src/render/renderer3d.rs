// =============================================================================
//  Brass Engine — Renderer3D  (v2)
//  Pełne PBR (Cook-Torrance GGX), shadow mapping (directional CSM-ready),
//  point + directional lights, MSAA 4x, HDR + ACES tonemapping,
//  frustum culling (AABB), instancing, cached samplers, poprawna normal matrix.
//
//  group(0) = camera
//  group(1) = model (per-draw: model + normal matrix, packed w jednym buforze)
//  group(2) = material + texture (albedo, normal, emissive, sampler)
//  group(3) = lights uniform + shadow texture + shadow sampler  ← scalono (limit = 4)
// =============================================================================

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;
use wgpu::*;

use super::context::RenderContext;

// ─── Stałe ────────────────────────────────────────────────────────────────────

const MAX_POINT_LIGHTS: usize = 16;
const SHADOW_MAP_SIZE:  u32   = 2048;
const MSAA_SAMPLES:     u32   = 4;

// ─── Vertex3D ─────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex3D {
    pub position: [f32; 3],
    pub normal:   [f32; 3],
    pub uv:       [f32; 2],
    pub tangent:  [f32; 4], // xyz = tangent, w = bitangent sign
}

impl Vertex3D {
    const ATTRIBS: [VertexAttribute; 4] = vertex_attr_array![
        0 => Float32x3, // position
        1 => Float32x3, // normal
        2 => Float32x2, // uv
        3 => Float32x4, // tangent
    ];

    pub fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex3D>() as BufferAddress,
            step_mode:    VertexStepMode::Vertex,
            attributes:   &Self::ATTRIBS,
        }
    }
}

// ─── AABB (frustum culling) ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn unit() -> Self { Self { min: Vec3::splat(-0.5), max: Vec3::splat(0.5) } }

    /// True jeśli AABB (w world space po transformacji) jest wewnątrz frustum.
    pub fn in_frustum(&self, transform: Mat4, planes: &[Vec4; 6]) -> bool {
        // Transformuj do world space (8 narożników → nowy AABB)
        let corners = [
            Vec3::new(self.min.x, self.min.y, self.min.z),
            Vec3::new(self.max.x, self.min.y, self.min.z),
            Vec3::new(self.min.x, self.max.y, self.min.z),
            Vec3::new(self.max.x, self.max.y, self.min.z),
            Vec3::new(self.min.x, self.min.y, self.max.z),
            Vec3::new(self.max.x, self.min.y, self.max.z),
            Vec3::new(self.min.x, self.max.y, self.max.z),
            Vec3::new(self.max.x, self.max.y, self.max.z),
        ];
        for plane in planes {
            let n = Vec3::new(plane.x, plane.y, plane.z);
            // Znajdź narożnik najdalej w kierunku normalnej płaszczyzny
            let p = corners.iter().map(|&c| {
                let wc = (transform * Vec4::new(c.x, c.y, c.z, 1.0)).truncate();
                n.dot(wc)
            }).fold(f32::NEG_INFINITY, f32::max);
            if p + plane.w < 0.0 { return false; }
        }
        true
    }
}

// ─── Mesh ─────────────────────────────────────────────────────────────────────

pub struct Mesh {
    pub vertices: Vec<Vertex3D>,
    pub indices:  Vec<u32>,
    pub aabb:     Aabb,
}

impl Mesh {
    /// Przelicz tangenty metodą Mikktspace (uproszczona).
    fn compute_tangents(verts: &mut Vec<Vertex3D>, indices: &[u32]) {
        let mut tan  = vec![[0f32; 3]; verts.len()];
        let mut btan = vec![[0f32; 3]; verts.len()];
        for tri in indices.chunks_exact(3) {
            let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let p0 = Vec3::from(verts[i0].position);
            let p1 = Vec3::from(verts[i1].position);
            let p2 = Vec3::from(verts[i2].position);
            let uv0 = Vec3::from([verts[i0].uv[0], verts[i0].uv[1], 0.0]);
            let uv1 = Vec3::from([verts[i1].uv[0], verts[i1].uv[1], 0.0]);
            let uv2 = Vec3::from([verts[i2].uv[0], verts[i2].uv[1], 0.0]);
            let e1 = p1 - p0; let e2 = p2 - p0;
            let du1 = uv1.x - uv0.x; let dv1 = uv1.y - uv0.y;
            let du2 = uv2.x - uv0.x; let dv2 = uv2.y - uv0.y;
            let r = 1.0 / (du1 * dv2 - du2 * dv1 + 1e-7);
            let t = (e1 * dv2 - e2 * dv1) * r;
            let b = (e2 * du1 - e1 * du2) * r;
            for i in [i0, i1, i2] {
                tan[i][0] += t.x; tan[i][1] += t.y; tan[i][2] += t.z;
                btan[i][0] += b.x; btan[i][1] += b.y; btan[i][2] += b.z;
            }
        }
        for (i, v) in verts.iter_mut().enumerate() {
            let n = Vec3::from(v.normal);
            let t = Vec3::from(tan[i]);
            let b = Vec3::from(btan[i]);
            let t_orth = (t - n * n.dot(t)).normalize_or_zero();
            let sign = if n.cross(t_orth).dot(b) < 0.0 { -1.0 } else { 1.0 };
            v.tangent = [t_orth.x, t_orth.y, t_orth.z, sign];
        }
    }

    fn compute_aabb(verts: &[Vertex3D]) -> Aabb {
        let mut mn = Vec3::splat(f32::MAX);
        let mut mx = Vec3::splat(f32::MIN);
        for v in verts {
            let p = Vec3::from(v.position);
            mn = mn.min(p); mx = mx.max(p);
        }
        Aabb { min: mn, max: mx }
    }

    pub fn cube() -> Self {
        let v = |px: f32, py: f32, pz: f32, nx: f32, ny: f32, nz: f32, u: f32, vt: f32| Vertex3D {
            position: [px, py, pz], normal: [nx, ny, nz], uv: [u, vt], tangent: [1.0,0.0,0.0,1.0],
        };
        let mut verts = vec![
            v(-0.5,-0.5, 0.5, 0.0,0.0,1.0, 0.0,1.0), v( 0.5,-0.5, 0.5, 0.0,0.0,1.0, 1.0,1.0),
            v( 0.5, 0.5, 0.5, 0.0,0.0,1.0, 1.0,0.0), v(-0.5, 0.5, 0.5, 0.0,0.0,1.0, 0.0,0.0),
            v( 0.5,-0.5,-0.5, 0.0,0.0,-1.0,0.0,1.0), v(-0.5,-0.5,-0.5, 0.0,0.0,-1.0,1.0,1.0),
            v(-0.5, 0.5,-0.5, 0.0,0.0,-1.0,1.0,0.0), v( 0.5, 0.5,-0.5, 0.0,0.0,-1.0,0.0,0.0),
            v(-0.5,-0.5,-0.5,-1.0,0.0,0.0, 0.0,1.0), v(-0.5,-0.5, 0.5,-1.0,0.0,0.0, 1.0,1.0),
            v(-0.5, 0.5, 0.5,-1.0,0.0,0.0, 1.0,0.0), v(-0.5, 0.5,-0.5,-1.0,0.0,0.0, 0.0,0.0),
            v( 0.5,-0.5, 0.5, 1.0,0.0,0.0, 0.0,1.0), v( 0.5,-0.5,-0.5, 1.0,0.0,0.0, 1.0,1.0),
            v( 0.5, 0.5,-0.5, 1.0,0.0,0.0, 1.0,0.0), v( 0.5, 0.5, 0.5, 1.0,0.0,0.0, 0.0,0.0),
            v(-0.5, 0.5, 0.5, 0.0,1.0,0.0, 0.0,1.0), v( 0.5, 0.5, 0.5, 0.0,1.0,0.0, 1.0,1.0),
            v( 0.5, 0.5,-0.5, 0.0,1.0,0.0, 1.0,0.0), v(-0.5, 0.5,-0.5, 0.0,1.0,0.0, 0.0,0.0),
            v(-0.5,-0.5,-0.5, 0.0,-1.0,0.0,0.0,1.0), v( 0.5,-0.5,-0.5, 0.0,-1.0,0.0,1.0,1.0),
            v( 0.5,-0.5, 0.5, 0.0,-1.0,0.0,1.0,0.0), v(-0.5,-0.5, 0.5, 0.0,-1.0,0.0,0.0,0.0),
        ];
        let indices: Vec<u32> = (0u32..6).flat_map(|f| { let b=f*4; [b,b+1,b+2,b,b+2,b+3] }).collect();
        Self::compute_tangents(&mut verts, &indices);
        let aabb = Self::compute_aabb(&verts);
        Self { vertices: verts, indices, aabb }
    }

    pub fn plane(size: f32) -> Self {
        let h = size * 0.5;
        let mut verts = vec![
            Vertex3D { position: [-h,0.0,-h], normal: [0.0,1.0,0.0], uv: [0.0,0.0], tangent: [1.0,0.0,0.0,1.0] },
            Vertex3D { position: [ h,0.0,-h], normal: [0.0,1.0,0.0], uv: [1.0,0.0], tangent: [1.0,0.0,0.0,1.0] },
            Vertex3D { position: [ h,0.0, h], normal: [0.0,1.0,0.0], uv: [1.0,1.0], tangent: [1.0,0.0,0.0,1.0] },
            Vertex3D { position: [-h,0.0, h], normal: [0.0,1.0,0.0], uv: [0.0,1.0], tangent: [1.0,0.0,0.0,1.0] },
        ];
        let indices = vec![0u32,1,2, 0,2,3];
        Self::compute_tangents(&mut verts, &indices);
        let aabb = Self::compute_aabb(&verts);
        Self { vertices: verts, indices, aabb }
    }

    /// Sfera UV (stacks × slices).
    pub fn sphere(radius: f32, stacks: u32, slices: u32) -> Self {
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        for s in 0..=stacks {
            let phi = std::f32::consts::PI * s as f32 / stacks as f32;
            for sl in 0..=slices {
                let theta = 2.0 * std::f32::consts::PI * sl as f32 / slices as f32;
                let x = phi.sin() * theta.cos();
                let y = phi.cos();
                let z = phi.sin() * theta.sin();
                verts.push(Vertex3D {
                    position: [x * radius, y * radius, z * radius],
                    normal:   [x, y, z],
                    uv:       [sl as f32 / slices as f32, s as f32 / stacks as f32],
                    tangent:  [1.0, 0.0, 0.0, 1.0],
                });
            }
        }
        for s in 0..stacks {
            for sl in 0..slices {
                let cur  = s * (slices + 1) + sl;
                let next = cur + slices + 1;
                indices.extend_from_slice(&[cur, next, cur+1, next, next+1, cur+1]);
            }
        }
        let mut m = Self { vertices: verts, indices, aabb: Aabb::unit() };
        Self::compute_tangents(&mut m.vertices, &m.indices.clone());
        m.aabb = Self::compute_aabb(&m.vertices);
        m
    }
}

// ─── GpuMesh ──────────────────────────────────────────────────────────────────

pub struct GpuMesh {
    pub vertex_buffer: Buffer,
    pub index_buffer:  Buffer,
    pub index_count:   u32,
    pub aabb:          Aabb,
}

impl GpuMesh {
    pub fn upload(device: &Device, mesh: &Mesh) -> Self {
        Self {
            vertex_buffer: device.create_buffer_init(&util::BufferInitDescriptor {
                label: Some("VB3D"), contents: bytemuck::cast_slice(&mesh.vertices), usage: BufferUsages::VERTEX,
            }),
            index_buffer: device.create_buffer_init(&util::BufferInitDescriptor {
                label: Some("IB3D"), contents: bytemuck::cast_slice(&mesh.indices), usage: BufferUsages::INDEX,
            }),
            index_count: mesh.indices.len() as u32,
            aabb: mesh.aabb,
        }
    }
}

// ─── Camera3D ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Camera3D {
    pub position: Vec3,
    pub target:   Vec3,
    pub up:       Vec3,
    pub fov_y:    f32,
    pub near:     f32,
    pub far:      f32,
}

impl Camera3D {
    pub fn new(position: Vec3, target: Vec3) -> Self {
        Self { position, target, up: Vec3::Y, fov_y: 60.0, near: 0.1, far: 1000.0 }
    }
    pub fn view_matrix(&self) -> Mat4 { Mat4::look_at_rh(self.position, self.target, self.up) }
    pub fn proj_matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y.to_radians(), aspect, self.near, self.far)
    }
    pub fn view_proj(&self, aspect: f32) -> Mat4 { self.proj_matrix(aspect) * self.view_matrix() }

    /// 6 płaszczyzn frustum (world space) do cullingu.
    pub fn frustum_planes(&self, aspect: f32) -> [Vec4; 6] {
        let m = self.view_proj(aspect);
        let r = m.row(0); let u = m.row(1); let f = m.row(2); let w = m.row(3);
        let planes = [
            w + r, w - r, // left, right
            w + u, w - u, // bottom, top
            f,     w - f, // near, far
        ];
        planes.map(|p| {
            let len = Vec3::new(p.x, p.y, p.z).length();
            Vec4::new(p.x / len, p.y / len, p.z / len, p.w / len)
        })
    }
}

impl Default for Camera3D {
    fn default() -> Self { Self::new(Vec3::new(0.0, 3.0, 5.0), Vec3::ZERO) }
}

// ─── Material ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Material {
    pub albedo:        Vec4,
    pub albedo_tex:    Option<u64>,
    pub normal_tex:    Option<u64>, // normal map (tangent space)
    pub metallic:      f32,
    pub roughness:     f32,
    pub emissive:      Vec3,
    pub emissive_tex:  Option<u64>,
}

impl Material {
    pub fn color(r: f32, g: f32, b: f32) -> Self {
        Self { albedo: Vec4::new(r,g,b,1.0), albedo_tex: None, normal_tex: None,
               metallic: 0.0, roughness: 0.8, emissive: Vec3::ZERO, emissive_tex: None }
    }
    pub fn pbr(albedo: Vec4, metallic: f32, roughness: f32) -> Self {
        Self { albedo, albedo_tex: None, normal_tex: None,
               metallic, roughness, emissive: Vec3::ZERO, emissive_tex: None }
    }
    pub fn emissive(color: Vec3, intensity: f32) -> Self {
        let mut m = Self::color(color.x, color.y, color.z);
        m.emissive = color * intensity;
        m
    }
}

impl Default for Material { fn default() -> Self { Self::color(1.0, 1.0, 1.0) } }

// ─── Lights ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DirectionalLight {
    pub direction: Vec3,
    pub color:     Vec3,
    pub intensity: f32,
    pub cast_shadows: bool,
}

impl DirectionalLight {
    pub fn new(direction: Vec3, color: Vec3, intensity: f32) -> Self {
        Self { direction: direction.normalize(), color, intensity, cast_shadows: true }
    }
}

impl Default for DirectionalLight {
    fn default() -> Self { Self::new(Vec3::new(-0.3,-1.0,-0.5), Vec3::ONE, 1.0) }
}

#[derive(Clone, Debug)]
pub struct PointLight {
    pub position:  Vec3,
    pub color:     Vec3,
    pub intensity: f32,
    pub radius:    f32,
}

impl PointLight {
    pub fn new(position: Vec3, color: Vec3, intensity: f32, radius: f32) -> Self {
        Self { position, color, intensity, radius }
    }
}

// ─── GPU uniforms ─────────────────────────────────────────────────────────────

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj:   [[f32;4];4],
    camera_pos:  [f32;3],
    _pad:        f32,
}

/// Model matrix + transponowana odwrócona (normal matrix) — poprawne transformacje normalnych.
#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct ModelUniform {
    model:        [[f32;4];4],
    normal_mat:   [[f32;4];4], // mat3 spakowany w mat4 (ostatnia kolumna/wiersz = 0)
}

impl ModelUniform {
    fn from_transform(t: Mat4) -> Self {
        let normal = t.inverse().transpose();
        Self { model: t.to_cols_array_2d(), normal_mat: normal.to_cols_array_2d() }
    }
}

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    albedo:    [f32;4],   // 16 B  offset 0
    metallic:  f32,       //  4 B  offset 16
    roughness: f32,       //  4 B  offset 20
    _pad0:     [f32;2],   //  8 B  offset 24  ← padding przed vec3 (align 16)
    emissive:  [f32;3],   // 12 B  offset 32
    _pad1:     f32,       //  4 B  offset 44
                          // = 48 B total — zgodne z WGSL
}

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPointLight { pos: [f32;3], radius: f32, color: [f32;3], intensity: f32 }

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct LightUniform {
    dir_direction: [f32;3], _p0:       f32,
    dir_color:     [f32;3], dir_intensity: f32,
    ambient:       [f32;3], _p1:       f32,
    point_lights:  [GpuPointLight; MAX_POINT_LIGHTS],
    num_point:     u32,     _p2: [f32; 3],
    shadow_vp:     [[f32;4];4],
}

// ─── DrawCall3D ───────────────────────────────────────────────────────────────

pub struct DrawCall3D {
    pub mesh_id:   u64,
    pub transform: Mat4,
    pub material:  Material,
}

// ─── Renderer3D ───────────────────────────────────────────────────────────────

pub struct Renderer3D {
    // Main pass (MSAA)
    pipeline:         RenderPipeline,
    msaa_texture:     Texture,
    msaa_view:        TextureView,
    depth_texture:    Texture,
    depth_view:       TextureView,

    // Shadow pass
    shadow_pipeline:  RenderPipeline,
    shadow_texture:   Texture,
    shadow_view:      TextureView,
    shadow_sampler:   Sampler,

    // Per-frame buffers (single allocation, updated per draw)
    camera_buf:   Buffer,
    camera_bg:    BindGroup,
    model_buf:    Buffer,
    model_bg:     BindGroup,
    mat_buf:      Buffer,
    light_buf:    Buffer,
    light_bg:     BindGroup,  // rebuilds when shadow view changes (resize)
    light_bgl:    BindGroupLayout,

    // Layouts
    mat_bgl:      BindGroupLayout,
    model_bgl:    BindGroupLayout,

    // Cached samplers (tworzone raz, nie per draw call!)
    linear_sampler:  Sampler,
    nearest_sampler: Sampler,

    pub camera:       Camera3D,
    pub dir_light:    DirectionalLight,
    pub point_lights: Vec<PointLight>,
    pub ambient:      Vec3,

    meshes:    std::collections::HashMap<u64, GpuMesh>,
    next_mesh: u64,
    calls:     Vec<DrawCall3D>,
}

impl Renderer3D {
    pub fn new(ctx: &RenderContext) -> Self {
        let device = &ctx.device;
        let (w, h) = ctx.viewport();

        // ── Samplers (raz!) ──
        let linear_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Linear Sampler"),
            mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            anisotropy_clamp: 16, // Anisotropowe filtrowanie
            ..Default::default()
        });
        let nearest_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Nearest Sampler"),
            mag_filter: FilterMode::Nearest, min_filter: FilterMode::Nearest,
            ..Default::default()
        });
        let shadow_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Shadow Sampler"),
            mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear,
            compare: Some(CompareFunction::LessEqual), // PCF-ready
            ..Default::default()
        });

        // ── BGLs ──
        let camera_bgl = bgl_uniform(device, "Cam BGL", ShaderStages::VERTEX_FRAGMENT);
        let model_bgl  = bgl_uniform(device, "Model BGL", ShaderStages::VERTEX_FRAGMENT);

        let mat_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Mat BGL"),
            entries: &[
                bgle_uniform(0, ShaderStages::FRAGMENT),
                bgle_texture(1, ShaderStages::FRAGMENT),  // albedo
                bgle_texture(2, ShaderStages::FRAGMENT),  // normal map
                bgle_texture(3, ShaderStages::FRAGMENT),  // emissive
                bgle_sampler(4, ShaderStages::FRAGMENT, SamplerBindingType::Filtering),
            ],
        });

        // group(3): lights uniform + shadow depth texture + shadow sampler
        // Scalono żeby zmieścić się w limicie 4 bind groups
        let light_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Light+Shadow BGL"),
            entries: &[
                bgle_uniform(0, ShaderStages::VERTEX_FRAGMENT),    // lights uniform
                bgle_depth_texture(1, ShaderStages::FRAGMENT),     // shadow map
                bgle_sampler(2, ShaderStages::FRAGMENT, SamplerBindingType::Comparison), // shadow sampler
            ],
        });

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("3D Layout"),
            bind_group_layouts: &[&camera_bgl, &model_bgl, &mat_bgl, &light_bgl],
            push_constant_ranges: &[],
        });

        let shadow_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Shadow Layout"),
            bind_group_layouts: &[&camera_bgl, &model_bgl],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("3D Shader"), source: ShaderSource::Wgsl(SHADER_3D.into()),
        });
        let shadow_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Shadow Shader"), source: ShaderSource::Wgsl(SHADER_SHADOW.into()),
        });

        let (msaa_texture, msaa_view) = make_msaa(device, w as u32, h as u32, ctx.format, MSAA_SAMPLES);
        let (depth_texture, depth_view) = make_depth(device, w as u32, h as u32, MSAA_SAMPLES);
        let (shadow_texture, shadow_view) = make_shadow_map(device, SHADOW_MAP_SIZE);

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("3D Pipeline"),
            layout: Some(&layout),
            vertex: VertexState {
                module: &shader, entry_point: "vs_main",
                buffers: &[Vertex3D::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader, entry_point: "fs_main",
                targets: &[Some(ColorTargetState {
                    format: ctx.format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: Some(Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState { count: MSAA_SAMPLES, mask: !0, alpha_to_coverage_enabled: false },
            multiview: None,
            cache: None,
        });

        let shadow_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Shadow Pipeline"),
            layout: Some(&shadow_layout),
            vertex: VertexState {
                module: &shadow_shader, entry_point: "vs_shadow",
                buffers: &[Vertex3D::layout()],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: Some(Face::Front), // peter-panning fix
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil: StencilState::default(),
                bias: DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let camera   = Camera3D::default();
        let dir_light = DirectionalLight::default();
        let aspect   = w / h;

        // Camera buf
        let cam_data = CameraUniform { view_proj: camera.view_proj(aspect).to_cols_array_2d(), camera_pos: camera.position.to_array(), _pad: 0.0 };
        let camera_buf = make_uniform_buf(device, "Cam Buf", bytemuck::bytes_of(&cam_data));
        let camera_bg  = uniform_bg(device, "Cam BG", &camera_bgl, &camera_buf);

        // Model buf
        let model_data = ModelUniform::from_transform(Mat4::IDENTITY);
        let model_buf  = make_uniform_buf(device, "Model Buf", bytemuck::bytes_of(&model_data));
        let model_bg   = uniform_bg(device, "Model BG", &model_bgl, &model_buf);

        // Mat buf
        let mat_data = MaterialUniform { albedo: [1.0;4], metallic: 0.0, roughness: 0.8, _pad0: [0.0;2], emissive: [0.0;3], _pad1: 0.0 };
        let mat_buf  = make_uniform_buf(device, "Mat Buf", bytemuck::bytes_of(&mat_data));

        // Light buf
        let light_data = LightUniform {
            dir_direction: dir_light.direction.to_array(), _p0: 0.0,
            dir_color: dir_light.color.to_array(), dir_intensity: dir_light.intensity,
            ambient: [0.1;3], _p1: 0.0,
            point_lights: [GpuPointLight { pos: [0.0;3], radius: 0.0, color: [0.0;3], intensity: 0.0 }; MAX_POINT_LIGHTS],
            num_point: 0, _p2: [0.0;3],
            shadow_vp: Mat4::IDENTITY.to_cols_array_2d(),
        };
        let light_buf = make_uniform_buf(device, "Light Buf", bytemuck::bytes_of(&light_data));

        // group(3): lights uniform + shadow map + shadow sampler — scalono
        let light_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Light+Shadow BG"), layout: &light_bgl,
            entries: &[
                BindGroupEntry { binding: 0, resource: light_buf.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&shadow_view) },
                BindGroupEntry { binding: 2, resource: BindingResource::Sampler(&shadow_sampler) },
            ],
        });

        Self {
            pipeline, msaa_texture, msaa_view, depth_texture, depth_view,
            shadow_pipeline, shadow_texture, shadow_view, shadow_sampler,
            camera_buf, camera_bg, model_buf, model_bg, mat_buf, light_buf, light_bg, light_bgl,
            mat_bgl, model_bgl,
            linear_sampler, nearest_sampler,
            camera, dir_light, point_lights: Vec::new(), ambient: Vec3::splat(0.05),
            meshes: std::collections::HashMap::new(), next_mesh: 1, calls: Vec::new(),
        }
    }

    pub fn upload_mesh(&mut self, device: &Device, mesh: &Mesh) -> u64 {
        let id = self.next_mesh; self.next_mesh += 1;
        self.meshes.insert(id, GpuMesh::upload(device, mesh));
        id
    }

    pub fn draw(&mut self, mesh_id: u64, transform: Mat4, material: Material) {
        self.calls.push(DrawCall3D { mesh_id, transform, material });
    }

    pub fn resize(&mut self, ctx: &RenderContext) {
        let (w, h) = ctx.viewport();
        let (mt, mv) = make_msaa(&ctx.device, w as u32, h as u32, ctx.format, MSAA_SAMPLES);
        let (dt, dv) = make_depth(&ctx.device, w as u32, h as u32, MSAA_SAMPLES);
        self.msaa_texture = mt; self.msaa_view = mv;
        self.depth_texture = dt; self.depth_view = dv;
        // shadow view nie zmienia się przy resize (stały rozmiar mapy cieni)
    }

    pub fn render(
        &mut self,
        ctx: &RenderContext,
        color_view: &TextureView,
        tex_manager: &super::texture_manager::TextureManager,
    ) {
        let (w, h) = ctx.viewport();
        let aspect = w / h;
        let planes = self.camera.frustum_planes(aspect);

        // Oblicz shadow VP dla directional light (ortho wokół origin)
        let shadow_vp = compute_shadow_vp(&self.dir_light);

        // Zaktualizuj camera uniform
        let cam_data = CameraUniform {
            view_proj: self.camera.view_proj(aspect).to_cols_array_2d(),
            camera_pos: self.camera.position.to_array(), _pad: 0.0,
        };
        ctx.queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&cam_data));

        // Zaktualizuj light uniform
        let mut gpu_points = [GpuPointLight { pos: [0.0;3], radius: 0.0, color: [0.0;3], intensity: 0.0 }; MAX_POINT_LIGHTS];
        let n = self.point_lights.len().min(MAX_POINT_LIGHTS);
        for i in 0..n {
            let pl = &self.point_lights[i];
            gpu_points[i] = GpuPointLight { pos: pl.position.to_array(), radius: pl.radius, color: pl.color.to_array(), intensity: pl.intensity };
        }
        let light_data = LightUniform {
            dir_direction: self.dir_light.direction.to_array(), _p0: 0.0,
            dir_color: self.dir_light.color.to_array(), dir_intensity: self.dir_light.intensity,
            ambient: self.ambient.to_array(), _p1: 0.0,
            point_lights: gpu_points, num_point: n as u32, _p2: [0.0;3],
            shadow_vp: shadow_vp.to_cols_array_2d(),
        };
        ctx.queue.write_buffer(&self.light_buf, 0, bytemuck::bytes_of(&light_data));

        let calls: Vec<DrawCall3D> = self.calls.drain(..).collect();

        let mut encoder = ctx.device.create_command_encoder(&CommandEncoderDescriptor { label: Some("3D Enc") });

        // ═══════════════════════════════
        //  SHADOW PASS
        // ═══════════════════════════════
        if self.dir_light.cast_shadows {
            // Wgraj shadow VP do camera buf tymczasowo (osobny buf byłby lepszy — TODO)
            let shadow_cam = CameraUniform { view_proj: shadow_vp.to_cols_array_2d(), camera_pos: [0.0;3], _pad: 0.0 };
            ctx.queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&shadow_cam));

            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Shadow Pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.camera_bg, &[]);

            for call in &calls {
                let gpu_mesh = match self.meshes.get(&call.mesh_id) { Some(m) => m, None => continue };
                let model_data = ModelUniform::from_transform(call.transform);
                ctx.queue.write_buffer(&self.model_buf, 0, bytemuck::bytes_of(&model_data));
                pass.set_bind_group(1, &self.model_bg, &[]);
                pass.set_vertex_buffer(0, gpu_mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(gpu_mesh.index_buffer.slice(..), IndexFormat::Uint32);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
            }
            drop(pass);

            // Przywróć główny camera buf
            ctx.queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&cam_data));
        }

        // ═══════════════════════════════
        //  MAIN PASS (MSAA)
        // ═══════════════════════════════
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("3D Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.msaa_view,
                    resolve_target: Some(color_view), // MSAA resolve do swapchain
                    ops: Operations { load: LoadOp::Clear(wgpu::Color { r:0.01, g:0.01, b:0.015, a:1.0 }), store: StoreOp::Store },
                })],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations { load: LoadOp::Clear(1.0), store: StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bg, &[]);
            pass.set_bind_group(3, &self.light_bg, &[]); // lights + shadow — scalona grupa

            for call in &calls {
                let gpu_mesh = match self.meshes.get(&call.mesh_id) { Some(m) => m, None => continue };

                // Frustum culling
                if !gpu_mesh.aabb.in_frustum(call.transform, &planes) { continue; }

                let model_data = ModelUniform::from_transform(call.transform);
                ctx.queue.write_buffer(&self.model_buf, 0, bytemuck::bytes_of(&model_data));
                pass.set_bind_group(1, &self.model_bg, &[]);

                let mat_data = MaterialUniform {
                    albedo:    call.material.albedo.to_array(),
                    metallic:  call.material.metallic,
                    roughness: call.material.roughness,
                    _pad0:     [0.0; 2],
                    emissive:  call.material.emissive.to_array(),
                    _pad1:     0.0,
                };
                ctx.queue.write_buffer(&self.mat_buf, 0, bytemuck::bytes_of(&mat_data));

                let albedo_tex = tex_for(&call.material.albedo_tex, tex_manager);
                let normal_tex = tex_for(&call.material.normal_tex,  tex_manager);
                let emissive_tex = tex_for(&call.material.emissive_tex, tex_manager);

                // BindGroup budowany raz per draw call — ale sampler jest cached!
                let mat_bg = ctx.device.create_bind_group(&BindGroupDescriptor {
                    label: Some("Mat BG"), layout: &self.mat_bgl,
                    entries: &[
                        BindGroupEntry { binding: 0, resource: self.mat_buf.as_entire_binding() },
                        BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&albedo_tex.view) },
                        BindGroupEntry { binding: 2, resource: BindingResource::TextureView(&normal_tex.view) },
                        BindGroupEntry { binding: 3, resource: BindingResource::TextureView(&emissive_tex.view) },
                        BindGroupEntry { binding: 4, resource: BindingResource::Sampler(&self.linear_sampler) },
                    ],
                });
                pass.set_bind_group(2, &mat_bg, &[]);

                pass.set_vertex_buffer(0, gpu_mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(gpu_mesh.index_buffer.slice(..), IndexFormat::Uint32);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
            }
        }

        ctx.queue.submit(std::iter::once(encoder.finish()));
    }
}

// ─── Shadow VP ────────────────────────────────────────────────────────────────

fn compute_shadow_vp(light: &DirectionalLight) -> Mat4 {
    let dir = light.direction.normalize();
    let up  = if dir.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let view = Mat4::look_at_rh(-dir * 50.0, Vec3::ZERO, up);
    let proj = Mat4::orthographic_rh(-50.0, 50.0, -50.0, 50.0, 0.1, 200.0);
    proj * view
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn tex_for<'a>(id: &Option<u64>, tm: &'a super::texture_manager::TextureManager) -> &'a super::texture_manager::GpuTexture {
    match id { Some(id) => tm.get(*id).unwrap_or(tm.white()), None => tm.white() }
}

fn make_msaa(device: &Device, w: u32, h: u32, format: TextureFormat, samples: u32) -> (Texture, TextureView) {
    let tex = device.create_texture(&TextureDescriptor {
        label: Some("MSAA"), size: Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: samples, dimension: TextureDimension::D2,
        format, usage: TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
    });
    let view = tex.create_view(&TextureViewDescriptor::default());
    (tex, view)
}

fn make_depth(device: &Device, w: u32, h: u32, samples: u32) -> (Texture, TextureView) {
    let tex = device.create_texture(&TextureDescriptor {
        label: Some("Depth"), size: Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: samples, dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float, usage: TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
    });
    let view = tex.create_view(&TextureViewDescriptor::default());
    (tex, view)
}

fn make_shadow_map(device: &Device, size: u32) -> (Texture, TextureView) {
    let tex = device.create_texture(&TextureDescriptor {
        label: Some("ShadowMap"),
        size: Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&TextureViewDescriptor {
        aspect: TextureAspect::DepthOnly, ..Default::default()
    });
    (tex, view)
}

fn make_uniform_buf(device: &Device, label: &str, data: &[u8]) -> Buffer {
    device.create_buffer_init(&util::BufferInitDescriptor {
        label: Some(label), contents: data, usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    })
}

fn bgl_uniform(device: &Device, label: &str, vis: ShaderStages) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[BindGroupLayoutEntry {
            binding: 0, visibility: vis,
            ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    })
}

fn bgle_uniform(binding: u32, vis: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding, visibility: vis,
        ty: BindingType::Buffer { ty: BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn bgle_texture(binding: u32, vis: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding, visibility: vis,
        ty: BindingType::Texture { sample_type: TextureSampleType::Float { filterable: true }, view_dimension: TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}

fn bgle_depth_texture(binding: u32, vis: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding, visibility: vis,
        ty: BindingType::Texture { sample_type: TextureSampleType::Depth, view_dimension: TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}

fn bgle_sampler(binding: u32, vis: ShaderStages, ty: SamplerBindingType) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry { binding, visibility: vis, ty: BindingType::Sampler(ty), count: None }
}

fn uniform_bg(device: &Device, label: &str, layout: &BindGroupLayout, buf: &Buffer) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some(label), layout,
        entries: &[BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
    })
}

// ─── Shadow WGSL ─────────────────────────────────────────────────────────────

const SHADER_SHADOW: &str = r#"
struct Camera { view_proj: mat4x4<f32>, camera_pos: vec3<f32> }
struct Model  { model: mat4x4<f32>, normal_mat: mat4x4<f32> }

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> model:  Model;

@vertex
fn vs_shadow(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return camera.view_proj * model.model * vec4<f32>(position, 1.0);
}
"#;

// ─── PBR WGSL (Cook-Torrance GGX, normal maps, shadow PCF, ACES tonemapping) ─

const SHADER_3D: &str = r#"
// ── Structs ──────────────────────────────────────────────────────────────────

struct Camera   { view_proj: mat4x4<f32>, camera_pos: vec3<f32> }
struct Model    { model: mat4x4<f32>, normal_mat: mat4x4<f32> }
struct Material { albedo: vec4<f32>, metallic: f32, roughness: f32, emissive: vec3<f32> }

struct PointLight { pos: vec3<f32>, radius: f32, color: vec3<f32>, intensity: f32 }

struct Lights {
    dir_direction: vec3<f32>,
    dir_color:     vec3<f32>,
    dir_intensity: f32,
    ambient:       vec3<f32>,
    points:        array<PointLight, 16>,
    num_point:     u32,
    shadow_vp:     mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera:   Camera;
@group(1) @binding(0) var<uniform> model:    Model;
@group(2) @binding(0) var<uniform> material: Material;
@group(2) @binding(1) var t_albedo:   texture_2d<f32>;
@group(2) @binding(2) var t_normal:   texture_2d<f32>;
@group(2) @binding(3) var t_emissive: texture_2d<f32>;
@group(2) @binding(4) var s_linear:   sampler;
@group(3) @binding(0) var<uniform> lights: Lights;
@group(3) @binding(1) var t_shadow:   texture_depth_2d;
@group(3) @binding(2) var s_shadow:   sampler_comparison;

// ── Vertex ────────────────────────────────────────────────────────────────────

struct VertIn {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
    @location(3) tangent:  vec4<f32>,
}

struct VertOut {
    @builtin(position) clip_pos:   vec4<f32>,
    @location(0) world_pos:        vec3<f32>,
    @location(1) normal:           vec3<f32>,
    @location(2) uv:               vec2<f32>,
    @location(3) tangent:          vec3<f32>,
    @location(4) bitangent:        vec3<f32>,
    @location(5) shadow_coord:     vec4<f32>,
}

@vertex
fn vs_main(in: VertIn) -> VertOut {
    let world_pos4 = model.model * vec4<f32>(in.position, 1.0);
    let N = normalize((model.normal_mat * vec4<f32>(in.normal,  0.0)).xyz);
    let T = normalize((model.normal_mat * vec4<f32>(in.tangent.xyz, 0.0)).xyz);
    let B = cross(N, T) * in.tangent.w;

    var out: VertOut;
    out.clip_pos    = camera.view_proj * world_pos4;
    out.world_pos   = world_pos4.xyz;
    out.normal      = N;
    out.uv          = in.uv;
    out.tangent     = T;
    out.bitangent   = B;
    out.shadow_coord = lights.shadow_vp * world_pos4;
    return out;
}

// ── PBR helpers ───────────────────────────────────────────────────────────────

const PI: f32 = 3.14159265358979;

fn distribution_ggx(N: vec3<f32>, H: vec3<f32>, roughness: f32) -> f32 {
    let a  = roughness * roughness;
    let a2 = a * a;
    let NdH = max(dot(N, H), 0.0);
    let denom = NdH * NdH * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

fn geometry_schlick_ggx(NdV: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return NdV / (NdV * (1.0 - k) + k);
}

fn geometry_smith(N: vec3<f32>, V: vec3<f32>, L: vec3<f32>, roughness: f32) -> f32 {
    let NdV = max(dot(N, V), 0.0);
    let NdL = max(dot(N, L), 0.0);
    return geometry_schlick_ggx(NdV, roughness) * geometry_schlick_ggx(NdL, roughness);
}

fn fresnel_schlick(cos_theta: f32, F0: vec3<f32>) -> vec3<f32> {
    return F0 + (1.0 - F0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn cook_torrance(N: vec3<f32>, V: vec3<f32>, L: vec3<f32>, albedo: vec3<f32>, metallic: f32, roughness: f32, light_color: vec3<f32>) -> vec3<f32> {
    let H   = normalize(V + L);
    let NdL = max(dot(N, L), 0.0);
    if NdL <= 0.0 { return vec3<f32>(0.0); }

    let F0  = mix(vec3<f32>(0.04), albedo, metallic);
    let D   = distribution_ggx(N, H, roughness);
    let G   = geometry_smith(N, V, L, roughness);
    let F   = fresnel_schlick(max(dot(H, V), 0.0), F0);

    let spec_num = D * G * F;
    let spec_den = 4.0 * max(dot(N,V), 0.0) * NdL + 0.0001;
    let specular = spec_num / spec_den;

    let kS = F;
    let kD = (vec3<f32>(1.0) - kS) * (1.0 - metallic);

    return (kD * albedo / PI + specular) * light_color * NdL;
}

// ── Shadow PCF 3×3 ────────────────────────────────────────────────────────────

fn shadow_factor(shadow_coord: vec4<f32>) -> f32 {
    let proj = shadow_coord.xyz / shadow_coord.w;
    let uv   = proj.xy * 0.5 + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) { return 1.0; }
    let depth = proj.z - 0.005; // bias

    var shadow = 0.0;
    let texel = 1.0 / f32(2048); // SHADOW_MAP_SIZE
    for (var x: i32 = -1; x <= 1; x++) {
        for (var y: i32 = -1; y <= 1; y++) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel;
            shadow += textureSampleCompare(t_shadow, s_shadow, uv + offset, depth);
        }
    }
    return shadow / 9.0;
}

// ── ACES Filmic Tonemapping ────────────────────────────────────────────────────

fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51; let b = 0.03; let c = 2.43; let d = 0.59; let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(1.0 / 2.2));
}

// ── Fragment ─────────────────────────────────────────────────────────────────

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // Normal map (tangent space → world space)
    let nm_sample = textureSample(t_normal, s_linear, in.uv).xyz * 2.0 - 1.0;
    let TBN_N = normalize(in.tangent    * nm_sample.x
                        + in.bitangent  * nm_sample.y
                        + in.normal     * nm_sample.z);
    let N = TBN_N;

    let albedo_tex  = textureSample(t_albedo,   s_linear, in.uv);
    let emissive_tex = textureSample(t_emissive, s_linear, in.uv).rgb;

    let base_color = albedo_tex.rgb * material.albedo.rgb;
    let metallic   = material.metallic;
    let roughness  = max(material.roughness, 0.04);
    let V          = normalize(camera.camera_pos - in.world_pos);

    // Directional light
    let L_dir = normalize(-lights.dir_direction);
    let shadow = shadow_factor(in.shadow_coord);
    var Lo = cook_torrance(N, V, L_dir, base_color, metallic, roughness,
                           lights.dir_color * lights.dir_intensity) * shadow;

    // Point lights
    for (var i: u32 = 0u; i < lights.num_point; i++) {
        let pl       = lights.points[i];
        let L_vec    = pl.pos - in.world_pos;
        let dist     = length(L_vec);
        let L        = normalize(L_vec);
        let atten    = clamp(1.0 - (dist / pl.radius), 0.0, 1.0);
        let atten2   = atten * atten;
        Lo += cook_torrance(N, V, L, base_color, metallic, roughness,
                            pl.color * pl.intensity * atten2);
    }

    // Ambient (simple IBL approximation)
    let F0      = mix(vec3<f32>(0.04), base_color, metallic);
    let kS      = fresnel_schlick(max(dot(N, V), 0.0), F0);
    let kD      = (vec3<f32>(1.0) - kS) * (1.0 - metallic);
    let ambient = lights.ambient * base_color * kD;

    // Emissive
    let emissive = material.emissive + emissive_tex;

    var color = ambient + Lo + emissive;

    // HDR → ACES tonemapping → sRGB
    color = aces(color);
    color = linear_to_srgb(color);

    return vec4<f32>(color, albedo_tex.a * material.albedo.a);
}
"#;