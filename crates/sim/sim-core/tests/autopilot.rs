//! The autopilot flying through `World::fly` and `World::step`: holds, a route, whole circuits
//! in calm air, crosswind and turbulence, and three aircraft sharing a circuit.
//!
//! `cargo test -p sim-core --test autopilot -- --nocapture` prints one line per scenario.

use sim_core::*;

const TAU: f64 = 2.0 * std::f64::consts::PI;

/// Flies `id` with `pilot` for `seconds`, calling `each` after every step with the time flown.
fn fly(
    world: &mut World,
    id: VehicleId,
    pilot: &mut Autopilot,
    seconds: f64,
    mut each: impl FnMut(&World, f64),
) {
    for i in 0..(seconds / World::DT).round() as usize {
        world.fly(id, pilot);
        world.step();
        each(world, (i + 1) as f64 * World::DT);
    }
}

/// An airport of the world other than the home one, the `index`-th nearest to the origin.
fn other_airport(world: &World, index: usize) -> Airport {
    let home = world.terrain.nearest_airport(0.0, 0.0).expect("home");
    let mut all = world
        .terrain
        .airports(-60_000.0, 60_000.0, -60_000.0, 60_000.0);
    all.retain(|a| a.id != home.id);
    all.sort_by(|a, b| a.n.hypot(a.e).total_cmp(&b.n.hypot(b.e)));
    all[index]
}

fn in_air(seed: u64, n: f64, e: f64, heading: f64) -> (World, VehicleId) {
    let mut world = World::new(seed);
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n, e, heading });
    (world, id)
}

// ---- Holds

/// Engages a hold on an aircraft trimmed at 100 kt and returns the worst errors over the last
/// `tail` seconds: (heading °, altitude m, airspeed kt), and the largest bank on the way.
fn hold(
    world: &mut World,
    id: VehicleId,
    pilot: &mut Autopilot,
    seconds: f64,
    tail: f64,
    target: (f64, f64, f64),
) -> (f64, f64, f64, f64) {
    let (mut dh, mut da, mut dv, mut bank) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    fly(world, id, pilot, seconds, |w, t| {
        let p = w.vehicle(id);
        let e = euler_angles(p.aircraft().unwrap().state.q);
        bank = bank.max(e.roll.abs() / DEG);
        if t > seconds - tail {
            dh = dh.max(remainder(e.heading - target.0, TAU).abs() / DEG);
            da = da.max((-p.aircraft().unwrap().state.pos.z - target.1).abs());
            dv = dv.max((p.aircraft().unwrap().indicated_airspeed() - target.2).abs() / KNOTS);
        }
    });
    assert!(world.vehicle(id).aircraft().unwrap().crashed().is_none());
    (dh, da, dv, bank)
}

#[test]
fn holds_reach_and_keep_their_targets() {
    // (seed, heading change °, altitude change m, speed change kt)
    let cases = [
        (1, 30.0, 0.0, 0.0),
        (1, -30.0, 300.0, 0.0),
        (2, 0.0, -300.0, 0.0),
        (2, 90.0, 0.0, -20.0),
        (3, -170.0, 150.0, 10.0),
        (3, 0.0, 0.0, 20.0),
    ];
    for (seed, turn, climb, faster) in cases {
        let (mut world, id) = in_air(seed, 3000.0, -2000.0, 1.0);
        let s = world.vehicle(id).aircraft().unwrap().state;
        let target = (
            remainder(1.0 + turn * DEG, TAU),
            -s.pos.z + climb,
            (100.0 + faster) * KNOTS,
        );
        let mut pilot = Autopilot::new(Goal::Hold {
            heading: Some(target.0),
            vertical: Vertical::Altitude(target.1),
            airspeed: Some(target.2),
        });
        let (dh, da, dv, bank) = hold(&mut world, id, &mut pilot, 200.0, 40.0, target);
        println!(
            "HOLD seed {seed}: heading {turn:+.0}°, altitude {climb:+.0} m, speed {faster:+.0} kt → after 160 s within {dh:.2}°, {da:.1} m, {dv:.2} kt; largest bank {bank:.1}°"
        );
        assert!(dh < 1.0, "heading {dh}");
        assert!(da < 5.0, "altitude {da}");
        assert!(dv < 2.0, "airspeed {dv}");
        assert!(bank < 27.0, "bank {bank}");
    }
}

#[test]
fn hold_in_wind_and_turbulence_and_a_vertical_speed() {
    let mut world = World::new(1);
    world.weather.wind_speed = 12.0 * KNOTS;
    world.weather.turbulence = 0.5;
    let id = world.add_aircraft(AircraftKind::C172, Start::Air {
        n: 0.0,
        e: 0.0,
        heading: 2.0,
    });
    let altitude = -world.vehicle(id).aircraft().unwrap().state.pos.z;
    let target = (2.0 + 30.0 * DEG, altitude, 95.0 * KNOTS);
    let mut pilot = Autopilot::new(Goal::Hold {
        heading: Some(target.0),
        vertical: Vertical::Altitude(target.1),
        airspeed: Some(target.2),
    });
    let (dh, da, dv, bank) = hold(&mut world, id, &mut pilot, 120.0, 60.0, target);
    println!("HOLD in turbulence 0.5, wind 12 kt: within {dh:.2}°, {da:.1} m, {dv:.2} kt; largest bank {bank:.1}°");
    assert!(dh < 3.0 && da < 10.0 && dv < 4.0, "{dh} {da} {dv}");

    // A vertical speed: 400 ft/min down, wings level on no particular heading.
    world.weather.turbulence = 0.0;
    pilot.set_goal(Goal::Hold {
        heading: None,
        vertical: Vertical::Speed(-2.0),
        airspeed: Some(90.0 * KNOTS),
    });
    let (mut lo, mut hi) = (9.0_f64, -9.0_f64);
    fly(&mut world, id, &mut pilot, 60.0, |w, t| {
        if t > 25.0 {
            let v = -act(w.vehicle(id).aircraft().unwrap().state.q, w.vehicle(id).aircraft().unwrap().state.vel).z;
            lo = lo.min(v);
            hi = hi.max(v);
        }
    });
    let roll = euler_angles(world.vehicle(id).aircraft().unwrap().state.q).roll / DEG;
    println!("HOLD vertical speed -2.0 m/s: {lo:.2}..{hi:.2} m/s, bank {roll:.1}°");
    assert!(lo > -2.4 && hi < -1.6, "{lo} {hi}");
    assert!(roll.abs() < 2.0);
}

#[test]
fn speed_limits_come_before_the_targets() {
    // Asked for a climb it cannot make at the speed asked, and for speeds outside the arcs.
    let (mut world, id) = in_air(1, 0.0, 0.0, 0.0);
    let mut pilot = Autopilot::new(Goal::Hold {
        heading: None,
        vertical: Vertical::Speed(8.0),
        airspeed: Some(40.0 * KNOTS),
    });
    let mut slowest = f64::INFINITY;
    fly(&mut world, id, &mut pilot, 90.0, |w, _| {
        slowest = slowest.min(w.vehicle(id).aircraft().unwrap().indicated_airspeed());
    });
    pilot.set_goal(Goal::Hold {
        heading: None,
        vertical: Vertical::Speed(-8.0),
        airspeed: Some(150.0 * KNOTS),
    });
    let mut fastest = 0.0_f64;
    fly(&mut world, id, &mut pilot, 90.0, |w, _| {
        fastest = fastest.max(w.vehicle(id).aircraft().unwrap().indicated_airspeed());
    });
    println!(
        "LIMITS asked 40 kt climbing 8 m/s: slowest {:.1} kt (1.3 Vs = 68.9); asked 150 kt diving: fastest {:.1} kt (yellow arc 127)",
        slowest / KNOTS,
        fastest / KNOTS
    );
    assert!(world.vehicle(id).aircraft().unwrap().crashed().is_none());
    assert!(slowest > 67.5 * KNOTS, "{}", slowest / KNOTS);
    assert!(fastest < 127.0 * KNOTS, "{}", fastest / KNOTS);
}

// ---- Route

#[test]
fn a_route_of_four_waypoints() {
    for seed in [1, 4] {
        let (mut world, id) = in_air(seed, 0.0, 0.0, 0.3);
        let a0 = -world.vehicle(id).aircraft().unwrap().state.pos.z;
        let route = vec![
            Waypoint {
                north: 5000.0,
                east: 2000.0,
                altitude: a0 + 100.0,
            },
            Waypoint {
                north: 6000.0,
                east: 8000.0,
                altitude: a0 + 100.0,
            },
            Waypoint {
                north: 0.0,
                east: 9000.0,
                altitude: a0 - 100.0,
            },
            Waypoint {
                north: -3000.0,
                east: 3000.0,
                altitude: a0,
            },
        ];
        let mut pilot = Autopilot::new(Goal::Route(route.clone()));
        // Closest pass to each waypoint and the altitude there; cross-track error on each leg
        // once it has been captured (within 30 m) and until the turn to the next begins.
        let mut pass = [(f64::INFINITY, 0.0_f64); 4];
        let (mut across, mut captured, mut leg_seen) = (0.0_f64, false, 0);
        let mut done_at = None;
        let mut t = 0.0;
        while t < 900.0 && done_at.is_none() {
            let (before, leg_before) = (pilot.phase(), pilot.leg());
            world.fly(id, &mut pilot);
            world.step();
            t += World::DT;
            let s = world.vehicle(id).aircraft().unwrap().state;
            for (i, wp) in route.iter().enumerate() {
                let d = (s.pos.x - wp.north).hypot(s.pos.y - wp.east);
                pass[i].0 = pass[i].0.min(d);
            }
            // The altitude that counts is the one it has when it turns for the next waypoint.
            if pilot.leg() != leg_before {
                pass[leg_before].1 = -s.pos.z - route[leg_before].altitude;
            }
            // The leg being flown, from the passes so far.
            let leg = pass
                .iter()
                .position(|p| p.0 > 1500.0 || p.0.is_infinite())
                .unwrap_or(3);
            let leg = leg.max(leg_seen);
            if leg != leg_seen {
                leg_seen = leg;
                captured = false;
            }
            if leg > 0 {
                let (from, to) = (route[leg - 1], route[leg]);
                let course = (to.east - from.east).atan2(to.north - from.north);
                let off = -(s.pos.x - to.north) * course.sin() + (s.pos.y - to.east) * course.cos();
                let left = (to.north - s.pos.x) * course.cos() + (to.east - s.pos.y) * course.sin();
                captured |= off.abs() < 30.0;
                if captured && left > 1500.0 {
                    across = across.max(off.abs());
                }
            }
            if before == Phase::Route && pilot.phase() == Phase::RouteDone {
                done_at = Some(t);
            }
        }
        let worst_pass = pass.iter().fold(0.0_f64, |m, p| m.max(p.0));
        let worst_alt = pass.iter().fold(0.0_f64, |m, p| m.max(p.1.abs()));
        println!(
            "ROUTE seed {seed}: 4 waypoints in {:.0} s; passes within {worst_pass:.0} m (turn anticipation), altitude there within {worst_alt:.1} m, cross-track on captured legs {across:.1} m",
            done_at.unwrap_or(-1.0)
        );
        assert!(world.vehicle(id).aircraft().unwrap().crashed().is_none());
        assert!(done_at.is_some(), "route not finished");
        assert!(worst_pass < 1200.0, "{worst_pass}");
        assert!(worst_alt < 15.0, "{worst_alt}");
        assert!(across < 40.0, "{across}");
    }
}

#[test]
fn a_route_does_not_fly_into_rising_ground() {
    // A route 60 m above the sea across the hills of each world.
    for seed in [1, 2, 3] {
        let (mut world, id) = in_air(seed, 0.0, 0.0, 0.0);
        let route = vec![
            Waypoint {
                north: 12_000.0,
                east: 9000.0,
                altitude: 60.0,
            },
            Waypoint {
                north: -9000.0,
                east: 15_000.0,
                altitude: 60.0,
            },
            Waypoint {
                north: -12_000.0,
                east: -9000.0,
                altitude: 60.0,
            },
        ];
        let mut pilot = Autopilot::new(Goal::Route(route));
        let (mut lowest, mut highest_ground) = (f64::INFINITY, 0.0_f64);
        fly(&mut world, id, &mut pilot, 900.0, |w, t| {
            let p = w.vehicle(id);
            if t > 300.0 {
                lowest = lowest.min(p.agl());
            }
            highest_ground = highest_ground.max(p.environment().ground_height);
        });
        println!("TERRAIN seed {seed}: route asked at 60 m over ground up to {highest_ground:.0} m: lowest {lowest:.0} m above it");
        assert!(
            world.vehicle(id).aircraft().unwrap().crashed().is_none(),
            "{:?}",
            world.vehicle(id).aircraft().unwrap().crashed()
        );
        assert!(lowest > 40.0, "{lowest}");
    }
}

// ---- Circuits

struct Landing {
    /// Metres past the threshold and off the centreline at touchdown, sink rate there.
    along: f64,
    across: f64,
    sink: f64,
    on_runway: bool,
    /// Worst distance off the centreline while rolling, where it stopped, and when.
    rolled: f64,
    stopped: Option<f64>,
    time: f64,
    crashed: Option<&'static str>,
}

/// Flies one circuit (or an arrival) until the aircraft stops, crashes or `limit` seconds pass.
fn land(
    world: &mut World,
    id: VehicleId,
    pilot: &mut Autopilot,
    airport: &Airport,
    reversed: bool,
    limit: f64,
) -> Landing {
    let course = airport.heading + if reversed { std::f64::consts::PI } else { 0.0 };
    let (c, s) = (course.cos(), course.sin());
    let local = |w: &World| {
        let p = w.vehicle(id).aircraft().unwrap().state.pos;
        let (dn, de) = (p.x - airport.n, p.y - airport.e);
        (dn * c + de * s + airport.length / 2.0, -dn * s + de * c)
    };
    let mut r = Landing {
        along: 0.0,
        across: 0.0,
        sink: 0.0,
        on_runway: false,
        rolled: 0.0,
        stopped: None,
        time: 0.0,
        crashed: None,
    };
    let (mut flown, mut down) = (false, false);
    while r.time < limit {
        world.fly(id, pilot);
        world.step();
        r.time += World::DT;
        let plane = world.vehicle(id);
        let (along, across) = local(world);
        flown |= plane.agl() > 30.0;
        if flown && !down && plane.aircraft().unwrap().data().on_ground() {
            down = true;
            r.along = along;
            r.across = across;
            r.sink = plane.aircraft().unwrap().last_touchdown().unwrap_or(0.0);
            r.on_runway = plane.environment().kind == GroundKind::Runway;
        }
        if down {
            r.rolled = r.rolled.max(across.abs());
            if plane.aircraft().unwrap().state.vel.length() < 0.5 {
                r.stopped = Some(along);
                break;
            }
        }
        if let Some(why) = plane.aircraft().unwrap().crashed() {
            r.crashed = Some(why);
            break;
        }
    }
    r
}

fn check(name: &str, airport: &Airport, r: &Landing) {
    println!(
        "{name}: touchdown {:.0} m past the threshold, {:+.1} m off the centreline, sink {:.2} m/s; rolled within {:.1} m, stopped at {:.0} m of {:.0}; {:.0} s {}",
        r.along,
        r.across,
        r.sink,
        r.rolled,
        r.stopped.unwrap_or(-1.0),
        airport.length,
        r.time,
        r.crashed.unwrap_or("")
    );
    assert!(r.crashed.is_none(), "{name}: {:?}", r.crashed);
    assert!(r.on_runway, "{name}: touched down off the runway");
    assert!(
        r.across.abs() < 6.0,
        "{name}: {} m off the centreline",
        r.across
    );
    assert!(r.sink < 2.0, "{name}: sink {}", r.sink);
    assert!(r.rolled < 10.0, "{name}: rolled {} m off", r.rolled);
    assert!(
        r.stopped.is_some_and(|x| x < airport.length),
        "{name}: not stopped on the runway"
    );
}

/// A whole circuit from standstill at an airport, in the given weather: wind in knots, from
/// `off` degrees to the right of the runway direction.
fn circuit(
    seed: u64,
    airport_index: Option<usize>,
    wind: f64,
    off: f64,
    turbulence: f64,
) -> (Airport, Landing) {
    circuit_with(seed, airport_index, wind, off, turbulence, 1.0)
}

/// As `circuit`, with every control-surface and throttle gain multiplied by `factor`: what a
/// flight model with that much more or less control power would look like to the autopilot.
fn circuit_with(
    seed: u64,
    airport_index: Option<usize>,
    wind: f64,
    off: f64,
    turbulence: f64,
    factor: f64,
) -> (Airport, Landing) {
    let mut world = World::new(seed);
    let airport = match airport_index {
        None => world.terrain.nearest_airport(0.0, 0.0).expect("home"),
        Some(i) => other_airport(&world, i),
    };
    world.weather.wind_from = (airport.heading / DEG + off).rem_euclid(360.0);
    world.weather.wind_speed = wind * KNOTS;
    world.weather.turbulence = turbulence;
    let id = world.add_aircraft(AircraftKind::C172, Start::Runway {
        airport,
        reversed: false,
    });
    let mut pilot = Autopilot::new(Goal::Circuit {
        airport,
        reversed: false,
        repeat: false,
    });
    let g = &mut pilot.gains;
    for gain in [
        &mut g.pitch_p,
        &mut g.pitch_d,
        &mut g.pitch_authority,
        &mut g.roll_p,
        &mut g.roll_d,
        &mut g.yaw_p,
        &mut g.yaw_d,
        &mut g.throttle_p,
        &mut g.throttle_i,
        &mut g.steer_p,
        &mut g.steer_d,
    ] {
        *gain *= factor;
    }
    let r = land(&mut world, id, &mut pilot, &airport, false, 900.0);
    (airport, r)
}

#[test]
fn circuit_in_calm_air() {
    for (seed, index) in [(1, None), (2, None), (3, None), (1, Some(0))] {
        let (airport, r) = circuit(seed, index, 0.0, 0.0, 0.0);
        check(
            &format!("CIRCUIT calm, seed {seed} airport {index:?}"),
            &airport,
            &r,
        );
    }
}

#[test]
fn the_gains_have_margin() {
    // The same circuit in wind and turbulence with the loops at half and at twice their gain.
    for factor in [0.5, 2.0] {
        let (airport, r) = circuit_with(1, None, 8.0, 40.0, 0.5, factor);
        check(
            &format!("MARGIN gains x{factor}, turbulence 0.5, 8 kt from +40°, seed 1"),
            &airport,
            &r,
        );
    }
}

#[test]
fn circuit_with_ten_knots_of_crosswind() {
    for (seed, index, off) in [(1, None, 90.0), (2, None, -90.0), (3, None, 90.0)] {
        let (airport, r) = circuit(seed, index, 10.0, off, 0.0);
        check(
            &format!("CIRCUIT crosswind 10 kt from {off:+.0}°, seed {seed}"),
            &airport,
            &r,
        );
    }
}

#[test]
fn circuit_with_turbulence_and_wind_off_the_runway() {
    for (seed, index, off) in [(1, None, 40.0), (2, None, -40.0), (3, None, 40.0)] {
        let (airport, r) = circuit(seed, index, 8.0, off, 0.5);
        check(
            &format!("CIRCUIT turbulence 0.5, 8 kt from {off:+.0}°, seed {seed}"),
            &airport,
            &r,
        );
    }
}

#[test]
fn arrivals_join_the_circuit_and_land() {
    // From 900 m above the field, from far away on the dead side, and already on final.
    let starts: [(&str, u64, f64, f64); 3] = [
        ("overhead", 1, 0.0, 0.0),
        ("8 km out", 2, -6000.0, 5500.0),
        ("12 km out", 3, 9000.0, -8000.0),
    ];
    for (name, seed, north, east) in starts {
        let mut world = World::new(seed);
        let airport = world.terrain.nearest_airport(0.0, 0.0).expect("home");
        let id = world.add_aircraft(AircraftKind::C172, Start::Air {
            n: airport.n + north,
            e: airport.e + east,
            heading: 4.0,
        });
        let mut pilot = Autopilot::new(Goal::Land {
            airport,
            reversed: true,
        });
        let r = land(&mut world, id, &mut pilot, &airport, true, 1200.0);
        check(
            &format!("LAND from {name}, seed {seed}, runway reversed"),
            &airport,
            &r,
        );
        fly(&mut world, id, &mut pilot, 1.0, |_, _| {});
        assert!(pilot.finished());
    }
    let mut world = World::new(1);
    let airport = world.terrain.nearest_airport(0.0, 0.0).expect("home");
    let id = world.add_aircraft(AircraftKind::C172, Start::Final {
        airport,
        reversed: false,
        distance: 3000.0,
    });
    let mut pilot = Autopilot::new(Goal::Land {
        airport,
        reversed: false,
    });
    let r = land(&mut world, id, &mut pilot, &airport, false, 300.0);
    check("LAND from 3 km on final, seed 1", &airport, &r);
}

#[test]
fn three_aircraft_fly_circuits_for_fifteen_minutes() {
    let mut world = World::new(1);
    let airport = world.terrain.nearest_airport(0.0, 0.0).expect("home");
    let goal = Goal::Circuit {
        airport,
        reversed: false,
        repeat: true,
    };
    let ids = [
        world.add_aircraft(AircraftKind::C172, Start::Runway {
            airport,
            reversed: false,
        }),
        world.add_aircraft(AircraftKind::C172, Start::Final {
            airport,
            reversed: false,
            distance: 3500.0,
        }),
        world.add_aircraft(AircraftKind::C172, Start::Air {
            n: airport.n + 4000.0,
            e: airport.e - 5000.0,
            heading: 1.0,
        }),
    ];
    let mut pilots = [
        Autopilot::new(goal.clone()),
        Autopilot::new(goal.clone()),
        Autopilot::new(goal),
    ];
    let mut closest = f64::INFINITY;
    let mut closest_in_flight = f64::INFINITY;
    // Where the longest takeoff run left the ground, metres past the threshold.
    let mut unstick = 0.0_f64;
    let (c, s) = (airport.heading.cos(), airport.heading.sin());
    for _ in 0..(900.0 / World::DT) as usize {
        for (id, pilot) in ids.iter().zip(pilots.iter_mut()) {
            world.fly(*id, pilot);
        }
        world.step();
        for id in ids {
            let p = world.vehicle(id);
            if p.aircraft().unwrap().data().on_ground() && p.aircraft().unwrap().controls.throttle > 0.9 {
                let pos = p.aircraft().unwrap().state.pos;
                unstick = unstick
                    .max((pos.x - airport.n) * c + (pos.y - airport.e) * s + airport.length / 2.0);
            }
        }
        for a in 0..3 {
            for b in a + 1..3 {
                let (pa, pb) = (world.vehicle(ids[a]), world.vehicle(ids[b]));
                let d = (pa.aircraft().unwrap().state.pos - pb.aircraft().unwrap().state.pos).length();
                closest = closest.min(d);
                if !pa.aircraft().unwrap().data().on_ground() && !pb.aircraft().unwrap().data().on_ground() {
                    closest_in_flight = closest_in_flight.min(d);
                }
            }
        }
    }
    let landings: Vec<u32> = pilots.iter().map(|p| p.landings()).collect();
    let phases: Vec<Phase> = pilots.iter().map(|p| p.phase()).collect();
    println!(
        "TRAFFIC three aircraft, 15 min: landings {landings:?}, now {phases:?}; closest {closest:.0} m (both in flight: {closest_in_flight:.0} m); the longest takeoff left the ground {unstick:.0} m past the threshold of {:.0}",
        airport.length
    );
    for id in ids {
        assert!(
            world.vehicle(id).aircraft().unwrap().crashed().is_none(),
            "{:?}",
            world.vehicle(id).aircraft().unwrap().crashed()
        );
    }
    assert!(landings.iter().all(|n| *n >= 2), "{landings:?}");
}

/// Not a test: prints a circuit second by second, to see what the autopilot does when tuning it.
/// `SEED=1 WIND=10 OFF=90 TURB=0.5 FROM=280 UNTIL=320 EVERY=0.5 cargo test -p sim-core --test autopilot trace -- --ignored --nocapture`
#[test]
#[ignore]
fn trace() {
    let seed: u64 = std::env::var("SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let until: f64 = std::env::var("UNTIL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60.0);
    let every: f64 = std::env::var("EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let from: f64 = std::env::var("FROM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let wind: f64 = std::env::var("WIND")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let off: f64 = std::env::var("OFF")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let turb: f64 = std::env::var("TURB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let mut world = World::new(seed);
    let airport = world.terrain.nearest_airport(0.0, 0.0).expect("home");
    world.weather.wind_from = (airport.heading / DEG + off).rem_euclid(360.0);
    world.weather.wind_speed = wind * KNOTS;
    world.weather.turbulence = turb;
    let id = world.add_aircraft(AircraftKind::C172, Start::Runway {
        airport,
        reversed: false,
    });
    let mut pilot = Autopilot::new(Goal::Circuit {
        airport,
        reversed: false,
        repeat: false,
    });
    let (c, s) = (airport.heading.cos(), airport.heading.sin());
    let mut next = from;
    let mut t = 0.0;
    while t < until {
        world.fly(id, &mut pilot);
        world.step();
        t += World::DT;
        let p = world.vehicle(id);
        if t >= next || p.aircraft().unwrap().crashed().is_some() {
            next += every;
            let st = p.aircraft().unwrap().state;
            let e = euler_angles(st.q);
            let (dn, de) = (st.pos.x - airport.n, st.pos.y - airport.e);
            let k = p.aircraft().unwrap().controls;
            println!(
                "{t:6.1} {:?} x {:6.0} y {:6.1} agl {:6.1} ias {:5.1} vs {:5.2} pitch {:5.1} roll {:5.1} hdg {:5.1} | el {:5.2} ail {:5.2} rud {:5.2} thr {:4.2} fl {} trim {:5.2} {}",
                pilot.phase(),
                dn * c + de * s + airport.length / 2.0,
                dn * s - de * c,
                p.agl(),
                p.aircraft().unwrap().indicated_airspeed() / KNOTS,
                -act(st.q, st.vel).z,
                e.pitch / DEG,
                e.roll / DEG,
                remainder(e.heading - airport.heading, TAU) / DEG,
                k.elevator, k.aileron, k.rudder, k.throttle, k.flap_notch, k.trim,
                p.aircraft().unwrap().crashed().unwrap_or("")
            );
            if p.aircraft().unwrap().crashed().is_some() {
                break;
            }
        }
    }
}

#[test]
fn a_pattern_survives_its_text_form() {
    use sim_core::Pattern;
    let mut p = Pattern::default();
    assert!(p.set("width", 600.0) && p.set("land_reversed", 1.0) && p.set("glide", 5.5));
    let back = Pattern::from_text(&p.to_text()).unwrap();
    for f in Pattern::FIELDS {
        assert!((back.get(f).unwrap() - p.get(f).unwrap()).abs() < 1e-3, "{f}");
    }
    assert!(Pattern::from_text("wingspan 3").is_err());
}
