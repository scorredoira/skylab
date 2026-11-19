//! Heights of the ground from the AWS Terrain Tiles, in Mapzen's Terrarium encoding: PNG tiles of
//! 256 by 256 pixels whose red, green and blue make one number, the height in 256ths of a metre
//! above −32768 m. As the tiles' documentation gives it: `(red × 256 + green + blue / 256) − 32768`.
//! Over the sea the coarse zooms give the depth of the bed; the fine ones read about 0 there
//! (`build` puts the bed in).
//!
//! Sources of the data (their terms ask for this attribution): SRTM, GMTED2010 and ETOPO1 (public
//! domain), USGS 3DEP and NED, Copernicus EU-DEM, and the national models of Canada, the United
//! Kingdom, Australia, New Zealand, Norway and Mexico, as the tiles' documentation lists them.

use std::collections::HashMap;

use crate::geo::TileId;
use crate::Error;

/// One tile's heights, row after row from the north-west corner.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightTile {
    pub size: usize,
    pub heights: Vec<f32>,
}

/// Where a tile is kept in the cache.
pub fn key(id: TileId) -> String {
    format!("terrarium/{}/{}/{}.png", id.z, id.x, id.y)
}

/// Where a tile is on the public bucket.
pub fn url(id: TileId) -> String {
    format!("https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{}/{}/{}.png", id.z, id.x, id.y)
}

/// Decodes a Terrarium PNG: RGB, RGBA or indexed, 8 bits a channel (16-bit ones keep their high
/// byte), any square size.
pub fn decode(png_bytes: &[u8]) -> Result<HeightTile, Error> {
    let fail = |what: String| Error::Data(format!("terrain: {what}"));
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| fail(e.to_string()))?;
    let stride = match reader.output_color_type().0 {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => return Err(fail(format!("{other:?}, not RGB"))),
    };
    let (width, height) = (reader.info().width as usize, reader.info().height as usize);
    if width != height {
        return Err(fail(format!("{width} by {height}, not square")));
    }
    let mut pixels = vec![0; reader.output_buffer_size().ok_or_else(|| fail("too large".into()))?];
    reader.next_frame(&mut pixels).map_err(|e| fail(e.to_string()))?;
    let heights = pixels.chunks_exact(stride).take(width * height).map(|p| height_of(p[0], p[1], p[2])).collect();
    Ok(HeightTile { size: width, heights })
}

/// The height a pixel's colour stands for, in metres: its 24 bits are the height in 256ths of a
/// metre above −32768 m, which an f32 holds exactly.
fn height_of(red: u8, green: u8, blue: u8) -> f32 {
    let steps = ((red as i32) << 16 | (green as i32) << 8 | blue as i32) - (32768 << 8);
    steps as f32 / 256.0
}

/// The tiles of one zoom, read as one picture of the Earth: heights by pixel of that zoom.
pub struct Mosaic {
    pub z: u8,
    /// Pixels a side of every tile.
    size: usize,
    tiles: HashMap<(u32, u32), HeightTile>,
    /// What the tiles that are missing are taken to be (the open sea, where the bucket has none).
    pub missing: f32,
}

impl Mosaic {
    pub fn new(z: u8, tiles: HashMap<(u32, u32), HeightTile>) -> Mosaic {
        let size = tiles.values().next().map_or(256, |t| t.size);
        Mosaic { z, size, tiles, missing: 0.0 }
    }

    /// The height of a pixel, counted from the north-west corner of the zoom's tiles.
    fn pixel(&self, x: i64, y: i64) -> f32 {
        let size = self.size as i64;
        let (tx, ty) = (x.div_euclid(size), y.div_euclid(size));
        match self.tiles.get(&(tx as u32, ty as u32)) {
            Some(t) => t.heights[(y.rem_euclid(size) * size + x.rem_euclid(size)) as usize],
            None => self.missing,
        }
    }

    /// Bilinear between the pixels round a point given in tile columns and rows (`geo::tile_xy`).
    pub fn sample(&self, tx: f64, ty: f64) -> f32 {
        let size = self.size as f64;
        // Pixel centres sit half a pixel in.
        let (px, py) = (tx * size - 0.5, ty * size - 0.5);
        let (x0, y0) = (px.floor(), py.floor());
        let (fx, fy) = ((px - x0) as f32, (py - y0) as f32);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = self.pixel(x0, y0) * (1.0 - fx) + self.pixel(x0 + 1, y0) * fx;
        let bottom = self.pixel(x0, y0 + 1) * (1.0 - fx) + self.pixel(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_heights() {
        // The documentation's formula in f64, for every red and some greens and blues.
        for red in 0..=255u8 {
            for (green, blue) in [(0, 0), (1, 128), (127, 255), (255, 255), (200, 3)] {
                let expected = red as f64 * 256.0 + green as f64 + blue as f64 / 256.0 - 32768.0;
                assert_eq!(height_of(red, green, blue) as f64, expected, "{red} {green} {blue}");
            }
        }
        assert_eq!(height_of(128, 0, 0), 0.0);
        assert_eq!(height_of(0, 0, 0), -32768.0);
    }

    #[test]
    fn what_is_not_a_square_rgb_png_is_an_error() {
        let png_of = |width: u32, height: u32, color: png::ColorType, data: &[u8]| {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(data).unwrap();
            bytes
        };
        assert!(decode(&png_of(2, 1, png::ColorType::Rgb, &[128, 0, 0, 128, 1, 0])).is_err());
        assert!(decode(&png_of(1, 1, png::ColorType::Grayscale, &[7])).is_err());
        let tile = decode(&png_of(1, 1, png::ColorType::Rgba, &[128, 10, 64, 255])).unwrap();
        assert_eq!(tile, HeightTile { size: 1, heights: vec![10.25] });
        // Cut anywhere: never a panic.
        let png = png_of(2, 2, png::ColorType::Rgb, &[128; 12]);
        for n in 0..png.len() {
            let _ = decode(&png[..n]);
        }
    }
}
