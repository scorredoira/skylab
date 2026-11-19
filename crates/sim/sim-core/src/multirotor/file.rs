//! Types of multirotor as data: a file `assets/aircraft/<key>/multirotor.ron` read into a
//! `MultirotorType` and checked, and the multirotors there are (`fleet`), as `aircraft::file` does
//! for the aircraft. The folder is the aircraft's: a drone's look (`exterior.glb`, `exterior.ron`)
//! is found the same way as an aircraft's.
//!
//! The sections and units are `crates/sim/sim-core/README.md`'s "Multirotor files"; the 5-inch FPV
//! quad's file is the example, the racing quad's (`race5`) another. Nothing here reads the disk: the game hands the files' text over
//! (`fleet_with`, `set_fleet`); without that the fleet is the files built into the program.

use std::sync::OnceLock;

use serde::Deserialize;

use super::{Camera, ControllerFit, HoldFit, Motor, MultirotorKind, MultirotorType, MultirotorVisual, PropStrike, Spin, Stability, StickRates, Surface, Tilt, Turbine, WashFit, MOST_MOTORS};
use crate::aircraft::file::FileError;
use crate::math::{DEG, V3};

/// The files of `assets/aircraft` built into the program: (where it is in the repository, its text).
pub const BUILT_IN: [(&str, &str); 3] = [
    ("assets/aircraft/fpv5/multirotor.ron", include_str!("../../../../../assets/aircraft/fpv5/multirotor.ron")),
    ("assets/aircraft/race5/multirotor.ron", include_str!("../../../../../assets/aircraft/race5/multirotor.ron")),
    ("assets/aircraft/kestrel/multirotor.ron", include_str!("../../../../../assets/aircraft/kestrel/multirotor.ron")),
];

/// Reads a type of multirotor from the text of its file; `file` names it in the errors.
pub fn load(text: &str, file: &str) -> Result<MultirotorType, FileError> {
    let error = |message: String| FileError { file: file.to_string(), message };
    let parsed: File = ron::from_str(text).map_err(|e| error(e.to_string()))?;
    parsed.check().map_err(error)?;
    Ok(parsed.build())
}

static FLEET: OnceLock<Vec<MultirotorType>> = OnceLock::new();

/// The types of multirotor there are: those given to `set_fleet`, or else the built-in ones.
pub fn fleet() -> &'static [MultirotorType] {
    FLEET.get_or_init(|| BUILT_IN.iter().map(|(file, text)| load(text, file).unwrap_or_else(|e| panic!("{e}"))).collect())
}

/// Makes `types` the multirotors, before anything has asked for them (the game does it at start).
/// Their keys and names must differ from each other's; the aircraft's are the game's to check.
pub fn set_fleet(types: Vec<MultirotorType>) -> Result<(), String> {
    for (i, t) in types.iter().enumerate() {
        for other in &types[..i] {
            if let Some(word) = t.words().find(|w| other.words().any(|o| o == *w)) {
                return Err(format!("the multirotors {} and {} are both called \"{word}\"", other.kind.key(), t.kind.key()));
            }
        }
    }
    FLEET.set(types).map_err(|_| "the multirotors were already in use when they were read".to_string())
}

/// The built-in types with these files' in their place: a file of a built-in type's key replaces
/// it, any other is added after them, in the order given.
pub fn fleet_with(files: &[(String, String)]) -> Result<Vec<MultirotorType>, FileError> {
    let mut types: Vec<MultirotorType> = Vec::new();
    for (file, text) in BUILT_IN.iter().map(|(f, t)| (*f, *t)).chain(files.iter().map(|(f, t)| (f.as_str(), t.as_str()))) {
        let t = load(text, file)?;
        match types.iter_mut().find(|old| old.kind == t.kind) {
            Some(old) => *old = t,
            None => types.push(t),
        }
    }
    Ok(types)
}

impl MultirotorType {
    /// The words that name it: its key and its aliases.
    pub(crate) fn words(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.kind.key()).chain(self.aliases.iter().copied())
    }
}

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
    frame: FrameFile,
    motors: Vec<MotorFile>,
    /// Electric motors off a battery, or turbines: one or the other.
    #[serde(default, deserialize_with = "present")]
    motor: Option<MotorSpecFile>,
    propeller: PropellerFile,
    wash: WashFile,
    #[serde(default, deserialize_with = "present")]
    battery: Option<BatteryFile>,
    #[serde(default, deserialize_with = "present")]
    turbine: Option<TurbineFile>,
    controller: ControllerFile,
    start: StartFile,
    /// A tilt-rotor's nacelles, wings and tail; a plain multirotor has none.
    #[serde(default, deserialize_with = "present")]
    tilt: Option<TiltFile>,
}

/// A section that may be left out: written, it is there (as any other section, with no `Some`).
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurbineFile {
    engines: u32,
    power_kw: f64,
    rotor_rpm: f64,
    pitch: (f64, f64),
    inflow_length: f64,
    induced_factor: f64,
    profile_power_kw: f64,
    inertia: f64,
    spool_time: f64,
    start_time: f64,
    fuel: f64,
    fuel_per_kwh: f64,
    idle_flow_per_hour: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TiltFile {
    pivot: Vector,
    forward_deg: f64,
    margin_deg: f64,
    #[serde(default)]
    stick_tilt_deg: f64,
    #[serde(default)]
    back_deg: f64,
    rate_deg: f64,
    conversion_rate_deg: f64,
    split_authority_deg: f64,
    cyclic_arm: f64,
    direct: Vector,
    #[serde(default, deserialize_with = "present")]
    hold: Option<HoldFile>,
    turn_from: f64,
    turn_full: f64,
    surfaces: Vec<SurfaceFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HoldFile {
    climb: (f64, f64),
    dead_band: f64,
    speed: (f64, f64),
    climb_gain: f64,
    speed_gain: f64,
    bank_deg: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceFile {
    position: Vector,
    area: f64,
    aspect: f64,
    #[serde(default)]
    upright: bool,
    #[serde(default)]
    incidence_deg: f64,
    stall_deg: f64,
    drag: f64,
    #[serde(default)]
    elevator: Option<ElevatorFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElevatorFile {
    effect: f64,
    travel_deg: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VisualFile {
    model: String,
    camera: CameraFile,
    #[serde(default, deserialize_with = "present")]
    cockpit: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraFile {
    position: Vector,
    uptilt_deg: f64,
    fov_deg: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MassFile {
    mass: f64,
    inertia: Vector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameFile {
    drag_area: Vector,
    rotational_damping: f64,
    contacts: Vec<Vector>,
    contact_spring: f64,
    contact_damping: f64,
    friction: f64,
    crash_speed: f64,
    solid: Vec<(Vector, Vector)>,
    parts: Vec<(Vector, f64)>,
    restitution: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MotorFile {
    position: Vector,
    spin: Spin,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MotorSpecFile {
    kv_rpm: f64,
    resistance: f64,
    no_load_current: f64,
    rotor_inertia: f64,
    max_current: f64,
    idle: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropellerFile {
    diameter: f64,
    pitch: f64,
    thrust_coefficient: f64,
    torque_coefficient: f64,
    edgewise_drag: f64,
    strike: StrikeFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrikeFile {
    thickness: f64,
    graze_speed: f64,
    chip_depth: f64,
    stub: f64,
    shake: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WashFile {
    onset: f64,
    full: f64,
    clear_from: f64,
    clear_to: f64,
    thrust_loss: f64,
    fluctuation: f64,
    buffet: f64,
    correlation_time: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatteryFile {
    cells: u32,
    capacity_mah: f64,
    internal_resistance: f64,
    cell_voltage: Vec<(f64, f64)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RatesFile {
    rc_rate: Vector,
    super_rate: Vector,
    expo: Vector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerFile {
    rates: RatesFile,
    p: Vector,
    i: Vector,
    d: Vector,
    feed_forward: Vector,
    i_limit: f64,
    angle_max_deg: f64,
    angle_gain: f64,
    /// The help it starts with: `Level` (the default), `Rates`, `Off` or `Hold`.
    #[serde(default)]
    stability: StabilityFile,
}

#[derive(Deserialize, Default, Clone, Copy)]
enum StabilityFile {
    #[default]
    Level,
    Rates,
    Off,
    Hold,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartFile {
    air_height: f64,
}

fn need(ok: bool, field: &str, what: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("{field}: {what}"))
    }
}

fn positive((x, y, z): Vector) -> bool {
    x > 0.0 && y > 0.0 && z > 0.0
}

impl File {
    /// What the model needs of the numbers, beyond their being there.
    fn check(&self) -> Result<(), String> {
        need(!self.key.is_empty() && self.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "key", "lowercase letters, digits and _ only")?;
        need(!self.name.trim().is_empty(), "name", "empty")?;
        need(!self.visual.model.is_empty(), "visual.model", "a model is needed")?;
        let c = &self.visual.camera;
        need((-30.0..=80.0).contains(&c.uptilt_deg), "visual.camera.uptilt_deg", "between -30 and 80")?;
        need((30.0..=170.0).contains(&c.fov_deg), "visual.camera.fov_deg", "between 30 and 170")?;
        need(self.mass.mass > 0.0, "mass.mass", "must be positive")?;
        need(positive(self.mass.inertia), "mass.inertia", "every moment must be positive")?;
        let f = &self.frame;
        need(f.drag_area.0 >= 0.0 && f.drag_area.1 >= 0.0 && f.drag_area.2 >= 0.0, "frame.drag_area", "must not be negative")?;
        need(f.contacts.len() >= 3, "frame.contacts", "at least three points to stand on")?;
        need(f.contact_spring > 0.0 && f.contact_damping >= 0.0 && f.friction >= 0.0, "frame", "contact_spring must be positive, contact_damping and friction not negative")?;
        need(f.crash_speed > 0.0, "frame.crash_speed", "must be positive")?;
        need(!f.solid.is_empty() && f.solid.iter().all(|(lo, hi)| lo.0 < hi.0 && lo.1 < hi.1 && lo.2 < hi.2), "frame.solid", "boxes with their lower corner first")?;
        need(!f.parts.is_empty() && f.parts.iter().all(|(_, r)| *r > 0.0), "frame.parts", "at least one ball, each of a positive radius")?;
        need((0.0..=1.0).contains(&f.restitution), "frame.restitution", "from 0 to 1")?;
        need((3..=MOST_MOTORS).contains(&self.motors.len()), "motors", &format!("from 3 to {MOST_MOTORS}"))?;
        need(self.motor.is_some() == self.battery.is_some() && self.turbine.is_some() != self.motor.is_some(), "turbine", "either motor and battery, or turbine")?;
        need(self.turbine.is_none() || self.tilt.is_some(), "turbine", "only a tilt-rotor's flight controller orders blades' pitches: it needs tilt")?;
        if let Some(m) = &self.motor {
            need(m.kv_rpm > 0.0 && m.resistance > 0.0 && m.rotor_inertia > 0.0 && m.max_current > 0.0, "motor", "kv_rpm, resistance, rotor_inertia and max_current must be positive")?;
            need(m.no_load_current >= 0.0, "motor.no_load_current", "must not be negative")?;
            need((0.0..0.5).contains(&m.idle), "motor.idle", "a share of the throttle from 0 to 0.5")?;
        }
        if let Some(t) = &self.turbine {
            need(t.engines > 0 && t.power_kw > 0.0 && t.rotor_rpm > 0.0 && t.inertia > 0.0, "turbine", "engines, power_kw, rotor_rpm and inertia must be positive")?;
            need(t.pitch.1 > t.pitch.0 && t.pitch.1 > 0.0, "turbine.pitch", "the least pitch, then the most, more than nothing")?;
            need(t.inflow_length > 0.0 && t.induced_factor >= 1.0 && t.profile_power_kw >= 0.0, "turbine", "inflow_length positive, induced_factor at least 1, profile_power_kw not negative")?;
            need(t.spool_time > 0.0 && t.start_time > 0.0, "turbine", "spool_time and start_time must be positive")?;
            need(t.fuel > 0.0 && t.fuel_per_kwh >= 0.0 && t.idle_flow_per_hour >= 0.0, "turbine", "fuel positive, fuel_per_kwh and idle_flow_per_hour not negative")?;
        }
        let p = &self.propeller;
        need(p.diameter > 0.0 && p.pitch > 0.0 && p.thrust_coefficient > 0.0 && p.torque_coefficient > 0.0, "propeller", "diameter, pitch and coefficients must be positive")?;
        need(p.edgewise_drag >= 0.0, "propeller.edgewise_drag", "must not be negative")?;
        let k = &p.strike;
        need(k.thickness > 0.0 && k.graze_speed >= 0.0 && k.chip_depth >= 0.0 && k.shake >= 0.0, "propeller.strike", "thickness positive, the others not negative")?;
        need((0.0..1.0).contains(&k.stub), "propeller.strike.stub", "a share of the radius from 0 to less than 1")?;
        let w = &self.wash;
        need(w.onset >= 0.0 && w.full > w.onset, "wash", "onset not negative and full past it")?;
        need(w.clear_from >= 0.0 && w.clear_to > w.clear_from, "wash", "clear_from not negative and clear_to past it")?;
        need((0.0..1.0).contains(&w.thrust_loss), "wash.thrust_loss", "a share from 0 to less than 1")?;
        need(w.fluctuation >= 0.0 && w.buffet >= 0.0, "wash", "fluctuation and buffet must not be negative")?;
        need(w.correlation_time > 0.0, "wash.correlation_time", "must be positive")?;
        if let Some(b) = &self.battery {
            need(b.cells > 0 && b.capacity_mah > 0.0 && b.internal_resistance >= 0.0, "battery", "cells and capacity must be positive")?;
            let shares: Vec<f64> = b.cell_voltage.iter().map(|p| p.0).collect();
            need(b.cell_voltage.len() >= 2 && shares[0] == 0.0 && shares[shares.len() - 1] == 1.0 && shares.windows(2).all(|w| w[0] < w[1]), "battery.cell_voltage", "from 0 (full) to 1 (empty), increasing")?;
            need(b.cell_voltage.windows(2).all(|w| w[0].1 >= w[1].1), "battery.cell_voltage", "the voltage must fall as the charge is used")?;
        }
        let c = &self.controller;
        need(positive(c.rates.rc_rate), "controller.rates.rc_rate", "must be positive")?;
        for (x, name) in [(c.rates.super_rate, "super_rate"), (c.rates.expo, "expo")] {
            need([x.0, x.1, x.2].iter().all(|k| (0.0..1.0).contains(k)), &format!("controller.rates.{name}"), "from 0 to less than 1")?;
        }
        need(positive(c.p), "controller.p", "must be positive")?;
        need(c.angle_max_deg > 0.0 && c.angle_max_deg < 90.0, "controller.angle_max_deg", "between 0 and 90 degrees")?;
        need(c.angle_gain > 0.0, "controller.angle_gain", "must be positive")?;
        need(self.start.air_height > 1.0, "start.air_height", "more than 1 m")?;
        if let Some(t) = &self.tilt {
            need(self.motors.iter().all(|m| m.position.1 != 0.0), "motors", "with nacelles, every motor is on the left one or the right (none on the centre line)")?;
            need([-1.0, 1.0].iter().all(|side| self.motors.iter().any(|m| m.position.1 * side > 0.0)), "motors", "with nacelles, motors on both sides")?;
            need(t.forward_deg > 0.0 && t.forward_deg <= 100.0, "tilt.forward_deg", "between 0 and 100")?;
            need((0.0..=30.0).contains(&t.margin_deg), "tilt.margin_deg", "from 0 to 30")?;
            need((0.0..=45.0).contains(&t.stick_tilt_deg) && (0.0..=30.0).contains(&t.back_deg), "tilt", "stick_tilt_deg from 0 to 45, back_deg from 0 to 30")?;
            need(t.rate_deg > 0.0 && t.conversion_rate_deg > 0.0, "tilt", "rate_deg and conversion_rate_deg must be positive")?;
            need(t.split_authority_deg >= 0.0 && t.split_authority_deg <= t.margin_deg, "tilt.split_authority_deg", "not negative and within margin_deg")?;
            need(t.cyclic_arm >= 0.0, "tilt.cyclic_arm", "must not be negative")?;
            need(positive(t.direct), "tilt.direct", "must be positive")?;
            if let Some(h) = &t.hold {
                need(h.climb.0 > 0.0 && h.climb.1 > 0.0 && h.speed.0 > 0.0 && h.speed.1 > 0.0, "tilt.hold", "climb and speed must be positive")?;
                need((0.0..0.5).contains(&h.dead_band) && h.climb_gain > 0.0 && h.speed_gain > 0.0 && h.bank_deg > 0.0 && h.bank_deg < 45.0, "tilt.hold", "dead_band from 0 to 0.5, the gains positive, bank_deg between 0 and 45")?;
            }
            need(t.turn_from > 0.0 && t.turn_full > t.turn_from, "tilt", "turn_from positive and turn_full past it")?;
            for (i, s) in t.surfaces.iter().enumerate() {
                let field = format!("tilt.surfaces[{i}]");
                need(s.area > 0.0 && s.aspect > 0.0, &field, "area and aspect must be positive")?;
                need(s.stall_deg > 0.0 && s.stall_deg < 45.0, &field, "stall_deg between 0 and 45")?;
                need(s.drag >= 0.0, &field, "drag must not be negative")?;
                if let Some(e) = &s.elevator {
                    need((0.0..=1.0).contains(&e.effect) && e.travel_deg > 0.0 && e.travel_deg <= 45.0, &field, "elevator: effect from 0 to 1, travel_deg from 0 to 45")?;
                    need(!s.upright, &field, "an elevator is a tailplane's, not a fin's")?;
                }
            }
            need(t.surfaces.iter().filter(|s| s.elevator.is_some()).count() <= 1, "tilt.surfaces", "one elevator at most")?;
        }
        Ok(())
    }

    fn build(self) -> MultirotorType {
        let kind = MultirotorKind(keep_str(self.key));
        let turbine = self.turbine.map(|t| Turbine {
            engines: t.engines,
            power: t.power_kw * 1000.0,
            rotor_speed: t.rotor_rpm * std::f64::consts::TAU / 60.0,
            pitch: t.pitch,
            inflow_length: t.inflow_length,
            induced_factor: t.induced_factor,
            profile_power: t.profile_power_kw * 1000.0,
            inertia: t.inertia,
            spool_time: t.spool_time,
            start_time: t.start_time,
            fuel: t.fuel,
            fuel_per_joule: t.fuel_per_kwh / 3.6e6,
            idle_flow: t.idle_flow_per_hour / 3600.0,
        });
        // With turbines there are no motors nor a battery: numbers that do nothing in their place
        // (a rotor's inertia its share of the shaft's).
        let rotors = self.motors.len() as f64;
        let m = self.motor.unwrap_or(MotorSpecFile { kv_rpm: 1.0, resistance: 1.0, no_load_current: 0.0, rotor_inertia: turbine.map_or(1.0, |t| t.inertia / rotors), max_current: 1.0, idle: 0.0 });
        let b = self.battery.unwrap_or(BatteryFile { cells: 1, capacity_mah: 1.0, internal_resistance: 0.0, cell_voltage: vec![(0.0, 0.0), (1.0, 0.0)] });
        let (c, f, p, k) = (self.visual.camera, self.frame, self.propeller, self.controller);
        let tilting = self.tilt.is_some();
        let motors: Vec<Motor> = self.motors.iter().map(|x| Motor { position: v(x.position), spin: x.spin, nacelle: tilting.then_some(if x.position.1 < 0.0 { 0 } else { 1 }) }).collect();
        let tilt = self.tilt.map(|t| Tilt {
            pivot: v(t.pivot),
            forward: t.forward_deg * DEG,
            margin: t.margin_deg * DEG,
            stick_tilt: t.stick_tilt_deg * DEG,
            back: t.back_deg * DEG,
            rate: t.rate_deg * DEG,
            conversion_rate: t.conversion_rate_deg * DEG,
            split_authority: t.split_authority_deg * DEG,
            cyclic_arm: t.cyclic_arm,
            direct: v(t.direct),
            hold: t.hold.map(|h| HoldFit { climb: h.climb, dead_band: h.dead_band, speed: h.speed, climb_gain: h.climb_gain, speed_gain: h.speed_gain, bank: h.bank_deg * DEG }),
            turn_from: t.turn_from,
            turn_full: t.turn_full,
            surfaces: keep(
                t.surfaces
                    .into_iter()
                    .map(|s| Surface {
                        position: v(s.position),
                        area: s.area,
                        aspect: s.aspect,
                        upright: s.upright,
                        incidence: s.incidence_deg * DEG,
                        stall: s.stall_deg * DEG,
                        drag: s.drag,
                        elevator: s.elevator.map(|e| (e.effect, e.travel_deg * DEG)),
                    })
                    .collect(),
            ),
        });
        let mut t = MultirotorType {
            kind,
            name: keep_str(self.name),
            aliases: keep(self.aliases.into_iter().map(keep_str).collect()),
            visual: MultirotorVisual { model: keep_str(self.visual.model), camera: Camera { position: v(c.position), uptilt: c.uptilt_deg * DEG, fov: c.fov_deg * DEG }, cockpit: self.visual.cockpit.map(keep_str) },
            mass: self.mass.mass,
            inertia: v(self.mass.inertia),
            drag_area: v(f.drag_area),
            rotational_damping: f.rotational_damping,
            contacts: keep(f.contacts.into_iter().map(v).collect()),
            contact_spring: f.contact_spring,
            contact_damping: f.contact_damping,
            friction: f.friction,
            crash_speed: f.crash_speed,
            solid: keep(f.solid.into_iter().map(|(lo, hi)| (v(lo), v(hi))).collect()),
            parts: keep(f.parts.into_iter().map(|(c, r)| (v(c), r)).collect()),
            restitution: f.restitution,
            motors: keep(motors),
            kv: m.kv_rpm * std::f64::consts::TAU / 60.0,
            winding_resistance: m.resistance,
            no_load_current: m.no_load_current,
            rotor_inertia: m.rotor_inertia,
            max_current: m.max_current,
            idle: m.idle,
            prop_diameter: p.diameter,
            prop_pitch: p.pitch,
            thrust_coefficient: p.thrust_coefficient,
            torque_coefficient: p.torque_coefficient,
            edgewise_drag: p.edgewise_drag,
            strike: PropStrike { thickness: p.strike.thickness, graze_speed: p.strike.graze_speed, chip_depth: p.strike.chip_depth, stub: p.strike.stub, shake: p.strike.shake },
            wash: WashFit {
                onset: self.wash.onset,
                full: self.wash.full,
                clear_from: self.wash.clear_from,
                clear_to: self.wash.clear_to,
                thrust_loss: self.wash.thrust_loss,
                fluctuation: self.wash.fluctuation,
                buffet: self.wash.buffet,
                correlation_time: self.wash.correlation_time,
            },
            cells: b.cells,
            capacity: b.capacity_mah / 1000.0 * 3600.0,
            internal_resistance: b.internal_resistance,
            cell_voltage: keep(b.cell_voltage),
            controller: ControllerFit {
                rates: StickRates { rc_rate: v(k.rates.rc_rate), super_rate: v(k.rates.super_rate), expo: v(k.rates.expo) },
                p: v(k.p),
                i: v(k.i),
                d: v(k.d),
                feed_forward: v(k.feed_forward),
                i_limit: k.i_limit,
                angle_max: k.angle_max_deg * DEG,
                angle_gain: k.angle_gain,
            },
            air_height: self.start.air_height,
            mixer: [[0.0; 4]; MOST_MOTORS],
            tilt,
            turbine,
            stability: match k.stability {
                StabilityFile::Level => Stability::Level,
                StabilityFile::Rates => Stability::Rates,
                StabilityFile::Off => Stability::Off,
                StabilityFile::Hold => Stability::Hold,
            },
        };
        t.mixer = t.mixer_matrix();
        t
    }
}
