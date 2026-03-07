# ⚙️ Brass Engine

A 2D game engine for Rust built on **wgpu**, **winit**, and a lightweight **ECS** with a scriptable component system.

```toml
[dependencies]
brass_engine = "0.1.0"
```

---

## Quick Start

```rust
use brass_engine::{run, AppConfig, World, Transform, RigidBody, SpriteComp, Vec2};

fn main() {
    run(
        AppConfig { title: "My Game".into(), width: 1280, height: 720 },

        |world| {
            // Spawn entities once at startup
            let player = world.spawn();
            world
                .add_transform(player, Transform::new(640.0, 360.0))
                .add_sprite(player, SpriteComp::new(64.0, 64.0).with_color(0.2, 0.8, 1.0, 1.0));
        },

        |world, renderer, dt| {
            // Called every frame — draw and update here
        },
    );
}
```

---

## Table of Contents

- [App Setup](#app-setup)
- [Renderer 2D](#renderer-2d)
- [ECS — World & Entities](#ecs--world--entities)
- [Components](#components)
- [Scripts](#scripts)
- [Physics](#physics)
- [Systems — Execution Order](#systems--execution-order)

---

## App Setup

```rust
use brass_engine::{run, AppConfig};

run(
    AppConfig {
        title:  "Game Title".into(),
        width:  1280,
        height: 720,
    },
    |world| {
        // on_start — runs once after GPU init
        // spawn your entities here
    },
    |world, renderer, dt| {
        // on_update — runs every frame
        // dt = delta time in seconds (f32)
    },
);
```

`on_start` gives you `&mut World`.  
`on_update` gives you `&mut World`, `&mut Renderer2D`, and `dt: f32`.

---

## Renderer 2D

The renderer collects draw calls every frame and flushes them in a single GPU batch.  
Call draw functions inside `on_update` or from scripts.

### Sprites & Quads

```rust
use brass_engine::{Sprite, Color, Vec2};

// Colored quad
renderer.draw_sprite(
    Sprite::new(Vec2::new(200.0, 200.0), Vec2::new(64.0, 64.0))
        .with_color(Color::RED)
        .with_rotation(0.5)          // radians
        .with_z(0.8),                // draw order — 0.0 back, 1.0 front
);

// Textured sprite
let tex_id = renderer.load_texture_bytes(ctx, include_bytes!("player.png"));
renderer.draw_sprite(
    Sprite::new(pos, size)
        .with_texture(tex_id)
        .with_uv(0.0, 0.0, 0.5, 1.0), // UV rect for sprite sheets
);
```

### Primitives

```rust
// Line
renderer.draw_line(
    Vec2::new(0.0, 0.0),
    Vec2::new(400.0, 400.0),
    2.0,          // thickness in pixels
    Color::WHITE,
);

// Filled rectangle
renderer.draw_rect(Vec2::new(100.0, 100.0), Vec2::new(200.0, 80.0), Color::CYAN, true);

// Rectangle outline with custom thickness
renderer.draw_rect_ex(Vec2::new(100.0, 100.0), Vec2::new(200.0, 80.0), Color::WHITE, false, 3.0);

// Circle (32 segments by default)
renderer.draw_circle(Vec2::new(640.0, 360.0), 80.0, Color::MAGENTA);

// Circle with custom segment count
renderer.draw_circle_ex(Vec2::new(640.0, 360.0), 80.0, Color::BLUE, 64);
```

### Text

```rust
// Placeholder — renders colored rectangles per character
// Font rendering (fontdue / ab_glyph) planned for a future release
renderer.draw_text("Hello World", Vec2::new(10.0, 10.0), 20.0, Color::WHITE);
```

### Color

```rust
Color::WHITE
Color::BLACK
Color::RED
Color::GREEN
Color::BLUE
Color::YELLOW
Color::CYAN
Color::MAGENTA

Color::rgba(0.2, 0.8, 1.0, 1.0)   // r, g, b, a — range 0.0–1.0
Color::hex(0xFF8C00)                // hex RGB
```

---

## ECS — World & Entities

Every game object is an `Entity` — a lightweight `u64` handle.  
Components are stored in `World` and accessed by entity ID.

```rust
// Spawn an entity
let e = world.spawn();

// Add components (chainable)
world
    .add_transform(e, Transform::new(x, y))
    .add_rigidbody(e, RigidBody::new())
    .add_sprite(e, SpriteComp::new(64.0, 64.0))
    .add_tag(e, "player");

// Get components
let t:  Option<&Transform>     = world.get_transform(e);
let rb: Option<&mut RigidBody> = world.get_rigidbody_mut(e);
let s:  Option<&mut SpriteComp> = world.get_sprite_mut(e);

// Move entity directly (no physics)
world.translate(e, Vec2::new(5.0, 0.0));

// Distance between two entities
let dist: Option<f32> = world.distance(a, b);

// Find by tag
let player: Option<Entity>  = world.find_by_tag("player");
let enemies: Vec<Entity>    = world.find_all_by_tag("enemy");

// Schedule entity for removal (safe to call inside scripts)
// Entity is removed at end of frame, not immediately
world.destroy(e);

// Get all living entities
let all: Vec<Entity> = world.entities();
```

---

## Components

### Transform

Position, scale and rotation of an entity.

```rust
Transform::new(x, y)
    .with_scale(2.0, 2.0)   // default: (1.0, 1.0)
    .with_rotation(0.5)      // radians, default: 0.0

// Fields
transform.position  // Vec2
transform.scale     // Vec2
transform.rotation  // f32 radians
```

### RigidBody

Physics data — velocity, acceleration, damping.

```rust
RigidBody::new()
    .with_velocity(200.0, 0.0)  // initial velocity
    .with_damping(0.05)          // 0.0 = no drag, 1.0 = instant stop
    .stationary()                // dynamic = false — physics won't move it

// Impulse — instant velocity change (one frame)
rigidbody.apply_impulse(Vec2::new(0.0, -500.0));

// Force — adds to acceleration (continuous, resets each frame)
rigidbody.apply_force(Vec2::new(0.0, 9.81));

// Direct access
rigidbody.velocity     // Vec2
rigidbody.acceleration // Vec2
rigidbody.damping      // f32
rigidbody.dynamic      // bool
```

### SpriteComp

Visual data — links an entity to the renderer.

```rust
SpriteComp::new(width, height)
    .with_color(r, g, b, a)      // RGBA 0.0–1.0, default: white
    .with_texture(texture_id)    // u64 from load_texture_bytes()
    .with_z(0.5)                 // draw order 0.0 back – 1.0 front

// Fields
sprite.visible     // bool — set false to hide without destroying
sprite.size        // Vec2
sprite.color       // [f32; 4]
sprite.texture_id  // Option<u64>
sprite.uv_rect     // [f32; 4]
```

### Tag

String label for finding entities by name.

```rust
world.add_tag(e, "enemy");
world.find_by_tag("enemy");       // first match
world.find_all_by_tag("enemy");   // all matches
```

---

## Scripts

Scripts attach behavior to entities. Two styles are supported.

### Closure Script — simple, stateless

```rust
world.add_script_fn(entity, |entity, world, dt| {
    // Runs every frame for this entity
    if let Some(rb) = world.get_rigidbody_mut(entity) {
        rb.apply_impulse(Vec2::new(100.0 * dt, 0.0));
    }
});
```

### Trait Script — complex, with own state

```rust
use brass_engine::{Script, Entity, World};

struct PlayerController {
    speed:     f32,
    jump_held: bool,
}

impl Script for PlayerController {
    fn on_start(&mut self, entity: Entity, world: &mut World) {
        // Called once when entity first runs
        if let Some(rb) = world.get_rigidbody_mut(entity) {
            rb.velocity = Vec2::new(self.speed, 0.0);
        }
    }

    fn on_update(&mut self, entity: Entity, world: &mut World, dt: f32) {
        // Called every frame
    }

    fn on_destroy(&mut self, entity: Entity, world: &mut World) {
        // Called when entity is destroyed
    }
}

// Attach to entity
use brass_engine::ScriptComponent;

let mut sc = ScriptComponent::new();
sc.add(PlayerController { speed: 300.0, jump_held: false });
world.add_script_component(entity, sc);
```

### Multiple scripts per entity

```rust
let mut sc = ScriptComponent::new();
sc.add(PlayerController { speed: 300.0, jump_held: false });
sc.add(HealthSystem { hp: 100 });
sc.add_fn(|e, world, dt| { /* closure alongside trait scripts */ });
world.add_script_component(entity, sc);
```

---

## Physics

Physics runs automatically each frame on every entity that has both a `Transform` and a `RigidBody`.

```
velocity     += acceleration * dt
position     += velocity * dt
velocity     *= (1.0 - damping)
acceleration  = Vec2::ZERO   ← reset each frame
```

Set `rigidbody.dynamic = false` to make an entity static — it won't be moved by the physics system.

---

## Systems — Execution Order

Each frame runs in this fixed order automatically:

```
1. script_system     — on_start (once) + on_update for all ScriptComponents
2. physics_system    — integrate velocity → position
3. on_update         — your callback (extra draw calls, game logic)
4. render_sync_system — write Transform + SpriteComp to Renderer2D
5. renderer.render() — GPU flush
6. cleanup_system    — remove entities queued with world.destroy()
```

You can also call the systems manually if you need a custom loop:

```rust
use brass_engine::{script_system, physics_system, render_sync_system, cleanup_system};

script_system(&mut world, dt);
physics_system(&mut world, dt);
render_sync_system(&world, &mut renderer);
cleanup_system(&mut world);
```

---

## License

MIT — see [LICENSE](LICENSE)