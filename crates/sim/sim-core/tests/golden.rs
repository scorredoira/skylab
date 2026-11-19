//! Two kinds of reference values.
//!
//! **The world** (the `GOLDEN` table at the end of this file): noise and hash functions, terrain,
//! airports, towns, roads, wind, printed by the Swift implementation this crate was ported from.
//! They are fixed: a change that moves any of them changes every world and is a bug.
//!
//! **The flight model** (`tests/physics_snapshot.txt`): rigging, stall, trim, forces, whole
//! flights and keyboard-pilot runs. These started as the Swift reference too, but the flight model
//! is meant to improve, so they are a regression snapshot: they fail on any unintended change and
//! are regenerated deliberately, after the handbook and JSBSim tests pass, with
//!
//! ```text
//! FLIGHT_SNAPSHOT=update cargo test -p sim-core --test golden
//! ```
//!
//! and the diff of the snapshot file is reviewed and committed with the change that caused it.

mod common;

use std::collections::BTreeMap;
use std::sync::Mutex;

use common::Sim;
use sim_core::*;

const SNAPSHOT_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/physics_snapshot.txt");
/// Serialises the tests when they rewrite the snapshot file.
static SNAPSHOT_LOCK: Mutex<()> = Mutex::new(());

/// The snapshot file: one line per key, the key followed by its values.
fn read_snapshot() -> BTreeMap<String, Vec<f64>> {
    let text = std::fs::read_to_string(SNAPSHOT_FILE).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let key = words.next()?.to_string();
            Some((key, words.map(|w| w.parse().unwrap()).collect()))
        })
        .collect()
}

/// Largest error of `got` against the reference values of `key`, each relative to
/// `max(|reference|, floor)`. Fails if it exceeds `tolerance`. World keys are looked up in
/// `GOLDEN`; any other key belongs to the flight-model snapshot.
#[track_caller]
fn check(key: &str, got: &[f64], floor: f64, tolerance: f64) -> f64 {
    let fixed = GOLDEN
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_vec());
    let want = match fixed {
        Some(v) => v,
        None => {
            let _guard = SNAPSHOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut all = read_snapshot();
            if std::env::var("FLIGHT_SNAPSHOT").as_deref() == Ok("update") {
                all.insert(key.to_string(), got.to_vec());
                let text: String = all
                    .iter()
                    .map(|(k, v)| {
                        let values: Vec<String> = v.iter().map(|x| format!("{x:?}")).collect();
                        format!("{k} {}\n", values.join(" "))
                    })
                    .collect();
                std::fs::write(SNAPSHOT_FILE, text).unwrap();
                println!("SNAPSHOT {key}: updated");
                return 0.0;
            }
            all.remove(key)
                .unwrap_or_else(|| panic!("no reference values for {key}"))
        }
    };
    assert_eq!(got.len(), want.len(), "{key}: number of values");
    let mut worst = 0.0_f64;
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        let err = (g - w).abs() / w.abs().max(floor);
        assert!(
            err <= tolerance,
            "{key}[{i}]: got {g}, reference {w}, relative error {err:e}"
        );
        worst = worst.max(err);
    }
    println!("GOLDEN {key}: max relative error {worst:e}");
    worst
}

fn v3(v: V3) -> [f64; 3] {
    v.to_array()
}

fn flight_data(d: &FlightData) -> Vec<f64> {
    let mut out = Vec::new();
    out.extend(v3(d.force));
    out.extend(v3(d.moment));
    out.push(d.prop_accel);
    out.extend(v3(d.specific));
    out.extend([
        d.alpha,
        d.beta,
        d.tas,
        d.ias,
        d.thrust,
        d.engine_torque,
        d.downwash_cl,
        d.stall_warning as u8 as f64,
        d.stalled,
    ]);
    // The three legs the aircraft of these numbers has.
    out.extend(&d.wheel_load[..3]);
    out.extend([
        d.hard_contact.map_or(-1.0, |i| i as f64),
        d.hard_impact,
        d.in_water as u8 as f64,
    ]);
    out
}

fn sim_state(sim: &Sim) -> Vec<f64> {
    let p = sim.vehicle();
    let (s, c) = (p.aircraft().unwrap().state, p.aircraft().unwrap().controls);
    let mut out = Vec::new();
    out.extend(v3(s.pos));
    out.extend(v3(s.vel));
    out.extend([s.q.x, s.q.y, s.q.z, s.q.w]);
    out.extend(v3(s.omega));
    out.extend([
        s.prop_speed,
        p.aircraft().unwrap().indicated_airspeed(),
        p.aircraft().unwrap().indicated_altitude(),
        p.aircraft().unwrap().vertical_speed(),
        p.aircraft().unwrap().turn_rate(),
        p.aircraft().unwrap().slip_ball(),
        p.agl(),
        p.aircraft().unwrap().flap_deg,
        p.aircraft().unwrap().downwash,
        c.throttle,
        c.trim,
        c.flap_notch as f64,
        p.aircraft().unwrap().last_touchdown().unwrap_or(-999.0),
    ]);
    out
}

// The noise is bit-exact, so its tolerance is only there for the decimal printing.
const EXACT: f64 = 1e-15;
// Terrain and everything derived from it.
const TERRAIN: f64 = 1e-9;

#[test]
fn math() {
    let hp: [(i64, i64, u64); 6] = [
        (0, 0, 0),
        (1, 2, 3),
        (-1, -7, 99),
        (123456, -654321, 1011),
        (-9_000_000, 4_000_000, 0xFFFF_FFFF_FFFF_FFF0),
        (7, 7, 1),
    ];
    check("hash2", &hp.map(|(x, y, s)| hash2(x, y, s)), 1.0, EXACT);
    let np: [(f64, f64, u64); 5] = [
        (0.3, 0.7, 1),
        (-1.25, 3.5, 12),
        (1234.567, -987.654, 101),
        (-0.0001, -0.9999, 5),
        (17.0, -4.0, 9),
    ];
    let noise: Vec<f64> = np
        .iter()
        .flat_map(|&(x, y, s)| {
            let r = noised(x, y, s);
            [r.v, r.dx, r.dy]
        })
        .collect();
    check("noised", &noise, 1.0, EXACT);
    check(
        "fbm",
        &[
            fbm(0.3, 0.7, 3, 12),
            fbm(-12.5, 88.25, 5, 60),
            fbm(1e3 + 0.1, -2e3 + 0.9, 9, 0),
            fbm(-0.5, -0.5, 3, u64::MAX),
        ],
        1.0,
        EXACT,
    );
    let mut r = Rng::new(42);
    check(
        "rng",
        &[
            r.uniform(),
            r.uniform(),
            r.normal(),
            r.normal(),
            r.uniform(),
        ],
        1.0,
        EXACT,
    );

    let q = quaternion(1.1, -0.3, 0.7);
    let e = euler_angles(q);
    let mut qv = vec![q.x, q.y, q.z, q.w, e.heading, e.pitch, e.roll];
    qv.extend(v3(act(q, V3::new(1.0, 2.0, 3.0))));
    qv.extend(v3(act(inverse(q), V3::new(1.0, 2.0, 3.0))));
    check("quat", &qv, 1.0, 1e-14);
    let tau = 2.0 * std::f64::consts::PI;
    check(
        "remainder",
        &[
            remainder(3.5, tau),
            remainder(-3.5, tau),
            remainder(10.0, tau),
            remainder(0.1, tau),
            remainder(-7.0, tau),
        ],
        1.0,
        EXACT,
    );
    check(
        "atmosphere",
        &[
            atmosphere::density(0.0),
            atmosphere::density(2438.4),
            atmosphere::pressure(3000.0),
            atmosphere::temperature(12000.0),
            atmosphere::density(-900.0),
        ],
        1.0,
        1e-14,
    );
    check(
        "interpolate",
        &[
            interpolate(AircraftType::c172().prop_j, AircraftType::c172().prop_ct, 0.55),
            interpolate(AircraftType::c172().prop_j, AircraftType::c172().prop_cp, 1.5),
            interpolate(AircraftType::c172().prop_j, AircraftType::c172().prop_ct, 3.0),
            interpolate(AircraftType::c172().flap_notches, AircraftType::c172().flap_lift_curve, 17.0),
        ],
        1.0,
        EXACT,
    );
}

#[test]
fn terrain_height() {
    let t = Terrain::new(1);
    let mut raw = Vec::new();
    for i in -4..=4 {
        for j in -4..=4 {
            let p = t.raw(i as f64 * 7919.3, j as f64 * 7919.3);
            raw.extend([p.h, p.river, p.valley]);
        }
    }
    check("raw.grid", &raw, 1.0, TERRAIN);
    raw.clear();
    for i in 0..120 {
        let p = t.raw(i as f64 * 311.7 - 9000.0, i as f64 * -173.3 + 500.0);
        raw.extend([p.h, p.river, p.valley]);
    }
    check("raw.line", &raw, 1.0, TERRAIN);
    raw.clear();
    let t9 = Terrain::new(0xDEAD_BEEF_0000_0009);
    for i in 0..30 {
        let p = t9.raw(i as f64 * 4311.7 - 60000.0, i as f64 * 2173.3 - 30000.0);
        raw.extend([p.h, p.river, p.valley]);
    }
    check("raw.seed9", &raw, 1.0, TERRAIN);
}

#[test]
fn airports_towns_and_roads() {
    let t = Terrain::new(1);
    for x in -3..=3 {
        for y in -3..=3 {
            let got = match t.airport((x, y)) {
                Some(a) => vec![
                    1.0,
                    a.n,
                    a.e,
                    a.heading,
                    a.elevation,
                    a.id as f64,
                    a.length,
                    a.width,
                    a.number() as f64,
                ],
                None => vec![0.0],
            };
            check(&format!("airport:{x}:{y}"), &got, 1.0, TERRAIN);
        }
    }
    for x in -4..=4 {
        for y in -4..=4 {
            let got = match t.town((x, y)) {
                Some(a) => vec![1.0, a.n, a.e, a.radius, a.grid],
                None => vec![0.0],
            };
            check(&format!("town:{x}:{y}"), &got, 1.0, TERRAIN);
        }
    }
    let roads = t.roads(-30000.0, 30000.0, -30000.0, 30000.0);
    let mut got = vec![roads.len() as f64];
    got.extend(roads.iter().flat_map(|r| [r.n0, r.e0, r.n1, r.e1]));
    check("roads", &got, 1.0, TERRAIN);
    let towns = t.towns(-20000.0, 20000.0, -20000.0, 20000.0);
    let mut got = vec![towns.len() as f64];
    got.extend(towns.iter().flat_map(|t| [t.n, t.e]));
    check("towns.rect", &got, 1.0, TERRAIN);
    let airports = t.airports(-20000.0, 20000.0, -20000.0, 20000.0);
    let mut got = vec![airports.len() as f64];
    got.extend(airports.iter().flat_map(|a| [a.n, a.e]));
    check("airports.rect", &got, 1.0, TERRAIN);
    let home = t.nearest_airport(0.0, 0.0).unwrap();
    let far = t.nearest_airport(123456.0, -98765.0).unwrap();
    check("nearest", &[home.n, home.e, far.n, far.e], 1.0, TERRAIN);
}

#[test]
fn ground_and_surface() {
    let t = Terrain::new(1);
    let home = t.nearest_airport(0.0, 0.0).unwrap();
    let (c, s) = (home.heading.cos(), home.heading.sin());
    let mut got = Vec::new();
    for along in [0.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1200.0] {
        for across in [0.0, 100.0, 250.0, 500.0] {
            let (n, e) = (
                home.n + along * c - across * s,
                home.e + along * s + across * c,
            );
            let g = t.ground(n, e);
            got.extend([g.h, g.river, g.flat, home.flat_weight(n, e)]);
        }
    }
    check("ground", &got, 1.0, TERRAIN);
    got.clear();
    for i in 0..40 {
        let (n, e) = (i as f64 * 1511.7 - 30000.0, i as f64 * -873.3 + 17000.0);
        let g = t.ground(n, e);
        let sf = t.surface(
            n,
            e,
            &g,
            0.02 * (i % 7) as f64,
            if i % 5 == 0 { 0.4 } else { 0.0 },
        );
        got.extend(
            [
                sf.color.x, sf.color.y, sf.color.z, sf.farm, sf.water, sf.forest,
            ]
            .map(f64::from),
        );
    }
    // Single precision values.
    check("surface", &got, 1.0, 1e-7);
}

#[test]
fn wind() {
    let weather = Weather {
        wind_from: 200.0,
        wind_speed: 8.0,
        turbulence: 0.7,
        ..Weather::default()
    };
    let mut w = WindField::new(5);
    let mut got = Vec::new();
    got.extend(v3(weather.mean_wind(50.0)));
    got.extend(v3(weather.mean_wind(1200.0)));
    got.extend(v3(weather.mean_wind(0.5)));
    for _ in 0..100 {
        w.update(&weather, 1.0 / 240.0, 120.0, 50.0, 0.05, -0.02);
    }
    got.extend(v3(w.air_velocity()));
    got.push(w.roll_gust());
    check("wind", &got, 1.0, 1e-12);
}

#[test]
fn aircraft_rigging_stall_and_trim() {
    let a = Aircraft::new();
    check(
        "rig",
        &[a.p.fin_rig, a.p.roll_rig, a.aileron_gain()],
        1.0,
        1e-9,
    );
    let strips: Vec<f64> = a
        .strips()
        .iter()
        .flat_map(|s| [s.y, s.weight, s.twist])
        .collect();
    check("strips", &strips, 1.0, 1e-14);
    let (s0, s30) = (a.stall(0.0, 0.0), a.stall(30.0, 0.0));
    check(
        "stall",
        &[s0.cl_max, s0.speed, s30.cl_max, s30.speed],
        1.0,
        1e-9,
    );

    let trim = |key: &str, t: Option<Trim>| {
        let got = t.map_or(vec![0.0], |t| {
            vec![
                1.0,
                t.alpha,
                t.elevator,
                t.throttle,
                t.climb_angle,
                t.prop_speed,
            ]
        });
        check(key, &got, 1.0, 1e-6)
    };
    trim("trim.cruise", a.trim_level(100.0 * KNOTS, 1500.0));
    trim("trim.climb", a.trim(73.0 * KNOTS, 0.0, 0.0, Some(1.0), 0.0));
    trim(
        "trim.final",
        a.trim(68.0 * KNOTS, 300.0, 20.0, None, -3.0 * DEG),
    );
    trim(
        "trim.glide",
        a.trim(65.0 * KNOTS, 600.0, 30.0, Some(0.0), 0.0),
    );
    trim("trim.high", a.trim_level(115.0 * KNOTS, 8000.0 * FEET));
    trim("trim.toofast", a.trim_level(140.0 * KNOTS, 8000.0 * FEET));

    check(
        "engine",
        &[
            a.engine_torque(0.6, 240.0, 1.1),
            a.engine_torque(0.0, 60.0, 1.225),
            a.engine_torque(1.0, 280.0, 0.9),
            a.engine_torque(0.0, 20.0, 1.225),
        ],
        1.0,
        1e-13,
    );
    let (d, prop_speed) = a.steady(50.0, 500.0, 0.05, 0.02, 0.1, 0.7, 10.0);
    let mut got = flight_data(&d);
    got.push(prop_speed);
    check("steady", &got, 1.0, FORCES);
}

// Forces in newtons and moments in newton metres, relative to at least 1.
const FORCES: f64 = 1e-9;

#[test]
fn forces() {
    let a = Aircraft::new();
    {
        let mut s = State {
            pos: V3::new(100.0, -50.0, -800.0),
            vel: V3::new(52.0, 3.0, 4.0),
            q: quaternion(2.0, 0.1, -0.4),
            omega: V3::new(0.2, -0.1, 0.15),
            prop_speed: 230.0,
            ..State::default()
        };
        let mut c = AircraftControls {
            aileron: 0.3,
            elevator: -0.2,
            rudder: 0.4,
            throttle: 0.8,
            trim: 0.1,
            ..AircraftControls::default()
        };
        let env = Environment {
            rho: 1.1,
            air_velocity: V3::new(3.0, -5.0, 1.0),
            roll_gust: 0.02,
            ..Environment::default()
        };
        check(
            "forces.air",
            &flight_data(&a.forces(&s, &c, &env, 0.03, 15.0)),
            1.0,
            FORCES,
        );
        c.elevator = 0.6;
        c.aileron = -0.8;
        c.rudder = -1.0;
        s.vel = V3::new(20.0, -2.0, 12.0);
        s.omega = V3::new(-0.5, 0.3, -0.4);
        check(
            "forces.stalled",
            &flight_data(&a.forces(&s, &c, &Environment::default(), 0.05, 30.0)),
            1.0,
            FORCES,
        );
    }
    {
        let mut env = Environment {
            ground_height: 50.0,
            ref_n: 10.0,
            ref_e: 20.0,
            slope_n: 0.02,
            slope_e: -0.03,
            kind: GroundKind::Runway,
            ..Environment::default()
        };
        let mut s = State {
            pos: V3::new(12.0, 18.0, -51.0),
            vel: V3::new(20.0, 1.5, 0.3),
            q: quaternion(0.5, 0.05, 0.08),
            omega: V3::new(0.05, 0.02, 0.3),
            prop_speed: 120.0,
            ..State::default()
        };
        let mut c = AircraftControls {
            rudder: 0.6,
            brake_left: 0.5,
            brake_right: 0.2,
            throttle: 0.2,
            ..AircraftControls::default()
        };
        check(
            "forces.runway",
            &flight_data(&a.forces(&s, &c, &env, 0.0, 0.0)),
            1.0,
            FORCES,
        );
        c.parking_brake = true;
        env.kind = GroundKind::Grass;
        s.pos = V3::new(12.0, 18.0, -51.3);
        s.q = quaternion(-1.0, -0.1, 0.5);
        check(
            "forces.wingtip",
            &flight_data(&a.forces(&s, &c, &env, 0.0, 0.0)),
            1.0,
            FORCES,
        );
        env.kind = GroundKind::Water;
        check(
            "forces.water",
            &flight_data(&a.forces(&s, &c, &env, 0.0, 0.0)),
            1.0,
            FORCES,
        );
    }
}

// Whole flights: thousands of steps of the integrator, so rounding differences have grown.
const FLIGHT: f64 = 1e-8;

#[test]
fn flights() {
    {
        let mut sim = Sim::new(1);
        let home = sim.vehicle().home.unwrap();
        let mut got = sim_state(&sim);
        got.extend([home.n, home.e]);
        check("sim.start", &got, 1.0, TERRAIN);
        sim.aircraft_mut().controls.throttle = 1.0;
        for _ in 0..240 * 8 {
            sim.step();
        }
        let mut got = sim_state(&sim);
        got.push(sim.world.time());
        check("sim.roll8s", &got, 1.0, FLIGHT);
    }
    {
        let mut sim = Sim::new(3);
        sim.world.weather.wind_speed = 10.0;
        sim.world.weather.turbulence = 1.0;
        sim.reset_in_air(0.0, 0.0, 1.0);
        check("sim.air0", &sim_state(&sim), 1.0, FLIGHT);
        for _ in 0..240 * 5 {
            sim.step();
        }
        let mut got = sim_state(&sim);
        got.extend(v3(sim.vehicle().environment().air_velocity));
        got.push(sim.vehicle().environment().roll_gust);
        check("sim.turb5s", &got, 1.0, FLIGHT);
    }
    {
        let mut sim = Sim::new(1);
        let home = sim.vehicle().home.unwrap();
        sim.world.reset(
            sim.id,
            Start::Final {
                airport: home,
                reversed: false,
                distance: Vehicle::FINAL_DISTANCE,
            },
        );
        check("sim.final0", &sim_state(&sim), 1.0, FLIGHT);
        for _ in 0..240 * 3 {
            sim.step();
        }
        check("sim.final3s", &sim_state(&sim), 1.0, FLIGHT);
    }
}

/// Flies `steps` steps with the given keys held.
fn fly(
    pilot: &mut KeyboardPilot,
    sim: &mut Sim,
    steps: usize,
    roll: f64,
    pitch: f64,
    pedals: f64,
    fine: f64,
) {
    pilot.set_keys(roll, pitch, pedals);
    pilot.fine = fine;
    for _ in 0..steps {
        sim.fly(pilot);
    }
}

fn keys_state(pilot: &KeyboardPilot, sim: &Sim) -> Vec<f64> {
    let mut out = vec![pilot.yoke_x(), pilot.yoke_y(), pilot.pedal()];
    out.extend(sim_state(sim));
    out
}

#[test]
fn keyboard_pilot() {
    {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        let mut pilot = KeyboardPilot::new();
        fly(&mut pilot, &mut sim, 480, 0.0, 0.0, 0.0, 0.0);
        fly(&mut pilot, &mut sim, 480, 0.0, -1.0, 0.0, 0.0);
        fly(&mut pilot, &mut sim, 480, 1.0, 0.0, 0.0, 1.0);
        fly(&mut pilot, &mut sim, 240, 0.0, 0.0, 1.0, 0.0);
        check("keys.assist.air", &keys_state(&pilot, &sim), 1.0, FLIGHT);
        pilot.mouse_yoke = Some((0.2, -0.1));
        pilot.mouse_rudder = Some(0.3);
        fly(&mut pilot, &mut sim, 120, 0.0, 0.0, 0.0, 0.0);
        pilot.mouse_yoke = None;
        pilot.mouse_rudder = None;
        fly(&mut pilot, &mut sim, 240, 0.0, 0.0, 0.0, 0.0);
        check("keys.assist.mouse", &keys_state(&pilot, &sim), 1.0, FLIGHT);
        pilot.assist = false;
        fly(&mut pilot, &mut sim, 240, -1.0, 1.0, -1.0, 0.0);
        pilot.mouse_rudder = Some(0.1);
        fly(&mut pilot, &mut sim, 60, 0.0, 0.0, 0.0, 0.0);
        pilot.mouse_rudder = None;
        pilot.assist = true;
        fly(&mut pilot, &mut sim, 240, 0.0, 0.0, 0.0, 0.0);
        check("keys.mixed.air", &keys_state(&pilot, &sim), 1.0, FLIGHT);
    }
    {
        let mut sim = Sim::new(1);
        let mut pilot = KeyboardPilot::new();
        pilot.assist = false;
        sim.aircraft_mut().controls.throttle = 0.3;
        fly(&mut pilot, &mut sim, 720, 0.0, 0.0, 0.0, 0.0);
        fly(&mut pilot, &mut sim, 480, -1.0, 0.0, 0.0, 0.0);
        check("keys.direct.ground", &keys_state(&pilot, &sim), 1.0, FLIGHT);
    }
    {
        let mut sim = Sim::new(1);
        let mut pilot = KeyboardPilot::new();
        sim.aircraft_mut().controls.throttle = 1.0;
        fly(&mut pilot, &mut sim, 240 * 6, 1.0, 0.0, 0.0, 0.0);
        fly(&mut pilot, &mut sim, 240 * 6, 0.0, 1.0, 0.0, 0.0);
        for _ in 0..240 * 15 {
            let pull = sim.vehicle().aircraft().unwrap().indicated_airspeed() > 55.0 * KNOTS
                && euler_angles(sim.aircraft().state.q).pitch / DEG < 8.0;
            fly(
                &mut pilot,
                &mut sim,
                1,
                0.0,
                if pull { 1.0 } else { 0.0 },
                0.0,
                0.0,
            );
        }
        check(
            "keys.assist.takeoff",
            &keys_state(&pilot, &sim),
            1.0,
            FLIGHT,
        );
    }
}

// MARK: - Reference values

#[rustfmt::skip]
const GOLDEN: &[(&str, &[f64])] = &[
    ("hash2", &[0.0, 0.5322488954716162, 0.4157788245630787, 0.15199474101423094, 0.10758369298034376, 0.01307270159127516]),
    ("noised", &[0.2394655699853856, -0.2129331608448834, 0.7843548342553879, -0.43924000440397615, -0.271654950666769, -0.28917901794779677, -0.1320634874999278, -2.3761170089701786, 0.9537444466831859, 0.27207239203756894, -1.1960296529242062e-07, -2.4352766758953987e-07, 0.7382017115935462, 0.0, -0.0]),
    ("fbm", &[-0.2728895168255959, -0.23501391753780368, -0.16710603750105615, 0.02710098410553966]),
    ("rng", &[0.7694437664417122, 0.9378199012028331, -1.6236092258362158, 2.1099339994165645, 0.2923972813794926]),
    ("quat", &[0.3624200943552256, 0.057539988180335386, 0.5291698089444968, 0.7650621793484506, 1.1, -0.3, 0.7, 0.31218093386704227, -0.27502603156937244, 3.718454483584423, 3.0227033669957053, 1.4328696260496683, 1.6763499007955103]),
    ("remainder", &[-2.7831853071795862, 2.7831853071795862, -2.5663706143591725, 0.1, -0.7168146928204138]),
    ("atmosphere", &[1.2250122659906946, 0.9628796297432257, 70108.52557600716, 216.64999999999998, 1.2849034738418061]),
    ("interpolate", &[0.06499999999999999, -0.020499999999999997, -0.15, 0.681]),
    ("raw.grid", &[177.96317614937823, 0.0, 0.0, 167.0758353904613, 0.0, 0.8331687059537228, 166.92249931856728, 0.0, 0.0, 145.91134116836156, 0.0, 0.0, 220.90655231149, 0.0, 0.0, 287.61164631206816, 0.0, 0.0, 234.72104579104305, 0.0, 0.0, 189.72889019944733, 0.0, 0.0, 120.06029070404423, 0.0, 0.0, 139.86460995789693, 0.0, 0.0, 244.17278311602854, 0.0, 0.0, 273.9594122810234, 0.0, 0.0, 329.1822755604844, 0.0, 0.0, 309.5121912610595, 0.0, 0.0, 314.2179848258471, 0.0, 0.0, 245.36190490233602, 0.0, 0.0, 167.05243644416962, 0.0, 0.0, 36.11406525915814, 0.0, 0.0, 228.74222760870802, 0.0, 0.0, 284.93418282455616, 0.0, 0.4355974445499239, 405.9152181940189, 0.0, 0.0, 784.3077769056509, 0.0, 0.0, 292.15648524524363, 0.0, 0.0, 265.33973118451877, 0.0, 0.0, 235.25078136144393, 0.0, 0.0, 148.67686705014796, 0.0, 0.0, 10.121525082893484, 0.0, 0.0, 621.4273606556335, 0.0, 0.0, 484.70686688105104, 0.0, 0.0, 920.7923178931883, 0.0, 0.0, 618.4760049088964, 0.0, 0.0, 345.9526983341777, 0.0, 0.0, 330.8956985221164, 0.0, 0.0, 228.29356770042463, 0.0, 0.0, 117.42352512508673, 0.4365621904205109, 0.9822208459890374, 14.179105719346996, 0.0, 0.0, 144.67475558401046, 0.0, 0.23836589228622718, 236.79830950048304, 0.0, 7.878052699528126e-05, 304.06924533799463, 0.0, 0.14136600087713236, 379.0284971747003, 0.0, 0.0, 328.40515403074716, 0.0, 0.0, 321.5491495018989, 0.0, 0.0, 201.96966184502182, 0.0, 0.0, 29.168068072304045, 0.0, 0.21866265001041474, -56.84139369295676, 0.0, 0.0, 165.01376174686095, 0.0, 0.0, 212.84234807798637, 0.0, 0.0, 240.21255752512454, 0.0, 0.0, 428.9843833879299, 0.0, 0.0, 354.77548813377734, 0.0, 0.0, 333.6221613614964, 0.0, 0.0, 109.01301441035625, 0.0, 0.0, 31.216576358855757, 0.0, 0.0, -77.29141996384925, 0.0, 0.0, 117.73393205817769, 0.0, 0.0, 204.62698003186006, 0.0, 0.0, 158.363195065163, 0.0, 0.0, 425.09384016013087, 0.0, 0.0, 278.2007091830945, 0.0, 0.01404359346747297, 227.39630494530007, 0.0, 0.06599396884855069, 125.00919519734121, 0.0, 0.0, 62.696425657116706, 0.0, 0.0, -5.582204845484753, 0.0, 0.0, 98.09130964517432, 0.0, 0.0, 142.3861038617076, 0.0, 0.0, 168.562014752431, 0.0, 0.0, 188.08388742170416, 0.0, 0.0, 189.84072335743537, 0.0, 0.0, 130.52048231499776, 0.0, 0.0, 119.64643018439673, 0.0, 0.0, 82.65928053247488, 0.0, 0.7866603423027132, 147.62946535557114, 0.0, 0.0, 114.81053494143625, 0.0, 0.0, 118.15322162678386, 0.0, 0.0, 131.24867558818602, 0.0, 0.0, 169.88614454017176, 0.0, 0.0, 164.50057855475097, 0.0, 0.0, 122.91736702658534, 0.0, 0.0, 98.13562075880132, 0.0, 0.0, 81.38751363542406, 0.0, 0.0, 214.09077699237443, 0.0, 0.0]),
    ("raw.line", &[349.15997726613057, 0.0, 0.0, 350.358631689309, 0.0, 0.0, 353.35240685773, 0.0, 0.0, 346.8290186610625, 0.0, 0.0, 341.0152936742616, 0.0, 0.0, 347.6524023901978, 0.0, 0.0, 364.45662046374395, 0.0, 0.0, 360.67710038262334, 0.0, 0.0, 357.3341008423507, 0.0, 0.0, 352.2486168101692, 0.0, 0.0, 356.71330087134277, 0.0, 0.0, 361.7286812977417, 0.0, 0.0, 361.1754935736046, 0.0, 0.0, 353.786023043486, 0.0, 0.0, 344.7364430976586, 0.0, 0.0, 330.9973874358226, 0.0, 0.0, 333.3999945676415, 0.0, 0.0, 330.4864864354348, 0.0, 0.0, 327.3980860820963, 0.0, 0.0, 327.5931651386519, 0.0, 0.0, 328.6174673002388, 0.0, 0.0, 335.48320453825374, 0.0, 0.0, 334.5300398397744, 0.0, 0.0, 340.87475946065007, 0.0, 0.0, 349.19357745318655, 0.0, 0.0, 353.36767294136814, 0.0, 0.0, 351.52846366982754, 0.0, 0.0, 354.04773367747737, 0.0, 0.0, 357.81580386348713, 0.0, 0.0, 368.50732450594745, 0.0, 0.0, 354.59885515000667, 0.0, 0.0, 358.95746361648474, 0.0, 0.0, 376.880867455963, 0.0, 0.0, 401.92988556511654, 0.0, 0.0, 416.5443664812707, 0.0, 0.0, 421.2546698807497, 0.0, 0.0, 420.3466393484209, 0.0, 0.12447614727802525, 352.476509941081, 0.0, 0.861964613745438, 363.6042792574261, 0.0, 0.8408065131445553, 458.6956723082916, 0.0, 0.25209073711191066, 530.3673491301291, 0.0, 0.0, 553.7567630898914, 0.0, 0.0, 587.905101297176, 0.0, 0.0, 607.7110272317827, 0.0, 0.0, 592.5017520824529, 0.0, 0.0, 504.58555544445534, 0.0, 0.0, 479.67932433663685, 0.0, 0.0, 458.65816259294525, 0.0, 0.03078758240792101, 444.4384498231758, 0.0, 0.30555507860500364, 402.92321936726603, 0.0, 0.7366963647606557, 295.9214018163599, 1.0, 0.9965589421771669, 334.6130787358026, 0.0, 0.880031046861938, 379.6442832659203, 0.0, 0.7083529678665329, 383.7673669349994, 0.0, 0.6461676556289238, 382.679644304113, 0.0, 0.6817391586932111, 357.5241475644775, 0.0, 0.7592453466719606, 327.37553841736525, 0.0, 0.814952983010574, 310.58741521808406, 0.0, 0.8482183512345545, 292.34530764241435, 0.0, 0.8780568768690131, 273.2274862863766, 0.0, 0.9119913733284692, 260.8443606405175, 0.0, 0.9402147823750633, 251.90052263825305, 0.0, 0.9589558719533849, 244.78548787898532, 0.0, 0.972724791279467, 239.31880378413416, 0.605062079761908, 0.9837247186915292, 234.93303828346885, 1.0, 0.9899491593578382, 230.20125924562396, 1.0, 0.9926021586090347, 224.9195376484022, 1.0, 0.9966891661693411, 219.15669038725284, 1.0, 0.9999674250166035, 213.07392958443478, 0.9468775347871004, 0.9872954697030264, 220.18013600731896, 0.0, 0.9365249079271756, 248.5322465014002, 0.0, 0.8684072669603311, 253.51201904658348, 0.0, 0.8460394417370474, 223.48135813172513, 0.0, 0.8946236324094827, 183.37875305384034, 0.0, 0.9697205774308156, 173.48589982326806, 1.0, 0.9999757117590965, 168.23969370963619, 0.06657390955863296, 0.977921230828781, 193.62892062029377, 0.0, 0.9304387925326247, 248.61285384800215, 0.0, 0.8446792013220943, 289.07721916019216, 0.0, 0.6967358894315856, 316.1961663753925, 0.0, 0.48183767944512645, 333.8253731469819, 0.0, 0.2422227397176021, 285.0274718816773, 0.0, 0.06740031811588454, 245.60734761563322, 0.0, 0.0, 219.39701393139984, 0.0, 0.0, 189.78383607560235, 0.0, 0.0, 168.78061588134693, 0.0, 0.0, 152.7432730987756, 0.0, 0.0, 148.01679303828902, 0.0, 0.0, 149.8351779236836, 0.0, 0.0, 153.36078623498597, 0.0, 0.0, 157.67680411837213, 0.0, 0.0, 161.61509080242772, 0.0, 0.0, 165.37399478858055, 0.0, 0.0, 168.71299111926604, 0.0, 0.0, 173.1949207846824, 0.0, 0.0, 177.29530762542274, 0.0, 0.0, 181.17964813297664, 0.0, 0.0, 182.95285937172534, 0.0, 0.0, 184.601571739844, 0.0, 0.0, 184.8588271260586, 0.0, 0.0, 185.00168957610296, 0.0, 0.0, 182.73895067816966, 0.0, 0.0, 178.1802433606046, 0.0, 0.0, 174.22528924198204, 0.0, 0.0, 172.43432593609563, 0.0, 0.0, 170.20584160747896, 0.0, 0.0, 166.204194767317, 0.0, 0.04353570636101578, 157.93725500674162, 0.0, 0.9347298669257126, 155.34738345580908, 0.0, 0.5016669537186824, 153.63435011902, 0.0, 0.0, 150.43903328417392, 0.0, 0.0, 145.8225294532964, 0.0, 0.0, 142.59428969709916, 0.0, 0.0, 139.17854958667508, 0.0, 0.0, 135.04223872866098, 0.0, 0.0, 132.6710244791081, 0.0, 0.0, 129.8130893949175, 0.0, 0.0, 127.30523034351714, 0.0, 0.0, 125.57126532598356, 0.0, 0.0, 123.11634204635025, 0.0, 0.0]),
    ("raw.seed9", &[147.64296408713994, 0.0, 0.0, 122.23291309870683, 0.0, 0.0, 112.91542251257829, 0.0, 0.0, 80.79322221777858, 0.0, 0.0, 48.09329549256864, 0.0, 0.0, 16.717090624269872, 0.0, 0.0, 0.8015066284596393, 0.0, 0.0, -8.4431886013233, 0.0, 0.0, -17.29663171689691, 0.0, 0.0, -17.869391593574775, 0.0, 0.0, 7.1697313370930615, 0.0, 0.0, 34.322027635001845, 0.0, 0.0, 50.57702049523511, 0.0, 0.7023305844332511, 47.8665872578827, 0.0, 0.0, 13.699477639117429, 0.0, 0.0, 29.891083272776612, 0.0, 0.0, 98.30845385083902, 0.0, 0.0, 164.795461087137, 0.0, 0.0, 459.2762525894969, 0.0, 0.30958233497403675, 230.69496659270445, 0.0, 0.0, 331.181483768859, 0.0, 0.0, 149.84408639975422, 0.0, 0.7730699961159914, 147.42189906778816, 0.0, 0.0, 206.29461678153874, 0.0, 0.0, 427.58553803998745, 0.0, 0.0, 684.0113724800584, 0.0, 0.0, 1219.5356987449388, 0.0, 0.0, 1205.1099256497503, 0.0, 0.0, 1740.4244228566965, 0.0, 0.0, 994.3587869494878, 0.0, 0.0]),
    ("airport:-3:-3", &[0.0]),
    ("airport:-3:-2", &[0.0]),
    ("airport:-3:-1", &[0.0]),
    ("airport:-3:0", &[1.0, -24548.60172671433, 3735.792387539321, 1.053454351889563, 285.15989265147294, 781861883.0, 1000.0, 24.0, 6.0]),
    ("airport:-3:1", &[1.0, -26442.547712966876, 15683.809352780745, 0.807359410062575, 230.48801201608137, 129774762.0, 1000.0, 24.0, 5.0]),
    ("airport:-3:2", &[1.0, -26806.808238093443, 22493.814699246675, 2.6410248171617443, 195.52685362595759, 962463795.0, 1000.0, 24.0, 15.0]),
    ("airport:-3:3", &[0.0]),
    ("airport:-2:-3", &[1.0, -16731.551123641595, -27995.81587894246, 0.7138448527814956, 241.56244248644236, 72663878.0, 1000.0, 24.0, 4.0]),
    ("airport:-2:-2", &[0.0]),
    ("airport:-2:-1", &[1.0, -15622.815005622055, -3047.4283323716354, 2.311764942053744, 304.2282232447542, 428994143.0, 1000.0, 24.0, 13.0]),
    ("airport:-2:0", &[1.0, -17008.337148988845, 8350.705933085723, 0.8236422506674506, 258.0063883583914, 673274770.0, 1000.0, 24.0, 5.0]),
    ("airport:-2:1", &[1.0, -15287.167162603866, 15810.522386435521, 0.3131898134296997, 236.1507287041859, 194730425.0, 1000.0, 24.0, 2.0]),
    ("airport:-2:2", &[1.0, -17921.42355055888, 23300.970560423866, 2.866156254316531, 161.6787953996331, 750728631.0, 1000.0, 24.0, 16.0]),
    ("airport:-2:3", &[0.0]),
    ("airport:-1:-3", &[0.0]),
    ("airport:-1:-2", &[1.0, -2184.4431873608382, -17309.618250026117, 0.8500607583095909, 302.2870160832448, 346522521.0, 1000.0, 24.0, 5.0]),
    ("airport:-1:-1", &[0.0]),
    ("airport:-1:0", &[0.0]),
    ("airport:-1:1", &[1.0, -6659.179819168006, 11970.998355616231, 0.20140012418269695, 283.35209428961485, 775580498.0, 1000.0, 24.0, 1.0]),
    ("airport:-1:2", &[1.0, -8253.497054153026, 26508.381716827786, 2.3565824075486552, 66.25747843982828, 890051950.0, 1000.0, 24.0, 14.0]),
    ("airport:-1:3", &[0.0]),
    ("airport:0:-3", &[1.0, 7965.814928811345, -23417.273070639163, 0.6946185855413098, 214.84297323699766, 904280029.0, 1000.0, 24.0, 4.0]),
    ("airport:0:-2", &[1.0, 2710.0755852934694, -16870.474767536376, 0.5429852279680153, 272.58539885619035, 944038739.0, 1000.0, 24.0, 3.0]),
    ("airport:0:-1", &[0.0]),
    ("airport:0:0", &[0.0]),
    ("airport:0:1", &[1.0, 2390.981190101887, 12347.631406559203, 2.8227434751375955, 256.81319499541354, 133153429.0, 1000.0, 24.0, 16.0]),
    ("airport:0:2", &[0.0]),
    ("airport:0:3", &[0.0]),
    ("airport:1:-3", &[1.0, 16077.755667629654, -26858.48317420558, 0.11680727500988031, 188.192735450026, 669161361.0, 1000.0, 24.0, 1.0]),
    ("airport:1:-2", &[1.0, 18434.912860316756, -17479.413295992286, 0.3076114389538784, 190.27446561260683, 378248296.0, 1000.0, 24.0, 2.0]),
    ("airport:1:-1", &[0.0]),
    ("airport:1:0", &[0.0]),
    ("airport:1:1", &[1.0, 17880.31420544199, 13947.862670817001, 0.10009734772808576, 161.75829488376056, 915600048.0, 1000.0, 24.0, 1.0]),
    ("airport:1:2", &[1.0, 17465.987459690536, 27520.63074192551, 0.9207511906138753, 19.443179653461847, 441124126.0, 1000.0, 24.0, 5.0]),
    ("airport:1:3", &[0.0]),
    ("airport:2:-3", &[1.0, 28367.619912659575, -22951.998565985043, 1.781375188239389, 119.35182173698627, 468524818.0, 1000.0, 24.0, 10.0]),
    ("airport:2:-2", &[1.0, 28374.948224081803, -17567.27976514191, 1.0991832853482275, 123.92736466900946, 750728631.0, 1000.0, 24.0, 6.0]),
    ("airport:2:-1", &[1.0, 26980.821735019745, -6722.698432586541, 2.277985550244403, 188.4144526380528, 922964652.0, 1000.0, 24.0, 13.0]),
    ("airport:2:0", &[1.0, 22439.889634580213, 4283.904550531471, 2.301907358184388, 150.01085738534013, 281412181.0, 1000.0, 24.0, 13.0]),
    ("airport:2:1", &[1.0, 25303.9721940596, 11929.215134287035, 1.2116081223039088, 111.58594693321828, 359466207.0, 1000.0, 24.0, 7.0]),
    ("airport:2:2", &[0.0]),
    ("airport:2:3", &[0.0]),
    ("airport:3:-3", &[1.0, 35222.08157659097, -22469.94155102474, 0.7333815651205833, 117.99702449383753, 424267297.0, 1000.0, 24.0, 4.0]),
    ("airport:3:-2", &[1.0, 32730.34605681388, -16932.76059495825, 2.1984222559277997, 129.147552037516, 334075081.0, 1000.0, 24.0, 13.0]),
    ("airport:3:-1", &[1.0, 33557.45228703313, -4316.190647219254, 0.2837606344642761, 190.02631410731644, 129774762.0, 1000.0, 24.0, 2.0]),
    ("airport:3:0", &[1.0, 32333.057000395453, 3039.2913855558754, 1.7920737800531774, 159.78084022442, 641194831.0, 1000.0, 24.0, 10.0]),
    ("airport:3:1", &[1.0, 33561.41029636609, 14546.012869711276, 2.239067094319749, 112.70692183463844, 895686803.0, 1000.0, 24.0, 13.0]),
    ("airport:3:2", &[0.0]),
    ("airport:3:3", &[0.0]),
    ("town:-4:-4", &[0.0]),
    ("town:-4:-3", &[0.0]),
    ("town:-4:-2", &[1.0, -24485.097135442702, -11238.991717182109, 221.23219908650898, 0.877849765808618]),
    ("town:-4:-1", &[0.0]),
    ("town:-4:0", &[1.0, -23057.58065476865, 4324.885806531849, 401.4647245539336, 0.20482009213095573]),
    ("town:-4:1", &[1.0, -24068.66923197907, 11450.770091773697, 261.8665560380673, 0.6489914188767604]),
    ("town:-4:2", &[0.0]),
    ("town:-4:3", &[0.0]),
    ("town:-4:4", &[1.0, -26557.645006427858, 30480.233872259945, 413.51973550783975, 1.183675759659362]),
    ("town:-3:-4", &[1.0, -19590.76026400204, -23833.232883294273, 301.8043859326728, 1.2246045739501346]),
    ("town:-3:-3", &[0.0]),
    ("town:-3:-2", &[0.0]),
    ("town:-3:-1", &[1.0, -17785.61705854148, -2401.0420003565446, 330.39625066504954, 0.4139300757640145]),
    ("town:-3:0", &[1.0, -18108.78153698692, 2942.6635725431165, 229.18230674255332, 1.3669607145438099]),
    ("town:-3:1", &[1.0, -16238.53573814349, 12497.746966350442, 290.5252200350161, 0.8384778929750215]),
    ("town:-3:2", &[1.0, -16638.20583983176, 18002.633309602097, 249.42093394938237, 0.9024985976875662]),
    ("town:-3:3", &[1.0, -16110.377089229298, 22705.612671931798, 292.69886246750013, 0.8900647642260157]),
    ("town:-3:4", &[0.0]),
    ("town:-2:-4", &[0.0]),
    ("town:-2:-3", &[0.0]),
    ("town:-2:-2", &[0.0]),
    ("town:-2:-1", &[0.0]),
    ("town:-2:0", &[1.0, -11533.946154718853, 2853.7384924987405, 339.6235688877678, 1.2649898679522458]),
    ("town:-2:1", &[0.0]),
    ("town:-2:2", &[0.0]),
    ("town:-2:3", &[1.0, -10657.352312866076, 25355.84416050288, 335.95064241725066, 1.1720572521673658]),
    ("town:-2:4", &[0.0]),
    ("town:-1:-4", &[0.0]),
    ("town:-1:-3", &[0.0]),
    ("town:-1:-2", &[0.0]),
    ("town:-1:-1", &[1.0, -3612.9512819784863, -1556.9631191419387, 230.9479802740864, 1.5525522531393339]),
    ("town:-1:0", &[1.0, -4493.507030974599, 4339.62621920093, 264.55929971808587, 0.19888247446727517]),
    ("town:-1:1", &[1.0, -2841.655542201999, 12498.021767824639, 249.4880288729533, 0.12351383594153954]),
    ("town:-1:2", &[1.0, -3422.8969601642566, 18241.849734924435, 263.8420428780051, 0.0012310181619909833]),
    ("town:-1:3", &[1.0, -3407.244442401121, 26272.258588885514, 493.4648162304729, 0.9045894779580803]),
    ("town:-1:4", &[0.0]),
    ("town:0:-4", &[0.0]),
    ("town:0:-3", &[0.0]),
    ("town:0:-2", &[0.0]),
    ("town:0:-1", &[0.0]),
    ("town:0:0", &[1.0, 1449.0945213715397, 4903.585335690754, 406.56981939642105, 1.2605249911879928]),
    ("town:0:1", &[1.0, 4733.315339572867, 10163.080685635727, 226.62220793789172, 1.5661903546726517]),
    ("town:0:2", &[0.0]),
    ("town:0:3", &[0.0]),
    ("town:0:4", &[0.0]),
    ("town:1:-4", &[1.0, 9271.260068617721, -25217.294724594674, 472.1184304869129, 0.8240693134707667]),
    ("town:1:-3", &[1.0, 10592.755557598879, -15727.741411114486, 493.4648162304729, 0.9045894779580803]),
    ("town:1:-2", &[0.0]),
    ("town:1:-1", &[1.0, 11158.344457798, -1501.9782321753594, 249.4880288729533, 0.12351383594153954]),
    ("town:1:0", &[1.0, 10383.717206434278, 5503.330346258277, 291.84967233491113, 0.25924197318001035]),
    ("town:1:1", &[1.0, 10387.048718021513, 12443.03688085806, 230.9479802740864, 1.5525522531393339]),
    ("town:1:2", &[1.0, 12362.50857143265, 18150.362915926064, 253.1615395003021, 0.7067866188317896]),
    ("town:1:3", &[1.0, 11408.617877918115, 25940.03385236702, 248.44051692386088, 1.5398136961837272]),
    ("town:1:4", &[0.0]),
    ("town:2:-4", &[1.0, 19550.848218286006, -24250.707835847163, 310.69872693627127, 1.4143808752416638]),
    ("town:2:-3", &[1.0, 16808.555337066726, -18294.137454689022, 436.05476992755905, 0.5030861250926999]),
    ("town:2:-2", &[0.0]),
    ("town:2:-1", &[0.0]),
    ("town:2:0", &[1.0, 17846.16576952907, 5408.498494165574, 308.32703010794467, 1.4279274888359907]),
    ("town:2:1", &[0.0]),
    ("town:2:2", &[1.0, 19108.98887986052, 19037.618303825642, 249.72077285890228, 0.5924358741527297]),
    ("town:2:3", &[1.0, 18072.41678622962, 25302.866643450536, 387.92025870336784, 0.3108045867859373]),
    ("town:2:4", &[0.0]),
    ("town:3:-4", &[1.0, 25166.841139130116, -22757.017601269843, 388.0330940723435, 1.3652468039110266]),
    ("town:3:-3", &[1.0, 25889.6229107707, -19294.387328068202, 292.69886246750013, 0.8900647642260157]),
    ("town:3:-2", &[1.0, 26505.77376052967, -10317.681266028105, 299.3823664958199, 0.8548949511441701]),
    ("town:3:-1", &[1.0, 25761.46426185651, -1502.2530336495574, 290.5252200350161, 0.8384778929750215]),
    ("town:3:0", &[1.0, 24061.55706128001, 4287.492146017766, 485.2361468843493, 1.1055878954885139]),
    ("town:3:1", &[0.0]),
    ("town:3:2", &[1.0, 24864.76299345738, 15992.831865350066, 232.58289256046552, 0.9976175710133112]),
    ("town:3:3", &[0.0]),
    ("town:3:4", &[0.0]),
    ("town:4:-4", &[1.0, 29442.354993572142, -25519.76612774006, 413.51973550783975, 1.183675759659362]),
    ("town:4:-3", &[0.0]),
    ("town:4:-2", &[1.0, 32356.4616133996, -9787.684244822107, 349.24512750647443, 0.45638390461140177]),
    ("town:4:-1", &[0.0]),
    ("town:4:0", &[1.0, 30204.012212961276, 5394.904863625619, 297.08018838322425, 0.4280846914330386]),
    ("town:4:1", &[1.0, 30059.70242119425, 11172.2180211696, 522.8626128177183, 0.18390080731886613]),
    ("town:4:2", &[1.0, 31747.197375944474, 19437.71829622012, 227.35728056745896, 0.7909718217082392]),
    ("town:4:3", &[1.0, 30145.967683160776, 23096.75412409899, 256.42058537720436, 1.108460764336736]),
    ("town:4:4", &[0.0]),
    ("roads", &[75.0, -37113.95081385674, -16751.55244969919, -30464.95798803439, -16594.94798709381, -37113.95081385674, -16751.55244969919, -37948.29146409548, -11527.177410455542, -37948.29146409548, -11527.177410455542, -32006.294789940417, -12412.843042951536, -37948.29146409548, -11527.177410455542, -40175.66649985886, -4015.6976155298526, -40175.66649985886, -4015.6976155298526, -31045.52622027346, -4580.002547867673, -37771.250775093315, 17903.26453489902, -31602.1554845293, 19043.545367996, -37771.250775093315, 17903.26453489902, -36480.536066764864, 23821.647243813197, -36480.536066764864, 23821.647243813197, -39548.18853315012, 32090.7856517243, -39548.18853315012, 32090.7856517243, -29864.346853396022, 30678.71486519461, -39548.18853315012, 32090.7856517243, -36853.53622211031, 39737.46412116037, -30464.95798803439, -16594.94798709381, -32006.294789940417, -12412.843042951536, -32006.294789940417, -12412.843042951536, -24485.097135442702, -11238.991717182109, -32006.294789940417, -12412.843042951536, -31045.52622027346, -4580.002547867673, -29864.346853396022, 30678.71486519461, -26557.645006427858, 30480.233872259945, -24229.70265319127, -39750.124695733146, -16348.897183938225, -37879.39391754789, -23057.58065476865, 4324.885806531849, -18108.78153698692, 2942.6635725431165, -23057.58065476865, 4324.885806531849, -24068.66923197907, 11450.770091773697, -24068.66923197907, 11450.770091773697, -16238.53573814349, 12497.746966350442, -16348.897183938225, -37879.39391754789, -17089.53218135195, -32593.760944636964, -17089.53218135195, -32593.760944636964, -19590.76026400204, -23833.232883294273, -17785.61705854148, -2401.0420003565446, -18108.78153698692, 2942.6635725431165, -18108.78153698692, 2942.6635725431165, -11533.946154718853, 2853.7384924987405, -18108.78153698692, 2942.6635725431165, -16238.53573814349, 12497.746966350442, -16238.53573814349, 12497.746966350442, -16638.20583983176, 18002.633309602097, -16638.20583983176, 18002.633309602097, -16110.377089229298, 22705.612671931798, -16110.377089229298, 22705.612671931798, -10657.352312866076, 25355.84416050288, -11533.946154718853, 2853.7384924987405, -4493.507030974599, 4339.62621920093, -10657.352312866076, 25355.84416050288, -3407.244442401121, 26272.258588885514, -3612.9512819784863, -1556.9631191419387, -4493.507030974599, 4339.62621920093, -4493.507030974599, 4339.62621920093, 1449.0945213715397, 4903.585335690754, -4493.507030974599, 4339.62621920093, -2841.655542201999, 12498.021767824639, -2841.655542201999, 12498.021767824639, 4733.315339572867, 10163.080685635727, -2841.655542201999, 12498.021767824639, -3422.8969601642566, 18241.849734924435, -3422.8969601642566, 18241.849734924435, -3407.244442401121, 26272.258588885514, 5471.608330926874, -31306.129441763307, 9298.466684113033, -30400.099239170748, 1449.0945213715397, 4903.585335690754, 10383.717206434278, 5503.330346258277, 1449.0945213715397, 4903.585335690754, 4733.315339572867, 10163.080685635727, 4733.315339572867, 10163.080685635727, 10387.048718021513, 12443.03688085806, 9298.466684113033, -30400.099239170748, 19159.543943476736, -31466.306771988806, 9298.466684113033, -30400.099239170748, 9271.260068617721, -25217.294724594674, 9271.260068617721, -25217.294724594674, 19550.848218286006, -24250.707835847163, 9271.260068617721, -25217.294724594674, 10592.755557598879, -15727.741411114486, 10592.755557598879, -15727.741411114486, 16808.555337066726, -18294.137454689022, 11158.344457798, -1501.9782321753594, 10383.717206434278, 5503.330346258277, 10383.717206434278, 5503.330346258277, 17846.16576952907, 5408.498494165574, 10383.717206434278, 5503.330346258277, 10387.048718021513, 12443.03688085806, 10387.048718021513, 12443.03688085806, 12362.50857143265, 18150.362915926064, 12362.50857143265, 18150.362915926064, 19108.98887986052, 19037.618303825642, 12362.50857143265, 18150.362915926064, 11408.617877918115, 25940.03385236702, 11408.617877918115, 25940.03385236702, 18072.41678622962, 25302.866643450536, 16681.914226935, -36498.432673381474, 26009.69243161273, -38352.00322157069, 16681.914226935, -36498.432673381474, 19159.543943476736, -31466.306771988806, 19159.543943476736, -31466.306771988806, 19550.848218286006, -24250.707835847163, 19550.848218286006, -24250.707835847163, 25166.841139130116, -22757.017601269843, 19550.848218286006, -24250.707835847163, 16808.555337066726, -18294.137454689022, 16808.555337066726, -18294.137454689022, 25889.6229107707, -19294.387328068202, 17846.16576952907, 5408.498494165574, 24061.55706128001, 4287.492146017766, 19108.98887986052, 19037.618303825642, 24864.76299345738, 15992.831865350066, 19108.98887986052, 19037.618303825642, 18072.41678622962, 25302.866643450536, 25166.841139130116, -22757.017601269843, 29442.354993572142, -25519.76612774006, 25166.841139130116, -22757.017601269843, 25889.6229107707, -19294.387328068202, 25889.6229107707, -19294.387328068202, 26505.77376052967, -10317.681266028105, 26505.77376052967, -10317.681266028105, 32356.4616133996, -9787.684244822107, 26505.77376052967, -10317.681266028105, 25761.46426185651, -1502.2530336495574, 25761.46426185651, -1502.2530336495574, 24061.55706128001, 4287.492146017766, 24061.55706128001, 4287.492146017766, 30204.012212961276, 5394.904863625619, 24864.76299345738, 15992.831865350066, 31747.197375944474, 19437.71829622012, 31058.527891561804, -31079.561704774835, 36708.48196824407, -31453.28405085968, 31058.527891561804, -31079.561704774835, 29442.354993572142, -25519.76612774006, 30204.012212961276, 5394.904863625619, 37802.94468478886, 3576.672543361197, 30204.012212961276, 5394.904863625619, 30059.70242119425, 11172.2180211696, 30059.70242119425, 11172.2180211696, 38954.47377972654, 9419.997452132327, 30059.70242119425, 11172.2180211696, 31747.197375944474, 19437.71829622012, 31747.197375944474, 19437.71829622012, 30145.967683160776, 23096.75412409899, 30145.967683160776, 23096.75412409899, 39535.042011965605, 25405.05201290619]),
    ("towns.rect", &[19.0, -17785.61705854148, -2401.0420003565446, -18108.78153698692, 2942.6635725431165, -16238.53573814349, 12497.746966350442, -16638.20583983176, 18002.633309602097, -11533.946154718853, 2853.7384924987405, -3612.9512819784863, -1556.9631191419387, -4493.507030974599, 4339.62621920093, -2841.655542201999, 12498.021767824639, -3422.8969601642566, 18241.849734924435, 1449.0945213715397, 4903.585335690754, 4733.315339572867, 10163.080685635727, 10592.755557598879, -15727.741411114486, 11158.344457798, -1501.9782321753594, 10383.717206434278, 5503.330346258277, 10387.048718021513, 12443.03688085806, 12362.50857143265, 18150.362915926064, 16808.555337066726, -18294.137454689022, 17846.16576952907, 5408.498494165574, 19108.98887986052, 19037.618303825642]),
    ("airports.rect", &[9.0, -15622.815005622055, -3047.4283323716354, -17008.337148988845, 8350.705933085723, -15287.167162603866, 15810.522386435521, -2184.4431873608382, -17309.618250026117, -6659.179819168006, 11970.998355616231, 2710.0755852934694, -16870.474767536376, 2390.981190101887, 12347.631406559203, 18434.912860316756, -17479.413295992286, 17880.31420544199, 13947.862670817001]),
    ("nearest", &[2390.981190101887, 12347.631406559203, 127344.94077313329, -102596.97766766213]),
    ("ground", &[256.81319499541354, 0.0, 1.0, 1.0, 256.8199561156919, 0.0, 0.9953280000000002, 0.9953280000000002, 258.02310822580984, 0.0, 0.6859354074074082, 0.6859354074074082, 264.67974112947473, 0.0, 0.014451182441701182, 0.014451182441701182, 256.81319499541354, 0.0, 1.0, 1.0, 256.8316656079632, 0.0, 0.9953280000000003, 0.9953280000000003, 258.88176390662966, 0.0, 0.6859354074074084, 0.6859354074074084, 266.34065093017887, 0.0, 0.014451182441701405, 0.014451182441701405, 257.0461056941123, 0.0, 0.9164224965706447, 0.9164224965706447, 257.1909610092133, 0.0, 0.912486540466392, 0.912486540466392, 259.32202510336833, 0.0, 0.6284547507365048, 0.6284547507365048, 266.1759634998146, 0.0, 0.008510444021025299, 0.008510444021025299, 258.0674629372248, 0.0, 0.6479999999999999, 0.6479999999999999, 258.545129274042, 0.0, 0.6451247920298508, 0.6451247920298508, 260.8643764278382, 0.0, 0.4294783160538307, 0.4294783160538307, 266.3722449093726, 0.0, 0.0, 0.0, 259.65686539759247, 0.0, 0.320318244170096, 0.320318244170096, 260.4221248799386, 0.0, 0.31850799442897093, 0.31850799442897093, 262.6183600796814, 0.0, 0.18452866350211583, 0.18452866350211583, 266.5851231909218, 0.0, 0.0, 0.0, 260.97383813802304, 0.0, 0.06506447187928688, 0.06506447187928688, 261.92493246785494, 0.0, 0.06432007804727558, 0.06432007804727558, 263.9031419757923, 0.0, 0.01675053609497479, 0.01675053609497479, 266.7836906025444, 0.0, 0.0, 0.0, 261.7038305306813, 0.0, 0.0, 0.0, 262.9278533685877, 0.0, 0.0, 0.0, 264.2041289627396, 0.0, 0.0, 0.0, 266.85330649599905, 0.0, 0.0, 0.0]),
    ("surface", &[0.08550436049699783, 0.10354108363389969, 0.07009781152009964, 0.0, 0.0, 0.6000000238418579, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.08218380063772202, 0.1458004117012024, 0.04396245256066322, 0.04696590080857277, 0.0, 0.04940694570541382, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.032849330455064774, 0.07058686017990112, 0.02576632611453533, 0.04979097843170166, 0.0, 0.9378970861434937, 0.08641541004180908, 0.10436537116765976, 0.07083533704280853, 0.0, 0.0, 0.6000000238418579, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.03208683058619499, 0.07109439373016357, 0.02550567500293255, 0.06655807048082352, 0.0, 0.928301215171814, 0.0777207612991333, 0.09676451236009598, 0.06345801800489426, 0.009860857389867306, 0.0, 0.5901391506195068, 0.17000000178813934, 0.17000000178813934, 0.07500000298023224, 0.0, 0.0, 0.0, 0.17000000178813934, 0.17000000178813934, 0.07500000298023224, 0.0, 0.0, 0.0, 0.17000000178813934, 0.17000000178813934, 0.07500000298023224, 0.0, 0.0, 0.0, 0.13940313458442688, 0.16280074417591095, 0.06420110911130905, 0.5826320052146912, 0.0, 0.0, 0.08477270603179932, 0.1028791144490242, 0.06950552761554718, 0.0, 0.0, 0.6000000238418579, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.14305420219898224, 0.16365981101989746, 0.06548971682786942, 0.8562189936637878, 0.0, 0.0, 0.15803678333759308, 0.16718512773513794, 0.07077768445014954, 0.16867242753505707, 0.0, 0.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.14996206760406494, 0.15896274149417877, 0.0886605903506279, 0.019978728145360947, 0.0, 0.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.13726937770843506, 0.16229867935180664, 0.06344801932573318, 0.5601598620414734, 0.0, 0.0, 0.12586259841918945, 0.15961472690105438, 0.0594220906496048, 0.5332027673721313, 0.0, 0.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.08201922476291656, 0.10038787126541138, 0.06727651506662369, 0.0, 0.0, 0.6000000238418579, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.08706875145435333, 0.1049564853310585, 0.07136422395706177, 0.0, 0.0, 0.6000000238418579, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.12449008971452713, 0.15929178893566132, 0.0589376799762249, 0.05539358779788017, 0.0, 0.0, 0.18757787346839905, 0.1809246987104416, 0.11087869852781296, 0.0, 0.0, 0.0, 0.12579436600208282, 0.1595986783504486, 0.05939801037311554, 1.0, 0.0, 0.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.02800000086426735, 0.06499999761581421, 0.024000000208616257, 0.0, 0.0, 1.0, 0.056174129247665405, 0.10701405256986618, 0.03437994047999382, 0.2004111111164093, 0.0, 0.5057170391082764]),
    ("wind", &[9.737348178056857, 3.7378177621736564, 0.0, 10.4182044264181, 8.741911491736934, 0.0, 5.233650317656801, 1.9059274501352454, 0.0, 9.414932639288173, 5.556469821789153, -0.8364123192035913, -0.08225150930709746]),
    ("strips", &[-1.125, 0.19808873949823552, 0.017453292519943295, -2.375, 0.13017605230306728, -0.00010535186631756635, -3.625, 0.10805461664936049, -0.017663996252578428, -4.8525, 0.06368059154933672, -0.0349065850398866, 1.125, 0.19808873949823552, 0.017453292519943295, 2.375, 0.13017605230306728, -0.00010535186631756635, 3.625, 0.10805461664936049, -0.017663996252578428, 4.8525, 0.06368059154933672, -0.0349065850398866]),
    ("sim.start", &[2827.795717825758, 12203.433388648513, -257.93319499541354, 0.0, 0.0, 0.0, 0.0, 0.0, 0.9873187933488654, 0.15875011905551586, 0.0, 0.0, 0.0, 70.0, 0.0, 257.93319499541354, 0.0, 0.0, 0.0, 1.1200000000000045, 0.0, 0.0, 0.0, 0.0, 0.0, -999.0, 2390.981190101887, 12347.631406559203]),
];
