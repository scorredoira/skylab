//! The terrain: ground height, rivers and lakes, airports, towns, roads and land cover, from one of
//! two sources. A seed defines an infinite world (the default); a `Region` gives a real place that
//! `geodata` built from maps before the world was made (seam 3 of `docs/ROADMAP.md`). The queries
//! are the same for both.
//!
//! Every query is a pure function of position and of the seed or the region. Generated airports,
//! towns and roads are found per grid cell and cached behind a mutex, so a `Terrain` can be shared
//! between threads.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex, MutexGuard};

use glam::{DVec3, Vec3};

use crate::circuit::Circuit;
use crate::lights::Class;
use crate::math::{clamp, fbm, hash2, lerp, noised, smoothstep, DEG};
use crate::region::{Cover, Footprint, Region};
use crate::scenery;
use crate::solids::{self, Solids};

/// Grid cell (north index, east index) of the airport or the town grid.
pub type Cell = (i32, i32);

/// An aerodrome's runway, level on flattened ground: the one of a generated aerodrome, or one of
/// the runways of a real one (`Region::airports`; they share its `id`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Airport {
    pub id: u64,
    pub n: f64,
    pub e: f64,
    /// Runway direction, radians from north, in [0, π).
    pub heading: f64,
    pub elevation: f64,
    pub length: f64,
    pub width: f64,
    /// What it is made of and what it has beside it.
    pub detail: Detail,
}

/// What a runway is made of and what stands beside it. Every generated aerodrome is
/// `Detail::GENERATED`; a real one's comes from its data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detail {
    /// Asphalt or concrete, painted; otherwise grass, earth or gravel, with no paint.
    pub paved: bool,
    /// Which lights it has; `None` lets the id decide (`lights::Class::of`).
    pub lighting: Option<Class>,
    /// Which side of the runway its apron, taxiway, hangars, office, windsock and floodlights are
    /// on: 1 to the right of `heading`, -1 to the left, 0 none (another runway of the aerodrome has them).
    pub apron: f64,
}

impl Detail {
    pub const GENERATED: Detail = Detail { paved: true, lighting: None, apron: 1.0 };
}

impl Airport {
    /// Runway number when taking off along `heading` (the opposite end is +18).
    pub fn number(&self) -> i32 {
        let k = (self.heading / DEG / 10.0).round() as i32;
        if k == 0 {
            36
        } else {
            k
        }
    }

    /// Position (along, across) the runway of a point, in metres.
    #[inline(always)]
    pub fn local(&self, pn: f64, pe: f64) -> (f64, f64) {
        let dn = pn - self.n;
        let de = pe - self.e;
        let c = self.heading.cos();
        let s = self.heading.sin();
        (dn * c + de * s, -dn * s + de * c)
    }

    /// How far from its middle the runway changes the ground (`flat_weight`): `Terrain::AIRPORT_REACH`
    /// for the generated ones, more for a long real one.
    pub fn reach(&self) -> f64 {
        Terrain::AIRPORT_REACH.max(self.length / 2.0 + 120.0 + 450.0 + self.width)
    }

    /// 1 on the flattened area, falling to 0 over the blend distance.
    #[inline(always)]
    pub fn flat_weight(&self, pn: f64, pe: f64) -> f64 {
        let (along, across) = self.local(pn, pe);
        let dx = (along.abs() - (self.length / 2.0 + 120.0)).max(0.0);
        let dy = (across.abs() - (self.width / 2.0 + 70.0)).max(0.0);
        let d = (dx * dx + dy * dy).sqrt();
        if d >= 450.0 {
            return 0.0;
        }
        1.0 - smoothstep(0.0, 450.0, d)
    }

    pub fn on_runway(&self, pn: f64, pe: f64) -> bool {
        let (along, across) = self.local(pn, pe);
        along.abs() <= self.length / 2.0 && across.abs() <= self.width / 2.0
    }

    /// The ground the runway keeps clear, as rectangles in its axes (along from, to; across from,
    /// to, metres): the paved strip with a margin round it (`CLEAR_STRIP`), and on the apron's
    /// side (`Detail::apron`) the taxiway, the apron, the hangars, the office and the parked
    /// aircraft, as `lights` and `scenery` lay them.
    pub fn clear_ground(&self) -> Vec<[f64; 4]> {
        let (half, hw) = (self.length / 2.0 + CLEAR_STRIP.0, self.width / 2.0);
        let mut areas = vec![[-half, half, -hw - CLEAR_STRIP.1, hw + CLEAR_STRIP.1]];
        let side = self.detail.apron;
        if side != 0.0 {
            let (near, far) = (hw * side, (hw + CLEAR_APRON.2) * side);
            areas.push([CLEAR_APRON.0, CLEAR_APRON.1, near.min(far), near.max(far)]);
        }
        areas
    }

    /// Whether an outline (north, east corners, the first not repeated) stands on the ground the
    /// runway keeps clear (`clear_ground`), even in part: a corner inside, a side across an edge,
    /// or the whole of it round it.
    pub fn blocks(&self, ring: &[[f32; 2]]) -> bool {
        if ring.is_empty() {
            return false;
        }
        let local: Vec<(f64, f64)> = ring.iter().map(|p| self.local(p[0] as f64, p[1] as f64)).collect();
        self.clear_ground().iter().any(|&[a0, a1, c0, c1]| {
            let inside = |(a, c): (f64, f64)| a >= a0 && a <= a1 && c >= c0 && c <= c1;
            // A side crosses the rectangle where the part of it between both pairs of edges is not empty.
            let crosses = |(pa, pc): (f64, f64), (qa, qc): (f64, f64)| {
                let (mut t0, mut t1) = (0.0f64, 1.0f64);
                for (p, d, lo, hi) in [(pa, qa - pa, a0, a1), (pc, qc - pc, c0, c1)] {
                    if d.abs() < 1e-12 {
                        if p < lo || p > hi {
                            return false;
                        }
                    } else {
                        let (u, v) = ((lo - p) / d, (hi - p) / d);
                        t0 = t0.max(u.min(v));
                        t1 = t1.min(u.max(v));
                    }
                }
                t0 <= t1
            };
            let n = local.len();
            let middle = ((a0 + a1) / 2.0, (c0 + c1) / 2.0);
            let round = {
                let mut odd = false;
                for i in 0..n {
                    let ((pa, pc), (qa, qc)) = (local[i], local[(i + 1) % n]);
                    if (pc > middle.1) != (qc > middle.1) && middle.0 < pa + (middle.1 - pc) / (qc - pc) * (qa - pa) {
                        odd = !odd;
                    }
                }
                odd
            };
            local.iter().any(|&p| inside(p)) || (0..n).any(|i| crosses(local[i], local[(i + 1) % n])) || round
        })
    }
}

/// The margin round a runway's paved strip kept clear of buildings: metres beyond its ends and
/// beyond its sides. A real runway's strip is wider (75 to 150 m each side of the centre line), but
/// the tables place it within some metres; nothing of the maps stands this near.
pub const CLEAR_STRIP: (f64, f64) = (60.0, 25.0);
/// The ground of the taxiway, apron, hangars, office and parked aircraft beside a runway
/// (`Detail::apron`): along it from and to (metres from its middle), and how far it reaches
/// beyond its edge.
pub const CLEAR_APRON: (f64, f64, f64) = (-115.0, 75.0, 135.0);

/// Colour and cover of a point of the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub color: Vec3,
    pub farm: f32,
    pub water: f32,
    pub forest: f32,
}

/// Ground before airports flatten it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Raw {
    pub h: f64,
    pub river: f64,
    pub valley: f64,
}

/// A lake of a generated world: water standing at `level` over the floor of a river's valley,
/// within an oval round its middle (`length` along `heading`, `width` across, metres, its shore
/// bent by a noise), as far as the ground is lower. The river flows into it and out over its
/// lower end, where the ground rises to its level (a sill) over `SILL` metres beyond the oval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lake {
    pub n: f64,
    pub e: f64,
    /// Height of its water above the sea, metres.
    pub level: f64,
    pub length: f64,
    pub width: f64,
    /// Direction of its length, radians from north, in [0, π).
    pub heading: f64,
}

impl Lake {
    /// Metres beyond the oval over which the ground rises to the lake's level.
    pub const SILL: f64 = 300.0;

    /// How far from its middle it changes the ground.
    pub fn reach(&self) -> f64 {
        self.length.max(self.width) * 1.25 + Lake::SILL
    }

    /// 1 inside its oval (its shore bent by up to a fifth), falling to 0 over `SILL` beyond.
    pub fn weight(&self, n: f64, e: f64, seed: u64) -> f64 {
        let (dn, de) = (n - self.n, e - self.e);
        let (c, s) = (self.heading.cos(), self.heading.sin());
        let (along, across) = ((dn * c + de * s) / self.length, (-dn * s + de * c) / self.width);
        let bend = 0.2 * fbm(n / 700.0, e / 700.0, 2, seed.wrapping_add(6100));
        let d = (along * along + across * across).sqrt() * self.width - (1.0 + bend) * self.width;
        1.0 - smoothstep(0.0, Lake::SILL, d)
    }

    /// Fills its basin at a point of the dry ground: inside, what is lower than its level is water
    /// at its level (deeper by how much lower the ground was); round it, the ground rises towards it.
    fn fill(&self, r: &mut Raw, n: f64, e: f64, seed: u64) {
        if r.h >= self.level || (n - self.n).abs().max((e - self.e).abs()) > self.reach() {
            return;
        }
        let w = self.weight(n, e, seed) * smoothstep(0.0, 0.25, r.valley);
        if w <= 0.0 {
            return;
        }
        let depth = self.level - r.h;
        r.h = lerp(r.h, self.level, w);
        // Water where the basin is full: shallow at the shore, all of it a metre in.
        r.river = r.river.max(smoothstep(0.98, 1.0, w) * smoothstep(0.0, 1.0, depth));
    }
}

/// A built-up area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Town {
    pub n: f64,
    pub e: f64,
    pub radius: f64,
    /// Direction of its street grid, radians.
    pub grid: f64,
}

impl Town {
    /// 1 in the built-up area, 0 outside.
    #[inline(always)]
    pub fn weight(&self, pn: f64, pe: f64) -> f64 {
        let d = (pn - self.n).hypot(pe - self.e);
        if d > self.radius * 1.15 {
            return 0.0;
        }
        1.0 - smoothstep(self.radius * 0.75, self.radius * 1.15, d)
    }
}

/// A straight road: between two towns of a generated world, or a stretch of a real one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Road {
    pub n0: f64,
    pub e0: f64,
    pub n1: f64,
    pub e1: f64,
    /// From kerb to kerb, metres.
    pub width: f64,
}

impl Road {
    /// The width of the roads between the towns of a generated world.
    pub const GENERATED_WIDTH: f64 = 7.0;

    /// Distance from a point (north, east) to the middle of the road.
    pub fn distance(&self, n: f64, e: f64) -> f64 {
        let (dn, de) = (self.n1 - self.n0, self.e1 - self.e0);
        let t = (((n - self.n0) * dn + (e - self.e0) * de) / (dn * dn + de * de)).clamp(0.0, 1.0);
        (n - (self.n0 + dn * t)).hypot(e - (self.e0 + de * t))
    }
}

/// Final ground at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    pub h: f64,
    pub river: f64,
    /// 1 on flattened airport ground.
    pub flat: f64,
}

#[derive(Default)]
struct Caches {
    airports: HashMap<Cell, Option<Airport>>,
    towns: HashMap<Cell, Option<Town>>,
    lakes: HashMap<Cell, Option<Lake>>,
    roads: HashMap<(i32, i32, i32), Option<Road>>,
}

/// The terrain of a seed or of a region: every query is a pure function of position.
pub struct Terrain {
    seed: u64,
    caches: Mutex<Caches>,
    /// A real place, when the terrain is one; the seed still decides the details the maps do not
    /// give (which tree stands where, the fields, the houses).
    place: Option<Place>,
    /// Race circuits laid on it (`with_circuits`): the ground is cut and filled to them.
    circuits: Vec<Arc<Circuit>>,
}

/// A region and the index of its roads by square, made once.
struct Place {
    region: Arc<Region>,
    /// The roads (indices into `region.roads`) that reach into each square of `ROAD_SQUARE`.
    roads: HashMap<Cell, Vec<u32>>,
    /// The buildings (indices into `region.buildings`) whose middle is in each square of `BUILDING_SQUARE`.
    buildings: HashMap<Cell, Vec<u32>>,
    /// The tallest of them, metres.
    tallest: f64,
    /// How much of the nine squares of `BUILDING_SQUARE` round each the buildings whose middle is
    /// in them cover, 0 to 1: how built up the place is there (`Terrain::built_share`).
    built: HashMap<Cell, f32>,
    /// The buildings and the bridges as solids (`solids`), made once the terrain is.
    solids: Solids,
}

/// Between what shares of the ground under buildings (`Terrain::built_share`) a region's ground
/// turns from the seed's into a city's the maps draw (`Terrain::urban`). A city's blocks are
/// 0.2 to 0.5, a rail yard or a park among them 0.08 to 0.2, its suburbs 0.05 to 0.15; the country,
/// a hamlet or a farm's sheds under 0.02.
const URBAN: (f64, f64) = (0.03, 0.07);
/// Side of the squares a region's roads are found by, metres.
const ROAD_SQUARE: f64 = 1000.0;
/// Side of the squares a region's buildings are found by, metres.
const BUILDING_SQUARE: f64 = 256.0;
/// The most a building of a region reaches from its middle, metres, as obstacles are looked for (an
/// aerodrome's terminal); `building_near` looks for those that reach less, `NEAR_REACH`.
pub const BUILDING_REACH: f64 = 320.0;
const NEAR_REACH: f64 = 120.0;

impl Place {
    fn new(region: Arc<Region>) -> Place {
        let mut roads: HashMap<Cell, Vec<u32>> = HashMap::new();
        for (i, r) in region.roads.iter().enumerate() {
            let pad = r.width / 2.0;
            let square = |v: f64| (v / ROAD_SQUARE).floor() as i32;
            for x in square(r.n0.min(r.n1) - pad)..=square(r.n0.max(r.n1) + pad) {
                for y in square(r.e0.min(r.e1) - pad)..=square(r.e0.max(r.e1) + pad) {
                    roads.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        let square = |v: f64| (v / BUILDING_SQUARE).floor() as i32;
        // Not those on a runway, its strip or its apron (`Airport::clear_ground`): the aerodrome's
        // ground is levelled under them, and what the maps draw there stands under a runway on
        // pillars (Madeira's) or was built before it.
        let mut buildings: HashMap<Cell, Vec<u32>> = HashMap::new();
        let mut tallest = 0.0f64;
        let mut built: HashMap<Cell, f32> = HashMap::new();
        for (i, b) in region.buildings.iter().enumerate() {
            let (n, e) = b.centre();
            let near = |a: &&Airport| (a.n - n).abs().max((a.e - e).abs()) < a.length / 2.0 + CLEAR_STRIP.0 + CLEAR_APRON.2 + a.width + BUILDING_REACH;
            if region.airports.iter().filter(near).any(|a| a.blocks(&b.ring)) {
                continue;
            }
            buildings.entry((square(n), square(e))).or_default().push(i as u32);
            tallest = tallest.max(b.height as f64);
            // A part raised on others (a tower on its podium) covers no more ground.
            if b.floor <= 0.0 {
                *built.entry((square(n), square(e))).or_default() += (b.area() / (BUILDING_SQUARE * BUILDING_SQUARE)) as f32;
            }
        }
        // Each square's share is the mean of the nine round it: a rail yard or a junction among
        // the blocks is in the city, a farm's sheds are not a town.
        let mut smoothed: HashMap<Cell, f32> = HashMap::new();
        for (&(x, y), &share) in &built {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    *smoothed.entry((x + dx, y + dy)).or_default() += share.min(1.0) / 9.0;
                }
            }
        }
        Place { region, roads, buildings, tallest, built: smoothed, solids: Solids::default() }
    }
}

impl Terrain {
    /// Side of the airport grid: at most one airport per cell.
    pub const CELL_SIZE: f64 = 10_000.0;
    /// How far an airport can change the ground around its centre.
    pub const AIRPORT_REACH: f64 = 1_300.0;
    /// The runway of a generated international airport: its length and width, m.
    pub const LONG_RUNWAY: (f64, f64) = (3_300.0, 60.0);
    /// Side of the town grid: at most one town per cell.
    pub const TOWN_CELL: f64 = 7_000.0;
    /// Side of the lake grid: at most one lake per cell; and the share of cells that try for one.
    pub const LAKE_CELL: f64 = 12_000.0;
    const LAKE_SHARE: f64 = 0.55;

    /// The infinite world of a seed.
    pub fn new(seed: u64) -> Terrain {
        Terrain {
            seed,
            caches: Mutex::new(Caches::default()),
            place: None,
            circuits: Vec::new(),
        }
    }

    /// A real place; `seed` decides what its maps do not say.
    pub fn from_region(seed: u64, region: Arc<Region>) -> Terrain {
        let mut terrain = Terrain {
            seed,
            caches: Mutex::new(Caches::default()),
            place: Some(Place::new(region)),
            circuits: Vec::new(),
        };
        // The solids stand on the terrain's ground (an aerodrome's levelled where it is), so they
        // are made once it is.
        if let Some(place) = &terrain.place {
            let mut kept: Vec<u32> = place.buildings.values().flatten().copied().collect();
            kept.sort_unstable();
            let region = place.region.clone();
            let solids = Solids::new(&region, &kept, &|b| scenery::mapped_base(&terrain, b), &|n, e| region.height(n, e));
            if let Some(place) = &mut terrain.place {
                place.tallest = place.tallest.max(solids.tallest());
                place.solids = solids;
            }
        }
        terrain
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The same terrain with race circuits laid on it: under and round each the ground is cut and
    /// filled to its surface (`Circuit::earthwork`), and nothing grows or is built there (`Ground::flat`).
    /// The circuits are laid out on the terrain without them (`Circuit::new`).
    pub fn with_circuits(mut self, circuits: Vec<Arc<Circuit>>) -> Terrain {
        self.circuits = circuits;
        self
    }

    /// The race circuits laid on it.
    pub fn circuits(&self) -> &[Arc<Circuit>] {
        &self.circuits
    }

    /// The real place this terrain is, if it is one.
    pub fn region(&self) -> Option<&Region> {
        self.place.as_ref().map(|p| &*p.region)
    }

    /// The caches only ever hold finished values, so a panic elsewhere cannot leave them broken.
    /// Web: the page's thread may not wait for a lock (the browser stops the program), and the
    /// workers ask the terrain too: it spins the moment another thread holds them.
    fn caches(&self) -> MutexGuard<'_, Caches> {
        #[cfg(target_arch = "wasm32")]
        loop {
            match self.caches.try_lock() {
                Ok(caches) => return caches,
                Err(std::sync::TryLockError::Poisoned(e)) => return e.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => std::hint::spin_loop(),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.caches.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Ground height before airports flatten it, and how much of the point is inland water (a river
    /// or a lake). In a region the water is its maps' (lakes and wide rivers), and there are no
    /// valleys; in a generated world the lakes fill basins of its river valleys (`Lake`).
    pub fn raw(&self, n: f64, e: f64) -> Raw {
        if let Some(place) = &self.place {
            let region = &place.region;
            return Raw { h: region.height(n, e), river: region.shares(n, e)[Cover::Water as usize], valley: 0.0 };
        }
        let mut r = self.dry(n, e);
        if r.valley <= 0.0 {
            return r;
        }
        let (x, y) = ((n / Terrain::LAKE_CELL).floor() as i32, (e / Terrain::LAKE_CELL).floor() as i32);
        for cell in [(x, y), (x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1), (x + 1, y + 1), (x - 1, y - 1), (x + 1, y - 1), (x - 1, y + 1)] {
            if let Some(lake) = self.lake(cell) {
                lake.fill(&mut r, n, e, self.seed);
            }
        }
        r
    }

    /// The generated ground before its lakes: what airports, towns and roads are placed on, so that
    /// the lakes, placed away from them, change none of them.
    fn dry(&self, n: f64, e: f64) -> Raw {
        let s = self.seed;
        let wx = n + 2200.0 * fbm(n / 11000.0, e / 11000.0, 3, s.wrapping_add(11));
        let wy = e + 2200.0
            * fbm(
                n / 11000.0 + 31.7,
                e / 11000.0 - 12.3,
                3,
                s.wrapping_add(23),
            );

        let cont = fbm(wx / 52000.0, wy / 52000.0, 4, s.wrapping_add(37));
        let base = 90.0 + cont * 420.0;
        let land = smoothstep(-5.0, 80.0, base);

        // Mountains: fractal noise damped by its own accumulated slope, which
        // leaves smooth valley floors and sharp ridges like eroded relief.
        let mask = smoothstep(
            -0.05,
            0.35,
            fbm(
                wx / 34000.0 + 5.1,
                wy / 34000.0 + 9.2,
                3,
                s.wrapping_add(41),
            ),
        ) * land;
        let mut mountains = 0.0;
        if mask > 0.001 {
            let (mut a, mut b, mut dx, mut dy) = (0.0, 1.0, 0.0, 0.0);
            let mut px = wx / 5200.0;
            let mut py = wy / 5200.0;
            for i in 0..9u64 {
                let nz = noised(px, py, s.wrapping_add(100).wrapping_add(i));
                dx += nz.dx * 0.5;
                dy += nz.dy * 0.5;
                a += b * (nz.v * 0.5 + 0.5) / (1.0 + dx * dx + dy * dy);
                b *= 0.5;
                let nx = 1.6 * px - 1.2 * py;
                let ny = 1.2 * px + 1.6 * py;
                px = nx;
                py = ny;
            }
            // Ridged noise adds the sharp crest lines; the eroded base keeps the massif broad instead of a cone.
            let r1 = 1.0
                - noised(wx / 2600.0 + 3.3, wy / 2600.0 - 1.7, s.wrapping_add(200))
                    .v
                    .abs();
            let r2 = 1.0
                - noised(wx / 1100.0 - 5.1, wy / 1100.0 + 2.9, s.wrapping_add(201))
                    .v
                    .abs();
            let ridge = r1 * r1 * 0.7 + r2 * r2 * r1 * 0.3;
            mountains = mask * ((0.7 * a + 0.3 * a * a) * 1250.0 + ridge * a * mask * 420.0);
        }

        let hill_amp = 6.0
            + 55.0
                * smoothstep(
                    -0.2,
                    0.4,
                    fbm(n / 16000.0, e / 16000.0, 2, s.wrapping_add(53)),
                );
        let hills = fbm(n / 1700.0, e / 1700.0, 5, s.wrapping_add(59)) * hill_amp * land;

        let mut h = base + mountains + hills;

        // Rivers follow the zero lines of a low-frequency noise.
        let r = fbm(
            wx / 15000.0 + 77.0,
            wy / 15000.0 - 41.0,
            3,
            s.wrapping_add(71),
        )
        .abs();
        let inland = smoothstep(2.0, 25.0, base);
        let (mut valley, mut river) = (0.0, 0.0);
        if r < 0.045 && inland > 0.0 {
            valley = (1.0 - smoothstep(0.0, 0.045, r)) * inland;
            let bed = base - 3.0;
            h = lerp(h, bed + (h - bed) * 0.2, valley);
            h = lerp(h, bed, (1.0 - smoothstep(0.003, 0.014, r)) * inland);
            river = (1.0 - smoothstep(0.0028, 0.0042, r)) * inland;
        }
        Raw { h, river, valley }
    }

    // MARK: Airports

    /// Whether a cell's aerodrome is an international airport, if the ground lets it: one cell in
    /// seven, and none within three cells of the origin, where the aerodromes are small ones.
    fn international(cell: Cell) -> bool {
        (cell.0.abs() > 3 || cell.1.abs() > 3) && (3 * cell.0 as i64 + 5 * cell.1 as i64).rem_euclid(7) == 0
    }

    /// An international airport in a cell, where its ground is flat enough along 4 km: a runway
    /// of `Terrain::LONG_RUNWAY`.
    fn find_international(&self, cell: Cell) -> Option<Airport> {
        let cs = Terrain::CELL_SIZE;
        let (cx, cy) = (cell.0 as i64, cell.1 as i64);
        let (length, width) = Terrain::LONG_RUNWAY;
        for attempt in 0..6u64 {
            let k = self.seed.wrapping_add(1500).wrapping_add(attempt.wrapping_mul(7919));
            let cn = (cell.0 as f64 + 0.25 + 0.5 * hash2(cx, cy, k)) * cs;
            let ce = (cell.1 as f64 + 0.25 + 0.5 * hash2(cx, cy, k.wrapping_add(1))) * cs;
            // (height range, heading, elevation) of the flattest direction found.
            let mut best: Option<(f64, f64, f64)> = None;
            for hi in 0..6 {
                let heading = hi as f64 * 30.0 * DEG + 0.4 * hash2(cx, cy, k.wrapping_add(2));
                let (c, s) = (heading.cos(), heading.sin());
                let (mut lo, mut high, mut sum, mut count) = (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0.0);
                let mut ok = true;
                'strip: for ia in 0..=20 {
                    let a = (ia as f64 / 20.0 - 0.5) * (length + 800.0);
                    for b in [-220.0, 0.0, 220.0] {
                        let p = self.dry(cn + a * c - b * s, ce + a * s + b * c);
                        if p.valley > 0.0 || p.h < 5.0 {
                            ok = false;
                            break 'strip;
                        }
                        lo = lo.min(p.h);
                        high = high.max(p.h);
                        if b == 0.0 {
                            sum += p.h;
                            count += 1.0;
                        }
                    }
                }
                let range = high - lo;
                if ok && range < 30.0 && range < best.map_or(f64::INFINITY, |b| b.0) {
                    best = Some((range, heading % PI, sum / count));
                }
            }
            if let Some((_, heading, elevation)) = best {
                let id = (hash2(cx, cy, self.seed.wrapping_add(5)) * 1e9) as u64;
                return Some(Airport { id, n: cn, e: ce, heading, elevation, length, width, detail: Detail::GENERATED });
            }
        }
        None
    }

    fn find_airport(&self, cell: Cell) -> Option<Airport> {
        if Terrain::international(cell) {
            if let Some(airport) = self.find_international(cell) {
                return Some(airport);
            }
        }
        let cs = Terrain::CELL_SIZE;
        let cx = cell.0 as i64;
        let cy = cell.1 as i64;
        for attempt in 0..4u64 {
            let k = self
                .seed
                .wrapping_add(1000)
                .wrapping_add(attempt.wrapping_mul(7919));
            let cn = (cell.0 as f64 + 0.15 + 0.7 * hash2(cx, cy, k)) * cs;
            let ce = (cell.1 as f64 + 0.15 + 0.7 * hash2(cx, cy, k.wrapping_add(1))) * cs;
            let centre = self.dry(cn, ce);
            if centre.h < 6.0 || centre.valley > 0.0 {
                continue;
            }

            // (height range, heading, elevation) of the flattest direction found.
            let mut best: Option<(f64, f64, f64)> = None;
            for hi in 0..6 {
                let heading = hi as f64 * 30.0 * DEG + 0.4 * hash2(cx, cy, k.wrapping_add(2));
                let c = heading.cos();
                let s = heading.sin();
                let (mut lo, mut hi2, mut sum, mut count) =
                    (f64::INFINITY, f64::NEG_INFINITY, 0.0, 0.0);
                let mut ok = true;
                'outer: for ia in 0..=10 {
                    let a = -700.0 + ia as f64 * 140.0;
                    for b in [-140.0, 0.0, 140.0] {
                        let p = self.dry(cn + a * c - b * s, ce + a * s + b * c);
                        if p.valley > 0.0 || p.h < 5.0 {
                            ok = false;
                            break 'outer;
                        }
                        lo = lo.min(p.h);
                        hi2 = hi2.max(p.h);
                        if b == 0.0 {
                            sum += p.h;
                            count += 1.0;
                        }
                    }
                }
                if !ok {
                    continue;
                }
                let range = hi2 - lo;
                if range < 14.0 && range < best.map_or(f64::INFINITY, |b| b.0) {
                    best = Some((range, heading % PI, sum / count));
                }
            }
            if let Some((_, heading, elevation)) = best {
                let id = (hash2(cx, cy, self.seed.wrapping_add(5)) * 1e9) as u64;
                return Some(Airport {
                    id,
                    n: cn,
                    e: ce,
                    heading,
                    elevation,
                    length: 1000.0,
                    width: 24.0,
                    detail: Detail::GENERATED,
                });
            }
        }
        None
    }

    /// The airport of a cell of the airport grid, if it has one.
    pub fn airport(&self, cell: Cell) -> Option<Airport> {
        if let Some(place) = &self.place {
            let cs = Terrain::CELL_SIZE;
            return place.region.airports.iter().find(|a| ((a.n / cs).floor() as i32, (a.e / cs).floor() as i32) == cell).copied();
        }
        if let Some(hit) = self.caches().airports.get(&cell) {
            return *hit;
        }
        let found = self.find_airport(cell);
        self.caches().airports.insert(cell, found);
        found
    }

    /// Airports that can affect the ground anywhere in the given rectangle.
    pub fn airports(&self, min_n: f64, max_n: f64, min_e: f64, max_e: f64) -> Vec<Airport> {
        let cs = Terrain::CELL_SIZE;
        let reach = Terrain::AIRPORT_REACH;
        if let Some(place) = &self.place {
            return place
                .region
                .airports
                .iter()
                .filter(|a| {
                    let reach = a.reach();
                    a.n > min_n - reach && a.n < max_n + reach && a.e > min_e - reach && a.e < max_e + reach
                })
                .copied()
                .collect();
        }
        // An international airport reaches further than the others (`Airport::reach`).
        let far = reach.max(Terrain::LONG_RUNWAY.0 / 2.0 + 120.0 + 450.0 + Terrain::LONG_RUNWAY.1);
        let x0 = ((min_n - far) / cs).floor() as i32;
        let x1 = ((max_n + far) / cs).floor() as i32;
        let y0 = ((min_e - far) / cs).floor() as i32;
        let y1 = ((max_e + far) / cs).floor() as i32;
        let mut out = Vec::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(a) = self.airport((x, y)) {
                    let reach = a.reach();
                    if a.n > min_n - reach
                        && a.n < max_n + reach
                        && a.e > min_e - reach
                        && a.e < max_e + reach
                    {
                        out.push(a);
                    }
                }
            }
        }
        out
    }

    /// Nearest airport, searching outwards cell by cell up to 12 rings (120 km).
    pub fn nearest_airport(&self, n: f64, e: f64) -> Option<Airport> {
        self.nearest_airport_within(n, e, 12)
    }

    /// The nearest airport with a runway at least `length` long, up to 250 km away; if there is
    /// none (a real place without one), the one with the longest runway there is near, or else
    /// the nearest. With no length asked it is `nearest_airport`.
    pub fn nearest_runway(&self, n: f64, e: f64, length: f64) -> Option<Airport> {
        if length <= 0.0 {
            return self.nearest_airport(n, e);
        }
        let distance = |a: &Airport| (a.n - n).hypot(a.e - e);
        let cs = Terrain::CELL_SIZE;
        let near: Vec<Airport> = match &self.place {
            Some(place) => place.region.airports.clone(),
            None => {
                let c0 = ((n / cs).floor() as i32, (e / cs).floor() as i32);
                let mut found = Vec::new();
                // Ring by ring, until one is found and no nearer one can be further out.
                for ring in 0..=25i32 {
                    if found.iter().any(|a| distance(a) < (ring - 1) as f64 * cs) {
                        break;
                    }
                    for dx in -ring..=ring {
                        for dy in -ring..=ring {
                            if dx.abs().max(dy.abs()) == ring && Terrain::international((c0.0 + dx, c0.1 + dy)) {
                                found.extend(self.airport((c0.0 + dx, c0.1 + dy)).filter(|a| a.length >= length));
                            }
                        }
                    }
                }
                found
            }
        };
        let long = near.iter().filter(|a| a.length >= length).min_by(|a, b| distance(a).total_cmp(&distance(b))).copied();
        long.or_else(|| near.iter().max_by(|a, b| a.length.total_cmp(&b.length)).copied()).or_else(|| self.nearest_airport(n, e))
    }

    /// Nearest airport, searching outwards cell by cell.
    pub fn nearest_airport_within(&self, n: f64, e: f64, max_rings: i32) -> Option<Airport> {
        let cs = Terrain::CELL_SIZE;
        if let Some(place) = &self.place {
            // As far as the rings of cells would have looked.
            let most = (max_rings as f64 + 1.0) * cs;
            let distance = |a: &Airport| (a.n - n).hypot(a.e - e);
            return place.region.airports.iter().filter(|a| distance(a) <= most).min_by(|a, b| distance(a).total_cmp(&distance(b))).copied();
        }
        let c0 = ((n / cs).floor() as i32, (e / cs).floor() as i32);
        let mut best: Option<Airport> = None;
        let mut best_d = f64::INFINITY;
        for ring in 0..=max_rings {
            if best.is_some() && (ring - 1) as f64 * cs > best_d {
                break;
            }
            let r = ring;
            for dx in -r..=r {
                for dy in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    if let Some(a) = self.airport((c0.0 + dx, c0.1 + dy)) {
                        let d = (a.n - n).hypot(a.e - e);
                        if d < best_d {
                            best_d = d;
                            best = Some(a);
                        }
                    }
                }
            }
        }
        best
    }

    // MARK: Towns and roads

    fn find_town(&self, cell: Cell) -> Option<Town> {
        let cs = Terrain::TOWN_CELL;
        let cx = cell.0 as i64;
        let cy = cell.1 as i64;
        let k = self.seed.wrapping_add(4000);
        if hash2(cx, cy, k) > 0.75 {
            return None;
        }
        let n = (cell.0 as f64 + 0.2 + 0.6 * hash2(cx, cy, k.wrapping_add(1))) * cs;
        let e = (cell.1 as f64 + 0.2 + 0.6 * hash2(cx, cy, k.wrapping_add(2))) * cs;
        let radius =
            220.0 + 420.0 * hash2(cx, cy, k.wrapping_add(3)) * hash2(cx, cy, k.wrapping_add(4));
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let around = [
            (0.0, 0.0),
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (0.7, 0.7),
            (-0.7, -0.7),
            (0.7, -0.7),
            (-0.7, 0.7),
        ];
        for (dn, de) in around {
            let p = self.dry(n + dn * radius, e + de * radius);
            if p.valley > 0.6 || p.h < 6.0 || p.h > 520.0 {
                return None;
            }
            lo = lo.min(p.h);
            hi = hi.max(p.h);
        }
        if hi - lo > 0.09 * radius {
            return None;
        }
        for a in self.airports(n, n, e, e) {
            if (a.n - n).hypot(a.e - e) < 1900.0 + radius {
                return None;
            }
        }
        Some(Town {
            n,
            e,
            radius,
            grid: hash2(cx, cy, k.wrapping_add(5)) * PI / 2.0,
        })
    }

    /// The town of a cell of the town grid, if it has one.
    pub fn town(&self, cell: Cell) -> Option<Town> {
        if let Some(place) = &self.place {
            let cs = Terrain::TOWN_CELL;
            return place.region.towns.iter().find(|t| ((t.n / cs).floor() as i32, (t.e / cs).floor() as i32) == cell).copied();
        }
        if let Some(hit) = self.caches().towns.get(&cell) {
            return *hit;
        }
        let found = self.find_town(cell);
        self.caches().towns.insert(cell, found);
        found
    }

    /// Towns that reach into the given rectangle.
    pub fn towns(&self, min_n: f64, max_n: f64, min_e: f64, max_e: f64) -> Vec<Town> {
        let cs = Terrain::TOWN_CELL;
        let reach = 800.0;
        if let Some(place) = &self.place {
            return place.region.towns.iter().filter(|t| {
                let reach = t.radius * 1.15;
                t.n > min_n - reach && t.n < max_n + reach && t.e > min_e - reach && t.e < max_e + reach
            }).copied().collect();
        }
        let x0 = ((min_n - reach) / cs).floor() as i32;
        let x1 = ((max_n + reach) / cs).floor() as i32;
        let y0 = ((min_e - reach) / cs).floor() as i32;
        let y1 = ((max_e + reach) / cs).floor() as i32;
        let mut out = Vec::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(t) = self.town((x, y)) {
                    if t.n > min_n - reach
                        && t.n < max_n + reach
                        && t.e > min_e - reach
                        && t.e < max_e + reach
                    {
                        out.push(t);
                    }
                }
            }
        }
        out
    }

    // MARK: Lakes

    /// The lake of a cell of the lake grid, if it has one (generated worlds only).
    pub fn lake(&self, cell: Cell) -> Option<Lake> {
        if self.place.is_some() {
            return None;
        }
        if let Some(hit) = self.caches().lakes.get(&cell) {
            return *hit;
        }
        let found = self.find_lake(cell);
        self.caches().lakes.insert(cell, found);
        found
    }

    /// Lakes whose water may reach into the given rectangle.
    pub fn lakes(&self, min_n: f64, max_n: f64, min_e: f64, max_e: f64) -> Vec<Lake> {
        let cs = Terrain::LAKE_CELL;
        let mut out = Vec::new();
        for x in ((min_n - cs) / cs).floor() as i32..=((max_n + cs) / cs).floor() as i32 {
            for y in ((min_e - cs) / cs).floor() as i32..=((max_e + cs) / cs).floor() as i32 {
                if let Some(l) = self.lake((x, y)) {
                    let reach = l.reach();
                    if l.n > min_n - reach && l.n < max_n + reach && l.e > min_e - reach && l.e < max_e + reach {
                        out.push(l);
                    }
                }
            }
        }
        out
    }

    /// Where a river's valley widens in a cell, a lake may fill it: about one cell in five has one.
    /// Its middle is the point of the valley floor nearest the river among a few
    /// tried; its water stands a few metres over the bed there and spreads over the valley floor
    /// as far as the ground is lower, within an oval round the middle, filling a sixth of it at
    /// least. Never near an aerodrome, a town or the sea, so that none of them and no road changes.
    fn find_lake(&self, cell: Cell) -> Option<Lake> {
        let cs = Terrain::LAKE_CELL;
        // Offset: `hash2` gives a cell and the one opposite the origin the same numbers.
        let (cx, cy) = (cell.0 as i64 + 7919, cell.1 as i64 + 104_729);
        let k = self.seed.wrapping_add(6000);
        if hash2(cx, cy, k) > Terrain::LAKE_SHARE {
            return None;
        }
        // The point nearest a river's middle (the most valley) among 9 by 9 over the cell.
        let mut best: Option<(f64, f64, Raw)> = None;
        for i in 0..81 {
            let n = (cell.0 as f64 + 0.1 + 0.8 * (i / 9) as f64 / 8.0) * cs;
            let e = (cell.1 as f64 + 0.1 + 0.8 * (i % 9) as f64 / 8.0) * cs;
            let p = self.dry(n, e);
            if p.valley > best.map_or(0.9, |b| b.2.valley) && p.h > 8.0 {
                best = Some((n, e, p));
            }
        }
        let (n, e, centre) = best?;
        let radius = 450.0 + 950.0 * hash2(cx, cy, k.wrapping_add(1));
        let lake = Lake {
            n,
            e,
            level: centre.h + 2.0 + 4.0 * hash2(cx, cy, k.wrapping_add(2)),
            length: radius * (1.0 + 0.8 * hash2(cx, cy, k.wrapping_add(3))),
            width: radius,
            heading: hash2(cx, cy, k.wrapping_add(4)) * PI,
        };
        let reach = lake.reach();
        if !self.airports(n - reach - Terrain::AIRPORT_REACH, n + reach + Terrain::AIRPORT_REACH, e - reach - Terrain::AIRPORT_REACH, e + reach + Terrain::AIRPORT_REACH).is_empty() {
            return None;
        }
        if !self.towns(n - reach - 300.0, n + reach + 300.0, e - reach - 300.0, e + reach + 300.0).is_empty() {
            return None;
        }
        // A basin worth the name: the valley floor under its level fills a sixth of its oval or more.
        let (c, s) = (lake.heading.cos(), lake.heading.sin());
        let filled = (0..225)
            .filter(|i| {
                let (u, v) = ((i / 15) as f64 / 7.0 - 1.0, (i % 15) as f64 / 7.0 - 1.0);
                let (a, b) = (u * lake.length, v * lake.width);
                let p = self.dry(n + a * c - b * s, e + a * s + b * c);
                u * u + v * v <= 1.0 && p.h < lake.level && p.valley > 0.25
            })
            .count();
        if filled < 30 {
            return None;
        }
        // No sea round it: its basin is inland.
        let sea = (0..24).any(|i| {
            let a = i as f64 / 24.0 * 2.0 * PI;
            self.dry(n + reach * a.cos(), e + reach * a.sin()).h < 2.0
        });
        (!sea).then_some(lake)
    }

    /// A road joins a town to its neighbour to the north (direction 0) or east (1), unless water or mountains are in the way.
    fn road(&self, cell: Cell, direction: i32) -> Option<Road> {
        let key = (cell.0, cell.1, direction);
        if let Some(hit) = self.caches().roads.get(&key) {
            return *hit;
        }
        let mut found = None;
        let next = (
            cell.0 + if direction == 0 { 1 } else { 0 },
            cell.1 + if direction == 1 { 1 } else { 0 },
        );
        if let (Some(a), Some(b)) = (self.town(cell), self.town(next)) {
            let (mut ok, mut lo, mut hi) = (true, f64::INFINITY, f64::NEG_INFINITY);
            for i in 0..=24 {
                let t = i as f64 / 24.0;
                let p = self.dry(lerp(a.n, b.n, t), lerp(a.e, b.e, t));
                if p.h < 1.5 {
                    ok = false;
                    break;
                }
                lo = lo.min(p.h);
                hi = hi.max(p.h);
            }
            if ok && hi - lo < 260.0 {
                found = Some(Road {
                    n0: a.n,
                    e0: a.e,
                    n1: b.n,
                    e1: b.e,
                    width: Road::GENERATED_WIDTH,
                });
            }
        }
        self.caches().roads.insert(key, found);
        found
    }

    /// The buildings of a region's maps whose middle is in the rectangle (none in a generated world):
    /// each in one rectangle of a tiling.
    pub fn buildings(&self, min_n: f64, max_n: f64, min_e: f64, max_e: f64) -> Vec<&Footprint> {
        let Some(place) = &self.place else { return Vec::new() };
        let square = |v: f64| (v / BUILDING_SQUARE).floor() as i32;
        let mut found = Vec::new();
        for x in square(min_n)..=square(max_n) {
            for y in square(min_e)..=square(max_e) {
                for &i in place.buildings.get(&(x, y)).into_iter().flatten() {
                    let b = &place.region.buildings[i as usize];
                    let (n, e) = b.centre();
                    if n >= min_n && n < max_n && e >= min_e && e < max_e {
                        found.push(b);
                    }
                }
            }
        }
        found
    }

    /// How much of the ground round a point (north, east) a region's buildings cover, 0 to 1: the
    /// share of the 768 m round each square of 256 m under its buildings, eased from one square's
    /// middle to the next. 0 in a generated world.
    pub fn built_share(&self, n: f64, e: f64) -> f64 {
        let Some(place) = &self.place else { return 0.0 };
        if place.built.is_empty() {
            return 0.0;
        }
        let (u, v) = (n / BUILDING_SQUARE - 0.5, e / BUILDING_SQUARE - 0.5);
        let (i, j) = (u.floor(), v.floor());
        let (fu, fv) = (u - i, v - j);
        let at = |di: i32, dj: i32| place.built.get(&(i as i32 + di, j as i32 + dj)).copied().unwrap_or(0.0).min(1.0) as f64;
        lerp(lerp(at(0, 0), at(0, 1), fv), lerp(at(1, 0), at(1, 1), fv), fu)
    }

    /// How much a point (north, east) of a region is in a city the maps draw, 0 to 1, by how much of
    /// the ground round it their buildings cover (`built_share`): there the maps draw every
    /// building, and what they leave unsaid is the city's ground, not the seed's woods and fields.
    /// 0 in a generated world.
    pub fn urban(&self, n: f64, e: f64) -> f64 {
        smoothstep(URBAN.0, URBAN.1, self.built_share(n, e))
    }

    /// Whether a building of a region's maps stands within `pad` metres of a point (north, east).
    pub fn building_near(&self, n: f64, e: f64, pad: f64) -> bool {
        let Some(place) = &self.place else { return false };
        if place.buildings.is_empty() {
            return false;
        }
        let square = |v: f64| (v / BUILDING_SQUARE).floor() as i32;
        let reach = NEAR_REACH + pad;
        for x in square(n - reach)..=square(n + reach) {
            for y in square(e - reach)..=square(e + reach) {
                for &i in place.buildings.get(&(x, y)).into_iter().flatten() {
                    let b = &place.region.buildings[i as usize];
                    // Most are farther than their box round them: those are not looked at closer.
                    let (lo, hi) = b.ring.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
                    if n < lo[0] as f64 - pad || n > hi[0] as f64 + pad || e < lo[1] as f64 - pad || e > hi[1] as f64 + pad {
                        continue;
                    }
                    // Within `pad` of a side of its outline, not only of a corner: a tree beside
                    // the middle of a long wall is as near it.
                    let near = |k: usize| {
                        let (a, c) = (b.ring[k], b.ring[(k + 1) % b.ring.len()]);
                        let (an, ae, dn, de) = (a[0] as f64, a[1] as f64, (c[0] - a[0]) as f64, (c[1] - a[1]) as f64);
                        let t = (((n - an) * dn + (e - ae) * de) / (dn * dn + de * de).max(1e-9)).clamp(0.0, 1.0);
                        (n - an - dn * t).hypot(e - ae - de * t) < pad
                    };
                    if b.contains(n, e) || (pad > 0.0 && (0..b.ring.len()).any(near)) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The tallest building or bridge of a region's maps above the ground under it, metres (0 in a
    /// generated world).
    pub fn tallest_building(&self) -> f64 {
        self.place.as_ref().map_or(0.0, |p| p.tallest)
    }

    /// What of a region's buildings and bridges a point (north, east, height above the sea) is
    /// inside of: each building its own outline from under the ground to its roof, each part of a
    /// bridge its own box or bar (`solids`). None in a generated world.
    pub fn solid_at(&self, n: f64, e: f64, h: f64) -> Option<solids::Hit> {
        let place = self.place.as_ref()?;
        place.solids.hit(&place.region, n, e, h)
    }

    /// The surface of the building or bridge nearest a point within `reach` (`Solids::surface`).
    pub fn solid_surface(&self, n: f64, e: f64, h: f64, reach: f64) -> Option<(f64, [f64; 3], [f64; 3], crate::scenery::ObstacleKind)> {
        let place = self.place.as_ref()?;
        place.solids.surface(&place.region, n, e, h, reach)
    }

    /// The highest roof or deck over a point (north, east) no higher than `h`: what something
    /// falling there comes to rest on before the ground.
    pub fn solid_under(&self, n: f64, e: f64, h: f64) -> Option<f64> {
        let place = self.place.as_ref()?;
        place.solids.top_under(&place.region, n, e, h)
    }

    /// The highest roof or deck over a point (north, east).
    pub fn solid_top(&self, n: f64, e: f64) -> Option<f64> {
        let place = self.place.as_ref()?;
        place.solids.top(&place.region, n, e)
    }

    /// Roads with an end in or next to the given rectangle.
    pub fn roads(&self, min_n: f64, max_n: f64, min_e: f64, max_e: f64) -> Vec<Road> {
        if let Some(place) = &self.place {
            let square = |v: f64| (v / ROAD_SQUARE).floor() as i32;
            let mut found: Vec<u32> = Vec::new();
            for x in square(min_n)..=square(max_n) {
                for y in square(min_e)..=square(max_e) {
                    found.extend(place.roads.get(&(x, y)).into_iter().flatten());
                }
            }
            found.sort_unstable();
            found.dedup();
            return found.into_iter().map(|i| place.region.roads[i as usize]).collect();
        }
        let cs = Terrain::TOWN_CELL;
        let x0 = (min_n / cs).floor() as i32 - 1;
        let x1 = (max_n / cs).floor() as i32;
        let y0 = (min_e / cs).floor() as i32 - 1;
        let y1 = (max_e / cs).floor() as i32;
        let mut out = Vec::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                for d in 0..=1 {
                    if let Some(r) = self.road((x, y), d) {
                        out.push(r);
                    }
                }
            }
        }
        out
    }

    // MARK: Final ground

    /// Ground at a point given the airports that can reach it (see `airports`). Use this one for
    /// bulk queries: gather the airports of the area once and pass them to every call.
    pub fn ground_with(&self, n: f64, e: f64, airports: &[Airport]) -> Ground {
        let r = self.raw(n, e);
        let (mut h, mut river, mut flat) = (r.h, r.river, 0.0_f64);
        for a in airports {
            let w = a.flat_weight(n, e);
            if w > 0.0 {
                h = lerp(h, a.elevation, w);
                river *= 1.0 - w;
                flat = flat.max(w);
            }
        }
        for c in &self.circuits {
            if let Some((level, w)) = c.earthwork(n, e, h) {
                h = lerp(h, level, w);
                river *= 1.0 - w;
                flat = flat.max(w);
            }
        }
        Ground { h, river, flat }
    }

    pub fn ground(&self, n: f64, e: f64) -> Ground {
        self.ground_with(n, e, &self.airports(n, n, e, e))
    }

    pub fn height(&self, n: f64, e: f64) -> f64 {
        self.ground(n, e).h
    }

    // MARK: Land cover

    /// Colour and cover of the ground. `slope` is the magnitude of the height gradient and `town`
    /// the weight of the built-up area at the point (0 outside towns).
    pub fn surface(&self, n: f64, e: f64, g: &Ground, slope: f64, town: f64) -> Surface {
        let Some(place) = &self.place else { return self.generated_surface(n, e, g, slope, town) };
        // Where the maps say nothing, the ground is the seed's; at their edge the two blend. The
        // seed's ground turns to beach and loses its fields and woods below 25 m, where its coasts
        // are; a region's coast is where its maps put it, and its low land is land like any other.
        let shares = place.region.shares(n, e);
        let unknown = shares[Cover::Unknown as usize];
        let inland = Ground { h: g.h.max(25.0), ..*g };
        if unknown >= 1.0 {
            return self.unmapped_surface(n, e, &inland, slope, town);
        }
        let mapped = self.mapped_surface(n, e, g, slope, town, &shares);
        if unknown <= 0.0 {
            return mapped;
        }
        let generated = self.unmapped_surface(n, e, &inland, slope, town);
        let mix = |a: f32, b: f32| a + (b - a) * unknown as f32;
        Surface {
            color: mapped.color + (generated.color - mapped.color) * unknown as f32,
            farm: mix(mapped.farm, generated.farm),
            water: mapped.water,
            forest: mix(mapped.forest, generated.forest),
        }
    }

    /// The ground of a region where its maps say nothing: the seed's, but in a city. Among the maps'
    /// buildings (`built_share`) what they leave unsaid is a rail yard, a school's or a hospital's
    /// grounds, the land inside a motorway junction, a car park: no wood or field of the seed's
    /// grows there, and from far it is the colour of built-up ground.
    fn unmapped_surface(&self, n: f64, e: f64, g: &Ground, slope: f64, town: f64) -> Surface {
        let generated = self.generated_surface(n, e, g, slope, town);
        let urban = self.urban(n, e) as f32;
        if urban <= 0.0 {
            return generated;
        }
        let patch = fbm(n / 420.0, e / 420.0, 2, self.seed.wrapping_add(89));
        let built = DVec3::new(0.21, 0.19, 0.17) * (0.85 + 0.3 * patch);
        let built = Vec3::new(built.x as f32, built.y as f32, built.z as f32);
        Surface {
            color: generated.color + (built - generated.color) * urban,
            farm: generated.farm * (1.0 - urban),
            water: generated.water,
            forest: generated.forest * (1.0 - urban),
        }
    }

    /// The ground as the maps of a region cover it (`shares`, of which some are not `Unknown`),
    /// with the rock of the steep slopes, the snow up high and the sand by the sea as on generated ground.
    fn mapped_surface(&self, n: f64, e: f64, g: &Ground, slope: f64, town: f64, shares: &crate::region::Shares) -> Surface {
        let h = g.h;
        let known = 1.0 - shares[Cover::Unknown as usize];
        let share = |c: Cover| shares[c as usize] / known;
        let moist = clamp(fbm(n / 6000.0, e / 6000.0, 3, self.seed.wrapping_add(83)) * 1.3 + 0.5, 0.0, 1.0);
        let patch = fbm(n / 420.0, e / 420.0, 2, self.seed.wrapping_add(89));

        let dry = DVec3::new(0.17, 0.17, 0.075);
        let lush = DVec3::new(0.085, 0.15, 0.045);
        let crops = dry + (lush - dry) * smoothstep(0.15, 0.75, moist + patch * 0.15);
        let rock_color = DVec3::new(0.20, 0.185, 0.165) * (0.85 + 0.3 * patch);
        let sand_color = DVec3::new(0.50, 0.44, 0.30);
        let snow_color = DVec3::new(0.86, 0.88, 0.92);
        let built_color = DVec3::new(0.21, 0.19, 0.17) * (0.85 + 0.3 * patch);
        let colour = |c: Cover| match c {
            Cover::Unknown => DVec3::ZERO,
            Cover::Grass => lush * (0.9 + 0.25 * patch),
            Cover::Farmland => crops,
            Cover::Scrub => dry + (lush - dry) * 0.3,
            Cover::Forest => DVec3::new(0.028, 0.065, 0.024),
            Cover::Rock => rock_color,
            Cover::Sand => sand_color,
            Cover::Ice => snow_color,
            Cover::Wetland => lush * 0.75,
            Cover::Built => built_color,
            // Ballast, sleepers and concrete.
            Cover::Railway => DVec3::new(0.16, 0.15, 0.14) * (0.9 + 0.2 * patch),
            // The banks, under the water the ground's shader draws.
            Cover::Water => lush,
            // The bed of the sea; by a beach the sand at the water's edge (`beach`) covers it.
            Cover::Sea => DVec3::new(0.30, 0.28, 0.22),
        };
        let mut c = Cover::ALL.iter().map(|&k| colour(k) * share(k)).fold(DVec3::ZERO, |a, b| a + b);

        let not_flat = 1.0 - g.flat;
        let built = (share(Cover::Built) + share(Cover::Railway)).max(town);
        let forest = share(Cover::Forest) * (1.0 - smoothstep(0.6, 0.9, slope)) * not_flat * (1.0 - g.river);
        let farm = share(Cover::Farmland) * (1.0 - smoothstep(0.06, 0.13, slope)) * not_flat;
        let rock = smoothstep(0.5, 0.85, slope).max(smoothstep(1900.0, 2300.0, h) * 0.75);
        c += (rock_color - c) * rock;
        let snow = smoothstep(2250.0, 2650.0, h + patch * 250.0) * (1.0 - smoothstep(0.9, 1.5, slope));
        c += (snow_color - c) * snow;
        let beach = (1.0 - smoothstep(1.0, 2.5, h)) * (1.0 - smoothstep(0.2, 0.5, slope)) * (1.0 - share(Cover::Built));
        c += (sand_color - c) * beach;
        c += (built_color - c) * town * 0.85;
        Surface {
            color: Vec3::new(c.x as f32, c.y as f32, c.z as f32),
            farm: (farm * (1.0 - rock) * (1.0 - built)) as f32,
            water: g.river as f32,
            forest: (forest * (1.0 - rock) * (1.0 - snow) * (1.0 - built)) as f32,
        }
    }

    /// The ground as a seed covers it.
    fn generated_surface(&self, n: f64, e: f64, g: &Ground, slope: f64, town: f64) -> Surface {
        let h = g.h;
        let moist = clamp(
            fbm(n / 6000.0, e / 6000.0, 3, self.seed.wrapping_add(83)) * 1.3 + 0.5,
            0.0,
            1.0,
        );
        let patch = fbm(n / 420.0, e / 420.0, 2, self.seed.wrapping_add(89));

        let dry = DVec3::new(0.17, 0.17, 0.075);
        let lush = DVec3::new(0.085, 0.15, 0.045);
        let mut c = dry + (lush - dry) * smoothstep(0.15, 0.75, moist + patch * 0.15);

        let not_flat = 1.0 - g.flat;
        let forest = smoothstep(0.50, 0.60, moist + patch * 0.22)
            * (1.0 - smoothstep(0.35, 0.6, slope))
            * (1.0 - smoothstep(1500.0, 1900.0, h))
            * smoothstep(3.0, 10.0, h)
            * not_flat
            * (1.0 - g.river);
        let farm = (1.0 - smoothstep(0.06, 0.13, slope))
            * smoothstep(8.0, 20.0, h)
            * (1.0 - smoothstep(280.0, 400.0, h))
            * (1.0 - forest)
            * smoothstep(0.2, 0.3, moist)
            * not_flat;

        c += (DVec3::new(0.028, 0.065, 0.024) - c) * forest;
        let rock = smoothstep(0.5, 0.85, slope).max(smoothstep(1900.0, 2300.0, h) * 0.75);
        let rock_color = DVec3::new(0.20, 0.185, 0.165) * (0.85 + 0.3 * patch);
        c += (rock_color - c) * rock;
        let snow =
            smoothstep(2250.0, 2650.0, h + patch * 250.0) * (1.0 - smoothstep(0.9, 1.5, slope));
        c += (DVec3::new(0.86, 0.88, 0.92) - c) * snow;
        let sand = (1.0 - smoothstep(1.5, 5.0, h)) * (1.0 - smoothstep(0.2, 0.5, slope));
        c += (DVec3::new(0.50, 0.44, 0.30) - c) * sand;
        // Built-up ground: streets, yards and roofs seen from far away.
        c += (DVec3::new(0.21, 0.19, 0.17) * (0.85 + 0.3 * patch) - c) * town * 0.85;

        Surface {
            color: Vec3::new(c.x as f32, c.y as f32, c.z as f32),
            farm: (farm * (1.0 - rock) * (1.0 - town)) as f32,
            water: g.river as f32,
            forest: (forest * (1.0 - rock) * (1.0 - snow) * (1.0 - town)) as f32,
        }
    }
}
