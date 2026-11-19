//! The bridges of a region: OpenStreetMap's ways tagged `bridge=*` as Overture has them
//! (`overture::read_bridges`), each given a deck that runs smoothly from one end to the other high
//! enough over what it crosses (`profile`), and the famous ones made by hand from their public
//! dimensions instead (`LANDMARKS`).
//!
//! The ground the ways cross is the region's own heights (the sea's bed below 0, its surface 0;
//! rivers and lakes at their surface): the bridges never change it, nor the water under them.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use sim_core::{Bridge, Carries, Cover, Region, Structure};

use crate::build::{fetch_all, BUILDINGS_HALF};
use crate::fetch::Fetcher;
use crate::geo::{tiles_over, Projection, TileId};
use crate::overture::{self, BridgePiece, Release, Tiles};
use crate::Error;

/// A bridge made by hand from its public dimensions, in the place of what the maps draw of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landmark {
    pub name: &'static str,
    /// Its item in Wikidata, as the maps' ways of it are tagged: those are left out.
    pub wikidata: &'static str,
    /// The middle of its deck from one end to the other: latitude, longitude (degrees) and the
    /// height of the deck's top above the sea, metres; a negative height is "on the ground there".
    /// A suspension bridge's cables come down to its ends (its anchorages).
    pub line: &'static [(f64, f64, f32)],
    /// The viaducts that take its road on from its ends to the ground, as plain bridges on piers.
    pub approaches: &'static [&'static [(f64, f64, f32)]],
    /// Where its towers stand on the line (latitude, longitude).
    pub towers: &'static [(f64, f64)],
    /// How high the towers rise above the deck, metres.
    pub tower_height: f32,
    pub width: f32,
    pub structure: Structure,
    pub carries: Carries,
    /// sRGB.
    pub colour: [u8; 3],
    /// Stone towers (`Bridge::masonry`).
    pub masonry: bool,
}

/// The bridges made by hand. More are added as rows of this table.
///
/// The Golden Gate Bridge (the Golden Gate Bridge, Highway and Transportation District's published
/// figures, as Wikipedia gives them): a suspension bridge of 1280 m between its towers with side
/// spans of 343 m; its towers 227 m above the water and 152 m above the roadway, so the roadway is
/// some 75 m up at the towers; 67 m of clearance under the deck at high water in the middle of the
/// span; a deck 27.4 m wide (six lanes and two walkways) stiffened by a truss 7.6 m deep; painted
/// International Orange (#C0362C on screens; drawn a little more orange, as it looks in the sun,
/// #C84826). Where its towers and anchorages stand is where OpenStreetMap
/// draws them (its `bridge:support` outlines, as Overture has them, November 2026): the south tower
/// at 37.81402 N 122.47789 W, the north tower at 37.82551 N 122.47923 W (1281 m apart), the south
/// anchorage at Fort Point, the north one under the Marin end; its approach viaducts follow
/// OpenStreetMap's way of the road to where it meets the ground.
///
/// The Brooklyn Bridge (New York City Department of Transportation's figures, as Wikipedia gives
/// them): a suspension bridge of 486.3 m between its towers with side spans of 283.5 m, 1834 m long
/// with its approaches; its towers of granite and limestone 84.3 m above high water, each pierced
/// by two pointed arches over the roadway; 38.7 m of clearance at high water in the middle of the
/// span, where the cambered roadway is highest; a deck 26 m wide; its steel painted a pale tan.
/// The towers stand where OpenStreetMap draws them (40.70415 N 73.99431 W, 40.70733 N 73.99829 W,
/// 487 m apart); the anchorages 283.5 m beyond them along the bridge.
pub const LANDMARKS: &[Landmark] = &[Landmark {
    name: "Golden Gate Bridge",
    wikidata: "Q44440",
    line: &[
        (37.809400, -122.477300, 70.0),
        (37.810950, -122.477520, 72.5),
        (37.814020, -122.477890, 75.0),
        (37.819765, -122.478560, 76.5),
        (37.825510, -122.479230, 75.0),
        (37.829000, -122.479650, 70.0),
    ],
    approaches: &[
        &[(37.809400, -122.477300, 70.0), (37.808900, -122.477100, 68.0), (37.808284, -122.476777, 66.0), (37.807900, -122.476450, -1.0)],
        &[(37.829000, -122.479650, 70.0), (37.830740, -122.479850, 69.0), (37.831650, -122.480250, 68.0), (37.832300, -122.480800, -1.0)],
    ],
    towers: &[(37.814020, -122.477890), (37.825510, -122.479230)],
    tower_height: 152.0,
    width: 27.4,
    structure: Structure::Suspension,
    carries: Carries::Road,
    colour: [200, 72, 38],
    masonry: false,
}, Landmark {
    name: "Brooklyn Bridge",
    wikidata: "Q125006",
    line: &[
        (40.702300, -73.991990, 33.0),
        (40.704150, -73.994310, 38.0),
        (40.705740, -73.996300, 44.0),
        (40.707330, -73.998290, 38.0),
        (40.709180, -74.000610, 33.0),
    ],
    approaches: &[
        &[(40.702300, -73.991990, 33.0), (40.701600, -73.991250, 22.0), (40.701000, -73.990640, -1.0)],
        &[(40.709180, -74.000610, 33.0), (40.710080, -74.001720, 24.0), (40.711000, -74.002880, 14.0), (40.711560, -74.003580, -1.0)],
    ],
    towers: &[(40.704150, -73.994310), (40.707330, -73.998290)],
    tower_height: 46.3,
    width: 26.0,
    structure: Structure::Suspension,
    carries: Carries::Road,
    colour: [196, 186, 160],
    masonry: true,
}];

/// The bridges round the origin: those of the maps in Overture's tiles of zoom 13 over the tiles
/// of OpenFreeMap (`with_bridges`, zoom 12) that have any, within `BUILDINGS_HALF`; and the
/// landmarks there in place of theirs.
pub fn of_region(projection: &Projection, centre: (f64, f64), fetcher: &Fetcher, release: &OnceLock<Result<Release, Error>>, with_bridges: &[TileId], region: &Region, report: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<Bridge>, Error> {
    let near: HashSet<TileId> = tiles_over(projection, centre, BUILDINGS_HALF, overture::BRIDGES_ZOOM).into_iter().collect();
    let mut tiles: Vec<TileId> = Vec::new();
    for id in with_bridges {
        let shift = overture::BRIDGES_ZOOM.saturating_sub(id.z) as u32;
        for dx in 0..1u32 << shift {
            for dy in 0..1u32 << shift {
                let child = TileId { z: overture::BRIDGES_ZOOM, x: (id.x << shift) + dx, y: (id.y << shift) + dy };
                if near.contains(&child) && !tiles.contains(&child) {
                    tiles.push(child);
                }
            }
        }
    }
    let source = Tiles::new(fetcher, "base", release);
    let done = AtomicUsize::new(0);
    let read = fetch_all(&tiles, |id| {
        let found = overture::read_bridges(&source.get(id)?, id, projection);
        report(done.fetch_add(1, Ordering::Relaxed) + 1, tiles.len());
        Ok(found)
    })?;
    let ways = overture::join(read.into_iter().flat_map(|(_, w)| w).collect());
    let mut out = Vec::new();
    // Each landmark's lines (its own and its approaches'), its item and its name.
    let mut corridors: Vec<(Vec<Vec<[f64; 2]>>, &str, &str)> = Vec::new();
    for l in LANDMARKS {
        let line: Vec<[f64; 2]> = l.line.iter().map(|&(lat, lon, _)| {
            let (n, e) = projection.to_world(lat, lon);
            [n, e]
        }).collect();
        let inside = |p: &[f64; 2]| (p[0] - centre.0).abs().max((p[1] - centre.1).abs()) < BUILDINGS_HALF;
        if line.iter().any(inside) {
            out.push(landmark(l, projection, region));
            let mut lines = vec![line];
            for approach in l.approaches {
                let mut b = landmark(&Landmark { line: approach, towers: &[], structure: Structure::Beam, ..*l }, projection, region);
                b.tower_height = 0.0;
                lines.push(b.line.iter().map(|p| [p[0] as f64, p[1] as f64]).collect());
                out.push(b);
            }
            corridors.push((lines, l.wikidata, l.name));
        }
    }
    for way in ways {
        // The maps' ways of a landmark: tagged with it, named after it (its walkways), or lying
        // along it.
        let theirs = corridors.iter().any(|(lines, wikidata, name)| {
            way.wikidata.as_deref() == Some(*wikidata)
                || way.name.as_deref().is_some_and(|n| n.to_lowercase().contains(&name.to_lowercase()))
                || way.points.iter().all(|p| lines.iter().map(|line| distance_to_line(line, *p)).fold(f64::MAX, f64::min) < 40.0)
        });
        if theirs {
            continue;
        }
        if let Some(b) = profile(&way, region) {
            out.push(b);
        }
    }
    Ok(out)
}

/// A landmark as a region's bridge.
pub fn landmark(l: &Landmark, projection: &Projection, region: &Region) -> Bridge {
    let line: Vec<[f32; 3]> = l
        .line
        .iter()
        .map(|&(lat, lon, h)| {
            let (n, e) = projection.to_world(lat, lon);
            let h = if h < 0.0 { region.height(n, e).max(0.0) as f32 } else { h };
            [n as f32, e as f32, h]
        })
        .collect();
    let mut b = Bridge { line, width: l.width, carries: l.carries, structure: l.structure, towers: Vec::new(), tower_height: l.tower_height, colour: Some(l.colour), landmark: true, masonry: l.masonry };
    // The towers as distances along the line: where the line passes nearest to each.
    for &(lat, lon) in l.towers {
        let (n, e) = projection.to_world(lat, lon);
        let mut best = (f64::MAX, 0.0);
        let mut s = 0.0;
        for w in b.line.windows(2) {
            let (a, c) = ([w[0][0] as f64, w[0][1] as f64], [w[1][0] as f64, w[1][1] as f64]);
            let d = (c[0] - a[0]).hypot(c[1] - a[1]);
            let t = if d > 0.0 { (((n - a[0]) * (c[0] - a[0]) + (e - a[1]) * (c[1] - a[1])) / (d * d)).clamp(0.0, 1.0) } else { 0.0 };
            let off = (a[0] + (c[0] - a[0]) * t - n).hypot(a[1] + (c[1] - a[1]) * t - e);
            if off < best.0 {
                best = (off, s + t * d);
            }
            s += d;
        }
        b.towers.push(best.1 as f32);
    }
    b
}

fn distance_to_line(line: &[[f64; 2]], p: [f64; 2]) -> f64 {
    line.windows(2)
        .map(|w| {
            let (a, c) = (w[0], w[1]);
            let (dn, de) = (c[0] - a[0], c[1] - a[1]);
            let d2 = dn * dn + de * de;
            let t = if d2 > 0.0 { (((p[0] - a[0]) * dn + (p[1] - a[1]) * de) / d2).clamp(0.0, 1.0) } else { 0.0 };
            (a[0] + dn * t - p[0]).hypot(a[1] + de * t - p[1])
        })
        .fold(f64::MAX, f64::min)
}

/// Samples of a deck's line, metres apart at most.
const SAMPLE: f64 = 8.0;

/// A way of the maps as a bridge: its deck's height along it. Where an end is on land it meets the
/// ground there (or stands up to 2 m over it on its abutment); over water its end is free (a long
/// bridge is often several ways). In between, the deck stands over what it crosses by a clearance
/// for what it carries and what is under it (over water more the longer it is, as the long ones
/// let ships under), and rises and falls to it no more steeply than what it carries climbs
/// (`grade`), smoothed. What is under a point is the lowest of the ground across the way's width
/// and some more on either side: the heights the satellites measured may hold the bridge itself.
/// `None` for a way too short to be a bridge.
pub fn profile(way: &BridgePiece, region: &Region) -> Option<Bridge> {
    let pts = &way.points;
    let length: f64 = pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
    if pts.len() < 2 || length < 6.0 {
        return None;
    }
    // Samples along the line: its corners and points between them.
    let mut samples: Vec<(f64, [f64; 2], [f64; 2])> = Vec::new();
    let mut s = 0.0;
    for w in pts.windows(2) {
        let (a, c) = (w[0], w[1]);
        let d = (c[0] - a[0]).hypot(c[1] - a[1]);
        let dir = if d > 0.0 { [(c[0] - a[0]) / d, (c[1] - a[1]) / d] } else { [1.0, 0.0] };
        let k = (d / SAMPLE).ceil().max(1.0) as usize;
        for i in 0..k {
            let t = i as f64 / k as f64;
            samples.push((s + d * t, [a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t], dir));
        }
        s += d;
    }
    let last = pts[pts.len() - 1];
    let dir_last = samples.last().map_or([1.0, 0.0], |x| x.2);
    samples.push((length, last, dir_last));
    let grade = match way.carries {
        Carries::Road => 0.06,
        Carries::Rail => 0.03,
        Carries::Path => 0.10,
    };
    let spread = (way.width.max(20.0)) / 2.0;
    let mut needed = Vec::with_capacity(samples.len());
    let mut water = Vec::with_capacity(samples.len());
    for &(_, p, dir) in &samples {
        let cover = region.cover_at(p[0], p[1]);
        let wet = matches!(cover, Cover::Water | Cover::Sea);
        let under = [-1.0, -0.5, 0.0, 0.5, 1.0]
            .iter()
            .map(|k| region.height(p[0] - dir[1] * spread * k, p[1] + dir[0] * spread * k))
            .fold(f64::MAX, f64::min)
            .max(if cover == Cover::Sea { 0.0 } else { f64::MIN });
        let under = if cover == Cover::Sea { 0.0 } else { under };
        let clearance = match (wet, way.carries, way.layer) {
            (_, _, layer) if layer <= 0 => 0.6,
            (true, Carries::Path, _) => 2.5,
            (true, _, _) => (5.0 + 0.02 * length).min(25.0),
            (false, Carries::Path, _) => 3.0,
            (false, _, _) => 5.2,
        };
        needed.push(under + clearance);
        water.push(wet);
    }
    let count = samples.len();
    // The ends: on land, the ground there; over water, free.
    let ground = |i: usize| region.height(samples[i].1[0], samples[i].1[1]).max(0.0);
    let ends = [(0, !water[0]), (count - 1, !water[count - 1])];
    let end_height = |(i, land): (usize, bool)| if land { ground(i) } else { needed[i] };
    let (h0, h1) = (end_height(ends[0]), end_height(ends[1]));
    let mut deck: Vec<f64> = (0..count)
        .map(|i| {
            let s = samples[i].0;
            let straight = h0 + (h1 - h0) * s / length;
            // The highest the deck is pushed up to here by what it must clear anywhere along it.
            let tent = (0..count).map(|j| needed[j] - grade * (s - samples[j].0).abs()).fold(f64::MIN, f64::max);
            straight.max(tent)
        })
        .collect();
    // On land the ends come down to the ground, steeply over the last metres if they must.
    for &(i, land) in &ends {
        if !land {
            continue;
        }
        let top = ground(i) + 2.0;
        for (k, h) in deck.iter_mut().enumerate() {
            let away = (samples[k].0 - samples[i].0).abs();
            *h = h.min(top + 0.12 * away);
        }
    }
    for _ in 0..4 {
        let before = deck.clone();
        for i in 1..count - 1 {
            deck[i] = 0.25 * before[i - 1] + 0.5 * before[i] + 0.25 * before[i + 1];
        }
    }
    // Every other sample is enough where the line runs straight.
    let line: Vec<[f32; 3]> = samples.iter().zip(&deck).enumerate().filter(|(i, _)| i % 2 == 0 || *i == count - 1).map(|(_, ((_, p, _), h))| [p[0] as f32, p[1] as f32, *h as f32]).collect();
    Some(Bridge { line, width: way.width as f32, carries: way.carries, structure: way.structure, towers: Vec::new(), tower_height: 0.0, colour: way.colour, landmark: false, masonry: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::Grid;

    fn river() -> Region {
        // Flat ground at 20 m, a river 100 m wide at 12 m running north-south through east 0.
        let mut heights = Grid::centred(0.0, 0.0, 2000.0, 10.0, 20.0f32);
        let mut cover = Grid::centred(0.0, 0.0, 2000.0, 10.0, Cover::Built as u8);
        for r in 0..heights.rows {
            for c in 0..heights.cols {
                let (_, e) = heights.node(r, c);
                if e.abs() <= 50.0 {
                    heights.set(r, c, 12.0);
                    cover.set(r, c, Cover::Water as u8);
                }
            }
        }
        Region { name: "river".into(), latitude: 0.0, longitude: 0.0, heights: vec![heights], cover, airports: Vec::new(), roads: Vec::new(), towns: Vec::new(), buildings: Vec::new(), bridges: Vec::new() }
    }

    fn way(points: Vec<[f64; 2]>, carries: Carries) -> BridgePiece {
        BridgePiece { id: "w".into(), points, carries, width: 10.0, layer: 1, structure: Structure::Beam, colour: None, wikidata: None, name: None }
    }

    #[test]
    fn a_deck_meets_the_banks_and_clears_the_river_without_changing_it() {
        let region = river();
        let b = profile(&way(vec![[0.0, -120.0], [0.0, 120.0]], Carries::Road), &region).unwrap();
        let first = b.line[0];
        let last = b.line[b.line.len() - 1];
        assert!((first[2] - 20.0).abs() < 2.1 && (last[2] - 20.0).abs() < 2.1, "{first:?} {last:?}");
        // Over the middle of the river, clear of its surface by the clearance.
        let middle = b.line.iter().min_by(|a, c| a[1].abs().total_cmp(&c[1].abs())).unwrap();
        assert!(middle[2] > 12.0 + 5.0, "{middle:?}");
        // Smooth: no step between samples steeper than a ramp.
        for w in b.line.windows(2) {
            let d = (w[1][1] - w[0][1]).hypot(w[1][0] - w[0][0]);
            assert!((w[1][2] - w[0][2]).abs() <= 0.13 * d + 0.05, "{w:?}");
        }
        // The water is where it was.
        assert_eq!(region.height(0.0, 0.0), 12.0);
        assert!(profile(&way(vec![[0.0, 0.0], [0.0, 3.0]], Carries::Path), &region).is_none());
    }

    #[test]
    fn the_brooklyn_bridge_has_its_span_and_stone_towers() {
        let bb = &LANDMARKS[1];
        let projection = Projection::new(40.7061, -73.9969);
        let b = landmark(bb, &projection, &river());
        assert!(b.masonry);
        assert!((b.towers[1] - b.towers[0] - 486.3).abs() < 5.0, "{:?}", b.towers);
        // Side spans of 283.5 m to the anchorages, the line's ends.
        assert!((b.towers[0] - 283.5).abs() < 5.0 && (b.length() as f32 - b.towers[1] - 283.5).abs() < 5.0, "{:?} {}", b.towers, b.length());
        // The towers' tops 84.3 m above the water.
        assert!((b.at(b.towers[0] as f64).0[2] as f32 + b.tower_height - 84.3).abs() < 0.1);
    }

    #[test]
    fn a_landmark_stands_where_its_table_says() {
        let gg = &LANDMARKS[0];
        let projection = Projection::new(37.8199, -122.4783);
        let mut region = river();
        region.heights = vec![Grid::centred(0.0, 0.0, 8000.0, 50.0, -30.0f32)];
        let b = landmark(gg, &projection, &region);
        assert!(b.landmark && b.structure == Structure::Suspension);
        assert_eq!(b.towers.len(), 2);
        // 1280 m between the towers, within the maps' few metres; side spans to the anchorages.
        let span = b.towers[1] - b.towers[0];
        assert!((span - 1280.0).abs() < 5.0, "{span}");
        assert!(b.towers[0] > 300.0 && (b.length() as f32 - b.towers[1]) > 300.0, "{:?} {}", b.towers, b.length());
        assert_eq!(b.line[0][2], 70.0);
        // Its approaches come down to the ground (here the bed of the sea, so 0).
        for approach in gg.approaches {
            let a = landmark(&Landmark { line: approach, towers: &[], ..*gg }, &projection, &region);
            assert_eq!(a.line[a.line.len() - 1][2], 0.0);
        }
    }
}
