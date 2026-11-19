//! The Boeing 747-100 (`wip/b747/aircraft.ron`, not in the game yet) against its published figures, and
//! what a jet transport does that the light aircraft do not: four engines that spool slowly and
//! one of them failing, Mach, a gear that retracts, spoilers and reverse thrust on the runway.
//! `examples/b747.rs` prints the same figures, to tune the file by.

mod common;

use common::assert_close;
use sim_core::*;

const DT: f64 = 1.0 / 240.0;
const FPM: f64 = 60.0 / FEET;

/// The fleet: the built-in types and the 747, from `wip`.
fn with_the_747() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let file = ("wip/b747/aircraft.ron".to_string(), include_str!("../../../../wip/b747/aircraft.ron").to_string());
        aircraft::file::set_fleet(aircraft::file::fleet_with(&[file]).unwrap()).unwrap();
    });
}

fn jumbo() -> Aircraft {
    with_the_747();
    Aircraft::of(AircraftKind::parse("747").expect("the fleet has the Boeing 747"))
}

/// True airspeed of an indicated one at an altitude.
fn tas(kt: f64, altitude: f64) -> f64 {
    kt * KNOTS / (atmosphere::density(altitude) / atmosphere::SEA_LEVEL_DENSITY).sqrt()
}

/// In trimmed level flight at `speed` m/s true, with `flaps` and the gear up or down.
fn flying(a: &mut Aircraft, speed: f64, altitude: f64, flap_notch: i32, gear_down: bool) -> Environment {
    a.gear = if gear_down { 1.0 } else { 0.0 };
    let flaps = a.p.flap_notches[flap_notch as usize];
    let t = a.trim(speed, altitude, flaps, None, 0.0).expect("it flies there");
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(speed * t.alpha.cos(), 0.0, speed * t.alpha.sin()),
        q: quaternion(0.0, t.alpha, 0.0),
        n1: t.n1,
        ..State::default()
    };
    a.flap_deg = flaps;
    a.controls = AircraftControls { throttle: t.throttle, trim: t.trim, flap_notch, gear_up: !gear_down, ..AircraftControls::default() };
    Environment::standard(altitude)
}

fn runway() -> Environment {
    Environment { ground_height: 0.0, kind: GroundKind::Runway, ..Environment::default() }
}

/// Standing on the runway, engines idling, settled on its legs.
fn standing(a: &mut Aircraft) -> Environment {
    let env = runway();
    let idle = a.p.jets.as_ref().unwrap().idle;
    a.state = State { pos: V3::new(0.0, 0.0, -a.p.cg_height), n1: [idle; MOST_ENGINES], ..State::default() };
    a.controls.parking_brake = true;
    for _ in 0..240 * 5 {
        a.step(DT, &env);
    }
    a.controls.parking_brake = false;
    env
}

fn pitch(a: &Aircraft) -> f64 {
    euler_angles(a.state.q).pitch
}

#[test]
fn it_is_a_747() {
    let a = jumbo();
    assert_close(a.mass(), 564_000.0 * 0.45359, 5.0, "mass");
    assert_eq!(a.p.jets.as_ref().unwrap().mounts.len(), 4);
    assert_eq!(a.p.gear.len(), 5);
    // Stalls at this weight: about 170 kt clean and 108 kt with flaps 30 (a reference speed of 141 kt).
    let clean = a.stall(0.0, 0.0).speed / KNOTS;
    let full = a.stall(30.0, 0.0).speed / KNOTS;
    assert!((158.0..176.0).contains(&clean), "clean stall {clean} kt");
    assert!((104.0..112.0).contains(&full), "stall with flaps 30 {full} kt");
}

#[test]
fn it_stands_level_on_its_five_legs() {
    let mut a = jumbo();
    standing(&mut a);
    let d = *a.data();
    assert!(a.crashed().is_none() && d.hard_contact.is_none(), "{:?}", a.crashed());
    assert!(pitch(&a).abs() < 1.0 * DEG, "pitch {}", pitch(&a) / DEG);
    // The nose leg carries some 7 % of the weight; the four main legs share the rest.
    let weight = a.mass() * GRAVITY;
    let total: f64 = d.wheel_load.iter().sum();
    assert_close(total, weight, 0.02 * weight, "weight on the wheels");
    assert!((0.04..0.11).contains(&(d.wheel_load[0] / weight)), "nose leg {}", d.wheel_load[0] / weight);
    for leg in 1..5 {
        assert!((0.18..0.29).contains(&(d.wheel_load[leg] / weight)), "leg {leg}: {}", d.wheel_load[leg] / weight);
    }
    // Idle thrust moves it without brakes, slowly.
    let env = runway();
    for _ in 0..240 * 20 {
        a.step(DT, &env);
    }
    assert!((0.2..8.0).contains(&a.state.vel.x), "taxi speed on idle thrust {}", a.state.vel.x);
}

#[test]
fn cruise_at_mach_0_84() {
    let mut a = jumbo();
    let altitude = 35_000.0 * FEET;
    let speed = 0.84 * atmosphere::Atmosphere::STANDARD.speed_of_sound(altitude);
    let env = flying(&mut a, speed, altitude, 0, false);
    for _ in 0..240 * 120 {
        a.step(DT, &env);
    }
    let d = *a.data();
    // It stays there, hands off.
    assert_close(-a.state.pos.z, altitude, 60.0, "altitude after two minutes");
    assert_close(d.mach, 0.84, 0.01, "Mach");
    // Some 10 tonnes of fuel an hour at this weight, at a lift over drag of 16 or 17.
    let fuel = d.fuel_flow * 3.6;
    assert!((8.5..11.5).contains(&fuel), "fuel {fuel} t/h");
    let lift_over_drag = a.mass() * GRAVITY / d.thrust;
    assert!((15.0..18.5).contains(&lift_over_drag), "L/D {lift_over_drag}");
    let n1 = a.state.n1[0];
    assert!((0.88..1.0).contains(&n1), "N1 {n1}");
    assert!(!d.overspeed);
    // Beyond Mach 0.92 the warning sounds; drag has grown steeply by then.
    let fast = a.trim(0.93 * 296.5, altitude, 0.0, None, 0.0);
    assert!(fast.is_none(), "level at Mach 0.93 should be beyond its thrust");
}

#[test]
fn it_climbs_as_a_light_747_does() {
    let mut a = jumbo();
    a.gear = 0.0;
    // Clean at 250 kt low down: some 3,500 ft/min at this weight; still climbing at 33,000 ft.
    let low = a.trim(tas(250.0, 1500.0), 1500.0, 0.0, Some(1.0), 0.0).unwrap();
    let rate = tas(250.0, 1500.0) * low.climb_angle.sin() * FPM;
    assert!((2800.0..4500.0).contains(&rate), "{rate} ft/min");
    let high = a.trim(0.84 * 299.5, 10_000.0, 0.0, Some(1.0), 0.0).unwrap();
    let rate = 0.84 * 299.5 * high.climb_angle.sin() * FPM;
    assert!((600.0..2000.0).contains(&rate), "{rate} ft/min at 10,000 m");
}

#[test]
fn takeoff() {
    let mut a = jumbo();
    let env = standing(&mut a);
    a.controls.flap_notch = 3;
    a.flap_deg = 10.0;
    a.controls.trim = 0.2;
    a.controls.throttle = 1.0;
    let (mut t, mut rotated, mut lift_off, mut spooled) = (0.0, None, None, None);
    while t < 90.0 && -a.state.pos.z - a.p.cg_height < 11.0 {
        let kt = a.data().ias / KNOTS;
        if spooled.is_none() && a.state.n1[0] > 0.95 {
            spooled = Some(t);
        }
        if kt > 145.0 {
            rotated.get_or_insert(a.state.pos.x);
            // Rotate at 2.5° a second towards 12°, then hold it.
            let error = 12.0 * DEG - pitch(&a);
            a.controls.elevator = clamp(4.0 * error - 4.0 * a.state.omega.y, -0.2, 0.7);
        }
        if lift_off.is_none() && !a.data().on_ground() && kt > 100.0 {
            lift_off = Some((a.state.pos.x, kt));
        }
        a.step(DT, &env);
        t += DT;
    }
    assert!(a.crashed().is_none(), "{:?}", a.crashed());
    let spooled = spooled.expect("the engines reach take-off thrust");
    assert!((5.0..11.0).contains(&spooled), "spool-up {spooled} s");
    let (roll, speed) = lift_off.expect("it lifts off");
    // At 564,000 lb with flaps 10: a ground roll of some 1,300 m, off at about 160 kt.
    assert!((1000.0..1700.0).contains(&roll), "ground roll {roll} m");
    assert!((148.0..172.0).contains(&speed), "lift-off at {speed} kt");
    assert!(rotated.unwrap() < roll);
    assert!(t < 60.0, "35 ft after {t} s");
    // No tail strike on the way.
    assert!(a.damage().events == 0, "{}", a.damage().last);

    // Gear up: it takes its time, and the drag goes with it.
    a.controls.gear_up = true;
    let air = Environment::standard(100.0);
    for _ in 0..240 * 6 {
        a.step(DT, &air);
    }
    assert!(a.gear > 0.3 && a.gear < 0.7, "gear in transit: {}", a.gear);
    for _ in 0..240 * 8 {
        a.step(DT, &air);
    }
    assert_eq!(a.gear, 0.0);
}

#[test]
fn an_outer_engine_fails_and_the_rudder_holds_it() {
    let mut a = jumbo();
    let env = flying(&mut a, tas(180.0, 600.0), 600.0, 3, false);
    a.controls.throttle = 1.0;
    for _ in 0..240 * 8 {
        a.step(DT, &env);
    }
    a.fail_engine(3);
    // Hands and feet off: it yaws and rolls towards the dead engine.
    let mut free = a.clone();
    for _ in 0..240 * 6 {
        free.step(DT, &env);
    }
    let e = euler_angles(free.state.q);
    let turned = remainder(e.heading, std::f64::consts::TAU);
    assert!(free.state.n1[3] < 0.5 && free.data().jet_thrust[3] < 0.0, "{:?}", free.data().jet_thrust);
    assert!(e.roll > 8.0 * DEG, "roll {}", e.roll / DEG);
    assert!(turned > 3.0 * DEG, "it should yaw right: {}", turned / DEG);

    // With rudder against it and the wings held level it flies straight and still climbs.
    let (mut heading_error, mut worst_roll, mut held) = (0.0_f64, 0.0_f64, 0.0);
    let start = -a.state.pos.z;
    for i in 0..240 * 40 {
        let mut e = euler_angles(a.state.q);
        e.heading = remainder(e.heading, std::f64::consts::TAU);
        held += e.heading * DT;
        a.controls.rudder = clamp(-3.0 * e.heading - 6.0 * a.state.omega.z - 1.5 * held, -1.0, 1.0);
        a.controls.aileron = clamp(-3.0 * e.roll - 2.0 * a.state.omega.x, -1.0, 1.0);
        let slow = a.data().ias / KNOTS - 175.0;
        a.controls.elevator = clamp(0.02 * slow - 1.5 * a.state.omega.y, -0.5, 0.5);
        a.step(DT, &env);
        if i > 240 * 15 {
            heading_error = heading_error.max(e.heading.abs());
            worst_roll = worst_roll.max(e.roll.abs());
        }
    }
    assert!(a.crashed().is_none());
    assert!(heading_error < 5.0 * DEG && worst_roll < 5.0 * DEG, "heading {} roll {}", heading_error / DEG, worst_roll / DEG);
    assert!(a.controls.rudder < -0.1 && a.controls.rudder > -0.9, "rudder held {}", a.controls.rudder);
    assert!(-a.state.pos.z > start + 100.0, "it climbs on three engines: {} m", -a.state.pos.z - start);
}

/// Rolling on the runway at `kt` after touching down: on its legs, flaps 30, engines at idle.
fn landed(a: &mut Aircraft, kt: f64) -> Environment {
    let env = standing(a);
    a.flap_deg = 30.0;
    a.controls.flap_notch = 6;
    a.state.vel = V3::new(kt * KNOTS, 0.0, 0.0);
    a.state.pos = V3::new(0.0, 0.0, a.state.pos.z);
    env
}

/// How far it rolls to 20 kt from 135 kt, with what the pilot does at each moment.
fn stop(mut act: impl FnMut(&mut Aircraft)) -> f64 {
    let mut a = jumbo();
    let env = landed(&mut a, 135.0);
    let mut t = 0.0;
    while a.state.vel.x > 20.0 * KNOTS && t < 200.0 {
        act(&mut a);
        a.step(DT, &env);
        t += DT;
    }
    assert!(a.crashed().is_none(), "{:?}", a.crashed());
    a.state.pos.x
}

#[test]
fn spoilers_brakes_and_reverse_stop_it() {
    let brakes = |a: &mut Aircraft| {
        a.controls.brake_left = 1.0;
        a.controls.brake_right = 1.0;
    };
    let coasting = stop(|_| {});
    let braked = stop(brakes);
    let with_spoilers = stop(|a| {
        brakes(a);
        a.controls.spoilers = 1.0;
    });
    let everything = stop(|a| {
        brakes(a);
        a.controls.spoilers = 1.0;
        a.controls.reverse = true;
        a.controls.throttle = 1.0;
    });
    let reverse_only = stop(|a| {
        a.controls.reverse = true;
        a.controls.throttle = 1.0;
    });
    println!("coasting {coasting:.0} m, brakes {braked:.0} m, with spoilers {with_spoilers:.0} m, with reverse too {everything:.0} m, reverse alone {reverse_only:.0} m");
    // The spoilers put the weight on the wheels, where the brakes can use it.
    assert!(with_spoilers < 0.9 * braked, "{with_spoilers} {braked}");
    assert!(everything < 0.95 * with_spoilers, "{everything} {with_spoilers}");
    assert!(reverse_only < 0.6 * coasting, "{reverse_only} {coasting}");
    // At its maximum landing weight, maximum braking on a dry runway: some 1,000 m of roll.
    assert!((600.0..1200.0).contains(&everything), "{everything} m");
}

#[test]
fn the_reversers_open_only_on_the_ground() {
    let mut a = jumbo();
    let env = flying(&mut a, tas(160.0, 300.0), 300.0, 6, true);
    a.controls.reverse = true;
    a.controls.spoilers = 1.0;
    for _ in 0..240 * 4 {
        a.step(DT, &env);
    }
    assert_eq!(a.reversers, [0.0; MOST_ENGINES]);
    // And the speed brakes stop at the flight detent.
    assert_close(a.spoilers, a.p.spoilers.unwrap().flight_limit, 1e-9, "spoilers in flight");
    assert!(a.data().jet_thrust.iter().all(|&t| t > -30_000.0));
}

#[test]
fn speed_brakes_bring_it_down() {
    let mut clean = jumbo();
    let env = flying(&mut clean, tas(280.0, 5000.0), 5000.0, 0, false);
    let mut braked = clean.clone();
    braked.controls.spoilers = 1.0;
    for _ in 0..240 * 30 {
        clean.step(DT, &env);
        braked.step(DT, &env);
    }
    let lost = clean.state.pos.z - braked.state.pos.z;
    let slower = (clean.data().ias - braked.data().ias) / KNOTS;
    assert!(lost < -50.0 || slower > 8.0, "after 30 s: {lost} m, {slower} kt");
}

#[test]
fn a_gear_up_landing_slides_on_the_nacelles() {
    let mut a = jumbo();
    let env = runway();
    a.gear = 0.0;
    a.controls.gear_up = true;
    a.flap_deg = 30.0;
    a.controls.flap_notch = 6;
    let idle = a.p.jets.as_ref().unwrap().flight_idle;
    a.state = State {
        pos: V3::new(0.0, 0.0, -6.0),
        vel: V3::new(140.0 * KNOTS, 0.0, 1.0),
        q: quaternion(0.0, 3.0 * DEG, 0.0),
        n1: [idle; MOST_ENGINES],
        ..State::default()
    };
    for _ in 0..240 * 40 {
        a.step(DT, &env);
    }
    assert_eq!(a.crashed(), Some("Landed with the gear up"));
    assert!(!a.damage().broken_up, "a gentle belly landing leaves it in one piece");
    assert!(a.damage().engines_failed.iter().any(|&failed| failed), "the nacelles scraped the runway");
    assert!(a.state.vel.length() < 1.0, "stopped: {}", a.state.vel.length());
}

#[test]
fn it_is_stable_hands_off() {
    // Disturbed in pitch and in yaw on the approach, it comes back.
    let mut a = jumbo();
    let env = flying(&mut a, tas(150.0, 600.0), 600.0, 6, true);
    a.state.omega = V3::new(0.0, 3.0 * DEG, 3.0 * DEG);
    let mut worst = 0.0_f64;
    for i in 0..240 * 60 {
        a.step(DT, &env);
        if i > 240 * 40 {
            worst = worst.max(a.state.omega.x.abs());
        }
    }
    let e = euler_angles(a.state.q);
    assert!(a.crashed().is_none());
    // What is left is a slow, steady turn: no rolling from side to side.
    assert!(worst < 0.2 * DEG, "the Dutch roll has died out: {} °/s", worst / DEG);
    assert!(e.roll.abs() < 25.0 * DEG, "roll {}", e.roll / DEG);
    assert_close(a.data().ias / KNOTS, 150.0, 12.0, "speed");
}

/// The world starts it at an aerodrome with a runway of its size, and the autopilot flies it round
/// the circuit there (its flaps out and in by their speeds, its gear up and down) to a landing on
/// the centre line, stopped with its spoilers, reverse thrust and brakes well before the end.
#[test]
fn the_autopilot_flies_it_round_the_circuit_of_an_airport_of_its_size() {
    with_the_747();
    let kind = AircraftKind::parse("747").unwrap();
    let mut w = World::new(1);
    let small = w.terrain.nearest_airport(0.0, 0.0).unwrap();
    assert_eq!((small.length, small.width), (1000.0, 24.0), "the aerodrome at the origin stays a small one");
    let id = w.add_aircraft(kind, Start::Home);
    let airport = w.vehicle(id).home.unwrap();
    assert_eq!((airport.length, airport.width), Terrain::LONG_RUNWAY);
    assert_eq!(w.terrain.nearest_runway(0.0, 0.0, 0.0), Some(small));
    let reversed = (euler_angles(w.vehicle(id).aircraft().unwrap().state.q).heading - airport.heading).cos() < 0.0;
    let mut ap = Autopilot::for_type(Goal::Circuit { airport, reversed, repeat: false }, kind.spec());
    let (mut highest, mut gear_was_up, mut most_flap_in_the_air, mut touchdown) = (0.0_f64, false, 0.0_f64, None);
    for _ in 0..240 * 900 {
        w.fly(id, &mut ap);
        w.step();
        let v = w.vehicle(id);
        let a = v.aircraft().unwrap();
        highest = highest.max(v.agl());
        gear_was_up |= a.gear == 0.0;
        if !a.data().on_ground() {
            most_flap_in_the_air = most_flap_in_the_air.max(a.flap_deg);
        }
        if ap.phase() == Phase::Rollout && touchdown.is_none() {
            touchdown = Some((airport.local(a.state.pos.x, a.state.pos.y), a.last_touchdown().unwrap_or(0.0), a.data().ias / KNOTS));
        }
        assert!(!a.data().flap_overspeed && a.data().ias < 260.0 * KNOTS, "{:?} at {:.0} kt, flaps {:.0}", ap.phase(), a.data().ias / KNOTS, a.flap_deg);
        if ap.finished() || a.crashed().is_some() {
            break;
        }
    }
    let a = w.vehicle(id).aircraft().unwrap();
    println!("{:?} after {:.0} s, up to {highest:.0} m; touchdown {touchdown:?}", ap.phase(), w.time());
    assert_eq!((ap.phase(), ap.landings(), a.crashed(), a.damage().events), (Phase::Stopped, 1, None, 0), "{}", a.damage().last);
    assert!(gear_was_up && a.gear == 1.0 && most_flap_in_the_air == 30.0);
    assert!((600.0..1100.0).contains(&highest), "{highest}");
    let ((along, across), sink, speed) = touchdown.unwrap();
    // On the centre line, in the first third of the runway, gently, at its approach speed.
    assert!(across.abs() < 3.0 && along.abs() < airport.length / 2.0 && (airport.length / 2.0 - along.abs()) < 1200.0, "{along} {across}");
    assert!(sink < 2.0 && (125.0..150.0).contains(&speed), "{sink} m/s at {speed} kt");
    let (stopped, off) = airport.local(a.state.pos.x, a.state.pos.y);
    assert!(off.abs() < 5.0 && stopped.abs() < airport.length / 2.0 - 500.0, "stopped at {stopped} {off}");
}
