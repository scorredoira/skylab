//! The 5-inch racing quad (`assets/aircraft/race5/multirotor.ron`) against how such quads fly:
//! 12 to 15 times its weight in thrust, a hover at some 15 % of the throttle, a climb of 40 m/s and
//! more, 150 to 180 km/h flat out self-levelled at its 75°, a racer's yaw rate; and that fpv5 is
//! left as it was. `cargo test -p sim-core --test race_quad -- --nocapture` prints the measurements.

use sim_core::{MultirotorControls, MultirotorKind, MultirotorType, Start, VehicleId, World, DEG};

const QUAD: MultirotorKind = MultirotorKind::RACE5;

fn hovering(seed: u64) -> (World, VehicleId) {
    let mut world = World::new(seed);
    let a = world.terrain.nearest_airport(0.0, 0.0).expect("an aerodrome");
    let id = world.add_multirotor(QUAD, Start::Air { n: a.n, e: a.e, heading: 0.4 });
    (world, id)
}

fn set(world: &mut World, id: VehicleId, change: impl FnOnce(MultirotorControls) -> MultirotorControls) {
    let m = world.vehicle_mut(id).multirotor_mut().unwrap();
    m.controls = change(m.controls);
}

fn up(world: &World, id: VehicleId) -> f64 {
    -world.vehicle(id).body().world_velocity().z
}

#[test]
fn it_is_in_the_fleet_beside_the_freestyle_quad() {
    let kinds: Vec<&str> = MultirotorKind::all().map(|k| k.key()).collect();
    assert_eq!(kinds, ["fpv5", "race5", "kestrel"]);
    assert_eq!(MultirotorKind::parse("race"), Some(QUAD));
    assert_eq!(MultirotorKind::parse("drone"), Some(MultirotorKind::FPV5));
    let p = MultirotorType::of(QUAD);
    assert!((p.visual.camera.uptilt / DEG - 40.0).abs() < 1e-9);
    assert_eq!(p.cells, 6);
    assert!((0.45..0.55).contains(&p.mass));
}

/// Some 13:1, hovering at some 15 % of the throttle with the pack full.
#[test]
fn thirteen_to_one_and_a_light_hover() {
    let p = MultirotorType::of(QUAD);
    let twr = p.thrust_to_weight();
    let hover = p.hover_throttle(1.225);
    println!("race5: thrust to weight {twr:.1}, hover throttle {:.1} %", hover * 100.0);
    assert!((12.0..15.0).contains(&twr), "thrust to weight {twr}");
    assert!((0.12..0.28).contains(&hover), "hover throttle {hover}");
}

/// Full throttle, level: a climb past 40 m/s within a few seconds, the pack sagging under 150 A
/// and more.
#[test]
fn full_throttle_climbs_past_forty_metres_a_second() {
    let (mut world, id) = hovering(2);
    set(&mut world, id, |c| MultirotorControls { throttle: 1.0, ..c });
    let (mut most, mut amps, mut least): (f64, f64, f64) = (0.0, 0.0, f64::MAX);
    for _ in 0..(5.0 / World::DT) as usize {
        world.step();
        most = most.max(up(&world, id));
        let b = world.vehicle(id).multirotor().unwrap().battery();
        amps = amps.max(b.current);
        least = least.min(b.voltage);
    }
    println!("race5 full throttle: climbing {most:.1} m/s at most, down to {least:.1} V under up to {amps:.0} A");
    assert!(most > 40.0, "climb {most}");
    assert!(amps > 150.0 && least < 23.0);
}

/// Nose down at full stick (75°), the throttle holding its height: 150 to 180 km/h.
#[test]
fn flat_out_it_flies_at_some_one_hundred_and_sixty_kilometres_an_hour() {
    let (mut world, id) = hovering(3);
    let mut best: f64 = 0.0;
    for k in 0..(10.0 / World::DT) as usize {
        let t = k as f64 * World::DT;
        let pos = world.vehicle(id).body().pos;
        let ground = world.terrain.height(pos.x, pos.y).max(0.0);
        let throttle = (0.5 + 0.05 * (ground + 60.0 + pos.z) - 0.05 * up(&world, id)).clamp(0.1, 1.0);
        set(&mut world, id, |c| MultirotorControls { throttle, pitch: if t > 0.5 { -1.0 } else { 0.0 }, ..c });
        world.step();
        let v = world.vehicle(id).body().world_velocity();
        best = best.max(v.x.hypot(v.y));
        assert!(world.vehicle(id).dynamics.crashed().is_none());
    }
    println!("race5 flat out: {best:.1} m/s, {:.0} km/h", best * 3.6);
    assert!((150.0..185.0).contains(&(best * 3.6)), "top speed {best}");
}

/// Full yaw stick: some 440°/s.
#[test]
fn it_yaws_at_a_racers_rate() {
    let (mut world, id) = hovering(4);
    set(&mut world, id, |c| MultirotorControls { yaw: 1.0, ..c });
    for _ in 0..(1.0 / World::DT) as usize {
        world.step();
    }
    let rate = world.vehicle(id).body().omega.z / DEG;
    println!("race5 yaw at full stick: {rate:.0}°/s");
    assert!((380.0..480.0).contains(&rate), "{rate}");
}

/// Full roll stick from level: it banks to its 75° and no further than a few degrees past it, and
/// levels again when let go, as briskly, without swinging through.
#[test]
fn it_banks_to_its_limit_without_overshooting() {
    let (mut world, id) = hovering(5);
    let tilt = |w: &World| {
        let e = sim_core::euler_angles(w.vehicle(id).body().q);
        (e.pitch.cos() * e.roll.cos()).acos() / DEG
    };
    set(&mut world, id, |c| MultirotorControls { roll: 1.0, throttle: 0.4, ..c });
    let mut most: f64 = 0.0;
    for _ in 0..(0.6 / World::DT) as usize {
        world.step();
        most = most.max(tilt(&world));
    }
    let held = tilt(&world);
    set(&mut world, id, |c| MultirotorControls { roll: -1.0, ..c });
    let mut through: f64 = 0.0;
    for _ in 0..(0.6 / World::DT) as usize {
        world.step();
        through = through.max(tilt(&world));
    }
    println!("race5 full roll: {most:.1}° at most, {held:.1}° held; then the other way {through:.1}° at most");
    assert!(most < 80.0 && (70.0..80.0).contains(&held), "{most} {held}");
    assert!(through < 82.0, "{through}");
}
