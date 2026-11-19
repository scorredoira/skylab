//! The files of the types of aircraft (`aircraft::file`): the built-in fleet, and what a broken
//! file is told. A whole new type from a file, flying, is `tests/fleet.rs`.

use sim_core::aircraft::file::{fleet_with, load};
use sim_core::*;

const TRAINER: &str = include_str!("fixtures/trainer.ron");

#[test]
fn the_built_in_fleet_is_the_c172_and_the_extra() {
    let keys: Vec<&str> = AircraftKind::all().map(AircraftKind::key).collect();
    assert_eq!(keys, ["c172", "extra300"]);
    assert_eq!(AircraftKind::C172.name(), "Cessna 172");
    assert_eq!(AircraftKind::EXTRA300.name(), "Extra 300L");
    for (word, kind) in [("c172", AircraftKind::C172), ("Cessna", AircraftKind::C172), ("172", AircraftKind::C172), ("extra", AircraftKind::EXTRA300), ("Extra 300L", AircraftKind::EXTRA300), ("ea-300", AircraftKind::EXTRA300)] {
        assert_eq!(AircraftKind::parse(word), Some(kind), "{word}");
    }
    assert_eq!(AircraftKind::parse("piper"), None);
    assert_eq!(AircraftType::c172().visual, Visual { model: "c172", cockpit: "c172" });
    assert_eq!(AircraftType::extra300().afcs, None);
}

/// A file's mistake is told with the file, and the line and column or the field.
#[test]
fn a_broken_file_says_what_is_wrong() {
    let broken = |from: &str, to: &str| {
        assert!(TRAINER.contains(from), "{from}");
        let text = TRAINER.replacen(from, to, 1);
        load(&text, "trainer.ron").expect_err("a broken file").to_string()
    };
    let cases = [
        // Not RON: the line and column.
        ("cl0: 0.375,", "cl0: 0.375", "trainer.ron: 39:9: Expected comma"),
        // A misspelt or missing field.
        ("cl0: 0.375,", "cl_0: 0.375,", "Unexpected field named `cl_0` in `WingFile`"),
        ("        cd0: 0.009,\n", "", "`cd0`"),
        // Numbers the flight model cannot take.
        ("strip_edges: [0.5, 1.75, 3.0, 4.25]", "strip_edges: [0.5, 3.0, 1.75, 4.25]", "wing.strip_edges: must increase"),
        ("strip_edges: [0.5, 1.75, 3.0, 4.25]", "strip_edges: [0.5, 1.75, 3.0]", "wing.strip_edges: four"),
        ("lift_curve: [0.0, 0.6, 1.0]", "lift_curve: [0.0, 1.0]", "flaps: lift_curve and drag_curve need a value for each notch"),
        ("final_flap_notch: 1", "final_flap_notch: 3", "start.final_flap_notch: not a notch"),
        ("mass: 1080.0,", "mass: -1080.0,", "mass.mass: must be positive"),
        ("load_limit_negative: -1.52,", "load_limit_negative: 1.52,", "structure: load limits"),
        ("key: \"trainer\"", "key: \"Trainer 2\"", "key: lowercase letters"),
    ];
    for (from, to, said) in cases {
        let message = broken(from, to);
        println!("{message}");
        assert!(message.starts_with("trainer.ron: ") && message.contains(said), "{message} (wanted {said})");
    }
    let mut six = TRAINER.to_string();
    let wheel = "            (contact: (1.37, 0.0, 1.20), spring: 22000.0, damping: 2000.0, steers: true, brake: None, slip: 0.07, strength: 11000.0, broken: \"Landing too hard: nose gear broken\"),\n";
    six = six.replacen(wheel, &wheel.repeat(4), 1);
    assert!(load(&six, "trainer.ron").unwrap_err().message.contains("gear.wheels: from 1 to 5"));
}

/// The fleet a game would make from the files on disk: the built-in types, each replaced by a file
/// of its key, and the others after them.
#[test]
fn files_replace_and_add_to_the_built_in_types() {
    let heavier = include_str!("../../../../assets/aircraft/c172/aircraft.ron").replacen("mass: 1043.0,", "mass: 1100.0,", 1);
    let files = [("trainer.ron".to_string(), TRAINER.to_string()), ("c172.ron".to_string(), heavier)];
    let fleet = fleet_with(&files).unwrap();
    let keys: Vec<&str> = fleet.iter().map(|t| t.kind.key()).collect();
    assert_eq!(keys, ["c172", "extra300", "trainer"]);
    assert_eq!(fleet[0].mass, 1100.0);
    let bad = [("trainer.ron".to_string(), TRAINER.replacen("cd0", "cd_0", 1))];
    assert_eq!(fleet_with(&bad).unwrap_err().file, "trainer.ron");
}
