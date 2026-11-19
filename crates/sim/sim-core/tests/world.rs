//! The world: terrain, airports, the atmosphere, and aircraft living in it.

mod common;

use common::{assert_close, Sim};
use sim_core::*;

#[test]
fn terrain_is_deterministic() {
    let (a, b, c) = (Terrain::new(7), Terrain::new(7), Terrain::new(8));
    let mut differs = false;
    for i in 0..200 {
        let (n, e) = (i as f64 * 733.1 - 40_000.0, i as f64 * -517.7 + 21_000.0);
        assert_eq!(a.height(n, e), b.height(n, e));
        assert!(a.height(n, e).is_finite());
        if (a.height(n, e) - c.height(n, e)).abs() > 1.0 {
            differs = true;
        }
    }
    assert!(differs);
}

#[test]
fn terrain_has_variety() {
    let t = Terrain::new(1);
    let (mut sea, mut high, mut total, mut top) = (0, 0, 0, 0.0_f64);
    for i in -60..=60 {
        for j in -60..=60 {
            let h = t.raw(i as f64 * 2500.0, j as f64 * 2500.0).h;
            total += 1;
            if h < 0.0 {
                sea += 1;
            }
            if h > 1000.0 {
                high += 1;
            }
            top = top.max(h);
        }
    }
    println!(
        "sea {}% mountains {}% highest {top} m",
        100 * sea / total,
        100 * high / total
    );
    assert!(sea > total / 50);
    assert!(sea < total / 2);
    assert!(high > total / 100);
    assert!(top < 4000.0);
}

#[test]
fn airports_are_flat_and_dry() {
    let t = Terrain::new(1);
    let mut found = 0;
    for x in -6..=6 {
        for y in -6..=6 {
            let Some(a) = t.airport((x, y)) else { continue };
            found += 1;
            let (c, s) = (a.heading.cos(), a.heading.sin());
            for i in 0..=22 {
                let along = -495.0 + i as f64 * 45.0;
                for across in [-11.0, 0.0, 11.0] {
                    let g = t.ground(a.n + along * c - across * s, a.e + along * s + across * c);
                    assert_close(g.h, a.elevation, 1e-6, "runway height");
                    assert_eq!(g.river, 0.0);
                    assert!(a.on_runway(a.n + along * c - across * s, a.e + along * s + across * c));
                }
            }
            assert!(a.elevation > 4.0);
            // The blend back to natural ground must not leave a cliff.
            let mut steepest = 0.0_f64;
            for i in 0..=140 {
                let along = -1400.0 + i as f64 * 20.0;
                let h0 = t.height(a.n + along * c, a.e + along * s);
                let h1 = t.height(a.n + (along + 20.0) * c, a.e + (along + 20.0) * s);
                steepest = steepest.max((h1 - h0).abs() / 20.0);
            }
            assert!(steepest < 0.35);
        }
    }
    println!("airports in 169 cells: {found}");
    assert!(found > 15);
}

#[test]
fn starts_parked_on_a_runway() {
    let mut sim = Sim::new(1);
    assert!(sim.vehicle().home.is_some());
    for _ in 0..240 * 8 {
        sim.step();
    }
    assert!(sim.aircraft().crashed().is_none());
    assert!(sim.aircraft().data().on_ground());
    assert_eq!(sim.vehicle().environment().kind, GroundKind::Runway);
    assert!(sim.aircraft().state.vel.length() < 0.1);
    assert_close(
        sim.vehicle().agl(),
        sim.aircraft().p.cg_height,
        0.1,
        "height on the runway",
    );
}

#[test]
fn flies_in_turbulence_without_blowing_up() {
    let mut sim = Sim::new(3);
    sim.world.weather.wind_speed = 10.0;
    sim.world.weather.turbulence = 1.0;
    sim.reset_in_air(0.0, 0.0, 1.0);
    let start = -sim.aircraft().state.pos.z;
    for _ in 0..240 * 30 {
        sim.step();
    }
    let s = sim.aircraft().state;
    println!(
        "turbulence: altitude {start} → {}, ias {} kt, roll {}",
        -s.pos.z,
        sim.vehicle().aircraft().unwrap().indicated_airspeed() / KNOTS,
        euler_angles(s.q).roll / DEG
    );
    assert!(s.pos.x.is_finite() && s.vel.x.is_finite());
    assert_close(
        sim.vehicle().aircraft().unwrap().indicated_airspeed() / KNOTS,
        95.0,
        35.0,
        "airspeed in turbulence",
    );
    assert_close(-s.pos.z, start, 400.0, "altitude in turbulence");
}

#[test]
fn atmosphere_matches_the_standard() {
    assert_close(
        atmosphere::density(0.0),
        1.225,
        0.001,
        "density at sea level",
    );
    assert_close(
        atmosphere::density(8000.0 * FEET),
        0.963,
        0.005,
        "density at 8000 ft",
    );
    assert_close(
        atmosphere::pressure(3000.0),
        70_109.0,
        100.0,
        "pressure at 3000 m",
    );
    // The tropopause and the isothermal stratosphere above it (ICAO Doc 7488 tables).
    assert_close(atmosphere::temperature(11_000.0), 216.65, 0.01, "temperature at the tropopause");
    assert_close(atmosphere::pressure(11_000.0), 22_632.0, 5.0, "pressure at 11 km");
    assert_close(atmosphere::pressure(15_000.0), 12_045.0, 5.0, "pressure at 15 km");
    assert_close(atmosphere::pressure(20_000.0), 5_474.9, 3.0, "pressure at 20 km");
    assert_close(atmosphere::density(20_000.0), 0.08803, 0.0001, "density at 20 km");
}

#[test]
fn a_hot_humid_day_is_thinner_air() {
    let dry = Atmosphere { sea_level_temperature: 303.15, ..Atmosphere::STANDARD };
    let humid = Atmosphere { relative_humidity: 1.0, ..dry };
    // At 30 °C water saturates at 42.5 hPa (Buck), and saturated air is 1.4 % lighter than dry.
    assert_close(atmosphere::saturation_vapour_pressure(303.15), 4245.0, 10.0, "saturation at 30 °C");
    assert_close(atmosphere::saturation_vapour_pressure(263.15), 259.9, 1.0, "over ice at -10 °C");
    assert_close(humid.density(0.0) / dry.density(0.0), 0.984, 0.002, "saturated against dry air at 30 °C");
    // Its specific humidity, 0.622 e / (p - 0.378 e): 26.5 g of water a kilogram.
    assert_close(humid.vapour_share(0.0), 0.0265, 0.0005, "vapour in it");
    assert_eq!(Atmosphere::STANDARD.vapour_share(5000.0), 0.0);
    // The same pressure: the altimeter does not care.
    assert_eq!(humid.pressure(1000.0), dry.pressure(1000.0));

    // Full power at sea level: the wing has less air and the engine less to burn.
    let power = |air: Atmosphere| {
        let mut world = World::new(1);
        world.weather.air = air;
        let id = world.add_aircraft(AircraftKind::C172, Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
        world.vehicle_mut(id).aircraft_mut().unwrap().controls.throttle = 1.0;
        for _ in 0..240 {
            world.step();
        }
        let a = world.vehicle(id).aircraft().unwrap();
        let env = world.vehicle(id).environment();
        (a.engine_torque(1.0, a.state.prop_speed, env.rho * (1.0 - env.vapour)), env.rho)
    };
    let (dry_torque, dry_rho) = power(dry);
    let (humid_torque, humid_rho) = power(humid);
    assert!(humid_rho < dry_rho);
    assert!(humid_torque < dry_torque * 0.985, "{humid_torque} against {dry_torque}");
}

/// Several aircraft share one world: they advance together and do not disturb each other.
#[test]
fn three_planes_fly_together() {
    const SEED: u64 = 1;
    let terrain = Terrain::new(SEED);
    let home = terrain.nearest_airport(0.0, 0.0).unwrap();
    // Another airport, for the third aircraft.
    let other = terrain
        .airports(-40_000.0, 40_000.0, -40_000.0, 40_000.0)
        .into_iter()
        .find(|a| a.id != home.id)
        .unwrap();
    let starts = [
        Start::Home,
        Start::Air {
            n: 5_000.0,
            e: -3_000.0,
            heading: 2.0,
        },
        Start::Final {
            airport: other,
            reversed: true,
            distance: Vehicle::FINAL_DISTANCE,
        },
    ];

    let mut world = World::new(SEED);
    let ids: Vec<VehicleId> = starts.iter().map(|s| world.add_aircraft(AircraftKind::C172, *s)).collect();
    assert_eq!(ids, [VehicleId(0), VehicleId(1), VehicleId(2)]);
    // The one on the runway takes off straight ahead; the other two fly hands off.
    world.vehicle_mut(ids[0]).aircraft_mut().unwrap().controls.throttle = 1.0;
    for _ in 0..240 * 30 {
        world.step();
    }
    assert_close(world.time(), 30.0, 1e-9, "world time");

    // Each one alone in its own world does exactly the same.
    for (i, start) in starts.iter().enumerate() {
        let mut alone = World::new(SEED);
        // Same turbulence seed as it had in the shared world.
        for _ in 0..i {
            alone.add_aircraft(AircraftKind::C172, Start::Home);
        }
        let id = alone.add_aircraft(AircraftKind::C172, *start);
        if i == 0 {
            alone.vehicle_mut(id).aircraft_mut().unwrap().controls.throttle = 1.0;
        }
        // The planes before it stay parked with the engine idling.
        for _ in 0..240 * 30 {
            alone.step();
        }
        assert_eq!(
            world.vehicle(ids[i]).aircraft().unwrap().state,
            alone.vehicle(id).aircraft().unwrap().state,
            "plane {i} was disturbed by the others"
        );
    }

    for &id in &ids {
        let plane = world.vehicle(id);
        let s = plane.aircraft().unwrap().state;
        assert!(
            plane.aircraft().unwrap().crashed().is_none(),
            "plane {} crashed: {:?}",
            id.0,
            plane.aircraft().unwrap().crashed()
        );
        assert!(s.pos.is_finite() && s.vel.is_finite());
    }
    // All three have moved, and none is where another one is.
    assert!(world.vehicle(ids[0]).aircraft().unwrap().state.vel.length() > 20.0);
    assert!(world.vehicle(ids[1]).agl() > 300.0);

    for &id in &ids {
        let obs = world.observe(id);
        let me = world.vehicle(id).aircraft().unwrap().state;
        let others: Vec<usize> = obs.traffic.iter().map(|t| t.id).collect();
        let expected: Vec<usize> = (0..3).filter(|i| *i != id.0).collect();
        assert_eq!(others, expected);
        for t in &obs.traffic {
            let other = world.vehicle(VehicleId(t.id)).aircraft().unwrap().state;
            assert_eq!(V3::from_array(t.relative), other.pos - me.pos);
            assert_eq!(V3::from_array(t.velocity), act(other.q, other.vel));
            assert!(V3::from_array(t.relative).length() > 500.0);
        }
        assert_eq!(
            (obs.north, obs.east, obs.altitude),
            (me.pos.x, me.pos.y, -me.pos.z)
        );
        assert_eq!(obs.agl, world.vehicle(id).agl());
        assert_eq!(obs.aircraft().unwrap().indicated_airspeed, world.vehicle(id).aircraft().unwrap().indicated_airspeed());
        let airport = obs.nearest_airport.expect("an airport in range");
        assert_close(
            airport.distance,
            (airport.north - obs.north).hypot(airport.east - obs.east),
            1e-9,
            "distance to the airport",
        );
        assert!((0.0..2.0 * std::f64::consts::PI).contains(&airport.bearing));
    }
    // The one that took off is still next to its own airport.
    assert_eq!(world.observe(ids[0]).nearest_airport.unwrap().id, home.id);
}

/// The terrain is queried from worker threads, and a world can be handed to one.
#[test]
fn terrain_and_world_can_cross_threads() {
    fn shared<T: Send + Sync>() {}
    fn sendable<T: Send>() {}
    shared::<Terrain>();
    sendable::<World>();
    sendable::<Observation>();

    let terrain = std::sync::Arc::new(Terrain::new(1));
    let here = terrain.nearest_airport(0.0, 0.0);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let t = terrain.clone();
            std::thread::spawn(move || t.nearest_airport(0.0, 0.0))
        })
        .collect();
    for w in workers {
        assert_eq!(w.join().unwrap(), here);
    }
}

/// The toe brakes hold the aircraft at full power on the runway, as for a run-up, and a braked
/// aircraft does not weathervane in a strong crosswind.
#[test]
fn the_brakes_hold_it() {
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Home);
    let p0 = world.vehicle(id).aircraft().unwrap().state.pos;
    let c = &mut world.vehicle_mut(id).aircraft_mut().unwrap().controls;
    (c.throttle, c.brake_left, c.brake_right) = (1.0, 1.0, 1.0);
    for _ in 0..240 * 10 {
        world.step();
    }
    let moved = (world.vehicle(id).aircraft().unwrap().state.pos - p0).length();
    println!("full power on the brakes: moved {moved:.2} m, {:.0} rpm", world.vehicle(id).aircraft().unwrap().rpm());
    assert!(moved < 1.0);

    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Home);
    let h = euler_angles(world.vehicle(id).aircraft().unwrap().state.q).heading;
    world.weather.wind_speed = 30.0 * KNOTS;
    world.weather.wind_from = h / DEG + 90.0;
    for _ in 0..240 * 20 {
        world.step();
    }
    let turned = remainder(euler_angles(world.vehicle(id).aircraft().unwrap().state.q).heading - h, 2.0 * std::f64::consts::PI) / DEG;
    println!("30 kt across, throttle closed: turned {turned:.1}° in 20 s");
    assert!(turned.abs() < 5.0);
}

#[test]
fn an_aircraft_placed_by_hand_reads_its_new_place_at_once() {
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
    for _ in 0..240 {
        world.step();
    }
    // 500 m lower, as the game's `--low` puts it: no descent to chase on the dials or the air data.
    let mut s = world.vehicle(id).aircraft().unwrap().state;
    s.pos.z += 500.0;
    world.place(id, s);
    let mut worst: f64 = 0.0;
    for _ in 0..240 {
        world.step();
        let p = world.vehicle(id);
        let r = p.aircraft().unwrap().instruments.readings();
        worst = worst.max(r.vertical_speed.abs()).max(r.air_data.vertical_speed.abs()).max(p.aircraft().unwrap().vertical_speed().abs());
    }
    let p = world.vehicle(id);
    println!("worst vertical speed read after the move: {worst:.2} m/s");
    assert!(worst < 5.0, "{worst} m/s");
    assert!((p.aircraft().unwrap().indicated_altitude() + p.aircraft().unwrap().state.pos.z).abs() < 15.0);
    assert!((p.aircraft().unwrap().instruments.readings().altitude + p.aircraft().unwrap().state.pos.z).abs() < 30.0);
}

#[test]
fn lakes_fill_river_valleys_away_from_aerodromes_and_towns_and_float_aircraft() {
    let t = Terrain::new(1);
    let lakes = t.lakes(-60_000.0, 60_000.0, -60_000.0, 60_000.0);
    assert!(lakes.len() >= 8, "{} lakes", lakes.len());
    for lake in &lakes {
        let reach = lake.reach();
        // Away from what was placed on the dry ground: no aerodrome nor town changes.
        let pad = reach + Terrain::AIRPORT_REACH;
        assert!(t.airports(lake.n - pad, lake.n + pad, lake.e - pad, lake.e + pad).is_empty(), "{lake:?}");
        assert!(t.towns(lake.n - reach, lake.n + reach, lake.e - reach, lake.e + reach).is_empty(), "{lake:?}");
        assert!(lake.level > 2.0);
        // Its middle is on a river and under its water, which is flat at its level wherever it is.
        let centre = t.raw(lake.n, lake.e);
        assert!(centre.valley > 0.9 && centre.river > 0.99 && centre.h == lake.level, "{lake:?}: {centre:?}");
        let (mut water, mut land) = (0, 0);
        for i in -20..=20 {
            for j in -20..=20 {
                let (n, e) = (lake.n + i as f64 * reach / 20.0, lake.e + j as f64 * reach / 20.0);
                let g = t.ground(n, e);
                // Its water is all at its level (a river may run through above it, flowing in).
                if g.river > 0.5 && g.h == lake.level {
                    water += 1;
                } else if g.river < 0.5 {
                    land += 1;
                    assert!(g.h >= lake.level - 1e-9 || lake.weight(n, e, 1) < 1.0 || t.raw(n, e).valley < 0.25, "{n} {e}: dry land under the lake's level, {g:?}");
                }
                // Nothing round it is below sea level.
                assert!(g.h > 0.0);
            }
        }
        assert!(water > 20 && land > 20, "{lake:?}: {water} of water, {land} of land");
    }
    // An aircraft on a lake floats on its level.
    let lake = lakes[0];
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n: lake.n, e: lake.e, heading: 0.0 });
    world.vehicle_mut(id).aircraft_mut().unwrap().state.pos.z = -(lake.level + 3.0);
    world.step();
    let env = world.vehicle(id).environment();
    assert_eq!((env.kind, env.ground_height), (GroundKind::Water, lake.level));
}

/// A new start fills the tanks again: a crash spills the fuel (all of it when the airframe breaks
/// up), and the restart must not leave the engine with nothing to run on. With `burns_fuel` off
/// a flight never uses any.
#[test]
fn a_restart_refuels_and_fuel_burns_only_when_chosen() {
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
    let full = world.vehicle(id).aircraft().unwrap().load().fuel;
    assert!(full > 0.0);

    // Straight into the ground at 90 kt, 35° nose down: it breaks up and spills everything.
    let ground = world.terrain.height(0.0, 0.0).max(0.0);
    let s = &mut world.vehicle_mut(id).aircraft_mut().unwrap().state;
    s.pos.z = -(ground + 40.0);
    s.q = quaternion(0.0, -35.0 * DEG, 0.0);
    s.vel = V3::new(46.0, 0.0, 0.0);
    for _ in 0..240 * 10 {
        world.step();
    }
    assert!(world.vehicle(id).aircraft().unwrap().damage().broken_up);
    assert_eq!(world.vehicle(id).aircraft().unwrap().load().fuel, 0.0);

    world.reset(id, Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
    assert_eq!(world.vehicle(id).aircraft().unwrap().load().fuel, full);
    for _ in 0..240 * 5 {
        world.step();
    }
    let a = world.vehicle(id).aircraft().unwrap();
    assert!(a.rpm() > 1500.0, "the engine runs after the restart: {} rpm", a.rpm());
    assert!(a.load().fuel < full, "burnt by default in the simulation");

    world.reset(id, Start::Air { n: 0.0, e: 0.0, heading: 0.0 });
    world.vehicle_mut(id).aircraft_mut().unwrap().burns_fuel = false;
    for _ in 0..240 * 5 {
        world.step();
    }
    assert_eq!(world.vehicle(id).aircraft().unwrap().load().fuel, full);
}
