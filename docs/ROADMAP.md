# Roadmap and the architecture it needs

Where the project is going, and the shape the code must take so that each step is a contained
change rather than a rewrite. Read with `ARCHITECTURE.md`; its three rules still hold, including
"only what is used": nothing below is built before the step that needs it. What this document
fixes is *where* each piece goes when its time comes, and the habits that keep that cheap.

## The road

| Phase | What | Needs from the architecture |
|---|---|---|
| Now | C172, physics, damage, cockpit, scenery, textures, trained pilots | — |
| A | Second aircraft: Extra 300 | Aircraft types as data |
| B | Look and feel: HDR and bloom, explosions, progressive damage (done: wings and tailplane torn at a station, surfaces lost one by one, partial contacts, trees by energy), instruments from pressures (done: pitot-static with QNH and blocked pitot and static, vacuum gyros that drift and run down, compass errors, the G1000 on the same air data; the air of the day in `Weather::air`), sea (done: waves for the wind as trains of sines filtered by the pixel, the sun's and the moon's glitter, Fresnel, sky and clouds mirrored, the colour of the water and the bed seen through it by the real depth, breakers and whitecaps, calm rivers; the sea's height stays flat, as the simulation floats on it), runway lights (done: every aerodrome's lights as data in `sim_core::lights` after ICAO Annex 14 and the FAA, by class of runway: edge, threshold and end, centre line and touchdown zone, MALSR or ALSF-2 with sequenced flashers, REIL, PAPI or VASI, taxiway and apron edges, floodlights that light the apron, beacon, obstacle lights; drawn as points of light in candelas, by their beams, through the haze, with halos in mist, set by the hour and the visibility), atmosphere (done: Rayleigh, Mie and ozone over a round planet with Hillaire's tables, so the sky, sunsets, twilight, the haze over the ground, the sea, the clouds and the lights, and the light of the sun, the moon and the sky on everything come from one model; the haze and fog from `Weather::visibility` and the humidity, clearer air higher up; the night drawn by an exposure that keeps part of the real range; in `sim-core`, the standard atmosphere up to the stratosphere and humid air), real places (done: crate `geodata` downloads and keeps the AWS Terrain Tiles, OpenStreetMap through OpenFreeMap and OurAirports, and builds a region round an aerodrome or a point: heights at 12, 30 and 400 m, the coast, woods, fields, towns, roads, every runway with its surface, lights and apron; `--place`, `--at` and three places in the settings) | Post-processing in `engine`; a terrain that can come from data |
| C | FPV drone (done, 17 Nov 2026: `sim_core::multirotor`, the 5-inch quad of `assets/aircraft/fpv5`, its self-levelling flight controller (no flight modes), the game's FPV camera, chase camera, OSD and prop sound, RC transmitters as joysticks, the replay and recordings, `sim-env`'s `hover` task), then an Avatar-style tilt-rotor (done, 18 Nov 2026: the Kestrel of `assets/aircraft/kestrel`, a multirotor with tilting nacelles, a wing and a tail) | Vehicles, not aircraft |
| D | A realistic driving game (begun, 17 Nov 2026: `sim_core::car`, a GT coupé as data, `sim_core::circuit` with Lakeside Park in the world of seed 1, its look in `world-render`'s `track.rs`, the game `crates/games/drive`, `sim-env`'s `drive` task; the plan in `docs/CARS.md`) | Tyres and suspension; ground seen from 1 m; a second game on the same world |
| Later | VOR/ILS/NDB, radio and ATC, multiplayer with AI pilots | `World` as the single authority (already so) |

Phase 1 is a realistic and spectacular game on an architecture that scales to any vehicle and
any physics: it ends with the realistic cockpit, the real autopilot (the C172's GFC 700,
`sim_core::gfc700`), the place picker (done: `flight`'s `picker`, key M, 16 Nov 2026), the architecture review, the world's visual loose ends
(sea, lights, scanned CC0 textures, Sentinel-2 ground colour in real places, done: `geodata::imagery`, branch `world-more`), a second pass on the
cockpits with those textures and better models, and the web version (seam 7, done: branch `web`; paused until the end of the project). Phases C, D and
Later, with the rest set aside, are in `docs/PHASE2.md`; fine tuning
waits in `docs/PHASE3.md`.

## Target shape

```
crates/engine/engine          window, input, GPU, post-processing, meshes, glTF, audio, egui   (any game)
crates/engine/world-render    draws the world: terrain, sky, clouds, water, scenery, fire     (any vehicle game)
crates/sim/sim-core           world, terrain, weather, bodies, contacts, damage, wrecks, pilots (any vehicle)
  vehicles/aircraft           fixed-wing dynamics, its systems and instruments' physics
  multirotor                  drones: motors, propellers, battery, flight controller       (phase C, done)
  car                         tyres, suspension, drivetrain, the racing driver; circuits     (phase D, begun)
crates/sim/geodata            downloads and caches real terrain, water, roads, airports      (done)
crates/sim/sim-env            AI environment over the socket (binary `flight-env`), any vehicle
crates/games/flight           the flight game: aircraft and drone visuals, cockpits, panels, HUD, keys
crates/games/drive            the driving game                                              (phase D, begun)
crates/games/remote           a running game flown by another program: socket, client, `skylab-ctl`
trainer/                      Swift + MLX
```

Dependencies stay one-way: `engine` and `sim-core` know nothing of each other; `geodata` feeds
`sim-core` plain data and is the only thing that touches the network; `world-render` uses
`engine` and `sim-core`; games use all three.

## The seams, and when each one is cut

Each seam is cut in the phase that first needs it, as its own commit or branch, with every test
green before and after and the C172 golden snapshot unchanged (it is the guard that a refactor did
not change the flight).

1. **Aircraft type as data** (done: one `AircraftType` in phase A, branch `extra300`; files in branch
   `architecture`, 16 Nov 2026). Every type is a file, `assets/aircraft/<key>/aircraft.ron`: geometry,
   mass and inertia, aerodynamics, engine and propeller, gear contact points, flaps, structural
   limits, floats, pieces, speeds, instruments and autopilot, and the names of the model and cockpit
   the game draws it with. Read and checked at start, built in as a fallback (tests, web). A third
   type is a file and no code (`sim-core/tests/fleet.rs`).
2. **Post-processing and HDR in `engine`** (phase B, branch `hdr`). Games write linear HDR; the
   engine tonemaps once. Needed by everything visual after it.
3. **Terrain from a source** (done, branch `real-places`, 16 Nov 2026). `Terrain` keeps its API;
   inside it the heights, water, roads and airports come either from the seed or from a
   `sim_core::Region` that `geodata` built before the `World` was created
   (`Terrain::from_region`, `World::with_terrain`). `sim-core` never waits on the network, so it
   stays deterministic: the same region kept on disk, the same world. The generated world did not
   change a bit (the golden snapshot, every `--shot` to the pixel). The buildings of the maps' footprints
   (zoom 14, with obstacles to match) are done (branch `world-details`); since 17 Nov 2026 they come
   from Overture Maps with their roofs, parts and facades, each a solid of its own outline
   (`sim_core::solids`), with the bridges. Still to do: the minor
   streets of a city (zoom 14), aprons and taxiways from the maps, runways that slope. A region
   that follows the aircraft beyond its 96 km is done (`geodata::follow`, branch `world-more`).
4. **Vehicles** (done in branch `architecture`, 16 Nov 2026, pulled into phase 1; the rename of
   `flight-core` to `sim-core` done too, `docs/ARCHITECTURE-REVIEW.md`). `World` holds `Vehicle`s by
   `VehicleId`. A vehicle is a rigid body (`body::Body`: position, attitude, velocity, rates) plus a
   dynamics that turns its own controls and the environment into forces and moments:
   `enum Dynamics { Aircraft(Aircraft), Multirotor(Multirotor) }`, to which `Car(..)` is added — an
   enum, not a trait object, so stepping stays fast, data-oriented and deterministic. Shared by all:
   the Runge-Kutta step (`body::Rates`), contacts with obstacles and with each other, wrecks, the
   wind field, the ground under and ahead, the common part of the observation. Controls are
   `enum Controls { Aircraft(..), Multirotor(..) }`; a `Pilot` gets the vehicle's kind of controls and
   returns the same; `Observation::vehicle` is what only that kind sees. Adding a kind: its struct, a
   variant of each enum, and the arms the compiler then asks for (`let Dynamics::Aircraft(..)` in code
   that only an aircraft has became a refusal to compile with the multirotor, 17 Nov 2026;
   `Vehicle::aircraft()` and `Observation::aircraft()` are `Option`s since, and every caller says what
   it does with the others). The multirotor was the test: it is a type file
   (`assets/aircraft/<key>/multirotor.ron`) beside the aircraft's, its look the same glTF and RON, and
   in the game only generic multirotor code (`flight`'s `multirotor.rs`, `ui/osd.rs`): no drone of
   its own. What it left for later: a crashed multirotor is its own wreck (no pieces, no fire), it
   has no prop wash on other vehicles, and `sim-env`'s `ModelPilot` flies aircraft only (a model
   trained on `hover` runs in the environment, not yet in the game).
5. **World rendering out of the game** (done, branch `world-render`, 16 Nov 2026, before the rest of
   phase B, which is almost all world rendering). The world (terrain, sky, water, scenery, fire,
   qualities) is `crates/engine/world-render`, with its own README, tests and a viewer (free camera, hour,
   weather, no aircraft), and can be improved on its own branch without touching the game. It knows
   no vehicle: the game adds meshes and asks for draws of them, gives the shape of the shadow of the
   body the view belongs to, the meshes of its wrecks and what is parked on the aprons, and draws
   its cockpit with pipelines of its own on the world's shaders between the sky and the rest.
   Every `--shot` stayed the same to the pixel. The test still to pass is that `flight` and `drive`
   both draw the same world without copying a line.
6. **Ground seen from 1 m** (phase C/D). The drone first, the car then, need roads with their own
   fine mesh (camber, kerbs, markings), close-up ground detail and contacts finer than a plane.
   Roads are already data in `terrain`; their mesh goes in `world-render`. The FPV quad flies on the
   ground and the obstacles as they are (its contacts on the ground's plane under it, the trees and
   buildings of `scenery`, `World`'s other vehicles): what it showed is that the ground's textures
   and the grass hold up from a metre, and the roads and the aprons are where it looks flat; this
   seam is still to cut, with the car.
7. **Web** (done, branch `web`, 16 Nov 2026; **paused by the owner the same day** until the end of the
   project: new work does not have to keep it compiling or run `scripts/check-web.sh`; when it is
   taken up again, the first step is to make `scripts/check-web.sh` pass). The game
   runs in the browser with WebGPU, tried in Chrome, meant for Safari 26 too
   (`scripts/build-web.sh`, `scripts/serve-web.sh`; the README says how). The GPU is set up
   asynchronously (`engine::app`); `engine::threads` are Web Workers sharing the module's memory
   (wasm threads: a pinned nightly rebuilds std with atomics, and the page is served cross-origin
   isolated), with the rule that the page's thread never waits
   (`threads::wait`, `threads::lock`); the textures are downloaded before the game starts
   (`engine::files`, the detail maps at 512 in JPEG); the settings, controller mappings, favourites
   and recents are kept in `localStorage`; real places load on workers with the browser's
   synchronous requests and are kept in its Cache Storage by a service worker (`geodata`'s
   `disk.rs`); sound through WebAudio after the first gesture, pads through the Gamepad API. Code
   that runs in the game still uses `engine::threads` (not `std::thread`) and `engine::Instant`
   (not `std::time::Instant`, which panics there); a native-only dependency goes under
   `[target.'cfg(not(target_arch = "wasm32"))']`; a lock the page's thread shares with a worker
   must never make it wait. `scripts/check-web.sh` checks the crates still compile for the web
   with stable Rust. Native-only by design: `sim-env`'s socket server, the trainer, the command
   line's tools (`--shot`, `--controllers`). Not on the web: the command line (what is flown and
   where comes from the settings), and real places download a tile at a time there (a worker
   cannot start threads), some twice as slow the first time.

## Habits from now on (every branch, every agent)

- **Name by role, not by C172.** New shared code says `vehicle`, `body`, `contact`; only code inside
  the aircraft module says `aircraft`, `wing`, `flap`.
- **Vehicle visuals behind one door.** Everything that draws a particular aircraft (its mesh,
  moving parts, lights, interior, panel) is reached from one place per type, not spread through
  `flight/src/render/mod.rs`. The world's drawing (`world-render`) does not know which vehicle is in it.
- **Physics stays pure.** No clock, no randomness without a seeded `Rng`, no I/O in `sim-core`.
  This is what lets AI training, replays, multiplayer and tests all work on every vehicle.
- **Effects are the world's, causes are the vehicle's.** Fire, smoke, wrecks, splashes, skid marks
  and craters belong to the world (`wreck`, `world-render`); the vehicle only reports what broke and
  where. A car crash then gets the same fire for free.
- **Inputs are mapped, not hard-wired.** Keys, sticks, wheels and RC transmitters go through the
  mapping layer (`controls.rs`) into the vehicle's controls, never read in the vehicle code.
- **Observations are plain data** per vehicle kind, so `sim-env` can serve any of them.

## What not to do

- No generic "plugin" or ECS framework in advance: an enum per kind and plain structs.
- No second world or second renderer for a new vehicle: if one seems needed, the seam is wrong.
