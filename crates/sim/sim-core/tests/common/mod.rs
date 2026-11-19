//! Shared by the test files: a world with a single aircraft, which is what most tests fly.
#![allow(dead_code)]

use sim_core::{Aircraft, AircraftKind, KeyboardPilot, Start, Vehicle, VehicleId, World};

/// A world with one aircraft, placed like a new game: on the runway nearest to the origin.
pub struct Sim {
    pub world: World,
    pub id: VehicleId,
}

impl Sim {
    pub fn new(seed: u64) -> Sim {
        let mut world = World::new(seed);
        let id = world.add_aircraft(AircraftKind::C172, Start::Home);
        Sim { world, id }
    }

    pub fn vehicle(&self) -> &Vehicle {
        self.world.vehicle(self.id)
    }

    pub fn aircraft(&self) -> &Aircraft {
        self.vehicle().aircraft().unwrap()
    }

    pub fn aircraft_mut(&mut self) -> &mut Aircraft {
        self.world.vehicle_mut(self.id).aircraft_mut().unwrap()
    }

    pub fn reset_in_air(&mut self, n: f64, e: f64, heading: f64) {
        self.world.reset(self.id, Start::Air { n, e, heading });
    }

    pub fn step(&mut self) {
        self.world.step();
    }

    /// One step with the keyboard pilot moving yoke and pedals from the keys it has set.
    pub fn fly(&mut self, pilot: &mut KeyboardPilot) {
        self.world.fly(self.id, pilot);
        self.world.step();
    }
}

/// `XCTAssertEqual(a, b, accuracy:)`.
#[track_caller]
pub fn assert_close(a: f64, b: f64, accuracy: f64, what: &str) {
    assert!(
        (a - b).abs() <= accuracy,
        "{what}: {a} is not within {accuracy} of {b}"
    );
}
