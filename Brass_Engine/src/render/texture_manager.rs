use std::collections::HashMap;
use wgpu::*;
use wgpu::util::{DeviceExt, TextureDataOrder};

// ─── GpuTexture ────────────────────────────────────────────────────────────────

pub struct GpuTexture {
    #[allow(dead_code)]
    pub texture:    Texture,
    pub view:       TextureView,
    pub bind_group: BindGroup,
}

// ─── TextureManager ───────────────────────────────────────────────────────────

pub struct TextureManager {
    textures:    HashMap<u64, GpuTexture>,
    path_cache:  HashMap<String, u64>,
    next_id:     u64,
    bind_layout: BindGroupLayout,
}

impl TextureManager {
    pub fn new(device: &Device, queue: &Queue) -> Self {
        let bind_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label:   Some("TexManager BGL"),
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
                    count:      None,
                },
            ],
        });

        let mut mgr = Self {
            textures:    HashMap::new(),
            path_cache:  HashMap::new(),
            next_id:     1,
            bind_layout,
        };

        // Biała tekstura 1x1 — ID 0
        mgr.upload_internal(device, queue, &[255u8, 255, 255, 255], 1, 1, 0);
        mgr
    }

    pub fn bind_layout(&self) -> &BindGroupLayout {
        &self.bind_layout
    }

    /// Załaduj z bajtów PNG/JPG. Jeśli key już znany — zwróć istniejące ID.
    pub fn load_bytes(&mut self, device: &Device, queue: &Queue, bytes: &[u8], key: &str) -> u64 {
        if let Some(&id) = self.path_cache.get(key) {
            return id;
        }
        let img    = image::load_from_memory(bytes).expect("Zły obraz").to_rgba8();
        let (w, h) = img.dimensions();
        let id     = self.next_id;
        self.next_id += 1;
        self.upload_internal(device, queue, &img, w, h, id);
        self.path_cache.insert(key.to_string(), id);
        id
    }

    /// Załaduj z surowych RGBA bajtów.
    pub fn load_raw(&mut self, device: &Device, queue: &Queue, data: &[u8], w: u32, h: u32) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.upload_internal(device, queue, data, w, h, id);
        id
    }

    pub fn get(&self, id: u64) -> Option<&GpuTexture> {
        self.textures.get(&id)
    }

    pub fn white(&self) -> &GpuTexture {
        self.textures.get(&0).expect("Brak białej tekstury")
    }

    pub fn remove(&mut self, id: u64) {
        self.textures.remove(&id);
    }

    // ── Wewnętrzne ────────────────────────────────────────────────────────────

    fn upload_internal(&mut self, device: &Device, queue: &Queue, data: &[u8], w: u32, h: u32, id: u64) {
        let texture = device.create_texture_with_data(
            queue,
            &TextureDescriptor {
                label:           Some("Brass Tex"),
                size:            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count:    1,
                dimension:       TextureDimension::D2,
                format:          TextureFormat::Rgba8UnormSrgb,
                usage:           TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats:    &[],
            },
            TextureDataOrder::LayerMajor,
            data,
        );

        let view    = texture.create_view(&TextureViewDescriptor::default());
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
            label:   Some("Tex BG"),
            layout:  &self.bind_layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&view) },
                BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&sampler) },
            ],
        });

        self.textures.insert(id, GpuTexture { texture, view, bind_group });
    }
}