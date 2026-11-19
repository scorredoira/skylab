//! The 5-inch FPV quad (`assets/aircraft/fpv5/multirotor.ron`) against how such quads fly: the
//! throttle it hovers at, its thrust to weight, how fast it climbs and flies, which way its
//! motors' torque yaws it, its flight controller's rates and self-levelling, arming, crashing, the wind,
//! and that it steps deterministically. `cargo test -p sim-core --test multirotor -- --nocapture`
//! prints the measurements.

use sim_core::multirotor::file;
use sim_core::{
    euler_angles, Controls, MultirotorControls, MultirotorKind, MultirotorType, Pilot, Spin, Start, Observation, State, VehicleId, World, DEG,
    V3,
};

const QUAD: MultirotorKind = MultirotorKind::FPV5;

fn spec() -> MultirotorType {
    MultirotorType::of(QUAD)
}

/// Over the middle of the runway nearest the origin: flat dry ground, nothing to run into.
fn over_home(world: &World, heading: f64) -> Start {
    let a = world.terrain.nearest_airport(0.0, 0.0).expect("an aerodrome");
    Start::Air { n: a.n, e: a.e, heading }
}

/// A world with the quad hovering 15 m over a runway, still air.
fn hovering(seed: u64) -> (World, VehicleId) {
    let mut world = World::new(seed);
    let start = over_home(&world, 0.4);
    let id = world.add_multirotor(QUAD, start);
    (world, id)
}

fn controls(world: &World, id: VehicleId) -> MultirotorControls {
    world.vehicle(id).multirotor().unwrap().controls
}

fn set(world: &mut World, id: VehicleId, change: impl FnOnce(MultirotorControls) -> MultirotorControls) {
    let m = world.vehicle_mut(id).multirotor_mut().unwrap();
    m.controls = change(m.controls);
}

fn run(world: &mut World, seconds: f64) {
    for _ in 0..(seconds / World::DT).round() as usize {
        world.step();
    }
}

fn up(world: &World, id: VehicleId) -> f64 {
    -world.vehicle(id).body().world_velocity().z
}

/// A pilot that holds the height it was given with the throttle alone, self-levelled, the other
/// sticks centred: a P and I on the height's error and the climb, on the observation only.
struct HoverPilot {
    target: f64,
    integral: f64,
}

impl Pilot for HoverPilot {
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls {
        let Controls::Multirotor(_) = current else { return *current };
        let m = obs.multirotor().expect("a multirotor's observation");
        let error = self.target - m.altitude;
        let climb = (0.8 * error).clamp(-3.0, 3.0);
        let climb_error = climb - m.vertical_speed;
        self.integral = (self.integral + 0.04 * climb_error * dt).clamp(-0.3, 0.3);
        let throttle = (0.40 + self.integral + 0.06 * climb_error).clamp(0.0, 1.0);
        Controls::Multirotor(MultirotorControls { throttle, roll: 0.0, pitch: 0.0, yaw: 0.0, arm: true, ..MultirotorControls::default() })
    }
}

#[test]
fn the_files_fleet_is_the_quad() {
    let kinds: Vec<&str> = MultirotorKind::all().map(|k| k.key()).collect();
    assert_eq!(kinds, ["fpv5", "race5", "kestrel"]);
    assert_eq!(MultirotorKind::parse("drone"), Some(QUAD));
    assert_eq!(MultirotorKind::parse("FPV"), Some(QUAD));
    assert_eq!(MultirotorKind::parse("c172"), None);
    let p = spec();
    assert_eq!(p.motors.len(), 4);
    assert!((p.visual.camera.uptilt / DEG - 25.0).abs() < 1e-9 && (p.visual.camera.fov / DEG - 120.0).abs() < 1e-9);
    // A broken file says where.
    let broken = file::BUILT_IN[0].1.replace("mass: 0.65", "mass: -1.0");
    let e = file::load(&broken, "fpv5.ron").unwrap_err();
    assert!(e.to_string().contains("mass.mass"), "{e}");
    let misspelt = file::BUILT_IN[0].1.replace("crash_speed", "crash_sped");
    assert!(file::load(&misspelt, "fpv5.ron").is_err());
}

/// About 4:1, and it hovers at 35 to 45 % of the throttle with the battery full.
#[test]
fn it_hovers_at_about_forty_percent_with_four_to_one_thrust() {
    let p = spec();
    let twr = p.thrust_to_weight();
    let hover = p.hover_throttle(1.225);
    println!("thrust to weight {twr:.2}, hover throttle {:.1} %", hover * 100.0);
    assert!((3.6..4.6).contains(&twr), "thrust to weight {twr}");
    assert!((0.35..0.45).contains(&hover), "hover throttle {hover}");

    // In the world: started hovering, the throttle where the model says, it stays within a
    // fraction of a metre a second, the rotors' thrust its weight; as the battery runs down the
    // same throttle lifts less (no compensation for the voltage), so it sinks slowly.
    let (mut world, id) = hovering(1);
    run(&mut world, 2.0);
    let m = world.vehicle(id).multirotor().unwrap();
    println!("after 2 s at {:.1} %: climbing {:.2} m/s, thrust {:.2} N for a weight of {:.2} N, {:.2} V", controls(&world, id).throttle * 100.0, up(&world, id), m.data().thrust, p.mass * 9.80665, m.battery().voltage);
    assert!(up(&world, id).abs() < 0.2, "climbing {}", up(&world, id));
    assert!((m.data().thrust / (p.mass * 9.80665) - 1.0).abs() < 0.02);
    assert!(m.armed());
    let volts = m.battery().voltage;
    run(&mut world, 10.0);
    let m = world.vehicle(id).multirotor().unwrap();
    println!("after 12 s: climbing {:.2} m/s, {:.2} V, {:.0} mAh used", up(&world, id), m.battery().voltage, m.battery().used / 3.6);
    assert!(m.battery().voltage < volts && up(&world, id) < 0.0);
}

/// Full throttle, level: it climbs at some 20 to 30 m/s (its thrust falling as the air
/// comes through the discs faster, its drag growing), and the battery sags.
#[test]
fn full_throttle_climbs_at_about_twenty_metres_a_second() {
    let (mut world, id) = hovering(2);
    let rest = world.vehicle(id).multirotor().unwrap().battery().voltage;
    set(&mut world, id, |c| MultirotorControls { throttle: 1.0, ..c });
    let (mut most, mut amps, mut least): (f64, f64, f64) = (0.0, 0.0, rest);
    for _ in 0..(6.0 / World::DT) as usize {
        world.step();
        most = most.max(up(&world, id));
        let battery = world.vehicle(id).multirotor().unwrap().battery();
        amps = amps.max(battery.current);
        least = least.min(battery.voltage);
    }
    println!("full throttle: climbing {most:.1} m/s at most, down to {least:.1} V under up to {amps:.0} A (hovering {rest:.1} V)");
    assert!((18.0..30.0).contains(&most), "climb {most}");
    assert!(least < rest - 1.5 && amps > 80.0);
}

/// Nose down at full stick with the throttle up, it flies at 20 to 35 m/s.
#[test]
fn flat_out_it_flies_at_about_thirty_metres_a_second() {
    let (mut world, id) = hovering(3);
    // Climb a little first, so that it does not meet the ground.
    { let s = over_home(&world, 0.4); world.reset(id, s) };
    let mut best: f64 = 0.0;
    for k in 0..(12.0 / World::DT) as usize {
        let t = k as f64 * World::DT;
        // Hold the height with the throttle while nose down.
        let alt = -world.vehicle(id).body().pos.z;
        let ground = world.terrain.height(world.vehicle(id).body().pos.x, world.vehicle(id).body().pos.y).max(0.0);
        let throttle = (0.75 + 0.05 * (ground + 60.0 - alt) - 0.05 * up(&world, id)).clamp(0.3, 1.0);
        set(&mut world, id, |c| MultirotorControls { throttle, pitch: if t > 0.5 { -1.0 } else { 0.0 }, ..c });
        world.step();
        let v = world.vehicle(id).body().world_velocity();
        best = best.max(v.x.hypot(v.y));
        assert!(world.vehicle(id).dynamics.crashed().is_none(), "{:?} at {t}", world.vehicle(id).dynamics.crashed());
    }
    println!("flat out: {best:.1} m/s over the ground");
    assert!((20.0..38.0).contains(&best), "top speed {best}");
}

/// The motors that turn counter-clockwise (seen from above) drag the frame clockwise: to yaw
/// right the controller speeds them up, and it yaws right.
#[test]
fn yaw_comes_from_the_motors_torque() {
    let (mut world, id) = hovering(4);
    set(&mut world, id, |c| MultirotorControls { yaw: 0.5, ..c });
    run(&mut world, 0.15);
    let m = world.vehicle(id).multirotor().unwrap();
    let speeds = m.rotor_speeds();
    let mean = |spin: Spin| {
        let s: Vec<f64> = m.p.motors.iter().zip(speeds).filter(|(motor, _)| motor.spin == spin).map(|(_, w)| *w).collect();
        s.iter().sum::<f64>() / s.len() as f64
    };
    let (cw, ccw) = (mean(Spin::Clockwise), mean(Spin::CounterClockwise));
    let r = world.vehicle(id).body().omega.z;
    println!("yaw right: counter-clockwise rotors {ccw:.0} rad/s, clockwise {cw:.0}, yaw rate {:.0}°/s", r / DEG);
    assert!(ccw > cw + 50.0, "ccw {ccw} cw {cw}");
    assert!(r > 0.5, "yaw rate {r}");

    // And the other way.
    let (mut world, id) = hovering(4);
    set(&mut world, id, |c| MultirotorControls { yaw: -0.5, ..c });
    run(&mut world, 0.5);
    assert!(world.vehicle(id).body().omega.z < -0.5);
}

/// The rates of the file: 667°/s at full stick on roll and pitch, 200 · (1 − 0.7) times less the
/// slope at the middle; the yaw, the one flown self-levelled, 364°/s at full stick and soft in the
/// middle, as angle-mode pilots set it.
#[test]
fn the_sticks_ask_for_betaflight_rates() {
    let r = spec().controller.rates;
    assert!((r.rate(0, 1.0) / DEG - 666.67).abs() < 0.1, "{}", r.rate(0, 1.0) / DEG);
    assert_eq!(r.rate(0, 0.0), 0.0);
    assert!((r.rate(1, -1.0) + r.rate(1, 1.0)).abs() < 1e-12);
    // At half stick: 200 · 0.5 / (1 − 0.35).
    assert!((r.rate(0, 0.5) / DEG - 100.0 / 0.65).abs() < 1e-6);
    // The yaw: 200 / (1 − 0.45) at full stick; at half, the stick bent by the expo first.
    assert!((r.rate(2, 1.0) / DEG - 200.0 / 0.55).abs() < 1e-6, "{}", r.rate(2, 1.0) / DEG);
    let half = 0.5 * 0.125 * 0.3 + 0.5 * 0.7;
    assert!((r.rate(2, 0.5) / DEG - 200.0 * half / (1.0 - 0.225)).abs() < 1e-6, "{}", r.rate(2, 0.5) / DEG);
    // Expo softens the middle and leaves the end.
    let soft = sim_core::StickRates { expo: V3::splat(0.5), ..r };
    assert!(soft.rate(0, 0.2) < r.rate(0, 0.2) && (soft.rate(0, 1.0) - r.rate(0, 1.0)).abs() < 1e-9);
}

/// The sticks tilt it to their angle and letting go levels it.
#[test]
fn the_sticks_tilt_it_and_it_levels() {
    let (mut world, id) = hovering(6);
    set(&mut world, id, |c| MultirotorControls { roll: 0.5, ..c });
    run(&mut world, 1.0);
    let roll = euler_angles(world.vehicle(id).body().q).roll;
    println!("self-levelling: {:.1}° of bank at half stick", roll / DEG);
    assert!((roll / DEG - 27.5).abs() < 2.5, "{}", roll / DEG);
    set(&mut world, id, |c| MultirotorControls { roll: 0.0, pitch: 1.0, ..c });
    run(&mut world, 1.0);
    let e = euler_angles(world.vehicle(id).body().q);
    assert!(e.roll.abs() < 2.0 * DEG && (e.pitch / DEG - 55.0).abs() < 3.0, "{e:?}");
    set(&mut world, id, |c| MultirotorControls { pitch: 0.0, ..c });
    run(&mut world, 1.0);
    let e = euler_angles(world.vehicle(id).body().q);
    assert!(e.roll.abs() < 1.0 * DEG && e.pitch.abs() < 1.0 * DEG, "{e:?}");
}

/// Arming: not with the throttle up, nor tilted; then the motors idle; disarmed they stop.
#[test]
fn it_arms_only_with_the_throttle_down() {
    let mut world = World::new(8);
    let id = world.add_multirotor(QUAD, Start::Home);
    let m = MultirotorControls::default;
    // It starts armed with the throttle down, as the aircraft start with the engine running.
    assert!(world.vehicle(id).dynamics.on_ground() && world.vehicle(id).multirotor().unwrap().armed());
    set(&mut world, id, |_| MultirotorControls { arm: false, ..m() });
    run(&mut world, 0.2);
    set(&mut world, id, |_| MultirotorControls { arm: true, throttle: 0.3, ..m() });
    run(&mut world, 0.2);
    let quad = world.vehicle(id).multirotor().unwrap();
    assert!(!quad.armed() && quad.arming_blocked() == Some("THROTTLE"));
    set(&mut world, id, |_| MultirotorControls { arm: true, throttle: 0.0, ..m() });
    run(&mut world, 0.5);
    let quad = world.vehicle(id).multirotor().unwrap();
    assert!(quad.armed() && quad.rotor_speeds().iter().all(|&w| w > 200.0), "{:?}", quad.rotor_speeds());
    // Idle does not lift it.
    assert!(world.vehicle(id).dynamics.on_ground());
    set(&mut world, id, |_| MultirotorControls { arm: false, ..m() });
    run(&mut world, 2.0);
    assert!(world.vehicle(id).multirotor().unwrap().rotor_speeds().iter().all(|&w| w < 30.0));
    // Off the ground and up, then down at its own pace onto the runway, unhurt.
    set(&mut world, id, |_| MultirotorControls { arm: true, throttle: 0.0, ..m() });
    run(&mut world, 0.2);
    set(&mut world, id, |_| MultirotorControls { arm: true, throttle: 0.5, ..m() });
    run(&mut world, 1.0);
    assert!(world.vehicle(id).agl() > 2.0);
    set(&mut world, id, |_| MultirotorControls { arm: true, throttle: 0.33, ..m() });
    run(&mut world, 15.0);
    let v = world.vehicle(id);
    assert!(v.dynamics.on_ground() && v.dynamics.crashed().is_none());
}

/// Dropped from 15 m with the motors off it hits at some 15 m/s and crashes: disarmed, the arm
/// switch does nothing until it is placed again.
#[test]
fn a_hard_fall_crashes_it() {
    let (mut world, id) = hovering(9);
    set(&mut world, id, |c| MultirotorControls { arm: false, ..c });
    run(&mut world, 4.0);
    let v = world.vehicle(id);
    assert_eq!(v.dynamics.crashed(), Some("Crashed into the ground"));
    set(&mut world, id, |c| MultirotorControls { arm: true, throttle: 0.0, ..c });
    run(&mut world, 0.5);
    let quad = world.vehicle(id).multirotor().unwrap();
    assert!(!quad.armed() && quad.arming_blocked() == Some("CRASH"));
    // It came to rest.
    run(&mut world, 5.0);
    assert!(world.vehicle(id).body().vel.length() < 0.05);
    { let s = over_home(&world, 0.4); world.reset(id, s) };
    assert!(world.vehicle(id).dynamics.crashed().is_none() && world.vehicle(id).multirotor().unwrap().armed());
}

/// The wind carries it: hovering level in 8 m/s of wind it drifts downwind at
/// nearly the wind's speed after some seconds.
#[test]
fn the_wind_carries_it() {
    let mut world = World::new(10);
    world.weather.wind_speed = 8.0;
    world.weather.wind_from = 270.0;
    let id = { let s = over_home(&world, 0.0); world.add_multirotor(QUAD, s) };
    let mut pilot = HoverPilot { target: -world.vehicle(id).body().pos.z, integral: 0.0 };
    for _ in 0..(15.0 / World::DT) as usize {
        world.fly(id, &mut pilot);
        world.step();
    }
    let v = world.vehicle(id).body().world_velocity();
    let wind = world.weather.mean_wind(world.vehicle(id).agl());
    println!("drifting at ({:.1}, {:.1}) m/s in a wind of ({:.1}, {:.1})", v.x, v.y, wind.x, wind.y);
    // From the west: towards the east.
    assert!(v.y > 0.6 * wind.y && v.y < 1.1 * wind.y, "{v} in {wind}");
}

/// Same start and sticks, the same flight, to the bit; and the C172's world is not changed by a
/// quad flying in it (each vehicle has its own air).
#[test]
fn it_steps_deterministically() {
    let fly = || {
        let (mut world, id) = hovering(11);
        world.weather.turbulence = 0.5;
        world.weather.wind_speed = 5.0;
        let mut states = Vec::new();
        for k in 0..2400 {
            let t = k as f64 * World::DT;
            let c = |c| MultirotorControls { roll: (t * 1.3).sin() * 0.4, pitch: (t * 0.7).cos() * 0.3, yaw: 0.2, throttle: 0.45 + 0.1 * (t * 2.0).sin(), ..c };
            set(&mut world, id, c);
            world.step();
            states.push(world.vehicle(id).body());
        }
        states
    };
    let (a, b) = (fly(), fly());
    assert!(a.iter().zip(&b).all(|(x, y)| x.pos.to_array().map(f64::to_bits) == y.pos.to_array().map(f64::to_bits) && x.q.to_array().map(f64::to_bits) == y.q.to_array().map(f64::to_bits)));
}

/// A hover pilot flying through `World::fly` on the observation alone holds a height.
#[test]
fn a_hover_pilot_holds_a_height() {
    let (mut world, id) = hovering(12);
    world.weather.turbulence = 0.3;
    let target = -world.vehicle(id).body().pos.z + 5.0;
    let mut pilot = HoverPilot { target, integral: 0.0 };
    let mut worst: f64 = 0.0;
    for k in 0..(30.0 / World::DT) as usize {
        world.fly(id, &mut pilot);
        world.step();
        if k as f64 * World::DT > 8.0 {
            worst = worst.max((-world.vehicle(id).body().pos.z - target).abs());
        }
    }
    println!("hover pilot: within {worst:.2} m of its height");
    assert!(worst < 0.5, "{worst}");
}

/// The quad on its feet: placed by hand anywhere, it stays upright and still.
#[test]
fn it_stands_on_its_feet() {
    let mut world = World::new(13);
    let id = world.add_multirotor(QUAD, Start::Home);
    let start = world.vehicle(id).body();
    run(&mut world, 3.0);
    let body = world.vehicle(id).body();
    assert!((body.pos - start.pos).length() < 0.01 && body.vel.length() < 0.01, "{:?}", body);
    // Turned over by hand onto its back from a little height, it lands on its battery, unhurt.
    let s = State { pos: start.pos - V3::new(0.0, 0.0, 0.3), q: sim_core::quaternion(1.0, 0.0, 180.0 * DEG), ..State::default() };
    world.place(id, s);
    run(&mut world, 2.0);
    let v = world.vehicle(id);
    assert!(v.dynamics.crashed().is_none() && v.body().vel.length() < 0.05);
}

/// From a hover 80 m up (moving `forward` m/s ahead), the throttle at `throttle` for 3 s: the
/// most it tilts (degrees), its mean tilt over the last two seconds, its fastest roll or pitch
/// rate (°/s) and its sink at the end (m/s).
fn drop(seed: u64, throttle: f64, forward: f64) -> (f64, f64, f64, f64) {
    let (mut world, id) = hovering(seed);
    let body = world.vehicle(id).body();
    let s = State { pos: body.pos - V3::new(0.0, 0.0, 65.0), vel: V3::new(forward, 0.0, 0.0), q: body.q, ..State::default() };
    world.place(id, s);
    set(&mut world, id, |c| MultirotorControls { throttle, ..c });
    let (mut most, mut sum, mut rate): (f64, f64, f64) = (0.0, 0.0, 0.0);
    let steps = (3.0 / World::DT).round() as usize;
    let late = (1.0 / World::DT).round() as usize;
    for k in 0..steps {
        world.step();
        let b = world.vehicle(id).body();
        let tilt = sim_core::act(b.q, V3::Z).z.clamp(-1.0, 1.0).acos() / DEG;
        most = most.max(tilt);
        if k >= late {
            sum += tilt;
        }
        rate = rate.max(b.omega.x.hypot(b.omega.y) / DEG);
        assert!(world.vehicle(id).dynamics.crashed().is_none());
    }
    (most, sum / (steps - late) as f64, rate, -up(&world, id))
}

/// Falling into its own wash with the motors at idle (the vortex ring state, "propwash" to FPV
/// pilots) it wobbles hard, all the way down: each rotor's thrust comes and goes in the turbulent
/// air and idle leaves the flight controller little to fight it with. Less with some throttle
/// (it punches out), hardly at all moving forward (it leaves its wash behind), and not hovering.
/// Before the wash shook each rotor it tilted 2.5 to 3.7° at most, 60°/s, at zero throttle.
#[test]
fn falling_into_its_own_wash_it_wobbles() {
    for seed in 20..23 {
        let mut runs = Vec::new();
        for (throttle, forward) in [(0.0, 0.0), (0.2, 0.0), (0.0, 20.0), (spec().hover_throttle(1.225), 0.0)] {
            let (most, mean, rate, sink) = drop(seed, throttle, forward);
            println!("seed {seed}, throttle {throttle:.2}, forward {forward:.0} m/s: tilt up to {most:.1}° ({mean:.1}° on the mean late on), rate up to {rate:.0}°/s, sinking {sink:.1} m/s");
            runs.push((most, mean, rate, sink));
        }
        let [idle, partial, forward, hover] = runs[..] else { unreachable!() };
        assert!(idle.0 > 12.0 && idle.1 > 3.0 && idle.2 > 150.0 && idle.3 > 12.0, "zero throttle {idle:?}");
        assert!(partial.0 < 0.8 * idle.0 && partial.2 < 0.8 * idle.2, "partial throttle {partial:?} against {idle:?}");
        assert!(forward.0 < 3.0 && forward.2 < 30.0, "forward {forward:?}");
        assert!(hover.0 < 1.0 && hover.3.abs() < 1.0, "hover {hover:?}");
    }
}

/// In still air, on the ground armed, the throttle raised to climb and then held: it goes straight
/// up, with or without turbulence asked for (there is no wind for it to stir). Nothing of its own
/// (the motors' torque, their places, its own wash or the ground's effect) pushes it sideways.
#[test]
fn in_still_air_it_climbs_straight_up() {
    for turbulence in [0.0, 0.3] {
        let mut world = World::new(11);
        world.weather.wind_speed = 0.0;
        world.weather.turbulence = turbulence;
        let id = world.add_multirotor(QUAD, Start::Home);
        let start = world.vehicle(id).body().pos;
        for k in 0..(8.0 / World::DT) as usize {
            let t = k as f64 * World::DT;
            // The throttle key held for a second and a half (0.45 a second, `game::THROTTLE_RATE`),
            // a little taken off after, then left there.
            let throttle = if t < 0.5 { 0.0 } else if t < 2.0 { 0.45 * (t - 0.5) } else if t < 2.3 { 0.675 - 0.45 * (t - 2.0) } else { 0.54 };
            set(&mut world, id, |c| MultirotorControls { throttle, ..c });
            world.step();
        }
        let b = world.vehicle(id).body();
        let moved = b.pos - start;
        let across = moved.x.hypot(moved.y);
        println!("turbulence {turbulence}: up {:.1} m, across {across:.2} m", -moved.z);
        assert!(-moved.z > 20.0, "climbed {}", -moved.z);
        assert!(across < 0.5, "drifted {across} m");
    }
}

/// A banked turn in the self-levelling mode: flying forward at speed, the roll stick banks it and
/// the yaw stick turns the heading about the vertical at the rate it asks for (the bank and the
/// pitch kept, not turned into each other); at the yaw rate a turn at that bank and speed needs,
/// `g · tan(bank) / speed`, it carves a clean arc, the nose along the way it flies.
#[test]
fn a_banked_turn_with_yaw_carves_a_clean_arc() {
    let (mut world, id) = hovering(12);
    // Forward, holding the height (the throttle by the pilot, the pitch stick 0.3 forward).
    let mut pilot = HoverPilot { target: -world.vehicle(id).body().pos.z, integral: 0.0 };
    let mut fly = |world: &mut World, seconds: f64, roll: f64, yaw: f64| {
        for _ in 0..(seconds / World::DT).round() as usize {
            world.fly(id, &mut pilot);
            set(world, id, |c| MultirotorControls { pitch: -0.3, roll, yaw, ..c });
            world.step();
        }
    };
    fly(&mut world, 6.0, 0.0, 0.0);
    let v = world.vehicle(id).body().world_velocity();
    let speed = v.x.hypot(v.y);
    // A bank of 0.4 of the stick's 55°, and the yaw stick that asks for a turn at that bank and speed.
    let p = spec();
    let bank = 0.4 * p.controller.angle_max;
    let wanted = 9.80665 * bank.tan() / speed;
    let mut stick = 0.0;
    while p.controller.rates.rate(2, stick) < wanted && stick < 1.0 {
        stick += 0.0005;
    }
    let heading = |world: &World| euler_angles(world.vehicle(id).body().q).heading;
    fly(&mut world, 1.5, 0.4, stick);
    let before = heading(&world);
    fly(&mut world, 1.0, 0.4, stick);
    let turned = (heading(&world) - before + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let b = world.vehicle(id).body();
    let e = euler_angles(b.q);
    let v = b.world_velocity();
    let track = v.y.atan2(v.x);
    let slip = (track - e.heading + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    println!(
        "at {speed:.1} m/s, bank {:.1}° (asked {:.1}°), pitch {:.1}°: heading turning {:.1}°/s (asked {:.1}°/s), sideslip {:.1}°",
        e.roll / DEG,
        bank / DEG,
        e.pitch / DEG,
        turned / DEG,
        wanted / DEG,
        slip / DEG
    );
    assert!(speed > 8.0, "{speed}");
    assert!((e.roll - bank).abs() < 3.0 * DEG, "bank {}", e.roll / DEG);
    assert!((e.pitch + 0.3 * p.controller.angle_max).abs() < 4.0 * DEG, "pitch {}", e.pitch / DEG);
    assert!((turned - wanted).abs() < 0.1 * wanted, "turned {}°/s for {}°/s", turned / DEG, wanted / DEG);
    assert!(slip.abs() < 6.0 * DEG, "sideslip {}°", slip / DEG);
}
