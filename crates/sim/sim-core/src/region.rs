//! A real place as plain data: the heights of its ground, what covers it, its aerodromes, roads,
//! towns and buildings, in the world's axes (north, east, metres from an origin). `geodata` builds one from maps
//! before the `World` is made; `Terrain::from_region` answers every query from it. Nothing here
//! knows latitudes and longitudes but the origin's, kept for the planet under the world.
//!
//! The same region always gives the same world: it is read, never changed, and nothing arrives
//! after the world is made.

use crate::math::lerp;
use crate::terrain::{Airport, Road, Town};

/// Values at the nodes of a regular grid: node (row, column) is at north `south + row · step`, east
/// `west + column · step`. Rows run northwards.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid<T> {
    pub south: f64,
    pub west: f64,
    pub step: f64,
    pub rows: usize,
    pub cols: usize,
    /// Row after row, `rows · cols` values.
    pub data: Vec<T>,
}

impl<T: Copy> Grid<T> {
    /// A grid of `rows` by `cols` nodes, all `fill`.
    pub fn new(south: f64, west: f64, step: f64, rows: usize, cols: usize, fill: T) -> Grid<T> {
        Grid { south, west, step, rows, cols, data: vec![fill; rows * cols] }
    }

    /// A grid centred on a point (north, east), `size` metres a side.
    pub fn centred(n: f64, e: f64, size: f64, step: f64, fill: T) -> Grid<T> {
        let nodes = (size / step).round() as usize + 1;
        let half = (nodes - 1) as f64 * step / 2.0;
        Grid::new(n - half, e - half, step, nodes, nodes, fill)
    }

    #[inline(always)]
    pub fn at(&self, row: usize, col: usize) -> T {
        self.data[row * self.cols + col]
    }

    #[inline(always)]
    pub fn set(&mut self, row: usize, col: usize, value: T) {
        self.data[row * self.cols + col] = value;
    }

    /// North and east of a node.
    pub fn node(&self, row: usize, col: usize) -> (f64, f64) {
        (self.south + row as f64 * self.step, self.west + col as f64 * self.step)
    }

    /// The point in nodes: (row, column), fractional, not clamped.
    #[inline(always)]
    pub fn locate(&self, n: f64, e: f64) -> (f64, f64) {
        ((n - self.south) / self.step, (e - self.west) / self.step)
    }

    /// How far inside the grid a point is, in nodes from the nearest edge: negative outside.
    #[inline(always)]
    pub fn inside(&self, n: f64, e: f64) -> f64 {
        let (r, c) = self.locate(n, e);
        r.min(c).min((self.rows - 1) as f64 - r).min((self.cols - 1) as f64 - c)
    }

    /// The value of the node nearest to a point, if the point is within half a step of the grid.
    pub fn nearest(&self, n: f64, e: f64) -> Option<T> {
        if self.inside(n, e) < -0.5 {
            return None;
        }
        let (r, c) = self.locate(n, e);
        let r = (r.round().max(0.0) as usize).min(self.rows - 1);
        let c = (c.round().max(0.0) as usize).min(self.cols - 1);
        Some(self.at(r, c))
    }

    /// The four nodes round a point and the weight of each, clamped to the grid.
    #[inline(always)]
    fn corners(&self, n: f64, e: f64) -> [(usize, f64); 4] {
        let (r, c) = self.locate(n, e);
        let r = r.clamp(0.0, (self.rows - 1) as f64);
        let c = c.clamp(0.0, (self.cols - 1) as f64);
        let (r0, c0) = ((r as usize).min(self.rows.saturating_sub(2)), (c as usize).min(self.cols.saturating_sub(2)));
        let (fr, fc) = (r - r0 as f64, c - c0 as f64);
        let (r1, c1) = ((r0 + 1).min(self.rows - 1), (c0 + 1).min(self.cols - 1));
        [
            (r0 * self.cols + c0, (1.0 - fr) * (1.0 - fc)),
            (r0 * self.cols + c1, (1.0 - fr) * fc),
            (r1 * self.cols + c0, fr * (1.0 - fc)),
            (r1 * self.cols + c1, fr * fc),
        ]
    }
}

impl Grid<f32> {
    /// Bilinear between the four nodes round the point; outside, the nearest edge's.
    #[inline(always)]
    pub fn sample(&self, n: f64, e: f64) -> f64 {
        self.corners(n, e).iter().map(|&(i, w)| self.data[i] as f64 * w).sum()
    }
}

/// What covers the ground at a node of `Region::cover`. Where two overlap on a map the greater wins:
/// water over a town, a town over a wood, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Cover {
    /// The maps say nothing: the ground is as the seed would make it.
    Unknown = 0,
    Grass,
    Farmland,
    Scrub,
    Forest,
    Rock,
    Sand,
    Ice,
    Wetland,
    /// Houses, industry, shops: built-up ground.
    Built,
    /// Lakes, reservoirs and the wide rivers.
    Water,
    Sea,
    /// A railway's ground: its tracks, yards and stations (OpenStreetMap's `landuse=railway`). No
    /// tree grows on it, no field or house stands on it.
    Railway,
}

impl Cover {
    pub const COUNT: usize = 13;
    pub const ALL: [Cover; Cover::COUNT] = [
        Cover::Unknown,
        Cover::Grass,
        Cover::Farmland,
        Cover::Scrub,
        Cover::Forest,
        Cover::Rock,
        Cover::Sand,
        Cover::Ice,
        Cover::Wetland,
        Cover::Built,
        Cover::Water,
        Cover::Sea,
        Cover::Railway,
    ];

    /// The cover a stored byte stands for (`Unknown` for anything else).
    pub fn from_byte(b: u8) -> Cover {
        Cover::ALL.get(b as usize).copied().unwrap_or(Cover::Unknown)
    }
}

/// How much of each cover there is round a point: the share of each of the four nodes round it.
pub type Shares = [f64; Cover::COUNT];

/// The shape of a building's roof, as OpenStreetMap's `roof:shape` names it (Overture's
/// `roof_shape`), or as `geodata` guesses it for a small house the maps say nothing of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Roof {
    #[default]
    Flat = 0,
    /// Two slopes meeting at a ridge along the building (`roofs::Frame`), gables at its ends.
    Gabled,
    /// Slopes from every side: a ridge on a long building, a point on a square one.
    Hipped,
    /// Slopes from every side up to one point.
    Pyramidal,
    /// One slope, rising across the building to its high edge along `Footprint::direction`.
    Skillion,
    Dome,
    /// A dome that swells out before it narrows to a point (an onion dome).
    Onion,
    /// A half cylinder along the ridge (a barrel vault).
    Round,
    /// Gabled, each side broken into a steep lower slope and a gentle upper one.
    Gambrel,
    /// Hipped, steep slopes round a flat top.
    Mansard,
}

impl Roof {
    pub const ALL: [Roof; 10] = [Roof::Flat, Roof::Gabled, Roof::Hipped, Roof::Pyramidal, Roof::Skillion, Roof::Dome, Roof::Onion, Roof::Round, Roof::Gambrel, Roof::Mansard];

    pub fn from_byte(b: u8) -> Roof {
        Roof::ALL.get(b as usize).copied().unwrap_or(Roof::Flat)
    }

    /// Whether it has sloping faces (all but the flat roof).
    pub fn pitched(self) -> bool {
        self != Roof::Flat
    }
}

/// What a building is for, from the maps' `building=*` (Overture's `class` and `subtype`). It
/// decides how its walls look when the maps do not say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Use {
    #[default]
    Unknown = 0,
    /// A house of one family: detached, semi-detached, a terrace, a bungalow, a farmhouse.
    House,
    /// Flats.
    Residential,
    /// Offices.
    Office,
    /// Shops, a supermarket, a mall.
    Retail,
    /// A factory, a warehouse, a shed for work.
    Industrial,
    /// A church, a cathedral, a chapel, a mosque, a temple.
    Religious,
    /// A town hall, a museum, a school, a hospital, a station: public buildings.
    Civic,
    /// A barn, a farm's shed, a greenhouse, a garage, a carport, a hut.
    Outbuilding,
}

impl Use {
    pub const ALL: [Use; 9] = [Use::Unknown, Use::House, Use::Residential, Use::Office, Use::Retail, Use::Industrial, Use::Religious, Use::Civic, Use::Outbuilding];

    pub fn from_byte(b: u8) -> Use {
        Use::ALL.get(b as usize).copied().unwrap_or(Use::Unknown)
    }
}

/// What a building's walls are made of, when the maps say (`building:material`, Overture's `facade_material`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Material {
    #[default]
    Unknown = 0,
    Glass,
    Brick,
    Stone,
    Concrete,
    Metal,
    /// Plaster or render.
    Plaster,
    Wood,
}

impl Material {
    pub const ALL: [Material; 8] = [Material::Unknown, Material::Glass, Material::Brick, Material::Stone, Material::Concrete, Material::Metal, Material::Plaster, Material::Wood];

    pub fn from_byte(b: u8) -> Material {
        Material::ALL.get(b as usize).copied().unwrap_or(Material::Unknown)
    }
}

/// A building as the maps draw it, or a part of one (`building:part`: a tower on a podium, a
/// spire, a dome on a drum): its outline, how high it stands and what it looks like.
#[derive(Clone, Debug, PartialEq)]
pub struct Footprint {
    /// Its outline, north and east, metres; the first point is not repeated at the end.
    pub ring: Vec<[f32; 2]>,
    /// Courtyards and light wells: rings inside the outline that are not the building.
    pub holes: Vec<Vec<[f32; 2]>>,
    /// How high its top and its bottom are above the ground, metres (a part raised on others, or
    /// a roof over a passage, has a bottom above it). The top is the roof's highest point.
    pub height: f32,
    pub floor: f32,
    pub roof: Roof,
    /// How high the roof rises above the eaves, metres; 0: as its shape and size make it
    /// (`roofs::Frame`).
    pub roof_height: f32,
    /// Radians from north: the way the ridge runs (a skillion's high edge); `None` when the maps do
    /// not say (then it runs along the longest side).
    pub direction: Option<f32>,
    /// Floors above the ground, 0 when not known.
    pub levels: u8,
    pub kind: Use,
    pub material: Material,
    /// The colours the maps give its walls and its roof, sRGB; `None` when they say nothing.
    pub wall_colour: Option<[u8; 3]>,
    pub roof_colour: Option<[u8; 3]>,
    /// Whether the maps give its height (otherwise it is guessed from its neighbours and its use).
    pub measured: bool,
}

impl Default for Footprint {
    fn default() -> Footprint {
        Footprint {
            ring: Vec::new(),
            holes: Vec::new(),
            height: 0.0,
            floor: 0.0,
            roof: Roof::Flat,
            roof_height: 0.0,
            direction: None,
            levels: 0,
            kind: Use::Unknown,
            material: Material::Unknown,
            wall_colour: None,
            roof_colour: None,
            measured: true,
        }
    }
}

impl Footprint {
    /// A building of that outline, height and roof, everything else unknown.
    pub fn new(ring: Vec<[f32; 2]>, height: f32, floor: f32, roof: Roof) -> Footprint {
        Footprint { ring, height, floor, roof, ..Footprint::default() }
    }

    /// The middle of its outline (the mean of its corners), north and east.
    pub fn centre(&self) -> (f64, f64) {
        let k = self.ring.len().max(1) as f64;
        let (n, e) = self.ring.iter().fold((0.0, 0.0), |(n, e), p| (n + p[0] as f64, e + p[1] as f64));
        (n / k, e / k)
    }

    /// The area inside its outline, m².
    pub fn area(&self) -> f64 {
        let twice: f64 = self.ring.iter().zip(self.ring.iter().cycle().skip(1)).map(|(a, b)| a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64).sum();
        twice.abs() / 2.0
    }

    /// The box round it along its longest side: the middle (north, east), the direction of that
    /// side (radians from north) and the half length along it and across.
    pub fn oriented_box(&self) -> ((f64, f64), f64, f64, f64) {
        let side = |i: usize| {
            let (a, b) = (self.ring[i], self.ring[(i + 1) % self.ring.len()]);
            ((b[0] - a[0]) as f64, (b[1] - a[1]) as f64)
        };
        let longest = (0..self.ring.len()).map(side).max_by(|a, b| a.0.hypot(a.1).total_cmp(&b.0.hypot(b.1))).unwrap_or((1.0, 0.0));
        let heading = longest.1.atan2(longest.0);
        let (c, s) = (heading.cos(), heading.sin());
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &self.ring {
            let (n, e) = (p[0] as f64, p[1] as f64);
            let (u, v) = (n * c + e * s, -n * s + e * c);
            lo = [lo[0].min(u), lo[1].min(v)];
            hi = [hi[0].max(u), hi[1].max(v)];
        }
        let (u, v) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
        ((u * c - v * s, u * s + v * c), heading, (hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0)
    }

    /// Whether a point (north, east) is inside its outline and in none of its courtyards.
    pub fn contains(&self, n: f64, e: f64) -> bool {
        inside_ring(&self.ring, n, e) && !self.holes.iter().any(|h| inside_ring(h, n, e))
    }
}

/// Whether a point (north, east) is inside a ring, by the even-odd rule.
pub fn inside_ring(ring: &[[f32; 2]], n: f64, e: f64) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let (a, b) = (ring[j], ring[i]);
        let (an, ae, bn, be) = (a[0] as f64, a[1] as f64, b[0] as f64, b[1] as f64);
        if (an > n) != (bn > n) && e < ae + (n - an) / (bn - an) * (be - ae) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// What a bridge carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Carries {
    #[default]
    Road = 0,
    Rail,
    /// People on foot or on bicycles.
    Path,
}

impl Carries {
    pub const ALL: [Carries; 3] = [Carries::Road, Carries::Rail, Carries::Path];

    pub fn from_byte(b: u8) -> Carries {
        Carries::ALL.get(b as usize).copied().unwrap_or(Carries::Road)
    }
}

/// How a bridge holds up its deck (OpenStreetMap's `bridge:structure`; a plain beam on piers when
/// it does not say).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Structure {
    #[default]
    Beam = 0,
    /// An arch under the deck carrying it on columns.
    Arch,
    /// Hung from two cables over two towers.
    Suspension,
    /// Hung from straight stays fanning down from its towers.
    CableStayed,
    /// Lattice girders beside the deck.
    Truss,
}

impl Structure {
    pub const ALL: [Structure; 5] = [Structure::Beam, Structure::Arch, Structure::Suspension, Structure::CableStayed, Structure::Truss];

    pub fn from_byte(b: u8) -> Structure {
        Structure::ALL.get(b as usize).copied().unwrap_or(Structure::Beam)
    }
}

/// A bridge: its deck along a line, how high it runs, what it carries and how it stands.
/// `sim_core::bridges` makes its parts (deck, piers, towers, cables) from it, for the simulation
/// to run into and the world to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bridge {
    /// The middle of the deck's top, north, east and height above the sea, metres, from one end to
    /// the other; its ends meet the ground.
    pub line: Vec<[f32; 3]>,
    /// Across the deck, metres.
    pub width: f32,
    pub carries: Carries,
    pub structure: Structure,
    /// Where along the line its towers stand (suspension and cable-stayed) or its arch springs from,
    /// metres from its first end; empty: as its structure and length make them.
    pub towers: Vec<f32>,
    /// How high its towers rise above the deck, metres (0: as its span makes them).
    pub tower_height: f32,
    /// The colour of its steel or stone, sRGB, when the maps or a landmark say.
    pub colour: Option<[u8; 3]>,
    /// Its towers are of stone: each one block, wider than the deck, pierced above it by an arch
    /// over each carriageway (the Brooklyn Bridge's), not two steel legs.
    pub masonry: bool,
    /// Made by hand from its public dimensions (`geodata`'s landmarks), not from the maps.
    pub landmark: bool,
}

impl Bridge {
    /// Its length along the line, metres.
    pub fn length(&self) -> f64 {
        self.line.windows(2).map(|w| ((w[1][0] - w[0][0]) as f64).hypot((w[1][1] - w[0][1]) as f64)).sum()
    }

    /// The point of the deck's top `s` metres along the line from its first end (clamped to its
    /// ends), and the direction of the line there (radians from north).
    pub fn at(&self, s: f64) -> ([f64; 3], f64) {
        let mut left = s.max(0.0);
        let last = self.line.len().saturating_sub(2);
        for (i, w) in self.line.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let (dn, de) = ((b[0] - a[0]) as f64, (b[1] - a[1]) as f64);
            let d = dn.hypot(de);
            if left <= d || i == last {
                let t = if d > 0.0 { (left / d).min(1.0) } else { 0.0 };
                let p = [a[0] as f64 + dn * t, a[1] as f64 + de * t, a[2] as f64 + (b[2] - a[2]) as f64 * t];
                return (p, de.atan2(dn));
            }
            left -= d;
        }
        let p = self.line.first().map_or([0.0; 3], |p| [p[0] as f64, p[1] as f64, p[2] as f64]);
        (p, 0.0)
    }
}

/// A real place, as `geodata` builds it and `Terrain::from_region` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// What the player calls it.
    pub name: String,
    /// Where north 0, east 0 is on the Earth, radians: for the planet under the world (the gyros, the
    /// compass). North and east are the azimuthal equidistant projection round it.
    pub latitude: f64,
    pub longitude: f64,
    /// Heights of the ground above the sea, metres, finest grid first; each coarser one covers more.
    /// The sea is below 0 and the land above it: a map's coast already decided which is which.
    pub heights: Vec<Grid<f32>>,
    /// What covers the ground (`Cover` as bytes) where the maps were read.
    pub cover: Grid<u8>,
    /// Every runway, one `Airport` each; the runways of an aerodrome share its `id`.
    pub airports: Vec<Airport>,
    pub roads: Vec<Road>,
    /// The built-up areas, as circles round which `scenery` lays its streets and houses.
    pub towns: Vec<Town>,
    /// The buildings the maps draw, where they draw them (a city's middle); `scenery` lays its own
    /// houses only where there are none of these.
    pub buildings: Vec<Footprint>,
    /// The bridges of the maps and the landmarks made by hand.
    pub bridges: Vec<Bridge>,
}

/// The colour of a real place's ground as satellites see it from above, for drawing: grids of
/// texels in the world's axes (north, east), finest first, each `[r, g, b, a]` with the colour
/// as sRGB of the ground's reflectance (its albedo under the sun, 0 to 1) and `a` 255 where it is
/// known and 0 where it is not (no cloud-free view, or outside what was photographed). `geodata`
/// makes it from Sentinel-2 after the region (`imagery`); the simulation never reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imagery {
    pub levels: Vec<Grid<[u8; 4]>>,
    /// The years of the photographs, oldest first: the credit names them.
    pub years: Vec<u16>,
}

/// Nodes over which a finer grid of heights gives way to the next coarser one at its edge.
const BLEND: f64 = 12.0;

impl Region {
    /// Height of the ground, metres: the finest grid that holds the point, eased into the next
    /// coarser one over its last `BLEND` nodes; beyond the coarsest, its edge.
    pub fn height(&self, n: f64, e: f64) -> f64 {
        self.height_from(0, n, e)
    }

    fn height_from(&self, level: usize, n: f64, e: f64) -> f64 {
        let Some(grid) = self.heights.get(level) else { return 0.0 };
        if level + 1 == self.heights.len() {
            return grid.sample(n, e);
        }
        let inside = grid.inside(n, e);
        if inside <= 0.0 {
            return self.height_from(level + 1, n, e);
        }
        let fine = grid.sample(n, e);
        if inside >= BLEND {
            return fine;
        }
        lerp(self.height_from(level + 1, n, e), fine, inside / BLEND)
    }

    /// How much of each cover there is at a point, from the four nodes round it (all `Unknown`
    /// outside the grid).
    pub fn shares(&self, n: f64, e: f64) -> Shares {
        let mut shares = [0.0; Cover::COUNT];
        if self.cover.inside(n, e) < 0.0 {
            shares[Cover::Unknown as usize] = 1.0;
            return shares;
        }
        for (i, w) in self.cover.corners(n, e) {
            shares[Cover::from_byte(self.cover.data[i]) as usize] += w;
        }
        shares
    }

    /// The cover of the node nearest to a point.
    pub fn cover_at(&self, n: f64, e: f64) -> Cover {
        self.cover.nearest(n, e).map_or(Cover::Unknown, Cover::from_byte)
    }

    /// The half side of the square the region's finest maps cover round its origin, metres: where
    /// it is worth flying.
    pub fn reach(&self) -> f64 {
        let g = &self.cover;
        (g.rows.min(g.cols) - 1) as f64 * g.step / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_sample_between_their_nodes_and_hold_their_edge_outside() {
        let mut g = Grid::new(-10.0, -10.0, 10.0, 3, 3, 0.0f32);
        g.set(1, 1, 8.0);
        g.set(2, 2, 4.0);
        assert_eq!(g.sample(0.0, 0.0), 8.0);
        assert_eq!(g.sample(5.0, 0.0), 4.0);
        assert_eq!(g.sample(10.0, 10.0), 4.0);
        assert_eq!(g.sample(500.0, 500.0), 4.0);
        assert_eq!(g.inside(0.0, 0.0), 1.0);
        assert!(g.inside(-30.0, 0.0) < 0.0);
        let c = Grid::centred(100.0, 200.0, 1000.0, 10.0, 0u8);
        assert_eq!((c.rows, c.node(50, 50)), (101, (100.0, 200.0)));
    }

    #[test]
    fn a_finer_grid_eases_into_the_coarser_one_at_its_edge() {
        let fine = Grid::centred(0.0, 0.0, 400.0, 10.0, 100.0f32);
        let coarse = Grid::centred(0.0, 0.0, 4000.0, 100.0, 0.0f32);
        let region = Region {
            name: "test".into(),
            latitude: 0.0,
            longitude: 0.0,
            heights: vec![fine, coarse],
            cover: Grid::centred(0.0, 0.0, 400.0, 10.0, Cover::Forest as u8),
            airports: Vec::new(),
            roads: Vec::new(),
            towns: Vec::new(),
            buildings: Vec::new(),
            bridges: Vec::new(),
        };
        assert_eq!(region.height(0.0, 0.0), 100.0);
        assert_eq!(region.height(1000.0, 0.0), 0.0);
        let edge = region.height(195.0, 0.0);
        assert!(edge > 0.0 && edge < 100.0, "{edge}");
        // Going out it only falls.
        let mut last = f64::INFINITY;
        for k in 0..60 {
            let h = region.height(70.0 + k as f64 * 2.5, 0.0);
            assert!(h <= last);
            last = h;
        }
        assert_eq!(region.shares(0.0, 0.0)[Cover::Forest as usize], 1.0);
        assert_eq!(region.shares(5000.0, 0.0)[Cover::Unknown as usize], 1.0);
        assert_eq!(region.cover_at(3.0, 3.0), Cover::Forest);
        assert_eq!(region.reach(), 200.0);
    }
}
