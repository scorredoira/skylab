//! The picker's map: where the view is (Web Mercator, as the tiles are cut), which tiles it needs,
//! the tiles kept and loaded in the background, and how they are drawn with egui.
//!
//! The world is a square of side 1 (x east from the antimeridian, y south from 85° N); the view
//! is its centre and a zoom: the world is `TILE_POINTS · 2^zoom` points wide on screen. The tiles
//! of zoom `floor(zoom)` (OpenFreeMap has them to 14; nearer, those of 14 drawn bigger) are each
//! one to two times `TILE_POINTS` across, so a screen needs about a dozen of them.
//!
//! Drawn with egui shapes rather than a pass of our own: a tile is decoded and triangulated once,
//! on a thread of its own (`tiles::shapes`), and each frame its triangles are only moved and scaled
//! into one mesh, and its lines and names drawn by egui (smooth, at any zoom). A screen of a city
//! is some 100 000 vertices, a few milliseconds; it also draws the same in `--shot` and on the web.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use engine::egui::{self, epaint::Vertex, pos2, vec2, Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke};
use engine::Instant;
use geodata::basemap::{Basemap, MAX_ZOOM};
use geodata::geo::{tile_to_earth, tile_xy, TileId};

use super::tiles::{self, LabelKind, Shapes};

/// Points across a tile drawn at its own zoom.
pub const TILE_POINTS: f64 = 512.0;
/// The nearest the map goes: an aerodrome's runways and taxiways across the screen.
pub const MAX_VIEW_ZOOM: f64 = 17.0;

/// A point of the Earth (degrees) on the map's square.
pub fn mercator(latitude: f64, longitude: f64) -> (f64, f64) {
    tile_xy(latitude, longitude, 0)
}

/// The latitude and longitude (degrees) of a point of the map's square.
pub fn earth(x: f64, y: f64) -> (f64, f64) {
    tile_to_earth(x, y, 0)
}

/// What part of the world the map shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The point in the middle of the map, on the map's square.
    pub centre: (f64, f64),
    pub zoom: f64,
}

impl View {
    /// The view centred on a latitude and longitude.
    pub fn at(latitude: f64, longitude: f64, zoom: f64) -> View {
        View { centre: mercator(latitude, longitude), zoom }
    }

    /// Points per unit of the map's square.
    pub fn scale(&self) -> f64 {
        TILE_POINTS * self.zoom.exp2()
    }

    /// Where a point of the map's square is on the screen.
    pub fn screen(self, rect: Rect, p: (f64, f64)) -> Pos2 {
        let s = self.scale();
        let c = rect.center();
        pos2(c.x + ((p.0 - self.centre.0) * s) as f32, c.y + ((p.1 - self.centre.1) * s) as f32)
    }

    /// The point of the map's square under a point of the screen.
    pub fn map(self, rect: Rect, at: Pos2) -> (f64, f64) {
        let s = self.scale();
        let c = rect.center();
        (self.centre.0 + (at.x - c.x) as f64 / s, self.centre.1 + (at.y - c.y) as f64 / s)
    }

    /// Moves the map with the pointer dragging it by `delta` points.
    pub fn pan(&mut self, rect: Rect, delta: egui::Vec2) {
        let s = self.scale();
        self.centre.0 -= delta.x as f64 / s;
        self.centre.1 -= delta.y as f64 / s;
        self.keep_in(rect);
    }

    /// Zooms by `steps` (one doubles the scale; negative, away), keeping the point under `at` where it is.
    pub fn zoom_about(&mut self, rect: Rect, at: Pos2, steps: f64) {
        let before = self.map(rect, at);
        self.zoom = (self.zoom + steps).clamp(Self::min_zoom(rect), MAX_VIEW_ZOOM);
        let after = self.map(rect, at);
        self.centre.0 += before.0 - after.0;
        self.centre.1 += before.1 - after.1;
        self.keep_in(rect);
    }

    /// The farthest zoom: the whole world as wide as the map.
    pub fn min_zoom(rect: Rect) -> f64 {
        (rect.width() as f64 / TILE_POINTS).log2()
    }

    /// Keeps the zoom in its range and the screen on the world (in the middle when the world is
    /// smaller than the screen).
    pub fn keep_in(&mut self, rect: Rect) {
        self.zoom = self.zoom.clamp(Self::min_zoom(rect), MAX_VIEW_ZOOM);
        let s = self.scale();
        let (hx, hy) = (rect.width() as f64 / 2.0 / s, rect.height() as f64 / 2.0 / s);
        let clamp = |v: f64, half: f64| if half >= 0.5 { 0.5 } else { v.clamp(half, 1.0 - half) };
        self.centre = (clamp(self.centre.0, hx), clamp(self.centre.1, hy));
    }

    /// The zoom of the tiles drawn.
    pub fn tile_zoom(&self) -> u8 {
        (self.zoom.floor().max(0.0) as u8).min(MAX_ZOOM)
    }

    /// The tiles of `tile_zoom` that the map shows, the middle ones first.
    pub fn tiles(&self, rect: Rect) -> Vec<TileId> {
        let z = self.tile_zoom();
        let n = 1u32 << z;
        let (a, b) = (self.map(rect, rect.min), self.map(rect, rect.max));
        let cells = |v0: f64, v1: f64| (v0 * n as f64).floor().max(0.0) as u32..=((v1 * n as f64).floor().max(0.0) as u32).min(n - 1);
        let middle = (self.centre.0 * n as f64, self.centre.1 * n as f64);
        let mut list: Vec<TileId> = cells(a.1, b.1).flat_map(|y| cells(a.0, b.0).map(move |x| TileId { z, x, y })).collect();
        let away = |t: &TileId| (t.x as f64 + 0.5 - middle.0).powi(2) + (t.y as f64 + 0.5 - middle.1).powi(2);
        list.sort_by(|a, b| away(a).total_cmp(&away(b)));
        list
    }

    /// Where a tile is on the screen.
    pub fn tile_rect(&self, rect: Rect, id: TileId) -> Rect {
        let n = (1u64 << id.z) as f64;
        Rect::from_min_max(self.screen(rect, (id.x as f64 / n, id.y as f64 / n)), self.screen(rect, ((id.x + 1) as f64 / n, (id.y + 1) as f64 / n)))
    }

    /// Metres of the ground in a point of the screen at the middle of the map.
    pub fn metres_per_point(&self) -> f64 {
        let (latitude, _) = earth(self.centre.0, self.centre.1);
        geodata::geo::EARTH_RADIUS * std::f64::consts::TAU * latitude.to_radians().cos() / self.scale()
    }
}

/// The tiles in memory, and those being read or downloaded.
pub struct Tiles {
    basemap: Arc<Basemap>,
    kept: HashMap<TileId, Slot>,
    sender: Sender<(TileId, Option<Arc<Shapes>>)>,
    done: Receiver<(TileId, Option<Arc<Shapes>>)>,
    loading: usize,
    /// Counts the frames, to know which tiles were used least lately.
    frame: u64,
}

enum Slot {
    Loading,
    Ready(Arc<Shapes>, u64),
    /// None to be had (no network, or the server failed): asked again after a while.
    Failed(Instant),
}

/// Tiles read or downloaded at once.
const LOADING: usize = 6;
/// Memory kept for tiles, bytes; past it the least used lately are let go.
const KEPT_BYTES: usize = 256 << 20;
/// Seconds before a tile that could not be had is asked for again.
const RETRY: f64 = 20.0;

impl Tiles {
    pub fn new(offline: bool) -> Tiles {
        let (sender, done) = channel();
        Tiles { basemap: Arc::new(Basemap::new(offline)), kept: HashMap::new(), sender, done, loading: 0, frame: 0 }
    }

    /// Takes in the tiles finished since the last frame and lets go of the least used past `KEPT_BYTES`.
    pub fn update(&mut self) {
        self.frame += 1;
        while let Ok((id, shapes)) = self.done.try_recv() {
            self.loading -= 1;
            self.kept.insert(id, shapes.map_or(Slot::Failed(Instant::now()), |s| Slot::Ready(s, self.frame)));
        }
        let mut ready: Vec<(u64, TileId, usize)> = self.kept.iter().filter_map(|(id, s)| if let Slot::Ready(t, used) = s { Some((*used, *id, t.bytes())) } else { None }).collect();
        let mut total: usize = ready.iter().map(|r| r.2).sum();
        if total > KEPT_BYTES {
            ready.sort_by_key(|r| r.0);
            for (used, id, bytes) in ready {
                if total <= KEPT_BYTES * 3 / 4 || used + 2 >= self.frame {
                    break;
                }
                self.kept.remove(&id);
                total -= bytes;
            }
        }
    }

    /// The tile, if it is ready; asks for it otherwise.
    pub fn get(&mut self, id: TileId) -> Option<Arc<Shapes>> {
        match self.kept.get_mut(&id) {
            Some(Slot::Ready(shapes, used)) => {
                *used = self.frame;
                return Some(shapes.clone());
            }
            Some(Slot::Loading) => return None,
            Some(Slot::Failed(when)) if when.elapsed().as_secs_f64() < RETRY => return None,
            _ => {}
        }
        if self.loading < LOADING {
            self.loading += 1;
            self.kept.insert(id, Slot::Loading);
            let (basemap, sender) = (self.basemap.clone(), self.sender.clone());
            engine::threads::spawn(move || {
                let shapes = match basemap.tile(id) {
                    Ok(Some(bytes)) => Some(Arc::new(tiles::shapes(&bytes, id.z))),
                    Ok(None) => Some(Arc::new(Shapes::default())),
                    Err(_) => None,
                };
                let _ = sender.send((id, shapes));
            });
        }
        None
    }

    /// The nearest ready tile above this one (a quarter of it, or less, is this one), to draw while it loads.
    pub fn ancestor(&mut self, id: TileId) -> Option<(TileId, Arc<Shapes>)> {
        let mut up = id;
        while up.z > 0 {
            up = TileId { z: up.z - 1, x: up.x / 2, y: up.y / 2 };
            if let Some(Slot::Ready(shapes, used)) = self.kept.get_mut(&up) {
                *used = self.frame;
                return Some((up, shapes.clone()));
            }
        }
        None
    }

    /// Tiles still being read or downloaded.
    pub fn loading(&self) -> usize {
        self.loading
    }

    /// The tiles kept in memory, and their bytes.
    pub fn kept(&self) -> (usize, usize) {
        self.kept.values().filter_map(|s| if let Slot::Ready(t, _) = s { Some(t.bytes()) } else { None }).fold((0, 0), |(n, b), t| (n + 1, b + t))
    }
}

/// Draws the map's tiles into `rect`: the land, then each tile's areas and lines (while a tile
/// loads, the nearest one above it that is ready). Returns how many tiles are still missing from
/// the picture, and the names of places, to be written once what goes over the map is drawn.
pub fn draw(painter: &Painter, rect: Rect, view: &View, tiles: &mut Tiles) -> (usize, Vec<PlaceLabel>) {
    painter.rect_filled(rect, 0.0, tiles::LAND);
    let wanted = view.tiles(rect);
    let mut drawn: Vec<(TileId, Arc<Shapes>)> = Vec::new();
    let mut under: Vec<(TileId, Arc<Shapes>)> = Vec::new();
    let mut missing = 0;
    for &id in &wanted {
        match tiles.get(id) {
            Some(shapes) => drawn.push((id, shapes)),
            None => {
                missing += 1;
                if let Some(a) = tiles.ancestor(id) {
                    if !under.iter().any(|u| u.0 == a.0) {
                        under.push(a);
                    }
                }
            }
        }
    }
    // Bigger tiles first, under the ones of the right zoom.
    under.sort_by_key(|u| u.0.z);
    let all: Vec<&(TileId, Arc<Shapes>)> = under.iter().chain(&drawn).collect();
    // How much bigger than at its own zoom the lines of a tile are drawn: they grow as the map nears.
    let grow = |id: TileId| ((view.zoom - id.z as f64).exp2() as f32).clamp(1.0, 2.5);
    let mut mesh = Mesh::default();
    for (id, shapes) in &all {
        let r = view.tile_rect(rect, *id);
        let base = mesh.vertices.len() as u32;
        mesh.vertices.extend(shapes.mesh.vertices.iter().map(|v| Vertex { pos: pos2(r.min.x + v.pos.x * r.width(), r.min.y + v.pos.y * r.height()), ..*v }));
        mesh.indices.extend(shapes.mesh.indices.iter().map(|i| base + i));
    }
    painter.add(Shape::mesh(mesh));
    for (id, shapes) in &all {
        let r = view.tile_rect(rect, *id);
        let grow = grow(*id);
        for line in &shapes.lines {
            let (colour, width) = line.kind.stroke(id.z);
            let points: Vec<Pos2> = line.points.iter().map(|p| pos2(r.min.x + p[0] * r.width(), r.min.y + p[1] * r.height())).collect();
            painter.add(Shape::line(points, Stroke::new(width * grow, colour)));
        }
    }
    // Names, from the tiles of the right zoom only (those under are about to go).
    let mut labels = Vec::new();
    for (id, shapes) in &drawn {
        let r = view.tile_rect(rect, *id);
        labels.extend(shapes.labels.iter().map(|l| PlaceLabel { kind: l.kind, at: pos2(r.min.x + l.at[0] * r.width(), r.min.y + l.at[1] * r.height()), text: l.text.clone() }));
    }
    (missing, labels)
}

/// A place's name and where it goes on the screen.
pub struct PlaceLabel {
    kind: LabelKind,
    at: Pos2,
    text: String,
}

/// Writes the names of places that overlap neither one another nor what is in `taken`, the most
/// important first, and adds where each went to `taken`.
pub fn draw_labels(painter: &Painter, rect: Rect, mut labels: Vec<PlaceLabel>, taken: &mut Vec<Rect>) {
    labels.sort_by_key(|l| l.kind);
    for PlaceLabel { kind, at, text } in labels {
        let (size, colour, dot) = match kind {
            LabelKind::Country => (13.0, Color32::from_rgb(196, 186, 214), false),
            LabelKind::City(rank) if rank <= 3 => (13.0, Color32::from_gray(232), true),
            LabelKind::City(_) => (12.0, Color32::from_gray(214), true),
            LabelKind::Town => (11.0, Color32::from_gray(196), true),
            LabelKind::Village => (10.0, Color32::from_gray(170), false),
        };
        let text = if kind == LabelKind::Country { text.to_uppercase() } else { text };
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(size), colour);
        let place = if dot { at + vec2(0.0, -galley.size().y / 2.0 - 4.0) } else { at };
        let label = Rect::from_center_size(place, galley.size()).expand(2.0);
        if !rect.contains_rect(label) || taken.iter().any(|t| t.intersects(label)) {
            continue;
        }
        taken.push(label);
        if dot {
            painter.circle_filled(at, 2.0, colour);
        }
        halo(painter, label.min + vec2(2.0, 2.0), galley);
    }
}

/// Text on the map with a dark edge round it, so it reads on land and water alike.
pub fn halo(painter: &Painter, at: Pos2, galley: Arc<egui::Galley>) {
    let shadow = Color32::from_black_alpha(200);
    for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
        painter.add(egui::epaint::TextShape::new(at + d, galley.clone(), shadow).with_override_text_color(shadow));
    }
    painter.galley(at, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::from_min_size(pos2(340.0, 0.0), vec2(1260.0, 1000.0))
    }

    #[test]
    fn points_go_to_the_screen_and_back() {
        let rect = screen();
        let view = View::at(39.8626, 4.2186, 9.3);
        // The middle of the view is the middle of the map.
        let middle = view.screen(rect, mercator(39.8626, 4.2186));
        assert!((middle - rect.center()).length() < 1e-3);
        for at in [pos2(400.0, 20.0), pos2(1500.0, 900.0), rect.center()] {
            let (x, y) = view.map(rect, at);
            assert!((view.screen(rect, (x, y)) - at).length() < 1e-3);
            let (lat, lon) = earth(x, y);
            let again = mercator(lat, lon);
            assert!((again.0 - x).abs() < 1e-12 && (again.1 - y).abs() < 1e-12);
        }
        // East is right, north is up.
        assert!(view.screen(rect, mercator(39.8626, 4.5)).x > middle.x);
        assert!(view.screen(rect, mercator(40.0, 4.2186)).y < middle.y);
        // A kilometre at zoom 9.3 over Menorca is some 10.5 points, the same north as east.
        let km = view.screen(rect, mercator(39.8626 + 1.0 / 111.195, 4.2186)).y - middle.y;
        assert!((km + 1000.0 / view.metres_per_point() as f32).abs() < 0.05, "{km}");
        assert!((10.0..11.0).contains(&-km), "{km}");
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let rect = screen();
        let mut view = View::at(47.26, 11.34, 8.0);
        let at = pos2(500.0, 300.0);
        let under = view.map(rect, at);
        view.zoom_about(rect, at, 1.5);
        assert_eq!(view.zoom, 9.5);
        let still = view.map(rect, at);
        assert!((still.0 - under.0).abs() < 1e-12 && (still.1 - under.1).abs() < 1e-12);
        // Never nearer than an aerodrome, never farther than the whole world as wide as the map.
        view.zoom_about(rect, at, 30.0);
        assert_eq!(view.zoom, MAX_VIEW_ZOOM);
        view.zoom_about(rect, at, -30.0);
        assert!((view.scale() - rect.width() as f64).abs() < 1e-6);
        assert_eq!(view.centre.0, 0.5, "the whole world is in the middle");
        // Dragging moves the map with the pointer.
        let mut view = View::at(47.26, 11.34, 8.0);
        let before = view.screen(rect, mercator(47.26, 11.34));
        view.pan(rect, vec2(40.0, -25.0));
        assert!((view.screen(rect, mercator(47.26, 11.34)) - before - vec2(40.0, -25.0)).length() < 1e-3);
    }

    #[test]
    fn the_tiles_that_cover_the_screen_the_middle_first() {
        let rect = screen();
        // The whole world: the one tile of zoom 1 at 1.3 is four.
        let world = View { centre: (0.5, 0.5), zoom: View::min_zoom(rect) };
        assert_eq!(world.tile_zoom(), 1);
        assert_eq!(world.tiles(rect).len(), 4);
        // Over Menorca at zoom 9.3: the tiles of zoom 9, each 630 points, the middle one first.
        let view = View::at(39.8626, 4.2186, 9.3);
        let tiles = view.tiles(rect);
        let (x, y) = tile_xy(39.8626, 4.2186, 9);
        assert_eq!(tiles[0], TileId { z: 9, x: x as u32, y: y as u32 });
        assert!((4..=12).contains(&tiles.len()), "{}", tiles.len());
        for t in &tiles {
            assert!(view.tile_rect(rect, *t).intersects(rect));
            assert!((view.tile_rect(rect, *t).width() - 630.3).abs() < 1.0);
        }
        // Nearer than the server's tiles go, its last zoom drawn bigger.
        let near = View::at(27.6868, 86.7295, 16.4);
        assert_eq!(near.tile_zoom(), MAX_ZOOM);
        assert!(near.tiles(rect).len() <= 4);
    }
}
