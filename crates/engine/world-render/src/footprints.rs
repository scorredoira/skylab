//! The buildings and bridges of a real place's maps (`sim_core::Footprint`, `sim_core::Bridge`):
//! each tile of `TILE` metres near the camera is one mesh of all the buildings whose middle is in
//! it and the parts of bridges whose middle is (`bridges.rs`), made in the background. Far tiles
//! leave out what is too small to be seen there and the details (`FAR_TILES`, `seen_far`).
//!
//! A building is its outline raised from under the ground (or from its bottom, for a part raised
//! on others) to its roof, the roof as `sim_core::roofs` shapes it (the same numbers the simulation
//! runs into): flat, with a parapet and what stands on a roof (a lift's housing, air
//! conditioning) on the bigger ones; gabled, round and gambrel, the walls rising to meet it at
//! the gables; skillion; hipped, pyramidal and mansard from a plane on every side; domes and
//! onions. Courtyards are open. Each wall knows its facade (`Look`): what the building is for, its
//! height and the region it stands in choose plaster, brick, stone, a curtain of glass, ribbon
//! windows, a shed's sheet or a house's few windows, with shop fronts on the ground floor of
//! city blocks; colours the maps give are kept. The windows themselves are drawn by the shader
//! (`facade` in scenery.wgsl), a column of them to each `cell` of the wall, a row to each storey.
//!
//! A vertex is `FLOATS` floats (`pipelines::footprint_vertex_layout`): position relative to the
//! tile's corner (east, height above the sea, south), normal, colour, what the surface is
//! (`SURFACE_*`), the height above the building's foot, a number of the building's own, then its
//! facade: style (`STYLE_*`), storey height, the column of windows along the wall and the height
//! of the ground floor (0: none of its own).

use crate::jobs::Jobs;
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Gpu};
use sim_core::roofs::{Frame, Shape};
use sim_core::scenery::mapped_base;
use sim_core::{hash2, Footprint, Material, Roof, Terrain, Use};
use std::collections::HashMap;
use std::sync::Arc;

/// Side of a tile, metres.
pub const TILE: f64 = 512.0;
/// Floats per vertex.
pub const FLOATS: usize = 16;
/// The detail of a tile's mesh by its distance from the camera: within `NEAR_TILES` every building
/// with what stands on its roof and the parapets; then every building, plain; beyond `FAR_TILES`
/// only those that can be told apart from there.
const NEAR_TILES: f64 = 800.0;
const FAR_TILES: f64 = 2000.0;

/// How much of the detail a tile is made with (0 the most), for a distance from the camera.
fn level(d: f64) -> u8 {
    if d < NEAR_TILES {
        0
    } else if d < FAR_TILES {
        1
    } else {
        2
    }
}

/// Whether a tile made at `made` still serves at a distance `d`: the same level, or the next one
/// within a tile of their border (so that a tile does not flicker between the two).
fn serves(made: u8, d: f64) -> bool {
    let wanted = level(d);
    made == wanted || (made + 1 == wanted && level(d - TILE) == made) || (wanted + 1 == made && level(d + TILE) == made)
}
/// The most tiles drawn: those round the camera within the full quality's range.
pub const MOST_TILES: usize = 800;
/// Tiles made at once.
const IN_FLIGHT: usize = 4;
/// The walls go this far into the ground, so that no gap shows on a slope.
const SINK: f32 = sim_core::solids::SINK as f32;
/// How high a flat roof's parapet stands above it, metres.
const PARAPET: f32 = 0.9;

/// What a surface is (the vertex's `more.x`): a wall with its facade, a plain one (a parapet, a
/// housing on a roof, a bridge's pier), a flat roof, a pitched one, the road on a bridge's deck,
/// painted steel.
pub const SURFACE_WALL: f32 = 0.0;
pub const SURFACE_PLAIN: f32 = 0.3;
pub const SURFACE_STEEL: f32 = 0.6;
pub const SURFACE_DECK: f32 = 0.75;
pub const SURFACE_PITCHED: f32 = 0.9;
pub const SURFACE_FLAT: f32 = 1.0;

/// The facades (the vertex's `facade.x`; `facade` in scenery.wgsl draws each).
pub const STYLE_PLASTER: f32 = 0.0;
pub const STYLE_BRICK: f32 = 1.0;
pub const STYLE_STONE: f32 = 2.0;
pub const STYLE_GLASS: f32 = 3.0;
pub const STYLE_RIBBON: f32 = 4.0;
pub const STYLE_INDUSTRIAL: f32 = 5.0;
pub const STYLE_HOUSE: f32 = 6.0;
pub const STYLE_CHURCH: f32 = 7.0;
/// No windows.
pub const STYLE_PLAIN: f32 = 8.0;

/// How wide a column of windows is on each facade, metres (the shader puts one window in each).
fn cell(style: f32) -> f32 {
    match style as i32 {
        2 => 3.8,
        3 => 1.5,
        4 => 3.0,
        5 => 7.0,
        6 => 3.6,
        7 => 5.0,
        _ => 3.3,
    }
}

/// A tile's mesh as it is made: vertices (`FLOATS` each) and the triangles' indices.
type TileMesh = (Vec<f32>, Vec<u32>);

/// A tile's mesh on the GPU.
pub struct Mesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub count: u32,
}

struct Tile {
    /// `None` when it has no building.
    mesh: Option<Mesh>,
    level: u8,
    last_used: u64,
    /// The terrain it was made of (`Footprints::generation`).
    generation: u64,
}

/// The meshes of the tiles near the camera.
pub struct Footprints {
    terrain: Arc<Terrain>,
    /// Distance from the camera up to which tiles are drawn, metres (`Quality::building_range`).
    pub range: f64,
    tiles: HashMap<(i32, i32), Tile>,
    jobs: Jobs<(i32, i32, u8), (TileMesh, u64)>,
    /// How many times the terrain was replaced: older tiles are drawn until made again.
    generation: u64,
    frame: u64,
    /// The tiles to draw this frame: key and the corner (east, south) of each.
    pub selected: Vec<((i32, i32), (f64, f64))>,
    pub pending: usize,
}

impl Footprints {
    pub fn new(terrain: Arc<Terrain>, range: f64) -> Footprints {
        Footprints { terrain, range, tiles: HashMap::new(), jobs: Jobs::new(), generation: 0, frame: 0, selected: Vec::new(), pending: 0 }
    }

    /// Another terrain of the same place (a region round another middle): the tiles stay and are
    /// made again, the nearest first, each drawn until its new one is there.
    pub fn replace(&mut self, terrain: Arc<Terrain>) {
        self.terrain = terrain;
        self.generation += 1;
    }

    /// Chooses the tiles to draw for a camera at (east, south), and starts making those missing or
    /// of the other detail. A terrain without buildings or bridges has none.
    pub fn select(&mut self, gpu: &Gpu, cx: f64, cz: f64) {
        self.frame += 1;
        for ((x, z, level), ((vertices, indices), generation)) in self.jobs.collect() {
            let mesh = (!indices.is_empty()).then(|| Mesh {
                vertices: gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("buildings of the maps"), contents: bytemuck::cast_slice(&vertices), usage: wgpu::BufferUsages::VERTEX }),
                indices: gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("buildings of the maps"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX }),
                count: indices.len() as u32,
            });
            self.tiles.insert((x, z), Tile { mesh, level, last_used: self.frame, generation });
        }
        self.selected.clear();
        self.pending = self.jobs.busy();
        if self.terrain.tallest_building() <= 0.0 {
            return;
        }
        let (x0, x1) = (((cx - self.range) / TILE).floor() as i32, ((cx + self.range) / TILE).floor() as i32);
        let (z0, z1) = (((cz - self.range) / TILE).floor() as i32, ((cz + self.range) / TILE).floor() as i32);
        let mut wanted = Vec::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                let d = ((x as f64 + 0.5) * TILE - cx).hypot((z as f64 + 0.5) * TILE - cz);
                if d > self.range + TILE {
                    continue;
                }
                let wanted_level = level(d);
                let tile = self.tiles.get_mut(&(x, z));
                let fits = tile.as_ref().is_some_and(|t| t.generation == self.generation && serves(t.level, d));
                if let Some(t) = tile {
                    t.last_used = self.frame;
                    if t.mesh.is_some() {
                        self.selected.push(((x, z), (x as f64 * TILE, z as f64 * TILE)));
                    }
                }
                if !fits && !self.jobs.contains(&(x, z, wanted_level)) {
                    wanted.push(((x, z, wanted_level), d));
                }
            }
        }
        wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
        self.pending += wanted.len();
        for (key, _) in wanted {
            if self.jobs.busy() >= IN_FLIGHT {
                break;
            }
            let (terrain, generation) = (self.terrain.clone(), self.generation);
            self.jobs.start(key, move || (tile_mesh(&terrain, (key.0, key.1), key.2), generation));
        }
        let frame = self.frame;
        self.tiles.retain(|_, t| frame - t.last_used <= 300);
    }

    pub fn mesh(&self, key: (i32, i32)) -> Option<&Mesh> {
        self.tiles.get(&key).and_then(|t| t.mesh.as_ref())
    }
}

/// Whether a building can be told apart from a far tile: tall, or big on the ground.
fn seen_far(b: &Footprint) -> bool {
    b.height >= 12.0 || b.area() >= 500.0
}

/// Where a place is, for the look of its buildings: the materials and colours of its towns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    /// Plaster in warm pale colours, terracotta roofs: the Mediterranean, Latin America.
    Warm,
    /// Brick and render, slate and red tiles: northern and central Europe.
    North,
    /// Brick blocks, painted siding on houses, shingle roofs: North America.
    America,
}

impl Palette {
    /// The palette of a place at a latitude and longitude (degrees).
    pub fn at(latitude: f64, longitude: f64) -> Palette {
        if (-170.0..-50.0).contains(&longitude) && latitude > 25.0 {
            Palette::America
        } else if latitude > 45.5 || (latitude > 43.0 && longitude > 5.0 && longitude < 30.0) {
            Palette::North
        } else {
            Palette::Warm
        }
    }
}

/// How a building looks: its facade and colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub style: f32,
    pub wall: [f32; 3],
    pub roof: [f32; 3],
    /// A storey's height, metres.
    pub storey: f32,
    /// The height of a ground floor of shops, metres; 0 when it has none.
    pub ground: f32,
    /// A flat roof the colour of tiles (an old block of a warm town): nothing stands on it.
    pub tiled: bool,
}

/// An sRGB colour of the maps as the linear albedo the shader lights, kept within what paint and
/// stone reflect (a "black" glass tower is dark glass, not a hole).
fn albedo(c: [u8; 3]) -> [f32; 3] {
    let lin = |v: u8| (v as f32 / 255.0).powf(2.2);
    let rgb = [lin(c[0]), lin(c[1]), lin(c[2])];
    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    let scale = if luma < 0.04 { 0.04 / luma.max(1e-4) } else if luma > 0.7 { 0.7 / luma } else { 1.0 };
    let lifted = rgb.map(|v| (v * scale).min(0.9));
    if luma < 0.004 { [0.04, 0.045, 0.05] } else { lifted }
}

impl Look {
    pub fn of(b: &Footprint, seed: f32, palette: Palette) -> Look {
        let pick = |list: &[[f32; 3]], k: f32| list[((k.fract() * list.len() as f32) as usize).min(list.len() - 1)];
        let k = |n: f32| (seed * n).fract();
        let tall = b.height;
        let area = b.area() as f32;
        // The facade: what the maps say it is made of, else what it is for.
        let residential = |k: f32| match palette {
            Palette::Warm => if k < 0.64 { STYLE_PLASTER } else { STYLE_BRICK },
            Palette::North => if k < 0.55 { STYLE_BRICK } else { STYLE_PLASTER },
            Palette::America => if tall > 60.0 && k < 0.4 { STYLE_GLASS } else if k < 0.6 { STYLE_BRICK } else if k < 0.85 { STYLE_PLASTER } else { STYLE_RIBBON },
        };
        let style = match (b.material, b.kind) {
            (Material::Glass, _) => STYLE_GLASS,
            (Material::Brick, _) => STYLE_BRICK,
            (Material::Stone, Use::Religious) => STYLE_CHURCH,
            (Material::Stone, _) => STYLE_STONE,
            (Material::Metal, Use::Industrial | Use::Outbuilding) => STYLE_INDUSTRIAL,
            (Material::Metal, _) => STYLE_GLASS,
            (Material::Wood, _) => STYLE_HOUSE,
            (Material::Concrete, Use::Office | Use::Civic) => STYLE_RIBBON,
            (_, Use::Industrial) => STYLE_INDUSTRIAL,
            (_, Use::Outbuilding) => if area > 300.0 { STYLE_INDUSTRIAL } else { STYLE_PLAIN },
            (_, Use::Religious) => STYLE_CHURCH,
            (_, Use::Civic) => if k(3.1) < 0.6 { STYLE_STONE } else { STYLE_RIBBON },
            (_, Use::House) => STYLE_HOUSE,
            (_, Use::Office) => if tall > 45.0 && k(5.3) < 0.75 { STYLE_GLASS } else if k(5.3) < 0.55 { STYLE_RIBBON } else if k(5.3) < 0.8 { STYLE_GLASS } else { STYLE_STONE },
            (_, Use::Retail) => if area > 2500.0 { STYLE_INDUSTRIAL } else { residential(k(7.7)) },
            (Material::Plaster, _) => STYLE_PLASTER,
            (_, _) if tall > 70.0 => if k(5.3) < 0.7 { STYLE_GLASS } else { STYLE_RIBBON },
            (_, _) if tall < 9.0 && area < 220.0 => STYLE_HOUSE,
            (_, _) if tall < 7.0 && area > 1500.0 => STYLE_INDUSTRIAL,
            _ => residential(k(7.7)),
        };
        const PLASTER_WARM: [[f32; 3]; 7] = [[0.62, 0.55, 0.42], [0.58, 0.43, 0.27], [0.66, 0.64, 0.59], [0.60, 0.44, 0.35], [0.64, 0.56, 0.38], [0.50, 0.48, 0.45], [0.57, 0.50, 0.42]];
        const PLASTER_COOL: [[f32; 3]; 5] = [[0.62, 0.61, 0.57], [0.55, 0.52, 0.46], [0.48, 0.48, 0.47], [0.60, 0.56, 0.47], [0.45, 0.47, 0.50]];
        const BRICK: [[f32; 3]; 5] = [[0.36, 0.17, 0.11], [0.45, 0.24, 0.14], [0.27, 0.15, 0.11], [0.52, 0.42, 0.29], [0.40, 0.28, 0.21]];
        const STONE: [[f32; 3]; 4] = [[0.60, 0.55, 0.46], [0.47, 0.46, 0.43], [0.62, 0.58, 0.50], [0.52, 0.47, 0.39]];
        const GLASS: [[f32; 3]; 5] = [[0.10, 0.14, 0.17], [0.06, 0.07, 0.08], [0.22, 0.26, 0.29], [0.09, 0.13, 0.12], [0.16, 0.15, 0.13]];
        const RIBBON: [[f32; 3]; 4] = [[0.52, 0.51, 0.48], [0.62, 0.62, 0.60], [0.44, 0.43, 0.40], [0.55, 0.50, 0.42]];
        const SHEET: [[f32; 3]; 5] = [[0.55, 0.56, 0.56], [0.30, 0.36, 0.42], [0.55, 0.50, 0.40], [0.42, 0.44, 0.40], [0.62, 0.62, 0.60]];
        const SIDING: [[f32; 3]; 5] = [[0.66, 0.65, 0.61], [0.58, 0.54, 0.44], [0.40, 0.45, 0.50], [0.50, 0.52, 0.46], [0.60, 0.50, 0.40]];
        let wall = match b.wall_colour {
            Some(c) => albedo(c),
            None => match style as i32 {
                1 => pick(&BRICK, k(11.0)),
                2 | 7 => pick(&STONE, k(11.0)),
                3 => pick(&GLASS, k(11.0)),
                4 => pick(&RIBBON, k(11.0)),
                5 => pick(&SHEET, k(11.0)),
                6 if palette == Palette::America => pick(&SIDING, k(11.0)),
                6 if palette == Palette::North && k(13.0) < 0.5 => pick(&BRICK, k(11.0)),
                _ if palette == Palette::Warm => pick(&PLASTER_WARM, k(11.0)),
                _ => pick(&PLASTER_COOL, k(11.0)),
            },
        };
        const FLAT: [[f32; 3]; 5] = [[0.22, 0.21, 0.20], [0.30, 0.29, 0.27], [0.16, 0.16, 0.17], [0.38, 0.38, 0.36], [0.26, 0.25, 0.22]];
        const TERRACOTTA: [[f32; 3]; 4] = [[0.45, 0.20, 0.12], [0.52, 0.26, 0.16], [0.40, 0.19, 0.13], [0.48, 0.30, 0.20]];
        const NORTH_ROOFS: [[f32; 3]; 4] = [[0.15, 0.16, 0.18], [0.40, 0.16, 0.10], [0.20, 0.20, 0.21], [0.33, 0.15, 0.11]];
        const SHINGLES: [[f32; 3]; 4] = [[0.22, 0.22, 0.23], [0.25, 0.18, 0.13], [0.30, 0.29, 0.27], [0.18, 0.19, 0.21]];
        const DOMES: [[f32; 3]; 3] = [[0.25, 0.42, 0.36], [0.30, 0.30, 0.31], [0.55, 0.42, 0.18]];
        // The old blocks of a warm town are roofed with tiles: seen from the air their flat tops
        // are the colour of them.
        let tiled = palette == Palette::Warm && b.height < 30.0 && matches!(b.kind, Use::Residential | Use::Unknown | Use::Religious | Use::Civic) && k(23.0) < 0.6;
        let roof = match b.roof_colour {
            Some(c) => albedo(c),
            None => match b.roof {
                Roof::Flat if tiled => pick(&TERRACOTTA, k(17.0)).map(|c| c * 0.72),
                Roof::Flat => pick(&FLAT, k(17.0)),
                Roof::Dome | Roof::Onion => pick(&DOMES, k(17.0)),
                _ => match palette {
                    Palette::Warm => pick(&TERRACOTTA, k(17.0)),
                    Palette::North => pick(&NORTH_ROOFS, k(17.0)),
                    Palette::America => pick(&SHINGLES, k(17.0)),
                },
            },
        };
        let default_storey = match style as i32 {
            3 | 4 => 3.8,
            2 | 7 => 4.4,
            5 => 6.0,
            6 => 2.9,
            _ => 3.1,
        };
        let walls_high = (b.height - b.floor).max(1.0);
        let storey = if b.levels > 0 { (walls_high / b.levels as f32).clamp(2.6, 6.5) } else { default_storey };
        // City blocks and towers have shops or a lobby on the ground floor.
        let ground = if b.floor == 0.0 && b.height > 10.0 && matches!(style as i32, 0 | 1 | 2 | 4) && k(19.0) < 0.7 { 4.2 } else if style == STYLE_GLASS && b.height > 30.0 { 6.0 } else { 0.0 };
        Look { style, wall, roof, storey, ground, tiled: tiled && b.roof == Roof::Flat && b.roof_colour.is_none() }
    }
}

/// The mesh of the buildings whose middle is in tile `key` (east, south) at a `level` of detail (0:
/// all of them with their parapets and what stands on their roofs; 1: all of them, plain; 2: those
/// seen from far off), and the parts of bridges whose middle is in it.
pub fn tile_mesh(terrain: &Terrain, key: (i32, i32), level: u8) -> TileMesh {
    let far = level >= 2;
    let (ox, oz) = (key.0 as f64 * TILE, key.1 as f64 * TILE);
    let mut mesh = MeshData { origin: (ox, oz), ..MeshData::default() };
    let palette = terrain.region().map_or(Palette::Warm, |r| Palette::at(r.latitude.to_degrees(), r.longitude.to_degrees()));
    for b in terrain.buildings(-(oz + TILE), -oz, ox, ox + TILE) {
        if b.ring.len() < 3 || (far && !seen_far(b)) {
            continue;
        }
        let base = mapped_base(terrain, b) as f32;
        if level == 0 {
            building(&mut mesh, b, base, palette, false);
        } else {
            // Farther, the outline with fewer corners: what is within this much of a straight
            // side cannot be told from it (and the walls are most of the triangles).
            let tolerance = if level == 1 { 0.6 } else { 1.8 };
            let simpler = Footprint { ring: simplify(&b.ring, tolerance), holes: b.holes.iter().map(|h| simplify(h, tolerance)).filter(|h| h.len() >= 3).collect(), ..b.clone() };
            if simpler.ring.len() >= 3 {
                building(&mut mesh, &simpler, base, palette, true);
            }
        }
    }
    crate::bridges::tile(&mut mesh, terrain, key, TILE, far);
    (mesh.vertices, mesh.indices)
}

/// A building into a mesh, standing on `base` above the sea; `plain`, without its parapet and what
/// stands on its roof.
fn building(mesh: &mut MeshData, b: &Footprint, base: f32, palette: Palette, plain: bool) {
    let (cn, ce) = b.centre();
    let seed = hash2(cn.round() as i64, ce.round() as i64, 77) as f32;
    let look = Look::of(b, seed, palette);
    let frame = Frame::of(b);
    let bottom = if b.floor > 0.0 { base + b.floor } else { base - SINK };
    let foot = bottom - base;
    // The rings with a corner where they cross a bend of the roof, so the walls meet its faces.
    let bends = frame.bends();
    let outer = with_bends(&b.ring, &frame, &bends, true);
    let holes: Vec<Vec<[f64; 2]>> = b.holes.iter().map(|h| with_bends(h, &frame, &bends, false)).collect();
    let top_at = |p: [f64; 2]| base + frame.height(&b.ring, p[0], p[1]) as f32;
    // A flat roof of a block has a parapet round it, the walls going on up to its top; the bigger
    // ones have their stairs' housing and machines on them.
    let parapet = frame.shape == Shape::Flat && !plain && b.height - b.floor > 7.0 && b.area() > 90.0;
    let wall_top = |p: [f64; 2]| match frame.shape {
        Shape::Flat if parapet => base + b.height + PARAPET,
        Shape::Flat => base + b.height,
        Shape::Planes { .. } | Shape::Dome { .. } => base + frame.eaves as f32,
        _ => top_at(p),
    };
    for ring in std::iter::once(&outer).chain(holes.iter()) {
        mesh.walls(ring, bottom, foot, base, &wall_top, &look, seed, SURFACE_WALL, look.style);
    }
    let roof_surface = if frame.shape == Shape::Flat { SURFACE_FLAT } else { SURFACE_PITCHED };
    match frame.shape {
        Shape::Flat | Shape::Dome { .. } => {
            let top = if frame.shape == Shape::Flat { base + b.height } else { base + frame.eaves as f32 };
            for t in triangulate_with_holes(&outer, &holes) {
                mesh.roof_triangle(t.map(|p| (p, top)), base, look.roof, SURFACE_FLAT, seed, &look);
            }
            if let Shape::Dome { onion } = frame.shape {
                mesh.dome(&frame, base, onion, look.roof, seed, &look);
            } else if parapet {
                // The parapet is the walls going on up past the roof: seen from above, their inner
                // side is its inner face.
                if b.area() > 250.0 && b.height > 11.0 && !look.tiled {
                    mesh.rooftop(b, top, base, &look, seed);
                }
            }
        }
        Shape::Planes { .. } => {
            for face in plane_faces(&b.ring, &frame) {
                for i in 1..face.len().saturating_sub(1) {
                    let t = [face[0], face[i], face[i + 1]];
                    mesh.roof_triangle(t.map(|p| (p, top_at(p))), base, look.roof, roof_surface, seed, &look);
                }
            }
        }
        _ => {
            for t in triangulate_with_holes(&outer, &holes) {
                for piece in split_by_bends(&t, &frame, &bends) {
                    for i in 1..piece.len().saturating_sub(1) {
                        let t = [piece[0], piece[i], piece[i + 1]];
                        mesh.roof_triangle(t.map(|p| (p, top_at(p))), base, look.roof, roof_surface, seed, &look);
                    }
                }
            }
        }
    }
}

/// A ring with the corners within `tolerance` metres of the straight line between their neighbours
/// left out (Douglas and Peucker, from its two farthest corners).
fn simplify(ring: &[[f32; 2]], tolerance: f32) -> Vec<[f32; 2]> {
    let k = ring.len();
    if k <= 4 {
        return ring.to_vec();
    }
    fn keep(points: &[[f32; 2]], from: usize, to: usize, tolerance: f32, kept: &mut [bool]) {
        let (a, b) = (points[from], points[to]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dy).max(1e-6);
        let (mut far, mut at) = (0.0f32, from);
        for (i, p) in points.iter().enumerate().take(to).skip(from + 1) {
            let d = ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / length;
            if d > far {
                (far, at) = (d, i);
            }
        }
        if far > tolerance {
            kept[at] = true;
            keep(points, from, at, tolerance, kept);
            keep(points, at, to, tolerance, kept);
        }
    }
    // Split at the corner farthest from the first, and simplify each half.
    let far = (1..k).max_by(|&i, &j| {
        let d = |p: [f32; 2]| (p[0] - ring[0][0]).hypot(p[1] - ring[0][1]);
        d(ring[i]).total_cmp(&d(ring[j]))
    }).unwrap_or(k / 2);
    let mut points = ring.to_vec();
    points.push(ring[0]);
    let mut kept = vec![false; k + 1];
    kept[0] = true;
    kept[far] = true;
    keep(&points, 0, far, tolerance, &mut kept);
    keep(&points, far, k, tolerance, &mut kept);
    (0..k).filter(|&i| kept[i]).map(|i| ring[i]).collect()
}

/// A ring (north, east) as `f64`, turning anticlockwise seen from above when `outer` (clockwise
/// when not: a hole), with a corner added wherever a side crosses one of the roof's bends.
fn with_bends(ring: &[[f32; 2]], frame: &Frame, bends: &[f64], outer: bool) -> Vec<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = ring.iter().map(|p| [p[0] as f64, p[1] as f64]).collect();
    // Anticlockwise seen from above with north up and east right: positive area in (east, north).
    let area: f64 = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a[1] * b[0] - b[1] * a[0]).sum();
    if (area > 0.0) != outer {
        points.reverse();
    }
    if bends.is_empty() {
        return points;
    }
    let mut out = Vec::with_capacity(points.len() + 4);
    for i in 0..points.len() {
        let (a, c) = (points[i], points[(i + 1) % points.len()]);
        out.push(a);
        let (va, vc) = (frame.local(a[0], a[1]).1, frame.local(c[0], c[1]).1);
        let mut crossings: Vec<f64> = bends.iter().filter(|&&k| (va - k) * (vc - k) < 0.0).map(|&k| (k - va) / (vc - va)).collect();
        crossings.sort_by(f64::total_cmp);
        for t in crossings {
            out.push([a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t]);
        }
    }
    out
}

/// The pieces of a triangle between the roof's bends: each flat, as convex polygons.
fn split_by_bends(t: &[[f64; 2]; 3], frame: &Frame, bends: &[f64]) -> Vec<Vec<[f64; 2]>> {
    let mut pieces = vec![t.to_vec()];
    for &k in bends {
        let mut next = Vec::new();
        let v = |p: [f64; 2]| frame.local(p[0], p[1]).1 - k;
        for piece in pieces {
            let below = clip(&piece, |p| v(p));
            let above = clip(&piece, |p| -v(p));
            next.extend([below, above].into_iter().filter(|p| p.len() >= 3));
        }
        pieces = next;
    }
    pieces
}

/// The part of a convex polygon where a linear function is at most 0 (Sutherland and Hodgman).
fn clip(poly: &[[f64; 2]], f: impl Fn([f64; 2]) -> f64) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (a, c) = (poly[i], poly[(i + 1) % poly.len()]);
        let (fa, fc) = (f(a), f(c));
        if fa <= 0.0 {
            out.push(a);
        }
        if (fa < 0.0) != (fc < 0.0) && fa != fc && !(fa == 0.0 || fc == 0.0) {
            let t = fa / (fa - fc);
            out.push([a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t]);
        }
    }
    out
}

/// The faces of a roof of planes over a convex outline (hipped, pyramidal, mansard): for each
/// side, where its plane is the lowest and under the rise; and the flat top where all are above
/// it. Each a convex polygon.
fn plane_faces(ring: &[[f32; 2]], frame: &Frame) -> Vec<Vec<[f64; 2]>> {
    let poly = with_bends(ring, frame, &[], true);
    let k = poly.len();
    // Each side's plane: its height over the eaves, as a share of the rise, is the distance to the
    // side's line over its reach, `(n·p - d) / reach`, n pointing inwards.
    let planes: Vec<([f64; 2], f64, f64)> = (0..k)
        .map(|i| {
            let (a, c) = (poly[i], poly[(i + 1) % k]);
            let (dn, de) = (c[0] - a[0], c[1] - a[1]);
            let length = dn.hypot(de).max(1e-9);
            // Anticlockwise in (east, north): the inside is to the left, (-de, dn) in (north, east)... as (n, e): left of (dn, de) is (de, -dn) turned; take the one pointing at the middle.
            let mut normal = [-de / length, dn / length];
            let middle = poly.iter().fold([0.0, 0.0], |m, p| [m[0] + p[0] / k as f64, m[1] + p[1] / k as f64]);
            if (middle[0] - a[0]) * normal[0] + (middle[1] - a[1]) * normal[1] < 0.0 {
                normal = [-normal[0], -normal[1]];
            }
            let reach = match frame.shape {
                Shape::Planes { pyramid: true, .. } => ((frame.centre.0 - a[0]) * normal[0] + (frame.centre.1 - a[1]) * normal[1]).max(0.5),
                Shape::Planes { mansard: true, .. } => frame.half_across.min(frame.half_along) * 0.3,
                _ => frame.half_across,
            };
            (normal, normal[0] * a[0] + normal[1] * a[1], reach)
        })
        .collect();
    let share = |i: usize, p: [f64; 2]| (planes[i].0[0] * p[0] + planes[i].0[1] * p[1] - planes[i].1) / planes[i].2;
    let mut faces = Vec::new();
    for i in 0..k {
        let mut face = poly.clone();
        for j in (0..k).filter(|&j| j != i) {
            face = clip(&face, |p| share(i, p) - share(j, p));
            if face.len() < 3 {
                break;
            }
        }
        face = clip(&face, |p| share(i, p) - 1.0);
        if face.len() >= 3 {
            faces.push(face);
        }
    }
    let mut top = poly.clone();
    for i in 0..k {
        top = clip(&top, |p| 1.0 - share(i, p));
    }
    if top.len() >= 3 {
        faces.push(top);
    }
    faces
}

/// Triangles covering an outline with holes, as corners (north, east): the holes are joined to the
/// outline by a cut from each hole's most eastern corner to the nearest corner of the outline it
/// can see, and the one ring that makes is ear-clipped.
fn triangulate_with_holes(outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>]) -> Vec<[[f64; 2]; 3]> {
    let mut ring: Vec<[f64; 2]> = outer.to_vec();
    // The holes turn the other way round from the outline, as the cut walks round them.
    let turning = signed_area(outer).signum();
    let holes: Vec<Vec<[f64; 2]>> = holes.iter().filter(|h| h.len() >= 3).map(|h| if signed_area(h).signum() == turning { h.iter().rev().copied().collect() } else { h.clone() }).collect();
    let mut order: Vec<&Vec<[f64; 2]>> = holes.iter().collect();
    order.sort_by(|a, b| b.iter().map(|p| p[1]).fold(f64::MIN, f64::max).total_cmp(&a.iter().map(|p| p[1]).fold(f64::MIN, f64::max)));
    for hole in order {
        let (hi, &hp) = hole.iter().enumerate().max_by(|a, b| a.1[1].total_cmp(&b.1[1])).unwrap_or((0, &hole[0]));
        let edges_cross = |a: [f64; 2], b: [f64; 2], ring: &[[f64; 2]]| {
            (0..ring.len()).any(|i| {
                let (c, d) = (ring[i], ring[(i + 1) % ring.len()]);
                if c == a || d == a || c == b || d == b {
                    return false;
                }
                segments_cross(a, b, c, d)
            })
        };
        let mut best: Option<(f64, usize)> = None;
        for (i, &p) in ring.iter().enumerate() {
            let d = (p[0] - hp[0]).hypot(p[1] - hp[1]);
            if best.is_some_and(|(bd, _)| bd <= d) {
                continue;
            }
            if !edges_cross(hp, p, &ring) && !edges_cross(hp, p, hole) {
                best = Some((d, i));
            }
        }
        let Some((_, at)) = best else { continue };
        let mut spliced = Vec::with_capacity(ring.len() + hole.len() + 2);
        spliced.extend_from_slice(&ring[..=at]);
        for k in 0..=hole.len() {
            spliced.push(hole[(hi + k) % hole.len()]);
        }
        spliced.push(ring[at]);
        spliced.extend_from_slice(&ring[at + 1..]);
        ring = spliced;
    }
    triangulate(&ring).into_iter().map(|[a, b, c]| [ring[a], ring[b], ring[c]]).collect()
}

fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0]);
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0) && d1 != 0.0 && d2 != 0.0 && d3 != 0.0 && d4 != 0.0
}

#[derive(Default)]
pub(crate) struct MeshData {
    pub(crate) vertices: Vec<f32>,
    pub(crate) indices: Vec<u32>,
    /// The tile's corner (east, south): positions are relative to it.
    pub(crate) origin: (f64, f64),
}

impl MeshData {
    /// A point (north, east, height above the sea) in the tile's axes (east, up, south).
    pub(crate) fn at(&self, p: [f64; 2], h: f32) -> [f32; 3] {
        [(p[1] - self.origin.0) as f32, h, (-p[0] - self.origin.1) as f32]
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn vertex(&mut self, p: [f32; 3], n: [f32; 3], colour: [f32; 3], surface: f32, above_foot: f32, seed: f32, facade: [f32; 4]) -> u32 {
        let i = (self.vertices.len() / FLOATS) as u32;
        self.vertices.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2], colour[0], colour[1], colour[2], surface, above_foot, seed]);
        self.vertices.extend_from_slice(&facade);
        i
    }

    /// The walls round a ring (north, east; anticlockwise for an outline, clockwise for a hole, so
    /// that they face out of the building), from `bottom` to `top(corner)` above the sea. Each side
    /// has a whole number of columns of windows.
    #[allow(clippy::too_many_arguments)]
    fn walls(&mut self, ring: &[[f64; 2]], bottom: f32, foot: f32, base: f32, top: &dyn Fn([f64; 2]) -> f32, look: &Look, seed: f32, surface: f32, style: f32) {
        let width = cell(style);
        for i in 0..ring.len() {
            let (a, c) = (ring[i], ring[(i + 1) % ring.len()]);
            let (pa, pc) = (self.at(a, 0.0), self.at(c, 0.0));
            let (dx, dz) = (pc[0] - pa[0], pc[2] - pa[2]);
            let length = dx.hypot(dz);
            if length < 0.05 {
                continue;
            }
            let (ta, tc) = (top(a), top(c));
            if ta <= bottom && tc <= bottom {
                continue;
            }
            // Out of the building: anticlockwise seen from above, the outside is to the right of
            // the way the side runs (x east, z south).
            let n = [-dz / length, 0.0, dx / length];
            let columns = (length / width).round().max(1.0);
            let facade = |u: f32| [style, look.storey, u, look.ground];
            let v = [
                self.vertex([pa[0], bottom, pa[2]], n, look.wall, surface, foot, seed, facade(0.0)),
                self.vertex([pc[0], bottom, pc[2]], n, look.wall, surface, foot, seed, facade(columns)),
                self.vertex([pc[0], tc, pc[2]], n, look.wall, surface, tc - base, seed, facade(columns)),
                self.vertex([pa[0], ta, pa[2]], n, look.wall, surface, ta - base, seed, facade(0.0)),
            ];
            self.indices.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
        }
    }

    /// A triangle of roof, corners (north, east) with their heights above the sea, facing up.
    fn roof_triangle(&mut self, t: [([f64; 2], f32); 3], base: f32, colour: [f32; 3], surface: f32, seed: f32, look: &Look) {
        let p = t.map(|(q, h)| self.at(q, h));
        let n = normal(p[0], p[1], p[2]);
        if n[1] < 0.02 {
            // Edge on, or a sliver from the clipping.
            return;
        }
        let facade = [STYLE_PLAIN, look.storey, 0.0, 0.0];
        let v: Vec<u32> = p.iter().map(|&q| self.vertex(q, n, colour, surface, q[1] - base, seed, facade)).collect();
        // Anticlockwise seen from above (from the normal's side), as the walls.
        let up = (p[1][0] - p[0][0]) * (p[2][2] - p[0][2]) - (p[1][2] - p[0][2]) * (p[2][0] - p[0][0]);
        if up < 0.0 {
            self.indices.extend_from_slice(&[v[0], v[1], v[2]]);
        } else {
            self.indices.extend_from_slice(&[v[0], v[2], v[1]]);
        }
    }

    /// What stands on a big flat roof: the housing of the stairs and lift, and boxes of air
    /// conditioning, where they fit inside the outline.
    fn rooftop(&mut self, b: &Footprint, top: f32, base: f32, look: &Look, seed: f32) {
        let ((cn, ce), heading, half_along, half_across) = b.oriented_box();
        let (c, s) = (heading.cos(), heading.sin());
        let count = 1 + ((b.area() / 600.0) as usize).min(4);
        for k in 0..count {
            let h = |n: u64| hash2(cn.round() as i64 + k as i64 * 31, ce.round() as i64, 300 + n);
            let housing = k == 0;
            let (along, across, up) = if housing { (4.0 + 3.0 * h(1), 3.5 + 2.0 * h(2), 3.0 + 0.6 * h(3)) } else { (1.4 + 1.6 * h(1), 1.0 + 1.2 * h(2), 1.0 + 0.8 * h(3)) };
            let u = (h(4) * 2.0 - 1.0) * (half_along - along).max(0.0) * 0.8;
            let v = (h(5) * 2.0 - 1.0) * (half_across - across).max(0.0) * 0.8;
            let centre = [cn + u * c - v * s, ce + u * s + v * c];
            let corners: Vec<[f64; 2]> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .iter()
                .map(|&(a, b): &(f64, f64)| [centre[0] + a * along / 2.0 * c - b * across / 2.0 * s, centre[1] + a * along / 2.0 * s + b * across / 2.0 * c])
                .collect();
            if !corners.iter().all(|p| b.contains(p[0], p[1])) {
                continue;
            }
            let ring = with_bends(&corners.iter().map(|p| [p[0] as f32, p[1] as f32]).collect::<Vec<_>>(), &Frame::of(b), &[], true);
            let (colour, surface) = if housing { (look.wall.map(|x| x * 0.85), SURFACE_PLAIN) } else { ([0.55, 0.56, 0.57], SURFACE_STEEL) };
            let box_look = Look { wall: colour, ..*look };
            let height = top + up as f32;
            self.walls(&ring, top - 0.1, top - 0.1 - base, base, &|_| height, &box_look, seed, surface, STYLE_PLAIN);
            for t in [[ring[0], ring[1], ring[2]], [ring[0], ring[2], ring[3]]] {
                self.roof_triangle(t.map(|p| (p, height)), base, colour.map(|x| x * 0.9), surface, seed, look);
            }
        }
    }

    /// A dome or an onion over the middle of a roof, round, at the eaves.
    fn dome(&mut self, frame: &Frame, base: f32, onion: bool, colour: [f32; 3], seed: f32, look: &Look) {
        const AROUND: usize = 20;
        const UP: usize = 10;
        let r = frame.half_across;
        let eaves = base + frame.eaves as f32;
        let point = |i: usize, j: usize| -> ([f64; 2], f32, [f32; 3]) {
            let a = std::f64::consts::TAU * i as f64 / AROUND as f64;
            let s = j as f64 / UP as f64;
            let (radius, height) = if onion { (sim_core::roofs::onion_radius(s), s) } else { ((s * std::f64::consts::FRAC_PI_2).cos(), (s * std::f64::consts::FRAC_PI_2).sin()) };
            let p = [frame.centre.0 + r * radius * a.cos(), frame.centre.1 + r * radius * a.sin()];
            // The normal of a sphere, good enough for an onion.
            let up = if onion { (s * 1.3).min(1.0) } else { height };
            let side = (1.0 - up * up).max(0.0).sqrt();
            (p, eaves + (frame.rise * height) as f32, [(a.sin() * side) as f32, up as f32, (-a.cos() * side) as f32])
        };
        let first = (self.vertices.len() / FLOATS) as u32;
        for j in 0..=UP {
            for i in 0..AROUND {
                let (p, h, n) = point(i, j);
                let q = self.at(p, h);
                self.vertex(q, n, colour, SURFACE_PITCHED, h - base, seed, [STYLE_PLAIN, look.storey, 0.0, 0.0]);
            }
        }
        for j in 0..UP as u32 {
            for i in 0..AROUND as u32 {
                let k = |i: u32, j: u32| first + j * AROUND as u32 + i % AROUND as u32;
                let (a, b, c, d) = (k(i, j), k(i + 1, j), k(i + 1, j + 1), k(i, j + 1));
                self.indices.extend_from_slice(&[a, c, b, a, d, c]);
            }
        }
    }
}

/// The unit normal of a sloping triangle, pointing up whichever way its corners turn.
pub(crate) fn normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
    let sign = if n[1] < -1e-4 { -1.0 } else { 1.0 };
    [n[0] * sign / length, n[1] * sign / length, n[2] * sign / length]
}

/// Twice the area of a ring, positive when it turns one way.
fn signed_area(ring: &[[f64; 2]]) -> f64 {
    ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum()
}

/// Triangles covering a simple polygon (ear clipping), as indices into its corners.
fn triangulate(ring: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let mut left: Vec<usize> = (0..ring.len()).collect();
    let mut out = Vec::with_capacity(ring.len().saturating_sub(2));
    let orientation = signed_area(ring).signum();
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut guard = 0;
    while left.len() > 3 && guard < ring.len() * ring.len() {
        guard += 1;
        let n = left.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (ring[left[(i + n - 1) % n]], ring[left[i]], ring[left[(i + 1) % n]]);
            if cross(a, b, c) * orientation <= 0.0 {
                return false;
            }
            // No other corner inside it.
            left.iter().all(|&k| {
                let p = ring[k];
                p == a || p == b || p == c || !(cross(a, b, p) * orientation > 0.0 && cross(b, c, p) * orientation > 0.0 && cross(c, a, p) * orientation > 0.0)
            })
        });
        // A ring that crosses itself has no ear: the rest is fanned.
        let i = ear.unwrap_or(1 % n);
        out.push([left[(i + n - 1) % n], left[i], left[(i + 1) % n]]);
        left.remove(i);
    }
    if left.len() == 3 {
        out.push([left[0], left[1], left[2]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(t: &[[f64; 2]; 3]) -> f64 {
        signed_area(t).abs() / 2.0
    }

    #[test]
    fn a_concave_outline_with_a_courtyard_is_covered_once() {
        // An L: 3 x 1 and 1 x 2, area 5.
        let ring = [[0.0, 0.0], [3.0, 0.0], [3.0, 1.0], [1.0, 1.0], [1.0, 3.0], [0.0, 3.0]];
        let triangles = triangulate(&ring);
        assert_eq!(triangles.len(), 4);
        let total: f64 = triangles.iter().map(|t| area(&[ring[t[0]], ring[t[1]], ring[t[2]]])).sum();
        assert!((total - 5.0).abs() < 1e-9, "{total}");
        // A block of 60 x 60 with a courtyard of 30 x 30.
        let outer = vec![[0.0, 0.0], [0.0, 60.0], [60.0, 60.0], [60.0, 0.0]];
        let hole = vec![[15.0, 15.0], [15.0, 45.0], [45.0, 45.0], [45.0, 15.0]];
        let triangles = triangulate_with_holes(&outer, &[hole]);
        let total: f64 = triangles.iter().map(area).sum();
        assert!((total - (3600.0 - 900.0)).abs() < 1e-6, "{total}");
        // None of them over the courtyard.
        for t in &triangles {
            let m = [(t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0];
            assert!(!(m[0] > 15.0 && m[0] < 45.0 && m[1] > 15.0 && m[1] < 45.0), "{t:?}");
        }
    }

    fn mesh_of(b: &Footprint) -> MeshData {
        let mut mesh = MeshData::default();
        building(&mut mesh, b, 0.0, Palette::Warm, false);
        mesh
    }

    #[test]
    fn walls_face_out_and_roofs_up_and_meet_the_roof_the_simulation_hits() {
        for roof in [Roof::Flat, Roof::Gabled, Roof::Hipped, Roof::Pyramidal, Roof::Skillion, Roof::Round, Roof::Gambrel, Roof::Mansard, Roof::Dome] {
            let b = Footprint::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 12.0], [0.0, 12.0]], 12.0, 0.0, roof);
            let mesh = mesh_of(&b);
            let frame = Frame::of(&b);
            let (cx, cz) = (6.0, -10.0);
            for v in mesh.vertices.as_chunks::<FLOATS>().0 {
                let (p, n) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
                if v[9] == SURFACE_WALL {
                    // Out from the middle of the building.
                    assert!((p[0] - cx) * n[0] + (p[2] - cz) * n[2] > 0.0, "{roof:?} {p:?} {n:?}");
                } else if v[9] == SURFACE_PITCHED || v[9] == SURFACE_FLAT {
                    assert!(n[1] >= 0.0, "{roof:?}");
                    // Where the simulation's roof is, but on the domes (round between their rings).
                    if roof != Roof::Dome {
                        let (north, east) = (-p[2] as f64, p[0] as f64);
                        let h = frame.height(&b.ring, north, east) as f32;
                        assert!((p[1] - h).abs() < 0.05, "{roof:?} at {north}, {east}: {} against {h}", p[1]);
                    }
                }
            }
            assert!(!mesh.indices.is_empty());
        }
    }

    #[test]
    fn a_roof_of_planes_covers_its_outline_once() {
        let b = Footprint::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 12.0], [0.0, 12.0]], 12.0, 0.0, Roof::Hipped);
        let frame = Frame::of(&b);
        let faces = plane_faces(&b.ring, &frame);
        let total: f64 = faces.iter().map(|f| signed_area(f).abs() / 2.0).sum();
        assert!((total - 240.0).abs() < 0.01, "{total}");
        assert_eq!(faces.len(), 4, "two slopes and two hips");
    }

    #[test]
    fn looks_follow_what_a_building_is() {
        let mut b = Footprint::new(vec![[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]], 150.0, 0.0, Roof::Flat);
        b.material = Material::Glass;
        assert_eq!(Look::of(&b, 0.3, Palette::Warm).style, STYLE_GLASS);
        b.material = Material::Unknown;
        b.kind = Use::Industrial;
        assert_eq!(Look::of(&b, 0.3, Palette::North).style, STYLE_INDUSTRIAL);
        b.kind = Use::Religious;
        assert_eq!(Look::of(&b, 0.3, Palette::North).style, STYLE_CHURCH);
        b.wall_colour = Some([255, 255, 255]);
        assert!(Look::of(&b, 0.3, Palette::North).wall[0] <= 0.7 + 1e-6);
        b.wall_colour = Some([0, 0, 0]);
        assert!(Look::of(&b, 0.3, Palette::North).wall[0] > 0.0);
        assert_eq!(Palette::at(40.48, -3.69), Palette::Warm);
        assert_eq!(Palette::at(40.75, -73.98), Palette::America);
        assert_eq!(Palette::at(52.37, 4.9), Palette::North);
    }
}
