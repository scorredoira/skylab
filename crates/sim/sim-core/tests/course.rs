//! Race courses: the files, the gates as obstacles, the race's count, the racer flying the
//! built-in courses round, every gate, proving them flyable; and Deepwood's trail cut through
//! the forest.

use std::sync::Arc;

use sim_core::course::racer::Racer;
use sim_core::course::{self, Crossing, Event, GateKind, Race, Track};
use sim_core::{euler_angles, MultirotorKind, Start, World, V3};

/// A world with the course's track in it and a quad on its start, on the ground.
fn racing(key: &str) -> (World, Arc<Track>, sim_core::VehicleId) {
    let course = course::file::course(key).expect("the course is built in");
    let mut world = World::new(1);
    let track = Arc::new(Track::new(course, &world.terrain));
    world.obstacles = Arc::new(track.obstacles());
    // In the course's own quad (Deepwood's race quad), else the FPV quad.
    let kind = course.quad.as_deref().and_then(MultirotorKind::parse).unwrap_or(MultirotorKind::FPV5);
    let id = world.add_multirotor(kind, Start::Air { n: track.start.x, e: track.start.y, heading: track.start_heading });
    let (terrain, weather, earth) = (world.terrain.clone(), world.weather, world.earth);
    world.vehicles[id.0].reset_on_ground(&terrain, &weather, &earth, track.start.x, track.start.y, track.start_heading);
    (world, track, id)
}

/// Flies the racer round; the events the referee saw, and the time.
fn fly(world: &mut World, track: &Arc<Track>, id: sim_core::VehicleId, seconds: f64, verbose: bool) -> (Vec<(f64, Event)>, Race) {
    let p = world.vehicle(id).multirotor().unwrap().p.clone();
    let mut racer = Racer::new(track.clone(), &p);
    let mut referee = Race::new(track.course.laps);
    let start = world.vehicle(id).body().pos;
    racer.go(world.time(), start);
    referee.go(world.time(), start);
    let mut events = Vec::new();
    let mut last_print = -10.0;
    while world.time() < seconds {
        world.fly(id, &mut racer);
        world.step();
        let body = world.vehicle(id).body();
        for e in referee.update(track, body.pos, world.time()) {
            if verbose {
                println!("{:6.2} s {e:?}", world.time());
            }
            events.push((world.time(), e));
        }
        let every = std::env::var("RACE_PRINT").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
        if verbose && world.time() - last_print >= every {
            last_print = world.time();
            let v = body.world_velocity();
            let (to, _) = track.target(referee.next);
            let local = track.gates[referee.next].local(body.pos);
            println!(
                "{:5.1} s pos ({:7.1},{:7.1}) local ({:6.1},{:6.1},{:5.1}) agl {:5.1} speed {:5.1} next {} at {:5.1} m heading {:4.0}",
                world.time(),
                body.pos.x,
                body.pos.y,
                local.x,
                local.y,
                local.z,
                world.vehicle(id).agl(),
                v.length(),
                referee.next,
                (to - body.pos).length(),
                euler_angles(body.q).heading.to_degrees()
            );
        }
        if let Some(why) = world.vehicle(id).dynamics.crashed() {
            panic!("crashed at {:.1} s, next gate {}: {why} at {:?}", world.time(), referee.next, body.pos);
        }
        if referee.finished.is_some() {
            break;
        }
    }
    (events, referee)
}

#[test]
fn the_built_in_course_and_the_gates_read() {
    let shapes = course::shapes();
    assert_eq!(shapes.len(), GateKind::ALL.len());
    for (shape, kind) in shapes.iter().zip(GateKind::ALL) {
        assert_eq!(shape.kind, kind);
        assert!(!shape.bars().is_empty());
    }
    let c = course::file::course("pinewood").unwrap();
    assert_eq!(c.gates[0].kind, GateKind::Arch);
    assert!(c.gates.len() >= 10);
    // A broken file says what is wrong.
    let e = course::file::load("(name: \"x\")", "bad.ron").unwrap_err();
    assert!(e.to_string().starts_with("bad.ron:"), "{e}");
}

#[test]
fn a_gate_counts_only_through_its_opening_the_right_way() {
    let (world, track, _) = racing("pinewood");
    let _ = world;
    let g = &track.gates[1];
    let shape = track.shape(1);
    let (middle, through) = track.target(1);
    let (_, right, _) = g.axes();
    assert_eq!(g.crossed(shape, middle - through, middle + through, 10.0), Some(Crossing::Through));
    assert_eq!(g.crossed(shape, middle + through, middle - through, 10.0), None, "the wrong way");
    let beside = middle + right * 3.0;
    assert!(matches!(g.crossed(shape, beside - through, beside + through, 10.0), Some(Crossing::Outside(_))));
    assert_eq!(g.crossed(shape, beside - through + right * 20.0, beside + through + right * 20.0, 10.0), None);
}

#[test]
fn the_frame_is_solid_and_the_opening_clear() {
    let (mut world, track, id) = racing("pinewood");
    // Straight at the second gate's side post at 15 m/s: crashed.
    let g = track.gates[1];
    let (f, r, _) = g.axes();
    let post = g.to_world(V3::new(0.0, -1.56, 2.0));
    let mut s = sim_core::State { pos: post - f * 3.0 + r * 0.0, q: sim_core::quaternion(g.heading, 0.0, 0.0), ..sim_core::State::default() };
    s.vel = V3::new(15.0, 0.0, 0.0);
    world.place(id, s);
    for _ in 0..240 {
        world.step();
    }
    assert_eq!(world.vehicle(id).dynamics.crashed(), Some("Crashed into a gate"));
}

#[test]
fn the_racer_flies_the_course_through_every_gate() {
    let (mut world, track, id) = racing("pinewood");
    let length = track.lap_length();
    let (events, race) = fly(&mut world, &track, id, 400.0, true);
    let missed = events.iter().filter(|(_, e)| matches!(e, Event::Missed { .. })).count();
    println!("lap {length:.0} m; laps {:?}, total {:?}, missed {missed}", race.lap_times, race.finished);
    assert!(race.finished.is_some(), "it finished");
    assert_eq!(missed, 0);
    assert_eq!(race.passed as usize, track.gates.len() * race.laps as usize + 1);
}

#[test]
fn the_racer_flies_deepwood_through_every_gate() {
    let (mut world, track, id) = racing("deepwood");
    assert_eq!(world.vehicle(id).multirotor().unwrap().p.kind, MultirotorKind::RACE5);
    let (events, race) = fly(&mut world, &track, id, 300.0, false);
    let missed = events.iter().filter(|(_, e)| matches!(e, Event::Missed { .. })).count();
    println!("Deepwood: lap {:.0} m; laps {:?}, total {:?}, missed {missed}", track.lap_length(), race.lap_times, race.finished);
    assert!(race.finished.is_some(), "it finished");
    assert_eq!(missed, 0);
}

/// Deepwood's trail: no trunk stands in it, no low crown reaches over it, and no tree stands in
/// a gate's frame; round it the wood is thick.
#[test]
fn deepwood_has_its_trail_cut_through_a_thick_wood() {
    use sim_core::scenery::{self, Shape};
    let world = World::new(1);
    let track = Track::new(course::file::course("deepwood").unwrap(), &world.terrain);
    let cuts = course::cuts(&world.terrain);
    assert!(cuts.len() >= track.gates.len(), "{} stretches", cuts.len());
    let mut seen = std::collections::BTreeSet::new();
    for c in &cuts {
        for k in 0..=8 {
            let t = k as f64 / 8.0;
            let (n, e) = (c.from.0 + (c.to.0 - c.from.0) * t, c.from.1 + (c.to.1 - c.from.1) * t);
            seen.insert(((e / 128.0).floor() as i32, (-n / 128.0).floor() as i32));
        }
    }
    let gate_bars = track.obstacles();
    let mut near = 0;
    for key in &seen {
        for o in scenery::tree_obstacles(&world.terrain, *key, 128.0) {
            let Shape::Tree { crown, radius, .. } = o.shape else { continue };
            for c in &cuts {
                let (d, half) = c.distance(o.n, o.e);
                assert!(d >= half, "a trunk {d:.1} m from the trail's line, {half:.1} m half wide");
                if crown < c.clearance {
                    assert!(d - radius >= half - 1e-6, "a low crown over the trail at n {:.0} e {:.0}", o.n, o.e);
                }
            }
            for b in &gate_bars {
                let (gap, _, _) = b.surface(o.n, o.e, o.base + 1.0);
                assert!(gap > 0.3, "a tree in a gate's frame at n {:.0} e {:.0}", o.n, o.e);
            }
            if cuts.iter().any(|c| c.distance(o.n, o.e).0 < 30.0) {
                near += 1;
            }
        }
    }
    println!("trees within 30 m of the trail's line: {near}");
    assert!(near > 300, "{near} trees by the trail");
}
