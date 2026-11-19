//! What is left of an aircraft that has come apart: its pieces, falling, tumbling and coming to
//! rest, and the fire of its fuel. `World` owns the wrecks and steps them with the aircraft; they
//! stay where they fell when the aircraft is placed somewhere new.
//!
//! A piece keeps the body axes the aircraft had: drawing the part of the aircraft that belongs to
//! it at `Piece::origin` and `Piece::q`, without what its `Piece::cuts` take away, puts it where it is.

use crate::aircraft::AircraftKind;
use crate::atmosphere;
use crate::math::{act, inverse, lerp, normalize, smoothstep, Quat, Rng, GRAVITY, V3};

/// The pieces an aircraft breaks into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceKind {
    /// Cabin, engine, propeller and landing gear.
    Fuselage,
    LeftWing,
    RightWing,
    /// Tail cone, stabiliser, elevator, fin and rudder.
    Tail,
}

impl PieceKind {
    pub const ALL: [PieceKind; 4] = [PieceKind::Fuselage, PieceKind::LeftWing, PieceKind::RightWing, PieceKind::Tail];

    fn index(self) -> usize {
        self as usize
    }

    /// The wing on a side: 0 left, 1 right.
    pub fn wing(side: usize) -> PieceKind {
        [PieceKind::LeftWing, PieceKind::RightWing][side]
    }
}

/// A surface that can be torn across its span: a wing, or each half of the tailplane. Stations are
/// distances from the centre line, m; the rest is body axes from the reference centre of gravity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    /// Where it leaves the fuselage, and its tip.
    pub root: f64,
    pub tip: f64,
    /// Leading and trailing edge (x) and the height of its chord line (z), at the root and at the tip.
    pub lead: (f64, f64),
    pub trail: (f64, f64),
    pub z: (f64, f64),
    /// Its depth, m, and the mass of one side, kg, spread evenly along it.
    pub thickness: f64,
    pub mass: f64,
}

impl Span {
    fn at(&self, edge: (f64, f64), station: f64) -> f64 {
        lerp(edge.0, edge.1, (station - self.root) / (self.tip - self.root))
    }

    /// Its leading and trailing edge at a station on a side (-1 left, 1 right).
    pub fn leading_edge(&self, side: f64, station: f64) -> V3 {
        V3::new(self.at(self.lead, station), side * station, self.at(self.z, station))
    }

    pub fn trailing_edge(&self, side: f64, station: f64) -> V3 {
        V3::new(self.at(self.trail, station), side * station, self.at(self.z, station))
    }

    pub fn chord(&self, station: f64) -> f64 {
        self.at(self.lead, station) - self.at(self.trail, station)
    }

    /// The share of one side between two stations.
    pub fn share(&self, from: f64, to: f64) -> f64 {
        ((to.min(self.tip) - from.max(self.root)) / (self.tip - self.root)).max(0.0)
    }

    /// Centre of mass of the part of one side between two stations: halfway along it, at a third of the chord.
    pub fn centre(&self, side: f64, from: f64, to: f64) -> V3 {
        let station = (from + to) / 2.0;
        self.leading_edge(side, station).lerp(self.trailing_edge(side, station), 0.35)
    }
}

/// Part of a span between two stations on one side (-1 left, 1 right): a wing, or half a tailplane,
/// torn across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slice {
    pub side: f64,
    pub from: f64,
    pub to: f64,
}

/// A plane across the vehicle, body axes, that a tear followed: the part of the vehicle on one side
/// of it is kept, `inside` (where `normal · p < offset`) or outside. The stub left on the vehicle and
/// the piece that came off are the same plane kept on opposite sides, so a ragged edge drawn the same
/// way on both matches. With `within` (a normal and an offset) it cuts only where `normal · p >
/// offset` of that second plane: across half of a tailplane and not the tail cone it stands on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub normal: V3,
    pub offset: f64,
    pub inside: bool,
    pub within: Option<(V3, f64)>,
}

impl Cut {
    /// Across a span at a station on a side: keeping what is inboard of it, or outboard.
    pub fn across(side: f64, station: f64, inboard: bool) -> Cut {
        Cut { normal: V3::new(0.0, side, 0.0), offset: station, inside: inboard, within: None }
    }
}

/// Which part of a vehicle a piece of its wreck is, and its mass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portion {
    pub kind: PieceKind,
    /// For a wing, the part of it between two stations; for the tail, a half of the tailplane. `None`:
    /// all of the piece (less what `cuts` say).
    pub slice: Option<Slice>,
    /// What has been torn off it, or what of the piece it is.
    pub cuts: [Option<Cut>; 2],
    pub mass: f64,
}

impl Portion {
    /// All of one piece.
    pub fn whole(kind: PieceKind, mass: f64) -> Portion {
        Portion { kind, slice: None, cuts: [None; 2], mass }
    }

    /// Its centre of mass in body axes, on an aircraft that comes apart as `shape` says.
    pub fn centre(&self, shape: &Pieces) -> V3 {
        match self.slice {
            Some(s) => shape.span(self.kind).centre(s.side, s.from, s.to),
            None => shape.centre(self.kind),
        }
    }
}

/// How one kind of aircraft comes apart: where it is cut, and the size, mass and shape of each
/// piece, in body axes from the reference centre of gravity. Indexed in the order of `PieceKind::ALL`.
#[derive(Debug, PartialEq)]
pub struct Pieces {
    /// The aircraft these are the pieces of.
    pub kind: AircraftKind,
    /// The tail is what is aft of this.
    pub tail_x: f64,
    /// The wings are what is outboard of the root of `wing` and between these heights.
    pub wing_z: (f64, f64),
    /// Each wing (with its strut, flap and aileron, without fuel), as it can be torn across.
    pub wing: Span,
    /// Each half of the tailplane (stabiliser and elevator), as it can be torn across.
    pub stabiliser: Span,
    /// Mass of the tail, both halves of the tailplane with it, kg.
    pub tail_mass: f64,
    pub(crate) centres: [V3; 4],
    pub(crate) points: [&'static [V3]; 4],
    /// Area of a flat plate with the drag of the piece as it tumbles, m².
    pub(crate) drag_areas: [f64; 4],
    /// Size of the box whose inertia the piece has: length, width, height.
    pub(crate) boxes: [(f64, f64, f64); 4],
}

impl Pieces {
    /// Where a body point (x forward, y right, z down, from the reference centre of gravity)
    /// belongs when the aircraft comes apart: the tail aft of `tail_x`, the wings outboard of the
    /// cabin and within the wing's heights.
    pub fn of_point(&self, p: V3) -> PieceKind {
        if p.x < self.tail_x {
            PieceKind::Tail
        } else if p.y.abs() > self.wing.root && p.z > self.wing_z.0 && p.z < self.wing_z.1 {
            if p.y < 0.0 {
                PieceKind::LeftWing
            } else {
                PieceKind::RightWing
            }
        } else {
            PieceKind::Fuselage
        }
    }

    /// Centre of mass of the piece in body axes.
    pub fn centre(&self, kind: PieceKind) -> V3 {
        self.centres[kind.index()]
    }

    /// Points of the piece that can touch the ground, body axes.
    pub fn points(&self, kind: PieceKind) -> &'static [V3] {
        self.points[kind.index()]
    }

    fn drag_area(&self, kind: PieceKind) -> f64 {
        self.drag_areas[kind.index()]
    }

    /// Principal moments of inertia of a piece of this mass, kg·m²: as a box of its size.
    fn inertia(&self, kind: PieceKind, mass: f64) -> V3 {
        let (x, y, z) = self.boxes[kind.index()];
        mass / 12.0 * V3::new(y * y + z * z, x * x + z * z, x * x + y * y)
    }

    /// The plane a tear across the span of a piece follows (`Cut::across`) at a station on a side
    /// (-1 left, 1 right); across the tailplane only aft of its leading edge, so that it leaves the
    /// tail cone it stands on whole.
    pub fn tear(&self, kind: PieceKind, side: f64, station: f64, inboard: bool) -> Cut {
        let cut = Cut::across(side, station, inboard);
        if kind != PieceKind::Tail {
            return cut;
        }
        let lead = self.stabiliser.lead.0.max(self.stabiliser.lead.1) + 0.1;
        Cut { within: Some((V3::new(-1.0, 0.0, 0.0), -lead)), ..cut }
    }

    /// The span a slice of this kind of piece is cut from: the wing's, or the tailplane's.
    pub fn span(&self, kind: PieceKind) -> &Span {
        if kind == PieceKind::Tail {
            &self.stabiliser
        } else {
            &self.wing
        }
    }
}

/// One piece of a wreck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub kind: PieceKind,
    /// How the aircraft it came from comes apart.
    pub shape: &'static Pieces,
    /// For a slice of a wing or of the tailplane, which (`Portion::slice`).
    pub slice: Option<Slice>,
    /// What of the piece's part of the aircraft it is not (`Portion::cuts`).
    pub cuts: [Option<Cut>; 2],
    /// The piece's centre of mass, world axes.
    pub pos: V3,
    /// Body axes → world.
    pub q: Quat,
    /// Velocity of the centre of mass, world axes, m/s.
    pub vel: V3,
    /// Angular velocity, body axes, rad/s.
    pub omega: V3,
    pub mass: f64,
    /// Seconds it has lain still; once at rest it is no longer moved.
    still: f64,
    pub at_rest: bool,
    /// Gone under water.
    pub sunk: bool,
    /// In the water (it went in: a splash).
    pub wet: bool,
}

impl Piece {
    /// A piece as it was on the aircraft at the moment it came off: `pos` and `q` are the
    /// aircraft's (its reference centre of gravity), `vel` and `omega` its velocity (world axes)
    /// and angular velocity (body axes).
    pub fn new(shape: &'static Pieces, portion: Portion, pos: V3, q: Quat, vel: V3, omega: V3) -> Piece {
        let mut piece = Piece {
            kind: portion.kind,
            shape,
            slice: portion.slice,
            cuts: portion.cuts,
            pos,
            q,
            vel,
            omega,
            mass: portion.mass,
            still: 0.0,
            at_rest: false,
            sunk: false,
            wet: false,
        };
        let arm = piece.centre();
        piece.pos = pos + act(q, arm);
        piece.vel = vel + act(q, omega.cross(arm));
        piece
    }

    /// Centre of mass in body axes.
    pub fn centre(&self) -> V3 {
        Portion { kind: self.kind, slice: self.slice, cuts: self.cuts, mass: self.mass }.centre(self.shape)
    }

    /// Where the aircraft's reference centre of gravity would be for this piece, world axes:
    /// draw the piece's part of the aircraft at this point with `q`.
    pub fn origin(&self) -> V3 {
        self.pos - act(self.q, self.centre())
    }

    /// The points of it that can touch the ground, body axes, and how many there are: a slice's
    /// are the corners of its two ends and the middle of its edges.
    fn points(&self) -> ([V3; 10], usize) {
        let mut out = [V3::ZERO; 10];
        let Some(s) = self.slice else {
            let points = self.shape.points(self.kind);
            let n = points.len().min(out.len());
            out[..n].copy_from_slice(&points[..n]);
            return (out, n);
        };
        let span = self.shape.span(self.kind);
        for (k, station) in [s.from, (s.from + s.to) / 2.0, s.to].into_iter().enumerate() {
            out[2 * k] = span.leading_edge(s.side, station);
            out[2 * k + 1] = span.trailing_edge(s.side, station);
        }
        (out, 6)
    }

    /// Area of a flat plate with the drag it has as it tumbles, m².
    fn drag_area(&self) -> f64 {
        match self.slice {
            // About 0.6 of its plan, as a whole wing tumbles.
            Some(s) => {
                let span = self.shape.span(self.kind);
                0.6 * span.chord((s.from + s.to) / 2.0) * (s.to - s.from)
            }
            None => self.shape.drag_area(self.kind),
        }
    }

    /// Principal moments of inertia, kg·m²: as a box of its size.
    fn inertia(&self) -> V3 {
        let Some(s) = self.slice else {
            return self.shape.inertia(self.kind, self.mass);
        };
        let span = self.shape.span(self.kind);
        let (x, y, z) = (span.chord((s.from + s.to) / 2.0), s.to - s.from, span.thickness);
        self.mass / 12.0 * V3::new(y * y + z * z, x * x + z * z, x * x + y * y)
    }

    /// One step of `dt` seconds. `ground(n, e, up)` is the height of what something at height `up`
    /// there comes down on (the ground, or a roof or a deck below it) and whether it is water.
    fn step(&mut self, dt: f64, ground: &impl Fn(f64, f64, f64) -> (f64, bool)) {
        if self.at_rest || self.sunk {
            return;
        }
        let m = self.mass;
        let inertia = self.inertia();
        let drag_area = self.drag_area();
        let rho = atmosphere::density(-self.pos.z);
        let speed = self.vel.length();
        let mut force = V3::new(0.0, 0.0, m * GRAVITY) - 0.5 * rho * speed * drag_area * self.vel;
        // Tumbling through the air damps the spin.
        let mut moment = -self.omega * (0.5 * rho * speed.max(3.0) * drag_area * 2.0);

        let (h, water) = ground(self.pos.x, self.pos.y, -self.pos.z);
        let mut touching = false;
        if water {
            // Under the surface: buoyancy short of the weight, and the drag of water.
            let depth = self.pos.z + h;
            self.wet = depth > 0.0;
            if depth > 0.0 {
                force.z -= 0.75 * m * GRAVITY * (depth / 0.5).min(1.0);
                force -= 0.5 * 1000.0 * speed * drag_area * 0.3 * self.vel;
                moment -= self.omega * m * 2.0;
                if depth > 15.0 {
                    self.sunk = true;
                }
            }
        } else {
            self.wet = false;
            // Each point that is in the ground pushes back like a stiff spring, and rubs.
            let k = m * 600.0;
            let c = m * 25.0;
            let centre = self.centre();
            let (points, count) = self.points();
            for &point in &points[..count] {
                let r = point - centre;
                let pw = self.pos + act(self.q, r);
                let depth = pw.z + if (pw.x - self.pos.x).abs() + (pw.y - self.pos.y).abs() < 1.0 { h } else { ground(pw.x, pw.y, -pw.z).0 };
                if depth <= 0.0 {
                    continue;
                }
                touching = true;
                let vw = self.vel + act(self.q, self.omega.cross(r));
                let normal = (k * depth.min(0.5) + c * vw.z).max(0.0);
                let slide = V3::new(vw.x, vw.y, 0.0);
                let s = slide.length();
                let friction = if s > 1e-4 { -slide / s * (s / 0.3).tanh() * 0.7 * normal } else { V3::ZERO };
                let f = V3::new(0.0, 0.0, -normal) + friction;
                force += f;
                moment += r.cross(act(inverse(self.q), f));
            }
        }

        self.vel += force / m * dt;
        let w = self.omega;
        self.omega += (moment - w.cross(inertia * w)) / inertia * dt;
        self.pos += self.vel * dt;
        let spin = Quat::from_xyzw(self.omega.x, self.omega.y, self.omega.z, 0.0);
        let dq = (self.q * spin) * (0.5 * dt);
        self.q = (self.q + dq).normalize();

        if touching && self.vel.length() < 0.2 && self.omega.length() < 0.2 {
            self.still += dt;
            if self.still > 1.0 {
                self.at_rest = true;
                self.vel = V3::ZERO;
                self.omega = V3::ZERO;
            }
        } else {
            self.still = 0.0;
        }
    }
}

/// The burning fuel of a wreck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fire {
    /// Fuel left to burn, kg.
    pub fuel: f64,
    /// World time it caught, and when it went out.
    pub started: f64,
    pub out: Option<f64>,
}

impl Fire {
    /// Fuel a fire of spilt avgas burns, kg/s: a full load of 109 kg lasts about five minutes.
    pub const BURN: f64 = 0.35;
}

/// Speeds of an impact (m/s, see `Wreck::severity`) between which the tanks go from holding to all
/// bursting. Crash-resistant tanks are tested to hold a drop at 20 m/s (FAA AC 20-135, the US Army's
/// 65 ft/s); a light aircraft's wet wing or thin metal tank splits from about 12 m/s, and at 30 m/s
/// the structure round it is crushed.
pub const TANKS_BURST: (f64, f64) = (12.0, 30.0);
/// Below this impact speed nothing lights the fuel that has spilt (sparks of metal on rock and road,
/// a hot exhaust torn open, a shorted battery all come with a violent impact); above it, it lights.
pub const IGNITION_SPEED: f64 = 18.0;
/// The share of the spilt fuel that is thrown out as a mist and goes up at once in a fireball: none
/// at `IGNITION_SPEED`, half at 45 m/s and above (a fast impact atomises what bursts out); the rest
/// pools round the wreck and burns there.
const FIREBALL_SHARE: (f64, f64, f64) = (IGNITION_SPEED, 45.0, 0.5);

/// What the fuel aboard does when a vehicle breaks up in an impact of `severity` (m/s, see
/// `Wreck::severity`): the kilograms that burst out and go up at once in a fireball, and those
/// that are spilt and burn on the ground. None of it burns in water: what spills floats away,
/// and there is nothing there to light it.
pub fn fuel_released(fuel: f64, severity: f64, water: bool) -> (f64, f64) {
    let spilt = fuel.max(0.0) * smoothstep(TANKS_BURST.0, TANKS_BURST.1, severity);
    if water || severity < IGNITION_SPEED || spilt < 1.0 {
        return (0.0, 0.0);
    }
    let (from, full, most) = FIREBALL_SHARE;
    let fireball = spilt * most * smoothstep(from, full, severity);
    (fireball, spilt - fireball)
}

/// A small piece torn off in the impact (a panel, a fairing, part of the engine) and flung away. It
/// flies on its own drag, in closed form, from the moment of the crash until it comes down; it does
/// not bounce or roll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fragment {
    /// Where it was, world axes, and its velocity at the moment of the crash.
    pub start: V3,
    pub vel: V3,
    /// Time constant of its drag, s: it falls at most at `GRAVITY * tau`.
    pub tau: f64,
    /// Across, m.
    pub size: f64,
    /// Seconds it burns and trails smoke after the crash (0: it does not).
    pub burns: f64,
    /// Seconds after the crash it came down, and whether into water.
    pub landed: Option<f64>,
    pub water: bool,
}

impl Fragment {
    /// Where it is `t` seconds after the crash, world axes: a linear drag gives the path in closed
    /// form. After it came down, where it lies.
    pub fn at(&self, t: f64) -> V3 {
        let t = self.landed.map_or(t, |l| t.min(l)).max(0.0);
        let terminal = V3::new(0.0, 0.0, GRAVITY * self.tau);
        self.start + terminal * t + (self.vel - terminal) * self.tau * (1.0 - (-t / self.tau).exp())
    }

    /// Its velocity `t` seconds after the crash (zero once down).
    pub fn velocity(&self, t: f64) -> V3 {
        if self.landed.is_some_and(|l| t >= l) {
            return V3::ZERO;
        }
        let terminal = V3::new(0.0, 0.0, GRAVITY * self.tau);
        terminal + (self.vel - terminal) * (-t.max(0.0) / self.tau).exp()
    }
}

/// A mark the crash leaves on the ground: scorched where the fireball or burning fuel touched it,
/// dug where the aircraft went in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scar {
    /// Its centre on the ground, world axes.
    pub pos: V3,
    /// Radius of the scorched ground, m.
    pub radius: f64,
    /// Radius of the hole dug, m (0: none).
    pub crater: f64,
    /// Seconds after the crash it was made, and until when fuel spilt on it burns (as long as it
    /// was made, none).
    pub made: f64,
    pub burns_until: f64,
}

/// Something falling into the water: the aircraft, a piece of it, a fragment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splash {
    /// Where it went in, on the surface, world axes; seconds after the crash.
    pub pos: V3,
    pub time: f64,
    /// Its velocity as it went in, world axes, m/s, and its mass, kg.
    pub vel: V3,
    pub mass: f64,
}

/// The pieces of one aircraft and what burns of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Wreck {
    pub pieces: Vec<Piece>,
    pub fire: Option<Fire>,
    /// World time it came apart.
    pub time: f64,
    /// Fuel that went up in the fireball at the moment of the crash, kg (0 = none).
    pub fireball: f64,
    /// Where it came apart (the aircraft's reference centre of gravity), its velocity then (world
    /// axes) and its mass.
    pub impact: V3,
    pub impact_vel: V3,
    pub mass: f64,
    /// How hard the impact was, as the speed at which all of the energy it took would have been
    /// lost (m/s): the speed times the root of the share of its energy lost. What bursts the tanks
    /// and flings the fragments.
    pub severity: f64,
    pub fragments: Vec<Fragment>,
    pub scars: Vec<Scar>,
    pub splashes: Vec<Splash>,
    /// Whether the ground where it came apart has been looked at (scars or a splash there).
    struck: bool,
    /// Where the main piece was when the last scar of its burning slide was left, and whether it
    /// has left the scar of where it rests.
    trail: V3,
    rested: bool,
}

impl Wreck {
    /// At most this many fragments; an impact flings more of them the harder it is.
    pub const MOST_FRAGMENTS: usize = 24;
    /// Scars and splashes kept.
    const MOST_SCARS: usize = 12;
    const MOST_SPLASHES: usize = 16;
    /// A burning piece that slides leaves fuel burning on the ground every this many metres.
    const TRAIL_STEP: f64 = 8.0;

    /// Pieces flung apart from an aircraft at `pos`, attitude `q`, velocity `vel` (world axes)
    /// and angular velocity `omega` (body axes), of total mass `mass`, each of them whole. `keep` is the
    /// share of the velocity the pieces keep after the impact; `seed` makes the scatter. The harder the
    /// impact, the more fragments it flings (none when nothing is lost: a wing torn off in the air).
    #[allow(clippy::too_many_arguments)]
    pub fn new(shape: &'static Pieces, kinds: &[PieceKind], mass: f64, pos: V3, q: Quat, vel: V3, omega: V3, keep: f64, seed: u64, time: f64) -> Wreck {
        let wings = kinds.iter().filter(|k| matches!(k, PieceKind::LeftWing | PieceKind::RightWing)).count() as f64;
        let tail = if kinds.contains(&PieceKind::Tail) { shape.tail_mass } else { 0.0 };
        let portions: Vec<Portion> = kinds
            .iter()
            .map(|&kind| {
                let m = match kind {
                    PieceKind::LeftWing | PieceKind::RightWing => shape.wing.mass,
                    PieceKind::Tail => shape.tail_mass,
                    PieceKind::Fuselage => (mass - wings * shape.wing.mass - tail).max(300.0),
                };
                Portion::whole(kind, m)
            })
            .collect();
        Wreck::of(shape, &portions, mass, pos, q, vel, omega, keep, seed, time)
    }

    /// `new` with pieces that may be parts of the pieces the aircraft breaks into (`Portion`): what is
    /// left of a torn wing, a slice of it torn off.
    #[allow(clippy::too_many_arguments)]
    pub fn of(shape: &'static Pieces, portions: &[Portion], mass: f64, pos: V3, q: Quat, vel: V3, omega: V3, keep: f64, seed: u64, time: f64) -> Wreck {
        let mut rng = Rng::new(seed);
        let speed = vel.length();
        let pieces: Vec<Piece> = portions
            .iter()
            .map(|&portion| {
                let m = portion.mass;
                let mut v = vel * keep;
                // Upwards and outwards at random, more the harder the impact; lighter pieces fly further.
                let kick = speed * (1.0 - keep) * 0.35 * (300.0 / m).sqrt().min(2.0);
                let away = normalize(act(q, portion.centre(shape)) + V3::new(0.0, 0.0, -1.0));
                v += kick * (away + V3::new(rng.normal(), rng.normal(), -rng.uniform()) * 0.4);
                let spin = V3::new(rng.normal(), rng.normal(), rng.normal()) * (0.3 + speed * (1.0 - keep) * 0.04);
                Piece::new(shape, portion, pos, q, v, omega * keep + spin)
            })
            .collect();
        let severity = speed * (1.0 - keep * keep).max(0.0).sqrt();
        // Where it came apart: the heaviest piece.
        let heaviest = pieces.iter().max_by(|a, b| a.mass.total_cmp(&b.mass)).map_or(shape.centre(PieceKind::Fuselage), Piece::centre);
        let pos = pos + act(q, heaviest);
        let fragments = Wreck::fling(&mut rng, pos, vel, keep, severity);
        Wreck {
            pieces,
            fire: None,
            time,
            fireball: 0.0,
            impact: pos,
            impact_vel: vel,
            mass,
            severity,
            fragments,
            scars: Vec::new(),
            splashes: Vec::new(),
            struck: false,
            trail: pos,
            rested: false,
        }
    }

    /// The fragments an impact of `severity` flings from `pos`: up and away all round, most of them
    /// on the way the aircraft was going, at up to most of the speed lost.
    fn fling(rng: &mut Rng, pos: V3, vel: V3, keep: f64, severity: f64) -> Vec<Fragment> {
        let count = (Wreck::MOST_FRAGMENTS as f64 * smoothstep(10.0, 45.0, severity)).round() as usize;
        let ahead = vel * keep;
        (0..count)
            .map(|_| {
                let up = 0.25 + 0.75 * rng.uniform();
                let around = rng.uniform() * std::f64::consts::TAU;
                let flat = (1.0 - up * up).sqrt();
                let dir = V3::new(around.cos() * flat, around.sin() * flat, -up);
                let speed = severity * (0.25 + 0.6 * rng.uniform());
                let size = 0.12 + 0.5 * rng.uniform().powi(2);
                Fragment {
                    start: pos + V3::new(rng.normal(), rng.normal(), -rng.uniform()) * 0.8,
                    vel: ahead * (0.4 + 0.8 * rng.uniform()) + dir * speed,
                    // A bigger piece of sheet falls faster: terminal speeds of 10 to 35 m/s.
                    tau: (1.0 + 2.5 * rng.uniform()) * (0.6 + size),
                    size,
                    burns: 0.0,
                    landed: None,
                    water: false,
                }
            })
            .collect()
    }

    /// The fuel aboard at the impact (kg) bursts out, and burns, as `fuel_released` says; in
    /// `water` none of it burns. A share of the fragments, lit by the fireball, trail smoke.
    pub fn spill(&mut self, fuel: f64, water: bool) {
        let (fireball, pool) = fuel_released(fuel, self.severity, water);
        self.fireball = fireball;
        if pool > 0.0 {
            self.fire = Some(Fire { fuel: pool, started: self.time, out: None });
        }
        if fireball > 0.0 {
            let mut rng = Rng::new(self.impact.x.to_bits() ^ self.impact.y.to_bits().rotate_left(11));
            for f in &mut self.fragments {
                if rng.uniform() < 0.7 {
                    f.burns = 2.0 + 6.0 * rng.uniform();
                }
            }
        }
    }

    /// The fuselage piece, or the first there is: where the fire burns.
    pub fn main(&self) -> &Piece {
        self.pieces.iter().find(|p| p.kind == PieceKind::Fuselage).unwrap_or(&self.pieces[0])
    }

    /// Whether anything of it still moves or burns.
    pub fn active(&self) -> bool {
        self.pieces.iter().any(|p| !p.at_rest && !p.sunk) || self.fire.is_some_and(|f| f.out.is_none()) || self.fragments.iter().any(|f| f.landed.is_none())
    }

    /// One step: every piece that still moves, the fragments, the fire, and the marks they leave.
    /// `ground(n, e)` is the height of the ground and whether it is water.
    pub fn step(&mut self, dt: f64, time: f64, ground: impl Fn(f64, f64) -> (f64, bool)) {
        self.step_on(dt, time, |n, e, _| ground(n, e));
    }

    /// One step over ground that has more on it than the ground: `ground(n, e, up)` is the height of
    /// what something at height `up` there comes down on (a roof or a bridge's deck under it, or the
    /// ground) and whether it is water.
    pub fn step_on(&mut self, dt: f64, time: f64, ground: impl Fn(f64, f64, f64) -> (f64, bool)) {
        let since = time - self.time;
        if !self.struck {
            self.struck = true;
            self.strike(&ground);
        }
        for piece in &mut self.pieces {
            let wet = piece.wet;
            let vel = piece.vel;
            piece.step(dt, &ground);
            if piece.wet && !wet && vel.z > 3.0 && self.splashes.len() < Wreck::MOST_SPLASHES {
                let (h, _) = ground(piece.pos.x, piece.pos.y, -piece.pos.z);
                self.splashes.push(Splash { pos: V3::new(piece.pos.x, piece.pos.y, -h), time: since, vel, mass: piece.mass });
            }
        }
        for f in self.fragments.iter_mut().filter(|f| f.landed.is_none()) {
            let p = f.at(since);
            let (h, water) = ground(p.x, p.y, -p.z);
            if p.z >= -h {
                f.landed = Some(since);
                f.water = water;
                if water && self.splashes.len() < Wreck::MOST_SPLASHES {
                    self.splashes.push(Splash { pos: V3::new(p.x, p.y, -h), time: since, vel: f.velocity(since), mass: 2.0 + 30.0 * f.size * f.size });
                }
            }
        }
        let main = *self.main();
        if let Some(fire) = &mut self.fire {
            if fire.out.is_none() {
                fire.fuel -= Fire::BURN * dt;
                if fire.fuel <= 0.0 || main.sunk || ground(main.pos.x, main.pos.y, -main.pos.z).1 {
                    fire.fuel = fire.fuel.max(0.0);
                    fire.out = Some(time);
                }
                // Sliding while it burns, it leaves burning fuel behind it on the ground; where it
                // comes to rest the pool of fuel under it scorches the ground (its fire is the wreck's).
                let (h, water) = ground(main.pos.x, main.pos.y, -main.pos.z);
                let near_ground = main.pos.z > -h - 2.5;
                let moved = (main.pos - self.trail).length();
                if fire.out.is_none() && near_ground && !water && self.scars.len() < Wreck::MOST_SCARS {
                    let surface = V3::new(main.pos.x, main.pos.y, -h);
                    if moved > Wreck::TRAIL_STEP && main.vel.length() > 2.0 {
                        self.trail = main.pos;
                        self.scars.push(Scar { pos: surface, radius: 3.0, crater: 0.0, made: since, burns_until: since + 25.0 });
                    } else if main.at_rest && !self.rested {
                        self.rested = true;
                        self.scars.push(Scar { pos: surface, radius: 5.0, crater: 0.0, made: since, burns_until: since });
                    }
                }
            }
        }
    }

    /// The marks of the impact on what it hit, looked at once: a splash in water; on the ground, the
    /// scorch of the fireball where it reached down to it, and a hole if it went in hard.
    fn strike(&mut self, ground: &impl Fn(f64, f64, f64) -> (f64, bool)) {
        let at = self.impact;
        let (h, water) = ground(at.x, at.y, -at.z + 1.0);
        let above = -at.z - h;
        if water {
            if above < 6.0 {
                self.splashes.push(Splash { pos: V3::new(at.x, at.y, -h), time: 0.0, vel: self.impact_vel, mass: self.mass });
            }
            return;
        }
        let surface = V3::new(at.x, at.y, -h);
        // A fuel fireball is about 5.8 m³ᐟ³ across per kg^⅓ of fuel (Hasegawa and Sato's fit, used
        // for hydrocarbon fireballs); it scorches the ground it reaches.
        let fireball = 2.9 * self.fireball.cbrt();
        if self.fireball > 0.0 && above < fireball {
            self.scars.push(Scar { pos: surface, radius: fireball * 0.8, crater: 0.0, made: 0.0, burns_until: 0.0 });
        }
        // A hole where it went in: its size grows with the cube root of the energy lost (about
        // 1.5 m across for a light aircraft at 40 m/s).
        let energy = 0.5 * self.mass * self.severity * self.severity;
        if above < 3.0 && self.severity > 15.0 {
            let crater = 0.75 * (energy / 1.0e6).cbrt();
            self.scars.push(Scar { pos: surface + normalize(V3::new(self.impact_vel.x, self.impact_vel.y, 0.0) + V3::new(1e-9, 0.0, 0.0)) * crater, radius: crater * 2.5, crater, made: 0.0, burns_until: 0.0 });
        }
    }
}
