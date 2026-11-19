//! A quad against things that hold (`Multirotor::hit`): a gate's tube, a trunk, a crown. Its
//! propellers striking them break, or chip on a light graze, as against another quad; its frame
//! bounces off with its restitution; a hard hit crashes it; and it never sticks (the old contact
//! put it back where it was a step before, and slow it could stay glued to a gate).
//! `cargo test -p sim-core --test quad_strikes -- --nocapture` prints the measurements.

use std::sync::Arc;

use sim_core::scenery::{Obstacle, ObstacleKind, Shape};
use sim_core::{MultirotorKind, Start, State, VehicleId, World, V3};

const QUAD: MultirotorKind = MultirotorKind::RACE5;

/// A quad hovering over the runway nearest the origin, facing north, moving at `velocity`, and
/// `things` placed round it (`at` is where they are given from: the quad's middle).
fn quad(velocity: V3, things: impl Fn(V3) -> Vec<Obstacle>) -> (World, VehicleId) {
    let mut world = World::new(1);
    let a = world.terrain.nearest_airport(0.0, 0.0).expect("an aerodrome");
    let id = world.add_multirotor(QUAD, Start::Air { n: a.n, e: a.e, heading: 0.0 });
    let b = world.vehicle(id).body();
    world.place(id, State { pos: b.pos, q: b.q, vel: velocity, ..State::default() });
    world.obstacles = Arc::new(things(b.pos));
    (world, id)
}

/// An upright tube of a gate (radius 6 cm) from 3 m under `at` to 3 m over it, `ahead` metres
/// north and `aside` east of it.
fn post(at: V3, ahead: f64, aside: f64) -> Vec<Obstacle> {
    vec![Obstacle { kind: ObstacleKind::Gate, n: at.x + ahead, e: at.y + aside, base: -at.z - 3.0, shape: Shape::Bar { dn: 0.0, de: 0.0, up: 6.0, radius: 0.06 } }]
}

fn blades(world: &World, id: VehicleId) -> Vec<[f64; 2]> {
    world.vehicle(id).multirotor().unwrap().blades().to_vec()
}

/// Brushing past a post at 1 m/s, the tip of one propeller 3 mm into it: that one is chipped, not
/// broken, and the quad flies on.
#[test]
fn a_light_graze_chips_a_tip() {
    // The front right disc reaches 0.0778 + 0.0648 m to the right of the middle; the post's side
    // 3 mm into it.
    let aside = 0.0778 + 0.06475 + 0.06 - 0.003;
    let (mut world, id) = quad(V3::new(1.0, 0.0, 0.0), |at| post(at, 1.0, aside));
    for _ in 0..(2.0 / World::DT) as usize {
        world.step();
    }
    let b = blades(&world, id);
    println!("after a graze: {b:?}");
    let struck: Vec<&[f64; 2]> = b.iter().filter(|x| **x != [1.0, 1.0]).collect();
    assert!(!struck.is_empty(), "nothing touched");
    assert!(struck.iter().all(|x| x[0].max(x[1]) > 0.8), "chipped only: {b:?}");
    assert!(world.vehicle(id).dynamics.crashed().is_none());
}

/// Into a post at 5 m/s: the propellers that meet it shatter, the quad tumbles down and is down
/// hard; it never stops in the air.
#[test]
fn propellers_into_a_post_break() {
    let (mut world, id) = quad(V3::new(5.0, 0.0, 0.0), |at| post(at, 1.0, 0.0778));
    let mut stuck = 0;
    for _ in 0..(6.0 / World::DT) as usize {
        world.step();
        let v = world.vehicle(id);
        if v.dynamics.crashed().is_none() && v.body().vel.length() < 0.05 {
            stuck += 1;
        }
    }
    let b = blades(&world, id);
    let crashed = world.vehicle(id).dynamics.crashed();
    println!("into a post at 5 m/s: {b:?}, {crashed:?}");
    assert!(b.iter().any(|x| x[0].max(x[1]) <= 0.25), "a propeller broken: {b:?}");
    // Down from 15 m on two rotors it lands hard (a crash into the ground) or on what is left of them.
    assert!(matches!(crashed, Some("Broke a propeller on a gate" | "Crashed into the ground")), "{crashed:?}");
    assert!(stuck < 10, "held still in the air {stuck} steps");
}

/// The frame of a quad falling with its motors stopped onto a level tube at 3 m/s bounces back up at
/// its restitution's share of that (0.3: some 0.9 m/s), and goes on falling past it, not held.
#[test]
fn the_frame_bounces_off_a_tube() {
    let (mut world, id) = quad(V3::new(0.0, 0.0, 3.0), |at| {
        vec![Obstacle { kind: ObstacleKind::Gate, n: at.x - 2.0, e: at.y, base: -at.z - 0.12, shape: Shape::Bar { dn: 4.0, de: 0.0, up: 0.0, radius: 0.06 } }]
    });
    world.vehicle_mut(id).multirotor_mut().unwrap().controls.arm = false;
    let mut most_up: f64 = 0.0;
    for _ in 0..(0.3 / World::DT) as usize {
        world.step();
        most_up = most_up.max(-world.vehicle(id).body().world_velocity().z);
    }
    println!("off a tube at 3 m/s: up to {most_up:.2} m/s back up, {:?}", world.vehicle(id).dynamics.crashed());
    assert!((0.6..1.3).contains(&most_up), "bounced at {most_up}");
    assert!(world.vehicle(id).dynamics.crashed().is_none());
}

/// Into a trunk at 15 m/s: crashed into a tree, its speed gone back off it.
#[test]
fn a_hard_hit_on_a_trunk_crashes_it() {
    let (mut world, id) = quad(V3::new(15.0, 0.0, 0.0), |at| {
        vec![Obstacle { kind: ObstacleKind::Tree, n: at.x + 2.0, e: at.y, base: -at.z - 3.0, shape: Shape::Tree { trunk: 0.3, crown: 9.0, radius: 4.0, top: 14.0, cone: false } }]
    });
    for _ in 0..(0.5 / World::DT) as usize {
        world.step();
    }
    let v = world.vehicle(id);
    println!("into a trunk at 15 m/s: {:?}, then {:.1} m/s north", v.dynamics.crashed(), v.body().world_velocity().x);
    assert_eq!(v.dynamics.crashed(), Some("Crashed into a tree"));
    assert!(v.body().world_velocity().x < 1.0);
}

/// Through a crown at 10 m/s the propellers meet its twigs and break: it comes down.
#[test]
fn flying_into_a_crown_breaks_propellers() {
    let (mut world, id) = quad(V3::new(10.0, 0.0, 0.0), |at| {
        vec![Obstacle { kind: ObstacleKind::Tree, n: at.x + 4.0, e: at.y + 2.0, base: -at.z - 3.0, shape: Shape::Tree { trunk: 0.3, crown: 2.0, radius: 4.0, top: 14.0, cone: false } }]
    });
    for _ in 0..(3.0 / World::DT) as usize {
        world.step();
    }
    let b = blades(&world, id);
    println!("through a crown at 10 m/s: {b:?}, {:?}", world.vehicle(id).dynamics.crashed());
    assert!(b.iter().any(|x| x[0].max(x[1]) <= 0.25), "{b:?}");
}
