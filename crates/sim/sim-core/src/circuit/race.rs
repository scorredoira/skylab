//! A race on a circuit: a standing start under the lights, laps counted car by car, the order and
//! the gaps, the lapped, the chequered flag and the results. Pure bookkeeping over a `World`: it
//! moves nothing (the game and the tests drive the cars; `Race::holding` says when they must stand).
//!
//! **The start**: five red lights come on one a second from `Race::new`'s `start`; after a pause
//! of 0.5 to 2.5 s (from the race's seed, so the same race starts the same way) they go out and
//! the race is on. Before that the cars stand on the grid.
//!
//! **Laps**: each car's progress is the metres it has driven round since the start line (negative
//! on the grid, behind it), followed sample by sample, so it counts only laps driven forwards. A
//! lap is done each time progress passes a multiple of the circuit's length. The leader finishes
//! after `laps`; every other car finishes the next time it crosses the line, whatever lap it is
//! on (the lapped ones a lap or more down).
//!
//! **Order and gaps**: by laps finished (the finished first, in the order they finished), then by
//! progress. The gap to the car ahead is the time between the two passing the same point: each
//! car's time at every 25 m of progress is kept.

use std::sync::Arc;

use crate::math::Rng;
use crate::vehicle::VehicleId;
use crate::world::World;

use super::Circuit;

/// Metres between the points of progress whose times are kept, for the gaps.
const MARK: f64 = 25.0;
/// The lights: this many, one a second.
pub const LIGHTS: usize = 5;

/// One car in a race.
#[derive(Clone, Debug)]
pub struct Entrant {
    pub id: VehicleId,
    pub name: String,
    /// Metres driven round since the start line (negative behind it on the grid).
    pub progress: f64,
    /// Where along the circuit it was last seen.
    along: Option<f64>,
    /// Laps done and their times, s.
    pub laps: Vec<f64>,
    /// When its lap under way began (race time).
    lap_start: f64,
    /// Its race time when it took the flag.
    pub finished: Option<f64>,
    /// Out of the race (wrecked).
    pub retired: bool,
    /// Race time at each `MARK` metres of progress.
    marks: Vec<f64>,
}

impl Entrant {
    pub fn best(&self) -> Option<f64> {
        self.laps.iter().copied().reduce(f64::min)
    }

    /// Laps done.
    pub fn lap_count(&self) -> u32 {
        self.laps.len() as u32
    }
}

/// Where a race is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// The lights coming on: how many are lit.
    Lights(usize),
    Racing,
    /// The leader has taken the flag; the others are finishing their lap.
    Flag,
    /// Everyone has finished or retired.
    Over,
}

/// A race: see the module.
#[derive(Clone, Debug)]
pub struct Race {
    pub circuit: Arc<Circuit>,
    /// Laps the race runs for.
    pub laps: u32,
    pub entrants: Vec<Entrant>,
    /// World time the first light comes on, and the lights go out.
    pub start: f64,
    pub go: f64,
    /// World time now (as of the last `update`).
    time: f64,
}

impl Race {
    /// A race of `laps` among the cars `ids` (named `names`), standing on the grid; the lights
    /// begin at world time `start`; `seed` decides the pause before they go out.
    pub fn new(circuit: Arc<Circuit>, laps: u32, entrants: &[(VehicleId, String)], start: f64, seed: u64) -> Race {
        let pause = 0.5 + 2.0 * Rng::new(seed ^ 0x4c49_4748_5453).uniform();
        let go = start + LIGHTS as f64 + pause;
        let entrants = entrants
            .iter()
            .map(|(id, name)| Entrant { id: *id, name: name.clone(), progress: 0.0, along: None, laps: Vec::new(), lap_start: 0.0, finished: None, retired: false, marks: Vec::new() })
            .collect();
        Race { circuit, laps: laps.max(1), entrants, start, go, time: start }
    }

    /// Seconds since the lights went out (negative before).
    pub fn clock(&self) -> f64 {
        self.time - self.go
    }

    pub fn phase(&self) -> Phase {
        if self.time < self.go {
            let lit = ((self.time - self.start).floor() as i64 + 1).clamp(0, LIGHTS as i64) as usize;
            return Phase::Lights(if self.time < self.start { 0 } else { lit });
        }
        if self.entrants.iter().all(|e| e.finished.is_some() || e.retired) {
            Phase::Over
        } else if self.entrants.iter().any(|e| e.finished.is_some()) {
            Phase::Flag
        } else {
            Phase::Racing
        }
    }

    /// Whether the cars must still stand on the grid.
    pub fn holding(&self) -> bool {
        self.time < self.go
    }

    /// After a step of the world: each car's progress, laps, flag.
    pub fn update(&mut self, world: &World) {
        self.time = world.time();
        let c = self.circuit.clone();
        let clock = self.clock();
        let flag = self.entrants.iter().any(|e| e.finished.is_some());
        let laps = self.laps;
        for e in &mut self.entrants {
            let vehicle = world.vehicle(e.id);
            if vehicle.dynamics.crashed().is_some() && e.finished.is_none() {
                e.retired = true;
            }
            let b = vehicle.body();
            let Some(at) = c.locate(b.pos.x, b.pos.y) else { continue };
            let from_start = c.from_start(at.along);
            let Some(before) = e.along else {
                // First seen: on the grid, behind the line.
                e.progress = if from_start > c.length / 2.0 { from_start - c.length } else { from_start };
                e.along = Some(at.along);
                continue;
            };
            e.along = Some(at.along);
            if e.finished.is_some() || e.retired {
                continue;
            }
            let moved = (at.along - before + c.length / 2.0).rem_euclid(c.length) - c.length / 2.0;
            let was = e.progress;
            e.progress += moved;
            if clock < 0.0 {
                continue;
            }
            // Its time at each mark passed, for the gaps.
            while e.progress >= 0.0 && (e.marks.len() as f64) * MARK <= e.progress {
                e.marks.push(clock);
            }
            // A lap done each time it crosses the line forwards.
            if (e.progress / c.length).floor() > (was / c.length).floor() && e.progress >= c.length {
                e.laps.push(clock - e.lap_start);
                e.lap_start = clock;
                if e.laps.len() as u32 >= laps || flag {
                    e.finished = Some(clock);
                }
            }
        }
    }

    /// The entrants' indices in race order.
    pub fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.entrants.len()).collect();
        order.sort_by(|&a, &b| {
            let (x, y) = (&self.entrants[a], &self.entrants[b]);
            let rank = |e: &Entrant| (e.retired as u8, if e.finished.is_some() { 0u8 } else { 1 });
            rank(x).cmp(&rank(y)).then_with(|| match (x.finished, y.finished) {
                (Some(p), Some(q)) => y.laps.len().cmp(&x.laps.len()).then(p.total_cmp(&q)),
                _ => y.progress.total_cmp(&x.progress),
            })
        });
        order
    }

    /// The position (1 the leader) of entrant `i`.
    pub fn position(&self, i: usize) -> usize {
        self.order().iter().position(|&k| k == i).unwrap_or(0) + 1
    }

    /// Seconds entrant `i` is behind entrant `ahead`: since `ahead` was where `i` is now (its
    /// times at each mark of progress, in between them as it went); `None` if not measurable yet.
    /// A car laps down is that many laps behind as well (see `laps_down`).
    pub fn gap(&self, ahead: usize, i: usize) -> Option<f64> {
        let (a, e) = (&self.entrants[ahead], &self.entrants[i]);
        if let (Some(p), Some(q)) = (a.finished, e.finished) {
            if a.laps.len() == e.laps.len() {
                return Some(q - p);
            }
        }
        let now = e.finished.unwrap_or(self.clock());
        let s = e.progress.max(0.0) / MARK;
        let k = s.floor() as usize;
        let t0 = *a.marks.get(k)?;
        let then = match a.marks.get(k + 1) {
            Some(t1) => t0 + (t1 - t0) * (s - s.floor()),
            None => t0,
        };
        Some((now - then).max(0.0))
    }

    /// Whole laps entrant `i` is behind entrant `ahead`.
    pub fn laps_down(&self, ahead: usize, i: usize) -> u32 {
        ((self.entrants[ahead].progress - self.entrants[i].progress) / self.circuit.length).floor().max(0.0) as u32
    }

    /// The lap entrant `i` is on (1 the first; the last while it finishes).
    pub fn lap(&self, i: usize) -> u32 {
        (self.entrants[i].laps.len() as u32 + 1).min(self.laps)
    }
}
