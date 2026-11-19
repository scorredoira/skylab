# Cars

The plan for driving: a second game, `crates/games/drive`, on the same world as the flight game.
Realistic physics (roughly the level of Forza or Gran Turismo, not arcade), good looks, and an
environment to train AI drivers, like the drone.

## Why on Skylab and not from scratch

The world is already there: the renderer (sky, volumetric clouds, water, shadows, rain, night),
sound, settings, the place picker, and real places anywhere on Earth with their relief, roads and
satellite colour. A driving game made from scratch would spend months getting back to that point.
The racing games made with AI that circulate online are invented cities with arcade physics; here
the hook is different: **real, famous roads, driven with real physics**.

## Where it stands (17 Nov 2026)

The first car drives on the first circuit (the owner chose a race circuit before a public road):

- **The car's physics**, `sim_core::car` (phase 1, done): a rigid body on four wheels with their own
  suspension (wheel rate, bump and rebound, anti-roll bars, bump stops, camber and toe with travel),
  Magic Formula tyres with combined slip, load sensitivity, camber thrust, relaxation lengths and
  rolling resistance, grip per surface (asphalt, kerb, concrete, grass, gravel, earth); engine,
  clutch (pedal or by itself), sequential and automatic gearbox, limited-slip differential, rear
  drive; brakes with bias and lock-up, anti-lock, traction and stability control (on, as in the road
  car, each switchable off); a road GT's understeer at the limit; drag and downforce;
  walls and other cars it bounces off or is wrecked by. Sub-stepped at 960 Hz, pure and deterministic.
- **The first car**, the GT coupé (`assets/cars/gt`), a generic rear-drive sports coupé of 1420 kg
  and 375 hp, of no make, against its class's figures (`sim-core/tests/car.rs`): 0-100 km/h 4.76 s,
  100-0 km/h 34.8 m with ABS, 284 km/h, 0.99 g on the skidpad, under 3° of sideslip in a step of
  steering at 100 km/h. Its model, an original fastback
  with its cabin, wheels and brakes, is made by a program (`crates/games/drive/examples/car_model/`:
  shapes as fields turned into smooth triangles) and drawn by the game's own shader: paint under a
  clear coat that mirrors the sky and its clouds, see-through glass, lamps that shine, brake
  discs that glow. Its tail has a form of its own (a ducktail's lip, the lamps' band set in with a
  line of light and the brake lights' cells, a panel set back with a lettered plate, a black
  valance with the diffuser's fins and the exhausts).
- **The first circuit**, Lakeside Park (`assets/circuits/lakeside.ron`, `sim_core::circuit`): an original
  4.5 km layout in the world of seed 1, by a lake, with a kilometre of straight, fast sweepers, a
  hairpin, a chicane, esses and 26 m of climb; the terrain cut and filled to it; its own mesh in
  `world-render` (`track.rs`): asphalt with its lines and grid, the rubber of the racing line,
  patches and sealed cracks, kerbs, run-off, barriers, pits, gantry, grandstands with their crowd
  and flags, advertising boards of invented names, floodlights placed by hand in its file that
  light it (and a little of the land round it) by night, marshals' posts, tufts of grass on its
  verges; wet in the rain, with standing water that mirrors the sky and what stands on the road
  (the car, its lamps, the towers' lamps as streaks by night).
- **The game**, `crates/games/drive` (`scripts/drive.sh`): chase, bonnet, cockpit and trackside
  cameras (the chase camera looks into the corner; the trackside ones
  have a long lens focused on the car), keyboard, gamepad and joystick (the T.16000M) through a
  mapping file, a HUD with lap and sector times, `--shot`. What the
  tyres leave: marks of rubber, smoke, dust, spray. A cockpit with numbered dials that move and
  glow by night, the driver's hands on the wheel, mirrors that show the road behind, drops on
  the glass and wipers in the rain. By night the car's dipped beams light the road and its
  verges and its tail lamps the ground behind. Through the lens: bloom, a vignette, the sun's
  flare, depth of field (the lens no longer opens with speed, nor blurs round the car: it read as
  a zoom).
- **The sound**, from real recordings (CC0, `assets/cars/gt/sound/`, credited in its `CREDITS.md`),
  as racing games make it: an engine recorded on a dynamometer at steady revs, on and off load,
  from the exhaust and the engine bay, two loops at a time resampled to the exact revs and blended
  by the engine's torque (lift-off, the gear change's fuel cut, the limiter, the starter); the tyres
  squealing and screaming by how they slide, rolling, kerbs' ridges, gravel, grass; the wind;
  impacts and scraping. Muffled from the seat; from the trackside cameras fainter with distance,
  with Doppler. Rendered off line with a video's frames (`--sound`) to check it. The recorded engine
  is a four (a Mercedes-Benz 190E 2.3-16V), not the GT's six: no free set of a six was found.
- **AI drivers**: `sim-env`'s `drive` task and the racing driver (`sim_core::car::driver`, a
  minimum-curvature line and its speeds), which laps cleanly and is the teacher.

Then (17 Nov 2026, later): **rain that is felt** (`sim_core::car::wet`: a wet road keeps 0.75 of
the dry grip, less at speed in deeper water, from the Bosch handbook's table; puddles where the
renderer draws them that aquaplane the tyres; slippery kerbs and lines; a film the rain lays and the
cars' tyres dry along the racing line); **races** against GT coupés driven by the racing driver
with characters of their own, which follow, slipstream, pass and give room (`crates/games/drive`,
`--race`); **the replay** (`F2`); **the ghost** of the best lap in practice, kept on disk; and a
network trained on `sim-env`'s `drive` task, dry and wet, that races as a rival (`trainer/DRIVE.md`).

Then (18 Nov 2026): the load the tyres' forces move is what statics say (it was 15 % short: the
forces reached the body at the roll centres, not at the road); shifts down match the revs (each one
slipped the rear tyres by a third, braking into a corner); stability control leaves quick
corrections of the wheel alone and the automatic holds its gear through a lift: with both, a
player at the keyboard (`crates/games/drive/src/feel.rs`, a test that drives with the keys) laps
Lakeside in 118 s, the racing driver in 115; before, in 255. Without the aids, the pedals' keys are feet
that feel the tyres (`G`): the throttle eased while the driven tyres spin, the brake while a wheel
locks; that player then laps in 122 s (all or nothing, it spins every lap). The driving line (`L`): arrows on
the road along the racing driver's line, green, yellow or red by the car's speed against the
line's. A steering wheel is a
`wheel` section of the controls' file: the front wheels turn as it is turned.

Then (18 Nov 2026): **the Tesla Model Y** (`assets/cars/modely`, `--car modely`), the 2025 Long Range
with all-wheel drive, the first electric car. `sim_core::car` drives it with no engine nor
gearbox (`electric` in its file): a motor on each axle (210 N m in front, 400 behind, each through
a 9.04 reduction and an open differential, the same torque to both wheels), the battery's 300 kW
shared by the two, its software's 201 km/h; traction control per axle; let go, the pedal
regenerates (the motors' torque up to the 75 kW the battery takes back, some 0.17 g at 100 km/h,
0.26 g at 40), the brakes hold it when it stops (Tesla's hold); drive, neutral and reverse. Against
Tesla's and the road tests' figures (`sim-core/tests/modely.rs`): 2000 kg, 0-100 km/h 4.88 s (4.8),
201 km/h, 100-0 km/h 36.8 m (34 to 38), 0.85 g on the skidpad (0.84 to 0.90). The racing driver laps
Lakeside in it in 123 s (the GT: 115), a player at the keyboard in 126. The HUD shows D, R or N
and the power meter (kW drawn, and given back in green); its sound has no engine: the motors'
and gears' whine at their orders of the shaft's speed, louder the more torque they pass, the
hum it plays for people on foot below 30 km/h, the tyres and the wind (`assets/cars/modely/sound`).
Its model is not made by a program: it is a model made in a modelling program
(`assets/cars/modely/source.glb`), turned into the game's by `crates/games/drive/examples/import_car.rs`
as `assets/cars/modely/import.ron` says (its axes, its materials as the game's, its wheels found by
their tyres and taken apart); the axles, tracks and tyres of `car.ron` are where the model has them.

Still to do on the circuit: an engine sound recorded from a six, force feedback for a steering
wheel, rivals' shadows. Of the look: mirrors that show the real scene (they draw the circuit's
own ground as it is where the car is, not its barriers, stands or other cars), headlights on the
land beyond the circuit's ground, a shadow of the car under the floodlights. Then phase 2 below:
the roads.

## Where each part goes

```
crates/sim/sim-core/src/car/       the car: tyres, suspension, drivetrain, brakes, aero (pure, no graphics)
crates/sim/geodata                 roads as a graph from OpenStreetMap, their profile, finer terrain where open
crates/engine/world-render         the road mesh, the ground seen from 1 m, roadside things, wet asphalt
crates/sim/sim-env                 a `drive` task to train drivers
crates/games/drive                 the game: cameras, HUD, keys/wheel/pad, sound, time trial
assets/cars/<id>/                  each car as data: car.ron, exterior.glb, cockpit.glb
```

The rules of `ARCHITECTURE.md` hold: physics pure and deterministic, the world knows no vehicle,
effects are the world's (skid marks, smoke, fire), inputs go through the mapping layer.

## Phases

### 1. The car's physics

- **Body**: a rigid body with mass, centre of gravity and inertia, stepped at 500–1000 Hz.
- **Suspension**: one ray or shape per wheel, spring and damper (bump and rebound), anti-roll bars,
  bump stops; geometry reduced to camber and toe changes with travel.
- **Tyres**: the Magic Formula (Pacejka) for longitudinal and lateral force, combined slip, load
  sensitivity, relaxation length (so the tyre builds force over a distance, which is what makes
  slides feel right), rolling resistance, grip per surface (dry and wet asphalt, gravel, grass).
  Coefficients from public papers and books, never from other games.
- **Drivetrain**: engine torque curve, inertia, rev limiter, clutch, gearbox (manual and
  automatic), differential (open and limited slip), front, rear or all-wheel drive.
- **Brakes**: brake bias, lock-up; ABS, traction control and ESC, on by default (as in the road car), each switchable off.
- **Aero**: drag, lift or downforce, front and rear.
- **Checked against published figures**: 0–100 km/h, 100–0 braking distance, top speed,
  skidpad lateral g. Tests for each, like the aircraft's.
- **First car**: a rear-drive sports car with numbers typical of its class (the GT coupé). Then
  real cars as they are, by their published figures (the Tesla Model Y). More cars as data.

### 2. Roads you can drive

Today roads are straight segments with a width, drawn as a texture on terrain of 30 m per pixel.
That is fine from a plane and unusable from a car.

- **A road graph** from OpenStreetMap: whole ways in order, with type, surface, lanes, width,
  bridges and tunnels.
- **The road's own profile**: a smooth line along its length (curves and gradients a real road
  has), not the 30 m terrain under it. The terrain is cut and filled to meet it, with banks.
- **Finer terrain where it is open**: 5 m in Spain (CC BY 4.0), 0.5–2 m in Switzerland, 1–10 m in
  the US and other countries; 30 m elsewhere. The licences are checked before using each one.
- **The road mesh**: camber, edges or kerbs, painted lines, asphalt that looks wet in rain,
  barriers on mountain roads. Tunnels come later.

### 3. The ground seen from 1 m

Verges, grass, gravel, rocks, posts, signs, trees close up. Today the ground is designed to be seen
from the air; the drone already showed where it looks flat (roads and aprons).

### 4. The game

- **Cameras**: chase, bonnet, cockpit, and replay cameras by the road.
- **Controls**: keyboard, pad, joystick and steering wheel with pedals (force feedback later),
  through `controls.rs`.
- **HUD**: speed, gear, rev counter, lap or stage time.
- **Sound**: recorded loops of a real engine at steady revs, on and off load, blended by revs and
  load (done on the circuit); tyres, wind, impacts.
- **Time trial**: a stage on a famous road, start and finish, a ghost of your best run, best times.
- **Picking the place**: the place picker with a list of famous roads, besides any road on the map.

### 5. Spectacle

Golden hour, night with headlights, rain with spray and reflections on the road, dust on gravel,
skid marks and tyre smoke, cinematic replays. The first video must look good.

### 6. AI drivers

A `drive` task in `sim-env` (stay on the road, time trial), a driving autopilot as the teacher, and
a network trained like the drone's (imitation, DAgger, PPO). Then race against it, as with the drone.

### Later

Traffic, more cars, damage with dents and parts, towns seen from the street, multiplayer.

## The first milestone

**One realistic sports car on one famous mountain road, at golden hour, as a time trial with a
ghost.** It needs phases 1 and 2, the basics of 3 and 4, and golden hour from 5. Candidates for the
road: Sa Calobra (Mallorca), Stelvio (Italy), Transfăgărășan (Romania), the Furka pass
(Switzerland, very fine terrain available).

## To decide

- The first road.
- Decided: the first car is a rear-drive sports car (the GT coupé); first a race circuit, then a
  road; keyboard, pad and joystick first, a steering wheel later.
