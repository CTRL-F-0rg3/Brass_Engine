use brass_engine::{
    run, AppConfig, World,
    Transform, RigidBody, SpriteComp, ScriptComponent, Script, Entity,
    Vec2,
};

// ─── Przykład 1: Trait Script — gracz z własnym stanem ───────────────────────

struct PlayerController {
    speed: f32,
}

impl Script for PlayerController {
    fn on_start(&mut self, entity: Entity, world: &mut World) {
        // Nadaj startową prędkość przy spawnie
        if let Some(rb) = world.get_rigidbody_mut(entity) {
            rb.velocity = Vec2::new(self.speed, 0.0);
        }
    }

    fn on_update(&mut self, entity: Entity, world: &mut World, _dt: f32) {
        // Odbijaj od krawędzi ekranu
        if let Some(t) = world.get_transform(entity) {
            let pos = t.position;
            if pos.x > 1200.0 || pos.x < 80.0 {
                if let Some(rb) = world.get_rigidbody_mut(entity) {
                    rb.velocity.x = -rb.velocity.x;
                }
            }
        }
    }
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() {
    env_logger::init();

    run(
        AppConfig {
            title:  "Brass Engine — ECS Demo".to_string(),
            width:  1280,
            height: 720,
        },

        // ── on_start: budujemy świat ──────────────────────────────────────────
        |world| {
            // Gracz — trait Script z własnym stanem
            let player = world.spawn();
            world
                .add_transform(player, Transform::new(200.0, 340.0))
                .add_rigidbody(player, RigidBody::new().with_damping(0.0))
                .add_sprite(player, SpriteComp::new(64.0, 64.0).with_color(0.2, 0.8, 1.0, 1.0))
                .add_tag(player, "player");

            let mut sc = ScriptComponent::new();
            sc.add(PlayerController { speed: 300.0 });
            world.add_script_component(player, sc);

            // Kula — closure script, prosta fizyka
            let ball = world.spawn();
            world
                .add_transform(ball, Transform::new(640.0, 200.0))
                .add_rigidbody(ball, RigidBody::new().with_velocity(150.0, 200.0).with_damping(0.0))
                .add_sprite(ball, SpriteComp::new(40.0, 40.0).with_color(1.0, 0.4, 0.1, 1.0))
                .add_tag(ball, "ball");

            // Closure script — odbijanie od ścian i sufitu/podłogi
            world.add_script_fn(ball, |entity, world, _dt| {
                if let Some(t) = world.get_transform(entity) {
                    let pos = t.position;
                    let mut flip_x = false;
                    let mut flip_y = false;
                    if pos.x > 1240.0 || pos.x < 40.0 { flip_x = true; }
                    if pos.y > 680.0  || pos.y < 40.0  { flip_y = true; }

                    if flip_x || flip_y {
                        if let Some(rb) = world.get_rigidbody_mut(entity) {
                            if flip_x { rb.velocity.x = -rb.velocity.x; }
                            if flip_y { rb.velocity.y = -rb.velocity.y; }
                        }
                    }
                }
            });

            // Statyczny blok — brak RigidBody, tylko wizual
            let wall = world.spawn();
            world
                .add_transform(wall, Transform::new(640.0, 650.0))
                .add_sprite(wall, SpriteComp::new(400.0, 30.0).with_color(0.5, 0.5, 0.5, 1.0))
                .add_tag(wall, "wall");
        },

        // ── on_update: dodatkowe draw calls + logika ──────────────────────────
        |world, renderer, _dt| {
            use brass_engine::{Color, Vec2};
            // HUD — rysuj bezpośrednio przez renderer
            renderer.draw_text("ECS Demo — Brass Engine", Vec2::new(10.0, 10.0), 18.0, Color::WHITE);

            // Wizualizacja pozycji gracza
            if let Some(player) = world.find_by_tag("player") {
                if let Some(t) = world.get_transform(player) {
                    renderer.draw_circle(t.position, 6.0, Color::YELLOW);
                }
            }
        },
    );
}