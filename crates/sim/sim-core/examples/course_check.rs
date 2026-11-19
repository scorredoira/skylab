//! Flies a course's racer round it and says how it went: each lap's time, its speeds, the battery
//! it used, how close it came to the trees, where it crashed; and writes its line, the course's
//! trail and gates as an overlay for a map (lines of `north east`, `# r g b` a colour).
//!
//! `course_check <key> [overlay.txt] [--wind <m/s> <from°>] [--pad <m>] [--quad <key>]`
//! Reads the courses from `assets/courses` of the working directory.

use std::sync::Arc;

use sim_core::course::racer::Racer;
use sim_core::course::{self, file, Track};
use sim_core::scenery::{self, Shape};
use sim_core::{MultirotorKind, Start, World, V3};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str, k: usize| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1 + k)).and_then(|v| v.parse::<f64>().ok());
    let mut files = Vec::new();
    for entry in std::fs::read_dir("assets/courses").expect("assets/courses") {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|x| x == "ron") && p.file_name().is_some_and(|n| n != "gates.ron") {
            files.push((p.display().to_string(), std::fs::read_to_string(&p).unwrap()));
        }
    }
    file::set_courses(file::courses_with(&files).unwrap_or_else(|e| panic!("{e}"))).unwrap();
    let key = args.first().expect("a course's key");
    let course = file::course(key).expect("no such course");
    let mut world = World::new(match course.place {
        course::Place::Seed(s) => s,
        _ => panic!("a generated world's course"),
    });
    if let Some(speed) = value("--wind", 0) {
        world.weather.wind_speed = speed;
        world.weather.wind_from = value("--wind", 1).unwrap_or(0.0);
    }
    let track = Arc::new(Track::new(course, &world.terrain));
    world.obstacles = Arc::new(track.obstacles());
    let quad = args.iter().position(|a| a == "--quad").and_then(|i| args.get(i + 1)).cloned().or(course.quad.clone()).unwrap_or("fpv5".into());
    let kind = MultirotorKind::parse(&quad).expect("no such quad");
    let id = world.add_multirotor(kind, Start::Air { n: track.start.x, e: track.start.y, heading: track.start_heading });
    let across = value("--pad", 0).unwrap_or(0.0);
    let (s, c) = (track.start_heading.sin(), track.start_heading.cos());
    {
        let (terrain, weather, earth) = (world.terrain.clone(), world.weather, world.earth);
        world.vehicles[id.0].reset_on_ground(&terrain, &weather, &earth, track.start.x - s * across, track.start.y + c * across, track.start_heading);
    }
    world.vehicle_mut(id).multirotor_mut().unwrap().controls.arm = true;
    let p = world.vehicle(id).multirotor().unwrap().p.clone();
    let mut racer = Racer::new(track.clone(), &p);
    racer.go(world.time(), world.vehicle(id).body().pos);
    let mut path = Vec::new();
    let (mut top, mut distance, mut closest, mut closest_at) = (0.0f64, 0.0, f64::MAX, V3::ZERO);
    let mut crowns = 0;
    let mut last = world.vehicle(id).body().pos;
    let started = world.time();
    let mut tiles: std::collections::BTreeMap<(i32, i32), Vec<scenery::Obstacle>> = Default::default();
    let mut lowest_agl = f64::MAX;
    let mut most_tilt = (0.0f64, 0.0f64);
    let mut gate_times = Vec::new();
    let mut next = 0;
    let mut last_speed = 0.0f64;
    let mut last_broken = 0;
    while world.time() - started < 240.0 {
        world.fly(id, &mut racer);
        world.step();
        let v = world.vehicle(id);
        let pos = v.body().pos;
        distance += (pos - last).length();
        last = pos;
        top = top.max(v.body().vel.length());
        let e = sim_core::euler_angles(v.body().q);
        let tilt = (e.pitch.cos() * e.roll.cos()).clamp(-1.0, 1.0).acos().to_degrees();
        if tilt > most_tilt.0 {
            most_tilt = (tilt, world.time() - started);
        }
        if racer.race.next != next {
            gate_times.push((next, world.time() - started, v.body().vel.length()));
            next = racer.race.next;
        }
        if racer.race.passed > 0 && racer.race.finished.is_none() {
            lowest_agl = lowest_agl.min(v.agl());
        }
        let step = (world.time() / World::DT).round() as i64;
        if std::env::var("TRACE").is_ok() {
            let speed = v.body().vel.length();
            let broken = v.multirotor().unwrap().blades().iter().filter(|b| b[0] < 1.0).count();
            if speed < last_speed - 2.0 || broken != last_broken {
                last_broken = broken;
                println!("KNOCK {last_speed:.1} -> {speed:.1} m/s at n {:.1} e {:.1} alt {:.1}; blades {:?}", pos.x, pos.y, -pos.z, world.vehicle(id).multirotor().unwrap().blades());
                let (kx, kz) = ((pos.y / 128.0).floor() as i32, (-pos.x / 128.0).floor() as i32);
                for x in kx - 1..=kx + 1 {
                    for z in kz - 1..=kz + 1 {
                        for o in scenery::tree_obstacles(&world.terrain, (x, z), 128.0).iter().chain(world.obstacles.iter()) {
                            let (gap, _, _) = o.surface(pos.x, pos.y, -pos.z);
                            let near_crown = matches!(o.shape, Shape::Tree { crown, radius, top, .. } if (o.n - pos.x).hypot(o.e - pos.y) < radius + 0.5 && -pos.z - o.base > crown - 0.5 && -pos.z - o.base < top);
                            if gap < 3.0 || near_crown {
                                println!("   {:?} at n {:.1} e {:.1} base {:.1}: {:?}, gap {gap:.2}", o.kind, o.n, o.e, o.base, o.shape);
                            }
                        }
                    }
                }
            }
            last_speed = speed;
        }
        if std::env::var("TRACE").is_ok() && step % 24 == 0 {
            let vel = v.body().world_velocity();
            println!("t {:5.1} n {:7.1} e {:7.1} alt {:6.1} agl {:5.2} vz {:5.1} v {:4.1} tilt {:3.0} next {} c {:?} blades {:?}", world.time() - started, pos.x, pos.y, -pos.z, v.agl(), -vel.z, vel.length(), { let e = sim_core::euler_angles(v.body().q); (e.pitch.cos() * e.roll.cos()).clamp(-1.0, 1.0).acos().to_degrees() }, racer.race.next, { let c = v.multirotor().unwrap().controls; [(c.throttle * 100.0).round(), (c.roll * 100.0).round(), (c.pitch * 100.0).round(), (c.yaw * 100.0).round()] }, v.multirotor().unwrap().blades().iter().map(|b| (b[0].max(b[1]) * 100.0).round()).collect::<Vec<_>>());
        }
        if step % 24 == 0 {
            path.push(pos);
            // The nearest trunk, and whether in a crown.
            let (kx, kz) = ((pos.y / 128.0).floor() as i32, (-pos.x / 128.0).floor() as i32);
            for x in kx - 1..=kx + 1 {
                for z in kz - 1..=kz + 1 {
                    let list = tiles.entry((x, z)).or_insert_with(|| scenery::tree_obstacles(&world.terrain, (x, z), 128.0));
                    for o in list.iter() {
                        if let Shape::Tree { .. } = o.shape {
                            let (gap, _, _) = o.surface(pos.x, pos.y, -pos.z);
                            if gap < closest {
                                closest = gap;
                                closest_at = pos;
                            }
                            if o.in_crown(pos.x, pos.y, -pos.z) {
                                crowns += 1;
                            }
                        }
                    }
                }
            }
        }
        if let Some(why) = v.dynamics.crashed() {
            println!("CRASHED at {:.1} s, n {:.1} e {:.1} alt {:.1} (agl {:.1}), going to gate {}: {why}", world.time() - started, pos.x, pos.y, -pos.z, v.agl(), racer.race.next);
            break;
        }
        if racer.race.finished.is_some() {
            break;
        }
    }
    let t = world.time() - started;
    let m = world.vehicle(id).multirotor().unwrap();
    println!(
        "{}: {} laps {:?} total {:?}; {:.0} m in {:.1} s: {:.1} m/s on average, {:.1} top ({:.0} km/h); battery {:.0} mAh used, {:.1} V",
        course.key,
        racer.race.lap_times.len(),
        racer.race.lap_times.iter().map(|x| (x * 10.0).round() / 10.0).collect::<Vec<_>>(),
        racer.race.finished.map(|x| (x * 10.0).round() / 10.0),
        distance,
        t,
        distance / t,
        top,
        top * 3.6,
        m.battery().used / 3.6,
        m.battery().voltage
    );
    println!("closest to a trunk {closest:.2} m at n {:.0} e {:.0}; {crowns} samples in a crown; lowest over the ground {lowest_agl:.2} m; most tilt {:.0}° at {:.1} s", closest_at.x, closest_at.y, most_tilt.0, most_tilt.1);
    let lap = track.lap_length();
    println!("lap along the gates {lap:.0} m; gates (gate, s, m/s): {}", gate_times.iter().map(|(g, t, v)| format!("{g}:{t:.1}/{v:.0}")).collect::<Vec<_>>().join(" "));

    if let Some(out) = args.get(1).filter(|a| !a.starts_with("--")) {
        let mut text = String::new();
        text += "# 255 255 255\n";
        for c in course::cuts(&world.terrain) {
            text += &format!("{} {}\n{} {}\n\n", c.from.0, c.from.1, c.to.0, c.to.1);
        }
        text += "# 255 0 0\n";
        for (i, g) in track.gates.iter().enumerate() {
            let (_, r, _) = g.axes();
            let half = 2.0;
            let (a, b) = (g.foot - r * half, g.foot + r * half);
            text += &format!("{} {}\n{} {}\n\n", a.x, a.y, b.x, b.y);
            let _ = i;
        }
        text += "# 30 60 255\n";
        for p in &path {
            text += &format!("{} {}\n", p.x, p.y);
        }
        std::fs::write(out, text).unwrap();
    }
}
