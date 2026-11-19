//! Where the trees and the buildings stand. The terrain says where forest, farmland, rivers, towns
//! and airports are; which tree grows on which spot is decided here, from hashes of the spot and
//! the seed, so the same world always looks the same. The game draws them (as instances, in tiles,
//! axes east, up, south); the simulation runs into them (`obstacles`).
//!
//! Instances are 8 floats (the game's `pipelines::instance_layout`):
//! - a tree: x, y, z in the tile, height | tone, width factor, slope of the ground along x and z.
//!   `tone` is `floor(32 * brightness) + hue`, hue in 0..1 from yellowish to bluish green; its
//!   fraction also turns the tree and staggers the distance at which it changes detail.
//! - a building: x, y, z in the tile, scale | roof tint r, g, b, heading in radians (where the
//!   model's x axis points, from east towards south).

use crate::math::{clamp, fbm, hash2, lerp, smoothstep, DEG};
use crate::region::{Cover, Footprint};
use crate::terrain::{Airport, Ground, Road, Terrain, Town};

/// The instances of one tile, by kind (which tree, which building), 8 floats each. Positions are
/// relative to the tile's corner.
pub type Instances = Vec<Vec<f32>>;

/// Distance between the spots where a tree may grow, metres. Tiles must be a multiple of it.
const SPACING: f64 = 12.8;

/// The kinds of tree as the simulation knows them, in the order of the lists `trees` returns. Each is
/// the shape of a crown something may run into (`CROWNS`); the world draws each as one of a few
/// species of that shape (`world_render::species`).
#[derive(Clone, Copy, PartialEq)]
pub enum Tree {
    Oak,
    Ash,
    HolmOak,
    Poplar,
    Spruce,
    Fir,
    Pine,
    Bush,
}

/// Height and slopes of the ground with the airports flattened, and how close the river is (1 at the water).
fn ground(terrain: &Terrain, n: f64, e: f64, airports: &[Airport]) -> (f64, f64, f64, f64) {
    let raw = terrain.raw(n, e);
    let (mut h, mut river, mut flat) = (raw.h, raw.river, 0.0f64);
    for a in airports {
        let w = a.flat_weight(n, e);
        if w > 0.0 {
            h = lerp(h, a.elevation, w);
            river *= 1.0 - w;
            flat = flat.max(w);
        }
    }
    // A race circuit's ground, as `Terrain::ground_with` has it.
    for c in terrain.circuits() {
        if let Some((level, w)) = c.earthwork(n, e, h) {
            h = lerp(h, level, w);
            river *= 1.0 - w;
            flat = flat.max(w);
        }
    }
    (h, river, flat, raw.valley)
}

/// `ihash` of shaders.wgsl, bit for bit: what the ground shader decides from it (the fields of the
/// farmland) can be worked out here too.
fn ihash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// The nearest boundary between two fields to a point (east, south), as `terrain_fragment` lays the
/// farmland out: squares of 256 m cut into two to four strips. Returns how far the boundary is along
/// x and z (one of the two is 0: boundaries run along the axes) and a number of its own, 0..1.
fn field_boundary(x: f64, z: f64) -> ((f64, f64), f32) {
    let (wx, wz) = (x.rem_euclid(16384.0), z.rem_euclid(16384.0));
    let (cx, cz) = ((wx / 256.0).floor(), (wz / 256.0).floor());
    let (ix, iz) = (cx as i32 & 63, cz as i32 & 63);
    let along_x = ihash(ix, iz) > 0.5;
    let strips = 2.0 + (ihash(ix + 91, iz + 91) * 3.0).floor() as f64;
    let (ux, uz) = (wx - cx * 256.0, wz - cz * 256.0);
    // Across the strips: the nearest line between two of them, or the side of the square.
    let (across, other) = if along_x { (ux, uz) } else { (uz, ux) };
    let width = 256.0 / strips;
    let line = (across / width).round();
    let to_line = line * width - across;
    let to_side = if other < 128.0 { -other } else { 256.0 - other };
    let (step, id) = if to_line.abs() <= to_side.abs() {
        let inner = line > 0.0 && line < strips;
        // A line inside the square belongs to it; a side is shared with the next square.
        let id = if inner { ihash(ix * 5 + line as i32, iz * 3 + 40) } else { ihash(if along_x { (cx + line / strips) as i32 } else { ix } + 500, if along_x { iz } else { (cz + line / strips) as i32 } + 900 + along_x as i32) };
        (if along_x { (to_line, 0.0) } else { (0.0, to_line) }, id)
    } else {
        let side = if other < 128.0 { 0.0 } else { 1.0 };
        let id = ihash(if along_x { ix } else { (cx + side) as i32 } + 500, if along_x { (cz + side) as i32 } else { iz } + 900 + !along_x as i32);
        (if along_x { (0.0, to_side) } else { (to_side, 0.0) }, id)
    };
    (step, id)
}

/// Side of a square of farmland, metres (`terrain_fragment` paints its fields in these squares).
const FIELD: f64 = 256.0;
/// Half the side of a farmyard.
const YARD: f64 = 26.0;

/// Below what height the ground is too near the sea for a tree, a farm or a house: `generated` in a
/// generated world, whose beaches and marshes reach some metres up; just above the land in a region,
/// whose coast is where its maps put it (its land is 0.5 m up at least).
fn shore(terrain: &Terrain, generated: f64) -> f64 {
    if terrain.region().is_some() {
        0.6
    } else {
        generated
    }
}

/// A farm: the middle of its yard (east, south), whether its buildings are turned a quarter, and a
/// number of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Farm {
    x: f64,
    z: f64,
    turned: bool,
    seed: u64,
}

impl Farm {
    /// A point of the yard given along and across its buildings, to (east, south).
    fn at(&self, u: f64, v: f64) -> (f64, f64) {
        if self.turned { (self.x - v, self.z + u) } else { (self.x + u, self.z + v) }
    }

    fn r(&self, k: u64) -> f64 {
        hash2(self.seed as i64, k as i64, 9711)
    }

    /// The buildings: kind, position along and across, heading along the first axis or across it, scale.
    fn buildings(&self) -> Vec<(Building, f64, f64, bool, f64)> {
        let house = if self.r(1) < 0.6 { Building::Cottage } else { Building::HipHouse };
        let mut list = vec![(house, -8.0, -6.0, self.r(2) < 0.5, 0.9 + 0.2 * self.r(3)), (Building::Shed, 10.0, 4.0, true, 0.6 + 0.2 * self.r(4))];
        if self.r(5) < 0.45 {
            list.push((Building::Shed, -8.0, 14.0, false, 0.6 + 0.2 * self.r(6)));
        }
        list
    }

    /// The trees of the yard: kind, position along and across, height.
    fn trees(&self) -> Vec<(Tree, f64, f64, f64)> {
        let mut list = vec![(if self.r(7) < 0.5 { Tree::Oak } else { Tree::HolmOak }, 18.0, -16.0, 7.0 + 4.0 * self.r(8))];
        if self.r(9) < 0.7 {
            list.push((if self.r(10) < 0.6 { Tree::Poplar } else { Tree::Ash }, -19.5, 2.0, 7.0 + 4.0 * self.r(11)));
        }
        list
    }

    /// Whether (east, south) is in the yard, with `margin` metres more around it.
    fn holds(&self, x: f64, z: f64, margin: f64) -> bool {
        (x - self.x).abs() < YARD + margin && (z - self.z).abs() < YARD + margin
    }
}

/// The farm of the square of farmland (east index, south index), if it has one: one square in five
/// or so, where the whole yard is level farmland away from towns, aerodromes, rivers and roads. Its
/// yard is in one corner of the square, clear of the hedges.
fn farm(terrain: &Terrain, cell: (i64, i64)) -> Option<Farm> {
    let seed = terrain.seed().wrapping_add(9700);
    if hash2(cell.0, cell.1, seed) > 0.22 {
        return None;
    }
    let corner = |k: u64| if hash2(cell.0, cell.1, seed.wrapping_add(k)) < 0.5 { YARD + 20.0 } else { FIELD - YARD - 20.0 };
    let (x, z) = (cell.0 as f64 * FIELD + corner(1), cell.1 as f64 * FIELD + corner(2));
    let towns = terrain.towns(-z - 100.0, -z + 100.0, x - 100.0, x + 100.0);
    if towns.iter().any(|t| (t.n + z).hypot(t.e - x) < t.radius * 1.15 + 80.0) {
        return None;
    }
    if !terrain.airports(-z, -z, x, x).is_empty() && terrain.ground(-z, x).flat > 0.0 {
        return None;
    }
    for (dx, dz) in [(0.0, 0.0), (-YARD, -YARD), (YARD, -YARD), (-YARD, YARD), (YARD, YARD)] {
        let (px, pz) = (x + dx, z + dz);
        let g = terrain.ground(-pz, px);
        if g.h < shore(terrain, 12.0) || g.river > 0.0 || g.flat > 0.0 {
            return None;
        }
        let d = 8.0;
        let slope = (terrain.ground(-pz, px + d).h - g.h).hypot(terrain.ground(-(pz + d), px).h - g.h) / d;
        if slope > 0.06 || terrain.surface(-pz, px, &g, slope, 0.0).farm < 0.85 {
            return None;
        }
    }
    // Roads between towns run through the farmland; a yard is not put on one.
    for r in terrain.roads(-z - 50.0, -z + 50.0, x - 50.0, x + 50.0) {
        let (dn, de) = (r.n1 - r.n0, r.e1 - r.e0);
        let t = (((-z - r.n0) * dn + (x - r.e0) * de) / (dn * dn + de * de)).clamp(0.0, 1.0);
        if (-z - (r.n0 + dn * t)).hypot(x - (r.e0 + de * t)) < YARD * 1.5 + 10.0 {
            return None;
        }
    }
    let turned = hash2(cell.0, cell.1, seed.wrapping_add(3)) < 0.5;
    Some(Farm { x, z, turned, seed: (hash2(cell.0, cell.1, seed.wrapping_add(4)) * 1e9) as u64 })
}

/// The farms whose yards can reach into the tile with corner (east, south) `(ox, oz)` and side `size`.
fn farms(terrain: &Terrain, ox: f64, oz: f64, size: f64) -> Vec<Farm> {
    let mut list = Vec::new();
    for cx in (ox / FIELD).floor() as i64..=((ox + size) / FIELD).floor() as i64 {
        for cz in (oz / FIELD).floor() as i64..=((oz + size) / FIELD).floor() as i64 {
            list.extend(farm(terrain, (cx, cz)));
        }
    }
    list
}

/// How wet the land is at a point, 0 (dry) to 1, as `Terrain::surface` works it out: where holm
/// oaks and pines grow rather than oaks and ashes. Seeded noise over tens of kilometres.
pub fn moisture(terrain: &Terrain, n: f64, e: f64) -> f64 {
    clamp(fbm(n / 6000.0, e / 6000.0, 3, terrain.seed().wrapping_add(83)) * 1.3 + 0.5, 0.0, 1.0)
}

/// How much a real place has the dry summers of the Mediterranean, 0 to 1, by its latitude: 1
/// between 31° and 44° (Menorca, Barcelona, California), none north of 46.5° (Innsbruck) or in the
/// tropics. Its lowland conifers are pines there, not spruces and firs. A generated world has none:
/// its own moisture decides.
pub fn summer_drought(terrain: &Terrain) -> f64 {
    let Some(region) = terrain.region() else { return 0.0 };
    let latitude = region.latitude.abs() / DEG;
    smoothstep(25.0, 31.0, latitude) * (1.0 - smoothstep(44.0, 46.5, latitude))
}

/// Up to what height a place where the summers are dry (`summer_drought`) has pines rather than
/// spruces and firs, metres.
pub const PINES_UP_TO: f64 = 1500.0;

/// The number of kinds of tree and of building: the lists `trees` and `houses` return.
pub const TREE_KINDS: usize = 8;
pub const HOUSE_KINDS: usize = 9;

/// How far from the kerb of a road a tree's trunk stands at least, metres: the trees along an
/// avenue stand on its pavement, their crowns over the carriageway, never on it.
pub const ROAD_VERGE: f64 = 2.5;
/// How far from the walls of a building of the maps a tree's trunk stands at least, metres.
pub const WALL_GAP: f64 = 3.0;

/// The roads whose carriageway comes within `pad` metres of the tile with corner (east, south)
/// `(ox, oz)` and side `size`: a region's square of roads holds hundreds, a tile few of them.
fn near_roads(terrain: &Terrain, ox: f64, oz: f64, size: f64, pad: f64) -> Vec<Road> {
    let (n0, n1, e0, e1) = (-(oz + size), -oz, ox, ox + size);
    let mut roads = terrain.roads(n0 - pad, n1 + pad, e0 - pad, e1 + pad);
    roads.retain(|r| {
        let reach = r.width / 2.0 + pad;
        r.n0.min(r.n1) - reach < n1 && r.n0.max(r.n1) + reach > n0 && r.e0.min(r.e1) - reach < e1 && r.e0.max(r.e1) + reach > e0
    });
    roads
}

/// What keeps the trees of a tile off the ground that is not theirs: the roads that reach into
/// it, and in a region a railway's ground and the buildings of its maps.
pub struct Clear<'a> {
    terrain: &'a Terrain,
    roads: Vec<Road>,
}

impl<'a> Clear<'a> {
    /// For the tile with corner (east, south) `(ox, oz)` and side `size`.
    pub fn new(terrain: &'a Terrain, ox: f64, oz: f64, size: f64) -> Clear<'a> {
        Clear { terrain, roads: near_roads(terrain, ox, oz, size, ROAD_VERGE + 1.0) }
    }

    /// Whether a tree may stand at (north, east): its trunk `ROAD_VERGE` off every road's kerb,
    /// not on a railway's ground, and `WALL_GAP` off every building of the maps.
    pub fn holds(&self, n: f64, e: f64) -> bool {
        if self.roads.iter().any(|r| r.distance(n, e) < r.width / 2.0 + ROAD_VERGE) {
            return false;
        }
        let Some(region) = self.terrain.region() else { return true };
        region.cover_at(n, e) != Cover::Railway && !self.terrain.building_near(n, e, WALL_GAP)
    }
}

/// The trees of a tile: forest, lone trees, scrub on dry open land and the trees along the rivers.
/// None on a road, a runway, a railway's ground or a building of the maps (`Clear`).
pub fn trees(terrain: &Terrain, key: (i32, i32), size: f64) -> Instances {
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    let airports = terrain.airports(-(oz + size), -oz, ox, ox + size);
    let towns = terrain.towns(-(oz + size), -oz, ox, ox + size);
    let clear = Clear::new(terrain, ox, oz, size);
    let n = (size / SPACING).round() as i64;
    let seed = terrain.seed().wrapping_add(7777);
    let mut all: Instances = vec![Vec::new(); TREE_KINDS];
    // Where the summers are dry the conifers are pines (`summer_drought`).
    let pines = summer_drought(terrain) > 0.5;
    let farms = farms(terrain, ox, oz, size);
    // The trails the race courses cut through the forest, those that reach into the tile
    // (`course::Cut`): what grows there is cut down, drawn and run into alike.
    let reach = |c: &crate::course::Cut| c.half.0.max(c.half.1) + c.band + 30.0;
    let cuts: Vec<crate::course::Cut> = crate::course::cuts(terrain)
        .into_iter()
        .filter(|c| {
            let (n0, n1) = (c.from.0.min(c.to.0) - reach(c), c.from.0.max(c.to.0) + reach(c));
            let (e0, e1) = (c.from.1.min(c.to.1) - reach(c), c.from.1.max(c.to.1) + reach(c));
            n1 > -(oz + size) && n0 < -oz && e1 > ox && e0 < ox + size
        })
        .collect();
    // The trees of the farmyards that stand in this tile.
    for f in &farms {
        for (kind, u, v, height) in f.trees() {
            let (x, z) = f.at(u, v);
            if x < ox || x >= ox + size || z < oz || z >= oz + size || !clear.holds(-z, x) {
                continue;
            }
            let (h, ..) = ground(terrain, -z, x, &airports);
            let d = 6.0;
            let sx = (ground(terrain, -z, x + d, &airports).0 - h) / d;
            let sz = (ground(terrain, -(z + d), x, &airports).0 - h) / d;
            let tone = ((0.68 + 0.3 * f.r(20)) * 32.0).floor() + f.r(21) * 0.999;
            all[kind as usize].extend_from_slice(&[(x - ox) as f32, h as f32 - 0.25, (z - oz) as f32, height as f32, tone as f32, (0.95 + 0.3 * f.r(22)) as f32, sx as f32, sz as f32]);
        }
    }
    for j in 0..n {
        for i in 0..n {
            let (gx, gz) = (key.0 as i64 * n + i, key.1 as i64 * n + j);
            // Near a course's trail the forest is thicker (`file::Trail::dense`): more spots in a
            // cell, each drawn as the first is, from hashes of its own.
            let middle = (ox + (i as f64 + 0.5) * SPACING, oz + (j as f64 + 0.5) * SPACING);
            let copies = if cuts.is_empty() { 1 } else { crate::course::thicket(&cuts, -middle.1, middle.0) };
            for copy in 0..copies {
                let seed = seed.wrapping_add(1000 * copy as u64);
                let r = |k: u64| hash2(gx, gz, seed.wrapping_add(k));
                let (mut x, mut z) = (ox + (i as f64 + r(0)) * SPACING, oz + (j as f64 + r(1)) * SPACING);
                // Near the boundary between two fields the spot moves on to it: hedges and lines of
                // trees grow there, on the boundaries that have them. A region's fields are not these
                // squares (the renderer draws them from the picture of the ground): no hedges there.
                let ((to_x, to_z), boundary) = field_boundary(x, z);
                let hedge = copy == 0 && to_x.abs().max(to_z.abs()) < SPACING * 0.3 && boundary < 0.55 && terrain.region().is_none();
                if hedge {
                    (x, z) = (x + to_x + (r(10) - 0.5) * 1.2, z + to_z + (r(11) - 0.5) * 1.2);
                }
                let (h, river, flat, valley) = ground(terrain, -z, x, &airports);
                // Not in the water, on the beach or on an airfield.
                if h < shore(terrain, 5.0) || river > 0.05 || flat > 0.02 {
                    continue;
                }
                // In a town trees grow in the gardens only.
                // Nor in a farmyard.
                if farms.iter().any(|f| f.holds(x, z, 4.0)) {
                    continue;
                }
                let town = town_at(&towns, -z, x, 0.25);
                if town.is_some_and(|t| !in_garden(t, -z, x) || r(9) > 0.4) {
                    continue;
                }
                if !clear.holds(-z, x) {
                    continue;
                }

                let d = 6.0;
                let sx = (ground(terrain, -z, x + d, &airports).0 - h) / d;
                let sz = (ground(terrain, -(z + d), x, &airports).0 - h) / d;
                let slope = (sx * sx + sz * sz).sqrt();
                let g = Ground { h, river, flat };
                let s = terrain.surface(-z, x, &g, slope, 0.0);
                // How wet the land is, as `Terrain::surface` works it out, and which trees grow in stands here.
                let moist = moisture(terrain, -z, x);
                let stand = fbm(x / 380.0, z / 380.0, 2, seed.wrapping_add(40));
                let scrub = smoothstep(-0.05, 0.45, fbm(x / 700.0, z / 700.0, 2, seed.wrapping_add(41)));
                let open = s.farm < 0.3 && slope < 0.5 && h < 1700.0;
                let chance = r(2);
                let in_forest = s.forest as f64 * 0.9 >= chance;
                let lone = open && chance < 0.012;
                let bank = open && valley > 0.8 && chance < 0.55 * smoothstep(0.8, 0.93, valley);
                let rough = open && s.forest < 0.5 && r(6) < 0.11 * scrub * (1.0 - smoothstep(0.55, 0.8, moist));
                let hedge = hedge && s.farm > 0.7 && r(12) < 0.85;
                if !(in_forest || lone || bank || rough || hedge || town.is_some()) {
                    continue;
                }

                let (r1, r2, r3) = (r(3), r(4), r(5));
                let mountain = smoothstep(700.0, 1100.0, h + 150.0 * stand);
                let dry = 1.0 - smoothstep(0.35, 0.65, moist + 0.1 * stand);
                let kind = if town.is_some() {
                    if r1 < 0.3 { Tree::Poplar } else if r1 < 0.65 { Tree::HolmOak } else if r1 < 0.85 { Tree::Oak } else { Tree::Bush }
                } else if hedge && !in_forest {
                    // Most boundaries are hedges; some are a line of trees.
                    if boundary > 0.42 { if boundary > 0.5 { Tree::Poplar } else { Tree::Oak } } else if r1 < 0.88 { Tree::Bush } else { Tree::HolmOak }
                } else if bank {
                    if r1 < 0.6 { Tree::Poplar } else if r1 < 0.85 { Tree::Ash } else { Tree::Bush }
                } else if rough && !in_forest && !lone {
                    if r1 < 0.72 { Tree::Bush } else if r1 < 0.92 { Tree::HolmOak } else { Tree::Pine }
                } else if r1 < mountain {
                    if pines && h < PINES_UP_TO { Tree::Pine } else if stand > 0.0 { Tree::Spruce } else { Tree::Fir }
                } else if stand > 0.28 {
                    // A stand of conifers in the lowland: pines where it is dry, firs where it is not.
                    if r3 < 0.85 { if dry > 0.5 || pines { Tree::Pine } else { Tree::Fir } } else { Tree::Oak }
                } else if r3 < dry * 0.75 {
                    if r1 < 0.8 { Tree::HolmOak } else { Tree::Pine }
                } else if r3 > 0.93 {
                    Tree::Bush
                } else if stand < -0.25 {
                    if r1 < 0.7 { Tree::Ash } else { Tree::Oak }
                } else if r1 < 0.75 {
                    Tree::Oak
                } else {
                    Tree::Ash
                };
                // Trees get smaller towards the tree line.
                let stunt = 1.0 - 0.5 * smoothstep(1300.0, 1850.0, h);
                let (height, width) = match kind {
                    Tree::Oak => (9.0 + 8.0 * r2, 0.9 + 0.4 * r3),
                    Tree::Ash => (12.0 + 9.0 * r2, 0.85 + 0.35 * r3),
                    Tree::HolmOak => (6.0 + 5.0 * r2, 0.95 + 0.45 * r3),
                    Tree::Poplar => (15.0 + 11.0 * r2, 0.9 + 0.3 * r3),
                    Tree::Spruce => (13.0 + 13.0 * r2, 0.85 + 0.4 * r3),
                    Tree::Fir => (11.0 + 11.0 * r2, 0.9 + 0.4 * r3),
                    Tree::Pine => (8.0 + 7.0 * r2, 0.9 + 0.45 * r3),
                    Tree::Bush => (1.6 + 2.6 * r2, 0.9 + 0.8 * r3),
                };
                // Garden trees are kept small.
                let height = height * if kind == Tree::Bush { 1.0 } else if town.is_some() { 0.6 } else if hedge { 0.75 } else { stunt };
                let (crown, radius, _) = CROWNS[kind as usize];
                if !cuts.is_empty() && crate::course::cut_down(&cuts, -z, x, crown * height, radius * height * width) {
                    continue;
                }

                // Whole stands are lighter or darker, and each tree a little on its own.
                let brightness = 0.62 + 0.14 * stand + 0.30 * r(7);
                let tone = (brightness * 32.0).floor() + r(8) * 0.999;
                all[kind as usize].extend_from_slice(&[(x - ox) as f32, h as f32 - 0.25, (z - oz) as f32, height as f32, tone as f32, width as f32, sx as f32, sz as f32]);
            }
        }
    }
    all
}

/// The kinds of building, in the order of `world_render::buildings::KINDS`, then the parked vehicle (`buildings::PARKED`).
#[derive(Clone, Copy, PartialEq)]
enum Building {
    Cottage,
    HipHouse,
    CornerHouse,
    TownHouse,
    Shed,
    Church,
    Hangar,
    Clubhouse,
    Aircraft,
}

/// Distance between the middles of two plots of a town along each direction of its grid, metres.
/// Every fourth row and every fifth column of plots is a street: `town_ground` in `scenery.wgsl`
/// paints the same grid.
const PLOT: (f64, f64) = (24.0, 19.0);

/// Side of the squares on which a region's built-up ground outside its towns gets a building or none.
const LOT: f64 = 32.0;

/// Where a point (north, east) is in the grid of a town: the plot (row, column), and its position
/// within the plot in metres from the middle.
fn plot(town: &Town, n: f64, e: f64) -> ((i64, i64), (f64, f64)) {
    let (dn, de) = (n - town.n, e - town.e);
    let (cg, sg) = (town.grid.cos(), town.grid.sin());
    let (a, b) = (dn * cg + de * sg, -dn * sg + de * cg);
    let (i, j) = ((a / PLOT.0).round(), (b / PLOT.1).round());
    ((i as i64, j as i64), (a - i * PLOT.0, b - j * PLOT.1))
}

fn is_street(i: i64, j: i64) -> bool {
    i.rem_euclid(4) == 0 || j.rem_euclid(5) == 0
}

/// The church takes the plot (2, 2) and the two beside it.
fn is_church(i: i64, j: i64) -> bool {
    j == 2 && (1..=3).contains(&i)
}

/// Whether a tree may grow at a point inside a town: in a garden, clear of the house and the street.
fn in_garden(town: &Town, n: f64, e: f64) -> bool {
    let ((i, j), (da, db)) = plot(town, n, e);
    !is_street(i, j) && !is_church(i, j) && (da.abs() > 8.5 || db.abs() > 7.5)
}

/// The town a point (north, east) belongs to: of those whose weight there is more than `least`, the
/// one with the nearest middle. A generated world's towns never overlap; a region's may.
fn town_at(towns: &[Town], n: f64, e: f64, least: f64) -> Option<&Town> {
    let distance = |t: &&Town| (t.n - n).hypot(t.e - e);
    towns.iter().filter(|t| t.weight(n, e) > least).min_by(|a, b| distance(a).total_cmp(&distance(b)))
}

/// The buildings in a tile: those of the towns, and the hangars, office and parked aircraft of the aerodromes.
pub fn houses(terrain: &Terrain, key: (i32, i32), size: f64) -> Instances {
    use std::f64::consts::FRAC_PI_2;
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    let mut all: Instances = vec![Vec::new(); HOUSE_KINDS];
    let inside = |n: f64, e: f64| e >= ox && e < ox + size && -n >= oz && -n < oz + size;
    let mut add = |kind: Building, n: f64, e: f64, y: f64, scale: f64, tint: [f32; 3], yaw: f64| {
        all[kind as usize].extend_from_slice(&[(e - ox) as f32, y as f32, (-n - oz) as f32, scale as f32, tint[0], tint[1], tint[2], yaw as f32]);
    };
    let towns = terrain.towns(-(oz + size), -oz, ox, ox + size);
    let airports = terrain.airports(-(oz + size), -oz, ox, ox + size);
    // A region's towns are laid on its built-up ground, not along its streets: none of their
    // houses stands on a road of its maps.
    let roads: Vec<Road> = if terrain.region().is_some() { near_roads(terrain, ox, oz, size, HOUSE_KERB + 1.0) } else { Vec::new() };
    let on_road = |n: f64, e: f64| roads.iter().any(|r| r.distance(n, e) < r.width / 2.0 + HOUSE_KERB);
    for town in &towns {
        let (cg, sg) = (town.grid.cos(), town.grid.sin());
        let reach = (town.radius * 1.2 / PLOT.1) as i64 + 1;
        let seed = terrain.seed().wrapping_add(9000).wrapping_add((town.n as i64 as u64).wrapping_mul(31)).wrapping_add(town.e as i64 as u64);
        // Render space yaw of a model whose x runs along the first direction of the grid.
        let along = FRAC_PI_2 - town.grid;
        for i in -reach..=reach {
            for j in -reach..=reach {
                if is_street(i, j) {
                    continue;
                }
                let (a, b) = (i as f64 * PLOT.0, j as f64 * PLOT.1);
                let (n, e) = (town.n + a * cg - b * sg, town.e + a * sg + b * cg);
                // Where the circles of two towns of a region overlap, each plot is the nearer one's.
                if !inside(n, e) || town_at(&towns, n, e, 0.0).is_some_and(|t| t != town) {
                    continue;
                }
                let r = |k: u64| hash2(i, j, seed.wrapping_add(k));
                let (r1, r2, r3) = (r(0), r(1), r(2));
                let d = a.hypot(b) / town.radius;
                let g = terrain.ground_with(n, e, &airports);
                // In a region, only on its built-up ground: a town's circle there is only mostly built.
                let unbuilt = terrain.region().is_some_and(|r| r.cover_at(n, e) != Cover::Built) || on_road(n, e) || terrain.building_near(n, e, MAPPED_CLEARANCE);
                if g.h < shore(terrain, 2.0) || g.river > 0.05 || g.flat > 0.0 || unbuilt {
                    continue;
                }
                if is_church(i, j) {
                    if i == 2 {
                        add(Building::Church, n, e, g.h - 0.3, 1.0, [0.9, 0.85, 0.8], along);
                    }
                    continue;
                }
                // Fewer houses towards the edge; the middle is built up solid.
                if d > 0.8 + 0.35 * r1 || r2 > (0.97 - 0.42 * d).min(0.9 + 0.1 * (1.0 - smoothstep(0.25, 0.4, d))) {
                    continue;
                }
                let kind = if d < 0.3 {
                    if r3 < 0.7 { Building::TownHouse } else { Building::HipHouse }
                } else if d > 0.85 && r3 < 0.15 {
                    Building::Shed
                } else if r3 < 0.45 {
                    Building::Cottage
                } else if r3 < 0.7 {
                    Building::CornerHouse
                } else if r3 < 0.9 {
                    Building::HipHouse
                } else {
                    Building::TownHouse
                };
                let scale = if kind == Building::Shed { 0.7 + 0.3 * r(3) } else { 0.85 + 0.4 * r(3) };
                let roof: [f32; 3] = match (r(4) * 10.0) as u32 {
                    0..=4 => [1.0, 1.0, 1.0],
                    5..=6 => [0.80, 0.92, 0.95],
                    7..=8 => [1.15, 0.85, 0.75],
                    _ => [0.55, 0.62, 0.70],
                };
                // Ridges along the street the house looks on to, with the odd one turned.
                let yaw = along + if r1 > 0.5 { FRAC_PI_2 } else { 0.0 } + if r(5) > 0.5 { std::f64::consts::PI } else { 0.0 };
                add(kind, n, e, g.h - 0.3, scale, roof, yaw);
            }
        }
    }
    // A region's built-up ground that no town covers (industry, shops, the edges of towns): sheds
    // and houses here and there, on squares of `LOT` metres, turned as the nearest town's streets.
    if let Some(region) = terrain.region() {
        let seed = terrain.seed().wrapping_add(9300);
        let n = (size / LOT).round() as i64;
        for j in 0..n {
            for i in 0..n {
                let (gx, gz) = (key.0 as i64 * n + i, key.1 as i64 * n + j);
                let r = |k: u64| hash2(gx, gz, seed.wrapping_add(k));
                if r(0) > 0.45 {
                    continue;
                }
                let (x, z) = (ox + (i as f64 + 0.3 + 0.4 * r(1)) * LOT, oz + (j as f64 + 0.3 + 0.4 * r(2)) * LOT);
                if region.cover_at(-z, x) != Cover::Built || towns.iter().any(|t| t.weight(-z, x) > 0.0) || on_road(-z, x) || terrain.building_near(-z, x, MAPPED_CLEARANCE) {
                    continue;
                }
                let g = terrain.ground_with(-z, x, &airports);
                if g.h < shore(terrain, 2.0) || g.river > 0.05 || g.flat > 0.0 {
                    continue;
                }
                let (kind, scale) = if r(3) < 0.6 { (Building::Shed, 1.0 + 0.7 * r(4)) } else if r(3) < 0.85 { (Building::HipHouse, 0.9 + 0.3 * r(4)) } else { (Building::TownHouse, 0.9 + 0.3 * r(4)) };
                let grid = towns.first().map_or(0.0, |t| t.grid);
                let yaw = FRAC_PI_2 - grid + if r(5) < 0.5 { FRAC_PI_2 } else { 0.0 };
                add(kind, -z, x, g.h - 0.3, scale, [0.8 + 0.3 * r(6) as f32, 0.85, 0.9], yaw);
            }
        }
    }
    // Farms: each building in the tile its middle is in.
    for f in farms(terrain, ox, oz, size) {
        for (k, (kind, u, v, across, scale)) in f.buildings().into_iter().enumerate() {
            let (x, z) = f.at(u, v);
            if !inside(-z, x) {
                continue;
            }
            // Model x along the yard's first axis, or across it; the far side of a building may face either way.
            let yaw = if across != f.turned { FRAC_PI_2 } else { 0.0 } + if f.r(30 + k as u64) < 0.5 { std::f64::consts::PI } else { 0.0 };
            let roof: [f32; 3] = if f.r(40) < 0.6 { [1.0, 1.0, 1.0] } else { [0.80, 0.92, 0.95] };
            add(kind, -z, x, terrain.ground(-z, x).h - 0.3, scale, roof, yaw);
        }
    }
    for a in &airports {
        for (kind, n, e, yaw) in aerodrome_buildings(terrain, a) {
            if inside(n, e) {
                add(kind, n, e, a.elevation - 0.05, 1.0, [1.0; 3], yaw);
            }
        }
    }
    all
}

/// The hangars, office and parked aircraft of an aerodrome: kind, north, east and render space yaw.
/// They stand at `elevation - 0.05`, on the apron's side of the runway (`Detail::apron`; none
/// without it).
fn aerodrome_buildings(terrain: &Terrain, a: &Airport) -> Vec<(Building, f64, f64, f64)> {
    let side = a.detail.apron;
    if side == 0.0 {
        return Vec::new();
    }
    let (c, s) = (a.heading.cos(), a.heading.sin());
    let hw = a.width / 2.0;
    let r = |k: u64| hash2(a.id as i64, k as i64, terrain.seed().wrapping_add(9500));
    let mut list = Vec::new();
    let mut place = |kind: Building, along: f64, across: f64, yaw: f64| {
        let across = across * side;
        list.push((kind, a.n + along * c - across * s, a.e + along * s + across * c, yaw));
    };
    // Render space yaws: the doors of the hangars and the noses of the aircraft towards the runway.
    let turn = if side < 0.0 { std::f64::consts::PI } else { 0.0 };
    let (to_runway, with_runway) = (c.atan2(-s) + turn, (-c).atan2(s) + turn);
    let hangars = 2 + (r(0) * 2.99) as usize;
    for k in 0..hangars {
        place(Building::Hangar, -14.0 - 25.0 * k as f64, hw + 118.0, to_runway);
    }
    place(Building::Clubhouse, 30.0, hw + 114.0, with_runway);
    let parked = 2 + (r(1) * 4.99) as usize;
    for k in 0..parked {
        if r(10 + k as u64) < 0.8 {
            place(Building::Aircraft, -52.0 + 13.5 * k as f64, hw + 80.0 + 6.0 * r(20 + k as u64), (-s).atan2(-c) + turn + (r(30 + k as u64) - 0.5) * 0.5);
        }
    }
    list
}

/// The buildings of an aerodrome as obstacles, one per block, as `house_obstacles` gives them
/// (from positions in `f64`, not rounded to a tile): what its obstacle lights stand on.
pub fn aerodrome_obstacles(terrain: &Terrain, a: &Airport) -> Vec<Obstacle> {
    let mut list = Vec::new();
    for (kind, n, e, yaw) in aerodrome_buildings(terrain, a) {
        for &block in BLOCKS[kind as usize] {
            list.push(Obstacle { kind: obstacle_kind(kind as usize), n, e, base: a.elevation - 0.05, shape: Shape::Block { block, yaw, scale: 1.0 } });
        }
    }
    list
}


// MARK: Obstacles

/// What an aircraft has run into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObstacleKind {
    Tree,
    Building,
    Hangar,
    /// A parked aircraft.
    Aircraft,
    /// A race course's gate, flag or arch (`course`).
    Gate,
    /// A bridge of a real place: its deck, a pier, a tower or a cable.
    Bridge,
}

/// The solid of an obstacle, in metres above its foot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A trunk of radius `trunk` up to `crown`, then a crown of `radius` up to `top`, narrowing
    /// to a point if `cone`.
    Tree { trunk: f64, crown: f64, radius: f64, top: f64, cone: bool },
    /// A block of a building in its model's axes (x along the ridge of its main roof, z across,
    /// as `Block` says), turned by `yaw` as the instance is and scaled by `scale`.
    Block { block: Block, yaw: f64, scale: f64 },
    /// A round bar of `radius` from its foot to `(dn, de, up)` metres north, east and up of it, any
    /// way: a tube of a race gate's frame (`course`), which may lie flat high above the ground.
    Bar { dn: f64, de: f64, up: f64, radius: f64 },
    /// A building or a bridge of a real place's maps: its solid is the terrain's to say
    /// (`Terrain::solid_at`), so this one holds no point; it says what was struck.
    Mapped,
}

/// A box of a building in model axes (x, z, y up): middle and half size of its outline, floor
/// and eaves, and a roof that rises `ridge` above the eaves along x (or along z with `ridge_z`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub x: f64,
    pub z: f64,
    pub half_x: f64,
    pub half_z: f64,
    pub floor: f64,
    pub eaves: f64,
    pub ridge: f64,
    pub ridge_z: bool,
}

const fn block(x: f64, z: f64, half_x: f64, half_z: f64, eaves: f64, ridge: f64) -> Block {
    Block { x, z, half_x, half_z, floor: 0.0, eaves, ridge, ridge_z: false }
}

/// The solid blocks of each kind of building, in the order of the kinds `houses` returns (the
/// world's `world_render::buildings` draws them, and the game what it parks; `buildings::check` says whether they agree).
pub const BLOCKS: [&[Block]; HOUSE_KINDS] = [
    &[block(0.0, 0.0, 4.5, 3.5, 5.4, 2.3)],
    &[block(0.0, 0.0, 5.0, 4.5, 5.8, 2.4)],
    &[block(-1.0, -1.5, 5.5, 3.4, 5.6, 2.2), block(3.4, 3.2, 3.0, 3.2, 3.0, 1.6)],
    &[block(0.0, 0.0, 4.6, 4.2, 8.8, 1.7)],
    &[block(0.0, 0.0, 12.0, 5.5, 4.6, 1.8)],
    &[block(2.5, 0.0, 10.0, 4.6, 8.0, 4.0), block(-10.0, 0.0, 2.9, 2.9, 19.0, 5.0)],
    &[Block { x: 0.0, z: 0.0, half_x: 9.0, half_z: 11.0, floor: 0.0, eaves: 5.6, ridge: 2.4, ridge_z: true }],
    &[block(0.0, 0.0, 8.0, 4.5, 3.4, 0.0), block(5.0, 0.0, 3.0, 3.0, 9.5, 0.0)],
    // A parked aircraft: wing, fuselage and gear, tailplane, fin.
    &[
        Block { x: 0.0, z: 0.0, half_x: 0.75, half_z: 5.45, floor: 1.9, eaves: 2.05, ridge: 0.0, ridge_z: false },
        Block { x: -1.7, z: 0.0, half_x: 3.5, half_z: 0.55, floor: 0.0, eaves: 1.9, ridge: 0.0, ridge_z: false },
        Block { x: -4.9, z: 0.0, half_x: 0.5, half_z: 1.7, floor: 1.3, eaves: 1.45, ridge: 0.0, ridge_z: false },
        Block { x: -5.0, z: 0.0, half_x: 0.6, half_z: 0.1, floor: 1.3, eaves: 2.9, ridge: 0.0, ridge_z: false },
    ],
];

/// Crown of each kind of tree, as fractions of its height: where it starts, its radius (times the
/// tree's width) and whether it narrows to the top. The world draws each as species whose meshes fit it
/// (`world_render`'s `vegetation`, `species::fit`).
pub const CROWNS: [(f64, f64, bool); TREE_KINDS] = [
    (0.27, 0.40, false),
    (0.33, 0.33, false),
    (0.28, 0.48, false),
    (0.12, 0.17, false),
    (0.10, 0.20, true),
    (0.16, 0.25, true),
    (0.66, 0.40, false),
    (0.0, 0.50, false),
];

/// A tree or a block of a building.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    pub kind: ObstacleKind,
    /// Its foot: north, east, and the height it stands at.
    pub n: f64,
    pub e: f64,
    pub base: f64,
    pub shape: Shape,
}

impl Obstacle {
    /// How far from its foot it reaches sideways, and how high above it.
    pub fn extent(&self) -> (f64, f64) {
        match self.shape {
            Shape::Tree { radius, top, trunk, .. } => (radius.max(trunk), top),
            Shape::Block { block: b, scale, .. } => (((b.x.abs() + b.half_x).hypot(b.z.abs() + b.half_z)) * scale, (b.eaves + b.ridge) * scale),
            Shape::Bar { dn, de, up, radius } => (dn.hypot(de) + radius, up.max(0.0) + radius),
            Shape::Mapped => (0.0, 0.0),
        }
    }

    /// Whether a point (north, east, height) is inside.
    pub fn contains(&self, n: f64, e: f64, h: f64) -> bool {
        let (dn, de, up) = (n - self.n, e - self.e, h - self.base);
        if let Shape::Bar { dn: bn, de: be, up: bu, radius } = self.shape {
            // The nearest point of the bar's axis.
            let length2 = bn * bn + be * be + bu * bu;
            let t = if length2 > 0.0 { ((dn * bn + de * be + up * bu) / length2).clamp(0.0, 1.0) } else { 0.0 };
            let (x, y, z) = (dn - t * bn, de - t * be, up - t * bu);
            return x * x + y * y + z * z < radius * radius;
        }
        if up < 0.0 {
            return false;
        }
        match self.shape {
            Shape::Tree { trunk, crown, radius, top, cone } => {
                if up > top {
                    return false;
                }
                let r = if up < crown {
                    trunk
                } else if cone {
                    radius * (top - up) / (top - crown).max(0.1)
                } else {
                    radius
                };
                dn * dn + de * de < r * r
            }
            Shape::Block { block: b, yaw, scale } => {
                // Back into the model's axes: east and south turned by -yaw (see `building_vertex`).
                let (east, south) = (de, -dn);
                let (c, s) = (yaw.cos(), yaw.sin());
                let x = (east * c + south * s) / scale - b.x;
                let z = (-east * s + south * c) / scale - b.z;
                let y = up / scale;
                if x.abs() > b.half_x || z.abs() > b.half_z || y < b.floor {
                    return false;
                }
                let slope = if b.ridge_z { 1.0 - x.abs() / b.half_x } else { 1.0 - z.abs() / b.half_z };
                y < b.eaves + b.ridge * slope
            }
            Shape::Bar { .. } => unreachable!("a bar was answered above"),
            Shape::Mapped => false,
        }
    }
}

impl Obstacle {
    /// A tree's trunk's radius `up` metres over its foot: as wide as the drawn trunk at its foot,
    /// narrowing to a fifth of that at its top (what `vehicle` and a multirotor strike).
    pub fn trunk_radius(trunk: f64, top: f64, up: f64) -> f64 {
        trunk * (1.0 - 0.8 * (up / top.max(0.1)).clamp(0.0, 1.0))
    }

    /// The solid surface of it nearest a point (north, east, height): how far outside it the point
    /// is (negative inside), the nearest point of the surface and the way out of it there (a unit
    /// vector, north, east, up). A tree is its trunk (its crown is twigs and leaves, `in_crown`);
    /// a block of a building its box up to its eaves and the roof's slope over the point.
    pub fn surface(&self, n: f64, e: f64, h: f64) -> (f64, [f64; 3], [f64; 3]) {
        let (dn, de, up) = (n - self.n, e - self.e, h - self.base);
        let out = |d: f64, at: (f64, f64, f64), way: (f64, f64, f64)| {
            let l = (way.0 * way.0 + way.1 * way.1 + way.2 * way.2).sqrt();
            let way = if l > 1e-9 { [way.0 / l, way.1 / l, way.2 / l] } else { [0.0, 0.0, 1.0] };
            (d, [self.n + at.0, self.e + at.1, self.base + at.2], way)
        };
        match self.shape {
            // A building or bridge of a real place: its surface is the terrain's (`Solids::surface`).
            Shape::Mapped => out(f64::INFINITY, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)),
            Shape::Tree { trunk, top, .. } => {
                // The axis of the trunk from its foot to its top: the nearest point of it.
                let z = up.clamp(0.0, top);
                let r = Obstacle::trunk_radius(trunk, top, z);
                let flat = dn.hypot(de);
                if up >= 0.0 && up <= top {
                    let way = if flat > 1e-9 { (dn, de, 0.0) } else { (1.0, 0.0, 0.0) };
                    let k = if flat > 1e-9 { r / flat } else { 0.0 };
                    out(flat - r, (dn * k, de * k, z), way)
                } else {
                    // Below its foot (in the ground) or over its top: from the end of the axis.
                    let way = (dn, de, up - z);
                    let d = (dn * dn + de * de + (up - z).powi(2)).sqrt();
                    out(d - r, (0.0, 0.0, z), way)
                }
            }
            Shape::Bar { dn: bn, de: be, up: bu, radius } => {
                let length2 = bn * bn + be * be + bu * bu;
                let t = if length2 > 0.0 { ((dn * bn + de * be + up * bu) / length2).clamp(0.0, 1.0) } else { 0.0 };
                let (x, y, z) = (dn - t * bn, de - t * be, up - t * bu);
                let d = (x * x + y * y + z * z).sqrt();
                let k = if d > 1e-9 { radius / d } else { 0.0 };
                out(d - radius, (t * bn + x * k, t * be + y * k, t * bu + z * k), (x, y, z))
            }
            Shape::Block { block: b, yaw, scale } => {
                let (east, south) = (de, -dn);
                let (c, s) = (yaw.cos(), yaw.sin());
                let x = (east * c + south * s) / scale - b.x;
                let z = (-east * s + south * c) / scale - b.z;
                let y = up / scale;
                // The roof over the point: as high as its slope there.
                let slope = if b.ridge_z { 1.0 - (x.abs() / b.half_x).min(1.0) } else { 1.0 - (z.abs() / b.half_z).min(1.0) };
                let roof = b.eaves + b.ridge * slope;
                let (lo, hi) = ([-b.half_x, b.floor, -b.half_z], [b.half_x, roof, b.half_z]);
                let p = [x, y, z];
                let inside = (0..3).all(|k| p[k] >= lo[k] && p[k] <= hi[k]);
                // Model axes (x, y up, z): the nearest point of the box and the way out.
                let (near, way, d) = if inside {
                    // Out through the nearest face.
                    let mut best = (f64::MAX, 0, 1.0);
                    for k in 0..3 {
                        for (side, face) in [(-1.0, lo[k]), (1.0, hi[k])] {
                            let gap = (p[k] - face).abs();
                            if gap < best.0 {
                                best = (gap, k, side);
                            }
                        }
                    }
                    let mut near = p;
                    near[best.1] = if best.2 > 0.0 { hi[best.1] } else { lo[best.1] };
                    let mut way = [0.0; 3];
                    way[best.1] = best.2;
                    (near, way, -best.0)
                } else {
                    let near = [p[0].clamp(lo[0], hi[0]), p[1].clamp(lo[1], hi[1]), p[2].clamp(lo[2], hi[2])];
                    let way = [p[0] - near[0], p[1] - near[1], p[2] - near[2]];
                    let d = (way[0] * way[0] + way[1] * way[1] + way[2] * way[2]).sqrt();
                    (near, way, d)
                };
                // Back to the world: east = c x - s z, south = s x + c z (before the block's offset
                // for the point, after it for a way).
                let to_world = |x: f64, z: f64| (c * x - s * z, s * x + c * z);
                let (ne, ns) = to_world((near[0] + b.x) * scale, (near[2] + b.z) * scale);
                let (we, ws) = to_world(way[0], way[2]);
                out(d * scale, (-ns, ne, near[1] * scale), (-ws, we, way[1]))
            }
        }
    }

    /// Whether a point (north, east, height) is in a tree's crown, outside its trunk.
    pub fn in_crown(&self, n: f64, e: f64, h: f64) -> bool {
        self.crown_depth(n, e, h) > 0.0
    }

    /// How deep a point (north, east, height) is in a tree's crown, metres in from its nearest
    /// side, bottom or top (0 outside it, or in its trunk).
    pub fn crown_depth(&self, n: f64, e: f64, h: f64) -> f64 {
        let Shape::Tree { crown, radius, top, cone, .. } = self.shape else { return 0.0 };
        let up = h - self.base;
        if up < crown || up > top || self.surface(n, e, h).0 <= 0.0 {
            return 0.0;
        }
        let r = if cone { radius * (top - up) / (top - crown).max(0.1) } else { radius };
        (r - (n - self.n).hypot(e - self.e)).min(up - crown).min(top - up).max(0.0)
    }
}

/// The trees of a tile as obstacles (`trees` gives the same tile to draw).
pub fn tree_obstacles(terrain: &Terrain, key: (i32, i32), size: f64) -> Vec<Obstacle> {
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    let mut list = Vec::new();
    for (kind, instances) in trees(terrain, key, size).iter().enumerate() {
        let (crown, radius, cone) = CROWNS[kind];
        for v in instances.as_chunks::<8>().0 {
            let (height, width) = (v[3] as f64, v[5] as f64);
            list.push(Obstacle {
                kind: ObstacleKind::Tree,
                n: -(v[2] as f64 + oz),
                e: v[0] as f64 + ox,
                base: v[1] as f64,
                shape: Shape::Tree { trunk: (0.03 * height).max(0.15), crown: crown * height, radius: radius * height * width, top: height, cone },
            });
        }
    }
    list
}

/// What a building of a kind of `houses` is when something runs into it.
fn obstacle_kind(kind: usize) -> ObstacleKind {
    match kind {
        k if k == Building::Hangar as usize => ObstacleKind::Hangar,
        k if k == Building::Aircraft as usize => ObstacleKind::Aircraft,
        _ => ObstacleKind::Building,
    }
}

/// The buildings of a tile as obstacles, one per block (`houses` gives the same tile to draw). Those
/// of a region's maps and its bridges are not here: each is its own solid, the terrain's
/// (`Terrain::solid_at`).
pub fn house_obstacles(terrain: &Terrain, key: (i32, i32), size: f64) -> Vec<Obstacle> {
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    let mut list = Vec::new();
    for (kind, instances) in houses(terrain, key, size).iter().enumerate() {
        let what = obstacle_kind(kind);
        for v in instances.as_chunks::<8>().0 {
            for &block in BLOCKS[kind] {
                list.push(Obstacle {
                    kind: what,
                    n: -(v[2] as f64 + oz),
                    e: v[0] as f64 + ox,
                    base: v[1] as f64,
                    shape: Shape::Block { block, yaw: v[7] as f64, scale: v[3] as f64 },
                });
            }
        }
    }
    list
}

/// What a building of a region's maps stands on: the lowest ground under its corners, metres.
pub fn mapped_base(terrain: &Terrain, b: &Footprint) -> f64 {
    b.ring.iter().map(|p| terrain.height(p[0] as f64, p[1] as f64)).fold(f64::INFINITY, f64::min)
}

/// The tallest thing that stands on the ground here: the church tower, or a region's tallest
/// building or bridge tower.
pub fn tallest(terrain: &Terrain) -> f64 {
    TALLEST.max(terrain.tallest_building())
}

/// How far a building of the towns reaches from its middle, at most, metres.
pub const HOUSE_REACH: f64 = 20.0;

/// No house of the towns stands within this many metres of a building of the maps: where the maps
/// draw them, they draw all of them.
const MAPPED_CLEARANCE: f64 = 50.0;
/// How far the middle of a house of a region's towns stands from the kerb of a road at least,
/// metres: half the largest house, and its front garden.
const HOUSE_KERB: f64 = 8.0;

/// Side of the tiles `tree_obstacles` and `house_obstacles` are asked for.
pub const TREE_TILE: f64 = 128.0;
pub const HOUSE_TILE: f64 = 512.0;
/// The tallest thing that stands on the ground: the church tower, metres.
pub const TALLEST: f64 = 26.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// The instances of a list as (kind, world x, world z, the other six floats), sorted.
    fn world(instances: &Instances, ox: f64, oz: f64) -> Vec<(usize, i64, i64, Vec<i64>)> {
        let mut all = Vec::new();
        for (kind, list) in instances.iter().enumerate() {
            for v in list.as_chunks::<8>().0 {
                let mm = |x: f64| (x * 1000.0).round() as i64;
                all.push((kind, mm(v[0] as f64 + ox), mm(v[2] as f64 + oz), [v[1], v[3], v[4], v[5], v[6], v[7]].iter().map(|x| mm(*x as f64)).collect()));
            }
        }
        all.sort();
        all
    }

    #[test]
    fn near_and_far_tiles_hold_the_same_trees() {
        // The detailed trees come from 128 m tiles and the simple ones from bigger tiles (`FAR_TILE` in
        // the game, 256 m); a tree must be in both, or it would vanish or be drawn twice where one gives
        // way to the other.
        let terrain = Terrain::new(1);
        let home = terrain.nearest_airport(0.0, 0.0).unwrap();
        // A tile beside the home runway (airfield, farmland, hedges) and one by the first town.
        let town = terrain.towns(-12_000.0, 12_000.0, 0.0, 24_000.0)[0];
        let mut total = 0;
        for ((n, e), size) in [(home.n, home.e + 600.0), (town.n, town.e)].into_iter().flat_map(|place| [(place, 256.0), (place, 512.0)]) {
            let key = ((e / size).floor() as i32, (-n / size).floor() as i32);
            let far = world(&trees(&terrain, key, size), key.0 as f64 * size, key.1 as f64 * size);
            let per = (size / 128.0) as i32;
            let mut near = Vec::new();
            for i in 0..per {
                for j in 0..per {
                    let k = (key.0 * per + i, key.1 * per + j);
                    near.extend(world(&trees(&terrain, k, 128.0), k.0 as f64 * 128.0, k.1 as f64 * 128.0));
                }
            }
            near.sort();
            assert_eq!(near.len(), far.len());
            // Positions are kept relative to the tile in f32: allow a millimetre or two.
            for (a, b) in near.iter().zip(&far) {
                assert_eq!(a.0, b.0);
                assert!((a.1 - b.1).abs() <= 2 && (a.2 - b.2).abs() <= 2, "{a:?} {b:?}");
                assert!(a.3.iter().zip(&b.3).all(|(x, y)| (x - y).abs() <= 2), "{a:?} {b:?}");
            }
            total += far.len();
        }
        assert!(total > 100, "only {total} trees: the test tiles are not where it was meant");
    }

    #[test]
    fn nothing_grows_on_a_runway() {
        let terrain = Terrain::new(1);
        let a = terrain.nearest_airport(0.0, 0.0).unwrap();
        let (x0, z0) = (((a.e - 700.0) / 512.0).floor() as i32, ((-a.n - 700.0) / 512.0).floor() as i32);
        let (mut trees_seen, mut buildings) = (0, 0);
        for i in 0..4 {
            for j in 0..4 {
                let key = (x0 + i, z0 + j);
                let (ox, oz) = (key.0 as f64 * 512.0, key.1 as f64 * 512.0);
                for (list, is_tree) in [(trees(&terrain, key, 512.0), true), (houses(&terrain, key, 512.0), false)] {
                    for v in list.iter().flat_map(|l| l.as_chunks::<8>().0) {
                        let (n, e) = (-(v[2] as f64 + oz), v[0] as f64 + ox);
                        let (along, across) = a.local(n, e);
                        // The runway and a strip of 30 m on each side are clear.
                        assert!(along.abs() > a.length / 2.0 + 10.0 || across.abs() > a.width / 2.0 + 30.0, "something at {along:.0}, {across:.0} from the runway");
                        if is_tree { trees_seen += 1 } else { buildings += 1 }
                    }
                }
            }
        }
        // The aerodrome has its hangars, office and parked aircraft, and trees around it.
        assert!(buildings >= 4, "{buildings} buildings at the aerodrome");
        assert!(trees_seen > 0);
    }

    #[test]
    fn hedges_stand_on_field_boundaries() {
        // `field_boundary` must find the same lines `terrain_fragment` paints: squares of 256 m cut
        // into strips. Its answer, followed, lands on a boundary, and the nearest one.
        for k in 0..2000 {
            let (x, z) = (hash2(k, 1, 5) * 30_000.0 - 15_000.0, hash2(k, 2, 5) * 30_000.0 - 15_000.0);
            let ((dx, dz), _) = field_boundary(x, z);
            assert!(dx == 0.0 || dz == 0.0);
            let (bx, bz) = (x + dx, z + dz);
            // On the boundary, every coordinate is a multiple of 256 or of a strip width across it.
            let (wx, wz) = (bx.rem_euclid(16384.0), bz.rem_euclid(16384.0));
            let (cx, cz) = ((wx / 256.0).floor(), (wz / 256.0).floor());
            let (ix, iz) = (cx as i32 & 63, cz as i32 & 63);
            let along_x = ihash(ix, iz) > 0.5;
            let strips = 2.0 + (ihash(ix + 91, iz + 91) * 3.0).floor() as f64;
            let on = |v: f64, step: f64| {
                let r = v.rem_euclid(step);
                r.min(step - r) < 1e-6
            };
            let across = if along_x { wx } else { wz };
            let other = if along_x { wz } else { wx };
            let ok = on(across, 256.0 / strips) || on(other, 256.0) || on(wx, 256.0) || on(wz, 256.0);
            assert!(ok, "({x:.1}, {z:.1}) + ({dx:.1}, {dz:.1}) is not on a boundary");
            assert!(dx.abs().max(dz.abs()) <= 128.0);
        }
    }
}

#[cfg(test)]
mod farm_tests {
    use super::*;

    #[test]
    fn farms_stand_on_level_farmland_and_their_buildings_do_not_overlap() {
        let terrain = Terrain::new(1);
        let home = terrain.nearest_airport(0.0, 0.0).unwrap();
        // The squares of farmland in 12 km around the home aerodrome.
        let (cx, cz) = ((home.e / FIELD) as i64, (-home.n / FIELD) as i64);
        let mut found = Vec::new();
        for i in -24..24 {
            for j in -24..24 {
                found.extend(farm(&terrain, (cx + i, cz + j)));
            }
        }
        assert!(found.len() > 10, "{} farms around the home aerodrome", found.len());
        for f in &found {
            // Footprints of the buildings and crowns of the trees, as boxes (u0, u1, v0, v1) in the yard.
            let mut boxes: Vec<(f64, f64, f64, f64)> = Vec::new();
            for (kind, u, v, across, scale) in f.buildings() {
                let (hu, hv) = match kind {
                    Building::Cottage => (4.95, 3.95),
                    Building::HipHouse => (5.45, 4.95),
                    Building::Shed => (12.45, 5.95),
                    _ => unreachable!(),
                };
                let (hu, hv) = if across { (hv * scale, hu * scale) } else { (hu * scale, hv * scale) };
                boxes.push((u - hu, u + hu, v - hv, v + hv));
            }
            for (_, u, v, height) in f.trees() {
                let r = height * 0.37 * 1.25;
                boxes.push((u - r, u + r, v - r, v + r));
            }
            for (i, a) in boxes.iter().enumerate() {
                assert!(a.0 > -YARD && a.1 < YARD && a.2 > -YARD && a.3 < YARD, "{a:?} out of the yard");
                for b in &boxes[i + 1..] {
                    assert!(a.1 <= b.0 || b.1 <= a.0 || a.3 <= b.2 || b.3 <= a.2, "{a:?} overlaps {b:?}");
                }
            }
            // Clear of the hedges on the sides of the square.
            let (wx, wz) = (f.x.rem_euclid(FIELD), f.z.rem_euclid(FIELD));
            assert!(wx.min(FIELD - wx) > YARD + 10.0 && wz.min(FIELD - wz) > YARD + 10.0);
        }
        // The same farm is found from every tile that reaches it, and its buildings are in exactly one.
        let f = found[0];
        let key = ((f.x / 512.0).floor() as i32, (f.z / 512.0).floor() as i32);
        let near: Vec<Farm> = farms(&terrain, key.0 as f64 * 512.0, key.1 as f64 * 512.0, 512.0);
        assert!(near.contains(&f));
        let mut count = 0;
        for i in -1..=1 {
            for j in -1..=1 {
                let k = (key.0 + i, key.1 + j);
                let list = houses(&terrain, k, 512.0);
                count += list.iter().flat_map(|l| l.as_chunks::<8>().0).filter(|v| f.holds(v[0] as f64 + k.0 as f64 * 512.0, v[2] as f64 + k.1 as f64 * 512.0, 0.0)).count();
            }
        }
        assert_eq!(count, f.buildings().len());
    }
}
