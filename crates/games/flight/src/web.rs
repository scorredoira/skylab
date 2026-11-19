//! The game in a browser (`wasm32`): the page (`web/index.html`) loads the module, which downloads
//! the game's files with the progress on the page, then opens the game in its canvas as the
//! window would. No command line: what is flown and where is what the settings say (O, M), kept
//! in the browser. `scripts/build-web.sh` makes the page; the README says how to open it.

use crate::game::{Flight, Options};
use crate::render::Preset;
use crate::{audio, fleet, places, settings};

/// The list of the files to download (`scripts/build-web.sh` writes it): textures, mostly.
const MANIFEST: &str = "files.txt";
/// The workers started before the game: as many as its background jobs at the busiest, round about.
const WORKERS: usize = 8;

pub fn main() {
    engine::web::show_panics();
    // Started now, as the files download: the game waits for some jobs as it starts (its cockpit).
    engine::threads::start_workers(WORKERS);
    wasm_bindgen_futures::spawn_local(async {
        let downloaded = engine::web::download(MANIFEST, |done, total| {
            let share = done as f64 / total.max(1) as f64;
            engine::web::say(&format!("Downloading textures: {:.0} of {:.0} MB", done as f64 / 1e6, total as f64 / 1e6), Some(share));
        })
        .await;
        if let Err(error) = downloaded {
            engine::web::say(&format!("Could not download the game's files ({error}). Reload the page."), Some(0.0));
            return;
        }
        if let Err(error) = fleet::load() {
            engine::web::say(&format!("The aircraft: {error}"), Some(0.0));
            return;
        }
        engine::web::say("Preparing the world…", None);
        start();
    });
}

/// Opens the game as the settings left it: the quality (`media` unless the player chose another,
/// as a browser draws slower than the program), the aircraft, the picker at the start, and the
/// real place, which loads in the background over the generated world (nothing may wait for it).
fn start() {
    let store = settings::Store::user();
    let quality = Some(store.word("quality").and_then(Preset::named).unwrap_or(Preset::Medium));
    // At the hour of the player's clock, if the settings say so (`settings::LOCAL_TIME`).
    let mut options = Options { quality, clock: engine::clock::local_hour(), ..Options::default() };
    if store.flag(places::PICKER_AT_START).unwrap_or(false) {
        options.picker = Some(Default::default());
    }
    let place = store.word("place").filter(|key| !key.is_empty()).map(str::to_string);
    engine::run("Skylab", 0, 0, move |gpu| {
        let mut game = Flight::new(gpu, &options, store, audio::Audio::start());
        if let Some(key) = place {
            game.choose_place(key);
        }
        engine::web::ready();
        game
    });
}
