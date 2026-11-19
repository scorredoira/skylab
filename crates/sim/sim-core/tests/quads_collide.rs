//! Two FPV quads that touch (`multirotor::collide_multirotors`): frames that meet knock each other
//! apart once, keeping their momentum, and never stick; propellers that strike at speed break, the
//! quad tumbles and comes down; a slow brush of the tips only chips them, and it flies on with a
//! little less thrust on those rotors, shaking. `cargo test -p sim-core --test quads_collide --
//! --nocapture` prints the measurements.

use sim_core::{act, MultirotorKind, Start, State, VehicleId, World, V3};

const QUAD: MultirotorKind = MultirotorKind::FPV5;

/// A world with two quads hovering over the runway nearest the origin, the throttle where they
/// hover, facing north; the second is then put `offset` from the first (north, east, down),
/// moving at `velocity` over the ground.
fn two(offset: V3, velocity: V3) -> (World, VehicleId, VehicleId) {
    let mut world = World::new(1);
    let a = world.terrain.nearest_airport(0.0, 0.0).expect("an aerodrome");
    let start = Start::Air { n: a.n, e: a.e, heading: 0.0 };
    let first = world.add_multirotor(QUAD, start);
    let second = world.add_multirotor(QUAD, start);
    let b = world.vehicle(first).body();
    // Facing north, level: body axes are the world's.
    world.place(second, State { pos: b.pos + offset, q: b.q, vel: velocity, ..State::default() });
    (world, first, second)
}

fn run(world: &mut World, seconds: f64) {
    for _ in 0..(seconds / World::DT).round() as usize {
        world.step();
    }
}

fn blades(world: &World, id: VehicleId) -> Vec<[f64; 2]> {
    world.vehicle(id).multirotor().unwrap().blades().to_vec()
}

/// What is left of a propeller's thrust, as the model takes it: each blade's radius cubed.
fn thrust_left(b: [f64; 2]) -> f64 {
    (b[0].powi(3) + b[1].powi(3)) / 2.0
}

fn momentum(world: &World, ids: [VehicleId; 2]) -> V3 {
    ids.iter().map(|id| world.vehicle(*id).body().world_velocity() * world.vehicle(*id).multirotor().unwrap().p.mass).sum()
}

/// One quad sinking at 0.5 m/s onto another hovering under it: their frames meet (the arms of the
/// upper on the battery of the lower), with no propeller in it. One knock: the speed between them
/// changes at once, their momentum the same through it, and they move apart; nothing breaks,
/// nothing sticks (the old way held them together, slowed every step).
#[test]
fn frames_that_meet_knock_each_other_apart() {
    let (mut world, low, high) = two(V3::new(0.0, 0.0, -0.12), V3::new(0.0, 0.0, 0.5));
    let ids = [low, high];
    let between = |w: &World| w.vehicle(high).body().pos - w.vehicle(low).body().pos;
    let rel = |w: &World| w.vehicle(high).body().world_velocity() - w.vehicle(low).body().world_velocity();
    let mut knocks = 0;
    for step in 0..(2.0 / World::DT) as usize {
        let (before, closing) = (momentum(&world, ids), rel(&world));
        world.step();
        let (after, now) = (momentum(&world, ids), rel(&world));
        if (now - closing).length() > 0.1 {
            knocks += 1;
            let d = between(&world).normalize();
            println!(
                "knock after {:.3} s, {:.3} m apart: closing at {:.2} m/s, then opening at {:.2}; momentum {:.4} -> {:.4} N s",
                step as f64 * World::DT,
                between(&world).length(),
                -closing.dot(d),
                now.dot(d),
                before.length(),
                after.length()
            );
            // The impulse is some 0.2 N s each way; their sum stays (gravity and thrust nearly cancel over a step).
            assert!((after - before).length() < 0.005, "momentum {before} -> {after}");
            assert!(now.dot(d) > 0.0, "they part");
        }
    }
    let (l, h) = (world.vehicle(low), world.vehicle(high));
    println!(
        "{knocks} knock; 2 s later: {:.2} m apart, the upper rising {:.2} m/s, the lower sinking {:.2} m/s",
        between(&world).length(),
        -h.body().world_velocity().z,
        l.body().world_velocity().z
    );
    assert_eq!(knocks, 1);
    assert!(between(&world).length() > 0.2, "they moved apart");
    for id in ids {
        assert!(world.vehicle(id).dynamics.crashed().is_none());
        assert!(blades(&world, id).iter().all(|b| *b == [1.0, 1.0]), "no propeller touched");
    }
}

/// One quad flying sideways at 3 m/s into another hovering: their propellers meet. Both break on
/// each (both blades to stubs, its thrust gone), and the quads that lost them tumble and fall; on
/// the ground they are crashed (from 15 m, by the fall itself). Neither stays in the air.
#[test]
fn propellers_that_strike_break_and_the_quads_fall() {
    let (mut world, hit, hitter) = two(V3::new(0.0, 0.40, 0.0), V3::new(0.0, -3.0, 0.0));
    let height = |w: &World, id: VehicleId| -w.vehicle(id).body().pos.z;
    let start = height(&world, hit);
    run(&mut world, 0.3);
    let stub = world.vehicle(hit).multirotor().unwrap().p.strike.stub;
    // The hovering one's right rotors (rear and front right), the other's left ones.
    let (struck, striker) = (blades(&world, hit), blades(&world, hitter));
    println!("after 0.3 s: struck {struck:?}, striker {striker:?}");
    for (b, side) in [(struck[0], "hit's rear right"), (struck[1], "hit's front right"), (striker[2], "hitter's rear left"), (striker[3], "hitter's front left")] {
        assert_eq!(b, [stub; 2], "{side} broken");
        assert!(thrust_left(b) < 0.02);
    }
    assert!(thrust_left(struck[2]) == 1.0 && thrust_left(striker[0]) == 1.0, "the far side's whole");
    let mut tilt: f64 = 0.0;
    let mut crashed_at = None;
    for step in 0..(8.0 / World::DT) as usize {
        world.step();
        tilt = tilt.max(act(world.vehicle(hit).body().q, V3::Z).z.acos());
        if crashed_at.is_none() && world.vehicle(hit).dynamics.crashed().is_some() {
            crashed_at = Some(0.3 + step as f64 * World::DT);
        }
    }
    println!("tilted up to {:.0}°, fell {:.1} m, crashed after {crashed_at:?} s: {:?}", tilt.to_degrees(), start - height(&world, hit), world.vehicle(hit).dynamics.crashed());
    assert!(tilt > 60f64.to_radians(), "it tumbles: {}", tilt.to_degrees());
    for id in [hit, hitter] {
        assert!(world.vehicle(id).dynamics.crashed().is_some());
        assert!(world.vehicle(id).agl() < 1.0, "down on the ground");
    }
}

/// One quad brushing past another, its front left propeller's tip 3 mm over the other's rear right
/// one, and moving on and away (1 m/s ahead, 0.15 m/s aside): those two tips are chipped, not
/// broken, the other propellers whole. Both fly on, their height kept within a metre over 3 s,
/// with a little less thrust on those rotors, and shaking more than a whole quad does.
#[test]
fn a_light_brush_only_chips_the_tips() {
    let shake = |world: &mut World, id: VehicleId| {
        let mut sum = 0.0;
        let n = (3.0 / World::DT) as usize;
        for _ in 0..n {
            world.step();
            sum += world.vehicle(id).body().omega.length_squared();
        }
        (sum / n as f64).sqrt()
    };
    // Abreast of each other's rotors, the rims 2 × 63.5 mm apart less 3 mm.
    let (mut world, hit, by) = two(V3::new(-2.0 * 0.0795, 2.0 * 0.0795 + 0.127 - 0.003, 0.0), V3::new(1.0, 0.15, 0.0));
    let start = world.vehicle(hit).body().pos.z;
    run(&mut world, 0.6);
    let (struck, brushed) = (blades(&world, hit), blades(&world, by));
    println!("chipped: {struck:?} and {brushed:?}");
    for b in [struck[0], brushed[3]] {
        assert!(b[0].min(b[1]) < 0.97 && b[0] != b[1], "chipped, unevenly: {b:?}");
        assert!((0.85..0.99).contains(&thrust_left(b)), "less thrust: {}", thrust_left(b));
    }
    assert!([struck[1], struck[2], struck[3], brushed[0], brushed[1], brushed[2]].iter().all(|b| *b == [1.0, 1.0]));
    let chipped = shake(&mut world, hit);
    let (mut alone, whole, _) = two(V3::new(0.0, 20.0, 0.0), V3::ZERO);
    run(&mut alone, 0.6);
    let steady = shake(&mut alone, whole);
    let sunk = world.vehicle(hit).body().pos.z - start;
    println!("over 3 s: rates {chipped:.3} rad/s rms chipped, {steady:.3} whole; sank {sunk:.2} m");
    assert!(chipped > 3.0 * steady && chipped > 0.05, "it shakes: {chipped} against {steady}");
    for id in [hit, by] {
        assert!(world.vehicle(id).dynamics.crashed().is_none(), "flies on");
    }
    assert!(sunk.abs() < 1.0, "height kept: {sunk}");
}
