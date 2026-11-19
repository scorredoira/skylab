//! Types of aircraft as data: a file `assets/aircraft/<key>/aircraft.ron` read into an
//! `AircraftType` and checked, and the fleet, the types there are.
//!
//! The file's sections and units are `crates/sim/sim-core/README.md`'s "Aircraft files"; the C172's
//! and the Extra's are the examples. Nothing here reads the disk: the game reads the files and hands
//! their text over (`fleet_with`, `set_fleet`); without that, the fleet is the files built into the
//! program (`BUILT_IN`), so the tests and the web need no files.
//!
//! A type read from a file lives as long as the program: its tables, strings and pieces are kept
//! (`&'static`) once per file read, and the files are read once, at start.

use std::f64::consts::PI;
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;

use super::jet::{Mount, Reverser};
use super::{AircraftKind, AircraftType, Brake, Compressibility, Governor, Jets, Load, Retract, Speeds, Spoilers, Visual, Wheel, MOST_ENGINES, MOST_WHEELS};
use crate::gfc700::Gfc700Fit;
use crate::instruments::{AirspeedMarks, GyroDrive, InstrumentFit, StaticPorts};
use crate::math::{DEG, KNOTS, V3};
use crate::wreck::{Pieces, Span};

/// The files of `assets/aircraft` built into the program: (where it is in the repository, its text).
pub const BUILT_IN: [(&str, &str); 2] = [
    ("assets/aircraft/c172/aircraft.ron", include_str!("../../../../../assets/aircraft/c172/aircraft.ron")),
    ("assets/aircraft/extra300/aircraft.ron", include_str!("../../../../../assets/aircraft/extra300/aircraft.ron")),
];

/// What is wrong with a file: which file, and what, with the line and column or the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileError {
    pub file: String,
    pub message: String,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file, self.message)
    }
}

impl std::error::Error for FileError {}

/// Reads a type of aircraft from the text of its file; `file` names it in the errors.
pub fn load(text: &str, file: &str) -> Result<AircraftType, FileError> {
    let error = |message: String| FileError { file: file.to_string(), message };
    let parsed: File = ron::from_str(text).map_err(|e| error(e.to_string()))?;
    parsed.check().map_err(error)?;
    Ok(parsed.build())
}

static FLEET: OnceLock<Vec<AircraftType>> = OnceLock::new();

/// The types of aircraft there are: those given to `set_fleet`, or else the built-in ones.
pub fn fleet() -> &'static [AircraftType] {
    FLEET.get_or_init(|| BUILT_IN.iter().map(|(file, text)| load(text, file).unwrap_or_else(|e| panic!("{e}"))).collect())
}

/// Makes `types` the fleet, before anything has asked for it (the game does it at start). They must
/// have different keys and names for the command line, and the C172 among them: it is the default.
pub fn set_fleet(types: Vec<AircraftType>) -> Result<(), String> {
    for (i, t) in types.iter().enumerate() {
        for other in &types[..i] {
            if let Some(word) = t.words().find(|w| other.words().any(|o| o == *w)) {
                return Err(format!("the aircraft {} and {} are both called \"{word}\"", other.kind.key(), t.kind.key()));
            }
        }
    }
    if !types.iter().any(|t| t.kind == AircraftKind::C172) {
        return Err("there is no aircraft c172, the default one".to_string());
    }
    FLEET.set(types).map_err(|_| "the aircraft were already in use when they were read".to_string())
}

/// The built-in types with these files' (each a name for its errors and its text) in their place:
/// a file of a built-in type's key replaces it, any other is added after them, in the order given.
pub fn fleet_with(files: &[(String, String)]) -> Result<Vec<AircraftType>, FileError> {
    let mut types: Vec<AircraftType> = Vec::new();
    for (file, text) in BUILT_IN.iter().map(|(f, t)| (*f, *t)).chain(files.iter().map(|(f, t)| (f.as_str(), t.as_str()))) {
        let t = load(text, file)?;
        match types.iter_mut().find(|old| old.kind == t.kind) {
            Some(old) => *old = t,
            None => types.push(t),
        }
    }
    Ok(types)
}

impl AircraftType {
    /// The words that name it: its key and its aliases.
    fn words(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.kind.key()).chain(self.aliases.iter().copied())
    }
}

/// Keeps a list for the rest of the program (see the module's documentation).
fn keep<T>(items: Vec<T>) -> &'static [T] {
    Box::leak(items.into_boxed_slice())
}

fn keep_str(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

type Vector = (f64, f64, f64);

fn v((x, y, z): Vector) -> V3 {
    V3::new(x, y, z)
}

// The file, section by section. Field names carry their unit when it is not SI.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    key: String,
    name: String,
    aliases: Vec<String>,
    visual: VisualFile,
    mass: MassFile,
    wing: WingFile,
    flaps: FlapsFile,
    tail: TailFile,
    fin: FinFile,
    body: BodyFile,
    /// The piston engine and its propeller, or else `jets`.
    #[serde(default, deserialize_with = "some")]
    propeller: Option<PropellerFile>,
    #[serde(default, deserialize_with = "some")]
    engine: Option<EngineFile>,
    #[serde(default, deserialize_with = "some")]
    jets: Option<JetsFile>,
    #[serde(default, deserialize_with = "some")]
    mach: Option<MachFile>,
    #[serde(default, deserialize_with = "some")]
    spoilers: Option<SpoilersFile>,
    gear: GearFile,
    hard_points: HardPointsFile,
    structure: StructureFile,
    floats: FloatsFile,
    airframe: AirframeFile,
    pieces: PiecesFile,
    start: StartFile,
    speeds: SpeedsFile,
    instruments: InstrumentsFile,
    autopilot: Option<AutopilotFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisualFile {
    model: String,
    cockpit: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MassFile {
    mass: f64,
    inertia: Vector,
    reference_load: LoadFile,
    stations: StationsFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadFile {
    front: f64,
    rear: f64,
    baggage: f64,
    fuel: f64,
}

/// Each item of the load: where it is and the half width it is spread over.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StationsFile {
    front: (Vector, f64),
    rear: (Vector, f64),
    baggage: (Vector, f64),
    fuel: (Vector, f64),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WingFile {
    area: f64,
    span: f64,
    chord: f64,
    position: Vector,
    strip_edges: Vec<f64>,
    flapped_strips: usize,
    aileron_strips: usize,
    lift_slope: f64,
    cl0: f64,
    stall_alpha_deg: f64,
    stall_alpha_negative_deg: f64,
    stall_sharpness: f64,
    stall_hysteresis_deg: f64,
    stall_lag_chords: f64,
    post_stall_lift: f64,
    cd0: f64,
    induced_k: f64,
    ground_lift: f64,
    cm: f64,
    root_twist_deg: f64,
    tip_twist_deg: f64,
    aileron_power: f64,
    aileron_max_deg: f64,
    roll_damping_relief: f64,
    #[serde(default)]
    sweep_deg: f64,
    #[serde(default = "one")]
    aileron_clean: f64,
}

fn one() -> f64 {
    1.0
}

/// A section or a field a type may have or not: written as itself when it has it, left out when
/// it has not.
fn some<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// The flaps of a light aircraft travel at 4° a second.
fn light_flap_rate() -> f64 {
    4.0
}

/// A point of a light airframe pressed into the ground: N/m and N·s/m.
fn light_hard_spring() -> (f64, f64) {
    (150_000.0, 8_000.0)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlapsFile {
    notches_deg: Vec<f64>,
    lift_curve: Vec<f64>,
    drag_curve: Vec<f64>,
    cl: f64,
    stall_shift_deg: f64,
    drag: f64,
    cm: f64,
    downwash: f64,
    #[serde(default = "light_flap_rate")]
    rate_deg_s: f64,
    /// Highest speed at each notch after the first (0°); without it, `structure.vfe_kt`.
    #[serde(default)]
    speeds_kt: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TailFile {
    position: Vector,
    area: f64,
    lift_slope: f64,
    stall_deg: f64,
    incidence_deg: f64,
    efficiency: f64,
    elevator_tau: f64,
    elevator_up_deg: f64,
    elevator_down_deg: f64,
    trim_range_deg: f64,
    downwash_per_cl: f64,
    wash: f64,
    /// (nose up, nose down): the trim turns the whole tailplane.
    #[serde(default, deserialize_with = "some")]
    stabiliser_trim_deg: Option<(f64, f64)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinFile {
    position: Vector,
    area: f64,
    lift_slope: f64,
    stall_deg: f64,
    rudder_tau: f64,
    rudder_max_deg: f64,
    wash: f64,
    swirl_factor: f64,
    rig: f64,
    roll_rig: f64,
    rig_speed_kt: f64,
    #[serde(default, deserialize_with = "some")]
    yaw_damper: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyFile {
    drag_area: Vector,
    roll_beta: f64,
    yaw_beta: f64,
    pitch_damping: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropellerFile {
    diameter: f64,
    position: Vector,
    inertia: f64,
    advance_ratio: Vec<f64>,
    thrust: Vec<f64>,
    power: Vec<f64>,
    pitch_deg: f64,
    governor: Option<GovernorFile>,
    p_factor: f64,
    torque_reaction: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GovernorFile {
    speed_rpm: f64,
    fine_deg: f64,
    coarse_deg: f64,
    gain: f64,
    damping: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineFile {
    cylinders: u32,
    gross_torque: f64,
    torque_rpm: f64,
    friction: (f64, f64),
    pumping: f64,
    fuel_idle: f64,
    fuel_per_joule: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JetsFile {
    engines: Vec<MountFile>,
    static_thrust: f64,
    airflow: f64,
    idle: f64,
    flight_idle: f64,
    redline: f64,
    altitude_rating: f64,
    pressure_exponent: f64,
    flow_exponent: f64,
    heat: f64,
    spool: (f64, f64),
    fuel_idle: f64,
    efficiency: f64,
    efficiency_cold: f64,
    #[serde(default, deserialize_with = "some")]
    reverser: Option<ReverserFile>,
    fan_diameter: f64,
    n1_rpm: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MountFile {
    at: Vector,
    pod: Vector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReverserFile {
    share: f64,
    time: f64,
    limit: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachFile {
    divergence: (f64, f64),
    tuck: (f64, f64),
    /// (Mach, share of the low-speed lift at the stall).
    lift_limit: Vec<(f64, f64)>,
    mmo: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpoilersFile {
    strips: (usize, usize),
    lift: f64,
    drag: f64,
    flight_limit: f64,
    roll: f64,
    time: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetractFile {
    time: f64,
    drag_area: f64,
    limit_speed_kt: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GearFile {
    wheels: Vec<WheelFile>,
    travel: f64,
    steer_max_deg: f64,
    cg_height: f64,
    ground_pitch_deg: f64,
    #[serde(default, deserialize_with = "some")]
    retract: Option<RetractFile>,
    #[serde(default = "one")]
    braking: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WheelFile {
    contact: Vector,
    spring: f64,
    damping: f64,
    steers: bool,
    brake: Brake,
    slip: f64,
    strength: f64,
    broken: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HardPointsFile {
    left_wing_tip: Vector,
    right_wing_tip: Vector,
    /// The tip of the propeller at its lowest; the nose of a jet.
    #[serde(alias = "nose")]
    propeller: Vector,
    cabin_top: Vector,
    fin_top: Vector,
    belly: Vector,
    tail_skid: Vector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StructureFile {
    load_limit: f64,
    load_limit_negative: f64,
    load_limit_flaps: f64,
    ultimate_factor: f64,
    vne_kt: f64,
    design_dive_speed_kt: f64,
    vfe_kt: (f64, f64),
    breakup_speed: f64,
    aileron_chord: f64,
    flap_chord: f64,
    elevator_share: f64,
    rudder_share: f64,
    wing_tanks: Option<f64>,
    wing_crush: f64,
    #[serde(default = "light_hard_spring")]
    hard_spring: (f64, f64),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FloatsFile {
    nose: FloatFile,
    cabin: FloatFile,
    tail: FloatFile,
    left_wing: FloatFile,
    right_wing: FloatFile,
    wheels: Vec<FloatFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FloatFile {
    at: Vector,
    volume: f64,
    depth: f64,
    area: f64,
}

/// Boxes, lower and upper corners.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AirframeFile {
    wing: (Vector, Vector),
    cabin: (Vector, Vector),
    tail_cone: (Vector, Vector),
    stabiliser: (Vector, Vector),
    fin: (Vector, Vector),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PiecesFile {
    tail_x: f64,
    wing_z: (f64, f64),
    wing: SpanFile,
    stabiliser: SpanFile,
    tail_mass: f64,
    fuselage: PieceFile,
    left_wing: PieceFile,
    right_wing: PieceFile,
    tail: PieceFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpanFile {
    root: f64,
    tip: f64,
    lead: (f64, f64),
    trail: (f64, f64),
    z: (f64, f64),
    thickness: f64,
    mass: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PieceFile {
    centre: Vector,
    points: Vec<Vector>,
    drag_area: f64,
    size: Vector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartFile {
    air_speed_kt: f64,
    final_speed_kt: f64,
    final_flap_notch: i32,
    #[serde(default)]
    takeoff_flap_notch: i32,
    /// The runway it needs, m.
    #[serde(default)]
    runway: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpeedsFile {
    stall_clean_kt: f64,
    stall_full_kt: f64,
    least_kt: f64,
    most_kt: f64,
    flaps_kt: f64,
    rotate_kt: f64,
    climb_kt: f64,
    pattern_kt: f64,
    descent_kt: f64,
    base_kt: f64,
    final_approach_kt: f64,
    join_kt: f64,
    cruise_kt: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstrumentsFile {
    static_ports: StaticPorts,
    /// (flaps, degrees; [(angle of attack, degrees; error)]).
    static_error: Vec<(f64, Vec<(f64, f64)>)>,
    alternate_static: Option<f64>,
    pitot_heat: bool,
    attitude: GyroDrive,
    heading: GyroDrive,
    attitude_pitch_limit_deg: f64,
    compass_lubber_deg: f64,
    compass_iron: Vector,
    airspeed_marks: MarksFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarksFile {
    white_kt: Option<(f64, f64)>,
    green_kt: (f64, f64),
    red_line_kt: f64,
    dial_top_kt: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum AutopilotFile {
    Gfc700 {
        roll_authority: f64,
        pitch_authority: f64,
        roll_rate: f64,
        pitch_rate: f64,
        trim_rate: f64,
        bank_limit_deg: f64,
        pitch_up_deg: f64,
        pitch_down_deg: f64,
        max_speed_kt: f64,
        min_speed_kt: f64,
    },
}

/// `Err` with the field and what is wrong with it, unless `ok`.
fn need(ok: bool, field: &str, what: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("{field}: {what}"))
    }
}

fn increasing(values: &[f64]) -> bool {
    values.windows(2).all(|w| w[0] < w[1])
}

impl File {
    /// What the flight model needs of the numbers, beyond their being there.
    fn check(&self) -> Result<(), String> {
        need(!self.key.is_empty() && self.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "key", "lowercase letters, digits and _ only")?;
        need(!self.name.trim().is_empty(), "name", "empty")?;
        need(!self.visual.model.is_empty() && !self.visual.cockpit.is_empty(), "visual", "a model and a cockpit are needed")?;
        let m = &self.mass;
        need(m.mass > 0.0, "mass.mass", "must be positive")?;
        need(m.inertia.0 > 0.0 && m.inertia.1 > 0.0 && m.inertia.2 > 0.0, "mass.inertia", "every moment must be positive")?;
        let w = &self.wing;
        need(w.area > 0.0 && w.span > 0.0 && w.chord > 0.0, "wing", "area, span and chord must be positive")?;
        need(w.strip_edges.len() == 4, "wing.strip_edges", "four: where the four strips of a half wing begin")?;
        need(w.strip_edges[0] > 0.0 && increasing(&w.strip_edges) && w.strip_edges[3] < w.span / 2.0, "wing.strip_edges", "must increase from the root, all of them inside the half span")?;
        need(w.flapped_strips + w.aileron_strips <= 4, "wing", "flapped_strips and aileron_strips are more than the 4 strips of a half wing")?;
        need(w.aileron_strips > 0, "wing.aileron_strips", "an aircraft needs ailerons")?;
        need(w.stall_alpha_deg > 0.0 && w.stall_alpha_negative_deg > 0.0, "wing", "stall angles must be positive")?;
        let f = &self.flaps;
        need(f.notches_deg.first() == Some(&0.0) && increasing(&f.notches_deg), "flaps.notches_deg", "must start at 0 (no flaps) and increase")?;
        need(f.lift_curve.len() == f.notches_deg.len() && f.drag_curve.len() == f.notches_deg.len(), "flaps", "lift_curve and drag_curve need a value for each notch")?;
        need(f.notches_deg.len() == 1 || w.flapped_strips > 0, "wing.flapped_strips", "flap notches but no strip carries flap")?;
        match (&self.propeller, &self.engine, &self.jets) {
            (Some(p), Some(e), None) => {
                need(p.diameter > 0.0, "propeller.diameter", "must be positive")?;
                need(p.advance_ratio.len() >= 2 && increasing(&p.advance_ratio), "propeller.advance_ratio", "at least two, increasing")?;
                need(p.thrust.len() == p.advance_ratio.len() && p.power.len() == p.advance_ratio.len(), "propeller", "thrust and power need a value for each advance ratio")?;
                if let Some(g) = &p.governor {
                    need(g.speed_rpm > 0.0 && g.fine_deg < g.coarse_deg, "propeller.governor", "a positive speed and a fine stop below the coarse one")?;
                }
                need(e.gross_torque > 0.0 && e.torque_rpm > 0.0, "engine", "gross_torque and torque_rpm must be positive")?;
            }
            (None, None, Some(j)) => {
                need((1..=MOST_ENGINES).contains(&j.engines.len()), "jets.engines", &format!("from 1 to {MOST_ENGINES}"))?;
                need(j.static_thrust > 0.0 && j.airflow > 0.0, "jets", "static_thrust and airflow must be positive")?;
                need(j.idle > 0.0 && j.idle <= j.flight_idle && j.flight_idle < 1.0 && j.redline >= 1.0, "jets", "0 < idle <= flight_idle < 1 <= redline")?;
                need(j.spool.0 > 0.0 && j.spool.1 >= 0.0, "jets.spool", "must be positive")?;
                need(j.efficiency > 0.0 && j.efficiency < 1.0, "jets.efficiency", "between 0 and 1")?;
                if let Some(r) = &j.reverser {
                    need(r.share >= 0.0 && r.time > 0.0 && r.limit > j.idle, "jets.reverser", "a share, a positive time and a limit above idle")?;
                }
            }
            _ => return Err("engines: either propeller and engine, or jets".to_string()),
        }
        if let Some(m) = &self.mach {
            need(m.lift_limit.len() >= 2 && increasing(&m.lift_limit.iter().map(|p| p.0).collect::<Vec<_>>()), "mach.lift_limit", "at least two, their Mach increasing")?;
            need(m.mmo > 0.0 && m.divergence.0 > 0.0, "mach", "mmo and divergence must be positive")?;
        }
        if let Some(sp) = &self.spoilers {
            need(sp.strips.0 < sp.strips.1 && sp.strips.1 <= 4, "spoilers.strips", "from a strip up to (not including) another, of the 4 of a half wing")?;
            need(sp.time > 0.0 && (0.0..=1.0).contains(&sp.flight_limit), "spoilers", "a positive time, and a flight_limit from 0 to 1")?;
        }
        need(w.sweep_deg.abs() < 60.0, "wing.sweep_deg", "less than 60")?;
        need(f.rate_deg_s > 0.0, "flaps.rate_deg_s", "must be positive")?;
        need(f.speeds_kt.is_empty() || f.speeds_kt.len() == f.notches_deg.len() - 1, "flaps.speeds_kt", "one for each notch after the first")?;
        if let Some(r) = &self.gear.retract {
            need(r.time > 0.0 && r.drag_area >= 0.0, "gear.retract", "a positive time and a drag area")?;
        }
        let g = &self.gear;
        need((1..=MOST_WHEELS).contains(&g.wheels.len()), "gear.wheels", &format!("from 1 to {MOST_WHEELS}"))?;
        need(g.wheels.iter().all(|w| w.spring > 0.0 && w.strength > 0.0 && w.slip > 0.0), "gear.wheels", "spring, strength and slip must be positive")?;
        need(g.travel > 0.0, "gear.travel", "must be positive")?;
        need(self.floats.wheels.len() == g.wheels.len(), "floats.wheels", "one for each wheel of gear.wheels")?;
        let s = &self.structure;
        need(s.load_limit > 0.0 && s.load_limit_negative < 0.0 && s.load_limit_flaps > 0.0, "structure", "load limits: positive, negative, and positive with flaps")?;
        need(s.ultimate_factor > 1.0, "structure.ultimate_factor", "must be more than 1")?;
        need(s.vne_kt > 0.0 && s.design_dive_speed_kt > s.vne_kt, "structure", "vne_kt positive and below design_dive_speed_kt")?;
        let pieces = &self.pieces;
        for (name, span) in [("pieces.wing", &pieces.wing), ("pieces.stabiliser", &pieces.stabiliser)] {
            need(span.root >= 0.0 && span.root < span.tip && span.mass > 0.0, name, "root before tip, and a mass")?;
        }
        need(pieces.wing.tip <= w.span / 2.0 + 0.01, "pieces.wing.tip", "beyond the half span")?;
        for (name, piece) in [("pieces.fuselage", &pieces.fuselage), ("pieces.left_wing", &pieces.left_wing), ("pieces.right_wing", &pieces.right_wing), ("pieces.tail", &pieces.tail)] {
            need((1..=10).contains(&piece.points.len()), name, "from 1 to 10 points")?;
            need(piece.drag_area > 0.0, name, "drag_area must be positive")?;
        }
        need((0..self.flaps.notches_deg.len() as i32).contains(&self.start.final_flap_notch), "start.final_flap_notch", "not a notch of flaps.notches_deg")?;
        need((0..self.flaps.notches_deg.len() as i32).contains(&self.start.takeoff_flap_notch), "start.takeoff_flap_notch", "not a notch of flaps.notches_deg")?;
        let sp = &self.speeds;
        need(sp.stall_full_kt > 0.0 && sp.stall_full_kt <= sp.stall_clean_kt, "speeds", "stall_full_kt positive and no more than stall_clean_kt")?;
        need(sp.stall_clean_kt < sp.least_kt && sp.least_kt <= sp.most_kt, "speeds", "stall_clean_kt < least_kt <= most_kt")?;
        let i = &self.instruments;
        need(increasing(&i.static_error.iter().map(|t| t.0).collect::<Vec<_>>()), "instruments.static_error", "flap settings must increase")?;
        for (flaps, table) in &i.static_error {
            need(!table.is_empty() && increasing(&table.iter().map(|p| p.0).collect::<Vec<_>>()), "instruments.static_error", &format!("the table for flaps {flaps}: angles must increase"))?;
        }
        Ok(())
    }

    /// The type the file describes, already checked.
    fn build(self) -> AircraftType {
        let kind = AircraftKind(keep_str(self.key));
        // A jet has no propeller and no piston engine: theirs say nothing then.
        let propeller = self.propeller.unwrap_or(PropellerFile {
            diameter: 1.0,
            position: (0.0, 0.0, 0.0),
            inertia: 1.0,
            advance_ratio: vec![0.0, 1.0],
            thrust: vec![0.0, 0.0],
            power: vec![0.0, 0.0],
            pitch_deg: 0.0,
            governor: None,
            p_factor: 0.0,
            torque_reaction: 0.0,
        });
        let engine = self.engine.unwrap_or(EngineFile { cylinders: 0, gross_torque: 0.0, torque_rpm: 1.0, friction: (0.0, 0.0), pumping: 0.0, fuel_idle: 0.0, fuel_per_joule: 0.0 });
        let jets = self.jets.map(|j| {
            Jets {
                mounts: keep(j.engines.iter().map(|m| Mount { at: v(m.at), pod: v(m.pod) }).collect()),
                static_thrust: j.static_thrust,
                airflow: j.airflow,
                idle: j.idle,
                flight_idle: j.flight_idle,
                redline: j.redline,
                altitude_rating: j.altitude_rating,
                pressure_exponent: j.pressure_exponent,
                flow_exponent: j.flow_exponent,
                heat: j.heat,
                spool: j.spool,
                fuel_idle: j.fuel_idle,
                efficiency: j.efficiency,
                efficiency_cold: j.efficiency_cold,
                reverser: j.reverser.map(|r| Reverser { share: r.share, time: r.time, limit: r.limit }),
                fan_diameter: j.fan_diameter,
                n1_rpm: j.n1_rpm,
                pressure_ratio: 0.0,
            }
            .rated()
        });
        let mach = self.mach.map(|m| Compressibility {
            divergence: m.divergence,
            tuck: m.tuck,
            lift_limit_mach: keep(m.lift_limit.iter().map(|p| p.0).collect()),
            lift_limit: keep(m.lift_limit.iter().map(|p| p.1).collect()),
            mmo: m.mmo,
        });
        let spoilers = self.spoilers.map(|x| Spoilers { strips: x.strips, lift: x.lift, drag: x.drag, flight_limit: x.flight_limit, roll: x.roll, time: x.time });
        let (m, w, f, t, n, b, p, e, g, h, s, fl, a, pc, st, sp, i) = (
            self.mass, self.wing, self.flaps, self.tail, self.fin, self.body, propeller, engine, self.gear,
            self.hard_points, self.structure, self.floats, self.airframe, self.pieces, self.start, self.speeds, self.instruments,
        );
        let station = |(at, half): (Vector, f64)| (v(at), half);
        let float = |x: &FloatFile| (v(x.at), x.volume, x.depth, x.area);
        // A wheel the type has not floats nothing and drags nothing.
        let wheel_float = |i: usize| fl.wheels.get(i).map_or((V3::ZERO, 0.0, 1.0, 0.0), float);
        let solid = |(lo, hi): (Vector, Vector)| (v(lo), v(hi));
        let span = |x: &SpanFile| Span { root: x.root, tip: x.tip, lead: x.lead, trail: x.trail, z: x.z, thickness: x.thickness, mass: x.mass };
        let parts = [&pc.fuselage, &pc.left_wing, &pc.right_wing, &pc.tail];
        let pieces: &'static Pieces = Box::leak(Box::new(Pieces {
            kind,
            tail_x: pc.tail_x,
            wing_z: pc.wing_z,
            wing: span(&pc.wing),
            stabiliser: span(&pc.stabiliser),
            tail_mass: pc.tail_mass,
            centres: parts.map(|x| v(x.centre)),
            points: parts.map(|x| keep(x.points.iter().map(|&q| v(q)).collect())),
            drag_areas: parts.map(|x| x.drag_area),
            boxes: parts.map(|x| x.size),
        }));
        let notches = keep(f.notches_deg);
        let retract = g.retract.as_ref().map(|r| Retract { time: r.time, drag_area: r.drag_area, limit_speed: r.limit_speed_kt * KNOTS });
        AircraftType {
            jets,
            sweep: w.sweep_deg * DEG,
            mach,
            spoilers,
            retract,
            stabiliser_trim: t.stabiliser_trim_deg.map(|(up, down)| (up * DEG, down * DEG)),
            yaw_damper: n.yaw_damper,
            flap_rate: f.rate_deg_s,
            flap_speeds: keep(f.speeds_kt.iter().map(|kt| kt * KNOTS).collect()),
            aileron_clean: w.aileron_clean,
            hard_spring: s.hard_spring,
            braking: g.braking,
            kind,
            name: keep_str(self.name),
            aliases: keep(self.aliases.into_iter().map(keep_str).collect()),
            visual: Visual { model: keep_str(self.visual.model), cockpit: keep_str(self.visual.cockpit) },
            mass: m.mass,
            inertia: v(m.inertia),
            wing_area: w.area,
            span: w.span,
            chord: w.chord,

            strip_edges: [w.strip_edges[0], w.strip_edges[1], w.strip_edges[2], w.strip_edges[3]],
            flapped_strips: w.flapped_strips,
            aileron_strips: w.aileron_strips,
            wing_pos: v(w.position),
            wing_slope: w.lift_slope,
            wing_cl0: w.cl0,
            stall_alpha: w.stall_alpha_deg * DEG,
            stall_alpha_neg: w.stall_alpha_negative_deg * DEG,
            stall_sharpness: w.stall_sharpness,
            stall_hysteresis: w.stall_hysteresis_deg * DEG,
            stall_lag_chords: w.stall_lag_chords,
            post_stall_lift: w.post_stall_lift,
            wing_cd0: w.cd0,
            induced_k: w.induced_k,
            ground_lift: w.ground_lift,
            wing_cm: w.cm,
            root_twist: w.root_twist_deg * DEG,
            tip_twist: w.tip_twist_deg * DEG,

            flap_notches: notches,
            flap_lift_curve: keep(f.lift_curve),
            flap_drag_curve: keep(f.drag_curve),
            flap_cl: f.cl,
            flap_stall_shift: f.stall_shift_deg * DEG,
            flap_drag: f.drag,
            flap_cm: f.cm,
            flap_downwash: f.downwash,
            aileron_power: w.aileron_power,
            aileron_max: w.aileron_max_deg * DEG,

            tail_pos: v(t.position),
            tail_area: t.area,
            tail_slope: t.lift_slope,
            tail_stall: t.stall_deg * DEG,
            tail_incidence: t.incidence_deg * DEG,
            tail_efficiency: t.efficiency,
            elevator_tau: t.elevator_tau,
            elevator_up: t.elevator_up_deg * DEG,
            elevator_down: t.elevator_down_deg * DEG,
            trim_range: t.trim_range_deg * DEG,
            downwash_per_cl: t.downwash_per_cl,
            tail_wash: t.wash,

            fin_pos: v(n.position),
            fin_area: n.area,
            fin_slope: n.lift_slope,
            fin_stall: n.stall_deg * DEG,
            rudder_tau: n.rudder_tau,
            rudder_max: n.rudder_max_deg * DEG,
            fin_wash: n.wash,
            swirl_factor: n.swirl_factor,
            fin_rig: n.rig,
            roll_rig: n.roll_rig,
            rig_speed: n.rig_speed_kt * KNOTS,

            body_drag_area: v(b.drag_area),
            body_roll_beta: b.roll_beta,
            body_yaw_beta: b.yaw_beta,
            body_pitch_damping: b.pitch_damping,

            prop_diameter: p.diameter,
            prop_pos: v(p.position),
            prop_inertia: p.inertia,
            prop_j: keep(p.advance_ratio),
            prop_ct: keep(p.thrust),
            prop_cp: keep(p.power),
            prop_pitch: p.pitch_deg * DEG,
            governor: p.governor.map(|g| Governor {
                speed: g.speed_rpm * 2.0 * PI / 60.0,
                fine: g.fine_deg * DEG,
                coarse: g.coarse_deg * DEG,
                gain: g.gain,
                damping: g.damping,
            }),
            cylinders: e.cylinders,
            gross_torque: e.gross_torque,
            torque_rpm: e.torque_rpm,
            friction: [e.friction.0, e.friction.1],
            pumping: e.pumping,
            p_factor: p.p_factor,
            roll_damping_relief: w.roll_damping_relief,
            torque_reaction: p.torque_reaction,
            fuel_idle: e.fuel_idle,
            fuel_per_joule: e.fuel_per_joule,

            gear: g
                .wheels
                .into_iter()
                .map(|x| Wheel {
                    contact: v(x.contact),
                    spring: x.spring,
                    damping: x.damping,
                    steers: x.steers,
                    brake: x.brake,
                    slip: x.slip,
                    strength: x.strength,
                    broken: keep_str(x.broken),
                })
                .collect(),
            gear_travel: g.travel,
            steer_max: g.steer_max_deg * DEG,
            cg_height: g.cg_height,
            ground_pitch: g.ground_pitch_deg * DEG,

            hard_points: [h.left_wing_tip, h.right_wing_tip, h.propeller, h.cabin_top, h.fin_top, h.belly, h.tail_skid].map(v),

            load_limit: s.load_limit,
            load_limit_negative: s.load_limit_negative,
            load_limit_flaps: s.load_limit_flaps,
            ultimate_factor: s.ultimate_factor,
            vne: s.vne_kt * KNOTS,
            // Free of flutter to 1.2 times the design diving speed: the margin certification asks for.
            flutter_speed: 1.2 * s.design_dive_speed_kt * KNOTS,
            vfe: [s.vfe_kt.0 * KNOTS, s.vfe_kt.1 * KNOTS],
            breakup_speed: s.breakup_speed,
            aileron_chord: s.aileron_chord,
            flap_chord: s.flap_chord,
            elevator_share: s.elevator_share,
            rudder_share: s.rudder_share,
            wing_tanks: s.wing_tanks,
            wing_crush: s.wing_crush,
            floats: [
                float(&fl.nose),
                float(&fl.cabin),
                float(&fl.tail),
                float(&fl.left_wing),
                float(&fl.right_wing),
                wheel_float(0),
                wheel_float(1),
                wheel_float(2),
                wheel_float(3),
                wheel_float(4),
            ],
            airframe: [a.wing, a.cabin, a.tail_cone, a.stabiliser, a.fin].map(solid),
            pieces,

            stations: [m.stations.front, m.stations.rear, m.stations.baggage, m.stations.fuel].map(station),
            reference_load: Load { front: m.reference_load.front, rear: m.reference_load.rear, baggage: m.reference_load.baggage, fuel: m.reference_load.fuel },

            air_start_speed: st.air_speed_kt * KNOTS,
            final_speed: st.final_speed_kt * KNOTS,
            final_flap_notch: st.final_flap_notch,
            takeoff_flap_notch: st.takeoff_flap_notch,
            runway: st.runway,

            speeds: Speeds {
                stall_clean: sp.stall_clean_kt * KNOTS,
                stall_full: sp.stall_full_kt * KNOTS,
                least: sp.least_kt * KNOTS,
                most: sp.most_kt * KNOTS,
                flaps: sp.flaps_kt * KNOTS,
                rotate: sp.rotate_kt * KNOTS,
                climb: sp.climb_kt * KNOTS,
                pattern: sp.pattern_kt * KNOTS,
                descent: sp.descent_kt * KNOTS,
                base: sp.base_kt * KNOTS,
                final_approach: sp.final_approach_kt * KNOTS,
                join: sp.join_kt * KNOTS,
                cruise: sp.cruise_kt * KNOTS,
            },

            instruments: InstrumentFit {
                static_ports: i.static_ports,
                static_error: keep(i.static_error.into_iter().map(|(flaps, table)| (flaps, keep(table.into_iter().map(|(alpha, error)| (alpha * DEG, error)).collect()))).collect()),
                alternate_static: i.alternate_static,
                pitot_heat: i.pitot_heat,
                attitude: i.attitude,
                heading: i.heading,
                attitude_pitch_limit: i.attitude_pitch_limit_deg * DEG,
                compass_lubber: i.compass_lubber_deg * DEG,
                compass_iron: v(i.compass_iron),
                airspeed_marks: AirspeedMarks {
                    white: i.airspeed_marks.white_kt.map(|(from, to)| (from * KNOTS, to * KNOTS)),
                    green: (i.airspeed_marks.green_kt.0 * KNOTS, i.airspeed_marks.green_kt.1 * KNOTS),
                    red_line: i.airspeed_marks.red_line_kt * KNOTS,
                    dial_top: i.airspeed_marks.dial_top_kt * KNOTS,
                },
            },

            afcs: self.autopilot.map(|AutopilotFile::Gfc700 { roll_authority, pitch_authority, roll_rate, pitch_rate, trim_rate, bank_limit_deg, pitch_up_deg, pitch_down_deg, max_speed_kt, min_speed_kt }| Gfc700Fit {
                roll_authority,
                pitch_authority,
                roll_rate,
                pitch_rate,
                trim_rate,
                bank_limit: bank_limit_deg * DEG,
                pitch_up: pitch_up_deg * DEG,
                pitch_down: pitch_down_deg * DEG,
                max_speed: max_speed_kt * KNOTS,
                min_speed: min_speed_kt * KNOTS,
            }),
        }
    }
}
