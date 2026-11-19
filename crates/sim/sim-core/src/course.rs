//! Race courses for multirotors: gates placed in a world, flown through in order, lap after lap.
//!
//! A course is data (`file`: `assets/courses/<key>.ron`): the world it is in, its frame (the home
//! aerodrome's runway, or the world's axes), where the racers start, how many laps, and its gates
//! in order, each a kind of `assets/courses/gates.ron` (`GateShape`: an arch, a square gate, a
//! hoop, a ladder, a flag to go round, a dive gate, a log to fly under, a high hoop, a hung hoop) placed, turned and scaled.
//!
//! - `Track` is a course placed on a terrain: each gate's foot on the ground and its axes in the
//!   world, and the gates as obstacles (`Track::obstacles`, bars of their frames' tubes) for
//!   `World::obstacles`: a quad runs into a frame as into a tree, crashing faster than its
//!   `crash_speed` and bouncing off slower.
//! - `Race` follows one racer round it from the positions it goes through: the gate it must fly
//!   through next (through the opening, the right way), the laps, the time of each lap and of each
//!   gate in it, a gate missed (flown past outside its opening, or another gate flown instead), the
//!   finish. Gate 0 is the start and finish: lap 1 runs from the start signal to the first time
//!   it is flown through again after every other gate, each next lap from there.
//! - `racer::Racer` is a pilot that flies a quad round a track with its self-levelling sticks: the
//!   proof that a course can be flown, and a baseline for trained ones.
//!
//! Pure functions of the course, the terrain and the positions: no clock of its own.

pub mod file;
pub mod racer;

use std::f64::consts::PI;

use serde::Deserialize;

use crate::math::{DEG, V3};
use crate::scenery::{Obstacle, ObstacleKind, Shape};
use crate::terrain::Terrain;

pub use file::{courses, shapes, Course, CourseGate, Frame, Place, Trail, TrailPoint};

/// The kinds of gate (`GateShape` says what each is).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
pub enum GateKind {
    /// The start and finish: a wide arch with a checkered banner.
    Arch,
    /// An upright square opening on legs.
    Square,
    /// A round hoop on a post.
    Round,
    /// Two square gates stacked: only the upper one counts.
    Ladder,
    /// A flag on a pole, flown round on one side.
    Flag,
    /// A level gate high on legs, flown down through.
    Dive,
    /// A log across two stumps, flown under.
    Log,
    /// A round hoop high on a pole.
    High,
    /// A round hoop hung from a beam between two posts.
    Hanging,
}

impl GateKind {
    pub const ALL: [GateKind; 9] = [GateKind::Arch, GateKind::Square, GateKind::Round, GateKind::Ladder, GateKind::Flag, GateKind::Dive, GateKind::Log, GateKind::High, GateKind::Hanging];

    /// Its place in `ALL` (and in the models' file).
    pub fn index(self) -> usize {
        GateKind::ALL.iter().position(|k| *k == self).unwrap_or(0)
    }

    /// Its word in the files and the models' nodes: `arch`, `square`...
    pub fn word(self) -> &'static str {
        match self {
            GateKind::Arch => "arch",
            GateKind::Square => "square",
            GateKind::Round => "round",
            GateKind::Ladder => "ladder",
            GateKind::Flag => "flag",
            GateKind::Dive => "dive",
            GateKind::Log => "log",
            GateKind::High => "high",
            GateKind::Hanging => "hanging",
        }
    }
}

/// Which side of a flag the racer keeps it on (a course's flags).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize)]
pub enum Side {
    /// The flag on the racer's left: it turns left round it.
    #[default]
    Left,
    Right,
}

/// What counts as flown through a kind of gate, in its axes (x the way through, y right, z up,
/// metres from its foot).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Opening {
    /// An upright rectangle across x: its middle (y, z) and half its width and height.
    Rect { middle: (f64, f64), half: (f64, f64) },
    /// An upright circle across x.
    Circle { middle: (f64, f64), radius: f64 },
    /// A level rectangle `z` up, flown down through: half its length (x) and width (y).
    Flat { z: f64, half: (f64, f64) },
    /// Beside a pole at the foot: the upright strip from it `reach` metres out to the side away
    /// from the flag (`Side`), up to `top`.
    Beside { reach: f64, top: f64 },
}

/// A piece of a kind's frame: tubes of the kind's `tube` radius, in its axes.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Part {
    /// Four tubes along the edges of an upright rectangle across x (their axes'), middle (y, z).
    Rect { middle: (f64, f64), half: (f64, f64) },
    /// A circle of tubes across x.
    Ring { middle: (f64, f64), radius: f64, segments: usize },
    /// Four tubes along the edges of a level rectangle `z` up: half along x and y.
    FlatRect { z: f64, half: (f64, f64) },
    /// One tube.
    Bar { from: (f64, f64, f64), to: (f64, f64, f64) },
}

/// A kind of gate (`assets/courses/gates.ron`).
#[derive(Clone, Debug, PartialEq)]
pub struct GateShape {
    pub kind: GateKind,
    /// For the player.
    pub name: String,
    pub opening: Opening,
    /// Radius of its tubes, m.
    pub tube: f64,
    pub parts: Vec<Part>,
    /// Where its number is shown, its axes: the middle of the plate, facing the racers coming.
    pub number: (f64, f64, f64),
}

impl GateShape {
    /// The axes of its tubes, in its axes, from the lower end.
    pub fn bars(&self) -> Vec<(V3, V3)> {
        let mut bars = Vec::new();
        let up = |(a, b): (V3, V3)| if a.z <= b.z { (a, b) } else { (b, a) };
        for part in &self.parts {
            match *part {
                Part::Rect { middle: (y, z), half: (w, h) } => {
                    let c = [V3::new(0.0, y - w, z - h), V3::new(0.0, y + w, z - h), V3::new(0.0, y + w, z + h), V3::new(0.0, y - w, z + h)];
                    for i in 0..4 {
                        bars.push(up((c[i], c[(i + 1) % 4])));
                    }
                }
                Part::Ring { middle: (y, z), radius, segments } => {
                    let at = |i: usize| {
                        let t = 2.0 * PI * i as f64 / segments.max(3) as f64;
                        V3::new(0.0, y + radius * t.sin(), z - radius * t.cos())
                    };
                    for i in 0..segments.max(3) {
                        bars.push(up((at(i), at(i + 1))));
                    }
                }
                Part::FlatRect { z, half: (a, b) } => {
                    let c = [V3::new(-a, -b, z), V3::new(a, -b, z), V3::new(a, b, z), V3::new(-a, b, z)];
                    for i in 0..4 {
                        bars.push((c[i], c[(i + 1) % 4]));
                    }
                }
                Part::Bar { from, to } => bars.push(up((V3::new(from.0, from.1, from.2), V3::new(to.0, to.1, to.2)))),
            }
        }
        bars
    }
}

/// A gate placed in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGate {
    pub kind: GateKind,
    /// The ground under its middle, world axes: north, east, down.
    pub foot: V3,
    /// The way through it, radians from north.
    pub heading: f64,
    pub scale: f64,
    /// A flag's side.
    pub side: Side,
}

/// What a move did at a gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Crossing {
    /// Through its opening, the right way.
    Through,
    /// Past it the right way, outside its opening: this many metres from it.
    Outside(f64),
}

impl PlacedGate {
    /// Forward (the way through, level), right and up, world axes.
    pub fn axes(&self) -> (V3, V3, V3) {
        let (c, s) = (self.heading.cos(), self.heading.sin());
        (V3::new(c, s, 0.0), V3::new(-s, c, 0.0), V3::new(0.0, 0.0, -1.0))
    }

    /// A point of its axes (x through, y right, z up, unscaled) in the world.
    pub fn to_world(&self, p: V3) -> V3 {
        let (f, r, u) = self.axes();
        self.foot + (f * p.x + r * p.y + u * p.z) * self.scale
    }

    /// A point of the world in its axes, unscaled.
    pub fn local(&self, p: V3) -> V3 {
        let (f, r, u) = self.axes();
        let d = (p - self.foot) / self.scale;
        V3::new(d.dot(f), d.dot(r), d.dot(u))
    }

    /// The middle of its opening and the way through it, world axes (a unit vector).
    pub fn target(&self, shape: &GateShape) -> (V3, V3) {
        let (f, _, u) = self.axes();
        match shape.opening {
            Opening::Rect { middle: (y, z), .. } | Opening::Circle { middle: (y, z), .. } => (self.to_world(V3::new(0.0, y, z)), f),
            Opening::Flat { z, .. } => (self.to_world(V3::new(0.0, 0.0, z)), -u),
            // A few metres off the pole, at a racer's height.
            Opening::Beside { reach, top } => {
                let away = match self.side {
                    Side::Left => 1.0,
                    Side::Right => -1.0,
                };
                (self.to_world(V3::new(0.0, away * (reach * 0.2).min(5.0), (top * 0.2).min(3.0))), f)
            }
        }
    }

    /// Whether going from `from` to `to` (world axes) went through it the right way, or past it
    /// outside its opening (within `near` metres of it), or neither.
    pub fn crossed(&self, shape: &GateShape, from: V3, to: V3, near: f64) -> Option<Crossing> {
        let (a, b) = (self.local(from), self.local(to));
        // The plane of the opening, and how far each end is past it the way through.
        let (before, after) = match shape.opening {
            Opening::Flat { z, .. } => (z - a.z, z - b.z),
            _ => (a.x, b.x),
        };
        if !(before < 0.0 && after >= 0.0) {
            return None;
        }
        let k = -before / (after - before);
        let p = a + (b - a) * k;
        let near = near / self.scale;
        let off = match shape.opening {
            Opening::Rect { middle: (y, z), half: (w, h) } => ((p.y - y).abs() - w).max((p.z - z).abs() - h),
            Opening::Circle { middle: (y, z), radius } => (p.y - y).hypot(p.z - z) - radius,
            Opening::Flat { half: (l, w), .. } => (p.x.abs() - l).max(p.y.abs() - w),
            Opening::Beside { reach, top } => {
                let out = match self.side {
                    Side::Left => p.y,
                    Side::Right => -p.y,
                };
                // On the flag's side of the pole is missing it; beyond the strip is not near it.
                if out < 0.0 {
                    if -out < near {
                        return Some(Crossing::Outside(-out * self.scale));
                    }
                    return None;
                }
                if out > reach || p.z > top {
                    return None;
                }
                -1.0
            }
        };
        if off <= 0.0 {
            Some(Crossing::Through)
        } else if off < near {
            Some(Crossing::Outside(off * self.scale))
        } else {
            None
        }
    }
}

/// A course placed on a terrain.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub course: Course,
    pub gates: Vec<PlacedGate>,
    /// Where the racers start, on the ground (world axes), and facing.
    pub start: V3,
    pub start_heading: f64,
}

impl Track {
    /// Places a course on a terrain: its frame's origin (the home aerodrome's runway), each gate's
    /// foot on the ground there.
    pub fn new(course: &Course, terrain: &Terrain) -> Track {
        let (origin_n, origin_e, axis) = course.frame.origin(terrain);
        let (c, s) = (axis.cos(), axis.sin());
        let place = |x: f64, y: f64| (origin_n + x * c - y * s, origin_e + x * s + y * c);
        let gates = course
            .gates
            .iter()
            .map(|g| {
                let (n, e) = place(g.x, g.y);
                let ground = terrain.height(n, e).max(0.0);
                PlacedGate { kind: g.kind, foot: V3::new(n, e, -ground), heading: axis + g.heading * DEG, scale: g.scale, side: g.side }
            })
            .collect();
        let (n, e) = place(course.start.0, course.start.1);
        Track { course: course.clone(), gates, start: V3::new(n, e, -terrain.height(n, e).max(0.0)), start_heading: axis + course.start.2 * DEG }
    }

    /// The shape of gate `i`.
    pub fn shape(&self, i: usize) -> &'static GateShape {
        shape(self.gates[i].kind)
    }

    /// Every tube of every gate as an obstacle, for `World::obstacles`.
    pub fn obstacles(&self) -> Vec<Obstacle> {
        let mut list = Vec::new();
        for g in &self.gates {
            let shape = shape(g.kind);
            for (a, b) in shape.bars() {
                let (a, b) = (g.to_world(a), g.to_world(b));
                list.push(Obstacle { kind: ObstacleKind::Gate, n: a.x, e: a.y, base: -a.z, shape: Shape::Bar { dn: b.x - a.x, de: b.y - a.y, up: a.z - b.z, radius: shape.tube * g.scale } });
            }
        }
        list
    }

    /// How long the lap is along the middles of the openings, m.
    pub fn lap_length(&self) -> f64 {
        let n = self.gates.len();
        (0..n).map(|i| (self.target(i).0 - self.target((i + 1) % n).0).length()).sum()
    }

    /// The middle of gate `i`'s opening and the way through it.
    pub fn target(&self, i: usize) -> (V3, V3) {
        self.gates[i].target(self.shape(i))
    }
}

/// The shape of a kind of gate (`file::shapes`).
pub fn shape(kind: GateKind) -> &'static GateShape {
    shapes().iter().find(|s| s.kind == kind).expect("every kind of gate has its shape")
}

/// What happened in a move (`Race::update`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Through gate `gate` (the start and finish at the start), so many seconds into the lap.
    Gate { gate: usize, split: f64 },
    /// Lap `lap` (1 the first) done in so many seconds.
    Lap { lap: u32, time: f64 },
    /// Every lap done: the whole race's time and the best lap's.
    Finished { total: f64, best: f64 },
    /// The next gate was missed: flown past outside its opening, or another flown instead.
    Missed { gate: usize },
}

/// A racer round a track: see the module.
#[derive(Clone, Debug, PartialEq)]
pub struct Race {
    pub laps: u32,
    /// The gate to fly through next.
    pub next: usize,
    /// Laps done.
    pub lap: u32,
    /// When the start signal was given, and the lap now flown began.
    pub started: Option<f64>,
    lap_start: f64,
    /// Each lap done, seconds.
    pub lap_times: Vec<f64>,
    /// The time into the lap each gate of this lap was flown through.
    pub splits: Vec<f64>,
    /// The last lap's, to compare with.
    pub last_splits: Vec<f64>,
    /// The race's time, once every lap is done.
    pub finished: Option<f64>,
    /// Gates flown through since the start, the start's too.
    pub passed: u32,
    last: Option<V3>,
}

/// A move past a gate's plane outside its opening within this many metres of it is a miss.
pub const MISS_NEAR: f64 = 12.0;

impl Race {
    pub fn new(laps: u32) -> Race {
        Race { laps: laps.max(1), next: 0, lap: 0, started: None, lap_start: 0.0, lap_times: Vec::new(), splits: Vec::new(), last_splits: Vec::new(), finished: None, passed: 0, last: None }
    }

    /// The start signal at `time`, the racer at `pos`.
    pub fn go(&mut self, time: f64, pos: V3) {
        *self = Race::new(self.laps);
        self.started = Some(time);
        self.lap_start = time;
        self.last = Some(pos);
    }

    /// Seconds since the start signal (the whole race's once finished).
    pub fn elapsed(&self, time: f64) -> f64 {
        match (self.finished, self.started) {
            (Some(total), _) => total,
            (None, Some(start)) => time - start,
            (None, None) => 0.0,
        }
    }

    /// Seconds into the lap now flown.
    pub fn lap_time(&self, time: f64) -> f64 {
        if self.started.is_none() || self.finished.is_some() {
            return 0.0;
        }
        time - self.lap_start
    }

    /// The quickest lap so far.
    pub fn best(&self) -> Option<f64> {
        self.lap_times.iter().copied().reduce(f64::min)
    }

    /// The racer is at `pos` at `time`: what it did since the last position.
    pub fn update(&mut self, track: &Track, pos: V3, time: f64) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(from) = self.last.replace(pos) else { return events };
        if self.started.is_none() || self.finished.is_some() || (pos - from).length() > 50.0 {
            return events;
        }
        let n = track.gates.len();
        let next = self.next;
        match track.gates[next].crossed(track.shape(next), from, pos, MISS_NEAR) {
            Some(Crossing::Through) => {
                self.passed += 1;
                let split = time - self.lap_start;
                if next == 0 && self.passed > 1 {
                    // The finish line: a lap done.
                    self.lap += 1;
                    self.lap_times.push(split);
                    events.push(Event::Lap { lap: self.lap, time: split });
                    self.last_splits = std::mem::take(&mut self.splits);
                    self.lap_start = time;
                    if self.lap >= self.laps {
                        let total = time - self.started.unwrap_or(time);
                        self.finished = Some(total);
                        events.push(Event::Finished { total, best: self.best().unwrap_or(total) });
                        return events;
                    }
                } else {
                    self.splits.push(split);
                }
                events.push(Event::Gate { gate: next, split: if next == 0 && self.passed > 1 { 0.0 } else { split } });
                self.next = (next + 1) % n;
            }
            Some(Crossing::Outside(_)) => events.push(Event::Missed { gate: next }),
            None => {
                // Another gate flown through instead of the next one.
                let other = (0..n).filter(|i| *i != next).any(|i| track.gates[i].crossed(track.shape(i), from, pos, 0.0) == Some(Crossing::Through));
                if other {
                    events.push(Event::Missed { gate: next });
                }
            }
        }
        events
    }

    /// How far round the race the racer is, in gates: those flown through and the share of the
    /// way to the next one, from the last (or the start) to it.
    pub fn progress(&self, track: &Track, pos: V3) -> f64 {
        let n = track.gates.len();
        let (to, _) = track.target(self.next);
        let from = if self.passed == 0 { track.start } else { track.target((self.next + n - 1) % n).0 };
        let leg = (to - from).length().max(1.0);
        self.passed as f64 + (1.0 - (to - pos).length() / leg).clamp(-1.0, 1.0)
    }
}

/// A stretch of a course's trail through the forest, in the world (`file::Trail`): from and to
/// (north, east), half the trail's width at each end, and the height crowns are cleared to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub half: (f64, f64),
    pub clearance: f64,
    /// Within `band` metres of it the forest is `dense` times as thick (`file::Trail`).
    pub dense: f64,
    pub band: f64,
}

impl Cut {
    /// How far (`n`, `e`) is from its line, and half the trail's width there.
    pub fn distance(&self, n: f64, e: f64) -> (f64, f64) {
        let (dn, de) = (self.to.0 - self.from.0, self.to.1 - self.from.1);
        let length2 = dn * dn + de * de;
        let t = if length2 > 0.0 { (((n - self.from.0) * dn + (e - self.from.1) * de) / length2).clamp(0.0, 1.0) } else { 0.0 };
        let (pn, pe) = (self.from.0 + t * dn, self.from.1 + t * de);
        ((n - pn).hypot(e - pe), self.half.0 + (self.half.1 - self.half.0) * t)
    }
}

/// The trails cut by the courses of a terrain's world, as stretches: those of every course whose
/// place is this generated world, its frame placed on this terrain. (A region's courses would need
/// its ICAO code to be told apart: none has a trail yet.)
pub fn cuts(terrain: &Terrain) -> Vec<Cut> {
    let mut list = Vec::new();
    if terrain.region().is_some() {
        return list;
    }
    for course in file::courses() {
        let (Place::Seed(seed), Some(trail)) = (&course.place, &course.trail) else { continue };
        if *seed != terrain.seed() {
            continue;
        }
        let (origin_n, origin_e, axis) = course.frame.origin(terrain);
        let (c, s) = (axis.cos(), axis.sin());
        let at = |p: &file::TrailPoint| (origin_n + p.x * c - p.y * s, origin_e + p.x * s + p.y * c);
        let n = trail.points.len();
        let stretches = if trail.closed { n } else { n - 1 };
        for i in 0..stretches {
            let (a, b) = (&trail.points[i], &trail.points[(i + 1) % n]);
            let clearance = a.clearance.unwrap_or(trail.clearance).max(b.clearance.unwrap_or(trail.clearance));
            list.push(Cut { from: at(a), to: at(b), half: (a.width / 2.0, b.width / 2.0), clearance, dense: trail.dense, band: trail.band });
        }
    }
    list
}

/// How many spots for a tree a cell of the forest whose middle is (`n`, `e`) has: one, or a
/// trail's `dense` within its `band`.
pub fn thicket(cuts: &[Cut], n: f64, e: f64) -> usize {
    cuts.iter().filter(|c| c.distance(n, e).0 < c.band).map(|c| c.dense.round().max(1.0) as usize).max().unwrap_or(1)
}

/// The starts of the courses of a terrain's world (north, east): where their pads are.
pub fn starts(terrain: &Terrain) -> Vec<(f64, f64)> {
    if terrain.region().is_some() {
        return Vec::new();
    }
    file::courses()
        .iter()
        .filter(|c| c.place == Place::Seed(terrain.seed()))
        .map(|c| {
            let (n, e, axis) = c.frame.origin(terrain);
            let (cos, sin) = (axis.cos(), axis.sin());
            (n + c.start.0 * cos - c.start.1 * sin, e + c.start.0 * sin + c.start.1 * cos)
        })
        .collect()
}

/// Whether a tree at (`n`, `e`) whose crown starts `crown` metres up and reaches `radius` from its
/// trunk was cut for a trail (`file::Trail`): its trunk in the trail, or a low crown reaching over
/// it (its branches trimmed back to the trail's side, as a cut trail's are).
pub fn cut_down(cuts: &[Cut], n: f64, e: f64, crown: f64, radius: f64) -> bool {
    cuts.iter().any(|c| {
        let (d, half) = c.distance(n, e);
        d < half || (crown < c.clearance && d - radius < half)
    })
}
