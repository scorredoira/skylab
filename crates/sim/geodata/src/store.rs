//! A built region kept on disk, so that a place already flown loads in a moment and offline: plain
//! little-endian numbers after a magic word and a version. A file of another version is not read
//! (the region is built again from the tiles, which stay in the cache).

use sim_core::lights::Class;
use sim_core::{Airport, Bridge, Carries, Detail, Footprint, Grid, Material, Region, Road, Roof, Structure, Town, Use};

use crate::Error;

const MAGIC: &[u8; 4] = b"FLRG";
/// Changes whenever the file or what `build` makes of the data changes.
pub const VERSION: u32 = 21;

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn text(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn ring(&mut self, ring: &[[f32; 2]]) {
        self.u32(ring.len() as u32);
        for v in ring.iter().flatten() {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn grid_head<T>(&mut self, g: &Grid<T>) {
        self.f64(g.south);
        self.f64(g.west);
        self.f64(g.step);
        self.u32(g.rows as u32);
        self.u32(g.cols as u32);
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| Error::Data("saved region is incomplete".into()))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap_or_default()))
    }
    fn floats(&mut self, count: usize) -> Result<Vec<f32>, Error> {
        if count > 1 << 24 {
            return Err(Error::Data("a list of impossible size".into()));
        }
        Ok(self.take(count * 4)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }

    fn ring(&mut self) -> Result<Vec<[f32; 2]>, Error> {
        let corners = self.u32()? as usize;
        Ok(self.floats(corners * 2)?.as_chunks::<2>().0.to_vec())
    }

    fn colour(&mut self) -> Result<Option<[u8; 3]>, Error> {
        let c = self.take(4)?;
        Ok((c[0] != 0).then(|| [c[1], c[2], c[3]]))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap_or_default()))
    }
    fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap_or_default()))
    }
    fn text(&mut self) -> Result<String, Error> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    /// A grid's place and size, and how many values it has.
    fn grid_head(&mut self) -> Result<(f64, f64, f64, usize, usize), Error> {
        let (south, west, step) = (self.f64()?, self.f64()?, self.f64()?);
        let (rows, cols) = (self.u32()? as usize, self.u32()? as usize);
        if rows < 2 || cols < 2 || rows * cols > 200_000_000 {
            return Err(Error::Data("grid of impossible size".into()));
        }
        Ok((south, west, step, rows, cols))
    }
}

fn class_byte(c: Option<Class>) -> u32 {
    match c {
        None => 0,
        Some(Class::Visual) => 1,
        Some(Class::NonPrecision) => 2,
        Some(Class::PrecisionOne) => 3,
        Some(Class::PrecisionTwo) => 4,
    }
}

fn class_of(b: u32) -> Option<Class> {
    match b {
        1 => Some(Class::Visual),
        2 => Some(Class::NonPrecision),
        3 => Some(Class::PrecisionOne),
        4 => Some(Class::PrecisionTwo),
        _ => None,
    }
}

/// The bytes of a region.
pub fn encode(r: &Region) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(r.heights.iter().map(|g| g.data.len() * 4).sum::<usize>() + r.cover.data.len() + 4096));
    w.0.extend_from_slice(MAGIC);
    w.u32(VERSION);
    w.text(&r.name);
    w.f64(r.latitude);
    w.f64(r.longitude);
    w.u32(r.heights.len() as u32);
    for g in &r.heights {
        w.grid_head(g);
        for v in &g.data {
            w.0.extend_from_slice(&v.to_le_bytes());
        }
    }
    w.grid_head(&r.cover);
    w.0.extend_from_slice(&r.cover.data);
    w.u32(r.airports.len() as u32);
    for a in &r.airports {
        w.u64(a.id);
        for v in [a.n, a.e, a.heading, a.elevation, a.length, a.width, a.detail.apron] {
            w.f64(v);
        }
        w.u32(a.detail.paved as u32);
        w.u32(class_byte(a.detail.lighting));
    }
    w.u32(r.roads.len() as u32);
    for road in &r.roads {
        for v in [road.n0, road.e0, road.n1, road.e1, road.width] {
            w.f64(v);
        }
    }
    w.u32(r.towns.len() as u32);
    for t in &r.towns {
        for v in [t.n, t.e, t.radius, t.grid] {
            w.f64(v);
        }
    }
    w.u32(r.buildings.len() as u32);
    for b in &r.buildings {
        w.ring(&b.ring);
        w.u32(b.holes.len() as u32);
        for hole in &b.holes {
            w.ring(hole);
        }
        for v in [b.height, b.floor, b.roof_height, b.direction.unwrap_or(f32::NAN)] {
            w.0.extend_from_slice(&v.to_le_bytes());
        }
        let colour = |c: Option<[u8; 3]>| c.map_or([0, 0, 0, 0], |c| [1, c[0], c[1], c[2]]);
        w.0.extend_from_slice(&[b.roof as u8, b.levels, b.kind as u8, b.material as u8, b.measured as u8]);
        w.0.extend_from_slice(&colour(b.wall_colour));
        w.0.extend_from_slice(&colour(b.roof_colour));
    }
    w.u32(r.bridges.len() as u32);
    for b in &r.bridges {
        w.u32(b.line.len() as u32);
        for v in b.line.iter().flatten() {
            w.0.extend_from_slice(&v.to_le_bytes());
        }
        w.u32(b.towers.len() as u32);
        for v in b.towers.iter().chain([&b.width, &b.tower_height]) {
            w.0.extend_from_slice(&v.to_le_bytes());
        }
        let colour = b.colour.map_or([0, 0, 0, 0], |c| [1, c[0], c[1], c[2]]);
        w.0.extend_from_slice(&[b.carries as u8, b.structure as u8, b.landmark as u8, b.masonry as u8]);
        w.0.extend_from_slice(&colour);
    }
    w.0
}

/// Whether the first bytes of a file (eight are enough) are those of a region of this version.
pub fn is_current(head: &[u8]) -> bool {
    head.len() >= 8 && head[..4] == *MAGIC && head[4..8] == VERSION.to_le_bytes()
}

/// A region from its bytes; `Ok(None)` for a file of another version.
pub fn decode(bytes: &[u8]) -> Result<Option<Region>, Error> {
    let mut r = Reader { b: bytes, p: 0 };
    if r.take(4)? != MAGIC {
        return Err(Error::Data("not a region".into()));
    }
    if r.u32()? != VERSION {
        return Ok(None);
    }
    let name = r.text()?;
    let (latitude, longitude) = (r.f64()?, r.f64()?);
    let mut heights = Vec::new();
    for _ in 0..r.u32()?.min(16) {
        let (south, west, step, rows, cols) = r.grid_head()?;
        let data = r.take(rows * cols * 4)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        heights.push(Grid { south, west, step, rows, cols, data });
    }
    let (south, west, step, rows, cols) = r.grid_head()?;
    let cover = Grid { south, west, step, rows, cols, data: r.take(rows * cols)?.to_vec() };
    let mut airports = Vec::new();
    for _ in 0..r.u32()? {
        let id = r.u64()?;
        let mut v = [0.0; 7];
        for x in &mut v {
            *x = r.f64()?;
        }
        let (paved, lighting) = (r.u32()? != 0, class_of(r.u32()?));
        airports.push(Airport { id, n: v[0], e: v[1], heading: v[2], elevation: v[3], length: v[4], width: v[5], detail: Detail { paved, lighting, apron: v[6] } });
    }
    let mut roads = Vec::new();
    for _ in 0..r.u32()? {
        let (n0, e0, n1, e1, width) = (r.f64()?, r.f64()?, r.f64()?, r.f64()?, r.f64()?);
        roads.push(Road { n0, e0, n1, e1, width });
    }
    let mut towns = Vec::new();
    for _ in 0..r.u32()? {
        let (n, e, radius, grid) = (r.f64()?, r.f64()?, r.f64()?, r.f64()?);
        towns.push(Town { n, e, radius, grid });
    }
    let mut buildings = Vec::new();
    for _ in 0..r.u32()? {
        let ring = r.ring()?;
        let holes = (0..r.u32()?.min(1 << 16)).map(|_| r.ring()).collect::<Result<Vec<_>, _>>()?;
        let v = r.floats(4)?;
        let k = r.take(5)?.to_vec();
        let (wall_colour, roof_colour) = (r.colour()?, r.colour()?);
        buildings.push(Footprint {
            ring,
            holes,
            height: v[0],
            floor: v[1],
            roof_height: v[2],
            direction: v[3].is_finite().then_some(v[3]),
            roof: Roof::from_byte(k[0]),
            levels: k[1],
            kind: Use::from_byte(k[2]),
            material: Material::from_byte(k[3]),
            measured: k[4] != 0,
            wall_colour,
            roof_colour,
        });
    }
    let mut bridges = Vec::new();
    for _ in 0..r.u32()? {
        let points = r.u32()? as usize;
        let line = r.floats(points * 3)?.as_chunks::<3>().0.to_vec();
        let towers = r.u32()? as usize;
        let mut v = r.floats(towers + 2)?;
        let (width, tower_height) = (v[towers], v[towers + 1]);
        v.truncate(towers);
        let k = r.take(4)?.to_vec();
        let colour = r.colour()?;
        bridges.push(Bridge { line, width, carries: Carries::from_byte(k[0]), structure: Structure::from_byte(k[1]), towers: v, tower_height, colour, landmark: k[2] != 0, masonry: k[3] != 0 });
    }
    Ok(Some(Region { name, latitude, longitude, heights, cover, airports, roads, towns, buildings, bridges }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_reads_back_as_it_was_written() {
        let mut fine = Grid::new(-100.0, -200.0, 10.0, 3, 4, 1.5f32);
        fine.set(2, 3, -7.25);
        let region = Region {
            name: "Menorca".into(),
            latitude: 0.7,
            longitude: 0.07,
            heights: vec![fine, Grid::new(-1000.0, -1000.0, 100.0, 2, 2, 3.0)],
            cover: Grid::new(-100.0, -200.0, 20.0, 2, 3, 4u8),
            airports: vec![Airport { id: 9, n: 1.0, e: 2.0, heading: 0.5, elevation: 91.0, length: 2100.0, width: 45.0, detail: Detail { paved: true, lighting: Some(Class::PrecisionOne), apron: -1.0 } }],
            roads: vec![Road { n0: 1.0, e0: 2.0, n1: 3.0, e1: 4.0, width: 9.0 }],
            towns: vec![Town { n: 5.0, e: 6.0, radius: 300.0, grid: 0.25 }],
            buildings: vec![
                Footprint {
                    holes: vec![vec![[4.0, 4.0], [5.0, 4.0], [5.0, 5.0]]],
                    roof_height: 2.5,
                    direction: Some(0.25),
                    levels: 4,
                    kind: Use::Religious,
                    material: Material::Stone,
                    wall_colour: Some([200, 190, 180]),
                    measured: false,
                    ..Footprint::new(vec![[1.0, 2.0], [11.0, 2.5], [10.5, 9.0]], 12.5, 3.0, Roof::Hipped)
                },
                Footprint::new(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], 3.0, 0.0, Roof::Flat),
            ],
            bridges: vec![Bridge {
                line: vec![[0.0, 0.0, 10.0], [100.0, 5.0, 12.0], [200.0, 0.0, 9.0]],
                width: 12.5,
                carries: Carries::Rail,
                structure: Structure::Suspension,
                towers: vec![40.0, 160.0],
                tower_height: 30.0,
                colour: Some([192, 54, 44]),
                landmark: true,
                masonry: true,
            }],
        };
        let bytes = encode(&region);
        assert_eq!(decode(&bytes).unwrap(), Some(region));
        assert!(decode(&bytes[..bytes.len() - 3]).is_err());
        let mut other = bytes.clone();
        other[4] ^= 0xff;
        assert_eq!(decode(&other).unwrap(), None);
    }
}
