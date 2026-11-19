//! Race circuits: the built-in one (`assets/circuits/lakeside.ron`) read and laid out on its
//! world, the ground under it, its walls, and the racing driver lapping it cleanly in the GT coupé.
//! `cargo test -p sim-core --test circuit -- --nocapture` prints the figures and the laps.

use std::sync::Arc;

use sim_core::car::driver::Driver;
use sim_core::car::{CarGround, Surface, TerrainGround};
use sim_core::circuit::{self, Circuit, CircuitFile};
use sim_core::{CarKind, Start, Terrain, World};

fn lakeside() -> (Arc<Circuit>, Arc<Terrain>) {
    let file = CircuitFile::find("lakeside").expect("the built-in circuit");
    let circuit = Arc::new(Circuit::new(file, &Terrain::new(file.seed())));
    let terrain = Arc::new(Terrain::new(file.seed()).with_circuits(vec![circuit.clone()]));
    (circuit, terrain)
}

#[test]
fn files_read_and_say_what_is_wrong() {
    assert!(circuit::circuits().iter().any(|c| c.key == "lakeside"));
    let broken = circuit::load("(name: \"x\", place: Seed(1), origin: (0, 0), width: 12, start: 0, laps: 1, sides: (runoff_width: 10), points: [(n: 0, e: 0), (n: 100, e: 0)])", "assets/circuits/x.ron");
    assert!(broken.unwrap_err().message.contains("four points"));
    let misspelt = circuit::load("(name: \"x\", place: Seed(1), origin: (0, 0), widht: 12)", "x.ron");
    assert!(misspelt.is_err());
}

#[test]
fn laid_out_on_its_world() {
    let (c, terrain) = lakeside();
    let (lo, hi) = c.samples.iter().fold((f64::MAX, f64::MIN), |(a, b), s| (a.min(s.height), b.max(s.height)));
    let tightest = c.samples.iter().map(|s| 1.0 / s.curvature.abs().max(1e-6)).fold(f64::MAX, f64::min);
    println!("{}: {:.0} m round, {:.0} m of climb from {:.0} to {:.0} m, tightest corner {:.0} m of radius", c.name, c.length, hi - lo, lo, hi, tightest);
    assert!((3000.0..=5000.0).contains(&c.length));
    assert!(hi - lo > 15.0, "it rises and falls");
    assert!(tightest < 30.0, "a hairpin");
    // Closed: the last sample leads to the first.
    let (a, b) = (c.samples.last().unwrap(), &c.samples[0]);
    assert!((a.n - b.n).hypot(a.e - b.e) < circuit::STEP * 1.5);
    // No part of it comes near another, but where it turns back on itself.
    let count = c.samples.len();
    for i in (0..count).step_by(4) {
        for j in (0..count).step_by(4) {
            let apart = (i as f64 - j as f64).abs() * circuit::STEP;
            if apart.min(c.length - apart) > 300.0 {
                let d = (c.samples[i].n - c.samples[j].n).hypot(c.samples[i].e - c.samples[j].e);
                assert!(d > 60.0, "{:.0} m and {:.0} m along are {d:.0} m apart", i as f64 * circuit::STEP, j as f64 * circuit::STEP);
            }
        }
    }
    // The ground under it is laid a little below its surface, and nothing grows on it.
    let ground = TerrainGround::dry(&terrain);
    for s in c.samples.iter().step_by(25) {
        let patch = ground.patch(s.n, s.e);
        assert_eq!(patch.surface, Surface::Asphalt);
        let under = terrain.height(s.n, s.e);
        assert!(patch.height - under > 0.2 && patch.height - under < 0.5, "at {:.0} m the terrain is {:.2} m below", s.along, patch.height - under);
        assert!(terrain.ground(s.n, s.e).flat > 0.99);
    }
    let tile = 256.0;
    let (s0, s1) = (&c.samples[0], &c.samples[count / 2]);
    for s in [s0, s1] {
        let key = ((s.e / tile).floor() as i32, ((-s.n) / tile).floor() as i32);
        for tree in sim_core::scenery::tree_obstacles(&terrain, key, tile) {
            assert!(c.locate(tree.n, tree.e).is_none_or(|at| at.across.abs() > 25.0), "a tree on the circuit at {:.0}, {:.0}", tree.n, tree.e);
        }
    }
}

#[test]
fn walls_where_the_file_puts_them() {
    let (c, _) = lakeside();
    let walls = c.all_walls();
    assert!(walls.len() > 1000, "{} pieces of barrier", walls.len());
    // Every barrier faces the track: its normal points to the centre line.
    for w in walls.iter().step_by(37) {
        let (mn, me) = ((w.a.0 + w.b.0) / 2.0, (w.a.1 + w.b.1) / 2.0);
        let at = c.locate(mn, me).expect("near the circuit");
        let s = c.sample(at.along);
        let to_centre = (s.n - mn, s.e - me);
        assert!(to_centre.0 * w.normal.0 + to_centre.1 * w.normal.1 > 0.0);
    }
}

#[test]
fn the_racing_driver_laps_it_cleanly() {
    let (circuit, terrain) = lakeside();
    let mut world = World::with_terrain(1, terrain);
    let (n, e, heading) = circuit.grid(0);
    let id = world.add_car(CarKind::GT, Start::Air { n, e, heading });
    let mut driver = Driver::new(circuit.clone(), CarKind::GT.spec());
    let mut last = circuit.from_start(circuit.locate(n, e).unwrap().along);
    let (mut laps, mut lap_start) = (0, 0.0);
    let mut times = Vec::new();
    let mut widest: f64 = 0.0;
    while laps < 4 && world.time() < 500.0 {
        world.fly(id, &mut driver);
        world.step();
        let car = world.vehicle(id).car().unwrap();
        assert!(car.crashed().is_none(), "{:?} at {:.0} s", car.crashed(), world.time());
        let b = car.body;
        let at = circuit.locate(b.pos.x, b.pos.y).expect("on the circuit");
        // Never more than a kerb's width off the asphalt.
        let edge = circuit.sample(at.along).width / 2.0;
        widest = widest.max(at.across.abs() - edge);
        let along = circuit.from_start(at.along);
        if along < last - circuit.length / 2.0 {
            if laps > 0 {
                times.push(world.time() - lap_start);
            }
            laps += 1;
            lap_start = world.time();
        }
        last = along;
    }
    println!("laps: {times:.2?} s; at most {widest:.2} m beyond the asphalt's edge");
    // From the grid, then two flying laps.
    assert_eq!(times.len(), 3, "three laps");
    assert!(widest < circuit::KERB_WIDTH + 0.5, "{widest:.2} m off the asphalt");
    assert!((times[2] - times[1]).abs() < 1.0, "the same flying lap twice");
}

#[test]
fn cars_knock_apart() {
    // One car runs into the back of another at 6 m/s on the main straight: they part, neither is
    // wrecked, and the one ahead is pushed on.
    let (circuit, terrain) = lakeside();
    let mut world = World::with_terrain(1, terrain);
    let (n, e, _, heading) = circuit.at(100.0);
    let (n2, e2, _, _) = circuit.at(110.0);
    let behind = world.add_car(CarKind::GT, Start::Air { n, e, heading });
    let ahead = world.add_car(CarKind::GT, Start::Air { n: n2, e: e2, heading });
    for id in [behind, ahead] {
        world.vehicle_mut(id).car_mut().unwrap().set_gear(0);
    }
    let b = world.vehicle(behind).body();
    world.vehicle_mut(behind).car_mut().unwrap().put(sim_core::Body { vel: sim_core::V3::new(6.0, 0.0, 0.0), ..b });
    world.vehicle_mut(behind).car_mut().unwrap().set_gear(0);
    for _ in 0..(4.0 / World::DT) as usize {
        world.step();
    }
    let (a, b) = (world.vehicle(behind).car().unwrap(), world.vehicle(ahead).car().unwrap());
    let gap = (a.body.pos - b.body.pos).length();
    println!("after the knock: {gap:.1} m apart, the one ahead at {:.1} m/s, the one behind at {:.1} m/s", b.data().speed, a.data().speed);
    assert!(a.crashed().is_none() && b.crashed().is_none());
    assert!(gap > 4.4, "they part: {gap:.2} m");
    assert!(b.data().speed > 1.0 || (b.body.pos - sim_core::V3::new(n2, e2, b.body.pos.z)).length() > 1.0, "the one ahead was pushed");
}
