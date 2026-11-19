# Plan

Everything still to do, in order. The details of each line are in `ROADMAP.md` (where it goes in
the code), `PHASE2.md` (vehicles and systems) and `PHASE3.md` (fine tuning).

## In progress

- **Cars** (`docs/CARS.md`): the first car on the first circuit is done (17 Nov 2026): the GT coupé
  on Lakeside Park, `scripts/drive.sh`. Next: a ghost of the best lap and a time trial, opponents
  (the racing driver in other cars), skid marks and smoke, rain on the asphalt, a better cockpit,
  a steering wheel with force feedback; then a famous road.
- **Drone race circuit**: LED gates as data (`assets/courses`), countdown, lap and split times,
  best lap, picked in the place picker (M) or with `--course`; an automatic pilot that proves it
  flyable; a `sim-env` task to train on it. Done: Pinewood Airfield (FPV quad) and Deepwood Forest,
  a forest track cut through the trees (logs, hoops, a high hoop, a split-S) raced in the 6S race
  quad (`race5`), with its trained network (`trainer/DEEPWOOD.md`); quads strike gates and trees as
  they strike each other (props break or chip, frames bounce). Left: the network does not see the
  other quads; a forest course in a real place (the trail's cut needs the region's ICAO to be told
  apart); the trail's floor is the forest's grass, not a dirt path.
- **Drone fixes**:
  - it drifts forward with no wind;
  - coordinated turns with a joystick feel wrong: axis mapping, yaw rate, yaw about the vertical
    when banked.
- **Game fixes**:
  - the game stalls after changing settings (wind, quality): it must stay responsive while it
    applies them;
  - a loading screen when changing place, with progress, and the reason if it fails.
- **Look**: turquoise shallow water; wheel shadows; clouds with rain curtains, overcast with
  relief from the ground, a more dramatic golden hour.

## Next, in order

1. **Small jet** (a trainer or a business jet). It brings what every jet needs: a jet engine
   (spool-up, thrust falling with height, fuel flow), retractable gear, speed brakes, and
   compressibility (drag rise and stability changes near Mach 0.8). Realistic in its main numbers;
   less public data than the 747.
2. **Boeing 747**. Reuses the jet engine, ×4. Aerodynamic data from NASA CR-114494 (public
   domain). Adds bogie gear, slats, multi-section flaps, spoilers, reverse thrust, an autopilot
   with autothrottle. The cockpit starts simple; systems (hydraulics, pressurisation, tanks, FMS)
   come later.
3. **Cars on roads**: the driving game (`crates/games/drive`, begun on a circuit) on a famous road:
   roads with their own mesh from the maps, and ground detail seen from 1 m (`docs/CARS.md`).

## Open bugs

- Madeira: the aircraft's instruments start to oscillate. Not reproduced; needs to know when it
  happens (ground or air, which aircraft, autopilot on, after a crash or F2).
- The FPV picture in the manual still shows the old on-screen display ("EASY"): re-run
  `scripts/manual-shots.sh`.
- The README points to the first Swift version at commit `4386a3a`, which is no longer in the
  repository's history.

## The tilt-rotor, still missing

The Kestrel flies (18 Nov 2026: `assets/aircraft/kestrel`, a multirotor with tilting nacelles,
turbines, collective and cyclic pitch and a tail, `crates/sim/sim-core/README.md`, "Tilt-rotors"). Left:

- More of its cockpit: it has its lining, seats, panel, three displays and controls; no
  switches, nothing printed, no mouse on it, no display on the windscreen.
- The nacelles' lever and the stability switch on a controller; today they are keys only (F, G, C).
- Its own sound (turbines, rotors): it sounds as a quad's rotors do, lower.
- Textures on its model (plates, rivets, wear: it is made in Blender, `scripts/blender/kestrel.py`,
  in plain colours); a wreck with pieces and fire.
- Its weight falling as it burns its fuel; one engine out of the two.

## The drone, still missing

- A wreck with pieces and fire when it crashes.
- Trained models flying it in the game (`--model`), not only in `sim-env`.
- Tried with a real RC transmitter.

## Look

- Textures per pixel on glTF models (today one colour per vertex).
- Ray tracing in the cockpit only.

## Flight and world (phase 2)

- Time acceleration; an in-screen map.
- Weather physics: gusts, shear, thermals, low ceilings, rain.
- Fuel and weight managed; engine and instrument failures in flight.
- Radio navigation (VOR, ILS, NDB) and flight plans on the G1000; radio and ATC with voice.
- Multiplayer with AI pilots.
- Sky: dawn and dusk, moon phases, darker night, town glow.

## Fine tuning (phase 3)

Damage detail, instrument and system detail, runway slopes, hedgerows, taxiway lights. See
`PHASE3.md`. Done there: cities (17 Nov 2026): the buildings of the whole world from Overture
Maps with their roofs, parts and facades, each run into as its own outline; OpenStreetMap's
bridges with decks, piers and structures; the Golden Gate and the Brooklyn Bridge by hand.

## Paused

- The web version (until the end of the project).
