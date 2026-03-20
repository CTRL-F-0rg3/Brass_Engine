// =============================================================================
//  Brass Engine — Physics2D
//
//  • AABB collision detection + swept resolution
//  • Rigidbody z grawitacją, tarciem, bounce
//  • Tilemap collision (dense bool grid)
//  • Queries: overlap check, AABB raycast
// =============================================================================

use glam::Vec2;
use std::collections::HashMap;

// ─── Stałe ────────────────────────────────────────────────────────────────────

pub const GRAVITY: f32 = 980.0; // px/s² (jak pikselowa gra platformowa)

// ─── AABB ─────────────────────────────────────────────────────────────────────

/// Axis-Aligned Bounding Box.
/// `position` = lewy górny róg, `size` = (szerokość, wysokość).
#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub position: Vec2,
    pub size:     Vec2,
}

impl Aabb {
    pub fn new(position: Vec2, size: Vec2) -> Self {
        Self { position, size }
    }

    /// Stwórz AABB ze środka (wygodne przy rysowaniu sprite'ów).
    pub fn from_center(center: Vec2, size: Vec2) -> Self {
        Self { position: center - size * 0.5, size }
    }

    pub fn center(&self) -> Vec2 {
        self.position + self.size * 0.5
    }

    pub fn min(&self) -> Vec2 { self.position }
    pub fn max(&self) -> Vec2 { self.position + self.size }

    pub fn left(&self)   -> f32 { self.position.x }
    pub fn right(&self)  -> f32 { self.position.x + self.size.x }
    pub fn top(&self)    -> f32 { self.position.y }
    pub fn bottom(&self) -> f32 { self.position.y + self.size.y }

    /// Czy dwa AABB się przecinają.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.left()   < other.right()  &&
        self.right()  > other.left()   &&
        self.top()    < other.bottom() &&
        self.bottom() > other.top()
    }

    /// Głębokość penetracji i normalna kolizji (MTV — minimum translation vector).
    /// Zwraca None jeśli nie ma kolizji.
    pub fn penetration(&self, other: &Aabb) -> Option<(Vec2, f32)> {
        if !self.overlaps(other) { return None; }

        let dx_left  = other.right()  - self.left();
        let dx_right = self.right()   - other.left();
        let dy_up    = other.bottom() - self.top();
        let dy_down  = self.bottom()  - other.top();

        // Najmniejsze przesunięcie żeby rozdzielić
        let (nx, px) = if dx_left < dx_right { (-1.0, dx_left)  } else { (1.0, dx_right) };
        let (ny, py) = if dy_up   < dy_down  { (-1.0, dy_up)    } else { (1.0, dy_down)  };

        if px < py {
            Some((Vec2::new(nx, 0.0), px))
        } else {
            Some((Vec2::new(0.0, ny), py))
        }
    }

    /// Swept AABB — kolizja podczas ruchu (zwraca czas wejścia 0..1 i normalną).
    /// `velocity` = ruch w tej klatce (przed aplikacją).
    /// Zwraca None jeśli nie trafi.
    pub fn swept(&self, other: &Aabb, velocity: Vec2) -> Option<SweptResult> {
        if velocity == Vec2::ZERO { return None; }

        let inv_entry_x;
        let inv_exit_x;
        let inv_entry_y;
        let inv_exit_y;

        if velocity.x > 0.0 {
            inv_entry_x = other.left()   - self.right();
            inv_exit_x  = other.right()  - self.left();
        } else {
            inv_entry_x = other.right()  - self.left();
            inv_exit_x  = other.left()   - self.right();
        }

        if velocity.y > 0.0 {
            inv_entry_y = other.top()    - self.bottom();
            inv_exit_y  = other.bottom() - self.top();
        } else {
            inv_entry_y = other.bottom() - self.top();
            inv_exit_y  = other.top()    - self.bottom();
        }

        let entry_x = if velocity.x == 0.0 { f32::NEG_INFINITY } else { inv_entry_x / velocity.x };
        let exit_x  = if velocity.x == 0.0 { f32::INFINITY     } else { inv_exit_x  / velocity.x };
        let entry_y = if velocity.y == 0.0 { f32::NEG_INFINITY } else { inv_entry_y / velocity.y };
        let exit_y  = if velocity.y == 0.0 { f32::INFINITY     } else { inv_exit_y  / velocity.y };

        let entry_time = entry_x.max(entry_y);
        let exit_time  = exit_x.min(exit_y);

        if entry_time > exit_time || entry_time >= 1.0 || exit_time <= 0.0 {
            return None;
        }

        let normal = if entry_x > entry_y {
            Vec2::new(if velocity.x < 0.0 { 1.0 } else { -1.0 }, 0.0)
        } else {
            Vec2::new(0.0, if velocity.y < 0.0 { 1.0 } else { -1.0 })
        };

        Some(SweptResult { time: entry_time.max(0.0), normal })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SweptResult {
    /// Czas wejścia w kolizję: 0.0 = od razu, 1.0 = na końcu ruchu.
    pub time:   f32,
    /// Normalna powierzchni kolizji.
    pub normal: Vec2,
}

// ─── Rigidbody ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Rigidbody {
    pub velocity:       Vec2,
    pub acceleration:   Vec2,   // dodatkowe siły (nie grawitacja)
    pub mass:           f32,
    pub gravity_scale:  f32,    // 0 = brak grawitacji (np. top-down)
    pub friction:       f32,    // 0..1, tłumienie poziome gdy na ziemi
    pub bounce:         f32,    // 0..1, współczynnik odbicia
    pub max_speed:      Vec2,   // limit prędkości (0 = brak limitu)
    pub on_ground:      bool,
    pub is_static:      bool,   // true = koliduje ale nie rusza się
}

impl Rigidbody {
    pub fn new() -> Self {
        Self {
            velocity:      Vec2::ZERO,
            acceleration:  Vec2::ZERO,
            mass:          1.0,
            gravity_scale: 1.0,
            friction:      0.85,
            bounce:        0.0,
            max_speed:     Vec2::ZERO,
            on_ground:     false,
            is_static:     false,
        }
    }

    pub fn with_gravity_scale(mut self, s: f32) -> Self { self.gravity_scale = s; self }
    pub fn with_friction(mut self, f: f32)      -> Self { self.friction = f;       self }
    pub fn with_bounce(mut self, b: f32)        -> Self { self.bounce = b;         self }
    pub fn with_mass(mut self, m: f32)          -> Self { self.mass = m;           self }
    pub fn static_body(mut self)                -> Self { self.is_static = true;   self }

    /// Dodaj impuls (natychmiastowa zmiana prędkości).
    pub fn impulse(&mut self, imp: Vec2) {
        if !self.is_static {
            self.velocity += imp / self.mass;
        }
    }

    /// Dodaj siłę (akumuluje do następnej klatki).
    pub fn add_force(&mut self, force: Vec2) {
        if !self.is_static {
            self.acceleration += force / self.mass;
        }
    }
}

impl Default for Rigidbody {
    fn default() -> Self { Self::new() }
}

// ─── Tilemap Collider ─────────────────────────────────────────────────────────

/// Mapa tilemap — gęsty grid bool (true = solid tile).
pub struct TilemapCollider {
    pub tile_size: f32,
    pub width:     u32,   // liczba kafelków w poziomie
    pub height:    u32,
    pub offset:    Vec2,  // pozycja lewego górnego rogu mapy w world space
    tiles:         Vec<bool>,
}

impl TilemapCollider {
    pub fn new(tile_size: f32, width: u32, height: u32, offset: Vec2) -> Self {
        Self {
            tile_size,
            width,
            height,
            offset,
            tiles: vec![false; (width * height) as usize],
        }
    }

    pub fn set_tile(&mut self, x: u32, y: u32, solid: bool) {
        if x < self.width && y < self.height {
            self.tiles[(y * self.width + x) as usize] = solid;
        }
    }

    pub fn get_tile(&self, x: u32, y: u32) -> bool {
        if x < self.width && y < self.height {
            self.tiles[(y * self.width + x) as usize]
        } else {
            false // poza mapą = nie solid (zmień na true jeśli chcesz ściany)
        }
    }

    /// Tile AABB w world space.
    pub fn tile_aabb(&self, x: u32, y: u32) -> Aabb {
        Aabb::new(
            self.offset + Vec2::new(x as f32 * self.tile_size, y as f32 * self.tile_size),
            Vec2::splat(self.tile_size),
        )
    }

    /// Wszystkie solid tile które AABB przecina (z marginesem na ruch).
    pub fn tiles_in_region(&self, aabb: &Aabb) -> Vec<(u32, u32)> {
        let margin = 1.0;
        let min = (aabb.min() - self.offset - Vec2::splat(margin)) / self.tile_size;
        let max = (aabb.max() - self.offset + Vec2::splat(margin)) / self.tile_size;

        let x0 = (min.x.floor() as i32).max(0) as u32;
        let y0 = (min.y.floor() as i32).max(0) as u32;
        let x1 = (max.x.ceil()  as i32).min(self.width  as i32 - 1).max(0) as u32;
        let y1 = (max.y.ceil()  as i32).min(self.height as i32 - 1).max(0) as u32;

        let mut result = Vec::new();
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                if self.get_tile(tx, ty) {
                    result.push((tx, ty));
                }
            }
        }
        result
    }

    /// Wczytaj mapę z Vec<Vec<bool>> (wiersze od góry).
    pub fn from_grid(tile_size: f32, grid: &[&[bool]], offset: Vec2) -> Self {
        let height = grid.len() as u32;
        let width  = grid.first().map(|r| r.len()).unwrap_or(0) as u32;
        let mut map = Self::new(tile_size, width, height, offset);
        for (y, row) in grid.iter().enumerate() {
            for (x, &solid) in row.iter().enumerate() {
                map.set_tile(x as u32, y as u32, solid);
            }
        }
        map
    }

    /// Wczytaj mapę z &str gdzie '#' = solid, ' ' = pusty.
    pub fn from_str(tile_size: f32, map_str: &str, offset: Vec2) -> Self {
        let rows: Vec<&str> = map_str.lines().collect();
        let height = rows.len() as u32;
        let width  = rows.iter().map(|r| r.len()).max().unwrap_or(0) as u32;
        let mut map = Self::new(tile_size, width, height, offset);
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                map.set_tile(x as u32, y as u32, ch == '#');
            }
        }
        map
    }
}

// ─── PhysicsWorld ─────────────────────────────────────────────────────────────

/// ID fizycznego obiektu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicsId(pub u64);

struct PhysicsBody {
    pub aabb:      Aabb,
    pub rigidbody: Rigidbody,
}

/// Centralny rejestr fizyki — update wszystkich ciał + kolizje.
pub struct PhysicsWorld {
    bodies:      HashMap<PhysicsId, PhysicsBody>,
    next_id:     u64,
    pub gravity: Vec2,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            bodies:  HashMap::new(),
            next_id: 1,
            gravity: Vec2::new(0.0, GRAVITY),
        }
    }

    /// Zarejestruj nowe ciało, zwróć ID.
    pub fn add(&mut self, aabb: Aabb, rb: Rigidbody) -> PhysicsId {
        let id = PhysicsId(self.next_id);
        self.next_id += 1;
        self.bodies.insert(id, PhysicsBody { aabb, rigidbody: rb });
        id
    }

    pub fn remove(&mut self, id: PhysicsId) {
        self.bodies.remove(&id);
    }

    pub fn get_aabb(&self, id: PhysicsId) -> Option<Aabb> {
        self.bodies.get(&id).map(|b| b.aabb)
    }

    pub fn get_rb(&self, id: PhysicsId) -> Option<&Rigidbody> {
        self.bodies.get(&id).map(|b| &b.rigidbody)
    }

    pub fn get_rb_mut(&mut self, id: PhysicsId) -> Option<&mut Rigidbody> {
        self.bodies.get_mut(&id).map(|b| &mut b.rigidbody)
    }

    /// Teleportuj ciało (bez fizyki).
    pub fn set_position(&mut self, id: PhysicsId, pos: Vec2) {
        if let Some(b) = self.bodies.get_mut(&id) {
            b.aabb.position = pos;
        }
    }

    /// Dodaj impuls do ciała.
    pub fn impulse(&mut self, id: PhysicsId, imp: Vec2) {
        if let Some(b) = self.bodies.get_mut(&id) {
            b.rigidbody.impulse(imp);
        }
    }

    // ── Główny update ─────────────────────────────────────────────────────────

    pub fn update(&mut self, dt: f32, tilemap: Option<&TilemapCollider>) {
        let ids: Vec<PhysicsId> = self.bodies.keys().copied().collect();

        for id in &ids {
            let body = match self.bodies.get_mut(id) {
                Some(b) => b,
                None    => continue,
            };
            if body.rigidbody.is_static { continue; }

            let rb = &mut body.rigidbody;

            // ── Grawitacja ────────────────────────────────────────────────────
            rb.velocity.y += self.gravity.y * rb.gravity_scale * dt;

            // ── Dodatkowe przyspieszenie ───────────────────────────────────────
            rb.velocity += rb.acceleration * dt;
            rb.acceleration = Vec2::ZERO;

            // ── Limit prędkości ───────────────────────────────────────────────
            if rb.max_speed.x > 0.0 {
                rb.velocity.x = rb.velocity.x.clamp(-rb.max_speed.x, rb.max_speed.x);
            }
            if rb.max_speed.y > 0.0 {
                rb.velocity.y = rb.velocity.y.clamp(-rb.max_speed.y, rb.max_speed.y);
            }

            let delta = rb.velocity * dt;

            // ── Kolizje z tilemap ─────────────────────────────────────────────
            let mut remaining = delta;
            rb.on_ground = false;

            if let Some(tm) = tilemap {
                // Rozdziel ruch na X i Y — każde osobno żeby uniknąć corner bugs
                // Najpierw X
                let moved_x = Aabb::new(body.aabb.position + Vec2::new(remaining.x, 0.0), body.aabb.size);
                let x_tiles = tm.tiles_in_region(&moved_x);
                for (tx, ty) in &x_tiles {
                    let tile = tm.tile_aabb(*tx, *ty);
                    if moved_x.overlaps(&tile) {
                        if remaining.x > 0.0 {
                            remaining.x = tile.left() - body.aabb.right();
                        } else {
                            remaining.x = tile.right() - body.aabb.left();
                        }
                        rb.velocity.x = if rb.bounce > 0.0 { -rb.velocity.x * rb.bounce } else { 0.0 };
                    }
                }

                // Potem Y
                let moved_y = Aabb::new(body.aabb.position + Vec2::new(remaining.x, remaining.y), body.aabb.size);
                let y_tiles = tm.tiles_in_region(&moved_y);
                for (tx, ty) in &y_tiles {
                    let tile = tm.tile_aabb(*tx, *ty);
                    let test = Aabb::new(body.aabb.position + Vec2::new(remaining.x, remaining.y), body.aabb.size);
                    if test.overlaps(&tile) {
                        if remaining.y > 0.0 {
                            // Leci w dół — lądowanie
                            remaining.y = tile.top() - body.aabb.bottom();
                            rb.on_ground = true;
                        } else {
                            // Leci w górę — uderza sufit
                            remaining.y = tile.bottom() - body.aabb.top();
                        }
                        rb.velocity.y = if rb.bounce > 0.0 { -rb.velocity.y * rb.bounce } else { 0.0 };
                    }
                }
            }

            // ── Tarcie poziome gdy na ziemi ───────────────────────────────────
            if body.rigidbody.on_ground {
                body.rigidbody.velocity.x *= body.rigidbody.friction;
                if body.rigidbody.velocity.x.abs() < 0.5 {
                    body.rigidbody.velocity.x = 0.0;
                }
            }

            body.aabb.position += remaining;
        }

        // ── AABB vs AABB (dynamiczne ciała między sobą) ───────────────────────
        let ids2: Vec<PhysicsId> = self.bodies.keys().copied().collect();
        for i in 0..ids2.len() {
            for j in (i+1)..ids2.len() {
                let ia = ids2[i];
                let ib = ids2[j];

                let (a_static, b_static, overlap) = {
                    let a = &self.bodies[&ia];
                    let b = &self.bodies[&ib];
                    if let Some(pen) = a.aabb.penetration(&b.aabb) {
                        (a.rigidbody.is_static, b.rigidbody.is_static, Some(pen))
                    } else {
                        (false, false, None)
                    }
                };

                if let Some((normal, depth)) = overlap {
                    let push = normal * depth;
                    match (a_static, b_static) {
                        (false, false) => {
                            // Oba dynamiczne — równy podział
                            self.bodies.get_mut(&ia).unwrap().aabb.position -= push * 0.5;
                            self.bodies.get_mut(&ib).unwrap().aabb.position += push * 0.5;
                            // Odbicie prędkości
                            let va = self.bodies[&ia].rigidbody.velocity;
                            let vb = self.bodies[&ib].rigidbody.velocity;
                            let ma = self.bodies[&ia].rigidbody.mass;
                            let mb = self.bodies[&ib].rigidbody.mass;
                            let e  = (self.bodies[&ia].rigidbody.bounce + self.bodies[&ib].rigidbody.bounce) * 0.5;
                            let rel = va - vb;
                            let imp = -(1.0 + e) * rel.dot(normal) / (1.0/ma + 1.0/mb);
                            let impulse = normal * imp;
                            self.bodies.get_mut(&ia).unwrap().rigidbody.velocity += impulse / ma;
                            self.bodies.get_mut(&ib).unwrap().rigidbody.velocity -= impulse / mb;
                        }
                        (true, false) => {
                            self.bodies.get_mut(&ib).unwrap().aabb.position += push;
                            let vb = self.bodies[&ib].rigidbody.velocity;
                            let e  = self.bodies[&ib].rigidbody.bounce;
                            if vb.dot(normal) < 0.0 {
                                let reflected = vb - normal * vb.dot(normal) * (1.0 + e);
                                self.bodies.get_mut(&ib).unwrap().rigidbody.velocity = reflected;
                            }
                            if normal.y < 0.0 {
                                self.bodies.get_mut(&ib).unwrap().rigidbody.on_ground = true;
                            }
                        }
                        (false, true) => {
                            self.bodies.get_mut(&ia).unwrap().aabb.position -= push;
                            let va = self.bodies[&ia].rigidbody.velocity;
                            let e  = self.bodies[&ia].rigidbody.bounce;
                            if va.dot(-normal) < 0.0 {
                                let reflected = va - (-normal) * va.dot(-normal) * (1.0 + e);
                                self.bodies.get_mut(&ia).unwrap().rigidbody.velocity = reflected;
                            }
                            if (-normal).y < 0.0 {
                                self.bodies.get_mut(&ia).unwrap().rigidbody.on_ground = true;
                            }
                        }
                        (true, true) => {} // oba statyczne — ignoruj
                    }
                }
            }
        }
    }
}

// ─── Raycast ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub point:    Vec2,
    pub normal:   Vec2,
    pub distance: f32,
}

/// Rzuć promień w `PhysicsWorld` — zwraca najbliższe trafienie.
pub fn raycast(
    world: &PhysicsWorld,
    origin: Vec2,
    direction: Vec2,
    max_dist: f32,
    ignore: Option<PhysicsId>,
) -> Option<(PhysicsId, RayHit)> {
    let dir = direction.normalize_or_zero();
    let mut closest: Option<(PhysicsId, RayHit)> = None;

    for (id, body) in &world.bodies {
        if Some(*id) == ignore { continue; }
        if let Some(hit) = ray_vs_aabb(origin, dir, max_dist, &body.aabb) {
            let better = closest.as_ref().map(|(_, h)| hit.distance < h.distance).unwrap_or(true);
            if better { closest = Some((*id, hit)); }
        }
    }
    closest
}

/// Rzuć promień przeciwko tilemap.
pub fn raycast_tilemap(
    tilemap: &TilemapCollider,
    origin: Vec2,
    direction: Vec2,
    max_dist: f32,
) -> Option<RayHit> {
    let dir = direction.normalize_or_zero();
    // DDA (Digital Differential Analyzer) — fast grid traversal
    let tile_size = tilemap.tile_size;
    let mut pos = (origin - tilemap.offset) / tile_size;
    let step_x: i32 = if dir.x >= 0.0 { 1 } else { -1 };
    let step_y: i32 = if dir.y >= 0.0 { 1 } else { -1 };

    let mut tile_x = pos.x.floor() as i32;
    let mut tile_y = pos.y.floor() as i32;

    let delta_dist_x = if dir.x == 0.0 { f32::INFINITY } else { (1.0 / dir.x).abs() };
    let delta_dist_y = if dir.y == 0.0 { f32::INFINITY } else { (1.0 / dir.y).abs() };

    let mut side_dist_x = if dir.x < 0.0 {
        (pos.x - tile_x as f32) * delta_dist_x
    } else {
        (tile_x as f32 + 1.0 - pos.x) * delta_dist_x
    };
    let mut side_dist_y = if dir.y < 0.0 {
        (pos.y - tile_y as f32) * delta_dist_y
    } else {
        (tile_y as f32 + 1.0 - pos.y) * delta_dist_y
    };

    let max_steps = (max_dist / tile_size) as u32 + 2;
    let mut side = 0; // 0=X, 1=Y

    for _ in 0..max_steps {
        if side_dist_x < side_dist_y {
            side_dist_x += delta_dist_x;
            tile_x += step_x;
            side = 0;
        } else {
            side_dist_y += delta_dist_y;
            tile_y += step_y;
            side = 1;
        }

        if tile_x < 0 || tile_y < 0 { break; }
        let (tx, ty) = (tile_x as u32, tile_y as u32);

        if tilemap.get_tile(tx, ty) {
            let dist = if side == 0 { side_dist_x - delta_dist_x } else { side_dist_y - delta_dist_y };
            let dist_world = dist * tile_size;
            if dist_world > max_dist { break; }

            let normal = if side == 0 {
                Vec2::new(-step_x as f32, 0.0)
            } else {
                Vec2::new(0.0, -step_y as f32)
            };

            return Some(RayHit {
                point:    origin + dir * dist_world,
                normal,
                distance: dist_world,
            });
        }
    }
    None
}

fn ray_vs_aabb(origin: Vec2, dir: Vec2, max_dist: f32, aabb: &Aabb) -> Option<RayHit> {
    let inv = Vec2::new(
        if dir.x == 0.0 { f32::INFINITY } else { 1.0 / dir.x },
        if dir.y == 0.0 { f32::INFINITY } else { 1.0 / dir.y },
    );
    let t1 = (aabb.min() - origin) * inv;
    let t2 = (aabb.max() - origin) * inv;
    let tmin = t1.min(t2);
    let tmax = t1.max(t2);
    let tnear = tmin.x.max(tmin.y);
    let tfar  = tmax.x.min(tmax.y);

    if tnear > tfar || tfar < 0.0 || tnear > max_dist { return None; }
    let t = if tnear < 0.0 { tfar } else { tnear };
    if t > max_dist { return None; }

    let normal = if tmin.x > tmin.y {
        Vec2::new(-dir.x.signum(), 0.0)
    } else {
        Vec2::new(0.0, -dir.y.signum())
    };

    Some(RayHit { point: origin + dir * t, normal, distance: t })
}

// ─── Overlap query ────────────────────────────────────────────────────────────

/// Zwróć wszystkie ID ciał które przecinają dany AABB.
pub fn query_overlap(world: &PhysicsWorld, aabb: &Aabb, ignore: Option<PhysicsId>) -> Vec<PhysicsId> {
    world.bodies.iter()
        .filter(|(id, body)| Some(**id) != ignore && aabb.overlaps(&body.aabb))
        .map(|(id, _)| *id)
        .collect()
}
