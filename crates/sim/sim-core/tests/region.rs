//! A terrain from a region (seam 3): the same queries as a seed's, answered from plain data, and a
//! world on it that is as deterministic as one on a seed.

use std::sync::Arc;

use sim_core::lights::{self, Class};
use sim_core::scenery;
use sim_core::*;

/// A small island: a hill in the middle, the sea round it, a lake, a wood, a town and an aerodrome
/// with two runways, one of them grass.
fn island() -> Region {
    let mut fine = Grid::centred(0.0, 0.0, 12_000.0, 20.0, 0.0f32);
    let mut cover = Grid::centred(0.0, 0.0, 12_000.0, 20.0, Cover::Unknown as u8);
    for r in 0..fine.rows {
        for c in 0..fine.cols {
            let (n, e) = fine.node(r, c);
            let d = n.hypot(e);
            // Land to 4.5 km out, rising to 300 m at the hill 2 km east of the aerodrome.
            let land = d < 4500.0;
            let hill = 300.0 * (-((n - 0.0).powi(2) + (e - 2500.0).powi(2)) / 1500.0f64.powi(2)).exp();
            let h = if land { 20.0 + hill } else { -15.0 - (d - 4500.0) * 0.02 };
            fine.set(r, c, h as f32);
            let what = if !land {
                Cover::Sea
            } else if (n + 2500.0).hypot(e + 1500.0) < 400.0 {
                Cover::Water
            } else if e > 1500.0 && n > 500.0 {
                Cover::Forest
            } else if (n - 2500.0).hypot(e + 2000.0) < 700.0 {
                Cover::Built
            } else {
                Cover::Farmland
            };
            cover.set(r, c, what as u8);
        }
    }
    let coarse = Grid::centred(0.0, 0.0, 60_000.0, 500.0, -50.0f32);
    let main = Airport { id: 77, n: 0.0, e: 0.0, heading: 0.3, elevation: 20.0, length: 1600.0, width: 30.0, detail: Detail { paved: true, lighting: Some(Class::PrecisionOne), apron: -1.0 } };
    let grass = Airport { id: 77, n: -300.0, e: -500.0, heading: 1.2, elevation: 20.0, length: 700.0, width: 25.0, detail: Detail { paved: false, lighting: Some(Class::Visual), apron: 0.0 } };
    Region {
        name: "Island".into(),
        latitude: 39.86 * DEG,
        longitude: 4.22 * DEG,
        heights: vec![fine, coarse],
        cover,
        airports: vec![main, grass],
        roads: vec![Road { n0: 2500.0, e0: -2000.0, n1: 800.0, e1: -200.0, width: 8.0 }, Road { n0: 800.0, e0: -200.0, n1: 700.0, e1: 3000.0, width: 6.0 }],
        towns: vec![Town { n: 2500.0, e: -2000.0, radius: 500.0, grid: 0.2 }, Town { n: 2700.0, e: -1500.0, radius: 300.0, grid: 0.7 }],
        // A block of flats of the maps in the first town, 30 m tall.
        buildings: vec![Footprint::new(vec![[2500.0, -1990.0], [2500.0, -1975.0], [2520.0, -1975.0], [2520.0, -1990.0]], 30.0, 0.0, Roof::Flat)],
        bridges: Vec::new(),
    }
}

fn terrain() -> Terrain {
    Terrain::from_region(1, Arc::new(island()))
}

#[test]
fn the_queries_answer_from_the_region() {
    let t = terrain();
    assert!(t.region().is_some() && Terrain::new(1).region().is_none());
    // The hill, the sea, the lake.
    assert!((t.height(0.0, 2500.0) - 320.0).abs() < 1.0, "{}", t.height(0.0, 2500.0));
    assert!(t.height(0.0, 5500.0) < 0.0);
    assert!(t.raw(-2500.0, -1500.0).river > 0.99);
    assert_eq!(t.raw(1000.0, 0.0).river, 0.0);
    // Beyond the fine grid the coarse one, beyond that its edge.
    assert_eq!(t.height(20_000.0, 0.0), -50.0);
    assert_eq!(t.height(200_000.0, 0.0), -50.0);
    // The aerodrome: both runways, the main one first, flat at its elevation.
    let home = t.nearest_airport(0.0, 0.0).unwrap();
    assert_eq!(home.length, 1600.0);
    assert_eq!(t.airports(-100.0, 100.0, -100.0, 100.0).len(), 2);
    assert!(t.airports(20_000.0, 21_000.0, 0.0, 1.0).is_empty());
    assert_eq!(t.ground(300.0, 90.0).h, 20.0);
    assert!(t.nearest_airport_within(40_000.0, 0.0, 1).is_none());
    // Roads and towns by rectangle.
    assert_eq!(t.roads(2400.0, 2600.0, -2100.0, -1900.0).len(), 1);
    assert_eq!(t.roads(-1000.0, 3000.0, -3000.0, 4000.0).len(), 2);
    assert_eq!(t.towns(2400.0, 2600.0, -2100.0, -1600.0).len(), 2);
    assert_eq!(t.towns(2400.0, 2600.0, -2600.0, -2400.0).len(), 1);
    assert!(t.towns(-3000.0, -2000.0, 2000.0, 3000.0).is_empty());
    // The cover decides the ground: a wood, farmland, a town; beyond the maps the seed's, as on its
    // land (the seed's beaches are below 25 m; a region's coast is where its maps put it).
    let at = |n: f64, e: f64| t.surface(n, e, &t.ground(n, e), 0.0, 0.0);
    assert!(at(2000.0, 3000.0).forest > 0.9);
    assert!(at(-1500.0, 2500.0).farm > 0.9 && at(-1500.0, 2500.0).forest == 0.0);
    assert_eq!(at(2500.0, -2000.0).forest, 0.0);
    let inland = Ground { h: 25.0, ..t.ground(9000.0, 0.0) };
    assert_eq!(at(9000.0, 0.0), Terrain::new(1).surface(9000.0, 0.0, &inland, 0.0, 0.0));
}

#[test]
fn real_aerodromes_have_the_lights_and_buildings_of_their_data() {
    let t = terrain();
    let all = t.airports(-100.0, 100.0, -100.0, 100.0);
    let (main, grass) = (all[0], all[1]);
    assert_eq!(Class::of(&main), Class::PrecisionOne);
    assert_eq!(Class::of(&grass), Class::Visual);
    // The apron, hangars, office, windsock and floodlights on the left of the main runway; none at the other.
    let (wn, we) = lights::windsock(&main).unwrap();
    assert!(main.local(wn, we).1 > 0.0, "the windsock is across from the apron");
    assert!(lights::windsock(&grass).is_none());
    let blocks = scenery::aerodrome_obstacles(&t, &main);
    assert!(!blocks.is_empty() && blocks.iter().all(|o| main.local(o.n, o.e).1 < -60.0));
    assert!(scenery::aerodrome_obstacles(&t, &grass).is_empty());
    let floods: Vec<_> = lights::aerodrome(&t, &main).into_iter().filter(|l| l.system == lights::System::Apron).collect();
    assert_eq!(floods.len(), 2);
    assert!(floods.iter().all(|l| main.local(l.at.x, l.at.y).1 < 0.0));
    assert!(!lights::aerodrome(&t, &grass).iter().any(|l| matches!(l.system, lights::System::Apron | lights::System::Taxiway)));
}

#[test]
fn scenery_follows_the_cover() {
    let t = terrain();
    let size = 512.0;
    let count = |list: &scenery::Instances| list.iter().map(|k| k.len() / 8).sum::<usize>();
    // A tile of the wood (east 2048..2560, north 1536..2048) is full of trees; one of the sea has none.
    let wood = scenery::trees(&t, (4, -4), size);
    assert!(count(&wood) > 600, "{}", count(&wood));
    assert_eq!(count(&scenery::trees(&t, (10, 0), size)), 0);
    // The town has houses, and no plot has two.
    let houses = scenery::houses(&t, (-4, -5), size);
    let homes: Vec<(f32, f32)> = houses[..4].iter().flat_map(|k| k.chunks(8).map(|h| (h[0], h[2]))).collect();
    assert!(homes.len() > 50, "{}", homes.len());
    for (i, a) in homes.iter().enumerate() {
        for b in &homes[i + 1..] {
            assert!((a.0 - b.0).hypot(a.1 - b.1) > 6.0, "{a:?} {b:?}");
        }
    }
}

#[test]
fn a_world_on_a_region_is_deterministic_and_on_its_planet() {
    let region = Arc::new(island());
    let fly = || {
        let mut world = World::with_terrain(3, Arc::new(Terrain::from_region(3, region.clone())));
        let id = world.add_aircraft(AircraftKind::C172, Start::Home);
        world.weather.wind_speed = 6.0;
        world.vehicle_mut(id).aircraft_mut().unwrap().controls.throttle = 1.0;
        let mut pilot = Autopilot::new(Goal::Circuit { airport: world.vehicle(id).home.unwrap(), reversed: false, repeat: false });
        for _ in 0..240 * 60 {
            world.fly(id, &mut pilot);
            world.step();
        }
        (world.vehicle(id).aircraft().unwrap().state, world.earth.latitude)
    };
    let (a, latitude) = fly();
    let (b, _) = fly();
    assert_eq!(a, b);
    assert_eq!(latitude, 39.86 * DEG);
    assert!(-a.pos.z > 100.0, "it took off and climbed: {}", -a.pos.z);

    // Over the sea the aircraft floats; over the lake too.
    let mut world = World::with_terrain(3, Arc::new(Terrain::from_region(3, region)));
    for (n, e) in [(0.0, 6000.0), (-2500.0, -1500.0)] {
        let id = world.add_aircraft(AircraftKind::C172, Start::Air { n, e, heading: 0.0 });
        world.vehicle_mut(id).aircraft_mut().unwrap().state.pos.z = -(world.terrain.height(n, e).max(0.0) + 3.0);
        world.step();
        assert_eq!(world.vehicle(id).environment().kind, GroundKind::Water);
    }
}

#[test]
fn the_buildings_of_the_maps_stand_and_are_run_into() {
    let t = terrain();
    assert_eq!(t.tallest_building(), 30.0);
    assert!(scenery::tallest(&t) >= 30.0 && scenery::tallest(&Terrain::new(1)) == scenery::TALLEST);
    // Found in the tile its middle is in, and nowhere else.
    let tile = |n: f64, e: f64| ((e / scenery::HOUSE_TILE).floor() as i32, (-n / scenery::HOUSE_TILE).floor() as i32);
    let key = tile(2510.0, -1982.5);
    let (ox, oz) = (key.0 as f64 * scenery::HOUSE_TILE, key.1 as f64 * scenery::HOUSE_TILE);
    assert_eq!(t.buildings(-(oz + scenery::HOUSE_TILE), -oz, ox, ox + scenery::HOUSE_TILE).len(), 1);
    assert!(t.buildings(-(oz + 2.0 * scenery::HOUSE_TILE), -(oz + scenery::HOUSE_TILE), ox, ox + scenery::HOUSE_TILE).is_empty());
    // A solid as tall as the maps say, over its whole outline, standing on the lowest ground under it.
    let base = scenery::mapped_base(&t, t.buildings(-(oz + scenery::HOUSE_TILE), -oz, ox, ox + scenery::HOUSE_TILE)[0]);
    let inside = |n: f64, e: f64, up: f64| t.solid_at(n, e, base + up).is_some_and(|h| h.kind == scenery::ObstacleKind::Building);
    assert!(inside(2510.0, -1982.5, 29.0) && inside(2501.0, -1989.0, 1.0) && inside(2519.0, -1976.0, 15.0));
    assert_eq!(t.solid_top(2510.0, -1982.5), Some(base + 30.0));
    assert!(!inside(2510.0, -1982.5, 31.0) && !inside(2523.0, -1982.5, 5.0) && !inside(2510.0, -1970.0, 5.0));
    // No house of the town stands on it or against it.
    let houses = scenery::houses(&t, key, scenery::HOUSE_TILE);
    for v in houses.iter().flat_map(|k| k.as_chunks::<8>().0) {
        let (n, e) = (-(v[2] as f64 + oz), v[0] as f64 + ox);
        assert!(!t.building_near(n, e, 49.0), "a house at {n}, {e}");
    }
}

#[test]
fn no_building_of_the_maps_stands_on_a_runway_its_strip_or_its_apron() {
    // Madeira's runway stands on pillars over a road, car parks and a sports ground, whose
    // buildings the maps draw: the levelled aerodrome stood them on the runway.
    let mut region = island();
    let main = region.airports[0];
    let (c, s) = (main.heading.cos(), main.heading.sin());
    let point = |along: f64, across: f64| [(main.n + along * c - across * s) as f32, (main.e + along * s + across * c) as f32];
    // A square of 16 m round a point along and across the main runway.
    let at = |along: f64, across: f64| {
        let ring = [(-8.0, -8.0), (-8.0, 8.0), (8.0, 8.0), (8.0, -8.0)].iter().map(|&(a, b)| point(along + a, across + b)).collect();
        Footprint::new(ring, 6.0, 0.0, Roof::Flat)
    };
    let kept = region.buildings.len();
    region.buildings.extend([
        // Under the runway, across its edge, beyond its end on the strip, on the apron (left of the
        // heading, `apron: -1`) and, a long shed, lying across the runway with its corners off it.
        at(500.0, 0.0),
        at(-300.0, 25.0),
        at(830.0, -10.0),
        at(-40.0, -80.0),
        Footprint::new(vec![point(300.0, -60.0), point(300.0, 60.0), point(305.0, 60.0), point(305.0, -60.0)], 4.0, 0.0, Roof::Flat),
    ]);
    // Beside it, clear of the strip on the side without the apron, well off the apron and beyond
    // the strip's end: they stay.
    region.buildings.extend([at(0.0, 70.0), at(-500.0, 60.0), at(0.0, -220.0), at(950.0, 0.0)]);
    let t = Terrain::from_region(1, Arc::new(region));
    let all = t.buildings(-5000.0, 5000.0, -5000.0, 5000.0);
    assert_eq!(all.len(), kept + 4, "{all:?}");
    assert!(all.iter().all(|b| !main.blocks(&b.ring)));
    let (n, e) = (point(500.0, 0.0)[0] as f64, point(500.0, 0.0)[1] as f64);
    assert!(!t.building_near(n, e, 20.0));
    assert!(t.solid_at(n, e, t.height(n, e) + 1.0).is_none());
}

/// A wood 3 km across with an avenue through it, a railway's yard, a building standing alone, and
/// a quarter of a city: blocks of the maps round a yard they say nothing of.
fn wooded() -> Region {
    let fine = Grid::centred(0.0, 0.0, 6_000.0, 20.0, 600.0f32);
    let mut cover = Grid::centred(0.0, 0.0, 6_000.0, 20.0, Cover::Forest as u8);
    let mut buildings = vec![Footprint::new(vec![[-500.0, -500.0], [-500.0, -470.0], [-480.0, -470.0], [-480.0, -500.0]], 9.0, 0.0, Roof::Flat)];
    for r in 0..cover.rows {
        for c in 0..cover.cols {
            let (n, e) = cover.node(r, c);
            if (500.0..700.0).contains(&n) && (-1000.0..0.0).contains(&e) {
                cover.set(r, c, Cover::Railway as u8);
            } else if (-1400.0..-600.0).contains(&n) && (600.0..1400.0).contains(&e) {
                cover.set(r, c, Cover::Unknown as u8);
            }
        }
    }
    // The blocks: 24 m squares every 48 m (a quarter of the ground), but in the middle 200 m.
    for i in 0..16 {
        for j in 0..16 {
            let (n, e) = (-1400.0 + 48.0 * i as f32 + 12.0, 600.0 + 48.0 * j as f32 + 12.0);
            if (n + 1000.0).abs() < 130.0 && (e - 1000.0).abs() < 130.0 {
                continue;
            }
            buildings.push(Footprint::new(vec![[n, e], [n, e + 24.0], [n + 24.0, e + 24.0], [n + 24.0, e]], 15.0, 0.0, Roof::Flat));
        }
    }
    Region {
        name: "Wood".into(),
        latitude: 40.4 * DEG,
        longitude: -3.7 * DEG,
        heights: vec![fine, Grid::centred(0.0, 0.0, 60_000.0, 500.0, 600.0f32)],
        cover,
        airports: Vec::new(),
        // A diagonal avenue of 16 m and a road of 6 m across it.
        roads: vec![Road { n0: -1500.0, e0: -1500.0, n1: 1500.0, e1: 1500.0, width: 16.0 }, Road { n0: 1000.0, e0: -1500.0, n1: 1000.0, e1: 1500.0, width: 6.0 }],
        towns: Vec::new(),
        buildings,
        bridges: Vec::new(),
    }
}

#[test]
fn no_tree_stands_on_a_road_a_railway_or_a_building() {
    let region = Arc::new(wooded());
    let t = Terrain::from_region(1, region.clone());
    let size = scenery::TREE_TILE;
    let (mut trees, mut beside) = (0, 0);
    for x in -12..12 {
        for z in -12..12 {
            let obstacles = scenery::tree_obstacles(&t, (x, z), size);
            // What is drawn is what is run into.
            assert_eq!(obstacles.len(), scenery::trees(&t, (x, z), size).iter().map(|k| k.len() / 8).sum::<usize>());
            for o in obstacles {
                trees += 1;
                for r in &region.roads {
                    let kerb = r.distance(o.n, o.e) - r.width / 2.0;
                    assert!(kerb >= scenery::ROAD_VERGE, "a tree {kerb:.1} m from the kerb at {:.0}, {:.0}", o.n, o.e);
                    beside += (kerb < scenery::ROAD_VERGE + 4.0) as usize;
                }
                assert_ne!(region.cover_at(o.n, o.e), Cover::Railway, "a tree on the railway at {:.0}, {:.0}", o.n, o.e);
                assert!(!t.building_near(o.n, o.e, scenery::WALL_GAP), "a tree against a building at {:.0}, {:.0}", o.n, o.e);
            }
        }
    }
    // The wood is still a wood, and it comes up to the roads: an avenue of trees on each side.
    assert!(trees > 10_000, "{trees}");
    assert!(beside > 100, "{beside}");
    // The yard among the blocks is the city's ground, not the seed's wood or fields; out in the
    // country what the maps say nothing of is the seed's.
    let at = |n: f64, e: f64| t.surface(n, e, &t.ground(n, e), 0.0, 0.0);
    assert!(t.built_share(-1000.0, 1000.0) > 0.1, "{}", t.built_share(-1000.0, 1000.0));
    assert_eq!((at(-1000.0, 1000.0).forest, at(-1000.0, 1000.0).farm), (0.0, 0.0));
    assert_eq!(t.built_share(2500.0, -2500.0), 0.0);
    // A generated world's trees keep off its roads too.
    let world = Terrain::new(1);
    for x in -40..40 {
        for z in -40..40 {
            let (ox, oz) = (x as f64 * size, z as f64 * size);
            let roads = world.roads(-(oz + size) - 30.0, -oz + 30.0, ox - 30.0, ox + size + 30.0);
            for o in scenery::tree_obstacles(&world, (x, z), size) {
                assert!(roads.iter().all(|r| r.distance(o.n, o.e) >= r.width / 2.0 + scenery::ROAD_VERGE), "a tree on a road at {:.0}, {:.0}", o.n, o.e);
            }
        }
    }
}
