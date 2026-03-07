use super::world::{World, Entity};

// ─── Trait Script ─────────────────────────────────────────────────────────────

/// Implementuj ten trait dla złożonych skryptów wymagających własnego stanu.
///
/// ```rust
/// struct PlayerController { speed: f32 }
///
/// impl Script for PlayerController {
///     fn on_start(&mut self, entity: Entity, world: &mut World) { ... }
///     fn on_update(&mut self, entity: Entity, world: &mut World, dt: f32) { ... }
/// }
/// ```
pub trait Script: Send + Sync {
    /// Wywołane raz przy spawnie encji.
    fn on_start(&mut self, _entity: Entity, _world: &mut World) {}

    /// Wywołane co klatkę.
    fn on_update(&mut self, entity: Entity, world: &mut World, dt: f32);

    /// Wywołane przy usunięciu encji.
    fn on_destroy(&mut self, _entity: Entity, _world: &mut World) {}
}

// ─── Closure Script ───────────────────────────────────────────────────────────

/// Lekki skrypt jako closure — dla prostych zachowań bez własnego stanu.
///
/// ```rust
/// world.add_script_fn(entity, |entity, world, dt| {
///     if let Some(rb) = world.get_rigidbody_mut(entity) {
///         rb.apply_impulse(Vec2::new(100.0 * dt, 0.0));
///     }
/// });
/// ```
pub struct ClosureScript {
    pub func: Box<dyn FnMut(Entity, &mut World, f32) + Send + Sync>,
}

impl Script for ClosureScript {
    fn on_update(&mut self, entity: Entity, world: &mut World, dt: f32) {
        (self.func)(entity, world, dt);
    }
}

// ─── ScriptComponent ──────────────────────────────────────────────────────────

/// Kontener przechowujący listę skryptów przypisanych do encji.
/// Jedna encja może mieć wiele skryptów jednocześnie.
pub struct ScriptComponent {
    pub scripts: Vec<Box<dyn Script>>,
    started:     bool,
}

impl ScriptComponent {
    pub fn new() -> Self {
        Self {
            scripts: Vec::new(),
            started: false,
        }
    }

    /// Dodaj skrypt implementujący trait Script.
    pub fn add<S: Script + 'static>(&mut self, script: S) {
        self.scripts.push(Box::new(script));
    }

    /// Dodaj skrypt jako closure.
    pub fn add_fn<F>(&mut self, f: F)
    where
        F: FnMut(Entity, &mut World, f32) + Send + Sync + 'static,
    {
        self.scripts.push(Box::new(ClosureScript { func: Box::new(f) }));
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn mark_started(&mut self) {
        self.started = true;
    }
}

impl Default for ScriptComponent {
    fn default() -> Self {
        Self::new()
    }
}
