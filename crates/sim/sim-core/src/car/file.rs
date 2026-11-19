//! Types of car as data: `assets/cars/<key>/car.ron` read into a `CarType` and checked, and the
//! fleet of them (`fleet`). As `aircraft::file`, nothing here reads the disk: the game hands the
//! files' text over (`fleet_with`, `set_fleet`); without that the fleet is the cars built into the
//! program. The units are SI (m, kg, N, s, radians) unless a field's name says otherwise (`_deg`,
//! `_rpm`); positions are body axes (x forward, y right, z down) from the centre of gravity.

use std::sync::OnceLock;

use serde::Deserialize;

use super::tyre::TyreFit;
use crate::aircraft::file::FileError;
use crate::math::V3;

/// The cars of `assets/cars` built into the program: (where it is in the repository, its text).
pub const BUILT_IN: [(&str, &str); 2] = [
    ("assets/cars/gt/car.ron", include_str!("../../../../../assets/cars/gt/car.ron")),
    ("assets/cars/modely/car.ron", include_str!("../../../../../assets/cars/modely/car.ron")),
];

/// What the game draws a car with.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarVisual {
    /// The folder of `assets/cars` its model is in.
    pub model: String,
    /// Where the driver's eyes are, and the camera on the bonnet.
    pub driver_eye: (f64, f64, f64),
    pub bonnet_eye: (f64, f64, f64),
}

/// One axle's suspension and tyres (both sides the same).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axle {
    /// Metres ahead of the centre of gravity (negative behind).
    pub x: f64,
    /// Between the middles of the two tyres, m.
    pub track: f64,
    /// Wheel rate: the spring's stiffness as felt at the wheel, N/m.
    pub spring: f64,
    /// Damping at the wheel, N per m/s, compressing and extending.
    pub bump: f64,
    pub rebound: f64,
    /// How far the wheel moves up from where it stands at rest, and down, m.
    pub bump_travel: f64,
    pub droop_travel: f64,
    /// Stiffness of the rubber bump stop, N/m, met this far before the end of the bump travel.
    pub bump_stop: f64,
    pub bump_stop_gap: f64,
    /// Anti-roll bar, N/m at the wheel per metre of difference between the two sides.
    pub anti_roll: f64,
    /// Camber at rest (negative: the top of the wheel leans in) and its change per metre of bump.
    pub camber_deg: f64,
    pub camber_gain_deg: f64,
    /// Toe at rest (positive: in) and its change per metre of bump (bump steer).
    pub toe_deg: f64,
    pub bump_steer_deg: f64,
    /// Height of the roll centre above the ground, m: the share of the load moved across the axle
    /// that its links carry, not its springs (the tyres' side force times this over half the track).
    pub roll_centre: f64,
    pub tyre: TyreFit,
}

/// The steering.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steering {
    /// The front wheels' angle at full lock.
    pub lock_deg: f64,
    /// The share of true Ackermann geometry (the inner wheel turning more).
    pub ackermann: f64,
    /// Turns of the steering wheel from lock to lock (for drawing it).
    pub wheel_turns: f64,
}

/// The engine.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineFit {
    /// Torque at full throttle against speed: (rpm, N m).
    pub torque: Vec<(f64, f64)>,
    /// Torque of the closed throttle (friction and pumping) at 0 rpm and per 1000 rpm, N m (both positive).
    pub drag: (f64, f64),
    /// Crankshaft, flywheel and clutch turning with it, kg m².
    pub inertia: f64,
    pub idle_rpm: f64,
    /// Where the fuel is cut, and where the shift light comes on.
    pub limiter_rpm: f64,
    pub redline_rpm: f64,
    /// For the sound.
    pub cylinders: u32,
}

impl EngineFit {
    /// Torque at a speed (rad/s) and throttle (0 to 1), N m.
    pub fn torque(&self, omega: f64, throttle: f64) -> f64 {
        let rpm = omega * 60.0 / std::f64::consts::TAU;
        let full = interpolate(&self.torque, rpm.max(0.0));
        let drag = -(self.drag.0 + self.drag.1 * rpm.max(0.0) / 1000.0);
        throttle * full + (1.0 - throttle) * drag
    }

    /// The most torque it gives, N m.
    pub fn peak_torque(&self) -> f64 {
        self.torque.iter().map(|p| p.1).fold(0.0, f64::max)
    }

    /// Its power at full throttle at a speed (rpm), W.
    pub fn power(&self, rpm: f64) -> f64 {
        interpolate(&self.torque, rpm) * rpm * std::f64::consts::TAU / 60.0
    }
}

/// The gearbox and the clutch.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearboxFit {
    /// Forward gears' ratios, first first.
    pub ratios: Vec<f64>,
    pub reverse: f64,
    pub final_drive: f64,
    /// Share of the torque that reaches the wheels.
    pub efficiency: f64,
    /// Seconds a shift takes, the clutch out and the fuel cut.
    pub shift_time: f64,
    /// The most torque the clutch passes before it slips, N m.
    pub clutch_torque: f64,
    /// The automatic's shift points at full throttle, rpm: up, and down when the lower gear would be below this.
    pub auto_up_rpm: f64,
    pub auto_down_rpm: f64,
}

/// The differential between the driven wheels.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum Differential {
    Open,
    /// A clutch-pack limited-slip differential: it locks the wheels together against `preload`
    /// (N m) plus a share of the torque through it, `power` driving and `coast` on the overrun.
    Limited { preload: f64, power: f64, coast: f64 },
}

/// Which wheels the engine drives: an electric car with a motor on each axle drives `All`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Drive {
    Rear,
    Front,
    All,
}

/// One axle's motor of an electric car.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorFit {
    /// `Front` or `Rear`: the axle it drives through its reduction and an open differential.
    pub axle: Drive,
    /// Torque at full pedal against speed, at its shaft: (rpm, N m).
    pub torque: Vec<(f64, f64)>,
    /// Its reduction to the wheels.
    pub ratio: f64,
    /// Its rotor, kg m².
    pub inertia: f64,
    /// The most torque it takes back regenerating, at its shaft, N m.
    pub regen: f64,
}

impl MotorFit {
    /// The axle it drives: 0 front, 1 rear.
    pub fn axle(&self) -> usize {
        if self.axle == Drive::Front {
            0
        } else {
            1
        }
    }
}

/// What drives an electric car: its motors and its battery.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElectricFit {
    /// One per axle at most.
    pub motors: Vec<MotorFit>,
    /// The most power the battery gives, and the most it takes back regenerating, W.
    pub battery_power: f64,
    pub regen_power: f64,
    /// Share of the battery's power that reaches the wheels.
    pub efficiency: f64,
    /// Where its software stops the motors pushing, km/h.
    pub top_speed_kmh: f64,
}

/// The brakes.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brakes {
    /// Torque of all four at full pedal, N m.
    pub torque: f64,
    /// Share of it on the front axle.
    pub front_share: f64,
    /// Torque of the hand brake on each rear wheel, N m.
    pub handbrake: f64,
    /// The slip the anti-lock system keeps the wheels at, when it is on.
    pub abs_slip: f64,
}

/// The air on the body.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aero {
    /// Drag coefficient times frontal area, m².
    pub drag_area: f64,
    /// Lift coefficient times area at each axle, m²: negative is downforce.
    pub lift_front: f64,
    pub lift_rear: f64,
    /// Height of the drag's centre above the ground, m.
    pub centre_height: f64,
}

/// A point of the body as its file gives it: (x, y, z).
pub type Point = (f64, f64, f64);

/// What the body is to the things it touches.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyFit {
    /// Boxes (lower and upper corners) another vehicle runs into.
    pub solid: Vec<(Point, Point)>,
    /// Points of the body that touch the ground and walls when the wheels do not keep it off.
    pub contacts: Vec<(f64, f64, f64)>,
    /// An impact into a wall or the ground faster than this (m/s, square to it) wrecks it.
    pub crash_speed: f64,
}

/// A type of car: see the module.
#[derive(Clone, Debug, PartialEq)]
pub struct CarType {
    pub key: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub visual: CarVisual,
    /// With the driver and a half tank, kg.
    pub mass: f64,
    /// Moments of inertia about the roll, pitch and yaw axes through the centre of gravity, kg m².
    pub inertia: V3,
    /// Height of the centre of gravity above the ground at rest, m.
    pub cg_height: f64,
    /// Front then rear.
    pub axles: [Axle; 2],
    pub steering: Steering,
    /// An electric car's: the first motor of `electric` as an engine of no idle, with one gear
    /// of its reduction (for what reads the revs and the gear: the HUD, the sound, a driver).
    pub engine: EngineFit,
    pub gearbox: GearboxFit,
    pub differential: Differential,
    pub drive: Drive,
    /// Its motors and battery, an electric car: then `engine` and `gearbox` only stand for them.
    pub electric: Option<ElectricFit>,
    pub brakes: Brakes,
    pub aero: Aero,
    pub body: BodyFit,
    /// Body z of each axle's hubs at rest (worked out from the loads, the tyres and `cg_height`).
    pub hub_z: [f64; 2],
    /// `body.solid` as vectors: what another vehicle runs into.
    pub solid: Vec<(V3, V3)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parsed {
    key: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    visual: CarVisual,
    mass: f64,
    inertia: (f64, f64, f64),
    cg_height: f64,
    front: Axle,
    rear: Axle,
    steering: Steering,
    #[serde(default)]
    engine: Option<EngineFit>,
    #[serde(default)]
    gearbox: Option<GearboxFit>,
    #[serde(default)]
    differential: Option<Differential>,
    #[serde(default)]
    drive: Option<Drive>,
    #[serde(default)]
    electric: Option<ElectricFit>,
    brakes: Brakes,
    aero: Aero,
    body: BodyFit,
}

impl CarType {
    /// Metres between the axles.
    pub fn wheelbase(&self) -> f64 {
        self.axles[0].x - self.axles[1].x
    }

    /// The load on each wheel of an axle at rest, N.
    pub fn static_load(&self, axle: usize) -> f64 {
        let l = self.wheelbase();
        let share = if axle == 0 { -self.axles[1].x / l } else { self.axles[0].x / l };
        self.mass * crate::math::GRAVITY * share / 2.0
    }

    /// Where wheel `i` (front left, front right, rear left, rear right) stands at rest, body axes.
    pub fn hub(&self, i: usize) -> V3 {
        let axle = &self.axles[i / 2];
        let side = if i.is_multiple_of(2) { -0.5 } else { 0.5 };
        V3::new(axle.x, side * axle.track, self.hub_z[i / 2])
    }

    /// The overall ratio from the engine to the driven wheels in a gear (negative in reverse, 0 in neutral).
    pub fn ratio(&self, gear: i32) -> f64 {
        let g = &self.gearbox;
        match gear {
            0 => 0.0,
            -1 => -g.reverse * g.final_drive,
            k => g.ratios.get(k as usize - 1).copied().unwrap_or(0.0) * g.final_drive,
        }
    }

    /// The most power it gives at the wheels, W.
    pub fn power(&self) -> f64 {
        match &self.electric {
            Some(e) => {
                let motors: f64 = e.motors.iter().map(|m| (0..200).map(|k| interpolate(&m.torque, k as f64 * 100.0) * k as f64 * 100.0 * std::f64::consts::TAU / 60.0).fold(0.0, f64::max)).sum();
                motors.min(e.battery_power) * e.efficiency
            }
            None => (0..100).map(|k| self.engine.power(k as f64 * 100.0)).fold(0.0, f64::max) * self.gearbox.efficiency,
        }
    }

    /// The most speed the engine reaches in a gear, m/s.
    pub fn top_of_gear(&self, gear: i32) -> f64 {
        let r = self.ratio(gear).abs();
        if r == 0.0 {
            return 0.0;
        }
        if let Some(e) = &self.electric {
            return e.top_speed_kmh / 3.6;
        }
        self.engine.limiter_rpm * std::f64::consts::TAU / 60.0 / r * self.axles[1].tyre.radius
    }
}

/// Reads a type of car from the text of its file; `file` names it in the errors.
pub fn load(text: &str, file: &str) -> Result<CarType, FileError> {
    let error = |message: String| FileError { file: file.to_string(), message };
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let p: Parsed = options.from_str(text).map_err(|e| error(e.to_string()))?;
    if p.front.x <= 0.0 || p.rear.x >= 0.0 {
        return Err(error("front.x must be ahead of the centre of gravity (positive), rear.x behind it".to_string()));
    }
    let rising = |t: &[(f64, f64)]| t.len() >= 2 && t.windows(2).all(|w| w[1].0 > w[0].0);
    let (engine, gearbox, differential, drive) = match (&p.electric, p.engine, p.gearbox, p.differential, p.drive) {
        (Some(e), None, None, None, None) => {
            let axles: Vec<usize> = e.motors.iter().map(MotorFit::axle).collect();
            if e.motors.is_empty() || e.motors.iter().any(|m| m.axle == Drive::All) || (axles.len() == 2 && axles[0] == axles[1]) || axles.len() > 2 {
                return Err(error("electric.motors: one or two, each on its own axle, Front or Rear".to_string()));
            }
            if e.motors.iter().any(|m| !rising(&m.torque)) {
                return Err(error("electric.motors' torque: two points at least, by rising rpm".to_string()));
            }
            // The motor driving the rear (or the only one) stands for the engine.
            let first = e.motors.iter().find(|m| m.axle == Drive::Rear).unwrap_or(&e.motors[0]);
            let top = e.top_speed_kmh / 3.6 / p.rear.tyre.radius * first.ratio * 60.0 / std::f64::consts::TAU;
            let engine = EngineFit { torque: first.torque.clone(), drag: (0.0, 0.0), inertia: first.inertia, idle_rpm: 0.0, limiter_rpm: top, redline_rpm: top, cylinders: 0 };
            let gearbox = GearboxFit { ratios: vec![1.0], reverse: 1.0, final_drive: first.ratio, efficiency: e.efficiency, shift_time: 0.2, clutch_torque: f64::INFINITY, auto_up_rpm: top, auto_down_rpm: 0.0 };
            let drive = if e.motors.len() == 2 { Drive::All } else { first.axle };
            (engine, gearbox, Differential::Open, drive)
        }
        (None, Some(engine), Some(gearbox), Some(differential), Some(drive)) => {
            if gearbox.ratios.is_empty() || gearbox.ratios.windows(2).any(|w| w[1] >= w[0]) {
                return Err(error("gearbox.ratios: one at least, each lower than the one before".to_string()));
            }
            if !rising(&engine.torque) {
                return Err(error("engine.torque: two points at least, by rising rpm".to_string()));
            }
            if drive == Drive::All {
                return Err(error("drive: All only for an electric car with a motor on each axle".to_string()));
            }
            (engine, gearbox, differential, drive)
        }
        _ => return Err(error("either engine, gearbox, differential and drive, or electric".to_string())),
    };
    if !(0.0..=1.0).contains(&p.brakes.front_share) {
        return Err(error("brakes.front_share: 0 to 1".to_string()));
    }
    let mut car = CarType {
        key: p.key,
        name: p.name,
        aliases: p.aliases,
        visual: p.visual,
        mass: p.mass,
        inertia: V3::new(p.inertia.0, p.inertia.1, p.inertia.2),
        cg_height: p.cg_height,
        axles: [p.front, p.rear],
        steering: p.steering,
        engine,
        gearbox,
        differential,
        drive,
        electric: p.electric,
        brakes: p.brakes,
        aero: p.aero,
        solid: p.body.solid.iter().map(|&(a, b)| (V3::new(a.0, a.1, a.2), V3::new(b.0, b.1, b.2))).collect(),
        body: p.body,
        hub_z: [0.0; 2],
    };
    // The hubs stand as high as the tyres, squashed by their share of the weight.
    for a in 0..2 {
        let tyre = &car.axles[a].tyre;
        let squash = car.static_load(a) / tyre.vertical_stiffness;
        car.hub_z[a] = car.cg_height - (tyre.radius - squash);
    }
    Ok(car)
}

/// Linear interpolation in a table of (x, y), held at its ends.
pub fn interpolate(table: &[(f64, f64)], x: f64) -> f64 {
    if x <= table[0].0 {
        return table[0].1;
    }
    for w in table.windows(2) {
        if x <= w[1].0 {
            let t = (x - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    table[table.len() - 1].1
}

static FLEET: OnceLock<Vec<CarType>> = OnceLock::new();

/// The built-in cars with `files` (path, text) in place of the built-in one of the same key, or
/// after them; what does not read is left out and said in the errors.
pub fn fleet_with(files: &[(String, String)]) -> (Vec<CarType>, Vec<FileError>) {
    let mut all: Vec<CarType> = BUILT_IN.iter().map(|(path, text)| load(text, path).expect("a built-in car reads")).collect();
    let mut errors = Vec::new();
    for (path, text) in files {
        match load(text, path) {
            Ok(c) => match all.iter_mut().find(|o| o.key == c.key) {
                Some(slot) => *slot = c,
                None => all.push(c),
            },
            Err(e) => errors.push(e),
        }
    }
    (all, errors)
}

/// Makes these the fleet, once, before anything asks for it; false if it was already set.
pub fn set_fleet(all: Vec<CarType>) -> bool {
    FLEET.set(all).is_ok()
}

/// The cars: those set, or the built-in ones.
pub fn fleet() -> &'static [CarType] {
    FLEET.get_or_init(|| fleet_with(&[]).0)
}
