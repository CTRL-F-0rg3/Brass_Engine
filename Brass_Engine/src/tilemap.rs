// =============================================================================
//  Brass Engine — Tilemap
//
//  TileSet   — tekstura podzielona na kafelki (sprite sheet)
//  TileMap   — siatka indeksów kafelków z renderowaniem przez Renderer2D
//  TileLayer — wiele warstw z różnymi z_order
// =============================================================================

use glam::Vec2;
use crate::render::renderer2d::{Color, Renderer2D, Sprite};

// ─── TileSet ──────────────────────────────────────────────────────────────────

/// Deskryptor jednego kafelka — nadpisuje domyślne UV lub dodaje właściwości
#[derive(Clone, Debug)]
pub struct TileMeta {
    /// Czy kafelek blokuje ruch (do kolizji / pathfinding)
    pub solid:    bool,
    /// Opcjonalny tag (np. "water", "lava", "grass")
    pub tag:      Option<String>,
    /// Kolor tintujący (domyślnie biały = bez modyfikacji)
    pub tint:     Color,
}

impl Default for TileMeta {
    fn default() -> Self {
        Self { solid: false, tag: None, tint: Color::WHITE }
    }
}

/// Sprite sheet podzielony na kafelki o jednakowym rozmiarze.
///
/// ```
/// let tileset = TileSet::new(texture_id, 512, 512, 16, 16);
/// // → 32×32 kafelki, każdy 16×16 pikseli
/// ```
#[derive(Clone, Debug)]
pub struct TileSet {
    /// ID tekstury (z TextureManager / Renderer2D::load_texture_bytes)
    pub texture_id:  u64,
    /// Rozmiar całego sprite sheetu (piksele)
    pub sheet_w:     u32,
    pub sheet_h:     u32,
    /// Rozmiar jednego kafelka (piksele)
    pub tile_w:      u32,
    pub tile_h:      u32,
    /// Liczba kolumn i wierszy
    pub cols:        u32,
    pub rows:        u32,
    /// Opcjonalne właściwości per-kafelek (indeksowane tile_id)
    pub meta:        Vec<TileMeta>,
}

impl TileSet {
    /// Utwórz TileSet.
    /// `sheet_w/h` — wymiary tekstury, `tile_w/h` — rozmiar jednego kafelka.
    pub fn new(texture_id: u64, sheet_w: u32, sheet_h: u32, tile_w: u32, tile_h: u32) -> Self {
        let cols = sheet_w / tile_w;
        let rows = sheet_h / tile_h;
        let count = (cols * rows) as usize;
        Self {
            texture_id,
            sheet_w, sheet_h,
            tile_w, tile_h,
            cols, rows,
            meta: vec![TileMeta::default(); count],
        }
    }

    /// Zwraca UV rect [u0, v0, u1, v1] dla kafelka o danym indeksie.
    /// Indeks = row * cols + col (od 0, lewo-górny).
    pub fn uv_for_tile(&self, tile_id: u32) -> [f32; 4] {
        let col = tile_id % self.cols;
        let row = tile_id / self.cols;

        let sw = self.sheet_w as f32;
        let sh = self.sheet_h as f32;
        let tw = self.tile_w  as f32;
        let th = self.tile_h  as f32;

        let u0 = col as f32 * tw / sw;
        let v0 = row as f32 * th / sh;
        let u1 = u0 + tw / sw;
        let v1 = v0 + th / sh;

        [u0, v0, u1, v1]
    }

    /// Indeks kafelka z (col, row).
    pub fn tile_id(&self, col: u32, row: u32) -> u32 {
        row * self.cols + col
    }

    /// Ustaw metadane dla kafelka.
    pub fn set_meta(&mut self, tile_id: u32, meta: TileMeta) {
        if let Some(m) = self.meta.get_mut(tile_id as usize) {
            *m = meta;
        }
    }

    /// Pobierz metadane kafelka.
    pub fn meta(&self, tile_id: u32) -> &TileMeta {
        self.meta.get(tile_id as usize).unwrap_or(&TileMeta {
            solid: false, tag: None, tint: Color::WHITE,
        })
    }

    /// Oznacz kafelki jako solid (shortcut).
    pub fn set_solid(&mut self, tile_ids: &[u32]) {
        for &id in tile_ids {
            if let Some(m) = self.meta.get_mut(id as usize) {
                m.solid = true;
            }
        }
    }
}

// ─── TileLayer ────────────────────────────────────────────────────────────────

/// Pojedyncza warstwa tilemapa.
/// Wiele warstw (tło, obiekty, overlay) z różnymi `z_order`.
#[derive(Clone, Debug)]
pub struct TileLayer {
    pub name:     String,
    /// Dane warstwy: `tiles[row * width + col]`, None = pusta komórka
    pub tiles:    Vec<Option<u32>>,
    pub width:    u32,
    pub height:   u32,
    /// Głębokość renderowania (0.0 = tył, 1.0 = przód)
    pub z_order:  f32,
    /// Czy warstwa jest widoczna
    pub visible:  bool,
    /// Globalny tint warstwy
    pub tint:     Color,
    /// Przesunięcie warstwy w pikselach (parallax/offset)
    pub offset:   Vec2,
}

impl TileLayer {
    pub fn new(name: &str, width: u32, height: u32, z_order: f32) -> Self {
        Self {
            name:    name.to_string(),
            tiles:   vec![None; (width * height) as usize],
            width,
            height,
            z_order,
            visible: true,
            tint:    Color::WHITE,
            offset:  Vec2::ZERO,
        }
    }

    /// Ustaw kafelek na pozycji (col, row).
    pub fn set(&mut self, col: u32, row: u32, tile_id: Option<u32>) {
        if col < self.width && row < self.height {
            self.tiles[(row * self.width + col) as usize] = tile_id;
        }
    }

    /// Pobierz tile_id na pozycji (col, row).
    pub fn get(&self, col: u32, row: u32) -> Option<u32> {
        if col < self.width && row < self.height {
            self.tiles[(row * self.width + col) as usize]
        } else {
            None
        }
    }

    /// Wypełnij prostokąt kafelkami (col, row, w, h, tile_id).
    pub fn fill_rect(&mut self, col: u32, row: u32, w: u32, h: u32, tile_id: u32) {
        for r in row..row + h {
            for c in col..col + w {
                self.set(c, r, Some(tile_id));
            }
        }
    }

    /// Wypełnij całą warstwę jednym kafelkiem.
    pub fn fill_all(&mut self, tile_id: u32) {
        for t in &mut self.tiles {
            *t = Some(tile_id);
        }
    }

    /// Wyczyść warstwę.
    pub fn clear(&mut self) {
        for t in &mut self.tiles {
            *t = None;
        }
    }

    /// Iteruj po niepustych kafelkach: `(col, row, tile_id)`
    pub fn iter_tiles(&self) -> impl Iterator<Item = (u32, u32, u32)> + '_ {
        self.tiles.iter().enumerate().filter_map(|(i, &t)| {
            t.map(|id| {
                let col = i as u32 % self.width;
                let row = i as u32 / self.width;
                (col, row, id)
            })
        })
    }
}

// ─── TileMap ──────────────────────────────────────────────────────────────────

/// Kompletna tilemap — zestaw warstw + TileSet + rozmiar kafelka w pikselach świata.
///
/// ```rust
/// let mut map = TileMap::new(tileset, 16.0, 16.0);
/// let bg = map.add_layer("bg", 20, 15, 0.1);
/// map.layer_mut(bg).fill_all(0);
/// map.draw(r2d);
/// ```
pub struct TileMap {
    pub tileset:        TileSet,
    /// Rozmiar kafelka w pikselach przestrzeni świata
    pub tile_pixel_w:   f32,
    pub tile_pixel_h:   f32,
    /// Pozycja całej mapy (lewy górny róg)
    pub position:       Vec2,
    /// Warstwy — renderowane rosnąco po z_order
    layers:             Vec<TileLayer>,
}

impl TileMap {
    pub fn new(tileset: TileSet, tile_pixel_w: f32, tile_pixel_h: f32) -> Self {
        Self {
            tileset,
            tile_pixel_w,
            tile_pixel_h,
            position: Vec2::ZERO,
            layers:   Vec::new(),
        }
    }

    /// Dodaj warstwę i zwróć jej indeks.
    pub fn add_layer(&mut self, name: &str, width: u32, height: u32, z_order: f32) -> usize {
        self.layers.push(TileLayer::new(name, width, height, z_order));
        self.layers.len() - 1
    }

    pub fn layer(&self, idx: usize) -> &TileLayer {
        &self.layers[idx]
    }

    pub fn layer_mut(&mut self, idx: usize) -> &mut TileLayer {
        &mut self.layers[idx]
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Zwraca pozycję w pikselach dla (col, row).
    pub fn tile_world_pos(&self, col: u32, row: u32) -> Vec2 {
        Vec2::new(
            self.position.x + col as f32 * self.tile_pixel_w + self.tile_pixel_w * 0.5,
            self.position.y + row as f32 * self.tile_pixel_h + self.tile_pixel_h * 0.5,
        )
    }

    /// Konwertuje pozycję w pikselach → (col, row). Zwraca None jeśli poza mapą.
    pub fn world_to_tile(&self, pos: Vec2) -> Option<(u32, u32)> {
        let local = pos - self.position;
        if local.x < 0.0 || local.y < 0.0 {
            return None;
        }
        let col = (local.x / self.tile_pixel_w) as u32;
        let row = (local.y / self.tile_pixel_h) as u32;

        // Sprawdź czy w granicach pierwszej warstwy
        if let Some(layer) = self.layers.first() {
            if col < layer.width && row < layer.height {
                return Some((col, row));
            }
        }
        None
    }

    /// Czy kafelek na danej pozycji jest solid (w dowolnej warstwie).
    pub fn is_solid(&self, col: u32, row: u32) -> bool {
        for layer in &self.layers {
            if let Some(tile_id) = layer.get(col, row) {
                if self.tileset.meta(tile_id).solid {
                    return true;
                }
            }
        }
        false
    }

    /// Czy punkt w pikselach świata stoi na solidnym kafelku.
    pub fn is_solid_at(&self, pos: Vec2) -> bool {
        self.world_to_tile(pos)
            .map(|(c, r)| self.is_solid(c, r))
            .unwrap_or(false)
    }

    /// Wyślij wszystkie warstwy do Renderer2D.
    /// Renderuje tylko kafelki w `camera_rect` (opcjonalny frustum culling).
    /// `camera_rect` = (left, top, right, bottom) w pikselach świata.
    pub fn draw(&self, r2d: &mut Renderer2D) {
        self.draw_culled(r2d, None);
    }

    pub fn draw_culled(
        &self,
        r2d: &mut Renderer2D,
        camera_rect: Option<(f32, f32, f32, f32)>,
    ) {
        // Sortuj warstwy po z_order
        let mut sorted: Vec<&TileLayer> = self.layers.iter()
            .filter(|l| l.visible)
            .collect();
        sorted.sort_by(|a, b| a.z_order.partial_cmp(&b.z_order).unwrap());

        for layer in sorted {
            let layer_offset = self.position + layer.offset;

            for (col, row, tile_id) in layer.iter_tiles() {
                let cx = layer_offset.x + col as f32 * self.tile_pixel_w + self.tile_pixel_w * 0.5;
                let cy = layer_offset.y + row as f32 * self.tile_pixel_h + self.tile_pixel_h * 0.5;

                // Frustum culling
                if let Some((left, top, right, bottom)) = camera_rect {
                    let hw = self.tile_pixel_w * 0.5;
                    let hh = self.tile_pixel_h * 0.5;
                    if cx + hw < left || cx - hw > right || cy + hh < top || cy - hh > bottom {
                        continue;
                    }
                }

                let uv   = self.tileset.uv_for_tile(tile_id);
                let meta = self.tileset.meta(tile_id);

                // Łącz tint warstwy z tintem kafelka
                let tint = Color::rgba(
                    layer.tint.r * meta.tint.r,
                    layer.tint.g * meta.tint.g,
                    layer.tint.b * meta.tint.b,
                    layer.tint.a * meta.tint.a,
                );

                let sprite = Sprite::new(
                    Vec2::new(cx, cy),
                    Vec2::new(self.tile_pixel_w, self.tile_pixel_h),
                )
                .with_texture(self.tileset.texture_id)
                .with_uv(uv[0], uv[1], uv[2], uv[3])
                .with_color(tint);

                // Ręczne ustawienie z_order (Sprite::new nie ma buildera)
                let mut s = sprite;
                s.layer = layer.z_order as u8;

                r2d.draw_sprite(s);
            }
        }
    }
}

// ─── TileMapBuilder ───────────────────────────────────────────────────────────

/// Wygodny builder do tworzenia mapy z danych tekstowych.
///
/// ```rust
/// let map = TileMapBuilder::new(tileset, 16.0, 16.0)
///     .layer("bg", 0.1, vec![
///         "000000000",
///         "001111100",
///         "001111100",
///         "000000000",
///     ])
///     .build();
/// ```
/// Znaki: '0'–'9' = tile_id 0-9, ' ' = pusta komórka
/// Dla większych ID użyj layer_raw().
pub struct TileMapBuilder {
    map: TileMap,
}

impl TileMapBuilder {
    pub fn new(tileset: TileSet, tile_w: f32, tile_h: f32) -> Self {
        Self { map: TileMap::new(tileset, tile_w, tile_h) }
    }

    pub fn position(mut self, pos: Vec2) -> Self {
        self.map.position = pos;
        self
    }

    /// Dodaj warstwę z danych tekstowych (znaki '0'-'9' lub ' ').
    pub fn layer(mut self, name: &str, z_order: f32, rows: Vec<&str>) -> Self {
        let height = rows.len() as u32;
        let width  = rows.iter().map(|r| r.len()).max().unwrap_or(0) as u32;
        let idx    = self.map.add_layer(name, width, height, z_order);
        let layer  = self.map.layer_mut(idx);

        for (r, row_str) in rows.iter().enumerate() {
            for (c, ch) in row_str.chars().enumerate() {
                let tile_id = match ch {
                    ' ' => None,
                    c if c.is_ascii_digit() => Some(c as u32 - '0' as u32),
                    _ => None,
                };
                layer.set(c as u32, r as u32, tile_id);
            }
        }
        self
    }

    /// Dodaj warstwę z surowych danych (Vec<Option<u32>>).
    pub fn layer_raw(
        mut self,
        name:    &str,
        width:   u32,
        height:  u32,
        z_order: f32,
        tiles:   Vec<Option<u32>>,
    ) -> Self {
        let idx   = self.map.add_layer(name, width, height, z_order);
        let layer = self.map.layer_mut(idx);
        layer.tiles = tiles;
        self
    }

    pub fn build(self) -> TileMap {
        self.map
    }
}
