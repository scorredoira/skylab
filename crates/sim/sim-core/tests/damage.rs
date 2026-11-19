//! What breaks: the structure against the handbook's limits (load factors, never-exceed speed,
//! flap speeds), the landing gear against the certification's descent velocities, impacts,
//! ditching, collisions with trees, buildings and other aircraft, and what is left afterwards.

mod common;

use sim_core::*;

const DT: f64 = 1.0 / 240.0;

fn ground_env() -> Environment {
    Environment {
        ground_height: 0.0,
        kind: GroundKind::Runway,
        ..Environment::default()
    }
}

/// An aircraft in trimmed level flight at `kt`, 1500 m up.
fn flying(kt: f64) -> (Aircraft, Environment) {
    let mut a = Aircraft::new();
    let altitude = 1500.0;
    let t = a.trim_level(kt * KNOTS, altitude).unwrap();
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(kt * KNOTS * t.alpha.cos(), 0.0, kt * KNOTS * t.alpha.sin()),
        q: quaternion(0.0, t.alpha, 0.0),
        prop_speed: t.prop_speed,
        ..State::default()
    };
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    let env = Environment { rho: atmosphere::density(altitude), ..Environment::default() };
    (a, env)
}

/// A touchdown at 25 m/s, sinking at `sink`, with the nose `pitch` degrees up.
fn touchdown(sink: f64, pitch: f64) -> Aircraft {
    let mut a = Aircraft::new();
    let env = ground_env();
    a.state.pos = V3::new(0.0, 0.0, -a.p.cg_height - 0.3);
    a.state.q = quaternion(0.0, pitch * DEG, 0.0);
    a.state.vel = act(inverse(a.state.q), V3::new(25.0, 0.0, sink));
    for _ in 0..480 {
        a.step(DT, &env);
    }
    a
}

#[test]
fn the_gear_takes_a_firm_landing_and_breaks_on_a_hard_one() {
    for (sink, pitch) in [(1.0, 4.0), (2.7, 0.0), (3.2, 4.0), (2.7, -6.0)] {
        let a = touchdown(sink, pitch);
        println!("touchdown {sink} m/s, {pitch}°: {:?}", a.crashed());
        assert!(a.crashed().is_none(), "{sink} m/s at {pitch}°: {:?}", a.crashed());
    }
    let a = touchdown(4.0, 0.0);
    println!("touchdown 4 m/s level: {:?} {:?}", a.damage().gear_broken, a.crashed());
    assert!(a.damage().gear_broken[1] && a.damage().gear_broken[2]);
    assert!(a.crashed().is_some() && !a.damage().broken_up);
    let a = touchdown(3.6, -6.0);
    println!("touchdown 3.6 m/s nose first: {:?} {:?}", a.damage().gear_broken, a.crashed());
    assert!(a.damage().gear_broken[0]);
    // Without its nose leg it goes on to the propeller, which stops the engine.
    assert!(a.damage().propeller_bent);
    assert!(a.rpm() < 100.0);
}

/// Full back yoke at a speed: the largest load factor felt and what is left.
fn full_back(kt: f64) -> Aircraft {
    let (mut a, env) = flying(kt);
    a.controls.elevator = 1.0;
    for _ in 0..240 * 3 {
        a.step(DT, &env);
    }
    a
}

#[test]
fn the_wings_take_the_limit_load_and_break_at_the_ultimate() {
    // Below the manoeuvring speed (the handbook's 97 kt at this weight) the wing stalls before
    // it reaches the limit load.
    let slow = full_back(90.0);
    println!("full back at 90 kt: {:.2} g, {:?}", slow.damage().most_g, slow.damage());
    assert!(slow.damage().most_g < 3.8 && slow.damage().bent == 0.0);
    assert!(slow.crashed().is_none());

    // Faster, the same yoke overstresses it, and much faster breaks a wing off.
    let fast = full_back(115.0);
    println!("full back at 115 kt: {:.2} g, bent {:.2}", fast.damage().most_g, fast.damage().bent);
    assert!(fast.damage().most_g > 3.8);
    assert!(fast.damage().bent > 0.0 || (fast.damage().wing_lost(0) || fast.damage().wing_lost(1)));

    let (mut a, env) = flying(120.0);
    a.controls.throttle = 1.0;
    a.controls.elevator = -0.3;
    while a.data().ias < 150.0 * KNOTS {
        a.step(DT, &env);
    }
    a.controls.elevator = 1.0;
    let mut lost = None;
    for i in 0..240 * 3 {
        a.step(DT, &env);
        if lost.is_none() && (a.damage().wing_lost(0) || a.damage().wing_lost(1)) {
            lost = Some((i as f64 * DT, a.damage().most_g));
        }
    }
    println!("full back at 150 kt: wing lost {lost:?}, {:?}", a.crashed());
    let (_, g) = lost.expect("a wing breaks off");
    assert!(g >= 3.8 * 1.5);
    assert!(a.crashed().is_some());
    let wrecks = a.take_wrecks();
    assert_eq!(wrecks.len(), 1);
    assert_eq!(wrecks[0].pieces.len(), 1);
    // What is left rolls towards the missing wing and falls.
    let (mut lowest_pitch, mut most_roll) = (0.0_f64, 0.0_f64);
    for _ in 0..240 * 10 {
        a.step(DT, &env);
        let e = euler_angles(a.state.q);
        lowest_pitch = lowest_pitch.min(e.pitch);
        most_roll = most_roll.max(a.state.omega.x.abs());
    }
    println!("one wing: roll rate up to {:.0}°/s, nose down to {:.0}°", most_roll / DEG, lowest_pitch / DEG);
    assert!(most_roll > 90.0 * DEG);
}

#[test]
fn flutter_past_the_never_exceed_speed_takes_one_surface_at_a_time() {
    // A dive at full power: past Vne it warns; at 1.2 times the design diving speed the control
    // surfaces flutter and go, one at a time and an aileron first, and what is left still flies.
    let (mut a, env) = flying(120.0);
    a.state.q = quaternion(0.0, -70.0 * DEG, 0.0);
    a.controls.throttle = 1.0;
    a.controls.elevator = -0.2;
    let mut warned = None;
    let mut first = None;
    for i in 0..240 * 60 {
        // Held in a 60° dive.
        let pitch = euler_angles(a.state.q).pitch;
        a.controls.elevator = clamp((-60.0 * DEG - pitch) * 3.0 - a.state.omega.y, -1.0, 1.0);
        a.step(DT, &env);
        if warned.is_none() && a.data().overspeed {
            warned = Some(a.data().ias / KNOTS);
        }
        if a.damage().ailerons.contains(&SurfaceState::Gone) {
            first = Some((i as f64 * DT, a.data().ias / KNOTS, *a.damage()));
            break;
        }
    }
    let (t, kt, d) = first.expect("an aileron flutters away");
    println!("flutter after {t:.1} s at {kt:.0} kt, warned at {warned:?} kt: {:?}", d.last);
    assert!(warned.is_some_and(|kt| kt < 165.0));
    assert!(kt > 1.2 * 181.0);
    assert_eq!(d.ailerons.iter().filter(|s| **s == SurfaceState::Gone).count(), 1);
    assert_eq!((d.elevator, d.rudder), (SurfaceState::Working, SurfaceState::Working));
    assert!(a.crashed().is_none(), "one aileron gone is not the end of the flight");
    // Held there, the rest follow, one at a time: the other aileron, the elevator, the rudder.
    let mut order = vec![d.last];
    for _ in 0..240 * 20 {
        let pitch = euler_angles(a.state.q).pitch;
        a.controls.elevator = clamp((-60.0 * DEG - pitch) * 3.0 - a.state.omega.y, -1.0, 1.0);
        a.step(DT, &env);
        if a.damage().last != *order.last().unwrap() {
            order.push(a.damage().last);
        }
    }
    println!("then: {order:?}");
    assert!(order.len() >= 2 && order[1].contains("aileron"), "{order:?}");
}

#[test]
fn flaps_out_too_fast_are_torn_off() {
    // Flaps 30 at 95 kt (Vfe 85 kt): a warning only. At 110 kt, more than the 22 % the structure
    // has in hand, one flap goes, and the aircraft rolls away from the one that is left.
    for (kt, torn) in [(95.0, false), (112.0, true)] {
        let (mut a, env) = flying(kt);
        a.controls.flap_notch = 3;
        a.controls.throttle = 1.0;
        let mut warned = false;
        let mut roll = 0.0_f64;
        for _ in 0..240 * 10 {
            a.step(DT, &env);
            warned |= a.data().flap_overspeed;
            if a.damage().flaps.contains(&SurfaceState::Gone) {
                roll = roll.max(a.state.omega.x.abs());
            }
        }
        println!("flaps 30 at {kt} kt: {:?}, roll rate {:.0}°/s", a.damage().flaps, roll / DEG);
        assert!(warned);
        assert_eq!(a.damage().flaps.contains(&SurfaceState::Gone), torn);
        assert!(a.crashed().is_none());
    }
}

#[test]
fn hitting_the_ground_hard_breaks_it_up_and_it_burns() {
    // 60 kt, 30° nose down into a field.
    let mut a = Aircraft::new();
    let env = Environment { ground_height: 0.0, kind: GroundKind::Grass, ..Environment::default() };
    a.state.pos = V3::new(0.0, 0.0, -3.0);
    a.state.q = quaternion(0.0, -30.0 * DEG, 0.0);
    a.state.vel = V3::new(31.0, 0.0, 0.0);
    a.state.prop_speed = 250.0;
    for _ in 0..240 * 2 {
        a.step(DT, &env);
    }
    println!("into the ground: {:?}", a.crashed());
    assert!(a.damage().broken_up);
    let mut wrecks = a.take_wrecks();
    assert_eq!(wrecks.len(), 1);
    let w = &mut wrecks[0];
    assert_eq!(w.pieces.len(), 4);
    assert!(w.fireball > 0.0 && w.fire.is_some());
    assert!(w.fragments.len() > 5, "{} fragments", w.fragments.len());
    // The pieces fly apart, fall and come to rest on the ground; the fire burns its fuel.
    let mut t = 0.0;
    while w.active() && t < 600.0 {
        w.step(DT, t, |_, _| (0.0, false));
        t += DT;
    }
    let fire = w.fire.unwrap();
    println!("fire out after {:.0} s; pieces at {:?}", fire.out.unwrap_or(-1.0), w.pieces.iter().map(|p| (p.kind, p.pos.x.round(), p.pos.y.round(), p.pos.z)).collect::<Vec<_>>());
    for p in &w.pieces {
        assert!(p.at_rest, "{:?} still moving", p.kind);
        assert!(p.pos.z < 0.5 && p.pos.z > -2.5, "{:?} at height {}", p.kind, -p.pos.z);
    }
    // The fragments came down round it, up to some tens of metres away; it dug a hole and scorched the ground.
    for f in &w.fragments {
        let landed = f.landed.expect("a fragment still flying");
        let at = f.at(landed);
        assert!(at.z.abs() < 1e-6 || at.z > 0.0 && at.z < 0.2, "{at:?}");
        assert!((at - w.impact).length() < 150.0, "{at:?}");
    }
    println!("scars: {:?}", w.scars.iter().map(|s| (s.radius, s.crater)).collect::<Vec<_>>());
    assert!(w.scars.iter().any(|s| s.crater > 0.5));
    assert!(w.scars.iter().any(|s| s.crater == 0.0 && s.radius > 3.0));
    assert!(fire.out.is_some_and(|out| out > 200.0 && out < 400.0));
    // The same crash is the same wreck.
    let mut b = Aircraft::new();
    b.state = State { pos: V3::new(0.0, 0.0, -3.0), q: quaternion(0.0, -30.0 * DEG, 0.0), vel: V3::new(31.0, 0.0, 0.0), prop_speed: 250.0, ..State::default() };
    for _ in 0..240 * 2 {
        b.step(DT, &env);
    }
    assert_eq!(b.take_wrecks()[0].pieces[0], a_first_piece(&env));
}

fn a_first_piece(env: &Environment) -> Piece {
    let mut a = Aircraft::new();
    a.state = State { pos: V3::new(0.0, 0.0, -3.0), q: quaternion(0.0, -30.0 * DEG, 0.0), vel: V3::new(31.0, 0.0, 0.0), prop_speed: 250.0, ..State::default() };
    for _ in 0..240 * 2 {
        a.step(DT, env);
    }
    a.take_wrecks()[0].pieces[0]
}

#[test]
fn a_gentle_ditching_floats_and_then_sinks() {
    let mut a = Aircraft::new();
    let env = Environment { ground_height: 0.0, kind: GroundKind::Water, ..Environment::default() };
    let t = a.trim(55.0 * KNOTS, 0.0, 30.0, None, -2.0 * DEG).unwrap();
    a.flap_deg = 30.0;
    a.controls.flap_notch = 3;
    a.controls.throttle = t.throttle;
    a.controls.elevator = t.elevator;
    a.state = State {
        pos: V3::new(0.0, 0.0, -2.0),
        vel: V3::new(55.0 * KNOTS * t.alpha.cos(), 0.0, 55.0 * KNOTS * t.alpha.sin()),
        q: quaternion(0.0, t.alpha - 2.0 * DEG, 0.0),
        prop_speed: t.prop_speed,
        ..State::default()
    };
    let mut afloat_at_30 = None;
    let mut sank = None;
    for i in 0..240 * 300 {
        a.step(DT, &env);
        let time = i as f64 * DT;
        if (time - 30.0).abs() < DT / 2.0 {
            afloat_at_30 = Some(-a.state.pos.z);
        }
        if sank.is_none() && a.state.pos.z > 3.0 {
            sank = Some(time);
        }
    }
    println!("ditching: {:?}, height after 30 s {afloat_at_30:?}, under at {sank:?} s, sunk {}", a.crashed(), a.damage().sunk);
    assert_eq!(a.crashed(), Some("Ditched"));
    assert!(!a.damage().broken_up);
    assert!(afloat_at_30.is_some_and(|h| h > -1.5), "it floats at first");
    assert!(sank.is_some_and(|t| t > 40.0 && t < 200.0));
}

#[test]
fn diving_into_water_breaks_it_up() {
    let mut a = Aircraft::new();
    let env = Environment { ground_height: 0.0, kind: GroundKind::Water, ..Environment::default() };
    a.state = State { pos: V3::new(0.0, 0.0, -3.0), q: quaternion(0.0, -40.0 * DEG, 0.0), vel: V3::new(50.0, 0.0, 0.0), ..State::default() };
    for _ in 0..240 {
        a.step(DT, &env);
    }
    assert!(a.damage().broken_up);
    let mut wrecks = a.take_wrecks();
    let w = &mut wrecks[0];
    assert!(w.fire.is_none() && w.fireball == 0.0, "nothing burns in the water");
    // It goes in with a splash, and so do the pieces and fragments that fall back in.
    for i in 0..240 * 10 {
        w.step(DT, i as f64 * DT, |_, _| (0.0, true));
    }
    println!("into the water: {} splashes, {} fragments", w.splashes.len(), w.fragments.len());
    assert!(w.splashes[0].time == 0.0 && w.splashes[0].mass > 500.0);
    assert!(w.splashes.len() > 2);
    assert!(w.scars.is_empty());
}

#[test]
fn the_fuel_burns_only_if_the_impact_bursts_the_tanks_and_lights_it() {
    let fuel = 100.0;
    // A soft impact: the tanks hold, or what leaks has nothing to light it.
    assert_eq!(fuel_released(fuel, 8.0, false), (0.0, 0.0));
    assert_eq!(fuel_released(fuel, IGNITION_SPEED - 0.5, false), (0.0, 0.0));
    // Harder, more of it bursts out, and more of that as a mist that goes up at once.
    let mut last = (0.0, 0.0);
    for severity in [20.0, 25.0, 30.0, 40.0, 60.0] {
        let (fireball, pool) = fuel_released(fuel, severity, false);
        println!("impact at {severity} m/s: fireball {fireball:.1} kg, pool {pool:.1} kg");
        assert!(fireball + pool <= fuel + 1e-9 && fireball >= last.0 && fireball + pool >= last.0 + last.1);
        last = (fireball, pool);
    }
    assert!(last.0 > 40.0 && last.0 + last.1 > 99.9);
    // Nothing in the water; nothing without fuel.
    assert_eq!(fuel_released(fuel, 60.0, true), (0.0, 0.0));
    assert_eq!(fuel_released(0.0, 60.0, false), (0.0, 0.0));
}

#[test]
fn a_slow_crash_into_a_building_neither_burns_nor_scatters_much() {
    // 17 m/s into a wall (the pieces keep 15 % of their speed): broken up, but it does not explode.
    let mut a = Aircraft::new();
    a.state = State { pos: V3::new(0.0, 0.0, -1.5), q: quaternion(0.0, 0.0, 0.0), vel: V3::new(17.0, 0.0, 0.0), ..State::default() };
    a.collide("Crashed into a building", 17.0, 0.15);
    let w = &a.take_wrecks()[0];
    println!("slowly into a wall: severity {:.1} m/s, {} fragments", w.severity, w.fragments.len());
    assert!(a.damage().broken_up);
    assert!(w.fire.is_none() && w.fireball == 0.0);
    assert!(w.fragments.len() <= 4);
}

/// The obstacles of a kind in the tiles around a point.
fn obstacles_near(terrain: &Terrain, n: f64, e: f64, kind: scenery::ObstacleKind) -> Vec<scenery::Obstacle> {
    let mut list = Vec::new();
    for (size, make) in [(scenery::TREE_TILE, scenery::tree_obstacles as fn(&Terrain, (i32, i32), f64) -> Vec<scenery::Obstacle>), (scenery::HOUSE_TILE, scenery::house_obstacles)] {
        let (x, z) = ((e / size).floor() as i32, (-n / size).floor() as i32);
        for i in -2..=2 {
            for j in -2..=2 {
                list.extend(make(terrain, (x + i, z + j), size).into_iter().filter(|o| o.kind == kind));
            }
        }
    }
    list.sort_by(|a, b| (a.n - n).hypot(a.e - e).total_cmp(&(b.n - n).hypot(b.e - e)));
    list
}

/// Puts the aircraft at a point, heading towards another at a speed, its wheels `height` above the ground.
fn aim(world: &mut World, id: VehicleId, from: (f64, f64), to: (f64, f64), speed: f64, height: f64) {
    let ground = world.terrain.height(from.0, from.1);
    let heading = (to.1 - from.1).atan2(to.0 - from.0);
    let a = &mut world.vehicle_mut(id).aircraft_mut().unwrap();
    a.state = State {
        pos: V3::new(from.0, from.1, -(ground + a.p.cg_height + height)),
        q: quaternion(heading, 0.0, 0.0),
        vel: V3::new(speed, 0.0, 0.0),
        prop_speed: 200.0,
        ..State::default()
    };
    a.controls.throttle = if height > 0.0 { 1.0 } else { 0.4 };
}

/// A tall tree near the home aerodrome of world 1, and its trunk's radius at its foot.
fn tall_tree(world: &World) -> (scenery::Obstacle, f64) {
    let home = world.vehicle(VehicleId(0)).home.unwrap();
    let tree = *obstacles_near(&world.terrain, home.n, home.e + 900.0, scenery::ObstacleKind::Tree)
        .iter()
        .find(|o| o.extent().1 > 12.0)
        .expect("a tall tree near the aerodrome");
    let scenery::Shape::Tree { trunk, .. } = tree.shape else { unreachable!() };
    (tree, trunk)
}

#[test]
fn flying_into_a_trunk_low_down_breaks_it_up_and_the_wreck_stays() {
    // 30 m/s into the trunk of a tall tree, a quarter of the way up: it holds, and the aircraft breaks up.
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Home);
    let (tree, trunk) = tall_tree(&world);
    let cg = world.vehicle(id).aircraft().unwrap().p.cg_height;
    let height = tree.base + tree.extent().1 * 0.25 - world.terrain.height(tree.n - 60.0, tree.e) - cg;
    aim(&mut world, id, (tree.n - 60.0, tree.e), (tree.n, tree.e), 30.0, height.max(1.0));
    for _ in 0..240 * 8 {
        world.step();
    }
    let a = world.vehicle(id).aircraft().unwrap();
    println!("into a trunk of {trunk:.2} m: {:?}, {} wrecks", a.crashed(), world.wrecks.len());
    assert_eq!(a.crashed(), Some("Crashed into a tree"));
    assert!(a.damage().broken_up);
    assert_eq!(world.wrecks.len(), 1);
    assert!(world.wrecks[0].fire.is_some());
    // A new start leaves the wreck where it is.
    world.reset(id, Start::Home);
    assert!(world.vehicle(id).aircraft().unwrap().crashed().is_none());
    assert_eq!(world.wrecks.len(), 1);
}

/// The C172 in level flight at `speed`, its left wing's leading edge `station` metres out lined up with
/// the trunk of `tree`, `ahead` metres short of it, the wing a share `up` of the tree's height above its foot.
fn wing_at_trunk(world: &mut World, id: VehicleId, tree: &scenery::Obstacle, station: f64, speed: f64, ahead: f64, up: f64) {
    let p = &world.vehicle(id).aircraft().unwrap().p;
    let wing_over_wheels = p.cg_height - p.pieces.wing.z.0;
    let (n, e) = (tree.n - ahead, tree.e + station);
    let height = tree.base + tree.extent().1 * up - world.terrain.height(n, e) - wing_over_wheels;
    aim(world, id, (n, e), (tree.n, e), speed, height);
    let a = &mut world.vehicle_mut(id).aircraft_mut().unwrap();
    let t = a.trim_level(speed, -a.state.pos.z).unwrap();
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    a.state.q = quaternion(0.0, t.alpha, 0.0);
    a.state.vel = V3::new(speed * t.alpha.cos(), 0.0, speed * t.alpha.sin());
}

#[test]
fn a_wing_into_a_trunk_is_cut_there_and_the_aircraft_goes_on() {
    // At 35 m/s the left wing meets a thick trunk 3.6 m out: the trunk holds, the wing is cut just
    // inboard of it and falls, and the aircraft goes on, slower, swung and rolled towards the cut (4 m
    // above the ground, it then cartwheels in).
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Home);
    let (tree, trunk) = tall_tree(&world);
    wing_at_trunk(&mut world, id, &tree, 3.6, 35.0, 25.0, 0.3);
    let before = world.vehicle(id).aircraft().unwrap().state.vel.length();
    let mut cut = None;
    for _ in 0..240 * 2 {
        world.step();
        if world.vehicle(id).aircraft().unwrap().damage().wing_torn[0].is_some() {
            cut = Some(world.vehicle(id).aircraft().unwrap().clone());
            break;
        }
    }
    let a = cut.expect("the wing is cut");
    let (after, omega) = (a.state.vel.length(), a.state.omega);
    let station = a.damage().wing_torn[0].unwrap();
    println!("left wing into a trunk of {trunk:.2} m at 3.6 m: cut at {station:.2} m, {before:.1} -> {after:.1} m/s, yaw {:.0}°/s, {:?} {:?}", omega.z / DEG, a.damage().last, a.crashed());
    assert!(station > 3.0 && station < 3.6, "{station}");
    assert!(!a.damage().broken_up);
    assert!(after < before - 0.5 && after > before - 10.0);
    // The trunk held the left wing back: the nose swings left.
    assert!(omega.z < -0.1);
    // The piece that came off is a wreck of its own: a slice of the left wing, out to the tip.
    world.step();
    let piece = world.wrecks.iter().flat_map(|w| &w.pieces).find(|p| p.kind == PieceKind::LeftWing).expect("the torn piece");
    let slice = piece.slice.unwrap();
    assert_eq!((slice.side, slice.from, slice.to), (-1.0, station, a.p.pieces.wing.tip));
    let wing = a.p.pieces.wing;
    assert!((piece.mass - wing.mass * (wing.tip - station) / (wing.tip - wing.root)).abs() < 1e-9);
    let mut bank = 0.0_f64;
    for _ in 0..120 {
        world.step();
        bank = bank.min(euler_angles(world.vehicle(id).aircraft().unwrap().state.q).roll);
    }
    println!("half a second later: {:.0}° of bank", bank / DEG);
    assert!(bank < -20.0 * DEG);
}

#[test]
fn trees_by_energy_a_young_one_is_felled_an_old_one_stops_a_slow_aircraft() {
    // The thinnest trunk near the aerodrome of a tree over 4 m (not a hedge), hit a metre up with the
    // nose at 25 m/s: it is felled, the propeller strikes it, the aircraft goes on slower. The thick
    // trunk of a tall tree at 4 m/s only stops it.
    let run = |speed: f64, young: bool| {
        let mut world = World::new(1);
        let id = world.add_aircraft(AircraftKind::C172, Start::Home);
        let home = world.vehicle(id).home.unwrap();
        let tree = if young {
            *obstacles_near(&world.terrain, home.n, home.e + 900.0, scenery::ObstacleKind::Tree)
                .iter()
                .filter(|o| o.extent().1 > 4.0)
                .min_by(|a, b| a.extent().1.total_cmp(&b.extent().1))
                .expect("a young tree near the aerodrome")
        } else {
            tall_tree(&world).0
        };
        // The nose (the propeller, 1.75 m ahead) a metre above the ground.
        aim(&mut world, id, (tree.n - 3.0 - speed * 0.3, tree.e), (tree.n, tree.e), speed, 0.2);
        let a = &mut world.vehicle_mut(id).aircraft_mut().unwrap();
        a.state.vel.z = 0.0;
        a.controls.throttle = 0.0;
        let before = a.state.vel.length();
        for _ in 0..240 * 2 {
            world.step();
        }
        (world, id, tree, before)
    };
    let (world, id, tree, before) = run(25.0, true);
    let a = world.vehicle(id).aircraft().unwrap();
    let s = a.state.pos;
    println!("into a tree {:.1} m tall at 25 m/s: {:?}, {:?}, {before:.1} -> {:.1} m/s, {:.0} m past it", tree.extent().1, a.damage().last, a.crashed(), a.state.vel.length(), s.x - tree.n);
    assert!(!a.damage().broken_up);
    assert!(a.damage().propeller_bent);
    assert!(s.x > tree.n + 5.0, "it went through");
    let (world, id, _, _) = run(4.0, false);
    let a = world.vehicle(id).aircraft().unwrap();
    println!("into a tall one at 4 m/s: {:?}", a.crashed());
    assert_eq!(a.crashed(), Some("Crashed into a tree"));
    assert!(!a.damage().broken_up && a.state.vel.length() < 0.5);
}

#[test]
fn through_a_crown_it_slows_and_is_not_destroyed() {
    // 30 m/s through the top of a tall tree's crown, where its branches and a thin top are all it
    // meets: the branches drag it back, the top snaps.
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Home);
    let (tree, _) = tall_tree(&world);
    wing_at_trunk(&mut world, id, &tree, 0.0, 30.0, 40.0, 0.85);
    let before = world.vehicle(id).aircraft().unwrap().state.vel.length();
    let mut slowest = before;
    for _ in 0..240 * 2 {
        world.step();
        slowest = slowest.min(world.vehicle(id).aircraft().unwrap().state.vel.length());
    }
    let a = world.vehicle(id).aircraft().unwrap();
    println!("through the crown at 30 m/s: {before:.1} -> {slowest:.1} m/s, {:?}, {:?}", a.damage().last, a.crashed());
    assert!(!a.damage().broken_up);
    assert!(slowest < before - 2.0, "the branches slow it");
}

#[test]
fn a_tip_on_the_ground_is_scraped_or_torn_and_the_rest_stays() {
    // At 20 m/s, banked 35° just over the ground, the left tip comes down onto it: at 1 m/s it is
    // scraped; at 4 m/s the outer strip is crushed and comes off, and the aircraft does not break up.
    let drop = |kind: AircraftKind, sink: f64| {
        let mut a = Aircraft::of(kind);
        let env = ground_env();
        let tip = a.p.hard_points[0];
        a.state.q = quaternion(0.0, a.p.ground_pitch, -35.0 * DEG);
        a.state.pos = V3::new(0.0, 0.0, -0.03 - act(a.state.q, tip).z);
        a.state.vel = act(inverse(a.state.q), V3::new(20.0, 0.0, sink));
        for _ in 0..60 {
            a.step(DT, &env);
        }
        a
    };
    for kind in AircraftKind::all() {
        let soft = drop(kind, 1.0);
        println!("{kind:?} tip at 1 m/s: {:?}, {:?}", soft.damage().last, soft.damage().wing_torn);
        assert_eq!(soft.damage().wing_torn, [None, None]);
        let hard = drop(kind, 4.0);
        println!("{kind:?} tip at 4 m/s: {:?}, {:?}, {:?}", hard.damage().last, hard.damage().wing_torn, hard.crashed());
        let end = hard.damage().wing_torn[0].expect("the outer wing is torn off");
        assert!(end > hard.p.pieces.wing.root, "{end}: not all of it");
        assert!(!hard.damage().broken_up);
    }
}

/// An aircraft of a kind in trimmed level flight at `kt`, 1500 m up.
fn flying_kind(kind: AircraftKind, kt: f64) -> (Aircraft, Environment) {
    let mut a = Aircraft::of(kind);
    let altitude = 1500.0;
    let t = a.trim_level(kt * KNOTS, altitude).unwrap();
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(kt * KNOTS * t.alpha.cos(), 0.0, kt * KNOTS * t.alpha.sin()),
        q: quaternion(0.0, t.alpha, 0.0),
        prop_speed: t.prop_speed,
        blade_pitch: t.blade_pitch,
        ..State::default()
    };
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    (a, Environment { rho: atmosphere::density(altitude), ..Environment::default() })
}

#[test]
fn losing_the_outer_wing_in_flight_rolls_it_towards_the_loss_and_the_ailerons_can_hold_it() {
    // The outer strip of the left wing torn off in cruise: lighter, the centre of gravity to the
    // right, less lift on the left. Hands off it rolls left; the right stick holds the wings level.
    for (kind, kt) in [(AircraftKind::C172, 100.0), (AircraftKind::EXTRA300, 140.0)] {
        let (mut a, env) = flying_kind(kind, kt);
        let (mass, cg) = (a.mass(), a.cg());
        let station = a.p.strip_edges[3];
        a.tear_wing(0, station, 1.0, "test");
        let piece = a.take_wrecks();
        assert_eq!(piece.len(), 1);
        assert!(piece[0].fire.is_none() && piece[0].fireball == 0.0);
        assert!(a.mass() < mass - 5.0 && a.cg().y > cg.y);
        assert!(a.crashed().is_none(), "it still flies");
        let mut hands_off = a.clone();
        for _ in 0..240 * 2 {
            hands_off.step(DT, &env);
        }
        let bank = euler_angles(hands_off.state.q).roll;
        // Holding the wings level with the ailerons alone.
        let (mut stick, mut trim) = (0.0_f64, 0.0);
        for _ in 0..240 * 10 {
            let roll = euler_angles(a.state.q).roll;
            trim -= roll * DT;
            a.controls.aileron = clamp(trim - 2.0 * roll - 0.5 * a.state.omega.x, -1.0, 1.0);
            a.step(DT, &env);
            stick = stick.max(a.controls.aileron);
        }
        let held = euler_angles(a.state.q).roll;
        println!("{kind:?} without {:.1} m of left wing at {kt} kt: hands off {:.0}° of bank in 2 s; held at {:.1}° with up to {:.0}% of right stick", a.p.pieces.wing.tip - station, bank / DEG, held / DEG, stick * 100.0);
        assert!(bank < -10.0 * DEG);
        assert!(held.abs() < 3.0 * DEG && stick < 0.9);
    }
}

#[test]
fn a_wing_torn_inboard_of_the_tank_bursts_it() {
    let (mut a, _) = flying(100.0);
    let fuel = a.load().fuel;
    a.tear_wing(1, 2.0, 1.0, "test");
    println!("right wing torn at 2 m: fuel {fuel:.0} -> {:.0} kg, {:?}", a.load().fuel, a.damage().tank_burst);
    assert_eq!(a.damage().tank_burst, [false, true]);
    assert!((a.load().fuel - fuel / 2.0).abs() < 1e-9);
    // Torn off in the air: what spilt does not burn.
    let w = &a.take_wrecks()[0];
    assert!(w.fire.is_none() && w.fireball == 0.0);
}

#[test]
fn half_the_tailplane_torn_off_pitches_and_rolls_it() {
    // Without the outer half of the right half of the tailplane: less tail lift (the nose drops,
    // as the trim needs more of it) and a roll from the lift the left half keeps.
    let (mut a, env) = flying(100.0);
    let span = a.p.pieces.stabiliser;
    let pitch = euler_angles(a.state.q).pitch;
    a.tear_stabiliser(1, (span.root + span.tip) / 2.0, 1.0, "test");
    assert_eq!(a.take_wrecks()[0].pieces[0].kind, PieceKind::Tail);
    let d = a.forces_settled(&a.state.clone(), &a.controls.clone(), &env, 0.0);
    for _ in 0..240 * 2 {
        a.step(DT, &env);
    }
    println!("half the right tailplane gone: pitch moment {:.0} N·m, roll moment {:.0} N·m; pitch {:.1}° -> {:.1}°", d.moment.y, d.moment.x, pitch / DEG, euler_angles(a.state.q).pitch / DEG);
    assert!(d.moment.y.abs() > 100.0);
    assert!(d.moment.x.abs() > 10.0);
}

#[test]
fn a_hard_tail_strike_slackens_the_cables() {
    // Tail first onto the runway at 3.4 m/s, 14° nose up (a flare held too long, then dropped in):
    // the tail cone buckles, the elevator and the rudder go slack; the gear holds.
    let mut a = Aircraft::new();
    let env = ground_env();
    let skid = a.p.hard_points[6];
    a.state.q = quaternion(0.0, 14.0 * DEG, 0.0);
    a.state.pos = V3::new(0.0, 0.0, -0.05 - act(a.state.q, skid).z);
    a.state.vel = act(inverse(a.state.q), V3::new(20.0, 0.0, 3.4));
    for _ in 0..240 {
        a.step(DT, &env);
    }
    println!("tail strike at 3.4 m/s: {:?}, {:?} {:?}", a.damage().last, a.damage().elevator, a.damage().rudder);
    assert_eq!(a.damage().elevator, SurfaceState::Loose);
    assert!(!a.damage().broken_up && !a.damage().gear_broken.contains(&true));
}

#[test]
fn the_same_damage_is_the_same_damage() {
    let run = || {
        let mut world = World::new(1);
        let id = world.add_aircraft(AircraftKind::C172, Start::Home);
        let (tree, _) = tall_tree(&world);
        wing_at_trunk(&mut world, id, &tree, 3.6, 35.0, 25.0, 0.3);
        for _ in 0..240 * 6 {
            world.step();
        }
        (*world.vehicle(id).aircraft().unwrap().damage(), world.wrecks.clone(), world.vehicle(id).aircraft().unwrap().state)
    };
    assert_eq!(run(), run());
}

#[test]
fn taxiing_into_a_hangar_stops_against_it() {
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Home);
    let home = world.vehicle(id).home.unwrap();
    let hangar = obstacles_near(&world.terrain, home.n, home.e, scenery::ObstacleKind::Hangar)[0];
    // Rolling at 3 m/s from 40 m away, towards its middle.
    let (dn, de) = (home.n - hangar.n, home.e - hangar.e);
    let d = dn.hypot(de);
    aim(&mut world, id, (hangar.n + dn / d * 40.0, hangar.e + de / d * 40.0), (hangar.n, hangar.e), 3.0, 0.0);
    for _ in 0..240 * 30 {
        world.step();
    }
    let a = world.vehicle(id).aircraft().unwrap();
    let s = a.state.pos;
    println!("taxiing into a hangar: {:?}, stopped {:.1} m from its middle", a.crashed(), (s.x - hangar.n).hypot(s.y - hangar.e));
    assert_eq!(a.crashed(), Some("Crashed into a hangar"));
    assert!(!a.damage().broken_up);
    assert!(a.state.vel.length() < 0.5);
}

#[test]
fn two_aircraft_that_meet_break_up() {
    let run = || {
        let mut world = World::new(1);
        let a = world.add_aircraft(AircraftKind::C172, Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
        let b = world.add_aircraft(AircraftKind::C172, Start::Air { n: 600.0, e: 0.0, heading: std::f64::consts::PI });
        // Level with each other.
        let z = world.vehicle(a).aircraft().unwrap().state.pos.z;
        world.vehicle_mut(b).aircraft_mut().unwrap().state.pos.z = z;
        for _ in 0..240 * 10 {
            world.step();
        }
        world
    };
    let world = run();
    for plane in &world.vehicles {
        println!("head on: {:?}", plane.aircraft().unwrap().crashed());
        assert_eq!(plane.aircraft().unwrap().crashed(), Some("Collision with another aircraft"));
    }
    assert_eq!(world.wrecks.len(), 2);
    // The same collision is the same wreck.
    assert_eq!(run().wrecks, world.wrecks);
}
