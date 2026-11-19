//! The tasks: where an episode starts, what is rewarded and when it ends.
//!
//! Each task has three structs of plain numbers: its `*Config` (starts, envelope, time limit),
//! its `*Rewards` (every weight of the reward, in one place) and an `AirConfig` (wind and
//! turbulence ranges: calm by default, widen them as a curriculum). All of them can be listed and
//! set by name (`config.time_limit`, `rewards.crash`, `air.wind_max`), which is how the server
//! describes and overrides them.
//!
//! Rewards written "per second" are multiplied by the decision interval, so a return does not
//! depend on the decision rate.

use std::f64::consts::PI;

use std::sync::Arc;

use sim_core::car::driver::{Pace, RacingLine};
use sim_core::circuit::{Circuit, CircuitFile};
use sim_core::course::Track;
use sim_core::{
    act, atmosphere, euler_angles, inverse, quaternion, remainder, Aircraft, Airport, Body, CarType, Controls, MultirotorControls, Observation, Rng,
    Start, State, VehicleId, Weather, World, DEG, V3,
};

use crate::guidance::{
    leg, Guidance, Runway, RunwayView, Targets, AIMING_POINT, APPROACH_SPEED, CLIMB_SPEED,
    TAKEOFF_HEIGHT, RACE_HEIGHT,
};

/// A named number of a task.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub value: f64,
    pub description: &'static str,
}

/// A struct of `f64` fields that can be listed and set by name. The doc comment of a field is its
/// description.
macro_rules! params {
    ($(#[$meta:meta])* pub struct $name:ident { $( #[doc = $doc:literal] $field:ident : $default:expr, )* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name { $( #[doc = $doc] pub $field: f64, )* }

        impl Default for $name {
            fn default() -> $name { $name { $( $field: $default, )* } }
        }

        impl $name {
            pub fn params(&self) -> Vec<Param> {
                vec![ $( Param { name: stringify!($field), value: self.$field, description: $doc.trim_ascii() }, )* ]
            }

            pub fn set(&mut self, name: &str, value: f64) -> bool {
                match name {
                    $( stringify!($field) => { self.$field = value; true } )*
                    _ => false,
                }
            }
        }
    };
}

params! {
    /// Wind and turbulence of an episode, drawn uniformly from these ranges. Calm by default.
    pub struct AirConfig {
        /// lowest wind speed 10 m above the ground, m/s
        wind_min: 0.0,
        /// highest wind speed 10 m above the ground, m/s
        wind_max: 0.0,
        /// lowest turbulence, 0 still to 1 rough
        turbulence_min: 0.0,
        /// highest turbulence, 0 still to 1 rough
        turbulence_max: 0.0,
        /// on runway tasks the wind blows from within this many degrees of the runway direction (180 = from anywhere)
        crosswind_angle: 30.0,
    }
}

params! {
    /// `hold`: starts and limits.
    pub struct HoldConfig {
        /// seconds before the episode is truncated
        time_limit: 60.0,
        /// lowest true airspeed at the start and lowest target, m/s
        speed_min: 38.0,
        /// highest true airspeed at the start and highest target, m/s
        speed_max: 54.0,
        /// the start is within this many metres of the home airport, north and east
        position_spread: 3000.0,
        /// the start is 900 m above the ground plus or minus this, m
        altitude_spread: 150.0,
        /// largest bank at the start, degrees
        roll_spread: 30.0,
        /// largest pitch away from trim at the start, degrees
        pitch_spread: 8.0,
        /// largest angular rate at the start about each axis, degrees per second
        rate_spread: 5.0,
        /// the target heading is within this many degrees of the starting one
        heading_change: 90.0,
        /// the target altitude is within this many metres of the starting one
        altitude_change: 100.0,
        /// seconds flown hands-off before the first observation, so the instruments have settled
        settle: 0.5,
        /// envelope: altitude error beyond which the episode ends, m
        max_altitude_error: 400.0,
        /// envelope: lowest height above the ground, m
        min_agl: 150.0,
        /// envelope: largest bank, degrees
        max_roll: 100.0,
        /// envelope: largest pitch either way, degrees
        max_pitch: 60.0,
    }
}

params! {
    /// `hover` (a multirotor): starts and limits.
    pub struct HoverConfig {
        /// seconds before the episode is truncated
        time_limit: 30.0,
        /// the start is within this many metres of the home airport, north and east
        position_spread: 200.0,
        /// largest speed at the start, m/s, each way
        speed_spread: 2.0,
        /// largest angular rate at the start about each axis, degrees per second
        rate_spread: 30.0,
        /// the point to hover over is within this many metres of the start, north and east
        point_spread: 10.0,
        /// the height to hover at above the ground, m
        height: 10.0,
        /// plus or minus this, m
        height_spread: 4.0,
        /// the heading to face is within this many degrees of the starting one
        heading_change: 90.0,
        /// seconds flown before the first observation (the start's hover)
        settle: 0.1,
        /// envelope: farthest from the point, m
        max_distance: 40.0,
        /// envelope: largest height error, m
        max_height_error: 15.0,
        /// envelope: largest tilt, degrees
        max_tilt: 80.0,
    }
}

params! {
    /// `gates` (a multirotor): the course, starts and limits.
    pub struct GatesConfig {
        /// seconds before the episode is truncated
        time_limit: 150.0,
        /// the course, by its place in the list of courses (`sim_core::course::file::courses`; 0 Pinewood Airfield, 1 Deepwood Forest)
        course: 0.0,
        /// laps of an episode (0: the course's own)
        laps: 1.0,
        /// share of the episodes that start in flight before a gate drawn at random, instead of on the start's pad
        gate_starts: 0.5,
        /// a start on the pad is this many metres to the right of its middle (across the start's heading)
        pad_offset: 0.0,
        /// and anywhere up to this many metres either side of that, at random (side by side on the pads of a race)
        pad_spread: 0.0,
        /// such a start is this many metres before the gate, on the line through it
        gate_lead: 20.0,
        /// and flies at this speed towards it, m/s
        gate_speed: 6.0,
        /// envelope: farthest from the next gate, m (the longest leg of the built-in course is some 250 m)
        max_distance: 350.0,
        /// envelope: largest tilt, degrees
        max_tilt: 85.0,
    }
}

params! {
    /// `gates`: reward weights.
    pub struct GatesRewards {
        /// for the way made round the course: this much per gate's worth (from the last gate to the next)
        progress: 2.0,
        /// once for each gate flown through
        gate: 5.0,
        /// once on finishing the laps
        finish: 50.0,
        /// taken off once for each gate missed (flown past outside its opening, or another flown instead)
        missed: 3.0,
        /// per second, taken off: the clock
        time: 0.2,
        /// per second, taken off times the squared angular rate (rad/s)^2, at most 20
        rates: 0.005,
        /// per second, taken off times the mean squared speed of the sticks (1/s)^2
        smooth: 0.005,
        /// taken off once on a crash
        crash: 20.0,
        /// taken off once on leaving the envelope
        envelope: 20.0,
    }
}

params! {
    /// `hover`: reward weights.
    pub struct HoverRewards {
        /// per second, times exp(-distance from the point / 2 m)
        position: 0.4,
        /// per second, times exp(-|height error| / 1 m)
        height: 0.4,
        /// per second, times exp(-|heading error| / 0.3 rad)
        heading: 0.2,
        /// per second, taken off times the squared angular rate (rad/s)^2, at most 20
        rates: 0.02,
        /// per second, taken off times the mean squared speed of the sticks (1/s)^2
        smooth: 0.01,
        /// taken off once on a crash
        crash: 20.0,
        /// taken off once on leaving the envelope
        envelope: 20.0,
    }
}

params! {
    /// `hold`: reward weights.
    pub struct HoldRewards {
        /// per second, times exp(-|heading error| / 0.15 rad)
        heading: 0.35,
        /// per second, times exp(-|altitude error| / 20 m)
        altitude: 0.35,
        /// per second, times exp(-|airspeed error| / 3 m/s)
        speed: 0.2,
        /// per second, taken off times the squared angular rate (rad/s)^2, at most 5
        rates: 0.1,
        /// per second, taken off times (sideslip / 0.1 rad)^2, at most 5
        slip: 0.05,
        /// per second, taken off times the mean squared speed of yoke and pedals (1/s)^2
        smooth: 0.02,
        /// taken off once on a crash
        crash: 20.0,
        /// taken off once on leaving the envelope
        envelope: 20.0,
    }
}

params! {
    /// `takeoff`: starts and limits.
    pub struct TakeoffConfig {
        /// seconds before the episode is truncated
        time_limit: 120.0,
        /// the start is within this many metres of the centreline
        lateral_spread: 4.0,
        /// the start is within this many degrees of the runway direction
        heading_spread: 3.0,
        /// success: height above the runway to reach, m
        height: TAKEOFF_HEIGHT,
        /// success: heading within this many degrees of the runway direction
        success_heading: 20.0,
        /// success: within this many metres of the extended centreline
        success_across: 200.0,
        /// envelope: farthest from the extended centreline, m
        max_across: 400.0,
        /// envelope: largest heading error from the runway direction, degrees
        max_heading_error: 90.0,
        /// envelope: largest bank, degrees
        max_roll: 70.0,
    }
}

params! {
    /// `takeoff`: reward weights.
    pub struct TakeoffRewards {
        /// for gaining airspeed: this much in total from standstill to 74 knots
        speed: 10.0,
        /// for gaining height: this much in total from the runway to the success height
        climb: 20.0,
        /// per second, taken off times min(|across| / 50 m, 1)
        centreline: 0.2,
        /// per second, taken off times |runway heading error| in rad
        heading: 0.2,
        /// per second, taken off times the mean squared speed of yoke and pedals (1/s)^2
        smooth: 0.02,
        /// per second, taken off always
        time: 0.05,
        /// added once on success
        success: 20.0,
        /// taken off once on a crash
        crash: 20.0,
        /// taken off once on leaving the envelope (which includes running off the runway)
        envelope: 10.0,
    }
}

params! {
    /// `landing`: starts and limits.
    pub struct LandingConfig {
        /// seconds before the episode is truncated
        time_limit: 240.0,
        /// nearest start, metres before the aiming point
        distance_min: 1500.0,
        /// farthest start, metres before the aiming point
        distance_max: 4000.0,
        /// the start is off the centreline by up to this fraction of the distance
        lateral_spread: 0.05,
        /// the start is above or below the 3 degree path by up to this fraction of its height
        height_spread: 0.25,
        /// the start is within this many degrees of the runway direction
        heading_spread: 5.0,
        /// the starting speed is 68 knots changed by up to this fraction
        speed_spread: 0.07,
        /// seconds flown hands-off before the first observation, so the instruments have settled
        settle: 0.5,
        /// success: largest sink rate at any touchdown, m/s
        max_sink: 2.5,
        /// the aircraft counts as stopped below this ground speed, m/s
        stop_speed: 1.0,
        /// envelope: farthest above the approach path, m
        max_path_error: 150.0,
        /// envelope: farthest from the centreline, m (or 0.3 of the distance to go, if that is more)
        max_across: 300.0,
        /// envelope: largest heading error from the runway direction, degrees
        max_heading_error: 90.0,
        /// envelope: largest bank, degrees
        max_roll: 70.0,
    }
}

params! {
    /// `landing`: reward weights.
    pub struct LandingRewards {
        /// in the air, per second, times exp(-|height above the path| / 10 m)
        path: 0.3,
        /// in the air, per second, times exp(-|across| / 15 m)
        centreline: 0.3,
        /// in the air, per second, times exp(-|airspeed - 65 knots| / 3 m/s)
        speed: 0.2,
        /// in the air, per 1000 m flown towards the aiming point
        progress: 5.0,
        /// per second, taken off times the mean squared speed of yoke and pedals (1/s)^2
        smooth: 0.02,
        /// added at the first touchdown on the runway, times its quality: 1 under 1 m/s of sink, 0.4 at 2.5, 0 from 3.5
        touchdown: 20.0,
        /// for slowing down on the runway after touchdown: this much per 30 m/s lost
        roll: 10.0,
        /// added once on success: stopped on the runway after touching down gently
        success: 30.0,
        /// taken off once on a crash
        crash: 30.0,
        /// taken off once on leaving the envelope, touching down off the runway or running off it
        envelope: 15.0,
    }
}

params! {
    /// `circuit`: starts and limits.
    pub struct CircuitConfig {
        /// seconds before the episode is truncated
        time_limit: 480.0,
        /// the start is within this many metres of the centreline
        lateral_spread: 4.0,
        /// the start is within this many degrees of the runway direction
        heading_spread: 3.0,
        /// success: largest sink rate at any touchdown, m/s
        max_sink: 2.5,
        /// the aircraft counts as stopped below this ground speed, m/s
        stop_speed: 1.0,
        /// envelope: greatest height above the runway, m
        max_height: 500.0,
        /// envelope: farthest from the aiming point, m
        max_distance: 6000.0,
        /// envelope: largest bank, degrees
        max_roll: 70.0,
    }
}

params! {
    /// `circuit`: reward weights.
    pub struct CircuitRewards {
        /// per second, times 0.4 exp(-|heading error| / 0.2 rad) + 0.4 exp(-|altitude error| / 20 m) + 0.2 exp(-|airspeed error| / 3 m/s) for the targets of the leg
        track: 0.2,
        /// on the upwind leg, for gaining airspeed: this much in total from standstill to 74 knots
        speed: 5.0,
        /// on the upwind leg, for gaining height: this much in total up to 150 m
        climb: 10.0,
        /// added each time a leg is completed (four times in a circuit)
        leg: 10.0,
        /// per second, taken off times the mean squared speed of yoke and pedals (1/s)^2
        smooth: 0.02,
        /// added at the first touchdown on the runway on final, times its quality (as in landing)
        touchdown: 20.0,
        /// for slowing down on the runway after that touchdown: this much per 30 m/s lost
        roll: 10.0,
        /// added once on success: stopped on the runway after a whole circuit and a gentle touchdown
        success: 50.0,
        /// taken off once on a crash
        crash: 30.0,
        /// taken off once on leaving the envelope, touching down off the runway or running off it
        envelope: 15.0,
    }
}

params! {
    /// `drive` (a car): the circuit, starts and limits.
    pub struct DriveConfig {
        /// seconds before the episode is truncated
        time_limit: 200.0,
        /// the circuit, by its place in the list of circuits (`sim_core::circuit::circuits`; 0 Lakeside Park)
        circuit: 0.0,
        /// laps to drive (from a start on the grid, the first begins at the line)
        laps: 1.0,
        /// share of the episodes that start on the grid at rest, instead of on the racing line somewhere round the lap at speed
        grid_starts: 0.3,
        /// a start on the line goes at least this share of the line's speed there
        start_speed_min: 0.5,
        /// and at most this share
        start_speed_max: 0.9,
        /// and up to this many metres either side of the line
        start_spread: 1.5,
        /// envelope: farthest beyond the asphalt's edge, m
        max_off: 15.0,
        /// envelope: seconds going the wrong way round (more than 110 degrees off the circuit's direction)
        max_reverse: 2.0,
        /// failed: seconds standing still (under 1 m/s)
        max_stopped: 8.0,
        /// share of the episodes in the rain (0: all dry; the circuit's water as long rain leaves it, `sim_core::car::wet`)
        wet_share: 0.0,
        /// the rain of a wet episode: from this (0 to 1)...
        rain_min: 0.2,
        /// ...to this
        rain_max: 0.8,
    }
}

params! {
    /// `drive`: reward weights.
    pub struct DriveRewards {
        /// for the way made round the circuit: this much per metre
        progress: 0.01,
        /// once for each lap done
        lap: 10.0,
        /// once on finishing the laps
        finish: 30.0,
        /// per second, taken off: the clock
        time: 0.05,
        /// per second, taken off while the car's middle is beyond the kerbs
        off_track: 0.5,
        /// per second, taken off times the mean squared speed of the steering and pedals (1/s)^2
        smooth: 0.005,
        /// taken off once on a crash
        crash: 20.0,
        /// taken off once on leaving the envelope (or stopping)
        envelope: 10.0,
    }
}

/// A task: its limits, its reward weights and its air.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Spec<C, R> {
    pub config: C,
    pub rewards: R,
    pub air: AirConfig,
}

params! {
    /// `race`: starts, the rules of a valid flight, and limits.
    pub struct RaceConfig {
        /// seconds before the episode is truncated
        time_limit: 480.0,
        /// the start is within this many metres of the centreline
        lateral_spread: 4.0,
        /// the start is within this many degrees of the runway direction
        heading_spread: 3.0,
        /// rule: height above the runway to reach before landing, m (the guidance's leg uses 120)
        min_height: RACE_HEIGHT,
        /// rule: the heading must at some time be this many degrees from the takeoff direction (the guidance uses 150)
        turn_back: 150.0,
        /// rule: largest sink rate at a touchdown, m/s (2.5 is about 500 ft/min)
        max_sink: 2.5,
        /// rule: largest bank at a touchdown, degrees
        max_touchdown_roll: 10.0,
        /// rule: largest angle between the nose and the runway (either way) at a touchdown, degrees
        max_touchdown_crab: 15.0,
        /// the aircraft counts as stopped below this ground speed, m/s
        stop_speed: 1.0,
        /// the flight has begun once the wheels are this high, m: a touchdown before is part of the takeoff
        airborne_height: 10.0,
        /// envelope: largest bank in the air, degrees
        max_roll: 60.0,
        /// envelope: largest pitch either way, degrees
        max_pitch: 30.0,
        /// envelope: greatest height above the runway, m
        max_height: 600.0,
        /// envelope: farthest from the middle of the runway, m
        max_distance: 5000.0,
    }
}

params! {
    /// `race`: reward weights. The return is mostly the success bonus less the time it took.
    pub struct RaceRewards {
        /// per second, taken off always (0: the clock is paid by `fast` instead, which does not make an early crash cheaper than flying on)
        time: 0.0,
        /// per second, taken off times the mean squared speed of yoke and pedals (1/s)^2
        smooth: 0.01,
        /// before the minimum height, for gaining airspeed: this much in total up to 74 knots
        speed: 5.0,
        /// before the minimum height, for gaining height: this much in total up to it
        climb: 10.0,
        /// added once on reaching the minimum height
        climbed: 5.0,
        /// added once on turning back
        turned: 10.0,
        /// after both: for coming nearer to the runway, this much per 1000 m
        approach: 5.0,
        /// added at the first touchdown, times its quality: 1 under 1 m/s of sink, 0 from 3.5
        touchdown: 20.0,
        /// for slowing down on the runway after touchdown: this much per 30 m/s lost
        roll: 10.0,
        /// added once on success: stopped on the runway after a valid flight
        success: 100.0,
        /// added on success for each second under `par`
        fast: 2.0,
        /// seconds the `fast` bonus counts from
        par: 300.0,
        /// taken off once on a crash
        crash: 50.0,
        /// taken off once on leaving the envelope, a touchdown off the runway or an invalid one
        envelope: 30.0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Task {
    /// In the air near trim at a random attitude and speed: fly to a heading, altitude and
    /// airspeed and hold them.
    HoldHeadingAltitude(Spec<HoldConfig, HoldRewards>),
    /// From standstill on the runway to 150 m above it on the runway heading.
    Takeoff(Spec<TakeoffConfig, TakeoffRewards>),
    /// From somewhere on final to a stop on the runway.
    Landing(Spec<LandingConfig, LandingRewards>),
    /// Take off, fly a left-hand circuit, land and stop.
    Circuit(Spec<CircuitConfig, CircuitRewards>),
    /// Take off, climb, turn back and land on the same runway as fast as possible.
    Race(Spec<RaceConfig, RaceRewards>),
    /// A multirotor: hover over a point at a height, facing a heading.
    Hover(Spec<HoverConfig, HoverRewards>),
    /// A multirotor: race round a course's gates.
    Gates(Spec<GatesConfig, GatesRewards>),
    /// A car: lap a circuit, fast and on the track.
    Drive(Spec<DriveConfig, DriveRewards>),
}

/// Calls the same expression on the `Spec` of whichever task it is.
macro_rules! each {
    ($task:expr, $s:ident => $body:expr) => {
        match $task {
            Task::HoldHeadingAltitude($s) => $body,
            Task::Takeoff($s) => $body,
            Task::Landing($s) => $body,
            Task::Circuit($s) => $body,
            Task::Race($s) => $body,
            Task::Hover($s) => $body,
            Task::Gates($s) => $body,
            Task::Drive($s) => $body,
        }
    };
}

impl Task {
    pub const NAMES: [&'static str; 8] = ["hold", "takeoff", "landing", "circuit", "race", "hover", "gates", "drive"];

    /// A task with its default numbers, by name.
    pub fn parse(name: &str) -> Option<Task> {
        match name {
            "hold" => Some(Task::HoldHeadingAltitude(Spec::default())),
            "takeoff" => Some(Task::Takeoff(Spec::default())),
            "landing" => Some(Task::Landing(Spec::default())),
            "circuit" => Some(Task::Circuit(Spec::default())),
            "race" => Some(Task::Race(Spec::default())),
            "hover" => Some(Task::Hover(Spec::default())),
            "gates" => Some(Task::Gates(Spec::default())),
            "drive" => Some(Task::Drive(Spec::default())),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Task::HoldHeadingAltitude(_) => "hold",
            Task::Takeoff(_) => "takeoff",
            Task::Landing(_) => "landing",
            Task::Circuit(_) => "circuit",
            Task::Race(_) => "race",
            Task::Hover(_) => "hover",
            Task::Gates(_) => "gates",
            Task::Drive(_) => "drive",
        }
    }

    pub fn needs_runway(&self) -> bool {
        !matches!(self, Task::HoldHeadingAltitude(_) | Task::Hover(_) | Task::Gates(_) | Task::Drive(_))
    }

    /// Whether it is driven in a car (`drive`).
    pub fn car(&self) -> bool {
        matches!(self, Task::Drive(_))
    }

    /// The circuit `drive` laps, as its config names it.
    pub fn circuit(&self) -> Option<&'static CircuitFile> {
        match self {
            Task::Drive(s) => sim_core::circuit::circuits().get(s.config.circuit.max(0.0) as usize),
            _ => None,
        }
    }

    /// Whether it is flown in a multirotor (`hover`, `gates`) rather than an aircraft (the rest).
    pub fn multirotor(&self) -> bool {
        matches!(self, Task::Hover(_) | Task::Gates(_))
    }

    /// The course `gates` races, as its config names it.
    pub fn course(&self) -> Option<&'static sim_core::course::Course> {
        match self {
            Task::Gates(s) => sim_core::course::file::courses().get(s.config.course.max(0.0) as usize),
            _ => None,
        }
    }

    /// Seconds before an episode is truncated.
    pub fn time_limit(&self) -> f64 {
        each!(self, s => s.config.time_limit)
    }

    pub fn air(&self) -> &AirConfig {
        each!(self, s => &s.air)
    }

    pub fn air_mut(&mut self) -> &mut AirConfig {
        each!(self, s => &mut s.air)
    }

    /// Every number of the task, in three sections: `config`, `rewards`, `air`.
    pub fn params(&self) -> [(&'static str, Vec<Param>); 3] {
        each!(self, s => [
            ("config", s.config.params()),
            ("rewards", s.rewards.params()),
            ("air", s.air.params()),
        ])
    }

    /// Sets one number by `section.name`, e.g. `rewards.crash`. False if there is no such number.
    pub fn set(&mut self, key: &str, value: f64) -> bool {
        let Some((section, name)) = key.split_once('.') else {
            return false;
        };
        each!(self, s => match section {
            "config" => s.config.set(name, value),
            "rewards" => s.rewards.set(name, value),
            "air" => s.air.set(name, value),
            _ => false,
        })
    }
}

/// Why an episode ended (or that it has not).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Running = 0,
    /// The task was completed.
    Success = 1,
    /// The aircraft is wrecked.
    Crashed = 2,
    /// It left the task's envelope (too far off, too steep, off the runway).
    OutOfEnvelope = 3,
    /// It ended the task without completing it (stopped after a hard landing).
    Failed = 4,
    /// The time limit was reached: the episode is truncated, not terminated.
    TimeLimit = 5,
}

impl Reason {
    pub const NAMES: [&'static str; 6] = [
        "running",
        "success",
        "crashed",
        "out_of_envelope",
        "failed",
        "time_limit",
    ];

    pub fn name(self) -> &'static str {
        Reason::NAMES[self as usize]
    }
}

/// Seconds a runway start stands still before the first observation: the aircraft sits down on
/// its wheels and the instruments read it.
const RUNWAY_SETTLE: f64 = 0.5;

fn uniform(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rng.uniform()
}

fn spread(rng: &mut Rng, half: f64) -> f64 {
    uniform(rng, -half, half)
}

/// The racing line of a circuit for a car, worked out once and kept (it takes some milliseconds,
/// and every episode of a drive needs it).
pub fn racing_line(circuit: &Circuit, car: &CarType) -> Arc<RacingLine> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    type Lines = HashMap<(String, String), Arc<RacingLine>>;
    static LINES: Mutex<Option<Lines>> = Mutex::new(None);
    let key = (circuit.key.clone(), car.key.clone());
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    lines.get_or_insert_with(HashMap::new).entry(key).or_insert_with(|| Arc::new(RacingLine::new(circuit, car, Pace::default()))).clone()
}

/// The racing line with its speeds for rain of `rain` (as `Driver::set_film` has them for the film
/// that rain leaves), kept for each
/// twentieth of rain (dry: the line itself).
pub fn wet_line(line: &Arc<RacingLine>, car: &CarType, rain: f64) -> Arc<RacingLine> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    if rain <= 0.0 {
        return line.clone();
    }
    type Lines = HashMap<(usize, String, i64), Arc<RacingLine>>;
    static LINES: Mutex<Option<Lines>> = Mutex::new(None);
    let step = (rain * 20.0).round() as i64;
    let key = (Arc::as_ptr(line) as usize, car.key.clone(), step);
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    lines
        .get_or_insert_with(HashMap::new)
        .entry(key)
        .or_insert_with(|| {
            let mut wet = (**line).clone();
            let film = sim_core::car::wet::film_for(step as f64 / 20.0) * 1000.0;
            wet.respeed(car, Pace::default(), &|v| sim_core::car::wet::wet_asphalt(film, v * 3.6));
            Arc::new(wet)
        })
        .clone()
}

/// `race`: the value of `Episode::leg` once the approach has begun.
const APPROACHING: u8 = 99;

/// What a task remembers during an episode.
#[derive(Clone, Debug)]
pub(crate) struct Episode {
    pub guidance: Guidance,
    pub runway: Option<Runway>,
    /// Seconds to fly hands-off before the first observation.
    pub settle: f64,
    potential: f64,
    distance: f64,
    ground_speed: f64,
    leg: u8,
    touched: bool,
    worst_sink: f64,
    started: bool,
    /// `race`: the wheels have been `airborne_height` up, the minimum height reached, turned back.
    airborne: bool,
    climbed: bool,
    turned: bool,
    /// `race`: seconds since the first observation.
    elapsed: f64,
    /// `gates`: gates flown through and missed so far.
    passed: u32,
    missed: u32,
    /// `drive`: laps done, and seconds going the wrong way round and standing still.
    laps: u32,
    reversing: f64,
    stopped: f64,
}

/// What happened during one decision interval.
pub(crate) struct Tick<'a> {
    pub obs: &'a Observation,
    pub targets: &'a Targets,
    pub before: &'a Controls,
    pub after: &'a Controls,
    pub dt: f64,
    /// Touchdowns during the interval: sink rate, and whether it was on the runway.
    pub touchdowns: &'a [(f64, bool)],
}

impl Tick<'_> {
    /// Mean squared speed of yoke and pedals (a multirotor's sticks), (1/s)^2.
    fn control_speed(&self) -> f64 {
        let d = match (self.before, self.after) {
            (Controls::Aircraft(b), Controls::Aircraft(a)) => vec![a.elevator - b.elevator, a.aileron - b.aileron, a.rudder - b.rudder],
            (Controls::Multirotor(b), Controls::Multirotor(a)) => vec![a.throttle - b.throttle, a.roll - b.roll, a.pitch - b.pitch, a.yaw - b.yaw],
            (Controls::Car(b), Controls::Car(a)) => vec![a.steer - b.steer, (a.throttle - a.brake) - (b.throttle - b.brake)],
            _ => vec![0.0],
        };
        d.iter().map(|x| (x / self.dt) * (x / self.dt)).sum::<f64>() / d.len() as f64
    }

    fn heading_error(&self) -> f64 {
        self.targets
            .heading
            .map_or(0.0, |h| remainder(h - self.obs.heading, 2.0 * PI))
    }

    fn altitude_error(&self) -> f64 {
        self.targets.altitude.map_or(0.0, |a| a - self.obs.altitude)
    }

    fn speed_error(&self) -> f64 {
        self.targets.airspeed.map_or(0.0, |v| v - indicated_airspeed(self.obs))
    }

    /// Distance from the point to hover over, m (0 when there is none).
    fn point_distance(&self) -> f64 {
        self.targets.point.map_or(0.0, |(n, e)| (n - self.obs.north).hypot(e - self.obs.east))
    }

    fn view(&self) -> RunwayView {
        self.targets.runway.unwrap_or_default()
    }
}

/// An aircraft's indicated airspeed, m/s (0 for another vehicle).
fn indicated_airspeed(obs: &Observation) -> f64 {
    obs.aircraft().map_or(0.0, |a| a.indicated_airspeed)
}

/// The aircraft the aircraft's tasks fly: `Env` builds the vehicle the task asks for.
fn aircraft(world: &mut World, id: VehicleId) -> &mut Aircraft {
    world.vehicle_mut(id).aircraft_mut().expect("an aircraft's task flies an aircraft")
}

/// Wind for an episode. On runway tasks it comes from roughly ahead of the runway in use.
fn draw_weather(air: &AirConfig, rng: &mut Rng, runway_heading: Option<f64>) -> Weather {
    let wind_speed = uniform(rng, air.wind_min, air.wind_max.max(air.wind_min)).max(0.0);
    let turbulence = uniform(
        rng,
        air.turbulence_min,
        air.turbulence_max.max(air.turbulence_min),
    )
    .max(0.0);
    let any = uniform(rng, 0.0, 360.0);
    let off = spread(rng, air.crosswind_angle.clamp(0.0, 180.0));
    let wind_from = match runway_heading {
        Some(h) => (h / DEG + off).rem_euclid(360.0),
        None => any,
    };
    Weather {
        wind_from,
        wind_speed,
        turbulence,
        ..Weather::default()
    }
}

/// Lets the aircraft keep its airspeed in a wind: the air mass moves, so its speed over the ground
/// changes by the wind.
fn add_wind(world: &mut World, id: VehicleId) {
    let wind = world.weather.mean_wind(world.vehicle(id).agl());
    let s = &mut aircraft(world, id).state;
    s.vel += act(inverse(s.q), wind);
}

fn on_runway_start(
    world: &mut World,
    id: VehicleId,
    rng: &mut Rng,
    airport: Airport,
    air: &AirConfig,
    lateral_spread: f64,
    heading_spread: f64,
) -> Runway {
    let runway = Runway {
        airport,
        reversed: rng.uniform() < 0.5,
    };
    world.weather = draw_weather(air, rng, Some(runway.heading()));
    world.reset(
        id,
        Start::Runway {
            airport,
            reversed: runway.reversed,
        },
    );
    let across = spread(rng, lateral_spread);
    let heading = runway.heading() + spread(rng, heading_spread) * DEG;
    let h = runway.heading();
    let s = &mut aircraft(world, id).state;
    s.pos += V3::new(-h.sin(), h.cos(), 0.0) * across;
    s.q = quaternion(heading, 0.0, 0.0);
    runway
}

impl Episode {
    pub(crate) fn new(guidance: Guidance, settle: f64) -> Episode {
        Episode {
            runway: guidance.runway(),
            guidance,
            settle,
            potential: 0.0,
            distance: 0.0,
            ground_speed: 0.0,
            leg: leg::NONE,
            touched: false,
            worst_sink: 0.0,
            started: false,
            airborne: false,
            climbed: false,
            turned: false,
            elapsed: 0.0,
            passed: 0,
            missed: 0,
            laps: 0,
            reversing: 0.0,
            stopped: 0.0,
        }
    }

    /// Sets the weather and places the aircraft for a new episode. `home` is the airport the
    /// runway tasks use; `hold` starts around it (around the origin without one).
    pub fn start(
        task: &Task,
        world: &mut World,
        id: VehicleId,
        rng: &mut Rng,
        home: Option<Airport>,
    ) -> Episode {
        let runway_home = || home.expect("this world has no airport for a runway task");
        match task {
            Task::HoldHeadingAltitude(s) => {
                let c = &s.config;
                world.weather = draw_weather(&s.air, rng, None);
                let (hn, he) = home.map_or((0.0, 0.0), |a| (a.n, a.e));
                let n = hn + spread(rng, c.position_spread);
                let e = he + spread(rng, c.position_spread);
                let heading = uniform(rng, 0.0, 2.0 * PI);
                world.reset(id, Start::Air { n, e, heading });
                let plane = aircraft(world, id);
                let altitude = -plane.state.pos.z + spread(rng, c.altitude_spread);
                let tas = uniform(rng, c.speed_min, c.speed_max);
                let roll = spread(rng, c.roll_spread) * DEG;
                let pitch = spread(rng, c.pitch_spread) * DEG;
                let rates = V3::new(
                    spread(rng, c.rate_spread),
                    spread(rng, c.rate_spread),
                    spread(rng, c.rate_spread),
                ) * DEG;
                if let Some(t) = plane.trim_level(tas, altitude) {
                    let p = &plane.p;
                    let travel = if t.elevator >= 0.0 {
                        p.elevator_up
                    } else {
                        p.elevator_down
                    };
                    plane.controls.throttle = t.throttle;
                    plane.controls.trim = (t.elevator * travel / p.trim_range).clamp(-1.0, 1.0);
                    plane.state.pos.z = -altitude;
                    plane.state.vel = V3::new(tas * t.alpha.cos(), 0.0, tas * t.alpha.sin());
                    plane.state.q = quaternion(heading, t.alpha + pitch, roll);
                    plane.state.omega = rates;
                    plane.state.prop_speed = t.prop_speed;
                }
                // Moved by hand from where the reset put it: the instruments start from here.
                let state = aircraft(world, id).state;
                world.place(id, state);
                add_wind(world, id);
                let target_altitude = altitude + spread(rng, c.altitude_change);
                let density = atmosphere::density(target_altitude) / atmosphere::SEA_LEVEL_DENSITY;
                Episode::new(
                    Guidance::hold(
                        (heading + spread(rng, c.heading_change) * DEG).rem_euclid(2.0 * PI),
                        target_altitude,
                        uniform(rng, c.speed_min, c.speed_max) * density.sqrt(),
                    ),
                    c.settle,
                )
            }
            Task::Takeoff(s) => {
                let c = &s.config;
                let runway = on_runway_start(
                    world,
                    id,
                    rng,
                    runway_home(),
                    &s.air,
                    c.lateral_spread,
                    c.heading_spread,
                );
                Episode::new(Guidance::takeoff(runway), RUNWAY_SETTLE)
            }
            Task::Circuit(s) => {
                let c = &s.config;
                let runway = on_runway_start(
                    world,
                    id,
                    rng,
                    runway_home(),
                    &s.air,
                    c.lateral_spread,
                    c.heading_spread,
                );
                Episode::new(Guidance::circuit(runway), RUNWAY_SETTLE)
            }
            Task::Race(s) => {
                let c = &s.config;
                let runway = on_runway_start(
                    world,
                    id,
                    rng,
                    runway_home(),
                    &s.air,
                    c.lateral_spread,
                    c.heading_spread,
                );
                Episode::new(Guidance::race(runway), RUNWAY_SETTLE)
            }
            Task::Landing(s) => {
                let c = &s.config;
                let airport = runway_home();
                let runway = Runway {
                    airport,
                    reversed: rng.uniform() < 0.5,
                };
                world.weather = draw_weather(&s.air, rng, Some(runway.heading()));
                let distance = uniform(rng, c.distance_min, c.distance_max.max(c.distance_min));
                world.reset(
                    id,
                    Start::Final {
                        airport,
                        reversed: runway.reversed,
                        distance,
                    },
                );
                let h = runway.heading();
                let across = spread(rng, c.lateral_spread) * distance;
                let up = spread(rng, c.height_spread) * runway.path_height(AIMING_POINT - distance);
                let turn = spread(rng, c.heading_spread) * DEG;
                let faster = 1.0 + spread(rng, c.speed_spread);
                let state = &mut aircraft(world, id).state;
                state.pos += V3::new(-h.sin() * across, h.cos() * across, -up);
                let e = euler_angles(state.q);
                state.q = quaternion(e.heading + turn, e.pitch, e.roll);
                state.vel *= faster;
                add_wind(world, id);
                Episode::new(Guidance::landing(runway), c.settle)
            }
            Task::Hover(s) => {
                let c = &s.config;
                world.weather = draw_weather(&s.air, rng, None);
                let (hn, he) = home.map_or((0.0, 0.0), |a| (a.n, a.e));
                let n = hn + spread(rng, c.position_spread);
                let e = he + spread(rng, c.position_spread);
                let heading = uniform(rng, 0.0, 2.0 * PI);
                // It hovers, armed, where a start in the air puts it; then a push and a turn.
                world.reset(id, Start::Air { n, e, heading });
                let body = world.vehicle(id).body();
                let push = V3::new(spread(rng, c.speed_spread), spread(rng, c.speed_spread), spread(rng, c.speed_spread));
                let rates = V3::new(spread(rng, c.rate_spread), spread(rng, c.rate_spread), spread(rng, c.rate_spread)) * DEG;
                let state = State { pos: body.pos, q: body.q, vel: act(inverse(body.q), push), omega: rates, ..State::default() };
                world.place(id, state);
                let (pn, pe) = (n + spread(rng, c.point_spread), e + spread(rng, c.point_spread));
                let ground = world.terrain.height(pn, pe).max(0.0);
                let altitude = ground + c.height + spread(rng, c.height_spread);
                let face = (heading + spread(rng, c.heading_change) * DEG).rem_euclid(2.0 * PI);
                Episode::new(Guidance::hover(pn, pe, altitude, face), c.settle)
            }
            Task::Gates(s) => {
                let c = &s.config;
                world.weather = draw_weather(&s.air, rng, None);
                let mut course = task.course().expect("the course of the config exists (`EnvConfig::validate`)").clone();
                if c.laps >= 1.0 {
                    course.laps = c.laps as u32;
                }
                let track = Arc::new(Track::new(&course, &world.terrain));
                world.obstacles = Arc::new(track.obstacles());
                let n = track.gates.len();
                let next = if rng.uniform() < c.gate_starts { 1 + (rng.uniform() * (n - 1) as f64) as usize % (n - 1) } else { 0 };
                if next == 0 {
                    // On the start's pad, armed, the throttle down.
                    let (terrain, weather, earth) = (world.terrain.clone(), world.weather, world.earth);
                    // Off the pad's middle, across the start, as a quad on another pad of the start.
                    // No draw without a spread: the episodes of a seed stay what they were.
                    let across = c.pad_offset + if c.pad_spread > 0.0 { spread(rng, c.pad_spread) } else { 0.0 };
                    let (n, e) = (track.start.x - track.start_heading.sin() * across, track.start.y + track.start_heading.cos() * across);
                    world.vehicles[id.0].reset_on_ground(&terrain, &weather, &earth, n, e, track.start_heading);
                    if let Some(m) = world.vehicle_mut(id).multirotor_mut() {
                        m.controls = MultirotorControls { arm: true, ..MultirotorControls::default() };
                    }
                } else {
                    // In flight on the line through a gate, some metres before it, flying at it.
                    let (middle, through) = track.target(next);
                    let at = middle - through * c.gate_lead;
                    let level = V3::new(through.x, through.y, 0.0);
                    let heading = if level.length() > 0.1 { level.y.atan2(level.x) } else { track.gates[next].heading };
                    world.reset(id, Start::Air { n: at.x, e: at.y, heading });
                    let q = quaternion(heading, 0.0, 0.0);
                    let state = State { pos: at, q, vel: act(inverse(q), through * c.gate_speed), ..State::default() };
                    world.place(id, state);
                }
                let pos = world.vehicle(id).body().pos;
                Episode::new(Guidance::gates(track, world.time(), pos, next), 0.0)
            }
            Task::Drive(s) => {
                let c = &s.config;
                world.weather = draw_weather(&s.air, rng, None);
                let circuit = world.terrain.circuits().first().cloned().expect("a drive's world has its circuit (`EnvConfig::terrain`)");
                let car = world.vehicle(id).car().expect("a drive drives a car").p.clone();
                // Rain on some episodes (no draw without it: the episodes of a seed stay what they were).
                world.weather.rain = if c.wet_share > 0.0 && rng.uniform() < c.wet_share { uniform(rng, c.rain_min, c.rain_max.max(c.rain_min)).clamp(0.0, 1.0) } else { 0.0 };
                world.settle_water();
                let line = wet_line(&racing_line(&circuit, &car), &car, world.weather.rain);
                if rng.uniform() < c.grid_starts {
                    let (n, e, heading) = circuit.grid(0);
                    world.stand(id, n, e, heading);
                } else {
                    // On the racing line somewhere round the lap, a little off it, at speed.
                    let along = rng.uniform() * circuit.length;
                    // Off the line by the spread, but on the asphalt.
                    let on_line = line.across[(along / sim_core::circuit::STEP) as usize % line.across.len()];
                    let room = circuit.sample(along).width / 2.0 - 1.0;
                    let across = (on_line + spread(rng, c.start_spread)).clamp(-room, room) - on_line;
                    let ((n, e), speed) = line.at(&circuit, along);
                    let ((n2, e2), _) = line.at(&circuit, along + 2.0);
                    let heading = (e2 - e).atan2(n2 - n);
                    let (n, e) = (n - heading.sin() * across, e + heading.cos() * across);
                    world.stand(id, n, e, heading);
                    let speed = speed * uniform(rng, c.start_speed_min, c.start_speed_max.max(c.start_speed_min));
                    if let Some(car) = world.vehicle_mut(id).car_mut() {
                        let gear = (1..=car.p.gearbox.ratios.len() as i32).find(|&g| car.p.top_of_gear(g) > speed * 1.15).unwrap_or(car.p.gearbox.ratios.len() as i32);
                        car.set_gear(gear);
                        let b = car.body;
                        car.put(Body { vel: V3::new(speed, 0.0, 0.0), ..b });
                    }
                }
                let pos = world.vehicle(id).body().pos;
                let along = circuit.locate(pos.x, pos.y).map_or(0.0, |a| a.along);
                Episode::new(Guidance::drive(circuit, line, along), 0.0)
            }
        }
    }

    fn takeoff_potential(speed: f64, climb: f64, height_goal: f64, tick: &Tick) -> f64 {
        let v = tick.view();
        speed * indicated_airspeed(tick.obs).clamp(0.0, CLIMB_SPEED) / CLIMB_SPEED
            + climb * v.height.clamp(0.0, height_goal) / height_goal
    }

    /// Call with the first observation of the episode, before any `judge`.
    pub fn begin(&mut self, task: &Task, tick: &Tick) {
        self.started = true;
        self.leg = tick.targets.leg;
        self.distance = tick.view().distance;
        self.ground_speed = tick.obs.speed;
        self.potential = match task {
            Task::Takeoff(s) => {
                Episode::takeoff_potential(s.rewards.speed, s.rewards.climb, s.config.height, tick)
            }
            Task::Circuit(s) => {
                Episode::takeoff_potential(s.rewards.speed, s.rewards.climb, TAKEOFF_HEIGHT, tick)
            }
            Task::Race(s) => {
                Episode::takeoff_potential(s.rewards.speed, s.rewards.climb, s.config.min_height, tick)
            }
            Task::Gates(_) => {
                let p = tick.targets.race.unwrap_or_default();
                (self.passed, self.missed) = (p.passed, p.missed);
                p.progress
            }
            Task::Drive(_) => tick.targets.drive.map_or(0.0, |d| d.progress),
            _ => 0.0,
        };
    }

    /// The landing part of `landing` and `circuit`: touchdown, roll-out and stop.
    /// Returns the reward (before the weights of crash and envelope) and how it ended, if it did.
    #[allow(clippy::too_many_arguments)]
    fn land(
        &mut self,
        tick: &Tick,
        touchdown: f64,
        roll: f64,
        success: f64,
        max_sink: f64,
        stop_speed: f64,
    ) -> (f64, Reason) {
        let mut r = 0.0;
        for &(sink, on_runway) in tick.touchdowns {
            if !on_runway {
                return (r, Reason::OutOfEnvelope);
            }
            if !self.touched {
                self.touched = true;
                r += touchdown * ((3.5 - sink) / 2.5).clamp(0.0, 1.0);
            }
            self.worst_sink = self.worst_sink.max(sink);
        }
        if self.touched && tick.obs.on_ground {
            let on_runway = self
                .runway
                .is_some_and(|rw| rw.on_runway(tick.obs.north, tick.obs.east));
            if !on_runway {
                return (r, Reason::OutOfEnvelope);
            }
            r += roll * (self.ground_speed - tick.obs.speed) / 30.0;
            if tick.obs.speed < stop_speed {
                return if self.worst_sink < max_sink {
                    (r + success, Reason::Success)
                } else {
                    (r, Reason::Failed)
                };
            }
        }
        (r, Reason::Running)
    }

    /// Reward for one decision interval and whether the episode ends with it.
    pub fn judge(&mut self, task: &Task, tick: &Tick) -> (f64, Reason) {
        debug_assert!(self.started);
        let obs = tick.obs;
        let dt = tick.dt;
        let crashed = obs.crashed.is_some();
        let v = tick.view();
        let (reward, reason) = match task {
            Task::HoldHeadingAltitude(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let rate2: f64 = obs.angular_rates.iter().map(|x| x * x).sum();
                let mut r = dt
                    * (w.heading * (-tick.heading_error().abs() / 0.15).exp()
                        + w.altitude * (-tick.altitude_error().abs() / 20.0).exp()
                        + w.speed * (-tick.speed_error().abs() / 3.0).exp()
                        - w.rates * rate2.min(5.0)
                        - w.slip * (obs.aircraft().map_or(0.0, |a| a.beta) / 0.1).powi(2).min(5.0)
                        - w.smooth * tick.control_speed());
                let out = tick.altitude_error().abs() > c.max_altitude_error
                    || obs.agl < c.min_agl
                    || obs.roll.abs() > c.max_roll * DEG
                    || obs.pitch.abs() > c.max_pitch * DEG;
                let reason = if crashed {
                    r -= w.crash;
                    Reason::Crashed
                } else if out {
                    r -= w.envelope;
                    Reason::OutOfEnvelope
                } else {
                    Reason::Running
                };
                (r, reason)
            }
            Task::Takeoff(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let potential = Episode::takeoff_potential(w.speed, w.climb, c.height, tick);
                let mut r = potential
                    - self.potential
                    - dt * (w.centreline * (v.across.abs() / 50.0).min(1.0)
                        + w.heading * v.heading_error.abs()
                        + w.smooth * tick.control_speed()
                        + w.time);
                self.potential = potential;
                let off_runway = obs.on_ground
                    && !self
                        .runway
                        .is_some_and(|rw| rw.on_runway(obs.north, obs.east));
                let out = off_runway
                    || v.across.abs() > c.max_across
                    || v.heading_error.abs() > c.max_heading_error * DEG
                    || obs.roll.abs() > c.max_roll * DEG;
                let done = v.height >= c.height
                    && v.heading_error.abs() <= c.success_heading * DEG
                    && v.across.abs() <= c.success_across;
                let reason = if crashed {
                    r -= w.crash;
                    Reason::Crashed
                } else if out {
                    r -= w.envelope;
                    Reason::OutOfEnvelope
                } else if done {
                    r += w.success;
                    Reason::Success
                } else {
                    Reason::Running
                };
                (r, reason)
            }
            Task::Landing(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let mut r = -dt * w.smooth * tick.control_speed();
                if !self.touched {
                    r += dt
                        * (w.path * (-v.path_error.abs() / 10.0).exp()
                            + w.centreline * (-v.across.abs() / 15.0).exp()
                            + w.speed
                                * (-(indicated_airspeed(obs) - APPROACH_SPEED).abs() / 3.0).exp())
                        + w.progress * (self.distance - v.distance) / 1000.0;
                }
                self.distance = v.distance;
                let (landed, mut reason) = self.land(
                    tick,
                    w.touchdown,
                    w.roll,
                    w.success,
                    c.max_sink,
                    c.stop_speed,
                );
                self.ground_speed = obs.speed;
                r += landed;
                let out = v.path_error > c.max_path_error
                    || v.across.abs() > c.max_across.max(0.3 * -v.along)
                    || v.heading_error.abs() > c.max_heading_error * DEG
                    || obs.roll.abs() > c.max_roll * DEG
                    // Still flying past the far end of the runway.
                    || (!obs.on_ground && v.along > self.runway.map_or(0.0, |rw| rw.airport.length));
                if crashed {
                    reason = Reason::Crashed;
                } else if reason == Reason::Running && out {
                    reason = Reason::OutOfEnvelope;
                }
                match reason {
                    Reason::Crashed => r -= w.crash,
                    Reason::OutOfEnvelope => r -= w.envelope,
                    _ => {}
                }
                (r, reason)
            }
            Task::Circuit(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let mut r = dt
                    * (w.track
                        * (0.4 * (-tick.heading_error().abs() / 0.2).exp()
                            + 0.4 * (-tick.altitude_error().abs() / 20.0).exp()
                            + 0.2 * (-tick.speed_error().abs() / 3.0).exp())
                        - w.smooth * tick.control_speed());
                let now = tick.targets.leg;
                if self.leg == leg::UPWIND {
                    let potential =
                        Episode::takeoff_potential(w.speed, w.climb, TAKEOFF_HEIGHT, tick);
                    r += potential - self.potential;
                    self.potential = potential;
                }
                if now > self.leg {
                    r += w.leg * f64::from(now - self.leg);
                    self.leg = now;
                }
                let mut reason = Reason::Running;
                if now == leg::FINAL {
                    let (landed, how) = self.land(
                        tick,
                        w.touchdown,
                        w.roll,
                        w.success,
                        c.max_sink,
                        c.stop_speed,
                    );
                    r += landed;
                    reason = how;
                }
                self.ground_speed = obs.speed;
                let off_runway = obs.on_ground
                    && !self
                        .runway
                        .is_some_and(|rw| rw.on_runway(obs.north, obs.east));
                let out = off_runway
                    || v.height > c.max_height
                    || v.distance > c.max_distance
                    || obs.roll.abs() > c.max_roll * DEG
                    // Down again before flying the circuit.
                    || (obs.on_ground && now > leg::UPWIND && now < leg::FINAL);
                if crashed {
                    reason = Reason::Crashed;
                } else if reason == Reason::Running && out {
                    reason = Reason::OutOfEnvelope;
                }
                match reason {
                    Reason::Crashed => r -= w.crash,
                    Reason::OutOfEnvelope => r -= w.envelope,
                    _ => {}
                }
                (r, reason)
            }
            Task::Race(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let mut r = -dt * (w.time + w.smooth * tick.control_speed());
                self.elapsed += dt;
                if !self.climbed {
                    let potential = Episode::takeoff_potential(w.speed, w.climb, c.min_height, tick);
                    r += potential - self.potential;
                    self.potential = potential;
                }
                if !obs.on_ground && v.height >= c.airborne_height {
                    self.airborne = true;
                }
                if !self.climbed && !obs.on_ground && v.height >= c.min_height {
                    self.climbed = true;
                    r += w.climbed;
                }
                if !self.turned && self.airborne && v.heading_error.abs() >= c.turn_back * DEG {
                    self.turned = true;
                    r += w.turned;
                }
                // Distance to the nearest point of the runway.
                let length = self.runway.map_or(0.0, |rw| rw.airport.length);
                let beyond = (-v.along).max(v.along - length).max(0.0);
                let to_runway = beyond.hypot(v.across);
                // `leg` marks that the approach has begun and `distance` holds its last value.
                if self.climbed && self.turned && !self.touched {
                    if self.leg == APPROACHING {
                        r += w.approach * (self.distance - to_runway) / 1000.0;
                    }
                    self.leg = APPROACHING;
                    self.distance = to_runway;
                }
                let mut reason = Reason::Running;
                for &(sink, on_runway) in tick.touchdowns {
                    if !self.airborne {
                        continue;
                    }
                    let crab = remainder(v.heading_error, PI).abs();
                    if !on_runway {
                        reason = Reason::OutOfEnvelope;
                    } else if !(self.climbed && self.turned)
                        || sink >= c.max_sink
                        || obs.roll.abs() > c.max_touchdown_roll * DEG
                        || crab > c.max_touchdown_crab * DEG
                    {
                        reason = Reason::Failed;
                    } else if !self.touched {
                        self.touched = true;
                        r += w.touchdown * ((3.5 - sink) / 2.5).clamp(0.0, 1.0);
                    }
                }
                let off_runway = obs.on_ground
                    && !self
                        .runway
                        .is_some_and(|rw| rw.on_runway(obs.north, obs.east));
                if reason == Reason::Running && self.touched && obs.on_ground && !off_runway {
                    r += w.roll * (self.ground_speed - obs.speed).max(0.0) / 30.0;
                    if obs.speed < c.stop_speed {
                        r += w.success + w.fast * (w.par - self.elapsed).max(0.0);
                        reason = Reason::Success;
                    }
                }
                self.ground_speed = obs.speed;
                let centre = (v.along - length / 2.0).hypot(v.across);
                let out = off_runway
                    || (!obs.on_ground
                        && (obs.roll.abs() > c.max_roll * DEG
                            || obs.pitch.abs() > c.max_pitch * DEG))
                    || v.height > c.max_height
                    || centre > c.max_distance;
                if crashed {
                    reason = Reason::Crashed;
                } else if reason == Reason::Running && out {
                    reason = Reason::OutOfEnvelope;
                }
                match reason {
                    Reason::Crashed => r -= w.crash,
                    Reason::OutOfEnvelope | Reason::Failed => r -= w.envelope,
                    _ => {}
                }
                (r, reason)
            }
            Task::Gates(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let p = tick.targets.race.unwrap_or_default();
                let rate2: f64 = obs.angular_rates.iter().map(|x| x * x).sum();
                let mut r = w.progress * (p.progress - self.potential)
                    + w.gate * f64::from(p.passed.saturating_sub(self.passed))
                    - w.missed * f64::from(p.missed.saturating_sub(self.missed))
                    - dt * (w.time + w.rates * rate2.min(20.0) + w.smooth * tick.control_speed());
                (self.potential, self.passed, self.missed) = (p.progress, p.passed, p.missed);
                let distance = tick.targets.gates[0].map_or(0.0, |g| (V3::from_array(g.middle) - V3::new(obs.north, obs.east, -obs.altitude)).length());
                let tilt = (obs.pitch.cos() * obs.roll.cos()).clamp(-1.0, 1.0).acos();
                let reason = if crashed {
                    r -= w.crash;
                    Reason::Crashed
                } else if p.finished {
                    r += w.finish;
                    Reason::Success
                } else if distance > c.max_distance || tilt > c.max_tilt * DEG {
                    r -= w.envelope;
                    Reason::OutOfEnvelope
                } else {
                    Reason::Running
                };
                (r, reason)
            }
            Task::Drive(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let d = tick.targets.drive.unwrap_or_default();
                let mut r = w.progress * (d.progress - self.potential) - dt * (w.time + w.smooth * tick.control_speed());
                self.potential = d.progress;
                let off = d.across.abs() - d.half_width;
                if off > sim_core::circuit::KERB_WIDTH {
                    r -= dt * w.off_track;
                }
                let laps = (d.progress / d.length.max(1.0)).floor().max(0.0) as u32;
                if laps > self.laps {
                    r += w.lap * f64::from(laps - self.laps);
                    self.laps = laps;
                }
                let speed = obs.car().map_or(obs.speed, |car| car.speed);
                self.reversing = if d.heading_error.abs() > 110.0 * DEG && speed > 2.0 { self.reversing + dt } else { 0.0 };
                self.stopped = if speed.abs() < 1.0 { self.stopped + dt } else { 0.0 };
                let reason = if crashed {
                    r -= w.crash;
                    Reason::Crashed
                } else if self.laps as f64 >= c.laps.max(1.0) {
                    r += w.finish;
                    Reason::Success
                } else if off > c.max_off || self.reversing > c.max_reverse {
                    r -= w.envelope;
                    Reason::OutOfEnvelope
                } else if self.stopped > c.max_stopped {
                    r -= w.envelope;
                    Reason::Failed
                } else {
                    Reason::Running
                };
                (r, reason)
            }
            Task::Hover(s) => {
                let (c, w) = (&s.config, &s.rewards);
                let rate2: f64 = obs.angular_rates.iter().map(|x| x * x).sum();
                let distance = tick.point_distance();
                let mut r = dt
                    * (w.position * (-distance / 2.0).exp()
                        + w.height * (-tick.altitude_error().abs() / 1.0).exp()
                        + w.heading * (-tick.heading_error().abs() / 0.3).exp()
                        - w.rates * rate2.min(20.0)
                        - w.smooth * tick.control_speed());
                let tilt = (obs.pitch.cos() * obs.roll.cos()).clamp(-1.0, 1.0).acos();
                let out = distance > c.max_distance || tick.altitude_error().abs() > c.max_height_error || tilt > c.max_tilt * DEG;
                let reason = if crashed {
                    r -= w.crash;
                    Reason::Crashed
                } else if out {
                    r -= w.envelope;
                    Reason::OutOfEnvelope
                } else {
                    Reason::Running
                };
                (r, reason)
            }
        };
        (reward, reason)
    }
}
