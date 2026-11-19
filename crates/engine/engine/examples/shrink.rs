//! A picture made smaller and kept as a JPEG: for the pictures a game ships (the flight manual's).
//! `cargo run --release -p engine --example shrink -- <in.png> <out.jpg> <width> [<quality>] [<x>,<y>,<w>,<h>]`
//!
//! The optional box is cut first, in pixels of the input; then the picture is scaled to `width`
//! pixels wide (its height in proportion) with a Lanczos filter and written at `quality` (1..100, 82).

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: shrink <in.png> <out.jpg> <width> [<quality>] [<x>,<y>,<w>,<h>]");
        std::process::exit(2);
    }
    let mut picture = image::open(&args[0]).unwrap_or_else(|error| panic!("{}: {error}", args[0])).to_rgb8();
    let width: u32 = args[2].parse().expect("the width, in pixels");
    let quality: u8 = args.get(3).map_or(82, |q| q.parse().expect("the quality, 1 to 100"));
    if let Some(cut) = args.get(4) {
        let n: Vec<u32> = cut.split(',').map(|v| v.trim().parse().expect("x,y,w,h in pixels")).collect();
        let [x, y, w, h] = n[..] else { panic!("the box is x,y,w,h") };
        picture = image::imageops::crop_imm(&picture, x, y, w, h).to_image();
    }
    let height = (picture.height() as f64 * width as f64 / picture.width() as f64).round() as u32;
    let small = image::imageops::resize(&picture, width, height, FilterType::Lanczos3);
    let file = std::fs::File::create(&args[1]).unwrap_or_else(|error| panic!("{}: {error}", args[1]));
    JpegEncoder::new_with_quality(std::io::BufWriter::new(file), quality).encode_image(&small).expect("could not write the JPEG");
    println!("{}: {width}x{height}, {} bytes", args[1], std::fs::metadata(&args[1]).map_or(0, |m| m.len()));
}
