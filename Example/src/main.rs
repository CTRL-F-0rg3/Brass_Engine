// =============================================================================
//  rust_bunker — Physics2D + Renderer2D v2 test
//
//  Scena:
//    • Gracz (niebieski) — WASD/strzałki + skok SPACE
//    • Tilemap — podłoga i platformy ze stringa
//    • Pomarańczowa skrzynka — dynamiczny Rigidbody, spada i odbija
//    • Burst cząsteczek przy skoku
//    • Raycast w dół — czerwona linia do podłogi
//    • Smooth camera follow
//    • HUD z pozycją / prędkością / on_ground
// =============================================================================

// physics2d i renderer2d wklejone jako inline moduły
// → po integracji z silnikiem zamień na: use brass_engine::physics2d::*;

mod physics2d;

use std::cell::RefCell;
use std::rc::Rc;

use brass_engine::{
    run, AppConfig,
    Renderer2D, Renderer3D, TextureManager, RenderContext,
    Input, Key, World, Vec2, Color,
};

use physics2d::{
    Aabb, Rigidbody, TilemapCollider, PhysicsWorld, PhysicsId,
    raycast_tilemap,
};

// ─── Stałe ────────────────────────────────────────────────────────────────────

const SCREEN_W: f32 = 1280.0;
const SCREEN_H: f32 = 720.0;
const TILE:     f32 = 40.0;

// '#' = solid tile, ' ' = pusty
// 20 znaków × 12 wierszy
const MAP: &str = concat!(
    "####################",
    "#                  #",
    "#                  #",
    "#      #####       #",
    "#                  #",
    "#           ####   #",
    "#                  #",
    "#   ###            #",
    "#                  #",
    "#                  #",
    "#                  #",
    "####################",
);
const MAP_W: u32 = 20;
const MAP_H: u32 = 12;

// ─── Particle (minimalna wersja — bez nowego renderera) ───────────────────────

struct Particle {
    pos:      Vec2,
    vel:      Vec2,
    life:     f32,
    max_life: f32,
    size:     f32,
}

impl Particle {
    fn t(&self) -> f32 { (self.life / self.max_life).clamp(0.0, 1.0) }
    fn alive(&self) -> bool { self.life > 0.0 }
}

struct Emitter {
    particles: Vec<Particle>,
}

impl Emitter {
    fn new() -> Self { Self { particles: Vec::new() } }

    fn burst(&mut self, pos: Vec2, count: u32) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        for i in 0..count {
            let r = ((seed.wrapping_mul(1_000_003).wrapping_add(i * 999_983)) % 1000) as f32 / 1000.0;
            let r2 = ((seed.wrapping_mul(999_961).wrapping_add(i * 1_000_033)) % 1000) as f32 / 1000.0;
            let angle = std::f32::consts::PI * (0.7 + r * 0.6);
            let speed = 60.0 + r2 * 100.0;
            self.particles.push(Particle {
                pos, vel: Vec2::new(angle.cos() * speed, angle.sin() * speed),
                life: 0.4 + r * 0.4, max_life: 0.4 + r * 0.4, size: 5.0,
            });
        }
    }

    fn update(&mut self, dt: f32) {
        for p in &mut self.particles {
            p.pos  += p.vel * dt;
            p.vel  *= 0.92;
            p.life -= dt;
        }
        self.particles.retain(|p| p.alive());
    }
}

// ─── GameState ────────────────────────────────────────────────────────────────

struct GameState {
    phys:          PhysicsWorld,
    player_id:     PhysicsId,
    box_id:        PhysicsId,
    tilemap_col:   TilemapCollider,
    emitter:       Emitter,
    cam:           Vec2,     // smooth camera (world-space lewy górny róg)
    jump_cd:       f32,
    floor_dist:    f32,
    anim_t:        f32,
    anim_frame:    u32,
}

impl GameState {
    fn new() -> Self {
        let mut phys = PhysicsWorld::new();

        let tilemap_col = TilemapCollider::from_str(TILE, MAP, Vec2::ZERO);

        // Gracz
        let player_id = phys.add(
            Aabb::from_center(Vec2::new(3.0 * TILE, 10.0 * TILE), Vec2::new(28.0, 36.0)),
            Rigidbody::new().with_gravity_scale(1.0).with_friction(0.7),
        );

        // Skrzynka
        let box_id = phys.add(
            Aabb::from_center(Vec2::new(12.0 * TILE, 2.0 * TILE), Vec2::new(32.0, 32.0)),
            Rigidbody::new().with_gravity_scale(1.0).with_bounce(0.35).with_friction(0.85),
        );

        Self {
            phys, player_id, box_id, tilemap_col,
            emitter: Emitter::new(),
            cam: Vec2::ZERO,
            jump_cd: 0.0,
            floor_dist: 0.0,
            anim_t: 0.0,
            anim_frame: 0,
        }
    }

    fn update(&mut self, input: &Input, dt: f32) {
        // ── Ruch gracza ───────────────────────────────────────────────────────
        let mut dx = 0.0_f32;
        if input.is_key_down(Key::KeyA) || input.is_key_down(Key::ArrowLeft)  { dx -= 1.0; }
        if input.is_key_down(Key::KeyD) || input.is_key_down(Key::ArrowRight) { dx += 1.0; }
        if let Some(rb) = self.phys.get_rb_mut(self.player_id) {
            rb.velocity.x = dx * 220.0;
        }

        // ── Skok ──────────────────────────────────────────────────────────────
        self.jump_cd -= dt;
        let on_ground = self.phys.get_rb(self.player_id).map(|r| r.on_ground).unwrap_or(false);
        if (input.is_key_pressed(Key::Space)
            || input.is_key_pressed(Key::KeyW)
            || input.is_key_pressed(Key::ArrowUp))
            && on_ground && self.jump_cd <= 0.0
        {
            self.phys.impulse(self.player_id, Vec2::new(0.0, -480.0));
            self.jump_cd = 0.25;
            if let Some(aabb) = self.phys.get_aabb(self.player_id) {
                self.emitter.burst(aabb.center() + Vec2::new(0.0, aabb.size.y * 0.5), 14);
            }
        }

        // ── Fizyka ────────────────────────────────────────────────────────────
        self.phys.update(dt, Some(&self.tilemap_col));
        self.emitter.update(dt);

        // ── Kamera smooth follow ───────────────────────────────────────────────
        if let Some(aabb) = self.phys.get_aabb(self.player_id) {
            let target = aabb.center() - Vec2::new(SCREEN_W * 0.5, SCREEN_H * 0.5);
            // Clamp do granic mapy
            let max_x = MAP_W as f32 * TILE - SCREEN_W;
            let max_y = MAP_H as f32 * TILE - SCREEN_H;
            let target = Vec2::new(
                target.x.clamp(0.0, max_x.max(0.0)),
                target.y.clamp(0.0, max_y.max(0.0)),
            );
            self.cam += (target - self.cam) * (7.0 * dt).min(1.0);
        }

        // ── Raycast w dół ─────────────────────────────────────────────────────
        if let Some(aabb) = self.phys.get_aabb(self.player_id) {
            self.floor_dist = raycast_tilemap(
                &self.tilemap_col, aabb.center(), Vec2::new(0.0, 1.0), 600.0,
            ).map(|h| (h.distance - aabb.size.y * 0.5).max(0.0))
             .unwrap_or(999.0);
        }

        // ── Animacja klatki ────────────────────────────────────────────────────
        self.anim_t += dt;
        if self.anim_t > 0.12 { self.anim_t = 0.0; self.anim_frame = (self.anim_frame + 1) % 4; }
    }

    fn draw(&self, r2d: &mut Renderer2D) {
        let off = self.cam;

        // ── Tło — siatka ──────────────────────────────────────────────────────
        let step = 64.0_f32;
        let cols = (SCREEN_W / step) as i32 + 3;
        let rows = (SCREEN_H / step) as i32 + 3;
        let sx = (off.x / step).floor() as i32 - 1;
        let sy = (off.y / step).floor() as i32 - 1;
        for i in 0..=cols {
            let x = (sx + i) as f32 * step - off.x;
            r2d.draw_line(Vec2::new(x, 0.0), Vec2::new(x, SCREEN_H),
                1.0, Color::rgba(0.13, 0.13, 0.17, 1.0));
        }
        for i in 0..=rows {
            let y = (sy + i) as f32 * step - off.y;
            r2d.draw_line(Vec2::new(0.0, y), Vec2::new(SCREEN_W, y),
                1.0, Color::rgba(0.13, 0.13, 0.17, 1.0));
        }

        // ── Tilemap ───────────────────────────────────────────────────────────
        for ty in 0..MAP_H {
            for tx in 0..MAP_W {
                if !self.tilemap_col.get_tile(tx, ty) { continue; }
                let sx = tx as f32 * TILE - off.x;
                let sy = ty as f32 * TILE - off.y;
                // Wypełnienie
                r2d.draw_rect(Vec2::new(sx, sy), Vec2::splat(TILE),
                    Color::rgba(0.25, 0.52, 0.28, 1.0), true);
                // Obramowanie
                r2d.draw_rect(Vec2::new(sx, sy), Vec2::splat(TILE),
                    Color::rgba(0.18, 0.38, 0.20, 1.0), false);
            }
        }

        // ── Skrzynka ──────────────────────────────────────────────────────────
        if let Some(aabb) = self.phys.get_aabb(self.box_id) {
            let sp = aabb.position - off;
            r2d.draw_rect(sp, aabb.size, Color::rgba(0.75, 0.45, 0.15, 1.0), true);
            r2d.draw_rect(sp, aabb.size, Color::rgba(1.0, 0.65, 0.25, 1.0), false);
        }

        // ── Gracz ─────────────────────────────────────────────────────────────
        if let Some(aabb) = self.phys.get_aabb(self.player_id) {
            let sp     = aabb.position - off;
            let center = aabb.center() - off;
            let on_g   = self.phys.get_rb(self.player_id).map(|r| r.on_ground).unwrap_or(false);

            // Kolor ciała — jasnoniebieski gdy w powietrzu, ciemniejszy na ziemi
            let body_col = if on_g {
                Color::rgba(0.15, 0.40, 0.95, 1.0)
            } else {
                Color::rgba(0.25, 0.55, 1.0, 1.0)
            };

            // Prosta animacja — lekkie "squash" przy lądowaniu (symulowane zmianą koloru)
            let tint_offs = [0.0f32, 0.05, 0.08, 0.05];
            let t = tint_offs[self.anim_frame as usize];
            let col = Color::rgba(
                (body_col.r + t).min(1.0),
                (body_col.g + t).min(1.0),
                (body_col.b + t).min(1.0),
                1.0,
            );
            r2d.draw_rect(sp, aabb.size, col, true);

            // Oczy
            r2d.draw_rect(center + Vec2::new(-9.0, -10.0), Vec2::new(7.0, 7.0), Color::WHITE, true);
            r2d.draw_rect(center + Vec2::new( 2.0, -10.0), Vec2::new(7.0, 7.0), Color::WHITE, true);
            // Źrenice
            r2d.draw_rect(center + Vec2::new(-7.0, -8.0),  Vec2::new(3.0, 3.0), Color::BLACK, true);
            r2d.draw_rect(center + Vec2::new( 4.0, -8.0),  Vec2::new(3.0, 3.0), Color::BLACK, true);

            // Raycast linia w dół
            r2d.draw_line(
                center,
                center + Vec2::new(0.0, self.floor_dist),
                1.5,
                Color::rgba(1.0, 0.25, 0.25, 0.45),
            );
        }

        // ── Cząsteczki ────────────────────────────────────────────────────────
        for p in &self.emitter.particles {
            let t   = p.t();
            let sp  = p.pos - off;
            let col = Color::rgba(0.4, 0.8 - t * 0.4, 1.0, 1.0 - t);
            r2d.draw_circle(sp, p.size * (1.0 - t * 0.7), col);
        }

        // ── HUD ───────────────────────────────────────────────────────────────
        // Tło panelu
        r2d.draw_rect(Vec2::new(8.0, 8.0), Vec2::new(240.0, 90.0),
            Color::rgba(0.0, 0.0, 0.0, 0.6), true);
        r2d.draw_rect(Vec2::new(8.0, 8.0), Vec2::new(240.0, 90.0),
            Color::rgba(0.35, 0.35, 0.45, 1.0), false);

        let pos = self.phys.get_aabb(self.player_id)
            .map(|a| a.center()).unwrap_or(Vec2::ZERO);
        let vel = self.phys.get_rb(self.player_id)
            .map(|r| r.velocity).unwrap_or(Vec2::ZERO);
        let on_g = self.phys.get_rb(self.player_id)
            .map(|r| r.on_ground).unwrap_or(false);

        r2d.draw_text("WASD + SPACE = skok",
            Vec2::new(16.0, 16.0), 9.0, Color::rgba(0.75, 0.75, 0.75, 1.0));
        r2d.draw_text(&format!("pos  x:{:.0} y:{:.0}", pos.x, pos.y),
            Vec2::new(16.0, 34.0), 9.0, Color::rgba(0.4, 1.0, 0.5, 1.0));
        r2d.draw_text(&format!("vel  x:{:.0} y:{:.0}", vel.x, vel.y),
            Vec2::new(16.0, 50.0), 9.0, Color::rgba(0.4, 0.7, 1.0, 1.0));
        r2d.draw_text(
            &format!("ground:{}  floor:{:.0}px", on_g, self.floor_dist),
            Vec2::new(16.0, 66.0), 9.0, Color::rgba(0.7, 0.7, 0.7, 1.0),
        );

        // Crosshair
        let (cx, cy) = (SCREEN_W * 0.5, SCREEN_H * 0.5);
        r2d.draw_line(Vec2::new(cx-10.0, cy), Vec2::new(cx+10.0, cy),
            1.0, Color::rgba(1.0, 1.0, 1.0, 0.25));
        r2d.draw_line(Vec2::new(cx, cy-10.0), Vec2::new(cx, cy+10.0),
            1.0, Color::rgba(1.0, 1.0, 1.0, 0.25));
    }
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() {
    env_logger::init();

    let state: Rc<RefCell<Option<GameState>>> = Rc::new(RefCell::new(None));
    let state_start  = Rc::clone(&state);
    let state_update = Rc::clone(&state);

    run(
        AppConfig {
            title:  "rust_bunker — Physics2D test".to_string(),
            width:  SCREEN_W as u32,
            height: SCREEN_H as u32,
        },

        move |_world, _r2d, _r3d, _tex, _ctx| {
            *state_start.borrow_mut() = Some(GameState::new());
        },

        move |_world, r2d, _r3d, _tex, input, dt| {
            let mut guard = state_update.borrow_mut();
            let s = match guard.as_mut() { Some(s) => s, None => return };
            s.update(input, dt);
            s.draw(r2d);
        },
    );
}