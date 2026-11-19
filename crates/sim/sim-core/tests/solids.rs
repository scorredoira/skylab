//! The buildings and bridges of a real place as the vehicles meet them: each building its own
//! outline (not the box round it), courtyards open, a bridge's deck and piers with room under it;
//! an aircraft, the FPV quad and the pieces of a wreck all run into them (`sim_core::solids`).

use std::sync::Arc;

use sim_core::*;

/// Flat built ground at 10 m: an L-shaped block 40 m tall (arms 12 m wide along north and east
/// from its corner at 1000, 0), a block of 60 m with a courtyard of 30 m at 1000, 300, a low wide
/// block at 1500, 0, and a bridge 20 m over the ground running east at north 2000.
fn place() -> Arc<Terrain> {
    let (n0, e0) = (1000.0f32, 0.0f32);
    let l = Footprint::new(vec![[n0, e0], [n0 + 40.0, e0], [n0 + 40.0, e0 + 12.0], [n0 + 12.0, e0 + 12.0], [n0 + 12.0, e0 + 40.0], [n0, e0 + 40.0]], 40.0, 0.0, Roof::Flat);
    let mut block = Footprint::new(vec![[1000.0, 300.0], [1060.0, 300.0], [1060.0, 360.0], [1000.0, 360.0]], 25.0, 0.0, Roof::Flat);
    block.holes = vec![vec![[1015.0, 315.0], [1045.0, 315.0], [1045.0, 345.0], [1015.0, 345.0]]];
    let low = Footprint::new(vec![[1500.0, -60.0], [1620.0, -60.0], [1620.0, 60.0], [1500.0, 60.0]], 12.0, 0.0, Roof::Flat);
    let bridge = Bridge { line: vec![[2000.0, -150.0, 30.0], [2000.0, 150.0, 30.0]], width: 12.0, ..Bridge::default() };
    let region = Region {
        name: "town".into(),
        latitude: 0.7,
        longitude: 0.0,
        heights: vec![Grid::centred(0.0, 0.0, 8000.0, 20.0, 10.0f32)],
        cover: Grid::centred(0.0, 0.0, 8000.0, 20.0, Cover::Built as u8),
        airports: Vec::new(),
        roads: Vec::new(),
        towns: Vec::new(),
        buildings: vec![l, block, low],
        bridges: vec![bridge],
    };
    Arc::new(Terrain::from_region(1, Arc::new(region)))
}

/// A C172 flying north at its cruise, put at a point `up` metres over the ground.
fn aircraft_at(world: &mut World, n: f64, e: f64, up: f64) -> VehicleId {
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n, e, heading: 0.0 });
    world.vehicle_mut(id).aircraft_mut().unwrap().state.pos = V3::new(n, e, -(10.0 + up));
    id
}

fn run(world: &mut World, seconds: f64) {
    for _ in 0..(seconds / World::DT).round() as usize {
        world.step();
    }
}

#[test]
fn an_aircraft_flies_through_the_empty_corner_of_an_l_and_strikes_its_arm() {
    let terrain = place();
    // The box round the L holds its open corner; the L does not.
    assert!(terrain.solid_at(1030.0, 30.0, 30.0).is_none());
    assert!(terrain.solid_at(1030.0, 6.0, 30.0).is_some());
    let mut world = World::with_terrain(1, terrain.clone());
    let through = aircraft_at(&mut world, 1022.0, 27.0, 20.0);
    let into = aircraft_at(&mut world, 1022.0, 5.0, 20.0);
    run(&mut world, 1.0);
    assert_eq!(world.vehicle(through).dynamics.crashed(), None, "through the corner");
    assert_eq!(world.vehicle(into).dynamics.crashed(), Some("Crashed into a building"));
    // Over its roof, and under the bridge, it flies on.
    let mut world = World::with_terrain(1, terrain);
    let over = aircraft_at(&mut world, 990.0, 6.0, 45.0);
    // Between the piers (every 33 m from the bridge's west end, one under the middle).
    let under = aircraft_at(&mut world, 1985.0, 33.0, 10.0);
    let deck = aircraft_at(&mut world, 1990.0, 40.0, 20.0);
    run(&mut world, 0.6);
    assert_eq!(world.vehicle(over).dynamics.crashed(), None);
    assert_eq!(world.vehicle(under).dynamics.crashed(), None, "under the deck between its piers");
    assert_eq!(world.vehicle(deck).dynamics.crashed(), Some("Crashed into a bridge"));
}

#[test]
fn the_quad_hovers_in_a_courtyard_and_crashes_into_its_walls() {
    let terrain = place();
    let mut world = World::with_terrain(1, terrain);
    // Flying west at 15 m/s, 12 m up: across the courtyard, or from its middle into its wall.
    let spot = |world: &mut World, n: f64, e: f64| {
        let id = world.add_multirotor(MultirotorKind::FPV5, Start::Air { n, e, heading: -std::f64::consts::FRAC_PI_2 });
        let body = &mut world.vehicle_mut(id).multirotor_mut().unwrap().body;
        body.pos = V3::new(n, e, -(10.0 + 12.0));
        body.vel = V3::new(15.0, 0.0, 0.0);
        id
    };
    let yard = spot(&mut world, 1030.0, 343.0);
    let wall = spot(&mut world, 1030.0, 322.0);
    run(&mut world, 0.8);
    assert_eq!(world.vehicle(yard).dynamics.crashed(), None);
    assert_eq!(world.vehicle(wall).dynamics.crashed(), Some("Crashed into a building"));
}

/// Slowly into a wall of a real place's building the quad bounces off and its propellers break on
/// it, as against a trunk: never held against the wall, never inside it.
#[test]
fn the_quad_bounces_off_a_wall_slowly_and_is_never_held() {
    let terrain = place();
    let mut world = World::with_terrain(1, terrain.clone());
    let id = world.add_multirotor(MultirotorKind::FPV5, Start::Air { n: 1030.0, e: 322.0, heading: -std::f64::consts::FRAC_PI_2 });
    {
        let body = &mut world.vehicle_mut(id).multirotor_mut().unwrap().body;
        body.pos = V3::new(1030.0, 322.0, -(10.0 + 12.0));
        body.vel = V3::new(3.0, 0.0, 0.0);
    }
    let mut stuck = 0;
    // 7 m from the courtyard's west wall (east 315), at 3 m/s towards it.
    for _ in 0..(5.0 / World::DT) as usize {
        world.step();
        let b = world.vehicle(id).body();
        assert!(terrain.solid_at(b.pos.x, b.pos.y, -b.pos.z).is_none(), "inside the wall at {:?}", b.pos);
        if b.vel.length() < 0.05 && -b.pos.z > 11.0 {
            stuck += 1;
        }
    }
    let quad = world.vehicle(id).multirotor().unwrap();
    assert!(quad.blades().iter().any(|b| b[0] < 1.0 || b[1] < 1.0), "a propeller cut on the wall: {:?}", quad.blades());
    assert!(stuck < 6, "held still against the wall for {stuck} steps");
    assert_ne!(world.vehicle(id).dynamics.crashed(), Some("Crashed into a building"), "too slow to crash on the wall itself");
}

#[test]
fn a_wreck_comes_to_rest_on_the_roof_it_fell_on() {
    let terrain = place();
    let mut world = World::with_terrain(1, terrain.clone());
    // Into the low block's roof, 12 m up: it breaks up there.
    let id = aircraft_at(&mut world, 1520.0, 0.0, 12.3);
    run(&mut world, 12.0);
    assert!(world.vehicle(id).dynamics.gone(), "{:?}", world.vehicle(id).dynamics.crashed());
    let wreck = world.wrecks.first().expect("a wreck");
    let main = wreck.main();
    // Its main piece lies on the roof (22 m above the sea), not on the ground under it (10 m).
    assert!(terrain.solid_top(main.pos.x, main.pos.y).is_some(), "{:?}", main.pos);
    assert!((-main.pos.z - 22.0).abs() < 3.0, "{:?}", main.pos);
}
