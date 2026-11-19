//! The Sentinel-2 scenes over a place: Copernicus' satellites photograph every point of the land
//! every five days at 10 m a pixel, and AWS Open Data keeps each scene of the processing that
//! corrects for the air (level 2A) as Cloud-Optimized GeoTIFFs, public and free
//! (`sentinel-cogs.s3.us-west-2.amazonaws.com`). Element 84's STAC catalogue (`earth-search`)
//! finds them: no key, no cost. Each scene is one MGRS square of 109.8 km in a UTM zone.
//!
//! What is read of a scene (its assets): `visual`, the true colour picture (B04, B03, B02 as 8 bits:
//! reflectance times 1020 or so, so it saturates at a quarter: bright ground clips); `scl`, the
//! classes of the scene at 20 m (vegetation, bare ground, water, cloud, cloud shadow, cirrus, snow);
//! and the bands `red`, `green` and `blue` themselves (reflectance in ten-thousandths) at an
//! overview, to give back what the picture clipped.
//!
//! Terms: Copernicus Sentinel data are free and open to anyone for any use, the attribution asked
//! is "Contains modified Copernicus Sentinel data <year>" (the Legal Notice on the use of
//! Copernicus Sentinel Data and Service Information, as the catalogue links to it).

use crate::fetch::Fetcher;
use crate::json::Json;
use crate::tiff::{self, Cog};
use crate::utm::Zone;
use crate::Error;

/// The catalogue's search.
pub const SEARCH: &str = "https://earth-search.aws.element84.com/v1/search";
/// Scenes more clouded than this, percent of the square, are not looked at.
const MOST_CLOUD: f64 = 20.0;
/// The months a scene is taken from: summer in the northern hemisphere, winter's opposite in the
/// southern (June to mid September there; December to mid March below the equator).
const SUMMER_NORTH: (&str, &str) = ("06-01", "09-15");
const SUMMER_SOUTH: (&str, &str) = ("12-01", "03-15");
/// Scenes asked for each summer, the least clouded first.
const PER_SUMMER: usize = 50;

/// One scene: an MGRS square on one day.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// The catalogue's name of it (`S2A_31TFE_20240930_0_L2A`).
    pub id: String,
    /// The MGRS square (`31TFE`).
    pub square: String,
    /// `2024-09-30`.
    pub date: String,
    /// Percent of the square under cloud, and without data (outside the satellite's swath).
    pub cloud: f64,
    pub nodata: f64,
    pub zone: Zone,
    /// The assets read: the true colour picture, the classes, and the red, green and blue bands.
    pub visual: String,
    pub scl: String,
    pub bands: [String; 3],
}

impl Scene {
    pub fn year(&self) -> u16 {
        self.date.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0)
    }

    /// How good a scene is for a composite: little cloud, and little of the square missing.
    pub fn score(&self) -> f64 {
        self.cloud + 0.3 * self.nodata
    }
}

/// The scenes of a catalogue's answer (a GeoJSON feature collection) that have every asset read.
pub fn scenes(answer: &[u8]) -> Result<Vec<Scene>, Error> {
    let text = std::str::from_utf8(answer).map_err(|_| Error::Data("STAC".into()))?;
    let json = Json::parse(text).ok_or_else(|| Error::Data("STAC".into()))?;
    let found = json.get("features").map(Json::list).unwrap_or_default();
    Ok(found
        .iter()
        .filter_map(|f| {
            let p = f.get("properties")?;
            let href = |asset: &str| f.at(&["assets", asset, "href"]).and_then(Json::text).filter(|h| h.starts_with("https://")).map(str::to_string);
            let square = p.get("grid:code").and_then(Json::text).map(|c| c.trim_start_matches("MGRS-").to_string())?;
            Some(Scene {
                id: f.get("id")?.text()?.to_string(),
                square,
                date: p.get("datetime")?.text()?.get(..10)?.to_string(),
                cloud: p.get("eo:cloud_cover")?.number()?,
                nodata: p.get("s2:nodata_pixel_percentage").and_then(Json::number).unwrap_or(0.0),
                zone: Zone::from_epsg(p.get("proj:epsg")?.number()? as u32)?,
                visual: href("visual")?,
                scl: href("scl")?,
                bands: [href("red")?, href("green")?, href("blue")?],
            })
        })
        .collect())
}

/// The scenes of the last `summers` summers before `year` over a box of latitudes and longitudes
/// (south, west, north, east), little clouded.
pub fn search(fetcher: &Fetcher, bbox: [f64; 4], year: i32, summers: i32) -> Result<Vec<Scene>, Error> {
    let (from, to) = if bbox[0] + bbox[2] >= 0.0 { SUMMER_NORTH } else { SUMMER_SOUTH };
    let mut all = Vec::new();
    for y in year - summers..year {
        // A southern summer runs over the new year.
        let end_year = if to < from { y + 1 } else { y };
        let body = format!(
            r#"{{"collections":["sentinel-2-l2a"],"bbox":[{:.5},{:.5},{:.5},{:.5}],"datetime":"{y}-{from}T00:00:00Z/{end_year}-{to}T23:59:59Z","query":{{"eo:cloud_cover":{{"lt":{MOST_CLOUD}}}}},"sortby":[{{"field":"properties.eo:cloud_cover","direction":"asc"}}],"limit":{PER_SUMMER}}}"#,
            bbox[1], bbox[0], bbox[3], bbox[2]
        );
        all.extend(scenes(&fetcher.query(SEARCH, &body)?)?);
    }
    Ok(all)
}

/// Every scene of the best `days` days among `found`, best day first. The scenes of one day are
/// one pass of the satellite over the place, cut into squares: taken together they join without
/// a seam. A day is better the more of the squares it holds (a pass may miss the edge of the
/// place) and the less clouded and the less empty they are.
pub fn choose(found: Vec<Scene>, days: usize) -> Vec<Scene> {
    let mut squares: Vec<&str> = found.iter().map(|s| s.square.as_str()).collect();
    squares.sort_unstable();
    squares.dedup();
    let mut dates: Vec<&str> = found.iter().map(|s| s.date.as_str()).collect();
    dates.sort_unstable();
    dates.dedup();
    let rank = |date: &str| {
        let of: Vec<&Scene> = found.iter().filter(|s| s.date == date).collect();
        let mut held: Vec<&str> = of.iter().map(|s| s.square.as_str()).collect();
        held.sort_unstable();
        held.dedup();
        // A square missing counts as a square all clouded.
        let missing = (squares.len() - held.len()) as f64 * 100.0;
        (of.iter().map(|s| s.score()).sum::<f64>() / of.len() as f64 * held.len() as f64 + missing) / squares.len() as f64
    };
    let mut ranked: Vec<(f64, &str)> = dates.iter().map(|d| (rank(d), *d)).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| b.1.cmp(a.1)));
    let best: Vec<String> = ranked.iter().take(days).map(|r| r.1.to_string()).collect();
    let mut chosen: Vec<Scene> = Vec::new();
    for date in &best {
        for s in found.iter().filter(|s| &s.date == date) {
            // The same square twice on one day (two processings): the first.
            if !chosen.iter().any(|c| c.date == s.date && c.square == s.square) {
                chosen.push(s.clone());
            }
        }
    }
    chosen
}

/// A file of a scene opened: its header read (from the cache, or its first bytes fetched).
pub struct Asset {
    pub url: String,
    /// Where its parts are kept in the cache: `sentinel2/<scene>/<asset>`.
    key: String,
    pub cog: Cog,
}

/// The first bytes of a COG fetched to read its header: GDAL writes the header first, and the
/// Sentinel-2 files' headers take some 10 KB.
const HEAD: u64 = 65_536;

impl Asset {
    pub fn open(fetcher: &Fetcher, scene: &Scene, name: &str, url: &str) -> Result<Asset, Error> {
        let key = format!("sentinel2/{}/{name}", scene.id);
        let head = fetcher.cached_range(&format!("{key}/head"), url, 0, HEAD)?;
        let cog = tiff::parse(&head, &|offset, length| fetcher.cached_range(&format!("{key}/head{offset}"), url, offset, length))?;
        Ok(Asset { url: url.to_string(), key, cog })
    }

    /// The bytes of a tile (column, row) of a level as stored, from the cache or fetched; `None`
    /// for a tile not written.
    pub fn tile(&self, fetcher: &Fetcher, level: usize, column: u32, row: u32) -> Result<Option<Vec<u8>>, Error> {
        let Some((offset, length)) = self.cog.levels[level].range(column, row) else { return Ok(None) };
        fetcher.cached_range(&format!("{}/{level}/{column}_{row}", self.key), &self.url, offset, length).map(Some)
    }

    /// The level whose pixel is nearest `pixel` metres without being coarser.
    pub fn level_for(&self, pixel: f64) -> usize {
        (0..self.cog.levels.len()).rev().find(|&l| self.cog.pixel_of(l) <= pixel * 1.01).unwrap_or(0)
    }
}

/// What a pixel of the classes says about the ground under it, for a composite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Seen {
    /// The ground itself: vegetation, bare soil, water, snow, dark areas.
    Ground,
    /// Not classified (often the edge of a cloud), or defective.
    Doubtful,
    /// Cloud, its shadow, cirrus.
    Cloud,
    /// No data.
    Nothing,
}

/// The Scene Classification Layer's classes (ESA's L2A product definition).
pub fn seen(class: u8) -> Seen {
    match class {
        2 | 4 | 5 | 6 | 11 => Seen::Ground,
        1 | 7 => Seen::Doubtful,
        3 | 8 | 9 | 10 => Seen::Cloud,
        _ => Seen::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalogues_answer_is_read_into_scenes_and_the_best_chosen() {
        let answer = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stac.json")).unwrap();
        let found = scenes(&answer).unwrap();
        // The fourth has no `visual` asset.
        assert_eq!(found.len(), 3);
        let s = &found[0];
        assert_eq!((s.id.as_str(), s.square.as_str(), s.date.as_str(), s.year()), ("S2A_31TFE_20240930_0_L2A", "31TFE", "2024-09-30", 2024));
        assert_eq!(s.zone, Zone { number: 31, north: true });
        assert!(s.visual.ends_with("/TCI.tif") && s.scl.ends_with("/SCL.tif") && s.bands[2].ends_with("/B02.tif"));
        assert_eq!((s.cloud, s.nodata), (1.5, 0.0));
        // Each day holds one of the two squares: the least clouded day first.
        let chosen = choose(found.clone(), 2);
        assert_eq!(chosen.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["S2A_31TFE_20240930_0_L2A", "S2B_31TFE_20240720_0_L2A"]);
        assert_eq!(choose(found, 9).len(), 3);
        assert_eq!((seen(4), seen(9), seen(7), seen(0)), (Seen::Ground, Seen::Cloud, Seen::Doubtful, Seen::Nothing));
    }
}
