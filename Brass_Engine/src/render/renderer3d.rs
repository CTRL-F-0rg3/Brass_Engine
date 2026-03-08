// =============================================================================
//  Brass Engine — Renderer3D
//  group(0) = camera, group(1) = model, group(2) = material+texture, group(3) = light
// =============================================================================

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;
use wgpu::*;

use super::context::RenderContext;

// ─── Vertex3D ─────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex3D {
    pub position: [f32; 3],
    pub normal:   [f32; 3],
    pub uv:       [f32; 2],
}

impl Vertex3D {
    const ATTRIBS: [VertexAttribute; 3] = vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
    ];

    pub fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex3D>() as BufferAddress,
            step_mode:    VertexStepMode::Vertex,
            attributes:   &Self::ATTRIBS,
        }
    }
}

// ─── Mesh ─────────────────────────────────────────────────────────────────────

pub struct Mesh {
    pub vertices: Vec<Vertex3D>,
    pub indices:  Vec<u32>,
}

impl Mesh {
    pub fn cube() -> Self {
        let v = |px: f32, py: f32, pz: f32, nx: f32, ny: f32, nz: f32, u: f32, vt: f32| Vertex3D {
            position: [px, py, pz], normal: [nx, ny, nz], uv: [u, vt],
        };
        Self {
            vertices: vec![
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
            ],
            indices: (0u32..6).flat_map(|f| { let b=f*4; [b,b+1,b+2,b,b+2,b+3] }).collect(),
        }
    }

    pub fn plane(size: f32) -> Self {
        let h = size * 0.5;
        Self {
            vertices: vec![
                Vertex3D { position: [-h,0.0,-h], normal: [0.0,1.0,0.0], uv: [0.0,0.0] },
                Vertex3D { position: [ h,0.0,-h], normal: [0.0,1.0,0.0], uv: [1.0,0.0] },
                Vertex3D { position: [ h,0.0, h], normal: [0.0,1.0,0.0], uv: [1.0,1.0] },
                Vertex3D { position: [-h,0.0, h], normal: [0.0,1.0,0.0], uv: [0.0,1.0] },
            ],
            indices: vec![0,1,2, 0,2,3],
        }
    }
}

// ─── GpuMesh ──────────────────────────────────────────────────────────────────

pub struct GpuMesh {
    pub vertex_buffer: Buffer,
    pub index_buffer:  Buffer,
    pub index_count:   u32,
}

impl GpuMesh {
    pub fn upload(device: &Device, mesh: &Mesh) -> Self {
        Self {
            vertex_buffer: device.create_buffer_init(&util::BufferInitDescriptor {
                label: Some("3D VB"), contents: bytemuck::cast_slice(&mesh.vertices), usage: BufferUsages::VERTEX,
            }),
            index_buffer: device.create_buffer_init(&util::BufferInitDescriptor {
                label: Some("3D IB"), contents: bytemuck::cast_slice(&mesh.indices), usage: BufferUsages::INDEX,
            }),
            index_count: mesh.indices.len() as u32,
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
}

impl Default for Camera3D {
    fn default() -> Self { Self::new(Vec3::new(0.0, 3.0, 5.0), Vec3::ZERO) }
}

// ─── Material ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Material {
    pub albedo:     Vec4,
    pub texture_id: Option<u64>,
    pub metallic:   f32,
    pub roughness:  f32,
}

impl Material {
    pub fn color(r: f32, g: f32, b: f32) -> Self {
        Self { albedo: Vec4::new(r,g,b,1.0), texture_id: None, metallic: 0.0, roughness: 0.8 }
    }
    pub fn textured(texture_id: u64) -> Self {
        Self { albedo: Vec4::ONE, texture_id: Some(texture_id), metallic: 0.0, roughness: 0.8 }
    }
}

impl Default for Material {
    fn default() -> Self { Self::color(1.0, 1.0, 1.0) }
}

// ─── DirectionalLight ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DirectionalLight {
    pub direction: Vec3,
    pub color:     Vec3,
    pub intensity: f32,
}

impl DirectionalLight {
    pub fn new(direction: Vec3, color: Vec3, intensity: f32) -> Self {
        Self { direction: direction.normalize(), color, intensity }
    }
}

impl Default for DirectionalLight {
    fn default() -> Self { Self::new(Vec3::new(-0.3,-1.0,-0.5), Vec3::ONE, 1.0) }
}

// ─── GPU uniforms ─────────────────────────────────────────────────────────────

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform3D { view_proj: [[f32;4];4], camera_pos: [f32;3], _pad: f32 }

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct ModelUniform { model: [[f32;4];4] }

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform { albedo: [f32;4], metallic: f32, roughness: f32, _pad: [f32;2] }

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct LightUniform { direction: [f32;3], _p0: f32, color: [f32;3], intensity: f32, ambient: [f32;3], _p1: f32 }

// ─── DrawCall3D ───────────────────────────────────────────────────────────────

pub struct DrawCall3D {
    pub mesh_id:  u64,
    pub transform: Mat4,
    pub material:  Material,
}

// ─── Renderer3D ───────────────────────────────────────────────────────────────

pub struct Renderer3D {
    pipeline:      RenderPipeline,
    depth_texture: Texture,
    depth_view:    TextureView,

    camera_buf: Buffer,
    camera_bg:  BindGroup,

    model_buf:  Buffer,
    model_bg:   BindGroup,

    // group(2): material uniform + texture + sampler — wszystko razem
    mat_bgl:    BindGroupLayout,
    mat_buf:    Buffer,

    light_buf:  Buffer,
    light_bg:   BindGroup,

    pub camera: Camera3D,
    pub light:  DirectionalLight,

    meshes:    std::collections::HashMap<u64, GpuMesh>,
    next_mesh: u64,
    calls:     Vec<DrawCall3D>,
}

impl Renderer3D {
    pub fn new(ctx: &RenderContext) -> Self {
        let device = &ctx.device;
        let (w, h) = ctx.viewport();

        // group(0) camera
        let camera_bgl = bgl_uniform(device, "Cam BGL", ShaderStages::VERTEX_FRAGMENT);
        // group(1) model
        let model_bgl  = bgl_uniform(device, "Model BGL", ShaderStages::VERTEX);
        // group(2) material uniform + texture + sampler
        let mat_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Mat+Tex BGL"),
            entries: &[
                // binding 0 — material uniform
                BindGroupLayoutEntry {
                    binding: 0, visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // binding 1 — texture
                BindGroupLayoutEntry {
                    binding: 1, visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 2 — sampler
                BindGroupLayoutEntry {
                    binding: 2, visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // group(3) light
        let light_bgl = bgl_uniform(device, "Light BGL", ShaderStages::FRAGMENT);

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("3D Layout"),
            bind_group_layouts: &[&camera_bgl, &model_bgl, &mat_bgl, &light_bgl],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("3D Shader"),
            source: ShaderSource::Wgsl(SHADER_3D.into()),
        });

        let (depth_texture, depth_view) = make_depth(device, w as u32, h as u32);

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
            primitive: PrimitiveState { topology: PrimitiveTopology::TriangleList, cull_mode: Some(Face::Back), ..Default::default() },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Less,
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let camera = Camera3D::default();
        let light  = DirectionalLight::default();
        let aspect = w / h;

        let cam_data = CameraUniform3D { view_proj: camera.view_proj(aspect).to_cols_array_2d(), camera_pos: camera.position.to_array(), _pad: 0.0 };
        let camera_buf = device.create_buffer_init(&util::BufferInitDescriptor { label: Some("Cam Buf"), contents: bytemuck::bytes_of(&cam_data), usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST });
        let camera_bg  = uniform_bg(device, "Cam BG", &camera_bgl, &camera_buf);

        let model_data = ModelUniform { model: Mat4::IDENTITY.to_cols_array_2d() };
        let model_buf  = device.create_buffer_init(&util::BufferInitDescriptor { label: Some("Model Buf"), contents: bytemuck::bytes_of(&model_data), usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST });
        let model_bg   = uniform_bg(device, "Model BG", &model_bgl, &model_buf);

        let mat_data = MaterialUniform { albedo: [1.0;4], metallic: 0.0, roughness: 0.8, _pad: [0.0;2] };
        let mat_buf  = device.create_buffer_init(&util::BufferInitDescriptor { label: Some("Mat Buf"), contents: bytemuck::bytes_of(&mat_data), usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST });

        let light_data = LightUniform { direction: light.direction.to_array(), _p0: 0.0, color: (light.color * light.intensity).to_array(), intensity: light.intensity, ambient: [0.1;3], _p1: 0.0 };
        let light_buf  = device.create_buffer_init(&util::BufferInitDescriptor { label: Some("Light Buf"), contents: bytemuck::bytes_of(&light_data), usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST });
        let light_bg   = uniform_bg(device, "Light BG", &light_bgl, &light_buf);

        Self {
            pipeline, depth_texture, depth_view,
            camera_buf, camera_bg,
            model_buf, model_bg,
            mat_bgl, mat_buf,
            light_buf, light_bg,
            camera, light,
            meshes: std::collections::HashMap::new(),
            next_mesh: 1,
            calls: Vec::new(),
        }
    }

    pub fn upload_mesh(&mut self, ctx: &RenderContext, mesh: &Mesh) -> u64 {
        let id = self.next_mesh; self.next_mesh += 1;
        self.meshes.insert(id, GpuMesh::upload(&ctx.device, mesh));
        id
    }

    /// Uploaduj mesh mając tylko &wgpu::Device — przydatne w on_start.
    pub fn upload_mesh_device(&mut self, device: &wgpu::Device, mesh: &Mesh) -> u64 {
        let id = self.next_mesh; self.next_mesh += 1;
        self.meshes.insert(id, GpuMesh::upload(device, mesh));
        id
    }

    pub fn draw_mesh(&mut self, mesh_id: u64, transform: Mat4, material: Material) {
        self.calls.push(DrawCall3D { mesh_id, transform, material });
    }

    pub fn resize(&mut self, ctx: &RenderContext) {
        let (w, h) = ctx.viewport();
        let (dt, dv) = make_depth(&ctx.device, w as u32, h as u32);
        self.depth_texture = dt;
        self.depth_view    = dv;
    }

    pub fn render(&mut self, ctx: &RenderContext, color_view: &TextureView, tex_manager: &super::texture_manager::TextureManager) {
        let (w, h) = ctx.viewport();
        let aspect = w / h;

        let cam_data = CameraUniform3D { view_proj: self.camera.view_proj(aspect).to_cols_array_2d(), camera_pos: self.camera.position.to_array(), _pad: 0.0 };
        ctx.queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&cam_data));

        let light_data = LightUniform { direction: self.light.direction.to_array(), _p0: 0.0, color: (self.light.color * self.light.intensity).to_array(), intensity: self.light.intensity, ambient: [0.1;3], _p1: 0.0 };
        ctx.queue.write_buffer(&self.light_buf, 0, bytemuck::bytes_of(&light_data));

        let mut encoder = ctx.device.create_command_encoder(&CommandEncoderDescriptor { label: Some("3D Encoder") });
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("3D Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: color_view, resolve_target: None,
                    ops: Operations { load: LoadOp::Clear(wgpu::Color { r:0.04, g:0.04, b:0.07, a:1.0 }), store: StoreOp::Store },
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
            pass.set_bind_group(3, &self.light_bg, &[]);

            let calls: Vec<DrawCall3D> = self.calls.drain(..).collect();
            for call in &calls {
                let gpu_mesh = match self.meshes.get(&call.mesh_id) { Some(m) => m, None => continue };

                let model_data = ModelUniform { model: call.transform.to_cols_array_2d() };
                ctx.queue.write_buffer(&self.model_buf, 0, bytemuck::bytes_of(&model_data));
                pass.set_bind_group(1, &self.model_bg, &[]);

                let mat_data = MaterialUniform { albedo: call.material.albedo.to_array(), metallic: call.material.metallic, roughness: call.material.roughness, _pad: [0.0;2] };
                ctx.queue.write_buffer(&self.mat_buf, 0, bytemuck::bytes_of(&mat_data));

                // Zbuduj bind group dla materiału + tekstury razem
                let tex = match call.material.texture_id {
                    Some(id) => tex_manager.get(id).unwrap_or(tex_manager.white()),
                    None     => tex_manager.white(),
                };
                let mat_bg = ctx.device.create_bind_group(&BindGroupDescriptor {
                    label: Some("Mat+Tex BG"),
                    layout: &self.mat_bgl,
                    entries: &[
                        BindGroupEntry { binding: 0, resource: self.mat_buf.as_entire_binding() },
                        BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&tex.view) },
                        BindGroupEntry { binding: 2, resource: BindingResource::Sampler(
                            &ctx.device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..Default::default() })
                        )},
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

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_depth(device: &Device, w: u32, h: u32) -> (Texture, TextureView) {
    let tex = device.create_texture(&TextureDescriptor {
        label: Some("Depth"), size: Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float, usage: TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
    });
    let view = tex.create_view(&TextureViewDescriptor::default());
    (tex, view)
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

fn uniform_bg(device: &Device, label: &str, layout: &BindGroupLayout, buf: &Buffer) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some(label), layout,
        entries: &[BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
    })
}

// ─── WGSL Shader — 4 grupy (0–3) ─────────────────────────────────────────────

const SHADER_3D: &str = r#"
struct Camera   { view_proj: mat4x4<f32>, camera_pos: vec3<f32> }
struct Model    { model: mat4x4<f32> }
struct Material { albedo: vec4<f32>, metallic: f32, roughness: f32 }
struct Light    { direction: vec3<f32>, color: vec3<f32>, intensity: f32, ambient: vec3<f32> }

@group(0) @binding(0) var<uniform> camera:   Camera;
@group(1) @binding(0) var<uniform> model:    Model;
@group(2) @binding(0) var<uniform> material: Material;
@group(2) @binding(1) var t_albedo:          texture_2d<f32>;
@group(2) @binding(2) var s_albedo:          sampler;
@group(3) @binding(0) var<uniform> light:    Light;

struct VertIn  { @location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32> }
struct VertOut { @builtin(position) clip_pos: vec4<f32>, @location(0) world_pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32> }

@vertex
fn vs_main(in: VertIn) -> VertOut {
    let world_pos = model.model * vec4<f32>(in.position, 1.0);
    var out: VertOut;
    out.clip_pos  = camera.view_proj * world_pos;
    out.world_pos = world_pos.xyz;
    out.normal    = normalize((model.model * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv        = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let tex_color  = textureSample(t_albedo, s_albedo, in.uv);
    let base_color = tex_color * material.albedo;
    let N          = normalize(in.normal);
    let L          = normalize(-light.direction);
    let diffuse    = max(dot(N, L), 0.0);
    let V          = normalize(camera.camera_pos - in.world_pos);
    let H          = normalize(L + V);
    let spec_pow   = mix(8.0, 256.0, 1.0 - material.roughness);
    let specular   = pow(max(dot(N, H), 0.0), spec_pow) * material.metallic;
    let ambient    = light.ambient * base_color.rgb;
    let color      = ambient + base_color.rgb * diffuse * light.color + vec3<f32>(specular) * light.color;
    return vec4<f32>(color, base_color.a);
}
"#;