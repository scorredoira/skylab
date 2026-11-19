//! The autopilot: a `Pilot` that flies the whole aircraft (yoke, pedals, throttle, flaps, trim)
//! towards a `Goal`, from what the `Observation` shows and nothing else.
//!
//! It is a small state machine (`Phase`) that, for each phase of a goal, says what it wants
//! (a line over the ground or a heading, an altitude or a rate, a speed, a flap setting), over
//! cascaded loops that turn that into controls:
//!
//! ```text
//! line / heading ─► bank wanted ─► ailerons          (rudder keeps the ball centred)
//! altitude ─► vertical speed ─► climb angle ─► pitch wanted ─► elevator ─► trim
//! airspeed ─► throttle        (in a full-power climb: airspeed ─► pitch wanted)
//! ```
//!
//! Every gain is a field of `Gains`, with the response it was tuned for.

use std::f64::consts::PI;

use crate::aircraft::{AircraftControls, AircraftKind, AircraftType, Speeds};
use crate::math::{clamp, remainder, smoothstep, DEG, GRAVITY, KNOTS};
use crate::pilot::Pilot;
use crate::terrain::Airport;
use crate::vehicle::Controls;
use crate::world::{Observation, GROUND_AHEAD};

const TAU: f64 = 2.0 * PI;

/// Where an aircraft arriving from elsewhere joins the downwind leg, metres past the threshold.
const JOIN_AT: f64 = 1800.0;
/// Runway left that a new takeoff needs.
const TAKEOFF_ROOM: f64 = 480.0;

/// The shape and the speeds of the circuit that `Goal::Circuit` and `Goal::Land` fly. The default
/// is a standard circuit, 300 m high and 1200 m wide with a 2200 m final; a smaller one is quicker
/// (`sim-env`'s `race` example searches for the quickest that still lands).
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    /// Height of the downwind leg above the runway, m.
    pub height: f64,
    /// Distance of the downwind leg from the centreline, m.
    pub width: f64,
    /// Where the final begins, metres before the threshold.
    pub final_length: f64,
    /// The approach path meets the runway this far past the threshold, m.
    pub aim: f64,
    /// Angle of the approach path, radians.
    pub glide: f64,
    /// The climb-out turns to crosswind once this high above the runway, m ...
    pub turn_height: f64,
    /// ... and this far past the far end of the runway, m (negative: before it).
    pub upwind: f64,
    /// Bank of the turns in the circuit, radians.
    pub bank: f64,
    /// Indicated airspeeds, m/s: rotation, climb, downwind, descending on downwind, base, final.
    pub rotate_speed: f64,
    pub climb_speed: f64,
    pub pattern_speed: f64,
    pub descent_speed: f64,
    pub base_speed: f64,
    pub final_speed: f64,
    /// Flap notch (0 to 3) on base and final.
    pub landing_flaps: i32,
    /// Flap notch for the takeoff, kept until 60 m above the ground.
    pub takeoff_flaps: i32,
    /// Height of the wheels at which the flare begins, m.
    pub flare_height: f64,
    /// `Goal::Circuit` only, one circuit: land in the direction opposite to the takeoff. After the
    /// climb-out the aircraft goes straight on to a downwind leg `width` to the left of the
    /// landing direction (a teardrop), then base and final as usual.
    pub land_reversed: bool,
}

impl Default for Pattern {
    /// The C172's circuit.
    fn default() -> Pattern {
        let s = AircraftKind::C172.spec().speeds;
        Pattern {
            height: 300.0,
            width: 1200.0,
            final_length: 2200.0,
            aim: 150.0,
            glide: 3.0 * DEG,
            turn_height: 120.0,
            upwind: 200.0,
            bank: 20.0 * DEG,
            rotate_speed: s.rotate,
            climb_speed: s.climb,
            pattern_speed: s.pattern,
            descent_speed: s.descent,
            base_speed: s.base,
            final_speed: s.final_approach,
            landing_flaps: 2,
            takeoff_flaps: 0,
            flare_height: 5.0,
            land_reversed: false,
        }
    }
}

impl Pattern {
    /// The default circuit flown at the speeds of this type of aircraft, with no more flap than it has.
    pub fn for_type(aircraft: &AircraftType) -> Pattern {
        let s = aircraft.speeds;
        let most = aircraft.max_flap_notch();
        let d = Pattern::default();
        let light = Pattern {
            rotate_speed: s.rotate,
            climb_speed: s.climb,
            pattern_speed: s.pattern,
            descent_speed: s.descent,
            base_speed: s.base,
            final_speed: s.final_approach,
            landing_flaps: d.landing_flaps.min(most),
            takeoff_flaps: d.takeoff_flaps.min(most),
            ..d
        };
        // A transport flies it faster: its turns are wider by the square of its speed, and the
        // legs with them; the final long enough to be settled on it, the downwind leg above
        // where the glide path is at its start.
        let k = (s.pattern / d.pattern_speed).powi(2);
        if !Profile::heavy(aircraft) {
            return light;
        }
        let final_length = 0.8 * k * d.final_length;
        let aim = d.aim * k.sqrt();
        Pattern {
            width: k * d.width,
            final_length,
            aim,
            height: (final_length + aim) * d.glide.tan() + 40.0,
            turn_height: d.turn_height * k.sqrt(),
            upwind: k * d.upwind,
            bank: 25.0 * DEG,
            landing_flaps: aircraft.final_flap_notch,
            takeoff_flaps: aircraft.takeoff_flap_notch,
            flare_height: d.flare_height * k.sqrt(),
            ..light
        }
    }

    /// The names of the fields, as `get`, `set` and the text form know them.
    pub const FIELDS: [&'static str; 18] = [
        "height",
        "width",
        "final_length",
        "aim",
        "glide",
        "turn_height",
        "upwind",
        "bank",
        "rotate_speed",
        "climb_speed",
        "pattern_speed",
        "descent_speed",
        "base_speed",
        "final_speed",
        "landing_flaps",
        "land_reversed",
        "takeoff_flaps",
        "flare_height",
    ];

    /// A field by name, in the units of the text form: metres, degrees, knots, notches, 0 or 1.
    pub fn get(&self, name: &str) -> Option<f64> {
        Some(match name {
            "height" => self.height,
            "width" => self.width,
            "final_length" => self.final_length,
            "aim" => self.aim,
            "glide" => self.glide / DEG,
            "turn_height" => self.turn_height,
            "upwind" => self.upwind,
            "bank" => self.bank / DEG,
            "rotate_speed" => self.rotate_speed / KNOTS,
            "climb_speed" => self.climb_speed / KNOTS,
            "pattern_speed" => self.pattern_speed / KNOTS,
            "descent_speed" => self.descent_speed / KNOTS,
            "base_speed" => self.base_speed / KNOTS,
            "final_speed" => self.final_speed / KNOTS,
            "landing_flaps" => f64::from(self.landing_flaps),
            "land_reversed" => f64::from(u8::from(self.land_reversed)),
            "takeoff_flaps" => f64::from(self.takeoff_flaps),
            "flare_height" => self.flare_height,
            _ => return None,
        })
    }

    /// Sets a field by name, in the units of `get`. False if there is no such field.
    pub fn set(&mut self, name: &str, x: f64) -> bool {
        let notch = |x: f64| x.round().clamp(0.0, 3.0) as i32;
        match name {
            "height" => self.height = x,
            "width" => self.width = x,
            "final_length" => self.final_length = x,
            "aim" => self.aim = x,
            "glide" => self.glide = x * DEG,
            "turn_height" => self.turn_height = x,
            "upwind" => self.upwind = x,
            "bank" => self.bank = x * DEG,
            "rotate_speed" => self.rotate_speed = x * KNOTS,
            "climb_speed" => self.climb_speed = x * KNOTS,
            "pattern_speed" => self.pattern_speed = x * KNOTS,
            "descent_speed" => self.descent_speed = x * KNOTS,
            "base_speed" => self.base_speed = x * KNOTS,
            "final_speed" => self.final_speed = x * KNOTS,
            "landing_flaps" => self.landing_flaps = notch(x),
            "land_reversed" => self.land_reversed = x >= 0.5,
            "takeoff_flaps" => self.takeoff_flaps = notch(x),
            "flare_height" => self.flare_height = x,
            _ => return false,
        }
        true
    }

    /// One `name value` line per field.
    pub fn to_text(&self) -> String {
        Pattern::FIELDS
            .iter()
            .map(|f| format!("{f} {:.3}\n", self.get(f).unwrap_or(0.0)))
            .collect()
    }

    /// Lines of `name value` over the default; `#` starts a comment line.
    pub fn from_text(text: &str) -> Result<Pattern, String> {
        let mut p = Pattern::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let ok = match (parts.next(), parts.next().and_then(|x| x.parse().ok())) {
                (Some(name), Some(x)) => p.set(name, x),
                _ => false,
            };
            if !ok {
                return Err(format!("bad pattern line: {line}"));
            }
        }
        Ok(p)
    }
}

/// A point of a route.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waypoint {
    pub north: f64,
    pub east: f64,
    /// Above sea level, metres.
    pub altitude: f64,
}

/// What a hold does with the height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Vertical {
    /// Go to this altitude (metres above sea level) and stay there.
    Altitude(f64),
    /// Climb or descend at this rate, m/s, positive up.
    Speed(f64),
}

/// What the autopilot is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    /// Hold a heading (radians; `None` keeps the wings level), an altitude or a vertical speed,
    /// and an indicated airspeed (m/s; `None` leaves the throttle where it is).
    Hold {
        heading: Option<f64>,
        vertical: Vertical,
        airspeed: Option<f64>,
    },
    /// Fly over these points in order, then keep the last course and altitude.
    Route(Vec<Waypoint>),
    /// Take off, fly the left-hand circuit, land and stop; with `repeat`, again and again.
    /// `reversed` uses the opposite runway direction. Engaged in the air it joins the circuit.
    Circuit {
        airport: Airport,
        reversed: bool,
        repeat: bool,
    },
    /// Join the circuit, land and stop. Engaged on the ground it is one circuit.
    Land { airport: Airport, reversed: bool },
}

/// Where in its goal the autopilot is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Hold,
    Route,
    /// Past the last waypoint: holding its course and altitude.
    RouteDone,
    Takeoff,
    Climbout,
    Crosswind,
    /// Flying to the start of the downwind leg.
    Join,
    /// Circling down to circuit height.
    Orbit,
    Downwind,
    Base,
    Final,
    Flare,
    Rollout,
    Stopped,
}

/// Every gain of the loops. The inner ones are those of `KeyboardPilot`'s assist; like there,
/// yoke deflections are scaled by (80 kt / airspeed)², because control power grows with dynamic
/// pressure. None is near the edge: `tests/autopilot.rs` lands the circuit in wind and turbulence
/// with the surface and throttle gains halved and doubled.
#[derive(Clone, Debug, PartialEq)]
pub struct Gains {
    /// Elevator per radian of pitch error and per rad/s of pitch rate: a 5° step of the wanted
    /// pitch is reached in about 1.5 s without overshoot.
    pub pitch_p: f64,
    pub pitch_d: f64,
    /// Share of the elevator correction that is added to the held elevator each second.
    pub pitch_i: f64,
    /// Largest elevator correction, before the dynamic-pressure scaling.
    pub pitch_authority: f64,
    /// Trim travel per second for each unit of held elevator: the trim takes over what the
    /// yoke holds in some 20 s, so the aircraft is in trim when the autopilot lets go.
    pub trim_p: f64,
    /// Aileron per radian of bank error and per rad/s of roll rate: 20° of bank in about 3 s.
    pub roll_p: f64,
    pub roll_d: f64,
    /// Rudder per radian of sideslip and per rad/s of yaw rate: ball centred in turns.
    pub yaw_p: f64,
    pub yaw_d: f64,
    /// Seconds in which a course error is to be turned out: bank = atan(v · error / (g · time)).
    /// A 30° heading change takes about 15 s and overshoots less than 2°.
    pub turn_time: f64,
    /// How fast the wanted bank moves, rad/s.
    pub bank_rate: f64,
    /// A line is followed by aiming at the point this many seconds ahead on it, en route and
    /// in the circuit; with `turn_time` it gives a damping ratio near 0.8.
    pub line_time: f64,
    /// The same on final, where the centreline is held tighter.
    pub final_time: f64,
    /// Vertical speed per metre of altitude error, 1/s: the last 20 m take about 15 s.
    pub altitude_p: f64,
    /// The same on the glide path.
    pub glide_p: f64,
    /// Pitch per radian of climb-angle error, and its integral per second.
    pub path_p: f64,
    pub path_i: f64,
    /// Full-power climb: pitch per m/s of airspeed error and its integral per second.
    pub climb_p: f64,
    pub climb_i: f64,
    /// Throttle per m/s of airspeed error, its integral per second, and per radian of wanted
    /// climb angle: 20 kt are gained in about 25 s and held within 2 kt.
    pub throttle_p: f64,
    pub throttle_i: f64,
    pub throttle_climb: f64,
    /// Climb angle given up per m/s of airspeed lost with the throttle against its stop, or
    /// below 1.3 times the stall speed: airspeed comes before the path.
    pub protect: f64,
    /// Highest pitch allowed, lowered by this much per m/s once the airspeed is within
    /// 1.5 m/s of the lowest allowed: it settles at that speed however steep the climb asked.
    pub stall_guard: f64,
    /// On the ground: rudder per radian of heading error and per rad/s, and heading offered
    /// per metre off the centreline.
    pub steer_p: f64,
    pub steer_d: f64,
    pub steer_across: f64,
    /// Flare: sink rate wanted per metre of wheel height, 1/s, and at the ground, m/s.
    pub flare_p: f64,
    pub flare_sink: f64,
    /// Flare: pitch per m/s of sink more than wanted.
    pub flare_pitch: f64,
}

impl Default for Gains {
    fn default() -> Gains {
        Gains {
            pitch_p: 3.2,
            pitch_d: 0.9,
            pitch_i: 0.5,
            pitch_authority: 0.28,
            trim_p: 0.15,
            roll_p: 2.2,
            roll_d: 0.7,
            yaw_p: 4.0,
            yaw_d: 0.3,
            turn_time: 4.0,
            bank_rate: 10.0 * DEG,
            line_time: 9.0,
            final_time: 6.0,
            altitude_p: 0.15,
            glide_p: 0.25,
            path_p: 1.0,
            path_i: 0.4,
            climb_p: 0.02,
            climb_i: 0.004,
            throttle_p: 0.12,
            throttle_i: 0.02,
            throttle_climb: 5.0,
            protect: 0.02,
            stall_guard: 4.0 * DEG,
            steer_p: 6.0,
            steer_d: 2.0,
            steer_across: 0.06,
            flare_p: 0.2,
            flare_sink: 0.3,
            flare_pitch: 2.5 * DEG,
        }
    }
}

impl Gains {
    /// The gains for a type of aircraft: the default ones, or a transport's, which answers its
    /// yoke and its throttles far more slowly and is flown with a lighter hand.
    pub fn for_type(aircraft: &AircraftType) -> Gains {
        let d = Gains::default();
        if !Profile::heavy(aircraft) {
            return d;
        }
        Gains {
            pitch_p: 2.2,
            pitch_d: 1.8,
            pitch_i: 0.25,
            pitch_authority: 0.35,
            trim_p: 0.1,
            roll_p: 2.0,
            roll_d: 1.4,
            bank_rate: 5.0 * DEG,
            turn_time: 6.0,
            line_time: 14.0,
            final_time: 9.0,
            // Its engines take seconds to answer: the throttles are moved gently.
            throttle_p: 0.05,
            throttle_i: 0.008,
            throttle_climb: 4.0,
            flare_p: 0.16,
            flare_sink: 0.8,
            flare_pitch: 1.2 * DEG,
            ..d
        }
    }
}

/// The runway in use, as a frame: x along it from the landing threshold, y to the left (the
/// circuit side).
#[derive(Clone, Copy)]
struct Frame {
    n: f64,
    e: f64,
    course: f64,
    cos: f64,
    sin: f64,
    length: f64,
    elevation: f64,
}

impl Frame {
    fn new(a: &Airport, reversed: bool) -> Frame {
        let course = remainder(a.heading + if reversed { PI } else { 0.0 }, TAU);
        let (cos, sin) = (course.cos(), course.sin());
        Frame {
            n: a.n - cos * a.length / 2.0,
            e: a.e - sin * a.length / 2.0,
            course,
            cos,
            sin,
            length: a.length,
            elevation: a.elevation,
        }
    }

    fn local(&self, n: f64, e: f64) -> (f64, f64) {
        let (dn, de) = (n - self.n, e - self.e);
        (dn * self.cos + de * self.sin, dn * self.sin - de * self.cos)
    }

    fn world(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.n + x * self.cos + y * self.sin,
            self.e + x * self.sin - y * self.cos,
        )
    }

    /// The line through a local point in a direction relative to the runway's.
    fn line(&self, x: f64, y: f64, turn: f64, time: f64) -> Lateral {
        let (n, e) = self.world(x, y);
        Lateral::Line {
            n,
            e,
            course: self.course + turn,
            time,
        }
    }
}

#[derive(Clone, Copy)]
enum Lateral {
    Level,
    Heading(f64),
    /// The line through a point with a course, aimed at `time` seconds ahead.
    Line {
        n: f64,
        e: f64,
        course: f64,
        time: f64,
    },
    Direct(f64, f64),
    Bank(f64),
}

#[derive(Clone, Copy)]
enum Path {
    /// Towards an altitude, with a vertical speed fed forward and limits up and down.
    Altitude {
        target: f64,
        feed: f64,
        up: f64,
        down: f64,
        gain: f64,
    },
    Rate(f64),
    /// Full power; the pitch holds this airspeed.
    Climb(f64),
    /// Round out with the wheels this high: the nose comes up as the ground comes near.
    Flare(f64),
}

#[derive(Clone, Copy)]
enum Power {
    Speed(f64),
    Set(f64),
    Keep,
}

/// What a phase wants from the loops.
#[derive(Clone, Copy)]
struct Command {
    lateral: Lateral,
    bank_limit: f64,
    path: Path,
    pitch: (f64, f64),
    power: Power,
    flaps: i32,
    /// Lowest altitude allowed by the ground ahead (0 = not looked at).
    floor: f64,
    /// Flare: point the nose along this course with the rudder.
    align: Option<f64>,
    /// The airspeed never to go below, as a multiple of the stall speed.
    margin: f64,
    /// Bank added to what the line asks: the wing held down into a crosswind.
    bank_offset: f64,
    flaring: bool,
}

/// Flies the aircraft towards a `Goal`. Create it with the goal, hand it to `World::fly` every
/// step; change the goal with `set_goal`. Dropping it is disengaging: it keeps the aircraft in
/// trim, so the controls can be let go where they are.
#[derive(Clone, Debug)]
pub struct Autopilot {
    goal: Goal,
    phase: Phase,
    pub gains: Gains,
    /// The circuit flown by `Circuit` and `Land`.
    pub pattern: Pattern,
    /// Indicated airspeed flown along a route, m/s.
    pub cruise: f64,
    started: bool,

    pitch_cmd: f64,
    bank_cmd: f64,
    elevator_hold: f64,
    /// The elevator correction of the last decision, over `elevator_hold`.
    elevator_correction: f64,
    aileron: f64,
    alpha_hold: f64,
    climb_pitch: f64,
    climbing: bool,
    throttle_hold: f64,
    throttle: f64,

    leg: usize,
    from: (f64, f64),
    done_course: f64,
    done_altitude: f64,
    crosswind_x: f64,
    /// Angle between the nose and the runway when the flare began.
    crab: f64,
    /// Altitude to circle up to before going on, when the ground ahead is too steep.
    escape: Option<f64>,
    phase_time: f64,
    landings: u32,
    /// The aircraft sits on a tailwheel: after landing the stick is held back to keep the tail down.
    tailwheel: bool,
    /// The airspeeds it flies the aircraft at, its full flap angle (degrees) and the height of its
    /// centre of gravity with the wheels on the ground (m).
    speeds: Speeds,
    full_flap: f64,
    profile: Profile,
    wheel_height: f64,
}

/// What the loops and the circuit take from the type of aircraft besides its speeds: a light
/// aircraft's (`Profile::default`, the C172's and the Extra's) or a transport's.
#[derive(Clone, Debug, PartialEq)]
struct Profile {
    /// The indicated airspeed, kt, at which the yoke's deflections are the gains': they are scaled
    /// by (this / airspeed)², within these bounds for the elevator and up to this for the ailerons.
    control_speed: f64,
    elevator_power: (f64, f64),
    aileron_power: f64,
    /// How large the circuit is against a light aircraft's: what its joining distances are scaled by.
    size: f64,
    /// The pitch the nose is raised to at rotation and how fast, rad/s; the pitch a full-power
    /// climb is kept within.
    rotation: (f64, f64),
    climb_pitch: (f64, f64),
    /// How far a full-power climb's pitch goes from where it has settled for an airspeed error, and
    /// how many seconds before reaching the circuit's height it starts to level off.
    climb_trade: f64,
    level_off: f64,
    /// The most climb angle given to lose airspeed with the throttle closed, radians.
    slow_climb: f64,
    /// The flare: the pitch it starts from, and how much more nose up on the ground.
    flare_pitch: (f64, f64),
    /// The speed of the first climb, to where the circuit turns, if not the circuit's climb speed.
    initial_climb: Option<f64>,
    /// Flap notch on the downwind leg, level (one more coming down).
    downwind_flaps: i32,
    /// The highest indicated airspeed with each flap notch from the first out, m/s; empty: one
    /// speed for all (`Speeds::flaps`), and the flaps come up as soon as they are asked to.
    flap_speeds: Vec<f64>,
    /// Stall speed at each notch, m/s (with `flap_speeds`), and the notches' angles, degrees: the
    /// flaps come up a notch only at 1.25 times the stall speed of the notch they come up to.
    notch_stall: Vec<f64>,
    notch_deg: Vec<f64>,
    /// The least airspeed of the first climb, as a multiple of the stall speed (1.3 elsewhere):
    /// a transport leaves the runway at 1.2 times it.
    climbout_margin: f64,
    /// It has a gear to raise and lower, spoilers and reverse thrust for the runway, and wheel
    /// brakes to use on it.
    retracts: bool,
    spoilers: bool,
    reverse: bool,
    brakes: f64,
}

impl Default for Profile {
    fn default() -> Profile {
        Profile {
            control_speed: 80.0,
            elevator_power: (0.5, 2.5),
            aileron_power: 1.5,
            size: 1.0,
            rotation: (8.0 * DEG, 5.0 * DEG),
            climb_pitch: (2.0 * DEG, 11.0 * DEG),
            climb_trade: 4.0 * DEG,
            level_off: 0.0,
            slow_climb: f64::INFINITY,
            flare_pitch: (-2.0 * DEG, 7.0 * DEG),
            initial_climb: None,
            downwind_flaps: 0,
            flap_speeds: Vec::new(),
            notch_stall: Vec::new(),
            notch_deg: Vec::new(),
            climbout_margin: 1.3,
            retracts: false,
            spoilers: false,
            reverse: false,
            brakes: 0.0,
        }
    }
}

impl Profile {
    /// Whether a type is flown as a transport: a jet.
    fn heavy(aircraft: &AircraftType) -> bool {
        aircraft.jets.is_some()
    }

    fn for_type(aircraft: &AircraftType) -> Profile {
        if !Profile::heavy(aircraft) {
            return Profile::default();
        }
        let s = aircraft.speeds;
        // Its stall speed at each notch, as it flies them.
        let model = crate::aircraft::Aircraft::with_type(aircraft.clone());
        let sea_level = (crate::atmosphere::density(0.0) / crate::atmosphere::SEA_LEVEL_DENSITY).sqrt();
        Profile {
            // Its controls are sized for its speeds: as the light aircraft's, 1.5 times the clean stall.
            control_speed: 1.5 * s.stall_clean / KNOTS,
            elevator_power: (0.4, 2.5),
            aileron_power: 2.5,
            size: s.pattern / Pattern::default().pattern_speed,
            // Slowly, and no further: the tail is not far off the runway.
            rotation: (9.0 * DEG, 2.5 * DEG),
            // It keeps climbing as it gathers speed, and takes long to stop climbing.
            climb_pitch: (6.5 * DEG, 15.0 * DEG),
            climb_trade: 2.5 * DEG,
            level_off: 9.0,
            // It slows down level, its flaps and gear going out: it does not zoom to.
            slow_climb: 0.01,
            flare_pitch: (1.0 * DEG, 4.0 * DEG),
            // With its take-off flaps still out: twenty knots over the rotation speed.
            initial_climb: Some(s.rotate + 20.0 * KNOTS),
            downwind_flaps: (aircraft.max_flap_notch() / 3).max(1).min(aircraft.max_flap_notch()),
            flap_speeds: aircraft.flap_speeds.to_vec(),
            notch_stall: aircraft.flap_notches.iter().map(|&deg| model.stall(deg, 0.0).speed * sea_level).collect(),
            notch_deg: aircraft.flap_notches.to_vec(),
            climbout_margin: 1.15,
            retracts: aircraft.retract.is_some(),
            spoilers: aircraft.spoilers.is_some(),
            reverse: aircraft.jets.as_ref().is_some_and(|j| j.reverser.is_some()),
            brakes: 0.7,
        }
    }
}

fn slew(value: f64, target: f64, step: f64) -> f64 {
    value + clamp(target - value, -step, step)
}

fn wrap(angle: f64) -> f64 {
    remainder(angle, TAU)
}

/// Bank of a standard-rate turn (3°/s) at this speed.
fn standard_bank(speed: f64) -> f64 {
    (speed * 3.0 * DEG / GRAVITY).atan().min(25.0 * DEG)
}

/// Distance before a corner at which a turn of `angle` has to begin.
fn lead(speed: f64, bank: f64, angle: f64) -> f64 {
    let radius = speed * speed / (GRAVITY * bank.tan());
    radius * (angle.abs().min(150.0 * DEG) / 2.0).tan() + speed * 2.0
}

impl Autopilot {
    pub fn new(goal: Goal) -> Autopilot {
        let speeds = AircraftKind::C172.spec().speeds;
        Autopilot {
            goal,
            phase: Phase::Hold,
            gains: Gains::default(),
            pattern: Pattern::default(),
            cruise: speeds.cruise,
            started: false,
            pitch_cmd: 0.0,
            bank_cmd: 0.0,
            elevator_hold: 0.0,
            elevator_correction: 0.0,
            aileron: 0.0,
            alpha_hold: 0.0,
            climb_pitch: 0.0,
            climbing: false,
            throttle_hold: 0.0,
            throttle: 0.0,
            leg: 0,
            from: (0.0, 0.0),
            done_course: 0.0,
            done_altitude: 0.0,
            crosswind_x: 0.0,
            crab: 0.0,
            escape: None,
            phase_time: 0.0,
            landings: 0,
            tailwheel: false,
            speeds,
            full_flap: 30.0,
            wheel_height: 1.12,
            profile: Profile::default(),
        }
    }

    /// An autopilot for an aircraft of this type: `new` is for the C172.
    pub fn for_type(goal: Goal, aircraft: &AircraftType) -> Autopilot {
        Autopilot {
            tailwheel: aircraft.tailwheel(),
            speeds: aircraft.speeds,
            pattern: Pattern::for_type(aircraft),
            cruise: aircraft.speeds.cruise,
            full_flap: aircraft.flap_notches.last().copied().unwrap_or(0.0),
            wheel_height: aircraft.cg_height,
            profile: Profile::for_type(aircraft),
            gains: Gains::for_type(aircraft),
            ..Autopilot::new(goal)
        }
    }

    /// Stall speed with this much flap, m/s.
    fn stall_speed(&self, flap_deg: f64) -> f64 {
        // A transport's, by its notches: the first degrees of flap, with its leading edge's, do most of it.
        if !self.profile.notch_stall.is_empty() {
            return crate::math::interpolate(&self.profile.notch_deg, &self.profile.notch_stall, flap_deg);
        }
        let share = if self.full_flap > 0.0 { clamp(flap_deg / self.full_flap, 0.0, 1.0) } else { 0.0 };
        self.speeds.stall_clean + (self.speeds.stall_full - self.speeds.stall_clean) * share
    }

    pub fn goal(&self) -> &Goal {
        &self.goal
    }

    /// Changes the goal. Between two holds only the targets change; anything else starts the
    /// new goal from wherever the aircraft is.
    pub fn set_goal(&mut self, goal: Goal) {
        let both_hold = matches!(self.goal, Goal::Hold { .. }) && matches!(goal, Goal::Hold { .. });
        self.goal = goal;
        if !both_hold {
            self.started = false;
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Takes the controls as they are as its own, as if it had moved them: for an autopilot
    /// that watches someone else fly (an instructor's hands on the controls without moving them),
    /// so that what it would do next starts from where yoke and throttle really are. Call it
    /// before `fly`; after an aircraft it flew itself it changes nothing.
    pub fn follow(&mut self, current: &AircraftControls) {
        self.elevator_hold = clamp(current.elevator - self.elevator_correction, -1.0, 1.0);
        self.aileron = current.aileron;
        self.throttle = current.throttle;
    }

    /// Index of the waypoint being flown to on a route.
    pub fn leg(&self) -> usize {
        self.leg
    }

    /// Landings completed (stopped on the ground) since it was created.
    pub fn landings(&self) -> u32 {
        self.landings
    }

    /// Nothing left to do: stopped after landing with no further circuit, or past the last waypoint.
    pub fn finished(&self) -> bool {
        match self.phase {
            Phase::RouteDone => true,
            Phase::Stopped => !self.repeats() || self.phase_time > 6.0,
            _ => false,
        }
    }

    fn repeats(&self) -> bool {
        matches!(self.goal, Goal::Circuit { repeat: true, .. })
    }

    /// The runway as landed on.
    fn frame(&self) -> Option<Frame> {
        match &self.goal {
            Goal::Circuit {
                airport, reversed, ..
            } => Some(Frame::new(airport, *reversed != self.pattern.land_reversed)),
            Goal::Land { airport, reversed } => Some(Frame::new(airport, *reversed)),
            _ => None,
        }
    }

    /// The runway as taken off from.
    fn takeoff_frame(&self) -> Option<Frame> {
        match &self.goal {
            Goal::Circuit {
                airport, reversed, ..
            } => Some(Frame::new(airport, *reversed)),
            _ => self.frame(),
        }
    }

    fn lands_reversed(&self) -> bool {
        self.pattern.land_reversed && matches!(self.goal, Goal::Circuit { .. })
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_time = 0.0;
    }

    /// Takes the aircraft as it is: the loops start from the present attitude and controls.
    fn start(&mut self, obs: &Observation, current: &AircraftControls) {
        self.started = true;
        let ground_speed = obs.velocity[0].hypot(obs.velocity[1]).max(10.0);
        self.pitch_cmd = obs.pitch;
        self.bank_cmd = obs.roll;
        self.elevator_hold = current.elevator;
        self.aileron = current.aileron;
        self.alpha_hold = obs.pitch - (-obs.velocity[2]).atan2(ground_speed);
        self.climb_pitch = obs.pitch;
        self.climbing = false;
        self.throttle_hold = current.throttle;
        self.throttle = current.throttle;
        self.leg = 0;
        self.escape = None;
        self.from = (obs.north, obs.east);
        let phase = match &self.goal {
            Goal::Hold { .. } => Phase::Hold,
            Goal::Route(points) => {
                self.done_course = obs.heading;
                self.done_altitude = obs.altitude;
                if points.is_empty() {
                    Phase::RouteDone
                } else {
                    Phase::Route
                }
            }
            Goal::Circuit { .. } | Goal::Land { .. } => {
                let f = self.frame().expect("a circuit has a runway");
                let (x, y) = f.local(obs.north, obs.east);
                if obs.on_ground || obs.agl < self.wheel_height + 0.5 {
                    self.elevator_hold = 0.0;
                    Phase::Takeoff
                } else if x < -400.0
                    && y.abs() < 150.0 - x * 0.1
                    && wrap(obs.heading - f.course).abs() < 30.0 * DEG
                    && obs.altitude < f.elevation + self.pattern.height + 60.0
                {
                    Phase::Final
                } else {
                    Phase::Join
                }
            }
        };
        self.enter(phase);
    }

    // ---- The state machine

    fn advance_route(&mut self, obs: &Observation) {
        let Goal::Route(points) = &self.goal else {
            return;
        };
        let wp = points[self.leg];
        let course = (wp.east - self.from.1).atan2(wp.north - self.from.0);
        let remaining = (wp.north - obs.north) * course.cos() + (wp.east - obs.east) * course.sin();
        let ground_speed = obs.velocity[0].hypot(obs.velocity[1]);
        let anticipation = match points.get(self.leg + 1) {
            Some(next) => {
                let after = (next.east - wp.east).atan2(next.north - wp.north);
                let length = (next.east - wp.east).hypot(next.north - wp.north);
                lead(ground_speed, standard_bank(obs.speed), wrap(after - course)).min(length * 0.5)
            }
            None => 0.0,
        };
        if remaining < anticipation {
            self.from = (wp.north, wp.east);
            self.leg += 1;
            if self.leg == points.len() {
                self.done_course = course;
                self.done_altitude = wp.altitude;
                self.enter(Phase::RouteDone);
            }
        }
    }

    fn advance_circuit(&mut self, obs: &Observation, f: &Frame) {
        let (x, y) = f.local(obs.north, obs.east);
        let ground_speed = obs.velocity[0].hypot(obs.velocity[1]);
        let track = obs.velocity[1].atan2(obs.velocity[0]);
        let p = &self.pattern;
        let pattern = f.elevation + p.height;
        let corner = lead(ground_speed, p.bank - 3.0 * DEG, PI / 2.0);
        let reversed = self.lands_reversed();
        let wheels = obs.agl - self.wheel_height;
        match self.phase {
            Phase::Takeoff if !obs.on_ground && wheels > 2.0 => {
                self.climb_pitch = self.pitch_cmd;
                self.climbing = true;
                self.enter(Phase::Climbout);
            }
            Phase::Climbout
                if reversed && obs.altitude > f.elevation + p.turn_height && x < -p.upwind =>
            {
                self.enter(Phase::Downwind);
            }
            Phase::Climbout
                if obs.altitude > f.elevation + p.turn_height && x > f.length + p.upwind =>
            {
                self.crosswind_x = x + corner;
                self.enter(Phase::Crosswind);
            }
            Phase::Crosswind if y > p.width - corner => self.enter(Phase::Downwind),
            Phase::Join | Phase::Orbit if obs.on_ground => self.started = false,
            Phase::Join => {
                let (n, e) = f.world(JOIN_AT, p.width);
                let size = self.profile.size;
                let near = (n - obs.north).hypot(e - obs.east) < 700.0 * size;
                let along = wrap(track - f.course - PI).abs() < 45.0 * DEG;
                let on_leg = (y - p.width).abs() < 500.0 * size
                    && x > 800.0 - p.final_length
                    && x < JOIN_AT + 700.0 * size
                    && along;
                if obs.altitude > pattern + 60.0 {
                    if near {
                        self.enter(Phase::Orbit);
                    }
                } else if near || on_leg {
                    self.enter(Phase::Downwind);
                }
            }
            Phase::Orbit
                if obs.altitude < pattern + 40.0
                    && wrap(track - f.course - PI).abs() < 20.0 * DEG =>
            {
                self.enter(Phase::Downwind)
            }
            Phase::Downwind if x < corner - p.final_length => self.enter(Phase::Base),
            Phase::Base if y < corner => self.enter(Phase::Final),
            Phase::Final if wheels < self.pattern.flare_height && x > -80.0 => {
                self.crab = wrap(obs.heading - f.course);
                self.enter(Phase::Flare);
            }
            Phase::Flare if obs.on_ground => self.enter(Phase::Rollout),
            Phase::Rollout if obs.on_ground && obs.speed < 0.5 => {
                self.landings += 1;
                self.enter(Phase::Stopped);
            }
            Phase::Stopped
                if self.repeats() && self.phase_time > 5.0 && f.length - x > TAKEOFF_ROOM
                    && !reversed =>
            {
                self.elevator_hold = 0.0;
                self.pitch_cmd = obs.pitch;
                self.enter(Phase::Takeoff);
            }
            _ => {}
        }
    }

    /// What the present phase wants while in the air.
    fn command(&self, obs: &Observation) -> Command {
        let g = &self.gains;
        let ahead = obs.ground_ahead;
        let floor = ahead.iter().fold(0.0_f64, |m, h| m.max(*h)) + 100.0;
        let standard = standard_bank(obs.speed);
        let level = |target: f64| Path::Altitude {
            target,
            feed: 0.0,
            up: 3.0,
            down: 2.5,
            gain: g.altitude_p,
        };
        let cruise = Command {
            lateral: Lateral::Level,
            bank_limit: standard,
            path: Path::Rate(0.0),
            pitch: (-10.0 * DEG, 12.0 * DEG),
            power: Power::Keep,
            flaps: 0,
            floor,
            align: None,
            margin: 1.3,
            bank_offset: 0.0,
            flaring: false,
        };
        let cmd = match self.phase {
            Phase::Hold => {
                let Goal::Hold {
                    heading,
                    vertical,
                    airspeed,
                } = &self.goal
                else {
                    return cruise;
                };
                Command {
                    lateral: heading.map_or(Lateral::Level, Lateral::Heading),
                    path: match *vertical {
                        Vertical::Altitude(a) => level(a),
                        Vertical::Speed(v) => Path::Rate(v),
                    },
                    power: airspeed.map_or(Power::Keep, Power::Speed),
                    ..cruise
                }
            }
            Phase::Route => {
                let Goal::Route(points) = &self.goal else {
                    return cruise;
                };
                let wp = points[self.leg];
                Command {
                    lateral: Lateral::Line {
                        n: wp.north,
                        e: wp.east,
                        course: (wp.east - self.from.1).atan2(wp.north - self.from.0),
                        time: g.line_time,
                    },
                    path: level(wp.altitude),
                    power: Power::Speed(self.cruise),
                    ..cruise
                }
            }
            Phase::RouteDone => Command {
                lateral: Lateral::Heading(self.done_course),
                path: level(self.done_altitude),
                power: Power::Speed(self.cruise),
                ..cruise
            },
            _ => self.circuit_command(obs, cruise),
        };
        match self.escape {
            // Ground ahead that a straight climb would not clear: circle up at full power.
            Some(_) => Command {
                lateral: Lateral::Bank(-20.0 * DEG),
                bank_limit: 20.0 * DEG,
                path: Path::Climb(self.speeds.climb),
                power: Power::Set(1.0),
                flaps: 0,
                ..cmd
            },
            None => cmd,
        }
    }

    /// Decides whether the ground ahead can be cleared by climbing straight on (6 % is what a
    /// loaded 172 is sure to make); if not, `escape` holds the altitude to circle up to.
    fn watch_terrain(&mut self, obs: &Observation) {
        let watched = matches!(
            self.phase,
            Phase::Hold | Phase::Route | Phase::RouteDone | Phase::Join
        );
        if !watched || obs.on_ground {
            self.escape = None;
            return;
        }
        match self.escape {
            Some(altitude) => {
                if obs.altitude >= altitude {
                    self.escape = None;
                }
            }
            None => {
                let starts = [0.0, GROUND_AHEAD[0], GROUND_AHEAD[1], GROUND_AHEAD[2]];
                let mut need: Option<f64> = None;
                for (ground, start) in obs.ground_ahead.iter().zip(starts) {
                    if ground + 50.0 > obs.altitude + 0.06 * start {
                        need = Some(need.unwrap_or(0.0).max(ground + 100.0));
                    }
                }
                self.escape = need;
            }
        }
    }

    fn circuit_command(&self, obs: &Observation, cruise: Command) -> Command {
        let g = &self.gains;
        let p = &self.pattern;
        let Some(f) = self.frame() else {
            return cruise;
        };
        let (x, y) = f.local(obs.north, obs.east);
        let ground_speed = obs.velocity[0].hypot(obs.velocity[1]);
        let pattern = f.elevation + p.height + self.wheel_height;
        let wheels = obs.agl - self.wheel_height;
        let ahead = obs.ground_ahead;
        let floor = ahead[0].max(ahead[1]).max(ahead[2]) + 60.0;
        let circuit = Command {
            bank_limit: p.bank,
            floor,
            ..cruise
        };
        // Climbing to circuit height at full power, then level.
        let rising = (-obs.velocity[2]).max(0.0) * self.profile.level_off;
        let up_to_pattern = if obs.altitude + rising < pattern - 12.0 && self.climbing {
            (Path::Climb(p.climb_speed), Power::Set(1.0))
        } else {
            (
                Path::Altitude {
                    target: pattern,
                    feed: 0.0,
                    up: 3.0,
                    down: 3.5,
                    gain: g.altitude_p,
                },
                Power::Speed(p.pattern_speed),
            )
        };
        // Coming down so as to meet the glide path where the final begins: the height the
        // 3° path has at the distance still to fly, less the corners that are cut.
        let glide = p.glide.tan();
        let descent = |to_fly: f64| Path::Altitude {
            target: (f.elevation + self.wheel_height + (to_fly - 500.0) * glide)
                .min(pattern)
                .max(f.elevation + self.wheel_height + (p.final_length + p.aim - 300.0) * glide),
            feed: 0.0,
            up: 3.0,
            down: 3.5,
            gain: g.altitude_p,
        };
        match self.phase {
            Phase::Climbout => Command {
                lateral: f.line(0.0, 0.0, if self.lands_reversed() { PI } else { 0.0 }, g.line_time),
                bank_limit: if obs.agl < 30.0 {
                    8.0 * DEG
                } else {
                    15.0 * DEG
                },
                path: Path::Climb(self.profile.initial_climb.unwrap_or(p.climb_speed)),
                pitch: (3.0 * DEG, self.profile.climb_pitch.1 + 1.0 * DEG),
                power: Power::Set(1.0),
                flaps: if obs.agl < 60.0 { p.takeoff_flaps } else { 0 },
                floor: 0.0,
                margin: self.profile.climbout_margin,
                ..circuit
            },
            Phase::Crosswind => Command {
                lateral: f.line(self.crosswind_x, 0.0, -PI / 2.0, g.line_time),
                path: up_to_pattern.0,
                power: up_to_pattern.1,
                ..circuit
            },
            Phase::Join => {
                let (n, e) = f.world(JOIN_AT, p.width);
                Command {
                    lateral: Lateral::Direct(n, e),
                    path: Path::Altitude {
                        target: pattern,
                        feed: 0.0,
                        up: 3.0,
                        down: 3.5,
                        gain: g.altitude_p,
                    },
                    power: Power::Speed(self.speeds.join),
                    ..circuit
                }
            }
            Phase::Orbit => Command {
                lateral: Lateral::Bank(-18.0 * DEG),
                path: Path::Altitude {
                    target: pattern,
                    feed: 0.0,
                    up: 3.0,
                    down: 3.5,
                    gain: g.altitude_p,
                },
                power: Power::Speed(self.speeds.join),
                ..circuit
            },
            Phase::Downwind => {
                let to_fly = (x + p.final_length).max(0.0) + p.width + p.final_length + p.aim;
                let descending = to_fly - 500.0 < p.height / glide;
                let (path, power) = if descending {
                    (descent(to_fly), Power::Speed(p.descent_speed))
                } else {
                    up_to_pattern
                };
                Command {
                    lateral: f.line(0.0, p.width, PI, g.line_time),
                    path,
                    power,
                    flaps: self.profile.downwind_flaps + i32::from(descending),
                    ..circuit
                }
            }
            Phase::Base => Command {
                lateral: f.line(-p.final_length, 0.0, PI / 2.0, g.line_time),
                path: descent(y.max(0.0) + p.final_length + p.aim),
                power: Power::Speed(p.base_speed),
                flaps: p.landing_flaps,
                margin: 1.2,
                floor: ahead[0].max(ahead[1]) + 45.0,
                ..circuit
            },
            Phase::Final => Command {
                lateral: f.line(0.0, 0.0, 0.0, g.final_time),
                bank_limit: if wheels < 15.0 { 8.0 * DEG } else { 15.0 * DEG },
                path: Path::Altitude {
                    target: f.elevation + self.wheel_height + (p.aim - x).max(0.0) * glide,
                    feed: -ground_speed * glide,
                    up: 1.5,
                    down: 4.5,
                    gain: g.glide_p,
                },
                power: Power::Speed(p.final_speed),
                flaps: p.landing_flaps,
                margin: 1.2,
                floor: 0.0,
                ..circuit
            },
            // Flare, and a bounce during the rollout.
            _ => Command {
                lateral: f.line(0.0, 0.0, 0.0, g.final_time),
                bank_limit: 8.0 * DEG,
                bank_offset: if wheels < 3.5 { 0.6 * self.crab } else { 0.0 },
                path: Path::Flare(wheels),
                pitch: (-3.0 * DEG, 9.0 * DEG),
                power: Power::Set(0.0),
                flaps: p.landing_flaps,
                floor: 0.0,
                align: if wheels < 3.5 { Some(f.course) } else { None },
                flaring: true,
                ..circuit
            },
        }
    }

    // ---- The loops

    fn elevator(&mut self, obs: &Observation, target: f64, rate: f64, dt: f64) -> f64 {
        let g = &self.gains;
        let ias = (aircraft(obs).indicated_airspeed / KNOTS).max(1.0);
        let reference = self.profile.control_speed;
        let power = clamp((reference / ias) * (reference / ias), self.profile.elevator_power.0, self.profile.elevator_power.1);
        self.pitch_cmd = slew(self.pitch_cmd, target, rate * dt);
        let limit = g.pitch_authority * power;
        let correction = clamp(
            ((self.pitch_cmd - obs.pitch) * g.pitch_p - obs.angular_rates[1] * g.pitch_d) * power,
            -limit,
            limit,
        );
        self.elevator_correction = correction;
        self.elevator_hold = clamp(self.elevator_hold + correction * g.pitch_i * dt, -1.0, 1.0);
        clamp(self.elevator_hold + correction, -1.0, 1.0)
    }

    fn ailerons(&mut self, obs: &Observation, bank: f64, dt: f64) -> f64 {
        let g = &self.gains;
        let ias = (aircraft(obs).indicated_airspeed / KNOTS).max(1.0);
        let reference = self.profile.control_speed;
        let power = clamp((reference / ias) * (reference / ias), 0.5, self.profile.aileron_power);
        self.bank_cmd = slew(self.bank_cmd, bank, g.bank_rate * dt);
        let wanted = clamp(
            ((self.bank_cmd - obs.roll) * g.roll_p - obs.angular_rates[0] * g.roll_d) * power,
            -0.7,
            0.7,
        );
        self.aileron = slew(self.aileron, wanted, 4.0 * dt);
        self.aileron
    }

    fn coordinated(&self, obs: &Observation) -> f64 {
        clamp(
            aircraft(obs).beta * self.gains.yaw_p - obs.angular_rates[2] * self.gains.yaw_d,
            -0.6,
            0.6,
        )
    }

    /// Pedals that keep the wheels on the runway centreline.
    fn steer(&self, obs: &Observation, f: &Frame) -> f64 {
        let g = &self.gains;
        let (_, y) = f.local(obs.north, obs.east);
        let error = wrap(f.course + clamp(y * g.steer_across, -0.15, 0.15) - obs.heading);
        clamp(
            error * g.steer_p - obs.angular_rates[2] * g.steer_d,
            -1.0,
            1.0,
        ) * smoothstep(1.0, 4.0, obs.speed)
    }

    /// The flap notch to select, towards the one wanted. A light aircraft's go out below their
    /// speed and come up when asked. A transport's a notch at a time: out below that notch's
    /// placard speed, up once fast enough for the notch it comes up to.
    fn flaps(&self, wanted: i32, now: i32, ias: f64) -> i32 {
        let p = &self.profile;
        if p.flap_speeds.is_empty() {
            return if wanted > now && ias > self.speeds.flaps { now } else { wanted };
        }
        if wanted > now {
            let limit = p.flap_speeds.get(now as usize).copied().unwrap_or(0.0);
            if ias < limit - 2.5 { now + 1 } else { now }
        } else if wanted < now {
            let stall = p.notch_stall.get((now - 1).max(0) as usize).copied().unwrap_or(0.0);
            if ias > 1.25 * stall { now - 1 } else { now }
        } else {
            now
        }
    }

    fn trimmed(&self, current: &AircraftControls, dt: f64) -> f64 {
        let rate = clamp(self.elevator_hold * self.gains.trim_p, -0.04, 0.04);
        clamp(current.trim + rate * dt, -1.0, 1.0)
    }

    /// Takeoff run, rotation and the first metres of the climb.
    fn takeoff(&mut self, obs: &Observation, current: &AircraftControls, f: &Frame, dt: f64) -> AircraftControls {
        let rotating = aircraft(obs).indicated_airspeed > self.pattern.rotate_speed;
        let elevator = if rotating {
            let (pitch, rate) = self.profile.rotation;
            self.elevator(obs, pitch, rate, dt)
        } else {
            self.pitch_cmd = obs.pitch;
            self.elevator_hold = 0.0;
            0.0
        };
        let (aileron, rudder) = if obs.on_ground {
            self.bank_cmd = 0.0;
            self.aileron = slew(self.aileron, 0.0, 2.0 * dt);
            (self.aileron, self.steer(obs, f))
        } else {
            (self.ailerons(obs, 0.0, dt), self.coordinated(obs))
        };
        self.throttle = slew(current.throttle, 1.0, 0.6 * dt);
        self.throttle_hold = self.throttle;
        AircraftControls {
            aileron,
            elevator,
            rudder,
            throttle: self.throttle,
            flap_notch: self.pattern.takeoff_flaps,
            trim: slew(current.trim, 0.0, 0.05 * dt),
            ..*current
        }
    }

    /// On the runway after landing: straight, throttle closed (the wheels brake by themselves).
    fn rollout(&mut self, obs: &Observation, current: &AircraftControls, f: &Frame, dt: f64) -> AircraftControls {
        self.bank_cmd = 0.0;
        self.pitch_cmd = obs.pitch;
        self.aileron = slew(self.aileron, 0.0, 2.0 * dt);
        // On a tailwheel the stick is neutral while it could still fly (pulled then, it would leave
        // the ground again and stall), and goes right back as it slows, to keep the tail down.
        let stall = self.speeds.stall_clean;
        let back = if self.tailwheel { smoothstep(0.9 * stall, 0.6 * stall, aircraft(obs).indicated_airspeed) } else { 0.0 };
        self.elevator_hold = slew(self.elevator_hold, back, if self.tailwheel { 1.0 } else { 0.3 } * dt);
        // A transport stops with its spoilers up, reverse thrust down to 60 kt and its brakes.
        let p = &self.profile;
        let rolling = self.phase == Phase::Rollout;
        let reverse = p.reverse && rolling && aircraft(obs).indicated_airspeed > 60.0 * KNOTS;
        self.throttle = if reverse { slew(current.throttle, 1.0, 0.5 * dt) } else { 0.0 };
        self.throttle_hold = 0.0;
        let brake = if rolling { p.brakes } else { 0.0 };
        AircraftControls {
            aileron: self.aileron,
            elevator: self.elevator_hold,
            rudder: self.steer(obs, f),
            throttle: self.throttle,
            flap_notch: if self.phase == Phase::Stopped {
                0
            } else {
                current.flap_notch
            },
            reverse,
            spoilers: if p.spoilers && rolling { 1.0 } else { 0.0 },
            brake_left: if p.brakes > 0.0 { brake } else { current.brake_left },
            brake_right: if p.brakes > 0.0 { brake } else { current.brake_right },
            ..*current
        }
    }

    fn airborne(
        &mut self,
        obs: &Observation,
        current: &AircraftControls,
        cmd: &Command,
        dt: f64,
    ) -> AircraftControls {
        let g = self.gains.clone();
        let ias = aircraft(obs).indicated_airspeed;
        let ground_speed = obs.velocity[0].hypot(obs.velocity[1]).max(10.0);
        let track = obs.velocity[1].atan2(obs.velocity[0]);
        let turn = |error: f64| (ground_speed * wrap(error) / (GRAVITY * g.turn_time)).atan();

        // Lateral: the bank that turns out the course error.
        let bank = match cmd.lateral {
            Lateral::Level => 0.0,
            Lateral::Bank(bank) => bank,
            Lateral::Heading(heading) => turn(heading - obs.heading),
            Lateral::Direct(n, e) => turn((e - obs.east).atan2(n - obs.north) - track),
            Lateral::Line { n, e, course, time } => {
                let across = -(obs.north - n) * course.sin() + (obs.east - e) * course.cos();
                let intercept = clamp(
                    (-across / (ground_speed * time)).atan(),
                    -45.0 * DEG,
                    45.0 * DEG,
                );
                turn(course + intercept - track)
            }
        };
        let bank = clamp(bank + cmd.bank_offset, -cmd.bank_limit, cmd.bank_limit);
        let aileron = self.ailerons(obs, bank, dt);

        // Vertical: the pitch that gives the climb angle, or the airspeed in a full-power climb.
        let flaring = cmd.flaring;
        let gamma = (-obs.velocity[2]).atan2(ground_speed);
        let floor_speed = cmd.margin * self.stall_speed(aircraft(obs).flap_deg);
        // Near the lowest airspeed the nose is not allowed up, whatever the path asks.
        let pitch_high = if flaring {
            cmd.pitch.1
        } else {
            (cmd.pitch.1 - g.stall_guard * (floor_speed + 1.5 - ias).max(0.0)).max(-8.0 * DEG)
        };
        let mut gamma_cmd;
        let pitch = match cmd.path {
            Path::Climb(speed) => {
                if !self.climbing {
                    self.climbing = true;
                    self.climb_pitch = self.pitch_cmd;
                }
                let error = ias - speed.max(floor_speed + 1.0);
                self.climb_pitch = clamp(
                    self.climb_pitch + clamp(error * g.climb_i, -0.6 * DEG, 0.6 * DEG) * dt,
                    self.profile.climb_pitch.0,
                    self.profile.climb_pitch.1,
                );
                gamma_cmd = 0.08;
                self.climb_pitch + clamp(error * g.climb_p, -self.profile.climb_trade, self.profile.climb_trade)
            }
            Path::Flare(wheels) => {
                let sink = -(g.flare_sink + g.flare_p * wheels.max(0.0));
                gamma_cmd = 0.0;
                self.profile.flare_pitch.0
                    + self.profile.flare_pitch.1 * clamp(1.0 - wheels / self.pattern.flare_height, 0.0, 1.0)
                    + g.flare_pitch * (sink + obs.velocity[2])
            }
            Path::Altitude { .. } | Path::Rate(_) => {
                if self.climbing {
                    self.climbing = false;
                    self.alpha_hold = obs.pitch - gamma;
                }
                let rate = match cmd.path {
                    Path::Altitude {
                        target,
                        feed,
                        up,
                        down,
                        gain,
                    } => clamp(
                        feed + gain * (target.max(cmd.floor) - obs.altitude),
                        -down,
                        up,
                    ),
                    Path::Rate(rate) if cmd.floor > obs.altitude => {
                        rate.max(g.altitude_p * (cmd.floor - obs.altitude)).min(3.0)
                    }
                    Path::Rate(rate) => rate,
                    Path::Climb(_) | Path::Flare(_) => 0.0,
                };
                gamma_cmd = clamp(rate / ground_speed, -0.2, 0.2);
                if !flaring {
                    // Airspeed before path: with the throttle against a stop, or near the stall
                    // or the yellow arc, the climb angle gives way.
                    if let Power::Speed(speed) = cmd.power {
                        if self.throttle > 0.95 {
                            gamma_cmd -= g.protect * (speed - 0.8 - ias).max(0.0);
                        }
                        if self.throttle < 0.05 {
                            gamma_cmd += (g.protect * (ias - speed - 0.8).max(0.0)).min(self.profile.slow_climb);
                        }
                    }
                    gamma_cmd -= 2.0 * g.protect * (floor_speed - ias).max(0.0);
                    gamma_cmd += 2.0 * g.protect * (ias - self.speeds.most - 1.5).max(0.0);
                }
                let error = gamma_cmd - gamma;
                // The integral waits while the pitch is against a limit.
                if (self.pitch_cmd < pitch_high - 0.2 * DEG || error < 0.0)
                    && (self.pitch_cmd > cmd.pitch.0 + 0.2 * DEG || error > 0.0)
                {
                    self.alpha_hold = clamp(
                        self.alpha_hold + error * g.path_i * dt,
                        -6.0 * DEG,
                        14.0 * DEG,
                    );
                }
                // A banked wing needs more angle of attack for the same lift.
                let banked = 0.08 * (1.0 / obs.roll.cos().max(0.7) - 1.0);
                gamma_cmd + self.alpha_hold + error * g.path_p + banked
            }
        };
        let rate = if flaring { 6.0 * DEG } else { 4.0 * DEG };
        let elevator = self.elevator(obs, clamp(pitch, cmd.pitch.0, pitch_high), rate, dt);

        // Power.
        let throttle = match cmd.power {
            Power::Keep => current.throttle,
            Power::Set(value) => {
                let rate = if cmd.flaring { 0.3 } else { 0.5 };
                self.throttle_hold = slew(self.throttle, value, rate * dt);
                self.throttle_hold
            }
            Power::Speed(speed) => {
                let speed = clamp(speed, floor_speed + 0.5, self.speeds.most);
                let error = speed - ias;
                let wanted =
                    self.throttle_hold + error * g.throttle_p + gamma_cmd * g.throttle_climb;
                // The integral stops while the throttle is against a stop.
                if (wanted < 1.0 || error < 0.0) && (wanted > 0.0 || error > 0.0) {
                    self.throttle_hold =
                        clamp(self.throttle_hold + error * g.throttle_i * dt, 0.0, 1.0);
                }
                slew(self.throttle, clamp(wanted, 0.0, 1.0), 0.7 * dt)
            }
        };
        self.throttle = throttle;

        let rudder = match cmd.align {
            Some(course) => clamp(
                wrap(course - obs.heading) * 3.0 - obs.angular_rates[2] * 1.0,
                -0.7,
                0.7,
            ),
            None => self.coordinated(obs),
        };
        let flap_notch = self.flaps(cmd.flaps, current.flap_notch, ias);
        AircraftControls {
            aileron,
            elevator,
            rudder,
            throttle,
            flap_notch,
            trim: self.trimmed(current, dt),
            ..*current
        }
    }
}

impl Autopilot {
    /// `Pilot::fly` for an aircraft.
    pub fn fly_aircraft(&mut self, obs: &Observation, current: &AircraftControls, dt: f64) -> AircraftControls {
        if obs.crashed.is_some() {
            return AircraftControls {
                throttle: 0.0,
                ..*current
            };
        }
        if !self.started {
            self.start(obs, current);
        }
        self.phase_time += dt;
        let frame = self.frame();
        match (self.phase, frame) {
            (Phase::Route, _) => self.advance_route(obs),
            (_, Some(f)) => self.advance_circuit(obs, &f),
            _ => {}
        }
        self.watch_terrain(obs);
        let controls = self.fly_phase(obs, current, frame, dt);
        // In a circuit a gear that retracts comes up once climbing away, and goes down for the base leg.
        if self.profile.retracts && frame.is_some() {
            let up = match self.phase {
                Phase::Climbout => current.gear_up || (obs.agl - self.wheel_height > 15.0 && obs.velocity[2] < -1.0),
                Phase::Crosswind | Phase::Join | Phase::Orbit | Phase::Downwind => true,
                _ => false,
            };
            return AircraftControls { gear_up: up && !obs.on_ground, ..controls };
        }
        controls
    }

    fn fly_phase(&mut self, obs: &Observation, current: &AircraftControls, frame: Option<Frame>, dt: f64) -> AircraftControls {
        match (self.phase, frame) {
            (Phase::Takeoff, Some(_)) => {
                let t = self.takeoff_frame().expect("a circuit has a runway");
                self.takeoff(obs, current, &t, dt)
            }
            (Phase::Rollout | Phase::Stopped, Some(f)) if obs.on_ground => {
                self.rollout(obs, current, &f, dt)
            }
            // A hold or a route has nothing to do on the ground.
            (Phase::Hold | Phase::Route | Phase::RouteDone, _) if obs.on_ground => *current,
            _ => {
                let cmd = self.command(obs);
                self.airborne(obs, current, &cmd, dt)
            }
        }
    }
}

impl Pilot for Autopilot {
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls {
        match (current, obs.aircraft()) {
            (Controls::Aircraft(c), Some(_)) => Controls::Aircraft(self.fly_aircraft(obs, c, dt)),
            // Not an aircraft: nothing it can fly.
            _ => *current,
        }
    }
}

/// What the aircraft's pilot sees of it: only ever asked through `fly_aircraft`, which `fly` calls
/// for an aircraft alone.
fn aircraft(obs: &Observation) -> &crate::aircraft::AircraftObservation {
    obs.aircraft().expect("an aircraft's pilot flies an aircraft")
}
