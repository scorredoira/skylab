//! The map tiles of OpenFreeMap (OpenStreetMap in OpenMapTiles' layers) for a map drawn on screen:
//! the game's place picker, from the whole world (zoom 0) down to an aerodrome. They are kept in
//! the same cache as a region's (`openfreemap/z/x/y.pbf`), so a place flown before shows at once,
//! and fetched as politely (`fetch`). Decoding them is `mvt`'s; drawing them is the game's.

use std::sync::OnceLock;

use crate::build::{map_key, map_template};
use crate::fetch::{cache_dir, Fetcher};
use crate::geo::TileId;
use crate::{disk, Error};

/// The deepest zoom the server has; nearer, the tiles of this zoom are drawn bigger.
pub const MAX_ZOOM: u8 = 14;

/// The tiles of the map, from the cache or the server.
pub struct Basemap {
    fetcher: Fetcher,
    /// The address of the tiles, asked once (the planet it names changes every week); only needed
    /// for a tile that is not in the cache.
    template: OnceLock<Result<String, Error>>,
}

impl Basemap {
    /// `offline` reads only the cache.
    pub fn new(offline: bool) -> Basemap {
        Basemap { fetcher: Fetcher::new(cache_dir(), offline), template: OnceLock::new() }
    }

    /// The bytes of a tile (gzipped or not, as `mvt::decode` takes them); `Ok(None)` where the
    /// server has none.
    pub fn tile(&self, id: TileId) -> Result<Option<Vec<u8>>, Error> {
        if id.z > MAX_ZOOM || id.x >= 1 << id.z || id.y >= 1 << id.z {
            return Ok(None);
        }
        let key = map_key(id);
        if let Ok(bytes) = disk::read(&self.fetcher.root().join(&key)) {
            if !bytes.is_empty() {
                return Ok(Some(bytes));
            }
        }
        let template = self.template.get_or_init(|| map_template(&self.fetcher)).clone()?;
        let url = template.replace("{z}", &id.z.to_string()).replace("{x}", &id.x.to_string()).replace("{y}", &id.y.to_string());
        self.fetcher.cached(&key, &url)
    }

    /// Bytes downloaded so far.
    pub fn downloaded(&self) -> u64 {
        self.fetcher.downloaded()
    }
}
