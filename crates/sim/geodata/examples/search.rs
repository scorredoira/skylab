//! Searches the aerodromes as the game's place picker does, and says how long it takes:
//! `cargo run --release -p geodata --example search -- <words...> [--offline]`.

use std::time::Instant;

use geodata::fetch::{cache_dir, Fetcher};
use geodata::search::Index;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let offline = args.iter().any(|a| a == "--offline");
    let query = args.iter().filter(|a| *a != "--offline").cloned().collect::<Vec<_>>().join(" ");
    let start = Instant::now();
    let index = match Index::load(&Fetcher::new(cache_dir(), offline)) {
        Ok(index) => index,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("{} aerodromes read in {:.0} ms", index.airfields.len(), start.elapsed().as_secs_f64() * 1000.0);
    let start = Instant::now();
    let found = index.search(&query, 20);
    println!("\"{query}\": {} in {:.2} ms", found.len(), start.elapsed().as_secs_f64() * 1000.0);
    for a in found.into_iter().map(|i| &index.airfields[i]) {
        println!("{:8} {:4} {:8} {} — {} ({} runways)", a.code, a.iata.as_deref().unwrap_or(""), a.size.name(), a.name, a.where_it_is(), a.runways.len());
    }
}
