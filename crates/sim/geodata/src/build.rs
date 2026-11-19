//! Builds the region of a place: downloads (or reads from the cache) the tiles and tables it
//! needs, and lays them on the world's grids. Everything is done before the world is made; a region
//! built once is kept on disk (`store`) and read back the next time.
//!
//! What a region holds, round the place (the middle of the main runway of an aerodrome, or the point):
//!
//! | | side | step | from |
//! |---|---|---|---|
//! | heights near | 24 km | 12 m | Terrarium, zoom 13 |
//! | heights | 96 km | 30 m | Terrarium, zoom 12 |
//! | heights far | 400 km | 400 m | Terrarium, zoom 8 |
//! | cover, coast, roads, towns | 96 km | 24 m | OpenFreeMap, zoom 12 |
//! | buildings, where the ground is built up | 32 km | | Overture, zoom 14 |
//! | bridges, where the maps have them | 32 km | | Overture, zoom 13 |
//! | aerodromes | 96 km | | OurAirports |
//!
//! About 300 tiles and 17 MB of tables the first time (some 50 MB in all), 55 MB of grids in memory.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use sim_core::{hash2, smoothstep, Airport, Cover, Footprint, Grid, Region, Road, Roof, Town, Use};

use crate::fetch::{cache_dir, Fetcher};
use crate::geo::{tile_to_earth, tile_xy, tiles_over, Projection, TileId};
use crate::mvt::{self, Kind};
use crate::ourairports::{self, Tables};
use crate::overture::{self, Release, Tiles};
use crate::terrarium::{self, HeightTile, Mosaic};
use crate::{bridges, disk, store, threads, Error, Place};

/// A grid of heights: half its side and its step, metres, and the zoom of the tiles it is read from.
pub(crate) struct Level {
    pub(crate) half: f64,
    pub(crate) step: f64,
    pub(crate) zoom: u8,
}

pub(crate) const HEIGHTS: [Level; 3] = [Level { half: 12_000.0, step: 12.0, zoom: 13 }, Level { half: 48_000.0, step: 30.0, zoom: 12 }, Level { half: 200_000.0, step: 400.0, zoom: 8 }];
/// Half the side of the square the maps are read over, and the step of the cover's grid.
pub(crate) const COVER_HALF: f64 = 48_000.0;
const COVER_STEP: f64 = 24.0;
/// The zoom of the vector tiles: woods, fields, towns, water and the main roads, simplified to a few metres.
pub(crate) const MAP_ZOOM: u8 = 12;
/// The buildings and the bridges are read over this half side round the origin, metres.
pub(crate) const BUILDINGS_HALF: f64 = 16_000.0;
/// Downloads at once from one server.
const DOWNLOADS: usize = 4;

/// What is being done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Reading (or downloading) the tables of aerodromes.
    Aerodromes,
    Heights,
    Maps,
    Buildings,
    Bridges,
    /// Laying it all on the grids.
    Building,
    Done,
}

/// How far a load has got: `done` of `total` in its stage, and the bytes downloaded so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    pub done: usize,
    pub total: usize,
    pub downloaded: u64,
}

impl Progress {
    /// What the player reads.
    pub fn text(&self) -> String {
        let what = match self.stage {
            Stage::Aerodromes => "aerodromes",
            Stage::Heights => "terrain",
            Stage::Maps => "map",
            Stage::Buildings => "buildings",
            Stage::Bridges => "bridges",
            Stage::Building => "building the region",
            Stage::Done => "ready",
        };
        let mb = self.downloaded as f64 / 1e6;
        let downloaded = if mb >= 0.1 { format!(", {mb:.0} MB downloaded") } else { String::new() };
        if self.total > 1 {
            format!("{what} {}/{}{downloaded}", self.done, self.total)
        } else {
            format!("{what}{downloaded}")
        }
    }

    /// The whole load, 0 to 1.
    pub fn share(&self) -> f64 {
        let part = if self.total > 0 { self.done as f64 / self.total as f64 } else { 0.0 };
        match self.stage {
            Stage::Aerodromes => 0.05 * part,
            Stage::Heights => 0.05 + 0.45 * part,
            Stage::Maps => 0.5 + 0.25 * part,
            Stage::Buildings => 0.75 + 0.08 * part,
            Stage::Bridges => 0.83 + 0.02 * part,
            Stage::Building => 0.85 + 0.15 * part,
            Stage::Done => 1.0,
        }
    }
}

/// A region and how it came.
pub struct Loaded {
    pub region: Region,
    /// Bytes downloaded for it (0 when everything was in the cache).
    pub downloaded: u64,
    /// It was read whole from the cache, as built before.
    pub kept: bool,
}

/// The region of a place: the one built before, from the cache, or a new one. `offline` asks
/// nothing of the network (what is not in the cache is an error). `progress` is called from the
/// loading threads.
pub fn load(place: &Place, offline: bool, progress: &(dyn Fn(Progress) + Sync)) -> Result<Loaded, Error> {
    load_around(place, (0.0, 0.0), offline, progress)
}

/// The region of a place round another middle than its origin, `centre` (north, east, metres in
/// the place's axes, on the lattice of `follow::LATTICE`): a region further along the way, its
/// grids' nodes where the place's own region has them (`follow`). Kept as `<place>@<n>_<e>.region`.
pub fn load_around(place: &Place, centre: (f64, f64), offline: bool, progress: &(dyn Fn(Progress) + Sync)) -> Result<Loaded, Error> {
    let fetcher = Fetcher::new(cache_dir(), offline);
    let name = if centre == (0.0, 0.0) { place.key() } else { format!("{}@{:.0}_{:.0}", place.key(), centre.0, centre.1) };
    let file = fetcher.root().join("regions").join(format!("{name}.region"));
    if let Ok(Some(region)) = disk::read(&file).map_err(|e| Error::Io(e.to_string())).and_then(|b| store::decode(&b)) {
        progress(Progress { stage: Stage::Done, done: 1, total: 1, downloaded: 0 });
        return Ok(Loaded { region, downloaded: 0, kept: true });
    }
    let region = build(place, centre, &fetcher, progress)?;
    disk::write(&file, &store::encode(&region)).map_err(|e| Error::Io(format!("{}: {e}", file.display())))?;
    progress(Progress { stage: Stage::Done, done: 1, total: 1, downloaded: fetcher.downloaded() });
    Ok(Loaded { region, downloaded: fetcher.downloaded(), kept: false })
}

/// Both tables of OurAirports, from the cache or downloaded.
pub fn tables(fetcher: &Fetcher) -> Result<Tables, Error> {
    let get = |key: &str, url: &str| -> Result<String, Error> {
        let bytes = fetcher.cached(key, url)?.ok_or_else(|| Error::Http(url.to_string()))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    let airports = get("ourairports/airports.csv", ourairports::AIRPORTS_URL)?;
    let runways = get("ourairports/runways.csv", ourairports::RUNWAYS_URL)?;
    Ok(Tables::parse(&airports, &runways))
}

fn build(place: &Place, centre: (f64, f64), fetcher: &Fetcher, progress: &(dyn Fn(Progress) + Sync)) -> Result<Region, Error> {
    let report = |stage: Stage, done: usize, total: usize| progress(Progress { stage, done, total, downloaded: fetcher.downloaded() });
    report(Stage::Aerodromes, 0, 1);
    let tables = tables(fetcher)?;
    let (name, origin) = match place {
        Place::Aerodrome(code) => {
            let a = tables.find(code).ok_or_else(|| Error::Unknown(format!("aerodrome {code}")))?;
            // The origin is the middle of its main runway, so that the world starts on it.
            let around = Projection::new(a.latitude, a.longitude);
            let runways: Vec<_> = tables.runways_of(a.id).collect();
            let main = ourairports::place(a, &runways, &around);
            let origin = main.first().map_or((a.latitude, a.longitude), |p| around.to_earth(p.airport.n, p.airport.e));
            (format!("{} ({code})", a.name), origin)
        }
        Place::At(lat, lon) => (format!("{lat:.3}, {lon:.3}"), (*lat, *lon)),
    };
    let projection = Projection::new(origin.0, origin.1);

    // Heights: the tiles of each level, then its grid.
    let mut mosaics = Vec::new();
    let all: Vec<Vec<TileId>> = HEIGHTS.iter().map(|l| tiles_over(&projection, centre, l.half + l.step * 2.0, l.zoom)).collect();
    let total: usize = all.iter().map(Vec::len).sum();
    let done = AtomicUsize::new(0);
    for (level, tiles) in HEIGHTS.iter().zip(&all) {
        let fetched = fetch_all(tiles, |id| {
            let key = terrarium::key(id);
            let tile = match fetcher.cached(&key, &terrarium::url(id))? {
                Some(bytes) => Some(terrarium::decode(&bytes)?),
                None => None,
            };
            report(Stage::Heights, done.fetch_add(1, Ordering::Relaxed) + 1, total);
            Ok(tile)
        })?;
        let tiles: HashMap<(u32, u32), HeightTile> = fetched.into_iter().filter_map(|(id, t)| t.map(|t| ((id.x, id.y), t))).collect();
        mosaics.push((Mosaic::new(level.zoom, tiles), level));
    }
    let mut heights: Vec<Grid<f32>> = mosaics.iter().map(|(mosaic, level)| heights_grid(&projection, mosaic, level, centre)).collect();

    // The maps: what covers the ground, the water and the roads.
    let tiles = tiles_over(&projection, centre, COVER_HALF, MAP_ZOOM);
    let template = if tiles.iter().all(|id| disk::exists(&fetcher.root().join(map_key(*id)))) { String::new() } else { map_template(fetcher)? };
    let done = AtomicUsize::new(0);
    let maps = fetch_all(&tiles, |id| {
        let read = match fetcher.cached(&map_key(id), &template.replace("{z}", &id.z.to_string()).replace("{x}", &id.x.to_string()).replace("{y}", &id.y.to_string()))? {
            Some(bytes) => read_map(&bytes, id, &projection),
            None => MapTile::default(),
        };
        report(Stage::Maps, done.fetch_add(1, Ordering::Relaxed) + 1, tiles.len());
        Ok(read)
    })?;
    report(Stage::Building, 0, 4);
    // Where the maps have bridges: their tiles of Overture's are read for them.
    let with_bridges: Vec<TileId> = maps.iter().filter(|(_, m)| m.bridges).map(|(id, _)| *id).collect();
    let maps: Vec<MapTile> = maps.into_iter().map(|(_, m)| m).collect();
    let cover = cover_grid(&maps, centre);
    let mut roads: Vec<Road> = maps.iter().flat_map(|m| m.roads.iter().copied()).collect();
    let inside = |n: f64, e: f64| (n - centre.0).abs().max((e - centre.1).abs()) < COVER_HALF;
    roads.retain(|r| inside(r.n0, r.e0) && inside(r.n1, r.e1));
    report(Stage::Building, 1, 4);

    // The coast as the maps draw it: the sea below 0, the land above.
    let works = works_grid(&maps, &cover);
    let offshore = Offshore::new(&cover, &works);
    let coastline = Coastline::new(&maps, &cover, &works);
    let (near, far) = heights.split_at_mut(2);
    for grid in near {
        let sea = sea_mask(grid, &maps);
        coast(grid, &sea, &cover, &coastline, &offshore, &far[0]);
    }
    drop((works, offshore, coastline));
    report(Stage::Building, 2, 4);
    let release = OnceLock::new();
    let buildings = buildings(&projection, centre, fetcher, &cover, &release, &|done, total| report(Stage::Buildings, done, total))?;

    let mut region = Region { name, latitude: origin.0.to_radians(), longitude: origin.1.to_radians(), heights, cover, airports: Vec::new(), roads, towns: Vec::new(), buildings, bridges: Vec::new() };
    region.bridges = bridges::of_region(&projection, centre, fetcher, &release, &with_bridges, &region, &|done, total| report(Stage::Bridges, done, total))?;
    region.airports = aerodromes(&tables, &projection, centre, &region);
    report(Stage::Building, 3, 4);
    region.towns = towns(&region.cover, &region.roads, &region.airports);
    report(Stage::Building, 4, 4);
    Ok(region)
}

/// The buildings round the origin where the ground is built up, from Overture's tiles of zoom 14
/// (`overture`): each once, from the tile its middle is in (a tile has a margin of its
/// neighbours'), with its courtyards; a building drawn by its parts (`has_parts`) left out where
/// its parts are there. Their heights are the maps' (`height`, or the floors at a storey's height
/// for the building's use, `overture::storey`); those without are given one (`settle`).
fn buildings(projection: &Projection, centre: (f64, f64), fetcher: &Fetcher, cover: &Grid<u8>, release: &OnceLock<Result<Release, Error>>, report: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<Footprint>, Error> {
    let built = |id: TileId| {
        // A tile is read when some of it is built up, sampled every 100 m or so.
        (0..12).flat_map(|i| (0..12).map(move |j| (i, j))).any(|(i, j)| {
            let (lat, lon) = tile_to_earth(id.x as f64 + (i as f64 + 0.5) / 12.0, id.y as f64 + (j as f64 + 0.5) / 12.0, id.z);
            let (n, e) = projection.to_world(lat, lon);
            cover.nearest(n, e).map(Cover::from_byte) == Some(Cover::Built)
        })
    };
    let tiles: Vec<TileId> = tiles_over(projection, centre, BUILDINGS_HALF, overture::BUILDINGS_ZOOM).into_iter().filter(|id| built(*id)).collect();
    let source = Tiles::new(fetcher, "buildings", release);
    let done = AtomicUsize::new(0);
    let read = fetch_all(&tiles, |id| {
        let found = overture::read_buildings(&source.get(id)?, id, projection);
        report(done.fetch_add(1, Ordering::Relaxed) + 1, tiles.len());
        Ok(found)
    })?;
    let all: Vec<overture::Read> = read.into_iter().flat_map(|(_, b)| b).collect();
    let drawn_by_parts: HashSet<&str> = all.iter().filter_map(|r| r.part_of.as_deref()).collect();
    let kept: Vec<&overture::Read> = all.iter().filter(|r| r.part_of.is_some() || !r.has_parts || !drawn_by_parts.contains(r.id.as_str())).collect();
    let roof_said: Vec<bool> = kept.iter().map(|r| r.roof_said).collect();
    let mut out: Vec<Footprint> = kept.into_iter().map(|r| r.footprint.clone()).collect();
    settle(&mut out, &roof_said);
    Ok(out)
}

/// Side of the squares over which the heights the maps give are taken as what an unmeasured
/// building's neighbours are like, metres.
const NEIGHBOURS: f64 = 200.0;

/// Gives a height to the buildings the maps give none (many: in a city, a third or more), and a
/// pitched roof to the small houses standing on their own whose roof they do not describe
/// (`roof_said`, one for each building). An unmeasured block of flats, offices or a building of
/// unknown use is as tall as the middle (median) of the measured ones of its square of `NEIGHBOURS`
/// (in Madrid's centre most are measured, the streets of one height); failing that, as its use and
/// size make it. Houses, sheds and the like are as their use makes them. A house that shares a wall
/// with another (a terrace, a row house: two corners or more on another's) keeps a flat roof, as
/// the row houses of American and Spanish towns mostly have.
pub(crate) fn settle(buildings: &mut [Footprint], roof_said: &[bool]) {
    let attached = attached(buildings);
    let square = |b: &Footprint| {
        let (n, e) = b.centre();
        ((n / NEIGHBOURS).floor() as i32, (e / NEIGHBOURS).floor() as i32)
    };
    let mut measured: HashMap<(i32, i32), Vec<f32>> = HashMap::new();
    for b in buildings.iter().filter(|b| b.measured && b.floor == 0.0 && !matches!(b.kind, Use::House | Use::Outbuilding) && b.area() > 80.0) {
        measured.entry(square(b)).or_default().push(b.height);
    }
    let medians: HashMap<(i32, i32), f32> = measured
        .into_iter()
        .filter(|(_, v)| v.len() >= 3)
        .map(|(k, mut v)| {
            v.sort_by(f32::total_cmp);
            (k, v[v.len() / 2])
        })
        .collect();
    for (index, b) in buildings.iter_mut().enumerate() {
        let area = b.area();
        if !b.measured {
            let by_use = match b.kind {
                Use::House => 6.5,
                Use::Outbuilding => 3.5,
                Use::Industrial => 8.0,
                Use::Retail => 7.0,
                Use::Religious => 14.0,
                Use::Civic => 12.0,
                Use::Residential => 12.0,
                Use::Office => 16.0,
                Use::Unknown if area < 60.0 => 3.5,
                Use::Unknown if area < 150.0 => 6.0,
                Use::Unknown => 9.0,
            };
            let near = medians.get(&square(b)).copied();
            b.height = match (b.kind, near) {
                (Use::House | Use::Outbuilding | Use::Industrial | Use::Religious, _) => by_use,
                // A shed in a yard is not as tall as the street.
                (_, Some(h)) if area < 60.0 => h.min(6.0),
                (_, Some(h)) => h,
                (_, None) => by_use,
            };
            b.height = b.height.max(b.floor + 2.5);
        }
        // Small houses whose roof the maps do not describe have a pitched one: gabled or hipped by
        // a number of their own; churches a gabled one.
        let small = area < 260.0 && b.ring.len() <= 8 && b.height <= 13.0;
        let house = matches!(b.kind, Use::House | Use::Outbuilding) || (b.kind == Use::Unknown && small);
        let alone = !attached[index];
        if !roof_said[index] && b.floor == 0.0 && b.holes.is_empty() && ((house && small && alone) || (b.kind == Use::Religious && area < 2500.0)) {
            let (n, e) = b.centre();
            b.roof = if b.kind != Use::Religious && hash2(n.round() as i64, e.round() as i64, 91) < 0.45 { Roof::Hipped } else { Roof::Gabled };
        }
    }
}

/// Whether each building shares a wall with another: two of its corners or more within 40 cm of
/// another's corners.
fn attached(buildings: &[Footprint]) -> Vec<bool> {
    const STEP: f32 = 0.4;
    let key = |p: [f32; 2]| ((p[0] / STEP).round() as i64) << 32 ^ ((p[1] / STEP).round() as i64 & 0xffff_ffff);
    let mut corners: Vec<(i64, u32)> = buildings.iter().enumerate().flat_map(|(i, b)| b.ring.iter().map(move |p| (key(*p), i as u32))).collect();
    corners.sort_unstable();
    let mut shared = vec![0u8; buildings.len()];
    for run in corners.chunk_by(|a, b| a.0 == b.0) {
        if run.iter().any(|c| c.1 != run[0].1) {
            for c in run {
                shared[c.1 as usize] = shared[c.1 as usize].saturating_add(1);
            }
        }
    }
    shared.into_iter().map(|k| k >= 2).collect()
}

/// A ring without its closing corner, the corners nearer than 30 cm to the last kept and those
/// on a straight line between their neighbours.
pub(crate) fn simplify(mut ring: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    let mut kept: Vec<[f32; 2]> = Vec::with_capacity(ring.len());
    for p in ring {
        if kept.last().is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > 0.3) {
            kept.push(p);
        }
    }
    let n = kept.len();
    if n < 4 {
        return kept;
    }
    (0..n)
        .filter(|&i| {
            let (a, b, c) = (kept[(i + n - 1) % n], kept[i], kept[(i + 1) % n]);
            let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            cross.abs() > 0.05 * (b[0] - a[0]).hypot(b[1] - a[1]) * (c[0] - b[0]).hypot(c[1] - b[1])
        })
        .map(|i| kept[i])
        .collect()
}

/// Runs `job` for every tile, `DOWNLOADS` at a time, and gives back the results in the tiles' order.
pub(crate) fn fetch_all<T: Send>(tiles: &[TileId], job: impl Fn(TileId) -> Result<T, Error> + Sync) -> Result<Vec<(TileId, T)>, Error> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(tiles.len()));
    let failed: Mutex<Option<Error>> = Mutex::new(None);
    threads::scope(|scope| {
        for _ in 0..DOWNLOADS.min(tiles.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= tiles.len() || failed.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                    break;
                }
                match job(tiles[i]) {
                    Ok(t) => results.lock().unwrap_or_else(|e| e.into_inner()).push((i, t)),
                    Err(e) => *failed.lock().unwrap_or_else(|e| e.into_inner()) = Some(e),
                }
            });
        }
    });
    if let Some(e) = failed.into_inner().unwrap_or_else(|e| e.into_inner()) {
        return Err(e);
    }
    let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort_by_key(|r| r.0);
    Ok(results.into_iter().map(|(i, t)| (tiles[i], t)).collect())
}

/// The heights of a level, sampled from its tiles at every node, on as many threads as there are cores.
fn heights_grid(projection: &Projection, mosaic: &Mosaic, level: &Level, centre: (f64, f64)) -> Grid<f32> {
    let mut grid = Grid::centred(centre.0, centre.1, level.half * 2.0, level.step, 0.0f32);
    let cols = grid.cols;
    let (south, west, step) = (grid.south, grid.west, grid.step);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    let rows_each = grid.rows.div_ceil(threads);
    threads::scope(|scope| {
        for (part, rows) in grid.data.chunks_mut(rows_each * cols).enumerate() {
            scope.spawn(move || {
                for (k, row) in rows.chunks_mut(cols).enumerate() {
                    let n = south + (part * rows_each + k) as f64 * step;
                    for (c, h) in row.iter_mut().enumerate() {
                        let (lat, lon) = projection.to_earth(n, west + c as f64 * step);
                        let (x, y) = tile_xy(lat, lon, mosaic.z);
                        *h = mosaic.sample(x, y);
                    }
                }
            });
        }
    });
    grid
}

/// An area of a map: the layer it is drawn in (its index in `LAYERS`), what covers it, and its
/// rings, north and east.
pub(crate) struct Area {
    pub(crate) layer: usize,
    pub(crate) cover: Cover,
    pub(crate) rings: Vec<Vec<[f64; 2]>>,
    /// Built up for work (OpenMapTiles' industrial, commercial, retail, garages): a port's ground,
    /// whose shore is a quay.
    pub(crate) works: bool,
}

/// What one vector tile says, in the world's axes.
#[derive(Default)]
pub(crate) struct MapTile {
    pub(crate) areas: Vec<Area>,
    pub(crate) roads: Vec<Road>,
    /// The edges of the sea's polygons in the tile itself (not in its margin, where the tile is
    /// cut): the coastline, as stretches between two points, north and east.
    pub(crate) coast: Vec<[[f64; 2]; 2]>,
    /// Whether it has bridges (`brunnel=bridge`): Overture's tiles over it are read for them.
    pub(crate) bridges: bool,
}

/// The layers read, in the order they are drawn: what is drawn later covers what was drawn before.
const LAYERS: [&str; 5] = ["landcover", "landuse", "aeroway", "water", "transportation"];

pub(crate) fn map_key(id: TileId) -> String {
    format!("openfreemap/{}/{}/{}.pbf", id.z, id.x, id.y)
}

/// The address of OpenFreeMap's tiles, from its TileJSON (it names the build of the planet in use).
pub(crate) fn map_template(fetcher: &Fetcher) -> Result<String, Error> {
    const TILEJSON: &str = "https://tiles.openfreemap.org/planet";
    // Read anew on every build: the planet it names changes every week.
    let bytes = fetcher.get(TILEJSON)?.ok_or_else(|| Error::Http(TILEJSON.into()))?;
    let text = String::from_utf8_lossy(&bytes);
    let start = text.find("\"tiles\"").and_then(|i| text[i..].find("\"http").map(|j| i + j + 1)).ok_or_else(|| Error::Data("OpenFreeMap TileJSON".into()))?;
    let end = text[start..].find('"').map(|j| start + j).ok_or_else(|| Error::Data("OpenFreeMap TileJSON".into()))?;
    Ok(text[start..end].to_string())
}

/// What covers an area of a layer, as OpenMapTiles classes it.
fn cover_of(layer: &str, f: &mvt::Feature) -> Option<Cover> {
    let class = f.text("class").unwrap_or("");
    match layer {
        "water" if f.text("brunnel") == Some("tunnel") || class == "swimming_pool" => None,
        // A dock is sea too, walled in.
        "water" if matches!(class, "ocean" | "dock") => Some(Cover::Sea),
        "water" => Some(Cover::Water),
        "landcover" => match class {
            "wood" | "forest" => Some(Cover::Forest),
            "farmland" => Some(Cover::Farmland),
            "grass" if matches!(f.text("subclass"), Some("scrub" | "heath" | "fell")) => Some(Cover::Scrub),
            "grass" => Some(Cover::Grass),
            "rock" => Some(Cover::Rock),
            "sand" => Some(Cover::Sand),
            "ice" | "glacier" => Some(Cover::Ice),
            "wetland" => Some(Cover::Wetland),
            _ => None,
        },
        "landuse" => match class {
            "residential" | "suburb" | "quarter" | "neighbourhood" | "commercial" | "industrial" | "retail" | "garages" => Some(Cover::Built),
            "quarry" => Some(Cover::Rock),
            // Tracks, yards and stations: ballast and concrete, no trees (over a town's ground).
            "railway" => Some(Cover::Railway),
            _ => None,
        },
        // The aerodrome's ground is mown: no trees, no fields, no houses.
        "aeroway" if matches!(class, "aerodrome" | "runway" | "taxiway" | "apron") => Some(Cover::Grass),
        _ => None,
    }
}

/// The width of a road of a class from kerb to kerb, metres; `None` for what is not a road for
/// cars. The maps draw a motorway or a dual carriageway as one way a carriageway (`oneway`), and
/// each of its slip roads as a way of its own (`ramp`): a carriageway of a motorway is some three
/// lanes and a hard shoulder, of a trunk road two lanes and a strip, a slip road one or two lanes.
/// Drawn as wide as the whole road, the dozens of ways of a junction would be one slab of asphalt.
fn road_width(class: &str, oneway: bool, ramp: bool) -> Option<f64> {
    let width = match class {
        "motorway" => 22.0,
        "trunk" => 16.0,
        "primary" => 11.0,
        "secondary" => 9.0,
        "tertiary" => 7.5,
        "minor" => 6.0,
        _ => return None,
    };
    Some(match (class, oneway, ramp) {
        (_, _, true) => f64::min(width, 6.5),
        ("motorway", true, _) => 12.0,
        ("trunk", true, _) => 9.0,
        _ => width,
    })
}

/// Reads a vector tile into the world's axes.
pub(crate) fn read_map(bytes: &[u8], id: TileId, projection: &Projection) -> MapTile {
    let layers = mvt::decode(bytes, &LAYERS);
    let mut out = MapTile::default();
    for (rank, name) in LAYERS.iter().enumerate() {
        let Some(layer) = layers.get(*name) else { continue };
        let to_world = |p: [f32; 2]| {
            let (lat, lon) = tile_to_earth(id.x as f64 + p[0] as f64 / layer.extent as f64, id.y as f64 + p[1] as f64 / layer.extent as f64, id.z);
            let (n, e) = projection.to_world(lat, lon);
            [n, e]
        };
        for f in &layer.features {
            if *name == "transportation" {
                // A bridge is not painted on what it crosses: it is a bridge of its own (`bridges`).
                if f.text("brunnel") == Some("bridge") {
                    out.bridges = true;
                    continue;
                }
                let flag = |key: &str| match f.properties.get(key) {
                    Some(mvt::Value::Bool(b)) => *b,
                    Some(v) => v.number().is_some_and(|v| v != 0.0),
                    None => false,
                };
                let Some(width) = road_width(f.text("class").unwrap_or(""), flag("oneway"), flag("ramp")) else { continue };
                if f.kind != Kind::Line || f.text("brunnel") == Some("tunnel") {
                    continue;
                }
                for line in &f.parts {
                    for pair in line.windows(2) {
                        // Each stretch once: the tile next door has those in its margin.
                        let mid = [(pair[0][0] + pair[1][0]) / 2.0, (pair[0][1] + pair[1][1]) / 2.0];
                        if mid[0] < 0.0 || mid[1] < 0.0 || mid[0] >= layer.extent || mid[1] >= layer.extent {
                            continue;
                        }
                        let (a, b) = (to_world(pair[0]), to_world(pair[1]));
                        out.roads.push(Road { n0: a[0], e0: a[1], n1: b[0], e1: b[1], width });
                    }
                }
            } else if f.kind == Kind::Polygon {
                if let Some(cover) = cover_of(name, f) {
                    if cover == Cover::Sea {
                        for ring in &f.parts {
                            for (i, &a) in ring.iter().enumerate() {
                                let b = ring[(i + 1) % ring.len()];
                                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                                if mid[0] > 0.0 && mid[1] > 0.0 && mid[0] < layer.extent && mid[1] < layer.extent {
                                    out.coast.push([to_world(a), to_world(b)]);
                                }
                            }
                        }
                    }
                    out.areas.push(Area { layer: rank, cover, rings: f.parts.iter().map(|ring| ring.iter().map(|&p| to_world(p)).collect()).collect(), works: *name == "landuse" && matches!(f.text("class"), Some("industrial" | "commercial" | "retail" | "garages")) });
                }
            }
        }
    }
    out
}

/// The cover's grid round `centre`: each layer drawn over the one before, and in a layer the greater cover over the lesser.
pub(crate) fn cover_grid(maps: &[MapTile], centre: (f64, f64)) -> Grid<u8> {
    fill_cover(Grid::centred(centre.0, centre.1, COVER_HALF * 2.0, COVER_STEP, Cover::Unknown as u8), maps)
}

/// `cover_grid` on a grid of any place and size.
fn fill_cover(mut grid: Grid<u8>, maps: &[MapTile]) -> Grid<u8> {
    let mut areas: Vec<&Area> = maps.iter().flat_map(|m| &m.areas).collect();
    areas.sort_by_key(|a| (a.layer, a.cover));
    let (rows, cols) = (grid.rows, grid.cols);
    for area in areas {
        // In nodes, with the centre of a node where the fill looks for it.
        let rings: Vec<Vec<[f64; 2]>> = area.rings.iter().map(|r| r.iter().map(|p| [(p[1] - grid.west) / grid.step + 0.5, (p[0] - grid.south) / grid.step + 0.5]).collect()).collect();
        mvt::fill_rings(cols, rows, &rings, |c, r| grid.set(r, c, area.cover as u8));
    }
    grid
}

/// How the bed falls from the shore, where the fine heights do not know it (they read 0 or so over
/// the sea): down the shelf (`SHELF`, 4 %) from the depth each kind of shore starts at
/// (`Shore::start`). Beyond `OPEN` from the shore the far grid's bathymetry (ETOPO1, 400 m) takes
/// over where it is shallower; nearer, it is too coarse to know the shore and is not used.
const SHELF: f64 = 0.04;
const OPEN: (f64, f64) = (1500.0, 3000.0);

/// Side of the squares the coastline's stretches are found by, metres; a node looks in the nine
/// round it, so the exact distance is known up to this far from the shore.
const COAST_SQUARE: f64 = 64.0;

/// What a stretch of coast is, by what the maps say covers the land behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Shore {
    /// Sand: a gentle shelf, the land rising slowly from the water.
    Beach,
    /// A port, an industry, shops (`Area::works`): deep water at once, the land standing as a quay.
    Quay,
    /// A town's front (houses by the water): some metres deep near it.
    Front,
    /// Rock, wood, fields, houses, or what the maps do not say: a few metres deep near it, as a
    /// rocky coast.
    Natural,
}

impl Shore {
    /// The shore a node of the cover's grid makes, by what covers it and whether it is a port's.
    fn of(cover: u8, works: bool) -> Shore {
        if cover == Cover::Sand as u8 {
            Shore::Beach
        } else if works {
            Shore::Quay
        } else if cover == Cover::Built as u8 {
            Shore::Front
        } else {
            Shore::Natural
        }
    }

    fn from_byte(b: u8) -> Shore {
        [Shore::Beach, Shore::Quay, Shore::Front, Shore::Natural].get(b as usize).copied().unwrap_or(Shore::Natural)
    }

    /// The depth the shelf starts at (the bed falls to it from the waterline, then down the
    /// shelf); how steeply ground and bed meet at the waterline (rise per metre, the same on both
    /// sides of it, so the 0 between two nodes falls where the map's coast is); and how high the
    /// land stands by it at least, reached at that slope.
    fn start(self) -> (f64, f64, f64) {
        match self {
            Shore::Beach => (0.3, 0.06, 0.5),
            Shore::Quay => (12.0, 0.5, 2.5),
            Shore::Front => (6.0, 0.5, 1.5),
            Shore::Natural => (2.0, 0.5, 0.5),
        }
    }
}

/// The coastline: the edges of the sea's polygons, each with what kind of shore it is, by squares
/// of `COAST_SQUARE`.
struct Coastline {
    stretches: Vec<([[f64; 2]; 2], Shore)>,
    squares: HashMap<(i32, i32), Vec<u32>>,
}

impl Coastline {
    fn new(maps: &[MapTile], cover: &Grid<u8>, works: &Grid<u8>) -> Coastline {
        let mut stretches = Vec::new();
        let mut squares: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let square = |v: f64| (v / COAST_SQUARE).floor() as i32;
        for &[a, b] in maps.iter().flat_map(|m| &m.coast) {
            let (dn, de) = (b[0] - a[0], b[1] - a[1]);
            let length = dn.hypot(de).max(1e-9);
            // The shore either side of its middle makes, 30 m off (the land's side: the sea's is
            // sea, `Natural`), the first in `Shore`'s order.
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            // Water on both sides (where a dock meets the open sea, or a river's mouth): no shore.
            let water = |s: f64| cover.nearest(mid[0] - de / length * s, mid[1] + dn / length * s).is_some_and(|c| c == Cover::Sea as u8 || c == Cover::Water as u8);
            if water(-30.0) && water(30.0) {
                continue;
            }
            let shore = [-30.0, 30.0]
                .iter()
                .map(|s| {
                    let (n, e) = (mid[0] - de / length * s, mid[1] + dn / length * s);
                    Shore::of(cover.nearest(n, e).unwrap_or(0), works.nearest(n, e) == Some(1))
                })
                .min()
                .unwrap_or(Shore::Natural);
            let i = stretches.len() as u32;
            stretches.push(([a, b], shore));
            for x in square(a[0].min(b[0]))..=square(a[0].max(b[0])) {
                for y in square(a[1].min(b[1]))..=square(a[1].max(b[1])) {
                    squares.entry((x, y)).or_default().push(i);
                }
            }
        }
        Coastline { stretches, squares }
    }

    /// The distance to the nearest stretch of coast within reach (`COAST_SQUARE`), and what it is.
    fn nearest(&self, n: f64, e: f64) -> Option<(f64, Shore)> {
        let (x, y) = ((n / COAST_SQUARE).floor() as i32, (e / COAST_SQUARE).floor() as i32);
        let mut best: Option<(f64, Shore)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &i in self.squares.get(&(x + dx, y + dy)).into_iter().flatten() {
                    let ([a, b], shore) = self.stretches[i as usize];
                    let d = Road { n0: a[0], e0: a[1], n1: b[0], e1: b[1], width: 0.0 }.distance(n, e);
                    if best.is_none_or(|(least, _)| d < least) {
                        best = Some((d, shore));
                    }
                }
            }
        }
        best.filter(|(d, _)| *d <= COAST_SQUARE)
    }
}

/// Where the ground is a port's, an industry's or shops' (`Area::works`), on the cover's grid: 1, else 0.
fn works_grid(maps: &[MapTile], cover: &Grid<u8>) -> Grid<u8> {
    let mut works = Grid::new(cover.south, cover.west, cover.step, cover.rows, cover.cols, 0u8);
    for area in maps.iter().flat_map(|m| &m.areas).filter(|a| a.works) {
        let rings: Vec<Vec<[f64; 2]>> = area.rings.iter().map(|r| r.iter().map(|p| [(p[1] - works.west) / works.step + 0.5, (p[0] - works.south) / works.step + 0.5]).collect()).collect();
        mvt::fill_rings(works.cols, works.rows, &rings, |c, r| works.set(r, c, 1));
    }
    works
}

/// The shore as seen from out at sea, at every node of the cover's grid: the distance to the
/// nearest node that is not sea, metres (0 on land), and what kind of shore that node is (as
/// `Shore::of`). A chamfer of two passes, within 1 % of the true distance; where the coastline is
/// out of `Coastline`'s reach, the depth goes by it, so a harbour is as deep in its middle as by
/// its quays.
struct Offshore {
    distance: Grid<f32>,
    shore: Grid<u8>,
}

impl Offshore {
    fn new(cover: &Grid<u8>, works: &Grid<u8>) -> Offshore {
        let mut distance = Grid::new(cover.south, cover.west, cover.step, cover.rows, cover.cols, 0.0f32);
        let mut shore = Grid::new(cover.south, cover.west, cover.step, cover.rows, cover.cols, 0u8);
        let (rows, cols) = (cover.rows, cover.cols);
        let (side, diagonal) = (cover.step as f32, cover.step as f32 * std::f32::consts::SQRT_2);
        for i in 0..rows * cols {
            if cover.data[i] == Cover::Sea as u8 {
                distance.data[i] = 1e9;
            } else {
                shore.data[i] = Shore::of(cover.data[i], works.data[i] == 1) as u8;
            }
        }
        let mut relax = |r: usize, c: usize, dr: isize, dc: isize, cost: f32| {
            let (rr, cc) = (r as isize + dr, c as isize + dc);
            if rr >= 0 && cc >= 0 && (rr as usize) < rows && (cc as usize) < cols {
                let through = distance.at(rr as usize, cc as usize) + cost;
                if through < distance.at(r, c) {
                    distance.set(r, c, through);
                    shore.set(r, c, shore.at(rr as usize, cc as usize));
                }
            }
        };
        for r in 0..rows {
            for c in 0..cols {
                for (dr, dc, cost) in [(-1, -1, diagonal), (-1, 0, side), (-1, 1, diagonal), (0, -1, side)] {
                    relax(r, c, dr, dc, cost);
                }
            }
        }
        for r in (0..rows).rev() {
            for c in (0..cols).rev() {
                for (dr, dc, cost) in [(1, 1, diagonal), (1, 0, side), (1, -1, diagonal), (0, 1, side)] {
                    relax(r, c, dr, dc, cost);
                }
            }
        }
        Offshore { distance, shore }
    }

    /// The distance to the shore and what it is, for a point out of `Coastline`'s reach.
    fn at(&self, n: f64, e: f64) -> (f64, Shore) {
        (self.distance.sample(n, e).max(COAST_SQUARE), Shore::from_byte(self.shore.nearest(n, e).unwrap_or(Shore::Natural as u8)))
    }
}

/// The nodes of a grid whose centres are inside the sea's polygons, row after row.
fn sea_mask(grid: &Grid<f32>, maps: &[MapTile]) -> Vec<bool> {
    let mut sea = vec![false; grid.rows * grid.cols];
    for area in maps.iter().flat_map(|m| &m.areas).filter(|a| a.cover == Cover::Sea) {
        let rings: Vec<Vec<[f64; 2]>> = area.rings.iter().map(|r| r.iter().map(|p| [(p[1] - grid.west) / grid.step + 0.5, (p[0] - grid.south) / grid.step + 0.5]).collect()).collect();
        mvt::fill_rings(grid.cols, grid.rows, &rings, |c, r| sea[r * grid.cols + c] = true);
    }
    sea
}

/// Puts the sea below 0 and the land above it where the maps tell them apart, so that the
/// waterline follows the map's coastline between the nodes: the heights near it go by the exact
/// distance to it, which changes smoothly from node to node, not by which side a node is on.
/// The bed falls from a beach down the shelf, from any other shore steeply to `DEEP` (`SHELF`,
/// `OPEN`); the land by a beach slopes up from the water, by any other shore it stands as a quay.
fn coast(grid: &mut Grid<f32>, sea: &[bool], cover: &Grid<u8>, coastline: &Coastline, offshore: &Offshore, bathymetry: &Grid<f32>) {
    let (cols, south, west, step) = (grid.cols, grid.south, grid.west, grid.step);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    let rows_each = grid.rows.div_ceil(threads);
    threads::scope(|scope| {
        for (part, (rows, sea)) in grid.data.chunks_mut(rows_each * cols).zip(sea.chunks(rows_each * cols)).enumerate() {
            scope.spawn(move || {
                for (i, h) in rows.iter_mut().enumerate() {
                    let (n, e) = (south + (part * rows_each + i / cols) as f64 * step, west + (i % cols) as f64 * step);
                    if cover.inside(n, e) < 0.0 {
                        continue;
                    }
                    let near = coastline.nearest(n, e);
                    *h = if sea[i] {
                        let (d, shore) = near.unwrap_or_else(|| offshore.at(n, e));
                        let (start, slope, _) = shore.start();
                        // From the waterline at the slope the land meets it with, down to its start,
                        // then the shelf.
                        let bed = -(slope * d).min(start + SHELF * d);
                        let far = bathymetry.sample(n, e).min(-1.0);
                        let bed = bed + (bed.max(far) - bed) * smoothstep(OPEN.0, OPEN.1, d);
                        (bed + ((*h as f64).min(bed) - bed) * smoothstep(5.0, 15.0, d)) as f32
                    } else if cover.nearest(n, e) == Some(Cover::Water as u8) {
                        continue;
                    } else {
                        match near {
                            // A beach rises from the water, a quay stands by it; both from the waterline
                            // at the bed's slope there, then as the ground is.
                            Some((d, shore)) => {
                                let (_, slope, top) = shore.start();
                                let reach = top / slope;
                                let ramp = (slope * d).min(top);
                                (ramp + ((*h as f64).max(top) - ramp) * smoothstep(reach, reach + 10.0, d)) as f32
                            }
                            _ => h.max(0.5),
                        }
                    };
                }
            });
        }
    });
}

/// Every runway of the aerodromes in the maps' square, each aerodrome at one height: its main
/// runway's in the table (halfway between its ends, or the aerodrome's), unless it is far from the
/// ground's (a mistake of one or the other), then the ground's.
fn aerodromes(tables: &Tables, projection: &Projection, centre: (f64, f64), region: &Region) -> Vec<Airport> {
    let mut list = Vec::new();
    for a in &tables.aerodromes {
        let (n, e) = projection.to_world(a.latitude, a.longitude);
        if (n - centre.0).abs().max((e - centre.1).abs()) > COVER_HALF - 2000.0 {
            continue;
        }
        let runways: Vec<_> = tables.runways_of(a.id).collect();
        let placed = ourairports::place(a, &runways, projection);
        let Some(main) = placed.first() else { continue };
        let m = main.airport;
        let ground = {
            let mut along: Vec<f64> = (0..=8).map(|k| (k as f64 / 8.0 - 0.5) * m.length).map(|d| region.height(m.n + d * m.heading.cos(), m.e + d * m.heading.sin())).collect();
            along.sort_by(f64::total_cmp);
            along[along.len() / 2]
        };
        let surveyed = main.surveyed.then_some(m.elevation);
        let elevation = match surveyed {
            Some(h) if (h - ground).abs() < 20.0 => h,
            _ => ground,
        }
        .max(0.5);
        list.extend(placed.into_iter().map(|p| Airport { elevation, ..p.airport }));
    }
    list
}

/// The share of a town's circle that must be built up: its streets are painted over all of it.
const BUILT: f64 = 0.8;

/// Distance from a point to a runway's paved strip, metres (0 on it).
fn runway_distance(a: &Airport, n: f64, e: f64) -> f64 {
    let (along, across) = a.local(n, e);
    (along.abs() - a.length / 2.0).max(0.0).hypot((across.abs() - a.width / 2.0).max(0.0))
}

/// Towns where the maps say the ground is built up: circles of 220 m to 1 km over it, the biggest
/// that are mostly built (`BUILT`) first, each away from those already laid and from the
/// aerodromes; their streets run along the roads round them.
fn towns(cover: &Grid<u8>, roads: &[Road], airports: &[Airport]) -> Vec<Town> {
    // The share of built ground in cells of 96 m, summed from a corner.
    const CELL: usize = 4;
    let (rows, cols) = (cover.rows / CELL, cover.cols / CELL);
    let cell = cover.step * CELL as f64;
    let mut sum = vec![0.0f64; (rows + 1) * (cols + 1)];
    for r in 0..rows {
        for c in 0..cols {
            let mut built = 0;
            for i in 0..CELL {
                for j in 0..CELL {
                    built += (cover.at(r * CELL + i, c * CELL + j) == Cover::Built as u8) as u32;
                }
            }
            let share = built as f64 / (CELL * CELL) as f64;
            sum[(r + 1) * (cols + 1) + c + 1] = share + sum[r * (cols + 1) + c + 1] + sum[(r + 1) * (cols + 1) + c] - sum[r * (cols + 1) + c];
        }
    }
    // The built share of the square of cells round (r, c) with as much area as a circle of radius `radius`.
    let built = |r: usize, c: usize, radius: f64| -> f64 {
        let half = (radius * 0.886 / cell).round() as usize;
        if r < half || c < half || r + half >= rows || c + half >= cols {
            return 0.0;
        }
        let (r0, r1, c0, c1) = (r - half, r + half + 1, c - half, c + half + 1);
        let s = sum[r1 * (cols + 1) + c1] - sum[r0 * (cols + 1) + c1] - sum[r1 * (cols + 1) + c0] + sum[r0 * (cols + 1) + c0];
        s / ((r1 - r0) * (c1 - c0)) as f64
    };
    const RADII: [f64; 6] = [1000.0, 750.0, 550.0, 400.0, 300.0, 220.0];
    let mut candidates = Vec::new();
    for r in (0..rows).step_by(2) {
        for c in (0..cols).step_by(2) {
            if let Some((radius, share)) = RADII.iter().map(|&radius| (radius, built(r, c, radius))).find(|&(_, share)| share >= BUILT) {
                let (n, e) = (cover.south + (r as f64 + 0.5) * cell, cover.west + (c as f64 + 0.5) * cell);
                candidates.push((radius, share, n, e));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.total_cmp(&a.1)));
    // The roads by square of 1 km, for the direction of the streets.
    let mut by_square: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, road) in roads.iter().enumerate() {
        by_square.entry((((road.n0 + road.n1) / 2000.0).floor() as i32, ((road.e0 + road.e1) / 2000.0).floor() as i32)).or_default().push(i);
    }
    let mut towns: Vec<Town> = Vec::new();
    for (radius, _, n, e) in candidates {
        // Away from the others, and from the aerodromes' flattened ground.
        if towns.iter().any(|t| (t.n - n).hypot(t.e - e) < 0.75 * (t.radius + radius)) || airports.iter().any(|a| a.flat_weight(n, e) > 0.0 || runway_distance(a, n, e) < radius + 150.0) {
            continue;
        }
        towns.push(Town { n, e, radius, grid: street_direction(roads, &by_square, n, e, radius) });
    }
    towns
}

/// The direction (radians from north, 0 to 90°) most of the length of the roads within `radius` of
/// a point runs along, give or take a right angle; 0 without roads.
fn street_direction(roads: &[Road], by_square: &HashMap<(i32, i32), Vec<usize>>, n: f64, e: f64, radius: f64) -> f64 {
    const BINS: usize = 18;
    let mut weight = [0.0; BINS];
    let reach = (radius / 1000.0).ceil() as i32 + 1;
    let (sn, se) = ((n / 1000.0).floor() as i32, (e / 1000.0).floor() as i32);
    for x in sn - reach..=sn + reach {
        for y in se - reach..=se + reach {
            for &i in by_square.get(&(x, y)).into_iter().flatten() {
                let r = &roads[i];
                if r.distance(n, e) > radius {
                    continue;
                }
                let (dn, de) = (r.n1 - r.n0, r.e1 - r.e0);
                let angle = de.atan2(dn).rem_euclid(std::f64::consts::FRAC_PI_2);
                weight[((angle / std::f64::consts::FRAC_PI_2 * BINS as f64) as usize).min(BINS - 1)] += dn.hypot(de);
            }
        }
    }
    let best = (0..BINS).max_by(|&a, &b| weight[a].total_cmp(&weight[b])).unwrap_or(0);
    if weight[best] == 0.0 {
        return 0.0;
    }
    (best as f64 + 0.5) / BINS as f64 * std::f64::consts::FRAC_PI_2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reads_and_grows() {
        let p = Progress { stage: Stage::Heights, done: 12, total: 300, downloaded: 4_200_000 };
        assert_eq!(p.text(), "terrain 12/300, 4 MB downloaded");
        let stages = [Stage::Aerodromes, Stage::Heights, Stage::Maps, Stage::Building, Stage::Done];
        let shares: Vec<f64> = stages.iter().map(|&stage| Progress { stage, done: 1, total: 2, downloaded: 0 }.share()).collect();
        assert!(shares.windows(2).all(|w| w[0] < w[1]) && shares[4] == 1.0);
    }

    #[test]
    fn towns_cover_the_built_ground_and_follow_its_streets() {
        let mut cover = Grid::centred(0.0, 0.0, 9600.0, 24.0, Cover::Farmland as u8);
        for r in 0..cover.rows {
            for c in 0..cover.cols {
                let (n, e) = cover.node(r, c);
                if (n - 1000.0).hypot(e + 500.0) < 900.0 {
                    cover.set(r, c, Cover::Built as u8);
                }
            }
        }
        // Streets 30° east of north, and across them.
        let (s, k) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
        let roads: Vec<Road> = (-5..=5).flat_map(|i| {
            let o = i as f64 * 150.0;
            [Road { n0: 1000.0 - 800.0 * k - o * s, e0: -500.0 - 800.0 * s + o * k, n1: 1000.0 + 800.0 * k - o * s, e1: -500.0 + 800.0 * s + o * k, width: 7.0 }]
        }).collect();
        let towns = towns(&cover, &roads, &[]);
        assert!(!towns.is_empty());
        let big = towns[0];
        assert!((big.n - 1000.0).hypot(big.e + 500.0) < 200.0, "{big:?}");
        assert!(big.radius >= 550.0, "{big:?}");
        assert!((big.grid.to_degrees() - 30.0).abs() < 5.0, "{}", big.grid.to_degrees());
        // Nothing out in the fields.
        assert!(towns.iter().all(|t| (t.n - 1000.0).hypot(t.e + 500.0) < 900.0));
    }

    /// A map of a sea to the west of a coastline 20° off north, a beach on its southern half and a
    /// town (a quay) on its northern; its cover drawn at 24 m, as `build` does.
    fn shore_map() -> (Vec<MapTile>, Grid<u8>) {
        let (s, c) = (20f64.to_radians().sin(), 20f64.to_radians().cos());
        // The coast through (0, 0) along north-north-east; the sea a big polygon west of it.
        let along = |t: f64| [t * c, t * s];
        let (a, b) = (along(-4000.0), along(4000.0));
        let ring = vec![a, b, [b[0], -6000.0], [a[0], -6000.0]];
        let beach = vec![a, along(0.0), [0.0, 60.0], [a[0], a[1] + 60.0]];
        let town = vec![along(0.0), b, [b[0], b[1] + 300.0], [0.0, 300.0]];
        let map = MapTile {
            areas: vec![Area { layer: 0, cover: Cover::Sand, rings: vec![beach], works: false }, Area { layer: 1, cover: Cover::Built, rings: vec![town], works: true }, Area { layer: 3, cover: Cover::Sea, rings: vec![ring], works: false }],
            roads: Vec::new(),
            coast: vec![[a, along(0.0)], [along(0.0), b]],
            bridges: false,
        };
        let maps = vec![map];
        let cover = fill_cover(Grid::centred(0.0, 0.0, 10_000.0, 24.0, Cover::Farmland as u8), &maps);
        (maps, cover)
    }

    #[test]
    fn the_coast_follows_the_map_smoothly_deep_by_a_quay_shallow_off_a_beach() {
        let (maps, cover) = shore_map();
        let mut heights = Grid::centred(0.0, 0.0, 8000.0, 12.0, 0.2f32);
        let bed = Grid::centred(0.0, 0.0, 20_000.0, 400.0, -20.0f32);
        let sea = sea_mask(&heights, &maps);
        let works = works_grid(&maps, &cover);
        coast(&mut heights, &sea, &cover, &Coastline::new(&maps, &cover, &works), &Offshore::new(&cover, &works), &bed);
        let (s, c) = (20f64.to_radians().sin(), 20f64.to_radians().cos());
        // Along rows every 3 m north, where the heights cross 0 going east: on the line, within a
        // metre or two, with no steps of a node (12 m).
        let mut worst: f64 = 0.0;
        for k in -200..200 {
            let n = k as f64 * 3.0;
            let line = n * s / c;
            let mut e = line - 150.0;
            while heights.sample(n, e) < 0.0 && e < line + 150.0 {
                e += 0.25;
            }
            worst = worst.max((e - line).abs());
        }
        assert!(worst < 3.0, "the waterline strays {worst} m from the map's");
        // Off the quay (north) deep at once; off the beach (south) shallow, down the shelf.
        let off = |t: f64, d: f64| heights.sample(t * c + d * s, t * s - d * c);
        assert!(off(800.0, 40.0) < -10.0, "{}", off(800.0, 40.0));
        assert!(off(-800.0, 40.0) > -3.0 && off(-800.0, 40.0) < -1.0, "{}", off(-800.0, 40.0));
        // The quay stands; the beach rises from the water.
        assert!(off(800.0, -20.0) >= Shore::Quay.start().2 - 0.01);
        assert!(off(-800.0, -12.0) < 1.5 && off(-800.0, -12.0) > 0.0);
        // Far out the far grid's bathymetry where it is shallower than the shelf; near the quay
        // it is not used (40 m off, deeper than its 20 m).
        assert_eq!(heights.sample(0.0, -3900.0), -20.0);
        assert!(heights.sample(0.0, -1000.0) < -30.0);
    }
}
