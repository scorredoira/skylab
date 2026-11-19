//! Loads (and the first time downloads and makes) the Sentinel-2 picture of a place's ground and
//! says how it went; with `--png <dir>` writes each grid of it as a picture to look at:
//! `cargo run --release -p geodata --example imagery -- <ICAO | lat,lon> [--offline] [--png <dir>]`.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(place) = args.first().and_then(|a| geodata::Place::parse(a)) else {
        eprintln!("usage: imagery <ICAO | lat,lon> [--offline] [--png <dir>]");
        std::process::exit(2);
    };
    let offline = args.iter().any(|a| a == "--offline");
    let png = args.iter().position(|a| a == "--png").and_then(|i| args.get(i + 1));
    let fail = |e: geodata::Error| -> ! {
        eprintln!("{e}");
        std::process::exit(1)
    };
    let region = geodata::load(&place, offline, &|_| {}).unwrap_or_else(|e| fail(e)).region;
    let start = Instant::now();
    let last = std::sync::Mutex::new(Instant::now());
    let loaded = geodata::imagery::load(&region, offline, &|p| {
        let mut last = last.lock().unwrap();
        if last.elapsed().as_secs_f64() > 1.0 || p.stage == geodata::imagery::Stage::Done {
            *last = Instant::now();
            eprintln!("{:5.1} s  {:3.0} %  {}", start.elapsed().as_secs_f64(), p.share() * 100.0, p.text());
        }
    });
    let loaded = loaded.unwrap_or_else(|e| fail(e));
    let i = &loaded.imagery;
    println!("{}: {:.1} s, {:.1} MB downloaded, {}, years {:?}", region.name, start.elapsed().as_secs_f64(), loaded.downloaded as f64 / 1e6, if loaded.kept { "kept" } else { "made" }, i.years);
    for (k, g) in i.levels.iter().enumerate() {
        let known = g.data.iter().filter(|t| t[3] > 0).count() as f64 / g.data.len() as f64;
        println!("level {k}: {} x {} every {} m, {:.1} % known", g.rows, g.cols, g.step, known * 100.0);
        if let Some(dir) = png {
            let path = std::path::Path::new(dir).join(format!("imagery{k}.png"));
            let file = std::fs::File::create(&path).unwrap_or_else(|e| fail(geodata::Error::Io(e.to_string())));
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), g.cols as u32, g.rows as u32);
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header().unwrap();
            // Row 0 of the grid is the south: the picture's first row is the north.
            let rows: Vec<u8> = g.data.chunks(g.cols).rev().flatten().flatten().copied().collect();
            writer.write_image_data(&rows).unwrap();
            println!("  {}", path.display());
        }
    }
}
