//! The Kestrel tilt-rotor (`assets/aircraft/kestrel/multirotor.ron`): that it hovers where its
//! numbers say, holds itself level, turns on the spot, goes from the hover to flying forward on
//! its nacelles and back, turns with its bank at speed, and that a lifting surface lifts, stalls
//! and drags as a wing does. `cargo test -p sim-core --test tiltrotor -- --nocapture` prints the
//! measurements.

use sim_core::multirotor::Surface;
use sim_core::multirotor::Stability;
use sim_core::{euler_angles, Controls, MultirotorControls, MultirotorKind, MultirotorType, Observation, Pilot, Start, VehicleId, World, DEG, V3};

fn kind() -> MultirotorKind {
    MultirotorKind::parse("kestrel").expect("the Kestrel is in the fleet")
}

fn spec() -> MultirotorType {
    MultirotorType::of(kind())
}

/// A world with it hovering over a runway, still air.
fn hovering() -> (World, VehicleId) {
    let mut world = World::new(3);
    let a = world.terrain.nearest_airport(0.0, 0.0).expect("an aerodrome");
    let id = world.add_multirotor(kind(), Start::Air { n: a.n, e: a.e, heading: 0.4 });
    (world, id)
}

/// Holds a height with the throttle, the other sticks and the lever as they are set; with a
/// `cruise` throttle set, holds it with the pitch stick at that throttle, as an aeroplane's pilot.
struct Flier {
    stability: Stability,
    cruise: Option<f64>,
    height: f64,
    integral: f64,
    hover: f64,
    roll: f64,
    /// The pitch it trims (of the controller's most), and the pitch stick: the nacelles'.
    pitch: f64,
    stick: f64,
    yaw: f64,
    tilt: f64,
}

impl Flier {
    fn at(world: &World, id: VehicleId) -> Flier {
        let m = world.vehicle(id).multirotor().unwrap();
        Flier { stability: Stability::Level, cruise: None, height: -m.body.pos.z, integral: 0.0, hover: m.controls.throttle, roll: 0.0, pitch: 0.0, stick: 0.0, yaw: 0.0, tilt: 0.0 }
    }
}

impl Pilot for Flier {
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls {
        let Controls::Multirotor(_) = current else { return *current };
        let m = obs.multirotor().expect("a multirotor's observation");
        let climb = (0.6 * (self.height - m.altitude)).clamp(-4.0, 4.0) - m.vertical_speed;
        if let Some(throttle) = self.cruise {
            self.integral = (self.integral + 0.02 * climb * dt).clamp(-0.4, 0.4);
            let pitch = (self.integral + 0.03 * climb).clamp(-0.5, 0.5);
            return Controls::Multirotor(MultirotorControls { throttle, roll: self.roll, pitch: self.stick, yaw: self.yaw, arm: true, tilt: self.tilt, stability: self.stability, trim: pitch });
        }
        self.integral = (self.integral + 0.03 * climb * dt).clamp(-0.4, 0.4);
        let throttle = (self.hover + self.integral + 0.08 * climb).clamp(0.0, 1.0);
        Controls::Multirotor(MultirotorControls { throttle, roll: self.roll, pitch: self.stick, yaw: self.yaw, arm: true, tilt: self.tilt, stability: self.stability, trim: self.pitch })
    }
}

fn fly(world: &mut World, id: VehicleId, pilot: &mut Flier, seconds: f64) {
    for _ in 0..(seconds / World::DT).round() as usize {
        let obs = world.observe(id);
        let controls = pilot.fly(&obs, &world.vehicle(id).dynamics.controls(), World::DT);
        world.vehicle_mut(id).dynamics.set_controls(controls);
        world.step();
    }
}

#[test]
fn its_numbers_hold_together() {
    let p = spec();
    let tilt = p.tilt.as_ref().expect("nacelles");
    let turbine = p.turbine.expect("turbines");
    assert_eq!(p.motors.iter().map(|m| m.nacelle).collect::<Vec<_>>(), [Some(0), Some(0), Some(1), Some(1)]);
    assert_eq!((tilt.surfaces.len(), turbine.engines, p.stability), (3, 2, Stability::Hold));
    let (hover, ratio) = (p.hover_throttle(1.225), p.thrust_to_weight());
    println!("hover collective {hover:.2}, thrust to weight {ratio:.2}");
    assert!((0.42..0.55).contains(&hover), "{hover}");
    assert!((1.4..1.7).contains(&ratio), "{ratio}");
    // The rotors of each ring turn against each other, and the rings mirror each other.
    let spin: Vec<f64> = p.motors.iter().map(|m| m.spin.sign()).collect();
    assert_eq!(spin, [1.0, -1.0, -1.0, 1.0]);
}

#[test]
fn it_hovers_level_where_it_is() {
    let (mut world, id) = hovering();
    let start = world.vehicle(id).body().pos;
    let mut pilot = Flier::at(&world, id);
    // With no hand on the sticks it stays as it is, in each kind of help but none.
    for stability in [Stability::Rates, Stability::Level] {
        pilot.stability = stability;
        fly(&mut world, id, &mut pilot, 10.0);
    }
    let m = world.vehicle(id).multirotor().unwrap();
    let e = euler_angles(m.body.q);
    let moved = m.body.pos - start;
    let power = m.engines().power / 1000.0;
    let rotors = m.rotor_speeds()[0] / spec().turbine.unwrap().rotor_speed;
    println!("after 20 s: moved {moved:.2}, pitch {:.2}°, roll {:.2}°, heading {:.2}°, {power:.0} kW (torque {:.0} %), rotors {:.1} %, fuel flow {:.0} kg/h", e.pitch / DEG, e.roll / DEG, e.heading / DEG, m.torque() * 100.0, rotors * 100.0, m.engines().flow * 3600.0);
    assert!(m.crashed().is_none());
    assert!(moved.length() < 3.0, "{moved}");
    assert!(e.pitch.abs() < 1.0 * DEG && e.roll.abs() < 1.0 * DEG && (e.heading - 0.4).abs() < 1.0 * DEG);
    // Some 1.28 MW to hover (the file's sums), the governor holding the rotors' speed.
    assert!((1150.0..1400.0).contains(&power), "{power}");
    assert!((rotors - 1.0).abs() < 0.005, "{rotors}");
}

#[test]
fn it_turns_on_the_spot_and_leans_where_the_stick_says() {
    let (mut world, id) = hovering();
    let mut pilot = Flier::at(&world, id);
    pilot.yaw = 1.0;
    fly(&mut world, id, &mut pilot, 6.0);
    let m = world.vehicle(id).multirotor().unwrap();
    let rate = -m.body.omega.dot(sim_core::act(m.body.q.conjugate(), V3::NEG_Z));
    println!("yaw rate at full stick {:.1}°/s, nacelles {:?}", rate / DEG, m.nacelles());
    assert!((55.0 * DEG..65.0 * DEG).contains(&rate), "{}", rate / DEG);
    // The right nacelle back and the left forward turn it right.
    assert!(m.nacelles()[0] > m.nacelles()[1]);

    pilot.yaw = 0.0;
    pilot.roll = 0.5;
    pilot.pitch = -0.3;
    fly(&mut world, id, &mut pilot, 4.0);
    let e = euler_angles(world.vehicle(id).body().q);
    let max = spec().controller.angle_max;
    println!("roll {:.1}° of {:.1}°, pitch {:.1}° of {:.1}°", e.roll / DEG, 0.5 * max / DEG, e.pitch / DEG, -0.3 * max / DEG);
    assert!((e.roll - 0.5 * max).abs() < 3.0 * DEG && (e.pitch + 0.3 * max).abs() < 3.0 * DEG);
    assert!(world.vehicle(id).multirotor().unwrap().crashed().is_none());
}

#[test]
fn it_flies_forward_on_its_nacelles_and_comes_back_to_the_hover() {
    let (mut world, id) = hovering();
    let mut pilot = Flier::at(&world, id);
    let height = pilot.height;
    let mut worst: f64 = 0.0;
    // The lever forward over twenty seconds, then held.
    for step in 0..50 {
        pilot.tilt = (step as f64 / 40.0).min(1.0);
        pilot.pitch = 0.15 * pilot.tilt;
        // Past half the lever it is flown as an aeroplane: the collective set, the height held
        // with the stick (with the collective alone the rotors cannot carry it tilted so far).
        if pilot.tilt > 0.5 && pilot.cruise.is_none() {
            pilot.cruise = Some(0.6);
            pilot.integral = pilot.pitch;
        }
        fly(&mut world, id, &mut pilot, 0.5);
        worst = worst.max((-world.vehicle(id).body().pos.z - height).abs());
    }
    fly(&mut world, id, &mut pilot, 60.0);
    let m = world.vehicle(id).multirotor().unwrap();
    let e = euler_angles(m.body.q);
    let speed = m.data().airspeed;
    let power = m.engines().power / 1000.0;
    println!("forward: {speed:.0} m/s, pitch {:.1}°, height off by {:.1} m (worst {worst:.1} in the transition), throttle {:.2}, {power:.0} kW, nacelles {:?}, elevator {:.1}°", e.pitch / DEG, -m.body.pos.z - height, m.controls.throttle, m.nacelles().map(|a| a / DEG), m.elevator() / DEG);
    assert!(m.crashed().is_none());
    assert!((55.0..110.0).contains(&speed), "{speed}");
    assert!((-m.body.pos.z - height).abs() < 15.0 && worst < 25.0);
    assert!(e.roll.abs() < 2.0 * DEG && (e.heading - 0.4).abs() < 3.0 * DEG);
    // It has no wing to speak of: the rotors carry it, and so fast takes most of its engines.
    assert!((1500.0..2250.0).contains(&power), "{power}");

    // A turn: banked 25°, it turns as a balanced turn at that bank and speed does.
    pilot.roll = 25.0 * DEG / spec().controller.angle_max;
    fly(&mut world, id, &mut pilot, 6.0);
    let before = euler_angles(world.vehicle(id).body().q).heading;
    fly(&mut world, id, &mut pilot, 4.0);
    let m = world.vehicle(id).multirotor().unwrap();
    let turned = sim_core::remainder(euler_angles(m.body.q).heading - before, std::f64::consts::TAU) / 4.0;
    let balanced = 9.81 * (25.0 * DEG).tan() / m.data().airspeed;
    let slip = (m.body.vel.y / m.body.vel.x).atan();
    println!("turn: {:.1}°/s (balanced {:.1}°/s), bank {:.1}°, slip {:.1}°, {:.0} m/s", turned / DEG, balanced / DEG, euler_angles(m.body.q).roll / DEG, slip / DEG, m.data().airspeed);
    assert!((turned / balanced - 1.0).abs() < 0.25, "{turned} against {balanced}");
    assert!(slip.abs() < 6.0 * DEG);

    // Back to the hover.
    pilot.roll = 0.0;
    pilot.cruise = None;
    pilot.integral = 0.0;
    // The lever back over fifteen seconds, and the nose up against the speed left, as a
    // helicopter is slowed.
    for step in 0..90 {
        pilot.tilt = (1.0 - step as f64 / 30.0).max(0.0);
        pilot.pitch = (0.04 * world.vehicle(id).body().vel.x).clamp(0.0, 0.5);
        fly(&mut world, id, &mut pilot, 0.5);
    }
    let m = world.vehicle(id).multirotor().unwrap();
    println!("back in the hover: {:.1} m/s, height off by {:.1} m", m.data().airspeed, -m.body.pos.z - height);
    assert!(m.crashed().is_none() && m.data().airspeed < 3.0 && (-m.body.pos.z - height).abs() < 15.0, "{}", m.data().airspeed);
}

#[test]
fn it_lifts_off_its_skids_and_lands_on_them() {
    let mut world = World::new(3);
    let id = world.add_multirotor(kind(), Start::Home);
    for _ in 0..480 {
        world.step();
    }
    let m = world.vehicle(id).multirotor().unwrap();
    assert!(m.armed() && m.data().on_ground && m.crashed().is_none());
    assert!(euler_angles(m.body.q).pitch.abs() < 0.5 * DEG && m.body.vel.length() < 0.05, "{}", m.body.vel);
    let ground = -m.body.pos.z;
    let mut pilot = Flier::at(&world, id);
    pilot.hover = spec().hover_throttle(1.225);
    pilot.height = ground + 10.0;
    fly(&mut world, id, &mut pilot, 12.0);
    assert!((-world.vehicle(id).body().pos.z - ground - 10.0).abs() < 2.0);
    pilot.height = ground - 3.0;
    fly(&mut world, id, &mut pilot, 15.0);
    let m = world.vehicle(id).multirotor().unwrap();
    println!("landed: on the ground {}, crashed {:?}", m.data().on_ground, m.crashed());
    assert!(m.data().on_ground && m.crashed().is_none());
}

#[test]
fn a_surface_lifts_stalls_and_drags_as_a_wing() {
    let wing = Surface { position: V3::ZERO, area: 10.0, aspect: 6.0, upright: false, incidence: 0.0, stall: 15.0 * DEG, drag: 0.01, elevator: Some((0.5, 25.0 * DEG)) };
    let (rho, speed) = (1.225, 50.0);
    let q = 0.5 * rho * speed * speed * wing.area;
    let at = |alpha: f64, deflection: f64| {
        let f = wing.force(V3::new(speed * alpha.cos(), 0.0, speed * alpha.sin()), rho, deflection);
        // Lift across the air's way and drag along it.
        ((-f.z * alpha.cos() + f.x * alpha.sin()) / q, (-f.x * alpha.cos() - f.z * alpha.sin()) / q)
    };
    let slope = std::f64::consts::TAU * 6.0 / 8.0;
    let (lift, drag) = at(5.0 * DEG, 0.0);
    assert!((lift - slope * 5.0 * DEG).abs() < 1e-9 && (drag - 0.01 - lift * lift / (std::f64::consts::PI * 0.8 * 6.0)).abs() < 1e-9);
    assert!(at(0.0, 0.0).0.abs() < 1e-12 && (at(-5.0 * DEG, 0.0).0 + lift).abs() < 1e-9);
    // The elevator down adds half its angle; past the stall it lifts less and drags much more.
    assert!((at(0.0, 10.0 * DEG).0 - slope * 5.0 * DEG).abs() < 1e-9);
    assert!(at(30.0 * DEG, 0.0).0 < at(14.0 * DEG, 0.0).0 && at(30.0 * DEG, 0.0).1 > 0.4);
    // Flat to the air: no lift, a plate's drag. From behind: drag still against the motion.
    let (lift, drag) = at(90.0 * DEG, 0.0);
    assert!(lift.abs() < 1e-9 && (drag - 2.01).abs() < 1e-9);
    assert!(wing.force(V3::new(-speed, 0.0, 0.0), rho, 0.0).x > 0.0);
    // A fin lifts sideways, against the slip.
    let fin = Surface { upright: true, elevator: None, ..wing };
    let f = fin.force(V3::new(speed, 5.0, 0.0), rho, 0.0);
    assert!(f.y < 0.0 && f.z == 0.0);
}

fn set(world: &mut World, id: VehicleId, change: impl FnOnce(&mut MultirotorControls)) {
    change(&mut world.vehicle_mut(id).multirotor_mut().unwrap().controls);
}

fn run(world: &mut World, seconds: f64) {
    for _ in 0..(seconds / World::DT).round() as usize {
        world.step();
    }
}

#[test]
fn the_sticks_ask_for_rates_and_it_stays_as_it_is_left() {
    let (mut world, id) = hovering();
    set(&mut world, id, |c| {
        c.stability = Stability::Rates;
        c.throttle = spec().hover_throttle(1.225);
        c.roll = 0.5;
    });
    run(&mut world, 1.0);
    let rate = world.vehicle(id).body().omega.x;
    set(&mut world, id, |c| c.roll = 0.0);
    run(&mut world, 1.0);
    let bank = euler_angles(world.vehicle(id).body().q).roll;
    run(&mut world, 3.0);
    let later = euler_angles(world.vehicle(id).body().q).roll;
    println!("half stick: {:.1}°/s; let go at {:.1}° of bank, {:.1}° three seconds later", rate / DEG, bank / DEG, later / DEG);
    // Half the stick is 30°/s; let go, the bank stays (levelling itself it would be gone).
    assert!((rate - 30.0 * DEG).abs() < 4.0 * DEG, "{}", rate / DEG);
    assert!(bank > 15.0 * DEG && (later - bank).abs() < 2.0 * DEG);
}

#[test]
fn with_no_help_the_sticks_turn_it_and_nothing_stops_it() {
    let (mut world, id) = hovering();
    set(&mut world, id, |c| {
        c.stability = Stability::Off;
        c.roll = 0.5;
    });
    run(&mut world, 0.5);
    let rate = world.vehicle(id).body().omega.x;
    set(&mut world, id, |c| c.roll = 0.0);
    run(&mut world, 1.0);
    let after = world.vehicle(id).body().omega.x;
    let direct = spec().tilt.unwrap().direct.x;
    println!("half stick for half a second: {:.1}°/s ({:.1} asked); a second after letting go, {:.1}°/s", rate / DEG, 0.25 * direct / DEG, after / DEG);
    // Half of 1.5 rad/s² for half a second, less what the rotors' own damping takes (the side
    // going down pushes more); let go, it goes on rolling, slowing by that alone.
    assert!((rate / (0.25 * direct) - 1.0).abs() < 0.3, "{rate}");
    assert!(after > 0.4 * rate);
}

#[test]
fn the_engines_start_stop_and_run_out_of_breath() {
    let mut world = World::new(3);
    let id = world.add_multirotor(kind(), Start::Home);
    let speed = spec().turbine.unwrap().rotor_speed;
    let share = |world: &World| world.vehicle(id).multirotor().unwrap().rotor_speeds()[0] / speed;
    run(&mut world, 3.0);
    let idle = world.vehicle(id).multirotor().unwrap().torque();
    assert!((share(&world) - 1.0).abs() < 0.01 && (0.05..0.2).contains(&idle), "{} {idle}", share(&world));
    // The switch off: the engines stop and the rotors run down.
    set(&mut world, id, |c| c.arm = false);
    run(&mut world, 30.0);
    let down = share(&world);
    assert!(!world.vehicle(id).multirotor().unwrap().engines().running && down < 0.5, "{down}");
    // On again: lit, and up to speed.
    set(&mut world, id, |c| c.arm = true);
    let mut took = 0.0;
    while share(&world) < 0.99 && took < 60.0 {
        run(&mut world, 0.5);
        took += 0.5;
    }
    println!("flat pitch: torque {:.0} %; thirty seconds stopped: rotors at {:.0} %; started again: {took:.1} s to 99 %", idle * 100.0, down * 100.0);
    assert!((10.0..30.0).contains(&took), "{took}");

    // In the hover, all the collective at once is more than the engines can turn: they give all
    // they have and the rotors slow down.
    let (mut world, id) = hovering();
    set(&mut world, id, |c| {
        c.stability = Stability::Level;
        c.throttle = 1.0;
    });
    run(&mut world, 4.0);
    let m = world.vehicle(id).multirotor().unwrap();
    println!("full collective in the hover: torque {:.0} %, rotors {:.0} %, climbing {:.1} m/s", m.torque() * 100.0, share(&world) * 100.0, -m.body.world_velocity().z);
    assert!(m.torque() > 0.95 && share(&world) < 0.97 && -m.body.world_velocity().z > 3.0);
}

#[test]
fn with_the_engines_out_the_air_turns_its_rotors() {
    let (mut world, id) = hovering();
    let speed = spec().turbine.unwrap().rotor_speed;
    // The engines cut in the hover and the collective lowered: it comes down level, flown still,
    // and the air coming up through the discs keeps the rotors turning.
    set(&mut world, id, |c| {
        c.stability = Stability::Level;
        c.arm = false;
        c.throttle = 0.12;
    });
    run(&mut world, 1.5);
    let early = world.vehicle(id).multirotor().unwrap().data().thrust;
    run(&mut world, 2.0);
    let m = world.vehicle(id).multirotor().unwrap();
    let (sink, rotors, e) = (m.body.world_velocity().z, m.rotor_speeds()[0] / speed, euler_angles(m.body.q));
    let weight = spec().mass * 9.81;
    println!("engines out for 3.5 s: sinking {sink:.1} m/s, rotors {:.0} %, thrust {:.0} % of its weight (after 1.5 s, {:.0} %)", rotors * 100.0, m.data().thrust / weight * 100.0, early / weight * 100.0);
    assert!(!m.engines().running && m.engines().power == 0.0 && m.crashed().is_none());
    assert!(rotors > 0.85, "{rotors}");
    assert!((12.0..25.0).contains(&sink) && m.data().thrust > early && m.data().thrust > 0.25 * weight);
    assert!(e.pitch.abs() < 12.0 * DEG && e.roll.abs() < 5.0 * DEG, "{} {}", e.pitch / DEG, e.roll / DEG);
    // With the engines turning them again it holds itself there.
    set(&mut world, id, |c| c.arm = true);
    assert_eq!(world.vehicle(id).multirotor().unwrap().arming_blocked(), None);
}

#[test]
fn the_stick_tilts_its_rings_and_it_goes_forward_level() {
    for stability in [Stability::Rates, Stability::Level] {
        let (mut world, id) = hovering();
        let mut pilot = Flier::at(&world, id);
        pilot.stability = stability;
        // Half the stick forward: both nacelles 10° forward, and it gathers speed with its nose
        // where it was.
        pilot.stick = -0.5;
        let mut worst: f64 = 0.0;
        for _ in 0..16 {
            fly(&mut world, id, &mut pilot, 0.5);
            worst = worst.max(euler_angles(world.vehicle(id).body().q).pitch.abs());
        }
        let m = world.vehicle(id).multirotor().unwrap();
        let (speed, nacelles) = (m.body.vel.x, m.nacelles());
        // Let go, it coasts; the stick back, the rings behind upright, it stops.
        pilot.stick = 0.6;
        let mut took = 0.0;
        while world.vehicle(id).body().vel.x > 0.5 && took < 30.0 {
            fly(&mut world, id, &mut pilot, 0.25);
            took += 0.25;
            worst = worst.max(euler_angles(world.vehicle(id).body().q).pitch.abs());
        }
        let back = world.vehicle(id).multirotor().unwrap().nacelles();
        println!("{stability:?}: 8 s of half stick: {speed:.1} m/s, nacelles {:.1}°; stopped in {took:.1} s with them at {:.1}°; pitch never past {:.1}°", nacelles[0] / DEG, back[0] / DEG, worst / DEG);
        assert!((nacelles[0] / DEG - 10.0).abs() < 0.5 && (nacelles[1] / DEG - 10.0).abs() < 0.5);
        assert!((10.0..16.0).contains(&speed), "{speed}");
        assert!((back[0] / DEG + 12.0).abs() < 0.5 && took < 12.0);
        assert!(worst < 3.0 * DEG, "{}", worst / DEG);
        // The trim raises its nose, and it holds it.
        pilot.stick = 0.0;
        pilot.pitch = 0.2;
        fly(&mut world, id, &mut pilot, 5.0);
        let pitch = euler_angles(world.vehicle(id).body().q).pitch;
        assert!((pitch - 0.2 * spec().controller.angle_max).abs() < 1.0 * DEG, "{}", pitch / DEG);
    }
}

#[test]
fn it_holds_itself_still_and_goes_where_the_levers_say() {
    let mut world = World::new(3);
    // A wind across it, gusting.
    world.weather.wind_speed = 5.0;
    world.weather.turbulence = 0.3;
    let a = world.terrain.nearest_airport(0.0, 0.0).expect("an aerodrome");
    let id = world.add_multirotor(kind(), Start::Air { n: a.n, e: a.e, heading: 0.4 });
    // It starts holding itself, the collective lever in the middle.
    let c = world.vehicle(id).multirotor().unwrap().controls;
    assert_eq!((c.stability, c.throttle), (Stability::Hold, 0.5));
    let start = world.vehicle(id).body().pos;
    run(&mut world, 30.0);
    let m = world.vehicle(id).multirotor().unwrap();
    let moved = m.body.pos - start;
    println!("hands off for 30 s in 5 m/s of wind: moved {:.1} m across and {:.1} m up, going {:.2} m/s", moved.x.hypot(moved.y), -moved.z, m.body.world_velocity().length());
    assert!(moved.x.hypot(moved.y) < 6.0 && moved.z.abs() < 1.5 && m.body.world_velocity().length() < 0.8);

    // The collective lever up: it climbs at what it asks for; back in the middle, it stops.
    set(&mut world, id, |c| c.throttle = 0.8);
    run(&mut world, 8.0);
    let climb = -world.vehicle(id).body().world_velocity().z;
    set(&mut world, id, |c| c.throttle = 0.5);
    run(&mut world, 8.0);
    let after = -world.vehicle(id).body().world_velocity().z;
    println!("lever at 80 %: climbing {climb:.1} m/s; back in the middle: {after:.2} m/s");
    // 80 % is half of what is left over the middle's dead band: 4.4 m/s.
    assert!((climb - 4.44).abs() < 0.5 && after.abs() < 0.3, "{climb} {after}");

    // Half the stick forward: 7.5 m/s over the ground; half to the right: 4 m/s that way. Let
    // go, it stops and stays.
    let over_ground = |world: &World| {
        let b = world.vehicle(id).body();
        let (v, h) = (b.world_velocity(), euler_angles(b.q).heading);
        (v.x * h.cos() + v.y * h.sin(), v.y * h.cos() - v.x * h.sin())
    };
    set(&mut world, id, |c| c.pitch = -0.5);
    run(&mut world, 12.0);
    let forward = over_ground(&world);
    set(&mut world, id, |c| (c.pitch, c.roll) = (0.0, 0.5));
    run(&mut world, 12.0);
    let sideways = over_ground(&world);
    set(&mut world, id, |c| c.roll = 0.0);
    run(&mut world, 12.0);
    let stopped = over_ground(&world);
    let e = euler_angles(world.vehicle(id).body().q);
    println!("half stick forward: {:.1} m/s ({:.1} sideways); to the right: {:.1} m/s ({:.1} forward); let go: {:.2}, {:.2} m/s, pitch {:.1}°", forward.0, forward.1, sideways.1, sideways.0, stopped.0, stopped.1, e.pitch / DEG);
    assert!((forward.0 - 7.5).abs() < 0.8 && forward.1.abs() < 0.8);
    assert!((sideways.1 - 4.0).abs() < 0.6 && sideways.0.abs() < 0.8);
    assert!(stopped.0.abs() < 0.5 && stopped.1.abs() < 0.5 && e.pitch.abs() < 2.0 * DEG);

    // The lever at its foot: it comes down at no more than it asks, onto its skids, the blades flat.
    set(&mut world, id, |c| c.throttle = 0.25);
    let mut hardest: f64 = 0.0;
    for _ in 0..(120.0 / 0.25) as usize {
        run(&mut world, 0.25);
        let m = world.vehicle(id).multirotor().unwrap();
        if m.data().on_ground {
            break;
        }
        hardest = hardest.max(m.body.world_velocity().z);
    }
    set(&mut world, id, |c| c.throttle = 0.0);
    run(&mut world, 3.0);
    let m = world.vehicle(id).multirotor().unwrap();
    println!("lever at 25 %: down at {hardest:.1} m/s at most; on the ground {}, crashed {:?}, torque {:.0} %", m.data().on_ground, m.crashed(), m.torque() * 100.0);
    assert!(m.data().on_ground && m.crashed().is_none() && hardest < 3.6 && m.torque() < 0.2);
}
