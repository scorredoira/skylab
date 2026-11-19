//! Real places in the game: a region loaded before the window opens (`--place`, `--at`, or the
//! place saved in the settings), or in the background while the game runs when the player chooses
//! another in the settings. `geodata` downloads and builds it; the world is made on it once it is whole.
//! The satellites' picture of its ground follows in the background (`Photos`), and so do the
//! regions further along the way as the aircraft flies on (`Following`).

use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use engine::Instant;
use sim_core::{Imagery, Region, Terrain};
use geodata::follow::Follow;
use geodata::{Place, Progress, Stage};

use crate::settings::Store;

/// A region being loaded in the background.
pub struct Loading {
    /// The place as the settings name it (`LEMH`, or `lat,lon`).
    pub key: String,
    progress: Arc<Mutex<Option<Progress>>>,
    /// When it started.
    pub started: Instant,
    done: Receiver<Result<Arc<Terrain>, geodata::Error>>,
}

impl Loading {
    /// Starts loading a place on a thread of its own, and making its terrain for `seed` there too
    /// (indexing a city's roads takes a while); `None` if `key` names none.
    pub fn start(key: &str, seed: u64) -> Option<Loading> {
        let place = Place::parse(key)?;
        let progress = Arc::new(Mutex::new(None));
        let (sender, done) = channel();
        let shared = progress.clone();
        engine::threads::spawn(move || {
            let loaded = geodata::load(&place, false, &|p| *shared.lock().unwrap_or_else(|e| e.into_inner()) = Some(p));
            let _ = sender.send(loaded.map(|l| Arc::new(Terrain::from_region(seed, Arc::new(l.region)))));
        });
        Some(Loading { key: key.to_string(), progress, done, started: Instant::now() })
    }

    /// A load that stays where it is, having got this far: for tests of what the player sees.
    #[cfg(test)]
    pub fn stalled(key: &str, progress: Option<Progress>, seconds: u64) -> Loading {
        let (_, done) = channel();
        let started = Instant::now().checked_sub(Duration::from_secs(seconds)).unwrap_or_else(Instant::now);
        Loading { key: key.to_string(), progress: Arc::new(Mutex::new(progress)), started, done }
    }

    /// How far it has got, for the player.
    pub fn text(&self) -> String {
        let progress = *engine::threads::lock(&self.progress);
        match progress {
            Some(p) => format!("Loading {}: {:.0} %, {}", self.key, p.share() * 100.0, p.text()),
            None => format!("Loading {}…", self.key),
        }
    }

    /// How far it has got, if it has said: the share of the whole (0 to 1) and what it is doing
    /// ("terrain 12/40, 8 MB downloaded"); past the end of the download, the world being made on it.
    pub fn progress(&self) -> Option<(f64, String)> {
        let p = (*engine::threads::lock(&self.progress))?;
        Some(match p.stage {
            geodata::Stage::Done => (1.0, "preparing the world".to_string()),
            _ => (p.share(), p.text()),
        })
    }

    /// The terrain of the place, or why there is none, once the load has ended.
    pub fn finished(&self) -> Option<Result<Arc<Terrain>, geodata::Error>> {
        self.done.try_recv().ok()
    }
}

/// The satellites' picture of a place's ground (`geodata::imagery`), made in the background once
/// its region is there: the ground keeps the colours of its maps until it comes, and keeps them
/// if it cannot be had.
pub struct Photos {
    done: Receiver<Option<Arc<Imagery>>>,
}

impl Photos {
    /// Starts making the picture of a real place's ground; `None` for the generated world.
    pub fn start(terrain: &Arc<Terrain>) -> Option<Photos> {
        terrain.region()?;
        let (sender, done) = channel();
        let terrain = terrain.clone();
        engine::threads::spawn(move || {
            let Some(region) = terrain.region() else { return };
            let made = geodata::imagery::load(region, false, &|_| {});
            if let Err(error) = &made {
                eprintln!("no satellite photo: {error}");
            }
            let _ = sender.send(made.ok().map(|l| Arc::new(l.imagery)));
        });
        Some(Photos { done })
    }

    /// The picture, or none, once it has been made.
    pub fn finished(&self) -> Option<Option<Arc<Imagery>>> {
        self.done.try_recv().ok()
    }
}

/// A real place followed beyond its region: the region further along the way is built in the
/// background before the aircraft gets there (`geodata::follow`), and flown on once it comes.
pub struct Following {
    place: Place,
    seed: u64,
    follow: Follow,
    building: Option<Receiver<Result<Arc<Terrain>, geodata::Error>>>,
}

impl Following {
    /// Follows a place flown in its own region; `None` if `key` names none.
    pub fn start(key: &str, seed: u64) -> Option<Following> {
        Some(Following { place: Place::parse(key)?, seed, follow: Follow::new((0.0, 0.0)), building: None })
    }

    /// For the aircraft at (north, east) moving at (north, east) m/s over the ground: starts
    /// building the next region when it is needed, and gives its terrain once it has come and
    /// serves where the aircraft is.
    pub fn update(&mut self, n: f64, e: f64, vn: f64, ve: f64) -> Option<Arc<Terrain>> {
        if let Some(centre) = self.follow.next(n, e, vn, ve) {
            let (sender, done) = channel();
            let (place, seed) = (self.place.clone(), self.seed);
            engine::threads::spawn(move || {
                let built = geodata::load_around(&place, centre, false, &|_| {});
                let _ = sender.send(built.map(|l| Arc::new(Terrain::from_region(seed, Arc::new(l.region)))));
            });
            self.building = Some(done);
        }
        let result = self.building.as_ref()?.try_recv().ok()?;
        self.building = None;
        match result {
            Ok(terrain) => self.follow.arrived(n, e).then_some(terrain),
            Err(error) => {
                eprintln!("could not follow the place: {error}");
                self.follow.failed();
                None
            }
        }
    }
}

/// Loads a place before the window opens, saying how it goes on the terminal about once a second.
pub fn load_now(place: &Place, offline: bool) -> Result<Region, geodata::Error> {
    let start = Instant::now();
    let shown = Mutex::new(Instant::now());
    let loaded = geodata::load(place, offline, &|p: Progress| {
        let mut shown = shown.lock().unwrap_or_else(|e| e.into_inner());
        if shown.elapsed() > Duration::from_secs(1) && p.stage != Stage::Done {
            *shown = Instant::now();
            println!("{:3.0} % {}", p.share() * 100.0, p.text());
        }
    })?;
    println!("{}: {:.1} s{}", loaded.region.name, start.elapsed().as_secs_f64(), if loaded.downloaded > 0 { format!(", {:.0} MB downloaded", loaded.downloaded as f64 / 1e6) } else { String::new() });
    Ok(loaded.region)
}

/// What the player reads for a place as the settings name it (`None`, the generated world).
pub fn name(key: Option<&str>) -> String {
    match key {
        None => "Generated world".to_string(),
        Some(key) => match (preset(key), point_of(key)) {
            (Some(p), _) => p.title.split(':').next().unwrap_or(p.code).to_string(),
            (None, Some((lat, lon))) => format!("Point {lat:.3}, {lon:.3}"),
            (None, None) => key.to_string(),
        },
    }
}

/// What a place is, in a few words.
pub fn describe(key: Option<&str>) -> String {
    match key {
        None => "The seed's world, no downloads".to_string(),
        Some(key) => preset(key).and_then(|p| p.title.split_once(": ")).map_or("Real place".to_string(), |t| t.1.to_string()),
    }
}

fn preset(key: &str) -> Option<&'static geodata::Preset> {
    geodata::PRESETS.iter().find(|p| p.code == key)
}

/// How the player starts at a place chosen in the picker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    /// Lined up on a runway, taking off along this true heading (degrees); `None`, the main
    /// runway's end into the wind.
    Runway { heading: Option<f64> },
    /// On final to the runway taking off along this heading (landing along it), this many nautical
    /// miles out.
    Final { heading: Option<f64>, miles: f64 },
    /// In level flight over the place, this many feet above the ground, into the wind.
    Air { feet: f64 },
    /// Racing the course of this place in `course::file::courses`, in its world.
    Course { index: usize },
}

/// What the player chose in the picker: where (`None` the generated world, else the key the
/// settings and `Place::parse` take) and how to start there.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub key: Option<String>,
    pub start: Start,
}

/// Of the runway ends given by their true headings (degrees), the one whose take-off is most into
/// a wind blowing from `wind_from` (degrees); `None` if there are none.
pub fn into_wind(headings: &[f64], wind_from: f64) -> Option<usize> {
    let headwind = |h: f64| (h - wind_from).to_radians().cos();
    (0..headings.len()).max_by(|&a, &b| headwind(headings[a]).total_cmp(&headwind(headings[b])))
}

/// The key a point of the Earth is kept under in the settings, and what `Place::parse` reads.
pub fn point_key(latitude: f64, longitude: f64) -> String {
    format!("{latitude:.4},{longitude:.4}")
}

/// The key's point, if it is one.
pub fn point_of(key: &str) -> Option<(f64, f64)> {
    match Place::parse(key)? {
        Place::At(lat, lon) => Some((lat, lon)),
        Place::Aerodrome(_) => None,
    }
}

/// The settings' keys of the places last flown (the latest first) and of the favourites.
pub const RECENT: &str = "places_recent";
pub const FAVOURITES: &str = "places_favourite";
/// The key of the setting that opens the picker when the game starts.
pub const PICKER_AT_START: &str = "picker_at_start";
/// How many recent places are remembered.
pub const RECENT_KEPT: usize = 8;

/// The places of a list kept in the settings (space between them).
pub fn list(store: &Store, key: &str) -> Vec<String> {
    store.word(key).unwrap_or("").split(' ').filter(|k| Place::parse(k).is_some()).map(str::to_string).collect()
}

/// Puts a place first among the recent ones, keeping `RECENT_KEPT`.
pub fn remember(store: &mut Store, place: &str) {
    let mut recent = list(store, RECENT);
    recent.retain(|k| k != place);
    recent.insert(0, place.to_string());
    recent.truncate(RECENT_KEPT);
    store.set(RECENT, recent.join(" "));
}

/// Makes a place a favourite, or not any more; says whether it is one now.
pub fn toggle_favourite(store: &mut Store, place: &str) -> bool {
    let mut favourites = list(store, FAVOURITES);
    let was = favourites.iter().any(|k| k == place);
    if was {
        favourites.retain(|k| k != place);
    } else {
        favourites.push(place.to_string());
    }
    store.set(FAVOURITES, favourites.join(" "));
    !was
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_into_the_wind_is_the_one_the_wind_blows_along_against() {
        // Runway 06/24: with the wind from 250° the take-off is from 24, from 030° from 06.
        assert_eq!(into_wind(&[60.0, 240.0], 250.0), Some(1));
        assert_eq!(into_wind(&[60.0, 240.0], 30.0), Some(0));
        // Two runways: the one most along the wind, across north.
        assert_eq!(into_wind(&[43.0, 223.0, 20.0, 200.0, 350.0, 170.0], 0.0), Some(4));
        assert_eq!(into_wind(&[], 5.0), None);
    }

    #[test]
    fn recent_places_and_favourites_are_kept_in_the_settings_file() {
        let path = std::env::temp_dir().join(format!("flight-places-{}", std::process::id())).join("settings");
        let mut store = Store::open(path.clone());
        for place in ["LEMH", "LOWI", "39.8600,4.2200", "LEMH"] {
            remember(&mut store, place);
        }
        assert_eq!(list(&store, RECENT), ["LEMH", "39.8600,4.2200", "LOWI"], "the latest first, each once");
        for k in 0..20 {
            remember(&mut store, &format!("K{k:03}"));
        }
        assert_eq!(list(&store, RECENT).len(), RECENT_KEPT);
        assert_eq!(list(&store, RECENT)[0], "K019");
        assert!(toggle_favourite(&mut store, "VNLK"));
        assert!(toggle_favourite(&mut store, &point_key(17.645, -63.22)));
        assert!(!toggle_favourite(&mut store, "VNLK"));
        assert!(toggle_favourite(&mut store, "LFLJ"));
        // Read back from the file.
        let again = Store::open(path.clone());
        assert_eq!(list(&again, FAVOURITES), ["17.6450,-63.2200", "LFLJ"]);
        assert_eq!(point_of("17.6450,-63.2200"), Some((17.645, -63.22)));
        assert_eq!(point_of("LFLJ"), None);
        // What is not a place is left out.
        store.set(FAVOURITES, "LFLJ  a,b 95,0");
        assert_eq!(list(&store, FAVOURITES), ["LFLJ"]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
