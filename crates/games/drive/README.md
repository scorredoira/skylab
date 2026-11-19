# drive

The driving game, `skylab-drive`: a car on a race circuit of the generated world, driven by the
player or by the racing driver of `sim-core`, drawn into the same world as the flight game with
`world-render` (which also draws the circuit: `world_render::TrackRenderer`), timed lap by lap,
heard; in free practice with the ghost of the best lap, or in a race against rivals from a standing
start; dry or in the rain; shown again in the replay. It is the second game on `engine`, `world-render` and `sim-core`; no code of the flight game
is in it.

Run it with `scripts/drive.sh [options]` (a release build, killed if it passes 4 GB). It is also
the "Cars" of Skylab's first screen, which starts this program in its place; the pause menu's
"Change vehicle" goes back there ("The pause menu").

```text
skylab-drive [--circuit <name>] [--car <name>] [--paint <colour>] [--hour <h>] [--view <chase|bonnet|cockpit|trackside>]
             [--driver] [--at <m>] [--quality <ultra|high|medium|low>] [--cover <%>] [--rain [<0-1>]] [--no-lens]
             [--race [<laps>]] [--rivals <n>] [--rival <driver|net|<file>.fltmodel>] [--grid <k>] [--seed <n>]
             [--manual] [--no-abs] [--no-tc] [--no-esc] [--orbit <deg> [<deg> [<m>]]] [--stunt <launch|lock|slide|run>] [--menu]
             [--shot <dir> [--wait <s>] [--size <w>x<h>] [--hud] [--frames <n> [--sound <wav>]] [--replay <s>]]
```

| Option | |
|---|---|
| `--circuit` | the circuit, a file of `assets/circuits` (default `lakeside`) |
| `--car` | the car, a folder of `assets/cars` (default `gt`; `modely`, the Tesla Model Y, electric: `D`, `R`, `N` with the gear keys, the pedal let go regenerates) |
| `--paint` | its paint: `red`, `blue`, `orange`, `green`, `yellow`, `white`, `silver`, `grey`, `black`, or `rrggbb` (default: the model's) |
| `--rain [<r>]` | it rains, `r` from 0 (dry) to 1 (a downpour), 0.6 if not given: the sky closed, the road wet (it mirrors the sky and what stands on it, standing water, spray), drops falling, from the seat drops on the glass and the wipers going, and the grip less: a film of water on the asphalt, puddles in its hollows that make the tyres aquaplane at speed, kerbs and painted lines slippery (`sim_core::car::wet`). `F6` and `F7` in the game: more rain, less; when it stops the road dries in some ten minutes, the racing line first |
| `--race [<laps>]` | a race (3 laps by default) against rivals (5 unless `--rivals`) from a standing start: lights, positions and gaps, the flag, the results |
| `--rivals <n>` | rivals on the grid, up to 11 (0: free practice, alone with the ghost) |
| `--rival <who>` | what drives the next rival, repeatable: `driver` (the racing driver, each with a character of its own), `net` (the trained network, `models/drive.fltmodel`: `trainer/DRIVE.md`) or a model file |
| `--grid <k>` | the player's place on the grid, 0 the pole (default: at the back) |
| `--seed <n>` | the rivals' characters (pace, braking points, line, mistakes) and the pause before the lights go out: the same seed, the same race |
| `--line <full\|braking\|off>` | the driving line (`src/guide.rs`): arrows on the road along the racing driver's line for 240 m ahead, green where the car can go faster, yellow about the line's speed, red where it must be slower by then than it is now (the red begins where the braking must); the wet road's speeds in the rain. The whole of it by default, `braking` only where to slow down; `L` in the game goes through the three. Not in a `--shot` unless given |
| `--no-lens` | without what the lens adds ("What the lens does"), to compare pictures and frame times |
| `--hour` | 0 to 24 (default 17.5, the golden hour); `N` and `Shift+N` move it in the game |
| `--view` | the camera to start with |
| `--driver` | the racing driver drives; `X` takes over or hands back |
| `--at <m>` | start on the racing line this many metres past the start line, at the line's speed |
| `--manual` | the sequential gearbox (`M`) |
| `--no-abs`, `--no-tc`, `--no-esc` | without anti-lock brakes, traction control, stability control (on by default, as in the road car; `B`, `T`, `Y`) |
| `--orbit <y> [<p> [<m>]]` | the chase camera turned round the car and up, degrees, and how far from it (6.2 m) |
| `--stunt <what>` | for pictures: what the car does by itself while `--wait` runs, nobody driving: `launch` (full throttle from rest: the tyres spin), `lock` (the brakes hard on, no anti-lock), `slide` (a turn on the hand brake with the throttle open), `run` (24 s for the sound, from the grid: idling, full throttle up through the gears, lifting off, the brakes locked, a slide on the hand brake) |
| `--menu` | start on the pause menu (`Esc`); with `--shot`, to see it |
| `--shot <dir>` | one frame to `<dir>/drive.png` once the world is drawn, without a window: `--wait <s>` simulates that long first, `--size` (1600x1000), `--hud` with the overlay; it prints the milliseconds of a frame (the fastest, the quartile and the median of 90). With `--frames <n>` it then goes on driving and writes `n` more frames, a sixtieth of a second apart, as `<dir>/00000.png`...: a video's frames (`scripts/video/assemble.sh` joins them); `--sound <wav>` also renders their sound off line (48 kHz stereo, the starter first if the car stands) and writes what it followed, a line a frame, to the same name with `.csv` |
| `--replay <s>` | with `--shot`: after the wait, the replay (`F2`), stopped `s` seconds before the end of what is kept |

Pictures: `scripts/drive.sh --shot /tmp/d --driver --at 1150 --wait 2 --hour 18.2` (chase, golden
hour); add `--view cockpit`, `--view trackside` (a long lens, the background out of focus),
`--hour 12.5`, `--hour 22` (night, under the floodlights), `--rain`; the car alone:
`--orbit 150 5 4.2`; smoke and marks: `--at 700 --stunt slide --wait 1.6 --orbit 30 18 10`. A video
with its sound: `scripts/drive.sh --shot /tmp/r --stunt run --frames 1440 --sound /tmp/r.wav --size
960x600 --hud`, then `ffmpeg -framerate 60 -i /tmp/r/%05d.png -i /tmp/r.wav -c:v libx264 -pix_fmt
yuv420p -c:a aac -shortest /tmp/r.mp4` (add `--view cockpit` or `--view trackside`). The start
of a race: `--race --shot /tmp/d --wait 4.5 --hud`; in it, in the rain: `--race --driver --grid 2
--wait 22 --rain 0.7 --hud`; the replay from the trackside: `--race --driver --wait 30 --replay 12
--view trackside --hud`; the ghost: `--driver --wait 300 --hud`.

## Keys

`↑`/`W` throttle, `↓`/`S` brake, `←` `→`/`A` `D` steer, `E` `Q` shift up and down, `Space` the hand
brake, `C` the camera, `R` back on the track where the car is (`Shift+R`: to the grid), `X` the
racing driver, `M` automatic or sequential, `B` anti-lock brakes, `T` traction control, `Y` stability control (all three on to start with), `G` the pedals' help, `L` the driving line, `N` the hour,
`F2` or `Tab` the replay, `[` `]` the car the cameras watch, `F6` `F7` more rain or less, `P` pause, `H`
the keys, `Esc` the pause menu, `F12` a video of the game with its sound (again: saved in
`~/Movies/Skylab`, as Skylab's; the engine's, see its README, "Recording a video"). A key steers towards full lock at a rate, and less far the
faster the car goes (as hands do); the pedals go down and come up at a rate too. A pedal's key is
all or nothing and a foot is not, so a key held is a foot that feels the tyres (`G` switches it
off): the throttle goes down as far as the driven tyres grip, eased while they spin past a slip of
0.10 or the tail slides out, and the brake as far as no wheel locks past 0.11. It matters with
traction control or the anti-lock brakes off (with them on the tyres never get that far): the
player of `src/feel.rs` then laps in 122 s; with the keys all or nothing it spins every lap.

## The pause menu (`src/pause.rs`)

`Esc` (a pad's Start) stops the game and shows three rows: **Drive on**, **Change vehicle** and
**Quit**. The arrows and Enter work it, a pad's direction pad or left stick and A, or the mouse;
`Esc` or B drives on. "Change vehicle" starts Skylab, the flight simulator, on its first screen
(planes, drones or cars) and ends this game: Skylab is the program `skylab` beside this one
(`engine::program`: where `cargo build` puts both, and in the macOS bundle), started with
`--menu vehicles`. The row is there only if that program is. In the replay `Esc` goes back to the
session, as before.

## Controllers (`src/controls.rs`)

Every gamepad and joystick has a section in `$XDG_CONFIG_HOME/skylab-drive/controls` (or
`~/.config/skylab-drive/controls`), named by the device, written with its defaults the first time
it is seen: `kind` (`gamepad`, `stick` or `wheel`), the axes `steer`, `pedal` (forward the throttle, back the
brake), the triggers `throttle`, `brake`, `clutch`, and the buttons `shift_up`, `shift_down`,
`handbrake`, `camera`, `restart`. Axes by gamepad name (`LeftStickX`) or by number (`axis0`), `-`
turning one over; buttons by name (`South`) or number (`button1`). A gamepad: left stick to steer,
right and left triggers throttle and brake, the bumpers shift, `South` the hand brake. A flight stick
(the Thrustmaster T.16000M): its X axis steers, forward and back are the pedals, the trigger shifts
up, button 2 down, button 3 the hand brake, button 4 the camera. A steering wheel with pedals is a
section of `kind = wheel` with its own axes: the front wheels turn as it is turned, nothing shaped,
cut with speed or smoothed (force feedback is to come). A stick's or a pad's steering is
shaped: a dead zone, then a strong expo (fine in the middle of the travel, full lock at its end),
the lock cut with speed as the keyboard's (about half at 100 km/h), and the wheels follow the stick
over 0.08 s. A gamepad's thumbstick, flicked from end to end, is cut harder (half the lock at
72 km/h, a third at 100) and followed more slowly: out over half a second, back faster. With the automatic, the car stopped, the brake held (the stick back, or `↓`) puts it in
reverse after 0.3 s; backing, back is the throttle and forward the brake, and forward held, stopped,
puts it back in first.

## How it is made

| File | What |
|---|---|
| `src/main.rs` | the command line, the window or `--shot` |
| `src/game.rs` | `Drive`: the world with the circuit (`circuit_world`), the car, stepping it at 240 Hz (the car sub-steps at 960 Hz), the keys, what is drawn each frame and in what order, what the lens does (`post`) |
| `src/look.rs`, `src/car.wgsl` | the car's model (`assets/cars/<model>/exterior.glb`) drawn as the car is, by the game's own shader on the world's ("How the car is drawn", below); its shadow: its model as it is (`CarLook::casters`: the body, the cabin, each wheel steered and on its spring; not the glass, which lets the sun in) drawn into the world's map of the vehicles' shadows, and a `ShadowShape` for the lowest quality and for the tyres' and the floor's darkening of the road where they stand (body and cabin flat and standing, the tyres as solids, the floor as a flat one) and its headlights as they are seen (what they light is the circuit's to draw: `world_render::TrackRenderer::beams`) |
| `src/effects.rs`, `src/effects.wgsl` | what the tyres leave, from what the physics says each wheel does: marks of rubber on the asphalt, smoke, dust, spray; the rain's drops ("What the tyres leave") |
| `src/camera.rs` | the chase camera (a critically damped spring behind the car: it swings out in turns and closes up braking, looks into the corner, its lens fixed), bonnet, cockpit (the head turned into the corner), all three trembling on kerbs and off the asphalt; trackside cameras every 300 m (one low by the kerb, one on a tower, one high and far, in turn) that follow the car with a long lens focused on it |
| `src/controls.rs` | keyboard, gamepads, joysticks and steering wheels to `CarControls`, the mapping file |
| `src/feel.rs` | a test that drives as a player at the keyboard does (keys held and tapped, all or nothing, through `controls`): it laps Lakeside cleanly with the aids on, within ten seconds of the racing driver; `cargo test --release -p drive feel -- --nocapture` prints its laps |
| `src/audio.rs` | the sound, mixed from recordings ("The sound", below) |
| `src/timing.rs` | laps, sectors, the best lap, track limits (a lap with the car's middle beyond the kerbs does not count for the best) |
| `src/hud.rs` | speed, gear, revs with the shift light, the lap and sectors, the last and best laps, the aids |
| `src/race.rs` | the race (`sim_core::circuit::race::Race`) and what is drawn of it: the lights, position, lap, order and gaps, results; the replay's bar, the ghost's gap, the rain (`overlay`) |
| `src/rivals.rs` | the rivals: their cars on the grid, names, paints, characters, and what drives them (the racing driver, or a network with the racing driver watching the traffic) |
| `src/replay.rs` | the replay: what is kept of every car, and the playback |
| `src/guide.rs`, `src/guide.wgsl` | the driving line: arrows on the asphalt ahead along the racing line, coloured by the car's speed against the line's |
| `src/ghost.rs`, `src/ghost.wgsl` | the ghost of the best lap: recorded, kept on disk, driven again see-through |
| `examples/import_car.rs` | makes a car's `exterior.glb` from a model made elsewhere (`assets/cars/<key>/import.ron`: the model, its axes, its materials as the game's, which make the wheels): `cargo run --release -p drive --example import_car -- assets/cars/modely/import.ron`; it prints where the model has the axles, the tracks and the tyres' radius, for `car.ron` |
| `examples/car_model/` | makes `assets/cars/gt/exterior.glb` ("The car's model"): `cargo build --release -p drive --example car_model && target/release/examples/car_model assets/cars/gt/exterior.glb` |

## The car's model (`examples/car_model/`)

The GT coupé is an original two-door fastback of no make, made by a program, so it is data anyone
can change and make again (MIT, nothing from anywhere else). Its shapes are fields (signed
distances: `sdf.rs`): the body is what its outline seen from the side and its outline seen from
above share, rounded where they meet, with crowned bonnet and deck, wings swelling over the
wheels, a scallop along the flanks, a greenhouse leaning in to the roof, and the arches, the mouth
and intakes, the diffuser's hollow, the tail's vents and the number plate's recess cut out
(`body.rs`); the rims, calipers, mirrors, seats, dashboard, steering wheel and the driver's
gloved hands are fields too (`wheel.rs`, `cabin.rs`). The tail, which the chase camera looks at,
has its own form: the deck ends in a ducktail's lip; the lamps' band is set 11 mm into the shell
(a black bezel, a red lens, a line of light along its top, the brake lights' block towards each
corner); under it a panel set back 24 mm between the haunches with the plate in its hollow; the
bumper's top a shut line; a black valance with two reflectors, the diffuser's hollow (in the
tail's shade), eight raked fins and two pipes with a burnt inner wall. A field becomes triangles by surface nets (a vertex in each cell of a
grid the surface crosses, moved on to it, the field's gradient as its normal: the shading is as
smooth as the field whatever the grid). Glass, lamps, the black trim and the shut lines are
outlines drawn on the surface from the side, the front or above, along which its triangles are
split, so their edges are crisp. Each vertex carries how open it is to the sky (ambient
occlusion, from the field), as its colour.

The nodes: `body`, `glass`, `cabin`, `steering` (about its own centre), `dial_rev` and
`dial_speed` (the dials' faces), `hand_left` and `hand_right` (the hands on the rim, in the
steering wheel's axes), `arm` (a forearm a metre long, stretched from wrist to elbow),
`wheel_front` and `wheel_rear` (about the hub: tyre, rim, the
spokes apart, and the disc they blur into), `brake_front` and `brake_rear` (disc, bell, caliper).
Each triangle's glTF material says what it is made of by its name: `paint`, `glass`, `trim`,
`gap`, `grille`, `carbon`, `fin`, `plate`, `chrome`, `burnt`, `mirror`, `lamp_ring`, `lamp_head`,
`lamp_tail`, `lamp_brake`, `lamp_lens`, `lamp_reflex`, `lamp_dark`,
`rubber`, `rim`, `spokes`, `blur`, `disc`, `caliper`, `dash`, `seat`, `glove`, `sleeve`, `dial`... (`MATERIALS` in
`main.rs`); its colour, metallic and roughness are what the shader draws it with. A second
argument is the grid of the shell in metres (0.018; finer is smoother and heavier).

## How the car is drawn (`look.rs`, `car.wgsl`)

The game's own pipelines, on the world's shaders (the same sun, sky, clouds, shadows and haze as
the ground the car stands on); the solid parts between `draw_sky` and `draw`, the see-through
ones (glass, a turning wheel's blur) after the world, which is then behind them. A material's name
says its kind:

- **Paint**: a metallic base (the colour, deepest head on and darker to the flanks; the flakes'
  highlight in the paint's own colour, a tight one in a wide one, kept under what the tone
  mapping turns pale so that a red stays red at midday; flakes that glint one by one from close)
  under a clear coat: a mirror with Fresnel's strength, in which the sky is seen with its clouds
  (`car_sky`: white where the sun lights them, with firmer edges than their shade has, and
  clouds of noise beyond the map of that shade, low over the horizon where a bonnet looks), a
  dark band of land at the horizon and the ground below, and the sun as a small hot highlight
  that the bloom catches.
- **Its shadow on itself** is the world's map of the vehicles' shadows (`vehicle_shadow`): the
  mirrors on the doors, the roof on the seats seen from inside, the sun through the windows. What
  the body shuts in (the valance, the diffuser, the wells), finer than the map tells, the sun does
  not reach either. Black trim is rough, so that a low sun behind the eye does
  not turn it tan.
- **Glass**: the same mirror over a tint that lets a third of the light through: the cabin shows
  inside. From the seat it is nearly clear.
- **Lamps**: glossy lenses that give off light by what the car does: the ring of the head lamps
  always, the beams from dusk, the tail lamp's line brighter at night, the brake lights' cells
  (parted by the shader) shining when the brakes are on. The number plate is lettered by the shader.
- **Wheels**: rubber; alloy that mirrors; above 16 rad/s the spokes give way to a see-through disc
  (they would jump from frame to frame); the brake discs glow from dull red to orange as they
  heat with the work they do and cool in the air.
- **The inside**: lit by the sky through the glass and a little by the sun (by night, by the
  floodlights and by the dials' glow); the shell seen from within is the lining: grey cloth on
  roof and pillars, door cards of dark leather with a red insert between two seams and an
  aluminium pull. The dashboard has a seam and an aluminium strip, the seats a quilted middle.
  The dials' marks, numbers (thousands of revs; km/h), red line and needles are drawn by the
  shader, the needles where the revs (to 8000) and the speed (to 320 km/h) are, the gear in the
  revs' face.
- **The driver's hands** (from the seat): on the rim at a quarter to three, turning with it as
  far as hands do (100 degrees either way; beyond, the rim slides through them); the one on the
  side of a gear change slides in and bends its fingers to the paddle; a forearm from each wrist
  to an elbow that stays put.
- **The mirrors**, from the seat, show the road behind: each is turned so that the driver sees
  straight back in it, and along that ray the shader draws the sky and its clouds, the trees'
  line and the circuit's own ground as it is where the car is (asphalt, edge lines, kerbs where
  it bends, grass, bending away with the circuit's curvature). Not the barriers, the stands or
  other cars: it is not a second picture of the scene.
- **Rain on the glass**, from the seat: the glass is drawn in the frame's second pass with the
  scene the first left; a mist blurs what is behind it, drops form, stand, run down and go, each
  a small lens with the scene turned over in it, and two wipers sweep the windscreen every 1.4 s
  and leave their part clear until drops form again.
- By night under the circuit's floodlights the car is lit by what light there is where it stands
  (`world_render::track::floodlight`), most of it from the brightest tower, where that stands:
  its rack of lamps is a bright patch in the clear coat.

## The sound (`audio.rs`, `assets/cars/<model>/sound/`)

Made of real recordings, as racing games make it: a car's folder has a `sound/` with WAV files, a
`sound.ron` that says what each is, and a `CREDITS.md` (whose they are, under which licence, what
was done to them; all CC0). The GT's engine is a real one on a dynamometer (a Mercedes-Benz 190E
2.3-16V, the only free set of steady-rev recordings found), from two microphones: at the exhaust
and in the engine bay.

- **The engine**: loops recorded at steady revs, on load (full throttle) and off load, each a whole
  number of engine cycles at one measured pitch (`rpm` in `sound.ron`: its firing frequency x 120
  / cylinders). At every sample, of each set the two loops nearest the revs play, each resampled
  to the exact revs (read at the revs over its own, with cubic interpolation), crossfaded with
  equal power by where the revs are between theirs. On and off load are blended by the engine's
  torque over its most: lifting off, a gear change's fuel cut and engine braking are heard as the
  physics has them. The limiter chops it some twenty times a second. Its loudness against the revs
  is in `sound.ron` (`on_level`, `off_level`). At the start the starter turns it over and it catches.
- **The tyres** (from `WheelState`): a squeal from how fast they slide sideways on a hard surface,
  times their share of the load; a harsher scream from wheels spinning or locked (sliding along);
  their roll on asphalt by the speed; on a kerb the rumble of its ridges, played at the rate they
  pass under the wheels (one every 0.7 m); gravel; grass and earth (the gravel, slower and duller).
- **The wind** by the speed; **impacts** (the hardest of each frame, louder and lower the harder)
  and **scraping** along a wall.
- **Where it is heard from** (`Listener`): behind the car mostly the exhaust; on the bonnet mostly
  the engine bay; from the seat the bay's microphone through the cabin (low-passed, quieter; the
  road louder, the wind less); from a trackside camera fainter with the distance, duller with the
  air (a low-pass from 18 kHz down), pitched up coming and down going (Doppler, on every sound's
  rate), to the left or right.
- Everything it follows is smoothed sample by sample (the revs over 20 ms, the load over 12 ms, the rest over 40 to
  150 ms), the loops are crossfaded over their loop points, and the output is bent softly before
  full scale: no clicks, no zipper noise, no clipping.

`Mixer` is the same off line: `--shot <dir> --frames <n> --sound <wav>` renders the sound of the
frames, and `cargo test -p drive --release audio` checks the GT's loops (they cover idle to the
limiter, each a whole number of cycles), the crossfade (continuous, equal power) and a run up and
down the revs (no jump between samples bigger than the loops' own, louder on load at high revs).

## What the tyres leave (`effects.rs`)

From `WheelState` (slip, slip angle, load, surface), sixty times a second: where a tyre rubs on
asphalt, kerb or concrete it lays a mark (a ribbon as wide as its tread, darker the harder it
rubs; some 2.5 km of them are kept) and, sliding fast enough, smoke; on gravel and earth it
raises dust, on grass a little; on a wet road, spray, most from the rear wheels. Puffs drift with
the wind and die away. In the rain, drops fall round the eye and streak past it as it moves.
Nothing here reaches the physics.

## What the lens does (`Game::post`, `engine::Post`)

(The wet road's mirror and the car's lamps on the ground are the circuit's: `world-render`'s
README, "Race circuits".)

On the world's exposure and tone mapping: darker corners (a vignette); the sun's flare when it is
in the picture; from the trackside cameras a depth of field focused on the car. No blur of speed
and no lens opening with speed: they read as a zoom. None of it at the `low` quality (it goes with the bloom).

## Not yet

A sound of the car's own six (the recordings are of a four: no free set of a six's steady revs
was found), the exhaust's pops on the overrun, gear-change clunks, a wheel's force feedback. Of the
look: mirrors that are a second picture of the scene (barriers, stands, cars), the car's headlights
on the land beyond the circuit's own ground, the car's shadow under the floodlights, a driver seen
from outside. The rivals have no shadow of their own (`world-render` draws one body's: the
player's), and the drying line is felt, not seen (the renderer's wet is one number for the whole
circuit). A race is chosen on the command line, not in a menu.

## The race (`race.rs`, `rivals.rs`)

`--race` (or `--rivals <n>`) puts GT coupés on the grid round the player, each driven by the
racing driver (`sim_core::car::driver::Driver`) with a character of its own
(`Character::rival(skill, seed)`): some 3 % of pace between the quickest and the slowest, braking a
little earlier or later, its own line a few tens of centimetres off, a mistake now and then (a
corner braked too late, one run wide, a lift), all from the seed: the same race every time. They see
the other cars (the player too): they keep a gap behind a slower car, braking for it as it slows;
they tuck into its slipstream on a straight (the wake takes up to 38 % of the drag behind it:
`sim_core::car::slipstream`) and pull out to pass when the braking for the next corner is near, on
its inside if there is room; side by side they leave each other a car's width, the one squeezed
towards the edge lifts; they drop back behind when the other moves over. Their brakes have
anti-lock. A rival driven by the trained network (`--rival net`) sees only its own car and the
circuit; the racing driver watches the traffic for it and its pedals take over while a car is in
its way.

`sim_core::circuit::race::Race` keeps the race: five red lights one a second, then out after 0.5 to
2.5 s (all cars stand with their brakes on until then); each car's progress followed from the start
line; laps; the order and the gaps (since the car ahead was where this one is now, or the laps
down); the leader's flag, after which every car finishes the next time it crosses the line; the
retired (wrecked). The overlay shows the lights, the position and lap at the top right, the order
with the gaps at the left, and the results when the player has the flag (`Shift+R` races again).
`sim-core`'s `tests/race.rs` races them: 20 races of six cars over three laps (its `many_races`)
are in "Contacts", below.

### Contacts

Forty races of six rivals over three laps from the grid (`sim-core`'s `tests/race.rs`,
`many_races`, seeds 1000–1019 dry and 2000–2019 in rain of 0.5): nobody wrecked and every car
finished in all of them. Dry, two cars touched 2.5 times a race (the hardest at 5.0 m/s, a nudge
in a braking zone or the hairpin's queue), and 33 places changed hands in the 20 races. In the
rain, 11.6 touches a race (the hardest 4.8 m/s: more nudging, as the cars brake earlier and run
closer through the slow corners) and 101 places changed. The quickest rival is some 3 % quicker
than the slowest in a corner, but on equal cars the order mostly holds on a dry track: a pass needs
a run in the slipstream on the straight by the lake.

## The replay (`replay.rs`)

`F2` or `Tab` shows the session again from the start of the player's last lap (a minute back if
there is none): every car as it was, twenty times a second, blended in between (five minutes are
kept, some 40 MB with six cars), with every camera (`C`), on any car (`[` `]`). The world stands still
meanwhile; leaving puts every car back as it was. `Space` stops and goes on, `←` `→` five seconds
(`Shift`: thirty), `↓` `↑` a quarter to four times the speed, `Home` `End`. The tyres' marks and
smoke are made again from the wheels shown.

## The ghost (`ghost.rs`, `ghost.wgsl`)

In free practice, the quickest lap within the track's limits ever driven on a circuit in a car is
kept in `$XDG_CONFIG_HOME/skylab-drive/ghosts/<circuit>-<car>.ghost` (or under `~/.config`), twenty
frames a second, and driven again as a see-through car (the car's own shader at a third of its
opacity) beside the player's laps; under the lap times, the ghost's lap and the time between them
here (green ahead, red behind). It is hidden while it is on the player's own car. A screenshot does
not write it.
