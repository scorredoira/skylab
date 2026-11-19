# What to learn or bring from FlightGear

Read-only study of FlightGear, November 2026. Nothing has been compiled or run.
Repos cloned with `--depth 1` (fgdata in parts) in
`/private/tmp/claude-501/-Users-santi-Documents-projects-flight/29a60362-04a2-4e5a-a2f6-7829c7269f5f/scratchpad/fg/`:

| Short name | Repo | Commit | License |
|---|---|---|---|
| `fg/` | https://gitlab.com/flightgear/flightgear `src/` | 635b247 (13 Nov 2026) | GPL-2.0-or-later (827 files with SPDX; 7 MIT, 1 CC0, all icons) |
| `sg/` | https://gitlab.com/flightgear/simgear `simgear/` | 428f484 | **Mostly LGPL-2.1-or-later** (600 files); 58 GPL-2.0-or-later, among them `environment/metar.cxx` |
| `fgdata/` | https://gitlab.com/flightgear/fgdata | 88c9704 | GPL v2 (LICENSE.md); shaders without their own header, except SMAA and ACES (MIT) |
| `jsb/` | https://github.com/JSBSim-Team/jsbsim | f80b9df | LGPL-2.1 (code, aircraft and engines) |
| `tg/` | https://gitlab.com/flightgear/terragear | eba49f2 | GPL v2 |
| FGAddon | https://svn.code.sf.net/p/flightgear/fgaddon/trunk/Aircraft/ | trunk | Each aircraft its own; almost all GPL v2+ |

Ours: paths starting with `crates/` are in this repo. Read together with `docs/ROADMAP.md`.

**License rule for the whole document:**
- **Idea, equation, number, file format or protocol:** can be used without inheriting a license. Physical equations and facts (a CLα, a 60° threshold) are not protected works.
- **Copying or translating GPL code to Rust** (FlightGear, GPL SimGear, fgdata, YASim): the whole project becomes GPL v2+.
- **Copying or translating LGPL code** (JSBSim, most of SimGear) into a statically linked Rust binary: the user must be allowed to relink with a modified version of that part (LGPL 2.1 §6). In Rust that is awkward; in practice, either it is isolated in a dynamic crate, or it is reimplemented from the equations.
- **Reading GPL data at run time** from a file the user brings (apt.dat, nav.dat, a JSBSim XML): does not spread to the program. **Embedded** with `include_bytes!` or converted and shipped with the game: those files stay GPL and their source must be given; better to avoid it.

## Prioritised summary (value / effort)

| # | What | From | To | Effort | License → how |
|---|---|---|---|---|---|
| 1 | JSBSim Edge 540 as reference for the Extra 300 (phase A); J3Cub for the taildragger gear; variable-pitch propeller | FGAddon `ZivkoEdge/ZivkoEdge540-jsbsim.xml`; `jsb/aircraft/J3Cub`; `jsb/src/models/propulsion/FGPropeller.cpp` | `crates/sim/sim-core/tests/` (new `extra300_reference.rs`), `AircraftType` | S-M | GPL (FGAddon) / LGPL → only numbers in tests |
| 2 | Real instruments: pitot that stalls at 60°, gyros with `spin⁶`, tumble past 45°, compass with magnetic dip | `fg/Systems/{pitot,static,vacuum}.cxx`, `fg/Instrumentation/{attitude_indicator,gyro,heading_indicator_dg,mag_compass,turn_indicator,slip_skid_ball,vertical_speed_indicator}.cxx` | new `crates/sim/sim-core/src/instruments.rs` | M | GPL → reimplement from the physics |
| 3 | Autopilot with real modes: KAP140 (ROL/HDG/NAV/APR/REV, VS/ALT/GS, arm and capture) and GFC700 | `fgdata/Aircraft/Generic/kap140.nas` (1733 lines), `fgdata/Aircraft/Instruments-3d/FG1000/GFC700-{lateral,vertical}-FSM.xml` | mode layer on top of `crates/sim/sim-core/src/autopilot.rs` | M | GPL → the mode table comes from the pilot's guide; FG only as a check |
| 4 | Magnetic declination (WMM) | `sg/magvar/coremag.cxx` | `crates/sim/geodata` (computes), `Weather`/`Terrain` (plain data) | S | LGPL; the WMM model is NOAA's, public domain → reimplement with its coefficients |
| 5 | Real METAR: download and parse | `fg/Environment/realwx_ctrl.cxx:392`, `sg/environment/metar.cxx` (1666 lines) | `crates/sim/geodata` (network and parsing) → `Weather` before the `World` is created | S-M | GPL → reimplement; the NOAA data is public domain |
| 6 | MIL-F-8785C / MIL-HDBK-1797 turbulence with probability-of-exceedance table | `jsb/src/models/atmosphere/FGWinds.cpp:256-360` | `crates/sim/sim-core/src/wind.rs` | M | LGPL → reimplement from the standard |
| 7 | Multirotor: brushless motor (Drela model), APC propellers in CT/CP tables, the F450 as test case | `jsb/src/models/propulsion/FGBrushLessDCMotor.cpp:134-152`, `jsb/aircraft/F450/`, `jsb/engine/DJI_9450.xml`, `jsb/utils/apc_dat_to_jsbsim_xml.py` | `sim-core/vehicles/multirotor` (phase C) | M | LGPL → reimplement (it is 3 equations) |
| 8 | Real airports and navaids with taxiways, lights and signs | `fgdata/Airports/apt.dat.gz` (format 1000), `fgdata/Navaids/{nav,fix,awy}.dat.gz` | `crates/sim/geodata` | M | **GPL v2+ data** → read from the user's disk, don't embed; embedded, OurAirports (public domain) |
| 9 | Integrator check cases (NASA EOM 2015, tumbling brick) | `jsb/check_cases/NASA_EOM_2015/`, https://nescacademy.nasa.gov/flightsim | `crates/sim/sim-core/tests/` | S | US government work → usable |
| 10 | Helicopter rotor (flapping, induced flow, tip losses) for the tiltrotor | `jsb/src/models/propulsion/FGRotor.cpp` (926 lines), `jsb/aircraft/ah1s` | `sim-core/vehicles/` (phase C, after the drone) | L | LGPL → reimplement from Padfield/Bramwell, which it cites |
| 11 | Spectral atmosphere (4 wavelengths) and volumetric aerial perspective | `fgdata/Shaders/HDR/atmos*.glsl`; the shader cites https://github.com/fgarlin/skytracer as its origin, which is MIT | `crates/engine/world-render` | M | fgdata GPL → take from skytracer (MIT) |
| 12 | Raymarched voxel clouds | `fgdata/Shaders/HDR/cloudquad.frag` (638 lines) | `crates/engine/world-render` | L | GPL → ideas only |
| 13 | Telemetry view with names (the good part of the property tree) | `sg/props/props.cxx` | `crates/sim/sim-env` / debugging | S | Idea |

For later or never: FG-compatible multiplayer (section 6), FG AI traffic and ATC (section 7),
TerraSync and TerraGear (section 5.3), YASim (section 1.4).

---

## 1. Flight models: other aircraft

### 1.1 Extra 300: none exists; the Edge 540 is its cousin

- **Extra 300:** there is none in JSBSim (`jsb/aircraft/`) or in FGAddon. FGAddon only has `extra500`, which is the EA-500 turboprop, a different aircraft.
- **Zivko Edge 540** (FGAddon `ZivkoEdge/`, JSBSim, Torsten Dreyer and Ron Jensen, 2008, status "beta"):
  - same type as the Extra 300: single/two-seat aerobatic, tailwheel, IO-540, three-blade variable-pitch propeller;
  - area 98 ft², span 24.3 ft, chord 4.03 ft; Ixx/Iyy/Izz 361/457/742 slug·ft²; empty 1080 lb (`edge.xml:14-44`);
  - 340 hp engine at 2700 rpm, 78 in propeller, 3 blades, pitch 8-45° (`Engines/io540.xml`, `Engines/prop.xml`);
  - **coefficients over ±180° of α**: `CLalpha` reaches 1.40 at 0.22 rad, drops to 0.7 at 0.6 rad and continues as a flat plate up to ±π (`edge.xml:535-566`), symmetric airfoil (CL(−α) = −CL(α)). This is what inverted flight, flat spins and figures need.
  - It has `Cmadot`, `Clr`, `Cnda`, ground effect `kCLge`/`kCDge` and sideslip drag: the same set of derivatives we already check in `tests/jsbsim_reference.rs`.
- **How to use it:** same as the c172p. Read its tables, fly its JSBSim executable loaded like the Extra, and keep the numbers as constants in an `extra300_reference.rs` test. The real Extra 300's numbers (POH: Vs, Vne, roll rate of about 400°/s, limits +10/−10 g) win; the Edge gives what the POH does not: Cnβ, Clp, Cmq, the shape of the stall.
- **Careful:** it is a 2008 model marked "ALPHA" in its header and built from geometry, not from a wind tunnel. Good for orders of magnitude and signs, not as truth. It is a worse reference than the c172p.
- **License:** FGAddon only accepts GPL-compatible licenses and this aircraft declares no other. Using its numbers in tests obliges nothing; copying the XML into the repo does (GPL).

### 1.2 Others worth having as reference

| Model | Where | What for |
|---|---|---|
| J3Cub | `jsb/aircraft/J3Cub/J3Cub.xml:121-183` | Taildragger gear: tail and main `BOGEY` contacts, wing tips as `STRUCTURE`. The Extra is also a taildragger and our gear today is tricycle. |
| pc7, t6texan2 | `jsb/aircraft/pc7`, `jsb/aircraft/t6texan2` | Aerobatic trainers with turbine engine and variable-pitch propeller, heavier. Second opinion for aerobatic derivatives. |
| c172x, c182, pa28 | `jsb/aircraft/` | More GA with data in the same style; the c182 has a variable-pitch propeller. |
| Cap10C | FGAddon `Cap10C/` (JSBSim `cap10-jsbsim`) | Two-seat aerobatic, slower. |
| Pitts S1C, Yak-52, Zlin 50 | FGAddon | **YASim**, not JSBSim: their coefficients don't exist as a table, they come out of the solver. Useless as numerical reference. |

**Variable-pitch propeller** (missing for the Extra; the C172 is fixed pitch): `jsb/src/models/propulsion/FGPropeller.cpp` (494 lines, LGPL) models the governor. Reimplement the idea: the pitch moves to hold the requested rpm, limited to `minpitch..maxpitch`, with CT and CP as functions of J and pitch (2D tables, like `propC10v.xml`). Where: the engine/propeller of `AircraftType`. Effort S-M.

### 1.3 Multirotor (phase C, FPV drone)

- **F450** (`jsb/aircraft/F450/`, 794 lines in 9 files): 450 mm DJI quadcopter with 4 `DJI_E305` motors and `DJI_9450` propellers pointed at 90° pitch, alternating directions (`Propulsion.xml`), mixer in `Effectors.xml`, control in `FlightControl.xml`, IMU, baro and GPS sensors.
- **Brushless motor** (`FGBrushLessDCMotor.cpp:134-152`), Mark Drela's first-order model (MIT, public document):
  `I = (V − rpm/Kv) / R`, `τ = (I − I0) / Kv`, with `V = Vmax · throttle`. E305 data: Kv 960, R 0.117 Ω, I0 0.45 A, 14.63 V.
- **Propeller:** CT and CP as functions of J taken from APC data (`DJI_9450.xml`). `jsb/utils/apc_dat_to_jsbsim_xml.py` converts APC files: that is a source for small propellers. The terms of use of the APC data have not been checked.
- **Watch out:** the F450 is a slow photography drone, not a racing FPV (5", ~1.8 kW/kg). It serves to validate the method (motor + propeller + mixer + PID stabilisation), not the feel of an FPV. For that we need data for 2207/2306 motors and 5" propellers, and the effect of axial and oblique flow on the propeller (JSBSim ignores it at 90°).
- **Where:** `sim-core/vehicles/multirotor`, three equations and one table per propeller. Effort M. Reimplement.

### 1.4 Helicopter and tiltrotor

- **JSBSim `FGRotor`** (926 lines, LGPL): rotor with diameter, blades, chord, twist, hinge offset, flapping moment, induced-flow lag, tip loss, transmission (`FGRotor.h:52-75`). Based on Bramwell (ARC R&M 3104) and Padfield, cited in `jsb/aircraft/ah1s/ah1s.xml:39`. `ah1s` (AH-1S Cobra) is the full case, with tail rotor.
- **YASim** (`fg/FDM/YASim/Rotor.cpp`, `Rotorpart.cpp`, GPL): rotor split into elements; flown by FGAddon's R22, bo105, ec135, UH-1 and Alouette. No coefficients to check against.
- **For the Avatar-style tiltrotor:** a tilting rotor is an `FGRotor` whose axis changes over time, plus the wing. Effort L. Reimplement from Padfield; JSBSim as numerical check in hover (induced power, torque).

### 1.5 YASim: the idea behind its solver

YASim (`fg/FDM/YASim/Airplane.cpp`, GPL, Andy Ross 2001) describes the aircraft by geometry (wings as surfaces, like our strips) and two POH points: cruise and approach. An iterative solver (`solveAirplane`, `Airplane.hpp:206`) adjusts drag, lift and tail incidence until it hits them. It is the same thing we do by hand with the manual tests. **Useful idea for `AircraftType`:** a test or tool that tunes 2-3 free parameters of a new type until it meets the POH. Don't copy code. Its physics is worse than ours: no tables over α, simplified stall.

### 1.6 NASA check cases (S)

`jsb/check_cases/NASA_EOM_2015/` has the brick (`aircraft/brick`) and an atmosphere case. The full set (NASA TM-2015-218675, https://nescacademy.nasa.gov/flightsim) has a falling sphere, a tumbling brick without and with damping, and more, with reference time series from six simulators. The brick cases test the Euler equations and the integrator without depending on the Earth model: they go into a `sim-core` test against our RK4. US government work.

---

## 2. Systems and instruments

### 2.1 What FlightGear has (C++ code, GPL)

The instruments are subsystems that read and write the property tree. David Megginson's originals (2002) were public domain, but the current versions carry changes under GPL (header of `altimeter.cxx:1-4`): treat them as GPL.

| Part | File | What it models that is worth having |
|---|---|---|
| Pitot | `fg/Systems/pitot.cxx:60-90` | Total pressure projected by `cos α · |cos β|`; **tube stall** when that projection drops below cos 60° (total = static); Rayleigh with shock wave above Mach 1. The pitot stall at 60° matters in aerobatics and spins. |
| Static, vacuum | `fg/Systems/static.cxx:70-115`, `vacuum.cxx:79-90` | Static with lag (τ) and **sideslip error** (port on the fuselage) or by α and β (port on the pitot): adds a fraction of the perpendicular impact pressure. Vacuum from the rpm of the fastest pump. |
| Gyro | `fg/Instrumentation/gyro.cxx` (96 lines) | Normalised spin that spins up and down with time constants depending on power (suction/minimum). |
| Attitude | `attitude_indicator.cxx:130-200` | `spin⁶` response: with a slow gyro, it barely moves. **Tumble:** past ±45° of bank it builds up quadratic error; re-erects in 300 s (1 s caged). |
| Heading | `heading_indicator_dg.cxx:47-55` | Electric gyro (spins up in 4 s, stops in 180 s); error proportional to yaw rate and g load (factors 0.033); heading bug. |
| Compass | `mag_compass.cxx:100-239` | Heading with magnetic dip, bank and pitch (Ed Williams' formula); the float tilts ~4° per longitudinal or lateral g; fluid damping; deviation table. |
| Turn coordinator, ball, VSI | `turn_indicator.cxx`, `slip_skid_ball.cxx`, `vertical_speed_indicator.cxx` | Electric turn coordinator with gyro; ball from lateral acceleration; VSI with calibrated leak from the static. |
| Radios | `navradio.cxx` (967), `adf.cxx`, `dme.cxx`, `marker_beacon.cxx`, `kr_87.cxx`, `gps.cxx` (1617) | Range by standard service volume (terminal, low and high, `navradio.cxx:268-300`, tables `fgdata/Navaids/range.*`: height → NM), × 1.3 over the published one; marker beacons. |

**What FG adds:** pitot stall by angle, attitude indicator tumble and the compass with the full dip formula. **Plan:** a single new `instruments.rs`, from the physics. Effort M.

### 2.2 Electrical and fuel of FG's c172p

- FlightGear's c172p (c172p-team, GPL v2): `Systems/electrical.nas` (procedural, in Nasal) and `electrical.xml` (LED logic), `fuel.xml` (tanks and selector), `c172p-damage.xml` (springs and dampers per contact; damage by compression speed).
- The battery itself says "This function is totally wrong" about the available amps (`electrical.nas:105`). Model: 24 V, 13.36 Ah, linear charge. **Worse than the POH; don't bring it.** The 172S POH and maintenance manual are the source.
- **What is worth having:** the structure of two avionics buses with split master switch (BAT/ALT), and the LEFT/RIGHT/BOTH fuel selector with flow per tank. As a list of what to model, not as code.

### 2.3 KAP140 and GFC700 autopilot (M)

- **KAP140** (`fgdata/Aircraft/Generic/kap140.nas`, Roy Vegard Ovesen; 2021 tuning by Josh Davidson in the c172p's `kap140-autopilot.xml`):
  - lateral modes `OFF, ROL, HDG, NAV, REV, APR`; vertical `OFF, VS, ALT, GS`; armed `NAV, APR, REV` and `ALT, GS` (`kap140.nas:40-43`);
  - works in **pressure**, not altitude: it holds a pressure rate of change (`target-pressure-rate`) and the preselected altitude as pressure, like the real unit (it reads the encoder, not the altimeter);
  - intercept angle by hand-made PID, with integrator only after capture (`kap140-autopilot.xml:156-245`);
  - power check at start, auto trim only in pitch, altitude alert.
- **GFC700** (`fgdata/Aircraft/Instruments-3d/FG1000/GFC700-{lateral,vertical,AP-off}-FSM.xml`, 1228 lines): declarative state machines. Lateral `ROL, HDG, NAV-VOR/LOC/GPS, GA` with arm and capture (`VOR-ARM → VOR`, `LOC-ARM → LOC`); vertical `PIT, VS, FLC, ALT, ALTS` (capture of the preselected one), `VNV, GS, BC, GA`, and the flight director separate from the servo.
- **Ours:** `crates/sim/sim-core/src/autopilot.rs` (1392 lines) is a state machine (`Phase`) that flies whole goals (takeoff, pattern, landing) on cascaded loops. It is a pilot, not a panel unit.
- **Proposal:** a `Kap140` layer (and later `Gfc700`) that is a unit: buttons, annunciators, armed and active modes, and that only moves roll and pitch (servos with torque and rate limits), like the real one. Underneath it reuses our loops. The modes and transitions are written from the Bendix/King and Garmin pilot's guides (they are the real data); the FG files serve to check odd cases (what happens when APR is pressed without a signal, when GS goes from armed to active). It first needs `nav.rs` and the altitude encoder (2.1).
- **License:** fgdata is GPL; the mode table is a fact about the unit and inherits nothing.

### 2.4 Magnetic declination (S)

`sg/magvar/coremag.cxx` (474 lines, LGPL) implements the World Magnetic Model. The model and its coefficients are NOAA/NGA's, public domain. **Reimplement** in `geodata` (about 200 lines with the WMM2025 coefficients) and give `sim-core` the region's declination and dip as plain data. The compass (2.1), the VORs and real runway headings need it. In a procedural world: constant per seed.

---

## 3. Real weather: METAR and turbulence

### 3.1 METAR (S-M)

- **Source:** FG downloads `https://tgftp.nws.noaa.gov/data/observations/metar/stations/[station].TXT` (`fg/Environment/realwx_ctrl.cxx:392`). There is a list of stations with METAR in `fgdata/Airports/metar.dat.gz`. NWS data is public domain.
- **Parser:** `sg/environment/metar.cxx` (1666 lines) is GPL, not LGPL like the rest of SimGear. A METAR parser in Rust is 300-400 lines: wind and gusts, variable direction, visibility, cloud layers (FEW/SCT/BKN/OVC with base), present weather, temperature and dew point, QNH (Q/A), trends that are ignored.
- **How it reaches the simulation** (`fg/Environment/fgmetar.cxx`, `metarproperties.cxx`, `environment_ctrl.cxx`): FG interpolates between nearby stations and builds wind layers with height. For us, same as real terrain: `geodata` downloads and parses the METAR **before** the `World` is created, and `Weather` receives it as data (wind at 10 m, gusts, QNH, temperature, layers). `sim-core` stays without network and deterministic. A weather change in flight would come in as explicit input to `step`, like the controls.
- **ATIS:** `fg/ATC/ATISEncoder.cxx` (566 lines) turns the METAR into the ATIS text. The idea is useful for the radio phase; the text is written new, in our own words.

### 3.2 Turbulence (M)

- `jsb/src/models/atmosphere/FGWinds.cpp` (647 lines, LGPL) has the `ttMilspec` and `ttTustin` models (Dryden spectrum, MIL-F-8785C / MIL-HDBK-1797): σ at low altitude from the wind at 20 ft; above 2000 ft, a **probability of exceedance** table (7 levels × 12 altitudes, `FGWinds.cpp:93-110`, `:267-294`) that gives σ for "light, moderate, severe"; linear interpolation between 1000 and 2000 ft; rotational gusts p, q, r.
- **For us:** the exceedance table is what lets us ask for "moderate turbulence at 8000 ft" with numbers from the standard. Reimplement from MIL-HDBK-1797 (public) with our `Rng`; JSBSim as a check of σ and scales.

---

## 4. Real scenery: airport and navaid data

### 4.1 What there is

| File | Contents | License |
|---|---|---|
| `fgdata/Airports/apt.dat.gz` (26 MB) | X-Plane 1000 format, cycle 2013.10 "modified by the FlightGear team": runways (`100`), taxiways as polygons with Bézier curves, signs, lights, PAPI, ramps, taxi network for the AI | GPL v2+ (file header; © Robin Peel) |
| `fgdata/Navaids/nav.dat.gz` | VOR, NDB, ILS (LOC, GS), DME, markers, with frequency, range, elevation, deviation | GPL v2+ (header, cycle 2013.10) |
| `fgdata/Navaids/fix.dat.gz`, `awy.dat.gz` | Fixes and airways | GPL v2+ |
| Parser | `fg/Airports/apt_loader.cxx`, `fg/Navaids/navdb.cxx` | GPL → reimplement; the formats are documented by X-Plane |

**Compared with OurAirports** (public domain):
- OurAirports gives runways (thresholds, width, surface, lights yes/no) and navaids, without taxi geometry.
- apt.dat gives **the real shape of the airport**: taxiways, aprons, markings, signs, lights by type. That is what makes a real airport recognisable from the cockpit.
- FG's data is from 2013. The X-Plane Scenery Gateway publishes an updated apt.dat, also GPL v2+ (not checked in this session).

**Proposal:**
1. Embedded in the game: OurAirports (runways and navaids).
2. Optional: if the user has an apt.dat (from FlightGear, X-Plane or downloaded themselves), `geodata` reads it from disk and draws the taxiways. The program does not become GPL by reading it. It is not shipped with the game.
3. Where: parser in `crates/sim/geodata`; `TerrainData` gains pavement polygons; its mesh in `world-render`. Effort M (parser S, pavement mesh with Bézier M).

### 4.2 What nav.dat has that our procedural `nav.rs` should imitate

The **range** field by class (and the service volumes of `fgdata/Navaids/range.*`), the magnetic deviation per station (VORs are aligned with an old declination), the glide slope height and angle per runway. When placing procedural navaids it pays to use these same columns, so a real region and a procedural one produce the same `Navaid`.

---

## 5. Terrain and scenery

### 5.1 TerraSync

- `sg/scene/tsync/terrasync.cxx`: HTTP client that downloads the scenery in 1°×1° tiles as you fly, with servers chosen by DNS (`dnsSelectServerForService`, `:478`).
- The scenery is precomputed meshes in BTG format (`sg/io/sg_binobj.cxx`, LGPL) with material per triangle, plus objects (`.stg`). WS2.0 was made with TerraGear; WS3.0 (in development since 2019) uses VirtualPlanetBuilder: elevation mesh with level of detail and land class texture.
- **Scenery license:** FlightGear says it is built from public, GPL-compatible geographic data. The exact license of the tiles was not checked in this session (wiki.flightgear.org blocks automated downloads). Check it before using a single tile.
- **For us:** no. It is a world of tens of GB, pre-baked for its engine (OSG), with meshes that don't fit our `Terrain` nor the rule that `sim-core` does not wait on the network. Our route (Terrarium tiles + OpenFreeMap, build the region before the `World`) is better and lighter.

### 5.2 What is worth having

- **CORINE land classes** (Copernicus, attribution): WS3.0 starts from CORINE and reclassifies it to its materials. For Europe, CORINE gives forest, crops, urban, water, rock at higher quality than OSM's `landcover`. Idea for the real places' land cover.
- **Airport elevation** in TerraGear (`tg/src/Airports/GenAirports/elevations.cxx`): a single mean height of all the airport's points. Cruder than a slope per runway, which is what we do.

### 5.3 TerraGear

`tg/` (GPL v2, GDAL + CGAL): command-line tools that build BTG from SRTM, CORINE, VMap0 and apt.dat. It adds nothing `geodata` does not give live.

---

## 6. Multiplayer

### 6.1 FG's protocol

- `fg/MultiPlayer/mpmessages.hxx`, `multiplaymgr.cxx` (2849 lines), GPL.
- UDP, packets up to 1200 bytes (`multiplaymgr.cxx:45`), XDR encoding, port 5000 by default.
- Header: magic `"FGFS"`, version 1.1, type, length, requested range in NM, reply port, 8-character callsign.
- Position: model name (96 characters), time, lag, ECEF position in `f64`, orientation as axis-angle, linear and angular velocities and accelerations in ECEF (`mpmessages.hxx`, `T_PositionMsg`). After that, properties by numeric id (surfaces, lights, engine, chat), each aircraft its own.
- `fgms` servers that only relay; each client simulates its own aircraft and extrapolates the others.

### 6.2 Why we don't want it as a base

- **There is no authority.** Each client owns its aircraft; there are no collisions between players and no real shared state. Our `World` is already the single, deterministic authority (ARCHITECTURE.md): the natural server is a `World` that receives `Controls` and emits `Observation`s, like `sim-env`.
- Per-aircraft properties mean every new model has to agree on ids with the rest.
- Extrapolation without clock correction (fixed `lag` per packet, the code itself marks it FIXME).

### 6.3 What can be done, later

- **Bridge to the FG network:** a separate process that publishes our aircraft on FG's servers (and on their map) and brings theirs in as traffic. Reimplementing the wire format from the header, without copying code: a protocol format is not a protected work. Few users, low value. Effort M.
- **Ideas for ours:** ask the server for a range (you are only sent what is near) and send velocities and accelerations to extrapolate between packets.

---

## 7. AI traffic and ATC

- **AI traffic** (`fg/AIModel/AIAircraft.cxx`, `fg/Traffic/`, `fgdata/AI/Traffic/` with 233 airline schedules, `fgdata/AI/Aircraft/performancedb.xml`): the AI aircraft are **kinematic**. They follow a flight plan with speeds and climb rate from a per-type table; they have no physics and no pilot. Our AI pilots fly real physics in the same `World`. **Don't bring it.** What is useful is the schedules, as an idea of "what traffic there is at this airport at this time".
- **ATC** (`fg/ATC/`): startup, ground, tower and approach controllers for the AI, with the apt.dat taxi network (`fg/Airports/groundnetwork.cxx`). For the human there is hardly anything: in practice the community uses human controllers over the MP network (OpenRadar, ATC-pie) and voice over FGCom. What can be used:
  - the taxi network as a graph of nodes and segments with direction rules (apt.dat format 1201/1202), so the AI and ATC know how to taxi;
  - the `ATISEncoder` (3.1).
- **For the ROADMAP's "radio and ATC" phase:** our own ATC that knows the whole `World` (it has it, it is the authority) can do what FG's does not: separate the AI and the human in the pattern with the same data.

---

## 8. Architecture: what to copy and what to avoid

### 8.1 Property tree and subsystems

- **Property tree** (`sg/props/props.cxx`, 4530 lines, LGPL): a global tree of named values (`/instrumentation/airspeed-indicator/indicated-speed-kt`) that everything reads and writes, with "listeners" and bound to Nasal, XML, network, HTTP, telnet and the UI.
- **Subsystem manager** (`sg/structure/subsystem_mgr.hxx:498-505`): groups in fixed order `INIT, GENERAL, FDM, POST_FDM, DISPLAY, SOUND`; each subsystem declares dependencies; the FDM runs at its own rate, several times per frame.

**Copy:**
- **Public, stable names for the observable state.** A flat, read-only view of the `Observation` with named paths (`aircraft/0/airspeed_kt`) is useful for debugging, for `sim-env`, for recording flights and for a future web panel, without anyone writing to it.
- **The fixed order by phases** within the step: physics → systems → instruments → pilots. We already do it implicitly in `World::step`; write it down when `systems.rs` and `instruments.rs` come in.

**Avoid:**
- **Mutable global state with text keys.** Anything can write to anything; a typo silently creates a new property; the order of writes decides the result; there is no guaranteed determinism. It is the opposite of rule 2 in ARCHITECTURE.md.
- **Aircraft logic in scripts and XML** (Nasal, XML filters). The KAP140 is 1733 lines of Nasal plus 600 of XML for what in typed Rust is a mode `enum` and a few loops. The c172p's electrical system is in Nasal and says itself that it is wrong. Hard to test: there are no tests for these systems.
- **Two (or three) FDMs** at once (JSBSim, YASim, UFO): each aircraft behaves according to who made it. Us: one physics, several `AircraftType`s.

### 8.2 Other things they do worse or we don't want

- **Uneven aircraft quality:** each is made by a volunteer; many are "alpha" or "beta". The c172p is good; most aerobatic ones are YASim without data to check against.
- **Spherical WGS84 world everywhere** (`sg/math/SGGeodesy.cxx`): needed to fly the whole planet, but forces ECEF in every calculation. Our projected plane per region is enough for 100-200 km and keeps the physics simple.
- **Slow startup and megabytes of data** (fgdata at several GB, scenery separate).
- **Rendering on OpenSceneGraph** with a lot of legacy code: three paths (classic, ALS, HDR). The new HDR one is good; the rest is not.

---

## 9. Rendering

FlightGear's current branch has a new HDR path (`fgdata/Compositor/HDR/`, `fgdata/Shaders/HDR/`): deferred G-buffer, clustered lights (`clustered.glsl`), cascaded shadows, GTAO, SMAA and FXAA, automatic exposure by histogram (`histogram*.frag`, `exposure.glsl`), bloom, ACES, physically scattered sky and voxel clouds. No shader carries a license header except SMAA and ACES (MIT), so they are GPL v2 like fgdata.

| Part | Where | Compared with ours | What to do |
|---|---|---|---|
| Spectral atmosphere | `atmos.glsl` (362), `atmos_transmittance.frag`, `atmos_multiple_scattering.frag`, `atmos_sky_view.frag`, `atmos_aerial_perspective*.{comp,frag}`; aerosols with cross sections | Ours: Hillaire 2020 in RGB. FG: 4 wavelengths (630, 560, 490, 430 nm) converted to sRGB with a matrix (`atmos_spectral.glsl`), aerial perspective in a froxel volume | Keep ours, or **skytracer**, the tracer the shader itself cites as its origin (`atmos_spectral.glsl:13`, https://github.com/fgarlin/skytracer, **MIT**). Careful: skytracer is a reference path tracer, not the real-time shaders; those come from the `msXXDS` shadertoy cited in `atmos.glsl:5`, whose license has not been checked. Spectral rendering fixes the colour of sunrises and sunsets; low cost once Hillaire is done |
| Histogram exposure | `histogram_*.frag`, `exposure.glsl` | Ours uses manual exposure | Idea for the `engine` post pass |
| Voxel clouds | `cloudquad.frag` (638 lines): voxel field with coarse and fine density in 3D textures, precomputed shading, 3D noise, wind displacement, field that repeats and mirrors to cover the horizon | Ours: volumetric clouds | Read as design; write our own. The idea of the mirrored repeating field is cheap and avoids the edge |
| Water | `water.frag` (72), `shading_water.frag` (101), `ws30_water.glsl` (74) | Simpler than ours (Bruneton, Fresnel, absorption, breakers) | Don't bring it |
| Runway lights | `terrain_runway.frag` + apt.dat lights | Photometric lights with PAPI are better | Our own |

---

## 10. License recommendation for the project

**What we would lose without GPL code from FlightGear/SimGear:** almost nothing.
- What is valuable in FlightGear for us is **data, equations and designs**: Edge 540 and c172p coefficients as reference in tests, KAP140 and GFC700 modes (facts about the unit, better from their guides), instrument formulas (public domain at origin: Ed Williams, NOAA WMM, MIL-HDBK-1797, Drela, Padfield), the apt.dat, nav.dat and METAR formats and the MP protocol. None of that is inherited when reimplemented.
- The code itself does not fit: C++ with OSG, Nasal and XML on a global property tree, the opposite of our pure, deterministic `sim-core`. Translating it would cost more than writing it from the physics, and our physics is already better than YASim's and checked against JSBSim.
- The best of their rendering, the atmosphere, comes from an MIT project the shader cites (skytracer), and we already cover HDR, atmosphere, water and lights.

**JSBSim (LGPL 2.1):** carry on as before. Running its executable as an oracle outside the game and copying numbers into tests obliges nothing. Don't translate its code to Rust: in a static binary, LGPL requires allowing relinking, which is awkward in Rust. Models we want to ship as they are (a JSBSim XML) go as a separate file under LGPL.

**GPL data (apt.dat, nav.dat, FGAddon aircraft):** don't embed them. Read them from disk if the user has them. Embedded, OurAirports (public domain).

**Recommendation: copy no code from FlightGear or SimGear, and publish under a permissive license.**
- **MIT or Apache-2.0, or both** (the usual in Rust: `MIT OR Apache-2.0`). Apache-2.0 adds an explicit patent license; MIT code comes in without friction under either (keeping its notice).
- GPL v2+ would only make sense if we really wanted to copy YASim or FG's instruments as they are, and it does not pay: it would close the door to `engine` and `world-render` being reused in other games (the ROADMAP idea) and to mixing with MIT/Apache from the Rust ecosystem, in exchange for code that would have to be rewritten anyway.
- **Discipline to keep it clean:** in every piece inspired by FG, cite the physical source or the manual in the comment (not the FG file), and don't open the `.cxx` while writing the equivalent `.rs`; if a number from FG is used, cite it as data with its origin, as `tests/jsbsim_reference.rs` already does.
- **Credits:** add "Reference data: JSBSim (LGPL), FlightGear/FGAddon" when their numbers are used, even if not required.

The licenses of the external data (TerraSync scenery, APC data, X-Plane Gateway) were not checked on their pages during this study: review them before using them.
