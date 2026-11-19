//! Compares two pictures of the same size pixel by pixel, to prove that a change to a renderer did
//! not change what it draws.
//! `cargo run --release -p engine --example compare -- <a.png> <b.png> [<difference.png>]`
//!
//! Prints how many pixels differ, by how much at most (0..255, the worst channel) and how many
//! differ by more than 8, which is more than rounding. With a third path, writes a picture that is
//! white where they differ. Exits with 1 when any pixel differs.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: compare <a.png> <b.png> [<difference.png>]");
        std::process::exit(2);
    }
    let open = |path: &str| image::open(path).unwrap_or_else(|error| panic!("{path}: {error}")).to_rgba8();
    let (a, b) = (open(&args[0]), open(&args[1]));
    if a.dimensions() != b.dimensions() {
        eprintln!("different sizes: {:?} and {:?}", a.dimensions(), b.dimensions());
        std::process::exit(1);
    }
    let (mut differ, mut clearly, mut worst) = (0u64, 0u64, 0u8);
    let mut mask = image::GrayImage::new(a.width(), a.height());
    for ((pa, pb), m) in a.pixels().zip(b.pixels()).zip(mask.pixels_mut()) {
        let d = pa.0.iter().zip(pb.0).map(|(x, y)| x.abs_diff(y)).max().unwrap_or(0);
        if d > 0 {
            differ += 1;
            m.0 = [255];
        }
        if d > 8 {
            clearly += 1;
        }
        worst = worst.max(d);
    }
    println!("{differ} of {} pixels differ, {clearly} by more than 8, worst {worst}", a.width() as u64 * a.height() as u64);
    if let Some(path) = args.get(2) {
        mask.save(path).expect("could not write the difference");
    }
    std::process::exit(if differ > 0 { 1 } else { 0 });
}
