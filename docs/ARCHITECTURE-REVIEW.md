# Architecture review (16 Nov 2026)

The review that ends phase 1's architecture work (`ROADMAP.md`): is the code shaped so that a drone,
a tilt-rotor, a car, multiplayer and the web are each a contained change? Read with
`ARCHITECTURE.md` and `ROADMAP.md`. Line numbers are of `main` at ff7a424, before this branch.

## What is good, and stays

- **One-way crates with a README contract each.** `engine` and `flight-core` know nothing of each
  other; `world-render` knows no vehicle (`crates/world-render/src/lib.rs`, `Body` and
  `ShadowShape`: a vehicle reaches it as meshes and a shadow's shape). The second game (`drive`)
  can reuse both as they are.
- **A deterministic simulation with no clock and no I/O.** Fixed step (`World::DT`), seeded `Rng`,
  terrain a pure function of the seed or of a `Region` built before the world. This is what lets
  tests, AI training, replays and a multiplayer server share one code path. Keep it.
- **Physics checked against sources.** The golden snapshot (`flight-core/tests/golden.rs`), the
  handbook and JSBSim tests. They are what makes every refactor below safe.
- **Per-type differences already behind doors.** In the game, `crates/flight/src/kinds.rs` is the
  only `match` on the kind of aircraft; the cockpits share `cockpit/` and differ by a `CabinSpec`.
  In the simulation the C172 and the Extra are two values of one `AircraftType`: the physics has no
  `if extra`.
- **Inputs are mapped** (`crates/flight/src/controls.rs`), and pilots see only plain data
  (`Observation` in, `Controls` out): the keyboard, the autopilots, the GFC 700 and the trained
  models are interchangeable.
- **The web is kept compiling** (`scripts/check-web.sh`, `engine::threads`, `engine::Instant`).

## Where the coupling is

1. **Aircraft types are Rust code.** `AircraftType::c172()` and `extra300()` are 400 lines of
   literals (`crates/flight-core/src/aircraft.rs:587-996`), plus `Pieces::C172`/`EXTRA300`
   (`wreck.rs:164-266`), `InstrumentFit::C172`/`EXTRA300` (`instruments.rs:118-196`),
   `Gfc700Fit::C172` (`gfc700.rs:67`) and `Speeds::C172` (`aircraft.rs:530`). `AircraftKind` is a
   closed enum (`aircraft.rs:262`), and the game matches on it in `kinds.rs` (7 matches),
   `models.rs:19`, `render/mod.rs:209` (`kind_index` over `AircraftKind::ALL`),
   `render/wreck.rs:74,88` and `ui/panel.rs:65-67`. A third aircraft touches ten files in three
   crates, even when it only needs new numbers.
2. **The world holds planes, not vehicles.** `World::planes: Vec<Plane>` (`world.rs:44`); `Plane`
   mixes what any vehicle needs (wind field, ground under it, ground ahead, trees near it) with what
   only an aircraft has (pitot-static instruments, lagged airspeed, turn rate, slip ball). Placing it
   (`reset_on_runway`, `reset_in_air`, `reset_on_final`) trims an aircraft; running into trees and
   buildings (`world.rs:468-620`) speaks of wings, tailplanes and legs; `collide_planes` and
   `touching` (`world.rs:962`, `1068`) use the aircraft's airframe boxes; `observe` (`world.rs:992`)
   builds one aircraft-shaped `Observation`. `Pilot` (`pilot.rs:11`) returns aircraft `Controls`.
3. **The rigid body carries the propeller.** `State` (`aircraft.rs:42`) is the body (position,
   attitude, velocity, rates) plus `prop_speed` and `blade_pitch`; the RK4 is inside `Aircraft::step`
   (`aircraft.rs:2033`). A drone or a car would copy the integrator.
4. **Fixed-size arrays with meaning by position.** `hard_points: [V3; 7]` (index 2 the propeller,
   4 the fin, 6 the tail: `aircraft.rs:2191-2224`), `floats: [_; 8]` (wings at 3 and 4, wheels from
   5), `airframe: [_; 5]`, `strip_edges: [f64; 4]` (four strips a side), `MOST_WHEELS = 3`,
   `PieceKind` = fuselage, two wings, tail. Fine for fixed-wing aircraft; a data file must respect
   them (validated), and a car or a drone will have its own, not these.
5. **The simulation returns player text.** `Aircraft::crashed() -> Option<&'static str>`, `Damage::last`,
   `Wheel::broken` and the strings in `world.rs:552-617` are player text inside the physics.
   Harmless today; a network protocol, replays and translations want codes the game words.
6. **Wrecks are an aircraft's.** `Pieces` cuts a body into fuselage, wings and tail and
   `world-render` finds a wreck's mesh by `Pieces::kind`, an `AircraftKind`
   (`world-render/src/renderer.rs:873`). A car's crash needs other pieces (doors, wheels).
7. **One ground plane per vehicle per step.** `Environment` (`aircraft.rs:80`) is a plane through the
   point below the centre of gravity, sloped from samples 4 m away. Enough for an aircraft; a car
   needs the ground under each tyre, and roads seen from 1 m (seam 6).
8. **`game.rs` is 2857 lines.** `Session` holds keys, cameras, autopilots, traffic, messages and
   restarts. Not urgent; it should split by what it does (view, traffic, keys) before a second game
   copies from it.

## What would hurt

- **A drone (phase C).** Today: a new `Plane`-like struct, a second `World::planes`, a copy of the
  RK4, the contacts and the wreck handling, a second observation and pilot trait. After this branch:
  a `Multirotor` struct, one `Dynamics` variant and the matches the compiler then asks for.
- **A car (phase D).** Same as the drone, plus items 4, 6 and 7: contacts per wheel against the
  terrain and roads, its own pieces, a fine ground mesh. `world-render` is ready to draw the world for
  it; `drive` would reuse `engine` and `world-render` as they are.
- **Multiplayer.** The single-authority `World` is the server. What hurts: player text inside the
  physics (item 5); trees felled per aircraft (`world.rs:677`, a tree one aircraft felled still
  stands for the others); transcendental functions (`sin`, `powf`) differ between platforms, so a
  native server and web clients cannot run lockstep: the server must send state, not only inputs.
  `Controls` and `Observation` are already plain data.
- **The web.** Aircraft defined in files read from disk would break it; they must be embedded too
  (done here). The rest is in `ROADMAP.md` seam 7.

## The plan, in order

| # | What | Where | When |
|---|---|---|---|
| 1 | Aircraft as data: `assets/aircraft/<key>/aircraft.ron`, loaded and validated, embedded as a fallback; `AircraftKind` a key into the fleet; the game's per-type visuals chosen by the file's `visual` | `flight-core::aircraft::file`, `flight::kinds` | done |
| 2 | Vehicles: `World::vehicles`, `Vehicle`, `VehicleId`, `enum Dynamics { Aircraft }`, `enum Controls { Aircraft }`, `Observation` with a part per kind; the rigid body's integration shared (`body`); contacts, wrecks, wind and observations through `Dynamics` | `flight-core::vehicle`, `body`, `world` | done |
| 3 | The mechanical renames and moves (below) | every crate | done |
| 4 | Player text out of the physics: `&'static str` messages become an enum of what broke, worded by the game | `flight-core`, `flight` | before multiplayer |
| 5 | Wrecks per vehicle kind: `Pieces` with a list of pieces instead of `PieceKind`'s four, looked up by key | `wreck`, `world-render` | with the car |
| 6 | Ground per contact point; trees felled for the world, not per vehicle | `world`, `terrain` | with the drone (it lands on anything) |
| 7 | `game.rs` split by concern | `flight` | before `drive` |
| 8 | Aircraft models as glTF assets named by the file's `visual` (no Rust for a new look) | `flight` | done (below) |

## Done in this branch

Steps 1 and 2 of the plan, each with every test green and the golden snapshot unchanged; a flight of
a C172 on circuits and an Extra holding in wind and turbulence for 200 s, hashed bit by bit (state,
air data, instruments), gave the same hash before and after each.

- **A new type of aircraft is a file**, `assets/aircraft/<key>/aircraft.ron`, and no Rust unless it
  needs physics the model has not got (`crates/flight-core/README.md`, "Aircraft files", says what
  those are). `flight-core/tests/fleet.rs` flies a made-up third type that is only a file round a
  circuit. A new *look* is still code: a model in `flight/src/models.rs` (`Model`) and, if it has its
  own, a cockpit (`kinds::Cockpit`); until then a file names an existing one.
- **A new kind of vehicle** is its struct (`Multirotor`, `Car`) with its force model integrated by
  `body::Rates`, a variant of `Dynamics` and of `Controls`, its part of the observation
  (`VehicleObservation`), and the arms the compiler then asks for: every `match` on `Dynamics`
  (`vehicle.rs`) and every `let Dynamics::Aircraft(..)` (aircraft-only code: the placements, the game).
  The world, the wind, the obstacles, the collisions and the wrecks' handling stay as they are.

Left for later, besides the plan's rows 4 to 7: the game's own names (`Session::planes`,
`PlaneVisual`) are the flight game's and were left. (`Pieces::C172`, kept for `world-render`'s tests
until `world-more` merged, is gone: they take `AircraftType::c172().pieces`.)

## The renames (done, branch `renames`, 16 Nov 2026)

Nothing in them changed behaviour: every test green, the golden snapshot unchanged, every `--shot`
the same to the pixel.

- `flight-core` is `sim-core` (`sim_core`), `flight-env` is `sim-env` (`sim_env`). `sim-env`'s
  server binary keeps the name `flight-env` (`target/release/flight-env`), and so do the strings it
  prints and sends: the Swift trainer runs it by that name.
- The crates are grouped by role: `crates/engine/{engine, world-render}`, `crates/sim/{sim-core,
  geodata, sim-env}`, `crates/games/{flight}` (`drive` goes there too). The paths that climb to the
  repository's root (the embedded aircraft, the textures, `geodata`'s fixtures) went one level up.
  Every crate keeps its name, so `cargo … -p <crate>` is as before.

The paths and line numbers above this section are of `ff7a424`, before both.

## Row 8: the looks are files (17 Nov 2026)

A type's look is the folders its `visual` names: `<model>/exterior.glb` and `exterior.ron` (the mesh
in named nodes, what moves each node by which of the simulation's numbers and about which axis, the
lights, the shadow, how it stands parked) and `<cockpit>/cockpit.glb` and `cockpit.ron` (the cabin
in named nodes and its glass, a triangle's material saying what it is made of; how each node moves
with the controls, the spots, the instruments fitted in the canvas, the print as shapes, the levers
and the keys and knobs the mouse works). `engine::gltf` reads and writes glTF 2.0; the game's
`models`, `cockpit::file`, `cockpit::instruments` and `cockpit::print` are generic, and `kinds`
reads each look once and checks it at the start. The C172's and the Extra's files were exported
from the procedural models and cockpits (`export.rs`, deleted with them after the export): every
`--shot` the same but for a few pixels a level off. A third aircraft is only files under
`assets/aircraft/<key>/` (its `aircraft.ron` and, for a look of its own, those four).

Not done: doors (the simulation has no door to open), textures drawn per pixel (the world's objects
take a texture's colour at each vertex: their pipeline has no texture coordinates). The web build
downloads the looks with the textures (`scripts/build-web.sh`).
