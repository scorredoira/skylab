//! The observation vector: one table that both builds it and describes it.
//!
//! Every entry is `raw value / scale`, clipped to ±`CLIP`. The table is the single source of
//! truth: `write` walks it to fill the vector, the protocol's `describe` message and the model
//! file's compatibility check print it. Adding, removing or reordering a row changes the layout
//! for everyone at once (and makes older model files fail to load, by design).
//!
//! **Images.** A camera image is not part of this vector. When one is added it will be a second,
//! separate array per environment (announced as `observation.image` in `describe`, today `null`),
//! so this layout and every model trained on it stay valid.

use std::f64::consts::PI;

use std::sync::OnceLock;

use sim_core::{remainder, AircraftControls, AircraftObservation, CarObservation, Controls, MultirotorObservation, Observation};

use crate::guidance::{DriveTarget, RunwayView, Targets};

/// Largest magnitude of any entry.
pub const CLIP: f32 = 10.0;

/// Everything an entry can be computed from.
pub struct Inputs<'a> {
    pub obs: &'a Observation,
    /// The controls as they are now (the `current` a `Pilot` receives).
    pub controls: &'a Controls,
    pub targets: &'a Targets,
}

impl Inputs<'_> {
    fn runway(&self) -> RunwayView {
        self.targets.runway.unwrap_or_default()
    }

    /// What an aircraft observes of itself; all zero for another vehicle (`FIELDS` is the
    /// aircraft's table, `MULTIROTOR_FIELDS` the multirotor's: `fields` picks one by the task).
    fn air(&self) -> &AircraftObservation {
        static NONE: OnceLock<AircraftObservation> = OnceLock::new();
        self.obs.aircraft().unwrap_or_else(|| NONE.get_or_init(AircraftObservation::default))
    }

    fn aircraft_controls(&self) -> AircraftControls {
        match self.controls {
            Controls::Aircraft(c) => *c,
            _ => AircraftControls::default(),
        }
    }

    /// What a car's driver observes of it; all zero for another vehicle.
    fn car(&self) -> CarObservation {
        self.obs.car().copied().unwrap_or_default()
    }

    /// Where the car is on its circuit (`drive`); all zero without one.
    fn drive(&self) -> DriveTarget {
        self.targets.drive.unwrap_or_default()
    }

    /// What a multirotor observes of itself; all zero for another vehicle.
    fn rotor(&self) -> MultirotorObservation {
        self.obs.multirotor().cloned().unwrap_or_default()
    }

    /// Where the point to hover over is from here, in the axes of the heading: metres ahead and to the right.
    fn point(&self) -> (f64, f64) {
        let Some((n, e)) = self.targets.point else { return (0.0, 0.0) };
        let (dn, de) = (n - self.obs.north, e - self.obs.east);
        let (c, s) = (self.obs.heading.cos(), self.obs.heading.sin());
        (dn * c + de * s, -dn * s + de * c)
    }

    /// The `k`th gate to fly through (0 the next) in the axes of the heading: where the middle of
    /// its opening is (ahead, right, up, m) and the way through it (a unit vector).
    fn gate(&self, k: usize) -> ([f64; 3], [f64; 3]) {
        let Some(g) = self.targets.gates[k] else { return ([0.0; 3], [0.0; 3]) };
        let (c, s) = (self.obs.heading.cos(), self.obs.heading.sin());
        let turn = |[n, e, d]: [f64; 3]| [n * c + e * s, -n * s + e * c, -d];
        let rel = [g.middle[0] - self.obs.north, g.middle[1] - self.obs.east, g.middle[2] + self.obs.altitude];
        (turn(rel), turn(g.through))
    }

    /// The velocity over the ground in the axes of the heading: ahead, right, up.
    fn velocity(&self) -> [f64; 3] {
        let [n, e, d] = self.obs.velocity;
        let (c, s) = (self.obs.heading.cos(), self.obs.heading.sin());
        [n * c + e * s, -n * s + e * c, -d]
    }
}

/// One entry of the observation vector.
#[derive(Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    /// The raw value (SI units, radians) is divided by this.
    pub scale: f32,
    pub description: &'static str,
    value: fn(&Inputs) -> f64,
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

macro_rules! field {
    ($name:literal, $scale:expr, $description:literal, $value:expr) => {
        Field {
            name: $name,
            scale: $scale,
            description: $description,
            value: $value,
        }
    };
}

/// The layout, in order.
pub static FIELDS: &[Field] = &[
    // What the instruments show.
    field!("airspeed", 50.0, "indicated airspeed, m/s", |x| x.air().indicated_airspeed),
    field!("ground_speed", 50.0, "speed over the ground, m/s", |x| x
        .obs
        .speed),
    field!("agl", 1000.0, "height above the ground below, m", |x| x
        .obs
        .agl),
    field!(
        "vertical_speed",
        10.0,
        "vertical speed, m/s, positive up",
        |x| x.air().vertical_speed
    ),
    field!("pitch", 0.5, "pitch, rad, positive nose up", |x| x
        .obs
        .pitch),
    field!("roll", 1.0, "roll, rad, positive right wing down", |x| x
        .obs
        .roll),
    field!("heading_sin", 1.0, "sine of the heading", |x| x
        .obs
        .heading
        .sin()),
    field!("heading_cos", 1.0, "cosine of the heading", |x| x
        .obs
        .heading
        .cos()),
    field!("roll_rate", 1.0, "body roll rate, rad/s", |x| x
        .obs
        .angular_rates[0]),
    field!("pitch_rate", 1.0, "body pitch rate, rad/s", |x| x
        .obs
        .angular_rates[1]),
    field!("yaw_rate", 1.0, "body yaw rate, rad/s", |x| x
        .obs
        .angular_rates[2]),
    field!("alpha", 0.25, "angle of attack, rad", |x| x.air().alpha),
    field!("beta", 0.25, "sideslip, rad", |x| x.air().beta),
    field!("slip_ball", 0.2, "slip ball, rad", |x| x.air().slip_ball),
    field!("rpm", 2700.0, "engine speed, rev/min", |x| x.air().rpm),
    // Where the controls are.
    field!("elevator", 1.0, "yoke back (+1) or forward (-1)", |x| x.aircraft_controls().elevator),
    field!("aileron", 1.0, "yoke right (+1) or left (-1)", |x| x.aircraft_controls().aileron),
    field!("rudder", 1.0, "right pedal (+1) or left (-1)", |x| x.aircraft_controls().rudder),
    field!("throttle", 1.0, "throttle, 0 closed to 1 open", |x| x.air().throttle),
    field!("flap", 30.0, "actual flap deflection, degrees", |x| x.air().flap_deg),
    field!("trim", 1.0, "elevator trim, +1 full nose up", |x| x.air().trim),
    field!("on_ground", 1.0, "1 with weight on the wheels", |x| flag(
        x.obs.on_ground
    )),
    field!("stall_warning", 1.0, "1 while the stall horn sounds", |x| {
        flag(x.air().stall_warning)
    }),
    // The task: targets (0 when the task has none).
    field!(
        "heading_error",
        1.0,
        "target heading minus heading, rad in (-pi, pi]; positive: turn right",
        |x| {
            x.targets
                .heading
                .map_or(0.0, |h| remainder(h - x.obs.heading, 2.0 * PI))
        }
    ),
    field!(
        "altitude_error",
        100.0,
        "target altitude minus altitude, m; positive: climb",
        |x| { x.targets.altitude.map_or(0.0, |a| a - x.obs.altitude) }
    ),
    field!(
        "speed_error",
        10.0,
        "target airspeed minus indicated airspeed, m/s",
        |x| {
            x.targets
                .airspeed
                .map_or(0.0, |v| v - x.air().indicated_airspeed)
        }
    ),
    // The task: the runway in use (all 0 when the task has none).
    field!(
        "has_runway",
        1.0,
        "1 when the runway terms below mean something",
        |x| flag(x.targets.runway.is_some())
    ),
    field!(
        "runway_along",
        1000.0,
        "metres past the threshold along the runway (negative on the approach)",
        |x| x.runway().along
    ),
    field!(
        "runway_across",
        100.0,
        "metres to the right of the runway centreline",
        |x| x.runway().across
    ),
    field!(
        "runway_height",
        100.0,
        "height of the wheels above the runway, m",
        |x| x.runway().height
    ),
    field!(
        "path_error",
        50.0,
        "height above the 3 degree approach path, m",
        |x| x.runway().path_error
    ),
    field!(
        "runway_heading_error",
        1.0,
        "runway direction minus heading, rad in (-pi, pi]",
        |x| x.runway().heading_error
    ),
    field!(
        "runway_distance",
        5000.0,
        "horizontal distance to the aiming point, m",
        |x| x.runway().distance
    ),
    field!(
        "leg",
        5.0,
        "leg of the circuit: 0 none, 1 upwind, 2 crosswind, 3 downwind, 4 base, 5 final",
        |x| f64::from(x.targets.leg)
    ),
];

/// A multirotor's layout (the `hover` task), in order: what it feels and its sticks, and where
/// the point, the height and the heading it is asked for are.
pub static MULTIROTOR_FIELDS: &[Field] = &[
    field!("agl", 20.0, "height above the ground below, m", |x| x.obs.agl),
    field!("vertical_speed", 5.0, "vertical speed, m/s, positive up", |x| x.rotor().vertical_speed),
    field!("forward_speed", 10.0, "speed over the ground along the heading, m/s", |x| x.velocity()[0]),
    field!("right_speed", 10.0, "speed over the ground to the right of the heading, m/s", |x| x.velocity()[1]),
    field!("pitch", 0.5, "pitch, rad, positive nose up", |x| x.obs.pitch),
    field!("roll", 0.5, "roll, rad, positive right side down", |x| x.obs.roll),
    field!("heading_sin", 1.0, "sine of the heading", |x| x.obs.heading.sin()),
    field!("heading_cos", 1.0, "cosine of the heading", |x| x.obs.heading.cos()),
    field!("roll_rate", 5.0, "body roll rate, rad/s", |x| x.obs.angular_rates[0]),
    field!("pitch_rate", 5.0, "body pitch rate, rad/s", |x| x.obs.angular_rates[1]),
    field!("yaw_rate", 5.0, "body yaw rate, rad/s", |x| x.obs.angular_rates[2]),
    field!("motor_1", 3000.0, "speed of the first rotor (rear right), rad/s", |x| x.rotor().motor_speed[0]),
    field!("motor_2", 3000.0, "speed of the second rotor (front right), rad/s", |x| x.rotor().motor_speed[1]),
    field!("motor_3", 3000.0, "speed of the third rotor (rear left), rad/s", |x| x.rotor().motor_speed[2]),
    field!("motor_4", 3000.0, "speed of the fourth rotor (front left), rad/s", |x| x.rotor().motor_speed[3]),
    field!("battery", 16.8, "battery voltage, V", |x| x.rotor().battery_voltage),
    field!("throttle", 1.0, "throttle stick, 0 down to 1 up", |x| x.rotor().throttle),
    field!("roll_stick", 1.0, "roll stick, +1 full right", |x| x.rotor().roll),
    field!("pitch_stick", 1.0, "pitch stick, +1 full back (nose up)", |x| x.rotor().pitch),
    field!("yaw_stick", 1.0, "yaw stick, +1 full right", |x| x.rotor().yaw),
    field!("armed", 1.0, "1 while the motors are armed", |x| flag(x.rotor().armed)),
    field!("on_ground", 1.0, "1 while it touches the ground", |x| flag(x.obs.on_ground)),
    field!(
        "heading_error",
        1.0,
        "target heading minus heading, rad in (-pi, pi]; positive: turn right",
        |x| x.targets.heading.map_or(0.0, |h| remainder(h - x.obs.heading, 2.0 * PI))
    ),
    field!("altitude_error", 10.0, "target altitude minus altitude, m; positive: climb", |x| x
        .targets
        .altitude
        .map_or(0.0, |a| a - x.obs.altitude)),
    field!("point_ahead", 10.0, "the point to hover over, metres ahead along the heading", |x| x.point().0),
    field!("point_right", 10.0, "the point to hover over, metres to the right of the heading", |x| x.point().1),
];

/// What the `gates` task sees besides the multirotor's table: the next two gates.
pub static GATE_FIELDS: &[Field] = &[
    field!("gate_ahead", 50.0, "the middle of the next gate's opening, metres ahead along the heading", |x| x.gate(0).0[0]),
    field!("gate_right", 50.0, "the middle of the next gate's opening, metres to the right of the heading", |x| x.gate(0).0[1]),
    field!("gate_up", 20.0, "the middle of the next gate's opening, metres above", |x| x.gate(0).0[2]),
    field!("gate_through_ahead", 1.0, "the way through the next gate, along the heading (a unit vector)", |x| x.gate(0).1[0]),
    field!("gate_through_right", 1.0, "the way through the next gate, to the right of the heading", |x| x.gate(0).1[1]),
    field!("gate_through_up", 1.0, "the way through the next gate, up (-1: a dive gate)", |x| x.gate(0).1[2]),
    field!("gate2_ahead", 50.0, "the middle of the gate after it, metres ahead along the heading", |x| x.gate(1).0[0]),
    field!("gate2_right", 50.0, "the middle of the gate after it, metres to the right of the heading", |x| x.gate(1).0[1]),
    field!("gate2_up", 20.0, "the middle of the gate after it, metres above", |x| x.gate(1).0[2]),
    field!("gate2_through_ahead", 1.0, "the way through the gate after it, along the heading", |x| x.gate(1).1[0]),
    field!("gate2_through_right", 1.0, "the way through the gate after it, to the right of the heading", |x| x.gate(1).1[1]),
    field!("gate2_through_up", 1.0, "the way through the gate after it, up", |x| x.gate(1).1[2]),
];

/// A car's layout (the `drive` task), in order: how it moves, its engine and pedals, its tyres,
/// where it is on the circuit and what is ahead (the circuit's bends and the racing line).
pub static DRIVE_FIELDS: &[Field] = &[
    field!("speed", 50.0, "speed along the car, m/s (negative backwards)", |x| x.car().speed),
    field!("lateral_speed", 10.0, "speed over the ground to the right of the heading, m/s", |x| x.velocity()[1]),
    field!("yaw_rate", 2.0, "body yaw rate, rad/s, positive turning right", |x| x.obs.angular_rates[2]),
    field!("rpm", 8000.0, "engine speed, rpm", |x| x.car().rpm),
    field!("gear", 6.0, "gear: -1 reverse, 0 neutral, 1 first...", |x| f64::from(x.car().gear)),
    field!("steer", 1.0, "steering, -1 full left to 1 full right", |x| x.car().steer),
    field!("throttle", 1.0, "throttle pedal, 0 to 1", |x| x.car().throttle),
    field!("brake", 1.0, "brake pedal, 0 to 1", |x| x.car().brake),
    field!("slip_fl", 1.0, "longitudinal slip of the front left tyre (positive driving)", |x| x.car().slip[0]),
    field!("slip_fr", 1.0, "longitudinal slip of the front right tyre", |x| x.car().slip[1]),
    field!("slip_rl", 1.0, "longitudinal slip of the rear left tyre", |x| x.car().slip[2]),
    field!("slip_rr", 1.0, "longitudinal slip of the rear right tyre", |x| x.car().slip[3]),
    field!("slip_angle_fl", 0.3, "slip angle of the front left tyre, rad", |x| x.car().slip_angle[0]),
    field!("slip_angle_fr", 0.3, "slip angle of the front right tyre, rad", |x| x.car().slip_angle[1]),
    field!("slip_angle_rl", 0.3, "slip angle of the rear left tyre, rad", |x| x.car().slip_angle[2]),
    field!("slip_angle_rr", 0.3, "slip angle of the rear right tyre, rad", |x| x.car().slip_angle[3]),
    field!("off_asphalt", 4.0, "wheels on grass, gravel or earth (0 to 4)", |x| x
        .car()
        .surface
        .iter()
        .filter(|s| !matches!(s, sim_core::car::Surface::Asphalt | sim_core::car::Surface::Kerb | sim_core::car::Surface::Concrete))
        .count() as f64),
    field!("across", 10.0, "metres to the right of the circuit's centre line", |x| x.drive().across),
    field!("half_width", 10.0, "half the asphalt's width there, m", |x| x.drive().half_width),
    field!("track_heading_error", 1.0, "the circuit's direction minus the heading, rad; positive: it turns right of the car", |x| x.drive().heading_error),
    field!("curvature_0", 0.05, "the circuit's curvature here, 1/m, positive right", |x| x.drive().curvature[0]),
    field!("curvature_20", 0.05, "its curvature 20 m ahead", |x| x.drive().curvature[1]),
    field!("curvature_50", 0.05, "its curvature 50 m ahead", |x| x.drive().curvature[2]),
    field!("curvature_100", 0.05, "its curvature 100 m ahead", |x| x.drive().curvature[3]),
    field!("curvature_150", 0.05, "its curvature 150 m ahead", |x| x.drive().curvature[4]),
    field!("line_10", 10.0, "the racing line 10 m ahead, metres right of the centre line", |x| x.drive().line[0]),
    field!("line_30", 10.0, "the racing line 30 m ahead", |x| x.drive().line[1]),
    field!("line_60", 10.0, "the racing line 60 m ahead", |x| x.drive().line[2]),
    field!("line_speed_0", 50.0, "the racing line's speed here, m/s", |x| x.drive().line_speed[0]),
    field!("line_speed_50", 50.0, "the racing line's speed 50 m ahead, m/s", |x| x.drive().line_speed[1]),
    field!("line_speed_100", 50.0, "the racing line's speed 100 m ahead, m/s", |x| x.drive().line_speed[2]),
    field!("rain", 1.0, "how hard it rains, 0 dry to 1 a downpour (the racing line's speeds are for it)", |x| x.car().rain),
    field!("water", 2.0, "water under the tyres, mm (the mean of the four): a film in the rain, more in a puddle", |x| x.car().water.iter().sum::<f64>() / 4.0 * 1000.0),
    field!("water_20", 2.0, "water on the road 20 m ahead along the way the car goes, mm (a puddle shines)", |x| x.car().water_ahead[0] * 1000.0),
    field!("water_40", 2.0, "water on the road 40 m ahead, mm", |x| x.car().water_ahead[1] * 1000.0),
    field!("water_70", 2.0, "water on the road 70 m ahead, mm", |x| x.car().water_ahead[2] * 1000.0),
];

/// The `gates` task's table: the multirotor's, then `GATE_FIELDS`.
pub fn gates_fields() -> &'static [Field] {
    static TABLE: OnceLock<Vec<Field>> = OnceLock::new();
    TABLE.get_or_init(|| MULTIROTOR_FIELDS.iter().chain(GATE_FIELDS).copied().collect())
}

/// The table of a task: the multirotor's for `hover`, with the gates for `gates`, the aircraft's
/// for the rest.
pub fn fields(task: &crate::task::Task) -> &'static [Field] {
    match task {
        crate::task::Task::Gates(_) => gates_fields(),
        crate::task::Task::Drive(_) => DRIVE_FIELDS,
        _ if task.multirotor() => MULTIROTOR_FIELDS,
        _ => FIELDS,
    }
}

/// Length of the aircraft's observation vector.
pub fn len() -> usize {
    FIELDS.len()
}

/// Index of a field of the aircraft's table by name.
pub fn index(name: &str) -> Option<usize> {
    FIELDS.iter().position(|f| f.name == name)
}

/// Index of a field of a table by name.
pub fn index_in(fields: &[Field], name: &str) -> Option<usize> {
    fields.iter().position(|f| f.name == name)
}

/// Fills `out` (of length `len()`) with the aircraft's observation.
pub fn write(inputs: &Inputs, out: &mut [f32]) {
    write_with(FIELDS, inputs, out);
}

/// Fills `out` (as long as `fields`) with the observation by a table.
pub fn write_with(fields: &[Field], inputs: &Inputs, out: &mut [f32]) {
    assert_eq!(out.len(), fields.len());
    for (o, f) in out.iter_mut().zip(fields) {
        let v = ((f.value)(inputs) / f64::from(f.scale)) as f32;
        *o = if v.is_finite() {
            v.clamp(-CLIP, CLIP)
        } else {
            0.0
        };
    }
}
