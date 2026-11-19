# Phase 2

What is set aside for after phase 1. Phase 1 ends with the realistic cockpit, the real autopilot,
the place picker, the architecture review (aircraft as data) and the loose ends of every line done
so far. Nothing here is started before that. `ROADMAP.md` says where each piece goes in the code
when its time comes.

## New vehicles and games

- ~~**FPV drone**~~ (done, 17 Nov 2026: `sim_core::multirotor`; in the game
  `crates/games/flight/README.md`, "The FPV quad"): four rotors, self-levelling (no flight modes),
  an uptilted wide camera, an RC transmitter over USB as a joystick. It is the test that `sim-core`
  holds vehicles, not only aircraft (seam 4).
- ~~**Avatar-style tilt-rotor drone**: invented but coherent physics, not arcade~~ (done: the
  Kestrel, `crates/sim/sim-core/README.md`, "Tilt-rotors"; what is left of it in `PLAN.md`).
- **Boeing 747** with realistic physics, after the drones and before the cars. Aerodynamic data
  from NASA CR-114494 / Hanke, "The Simulation of a Jumbo Jet Transport Aircraft" (1971, public
  domain). Needs: turbofan engines (N1/N2, spool-up, fuel flow, reverse, engine failure), several
  engines per aircraft and asymmetric thrust, compressibility up to Mach 0.9 (drag rise, Mach
  tuck), bogie gear with many wheels (the fixed three-wheel arrays generalised), slats,
  multi-section flaps, spoilers and speed brakes, trimmable stabiliser; later the systems
  (hydraulics, pressurisation, tanks, autothrottle, autobrake, a simple FMS) and its cockpit,
  modelled from a glTF model.
- **Realistic driving game**: tyres, suspension, drivetrain; roads seen from 1 m (seam 6), on the
  same world and the same renderer.

## Flight

- **Time acceleration**, with the autopilot flying.
- **Weather physics**: gusts, wind shear, crosswind, thermals, a low cloud ceiling, rain.
- **Fuel and weight** visible and managed; **engine and instrument failures** in flight (the
  instruments already expose their failures for this).
- ~~**Replay** of a flight~~ (done: F2, `crates/games/flight/README.md`, "The replay") and an
  **in-screen map**.
- **Radio and ATC** with voice.
- **Radio navigation**: VOR, ILS, NDB, flown with the G1000 and the GFC 700: its VOR and LOC
  modes for NAV, the ILS's glide slope for APR, BC, the CRS knob; and flight plans of several
  waypoints with altitudes (FPL, PROC, VNV), where today there is D→ and a GPS approach.
- **Multiplayer** with AI pilots.

## Sky

- More careful **dawn and dusk**, **moon phases**, a darker night, the glow of towns on the night
  sky, cloud shadows and light shafts in the haze, clouds as part of the sky's light under overcast.
