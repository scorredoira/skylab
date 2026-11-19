//! Real places for the simulator: downloads, keeps and reads free, open map data and builds from it
//! a `sim_core::Region` before the world is made. The only code of the project that touches the
//! network; `sim-core` stays offline and deterministic, and the same region kept on disk always
//! gives the same world.
//!
//! The sources need no key and cost nothing (their licences and the attribution they ask for are
//! in the README):
//!
//! - heights: the AWS Terrain Tiles (Mapzen's Terrarium encoding), on a public bucket (`terrarium`);
//! - the coast, lakes, woods, fields, towns, roads and aerodrome grounds: OpenStreetMap, as the vector
//!   tiles of OpenFreeMap (OpenMapTiles' schema; `mvt`);
//! - the buildings, and the bridges with all their tags: Overture Maps (OpenStreetMap with
//!   Microsoft's, Google's and Esri's buildings), its PMTiles read a tile at a time (`overture`,
//!   `pmtiles`); the famous bridges made by hand instead (`bridges`);
//! - aerodromes and runways: OurAirports (`ourairports`);
//! - the colour of the ground: Copernicus Sentinel-2 on AWS Open Data, found with Element 84's STAC
//!   catalogue (`sentinel`), made into a cloud-free picture after the region (`imagery`).
//!
//! ```no_run
//! let place = geodata::Place::parse("LEMH").unwrap();
//! let loaded = geodata::load(&place, false, &|p: geodata::Progress| eprintln!("{}", p.text())).unwrap();
//! let terrain = sim_core::Terrain::from_region(1, std::sync::Arc::new(loaded.region));
//! ```

pub mod basemap;
pub mod bridges;
pub mod build;
pub mod cache;
mod disk;
pub mod fetch;
pub mod follow;
#[cfg(test)]
mod fixtures;
pub mod geo;
pub mod imagery;
pub mod json;
pub mod mvt;
pub mod ourairports;
pub mod overture;
pub mod pmtiles;
pub mod search;
pub mod sentinel;
pub mod store;
pub mod terrarium;
mod threads;
pub mod tiff;
pub mod utm;

use std::fmt;

pub use build::{load, load_around, Loaded, Progress, Stage};

/// What went wrong, said to the player.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Offline and this is not in the cache.
    Offline(String),
    Http(String),
    Io(String),
    /// A file that could not be read.
    Data(String),
    /// No aerodrome with that code, or a place that is not one.
    Unknown(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Offline(what) => write!(f, "offline and {what} is not in the cache"),
            Error::Http(what) => write!(f, "could not download {what}"),
            Error::Io(what) => write!(f, "could not write {what}"),
            Error::Data(what) => write!(f, "data that cannot be read: {what}"),
            Error::Unknown(what) => write!(f, "unknown {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// A place to fly from: an aerodrome by its code (ICAO, IATA...) or a point of the Earth.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    Aerodrome(String),
    /// Latitude and longitude, degrees.
    At(f64, f64),
}

impl Place {
    /// `LEMH`, `lemh`, or `39.86,4.22` (latitude, longitude).
    pub fn parse(text: &str) -> Option<Place> {
        let text = text.trim();
        if let Some((lat, lon)) = text.split_once(',') {
            let (lat, lon) = (lat.trim().parse::<f64>().ok()?, lon.trim().parse::<f64>().ok()?);
            return ((-85.0..=85.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some(Place::At(lat, lon));
        }
        (!text.is_empty() && text.len() <= 8 && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| Place::Aerodrome(text.to_ascii_uppercase()))
    }

    /// How the cache names the region built for it.
    fn key(&self) -> String {
        match self {
            Place::Aerodrome(code) => code.clone(),
            Place::At(lat, lon) => format!("{lat:.4}_{lon:.4}"),
        }
    }
}

/// A place the game offers in its settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    /// The aerodrome's ICAO code.
    pub code: &'static str,
    /// What the player reads.
    pub title: &'static str,
}

/// An island by the sea, a valley among mountains and a big city; then fjords, islands, the
/// highest mountains, bays and cities from round the world.
pub const PRESETS: [Preset; 12] = [
    Preset { code: "LEMH", title: "Menorca: island, coast and cliffs" },
    Preset { code: "LOWI", title: "Innsbruck: a valley between 2600 m mountains" },
    Preset { code: "LEBL", title: "Barcelona: a big city by the sea" },
    Preset { code: "ENSG", title: "Sogndal: the Sognefjord, the longest and deepest in Norway" },
    Preset { code: "ENLK", title: "Lofoten: islands of peaks over the sea, north of the Arctic Circle" },
    Preset { code: "TNCS", title: "Saba: a volcano in the Caribbean and a 400 m runway" },
    Preset { code: "VNLK", title: "Lukla: the Everest runway, at 2860 m in the Himalaya" },
    Preset { code: "LFLJ", title: "Courchevel: an altiport at 2000 m among the ski slopes" },
    Preset { code: "LPMA", title: "Madeira: a runway between the cliff and the sea, below the mountain" },
    Preset { code: "VHHH", title: "Hong Kong: islands, bay and mountains next to the city" },
    Preset { code: "KSFO", title: "San Francisco: the bay, the Golden Gate and the hills" },
    Preset { code: "NZQN", title: "Queenstown: a lake and mountains in New Zealand" },
];

/// What the data's licences ask to be said wherever it is shown (as the game's credits).
pub const ATTRIBUTION: &str = "Terrain: Mapzen and AWS Terrain Tiles (SRTM, GMTED, ETOPO1, USGS 3DEP, Copernicus EU-DEM and others) · \
Map © OpenStreetMap contributors (ODbL), © OpenMapTiles, OpenFreeMap · Buildings and bridges: Overture Maps Foundation \
(overturemaps.org; ODbL), with OpenStreetMap, Microsoft (ODbL), Google Open Buildings and Esri Community Maps (CC BY 4.0) · \
Aerodromes: OurAirports (public domain) · \
Ground colour: Contains modified Copernicus Sentinel data 2023-2025 (Sentinel-2 L2A, AWS Open Data, Element 84 STAC catalogue)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_read_as_codes_or_points() {
        assert_eq!(Place::parse("lemh"), Some(Place::Aerodrome("LEMH".into())));
        assert_eq!(Place::parse(" 39.86, 4.22 "), Some(Place::At(39.86, 4.22)));
        assert_eq!(Place::parse("95,0"), None);
        assert_eq!(Place::parse("a b"), None);
        assert_eq!(Place::parse(""), None);
        assert_eq!(Place::At(39.86, 4.22).key(), "39.8600_4.2200");
    }
}
