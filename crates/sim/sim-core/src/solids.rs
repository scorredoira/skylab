//! What a real place has standing on its ground besides trees and the towns' houses: the buildings
//! of its maps, each its own outline (courtyards and all) raised from under the ground to its roof
//! (`roofs::Frame`), and the parts of its bridges (`bridges`). Found by a grid of squares of `CELL`
//! metres, each listing what reaches into it, so that asking whether a point is inside one looks at
//! the few things of one square: a vehicle asks for each of its points every step.

use std::collections::HashMap;

use crate::bridges::{self, Piece};
use crate::region::{Footprint, Region};
use crate::roofs::Frame;
use crate::scenery::ObstacleKind;

/// Side of the squares, metres.
pub const CELL: f64 = 16.0;
/// How far under the ground a building's walls reach where it stands on the ground: a slope
/// under it is never seen between its walls and the ground.
pub const SINK: f64 = 1.5;

/// A building, or a part of a bridge.
#[derive(Clone, Debug)]
enum Item {
    Building { index: u32, base: f32, frame: Frame },
    Part { bridge: u32, piece: Piece },
}

/// What a point is inside of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub kind: ObstacleKind,
    /// The middle of what was hit, north and east: the same for every point of it.
    pub n: f64,
    pub e: f64,
}

/// The solids of a region, found by squares.
#[derive(Clone, Debug, Default)]
pub struct Solids {
    items: Vec<Item>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// The highest any of them stands above the ground under it, metres.
    tallest: f64,
}

impl Solids {
    /// The solids of the region's buildings in `buildings` (indices into `region.buildings`, each
    /// standing on `base` of it) and of its bridges; `ground` is the height of the ground (or the
    /// bed of the water) at a point.
    pub fn new(region: &Region, buildings: &[u32], base: &dyn Fn(&Footprint) -> f64, ground: &dyn Fn(f64, f64) -> f64) -> Solids {
        let mut solids = Solids::default();
        for &i in buildings {
            let b = &region.buildings[i as usize];
            if b.ring.len() < 3 {
                continue;
            }
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for p in &b.ring {
                lo = [lo[0].min(p[0] as f64), lo[1].min(p[1] as f64)];
                hi = [hi[0].max(p[0] as f64), hi[1].max(p[1] as f64)];
            }
            solids.tallest = solids.tallest.max(b.height as f64);
            solids.add(Item::Building { index: i, base: base(b) as f32, frame: Frame::of(b) }, [lo[0], lo[1], hi[0], hi[1]]);
        }
        for (i, bridge) in region.bridges.iter().enumerate() {
            for part in bridges::parts(bridge, ground) {
                let bounds = part.piece.bounds();
                let (_, top) = part.piece.heights(bounds[0], bounds[1]);
                let under = ground((bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0).max(0.0);
                solids.tallest = solids.tallest.max(top - under);
                solids.add(Item::Part { bridge: i as u32, piece: part.piece }, bounds);
            }
        }
        solids
    }

    fn add(&mut self, item: Item, bounds: [f64; 4]) {
        let k = self.items.len() as u32;
        self.items.push(item);
        let cell = |v: f64| (v / CELL).floor() as i32;
        for x in cell(bounds[0])..=cell(bounds[2]) {
            for y in cell(bounds[1])..=cell(bounds[3]) {
                self.cells.entry((x, y)).or_default().push(k);
            }
        }
    }

    /// The highest any stands above the ground under it, metres (0 when there are none).
    pub fn tallest(&self) -> f64 {
        self.tallest
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn here(&self, n: f64, e: f64) -> impl Iterator<Item = &Item> {
        let key = ((n / CELL).floor() as i32, (e / CELL).floor() as i32);
        self.cells.get(&key).into_iter().flatten().map(|&k| &self.items[k as usize])
    }

    /// The bottom and top of an item over a point (north, east), if the point is over it.
    fn span(item: &Item, region: &Region, n: f64, e: f64) -> Option<(f64, f64)> {
        match item {
            Item::Building { index, base, frame } => {
                let b = &region.buildings[*index as usize];
                if !b.contains(n, e) {
                    return None;
                }
                let base = *base as f64;
                let bottom = if b.floor > 0.0 { base + b.floor as f64 } else { base - SINK };
                Some((bottom, base + frame.height(&b.ring, n, e)))
            }
            Item::Part { piece: piece @ Piece::Prism { ring, .. }, .. } => crate::region::inside_ring(ring, n, e).then(|| piece.heights(n, e)),
            Item::Part { .. } => None,
        }
    }

    /// What a point (north, east, height above the sea) is inside of, if anything.
    pub fn hit(&self, region: &Region, n: f64, e: f64, h: f64) -> Option<Hit> {
        for item in self.here(n, e) {
            let inside = match item {
                Item::Part { piece: piece @ Piece::Bar { .. }, .. } => piece.contains(n, e, h),
                _ => Solids::span(item, region, n, e).is_some_and(|(bottom, top)| h > bottom && h < top),
            };
            if !inside {
                continue;
            }
            return Some(match item {
                Item::Building { index, .. } => {
                    let (cn, ce) = region.buildings[*index as usize].centre();
                    Hit { kind: ObstacleKind::Building, n: cn, e: ce }
                }
                Item::Part { bridge, .. } => {
                    let (p, _) = region.bridges[*bridge as usize].at(0.0);
                    Hit { kind: ObstacleKind::Bridge, n: p[0], e: p[1] }
                }
            });
        }
        None
    }

    /// The highest top of what stands over a point (north, east) no higher than `h`: what
    /// something falling there at `h` comes to rest on, if not the ground.
    pub fn top_under(&self, region: &Region, n: f64, e: f64, h: f64) -> Option<f64> {
        self.here(n, e).filter_map(|item| Solids::span(item, region, n, e)).map(|(_, top)| top).filter(|&top| top <= h).reduce(f64::max)
    }

    /// The surface of the solid nearest a point (north, east, height above the sea) within `reach`
    /// metres: how far outside it the point is (negative inside), the nearest point of the surface
    /// and the way out of the solid there (a unit vector, north, east, up), and what it is. Found
    /// by probing along 26 directions and bisecting where a probe crosses the surface: the solids
    /// answer only whether a point is inside (`hit`), and this is what a quad's propellers and frame
    /// need to bounce off a wall, a roof or a bridge's deck as off a trunk (`Multirotor::hit`).
    pub fn surface(&self, region: &Region, n: f64, e: f64, h: f64, reach: f64) -> Option<(f64, [f64; 3], [f64; 3], ObstacleKind)> {
        if self.items.is_empty() {
            return None;
        }
        let inside = |p: [f64; 3]| self.hit(region, p[0], p[1], p[2]);
        let at = |d: [f64; 3], s: f64| [n + d[0] * s, e + d[1] * s, h + d[2] * s];
        let mut dirs = Vec::with_capacity(26);
        for x in -1i32..=1 {
            for y in -1i32..=1 {
                for z in -1i32..=1 {
                    if (x, y, z) != (0, 0, 0) {
                        let l = ((x * x + y * y + z * z) as f64).sqrt();
                        dirs.push([x as f64 / l, y as f64 / l, z as f64 / l]);
                    }
                }
            }
        }
        // Where along `d` between `a` (on the `from` side) and `b` (on the other) the surface is.
        let bisect = |d: [f64; 3], mut a: f64, mut b: f64, from: bool| {
            for _ in 0..12 {
                let m = (a + b) / 2.0;
                if inside(at(d, m)).is_some() == from {
                    a = m;
                } else {
                    b = m;
                }
            }
            (a + b) / 2.0
        };
        if let Some(hit) = inside([n, e, h]) {
            // Inside: the shortest way out, looked for ever farther.
            let mut s = 0.02;
            while s <= 4.0 {
                let best = dirs.iter().filter(|&&d| inside(at(d, s)).is_none()).map(|&d| (bisect(d, 0.0, s, true), d)).min_by(|x, y| x.0.total_cmp(&y.0));
                if let Some((depth, d)) = best {
                    return Some((-depth, at(d, depth), d, hit.kind));
                }
                s *= 2.0;
            }
            return Some((-4.0, [n, e, h], [0.0, 0.0, 1.0], hit.kind));
        }
        // Outside: the nearest surface within reach, the way out pointing back at the point.
        dirs.iter()
            .filter_map(|&d| inside(at(d, reach)).map(|hit| (bisect(d, 0.0, reach, false), d, hit.kind)))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(gap, d, kind)| (gap, at(d, gap), [-d[0], -d[1], -d[2]], kind))
    }

    /// The highest top of anything over a point (north, east).
    pub fn top(&self, region: &Region, n: f64, e: f64) -> Option<f64> {
        self.here(n, e).filter_map(|item| Solids::span(item, region, n, e)).map(|(_, top)| top).reduce(f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{Bridge, Grid, Roof};

    fn region(buildings: Vec<Footprint>, bridges: Vec<Bridge>) -> Region {
        Region {
            name: "test".into(),
            latitude: 0.0,
            longitude: 0.0,
            heights: vec![Grid::centred(0.0, 0.0, 2000.0, 10.0, 0.0f32)],
            cover: Grid::centred(0.0, 0.0, 2000.0, 10.0, 0u8),
            airports: Vec::new(),
            roads: Vec::new(),
            towns: Vec::new(),
            buildings,
            bridges,
        }
    }

    #[test]
    fn an_l_shaped_building_keeps_its_empty_corner_and_its_courtyard_open() {
        // An L: 40 m along north and east, 12 m wide arms; the corner (20, 20) is open ground.
        let l = Footprint::new(vec![[0.0, 0.0], [40.0, 0.0], [40.0, 12.0], [12.0, 12.0], [12.0, 40.0], [0.0, 40.0]], 20.0, 0.0, Roof::Flat);
        // A block with a courtyard.
        let mut block = Footprint::new(vec![[100.0, 0.0], [160.0, 0.0], [160.0, 60.0], [100.0, 60.0]], 25.0, 0.0, Roof::Flat);
        block.holes = vec![vec![[115.0, 15.0], [145.0, 15.0], [145.0, 45.0], [115.0, 45.0]]];
        let r = region(vec![l, block], Vec::new());
        let s = Solids::new(&r, &[0, 1], &|_| 5.0, &|_, _| 5.0);
        assert!(s.hit(&r, 5.0, 30.0, 10.0).is_some(), "in the arm");
        assert!(s.hit(&r, 25.0, 25.0, 10.0).is_none(), "the box round it would have it, the L does not");
        assert!(s.hit(&r, 5.0, 30.0, 26.0).is_none(), "above the roof");
        assert!(s.hit(&r, 130.0, 30.0, 10.0).is_none(), "in the courtyard");
        assert!(s.hit(&r, 105.0, 30.0, 10.0).is_some_and(|h| h.kind == ObstacleKind::Building));
        assert_eq!(s.top(&r, 105.0, 30.0), Some(30.0));
        assert_eq!(s.top_under(&r, 105.0, 30.0, 29.0), None);
        assert_eq!(s.top_under(&r, 105.0, 30.0, 40.0), Some(30.0));
        assert!((s.tallest() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn a_pitched_roof_is_hit_under_its_slope_and_not_above_it() {
        let house = Footprint::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]], 10.0, 0.0, Roof::Gabled);
        let r = region(vec![house], Vec::new());
        let s = Solids::new(&r, &[0], &|_| 0.0, &|_, _| 0.0);
        // Under the ridge it is 10 m; near the eaves, much lower.
        assert!(s.hit(&r, 10.0, 5.0, 9.5).is_some());
        assert!(s.hit(&r, 10.0, 0.5, 9.5).is_none());
        // A part raised on others has nothing under it.
        let mut raised = Footprint::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]], 30.0, 20.0, Roof::Flat);
        raised.levels = 3;
        let r = region(vec![raised], Vec::new());
        let s = Solids::new(&r, &[0], &|_| 0.0, &|_, _| 0.0);
        assert!(s.hit(&r, 10.0, 5.0, 10.0).is_none() && s.hit(&r, 10.0, 5.0, 25.0).is_some());
    }

    #[test]
    fn a_bridge_is_hit_on_its_deck_and_piers_and_flown_under() {
        let bridge = Bridge { line: vec![[0.0, -100.0, 20.0], [0.0, 100.0, 20.0]], width: 12.0, ..Bridge::default() };
        let r = region(Vec::new(), vec![bridge]);
        let s = Solids::new(&r, &[], &|_| 0.0, &|_, _| 0.0);
        assert!(s.hit(&r, 0.0, 10.0, 19.5).is_some_and(|h| h.kind == ObstacleKind::Bridge));
        assert!(s.hit(&r, 0.0, 10.0, 21.0).is_none());
        assert!(s.hit(&r, 0.0, 15.0, 10.0).is_none(), "under it between the piers");
        assert!(s.hit(&r, 8.0, 0.0, 19.5).is_none(), "beside it");
        assert_eq!(s.top_under(&r, 0.0, 10.0, 30.0), Some(20.0));
    }
}
