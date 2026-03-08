# ⚙️ Brass Engine

A 2D/3D game engine for Rust built on **wgpu**, **winit**, with a lightweight **ECS**, scripting system, input handling and texture management.

```toml
[dependencies]
brass_engine = "0.2.0"
```

---

## Quick Start

```rust
use brass_engine::{run, AppConfig, World, Transform, SpriteComp, Vec2, Color, Key};

fn main() {
    run(
        AppConfig { title: "My Game".into(), width: 1280, height: 720 },

        |world, _r2d, _r3d, _textures| {
            let e = world.spawn();
            world
                .add_transform(e, Transform::new(640.0, 360.0))
                .add_sprite(e, SpriteComp::new(64.0, 64.0).with_color(0.2, 0.8, 1.0, 1.0));
        },

        |world, renderer, _r3d, _textures, input, dt| {
            if input.is_key_down(Key::Escape) { std::process::exit(0); }
            renderer.draw_text("Hello Brass!", Vec2::new(10.0, 10.0), 20.0, Color::WHITE);
        },
    );
}
```

---

## Table of Contents

- [App Setup](#app-setup)
- [Input System](#input-system)
- [Renderer 2D](#renderer-2d)
- [Renderer 3D](#renderer-3d)
- [Texture Manager](#texture-manager)
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

    // on_start — runs once after GPU init
    |world, renderer2d, renderer3d, textures| {
        // spawn entities, load textures, upload meshes
    },

    // on_update — runs every frame
    |world, renderer2d, renderer3d, textures, input, dt| {
        // dt = delta time in seconds (f32)
    },
);
```

---

## Input System

`Input` jest dostępny jako szósty argument w `on_update`. To jedyne miejsce gdzie możesz go odczytać — skrypty (`Script::on_update`) dostają tylko `(entity, world, dt)`, dlatego sterowanie najlepiej obsługiwać w `on_update` i aplikować przez komponenty (`RigidBody`, `Transform`).

```rust
run(
    AppConfig { .. },
    |world, r2d, r3d, textures| { /* on_start */ },
    |world, r2d, r3d, textures, input, dt| {
        // Ruch gracza przez RigidBody
        if let Some(player) = world.find_by_tag("player") {
            let dir = input.axis2d(Key::KeyA, Key::KeyD, Key::KeyW, Key::KeyS);
            if let Some(rb) = world.get_rigidbody_mut(player) {
                rb.velocity = dir * 300.0;
            }
        }

        // Jednorazowa akcja
        if let Some(player) = world.find_by_tag("player") {
            if input.is_key_pressed(Key::Space) {
                if let Some(rb) = world.get_rigidbody_mut(player) {
                    rb.apply_impulse(Vec2::new(0.0, -500.0));
                }
            }
        }
    },
);
```

### Klawiatura

```rust
input.is_key_down(Key::KeyW)         // wciśnięty (ciągłe, co klatkę)
input.is_key_pressed(Key::Space)     // wciśnięty tylko w tej klatce
input.is_key_released(Key::KeyE)     // puszczony tylko w tej klatce
```

### Axis helpers

```rust
// -1.0 / 0.0 / 1.0
let x: f32 = input.axis(Key::KeyA, Key::KeyD);

// Vec2 — WASD lub strzałki
let dir: Vec2 = input.axis2d(
    Key::KeyA, Key::KeyD,   // lewo / prawo
    Key::KeyW, Key::KeyS,   // góra / dół
);
// rb.velocity = dir * speed;
```

### Mysz

```rust
input.is_mouse_down(MouseButton::Left)      // przytrzymany
input.is_mouse_pressed(MouseButton::Right)  // kliknięty w tej klatce
input.is_mouse_released(MouseButton::Left)  // puszczony w tej klatce

input.mouse_position()  // Vec2 — pozycja kursora w pikselach
input.mouse_delta()     // Vec2 — ruch od ostatniej klatki (FPS kamera)
input.scroll()          // f32  — kółko myszy (+ góra, - dół)
```

### Przykład: kamera FPS myszą

```rust
if input.is_mouse_down(MouseButton::Right) {
    let delta = input.mouse_delta();
    let cam   = &mut renderer3d.camera;
    let dir   = (cam.target - cam.position).normalize();
    let right = dir.cross(Vec3::Y).normalize();
    let rot_y = Mat4::from_rotation_y(-delta.x * 0.003);
    let rot_p = Mat4::from_axis_angle(right, -delta.y * 0.003);
    let off   = cam.position - cam.target;
    cam.position = cam.target + rot_p.transform_vector3(rot_y.transform_vector3(off));
}

let zoom = input.scroll();
if zoom != 0.0 {
    let dir = (renderer3d.camera.target - renderer3d.camera.position).normalize();
    renderer3d.camera.position += dir * zoom * 0.5;
}
```

Pełna lista klawiszy: każdy wariant z [`winit::keyboard::KeyCode`](https://docs.rs/winit/latest/winit/keyboard/enum.KeyCode.html) działa jako `Key::*`.

---

## Renderer 2D

The 2D renderer collects all draw calls and flushes them in a single GPU batch per frame.  
It renders **on top of** the 3D scene automatically.

### Sprites & Quads

```rust
use brass_engine::{Sprite, Color, Vec2};

// Colored quad
renderer2d.draw_sprite(
    Sprite::new(Vec2::new(200.0, 200.0), Vec2::new(64.0, 64.0))
        .with_color(Color::RED)
        .with_rotation(0.5)           // radians
        .with_z(0.8),                 // draw order: 0.0 = back, 1.0 = front
);

// Textured sprite
let id = textures.load_bytes(&ctx.device, &ctx.queue, include_bytes!("hero.png"), "hero");
renderer2d.draw_sprite(
    Sprite::new(pos, size)
        .with_texture(id)
        .with_uv(0.0, 0.0, 0.5, 1.0),  // UV rect for sprite sheets
);
```

### Primitives

```rust
// Line
renderer2d.draw_line(start, end, thickness, Color::WHITE);

// Filled rectangle
renderer2d.draw_rect(Vec2::new(100.0, 100.0), Vec2::new(200.0, 80.0), Color::CYAN, true);

// Rectangle outline with custom thickness
renderer2d.draw_rect_ex(pos, size, Color::WHITE, false, 3.0);

// Circle — 32 segments default
renderer2d.draw_circle(Vec2::new(640.0, 360.0), 80.0, Color::MAGENTA);

// Circle — custom segments
renderer2d.draw_circle_ex(center, radius, Color::BLUE, 64);

// Text (placeholder — font rendering planned)
renderer2d.draw_text("Hello!", Vec2::new(10.0, 10.0), 20.0, Color::WHITE);
```

### Color

```rust
Color::WHITE | BLACK | RED | GREEN | BLUE | YELLOW | CYAN | MAGENTA

Color::rgba(0.2, 0.8, 1.0, 1.0)   // r, g, b, a  — range 0.0–1.0
Color::hex(0xFF8C00)                // RGB hex
```

---

## Renderer 3D

Blinn-Phong shading with directional light, perspective camera, and per-mesh materials.

```rust
use brass_engine::{Mesh, Material, Camera3D, DirectionalLight, Vec3, Mat4};

// on_start — upload mesh once
let cube_id = renderer3d.upload_mesh(ctx, &Mesh::cube());
let plane_id = renderer3d.upload_mesh(ctx, &Mesh::plane(10.0));

// Set camera
renderer3d.camera = Camera3D::new(
    Vec3::new(0.0, 3.0, 6.0),  // position
    Vec3::ZERO,                  // target
);
renderer3d.camera.fov_y = 60.0; // degrees
renderer3d.camera.near  = 0.1;
renderer3d.camera.far   = 1000.0;

// Set directional light
renderer3d.light = DirectionalLight::new(
    Vec3::new(-0.3, -1.0, -0.5),  // direction
    Vec3::ONE,                      // color (white)
    1.0,                            // intensity
);

// on_update — draw meshes
renderer3d.draw_mesh(
    cube_id,
    Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)),
    Material::color(1.0, 0.3, 0.2),
);

renderer3d.draw_mesh(
    plane_id,
    Mat4::IDENTITY,
    Material::textured(texture_id),
);
```

### Built-in Meshes

```rust
Mesh::cube()          // unit cube centered at origin
Mesh::plane(size)     // flat XZ plane
```

### Material

```rust
Material::color(r, g, b)          // solid color
Material::textured(texture_id)     // uses texture from TextureManager

// Fine control
Material { albedo, texture_id, metallic, roughness }
```

### Camera3D

```rust
Camera3D::new(position, target)
camera.position   // Vec3
camera.target     // Vec3
camera.up         // Vec3 (default Y)
camera.fov_y      // f32 degrees
camera.near       // f32
camera.far        // f32
camera.view_matrix()          // Mat4
camera.proj_matrix(aspect)    // Mat4
camera.view_proj(aspect)      // Mat4 — combined
```

---

## Texture Manager

Central GPU texture cache — each image loaded once, referenced by `u64` ID.

```rust
// Load from embedded bytes (PNG/JPG)
let id = textures.load_bytes(
    &ctx.device, &ctx.queue,
    include_bytes!("assets/hero.png"),
    "hero",   // cache key — same key = same ID returned
);

// Load from raw RGBA bytes
let id = textures.load_raw(&ctx.device, &ctx.queue, &rgba_bytes, width, height);

// Use in 2D sprite
sprite.with_texture(id);

// Use in 3D material
Material::textured(id);

// Free from GPU
textures.remove(id);
```

---

## ECS — World & Entities

```rust
// Spawn
let e = world.spawn();

// Add components (chainable)
world
    .add_transform(e, Transform::new(x, y))
    .add_rigidbody(e, RigidBody::new())
    .add_sprite(e, SpriteComp::new(64.0, 64.0))
    .add_tag(e, "player");

// Access components
let t:  Option<&Transform>      = world.get_transform(e);
let rb: Option<&mut RigidBody>  = world.get_rigidbody_mut(e);
let s:  Option<&mut SpriteComp> = world.get_sprite_mut(e);

// Find by tag
let player: Option<Entity> = world.find_by_tag("player");
let enemies: Vec<Entity>   = world.find_all_by_tag("enemy");

// Move without physics
world.translate(e, Vec2::new(5.0, 0.0));

// Distance between entities
let dist: Option<f32> = world.distance(a, b);

// Schedule removal — safe to call inside scripts, executes end of frame
world.destroy(e);
```

---

## Components

### Transform
```rust
Transform::new(x, y)
    .with_scale(2.0, 2.0)
    .with_rotation(0.5)   // radians

transform.position  // Vec2
transform.scale     // Vec2
transform.rotation  // f32
```

### RigidBody
```rust
RigidBody::new()
    .with_velocity(200.0, 0.0)
    .with_damping(0.05)       // 0.0 = no drag, 1.0 = instant stop
    .stationary()             // won't be moved by physics

rigidbody.apply_impulse(Vec2::new(0.0, -500.0))  // one-frame push
rigidbody.apply_force(Vec2::new(0.0, 9.81))       // continuous (resets each frame)

rigidbody.velocity      // Vec2
rigidbody.acceleration  // Vec2
rigidbody.damping       // f32
rigidbody.dynamic       // bool
```

### SpriteComp
```rust
SpriteComp::new(width, height)
    .with_color(r, g, b, a)
    .with_texture(texture_id)
    .with_z(0.5)             // draw order 0.0–1.0

sprite.visible    // bool — hide without destroying
sprite.size       // Vec2
sprite.color      // [f32; 4]
sprite.texture_id // Option<u64>
```

### Tag
```rust
world.add_tag(e, "enemy");
world.find_by_tag("enemy");
world.find_all_by_tag("enemy");
```

---

## Scripts

### Closure — simple, no state
```rust
world.add_script_fn(entity, |entity, world, dt| {
    if let Some(rb) = world.get_rigidbody_mut(entity) {
        rb.apply_impulse(Vec2::new(100.0 * dt, 0.0));
    }
});
```

### Trait — complex, with own state
```rust
use brass_engine::{Script, Entity, World};

struct Enemy { hp: i32, speed: f32 }

impl Script for Enemy {
    fn on_start(&mut self, entity: Entity, world: &mut World) {
        // runs once on first frame
    }
    fn on_update(&mut self, entity: Entity, world: &mut World, dt: f32) {
        // runs every frame
    }
    fn on_destroy(&mut self, entity: Entity, world: &mut World) {
        // runs when entity is destroyed
    }
}

let mut sc = ScriptComponent::new();
sc.add(Enemy { hp: 100, speed: 150.0 });
sc.add_fn(|e, world, dt| { /* mix trait + closure */ });
world.add_script_component(entity, sc);
```

---

## Physics

Runs automatically each frame on entities with both `Transform` and `RigidBody`:

```
velocity     += acceleration * dt
position     += velocity * dt
velocity     *= (1.0 - damping)
acceleration  = Vec2::ZERO    ← reset each frame
```

Set `rigidbody.dynamic = false` for static entities.

---

## Systems — Execution Order

Each frame runs automatically in this order:

```
1. script_system      — on_start (once) + on_update for all ScriptComponents
2. physics_system     — integrate velocity → position
3. on_update callback — your game logic + extra draw calls
4. render_sync_system — ECS Transform + SpriteComp → Renderer2D
5. Renderer3D.render  — 3D scene (clears screen)
6. Renderer2D.render  — 2D overlay on top of 3D
7. cleanup_system     — remove entities from world.destroy()
8. input.flush()      — clear single-frame input states
```

You can also call systems manually:

```rust
use brass_engine::{script_system, physics_system, render_sync_system, cleanup_system};
```

---

## License

MIT — see [LICENSE](LICENSE)