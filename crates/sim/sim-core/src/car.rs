//! Cars: a rigid body on four wheels, each on its own suspension, with Magic Formula tyres
//! (`tyre`), an engine, clutch, gearbox and limited-slip differential, brakes, the air, and the
//! walls and ground it runs into. A type of car is data (`file`, `assets/cars/<key>/car.ron`); the
//! ground and walls it runs on are asked through `ground::CarGround`. A racing pilot that laps a
//! circuit is `driver::Driver`.
//!
//! Each step of `World::DT` is cut in `SUBSTEPS` (960 Hz):
//!
//! - **Suspension**, per wheel, along the body's z axis: the spring (preloaded to carry the wheel's
//!   share of the weight at rest), the damper (bump and rebound), the anti-roll bar (the difference
//!   with the other side of the axle), the bump stop and the travel's ends, and the tyre's radial
//!   stiffness in series, against the ground found under the hub (`CarGround::patch`). The wheel's
//!   own mass is left out: the hub's travel follows from the balance of those forces, worked out
//!   implicitly (stable at any stiffness), and the load on the tyre is what the tyre's spring is
//!   squashed by. The links add to that balance the share of the tyre's side force they carry
//!   by the roll centre's height. Camber changes with the travel and the body's roll; toe with
//!   the travel.
//! - **Tyres** (`tyre::TyreFit::forces`): slip along and across the wheel as the transient tyre has
//!   them, each built up over its relaxation length as the wheel rolls (Pacejka 2012, ch. 7), with
//!   the damping it needs to come to rest at walking pace; the grip of the surface under it. The
//!   forces and the load reach the body at the contact, where the road makes them.
//! - **Drivetrain**: the engine's torque (its curve at full throttle and its drag closed, an idle
//!   governor, the rev limiter) through a friction clutch, the gearbox and a limited-slip
//!   differential to the rear wheels, and the brakes on all four: friction couplings solved together
//!   by sequential impulses each sub-step (as rigid-body engines solve joints with limits), so the
//!   engine, both wheels and the brakes stay consistent at any stiffness. The clutch is the
//!   player's pedal, or works by itself (taking up the drive from rest, letting go before the engine
//!   would stall, opening while a gear changes). Shifts take `shift_time`, the fuel cut going up,
//!   the throttle opened going down to match the revs of the gear to come (`blip`); a shift down
//!   past the limiter is refused. The automatic shifts by the revs and the pedals, holding its
//!   gear through a lift (`demand`). Anti-lock brakes, traction control and
//!   stability control (ESC: a wheel braked against a yaw the steering does not ask for, the engine
//!   cut while the tail is out) are on, as in the road car, each switchable off.
//! - **An electric car** (`file::ElectricFit`, the Tesla Model Y) has no engine, clutch nor
//!   gearbox: a motor on one axle or each, geared to it through an open differential, its torque
//!   by the pedal and its speed, capped by the battery's power and the top speed; regeneration
//!   when the pedal is let go, the brakes holding it once stopped; drive, neutral and reverse
//!   (`electric`, `selector`).
//! - **Air**: drag and lift (downforce) at each axle, through the wind.
//! - **Contacts**: points of the body against the ground (rolled over, grounded) and against the
//!   walls near it (`CarGround::walls`), springs and dampers with friction: it bounces off and
//!   scrapes along; striking either square faster than `BodyFit::crash_speed` wrecks it.
//! - The body is integrated semi-implicitly (velocities, then the position and attitude with them).
//!
//! All pure: the same controls on the same ground, the same drive.

pub mod driver;
pub mod file;
pub mod ground;
pub mod tyre;
pub mod wet;

pub use file::{fleet, fleet_with, set_fleet, Aero, Axle, BodyFit, Brakes, CarType, CarVisual, Differential, Drive, ElectricFit, EngineFit, GearboxFit, MotorFit, Steering};
pub use ground::{CarGround, Patch, Plane, Surface, TerrainGround, Wall, WallKind};
pub use tyre::TyreFit;

use std::f64::consts::TAU;

use file::interpolate as interpolate_table;

use crate::aircraft::Environment;
use crate::body::Body;
use crate::math::{act, euler_angles, inverse, quaternion, smoothstep, Quat, DEG, GRAVITY, V3};
use crate::vehicle::{ContactPoint, On};

/// How far ahead a driver looks at the water on the road (`CarObservation::water_ahead`), m.
pub const AHEAD_WATER: [f64; 3] = [20.0, 40.0, 70.0];

/// Sub-steps of each `World::DT`: 960 Hz.
pub const SUBSTEPS: usize = 4;
/// Most body contact points a type may have.
pub const MOST_CONTACTS: usize = 32;
/// The slip of the driven wheels traction control holds them at, going straight: a little under
/// the tyres' peak (some 0.09), so that the tyre keeps grip across as well; less as the rear tyres
/// slip across (`TRACTION_SLIP_CORNERING` at a slip angle of 0.1 rad or more), as a road car's
/// system does, which reads the lateral acceleration and the steering.
const TRACTION_SLIP: f64 = 0.11;
const TRACTION_SLIP_CORNERING: f64 = 0.05;
/// Stability control: the most brake torque it puts on one wheel, N m.
const ESC_TORQUE: f64 = 3500.0;
/// Stability control: how much more yaw than the steering asks for it lets pass, rad/s, and the
/// sideslip of the body from which it also cuts the engine, rad.
const ESC_BAND: f64 = 0.12;
const ESC_CUT_SLIDE: f64 = 2.0 * DEG;
/// Seconds the clutch takes to take up the drive after a shift, as an automated gearbox does.
const TAKE_UP: f64 = 0.25;
/// The revs the engine is held at while the clutch takes up the drive from rest at full throttle.
const LAUNCH_RPM: f64 = 3500.0;
/// Seconds a full throttle let go still counts whole for the automatic's shifts up: it fades over them.
const DEMAND_HELD: f64 = 3.0;
/// Shifting down, the throttle the gearbox opens is full this far below the revs of the gear to
/// come, rad/s, and shut at them.
const BLIP_BAND: f64 = 40.0;

/// A type of car of the fleet, by its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CarKind(&'static str);

impl CarKind {
    /// The GT coupé, the first car.
    pub const GT: CarKind = CarKind("gt");
    /// The Tesla Model Y, electric, a motor on each axle.
    pub const MODEL_Y: CarKind = CarKind("modely");

    pub fn all() -> impl Iterator<Item = CarKind> {
        fleet().iter().map(|c| CarKind(leak(&c.key)))
    }

    pub fn key(self) -> &'static str {
        self.0
    }

    pub fn name(self) -> &'static str {
        leak(&self.spec().name)
    }

    /// By its key, a word of its name or an alias.
    pub fn parse(word: &str) -> Option<CarKind> {
        let w = word.to_lowercase();
        fleet().iter().find(|c| c.key == w || c.aliases.contains(&w) || c.name.to_lowercase().split_whitespace().any(|x| x == w)).map(|c| CarKind(leak(&c.key)))
    }

    pub fn spec(self) -> &'static CarType {
        fleet().iter().find(|c| c.key == self.0).unwrap_or(&fleet()[0])
    }
}

/// The fleet lives for the whole program: its strings can be handed out as `'static`.
fn leak(s: &str) -> &'static str {
    let found = fleet().iter().flat_map(|c| [c.key.as_str(), c.name.as_str()]).find(|x| *x == s);
    found.unwrap_or("")
}

/// What a driver sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarControls {
    /// −1 full left to 1 full right.
    pub steer: f64,
    /// 0 to 1.
    pub throttle: f64,
    pub brake: f64,
    /// The clutch pedal, 0 up (engaged) to 1 down. Left up, the clutch works by itself.
    pub clutch: f64,
    pub handbrake: f64,
    /// A shift asked for: +1 up, −1 down (the car takes it and sets it back to 0).
    pub shift: i32,
    /// The gearbox shifts by itself.
    pub automatic: bool,
    /// Anti-lock brakes, traction control and stability control (ESC), on in a road car unless
    /// switched off.
    pub abs: bool,
    pub traction_control: bool,
    pub stability_control: bool,
}

impl Default for CarControls {
    fn default() -> CarControls {
        CarControls { steer: 0.0, throttle: 0.0, brake: 0.0, clutch: 0.0, handbrake: 0.0, shift: 0, automatic: true, abs: true, traction_control: true, stability_control: true }
    }
}

/// One wheel as it is now.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WheelState {
    /// The suspension's travel from where it stands at rest, m: positive down (extended).
    pub travel: f64,
    /// Spin, rad/s, and its angle (for drawing).
    pub omega: f64,
    pub angle: f64,
    /// Where it is steered, rad (positive to the right), toe included.
    pub steer: f64,
    /// Slip along (κ) and across (tan α), as the tyre has built them up.
    pub slip: f64,
    pub slip_angle: f64,
    /// Load on the tyre, N, and whether it touches.
    pub load: f64,
    pub contact: bool,
    pub surface: Surface,
    /// The tyre's forces along and across the wheel, N.
    pub force: (f64, f64),
    /// How fast the tyre slides over the ground, m/s.
    pub sliding: f64,
    /// Lean against the ground, rad (positive: the top to the right).
    pub camber: f64,
    /// Water under the tyre, m, and the share of the surface's dry grip it leaves (1 dry).
    pub water: f64,
    pub wet_share: f64,
    /// The anti-lock system's share of the brake on this wheel.
    abs: f64,
}

/// What the last step measured.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CarData {
    /// Speed over the ground, and along the body (negative backwards), m/s.
    pub speed: f64,
    pub forward: f64,
    /// Acceleration felt (the accelerometer's), body axes, m/s².
    pub accel: V3,
    /// Any wheel touches the ground.
    pub on_ground: bool,
    /// The hardest impact of the step, its speed square to what was hit, m/s; and how fast the body
    /// scrapes along a wall or the ground.
    pub impact: f64,
    pub scrape: f64,
    /// The engine's torque, N m, and whether the limiter is cutting.
    pub engine_torque: f64,
    pub limiter: bool,
    /// The clutch's engagement, 0 to 1.
    pub clutch: f64,
    /// An electric car's motors' power, W: negative regenerating.
    pub power: f64,
}

/// What its driver observes of it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CarObservation {
    /// Along the body, m/s (negative backwards).
    pub speed: f64,
    pub rpm: f64,
    /// −1 reverse, 0 neutral, 1... forward.
    pub gear: i32,
    pub gears: i32,
    pub steer: f64,
    pub throttle: f64,
    pub brake: f64,
    pub clutch: f64,
    pub handbrake: f64,
    /// Per wheel (front left, front right, rear left, rear right): its speed at the rim (ωR, m/s),
    /// slip along and across, load (N), and the surface under it.
    pub wheel_speed: [f64; 4],
    pub slip: [f64; 4],
    pub slip_angle: [f64; 4],
    pub load: [f64; 4],
    pub surface: [Surface; 4],
    /// Acceleration felt, body axes (forward, right, down), m/s².
    pub accel: [f64; 3],
    pub automatic: bool,
    /// Water under each tyre, m.
    pub water: [f64; 4],
    /// Water on the road straight ahead along the way it goes, `AHEAD_WATER` metres on, m: what
    /// the driver sees coming (a puddle shines).
    pub water_ahead: [f64; 3],
    /// How hard it rains, 0 to 1 (the world's weather: the driver sees it).
    pub rain: f64,
}

/// What is seen of a car at a moment, for a replay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarLook {
    pub body: Body,
    pub controls: CarControls,
    pub wheels: [WheelState; 4],
    pub engine: f64,
    pub gear: i32,
    pub data: CarData,
    pub crashed: Option<&'static str>,
}

impl CarLook {
    /// `k` of the way (0 to 1) to `next`, `dt` seconds later.
    pub fn blend(&self, next: &CarLook, k: f64, dt: f64) -> CarLook {
        let mut wheels = self.wheels;
        for (w, n) in wheels.iter_mut().zip(&next.wheels) {
            w.travel += (n.travel - w.travel) * k;
            w.steer += (n.steer - w.steer) * k;
            w.angle += (n.angle - w.angle) * k;
        }
        CarLook { body: self.body.blend(&next.body, k, dt), wheels, engine: self.engine + (next.engine - self.engine) * k, ..*self }
    }
}

/// A car: see the module.
#[derive(Clone, Debug)]
pub struct Car {
    pub p: CarType,
    /// Placed by its centre of gravity.
    pub body: Body,
    pub controls: CarControls,
    wheels: [WheelState; 4],
    /// The engine's speed, rad/s.
    engine: f64,
    gear: i32,
    /// A shift under way: seconds left, and to which gear; the clutch taking up after it, 0 to 1.
    shift_left: f64,
    shift_to: i32,
    take_up: f64,
    since_shift: f64,
    clutch_locked: bool,
    limiter: bool,
    /// Traction control's share of the throttle.
    traction: f64,
    /// The throttle the automatic shifts up by: the most of the last seconds (`DEMAND_HELD`).
    demand: f64,
    data: CarData,
    crashed: Option<&'static str>,
    sunk: bool,
    /// Seconds it has been upside down.
    upside_down: f64,
    previous: Body,
    /// The ground under each wheel and under the body, kept for a step where it is the terrain's.
    patches: [(Patch, f64, f64); 4],
    under: (Patch, f64, f64),
    /// The walls near it this step.
    walls: Vec<crate::car::ground::Wall>,
    /// Which body points touched something in the last sub-step (an impact is the first touch).
    touching: u64,
    /// The road's grain under each wheel: a phase for the rough surfaces.
    seed: u64,
    /// Stability control's yaw rate expected, rad/s (the steering's, lagged as the car's yaw is).
    esc_yaw: f64,
    /// The share of its drag (and lift) taken away by the wake of a car ahead (`slipstream`).
    pub draft: f64,
    /// An electric car's motors' speeds, rad/s (front, rear), and traction control's share of
    /// each one's torque.
    motor: [f64; 2],
    pull: [f64; 2],
}

impl Car {
    /// One of a kind of the fleet, at the origin.
    pub fn of(kind: CarKind) -> Car {
        Car::with_type(kind.spec().clone())
    }

    pub fn with_type(p: CarType) -> Car {
        let body = Body { pos: V3::ZERO, vel: V3::ZERO, q: Quat::IDENTITY, omega: V3::ZERO };
        let level = Patch { height: 0.0, normal: V3::new(0.0, 0.0, -1.0), surface: Surface::Asphalt, exact: false, water: 0.0, paint: 0.0 };
        let idle = p.engine.idle_rpm * TAU / 60.0;
        Car {
            p,
            body,
            controls: CarControls::default(),
            wheels: [WheelState { abs: 1.0, ..WheelState::default() }; 4],
            engine: idle,
            gear: 1,
            shift_left: 0.0,
            shift_to: 1,
            take_up: 1.0,
            since_shift: 10.0,
            clutch_locked: false,
            limiter: false,
            traction: 1.0,
            demand: 0.0,
            data: CarData::default(),
            crashed: None,
            sunk: false,
            upside_down: 0.0,
            previous: body,
            patches: [(level, 0.0, 0.0); 4],
            under: (level, 0.0, 0.0),
            walls: Vec::new(),
            touching: 0,
            seed: 1,
            esc_yaw: 0.0,
            draft: 0.0,
            motor: [0.0; 2],
            pull: [1.0; 2],
        }
    }

    pub(crate) fn seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    pub fn data(&self) -> &CarData {
        &self.data
    }

    pub fn wheels(&self) -> &[WheelState; 4] {
        &self.wheels
    }

    /// The engine's speed, rpm.
    pub fn rpm(&self) -> f64 {
        self.engine * 60.0 / TAU
    }

    /// −1 reverse, 0 neutral, 1... forward; during a shift, the gear it is going to.
    pub fn gear(&self) -> i32 {
        if self.shift_left > 0.0 {
            self.shift_to
        } else {
            self.gear
        }
    }

    /// Whether a shift is under way.
    pub fn shifting(&self) -> bool {
        self.shift_left > 0.0
    }

    pub fn crashed(&self) -> Option<&'static str> {
        self.crashed
    }

    /// In the water: nothing moves it any more.
    pub fn sunk(&self) -> bool {
        self.sunk
    }

    /// At rest on its wheels at (`n`, `e`) facing `heading`, on the ground there, in first gear
    /// with the engine idling, nothing broken.
    pub fn stand<G: CarGround>(&mut self, ground: &G, n: f64, e: f64, heading: f64) {
        let patch = ground.patch(n, e);
        // Pitch and roll of the ground under it, for the heading.
        let up = patch.normal;
        let (c, s) = (heading.cos(), heading.sin());
        let along = -(up.x * c + up.y * s) / -up.z;
        let across = -(-up.x * s + up.y * c) / -up.z;
        let q = quaternion(heading, along.atan(), -across.atan());
        let height = patch.height + self.p.cg_height;
        self.body = Body { pos: V3::new(n, e, -height), vel: V3::ZERO, q, omega: V3::ZERO };
        self.previous = self.body;
        let c = self.controls;
        self.controls = CarControls { automatic: c.automatic, abs: c.abs, traction_control: c.traction_control, stability_control: c.stability_control, ..CarControls::default() };
        self.renew();
        // The springs as they settle at rest.
        for w in &mut self.wheels {
            w.contact = true;
        }
    }

    /// As new: engine idling in first, nothing crashed.
    fn renew(&mut self) {
        self.wheels = [WheelState { abs: 1.0, ..WheelState::default() }; 4];
        self.engine = self.p.engine.idle_rpm * TAU / 60.0;
        self.gear = 1;
        self.shift_left = 0.0;
        self.shift_to = 1;
        self.take_up = 1.0;
        self.since_shift = 10.0;
        self.clutch_locked = false;
        self.limiter = false;
        self.traction = 1.0;
        self.demand = 0.0;
        self.data = CarData::default();
        self.crashed = None;
        self.sunk = false;
        self.upside_down = 0.0;
        self.touching = 0;
        self.motor = [0.0; 2];
        self.pull = [1.0; 2];
    }

    /// Put in a body set by hand, as it drives: its wheels turning with it, the engine with them
    /// in the gear it is in.
    pub fn put(&mut self, body: Body) {
        self.body = body;
        self.previous = body;
        // The wheels turning with it.
        let forward = body.vel.x;
        for (i, w) in self.wheels.iter_mut().enumerate() {
            w.omega = forward / self.p.axles[i / 2].tyre.radius;
            w.slip = 0.0;
            w.slip_angle = 0.0;
        }
        if let Some(e) = &self.p.electric {
            for m in &e.motors {
                let a = m.axle();
                self.motor[a] = forward / self.p.axles[a].tyre.radius * m.ratio;
            }
            self.engine = self.motor[self.first_motor()];
            return;
        }
        let ratio = self.p.ratio(self.gear);
        if ratio != 0.0 {
            let idle = self.p.engine.idle_rpm * TAU / 60.0;
            self.engine = (forward / self.p.axles[1].tyre.radius * ratio).max(idle);
            self.clutch_locked = true;
        }
    }

    /// Sets the gear at once (for tests and starts): −1 reverse, 0 neutral, 1 first...
    pub fn set_gear(&mut self, gear: i32) {
        self.gear = gear.clamp(-1, self.p.gearbox.ratios.len() as i32);
        self.shift_left = 0.0;
        self.shift_to = self.gear;
    }

    /// One step of `dt` on `ground` in the air of `env`.
    pub fn step<G: CarGround>(&mut self, dt: f64, env: &Environment, ground: &G) {
        if self.sunk {
            return;
        }
        self.previous = self.body;
        self.sample_ground(ground, dt);
        self.gearbox(dt);
        self.data.impact = 0.0;
        self.data.scrape = 0.0;
        let h = dt / SUBSTEPS as f64;
        let v0 = self.body.vel;
        for _ in 0..SUBSTEPS {
            self.substep(h, env, ground);
        }
        let b = &self.body;
        let accel = (b.vel - v0) / dt + b.omega.cross(b.vel);
        let gravity = act(inverse(b.q), V3::new(0.0, 0.0, GRAVITY));
        self.data.accel = accel - gravity;
        self.data.speed = b.world_velocity().length();
        self.data.forward = b.vel.x;
        self.data.on_ground = self.wheels.iter().any(|w| w.contact);
        self.judge(dt);
    }

    /// What wrecks it besides an impact: upside down for 3 s, or in the water.
    fn judge(&mut self, dt: f64) {
        let up = act(self.body.q, V3::new(0.0, 0.0, -1.0));
        if up.z > 0.2 && self.data.speed < 3.0 {
            self.upside_down += dt;
        } else {
            self.upside_down = 0.0;
        }
        if self.upside_down > 3.0 && self.crashed.is_none() {
            self.crashed = Some("Rolled over");
        }
        let (under, ..) = self.under;
        if under.surface == Surface::Water && -self.body.pos.z < under.height + 0.3 {
            if self.crashed.is_none() {
                self.crashed = Some("Drove into the water");
            }
            if self.data.speed < 0.5 {
                self.sunk = true;
            }
        }
    }

    /// Finds the ground under each wheel and the body, and the walls near it, for this step.
    fn sample_ground<G: CarGround>(&mut self, ground: &G, dt: f64) {
        let b = self.body;
        for i in 0..4 {
            let (at, _) = b.point(self.p.hub(i) + V3::new(0.0, 0.0, self.wheels[i].travel));
            self.patches[i] = (ground.patch(at.x, at.y), at.x, at.y);
        }
        self.under = (ground.patch(b.pos.x, b.pos.y), b.pos.x, b.pos.y);
        self.walls.clear();
        ground.walls(b.pos.x, b.pos.y, 4.0 + b.vel.length() * dt, &mut self.walls);
    }

    /// The ground at a point near wheel `i` (or the body's middle, `None`): the circuit's exactly,
    /// the terrain's as the plane found for this step.
    fn ground_at<G: CarGround>(&self, ground: &G, i: Option<usize>, n: f64, e: f64) -> Patch {
        let (p, n0, e0) = match i {
            Some(i) => self.patches[i],
            None => self.under,
        };
        if p.exact {
            return ground.patch(n, e);
        }
        Patch { height: p.height_at(n0, e0, n, e), ..p }
    }

    /// Shifts asked for or decided by the automatic, and those under way.
    fn gearbox(&mut self, dt: f64) {
        if self.p.electric.is_some() {
            self.selector(dt);
            return;
        }
        let top = self.p.gearbox.ratios.len() as i32;
        self.since_shift += dt;
        self.demand = self.controls.throttle.clamp(0.0, 1.0).max(self.demand - dt / DEMAND_HELD);
        if self.shift_left > 0.0 {
            self.shift_left -= dt;
            if self.shift_left <= 0.0 {
                self.gear = self.shift_to;
                self.take_up = 0.0;
                self.clutch_locked = false;
            }
            self.controls.shift = 0;
            return;
        }
        let ask = self.controls.shift.signum();
        self.controls.shift = 0;
        let forward = self.body.vel.x;
        let mut to = self.gear;
        if ask != 0 {
            to = (self.gear + ask).clamp(-1, top);
            // The automatic goes from first straight to reverse and back.
            if self.controls.automatic && to == 0 {
                to = ask.signum();
            }
            // Into reverse only stopped, and out of it only stopped going forward.
            if to == -1 && forward > 1.5 {
                to = self.gear;
            }
            // A shift down that would turn the engine past its limiter is not taken, as an
            // automated gearbox refuses it (it would lock the driven wheels).
            if to >= 1 && to < self.gear && self.wheel_rpm() * self.p.ratio(to) / self.p.ratio(self.gear) > self.p.engine.limiter_rpm {
                to = self.gear;
            }
        } else if self.controls.automatic && self.gear >= 1 && self.since_shift > 0.6 && self.crashed.is_none() {
            let rpm = self.wheel_rpm();
            let t = self.controls.throttle;
            let g = &self.p.gearbox;
            // Up at the revs the throttle asks for (the shift point at full throttle, much lower
            // cruising); down when the revs fall below what the pedal asks, or braking, as soon as
            // the lower gear keeps the engine below its shift point (the engine kept on the boil).
            // The throttle that counts for shifting up is the most of the last seconds (`demand`):
            // lifting off for a corner and coming back on the pedal is not cruising, and the gear
            // is held through it, as a sports automatic holds it.
            let up_at = 4000.0 + (g.auto_up_rpm - 4000.0) * smoothstep(0.0, 0.7, self.demand);
            let down_at = 1700.0 + (g.auto_down_rpm - 1700.0) * t;
            let lower = if self.gear > 1 { rpm * self.p.ratio(self.gear - 1) / self.p.ratio(self.gear) } else { f64::INFINITY };
            let braking = self.controls.brake > 0.1;
            if self.gear < top && (rpm > up_at && t > 0.05 && !braking || rpm > self.p.engine.limiter_rpm - 150.0) {
                to = self.gear + 1;
            } else if braking && lower < g.auto_up_rpm - 1200.0 || rpm < down_at && lower < up_at - 600.0 {
                to = self.gear - 1;
            }
        }
        if to != self.gear {
            self.shift_to = to;
            self.shift_left = if self.gear == 0 || to == 0 { self.p.gearbox.shift_time * 0.5 } else { self.p.gearbox.shift_time };
            self.since_shift = 0.0;
        }
    }

    /// An electric car's selector: drive (1), neutral (0) or reverse (−1). A shift up from reverse
    /// is drive; a shift down from drive is reverse, taken only nearly stopped.
    fn selector(&mut self, dt: f64) {
        self.since_shift += dt;
        if self.shift_left > 0.0 {
            self.shift_left -= dt;
            if self.shift_left <= 0.0 {
                self.gear = self.shift_to;
            }
            self.controls.shift = 0;
            return;
        }
        let ask = self.controls.shift.signum();
        self.controls.shift = 0;
        let to = match ask {
            1 if self.gear < 1 && self.body.vel.x > -1.5 => 1,
            -1 if self.gear > -1 && self.body.vel.x < 1.5 => -1,
            _ => self.gear,
        };
        if to != self.gear {
            self.shift_to = to;
            self.shift_left = self.p.gearbox.shift_time;
            self.since_shift = 0.0;
        }
    }

    /// The axle of the motor that stands for the engine (the rear one, or the only one).
    fn first_motor(&self) -> usize {
        self.p.electric.as_ref().map_or(1, |e| e.motors.iter().find(|m| m.axle == Drive::Rear).unwrap_or(&e.motors[0]).axle())
    }

    /// The engine's speed the driven wheels would turn it at in the gear engaged, rpm.
    fn wheel_rpm(&self) -> f64 {
        let (a, b) = self.driven();
        let axle = (self.wheels[a].omega + self.wheels[b].omega) / 2.0;
        (axle * self.p.ratio(self.gear)).abs() * 60.0 / TAU
    }

    /// The throttle while a gear changes: shut going up; going down, opened by the gearbox to
    /// bring the engine to the revs it will turn at in the lower gear (the blip of an automated
    /// gearbox, a driver's heel and toe), so that the clutch takes up the drive without braking
    /// the driven wheels.
    fn blip(&self) -> f64 {
        if self.shift_to < 1 || self.shift_to >= self.gear {
            return 0.0;
        }
        let (a, b) = self.driven();
        let wanted = ((self.wheels[a].omega + self.wheels[b].omega) / 2.0 * self.p.ratio(self.shift_to)).min(self.p.engine.limiter_rpm * TAU / 60.0);
        ((wanted - self.engine) / BLIP_BAND).clamp(0.0, 1.0)
    }

    /// The driven wheels.
    fn driven(&self) -> (usize, usize) {
        match self.p.drive {
            file::Drive::Rear | file::Drive::All => (2, 3),
            file::Drive::Front => (0, 1),
        }
    }

    /// How much the clutch is engaged, 0 to 1: the pedal's, or the clutch working by itself.
    fn clutch(&mut self, h: f64, engine_torque: f64) -> f64 {
        if self.gear == 0 || self.shift_left > 0.0 {
            self.clutch_locked = false;
            return 0.0;
        }
        self.take_up = (self.take_up + h / TAKE_UP).min(1.0);
        if self.controls.clutch > 0.02 {
            self.clutch_locked = false;
            return (1.0 - self.controls.clutch) * self.take_up;
        }
        let idle = self.p.engine.idle_rpm * TAU / 60.0;
        let (a, b) = self.driven();
        let wheels = (self.wheels[a].omega + self.wheels[b].omega) / 2.0 * self.p.ratio(self.gear);
        let we = self.engine;
        if self.clutch_locked && we < idle * 1.08 && wheels.abs() < idle * 1.1 && self.controls.throttle < 0.1 {
            self.clutch_locked = false;
        }
        if self.clutch_locked {
            return self.take_up;
        }
        // Taking up the drive from rest as a driver does: the revs held up (more the more throttle)
        // while the clutch slips, passing the engine's torque and what slows it to those revs.
        let launch = if self.controls.throttle > 0.02 {
            let revs = idle + (LAUNCH_RPM * TAU / 60.0 - idle) * self.controls.throttle;
            ((engine_torque + self.p.engine.inertia * (we - revs) / 0.08) / self.p.gearbox.clutch_torque).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Rolling (coasting in gear, or the drive taken up after a shift): engaged progressively
        // when the wheels would turn the engine above idle and it is not racing ahead of them.
        let rolling = if self.controls.throttle < 0.1 || we <= wheels.abs() + 10.0 { smoothstep(idle * 1.15, idle * 1.6, wheels.abs()) } else { 0.0 };
        let c = launch.max(rolling);
        if c > 0.2 && (we - wheels).abs() < 8.0 {
            self.clutch_locked = true;
        }
        c * self.take_up
    }

    /// One sub-step of `h`.
    fn substep<G: CarGround>(&mut self, h: f64, env: &Environment, ground: &G) {
        let p = &self.p;
        let b = self.body;
        let inv = inverse(b.q);
        let down = act(b.q, V3::Z);
        let mut force = act(inv, V3::new(0.0, 0.0, p.mass * GRAVITY));
        let mut moment = V3::ZERO;
        let crashed = self.crashed.is_some();
        let controls = if crashed { CarControls { throttle: 0.0, brake: 0.4, steer: 0.0, ..self.controls } } else { self.controls };

        // Steering: the inner wheel more than the outer as far as `ackermann` goes.
        let steer = controls.steer.clamp(-1.0, 1.0) * p.steering.lock_deg * DEG;
        let (left, right) = ackermann(steer, p.wheelbase(), p.axles[0].track, p.steering.ackermann);

        let mut tyre_torque = [0.0; 4];
        let mut rolling = [0.0; 4];
        let travels: [f64; 4] = std::array::from_fn(|i| self.wheels[i].travel);
        for i in 0..4 {
            let axle = &p.axles[i / 2];
            let tyre = &axle.tyre;
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            let w = self.wheels[i];
            let (hub, _) = b.point(p.hub(i) + V3::new(0.0, 0.0, w.travel));
            let patch = self.ground_at(ground, Some(i), hub.x, hub.y);
            let up = patch.normal;
            let cos = -down.dot(up);
            let rough = patch.surface.roughness() * bumps(hub.x, hub.y, self.seed);
            // How high the hub is over the ground, along its normal, and the travel at which the
            // tyre would just touch.
            let above = (-hub.z - patch.height - rough) * -up.z;
            let touch = if cos > 0.3 { w.travel + (above - tyre.radius) / cos } else { f64::INFINITY };

            // The hub's travel: the spring, damper, anti-roll bar and bump stop against the tyre's spring.
            let k = axle.spring;
            let preload = p.static_load(i / 2) / k;
            let other = travels[i ^ 1];
            let kt = tyre.vertical_stiffness * cos.max(0.0).powi(2);
            let stop = -(axle.bump_travel - axle.bump_stop_gap);
            // The suspension's links carry the tyre's force across to the body along the line from
            // the contact to the axle's roll centre, so they push the hub down (and the body up) by
            // that force times the line's slope: the share of the load moved across that does not
            // go through the springs (Milliken, ch. 17). From the force of the sub-step before.
            let across = w.force.0 * w.steer.sin() + w.force.1 * w.steer.cos();
            let link = -side * across * axle.roll_centre / (axle.track / 2.0);
            let solve = |c: f64, touching: bool, stopped: bool| {
                let mut den = c / h + k + axle.anti_roll;
                let mut rhs = c / h * w.travel + k * preload + axle.anti_roll * other + link;
                if touching {
                    den += kt;
                    rhs += kt * touch;
                }
                if stopped {
                    den += axle.bump_stop;
                    rhs += axle.bump_stop * stop;
                }
                rhs / den
            };
            let mut touching = false;
            let mut stopped = false;
            let mut c = (axle.bump + axle.rebound) / 2.0;
            let mut travel = solve(c, touching, stopped);
            for _ in 0..3 {
                touching = travel > touch;
                stopped = travel < stop;
                c = if travel < w.travel { axle.bump } else { axle.rebound };
                travel = solve(c, touching, stopped);
            }
            travel = travel.clamp(-axle.bump_travel, axle.droop_travel);
            let squash = if cos > 0.3 { (travel - touch).max(0.0) * cos } else { 0.0 };
            let load = tyre.vertical_stiffness * squash;
            let wheel = &mut self.wheels[i];
            wheel.travel = travel;
            wheel.contact = load > 0.0;
            wheel.load = load;
            wheel.surface = patch.surface;

            // The wheel's direction: steered, toed in, bump-steered.
            let toe = (axle.toe_deg + axle.bump_steer_deg * -travel) * DEG * -side;
            wheel.steer = toe + if i < 2 { if i == 0 { left } else { right } } else { 0.0 };
            if load <= 0.0 {
                wheel.force = (0.0, 0.0);
                wheel.sliding = 0.0;
                continue;
            }
            let hub = hub + down * (travel - w.travel);
            let contact = hub - up * (above - (travel - w.travel) * cos).max(0.0);
            let r = act(inv, contact - b.pos);
            let v = act(b.q, b.vel + b.omega.cross(r));
            let heading = act(b.q, V3::new(wheel.steer.cos(), wheel.steer.sin(), 0.0));
            let fwd = (heading - up * heading.dot(up)).normalize_or_zero();
            let lat = fwd.cross(up);
            let vx = v.dot(fwd);
            let vy = v.dot(lat);
            let rim = wheel.omega * tyre.radius;

            // The slips, built up over the relaxation lengths as the tyre rolls.
            let speed = vx.abs();
            let relax = |state: f64, target_rate: f64, length: f64| {
                if speed > 1e-3 {
                    let decay = (-speed * h / length).exp();
                    state * decay + target_rate / speed * (1.0 - decay)
                } else {
                    state + target_rate * h / length
                }
            };
            wheel.slip = relax(wheel.slip, rim - vx, tyre.relaxation_longitudinal).clamp(-1.5, 1.5);
            wheel.slip_angle = relax(wheel.slip_angle, -vy, tyre.relaxation_lateral).clamp(-3.0, 3.0);

            // Camber: the wheel's lean in the body, with its travel, and the body's roll on the ground.
            let lean_body = (axle.camber_deg + axle.camber_gain_deg * -travel) * DEG * side;
            let roll = -act(b.q, V3::Y).dot(up).asin();
            wheel.camber = -lean_body + roll;
            // The water the tyre rides on: deeper water lifts it only once it has rolled into it
            // for a while (the wedge of water builds under the contact over `wet::WEDGE`), and
            // it is back on the road as soon as it leaves; a short puddle twitches the car, a long
            // one lets go of it.
            let rolled = (vx.abs() * h / wet::WEDGE).min(1.0);
            wheel.water = if patch.water > wheel.water { wheel.water + (patch.water - wheel.water) * rolled } else { patch.water };
            // The grip of what it rolls on, less under water (`wet`): the deeper and the faster, the less.
            let (grip, wet) = wet::wet_grip(patch.surface, patch.paint, wheel.water, vx.abs());
            wheel.wet_share = grip / patch.surface.grip();
            let (mut fx, mut fy) = tyre.forces_wet(load, wheel.slip, wheel.slip_angle, wheel.camber, grip, wet);
            // At walking pace the slips say little: the tyre's damping brings it to rest.
            let slow = 1.0 - smoothstep(1.0, 4.0, vx.hypot(vy));
            if slow > 0.0 {
                let damping = 3.0 * load * slow;
                fx -= damping * (vx - rim);
                fy -= damping * vy;
                let most = 1.2 * tyre.grip * grip * load;
                let f = fx.hypot(fy);
                if f > most {
                    fx *= most / f;
                    fy *= most / f;
                }
            }
            wheel.force = (fx, fy);
            wheel.sliding = (vx - rim).hypot(vy);
            tyre_torque[i] = -fx * tyre.radius;
            rolling[i] = load * (patch.surface.rolling() + tyre.rolling_speed * vx * vx) * tyre.radius;

            // The load along the ground's normal and the tyre's forces, at the contact, where the
            // road makes them: all the load they move from side to side and from end to end is there.
            let at_contact = act(inv, up * load + fwd * fx + lat * fy);
            force += at_contact;
            moment += r.cross(at_contact);
        }

        // Air: drag through the wind at the drag's centre, lift (downforce) at each axle.
        let air = act(inv, b.world_velocity() - env.air_velocity);
        let q = 0.5 * env.rho * air.length_squared();
        // In the wake of a car ahead the air comes slower: less drag, and less lift (`slipstream`).
        let wake = 1.0 - self.draft;
        let drag = -0.5 * env.rho * p.aero.drag_area * air.length() * air * wake;
        let centre = V3::new(0.0, 0.0, p.cg_height - p.aero.centre_height);
        force += drag;
        moment += centre.cross(drag);
        let qx = 0.5 * env.rho * air.x * air.x;
        let _ = q;
        for (a, lift) in [(0, p.aero.lift_front), (1, p.aero.lift_rear)] {
            let f = V3::new(0.0, 0.0, -lift * qx * wake);
            force += f;
            moment += V3::new(p.axles[a].x, 0.0, 0.0).cross(f);
        }

        // The body against the ground and the walls.
        let (f, m) = self.contacts(ground);
        force += f;
        moment += m;

        // The drivetrain and the wheels' spin.
        self.drivetrain(h, &controls, &tyre_torque, &rolling);

        // The body: velocities first, then where it goes with them.
        let b = &mut self.body;
        let inertia = self.p.inertia;
        let mass = self.p.mass;
        b.vel += (force / mass - b.omega.cross(b.vel)) * h;
        let w = b.omega;
        b.omega += (moment - w.cross(inertia * w)) / inertia * h;
        let spin = Quat::from_xyzw(b.omega.x, b.omega.y, b.omega.z, 0.0);
        let dq = b.q * spin;
        b.q = Quat::from_xyzw(b.q.x + 0.5 * dq.x * h, b.q.y + 0.5 * dq.y * h, b.q.z + 0.5 * dq.z * h, b.q.w + 0.5 * dq.w * h).normalize();
        b.pos += act(b.q, b.vel) * h;
    }

    /// The forces of the body's points against the ground and the walls (body axes), and impacts.
    fn contacts<G: CarGround>(&mut self, ground: &G) -> (V3, V3) {
        let b = self.body;
        let inv = inverse(b.q);
        let (mut force, mut moment) = (V3::ZERO, V3::ZERO);
        let mut touching = 0u64;
        let crash_speed = self.p.body.crash_speed;
        let mut hardest: f64 = 0.0;
        let mut scrape: f64 = 0.0;
        let mut wrecked = None;
        for (k, &(x, y, z)) in self.p.body.contacts.iter().enumerate().take(MOST_CONTACTS) {
            let r = V3::new(x, y, z);
            let (at, v) = b.point(r);
            // The ground.
            let patch = self.ground_at(ground, None, at.x, at.y);
            let up = patch.normal;
            let depth = (at.z + patch.height) * -up.z;
            if depth > 0.0 && patch.surface != Surface::Water {
                let vn = v.dot(up);
                let normal = (4.0e5 * depth - 1.2e4 * vn).max(0.0);
                let slide = v - up * vn;
                let s = slide.length();
                let friction = -slide / s.max(0.5) * 0.5 * normal;
                let f = up * normal + friction;
                force += act(inv, f);
                moment += r.cross(act(inv, f));
                touching |= 1 << k;
                if self.touching & (1 << k) == 0 {
                    hardest = hardest.max(-vn);
                    if -vn > crash_speed {
                        wrecked = Some("Crashed");
                    }
                }
                scrape = scrape.max(s);
            }
            // The walls: the one it is deepest into.
            let mut deepest: Option<(f64, &crate::car::ground::Wall)> = None;
            for wall in &self.walls {
                let (dn, de) = (wall.b.0 - wall.a.0, wall.b.1 - wall.a.1);
                let len2 = dn * dn + de * de;
                let t = ((at.x - wall.a.0) * dn + (at.y - wall.a.1) * de) / len2.max(1e-9);
                if !(-0.05..=1.05).contains(&t) {
                    continue;
                }
                let dist = (at.x - wall.a.0) * wall.normal.0 + (at.y - wall.a.1) * wall.normal.1;
                let up_wall = -at.z - wall.base;
                if dist >= 0.0 || dist < -1.2 || up_wall < -0.4 || up_wall > wall.height + 0.1 {
                    continue;
                }
                if deepest.is_none_or(|(d, _)| -dist > d) {
                    deepest = Some((-dist, wall));
                }
            }
            if let Some((pen, wall)) = deepest {
                let n = V3::new(wall.normal.0, wall.normal.1, 0.0);
                let vn = v.dot(n);
                let (stiff, damp, mu) = wall.kind.contact();
                let normal = (stiff * pen - damp * vn).max(0.0);
                let slide = v - n * vn;
                let s = slide.length();
                let f = n * normal - slide / s.max(0.3) * mu * normal;
                force += act(inv, f);
                moment += r.cross(act(inv, f));
                touching |= 1 << (k + MOST_CONTACTS);
                if self.touching & (1 << (k + MOST_CONTACTS)) == 0 {
                    hardest = hardest.max(-vn);
                    let limit = if wall.kind == WallKind::Tyres { crash_speed * 1.4 } else { crash_speed };
                    if -vn > limit {
                        wrecked = Some(match wall.kind {
                            WallKind::Tyres => "Crashed into the tyre wall",
                            WallKind::Armco => "Crashed into the barrier",
                            WallKind::Concrete => "Crashed into the wall",
                        });
                    }
                }
                scrape = scrape.max(s);
            }
        }
        self.touching = touching;
        self.data.impact = self.data.impact.max(hardest);
        self.data.scrape = self.data.scrape.max(scrape);
        if self.crashed.is_none() {
            self.crashed = wrecked;
        }
        (force, moment)
    }

    /// The engine, clutch, gearbox, differential and brakes: the spins after `h`, given the tyres'
    /// torques on the wheels and their rolling resistance.
    fn drivetrain(&mut self, h: f64, controls: &CarControls, tyre_torque: &[f64; 4], rolling: &[f64; 4]) {
        let esc = self.stability(controls, h);
        self.anti_lock(controls, h);
        if self.p.electric.is_some() {
            self.electric(h, controls, tyre_torque, rolling, esc);
            return;
        }
        let p = &self.p;
        let idle = p.engine.idle_rpm * TAU / 60.0;
        let limiter = p.engine.limiter_rpm * TAU / 60.0;
        let (a, b) = self.driven();
        // Traction control: the engine's torque cut while a driven wheel spins past its target
        // slip, which is lower the more the rear tyres slip across.
        let spin = self.wheels[a].slip.max(self.wheels[b].slip);
        let across = self.wheels[a].slip_angle.abs().max(self.wheels[b].slip_angle.abs());
        let target = TRACTION_SLIP + (TRACTION_SLIP_CORNERING - TRACTION_SLIP) * (across / 0.1).min(1.0);
        let mut rate = if controls.traction_control && spin > target { -4.0 - 80.0 * (spin - target) } else { 2.0 };
        // Stability control: the yaw the driver asks for (the steering, a gentle understeer, no
        // more than the grip allows) against the yaw the car has, and how far its body slides
        // sideways; one wheel braked to turn it back (the outer front against oversteer, the inner
        // rear against understeer), the engine cut while the tail is out (Bosch's ESP: van Zanten,
        // *Bosch ESP Systems: 5 Years of Experience*, SAE 2000-01-1633).
        if esc.1 {
            rate = rate.min(-6.0);
        }
        self.traction = (self.traction + rate * h).clamp(0.0, 1.0);

        // The engine's torque: the throttle, the idle governor, the limiter.
        if self.engine > limiter {
            self.limiter = true;
        } else if self.engine < limiter - 250.0 * TAU / 60.0 {
            self.limiter = false;
        }
        let mut throttle = if self.shift_left > 0.0 { self.blip() } else { controls.throttle.clamp(0.0, 1.0) * self.traction };
        throttle = throttle.max(((idle * 1.04 - self.engine) / (idle * 0.25)).clamp(0.0, 0.4));
        if self.limiter {
            throttle = 0.0;
        }
        let engine_torque = p.engine.torque(self.engine, throttle);
        self.data.engine_torque = engine_torque;
        self.data.limiter = self.limiter;

        let clutch = self.clutch(h, engine_torque);
        let p = &self.p;
        self.data.clutch = clutch;
        let ratio = p.ratio(self.gear);
        let capacity = p.gearbox.clutch_torque * clutch * h;
        let ie = p.engine.inertia;
        let iw: [f64; 4] = std::array::from_fn(|i| p.axles[i / 2].tyre.inertia);

        // Free spins under the engine's and the tyres' torques.
        let mut we = self.engine + engine_torque * h / ie;
        let mut w: [f64; 4] = std::array::from_fn(|i| self.wheels[i].omega + tyre_torque[i] * h / iw[i]);

        let limits = self.brake_limits(h, controls.brake, controls.handbrake, &esc.0, rolling);
        let (mut clutch_total, mut diff_total) = (0.0f64, 0.0f64);
        let mut brake_total = [0.0f64; 4];
        let (preload, power, coast) = match p.differential {
            Differential::Open => (0.0, 0.0, 0.0),
            Differential::Limited { preload, power, coast } => (preload, power, coast),
        };
        for _ in 0..12 {
            // The clutch: the engine against the driven axle through the gears.
            if ratio != 0.0 && capacity > 0.0 {
                let axle = (w[a] + w[b]) / 2.0;
                let relative = we - ratio * axle;
                let k = 1.0 / ie + ratio * ratio / 4.0 * (1.0 / iw[a] + 1.0 / iw[b]);
                let total = (clutch_total + relative / k).clamp(-capacity, capacity);
                let lambda = total - clutch_total;
                clutch_total = total;
                we -= lambda / ie;
                w[a] += ratio * lambda / 2.0 / iw[a];
                w[b] += ratio * lambda / 2.0 / iw[b];
            }
            // The limited-slip differential: the two wheels held together up to its locking torque.
            if ratio != 0.0 {
                let through = (clutch_total / h * ratio).abs();
                let ramp = if clutch_total * ratio >= 0.0 { power } else { coast };
                let lock = (preload + ramp * through) * h;
                if lock > 0.0 {
                    let relative = w[a] - w[b];
                    let k = 1.0 / iw[a] + 1.0 / iw[b];
                    let total = (diff_total + relative / k).clamp(-lock, lock);
                    let lambda = total - diff_total;
                    diff_total = total;
                    w[a] -= lambda / iw[a];
                    w[b] += lambda / iw[b];
                }
            }
            Self::brake(&mut w, &iw, &limits, &mut brake_total);
        }
        // The gearbox's losses, as a drag on the engine's share of the torque it passes.
        let passed = clutch_total / h;
        if passed > 0.0 {
            we -= passed * (1.0 - p.gearbox.efficiency) * h / ie;
        }
        self.engine = we.clamp(0.0, limiter * 1.15);
        for (i, wheel) in self.wheels.iter_mut().enumerate() {
            wheel.omega = w[i];
            wheel.angle = (wheel.angle + w[i] * h) % TAU;
        }
    }

    /// Anti-lock: each wheel's brake let off while it slips past the target. On a wet road the
    /// tyre's peak comes at a smaller slip (its stiffness stays, its friction falls): the system
    /// estimates the road's friction from the wheels' deceleration, as modern ones do, and aims
    /// lower by as much.
    fn anti_lock(&mut self, controls: &CarControls, h: f64) {
        let moving = self.body.vel.x.abs() > 2.0;
        let slip = self.p.brakes.abs_slip;
        for w in &mut self.wheels {
            let target = slip * if w.wet_share > 0.0 { w.wet_share.clamp(0.3, 1.0) } else { 1.0 };
            let rate = if controls.abs && moving { (60.0 * (w.slip + target)).clamp(-20.0, 8.0) } else { 8.0 };
            w.abs = (w.abs + rate * h).clamp(0.1, 1.0);
        }
    }

    /// The most torque (times `h`) the brakes, stability control's and the rolling resistance hold
    /// each wheel with, for a pedal of `brake` and a hand brake of `hand`.
    fn brake_limits(&self, h: f64, brake: f64, hand: f64, esc: &[f64; 4], rolling: &[f64; 4]) -> [f64; 4] {
        let p = &self.p;
        let brake = brake.clamp(0.0, 1.0) * p.brakes.torque;
        std::array::from_fn(|i| {
            let share = if i < 2 { p.brakes.front_share } else { 1.0 - p.brakes.front_share } / 2.0;
            let hand = if i >= 2 { hand.clamp(0.0, 1.0) * p.brakes.handbrake } else { 0.0 };
            ((brake * share + esc[i]) * self.wheels[i].abs + hand + rolling[i]) * h
        })
    }

    /// One pass of the brakes and the rolling resistance, as impulses towards no spin within their
    /// torque (`total` what they have applied so far this sub-step).
    fn brake(w: &mut [f64; 4], iw: &[f64; 4], limits: &[f64; 4], total: &mut [f64; 4]) {
        for i in 0..4 {
            let t = (total[i] + w[i] * iw[i]).clamp(-limits[i], limits[i]);
            let lambda = t - total[i];
            total[i] = t;
            w[i] -= lambda / iw[i];
        }
    }

    /// An electric car's motors and brakes: the spins after `h`. Each motor turns with its axle
    /// through its reduction and an open differential (the same torque to both wheels, which may
    /// turn at their own speeds). The pedal asks for its share of the motors' torque at their
    /// speed; let go, they regenerate, up to their torque and the power the battery takes back,
    /// fading out at walking pace, where the brakes hold the car when it stops (as Tesla's "hold").
    /// The battery's power caps the two motors' together; past the top speed they stop pushing.
    /// Traction control cuts each motor's torque while its wheels spin; stability control brakes
    /// one wheel and cuts both motors, as in the engine's car. Regeneration gives way while
    /// anti-lock works or the stability control brakes.
    fn electric(&mut self, h: f64, controls: &CarControls, tyre_torque: &[f64; 4], rolling: &[f64; 4], esc: ([f64; 4], bool)) {
        let e = self.p.electric.clone().expect("an electric car");
        let iw: [f64; 4] = std::array::from_fn(|i| self.p.axles[i / 2].tyre.inertia);
        let forward = self.body.vel.x;
        let direction = match if self.shift_left > 0.0 { 0 } else { self.gear } {
            1 => 1.0,
            -1 => -1.0,
            _ => 0.0,
        };
        let pedal = controls.throttle.clamp(0.0, 1.0);

        // Traction control, per axle: the torque cut while its wheels spin past the target.
        for m in &e.motors {
            let a = m.axle();
            let (l, r) = (2 * a, 2 * a + 1);
            let spin = (self.wheels[l].slip.max(self.wheels[r].slip) * direction).max(0.0);
            let across = self.wheels[l].slip_angle.abs().max(self.wheels[r].slip_angle.abs());
            let target = TRACTION_SLIP + (TRACTION_SLIP_CORNERING - TRACTION_SLIP) * (across / 0.1).min(1.0);
            let mut rate = if controls.traction_control && spin > target { -4.0 - 80.0 * (spin - target) } else { 2.0 };
            if esc.1 {
                rate = rate.min(-6.0);
            }
            self.pull[a] = (self.pull[a] + rate * h).clamp(0.0, 1.0);
        }

        // What each motor is asked for, at its shaft.
        let top = e.top_speed_kmh / 3.6;
        let held = 1.0 - smoothstep(top - 2.0, top, forward.abs());
        let braking_wheels = self.wheels.iter().map(|w| w.abs).fold(1.0, f64::min);
        let esc_braking = esc.0.iter().any(|&t| t > 0.0);
        let mut torque = [0.0; 2];
        let (mut drive_power, mut regen_power) = (0.0, 0.0);
        for m in &e.motors {
            let a = m.axle();
            let omega = self.motor[a];
            let rpm = omega.abs() * 60.0 / TAU;
            let push = pedal * interpolate_table(&m.torque, rpm) * self.pull[a] * held * direction;
            // Regeneration: against the way it turns, faded in from walking pace.
            let fade = smoothstep(1.0, 4.0, forward.abs());
            let regen = if direction != 0.0 && !esc_braking { -(1.0 - pedal) * m.regen * fade * braking_wheels * omega.signum() } else { 0.0 };
            torque[a] = push + regen;
            drive_power += (push * omega).max(0.0);
            regen_power += (-regen * omega).max(0.0);
        }
        let most = e.battery_power * e.efficiency;
        if drive_power > most {
            for m in &e.motors {
                let a = m.axle();
                if torque[a] * self.motor[a] > 0.0 {
                    torque[a] *= most / drive_power;
                }
            }
        }
        if regen_power > e.regen_power {
            for m in &e.motors {
                let a = m.axle();
                if torque[a] * self.motor[a] < 0.0 {
                    torque[a] *= e.regen_power / regen_power;
                }
            }
        }
        // Hold: stopped with no pedal, the brakes keep it there.
        let hold = if pedal < 0.02 && direction != 0.0 && forward.abs() < 1.0 { 0.35 } else { 0.0 };
        let limits = self.brake_limits(h, controls.brake.max(hold), controls.handbrake, &esc.0, rolling);

        // Free spins under the motors' and the tyres' torques.
        let mut wm = self.motor;
        for m in &e.motors {
            let a = m.axle();
            wm[a] += torque[a] * h / m.inertia;
        }
        let mut w: [f64; 4] = std::array::from_fn(|i| self.wheels[i].omega + tyre_torque[i] * h / iw[i]);
        let mut gear_total = [0.0f64; 2];
        let mut brake_total = [0.0f64; 4];
        for _ in 0..12 {
            // Each motor geared to its axle, through an open differential: the motor's speed is
            // its ratio times the mean of the two wheels', and both get the same torque.
            for m in &e.motors {
                let a = m.axle();
                let (l, r) = (2 * a, 2 * a + 1);
                let axle = (w[l] + w[r]) / 2.0;
                let relative = wm[a] - m.ratio * axle;
                let k = 1.0 / m.inertia + m.ratio * m.ratio / 4.0 * (1.0 / iw[l] + 1.0 / iw[r]);
                let lambda = relative / k;
                gear_total[a] += lambda;
                wm[a] -= lambda / m.inertia;
                w[l] += m.ratio * lambda / 2.0 / iw[l];
                w[r] += m.ratio * lambda / 2.0 / iw[r];
            }
            Self::brake(&mut w, &iw, &limits, &mut brake_total);
        }
        // The losses from the battery to the wheels, as a drag on what the motors pass.
        for m in &e.motors {
            let a = m.axle();
            let passed = gear_total[a] / h;
            if passed * wm[a] > 0.0 {
                wm[a] -= passed * (1.0 - e.efficiency) * h / m.inertia;
            }
        }
        self.motor = wm;
        self.engine = wm[self.first_motor()];
        self.data.engine_torque = torque.iter().sum();
        self.data.power = e.motors.iter().map(|m| torque[m.axle()] * wm[m.axle()]).sum();
        self.data.limiter = held < 1.0 && pedal > 0.0;
        self.data.clutch = 1.0;
        self.limiter = self.data.limiter;
        for (i, wheel) in self.wheels.iter_mut().enumerate() {
            wheel.omega = w[i];
            wheel.angle = (wheel.angle + w[i] * h) % TAU;
        }
    }

    /// Stability control: the brake torque it puts on each wheel (N m), and whether it cuts the
    /// engine (the tail is out).
    fn stability(&mut self, controls: &CarControls, h: f64) -> ([f64; 4], bool) {
        let v = self.body.vel.x;
        if !controls.stability_control || v < 4.0 || self.crashed.is_some() {
            self.esc_yaw = self.body.omega.z;
            return ([0.0; 4], false);
        }
        let p = &self.p;
        let wheelbase = p.wheelbase();
        let delta = controls.steer.clamp(-1.0, 1.0) * p.steering.lock_deg * DEG;
        let grip = p.axles[0].tyre.grip.min(p.axles[1].tyre.grip) * self.wheels.iter().map(|w| if w.contact { w.surface.grip() * if w.wet_share > 0.0 { w.wet_share } else { 1.0 } } else { 1.0 }).fold(1.0, f64::min);
        let most = 0.85 * grip * GRAVITY / v;
        let steady = (v * delta.tan() / (wheelbase * (1.0 + 0.0018 * v * v))).clamp(-most, most);
        // The yaw the steering asks for comes as a car's does, over a tenth of a second (the
        // reference model of a real system is a car's own response): a quick twitch of the wheel
        // is not a yaw it expects at once, and it does not brake against it.
        self.esc_yaw += (steady - self.esc_yaw) * (h / 0.1).min(1.0);
        let wanted = self.esc_yaw;
        let r = self.body.omega.z;
        let beta = self.body.vel.y.atan2(v);
        // The yaw acceleration that would bring it back, rad/s²: the yaw it has against the yaw
        // asked for, past a dead band, and the sideslip beyond 4°. Turning more than asked (or
        // against the steering): oversteer, corrected firmly; turning less: understeer, corrected
        // gently past a wider band. The bands are wide enough for a driver's quick corrections of
        // the wheel, which the car follows a moment late, to be left alone.
        let same = r * wanted >= 0.0;
        let more = if same { r.abs() - wanted.abs() } else { r.abs() + wanted.abs() };
        let over = ESC_BAND + 0.2 * wanted.abs();
        let yaw = if r.abs() > 0.03 && more > over {
            -r.signum() * (more - over) * 8.0
        } else if same && wanted.abs() > 0.1 && wanted.abs() > r.abs() + 0.12 + 0.3 * wanted.abs() {
            wanted.signum() * (wanted.abs() - r.abs() - 0.12 - 0.3 * wanted.abs()) * 4.0
        } else {
            0.0
        };
        let slide = (beta.abs() - 4.0 * DEG).max(0.0) * beta.signum() * 25.0;
        let accel = yaw + slide;
        if accel.abs() < 1e-3 {
            return ([0.0; 4], false);
        }
        // Braking a wheel on the left turns it left (a negative yaw moment).
        let moment = p.inertia.z * accel;
        let left = moment < 0.0;
        // Against the way it turns now: the tail is out, the outer front is braked.
        let oversteer = r.abs() > 0.02 && moment.signum() != r.signum();
        let wheel = match (oversteer, left) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        };
        let axle = &p.axles[wheel / 2];
        let torque = (moment.abs() / (axle.track / 2.0) * axle.tyre.radius).min(ESC_TORQUE);
        let mut out = [0.0; 4];
        out[wheel] = torque;
        // The engine is cut only while the tail is really out (the body slides), not for a yaw
        // that is merely late.
        (out, oversteer && torque > 300.0 && beta.abs() > ESC_CUT_SLIDE)
    }

    /// The points of it that run into things (its body's contact points), where they are and how
    /// they move.
    pub(crate) fn contact_points(&self) -> Vec<ContactPoint> {
        let b = &self.body;
        self.p
            .body
            .contacts
            .iter()
            .map(|&(x, y, z)| {
                let r = V3::new(x, y, z);
                let (at, vel) = b.point(r);
                ContactPoint { at, vel, body: r, on: On::Body, length: 0.5 }
            })
            .collect()
    }

    /// A point of it has run into a trunk, a building or an obstacle: faster than `crash_speed` it
    /// is wrecked; slower it is stopped where it was a step before, bouncing back a little.
    pub(crate) fn strike(&mut self, point: &ContactPoint, what: &'static str) {
        let speed = point.vel.length();
        if speed > self.p.body.crash_speed {
            self.crashed.get_or_insert(what);
        }
        self.data.impact = self.data.impact.max(speed);
        let back = self.previous;
        self.body = Body { vel: -0.2 * back.vel, omega: back.omega * 0.3, ..back };
    }

    /// Takes `energy` (J) out of its motion; whether it had that much.
    pub(crate) fn absorb(&mut self, energy: f64) -> bool {
        let kinetic = 0.5 * self.p.mass * self.body.vel.length_squared();
        if kinetic <= energy {
            self.body.vel = V3::ZERO;
            return false;
        }
        self.body.vel *= (1.0 - energy / kinetic).sqrt();
        true
    }

    /// A collision against something that holds at `speed`: wrecked if faster than it can take,
    /// keeping `keep` of its velocity.
    pub(crate) fn collide(&mut self, what: &'static str, speed: f64, keep: f64) {
        if speed > self.p.body.crash_speed {
            self.crashed.get_or_insert(what);
        }
        self.data.impact = self.data.impact.max(speed);
        self.body.vel *= keep;
    }

    /// What its driver observes of it.
    pub fn observe(&self) -> CarObservation {
        let c = &self.controls;
        CarObservation {
            speed: self.body.vel.x,
            rpm: self.rpm(),
            gear: self.gear(),
            gears: self.p.gearbox.ratios.len() as i32,
            steer: c.steer,
            throttle: c.throttle,
            brake: c.brake,
            clutch: self.data.clutch,
            handbrake: c.handbrake,
            wheel_speed: std::array::from_fn(|i| self.wheels[i].omega * self.p.axles[i / 2].tyre.radius),
            slip: std::array::from_fn(|i| self.wheels[i].slip),
            slip_angle: std::array::from_fn(|i| self.wheels[i].slip_angle.atan()),
            load: std::array::from_fn(|i| self.wheels[i].load),
            surface: std::array::from_fn(|i| self.wheels[i].surface),
            accel: self.data.accel.to_array(),
            automatic: c.automatic,
            water: std::array::from_fn(|i| self.wheels[i].water),
            water_ahead: [0.0; 3],
            rain: 0.0,
        }
    }

    /// What is seen of it now, for a replay.
    pub fn look(&self) -> CarLook {
        CarLook { body: self.body, controls: self.controls, wheels: self.wheels, engine: self.engine, gear: self.gear(), data: self.data, crashed: self.crashed }
    }

    /// For a replay: looks as it did when `look` was taken. Only to be drawn.
    pub fn show(&mut self, look: &CarLook) {
        self.body = look.body;
        self.controls = look.controls;
        self.wheels = look.wheels;
        self.engine = look.engine;
        self.gear = look.gear;
        self.data = look.data;
        self.crashed = look.crashed;
    }

    /// Heading, pitch and roll, radians.
    pub fn attitude(&self) -> (f64, f64, f64) {
        let e = euler_angles(self.body.q);
        (e.heading, e.pitch, e.roll)
    }
}

/// The front wheels' angles for a steer of `delta` (positive right): the inner one turning more,
/// as far as `share` of true Ackermann geometry. (left, right).
fn ackermann(delta: f64, wheelbase: f64, track: f64, share: f64) -> (f64, f64) {
    if delta.abs() < 1e-6 {
        return (0.0, 0.0);
    }
    let radius = wheelbase / delta.abs().tan();
    let inner = (wheelbase / (radius - share * track / 2.0).max(0.1)).atan();
    let outer = (wheelbase / (radius + share * track / 2.0)).atan();
    let s = delta.signum();
    if delta > 0.0 {
        (s * outer, s * inner)
    } else {
        (s * inner, s * outer)
    }
}

/// Bumps of a rough surface at a point, −1 to 1, about a metre across.
fn bumps(n: f64, e: f64, seed: u64) -> f64 {
    crate::math::noised(n * 1.3, e * 1.3, seed ^ 0x6361_7262).v * 2.0 - 1.0
}

/// How much of a following car's drag the wake of a car ahead takes away: `gap` metres behind its
/// tail along its way, `offset` metres to its side. The wake of a car is a slower stream of air
/// that narrows and fills in behind it: right behind, a car meets air a third slower and needs some
/// 35 % less power to go as fast, at two lengths 20 %, at eight 5 %, and a car-width to the side
/// little at all (Romberg, Chianese and Lajoie, *Aerodynamics of race cars in drafting and passing
/// situations*, SAE 710213, 1971; Katz, *Race Car Aerodynamics*, 1995, ch. 9).
pub fn slipstream(gap: f64, offset: f64) -> f64 {
    if gap <= -1.0 || gap > 80.0 {
        return 0.0;
    }
    let along = 0.38 * (-gap.max(0.0) / 16.0).exp();
    let width = 0.9 + gap.max(0.0) * 0.03;
    along * (-(offset / width).powi(2)).exp()
}

/// Two cars touching: each a set of boxes (`BodyFit::solid`); a point of one inside the other's
/// boxes pushes them apart with the impulse of two rigid bodies meeting (restitution 0.25, some
/// friction), their momentum kept; harder than either's `crash_speed` wrecks both.
pub(crate) fn collide_cars(a: &mut Car, b: &mut Car) {
    for (first, second) in [(true, false), (false, true)] {
        let (x, y) = if first { (&mut *a, &mut *b) } else { (&mut *b, &mut *a) };
        let _ = second;
        let inside = |p: V3, c: &Car| -> Option<(V3, f64)> {
            let r = act(inverse(c.body.q), p - c.body.pos);
            for &((lx, ly, lz), (hx, hy, hz)) in &c.p.body.solid {
                if r.x > lx && r.x < hx && r.y > ly && r.y < hy && r.z > lz && r.z < hz {
                    // The nearest face: out through it.
                    let faces = [(r.x - lx, V3::NEG_X), (hx - r.x, V3::X), (r.y - ly, V3::NEG_Y), (hy - r.y, V3::Y)];
                    let (depth, normal) = faces.iter().copied().fold((f64::INFINITY, V3::X), |m, f| if f.0 < m.0 { f } else { m });
                    return Some((act(c.body.q, normal), depth));
                }
            }
            None
        };
        for &(px, py, pz) in &x.p.body.contacts {
            let r = V3::new(px, py, pz);
            let (at, _) = x.body.point(r);
            let Some((normal, depth)) = inside(at, y) else { continue };
            let n = V3::new(normal.x, normal.y, 0.0).normalize_or_zero();
            if n == V3::ZERO {
                continue;
            }
            // Velocities of the two points, and the impulse that parts them.
            let ra = at - x.body.pos;
            let rb = at - y.body.pos;
            let va = x.body.world_velocity() + act(x.body.q, x.body.omega).cross(ra);
            let vb = y.body.world_velocity() + act(y.body.q, y.body.omega).cross(rb);
            let closing = (va - vb).dot(n);
            if closing < 0.0 {
                let speed = -closing;
                let ma = x.p.mass;
                let mb = y.p.mass;
                let j = -(1.25) * closing / (1.0 / ma + 1.0 / mb);
                let push = n * j;
                for (car, sign, mass, rr) in [(&mut *x, 1.0, ma, ra), (&mut *y, -1.0, mb, rb)] {
                    let dv = push * sign / mass;
                    car.body.vel += act(inverse(car.body.q), dv);
                    let torque = act(inverse(car.body.q), rr.cross(push * sign));
                    car.body.omega += torque / car.p.inertia * 0.5;
                    car.data.impact = car.data.impact.max(speed);
                    if speed > car.p.body.crash_speed {
                        car.crashed.get_or_insert("Collision with another car");
                    }
                }
            }
            // Out of each other by the depth, shared.
            let shift = n * (depth * 0.5);
            x.body.pos += shift;
            y.body.pos -= shift;
            return;
        }
    }
}
