# flight

The game: it draws the world of `sim-core` with `world-render` and its aircraft, cockpits and
multirotors into it with `engine`, turns the keyboard, the mouse and game controllers into flight controls,
and puts the instruments, texts, settings and sound on top. Texts shown to the player are in English.

Run it: `cargo run -p flight` (window, 1440×900) or `cargo run -q -p flight -- --shot <dir>`.

## Modules

| To change | Read |
|---|---|
| Keys, mouse, pause, restarts, cameras, status line, messages, help text | `src/game.rs` |
| Aircraft flown by trained models (`--model`): starts, referee, restarts | `src/ai.rs` |
| Gamepads and flight sticks: defaults, shaping, the mapping file, `--controllers` | `src/controls.rs` |
| Command line, the `--shot` procedure | `src/main.rs` |
| The game flown by another program (`--remote`): the controls it sets, the state it reads, its orders | `src/remote.rs`; the socket, the protocol and `skylab-ctl` are `crates/games/remote` (its `README.md`, `PROTOCOL.md`) |
| How the world looks (sky, terrain, scenery, water, clouds, fire, quality, photographs) | `crates/engine/world-render/README.md`; its viewer draws it alone |
| How the aircraft are drawn into it: parts, lights, the propeller's blur, their shadows, the cockpit's pipelines | `src/render/mod.rs`, "Drawing" below |
| The aircraft parked at the aerodromes | `kinds::parked_aircraft`, `models::parked` |
| The types of aircraft read at start from `assets/aircraft` (else the built-in ones) | `src/fleet.rs`; the files' format is `crates/sim/sim-core/README.md`, "Aircraft files" |
| What differs per type of aircraft: the look its file names (its model's and cockpit's files, read once and checked at the start), and from its numbers the panel marks, engine sound and autopilot speeds | `src/kinds.rs`, the one door; nothing matches on a kind |
| The aircraft's look: `assets/aircraft/<model>/exterior.glb` and `.ron` (mesh, moving parts, lights, shadow), `<cockpit>/cockpit.glb` and `.ron` (cabin, spots, instruments, print, levers, clicks) | `src/models.rs`, `src/cockpit/file.rs`, "The aircraft" below; glTF is `engine::gltf`; `examples/models.rs` draws a model alone |
| The C172's outside, from a model made in a modelling program: `assets/aircraft/c172/source/` (`source.glb`, `import.py`, run by Blender: `Blender -b --python assets/aircraft/c172/source/import.py`) | its pieces turned into the nodes `exterior.ron` moves, in body axes, the wheels where `aircraft.ron` has them, the propeller squared to its shaft; its materials and pictures as the game draws them: the white paint cleaned of the picture's smears (the cowling smooth, its seams and fasteners drawn again straight), a normal map of its panel lines and rivets, ambient occlusion baked with Cycles, the livery (cheatline, fin, registration) painted on in the aircraft's axes and the registration again as a decal of its own at a millimetre a texel, the wheels' fairings glossier, the spinner polished metal, the windows a light tint, the inside's pictures at 512 |
| Wrecks: the aircraft cut into its pieces (the world draws them where they lie, and the fire); each part sorted by piece, to draw what a tear left | `src/render/wreck.rs` |
| The row of instruments of the outside view (centred in what the yoke box leaves at the bottom, smaller in narrow windows), with throttle, flaps, trim and the stall lamp in one round dial and, for the Extra, the round accelerometer | `src/ui/panel.rs` (`examples/panel.rs` draws them and the G1000's pictures) |
| The instruments' knobs and switches at the keyboard (altimeter setting, QNH, heading gyro, pitot heat, alternate static) and the failures tried by hand | `src/instrument_keys.rs`; their physics is `sim_core::instruments` |
| The C172's autopilot (GFC 700) at the keyboard, a controller and the mouse: which key is which (`controls::panel_key`), the bezel's keys and knobs the mouse presses and turns (`cockpit::PanelControl`, the `controls` of its `cockpit.ron`), what each does (`Session::panel`), the knobs drawn turning (`cockpit::KnobTurns`) and the tag that names what the pointer is over (`Session::panel_tag`, `hud::draw_pointed`); its physics is `sim_core::gfc700` | `src/controls.rs`, `src/game.rs`, "The C172's autopilot" below |
| The cockpits: how they are drawn, the instruments a file can fit (the G1000's displays in `src/g1000/`), the print, the levers the mouse takes | `src/cockpit/`, "The cockpits" below |
| Status text, message, help column, yoke box and pedal bar | `src/ui/hud.rs` |
| The settings: what each is, its units and limits, the kinds of day, the settings file | `src/settings.rs` |
| The menus: the first screen (planes, drones or cars), the start and pause screen, the settings' panel, the aircraft's pictures; their look (fonts, icons, colours), the keyboard's and the pad's cursor | `src/ui/menu/` (`vehicles.rs`, `home.rs`, `side.rs`, `hangar.rs`, `theme.rs`, `mod.rs`), "The menus" below |
| The replay (F2): what is kept of the last minutes, the playback, the last landing measured | `src/replay.rs`; its bar and debrief `src/ui/replay.rs`; "The replay" below |
| The manual (F1): its pages, the pictures and the marks over them, the keys and joystick controls it names | `src/manual/`, "The manual" below |
| Real places: loading a region before the window or in the background, the places offered, the maps' credits | `src/places.rs`; the data is `geodata`'s (`crates/sim/geodata/README.md`) |
| The place picker ("Choose place", key M): search, the map, the lists, the start chosen | `src/picker/` (`mod.rs` the screen, `map.rs` the view and the tiles kept, `tiles.rs` a vector tile into what is drawn and the map's style); "Real places" below |
| Engine (four cylinders or six), wind, wheels and stall-horn sound; a multirotor's propellers and motors | `src/audio.rs` |
| What the game does for any multirotor: its FPV camera, the chase camera's size, its keys and sticks, its messages, what its display shows | `src/multirotor.rs`, "The FPV quad" below |
| A multirotor's on-screen display (OSD), and a race's over it | `src/ui/osd.rs` |
| Race courses: the courses read at start, the race flown (countdown, laps, splits, best lap, chimes), its autopilot | `src/race.rs`, "Race courses" below; the courses themselves are `sim_core::course` |
| How the race gates are drawn: their frames, LED strips, numbers and the start's pad | `src/render/course.rs` |
| The race gates' models (`assets/courses/gates.glb`), made from the shapes the simulation hits | `examples/gate_models.rs` |
| The 5-inch quads' models (`assets/aircraft/fpv5/exterior.glb`, `race5/exterior.glb`), made from shapes | `examples/quad_model.rs` |
| The Kestrel tilt-rotor's model and cockpit (`assets/aircraft/kestrel/exterior.glb`, `cockpit.glb`), made in Blender | `scripts/blender/kestrel.py` (with `shapes.py`); `Blender -b --python scripts/blender/kestrel.py` from the repository's root |
| The C172's cockpit (`assets/aircraft/c172/cockpit.glb`): the G1000 panel made for the game and glare shield (`source/panel.glb`) inside the model's lining, seats and glass (`source/source.glb`) | `assets/aircraft/c172/source/cockpit.py`; `Blender -b --python assets/aircraft/c172/source/cockpit.py` from the repository's root |
| The knobs of a cockpit's panel that turn (the C172's HDG, ALT and CRS / BARO), made parts of their own in its `cockpit.glb` | `examples/panel_knobs.rs` | (the C172's are kept in `source/panel.glb`)

`game.rs` has two layers. `Session` needs no window: the `World`, the player's `KeyboardPilot`, the
view, the texts; its tests press keys for seconds of simulated time. `Flight` implements
`engine::Game` around it: reads `Input`, calls `Session`, builds the `render::Scene`, draws the overlay.

After a crash the world keeps running but nobody flies the player's aircraft; once it is in pieces
the camera stands 30 m back from the wreck (`Session::player_wreck`) and 1, 2 or 3 start again (the
wreck stays). What breaks, overload, Vne and flap speed are said in the middle of the screen. Damage
that leaves it flying (a wing tip, an aileron, half the tailplane) does not end the flight: the
aircraft is flown as it is, rolling towards what it lost.

Each step is `world.fly(id, pilot)` for every entry of `Session::planes` (an aircraft and who flies
it), then `world.step()`, at `World::DT`, at most 0.1 s of real time per frame.

Time can go faster (`Session::time_rate`, F9 and F10, `TIME_RATES` ×1 ×2 ×4 ×8 ×16, the `time`
order of `--remote`): the steps stay `World::DT` long and there are that many times more of them a
frame, so the physics, the GFC 700, the traffic, the races, the weather's gusts and the replay's
recording are the same flight step for step, only sooner. The steps real time asks for are always
taken; the ones beyond them only while they fit in `step_budget` (10 ms of the frame), and the rest
are let go of: the world then goes slower than chosen and the tag at the top right says both (`×16
→ ×9.4`; plain `×4` while it keeps up; `Session::time_text`, the rate smoothed over about a second).
Back to ×1 at a touchdown and at a crash (inside the frame: the steps after it are let go of), on a
pause (Esc, the menu, the picker, the manual, the aircraft), a replay and another place; not faster
while the aircraft is in pieces or paused. The texts in the middle of the screen are timed in real
seconds (`Session::clock`), the propellers and the wheels turn at the world's rate, the sound
follows the engine as it is. The hour of the day stays where it is set (T moves it).

Throttle, flaps and trim are
written straight to the player's `aircraft.controls`; the pilot only moves yoke and pedals. While
`Session::autopilot` is `Some`, that `sim_core::Autopilot` flies the player's aircraft instead and
works every control. An aircraft with a GFC 700 (`AircraftType::afcs`, the C172) has it in `Session::gfc`,
behind whoever flies (`Hands`: the keyboard's pilot or the game's autopilot set the controls, then
the GFC 700 sees them and, engaged, its servos take the ailerons, elevator and trim). `Session::add_traffic(n)` adds aircraft with `Flier::Traffic`, each with an
autopilot flying circuits for ever: every other one at the home airport (they start in the air or on
a long final, never on the runway), the rest on the runways of the nearest airports.
`Session::add_model(spec)` adds one with `Flier::Model`, flown by a `sim_env::ModelPilot` read
from a model file (`--model <file>[:<task>]`, see `trainer/README.md`). A `.pattern` file instead (written by
`sim-env`'s `race search`) is the autopilot flying the `race` with that `Pattern`. Its start depends on the
task, at the home airport the models are trained at and into the wind: `takeoff` (and `circuit`,
`race`) on the runway, at the other end if another aircraft stands at this one; `landing` on final
2.5 km out; `hold` in the air 2 km to the left of the runway, holding the heading and altitude it
starts with at 95 kt. A `sim_env::Referee` judges the flight as the training environment
would; when its episode ends (success, crash, out of the envelope, or the time limit of a runway
task) the aircraft starts again `ai::RESTART_DELAY` (4 s) later. A `hold` just flies on past its
time limit. Models count against the six aircraft of the traffic. With more than one aircraft the
status line says which one the camera is on and who flies it (you, the autopilot, or the model
and its last ending, and for the `race` the seconds flown and the time of the last success).

## The aircraft

Each type is a file, `assets/aircraft/<key>/aircraft.ron`: the Cessna 172 and the Extra 300L (a
Boeing 747 is on its way in `wip/b747`). At
start `fleet::load` reads every such file it finds (`$FLIGHT_ASSETS/aircraft`, else `assets/aircraft`
beside the executable or the current folder, or above them), each in place of the built-in type of
its key or added after them, and stops with the file's mistake if one is wrong; without the folder,
the types built in when the game was compiled. A file's `visual` names the folders of
`assets/aircraft` whose files it is drawn with: `model` the one with `exterior.glb` and
`exterior.ron`, `cockpit` the one with `cockpit.glb` and `cockpit.ron` (a type may name another's
look). `kinds::check` reads the RON files at the start and refuses a type whose files are missing or
wrong. **A new aircraft is only files**: its `aircraft.ron` and its look in the same folder; no
Rust (and, for the menu, a `card.ron` with what to say of it and a `picture.jpg`: "The menus").
The player picks one from the menu (Esc, "Aircraft": their pictures; it starts again on the runway) or
with `--aircraft` (its key or an alias); traffic is C172s. `kinds.rs` reads each look once and says
the rest that differs. The renderer keeps every type's mesh, parts, lights and wreck, by the place of
the type in the fleet (`AircraftKind::all`), and draws each aircraft with its own
(`PlaneVisual::kind`). Every aircraft near casts its shadow from its mesh as it is drawn (the world's
map of the vehicles' shadows: its surfaces, its gear on its springs, its propeller standing; also
what is not drawn from the cockpit, the cabin's outside); far below the player's (in the air) and on
the lowest quality its shadow has the shape its `exterior.ron` gives (`kinds::shadow`), its wing as
short as a tear left it (`kinds::damaged_shadow`).

The meshes are glTF 2.0 (`engine::gltf` reads them: node hierarchy, meshes, normals or smooth ones
made, vertex colours, PBR materials and their textures). A model with textures (the C172's) is
drawn with them, per pixel (`WorldRenderer::add_look` and `add_textured_mesh`, "Textured meshes" in
world-render's README): `models::build` keeps, beside each vertex, its texture coordinates and
material (`AircraftModel::surface`, each part's triangles in the order of their materials) and the
materials and pictures (`AircraftModel::look`); each vertex's colour is still there, the texture's
blurred to the size of a triangle, for the sea's mirror. The aircraft parked on the aprons is drawn
with the look too (`models::parked_surface`, `WorldRenderer::set_parked_look`). The
others (the Extra's, the quads', the Kestrel's) have a colour per face and are drawn with it. The
Extra's glTF and RON were written from the procedural model the game had until then, so it is that to the vertex.
Body axes (x forward, y right,
z down, metres from the centre of gravity), with the dimensions the type flies with: the tyres'
lowest points are its `gear` contact points, the wing tips its hard points, the propeller turns on
the thrust line at `prop_diameter`. The tests in `models.rs` hold these for every type.

### The model: `exterior.glb` and `exterior.ron`

`models.rs` says what each field is. A node of the glTF is a part (`models::Part`), a range of the
mesh's vertices that `render` moves as one piece; a node's origin is the point it moves about. The
RON says for each node what moves it (`models::Moves`): nothing (`Fixed`); a control surface
(`Surface(by: AileronLeft | AileronRight | FlapLeft | FlapRight | Elevator | Rudder | Trim, scale)`:
turned about `axis` by the simulation's angle times `scale`, not drawn torn off, trailing broken
loose); a leg of the gear (`Strut`, `Fork`, `Wheel` of leg 0, 1 or 2 in the order of
`AircraftType::gear`: the strut bent by its spring's compression along `axis`, the fork raised by
it and steered about `axis`, the wheel turned as it rolls, so the tyres stay on the ground, steer as
the simulation steers them and roll at its speed); the `Propeller` (turned at the engine's speed,
hidden in the cockpit when it turns) and its blur, `Disc` (drawn blended). `inside: Hidden` leaves
a node out of the cockpit view (the fuselage: the cabin's lining is drawn there), `shades_cabin`
puts it in the cockpit's sun shadow map (the wing, the cowling). Lights are nodes too
(`lights`: navigation from dusk to dawn, the landing light, a flashing beacon), handed to the world
as `Lamp`s with their photometry (`render::lamp`: FAR 23 position lights, a 400 cd anti-collision
light, a PAR-36 landing lamp that lights the runway ahead). `shadow` is the player's shadow's shape
and `parked` how the model stands at the aerodromes' aprons (the first type's that has one:
`kinds::parked_aircraft`).

The C172's (imported: `source/import.py`): the fuselage with the cabin's inside seen through its
windows, the cowling, the wing and struts, the flaps, ailerons, elevator, rudder, the three legs in
their fairings, a two-blade propeller and its spinner; white paint with its panel lines, rivets and
the exhaust's stain, a navy and red cheatline and EC-SKY. The Extra's: a narrow tandem fuselage under a bubble canopy, a low tapered
wing with long ailerons, spring-steel legs in spats, the tailwheel (leg 0: the steering wheel), a
three-blade propeller, no flaps, red and white paint of rays and checks, every colour per face, so
the paint's edges are mesh edges. `cargo run -p flight --example models -- c172 <view> <out.png>
[disc] [flap=30] [aileron=15] [elevator=-10] [rudder=10]` draws a model from a side.

### The cockpits: `cockpit.glb` and `cockpit.ron`

`cockpit/file.rs` says what each field is. The cockpit view looks from one of its `spots`
(`cockpit::Spot`: an eye in body axes, where it looks and how wide; `Seat` along the nose or
`LookingAt` a point), the pilot's seat first. The mouse wheel and the + and − keys zoom
(`Session::zoom`): the view is magnified 12 % a line (any fraction of one: a trackpad's scroll),
from 0.8 to 6 times, about where it looks, the eye in its spot and the head as the mouse left it;
the picture eases there (two thirds of the way every 0.12 s, `Session::ease_zoom`) and stays:
nothing steps and nothing comes back by itself. Space goes through the first spots and `--spot`
starts at any; the eye glides to a spot in a fraction of a second (`Session::advance`) and is seen
from it without the zoom, the head still turned by the mouse and Z X from there. The C172's are the
seat, the whole panel from nearer, and close to the PFD, the standby dials, the MFD and the row of
switches; the Extra's the rear seat, its panel and its instruments close. The exposure opens for
what is in the middle of the view (`render::cabin_post`): near the panel, for the panel. Space (a
stick's fourth button) goes between the seat and leaning in ("pilot, leaning in"), from any other
spot back to the seat, from outside into the cockpit at the seat (`Session::look_at_panel`). `--spot <n>` starts at spot n.

Every cockpit is drawn the same way (`src/cockpit/`): `kinds::cockpit_spec(kind)` reads its files
into a `cockpit::CabinSpec` (the mesh in body axes as `CabinModel`, its parts and how each moves
with the controls, `Motion`: fixed, a yoke, a stick, a slide or a turn by the throttle, flap lever,
trim, pedals, a dimming knob or a knob of the panel (`Control::Knob`: its `angle` for each click it
was turned), about its node's origin; the windows' glass, a node of its own; the
box the light is baked in; the instruments' canvas; the print and where it lies; the levers the
mouse can take; the keys and knobs it clicks), `cockpit::render::CockpitRenderer` holds it on the
GPU (one per kind) and `cockpit/cabin.wgsl` lights it. What takes time and no GPU (reading the
files, the light through the windows, the occlusion, the photographs read with their mip levels:
some 300 ms a kind on every core) is `cockpit::render::bake`d for every kind of the fleet on threads
of its own (`engine::threads`) as soon as the renderer is made, and put on the GPU the frame it comes
back (some 7 ms: the uploads and the print painted), so entering a cockpit, the first time too,
costs nothing; only a game that starts in a cockpit waits for its bake, at load (on the web too, on
workers; each bake's own work goes one part after another there: `engine::threads::scope`).
`render` only asks it for its draws and draws them with the `cabin` and `cabin_glass` pipelines.
Grabbing a lever (`cockpit::Grip`) drags it along its travel as the screen shows it.

- **Instruments** (`cockpit::instruments`, painted each frame in the cockpit by
  `kinds::paint_instruments`). The file fits instruments of the kinds the game has where it says in
  the canvas (`instruments: [(kind: Pfd, at: (left, top, width, height), clip, fill)]`): the
  G1000's PFD and MFD (`g1000`), the round dials of `ui::panel` (airspeed, attitude, altimeter,
  vertical speed, turn coordinator, tachometer, accelerometer), a magnetic compass's card and its
  deviation card, a radio's display. The mesh shows them where its triangles have the `screen`
  material (a display that shines) or `dial` (lit as any surface), at their texture coordinates.
  Everything shows what its instrument reads (`sim_core::Readings`), not the true flight: the PFD
  the air data and attitude-heading computers (`g1000::display::Pfd`), electric, on the same pitot
  and static lines as the standby airspeed and altimeter; the standby horizon the vacuum gyro. The
  PFD has attitude over the whole screen, speed and altitude tapes with the BARO setting, vertical
  speed, HSI with turn rate and track, wind, OAT, time, LOW VACUUM, the GFC 700's annunciations,
  command bars, bugs and alerter (see "The C172's autopilot"); the MFD an engine strip from the
  engine's rpm, fuel flow and suction and a moving map, track up, of the aerodromes, towns and roads
  within 25 km (`g1000::Instruments` keeps it).
- **Materials.** A cabin's triangle says what it is made of by its glTF material's name
  (`cockpit::Material`: plain, plastic, leather, fabric, carpet, rubber, metal, screen, dial, lamp,
  printed, vinyl, chrome; another name is plastic, metal or a lamp by its PBR values, and one with
  a base colour texture is `Textured`: drawn with its own picture and normal map, per pixel, at its
  texture coordinates, all of a cabin's pictures of one size, `CabinSpec::pictures`), its gloss
  1 − roughness; in the cabin's mesh a vertex colour's alpha is the material plus the gloss. Each
  material takes one of the photographs (`assets/textures`: `plastic`, `leather`, `fabric`,
  `carpet`, `rubber`, `metal_painted`, read at 512 px when the cockpit is baked) at its own size
  and strength (`CABIN_LAYER`, `CABIN_SCALE` and the rest in `cabin.wgsl`): the panel's crinkle
  paint is the plastic's relief at 5 cm, the trim's textured vinyl the leather's grain at 9 cm, the
  headliner and seats the cloth, the floor the carpet, chrome the painted metal's relief barely.
  The photograph is projected along the three body axes and blended by how much the surface faces
  each, so curved parts show no seam (one projection in `low`); it gives its grain to the vertex
  colour, its relief to the normal, its roughness to the gloss and its occlusion to the light. The
  sun's highlight is as sharp as the gloss and both it and what a surface mirrors follow Schlick's
  Fresnel; chrome reflects in its own colour and has almost no diffuse light.
- **Print** (`cockpit::print`): what is printed in the cabin is data, `print.shapes` in the file:
  text, rectangles, circles and lines in points of the print's canvas, painted once with egui into
  an `engine::Canvas` at four pixels a point (a legend 1 mm high some 15 pixels). The shader lays the
  canvas's panel part (`print.panel`: the panel seen from the seat) along x over every `printed`
  surface that faces aft (panel, bezels, keys) and its floor part (`print.floor`) along z over those
  that face up. The C172's: the legends of switches, key, dimming knobs, breakers and levers, the
  bezels' and the audio panel's keys and knobs, the plates' seams and screws, the placards with
  their real wording, the tail number and the hour meter, the fuel selector's plate; the Extra's
  switches' names and its limitations placard. Both canvases have every mip level and are read
  through the anisotropic sampler, so text stays sharp and steady from the seat and close up.
- **Light** (`cabin_fragment` in `cockpit/cabin.wgsl`, `cockpit/render.rs`). The cabin is in the
  shade of its roof and walls and the world bright through its windows. The sky comes in as much as
  each point sees of it, from an `engine::SkyVolume` baked from the cabin's mesh (`bake`, 3 cm
  apart) in the file's `sky_box`; what the cabin throws back of the light that comes in reaches
  what sees no window, a soft half-light (`cabin_bounce_light`: `CABIN_BOUNCE`, 0.36, of the sky's
  and half the lit ground's, with more of their colour than the sky's own light has, and
  `CABIN_SUN`, 0.035, of the sun's: blue at noon, warm at dusk); both as open as each
  vertex is to the cabin round it within 30 cm (`engine::ambient_occlusion`, cells of 1 cm, baked
  then too, each point's averaged with its neighbours' within 2 cm that face its way, kept in a
  vertex buffer of its own). The sun (or the moon) only where its shadow map lets it through: each
  frame `CockpitRenderer::prepare` draws the cabin as the controls have it and the exterior's parts
  that `shades_cabin` into a 2048-texel depth map seen from the sun over the cabin's box
  (`light_matrix`), and the shader reads it with nine filtered taps.
- **The eye** (`render::cabin_post`, `cabin_lift` in `cockpit/cabin.wgsl`). From a cabin the
  exposure opens `CABIN_EXPOSURE` (0.9) stops more than the world's, and for what the pilot looks
  at (`engine::Post::spot`, `CABIN_SPOT`, 0.7): where the middle of the view would be left more
  than a stop and a half under mid grey (the panel in the shade seen from near, or at dusk) it
  opens by that share of what is missing, up to three stops, in about a second, and the world
  through the windows is left to glare; at night only by `CABIN_SPOT_NIGHT` (0.2), so the night
  stays dark and the panel is read by its lights. Beyond the exposure the eye adapts to the cabin's
  shade (`cabin_lift`): what is lit less than 1.5 times the light of the open sky (`LIFT_FROM`) is
  drawn brighter by that share to the power 0.8, up to 12 times, from the day to the end of the
  dusk; half of that at night (`LIFT_NIGHT`).
- **The panel's lights.** A display shines with its own light, 2000 cd/m² at noon, and as bright
  to the eye until the end of the dusk, as its photocell keeps it (`DISPLAY`); at night as the
  DIMMING PANEL knob sets it (`DISPLAY_NIGHT`). The same knob lights from behind the white legends
  of the print (the bezels' keys and knobs, the audio panel's, the switches', the breakers'), by
  the square of its turn: `LEGENDS_DAY` (60 cd/m²) by day, when they do not show against the
  daylight, down to `LEGENDS_NIGHT` (4) at night, so they glow from the dusk on; and at night a
  warm flood light under the glare shield on the panel's face (`FLOOD`, 2 lux), by which the
  placards and the knobs are seen. STBY IND lights the standby dials. As in the aircraft, the
  GFC 700's keys have no lamps of their own: only their legends are lit, and the mode is read on
  the PFD.
- **The displays' and dials' glass** (`cabin_fragment`, `SCREEN_F0`, `SCREEN_SMUDGE`,
  `SCREEN_DUST`): it mirrors what is behind the eye, the windows where its reflection finds them
  (the light through the windows' volume, along the mirrored ray) and the dim cabin elsewhere, 3 %
  head on and more at a slant, a little more where it is smudged; the sun, when it is behind, in a
  sharp glint and a wide soft one; its dust lit by what falls on it. A soft sheen across a dark
  screen by day, nothing at night.
- **Glass** (`cabin_glass_fragment`, drawn over the world: the file's `glass` node): it mirrors a
  little of the dim cabin, more at a slant, and its dust and smears scatter the sun that falls on
  them, mostly forward.
- **Drawn first,** right after the sky, so the world behind the panel and the cabin is not shaded.
  The near plane is 5 cm in the cockpit view (25 cm outside), for the walls beside the head.
- **The mouse** takes the knobs of the file's `levers` (the C172's throttle, flap lever and the top
  of the trim wheel; the Extra's throttle) and drags them along their travel as the screen shows it
  (`Grip`), presses the keys and turns the knobs of its `controls` (`cockpit::panel_at`: the
  pointer's ray meets the plane of each key's or knob's face; the C172's bezels' AFCS keys and D→,
  the HDG, ALT and BARO knobs); anywhere else it looks around. A key takes the pointer up to
  1.8 mm beyond its edge and a knob 7 mm (`KEY_REACH`, `KNOB_REACH`: from the seat a knob is some
  twenty points across), the one whose edge is nearer where two would. What the pointer is over
  has a tag beside it with its name and, for a knob, what it is set to, and a knob its edge lit
  (`Flight::pointed`, `hud::draw_pointed`; the keys are not lit, as the real ones are not: the
  PFD's status bar says what is on); a knob turned by a key is lit and named too, for a
  second and a half, at the screen's edge with an arrow when it is out of the picture.
- **The panel's knobs turn.** A part moved by `Turn(by: Knob(Heading), angle: 0.2617994)` turns
  that angle for every click its knob was turned, by the mouse, a key or a controller
  (`Session::knobs`, a `cockpit::KnobTurns`: the clicks, and the drawing going two thirds of the
  way there every 50 ms, so a click is seen as a movement and a knob held turns smoothly). The
  C172's HDG knob, the ALT knob's outer ring and inner knob and the CRS / BARO knob's, on both
  bezels, are such parts (`knob_heading_1`, `knob_altitude_coarse_1`, `knob_altitude_fine_1`,
  `knob_baro_1`, `knob_course_1`, and `_2` on the MFD): machined knobs (a fine knurl, a thin pale line on the face), which
  `examples/panel_knobs.rs` makes where the file's `controls` have a knob, in place of the smooth
  round one (a round knob without a mark does not show that it turns).

The C172's cabin, as its files have it (`source/cockpit.py` makes the `.glb`): seen from the left
seat, with a G1000 panel; the model's own inside, with its pictures, the same shell the outside
has: the white lining with the windows' moulded frames and the windscreen's posts, the wing's
roots and vents, the doors' tan panels, the seats with their stitching on their rails,
the belts; its floor carpeted and its firewall in dark trim, the game's materials; the magnetic compass hanging from the windscreen's top frame; made for the game, the
padded glare shield, straight, shaped to the windscreen's foot, and the panel under it, dark grey
with white legends: the PFD
and the MFD in their bezels with their knobs (smooth dark grey; those the mouse turns knurled, with a line on the face)
and keys and the softkeys, the audio panel, three
standby dials under chrome rims, the DIMMING knobs, the row of rockers and the breakers, throttle,
mixture and flap lever, the ELT and the hour meter; two yokes on chrome shafts (they turn 45° with
the ailerons and slide 7 cm with the elevator), the pedals (±4.5 cm), the trim wheel (±50°), the
fuel selector. From the cockpit the propeller's blur is a third as strong as from outside. The Extra's: flown from the rear seat,
the tub of both cockpits with the fuselage's welded steel tubes along its sides, the canopy's
frame and the bubble's glass, the rear panel with its round instruments in square flanges
(airspeed, altimeter, accelerometer, tachometer; vertical speed, turn and slip), the radio, the
compass on the coaming, the switches and the magnetos' key, the stick between the knees (it leans
with elevator and ailerons), the throttle on the left wall, the pedals, both seats with their
harnesses and the front cockpit beyond.

### The C172's autopilot, a Garmin GFC 700

`sim_core::gfc700` is the autopilot itself (its README says how each mode behaves and what it
measured); here is how the player works it and what the G1000 shows of it.

- **Keys** (`controls::panel_key`, only in an aircraft with a GFC 700): P is AP (see "P and the
  arrows" below) and Shift+P FD; 4 5 6
  HDG NAV APR; 7 8 9 ALT VS FLC; , and . the UP/DN wheel (nose down, up); [ and ] the heading bug a
  degree (with Shift ten), \ the HDG knob pushed (the bug to the heading); ; and ' the selected
  altitude 100 ft (with Shift 1000); / D→ the nearest aerodrome; C is AP DISC (it disconnects, or
  silences the alert; with nothing engaged it centres the yoke as ever); K the BARO knob. The knobs'
  keys and the wheel's turn on while held (after 0.4 s, 12 clicks a second). A controller's
  buttons can be mapped to them (`autopilot`, `heading`, `nav`, `approach`, `altitude`,
  `vertical_speed`, `level_change`, `nose_up`, `nose_down`, `heading_bug_left`,
  `heading_bug_right`, `direct_to` in the mapping file; a stick has 7 to 12 by default: AP, HDG,
  ALT, FLC and the heading bug left and right).
- **Clicks and drags** (`cockpit::panel_at`): the AFCS keys and D→ on both bezels are pressed by a
  click. The HDG knob, the ALT knob (the outer ring 1000 ft, the inner knob 100 ft) and the BARO
  ring are turned by dragging them: pressed on, they are held (`Drag::Knob`) and turn as the
  pointer moves, up or right clockwise, down or left anticlockwise, a click every 6 points moved
  slowly and up to three times as many moved fast (`cockpit::drag_clicks`: twice at 500 points a
  second), wherever the pointer goes while the button is down; a click without moving turns
  nothing. While a knob is held its tag says what it is set to, click by click, beside the whole
  knob. Or by the mouse wheel over them. The selected altitude goes from 0 to 25 000 ft
  (`sim_core::gfc700`'s `SELECTED_RANGE`). The pointer is a hand over
  everything the mouse can press, turn or take (the levers too), closed while it holds a lever or a knob.
  The CRS knob says it is for the VOR (phase 2). A notch of the wheel is a click: macOS gives a
  mouse's notch turned slowly as a tenth of a line (more the faster it turns), which
  `engine::Input::wheel` counts as a whole one, or ten notches would pass before a knob moved or
  the eye changed spot; a trackpad's scroll is a click every 40 points.
- **The ALT knob from the seat**: it is at the bottom of the bezel, under the AFCS keys: small
  beside the yoke from the seat, and below the picture leaning in. So Shift and the wheel over the HDG
  knob or any AFCS key turn the selected altitude, 100 ft a click (`cockpit::wheel_knob`; macOS
  sends the wheel as a scroll across while Shift is held, which `engine::Input::scroll` counts
  then), and ; and ' do it at the keyboard. The spots "panel" and "PFD" (`--spot`) show the
  knob itself.
- **Knobs that turn, and their tags**: each knob is drawn turning 15° a click, whatever turned it
  (the wheel, a click, its key: [ ] for HDG, ; ' for ALT, K for BARO), the ALT knob's ring and its
  inner knob each by itself. Under the pointer a knob has its edge lit, and a key or a knob a tag: `HDG
  080°`, `ALT 5500 ft` (with `inner knob: 100 ft a click` or `outer ring: 1000 ft a click`), `BARO
  1013 hPa`, `AP · autopilot`; over the HDG knob and the keys a second line says `Shift + wheel:
  ALT 5500 ft`, and with Shift held the tag is the altitude's. A knob turned by its key is lit and
  tagged for a second and a half; the ALT knob's tag is at the bottom edge with an arrow when the
  knob is below the picture.
- **APR** without an approach loaded loads the GPS approach of the last D→'s aerodrome, or of the
  nearest, to the runway end into the wind (`Session::panel`), and arms it. NAV needs a D→ first.
  A key that does nothing says why in the middle of the screen (on the ground, no plan, VNV and BC
  not fitted).
- **P and the arrows** (`Session::hold_here`, `Session::afcs_arrow`): P with nothing on engages
  the autopilot holding what there is, with no other key: HDG with the bug on the heading of now,
  ALT at the altitude of now, the selected altitude the nearest hundred feet. (With the flight
  director already on, P couples the autopilot to its modes as they are; the bezel's AP key
  clicked is the real one: ROL and PIT.) While it is engaged the arrows are not the yoke's but its
  targets': ← → the heading bug 5° (Shift 1°), in HDG; ↑ ↓ the selected altitude 100 ft (Shift
  1000 ft) and VS at 500 ft/min towards it, ALTS armed, so it levels there by itself and goes to
  ALT (the throttle is the pilot's: more to climb). Held, they go on as the knobs' keys do. [ ]
  and ; ' stay the knobs. P again or C disconnect.
- **No autothrottle**: W and S stay the throttle's. A firm hand on the yoke (a gamepad's or a
  stick's yoke 0.3 of its travel from where the servos hold it for half a second; not the arrows,
  which are the targets' then), the trim by hand (Q and E without the assist, a controller's trim) and C disconnect it;
  the keyboard's pilot then has the yoke, holding the attitude it is handed (it starts afresh each
  frame while the servos fly). L hands the aircraft to the game's autopilot, which lands it; P
  then hands it back.
- **What shows it**: the PFD's top bar (`g1000::Afcs`): the lateral mode active (green) and armed
  (white), AP, the vertical mode with its reference and the one armed, a mode just captured
  flashing, AP flashing yellow after a disconnect (red if it let go by itself); under it the active
  leg (D→ RW13, its distance, desired track) or the ground speed and track. On the attitude, the
  flight director's magenta command bars; the heading bug and its box on the HSI, whose needle is
  the active leg's course with its deviation (2 NM full scale en route, 1 NM on the approach's
  first legs, 0.3 NM on the final); the selected altitude above the altitude tape (flashing within
  1000 ft, framed within 200 ft, yellow when it is left) and its bug; FLC's airspeed above the
  airspeed tape, or MAXSPD and MINSPD flashing there; the VS reference over the vertical speed
  tape; the glide path beside the altitude tape. The MFD's map draws the plan's legs. The tags at
  the top right say AP or FD, the modes and references. The two-tone disconnect tone is
  `audio`'s (two notes, a quarter of a second each).

## The FPV quad

The player can fly a multirotor too, `sim_core::multirotor`: the 5-inch FPV quad of
`assets/aircraft/fpv5` (`multirotor.ron`, its physics and flight controller; `exterior.glb` and
`.ron`, its look, made by `examples/quad_model.rs`), or the 5-inch racing quad of
`assets/aircraft/race5` (2207 1950 kV on 6S, 500 g, 13.6:1, 175 km/h flat out, the camera up 40°;
its look made by `examples/quad_model.rs race5`: neon three-blade props, a white 6S pack). Pick one in
the menu (Esc, "Aircraft": the aircraft and the multirotors are `kinds::Craft`) or with
`--aircraft fpv` / `--aircraft race`. It starts on its feet
at the start of the runway, disarmed; `--air` (and 2) hovering 15 m up, armed; 1 puts it on the
ground right under where it is (on water, back on the runway). A new multirotor is only files, as an aircraft is: its `multirotor.ron` and its
look; the game has no drone of its own, only `multirotor.rs`, for any of them.

The Kestrel (`assets/aircraft/kestrel`, `--aircraft kestrel`) is a tilt-rotor
(`sim_core::multirotor::Tilt`): a multirotor whose two nacelles tilt, with a wing and a tail. It
has the quad's keys and F and G, its nacelles' lever, a notch (12°) forward or back, and C, the
help its flight controller gives (hold, as it starts: the collective lever asks for a climb and
the stick for a speed over the ground, and let go it stays where it is; level; rates; none). It
has turbines: W and S are the collective lever, Enter starts and stops the engines (R is the
nacelles back, as G: a joystick's flap buttons are its nacelles, its trim hat its nose, its centre
button the help), and its display shows the rotors' speed, the
engines' torque and the fuel (NR, TQ, FUEL) where a quad's has its battery, the help (SAS) and
where the nacelles are (NAC); it warns of LOW ROTOR RPM and LOW FUEL. Its pitch stick tilts
its rings and it goes forward level; Q and E trim its nose down and up.

It has a cockpit (`assets/aircraft/kestrel/cockpit.glb` and `.ron`, made with its model in
Blender by `scripts/blender/kestrel.py`; `kinds::cabin` says which craft have one, and the renderer keeps a
cockpit per craft): V goes into it, the wheel through its spots (the pilot's seat, on the right;
leaning in; the panel; the flight display). Its panel has three displays (`cockpit::glass`, thin
lines on black: `FlightDisplay`, the horizon across the screen with the speed's and the height's
tapes, the heading, where it is going and its rings from the side; `SystemsDisplay`, torque,
rotors, collective and fuel as bars, the endurance, the engines, the help and what is wrong;
`SituationDisplay`, the ground from above with the aerodrome it started from), from
`multirotor::panel` and `rotorcraft`; its stick, collective lever and pedals move. The round
gauges of a turbine tilt-rotor (`Torque`, `Rotors`, `Fuel`, `Nacelles`, `Annunciators`, `Heading`)
are there for a cockpit that wants dials.
In it the display over the picture is not drawn. Its view from inside is the pilot's seat; the chase camera
follows it from as far as its size asks (`chase_scale`). Its model's nacelles are `Moves::Nacelle`
parts, turned about the line across the frame through their node; the rotors on them turn with
them (`render::tilted`), and its elevator is a `Moves::Surface`. `--nacelles <0..1>` sets the
lever from the start (for a picture).

- **Flying it**: the sticks of its radio (`MultirotorControls`). It starts armed with the throttle down,
  as an aircraft starts with its engine running. W S the throttle stick (it stays where it is left,
  as a radio's does: it never moves by itself; plain power, more climbs and less sinks), ↑ ↓ the pitch stick (↑ forward: nose down, it flies forward), ← → roll, A D yaw; the
  keys move their sticks as springs (out in a quarter of a second, back when let go). R (or Enter)
  the arm switch, off and on again: it arms only with the throttle down, level,
  not crashed, and the message in the middle says why not. It has no flight modes: it levels
  itself. The pitch and roll sticks tilt it, up to 55°, and it levels when they are let go; the yaw
  stick is a rate of turn. A controller's sticks are where they are (`controls::Pads::sticks`), an RC
  transmitter's too (`radio` below).
- **Views**: V between its FPV camera (the cockpit's view, `Session::cockpit`: the camera tilted up
  25°, 120° across the picture, nothing of the quad drawn) and the chase camera, 1.4 m behind and a
  little above it (the wheel, + −: nearer or further), following its heading some 0.4 s behind
  (`Session::follow`, `multirotor::chase_lag`; an aircraft's 0.2 s): a quad yawing 200°/s through a
  gate turns away from the camera and the camera comes round after it, instead of swinging round the
  quad with it. The heading it follows is the nose's way over the ground and, the steeper the nose,
  the back's (`chase_heading`): steady diving straight down, where the Euler heading turns half round.
  On another quad (N), after a restart or a jump it is straight behind at once. Z X and the mouse
  look round as outside.
- **Its display** (`ui/osd.rs`), in both views, as Betaflight's: the battery (an icon by a cell's
  voltage, the pack's voltage, a cell's, the current, the mAh used) at the top left, the heading at
  the top, the time armed at the top right; through the camera the crosshair and
  the horizon as the camera sees it, tilted; DISARMED and ARMING BLOCKED and why; LOW BATTERY (a cell
  under 3.5 V) and CRASH blinking; the height above where it was armed and the climb at the bottom
  left, the throttle at the bottom, the speed in km/h at the bottom right.
- **Sound**: each two-blade propeller a buzz at twice its turns a second, rich in harmonics, with
  the air rushing at each blade and its motor's whine at seven times them, louder the faster it
  turns; the four a little apart, so they beat (`audio.rs`).
- **Crashes**: a hit harder than 10 m/s (the ground, a trunk, a building) crashes it: disarmed, it
  falls and rests whole; the message says 1 or 2 start again. Slower it bounces off. In water it sinks.
- **Against another quad** (a rival, `sim_core::multirotor`'s "Against another multirotor"): their
  propellers meeting at more than 1.5 m/s break (the quad tumbles down on three rotors and is
  crashed on the ground), a slow brush chips a tip (it flies on, shaking); frames knock apart. A
  broken propeller is drawn as the stubs at its hub, its blur as small (`PlaneVisual::props`).
  `--damage props` with `--shot` shows one: the front left broken, the front right chipped.
- **Replay and recordings**: F2 shows it as it flew (`MultirotorLook`: body, sticks, rotors,
  battery, armed, its propellers' blades); a recording's shot flies it too (`--aircraft fpv` in its `args`; `keys`
  `"pitch=-0.8"`, `"roll=0.3"`, `"yaw=..."` hold its sticks part of the way; `Cockpit` cameras are its
  FPV camera; `osd: true` draws its display over the frames; with `--course` in `args`, `racer: true`
  lets the race's autopilot fly it: `scripts/video/course.ron` is a lap of Pinewood Airfield and its
  stills, at 1280 × 720 and 30 frames a second, `scripts/video/assemble.sh <dir> <out.mp4> --fps 30 ...`); see "Recordings").

## Race courses

A race round a course of `assets/courses` (`sim_core::course`: its README says what a course file
is) in a quad: chosen in the place picker (M, "Race courses": "Race") or with `--course <name>`.
Two are built in, both in the generated world of seed 1: Deepwood Forest (`deepwood`, the default
of `--ai`), a forest track 6 km west of the home aerodrome, raced in the race quad: a trail cut up a
slope through a thick wood of oaks, ashes and firs, under the canopy, under two logs, through
hoops on posts and hung from beams, up to a high hoop in a clearing, a split-S down and a fast run
home to the arch at the forest's edge; and Pinewood Airfield (`pinewood`), 9 km south of the home
aerodrome, raced in the FPV quad. The game goes to the course's world, puts the player in the
course's quad (`quad`; else the one flown, else the first of the fleet), on the start's pad, its
FPV camera, and the race begins (`Session::start_race`, `race::RaceSession`):

- **Countdown**: 3, 2, 1, GO! in the middle of the display, a beep a second and a higher one at GO;
  the sticks are held, the throttle down, until then. The clock runs from GO.
- **Gates**: each must be flown through in order, the right way (`course::Race`); a chime at each, its
  split into the lap under the clock, green or red against the last lap's there. Flying past the next
  gate outside its opening, or through another, is a miss: a buzz and MISSED GATE n blinking; go back
  and take it. The next gate's strips pulse bright, those flown this lap are dim; a diamond on the
  display marks it (with its distance), or an arrow on the edge of the picture the way to turn.
- **Laps**: at the top the race's clock, the lap and the gate (`3/16`, START, FINISH); at the right
  the lap's time, the last and the best; the best lap of each course is kept in the settings
  (`best_lap_<course>`), and shown in the picker. At the finish the results: each lap, the total, a
  new best lap.
- **Restarts**: 1, 2, 3 (and Backspace) start the race again from the pad; a crash ends it (CRASHED,
  1: race again). L hands the quad to the race's autopilot (`course::racer::Racer`, AUTOPILOT on the
  display) or takes it back. The replay (F2) shows the race as it was flown; the race waits meanwhile.
  Another place, or an aircraft, ends the race.
- **Rivals** (`race::Rival`): quads flown by the game race against the player from the same GO,
  each in a quad of its own on a pad of its own, side by side across the start with the player's in
  the middle, 3 m apart (`rival_pad`: left, right, then further out), its frame tinted its colour (red, blue, yellow, violet: `PlaneVisual::tint`):
  `--rival racer` (the race's autopilot), `--rival <file>.fltmodel` (a network trained on the `gates`
  task: `trainer/GATES.md`; `--model` with such a file does the same), `--rival net` the course's
  network (`models/gates-<course>.fltmodel` if there is one, Deepwood's `trainer/DEEPWOOD.md`, else
  `models/gates.fltmodel`, Pinewood's), `--ai` both (the course's network and the autopilot); without
  `--course`, at Deepwood. Every rival flies the player's quad's kind, the course's. Every rival keeps its
  own count (`course::Race`); a network, its race over, is handed to the racer to hover; a crashed
  one stays where it fell. The display adds the position of the quad watched (`P2/3`) and everyone
  in order, each in its colour with its lap and next gate, or its time, or CRASHED, and each other
  quad's name and distance over it. **N** puts the camera on the next quad, as it is (its FPV
  camera, or behind it: V): the display is then that quad's race (WATCHING NET); `--watch <n>` starts
  on quad n. A restart puts the rivals back on their pads; the end of the race takes them away.
  They touch as any two quads do (above: propellers break or chip, frames knock apart); the racer
  gives way to a quad closing on it when flying fast. In 4 kt of wind from any of eight ways none
  touches another over the whole race (`race.rs`'s `rivals_never_freeze_in_the_wind`, and
  `deepwood_rivals_never_freeze_in_the_wind` among the trees). Against a gate or a tree a quad is
  as against another quad: its propellers break or chip, its frame bounces off, a hard hit crashes
  it; nothing holds it.
  To watch a race: `scripts/run.sh --ai` (then N, and V), or with the camera on the network from
  the start, behind it: `scripts/run.sh --ai --watch 1 --ext`.
- **Look** (`render::course`): the gates' frames from `assets/courses/gates.glb`, each painted its
  colour, its LED strips along the inside of the opening shining in it (light by themselves, some
  15 000 cd/m², the next gate 45 000), so by day they are coloured tubes and at dusk and at night
  they glow with the bloom round them; its number in white LEDs on its plate, facing the racers; the
  arch's checkered banner; the start's pads, one for each quad racing. Some 50 draws in all, only
  when a course is raced.
- **Sound**: the chimes are `audio.rs`'s (`Audio::chime`): a bell-like ding at each gate, a longer
  higher one for a lap and GO, a low buzz for a miss.

A new course is only a file in `assets/courses` (read at start, `race::load`; without the folder the
built-in ones); a new kind of gate is a shape in `gates.ron` and its model in `examples/gate_models.rs`
(the log is bark with cut ends, its own colour; the high and hung hoops' tubes are painted as any).
The trail a course cuts through the forest (`sim_core::course::Trail`) is the world's: its trees are
grown so, drawn and hit alike, and the grass is worn along it and gone round the start's pads.
`sim-core`'s `examples/course_check.rs` flies a course's racer and writes its line for a map.

## The replay

F2, or "Watch replay (F2)" in the settings, shows the last minutes again, as in MSFS: everything as
it happened (the player's aircraft with its controls, surfaces, propeller, flaps, dials and G1000,
damage and crash, the other aircraft, the wrecks and their fire, the hour), but nobody flies. The
world's clock stands still meanwhile (no step is taken: the traffic, the wind and the fuel wait); the
picture's clock (water, fire, clouds) is the replay's. F2 or Esc goes back to the flight exactly
where it was. Every camera works (V, N, Z X, the mouse, the wheel, + −, the cockpit's spots); the keys
and the mouse that fly or work the panel do nothing; O and M leave the replay first.

It starts 30 s before the last landing kept, else a minute before now (or at the start of what is
kept). At the bottom a bar: −30 s, −10 s, stop / go on, +10 s, the speed (0.25× to 8×), a scrub bar
over what is kept, the time left, "Back to flight". Keys: Space (or P) stop / go on (at the end, from
the start), ← → 10 s back / on (⇧ 30 s), ↓ ↑ slower / faster, Home and End. At the top right the
last landing (a touchdown after 3 s in the air, not a placement): vertical speed, the most g in the
0.6 s after, airspeed and ground speed, bank and pitch, and on a runway its number, the metres past
its threshold and off its centre line; under it a chart of altitude and airspeed over what is kept,
the moment shown and the touchdown marked (a click or a drag on it goes there).

`replay::Recorder` keeps, after every step (`Session::advance`), what is due in rings that forget what
is older than the window (`replay_minutes` in the settings file, 1–30, 10 by default; a slider in the
settings): the player's `sim_core::Look` (what moves and what is shown, from one step to the next)
with its height above the ground and the hour 15 times a second; a whole copy of its `Aircraft` every
2 s (its type, what changes slowly) for each look to be put over (`Aircraft::show`); its GFC 700 5
times a second; the other aircraft's state, controls and flaps 5 times a second; the wrecks when they
change (as often as the looks while the newest is under 30 s old, then once a second). In between, the
looks are blended (`Look::blend`: the position along the curve both velocities draw, the attitude
slerped, the dials and controls in between). `replay::Playback` holds the world's aircraft, wrecks,
GFC 700 and hour of when the replay began while the world's own are written with the moment shown
(`Recorder::show`); `Session::end_replay` puts them back. Ten minutes with six other aircraft take
some 18 MB (the test `ten_minutes_with_traffic_fit_in_a_few_megabytes...` measures it); alone, some
15. A world made anew (another place) forgets what was kept. `--shot` with `--replay <s>` shows the
replay stopped `<s>` seconds before the end, after `--wait` (and `--land`: L first).

## Recordings

`--record <dir> --script <file.ron>` renders a script's shots frame by frame without a window, in
the game's own time, to `<dir>/<shot>/00000.png...`; `--only <shot>[,...]` just those, and
`--storyboard` one still of each (at its `still` seconds) to `<dir>/<shot>.png`, which is how a
shot's timing and angle are chosen before its frames are paid for. `record.rs` documents the
script; `scripts/video/` has three (`showcase.ron`, `course.ron`, `deepwood.ron`) and
`assemble.sh`, which makes the video of the frames with ffmpeg. What a script says:

- **The picture**: `width`, `height`, `fps`; `supersample` (drawn that many times larger each way)
  and `subframes` averaged over the `shutter` (a fraction of the frame), in linear light: motion blur.
- **A shot**: the game's command line (`args`), where it starts, the seconds flown `before` its
  first frame, keys held, the GFC 700 worked, `racer: true` (the race's autopilot flies the
  course of `args`), `osd: true` (a quad's display over the picture).
- **Cameras** (`camera`, keys along a smooth curve): `Chase` (round the craft at a distance),
  `Fixed` (standing where the craft was, in its axes then), `Cockpit` (a seat's spot, a quad's FPV
  camera), `Follow` (a chase drone: it flies for a point just behind the craft, `lag` seconds late,
  so it trails with the speed and swings wide in the turns, as low as 0.25 m) and `At` (standing
  at a point of the world, `n`, `e`, `agl`, turning to look at the craft: by a gate with a long
  lens, or high above).
- **The lens** (`lens`, the script's or a shot's own; `engine::Post` through `game::Lens`): `blur`
  (depth of field, focused on the craft unless `focus` says how far), `flare` (the sun's, where
  the sun is in the picture), `vignette`, `bloom` (the glow of the gates' LEDs), `warmth` (the white
  balance), `contrast`, `saturation`, `exposure` (stops). Without a lens the picture is the game's.
- **Slow motion** (`speed`): the world's rate at given seconds of the shot, eased between them
  (`[(0.8, 1.0), (1.1, 0.25), (1.6, 0.25), (1.9, 1.0)]`). The frames fall between the simulation's
  steps, so the quads are drawn (and heard) moved on from their last step (`Session::lead`); the
  simulation is the same as without.
- **Sound** (`sound: true`): each shot's sound to `<dir>/<shot>.wav` (48 kHz stereo), made with its
  frames by `audio::Mixer`: every quad a voice of the game's own synthesiser (`audio::Synth`),
  heard from the camera (level by distance, side by where it is in the picture, pitch by the
  Doppler shift of its speed, its top dulled by distance), the air past the camera, and the race's
  chimes. In slow motion the notes fall with the square root of the rate (an octave at a quarter
  of the speed). `assemble.sh` joins the shots' sounds and puts them under the video (AAC).

`scripts/video/deepwood.ron` is the race round Deepwood Forest at sunset; its header says how to
render and assemble it (also a square crop: `assemble.sh ... --crop 1080:1080`).

## The menus

The game opens on its **first screen** (`ui::menu::vehicles`): what to drive, as three pictures
that fill the window (side by side, or one above the other in a window taller than wide), each
taken in the game, with a name, what there is of it and a line: **Planes**, **Drones**, **Cars**.

- The planes and the drones are this game's two families of craft (`kinds::Family`). Choosing one
  flies its craft flown last (the settings keep one for each: `plane`, `drone`; the first of the
  family before any was) and shows its aircraft to choose from, over the start and pause screen
  (`Session::choose_family`). From then on the aircraft shown are those of the family flown.
- The cars are the driving game (`crates/games/drive`), which this program has inside it: "Cars"
  shows it in this window in place of this game (`engine::program::hand_over`, made by `Shown` in
  `main.rs`: the flight game is dropped, its sound with it), and its pause menu's "Change vehicle"
  brings this game back, made again with `--menu vehicles`, on the first screen. The web has no
  driving game: there the screen shows the planes and the drones.
- Esc (or "Start screen" at its top right) chooses nothing: the start and pause screen, with the
  craft that waits.

The first screen is for the player who has said nothing yet: a command line that says what is
flown or where (`--aircraft`, `--course`, `--place`, `--at`, `--air`, `--final`, `--pos`, `--seed`,
`--model`, `--rival`, `--ai`) opens on the start and pause screen instead, and `--shot`, `--record`
and `--remote` open no menu at all, as before (`main.rs`, `opening_menu`). The start and pause
screen's "Change vehicle", in its head, shows it again.

Everything else the game has is a click away on the **start and pause screen** (`ui::menu::home`):
it is under the first screen when the game opens (the setting `menu_at_start`, on unless taken away
in Settings › Flight: without it the game opens in the cockpit, with neither) and is shown whenever
Esc pauses the flight. A column on the left over the waiting flight: the aircraft that
waits with its picture, **Fly** (or "Resume flight"), then Aircraft, Place, Weather & time,
Settings, Manual and Replay, each with what is chosen now and its picture or its key. The keys that
open them in flight still do (O, M, F1, F2), so nobody has to know them.

- **The settings** (O, `ui::menu::side`) are a panel down the right side: the world stays in view
  on the left, so a change of the hour, the clouds or the haze is seen at once. Opened in flight
  (O, ⌘,) the flight goes on beside them: the keys and the controllers still fly, the panel is
  worked with the mouse, and O again or Esc closes it. Opened from the start and pause screen the
  game waits under it, and the keys and the pad move in it as in the other menus. The aircraft,
  the picker and the manual always pause: they cover the flight. At its top
  the aircraft and the place flown, each a way to its screen; then four sections. *Weather*: the
  kinds of day as six little skies, the hour as the day's sky in a band with its sun, the sunrise
  and the sunset marked (`world_render::sun_elevation`), "Start at the current time" (`local_time`,
  on unless taken away: a game opened in a window starts at the hour of the computer's clock,
  `engine::clock::local_hour`, wherever it flies, a real place far away too, simply the hour the
  player lives in; `--hour` wins, and screenshots, recordings and tests never read the clock:
  `Options::clock` is only set by `main` for the window and by the web's start), the wind as a compass rose (a drag sets
  where it blows from and, by how far out, how hard), and sliders with a picture of what each
  means (a sky filling with cloud, a cloud at its height, hills in the haze, a thermometer in the
  colour of the day). *Flight*: the keyboard assist, the fuel, the controls' box, the yokes and the propeller
  in the cockpit's view (`yokes`, `propeller`: off, the cockpit is drawn without them, so the
  panel behind the yoke shows whole and nothing turns ahead; outside they are always drawn and the
  yoke flies the same; `Scene::propeller_inside`, `CockpitControls::yokes`), the natural mouse
  look (`look_natural`: dragging takes the scene with the pointer, down looks up and right looks
  left, in the cockpit and round the aircraft outside; `Session::mouse_moved`), the
  replay's minutes, the manual, what opens at the start. *Graphics*: the four qualities as cards
  with what each draws and its load. *About*: the version, who made it, where its source is. A change is applied at once and written
  to the settings file, as always.
- **The aircraft** (`ui::menu::hangar`) are a sheet over most of the window: a big picture of each
  of the family flown (the planes, or the drones), its name, what it is, a few figures, how hard
  it is to fly and how it starts. One click flies it.
  What it says of a craft is its folder's `card.ron` (`kinds::Card`: `class`, `tagline`,
  `good_for`, `difficulty` 1 to 5, `facts`), its picture the folder's `picture.jpg`; a craft
  without them shows its name over a plain card.
- **The place** is the place picker (M, "Real places" below), in the same look.

The mouse works them as any widget. The keyboard and the game controllers move a cursor
(`ui::menu::Cursor`): the arrows, the direction pad or the left stick move it by rows and along
them (on a slider, left and right change the value), Enter, the space bar or A choose, Tab or the
shoulder buttons change the settings' section, Esc or B go back: from the first screen, the
settings or the aircraft to the start and pause screen if they were opened from it, else to the
flight. On the start and
pause screen any other key flies on, as a paused game always did; on the first screen, in the
settings opened from the menu and in the aircraft the other keys wait. The settings opened in
flight take no keys: the flight has them (`Session::waiting_menu_shown` is false for them). The game hands the keys over as moves (`Session::menu_keys`,
`ui::menu::Nav`), so the session's tests cover them.

The look is `ui::menu::theme`: panels of dark glass (no blur behind them: the overlay is drawn
after the picture is finished), one accent colour, Barlow in three weights and the Phosphor icon
font (`assets/fonts`, OFL and MIT; used by the menus only, the instruments keep the overlay's own
font). The pictures are the game's own: `scripts/menu-shots.sh [name...]` takes them again
(`--shot --clean`, the outside camera with `--look` and `--zoom`; the real places are downloaded
into its own cache) into `assets/aircraft/<model>/picture.jpg`, `assets/menu/places/<key>.jpg` and
`assets/menu/vehicles/<planes|drones|cars>.jpg` (the first screen's, square with the subject in
the middle; the cars' is taken by the driving game)
(a featured place's code, a course's key, `generated`). They are read from the `assets` folder the
first time a screen shows them (`ui::menu::Pictures`); one that is missing is a plain card with an
icon. `--menu <vehicles|home|aircraft|settings|weather|flight|graphics|about>` opens a menu at the start,
for `--shot`.

## The manual

F1, or "Manual" on the start and pause screen or in the settings, opens a short illustrated manual over
the whole window; the game pauses (as with the picker: the world is not drawn, `Session::covered`)
and Esc or F1 closes it. Seven pages, in a column on the left (← → turn them): Getting started (take
off, fly, land, in steps; seeing the landing again in the replay), Views and cockpit (the cockpit's views, small, and how to go through them),
Controls (keys, the T.16000M drawn with what each control does, the levers), Autopilot (the
GFC 700's keys and knobs on the PFD, where it shows its modes, steps to hold heading and altitude,
turn, climb with FLC or VS, go direct and disconnect), Instruments (the PFD at a glance), Places and
weather (the picker, the settings, the hour), FPV quad (its keys, its modes, its display). `src/manual/`:

- `pages.rs` is what it says: each page a list of blocks (titles, short lines, pictures with their
  marks, the views, steps, tables of controls, the joystick). A mark is a box and a label in
  fractions of its picture, yellow for what is pressed, green for what is read, pink for what was
  chosen; they are drawn over the picture each frame (`mod.rs`), so they stay sharp at any size and
  their text is the program's.
- `inputs.rs` names what each step does (`Act`), not its key: the keys are found from it, the
  G1000's in `controls::panel_key`, the joystick's buttons in the T.16000M's defaults, so the manual
  cannot name a key or a button the game does not use (`Action::key_code` ties a button to its key).
- `joystick.rs` draws the T.16000M, its buttons' jobs read from the same defaults.
- The pictures are the game's own, 1600 × 1000 shots made smaller as JPEGs in `assets/manual` (15,
  1 MB, built into the program; read the first time a page shows them). `scripts/manual-shots.sh
  [name...]` takes them again (`--shot` with `--spot`, `--afcs`, `--air`, `--final`, `--ext`,
  `--picker`; one game at a time, killed past 4 GB, under `$FLIGHT_GPU_LOCK` if it is set, with
  no settings of the player's and its own download cache) and `engine`'s `shrink` example makes them
  small. Then look at each page (`--manual <page> --shot <dir>`) and move the marks that no longer fit.
- Tests: every page has pictures that can be read, at the game's proportions; every mark is inside
  its picture; the views are `kinds::spots`'; every act has a key, and pressed (or held, through
  `Held::with`) in a session each does what the manual says (`game.rs`); the joystick's words
  match its default axes and hat.

## Keys

| Key | Does |
|---|---|
| G | keyboard assist on / off (remembered) |
| ↓ ↑ ← → | with assist: nose up / down and bank, and they stay, up to 30° of nose and 60° of bank; held past that (or beyond it) they turn the aircraft on round, for loops, rolls, dives and spins (`sim_core::ASSIST_*`); without: move the yoke |
| Q E | with assist: nose, fine; without: trim down / up |
| W S, Page Up / Page Down | throttle |
| B | brakes, while held (with the throttle closed on the ground they brake a little by themselves) |
| F R | flaps down / up one notch |
| Shift+G, Shift+B, Shift+S | An aircraft with a gear that retracts, spoilers, reversers: the gear up / down (in the air), the speed brakes out / in, reverse thrust on / off (on the ground; then W for more) |
| A D | pedals |
| C | centre yoke and pedals (or click their box); with the autopilot engaged, disengage it (in the C172: AP DISC) |
| P | C172: the GFC 700's AP, holding the heading and the altitude of now (Shift+P: FD); while it is on, ← → move the heading bug 5° (Shift 1°) and ↑ ↓ the selected altitude 100 ft (Shift 1000), and it turns and climbs there; P again or C disconnect; see "The C172's autopilot". Extra: the game's autopilot on / off: holds the heading, altitude and airspeed of the moment (in flight only). While it is on, ← → move the heading 5°, ↑ ↓ the altitude 100 ft, W S the airspeed (90–155 kt), and the tag at the top right shows the three |
| 4 5 6, 7 8 9 | C172: HDG, NAV, APR; ALT, VS, FLC |
| , . | C172: NOSE DN, NOSE UP (the UP/DN wheel) |
| [ ], \\, ; ' | C172: the heading bug −1° / +1° (Shift: 10°), to the heading; the selected altitude −100 / +100 ft (Shift: 1000 ft) |
| / | C172: D→ the nearest aerodrome |
| L | autopilot: fly the circuit of the nearest airport, land into the wind and stop; then the aircraft is the player's again (in the C172 the GFC 700 lets go first) |
| N | outside view of the next aircraft (with `--traffic` or `--model`); the status line says who flies it; V returns to the player's cockpit |
| V | cockpit / outside view |
| + −, mouse wheel | in the cockpit, zoom in or out where the view looks, smoothly, and it stays (0.8 to 6 times); outside, the camera nearer or further, smoothly too |
| Space | the seat and leaning in, in turn; from any other spot back to the seat; outside, into the cockpit at the seat |
| I, Shift+I | the DIMMING knobs (panel's lights, displays at night, standby dials) brighter / dimmer |
| Z X | look left / right (outside: swing the camera) |
| 1 or Backspace, 2, 3 | back on the runway, start in flight, start on final |
| T, Shift+T | time of day forwards / backwards |
| H | help column |
| F2 | the replay of the last minutes; again, or Esc, back to the flight (see "The replay": Space, ← →, ↑ ↓ in it) |
| F9, F10 | time slower / faster: ×1, ×2, ×4, ×8, ×16 (more steps of the same length; see "Each step" above); the tag at the top right says it |
| F1 | the manual, with pictures (the game pauses; Esc or F1 closes it): see "The manual" |
| O, ⌘, | settings, a panel on the right beside the flight, which goes on, the keys still flying it (the panel is worked with the mouse); O again or Esc closes it. Opened from the start and pause screen the game waits under it: see "The menus" |
| M | the place picker, "Choose place" (the game pauses; Esc closes it): see "Real places" |
| K, Shift+K | altimeter setting (and the G1000's BARO) up / down 1 hPa |
| Y | altimeter set to the nearest aerodrome's QNH (the status line gives it) |
| J | heading indicator set to the compass (in steady, level flight: the compass errs in turns) |
| U, Shift+U | pitot heat; alternate static source (the C172 has both, the Extra neither) |
| F5, F6, F7, F8 | try a failure: pitot (blocked at the inlet, then sealed, then clear), static ports, vacuum pump, icing (only below +2 °C); the status line lists them |
| F3 | frames per second and milliseconds per frame at the end of the status line |
| F12 | record a video of the game as it is seen, with its sound; F12 again saves it in `~/Movies/Skylab` (the engine's, see `crates/engine/engine/README.md`, "Recording a video") |
| Esc | closes what is on top: the help, the settings, the aircraft, a replay; else pauses on the start and pause screen (see "The menus"): the arrows and Enter choose in it, Esc again or any other key flies on |

A multirotor (see "The FPV quad"):

| Key | Does |
|---|---|
| W S | throttle stick up / down (more power climbs, less sinks) |
| ↑ ↓ | pitch stick forward (nose down) / back |
| ← → | roll stick |
| A D | yaw stick |
| R, Enter | arm switch on / off |
| V | FPV camera / chase camera |
| 1, 2, 3 | on the ground here, hovering 15 m up here, hovering on the approach; in a race, the race again from the pad |
| L | in a race: the race's autopilot flies it, or gives it back |

Mouse: drag (or the right button) looks around (the setting "Natural mouse look" turns it round: the scene goes with the pointer); the wheel zooms the cockpit's view where it looks, and it stays (outside, the camera nearer or further); in the cockpit, dragging the throttle, the flap lever or the trim wheel moves it. In the C172 a click on a bezel's AFCS key presses it and dragging the HDG, ALT or BARO knob turns it (up or right clockwise; so does the wheel over it); Shift and the wheel over the HDG knob or an AFCS key turn the selected altitude (the ALT knob is at the bottom of the bezel, beside the yoke from the seat); the pointer is a hand over all of it, and what it is over is named, a knob (its edge lit) with what it is set to. Losing the window focus pauses.

## Controllers

Gamepads and flight sticks work without configuration, next to the keyboard and the mouse; the status
line names the ones in use. `src/controls.rs` decides what each does from its kind:

- **gamepad** (a device gilrs has in its layout database). Left stick: yoke, back is nose up. Right
  stick sideways: pedals. Right / left trigger: throttle up / down, at the rate of W and S times how
  far the trigger is pressed. D-pad down / up: trim nose up / down. A / B: flaps down / up. Bumpers:
  look left / right. Y: view. X: centre. Start: pause. D-pad left / right: time slower / faster. The stick moves the yoke away from where it
  was when the stick left the centre, by less the faster the aircraft flies (`gamepad_authority`:
  at 95 kt full stick is 0.4 of the ailerons, about 22° of bank per second, and 0.22 of the elevator,
  about 8° of nose per second). Back in the centre, the yoke returns to where it was and the keyboard
  has it again: with the assist the attitude is then held.
- **stick** (the name contains `16000`, `joystick`, `stick`, `hotas` or `flight`, or the device is
  not a known gamepad). Axis 0 / 1: yoke, always and at full travel, so the keyboard assist does
  nothing while one is connected. Axis 5 (twist): pedals. Axis 6 (the lever on the base): throttle
  as a position, forward for full; it sets the throttle when it moves, so the keys and the panel
  still work. Hat sideways: look; hat forward / back: trim nose down / up. Button 1 (trigger): view.
  Buttons 2 / 3: flaps down / up. Button 4 (right of the hat): leaning in and back, as Space. Buttons
  5 / 6 (the first two of the base): the view zoomed in / out, as + and −. Buttons
  7 to 10: the GFC 700's AP, HDG, ALT and FLC; 11 / 12: the heading bug left / right.
- **throttle** (the name contains `twcs`, `throttle` or `quadrant`). Axis 2: throttle. Nothing else.
- **radio**, an RC transmitter plugged in by USB as a joystick (the name contains `EdgeTX`,
  `OpenTX`, `RadioMaster`, `FrSky`, `Taranis`, `Jumper`, `TX16`, `TX12`, `Zorro`, `Boxer`, `Pocket`,
  `BetaFPV`, `TBS Tango`, `Mambo`, `FlySky`...), mode 2, as EdgeTX sends its first four channels:
  axis 0 roll, axis 1 pitch, axis 2 throttle, axis 3 yaw (the left stick throttle and yaw, the right
  pitch and roll); button 1 the arm switch (armed while it is on), button 2 an aircraft's flaps (F; it
  does nothing to a multirotor), button 3 the view. Flying an aircraft it is a stick (roll, pitch, yaw on the rudder, throttle).
  If an axis goes the wrong way, a `-` in front of it in the mapping file turns it over.

A multirotor takes the same axes as its sticks: roll, pitch (back is nose up), the rudder axis as
its yaw, the throttle as its throttle stick, a gamepad's left stick as its right one (roll and
pitch) at full travel; the throttle buttons (a gamepad's triggers) move the throttle stick; the
flaps-up button (a gamepad's B, a stick's 3) is R, its arm switch. Several devices work together; a stick has the yoke over a gamepad.

### Mapping file

`$XDG_CONFIG_HOME/flight/controls` or `~/.config/flight/controls`, next to the settings. A section
per device, `[name as the device calls itself]`, then `key=value` lines. The first time a device is
seen its section is added with the defaults; edit it and start the game again. A value left empty
maps nothing. Lines starting with `#` are comments. `defaults` is the version of the defaults the
section was written with: when the game brings newer ones (version 2: a stick's `panel`, `closer`
and `further`; version 3: its `autopilot`, `heading`, `altitude`, `level_change`, `heading_bug_left`
and `heading_bug_right`; version 4: a gamepad's `time_slower` and `time_faster` on the d-pad's left
and right), a section written before gets them when the file is read, only on keys it lacks or
leaves empty and only with buttons no other key of it uses; every line the player wrote stays.

```
[Thrustmaster T.16000M]
kind=stick
defaults=3
dead_zone=0.02
rudder_dead_zone=0.06
expo=0.2
roll=axis0
pitch=axis1
rudder=axis5
throttle=-axis6
look=DPadX
throttle_up=
throttle_down=
trim_nose_up=DPadY-
trim_nose_down=DPadY+
flaps_down=button2
flaps_up=button3
look_left=
look_right=
view=button1
closer=button5
further=button6
panel=button4
panel_lights=
centre=
pause=
autopilot=button7
heading=button8
nav=
approach=
altitude=button9
vertical_speed=
level_change=button10
nose_up=
nose_down=
heading_bug_left=button11
heading_bug_right=button12
direct_to=
```

| Key | Value |
|---|---|
| `kind` | `gamepad`, `stick`, `throttle` or `radio` |
| `defaults` | the version of the defaults the section was written with (3) |
| `dead_zone`, `rudder_dead_zone` | travel around the centre that does nothing, 0 to 0.9 (gamepad 0.08 / 0.08, stick 0.02 / 0.06, radio 0.01 / 0.01) |
| `expo` | 0 a straight response, 1 very fine near the centre: `(1 − expo)·t + expo·t³` (gamepad 0.5, stick 0.2, radio 0: the flight controller has its own rates) |
| `roll`, `pitch`, `rudder`, `throttle`, `look` | one axis. Positive is right, nose up, right pedal, full throttle, look right |
| `throttle_up`, `throttle_down`, `trim_nose_up`, `trim_nose_down`, `flaps_down`, `flaps_up`, `look_left`, `look_right`, `view`, `closer`, `further`, `panel`, `panel_lights`, `centre`, `pause`, `autopilot`, `heading`, `nav`, `approach`, `altitude`, `vertical_speed`, `level_change`, `nose_up`, `nose_down`, `heading_bug_left`, `heading_bug_right`, `direct_to`, `time_slower`, `time_faster` | buttons, separated by spaces (`closer` and `further` are the mouse wheel, `panel` the Space key, `panel_lights` the I key, the last twelve the GFC 700's keys) |
| `arm` | a multirotor's arm switch: armed while the button is held (a radio's switch is a button held down while it is on); without it, R arms and disarms |

An axis is a gamepad name (`LeftStickX`, `LeftStickY`, `LeftZ`, `RightStickX`, `RightStickY`,
`RightZ`, `DPadX`, `DPadY`; up is positive) or a device number (`axis0`, `axis1`...; as the device
sends it, where back is positive). A leading `-` turns it over. A button is a gamepad name (`South`,
`East`, `North`, `West`, `LeftTrigger` and `RightTrigger` for the bumpers, `LeftTrigger2` and
`RightTrigger2` for the triggers, `Select`, `Start`, `Mode`, `LeftThumb`, `RightThumb`, `DPadUp`,
`DPadDown`, `DPadLeft`, `DPadRight`, `C`, `Z`), a device number (`button1`...), or an axis pushed
one way past half its travel (`RightZ+`, `axis17-`).

`cargo run -q -p flight -- --controllers` lists the connected devices and prints, for 10 seconds, a
line each time one of them changes (axes by number as the device sends them, with gilrs's name in
brackets, and the buttons held), then the range each axis covered and the buttons seen. That is how
a new device's numbers are found. Nothing here has been tried with a real device yet.

## Command line

`--seed <n>` world · `--air` start in flight · `--final` start on final · `--pos <n> <e>` start in
flight over a point · `--ext` outside view · `--hour <h>` (over the computer's clock and the settings) · `--time <n>` time n times faster from the start (1, 2, 4, 8, 16; F9 F10) · `--help-shown` help column open · `--spot <n>` in the cockpit, the eye at spot n of `kinds::spots` · `--hover <x> <y>` with `--shot`, in a cockpit: the mouse's pointer at that point of the picture (what it lights and names on the panel) · `--look <yaw> [<pitch>]` in the cockpit, the head turned and held,
degrees right and up (to look at the cabin with `--shot`); with `--ext`, the camera that far round the aircraft and above it · `--traffic <n>` 0–6 aircraft flown by the autopilot ·
`--model <file>[:<task>]` an aircraft flown by a trained model, repeatable · `--quality <ultra|high|medium|low>` how much is drawn
(see "Drawing"; without it, what the settings file says) · `--fps` start with the frame rate shown (F3) ·
`--shot <dir>` write `<dir>/scene.png` (1600×1000, overlay included) and exit · `--retina` with
`--shot`: 2880×1800 at two pixels per point (the 1440×900 window as a Retina screen shows it) · `--size <w>x<h>` with `--shot`: a picture of that size instead (a screen as a small window shows it) ·
`--aircraft <c172|extra|fpv|...>` what is flown (a key or alias of a type of aircraft or multirotor of `assets/aircraft`) (without it, what the settings file says, or the C172) ·
`--remote [<port>]` other programs can fly the game: it listens on 127.0.0.1 (port 7448 unless given) and starts without the start screen (`crates/games/remote/README.md`) · `--controllers` list the game controllers and what they send, for 10 seconds · `--low <m>` starting in flight, this many metres above the ground instead of 900 · `--crash <s>` with `--shot`: dive into the ground ahead and show the wreck `<s>` seconds later ·
`--damage <tip|gear|wing|flutter|props>` with `--shot`: break the aircraft first, by putting it where that
gives way (`Session::damage_ahead`): a wing tip dropped onto the runway, a touchdown that collapses
the gear, with `--air` the outer left wing and half the right tailplane torn off, or a dive until
a surface flutters away; in a quad (`--aircraft fpv`), a broken and a chipped propeller. `--fail <pitot|inlet|static|vacuum|icing>[,...]` with `--shot`: fail
instruments first (`instrument_keys::fail`) · `--wait <s>` with `--shot`: fly `<s>` seconds more
before the picture (`--air --fail vacuum --wait 600`: the standby horizon run down beside a
working PFD). `--afcs <what>[,...]` with `--shot`, in the C172: work the GFC 700 first and fly the
wait with it: its keys by name (`ap fd hdg nav apr alt vs flc up dn dto sync disc`), `bug=<deg>`,
`turn=<hdg|alt|alt1000|baro|crs>:<clicks>` (a knob turned),
`sel=<ft>` (with a sign: from the heading or the altitude of now), `throttle=<0..1>`
(`--air --spot 2 --afcs ap,bug=+60,hdg,alt --wait 6`: the PFD turning in HDG and ALT).
`--place <ICAO>` fly from a real aerodrome (the middle of its main runway is the world's origin; the
player starts on that runway) · `--at <lat>,<lon>` a real place round that point, starting in flight
over it · `--offline` with them: only what is in the cache · `--picker` the place picker open at the
start, with `--map <lat>,<lon>,<zoom>` where its map looks, `--search <text>` what is typed in it and
`--select <ICAO | lat,lon>` what is chosen (for `--shot`). See "Real places". `--manual <page>` the manual open at the start at that page, 1 the first (for `--shot`). `--menu <which>` a menu open at the start: `vehicles` (the first screen: planes, drones or cars), `home` (the start and pause screen), `aircraft`, `settings` or one of its sections (`weather`, `flight`, `graphics`, `about`); `--clean` nothing written over the picture and `--zoom <lines>` the outside camera nearer (the menus' pictures: "The menus").
`--course <name>` race a course of `assets/courses` in its quad (see "Race courses"); `--racer`
with it, the race's autopilot flies (for `--shot` with `--wait`, which flies the race as the game does).
`--land` with `--shot`: L first, the autopilot lands at the nearest aerodrome; `--replay <s>` with
`--shot`: after the wait the replay (F2), stopped `<s>` seconds before the end (`--final --land
--wait 100 --replay 12 --ext`: the touchdown seen from outside, with the debrief).

## Real places

The world is the seed's unless a real place is asked for: `--place`, `--at`, or the place picker
(below). `geodata` downloads its maps the first time (some 20 to 50 MB, half a minute) and keeps
them, and the region built from them, in the cache (`~/Library/Caches/flight` on macOS); from then
on a place loads at once, offline too. From the command line, or the place saved in the settings,
it is loaded before the window opens, its progress on the terminal (`places::load_now`); if it
cannot be had the game says why and flies the generated world. Chosen in the picker while flying,
it loads on a thread of its own (`places::Loading`, which also makes its `Terrain`), its progress
in the middle of the screen and in the picker and the settings, and the game goes on
meanwhile; once it is whole, `Session::move_to` makes the world again on it (same weather, hour,
aircraft and amount of traffic; the trained models are left behind), `Session::start_at` puts the
aircraft where the picker said, and `WorldRenderer::set_terrain` draws it. That takes 2.3 ms of the
frame it happens in (Barcelona, the biggest of the three).

Once a place's world is made, the satellites' picture of its ground is made on another thread
(`places::Photos`, `geodata::imagery`: 90 to 200 MB the first time, half a minute to a minute; then
from the cache) and given to the renderer (`WorldRenderer::set_imagery`) when it comes; the ground
has the colours of its maps meanwhile, and keeps them if it cannot be had. `--shot` waits for it.

Flown beyond its region, a place goes on (`places::Following`, `geodata::follow`): when the
aircraft, or where it will be in three minutes, is 30 km from the middle of its region, the region
round a point 24 km further along is built on a thread and, once it has come, the world flies on
it (`World::terrain` replaced under the aircraft, in the same axes, the old one freed on a thread)
and the renderer draws it without dropping anything (`WorldRenderer::replace_terrain`); its
ground's picture follows. It is kept, so flown again, and offline, it loads from the cache.


| | Menorca (LEMH) | Innsbruck (LOWI) | Barcelona (LEBL) |
|---|---|---|---|
| first time: downloaded, built | 23 s, 6 MB (and the 17 MB tables of aerodromes, once) | 27 s, 50 MB | 22 s, 22 MB (and the tables) |
| kept on disk (tiles, region) | 7 MB, 74 MB | 50 MB, 80 MB | 23 MB, 83 MB |
| from the cache (built again from the tiles kept) | under 0.1 s (0.5 s) | under 0.1 s (0.5 s) | under 0.1 s (0.6 s) |
| largest memory of the game, flying it (building it first) | 787 MB (1057 MB) | 664 MB | 874 MB (1111 MB) |
| median frame, `--shot` | 4.8 ms on the runway, outside | 7.9 ms on the runway, cockpit (the valley and its mountains fill the view) | 6.4 ms 900 m up, 6.7 ms 400 m over the city |

(Apple M5 Pro, 1600 × 1000; the generated world's default `--shot`, 3.6 ms and 580 to 710 MB.)

### The place picker, "Choose place"

M, "Place" on the start and pause screen or at the top of the settings, or at the start if the
player ticks "Open this screen at start" (`picker_at_start`). The featured places and the race
courses are cards with their pictures (`assets/menu/places`), and the featured places are pins on
the map as well, with their picture under the pointer. It covers the window (the world is not drawn under it)
and the game waits. `src/picker/`:

- **Search** (`geodata::search`): the aerodromes of OurAirports an aeroplane can fly from (48 080),
  read from the cache on a thread of their own when the picker first opens (0.25 s; downloaded the
  first time). A code typed whole (ICAO, IATA, GPS) comes first, then the town, then the name, any
  case and without accents, every word typed starting one of the aerodrome's words (`barcelona
  estados` finds the Barcelona of New Hampshire); among equals the bigger aerodromes, with airlines,
  the longer runway. Each with its codes, town, country, size and runways. 1 to 2 ms a
  search.
- **Map** (`map.rs`, `tiles.rs`): Web Mercator, dragged with the mouse, zoomed with the wheel, two
  fingers or a pinch (about the pointer), double click or the + − buttons, from the whole world
  (zoom 1.3 in a 1600-point window) to an aerodrome (17). OpenFreeMap's vector tiles of zoom
  `floor(zoom)` up to 14 (`geodata::basemap`: the same cache as a region's tiles, downloaded once and
  politely), each decoded and triangulated once on a thread of its own (`earcutr`), six at a time,
  kept in memory up to 256 MB (the least used lately let go); while one loads, the nearest ready
  tile above it is drawn. A flat, dark style: land, water, woods, fields, ice, sand, towns,
  aerodrome grounds; rivers, borders of countries and regions, roads by class (dim and thin far out);
  names of countries, cities, towns and villages by zoom, in English when the map has it, in Latin
  letters otherwise, never overlapping. Drawn with egui shapes, not a pass of our own: each frame the
  tiles' triangles are only moved and scaled into one mesh and their lines drawn by egui (smooth at
  any zoom), which is little (below) and draws the same in `--shot` and on the web. The aerodromes on
  it as dots by size (large from the world view, medium from zoom 4.5, small with a runway from 7,
  without from 9.5), their codes where there is room, the name too from zoom 11; their runways from
  10.5 at their real width, and the idents of their ends from 13.5. A click on an aerodrome chooses
  it (the pointer over one says what it is), anywhere else chooses that point; the square a region
  will cover (96 km) is drawn round what is chosen.
- **Lists**: favourites (the star) and recent places (the last 8 flown), then the featured ones: the
  generated world and `geodata::PRESETS` (Menorca, Innsbruck, Barcelona, Sogndal by the Sognefjord,
  Lofoten, Saba, Lukla, Courchevel, Madeira, Hong Kong, San Francisco, Queenstown). Each says
  "downloaded" if its region is kept, or about how many MB it still needs (`geodata::cache::need`,
  which counts the tiles missing from the cache; three places worked out a frame).
- **Start** (`places::Start`, `Session::start_at`): on the runway, at the end into the wind (of all
  the aerodrome's runways, `places::into_wind`) or the one chosen; on final at 1 to 10 NM of that
  end, on its 3° slope; or in the air over the place, 500 to 12 000 ft above the ground, into the
  wind (the only start at a point). "Fly" closes the picker, remembers the place (`place`,
  `places_recent`) and starts there: at once if it is the place flown, else once it has loaded.

On the M5 Pro, 1600 × 1000: the picker takes 0.2 to 1.3 ms of the CPU a frame (the whole frame 1.6
to 4.7 ms; Catalonia at zoom 8.6, the busiest, the most); the game at its peak 230 to 240 MB in it
(570 MB over Catalonia at zoom 8.6), of which the tiles kept are 1 to 10 MB.
`--picker` opens it at the start, and `--shot` draws it, with `--map <lat>,<lon>,<zoom>`, `--search
<text>` and `--select <ICAO | lat,lon>`, and says those costs.

## Settings file

`$XDG_CONFIG_HOME/flight/settings` or `~/.config/flight/settings`, `key=value` lines: `windFrom`,
`windSpeed` (kt), `turbulence` (%), `hour`, `visibility` (km, 0.2 to 200, 150 by default: below 5 a mist, below 1 a
fog), `humidity` (%), `cloudCover` (%), `cloudBase` (ft), `qnh` (hPa) and `temperature` (°C at sea
level): the weather of the world (`World::weather`; the aircraft fly in its air, the instruments sense
it, the world draws its haze),
`assist`, `quality`, `aircraft` (the key of a type: `c172`, `extra300`), `place` (an ICAO code or `lat,lon`; empty, the
generated world), `places_recent` and `places_favourite` (places as `place` writes them, separated by
spaces, the latest first), `best_lap_<course>` (the best lap of a race course, seconds), `picker_at_start` (`true` opens the picker at the start), `menu_at_start` (`false`: the game opens in the cockpit, not on the start screen), `local_time` (`false`: the game opens at `hour`, not at the hour of the computer's clock), `replay_minutes` (minutes the replay keeps, 1–30, 10 by default). Read at start,
written when a slider, the quality, the aircraft, the place, a favourite, the picker's box or the
assist changes. `--shot` ignores it.

## Drawing

The world (terrain, sky, clouds, sea, scenery, aerodromes, wrecks and fire) is `world-render`'s:
its README has the contract, the qualities, the photographs and the scenery. `render::Renderer`
holds a `world_render::WorldRenderer` (`renderer.world`; the settings writes its `cloud_*`,
and the world's `Weather` goes to it each frame) and puts the aircraft into it:

- At the start it gives the world each kind's mesh and its wreck cut into pieces
  (`add_mesh`, `add_wreck_shape`; a textured model's with its look, `add_look` and `add_textured_mesh`), and the C172 parked on the aprons (`models::parked_aircraft`).
- Each frame (`Renderer::prepare`, with a `render::Scene` from `game.rs`) it turns each aircraft's
  parts (surfaces about their hinges, legs on their springs, the propeller) into `ObjectDraw`s, piece
  by piece (`wreck::regions`) with what `Damage::left_of` says is left of each: a wing or the
  tailplane torn short is drawn with the `Cut` of its tear (the world shades its ragged edge and the
  inside of the skin), a surface torn off is not drawn and a loose one trails, fluttering; a
  collapsed leg is folded about where it is fixed, and the lights on what came off are out. It turns its lights into `Lamp`s and the propeller's blur into a see-through draw,
  and the same parts, whole and the fast propellers left out, into `CasterDraw`s, each aircraft a
  `Caster` round its centre of gravity (the player's first); and makes the player's aircraft the
  world's `Body`: its `ShadowShape` (`kinds::shadow`, none once in pieces) and the axes the cockpits'
  shaders light the cabin by.
- The cockpit from the seat is drawn with a pipeline of its own on the world's shaders and groups
  (`cockpit/cabin.wgsl`), between the world's sky and the rest (`Renderer::draw`), so what it covers
  is not shaded, and its glass after the world, blended over it.

`--quality` and the settings choose a `world_render::Preset` (ultra, high, medium, low; see the
world's README); the game hands its `samples` to the engine.

`cargo run --release -p flight --example world -- <out.png> [ext] [at=<n>,<e>,<agl>] [cam=<n>,<e>,<agl>,<heading>,<pitch>] [orbit=<m>,<bearing>,<elevation>] [fov=<deg>]`
draws the world with the aircraft from any camera (its shadow on a roof, say; `orbit` a close-up of
it from a bearing off its nose); `--example models --
aircraft side out.png` draws one model alone.

## Checking a change

- `cargo test -p flight`: keys, pause, views, texts, settings file, the synthesiser's levels,
  controllers (shaping, mapping file, a synthetic device flown through `Session`).
- The cockpits' shaders on the world's of every quality, without a GPU: `cargo test -p flight
  the_cockpits_shaders_are_valid_wgsl`. The world's own: `cargo test -p world-render`.
- Anything visible: `cargo run -q -p flight -- --shot /tmp/x [--ext] [--air] [--help-shown]` and
  look at `/tmp/x/scene.png`. To prove a change leaves the picture alone, take it before and after and
  `cargo run --release -p engine --example compare -- before.png after.png`: trees may move a
  handful of edge pixels, nothing else may differ.
- Speed: `--shot` prints the milliseconds of a frame over 240 frames drawn after 60 of warm-up (a
  GPU that has been waiting between frames runs slow for the first twenty): the mean, and the
  fastest, quartile and median single frame. Compare medians of several runs, taken in turn with
  the other build, with no other game window open: one open window doubles the figure. `ENGINE_PROFILE=1`
  adds where the time went.
- Sound has no test beyond levels: run the game and listen.
