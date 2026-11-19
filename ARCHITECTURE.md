# Architecture

A flight simulator in Rust, in crates with one-way dependencies. It was first written in Swift;
that version is in the git history (commit 4386a3a) and the numbers in `sim-core/tests/golden.rs`
were taken from it.

```
crates/engine/engine        reusable: window, input, GPU, meshes, camera, sound, 2D overlay
crates/engine/world-render  draws the world of sim-core with engine: terrain, sky, water, scenery, fire. No vehicles
crates/sim/sim-core         the simulation: aircraft, terrain, air, pilots. No graphics, no OS
crates/sim/geodata          real places: downloads and keeps open map data, builds a Region of it. The only network
crates/sim/sim-env          AI pilots over a socket (sim-core only; its binary is `flight-env`)
crates/games/flight         the game: its aircraft and cockpits drawn into that world, keys to pilots, instruments
crates/games/drive          the driving game: a car on a race circuit of that world, its cameras, controls, sound, lap times
crates/games/remote         a running game flown by another program: the socket, its messages, a client, `skylab-ctl`
```

`engine` and `sim-core` depend on nothing of ours and not on each other. `geodata` depends on
`sim-core` only (it builds its `Region`); `world-render` on `engine` and `sim-core`; `flight`
on all of them (and `sim-env`); `drive` on `engine`, `world-render` and `sim-core`. `remote`
depends on `sim-env` only (its frames and its JSON) and knows no game; `flight` uses it.

## The rules

1. **`engine` knows nothing about aircraft.** If a type mentions runways, wings or instruments it
   belongs in `world-render` (runways) or `flight` (wings, instruments). Other games (cars,
   shooters) are meant to reuse `engine` as it is.
2. **`sim-core` has no graphics and no clock.** It advances only when `step()` is called, with a
   fixed step of 1/240 s, and the same inputs always give the same result. That is what lets it run
   headless, faster than real time, in tests and for AI pilots.
3. **Only what is used.** A piece goes into `engine` when the game needs it, in the simplest form
   that serves it. It is generalised when a second user of it exists, not before.
4. **`world-render` knows no vehicle.** It draws the world for any game; a vehicle reaches it as
   meshes, draws, the meshes that cast its shadow and a shadow's shape the game gives
   (`crates/engine/world-render/README.md`).

## Working on one part without knowing the rest

Each crate has a `README.md` with its public contract. To change a part, read that README and the
module you are changing; you should not need anything else.

| To work on | Read | Check with |
|---|---|---|
| Flight physics, engine, gear | `crates/sim/sim-core/README.md`, `src/aircraft.rs` | `cargo test -p sim-core` |
| A type of aircraft: its numbers, or a new one | its file `assets/aircraft/<key>/aircraft.ron`; the format in `crates/sim/sim-core/README.md` ("Aircraft files") | `cargo test -p sim-core` (a new type: a test like `tests/fleet.rs`) |
| Terrain shape, airports, towns | `crates/sim/sim-core/README.md`, `src/terrain.rs` | `cargo test -p sim-core` |
| Real places: what is downloaded, from where, how a region is built (heights, coast, cover, roads, towns, runways), the cache | `crates/sim/geodata/README.md` | `cargo test -p geodata`; `cargo run --release -p geodata --example region -- <ICAO>`; the viewer's `place=<ICAO>` |
| Which lights an aerodrome has, where, their colours, intensities and beams, and how bright the tower sets them | `crates/sim/sim-core/README.md` (lights), `src/lights.rs` | `cargo test -p sim-core --lib lights`; how they look, in the viewer (`final=<m>`) |
| Instruments' physics: pitot-static, vacuum and gyros, compass, air data | `crates/sim/sim-core/README.md`, `src/instruments.rs` | `cargo test -p sim-core --test instruments` |
| Pilots (keyboard assist, AI) | `crates/sim/sim-core/README.md`, `src/pilot.rs` | `cargo test -p sim-core` |
| The C172's autopilot (Garmin GFC 700): modes, servos, flight plan, alerter | `crates/sim/sim-core/README.md` ("gfc700"), `src/gfc700.rs`; its keys, clicks and displays in `crates/games/flight/README.md` | `cargo test -p sim-core --test gfc700` |
| Window, input, GPU plumbing, sound | `crates/engine/engine/README.md` | `cargo test -p engine` |
| How the world looks: terrain, sky, clouds, water, scenery, aerodromes, crashes (fire, smoke, fragments, splashes), quality | `crates/engine/world-render/README.md` | `cargo test -p world-render`; `cargo run --release -p world-render --example viewer -- --shot <dir> [crash=<s>]` and look at the PNG; compare before and after with `engine`'s `compare` |
| A car's physics (tyres, suspension, drivetrain), a type of car, race circuits, the racing driver | `crates/sim/sim-core/README.md` ("car", "Car files", "circuit"), `src/car.rs`, `src/car/`, `src/circuit.rs`, `assets/cars/<key>/car.ron`, `assets/circuits/<key>.ron` | `cargo test -p sim-core --test car --test circuit` |
| How a circuit looks (asphalt, kerbs, run-off, barriers, stands and their crowd, boards, floodlights, the wet road) | `crates/engine/world-render/README.md` ("Race circuits"), `src/track.rs`, `src/track.wgsl` | `cargo test -p world-render track`; `scripts/drive.sh --shot <dir>` |
| The driving game: cameras, controls, sound, lap times, how the car is drawn (paint, glass, lamps), what its tyres leave (marks, smoke, spray) | `crates/games/drive/README.md` | `scripts/drive.sh --shot <dir>` and look at the PNG |
| The game flown by another program (`--remote`, `skylab-ctl`): the messages, the controls, the state, the orders | `crates/games/remote/README.md` and `PROTOCOL.md`; `crates/games/flight/src/remote.rs` | `cargo test -p remote`; `cargo test -p flight remote` |
| A car's model: its shape, its materials | `crates/games/drive/README.md` ("The car's model"), `crates/games/drive/examples/car_model/` | make it again (the command is in the README) and look at `scripts/drive.sh --shot <dir> --orbit 150 5 4.2` |
| What the lens adds to any game's picture: vignette, a blur of speed, the sun's flare, depth of field | `crates/engine/engine/README.md` ("From light to picture"), `src/post.rs`, `src/post.wgsl`, `src/focus.wgsl` | `cargo test -p engine --test post` |
| How the aircraft look and are drawn into the world, their shadow, the cockpits | `crates/games/flight/README.md`, `src/render/`, `src/cockpit/` (what every cockpit shares), `src/kinds.rs` | `cargo run -p flight -- --shot <dir>` and look at the PNG |
| Recordings: a video's shots rendered frame by frame, their cameras, lens, slow motion and sound | `crates/games/flight/README.md` ("Recordings"), `src/record.rs`, `src/audio.rs` (`Mixer`), `scripts/video/` | `cargo test -p flight record audio`; `--record <dir> --script <file> --storyboard` and look at the PNGs |
| Instruments, help, the manual (F1), settings | `crates/games/flight/README.md`, `src/ui/`, `src/manual/` | same; the manual also `cargo test -p flight manual` |
| The menus: the first screen (planes, drones or cars; the cars are the driving game, started in the game's place: `engine::program`), the start and pause screen (Esc), the settings' panel (O), the aircraft's pictures; their look and their pictures | `crates/games/flight/README.md` ("The menus"), `src/ui/menu/` | `cargo test -p flight menu`; see them with `--menu <which> --shot <dir>`; `scripts/menu-shots.sh` takes their pictures again |

Every visual change is checked with `--shot`, which renders one frame to a PNG without opening a
window. Every physics change is checked against the handbook figures in the tests.

## The simulation contract (`sim-core`)

- `World` owns the terrain, the weather, the time and **any number of vehicles** (`Vehicle`, by
  `VehicleId`). One process is the single authority over all of them. A vehicle is a rigid body
  moved by its `Dynamics`, an enum with a variant per kind: only `Aircraft` today (each aircraft a
  type read from its file, `assets/aircraft`).
- Each vehicle is driven by a `Pilot`: something that receives an `Observation` and returns
  `Controls` of the vehicle's kind (`Controls::Aircraft`). The human at the keyboard, the keyboard
  assist, the autopilots and an AI model are all pilots and are interchangeable. The simulation does
  not know which kind is flying.
- `Observation` is plain data: position and attitude, the nearest runway, the other vehicles nearby,
  and what only its kind has (an aircraft's instruments). `Controls` is plain data too. Both can be
  sent over a socket.

### Planned on top of this, not built yet

The road ahead (more aircraft, drones, a driving game, real places) and where each piece goes is in
`docs/ROADMAP.md`.

- **AI pilots out of process.** A small server exposing `Observation` in and `Controls` out over a
  local socket, so a model in another program can fly an aircraft. Later the observation can also
  carry pixels: `flight` renders the view from any aircraft's cockpit into an image, which is what
  `--shot` already does for one.
- **Several aircraft at once.** The human and two or three AI pilots, each in its own aircraft, in
  the same `World`. The renderer draws all of them and follows the one chosen.
- **Other machines.** Not designed yet. The single-authority `World` is the natural server.

## Coordinates

- World: north, east, down, in metres, as `f64` (the world is unbounded).
- Body: x forward, y right, z down.
- Render: x east, y up, z south, **relative to the camera**, as `f32`. `world_render::to_render` converts.

## Conventions

- Everything in English: comments, identifiers, docs and the text shown to the player.
- No `unsafe` outside `engine`.
- Shaders write linear HDR light, without tone mapping or clamping; the engine makes the picture
  once, at the end (`crates/engine/engine/README.md`, `Post`).
- A new dependency needs a reason written next to it in `Cargo.toml`.
