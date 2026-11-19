//! PMTiles, version 3: a whole tiled map in one file, read a tile at a time by HTTP range
//! requests. The format is Protomaps' open specification (github.com/protomaps/PMTiles, `spec/v3`):
//! a header of 127 bytes, a root directory inside the first 16 KB, leaf directories, then the
//! tiles. A directory lists runs of tiles by their number along a Hilbert curve per zoom
//! (`tile_id`), each with where its bytes are; an entry with no run points to a leaf directory.
//! Directories are gzipped (as the files read here have them); tiles are left as they come.
//!
//! The parts read are kept in the cache like everything else (`Fetcher::cached_range`): the head
//! and the leaf directories under the name of the file, so another release of it is read anew.

use std::io::Read;

use crate::fetch::Fetcher;
use crate::geo::TileId;
use crate::Error;

/// The bytes the root directory is always within (the header included).
const HEAD: u64 = 16_384;

/// What the header says of where things are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Header {
    pub root: (u64, u64),
    pub leaves: u64,
    pub tiles: u64,
    pub max_zoom: u8,
    /// How directories are compressed: 1 none, 2 gzip.
    pub compression: u8,
}

impl Header {
    pub fn parse(b: &[u8]) -> Result<Header, Error> {
        if b.len() < 127 || &b[..7] != b"PMTiles" || b[7] != 3 {
            return Err(Error::Data("PMTiles header".into()));
        }
        let u = |at: usize| u64::from_le_bytes(b[at..at + 8].try_into().unwrap_or([0; 8]));
        Ok(Header { root: (u(8), u(16)), leaves: u(40), tiles: u(56), max_zoom: b[101], compression: b[97] })
    }
}

/// One entry of a directory: tiles `id` to `id + run - 1` all are the bytes at `offset` (from the
/// start of the tiles), or, with `run` 0, a leaf directory at `offset` from the start of the leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub offset: u64,
    pub length: u32,
    pub run: u32,
}

/// The number of a tile: the tiles of the zooms below it, then its place along the Hilbert curve
/// over its zoom.
pub fn tile_id(z: u8, x: u32, y: u32) -> u64 {
    let below = ((1u64 << (2 * z as u32)) - 1) / 3;
    let n = 1u64 << z;
    let (mut x, mut y) = (x as u64, y as u64);
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u64::from(x & s != 0);
        let ry = u64::from(y & s != 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = n.wrapping_sub(1).wrapping_sub(x);
                y = n.wrapping_sub(1).wrapping_sub(y);
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    below + d
}

fn varint(b: &[u8], p: &mut usize) -> Result<u64, Error> {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let c = *b.get(*p).ok_or_else(|| Error::Data("PMTiles directory".into()))?;
        *p += 1;
        v |= ((c & 0x7f) as u64) << shift;
        if c & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
        if shift > 63 {
            return Err(Error::Data("PMTiles directory".into()));
        }
    }
}

/// The entries of a directory, its bytes as they are in the file.
pub fn directory(bytes: &[u8], compression: u8) -> Result<Vec<Entry>, Error> {
    let mut raw = Vec::new();
    let b = match compression {
        1 => bytes,
        2 => {
            flate2::read::GzDecoder::new(bytes).read_to_end(&mut raw).map_err(|_| Error::Data("PMTiles directory".into()))?;
            &raw[..]
        }
        _ => return Err(Error::Data("PMTiles compression".into())),
    };
    let mut p = 0;
    let count = varint(b, &mut p)? as usize;
    if count > 1 << 24 {
        return Err(Error::Data("PMTiles directory".into()));
    }
    let mut entries = vec![Entry { id: 0, offset: 0, length: 0, run: 0 }; count];
    let mut last = 0;
    for e in entries.iter_mut() {
        last += varint(b, &mut p)?;
        e.id = last;
    }
    for e in entries.iter_mut() {
        e.run = varint(b, &mut p)? as u32;
    }
    for e in entries.iter_mut() {
        e.length = varint(b, &mut p)? as u32;
    }
    for i in 0..count {
        let v = varint(b, &mut p)?;
        // 0: right after the one before.
        entries[i].offset = if v == 0 && i > 0 { entries[i - 1].offset + entries[i - 1].length as u64 } else { v.saturating_sub(1) };
    }
    Ok(entries)
}

/// The entry that holds tile `id`, if any: the last whose first tile is not past it.
pub fn find(entries: &[Entry], id: u64) -> Option<Entry> {
    let i = entries.partition_point(|e| e.id <= id).checked_sub(1)?;
    let e = entries[i];
    (e.run == 0 || id < e.id + e.run as u64).then_some(e)
}

/// A PMTiles file on a server, read through the cache.
pub struct Archive<'a> {
    fetcher: &'a Fetcher,
    url: String,
    /// The cache's name for the file's parts.
    name: String,
    header: Header,
    root: Vec<Entry>,
}

impl<'a> Archive<'a> {
    /// Reads the head of the file at `url`, kept as `<name>/head`.
    pub fn open(fetcher: &'a Fetcher, url: &str, name: &str) -> Result<Archive<'a>, Error> {
        let head = fetcher.cached_range(&format!("{name}/head"), url, 0, HEAD)?;
        let header = Header::parse(&head)?;
        let (at, length) = header.root;
        let bytes = head.get(at as usize..(at + length) as usize).ok_or_else(|| Error::Data("PMTiles root directory".into()))?;
        let root = directory(bytes, header.compression)?;
        Ok(Archive { fetcher, url: url.to_string(), name: name.to_string(), header, root })
    }

    pub fn header(&self) -> Header {
        self.header
    }

    /// Where the bytes of a tile are in the file, as (offset, length); `None` if it has none (no
    /// feature there). Leaf directories are read on the way, kept as `<name>/leaf/<offset>`.
    pub fn locate(&self, tile: TileId) -> Result<Option<(u64, u64)>, Error> {
        let id = tile_id(tile.z, tile.x, tile.y);
        let mut entries = std::borrow::Cow::Borrowed(&self.root);
        for _ in 0..4 {
            let Some(e) = find(&entries, id) else { return Ok(None) };
            if e.run > 0 {
                return Ok(Some((self.header.tiles + e.offset, e.length as u64)));
            }
            let at = self.header.leaves + e.offset;
            let bytes = self.fetcher.cached_range(&format!("{}/leaf/{at}", self.name), &self.url, at, e.length as u64)?;
            entries = std::borrow::Cow::Owned(directory(&bytes, self.header.compression)?);
        }
        Err(Error::Data("PMTiles directories nested too deep".into()))
    }

    /// The bytes of a tile as stored (gzipped, for the files read here), or `None`.
    pub fn tile(&self, tile: TileId, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match self.locate(tile)? {
            Some((at, length)) => self.fetcher.cached_range(key, &self.url, at, length).map(Some),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_are_numbered_along_the_hilbert_curve_zoom_after_zoom() {
        // The specification's own examples.
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(2, 0, 0), 5);
        assert_eq!(tile_id(12, 3423, 1763), 19078479);
        assert_eq!(tile_id(20, 0, 0), 366503875925);
    }

    #[test]
    fn a_directory_reads_its_runs_and_finds_tiles() {
        // Three entries: tiles 5..8 at 0 (4 bytes), tile 10 right after it (no offset), a leaf at 100.
        let mut b = vec![3u8, 5, 5, 7, 4, 1, 0, 4, 6, 9, 1, 0, 101];
        let entries = directory(&b, 1).unwrap();
        assert_eq!(entries[0], Entry { id: 5, offset: 0, length: 4, run: 4 });
        assert_eq!(entries[1], Entry { id: 10, offset: 4, length: 6, run: 1 });
        assert_eq!(entries[2], Entry { id: 17, offset: 100, length: 9, run: 0 });
        assert_eq!(find(&entries, 7), Some(entries[0]));
        assert_eq!(find(&entries, 9), None);
        assert_eq!(find(&entries, 4), None);
        assert_eq!(find(&entries, 900), Some(entries[2]));
        b.truncate(5);
        assert!(directory(&b, 1).is_err());
    }
}
