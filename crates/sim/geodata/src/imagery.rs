//! The colour of a region's ground from Sentinel-2 (`sentinel`): a cloud-free picture made of
//! several summer scenes, laid on the world's grids and kept on disk.
//!
//! | | side | step | days | from |
//! |---|---|---|---|---|
//! | near | 30.7 km | 10 m | 2 | the true colour picture at 10 m, the classes at 20 m |
//! | far | 98 km | 32 m | 3 | the picture's overview at 40 m, the classes' at 40 m |
//!
//! For every texel and every scene over it: the picture's colour (bilinear), turned into
//! reflectance (it is reflectance times 1020), and where the picture clipped (255, a bright field,
//! sand, a roof) the band's own reflectance at 80 m instead, if more; and the class of the ground
//! there. The texel takes the scene whose brightness is the median of those that see the ground
//! (no cloud, no cloud shadow, no cirrus), so that a cloud, a shadow, a ploughed field of one week
//! or a harvest are outvoted; if none sees it, of the doubtful ones; if none, the darkest (a cloud
//! is bright). A near texel no scene saw is left unknown: the far grid is drawn there.
//!
//! The colour is the reflectance as the sensor measured it over the ground's surface after the
//! correction for the air (Sen2Cor's), which is the albedo the renderer lights: it is kept as sRGB
//! so that the darks keep their steps in 8 bits.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sim_core::{Cover, Grid, Imagery, Region};

use crate::fetch::{cache_dir, Fetcher};
use crate::geo::Projection;
use crate::sentinel::{self, seen, Asset, Scene, Seen};
use crate::utm::Zone;
use crate::{disk, threads, Error};

/// A grid of the picture: its nodes a side, their step (metres), the pixel of the pictures it is read
/// from, and how many days of photographs. The near one takes two: where neither sees the ground,
/// the far one is drawn. Both have as many nodes, so that the renderer holds them as one texture.
struct Level {
    nodes: usize,
    step: f64,
    source: f64,
    /// How many of the best days it is made of.
    days: usize,
}

const LEVELS: [Level; 2] = [Level { nodes: 3072, step: 10.0, source: 10.0, days: 2 }, Level { nodes: 3072, step: 32.0, source: 40.0, days: 3 }];
/// The days of photographs a picture is made of.
const DATES: usize = 3;
/// The picture saturates at this reflectance (255): it is reflectance times 1020.
const PICTURE_SCALE: f64 = 1020.0;
/// The overview of the bands that gives back what the picture clipped, metres a pixel.
const BANDS_PIXEL: f64 = 80.0;
/// The summers looked at: the three up to 2025, the last whole one when this was written. Fixed, so
/// that a place looks the same on every computer whenever its picture is made.
const LAST_SUMMER: i32 = 2025;
const SUMMERS: i32 = 3;
/// Downloads at once.
const DOWNLOADS: usize = 4;
/// How far in from the edge of a scene it gives way to the next scene of the day, metres.
const RAMP: f64 = 4000.0;
/// Rows of texels made at once.
const BAND: usize = 128;
/// Nodes between two points where the projection to UTM is worked out exactly; between them it
/// is interpolated (bilinear over 320 m of a nearly affine map: under a millimetre off).
const LATTICE: usize = 32;

/// What is being done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Asking the catalogue for the scenes.
    Searching,
    /// Fetching the parts of the scenes (`done` of `total`).
    Downloading,
    /// Making the picture (`done` of `total` rows of bands).
    Composing,
    Done,
}

/// How far it has got.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    pub done: usize,
    pub total: usize,
    /// Bytes downloaded so far.
    pub downloaded: u64,
}

impl Progress {
    /// What the player reads.
    pub fn text(&self) -> String {
        let what = match self.stage {
            Stage::Searching => "looking for satellite pictures",
            Stage::Downloading => "satellite pictures",
            Stage::Composing => "putting the satellite picture together",
            Stage::Done => "satellite picture ready",
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
            Stage::Searching => 0.0,
            Stage::Downloading => 0.05 + 0.75 * part,
            Stage::Composing => 0.8 + 0.2 * part,
            Stage::Done => 1.0,
        }
    }
}

/// A picture and how it came.
pub struct Loaded {
    pub imagery: Imagery,
    /// Bytes downloaded for it (0 when it was kept).
    pub downloaded: u64,
    /// It was read whole from the cache.
    pub kept: bool,
}

/// The picture of a region's ground: the one made before, from the cache, or a new one, round the
/// middle of the region's maps. `progress` is called from the loading threads.
pub fn load(region: &Region, offline: bool, progress: &(dyn Fn(Progress) + Sync)) -> Result<Loaded, Error> {
    let fetcher = Fetcher::new(cache_dir(), offline);
    let centre = centre(region);
    let (lat, lon) = (region.latitude.to_degrees(), region.longitude.to_degrees());
    let file = fetcher.root().join("imagery").join(format!("{lat:.5}_{lon:.5}_{:.0}_{:.0}.sat", centre.0, centre.1));
    if let Ok(Some(imagery)) = disk::read(&file).map_err(|e| Error::Io(e.to_string())).and_then(|b| decode(&b)) {
        progress(Progress { stage: Stage::Done, done: 1, total: 1, downloaded: 0 });
        return Ok(Loaded { imagery, downloaded: 0, kept: true });
    }
    let imagery = make(region, &Projection::new(lat, lon), centre, &fetcher, progress)?;
    disk::write(&file, &encode(&imagery)?).map_err(|e| Error::Io(format!("{}: {e}", file.display())))?;
    progress(Progress { stage: Stage::Done, done: 1, total: 1, downloaded: fetcher.downloaded() });
    Ok(Loaded { imagery, downloaded: fetcher.downloaded(), kept: false })
}

/// What the Copernicus Sentinel data's notice asks to be said where a picture made of them is
/// shown, for the years of its scenes (`Imagery::years`); `None` without any.
pub fn attribution(years: &[u16]) -> Option<String> {
    let (first, last) = (years.iter().min()?, years.iter().max()?);
    Some(if first == last { format!("Contains modified Copernicus Sentinel data {first}") } else { format!("Contains modified Copernicus Sentinel data {first}-{last}") })
}

/// The middle of a region's maps, north and east.
fn centre(region: &Region) -> (f64, f64) {
    let g = &region.cover;
    (g.south + (g.rows - 1) as f64 * g.step / 2.0, g.west + (g.cols - 1) as f64 * g.step / 2.0)
}

/// A tile of a file of a scene: the scene, the asset (`reads`), the level, its column and row.
type Part = (usize, usize, usize, u32, u32);

/// A scene's files, opened.
struct Opened {
    scene: Scene,
    visual: Asset,
    scl: Asset,
    bands: [Asset; 3],
}

/// What a level reads of a scene: the asset (0 picture, 1 classes, 2..4 bands) and its level.
fn reads(o: &Opened, pixel: f64) -> [(usize, usize); 5] {
    let bands = o.bands[0].level_for(BANDS_PIXEL);
    [(0, o.visual.level_for(pixel)), (1, o.scl.level_for(pixel.max(20.0))), (2, bands), (3, bands), (4, bands)]
}

impl Opened {
    fn asset(&self, i: usize) -> &Asset {
        match i {
            0 => &self.visual,
            1 => &self.scl,
            k => &self.bands[k - 2],
        }
    }
}

fn make(region: &Region, projection: &Projection, centre: (f64, f64), fetcher: &Fetcher, progress: &(dyn Fn(Progress) + Sync)) -> Result<Imagery, Error> {
    let report = |stage: Stage, done: usize, total: usize| progress(Progress { stage, done, total, downloaded: fetcher.downloaded() });
    report(Stage::Searching, 0, 1);
    let grids: Vec<Grid<[u8; 4]>> = LEVELS.iter().map(|l| Grid::centred(centre.0, centre.1, (l.nodes - 1) as f64 * l.step, l.step, [0u8; 4])).collect();
    // The box of latitudes and longitudes of the widest grid.
    let wide = &grids[grids.len() - 1];
    let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for (r, c) in [(0, 0), (0, wide.cols - 1), (wide.rows - 1, 0), (wide.rows - 1, wide.cols - 1), (0, wide.cols / 2), (wide.rows - 1, wide.cols / 2)] {
        let (n, e) = wide.node(r, c);
        let (lat, lon) = projection.to_earth(n, e);
        bbox = [bbox[0].min(lat), bbox[1].min(lon), bbox[2].max(lat), bbox[3].max(lon)];
    }
    let found = sentinel::search(fetcher, bbox, LAST_SUMMER + 1, SUMMERS)?;
    let chosen = sentinel::choose(found, DATES);
    if chosen.is_empty() {
        return Err(Error::Data("no Sentinel-2 scene without clouds".into()));
    }
    let opened = parallel(&chosen, |s| {
        Ok(Opened {
            scene: s.clone(),
            visual: Asset::open(fetcher, s, "visual", &s.visual)?,
            scl: Asset::open(fetcher, s, "scl", &s.scl)?,
            bands: [Asset::open(fetcher, s, "red", &s.bands[0])?, Asset::open(fetcher, s, "green", &s.bands[1])?, Asset::open(fetcher, s, "blue", &s.bands[2])?],
        })
    })?;
    // Only what is land, or may be: the open sea is the sea's own to draw.
    let land = |n: f64, e: f64| match region.cover_at(n, e) {
        Cover::Sea => false,
        Cover::Unknown => region.height(n, e) > -2.0,
        _ => true,
    };
    let maps: Vec<Mapping> = grids.iter().zip(&LEVELS).map(|(g, l)| Mapping::new(projection, g, &opened, l.days, &land)).collect();

    // Every tile of every file a grid touches, fetched first (and kept), a few at a time.
    let mut wanted: Vec<Part> = Vec::new();
    for (level, (l, g)) in LEVELS.iter().zip(&grids).enumerate() {
        for (s, o) in opened.iter().enumerate() {
            for (asset, lv) in reads(o, l.source) {
                wanted.extend(maps[level].tiles(o, s, asset, lv, 0, g.rows).into_iter().map(|(tx, ty)| (s, asset, lv, tx, ty)));
            }
        }
    }
    wanted.sort_unstable();
    wanted.dedup();
    let done = AtomicUsize::new(0);
    parallel(&wanted, |&(s, asset, lv, tx, ty)| {
        opened[s].asset(asset).tile(fetcher, lv, tx, ty)?;
        report(Stage::Downloading, done.fetch_add(1, Ordering::Relaxed) + 1, wanted.len());
        Ok(())
    })?;

    let total: usize = grids.iter().map(|g| g.rows.div_ceil(BAND)).sum();
    let done = AtomicUsize::new(0);
    let mut levels = Vec::new();
    for (level, mut grid) in grids.into_iter().enumerate() {
        compose(&mut grid, &LEVELS[level], &opened, &maps[level], fetcher, &|| report(Stage::Composing, done.fetch_add(1, Ordering::Relaxed) + 1, total))?;
        levels.push(grid);
    }
    let mut years: Vec<u16> = opened.iter().map(|o| o.scene.year()).collect();
    years.sort_unstable();
    years.dedup();
    Ok(Imagery { levels, years })
}

/// Where the nodes of a grid fall in each UTM zone of the scenes: worked out exactly every
/// `LATTICE` nodes, interpolated between; and which squares of that lattice are wanted (land).
struct Mapping {
    rows: usize,
    cols: usize,
    /// For each zone, its easting and northing at the lattice's points, row after row.
    zones: Vec<(Zone, Vec<(f64, f64)>)>,
    /// For each square between four points of the lattice, whether any of it is land.
    land: Vec<bool>,
    /// For each scene, the squares of land it gives the picture: of the scenes of one day that
    /// hold a square whole and `RAMP` in from their edge (two squares of MGRS overlap by 10 km),
    /// only the first; none for a scene of a day the grid does not take.
    supplies: Vec<Vec<bool>>,
}

impl Mapping {
    fn new(projection: &Projection, grid: &Grid<[u8; 4]>, opened: &[Opened], days: usize, land: &dyn Fn(f64, f64) -> bool) -> Mapping {
        let (rows, cols) = (grid.rows.div_ceil(LATTICE) + 1, grid.cols.div_ceil(LATTICE) + 1);
        let mut all: Vec<(Zone, Vec<(f64, f64)>)> = Vec::new();
        for zone in opened.iter().map(|o| o.scene.zone) {
            if all.iter().any(|z| z.0 == zone) {
                continue;
            }
            let points = (0..rows * cols)
                .map(|i| {
                    let (n, e) = (grid.south + ((i / cols) * LATTICE) as f64 * grid.step, grid.west + ((i % cols) * LATTICE) as f64 * grid.step);
                    let (lat, lon) = projection.to_earth(n, e);
                    zone.project(lat, lon)
                })
                .collect();
            all.push((zone, points));
        }
        // A square is land if land is found at any of 9 by 9 points over it and a little round it.
        let side = LATTICE as f64 * grid.step;
        let land: Vec<bool> = (0..rows * cols)
            .map(|i| {
                let (n, e) = (grid.south + ((i / cols) * LATTICE) as f64 * grid.step, grid.west + ((i % cols) * LATTICE) as f64 * grid.step);
                (0..81).any(|k| land(n + side * ((k / 9) as f64 / 8.0 * 1.2 - 0.1), e + side * ((k % 9) as f64 / 8.0 * 1.2 - 0.1)))
            })
            .collect();
        let mut map = Mapping { rows, cols, zones: all, land, supplies: Vec::new() };
        let mut dates: Vec<&str> = Vec::new();
        for o in opened {
            if !dates.contains(&o.scene.date.as_str()) {
                dates.push(&o.scene.date);
            }
        }
        let taken = &dates[..days.min(dates.len())];
        // Whether a scene's picture holds a square of the lattice whole, `RAMP` in from its edge.
        let holds = |o: &Opened, i: usize| {
            let (r, c) = (i / cols, i % cols);
            let zone = map.zone_of(o.scene.zone);
            [(0, 0), (0, 1), (1, 0), (1, 1)].iter().all(|(dr, dc)| {
                let (e, n) = map.at(zone, (r + dr) * LATTICE, (c + dc) * LATTICE);
                inside(&o.visual.cog, e, n) >= RAMP
            })
        };
        let supplies = opened
            .iter()
            .enumerate()
            .map(|(s, o)| {
                if !taken.contains(&o.scene.date.as_str()) {
                    return vec![false; rows * cols];
                }
                (0..rows * cols).map(|i| map.land[i] && !opened[..s].iter().any(|before| before.scene.date == o.scene.date && holds(before, i))).collect()
            })
            .collect();
        map.supplies = supplies;
        map
    }

    /// Whether scene `s` gives node (row, column) its look.
    #[inline]
    fn gives(&self, s: usize, row: usize, col: usize) -> bool {
        self.supplies[s][(row / LATTICE) * self.cols + col / LATTICE]
    }

    /// Easting and northing of node (row, column) in a zone.
    #[inline]
    fn at(&self, zone: usize, row: usize, col: usize) -> (f64, f64) {
        let points = &self.zones[zone].1;
        let (r, c) = (row / LATTICE, col / LATTICE);
        let (fr, fc) = ((row % LATTICE) as f64 / LATTICE as f64, (col % LATTICE) as f64 / LATTICE as f64);
        let p = |r: usize, c: usize| points[r.min(self.rows - 1) * self.cols + c.min(self.cols - 1)];
        let (a, b, c2, d) = (p(r, c), p(r, c + 1), p(r + 1, c), p(r + 1, c + 1));
        let lerp = |x: f64, y: f64, t: f64| x + (y - x) * t;
        (lerp(lerp(a.0, b.0, fc), lerp(c2.0, d.0, fc), fr), lerp(lerp(a.1, b.1, fc), lerp(c2.1, d.1, fc), fr))
    }

    /// Whether node (row, column) is in a square of land.
    #[inline]
    fn wanted(&self, row: usize, col: usize) -> bool {
        self.land[(row / LATTICE) * self.cols + col / LATTICE]
    }

    fn zone_of(&self, zone: Zone) -> usize {
        self.zones.iter().position(|z| z.0 == zone).unwrap_or(0)
    }

    /// The tiles of a level of an asset that the squares of land of rows `from..to` of the grid
    /// touch, with two pixels to spare for the bilinear reading.
    fn tiles(&self, o: &Opened, s: usize, asset: usize, level: usize, from: usize, to: usize) -> Vec<(u32, u32)> {
        let cog = &o.asset(asset).cog;
        let lv = &cog.levels[level];
        let pixel = cog.pixel_of(level);
        let zone = self.zone_of(o.scene.zone);
        let (w, h) = (lv.width as f64, lv.height as f64);
        let (tw, th) = (lv.tile_width as f64, lv.tile_height as f64);
        let (across, down) = lv.tiles();
        let tx = |x: f64| ((x / tw).floor().max(0.0) as u32).min(across - 1);
        let ty = |y: f64| ((y / th).floor().max(0.0) as u32).min(down - 1);
        let mut found = Vec::new();
        for r in from / LATTICE..to.div_ceil(LATTICE).min(self.rows - 1) {
            for c in 0..self.cols - 1 {
                if !self.supplies[s][r * self.cols + c] {
                    continue;
                }
                let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for (dr, dc) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                    let (e, n) = self.at(zone, (r + dr) * LATTICE, (c + dc) * LATTICE);
                    let (x, y) = ((e - cog.west) / pixel, (cog.north - n) / pixel);
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
                if x1 < -2.0 || y1 < -2.0 || x0 > w + 2.0 || y0 > h + 2.0 {
                    continue;
                }
                found.extend((ty(y0 - 2.0)..=ty(y1 + 2.0)).flat_map(|y| (tx(x0 - 2.0)..=tx(x1 + 2.0)).map(move |x| (x, y))));
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }
}

/// The decoded tiles of one level of one asset that a band of rows reads, by column and row.
struct Window {
    x0: u32,
    y0: u32,
    across: u32,
    tiles: Vec<Option<Arc<Vec<u8>>>>,
    tile_width: u32,
    tile_height: u32,
    width: u32,
    height: u32,
    samples: usize,
    bytes: usize,
    /// Metres a pixel, and the projected corner of the image.
    pixel: f64,
    west: f64,
    north: f64,
}

impl Window {
    /// Sample `k` of pixel (x, y), as a number; `None` outside the image or a tile not there.
    #[inline]
    fn get(&self, x: i64, y: i64, k: usize) -> Option<u32> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let (x, y) = (x as u32, y as u32);
        let (tx, ty) = (x / self.tile_width, y / self.tile_height);
        if tx < self.x0 || ty < self.y0 || tx >= self.x0 + self.across {
            return None;
        }
        let tile = self.tiles.get(((ty - self.y0) * self.across + tx - self.x0) as usize)?.as_ref()?;
        let i = (((y % self.tile_height) * self.tile_width + x % self.tile_width) as usize * self.samples + k) * self.bytes;
        Some(if self.bytes == 2 { u16::from_le_bytes([tile[i], tile[i + 1]]) as u32 } else { tile[i] as u32 })
    }

    /// The four pixels round a projected point and their weights.
    #[inline]
    fn around(&self, e: f64, n: f64) -> ([(i64, i64); 4], [f64; 4]) {
        let (x, y) = ((e - self.west) / self.pixel - 0.5, (self.north - n) / self.pixel - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        ([(x0, y0), (x0 + 1, y0), (x0, y0 + 1), (x0 + 1, y0 + 1)], [(1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy])
    }

    /// The nearest pixel's sample.
    #[inline]
    fn nearest(&self, e: f64, n: f64, k: usize) -> Option<u32> {
        let (x, y) = ((e - self.west) / self.pixel, (self.north - n) / self.pixel);
        self.get(x.floor() as i64, y.floor() as i64, k)
    }
}

/// One scene's look at a texel: reflectance in ten-thousandths, and what its classes say.
#[derive(Clone, Copy)]
struct Look {
    rgb: [u16; 3],
    seen: Seen,
    /// The day it was taken (the index of its first scene), and how far it is from the edge of
    /// its scene, 0 to 1 over `RAMP`.
    day: usize,
    weight: f32,
}

impl Look {
    fn brightness(&self) -> u32 {
        // Rec. 709's weights, in thousandths.
        (213 * self.rgb[0] as u32 + 715 * self.rgb[1] as u32 + 72 * self.rgb[2] as u32) / 1000
    }
}

/// Makes a grid's texels, a band of rows at a time: the tiles the band reads are decoded (on as
/// many threads as there are cores) and the rows made from them.
fn compose(grid: &mut Grid<[u8; 4]>, level: &Level, opened: &[Opened], map: &Mapping, fetcher: &Fetcher, step: &dyn Fn()) -> Result<(), Error> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    let mut decoded: HashMap<Part, Arc<Vec<u8>>> = HashMap::new();
    let cols = grid.cols;
    for start in (0..grid.rows).step_by(BAND) {
        let end = (start + BAND).min(grid.rows);
        // The windows of this band: for each scene used, its five reads.
        let mut needed = Vec::new();
        let mut shapes = Vec::new();
        for (s, o) in opened.iter().enumerate() {
            for (asset, lv) in reads(o, level.source) {
                let tiles = map.tiles(o, s, asset, lv, start, end);
                needed.extend(tiles.iter().map(|&(x, y)| (s, asset, lv, x, y)));
                shapes.push((s, asset, lv, tiles));
            }
        }
        decoded.retain(|k, _| needed.contains(k));
        let missing: Vec<_> = needed.iter().copied().filter(|k| !decoded.contains_key(k)).collect();
        let fresh = Mutex::new(Vec::new());
        let next = AtomicUsize::new(0);
        let failed = Mutex::new(None);
        threads::scope(|scope| {
            for _ in 0..threads.min(missing.len()) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(s, asset, lv, x, y)) = missing.get(i) else { break };
                    let a = opened[s].asset(asset);
                    let result = a.tile(fetcher, lv, x, y).and_then(|bytes| bytes.map(|b| a.cog.levels[lv].decode(&b)).transpose());
                    match result {
                        Ok(Some(pixels)) => fresh.lock().unwrap_or_else(|e| e.into_inner()).push(((s, asset, lv, x, y), Arc::new(pixels))),
                        Ok(None) => {}
                        Err(e) => *failed.lock().unwrap_or_else(|e| e.into_inner()) = Some(e),
                    }
                });
            }
        });
        if let Some(e) = failed.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(e);
        }
        decoded.extend(fresh.into_inner().unwrap_or_else(|e| e.into_inner()));

        // One window for each read of each scene.
        let windows: Vec<(usize, Vec<Window>)> = opened
            .iter()
            .enumerate()
            .map(|(s, o)| {
                let list = shapes
                    .iter()
                    .filter(|w| w.0 == s)
                    .map(|(_, asset, lv, tiles)| {
                        let cog = &o.asset(*asset).cog;
                        let l = &cog.levels[*lv];
                        let (x0, y0) = tiles.iter().fold((u32::MAX, u32::MAX), |m, t| (m.0.min(t.0), m.1.min(t.1)));
                        let x1 = tiles.iter().map(|t| t.0).max().unwrap_or(0);
                        let y1 = tiles.iter().map(|t| t.1).max().unwrap_or(0);
                        let across = if tiles.is_empty() { 0 } else { x1 - x0 + 1 };
                        let down = if tiles.is_empty() { 0 } else { y1 - y0 + 1 };
                        let tiles = (0..across * down).map(|i| decoded.get(&(s, *asset, *lv, x0 + i % across, y0 + i / across)).cloned()).collect();
                        Window { x0, y0, across, tiles, tile_width: l.tile_width, tile_height: l.tile_height, width: l.width, height: l.height, samples: l.samples as usize, bytes: l.bytes as usize, pixel: cog.pixel_of(*lv), west: cog.west, north: cog.north }
                    })
                    .collect();
                (map.zone_of(o.scene.zone), list)
            })
            .collect();

        let rows = &mut grid.data[start * cols..end * cols];
        let day_of: Vec<usize> = opened.iter().map(|o| opened.iter().position(|p| p.scene.date == o.scene.date).unwrap_or(0)).collect();
        let day_of = &day_of;
        let rows_each = (end - start).div_ceil(threads);
        threads::scope(|scope| {
            for (part, chunk) in rows.chunks_mut(rows_each * cols).enumerate() {
                let windows = &windows;
                scope.spawn(move || {
                    let mut looks: Vec<Look> = Vec::with_capacity(windows.len());
                    for (k, texel) in chunk.iter_mut().enumerate() {
                        let (row, col) = (start + part * rows_each + k / cols, k % cols);
                        looks.clear();
                        if !map.wanted(row, col) {
                            *texel = [0; 4];
                            continue;
                        }
                        for (s, (zone, w)) in windows.iter().enumerate() {
                            if !map.gives(s, row, col) {
                                continue;
                            }
                            let (e, n) = map.at(*zone, row, col);
                            if let Some(mut look) = look(w, e, n) {
                                look.day = day_of[s];
                                look.weight = (inside(&opened[s].visual.cog, e, n) / RAMP).clamp(0.02, 1.0) as f32;
                                looks.push(look);
                            }
                        }
                        merge_days(&mut looks);
                        *texel = choose(&mut looks).map_or([0; 4], |l| {
                            let [r, g, b] = l.rgb.map(|v| srgb(v as f64 / 10_000.0));
                            [r, g, b, 255]
                        });
                    }
                });
            }
        });
        step();
    }
    Ok(())
}

/// What a scene shows at a projected point: the picture's colour as reflectance, given back from
/// the bands where it clipped, and its class. `None` where it has no data.
#[inline]
fn look(w: &[Window], e: f64, n: f64) -> Option<Look> {
    let (picture, classes, bands) = (&w[0], &w[1], &w[2..5]);
    let (points, weights) = picture.around(e, n);
    let mut rgb = [0u16; 3];
    for (k, value) in rgb.iter_mut().enumerate() {
        let (mut sum, mut clipped) = (0.0, false);
        for (p, wt) in points.iter().zip(weights).filter(|(_, wt)| *wt > 0.0) {
            // 0 is no data.
            let v = picture.get(p.0, p.1, k).filter(|&v| v > 0)?;
            clipped |= v == 255;
            sum += v as f64 * wt;
        }
        let mut reflectance = sum / PICTURE_SCALE;
        if clipped {
            let (points, weights) = bands[k].around(e, n);
            let band: f64 = points.iter().zip(weights).map(|(p, wt)| bands[k].get(p.0, p.1, 0).unwrap_or(0) as f64 * wt).sum();
            reflectance = reflectance.max(band / 10_000.0);
        }
        *value = (reflectance * 10_000.0).round().min(10_000.0) as u16;
    }
    let seen = classes.nearest(e, n, 0).map_or(Seen::Doubtful, |c| seen(c as u8));
    (seen != Seen::Nothing).then_some(Look { rgb, seen, day: 0, weight: 1.0 })
}

/// How far a projected point is inside a scene's picture, metres from its nearest edge (less
/// than 0 outside).
fn inside(cog: &crate::tiff::Cog, e: f64, n: f64) -> f64 {
    let extent = cog.pixel * cog.levels[0].width as f64;
    (e - cog.west).min(cog.west + extent - e).min(cog.north - n).min(n - (cog.north - extent))
}

/// The looks of one day from two scenes (where squares of MGRS overlap) made one, each weighed
/// by how far it is from the edge of its scene: the correction for the air is worked out for
/// each scene on its own, and the same ground comes out a little brighter or bluer in one than
/// in the other. Blended over `RAMP`, the change is not seen.
fn merge_days(looks: &mut Vec<Look>) {
    let mut i = 0;
    while i < looks.len() {
        let mut j = i + 1;
        while j < looks.len() {
            if looks[j].day != looks[i].day {
                j += 1;
                continue;
            }
            let (a, b) = (looks[i], looks.swap_remove(j));
            let t = b.weight / (a.weight + b.weight);
            let rgb = [0, 1, 2].map(|k| (a.rgb[k] as f32 + (b.rgb[k] as f32 - a.rgb[k] as f32) * t).round() as u16);
            looks[i] = Look { rgb, seen: if t > 0.5 { b.seen } else { a.seen }, day: a.day, weight: a.weight + b.weight };
        }
        i += 1;
    }
}

/// The look a texel takes of those it has: the median brightness of those that see the ground,
/// or of the doubtful ones, or the darkest.
fn choose(looks: &mut [Look]) -> Option<Look> {
    looks.sort_by_key(|l| (l.seen, l.brightness()));
    let best = looks.first()?.seen;
    if best == Seen::Cloud {
        return looks.iter().min_by_key(|l| l.brightness()).copied();
    }
    let same = looks.iter().take_while(|l| l.seen == best).count();
    // The lower of the two middle ones when there are two: a shadow missed is darker, a haze brighter.
    Some(looks[(same - 1) / 2])
}

/// sRGB's encoding of a linear value, 0 to 1, as a byte.
fn srgb(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// Runs `job` for every item, `DOWNLOADS` at a time, and gives back the results in the items' order.
fn parallel<I: Sync, T: Send>(items: &[I], job: impl Fn(&I) -> Result<T, Error> + Sync) -> Result<Vec<T>, Error> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(items.len()));
    let failed: Mutex<Option<Error>> = Mutex::new(None);
    threads::scope(|scope| {
        for _ in 0..DOWNLOADS.min(items.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() || failed.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                    break;
                }
                match job(&items[i]) {
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
    Ok(results.into_iter().map(|(_, t)| t).collect())
}

const MAGIC: &[u8; 4] = b"FLSI";
/// Changes whenever the file or what `make` makes of the scenes changes.
const VERSION: u32 = 1;

/// The picture as kept on disk: a magic word and the version, the years, then each grid's
/// placing and its texels as a PNG (RGBA, 8 bits), little-endian numbers.
pub fn encode(imagery: &Imagery) -> Result<Vec<u8>, Error> {
    let mut out = MAGIC.to_vec();
    out.extend(VERSION.to_le_bytes());
    out.extend((imagery.years.len() as u32).to_le_bytes());
    for y in &imagery.years {
        out.extend(y.to_le_bytes());
    }
    out.extend((imagery.levels.len() as u32).to_le_bytes());
    for g in &imagery.levels {
        for v in [g.south, g.west, g.step] {
            out.extend(v.to_le_bytes());
        }
        out.extend((g.rows as u32).to_le_bytes());
        out.extend((g.cols as u32).to_le_bytes());
        let mut png_bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut png_bytes, g.cols as u32, g.rows as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let bad = |e: png::EncodingError| Error::Io(format!("PNG: {e}"));
        let mut writer = encoder.write_header().map_err(bad)?;
        writer.write_image_data(g.data.as_flattened()).map_err(bad)?;
        writer.finish().map_err(bad)?;
        out.extend((png_bytes.len() as u64).to_le_bytes());
        out.extend(png_bytes);
    }
    Ok(out)
}

/// The picture of a file `encode` wrote; `None` for a file of another version.
pub fn decode(bytes: &[u8]) -> Result<Option<Imagery>, Error> {
    let short = || Error::Data("saved satellite picture is incomplete".into());
    let mut p = 0usize;
    let mut take = |n: usize| -> Result<&[u8], Error> {
        let s = bytes.get(p..p + n).ok_or_else(short)?;
        p += n;
        Ok(s)
    };
    if take(4)? != MAGIC || take(4)? != VERSION.to_le_bytes() {
        return Ok(None);
    }
    let u32_of = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap_or_default());
    let years = (0..u32_of(take(4)?)).map(|_| take(2).map(|s| u16::from_le_bytes([s[0], s[1]]))).collect::<Result<Vec<_>, _>>()?;
    let count = u32_of(take(4)?);
    let mut levels = Vec::new();
    for _ in 0..count {
        let f = |s: &[u8]| f64::from_le_bytes(s.try_into().unwrap_or_default());
        let (south, west, step) = (f(take(8)?), f(take(8)?), f(take(8)?));
        let (rows, cols) = (u32_of(take(4)?) as usize, u32_of(take(4)?) as usize);
        let length = u64::from_le_bytes(take(8)?.try_into().unwrap_or_default()) as usize;
        let png_bytes = take(length)?;
        let bad = |e: &dyn std::fmt::Display| Error::Data(format!("satellite picture: {e}"));
        let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().map_err(|e| bad(&e))?;
        let mut data = vec![[0u8; 4]; rows * cols];
        let info = reader.next_frame(data.as_flattened_mut()).map_err(|e| bad(&e))?;
        if (info.width as usize, info.height as usize, info.color_type) != (cols, rows, png::ColorType::Rgba) {
            return Err(bad(&"size"));
        }
        levels.push(Grid { south, west, step, rows, cols, data });
    }
    Ok(Some(Imagery { levels, years }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_kept_and_read_back() {
        let mut g = Grid::new(-100.0, -50.0, 10.0, 3, 4, [0u8; 4]);
        g.set(1, 2, [10, 20, 30, 255]);
        let imagery = Imagery { levels: vec![g.clone(), Grid::new(0.0, 0.0, 40.0, 2, 2, [7u8, 8, 9, 255])], years: vec![2023, 2025] };
        let bytes = encode(&imagery).unwrap();
        assert_eq!(decode(&bytes).unwrap().as_ref(), Some(&imagery));
        let mut other = bytes.clone();
        other[4] ^= 1;
        assert_eq!(decode(&other).unwrap(), None);
        assert!(decode(&bytes[..bytes.len() - 3]).is_err());
        assert_eq!(attribution(&imagery.years).as_deref(), Some("Contains modified Copernicus Sentinel data 2023-2025"));
        assert_eq!(attribution(&[2025]).as_deref(), Some("Contains modified Copernicus Sentinel data 2025"));
        assert_eq!(attribution(&[]), None);
    }

    #[test]
    fn a_texel_takes_the_median_of_the_scenes_that_see_the_ground() {
        let at = |v: u16, seen: Seen| Look { rgb: [v; 3], seen, day: v as usize, weight: 1.0 };
        // A cloud and a cloud shadow among them are not counted; of the rest, the middle.
        let mut looks = [at(9000, Seen::Cloud), at(400, Seen::Ground), at(500, Seen::Ground), at(80, Seen::Cloud), at(450, Seen::Ground)];
        assert_eq!(choose(&mut looks).unwrap().rgb[0], 450);
        // Two: the darker. None clear: the darkest.
        assert_eq!(choose(&mut [at(500, Seen::Doubtful), at(300, Seen::Doubtful), at(9000, Seen::Cloud)]).unwrap().rgb[0], 300);
        assert_eq!(choose(&mut [at(7000, Seen::Cloud), at(6000, Seen::Cloud)]).unwrap().rgb[0], 6000);
        assert!(choose(&mut []).is_none());
        // Two scenes of one day blend by how far each is from its edge; another day stays apart.
        let mut looks = vec![Look { weight: 0.25, day: 1, ..at(400, Seen::Ground) }, at(900, Seen::Ground), Look { weight: 0.75, day: 1, ..at(800, Seen::Cloud) }];
        merge_days(&mut looks);
        assert_eq!(looks.len(), 2);
        assert_eq!((looks[0].rgb[0], looks[0].seen, looks[0].weight), (700, Seen::Cloud, 1.0));
        // The encoding: black, the middle grey of sRGB, white.
        assert_eq!((srgb(0.0), srgb(0.216), srgb(1.0), srgb(1.5)), (0, 128, 255, 255));
    }

    #[test]
    fn the_picture_is_read_through_its_tiles_and_given_back_where_it_clipped() {
        // A window of one tile of 2 by 2 pixels of 10 m: one clipped red, a band that knows it.
        let window = |samples: usize, bytes: usize, data: Vec<u8>, pixel: f64| Window {
            x0: 0,
            y0: 0,
            across: 1,
            tiles: vec![Some(Arc::new(data))],
            tile_width: 2,
            tile_height: 2,
            width: 2,
            height: 2,
            samples,
            bytes,
            pixel,
            west: 0.0,
            north: 20.0,
        };
        let picture = window(3, 1, vec![255, 100, 50, 255, 100, 50, 255, 100, 50, 255, 100, 50], 10.0);
        let classes = window(1, 1, vec![4, 4, 9, 9], 10.0);
        let band = |v: u16| window(1, 2, [v; 4].iter().flat_map(|v| v.to_le_bytes()).collect(), 10.0);
        let w = [picture, classes, band(3500), band(900), band(400)];
        let l = look(&w, 10.0, 10.0).unwrap();
        // Red clipped at a quarter: the band's 0.35 instead; green and blue from the picture.
        assert_eq!(l.rgb, [3500, 980, 490]);
        assert_eq!(look(&w, 5.0, 15.0).unwrap().seen, Seen::Ground);
        assert_eq!(look(&w, 5.0, 5.0).unwrap().seen, Seen::Cloud);
        // Outside the image there is nothing.
        assert!(look(&w, 50.0, 5.0).is_none());
    }
}
