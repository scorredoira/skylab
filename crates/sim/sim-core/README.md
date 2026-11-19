# sim-core

The simulation: the aircraft (a Cessna 172 and an Extra 300L), multirotors (a 5-inch FPV quad and a 5-inch racing quad), a car (a GT coupé) and race circuits, the terrain, the air, and pilots. No graphics, no clock, no OS.
Dependencies: `glam` (`DVec3`/`DQuat`, all `f64`), and `serde` with `ron` to read the types of aircraft and multirotor from their files. Ported from the Swift original in the git history (commit 4386a3a).

## Conventions

- World axes: north, east, down, metres. Altitude is `-pos.z`. Body axes: x forward, y right, z down.
- `State.q` rotates body into world; `State.vel` and `omega` are in body axes.
- SI units and radians everywhere. `KNOTS`, `FEET`, `DEG` convert (`70.0 * KNOTS` is m/s).
- Time advances only in `World::step()`, by `World::DT` = 1/240 s. Same inputs, same result.

## Modules

| Module | What it is for | Main items |
|---|---|---|
| `math` | Units, helpers, attitude, RNG, noise | `clamp` `lerp` `smoothstep` `interpolate` `remainder`; `quaternion(heading, pitch, roll)`, `euler_angles(q) -> Euler`, `act(q, v)`, `inverse(q)`; `Rng`; `hash2`, `noised`, `fbm` |
| `atmosphere` | Standard atmosphere and the air of the day | `Atmosphere` (sea-level pressure and temperature, so a deviation from the standard at every height, and relative humidity; `Weather::air`, the one source of density, pressure and temperature), its `density`, `vapour_share`, `vapour_pressure`; `density(alt)`, `pressure(alt)`, `temperature(alt)` of the standard day, troposphere and isothermal stratosphere above `TROPOPAUSE` (11 km); `saturation_vapour_pressure`, `qnh`, `pressure_altitude`, `impact_pressure`, `calibrated_airspeed`, `mach` |
| `terrain` | The world a seed generates, or a real place's | `Terrain`, `Airport`, `Detail`, `Town`, `Road`, `Lake`, `Ground`, `Surface` |
| `region` | A real place as plain data, built by `geodata` before the world; the colour of its ground from above, for drawing (`Imagery`, never read by the simulation) | `Region`, `Grid`, `Cover`, `Shares`, `Footprint`, `Roof`, `Use`, `Material`, `Bridge`, `Carries`, `Structure`, `Imagery` |
| `roofs` | A building's roof as a shape: how high it stands over each point of the outline, for what is hit and what is drawn | `Frame`, `Shape`, `profile`, `onion_radius` |
| `bridges` | A bridge's parts from its line and structure: deck, piers, towers, cables, hangers, arches, girders | `parts`, `Part`, `PartKind`, `Piece`, `main_span`, `tower_height`, `deck_depth` |
| `solids` | A real place's buildings and bridges as solids, found by squares of 16 m | `Solids`, `Hit` |
| `wind` | Wind, lift over slopes, turbulence | `Weather` (shared), `WindField` (one per aircraft) |
| `instruments` | What the dials and displays read, from the pitot-static system, the vacuum and gyros, the compass | `Instruments`, `InstrumentFit`, `AirspeedMarks`, `Readings`, `AirData`, `Failures`, `PitotBlock`, `Sense`, `Earth`, `GyroDrive`, `StaticPorts` |
| `aircraft` | The flight model, the types of aircraft and what breaks | `Aircraft`, `AircraftType`, `AircraftKind`, `Wheel`, `Governor`, `Speeds`, `Visual`, `Load`, `State`, `AircraftControls`, `AircraftObservation`, `Environment`, `FlightData`, `Touch`, `Trim`, `Damage`, `SurfaceState` |
| `scenery` | Where trees and buildings stand, as instances to draw and as obstacles | `trees`, `houses`, `tree_obstacles`, `house_obstacles`, `aerodrome_obstacles`, `Obstacle`, `Shape`, `BLOCKS` |
| `course` | Race courses for multirotors as data: gates placed in a world, the trail cut for them through the forest, the race round them, a racing pilot | `Course`, `CourseGate`, `Trail`, `Cut`, `cuts`, `GateShape`, `GateKind`, `Track`, `PlacedGate`, `Race`, `Event`, `racer::Racer` |
| `lights` | The lights of the aerodromes, as data | `aerodrome`, `Light`, `Beam`, `Slope`, `Flash`, `Color`, `System`, `Class`, `setting`, `AIM`, `GLIDE_PATH` |
| `wreck` | The pieces of an aircraft that has come apart or been torn, its fragments, its fire and the marks it leaves | `Wreck`, `Piece`, `PieceKind`, `Portion`, `Span`, `Slice`, `Cut`, `Fire`, `Fragment`, `Scar`, `Splash`, `fuel_released` |
| `world` | Many vehicles in one world: stepping them, their collisions, their wrecks, what each pilot sees | `World`, `Start`, `Observation`, `VehicleObservation` |
| `vehicle` | One vehicle of any kind and what it shares with the others: wind, ground, obstacles | `Vehicle`, `VehicleId`, `Dynamics`, `Controls` |
| `body` | The rigid body every vehicle is, and its Runge-Kutta step | `Body`, `Rates` |
| `multirotor` | Drones: motors, propellers, battery and frame as data, and their flight controller (self-levelling, no modes) | `Multirotor`, `MultirotorType`, `MultirotorKind`, `MultirotorControls`, `StickRates`, `MultirotorObservation`, `MultirotorLook`, `Battery`, `Spin` |
| `car` | Cars: a body on four sprung wheels, Magic Formula tyres, engine, clutch, gearbox, differential, brakes, air; the types of car as data; the ground and walls they run on; a racing driver | `Car`, `CarType`, `CarKind`, `CarControls`, `CarObservation`, `CarData`, `CarLook`, `WheelState`, `TyreFit`, `Surface`, `Patch`, `Wall`, `CarGround`, `TerrainGround`, `Plane`, `wet::RoadWater`, `wet::wet_grip`, `slipstream`, `driver::Driver`, `driver::Character`, `driver::RacingLine` |
| `circuit` | Race circuits as data: a centre line with its widths, kerbs, run-off and barriers laid on the terrain; where a point is on it, its ground, its walls, the earthworks the terrain is cut to, the grid | `Circuit`, `CircuitFile`, `Sample`, `Located`, `Runoff`, `BarrierKind`, `Prop`, `PropKind` |
| `pilot` | What moves the controls | `trait Pilot`, `KeyboardPilot`, `ASSIST_PITCH`, `ASSIST_BANK`, `ASSIST_LOAD` |
| `autopilot` | A pilot that flies the whole aircraft to a goal | `Autopilot`, `Goal`, `Vertical`, `Waypoint`, `Phase`, `Gains`, `Pattern` |
| `gfc700` | The C172's autopilot, a Garmin GFC 700, on the G1000's data | `Gfc700`, `Gfc700Fit`, `AfcsKey`, `AfcsKnob`, `LateralMode`, `VerticalMode`, `FlightPlan`, `Leg`, `Approach`, `Alert`, `Disconnect`, `AltitudeAlert` |

`aircraft` knows nothing about terrain or weather: `world` condenses them into an `Environment`
(air density, air velocity, a ground plane and its kind) for each step.

### terrain
`Terrain::new(seed)` is `Send + Sync`; its caches sit behind a `Mutex`. Everything is a pure function
of position: `raw(n, e)` (height before airports), `ground(n, e)`, `height(n, e)`,
`surface(n, e, &ground, slope, town)`. Features live on grids: `airport(cell)`, `town(cell)`, and by
rectangle `airports(..)`, `towns(..)`, `roads(..)`; `nearest_airport(n, e)`. For bulk queries gather
`airports(min_n, max_n, min_e, max_e)` once and call `ground_with(n, e, &airports)`.
`hash2`, `noised` and `fbm` are bit-exact: changing them changes every world.

**Lakes** (a generated world's; a region's water is its maps'): `lake(cell)` on a grid of 12 km
(`LAKE_CELL`), `lakes(..)` by rectangle. About one cell in five has one where a river's valley
widens: its middle is the most valley-like point of the cell, its water stands 2 to 6 m over the
river's bed there and fills the valley floor as far as the ground is lower, within an oval of 0.9
to 5 km round the middle (its shore bent by a noise), a sixth of it at least; beyond the oval the
ground rises to its level over 300 m (`Lake::SILL`), the river leaving over the sill. `raw` fills
the basin: water at the lake's level, so the simulation floats and ditches on it as on a river
(`river > 0.5`). Airports, towns and roads are placed on the ground before the lakes (`dry`, the
seed's relief as it always was), and a lake keeps away from them and from the sea: none of them
moved, and the golden numbers of the world did not change.

**A real place** (seam 3 of `docs/ROADMAP.md`): `Terrain::from_region(seed, Arc<Region>)` answers
the same queries from a `Region` that `geodata` built from maps before the world was made (no
network, no clock here: the same region, the same world). `region()` says whether a terrain is
one. A `Region` is plain data in the world's axes: grids of heights (`Grid<f32>`, finest first,
each eased into the next coarser over its last 12 nodes; beyond the coarsest its edge), a grid of
what covers the ground (`Cover`: grass, farmland, scrub, forest, rock, sand, ice, wetland, built
up, inland water, sea, a railway's ground, or unknown), every runway as an `Airport`, the roads as straight stretches,
the towns as circles, the buildings of the maps as outlines with heights (`Footprint`: ring,
courtyards, height, bottom, `Roof` shape with its height and direction, floors, `Use`, wall
`Material`, the colours the maps give, whether its height was measured) and its bridges (`Bridge`:
the line of the deck's top with its heights, width, what it `Carries`, its `Structure`, where its
towers stand and how high, colour, stone towers, made by hand); and the latitude and longitude of
its origin, which `World::with_terrain` gives the planet under it (`Earth::latitude`). In a region:

- `raw` is the heights' (the sea below 0, the land above: the maps drew the coast); `river` is how
  much of the point is inland water (lakes, wide rivers), there are no valleys;
- `airports`, `nearest_airport_within`, `towns` and `roads` look through its lists (roads by squares
  of 1 km, indexed once); an airport reaches as far as its runway changes the ground
  (`Airport::reach`, more than `AIRPORT_REACH` for a long one);
- `buildings` gives those whose middle is in a rectangle (each in one tile of a tiling, by squares of
  256 m), `building_near` whether one stands within some metres of a point (of a side of its
  outline), `built_share` how much of the ground round a point (the 768 m round each square of
  256 m) its buildings cover and `urban` whether that is a city the maps draw (from 3 to 7 %: a
  city's blocks are 20 to 50 %, a rail yard or a junction among them 8 to 20 %, its suburbs 5 to
  15 %, a hamlet under 2 %), `tallest_building` the
  tallest building or bridge tower above the ground under it (0 in a generated world);
  `solid_at(n, e, h)` what building or bridge a point is inside of, `solid_under` the highest roof or
  deck under a point (what something falling there lands on), `solid_top` the highest over it
  (`solids`); none of the buildings stands on the ground a runway keeps clear
  (`Airport::clear_ground`: the runway and a margin round it, its apron; `Airport::blocks`): the
  aerodrome is levelled there, and what the maps draw under a runway on pillars (Madeira's) would
  stand on it;
- `surface` mixes the colour and the share of forest and farmland of each cover round the point (the
  rock of steep slopes, the snow high up and the sand at the water's edge as on generated ground);
  where the maps say nothing, the seed's ground, as on its land, but in a city the maps draw
  (`urban`): what they leave unsaid there is a rail yard, a school's grounds, the land inside a
  junction, a car park, not the seed's woods and fields (Manhattan, the Eixample and Madrid's
  rail yards were woods), so it has neither and is the colour of built-up ground.

`Airport::detail` (`Detail`) is what a runway is made of and has: paved or not, its lights
(`lights::Class`; `None` lets the id decide, as for every generated one), and the side its apron,
hangars, office, windsock and floodlights are on (or none, for the other runways of a real
aerodrome). `Detail::GENERATED` is the generated aerodromes', unchanged. `Road::width` is 7 m
(`Road::GENERATED_WIDTH`) between generated towns.

### aircraft
`Aircraft::new()` builds and rigs a C172; `Aircraft::of(kind)` one of any `AircraftKind` of the
fleet (`AircraftKind::C172`, `EXTRA300`, `parse("extra")`, `all()`), `Aircraft::with_type(p)` one of any
`AircraftType`. Everything that makes one type differ from another is a field of `AircraftType`
(`aircraft.p`): mass and inertia, the wing's strips and sections, which strips carry flap and
aileron, the flap notches and their lift and drag curves (none on the Extra), the tail, the engine's
torque and friction, the propeller's tables at its blade angle and, for a constant-speed one, its
`Governor` (`State::blade_pitch` is where the governor has the blades), the gear as a list of
`Wheel`s (contact point, spring, steering, brake, side grip, strength; a steering wheel behind the
centre of gravity is a tailwheel, `tailwheel()`), the structural limits, the floats, the solid other
aircraft hit, the pieces it breaks into, the load stations, where the world places it, the
`Speeds` the autopilot flies it at and the autopilot it has (`afcs`: the C172's GFC 700). Each type is a file, `assets/aircraft/<key>/aircraft.ron` (below, "Aircraft files"), with the
sources of each figure written next to it; `AircraftType::c172()` and `extra300()` are the fleet's two.
A new type is a new file and no code. `forces(&state, &controls, &env, downwash, flap_deg)`
is the whole force model and returns `FlightData`; `step(dt, &env)` integrates it (RK4) and decides
what breaks; `data()` is what the last step measured, `crashed()` why the flight is over (as text)
and `damage()` what has given way. For a replay, `look()` is what is seen of it now and changes from
step to step (state, controls, flaps, `data()`, the dials' readings, the gauges, damage, crash) as a
`Look`; `show(&look)` puts one over a copy of the aircraft, and `Look::blend` goes between two
(`Vehicle::show_agl` sets its height above the ground with it).
`trim(tas, altitude, flap_deg, throttle, climb_angle)` solves steady flight (`trim_level` for the
common case), `steady(..)` evaluates one condition, `stall(flap_deg, altitude)` gives CLmax and speed.
All tunable numbers are fields of `AircraftType`, given for its `reference_load` (the C172's is
2300 lb, centre of gravity at station 43.6 in; the Extra's, the pilot alone in the
rear seat with 37 kg of fuel, 791 kg). `set_load(Load { front, rear, baggage, fuel })` (kg) loads it
otherwise; `mass()`, `cg()`, `inertia()` and `load()` say what it then is. The engine burns the fuel
(`FlightData::fuel_flow`, kg/s) and stops when there is none.
`forces` assumes a wing that has not stalled lately; `step` remembers which strips have (they reattach
about 3° lower) and uses `forces_separated`. `forces_settled` is `forces` with the downwash of steady flight.

**What breaks** (`Damage`, limits in `AircraftType`). It gives way a part at a time, and what is
left flies, rolls or slides as the force model makes it: nothing is a scripted kick.
Load factor past the handbook's limit (C172: +3.8 / -1.52 g, +3.0 with flaps; Extra: ±10 g) bends
the wings (more drag, the more loaded side lifts less); 1.5 times it (the ultimate load) tears the
more loaded wing off at its root, where the bending moment is greatest against what it is built for.
A wing can also be torn across at any station (`Damage::wing_torn`, `Aircraft::tear_wing`), and each
half of the tailplane (`stabiliser_torn`, `tear_stabiliser`): what is outboard comes off as a piece
of its own (a `Wreck` whose `Piece` is a `Slice` of the wing's or tailplane's `Span`), the strips of the
wing outboard of the tear lift nothing and the loading falls to nothing at the new tip, so the roll,
the yaw and the lost lift come out of the strips; the mass, centre of gravity and inertia lose the
piece; a tear inboard of the end of the wing's tank (`AircraftType::wing_tanks`, the C172's) bursts it.
The control surfaces go one at a time (`SurfaceState`: working, `Loose` when its cable has parted
or its hinge is bent and it trails with the air, `Gone`): past Vne (163 kt; Extra 220 kt)
`FlightData::overspeed` warns, and at 1.2 times the design diving speed (217 kt; Extra 244 kt)
flutter takes an aileron, then the other, the elevator and the rudder; a surface torn off takes its
share of the chord or of the tail's area (`aileron_chord`, `elevator_share`...). Flaps out faster than
1.22 times their speed (110 kt at 10°, 85 kt beyond) are torn off, one and then, if the load stays,
the other. On the ground each part takes what touches it: each leg collapses at its own load
(`Wheel::strength`; the C172's are 18.5 kN for a main leg and 11 kN for the nose leg: a level touchdown at 3.6 m/s breaks
the mains) and the aircraft settles on what is left; a wing tip the ground pushes harder than
`wing_crush` (C172 20 kN, about a level tip dropping at 2 m/s) is crushed and torn off a strip at a
time, a lighter touch only scrapes it (`FlightData::touches`); the tail into the ground faster than
`TAIL_STRIKE` (3 m/s) buckles the cone and leaves the elevator and rudder loose; the fin crushed
upside down loses the rudder; the propeller striking stops the engine. Trees go by energy
(`world`): a point inside a crown is dragged back by its branches; a trunk (its radius at that
height, green wood breaking at 40 MPa) is felled if it gives way before the part that hits it, and
takes its energy from the aircraft; otherwise a wing or half tailplane is cut through where it hit
if the point brings the energy to crush it over its chord, and the aircraft goes on slower and swung;
what brings less stops against it. Buildings never give way. The energy a point brings, and the
impulse that takes it (`Aircraft::energy_at`, `absorb`), use the mass that point carries: a wing tip
swings the aircraft round. A crashed aircraft goes on moving (sliding, falling, floating). It breaks up
(`Damage::broken_up`) when its fuselage, propeller or fin goes into the ground faster than 9 m/s or
into water faster than that, or at more than 45 m/s, or into a trunk, building or aircraft that holds
faster than `COLLISION_BREAKUP` (15 m/s; slower, `collide` stops it against it). Its pieces (what is
left of each wing and of the tail, with the `Cut`s of the tears) go to the world as a
`Wreck` (`take_wrecks`). How hard the impact was is `Wreck::severity`, the speed times the root of
the share of its energy lost: the tanks start to burst at 12 m/s and all have at 30 m/s
(`TANKS_BURST`), and what bursts out burns only from `IGNITION_SPEED` (18 m/s) and never in water
(`fuel_released`); of that, up to half goes up at once in a fireball (`Wreck::fireball`, more the
harder the impact) and the rest burns where the wreck lies (`Fire`, 0.35 kg/s). A soft crash or a
ditching does not explode. The impact flings up to 24 fragments (`Fragment`: closed-form flights on
their own drag, from when they came off until they came down; most burn and trail smoke after a
fireball), digs a hole and scorches the ground the fireball reached (`Scar`), leaves burning fuel
along the slide, and records each thing that falls into water (`Splash`). All of it is decided by
the seed of the crash: the same crash, the same wreck; the same tear, the same piece. On water it floats on
`AircraftType::floats`, the wheels dig in, it fills and sinks in a minute or two. `clear_crash` repairs it all.

### Aircraft files
A type of aircraft is a file, `assets/aircraft/<key>/aircraft.ron` (RON: Rust's structs as text,
with `//` comments), read by `aircraft::file`. `load(text, name)` reads and checks one and gives the
`AircraftType` or a `FileError` that says the file and the line and column (not RON, a misspelt or
missing field) or the field and what is wrong with it (strip edges that do not increase, a curve
without a value for each flap notch, a final flap notch it has not, more than `MOST_WHEELS` wheels,
load limits of the wrong sign...). The C172's and the Extra's files are the examples; their
comments give the source of each figure.

Units are SI (m, kg, N, s, radians) unless the field's name says otherwise: `_deg` degrees, `_kt`
knots, `_rpm` turns a minute. Positions are body axes from the centre of gravity of the reference
load. The sections: `key` (the file's folder, lowercase), `name` (for the player), `aliases` (more
words for `AircraftKind::parse`), `visual` (the names of the model and the cockpit the game draws it
with: `c172` or `extra300`; the game refuses a name it has not), `mass` (mass, inertia, the reference
load and the stations of the load), `wing` (four strips a side: where they begin, how many carry flap
from the root and aileron from the tip; lift, stall, drag, twist, ailerons), `flaps` (the notches,
from 0, with a share of the full flap's lift and drag at each), `tail`, `fin`, `body`, `propeller`
(its tables against the advance ratio at `pitch_deg`, and a `governor` for a constant-speed one),
`engine` (with its `cylinders`, for the sound), `gear` (up to three wheels: the steering one behind
the main ones is a tailwheel), `hard_points` (named: wing tips, propeller, top of the cabin, top of
the fin, belly, tail skid), `structure` (load limits, Vne, the design diving speed whose 1.2 times
flutter starts at, flap speeds, what goes with each surface, the wing tanks, what crushes the wing),
`floats` (what keeps it afloat: nose, cabin, tail, each wing, a float per wheel), `airframe` (the
boxes another aircraft hits: wing, cabin, tail cone, stabiliser, fin), `pieces` (where it comes
apart: the spans of the wing and of the tailplane, and of each piece its centre, the points it
lands on, its drag and size), `start` (speeds of a start in the air and on final, and that flap
notch), `speeds` (what the autopilots fly it at; `least_kt` is the slowest the game's autopilot
holds), `instruments` (the `InstrumentFit`: static ports and their error by angle of attack and flaps,
gyros, compass, the marks of the airspeed indicator) and `autopilot` (`Some(Gfc700(..))` or `None`).

The fleet (`file::fleet()`, `AircraftKind::all()`) is the files built into the program
(`file::BUILT_IN`, embedded when it was compiled, so the tests and the web need no files) unless
the game gave others at start: `fleet_with(files)` puts each file in place of the built-in type of
its key, or after them, and `set_fleet` makes the result the fleet, once, before anything asks for
it. This crate never reads the disk; the game does (`crates/games/flight/src/fleet.rs`). A type read from
a file lives for the rest of the program (its tables are `&'static`): files are read once.

A new type needs no code unless it brings physics the model does not have: `tests/fleet.rs` flies
a made-up one from `tests/fixtures/trainer.ron` (180 hp, a constant-speed propeller, three flap
notches) round a circuit. What the model has not, and so what a type cannot be given by a file: more
than four strips a side, more than five legs, more than one piston engine, a canard or a biplane's
two wings.

**Jet transports.** The Boeing 747 (`wip/b747/aircraft.ron`, not built in nor in the game yet;
its figures against the published ones in `tests/b747.rs`, printed by `examples/b747.rs`) brings
what a light aircraft has not, each a section or a field a file may leave out (then the model is
the light aircraft's, number for number):
- `jets` in place of `propeller` and `engine`: up to four turbofans (`aircraft::jet`, `Jets`), each
  where it hangs (`engines`: where its thrust acts and the bottom of its nacelle, which may touch
  the ground). An engine is its fan's speed, `State::n1` (1 at the take-off rating); the throttle
  asks for one between idle (higher in the air) and what the ratings allow in that air, and the
  engine spools to it slowly from idle and briskly near the top. Thrust is the momentum it gives
  the air it takes in (`Jets::run`): it falls along the runway and with height, idle drags at
  speed, a dead engine windmills and drags (`Aircraft::fail_engine`, `Damage::engines_failed`;
  `FlightData::jet_thrust` each one's). Reversers (`AircraftControls::reverse`, the throttle then
  sets how much) open only on the ground, the engines at idle while they move (`Aircraft::reversers`).
- `mach`: with Mach (`FlightData::mach`, from `Environment::temperature`) the lift curve steepens
  as a swept wing's of its aspect ratio does, wave drag grows from the critical Mach number (which
  lift lowers), the lift moves aft (Mach tuck) and the wing stalls at less lift (the buffet
  boundary); past `mmo`, `FlightData::overspeed`.
- `wing.sweep_deg`: the outer strips sit aft, and the wing that leads into a sideslip lifts more.
  `wing.aileron_clean`: the share of the roll power there is with the flaps up (outer ailerons locked).
- `spoilers`: panels on some strips that dump lift and drag; the lever (`AircraftControls::spoilers`)
  raises them all, to the flight detent in the air, and the ailerons those of the wing going down.
- `gear.retract`: the gear goes up and comes down in its time (`AircraftControls::gear_up`,
  `Aircraft::gear`), never up with weight on the wheels; down, it drags; up, the belly and the
  nacelles land ("Landed with the gear up"). `gear.braking` scales the brakes' grip. Up to five
  legs (`MOST_WHEELS`): a bogie is one leg.
- `start.takeoff_flap_notch` and `start.runway`: the flaps it takes off with, and the runway it
  needs, m: the world starts it (`Start::Home`) at the nearest aerodrome that has one
  (`Terrain::nearest_runway`). The generated world has an international airport, a runway of
  3300 m by 60 (`Terrain::LONG_RUNWAY`), in one cell of its aerodromes' grid in seven where the
  ground is flat enough, none within three cells of the origin.
- `tail.stabiliser_trim_deg`: the trim turns the whole tailplane (`Trim::trim` carries the load).
  `fin.yaw_damper`. `flaps.rate_deg_s` and `flaps.speeds_kt` (a placard speed for each notch).
  `structure.hard_spring`: how stiff the airframe is against the ground.

### world and vehicle
```rust
let mut world = World::new(seed);               // or World::with_terrain(seed, terrain), a real place's
let id = world.add_aircraft(AircraftKind::C172, Start::Home);   // or Runway{..}, Air{..}, Final{..}
let x = world.add_aircraft(AircraftKind::EXTRA300, Start::Home);
let q = world.add_multirotor(MultirotorKind::FPV5, Start::Air { n, e, heading });   // hovering, armed
world.vehicle_mut(id).aircraft_mut().unwrap().controls.throttle = 1.0;   // throttle, flaps, trim: set directly
world.fly(id, &mut pilot);                       // observe → pilot.fly → controls
world.step();                                    // every vehicle, 1/240 s
```
`World::vehicles` are `Vehicle`s (`vehicle`), each found by its `VehicleId`. A vehicle is a rigid
body moved by its `Dynamics`, an enum with a variant per kind of vehicle: `Aircraft`,
`Multirotor` and `Car` (`docs/ROADMAP.md`, seam 4). Plain enums, no trait objects: what
differs per kind is a `match` on `Dynamics` (and on `Controls`, `VehicleObservation`), each kind an
arm; the aircraft are much bigger than a multirotor and are kept by value all the same (the
vehicles are stepped in place). `Vehicle::aircraft()` and `aircraft_mut()` give the aircraft, if
the vehicle is one, `multirotor()` and `multirotor_mut()` the multirotor: `Option`s, so what only
an aircraft has says what it does with the others. A start puts each kind where it belongs: on a
runway an aircraft on its wheels and a multirotor on its feet, disarmed; in the air an aircraft in
trimmed flight 900 m up and a multirotor hovering `air_height` up, armed; on final a multirotor hovers there. `Vehicle::reset_on_ground(n, e, heading)` stands
either on the ground anywhere. What every vehicle shares is the vehicle's: its `WindField`, `home`, `agl()`,
`environment()` (the ground plane under it and the air), the ground ahead, the trees and buildings
near it; its rigid body is `body()` (`body::Body`: position, attitude, velocity and rates), and
`body::Rates` is the fourth-order Runge-Kutta every dynamics integrates with. `Dynamics` answers what
the world asks of any vehicle: its body and centre of gravity, airspeed, whether it is on the ground,
crashed or gone (in pieces or sunk), its controls, its solid (what another vehicle hits), the points
of it that run into things, a collision, energy absorbed at a point, the wrecks that came off it.
What is an aircraft's is in `Aircraft` (`aircraft::in_world`): its `instruments` (below), the air
data an ideal sensor gives with a lag (`indicated_airspeed()`, `indicated_altitude()`,
`vertical_speed()`, `turn_rate()`, `slip_ball()`: what the pilots and the trained models fly by),
`last_touchdown()`, how it is placed, what striking a tree or a building does to each part of it.

`World::reset(id, start)` places a vehicle again (`Vehicle::reset_on_runway`, `reset_in_air`,
`reset_on_final` do the work); `World::place(id, state)` puts it in a state set by
hand (another height, attitude, speed) with its instruments settled there as at any placement:
writing `aircraft_mut().state` directly leaves the pitot-static lines, the vertical speed's capsule and the
lagged air data chasing the jump (a drop of 500 m read as a dive of 20 000 ft/min). On the ground with the throttle closed the wheels
brake by themselves (0.6); `AircraftControls::brake_left` and `brake_right` are the pilot's toe brakes on top.
`World::weather.air` is the air of the day (the standard day unless changed): the aircraft fly in its
density (humid air is lighter), the engine burns its dry air only (`Environment::vapour` is the share
of the density that is water: a hot, humid day takes power as a high aerodrome does) and the
instruments sense its pressure and temperature. `Weather::visibility` (m, 150 km unless changed: a clear day) is the
day's meteorological visibility: what the towers set their lights by (`lights::setting`) and what
`world-render` draws the haze and fog with. `World::earth` is the planet under
the flat world (latitude 40° N, the magnetic field's dip of 57° and no variation, the runways being
numbered by true heading).
A vehicle low enough runs into the trees and buildings of `scenery` (asked for by tile and kept while
it is low), and any vehicle into `World::obstacles`, what has been placed in the world besides its
scenery (a race course's gates, `course::Track::obstacles`; empty unless given); vehicles that touch break up, both. Vehicles do not disturb each other's air.
`World::wrecks` keeps what is left of the vehicles that came apart (the last `MOST_WRECKS`), steps
their pieces until they rest and their fire until it is out, and leaves them there after a reset.
`World::observe(id) -> Observation`: plain numbers and strings, to be sent over a socket: what any
vehicle's pilot sees (attitude, rates, velocity, position, `agl`, `on_ground`, `nearest_airport`,
`traffic`, `crashed` as text) and, in `vehicle`, what its kind's sees besides
(`VehicleObservation::Aircraft(AircraftObservation)`: the air data above, what every instrument reads,
angle of attack and sideslip, engine, flaps, trim, the stall warning; `Observation::aircraft()`;
`VehicleObservation::Multirotor(MultirotorObservation)`: armed and why not, the
sticks, each rotor's speed, the battery's voltage, current and charge used, the barometer's height
and climb, the airspeed; `Observation::multirotor()`).
`ground_ahead: [f64; 4]` is the highest ground (m above sea level) in four stretches of the straight
line the vehicle is moving along over the ground, ending 500 m, 1, 2 and 4 km ahead (`GROUND_AHEAD`);
it is sampled every 250 m and refreshed four times a second. It is what lets a pilot keep clear of
rising ground without touching the terrain.

### multirotor
`Multirotor::of(kind)` builds one of a `MultirotorKind` of the fleet (`MultirotorKind::FPV5`,
`parse("drone")`, `all()`), `Multirotor::with_type(p)` one of any `MultirotorType` (`multirotor::file`
reads them, as `aircraft::file` the aircraft: below, "Multirotor files"). Its `body` (`body::Body`,
placed by its centre of gravity), its `controls` (`MultirotorControls`: the radio's sticks, throttle
0 to 1, roll, pitch (back is nose up) and yaw −1 to 1, and the arm switch), and
each step of `World::DT`:

- **The flight controller** (`multirotor::controller`, from the published descriptions of the FPV
  firmwares, no code of theirs) turns the sticks into the motors' duties. It arms on the switch only
  with the throttle down, within 25° of level and not crashed, and says why not (`arming_blocked`:
  THROTTLE, ANGLE, CRASH). It has no flight modes: it always levels itself. The roll and pitch
  sticks ask for a bank and a pitch up to `angle_max` (55°), levelled at `angle_gain`, and it
  levels when they are let go; the yaw stick a rate of turn about the vertical (banked, the heading
  turns flat and the bank is kept, as the firmwares' angle mode does), `StickRates::rate`, the
  "Betaflight" curve (RC rate, super rate, expo; the FPV5's 364°/s at full stick, 95°/s at half);
  the throttle is plain power (more climbs, less sinks).
  A rate loop per axis (P, I that is held while the stick moves fast and at rest on the ground,
  D on the gyro, feed-forward) asks for moments, and the mixer (`MultirotorType::mixer`, the
  pseudo-inverse of what each motor's thrust does to the collective, roll, pitch and yaw) splits them
  between the motors, giving up yaw first, then roll and pitch together, and shifting the whole
  between idle and full: control at zero throttle and at full ("air mode"). The throttle stick sets
  the duty (idle 5.5 % to full): as the battery runs down the same stick lifts less.
- **Motors and battery**: each motor a DC machine (kv, winding resistance, no-load current) at its
  duty off a battery whose voltage is its cells' (by the charge used) less what its resistance drops
  under the current drawn; its rotor speeds up under the motor's torque against the propeller's
  `torque_coefficient · ω²`, in four sub-steps; the ESC limits the current and brakes too.
- **Forces**: thrust `thrust_coefficient · ρ/ρ₀ · ω²` along −z at each motor, falling with the air
  coming through the disc to nothing at 1.1 times the pitch speed (more in a descent, up to 1.25),
  more near the ground (`1 / (1 − (R/4h)²)`, at most 1.35); in its own wash (the vortex ring
  state, "propwash": `wash`, `WashFit`) each rotor's depth grows with its sink over its induced
  velocity now (`√(T / 2ρA)`: idle pushes the air down slowly, so falling with the throttle cut
  it is deep in it, throttle punches out) and shrinks as the wake trails behind with speed across;
  deep in it a rotor loses `thrust_loss` and its thrust comes and goes by `fluctuation` of its own
  and `buffet` of `½ρAV²`, a seeded noise per rotor over `correlation_time` (deterministic). The
  FPV5 falling at zero throttle wobbles 15 to 20°, 200 to 300°/s; at 20 % throttle some 10°;
  moving forward at 20 m/s or hovering, not at all; each disc's edgewise drag; the rotors' drag torque
  and spin-up on the frame (counter-clockwise rotors yaw it right) and their gyroscopic moment; the
  frame's drag (`drag_area` per axis) and damping, in the vehicle's wind; gravity. The body is
  integrated with `body::Rates`' Runge-Kutta, the rotors held over the step.
- **Contacts**: its points (`contacts`: feet, bells, propeller tips, camera, battery) are springs
  and dampers with friction against the ground plane under it; any point that goes into the ground
  faster than `crash_speed` (10 m/s) crashes it; striking an aircraft that fast too; in water it
  sinks (`sunk`, gone). Crashed, the controller disarms and it tumbles and rests whole, its own
  wreck: it leaves no `Wreck`. The trees' crowns drag it as they drag an aircraft.
- **Against fixed things** (`Multirotor::hit`, from `Vehicle`: the trunks and buildings near it,
  `World::obstacles`' gate tubes), as against another quad (below) but for a thing that does not
  move (`scenery::Obstacle::surface`: the nearest point of a trunk, tapering to a fifth at its top,
  of a tube, of a building's block, and the way out). A turning disc that meets one is cut by how
  deep it reaches into it and how fast they meet (chipped on a graze slower than `graze_speed`,
  broken otherwise; down on what is left: "Broke a propeller on a tree", "... on a gate"); turning
  through a crown it meets its twigs, 1.5 a metre a metre inside it (fewer towards its fringe,
  none at its outline), each a 4 mm cut (chipped at a walk, broken faster), drawn from a hash of
  where the disc is. The frame's balls in a solid are moved out by
  the deepest overlap and knocked out along the way out at `restitution` of the speed they came in
  at, turned by where they are struck; harder than `crash_speed`, "Crashed into a tree" (a gate, a
  building...). Nothing holds it while it moves away: it never sticks (it used to be put back where
  it was a step before, and slow it could stay glued to a gate). `tests/quad_strikes.rs`.
- **Against another multirotor** (`collide_multirotors`, from `World::collide_vehicles`): each is
  its frame as balls (`parts`) and its rotors' discs (`thickness` thick, as far as the longer blade
  reaches; turning faster than 100 rad/s, else not cutting). A disc that meets the other's frame or
  one of its discs is cut by how deep the other reached into it (`PropStrike`): meeting slower than
  `graze_speed` (1.5 m/s: under 2 mm between two blades' passes) the first blade loses its tip that
  deep and the other half as much; faster, or once a blade has lost more than `chip_depth` (8 mm),
  both shatter to `stub` (a quarter of the radius). A propeller's thrust and drag torque go as its
  blades' radii cubed and to the fourth (a chip of 3 mm costs some 8 %; broken, 98 %), its rotor
  inertia with them (the bell's quarter stays); uneven blades make its thrust come and go by `shake`
  times the difference of their swept areas, on its wash's noise (a chipped quad shakes, some 0.2 rad/s
  in the hover). With a propeller broken the flight controller cannot hold it on the others: it
  tumbles and comes down, and on the ground it is crashed ("Broke a propeller against another quad", or
  a hard landing).
  Frames that touch and are closing get one knock: at every pair of balls that touch, the impulse of
  two rigid bodies with `restitution` (0.3), their momentum kept, each turned by where it is struck,
  worked out together over all the points (a few rounds, each impulse pushing only); they are moved
  out of each other by the deepest overlap. Nothing slows them while they part, so they never stick
  (they used to: slowed to 0.4 of their speed every step they touched). Faster than `crash_speed`
  both crash. Aircraft against aircraft or a multirotor are as before.

`data()` is what the last step measured (`MultirotorData`: airspeed, on the ground, the
accelerometer, the thrust, the wash), `battery()`, `rotor_speeds()`, `blades()` (how far each
propeller's two blades reach, a share of its radius) and `propeller_broken()`, `armed()`,
`armed_time()` and `armed_altitude()` (what its display shows), `crashed()`, `observe()`;
`strike_propeller(i, depth, speed)` does to a propeller what a strike does; `look()` and `show()`
for a replay (`MultirotorLook`, `blend`; the blades too). `MultirotorType` answers what its controller and the game ask of
the model: `steady_speed`, `steady_thrust` and `duty_for` (the motors in still air), `hover_throttle`
and `thrust_to_weight` (the FPV5's: 37 % and 4.4; the RACE5's: 15 % and 13.6).

The fleet:

| Key | What | Mass | Thrust to weight | Top speed (self-levelled) | Camera |
|---|---|---|---|---|---|
| `fpv5` | 5-inch freestyle quad, 2306 2400 kV on 4S 1300 mAh, 5x3 two-blade props | 650 g | 4.4:1 | some 30 m/s (55°) | up 25°, 120° |
| `race5` | 5-inch racing quad, 2207 1950 kV on 6S 1300 mAh, 5.1x4.3 three-blade props | 500 g | 13.6:1 | 49 m/s, 175 km/h (75°) | up 40°, 125° |

| `kestrel` | An invented tilt-rotor gunship: two tilting rings with two 4 m rotors each, two turboshafts of 1150 kW, stub wings, a tailplane and a fin | 4.8 t | 1.57:1 | 83 m/s, the nacelles forward | the pilot's eyes, 100° |

`race5`'s file gives its sources (T-Motor's F60 Pro V and IV 1950 kV sheets, thrust stands' tables for
5.1x4.3 three-blade props, Tattu's R-Line 6S 1300 mAh packs); `tests/race_quad.rs` checks it: 13.6:1,
hovering at 15 %, climbing at 49 m/s, 175 km/h flat out, 447°/s of yaw, banking to its 75° without
swinging past.

### Multirotor files
A type of multirotor is a file, `assets/aircraft/<key>/multirotor.ron`, in the aircraft's folder (its
look, `exterior.glb` and `.ron`, is found as an aircraft's), read by `multirotor::file` as
`aircraft::file` reads the aircraft: `load`, `fleet`, `fleet_with`, `set_fleet`, the built-in ones
(`BUILT_IN`) unless the game gives others. The FPV5's is the example; its comments give the
sources. Units as in the aircraft's files (`_deg`, `_rpm`, `_mah` say otherwise). The sections:
`key`, `name`, `aliases`; `visual` (`model`: a folder of `assets/aircraft`; `camera`: its position,
`uptilt_deg`, `fov_deg` across a 16:9 picture; `cockpit`, if it has one: a folder as an aircraft's); `mass` (mass, inertia); `frame` (`drag_area` per
axis, `rotational_damping`, `contacts`, `contact_spring` and `contact_damping` per point, `friction`,
`crash_speed`, the `solid` other vehicles hit, the `parts` another multirotor strikes, balls of a
centre and radius, and the `restitution` of a knock against them or a fixed thing); `motors` (3 to 8: position of each hub and its
`spin`, `Clockwise` or `CounterClockwise` seen from above, in the flight controller's order);
`motor` (`kv_rpm`, `resistance`, `no_load_current`, `rotor_inertia`, `max_current`, `idle`);
`propeller` (`diameter`, `pitch`, `thrust_coefficient`, `torque_coefficient`, `edgewise_drag`,
`strike`: `thickness`, `graze_speed`, `chip_depth`, `stub`, `shake`, as `PropStrike`);
`wash` (`onset`, `full`, `clear_from`, `clear_to`, `thrust_loss`, `fluctuation`, `buffet`,
`correlation_time`: `WashFit`);
`battery` (`cells`, `capacity_mah`, `internal_resistance`, `cell_voltage` against the share used);
`controller` (`rates`, `p`, `i`, `d`, `feed_forward` per axis, `i_limit`, `angle_max_deg`,
`angle_gain`); `start` (`air_height`). A broken file is told as an aircraft's.

A tilt-rotor has one more section, `tilt` (`Tilt`; the Kestrel's file is the example): the `pivot`
the nacelles turn about (a line across the frame through it), how far forward the lever takes them
(`forward_deg`) and how far either side of that the flight controller may (`margin_deg`), the
actuators' `rate_deg` and the conversion's (`conversion_rate_deg`: how fast the lever is followed),
`stick_tilt_deg` and `back_deg` (how far the pitch stick tilts both nacelles from where the lever
has them, and how far behind upright: with them the frame is flown level), `split_authority_deg`
(one nacelle against the other), the rotors' cyclic (`cyclic_arm`: how far
it moves each one's thrust off its hub), what full sticks ask for with no help (`direct`, rad/s²),
the speeds between which it starts to turn with its bank
(`turn_from`, `turn_full`) and its `surfaces`: each a `position`, `area`, `aspect`, `upright` (a
fin), `incidence_deg`, `stall_deg`, `drag`, and the tailplane's `elevator` (`effect`, `travel_deg`).
With it, the motors left of the centre line are on the left nacelle and those right of it on the
right one.

In place of `motor` and `battery` a file may have `turbine` (`Turbine`; it needs `tilt`): how many
`engines` and each one's `power_kw`, the `rotor_rpm` their governor holds, the blades' `pitch` at
the bottom and the top of the collective lever, `inflow_length`, `induced_factor`,
`profile_power_kw` per rotor, the shaft's `inertia`, `spool_time`, `start_time`, `fuel` (kg),
`fuel_per_kwh` and `idle_flow_per_hour` per engine. The controller's `stability` (`Level`, the
default, `Rates` or `Off`) is the help it starts with.

### Tilt-rotors
A multirotor with a `tilt` section (`multirotor::Tilt`) is a tilt-rotor: the same motors, battery,
contacts and strikes, and

- **Nacelles**: each turns about the pivot's line, from rotors up (0) to `forward` and the margin,
  at its actuator's rate; a rotor's hub, thrust, drag torque, gyroscopic moment and disc (for what
  it cuts) are where its nacelle has them. `MultirotorControls::tilt` is the pilot's lever, 0 to 1.
- **Surfaces** (`multirotor::Surface::force`): wings, a tailplane and a fin, each by its own angle
  to the air it moves through: a finite wing's lift slope and induced drag up to its stall, a flat
  plate past it (`C_L = sin 2α`, `C_D = 2 sin²α`), so they hold at any angle. The elevator adds to
  the tailplane's angle.
- **Cyclic**: each rotor's cyclic pitch moves its thrust up to `cyclic_arm` off its hub, a moment
  about the line across the frame: pitch without speed.
- **The flight controller** (`controller::tilted`) arms, levels and runs its rate loop as a quad's,
  and splits the moments another way: roll and yaw between the difference of the nacelles' thrust
  and the difference of their tilt (which one does which turns with the nacelles: a rotation by
  their angle, undone exactly), pitch between the cyclic and the elevator by what each can give
  now. Above `turn_from` it adds the rate of turn of a balanced turn at its bank (`g tan φ / V`).
  The lever is followed at the conversion's rate.

- **Flown level** (`Tilt::stick_tilt`): the pitch stick tilts both nacelles forward or back of
  where the lever has them, at once, and the frame holds the pitch the pilot trims
  (`MultirotorControls::trim`, a share of `angle_max`): it goes forward and stops by its rings,
  its nose where it was. With no help the stick pitches the frame.
- **Hold** (`Stability::Hold`, `Tilt::hold`: `HoldFit`; the file's `tilt.hold`: `climb` up and
  down, `dead_band`, `speed` forward and sideways, `climb_gain`, `speed_gain`, `bank_deg`): it
  holds itself still. The collective lever asks for a rate of climb, none in the dead band round
  its middle (the thrust that carries its weight and goes after the climb asked for, over how
  much of it points up; at the lever's foot the blades are flat). With the nacelles' lever
  upright the stick asks for a speed over the ground, forward by the nacelles' tilt and sideways
  by its bank, each with an integral that holds the wind off; with the lever forward the stick is
  `Level`'s. The Kestrel starts so: hands off in 5 m/s of gusting wind it moves under a metre
  in half a minute.
- **Stability** (`MultirotorControls::stability`, `multirotor::Stability`): how much it helps, the
  pilot's to choose. `Level`: the sticks ask for a bank and a pitch and it levels itself. `Rates`:
  they ask for rates of turn, and let go it stays as it is. `Off`: no loop, the sticks ask for
  accelerations (`direct`) and only the air damps it (the rotors' own damping: the side going
  down pushes more).
- **Turbines** (`multirotor::Turbine`, `Engines`): every rotor on one shaft. A rotor's thrust is
  `thrust_coefficient · ρ/ρ₀ · ω²` times its blades' pitch less the air through its disc (the
  climb along its axis and its induced velocity, which follows the thrust over a tenth of a
  second: dynamic inflow) over `ω · inflow_length`. Turning it takes the induced power of its
  thrust (`induced_factor` times thrust times Glauert's induced velocity, in the hover, climbing
  or moving across), the climb's, and `profile_power` (by the density and the cube of its speed).
  The engines' governor asks their gas generators for what the rotors took, more or less by the
  rotors' speed and its integral; they follow over `spool_time` and give that share of their
  power (less with height, the density's 0.8th power) through a freewheel. More pitch than they
  can turn slows the rotors; stopped (the arm switch, or no fuel) they give nothing, the flight
  controller goes on flying the rotors, and the air coming up through the discs turns them
  (autorotation). Started, they light over `start_time` and run the rotors up over three times
  that from rest. Fuel goes by the power given and by each engine just running. The throttle
  stick is the collective lever: the blades' pitch, from `pitch.0` to `pitch.1`.

The Kestrel (`tests/tiltrotor.rs`) hovers on 1.28 MW, 56 % of its engines' torque, the collective
at 52 %, burning 506 kg/h; turns on the spot at 60°/s; with the nacelles 60° forward does 83 m/s
on 2.0 MW; and with all the collective at once in the hover its rotors fall to 84 % while it
climbs at 15 m/s. It has no wing to fly on: the rotors carry it at any speed, and fast the height
is held with the stick as much as with the collective. Engines out, it comes down at some 25 m/s
with its rotors near 90 %.

### course
A race course is a file, `assets/courses/<key>.ron` (`course::file`, read as the aircraft's files are:
`load`, `courses`, `courses_with`, `set_courses`, the built-in ones unless the game gives others): its
`name` and `description`, its `place` (`Seed(n)`, the generated world of a seed, or `Airport("LEMH")`),
its `frame` (`Runway(near: (n, e), reversed)`: from the middle of the runway of the aerodrome nearest
that point, x along it, y to its right, headings from x; or `World`), where the racers `start` (on the
ground) and which way they face, how many `laps`, and its `gates` in order, each a `kind`, `x`, `y`,
`heading` (the way through it), a `scale` and, for a flag, the `side` the racer keeps it on. The
first gate is the start and finish. Optional: the `quad` it is raced in (a multirotor's key), its
racer's pace (`racer`: `Pace`), and a `trail` cut through the forest (`Trail`, `file.rs` says each
field): no trunk within half its `width` of its line, no crown lower than its `clearance` over it
(taller trees keep their crowns over it, the canopy; a point's own clearance opens a clearing to the
sky), and the wood `dense` times as thick within `band` metres of it. `scenery::trees` grows the
world's trees so (`course::cuts`, `cut_down`, `thicket`): drawn and run into alike; `world-render`'s
grass is worn on the trail and gone round the start (`course::starts`). The kinds of gate are `assets/courses/gates.ron` (`GateShape`, in
the gate's axes: x the way through, y right, z up from its foot): what counts as flown through it
(`Opening`: an upright rectangle or circle, a level rectangle flown down through, or the strip beside
a pole) and its frame as tubes (`Part`: rectangles, rings, level rectangles and bars of the kind's
`tube` radius; `GateShape::bars`). `Arch` (8 by 4.8 m inside, the start and finish), `Square` (3 m,
a metre up), `Round` (a hoop 3.2 m across), `Ladder` (two squares stacked, through the upper one,
4.4 to 7.4 m up), `Flag` (round it on its side, within 25 m of the pole), `Dive` (4 m square, 7 m up,
flown down through), `Log` (a log across two stumps, 3 m up: under it, 4.1 m wide, 0.45 to 2.75 m
up), `High` (a hoop 3.2 m across, its middle 7.4 m up on a pole), `Hanging` (a hoop hung from a beam
between two posts, its middle 3.4 m up). The game's models of them are made from the same tubes.

- `Track::new(course, terrain)` places a course: each gate's foot on the ground (`PlacedGate`: axes,
  `to_world`, `local`, `target`: the middle of the opening and the way through), the start's spot;
  `obstacles()` is every tube as a `scenery::Shape::Bar` of `ObstacleKind::Gate`, for
  `World::obstacles`: a quad's propellers break on a frame, its frame bounces off it, faster than
  its `crash_speed` it crashes ("Crashed into a gate"); an aircraft strikes it as a parked aircraft.
- `Race` follows a racer from the positions it goes through (`update(track, pos, time)` after each
  step, or each decision: a move is a straight segment): the next gate (`PlacedGate::crossed`: through
  the opening the right way, or past it outside the opening within `MISS_NEAR`, 12 m, which is a
  miss, as is flying through another gate instead), the laps (lap 1 from the start signal, `go`, to
  the first time gate 0 is flown through after every other gate; each next lap from there), each lap's
  time, each gate's split into the lap and the last lap's (`splits`, `last_splits`), the finish
  (`Event::Finished`), and `progress`: gates flown and the share of the way to the next.
- `racer::Racer` is a `Pilot` that races a multirotor round a track with the self-levelling sticks a
  player has, from the observation and its own `Race`: it wants a velocity along the line through the
  next gate's opening, coming onto that line before it (its lateral speed as its offset over the way
  left), as fast as it can still turn at the gate (by the angle the course turns there and the room to
  the next gate, slowing at its `Pace::braking`), over a dive gate first and down through it, round a
  flag a few metres off its pole; the acceleration that brings the velocity, the tilt and the thrust
  for it (the motors' model gives the throttle for the thrust off the battery, taking the air through
  the discs at speed into account; an integral of the climb's error the rest), the nose where it goes.
  Faster than 8 m/s it gives way to the other vehicles of its observation (`traffic`): from one that
  would pass within `GIVE_WAY` (1.5 m) of it in the next second it moves aside, up to 4 m/s, level
  and across its way (slow, round a gate, it keeps its line: aside is the gate's frame). Alone (as in
  `sim-env`'s `gates`) it flies as it always has. `go` at the start signal, `take_over` a race already going.
  Its `Pace` is the course's (`top`, `straight`, `hairpin`, `braking`, `tilt`, the `floor` it keeps
  over the ground, `yaw_rate`); with `follow_ground` (a forest's trail, under the canopy) it holds the
  height over the ground of the last gate's opening going over to the next one's, climbing with the
  ground's rise under it (worked out from its height over the ground against its own climb).

`tests/course.rs`: the files read, a gate counts only through its opening the right way, a frame is
solid (a quad at 15 m/s into a post crashes), and the racer flies the built-in course, Pinewood
Airfield (the generated world of seed 1, the airfield in a clearing of the forest 9 km south of the
home one: the hangars' lane, a slalom of flags, a dive gate, a gauntlet of six hoops through the gaps of
the forest's edge, a hoop and a ladder on the straight back, a hairpin round a flag), every gate of
both laps and none missed: 89 s from the start's pad, 86 s a flying lap (`cargo test -p sim-core
--test course -- --nocapture` prints the gates; `RACE_PRINT=0.1` its path every tenth of a second);
and Deepwood Forest in its race quad (the forest 6 km west of the home aerodrome: a trail 9 m wide
up a slope through the trees, logs and hoops, a high hoop in a clearing, a split-S, the run home):
every gate, both laps, 130 s; its trail clear of trunks, low crowns and the gates' frames, and the
wood thick round it. `examples/course_check.rs` flies a course's racer and says its laps, speeds,
battery, the closest trunk, the most tilt (`TRACE=1`: its path, the knocks), and writes its line,
the trail and the gates for a map.

### car
`Car::of(kind)` builds one of a `CarKind` of the fleet (`CarKind::GT`, `parse("coupe")`), `Car::with_type(p)`
one of any `CarType` (`car::file` reads them: "Car files", below). `World::add_car(kind, start)` adds one
(standing on its wheels where the start puts it: `Start::Air` and `Start::Final` stand it on the ground
there), `World::stand(id, n, e, heading)` puts it somewhere else at rest. Its `body` (`Body`, placed by
its centre of gravity), its `controls` (`CarControls`: `steer` −1 to 1, `throttle`, `brake`, `clutch`
(the pedal; up, the clutch works by itself), `handbrake`, `shift` (+1 or −1, taken once), `automatic`,
`abs`, `traction_control`, `stability_control`: on by default, as in the road car), and each step of `World::DT`, cut in `SUBSTEPS` (960 Hz):

- **Suspension** (per wheel, along the body's z): the spring (its wheel rate, preloaded to carry the
  wheel's share of the weight at rest), the damper (bump and rebound), the anti-roll bar (the
  difference with the other side), the bump stop and the travel's ends, and the tyre's radial stiffness
  in series, against the ground under the hub (`CarGround::patch`). The wheel's own mass is left out:
  the hub's travel follows from those forces, solved implicitly (stable at any stiffness); the tyre's
  load is what its spring is squashed by. Camber changes with the travel and the roll, toe with the
  travel (bump steer); Ackermann steering, a share of true.
- **Tyres** (`tyre::TyreFit::forces`): Pacejka's Magic Formula along and across, the cornering
  stiffness and the friction varying with the load, combined slip by the weighting functions, camber
  thrust, rolling resistance by the surface and the speed, the surface's grip (`Surface::grip`: dry
  asphalt 1, kerb 0.9, gravel 0.55, grass 0.5). The slips build up over the relaxation lengths as the
  tyre rolls (Pacejka 2012, ch. 7) and are damped at walking pace, so a car comes to rest and parks.
  The tyre's forces reach the body at the contact, where the road makes them, so the load they move
  from side to side and from end to end is what statics say (the weight times the acceleration, times
  the centre of gravity's height over the track or the wheelbase); the suspension's links carry the
  share of it that the axle's roll centre gives (the side force times the roll centre's height over
  half the track, pushing the hub down and the body up: Milliken, ch. 17), the springs and bars the
  rest. The formulas and their sources are in `car/tyre.rs`; every number is the car's file's.
- **Drivetrain**: the engine (its torque at full throttle against the revs, its drag closed, an idle
  governor, the rev limiter) through a friction clutch, the gearbox and a limited-slip differential
  (preload and ramps) to the driven wheels, and the brakes on all four (the bias, the hand brake on the
  rear): friction couplings solved together by sequential impulses each sub-step. The clutch is the
  pedal's, or takes up the drive from rest by itself (the revs held up while it slips), lets go before
  the engine stalls and opens while a gear changes (`shift_time`, then taken up over a quarter
  second): the fuel cut going up; going down the gearbox opens the throttle to bring the engine to
  the revs of the gear to come, so the clutch does not brake the driven wheels, and a shift down that
  would turn the engine past its limiter is refused. The automatic shifts by the revs and the pedals
  (down under braking); the throttle that counts for shifting up is the most of the last three
  seconds, so the gear is held through a lift for a corner.
  Anti-lock brakes hold each wheel's slip at `abs_slip` (lower on a slippery road), traction control
  cuts the engine's torque while the driven wheels slip past 0.11 (0.05 when the rear tyres slip
  across: grip is kept for the corner), stability control (ESC) compares the yaw rate with what the
  steering asks for (a gentle understeer, no more than the grip allows, lagged as the car's own yaw
  is) past a band of 0.12 rad/s, and the body's sideslip past 4°, and brakes one wheel to turn the car
  back (the outer front against oversteer, the inner rear against understeer), cutting the engine only
  while the body slides more than 2° (a yaw that is merely late, as after a quick correction of the
  wheel or a tap of a key, is left alone). All three on by default, each
  switchable off.
- **Air**: drag through the wind, lift (downforce) at each axle; less of both in the wake of a car
  ahead (`slipstream(gap, offset)`: up to 38 % of the drag right behind it, 20 % two lengths back, a
  car's width aside little; Romberg et al., SAE 710213; `World` sets each car's `draft` every step).
- **Contacts**: the body's points (`BodyFit::contacts`) against the ground (a car on its roof slides on
  it) and against the walls near it (`CarGround::walls`, a circuit's barriers): springs and dampers
  with friction, stiffer for concrete, softer for a tyre wall; struck square faster than
  `crash_speed` (20 m/s; 1.4 times that into tyres) it is wrecked, slower it bounces off and scrapes
  along. Trees, buildings and `World::obstacles` as for a multirotor; another car knocks it with the
  impulse of two bodies (`collide_cars`, restitution 0.25), wrecking both only if hard. In water, or on
  its roof for 3 s, it is out (`crashed`).

The body is integrated semi-implicitly. Pure: the same controls, the same drive. `data()` (speed,
the accelerometer, the engine's torque, the clutch, impacts and scraping), `wheels()` (`WheelState`:
travel, spin, steer, slips, load, surface, forces, how fast the tyre slides), `rpm()`, `gear()`,
`shifting()`, `observe()` (`CarObservation`, `Observation::car()`), `look()` and `show()` for a replay.

`car::ground`: `CarGround` is what a car asks of the world: the ground at a point (`Patch`: height,
normal, `Surface`, whether it is exact, the water on it, how much of a tyre is on paint) and the walls
near it. `TerrainGround` is the world's: its circuits where they are (with their water), the terrain
elsewhere (a plane kept for a step under each wheel). `Plane` is a level or sloping plane with walls
and water, for tests.

`car::wet`: grip on a wet road. `wet_grip(surface, paint, water, speed)` is the share of the dry
asphalt's friction a tyre has: on asphalt from the Bosch handbook's coefficients for a new road
tyre (0.85 dry, 0.65 under 0.2 mm, 0.55 under 1 mm, 0.5 in 2 mm puddles at 50 km/h; at 90 km/h
0.8, 0.6, 0.3, 0.05), the low-speed share by depth times what aquaplaning leaves, `1 / (1 +
(v/v_c)^n)` with `v_c` some 96 km/h in 1 mm (Horne's 9 √p knots), lower in deeper water; paint and
kerbs 0.7 of that (EN 1436's 45 against 55 to 70 for wet asphalt), wet grass 0.55, mud 0.6. The
Magic Formula's curvature is lowered on a wet road (a peakier curve) and, the stiffness kept, the
peak comes at less slip; anti-lock aims lower by as much. The water under a tyre is felt over
`WEDGE` (0.6 m) of rolling into it. `RoadWater` is a circuit's water: a film per 2 m and per metre
across that the rain (`Weather::rain`, 0 to 1) lays (0.15 to 0.7 mm, in a minute or so), dries in
some ten minutes and every tyre rolling over it wipes (the racing line dries first), and puddles in
the asphalt's hollows (the same noise as `world-render`'s `track.wgsl`, `puddle`), up to 2.5 mm,
filling in two minutes and draining in fifteen. `World::water` has one per circuit of the terrain,
stepped with the world; `World::settle_water` sets it as long rain leaves it; `World::water_ahead`
is what a driver sees of it on the road ahead (`CarObservation::water_ahead`).

`car::driver`: `RacingLine::new(circuit, car, pace)` works out a circuit's line of least curvature
within the asphalt (an elastic band) and the speeds along it (the grip in each corner, the braking
worked backwards and the acceleration forwards within a friction ellipse and the engine's power);
`Driver` is a `Pilot` that drives it from the observation: pure pursuit of the line ahead with the yaw
damped and counter-steer against a slide, the pedals by the line's speed a little ahead, the throttle
fed in as the rear tyres grip, the brakes at the threshold without anti-lock. On a wet road it sees
the film under its wheels and drives the line's speeds for it (`RacingLine::respeed` with the grip
of `wet::wet_asphalt` at each speed: slower corners, earlier braking, no faster than the water lets
the tyres keep the road), and slows for the puddles it sees ahead. It laps Lakeside Park in 115.7 s
dry, every lap the same; 134 s in light rain (0.2), 152 in steady rain (0.5), 169 in a downpour
(0.8). It is the teacher of `sim-env`'s `drive` task. `Driver::with_character(circuit, car,
Character::rival(skill, seed))` is a rival: its pace, braking points, own line and mistakes from a
seed; it sees the other cars (`Observation::traffic`): keeps a gap behind one in its way (matching it
as it slows), slipstreams and passes it on a straight when the braking for the next corner nears (on
its inside if there is room, dropping back if the other moves over), leaves a car's width beside one
alongside and lifts when squeezed towards the edge.

`circuit::race::Race` keeps a race among cars of a `World`: five lights a second apart and out after
a pause from the seed (`holding()` until then), each car's progress round from the start line, its
laps, the order (`order`, `position`), the gaps (`gap`: since the car ahead was where this one is),
the laps down, the flag (the leader's last lap; every other car finishes the next time it crosses the
line), the retired.

### Car files
A type of car is a file, `assets/cars/<key>/car.ron`, read by `car::file` as the aircraft's are (`load`,
`fleet`, `fleet_with`, `set_fleet`, the built-in ones unless the game gives others). The GT coupé's is
the example; its comments give the sources. SI units; positions body axes from the centre of
gravity. The sections: `key`, `name`, `aliases`; `visual` (`model`, a folder of `assets/cars`; the
driver's and the bonnet camera's eyes); `mass`, `inertia` (roll, pitch, yaw), `cg_height`; `front`
and `rear` (`Axle`: `x`, `track`, wheel rate, bump and rebound damping, travel, bump stop, anti-roll
bar, camber and its gain, toe and bump steer, roll centre, and its `tyre`: radius, width, inertia,
radial stiffness, the Magic Formula's numbers, relaxation lengths); `steering` (lock, Ackermann share,
turns of the wheel); `engine` (torque curve, drag, inertia, idle, limiter, shift light, cylinders);
`gearbox` (ratios, reverse, final drive, efficiency, shift time, clutch torque, the automatic's shift
points); `differential` (`Open` or `Limited`); `drive` (`Rear` or `Front`); `brakes` (torque,
front share, hand brake, the anti-lock slip); `aero` (drag area, lift at each axle); `body` (the boxes
another vehicle hits, the contact points, the crash speed).

### circuit
A circuit is a file, `assets/circuits/<key>.ron` (`circuit::file`: `load`, `circuits`, `circuits_with`,
`set_circuits`): its `place` (`Seed(n)`), `origin`, the asphalt's `width`, the `start` line, `laps`,
the cross `fall` on the straights, how far the natural ground's heights are `smoothing`ed, the
`sides` (run-off: grass, gravel or asphalt, its width to the barrier, the barrier: armco, tyres, a
wall or none, kerbs: auto, always or never, the pit lane behind), the `points` of the centre line
(metres from the origin; each may change the width, lift the ground, bank it, or either side from
there on), and the `props` beside it (pits, gantry, grandstands, marshals' posts, a footbridge, the floodlights' towers), which
`world-render` draws. `Circuit::new(file, terrain)` lays it out on the natural terrain: a centripetal
Catmull-Rom spline through the points sampled every `STEP` (2 m), the heights the ground's smoothed,
a fall to the right on the straights and banking into the corners (up to 3.5 %), kerbs on both sides
where it turns tighter than `KERB_RADIUS` (160 m), the run-off shrinking on the inside of tight
corners. It answers `locate(n, e)` (along, across), `patch` (the ground: asphalt with its cross slope,
kerbs raised with ridges, run-off, verge, the pit lane), `walls` (the barriers' pieces, facing the
track), `earthwork` (what the terrain is cut and filled to), `grid(k)`, `point`, `at`, `sample`.

`Terrain::with_circuits(circuits)` lays circuits on a terrain: under each, its ground a little lower
than the circuit's own (`UNDER`), then a bank back to the natural ground; the ground there is `flat`,
so no tree, house or field stands on it. A terrain without circuits is as it always was.

The built-in circuit is Lakeside Park (`assets/circuits/lakeside.ron`): 4.5 km in the world of seed 1,
between the lake 9 km east of the home aerodrome and the sea, 26 m of climb: a kilometre of straight
by the lake with the pits on its left, a fast right into the woods, sweepers down to a hairpin, a
chicane with asphalt run-off, esses climbing back and a long final corner.

### instruments
`Aircraft::instruments` is what the aircraft's dials and displays read, moved by the physics that moves
the real ones and never by the true state; `readings()` gives them as plain numbers (`Readings`),
and so does `AircraftObservation::instruments`. The world steps them after the aircraft, from what they sense
(`Sense`: altitude, attitude, rates, airflow, accelerometer, rpm, flaps), and settles them at every
placement as after taxiing out (no failures, switches off, gyros up to speed and erect, the heading
gyro set to the compass, the altimeter set to the aerodrome's QNH, `Atmosphere::qnh`). What a type
has is `AircraftType::instruments`, an `InstrumentFit`: the C172 one static port on the left with the
handbook's airspeed calibration, the alternate static source, pitot heat, vacuum attitude and heading
gyros; the Extra ports on both sides, no vacuum, an electric attitude gyro and a vertical-card compass.

- **Pitot-static.** Impact pressure (isentropic) of the airflow the tube recovers, true to 20° off its
  axis and nothing past 65°; the static ports see ambient plus the type's position error (by angle of
  attack and flaps) and, with one port, sideslip. The lines lag their sources (0.05 s, 0.1 s). The
  airspeed indicator is calibrated airspeed from pitot minus static; the altimeter the standard
  atmosphere's height of the static pressure above `altimeter_setting` (the Kollsman window, 946 to
  1050 hPa); the vertical speed indicator the static line against its case, which follows it through a
  capillary (`VSI_LEAK`, 2.5 s at sea level, longer as the pressure falls: 95 % of a new rate after
  7.5 s). The needles lag a little more.
- **Switches and failures** (`Failures`, for the failures to come; the game tries them by hand):
  `PitotBlock::Inlet` (the drain bleeds the line to static: the airspeed falls away),
  `PitotBlock::Sealed` (the trapped pressure makes the airspeed an altimeter), `static_blocked`
  (altimeter frozen, vertical speed back to zero, airspeed low climbing and high descending),
  `vacuum_pump`, `icing` (below `ICING_BELOW`, +2 °C, ice seals an unheated pitot in 45 s;
  `pitot_heat` melts it in 20 s). `alternate_static` takes the cabin's pressure (7 % of the impact
  pressure below ambient: a little high on both).
- **Vacuum and gyros.** Suction from rpm (4.4 inHg at idle, regulated to 5 from about 1000 rpm; LOW
  VACUUM below 3). The rotors spin up in some minutes and run down on friction over a quarter of an
  hour; a slow one is dragged by the case and wanders off. The attitude gyro is erected towards the
  felt vertical at `ERECTION` (4°/min: it leans after accelerations and long turns) and tumbles on
  its pitch stops (85°). The heading gyro drifts by its own 4 to 10°/h (seeded) plus the Earth's
  rotation the latitude nut does not take off, shows the gimbal error banked, and
  `align_heading_indicator` sets it to the compass.
- **Compass.** The Earth's field (`Earth::field`) plus the aircraft's own (`compass_iron`) seen in the
  plane of a pendulous card that hangs along the felt vertical (within 18°), turned as a damped
  oscillator (period about 1.8 s, damping 0.35) that keeps its place in space as the case turns:
  deviation of a few degrees, the northerly turning error (about 20° at 15° of bank), the acceleration
  error (ANDS), the swing.
- **Air data and attitude-heading computers** (`Readings::air_data`, `ahrs`), the G1000's: the same
  lines, read without the needles' lags (vertical speed through a 0.8 s filter), true airspeed from
  the Mach number and the outside air; attitude and magnetic heading from solid-state sensors, so a
  vacuum failure leaves them working and a blocked pitot fools them as it fools the dials.

### gfc700
`Gfc700::new(fit)` is the Cessna 172's Garmin GFC 700 (G1000 Nav III): a flight director and an
autopilot of three servos, roll, pitch and pitch trim, behaving as its pilot's guide describes.
What a type has is `AircraftType::afcs` (the C172's file gives one, the Extra's none): the servos'
authority (a share of the surfaces' travel, what their clutches pass) and rates, the trim servo's
speed, the bank limit (22°), the pitch limits (+20°, −15°) and the airspeeds of MAXSPD (160 kt)
and MINSPD (60 kt). It is a `Pilot` handed the controls the pilot's hands set: with the
autopilot off it returns them untouched and keeps only the flight director and the alerter going,
so the game runs it behind the keyboard's pilot every step; engaged, its servos take the ailerons,
the elevator and the trim. The rudder and the throttle stay the pilot's: the C172 has no yaw servo
and no autothrottle.

It flies on the G1000's data and nothing else (`Readings::air_data`, `ahrs`, the body rates; the
GPS's position, track and height): a blocked pitot fools it as it fools the pilot.

- **Keys** (`press(Key)`, `turn(Knob, clicks)`; false when a key does nothing): AP (with the
  flight director off, it comes on in ROL and PIT), FD (not off while the autopilot flies), HDG,
  ALT, NAV, APR, VS, FLC, NOSE UP / NOSE DN (the wheel: PIT 0.5°, VS 100 ft/min, FLC 1 kt, ALT
  10 ft), AP DISC (`Key::Disconnect`, also silences the alert), the HDG knob pushed (bug to the
  heading); the HDG knob 1°, the ALT knobs 100 and 1000 ft. VNV and BC are not fitted yet. A mode
  key with the flight director off turns it on, the other axis in its hold; pressed again, its
  axis goes back to ROL or PIT.
- **Lateral modes** (`LateralMode`): ROL (wings level under 6° of bank, else that bank up to 22°),
  HDG (to the bug at standard rate, 22° at most), GPS along the active leg of the `FlightPlan`
  (intercepting at up to 45°, the next leg begun a turn's lead early); armed while the aircraft is
  away from the leg and not closing on it, captured then (it flashes).
- **Vertical modes** (`VerticalMode`): PIT, ALT, VS, FLC (pitch for the airspeed, power the
  pilot's; it settles the airspeed at about 1 kt/s and never climbs away from a selected altitude
  below, nor dives away from one above), GP (down the approach's 3° path from the GPS height).
  ALTS arms in PIT, VS and FLC when the selected altitude is 50 ft away or more, captures it
  (ALTS) and becomes ALT there; turning the selected altitude while capturing gives PIT.
- **Flight plan**: `direct_to(&airport)` (D→, a leg from where the aircraft is); `load_approach(Approach)`:
  a GPS approach with vertical guidance to a runway end, straight to the intermediate fix 9 NM
  out, the final approach fix at 5 NM and the runway, joined on its legs if the aircraft is
  already lined up between them; APR flies it in GPS and arms GP, captured within 8 m below or
  20 m above the path.
- **Protections**: MAXSPD and MINSPD, looking 4 s ahead at the airspeed's trend, hold the limit
  by the pitch in PIT, VS and FLC; in ALT, ALTS and GP they only warn (power is the pilot's).
- **Disconnects**: the AP key, AP DISC, a push on the yoke past 0.3 of its travel for half a
  second (`push_yoke`) and the manual trim (`manual_trim`) are the pilot's: the two-tone sounds
  for 2 s and AP flashes for 5 s (`alert`, `tone`). Beyond 45° of bank, 30° nose up or 25° nose
  down, or crashed, it lets go by itself: the tone goes on until AP DISC. The flight director
  stays on in its modes. It does not engage on the ground nor beyond those attitudes.
- **Altitude alerter** (`altitude_alert`): approaching within 1000 ft of the selected altitude,
  5 s of flashing; within 200 ft; more than 200 ft off after having been within, 5 s of flashing.

What it does, measured (`tests/gfc700.rs`, a C172 at 100 kt): HDG turns 90°, 170° and 30° at
3.1°/s with 15.7° of bank and holds the bug within 0.1°, the altitude within 5 ft meanwhile; ALT
within 17 ft turning 120° in 15 kt of wind and turbulence 0.25; VS +500, +300 and −700 ft/min
within 10 ft/min, ALTS levelling with less than 1 ft of overshoot and holding within 4 ft; FLC
75 kt climbing at full power, 90 kt at idle and 110 kt descending within 1 kt once there, levelling
within 2 ft; MINSPD holds 60.0 kt with the nose 17° up at a third of the power, MAXSPD 160.0 kt
asked for −2000 ft/min at full power; D→ followed within 18 m, the aerodrome passed 2 m off; a
GPS approach joined from 45° off, the glide path held within 0.2 m and the centre line within 11 m
down to 200 ft. With the pitot inlet blocked ALT holds within 3 ft (MINSPD only warns); with the
pitot sealed in an FLC climb it reads 99 kt while the real airspeed falls to 49 kt, and lets go
18 s later as the aircraft stalls past 45° of bank.

### scenery
`trees(terrain, key, size)` and `houses(..)` give the instances of a square tile (key = east and south
index of the tile, axes east, up, south, as the game draws them); `tree_obstacles` and `house_obstacles`
turn the same tile into `Obstacle`s (`contains(n, e, height)`): a trunk and a crown per tree by its kind
(`Tree`, `CROWNS`, which the world draws as species of that crown: `world-render`'s "Trees"), a block
per part of a building (`BLOCKS`, which `world-render`'s meshes must match). Where a real place's
summers are dry (`summer_drought`, by its latitude: Menorca, Barcelona) its lowland conifers are
pines; `moisture` is how wet the land is, as before. Pure functions of the seed (and of the region: there the forest, the fields and the towns are where its maps put them, houses stand only on its built-up ground and 8 m or more from the kerb of its roads, and where no town covers it, a shed or a house on one square of 32 m in two or so).
No tree stands on a road, a railway's ground or a building of the maps (`Clear`, which
`world-render`'s parcels' trees use too): its trunk is `ROAD_VERGE` (2.5 m) or more from the kerb
of every road, generated or of the maps (as wide as the road map draws them), and `WALL_GAP` (3 m)
from the walls of every building; an avenue's trees stand on its pavements, their crowns over the
carriageway. What is drawn is what is run into: the same trees.
A region's buildings and bridges are not obstacles of a tile: each is its own solid, the terrain's
(`Terrain::solid_at`, `solids`), which a vehicle's points are asked against after the tiles' trees
and houses (an `Obstacle` of `Shape::Mapped` says what was struck). A building is its outline,
courtyards open, from 1.5 m under the lowest ground under its corners (`mapped_base`; or from its
bottom, for a part raised on others) up to its roof as `roofs::Frame` shapes it, the same numbers
`world-render` draws: a gabled roof is hit under its slopes, not over its eaves. A bridge is its
parts (`bridges::parts`): boxes of deck following its line and rise, piers, towers, girders, and
the bars of its cables, stays and hangers. Found by a grid of squares of 16 m listing what reaches
into each, so a point looks at a handful. No house of the towns stands within 50 m of a building
of the maps (where the maps draw buildings, they draw them all); `tallest` says how high to look.
The pieces of a wreck come to rest on the roofs and decks under them (`World::step`,
`Wreck::step_on`), and a fire's air stops at them (`world-render`'s `fire.rs`).

### roofs and bridges
`roofs::Frame::of(&footprint)` lays a roof in the box round the outline along its ridge (the
direction the maps give, else the longest side): where the walls end (`eaves`) and how much higher
it rises (the maps' `roof_height`, else by its shape and size: 0.6 of the half width for a gabled
roof, the half width for a dome, at least a storey of walls left under it). `height(ring, n, e)` is
how high the roof stands over a point: gabled, round and gambrel by a profile across the ridge
(the walls rise to meet it, as gables do, whatever the outline); skillion one plane; hipped,
pyramidal and mansard the lowest of a plane rising from each side (the straight skeleton of a
convex outline; a concave one gets a gabled roof); domes and onions round over the middle.
`bends()` are where a profile roof's faces meet, for a mesh to split the roof there.

`bridges::parts(&bridge, ground)` makes a bridge's parts, with proportions from how such bridges
are built where the maps say nothing (the module's header gives them): the deck in boxes of up to
24 m along the line, as deep as its span asks (1.8 m for a road on piers, a 170th of the main span
for a suspension bridge, the Golden Gate's 7.6 m); piers every 32 m for a road (18 m for a path, 35
m for a railway) where the deck stands clear of the ground, one column under a narrow deck, two
under a wide one; a suspension bridge's two towers (steel legs and beams, or one block of stone
pierced by an arch over each half of the deck, `Bridge::masonry`), its two cables sagging in a
parabola to just over the deck in the middle, hangers every 15.24 m, no piers; a cable-stayed
bridge's pylons and stays in two planes; a deck arch's ribs and columns; a truss's girders.
`aerodrome_obstacles(terrain, &airport)` gives the blocks of one aerodrome's hangars, office and parked
aircraft, unrounded (what its obstacle lights stand on).

### lights
`aerodrome(terrain, &airport)` gives every light of an aerodrome as plain data (`Light`): where it
shines from, its aviation colour, the intensity of its main beam in candelas and the beam (axis,
half width and height, a beacon's turn), what it shines all round, a slope indicator's red-to-white
angle, a flash, the size of its lens and the system it belongs to. Its class (`Class::of`, from its data on a real aerodrome, else from its
id: visual, non-precision, category I or II) decides what it has; the layout, colours and
intensities follow ICAO Annex 14 and the FAA's AC 150/5340-30 (the module's header lists them):
edge lights (yellow over the last third on instrument runways), threshold and end, centre line and
touchdown zone for category II, a MALSR or an ALSF-2 on the main approach (`main_end_reversed`),
REIL strobes, a PAPI at the aiming point (`AIM`, where `reset_on_final` and the autopilot aim: 3°30',
3°10', 2°50', 2°30' from the runway out, a 3' transition) or a VASI on visual fields, taxiway and apron
edges, two floodlight masts, the beacon on the office's mast, obstacle lights on the hangars and the
windsock (`windsock`). `setting(system, sun_elevation, visibility)` is the share of the highest
intensity a tower would set for the light of the day and the visibility, 0 when off. Pure functions:
the same aerodrome, the same lights.

### pilot
`trait Pilot { fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls; }`
A pilot sees only the `Observation` and the controls as they are, and returns the new controls, of
the vehicle's kind (`Controls::Aircraft(AircraftControls)`, `Controls::Multirotor(MultirotorControls)`).
The aircraft's pilots (`KeyboardPilot`, `Autopilot`, `Gfc700`) do their work in
`fly_aircraft(obs, &AircraftControls, dt)`, which their `fly` calls for an aircraft; a multirotor's
controls they hand back untouched. A multirotor's pilot sets its sticks (`tests/multirotor.rs` has a
hover pilot on the observation alone).
`KeyboardPilot` moves only aileron, elevator and rudder and returns the rest of `current` untouched.
Set its key state before each step: `roll`, `pitch`, `pedals`, `fine` (-1/0/1), `mouse_yoke`,
`mouse_rudder`; `assist` chooses between moving the yoke directly and holding an attitude
(`hold_attitude(pitch, bank)` gives it that attitude in numbers instead of with the arrows: the
game's remote pilot flies with it).
With the assist the arrows hold up to `ASSIST_PITCH` (30°) of nose and `ASSIST_BANK` (60°) of bank; a key
held past them, or the aircraft beyond them, asks for a pitch or roll rate instead, kept to
`ASSIST_LOAD` (3.4 g) and short of the stall unless held against those limits for `ASSIST_INSIST` (3 s).
`yoke_x()`, `yoke_y()`, `pedal()` are where the controls are; `centre()` and `reset()` forget them.

### autopilot
`Autopilot::new(goal)` is a `Pilot` that works every control (yoke, pedals, throttle, flaps, trim)
from the `Observation` alone, for a C172; `Autopilot::for_type(goal, &aircraft.p)` flies any type at
its own `Speeds` (and on a tailwheel keeps the stick neutral on the rollout while it could still fly,
then right back). Hand it to `World::fly` each step; `set_goal` changes what it does,
`phase()` says where it is, `finished()` when there is nothing left, `landings()` how many times it
has stopped on a runway. Dropping it is disengaging: it keeps the aircraft in trim.

| Goal | Does |
|---|---|
| `Hold { heading, vertical, airspeed }` | Heading by coordinated turns at standard rate (bank ≤ 25°; `None` = wings level); `Vertical::Altitude(m)` or `Vertical::Speed(m/s)` with the pitch; airspeed with the throttle (`None` leaves it alone) |
| `Route(Vec<Waypoint>)` | Flies the legs between the points at `cruise` (100 kt), turning before each one, then keeps the last course and altitude |
| `Circuit { airport, reversed, repeat }` | Takeoff (rotate at 55 kt, climb at 74), left-hand circuit 300 m above the runway and 1200 m from it, landing, stop; with `repeat`, again from where it stopped |
| `Land { airport, reversed }` | Joins the downwind leg (circling down first if it arrives high), lands and stops |

The circuit's shape and speeds are `Autopilot::pattern`, a `Pattern` (default: the standard circuit
above; height, width, final length, aiming point, glide angle, when to turn crosswind, bank,
every speed, landing flaps, and `land_reversed` to come back the other way in a teardrop).

Limits it keeps: never slower than 1.3 times the stall speed of the flap setting (1.2 on base and
final), never faster than 120 kt, never lower than 100 m over the ground ahead in a hold or on a
route (if a straight climb would not clear it, it circles up first). The approach is a 2200 m final
on the 3° path to the point 150 m past the threshold, flaps 20, 66 kt, the wing held down into a
crosswind in the flare. It does not go around, does not see other aircraft, and assumes the ground
under the circuit is lower than the circuit. A small state machine (`Phase`) over cascaded loops;
every gain is a documented field of `Gains`.

A transport (a jet: the Boeing 747) is flown with a profile, gains and a circuit of its own
(`Profile::for_type`, `Gains::for_type`, `Pattern::for_type`): the circuit as much wider and longer
as the square of its speed asks (some 7 km wide and 700 m up, a final of 10 km), a slow rotation to
9° and a first climb at 20 kt over the rotation speed with its take-off flaps, the flaps in a notch
at a time as its speed allows each (1.25 times that notch's stall speed, as the model stalls) and
out below each notch's placard speed, the gear up once climbing away and down for the base leg,
the yoke's and throttles' gains for an aircraft that answers in seconds, a flare from 12 m, and on
the runway the spoilers, reverse thrust down to 60 kt and the brakes. A light aircraft's are the
defaults, and it flies as before. `tests/b747.rs` flies the circuit.

## Tests

`cargo test -p sim-core` (about 5 s; `-- --nocapture` prints the measurements). A failing figure
means the model or the port changed: fix the cause, never loosen a tolerance.

The handbook is the truth for what it gives; JSBSim's `c172p` is the reference for the rest, with
its numbers kept as constants in `tests/jsbsim_reference.rs` (nothing of JSBSim is needed to run them).
After a deliberate change to the flight model, once those pass, regenerate the regression snapshot and
commit its diff with the change:

```text
FLIGHT_SNAPSHOT=update cargo test -p sim-core --test golden
```

| File | Guards |
|---|---|
| `tests/golden.rs` | The world, fixed for ever: noise and hashes (exact), terrain, airports, towns, roads, wind against the Swift reference. And the flight model as a regression snapshot (`tests/physics_snapshot.txt`): rigging, stall, trim, forces, whole flights, keyboard-pilot runs |
| `tests/jsbsim_reference.rs` | Against JSBSim c172p: stability and control derivatives, elevator, aileron and rudder responses, phugoid, dutch roll, trim changes with flaps and power, ground effect |
| `tests/loading.rs` | Weight, centre of gravity, inertia, fuel burn and fuel exhaustion |
| `tests/extra300.rs` | The Extra 300L against its published figures: stall at 820 and 950 kg, climb, cruise at 65 %, the governor at 2700 rpm, 400°/s of roll, ±10 g and what breaks it, inverted flight, three-point attitude, tail-up take-off, ground loop, gear, the autopilot; the governor's blade angle against JSBSim's `propC10v` (numbers only, as a reference) |
| `tests/flight_model.rs` | Handbook figures: stall, cruise, climb, glide, rpm, takeoff roll, brakes, spin, stability, stall hysteresis |
| `tests/damage.rs` | What breaks: wings at the ultimate load, flutter past 1.2 VD one surface at a time, a wing or half tailplane torn across in flight (C172 and Extra: the roll, and the aileron that holds it), a tip crushed or scraped on the ground, a tail strike, a burst tank, trees by energy (felled, a wing cut by a trunk, a crown that drags, a trunk that stops), flaps over their speed, the gear against FAR 23 descent velocities, impacts, ditching, trees, hangars, mid-air collisions, wrecks, their fragments, scars and splashes, what burns (only an impact that bursts the tanks and lights the fuel, never in water), determinism |
| `tests/flight_test_campaign.rs` | Climb and cruise tables, stall warning margin; prints modes and handling |
| `tests/instruments.rs` | The instruments from their physics: the C172 handbook's airspeed calibration, calibrated against equivalent airspeed, the altimeter against its setting and on a cold day, QNH, the vertical speed indicator's lag, the pull-up's static error, blocked inlet, sealed pitot, blocked static and the alternate source, pitot ice and heat, the pitot far off its axis; suction against rpm, erection, heading gyro drift, the gyros running down without vacuum while the AHRS works, tumbling; the compass's deviation, northerly turning and acceleration errors and swing; the instruments in the world |
| `tests/world.rs` | Terrain variety and determinism, flat dry airports, lakes (in river valleys, flat at their level, away from aerodromes, towns and the sea, floated on), atmosphere (the standard tables up to 20 km, humid air and the power it takes), turbulence, three planes at once, threads, brakes holding full power and a crosswind |
| `tests/solids.rs` | A real place's buildings and bridges as the vehicles meet them: a C172 flies through the open corner of an L-shaped block (its box would have held it) and strikes its arm, flies over its roof and under a bridge between its piers and strikes its deck; the FPV quad crosses a courtyard and crashes into its wall; a wreck comes to rest on the roof it fell on |
| `tests/region.rs` | A terrain from a small made-up region: heights, sea and lake, its runways, roads and towns, the cover deciding the ground, real aerodromes' lights and buildings by their data, trees and houses by the cover, no tree on a road, a railway or against a building (in a region and a generated world), a rail yard among a city's blocks neither wood nor field, a world on it deterministic and on its latitude, floating on its sea and lake |
| `tests/aircraft_files.rs` | The built-in fleet and its names; what a broken file is told (line and column, the field); files on disk in place of the built-in types or after them |
| `tests/fleet.rs` | A third type that is only a file (`tests/fixtures/trainer.ron`), in the fleet, flown by the autopilot round a circuit |
| `tests/autopilot.rs` | The autopilot through `World::fly`: holds, speed limits, a route, rising ground, circuits in calm, crosswind and turbulence, arrivals, gain margins, three aircraft for 15 minutes |
| `tests/gfc700.rs` | The GFC 700 through `World::fly`: ROL, HDG, ALT in wind and turbulence, VS and FLC with ALTS capturing, FLC kept from climbing away, PIT with MINSPD and MAXSPD, D→ and NAV armed then captured, the GPS approach down its glide path, the disconnects and their tone, the altitude alerter, a blocked and a sealed pitot, determinism |
| `tests/keyboard.rs` | `KeyboardPilot` direct and assisted: takeoff, attitude hold, landing, a loop, a steep dive and an aileron roll with the assist; leaves throttle/flaps/trim alone |
| `tests/course.rs` | Race courses: the course and gate files, a gate counted only through its opening the right way, the frame solid, the racer flying each built-in course's two laps through every gate (Pinewood in the FPV quad, Deepwood in the race quad), Deepwood's trail clear of trunks, low crowns and gate frames in a thick wood |
| `tests/multirotor.rs` | The FPV quad: its file, 4.4:1 and hovering at 37 % of the throttle (thrust equal to its weight; sinking as the battery runs down), climbing at 25 m/s at full throttle with the battery sagging, 31 m/s flat out, yaw from the motors' torque (counter-clockwise rotors faster to yaw right), the "Betaflight" rates, the sticks tilting it and it levelling when let go, arming only with the throttle down, a hard fall crashing it, wobbling as it falls into its own wash at zero throttle (less with throttle, not moving forward), the wind carrying it, a hover pilot on the observation, standing on its feet and landing on its back, determinism |
| `tests/car.rs` | The GT coupé against its class's figures: 0-100 km/h in 4.76 s with traction control (4.3 to 5), 100-0 in 34.8 m with anti-lock brakes (33 to 37; locked, 47 m), 284 km/h flat out (270 to 295), 0.99 g held on a skidpad of 45.7 m (0.95 to 1.05); the load moved to the outer wheels cornering and to the front ones braking, at 0.7 g, within 4 % of what statics say; its handling: a step of steering at 100 km/h settles at 0.6 to 2.9° of sideslip (it understeers), full throttle in a corner at 50 km/h does not spin it with the aids nor, without them, at 3° of steering (at 9° it does: power oversteer), from 20 km/h with half lock it can be provoked and the aids catch it, lifting off in a corner at 100 km/h steps the tail out under 3°; braking from 200 km/h the automatic's four shifts down slip the rear tyres 0.07 at most (the revs matched), and a shift down past the limiter is refused; full throttle with the wheel worked from side to side gains 134 km/h in 5 s with stability control against 139 without; the automatic holds its gear through a lift and shifts up cruising; still at rest, parked with the hand brake on a 15 % slope facing up and down, no energy from nowhere coasting or dropped, a barrier scraped along at a shallow angle and struck square, determinism; on a wet road (0.2, 0.5 and 1 mm of water): 100-0 km/h in 45.0, 48.3 and 73.2 m with anti-lock brakes (34.8 dry), 0.79, 0.74 and 0.64 g on the skidpad (0.99 dry), and in 3 mm at 120 km/h a second of full braking takes off 2 km/h (aquaplaning); `wet.rs`'s own tests check the handbook's table |
| `tests/race.rs` | Races of racing drivers with their characters from a standing start: six cars, two laps, all finishing within seconds of each other, nobody wrecked, no hard hit; the same race twice; a wet race slower than a dry one (rain 0.5: 158 s a lap against 119); the racing line dries first when the rain stops (0.24 mm on it, 0.31 beside it after four minutes, from 0.48). `--ignored many_races` (`RACES`, `FIRST`, `RAIN`, `TOUCHES`) races many and counts the touches |
| `tests/circuit.rs` | Circuits: the files and what a broken one is told, Lakeside Park laid out (length, climb, a hairpin, closed, its parts apart), the terrain under it and nothing growing on it, its barriers facing the track, two cars knocked apart, and the racing driver lapping it within the kerbs, every flying lap the same (116 s) |
| `tests/race_quad.rs` | The racing quad: in the fleet, 13.6:1 and hovering at 15 %, 49 m/s climbing, 175 km/h flat out, 447°/s of yaw, banking to 75° without swinging past |
| `tests/tiltrotor.rs` | The Kestrel tilt-rotor: its numbers (the collective at 52 % in the hover, 1.57:1), a still hover on 1.28 MW with the rotors at their speed, 60°/s of yaw at full stick by tilting one nacelle against the other, leaning where the sticks say when it levels itself, going forward and stopping by its rings with its nose within 2° of level (10° of nacelles at half stick, 13 m/s in 8 s, stopped in 6) and holding the pitch trimmed, holding itself still in a gusting wind and going at the climb and the speeds over the ground its levers ask for, down onto its skids, the rates' help (30°/s at half stick, the bank kept when let go) and none (the sticks' accelerations, only the rotors damping it), the conversion to 83 m/s and back to the hover, a balanced turn at 25° of bank, lifting off its skids and landing on them, the engines stopped and started again (14 s to speed) and overpitched, the rotors turning on the air with the engines out, and a surface's lift, stall and drag |
| `tests/quad_strikes.rs` | A quad against fixed things: a light graze of a gate's tube chips a tip and it flies on; into a post at 5 m/s its propellers break and it comes down, never held in the air; its frame bounces off a tube at its restitution; into a trunk at 15 m/s it crashes; through a crown at 10 m/s the twigs break its propellers |
| `tests/quads_collide.rs` | Two FPV quads touching: frames that meet knock apart once (momentum kept, the restitution's share of the speed back) and part, nothing sticking; propellers meeting at 3 m/s break, both blades to stubs, and the quads tumble down and crash on the ground; a brush of the tips at 1 m/s only chips them, unevenly, and the quad flies on, its height kept, shaking |
