//! The other cars of a race: GT coupés on the grid, each driven by the racing driver with a
//! character of its own (`sim_core::car::driver::Character::rival`: pace, braking points, line,
//! mistakes, all from a seed: the same race every time), or by a trained network (`--rival net`,
//! `models/drive.fltmodel`). They see each other and the player (`Observation::traffic`): they
//! brake behind a slower car, pass on the straights with room left beside, and give room.
//!
//! A network's observation is its own car and the circuit (`sim-env`'s `drive` task): it does not
//! see the others. It drives, and the racing driver watches the traffic for it: when a car is in
//! its way the driver's pedals (which brake for it) take over until the way is clear.

use std::sync::Arc;

use sim_core::car::driver::{Character, Driver};
use sim_core::circuit::Circuit;
use sim_core::{CarControls, CarKind, Controls, Pilot, VehicleId, World};
use sim_env::ModelPilot;

/// The rivals' names, in the order they are given out.
const NAMES: [&str; 11] = ["Arden", "Bauer", "Castell", "Duval", "Ekström", "Ferreira", "Galli", "Hayes", "Ivanov", "Jansen", "Kato"];
/// Their paints (linear), in the same order.
const PAINTS: [[f32; 3]; 11] = [
    [0.008, 0.035, 0.26],
    [0.75, 0.75, 0.72],
    [0.66, 0.06, 0.004],
    [0.006, 0.08, 0.025],
    [0.75, 0.43, 0.004],
    [0.30, 0.31, 0.34],
    [0.004, 0.004, 0.005],
    [0.03, 0.14, 0.35],
    [0.25, 0.012, 0.07],
    [0.16, 0.30, 0.05],
    [0.45, 0.45, 0.48],
];

/// What drives a rival.
pub enum Brain {
    Driver(Box<Driver>),
    /// A network, with the racing driver watching the traffic for it.
    Network { net: Box<ModelPilot>, guard: Box<Driver> },
}

/// A rival car.
pub struct Rival {
    pub id: VehicleId,
    pub name: String,
    pub paint: [f32; 3],
    pub brain: Brain,
}

impl Rival {
    /// Forgets where it was (placed somewhere new).
    pub fn reset(&mut self) {
        match &mut self.brain {
            Brain::Driver(d) => d.reset(),
            Brain::Network { net, guard } => {
                net.reset();
                guard.reset();
            }
        }
    }

    /// Its controls for the next step.
    pub fn drive(&mut self, world: &mut World) {
        match &mut self.brain {
            Brain::Driver(d) => world.fly(self.id, d.as_mut()),
            Brain::Network { net, guard } => {
                let obs = world.observe(self.id);
                let current = match world.vehicle(self.id).dynamics.controls() {
                    Controls::Car(c) => c,
                    _ => return,
                };
                let mine = match net.fly(&obs, &Controls::Car(current), World::DT) {
                    Controls::Car(c) => c,
                    _ => current,
                };
                // The driver keeps its own idea of the race; its pedals win when it is held up.
                let watch = guard.drive(&obs, &current);
                let c = if guard.held_up() { CarControls { throttle: watch.throttle.min(mine.throttle), brake: watch.brake.max(mine.brake), ..mine } } else { mine };
                if let Some(car) = world.vehicle_mut(self.id).car_mut() {
                    car.controls = c;
                }
            }
        }
    }
}

/// How the rivals are driven: by the racing driver (each with its character), or some by a network.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Driver,
    /// A network's model file.
    Network(String),
}

/// `count` rivals on the grid's places `places` (one each), in a car of `car`; `kinds` says what
/// drives the first ones (the racing driver for the rest); `seed` their characters.
pub fn grid(world: &mut World, circuit: &Arc<Circuit>, car: CarKind, places: &[usize], kinds: &[Kind], seed: u64, abs: bool) -> Result<Vec<Rival>, String> {
    let n = places.len();
    let mut out = Vec::with_capacity(n);
    for (k, &place) in places.iter().enumerate() {
        let (pn, pe, heading) = circuit.grid(place);
        let id = world.add_car(car, sim_core::Start::Air { n: pn, e: pe, heading });
        if let Some(c) = world.vehicle_mut(id).car_mut() {
            c.controls.abs = abs;
            c.controls.traction_control = false;
        }
        // Skills spread over the field, not in grid order: the quick ones are not all at the front.
        let skill = ((k * 7 + seed as usize * 3) % n.max(1)) as f64 / (n.max(2) - 1) as f64;
        let character = Character::rival(skill, seed.wrapping_mul(101).wrapping_add(k as u64));
        let driver = Driver::with_character(circuit.clone(), car.spec(), character);
        let brain = match kinds.get(k) {
            Some(Kind::Network(file)) => {
                let model = sim_env::Model::load(file)?;
                let net = ModelPilot::driving(model, circuit.clone(), car.spec())?;
                // The guard drives as the network does, flat out: it only judges the traffic.
                let guard = Driver::new(circuit.clone(), car.spec());
                Brain::Network { net: Box::new(net), guard: Box::new(guard) }
            }
            _ => Brain::Driver(Box::new(driver)),
        };
        let name = if matches!(brain, Brain::Network { .. }) { "Neural".to_string() } else { NAMES[k % NAMES.len()].to_string() };
        out.push(Rival { id, name, paint: PAINTS[k % PAINTS.len()], brain });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::circuit::race::{Phase, Race};

    #[test]
    fn the_network_races_the_racing_drivers() {
        // `models/drive.fltmodel` on the pole, two racing drivers behind it, two laps from a
        // standing start: everyone finishes, nobody is wrecked.
        let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../models/drive.fltmodel");
        let (circuit, terrain) = crate::game::circuit_world("lakeside").unwrap();
        let mut world = World::with_terrain(1, terrain);
        let kinds = [Kind::Network(model.to_string_lossy().into_owned())];
        let mut rivals = grid(&mut world, &circuit, CarKind::GT, &[0, 1, 2], &kinds, 3, true).expect("the model loads");
        assert!(matches!(rivals[0].brain, Brain::Network { .. }));
        let names: Vec<(VehicleId, String)> = rivals.iter().map(|r| (r.id, r.name.clone())).collect();
        let mut race = Race::new(circuit.clone(), 2, &names, 0.0, 3);
        while race.phase() != Phase::Over && world.time() < 400.0 {
            for r in &mut rivals {
                if race.holding() {
                    if let Some(c) = world.vehicle_mut(r.id).car_mut() {
                        (c.controls.brake, c.controls.throttle) = (1.0, 0.0);
                    }
                } else {
                    r.drive(&mut world);
                }
            }
            world.step();
            race.update(&world);
        }
        let times: Vec<String> = race.order().iter().map(|&k| format!("{} {:?} best {:?}", race.entrants[k].name, race.entrants[k].finished, race.entrants[k].best())).collect();
        println!("{}", times.join(" | "));
        for e in &race.entrants {
            assert!(e.finished.is_some() && !e.retired, "{} did not finish", e.name);
        }
    }
}
