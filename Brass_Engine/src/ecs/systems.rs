// =============================================================================
//  Brass Engine — Systems
//  Każdy system to zwykła funkcja: fn foo_system(world: &mut World, dt: f32)
//  Kolejność wywołania: script → physics → render_sync → cleanup
// =============================================================================

use crate::render::renderer2d::{Renderer2D, Sprite, Color};
use super::world::{World, Entity};

// ─── Script System ────────────────────────────────────────────────────────────

/// Wywołuje on_start() (raz) i on_update() na wszystkich ScriptComponent.
/// Skrypty są tymczasowo wyjmowane z World żeby uniknąć double borrow.
pub fn script_system(world: &mut World, dt: f32) {
    let scripted = world.query_scripted();

    for entity in scripted {
        // Wyjmij ScriptComponent żeby móc przekazać &mut World do skryptów
        let mut sc = match world.scripts.remove(&entity) {
            Some(s) => s,
            None    => continue,
        };

        // on_start — tylko raz przy pierwszej klatce
        if !sc.is_started() {
            for script in sc.scripts.iter_mut() {
                script.on_start(entity, world);
            }
            sc.mark_started();
        }

        // on_update — co klatkę
        for script in sc.scripts.iter_mut() {
            script.on_update(entity, world, dt);
        }

        // Wróć ScriptComponent do World
        world.scripts.insert(entity, sc);
    }
}

// ─── Physics System ───────────────────────────────────────────────────────────

/// velocity += acceleration * dt
/// position += velocity * dt
/// velocity *= (1 - damping)
/// acceleration resetowana po każdej klatce
pub fn physics_system(world: &mut World, dt: f32) {
    let entities = world.query_physics();

    for e in entities {
        // Borrow splits — najpierw wczytaj dane z RigidBody
        let (vel, damp, dynamic) = {
            let rb = match world.rigidbodies.get_mut(&e) {
                Some(r) => r,
                None    => continue,
            };
            if !rb.dynamic {
                continue;
            }
            rb.velocity += rb.acceleration * dt;
            rb.velocity *= 1.0 - rb.damping;
            rb.acceleration = glam::Vec2::ZERO;
            (rb.velocity, rb.damping, rb.dynamic)
        };

        // Teraz zaktualizuj Transform
        if let Some(t) = world.transforms.get_mut(&e) {
            t.position += vel * dt;
        }

        let _ = (damp, dynamic); // suppress warnings
    }
}

// ─── Render Sync System ───────────────────────────────────────────────────────

/// Synchronizuje Transform + SpriteComp z Renderer2D.
/// Wywoływane na końcu każdej klatki — buduje draw calls.
pub fn render_sync_system(world: &World, renderer: &mut Renderer2D) {
    let renderables = world.query_renderable();

    for e in renderables {
        let transform = match world.transforms.get(&e) {
            Some(t) => t,
            None    => continue,
        };
        let sprite_comp = match world.sprites.get(&e) {
            Some(s) => s,
            None    => continue,
        };

        if !sprite_comp.visible {
            continue;
        }

        let [r, g, b, a] = sprite_comp.color;

        let sprite = Sprite::new(transform.position, sprite_comp.size * transform.scale)
            .with_color(Color::rgba(r, g, b, a))
            .with_rotation(transform.rotation);

        let sprite = if let Some(id) = sprite_comp.texture_id {
            sprite.with_texture(id)
        } else {
            sprite
        };

        renderer.draw_sprite(sprite);
    }
}

// ─── Cleanup System ───────────────────────────────────────────────────────────

/// Usuwa encje zakolejkowane przez world.destroy(entity).
pub fn cleanup_system(world: &mut World) {
    world.flush_destroyed();
}
