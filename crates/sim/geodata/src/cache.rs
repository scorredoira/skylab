//! What the cache already holds of a place, and about how much is left to download for it: what
//! the game's place picker says next to each place before the player chooses it. Nothing is
//! downloaded or built here; only the names of the files are looked at.

use std::path::Path;

use crate::build::{map_key, COVER_HALF, HEIGHTS, MAP_ZOOM};
use crate::fetch::cache_dir;
use crate::geo::{tiles_over, Projection, TileId};
use crate::{disk, store, terrarium, Place};

/// The bytes a tile takes on average, as downloaded for the first places: heights at zoom 13, 12
/// and 8, and a map tile at zoom 12 (a city's is heavier, open sea's lighter).
const HEIGHT_TILE: [u64; 3] = [74_000, 71_000, 98_000];
const MAP_TILE: u64 = 24_000;
/// OurAirports' two tables, downloaded once for every place.
const TABLES: u64 = 17_000_000;

/// How much of a place is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Need {
    /// Its region was built before and is kept: it loads at once and without the network.
    pub kept: bool,
    /// About how many bytes are still to be downloaded (0 when kept).
    pub bytes: u64,
}

/// What the cache at `$FLIGHT_CACHE` or the user's cache folder holds of a place whose region is
/// centred at about this latitude and longitude (an aerodrome's point is near enough).
pub fn need(place: &Place, latitude: f64, longitude: f64) -> Need {
    need_in(&cache_dir(), place, latitude, longitude)
}

/// `need`, with the cache in `root`.
pub fn need_in(root: &Path, place: &Place, latitude: f64, longitude: f64) -> Need {
    if kept(root, place) {
        return Need { kept: true, bytes: 0 };
    }
    let projection = Projection::new(latitude, longitude);
    let missing = |tiles: Vec<TileId>, key: fn(TileId) -> String| tiles.into_iter().filter(|id| !disk::exists(&root.join(key(*id)))).count() as u64;
    let mut bytes = 0;
    for (level, size) in HEIGHTS.iter().zip(HEIGHT_TILE) {
        bytes += size * missing(tiles_over(&projection, (0.0, 0.0), level.half + level.step * 2.0, level.zoom), terrarium::key);
    }
    bytes += MAP_TILE * missing(tiles_over(&projection, (0.0, 0.0), COVER_HALF, MAP_ZOOM), map_key);
    if !disk::exists(&root.join("ourairports/airports.csv")) || !disk::exists(&root.join("ourairports/runways.csv")) {
        bytes += TABLES;
    }
    Need { kept: false, bytes }
}

/// Whether the region of a place was built before by this version of the program.
fn kept(root: &Path, place: &Place) -> bool {
    let file = root.join("regions").join(format!("{}.region", place.key()));
    disk::head(&file, 8).is_ok_and(|head| store::is_current(&head))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_region_needs_nothing_and_a_new_one_its_missing_tiles() {
        let root = std::env::temp_dir().join(format!("flight-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let place = Place::Aerodrome("LEMH".into());
        // Nothing at all: every tile and the tables, some 25 to 50 MB at Menorca's latitude.
        let all = need_in(&root, &place, 39.86, 4.22);
        assert!(!all.kept && (25_000_000..60_000_000).contains(&all.bytes), "{}", all.bytes);
        // The tables and a tile already there: less.
        std::fs::create_dir_all(root.join("ourairports")).unwrap();
        std::fs::write(root.join("ourairports/airports.csv"), "x").unwrap();
        std::fs::write(root.join("ourairports/runways.csv"), "x").unwrap();
        let (x, y) = crate::geo::tile_xy(39.86, 4.22, 8);
        let id = TileId { z: 8, x: x as u32, y: y as u32 };
        std::fs::create_dir_all(root.join(terrarium::key(id)).parent().unwrap()).unwrap();
        std::fs::write(root.join(terrarium::key(id)), "x").unwrap();
        assert_eq!(need_in(&root, &place, 39.86, 4.22).bytes, all.bytes - TABLES - HEIGHT_TILE[2]);
        // A region of another version is built again; one of this version is kept.
        std::fs::create_dir_all(root.join("regions")).unwrap();
        let mut old = b"FLRG".to_vec();
        old.extend_from_slice(&(store::VERSION - 1).to_le_bytes());
        std::fs::write(root.join("regions/LEMH.region"), &old).unwrap();
        assert!(!need_in(&root, &place, 39.86, 4.22).kept);
        let mut current = b"FLRG".to_vec();
        current.extend_from_slice(&store::VERSION.to_le_bytes());
        std::fs::write(root.join("regions/LEMH.region"), &current).unwrap();
        assert_eq!(need_in(&root, &place, 39.86, 4.22), Need { kept: true, bytes: 0 });
        // A point is kept under its own name.
        assert!(!need_in(&root, &Place::At(39.86, 4.22), 39.86, 4.22).kept);
        std::fs::remove_dir_all(root).unwrap();
    }
}
