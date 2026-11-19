//! A multirotor: a rigid frame lifted by fixed-pitch propellers on electric motors, flown by its
//! flight controller (`controller`), the second kind of vehicle (`vehicle::Dynamics::Multirotor`,
//! `docs/ROADMAP.md` seam 4). Its type is data (`MultirotorType`, read from
//! `assets/aircraft/<key>/multirotor.ron` by `file`); the 5-inch FPV quad is the built-in one.
//!
//! What it models, each step of `World::DT`:
//! - **Motors and battery.** Each motor is a DC machine (its `kv`, winding resistance, no-load
//!   current) driven by its ESC at a duty from the flight controller off a battery that sags under
//!   load (its open-circuit voltage from the charge used, its internal resistance). The rotor
//!   (motor bell and propeller, `rotor_inertia`) speeds up under the motor's torque against the
//!   propeller's drag torque `torque_coefficient · ω²`: the motor's time constant (some 25 ms)
//!   comes out of that, as does the hover throttle and how it creeps up as the battery runs down.
//! - **Propellers.** Thrust `thrust_coefficient · ω²` along the motor's axis, scaled by the air's
//!   density, falling linearly with the air coming through the disc to nothing a little past the
//!   pitch speed (`1.1 · pitch · ω / 2π`) and rising a little in a descent; more near the ground
//!   (Cheeseman and Bennett's ground effect, `1 / (1 − (R / 4h)²)`); the edgewise drag of a disc
//!   moving through the air sideways (`edgewise_drag · ω · v`); the reaction of each rotor's drag
//!   torque and of its speeding up on the frame (what yaws it), and the rotors' gyroscopic moment.
//!   Sinking into its own wash (the vortex ring state, "propwash" to FPV pilots: falling faster
//!   than its rotors push the air down, and the more so the less they push) costs thrust and makes
//!   each rotor's come and go in the turbulent air, a seeded noise per rotor (so still
//!   deterministic); with the motors at idle it wobbles hard. Forward speed leaves the wash behind,
//!   throttle punches out of it (`WashFit`).
//! - **Frame.** Drag of its frontal, side and top areas, a little rotational damping, contacts at
//!   points of it (feet, arms, camera, battery) with springs, dampers and friction against the
//!   ground; landing on water it sinks. Struck harder than `crash_speed` (the ground, a tree, a
//!   building) it crashes: the flight controller disarms, the props stop, and what is left tumbles
//!   and rests as a body. It leaves no `Wreck`: it is its own.
//! - **Against another multirotor** (`collide_multirotors`). A propeller's disc that meets the
//!   other's frame or propellers while it turns breaks (`PropStrike`): both blades shatter, its
//!   thrust is gone and it tumbles down on the others; a slow touch only chips a tip, and it flies
//!   on with a little less thrust on that rotor, shaking. Frames that touch knock each other apart
//!   once, along the line between them, with the impulse that keeps their momentum (a crash if
//!   harder than `crash_speed`).
//!
//! - **Tilting nacelles, wings and a tail** (`Tilt`, a type that has them: a tilt-rotor). The motors
//!   either side of the frame stand on a nacelle each that turns about a line across the frame,
//!   from rotors up to rotors ahead, as fast as its actuator goes; each rotor's thrust, drag
//!   torque and gyroscopic moment are along its nacelle's axis, where its hub then is. Wings, a
//!   tailplane and a fin (`Surface`) lift and drag by their own angle to the air, attached up to
//!   their stall and as flat plates past it, so they are sound at any angle (hovering, going
//!   backwards, falling flat); the tailplane's elevator adds to its angle. The rotors' cyclic
//!   pitch gives a moment at each hub, nose up or down: what pitches it when it has no speed for
//!   the elevator. The flight controller flies it with what it has at each tilt (`controller`).
//! - **Turbines** (`Turbine`, a type that has them in place of electric motors and a battery).
//!   All the rotors turn on one shaft, at the speed the engines' governor holds, and each pushes
//!   by its blades' pitch (the collective): thrust in proportion to the pitch less the angle the
//!   air comes through the disc at, its own induced velocity with it (which follows the thrust
//!   a tenth of a second behind: dynamic inflow). What turning them takes is momentum theory's: the induced
//!   power of the thrust (Glauert's induced velocity, in the hover, climbing or moving across),
//!   the climb's, and the blades' profile power. The engines give what the governor asks of
//!   their gas generators, which take time to follow, up to what the air's density leaves them,
//!   through a freewheel: more pitch than they can turn slows the rotors down, and with the
//!   engines out the air coming up through the discs turns them (autorotation). They burn fuel
//!   by the power they give.
//!
//! The wind is the vehicle's (`Environment::air_velocity`); the integration is the shared fourth-order
//! Runge-Kutta of `body::Rates`, with the rotors' speeds held over the step and stepped themselves
//! in four sub-steps before it.

pub mod controller;
pub mod file;

use serde::Deserialize;

use crate::aircraft::{Environment, GroundKind};
use crate::atmosphere::SEA_LEVEL_DENSITY;
use crate::body::{Body, Rates};
use crate::math::{act, clamp, interpolate, inverse, normalize, quaternion, smoothstep, Quat, Rng, GRAVITY, V3};
use crate::scenery::{Obstacle, ObstacleKind, Shape};
use crate::vehicle::{ContactPoint, On};
pub use controller::Controller;

/// The most motors a multirotor has.
pub const MOST_MOTORS: usize = 8;
/// The most points a multirotor touches the ground with (`MultirotorType::contacts`).
pub const MOST_CONTACTS: usize = 32;

/// Which type of multirotor: the key of its file (`assets/aircraft/<key>/multirotor.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MultirotorKind(&'static str);

impl MultirotorKind {
    /// A 5-inch freestyle FPV quad.
    pub const FPV5: MultirotorKind = MultirotorKind("fpv5");
    /// A 5-inch racing quad on 6S (`assets/aircraft/race5`): what the forest course is raced in.
    pub const RACE5: MultirotorKind = MultirotorKind("race5");

    /// Every kind there is (`file::fleet`), in its order.
    pub fn all() -> impl Iterator<Item = MultirotorKind> {
        file::fleet().iter().map(|t| t.kind)
    }

    /// Name for the player.
    pub fn name(self) -> &'static str {
        self.spec().name
    }

    /// The key of its file.
    pub fn key(self) -> &'static str {
        self.0
    }

    /// The kind a word names, by its key or one of its aliases (`fpv`, `quad`, `drone`...).
    pub fn parse(word: &str) -> Option<MultirotorKind> {
        let w = word.to_ascii_lowercase().replace([' ', '-', '_'], "");
        let plain = |name: &str| name.replace('_', "") == w;
        file::fleet().iter().find(|t| t.words().any(plain)).map(|t| t.kind)
    }

    /// Its type. Panics for a kind not among `file::fleet`.
    pub fn spec(self) -> &'static MultirotorType {
        file::fleet().iter().find(|t| t.kind == self).unwrap_or_else(|| panic!("the multirotor {} is not in the fleet", self.0))
    }
}

/// Which way a propeller turns, seen from above.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Spin {
    Clockwise,
    CounterClockwise,
}

impl Spin {
    /// Its turn about the body's z axis (down): clockwise from above is positive.
    pub fn sign(self) -> f64 {
        match self {
            Spin::Clockwise => 1.0,
            Spin::CounterClockwise => -1.0,
        }
    }
}

/// A motor: where its propeller's hub is (body axes from the centre of gravity) and its turn.
/// It pushes up along the frame's −z; on a nacelle (`Tilt`: 0 the left one, 1 the right), along
/// the nacelle's axis, and its hub is given with the nacelle upright.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motor {
    pub position: V3,
    pub spin: Spin,
    pub nacelle: Option<usize>,
}

/// The nacelles a multirotor has: the left and the right.
pub const NACELLES: usize = 2;

/// A wing, a tailplane or a fin: a flat lifting surface, by its numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    /// Its aerodynamic centre, body axes.
    pub position: V3,
    /// m², and its span squared over its area.
    pub area: f64,
    pub aspect: f64,
    /// A fin (it lifts sideways) rather than a wing or a tailplane (they lift up).
    pub upright: bool,
    /// How it is set on the frame, radians, leading edge up.
    pub incidence: f64,
    /// The angle to the air it lifts up to, radians, and its drag coefficient edge on.
    pub stall: f64,
    pub drag: f64,
    /// Its elevator, if it has one: how much of its deflection adds to the surface's angle (a
    /// share: the flap effectiveness of its chord), and how far it goes either way, radians.
    pub elevator: Option<(f64, f64)>,
}

impl Surface {
    /// Its force (body axes, N) moving through the air at `v` (body axes, m/s) of density `rho`,
    /// its elevator `deflection` down (radians).
    ///
    /// Its angle to the air is taken in its own plane of lift (x and z; a fin's, x and y). Up to
    /// the stall it lifts as a finite wing (`2π·A/(A + 2)` per radian, Helmbold's slope for a low
    /// aspect ratio near enough) with the drag that lift induces (Oswald's factor 0.8); past it,
    /// over 8°, it becomes a flat plate (`C_L = sin 2α`, `C_D = 2 sin²α`: Hoerner's plate normal
    /// to the flow has a drag coefficient near 2), as it is from behind.
    pub fn force(&self, v: V3, rho: f64, deflection: f64) -> V3 {
        let (u, w) = (v.x, if self.upright { v.y } else { v.z });
        let speed2 = u * u + w * w;
        if speed2 < 1e-6 {
            return V3::ZERO;
        }
        let effect = self.elevator.map_or(0.0, |(effect, _)| effect);
        let alpha = crate::math::remainder(w.atan2(u) + self.incidence + effect * deflection, std::f64::consts::TAU);
        let slope = std::f64::consts::TAU * self.aspect / (self.aspect + 2.0);
        let attached = slope * alpha;
        let plate = if alpha.cos() < 0.0 { 1.0 } else { smoothstep(self.stall, self.stall + 8.0 * crate::math::DEG, alpha.abs()) };
        let lift = attached * (1.0 - plate) + (2.0 * alpha).sin() * plate;
        let induced = attached * attached / (std::f64::consts::PI * 0.8 * self.aspect);
        let drag = self.drag + induced * (1.0 - plate) + 2.0 * alpha.sin().powi(2) * plate;
        // Lift across the air's way, drag along it, in the plane of lift.
        let k = 0.5 * rho * self.area * speed2.sqrt();
        let (along, across) = (k * (lift * w - drag * u), k * (-lift * u - drag * w));
        if self.upright {
            V3::new(along, across, 0.0)
        } else {
            V3::new(along, 0.0, across)
        }
    }
}

/// What makes a multirotor a tilt-rotor: nacelles that turn, wings and a tail, and how its flight
/// controller uses them (`controller`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tilt {
    /// The line the nacelles turn about runs across the frame through this point (body axes).
    pub pivot: V3,
    /// How far forward the pilot's lever tilts them, radians from upright (rotors up).
    pub forward: f64,
    /// How far a nacelle goes either side of that for control, radians: back of upright and
    /// forward of `forward`.
    pub margin: f64,
    /// How far the pitch stick tilts both nacelles from where the lever has them, radians at its
    /// end (forward stick, forward), and how far back of upright that takes them at most. With
    /// it the stick does not pitch the frame: it stays at the attitude the pilot trims
    /// (`MultirotorControls::trim`), and goes forward or back by its nacelles. Nothing: the
    /// stick pitches the frame.
    pub stick_tilt: f64,
    pub back: f64,
    /// How fast its actuator turns a nacelle, rad/s, and how fast the flight controller lets the
    /// lever take both of them from one tilt to another (the conversion), rad/s.
    pub rate: f64,
    pub conversion_rate: f64,
    /// How far the flight controller tilts one nacelle against the other, for yaw (hovering) and
    /// roll (flying forward), radians.
    pub split_authority: f64,
    /// The rotors' cyclic pitch: the moment each rotor's hub gives at full cyclic, nose up, per
    /// newton of its thrust (m: how far its thrust moves off its hub).
    pub cyclic_arm: f64,
    /// With no stability help (`Stability::Off`): the angular accelerations full sticks ask
    /// for (roll, pitch, yaw), rad/s².
    pub direct: V3,
    /// What `Stability::Hold` holds, if it has it (without, that help is `Level`'s).
    pub hold: Option<HoldFit>,
    /// The speed (m/s) from which it turns with its bank as an aeroplane does, and that at
    /// which it does so fully.
    pub turn_from: f64,
    pub turn_full: f64,
    pub surfaces: &'static [Surface],
}

impl Tilt {
    /// The sine and cosine of a nacelle's angle.
    fn turn(angle: f64) -> (f64, f64) {
        angle.sin_cos()
    }
}

/// Turbine engines turning every rotor on one shaft, in place of the motors and the battery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turbine {
    /// How many engines, and what each gives at sea level, W.
    pub engines: u32,
    pub power: f64,
    /// The speed the governor holds the rotors at, rad/s.
    pub rotor_speed: f64,
    /// The blades' pitch at the bottom and the top of the collective lever, in the unit that
    /// gives the thrust `thrust_coefficient · ω²` with no air through the disc.
    pub pitch: (f64, f64),
    /// How much pitch the air coming through a disc takes: the thrust's share of
    /// `thrust_coefficient · ω²` is the pitch less the speed through the disc (the climb along
    /// its axis and the induced velocity) over the rotor's speed times this length, m.
    pub inflow_length: f64,
    /// Induced power over momentum theory's (tip losses, an uneven wash), and the blades' profile
    /// power per rotor at the governed speed at sea level, W.
    pub induced_factor: f64,
    pub profile_power: f64,
    /// The shaft's inertia with everything on it, kg m².
    pub inertia: f64,
    /// How long the gas generators take to follow the governor, s, and to light and reach idle.
    pub spool_time: f64,
    pub start_time: f64,
    /// Fuel aboard, kg; burnt per joule given, kg, and per second by each engine just running.
    pub fuel: f64,
    pub fuel_per_joule: f64,
    pub idle_flow: f64,
}

/// The engines of a turbine multirotor as the last step left them.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Engines {
    /// Whether they are lit.
    pub running: bool,
    /// The gas generators, 0 to 1 of what they can give.
    pub gas: f64,
    /// How far they are into lighting, 0 to 1 (lit), and the speed their governor is bringing
    /// the rotors to, rad/s: up to the governed one as they run up.
    lit: f64,
    target: f64,
    /// The governor's integral.
    integral: f64,
    /// What they give the shaft, W, and what the rotors took of it in the last step, W.
    pub power: f64,
    pub load: f64,
    /// Fuel left, kg, and its flow, kg/s.
    pub fuel: f64,
    pub flow: f64,
}

/// How much the flight controller helps (`controller`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stability {
    /// The sticks ask for a bank and a pitch, and it levels itself when they are let go.
    #[default]
    Level,
    /// The sticks ask for rates of turn; let go, it stays as it is.
    Rates,
    /// Nothing: the sticks move the rotors.
    Off,
    /// A turbine tilt-rotor's: it holds itself still. The collective lever asks for a rate of
    /// climb (its middle, none: it holds its height) and, with the nacelles' lever upright, the
    /// stick for a speed over the ground, forward and sideways (let go, it stops and stays, the
    /// wind held off). With the nacelles' lever forward the stick is `Level`'s.
    Hold,
}

/// What `Stability::Hold` holds and how hard (`Tilt::hold`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoldFit {
    /// The rates of climb the collective lever asks for at its top and its bottom, m/s (up
    /// positive), and the share of its travel round the middle that asks for none.
    pub climb: (f64, f64),
    pub dead_band: f64,
    /// The speeds over the ground full stick asks for, forward and sideways, m/s.
    pub speed: (f64, f64),
    /// How hard it goes after a speed it has not: the acceleration per m/s of error, 1/s, for
    /// the climb and for the speed over the ground; and the most it leans sideways for it, radians.
    pub climb_gain: f64,
    pub speed_gain: f64,
    pub bank: f64,
}

/// A vector of a rotor's own axes (z down its axis, away from its thrust) in body axes, the
/// rotor's nacelle at an angle of sine `s` and cosine `c` (upright, 0 and 1: itself).
fn from_rotor(v: V3, (s, c): (f64, f64)) -> V3 {
    V3::new(v.x * c - v.z * s, v.y, v.x * s + v.z * c)
}

/// A vector of body axes in a rotor's own.
fn to_rotor(v: V3, (s, c): (f64, f64)) -> V3 {
    V3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

/// The FPV camera: where it is (body axes), tilted up by `uptilt` (radians), and its vertical
/// field of view... its diagonal one as lenses are sold (radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub position: V3,
    pub uptilt: f64,
    pub fov: f64,
}

/// How the game draws a type: its model's name (a folder of `assets/aircraft`), and the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MultirotorVisual {
    pub model: &'static str,
    pub camera: Camera,
    /// Its cockpit, if it has one: a folder of `assets/aircraft` as an aircraft's.
    pub cockpit: Option<&'static str>,
}

/// The stick-to-rate curve of a flight controller, per axis (roll, pitch, yaw), in the form the
/// FPV world calls "Betaflight rates": `rc_rate` (200°/s per unit at full stick, steeper past 2),
/// `super_rate` (how much the rate rises towards the ends of the stick) and `expo` (how soft the
/// middle is).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickRates {
    pub rc_rate: V3,
    pub super_rate: V3,
    pub expo: V3,
}

impl StickRates {
    /// The rate asked for on `axis` (0 roll, 1 pitch, 2 yaw) at `stick` (−1...1), rad/s. From the
    /// published formula: the stick bent by the expo (`s·|s|³·e + s·(1 − e)`), times 200°/s and the
    /// RC rate (beyond 2, 14.54 times steeper), over `1 − |s|·super`.
    pub fn rate(&self, axis: usize, stick: f64) -> f64 {
        let s = clamp(stick, -1.0, 1.0);
        let (rc, sup, expo) = (self.rc_rate[axis], self.super_rate[axis], self.expo[axis]);
        let s_expo = s * s.abs().powi(3) * expo + s * (1.0 - expo);
        let rc = if rc > 2.0 { rc + 14.54 * (rc - 2.0) } else { rc };
        let rate = 200.0 * rc * s_expo / clamp(1.0 - s.abs() * sup, 0.01, 1.0);
        rate * crate::math::DEG
    }
}

/// The flight controller's tune (`controller`): rates, the rate loop's gains per axis (roll,
/// pitch, yaw) as angular accelerations (P in 1/s per rad/s of error, I in 1/s², D in s,
/// feed-forward a share of the change of the rate asked for), and its self-levelling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerFit {
    pub rates: StickRates,
    pub p: V3,
    pub i: V3,
    pub d: V3,
    pub feed_forward: V3,
    /// The most the integral holds, rad/s².
    pub i_limit: f64,
    /// Self-levelling: the bank and pitch at full stick, radians, and how fast it levels (1/s).
    pub angle_max: f64,
    pub angle_gain: f64,
}

/// What its own wash does to it (the vortex ring state, "propwash"). Each rotor's depth in it,
/// 0 to 1: how fast it sinks along its axis over the speed it pushes the air down at (the induced
/// velocity of its thrust now, `√(T / 2ρA)`), from `onset` to `full` times as fast; times how far
/// it is from moving out of it across, by the angle its wake trails at (the speed across over the
/// sink and the induced velocity, from `clear_from`, all of it, to `clear_to`, none). Deep in it a
/// rotor loses `thrust_loss` of its thrust on the mean, and its thrust shakes by `fluctuation` of
/// its own and `buffet` of the descent's dynamic pressure on its disc (`½ρAV²`), a noise of
/// standard deviation one per rotor that changes over `correlation_time` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WashFit {
    pub onset: f64,
    pub full: f64,
    pub clear_from: f64,
    pub clear_to: f64,
    pub thrust_loss: f64,
    pub fluctuation: f64,
    pub buffet: f64,
    pub correlation_time: f64,
}

/// What a propeller takes when its disc meets something solid while it turns (another quad's
/// frame or propellers; `collide_multirotors`). Touched slower than `graze_speed` (m/s), a blade
/// loses its tip, as deep as the thing reached into the disc; faster, or once a blade has lost
/// more than `chip_depth` (m) in all, both blades shatter to `stub` (a share of the radius). Its
/// thrust and drag torque go as each blade's radius cubed and to the fourth (a blade element's lift
/// grows with the square of its speed, so with r², over dr); uneven blades shake the rotor, its
/// thrust coming and going by `shake` times the difference of their swept areas (the shares of the
/// radius squared), on the noise of its own wash. The disc is `thickness` (m) thick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropStrike {
    pub thickness: f64,
    pub graze_speed: f64,
    pub chip_depth: f64,
    pub stub: f64,
    pub shake: f64,
}

/// A type of multirotor: everything that makes one differ from another, as its file gives it.
/// Positions are body axes (x forward, y right, z down) from the centre of gravity.
#[derive(Clone, Debug, PartialEq)]
pub struct MultirotorType {
    pub kind: MultirotorKind,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub visual: MultirotorVisual,
    /// All up, battery included, kg; principal moments of inertia, kg m².
    pub mass: f64,
    pub inertia: V3,
    /// Drag coefficient times area along each body axis, m².
    pub drag_area: V3,
    /// N m per rad/s.
    pub rotational_damping: f64,
    /// The points it touches the ground and things with, and their springs (N/m each), dampers
    /// (N s/m) and friction; struck harder than `crash_speed` (m/s) it breaks.
    pub contacts: &'static [V3],
    pub contact_spring: f64,
    pub contact_damping: f64,
    pub friction: f64,
    pub crash_speed: f64,
    /// The boxes of it another vehicle hits (lower and upper corners).
    pub solid: &'static [(V3, V3)],
    /// The frame as balls (centre, radius) another multirotor's frame and propellers strike, and
    /// how much of the speed they meet at it gives back (0 a dead knock, 1 a perfect bounce).
    pub parts: &'static [(V3, f64)],
    pub restitution: f64,
    pub motors: &'static [Motor],
    /// rad/s per volt (the motor's torque constant is its inverse, N m/A).
    pub kv: f64,
    /// Ω, A, kg m² (bell and propeller), A, share of full throttle the ESC idles at while armed.
    pub winding_resistance: f64,
    pub no_load_current: f64,
    pub rotor_inertia: f64,
    pub max_current: f64,
    pub idle: f64,
    /// m; thrust and drag torque per (rad/s)² at sea level; edgewise drag, N per rad/s per m/s.
    pub prop_diameter: f64,
    pub prop_pitch: f64,
    pub thrust_coefficient: f64,
    pub torque_coefficient: f64,
    pub edgewise_drag: f64,
    pub strike: PropStrike,
    pub wash: WashFit,
    /// Cells in series, the charge (coulombs), the pack's resistance (Ω) and the open-circuit
    /// voltage of a cell against the share of the charge used.
    pub cells: u32,
    pub capacity: f64,
    pub internal_resistance: f64,
    pub cell_voltage: &'static [(f64, f64)],
    pub controller: ControllerFit,
    /// How high above the ground a start in the air hovers, m.
    pub air_height: f64,
    /// Per motor, its thrust per newton of collective thrust and per N m of roll, pitch and yaw
    /// moment (`mixer_matrix`). Unused (zero) with nacelles: `controller` says how it is flown then.
    pub mixer: [[f64; 4]; MOST_MOTORS],
    /// Its nacelles, wings and tail, if it is a tilt-rotor.
    pub tilt: Option<Tilt>,
    /// Its turbines, if it has them in place of its motors and battery (whose numbers are unused then).
    pub turbine: Option<Turbine>,
    /// The help its flight controller starts with.
    pub stability: Stability,
}

impl MultirotorType {
    /// The fleet's type of a kind (`MultirotorKind::spec`).
    pub fn of(kind: MultirotorKind) -> MultirotorType {
        kind.spec().clone()
    }

    /// Propeller radius, m.
    pub fn prop_radius(&self) -> f64 {
        self.prop_diameter / 2.0
    }

    /// The battery's voltage with nothing drawn, at `used` (share of the charge, 0 full, 1 empty).
    pub fn open_voltage(&self, used: f64) -> f64 {
        let (xs, ys): (Vec<f64>, Vec<f64>) = self.cell_voltage.iter().copied().unzip();
        interpolate(&xs, &ys, clamp(used, 0.0, 1.0)) * self.cells as f64
    }

    /// The speed a rotor settles at, rad/s, at a `duty` of a battery giving `volts`, in still air
    /// at sea level: where the motor's torque, `(duty·V − ω/kv) / (R·kv)`, equals the propeller's
    /// and the motor's friction. The flight controller's model of its motors.
    pub fn steady_speed(&self, duty: f64, volts: f64) -> f64 {
        let r = self.winding_resistance;
        let a = r * self.torque_coefficient * self.kv;
        let b = 1.0 / self.kv;
        let c = r * self.no_load_current - duty * volts;
        if c >= 0.0 {
            return 0.0;
        }
        (-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)
    }

    /// The thrust of one rotor at a `duty`, N, as `steady_speed`.
    pub fn steady_thrust(&self, duty: f64, volts: f64) -> f64 {
        let w = self.steady_speed(duty, volts);
        self.thrust_coefficient * w * w
    }

    /// The duty that gives one rotor `thrust` (N) off `volts`, by the same model; not clamped.
    pub fn duty_for(&self, thrust: f64, volts: f64) -> f64 {
        let w = (thrust.max(0.0) / self.thrust_coefficient).sqrt();
        let current = self.torque_coefficient * w * w * self.kv + self.no_load_current;
        (w / self.kv + self.winding_resistance * current) / volts.max(1.0)
    }

    /// The throttle stick (0...1) it hovers at in still air of density `rho`, its battery full and
    /// sagging under the hover's current.
    pub fn hover_throttle(&self, rho: f64) -> f64 {
        let n = self.motors.len() as f64;
        if let Some(t) = &self.turbine {
            // The collective whose pitch carries its weight at the governed speed.
            let pitch = self.static_pitch(t, self.mass * GRAVITY / n, rho);
            return clamp((pitch - t.pitch.0) / (t.pitch.1 - t.pitch.0), 0.0, 1.0);
        }
        let thrust = self.mass * GRAVITY / n * SEA_LEVEL_DENSITY / rho;
        // The sag under the hover's current, found by going round twice.
        let mut volts = self.open_voltage(0.0);
        let mut duty = 0.0;
        for _ in 0..3 {
            duty = self.duty_for(thrust, volts);
            let w = (thrust / self.thrust_coefficient).sqrt();
            let current = self.torque_coefficient * w * w * self.kv + self.no_load_current;
            volts = self.open_voltage(0.0) - self.internal_resistance * (n * duty * current + ELECTRONICS);
        }
        clamp((duty - self.idle) / (1.0 - self.idle), 0.0, 1.0)
    }

    /// Thrust over weight at full throttle, battery full, still air at sea level.
    pub fn thrust_to_weight(&self) -> f64 {
        let n = self.motors.len() as f64;
        if let Some(t) = &self.turbine {
            // What its engines' power lifts in the hover at sea level: the thrust whose induced
            // and profile power is all they give, and no more than its blades' at full pitch.
            let area = std::f64::consts::PI * self.prop_radius().powi(2);
            let each = (t.engines as f64 * t.power / n - t.profile_power).max(0.0);
            let thrust = (each / t.induced_factor * (2.0 * SEA_LEVEL_DENSITY * area).sqrt()).powf(2.0 / 3.0);
            let blades = self.static_thrust(t, t.pitch.1, SEA_LEVEL_DENSITY);
            return n * thrust.min(blades) / (self.mass * GRAVITY);
        }
        let mut volts = self.open_voltage(0.0);
        let mut thrust = 0.0;
        for _ in 0..4 {
            let w = self.steady_speed(1.0, volts);
            thrust = self.thrust_coefficient * w * w;
            let current = self.torque_coefficient * w * w * self.kv + self.no_load_current;
            volts = self.open_voltage(0.0) - self.internal_resistance * (n * current + ELECTRONICS);
        }
        n * thrust / (self.mass * GRAVITY)
    }

    /// Turbines: a rotor's thrust at a blades' `pitch` in the hover, at the governed speed in air
    /// of density `rho`: where `T = k ω² (pitch − v / ω L)` meets its own induced velocity
    /// `v = √(T / 2ρA)`, a quadratic in `√T`.
    pub fn static_thrust(&self, t: &Turbine, pitch: f64, rho: f64) -> f64 {
        let full = self.thrust_coefficient * rho / SEA_LEVEL_DENSITY * t.rotor_speed * t.rotor_speed;
        let area = std::f64::consts::PI * self.prop_radius().powi(2);
        let b = full / (t.rotor_speed * t.inflow_length * (2.0 * rho * area).sqrt());
        let c = full * pitch;
        if c <= 0.0 {
            return 0.0;
        }
        ((-b + (b * b + 4.0 * c).sqrt()) / 2.0).powi(2)
    }

    /// The pitch that gives a rotor `thrust` in the hover (`static_thrust`'s inverse).
    pub fn static_pitch(&self, t: &Turbine, thrust: f64, rho: f64) -> f64 {
        let full = self.thrust_coefficient * rho / SEA_LEVEL_DENSITY * t.rotor_speed * t.rotor_speed;
        let area = std::f64::consts::PI * self.prop_radius().powi(2);
        let thrust = thrust.max(0.0);
        thrust / full + (thrust / (2.0 * rho * area)).sqrt() / (t.rotor_speed * t.inflow_length)
    }

    /// The least-squares split of a collective thrust and the three moments between the motors:
    /// the matrix `A` takes the motors' thrusts to (collective, roll, pitch, yaw) (a motor at `y`
    /// rolls by `−y·T`, at `x` pitches by `x·T`, and yaws the frame by `−spin·(kQ/kT)·T`); this is
    /// its pseudo-inverse `Aᵀ(AAᵀ)⁻¹`, per motor.
    pub(crate) fn mixer_matrix(&self) -> [[f64; 4]; MOST_MOTORS] {
        if self.tilt.is_some() {
            return [[0.0; 4]; MOST_MOTORS];
        }
        let ratio = self.torque_coefficient / self.thrust_coefficient;
        let rows = |m: &Motor| [1.0, -m.position.y, m.position.x, -m.spin.sign() * ratio];
        let mut aat = [[0.0; 4]; 4];
        for m in self.motors {
            let r = rows(m);
            for (j, row) in aat.iter_mut().enumerate() {
                for (k, x) in row.iter_mut().enumerate() {
                    *x += r[j] * r[k];
                }
            }
        }
        let inv = glam::DMat4::from_cols_array_2d(&aat).inverse();
        let inv = inv.to_cols_array_2d();
        let mut mixer = [[0.0; 4]; MOST_MOTORS];
        for (i, m) in self.motors.iter().enumerate() {
            let r = rows(m);
            for (k, out) in mixer[i].iter_mut().enumerate() {
                // (AAᵀ)⁻¹ is symmetric: column or row alike.
                *out = (0..4).map(|j| r[j] * inv[j][k]).sum();
            }
        }
        mixer
    }
}

/// What the flight controller, the camera and the video transmitter draw besides the motors, A.
const ELECTRONICS: f64 = 0.6;
/// Sub-steps of the motors' speeds within one step of the world.
const MOTOR_STEPS: usize = 4;
/// How long a rotor's induced velocity takes to follow its thrust, s (dynamic inflow: the air
/// above and below the disc has to be set moving; a tenth of a second for rotors of metres).
const INFLOW_TIME: f64 = 0.1;
/// Why a multirotor that broke a propeller against another came down.
const BROKEN_PROPELLER: &str = "Broke a propeller against another quad";
/// Twigs in a crown a disc meets, per metre it moves through it, a metre and more inside it
/// (estimated: the twigs of a crown stand some tens of centimetres apart, and a disc a quarter of
/// a metre wide sweeps through them); fewer towards its edge, none at it, over `TWIG_FRINGE`
/// metres (the crown's outline is the reach of its outermost leaves).
const TWIGS_PER_METRE: f64 = 1.5;
const TWIG_FRINGE: f64 = 1.0;
/// How far a twig reaches into a disc, m: under `PropStrike::chip_depth`, a chip when slow.
const TWIG_DEPTH: f64 = 0.004;
/// Why two multirotors that met harder than they take crashed.
const COLLIDED: &str = "Collided with another quad";
/// A rotor slower than this (rad/s, some 1000 rpm) is not cutting anything: its blades are struck
/// as the frame is, and nothing breaks.
const CUTTING: f64 = 100.0;

/// The tilt beyond which it does not arm, radians.
pub const ARMING_ANGLE: f64 = 25.0 * crate::math::DEG;

/// The sticks and switches of its radio: what a pilot sets (`vehicle::Controls::Multirotor`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MultirotorControls {
    /// 0 (stick down) to 1 (up).
    pub throttle: f64,
    /// −1 to 1: right, nose up (the stick pulled back), nose right.
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    /// The arm switch: it arms when it is on and the throttle low (`controller`).
    pub arm: bool,
    /// A tilt-rotor's nacelles' lever: 0 upright (rotors up) to 1 forward (`Tilt::forward`).
    pub tilt: f64,
    /// How much the flight controller helps.
    pub stability: Stability,
    /// The pitch a tilt-rotor whose stick tilts its nacelles holds, −1 to 1 of its controller's
    /// `angle_max`, nose up.
    pub trim: f64,
}

/// The battery: the charge used (coulombs), its voltage and the current drawn, as the last step left them.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Battery {
    pub used: f64,
    pub voltage: f64,
    pub current: f64,
}

/// What the last step measured.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MultirotorData {
    /// Speed through the air, m/s.
    pub airspeed: f64,
    pub on_ground: bool,
    /// What an accelerometer reads, body axes, m/s² (the force on it but gravity's, over its mass).
    pub specific: V3,
    /// Thrust of all the rotors, N.
    pub thrust: f64,
    /// Share of the thrust lost on the mean to its own wash (the vortex ring state), 0 to the
    /// type's `WashFit::thrust_loss`.
    pub wash: f64,
}

/// What its pilot observes of a multirotor besides what it observes of any vehicle
/// (`world::Observation`): plain numbers and strings.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MultirotorObservation {
    pub armed: bool,
    /// Why it does not arm while the switch is on ("THROTTLE", "ANGLE", "CRASH").
    pub arming_blocked: Option<String>,
    /// The sticks as they are.
    pub throttle: f64,
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    /// Each rotor's speed, rad/s (`motors` of them).
    pub motor_speed: [f64; MOST_MOTORS],
    pub motors: usize,
    /// Volts, amps, and the charge used, mAh.
    pub battery_voltage: f64,
    pub battery_current: f64,
    pub battery_used_mah: f64,
    /// Its barometer's height (m above sea level) and vertical speed (m/s, up).
    pub altitude: f64,
    pub vertical_speed: f64,
    pub airspeed: f64,
    /// A tilt-rotor's nacelles' lever, and where its nacelles are, radians forward of upright (left, right).
    pub tilt: f64,
    pub nacelles: [f64; NACELLES],
    /// Turbines: the rotors' speed over the governed one, the power the engines give over all
    /// they have at sea level (their torque at the governed speed), and the fuel left, kg.
    pub rotor_share: f64,
    pub torque: f64,
    pub fuel: f64,
}

/// A multirotor: its type, body, rotors, battery, flight controller and radio.
#[derive(Clone, Debug)]
pub struct Multirotor {
    pub p: MultirotorType,
    pub body: Body,
    pub controls: MultirotorControls,
    rotors: [f64; MOST_MOTORS],
    rotor_accel: [f64; MOST_MOTORS],
    inflow: [f64; MOST_MOTORS],
    battery: Battery,
    fc: Controller,
    data: MultirotorData,
    crashed: Option<&'static str>,
    sunk: bool,
    rng: Rng,
    /// The noise of each rotor's thrust in its own wash this step (standard deviation one).
    wash: [f64; MOST_MOTORS],
    /// The body before the last step: how its points came into the ground.
    previous: Body,
    /// Which contact points were in the ground after the last step.
    touching: u32,
    /// How far each propeller's two blades reach, a share of its radius (1 whole; `PropStrike`).
    blades: [[f64; 2]; MOST_MOTORS],
    /// Why it comes down once a propeller is broken: what broke it.
    broken_on: &'static str,
    /// The air's density over the sea level's, where the last step found it.
    air: f64,
    /// Where each nacelle is, radians forward of upright (left, right), the elevator, radians
    /// down, and the rotors' cyclic, −1 to 1 nose up.
    nacelles: [f64; NACELLES],
    elevator: f64,
    cyclic: f64,
    /// Turbines: the engines, and each rotor's blades' pitch (a share of full: `Turbine::pitch`).
    engines: Engines,
    pitch: [f64; MOST_MOTORS],
    /// The air each rotor pushes through itself, m/s: its induced velocity, followed over
    /// `INFLOW_TIME` (turbines).
    induced: [f64; MOST_MOTORS],
}

impl Multirotor {
    /// One of a kind of the fleet, at the origin, disarmed.
    pub fn of(kind: MultirotorKind) -> Multirotor {
        Multirotor::with_type(kind.spec().clone())
    }

    pub fn with_type(p: MultirotorType) -> Multirotor {
        let volts = p.open_voltage(0.0);
        let body = Body { pos: V3::ZERO, vel: V3::ZERO, q: Quat::IDENTITY, omega: V3::ZERO };
        Multirotor {
            p,
            body,
            controls: MultirotorControls::default(),
            rotors: [0.0; MOST_MOTORS],
            rotor_accel: [0.0; MOST_MOTORS],
            inflow: [1.0; MOST_MOTORS],
            battery: Battery { used: 0.0, voltage: volts, current: 0.0 },
            fc: Controller::default(),
            data: MultirotorData::default(),
            crashed: None,
            sunk: false,
            rng: Rng::new(7),
            wash: [0.0; MOST_MOTORS],
            previous: body,
            touching: 0,
            blades: [[1.0; 2]; MOST_MOTORS],
            broken_on: BROKEN_PROPELLER,
            air: 1.0,
            nacelles: [0.0; NACELLES],
            elevator: 0.0,
            cyclic: 0.0,
            engines: Engines::default(),
            pitch: [0.0; MOST_MOTORS],
            induced: [0.0; MOST_MOTORS],
        }
    }

    /// Seeds what is random of it (the shaking of its wash).
    pub(crate) fn seed(&mut self, seed: u64) {
        self.rng = Rng::new(seed ^ 0x6d75_6c74_6972_6f74);
    }

    pub fn data(&self) -> &MultirotorData {
        &self.data
    }

    pub fn battery(&self) -> &Battery {
        &self.battery
    }

    /// Each rotor's speed, rad/s.
    pub fn rotor_speeds(&self) -> &[f64] {
        &self.rotors[..self.p.motors.len()]
    }

    /// Where each nacelle is, radians forward of upright (left, right); upright without them.
    pub fn nacelles(&self) -> [f64; NACELLES] {
        self.nacelles
    }

    /// Its elevator, radians, trailing edge down.
    pub fn elevator(&self) -> f64 {
        self.elevator
    }

    /// Its rotors' cyclic, −1 to 1, nose up.
    pub fn cyclic(&self) -> f64 {
        self.cyclic
    }

    /// Its turbines as the last step left them (all zero without them).
    pub fn engines(&self) -> &Engines {
        &self.engines
    }

    /// The power its engines give over all they have at sea level: their torque, at the governed speed.
    pub fn torque(&self) -> f64 {
        self.p.turbine.as_ref().map_or(0.0, |t| self.engines.power / (t.engines as f64 * t.power))
    }

    /// The sine and cosine of the angle of motor `i`'s nacelle (0 and 1 off a nacelle).
    fn turn(&self, i: usize) -> (f64, f64) {
        match self.p.motors[i].nacelle {
            Some(k) => Tilt::turn(self.nacelles[k]),
            None => (0.0, 1.0),
        }
    }

    /// Where motor `i`'s hub is now (body axes), and its nacelle's turn (`turn`).
    fn hub(&self, i: usize) -> (V3, (f64, f64)) {
        let motor = &self.p.motors[i];
        match (motor.nacelle, &self.p.tilt) {
            (Some(_), Some(tilt)) => {
                let turn = self.turn(i);
                (tilt.pivot + from_rotor(motor.position - tilt.pivot, turn), turn)
            }
            _ => (motor.position, (0.0, 1.0)),
        }
    }

    /// The way motor `i` pushes, body axes.
    pub fn thrust_axis(&self, i: usize) -> V3 {
        from_rotor(V3::new(0.0, 0.0, -1.0), self.turn(i))
    }

    /// How far each propeller's two blades reach, a share of its radius: 1 whole, less chipped,
    /// `PropStrike::stub` broken.
    pub fn blades(&self) -> &[[f64; 2]] {
        &self.blades[..self.p.motors.len()]
    }

    /// Propeller `i` strikes something `depth` metres into its disc at `speed` m/s, as it would
    /// against another quad (`collide_multirotors`): chipped or broken (`PropStrike`).
    pub fn strike_propeller(&mut self, i: usize, depth: f64, speed: f64) {
        if i < self.p.motors.len() {
            self.cut(i, depth, speed);
        }
    }

    /// Whether a propeller has broken (struck another quad).
    pub fn propeller_broken(&self) -> bool {
        self.blades().iter().any(|b| b[0].max(b[1]) <= self.p.strike.stub)
    }

    /// What is left of propeller `i`: shares of its whole thrust, drag torque and rotor inertia
    /// (the bell's quarter stays), and how much it shakes (`PropStrike`).
    fn propeller(&self, i: usize) -> (f64, f64, f64, f64) {
        let [a, b] = self.blades[i];
        let thrust = (a.powi(3) + b.powi(3)) / 2.0;
        let torque = (a.powi(4) + b.powi(4)) / 2.0;
        (thrust, torque, 0.25 + 0.75 * thrust, self.p.strike.shake * (a * a - b * b).abs())
    }

    pub fn armed(&self) -> bool {
        self.fc.armed
    }

    /// Seconds armed since it was last armed.
    pub fn armed_time(&self) -> f64 {
        self.fc.armed_time
    }

    /// Height above sea level it was armed at, m.
    pub fn armed_altitude(&self) -> f64 {
        self.fc.armed_altitude
    }

    /// Why the flight controller does not arm while the switch is on, if so.
    pub fn arming_blocked(&self) -> Option<&'static str> {
        self.fc.blocked
    }

    pub fn crashed(&self) -> Option<&'static str> {
        self.crashed
    }

    /// Fell into water and sank: nothing moves it any more.
    pub fn sunk(&self) -> bool {
        self.sunk
    }

    /// Seconds of flight left at the current draw, from what is left of the charge.
    pub fn time_left(&self) -> f64 {
        (self.p.capacity - self.battery.used).max(0.0) / self.battery.current.max(1.0)
    }

    /// At rest on its feet at (`n`, `e`) on ground `elevation` high, facing `heading`, armed with
    /// the throttle down (as the aircraft start with the engine running), battery full, nothing broken.
    pub(crate) fn stand(&mut self, n: f64, e: f64, elevation: f64, heading: f64) {
        let feet = self.p.contacts.iter().map(|c| c.z).fold(f64::MIN, f64::max);
        self.body = Body { pos: V3::new(n, e, -(elevation + feet)), vel: V3::ZERO, q: quaternion(heading, 0.0, 0.0), omega: V3::ZERO };
        self.controls = MultirotorControls { arm: true, stability: self.p.stability, ..MultirotorControls::default() };
        self.renew();
        self.fc.arm_now(&self.body, elevation);
        self.run_engines(self.p.turbine.map_or(0.0, |t| t.pitch.0));
        self.data.on_ground = true;
        self.touching = u32::MAX;
    }

    /// Hovering at `pos` facing `heading`, armed (as if on the ground `ground` high below it), the
    /// throttle where it hovers in air of density `rho`, the rotors at their hover speed.
    pub(crate) fn hover(&mut self, pos: V3, heading: f64, rho: f64, ground: f64) {
        self.body = Body { pos, vel: V3::ZERO, q: quaternion(heading, 0.0, 0.0), omega: V3::ZERO };
        let hover = self.p.hover_throttle(rho);
        // Holding itself, the collective lever in the middle asks for no climb.
        let throttle = if self.p.stability == Stability::Hold { 0.5 } else { hover };
        self.controls = MultirotorControls { throttle, arm: true, stability: self.p.stability, ..MultirotorControls::default() };
        self.renew();
        let thrust = self.p.mass * GRAVITY / self.p.motors.len() as f64 * SEA_LEVEL_DENSITY / rho;
        let w = (thrust / self.p.thrust_coefficient).sqrt();
        for r in &mut self.rotors[..self.p.motors.len()] {
            *r = w;
        }
        self.fc.arm_now(&self.body, ground);
        if let Some(t) = self.p.turbine {
            self.run_engines(t.pitch.0 + hover * (t.pitch.1 - t.pitch.0));
            self.air = rho / SEA_LEVEL_DENSITY;
        }
        self.touching = 0;
    }

    /// Put in a body set by hand, as it flies (armed or not as it was).
    pub(crate) fn put(&mut self, body: Body) {
        self.body = body;
        self.previous = body;
        self.fc.settle(&body);
    }

    /// As new: battery full, nothing crashed, the controller reset, the rotors still.
    fn renew(&mut self) {
        self.rotors = [0.0; MOST_MOTORS];
        self.rotor_accel = [0.0; MOST_MOTORS];
        self.inflow = [1.0; MOST_MOTORS];
        self.battery = Battery { used: 0.0, voltage: self.p.open_voltage(0.0), current: 0.0 };
        self.fc = Controller::default();
        self.fc.settle(&self.body);
        self.data = MultirotorData::default();
        self.crashed = None;
        self.sunk = false;
        self.wash = [0.0; MOST_MOTORS];
        self.previous = self.body;
        self.blades = [[1.0; 2]; MOST_MOTORS];
        self.broken_on = BROKEN_PROPELLER;
        self.nacelles = [0.0; NACELLES];
        self.elevator = 0.0;
        self.cyclic = 0.0;
        self.engines = Engines { fuel: self.p.turbine.map_or(0.0, |t| t.fuel), ..Engines::default() };
        self.pitch = [0.0; MOST_MOTORS];
        self.induced = [0.0; MOST_MOTORS];
    }

    /// Turbines: the engines running and the rotors at the governed speed, every blade at
    /// `pitch`, the gas generators where that pitch has them in the hover.
    fn run_engines(&mut self, pitch: f64) {
        let Some(t) = self.p.turbine else { return };
        let n = self.p.motors.len();
        self.rotors[..n].fill(t.rotor_speed);
        self.pitch[..n].fill(pitch);
        let area = std::f64::consts::PI * self.p.prop_radius().powi(2);
        let thrust = self.p.static_thrust(&t, pitch, SEA_LEVEL_DENSITY);
        let induced = (thrust / (2.0 * SEA_LEVEL_DENSITY * area)).sqrt();
        self.induced[..n].fill(induced);
        let load = n as f64 * (t.induced_factor * thrust * induced + t.profile_power);
        let gas = clamp(load / (t.engines as f64 * t.power), 0.0, 1.0);
        self.engines = Engines { running: true, gas, lit: 1.0, target: t.rotor_speed, integral: 0.0, power: load, load, fuel: self.engines.fuel, flow: 0.0 };
    }

    /// One step of `dt`: the flight controller sets the motors, the rotors speed up or slow down,
    /// the frame moves; then what it struck.
    pub fn step(&mut self, dt: f64, env: &Environment) {
        if self.sunk {
            return;
        }
        self.previous = self.body;
        let orders = self.fc.control(&self.p, &self.controls, &self.body, &self.battery, &self.data, self.nacelles, self.crashed.is_some(), dt);
        match self.p.turbine {
            Some(t) => self.turn_shaft(dt, &t, &orders.duties),
            None => self.spin_motors(dt, &orders.duties),
        }
        if let Some(tilt) = &self.p.tilt {
            // Each nacelle's actuator turns it towards where it is asked, no faster than it can;
            // the elevator's and the cyclic's are taken as quick as the step.
            self.cyclic = orders.cyclic;
            for (angle, asked) in self.nacelles.iter_mut().zip(orders.nacelles) {
                let asked = clamp(asked, -tilt.back - tilt.margin, tilt.forward + tilt.margin);
                *angle += clamp(asked - *angle, -tilt.rate * dt, tilt.rate * dt);
            }
            self.elevator = orders.elevator;
        }
        self.shake(dt);

        let (m, inertia) = (self.p.mass, self.p.inertia);
        let rates = |b: &Body| {
            let f = self.forces(b, env);
            Rates::of(b, m, inertia, f.force, f.moment)
        };
        let b = self.body;
        let k1 = rates(&b);
        let k2 = rates(&b.advance(&k1, dt / 2.0));
        let k3 = rates(&b.advance(&k2, dt / 2.0));
        let k4 = rates(&b.advance(&k3, dt));
        self.body = b.advance(&Rates::rk4(&k1, &k2, &k3, &k4), dt).normalised();

        let f = self.forces(&self.body, env);
        let gravity = act(inverse(self.body.q), V3::new(0.0, 0.0, m * GRAVITY));
        self.inflow = f.inflow;
        self.engines.load = f.power;
        for (now, to) in self.induced.iter_mut().zip(f.induced) {
            *now += (to - *now) * (dt / INFLOW_TIME).min(1.0);
        }
        self.air = env.rho / SEA_LEVEL_DENSITY;
        let air = act(inverse(self.body.q), env.air_velocity);
        self.data = MultirotorData { airspeed: (self.body.vel - air).length(), on_ground: f.touching != 0, specific: (f.force - gravity) / m, thrust: f.thrust, wash: f.wash };
        self.struck(env, f.touching);
    }

    /// Turbines: the shaft over a step. The blades go to the pitches the flight controller orders
    /// (`pitches`: its "duties", nothing while the engines are stopped). The governor asks the gas
    /// generators for what the rotors took in the last step, more or less by how far the rotors
    /// are from their speed (and its integral); they follow over `spool_time` and give the shaft
    /// that share of what the air leaves them (the density's 0.8th power, as turboshafts lose
    /// power with height), never less than nothing (the freewheel). The shaft speeds up or slows
    /// down by what is left over. Started, they take `start_time` to light, giving nothing, and
    /// then run the rotors up from the speed they have to the governed one, over three times as
    /// long from rest. Stopped, they give nothing at once.
    fn turn_shaft(&mut self, dt: f64, t: &Turbine, pitches: &[f64; MOST_MOTORS]) {
        let n = self.p.motors.len();
        let e = &mut self.engines;
        e.running = self.fc.armed && e.fuel > 0.0;
        let w = self.rotors[0];
        let all = t.engines as f64 * t.power;
        if e.running {
            if e.lit < 1.0 {
                e.lit = (e.lit + dt / t.start_time).min(1.0);
                e.target = w;
            }
            e.target = (e.target + t.rotor_speed / (3.0 * t.start_time) * dt).min(t.rotor_speed);
        } else {
            (e.lit, e.gas) = (0.0, 0.0);
        }
        let asked = if e.running && e.lit >= 1.0 {
            let error = (e.target - w) / t.rotor_speed;
            e.integral = clamp(e.integral + 0.6 * error * dt, -0.3, 0.3);
            clamp(e.load / all + 4.0 * error + e.integral, 0.02, 1.0)
        } else {
            e.integral = 0.0;
            0.0
        };
        e.gas += (asked - e.gas) * (dt / t.spool_time).min(1.0);
        // The air's density where it is, from the last step's forces.
        e.power = all * self.air.powf(0.8) * e.gas.max(0.0);
        e.flow = if e.running { t.fuel_per_joule * e.power + t.engines as f64 * t.idle_flow } else { 0.0 };
        e.fuel = (e.fuel - e.flow * dt).max(0.0);
        let accel = (e.power - e.load) / (t.inertia * w.max(5.0));
        let w = (w + accel * dt).max(0.0);
        self.rotors[..n].fill(w);
        self.rotor_accel[..n].fill(accel);
        self.pitch[..n].copy_from_slice(&pitches[..n]);
    }

    /// The rotors over a step at these duties, off the battery as it sags: each motor's current
    /// from its duty, the battery's voltage and its own back-EMF, limited by its ESC; its torque
    /// against the propeller's and its friction.
    fn spin_motors(&mut self, dt: f64, duties: &[f64; MOST_MOTORS]) {
        let p = &self.p;
        let n = p.motors.len();
        let h = dt / MOTOR_STEPS as f64;
        let open = p.open_voltage(self.battery.used / p.capacity);
        let mut volts = open - p.internal_resistance * self.battery.current;
        let mut drawn = 0.0;
        for _ in 0..MOTOR_STEPS {
            let mut total = ELECTRONICS;
            for (i, &d) in duties.iter().enumerate().take(n) {
                let w = self.rotors[i];
                let (_, torque, inertia, _) = self.propeller(i);
                // Disarmed the ESC lets the motor turn freely; armed it drives it, and brakes it too.
                let current = if d > 0.0 { clamp((d * volts - w / p.kv) / p.winding_resistance, -p.max_current, p.max_current) } else { 0.0 };
                let friction = p.no_load_current / p.kv * (w / 30.0).tanh();
                let drag = p.torque_coefficient * torque * w * w * self.inflow[i];
                let accel = (current / p.kv - friction - drag) / (p.rotor_inertia * inertia);
                self.rotor_accel[i] = accel;
                self.rotors[i] = (w + accel * h).max(0.0);
                total += d * current;
            }
            total = total.max(0.0);
            volts = (open - p.internal_resistance * total).max(0.0);
            drawn += total * h;
        }
        self.battery.current = drawn / dt;
        self.battery.used += drawn;
        self.battery.voltage = volts;
    }

    /// Each rotor's noise in its own wash for this step: a seeded noise of standard deviation one
    /// that forgets itself over the wash's `correlation_time` (whether it is in its wash or not,
    /// so that the stream of numbers does not depend on the flight).
    fn shake(&mut self, dt: f64) {
        let keep = (-dt / self.p.wash.correlation_time).exp();
        let new = (1.0 - keep * keep).sqrt();
        for x in &mut self.wash[..self.p.motors.len()] {
            *x = *x * keep + new * self.rng.normal();
        }
    }

    /// How deep a rotor turning at `w` and moving through the air at `v` (body axes) is in its
    /// own wash, 0 to 1 (`WashFit`), and the descent's speed along its axis, m/s.
    /// `v` in the rotor's own axes (`to_rotor`); `share`: its thrust over `thrust_coefficient · ω²`.
    fn wash_depth(&self, w: f64, v: V3, rho: f64, share: f64) -> (f64, f64) {
        let p = &self.p;
        let fit = &p.wash;
        let sink = v.z.max(0.0);
        let area = std::f64::consts::PI * p.prop_radius().powi(2);
        let induced = (p.thrust_coefficient * rho / SEA_LEVEL_DENSITY * w * w * share / (2.0 * rho * area)).sqrt().max(0.3);
        let deep = smoothstep(fit.onset, fit.full, sink / induced);
        let across = v.x.hypot(v.y) / (sink + induced);
        // A rotor that hardly turns has no wash to fall into.
        let turning = smoothstep(20.0, 150.0, w);
        (deep * (1.0 - smoothstep(fit.clear_from, fit.clear_to, across)) * turning, sink)
    }

    /// The force and moment on the frame (body axes) in body `b`, with the rotors as they are.
    fn forces(&self, b: &Body, env: &Environment) -> Forces {
        let p = &self.p;
        let q_inv = inverse(b.q);
        let air = act(q_inv, env.air_velocity);
        let rho = env.rho / SEA_LEVEL_DENSITY;
        let mut force = act(q_inv, V3::new(0.0, 0.0, p.mass * GRAVITY));
        let mut moment = V3::ZERO;
        let mut thrust = 0.0;
        let mut power = 0.0;
        let mut induced_now = [0.0; MOST_MOTORS];
        let mut inflow = [1.0; MOST_MOTORS];
        let mut wash = 0.0;
        let radius = p.prop_radius();
        let area = std::f64::consts::PI * radius * radius;
        let mut momentum = V3::ZERO;
        for (i, motor) in p.motors.iter().enumerate() {
            let w = self.rotors[i];
            let (r, turn) = self.hub(i);
            let (left, torque, inertia, uneven) = self.propeller(i);
            // In the rotor's own axes: z down its axis.
            let v = to_rotor(b.vel + b.omega.cross(r) - air, turn);
            // The rotor's axis points up: the ground is under it.
            let upright = act(b.q, from_rotor(V3::Z, turn)).z;
            // Through the disc: climbing along its thrust brings the air in faster.
            let pitch_speed = 1.1 * p.prop_pitch * w / std::f64::consts::TAU + 0.5;
            let f_in = clamp(1.0 + v.z / pitch_speed, 0.0, 1.25);
            inflow[i] = f_in;
            let ground = if upright > 0.5 {
                let at = b.pos + act(b.q, r);
                let height = (-at.z - env.height(at.x, at.y)).max(0.0) / upright;
                let k = radius / (4.0 * height.max(radius * 0.6));
                (1.0 / (1.0 - k * k)).min(1.35)
            } else {
                1.0
            };
            // Turbines: the blades' pitch, less what the air through the disc takes of it.
            let share = match &p.turbine {
                Some(t) => clamp(self.pitch[i] + (v.z - self.induced[i]) / (w.max(1.0) * t.inflow_length), -0.3, 1.3),
                None => 1.0,
            };
            // Its own wash: thrust lost, and thrust that comes and goes.
            let (deep, sink) = self.wash_depth(w, v, env.rho, share.max(0.0));
            let clean = match &p.turbine {
                Some(_) => p.thrust_coefficient * left * rho * w * w * share * ground,
                None => p.thrust_coefficient * left * rho * w * w * f_in * ground,
            };
            let shake = p.wash.fluctuation * clean + p.wash.buffet * 0.5 * env.rho * area * sink * sink;
            // A chipped propeller shakes on the same noise, in its wash or not.
            let t = clean * (1.0 - p.wash.thrust_loss * deep) + (deep * shake + uneven * clean) * self.wash[i];
            wash += p.wash.thrust_loss * deep / p.motors.len() as f64;
            thrust += t;
            let push = V3::new(0.0, 0.0, -t);
            let edgewise = -p.edgewise_drag * left * rho * w * V3::new(v.x, v.y, 0.0);
            let pushed = from_rotor(push + edgewise, turn);
            force += pushed;
            moment += r.cross(pushed);
            if let (Some(tilt), Some(_)) = (&p.tilt, motor.nacelle) {
                // The cyclic: the thrust moved off the hub, a moment about the line across the frame.
                moment.y += tilt.cyclic_arm * self.cyclic * t;
            }
            let spin = motor.spin.sign();
            let drag = match &p.turbine {
                Some(tb) => {
                    // What turning it takes: the induced power of its thrust (Glauert's induced
                    // velocity through a disc that climbs at `climb` and moves across at
                    // `across`: `v = T / 2ρA√(across² + (climb + v)²)`, found going round), the
                    // climb's, and the blades' profile power; over its speed, the torque.
                    let (climb, across) = (-v.z, v.x.hypot(v.y));
                    let k = t.abs() / (2.0 * env.rho * area);
                    let mut induced = k.sqrt();
                    for _ in 0..4 {
                        induced = 0.5 * (induced + k / (across * across + (climb + induced).powi(2)).sqrt().max(1e-3));
                    }
                    let needed = tb.induced_factor * t.abs() * induced + t * climb + tb.profile_power * torque * rho * (w / tb.rotor_speed).powi(3);
                    power += needed;
                    induced_now[i] = if t >= 0.0 { induced } else { -induced };
                    needed / w.max(5.0)
                }
                None => p.torque_coefficient * torque * rho * w * w * f_in,
            };
            moment -= from_rotor(V3::new(0.0, 0.0, spin * (drag + p.rotor_inertia * inertia * self.rotor_accel[i])), turn);
            momentum += from_rotor(V3::new(0.0, 0.0, spin * p.rotor_inertia * inertia * w), turn);
        }
        // The rotors' gyroscopic moment, and the frame's drag and damping.
        moment -= b.omega.cross(momentum);
        let v = b.vel - air;
        force -= 0.5 * env.rho * v.length() * p.drag_area * v;
        moment -= p.rotational_damping * b.omega;
        // Its wings and tail, each in the air it moves through.
        if let Some(tilt) = &p.tilt {
            for surface in tilt.surfaces {
                let lift = surface.force(b.vel + b.omega.cross(surface.position) - air, env.rho, self.elevator);
                force += lift;
                moment += surface.position.cross(lift);
            }
        }

        // The ground: each point a spring and a damper along the ground's normal, and friction.
        let mut touching = 0u32;
        if env.kind != GroundKind::Water {
            let up = normalize(V3::new(-env.slope_n, -env.slope_e, -1.0));
            for (k, &r) in p.contacts.iter().enumerate().take(MOST_CONTACTS) {
                let at = b.pos + act(b.q, r);
                let depth = at.z + env.height(at.x, at.y);
                if depth <= 0.0 {
                    continue;
                }
                touching |= 1 << k;
                let pen = depth * -up.z;
                let vw = act(b.q, b.vel + b.omega.cross(r));
                let vn = vw.dot(up);
                let normal = (p.contact_spring * pen - p.contact_damping * vn).max(0.0);
                let vt = vw - vn * up;
                let speed = vt.length();
                let friction = if speed > 1e-6 { -vt / speed * (speed / 0.05).tanh() * p.friction * normal } else { V3::ZERO };
                let f = act(q_inv, normal * up + friction);
                force += f;
                moment += r.cross(f);
            }
        }
        Forces { force, moment, thrust, inflow, touching, wash, power, induced: induced_now }
    }

    /// After a step: what it struck. A point that has gone into the ground faster than
    /// `crash_speed` breaks it; under water it sinks.
    fn struck(&mut self, env: &Environment, touching: u32) {
        if env.kind == GroundKind::Water && self.body.pos.z + env.ground_height > -0.02 {
            if self.crashed.is_none() {
                self.crashed = Some("Fell into the water");
            }
            self.sunk = true;
            self.fc.disarm();
            return;
        }
        let new = touching & !self.touching;
        self.touching = touching;
        if new == 0 || self.crashed.is_some() {
            return;
        }
        let up = normalize(V3::new(-env.slope_n, -env.slope_e, -1.0));
        let before = self.previous;
        let hardest = (0..self.p.contacts.len().min(MOST_CONTACTS))
            .filter(|k| new & (1 << k) != 0)
            .map(|k| {
                let r = self.p.contacts[k];
                -act(before.q, before.vel + before.omega.cross(r)).dot(up)
            })
            .fold(0.0, f64::max);
        if hardest > self.p.crash_speed {
            self.crash("Crashed into the ground");
        } else if self.propeller_broken() {
            // Down on what is left of its rotors: the pilot disarms.
            self.crash(self.broken_on);
        }
    }

    fn crash(&mut self, why: &'static str) {
        self.crashed = Some(why);
        self.fc.disarm();
    }

    /// The points of it that run into things (its contacts and its rotors' hubs), where they are
    /// and how they move.
    pub(crate) fn contact_points(&self) -> Vec<ContactPoint> {
        let b = &self.body;
        let hubs: Vec<V3> = (0..self.p.motors.len()).map(|i| self.hub(i).0).collect();
        self.p
            .contacts
            .iter()
            .chain(hubs.iter())
            .map(|&r| {
                let (at, vel) = b.point(r);
                ContactPoint { at, vel, body: r, on: On::Body, length: 0.05 }
            })
            .collect()
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

    /// A collision against something that holds at `speed`: it crashes if it is faster than it
    /// can take, keeping `keep` of its velocity.
    pub(crate) fn collide(&mut self, what: &'static str, speed: f64, keep: f64) {
        if speed > self.p.crash_speed {
            self.crash(what);
        }
        self.body.vel *= keep;
    }

    /// What its pilot observes of it.
    pub fn observe(&self) -> MultirotorObservation {
        let n = self.p.motors.len();
        let c = &self.controls;
        MultirotorObservation {
            armed: self.fc.armed,
            arming_blocked: self.fc.blocked.map(str::to_string),
            throttle: c.throttle,
            roll: c.roll,
            pitch: c.pitch,
            yaw: c.yaw,
            motor_speed: self.rotors,
            motors: n,
            battery_voltage: self.battery.voltage,
            battery_current: self.battery.current,
            battery_used_mah: self.battery.used / 3.6,
            altitude: -self.body.pos.z,
            vertical_speed: -self.body.world_velocity().z,
            airspeed: self.data.airspeed,
            tilt: c.tilt,
            nacelles: self.nacelles,
            rotor_share: self.p.turbine.map_or(0.0, |t| self.rotors[0] / t.rotor_speed),
            torque: self.torque(),
            fuel: self.engines.fuel,
        }
    }

    /// What is seen of it now, for a replay.
    pub fn look(&self) -> MultirotorLook {
        MultirotorLook {
            body: self.body,
            controls: self.controls,
            rotors: self.rotors,
            battery: self.battery,
            armed: self.fc.armed,
            armed_time: self.fc.armed_time,
            armed_altitude: self.fc.armed_altitude,
            data: self.data,
            crashed: self.crashed,
            sunk: self.sunk,
            blades: self.blades,
            nacelles: self.nacelles,
            elevator: self.elevator,
            cyclic: self.cyclic,
            engines: self.engines,
        }
    }

    /// For a replay: looks as it did when `look` was taken. Only to be drawn.
    pub fn show(&mut self, look: &MultirotorLook) {
        self.body = look.body;
        self.controls = look.controls;
        self.rotors = look.rotors;
        self.battery = look.battery;
        self.fc.armed = look.armed;
        self.fc.armed_time = look.armed_time;
        self.fc.armed_altitude = look.armed_altitude;
        self.data = look.data;
        self.crashed = look.crashed;
        self.sunk = look.sunk;
        self.blades = look.blades;
        self.nacelles = look.nacelles;
        self.elevator = look.elevator;
        self.cyclic = look.cyclic;
        self.engines = look.engines;
    }
}

/// A propeller's disc in world axes, as it meets things: its middle, its axis, how far its
/// longer blade reaches (m), and its rotor.
struct Disc {
    centre: V3,
    axis: V3,
    radius: f64,
    rotor: usize,
}

impl Multirotor {
    /// The discs of its rotors that are cutting (turning faster than `CUTTING`, not broken).
    fn discs(&self) -> Vec<Disc> {
        let b = &self.body;
        let radius = self.p.prop_radius();
        (0..self.p.motors.len())
            .filter(|&i| self.rotors[i] > CUTTING && self.blades[i][0].max(self.blades[i][1]) > self.p.strike.stub)
            .map(|i| {
                let (hub, turn) = self.hub(i);
                Disc { centre: b.pos + act(b.q, hub), axis: act(b.q, from_rotor(V3::Z, turn)), radius: radius * self.blades[i][0].max(self.blades[i][1]), rotor: i }
            })
            .collect()
    }

    /// The balls of its frame in world axes (`MultirotorType::parts`).
    fn balls(&self) -> impl Iterator<Item = (V3, f64)> + '_ {
        self.p.parts.iter().map(|&(c, r)| (self.body.pos + act(self.body.q, c), r))
    }

    /// How far any part of it reaches from its centre of gravity, m.
    fn reach(&self) -> f64 {
        let props = (0..self.p.motors.len()).map(|i| self.hub(i).0.length() + self.p.prop_radius());
        self.p.parts.iter().map(|(c, r)| c.length() + r).chain(props).fold(0.0, f64::max)
    }

    /// How a point of it (world axes) moves, world axes.
    fn velocity_at(&self, at: V3) -> V3 {
        let b = &self.body;
        b.point(act(inverse(b.q), at - b.pos)).1
    }

    /// One over the mass it shows to a push along `n` at `at` (world axes): its mass's, and its
    /// inertia's as the push turns it.
    fn give(&self, at: V3, n: V3) -> f64 {
        let back = inverse(self.body.q);
        let (r, n) = (act(back, at - self.body.pos), act(back, n));
        let turn = r.cross(n) / self.p.inertia;
        1.0 / self.p.mass + n.dot(turn.cross(r))
    }

    /// An impulse `j` (N s, world axes) at `at`.
    fn push(&mut self, at: V3, j: V3) {
        let back = inverse(self.body.q);
        let (r, j) = (act(back, at - self.body.pos), act(back, j));
        self.body.vel += j / self.p.mass;
        self.body.omega += r.cross(j) / self.p.inertia;
    }

    /// Propeller `i` has struck something `depth` metres into its disc (from its longer blade's
    /// tip), meeting it at `speed` (m/s): a blade chipped, or the propeller broken (`PropStrike`).
    fn cut(&mut self, i: usize, depth: f64, speed: f64) {
        let s = self.p.strike;
        let radius = self.p.prop_radius();
        let blades = &mut self.blades[i];
        let longer = if blades[0] >= blades[1] { 0 } else { 1 };
        let left = (blades[longer] - depth / radius).max(s.stub);
        if speed > s.graze_speed || (1.0 - left) * radius > s.chip_depth {
            *blades = [s.stub; 2];
        } else {
            // The blade that comes round first loses its tip; the other, a millisecond later,
            // finds the thing knocked back by the blow and loses half as much.
            let other = 1 - longer;
            blades[other] = blades[other].min(blades[longer] - depth / radius / 2.0);
            blades[longer] = left;
        }
    }
}

/// How deep a ball (centre, radius) reaches into a disc `half` thick either side of its plane, m
/// from the rim, and which way from its axis it is (in its plane).
fn into_disc(d: &Disc, centre: V3, r: f64, half: f64) -> Option<(f64, V3)> {
    let rel = centre - d.centre;
    let along = rel.dot(d.axis);
    let gap = along.abs() - half;
    if gap >= r {
        return None;
    }
    // The ball's section in the disc's thickness.
    let section = if gap > 0.0 { (r * r - gap * gap).sqrt() } else { r };
    let flat = rel - along * d.axis;
    let from_axis = flat.length();
    let depth = d.radius - (from_axis - section);
    if depth <= 0.0 {
        return None;
    }
    let out = if from_axis > 1e-9 { flat / from_axis } else { V3::ZERO };
    Some((depth.min(d.radius), out))
}

/// Where two discs (each `half` thick either side of its plane) meet, if they do: how deep each
/// reaches into the other (m from its rim) and a point where they meet.
fn discs_meet(a: &Disc, b: &Disc, half: f64) -> Option<(f64, f64, V3)> {
    let c = a.axis.dot(b.axis);
    let cross = a.axis.cross(b.axis);
    let s = cross.length();
    if s < 0.1 {
        // Within 6° of each other: two flat discs, as deep as they overlap seen along the axis,
        // if their planes are within their thickness (and the tilt of the rims) of each other.
        let rel = b.centre - a.centre;
        let along = rel.dot(a.axis);
        if along.abs() > 2.0 * half + a.radius.max(b.radius) * s {
            return None;
        }
        let flat = rel - along * a.axis;
        let apart = flat.length();
        let overlap = a.radius + b.radius - apart;
        if overlap <= 0.0 {
            return None;
        }
        let towards = if apart > 1e-9 { flat / apart } else { V3::ZERO };
        let at = a.centre + towards * (a.radius - overlap / 2.0) + along / 2.0 * a.axis;
        return Some((overlap.min(a.radius), overlap.min(b.radius), at));
    }
    // Planes at an angle: they meet along a line; each disc holds a stretch of it (a chord), and
    // the discs meet where the stretches overlap.
    let u = cross / s;
    let (da, db) = (a.axis.dot(a.centre), b.axis.dot(b.centre));
    let p0 = (a.axis * (da - db * c) + b.axis * (db - da * c)) / (s * s);
    let chord = |d: &Disc| {
        let t = (d.centre - p0).dot(u);
        let off = (p0 + u * t - d.centre).length();
        (off < d.radius).then(|| (t, off, (d.radius * d.radius - off * off).sqrt()))
    };
    let ((ta, oa, ha), (tb, ob, hb)) = (chord(a)?, chord(b)?);
    let (lo, hi) = ((ta - ha).max(tb - hb), (ta + ha).min(tb + hb));
    if lo >= hi {
        return None;
    }
    // Each the deeper the nearer to its middle the shared stretch comes.
    let depth = |t: f64, off: f64, r: f64| r - (off * off + (t.clamp(lo, hi) - t).powi(2)).sqrt();
    let at = p0 + u * ((ta + tb) / 2.0).clamp(lo, hi);
    Some((depth(ta, oa, a.radius), depth(tb, ob, b.radius), at))
}

/// Two multirotors that may touch, after a step of the world (`World::collide_vehicles`).
///
/// A propeller of either whose disc meets the other's frame, or one of the other's discs, is cut
/// (`Multirotor::cut`: a chip if slow and shallow, broken otherwise), by how deep the other reached
/// into it and how fast they met there. A broken propeller leaves its quad on the other rotors: the
/// flight controller cannot hold it, it tumbles and comes down, and is crashed when it reaches the
/// ground (`struck`).
///
/// Frames that touch (their balls, `MultirotorType::parts`) and are coming together get one knock:
/// at each pair of balls that touch, an impulse along the line between their middles, those of a
/// collision of two rigid bodies with restitution (the lower of theirs), their momentum kept, each
/// turned by where it is struck; and they are moved out of each other by the deepest overlap, the
/// lighter the more. Nothing slows them while they move apart, so they never stick. Meeting faster
/// than its `crash_speed`, each crashes.
pub(crate) fn collide_multirotors(a: &mut Multirotor, b: &mut Multirotor) {
    if (a.body.pos - b.body.pos).length() > a.reach() + b.reach() {
        return;
    }
    // The propellers, all of them measured before any is cut: (on b, rotor, depth, speed).
    let mut cuts: Vec<(bool, usize, f64, f64)> = Vec::new();
    let (discs_a, discs_b) = (a.discs(), b.discs());
    let (half_a, half_b) = (a.p.strike.thickness / 2.0, b.p.strike.thickness / 2.0);
    for (mine, other, discs, half, on_b) in [(&*a, &*b, &discs_a, half_a, false), (&*b, &*a, &discs_b, half_b, true)] {
        for d in discs {
            let mut deepest: Option<(f64, V3)> = None;
            for (c, r) in other.balls() {
                if let Some((depth, out)) = into_disc(d, c, r, half) {
                    if deepest.is_none_or(|x| depth > x.0) {
                        deepest = Some((depth, d.centre + out * (d.radius - depth)));
                    }
                }
            }
            if let Some((depth, at)) = deepest {
                let speed = (mine.velocity_at(at) - other.velocity_at(at)).length();
                cuts.push((on_b, d.rotor, depth, speed));
            }
        }
    }
    for da in &discs_a {
        for db in &discs_b {
            if let Some((into_a, into_b, at)) = discs_meet(da, db, half_a.max(half_b)) {
                let speed = (a.velocity_at(at) - b.velocity_at(at)).length();
                cuts.push((false, da.rotor, into_a, speed));
                cuts.push((true, db.rotor, into_b, speed));
            }
        }
    }
    for (on_b, rotor, depth, speed) in cuts {
        let m = if on_b { &mut *b } else { &mut *a };
        m.cut(rotor, depth, speed);
        if m.blades[rotor][0].max(m.blades[rotor][1]) <= m.p.strike.stub {
            m.broken_on = BROKEN_PROPELLER;
        }
    }

    // The frames: every two balls that overlap, the normal from b's to a's.
    let mut contacts: Vec<(f64, V3, V3)> = Vec::new();
    for (ca, ra) in a.balls() {
        for (cb, rb) in b.balls() {
            let d = ca - cb;
            let apart = d.length();
            let overlap = ra + rb - apart;
            if overlap > 0.0 {
                // Balls with the same middle: apart the way the quads' middles are.
                let n = if apart > 1e-9 { d / apart } else { normalize(a.body.pos - b.body.pos + V3::new(0.0, 0.0, -1e-9)) };
                contacts.push((overlap, cb + n * (rb - overlap / 2.0), n));
            }
        }
    }
    let Some(&(overlap, _, n)) = contacts.iter().max_by(|x, y| x.0.total_cmp(&y.0)) else { return };
    // Out of each other by the deepest overlap, each by its share (the lighter the more).
    let (inv_a, inv_b) = (1.0 / a.p.mass, 1.0 / b.p.mass);
    a.body.pos += n * (overlap * inv_a / (inv_a + inv_b));
    b.body.pos -= n * (overlap * inv_b / (inv_a + inv_b));
    // The knock: at each point that is closing, the impulse that leaves it opening at the
    // restitution's share of the speed it closed at. Points of one knock push on each other (two
    // arms on one battery: each alone would twist it), so the impulses are worked out together, a
    // few rounds over all of them, each kept pushing (never pulling).
    let closing: Vec<f64> = contacts.iter().map(|&(_, at, n)| -(a.velocity_at(at) - b.velocity_at(at)).dot(n)).collect();
    let hardest = closing.iter().copied().fold(0.0, f64::max);
    if hardest <= 0.0 {
        return;
    }
    let e = a.p.restitution.min(b.p.restitution);
    let mut given = vec![0.0; contacts.len()];
    for _ in 0..8 {
        for (k, &(_, at, n)) in contacts.iter().enumerate() {
            if closing[k] <= 0.0 {
                continue;
            }
            let now = -(a.velocity_at(at) - b.velocity_at(at)).dot(n);
            let j = ((now + e * closing[k]) / (a.give(at, n) + b.give(at, n))).max(-given[k]);
            given[k] += j;
            a.push(at, n * j);
            b.push(at, -n * j);
        }
    }
    for m in [a, b] {
        if hardest > m.p.crash_speed && m.crashed.is_none() {
            m.crash(COLLIDED);
        }
    }
}

/// The surface of a real place's buildings and bridges nearest a point (north, east, height)
/// within a reach: how far, where, the way out of it and what it is (`Terrain::solid_surface`).
pub(crate) type Mapped<'a> = dyn Fn([f64; 3], f64) -> Option<(f64, [f64; 3], [f64; 3], ObstacleKind)> + 'a;

impl Multirotor {
    /// Runs into fixed things after a step (`vehicle`: the trees and buildings near it, a course's
    /// gates), as against another quad (`collide_multirotors`) but for a thing that does not move:
    ///
    /// - A propeller whose disc meets a trunk, a gate's tube or a wall while it turns is cut by how
    ///   deep the thing reaches into it and how fast they meet there (`cut`: a chip if slow and
    ///   shallow, broken otherwise). Turning through a crown it meets its twigs, `TWIGS_PER_METRE`
    ///   of the way it moves through it (fewer at the crown's fringe), each a cut of `TWIG_DEPTH`
    ///   (chipped at a walk, broken faster), drawn from a hash of where the disc is: still
    ///   deterministic.
    /// - The frame's balls (`MultirotorType::parts`) that reach into a solid are moved out of it by
    ///   the deepest overlap and get one knock along the way out: the restitution's share of the
    ///   speed they came in at, turned by where they are struck. Harder than `crash_speed` it
    ///   crashes. Nothing holds it while it moves away, so it never sticks.
    ///
    /// `what` says of each kind what a crash into it and a propeller broken on it are called.
    /// `mapped` is the surface of a real place's buildings and bridges nearest a point (north, east,
    /// height) within a reach (`Terrain::solid_surface`), met as walls are.
    pub(crate) fn hit(&mut self, obstacles: &[Obstacle], what: impl Fn(ObstacleKind) -> (&'static str, &'static str), mapped: &Mapped) {
        let reach = self.reach() + self.body.vel.length() * crate::world::World::DT;
        let pos = self.body.pos;
        let near: Vec<&Obstacle> = obstacles
            .iter()
            .filter(|o| {
                let (side, top) = o.extent();
                let up = -pos.z - o.base;
                (o.n - pos.x).hypot(o.e - pos.y) < side + reach && up > -reach - side && up < top + reach
            })
            .collect();
        let mapped_near = mapped([pos.x, pos.y, -pos.z], reach).is_some();
        if near.is_empty() && !mapped_near {
            return;
        }
        // The propellers: how deep a solid reaches into each disc, and the twigs of a crown.
        let half = self.p.strike.thickness / 2.0;
        let mut cuts: Vec<(usize, f64, f64, &'static str)> = Vec::new();
        for d in self.discs() {
            let c = d.centre;
            for o in &near {
                let (gap, at, _) = o.surface(c.x, c.y, -c.z);
                if gap < d.radius + half {
                    // The solid's section nearest the disc's middle as a ball (a trunk or a tube as
                    // thick as it is there; a wall a point of its face): into the disc as it reaches.
                    let point = V3::new(at[0], at[1], -at[2]);
                    let thick = match o.shape {
                        Shape::Tree { trunk, top, .. } => Obstacle::trunk_radius(trunk, top, -c.z - o.base).max(0.02),
                        Shape::Bar { radius, .. } => radius,
                        Shape::Block { .. } | Shape::Mapped => 0.002,
                    };
                    let centre = point - (c - point).normalize_or_zero() * thick;
                    if let Some((depth, out)) = into_disc(&d, centre, thick, half) {
                        let at = d.centre + out * (d.radius - depth);
                        cuts.push((d.rotor, depth, self.velocity_at(at).length(), what(o.kind).1));
                    }
                }
                let depth = o.crown_depth(c.x, c.y, -c.z);
                if depth > 0.0 {
                    let speed = self.velocity_at(c).length();
                    let key = |x: f64| (x * 50.0).round() as i64;
                    let draw = crate::math::hash2(key(c.x), key(c.y), (key(c.z) as u64).wrapping_mul(31).wrapping_add(d.rotor as u64));
                    if draw < TWIGS_PER_METRE * smoothstep(0.0, TWIG_FRINGE, depth) * speed * crate::world::World::DT {
                        cuts.push((d.rotor, TWIG_DEPTH, speed, what(o.kind).1));
                    }
                }
            }
            if mapped_near {
                // A wall, a roof or a deck of a real place: a point of its face, as a block's.
                if let Some((gap, at, _, kind)) = mapped([c.x, c.y, -c.z], d.radius + half) {
                    if gap < d.radius + half {
                        let point = V3::new(at[0], at[1], -at[2]);
                        let centre = point - (c - point).normalize_or_zero() * 0.002;
                        if let Some((depth, out)) = into_disc(&d, centre, 0.002, half) {
                            let at = d.centre + out * (d.radius - depth);
                            cuts.push((d.rotor, depth, self.velocity_at(at).length(), what(kind).1));
                        }
                    }
                }
            }
        }
        for (rotor, depth, speed, on) in cuts {
            self.cut(rotor, depth, speed);
            if self.blades[rotor][0].max(self.blades[rotor][1]) <= self.p.strike.stub {
                self.broken_on = on;
            }
        }

        // The frame: every ball that reaches into a solid, how deep, where and the way out.
        let mut contacts: Vec<(f64, V3, V3, &'static str)> = Vec::new();
        let balls: Vec<(V3, f64)> = self.balls().collect();
        for (c, r) in balls {
            for o in &near {
                let (gap, at, way) = o.surface(c.x, c.y, -c.z);
                if gap < r {
                    contacts.push((r - gap, V3::new(at[0], at[1], -at[2]), V3::new(way[0], way[1], -way[2]), what(o.kind).0));
                }
            }
            if mapped_near {
                if let Some((gap, at, way, kind)) = mapped([c.x, c.y, -c.z], r) {
                    if gap < r {
                        contacts.push((r - gap, V3::new(at[0], at[1], -at[2]), V3::new(way[0], way[1], -way[2]), what(kind).0));
                    }
                }
            }
        }
        let Some(&(overlap, _, n, _)) = contacts.iter().max_by(|x, y| x.0.total_cmp(&y.0)) else { return };
        self.body.pos += n * overlap;
        // The knock: at each point coming in, the impulse that sends it out at the restitution's
        // share of the speed it came in at, worked out together over a few rounds (as two quads').
        let closing: Vec<f64> = contacts.iter().map(|&(_, at, n, _)| -self.velocity_at(at).dot(n)).collect();
        let (hardest_at, hardest) = closing.iter().copied().enumerate().fold((0, 0.0), |a, (k, x)| if x > a.1 { (k, x) } else { a });
        if hardest <= 0.0 {
            return;
        }
        let e = self.p.restitution;
        let mut given = vec![0.0; contacts.len()];
        for _ in 0..8 {
            for (k, &(_, at, n, _)) in contacts.iter().enumerate() {
                if closing[k] <= 0.0 {
                    continue;
                }
                let now = -self.velocity_at(at).dot(n);
                let j = ((now + e * closing[k]) / self.give(at, n)).max(-given[k]);
                given[k] += j;
                self.push(at, n * j);
            }
        }
        if hardest > self.p.crash_speed && self.crashed.is_none() {
            self.crash(contacts[hardest_at].3);
        }
    }
}

/// What a step's forces come to.
struct Forces {
    force: V3,
    moment: V3,
    thrust: f64,
    inflow: [f64; MOST_MOTORS],
    touching: u32,
    wash: f64,
    /// Turbines: what turning the rotors takes, W, and each one's induced velocity, m/s.
    power: f64,
    induced: [f64; MOST_MOTORS],
}

/// What a replay keeps of a multirotor many times a second (`Multirotor::look`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MultirotorLook {
    pub body: Body,
    pub controls: MultirotorControls,
    pub rotors: [f64; MOST_MOTORS],
    pub battery: Battery,
    pub armed: bool,
    pub armed_time: f64,
    pub armed_altitude: f64,
    pub data: MultirotorData,
    pub crashed: Option<&'static str>,
    pub sunk: bool,
    /// How far each propeller's blades reach (`Multirotor::blades`).
    pub blades: [[f64; 2]; MOST_MOTORS],
    /// Where its nacelles, its elevator and its cyclic are (`Multirotor::nacelles`, `elevator`,
    /// `cyclic`), and its engines.
    pub nacelles: [f64; NACELLES],
    pub elevator: f64,
    pub cyclic: f64,
    pub engines: Engines,
}

impl MultirotorLook {
    /// The look `k` of the way (0...1) from this one to `next`, taken `dt` seconds later.
    pub fn blend(&self, next: &MultirotorLook, k: f64, dt: f64) -> MultirotorLook {
        if k <= 0.0 {
            return *self;
        }
        if k >= 1.0 {
            return *next;
        }
        let near = if k < 0.5 { self } else { next };
        let mix = |a: f64, b: f64| a + (b - a) * k;
        let (c, d) = (&self.controls, &next.controls);
        MultirotorLook {
            body: self.body.blend(&next.body, k, dt),
            controls: MultirotorControls { throttle: mix(c.throttle, d.throttle), roll: mix(c.roll, d.roll), pitch: mix(c.pitch, d.pitch), yaw: mix(c.yaw, d.yaw), tilt: mix(c.tilt, d.tilt), trim: mix(c.trim, d.trim), ..near.controls },
            nacelles: std::array::from_fn(|i| mix(self.nacelles[i], next.nacelles[i])),
            elevator: mix(self.elevator, next.elevator),
            cyclic: mix(self.cyclic, next.cyclic),
            rotors: std::array::from_fn(|i| mix(self.rotors[i], next.rotors[i])),
            battery: Battery { used: mix(self.battery.used, next.battery.used), voltage: mix(self.battery.voltage, next.battery.voltage), current: mix(self.battery.current, next.battery.current) },
            armed_time: mix(self.armed_time, next.armed_time),
            ..*near
        }
    }
}
