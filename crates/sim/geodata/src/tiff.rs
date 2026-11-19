//! Cloud-Optimized GeoTIFFs read by parts: the header says where every tile of every level is, and
//! only the tiles wanted are fetched (HTTP range requests through `fetch`). Enough of TIFF for the
//! Sentinel-2 files of AWS Open Data: classic or BigTIFF, little-endian, tiled, 8 or 16 bits a sample,
//! chunky (pixel-interleaved), uncompressed or deflate with or without the horizontal predictor; the
//! levels are the full image and its overviews; the georeference is a tie point and a pixel size
//! in a projection named by its EPSG code.

use crate::Error;

/// One level of the image: the full resolution or an overview.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    /// Samples a pixel (3 for a colour image, 1 for a class).
    pub samples: u16,
    /// Bytes a sample: 1 or 2.
    pub bytes: u16,
    compression: u16,
    predictor: u16,
    /// Where each tile is in the file and how long, row after row of tiles.
    pub offsets: Vec<u64>,
    pub lengths: Vec<u64>,
}

impl Level {
    /// Tiles across and down.
    pub fn tiles(&self) -> (u32, u32) {
        (self.width.div_ceil(self.tile_width), self.height.div_ceil(self.tile_height))
    }

    /// Where tile (column, row) is in the file: offset and length; `None` for a tile not written
    /// (all of it no data).
    pub fn range(&self, column: u32, row: u32) -> Option<(u64, u64)> {
        let i = (row * self.tiles().0 + column) as usize;
        let (offset, length) = (*self.offsets.get(i)?, *self.lengths.get(i)?);
        (length > 0).then_some((offset, length))
    }

    /// The samples of a tile from its bytes as stored: `tile_width · tile_height · samples` of
    /// `bytes` each (little-endian), row after row.
    pub fn decode(&self, bytes: &[u8]) -> Result<Vec<u8>, Error> {
        let (k, wide) = (self.samples as usize, self.bytes == 2);
        let size = (self.tile_width * self.tile_height) as usize * k * self.bytes as usize;
        let mut data = match self.compression {
            1 => bytes.to_vec(),
            8 | 32946 => {
                use std::io::Read;
                let mut out = Vec::with_capacity(size);
                flate2::read::ZlibDecoder::new(bytes).read_to_end(&mut out).map_err(|e| Error::Data(format!("GeoTIFF: {e}")))?;
                out
            }
            other => return Err(Error::Data(format!("GeoTIFF: compression {other}"))),
        };
        if data.len() < size {
            return Err(Error::Data("GeoTIFF: short tile".into()));
        }
        data.truncate(size);
        if self.predictor == 2 {
            // Each sample is stored as its difference from the same sample of the pixel to its left.
            for line in data.chunks_mut(self.tile_width as usize * k * self.bytes as usize) {
                if wide {
                    for i in k..line.len() / 2 {
                        let left = u16::from_le_bytes([line[2 * (i - k)], line[2 * (i - k) + 1]]);
                        let v = u16::from_le_bytes([line[2 * i], line[2 * i + 1]]).wrapping_add(left);
                        line[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
                    }
                } else {
                    for i in k..line.len() {
                        line[i] = line[i].wrapping_add(line[i - k]);
                    }
                }
            }
        }
        Ok(data)
    }
}

/// What the header of a GeoTIFF says.
#[derive(Clone, Debug, PartialEq)]
pub struct Cog {
    /// The full image first, then its overviews, each half the one before or so.
    pub levels: Vec<Level>,
    /// The projection's EPSG code (32631 is UTM zone 31 north).
    pub epsg: u32,
    /// Projected x and y of the north-west corner of the image, and the size of a pixel of the
    /// full image, metres.
    pub west: f64,
    pub north: f64,
    pub pixel: f64,
}

impl Cog {
    /// The georeference of a level: x and y of its north-west corner and the size of its pixel.
    pub fn pixel_of(&self, level: usize) -> f64 {
        let full = &self.levels[0];
        self.pixel * full.width as f64 / self.levels[level].width as f64
    }
}

/// Reads the header from the first bytes of the file. `more(offset, length)` gives other bytes
/// of it when an array lies beyond them (rare: a COG keeps its header first).
pub fn parse(head: &[u8], more: &dyn Fn(u64, u64) -> Result<Vec<u8>, Error>) -> Result<Cog, Error> {
    let bad = |what: &str| Error::Data(format!("GeoTIFF: {what}"));
    if head.len() < 16 || &head[0..2] != b"II" {
        return Err(bad("header"));
    }
    let u16_at = |b: &[u8], p: usize| -> Result<u16, Error> { b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| bad("short")) };
    let u32_at = |b: &[u8], p: usize| -> Result<u32, Error> { b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| bad("short")) };
    let u64_at = |b: &[u8], p: usize| -> Result<u64, Error> { b.get(p..p + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap_or_default())).ok_or_else(|| bad("short")) };
    let big = match u16_at(head, 2)? {
        42 => false,
        43 => true,
        _ => return Err(bad("version")),
    };
    // The bytes from `offset` on, from the head when they are in it.
    let bytes = |offset: u64, length: u64| -> Result<std::borrow::Cow<'_, [u8]>, Error> {
        let end = offset.checked_add(length).ok_or_else(|| bad("offset"))?;
        if end <= head.len() as u64 {
            Ok(std::borrow::Cow::Borrowed(&head[offset as usize..end as usize]))
        } else {
            Ok(std::borrow::Cow::Owned(more(offset, length)?))
        }
    };
    let (entry_size, count_size) = if big { (20u64, 8u64) } else { (12, 2) };
    let mut next = if big { u64_at(head, 8)? } else { u32_at(head, 4)? as u64 };
    let mut levels = Vec::new();
    let (mut epsg, mut tie, mut scale) = (0u32, None, None);
    while next != 0 && levels.len() < 16 {
        let count_bytes = bytes(next, count_size)?;
        let count = if big { u64_at(&count_bytes, 0)? } else { u16_at(&count_bytes, 0)? as u64 };
        let table = bytes(next + count_size, count * entry_size + if big { 8 } else { 4 })?;
        let mut tags = std::collections::HashMap::new();
        for i in 0..count as usize {
            let e = i * entry_size as usize;
            let tag = u16_at(&table, e)?;
            let kind = u16_at(&table, e + 2)?;
            let n = if big { u64_at(&table, e + 4)? } else { u32_at(&table, e + 4)? as u64 };
            let width: u64 = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 | 16 | 17 | 18 => 8,
                _ => continue,
            };
            let inline = if big { 8 } else { 4 };
            let field = if big { e + 12 } else { e + 8 };
            let data: Vec<u8> = if n * width <= inline {
                table[field..field + (n * width) as usize].to_vec()
            } else {
                let at = if big { u64_at(&table, field)? } else { u32_at(&table, field)? as u64 };
                bytes(at, n * width)?.into_owned()
            };
            tags.insert(tag, (kind, n, data));
        }
        let after = (count * entry_size) as usize;
        next = if big { u64_at(&table, after)? } else { u32_at(&table, after)? as u64 };

        // The values of a tag as integers or as floats.
        let ints = |tag: u16| -> Vec<u64> {
            let Some((kind, n, data)) = tags.get(&tag) else { return Vec::new() };
            (0..*n as usize)
                .filter_map(|i| match kind {
                    1 | 7 => data.get(i).map(|&b| b as u64),
                    3 => u16_at(data, i * 2).ok().map(u64::from),
                    4 => u32_at(data, i * 4).ok().map(u64::from),
                    16 => u64_at(data, i * 8).ok(),
                    _ => None,
                })
                .collect()
        };
        let floats = |tag: u16| -> Vec<f64> {
            let Some((12, n, data)) = tags.get(&tag) else { return Vec::new() };
            (0..*n as usize).filter_map(|i| u64_at(data, i * 8).ok().map(f64::from_bits)).collect()
        };
        let one = |tag: u16, default: u64| ints(tag).first().copied().unwrap_or(default);
        if one(259, 1) == 7 || one(284, 1) != 1 || !matches!(one(258, 8), 8 | 16) {
            return Err(bad("pixel format"));
        }
        // A mask (subfile type 4) is not a level of the image.
        if one(254, 0) & 4 != 0 {
            continue;
        }
        let level = Level {
            width: one(256, 0) as u32,
            height: one(257, 0) as u32,
            tile_width: one(322, 0) as u32,
            tile_height: one(323, 0) as u32,
            samples: one(277, 1) as u16,
            bytes: one(258, 8) as u16 / 8,
            compression: one(259, 1) as u16,
            predictor: one(317, 1) as u16,
            offsets: ints(324),
            lengths: ints(325),
        };
        if level.tile_width == 0 || level.tile_height == 0 || level.offsets.len() != level.lengths.len() {
            return Err(bad("no tiles"));
        }
        if levels.is_empty() {
            let keys = ints(34735);
            // GeoKeyDirectory: a header of four, then (key, location, count, value) each.
            for k in keys.chunks(4).skip(1) {
                if k.len() == 4 && k[0] == 3072 && k[1] == 0 {
                    epsg = k[3] as u32;
                }
            }
            tie = floats(33922).get(3..5).map(|s| (s[0], s[1]));
            scale = floats(33550).first().copied();
        }
        levels.push(level);
    }
    let ((west, north), pixel) = tie.zip(scale).ok_or_else(|| bad("no georeference"))?;
    if levels.is_empty() || epsg == 0 {
        return Err(bad("no image"));
    }
    Ok(Cog { levels, epsg, west, north, pixel })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little tiled GeoTIFF made up here (no real data), as GDAL writes a COG: the header first,
    /// a full level of 32 by 32 pixels in tiles of 16 and an overview of 16, three samples,
    /// deflate with the horizontal predictor, UTM zone 31 north, 10 m pixels. Gives the file and
    /// the full level's tiles as they should decode.
    pub(crate) fn sample() -> (Vec<u8>, Vec<Vec<u8>>) {
        use std::io::Write;
        let pixel = |level: u32, x: u32, y: u32| [(x * 7 + level) as u8, (y * 5) as u8, (x ^ y) as u8];
        let tile = |level: u32, tx: u32, ty: u32| -> Vec<u8> { (0..16).flat_map(|y| (0..16).flat_map(move |x| pixel(level, tx * 16 + x, ty * 16 + y))).collect() };
        let pack = |raw: &[u8]| -> Vec<u8> {
            let mut diff = raw.to_vec();
            for line in diff.chunks_mut(48) {
                for i in (3..48).rev() {
                    line[i] = line[i].wrapping_sub(line[i - 3]);
                }
            }
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(&diff).unwrap();
            z.finish().unwrap()
        };
        let full: Vec<Vec<u8>> = (0..4).map(|i| tile(0, i % 2, i / 2)).collect();
        let packed: Vec<Vec<u8>> = full.iter().map(|t| pack(t)).chain([pack(&tile(1, 0, 0))]).collect();

        // Two IFDs of plain entries (tag, type, count, value or offset) and their arrays after them.
        let mut out = b"II*\0".to_vec();
        out.extend(8u32.to_le_bytes());
        let ifd = |out: &mut Vec<u8>, width: u32, offsets: &[u32], lengths: &[u32], first: bool, next: u32, arrays: u32| {
            let mut entries: Vec<(u16, u16, u32, u32)> = vec![(256, 3, 1, width), (257, 3, 1, width), (258, 3, 1, 8), (259, 3, 1, 8), (277, 3, 1, 3), (284, 3, 1, 1), (317, 3, 1, 2), (322, 3, 1, 16), (323, 3, 1, 16)];
            entries.push((324, 4, offsets.len() as u32, if offsets.len() == 1 { offsets[0] } else { arrays }));
            entries.push((325, 4, lengths.len() as u32, if lengths.len() == 1 { lengths[0] } else { arrays + 16 }));
            if first {
                entries.push((33550, 12, 3, arrays + 32));
                entries.push((33922, 12, 6, arrays + 56));
                entries.push((34735, 3, 8, arrays + 104));
            } else {
                entries.insert(0, (254, 4, 1, 1));
            }
            out.extend((entries.len() as u16).to_le_bytes());
            for (tag, kind, n, v) in entries {
                out.extend(tag.to_le_bytes());
                out.extend(kind.to_le_bytes());
                out.extend(n.to_le_bytes());
                out.extend(v.to_le_bytes());
            }
            out.extend(next.to_le_bytes());
        };
        // Layout: IFD 0 (14 entries) at 8, its arrays, IFD 1 (12 entries), then the tiles.
        let ifd0_len = 2 + 14 * 12 + 4;
        let arrays = 8 + ifd0_len;
        let arrays_len = 16 + 16 + 24 + 48 + 16;
        let ifd1 = arrays + arrays_len;
        let data = ifd1 + 2 + 12 * 12 + 4;
        let mut offsets = Vec::new();
        let mut at = data;
        for p in &packed {
            offsets.push(at as u32);
            at += p.len();
        }
        let lengths: Vec<u32> = packed.iter().map(|p| p.len() as u32).collect();
        ifd(&mut out, 32, &offsets[..4], &lengths[..4], true, ifd1 as u32, arrays as u32);
        for v in offsets[..4].iter().chain(&lengths[..4]) {
            out.extend(v.to_le_bytes());
        }
        for v in [10.0f64, 10.0, 0.0, 0.0, 0.0, 0.0, 600_000.0, 4_500_000.0, 0.0] {
            out.extend(v.to_bits().to_le_bytes());
        }
        for v in [1u16, 1, 0, 1, 3072, 0, 1, 32631] {
            out.extend(v.to_le_bytes());
        }
        assert_eq!(out.len(), ifd1);
        ifd(&mut out, 16, &offsets[4..], &lengths[4..], false, 0, 0);
        assert_eq!(out.len(), data);
        for p in &packed {
            out.extend(p);
        }
        (out, full)
    }

    #[test]
    fn a_cog_header_and_its_tiles_are_read() {
        let (file, tiles) = sample();
        let cog = parse(&file[..file.len().min(4096)], &|o, l| Ok(file[o as usize..(o + l) as usize].to_vec())).unwrap();
        assert_eq!(cog.epsg, 32631);
        assert_eq!((cog.west, cog.north, cog.pixel), (600_000.0, 4_500_000.0, 10.0));
        assert_eq!(cog.levels.len(), 2);
        assert_eq!((cog.levels[0].width, cog.levels[0].tiles()), (32, (2, 2)));
        assert_eq!(cog.pixel_of(1), 20.0);
        for (i, want) in tiles.iter().enumerate() {
            let (o, l) = cog.levels[0].range(i as u32 % 2, i as u32 / 2).unwrap();
            let got = cog.levels[0].decode(&file[o as usize..(o + l) as usize]).unwrap();
            assert_eq!(&got, want, "tile {i}");
        }
        let (o, l) = cog.levels[1].range(0, 0).unwrap();
        assert_eq!(cog.levels[1].decode(&file[o as usize..(o + l) as usize]).unwrap().len(), 16 * 16 * 3);
    }
}
