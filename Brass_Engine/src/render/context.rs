use std::sync::Arc;
use wgpu::*;
use winit::window::Window;

/// Niskopoziomowy kontekst GPU — device, queue, skonfigurowany surface.
/// Trzymany przez Renderer2D i Renderer3D.
pub struct RenderContext {
    pub surface:  Surface<'static>,
    pub device:   Device,
    pub queue:    Queue,
    pub config:   SurfaceConfiguration,
    pub size:     winit::dpi::PhysicalSize<u32>,
    pub format:   TextureFormat,
}

impl RenderContext {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone())
            .expect("Nie można utworzyć surface");

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference:       PowerPreference::HighPerformance,
                compatible_surface:     Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Brak kompatybilnego adaptera GPU");

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label:             Some("Brass Device"),
                    required_features: Features::empty(),
                    required_limits:   Limits::default(),
                    ..Default::default()
                },
                None,
            )
            .await
            .expect("Nie można utworzyć device");

        let caps   = surface.get_capabilities(&adapter);
        let format = caps.formats.iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        let config = SurfaceConfiguration {
            usage:                        TextureUsages::RENDER_ATTACHMENT,
            format,
            width:                        size.width.max(1),
            height:                       size.height.max(1),
            present_mode:                 PresentMode::AutoVsync,
            alpha_mode:                   caps.alpha_modes[0],
            view_formats:                 vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Self { surface, device, queue, config, size, format }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size          = new_size;
        self.config.width  = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Zwraca bieżącą rozdzielczość jako (width, height) w f32.
    pub fn viewport(&self) -> (f32, f32) {
        (self.size.width as f32, self.size.height as f32)
    }
}