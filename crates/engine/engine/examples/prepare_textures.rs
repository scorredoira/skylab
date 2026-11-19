//! Turns downloaded PBR materials (2K or 4K originals) into the small, same-sized, tileable set a
//! game ships, and draws contact sheets to judge candidates by eye. Needs no GPU.
//!
//! ```text
//! prepare_textures material <out_dir> <name> <size> color=<file> normal=<file> [rough=<file>] [ao=<file>] [dx]
//! prepare_textures sheet <out.png> <cell> <file>...
//! prepare_textures web <size> <quality> <out_dir> <file>...
//! prepare_textures cutout <out.png> <scale> <diffuse> <alpha> <x0,y0,x1,y1>
//! ```
//!
//! `material` writes `<name>_color.jpg` (sRGB, quality 90), `<name>_normal.png` (OpenGL convention:
//! +X right, +Y up the image, +Z out; `dx` flips the green channel of a DirectX map),
//! `<name>_rough.jpg` and `<name>_ao.jpg` (8-bit grey, quality 80). Only the normal map is kept
//! lossless: compression blocks in it show as blocks in the lighting. Colour is averaged in linear
//! light, normals are renormalised, and the borders are filtered as if the image repeated, so a
//! tileable original stays tileable.
//!
//! `sheet` lays the files out in a grid, each one repeated 2 x 2 in a square of `cell` pixels with its
//! name burnt into the file order (left to right, top to bottom): seams and repetition show at once.

use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::{GrayImage, ImageBuffer, Luma, Pixel, PixelWithColorType, Rgb, Rgb32FImage, RgbImage};
use std::path::Path;

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn open(path: &str) -> Rgb32FImage {
    image::open(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}")).to_rgb32f()
}

/// Shrinks to `size` x `size`. The image is first surrounded by a margin of its own repeats, so the
/// filter sees across the border what a repeating sampler will show there. An original twice as tall
/// as it is wide (some barks) is squeezed into the square: its repeat is then twice as tall as wide.
fn shrink(src: &Rgb32FImage, size: u32) -> Rgb32FImage {
    let (w, h) = src.dimensions();
    assert!(w == h || h == 2 * w, "the original must be square or twice as tall as wide, it is {w} x {h}");
    if w == size && h == size {
        return src.clone();
    }
    assert!(w % size == 0, "the original ({w}) must be a multiple of the target size ({size})");
    let (mx, my) = (8 * (w / size), 8 * (h / size));
    let padded = ImageBuffer::from_fn(w + 2 * mx, h + 2 * my, |x, y| *src.get_pixel((x + w - mx) % w, (y + h - my) % h));
    let small = imageops::resize(&padded, size + 16, size + 16, FilterType::Lanczos3);
    imageops::crop_imm(&small, 8, 8, size, size).to_image()
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn color(path: &str, size: u32) -> RgbImage {
    let mut img = open(path);
    for p in img.pixels_mut() {
        p.0 = p.0.map(srgb_to_linear);
    }
    let small = shrink(&img, size);
    ImageBuffer::from_fn(size, size, |x, y| Rgb(small.get_pixel(x, y).0.map(|v| byte(linear_to_srgb(v.max(0.0))))))
}

fn normal(path: &str, size: u32, directx: bool) -> RgbImage {
    let small = shrink(&open(path), size);
    ImageBuffer::from_fn(size, size, |x, y| {
        let [r, g, b] = small.get_pixel(x, y).0;
        let (nx, mut ny, nz) = (r * 2.0 - 1.0, g * 2.0 - 1.0, (b * 2.0 - 1.0).max(0.05));
        if directx {
            ny = -ny;
        }
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        Rgb([byte(nx / len * 0.5 + 0.5), byte(ny / len * 0.5 + 0.5), byte(nz / len * 0.5 + 0.5)])
    })
}

fn gray(path: &str, size: u32) -> GrayImage {
    let small = shrink(&open(path), size);
    ImageBuffer::from_fn(size, size, |x, y| Luma([byte(small.get_pixel(x, y).0[0])]))
}

/// PNG at the highest compression: these files are written once and kept in the repository.
fn save_png<P: Pixel<Subpixel = u8> + PixelWithColorType>(image: &ImageBuffer<P, Vec<u8>>, path: &Path) {
    let writer = std::io::BufWriter::new(std::fs::File::create(path).expect("cannot write"));
    image.write_with_encoder(PngEncoder::new_with_quality(writer, CompressionType::Best, PngFilter::Adaptive)).expect("cannot encode");
}

fn save_jpeg<P: Pixel<Subpixel = u8> + PixelWithColorType>(image: &ImageBuffer<P, Vec<u8>>, path: &Path, quality: u8) {
    let mut writer = std::io::BufWriter::new(std::fs::File::create(path).expect("cannot write"));
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality).encode_image(image).expect("cannot encode");
}

fn material(args: &[String]) {
    let (out, name) = (Path::new(&args[0]), &args[1]);
    let size: u32 = args[2].parse().expect("size");
    std::fs::create_dir_all(out).expect("cannot create the output folder");
    let directx = args.iter().any(|a| a == "dx");
    for arg in &args[3..] {
        let Some((kind, file)) = arg.split_once('=') else { continue };
        match kind {
            "color" => save_jpeg(&color(file, size), &out.join(format!("{name}_color.jpg")), 90),
            "normal" => save_png(&normal(file, size, directx), &out.join(format!("{name}_normal.png"))),
            "rough" => save_jpeg(&gray(file, size), &out.join(format!("{name}_rough.jpg")), 80),
            "ao" => save_jpeg(&gray(file, size), &out.join(format!("{name}_ao.jpg")), 80),
            other => panic!("unknown map kind: {other}"),
        }
    }
}

fn sheet(args: &[String]) {
    let cell: u32 = args[1].parse().expect("cell size");
    let files = &args[2..];
    let columns = (files.len() as f32).sqrt().ceil() as u32;
    let rows = (files.len() as u32).div_ceil(columns);
    let gap = 6;
    let mut out = RgbImage::from_pixel(columns * (cell + gap), rows * (cell + gap), Rgb([255, 0, 255]));
    for (i, file) in files.iter().enumerate() {
        let half = imageops::resize(&image::open(file).unwrap_or_else(|e| panic!("cannot read {file}: {e}")).to_rgb8(), cell / 2, cell / 2, FilterType::Triangle);
        let (cx, cy) = ((i as u32 % columns) * (cell + gap), (i as u32 / columns) * (cell + gap));
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            imageops::replace(&mut out, &half, (cx + dx * (cell / 2)) as i64, (cy + dy * (cell / 2)) as i64);
        }
        println!("{},{}: {file}", i as u32 / columns + 1, i as u32 % columns + 1);
    }
    out.save(&args[0]).expect("cannot write the sheet");
}

/// For the web build, where every byte is downloaded: each file shrunk to `size` (a normal map
/// renormalised, known by `_normal` in its name) and written as a JPEG of `quality` under its own
/// name in `out_dir`; the game reads a picture by its bytes, not its name. A lossless normal map of
/// 1024 is some 2.5 MB; at 512 and quality 90, a tenth of that.
fn web(args: &[String]) {
    let size: u32 = args[0].parse().expect("size");
    let quality: u8 = args[1].parse().expect("quality, 1 to 100");
    let out = Path::new(&args[2]);
    std::fs::create_dir_all(out).expect("cannot create the output folder");
    for file in &args[3..] {
        let name = Path::new(file).file_name().expect("a file name");
        if name.to_string_lossy().contains("_normal") {
            save_jpeg(&normal(file, size, false), &out.join(name), quality);
        } else {
            save_jpeg(&gray(file, size), &out.join(name), quality);
        }
    }
}


/// A part of a plant model's texture with its alpha, for leaves and twigs drawn on cards: the
/// rectangle `x0,y0,x1,y1` (pixels of the originals) of `diffuse` with the opacity of `alpha`,
/// shrunk by `scale` (0.5 halves it), written as an RGBA PNG. Where it is transparent the colour
/// is spread from the nearest opaque texels, so that a filtered or smaller copy brings in no
/// colour of the background the model's texture had there.
fn cutout(args: &[String]) {
    let out = Path::new(&args[0]);
    let scale: f32 = args[1].parse().expect("scale");
    let (diffuse, alpha) = (open(&args[2]), image::open(&args[3]).unwrap_or_else(|e| panic!("cannot read {}: {e}", args[3])).to_luma32f());
    let rect: Vec<u32> = args[4].split(',').map(|v| v.parse().expect("x0,y0,x1,y1")).collect();
    let (x0, y0, w, h) = (rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]);
    // Premultiplied linear colour and opacity, shrunk together.
    let mut img: ImageBuffer<image::Rgba<f32>, Vec<f32>> = ImageBuffer::from_fn(w, h, |x, y| {
        let a = alpha.get_pixel(x0 + x, y0 + y).0[0];
        let c = diffuse.get_pixel(x0 + x, y0 + y).0.map(srgb_to_linear);
        image::Rgba([c[0] * a, c[1] * a, c[2] * a, a])
    });
    if scale != 1.0 {
        img = imageops::resize(&img, ((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1), FilterType::Lanczos3);
    }
    let (w, h) = img.dimensions();
    // Unpremultiplied colour where there is some, then spread outwards ring by ring.
    let mut colour: Vec<Option<[f32; 3]>> = img.pixels().map(|p| (p.0[3] > 0.02).then(|| [p.0[0] / p.0[3], p.0[1] / p.0[3], p.0[2] / p.0[3]])).collect();
    for _ in 0..w.max(h) {
        let before = colour.clone();
        let mut changed = false;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                if before[(y as u32 * w + x as u32) as usize].is_some() {
                    continue;
                }
                let (mut sum, mut n) = ([0.0f32; 3], 0.0);
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 {
                        if let Some(c) = before[(ny as u32 * w + nx as u32) as usize] {
                            sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]];
                            n += 1.0;
                        }
                    }
                }
                if n > 0.0 {
                    colour[(y as u32 * w + x as u32) as usize] = Some(sum.map(|v| v / n));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let rgba = image::RgbaImage::from_fn(w, h, |x, y| {
        let c = colour[(y * w + x) as usize].unwrap_or([0.0; 3]);
        image::Rgba([byte(linear_to_srgb(c[0])), byte(linear_to_srgb(c[1])), byte(linear_to_srgb(c[2])), byte(img.get_pixel(x, y).0[3])])
    });
    save_png(&rgba, out);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("material") if args.len() >= 6 => material(&args[1..]),
        Some("sheet") if args.len() >= 4 => sheet(&args[1..]),
        Some("web") if args.len() >= 5 => web(&args[1..]),
        Some("cutout") if args.len() >= 6 => cutout(&args[1..]),
        _ => eprintln!("usage:\n  prepare_textures material <out_dir> <name> <size> color=<file> normal=<file> [rough=<file>] [ao=<file>] [dx]\n  prepare_textures sheet <out.png> <cell> <file>...\n  prepare_textures web <size> <quality> <out_dir> <file>...\n  prepare_textures cutout <out.png> <scale> <diffuse> <alpha> <x0,y0,x1,y1>"),
    }
}
