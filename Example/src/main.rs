// =============================================================================
//  Brass Engine — Renderer3D Demo
// =============================================================================

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use brass_engine::{
    run, AppConfig,
    Renderer2D, Renderer3D, TextureManager, RenderContext,
    Mesh, Camera3D, Material, DirectionalLight,
    Input, MouseButton,
    Vec3, Vec4, Mat4, Quat,
};
// PointLight nie jest re-eksportowany w lib.rs — import bezpośrednio z modułu
use brass_engine::render::renderer3d::PointLight;
use brass_engine::ecs::world::World;

// =============================================================================
//  Orbital Camera
// =============================================================================

struct OrbitCamera {
    yaw:      f32,
    pitch:    f32,
    distance: f32,
    target:   Vec3,
}

impl OrbitCamera {
    fn new() -> Self {
        Self { yaw: 30.0, pitch: 25.0, distance: 10.0, target: Vec3::new(0.0, 1.0, 0.0) }
    }

    fn update(&mut self, input: &Input) {
        // is_mouse_down() — poprawna nazwa z input.rs
        if input.is_mouse_down(MouseButton::Left) {
            let d = input.mouse_delta();
            self.yaw   += d.x * 0.4;
            self.pitch  = (self.pitch - d.y * 0.4).clamp(-89.0, 89.0);
        }
        let scroll = input.scroll();
        if scroll.abs() > 0.001 {
            self.distance = (self.distance - scroll * 0.8).clamp(1.5, 60.0);
        }
    }

    fn build(&self) -> Camera3D {
        let yr  = self.yaw.to_radians();
        let pr  = self.pitch.to_radians();
        let pos = self.target + Vec3::new(
            self.distance * pr.cos() * yr.sin(),
            self.distance * pr.sin(),
            self.distance * pr.cos() * yr.cos(),
        );
        Camera3D::new(pos, self.target)
    }
}

// =============================================================================
//  DemoState
// =============================================================================

struct DemoState {
    sphere_id: u64,
    cube_id:   u64,
    plane_id:  u64,
    orbit:     OrbitCamera,
    start:     Instant,
}

// =============================================================================
//  main
// =============================================================================

fn main() {
    env_logger::init();

    let state: Rc<RefCell<Option<DemoState>>> = Rc::new(RefCell::new(None));
    let state_start  = Rc::clone(&state);
    let state_update = Rc::clone(&state);

    run(
        AppConfig {
            title:  "Brass Engine — Renderer3D Demo".to_string(),
            width:  1280,
            height: 720,
        },

        // ── on_start ──────────────────────────────────────────────────────────
        move |_world: &mut World,
              _r2d:   &mut Renderer2D,
              r3d:    &mut Renderer3D,
              _tex:   &mut TextureManager,
              ctx:    &RenderContext|
        {
            let sphere_id = r3d.upload_mesh(&ctx.device, &Mesh::sphere(1.0, 32, 32));
            let cube_id   = r3d.upload_mesh(&ctx.device, &Mesh::cube());
            let plane_id  = r3d.upload_mesh(&ctx.device, &Mesh::plane(20.0));

            r3d.dir_light = DirectionalLight::new(
                Vec3::new(-0.4, -1.0, -0.6),
                Vec3::new(1.0, 0.95, 0.85),
                2.0,
            );
            r3d.ambient = Vec3::new(0.03, 0.03, 0.05);

            r3d.point_lights = vec![
                PointLight::new(Vec3::ZERO, Vec3::new(1.0, 0.2, 0.1), 8.0, 12.0),
                PointLight::new(Vec3::ZERO, Vec3::new(0.1, 0.4, 1.0), 8.0, 12.0),
                PointLight::new(Vec3::ZERO, Vec3::new(0.2, 1.0, 0.3), 8.0, 12.0),
                PointLight::new(Vec3::ZERO, Vec3::new(1.0, 0.8, 0.1), 8.0, 12.0),
            ];

            *state_start.borrow_mut() = Some(DemoState {
                sphere_id,
                cube_id,
                plane_id,
                orbit: OrbitCamera::new(),
                start: Instant::now(),
            });
        },

        // ── on_update ─────────────────────────────────────────────────────────
        move |_world: &mut World,
              _r2d:   &mut Renderer2D,
              r3d:    &mut Renderer3D,
              _tex:   &mut TextureManager,
              input:  &Input,
              _dt:    f32|
        {
            let mut guard = state_update.borrow_mut();
            let s = match guard.as_mut() { Some(s) => s, None => return };
            let t = s.start.elapsed().as_secs_f32();

            // ── Kamera ────────────────────────────────────────────────────────
            s.orbit.update(input);
            r3d.camera = s.orbit.build();

            // ── Point lighty — orbita + pulsowanie ───────────────────────────
            for (i, pl) in r3d.point_lights.iter_mut().enumerate() {
                let angle    = t * 0.7 + i as f32 * std::f32::consts::TAU / 4.0;
                pl.position  = Vec3::new(
                    angle.cos() * 5.5,
                    1.5 + (t * 0.9 + i as f32).sin() * 0.6,
                    angle.sin() * 5.5,
                );
                pl.intensity = 5.0 + 3.5 * (t * 1.3 + i as f32 * 1.1).sin();
            }

            // ── Podłoga ───────────────────────────────────────────────────────
            r3d.draw(
                s.plane_id,
                Mat4::IDENTITY,
                Material::pbr(Vec4::new(0.48, 0.48, 0.48, 1.0), 0.0, 0.9),
            );

            // ── Centralna złota sfera ─────────────────────────────────────────
            r3d.draw(
                s.sphere_id,
                Mat4::from_translation(Vec3::new(0.0, 1.3 + (t * 1.2).sin() * 0.18, 0.0)),
                Material::pbr(Vec4::new(1.0, 0.78, 0.05, 1.0), 1.0, 0.12),
            );

            // ── 4 satelitarne sześciany ───────────────────────────────────────
            for i in 0..4u32 {
                let angle = t * 0.55 + i as f32 * std::f32::consts::TAU / 4.0;
                let pos   = Vec3::new(
                    angle.cos() * 3.5,
                    1.0 + (t * 0.8 + i as f32 * 0.9).sin() * 0.4,
                    angle.sin() * 3.5,
                );
                let spin = Quat::from_rotation_y(t * 1.1 + i as f32)
                         * Quat::from_rotation_x(t * 0.65 + i as f32 * 0.5);
                let mat = match i {
                    0 => Material::pbr(Vec4::new(0.8, 0.1, 0.1, 1.0), 0.0, 0.55),
                    1 => Material::pbr(Vec4::new(0.1, 0.3, 0.9, 1.0), 0.9, 0.25),
                    2 => Material::pbr(Vec4::new(0.1, 0.8, 0.2, 1.0), 0.0, 0.65),
                    _ => {
                        let mut m = Material::pbr(Vec4::new(0.85, 0.9, 1.0, 1.0), 1.0, 0.05);
                        m.emissive = Vec3::new(0.2, 0.45, 1.0)
                            * (0.4 + 0.6 * (t * 2.5).sin().abs());
                        m
                    }
                };
                r3d.draw(
                    s.cube_id,
                    Mat4::from_scale_rotation_translation(Vec3::splat(0.42), spin, pos),
                    mat,
                );
            }

            // ── Roughness demo — tył (metallic=1, roughness 0→1) ─────────────
            for i in 0..6u32 {
                r3d.draw(
                    s.sphere_id,
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.35), Quat::IDENTITY,
                        Vec3::new(-2.5 + i as f32, 0.35, -3.8),
                    ),
                    Material::pbr(Vec4::new(0.9, 0.9, 0.9, 1.0), 1.0, (i as f32 / 5.0).max(0.04)),
                );
            }

            // ── Metallic demo — przód (roughness=0.4, metallic 0→1) ──────────
            for i in 0..6u32 {
                r3d.draw(
                    s.sphere_id,
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.35), Quat::IDENTITY,
                        Vec3::new(-2.5 + i as f32, 0.35, 3.8),
                    ),
                    Material::pbr(Vec4::new(0.7, 0.18, 0.08, 1.0), i as f32 / 5.0, 0.4),
                );
            }

            // ── Emissive kula na środku ───────────────────────────────────────
            {
                let mut m = Material::pbr(Vec4::new(0.1, 0.3, 1.0, 1.0), 0.0, 1.0);
                m.emissive = Vec3::new(0.1, 0.4, 2.0) * (0.5 + 0.5 * (t * 4.0).sin());
                r3d.draw(
                    s.sphere_id,
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.18), Quat::IDENTITY,
                        Vec3::new(0.0, 0.18, 0.0),
                    ),
                    m,
                );
            }
        },
    );
}