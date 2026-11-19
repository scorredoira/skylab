//! Race circuits as data: a centre line through points, the width of the asphalt, kerbs, run-off
//! areas and barriers either side, placed in a world (`file`: `assets/circuits/<key>.ron`).
//!
//! `Circuit::new(file, terrain)` lays one out on a terrain: the centre line as a smooth closed curve
//! (a centripetal Catmull-Rom spline through the points) sampled every `STEP` metres, with at each
//! sample its width, its height (the natural ground along it, smoothed, so the circuit rises and
//! falls with the land without its bumps), its cross slope (a fall to one side on the straights for
//! the rain, banked into the corners), its kerbs (auto: wherever it turns tighter than
//! `KERB_RADIUS`), what lies either side up to the barrier and the barrier itself. It answers:
//!
//! - `patch(n, e)`: the ground of the circuit at a point (height, normal, `Surface`), from the
//!   asphalt to the grass behind the barriers; `None` off it;
//! - `earthwork(n, e, natural)`: the ground the terrain is cut and filled to under and round it
//!   (`Terrain::with_circuits`), so it sits on the land: level with it under the circuit, a bank
//!   back to the natural ground beyond;
//! - `walls(n, e, reach)`: the barriers near a point, for the cars to run into;
//! - `locate(n, e)`: where along it and how far across a point is; `lap` and `Timing`: laps,
//!   sectors, the start line and the grid.
//!
//! Axes: north, east, metres; `along` from the first point, `across` positive to the right of the
//! way the circuit is driven.

pub mod file;
pub mod race;

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

pub use file::{circuits, circuits_with, load, set_circuits, BarrierKind, CircuitFile, Kerbs, Place, PointFile, Prop, PropKind, Runoff, Side, SideChange, SideSpec};

use crate::car::ground::{Patch, Surface, Wall, WallKind};
use crate::math::{lerp, smoothstep, V3};
use crate::terrain::Terrain;

/// Metres between two samples of the centre line.
pub const STEP: f64 = 2.0;
/// Width of a kerb, metres from the asphalt's edge.
pub const KERB_WIDTH: f64 = 1.2;
/// Corners tighter than this (radius, metres) get kerbs on both sides when a side's kerbs are `Auto`.
pub const KERB_RADIUS: f64 = 160.0;
/// How far behind a barrier the circuit's ground goes on (grass, posts, fences), metres.
pub const VERGE: f64 = 5.0;
/// The most the ground of a circuit reaches from its centre line, with its banks, metres.
pub const MOST_REACH: f64 = 140.0;
/// How much lower than the circuit's own surface the terrain is laid under it, so that the
/// terrain's coarser triangles never show through, metres.
pub const UNDER: f64 = 0.35;
/// Side of the squares the samples are found by, metres.
const CELL: f64 = 24.0;

/// One sample of the centre line and everything across it there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Where it is (north, east) and its height above the sea.
    pub n: f64,
    pub e: f64,
    pub height: f64,
    /// Metres along the circuit from its first point.
    pub along: f64,
    /// Its direction, radians from north.
    pub heading: f64,
    /// Turning, radians per metre: positive to the right.
    pub curvature: f64,
    /// Width of the asphalt, metres.
    pub width: f64,
    /// Rise of the ground per metre across, to the right: negative falls to the right.
    pub cross: f64,
    /// Left side then right side.
    pub sides: [SampleSide; 2],
}

/// One side of the circuit at a sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleSide {
    pub kerb: bool,
    pub runoff: Runoff,
    /// Metres from the asphalt's edge to the barrier (or to the end of the run-off if there is none).
    pub runoff_width: f64,
    pub barrier: BarrierKind,
    /// Metres from the centre line to the end of the circuit's own ground (behind the barrier).
    pub reach: f64,
    /// The pit lane's ground behind the barrier: paved instead of grass.
    pub pit: bool,
}

impl SampleSide {
    /// Metres from the centre line to the barrier's face, on a circuit of this width.
    pub fn barrier_at(&self, width: f64) -> f64 {
        width / 2.0 + self.runoff_width
    }
}

/// Where a point is on a circuit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    /// Metres along from the first point, in [0, length).
    pub along: f64,
    /// Metres to the right of the centre line.
    pub across: f64,
    /// The sample before it and how far it is to the next (0 to 1).
    pub index: usize,
    pub t: f64,
}

/// A circuit laid out on a terrain: see the module.
#[derive(Clone, Debug)]
pub struct Circuit {
    pub key: String,
    pub name: String,
    pub description: String,
    /// Metres round.
    pub length: f64,
    pub samples: Vec<Sample>,
    /// Metres along of the start and finish line, and of the two lines that split a lap in three sectors.
    pub start: f64,
    pub sectors: [f64; 2],
    pub laps: u32,
    /// The things placed beside it (stands, the gantry, the pits' buildings, marshals' posts), for
    /// the world's drawing: where along, which side and how far out.
    pub props: Vec<Prop>,
    /// Its extent: lowest and highest north and east, with `MOST_REACH` round it.
    pub bounds: [f64; 4],
    /// The samples whose segments reach into each square of `CELL`, for `locate`.
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// The barriers' pieces, and those near each square.
    walls: Vec<Wall>,
    wall_cells: HashMap<(i32, i32), Vec<u32>>,
}

impl Circuit {
    /// Lays out a circuit's file on a terrain (the terrain before any circuit changed it: its
    /// heights are what the circuit's follow).
    pub fn new(file: &CircuitFile, terrain: &Terrain) -> Circuit {
        let (on, oe) = file.origin;
        let points: Vec<(f64, f64)> = file.points.iter().map(|p| (on + p.n, oe + p.e)).collect();
        let line = spline(&points);
        let (curve, starts) = resample(&line, &points);
        let count = curve.len();
        let length = count as f64 * STEP;

        // Each point's settings, carried on to the next point that changes them.
        let mut width = Vec::with_capacity(points.len());
        let mut lift = Vec::with_capacity(points.len());
        let mut bank = Vec::with_capacity(points.len());
        let mut sides: Vec<[SideSpec; 2]> = Vec::with_capacity(points.len());
        let (mut w, mut s) = (file.width, [file.sides; 2]);
        for p in &file.points {
            w = p.width.unwrap_or(w);
            if let Some(l) = p.left {
                s[0] = s[0].with(&l);
            }
            if let Some(r) = p.right {
                s[1] = s[1].with(&r);
            }
            if let Some(b) = p.both {
                s[0] = s[0].with(&b);
                s[1] = s[1].with(&b);
            }
            width.push(w);
            lift.push(p.lift.unwrap_or(0.0));
            bank.push(p.bank.map(|b| b / 100.0));
            sides.push(s);
        }
        // Between two points: the widths and lifts change smoothly, the kinds at the point.
        let segment_of = |i: usize| -> (usize, f64) {
            let k = starts.iter().rposition(|&st| st <= i).unwrap_or(0);
            let (a, b) = (starts[k], if k + 1 < starts.len() { starts[k + 1] } else { count + starts[0] });
            (k, (i - a) as f64 / (b - a).max(1) as f64)
        };
        let n_points = points.len();

        // Curvature, smoothed over 10 m.
        let mut curvature: Vec<f64> = (0..count)
            .map(|i| {
                let (a, b) = (curve[(i + count - 1) % count].2, curve[(i + 1) % count].2);
                remainder_pi(b - a) / (2.0 * STEP)
            })
            .collect();
        curvature = smooth(&curvature, 5, 1);

        // Heights: the natural ground along the centre line, smoothed over some 150 m, and lifted
        // where the file says.
        let natural: Vec<f64> = curve.iter().map(|&(n, e, _)| terrain.height(n, e).max(1.0)).collect();
        let mut height = smooth(&natural, (file.smoothing / STEP / 2.0).round().max(1.0) as usize, 4);
        for (i, h) in height.iter_mut().enumerate() {
            let (k, t) = segment_of(i);
            *h += lerp(lift[k], lift[(k + 1) % n_points], t);
        }
        height = smooth(&height, 10, 2);

        // Cross slope: a fall to the right on the straights, banked into the corners; the file's
        // bank where it gives one.
        let mut cross: Vec<f64> = (0..count)
            .map(|i| {
                let k = curvature[i];
                let into = -k.signum() * (k.abs() * 3.0).min(0.035);
                let corner = (k.abs() * 160.0).min(1.0);
                let (seg, _) = segment_of(i);
                bank[seg].unwrap_or(lerp(-file.fall / 100.0, into, corner))
            })
            .collect();
        cross = smooth(&cross, 8, 2);

        // Kerbs where the corners are tight (on `Auto` sides), widened along by 16 m.
        let tight: Vec<bool> = curvature.iter().map(|k| k.abs() > 1.0 / KERB_RADIUS).collect();
        let spread = 8usize;
        let near_tight = |i: usize| (0..=2 * spread).any(|j| tight[(i + count + j - spread) % count]);

        let mut samples = Vec::with_capacity(count);
        for i in 0..count {
            let (n, e, heading) = curve[i];
            let (k, t) = segment_of(i);
            let next = (k + 1) % n_points;
            let w = lerp(width[k], width[next], t);
            let radius = 1.0 / curvature[i].abs().max(1e-6);
            let mut both = [SampleSide { kerb: false, runoff: Runoff::Grass, runoff_width: 0.0, barrier: BarrierKind::None, reach: 0.0, pit: false }; 2];
            for (side, out) in both.iter_mut().enumerate() {
                let spec = sides[k][side];
                let to = sides[next][side];
                let mut runoff_width = lerp(spec.runoff_width, to.runoff_width, t);
                let inside = (side == 1) == (curvature[i] > 0.0);
                // On the inside of a tight corner there is only room for so much.
                let room = if inside { radius * 0.8 - w / 2.0 - VERGE } else { f64::INFINITY };
                let mut barrier = spec.barrier;
                if runoff_width > room {
                    runoff_width = room.max(2.0);
                    if runoff_width < 6.0 {
                        barrier = BarrierKind::None;
                    }
                }
                let kerb = match spec.kerbs {
                    Kerbs::Auto => near_tight(i),
                    Kerbs::Always => true,
                    Kerbs::Never => false,
                };
                let pit = spec.pit;
                let behind = if pit { VERGE + 16.0 } else { VERGE };
                let reach = (w / 2.0 + runoff_width + behind).min(if inside { radius * 0.95 } else { f64::INFINITY }).max(w / 2.0 + 2.0);
                *out = SampleSide { kerb, runoff: spec.runoff, runoff_width, barrier, reach, pit };
            }
            samples.push(Sample { n, e, height: height[i], along: i as f64 * STEP, heading, curvature: curvature[i], width: w, cross: cross[i], sides: both });
        }

        let start = file.start.rem_euclid(length);
        let sectors = [(start + length / 3.0) % length, (start + 2.0 * length / 3.0) % length];
        let mut circuit = Circuit {
            key: file.key.clone(),
            name: file.name.clone(),
            description: file.description.clone(),
            length,
            samples,
            start,
            sectors,
            laps: file.laps,
            props: file.props.clone(),
            bounds: [0.0; 4],
            cells: HashMap::new(),
            walls: Vec::new(),
            wall_cells: HashMap::new(),
        };
        circuit.index();
        circuit.build_walls();
        circuit
    }

    /// Finds the samples by square, and the circuit's bounds.
    fn index(&mut self) {
        let count = self.samples.len();
        let mut bounds = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
        for s in &self.samples {
            bounds[0] = bounds[0].min(s.n - MOST_REACH);
            bounds[1] = bounds[1].max(s.n + MOST_REACH);
            bounds[2] = bounds[2].min(s.e - MOST_REACH);
            bounds[3] = bounds[3].max(s.e + MOST_REACH);
        }
        self.bounds = bounds;
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for i in 0..count {
            let (a, b) = (&self.samples[i], &self.samples[(i + 1) % count]);
            let r = MOST_REACH;
            let cell = |v: f64| (v / CELL).floor() as i32;
            for x in cell(a.n.min(b.n) - r)..=cell(a.n.max(b.n) + r) {
                for y in cell(a.e.min(b.e) - r)..=cell(a.e.max(b.e) + r) {
                    // Only the squares within reach of the segment.
                    let (cn, ce) = ((x as f64 + 0.5) * CELL, (y as f64 + 0.5) * CELL);
                    if segment_distance(cn, ce, a.n, a.e, b.n, b.e).0 < r + CELL * 0.71 {
                        cells.entry((x, y)).or_default().push(i as u32);
                    }
                }
            }
        }
        self.cells = cells;
    }

    /// The barriers as pieces of wall, one per sample where a side has one.
    fn build_walls(&mut self) {
        let count = self.samples.len();
        let mut walls = Vec::new();
        for i in 0..count {
            let (a, b) = (&self.samples[i], &self.samples[(i + 1) % count]);
            for side in 0..2 {
                let (sa, sb) = (&a.sides[side], &b.sides[side]);
                let kind = match (sa.barrier, sb.barrier) {
                    (BarrierKind::None, _) | (_, BarrierKind::None) => continue,
                    (k, _) => k,
                };
                let sign = if side == 0 { -1.0 } else { 1.0 };
                let (da, db) = (sign * sa.barrier_at(a.width), sign * sb.barrier_at(b.width));
                let pa = (a.n - a.heading.sin() * da, a.e + a.heading.cos() * da);
                let pb = (b.n - b.heading.sin() * db, b.e + b.heading.cos() * db);
                let base = (a.height + a.cross * da).min(b.height + b.cross * db);
                // The normal points back to the track: left of a right-hand wall.
                let (tn, te) = (pb.0 - pa.0, pb.1 - pa.1);
                let len = tn.hypot(te).max(1e-9);
                let right = (-te / len, tn / len);
                let normal = if side == 1 { (-right.0, -right.1) } else { right };
                let (height, wall) = match kind {
                    BarrierKind::Armco => (0.75, WallKind::Armco),
                    BarrierKind::Tyres => (1.0, WallKind::Tyres),
                    BarrierKind::Wall => (1.1, WallKind::Concrete),
                    BarrierKind::None => unreachable!(),
                };
                walls.push(Wall { a: pa, b: pb, base, height, normal, kind: wall });
            }
        }
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let cell = |v: f64| (v / CELL).floor() as i32;
        for (k, w) in walls.iter().enumerate() {
            let r = 6.0;
            for x in cell(w.a.0.min(w.b.0) - r)..=cell(w.a.0.max(w.b.0) + r) {
                for y in cell(w.a.1.min(w.b.1) - r)..=cell(w.a.1.max(w.b.1) + r) {
                    cells.entry((x, y)).or_default().push(k as u32);
                }
            }
        }
        self.walls = walls;
        self.wall_cells = cells;
    }

    /// Every piece of barrier.
    pub fn all_walls(&self) -> &[Wall] {
        &self.walls
    }

    /// The barriers within `reach` (at most 6 m beyond their own square) of a point.
    pub fn walls(&self, n: f64, e: f64, reach: f64, out: &mut Vec<Wall>) {
        if n < self.bounds[0] || n > self.bounds[1] || e < self.bounds[2] || e > self.bounds[3] {
            return;
        }
        let key = ((n / CELL).floor() as i32, (e / CELL).floor() as i32);
        if let Some(list) = self.wall_cells.get(&key) {
            for &k in list {
                let w = self.walls[k as usize];
                if segment_distance(n, e, w.a.0, w.a.1, w.b.0, w.b.1).0 <= reach {
                    out.push(w);
                }
            }
        }
    }

    /// Where a point is: along and across the centre line it is nearest to (within
    /// `MOST_REACH`), or `None` beyond.
    pub fn locate(&self, n: f64, e: f64) -> Option<Located> {
        if n < self.bounds[0] || n > self.bounds[1] || e < self.bounds[2] || e > self.bounds[3] {
            return None;
        }
        let list = self.cells.get(&((n / CELL).floor() as i32, (e / CELL).floor() as i32))?;
        let count = self.samples.len();
        let mut best: Option<(f64, usize, f64)> = None;
        for &i in list {
            let i = i as usize;
            let (a, b) = (&self.samples[i], &self.samples[(i + 1) % count]);
            let (d, t) = segment_distance(n, e, a.n, a.e, b.n, b.e);
            if best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, i, t));
            }
        }
        let (dist, i, t) = best?;
        if dist > MOST_REACH {
            return None;
        }
        let (a, b) = (&self.samples[i], &self.samples[(i + 1) % count]);
        let heading = lerp_angle(a.heading, b.heading, t);
        let (pn, pe) = (lerp(a.n, b.n, t), lerp(a.e, b.e, t));
        let across = (n - pn) * -heading.sin() + (e - pe) * heading.cos();
        Some(Located { along: (a.along + t * STEP) % self.length, across, index: i, t })
    }

    /// The samples either side of a located point, and how far between them.
    fn between(&self, at: &Located) -> (&Sample, &Sample) {
        (&self.samples[at.index], &self.samples[(at.index + 1) % self.samples.len()])
    }

    /// The side (0 left, 1 right) of a point across, and the values of that side interpolated.
    fn side_at(&self, at: &Located) -> (usize, SampleSide, f64) {
        let (a, b) = self.between(at);
        let side = if at.across < 0.0 { 0 } else { 1 };
        let (sa, sb) = (a.sides[side], b.sides[side]);
        let near = if at.t < 0.5 { sa } else { sb };
        let s = SampleSide {
            runoff_width: lerp(sa.runoff_width, sb.runoff_width, at.t),
            reach: lerp(sa.reach, sb.reach, at.t),
            ..near
        };
        (side, s, lerp(a.width, b.width, at.t))
    }

    /// The plane of the circuit across a located point: its height there without kerbs, its rise
    /// along (per metre) and across.
    fn plane(&self, at: &Located) -> (f64, f64, f64) {
        let (a, b) = self.between(at);
        let cross = lerp(a.cross, b.cross, at.t);
        let h = lerp(a.height, b.height, at.t) + cross * at.across;
        // The grade along, and the cross slope that is in the same plane.
        let grade = (b.height - a.height + (b.cross - a.cross) * at.across) / STEP;
        (h, grade, cross)
    }

    /// What the circuit's ground is at a point, how far across: the asphalt, a kerb, the run-off,
    /// the pit lane or the verge; `None` beyond its reach.
    pub fn surface_at(&self, at: &Located) -> Option<(Surface, f64)> {
        let (_, side, width) = self.side_at(at);
        let x = at.across.abs();
        if x > side.reach {
            return None;
        }
        let edge = width / 2.0;
        let barrier = side.barrier_at(width);
        let mut kerb = 0.0;
        let surface = if x <= edge {
            Surface::Asphalt
        } else if side.kerb && x <= edge + KERB_WIDTH {
            let u = (x - edge) / KERB_WIDTH;
            let (a, _) = self.between(at);
            let along = a.along + at.t * STEP;
            // A low ramp up to a flat top with ridges across it every 0.7 m, back down at its outer edge.
            let ridge = 0.5 + 0.5 * (along * TAU / 0.7).sin();
            kerb = 0.03 * smoothstep(0.0, 0.3, u) * (1.0 - smoothstep(0.85, 1.0, u)) + 0.012 * ridge * smoothstep(0.1, 0.35, u) * (1.0 - smoothstep(0.85, 1.0, u));
            Surface::Kerb
        } else if x <= barrier {
            match side.runoff {
                Runoff::Grass => Surface::Grass,
                Runoff::Gravel => Surface::Gravel,
                Runoff::Asphalt => Surface::Asphalt,
            }
        } else if side.pit && x > barrier + 1.5 {
            Surface::Concrete
        } else {
            Surface::Grass
        };
        Some((surface, kerb))
    }

    /// The circuit's ground at a point, or `None` off it.
    pub fn patch(&self, n: f64, e: f64) -> Option<Patch> {
        self.patch_located(n, e).map(|(p, _)| p)
    }

    /// How much of a tyre's width (taken as 0.24 m) there is on paint at a located point of the
    /// asphalt, 0 to 1: the lines along its edges (10 cm wide, 10 cm in from the edge) and the
    /// start's chequered line (1.2 m long, half of it white), as `track.wgsl` paints them.
    pub fn paint(&self, at: &Located) -> f64 {
        let (_, _, width) = self.side_at(at);
        let x = at.across.abs();
        let half = width / 2.0;
        let overlap = |a: f64, b: f64| ((x + 0.12).min(b) - (x - 0.12).max(a)).max(0.0) / 0.24;
        let mut paint = overlap(half - 0.2, half - 0.1);
        let from_start = remainder_pi_len(at.along - self.start, self.length);
        if from_start.abs() < 0.6 && x < half {
            paint = paint.max(0.5);
        }
        paint
    }

    /// The circuit's ground at a point and where on it the point is, or `None` off it.
    pub fn patch_located(&self, n: f64, e: f64) -> Option<(Patch, Located)> {
        let at = self.locate(n, e)?;
        let (surface, kerb) = self.surface_at(&at)?;
        let (h, grade, cross) = self.plane(&at);
        let (a, b) = self.between(&at);
        let heading = lerp_angle(a.heading, b.heading, at.t);
        let (tn, te) = (heading.cos(), heading.sin());
        let (rn, re) = (-te, tn);
        let dn = grade * tn + cross * rn;
        let de = grade * te + cross * re;
        let normal = V3::new(-dn, -de, -1.0).normalize();
        let paint = if surface == Surface::Asphalt { self.paint(&at) } else { 0.0 };
        Some((Patch { height: h + kerb, normal, surface, exact: true, water: 0.0, paint }, at))
    }

    /// The ground the terrain is laid to at a point, and how much of it (1 under the circuit,
    /// falling to 0 at the end of its bank), given the natural height there; `None` beyond.
    pub fn earthwork(&self, n: f64, e: f64, natural: f64) -> Option<(f64, f64)> {
        let at = self.locate(n, e)?;
        let (_, side, _) = self.side_at(&at);
        let x = at.across.abs();
        let (h, _, _) = self.plane(&at);
        if x <= side.reach {
            // Under the circuit's own ground, a little lower, rising to it over the last metres.
            let under = UNDER * (1.0 - smoothstep(side.reach - 6.0, side.reach, x));
            return Some((h - under, 1.0));
        }
        let bank = (10.0 + 1.6 * (natural - h).abs()).min(MOST_REACH - side.reach);
        if x >= side.reach + bank {
            return None;
        }
        Some((h, 1.0 - smoothstep(side.reach, side.reach + bank, x)))
    }

    /// The point along at a distance, its height and direction: (north, east, height, heading).
    pub fn at(&self, along: f64) -> (f64, f64, f64, f64) {
        let s = along.rem_euclid(self.length) / STEP;
        let i = s.floor() as usize % self.samples.len();
        let t = s - s.floor();
        let (a, b) = (&self.samples[i], &self.samples[(i + 1) % self.samples.len()]);
        (lerp(a.n, b.n, t), lerp(a.e, b.e, t), lerp(a.height, b.height, t), lerp_angle(a.heading, b.heading, t))
    }

    /// A point of the circuit's surface: `across` metres right of the centre line at `along`.
    pub fn point(&self, along: f64, across: f64) -> (f64, f64, f64) {
        let (n, e, h, heading) = self.at(along);
        let (pn, pe) = (n - heading.sin() * across, e + heading.cos() * across);
        let s = along.rem_euclid(self.length) / STEP;
        let i = s.floor() as usize % self.samples.len();
        let t = s - s.floor();
        let cross = lerp(self.samples[i].cross, self.samples[(i + 1) % self.samples.len()].cross, t);
        (pn, pe, h + cross * across)
    }

    /// The sample nearest a distance along.
    pub fn sample(&self, along: f64) -> &Sample {
        let i = (along.rem_euclid(self.length) / STEP).round() as usize % self.samples.len();
        &self.samples[i]
    }

    /// The `k`th place of the starting grid (0 the pole): two by two behind the start line, 8 m
    /// apart, staggered left and right. (north, east, heading).
    pub fn grid(&self, k: usize) -> (f64, f64, f64) {
        let along = self.start - 10.0 - 8.0 * k as f64;
        let w = self.sample(along).width;
        let across = if k.is_multiple_of(2) { -w / 4.0 } else { w / 4.0 };
        let (n, e, _) = self.point(along, across);
        (n, e, self.at(along).3)
    }

    /// Metres from the start line to a point along, forwards, in [0, length).
    pub fn from_start(&self, along: f64) -> f64 {
        (along - self.start).rem_euclid(self.length)
    }
}

/// Distance from a point to a segment, and how far along it (0 to 1) its nearest point is.
fn segment_distance(n: f64, e: f64, an: f64, ae: f64, bn: f64, be: f64) -> (f64, f64) {
    let (dn, de) = (bn - an, be - ae);
    let len2 = dn * dn + de * de;
    let t = if len2 > 0.0 { (((n - an) * dn + (e - ae) * de) / len2).clamp(0.0, 1.0) } else { 0.0 };
    ((n - an - t * dn).hypot(e - ae - t * de), t)
}

/// An angle in (−π, π].
fn remainder_pi(a: f64) -> f64 {
    let r = a.rem_euclid(TAU);
    if r > PI {
        r - TAU
    } else {
        r
    }
}

/// A distance along a lap of `length` in [−length/2, length/2).
fn remainder_pi_len(d: f64, length: f64) -> f64 {
    (d + length / 2.0).rem_euclid(length) - length / 2.0
}

fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    a + remainder_pi(b - a) * t
}

/// A closed list of values smoothed by a moving average `half` samples either side, `passes` times
/// (three passes come close to a Gaussian).
fn smooth(values: &[f64], half: usize, passes: usize) -> Vec<f64> {
    let count = values.len();
    let mut v = values.to_vec();
    for _ in 0..passes {
        let mut out = vec![0.0; count];
        let window = (2 * half + 1) as f64;
        let mut sum: f64 = (0..=2 * half).map(|j| v[(j + count - half) % count]).sum();
        for (i, o) in out.iter_mut().enumerate() {
            *o = sum / window;
            sum += v[(i + half + 1) % count] - v[(i + count - half) % count];
        }
        v = out;
    }
    v
}

/// The centre line through the points as a closed centripetal Catmull-Rom spline, finely: points
/// (north, east), each segment cut in many, the index in it where each control point is.
fn spline(points: &[(f64, f64)]) -> Vec<(f64, f64, usize)> {
    let count = points.len();
    let mut out = Vec::new();
    for i in 0..count {
        let p0 = points[(i + count - 1) % count];
        let p1 = points[i];
        let p2 = points[(i + 1) % count];
        let p3 = points[(i + 2) % count];
        let knot = |a: (f64, f64), b: (f64, f64)| ((b.0 - a.0).hypot(b.1 - a.1)).sqrt().max(1e-6);
        let (t1, t2, t3) = (knot(p0, p1), knot(p0, p1) + knot(p1, p2), knot(p0, p1) + knot(p1, p2) + knot(p2, p3));
        let pieces = (((p2.0 - p1.0).hypot(p2.1 - p1.1)) / 0.5).ceil().max(4.0) as usize;
        for k in 0..pieces {
            let t = t1 + (t2 - t1) * k as f64 / pieces as f64;
            let mix = |a: (f64, f64), b: (f64, f64), ta: f64, tb: f64| {
                let w = (t - ta) / (tb - ta);
                (a.0 + (b.0 - a.0) * w, a.1 + (b.1 - a.1) * w)
            };
            let a1 = mix(p0, p1, 0.0, t1);
            let a2 = mix(p1, p2, t1, t2);
            let a3 = mix(p2, p3, t2, t3);
            let b1 = mix(a1, a2, 0.0, t2);
            let b2 = mix(a2, a3, t1, t3);
            let c = mix(b1, b2, t1, t2);
            out.push((c.0, c.1, i));
        }
    }
    out
}

/// The fine line resampled every `STEP` metres: (north, east, heading) per sample, and the sample
/// each control point falls on.
fn resample(line: &[(f64, f64, usize)], points: &[(f64, f64)]) -> (Vec<(f64, f64, f64)>, Vec<usize>) {
    let count = line.len();
    let mut lengths = Vec::with_capacity(count + 1);
    let mut total = 0.0;
    for i in 0..count {
        lengths.push(total);
        let (a, b) = (line[i], line[(i + 1) % count]);
        total += (b.0 - a.0).hypot(b.1 - a.1);
    }
    lengths.push(total);
    let samples = (total / STEP).round() as usize;
    let step = total / samples as f64;
    let mut out = Vec::with_capacity(samples);
    let mut j = 0;
    for k in 0..samples {
        let s = k as f64 * step;
        while j + 1 < count && lengths[j + 1] <= s {
            j += 1;
        }
        let (a, b) = (line[j], line[(j + 1) % count]);
        let seg = lengths[j + 1] - lengths[j];
        let t = if seg > 0.0 { (s - lengths[j]) / seg } else { 0.0 };
        let heading = (b.1 - a.1).atan2(b.0 - a.0);
        out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, heading));
    }
    // Where each control point is: the sample nearest the start of its segment of the fine line.
    let mut starts = Vec::with_capacity(points.len());
    for i in 0..points.len() {
        let first = line.iter().position(|l| l.2 == i).unwrap_or(0);
        starts.push(((lengths[first] / step).round() as usize).min(samples - 1));
    }
    // The samples are about `STEP` apart; the lap's length is theirs times `STEP`.
    let _ = step;
    (out, starts)
}
