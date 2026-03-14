// =============================================================================
//  Brass Engine — Renderer2D
//  Batch renderer obsługujący:
//    • Sprite (textury / kolorowe quady)
//    • Prymitywy (linie, prostokąty, koła jako wielokąty)
//    • UI / text (placeholder — gotowe do podpięcia fontdue/ab_glyph)
// =============================================================================

use std::collections::HashMap;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2};
use wgpu::util::DeviceExt;
use wgpu::*;

use super::context::RenderContext;

// ─── stałe ────────────────────────────────────────────────────────────────────

const MAX_QUADS:    usize = 10_000;
const MAX_VERTICES: usize = MAX_QUADS * 4;
const MAX_INDICES:  usize = MAX_QUADS * 6;

const WHITE_TEXTURE_ID: u64 = 0;

// ─── typy publiczne ────────────────────────────────────────────────────────────

/// RGBA kolor w zakresie 0.0–1.0
#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE:   Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK:   Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const RED:     Color = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const GREEN:   Color = Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const BLUE:    Color = Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
    pub const YELLOW:  Color = Color { r: 1.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const CYAN:    Color = Color { r: 0.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const MAGENTA: Color = Color { r: 1.0, g: 0.0, b: 1.0, a: 1.0 };

    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as f32 / 255.0,
            g: ((hex >> 8)  & 0xFF) as f32 / 255.0,
            b: (hex & 0xFF)          as f32 / 255.0,
            a: 1.0,
        }
    }

    fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// Deskryptor sprite'a — wszystko czego potrzebujesz do narysowania quada
#[derive(Clone, Debug)]
pub struct Sprite {
    pub position:   Vec2,
    pub size:       Vec2,
    pub rotation:   f32,
    pub color:      Color,
    pub texture_id: Option<u64>,
    pub uv_rect:    [f32; 4],
    pub z_order:    f32,
}

impl Sprite {
    pub fn new(position: Vec2, size: Vec2) -> Self {
        Self {
            position,
            size,
            rotation:   0.0,
            color:      Color::WHITE,
            texture_id: None,
            uv_rect:    [0.0, 0.0, 1.0, 1.0],
            z_order:    0.5,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_texture(mut self, id: u64) -> Self {
        self.texture_id = Some(id);
        self
    }

    pub fn with_rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    pub fn with_uv(mut self, u_min: f32, v_min: f32, u_max: f32, v_max: f32) -> Self {
        self.uv_rect = [u_min, v_min, u_max, v_max];
        self
    }
}

/// Komenda rysowania — wewnętrzny enum batch renderera
#[derive(Clone, Debug)]
pub enum DrawCommand {
    Quad(Sprite),
    Line    { start: Vec2, end: Vec2, thickness: f32, color: Color },
    Rect    { position: Vec2, size: Vec2, color: Color, filled: bool, thickness: f32 },
    Circle  { center: Vec2, radius: f32, color: Color, segments: u32 },
    Text    { text: String, position: Vec2, size: f32, color: Color },
}

// ─── vertex ───────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex2D {
    position: [f32; 2],
    uv:       [f32; 2],
    color:    [f32; 4],
    z:        f32,
    _pad:     f32,
}

impl Vertex2D {
    const ATTRIBS: [VertexAttribute; 4] = vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x2,
    ];

    fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex2D>() as BufferAddress,
            step_mode:    VertexStepMode::Vertex,
            attributes:   &Self::ATTRIBS,
        }
    }
}

// ─── uniform ──────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

// ─── tekstury ─────────────────────────────────────────────────────────────────

struct GpuTexture {
    #[allow(dead_code)]
    texture:    Texture,
    #[allow(dead_code)]
    view:       TextureView,
    bind_group: BindGroup,
}

// ─── Renderer2D ───────────────────────────────────────────────────────────────

pub struct Renderer2D {
    pipeline:             RenderPipeline,
    vertex_buffer:        Buffer,
    index_buffer:         Buffer,
    uniform_buffer:       Buffer,
    uniform_bind_group:   BindGroup,
    texture_bind_layout:  BindGroupLayout,

    vertices: Vec<Vertex2D>,
    indices:  Vec<u32>,

    textures:    HashMap<u64, GpuTexture>,
    next_tex_id: u64,

    commands: Vec<DrawCommand>,

    camera:   CameraUniform,
    viewport: (f32, f32),
}

impl Renderer2D {
    pub fn new(ctx: &RenderContext) -> Self {
        let device = &ctx.device;

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label:  Some("2D Shader"),
            source: ShaderSource::Wgsl(SHADER_2D.into()),
        });

        let uniform_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label:   Some("Uniform BGL"),
            entries: &[BindGroupLayoutEntry {
                binding:    0,
                visibility: ShaderStages::VERTEX,
                ty:         BindingType::Buffer {
                    ty:                 BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size:   None,
                },
                count: None,
            }],
        });

        let texture_bind_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label:   Some("Texture BGL"),
            entries: &[
                BindGroupLayoutEntry {
                    binding:    0,
                    visibility: ShaderStages::FRAGMENT,
                    ty:         BindingType::Texture {
                        sample_type:    TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding:    1,
                    visibility: ShaderStages::FRAGMENT,
                    ty:         BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label:                Some("2D Pipeline Layout"),
            bind_group_layouts:   &[&uniform_layout, &texture_bind_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label:  Some("2D Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module:              &shader,
                entry_point:        "vs_main",
                buffers:             &[Vertex2D::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module:              &shader,
                entry_point:        "fs_main",
                targets:             &[Some(ColorTargetState {
                    format:     ctx.format,
                    blend:      Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology:           PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face:         FrontFace::Ccw,
                cull_mode:          None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample:   MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label:              Some("2D VB"),
            size:               (MAX_VERTICES * std::mem::size_of::<Vertex2D>()) as u64,
            usage:              BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&BufferDescriptor {
            label:              Some("2D IB"),
            size:               (MAX_INDICES * std::mem::size_of::<u32>()) as u64,
            usage:              BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (w, h) = ctx.viewport();
        let camera = CameraUniform {
            view_proj: Mat4::orthographic_rh(0.0, w, h, 0.0, -1.0, 1.0).to_cols_array_2d(),
        };

        let uniform_buffer = device.create_buffer_init(&util::BufferInitDescriptor {
            label:    Some("2D Uniform"),
            contents: bytemuck::bytes_of(&camera),
            usage:    BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label:   Some("2D Uniform BG"),
            layout:  &uniform_layout,
            entries: &[BindGroupEntry {
                binding:  0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let mut renderer = Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            uniform_bind_group,
            texture_bind_layout,
            vertices: Vec::with_capacity(MAX_VERTICES),
            indices:  Vec::with_capacity(MAX_INDICES),
            textures: HashMap::new(),
            next_tex_id: 1,
            commands: Vec::new(),
            camera,
            viewport: (w, h),
        };

        renderer.create_white_texture(ctx);
        renderer
    }

    // ── Publiczne API ─────────────────────────────────────────────────────────

    pub fn load_texture_bytes(&mut self, ctx: &RenderContext, bytes: &[u8]) -> u64 {
        let img = image::load_from_memory(bytes)
            .expect("Nieprawidłowe dane obrazu")
            .to_rgba8();
        let (w, h) = img.dimensions();
        self.upload_texture(ctx, &img, w, h)
    }

    pub fn draw_sprite(&mut self, sprite: Sprite) {
        self.commands.push(DrawCommand::Quad(sprite));
    }

    pub fn draw_line(&mut self, start: Vec2, end: Vec2, thickness: f32, color: Color) {
        self.commands.push(DrawCommand::Line { start, end, thickness, color });
    }

    pub fn draw_rect(&mut self, position: Vec2, size: Vec2, color: Color, filled: bool) {
        self.commands.push(DrawCommand::Rect { position, size, color, filled, thickness: 2.0 });
    }

    pub fn draw_rect_ex(&mut self, position: Vec2, size: Vec2, color: Color, filled: bool, thickness: f32) {
        self.commands.push(DrawCommand::Rect { position, size, color, filled, thickness });
    }

    pub fn draw_circle(&mut self, center: Vec2, radius: f32, color: Color) {
        self.commands.push(DrawCommand::Circle { center, radius, color, segments: 32 });
    }

    pub fn draw_circle_ex(&mut self, center: Vec2, radius: f32, color: Color, segments: u32) {
        self.commands.push(DrawCommand::Circle { center, radius, color, segments });
    }

    pub fn draw_text(&mut self, text: &str, position: Vec2, size: f32, color: Color) {
        self.commands.push(DrawCommand::Text {
            text: text.to_string(), position, size, color,
        });
    }

    pub fn resize(&mut self, ctx: &RenderContext) {
        let (w, h) = ctx.viewport();
        self.viewport = (w, h);
        self.camera.view_proj =
            Mat4::orthographic_rh(0.0, w, h, 0.0, -1.0, 1.0).to_cols_array_2d();
        ctx.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&self.camera));
    }

    /// Standalone render — pobiera własny frame (używaj gdy nie ma 3D).
    pub fn render(&mut self, ctx: &RenderContext) -> Result<(), SurfaceError> {
        let output = ctx.surface.get_current_texture()?;
        let view   = output.texture.create_view(&TextureViewDescriptor::default());

        self.flush_commands();

        if !self.vertices.is_empty() {
            ctx.queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
            ctx.queue.write_buffer(&self.index_buffer,  0, bytemuck::cast_slice(&self.indices));
        }

        let mut encoder = ctx.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("2D Encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("2D Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view:           &view,
                    resolve_target: None,
                    ops: Operations {
                        load:  LoadOp::Clear(wgpu::Color { r: 0.08, g: 0.08, b: 0.10, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            if !self.vertices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                self.draw_batches(&mut pass);
            }
        }

        ctx.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        self.vertices.clear();
        self.indices.clear();

        Ok(())
    }

    /// Render na zewnętrzny TextureView — używany gdy 3D renderuje pierwszy
    /// (app.rs: r3d.render → r2d.render_to_view z LoadOp::Load).
    pub fn render_to_view(
        &mut self,
        ctx:  &RenderContext,
        view: &wgpu::TextureView,
    ) -> Result<(), SurfaceError> {
        // ── FIX: przetwórz komendy → vertices przed sprawdzeniem czy coś jest ──
        self.flush_commands();

        if !self.vertices.is_empty() {
            ctx.queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
            ctx.queue.write_buffer(&self.index_buffer,  0, bytemuck::cast_slice(&self.indices));
        }

        let mut encoder = ctx.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("2D Overlay Encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("2D Overlay Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: Operations {
                        load:  LoadOp::Load, // nie czyść — nałóż na 3D
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            if !self.vertices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                self.draw_batches(&mut pass);
            }
        }

        ctx.queue.submit(std::iter::once(encoder.finish()));

        self.vertices.clear();
        self.indices.clear();

        Ok(())
    }

    // ── Internale ─────────────────────────────────────────────────────────────

    fn flush_commands(&mut self) {
        let cmds: Vec<DrawCommand> = self.commands.drain(..).collect();

        let mut sorted = cmds;
        sorted.sort_by(|a, b| {
            cmd_z(a).partial_cmp(&cmd_z(b)).unwrap_or(std::cmp::Ordering::Equal)
        });

        for cmd in sorted {
            match cmd {
                DrawCommand::Quad(s)   => self.push_sprite(&s),
                DrawCommand::Line { start, end, thickness, color } =>
                    self.push_line(start, end, thickness, color),
                DrawCommand::Rect { position, size, color, filled, thickness } =>
                    self.push_rect(position, size, color, filled, thickness),
                DrawCommand::Circle { center, radius, color, segments } =>
                    self.push_circle(center, radius, color, segments),
                DrawCommand::Text { text, position, size, color } =>
                    self.push_text_placeholder(&text, position, size, color),
            }
        }
    }

    fn push_sprite(&mut self, sprite: &Sprite) {
        let base  = self.vertices.len() as u32;
        let [u0, v0, u1, v1] = sprite.uv_rect;
        let color = sprite.color.to_array();
        let z     = sprite.z_order;
        let hw    = sprite.size.x * 0.5;
        let hh    = sprite.size.y * 0.5;

        let corners = [
            Vec2::new(-hw, -hh),
            Vec2::new( hw, -hh),
            Vec2::new( hw,  hh),
            Vec2::new(-hw,  hh),
        ];
        let uvs = [[u0,v0],[u1,v0],[u1,v1],[u0,v1]];
        let (sin, cos) = sprite.rotation.sin_cos();

        for (i, c) in corners.iter().enumerate() {
            let rx = c.x * cos - c.y * sin + sprite.position.x;
            let ry = c.x * sin + c.y * cos + sprite.position.y;
            self.vertices.push(Vertex2D { position: [rx, ry], uv: uvs[i], color, z, _pad: 0.0 });
        }
        self.indices.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
    }

    fn push_line(&mut self, start: Vec2, end: Vec2, thickness: f32, color: Color) {
        let dir   = (end - start).normalize_or_zero();
        let perp  = Vec2::new(-dir.y, dir.x) * (thickness * 0.5);
        let color = color.to_array();
        let base  = self.vertices.len() as u32;
        let uvs   = [[0f32,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]];

        for (i, p) in [start+perp, start-perp, end-perp, end+perp].iter().enumerate() {
            self.vertices.push(Vertex2D { position: p.to_array(), uv: uvs[i], color, z: 0.9, _pad: 0.0 });
        }
        self.indices.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
    }

    fn push_rect(&mut self, pos: Vec2, size: Vec2, color: Color, filled: bool, thickness: f32) {
        if filled {
            self.push_sprite(&Sprite::new(pos + size * 0.5, size).with_color(color));
        } else {
            let (x, y, w, h) = (pos.x, pos.y, size.x, size.y);
            self.push_line(Vec2::new(x,   y),   Vec2::new(x+w, y),   thickness, color);
            self.push_line(Vec2::new(x+w, y),   Vec2::new(x+w, y+h), thickness, color);
            self.push_line(Vec2::new(x+w, y+h), Vec2::new(x,   y+h), thickness, color);
            self.push_line(Vec2::new(x,   y+h), Vec2::new(x,   y),   thickness, color);
        }
    }

    fn push_circle(&mut self, center: Vec2, radius: f32, color: Color, segments: u32) {
        let color = color.to_array();
        let base  = self.vertices.len() as u32;
        let step  = std::f32::consts::TAU / segments as f32;

        self.vertices.push(Vertex2D {
            position: center.to_array(), uv: [0.5, 0.5], color, z: 0.5, _pad: 0.0,
        });
        for i in 0..=segments {
            let a = i as f32 * step;
            self.vertices.push(Vertex2D {
                position: [center.x + radius * a.cos(), center.y + radius * a.sin()],
                uv: [(a.cos() + 1.0) * 0.5, (a.sin() + 1.0) * 0.5],
                color, z: 0.5, _pad: 0.0,
            });
        }
        for i in 0..segments {
            self.indices.extend_from_slice(&[base, base+1+i, base+2+i]);
        }
    }

    fn push_text_placeholder(&mut self, text: &str, position: Vec2, font_size: f32, color: Color) {
        let char_w = font_size * 0.6;
        for (i, _) in text.chars().enumerate() {
            let x = position.x + i as f32 * (char_w + 1.0);
            self.push_sprite(
                &Sprite::new(
                    Vec2::new(x + char_w * 0.5, position.y + font_size * 0.5),
                    Vec2::new(char_w, font_size),
                ).with_color(color)
            );
        }
    }

    fn draw_batches<'a>(&'a self, pass: &mut RenderPass<'a>) {
        let white = self.textures.get(&WHITE_TEXTURE_ID).expect("Brak białej tekstury");
        pass.set_bind_group(1, &white.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);
    }

    fn create_white_texture(&mut self, ctx: &RenderContext) {
        self.upload_texture_raw(ctx, &[255u8; 4], 1, 1, WHITE_TEXTURE_ID);
    }

    fn upload_texture(&mut self, ctx: &RenderContext, data: &[u8], w: u32, h: u32) -> u64 {
        let id = self.next_tex_id;
        self.next_tex_id += 1;
        self.upload_texture_raw(ctx, data, w, h, id);
        id
    }

    fn upload_texture_raw(&mut self, ctx: &RenderContext, data: &[u8], w: u32, h: u32, id: u64) {
        let device = &ctx.device;
        let queue  = &ctx.queue;

        let texture = device.create_texture(&TextureDescriptor {
            label:           Some("Brass Texture"),
            size:            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       TextureDimension::D2,
            format:          TextureFormat::Rgba8UnormSrgb,
            usage:           TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats:    &[],
        });

        queue.write_texture(
            ImageCopyTexture {
                texture: &texture, mip_level: 0,
                origin: Origin3d::ZERO, aspect: TextureAspect::All,
            },
            data,
            ImageDataLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        let view = texture.create_view(&TextureViewDescriptor::default());

        let sampler = device.create_sampler(&SamplerDescriptor {
            label:          Some("Brass Sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter:     FilterMode::Linear,
            min_filter:     FilterMode::Linear,
            mipmap_filter:  FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label:   Some("Texture BG"),
            layout:  &self.texture_bind_layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&view) },
                BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&sampler) },
            ],
        });

        self.textures.insert(id, GpuTexture { texture, view, bind_group });
    }
}

// ─── helpers ──────────────────────────────────────────────────────────────────

fn cmd_z(cmd: &DrawCommand) -> f32 {
    match cmd {
        DrawCommand::Quad(s) => s.z_order,
        _                    => 0.5,
    }
}

// ─── WGSL Shader ──────────────────────────────────────────────────────────────

const SHADER_2D: &str = r#"
struct Camera { view_proj: mat4x4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var t_diffuse: texture_2d<f32>;
@group(1) @binding(1) var s_diffuse: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv:       vec2<f32>,
    @location(2) color:    vec4<f32>,
    @location(3) zpad:     vec2<f32>,
}
struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv:    vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_pos = camera.view_proj * vec4<f32>(in.position, in.zpad.x, 1.0);
    out.uv       = in.uv;
    out.color    = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_diffuse, s_diffuse, in.uv) * in.color;
}
"#;