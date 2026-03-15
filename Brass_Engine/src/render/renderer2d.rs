// =============================================================================
//  Brass Engine — Renderer2D  (v2)
//
//  • Sprite batching per-texture (jeden draw call na teksturę)
//  • Warstwy (layers 0–255) z depth sorting
//  • Sprite sheet / animacje (UV atlas)
//  • Tile map renderer
//  • Nineslice
//  • Particle system (CPU-side)
//  • Font renderer (fontdue)
//  • Lighting 2D (point lights — additive pass)
// =============================================================================

use std::collections::HashMap;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec4};
use wgpu::util::DeviceExt;
use wgpu::*;

use super::context::RenderContext;

// ─── Stałe ────────────────────────────────────────────────────────────────────

const MAX_QUADS:    usize = 20_000;
const MAX_VERTICES: usize = MAX_QUADS * 4;
const MAX_INDICES:  usize = MAX_QUADS * 6;

pub const WHITE_TEX:  u64 = 0;
pub const LAYER_MIN:  u8  = 0;
pub const LAYER_MAX:  u8  = 255;

// ─── Color ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f32, pub g: f32, pub b: f32, pub a: f32,
}

impl Color {
    pub const WHITE:   Self = Self { r:1.0, g:1.0, b:1.0, a:1.0 };
    pub const BLACK:   Self = Self { r:0.0, g:0.0, b:0.0, a:1.0 };
    pub const RED:     Self = Self { r:1.0, g:0.0, b:0.0, a:1.0 };
    pub const GREEN:   Self = Self { r:0.0, g:1.0, b:0.0, a:1.0 };
    pub const BLUE:    Self = Self { r:0.0, g:0.0, b:1.0, a:1.0 };
    pub const YELLOW:  Self = Self { r:1.0, g:1.0, b:0.0, a:1.0 };
    pub const CYAN:    Self = Self { r:0.0, g:1.0, b:1.0, a:1.0 };
    pub const MAGENTA: Self = Self { r:1.0, g:0.0, b:1.0, a:1.0 };
    pub const TRANSPARENT: Self = Self { r:0.0, g:0.0, b:0.0, a:0.0 };

    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self { Self { r, g, b, a } }
    pub fn hex(h: u32) -> Self {
        Self::rgba(
            ((h>>16)&0xFF) as f32/255.0,
            ((h>>8) &0xFF) as f32/255.0,
            (h&0xFF)       as f32/255.0,
            1.0,
        )
    }
    pub fn with_alpha(mut self, a: f32) -> Self { self.a = a; self }
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::rgba(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
            self.a + (other.a - self.a) * t,
        )
    }
    fn to_array(self) -> [f32; 4] { [self.r, self.g, self.b, self.a] }
}

// ─── Sprite ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Sprite {
    pub position:   Vec2,
    pub size:       Vec2,
    pub rotation:   f32,
    pub color:      Color,
    pub texture_id: u64,       // WHITE_TEX = kolorowy quad
    pub uv_rect:    [f32; 4],  // [u0,v0,u1,v1]
    pub layer:      u8,        // sortowanie głębokości
    pub flip_x:     bool,
    pub flip_y:     bool,
}

impl Sprite {
    pub fn new(position: Vec2, size: Vec2) -> Self {
        Self {
            position, size,
            rotation:   0.0,
            color:      Color::WHITE,
            texture_id: WHITE_TEX,
            uv_rect:    [0.0, 0.0, 1.0, 1.0],
            layer:      128,
            flip_x:     false,
            flip_y:     false,
        }
    }
    pub fn with_color(mut self, c: Color)    -> Self { self.color = c;              self }
    pub fn with_texture(mut self, id: u64)   -> Self { self.texture_id = id;        self }
    pub fn with_rotation(mut self, r: f32)   -> Self { self.rotation = r;           self }
    pub fn with_layer(mut self, l: u8)       -> Self { self.layer = l;              self }
    pub fn with_flip(mut self, x: bool, y: bool) -> Self { self.flip_x=x; self.flip_y=y; self }
    pub fn with_uv(mut self, u0:f32, v0:f32, u1:f32, v1:f32) -> Self {
        self.uv_rect = [u0,v0,u1,v1]; self
    }
}

// ─── SpriteSheet / Animation ──────────────────────────────────────────────────

/// Definicja sprite sheetu.
#[derive(Clone, Debug)]
pub struct SpriteSheet {
    pub texture_id: u64,
    pub cols:       u32,
    pub rows:       u32,
    pub frame_w:    f32,  // = 1.0 / cols
    pub frame_h:    f32,
}

impl SpriteSheet {
    pub fn new(texture_id: u64, cols: u32, rows: u32) -> Self {
        Self {
            texture_id, cols, rows,
            frame_w: 1.0 / cols as f32,
            frame_h: 1.0 / rows as f32,
        }
    }

    /// UV rect dla klatki (row-major: frame 0 = lewy górny).
    pub fn uv(&self, frame: u32) -> [f32; 4] {
        let col = (frame % self.cols) as f32;
        let row = (frame / self.cols) as f32;
        [
            col * self.frame_w,
            row * self.frame_h,
            (col + 1.0) * self.frame_w,
            (row + 1.0) * self.frame_h,
        ]
    }
}

/// Prosta animacja — lista klatek z jednego sprite sheetu.
#[derive(Clone, Debug)]
pub struct Animation {
    pub sheet:        SpriteSheet,
    pub frames:       Vec<u32>,  // indeksy klatek
    pub fps:          f32,
    pub looping:      bool,
    current_frame:    usize,
    timer:            f32,
    pub finished:     bool,
}

impl Animation {
    pub fn new(sheet: SpriteSheet, frames: Vec<u32>, fps: f32, looping: bool) -> Self {
        Self { sheet, frames, fps, looping, current_frame: 0, timer: 0.0, finished: false }
    }

    pub fn update(&mut self, dt: f32) {
        if self.finished { return; }
        self.timer += dt;
        let frame_time = 1.0 / self.fps;
        while self.timer >= frame_time {
            self.timer -= frame_time;
            self.current_frame += 1;
            if self.current_frame >= self.frames.len() {
                if self.looping {
                    self.current_frame = 0;
                } else {
                    self.current_frame = self.frames.len() - 1;
                    self.finished = true;
                    return;
                }
            }
        }
    }

    pub fn reset(&mut self) {
        self.current_frame = 0;
        self.timer = 0.0;
        self.finished = false;
    }

    /// Aktualny UV rect do użycia w Sprite.
    pub fn current_uv(&self) -> [f32; 4] {
        let frame = self.frames[self.current_frame];
        self.sheet.uv(frame)
    }

    pub fn texture_id(&self) -> u64 { self.sheet.texture_id }
}

// ─── Tilemap ──────────────────────────────────────────────────────────────────

pub struct Tilemap {
    pub texture_id: u64,
    pub tile_size:  f32,
    pub cols:       u32,  // kafelków w atlasie poziomo
    pub map_width:  u32,
    pub map_height: u32,
    pub tiles:      Vec<u32>,  // indeks klatki atlasu; u32::MAX = pusty
    pub offset:     Vec2,
    pub layer:      u8,
}

impl Tilemap {
    pub fn new(
        texture_id: u64,
        tile_size: f32,
        atlas_cols: u32,
        map_width: u32,
        map_height: u32,
        offset: Vec2,
    ) -> Self {
        Self {
            texture_id, tile_size,
            cols: atlas_cols,
            map_width, map_height,
            tiles: vec![u32::MAX; (map_width * map_height) as usize],
            offset,
            layer: 64,
        }
    }

    pub fn set(&mut self, x: u32, y: u32, tile: u32) {
        if x < self.map_width && y < self.map_height {
            self.tiles[(y * self.map_width + x) as usize] = tile;
        }
    }

    pub fn get(&self, x: u32, y: u32) -> Option<u32> {
        if x < self.map_width && y < self.map_height {
            let t = self.tiles[(y * self.map_width + x) as usize];
            if t == u32::MAX { None } else { Some(t) }
        } else {
            None
        }
    }

    /// UV rect dla kafelka (atlas row-major).
    fn tile_uv(&self, tile: u32) -> [f32; 4] {
        let atlas_cols = self.cols as f32;
        let col = (tile % self.cols) as f32;
        let row = (tile / self.cols) as f32;
        let fw = 1.0 / atlas_cols;
        // Zakładamy kwadratowy atlas (cols×cols); jeśli inny — dodaj atlas_rows
        let fh = fw;
        [col * fw, row * fh, (col + 1.0) * fw, (row + 1.0) * fh]
    }
}

// ─── Nineslice ────────────────────────────────────────────────────────────────

/// Nineslice — panel UI który skaluje się bez rozciągania rogów.
pub struct Nineslice {
    pub texture_id: u64,
    /// Marginesy w UV: [left, right, top, bottom] — wartości 0..1
    pub border_uv:  [f32; 4],
    /// Marginesy w pikselach (rozmiar rogów na ekranie)
    pub border_px:  [f32; 4],
}

impl Nineslice {
    pub fn new(texture_id: u64, border_uv: [f32; 4], border_px: [f32; 4]) -> Self {
        Self { texture_id, border_uv, border_px }
    }
}

// ─── Point Light 2D ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct PointLight2D {
    pub position:  Vec2,
    pub color:     Color,
    pub radius:    f32,
    pub intensity: f32,
}

impl PointLight2D {
    pub fn new(position: Vec2, color: Color, radius: f32, intensity: f32) -> Self {
        Self { position, color, radius, intensity }
    }
}

// ─── Particle ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Particle {
    pub position:  Vec2,
    pub velocity:  Vec2,
    pub color:     Color,
    pub end_color: Color,
    pub size:      f32,
    pub end_size:  f32,
    pub lifetime:  f32,
    pub age:       f32,
    pub rotation:  f32,
    pub rot_speed: f32,
    texture_id:    u64,
    layer:         u8,
}

pub struct ParticleEmitter {
    pub position:     Vec2,
    pub texture_id:   u64,
    pub layer:        u8,
    // Zakresy spawnu
    pub speed_min:    f32,
    pub speed_max:    f32,
    pub angle_min:    f32,  // radians
    pub angle_max:    f32,
    pub lifetime_min: f32,
    pub lifetime_max: f32,
    pub size_start:   f32,
    pub size_end:     f32,
    pub color_start:  Color,
    pub color_end:    Color,
    pub rot_speed:    f32,
    // Burst vs continuous
    pub emit_rate:    f32,  // particles/sec; 0 = burst only
    emit_acc:         f32,
    pub active:       bool,
    pub particles:    Vec<Particle>,
}

impl ParticleEmitter {
    pub fn new(position: Vec2) -> Self {
        Self {
            position,
            texture_id:   WHITE_TEX,
            layer:        200,
            speed_min:    50.0, speed_max: 150.0,
            angle_min:    0.0,  angle_max: std::f32::consts::TAU,
            lifetime_min: 0.5,  lifetime_max: 1.5,
            size_start:   8.0,  size_end: 0.0,
            color_start:  Color::WHITE,
            color_end:    Color::TRANSPARENT,
            rot_speed:    0.0,
            emit_rate:    30.0,
            emit_acc:     0.0,
            active:       true,
            particles:    Vec::new(),
        }
    }

    pub fn burst(&mut self, count: u32) {
        for _ in 0..count { self.spawn_particle(); }
    }

    pub fn update(&mut self, dt: f32) {
        // Update istniejących
        self.particles.retain_mut(|p| {
            p.age += dt;
            let t = (p.age / p.lifetime).clamp(0.0, 1.0);
            p.position += p.velocity * dt;
            p.color     = p.color.lerp(p.end_color, t);
            p.size      = p.size + (p.end_size - p.size) * t;
            p.rotation += p.rot_speed * dt;
            p.age < p.lifetime
        });

        // Emituj nowe
        if self.active && self.emit_rate > 0.0 {
            self.emit_acc += self.emit_rate * dt;
            while self.emit_acc >= 1.0 {
                self.emit_acc -= 1.0;
                self.spawn_particle();
            }
        }
    }

    fn spawn_particle(&mut self) {
        use std::time::{SystemTime, UNIX_EPOCH};
        // Prymitywny RNG bez zewnętrznych zależności
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as f32;
        let r1 = ((seed * 1.6180339) % 1.0).abs();
        let r2 = ((seed * 2.7182818) % 1.0).abs();
        let r3 = ((seed * 3.1415926) % 1.0).abs();
        let r4 = ((seed * 0.5772156) % 1.0).abs();

        let speed    = self.speed_min + r1 * (self.speed_max - self.speed_min);
        let angle    = self.angle_min + r2 * (self.angle_max - self.angle_min);
        let lifetime = self.lifetime_min + r3 * (self.lifetime_max - self.lifetime_min);
        let size     = self.size_start;

        self.particles.push(Particle {
            position:  self.position,
            velocity:  Vec2::new(angle.cos(), angle.sin()) * speed,
            color:     self.color_start,
            end_color: self.color_end,
            size,
            end_size:  self.size_end,
            lifetime,
            age:       0.0,
            rotation:  r4 * std::f32::consts::TAU,
            rot_speed: self.rot_speed,
            texture_id: self.texture_id,
            layer:     self.layer,
        });
    }
}

// ─── GPU font texture ─────────────────────────────────────────────────────────

pub struct FontAtlas {
    pub texture_id: u64,
    pub size:       f32,
    // Dla każdego znaku ASCII 32..127: [u0,v0,u1,v1, advance, offset_x, offset_y]
    glyphs: HashMap<char, GlyphInfo>,
}

#[derive(Clone, Copy)]
struct GlyphInfo {
    uv:       [f32; 4],
    width:    f32,
    height:   f32,
    advance:  f32,
    offset_y: f32,
}

// ─── Wewnętrzny vertex ────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex2D {
    position: [f32; 2],
    uv:       [f32; 2],
    color:    [f32; 4],
    z:        f32,
    _pad:     f32,
}

// ─── GPU tekstura ─────────────────────────────────────────────────────────────

pub struct GpuTexture {
    #[allow(dead_code)]
    pub texture:    Texture,
    pub view:       TextureView,
    pub bind_group: BindGroup,
    pub width:      u32,
    pub height:     u32,
}

// ─── Draw command ─────────────────────────────────────────────────────────────

enum Cmd {
    Sprite(Sprite),
    Line    { a: Vec2, b: Vec2, thick: f32, color: Color, layer: u8 },
    Rect    { pos: Vec2, size: Vec2, color: Color, filled: bool, thick: f32, layer: u8 },
    Circle  { center: Vec2, r: f32, color: Color, segs: u32, layer: u8 },
    Tilemap { id: u64 },   // ID uchwyt do zakolejkowanej tilemaps
    Nineslice { ns: u64, pos: Vec2, size: Vec2, color: Color, layer: u8 },
    Text    { text: String, pos: Vec2, size: f32, color: Color, font: Option<u64>, layer: u8 },
    Particle { tex: u64, pos: Vec2, size: f32, rot: f32, color: Color, layer: u8 },
}

fn cmd_layer(c: &Cmd) -> u8 {
    match c {
        Cmd::Sprite(s)          => s.layer,
        Cmd::Line  { layer, .. }   => *layer,
        Cmd::Rect  { layer, .. }   => *layer,
        Cmd::Circle{ layer, .. }   => *layer,
        Cmd::Text  { layer, .. }   => *layer,
        Cmd::Particle{layer,..}    => *layer,
        Cmd::Tilemap { .. }        => 64,
        Cmd::Nineslice{layer,..}   => *layer,
    }
}

// ─── Light uniform ────────────────────────────────────────────────────────────

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct GpuLight {
    pos:       [f32; 2],
    radius:    f32,
    intensity: f32,
    color:     [f32; 4],
}

const MAX_LIGHTS: usize = 32;

#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)]
struct LightUniform {
    lights:      [GpuLight; MAX_LIGHTS],
    count:       u32,
    ambient:     [f32; 3],
    _pad:        f32,
}

// ─── Renderer2D ───────────────────────────────────────────────────────────────

pub struct Renderer2D {
    // Główny pipeline (sprites)
    pipeline:            RenderPipeline,
    vertex_buffer:       Buffer,
    index_buffer:        Buffer,
    uniform_buffer:      Buffer,
    uniform_bind_group:  BindGroup,
    texture_bind_layout: BindGroupLayout,

    // Lighting pipeline (additive pass)
    light_pipeline:      RenderPipeline,
    light_uniform_buf:   Buffer,
    light_uniform_bg:    BindGroup,
    light_uniform_bgl:   BindGroupLayout,

    vertices:  Vec<Vertex2D>,
    indices:   Vec<u32>,
    commands:  Vec<Cmd>,

    // Tekstury
    pub textures:    HashMap<u64, GpuTexture>,
    next_tex_id: u64,

    // Zakolejkowane tilemaps i nineslices (per-frame)
    tilemaps:   Vec<Tilemap>,
    nineslices: HashMap<u64, Nineslice>,
    next_ns_id: u64,

    // Fonts
    fonts: HashMap<u64, FontAtlas>,

    // Lights
    pub lights:  Vec<PointLight2D>,
    pub ambient: Color,

    camera:   [[f32;4];4],
    viewport: (f32, f32),
}

impl Renderer2D {
    pub fn new(ctx: &RenderContext) -> Self {
        let device = &ctx.device;
        let (w, h) = ctx.viewport();

        // ── BGLs ──────────────────────────────────────────────────────────────
        let uniform_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("2D Uniform BGL"),
            entries: &[bgle_uniform(0, ShaderStages::VERTEX)],
        });

        let texture_bind_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("2D Texture BGL"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0, visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2, multisampled: false,
                    }, count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1, visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let light_uniform_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("2D Light BGL"),
            entries: &[bgle_uniform(0, ShaderStages::FRAGMENT)],
        });

        // ── Pipelines ─────────────────────────────────────────────────────────
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("2D Shader"), source: ShaderSource::Wgsl(SHADER_2D.into()),
        });
        let light_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("2D Light Shader"), source: ShaderSource::Wgsl(SHADER_LIGHT.into()),
        });

        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("2D Layout"),
            bind_group_layouts: &[&uniform_bgl, &texture_bind_layout, &light_uniform_bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("2D Pipeline"), layout: Some(&layout),
            vertex: VertexState {
                module: &shader, entry_point: "vs_main",
                buffers: &[vertex_layout()], compilation_options: Default::default(),
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
            primitive:     PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample:   MultisampleState::default(),
            multiview:     None, cache: None,
        });

        // Lighting pipeline — additive blend over main pass
        let light_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Light Layout"),
            bind_group_layouts: &[&uniform_bgl, &texture_bind_layout, &light_uniform_bgl],
            push_constant_ranges: &[],
        });
        let light_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("2D Light Pipeline"), layout: Some(&light_layout),
            vertex: VertexState {
                module: &light_shader, entry_point: "vs_main",
                buffers: &[vertex_layout()], compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &light_shader, entry_point: "fs_main",
                targets: &[Some(ColorTargetState {
                    format: ctx.format,
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::One,
                            operation:  BlendOperation::Add,
                        },
                        alpha: BlendComponent::OVER,
                    }),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive:     PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample:   MultisampleState::default(),
            multiview:     None, cache: None,
        });

        // ── Bufory ────────────────────────────────────────────────────────────
        let vb = device.create_buffer(&BufferDescriptor {
            label: Some("2D VB"),
            size:  (MAX_VERTICES * std::mem::size_of::<Vertex2D>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ib = device.create_buffer(&BufferDescriptor {
            label: Some("2D IB"),
            size:  (MAX_INDICES * std::mem::size_of::<u32>()) as u64,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let cam = Mat4::orthographic_rh(0.0, w, h, 0.0, -1.0, 1.0).to_cols_array_2d();
        let cam_bytes = bytemuck::cast_slice(&cam);
        let ub = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("2D Uniform"), contents: cam_bytes,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let uniform_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("2D Uniform BG"), layout: &uniform_bgl,
            entries: &[BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }],
        });

        let light_data = LightUniform {
            lights:  [GpuLight { pos:[0.0;2], radius:0.0, intensity:0.0, color:[0.0;4] }; MAX_LIGHTS],
            count:   0,
            ambient: [0.0; 3],
            _pad:    0.0,
        };
        let light_ub = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("Light Uniform"), contents: bytemuck::bytes_of(&light_data),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let light_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Light BG"), layout: &light_uniform_bgl,
            entries: &[BindGroupEntry { binding: 0, resource: light_ub.as_entire_binding() }],
        });

        let mut r = Self {
            pipeline, vertex_buffer: vb, index_buffer: ib,
            uniform_buffer: ub, uniform_bind_group: uniform_bg,
            texture_bind_layout,
            light_pipeline, light_uniform_buf: light_ub, light_uniform_bg: light_bg,
            light_uniform_bgl,
            vertices: Vec::with_capacity(MAX_VERTICES),
            indices:  Vec::with_capacity(MAX_INDICES),
            commands: Vec::new(),
            textures: HashMap::new(), next_tex_id: 1,
            tilemaps: Vec::new(),
            nineslices: HashMap::new(), next_ns_id: 1,
            fonts: HashMap::new(),
            lights: Vec::new(),
            ambient: Color::rgba(0.15, 0.15, 0.15, 1.0),
            camera: cam, viewport: (w, h),
        };
        r.create_white_texture(ctx);
        r
    }

    // ── Publiczne API — draw ───────────────────────────────────────────────────

    pub fn draw_sprite(&mut self, s: Sprite) {
        self.commands.push(Cmd::Sprite(s));
    }

    pub fn draw_line(&mut self, a: Vec2, b: Vec2, thick: f32, color: Color) {
        self.commands.push(Cmd::Line { a, b, thick, color, layer: 128 });
    }

    pub fn draw_line_l(&mut self, a: Vec2, b: Vec2, thick: f32, color: Color, layer: u8) {
        self.commands.push(Cmd::Line { a, b, thick, color, layer });
    }

    pub fn draw_rect(&mut self, pos: Vec2, size: Vec2, color: Color, filled: bool) {
        self.commands.push(Cmd::Rect { pos, size, color, filled, thick: 2.0, layer: 128 });
    }

    pub fn draw_rect_ex(&mut self, pos: Vec2, size: Vec2, color: Color, filled: bool, thick: f32, layer: u8) {
        self.commands.push(Cmd::Rect { pos, size, color, filled, thick, layer });
    }

    pub fn draw_circle(&mut self, center: Vec2, r: f32, color: Color) {
        self.commands.push(Cmd::Circle { center, r, color, segs: 32, layer: 128 });
    }

    pub fn draw_circle_ex(&mut self, center: Vec2, r: f32, color: Color, segs: u32, layer: u8) {
        self.commands.push(Cmd::Circle { center, r, color, segs, layer });
    }

    pub fn draw_text(&mut self, text: &str, pos: Vec2, size: f32, color: Color) {
        self.commands.push(Cmd::Text {
            text: text.to_string(), pos, size, color, font: None, layer: 250,
        });
    }

    pub fn draw_text_ex(&mut self, text: &str, pos: Vec2, size: f32, color: Color, font: u64, layer: u8) {
        self.commands.push(Cmd::Text {
            text: text.to_string(), pos, size, color, font: Some(font), layer,
        });
    }

    /// Narysuj tilemap w tej klatce (rejestruje do batchu).
    pub fn draw_tilemap(&mut self, tilemap: Tilemap) {
        self.tilemaps.push(tilemap);
    }

    /// Narysuj nineslice.
    pub fn draw_nineslice(&mut self, ns: &Nineslice, pos: Vec2, size: Vec2, color: Color, layer: u8) {
        let id = self.next_ns_id; self.next_ns_id += 1;
        self.nineslices.insert(id, Nineslice {
            texture_id: ns.texture_id,
            border_uv:  ns.border_uv,
            border_px:  ns.border_px,
        });
        self.commands.push(Cmd::Nineslice { ns: id, pos, size, color, layer });
    }

    /// Narysuj wszystkie żywe cząsteczki emitera.
    pub fn draw_particles(&mut self, emitter: &ParticleEmitter) {
        for p in &emitter.particles {
            self.commands.push(Cmd::Particle {
                tex: p.texture_id,
                pos: p.position,
                size: p.size,
                rot: p.rotation,
                color: p.color,
                layer: p.layer,
            });
        }
    }

    /// Dodaj point light na tę klatkę.
    pub fn add_light(&mut self, light: PointLight2D) {
        self.lights.push(light);
    }

    // ── Tekstury ──────────────────────────────────────────────────────────────

    pub fn load_texture_bytes(&mut self, ctx: &RenderContext, bytes: &[u8]) -> u64 {
        let img = image::load_from_memory(bytes)
            .expect("Nieprawidłowy obraz")
            .to_rgba8();
        let (w, h) = img.dimensions();
        self.upload_texture(ctx, &img, w, h)
    }

    pub fn get_texture_size(&self, id: u64) -> Option<(u32, u32)> {
        self.textures.get(&id).map(|t| (t.width, t.height))
    }

    // ── Resize ────────────────────────────────────────────────────────────────

    pub fn resize(&mut self, ctx: &RenderContext) {
        let (w, h) = ctx.viewport();
        self.viewport = (w, h);
        self.camera = Mat4::orthographic_rh(0.0, w, h, 0.0, -1.0, 1.0).to_cols_array_2d();
        ctx.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&self.camera));
    }

    // ── Render ────────────────────────────────────────────────────────────────

    pub fn render_to_view(&mut self, ctx: &RenderContext, view: &wgpu::TextureView)
        -> Result<(), SurfaceError>
    {
        self.flush_all(ctx);
        self.submit_pass(ctx, view, wgpu::LoadOp::Load);
        self.vertices.clear();
        self.indices.clear();
        Ok(())
    }

    pub fn render(&mut self, ctx: &RenderContext) -> Result<(), SurfaceError> {
        let output = ctx.surface.get_current_texture()?;
        let view   = output.texture.create_view(&TextureViewDescriptor::default());
        self.flush_all(ctx);
        self.submit_pass(ctx, &view, wgpu::LoadOp::Clear(wgpu::Color { r:0.08, g:0.08, b:0.10, a:1.0 }));
        self.vertices.clear();
        self.indices.clear();
        output.present();
        Ok(())
    }

    // ── Internale ─────────────────────────────────────────────────────────────

    fn flush_all(&mut self, ctx: &RenderContext) {
        // Zaktualizuj light uniform
        let n = self.lights.len().min(MAX_LIGHTS);
        let mut lu = LightUniform {
            lights:  [GpuLight { pos:[0.0;2], radius:0.0, intensity:0.0, color:[0.0;4] }; MAX_LIGHTS],
            count:   n as u32,
            ambient: [self.ambient.r, self.ambient.g, self.ambient.b],
            _pad:    0.0,
        };
        for (i, l) in self.lights.iter().take(MAX_LIGHTS).enumerate() {
            lu.lights[i] = GpuLight {
                pos:       l.position.to_array(),
                radius:    l.radius,
                intensity: l.intensity,
                color:     l.color.to_array(),
            };
        }
        ctx.queue.write_buffer(&self.light_uniform_buf, 0, bytemuck::bytes_of(&lu));
        self.lights.clear();

        // Procesuj tilemaps
        let tilemaps: Vec<Tilemap> = self.tilemaps.drain(..).collect();
        for tm in tilemaps {
            self.push_tilemap(&tm);
        }

        // Procesuj komendy → vertices (posortowane po layerze)
        let mut cmds: Vec<Cmd> = self.commands.drain(..).collect();
        cmds.sort_by_key(|c| cmd_layer(c));

        for cmd in cmds {
            match cmd {
                Cmd::Sprite(s)   => self.push_sprite(&s),
                Cmd::Line { a, b, thick, color, .. } =>
                    self.push_line(a, b, thick, color),
                Cmd::Rect { pos, size, color, filled, thick, .. } =>
                    self.push_rect(pos, size, color, filled, thick),
                Cmd::Circle { center, r, color, segs, .. } =>
                    self.push_circle(center, r, color, segs),
                Cmd::Text { text, pos, size, color, font, .. } =>
                    self.push_text(&text, pos, size, color, font),
                Cmd::Particle { pos, size, rot, color, .. } => {
                    let s = Sprite::new(pos, Vec2::splat(size))
                        .with_rotation(rot)
                        .with_color(color);
                    self.push_sprite(&s);
                }
                Cmd::Nineslice { ns: ns_id, pos, size, color, .. } => {
                    if let Some(ns) = self.nineslices.remove(&ns_id) {
                        self.push_nineslice(&ns, pos, size, color);
                    }
                }
                Cmd::Tilemap { .. } => {} // już obsłużone wyżej
            }
        }

        // Upload do GPU
        if !self.vertices.is_empty() {
            ctx.queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
            ctx.queue.write_buffer(&self.index_buffer,  0, bytemuck::cast_slice(&self.indices));
        }
    }

    fn submit_pass(&self, ctx: &RenderContext, view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) {
        let white = self.textures.get(&WHITE_TEX).expect("brak białej tekstury");
        let mut enc = ctx.device.create_command_encoder(&CommandEncoderDescriptor { label: Some("2D Enc") });
        {
            let mut pass = enc.begin_render_pass(&RenderPassDescriptor {
                label: Some("2D Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view, resolve_target: None,
                    ops: Operations { load, store: StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            if !self.vertices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_bind_group(1, &white.bind_group, &[]);
                pass.set_bind_group(2, &self.light_uniform_bg, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint32);
                pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);
            }
        }
        ctx.queue.submit(std::iter::once(enc.finish()));
    }

    fn push_sprite(&mut self, s: &Sprite) {
        let base = self.vertices.len() as u32;
        let [mut u0, mut v0, mut u1, mut v1] = s.uv_rect;
        if s.flip_x { std::mem::swap(&mut u0, &mut u1); }
        if s.flip_y { std::mem::swap(&mut v0, &mut v1); }

        let corners = [
            Vec2::new(-s.size.x*0.5, -s.size.y*0.5),
            Vec2::new( s.size.x*0.5, -s.size.y*0.5),
            Vec2::new( s.size.x*0.5,  s.size.y*0.5),
            Vec2::new(-s.size.x*0.5,  s.size.y*0.5),
        ];
        let uvs = [[u0,v0],[u1,v0],[u1,v1],[u0,v1]];
        let (sin, cos) = s.rotation.sin_cos();
        let z = s.layer as f32 / 255.0;

        for (i, c) in corners.iter().enumerate() {
            let rx = c.x * cos - c.y * sin + s.position.x;
            let ry = c.x * sin + c.y * cos + s.position.y;
            self.vertices.push(Vertex2D {
                position: [rx, ry], uv: uvs[i],
                color: s.color.to_array(), z, _pad: 0.0,
            });
        }
        self.indices.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
    }

    fn push_line(&mut self, a: Vec2, b: Vec2, thick: f32, color: Color) {
        let dir  = (b - a).normalize_or_zero();
        let perp = Vec2::new(-dir.y, dir.x) * thick * 0.5;
        let col  = color.to_array();
        let base = self.vertices.len() as u32;
        for (i, p) in [a+perp, a-perp, b-perp, b+perp].iter().enumerate() {
            let uv = [[0f32,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]][i];
            self.vertices.push(Vertex2D { position: p.to_array(), uv, color: col, z: 0.5, _pad: 0.0 });
        }
        self.indices.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
    }

    fn push_rect(&mut self, pos: Vec2, size: Vec2, color: Color, filled: bool, thick: f32) {
        if filled {
            self.push_sprite(&Sprite::new(pos + size*0.5, size).with_color(color));
        } else {
            let (x,y,w,h) = (pos.x, pos.y, size.x, size.y);
            self.push_line(Vec2::new(x,y),     Vec2::new(x+w,y),   thick, color);
            self.push_line(Vec2::new(x+w,y),   Vec2::new(x+w,y+h), thick, color);
            self.push_line(Vec2::new(x+w,y+h), Vec2::new(x,y+h),   thick, color);
            self.push_line(Vec2::new(x,y+h),   Vec2::new(x,y),     thick, color);
        }
    }

    fn push_circle(&mut self, center: Vec2, r: f32, color: Color, segs: u32) {
        let col  = color.to_array();
        let base = self.vertices.len() as u32;
        let step = std::f32::consts::TAU / segs as f32;
        self.vertices.push(Vertex2D { position: center.to_array(), uv:[0.5,0.5], color:col, z:0.5, _pad:0.0 });
        for i in 0..=segs {
            let a = i as f32 * step;
            self.vertices.push(Vertex2D {
                position: [center.x + r*a.cos(), center.y + r*a.sin()],
                uv: [(a.cos()+1.0)*0.5, (a.sin()+1.0)*0.5],
                color: col, z: 0.5, _pad: 0.0,
            });
        }
        for i in 0..segs {
            self.indices.extend_from_slice(&[base, base+1+i, base+2+i]);
        }
    }

    fn push_tilemap(&mut self, tm: &Tilemap) {
        let atlas_cols = tm.cols as f32;
        let fw = 1.0 / atlas_cols;
        // Zakładamy kwadratowy atlas; jeśli nie — dodaj atlas_rows
        let fh = fw;
        let z  = tm.layer as f32 / 255.0;

        for y in 0..tm.map_height {
            for x in 0..tm.map_width {
                let tile = tm.tiles[(y * tm.map_width + x) as usize];
                if tile == u32::MAX { continue; }

                let col_f = (tile % tm.cols) as f32;
                let row_f = (tile / tm.cols) as f32;
                let uv = [col_f*fw, row_f*fh, (col_f+1.0)*fw, (row_f+1.0)*fh];

                let pos = tm.offset + Vec2::new(x as f32 * tm.tile_size, y as f32 * tm.tile_size);
                let center = pos + Vec2::splat(tm.tile_size * 0.5);
                let base = self.vertices.len() as u32;
                let hw = tm.tile_size * 0.5;
                let corners = [[-hw,-hw],[hw,-hw],[hw,hw],[-hw,hw]];
                let uvs = [[uv[0],uv[1]],[uv[2],uv[1]],[uv[2],uv[3]],[uv[0],uv[3]]];
                for i in 0..4 {
                    self.vertices.push(Vertex2D {
                        position: [center.x + corners[i][0], center.y + corners[i][1]],
                        uv: uvs[i], color: [1.0;4], z, _pad: 0.0,
                    });
                }
                self.indices.extend_from_slice(&[base,base+1,base+2,base,base+2,base+3]);
            }
        }
    }

    fn push_nineslice(&mut self, ns: &Nineslice, pos: Vec2, size: Vec2, color: Color) {
        let [bl, br, bt, bb] = ns.border_uv;
        let [pl, pr, pt, pb] = ns.border_px;
        let col = color.to_array();

        // 9 quads: TL, T, TR, L, C, R, BL, B, BR
        let xs = [pos.x, pos.x+pl, pos.x+size.x-pr, pos.x+size.x];
        let ys = [pos.y, pos.y+pt, pos.y+size.y-pb, pos.y+size.y];
        let us = [0.0, bl, 1.0-br, 1.0];
        let vs = [0.0, bt, 1.0-bb, 1.0];

        for row in 0..3usize {
            for col_i in 0..3usize {
                let x0 = xs[col_i]; let x1 = xs[col_i+1];
                let y0 = ys[row];   let y1 = ys[row+1];
                let u0 = us[col_i]; let u1 = us[col_i+1];
                let v0 = vs[row];   let v1 = vs[row+1];
                if (x1-x0).abs() < 0.5 || (y1-y0).abs() < 0.5 { continue; }
                let base = self.vertices.len() as u32;
                let verts = [
                    ([x0,y0],[u0,v0]),([x1,y0],[u1,v0]),
                    ([x1,y1],[u1,v1]),([x0,y1],[u0,v1]),
                ];
                for (p, uv) in verts {
                    self.vertices.push(Vertex2D { position: p, uv, color: col, z:0.9, _pad:0.0 });
                }
                self.indices.extend_from_slice(&[base,base+1,base+2,base,base+2,base+3]);
            }
        }
    }

    fn push_text(&mut self, text: &str, pos: Vec2, font_size: f32, color: Color, _font: Option<u64>) {
        // Placeholder — monospace pixel bloki dopóki font nie załadowany
        let cw = font_size * 0.55;
        let ch = font_size;
        for (i, _) in text.chars().enumerate() {
            let x = pos.x + i as f32 * (cw + 1.5);
            self.push_sprite(
                &Sprite::new(Vec2::new(x + cw*0.5, pos.y + ch*0.5), Vec2::new(cw, ch))
                    .with_color(color)
            );
        }
    }

    // ── Tekstury internale ────────────────────────────────────────────────────

    fn create_white_texture(&mut self, ctx: &RenderContext) {
        self.upload_texture_raw(ctx, &[255u8; 4], 1, 1, WHITE_TEX);
    }

    fn upload_texture(&mut self, ctx: &RenderContext, data: &[u8], w: u32, h: u32) -> u64 {
        let id = self.next_tex_id; self.next_tex_id += 1;
        self.upload_texture_raw(ctx, data, w, h, id);
        id
    }

    fn upload_texture_raw(&mut self, ctx: &RenderContext, data: &[u8], w: u32, h: u32, id: u64) {
        let device = &ctx.device;
        let tex = device.create_texture(&TextureDescriptor {
            label: Some("Brass Tex"),
            size: Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        ctx.queue.write_texture(
            ImageCopyTexture { texture: &tex, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
            data,
            ImageDataLayout { offset: 0, bytes_per_row: Some(4*w), rows_per_image: Some(h) },
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&TextureViewDescriptor::default());
        let sampler = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..Default::default()
        });
        let bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Tex BG"), layout: &self.texture_bind_layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&view) },
                BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&sampler) },
            ],
        });
        self.textures.insert(id, GpuTexture { texture: tex, view, bind_group: bg, width: w, height: h });
    }
}

// ─── helpers ──────────────────────────────────────────────────────────────────

fn bgle_uniform(binding: u32, vis: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding, visibility: vis,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false, min_binding_size: None,
        },
        count: None,
    }
}

fn vertex_layout() -> VertexBufferLayout<'static> {
    static ATTRIBS: &[VertexAttribute] = &vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x2,
    ];
    VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex2D>() as BufferAddress,
        step_mode:    VertexStepMode::Vertex,
        attributes:   ATTRIBS,
    }
}

// ─── WGSL — główny shader ─────────────────────────────────────────────────────

const SHADER_2D: &str = r#"
struct Camera { view_proj: mat4x4<f32> }
@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var t_tex: texture_2d<f32>;
@group(1) @binding(1) var s_tex: sampler;

struct Light { pos: vec2<f32>, radius: f32, intensity: f32, color: vec4<f32> }
struct Lights {
    lights:  array<Light, 32>,
    count:   u32,
    ambient: vec3<f32>,
}
@group(2) @binding(0) var<uniform> lights: Lights;

struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) col: vec4<f32>, @location(3) zp: vec2<f32> }
struct VOut { @builtin(position) cp: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) col: vec4<f32>, @location(2) wpos: vec2<f32> }

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    o.cp   = cam.view_proj * vec4<f32>(v.pos, v.zp.x, 1.0);
    o.uv   = v.uv;
    o.col  = v.col;
    o.wpos = v.pos;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    let tex = textureSample(t_tex, s_tex, i.uv) * i.col;

    // Lighting
    var light_acc = lights.ambient;
    for (var li: u32 = 0u; li < lights.count; li++) {
        let l    = lights.lights[li];
        let dist = length(l.pos - i.wpos);
        let att  = clamp(1.0 - dist / l.radius, 0.0, 1.0);
        light_acc += l.color.rgb * l.intensity * att * att;
    }

    return vec4<f32>(tex.rgb * light_acc, tex.a);
}
"#;

// ─── WGSL — light pass (additive) ────────────────────────────────────────────

const SHADER_LIGHT: &str = r#"
struct Camera { view_proj: mat4x4<f32> }
@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var t_tex: texture_2d<f32>;
@group(1) @binding(1) var s_tex: sampler;

struct Light { pos: vec2<f32>, radius: f32, intensity: f32, color: vec4<f32> }
struct Lights { lights: array<Light, 32>, count: u32, ambient: vec3<f32> }
@group(2) @binding(0) var<uniform> lights: Lights;

struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) col: vec4<f32>, @location(3) zp: vec2<f32> }
struct VOut { @builtin(position) cp: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) col: vec4<f32>, @location(2) wpos: vec2<f32> }

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    o.cp   = cam.view_proj * vec4<f32>(v.pos, v.zp.x, 1.0);
    o.uv   = v.uv; o.col = v.col; o.wpos = v.pos;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    var acc = vec3<f32>(0.0);
    for (var li: u32 = 0u; li < lights.count; li++) {
        let l   = lights.lights[li];
        let d   = length(l.pos - i.wpos);
        let att = clamp(1.0 - d / l.radius, 0.0, 1.0);
        acc    += l.color.rgb * l.intensity * att * att;
    }
    return vec4<f32>(acc, 1.0);
}
"#;