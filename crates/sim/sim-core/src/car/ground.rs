//! What a car stands and runs on: the ground under each wheel (`Patch`: its height, its slope and
//! what it is made of, `Surface`) and the walls beside it (`Wall`). `CarGround` is what answers
//! those questions: the world's terrain with its circuits (`TerrainGround`), or a plane in a test.

use crate::math::V3;
use crate::terrain::Terrain;

use super::wet::RoadWater;

/// What the ground is made of where a tyre touches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Surface {
    /// A circuit's or a road's asphalt.
    #[default]
    Asphalt,
    /// The painted kerbs of a circuit's corners.
    Kerb,
    /// Concrete: a pit lane, an apron, a runway.
    Concrete,
    Grass,
    /// A gravel trap: loose stones that the tyres plough into.
    Gravel,
    /// Bare earth and forest floor off the circuit.
    Dirt,
    /// Water deeper than the tyres: a car that drives into it is lost.
    Water,
}

impl Surface {
    /// How much grip a tyre has on it, a share of what it has on dry asphalt (`TyreFit::grip`).
    /// Dry asphalt and concrete 1; painted kerbs 0.9 (the paint is smoother than the asphalt's
    /// grain); short grass 0.5, gravel 0.55 and earth 0.6: the usual figures for road tyres on
    /// those surfaces (Wong, Theory of Ground Vehicles, 4th ed., 2008, table 1.3; Gillespie,
    /// Fundamentals of Vehicle Dynamics, 1992, table 3.1), lower ends for a slick-ish tread.
    pub fn grip(self) -> f64 {
        match self {
            Surface::Asphalt => 1.0,
            Surface::Concrete => 0.97,
            Surface::Kerb => 0.9,
            Surface::Grass => 0.5,
            Surface::Gravel => 0.55,
            Surface::Dirt => 0.6,
            Surface::Water => 0.1,
        }
    }

    /// Rolling resistance coefficient on it at low speed: force over load. Asphalt 0.012, grass
    /// 0.06 (Wong, table 1.2: 0.045 to 0.08 for a car tyre on grass and soil), gravel 0.25: a trap's
    /// deep loose stones, which bring a car at 100 km/h to a stop in some 60 to 80 m (the FIA's
    /// guidance for gravel beds asks for 0.25 to 0.45 of deceleration in g).
    pub fn rolling(self) -> f64 {
        match self {
            Surface::Asphalt | Surface::Concrete => 0.012,
            Surface::Kerb => 0.014,
            Surface::Grass => 0.06,
            Surface::Dirt => 0.05,
            Surface::Gravel => 0.25,
            Surface::Water => 0.4,
        }
    }

    /// How rough it is to roll over: the height of its bumps, metres (the kerbs' ridges are part
    /// of their shape, not of this).
    pub fn roughness(self) -> f64 {
        match self {
            Surface::Asphalt | Surface::Concrete | Surface::Kerb => 0.0,
            Surface::Grass => 0.012,
            Surface::Dirt => 0.02,
            Surface::Gravel => 0.025,
            Surface::Water => 0.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Surface::Asphalt => "asphalt",
            Surface::Kerb => "kerb",
            Surface::Concrete => "concrete",
            Surface::Grass => "grass",
            Surface::Gravel => "gravel",
            Surface::Dirt => "dirt",
            Surface::Water => "water",
        }
    }
}

/// The ground at a point: its height above the sea, its normal (world axes, north, east, down,
/// pointing up: (0, 0, −1) when level) and what it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub height: f64,
    pub normal: V3,
    pub surface: Surface,
    /// Whether it is a circuit's (exact, cheap to ask again) or the terrain's (a plane that can be
    /// kept for a step).
    pub exact: bool,
    /// Water standing on it, m (0 dry; `wet::RoadWater`).
    pub water: f64,
    /// How much of a tyre's width there is on paint (a circuit's lines), 0 to 1: slippery wet.
    pub paint: f64,
}

impl Patch {
    /// The height of the plane of the patch at another point near it, with it at (`n0`, `e0`).
    pub fn height_at(&self, n0: f64, e0: f64, n: f64, e: f64) -> f64 {
        let nz = -self.normal.z;
        if nz < 1e-3 {
            return self.height;
        }
        self.height + (self.normal.x * (n - n0) + self.normal.y * (e - e0)) / -nz
    }
}

/// What a wall is: how hard it is and how it slides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallKind {
    /// Steel guard rail on posts: it gives a little and the car scrapes along it.
    Armco,
    /// Tyres bundled and belted: soft, it takes much of the impact.
    Tyres,
    /// A concrete wall: hard.
    Concrete,
}

impl WallKind {
    /// Stiffness, N/m of how far the car is pushed into it; damping, N per m/s (what of the
    /// impact it takes); friction of the car sliding along it.
    pub fn contact(self) -> (f64, f64, f64) {
        match self {
            WallKind::Armco => (2.5e6, 6.0e4, 0.45),
            WallKind::Tyres => (6.0e5, 5.0e4, 0.7),
            WallKind::Concrete => (6.0e6, 5.0e4, 0.5),
        }
    }
}

/// A straight piece of wall, standing on the ground from `a` to `b` (north, east), from `base` up
/// to `base + height`; `normal` (north, east) points away from it to the side a car belongs on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    pub a: (f64, f64),
    pub b: (f64, f64),
    pub base: f64,
    pub height: f64,
    pub normal: (f64, f64),
    pub kind: WallKind,
}

/// What a car asks of the world it runs in.
pub trait CarGround {
    /// The ground at a point.
    fn patch(&self, n: f64, e: f64) -> Patch;
    /// The walls within `reach` metres of a point, added to `out`.
    fn walls(&self, n: f64, e: f64, reach: f64, out: &mut Vec<Wall>);
}

/// The world's ground: its circuits where they are (with the water on them, if any), the terrain
/// elsewhere.
pub struct TerrainGround<'a> {
    pub terrain: &'a Terrain,
    /// The water on the terrain's circuits (none: dry), in the order of `Terrain::circuits`.
    pub water: &'a [RoadWater],
}

impl<'a> TerrainGround<'a> {
    /// The terrain, dry.
    pub fn dry(terrain: &'a Terrain) -> TerrainGround<'a> {
        TerrainGround { terrain, water: &[] }
    }
}

impl CarGround for TerrainGround<'_> {
    fn patch(&self, n: f64, e: f64) -> Patch {
        for (k, c) in self.terrain.circuits().iter().enumerate() {
            if let Some((mut p, at)) = c.patch_located(n, e) {
                if let Some(w) = self.water.get(k).filter(|w| !w.dry()) {
                    if matches!(p.surface, Surface::Asphalt | Surface::Kerb | Surface::Concrete) {
                        p.water = w.depth(&at, p.surface == Surface::Kerb);
                    } else if p.surface != Surface::Water {
                        // Off the paved ground the rain only wets it (no puddles drawn there).
                        p.water = w.film(at.along, at.across);
                    }
                }
                return p;
            }
        }
        let g = self.terrain.ground(n, e);
        if g.h <= 0.3 || g.river > 0.5 {
            return Patch { height: g.h.max(0.0), normal: V3::new(0.0, 0.0, -1.0), surface: Surface::Water, exact: false, water: 0.0, paint: 0.0 };
        }
        let d = 1.0;
        let gn = self.terrain.ground(n + d, e).h;
        let ge = self.terrain.ground(n, e + d).h;
        let normal = V3::new(-(gn - g.h) / d, -(ge - g.h) / d, -1.0).normalize();
        let on_runway = self.terrain.airports(n, n, e, e).iter().any(|a| a.on_runway(n, e));
        let surface = if on_runway { Surface::Concrete } else if g.flat > 0.5 { Surface::Grass } else { Surface::Dirt };
        Patch { height: g.h, normal, surface, exact: false, water: 0.0, paint: 0.0 }
    }

    fn walls(&self, n: f64, e: f64, reach: f64, out: &mut Vec<Wall>) {
        for c in self.terrain.circuits() {
            c.walls(n, e, reach, out);
        }
    }
}

/// A plane, for tests: level or sloping, of one surface, with walls if given.
#[derive(Clone, Debug)]
pub struct Plane {
    /// Height at the origin.
    pub height: f64,
    /// How much it rises per metre north and per metre east.
    pub rise: (f64, f64),
    pub surface: Surface,
    pub walls: Vec<Wall>,
    /// Water standing on it, m.
    pub water: f64,
}

impl Plane {
    pub fn level(surface: Surface) -> Plane {
        Plane { height: 0.0, rise: (0.0, 0.0), surface, walls: Vec::new(), water: 0.0 }
    }
}

impl CarGround for Plane {
    fn patch(&self, n: f64, e: f64) -> Patch {
        let normal = V3::new(-self.rise.0, -self.rise.1, -1.0).normalize();
        Patch { height: self.height + self.rise.0 * n + self.rise.1 * e, normal, surface: self.surface, exact: true, water: self.water, paint: 0.0 }
    }

    fn walls(&self, _n: f64, _e: f64, _reach: f64, out: &mut Vec<Wall>) {
        out.extend_from_slice(&self.walls);
    }
}
