//! Loads (and the first time downloads and builds) the region of a place and says what it holds:
//! `cargo run --release -p geodata --example region -- <ICAO | lat,lon> [--offline]`.

use std::time::Instant;

use sim_core::{Cover, Terrain};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(place) = args.first().and_then(|a| geodata::Place::parse(a)) else {
        eprintln!("usage: region <ICAO | lat,lon> [--offline]");
        std::process::exit(2);
    };
    let offline = args.iter().any(|a| a == "--offline");
    let start = Instant::now();
    let last = std::sync::Mutex::new(Instant::now());
    let loaded = geodata::load(&place, offline, &|p: geodata::Progress| {
        let mut last = last.lock().unwrap();
        if last.elapsed().as_secs_f64() > 1.0 || p.stage == geodata::Stage::Done {
            *last = Instant::now();
            eprintln!("{:5.1} s  {:3.0} %  {}", start.elapsed().as_secs_f64(), p.share() * 100.0, p.text());
        }
    });
    let loaded = match loaded {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let r = &loaded.region;
    println!("{}: {:.1} s, {:.1} MB downloaded, {}", r.name, start.elapsed().as_secs_f64(), loaded.downloaded as f64 / 1e6, if loaded.kept { "kept" } else { "built" });
    println!("origin {:.5}, {:.5}", r.latitude.to_degrees(), r.longitude.to_degrees());
    for g in &r.heights {
        let (lo, hi) = g.data.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &h| (lo.min(h), hi.max(h)));
        println!("heights {} x {} every {} m: {lo:.0} to {hi:.0} m", g.rows, g.cols, g.step);
    }
    let mut counts = [0usize; Cover::COUNT];
    for &b in &r.cover.data {
        counts[Cover::from_byte(b) as usize] += 1;
    }
    let total = r.cover.data.len() as f64;
    let shares: Vec<String> = Cover::ALL.iter().map(|c| format!("{c:?} {:.1}%", counts[*c as usize] as f64 / total * 100.0)).collect();
    println!("cover: {}", shares.join(", "));
    println!("{} runways, {} road segments, {} towns", r.airports.len(), r.roads.len(), r.towns.len());
    let tallest = r.buildings.iter().map(|b| b.height).fold(0.0f32, f32::max);
    let measured = r.buildings.iter().filter(|b| b.measured).count();
    let pitched = r.buildings.iter().filter(|b| b.roof.pitched()).count();
    println!("{} buildings ({measured} of measured height, {pitched} with pitched roofs), the tallest {tallest:.0} m", r.buildings.len());
    let landmarks: Vec<_> = r.bridges.iter().filter(|b| b.landmark).collect();
    println!("{} bridges ({:.0} km of deck), {} of them landmarks", r.bridges.len(), r.bridges.iter().map(|b| b.length()).sum::<f64>() / 1000.0, landmarks.len());
    for a in r.airports.iter().take(12) {
        println!("  runway {:02} id {} n {:.0} e {:.0} {:.0} x {:.0} m, {:.0} m, {:?}", a.number(), a.id, a.n, a.e, a.length, a.width, a.elevation, a.detail);
    }
    let terrain = Terrain::from_region(1, std::sync::Arc::new(loaded.region));
    let home = terrain.nearest_airport(0.0, 0.0);
    println!("home: {home:?}");
}
