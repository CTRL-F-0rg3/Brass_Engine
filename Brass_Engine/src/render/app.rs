use std::sync::Arc;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{WindowEvent, StartCause, DeviceEvent, DeviceId},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{Window, WindowId},
};

use super::context::RenderContext;
use super::renderer2d::Renderer2D;
use super::renderer3d::Renderer3D;
use super::texture_manager::TextureManager;
use crate::ecs::world::World;
use crate::ecs::systems::{script_system, physics_system, render_sync_system, cleanup_system};
use crate::input::input::{Input, MouseButton};

// ─── AppConfig ────────────────────────────────────────────────────────────────

pub struct AppConfig {
    pub title:  String,
    pub width:  u32,
    pub height: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { title: "Brass Engine".to_string(), width: 1280, height: 720 }
    }
}

// ─── run() ────────────────────────────────────────────────────────────────────

/// Uruchom silnik.
///
/// - `on_start`  — `(world, renderer2d, renderer3d, textures)` — spawn + ładowanie assetów
/// - `on_update` — `(world, renderer2d, renderer3d, textures, input, dt)` — logika + draw calls
pub fn run<S, U>(config: AppConfig, on_start: S, on_update: U)
where
    S: FnOnce(&mut World, &mut Renderer2D, &mut Renderer3D, &mut TextureManager, &RenderContext) + 'static,
    U: FnMut(&mut World, &mut Renderer2D, &mut Renderer3D, &mut TextureManager, &Input, f32) + 'static,
{
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = BrassApp {
        window:    None,
        ctx:       None,
        r2d:       None,
        r3d:       None,
        textures:  None,
        world:     World::new(),
        input:     Input::new(),
        config,
        on_start:  Some(Box::new(on_start)),
        on_update: Box::new(on_update),
        last_time: Instant::now(),
    };

    event_loop.run_app(&mut app).unwrap();
}

// ─── BrassApp ─────────────────────────────────────────────────────────────────

struct BrassApp {
    window:    Option<Arc<Window>>,
    ctx:       Option<RenderContext>,
    r2d:       Option<Renderer2D>,
    r3d:       Option<Renderer3D>,
    textures:  Option<TextureManager>,
    world:     World,
    input:     Input,
    config:    AppConfig,
    on_start:  Option<Box<dyn FnOnce(&mut World, &mut Renderer2D, &mut Renderer3D, &mut TextureManager, &RenderContext)>>,
    on_update: Box<dyn FnMut(&mut World, &mut Renderer2D, &mut Renderer3D, &mut TextureManager, &Input, f32)>,
    last_time: Instant,
}

impl ApplicationHandler for BrassApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(&self.config.title)
                    .with_inner_size(winit::dpi::LogicalSize::new(
                        self.config.width, self.config.height,
                    )),
            ).unwrap(),
        );

        let ctx      = pollster::block_on(RenderContext::new(window.clone()));
        let r2d      = Renderer2D::new(&ctx);
        let r3d      = Renderer3D::new(&ctx);
        let textures = TextureManager::new(&ctx.device, &ctx.queue);

        self.window   = Some(window);

        // Tymczasowo wyciągamy żeby przekazać do on_start
        self.r2d      = Some(r2d);
        self.r3d      = Some(r3d);
        self.textures = Some(textures);
        self.ctx      = Some(ctx);

        if let Some(start_fn) = self.on_start.take() {
            let ctx      = self.ctx.as_ref().unwrap();
            let r2d      = self.r2d.as_mut().unwrap();
            let r3d      = self.r3d.as_mut().unwrap();
            let textures = self.textures.as_mut().unwrap();
            start_fn(&mut self.world, r2d, r3d, textures, ctx);
        }

        self.last_time = Instant::now();
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        // Raw mouse delta — niezależny od pozycji kursora (dobry do FPS camera)
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            self.input.on_mouse_move(
                self.input.mouse_position().x + dx as f32,
                self.input.mouse_position().y + dy as f32,
            );
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if cause == StartCause::Poll {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(new_size) => {
                if let Some(ctx) = &mut self.ctx {
                    ctx.resize(new_size);
                    if let Some(r2d) = &mut self.r2d { r2d.resize(ctx); }
                    if let Some(r3d) = &mut self.r3d { r3d.resize(ctx); }
                }
            }

            // ── Klawiatura ────────────────────────────────────────────────────
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    match event.state {
                        winit::event::ElementState::Pressed  => self.input.on_key_down(key),
                        winit::event::ElementState::Released => self.input.on_key_up(key),
                    }
                }
            }

            // ── Mysz ──────────────────────────────────────────────────────────
            WindowEvent::CursorMoved { position, .. } => {
                self.input.on_mouse_move(position.x as f32, position.y as f32);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let btn = MouseButton::from(button);
                match state {
                    winit::event::ElementState::Pressed  => self.input.on_mouse_down(btn),
                    winit::event::ElementState::Released => self.input.on_mouse_up(btn),
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p)   => p.y as f32 * 0.1,
                };
                self.input.on_scroll(scroll);
            }

            // ── Render ────────────────────────────────────────────────────────
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt  = now.duration_since(self.last_time).as_secs_f32().min(0.05);
                self.last_time = now;

                if let (Some(ctx), Some(r2d), Some(r3d), Some(textures)) = (
                    &mut self.ctx,
                    &mut self.r2d,
                    &mut self.r3d,
                    &mut self.textures,
                ) {
                    // 1. Skrypty + fizyka
                    script_system(&mut self.world, dt);
                    physics_system(&mut self.world, dt);

                    // 2. Callback użytkownika
                    (self.on_update)(&mut self.world, r2d, r3d, textures, &self.input, dt);

                    // 3. Sync ECS → Renderer2D
                    render_sync_system(&self.world, r2d);

                    // 4. GPU render
                    match ctx.surface.get_current_texture() {
                        Ok(output) => {
                            let view = output.texture.create_view(
                                &wgpu::TextureViewDescriptor::default()
                            );

                            // 3D najpierw (clear), 2D na wierzch (load)
                            r3d.render(ctx, &view, textures);

                            match r2d.render_to_view(ctx, &view) {
                                Ok(_) => {}
                                Err(e) => eprintln!("[Brass 2D] {e:?}"),
                            }

                            output.present();
                        }
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            ctx.resize(ctx.size);
                            r2d.resize(ctx);
                            r3d.resize(ctx);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            eprintln!("[Brass] OutOfMemory");
                            event_loop.exit();
                        }
                        Err(e) => eprintln!("[Brass] {e:?}"),
                    }

                    // 5. Cleanup + flush input
                    cleanup_system(&mut self.world);
                    self.input.flush();
                }
            }

            _ => {}
        }
    }
}