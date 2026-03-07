use std::sync::Arc;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{WindowEvent, StartCause},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use super::context::RenderContext;
use super::renderer2d::Renderer2D;
use crate::ecs::world::World;
use crate::ecs::systems::{script_system, physics_system, render_sync_system, cleanup_system};

// ─── AppConfig ────────────────────────────────────────────────────────────────

pub struct AppConfig {
    pub title:  String,
    pub width:  u32,
    pub height: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            title:  "Brass Engine".to_string(),
            width:  1280,
            height: 720,
        }
    }
}

// ─── run() ────────────────────────────────────────────────────────────────────

pub fn run<S, U>(config: AppConfig, on_start: S, on_update: U)
where
    S: FnOnce(&mut World) + 'static,
    U: FnMut(&mut World, &mut Renderer2D, f32) + 'static,
{
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = BrassApp {
        window:    None,
        ctx:       None,
        renderer:  None,
        world:     World::new(),
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
    renderer:  Option<Renderer2D>,
    world:     World,
    config:    AppConfig,
    on_start:  Option<Box<dyn FnOnce(&mut World)>>,
    on_update: Box<dyn FnMut(&mut World, &mut Renderer2D, f32)>,
    last_time: Instant,
}

impl ApplicationHandler for BrassApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(&self.config.title)
                    .with_inner_size(winit::dpi::LogicalSize::new(
                        self.config.width,
                        self.config.height,
                    )),
            ).unwrap(),
        );

        let ctx      = pollster::block_on(RenderContext::new(window.clone()));
        let renderer = Renderer2D::new(&ctx);

        self.window   = Some(window);
        self.ctx      = Some(ctx);
        self.renderer = Some(renderer);

        if let Some(start_fn) = self.on_start.take() {
            start_fn(&mut self.world);
        }

        self.last_time = Instant::now();
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
                if let (Some(ctx), Some(renderer)) = (&mut self.ctx, &mut self.renderer) {
                    ctx.resize(new_size);
                    renderer.resize(ctx);
                }
            }

            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt  = now.duration_since(self.last_time).as_secs_f32().min(0.05);
                self.last_time = now;

                if let (Some(ctx), Some(renderer)) = (&mut self.ctx, &mut self.renderer) {
                    script_system(&mut self.world, dt);
                    physics_system(&mut self.world, dt);
                    (self.on_update)(&mut self.world, renderer, dt);
                    render_sync_system(&self.world, renderer);

                    match renderer.render(ctx) {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            ctx.resize(ctx.size);
                            renderer.resize(ctx);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            eprintln!("[Brass] OutOfMemory");
                            event_loop.exit();
                        }
                        Err(e) => eprintln!("[Brass] render error: {e:?}"),
                    }

                    cleanup_system(&mut self.world);
                }
            }

            _ => {}
        }
    }
}