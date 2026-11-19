//! A third type of aircraft that is only a file (`tests/fixtures/trainer.ron`): made part of the
//! fleet, named on the command line, placed, trimmed and flown by the autopilot round a circuit to a
//! landing, with no Rust code of its own. Its own process: the fleet is set once.

use sim_core::aircraft::file::{fleet_with, set_fleet};
use sim_core::*;

#[test]
fn a_type_from_a_file_flies_with_no_code_of_its_own() {
    let files = [("trainer.ron".to_string(), include_str!("fixtures/trainer.ron").to_string())];
    set_fleet(fleet_with(&files).unwrap()).unwrap();
    assert!(set_fleet(Vec::new()).is_err(), "the fleet is set once");
    let kind = AircraftKind::parse("t180").expect("the trainer by its alias");
    let p = kind.spec();
    assert_eq!((kind.key(), kind.name(), p.max_flap_notch(), p.visual.model), ("trainer", "Trainer T-180", 2, "c172"));

    // Its own numbers: more power than the C172's climbs steeper; its flaps stall it slower.
    let (trainer, c172) = (Aircraft::with_type(p.clone()), Aircraft::new());
    let climb = |a: &Aircraft| a.trim(75.0 * KNOTS, 300.0, 0.0, Some(1.0), 0.0).expect("a climb at full power").climb_angle / DEG;
    println!("full power at 75 kt: the trainer climbs at {:.1}°, the C172 at {:.1}°", climb(&trainer), climb(&c172));
    assert!(climb(&trainer) > climb(&c172) + 0.5);
    let (clean, full) = (trainer.stall(0.0, 300.0).speed / KNOTS, trainer.stall(35.0, 300.0).speed / KNOTS);
    println!("trainer: stall {clean:.0} kt clean, {full:.0} kt with flaps 35");
    assert!(full < clean - 3.0 && (45.0..62.0).contains(&clean));
    assert!(trainer.trim_level(110.0 * KNOTS, 900.0).is_some(), "trims level at its air start speed");

    // In the world, by its kind: placed on the runway, and flown round the circuit to a landing by
    // the autopilot at its own speeds.
    let mut w = World::new(1);
    let id = w.add_aircraft(kind, Start::Home);
    let airport = w.vehicle(id).home.unwrap();
    let mut ap = Autopilot::for_type(Goal::Circuit { airport, reversed: false, repeat: false }, p);
    for _ in 0..240 * 900 {
        w.fly(id, &mut ap);
        w.step();
        if ap.finished() || w.vehicle(id).aircraft().unwrap().crashed().is_some() {
            break;
        }
    }
    println!("{:?} after {:.0} s, {:?}", ap.phase(), w.time(), w.vehicle(id).aircraft().unwrap().crashed());
    assert_eq!((ap.phase(), ap.landings()), (Phase::Stopped, 1));
    assert!(w.vehicle(id).aircraft().unwrap().crashed().is_none());
}
