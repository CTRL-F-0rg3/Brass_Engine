use std::collections::HashMap;
use glam::Vec2;

use super::components::{Transform, RigidBody, SpriteComp, Tag};
use super::script::ScriptComponent;

// ─── Entity ───────────────────────────────────────────────────────────────────

/// Handle encji — prosty u64 ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity(pub u64);

// ─── World ────────────────────────────────────────────────────────────────────

/// Centralny rejestr wszystkich encji i ich komponentów.
pub struct World {
    next_id: u64,

    // Żywe encje
    entities: Vec<Entity>,

    // Storage per-typ komponentu (HashMap<Entity, Komponent>)
    pub transforms:  HashMap<Entity, Transform>,
    pub rigidbodies: HashMap<Entity, RigidBody>,
    pub sprites:     HashMap<Entity, SpriteComp>,
    pub tags:        HashMap<Entity, Tag>,
    pub scripts:     HashMap<Entity, ScriptComponent>,

    // Kolejka do usunięcia (bezpieczne usuwanie podczas iteracji)
    to_destroy: Vec<Entity>,
}

impl World {
    pub fn new() -> Self {
        Self {
            next_id:     1,
            entities:    Vec::new(),
            transforms:  HashMap::new(),
            rigidbodies: HashMap::new(),
            sprites:     HashMap::new(),
            tags:        HashMap::new(),
            scripts:     HashMap::new(),
            to_destroy:  Vec::new(),
        }
    }

    // ── Tworzenie / usuwanie encji ────────────────────────────────────────────

    /// Stwórz nową encję i zwróć jej ID.
    pub fn spawn(&mut self) -> Entity {
        let e = Entity(self.next_id);
        self.next_id += 1;
        self.entities.push(e);
        e
    }

    /// Zakolejkuj encję do usunięcia (wykona się na końcu klatki).
    pub fn destroy(&mut self, entity: Entity) {
        self.to_destroy.push(entity);
    }

    /// Faktyczne usunięcie — wywoływane przez `systems::cleanup`.
    pub fn flush_destroyed(&mut self) {
        for e in self.to_destroy.drain(..) {
            self.entities.retain(|x| *x != e);
            self.transforms.remove(&e);
            self.rigidbodies.remove(&e);
            self.sprites.remove(&e);
            self.tags.remove(&e);
            self.scripts.remove(&e);
        }
    }

    /// Lista wszystkich żywych encji (kopia — bezpieczna do iteracji).
    pub fn entities(&self) -> Vec<Entity> {
        self.entities.clone()
    }

    // ── Dodawanie komponentów (builder-style) ─────────────────────────────────

    pub fn add_transform(&mut self, e: Entity, t: Transform) -> &mut Self {
        self.transforms.insert(e, t);
        self
    }

    pub fn add_rigidbody(&mut self, e: Entity, rb: RigidBody) -> &mut Self {
        self.rigidbodies.insert(e, rb);
        self
    }

    pub fn add_sprite(&mut self, e: Entity, s: SpriteComp) -> &mut Self {
        self.sprites.insert(e, s);
        self
    }

    pub fn add_tag(&mut self, e: Entity, tag: &str) -> &mut Self {
        self.tags.insert(e, Tag::new(tag));
        self
    }

    pub fn add_script_component(&mut self, e: Entity, sc: ScriptComponent) -> &mut Self {
        self.scripts.insert(e, sc);
        self
    }

    // ── Shortcut: dodaj closure script bez ręcznego budowania ScriptComponent ──

    pub fn add_script_fn<F>(&mut self, e: Entity, f: F)
    where
        F: FnMut(Entity, &mut World, f32) + Send + Sync + 'static,
    {
        let sc = self.scripts.entry(e).or_insert_with(ScriptComponent::new);
        sc.add_fn(f);
    }

    // ── Gettery mutowalne ─────────────────────────────────────────────────────

    pub fn get_transform(&self, e: Entity) -> Option<&Transform> {
        self.transforms.get(&e)
    }

    pub fn get_transform_mut(&mut self, e: Entity) -> Option<&mut Transform> {
        self.transforms.get_mut(&e)
    }

    pub fn get_rigidbody(&self, e: Entity) -> Option<&RigidBody> {
        self.rigidbodies.get(&e)
    }

    pub fn get_rigidbody_mut(&mut self, e: Entity) -> Option<&mut RigidBody> {
        self.rigidbodies.get_mut(&e)
    }

    pub fn get_sprite(&self, e: Entity) -> Option<&SpriteComp> {
        self.sprites.get(&e)
    }

    pub fn get_sprite_mut(&mut self, e: Entity) -> Option<&mut SpriteComp> {
        self.sprites.get_mut(&e)
    }

    // ── Wyszukiwanie po tagu ──────────────────────────────────────────────────

    pub fn find_by_tag(&self, tag: &str) -> Option<Entity> {
        self.tags.iter()
            .find(|(_, t)| t.0 == tag)
            .map(|(e, _)| *e)
    }

    pub fn find_all_by_tag(&self, tag: &str) -> Vec<Entity> {
        self.tags.iter()
            .filter(|(_, t)| t.0 == tag)
            .map(|(e, _)| *e)
            .collect()
    }

    // ── Query helpers ─────────────────────────────────────────────────────────

    /// Encje które mają Transform I RigidBody.
    pub fn query_physics(&self) -> Vec<Entity> {
        self.entities.iter()
            .filter(|e| self.transforms.contains_key(e) && self.rigidbodies.contains_key(e))
            .copied()
            .collect()
    }

    /// Encje które mają Transform I SpriteComp.
    pub fn query_renderable(&self) -> Vec<Entity> {
        self.entities.iter()
            .filter(|e| self.transforms.contains_key(e) && self.sprites.contains_key(e))
            .copied()
            .collect()
    }

    /// Encje które mają ScriptComponent.
    pub fn query_scripted(&self) -> Vec<Entity> {
        self.entities.iter()
            .filter(|e| self.scripts.contains_key(e))
            .copied()
            .collect()
    }

    // ── Utility ───────────────────────────────────────────────────────────────

    /// Odległość między dwoma encjami (wymaga Transform na obu).
    pub fn distance(&self, a: Entity, b: Entity) -> Option<f32> {
        let ta = self.transforms.get(&a)?;
        let tb = self.transforms.get(&b)?;
        Some(ta.position.distance(tb.position))
    }

    /// Przesuń encję bezpośrednio (bez fizyki).
    pub fn translate(&mut self, e: Entity, delta: Vec2) {
        if let Some(t) = self.transforms.get_mut(&e) {
            t.position += delta;
        }
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}
