//! Flight simulation: aircraft, multirotors, terrain, air and time. No graphics and no operating system.
//!
//! World axes are north, east, down in metres (`f64`); body axes are x forward, y right, z down.
//! See `README.md` for the contract of each module.

pub mod aircraft;
pub mod atmosphere;
pub mod autopilot;
pub mod body;
pub mod bridges;
pub mod car;
pub mod circuit;
pub mod course;
pub mod gfc700;
pub mod instruments;
pub mod lights;
pub mod math;
pub mod multirotor;
pub mod pilot;
pub mod region;
pub mod roofs;
pub mod scenery;
pub mod solids;
pub mod terrain;
pub mod vehicle;
pub mod wind;
pub mod world;
pub mod wreck;

pub use aircraft::{
    Aircraft, AircraftControls, AircraftKind, AircraftObservation, AircraftType, Brake, Compressibility, Damage, Environment, FlightData,
    Governor, GroundKind, Intake, Jets, Load, Look, Retract, Speeds, Spoilers, Stall, State, SurfaceState, Touch, Trim, Visual, Wheel,
    WingStrip, COLLISION_BREAKUP, MOST_ENGINES, MOST_TOUCHES, MOST_WHEELS, TAIL_STRIKE,
};
pub use atmosphere::Atmosphere;
pub use instruments::{AirData, AirspeedMarks, Earth, Failures, GyroDrive, InstrumentFit, Instruments, PitotBlock, Readings, Sense, StaticPorts, HPA, INHG};
pub use body::{Body, Rates};
pub use autopilot::{Autopilot, Gains, Goal, Pattern, Phase, Vertical, Waypoint};
pub use gfc700::{Alert, AltitudeAlert, Approach, Disconnect, FlightPlan, Gfc700, Gfc700Fit, Key as AfcsKey, Knob as AfcsKnob, LateralMode, Leg, VerticalMode};
pub use math::{
    act, axis_angle, clamp, euler_angles, fbm, hash2, interpolate, inverse, lerp, noised,
    normalize, quaternion, remainder, smoothstep, Euler, Noise, Quat, Rng, DEG, FEET, GRAVITY,
    KNOTS, V3,
};
pub use pilot::{KeyboardPilot, Pilot, ASSIST_BANK, ASSIST_INSIST, ASSIST_LOAD, ASSIST_PITCH};
pub use region::{Bridge, Carries, Cover, Footprint, Grid, Imagery, Material, Region, Roof, Structure, Use};
pub use terrain::{Airport, Cell, Detail, Ground, Lake, Raw, Road, Surface, Terrain, Town};
pub use wind::{Weather, WindField};
pub use wreck::{fuel_released, Cut, Fire, Fragment, Piece, PieceKind, Pieces, Portion, Scar, Slice, Span, Splash, Wreck, IGNITION_SPEED, TANKS_BURST};
pub use multirotor::{
    Battery, Multirotor, MultirotorControls, MultirotorData, MultirotorKind, MultirotorLook, MultirotorObservation,
    MultirotorType, Spin, StickRates, Tilt, MOST_MOTORS, NACELLES,
};
pub use car::{Car, CarControls, CarData, CarKind, CarLook, CarObservation, CarType, WheelState};
pub use circuit::Circuit;
pub use vehicle::{Controls, Dynamics, Vehicle, VehicleId};
pub use world::{AirportObservation, Observation, Start, Traffic, VehicleObservation, World, GROUND_AHEAD};
