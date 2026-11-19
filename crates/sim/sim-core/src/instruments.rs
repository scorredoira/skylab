//! The instruments: what the pilot reads, moved by the pressures, the gyroscopes and the magnet
//! that drive the real ones, never by the true state of the flight.
//!
//! - **Pitot-static.** The pitot tube feeds the total pressure of the air it faces into a line;
//!   the static ports feed the pressure round the fuselage into another. The airspeed indicator
//!   shows the difference of the two (calibrated for the standard sea level: indicated airspeed),
//!   the altimeter the static line against its Kollsman setting, the vertical speed indicator the
//!   static line against its own case, which follows it through a calibrated leak and so lags it.
//!   The ports sit where the fuselage's flow is not quite ambient: the error with angle of attack
//!   and flaps is the type's airspeed calibration, a single port feels sideslip.
//! - **Failures** (`Failures`): a pitot blocked at its inlet (the drain bleeds the line to static:
//!   the airspeed falls to nothing) or sealed (ice in the inlet and the drain: the airspeed indicator
//!   becomes an altimeter), the static ports blocked (altimeter and vertical speed frozen, airspeed
//!   wrong as soon as the altitude changes), the vacuum pump gone, icing. The alternate static
//!   source takes the cabin's pressure; pitot heat melts the ice.
//! - **Vacuum and gyros.** The engine-driven pump's suction spins the attitude and heading gyros.
//!   A spinning rotor keeps its axis in space: the attitude gyro is erected towards the felt
//!   vertical a few degrees a minute (and so leans after long turns and accelerations), and tumbles
//!   on its pitch stops; the heading gyro drifts and has to be set to the compass. When the suction
//!   goes the rotors run down over minutes, the case drags them round and the horizon wanders off.
//! - **Compass.** A magnet on a pendulous card in fluid: it settles along the Earth's field as seen
//!   in the plane of the card, which hangs along the felt vertical. The dip of the field then gives
//!   the turning errors (it lags turning off north, leads off south) and the acceleration errors
//!   (on east or west, accelerating swings it north); the card swings and is damped by its fluid,
//!   and the aircraft's own magnetism gives its deviation.
//! - **Air data and attitude-heading computers** (`Readings::air_data`, `ahrs`), as in a G1000:
//!   electric, so they work without vacuum, but on the same pitot and static lines.
//!
//! Sources: the figures are written next to each. FlightGear's and JSBSim's instruments were read
//! for what they model; the equations here are the physics, nothing of their code.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::atmosphere::{self, Atmosphere, SEA_LEVEL_DENSITY, SEA_LEVEL_PRESSURE};
use crate::math::{act, axis_angle, clamp, euler_angles, inverse, normalize, smoothstep, Quat, Rng, DEG, GRAVITY, V3};

/// Pascal per hectopascal and per inch of mercury.
pub const HPA: f64 = 100.0;
pub const INHG: f64 = 3386.39;

/// The planet under the flat world: how fast it turns under a gyro and its magnetic field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Earth {
    /// Latitude, radians, north positive.
    pub latitude: f64,
    /// Magnetic variation, radians: magnetic north is this far east of true north. Zero, because
    /// the runways are numbered by their true heading.
    pub declination: f64,
    /// Inclination of the field below the horizontal, radians (positive in the north).
    pub dip: f64,
}

impl Default for Earth {
    /// 40° N, where the dip is about 57° (NOAA's World Magnetic Model, Iberia and the middle of
    /// the United States lie between 55° and 65°).
    fn default() -> Earth {
        Earth { latitude: 40.0 * DEG, declination: 0.0, dip: 57.0 * DEG }
    }
}

impl Earth {
    /// Direction of the magnetic field, unit, north-east-down.
    pub fn field(&self) -> V3 {
        let h = self.dip.cos();
        V3::new(h * self.declination.cos(), h * self.declination.sin(), self.dip.sin())
    }
}

/// What drives a gyro instrument, if the type has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum GyroDrive {
    None,
    /// The engine-driven vacuum pump.
    Vacuum,
    /// Its own electric motor.
    Electric,
}

/// Where the static ports are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum StaticPorts {
    /// One port on the left of the fuselage: it feels sideslip.
    Left,
    /// One each side, joined: their sideslip errors cancel.
    BothSides,
}

/// The instruments a type of aircraft has and how its pitot-static system errs
/// (`AircraftType::instruments`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstrumentFit {
    pub static_ports: StaticPorts,
    /// Pressure at the static ports above ambient, as a fraction of the impact pressure, by angle of
    /// attack (rad), for each flap setting (degrees): `(flap, &[(alpha, error)])`, both increasing.
    /// Empty: the ports read ambient.
    pub static_error: &'static [(f64, &'static [(f64, f64)])],
    /// The alternate static source, if there is one: the cabin's pressure, this fraction of the
    /// impact pressure below ambient.
    pub alternate_static: Option<f64>,
    pub pitot_heat: bool,
    pub attitude: GyroDrive,
    pub heading: GyroDrive,
    /// Pitch of the case against the attitude gyro at which its gimbal hits the stops (it tumbles).
    pub attitude_pitch_limit: f64,
    /// The compass's lubber line, misaligned this much (rad), and the aircraft's own magnetism
    /// (hard iron) at the compass, body axes, as a fraction of the Earth's field: its deviation.
    pub compass_lubber: f64,
    pub compass_iron: V3,
    /// The marks of its airspeed indicator.
    pub airspeed_marks: AirspeedMarks,
}

/// The marks of an airspeed indicator, indicated airspeeds in m/s: the white arc of the flaps (if it
/// has flaps), the green arc (the yellow one runs from its end to the red line), the red line and
/// the highest speed on the dial.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirspeedMarks {
    pub white: Option<(f64, f64)>,
    pub green: (f64, f64),
    pub red_line: f64,
    pub dial_top: f64,
}

impl InstrumentFit {
    /// The static ports' error at an angle of attack and a flap setting, as `static_error` gives it.
    pub fn static_error(&self, alpha: f64, flap_deg: f64) -> f64 {
        let tables = self.static_error;
        if tables.is_empty() {
            return 0.0;
        }
        let at = |k: usize| along(tables[k].1, alpha);
        let last = tables.len() - 1;
        match tables.iter().position(|t| t.0 > flap_deg) {
            None => at(last),
            Some(0) => at(0),
            Some(k) => {
                let t = (flap_deg - tables[k - 1].0) / (tables[k].0 - tables[k - 1].0);
                at(k - 1) + (at(k) - at(k - 1)) * t
            }
        }
    }
}

/// Linear interpolation in a table of `(x, y)` with increasing x, held at the ends.
fn along(table: &[(f64, f64)], x: f64) -> f64 {
    let (first, last) = (table[0], table[table.len() - 1]);
    if x <= first.0 {
        return first.1;
    }
    if x >= last.0 {
        return last.1;
    }
    let k = table.iter().position(|p| p.0 > x).unwrap_or(table.len() - 1);
    let (a, b) = (table[k - 1], table[k]);
    a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
}

/// How the pitot tube is blocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PitotBlock {
    #[default]
    Clear,
    /// The inlet (an insect, a cover left on): the drain hole bleeds the line down to static.
    Inlet,
    /// Inlet and drain (ice): the line keeps the pressure it had.
    Sealed,
}

/// What has gone wrong, for the failures to come and for testing now. `Instruments::failures`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Failures {
    pub pitot: PitotBlock,
    pub static_blocked: bool,
    pub vacuum_pump: bool,
    /// Flying through cloud with the air below freezing (`ICING_BELOW`): ice builds in the pitot
    /// until it seals it, unless the pitot heat is on.
    pub icing: bool,
}

/// What the instruments sense of the aircraft in one step: plain inputs.
#[derive(Clone, Copy, Debug)]
pub struct Sense {
    /// Above sea level, m.
    pub altitude: f64,
    /// Body to world.
    pub attitude: Quat,
    /// Body angular rates, rad/s.
    pub rates: V3,
    /// Velocity of the aircraft through the air, body axes.
    pub airflow: V3,
    /// What an accelerometer reads, body axes.
    pub specific: V3,
    pub rpm: f64,
    pub flap_deg: f64,
}

/// A G1000's air data computer: the same pitot and static lines, read electronically.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct AirData {
    /// Indicated (calibrated) airspeed, m/s.
    pub airspeed: f64,
    pub true_airspeed: f64,
    /// From the altimeter setting, m.
    pub altitude: f64,
    /// m/s, positive up.
    pub vertical_speed: f64,
    /// Outside air, K.
    pub temperature: f64,
}

/// What the dials and displays show, plain numbers, SI units, angles in radians.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Readings {
    /// Airspeed indicator, m/s (indicated: calibrated for the standard sea level).
    pub airspeed: f64,
    /// Altimeter, m, from its setting.
    pub altitude: f64,
    /// Vertical speed indicator, m/s, positive up.
    pub vertical_speed: f64,
    /// The altimeter's Kollsman window (and the G1000's BARO), Pa.
    pub altimeter_setting: f64,
    /// Attitude indicator: pitch and roll as its horizon shows them; `None` if the type has none.
    pub attitude: Option<[f64; 2]>,
    /// Heading indicator, in [0, 2π); `None` if the type has none.
    pub heading_indicator: Option<f64>,
    /// Vacuum gauge, Pa below ambient; `None` without a vacuum system.
    pub suction: Option<f64>,
    /// The suction is below the green arc (the G1000's LOW VACUUM).
    pub low_vacuum: bool,
    /// Magnetic compass, in [0, 2π).
    pub compass: f64,
    pub air_data: AirData,
    /// Attitude and heading reference: pitch, roll, magnetic heading in [0, 2π).
    pub ahrs: [f64; 3],
    pub pitot_heat: bool,
    pub alternate_static: bool,
}

// ---- Constants of the pitot-static system

/// Time constants of the lines, s: the pitot line from its tube, the static line from its ports
/// and from the cabin, the pitot line bled through its drain with the inlet blocked.
const PITOT_LAG: f64 = 0.05;
const STATIC_LAG: f64 = 0.1;
const CABIN_LAG: f64 = 0.4;
const DRAIN_LAG: f64 = 4.0;
/// The vertical speed indicator's calibrated leak at standard sea level, s: a first-order lag that
/// shows a new rate 95 % of the way after 7.5 s (the FAA's handbook: 6 to 9 seconds). It is a
/// capillary, so it grows as the pressure falls and as the air's viscosity rises.
pub const VSI_LEAK: f64 = 2.5;
/// Lags of the needles, s: airspeed, altimeter, vertical speed.
const NEEDLE_LAGS: [f64; 3] = [0.1, 0.15, 0.3];
/// The air data computer's vertical speed filter, s.
const AIR_DATA_LAG: f64 = 0.8;
/// The pitot tube reads the total pressure to this angle off its axis, and nothing past `PITOT_STALL`.
const PITOT_TRUE_TO: f64 = 20.0 * DEG;
const PITOT_STALL: f64 = 65.0 * DEG;
/// Per-port sideslip error: linear (+ windward, − leeward) and the crossflow's stagnation.
const SLIP_LINEAR: f64 = 0.6;
const SLIP_SQUARE: f64 = 0.25;
/// Ice forms in the pitot below this temperature, K; seals it after `ICE_SEALS` seconds unheated,
/// and is gone `ICE_MELTS` seconds after the heat comes on.
pub const ICING_BELOW: f64 = 275.15;
const ICE_SEALS: f64 = 45.0;
const ICE_MELTS: f64 = 20.0;
/// The Kollsman window's range, Pa.
pub const SETTING_RANGE: (f64, f64) = (946.0 * HPA, 1050.0 * HPA);

// ---- Constants of the vacuum system and the gyros

/// Regulated suction, the C172's green arc's middle (4.5 to 5.5 inHg), Pa; the pump's suction
/// against rpm is `PUMP_MOST · (1 − e^(−rpm / PUMP_RPM))`: about 4.4 inHg at idle, regulated from
/// some 1000 rpm. The LOW VACUUM annunciation is below 3 inHg.
pub const SUCTION_RATED: f64 = 5.0 * INHG;
const PUMP_MOST: f64 = 6.2 * INHG;
const PUMP_RPM: f64 = 550.0;
const SUCTION_LAG: f64 = 0.8;
pub const LOW_VACUUM: f64 = 3.0 * INHG;
/// Rotor time constants, s: spinning up on full suction or power (five minutes to erect, as the
/// handbooks allow), running down on bearing friction alone (the rotor is of little use after
/// five minutes and stops after fifteen).
const SPIN_UP: f64 = 45.0;
const ELECTRIC_SPIN_UP: f64 = 30.0;
const SPIN_DOWN: f64 = 300.0;
/// Erection of the attitude gyro towards the felt vertical, rad/s (3 to 8°/min for air-driven ones).
pub const ERECTION: f64 = 4.0 * DEG / 60.0;
/// Below this fraction of its speed the rotor's rigidity is lost to the friction of its gimbals:
/// the share of the case's rotation it follows is `(FOLLOW_SPIN / spin)³`.
const FOLLOW_SPIN: f64 = 0.1;
/// A rotor slower than this wanders off, up to `WANDER_RATE` when stopped.
const WANDER_SPIN: f64 = 0.4;
const WANDER_RATE: f64 = 3.0 * DEG;
/// A tumble: the rotor precesses this fast about the roll axis for this long.
const TUMBLE_RATE: f64 = 200.0 * DEG;
const TUMBLE_TIME: f64 = 0.7;
/// The Earth's rotation, rad/s, and the latitude the heading gyro's latitude nut compensates it
/// for (the world's default): elsewhere it drifts by the difference.
const EARTH_RATE: f64 = 7.2921e-5;
const LATITUDE_NUT: f64 = 40.0 * DEG;
/// Each heading gyro's own drift, rad/s, between these (the FAA asks to reset it every 15 minutes;
/// 3° in that time is the usual tolerance).
const DRIFT_RANGE: (f64, f64) = (4.0 * DEG / 3600.0, 10.0 * DEG / 3600.0);

// ---- Constants of the compass

/// The card's swing: natural frequency (rad/s) in the field of the default `Earth`, damping ratio
/// of its fluid; its tilt as a pendulum, and the most it tilts.
const COMPASS_FREQUENCY: f64 = 3.5;
const COMPASS_DAMPING: f64 = 0.35;
const CARD_FREQUENCY: f64 = 8.0;
const CARD_DAMPING: f64 = 0.6;
const CARD_TILT: f64 = 18.0 * DEG;

/// Moves `x` towards `target` as a first-order lag of time constant `tau`.
fn follow(x: &mut f64, target: f64, tau: f64, dt: f64) {
    *x += (target - *x) * (1.0 - (-dt / tau).exp());
}

fn wrap(angle: f64) -> f64 {
    angle.rem_euclid(TAU)
}

/// Turns `v` towards `target` (both unit) by at most `angle`.
fn turn_towards(v: V3, target: V3, angle: f64) -> V3 {
    let cross = v.cross(target);
    let between = cross.length().atan2(v.dot(target));
    if between < 1e-12 || cross.length() < 1e-12 {
        return v;
    }
    normalize(act(axis_angle(between.min(angle), normalize(cross)), v))
}

/// The rotation between two attitudes as a rotation vector, world axes.
fn rotation(from: Quat, to: Quat) -> V3 {
    let d = to * inverse(from);
    let sign = if d.w < 0.0 { -1.0 } else { 1.0 };
    V3::new(d.x, d.y, d.z) * (2.0 * sign)
}

/// Pitch and roll of a case of attitude `q` against a gyro whose down is `axis`.
fn horizon(q: Quat, axis: V3) -> [f64; 2] {
    let d = act(inverse(q), axis);
    [clamp(-d.x, -1.0, 1.0).asin(), d.y.atan2(d.z)]
}

/// A rotor's speed, as a fraction of its rated speed, after `dt` towards `free`.
fn spin(spin: &mut f64, free: f64, up: f64, dt: f64) {
    let tau = if *spin < free { up } else { SPIN_DOWN };
    follow(spin, free, tau, dt);
}

/// The attitude indicator's gyro.
#[derive(Clone, Debug)]
struct AttitudeGyro {
    /// The rotor's axis, world axes: the gyro's idea of down.
    axis: V3,
    spin: f64,
    /// Seconds of tumbling left, and which way.
    tumble: f64,
    tumble_sense: f64,
    on_stops: bool,
    /// The body axis a slow rotor wanders about.
    wander: V3,
}

impl AttitudeGyro {
    fn step(&mut self, dt: f64, sense: &Sense, turn: V3, free: f64, up: f64, pitch_limit: f64) {
        spin(&mut self.spin, free, up, dt);
        let q = sense.attitude;
        // The friction of the gimbals drags a slow rotor with the case.
        let dragged = (FOLLOW_SPIN / self.spin.max(1e-6)).powi(3).min(1.0);
        if dragged > 1e-9 && turn.length() > 1e-15 {
            let moved = act(axis_angle(turn.length(), normalize(turn)), self.axis);
            self.axis = normalize(self.axis.lerp(moved, dragged));
        }
        // Erection towards the vertical the pendulous vanes feel.
        let felt = act(q, -sense.specific);
        if felt.length() > 0.3 * GRAVITY {
            let rate = ERECTION * smoothstep(0.3, 0.9, self.spin);
            self.axis = turn_towards(self.axis, normalize(felt), rate * dt);
        }
        if self.spin < WANDER_SPIN {
            let rate = WANDER_RATE * (1.0 - self.spin / WANDER_SPIN);
            self.axis = normalize(act(axis_angle(rate * dt, act(q, self.wander)), self.axis));
        }
        // The pitch gimbal on its stops: the case pushes the rotor round about the pitch axis, which
        // precesses it violently about the axis square to that push and to its spin.
        let [pitch, roll] = horizon(q, self.axis);
        if pitch.abs() > pitch_limit {
            if !self.on_stops && self.spin > 0.2 {
                self.tumble = TUMBLE_TIME;
            }
            self.on_stops = true;
            let p = pitch.signum() * pitch_limit;
            self.axis = act(q, V3::new(-p.sin(), roll.sin() * p.cos(), roll.cos() * p.cos()));
        } else {
            self.on_stops = false;
        }
        if self.tumble > 0.0 {
            self.tumble -= dt;
            let about = self.axis.cross(act(q, V3::Y));
            if about.length() > 1e-6 {
                self.axis = normalize(act(axis_angle(TUMBLE_RATE * self.tumble_sense * dt, normalize(about)), self.axis));
            }
        }
    }
}

/// The heading indicator's gyro: its rotor's axis kept level, at an azimuth.
#[derive(Clone, Debug)]
struct HeadingGyro {
    /// Azimuth of the rotor's axis, rad from true north.
    azimuth: f64,
    spin: f64,
    /// What the card reads with the axis along the lubber line: where the knob set it.
    card: f64,
    /// Its own drift, rad/s.
    drift: f64,
}

impl HeadingGyro {
    /// Angle of the rotor's axis from the lubber line, in the case's plane: the gimbals.
    fn gimbal(&self, q: Quat) -> f64 {
        let s = act(inverse(q), V3::new(self.azimuth.cos(), self.azimuth.sin(), 0.0));
        s.y.atan2(s.x)
    }

    fn reading(&self, q: Quat) -> f64 {
        wrap(self.card - self.gimbal(q))
    }

    fn step(&mut self, dt: f64, turn: V3, free: f64, latitude: f64) {
        spin(&mut self.spin, free, SPIN_UP, dt);
        let rigid = smoothstep(0.05, 0.3, self.spin);
        // A rotor fixed in space turns against the Earth under it, less what the nut takes off.
        let drift = self.drift + EARTH_RATE * (latitude.sin() - LATITUDE_NUT.sin());
        let dragged = (FOLLOW_SPIN / self.spin.max(1e-6)).powi(3).min(1.0);
        self.azimuth = wrap(self.azimuth + drift * rigid * dt + turn.z * dragged);
    }
}

/// The magnetic compass's card.
#[derive(Clone, Debug, Default)]
struct Compass {
    /// The card's tilt as a pendulum: about the roll and the pitch axes, and their rates.
    tilt: [f64; 2],
    tilt_rate: [f64; 2],
    /// What it reads, and how fast that changes.
    reading: f64,
    rate: f64,
    /// The case's turn rate about the card's axis, last step.
    case_rate: f64,
}

impl Compass {
    /// The tilt the card would hang at.
    fn hang(&self, specific: V3) -> [f64; 2] {
        let felt = -specific;
        if felt.length() < 0.2 * GRAVITY {
            return self.tilt;
        }
        let pitch = (-felt.x).atan2(felt.z);
        let roll = felt.y.atan2(felt.z);
        [clamp(roll, -CARD_TILT, CARD_TILT), clamp(pitch, -CARD_TILT, CARD_TILT)]
    }

    /// The card's axis, body axes.
    fn normal(&self) -> V3 {
        normalize(V3::new(-self.tilt[1].tan(), self.tilt[0].tan(), 1.0))
    }

    /// What the card settles at, and the strength of the field in its plane (fraction of the
    /// Earth's): the field, with the aircraft's own, seen in the plane of the card.
    fn settles(&self, q: Quat, field: V3, fit: &InstrumentFit) -> (f64, f64) {
        let n = self.normal();
        let b = act(inverse(q), field) + fit.compass_iron;
        let flat = |v: V3| v - n * v.dot(n);
        let e1 = normalize(flat(V3::X));
        let e2 = n.cross(e1);
        let bp = flat(b);
        (wrap(fit.compass_lubber - bp.dot(e2).atan2(bp.dot(e1))), bp.length())
    }

    fn settle(&mut self, sense: &Sense, field: V3, fit: &InstrumentFit) {
        self.tilt = [0.0; 2];
        self.tilt = self.hang(sense.specific);
        self.tilt_rate = [0.0; 2];
        self.reading = self.settles(sense.attitude, field, fit).0;
        self.rate = 0.0;
        self.case_rate = sense.rates.dot(self.normal());
    }

    fn step(&mut self, dt: f64, sense: &Sense, field: V3, fit: &InstrumentFit, reference: f64) {
        let target = self.hang(sense.specific);
        for ((tilt, rate), target) in self.tilt.iter_mut().zip(&mut self.tilt_rate).zip(target) {
            *rate += (CARD_FREQUENCY * CARD_FREQUENCY * (target - *tilt) - 2.0 * CARD_DAMPING * CARD_FREQUENCY * *rate) * dt;
            *tilt += *rate * dt;
            if tilt.abs() > CARD_TILT {
                *tilt = tilt.signum() * CARD_TILT;
                *rate = 0.0;
            }
        }
        let (settles, strength) = self.settles(sense.attitude, field, fit);
        // The card keeps its place in space as the case turns under it; the field pulls it round,
        // the fluid (turning with the bowl) damps its motion against the case.
        let case_rate = sense.rates.dot(self.normal());
        let case_accel = (case_rate - self.case_rate) / dt;
        self.case_rate = case_rate;
        let w2 = COMPASS_FREQUENCY * COMPASS_FREQUENCY * strength / reference;
        let accel = case_accel + w2 * (settles - self.reading).sin() - 2.0 * COMPASS_DAMPING * COMPASS_FREQUENCY * self.rate;
        self.rate += accel * dt;
        self.reading = wrap(self.reading + self.rate * dt);
    }
}

/// The instruments of one aircraft: their pressures, gyros and compass, and what they show.
#[derive(Clone, Debug)]
pub struct Instruments {
    pub failures: Failures,
    /// Switches and knobs.
    pub pitot_heat: bool,
    pub alternate_static: bool,
    /// The Kollsman window, Pa (`SETTING_RANGE`): the altimeter and the G1000's BARO.
    pub altimeter_setting: f64,
    /// Ice in the pitot: 1 seals it.
    pitot_ice: f64,
    pitot_line: f64,
    static_line: f64,
    vsi_case: f64,
    /// Airspeed, altitude and vertical speed as the needles have them.
    needles: [f64; 3],
    /// The air data computer's altitude, filtered for its vertical speed.
    air_data_altitude: f64,
    suction: f64,
    attitude: AttitudeGyro,
    heading: HeadingGyro,
    compass: Compass,
    last_attitude: Quat,
    readings: Readings,
}

impl Instruments {
    /// Instruments with nothing settled yet (`settle` them where the aircraft is placed). The seed
    /// decides each gyro's own drift and the way a slow rotor wanders.
    pub fn new(seed: u64) -> Instruments {
        let mut rng = Rng::new(seed ^ 0x6A09_E667_F3BC_C908);
        let drift = DRIFT_RANGE.0 + (DRIFT_RANGE.1 - DRIFT_RANGE.0) * rng.uniform();
        let drift = if rng.uniform() < 0.5 { -drift } else { drift };
        let wander = rng.uniform() * TAU;
        let tumble_sense = if rng.uniform() < 0.5 { -1.0 } else { 1.0 };
        Instruments {
            failures: Failures::default(),
            pitot_heat: false,
            alternate_static: false,
            altimeter_setting: SEA_LEVEL_PRESSURE,
            pitot_ice: 0.0,
            pitot_line: SEA_LEVEL_PRESSURE,
            static_line: SEA_LEVEL_PRESSURE,
            vsi_case: SEA_LEVEL_PRESSURE,
            needles: [0.0; 3],
            air_data_altitude: 0.0,
            suction: 0.0,
            attitude: AttitudeGyro {
                axis: V3::Z,
                spin: 0.0,
                tumble: 0.0,
                tumble_sense,
                on_stops: false,
                wander: V3::new(wander.cos(), wander.sin(), 0.0),
            },
            heading: HeadingGyro { azimuth: 0.0, spin: 0.0, card: 0.0, drift },
            compass: Compass::default(),
            last_attitude: Quat::IDENTITY,
            readings: Readings::default(),
        }
    }

    pub fn readings(&self) -> &Readings {
        &self.readings
    }

    /// For a replay: the dials and displays show these, whatever the lines and gyros say.
    pub(crate) fn show(&mut self, readings: Readings) {
        self.readings = readings;
    }

    /// Ice in the pitot tube, 0 to 1 (1 seals it).
    pub fn pitot_ice(&self) -> f64 {
        self.pitot_ice
    }

    /// Rotor speeds of the attitude and heading gyros, fractions of their rated speeds.
    pub fn gyro_spin(&self) -> [f64; 2] {
        [self.attitude.spin, self.heading.spin]
    }

    /// Everything as a pilot finds it after taxiing out: no failures, switches off, gyros up to
    /// speed and erect, the heading gyro set to the compass, the altimeter set to `setting` (Pa),
    /// the lines at the pressures they sense.
    pub fn settle(&mut self, sense: &Sense, fit: &InstrumentFit, air: &Atmosphere, earth: &Earth, setting: f64) {
        self.failures = Failures::default();
        self.pitot_heat = false;
        self.alternate_static = false;
        self.altimeter_setting = clamp(setting, SETTING_RANGE.0, SETTING_RANGE.1);
        self.pitot_ice = 0.0;
        let (pitot, port, _) = sources(sense, fit, air);
        self.pitot_line = pitot;
        self.static_line = port;
        self.vsi_case = port;
        self.suction = suction_of(sense.rpm, fit, &self.failures);
        let [attitude, heading] = self.free_spin(fit);
        self.attitude.spin = attitude;
        self.attitude.axis = V3::Z;
        self.attitude.tumble = 0.0;
        self.attitude.on_stops = false;
        self.heading.spin = heading;
        self.heading.azimuth = 0.0;
        self.compass.settle(sense, earth.field(), fit);
        self.heading.card = 0.0;
        self.align_heading_indicator(sense.attitude);
        self.last_attitude = sense.attitude;
        self.read(sense, fit, air, earth);
        self.readings.air_data.vertical_speed = 0.0;
        let r = self.readings;
        self.needles = [r.airspeed, r.altitude, r.vertical_speed];
        self.air_data_altitude = r.air_data.altitude;
    }

    /// Turns the heading indicator's knob until it reads what the compass reads now (do it in
    /// steady, level flight: the compass is wrong in turns and accelerations).
    pub fn align_heading_indicator(&mut self, attitude: Quat) {
        self.heading.card = wrap(self.compass.reading + self.heading.gimbal(attitude));
        self.readings.heading_indicator = self.readings.heading_indicator.map(|_| self.heading.reading(attitude));
    }

    /// How fast each rotor would spin with the suction or power it has now.
    fn free_spin(&self, fit: &InstrumentFit) -> [f64; 2] {
        let vacuum = (self.suction / SUCTION_RATED).max(0.0).sqrt().min(1.0);
        [fit.attitude, fit.heading].map(|drive| match drive {
            GyroDrive::None => 0.0,
            GyroDrive::Vacuum => vacuum,
            GyroDrive::Electric => 1.0,
        })
    }

    /// Advances the instruments by `dt` with what they sense now.
    pub fn step(&mut self, dt: f64, sense: &Sense, fit: &InstrumentFit, air: &Atmosphere, earth: &Earth) {
        let ambient = air.pressure(sense.altitude);
        let oat = air.temperature(sense.altitude);
        let (pitot, port, cabin) = sources(sense, fit, air);

        // Ice in the pitot.
        if self.pitot_heat && fit.pitot_heat {
            self.pitot_ice = (self.pitot_ice - dt / ICE_MELTS).max(0.0);
        } else if self.failures.icing && oat < ICING_BELOW {
            self.pitot_ice = (self.pitot_ice + dt / ICE_SEALS).min(1.0);
        }
        let block = if self.pitot_ice >= 1.0 { PitotBlock::Sealed } else { self.failures.pitot };
        match block {
            PitotBlock::Clear => follow(&mut self.pitot_line, pitot, PITOT_LAG, dt),
            PitotBlock::Inlet => follow(&mut self.pitot_line, ambient, DRAIN_LAG, dt),
            PitotBlock::Sealed => {}
        }
        match fit.alternate_static {
            Some(_) if self.alternate_static => follow(&mut self.static_line, cabin, CABIN_LAG, dt),
            _ if self.failures.static_blocked => {}
            _ => follow(&mut self.static_line, port, STATIC_LAG, dt),
        }
        // The VSI's case breathes through a capillary: its time constant goes as the viscosity of
        // the air over its pressure.
        let leak = VSI_LEAK * SEA_LEVEL_PRESSURE / self.static_line.max(1000.0) * viscosity_ratio(oat);
        follow(&mut self.vsi_case, self.static_line, leak, dt);

        // Vacuum and gyros.
        let pump = suction_of(sense.rpm, fit, &self.failures);
        follow(&mut self.suction, pump, SUCTION_LAG, dt);
        let [attitude_free, heading_free] = self.free_spin(fit);
        let turn = rotation(self.last_attitude, sense.attitude);
        self.last_attitude = sense.attitude;
        let up = if fit.attitude == GyroDrive::Electric { ELECTRIC_SPIN_UP } else { SPIN_UP };
        self.attitude.step(dt, sense, turn, attitude_free, up, fit.attitude_pitch_limit);
        self.heading.step(dt, turn, heading_free, earth.latitude);
        let reference = Earth::default().dip.cos();
        self.compass.step(dt, sense, earth.field(), fit, reference);

        self.read(sense, fit, air, earth);
        let r = &mut self.readings;
        for (needle, (value, lag)) in self.needles.iter_mut().zip([r.airspeed, r.altitude, r.vertical_speed].into_iter().zip(NEEDLE_LAGS)) {
            follow(needle, value, lag, dt);
        }
        [r.airspeed, r.altitude, r.vertical_speed] = self.needles;
        follow(&mut self.air_data_altitude, r.air_data.altitude, AIR_DATA_LAG, dt);
        r.air_data.vertical_speed = (r.air_data.altitude - self.air_data_altitude) / AIR_DATA_LAG;
    }

    /// The readings from the lines, gyros and compass as they are (the needles' lags apart).
    fn read(&mut self, sense: &Sense, fit: &InstrumentFit, air: &Atmosphere, earth: &Earth) {
        let impact = self.pitot_line - self.static_line;
        let oat = air.temperature(sense.altitude);
        let altitude = atmosphere::pressure_altitude(self.static_line, self.altimeter_setting);
        let climb = -(self.static_line - self.vsi_case) / (VSI_LEAK * SEA_LEVEL_DENSITY * GRAVITY);
        let e = euler_angles(sense.attitude);
        let q = sense.attitude;
        let vacuum = fit.attitude == GyroDrive::Vacuum || fit.heading == GyroDrive::Vacuum;
        self.readings = Readings {
            airspeed: atmosphere::calibrated_airspeed(impact),
            altitude,
            vertical_speed: climb,
            altimeter_setting: self.altimeter_setting,
            attitude: (fit.attitude != GyroDrive::None).then(|| horizon(q, self.attitude.axis)),
            heading_indicator: (fit.heading != GyroDrive::None).then(|| self.heading.reading(q)),
            suction: vacuum.then_some(self.suction),
            low_vacuum: vacuum && self.suction < LOW_VACUUM,
            compass: self.compass.reading,
            air_data: AirData {
                airspeed: atmosphere::calibrated_airspeed(impact),
                true_airspeed: atmosphere::mach(impact, self.static_line) * (atmosphere::GAMMA * atmosphere::GAS_CONSTANT * oat).sqrt(),
                altitude,
                vertical_speed: self.readings.air_data.vertical_speed,
                temperature: oat,
            },
            ahrs: [e.pitch, e.roll, wrap(e.heading - earth.declination)],
            pitot_heat: self.pitot_heat && fit.pitot_heat,
            alternate_static: self.alternate_static && fit.alternate_static.is_some(),
        };
    }
}

/// The pressures at the pitot tube, at the static ports and in the cabin, Pa.
fn sources(sense: &Sense, fit: &InstrumentFit, air: &Atmosphere) -> (f64, f64, f64) {
    let ambient = air.pressure(sense.altitude);
    let speed = sense.airflow.length();
    if speed < 0.1 {
        return (ambient, ambient, ambient);
    }
    let mach = (speed / air.speed_of_sound(sense.altitude)).min(0.99);
    let impact = atmosphere::impact_pressure(ambient, mach);
    // Total pressure is the same everywhere outside the boundary layers: the tube recovers it if it
    // faces the flow, less and less beyond 20° off its axis, none past 65°; from behind it reads a
    // little under static.
    let off_axis = clamp(sense.airflow.x / speed, -1.0, 1.0).acos();
    let recovery = if off_axis < FRAC_PI_2 {
        1.0 - smoothstep(PITOT_TRUE_TO, PITOT_STALL, off_axis)
    } else {
        -0.3 * smoothstep(FRAC_PI_2, 0.75 * PI, off_axis)
    };
    let alpha = sense.airflow.z.atan2(sense.airflow.x.max(1e-3));
    let slip = clamp(sense.airflow.y / speed, -1.0, 1.0);
    // Sideslip from the right (positive) puts the left port in the lee.
    let port_slip = match fit.static_ports {
        StaticPorts::Left => -SLIP_LINEAR * slip + SLIP_SQUARE * slip * slip,
        StaticPorts::BothSides => SLIP_SQUARE * slip * slip,
    };
    let error = if sense.airflow.x > 0.0 { fit.static_error(alpha, sense.flap_deg) } else { 0.0 };
    let port = ambient + (error + port_slip) * impact;
    let cabin = ambient - fit.alternate_static.unwrap_or(0.0) * impact;
    (ambient + recovery * impact, port, cabin)
}

/// The suction the vacuum system holds at an engine speed, Pa.
fn suction_of(rpm: f64, fit: &InstrumentFit, failures: &Failures) -> f64 {
    let vacuum = fit.attitude == GyroDrive::Vacuum || fit.heading == GyroDrive::Vacuum;
    if !vacuum || failures.vacuum_pump {
        return 0.0;
    }
    (PUMP_MOST * (1.0 - (-rpm.max(0.0) / PUMP_RPM).exp())).min(SUCTION_RATED)
}

/// Viscosity of air against the standard sea level's (Sutherland's law).
fn viscosity_ratio(temperature: f64) -> f64 {
    let t0 = atmosphere::SEA_LEVEL_TEMPERATURE;
    (temperature / t0).powf(1.5) * (t0 + 110.4) / (temperature + 110.4)
}
