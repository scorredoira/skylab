//! The aircraft: a rigid body with strip-theory wing, tail, propeller, engine and landing gear.
//! What kind of aircraft it is lives in an `AircraftType`, read from its file (`file`):
//! `AircraftType::c172()` (the default) and `AircraftType::extra300()`. It knows nothing about terrain
//! or weather: each step receives an `Environment`. What the world asks of it as one of its vehicles
//! (instruments, placing, contacts, observation) is `in_world`.
//!
//! Body axes: x forward, y right, z down. World axes: north, east, down.

use std::f64::consts::PI;

use glam::DMat3;

pub mod file;
mod in_world;
pub mod jet;
pub use jet::{Intake, Jets, MOST_ENGINES};
pub use in_world::Look;

pub use in_world::AircraftObservation;
use in_world::Gauges;

use crate::atmosphere::{self, GAMMA, GAS_CONSTANT};
use crate::body::{Body, Rates};
use crate::gfc700::Gfc700Fit;
use crate::instruments::{InstrumentFit, Instruments};
use crate::wreck::{Cut, PieceKind, Pieces, Portion, Slice, Wreck};
use crate::math::{
    act, axis_angle, clamp, interpolate, inverse, lerp, normalize, quaternion, smoothstep, Quat, Rng,
    DEG, GRAVITY, V3,
};

/// What the pilot of an aircraft (and the game) sets (`vehicle::Controls::Aircraft`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct AircraftControls {
    /// +1 rolls right.
    pub aileron: f64,
    /// +1 is full back (nose up).
    pub elevator: f64,
    /// +1 is full right pedal.
    pub rudder: f64,
    pub throttle: f64,
    /// 0...3 for 0°, 10°, 20°, 30°.
    pub flap_notch: i32,
    /// +1 is full nose-up trim.
    pub trim: f64,
    pub brake_left: f64,
    pub brake_right: f64,
    pub parking_brake: bool,
    /// Reverse thrust selected (a jet with reversers): the throttle then sets how much. The
    /// reversers open only on the ground, and the engines stay at idle while they move.
    pub reverse: bool,
    /// The speed brake lever, 0 (down) to 1 (all the spoilers up); in the air they rise only as
    /// far as the flight detent.
    pub spoilers: f64,
    /// The landing gear lever up (a retractable gear). It stays down on the ground.
    pub gear_up: bool,
}

impl AircraftControls {
    /// How hard a wheel with `brake` is braked, 0...1: the parking brake holds both.
    pub fn brake(&self, brake: Brake) -> f64 {
        match brake {
            Brake::None => 0.0,
            _ if self.parking_brake => 1.0,
            Brake::Left => self.brake_left,
            Brake::Right => self.brake_right,
        }
    }
}

/// The aircraft's state: its rigid body (`body()`) and its propeller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    /// World position: north, east, down.
    pub pos: V3,
    /// Inertial velocity in body axes.
    pub vel: V3,
    /// Body → world.
    pub q: Quat,
    /// Angular velocity in body axes, rad/s.
    pub omega: V3,
    /// Propeller angular speed, rad/s.
    pub prop_speed: f64,
    /// Blade angle at 75% of the radius of a constant-speed propeller, radians; the governor
    /// moves it between its stops (`Governor`). A fixed-pitch propeller ignores it.
    pub blade_pitch: f64,
    /// Fan speed of each jet engine (`AircraftType::jets`, left to right), as a share of the
    /// take-off rating's: `Jets`. Zero for the engines it has not.
    pub n1: [f64; MOST_ENGINES],
}

impl Default for State {
    fn default() -> State {
        State {
            pos: V3::ZERO,
            vel: V3::ZERO,
            q: axis_angle(0.0, V3::new(0.0, 0.0, 1.0)),
            omega: V3::ZERO,
            prop_speed: 70.0,
            blade_pitch: 0.0,
            n1: [0.0; MOST_ENGINES],
        }
    }
}

impl State {
    /// The rigid body: position, attitude, velocity and rates.
    pub fn body(&self) -> Body {
        Body { pos: self.pos, vel: self.vel, q: self.q, omega: self.omega }
    }

    /// This state with the rigid body's moved to `body`.
    pub fn with_body(self, body: Body) -> State {
        State { pos: body.pos, vel: body.vel, q: body.q, omega: body.omega, ..self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundKind {
    Runway,
    Grass,
    Water,
}

/// The air and the ground around the aircraft during one step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    pub rho: f64,
    /// Temperature of the air, K: with `rho`, its pressure and the speed of sound.
    pub temperature: f64,
    /// The share of `rho` that is water vapour (0 in dry air): the engine burns only the rest.
    pub vapour: f64,
    /// Velocity of the air mass, NED.
    pub air_velocity: V3,
    /// Vertical gust gradient across the span, as an equivalent roll rate (rad/s).
    pub roll_gust: f64,
    /// Ground as a plane through a reference point.
    pub ref_n: f64,
    pub ref_e: f64,
    pub ground_height: f64,
    pub slope_n: f64,
    pub slope_e: f64,
    pub kind: GroundKind,
}

impl Default for Environment {
    fn default() -> Environment {
        Environment {
            rho: atmosphere::SEA_LEVEL_DENSITY,
            temperature: atmosphere::SEA_LEVEL_TEMPERATURE,
            vapour: 0.0,
            air_velocity: V3::ZERO,
            roll_gust: 0.0,
            ref_n: 0.0,
            ref_e: 0.0,
            ground_height: -1e6,
            slope_n: 0.0,
            slope_e: 0.0,
            kind: GroundKind::Grass,
        }
    }
}

impl Environment {
    /// The air of an altitude of the standard day, still, over no ground.
    pub fn standard(altitude: f64) -> Environment {
        Environment { rho: atmosphere::density(altitude), temperature: atmosphere::temperature(altitude), ..Environment::default() }
    }

    /// Speed of sound, m/s.
    #[inline(always)]
    pub fn sound_speed(&self) -> f64 {
        (GAMMA * GAS_CONSTANT * self.temperature).sqrt()
    }

    /// Static pressure, Pa.
    #[inline(always)]
    pub fn pressure(&self) -> f64 {
        self.rho * GAS_CONSTANT * self.temperature
    }

    /// Height of the ground plane at a point.
    #[inline(always)]
    pub fn height(&self, n: f64, e: f64) -> f64 {
        self.ground_height + self.slope_n * (n - self.ref_n) + self.slope_e * (e - self.ref_e)
    }
}

/// The most legs the landing gear of an aircraft has: each a `Wheel`, whatever tyres it carries
/// (a bogie of four is one leg).
pub const MOST_WHEELS: usize = 5;

/// How many points of the airframe may touch the ground: `AircraftType::hard_points`, then the
/// nacelle of each jet engine (`jet::Mount::pod`).
pub const MOST_TOUCHES: usize = 7 + MOST_ENGINES;

/// Which brake pedal works a wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum Brake {
    None,
    Left,
    Right,
}

/// One leg of the landing gear: where its tyre touches the ground (body axes, from the reference
/// centre of gravity, leg extended), its spring, and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    pub contact: V3,
    /// Spring rate, N/m, and damping, N·s/m.
    pub spring: f64,
    pub damping: f64,
    /// Turned by the pedals (a nose or tail wheel).
    pub steers: bool,
    pub brake: Brake,
    /// Slip angle, radians, at which its side grip is nearly all used.
    pub slip: f64,
    /// Load that breaks the leg, N, and what the player is told then.
    pub strength: f64,
    pub broken: &'static str,
}

/// Speed of a collision with a tree, a building or another aircraft beyond which the aircraft
/// breaks up, m/s (about 30 kt).
pub const COLLISION_BREAKUP: f64 = 15.0;

/// How a control surface is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SurfaceState {
    #[default]
    Working,
    /// Its cable or rod has parted, or its hinge is bent: it trails with the air and the pilot no
    /// longer moves it.
    Loose,
    /// Torn off.
    Gone,
}

/// Closing speed of the tail into the ground that buckles the tail cone, m/s. Behind a tricycle gear
/// it is not built to land on, and gives way at about the descent velocity the gear is built for
/// (FAR 23.473: 2.7 m/s, more with the give of the skid). Estimated. Its cables to the elevator and
/// the rudder go slack.
pub const TAIL_STRIKE: f64 = 3.0;

/// What has given way in the airframe. A new aircraft has nothing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Damage {
    /// Permanent set of the wings from loads past the limit load, 0 (none) to 1 (at the ultimate
    /// load): more drag, and the side that took more load lifts less.
    pub bent: f64,
    /// Which wing was bent more: 0 left, 1 right.
    pub bent_side: usize,
    /// Where each wing (left, right) has been torn across, as the distance of the tear from the
    /// centre line, m: what was outboard of it has gone. `Some(0.0)`: all of the wing has.
    pub wing_torn: [Option<f64>; 2],
    /// The same for each half of the tailplane (stabiliser and elevator).
    pub stabiliser_torn: [Option<f64>; 2],
    /// Share of the fin that has been crushed or torn away, 0...1.
    pub fin_lost: f64,
    /// The control surfaces: ailerons and flaps left and right, elevator, rudder.
    pub ailerons: [SurfaceState; 2],
    pub flaps: [SurfaceState; 2],
    pub elevator: SurfaceState,
    pub rudder: SurfaceState,
    /// Seconds the flaps have been loaded past what they take.
    pub flap_strain: f64,
    /// How far flutter has gone towards taking the next surface, 0...1.
    pub flutter: f64,
    /// The fuel tank in each wing has burst (the wing torn inboard of its outer end).
    pub tank_burst: [bool; 2],
    /// Each leg collapsed, in the order of `AircraftType::gear`.
    pub gear_broken: [bool; MOST_WHEELS],
    /// The propeller has hit something: the engine is stopped.
    pub propeller_bent: bool,
    /// Each jet engine that has failed (`AircraftType::jets` order): it burns nothing and windmills.
    pub engines_failed: [bool; MOST_ENGINES],
    /// In pieces: they are a `Wreck` of the world now, and the aircraft no longer moves.
    pub broken_up: bool,
    /// How much of what kept it afloat has filled with water, 0...1.
    pub flooded: f64,
    /// Gone under water.
    pub sunk: bool,
    /// The largest and smallest load factors felt.
    pub most_g: f64,
    pub least_g: f64,
    /// How many things have given way, and the last, in words for the player.
    pub events: u32,
    pub last: &'static str,
}

impl Damage {
    fn event(&mut self, what: &'static str) {
        self.events += 1;
        self.last = what;
    }

    /// Whether all of a wing (0 left, 1 right) has gone.
    pub fn wing_lost(&self, side: usize) -> bool {
        self.wing_torn[side].is_some_and(|y| y <= 0.0)
    }

    /// What is left on the aircraft of one of the pieces it breaks into: `None` if all of it has
    /// gone, or the planes it was torn across (`Cut`): a wing torn short, the tail without what was
    /// torn off its tailplane. The game draws the aircraft with them.
    pub fn left_of(&self, pieces: &Pieces, kind: PieceKind) -> Option<[Option<Cut>; 2]> {
        let sign = |side: usize| [-1.0, 1.0][side];
        match kind {
            PieceKind::Fuselage => Some([None; 2]),
            PieceKind::LeftWing | PieceKind::RightWing => {
                let side = usize::from(kind == PieceKind::RightWing);
                match self.wing_torn[side] {
                    None => Some([None; 2]),
                    Some(end) if end <= 0.0 => None,
                    Some(end) => Some([Some(pieces.tear(kind, sign(side), end, true)), None]),
                }
            }
            PieceKind::Tail => Some([0, 1].map(|side| self.stabiliser_torn[side].map(|end| pieces.tear(kind, sign(side), end.max(pieces.stabiliser.root), true)))),
        }
    }

    /// Where a wing (or half tailplane) on a side ends now, given where its tip is.
    fn end(torn: Option<f64>, tip: f64) -> f64 {
        torn.map_or(tip, |y| y.min(tip))
    }
}

/// Which type of aircraft: the key of its file (`assets/aircraft/<key>/aircraft.ron`). The types
/// there are are the fleet (`file::fleet`); a kind names one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AircraftKind(&'static str);

impl Default for AircraftKind {
    fn default() -> AircraftKind {
        AircraftKind::C172
    }
}

impl AircraftKind {
    /// Cessna 172 with the 160 hp engine: high wing, tricycle gear, fixed-pitch propeller. The default.
    pub const C172: AircraftKind = AircraftKind("c172");
    /// Extra 300L: two-seat aerobatic monoplane, low wing, tailwheel, 300 hp and a
    /// constant-speed propeller, ±10 g.
    pub const EXTRA300: AircraftKind = AircraftKind("extra300");

    /// Every kind of the fleet, in its order.
    pub fn all() -> impl Iterator<Item = AircraftKind> {
        file::fleet().iter().map(|t| t.kind)
    }

    /// Name for the player.
    pub fn name(self) -> &'static str {
        self.spec().name
    }

    /// Short name, for files and the command line: the key of its file.
    pub fn key(self) -> &'static str {
        self.0
    }

    /// The kind of the fleet a word names, by its key or one of its aliases: `c172`, `cessna`,
    /// `extra`, `extra300`, `300l`...
    pub fn parse(word: &str) -> Option<AircraftKind> {
        let w = word.to_ascii_lowercase().replace([' ', '-', '_'], "");
        let plain = |name: &str| name.replace('_', "") == w;
        file::fleet().iter().find(|t| plain(t.kind.0) || t.aliases.iter().any(|a| plain(a))).map(|t| t.kind)
    }

    /// Its type, as the fleet has it. Panics for a kind that is not in the fleet (a type read with
    /// `file::load` and never made part of it): fly that one with `Aircraft::with_type`.
    pub fn spec(self) -> &'static AircraftType {
        file::fleet().iter().find(|t| t.kind == self).unwrap_or_else(|| panic!("the aircraft {} is not in the fleet", self.0))
    }
}

/// The governor of a constant-speed propeller: it turns the blades between two stops to hold
/// one engine speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Governor {
    /// Propeller speed it holds, rad/s.
    pub speed: f64,
    /// Blade angle at 75% radius of the fine and coarse stops, radians.
    pub fine: f64,
    pub coarse: f64,
    /// Blade angle rate per rad/s of overspeed, 1/s, and per rad/s² of the propeller speeding up, s.
    pub gain: f64,
    pub damping: f64,
}

/// How the game draws a type: the names of its model and of its cockpit, which the game knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visual {
    pub model: &'static str,
    pub cockpit: &'static str,
}

/// Geometry, mass, aerodynamic and structural data of one kind of aircraft. Positions are body
/// axes from the centre of gravity of `reference_load`.
#[derive(Clone, Debug, PartialEq)]
pub struct AircraftType {
    pub kind: AircraftKind,
    /// Name for the player, and other words that name it on the command line.
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    /// How the game draws it.
    pub visual: Visual,
    /// Mass and principal moments of inertia with `reference_load` aboard.
    pub mass: f64,
    pub inertia: V3,
    pub wing_area: f64,
    pub span: f64,
    pub chord: f64,

    // Wing, as spanwise strips.
    /// Where the strips of a half wing meet, from the root out; the last strip ends at the tip.
    pub strip_edges: [f64; 4],
    /// How many strips of each half wing, from the root, carry flap, and how many, from the tip, aileron.
    pub flapped_strips: usize,
    pub aileron_strips: usize,
    pub wing_pos: V3,
    pub wing_slope: f64,
    pub wing_cl0: f64,
    pub stall_alpha: f64,
    pub stall_alpha_neg: f64,
    pub stall_sharpness: f64,
    /// Once the flow over a strip has separated it reattaches only this far below the angle
    /// it separated at (about 3° for a 2412-type section at these Reynolds numbers; JSBSim's
    /// c172p keeps a separate, lower lift curve for the stalled wing between 6° and 21°).
    pub stall_hysteresis: f64,
    /// Chords of travel the flow takes to separate or reattach.
    pub stall_lag_chords: f64,
    pub post_stall_lift: f64,
    pub wing_cd0: f64,
    pub induced_k: f64,
    /// Lift gained near the ground per unit of induced drag lost there.
    pub ground_lift: f64,
    pub wing_cm: f64,
    pub root_twist: f64,
    pub tip_twist: f64,

    // Flaps.
    /// Flap angle of each notch of the lever, degrees; one notch (0°) is no flaps.
    pub flap_notches: &'static [f64],
    /// Share of the full-flap lift and drag at the angle of each notch.
    pub flap_lift_curve: &'static [f64],
    pub flap_drag_curve: &'static [f64],
    pub flap_cl: f64,
    pub flap_stall_shift: f64,
    pub flap_drag: f64,
    pub flap_cm: f64,
    pub flap_downwash: f64,
    pub aileron_power: f64,
    pub aileron_max: f64,

    // Horizontal tail.
    pub tail_pos: V3,
    pub tail_area: f64,
    pub tail_slope: f64,
    pub tail_stall: f64,
    pub tail_incidence: f64,
    pub tail_efficiency: f64,
    pub elevator_tau: f64,
    pub elevator_up: f64,
    pub elevator_down: f64,
    pub trim_range: f64,
    pub downwash_per_cl: f64,
    pub tail_wash: f64,

    // Fin and rudder.
    pub fin_pos: V3,
    pub fin_area: f64,
    pub fin_slope: f64,
    pub fin_stall: f64,
    pub rudder_tau: f64,
    pub rudder_max: f64,
    pub fin_wash: f64,
    pub swirl_factor: f64,
    /// Fin offset and wing rigging that make it fly straight, hands off, at `rig_speed`.
    pub fin_rig: f64,
    pub roll_rig: f64,
    pub rig_speed: f64,

    // Fuselage, struts and gear drag (equivalent flat-plate areas) and body moments.
    pub body_drag_area: V3,
    pub body_roll_beta: f64,
    pub body_yaw_beta: f64,
    pub body_pitch_damping: f64,

    // Propeller and engine.
    pub prop_diameter: f64,
    pub prop_pos: V3,
    pub prop_inertia: f64,
    /// Thrust and power coefficients against advance ratio, at blade angle `prop_pitch`.
    pub prop_j: &'static [f64],
    pub prop_ct: &'static [f64],
    pub prop_cp: &'static [f64],
    /// Blade angle at 75% radius the coefficients are for, radians.
    pub prop_pitch: f64,
    /// `None` for a fixed-pitch propeller.
    pub governor: Option<Governor>,
    /// Cylinders of the engine (what it sounds like).
    pub cylinders: u32,
    pub gross_torque: f64,
    /// Engine speed of the largest torque, rpm.
    pub torque_rpm: f64,
    /// Friction torque, N·m: constant and per rpm; and the pumping loss per rpm with the throttle closed.
    pub friction: [f64; 2],
    pub pumping: f64,
    pub p_factor: f64,
    /// Strip theory overstates roll damping: the real wing loses lift towards the tips.
    pub roll_damping_relief: f64,
    pub torque_reaction: f64,
    /// Fuel burnt with the engine barely turning, kg/s, and per joule of shaft work, kg.
    pub fuel_idle: f64,
    pub fuel_per_joule: f64,

    // Landing gear.
    /// The wheels, as contact points with their legs: at most `MOST_WHEELS`. `FlightData::wheel_load`
    /// and `Damage::gear_broken` follow the same order.
    pub gear: Vec<Wheel>,
    /// How far a leg compresses before it bottoms out, m, and how far a steering wheel turns with full pedal.
    pub gear_travel: f64,
    pub steer_max: f64,
    /// Height of the centre of gravity over the ground, and pitch, standing on its wheels.
    pub cg_height: f64,
    pub ground_pitch: f64,

    /// Points of the airframe that must not touch the ground: wing tips, propeller tip, top of
    /// the cabin, top of the fin, belly. The last one is the tail skid, which may.
    pub hard_points: [V3; 7],

    // Structure.
    /// Limit load factors, g: flaps up, positive and negative, and flaps down, positive.
    pub load_limit: f64,
    pub load_limit_negative: f64,
    pub load_limit_flaps: f64,
    /// Ultimate load as a multiple of the limit load: the airframe gives way there (FAR 23.303).
    pub ultimate_factor: f64,
    /// Never-exceed speed, and the indicated airspeed flutter of the control surfaces starts at.
    pub vne: f64,
    pub flutter_speed: f64,
    /// Highest indicated airspeeds with the flaps at 10° and beyond 10°.
    pub vfe: [f64; 2],
    /// Speed of a point of the airframe into the ground (or into water) beyond which the aircraft
    /// breaks up, m/s.
    pub breakup_speed: f64,
    /// Share of the wing's chord that is aileron and that is flap, of the tailplane's area that is
    /// elevator and of the fin's that is rudder: what goes with each when it is torn off.
    pub aileron_chord: f64,
    pub flap_chord: f64,
    pub elevator_share: f64,
    pub rudder_share: f64,
    /// Where the fuel tanks in the wings end, from the centre line, m (`None`: there are none in
    /// the wings): a wing torn inboard of it bursts its tank.
    pub wing_tanks: Option<f64>,
    /// Force that crushes the wing against a post, a wall or the ground, N: past it the wing is cut
    /// where it strikes. The tailplane takes half of it.
    pub wing_crush: f64,
    /// What keeps it afloat and what drags in water: body point, volume (m³), height over which
    /// it goes under (m), and area against the water (m²). Wings at 3 and 4, wheels from 5 on.
    pub floats: [(V3, f64, f64, f64); 5 + MOST_WHEELS],
    /// The solid of the aircraft as boxes, lower and upper corners: what another aircraft hits.
    pub airframe: [(V3, V3); 5],
    /// The pieces it breaks into.
    pub pieces: &'static Pieces,

    // Load.
    /// Body position of each item of `Load` and the half-width it is spread over.
    pub stations: [(V3, f64); 4],
    /// The loading every figure above is given for.
    pub reference_load: Load,

    // Where the world places it.
    /// True airspeed of a start in the air; and on final, the speed and the flap notch.
    pub air_start_speed: f64,
    pub final_speed: f64,
    pub final_flap_notch: i32,
    /// The flap notch it takes off with, and the runway it needs, m (0: any will do): the world
    /// starts it at an aerodrome that has one (`Start::Home`), and the autopilot lands it at one.
    pub takeoff_flap_notch: i32,
    pub runway: f64,

    /// The indicated airspeeds it is flown at, for the autopilot.
    pub speeds: Speeds,

    /// The instruments it has and how its pitot-static system errs.
    pub instruments: InstrumentFit,

    /// The autopilot it has: the C172's Garmin GFC 700 with its servos; the Extra none.
    pub afcs: Option<Gfc700Fit>,

    // What a jet transport has and a light aircraft has not. Without them (the defaults) the
    // model is the light aircraft's, number for number.
    /// Jet engines, in place of the piston engine and its propeller (whose fields then say nothing).
    pub jets: Option<Jets>,
    /// Sweep of the wing's quarter-chord line, radians: the outer strips sit further aft, and in
    /// a sideslip the wing that leads meets the air squarer and lifts more.
    pub sweep: f64,
    /// What changes as the flow nears the speed of sound.
    pub mach: Option<Compressibility>,
    pub spoilers: Option<Spoilers>,
    /// A gear that retracts; `None` for a fixed one.
    pub retract: Option<Retract>,
    /// Trim moves the whole tailplane, not the elevator: how far it turns nose up (its leading
    /// edge down) and nose down from `tail_incidence`, radians.
    pub stabiliser_trim: Option<(f64, f64)>,
    /// A yaw damper: rudder (as a share of its travel) per rad/s of yaw rate away from the turn's.
    pub yaw_damper: Option<f64>,
    /// How fast the flaps travel, degrees a second.
    pub flap_rate: f64,
    /// Highest indicated airspeed with the flaps at each notch from the first out; empty where
    /// `vfe` says it.
    pub flap_speeds: &'static [f64],
    /// Share of `aileron_power` there is with the flaps up, when the outer ailerons are locked.
    pub aileron_clean: f64,
    /// Spring (N/m) and damping (N·s/m) of a point of the airframe pressed into the ground.
    pub hard_spring: (f64, f64),
    /// How much of a light aircraft's braking grip its brakes reach (the anti-skid of a heavy one
    /// keeps well below what a tyre can give).
    pub braking: f64,
}

/// What compressibility does to a wing: the lift slope grows with Mach as a swept wing's does
/// (from its aspect ratio and sweep), and with it these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compressibility {
    /// Mach of drag divergence at no lift, and how much lower it is per unit of lift coefficient
    /// (Korn's relation); the wave drag grows from below it as Lock's fourth power.
    pub divergence: (f64, f64),
    /// Mach from which the wing's lift moves aft (the nose tucks), and chords it moves per unit of Mach.
    pub tuck: (f64, f64),
    /// Share of the low-speed lift at the stall that the wing still reaches, by Mach: shocks
    /// separate the flow sooner (the buffet boundary).
    pub lift_limit_mach: &'static [f64],
    pub lift_limit: &'static [f64],
    /// The highest Mach it may be flown at.
    pub mmo: f64,
}

/// Spoilers on the wing: panels that rise from its top, dump lift and drag. The lever raises them
/// all (speed brakes); the ailerons raise those of the wing that goes down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spoilers {
    /// Which strips of a half wing carry them: from this one (0 is the root) up to, not including, that one.
    pub strips: (usize, usize),
    /// Lift coefficient those strips lose and drag coefficient (of the whole wing's area) they add, fully up.
    pub lift: f64,
    pub drag: f64,
    /// How far the lever raises them in the air, 0...1: the flight detent.
    pub flight_limit: f64,
    /// How far full aileron raises them on the wing going down, 0...1.
    pub roll: f64,
    /// Seconds from down to fully up.
    pub time: f64,
}

/// A gear that retracts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retract {
    /// Seconds it takes to go up or come down.
    pub time: f64,
    /// Its drag when down, as a flat plate's area, m².
    pub drag_area: f64,
    /// Highest indicated airspeed with it down, m/s.
    pub limit_speed: f64,
}

/// The indicated airspeeds an aircraft is flown at, m/s: what the autopilot holds in each part of
/// a flight, and the limits it keeps to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speeds {
    /// Stall speed with the flaps up and with them fully down.
    pub stall_clean: f64,
    pub stall_full: f64,
    /// The slowest the game's autopilot holds when the player asks for less.
    pub least: f64,
    /// Never commanded and never held above this.
    pub most: f64,
    /// Flaps are not lowered above this.
    pub flaps: f64,
    pub rotate: f64,
    pub climb: f64,
    pub pattern: f64,
    pub descent: f64,
    pub base: f64,
    pub final_approach: f64,
    pub join: f64,
    /// Flown along a route.
    pub cruise: f64,
}

impl Default for AircraftType {
    fn default() -> AircraftType {
        AircraftType::c172()
    }
}

impl AircraftType {
    /// The Cessna 172 of the fleet (`assets/aircraft/c172/aircraft.ron`), at its maximum weight. Its
    /// figures are checked against the handbook and JSBSim's c172p in `tests/flight_model.rs` and
    /// `tests/jsbsim_reference.rs`.
    pub fn c172() -> AircraftType {
        AircraftKind::C172.spec().clone()
    }

    /// The Extra 300L of the fleet (`assets/aircraft/extra300/aircraft.ron`), flown solo from the rear
    /// seat; its figures against the published ones in `tests/extra300.rs`.
    pub fn extra300() -> AircraftType {
        AircraftKind::EXTRA300.spec().clone()
    }

    /// Whether the steerable wheel is behind the main wheels.
    pub fn tailwheel(&self) -> bool {
        self.gear.iter().any(|w| w.steers && w.contact.x < 0.0)
    }

    /// How far `wheel` is turned, radians, its front to the right positive, with the pedals at
    /// `rudder` (-1...1, right positive) and the aircraft rolling at `speed` m/s along its axis. A
    /// steering wheel is linked to the pedals through springs, so it follows them less the faster it
    /// rolls; right pedal turns a nose wheel's front to the right and a tailwheel's to the left, both
    /// turning the aircraft right. The others do not turn.
    pub fn steer_angle(&self, wheel: &Wheel, rudder: f64, speed: f64) -> f64 {
        if !wheel.steers {
            return 0.0;
        }
        let behind = if wheel.contact.x < 0.0 { -1.0 } else { 1.0 };
        behind * clamp(rudder, -1.0, 1.0) * self.steer_max / (1.0 + (speed / 12.0) * (speed / 12.0))
    }

    /// The highest flap notch.
    pub fn max_flap_notch(&self) -> i32 {
        self.flap_notches.len() as i32 - 1
    }
}

/// What the aircraft carries, in kilograms. The airframe's own mass, centre of gravity and
/// inertia are those of its `AircraftType` for `AircraftType::reference_load`; any other load
/// moves all three.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Load {
    /// Front seats: pilot and passenger of the C172, the passenger of the Extra.
    pub front: f64,
    /// Rear seat: the pilot of the Extra.
    pub rear: f64,
    pub baggage: f64,
    /// Usable fuel; the engine burns it and stops when it runs out.
    pub fuel: f64,
}

impl Load {
    fn items(&self) -> [f64; 4] {
        [self.front, self.rear, self.baggage, self.fuel]
    }

    pub fn total(&self) -> f64 {
        self.front + self.rear + self.baggage + self.fuel
    }
}

/// One spanwise strip of the wing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WingStrip {
    pub y: f64,
    pub weight: f64,
    pub twist: f64,
    pub flapped: bool,
    pub aileron: bool,
    /// The innermost strip of its half wing, where the stall warning feels the flow.
    pub root: bool,
    /// It carries spoilers.
    pub spoiler: bool,
    /// How far forward of the wing's position it is (aft, negative, for the outer strips of a swept wing).
    pub x: f64,
}

/// A point of the airframe on the ground.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Touch {
    /// How hard the ground pushes on it, N; how fast it moves into the ground, and how fast at all, m/s.
    pub load: f64,
    pub closing: f64,
    pub speed: f64,
}

/// Everything the force model reports besides the accelerations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlightData {
    /// Total force and moment in body axes, gravity included.
    pub force: V3,
    pub moment: V3,
    pub prop_accel: f64,
    /// Fuel burnt, kg/s.
    pub fuel_flow: f64,
    /// Non-gravitational acceleration in body axes (what an accelerometer reads).
    pub specific: V3,
    pub alpha: f64,
    pub beta: f64,
    pub tas: f64,
    pub ias: f64,
    pub mach: f64,
    /// Thrust of the propeller, or of all the jet engines together, N.
    pub thrust: f64,
    /// Net thrust of each jet engine, N (`AircraftType::jets` order).
    pub jet_thrust: [f64; MOST_ENGINES],
    pub engine_torque: f64,
    pub downwash_cl: f64,
    pub stall_warning: bool,
    /// Share of the wing that is stalled, 0...1.
    pub stalled: f64,
    /// The same for each strip of the wing, left tip first.
    pub strip_stalled: [f64; 8],
    /// Normal force on each wheel, N, in the order of `AircraftType::gear`.
    pub wheel_load: [f64; MOST_WHEELS],
    /// How far each touching wheel is pressed into the ground, m: its leg's compression (more than
    /// `AircraftType::gear_travel` once it has bottomed out); 0 off the ground.
    pub wheel_compression: [f64; MOST_WHEELS],
    /// Speed of each touching wheel over the ground along its heading, m/s, forward positive: what it
    /// rolls at, unless braked to a skid; 0 off the ground.
    pub wheel_roll: [f64; MOST_WHEELS],
    /// Index in `AircraftType::hard_points` of a part of the airframe that touches the ground.
    pub hard_contact: Option<usize>,
    pub hard_impact: f64,
    /// The fastest a touching point of the airframe moves into the ground, m/s.
    pub hard_sink: f64,
    /// Each of `AircraftType::hard_points` that touches the ground (a wing tip where the wing ends
    /// now), and after them the nacelle of each jet engine.
    pub touches: [Option<Touch>; MOST_TOUCHES],
    pub in_water: bool,
    /// How fast the aircraft sinks into the water while touching it, m/s.
    pub water_sink: f64,
    /// Load factor of the air forces alone, g: what the wings carry.
    pub aero_load: f64,
    /// Lift of the left and right wing, N.
    pub wing_lift: [f64; 2],
    /// Faster than the never-exceed speed; faster than the flaps may be out at.
    pub overspeed: bool,
    pub flap_overspeed: bool,
}

impl Default for FlightData {
    fn default() -> FlightData {
        FlightData {
            force: V3::ZERO,
            moment: V3::ZERO,
            prop_accel: 0.0,
            fuel_flow: 0.0,
            specific: V3::ZERO,
            alpha: 0.0,
            beta: 0.0,
            tas: 0.0,
            ias: 0.0,
            mach: 0.0,
            thrust: 0.0,
            jet_thrust: [0.0; MOST_ENGINES],
            engine_torque: 0.0,
            downwash_cl: 0.0,
            stall_warning: false,
            stalled: 0.0,
            strip_stalled: [0.0; 8],
            wheel_load: [0.0; MOST_WHEELS],
            wheel_compression: [0.0; MOST_WHEELS],
            wheel_roll: [0.0; MOST_WHEELS],
            hard_contact: None,
            hard_impact: 0.0,
            hard_sink: 0.0,
            touches: [None; MOST_TOUCHES],
            in_water: false,
            water_sink: 0.0,
            aero_load: 0.0,
            wing_lift: [0.0; 2],
            overspeed: false,
            flap_overspeed: false,
        }
    }
}

impl FlightData {
    pub fn load_factor(&self) -> f64 {
        -self.specific.z / GRAVITY
    }

    pub fn on_ground(&self) -> bool {
        self.wheel_load.iter().sum::<f64>() > 1.0
    }
}

/// A steady flight condition found by `Aircraft::trim`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trim {
    pub alpha: f64,
    /// The yoke, and the trim: one of them carries the tail's load and the other is 0 (the trim
    /// of a type whose trim moves the tailplane, `AircraftType::stabiliser_trim`; else the yoke).
    pub elevator: f64,
    pub trim: f64,
    pub throttle: f64,
    pub climb_angle: f64,
    pub prop_speed: f64,
    pub blade_pitch: f64,
    pub n1: [f64; MOST_ENGINES],
}

/// Result of `Aircraft::stall`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stall {
    pub cl_max: f64,
    /// m/s.
    pub speed: f64,
}

/// Lift of one strip, kept between the two passes over the wing.
#[derive(Clone, Copy, Default)]
struct StripLift {
    r: V3,
    v2: f64,
    flow: f64,
    a: f64,
    cl: f64,
    sigma: f64,
    /// How far up its spoilers are, 0...1.
    spoil: f64,
}

struct Derivative {
    body: Rates,
    prop: f64,
}

#[derive(Clone, Debug)]
pub struct Aircraft {
    pub p: AircraftType,
    pub state: State,
    pub controls: AircraftControls,
    /// Actual flap deflection, degrees; it follows the selected notch at the motor's speed.
    pub flap_deg: f64,
    /// Where the gear is: 1 down and locked, 0 up. Always 1 for a fixed one.
    pub gear: f64,
    /// How far up the spoilers are as speed brakes, 0...1.
    pub spoilers: f64,
    /// How far open the reverser of each jet engine is, 0...1.
    pub reversers: [f64; MOST_ENGINES],
    /// Downwash angle at the tail, lagging the wing by the time the air takes to get there.
    pub downwash: f64,
    /// Whether the engine uses up the fuel it burns (true by default, as in the aircraft). A game
    /// may turn it off so a long flight never runs dry; the fuel flow is still worked out and
    /// shown, and a crash still spills what is aboard.
    pub burns_fuel: bool,
    data: FlightData,
    crashed: Option<&'static str>,
    damage: Damage,
    /// Pieces that have come off and not yet been handed to the world.
    wrecks: Vec<Wreck>,

    strips: [WingStrip; 8],
    /// Share left of each half wing's lift (its strips' weights against a whole one's), and of the
    /// lift of its flaps, left and right: 1 on a whole aircraft.
    wing_left: [f64; 2],
    flaps_left: [f64; 2],
    /// Share of the whole wing that carries flap.
    flapped_share: f64,
    load: Load,
    mass: f64,
    cg: V3,
    inertia: V3,
    /// How separated the flow over each strip has been lately, 0...1: the stall's memory.
    separation: [f64; 8],
    aileron_gain: f64,
    /// What its dials and displays show, from its pitot-static system, gyros and compass; its
    /// switches and failures. The world steps them after the flight model and sets them at every
    /// placement as after taxiing out.
    pub instruments: Instruments,
    /// The air data an ideal sensor gives, with a lag (`indicated_airspeed()`...).
    gauges: Gauges,
}

impl Default for Aircraft {
    fn default() -> Aircraft {
        Aircraft::new()
    }
}

/// The strips of the wing of a type with `damage`, left tip first, and the share of each half wing's
/// lift and of its flaps' that is left. A half wing torn across ends at the tear, and its lift falls
/// to nothing at that new tip as it did at the old one (the elliptical loading over the shorter half
/// wing); a strip that has lost its aileron or flap has lost that part of its chord.
fn wing_strips(p: &AircraftType, damage: &Damage) -> ([WingStrip; 8], [f64; 2], [f64; 2]) {
    let half = p.span / 2.0;
    let e = p.strip_edges;
    let edges = [e[0], e[1], e[2], e[3], half];
    // (y, weight) of the four strips of a half wing.
    let mut raw = [(0.0, 0.0); 4];
    for i in 0..4 {
        let y = (edges[i] + edges[i + 1]) / 2.0;
        // Elliptical loading; the innermost strip also carries the fuselage's share.
        let from = if i == 0 { 0.0 } else { edges[i] };
        raw[i] = (
            y,
            (edges[i + 1] - from) * (1.0 - (y / half) * (y / half)).sqrt(),
        );
    }
    let total = 2.0 * raw.iter().fold(0.0, |s, r| s + r.1);
    // A swept wing's strips lie along its quarter-chord line, about the station its lift acts at.
    let mean_y = 2.0 * raw.iter().fold(0.0, |s, r| s + r.0 * r.1) / total;
    let sweep = p.sweep.tan();
    let spoilers = p.spoilers.map_or((0, 0), |sp| sp.strips);
    let mut strips = [WingStrip::default(); 8];
    let (mut wing_left, mut flaps_left) = ([1.0; 2], [1.0; 2]);
    let mut k = 0;
    for (side_index, side) in [-1.0, 1.0].into_iter().enumerate() {
        let torn = damage.wing_torn[side_index];
        let flap_gone = damage.flaps[side_index] == SurfaceState::Gone;
        let aileron_gone = damage.ailerons[side_index] == SurfaceState::Gone;
        let (mut kept, mut flapped, mut flapped_whole) = (0.0, 0.0, 0.0);
        for (i, r) in raw.iter().enumerate() {
            let t = (r.0 - raw[0].0) / (raw[3].0 - raw[0].0);
            let (flap, aileron) = (i < p.flapped_strips, i >= 4 - p.aileron_strips);
            let (y, mut weight) = match torn {
                None => (r.0, r.1 / total),
                Some(end) => {
                    let outer = edges[i + 1].min(end);
                    if outer <= edges[i] {
                        (r.0, 0.0)
                    } else {
                        let y = (edges[i] + outer) / 2.0;
                        let from = if i == 0 { 0.0 } else { edges[i] };
                        (y, (outer - from) * (1.0 - (y / end) * (y / end)).max(0.0).sqrt() / total)
                    }
                }
            };
            if flap && flap_gone {
                weight *= 1.0 - p.flap_chord;
            }
            if aileron && aileron_gone {
                weight *= 1.0 - p.aileron_chord;
            }
            kept += weight;
            if flap {
                flapped += weight;
                flapped_whole += r.1 / total;
            }
            strips[k] = WingStrip {
                y: side * y,
                weight,
                twist: lerp(p.root_twist, p.tip_twist, t),
                flapped: flap,
                aileron,
                root: i == 0,
                spoiler: (spoilers.0..spoilers.1).contains(&i),
                x: if sweep == 0.0 { 0.0 } else { -(r.0 - mean_y) * sweep },
            };
            k += 1;
        }
        if torn.is_some() || flap_gone || aileron_gone {
            wing_left[side_index] = kept / (raw.iter().fold(0.0, |s, r| s + r.1) / total);
        }
        if damage.flaps[side_index] != SurfaceState::Working {
            flaps_left[side_index] = 0.0;
        } else if torn.is_some() && flapped_whole > 0.0 {
            flaps_left[side_index] = flapped / flapped_whole;
        }
    }
    (strips, wing_left, flaps_left)
}

impl Aircraft {
    /// A Cessna 172 (`AircraftType::c172()`).
    pub fn new() -> Aircraft {
        Aircraft::with_type(AircraftType::c172())
    }

    /// An aircraft of the given kind.
    pub fn of(kind: AircraftKind) -> Aircraft {
        Aircraft::with_type(kind.spec().clone())
    }

    /// An aircraft of the given type, with its reference load aboard and rigged.
    pub fn with_type(p: AircraftType) -> Aircraft {
        let (strips, wing_left, flaps_left) = wing_strips(&p, &Damage::default());
        let arm = strips
            .iter()
            .filter(|s| s.aileron)
            .fold(0.0, |sum, s| sum + s.weight * s.y.abs());
        let aileron_gain = p.aileron_power * p.span / arm;
        let flapped_share = strips
            .iter()
            .filter(|st| st.flapped)
            .fold(0.0, |sum, st| sum + st.weight);
        let load = p.reference_load;
        let mut a = Aircraft {
            p,
            state: State::default(),
            controls: AircraftControls::default(),
            flap_deg: 0.0,
            gear: 1.0,
            spoilers: 0.0,
            reversers: [0.0; MOST_ENGINES],
            downwash: 0.0,
            data: FlightData::default(),
            crashed: None,
            damage: Damage::default(),
            wrecks: Vec::new(),
            strips,
            wing_left,
            flaps_left,
            flapped_share,
            aileron_gain,
            burns_fuel: true,
            load,
            mass: 0.0,
            cg: V3::ZERO,
            inertia: V3::ZERO,
            separation: [0.0; 8],
            instruments: Instruments::new(0),
            gauges: Gauges::default(),
        };
        a.set_load(load);
        a.rig();
        a
    }

    /// What the force model reported at the start of the last step.
    pub fn data(&self) -> &FlightData {
        &self.data
    }

    /// Why the flight is over, if it is: crashed, broken or ditched (`damage` says what is left).
    pub fn crashed(&self) -> Option<&'static str> {
        self.crashed
    }

    /// The spanwise strips the wing is computed with, left tip first.
    pub fn strips(&self) -> &[WingStrip] {
        &self.strips
    }

    /// Lift coefficient change of the outer strips per radian of aileron.
    pub fn aileron_gain(&self) -> f64 {
        self.aileron_gain
    }

    /// Sets the fin offset and wing rigging so the engine torque and slipstream are balanced in cruise,
    /// as the real aircraft is rigged at the factory.
    fn rig(&mut self) {
        let tas = self.p.rig_speed;
        let altitude = 1500.0;
        let Some(t) = self.trim(tas, altitude, 0.0, None, 0.0) else {
            return;
        };
        let lateral = |a: &Aircraft| {
            a.steady(tas, altitude, t.alpha, 0.0, t.elevator, t.throttle, 0.0)
                .0
        };
        for _ in 0..3 {
            let n0 = lateral(self).moment.z;
            self.p.fin_rig += 0.01;
            let n1 = lateral(self).moment.z;
            self.p.fin_rig -= 0.01;
            if (n1 - n0).abs() > 1e-9 {
                self.p.fin_rig -= n0 * 0.01 / (n1 - n0);
            }
            let q = 0.5 * atmosphere::density(altitude) * tas * tas;
            self.p.roll_rig -= lateral(self).moment.x / (q * self.p.wing_area * self.p.span);
        }
    }

    // MARK: Engine and propeller

    /// Shaft torque, N·m, at a throttle setting, propeller speed (rad/s) and air density.
    pub fn engine_torque(&self, throttle: f64, prop_speed: f64, rho: f64) -> f64 {
        let rpm = prop_speed * 60.0 / (2.0 * PI);
        let altitude = (1.132 * rho / atmosphere::SEA_LEVEL_DENSITY - 0.132).max(0.0);
        let x = (rpm - self.p.torque_rpm) / self.p.torque_rpm;
        let shape = 1.0 - 0.3 * x * x;
        let thr = throttle;
        // With the throttle closed the engine only gets its idle air, so its torque falls as it is spun
        // faster, and pumping against the closed plate brakes a windmilling propeller.
        let friction = self.p.friction[0] + self.p.friction[1] * rpm;
        let full = (self.p.gross_torque * shape - friction) * altitude;
        let closed =
            0.14 * self.p.gross_torque * 650.0 / rpm.max(300.0) * altitude - friction - self.p.pumping * rpm;
        lerp(closed, full, thr)
    }

    /// Engine speed, rpm: the propeller's, or the fan's of the fastest jet engine.
    pub fn rpm(&self) -> f64 {
        match &self.p.jets {
            Some(jets) => jets.rpm(self.state.n1.iter().copied().fold(0.0, f64::max)),
            None => self.state.prop_speed * 60.0 / (2.0 * PI),
        }
    }

    /// The air a jet engine breathes in state `s`.
    fn intake(&self, s: &State, env: &Environment) -> Intake {
        let v_air = s.vel - act(inverse(s.q), env.air_velocity);
        Intake { pressure: env.pressure(), temperature: env.temperature, speed: v_air.x }
    }

    /// Whether a jet engine burns: it has not failed and there is fuel.
    fn burning(&self, engine: usize) -> bool {
        !self.damage.engines_failed[engine] && self.load.fuel > 0.0
    }

    /// N1 of each jet engine once it has settled at a throttle setting in the air of `env`, flying.
    fn settled_n1(&self, s: &State, throttle: f64, env: &Environment) -> [f64; MOST_ENGINES] {
        let mut n1 = [0.0; MOST_ENGINES];
        if let Some(jets) = &self.p.jets {
            let intake = self.intake(s, env);
            for (i, n) in n1.iter_mut().enumerate().take(jets.mounts.len()) {
                *n = if self.burning(i) { jets.command(throttle, &intake, true, false) } else { jets.windmill(&intake) };
            }
        }
        n1
    }

    /// A jet engine fails (`AircraftType::jets` order): it flames out and windmills.
    pub fn fail_engine(&mut self, engine: usize) {
        if engine < MOST_ENGINES && !self.damage.engines_failed[engine] {
            self.damage.engines_failed[engine] = true;
            self.damage.event(["Engine 1 failed", "Engine 2 failed", "Engine 3 failed", "Engine 4 failed"][engine]);
        }
    }

    /// A failed jet engine is lit again.
    pub fn relight_engine(&mut self, engine: usize) {
        if engine < MOST_ENGINES {
            self.damage.engines_failed[engine] = false;
        }
    }

    /// Highest indicated airspeed with the flaps at `flap_deg`, m/s.
    pub fn flap_limit(&self, flap_deg: f64) -> f64 {
        let p = &self.p;
        if p.flap_speeds.is_empty() {
            return if flap_deg <= 10.5 { p.vfe[0] } else { p.vfe[1] };
        }
        // The speed of the first notch the flaps have not gone past.
        let notch = p.flap_notches[1..].iter().position(|&deg| flap_deg <= deg + 0.5).unwrap_or(p.flap_speeds.len() - 1);
        p.flap_speeds[notch.min(p.flap_speeds.len() - 1)]
    }

    // MARK: Forces

    /// How much steeper the lift curve of the wing is at a Mach number than at none: a swept wing
    /// of this aspect ratio (the DATCOM's formula, Helmbold's with the Prandtl-Glauert factor).
    fn compressibility(&self, mach: f64) -> f64 {
        let aspect = self.p.span * self.p.span / self.p.wing_area;
        let sweep = self.p.sweep.tan();
        let slope = |m: f64| {
            let beta2 = 1.0 - m * m;
            1.0 / (2.0 + (4.0 + aspect * aspect * (beta2 + sweep * sweep)).sqrt())
        };
        slope(mach.min(0.95)) / slope(0.0)
    }

    /// Lift coefficient and stalled fraction of a surface at angle of attack `a`.
    #[inline(always)]
    fn lift_curve(
        &self,
        a: f64,
        slope: f64,
        cl0: f64,
        stall_pos: f64,
        stall_neg: f64,
    ) -> (f64, f64) {
        let k = self.p.stall_sharpness;
        let s_pos = 1.0 / (1.0 + (-k * (a - stall_pos)).exp());
        let s_neg = 1.0 / (1.0 + (k * (a + stall_neg)).exp());
        let sigma = (s_pos + s_neg).min(1.0);
        let flat = 2.0 * a.sin() * a.cos() * self.p.post_stall_lift;
        ((1.0 - sigma) * (cl0 + slope * a) + sigma * flat, sigma)
    }

    /// All forces and moments on the aircraft in state `s` with controls `c`. `downwash` is the
    /// downwash angle at the tail and `flap_deg` the actual flap deflection.
    pub fn forces(
        &self,
        s: &State,
        c: &AircraftControls,
        env: &Environment,
        downwash: f64,
        flap_deg: f64,
    ) -> FlightData {
        self.forces_separated(s, c, env, downwash, flap_deg, &[0.0; 8])
    }

    /// `forces` for a wing whose strips have been stalled lately by `separation` (0...1 each):
    /// they stay stalled down to a lower angle of attack.
    #[allow(clippy::too_many_arguments)]
    pub fn forces_separated(
        &self,
        s: &State,
        c: &AircraftControls,
        env: &Environment,
        downwash: f64,
        flap_deg: f64,
        separation: &[f64; 8],
    ) -> FlightData {
        let p = &self.p;
        let eps = downwash;
        let mut out = FlightData::default();
        let q_inv = inverse(s.q);
        let v_air = s.vel - act(q_inv, env.air_velocity);
        let v = v_air.length().max(0.1);
        let rho = env.rho;
        let alpha = v_air.z.atan2(v_air.x.max(1e-3));
        let beta = clamp(v_air.y / v, -1.0, 1.0).asin();
        let qbar = 0.5 * rho * v * v;
        let area = p.wing_area;
        let mut f_sum = V3::ZERO;
        let mut m_sum = V3::ZERO;

        // Ground effect: the ground weakens the trailing vortices. Wieselsberger's factor on the induced
        // drag, which is also what JSBSim's c172p tabulates as kCDge.
        let altitude = -s.pos.z;
        let wing_height = (altitude - env.height(s.pos.x, s.pos.y) - p.wing_pos.z).max(0.3);
        let hb = wing_height / p.span;
        let ge = (1.0 - (1.0 - 1.32 * hb) / (1.05 + 7.4 * hb)).min(1.0);

        // Mach number, and what it does to the lift: its slope, how far it goes, where it acts.
        let mach = v / env.sound_speed();
        let (slope_gain, lift_limit, tuck) = match &p.mach {
            Some(m) => (
                self.compressibility(mach),
                interpolate(m.lift_limit_mach, m.lift_limit, mach),
                m.tuck.1 * (mach - m.tuck.0).max(0.0),
            ),
            None => (1.0, 1.0, 0.0),
        };

        // Engines: jets, each pushing where it hangs, or the propeller and its slipstream.
        let (wash, swirl) = if let Some(jets) = &p.jets {
            let intake = Intake { pressure: env.pressure(), temperature: env.temperature, speed: v_air.x };
            for (i, mount) in jets.mounts.iter().enumerate() {
                let run = jets.run(s.n1[i], &intake, self.reversers[i], self.burning(i));
                let f = V3::new(run.thrust, 0.0, 0.0);
                f_sum += f;
                m_sum += (mount.at - self.cg).cross(f);
                out.jet_thrust[i] = run.thrust;
                out.thrust += run.thrust;
                out.fuel_flow += run.fuel_flow;
            }
            (0.0, 0.0)
        } else {
            let n = (s.prop_speed / (2.0 * PI)).max(1.0);
            let d = p.prop_diameter;
            let up = v_air.x.max(0.0);
            let j = up / (n * d);
            // A coarser blade meets the air at the angle a finer one does at a lower advance ratio:
            // the coefficients slide along J by the change in the blade's helix at 75% radius.
            let j = match p.governor {
                Some(g) => {
                    let pitch = clamp(s.blade_pitch, g.fine, g.coarse);
                    j - 0.75 * PI * (pitch.tan() - p.prop_pitch.tan())
                }
                None => j,
            };
            let thrust = interpolate(p.prop_j, p.prop_ct, j) * rho * n * n * d * d * d * d
                * if self.damage.propeller_bent { 0.0 } else { 1.0 };
            let prop_torque =
                interpolate(p.prop_j, p.prop_cp, j) * rho * n * n * d * d * d * d * d
                    / (2.0 * PI);
            let disc = PI * d * d / 4.0;
            let wake2 = up * up + 2.0 * thrust / (rho * disc);
            let wake = if wake2 > 0.0 { wake2.sqrt() } else { 0.0 };
            let wash = wake - up;
            let mass_flow = (rho * disc * (up + wake) / 2.0).max(5.0);
            let swirl = prop_torque / (mass_flow * 0.6);
            // Out of fuel the engine is only turned by the propeller; a bent propeller has stopped it.
            let bent_prop = self.damage.propeller_bent;
            let q_engine = if bent_prop {
                prop_torque - s.prop_speed * p.prop_inertia * 4.0
            } else if self.load.fuel > 0.0 {
                self.engine_torque(c.throttle, s.prop_speed, rho * (1.0 - env.vapour))
            } else {
                self.engine_torque(0.0, s.prop_speed, 0.0)
            };
            out.fuel_flow = if bent_prop { 0.0 } else { self.fuel_flow(q_engine * s.prop_speed) };

            let moving = smoothstep(4.0, 15.0, v);
            let pf_y = p.p_factor * d / 2.0 * alpha.sin() * moving;
            let r_prop = V3::new(p.prop_pos.x, pf_y, p.prop_pos.z) - self.cg;
            let f_prop = V3::new(thrust, 0.0, 0.0);
            f_sum += f_prop;
            m_sum += r_prop.cross(f_prop);
            // The airframe straightens the rotating slipstream and so gets back most of the engine torque.
            if !bent_prop {
                m_sum.x -= q_engine * p.torque_reaction;
            }
            let h = p.prop_inertia * s.prop_speed;
            m_sum.y -= s.omega.z * h;
            m_sum.z += s.omega.y * h;
            out.prop_accel = (q_engine - prop_torque) / p.prop_inertia;
            out.thrust = thrust;
            out.engine_torque = q_engine;
            (wash, swirl)
        };

        // Wing. First the lift of every strip, then the forces: induced drag depends on the lift of
        // the whole wing, not of each strip alone.
        let fl = interpolate(p.flap_notches, p.flap_lift_curve, flap_deg);
        let fd = interpolate(p.flap_notches, p.flap_drag_curve, flap_deg);
        let flapped_share = self.flapped_share;
        let dmg = &self.damage;
        // An aileron the pilot no longer moves trails with the air.
        // With the flaps up the outer ailerons are locked: less roll power.
        let unlocked = if p.aileron_clean < 1.0 && p.flap_notches.len() > 1 {
            lerp(p.aileron_clean, 1.0, (flap_deg / p.flap_notches[1]).clamp(0.0, 1.0))
        } else {
            1.0
        };
        let da = dmg.ailerons.map(|a| if a == SurfaceState::Working { c.aileron * p.aileron_max * unlocked } else { 0.0 });
        let spoiler_share = if p.spoilers.is_some() { self.strips.iter().filter(|st| st.spoiler).fold(0.0, |sum, st| sum + st.weight) } else { 0.0 };
        let (sweep_sin, sweep_cos) = p.sweep.sin_cos();
        let (mut stalled, mut wing_cl, mut flap_lift) = (0.0, 0.0, 0.0);
        let mut lifts = [StripLift::default(); 8];
        for ((st, lift), sep) in self.strips.iter().zip(lifts.iter_mut()).zip(separation) {
            let r = V3::new(p.wing_pos.x + st.x, st.y, p.wing_pos.z) - self.cg;
            let side = usize::from(st.y > 0.0);
            // What has been torn off lifts nothing.
            if st.weight == 0.0 {
                *lift = StripLift { r, ..StripLift::default() };
                continue;
            }
            // A flap torn off, or loose, is blown back up.
            let fl = if dmg.flaps[side] == SurfaceState::Working { fl } else { 0.0 };
            let mut vs = v_air + s.omega.cross(r);
            vs.z += env.roll_gust * st.y;
            let mut v2 = vs.x * vs.x + vs.z * vs.z;
            let mut squared = 1.0;
            if sweep_sin != 0.0 {
                // The swept wing that leads into a sideslip meets the air squarer: cos²(Λ ∓ β) / cos²Λ.
                let slip = clamp(vs.y / vs.x.abs().max(1.0), -0.5, 0.5) * if st.y > 0.0 { 1.0 } else { -1.0 };
                let squarer = (sweep_cos + sweep_sin * slip) / sweep_cos;
                squared = squarer * squarer;
                v2 *= squared;
            }
            let flow = vs.z.atan2(vs.x);
            let a = flow + st.twist;
            let mut stall_pos = p.stall_alpha
                - p.stall_hysteresis * sep
                - if st.flapped {
                    p.flap_stall_shift * fl
                } else {
                    0.0
                };
            // The flaps' hinges are swept far less than the wing: their lift does not feel the sideslip.
            let mut cl0 = p.wing_cl0 + if st.flapped { p.flap_cl * fl / squared } else { 0.0 };
            if p.mach.is_some() {
                // The steeper curve of high Mach stops at a lower lift: the angle it reaches it at.
                let zero_lift = cl0 / p.wing_slope;
                stall_pos = lift_limit / slope_gain * (stall_pos + zero_lift) - zero_lift;
                cl0 *= slope_gain;
            }
            if st.aileron {
                cl0 -= (if st.y > 0.0 { 1.0 } else { -1.0 }) * self.aileron_gain * da[side];
            }
            // Spoilers up: the lever's, and the aileron's on the wing going down.
            let mut spoil = 0.0;
            if let (true, Some(sp)) = (st.spoiler, &p.spoilers) {
                let roll = c.aileron * if st.y > 0.0 { 1.0 } else { -1.0 };
                spoil = clamp(self.spoilers + sp.roll * roll.max(0.0), 0.0, 1.0);
                cl0 -= sp.lift * spoil;
            }
            let (lc_cl, sigma) =
                self.lift_curve(a, p.wing_slope * slope_gain, cl0, stall_pos, p.stall_alpha_neg);
            let mut cl = lc_cl * (1.0 + p.ground_lift * (1.0 - ge));
            if side == dmg.bent_side {
                cl *= 1.0 - 0.08 * dmg.bent;
            }
            *lift = StripLift {
                r,
                v2,
                flow,
                a,
                cl,
                sigma,
                spoil,
            };
            wing_cl += cl * st.weight;
            if st.flapped {
                flap_lift += p.flap_cl * fl * (1.0 - sigma) * st.weight;
            }
            stalled += sigma * st.weight;
            if st.root
                && st.y.abs() < 1.5
                && a > stall_pos - (5.5 + 2.0 * fl) * DEG
                && v2 > 100.0
            {
                out.stall_warning = true;
            }
        }
        // Wave drag: Lock's fourth power from the critical Mach number, which lift lowers (Korn).
        let wave = p.mach.as_ref().map_or(0.0, |m| {
            let critical = m.divergence.0 - m.divergence.1 * wing_cl.abs() - 0.108;
            20.0 * (mach - critical).max(0.0).powi(4)
        });
        for (st, w) in self.strips.iter().zip(lifts.iter()) {
            let side = usize::from(st.y > 0.0);
            if st.weight == 0.0 {
                continue;
            }
            let fd = if dmg.flaps[side] == SurfaceState::Working { fd } else { 0.0 };
            let sa = w.a.sin();
            let cd = p.wing_cd0 * (1.0 + 2.0 * dmg.bent)
                + wave
                + if w.spoil > 0.0 { p.spoilers.map_or(0.0, |sp| sp.drag * w.spoil / spoiler_share) } else { 0.0 }
                + if st.flapped {
                    p.flap_drag * fd / flapped_share
                } else {
                    0.0
                }
                + (1.0 - w.sigma) * p.induced_k * ge * (w.cl * wing_cl).max(0.0)
                + w.sigma * 1.9 * sa * sa;
            let q_s = 0.5 * rho * w.v2 * area * st.weight;
            let lift = q_s * w.cl;
            out.wing_lift[side] += lift;
            let drag = q_s * cd;
            let f = V3::new(
                lift * w.flow.sin() - drag * w.flow.cos(),
                0.0,
                -lift * w.flow.cos() - drag * w.flow.sin(),
            );
            // The centre of pressure moves aft once the flow separates.
            let mut r = w.r;
            r.x -= (0.2 * w.sigma + tuck) * p.chord;
            f_sum += f;
            m_sum += r.cross(f);
        }
        let qxz = 0.5 * rho * (v_air.x * v_air.x + v_air.z * v_air.z);
        let wings = 0.5 * self.wing_left[0] + 0.5 * self.wing_left[1];
        let flaps = 0.5 * self.flaps_left[0] + 0.5 * self.flaps_left[1];
        m_sum.y += qxz * area * p.chord * (p.wing_cm * wings + p.flap_cm * fl * flaps);
        // The tail of a high-wing aircraft sits below most of the extra downwash the flaps make.
        out.downwash_cl = (wing_cl - flap_lift * (1.0 - p.flap_downwash)) * ge;
        out.stalled = stalled;
        for (o, w) in out.strip_stalled.iter_mut().zip(lifts.iter()) {
            *o = w.sigma;
        }

        // Horizontal tail, in the wing's downwash and partly in the slipstream. What is left of it
        // when a half has been torn across lifts nearer the other side; without its elevator it is smaller.
        {
            let (share, centre) = self.tailplane_left();
            let gone = if dmg.elevator == SurfaceState::Gone { 1.0 - p.elevator_share } else { 1.0 };
            let mut r = p.tail_pos - self.cg;
            r.y += centre;
            let mut vt = v_air + s.omega.cross(r);
            vt.x += p.tail_wash * wash;
            let flow = vt.z.atan2(vt.x) - eps;
            // An elevator the pilot no longer moves trails with the air, and its trim tab with it.
            let de = if dmg.elevator != SurfaceState::Working {
                0.0
            } else {
                (if c.elevator >= 0.0 {
                    -c.elevator * p.elevator_up
                } else {
                    -c.elevator * p.elevator_down
                }) - if p.stabiliser_trim.is_some() { 0.0 } else { c.trim * p.trim_range }
            };
            // A trimmable tailplane turns whole: its leading edge down for nose up.
            let incidence = p.tail_incidence
                - p.stabiliser_trim.map_or(0.0, |(up, down)| if c.trim >= 0.0 { c.trim.min(1.0) * up } else { c.trim.max(-1.0) * down });
            let a = flow
                + incidence
                + p.elevator_tau / slope_gain * clamp(de, -p.elevator_up, p.elevator_down);
            let (cl, sigma) = self.lift_curve(a, p.tail_slope * slope_gain, 0.0, p.tail_stall, p.tail_stall);
            let sa = a.sin();
            let cd = 0.01 + (1.0 - sigma) * 0.08 * cl * cl + sigma * 1.9 * sa * sa;
            let q_s = 0.5 * rho * p.tail_efficiency * (vt.x * vt.x + vt.z * vt.z) * p.tail_area * share * gone;
            let lift = q_s * cl;
            let drag = q_s * cd;
            let f = V3::new(
                lift * flow.sin() - drag * flow.cos(),
                0.0,
                -lift * flow.cos() - drag * flow.sin(),
            );
            f_sum += f;
            m_sum += r.cross(f);
        }

        // Fin and rudder, in the rotating slipstream; smaller for what has been crushed or torn off.
        {
            let left = (1.0 - dmg.fin_lost) * if dmg.rudder == SurfaceState::Gone { 1.0 - p.rudder_share } else { 1.0 };
            let mut rudder = if dmg.rudder == SurfaceState::Working { c.rudder } else { 0.0 };
            if let (Some(gain), SurfaceState::Working) = (p.yaw_damper, dmg.rudder) {
                // The yaw damper: against the yaw rate that is not the turn's (g sin φ cos θ / V).
                let down = act(q_inv, V3::new(0.0, 0.0, 1.0));
                let turning = GRAVITY * down.y / v.max(30.0);
                rudder = clamp(rudder + clamp(-gain * (s.omega.z - turning), -0.3, 0.3), -1.0, 1.0);
            }
            let r = p.fin_pos - self.cg;
            let mut vf = v_air + s.omega.cross(r);
            vf.x += p.fin_wash * wash;
            vf.y -= p.swirl_factor * swirl;
            let flow = vf.y.atan2(vf.x);
            let a = flow + p.fin_rig + p.rudder_tau / slope_gain * rudder * p.rudder_max;
            let (cl, sigma) = self.lift_curve(a, p.fin_slope * slope_gain, 0.0, p.fin_stall, p.fin_stall);
            let sa = a.sin();
            let cd = 0.008 + (1.0 - sigma) * 0.1 * cl * cl + sigma * 1.9 * sa * sa;
            let q_s = 0.5 * rho * (vf.x * vf.x + vf.y * vf.y) * p.fin_area * left;
            let lift = q_s * cl;
            let drag = q_s * cd;
            let f = V3::new(
                lift * flow.sin() - drag * flow.cos(),
                -lift * flow.cos() - drag * flow.sin(),
                0.0,
            );
            f_sum += f;
            m_sum += r.cross(f);
        }

        // Fuselage.
        f_sum -= 0.5 * rho * v * p.body_drag_area * v_air;
        // A gear that retracts drags while it is out, below the centre of gravity.
        if let Some(retract) = &p.retract {
            let f = -0.5 * rho * v * retract.drag_area * self.gear * v_air;
            f_sum += f;
            m_sum += (V3::new(0.0, 0.0, 0.7 * p.cg_height) - self.cg).cross(f);
        }
        let b = clamp(beta, -0.6, 0.6);
        m_sum.x += qbar * area * p.span * (p.body_roll_beta * b + p.roll_rig);
        m_sum.z += qbar * area * p.span * p.body_yaw_beta * b;
        m_sum.y += qbar * area * p.chord * p.body_pitch_damping * s.omega.y * p.chord / (2.0 * v);
        m_sum.x +=
            qbar * area * p.span * p.roll_damping_relief * (1.0 - stalled) * s.omega.x * p.span
                / (2.0 * v);

        let aero = f_sum;
        out.specific = aero / self.mass;
        out.aero_load = -aero.z / (self.mass * GRAVITY);

        // Ground contact.
        // How high the centre of gravity can be with a point of the airframe on the ground.
        let reach = (0.6 * p.span).max(8.0);
        if altitude - env.height(s.pos.x, s.pos.y) < reach && env.kind == GroundKind::Water {
            // Water: what is under the surface floats the aircraft (less as it fills) and drags,
            // far more across the flow than along it. The wheels dig in.
            let (mut g_sum, mut gm_sum) = (V3::ZERO, V3::ZERO);
            let afloat = 1.0 - dmg.flooded;
            for (i, &(point, volume, height, area)) in p.floats.iter().enumerate() {
                let gone = match i {
                    3 | 4 => dmg.wing_torn[i - 3].is_some_and(|end| end < point.y.abs()),
                    5.. => dmg.gear_broken[i - 5] || self.gear < 0.5,
                    _ => false,
                };
                if gone {
                    continue;
                }
                let r = point - self.cg;
                let pw = s.pos + act(s.q, r);
                let depth = pw.z + env.height(pw.x, pw.y);
                if depth <= 0.0 {
                    continue;
                }
                out.in_water = true;
                let under = (depth / height).min(1.0);
                let vw = act(s.q, s.vel + s.omega.cross(r));
                let along = if i >= 5 { 1.0 } else { 0.15 };
                let flat = V3::new(vw.x, vw.y, 0.0);
                let f = V3::new(0.0, 0.0, -1000.0 * GRAVITY * volume * under * afloat)
                    - 0.5 * 1000.0 * area * under * (along * flat.length() * flat + V3::new(0.0, 0.0, vw.z.abs() * vw.z));
                let f = act(q_inv, f);
                g_sum += f;
                gm_sum += r.cross(f);
            }
            if out.in_water {
                out.water_sink = act(s.q, s.vel).z;
            }
            f_sum += g_sum;
            m_sum += gm_sum;
            out.specific += g_sum / self.mass;
        } else if altitude - env.height(s.pos.x, s.pos.y) < reach {
            let up3 = normalize(V3::new(-env.slope_n, -env.slope_e, -1.0));
            let runway = env.kind == GroundKind::Runway;
            let roll_mu = if runway { 0.015 } else { 0.06 };
            let brake_mu = p.braking * if runway { 0.6 } else { 0.4 };
            let side_mu = if runway { 0.8 } else { 0.6 };

            // Penetration, world velocity and normal force of a point of the airframe.
            let contact = |r: V3, k: f64, c_damp: f64, travel: f64| -> (f64, V3, f64) {
                let pw = s.pos + act(s.q, r);
                let depth = pw.z + env.height(pw.x, pw.y);
                if depth <= 0.0 {
                    return (0.0, V3::ZERO, 0.0);
                }
                let pen = depth * -up3.z;
                let vw = act(s.q, s.vel + s.omega.cross(r));
                let vn = vw.dot(up3);
                let mut normal = k * pen - c_damp * vn;
                if pen > travel {
                    normal += 10.0 * k * (pen - travel);
                }
                (pen, vw, normal.max(0.0))
            };

            let mut g_sum = V3::ZERO;
            let mut gm_sum = V3::ZERO;
            for (i, wheel) in p.gear.iter().enumerate() {
                // A leg that has collapsed, or that is not down and locked, holds nothing.
                if dmg.gear_broken[i] || self.gear < 0.99 {
                    continue;
                }
                let r = wheel.contact - self.cg;
                let (pen, vw, normal) = contact(r, wheel.spring, wheel.damping, p.gear_travel);
                if pen <= 0.0 {
                    continue;
                }
                out.wheel_load[i] = normal;
                out.wheel_compression[i] = pen;
                let steer = p.steer_angle(wheel, c.rudder, s.vel.x);
                let mut fw = act(s.q, V3::new(steer.cos(), steer.sin(), 0.0));
                fw -= fw.dot(up3) * up3;
                fw = normalize(fw);
                let side = up3.cross(fw);
                let vt = vw - vw.dot(up3) * up3;
                let vf = vt.dot(fw);
                let vs = vt.dot(side);
                out.wheel_roll[i] = vf;
                let brake = c.brake(wheel.brake);
                // A braked wheel holds (static friction, made smooth over 3 cm/s); a rolling one only drags.
                let mut f_long = -((vf / 0.25).tanh() * roll_mu + (vf / 0.03).tanh() * brake_mu * brake) * normal;
                let mut f_lat = -(vs / (vf.abs() + 1.5) / wheel.slip).tanh() * side_mu * normal;
                let limit = side_mu * normal;
                let mag = f_long.hypot(f_lat);
                if mag > limit && mag > 0.0 {
                    f_long *= limit / mag;
                    f_lat *= limit / mag;
                }
                let f = act(q_inv, normal * up3 + f_long * fw + f_lat * side);
                g_sum += f;
                gm_sum += r.cross(f);
            }
            let pods = p.jets.as_ref().map_or(&[][..], |jets| jets.mounts);
            for (i, hard) in p.hard_points.iter().copied().chain(pods.iter().map(|mount| mount.pod)).enumerate() {
                // A wing torn across touches with where it ends now.
                let hard = match dmg.wing_torn.get(i) {
                    Some(&Some(_)) if dmg.wing_lost(i) => continue,
                    Some(&Some(end)) => {
                        let wing = &p.pieces.wing;
                        let toward_root = ((wing.tip - end) / (wing.tip - wing.root)).clamp(0.0, 1.0);
                        V3::new(hard.x, hard.y.signum() * end, lerp(hard.z, wing.z.0, toward_root))
                    }
                    _ => hard,
                };
                let r = hard - self.cg;
                let (pen, vw, normal) = contact(r, p.hard_spring.0, p.hard_spring.1, 1.0);
                if pen <= 0.0 {
                    continue;
                }
                out.touches[i] = Some(Touch { load: normal, closing: -vw.dot(up3), speed: vw.length() });
                let vt = vw - vw.dot(up3) * up3;
                let speed = vt.length();
                let friction = if speed > 1e-3 {
                    -vt / speed * (speed / 0.5).tanh() * 0.6 * normal
                } else {
                    V3::ZERO
                };
                let f = act(q_inv, normal * up3 + friction);
                g_sum += f;
                gm_sum += r.cross(f);
                // The tail skid may touch.
                if i != 6 {
                    out.hard_contact = Some(i);
                    out.hard_impact = out.hard_impact.max(vw.length());
                    out.hard_sink = out.hard_sink.max(-vw.dot(up3));
                }
            }
            f_sum += g_sum;
            m_sum += gm_sum;
            out.specific += g_sum / self.mass;
        }

        f_sum += self.mass * act(q_inv, V3::new(0.0, 0.0, GRAVITY));

        out.force = f_sum;
        out.moment = m_sum;
        out.alpha = alpha;
        out.beta = beta;
        out.tas = v;
        let pitot = v_air.x.max(0.0);
        out.ias = pitot * (rho / atmosphere::SEA_LEVEL_DENSITY).sqrt();
        out.mach = mach;
        out.overspeed = out.ias > p.vne || p.mach.as_ref().is_some_and(|m| mach > m.mmo);
        out.flap_overspeed = flap_deg > 0.5 && out.ias > self.flap_limit(flap_deg);
        out
    }

    /// The force model with the downwash at the tail settled to the wing's lift, as in steady flight.
    pub fn forces_settled(
        &self,
        s: &State,
        c: &AircraftControls,
        env: &Environment,
        flap_deg: f64,
    ) -> FlightData {
        let mut eps = 0.0;
        let mut d = FlightData::default();
        for _ in 0..4 {
            d = self.forces(s, c, env, eps, flap_deg);
            eps = self.p.downwash_per_cl * d.downwash_cl;
        }
        d
    }

    // MARK: Loading

    /// Mass of the aircraft as loaded, kg.
    pub fn mass(&self) -> f64 {
        self.mass
    }

    /// Centre of gravity as loaded, in body axes relative to that of the reference loading
    /// (x forward). All positions in `AircraftType` are measured from the reference one.
    pub fn cg(&self) -> V3 {
        self.cg
    }

    /// Principal moments of inertia as loaded, about the centre of gravity, kg·m².
    pub fn inertia(&self) -> V3 {
        self.inertia
    }

    pub fn load(&self) -> Load {
        self.load
    }

    /// Loads the aircraft: mass, centre of gravity and inertia follow.
    pub fn set_load(&mut self, load: Load) {
        self.load = load;
        // Each item as a point mass (two, for what sits either side) added to or taken from the
        // reference loading. Products of inertia are left out: the load is nearly symmetric.
        let mut mass = self.p.mass;
        let mut moment = V3::ZERO;
        let mut inertia = self.p.inertia;
        for ((r, half_width), (now, reference)) in self
            .p
            .stations
            .iter()
            .zip(load.items().iter().zip(self.p.reference_load.items()))
        {
            let dm = now.max(0.0) - reference;
            let y2 = half_width * half_width;
            mass += dm;
            moment += dm * *r;
            inertia += dm * V3::new(y2 + r.z * r.z, r.x * r.x + r.z * r.z, r.x * r.x + y2);
        }
        // What has been torn off has taken its mass with it: all of a wing, or what was outboard of
        // a tear in a wing or in half of the tailplane.
        for portion in self.torn_off().into_iter().flatten() {
            let (r, dm) = (portion.centre(self.p.pieces), -portion.mass);
            mass += dm;
            moment += dm * r;
            inertia += dm * V3::new(r.y * r.y + r.z * r.z, r.x * r.x + r.z * r.z, r.x * r.x + r.y * r.y);
        }
        let cg = moment / mass;
        // Parallel axes: from the reference centre of gravity to the actual one.
        inertia -= mass
            * V3::new(
                cg.y * cg.y + cg.z * cg.z,
                cg.x * cg.x + cg.z * cg.z,
                cg.x * cg.x + cg.y * cg.y,
            );
        self.mass = mass;
        self.cg = cg;
        self.inertia = inertia;
    }

    /// What has been torn off the wings and the tailplane: all of a wing, or what was outboard of
    /// where it, or half of the tailplane, was torn across. Left wing, right wing, left and right
    /// half of the tailplane.
    fn torn_off(&self) -> [Option<Portion>; 4] {
        let pieces = self.p.pieces;
        let mut out = [None; 4];
        for side in 0..2 {
            let sign = [-1.0, 1.0][side];
            if let Some(end) = self.damage.wing_torn[side] {
                let kind = PieceKind::wing(side);
                out[side] = Some(if end <= 0.0 {
                    Portion::whole(kind, pieces.wing.mass)
                } else {
                    let slice = Slice { side: sign, from: end, to: pieces.wing.tip };
                    Portion { kind, slice: Some(slice), cuts: [None; 2], mass: pieces.wing.mass * pieces.wing.share(end, pieces.wing.tip) }
                });
            }
            if let Some(end) = self.damage.stabiliser_torn[side] {
                let span = &pieces.stabiliser;
                let from = end.max(span.root);
                let slice = Slice { side: sign, from, to: span.tip };
                out[2 + side] = Some(Portion { kind: PieceKind::Tail, slice: Some(slice), cuts: [None; 2], mass: span.mass * span.share(from, span.tip) });
            }
        }
        out
    }

    /// Fuel burnt per second, kg, at a shaft power in watts. Fitted to the handbook's cruise
    /// table: 8.4 gal/h at 75% power, 6.3 gal/h at 55% (a line through both).
    pub fn fuel_flow(&self, power: f64) -> f64 {
        if self.load.fuel <= 0.0 {
            return 0.0;
        }
        self.p.fuel_idle + self.p.fuel_per_joule * power.max(0.0)
    }
    // MARK: Integration

    /// The rates of the state: the rigid body's (`body::Rates`) and the propeller's.
    fn derivative(&self, s: &State, env: &Environment) -> (Derivative, FlightData) {
        let d = self.forces_separated(
            s,
            &self.controls,
            env,
            self.downwash,
            self.flap_deg,
            &self.separation,
        );
        let body = Rates::of(&s.body(), self.mass, self.inertia, d.force, d.moment);
        (Derivative { body, prop: d.prop_accel }, d)
    }

    fn advance(s: &State, d: &Derivative, h: f64) -> State {
        State {
            prop_speed: (s.prop_speed + d.prop * h).max(5.0),
            ..s.with_body(s.body().advance(&d.body, h))
        }
    }

    /// Advances the aircraft by `dt` seconds (fourth-order Runge-Kutta). A crashed aircraft goes on
    /// moving (sliding, falling, floating) until it has broken up or sunk; then nothing moves it.
    pub fn step(&mut self, dt: f64, env: &Environment) {
        if self.damage.broken_up || self.damage.sunk {
            return;
        }
        let (k1, d1) = self.derivative(&self.state, env);
        let (k2, _) = self.derivative(&Self::advance(&self.state, &k1, dt / 2.0), env);
        let (k3, _) = self.derivative(&Self::advance(&self.state, &k2, dt / 2.0), env);
        let (k4, _) = self.derivative(&Self::advance(&self.state, &k3, dt), env);
        let sum = Derivative {
            body: Rates::rk4(&k1.body, &k2.body, &k3.body, &k4.body),
            prop: (k1.prop + 2.0 * k2.prop + 2.0 * k3.prop + k4.prop) / 6.0,
        };
        let s = Self::advance(&self.state, &sum, dt);
        self.state = s.with_body(s.body().normalised());
        self.data = d1;

        // Downwash reaches the tail after the air has travelled the tail arm.
        let target = self.p.downwash_per_cl * d1.downwash_cl;
        let tau = (-self.p.tail_pos.x / d1.tas.max(10.0)).max(0.02);
        self.downwash += (target - self.downwash) * (dt / tau).min(1.0);

        let tau = self.p.stall_lag_chords * self.p.chord / d1.tas.max(10.0);
        for (sep, sigma) in self.separation.iter_mut().zip(d1.strip_stalled) {
            *sep += (sigma - *sep) * (dt / tau).min(1.0);
        }

        let notches = self.p.flap_notches;
        let wanted = notches[self.controls.flap_notch.clamp(0, notches.len() as i32 - 1) as usize];
        let travel = self.p.flap_rate * dt;
        self.flap_deg += clamp(wanted - self.flap_deg, -travel, travel);

        let on_ground = d1.on_ground();
        // The gear follows its lever, but not up with weight on the wheels.
        if let Some(retract) = &self.p.retract {
            let wanted = if self.controls.gear_up && !on_ground { 0.0 } else { 1.0 };
            let travel = dt / retract.time;
            self.gear += clamp(wanted - self.gear, -travel, travel);
        }
        // The spoilers follow the speed brake lever: in the air, no further than the flight detent.
        if let Some(sp) = &self.p.spoilers {
            let lever = clamp(self.controls.spoilers, 0.0, 1.0);
            let wanted = if on_ground { lever } else { lever.min(sp.flight_limit) };
            let travel = dt / sp.time;
            self.spoilers += clamp(wanted - self.spoilers, -travel, travel);
        }
        // Jet engines: each reverser opens on the ground when selected and closes when not; each
        // engine spools to what the throttle asks, held at idle while its reverser moves.
        if let Some(jets) = &self.p.jets {
            let intake = self.intake(&self.state, env);
            for i in 0..jets.mounts.len() {
                let burning = self.burning(i);
                let mut open = false;
                if let Some(reverser) = &jets.reverser {
                    let wanted = if self.controls.reverse && on_ground && burning { 1.0 } else { 0.0 };
                    let travel = dt / reverser.time;
                    self.reversers[i] += clamp(wanted - self.reversers[i], -travel, travel);
                    open = self.reversers[i] >= 1.0;
                }
                let in_place = if self.controls.reverse { open } else { self.reversers[i] <= 0.0 };
                let throttle = if in_place { self.controls.throttle } else { 0.0 };
                let target = if burning { jets.command(throttle, &intake, !on_ground, open) } else { jets.windmill(&intake) };
                self.state.n1[i] = jets.spool(self.state.n1[i], target, &intake, burning, dt);
            }
        }

        // The governor turns the blades coarser when the propeller runs fast, finer when it runs slow.
        if let Some(g) = self.p.governor {
            let over = self.state.prop_speed - g.speed;
            let pitch = self.state.blade_pitch + (g.gain * over + g.damping * d1.prop_accel) * dt;
            self.state.blade_pitch = clamp(pitch, g.fine, g.coarse);
        }

        if d1.fuel_flow > 0.0 && self.burns_fuel {
            let mut load = self.load;
            load.fuel = (load.fuel - d1.fuel_flow * dt).max(0.0);
            self.set_load(load);
        }

        self.check_structure(dt, &d1);
        self.check_contact(dt, &d1, env);
    }

    /// What the air loads have broken: wings past their ultimate load, flaps out too fast,
    /// control surfaces fluttering.
    fn check_structure(&mut self, dt: f64, d: &FlightData) {
        let p = &self.p;
        let (n, ias) = (d.aero_load, d.ias);
        let (uf, positive, negative) = (p.ultimate_factor, p.load_limit, p.load_limit_negative);
        let (flaps_limit, flutter_speed) = (p.load_limit_flaps, p.flutter_speed);
        let flap_limit = self.flap_limit(self.flap_deg);
        let dmg = &mut self.damage;
        dmg.most_g = dmg.most_g.max(n);
        dmg.least_g = dmg.least_g.min(n);
        // The wing that carries more gives way first; the right one if they carry the same. It gives
        // way at its root, where the bending moment is greatest against what the wing is built for:
        // the strut's fittings of the C172, the spar of the Extra, as the accident record has them.
        let side = usize::from(d.wing_lift[1].abs() >= d.wing_lift[0].abs());
        let limit = if n > 0.0 { positive } else { negative };
        let over = (n - limit) / (limit * (uf - 1.0));
        if n.abs() > limit.abs() && !dmg.wing_lost(0) && !dmg.wing_lost(1) {
            if over >= 1.0 {
                self.tear_wing(side, 0.0, 1.0, ["Left wing failed in flight: overload", "Right wing failed in flight: overload"][side]);
            } else if over > dmg.bent {
                if dmg.bent == 0.0 {
                    dmg.event(if n > 0.0 { "Overload: wings bent" } else { "Negative overload: wings bent" });
                }
                dmg.bent = over;
                dmg.bent_side = side;
            }
        }

        // The flaps are built for their own limit load, at their speed: half as much again breaks them.
        let dmg = &mut self.damage;
        if self.flap_deg > 1.0 {
            let v = flap_limit;
            if (ias / v).powi(2) > uf || n > flaps_limit * uf {
                // One goes first; the other a moment later if the load stays.
                dmg.flap_strain += dt;
                let side = if dmg.flaps[side] == SurfaceState::Gone { 1 - side } else { side };
                let first = !dmg.flaps.contains(&SurfaceState::Gone);
                if (first || dmg.flap_strain > 1.5) && dmg.flaps[side] != SurfaceState::Gone && !dmg.wing_lost(side) {
                    dmg.flaps[side] = SurfaceState::Gone;
                    dmg.event(["Left flap torn off", "Right flap torn off"][side]);
                    self.reshape();
                }
            }
        }

        if ias > flutter_speed {
            let dmg = &mut self.damage;
            dmg.flutter += dt * 8.0 * (ias / flutter_speed - 0.98);
            if dmg.flutter >= 1.0 {
                dmg.flutter = 0.0;
                self.flutter_away();
            }
        }
    }

    /// Flutter tears off the next control surface: an aileron (on the side the seed of the moment
    /// says), then the other, the elevator and the rudder. One surface at a time, as flutter is, the
    /// ailerons first, the most prone to it on a light aircraft.
    fn flutter_away(&mut self) {
        let dmg = &mut self.damage;
        let ailerons = [0, 1].map(|side| dmg.ailerons[side] != SurfaceState::Gone);
        if ailerons.contains(&true) {
            let s = self.state;
            let seed = s.pos.x.to_bits() ^ s.pos.y.to_bits().rotate_left(23) ^ s.pos.z.to_bits().rotate_left(41);
            let side = match ailerons {
                [true, true] => usize::from(Rng::new(seed).uniform() < 0.5),
                [left, _] => usize::from(!left),
            };
            dmg.ailerons[side] = SurfaceState::Gone;
            dmg.event(["Flutter: left aileron torn off", "Flutter: right aileron torn off"][side]);
        } else if dmg.elevator != SurfaceState::Gone {
            dmg.elevator = SurfaceState::Gone;
            dmg.event("Flutter: elevator torn off");
        } else if dmg.rudder != SurfaceState::Gone {
            dmg.rudder = SurfaceState::Gone;
            dmg.event("Flutter: rudder torn off");
        }
        self.reshape();
    }

    /// The ground or the water: legs that collapse, a wing crushed where it touches, the tail and
    /// the fin struck, the propeller striking, the airframe hitting hard enough to break up, ditching.
    fn check_contact(&mut self, dt: f64, d: &FlightData, env: &Environment) {
        if d.in_water {
            if !self.damage.broken_up && (d.water_sink > self.p.breakup_speed || d.tas > 45.0) {
                self.break_up("Hit the water: the aircraft broke up", 0.3);
                return;
            }
            self.crashed.get_or_insert("Ditched");
            // It fills in about three minutes; it goes under about a minute after touching.
            self.damage.flooded = (self.damage.flooded + dt / 180.0).min(1.0);
            if self.state.pos.z + env.ground_height > 20.0 {
                self.damage.sunk = true;
            }
            return;
        }
        for i in 0..self.p.gear.len() {
            let wheel = self.p.gear[i];
            if !self.damage.gear_broken[i] && d.wheel_load[i] > wheel.strength {
                self.break_leg(i, wheel.broken);
            }
        }

        // A wing whose end the ground pushes harder than it takes is crushed and torn there, a strip
        // at a time: a tip that only touches is scraped.
        for side in 0..2 {
            let Some(touch) = d.touches[side] else { continue };
            if touch.load > self.p.wing_crush {
                let end = Damage::end(self.damage.wing_torn[side], self.p.pieces.wing.tip);
                let what = ["Left wing broken against the ground", "Right wing broken against the ground"][side];
                self.tear_wing(side, self.station_inboard(end), 0.6, what);
                self.crashed.get_or_insert(what);
            } else if touch.closing > 0.5 && self.damage.last != ["Left wingtip scraped", "Right wingtip scraped"][side] {
                self.damage.event(["Left wingtip scraped", "Right wingtip scraped"][side]);
            }
        }
        // The tail into the ground fast buckles the tail cone, and its cables go slack.
        if d.touches[6].is_some_and(|t| t.closing > TAIL_STRIKE) && self.damage.elevator == SurfaceState::Working {
            self.damage.elevator = SurfaceState::Loose;
            if self.damage.rudder == SurfaceState::Working {
                self.damage.rudder = SurfaceState::Loose;
            }
            self.damage.event("Tail strike: elevator and rudder cables slack");
        }

        // A nacelle dragged along the ground at speed: its engine swallows what it throws up and fails.
        for engine in 0..MOST_ENGINES {
            if d.touches[7 + engine].is_some_and(|t| t.speed > 2.0) && !self.damage.engines_failed[engine] {
                self.damage.engines_failed[engine] = true;
                self.damage.event(["Engine 1 struck the ground", "Engine 2 struck the ground", "Engine 3 struck the ground", "Engine 4 struck the ground"][engine]);
            }
        }

        // The rest of the airframe: the propeller (the nose of a jet), the top of the cabin and of
        // the fin, the belly, the nacelles.
        let body = [2, 3, 4, 5, 7, 8, 9, 10].map(|i| d.touches[i].map(|t| (i, t)));
        let Some(&(last, _)) = body.iter().flatten().last() else { return };
        let sink = body.iter().flatten().fold(0.0_f64, |most, (_, t)| most.max(t.closing));
        // The belly and the nacelles may slide along the ground.
        let impact = body.iter().flatten().filter(|(i, _)| *i < 5).fold(0.0_f64, |most, (_, t)| most.max(t.speed));
        // Into the ground faster than the airframe takes, or the top of the cabin or of the fin, or
        // the propeller, hitting it at speed: it cartwheels apart.
        if sink > self.p.breakup_speed || impact > 35.0 {
            self.break_up("Hit the ground: the aircraft broke up", 0.5);
            return;
        }
        if body.iter().flatten().all(|(_, t)| t.speed <= 2.0) {
            return;
        }
        if d.touches[2].is_some() && !self.damage.propeller_bent && self.state.prop_speed > 20.0 && self.p.jets.is_none() {
            self.strike_propeller();
        }
        // Upside down, the fin is crushed against the ground and its rudder with it.
        if d.touches[4].is_some_and(|t| t.load > 0.5 * self.p.wing_crush) && self.damage.fin_lost < 0.5 {
            self.damage.fin_lost = 0.5;
            self.damage.rudder = SurfaceState::Gone;
            self.damage.event("Fin crushed: rudder lost");
        }
        let gear_up = self.gear < 0.99 && !self.damage.gear_broken.contains(&true);
        let what = match last {
            2 if self.p.jets.is_some() => "Nose on the ground",
            2 => "Propeller strike",
            3 | 4 => "Aircraft overturned",
            _ if gear_up => "Landed with the gear up",
            5 => "Gear collapsed",
            _ => "Engine nacelle on the ground",
        };
        self.crashed.get_or_insert(what);
    }

    /// The station where a wing or a half of the tailplane that ends at `end` is torn when its end
    /// is crushed: where the next strip of the wing begins, inboard (0, all of it, when there is none
    /// outboard of the root). The force model knows the wing no finer than its strips.
    fn station_inboard(&self, end: f64) -> f64 {
        let root = self.p.pieces.wing.root;
        self.p.strip_edges.iter().rev().copied().find(|&y| y > root && y < end - 0.3).unwrap_or(0.0)
    }

    /// After something has been torn off: the wing's strips, and the mass, centre of gravity and
    /// inertia of what is left.
    fn reshape(&mut self) {
        (self.strips, self.wing_left, self.flaps_left) = wing_strips(&self.p, &self.damage);
        self.set_load(self.load);
    }

    /// A wing is torn across `station` (m from the centre line; at or inside its root, all of it):
    /// what was outboard of it comes off and falls as a piece of its own, keeping `keep` of its
    /// velocity (all of it torn off in the air), and what is left flies as it can. A tear inboard of
    /// the end of the wing's fuel tank bursts it: its fuel goes with the piece, and burns only if
    /// what tore it was violent enough (`wreck::fuel_released`). Without a wing the flight is over.
    pub fn tear_wing(&mut self, side: usize, station: f64, keep: f64, what: &'static str) {
        let wing = self.p.pieces.wing;
        let end = Damage::end(self.damage.wing_torn[side], wing.tip);
        if self.damage.broken_up || self.damage.wing_lost(side) || station >= end {
            return;
        }
        let station = if station <= wing.root { 0.0 } else { station };
        let sign = [-1.0, 1.0][side];
        let kind = PieceKind::wing(side);
        let portion = if station == 0.0 && end >= wing.tip {
            Portion::whole(kind, wing.mass)
        } else {
            let from = station.max(wing.root);
            let pieces = self.p.pieces;
            let outboard = (station > 0.0).then(|| pieces.tear(kind, sign, from, false));
            let inboard = (end < wing.tip).then(|| pieces.tear(kind, sign, end, true));
            Portion { kind, slice: Some(Slice { side: sign, from, to: end }), cuts: [outboard, inboard], mass: wing.mass * wing.share(from, end) }
        };
        let mut fuel = 0.0;
        if self.p.wing_tanks.is_some_and(|tank| station < tank) && !self.damage.tank_burst[side] {
            self.damage.tank_burst[side] = true;
            // Both tanks feed the engine: each holds half of what is left, until one has burst.
            fuel = if self.damage.tank_burst[1 - side] { self.load.fuel } else { self.load.fuel / 2.0 };
            self.load.fuel -= fuel;
            self.damage.event(["Left tank burst", "Right tank burst"][side]);
        }
        self.shed(portion, keep, fuel);
        self.damage.wing_torn[side] = Some(station);
        self.damage.event(what);
        if station == 0.0 {
            self.crashed.get_or_insert(what);
        }
        self.reshape();
    }

    /// Half of the tailplane is torn across `station`: what was outboard of it comes off.
    pub fn tear_stabiliser(&mut self, side: usize, station: f64, keep: f64, what: &'static str) {
        let span = self.p.pieces.stabiliser;
        let end = Damage::end(self.damage.stabiliser_torn[side], span.tip);
        let station = station.max(span.root);
        if self.damage.broken_up || station >= end {
            return;
        }
        let sign = [-1.0, 1.0][side];
        let pieces = self.p.pieces;
        let inboard = (end < span.tip).then(|| pieces.tear(PieceKind::Tail, sign, end, true));
        let cuts = [Some(pieces.tear(PieceKind::Tail, sign, station, false)), inboard];
        let portion = Portion { kind: PieceKind::Tail, slice: Some(Slice { side: sign, from: station, to: end }), cuts, mass: span.mass * span.share(station, end) };
        self.shed(portion, keep, 0.0);
        self.damage.stabiliser_torn[side] = Some(station);
        self.damage.event(what);
        self.reshape();
    }

    /// A part of the airframe comes off as a piece of its own, with the fuel it spills.
    fn shed(&mut self, portion: Portion, keep: f64, fuel: f64) {
        let s = self.state;
        let seed = s.pos.x.to_bits() ^ s.pos.y.to_bits().rotate_left(17);
        let mut wreck = Wreck::of(self.p.pieces, &[portion], portion.mass, s.pos - act(s.q, self.cg), s.q, act(s.q, s.vel), s.omega, keep, seed, 0.0);
        wreck.spill(fuel, self.data.in_water);
        self.wrecks.push(wreck);
    }

    /// A leg of the landing gear collapses (`AircraftType::gear` order): the flight is over.
    pub fn break_leg(&mut self, leg: usize, what: &'static str) {
        if self.damage.gear_broken[leg] {
            return;
        }
        self.damage.gear_broken[leg] = true;
        self.damage.event(what);
        self.crashed.get_or_insert(what);
    }

    /// The propeller strikes something: it is bent, and the engine stops.
    pub fn strike_propeller(&mut self) {
        if !self.damage.propeller_bent {
            self.damage.propeller_bent = true;
            self.damage.event("Propeller damaged: engine stopped");
        }
    }

    /// An aileron's hinge is bent: it no longer follows the stick.
    pub fn loosen_aileron(&mut self, side: usize, what: &'static str) {
        if self.damage.ailerons[side] == SurfaceState::Working {
            self.damage.ailerons[side] = SurfaceState::Loose;
            self.damage.event(what);
        }
    }

    /// Share of the tailplane's area that is left, and how far to the right of the middle its lift
    /// acts now, m: what is left of a half torn across lifts nearer the other side.
    fn tailplane_left(&self) -> (f64, f64) {
        let torn = self.damage.stabiliser_torn;
        if torn == [None, None] {
            return (1.0, 0.0);
        }
        let span = &self.p.pieces.stabiliser;
        let [left, right] = torn.map(|t| t.map_or(1.0, |end| span.share(span.root, end)));
        if left + right <= 0.0 {
            return (0.0, 0.0);
        }
        let arm = |share: f64| span.root + share * (span.tip - span.root) / 2.0;
        ((left + right) / 2.0, (right * arm(right) - left * arm(left)) / (left + right))
    }

    /// How a point of the airframe (body axes, from the reference centre of gravity) moves: the
    /// direction (body axes), the speed, and the mass it carries along that direction (less than the
    /// aircraft's away from the centre of gravity, where a push turns it as well).
    fn point_motion(&self, r: V3) -> (V3, f64, f64) {
        let arm = r - self.cg;
        let v = self.state.vel + self.state.omega.cross(arm);
        let speed = v.length();
        if speed < 1e-6 {
            return (V3::X, 0.0, self.mass);
        }
        let u = v / speed;
        let k = arm.cross(u);
        (u, speed, 1.0 / (1.0 / self.mass + (k * k / self.inertia).element_sum()))
    }

    /// The kinetic energy a point of the airframe (body axes, from the reference centre of gravity)
    /// can give up against something it runs into before it stops, J.
    pub fn energy_at(&self, r: V3) -> f64 {
        let (_, speed, mass) = self.point_motion(r);
        0.5 * mass * speed * speed
    }

    /// A point of the airframe runs into something that takes `energy` (J) out of its motion: an
    /// impulse there against its velocity, which slows the aircraft and turns it about the point.
    /// False if the point had less than that to give: it has stopped.
    pub fn absorb(&mut self, r: V3, energy: f64) -> bool {
        let (u, speed, mass) = self.point_motion(r);
        let room = speed * speed - 2.0 * energy / mass;
        let (push, through) = if room > 0.0 { (mass * (speed - room.sqrt()), true) } else { (mass * speed, false) };
        let impulse = -push * u;
        self.state.vel += impulse / self.mass;
        self.state.omega += (r - self.cg).cross(impulse) / self.inertia;
        through
    }

    /// The aircraft breaks into pieces, which go to the world as a wreck; they keep `keep` of
    /// its velocity. With fuel aboard and a hard enough impact the tanks burst, and the fuel goes up
    /// in a fireball and burns (`wreck::fuel_released`).
    fn break_up(&mut self, what: &'static str, keep: f64) {
        if self.damage.broken_up {
            return;
        }
        self.damage.broken_up = true;
        self.damage.event(what);
        // What broke it in the air says more than where it ended.
        if !self.damage.wing_lost(0) && !self.damage.wing_lost(1) {
            self.crashed = Some(what);
        }
        let s = self.state;
        let pieces = self.p.pieces;
        // What is left of each wing and of the tail; the fuselage is the rest.
        let mut rest = Vec::with_capacity(3);
        for side in 0..2 {
            let sign = [-1.0, 1.0][side];
            let kind = PieceKind::wing(side);
            match self.damage.wing_torn[side] {
                None => rest.push(Portion::whole(kind, pieces.wing.mass)),
                Some(end) if end > 0.0 => {
                    let slice = Slice { side: sign, from: pieces.wing.root, to: end };
                    let cuts = self.damage.left_of(pieces, kind).unwrap_or_default();
                    rest.push(Portion { kind, slice: Some(slice), cuts, mass: pieces.wing.mass * pieces.wing.share(pieces.wing.root, end) });
                }
                Some(_) => {}
            }
        }
        let torn = self.torn_off();
        let tail_cuts = self.damage.left_of(pieces, PieceKind::Tail).unwrap_or_default();
        let tail_mass = pieces.tail_mass - torn[2..].iter().flatten().fold(0.0, |sum, p| sum + p.mass);
        rest.push(Portion { kind: PieceKind::Tail, slice: None, cuts: tail_cuts, mass: tail_mass });
        let fuselage = (self.mass - rest.iter().fold(0.0, |sum, p| sum + p.mass)).max(300.0);
        let portions: Vec<Portion> = std::iter::once(Portion::whole(PieceKind::Fuselage, fuselage)).chain(rest).collect();
        let vel = act(s.q, s.vel);
        let seed = s.pos.x.to_bits() ^ s.pos.y.to_bits().rotate_left(29) ^ s.pos.z.to_bits().rotate_left(7);
        let mut wreck = Wreck::of(pieces, &portions, self.mass, s.pos - act(s.q, self.cg), s.q, vel, s.omega, keep, seed, 0.0);
        wreck.spill(self.load.fuel, self.data.in_water);
        self.wrecks.push(wreck);
        let mut load = self.load;
        load.fuel = 0.0;
        self.set_load(load);
        self.state.vel = V3::ZERO;
        self.state.omega = V3::ZERO;
    }

    /// The aircraft has run into something (a tree, a building, another aircraft) at `speed`
    /// m/s: fast, it breaks up and its pieces keep `keep` of its velocity; slowly, it is stopped
    /// against it and the flight is over.
    pub fn collide(&mut self, what: &'static str, speed: f64, keep: f64) {
        if self.damage.broken_up {
            return;
        }
        if speed > COLLISION_BREAKUP {
            self.break_up(what, keep);
        } else {
            if self.crashed.is_none() {
                self.damage.event(what);
            }
            self.crashed.get_or_insert(what);
            self.state.vel = V3::ZERO;
            self.state.omega = V3::ZERO;
        }
    }

    /// What has given way in the airframe.
    pub fn damage(&self) -> &Damage {
        &self.damage
    }

    /// The pieces that have come off since the last call, for the world to keep.
    pub fn take_wrecks(&mut self) -> Vec<Wreck> {
        std::mem::take(&mut self.wrecks)
    }

    /// Called when the aircraft is placed somewhere new: forgets the crash, the damage and the last
    /// measurements, and is whole again.
    pub fn clear_crash(&mut self) {
        self.crashed = None;
        self.separation = [0.0; 8];
        self.data = FlightData::default();
        self.damage = Damage::default();
        self.wrecks.clear();
        self.reshape();
    }

    // MARK: Steady flight

    /// Propeller speed (rad/s) at which engine and propeller torques balance, at blade angle
    /// `s.blade_pitch`.
    fn equilibrium_prop_speed(&self, s: &State, c: &AircraftControls, env: &Environment) -> f64 {
        let (mut lo, mut hi) = (20.0, 400.0);
        let mut t = *s;
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            t.prop_speed = mid;
            if self.forces(&t, c, env, 0.0, 0.0).prop_accel > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }

    /// Propeller speed and blade angle the engine settles at: for a fixed-pitch propeller the
    /// speed that balances; with a governor its speed and the blade angle that holds it, or the
    /// speed at a stop when the governor cannot hold it.
    fn equilibrium_prop(&self, s: &State, c: &AircraftControls, env: &Environment) -> (f64, f64) {
        let Some(g) = self.p.governor else {
            return (self.equilibrium_prop_speed(s, c, env), s.blade_pitch);
        };
        let mut t = *s;
        t.prop_speed = g.speed;
        let accel = |t: &State| self.forces(t, c, env, 0.0, 0.0).prop_accel;
        for stop in [g.fine, g.coarse] {
            t.blade_pitch = stop;
            let a = accel(&t);
            if (stop == g.fine && a <= 0.0) || (stop == g.coarse && a >= 0.0) {
                return (self.equilibrium_prop_speed(&t, c, env), stop);
            }
        }
        let (mut lo, mut hi) = (g.fine, g.coarse);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            t.blade_pitch = mid;
            if accel(&t) > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (g.speed, (lo + hi) / 2.0)
    }

    /// Forces in steady, wings-level flight at the given angle of attack and flight-path angle,
    /// and the propeller speed (rad/s) the engine settles at.
    #[allow(clippy::too_many_arguments)]
    pub fn steady(
        &self,
        tas: f64,
        altitude: f64,
        alpha: f64,
        climb_angle: f64,
        elevator: f64,
        throttle: f64,
        flap_deg: f64,
    ) -> (FlightData, f64) {
        let (d, s) = self.steady_state(tas, altitude, alpha, climb_angle, elevator, throttle, flap_deg);
        (d, s.prop_speed)
    }

    /// `steady`, with the whole state it was computed for (propeller speed and blade angle settled).
    #[allow(clippy::too_many_arguments)]
    pub fn steady_state(
        &self,
        tas: f64,
        altitude: f64,
        alpha: f64,
        climb_angle: f64,
        elevator: f64,
        throttle: f64,
        flap_deg: f64,
    ) -> (FlightData, State) {
        let env = Environment::standard(altitude);
        let mut s = State {
            pos: V3::new(0.0, 0.0, -altitude),
            vel: V3::new(tas * alpha.cos(), 0.0, tas * alpha.sin()),
            q: quaternion(0.0, alpha + climb_angle, 0.0),
            ..State::default()
        };
        // What carries the tail's load in steady flight: the trim of a trimmable tailplane, else the yoke.
        let stabiliser = self.p.stabiliser_trim.is_some();
        let c = AircraftControls {
            elevator: if stabiliser { 0.0 } else { elevator },
            trim: if stabiliser { elevator } else { 0.0 },
            throttle,
            ..AircraftControls::default()
        };
        if self.p.jets.is_some() {
            s.n1 = self.settled_n1(&s, throttle, &env);
        } else {
            (s.prop_speed, s.blade_pitch) = self.equilibrium_prop(&s, &c, &env);
        }
        let mut eps = 0.0;
        let mut d = FlightData::default();
        for _ in 0..4 {
            d = self.forces(&s, &c, &env, eps, flap_deg);
            eps = self.p.downwash_per_cl * d.downwash_cl;
        }
        (d, s)
    }

    /// Solves for steady flight. Give `throttle` to get the climb angle; give `None` to get the
    /// throttle that holds `climb_angle` (which is ignored when a throttle is given).
    /// Returns `None` if the aircraft cannot fly that condition.
    pub fn trim(
        &self,
        tas: f64,
        altitude: f64,
        flap_deg: f64,
        throttle: Option<f64>,
        climb_angle: f64,
    ) -> Option<Trim> {
        // alpha, elevator, throttle or climb angle
        let mut x = [4.0 * DEG, 0.0, if throttle.is_none() { 0.5 } else { 0.0 }];
        let weight = self.mass * GRAVITY;
        let residual = |x: &[f64; 3]| -> ([f64; 3], State) {
            let (data, state) = self.steady_state(
                tas,
                altitude,
                x[0],
                if throttle.is_none() {
                    climb_angle
                } else {
                    x[2]
                },
                x[1],
                throttle.unwrap_or(x[2]),
                flap_deg,
            );
            (
                [
                    data.force.x / weight,
                    data.force.z / weight,
                    data.moment.y / (weight * self.p.chord),
                ],
                state,
            )
        };
        for _ in 0..60 {
            let (f, state) = residual(&x);
            if f[0].abs() + f[1].abs() + f[2].abs() < 1e-7 {
                if x[1].abs() > 1.0 || (throttle.is_none() && (x[2] < 0.0 || x[2] > 1.0)) {
                    return None;
                }
                let stabiliser = self.p.stabiliser_trim.is_some();
                return Some(Trim {
                    alpha: x[0],
                    elevator: if stabiliser { 0.0 } else { x[1] },
                    trim: if stabiliser { x[1] } else { 0.0 },
                    throttle: throttle.unwrap_or(x[2]),
                    climb_angle: if throttle.is_none() {
                        climb_angle
                    } else {
                        x[2]
                    },
                    prop_speed: state.prop_speed,
                    blade_pitch: state.blade_pitch,
                    n1: state.n1,
                });
            }
            let mut cols = [V3::ZERO; 3];
            for (j, col) in cols.iter_mut().enumerate() {
                let mut xp = x;
                let h = 1e-5;
                xp[j] += h;
                let (fp, _) = residual(&xp);
                *col = V3::new((fp[0] - f[0]) / h, (fp[1] - f[1]) / h, (fp[2] - f[2]) / h);
            }
            let dx =
                DMat3::from_cols(cols[0], cols[1], cols[2]).inverse() * V3::new(f[0], f[1], f[2]);
            if !dx.x.is_finite() {
                return None;
            }
            // Damped step: shorten it until the residual actually falls.
            let mut scale = (0.1
                / dx.x
                    .abs()
                    .max(dx.y.abs() * 0.3)
                    .max(dx.z.abs() * 0.3)
                    .max(1e-12))
            .min(1.0);
            let norm = f[0].abs() + f[1].abs() + f[2].abs();
            for _ in 0..12 {
                let trial = [
                    x[0] - dx.x * scale,
                    x[1] - dx.y * scale,
                    x[2] - dx.z * scale,
                ];
                let ft = residual(&trial).0;
                if ft[0].abs() + ft[1].abs() + ft[2].abs() < norm {
                    break;
                }
                scale *= 0.5;
            }
            x[0] -= dx.x * scale;
            x[1] -= dx.y * scale;
            x[2] -= dx.z * scale;
        }
        None
    }

    /// Level flight at a true airspeed and altitude, flaps up: the throttle and elevator it needs.
    pub fn trim_level(&self, tas: f64, altitude: f64) -> Option<Trim> {
        self.trim(tas, altitude, 0.0, None, 0.0)
    }

    /// Largest lift coefficient reachable in trimmed, power-off flight, and the stall speed it gives.
    pub fn stall(&self, flap_deg: f64, altitude: f64) -> Stall {
        let rho = atmosphere::density(altitude);
        let v = 26.0;
        let mut best = 0.0_f64;
        let moment = |alpha: f64, elevator: f64| {
            self.steady(v, altitude, alpha, 0.0, elevator, 0.0, flap_deg)
                .0
                .moment
                .y
        };
        for i in 0..=80 {
            let alpha = (4.0 + i as f64 * 0.2) * DEG;
            let (mut lo, mut hi) = (-1.0, 1.0);
            let m_lo = moment(alpha, lo);
            let m_hi = moment(alpha, hi);
            if m_lo * m_hi > 0.0 {
                continue;
            }
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                let m = moment(alpha, mid);
                if m * m_lo > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let d = self
                .steady(v, altitude, alpha, 0.0, (lo + hi) / 2.0, 0.0, flap_deg)
                .0;
            let lift = self.mass * (-d.specific.z * alpha.cos() + d.specific.x * alpha.sin());
            best = best.max(lift / (0.5 * rho * v * v * self.p.wing_area));
        }
        let speed = (2.0 * self.mass * GRAVITY / (rho * self.p.wing_area * best)).sqrt();
        Stall {
            cl_max: best,
            speed,
        }
    }
}
