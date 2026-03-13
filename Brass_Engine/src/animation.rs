// =============================================================================
//  Brass Engine — Animation System
//
//  AnimationClip    — sekwencja klatek na sprite sheecie
//  AnimationState   — węzeł w state machine (clip + warunki przejścia)
//  Animator         — state machine + tick + zapis do Sprite.uv_rect
// =============================================================================

use std::collections::HashMap;
use crate::render::renderer2d::Sprite;
use crate::tilemap::TileSet;

// ─── AnimationFrame ───────────────────────────────────────────────────────────

/// Pojedyncza klatka animacji — UV rect + opcjonalny override czasu trwania.
#[derive(Clone, Debug)]
pub struct AnimationFrame {
    /// UV rect [u0, v0, u1, v1] — bezpośrednio do Sprite.uv_rect
    pub uv:       [f32; 4],
    /// Czas trwania klatki w sekundach. None = użyj `AnimationClip::frame_duration`
    pub duration: Option<f32>,
}

impl AnimationFrame {
    pub fn new(uv: [f32; 4]) -> Self {
        Self { uv, duration: None }
    }

    pub fn with_duration(mut self, secs: f32) -> Self {
        self.duration = Some(secs);
        self
    }
}

// ─── AnimationClip ────────────────────────────────────────────────────────────

/// Sekwencja klatek odtwarzana jako animacja.
#[derive(Clone, Debug)]
pub struct AnimationClip {
    pub name:           String,
    pub frames:         Vec<AnimationFrame>,
    /// Domyślny czas trwania klatki (sekundy), nadpisywany przez `AnimationFrame::duration`
    pub frame_duration: f32,
    /// Czy animacja się pętli
    pub looping:        bool,
}

impl AnimationClip {
    pub fn new(name: &str, frame_duration: f32) -> Self {
        Self {
            name: name.to_string(),
            frames: Vec::new(),
            frame_duration,
            looping: true,
        }
    }

    /// Nie pętluj — zatrzymaj na ostatniej klatce.
    pub fn once(mut self) -> Self {
        self.looping = false;
        self
    }

    /// Dodaj klatkę przez UV rect.
    pub fn frame(mut self, uv: [f32; 4]) -> Self {
        self.frames.push(AnimationFrame::new(uv));
        self
    }

    /// Dodaj klatkę z niestandardowym czasem trwania.
    pub fn frame_timed(mut self, uv: [f32; 4], secs: f32) -> Self {
        self.frames.push(AnimationFrame::new(uv).with_duration(secs));
        self
    }

    /// Wygodny konstruktor: klatki z TileSet (id_start..id_end włącznie).
    pub fn from_tileset(name: &str, tileset: &TileSet, id_start: u32, id_end: u32, fps: f32) -> Self {
        let frame_duration = 1.0 / fps;
        let mut clip = AnimationClip::new(name, frame_duration);
        for id in id_start..=id_end {
            clip.frames.push(AnimationFrame::new(tileset.uv_for_tile(id)));
        }
        clip
    }

    /// Klatki z wiersza sprite sheetu (row, col_start..col_end, fps).
    pub fn from_row(
        name:      &str,
        tileset:   &TileSet,
        row:       u32,
        col_start: u32,
        col_end:   u32,
        fps:       f32,
    ) -> Self {
        let id_start = tileset.tile_id(col_start, row);
        let id_end   = tileset.tile_id(col_end, row);
        Self::from_tileset(name, tileset, id_start, id_end, fps)
    }

    /// Liczba klatek.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Czas trwania klatki o indeksie `i`.
    pub fn frame_dur(&self, i: usize) -> f32 {
        self.frames.get(i)
            .and_then(|f| f.duration)
            .unwrap_or(self.frame_duration)
    }

    /// Całkowity czas trwania klipu (jedna pętla).
    pub fn total_duration(&self) -> f32 {
        (0..self.frames.len()).map(|i| self.frame_dur(i)).sum()
    }
}

// ─── Transition ───────────────────────────────────────────────────────────────

/// Warunek przejścia między stanami.
#[derive(Clone, Debug)]
pub enum TransitionCondition {
    /// Przejdź gdy wartość f32 > threshold
    FloatGt  { param: String, threshold: f32 },
    /// Przejdź gdy wartość f32 < threshold
    FloatLt  { param: String, threshold: f32 },
    /// Przejdź gdy bool == expected
    BoolIs   { param: String, expected: bool },
    /// Przejdź gdy trigger (jednorazowy bool) jest aktywny
    Trigger  { param: String },
    /// Przejdź po zakończeniu bieżącej animacji (tylko dla looping=false)
    OnFinish,
    /// Zawsze przejdź po czasie
    After    { secs: f32 },
}

/// Reguła przejścia do innego stanu.
#[derive(Clone, Debug)]
pub struct Transition {
    pub target:    String,
    pub condition: TransitionCondition,
    /// Priorytet (wyższy = sprawdzany wcześniej)
    pub priority:  i32,
}

impl Transition {
    pub fn new(target: &str, condition: TransitionCondition) -> Self {
        Self { target: target.to_string(), condition, priority: 0 }
    }

    pub fn priority(mut self, p: i32) -> Self {
        self.priority = p;
        self
    }
}

// ─── AnimationState ───────────────────────────────────────────────────────────

/// Węzeł state machine — jeden clip + lista wyjść.
#[derive(Clone, Debug)]
pub struct AnimationState {
    pub name:        String,
    pub clip:        AnimationClip,
    pub transitions: Vec<Transition>,
}

impl AnimationState {
    pub fn new(name: &str, clip: AnimationClip) -> Self {
        Self {
            name: name.to_string(),
            clip,
            transitions: Vec::new(),
        }
    }

    pub fn with_transition(mut self, t: Transition) -> Self {
        self.transitions.push(t);
        self
    }
}

// ─── Animator ─────────────────────────────────────────────────────────────────

/// Aktywny playback state jednej animacji.
#[derive(Clone, Debug, Default)]
struct PlaybackState {
    frame:       usize,
    frame_timer: f32,
    elapsed:     f32,
    finished:    bool,
}

/// Zestaw parametrów (float, bool, trigger) używanych przez warunki.
#[derive(Clone, Debug, Default)]
pub struct AnimatorParams {
    floats:   HashMap<String, f32>,
    bools:    HashMap<String, bool>,
    triggers: HashMap<String, bool>,
}

impl AnimatorParams {
    pub fn set_float(&mut self, name: &str, value: f32) {
        self.floats.insert(name.to_string(), value);
    }

    pub fn set_bool(&mut self, name: &str, value: bool) {
        self.bools.insert(name.to_string(), value);
    }

    pub fn set_trigger(&mut self, name: &str) {
        self.triggers.insert(name.to_string(), true);
    }

    pub fn get_float(&self, name: &str) -> f32 {
        *self.floats.get(name).unwrap_or(&0.0)
    }

    pub fn get_bool(&self, name: &str) -> bool {
        *self.bools.get(name).unwrap_or(&false)
    }

    fn consume_trigger(&mut self, name: &str) -> bool {
        let active = *self.triggers.get(name).unwrap_or(&false);
        if active {
            self.triggers.insert(name.to_string(), false);
        }
        active
    }

    fn clear_triggers(&mut self) {
        for v in self.triggers.values_mut() {
            *v = false;
        }
    }
}

/// State machine animacji podpięta do jednego Sprite'a.
///
/// ```rust
/// let mut anim = Animator::new("idle");
/// anim.add_state(AnimationState::new("idle", idle_clip)
///     .with_transition(Transition::new("walk",
///         TransitionCondition::FloatGt { param: "speed".into(), threshold: 0.1 }
///     ))
/// );
/// anim.add_state(AnimationState::new("walk", walk_clip)
///     .with_transition(Transition::new("idle",
///         TransitionCondition::FloatLt { param: "speed".into(), threshold: 0.1 }
///     ))
/// );
///
/// // W on_update:
/// anim.params.set_float("speed", velocity.length());
/// anim.update(dt);
/// anim.apply(&mut sprite);
/// ```
pub struct Animator {
    states:   HashMap<String, AnimationState>,
    current:  String,
    playback: PlaybackState,
    pub params: AnimatorParams,
    /// Aktualny UV rect (wynik ostatniego update)
    current_uv: [f32; 4],
    /// Szybkość odtwarzania (1.0 = normalna)
    pub speed: f32,
    /// Czy animacja jest wstrzymana
    pub paused: bool,
}

impl Animator {
    pub fn new(initial_state: &str) -> Self {
        Self {
            states:     HashMap::new(),
            current:    initial_state.to_string(),
            playback:   PlaybackState::default(),
            params:     AnimatorParams::default(),
            current_uv: [0.0, 0.0, 1.0, 1.0],
            speed:      1.0,
            paused:     false,
        }
    }

    /// Dodaj stan. Zwraca &mut Self dla chainowania.
    pub fn add_state(&mut self, state: AnimationState) -> &mut Self {
        self.states.insert(state.name.clone(), state);
        self
    }

    /// Przejdź do stanu natychmiastowo (reset playback).
    pub fn set_state(&mut self, name: &str) {
        if self.current != name {
            self.current  = name.to_string();
            self.playback = PlaybackState::default();
        }
    }

    /// Bieżąca nazwa stanu.
    pub fn current_state(&self) -> &str {
        &self.current
    }

    /// Czy animacja dobiegła końca (tylko one-shot).
    pub fn is_finished(&self) -> bool {
        self.playback.finished
    }

    /// Aktualizuj animację — wywołaj co klatkę przed `apply`.
    pub fn update(&mut self, dt: f32) {
        if self.paused { return; }

        let dt_scaled = dt * self.speed;

        // Pobierz bieżący stan (jeśli nie istnieje — nic nie rób)
        let state = match self.states.get(&self.current).cloned() {
            Some(s) => s,
            None    => return,
        };

        let clip = &state.clip;

        if clip.frames.is_empty() { return; }

        // Ustaw UV bieżącej klatki
        let frame_idx = self.playback.frame.min(clip.frames.len() - 1);
        self.current_uv = clip.frames[frame_idx].uv;

        // Nie aktualizuj timera jeśli one-shot zakończony
        if self.playback.finished && !clip.looping {
            return;
        }

        // Zaawansuj timer
        self.playback.frame_timer += dt_scaled;
        self.playback.elapsed     += dt_scaled;

        let frame_dur = clip.frame_dur(self.playback.frame);

        // Przejdź do następnej klatki
        while self.playback.frame_timer >= frame_dur {
            self.playback.frame_timer -= frame_dur;
            self.playback.frame += 1;

            if self.playback.frame >= clip.frames.len() {
                if clip.looping {
                    self.playback.frame = 0;
                } else {
                    self.playback.frame    = clip.frames.len() - 1;
                    self.playback.finished = true;
                    break;
                }
            }
        }

        // Sprawdź przejścia
        let next = self.evaluate_transitions(&state);
        if let Some(target) = next {
            self.params.clear_triggers();
            self.current  = target;
            self.playback = PlaybackState::default();
        }
    }

    /// Aplikuj bieżące UV do sprite'a.
    pub fn apply(&self, sprite: &mut Sprite) {
        let [u0, v0, u1, v1] = self.current_uv;
        sprite.uv_rect = [u0, v0, u1, v1];
    }

    /// Pobierz bieżące UV bez modyfikowania sprite'a.
    pub fn current_uv(&self) -> [f32; 4] {
        self.current_uv
    }

    // ── Internale ─────────────────────────────────────────────────────────────

    fn evaluate_transitions(&mut self, state: &AnimationState) -> Option<String> {
        // Sortuj przejścia po priorytecie (wyższy = wcześniej)
        let mut transitions = state.transitions.clone();
        transitions.sort_by(|a, b| b.priority.cmp(&a.priority));

        for t in &transitions {
            let ok = match &t.condition {
                TransitionCondition::FloatGt { param, threshold } => {
                    self.params.get_float(param) > *threshold
                }
                TransitionCondition::FloatLt { param, threshold } => {
                    self.params.get_float(param) < *threshold
                }
                TransitionCondition::BoolIs { param, expected } => {
                    self.params.get_bool(param) == *expected
                }
                TransitionCondition::Trigger { param } => {
                    // Trigger jest konsumowany
                    let name = param.clone();
                    self.params.consume_trigger(&name)
                }
                TransitionCondition::OnFinish => {
                    self.playback.finished
                }
                TransitionCondition::After { secs } => {
                    self.playback.elapsed >= *secs
                }
            };

            if ok {
                return Some(t.target.clone());
            }
        }

        None
    }
}

// ─── AnimatorBuilder ──────────────────────────────────────────────────────────

/// Wygodny builder.
///
/// ```rust
/// let animator = AnimatorBuilder::new("idle")
///     .state(AnimationState::new("idle", idle_clip)
///         .with_transition(Transition::new("run",
///             TransitionCondition::FloatGt { param: "speed".into(), threshold: 10.0 }
///         ))
///     )
///     .state(AnimationState::new("run", run_clip)
///         .with_transition(Transition::new("idle",
///             TransitionCondition::FloatLt { param: "speed".into(), threshold: 10.0 }
///         ))
///     )
///     .build();
/// ```
pub struct AnimatorBuilder {
    animator: Animator,
}

impl AnimatorBuilder {
    pub fn new(initial: &str) -> Self {
        Self { animator: Animator::new(initial) }
    }

    pub fn state(mut self, state: AnimationState) -> Self {
        self.animator.add_state(state);
        self
    }

    pub fn speed(mut self, speed: f32) -> Self {
        self.animator.speed = speed;
        self
    }

    pub fn build(self) -> Animator {
        self.animator
    }
}
