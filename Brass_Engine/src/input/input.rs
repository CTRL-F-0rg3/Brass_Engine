use std::collections::HashSet;
use glam::Vec2;
use winit::keyboard::KeyCode;

// ─── Key re-export ─────────────────────────────────────────────────────────────

/// Klawisze — re-export winit::KeyCode z wygodnym aliasem.
pub use winit::keyboard::KeyCode as Key;

/// Przyciski myszy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl From<winit::event::MouseButton> for MouseButton {
    fn from(b: winit::event::MouseButton) -> Self {
        match b {
            winit::event::MouseButton::Left   => MouseButton::Left,
            winit::event::MouseButton::Right  => MouseButton::Right,
            winit::event::MouseButton::Middle => MouseButton::Middle,
            _                                 => MouseButton::Left,
        }
    }
}

// ─── Input ────────────────────────────────────────────────────────────────────

/// Centralny stan wejścia — klawiatura i mysz.
/// Przekazywany do `on_update` i skryptów.
pub struct Input {
    // Klawiatura
    keys_held:     HashSet<KeyCode>,
    keys_pressed:  HashSet<KeyCode>,   // wciśnięte w tej klatce
    keys_released: HashSet<KeyCode>,   // puszczone w tej klatce

    // Mysz
    mouse_pos:          Vec2,
    mouse_delta:        Vec2,
    mouse_scroll:       f32,
    mouse_held:         HashSet<MouseButton>,
    mouse_pressed:      HashSet<MouseButton>,
    mouse_released:     HashSet<MouseButton>,
}

impl Input {
    pub fn new() -> Self {
        Self {
            keys_held:      HashSet::new(),
            keys_pressed:   HashSet::new(),
            keys_released:  HashSet::new(),
            mouse_pos:      Vec2::ZERO,
            mouse_delta:    Vec2::ZERO,
            mouse_scroll:   0.0,
            mouse_held:     HashSet::new(),
            mouse_pressed:  HashSet::new(),
            mouse_released: HashSet::new(),
        }
    }

    // ── Publiczne API ─────────────────────────────────────────────────────────

    /// Klawisz aktualnie wciśnięty (ciągłe sprawdzanie).
    pub fn is_key_down(&self, key: Key) -> bool {
        self.keys_held.contains(&key)
    }

    /// Klawisz wciśnięty dokładnie w tej klatce (jednorazowe).
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.keys_pressed.contains(&key)
    }

    /// Klawisz puszczony w tej klatce.
    pub fn is_key_released(&self, key: Key) -> bool {
        self.keys_released.contains(&key)
    }

    /// Przycisk myszy aktualnie wciśnięty.
    pub fn is_mouse_down(&self, btn: MouseButton) -> bool {
        self.mouse_held.contains(&btn)
    }

    /// Przycisk myszy kliknięty w tej klatce.
    pub fn is_mouse_pressed(&self, btn: MouseButton) -> bool {
        self.mouse_pressed.contains(&btn)
    }

    /// Przycisk myszy puszczony w tej klatce.
    pub fn is_mouse_released(&self, btn: MouseButton) -> bool {
        self.mouse_released.contains(&btn)
    }

    /// Pozycja kursora w pikselach ekranu.
    pub fn mouse_position(&self) -> Vec2 {
        self.mouse_pos
    }

    /// Ruch myszy od ostatniej klatki.
    pub fn mouse_delta(&self) -> Vec2 {
        self.mouse_delta
    }

    /// Scroll kółkiem (dodatni = w górę).
    pub fn scroll(&self) -> f32 {
        self.mouse_scroll
    }

    /// Axis helper — zwraca -1.0 / 0.0 / 1.0.
    /// Przykład: `input.axis(Key::ArrowLeft, Key::ArrowRight)`
    pub fn axis(&self, negative: Key, positive: Key) -> f32 {
        let n = if self.is_key_down(negative) { -1.0 } else { 0.0 };
        let p = if self.is_key_down(positive) {  1.0 } else { 0.0 };
        n + p
    }

    /// Vec2 axis — np. WASD → Vec2
    pub fn axis2d(&self, left: Key, right: Key, up: Key, down: Key) -> Vec2 {
        Vec2::new(self.axis(left, right), self.axis(up, down))
    }

    // ── Wewnętrzne — wywoływane przez app.rs ──────────────────────────────────

    /// Wyczyść stany jednoklatkowe — wywoływane na początku każdej klatki.
    pub fn flush(&mut self) {
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.mouse_pressed.clear();
        self.mouse_released.clear();
        self.mouse_delta  = Vec2::ZERO;
        self.mouse_scroll = 0.0;
    }

    pub fn on_key_down(&mut self, key: KeyCode) {
        if !self.keys_held.contains(&key) {
            self.keys_pressed.insert(key);
        }
        self.keys_held.insert(key);
    }

    pub fn on_key_up(&mut self, key: KeyCode) {
        self.keys_held.remove(&key);
        self.keys_released.insert(key);
    }

    pub fn on_mouse_move(&mut self, x: f32, y: f32) {
        let new_pos = Vec2::new(x, y);
        self.mouse_delta = new_pos - self.mouse_pos;
        self.mouse_pos   = new_pos;
    }

    pub fn on_mouse_down(&mut self, btn: MouseButton) {
        if !self.mouse_held.contains(&btn) {
            self.mouse_pressed.insert(btn);
        }
        self.mouse_held.insert(btn);
    }

    pub fn on_mouse_up(&mut self, btn: MouseButton) {
        self.mouse_held.remove(&btn);
        self.mouse_released.insert(btn);
    }

    pub fn on_scroll(&mut self, delta: f32) {
        self.mouse_scroll += delta;
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}
