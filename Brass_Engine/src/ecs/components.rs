use glam::Vec2;

// ─── Transform ────────────────────────────────────────────────────────────────

/// Pozycja, skala i obrót encji w przestrzeni 2D.
#[derive(Clone, Debug)]
pub struct Transform {
    pub position: Vec2,
    pub scale:    Vec2,
    pub rotation: f32,  // radiany
}

impl Transform {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            position: Vec2::new(x, y),
            scale:    Vec2::ONE,
            rotation: 0.0,
        }
    }

    pub fn with_scale(mut self, sx: f32, sy: f32) -> Self {
        self.scale = Vec2::new(sx, sy);
        self
    }

    pub fn with_rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}

// ─── RigidBody ────────────────────────────────────────────────────────────────

/// Dane fizyczne encji — velocity, acceleration, masa, flagi.
#[derive(Clone, Debug)]
pub struct RigidBody {
    pub velocity:     Vec2,
    pub acceleration: Vec2,
    /// Tłumienie prędkości per-frame (0.0 = brak, 1.0 = natychmiastowe zatrzymanie)
    pub damping:      f32,
    /// false = statyczny (nie porusza się pod wpływem fizyki)
    pub dynamic:      bool,
}

impl RigidBody {
    pub fn new() -> Self {
        Self {
            velocity:     Vec2::ZERO,
            acceleration: Vec2::ZERO,
            damping:      0.05,
            dynamic:      true,
        }
    }

    pub fn with_velocity(mut self, vx: f32, vy: f32) -> Self {
        self.velocity = Vec2::new(vx, vy);
        self
    }

    pub fn with_damping(mut self, d: f32) -> Self {
        self.damping = d.clamp(0.0, 1.0);
        self
    }

    pub fn stationary(mut self) -> Self {
        self.dynamic = false;
        self
    }

    /// Dodaj siłę jednorazowo (impulse).
    pub fn apply_impulse(&mut self, force: Vec2) {
        self.velocity += force;
    }

    /// Dodaj przyspieszenie ciągłe (np. grawitacja).
    pub fn apply_force(&mut self, force: Vec2) {
        self.acceleration += force;
    }
}

impl Default for RigidBody {
    fn default() -> Self {
        Self::new()
    }
}

// ─── SpriteComp ───────────────────────────────────────────────────────────────

/// Komponent wizualny — łączy encję z danymi sprite'a dla Renderer2D.
#[derive(Clone, Debug)]
pub struct SpriteComp {
    pub size:       Vec2,
    pub color:      [f32; 4],    // RGBA
    pub texture_id: Option<u64>,
    pub uv_rect:    [f32; 4],
    pub z_order:    f32,
    pub visible:    bool,
}

impl SpriteComp {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size:       Vec2::new(width, height),
            color:      [1.0, 1.0, 1.0, 1.0],
            texture_id: None,
            uv_rect:    [0.0, 0.0, 1.0, 1.0],
            z_order:    0.5,
            visible:    true,
        }
    }

    pub fn with_color(mut self, r: f32, g: f32, b: f32, a: f32) -> Self {
        self.color = [r, g, b, a];
        self
    }

    pub fn with_texture(mut self, id: u64) -> Self {
        self.texture_id = Some(id);
        self
    }

    pub fn with_z(mut self, z: f32) -> Self {
        self.z_order = z;
        self
    }
}

// ─── Tag ──────────────────────────────────────────────────────────────────────

/// Prosty string tag — do wyszukiwania encji ("player", "enemy", "wall" itp.)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tag(pub String);

impl Tag {
    pub fn new(s: &str) -> Self {
        Self(s.to_string())
    }
}
