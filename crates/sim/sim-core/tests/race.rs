//! Races of racing drivers (`car::driver::Driver` with a `Character` each) round Lakeside Park
//! from a standing start (`circuit::race::Race`): they finish, keep off each other and pass, and
//! a wet race is slower. `cargo test -p sim-core --release --test race -- --nocapture` prints each
//! race; `-- --ignored many_races` runs many and prints how often cars touched.

use std::sync::Arc;

use sim_core::car::driver::{Character, Driver};
use sim_core::circuit::race::{Phase, Race};
use sim_core::circuit::{Circuit, CircuitFile};
use sim_core::{CarKind, Start, Terrain, VehicleId, World};

fn lakeside() -> (Arc<Circuit>, Arc<Terrain>) {
    let file = CircuitFile::find("lakeside").expect("the built-in circuit");
    let circuit = Arc::new(Circuit::new(file, &Terrain::new(file.seed())));
    let terrain = Arc::new(Terrain::new(file.seed()).with_circuits(vec![circuit.clone()]));
    (circuit, terrain)
}

/// What came of a race.
#[derive(Debug, Default)]
struct Outcome {
    /// Finishing order (car index on the grid), times, best laps.
    order: Vec<usize>,
    times: Vec<Option<f64>>,
    best: Vec<Option<f64>>,
    /// Times two cars touched (a new touch after a second apart), and the hardest, m/s.
    contacts: usize,
    hardest: f64,
    /// Cars wrecked.
    wrecked: usize,
    /// Passes: a car moving up a place, between cars that both finished.
    passes: usize,
    /// Each car wrecked: which, when, where (along, across), why.
    wrecks: Vec<(usize, f64, f64, f64, String)>,
}

/// A race of `cars` from the grid, `laps` long, in rain of `rain`; the grid's skills shuffled by `seed`.
fn race(cars: usize, laps: u32, rain: f64, seed: u64) -> Outcome {
    let (circuit, terrain) = lakeside();
    let mut world = World::with_terrain(1, terrain);
    world.weather.rain = rain;
    world.settle_water();
    let mut ids: Vec<VehicleId> = Vec::new();
    let mut drivers = Vec::new();
    for k in 0..cars {
        let (n, e, heading) = circuit.grid(k);
        let id = world.add_car(CarKind::GT, Start::Air { n, e, heading });
        world.vehicle_mut(id).car_mut().unwrap().controls.abs = true;
        ids.push(id);
        // The quicker ones not always at the front.
        let skill = ((k as u64 * 7 + seed * 3) % cars as u64) as f64 / (cars - 1).max(1) as f64;
        drivers.push(Driver::with_character(circuit.clone(), CarKind::GT.spec(), Character::rival(skill, seed * 101 + k as u64)));
    }
    let names: Vec<(VehicleId, String)> = ids.iter().enumerate().map(|(k, id)| (*id, format!("car {k}"))).collect();
    let mut race = Race::new(circuit.clone(), laps, &names, 0.0, seed);
    let mut out = Outcome::default();
    let mut last_touch = vec![vec![-10.0f64; cars]; cars];
    let mut last_order = race.order();
    while race.phase() != Phase::Over && world.time() < (140.0 + 120.0 * rain) * laps as f64 + 60.0 {
        for (k, id) in ids.iter().enumerate() {
            let parked = std::env::var("PARKED").ok().and_then(|s| s.parse::<usize>().ok()) == Some(k);
            if race.holding() || parked {
                let c = &mut world.vehicle_mut(*id).car_mut().unwrap().controls;
                (c.brake, c.throttle) = (1.0, 0.0);
            } else {
                world.fly(*id, &mut drivers[k]);
            }
        }
        world.step();
        race.update(&world);
        if let Some(k) = std::env::var("TRACE").ok().and_then(|s| s.parse::<usize>().ok()) {
            let t = world.time();
            let from: f64 = std::env::var("TFROM").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            if (t * 4.0).fract() < World::DT * 4.0 && t >= from && t < from + 20.0 {
                let v = world.vehicle(ids[k]);
                let b = v.body();
                let c = v.car().unwrap().controls;
                let at = circuit.locate(b.pos.x, b.pos.y).map_or((0.0, 0.0), |l| (l.along, l.across));
                let others: Vec<String> = ids.iter().enumerate().filter(|(j, _)| *j != k).filter_map(|(j, id)| {
                    let o = world.vehicle(*id).body();
                    let l = circuit.locate(o.pos.x, o.pos.y)?;
                    ((l.along - at.0).abs() < 40.0).then(|| format!("{j}@{:.0},{:.1}", l.along - at.0, l.across))
                }).collect();
                println!("  t {t:.2} car {k}: {:.0} m {:.1} across {:.1} m/s steer {:.2} thr {:.2} brk {:.2} passing {} | {}", at.0, at.1, b.vel.x, c.steer, c.throttle, c.brake, drivers[k].passing(), others.join(" "));
            }
        }
        for (k, id) in ids.iter().enumerate() {
            if let (Some(why), false) = (world.vehicle(*id).dynamics.crashed(), out.wrecks.iter().any(|w| w.0 == k)) {
                let b = world.vehicle(*id).body();
                let at = circuit.locate(b.pos.x, b.pos.y).map_or((0.0, 0.0), |l| (l.along, l.across));
                out.wrecks.push((k, world.time(), at.0, at.1, why.to_string()));
            }
        }
        // Touches: two cars close and both struck this step.
        for i in 0..cars {
            for j in i + 1..cars {
                let (a, b) = (world.vehicle(ids[i]), world.vehicle(ids[j]));
                if (a.body().pos - b.body().pos).length() > 6.0 {
                    continue;
                }
                let (ia, ib) = (a.car().unwrap().data().impact, b.car().unwrap().data().impact);
                if ia > 0.05 && ib > 0.05 {
                    if world.time() - last_touch[i][j] > 1.0 {
                        out.contacts += 1;
                        if std::env::var("TOUCHES").is_ok() {
                            let at = |v: &sim_core::Vehicle| circuit.locate(v.body().pos.x, v.body().pos.y).map_or((0.0, 0.0), |l| (l.along, l.across));
                            println!("  touch at {:.1} s: car {i} {:?} {:.1} m/s, car {j} {:?} {:.1} m/s, impact {:.1}/{:.1}", world.time(), at(a), a.body().vel.x, at(b), b.body().vel.x, ia, ib);
                        }
                    }
                    last_touch[i][j] = world.time();
                    out.hardest = out.hardest.max(ia.max(ib));
                }
            }
        }
        if race.phase() == Phase::Racing {
            let order = race.order();
            for (p, k) in order.iter().enumerate() {
                if last_order.iter().position(|x| x == k).is_some_and(|q| q > p) {
                    out.passes += 1;
                }
            }
            last_order = order;
        }
    }
    out.wrecked = ids.iter().filter(|id| world.vehicle(**id).dynamics.crashed().is_some()).count();
    for (k, e) in race.entrants.iter().enumerate() {
        if e.finished.is_none() && !e.retired {
            let b = world.vehicle(ids[k]).body();
            let at = circuit.locate(b.pos.x, b.pos.y).map_or((0.0, 0.0), |l| (l.along, l.across));
            println!("  car {k} still running at {:.0} m of progress, {:.0} m round, {:.1} across, {:.1} m/s", e.progress, at.0, at.1, b.vel.x);
        }
    }
    out.order = race.order();
    out.times = race.entrants.iter().map(|e| e.finished).collect();
    out.best = race.entrants.iter().map(|e| e.best()).collect();
    out
}

fn print(label: &str, o: &Outcome) {
    let times: Vec<String> = o.order.iter().map(|&k| format!("{k}:{}", o.times[k].map_or("DNF".into(), |t| format!("{t:.1}")))).collect();
    let best = o.best.iter().flatten().copied().fold(f64::INFINITY, f64::min);
    println!("{label}: {} | best lap {best:.2} s | {} touches (hardest {:.1} m/s), {} wrecked, {} places changed", times.join(" "), o.contacts, o.hardest, o.wrecked, o.passes);
    for (k, t, along, across, why) in &o.wrecks {
        println!("  car {k} wrecked at {t:.1} s, {along:.0} m round, {across:.1} m across: {why}");
    }
}

#[test]
fn a_race_from_the_grid() {
    let o = race(6, 2, 0.0, 1);
    print("dry, 6 cars, 2 laps", &o);
    assert_eq!(o.wrecked, 0, "nobody wrecked");
    assert!(o.times.iter().all(Option::is_some), "everyone finishes: {:?}", o.times);
    assert!(o.hardest < 6.0, "no hard hits: {:.1} m/s", o.hardest);
    // The field spreads out: the winner is ahead by seconds, not by a whole lap.
    let first = o.times[o.order[0]].unwrap();
    let last = o.times[*o.order.last().unwrap()].unwrap();
    assert!(last - first > 1.0 && last - first < 40.0, "{first:.1} to {last:.1}");
}

#[test]
fn the_same_race_twice() {
    let a = race(4, 1, 0.0, 7);
    let b = race(4, 1, 0.0, 7);
    assert_eq!(a.order, b.order);
    assert_eq!(a.times, b.times);
}

#[test]
fn a_wet_race_is_slower() {
    let dry = race(1, 2, 0.0, 3);
    let wet = race(1, 2, 0.5, 3);
    print("dry, alone", &dry);
    print("rain 0.5, alone", &wet);
    let (d, w) = (dry.best[0].unwrap(), wet.best[0].expect("finishes in the wet"));
    assert_eq!(wet.wrecked, 0);
    assert!(w > d * 1.06 && w < d * 1.5, "dry {d:.2} s, wet {w:.2} s");
}

#[test]
fn the_racing_line_dries_first() {
    // Rain of 0.6 has soaked the circuit; it stops, and two cars lap it for four minutes: the
    // film on their line is thinner than beside it, and all of it thinner than it was.
    let (circuit, terrain) = lakeside();
    let mut world = World::with_terrain(1, terrain);
    world.weather.rain = 0.6;
    world.settle_water();
    let soaked = world.water[0].film(500.0, 0.0);
    world.weather.rain = 0.0;
    let mut drivers = Vec::new();
    let mut ids = Vec::new();
    for k in 0..2 {
        let (n, e, heading) = circuit.grid(k);
        ids.push(world.add_car(CarKind::GT, Start::Air { n, e, heading }));
        drivers.push(Driver::with_character(circuit.clone(), CarKind::GT.spec(), Character::rival(k as f64, k as u64)));
    }
    for step in 0..(240.0 / World::DT) as usize {
        for (k, id) in ids.iter().enumerate() {
            world.fly(*id, &mut drivers[k]);
        }
        world.step();
        if step % 2400 == 0 {
            let b = world.vehicle(ids[0]).body();
            let at = circuit.locate(b.pos.x, b.pos.y).unwrap();
            println!("  {:.0} s: at {:.0} m, {:.1} across: film {:.3} mm here, {:.3} 4 m left", world.time(), at.along, at.across, world.water[0].film(at.along - 3.0, at.across + 0.78) * 1e3, world.water[0].film(at.along - 3.0, at.across - 4.0) * 1e3);
        }
    }
    let line = &drivers[0].line;
    let (mut on, mut off) = (0.0, 0.0);
    let samples = (400..1800).step_by(2);
    let n = samples.len() as f64;
    for i in samples {
        let along = i as f64 * sim_core::circuit::STEP;
        let across = line.across[i];
        // The two tyres' tracks either side of the line, and two metres beyond them.
        on += (world.water[0].film(along, across - 0.8) + world.water[0].film(along, across + 0.8)) / 2.0 / n;
        let side = if across > 0.0 { -1.0 } else { 1.0 };
        off += world.water[0].film(along, across + side * 3.5) / n;
    }
    println!("film: {:.3} mm soaked; after 4 minutes dry, {:.3} mm on the line, {:.3} mm off it", soaked * 1e3, on * 1e3, off * 1e3);
    assert!(off < soaked && on < 0.8 * off, "on {on:.6}, off {off:.6}, soaked {soaked:.6}");
    assert!(world.water[0].standing < 0.8, "the puddles drain: {}", world.water[0].standing);
}

#[test]
#[ignore]
fn many_races() {
    let races: u64 = std::env::var("RACES").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let first: u64 = std::env::var("FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let rain: f64 = std::env::var("RAIN").ok().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let (mut contacts, mut wrecked, mut dnf, mut hardest, mut passes) = (0, 0, 0, 0.0f64, 0);
    for seed in first..first + races {
        let o = race(6, 3, rain, 1000 + seed);
        print(&format!("race {seed}"), &o);
        contacts += o.contacts;
        wrecked += o.wrecked;
        dnf += o.times.iter().filter(|t| t.is_none()).count();
        hardest = hardest.max(o.hardest);
        passes += o.passes;
    }
    println!("{races} races of 6 cars, 3 laps, rain {rain}: {contacts} touches ({:.2} a race), hardest {hardest:.1} m/s, {wrecked} wrecked, {dnf} not finished, {passes} places changed", contacts as f64 / races as f64);
}
