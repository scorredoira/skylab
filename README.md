# Skylab

A flight simulator to train AI models: a realistic, good-looking world where they learn to fly
planes, drones and other vehicles, and where a person can fly too.

For now it flies a Cessna 172, an aerobatic Extra 300L, a Boeing 747, a 5-inch FPV quad, a 5-inch racing quad and a tilt-rotor, over seed-generated terrain or real places,
and drives a sports car round a race circuit (the driving game, below). Written in Rust; runs on
macOS and Linux, and in the browser (with WebGPU).

## Getting started

You need Rust (`rustup`, stable). On Linux, also the development packages for ALSA,
Wayland/X11 and `pkg-config`.

```
scripts/run.sh
```

Builds both games and opens Skylab on its first screen: planes, drones or cars. It takes the same
options as the game: `scripts/run.sh --air`, `scripts/run.sh --seed 7`. `SKYLAB_CAP_GB=4` kills it
past that much memory (for unattended runs).

The first build takes a few minutes; the next ones, seconds. The game opens on its first screen,
three pictures that fill the window: **Planes**, **Drones** or **Cars**. The planes and the drones
go on to their aircraft, to choose one, and then to the menu: you start at the threshold of a
runway with the engine at idle, behind **Fly**, Aircraft, Place, Weather & time, Settings, Manual
and Replay. The cars are the driving game ("Driving" below), opened in the game's place. All of it
with the mouse, the arrows and Enter, or a gamepad. `Esc` brings the menu back at any time (the game
waits) and `Esc` again flies on, its "Change vehicle" the first screen, and `H` shows all the keys.
The game starts at the hour of your computer's clock (Settings › Weather, "Start at the current time"). Options that say what is
flown or where (`--aircraft`, `--course`, `--place`, `--air`...) go past the first screen.

The programs are `target/release/skylab` and, beside it, `target/release/skylab-drive`. On macOS,
`scripts/bundle-macos.sh` makes `target/release/Skylab.app`, with both and its icon. The logo and
the icon are in `assets/brand` (SVG and PNG).

| To | Press |
|---|---|
| Take off | `W` to full and `↓` at 55 knots |
| See the help | `H`; the illustrated manual, `F1` |
| Watch the last few minutes again (replay; again to go back to flying) | `F2` |
| Record a video of the game with its sound, to share; again to save it in `~/Movies/Skylab` | `F12` |
| Change view | `V` |
| Normal view → close → panel (on the joystick, button 4) | `Space` |
| The menu (the game waits): fly, aircraft, place, weather, settings, manual, replay; again, fly on | `Esc` |
| Choose the aircraft, from their pictures | the menu's "Aircraft" |
| Planes, drones or cars: the first screen again | the menu's "Change vehicle" |
| Settings, a panel on the right beside the flight, which goes on: weather and time, flight aids, graphics, credits | `O` (again, or `Esc`, closes it) |
| Time faster or slower, ×1 to ×16 (as accurate: more steps, not longer ones), e.g. while the autopilot flies a long leg | `F10` / `F9` (gamepad: d-pad → / ←) |
| Choose place: featured places with pictures, search for an airfield, world map, favourites | `M` |
| Back to the runway / start in the air / on final | `1` / `2` / `3` |
| Turn keyboard assist on or off | `G` |
| Altimeter to QNH, heading indicator to the compass, test failures | `Y`, `J`, `F5`–`F8` |
| C172 autopilot (Garmin GFC 700) | `P` holds heading and altitude, the arrows then change them; modes with `4`–`9`, or click the G1000 keys and drag or scroll its knobs (Shift + scroll over the HDG knob: the altitude) |
| FPV quad (it levels itself): disarm and arm, its camera or behind it | `R`, `V`; `W` `S` throttle, arrows pitch and roll, `A` `D` yaw |
| Tilt-rotor: collective, stick and pedals; its engines; its nacelles forward or back a notch; its stability help | `W` `S`, arrows, `A` `D`; `R`; `F`, `G`; `C` |
| Race a drone course against a trained network and the autopilot (Deepwood Forest; `--course pinewood` the airfield's) | `scripts/run.sh --ai`; `N` the camera on the next quad |

The full list of keys and command-line options is in
[crates/games/flight/README.md](crates/games/flight/README.md).

Another world: `scripts/run.sh --seed 7`. The Extra: `scripts/run.sh --aircraft extra`, or pick it in the menu (`Esc`, "Aircraft").
The FPV quad: `scripts/run.sh --aircraft fpv` (`--air` to start hovering); the racing quad (6S, 175 km/h):
`--aircraft race`; an RC
transmitter plugged in by USB is its radio. The Kestrel tilt-rotor: `--aircraft kestrel`.

## Real places

`M` opens "Choose place": you search for any airfield in the world by name, city or ICAO or
IATA code, or pick it on a map you drag and zoom with the mouse, or click any point.
On the left, favourites, recent places, a dozen featured places with their pictures (Menorca,
Innsbruck, Barcelona, a fjord, the Caribbean, Lukla, Courchevel, Hong Kong, San Francisco...; they
are pins on the map too) and the drone race courses, each one saying whether it is already downloaded. You start on the runway (into the wind or at the threshold you pick), on final
at the miles you set, or in the air over the place. Also from the menu (`Esc`, "Place") and the
settings (`O`), and at start-up if you tick it.

From the command line: with `scripts/run.sh --place LEMH` you start from the main runway of a
real airfield, with its terrain, its coast, forests, fields, villages, roads and runways. Any ICAO
or IATA code works. `--at 47.26,11.34` is any point, and you start in the air over it.

The first time it is downloaded (20 to 50 MB, half a minute) and saved in `~/Library/Caches/flight`.
After that it loads at once and offline (`--offline` to download nothing). Without `--place` the
world is the generated one, as always.

If you fly beyond the 96 km of the region, the next one is built in the background before
you get there and the world goes on without breaks. Once in the air, the ground takes the colour of the Sentinel-2 satellite photos (10 m per pixel close by, cloud-free,
from several summer days): 90 to 200 MB and half a minute to a minute the first time, in the
background, and from the cache after that. Until it arrives, the ground has the map's colours.
Below 500 m, where the 10 m photo would be a blur, the ground is drawn with the plots the photo
shows: each field with its sharp, straight edge, furrows, stubble or ploughed soil, dry-stone walls
or hedges between them, scrub, stones and the trees seen in the photo (high and medium quality;
those trees are visible but you can't hit them).

Cities have their real buildings, from Overture Maps (OpenStreetMap's, and where it has none those
Microsoft, Google and Esri found in aerial photos), each with its outline, courtyards, height,
roof shape (flat with its parapet and machines, gabled, hipped, domes...) and the parts it is
drawn by, so a tower stands on its podium. Their facades follow what each building is and where
it stands: plaster and brick blocks with shops below, glass towers, stone, sheds, houses, with
windows that light up at night. Bridges come from OpenStreetMap too, decks over piers high enough
over what they cross, and the suspension, cable-stayed, arch and truss ones with their towers,
cables and girders; the Golden Gate and the Brooklyn Bridge are modelled by hand from their real
dimensions. You crash into a building where its walls are, not into the box around it, you can
fly down a street, into a courtyard or under a bridge, and wreckage comes to rest on the roofs
and decks. A city adds some 60 MB (Madrid) to 120 MB (New York) and 20 to 30 s to its first download.

The data is free and needs no key: terrain from Mapzen and AWS Terrain Tiles, map from OpenStreetMap
(via OpenFreeMap), buildings and bridges from Overture Maps, and airfields from OurAirports. How
the region is built is in [crates/sim/geodata/README.md](crates/sim/geodata/README.md).

## In the browser

**On hold until the end of the project:** new work does not have to keep building for the web.
When it is picked up again, the first thing is to get `scripts/check-web.sh` passing again.

The same game runs in the browser with WebGPU: Chrome 113 or later (tested) and Safari 26 or
later (not tested yet):

```
scripts/build-web.sh     # builds the web version into dist/
scripts/serve-web.sh     # serves it at http://localhost:8080/: open it in Chrome or Safari
```

The first time, `build-web.sh` installs a pinned nightly with `rustup` (wasm threads need the
standard library built again) and `wasm-bindgen-cli`, and takes a few minutes; after that,
seconds. With `wasm-opt` installed (`brew install binaryen`) the module comes out smaller.

`dist/` is static files: 25 MB, 8 of them the module (3 compressed) and 17 the textures (the
terrain ones at 512 and in JPEG; in the program they are 72 MB). They work on any server that sends
`Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`: the game
splits its work across workers that share its memory, and the browser only allows that this way.
`serve-web.sh` sets them; opening `index.html` as a file does not work.

What changes compared to the program:

- There is no command line. The aircraft, time, weather and quality are chosen in settings (`O`) and
  the place in `M`; all of that, the favourites, the recent places and the controllers are saved in the browser.
- `medium` quality by default (change it in `O`).
- Real places download the same way, from the browser, and are saved in its cache. The first
  time they are slower than in the program, piece by piece (Menorca: a minute for the region and another
  for the satellite photo, while you fly); after that they load in seconds.
- Sound starts with the first key or click, and a controller shows up when you press one of its buttons.

## Driving

```
scripts/drive.sh
```

Builds and opens the driving game, `skylab-drive`, directly: the GT coupé (a
rear-drive sports car of some 1400 kg and 375 hp, `assets/cars/gt`; `--car modely` for the Tesla
Model Y, electric, all-wheel drive, `assets/cars/modely`) on the grid of Lakeside Park, a
4.5 km circuit by a lake in the world of seed 1, at the golden hour. `H` shows the keys. It is also
the "Cars" of Skylab's first screen, and `Esc` is its pause menu: drive on, "Change vehicle" (back
to Skylab's first screen) or quit.

| To | Press |
|---|---|
| Throttle, brake, steer | `↑` `↓` `←` `→` (or `W` `S` `A` `D`) |
| Shift up, down (the automatic shifts by itself; `M` for the sequential) | `E`, `Q` |
| Hand brake | `Space` |
| Camera: chase, bonnet, cockpit, trackside | `C` |
| Back on the track where you are / to the grid | `R` / `Shift+R` |
| Let the racing driver drive (and take over again) | `X` |
| Anti-lock brakes, traction control, stability control (on by default) | `B`, `T`, `Y` |
| Pedal help: the pedals' keys ease as the tyres spin or lock (on by default) | `G` |
| The driving line on the road: the whole of it, only where to slow down, off | `L` |
| Later or earlier in the day | `N`, `Shift+N` |
| The replay of the last lap (`Space`, `←` `→`, `↓` `↑` in it) | `F2` or `Tab` |
| The car the cameras watch | `[`, `]` |
| More rain, less | `F6`, `F7` |
| Record a video of the game with its sound, to share; again to save it in `~/Movies/Skylab` | `F12` |
| The pause menu: drive on, change vehicle (planes, drones or cars), quit | `Esc` |

A gamepad (triggers for the pedals, the left stick to steer) and a joystick (the T.16000M: its
stick steers, forward and back are throttle and brake, the trigger shifts up) work as they are; their
axes and buttons can be changed in `~/.config/skylab-drive/controls`, written the first time each is
seen. Options: `scripts/drive.sh --hour 12 --view cockpit --manual`; `--driver` lets the racing driver
drive from the start; `--race` races five rivals from a standing start (`--rivals <n>`, `--rival net`
for the trained network); without it, free practice with the ghost of your best lap; `--rain 0.6`
wets the road and takes its grip (puddles aquaplane the tyres at speed), `--hour 22` drives under the floodlights; `--shot <dir>`
writes one frame to `<dir>/drive.png` without a window. All of them in [crates/games/drive/README.md](crates/games/drive/README.md).

## Checking that everything works

```
cargo test --workspace                                  # physics, terrain, pilots, pipelines
cargo run --release -p flight -- --shot /tmp/capture    # one frame to PNG, no window
```

`--shot` writes `scene.png` and prints the milliseconds per frame. With `--air`, `--final`,
`--ext`, `--hour 18` or `--pos <north> <east>` you choose what is shown.

`scripts/check-web.sh` checks, quickly and with stable Rust, that `sim-core`, `engine`,
`world-render`, `geodata` and `flight` still build for the web, so nothing slips in that only
works on the desktop. It needs `rustup` (it adds the target if missing). While the web is
on hold there is no need to run it; `scripts/build-web.sh` makes the real version.

## How it is organised

Crates with one-way dependencies, grouped by role: `crates/engine` (engine and world rendering), `crates/sim` (simulation, real places, AI environment) and `crates/games` (the games):

| Crate | What it is |
|---|---|
| [crates/engine/engine](crates/engine/engine/README.md) | Reusable engine: window, GPU, meshes, camera, sound, 2D. Knows nothing about aircraft. |
| [crates/sim/sim-core](crates/sim/sim-core/README.md) | The simulation: aircraft, terrain, air, pilots. No graphics. |
| [crates/sim/geodata](crates/sim/geodata/README.md) | Real places: downloads and stores the maps, and builds the region from them. The only thing that uses the network. |
| [crates/engine/world-render](crates/engine/world-render/README.md) | Draws the world: terrain, sky, clouds, water, vegetation, villages, airfields, fire. Knows nothing about aircraft; has its own viewer (`cargo run --release -p world-render --example viewer`). |
| [crates/games/flight](crates/games/flight/README.md) | The game: draws its aircraft and cockpits in that world, reads the keyboard, instruments and settings. |
| [crates/games/drive](crates/games/drive/README.md) | The driving game: a car on a race circuit in the same world, its cameras, controls, sound and lap times. |
| [crates/games/remote](crates/games/remote/README.md) | A running game flown by another program over a local socket: the protocol, a client and `skylab-ctl`, the client at the command line. |

The rules, and what to read before touching each part, are in [ARCHITECTURE.md](ARCHITECTURE.md).

The first version was written in Swift and is in the git history (commit 4386a3a).

## License

MIT (see [LICENSE](LICENSE)). The textures in `assets/textures` are CC0 (see their `CREDITS.md`). What comes from other projects keeps its notice in the file itself and its license in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Real places use third-party data, downloaded on each machine and not kept in the repository:
terrain from Mapzen and AWS Terrain Tiles (SRTM, GMTED, ETOPO1, USGS 3DEP, Copernicus EU-DEM and others),
map © OpenStreetMap contributors (ODbL), © OpenMapTiles, served by OpenFreeMap, buildings and bridges
from Overture Maps Foundation (ODbL; with OpenStreetMap, Microsoft, Google Open Buildings and Esri
Community Maps, CC BY 4.0), and airfields from OurAirports (public domain), and the ground colour from Copernicus Sentinel-2 via AWS Open Data
("Contains modified Copernicus Sentinel data 2023-2025"; free for any use). The game
says so on screen while you fly a real place.
