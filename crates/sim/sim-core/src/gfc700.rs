//! The Garmin GFC 700 of the Cessna 172's G1000: a flight director and an autopilot of three
//! servos (roll, pitch, pitch trim), worked from the bezel's keys and knobs, after the behaviour
//! its pilot's guide describes.
//!
//! It flies on what the G1000 knows and nothing else: attitude, heading and body rates from the
//! attitude-heading reference, airspeed, altitude and vertical speed from the air data computer
//! (`Readings::air_data`, `ahrs`: a blocked pitot fools it as it fools the pilot), position, ground
//! track and the glide path's height from the GPS. It moves the ailerons, the elevator and the trim,
//! no more than its servos can (`Gfc700Fit`); the rudder and the throttle stay the pilot's (the C172
//! has no yaw servo and no autothrottle).
//!
//! ```text
//! modes ─► flight director: bank and pitch wanted ─► servos ─► ailerons, elevator ─► trim
//!   lateral   ROL · HDG · GPS (armed, then active)            limited in travel and rate
//!   vertical  PIT · ALT · ALTS · VS · FLC · GP (ALTS, GP armed)
//! ```
//!
//! `Gfc700` is a `Pilot` that is handed the controls the pilot's hands set: with the autopilot off
//! it returns them untouched and only keeps the flight director and the altitude alerter going;
//! engaged, it takes the ailerons, the elevator and the trim. Keys and knobs (`press`, `turn`)
//! act between steps, on what it sensed in the last one.

use std::f64::consts::PI;

use crate::aircraft::AircraftControls;
use crate::lights::{AIM, GLIDE_PATH};
use crate::math::{clamp, remainder, DEG, FEET, GRAVITY, KNOTS};
use crate::pilot::Pilot;
use crate::terrain::Airport;
use crate::vehicle::Controls;
use crate::world::Observation;

const TAU: f64 = 2.0 * PI;
/// Feet per minute, in m/s.
const FPM: f64 = FEET / 60.0;
/// A nautical mile, m.
const NM: f64 = 1852.0;

/// What a type's GFC 700 can do: its servos and the limits its flight director keeps
/// (`AircraftType::afcs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gfc700Fit {
    /// The most the roll and pitch servos move the ailerons and the elevator, a share of their
    /// full travel: what their clutches pass on. The pilot overpowers them past it.
    pub roll_authority: f64,
    pub pitch_authority: f64,
    /// How fast the roll and pitch servos move their surfaces, share of full travel per second.
    pub roll_rate: f64,
    pub pitch_rate: f64,
    /// How fast the trim servo runs the trim, in `AircraftControls::trim` per second (its whole range is 2).
    pub trim_rate: f64,
    /// The most bank the flight director asks for, in every lateral mode.
    pub bank_limit: f64,
    /// The most pitch it asks for, nose up and nose down (positive both).
    pub pitch_up: f64,
    pub pitch_down: f64,
    /// Indicated airspeeds it will not fly past (MAXSPD) or below (MINSPD), whatever the mode
    /// asks; also the range of the FLC reference.
    pub max_speed: f64,
    pub min_speed: f64,
}

/// A key of the bezel (and the yoke's AP DISC).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Ap,
    Fd,
    Hdg,
    Alt,
    Nav,
    Vnv,
    Apr,
    Bc,
    Vs,
    Flc,
    NoseUp,
    NoseDown,
    /// The red AP DISC switch on the yoke: disconnects, and silences the alert.
    Disconnect,
    /// The HDG knob pushed: the bug to the present heading.
    HeadingSync,
}

/// A knob of the bezel, turned by clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Knob {
    /// The heading bug, 1° a click.
    Heading,
    /// The selected altitude: the inner knob 100 ft a click, the outer 1000 ft.
    AltitudeFine,
    AltitudeCoarse,
}

/// The lateral mode the flight director is in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LateralMode {
    /// Roll hold: wings level, or this bank (radians, positive right).
    Roll(f64),
    /// To the heading bug.
    Heading,
    /// Along the active leg of the flight plan.
    Gps,
}

/// The vertical mode the flight director is in, with its reference.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VerticalMode {
    /// Pitch hold, radians.
    Pitch(f64),
    /// Altitude hold, m (the altimeter's).
    Altitude(f64),
    /// Capturing the selected altitude.
    Capture,
    /// Vertical speed, m/s, positive up.
    VerticalSpeed(f64),
    /// Flight level change: this indicated airspeed, m/s, with the pitch; power is the pilot's.
    Flc(f64),
    /// Down the approach's glide path.
    GlidePath,
}

/// How the autopilot was last disconnected: by the pilot (AP key, AP DISC, a firm hand on the
/// yoke, the manual trim) or by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disconnect {
    Pilot,
    Overpowered,
    ManualTrim,
    /// Beyond the attitudes it flies in (`ATTITUDE_LIMITS`).
    Attitude,
    /// The aircraft crashed.
    Crashed,
}

impl Disconnect {
    /// The autopilot let go by itself: the alert goes on until the pilot acknowledges it.
    pub fn abnormal(self) -> bool {
        matches!(self, Disconnect::Attitude | Disconnect::Crashed)
    }
}

/// The disconnect alert: the AP annunciation flashing and the two-tone sounding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alert {
    pub why: Disconnect,
    /// When it started, world seconds.
    pub since: f64,
    /// The pilot acknowledged it (AP DISC).
    pub silenced: bool,
}

/// After a normal disconnect the tone sounds this long and the AP annunciation flashes this long.
pub const TONE_NORMAL: f64 = 2.0;
pub const FLASH_NORMAL: f64 = 5.0;
/// A mode that becomes active by itself (armed, then captured) flashes this long.
pub const MODE_FLASH: f64 = 10.0;
/// The autopilot lets go beyond this bank, and this pitch up and down.
pub const ATTITUDE_LIMITS: [f64; 3] = [45.0 * DEG, 30.0 * DEG, 25.0 * DEG];
/// A push on the yoke past this share of its travel, held this long, overpowers the servos.
pub const OVERPOWER: f64 = 0.3;
pub const OVERPOWER_TIME: f64 = 0.5;

// ---- The flight director's laws

/// ROL: below this bank it levels the wings, above it holds the bank (up to the limit).
const ROLL_LEVEL: f64 = 6.0 * DEG;
/// How fast the wanted bank and pitch move, rad/s: the GFC 700's smooth rolls and pitch changes.
const BANK_RATE: f64 = 5.0 * DEG;
const PITCH_RATE: f64 = 2.0 * DEG;
/// Bank per radian of heading or track error: 15° of error asks 15° of bank.
const TURN_GAIN: f64 = 1.0;
/// A standard-rate turn, rad/s: the most HDG and GPS turn at (and never past `bank_limit`).
const STANDARD_RATE: f64 = 3.0 * DEG;
/// A leg is followed by aiming at the point this many seconds ahead on it, en route and on an
/// approach; the intercept is never steeper than 45°.
const LINE_TIME: f64 = 25.0;
const APPROACH_LINE_TIME: f64 = 15.0;
/// GPS arms rather than captures until the aircraft is this close to the leg, or closing on it
/// to be there within `CAPTURE_TIME`.
const CAPTURE_NEAR: f64 = 300.0;
const CAPTURE_TIME: f64 = 20.0;
/// Vertical speed asked per metre of altitude error in ALT, 1/s, and the most it asks.
const ALT_GAIN: f64 = 0.12;
const ALT_RATE: f64 = 4.0;
/// ALTS: the vertical speed asked per metre short of the selected altitude, 1/s (capture starts
/// that many seconds' climb short of it), and the band where it becomes ALT.
const CAPTURE_GAIN: f64 = 0.15;
const CAPTURED: f64 = 10.0 * FEET;
/// ALTS is armed when the selected altitude is more than this from the altimeter.
const ALTS_ARM: f64 = 50.0 * FEET;
/// From a vertical speed to a pitch: pitch per radian of flight path error and its integral per
/// second, the angle of attack held within these.
const PATH_P: f64 = 0.8;
const PATH_I: f64 = 0.3;
const ALPHA_RANGE: (f64, f64) = (-6.0 * DEG, 14.0 * DEG);
/// The angle of attack's estimate (pitch less the flight path) is filtered over this many seconds.
const ALPHA_LAG: f64 = 2.0;
/// FLC: the airspeed is brought to its reference at this share of the difference a second, never
/// faster than `ACCEL_MOST` (m/s²), and the pitch moves at `SPEED_PITCH` radians a second per
/// radian of flight path the airspeed's change is off that (the airspeed's rate filtered over
/// `SPEED_LAG`). Self-trimming: whatever the power, the pitch settles where the airspeed stays.
const SPEED_GAIN: f64 = 0.15;
const ACCEL_MOST: f64 = 0.5;
const SPEED_PITCH: f64 = 0.3;
const SPEED_LAG: f64 = 0.5;
/// The protections look this many seconds ahead at the airspeed's trend, and let go this far
/// inside their limit. They hold the limit with FLC's law.
const PROTECT_LEAD: f64 = 4.0;
const PROTECT_MARGIN: f64 = 3.0 * KNOTS;
/// GP: vertical speed per metre off the path, 1/s, and the band below and above it where it captures.
const GLIDE_GAIN: f64 = 0.2;
const GLIDE_CAPTURE: (f64, f64) = (-8.0, 20.0);
/// The references' steps on NOSE UP / NOSE DN and their ranges.
const PITCH_STEP: f64 = 0.5 * DEG;
const VS_STEP: f64 = 100.0 * FPM;
const VS_RANGE: (f64, f64) = (-2000.0 * FPM, 1500.0 * FPM);
const ALT_STEP: f64 = 10.0 * FEET;
/// The selected altitude: never below sea level, so a careless spin of the knob sets no target
/// under the ground.
const SELECTED_RANGE: (f64, f64) = (0.0, 25_000.0 * FEET);

// ---- The servos

/// Aileron per radian of bank error and per rad/s of roll rate; elevator per radian of pitch error,
/// per rad/s of pitch rate, and the share of a correction the elevator keeps each second. Both
/// scaled by (80 kt / airspeed)², as control power grows with dynamic pressure.
const ROLL_P: f64 = 2.0;
const ROLL_D: f64 = 0.7;
const PITCH_P: f64 = 3.0;
const PITCH_D: f64 = 0.9;
const PITCH_I: f64 = 0.5;
/// The trim servo runs while the pitch servo holds more than this (and at its full rate from
/// `TRIM_FULL` more), after holding it for `TRIM_DELAY` seconds.
const TRIM_DEADBAND: f64 = 0.01;
const TRIM_FULL: f64 = 0.04;
const TRIM_DELAY: f64 = 1.0;

// ---- The approach

/// The final approach fix and the intermediate fix, metres before the threshold.
const FAF: f64 = 5.0 * NM;
const IF: f64 = 9.0 * NM;

/// One leg of the flight plan: a great circle on a flat world, a straight line.
#[derive(Clone, Debug, PartialEq)]
pub struct Leg {
    /// North and east, m.
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// What the G1000 calls the point it goes to.
    pub name: String,
}

impl Leg {
    /// Its course over the ground, radians from true north.
    pub fn course(&self) -> f64 {
        (self.to.1 - self.from.1).atan2(self.to.0 - self.from.0).rem_euclid(TAU)
    }

    /// How far a point is right of it, and how far it still is to its end along it, m.
    pub fn place(&self, n: f64, e: f64) -> (f64, f64) {
        let c = self.course();
        let (dn, de) = (n - self.to.0, e - self.to.1);
        (-dn * c.sin() + de * c.cos(), -(dn * c.cos() + de * c.sin()))
    }
}

/// A GPS approach to a runway with vertical guidance (the LPV of the real one): a final course
/// down the runway's line and a glide path of `GLIDE_PATH` to the aiming point, from the GPS's
/// position and height (barometric errors do not move it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Approach {
    pub runway: Airport,
    /// Landing towards the runway's heading plus 180°.
    pub reversed: bool,
}

impl Approach {
    /// The landing direction, radians from true north.
    pub fn course(&self) -> f64 {
        (self.runway.heading + if self.reversed { PI } else { 0.0 }).rem_euclid(TAU)
    }

    /// The point `before` metres before the threshold on the runway's line (negative: past it).
    pub fn point(&self, before: f64) -> (f64, f64) {
        let c = self.course();
        let back = self.runway.length / 2.0 + before;
        (self.runway.n - c.cos() * back, self.runway.e - c.sin() * back)
    }

    /// The runway's name, as the plan shows it: RW27.
    pub fn name(&self) -> String {
        let number = (self.runway.number() + if self.reversed { 18 } else { 0 } - 1) % 36 + 1;
        format!("RW{number:02}")
    }

    /// The glide path's height above sea level over a point (straight down the line from it).
    pub fn path_altitude(&self, n: f64, e: f64) -> f64 {
        let c = self.course();
        let aim = self.point(-AIM);
        let to_aim = (aim.0 - n) * c.cos() + (aim.1 - e) * c.sin();
        self.runway.elevation + to_aim * GLIDE_PATH.tan()
    }

    /// Distance to the threshold along the final course, m (negative past it).
    fn before_threshold(&self, n: f64, e: f64) -> f64 {
        let c = self.course();
        let t = self.point(0.0);
        (t.0 - n) * c.cos() + (t.1 - e) * c.sin()
    }
}

/// The flight plan the GPS mode follows: a direct-to, or an approach.
#[derive(Clone, Debug, PartialEq)]
pub struct FlightPlan {
    pub legs: Vec<Leg>,
    pub active: usize,
    pub approach: Option<Approach>,
}

impl FlightPlan {
    pub fn active_leg(&self) -> &Leg {
        &self.legs[self.active]
    }
}

/// What the altitude alerter says about the selected altitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltitudeAlert {
    None,
    /// Within 1000 ft of it, for the first 5 s: the box flashes.
    Approaching,
    /// Within 200 ft: the box shows it so.
    Within,
    /// More than 200 ft off it after having been within, for the first 5 s.
    Deviation,
}

/// How long the alerter's flashes last, and its bands.
const ALERT_FLASH: f64 = 5.0;
const ALERT_FAR: f64 = 1000.0 * FEET;
const ALERT_NEAR: f64 = 200.0 * FEET;

/// The altitude alerter of the G1000, on the selected altitude.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Alerter {
    /// Been within `ALERT_NEAR` since the selected altitude was set.
    reached: bool,
    /// Was within `ALERT_FAR` at the last look.
    near: bool,
    /// When the last flash began, and which.
    flash: Option<(f64, AltitudeAlert)>,
}

impl Alerter {
    fn new() -> Alerter {
        Alerter { reached: false, near: true, flash: None }
    }

    fn update(&mut self, error: f64, time: f64) {
        let off = error.abs();
        if off <= ALERT_NEAR {
            self.reached = true;
            self.flash = None;
        } else if self.reached {
            self.reached = false;
            self.flash = Some((time, AltitudeAlert::Deviation));
        } else if off <= ALERT_FAR && !self.near {
            self.flash = Some((time, AltitudeAlert::Approaching));
        }
        self.near = off <= ALERT_FAR;
    }

    fn alert(&self, time: f64) -> AltitudeAlert {
        match self.flash {
            Some((since, what)) if time - since < ALERT_FLASH => what,
            _ if self.reached => AltitudeAlert::Within,
            _ => AltitudeAlert::None,
        }
    }
}

/// What the GFC 700 reads from the G1000 in a step.
#[derive(Clone, Copy, Debug, Default)]
struct Sensed {
    time: f64,
    /// Air data: indicated and true airspeed, altitude, vertical speed.
    ias: f64,
    tas: f64,
    altitude: f64,
    vs: f64,
    /// Attitude and heading reference: pitch, roll, magnetic heading, body rates.
    pitch: f64,
    roll: f64,
    heading: f64,
    rates: [f64; 3],
    /// GPS: position, track and speed over the ground, height.
    north: f64,
    east: f64,
    track: f64,
    ground_speed: f64,
    gps_altitude: f64,
    on_ground: bool,
    crashed: bool,
}

impl Sensed {
    fn of(obs: &Observation) -> Sensed {
        let r = &aircraft(obs).instruments;
        let air = &r.air_data;
        let (vn, ve) = (obs.velocity[0], obs.velocity[1]);
        Sensed {
            time: obs.time,
            ias: air.airspeed,
            tas: air.true_airspeed,
            altitude: air.altitude,
            vs: air.vertical_speed,
            pitch: r.ahrs[0],
            roll: r.ahrs[1],
            heading: r.ahrs[2],
            rates: obs.angular_rates,
            north: obs.north,
            east: obs.east,
            track: ve.atan2(vn).rem_euclid(TAU),
            ground_speed: vn.hypot(ve),
            gps_altitude: obs.altitude,
            on_ground: obs.on_ground,
            crashed: obs.crashed.is_some(),
        }
    }

    /// Bank of a standard-rate turn at this speed.
    fn standard_bank(&self) -> f64 {
        (self.tas.max(20.0) * STANDARD_RATE / GRAVITY).atan()
    }

    /// Flight path angle through the air.
    fn path(&self) -> f64 {
        (self.vs / self.tas.max(20.0)).clamp(-1.0, 1.0).asin()
    }
}

/// The GFC 700: modes, references, flight plan, the flight director's commands and the servos.
#[derive(Clone, Debug)]
pub struct Gfc700 {
    fit: Gfc700Fit,
    ap: bool,
    fd: bool,
    lateral: LateralMode,
    vertical: VerticalMode,
    /// GPS armed (the lateral mode active meanwhile is ROL or HDG), and in approach (APR).
    gps_armed: bool,
    approach_mode: bool,
    /// GP armed.
    glide_armed: bool,
    /// ALTS armed: set once the selected altitude is `ALTS_ARM` away in PIT, VS or FLC, kept until captured.
    alts: bool,
    heading_bug: f64,
    selected_altitude: f64,
    plan: Option<FlightPlan>,
    /// When a mode last became active by itself: lateral, vertical.
    changed: [Option<f64>; 2],
    /// The protection holding the pitch now: MAXSPD (true) or MINSPD (false).
    protecting: Option<bool>,
    alert: Option<Alert>,
    alerter: Alerter,
    sensed: Sensed,
    started: bool,
    /// The flight director's commands as they move (`BANK_RATE`, `PITCH_RATE`).
    bank_cmd: f64,
    pitch_cmd: f64,
    /// The angle of attack's estimate, and the one the vertical speed laws hold.
    alpha: f64,
    alpha_hold: f64,
    /// FLC: the pitch it settles to, and the airspeed's rate of change (filtered) with the last airspeed.
    flc_hold: f64,
    speed_rate: f64,
    last_ias: f64,
    /// The pitch a protection settles to.
    protect_hold: f64,
    /// ALTS: the vertical speed it started capturing at.
    capture_rate: f64,
    /// Servos: where they hold the ailerons and the elevator, the elevator's steady part, and
    /// how long the elevator has been held off centre.
    aileron: f64,
    elevator: f64,
    elevator_hold: f64,
    trim_wait: f64,
    /// How long the pilot has been pushing the yoke past the servos.
    pushed: f64,
    /// Just engaged: the servos take the controls where they are at the next step.
    taking: bool,
}

impl Gfc700 {
    pub fn new(fit: Gfc700Fit) -> Gfc700 {
        Gfc700 {
            fit,
            ap: false,
            fd: false,
            lateral: LateralMode::Roll(0.0),
            vertical: VerticalMode::Pitch(0.0),
            gps_armed: false,
            approach_mode: false,
            glide_armed: false,
            alts: false,
            heading_bug: 0.0,
            selected_altitude: 0.0,
            plan: None,
            changed: [None; 2],
            protecting: None,
            alert: None,
            alerter: Alerter::new(),
            sensed: Sensed::default(),
            started: false,
            bank_cmd: 0.0,
            pitch_cmd: 0.0,
            alpha: 0.0,
            alpha_hold: 0.0,
            flc_hold: 0.0,
            speed_rate: 0.0,
            last_ias: 0.0,
            protect_hold: 0.0,
            capture_rate: 0.0,
            aileron: 0.0,
            elevator: 0.0,
            elevator_hold: 0.0,
            trim_wait: 0.0,
            pushed: 0.0,
            taking: false,
        }
    }

    pub fn fit(&self) -> &Gfc700Fit {
        &self.fit
    }

    /// The autopilot is engaged: its servos have the ailerons, elevator and trim.
    pub fn engaged(&self) -> bool {
        self.ap
    }

    /// The flight director is on (the autopilot always flies it; it may be on alone).
    pub fn director(&self) -> bool {
        self.fd
    }

    pub fn lateral(&self) -> LateralMode {
        self.lateral
    }

    pub fn vertical(&self) -> VerticalMode {
        self.vertical
    }

    /// GPS armed, waiting to capture its leg.
    pub fn gps_armed(&self) -> bool {
        self.fd && self.gps_armed
    }

    /// ALTS armed: it will level at the selected altitude.
    pub fn alts_armed(&self) -> bool {
        self.fd
            && matches!(self.vertical, VerticalMode::Pitch(_) | VerticalMode::VerticalSpeed(_) | VerticalMode::Flc(_))
            && (self.alts || (self.selected_altitude - self.sensed.altitude).abs() > ALTS_ARM)
    }

    /// GP armed, waiting for the glide path.
    pub fn glide_armed(&self) -> bool {
        self.fd && self.glide_armed
    }

    /// APR was pressed: GPS on the approach.
    pub fn approach_mode(&self) -> bool {
        self.approach_mode
    }

    /// Radians, magnetic.
    pub fn heading_bug(&self) -> f64 {
        self.heading_bug
    }

    /// m, on the altimeter.
    pub fn selected_altitude(&self) -> f64 {
        self.selected_altitude
    }

    pub fn plan(&self) -> Option<&FlightPlan> {
        self.plan.as_ref()
    }

    /// The flight director's command bars, (pitch, bank) in radians, while it is on.
    pub fn command(&self) -> Option<(f64, f64)> {
        self.fd.then_some((self.pitch_cmd, self.bank_cmd))
    }

    /// MAXSPD or MINSPD while a protection holds the pitch.
    pub fn protection(&self) -> Option<&'static str> {
        self.protecting.map(|over| if over { "MAXSPD" } else { "MINSPD" })
    }

    /// The last disconnect, while its alert is on.
    pub fn alert(&self) -> Option<Alert> {
        self.alert.filter(|a| !a.silenced && (a.why.abnormal() || self.sensed.time - a.since < FLASH_NORMAL))
    }

    /// The two-tone sounds now: for `TONE_NORMAL` after the pilot disconnects, and until he
    /// acknowledges it after the autopilot lets go by itself.
    pub fn tone(&self) -> bool {
        self.alert().is_some_and(|a| a.why.abnormal() || self.sensed.time - a.since < TONE_NORMAL)
    }

    /// The altitude alerter.
    pub fn altitude_alert(&self) -> AltitudeAlert {
        self.alerter.alert(self.sensed.time)
    }

    /// The lateral (`false`) or vertical (`true`) mode became active by itself lately: it flashes.
    pub fn flashing(&self, vertical: bool) -> bool {
        self.changed[usize::from(vertical)].is_some_and(|t| self.sensed.time - t < MODE_FLASH)
    }

    /// How far the aircraft is above the glide path, m, while an approach is loaded and it is on
    /// its final legs.
    pub fn glide_deviation(&self) -> Option<f64> {
        let plan = self.plan.as_ref()?;
        let a = plan.approach?;
        let s = &self.sensed;
        (a.before_threshold(s.north, s.east) < IF + NM && plan.active + 2 >= plan.legs.len())
            .then(|| s.gps_altitude - a.path_altitude(s.north, s.east))
    }

    /// The names of the modes as the PFD's bar shows them: lateral active and armed, vertical
    /// active and armed (empty when there is none).
    pub fn annunciations(&self) -> [&'static str; 4] {
        if !self.fd {
            return [""; 4];
        }
        let lateral = match self.lateral {
            LateralMode::Roll(_) => "ROL",
            LateralMode::Heading => "HDG",
            LateralMode::Gps => "GPS",
        };
        let vertical = match self.vertical {
            VerticalMode::Pitch(_) => "PIT",
            VerticalMode::Altitude(_) => "ALT",
            VerticalMode::Capture => "ALTS",
            VerticalMode::VerticalSpeed(_) => "VS",
            VerticalMode::Flc(_) => "FLC",
            VerticalMode::GlidePath => "GP",
        };
        let armed = if self.glide_armed() {
            "GP"
        } else if self.alts_armed() {
            "ALTS"
        } else {
            ""
        };
        [lateral, if self.gps_armed() { "GPS" } else { "" }, vertical, armed]
    }

    // ---- Keys and knobs

    /// A key pressed. False if it does nothing now (VNV and BC are not fitted; NAV and APR need a
    /// flight plan; the autopilot does not engage on the ground).
    pub fn press(&mut self, key: Key) -> bool {
        let s = self.sensed;
        match key {
            Key::Ap if self.ap => self.disconnect(Disconnect::Pilot),
            Key::Ap => {
                if s.on_ground || s.crashed || !self.started || beyond_limits(&s) {
                    return false;
                }
                self.ap = true;
                self.alert = None;
                if !self.fd {
                    self.director_on();
                }
                self.taking = true;
                self.pushed = 0.0;
                self.trim_wait = 0.0;
            }
            Key::Fd if self.ap => return false,
            Key::Fd if self.fd => self.fd = false,
            Key::Fd => self.director_on(),
            Key::Disconnect => {
                if self.ap {
                    self.disconnect(Disconnect::Pilot);
                } else if let Some(alert) = &mut self.alert {
                    alert.silenced = true;
                } else {
                    return false;
                }
            }
            Key::HeadingSync => self.heading_bug = s.heading,
            Key::Hdg => {
                self.ensure_director();
                if self.lateral == LateralMode::Heading {
                    self.set_lateral(self.roll_hold());
                } else {
                    self.set_lateral(LateralMode::Heading);
                }
            }
            Key::Nav | Key::Apr => {
                let approach = key == Key::Apr;
                let Some(plan) = &self.plan else { return false };
                if approach && plan.approach.is_none() {
                    return false;
                }
                let on = (self.lateral == LateralMode::Gps || self.gps_armed) && self.approach_mode == approach;
                self.ensure_director();
                if on {
                    // Pressed again: back to roll hold (or just disarmed).
                    if self.lateral == LateralMode::Gps {
                        self.set_lateral(self.roll_hold());
                    }
                    self.gps_armed = false;
                    self.approach_mode = false;
                    self.glide_armed = false;
                    if self.vertical == VerticalMode::GlidePath {
                        self.set_vertical(VerticalMode::Pitch(s.pitch));
                    }
                } else {
                    self.approach_mode = approach;
                    self.glide_armed = approach;
                    if self.lateral == LateralMode::Gps || self.captures(self.plan.as_ref().expect("a plan")) {
                        self.set_lateral(LateralMode::Gps);
                        self.gps_armed = false;
                    } else {
                        self.gps_armed = true;
                    }
                }
            }
            Key::Alt => {
                self.ensure_director();
                if matches!(self.vertical, VerticalMode::Altitude(_)) {
                    self.set_vertical(VerticalMode::Pitch(s.pitch));
                } else {
                    let altitude = (s.altitude / (10.0 * FEET)).round() * 10.0 * FEET;
                    self.set_vertical(VerticalMode::Altitude(altitude));
                }
            }
            Key::Vs => {
                self.ensure_director();
                if matches!(self.vertical, VerticalMode::VerticalSpeed(_)) {
                    self.set_vertical(VerticalMode::Pitch(s.pitch));
                } else {
                    let rate = clamp((s.vs / VS_STEP).round() * VS_STEP, VS_RANGE.0, VS_RANGE.1);
                    self.set_vertical(VerticalMode::VerticalSpeed(rate));
                }
            }
            Key::Flc => {
                self.ensure_director();
                if matches!(self.vertical, VerticalMode::Flc(_)) {
                    self.set_vertical(VerticalMode::Pitch(s.pitch));
                } else {
                    let speed = clamp((s.ias / KNOTS).round() * KNOTS, self.fit.min_speed, self.fit.max_speed);
                    self.set_vertical(VerticalMode::Flc(speed));
                }
            }
            Key::NoseUp | Key::NoseDown => {
                if !self.fd {
                    return false;
                }
                let up = if key == Key::NoseUp { 1.0 } else { -1.0 };
                self.vertical = match self.vertical {
                    VerticalMode::Pitch(p) => VerticalMode::Pitch(clamp(p + up * PITCH_STEP, -self.fit.pitch_down, self.fit.pitch_up)),
                    VerticalMode::VerticalSpeed(v) => VerticalMode::VerticalSpeed(clamp(v + up * VS_STEP, VS_RANGE.0, VS_RANGE.1)),
                    // Nose up is slower.
                    VerticalMode::Flc(v) => VerticalMode::Flc(clamp(v - up * KNOTS, self.fit.min_speed, self.fit.max_speed)),
                    VerticalMode::Altitude(a) => VerticalMode::Altitude(a + up * ALT_STEP),
                    VerticalMode::Capture | VerticalMode::GlidePath => return false,
                };
            }
            Key::Vnv | Key::Bc => return false,
        }
        true
    }

    /// A knob turned by `clicks` (positive clockwise).
    pub fn turn(&mut self, knob: Knob, clicks: i32) {
        let c = f64::from(clicks);
        match knob {
            Knob::Heading => {
                let degrees = (self.heading_bug / DEG).round() + c;
                self.heading_bug = (degrees * DEG).rem_euclid(TAU);
            }
            Knob::AltitudeFine | Knob::AltitudeCoarse => {
                let step = if knob == Knob::AltitudeFine { 100.0 } else { 1000.0 };
                let feet = (self.selected_altitude / FEET / step).round() * step + c * step;
                self.set_selected_altitude(feet * FEET);
            }
        }
    }

    /// VS at this rate, m/s (within its range), the flight director on: as the VS key and then the
    /// UP/DN wheel set it.
    pub fn set_vertical_speed(&mut self, rate: f64) {
        self.ensure_director();
        self.set_vertical(VerticalMode::VerticalSpeed(clamp(rate, VS_RANGE.0, VS_RANGE.1)));
    }

    /// Sets the heading bug, radians magnetic.
    pub fn set_heading_bug(&mut self, heading: f64) {
        self.heading_bug = heading.rem_euclid(TAU);
    }

    /// Sets the selected altitude, m. Changed while capturing it, the capture gives way to PIT.
    pub fn set_selected_altitude(&mut self, altitude: f64) {
        self.selected_altitude = clamp(altitude, SELECTED_RANGE.0, SELECTED_RANGE.1);
        self.alerter = Alerter::new();
        self.alts = false;
        if self.vertical == VerticalMode::Capture {
            self.set_vertical(VerticalMode::Pitch(self.sensed.pitch));
        }
    }

    /// D→: a leg from where the aircraft is now straight to an aerodrome (the middle of its runway).
    pub fn direct_to(&mut self, airport: &Airport) {
        let s = &self.sensed;
        let leg = Leg { from: (s.north, s.east), to: (airport.n, airport.e), name: format!("RW{:02}", airport.number()) };
        self.plan = Some(FlightPlan { legs: vec![leg], active: 0, approach: None });
    }

    /// Loads and activates a GPS approach to a runway: straight to the intermediate fix 9 NM out on
    /// its line, then the final approach fix at 5 NM and the runway; joined on its legs if the
    /// aircraft is already lined up within 3 NM of the line and between the fixes and the runway.
    pub fn load_approach(&mut self, approach: Approach) {
        let s = &self.sensed;
        let (i, f, t) = (approach.point(IF), approach.point(FAF), approach.point(0.0));
        let name = approach.name();
        let mut legs = vec![
            Leg { from: (s.north, s.east), to: i, name: "IF".to_string() },
            Leg { from: i, to: f, name: "FAF".to_string() },
            Leg { from: f, to: t, name },
        ];
        let before = approach.before_threshold(s.north, s.east);
        let (across, _) = legs[2].place(s.north, s.east);
        let aligned = remainder(s.track - approach.course(), TAU).abs() < 60.0 * DEG;
        let active = if aligned && across.abs() < 3.0 * NM && before > 0.0 && before < IF {
            if before > FAF { 1 } else { 2 }
        } else {
            0
        };
        if active > 0 {
            legs[0].from = legs[1].from;
        }
        self.plan = Some(FlightPlan { legs, active, approach: Some(approach) });
    }

    /// The pilot pushing or pulling the yoke this hard (0 to 1 of its travel) for `dt` seconds:
    /// held past `OVERPOWER` for `OVERPOWER_TIME`, the servos are overpowered and it lets go.
    pub fn push_yoke(&mut self, push: f64, dt: f64) {
        self.pushed = if push.abs() > OVERPOWER { self.pushed + dt } else { 0.0 };
        if self.ap && self.pushed > OVERPOWER_TIME {
            self.disconnect(Disconnect::Overpowered);
        }
    }

    /// The pilot works the manual electric trim: the autopilot lets go.
    pub fn manual_trim(&mut self) {
        if self.ap {
            self.disconnect(Disconnect::ManualTrim);
        }
    }

    // ---- Modes

    fn disconnect(&mut self, why: Disconnect) {
        self.ap = false;
        self.alert = Some(Alert { why, since: self.sensed.time, silenced: false });
    }

    /// The flight director on, in roll and pitch hold.
    fn director_on(&mut self) {
        self.fd = true;
        self.lateral = self.roll_hold();
        self.vertical = VerticalMode::Pitch(clamp(self.sensed.pitch, -self.fit.pitch_down, self.fit.pitch_up));
        self.gps_armed = false;
        self.approach_mode = false;
        self.glide_armed = false;
        self.changed = [None; 2];
        self.bank_cmd = self.sensed.roll;
        self.pitch_cmd = self.sensed.pitch;
    }

    /// A mode key with the flight director off turns it on with the other axis in its hold.
    fn ensure_director(&mut self) {
        if !self.fd {
            self.director_on();
        }
    }

    /// ROL as it starts now: wings level if nearly so, else the bank (within the limit).
    fn roll_hold(&self) -> LateralMode {
        let roll = self.sensed.roll;
        LateralMode::Roll(if roll.abs() < ROLL_LEVEL { 0.0 } else { clamp(roll, -self.fit.bank_limit, self.fit.bank_limit) })
    }

    fn set_lateral(&mut self, mode: LateralMode) {
        self.lateral = mode;
        self.changed[0] = None;
    }

    fn set_vertical(&mut self, mode: VerticalMode) {
        let s = self.sensed;
        match mode {
            VerticalMode::Pitch(p) => self.vertical = VerticalMode::Pitch(clamp(p, -self.fit.pitch_down, self.fit.pitch_up)),
            VerticalMode::Flc(_) => {
                self.flc_hold = s.pitch;
                self.vertical = mode;
            }
            _ => {
                self.alpha_hold = self.alpha;
                self.vertical = mode;
            }
        }
        self.changed[1] = None;
    }

    /// Whether the GPS can take its active leg now: near it, or closing on it.
    fn captures(&self, plan: &FlightPlan) -> bool {
        let s = &self.sensed;
        let leg = plan.active_leg();
        let (across, _) = leg.place(s.north, s.east);
        let closing = -across.signum() * s.ground_speed * remainder(s.track - leg.course(), TAU).sin();
        across.abs() < CAPTURE_NEAR || (closing > 0.0 && across.abs() < closing * CAPTURE_TIME)
    }

    /// Armed modes that capture, the selected altitude that is captured, a plan's next leg.
    fn transitions(&mut self) {
        let s = self.sensed;
        if let Some(plan) = &mut self.plan {
            let leg = plan.active_leg();
            if plan.active + 1 < plan.legs.len() {
                let next = &plan.legs[plan.active + 1];
                let (_, to_go) = leg.place(s.north, s.east);
                let turn = remainder(next.course() - leg.course(), TAU).abs();
                let speed = s.ground_speed.max(25.0);
                let radius = speed * speed / (GRAVITY * s.standard_bank().min(self.fit.bank_limit).tan());
                if to_go < radius * (turn.min(150.0 * DEG) / 2.0).tan() + speed * 2.0 {
                    plan.active += 1;
                }
            }
        }
        if self.gps_armed && self.plan.as_ref().is_some_and(|p| self.captures(p)) {
            self.gps_armed = false;
            self.lateral = LateralMode::Gps;
            self.changed[0] = Some(s.time);
        }
        if self.glide_armed && self.lateral == LateralMode::Gps && self.approach_mode {
            if let Some(off) = self.glide_deviation() {
                if off > GLIDE_CAPTURE.0 && off < GLIDE_CAPTURE.1 {
                    self.glide_armed = false;
                    self.alpha_hold = self.alpha;
                    self.vertical = VerticalMode::GlidePath;
                    self.changed[1] = Some(s.time);
                }
            }
        }
        let to_go = self.selected_altitude - s.altitude;
        let climbing = matches!(self.vertical, VerticalMode::Pitch(_) | VerticalMode::VerticalSpeed(_) | VerticalMode::Flc(_));
        if !climbing {
            self.alts = false;
        } else if to_go.abs() > ALTS_ARM {
            self.alts = true;
        }
        match self.vertical {
            _ if climbing && self.alts => {
                // Capture begins where the climb at this rate would be asked to ease.
                if to_go * s.vs > 0.0 && to_go.abs() < s.vs.abs() / CAPTURE_GAIN {
                    self.alts = false;
                    self.capture_rate = s.vs.abs();
                    self.alpha_hold = self.alpha;
                    self.vertical = VerticalMode::Capture;
                    self.changed[1] = Some(s.time);
                }
            }
            VerticalMode::Capture if to_go.abs() < CAPTURED => {
                self.vertical = VerticalMode::Altitude(self.selected_altitude);
                self.changed[1] = Some(s.time);
            }
            _ => {}
        }
    }

    // ---- The flight director

    /// The bank wanted now.
    fn bank_wanted(&self) -> f64 {
        let s = &self.sensed;
        let limit = self.fit.bank_limit;
        let turning = s.standard_bank().min(limit);
        match self.lateral {
            LateralMode::Roll(bank) => bank,
            LateralMode::Heading => clamp(TURN_GAIN * remainder(self.heading_bug - s.heading, TAU), -turning, turning),
            LateralMode::Gps => {
                let Some(plan) = &self.plan else { return 0.0 };
                let leg = plan.active_leg();
                let (across, _) = leg.place(s.north, s.east);
                let time = if self.approach_mode { APPROACH_LINE_TIME } else { LINE_TIME };
                let intercept = clamp((-across / (s.ground_speed.max(25.0) * time)).atan(), -45.0 * DEG, 45.0 * DEG);
                clamp(TURN_GAIN * remainder(leg.course() + intercept - s.track, TAU), -turning, turning)
            }
        }
    }

    /// Pitch for a vertical speed, through the flight path and the angle of attack it holds.
    fn pitch_for_rate(&mut self, rate: f64, dt: f64) -> f64 {
        let s = self.sensed;
        let tas = s.tas.max(20.0);
        let wanted = (rate / tas).clamp(-0.5, 0.5);
        // The flight path from the pitch, less the angle of attack the air data have shown over the
        // last seconds, as quick as the attitude, for the proportional part; the air data's own for
        // the integral, which settles on what they read.
        let error = wanted - (s.pitch - self.alpha);
        let read = wanted - s.path();
        // The integral waits while the pitch is against a limit.
        let (low, high) = (-self.fit.pitch_down, self.fit.pitch_up);
        if (self.pitch_cmd < high - 0.2 * DEG || read < 0.0) && (self.pitch_cmd > low + 0.2 * DEG || read > 0.0) {
            self.alpha_hold = clamp(self.alpha_hold + read * PATH_I * dt, ALPHA_RANGE.0, ALPHA_RANGE.1);
        }
        // A banked wing needs more angle of attack for the same lift.
        let banked = 0.08 * (1.0 / s.roll.cos().max(0.7) - 1.0);
        wanted + self.alpha_hold + error * PATH_P + banked
    }

    /// Pitch for an indicated airspeed: nose up when faster.
    fn pitch_for_speed(&mut self, speed: f64, dt: f64) -> f64 {
        let mut hold = self.flc_hold;
        let pitch = self.speed_law(&mut hold, speed, dt);
        self.flc_hold = hold;
        pitch
    }

    /// FLC's law on `hold`, the pitch it moves.
    fn speed_law(&self, hold: &mut f64, speed: f64, dt: f64) -> f64 {
        let wanted = clamp(SPEED_GAIN * (speed - self.sensed.ias), -ACCEL_MOST, ACCEL_MOST);
        *hold = clamp(*hold + SPEED_PITCH * (self.speed_rate - wanted) / GRAVITY * dt, -self.fit.pitch_down, self.fit.pitch_up);
        *hold
    }

    /// The pitch wanted now by the vertical mode, then by the protections.
    fn pitch_wanted(&mut self, dt: f64) -> f64 {
        let s = self.sensed;
        let pitch = match self.vertical {
            VerticalMode::Pitch(p) => p,
            VerticalMode::Altitude(a) => self.pitch_for_rate(clamp(ALT_GAIN * (a - s.altitude), -ALT_RATE, ALT_RATE), dt),
            VerticalMode::Capture => {
                let most = self.capture_rate.max(1.0);
                let to_go = self.selected_altitude - s.altitude;
                self.pitch_for_rate(clamp(CAPTURE_GAIN * to_go, -most, most), dt)
            }
            VerticalMode::VerticalSpeed(v) => self.pitch_for_rate(v, dt),
            VerticalMode::Flc(v) => {
                let mut pitch = self.pitch_for_speed(v, dt);
                // It never climbs away from a selected altitude below, nor dives away from one above.
                let level = self.alpha + 0.08 * (1.0 / s.roll.cos().max(0.7) - 1.0);
                let to_go = self.selected_altitude - s.altitude;
                if to_go > ALTS_ARM {
                    pitch = pitch.max(level);
                } else if to_go < -ALTS_ARM {
                    pitch = pitch.min(level);
                }
                pitch
            }
            VerticalMode::GlidePath => {
                let off = self.glide_deviation().unwrap_or(0.0);
                let descent = s.ground_speed * GLIDE_PATH.tan();
                self.pitch_for_rate(clamp(-descent - GLIDE_GAIN * off, -2.0 * descent - 1.0, 1.0), dt)
            }
        };
        // MAXSPD and MINSPD, a few seconds ahead: the airspeed comes before the mode, except in
        // those that hold a height (ALT, ALTS, GP), where they only warn: power is the pilot's.
        let ahead = s.ias + self.speed_rate * PROTECT_LEAD;
        let (over, under) = (ahead - self.fit.max_speed, self.fit.min_speed - ahead);
        let was = self.protecting;
        self.protecting = match was {
            _ if over > 0.0 => Some(true),
            _ if under > 0.0 => Some(false),
            Some(true) if s.ias > self.fit.max_speed - PROTECT_MARGIN => Some(true),
            Some(false) if s.ias < self.fit.min_speed + PROTECT_MARGIN => Some(false),
            _ => None,
        };
        let pitches = matches!(self.vertical, VerticalMode::Pitch(_) | VerticalMode::VerticalSpeed(_) | VerticalMode::Flc(_));
        let protected = match self.protecting {
            Some(over) if pitches => {
                if was != self.protecting {
                    self.protect_hold = s.pitch;
                }
                // FLC's law, to the limit.
                let mut hold = self.protect_hold;
                self.speed_law(&mut hold, if over { self.fit.max_speed } else { self.fit.min_speed }, dt);
                self.protect_hold = hold;
                if over { pitch.max(hold) } else { pitch.min(hold) }
            }
            _ => pitch,
        };
        clamp(protected, -self.fit.pitch_down, self.fit.pitch_up)
    }

    /// Reads the G1000, moves the modes on and works out the flight director's commands.
    fn update(&mut self, obs: &Observation, dt: f64) {
        let s = Sensed::of(obs);
        if !self.started {
            self.started = true;
            self.heading_bug = s.heading;
            self.selected_altitude = (s.altitude / (100.0 * FEET)).round() * 100.0 * FEET;
            self.last_ias = s.ias;
            self.alpha = s.pitch - s.path();
        }
        self.sensed = s;
        self.alpha += (s.pitch - s.path() - self.alpha) * (dt / ALPHA_LAG).min(1.0);
        let accel = (s.ias - self.last_ias) / dt.max(1e-6);
        self.speed_rate += (accel - self.speed_rate) * (dt / SPEED_LAG).min(1.0);
        self.last_ias = s.ias;
        self.alerter.update(self.selected_altitude - s.altitude, s.time);

        if self.ap && s.crashed {
            self.disconnect(Disconnect::Crashed);
        } else if self.ap && beyond_limits(&s) {
            self.disconnect(Disconnect::Attitude);
        }
        if !self.fd {
            self.bank_cmd = s.roll;
            self.pitch_cmd = s.pitch;
            return;
        }
        self.transitions();
        let bank = self.bank_wanted();
        self.bank_cmd += clamp(bank - self.bank_cmd, -BANK_RATE * dt, BANK_RATE * dt);
        let pitch = self.pitch_wanted(dt);
        self.pitch_cmd += clamp(pitch - self.pitch_cmd, -PITCH_RATE * dt, PITCH_RATE * dt);
    }

    // ---- The servos

    /// The servos for one step: ailerons and elevator towards the flight director's commands, within
    /// their travel and rate, and the trim run to take the elevator's load off.
    fn servos(&mut self, current: &AircraftControls, dt: f64) -> AircraftControls {
        let s = self.sensed;
        let fit = self.fit;
        let ias = (s.ias / KNOTS).max(1.0);
        let power = clamp((80.0 / ias) * (80.0 / ias), 0.5, 2.5);

        let roll = clamp(((self.bank_cmd - s.roll) * ROLL_P - s.rates[0] * ROLL_D) * power.min(1.5), -fit.roll_authority, fit.roll_authority);
        self.aileron += clamp(roll - self.aileron, -fit.roll_rate * dt, fit.roll_rate * dt);

        let correction = clamp(((self.pitch_cmd - s.pitch) * PITCH_P - s.rates[1] * PITCH_D) * power, -fit.pitch_authority, fit.pitch_authority);
        self.elevator_hold = clamp(self.elevator_hold + correction * PITCH_I * dt, -fit.pitch_authority, fit.pitch_authority);
        let pitch = clamp(self.elevator_hold + correction, -fit.pitch_authority, fit.pitch_authority);
        self.elevator += clamp(pitch - self.elevator, -fit.pitch_rate * dt, fit.pitch_rate * dt);

        // The trim servo takes over what the elevator servo has held for a while.
        let load = self.elevator.abs() - TRIM_DEADBAND;
        self.trim_wait = if load > 0.0 { self.trim_wait + dt } else { 0.0 };
        let trim = if self.trim_wait > TRIM_DELAY {
            let run = fit.trim_rate * (load / (TRIM_FULL - TRIM_DEADBAND)).min(1.0);
            clamp(current.trim + self.elevator.signum() * run * dt, -1.0, 1.0)
        } else {
            current.trim
        };
        AircraftControls { aileron: self.aileron, elevator: self.elevator, trim, ..*current }
    }
}

/// Beyond the attitudes the autopilot flies in.
fn beyond_limits(s: &Sensed) -> bool {
    s.roll.abs() > ATTITUDE_LIMITS[0] || s.pitch > ATTITUDE_LIMITS[1] || s.pitch < -ATTITUDE_LIMITS[2]
}

impl Gfc700 {
    /// `Pilot::fly` for an aircraft. `current` is what the pilot's hands set: returned as it is with
    /// the autopilot off, with the ailerons, the elevator and the trim the servos' with it on.
    pub fn fly_aircraft(&mut self, obs: &Observation, current: &AircraftControls, dt: f64) -> AircraftControls {
        self.update(obs, dt);
        if !self.ap {
            return *current;
        }
        if std::mem::take(&mut self.taking) {
            // Engaged since the last step: the servos start where the controls are.
            self.aileron = current.aileron;
            self.elevator = current.elevator;
            self.elevator_hold = current.elevator;
        }
        self.servos(current, dt)
    }
}

impl Pilot for Gfc700 {
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
