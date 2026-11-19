//! What the game does for a multirotor (`sim_core::multirotor`), any kind of it, from its type's
//! numbers: its FPV camera as the one spot of the view from it, how close the chase camera
//! follows, its keys and its sticks, the message in the middle of the screen and what its flight
//! controller's display shows (`ui::osd`). Nothing here knows a particular drone.
//!
//! Keys (`key`, `sticks`): W and S the throttle stick, the arrows the right stick (↑ forward, nose
//! down; ← → roll), A and D the yaw; R (or Enter) the arm switch. The keys move the sticks as springs do, back to the middle when let go; a controller's sticks are where they are
//! (`controls::Pads::sticks`). A tilt-rotor's nacelles' lever (`sim_core::Tilt`): F a notch
//! forward, G a notch back; C the help its flight controller gives (rates, level, none); Q and E
//! the pitch it holds (its stick tilts its nacelles). With turbines W and S are the collective
//! lever and Enter the engines (R is the nacelles back, as G: a controller's flap buttons press F
//! and R, its trim hat moves the pitch held, its centre button presses C). One with a cockpit (`MultirotorVisual::cockpit`) is seen from it as an
//! aircraft is, its instruments on its panel (`panel`, `rotorcraft`), and has that display only
//! from outside.

use crate::cockpit::instruments::Rotorcraft;
use crate::cockpit::Spot;
use crate::ui::panel::{Airframe, PanelData};
use crate::controls::Pads;
use crate::game::{Held, THROTTLE_RATE};
use crate::kinds::{self, Craft};
use crate::ui::osd::Osd;
use engine::KeyCode;
use glam::Vec3;
use sim_core::multirotor::Stability;
use sim_core::{Multirotor, MultirotorControls, Vehicle, World};

/// How fast a key moves its stick out, and back to the middle when let go, a second: out over
/// a quarter of a second to its travel, back at once.
const STICK_OUT: f64 = 2.5;
const STICK_BACK: f64 = 8.0;
/// How far a key moves its stick: a key is all or nothing, so not the whole travel a controller's
/// stick has. Roll and pitch to 0.65 (36° of bank), the yaw to 0.6 (some 130°/s): a turn on the
/// keys is a turn, not a flip. A controller's sticks keep their whole travel.
const KEY_TILT: f64 = 0.65;
const KEY_YAW: f64 = 0.6;
/// A cell's voltage under which its display warns, V.
const LOW_CELL: f64 = 3.5;
/// The rotors' speed (a share of the governed one) under which a turbine's display warns, and
/// the share of its fuel.
const LOW_ROTORS: f32 = 0.9;
const LOW_FUEL: f32 = 0.1;
/// A key moves a tilt-rotor's nacelles' lever by this much of its travel.
const TILT_NOTCH: f64 = 0.2;
/// How fast Q and E move the pitch a tilt-rotor holds, of its most a second.
const TRIM_RATE: f64 = 0.25;
/// The widest multirotor the chase camera follows as closely as it does a small quad, and the
/// size of the aircraft it follows from where it follows them, m.
const SMALL: f64 = 0.9;
const AIRCRAFT_SIZE: f64 = 11.0;

/// The places the eye can be: an aircraft's cockpit's spots, a multirotor's FPV camera (its
/// `visual.camera`: tilted up, its field of view across a 16:9 picture).
pub fn spots(craft: Craft) -> Vec<Spot> {
    match craft {
        Craft::Aircraft(kind) => kinds::spots(kind),
        // One with a cockpit: its cockpit's.
        Craft::Multirotor(kind) if kind.spec().visual.cockpit.is_some() => kinds::spots_in(kind.spec().visual.cockpit.expect("it has one")),
        Craft::Multirotor(kind) => {
            let camera = kind.spec().visual.camera;
            let p = camera.position;
            let fov_y = 2.0 * ((camera.fov / 2.0).tan() * 9.0 / 16.0).atan();
            vec![Spot { name: "FPV camera", eye: Vec3::new(p.x as f32, p.y as f32, p.z as f32), yaw: 0.0, pitch: camera.uptilt as f32, fov: fov_y as f32, shift: 0.0 }]
        }
    }
}

/// The size of a craft against an aircraft's, for the chase camera: how far it follows, how high
/// above the ground it keeps.
pub fn chase_scale(craft: Craft) -> f64 {
    match craft {
        // A light aircraft is the measure; a transport is several of them across.
        Craft::Aircraft(kind) => (kind.spec().span / AIRCRAFT_SIZE).max(1.0),
        Craft::Multirotor(kind) => (size(kind) / AIRCRAFT_SIZE).max(0.08),
    }
}

/// How long or wide a multirotor is, m: the longest side of its solid's boxes.
fn size(kind: sim_core::MultirotorKind) -> f64 {
    kind.spec().solid.iter().map(|(lo, hi)| (*hi - *lo).max_element()).fold(0.0, f64::max)
}

/// How far behind the turns of the craft the chase camera swings round, seconds (the time constant
/// it follows the heading with, `Session::follow`): a quad yaws 200°/s and more through a gate,
/// and a camera at its tail copying that swings round it in a jerk; an aircraft turns slowly and
/// the camera stays nearly behind it.
pub fn chase_lag(craft: Craft) -> f64 {
    match craft {
        Craft::Aircraft(_) => 0.2,
        Craft::Multirotor(kind) => if size(kind) > SMALL { 0.2 } else { 0.4 },
    }
}

/// How far above the horizon the chase camera starts, radians.
pub fn orbit_pitch(craft: Craft) -> f64 {
    match craft {
        Craft::Aircraft(_) => 0.18,
        Craft::Multirotor(_) => 0.25,
    }
}

/// A key of a multirotor's radio: the arm switch, and a tilt-rotor's nacelles' lever. What to
/// say, or `None` if the key is not one of them.
pub fn key(m: &mut Multirotor, key: KeyCode) -> Option<String> {
    let forward = m.p.tilt.as_ref().map(|t| t.forward);
    let turbines = m.p.turbine.is_some();
    let c = &mut m.controls;
    match (key, forward) {
        (KeyCode::KeyC, Some(_)) => {
            c.stability = match c.stability {
                Stability::Hold => Stability::Level,
                Stability::Level => Stability::Rates,
                Stability::Rates => Stability::Off,
                Stability::Off => Stability::Hold,
            };
            Some(match c.stability {
                Stability::Hold => "Stability: hold (the collective lever asks for a climb, its middle holds the height; the stick for a speed over the ground; let go, it stays)",
                Stability::Rates => "Stability: rates (the sticks ask for a rate of turn; let go, it stays as it is)",
                Stability::Level => "Stability: level (the sticks ask for a bank and a pitch; let go, it levels)",
                Stability::Off => "Stability: off (nothing helps: the sticks move the rotors)",
            }.to_string())
        }
        (KeyCode::Enter | KeyCode::NumpadEnter, _) if turbines => {
            c.arm = !c.arm;
            Some(if c.arm { "Engines: start (the collective down)" } else { "Engines: stopped" }.to_string())
        }
        // F and G, and R (what a controller's flap buttons press: F down, R up).
        (KeyCode::KeyF | KeyCode::KeyG | KeyCode::KeyR, Some(forward)) => {
            let step = if key == KeyCode::KeyF { TILT_NOTCH } else { -TILT_NOTCH };
            c.tilt = ((c.tilt + step) / TILT_NOTCH).round().clamp(0.0, 1.0 / TILT_NOTCH) * TILT_NOTCH;
            Some(format!("Nacelles {:.0}° forward", c.tilt * forward.to_degrees()))
        }
        _ => arm_key(c, key),
    }
}

fn arm_key(c: &mut MultirotorControls, key: KeyCode) -> Option<String> {
    match key {
        KeyCode::KeyR | KeyCode::Enter | KeyCode::NumpadEnter => {
            c.arm = !c.arm;
            Some(if c.arm { "Arm switch on" } else { "Arm switch off: disarmed" }.to_string())
        }
        _ => None,
    }
}

/// Moves the sticks for `dt` seconds of the keys held and the controllers: see the module.
pub fn sticks(c: &mut MultirotorControls, held: &Held, pads: &mut Pads, dt: f64) {
    let spring = |value: f64, key: f64| {
        let target = key;
        let rate = if key != 0.0 { STICK_OUT } else { STICK_BACK };
        value + (target - value).clamp(-rate * dt, rate * dt)
    };
    c.roll = spring(c.roll, held.roll * KEY_TILT);
    c.pitch = spring(c.pitch, held.pitch * KEY_TILT);
    c.yaw = spring(c.yaw, held.pedals * KEY_YAW);
    let up = held.throttle_up as i32 as f64 - held.throttle_down as i32 as f64;
    // The throttle stays where the keys leave it: it never moves by itself.
    c.throttle = (c.throttle + up * THROTTLE_RATE * dt).clamp(0.0, 1.0);
    // Q and E: the pitch a tilt-rotor flown level holds (a quad has no use for it).
    c.trim = (c.trim + held.fine * TRIM_RATE * dt).clamp(-1.0, 1.0);
    pads.sticks(c, dt);
}

/// What the dials of a multirotor's cockpit read (`ui::panel`): its speed through the air, its
/// height above the sea at the standard setting, its climb, its attitude and its heading, as they are.
pub fn panel(m: &Multirotor) -> PanelData {
    let e = sim_core::euler_angles(m.body.q);
    let turn = m.body.omega.dot(sim_core::act(m.body.q.conjugate(), sim_core::V3::Z));
    PanelData {
        airspeed_kt: (m.data().airspeed / sim_core::KNOTS) as f32,
        altitude_ft: (-m.body.pos.z / sim_core::FEET) as f32,
        vertical_speed_fpm: (-m.body.world_velocity().z / sim_core::FEET * 60.0) as f32,
        pitch: e.pitch as f32,
        roll: e.roll as f32,
        heading: Some(e.heading as f32),
        compass: e.heading as f32,
        altimeter_hpa: 1013.0,
        turn_rate: turn as f32,
        throttle: m.controls.throttle as f32,
        trim: m.controls.trim as f32,
        // The airspeed dial's marks: green to 150 kt, the red line at 170.
        airframe: Airframe { white: None, green: (0.0, 150.0), vne: 170.0, dial_top: 200.0, flaps: false, g_limits: (-1.0, 3.5), fan: None },
        load_factor: (-m.data().specific.z / 9.81) as f32,
        ..PanelData::default()
    }
}

/// What a turbine tilt-rotor's own instruments read (`cockpit::instruments::Rotorcraft`), if it is one.
pub fn rotorcraft(m: &Multirotor, vehicle: &Vehicle) -> Option<Rotorcraft> {
    let (turbine, tilt) = (m.p.turbine?, m.p.tilt.as_ref()?);
    let v = m.body.world_velocity();
    let home = vehicle.home.map(|a| {
        let (north, east) = (a.n - m.body.pos.x, a.e - m.body.pos.y);
        (east.atan2(north) as f32, north.hypot(east) as f32, a.heading as f32)
    });
    Some(Rotorcraft {
        fuel_kg: m.engines().fuel as f32,
        endurance_min: (m.engines().fuel / m.engines().flow.max(1e-3) / 60.0).min(599.0) as f32,
        agl_ft: (vehicle.agl() / sim_core::FEET) as f32,
        ground_speed_kt: (v.x.hypot(v.y) / sim_core::KNOTS) as f32,
        track: if v.x.hypot(v.y) > 1.0 { v.y.atan2(v.x) as f32 } else { sim_core::euler_angles(m.body.q).heading as f32 },
        home,
        torque: m.torque() as f32,
        rotors: (m.rotor_speeds()[0] / turbine.rotor_speed) as f32,
        engines: m.engines().running,
        fuel: (m.engines().fuel / turbine.fuel) as f32,
        nacelles: ((m.nacelles()[0] + m.nacelles()[1]) / 2.0).to_degrees() as f32,
        nacelles_back: tilt.back.to_degrees() as f32,
        nacelles_forward: tilt.forward.to_degrees() as f32,
        stability: match m.controls.stability {
            Stability::Hold => 3,
            Stability::Rates => 0,
            Stability::Level => 1,
            Stability::Off => 2,
        },
    })
}

/// The message in the middle of the screen: why it does not arm.
pub fn message(m: Option<&Multirotor>) -> String {
    let Some(m) = m else { return String::new() };
    match m.arming_blocked() {
        Some("THROTTLE") if m.p.turbine.is_some() => "The engines do not start: the collective must be down (S)".to_string(),
        Some(why) if m.p.turbine.is_some() => format!("The engines do not start: {why}"),
        Some("THROTTLE") => "Not armed: the throttle stick must be down (S)".to_string(),
        Some("ANGLE") => "Not armed: it is tilted (1: on the ground here)".to_string(),
        Some(why) => format!("Not armed: {why}"),
        None => String::new(),
    }
}

/// What the flight controller's display shows now. `fpv`: seen through its camera (the view's
/// centre is the camera's axis).
pub fn osd(m: &Multirotor, vehicle: &Vehicle, world: &World, fpv: bool) -> Osd {
    let body = m.body;
    let e = sim_core::euler_angles(body.q);
    let v = body.world_velocity();
    let battery = m.battery();
    let cells = m.p.cells as f64;
    let engines = m.p.turbine.map(|t| ((m.rotor_speeds()[0] / t.rotor_speed) as f32, m.torque() as f32, m.engines().fuel as f32, t.fuel as f32));
    let warning = if m.crashed().is_some() {
        Some("CRASH")
    } else if let Some((rotors, _, fuel, full)) = engines {
        if rotors < LOW_ROTORS && (m.armed() || !m.data().on_ground) {
            Some("LOW ROTOR RPM")
        } else if fuel < LOW_FUEL * full {
            Some("LOW FUEL")
        } else {
            None
        }
    } else if battery.voltage / cells < LOW_CELL && m.armed() {
        Some("LOW BATTERY")
    } else {
        None
    };
    Osd {
        armed: m.armed(),
        blocked: m.arming_blocked(),
        volts: battery.voltage as f32,
        cell: (battery.voltage / cells) as f32,
        amps: battery.current as f32,
        used_mah: (battery.used / 3.6) as f32,
        altitude: (-body.pos.z - m.armed_altitude()) as f32,
        agl: vehicle.agl() as f32,
        speed_kmh: (v.length() * 3.6) as f32,
        climb: (-v.z) as f32,
        throttle: m.controls.throttle as f32,
        time: m.armed_time() as f32,
        pitch: e.pitch as f32,
        roll: e.roll as f32,
        heading: e.heading as f32,
        uptilt: if fpv { m.p.visual.camera.uptilt as f32 } else { 0.0 },
        fov: m.p.visual.camera.fov as f32,
        nacelles: m.p.tilt.as_ref().map(|_| ((m.nacelles()[0] + m.nacelles()[1]) / 2.0) as f32),
        engines: engines.map(|(rotors, torque, fuel, _)| (rotors, torque, fuel)),
        stability: m.p.tilt.as_ref().map(|_| match m.controls.stability {
            Stability::Hold => "HOLD",
            Stability::Rates => "RATES",
            Stability::Level => "LEVEL",
            Stability::Off => "OFF",
        }),
        horizon: fpv,
        warning,
        clock: world.hour as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{MultirotorKind, Start};

    /// The keys: it starts armed and R disarms it, F does nothing; the sticks spring back.
    #[test]
    fn the_keys_arm_it() {
        let mut world = World::new(1);
        let id = world.add_multirotor(MultirotorKind::FPV5, Start::Home);
        let m = world.vehicle_mut(id).multirotor_mut().unwrap();
        assert!(m.controls.arm && key(m, KeyCode::KeyR).is_some() && !m.controls.arm);
        let before = m.controls;
        assert!(key(m, KeyCode::KeyF).is_none() && m.controls == before);
        assert!(key(m, KeyCode::KeyW).is_none());
        let mut pads = Pads::new(crate::controls::Mappings::memory());
        let mut c = MultirotorControls::default();
        let held = Held { roll: 1.0, throttle_up: true, ..Held::default() };
        for _ in 0..60 {
            sticks(&mut c, &held, &mut pads, 1.0 / 60.0);
        }
        assert!((c.roll - KEY_TILT).abs() < 1e-9 && c.throttle > 0.4);
        let held = Held { pedals: 1.0, ..Held::default() };
        for _ in 0..30 {
            sticks(&mut c, &held, &mut pads, 1.0 / 60.0);
        }
        assert!((c.yaw - KEY_YAW).abs() < 1e-9);
        for _ in 0..30 {
            sticks(&mut c, &Held::default(), &mut pads, 1.0 / 60.0);
        }
        assert!(c.roll.abs() < 1e-9);
    }

    /// The FPV camera looks up by its tilt, across 120° of a 16:9 picture.
    #[test]
    fn the_fpv_camera_is_the_type_s() {
        let spot = spots(Craft::Multirotor(MultirotorKind::FPV5))[0];
        assert!((spot.pitch.to_degrees() - 25.0).abs() < 1e-3);
        let across = 2.0 * ((spot.fov / 2.0).tan() * 16.0 / 9.0).atan();
        assert!((across.to_degrees() - 120.0).abs() < 0.01, "{}", across.to_degrees());
    }

    /// A tilt-rotor: F and G move its nacelles' lever by notches, within its travel; the chase
    /// camera follows it from as far as its size asks, a quad from close.
    #[test]
    fn a_tilt_rotor_s_lever_and_size() {
        let kind = MultirotorKind::parse("kestrel").unwrap();
        let mut world = World::new(1);
        let id = world.add_multirotor(kind, Start::Home);
        let m = world.vehicle_mut(id).multirotor_mut().unwrap();
        assert_eq!(key(m, KeyCode::KeyF).as_deref(), Some("Nacelles 12° forward"));
        for _ in 0..6 {
            key(m, KeyCode::KeyF);
        }
        assert!((m.controls.tilt - 1.0).abs() < 1e-9);
        assert_eq!(key(m, KeyCode::KeyG).as_deref(), Some("Nacelles 48° forward"));
        // R is the nacelles back too (a controller's flaps-up button); Enter, the engines.
        assert_eq!(key(m, KeyCode::KeyR).as_deref(), Some("Nacelles 36° forward"));
        assert!(m.controls.arm && key(m, KeyCode::Enter).as_deref() == Some("Engines: stopped") && !m.controls.arm);
        // C: the help, from the hold it starts with to level, rates, none, and round.
        assert_eq!(m.controls.stability, Stability::Hold);
        for expected in [Stability::Level, Stability::Rates, Stability::Off, Stability::Hold] {
            assert!(key(m, KeyCode::KeyC).is_some());
            assert_eq!(m.controls.stability, expected);
        }
        assert!(chase_scale(Craft::Multirotor(kind)) > 0.7 && chase_lag(Craft::Multirotor(kind)) == 0.2);
        assert_eq!(chase_scale(Craft::Multirotor(MultirotorKind::FPV5)), 0.08);
        // It has a cockpit: the eye's places are the cockpit's, the pilot's seat first.
        assert_eq!(spots(Craft::Multirotor(kind))[0].name, "pilot");
        assert!(panel(m).heading.is_some());
        let shown = rotorcraft(world.vehicle(id).multirotor().unwrap(), world.vehicle(id)).unwrap();
        assert!((shown.nacelles_forward - 60.0).abs() < 1e-3 && shown.home.is_some() && shown.fuel_kg > 700.0);
    }
}
