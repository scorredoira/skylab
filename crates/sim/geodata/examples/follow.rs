//! A real place followed as far as the aircraft flies, without a window: a C172 on its autopilot
//! flies a straight line from a place, the simulation sped up, and the regions further along are
//! built in the background (`geodata::follow`) and swapped in under it, as the game does. Says when
//! each was asked for and came, how long the swap took, how much the ground under the aircraft
//! moved at it, whether the aircraft ever outflew its region, what was downloaded and the
//! process's peak memory:
//!
//! `cargo run --release -p geodata --example follow -- <ICAO | lat,lon> [km=300] [heading=<deg>]
//! [altitude=<m>] [speed=<times>] [offline=1] [imagery=1]`
//!
//! `speed` is how many times faster than real time the simulation runs (20); `imagery=1` makes
//! each region's ground picture too, as the game does after a swap.

use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sim_core::{act, AircraftKind, Autopilot, Goal, Start, Terrain, Vertical, World, DEG, KNOTS};
use geodata::follow::{Follow, SERVES};

/// What the process holds in memory now, kilobytes, as `ps` says.
fn rss() -> u64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output();
    out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).unwrap_or(0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().find_map(|a| a.strip_prefix(name)?.strip_prefix('='));
    let number = |name: &str, default: f64| value(name).and_then(|v| v.parse::<f64>().ok()).unwrap_or(default);
    let Some(place) = args.first().and_then(|a| geodata::Place::parse(a)) else {
        eprintln!("usage: follow <ICAO | lat,lon> [km=300] [heading=<degrees>] [altitude=<m>] [speed=<times>] [offline=1] [imagery=1]");
        std::process::exit(2);
    };
    let (km, heading, altitude, speed) = (number("km", 300.0), number("heading", 90.0) * DEG, number("altitude", 2500.0), number("speed", 20.0));
    let (offline, imagery) = (value("offline") == Some("1"), value("imagery") == Some("1"));
    let seed = 1;
    let fail = |e: geodata::Error| -> ! {
        eprintln!("{e}");
        std::process::exit(1)
    };
    let first = geodata::load(&place, offline, &|_| {}).unwrap_or_else(|e| fail(e));
    let mut downloaded = first.downloaded;
    let mut world = World::with_terrain(seed, Arc::new(Terrain::from_region(seed, Arc::new(first.region))));
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n: 0.0, e: 0.0, heading });
    world.vehicle_mut(id).aircraft_mut().unwrap().state.pos.z = -altitude;
    world.vehicle_mut(id).aircraft_mut().unwrap().controls.throttle = 0.8;
    let mut pilot = Autopilot::new(Goal::Hold { heading: Some(heading), vertical: Vertical::Altitude(altitude), airspeed: Some(100.0 * KNOTS) });

    let mut follow = Follow::new((0.0, 0.0));
    let (sender, built) = channel();
    let start = Instant::now();
    let (mut swaps, mut worst_swap, mut worst_step, mut outflown, mut peak) = (0, Duration::ZERO, 0.0f64, 0.0f64, rss());
    println!("{:>7} {:>8} {:>8}  what", "sim s", "km", "MB");
    loop {
        let s = world.vehicle(id).aircraft().unwrap().state;
        let travelled = s.pos.x.hypot(s.pos.y) / 1000.0;
        if travelled >= km {
            break;
        }
        // A simulated second, then the wall clock is let catch up to the speed asked for.
        for _ in 0..240 {
            world.fly(id, &mut pilot);
            world.step();
        }
        let sim = world.time();
        let behind = Duration::from_secs_f64(sim / speed).saturating_sub(start.elapsed());
        std::thread::sleep(behind);

        let s = world.vehicle(id).aircraft().unwrap().state;
        let v = act(s.q, s.vel);
        let (n, e) = (s.pos.x, s.pos.y);
        // How far the aircraft is beyond what its region serves (0 while it is served).
        let beyond = ((n - follow.centre.0).abs().max((e - follow.centre.1).abs()) - SERVES).max(0.0);
        outflown = outflown.max(beyond);
        if let Some(centre) = follow.next(n, e, v.x, v.y) {
            println!("{sim:7.0} {travelled:8.1} {:8.0}  asks for the region round {:.0}, {:.0} km", rss() as f64 / 1024.0, centre.0 / 1000.0, centre.1 / 1000.0);
            let (sender, place) = (sender.clone(), place.clone());
            std::thread::spawn(move || {
                let started = Instant::now();
                let made = geodata::load_around(&place, centre, offline, &|_| {}).and_then(|loaded| {
                    let mut bytes = loaded.downloaded;
                    let terrain = Arc::new(Terrain::from_region(1, Arc::new(loaded.region)));
                    if imagery {
                        let region = terrain.region().expect("a region");
                        bytes += geodata::imagery::load(region, offline, &|_| {})?.downloaded;
                    }
                    Ok((terrain, bytes))
                });
                let _ = sender.send((made, started.elapsed()));
            });
        }
        if let Ok((made, took)) = built.try_recv() {
            match made {
                Ok((terrain, bytes)) => {
                    downloaded += bytes;
                    if follow.arrived(n, e) {
                        let before = world.terrain.height(n, e);
                        let swap = Instant::now();
                        // As the game does: the old region let go of on a thread (freeing it takes milliseconds).
                        let old = std::mem::replace(&mut world.terrain, terrain);
                        let swap = swap.elapsed();
                        std::thread::spawn(move || drop(old));
                        let step = (world.terrain.height(n, e) - before).abs();
                        (swaps, worst_swap, worst_step) = (swaps + 1, worst_swap.max(swap), worst_step.max(step));
                        println!("{sim:7.0} {travelled:8.1} {:8.0}  flies on the region round {:.0}, {:.0} km: built in {:.1} s, {:.1} MB, swapped in {:?}, ground under it moved {step:.2} m", rss() as f64 / 1024.0, follow.centre.0 / 1000.0, follow.centre.1 / 1000.0, took.as_secs_f64(), bytes as f64 / 1e6, swap);
                    }
                }
                Err(e) => {
                    follow.failed();
                    println!("{sim:7.0} {travelled:8.1}  could not build it: {e}");
                }
            }
        }
        peak = peak.max(rss());
    }
    let s = world.vehicle(id).aircraft().unwrap().state;
    println!(
        "{km:.0} km in {:.0} s of flight ({:.0} s here), {swaps} regions swapped in (the longest swap {worst_swap:?}, the ground under the aircraft moved {worst_step:.2} m at most), outflown by {:.1} km at most, {:.0} MB downloaded, peak memory {:.0} MB, at {:.0} m",
        world.time(),
        start.elapsed().as_secs_f64(),
        outflown / 1000.0,
        downloaded as f64 / 1e6,
        peak as f64 / 1024.0,
        -s.pos.z
    );
}
