//! A racing driver: a `Pilot` that laps a circuit in a car, from the observation alone. It is the
//! teacher the AI drivers learn from (`sim-env`'s `drive` task), and the opponent of the game.
//!
//! Before it drives, `RacingLine::new` works out its line and its speeds round the circuit:
//!
//! - **The line**: the path of least curvature within the asphalt (kept `margin` metres from its
//!   edges, the kerbs allowed), found as an elastic band: each point of the line moved, across the
//!   circuit only, towards the middle of its neighbours, over and over, from far neighbours to near
//!   ones (the classic minimum-curvature line of racing, as in Braghin et al., *Race driver model*,
//!   Computers & Structures 86, 2008, and the "elastic band" smoothing used in robotics).
//! - **The speeds**: as fast as the grip allows in each corner, `sqrt(a / curvature)` with `a` the
//!   lateral acceleration the car can hold; then the braking for each corner worked backwards and the
//!   acceleration out of it forwards, each within what the tyres have left beside the cornering (the
//!   "g-g diagram" of a friction ellipse), and the acceleration also within the engine's power.
//!
//! Driving: the steering points the car at a point of the line some way ahead (pure pursuit, the
//! distance growing with the speed), with the yaw rate damped; the pedals follow the speed of the
//! line a little ahead (throttle or brake by the error), easing off the throttle when the rear
//! slides. The gearbox is left to the automatic.

use std::sync::Arc;

use crate::circuit::Circuit;
use crate::math::{remainder, Rng, GRAVITY};
use crate::pilot::Pilot;
use crate::vehicle::Controls;
use crate::world::Observation;

use super::file::CarType;
use super::CarControls;

/// How it drives: its limits as shares of what the car can do, and its line's margins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    /// Lateral acceleration it corners at, a share of the tyres' grip.
    pub cornering: f64,
    /// Braking and accelerating, shares of the grip.
    pub braking: f64,
    pub accelerating: f64,
    /// Metres it keeps from the asphalt's edges.
    pub margin: f64,
}

impl Default for Pace {
    fn default() -> Pace {
        Pace { cornering: 0.82, braking: 0.85, accelerating: 0.8, margin: 1.2 }
    }
}

/// The line round a circuit and the speed along it.
#[derive(Clone, Debug)]
pub struct RacingLine {
    /// Per sample of the circuit: metres across of the line, its curvature (1/m), and the speed
    /// (m/s) to drive it at.
    pub across: Vec<f64>,
    pub curvature: Vec<f64>,
    pub speed: Vec<f64>,
    /// The speeds the corners and the braking for them allow, before the engine's power is
    /// counted: where it must brake, whatever it can reach.
    pub braking: Vec<f64>,
}

impl RacingLine {
    pub fn new(circuit: &Circuit, car: &CarType, pace: Pace) -> RacingLine {
        let count = circuit.samples.len();
        let limit: Vec<f64> = circuit.samples.iter().map(|s| (s.width / 2.0 - pace.margin).max(0.0)).collect();
        let normal: Vec<(f64, f64)> = circuit.samples.iter().map(|s| (-s.heading.sin(), s.heading.cos())).collect();
        let point = |i: usize, o: f64| (circuit.samples[i].n + normal[i].0 * o, circuit.samples[i].e + normal[i].1 * o);
        let mut across = vec![0.0; count];
        for span in [40usize, 25, 15, 9, 5, 3] {
            for _ in 0..200 {
                for i in 0..count {
                    let (a, b) = (point((i + count - span) % count, across[(i + count - span) % count]), point((i + span) % count, across[(i + span) % count]));
                    let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                    let s = &circuit.samples[i];
                    let o = (mid.0 - s.n) * normal[i].0 + (mid.1 - s.e) * normal[i].1;
                    across[i] = o.clamp(-limit[i], limit[i]);
                }
            }
        }
        // The line's curvature over 12 m.
        let k = 6;
        let curvature: Vec<f64> = (0..count)
            .map(|i| {
                let a = point((i + count - k) % count, across[(i + count - k) % count]);
                let b = point(i, across[i]);
                let c = point((i + k) % count, across[(i + k) % count]);
                menger(a, b, c)
            })
            .collect();

        let mut line = RacingLine { across, curvature, speed: Vec::new(), braking: Vec::new() };
        line.respeed(car, pace, &|_| 1.0);
        line
    }

    /// The line's speeds worked out again for a road that gives `grip(v)` of the dry grip at a
    /// speed `v` (m/s): the line kept. On a wet road the corners are taken slower, the braking
    /// begins earlier, and where water would lift the tyres off the road at speed (`wet`) the
    /// straights are not driven flat out.
    pub fn respeed(&mut self, car: &CarType, pace: Pace, grip: &dyn Fn(f64) -> f64) {
        let curvature = &self.curvature;
        let count = curvature.len();
        // How hard the car can corner, brake and accelerate.
        let tyre = &car.axles[0].tyre;
        let lateral = tyre.grip * GRAVITY * pace.cornering;
        let braking = tyre.grip_longitudinal * GRAVITY * pace.braking;
        let traction = tyre.grip_longitudinal * GRAVITY * pace.accelerating * 0.55;
        let power = car.power();
        let mut top = car.top_of_gear(car.gearbox.ratios.len() as i32);
        // No faster than where the road keeps four fifths of what it gives slowly.
        while top > 20.0 && grip(top) < 0.8 * grip(10.0) {
            top -= 1.0;
        }
        let drag = |v: f64| 0.5 * 1.225 * car.aero.drag_area * v * v / car.mass + 0.015 * GRAVITY;

        // In each corner as fast as the grip there lets it (the grip itself depending on the speed).
        let mut speed: Vec<f64> = curvature
            .iter()
            .map(|c| {
                let k = c.abs().max(1e-5);
                let mut v = (lateral / k).sqrt().min(top);
                for _ in 0..6 {
                    v = (lateral * grip(v) / k).sqrt().min(top);
                }
                v
            })
            .collect();
        let ds = crate::circuit::STEP;
        let room = |v: f64, k: f64, a: f64| {
            let g = grip(v);
            let used = (v * v * k.abs() / (lateral * g)).min(1.0);
            a * g * (1.0 - used * used).max(0.0).sqrt()
        };
        // Twice round each way, so the laps join.
        for _ in 0..2 {
            for j in (0..count).rev() {
                let next = speed[(j + 1) % count];
                let a = room(next, curvature[j], braking) + drag(next);
                speed[j] = speed[j].min((next * next + 2.0 * a * ds).sqrt());
            }
        }
        self.braking = speed.clone();
        for _ in 0..2 {
            for j in 0..count {
                let v = speed[j];
                let push = (power / (car.mass * v.max(5.0))).min(traction * grip(v));
                let a = (room(v, curvature[j], push / grip(v).max(1e-3)) - drag(v)).max(0.0);
                let n = (j + 1) % count;
                speed[n] = speed[n].min((v * v + 2.0 * a * ds).sqrt());
            }
        }
        self.speed = speed;
    }

    /// The line's point at a distance along: (north, east), and the speed there.
    pub fn at(&self, circuit: &Circuit, along: f64) -> ((f64, f64), f64) {
        let count = self.across.len();
        let s = along.rem_euclid(circuit.length) / crate::circuit::STEP;
        let i = s.floor() as usize % count;
        let t = s - s.floor();
        let j = (i + 1) % count;
        let across = self.across[i] + (self.across[j] - self.across[i]) * t;
        let (n, e, _) = circuit.point(along, across);
        ((n, e), self.speed[i] + (self.speed[j] - self.speed[i]) * t)
    }

    /// The lowest speed of the line within `ahead` metres of a point along.
    pub fn speed_ahead(&self, circuit: &Circuit, along: f64, ahead: f64) -> f64 {
        let count = self.speed.len();
        let i = (along.rem_euclid(circuit.length) / crate::circuit::STEP) as usize;
        let n = (ahead / crate::circuit::STEP).ceil() as usize;
        (0..=n).map(|k| self.speed[(i + k) % count]).fold(f64::INFINITY, f64::min)
    }
}

/// The curvature of the circle through three points (Menger's), signed: positive turning right.
fn menger(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let (ab, bc, ca) = ((b.0 - a.0).hypot(b.1 - a.1), (c.0 - b.0).hypot(c.1 - b.1), (a.0 - c.0).hypot(a.1 - c.1));
    2.0 * cross / (ab * bc * ca).max(1e-9)
}

/// How a driver races, besides the line: its pace and its habits. `Character::default()` is the
/// teacher: on the line, at its speeds, braking where it says, no mistakes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Character {
    /// A share of the line's speeds: 1 races flat out.
    pub pace: f64,
    /// How late it brakes: the distance it looks ahead for the line's slowest speed shortened by
    /// this share (0.1: a tenth later, and harder).
    pub late_braking: f64,
    /// Metres to the right of the racing line it drives (its own line).
    pub line: f64,
    /// Mistakes it makes in a hundred seconds, on average: a corner braked too late, one run wide,
    /// a lift where it should not.
    pub mistakes: f64,
    /// The seed of its mistakes: the same seed, the same mistakes at the same moments.
    pub seed: u64,
}

impl Default for Character {
    fn default() -> Character {
        Character { pace: 1.0, late_braking: 0.0, line: 0.0, mistakes: 0.0, seed: 1 }
    }
}

impl Character {
    /// A rival of `skill` (0 the slowest of a grid to 1 the quickest), its habits drawn from
    /// `seed`: some 3 % of pace between the extremes, braking a little early or late, its line a
    /// few tens of centimetres off, a mistake every few laps (fewer the more skilled).
    pub fn rival(skill: f64, seed: u64) -> Character {
        let mut rng = Rng::new(seed ^ 0x5249_5641_4c00);
        let skill = skill.clamp(0.0, 1.0);
        let spread = |rng: &mut Rng| rng.uniform() * 2.0 - 1.0;
        Character {
            pace: 0.955 + 0.035 * skill + 0.004 * spread(&mut rng),
            late_braking: 0.06 * spread(&mut rng) + 0.04 * skill,
            line: 0.35 * spread(&mut rng),
            mistakes: 0.35 + 0.5 * (1.0 - skill) * rng.uniform(),
            seed,
        }
    }
}

/// A mistake under way.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mistake {
    /// Braking for the next corner later than it can: it runs wide.
    LateBraking,
    /// Running wide, metres to the outside.
    Wide(f64),
    /// Lifting off for no reason: this share of its speeds.
    Lift(f64),
}

/// Another car as a driver sees it on the circuit.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Near {
    /// Metres ahead along the circuit (negative behind).
    ahead: f64,
    /// Metres to the right of the centre line.
    across: f64,
    /// Its speed along the circuit, m/s, and how hard it slows (m/s², from its last speeds seen).
    speed: f64,
    slowing: f64,
}

/// A car's length, m, and how far apart two cars' middles must be across to pass side by side.
const CAR_LENGTH: f64 = 4.7;
const SIDE_BY_SIDE: f64 = 2.7;
/// How fast it moves across the circuit, off its line or back, m/s.
const SHIFT_RATE: f64 = 2.2;

/// The racing driver: see the module.
#[derive(Clone, Debug)]
pub struct Driver {
    pub circuit: Arc<Circuit>,
    pub line: RacingLine,
    car: CarType,
    limits: Pace,
    wheelbase: f64,
    lock: f64,
    /// Where along the circuit it last was, to find itself again near there.
    along: Option<f64>,
    /// A share of the line's speeds: 1 races, less drives more gently.
    pub pace: f64,
    pub character: Character,
    /// The film of water its line's speeds are for, mm, and the film it sees under its wheels.
    rain: f64,
    wet_seen: f64,
    /// Metres it is now off its line to the right (to pass, to give room, a mistake), moving
    /// towards where it wants to be at `SHIFT_RATE`.
    shift: f64,
    /// Passing a car: the across it passes on.
    passing: Option<f64>,
    mistake: Option<(Mistake, f64)>,
    /// When it last thought of making a mistake, and the dice it throws.
    rolled: f64,
    rng: Rng,
    /// The time of the last observation, for how fast things move.
    time: Option<f64>,
    /// The other cars' speeds along the circuit last seen, and how hard they slowed: by id.
    seen: Vec<(usize, f64, f64)>,
    /// Whether, at its last decision, a car in its way held it below its line's speed.
    held: bool,
}

impl Driver {
    pub fn new(circuit: Arc<Circuit>, car: &CarType) -> Driver {
        Driver::with_character(circuit, car, Character::default())
    }

    /// A driver with its own pace and habits (`Character::rival`).
    pub fn with_character(circuit: Arc<Circuit>, car: &CarType, character: Character) -> Driver {
        let limits = Pace::default();
        let line = RacingLine::new(&circuit, car, limits);
        Driver {
            circuit,
            line,
            car: car.clone(),
            limits,
            wheelbase: car.wheelbase(),
            lock: car.steering.lock_deg.to_radians(),
            along: None,
            pace: character.pace,
            character,
            rain: 0.0,
            wet_seen: 0.0,
            shift: character.line,
            passing: None,
            mistake: None,
            rolled: 0.0,
            rng: Rng::new(character.seed),
            time: None,
            seen: Vec::new(),
            held: false,
        }
    }

    /// Forgets where it was (after the car is placed somewhere new).
    pub fn reset(&mut self) {
        self.along = None;
        self.passing = None;
        self.mistake = None;
        self.shift = self.character.line;
        self.time = None;
        self.seen.clear();
    }

    /// Where along the circuit it is now, if on it.
    pub fn along(&self) -> Option<f64> {
        self.along
    }

    /// Whether a car in its way held it below its line's speed at its last decision.
    pub fn held_up(&self) -> bool {
        self.held
    }

    /// Whether it is passing a car now.
    pub fn passing(&self) -> bool {
        self.passing.is_some()
    }

    /// The line's speeds for a road under `film` millimetres of water (0 dry): the grip a tyre
    /// keeps there at each speed (`wet::wet_asphalt`).
    pub fn set_film(&mut self, film: f64) {
        self.rain = film;
        if film <= 0.0 {
            self.line.respeed(&self.car, self.limits, &|_| 1.0);
        } else {
            self.line.respeed(&self.car, self.limits, &|v| super::wet::wet_asphalt(film, v * 3.6));
        }
    }

    /// The other cars near it on the circuit, as it sees them; how hard each slows is followed
    /// from one look to the next, `dt` seconds apart.
    fn traffic(&mut self, obs: &Observation, along: f64, dt: f64) -> Vec<Near> {
        let c = self.circuit.clone();
        let heading = c.at(along).3;
        let (ch, sh) = (heading.cos(), heading.sin());
        let mut out = Vec::new();
        for t in obs.traffic.iter().filter(|t| t.relative[0].hypot(t.relative[1]) < 150.0 && t.relative[2].abs() < 3.0) {
            let Some(o) = c.locate(obs.north + t.relative[0], obs.east + t.relative[1]) else { continue };
            let ahead = (o.along - along + c.length / 2.0).rem_euclid(c.length) - c.length / 2.0;
            let speed = t.velocity[0] * ch + t.velocity[1] * sh;
            let slowing = match self.seen.iter_mut().find(|s| s.0 == t.id) {
                Some(s) => {
                    if dt > 0.0 {
                        let now = (s.1 - speed) / dt;
                        s.2 += (now - s.2) * (dt / 0.15).min(1.0);
                    }
                    s.1 = speed;
                    s.2
                }
                None => {
                    self.seen.push((t.id, speed, 0.0));
                    0.0
                }
            };
            out.push(Near { ahead, across: o.across, speed, slowing });
        }
        out
    }

    /// The line's across at a distance along, interpolated.
    fn line_across(&self, along: f64) -> f64 {
        let count = self.line.across.len();
        let s = along.rem_euclid(self.circuit.length) / crate::circuit::STEP;
        let i = s.floor() as usize % count;
        let t = s - s.floor();
        self.line.across[i] + (self.line.across[(i + 1) % count] - self.line.across[i]) * t
    }

    /// The controls for a car at an observation.
    pub fn drive(&mut self, obs: &Observation, current: &CarControls) -> CarControls {
        let Some(car) = obs.car() else { return *current };
        let Some(at) = self.circuit.locate(obs.north, obs.east) else {
            return CarControls { throttle: 0.0, brake: 1.0, ..*current };
        };
        // How wet the road is, as it sees it under its wheels (the least of the four: a puddle is
        // a puddle, not the road), over a few seconds; the line's speeds for it when it changes.
        let dt = self.time.map_or(0.0, |t| (obs.time - t).clamp(0.0, 0.5));
        let under = car.water.iter().copied().fold(f64::INFINITY, f64::min).max(0.0) * 1000.0;
        self.wet_seen = if self.time.is_none() { under } else { self.wet_seen + (under - self.wet_seen) * (dt / 3.0).min(1.0) };
        let film = if self.wet_seen < 0.02 { 0.0 } else { (self.wet_seen * 20.0).round() / 20.0 };
        if (film - self.rain).abs() > 1e-9 {
            self.set_film(film);
        }
        self.time = Some(obs.time);
        self.along = Some(at.along);
        let v = car.speed.max(0.0);
        let c = self.circuit.clone();
        let half = c.sample(at.along).width / 2.0;

        // The others: who is ahead in its way, who is beside it.
        let traffic = if obs.traffic.is_empty() { Vec::new() } else { self.traffic(obs, at.along, dt) };
        let brake_g = self.car.axles[0].tyre.grip_longitudinal * GRAVITY * 0.7 * if self.rain > 0.0 { super::wet::wet_asphalt(self.rain, v * 3.6) } else { 1.0 };
        let mut cap = f64::INFINITY;
        let (mut lowest, mut highest) = (-half + 1.0, half - 1.0);
        let mut blocker: Option<Near> = None;
        let mut beside = false;
        let mut crowded = false;
        for n in &traffic {
            crowded |= n.ahead.abs() < 20.0;
            let closing = v - n.speed;
            // Overlapping: alongside (room left between them, whoever is a little ahead), or, in
            // the same lane or closing fast on it, about to run into it: off the throttle at once.
            if n.ahead.abs() < CAR_LENGTH + 2.5 {
                if (n.across - at.across).abs() > 2.0 && !(n.ahead > 0.0 && closing > 2.0) {
                    beside = true;
                    if n.across > at.across {
                        highest = highest.min(n.across - SIDE_BY_SIDE);
                    } else {
                        lowest = lowest.max(n.across + SIDE_BY_SIDE);
                    }
                } else if n.ahead > 0.0 {
                    // Close behind a stopped car it is getting round, it creeps out beside it.
                    let creep = if self.passing.is_some() && n.speed < 4.0 && n.ahead > CAR_LENGTH { 3.0 } else { 0.0 };
                    cap = cap.min((n.speed - 2.0).max(creep));
                    blocker = Some(*n);
                }
                continue;
            }
            if n.ahead <= 0.0 || n.ahead > 140.0 {
                continue;
            }
            // Going round a stopped car: it is not in the way of the lane taken beside it.
            if self.passing.is_some() && n.speed < 4.0 && (n.across - at.across).abs() > 2.2 {
                continue;
            }
            // Ahead: in its way if it is where it would be across when it gets there, or in its
            // lane now and near; the faster it closes, the wider the berth it wants.
            let path = self.line_across(at.along + n.ahead) + self.shift;
            let berth = SIDE_BY_SIDE - 0.2 + 0.08 * closing.max(0.0);
            if (n.across - path).abs() > berth && ((n.across - at.across).abs() > berth || n.ahead > 30.0 || self.passing.is_some()) {
                continue;
            }
            // As fast as it can still match the other's speed (as it slows) a safe gap behind it.
            let gap = n.ahead - CAR_LENGTH;
            let keep = 2.5 + 0.09 * n.speed.max(0.0);
            let relative = (brake_g - n.slowing.max(0.0)).max(1.0);
            let allowed = if gap > keep { n.speed.max(0.0) + (2.0 * relative * (gap - keep)).sqrt() } else { (n.speed - 1.0).max(0.0) };
            if allowed < cap {
                cap = allowed;
                blocker = Some(*n);
            }
        }

        // Passing: held up by a car ahead on a straight, it keeps in its slipstream until it
        // closes on it, or until the braking for the next corner is near; then out to the inside of
        // that corner (the other side if there is no room), to brake later than the other beside it.
        // What it could do here alone: as fast as the corners ahead allow.
        let line_here = {
            let n = ((4.0 + v * 0.25) / crate::circuit::STEP).ceil() as usize;
            let count = self.line.braking.len();
            (0..=n).map(|k| self.line.braking[(at.index + k) % count]).fold(f64::INFINITY, f64::min).min(self.line.speed_ahead(&c, at.along, 4.0 + v * 0.25) + 15.0) * self.pace
        };
        // A car stopped (or crawling) in its way: round it, wherever.
        let stalled = blocker.is_some_and(|b| b.speed < 4.0 && b.ahead < 40.0);
        if let Some(b) = blocker.filter(|b| b.ahead < 45.0 && (cap < line_here - 1.5 || stalled)) {
            if self.passing.is_none() {
                let count = self.line.curvature.len();
                let curved = (0..40).map(|k| self.line.curvature[(at.index + k) % count].abs()).fold(0.0, f64::max);
                // The next braking point and the corner after it.
                let braking = (1..150).find(|&k| self.line.speed[(at.index + k) % count] < v * 0.9);
                let corner = braking.and_then(|k| (k..k + 60).map(|j| self.line.curvature[(at.index + j) % count]).find(|x| x.abs() > 1.0 / 300.0));
                let near = braking.is_some_and(|k| k as f64 * crate::circuit::STEP < 140.0 + 2.0 * v);
                let closing = v - b.speed;
                // Where the other will be when it brakes and turns in: on its line there (the
                // line's across at the braking point); the room must be there then, as now.
                let room_left = b.across - (-half + 1.0);
                let room_right = (half - 1.0) - b.across;
                let inside = corner.map_or(0.0, f64::signum);
                let (side, room) = if inside < 0.0 && room_left > SIDE_BY_SIDE + 0.3 {
                    (-1.0, room_left)
                } else if inside > 0.0 && room_right > SIDE_BY_SIDE + 0.3 {
                    (1.0, room_right)
                } else if room_left > room_right {
                    (-1.0, room_left)
                } else {
                    (1.0, room_right)
                };
                // Out of the slipstream with a run on it, and enough straight left to get by before
                // the braking (in the wet, a clearer run still).
                let room_ahead = braking.map_or(f64::INFINITY, |k| k as f64 * crate::circuit::STEP);
                let chance = if self.rain > 0.0 { closing > 3.0 && !near } else { closing > 1.0 && room_ahead > 120.0 + 1.5 * v };
                if (stalled || curved < 1.0 / 250.0 && chance) && room > SIDE_BY_SIDE + 0.3 {
                    self.passing = Some((b.across + side * (SIDE_BY_SIDE + 0.3)).clamp(-half + 1.0, half - 1.0));
                }
            }
        }
        if let Some(p) = self.passing {
            // Done when nobody is in the way or alongside any more, or given up in a corner.
            let curved = self.line.curvature[at.index].abs() > 1.0 / 120.0;
            // In the wet, given up before the braking for the next corner if not clear ahead.
            let count = self.line.speed.len();
            let braking_near = (1..(70.0 + v) as usize).any(|k| self.line.speed[(at.index + k) % count] < v * 0.9);
            // Not by the braking: it gives up and drops in behind (the other has the corner).
            let behind = traffic.iter().any(|n| n.ahead > -1.0 && n.ahead < 45.0 && (n.across - p).abs() < SIDE_BY_SIDE + 1.0);
            if (blocker.is_none() && !beside) || (!stalled && (curved || (braking_near && behind))) {
                self.passing = None;
            } else {
                // Keep to its side of the car passed, wherever that one goes.
                let passed = traffic.iter().filter(|n| n.ahead > -CAR_LENGTH && n.ahead < 45.0).min_by(|a, b| a.ahead.abs().total_cmp(&b.ahead.abs()));
                if let Some(n) = passed {
                    let side = (p - n.across).signum();
                    let lane = n.across + side * (SIDE_BY_SIDE + 0.3);
                    // No room left on that side (the other has moved over): it gives up and drops
                    // in behind.
                    self.passing = (lane.abs() <= half - 1.0).then_some(lane);
                }
            }
        }

        // Mistakes: now and then, never with a car close by.
        if self.character.mistakes > 0.0 && obs.time - self.rolled >= 1.0 {
            self.rolled = obs.time;
            let roll = self.rng.uniform();
            let kind = self.rng.uniform();
            if self.mistake.is_none() && !crowded && v > 15.0 && roll < self.character.mistakes / 100.0 {
                let outside = -self.line.curvature[(at.index + 20) % self.line.curvature.len()].signum();
                let m = if kind < 0.4 {
                    Mistake::LateBraking
                } else if kind < 0.75 {
                    Mistake::Wide(outside * (1.0 + kind))
                } else {
                    Mistake::Lift(0.88)
                };
                self.mistake = Some((m, obs.time + if matches!(m, Mistake::LateBraking) { 3.0 } else { 2.5 }));
            }
        }
        if self.mistake.is_some_and(|(_, until)| obs.time > until) {
            self.mistake = None;
        }

        // Where across it wants to be: its line, or beside the car it passes, or where a mistake
        // takes it; then within what the cars beside it leave.
        let mut want = match self.passing {
            Some(p) => p - self.line_across(at.along),
            None => self.character.line,
        };
        if let Some((Mistake::Wide(w), _)) = self.mistake {
            want += w;
        }
        let step = SHIFT_RATE * dt;
        self.shift += (want - self.shift).clamp(-step, step);
        let ahead = (6.0 + 0.55 * v).min(45.0);
        let on_line = self.line_across(at.along + ahead);
        let mut target_across = on_line + self.shift;
        if beside {
            target_across = target_across.clamp(lowest.min(highest), highest.max(lowest));
        }
        // Never aiming off the asphalt there.
        let there = c.sample(at.along + ahead).width / 2.0 - 0.9;
        target_across = target_across.clamp(-there, there);
        // Giving room: a car alongside on the inside of the corner coming, its nose level or ahead,
        // has the corner (as racing's rules have it): it lifts and lets it through.
        if beside && v > 10.0 {
            let count = self.line.curvature.len();
            let coming = (5..60).map(|k| self.line.curvature[(at.index + k) % count]).find(|x| x.abs() > 1.0 / 200.0);
            if let Some(k) = coming {
                let inside = k.signum();
                if let Some(n) = traffic.iter().find(|n| n.ahead.abs() < CAR_LENGTH + 0.8 && n.ahead > -1.0 && n.speed > 8.0 && (n.across - at.across) * inside > 1.5) {
                    cap = cap.min((n.speed - 1.0).max(0.0));
                }
            }
        }
        // Squeezed off the asphalt by a car beside it: it lifts and lets it go.
        if beside && (lowest > highest || lowest > half - 1.2 || highest < -half + 1.2) {
            if let Some(n) = traffic.iter().filter(|n| n.ahead.abs() < CAR_LENGTH + 0.8 && n.ahead > -1.5 && n.speed > 8.0).min_by(|a, b| a.ahead.total_cmp(&b.ahead)) {
                cap = cap.min((n.speed - 2.0).max(0.0));
            }
        }

        // Steering: pure pursuit of a point of the line ahead (off it by the shift).
        let (tn, te) = if self.shift == 0.0 && !beside {
            self.line.at(&c, at.along + ahead).0
        } else {
            let (n, e, _) = c.point(at.along + ahead, target_across);
            (n, e)
        };
        let (dn, de) = (tn - obs.north, te - obs.east);
        let (ch, sh) = (obs.heading.cos(), obs.heading.sin());
        let forward = dn * ch + de * sh;
        let right = -dn * sh + de * ch;
        let distance = forward.hypot(right).max(1.0);
        let curvature = 2.0 * right / (distance * distance);
        let mut steer = (curvature * self.wheelbase).atan();
        // A little more lock with speed for the tyres' slip, and the yaw rate damped.
        let line_curvature = self.line.curvature[at.index];
        steer += 0.0025 * v * v * line_curvature * 0.25;
        let yaw = obs.angular_rates[2];
        let wanted = v * curvature;
        steer -= 0.04 * (yaw - wanted);
        // Against a slide: the front wheels turned towards where the car is going (counter-steer).
        if v > 5.0 {
            let course = obs.velocity[1].atan2(obs.velocity[0]);
            let slip = remainder(course - obs.heading, std::f64::consts::TAU);
            steer += 0.9 * slip;
        }
        let steer = (steer / self.lock).clamp(-1.0, 1.0);

        // Pedals: the line's speed a little ahead (nearer when braking late), within what the
        // traffic ahead leaves; off its line in a corner a little slower.
        // Passing, it commits: it brakes a little later than its line says, on the inside.
        let committed = if self.passing.is_some() && self.rain <= 0.0 { 0.12 } else { 0.0 };
        let late = self.character.late_braking + committed + if matches!(self.mistake, Some((Mistake::LateBraking, _))) { 0.45 } else { 0.0 };
        let mut pace = self.pace;
        if let Some((Mistake::Lift(k), _)) = self.mistake {
            pace *= k;
        }
        let look = if late == 0.0 { 4.0 + v * 0.25 } else { 4.0 + v * 0.25 * (1.0 - late).max(0.2) };
        let mut target = self.line.speed_ahead(&c, at.along, look) * pace;
        let off_line = (target_across - on_line).abs();
        if off_line > 0.5 {
            target *= (1.0 - 0.01 * off_line * (line_curvature.abs() * 150.0).min(1.0)).max(0.95);
        }
        self.held = cap < target;
        // Standing water seen ahead: slow enough to cross it before the tyres would aquaplane
        // (where they keep half the grip of the film around it), braking for it in time.
        if self.rain > 0.0 {
            for (d, &w) in super::AHEAD_WATER.iter().zip(&car.water_ahead) {
                let mm = w * 1000.0;
                if mm > self.rain + 0.2 {
                    let around = super::wet::wet_asphalt(self.rain, 50.0);
                    let mut safe = 8.0;
                    while safe < 90.0 && super::wet::wet_asphalt(mm, (safe + 1.0) * 3.6) > 0.4 * around {
                        safe += 1.0;
                    }
                    let safe = safe * self.pace;
                    let a = brake_g * 0.8;
                    let reach = (d - 6.0).max(0.0);
                    cap = cap.min((safe * safe + 2.0 * a * reach).sqrt());
                }
            }
        }
        // In a car's slipstream it goes faster than it would alone: flat out until it must brake.
        if blocker.is_some_and(|b| b.ahead < 45.0) && self.rain <= 0.0 {
            let n = ((4.0 + v * 0.25) / crate::circuit::STEP).ceil() as usize;
            let count = self.line.braking.len();
            let i = (at.along.rem_euclid(c.length) / crate::circuit::STEP) as usize;
            target = target.max((0..=n).map(|k| self.line.braking[(i + k) % count]).fold(f64::INFINITY, f64::min) * pace);
        }
        let target = target.min(cap);
        let error = target - v;
        let (mut throttle, mut brake) = if error > 0.0 {
            (((error + 1.0) * 0.4).min(1.0), 0.0)
        } else if error > -1.5 {
            (0.15, 0.0)
        } else {
            (0.0, ((-error - 1.0) * 0.25).min(1.0))
        };
        // Feeding the throttle in as the rear tyres grip (as a driver feels for the wheelspin),
        // easing off when the rear slides out, or way off the line.
        let spin = car.slip[2].max(car.slip[3]);
        if spin > 0.12 {
            throttle *= (1.0 - (spin - 0.12) * 5.0).clamp(0.2, 1.0);
        }
        let rear_slip = car.slip_angle[2].abs().max(car.slip_angle[3].abs());
        if rear_slip > 0.12 {
            throttle *= (1.0 - (rear_slip - 0.12) * 8.0).max(0.0);
        }
        // Braking at the threshold, as a driver without anti-lock brakes does: the pedal eased as
        // a wheel begins to lock (sooner on a wet road, where the tyre's peak comes at less slip).
        if brake > 0.0 && !current.abs {
            let lock = car.slip.iter().fold(0.0f64, |m, s| m.max(-s));
            let peak = if self.rain > 0.0 { 0.07 } else { 0.10 };
            if lock > peak {
                brake *= (1.0 - (lock - peak) * 6.0).clamp(0.25, 1.0);
            }
        }
        let off = (at.across - self.line.across[at.index] - self.shift).abs();
        if off > 4.0 {
            throttle = throttle.min(0.4);
        }
        if v < 1.0 && target > 1.0 {
            brake = 0.0;
        }
        CarControls { steer, throttle, brake, clutch: 0.0, handbrake: 0.0, shift: 0, automatic: true, ..*current }
    }
}

impl Pilot for Driver {
    fn fly(&mut self, obs: &Observation, current: &Controls, _dt: f64) -> Controls {
        match current {
            Controls::Car(c) => Controls::Car(self.drive(obs, c)),
            other => *other,
        }
    }
}

/// The angle from `a` to `b`, in (−π, π].
pub fn turn(a: f64, b: f64) -> f64 {
    remainder(b - a, std::f64::consts::TAU)
}
