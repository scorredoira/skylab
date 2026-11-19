# world-render

Draws the world of `sim-core` with `engine`: the terrain and its height map, the sky, sun,
moon and stars and the haze as one physical model of the air, the clouds and their shadows, the sea and the rivers, the photographs on
the ground and the buildings, trees, hedges, towns, aerodromes (runways, hangars,
windsocks, and their lights seen as they shine: runway, approach, PAPI, beacon, floodlights), the shadows of the relief, the trees and the buildings, the wrecks and
what their crash looks like (fireball, fire and smoke as a simulated gas, fragments, sparks, splash
and foam, the fire's light, scorched ground), and how much of all that a quality draws.

It knows nothing about the vehicles of the game that uses it. A vehicle reaches it as meshes the
game adds and draws the game asks for, as the meshes whose shadows it casts, and as the one body the
view belongs to. `flight` draws its aircraft and cockpits with it, `drive` its cars, on the same world.

Depends on `engine` and `sim-core`; nothing depends on it but the games.

## The contract

```rust
let mut world = WorldRenderer::new(gpu, terrain.clone(), parked);   // parked: buildings::stand_in() or the game's
let hull = world.add_mesh(gpu, &mesh);                               // a vehicle's mesh, once
let look = world.add_look(gpu, &materials, &images)?;                // or a model's materials and pictures (glTF)...
let hull = world.add_textured_mesh(gpu, &mesh, &surface, look);      // ...and a mesh drawn with them, per pixel
world.add_wreck_shape(pieces, wreck_mesh, ranges);                   // how its wrecks look, once
world.set_terrain(gpu, terrain);                                     // another world (a real place loaded meanwhile), from the next frame
world.set_imagery(gpu, Some(imagery));                               // a real place's ground as the satellites saw it (geodata::imagery)
world.replace_terrain(terrain);                                     // the same place round another middle (geodata::follow), nothing dropped

// Every frame, in Game::update:
world.set_quality(gpu, quality);                                     // when it changes; Game::samples = quality.samples
world.prepare(gpu, &Scene { view, hour, time, weather, height_above_ground, body, objects, glows, lamps, see_through, wrecks });
// In Game::draw:
world.draw_sky(pass);
//   ... the game's own pipelines here: what is round the eye (a cabin) covers the world ...
world.draw(pass);
// Game::translucent, Game::before_translucent, Game::draw_translucent (a second pass, only after a crash):
world.translucent();
world.before_translucent(encoder);
world.draw_translucent(pass);
// A game's own see-through pipelines after `draw` (a vehicle's glass, smoke): world.bind_frame(pass) first.
// In Game::post:
world.post()
```

**What it is given each frame** (`Scene`, in `src/lib.rs`):

| Field | What |
|---|---|
| `view: ViewPoint` | Eye in world axes (north, east, down, `f64`), orientation in render axes, vertical field of view, the screen shift (`engine::Camera::shift_y`) and the near distance (0.25 m outside, less with walls beside the eye) |
| `hour`, `time` | Hour of the day (sun, moon, stars, lights after dusk) and seconds of the simulation (clouds, sea, fire) |
| `weather: &Weather` | `sim_core`'s: its wind 6 m up (the windsocks, the smoke, the waves of the sea and the rivers), its visibility and humidity (the haze and the fog, how the towers set the lights: "The sky and the air") |
| `height_above_ground` | Of the eye or the body it follows: from high up trees and houses are left out |
| `body: Option<Body>` | The body the view belongs to: position, attitude and an optional `ShadowShape`. Its shadow falls on ground, sea, trees and buildings; its axes reach the shaders as `frame.body_*` |
| `casters: &[Caster]` | The vehicles whose shadows are drawn from their meshes, the body followed first: each a sphere that holds it (centre in world axes, radius) and its draws as they are now (`CasterDraw`: a mesh of the world's, `CasterMesh::World`, or of the game's own, `CasterMesh::Game`, a range, a matrix, cuts). Empty: only the body's `ShadowShape` |
| `objects: &[ObjectDraw]` | Ranges of the game's meshes, a matrix to render axes relative to the camera, a tint and up to two `sim_core::Cut`s (planes in the mesh's axes it is cut along, for what a tear left of a vehicle): solid, drawn after the scenery, lit and hazed as the world is |
| `glows: &[Glow]` | Lights as glowing squares facing the camera (position relative to the camera, colour) |
| `lamps: &[Lamp]` | A vehicle's lights as they shine, drawn as the aerodromes' are ("Aerodrome lights"): where, aviation colour, candelas in the beam and all round, the beam's axis and half width and height, the lens. A narrow strong beam (5000 cd or more, 20° or less: a landing or taxi light) also lights the ground ahead |
| `see_through: &[ObjectDraw]` | Blended over everything, after the clouds (a rotor's blur) |
| `wrecks: &[sim_core::Wreck]` | Their pieces where they lie, blackened as they burn (if their `Pieces` shape was given), and everything the crash looks like: flash, fireball, flames, smoke, fragments and their trails, sparks, dust, splash and foam, the fire's light, scorched and dug ground ("Crashes", below) |

The clouds the world draws are in `WorldRenderer`'s fields: `cloud_altitude` and `cloud_thickness`
(m), `cloud_cover` (0 clear to 1 overcast).

**What it gives**: the frame drawn (`draw_sky`, `draw`, and after a crash the second pass: `translucent`, `before_translucent`, `draw_translucent`, which the game hands on from the same calls of `engine::Game`); `post()`, how the engine makes the
picture of the linear light the shaders write (below); `pending()`, the tiles still being made in
the background and what a crash would need that is still being made (0 when the picture is
complete; `--shot` waits for it); `draw_calls()`;
`quality()`; `sun(hour)`, where the light the world is lit by comes from (the sun, or the moon once
the sun is 4.6° down) and how much day it is (a game uses `day` for its own lights).

**A game's own pipelines**, for what the world cannot draw with its objects (a cabin lit by its
windows): build them on `shader_source(&quality)` with your WGSL appended, with
`frame_layout()` as group 0 and `object_layout()` (an `ObjectUniforms` at a dynamic offset) as
group 1, and make them again when `quality()` or `Gpu::sample_count` change. Draw them between
`draw_sky` and `draw`, which leaves group 0 bound. `mesh(id)` gives back a mesh added with `add_mesh`.

### Textured meshes

A mesh a game adds with a look (`add_look`, `add_textured_mesh`; `looks.rs`, `textured.wgsl`) is
drawn per pixel with its materials, as glTF has them (`engine::gltf::Material`): base colour,
metallic and roughness, the normal map, ambient occlusion, emissive, glass (`alphaMode: BLEND`) and
decals cut out by their alpha (`MASK`).
`surface` gives, for each vertex of the mesh, its texture coordinates and its material: the mesh is
drawn as runs of one material each, every draw of `Scene::objects` (and a wreck's piece) cut into
the runs it covers. The pictures are on the GPU with their whole mip chain and sampled with 16x
anisotropic filtering; per material a picture of its occlusion, roughness and metallic (at most
1024 wide), the normal map in two channels, the base colour and emissive in sRGB. The normal is bent
in the frame the texture coordinates make at the pixel (from the derivatives: no tangents needed).
It is lit as `object_shade` lights the objects (sun through the air, the vehicles', clouds' and
relief's shadows, the sky, the lights of the ground, the haze) with a GGX highlight of the sun, and
the sky with its clouds and the ground mirrored as rough as the surface is. Its solid runs are drawn
after the other objects, its glass after the clouds, before `see_through`: what is behind through
its tint, the world mirrored on it with Fresnel's strength. A surface mirrors nothing from behind its
own face, and one turned to the ground (a wing's underside) less than the sky gives one turned up.
Its shadow, its cuts and its mirror in the sea are drawn from the mesh's vertices, the last with
their colours. The vehicle parked on the aprons (`buildings::PARKED`) is drawn the same way, by
instances (`parked_vertex`), when the game gives a textured mesh of it (`set_parked_look`); its
shadow and mirror stay the building's.

### The vehicles' shadows: where the frontier is

The world casts the vehicles' shadows and does not know what a vehicle is. The game gives, every
frame, what casts each one (`Scene::casters`): the vehicle's meshes as they are now (a car's body,
its wheels steered and on their springs; an aircraft's surfaces, gear and propellers), and a sphere
that holds it. The world decides when there is a shadow (by day, the sun more than 1.7° up) and how
strong.

**The map** (`casters.rs`, `caster_tile` in shaders.wgsl). Each frame the vehicles are drawn as the
sun sees them, depth only, into an atlas of two by two tiles of 1024 texels: the body followed
(the first caster) and the three nearest others within 250 m. A tile is an orthographic view along
the sunlight fitted round its vehicle's sphere (6 mm a texel for a car, 12 mm for a light aircraft),
its middle moved by whole texels of the world's grid as the vehicle goes, so that the shadow's
edges do not crawl; it is drawn with both faces and a bias by the slope, so that a face does not
shade itself in stripes. Everything that the sun lights then looks itself up in the tiles
(`body_shadow`): the ground, the sea, the circuit's asphalt, kerbs and run-off, the grass, the trees,
the buildings and their roofs, and the vehicles themselves (`vehicle_shadow`, lifted off the face by
a centimetre: a wing's shadow on its fuselage, a car's mirrors on its doors, its roof on the seats
seen from inside). The penumbra is the sun's disc's from how far behind what hides the sun the point
is (the vehicle's depth round it, averaged), 1.6 times wider by eye, sampled at twelve points of a
disc of comparisons of four texels each: crisp where a tyre stands on the road, soft under a wing
tip. 10 to 28 m beyond the far side of its sphere a tile gives way: the body followed's to its
`ShadowShape` (below, an aircraft in the air), the others' to nothing (their shadows are too soft
there to tell). Draws cast what their cuts leave of them. Group 0 holds the map (bindings 13 and
14), which is why a shader stage reads up to 17 textures (`engine::Gpu` asks the adapter for them).

**The `ShadowShape`**: the body followed's shape, for where the map does not reach and for the
lowest quality (`Quality::vehicle_shadows`), where it is the only shadow. Up to four flat pieces
lying in the body's plane at a height each, and three standing in its plane of symmetry, as
rectangles in body axes. At every pixel `shape_shadow` (shaders.wgsl) follows the ray to the sun
through each piece and hides as much of the sun's disc (0.53°) as the piece covers: sharp near the
ground, a faint penumbra far above. `None` casts none (a vehicle in pieces: its wreck is drawn).
`flight`'s shapes are in `flight/src/kinds.rs` (`shadow`).

What is too round or too slanted for those planes, the gear, is up to nine solid pieces
(`ShadowShape::solids`): ellipsoids along the body's axes (a tyre, a wheel's fairing) and rods (a
strut), whose shadows `solid_cover` works out from how far the ray to the sun passes outside them,
crisp where they stand on the ground. Only the pixels whose ray passes within the pieces' reach of
the body try them. The ellipsoids also darken the ground under and round them, by day, night and
under an overcast alike, with the map or without it (`body_contact`, art-directed: the soft shadow
where a tyre stands on the runway, fading as the gap below a piece grows). `flight` makes them from
each leg's parts in its model (`render/mod.rs`, `legs` and `gear_solids`) and raises them with the
springs every frame; a collapsed leg casts none.

The wrecks, trees and buildings cast their shadows as the world's.

### Wrecks and parked vehicles

Fire, smoke and wrecks are the world's: the simulation says where the pieces lie and since when it
burns. What a piece looks like is the vehicle's: the game cuts its mesh into the pieces
(`sim_core::Pieces`) and gives it once with `add_wreck_shape`. A piece that is only part of one (a wing
or half a tailplane torn across, or what was left of it) carries the `Cut`s of its tears
(`Piece::cuts`), and is drawn cut along them. A cut draw has a torn edge (`torn_edge` in
shaders.wgsl): across the cut's plane a broken line of straight runs between sharp corners, as
sheet metal parts between its rivets, a shorter one the other way (the upper and the lower skin do
not part at the same place) and small teeth along it; the same on both sides of a tear, so that
the piece and the stub it came off fit. Bare metal along it and the primer of the inside of the
skin through it. The aerodromes' aprons have
vehicles parked on them (`buildings::PARKED`, the last kind of `sim_core::scenery::houses`): the
game gives that building to `new`; `buildings::stand_in()` is a grey one made of the solids the
simulation runs into there, and `buildings::check` tells whether a building agrees with them.

### Crashes

The simulation says what happened and how hard (`sim_core::Wreck`): where it came apart and at
what speed, how much fuel went up at once in a fireball and how much burns where the wreck lies (only
if the impact burst the tanks and lit the fuel: a soft crash or a ditching does not explode), where
each fragment flew, the scorched and dug ground, the splashes. The world draws it, as a pure function
of the wreck and the time: nothing is kept from frame to frame but the fire's simulation, which is
played up to the moment shown. A car's crash would look the same from the same data.

- **The fire as a gas** (`fire.rs`, `fire_sim.wgsl`, `fire_draw.wgsl`, `fire_common.wgsl`,
  `heat.wgsl`). A grid of 80 x 112 x 80 cells of 0.85 m (68 m wide, 95 m high) round the newest
  crash within 4 km that still burns or smokes; each cell holds the air's velocity, fuel, heat,
  soot and dust, stepped on the GPU every 1/60 s:
  - **What feeds it** (`fire::emitters`, a pure function of the wreck and the time): the fuel of
    the fireball bursting out as a lit mist, one cloud and fourteen smaller bursts round it one
    after another, upwards and the way the aircraft went, over half a second (the ball is lumps on
    lumps); what fell back burning on the ground for five seconds; the vapour of the pool where
    the main piece lies, and of the patches along its slide; the smoke of what smoulders once the
    fire is out; the earth the impact throws up, for a second, which hangs over the place with
    the fireball's smoke for most of a minute. Nothing is fed over water: there the fuel does not
    burn, and a ditching is spray and mist alone.
  - **A step** (`fire_sim.wgsl`): everything is carried along the flow (followed back along the
    velocity and read there: Stam 1999); fuel burns where it is hot, a rich cloud as fast as air
    gets into it, leaving heat and soot and making the gas take up more room (Feldman, O'Brien and
    Arikan 2003), which is what blows the fireball up to its size (some 15 m in radius for an
    aircraft's 100 kg) and thins what is in it by as much as the flow really spread; it cools by
    its own glow and into the air, rises by its heat (the fireball's cloud, once it has stopped
    glowing, is cooled and held back by the air it takes in during its first seconds: from its
    second second it is a mass of dark smoke with fire inside, which hangs some 15 m up and drifts, while the fire that burns on sends its smoke up), and is kept turbulent: the whirls the grid
    would smear away are pushed back (the vorticity confinement of Fedkiw, Stam and Jensen 2001)
    and hot gas is stirred by a curl of noise (Bridson, Houriham and Nordenstam 2007). The pressure
    that takes the spreading away is relaxed in place, the cells of one colour of a chessboard from
    those of the other, five sweeps a step. The ground, the buildings and the trees' crowns (which
    only hold the air back) come from a map of 218 m made at the crash (`fire::Land`).
  - **The grid follows the fire** from the impact (a dozen metres towards the main piece, so that
    both are in it) to where the main piece burns, a cell a step at most (`fire::Grid`): the step
    reads the one before that many cells away.
  - **Its light** (`lighting`, once a frame, on half the cells each way): how much smoke lies
    between each place and the sun, and above it; and the fire's own light, averaged coarser and
    coarser, which lights the smoke round it by night as by day. Its coarse levels also keep how
    much gas there is, which is how the march knows where there is none.
  - **The drawing** (`fire_draw.wgsl`): marched between the passes (`before_translucent`) into a
    picture of half the scene's size, in steps of a cell that shorten where the gas is thick and
    of 6 m where there is none for 12 m round, stopping where the scene is or nothing more shows
    through; put over the scene in the second pass, each of the scene's pixels from those of the
    march that stopped at about its distance. A noise that drifts up with the gas
    pushes the place the gas is read at (billows and rags finer than the cells; a wider one, the
    bulges of a fireball) and makes its heat uneven: where it is low the soot is a cold black crust
    over the fire, where it is high a tongue of flame. What glows is the soot and the fuel alight, by Planck's law at three wavelengths
    (`heat.wgsl`), no brighter than their heat however deep the flame; cold soot scatters the
    sun's light forwards and back, through the smoke on the sun's side.
  A step takes 0.6 ms on an Apple M5 Pro (one a frame at 60 fps); a fire first seen late is played
  from the crash, or from 45 s back, at 30 steps a frame. Drawing it, particles and all, takes
  1.3 ms at 1600 x 1000 with the fireball over a third of the picture (a frame of 10.8 ms of
  which the scene is 9.5), and 1 ms with the pool's fire alone.
  Above the grid the pool's plume and the fireball's smoke go on as particles (`effects::cap_height`
  is where the simulation puts the fireball's smoke, which a test holds it to).
  `Quality::fire_simulation` (off in `low`) draws it all as particles instead; `fire_detail` says
  how finely the grid is marched (`medium`: steps of a cell and a half).
- **Particles** (`effects.rs`, `particles.rs`, `effects.wgsl`): one storage buffer, one draw (two
  when a grid is between them), sorted far to near, blended premultiplied so what glows adds and what
  is dark covers. Balls of gas (`gas_ball`) marched in six steps through a 3D noise that eats
  lumps and hollows into them (`noise.wgsl`: Perlin's gradient noise and Worley's cells, made on
  the GPU when the renderer starts), lit by the sun through the rest of the ball, the sky and the
  fires (a pale gas sends its light on again and again inside it: mist is white right through,
  soot black), glowing by their heat as the grid does, cut where the scene is so they meet the ground
  softly; small ones (a trail's wisps, hundreds of them one over another) are smooth, and what
  the view's ray meets of one has a closed form: no march, no noise. What does not change over a
  ball (the sky's light, the fires', the haze before it) is worked out at its corners. They are the fireball and
  the flames without the grid, the plume, the fireball's smoke above the grid, the trails of the
  burning fragments, dust, mist. Streaks for sparks and thrown water, the flash, and a tumbling
  sliver for each fragment where it flies and then lies.
- **On the ground and the water** (`FrameUniforms::fire_*`, `marks_*`, `foam*`, read in `shaders.wgsl`
  and `scenery.wgsl`): up to four lights of the fires (the flash, the fireball, the pool, the patches
  along the slide) on the ground, sea, trees, buildings and wrecks, falling off with the square of the
  distance; up to eight marks of scorched ground, a hole and the earth thrown out of it, embers
  while it is hot; up to eight patches of foam that spread and fade over two and a half minutes.
  With no wreck near, the shaders skip all of it and the frame is one pass, as before.
- **Made before any crash** (`effect_pipelines.rs`): the pipelines of the simulation and of the
  second pass and the noise are made on background threads when the renderer starts, the drawing
  again for each quality and number of samples. Made at the first crash, the driver's compiling of
  their shaders froze the picture for 1.1 s the first time a build ran (Apple M5 Pro).

### Race circuits

A terrain's circuits (`sim_core::circuit`, `Terrain::circuits`) are drawn by a `TrackRenderer` the game
makes for it (`TrackRenderer::new(gpu, &world, &terrain)`, or `with_lines` with the line the cars
run on, a `track::Line` per sample: metres across and how much rubber) and asks to
`prepare(gpu, &world, eye)` after `WorldRenderer::prepare` and to `draw(pass)` between `draw_sky`
and `draw`. Its `wet` (0 to 1) is the game's to set when it rains. Each circuit is one mesh of
its own (`track.rs`, `mesh`), made from the samples the simulation drives on, so what is seen is what the
tyres touch: across each sample, from the middle out, the asphalt (its cross slope), the kerb (raised
as the simulation has it), the run-off (grass, gravel or asphalt), the verge or the pit lane behind
the barrier, to the end of the circuit's own ground, where the terrain (laid a little lower under it)
takes over at the same height; then the barriers (armco rail on posts, tyre walls behind a striped
belt, concrete walls with a debris fence), the file's props (the pits' garages, the start's gantry
and its lights, grandstands, marshals' posts, a footbridge), an advertising board every 50 m behind
the barriers and along the stands' foot, and the floodlights' towers where the file places them
(`PropKind::Floodlight`; one every 90 m, one side and the other in turn, for a file that places none). Its shader (`track.wgsl`, on the world's shaders: `track::track_source`) paints the asphalt's
photograph with a darker band of rubber along the cars' line (darkest where they brake and turn), dust
off it, patches of newer asphalt and cracks sealed with tar, its edge lines, the chequered start line
and the grid's boxes, red and white kerbs with their ridges, mown stripes on the grass, raked gravel,
the first metres of an asphalt run-off in green, concrete slabs in the pit lane; and lights it as the
ground is (the sun through the relief's, the clouds' and the vehicles' shadows, the body's contact
darkening under its tyres, the sky, the fires, the haze). One draw.

- **The crowd**: a card standing over each row of a stand's seats, on which the shader draws the
  people of the row, half a metre each (a head over shoulders, in what each wears; some seats
  empty) and nothing between them.
- **The boards**: a colour and a name in letters of five dots by seven, which the shader reads from
  tables made by `track.rs` (`lettering`). The names (`track::BRANDS`) are invented, of no real make.
- **Wet** (`TrackRenderer::wet`): the paved ground darkens and a film of water lies on it, level
  whatever the grain, deeper in the hollows (puddles the rain stirs); it mirrors the sky and its
  clouds as the sea does (`water_sky`), most at a glance, the land's dark band low over the horizon,
  and the body the view follows as a dark shape under it. What stands on it (the car and its lamps,
  barriers, towers and their lamps, trees) it mirrors in the frame's second pass, out of the
  picture of the first (`track_mirrored`: the mirrored ray followed across the picture against
  its depth, some forty steps and four halvings; where it leaves the picture or meets only sky, the
  first pass's sky stays). The game makes that pass when `mirrors()` says so (`Game::translucent`
  and `reads_scene`) and calls `before_wet(gpu)` between the passes and `draw_wet(pass)` in the
  second, before anything see-through. Not at the `low` quality nor with one sample per pixel.
- **By night** the towers' lamps shine and light the circuit: what every vertex gets from all of
  them is worked out with the mesh (`Vertex::light`, `track::floodlight(towers, point)`: a tower
  that stands far back is the stronger for it), so the pools of light are where the towers are.
  `TrackRenderer::towers()` gives them to the game, to light its vehicles by and to hand the nearest
  to the world as lamps (the land and trees beyond the circuit then have some of their light). The
  four nearest the eye reach the shader as they are, for their glint in a wet road when they are
  over the picture's edge.
- **The followed car's lamps** (`TrackRenderer::beams`, `tail`, in candelas, with `hour`): its
  dipped beams on the ground and what stands by it (wide, cut off under the horizontal, evened
  near and far, falling off with the square of the distance) and the red of its tail lamps behind.
- **Grass** on the circuit's verges and run-offs: the world's tufts (`grass.rs`), short as mown
  grass is, longer from 8 m out.

## Modules

| To change | Read |
|---|---|
| What the renderer draws each frame and in what order (the second pass after a crash), the windsocks and floodlight masts, the wrecks' pieces | `src/renderer.rs` |
| The contract: `Scene`, `ViewPoint`, `Body`, `ShadowShape`, `Caster`, `ObjectDraw`, `Glow`, `sun` | `src/lib.rs` |
| The vehicles' shadow map: its tiles, how each is fitted and kept on its texels; how it is read (`caster_tile`, `body_shadow`, `vehicle_shadow` in `src/shaders.wgsl`) | `src/casters.rs`, "The vehicles' shadows" above |
| The air: what it is made of for the weather, the sun's and the moon's light through it, the exposure of the hour; its tables on the GPU (transmittance, multiple scattering, sky view, aerial perspective, ambient light) | `src/sky.rs`, `src/sky.wgsl`, `src/sky_common.wgsl`, "The sky and the air" below |
| The sky, sun, moon and stars as drawn, the haze and the light of the sky on every surface read from those tables, terrain, sea and rivers, clouds, objects, fire, the vehicles' shadows | `src/shaders.wgsl` |
| The volumetric clouds: their layers, shadow map, march, light and upscale; their textures; the lab that draws them alone | `src/clouds.rs`, `src/clouds.wgsl`, `src/clouds_shadow.wgsl`, `src/clouds_draw.wgsl`, `src/cloud_noise.rs`, `src/cloud_noise.wgsl`, `examples/cloud_lab.rs`, "Clouds" under "Scenery" below |
| The sea's waves for the wind: the trains of waves, their slopes, what they leave to the roughness, the whitecaps | `src/sea.rs`, "Water" below |
| The land, trees and buildings mirrored in the sea | `src/reflection.rs`, the `*_mirror_fragment`s, `mirrored_seen` in `src/shaders.wgsl`, "Water" below |
| Buildings and their shadows, the forest beyond the trees, town streets, aerodrome ground | `src/scenery.wgsl`, "Scenery" below |
| Which tree is drawn where: the species, their looks, the climate | `src/species.rs`, "Trees" below |
| The trees' meshes, near and far, and the tufts of grass | `src/vegetation.rs`, "Trees" below |
| The cards' pictures composed of the scanned leaves, twigs and grass | `src/foliage.rs`, `assets/textures/CREDITS.md` |
| The trees' textures and pictures from far (impostors), baked on the GPU | `src/impostors.rs`, `src/impostor_bake.wgsl` |
| How a tree is drawn and lit, changes level, sways, and casts its shadow | `src/flora.wgsl`, `src/flora_surface.wgsl` |
| Tufts of grass round the camera | `src/grass.rs`, the `grass` part of `src/flora.wgsl` |
| The aerodromes' lights: their instances, how bright a candela is in the scene, the settings by the hour; how each is drawn (beam, flash, PAPI, haze, halo) | `src/lights.rs`, `src/lights.wgsl`, "Aerodrome lights" below; what lights there are is `sim_core::lights` |
| Pipelines, bindings and the uniform structs (mirrored field by field in the WGSL) | `src/pipelines.rs` |
| The photographs on ground, runway, roads and buildings | `src/materials.rs`, the `[materials]` parts of `src/shaders.wgsl` and of `building_vertex` / `building_fragment` in `src/scenery.wgsl`, `assets/textures/APPLYING.md` |
| How much each quality draws | `src/quality.rs`, "Quality" below |
| Terrain tiles and their levels of detail | `src/terrain.rs` |
| Race circuits: their mesh (asphalt, kerbs, run-off, barriers, stands) and how they are painted and lit | `src/track.rs`, `src/track.wgsl`, "Race circuits" above |
| A real place's roads round the camera, as a picture the ground reads | `src/roadmap.rs`, "Real places" below |
| A real place's picture cut into fields, woods and scrub; their edges, kinds and walls; the trees they show | `src/parcels.rs`, the `[parcels]` parts of `src/shaders.wgsl`, "Real places" below |
| A real place's ground as the satellites saw it, on the GPU and in the ground's shader | `src/imagery.rs`, `ground_photo` and the `[imagery]` parts of `lit_ground` in `src/shaders.wgsl`, "Real places" below |
| The height map the relief's shadows are walked over | `src/heightmap.rs` |
| Trees and houses by tile, in the background, in batches | `src/scatter.rs`, `src/jobs.rs` |
| The buildings of a real place's maps, a mesh to a tile: walls, roofs, what they look like | `src/footprints.rs`, `footprint_vertex` and `facade` in `src/scenery.wgsl` |
| The bridges of a real place, in the same tiles | `src/bridges.rs` (their shapes are `sim_core::bridges`') |
| Where each tree and building stands (also what vehicles run into) | `sim_core::scenery` |
| The meshes: buildings, pole, windsock, the lights' square | `src/buildings.rs`, `src/models.rs` |
| A game's mesh drawn with its textures: the materials and pictures on the GPU, the runs, how it is lit; its glass | `src/looks.rs`, `src/textured.wgsl`, "Textured meshes" above |
| What a crash looks like: the particles, the fire's lights, the marks and the foam, from a wreck at a moment | `src/effects.rs` |
| What a crash needs on the GPU made before it (pipelines, noise) | `src/effect_pipelines.rs` |
| The particles on the GPU, the second pass's pipeline, the noise | `src/particles.rs`, `src/effects.wgsl`, `src/noise.wgsl` |
| The fire as a gas on a grid: what feeds it, where the grid is, the simulation and its drawing; the light of what is hot | `src/fire.rs`, `src/fire_sim.wgsl`, `src/fire_common.wgsl`, `src/fire_draw.wgsl`, `src/heat.wgsl` |
| The fire's light, scorched ground and foam in the ground's and sea's shaders | `fire_light`, `ground_marks`, `foam_at` in `src/shaders.wgsl` |

## The viewer

The world alone, with a free camera and no vehicle:

```text
cargo run --release -p world-render --example viewer -- [--shot <dir>] [eye=<n>,<e>,<agl>] [look=<heading>,<pitch>]
    [hour=<h>] [seed=<n>] [visibility=<km>] [humidity=<%>] [cover=<%>] [base=<m>] [thickness=<m>] [wind=<from>,<kt>]
    [quality=<high|medium|low>] [textures=off] [size=<w>x<h>] [crash=<s>[,<m>]] [aerodrome=<class>] [final=<m>]
    [place=<ICAO> | at=<lat>,<lon>] [offline=1] [imagery=off] [cruise=<m/s>,<s>]
cargo run --release -p world-render --example viewer -- places
```

In the window: W S forward and back, A D sideways, R F up and down, ← → turn, ↑ ↓ look up and down
(or drag the mouse), Shift ten times faster; T and Shift+T the hour, 1 2 3 the quality, C and
Shift+C the cloud cover, P prints the options that take that picture again, Esc closes it. The
line at the top says the same and the milliseconds of a frame. Without `eye` it starts 2 km short
of the home aerodrome's runway, 150 m up. `--shot <dir>` writes `<dir>/scene.png` (1600×1000)
once everything is loaded and prints the fastest, quartile and median frame of 240 timed after 60.
`places` lists the home aerodrome and the towns, lakes, river, coast and densest wood near it, and the nearest aerodrome
of each class of lighting. `aerodrome=<visual|nonprecision|cat1|cat2>` starts at the one of that
class instead of the home one, and `final=<m>` puts the eye on the 3° path of its main approach, `m`
metres before the aiming point, 2 m over the wheels (`final=9260` is 5 NM). `crash=<s>[,<m>]` flies a
light aircraft with full tanks into the ground (or the sea) `m` metres ahead of the eye (120) at
90 kt and 35° down, and shows it `s` seconds after it came apart: its fire, smoke, fragments, marks
and splash, not its pieces (they are the game's to draw); in the window the time then runs on.
`place=<ICAO>` draws a real place round that aerodrome instead of the generated world (the eye
starts short of its main runway), `at=<lat>,<lon>` round a point (the eye over it): `geodata`
downloads it the first time and keeps it, `offline=1` reads only what it kept. Its ground takes
the colour of the satellites' picture of it (made the first time too); `imagery=off` leaves it
the colours of its maps.

## Quality

`Quality` says how much `WorldRenderer` draws; `Preset` names the three a player chooses from.

| | high | medium | low |
|---|---|---|---|
| the clouds' march (`cloud_march`: takram's presets) | `HIGH`: steps of 50 m, detail, turbulence, shafts, a shadow map of 1024 | `MEDIUM`: no turbulence nor shafts, a shadow map of 512 | `LOW`: steps of 100 m, no detail, one step to the sun |
| trees up to | 2600 m | 1800 m | 1000 m |
| trees as their full mesh up to, as their simpler one up to, then as their pictures (`tree_detail`, `tree_pictures`, times 220 m in a picture 1000 pixels tall; "Trees") | 121 m, 253 m | 99 m, 220 m | 77 m, 176 m |
| a view of a tree's picture from far (`tree_picture_size`; 64 views a species) | 80 texels | 64 | 32 (the cards and barks at half side too) |
| tufts of grass round the camera, when it is lower than half that (`grass_range`) | 70 m | 45 m | none |
| relief shadow steps (fewer are longer, same reach) | 22 | 12 | none |
| samples per pixel | 4 | 4 | 1 |
| terrain up to (the haze closes in to hide its end) | 72 km | 56 km | 40 km |
| photographs of ground and buildings (`texture_size`) | 1024 px | 1024 px | 512 px |
| they fade into their average colour by (`texture_range`) | 4 km | 3 km | 1.5 km |
| patches for the view from the air (the dry grass enlarged 64 times, blurred), up to | 9 km | 6.75 km | 3.4 km |
| normal maps light the surface up to (`texture_relief`) | 1500 m | 250 m | never: colour only |
| second fetch that hides their repetition from the air (`texture_scatter`) | yes | yes | no, and the big scale at half contrast |
| glow round bright light (`bloom`, `engine::Post::bloom`) | 3 % | 3 % | none |
| halos round the aerodromes' lights in mist (`LIGHT_HALOS` in lights.wgsl, with the bloom) | yes | yes | no |
| texture memory on the GPU | 213 MB | 213 MB | 27 MB |
| a crash's fire as a simulated gas (`fire_simulation`; otherwise particles) | yes | yes, marched in longer steps (`fire_detail`) | no |
| trains of waves on the sea (`sea_waves`; the shorter ones go into its roughness) | 32, to 25 cm | 24, to 1.4 m | 12, to 20 m |
| the sky mirrored over the waves (three looks, one), its clouds, the bed's photographs and the light the waves focus on it (`sea_detail`) | yes | yes | no: the bed its plain colour |
| the land, trees, buildings and lights mirrored in the sea (`reflection`: the screen's size over) | 2 | 3 | none: the sky alone |
| the buildings of a real place's maps up to (`building_range`; far tiles hold the big ones) | 6 km | 4.5 km | 2.5 km |
| a real place's picture cut into its parcels, drawn sharp close up with their rows, walls, hedges and trees (`ground_detail`, "Real places" below; 150 MB on the GPU and as much in memory) | yes | yes | no: the picture alone |
| the vehicles' shadows from their meshes (`vehicle_shadows`, "The vehicles' shadows"; a map of 16 MB) | yes | yes | no: the body followed's `ShadowShape` alone |

`high` is the picture the world was designed with: the shader constants of the other two are
written into the source by `pipelines.rs`, which leaves the files untouched for `high`. A struct
of your own (`cloud_march: None` draws the clouds as flat slices, and `cloud_slices: 0` with it
none) goes to `set_quality`. `Quality::ULTRA` (`--quality ultra`), for pictures and video, marches
the clouds at every pixel in steps of 10 m (`CloudMarch::ULTRA`). Only 4 and 1 samples exist:
2 is not offered by every GPU. `texture_size: 0` draws the ground without photographs, with the
noise it had before them, and loads none.

The shaders write linear light, unclamped (the engine's rule); `post()`, given from `Game::post`,
says how the engine makes the picture of it: AgX with a contrast of 0.4 and colour 1.1 (the look
the scene had with the tone mapping each shader used to apply), and an exposure that follows a
quarter of the way to the scene's brightness, so the night is seen darker but readable, and stops
down for a glare (`Post::glare`, 1): the brightest patch of the blurred scene (a lit fog, not a point
of light) is kept within 4 stops above mid grey.

## The sky and the air

One model of the air gives the colour of the sky, the light of the sun and the moon where it
reaches, the light of the sky on every surface and the haze between the eye and what it sees, so
that they agree: a sunset reddens the sun on the ground, the clouds and the sky together, the air
is clearer and the sky a deeper blue at 3000 m, and a hazy or foggy day is the same air with more in
it. Rayleigh scattering by the air, Mie by its haze, absorption by the ozone, on a round planet
(Hillaire 2020, "A Scalable and Production Ready Sky and Atmosphere Rendering Technique"; the
transmittance laid out as Bruneton's, 2017). `sky.rs` on the CPU, `sky.wgsl` on the GPU,
`sky_common.wgsl` what both and `shaders.wgsl` agree on.

- **What the air holds** (`Air::new`, from `Scene::weather`): the air's Rayleigh scattering (8 km
  scale height), the ozone's layer 25 km up, and the haze or the fog of the day. Its extinction is
  what the visibility leaves after the air's own (Koschmieder: a dark object keeps 2 % of its
  contrast there); it is uniform up to the nearest aerodrome, where the visibility is reported, and
  thins by e every 1200 m above it. Haze scatters blue a little more (an Ångström exponent of 1.3)
  and absorbs a tenth of what it takes; humid, it whitens and absorbs less. Fog (from 5 km down,
  all of it below 1) is water: white, absorbing nothing, lying in 300 m. Below the full quality the
  haze thickens to hide the nearer end of the terrain (`Quality::haze_visibility`).
  The default day (`Weather::default`, 150 km) is a clear one: mountains 60 km off stand plain,
  and a low sun has a bright core and a warm halo. The halo is the haze's forward scattering, so it
  grows with the haze: at 45 km it is a white glare a quarter of the picture wide, as a camera sees it.
- **The tables** (`SkyTables`, `Rgba16Float`, made by compute passes): the transmittance to the top
  of the air (256 x 64) and the light scattered more than once (32 x 32, Hillaire's approximation)
  when the air changes; the sky seen from the eye (192 x 108), the aerial perspective (32 x 48
  directions by 32 distances, out to 30 times the visibility and at most 200 km: the light
  scattered towards the eye and what gets through, per colour) and the ambient light (8 x 8 ways a
  surface faces, at the aerodrome's height and at the eye's) when the sun, the moon or the eye
  move more than shows: a hundredth of a degree, 2 m or half a percent of the height. All five
  take 0.36 ms on an Apple M5 Pro, the last three 0.13 ms, a still view nothing. The sky is the same
  on both sides of the sun's vertical plane, so the tables hold one side; the world's moon keeps
  to that plane, across the sky from the sun.
- **Read at every pixel** (the "sky" part of `shaders.wgsl`): `sky_color` the sky along a
  direction; `light_at` the sun's or the moon's light through the air above a point (red low down
  at sunset while a cloud above is still gold: the clouds take both, `sun_light_at` and
  `moon_light_at`); `sky_light` the light of the sky and of the ground on a surface by the way it
  faces; `apply_fog`, `haze_at`, `haze_through` what the air lets through of a point and what it
  scatters towards the eye. A direction is found in the tables by the cosine of its azimuth from
  the sun and its height: no angle is taken. The ground, trees, buildings, water, clouds, objects,
  the effects of a crash and the aerodromes' lights all read them; a game's own pipelines too
  (`flight`'s cabins use `sky_seen`).
- **Sun, moon and stars.** Both discs are seen through the air before them, 40 times as bright as a
  white surface they light (their real radiance, some 46 000 times, would bloom into a glare). The
  moon is always nearly full, 0.25 lux overhead; it lights the world once the sun is 4.6° down
  (`SkyLights::moonlit`), tinted as the eye sees by moonlight, with the rods (bluer: the Purkinje
  shift a screen cannot make by itself). The stars have magnitudes (2.5 microlux at 0), are dimmed
  by the air near the horizon and drowned in the sky by day.
- **Exposure** (`sky::exposure`). The scene's light is the sun's `SUN_SCALE` (2.6) for the 128 000
  lux it has above the air, which draws the day as the scene always was. Below 80 000 lux on the
  ground (the U.S. Naval Observatory's tables, `daylight`), what is `x` stops darker in the world is
  drawn `6.5 (1 - e^(-x / 6.5))` stops darker: the sun on the horizon 4.5 stops below noon, the end
  of civil twilight 5.8, a moonlit night 6.2 (in the world, 20); the engine's exposure makes up a
  quarter of that. The aerodromes' lights go into the scene by the same measure (`lights::gain`).

What it costs, median frame of the viewer's `--shot` at 1600 x 1000 on an Apple M5 Pro, before
(the hand-made sky and haze) and after, the home aerodrome of seed 1 (`places`):

| view | options | before | after |
|---|---|---|---|
| noon | `hour=12` | 6.8 ms | 6.8 ms |
| golden hour, into the sun | `hour=18.0 look=285,2` | 7.6 ms | 7.6 ms |
| sunset, the sun on the horizon | `hour=18.69 look=285,2` | 6.6 ms | 6.6 ms |
| civil twilight | `hour=19.0 look=285,4` | 7.7 ms | 6.7 ms |
| dawn | `hour=5.4 look=75,2` | 7.0 ms | 7.0 ms |
| night | `hour=23` | 6.8 ms | 6.8 ms |
| 3000 m, noon | `eye=2391,12348,3000 look=110,-12 hour=12` | 8.8 ms | 8.9 ms |
| 3000 m, sunset | `eye=2391,12348,3000 look=285,-4 hour=18.6` | 8.3 ms | 8.4 ms |
| hazy day | `hour=12 visibility=8` | 6.8 ms | 6.8 ms |
| fog | `hour=11 visibility=0.8` | 6.8 ms | 6.9 ms |
| sea, 300 m | `eye=3763,24300,300 look=150,-5` | 6.2 ms | 6.3 ms |
| night, 2 NM out | `final=3704 hour=22.5 visibility=30` | 8.1 ms | 8.1 ms |
| noon, `low` | `hour=12 quality=low` | 2.9 ms | 2.9 ms |

It costs what the hand-made sky did: a lookup in a table for the haze, the sky's light and the
sun's, where it worked them out. In the twilight it is cheaper: the moon took over from 2.3° down,
and the relief's shadows were walked for it at every pixel; now the set sun counts until 4.6° down.

The shade of the clouds in the air (shafts of light) is drawn with the clouds ("Clouds" under
"Scenery"). Not yet: the relief's shade in the air, the clouds as part
of the sky's light (overcast, the light of the sky is only greyed by the cover), the moon's phases,
airglow and the glow of towns, a sky below the flat world's horizon.

## Textures

The ground, the runway and apron, the roads and the buildings take their grain from the photographs
in `assets/textures` (sources in its `CREDITS.md`, the method in its `APPLYING.md`). They are read
at the first frame, and again when the quality changes to one with other textures (`high` and
`medium` share theirs). The folder is the first of these that holds `grass_short_color.jpg`
(`materials::find_textures`, which a game may use for photographs of its own):

1. `$FLIGHT_ASSETS/textures`;
2. `assets/textures` next to the executable or in any folder above it (so a game installed with
   its `assets` folder beside the binary works from anywhere, and so does `target/release/skylab`);
3. `assets/textures` in the current folder or any folder above it;
4. the `assets/textures` of the source tree it was compiled from.

If none has them the renderer prints the folders it tried and ends the program. `ENGINE_PROFILE=1`
prints the folder used, the memory taken and the time it took to read them.

Trees and houses are drawn in batches: one instance buffer per kind with every tile in range,
positions relative to a tile corner near the camera, made again only when the tiles in range
change (`scatter.rs`). A frame is 380 to 500 draw calls, about 270 of them terrain tiles.

## Scenery

`sim-core` says where forest, farmland, rivers, towns and aerodromes are. Which tree or building
stands on which spot is decided in `sim_core::scenery` from hashes of the spot and the seed; the
simulation runs into the same trees and buildings.

- **Trees.** Fifteen species, drawn for the simulation's eight shapes of crown, with cards of
  scanned leaves, three levels of detail and their own shade; see "Trees" below.
- **Forest beyond the trees.** Past the tree range the ground itself is lifted 9 m where there is
  forest and painted as a canopy (`forest_ground`); under the trees it is dark.
- **Shadows.** Each tree lays its outline as the sun sees it on the ground ("Trees"); each building
  flattens a hull of convex solids (`buildings::Building::hull`) on to the ground along the sunlight.
  Both are blended over the frame after everything solid. They assume the ground under them is the
  plane through their foot (trees: with the slope stored in the instance; buildings: level).
  The vehicles' shadows are looked up at every pixel instead (above).
- **Buildings.** Nine kinds (`buildings::KINDS` and the parked vehicle): houses, a shed, a church,
  and at the aerodromes hangars, an office with a tower and what is parked. The alpha of a vertex
  colour says wall, roof or plain; walls get windows from the shader, lit after dark. A town's
  streets and plots are painted on the ground by `town_ground` on the grid
  `sim_core::scenery::PLOT` puts the houses on; it also says which photograph each part takes
  (gardens grass, yards bare earth, paving and pavements concrete, streets asphalt).
- **Fields.** `scenery::field_boundary` repeats the layout `terrain_fragment` paints (with
  `ihash`, bit for bit), so hedges and lines of trees stand on the boundaries. Change one and
  change the other. Only in a
  generated world: a real place's fields are the parcels of its picture ("Real places"), and the
  simulation puts no hedges on these squares there.
  About one square of farmland in five has a farm (`scenery::farm`): a house,
  one or two sheds and a tree or two in a yard in one corner, clear of the hedges, on level ground
  away from towns, aerodromes, rivers and roads. Trees keep out of the yards. The ground under a
  yard is still the crop of its field: the shader does not know where farms are.
- **Aerodrome lights**: see "Aerodrome lights" below. The floodlight masts are the windsock's pole, taller.
- **Clouds.** Volumetric, in up to four layers (`clouds.rs`, `clouds.wgsl`, `clouds_shadow.wgsl`,
  `clouds_draw.wgsl`, `cloud_noise.rs`, `cloud_noise.wgsl`): a port of the clouds of takram's
  three-geospatial (MIT; `THIRD_PARTY_NOTICES.md` at the root says which file comes from which).
  `Layers::of` makes the layers of a sky of ours: cumulus from `cloud_altitude`, `cloud_thickness`
  tall, taller ones among them, cirrus above and, with `cloud_storms`, a layer as tall as the storm
  towers that is only where they stand. `clouds::coverage` turns `cloud_cover` into their coverage.
  The weather, the shape, its detail and the turbulence are textures drawn once at the start by
  their generators; the samples of the marches are placed by a blue noise of our own. Every frame:
  the shadow map is marched from the sun (one map of 98 km over the flat world, at the level of
  the top of the layers that shade) and averaged with the frames before; the shade on the ground
  is drawn from it into the second channel of the height map, where the world's shaders and the
  sea's mirror read it; the view is marched at one pixel of every block of four by four, a
  different one each frame, lit by our sky's tables (`cloud_sun_light`, `cloud_sky_light`); and
  that is brought to the scene's size with the frames before and put over the scene. The same
  march finds how much of each ray lies in the clouds' shade (from the shadow map) and takes
  that share of the air's light away, from the clouds' own haze and from the scene's behind them:
  the shafts of light. Their haze under the clouds is drawn too (it comes with the coverage).
  What is ours and not theirs is said at the top of each file; by eye and not by physics are
  `clouds::own_shade` and `sun_gain` (a low sun), `cloud_overcast_light` and the grey of a storm
  cloud, and these three, all art direction: **the rain** (`cloud_rain` in `clouds.wgsl`,
  `clouds::rain`): grey curtains under every storm tower, and thinner ones under the heaviest
  cumulus round a storm and here and there under a sky that is all cloud, from the base towards
  the ground (to it under a storm, part of the way elsewhere), leaning as the wind carries them,
  dark in the middle, lit by a glow of the sun from behind; marched in 48 long steps under the
  base only where it rains, its streaks stripes round the eye and not places of the world, so
  that they do not depend on where a step falls; nothing falls on the ground nor on the
  windscreen. **The overcast from below** (`cloud_weather_at`, the fourth channel of the weather
  texture): the base of a closed sheet rolls up to 450 m, the sheet thins in patches, and its
  base is darker and bluer where it is thick and lighter where it thins; from above it is the
  sheet it was. **The hour of the gold** (`cloud_low_sun`, from 20 degrees of sun down): the
  sun's light on the clouds is more orange and at the last pink (`cloud_golden`), the sky's on
  their shaded side bluer and less (`cloud_golden_sky`), the thin rim of a cloud before the sun
  shines (`cloud_silver`), their haze is thicker and the shafts take more of the air's light
  away; the shadow map is never marched from lower than 4 degrees (`clouds::shade`), so the
  clouds keep their shade and their shafts until the sun sets (they lost both under 1.7
  degrees); and the picture has more colour and contrast (`WorldRenderer::post`,
  `clouds::golden_hour`). What the three cost, the whole frame's median in `high` at 1600x1000
  against the build before them, turn about, three times each: by a storm 7.9 ms (was 7.6),
  a storm far off from the runway 12.3 (12.2), under an overcast 9.9 (9.8), a fair noon 9.2
  (9.1), after sunset 5.6 (5.8); with the sun one degree up, when there were no shafts and now
  there are, 11.9 (10.3), which is what a frame with shafts costs at any hour. In motion (ours, `cloud_volume_resolve` in `clouds_draw.wgsl`): what the frames before
  have not seen (the side the view turns towards, what was behind a cloud the eye comes out of)
  is made of the frame's march alone, its texels joined smoothly and blurred, and is for sixteen
  frames the mean of what they find round it; cloud right before an eye that moves, which every
  texel of the march finds different, is drawn as the blur of a fog; and the place a pixel had
  in the frame before goes smoothly from a pixel to the next. `scripts/video/clouds-motion.ron`
  records both as the game draws them (out of a cumulus at four times the speed, a turn of the
  camera at 100 degrees a second). Known limits: a pixel is marched once in sixteen frames, so a
  cloud's edge against terrain more than 250 m away (a ridge) is soft, and so is, for a second
  or so, what has just come into view; the frame's group and the
  march together use all 16 textures a stage may read. What they cost (the same before and after
  what is done for motion): the whole
  frame's median in `high` (`--shot`, 17 Nov 2026, the machine not idle), against the clouds
  before them (a library of cumulus marched at half the size), ms at 1600x1000 and at 2880x1800:
  outside 12.0 (was 14.6) and 20.0 (26.2), in the air 9.5 (12.1) and 15.4 (21.7), at sunset 10.7
  (12.1) and 16.7 (21.9), on the runway 7.6 (7.6) and 12.8 (14.0), towards the low sun 10.7 (10.7)
  and 15.3 (19.1). The outside view in `low`, `medium` and `ultra`: 5.1, 9.5 and 21.9 ms at
  1600x1000. A `Quality` with `cloud_march: None` draws the
  old flat slices instead (each samples the cloud at a point of its ray through the slab around
  it, chosen per pixel, so the slices do not show as stripes).
- **Sea.** The height map holds heights below sea level too: the breakers read the slope of the
  bed from it ("Water" below).

## Trees

What a tree is to the simulation does not change by being drawn: where it stands, how tall and
wide it is and which of eight crowns it has (`sim_core::scenery::trees`, `CROWNS`) is what the
aircraft runs into (a race course's trail through the forest included: `sim_core::course::cuts`
clears it and thickens the wood round it where the trees grow). Only the parcels' trees and the
tufts of grass are drawn and never run into.

- **Species** (`species.rs`): oak, beech, ash, holm oak, olive, poplar, cypress, spruce, fir, larch,
  stone pine, pine (Aleppo or Scots), palm, the shrubs of the maquis and those of greener land. Each
  of the simulation's trees is drawn as a species of its crown, chosen by its place alone (the same
  tree in a tile of any size): the climate of the place (a real place's latitude says whether its
  summers are dry as the Mediterranean's, 31° to 44°, `sim_core::scenery::summer_drought`, where the
  simulation's own lowland conifers are pines too, or whether it is tropical; a generated world's
  moisture, as before), the height (beeches from 650 m, larches near the tree line) and the coast
  (palms along a warm coast, many in the tropics, never high up). A holm oak drawn for the
  simulation's oak is narrowed to the oak's crown (`species::fit`). Menorca is holm oaks, wild
  olives, stone and Aleppo pines and maquis; Innsbruck is oaks, ashes and beeches in the valley,
  spruces, firs and larches up its slopes. A real place's parcels' trees (`parcels::trees`) are
  chosen the same way, and on the dry open fields of a Mediterranean place where the picture sees
  scattered crowns stand olive groves, in rows along the field's own rows, 7.5 by 8.5 m, pruned low.
- **Cards of scanned leaves** (`foliage.rs`): leaves, spruce twigs, pine sprigs and grass tufts cut
  out of the textures of Poly Haven's plant models (CC0, `assets/textures/CREDITS.md`) are found in
  their pictures and composed, on a thread when the renderer starts (a quarter of a second), into
  nine sprays of 512 texels: broad leaves, small leathery ones (holm oaks, the maquis), an olive's
  two-toned ones, a spruce's branchlet, a pine's tufts, a palm's frond, a cypress's scales, a
  larch's rosettes, grass. Their colour is scaled to a mean grey and takes the species' (measured
  leaf albedos, the green of early summer); each texel also holds the leaf's slope across the card
  and how much light it lets through. Their smaller levels keep the share of the card the full one
  covers (Castaño's alpha mipmaps), so a crown is as dense far as near, and the shader sharpens the
  outline to a pixel at any distance (alpha to coverage). Barks are four CC0 photographs: furrowed,
  a pine's plates, a palm's rings, smooth.
- **Meshes** (`vegetation.rs`): each species built once by a rule of its own from a seeded
  generator: a trunk and limbs as tubes with their bark, and clusters of two cards facing out of the
  crown round the ends of the branches and over the crown's ellipsoid; a spruce's and a fir's tiers
  of branches, each a V of two sprays and a third hanging from it; a stone pine's dome on its forked
  trunk; a palm's arching fronds and the dry ones under them; a cypress's flame. The full mesh is
  300 to 800 triangles, the simpler one (the trunk, the main limbs, half as many cards twice as
  big) 60 to 270. Each vertex keeps its place in its crown and how open to the sky it is.
- **Light** (`flora_light` in flora.wgsl): the leaves' normals lean out of the crown's middle, so
  the crown is lit as one volume, with each leaf's slope on top; the sun's way through the crown's
  ellipsoid from each point gives the crown its own shade (`SPECIES_CROWN`: the ellipsoid and its
  density); the leaves let the sun through towards the eye when it is behind them, by species
  (`SPECIES_LIGHT`); holm oaks and olives shine a little; the sky lights each point as open as it
  is. The relief's shade is read as for the ground, with a third of the steps
  (`coarse_terrain_shadow`), and the clouds'.
- **Wind**: each tree leans with the gusts of the weather's wind (`Scene::weather`, as the sea),
  most at its top, and its branches and leaves flutter, each cluster with a phase of its own.
- **Levels** (`flora.wgsl`, `renderer.rs`): the full mesh up to `Quality::tree_detail`, the simpler
  one up to `tree_pictures`, then the tree's picture up to `tree_range`, where the forest's canopy
  painted on the ground takes over. Each tree changes at distances of its own, and over 12 % of the
  distance both levels draw it, each on its share of every pixel's samples (a dither that does not
  move): it dissolves into the next, never pops. One draw per species and level, of the tiles that
  can hold a tree of that level.
- **Pictures from far** (`impostors.rs`): each species' full mesh drawn unlit from 64 directions
  over the upper half of the sky round it (hemi-octahedral), as a layer of a texture array: colour
  and outline, normal and depth. A tree is then one square facing the eye, painted by the four
  views nearest the way it is seen from, each where the eye's ray crosses it, and lit by its normal
  and its place in the crown (from the depth) as the meshes are: the same colour, the same light
  through the leaves and the same shade of the crown, so it changes level unnoticed. Baked once
  the cards are made, and again for another size of view.
- **Shadows**: each tree's shadow on the ground is its outline in the view nearest the sun's way,
  followed up the sunlight from each point of the ground and blurred a little: a pine's has its
  gaps, a poplar's is a column.
- **Grass** (`grass.rs`): with the eye lower than half of `Quality::grass_range`, tufts of the
  grass picture every 0.7 m round the camera, where the ground is grass (meadows, dry grass, fields,
  rough ground: by the parcels in a real place, by `Terrain::surface` in a generated one), green or
  straw as the ground is, shrinking into it at the edge of their ring; none on water, roads, towns
  or an aerodrome's mown ground; a third of them on a race course's trail (worn), none within 9 m
  of its start (its pads stand on bare ground).

On the GPU: the cards and barks 34 MB (low 8), the pictures from far 65 MB (medium 42, low 10).

What it costs, median frame of the viewer's `--shot` at 1600 x 1000 on an Apple M5 Pro (the least
of two runs, the builds taken in turn), and the memory it holds once all is drawn, before (the
eight smooth-shaded species and their blobs of shadow) and after. The generated world's densest
wood (`places`: `eye=4900,12500,50 look=10,-6`, `eye=4400,12500,300 look=10,-15`,
`eye=3300,12500,1000 look=10,-20`), Menorca east of Alaior and the Inn valley as in "Real places",
looking down 6°, 15° and 20°:

| view | 50 m | 300 m | 1000 m | 300 m, `low` |
|---|---|---|---|---|
| generated wood | 9.22 → 11.72 ms | 11.85 → 12.41 ms | 11.77 → 12.38 ms | 4.15 → 4.39 ms |
| Menorca | 6.61 → 7.55 ms | 8.64 → 10.04 ms | 9.03 → 10.17 ms | 3.25 → 3.59 ms |
| Innsbruck | 7.47 → 8.57 ms | 8.63 → 9.70 ms | 10.03 → 10.83 ms | 3.88 → 3.95 ms |
| memory, generated | 0.58 → 0.66 GB | 0.58 → 0.66 GB | 0.58 → 0.65 GB | 0.53 → 0.59 GB |
| memory, Menorca | 0.95 → 1.02 GB | 0.91 → 1.02 GB | 0.88 → 1.00 GB | 0.71 → 0.69 GB |
| memory, Innsbruck | 1.32 → 1.30 GB | 1.30 → 1.34 GB | 1.34 → 1.36 GB | 0.68 → 0.74 GB |

On the ground by the wood with the grass (`eye=4600,12600,2 look=10,0`): 8.48 → 10.61 ms.

`cargo run -p flight --example models -- <species> <view> <out.png>` draws a species' mesh (its
cards as solid squares); `far-<species>` the simpler one.

## Water

The sea is at height 0 and flat, as the simulation floats and ditches on it; only the light shows
the waves. The inland water is the rivers and the lakes, drawn by the ground as rivers are: a
generated world's lakes (`sim_core::Lake`) are water flat at their level in the river's valley,
and a real place's lakes its maps'. The water section of
`shaders.wgsl` (its header says the rest) draws it in three parts, so that the ground's shader, the
biggest of all, does not carry the waves (it ran slower everywhere, land included, when it did):

- **Under the surface**, drawn by whatever is under the sea (`under_sea_color`, from
  `terrain_fragment` and `object_fragment`): the light that comes up through the water. The water's
  own colour (deep water reflects a third of its backscattering over its absorption, Morel and
  Prieur) and the bed seen through it, dimmed along the sun's way down and the eye's way up by
  the real depth of that fragment, art-directed (`sea_column`): the bed is seen through clearer water
  than the water's own colour says, along a path no longer than a few times the depth however low
  the eye, and pale sand glows turquoise under it, so that clear water over sand is the bright
  turquoise of Menorca's calas and the Caribbean's banks a metre or two deep, aquamarine to some
  15 m, and the navy of deep water beyond; from 3000 m the coast has its turquoise fringe. The bed is
  gone by 45 m (`OPAQUE_DEPTH`; deeper the ground is not shaded at all). Rivers and lakes keep their
  own water (`river_color`). Where little of the
  bed shows it is its plain colour, with no photographs. In shallow water the light the waves
  focus moves over the bed, and the breakers come in: the swell slows as `sqrt(g h)` over the
  real depth, so its crests bunch up and follow the shore, each breaks where the water is
  shallower than its height over 0.78 and runs in as a bore, a ragged white front and the lace of foam it
  leaves, which opens and fades in some five seconds, and at the edge each bore's sheet of foam
  lies as far out as Hunt's run-up says and drains away (`surf`; `tests/sea.rs` draws it, and the
  glitter and Fresnel below, from the shader's own source). How much foam lies there goes in the scene's alpha; the sky writes 0.
- **The surface** (`sea_fragment`), a plane at sea level blended over that (`pipelines::sea_blend`):
  32 trains of waves (`sea.rs`) for the wind of `Scene::wind`: three of a swell of 156 m, and the
  wind's from two and a half times its peak's length (at most 220 m) to 25 cm ripples, so that a light
  wind has five or six to each octave and its crests do not line up, their slopes scaled to Cox and Munk's for the wind. Each train finer than a pixel can
  show leaves the normal and goes into the slope covariance (whole rests of them at once, from
  `wave_rest`), so the sea is as bright and as rough near and far and does not shimmer, and a
  pixel far off spreads the sun into the long path of glitter on the sea. With that covariance:
  the sun's and by night the moon's glitter (`glitter`: Bruneton et al. 2010, with Smith's
  shadowing as Walter et al. 2007 fit it), Fresnel averaged over the facets, the sky over three
  looks along the view, each counted by how much of its facet the eye sees, and the clouds of
  the layer where the reflected ray meets it, the whitecaps where the crests stand high and fold over
  (the trains gather the water towards their crests, Gerstner's, and where together they squeeze it
  most it breaks), so that as much of the sea is white as Monahan measured for the wind; the light
  that goes into the back of a crest and comes out green and blue through it, against a low sun;
  in light air, slicks where the gusts leave the water glassy. It hides `F` of what
  is under it, and lets the foam below show through its own light. Beyond the end of the ground it
  is deep water by itself.
- **The world mirrored** (`reflection.rs`): the land, trees, buildings, the game's objects and the
  aerodromes' lights drawn before the scene through the view mirrored in the sea's plane, at half
  the screen's size (`Quality::reflection`), into a picture the surface reads (group 1 of the sea).
  The sea is flat, so the mirror is exact: the waves only turn the ray, which the surface follows
  by reading the picture off its pixel by the turn seen on the screen, spread along the view as
  the sky is. What is under the sea is left out; the land as it is is drawn first into the depth
  alone, so that only what is seen through the water is shaded. It is drawn only when a tile of
  ground with sea in it may be seen. On a rippled sea seen from low down the coast mirrors
  little, broken by the waves, as it does; the slicks of light air and the view from higher up
  show it whole. Each surface is hazed as seen from the eye, a little less than along its way
  through the mirror.
- **Rivers and lakes** are drawn by the ground (`river_color`): a metre of ripples that fades into
  roughness, the sky and the banks mirrored, brown water over a muddy bed.

A crash's splash and foam are the world's as before: the foam lies on the sea under its surface
(`under_sea_color`) and on the rivers.

What it costs, median frame of the viewer's `--shot` at 1600 x 1000 on an Apple M5 Pro, before
and after (`eye=3763,…`, the coast near the home aerodrome of seed 1, `places`):

| view | options | before | after |
|---|---|---|---|
| beach, 60 m, calm | `eye=3763,24300,60 look=120,-6` | 4.8 ms | 5.0–5.2 ms |
| coast and shallows, 300 m, 12 kt | `eye=3763,24300,300 look=150,-5 wind=200,12` | 6.5 ms | 6.2 ms |
| open sea towards the sun, 300 m, 07:00 | `eye=3763,25000,300 look=88,-6 hour=6.8 wind=200,10` | 6.0 ms | 4.3–4.8 ms |
| over shallow water, 15 m | `eye=3500,24700,15 look=60,-3 wind=90,8` | 4.2 ms | 4.6 ms |
| sea and coast, 3000 m | `eye=3763,23000,3000 look=110,-20` | 7.1 ms | 6.6 ms |
| sea by night, 300 m | `eye=3763,24300,300 look=90,-8 hour=23` | 5.8 ms | 4.5 ms |
| a river, 120 m | `eye=8968,16000,120 look=60,-12` | 6.5 ms | 6.6 ms |
| coast and shallows, `low` | as the second, `quality=low` | 2.8 ms | 2.8 ms |

The sea got cheaper over open water: the ground under it is no longer shaded where the bed is not
seen. It costs most low over shallow water, where the bed is drawn with its photographs and seen.

## Aerodrome lights

What lights an aerodrome has, where, their colours, beams and intensities, is
`sim_core::lights` (plain data, after ICAO Annex 14 and the FAA; its README and the module's
header list them). `lights.rs` keeps a buffer of instances per aerodrome near the camera, made the
first time it is near (the 8 nearest within 30 km), and
`lights.wgsl` draws each light as a square facing the camera, added to the scene after everything
solid; a frame is one draw per aerodrome.

- **Photometry.** The intensity towards the eye is the main beam's (a Gaussian over the angles off
  its axis: half at its half width and half height; beacons turn it) plus what the light shines all
  round, times how much of its system is lit (`sim_core::lights::setting`: the stage a tower sets
  for the hour and the visibility: runway and approach lights at 4 % by day, 0.8 % at night, more
  as the visibility falls; slope indicators full by day; taxiways, floodlights, beacon and obstacle
  lights from dusk). Its illuminance at the eye, `I / d²`, is dimmed by the same haze as the
  ground (`haze_at`, so red carries further than blue in haze) and shines over the smaller of the
  lens and two minutes of arc (`ACUITY`, what the eye resolves), drawn as a blob as wide as the lens
  or a pixel. A pixel is coarser than the eye: the light keeps the brightness the eye sees it with,
  so a PAPI is a dot by day and a light looks the same at any size of the picture. Seen near it is
  its lens; far, never less than a pixel and never more, so it neither shimmers nor pops; beyond
  18 km the lights fade out, and are gone at 27, before the aerodrome leaves the list.
- **How bright a candela is in the scene** (`lights::gain`): the scene is not in candelas per
  square metre, and its night is drawn far brighter than a real one so that it can be seen
  (`sky::exposure`, "The sky and the air"). The lights go into it by the same measure as the sun,
  the moon and the sky: the sun's `SUN_SCALE` stands for its 128 000 lux, times the exposure of the
  hour. By day a light stands out of the grass as little as it does, at night as much more as it
  does. The bloom and the tone mapping make of it what a camera would.
- **PAPI and VASI**: each unit is white above its angle and red (through a filter that lets 15 %
  through) below, changing over 3' of arc for a PAPI and 15' for a VASI, by the elevation of the eye
  as seen from the unit: from the cockpit and from outside alike.
- **Flashes**: sequenced flashers and REIL for 40 ms every half second, the flashers running in
  towards the threshold; the beacon's two beams turn 12 times a minute.
- **Mist and fog**: of the light the air takes out of the way, half goes on towards the eye in a
  glow round the light (`LIGHT_HALOS`, off in `low`): the forward lobe of a fog's droplets, 0.7°
  through a thin mist, widened and dimmed as the light is scattered again and again through a
  thicker one (1 + 2.5 τ + τ² times, τ the optical depth; Narasimhan and Nayar's atmospheric point
  spread function), so that in fog a light is a wide soft glow and not a blinding blob; in haze
  (visibility over 10 km) the aerosol spreads it over tens of degrees and it is not drawn. Only
  its middle is drawn, to 7°. Its depth is near the eye, so the ground in front does not cut it.
  The towers set the steps of the FAA's tables (`sim_core::lights::setting`), and the world's
  exposure stops down for a bright glow as the eye does (`Post::glare`, below).
- **Floodlights** light the ground as the fires do (`fire_light` in shaders.wgsl), in their beams
  (`in_beam`: a cone down on the stands, half as bright 25° off its axis, and what they spill all
  round): two on the apron of each of the three nearest aerodromes go into the same four lights of
  the ground, about 20 lux on the stands, a pool of light on the apron and what is parked.
- **Vehicles' lamps** (`Scene::lamps`) are drawn by the same shader, as an instance buffer made
  each frame, and are mirrored in the sea; a landing or taxi light's beam lights the runway ahead
  as a light of the ground. After dusk the windsock
  shines with its lamps.

What it costs, median frame of the viewer's `--shot` at 1600 x 1000 on an Apple M5 Pro, before and
after, at the home aerodrome of seed 1 (category II, ALSF-2):

| view | options | before | after |
|---|---|---|---|
| night, 5 NM out | `final=9260 hour=22.5 visibility=30` | 8.9 ms | 9.1 ms |
| night, 2 NM out | `final=3704 hour=22.5 visibility=30` | 8.0 ms | 8.1 ms |
| night, short final | `final=700 hour=22.5 visibility=30` | 6.7 ms | 6.8 ms |
| night, over the threshold | `final=150 hour=22.5 visibility=30` | 6.3 ms | 6.5 ms |
| dusk, 2 NM out | `final=3704 hour=18.9 visibility=30` | 8.0 ms | 8.1 ms |
| day, 1 NM out | `final=1852 hour=11 visibility=30` | 7.3 ms | 7.3 ms |
| night in fog, short final | `final=400 hour=22.5 visibility=0.8` | 6.5 ms | 6.7 ms |
| the viewer's start, by day | (none) | 6.8 ms | 6.8 ms |
| night, short final, `low` | as the third, `quality=low` | 2.95 ms | 2.97 ms |

## Real places

A terrain made from a `sim_core::Region` (`geodata`'s, seam 3) is drawn with the same code as a
generated one: its tiles, height map, trees, houses, sea and aerodromes come from the same
queries. What differs:

- **Roads.** A region has tens of thousands of stretches of road, far more than the frame's 32.
  They are drawn instead from a road map (`roadmap.rs`): 4096 x 4096 texels of 3 m (12 km) round
  the camera, how much of each is road, drawn in the background when the camera has moved 3 km
  from its middle, with its mip levels (`R8Unorm`, group 0, binding 12, read with the height map's
  sampler). The ground's shader reads the level of the ground a pixel spans, so a road fades
  into the ground with distance as it would, a little stronger than its share of the pixel; the
  roads fade out before the map's edge. The 12 km round the camera are what shows; beyond, the
  roads are not drawn. A generated world's roads stay in the uniforms, as before (its road map is
  one texel never drawn on). It costs a texture fetch per pixel of ground. Close up, where a texel of the
  map spans more than a pixel, a road's edge (where the map is a half, its texels falling from 1
  to 0 across one) is drawn a pixel wide rather than a texel: sharp from 30 m up.
- **Aerodromes** (`FrameUniforms::airports_c`): the apron and its taxiway on the side of the
  runway its data says, or none on the other runways of a real aerodrome; a grass runway is its
  mown strip, unpainted.
- **Buildings of the maps** (`footprints.rs`, `footprint_vertex` and `facade` in scenery.wgsl):
  where a region has them (`sim_core::Footprint`), each tile of 512 m within
  `Quality::building_range` is one mesh made in the background. A building is its outline raised
  from 1.5 m under its lowest corner (or from its bottom, a part on others: a tower on its podium,
  the leaning Puerta de Europa) to its roof, its courtyards open (the outline and its holes
  ear-clipped as one ring). The roof is `sim_core::roofs`' shape, the same the simulation runs into:
  flat; gabled, round and gambrel, the roof cut into flat faces at its bends and the walls rising to
  meet it at the gables; skillion; hipped, pyramidal and mansard, a face for each side where its
  plane is the lowest (convex outlines); domes and onions in rings on a flat roof. A flat roof of a
  block has its parapet (the walls going on 0.9 m above it) and, on the bigger ones, the housing of
  its stairs and boxes of air conditioning. Each wall has its `Look`: a facade style chosen by what
  the maps say it is made of, else by its use, height and the region it stands in (`Palette`: warm
  plaster and terracotta round the Mediterranean, brick, render and slate in the north, brick
  blocks, siding and shingles in North America): plaster with shutters and balconies, brick in
  courses close up, stone with tall windows, a curtain of glass that mirrors the sky, ribbon
  windows, a shed's metal sheet with a row of high windows, a house's few windows, a church's tall
  ones; city blocks and towers with shop fronts or a lobby on the ground floor. A wall has a whole
  number of columns of windows and a row to each storey (the maps' floors, else the style's
  storey); `facade` draws each window from the column and height with its edges softened over
  the pixel, and far off, where a window is smaller than a pixel, the mean of the wall and its
  windows. After dark some are lit, warm in homes and cool in offices. The colours the maps give
  are kept (within what paint and stone reflect); the old blocks of a warm town have flat tops the
  colour of tiles, as they look from the air. Detail falls with distance: within 800 m every
  building with its parapet and roof machines, to 2 km every building plain with its outline
  simplified to 0.6 m, beyond only what can be told apart (12 m tall or 500 m²) simplified to
  1.8 m. Mirrored in the sea. They cast no shadow yet. The town houses the generated scenery
  would put there are not put. What they cost, the viewer at `quality=high`, 1600 x 1000, the median
  of a frame against the buildings of OpenMapTiles before them (taken in turn): Madrid from 120 m
  south of the Cuatro Torres (`at=40.4766,-3.6880 eye=-900,-150,120 look=10,4`) 10.8 ms before,
  11.0 now; Madrid's centre from 180 m looking down (`eye=-7000,-1500,180 look=20,-22`) 12.2 and
  13.0 ms; Manhattan from 250 m (`at=40.7549,-73.9840 eye=-2500,-600,250 look=20,2`) 11.1 and
  11.6 ms. A tile of Madrid's centre holds half again the triangles it did (1.6 million in view
  against 1.0), most of them walls; the facades' shader costs some 0.5 ms of it.
- **Bridges** (`bridges.rs`): the parts `sim_core::bridges::parts` makes of each of the region's
  bridges whose middle is in a tile, drawn in the same mesh: boxes with their sides, top and
  underside (seen when flying under), the deck's top asphalt with a parapet (concrete) or a rail
  (steel) along its edges, bars as square tubes. The colour is the maps' (or the landmark's), else
  concrete for a beam bridge and grey steel for the others. A main cable is drawn twice as thick as
  it is and a hanger 25 cm across, so that they stand out from far as they do (a hanger of the
  Golden Gate is 7 cm across).
- **The ground from the satellites** (`imagery.rs`, `set_imagery`): `geodata`'s cloud-free
  Sentinel-2 picture of the place, a near grid (10 m, 30.7 km) and a far one (32 m, 98 km), as two
  layers of one `Rgba8UnormSrgb` texture with its mip levels (group 2 of the terrain's pipelines,
  not of the frame's: those and the fire's drawing already take the 16 textures a fragment shader
  may read on the web). Its smaller copies are made on a thread and it is sent to the GPU 6 MB a
  frame (some 15 frames), the picture before it drawn meanwhile; 100 MB on the GPU. The ground's
  shader (`ground_photo`) reads the near grid over the far one at the level of the ground a pixel
  spans (the mean of its two sides: no anisotropic filtering), each fading out at its edge, and
  where it knows the ground: its colour is the albedo, brightness above 0.16 eased (roofs and
  concrete in the full sun, under buildings drawn again); the painted fields, the canopy and,
  from 300 m to 1.5 km out, a town's painted streets give way to it, and so do the large made-up
  patches of light and dark and seven tenths of the patches seen from the air; close up the
  photographs of the materials give it its grain, chosen as before by what the maps say the
  ground is (town, field, forest, rock, sand, snow), and by the picture where it says more: how
  green it is (dry or meadow grass), and a field's crop (green: a crop; pale: stubble; brown:
  ploughed) with its rows. Roads, runways, rivers and the sea are drawn over it as before.
- **The picture cut into parcels** (`parcels.rs`, `Quality::ground_detail`): from 300 m down a
  texel of 10 m is tens of pixels wide and the picture alone is soft patches. So, on a thread
  when the picture is given (some 1.2 s a grid), each grid is cut into the parcels it shows:
  superpixels by colour and place (SLIC) on the picture with its grain smoothed away, the
  neighbours alike in colour merged up to 3 ha, the smaller than 0.15 ha and the slivers (a line
  of trees along a wall) joined to the neighbour most alike. Their edges are then followed from one
  meeting of three parcels to the next, smoothed and cut to the fewest straight lines within 0.8 of
  a texel (Douglas-Peucker), and every texel keeps its distance to the nearest straight edge of its
  own parcel: between four texels the parcel whose signed distance interpolates highest is the one
  there, and its edge is where it equals the next, straight and sharp at any size. Each parcel is
  told what it is by its mean colour, how mottled it is, how much of it looks like tree crowns and
  the maps' cover: a green crop, stubble, ploughed soil, dry grass, a meadow, scrub, a wood, bare
  rock (or none: water, towns, sand, snow, what the picture does not know). The maps keep some
  ground out of the parcels before the picture is cut (`kept_out`): every runway and its strip
  (70 m beyond its ends, 45 m beyond its sides), the aerodrome's mown ground round it (what the maps
  call grass within 400 m of its ends and 700 m of its sides: inside the fence, its aprons, taxiways
  and buildings), the buildings of the maps, the towns, and what they cover with water, sea, houses, a railway,
  sand or snow: there the picture alone is drawn, and the parcels round it end on a straight edge
  with nothing on it. The distance to the edge is kept as the square root of its share of 8 texels
  (finest near the edge: some 2 cm a step within a texel of it), so a wall's sides stay straight. The shader
  (`ground_parcel`, `[parcels]`) draws a parcel in its own colour up to its edge, with two fifths
  of the picture's own light and dark inside it, and what its kind has: rows along the parcel (along
  its edge on the headland) and tramlines, the swaths of a harvest, furrows, mown stripes, tufts,
  bushes and crowns with their shadows, stones; each faded out before it is finer than some three
  pixels, so nothing shimmers. On an edge between open parcels the pair decides, the same on both
  sides, a dry-stone wall where the land is dry, a hedge where it is green, a dirt track beside
  some, or nothing; none inside woods, scrub or rock. A wall is the Mediterranean's of pale
  limestone, some 0.7 m wide and 1 m high: its top a row of lighter and darker stones, broken here
  and there by a gap or a stretch fallen to rubble (by its place along the edge, the same from both
  sides), a few dry weeds at its foot a little darker than the field, its shadow a thin strip on
  the side away from the sun; a hedge has its gaps too. Thinner than a pixel, from some 0.15 m a
  pixel, a wall or a hedge is drawn less, to two fifths at 0.8 m: a pale thread, not a dark line. No
  wall, hedge or track, nor their shadows, on a runway, an apron or the strip round them, a road or
  a river (the shader's own masks, `[parcels]` in `lit_ground`). The parcels are drawn as a texel of their grid grows on the screen, none under two pixels
  and all from six: from 1000 m up the place is the picture as before, and they come in without a
  line. The near grid's are drawn where it is, the far one's beyond, fading out and in where one
  gives way to the other. Where its trees are drawn a wood's ground is its floor, not the crowns the
  picture saw. They go to the GPU after the picture, 6 MB a frame, as a second texture of the
  picture's group (binding 1: `Rgba8Unorm`, two layers a grid, the map and the colours): 150 MB,
  and the same kept in memory for the trees.
- **The trees the parcels show** (`parcels::trees`): every 6.4 m a spot, decided by its place
  alone (the near and far tiles hold the same trees): a tree where the picture sees a wood and the
  maps none, in scrub, on the texels that look like tree crowns (the clumps in a pasture, the lines
  along walls), and bushes along the walls and hedges; holm oaks, pines and bushes where the land
  is dry, oaks, ashes and poplars, or spruces and firs high up, where it is green, toned by the
  picture, each drawn as a species of the place ("Trees"); and in a Mediterranean place, olive
  groves in rows on the dry fields where the picture sees scattered crowns. Not on water, towns or
  aerodromes, nor where the maps' woods have theirs, and off roads, railways and buildings as the
  simulation's trees are (`sim_core::scenery::Clear`). They are drawn, not run
  into: the simulation never reads the picture. The tiles of trees are made again (nearest first,
  the old drawn meanwhile) when the parcels come or go.
- **Towns**: up to 16 (`MOST_TOWNS`) have their streets painted; a region's towns may overlap
  (circles over a city's built-up ground), and a point belongs to the one whose middle is nearest,
  as `sim_core::scenery` lays the houses.

What it costs, median frame of the viewer's `--shot` at 1600 x 1000 on an Apple M5 Pro: the
generated world is drawn as before to the pixel (the start, a town, a night final and a road, and the
game's `--shot`, compared before and after with `compare`: no pixel differs).

| view | options | median |
|---|---|---|
| Menorca, the start | `place=LEMH` | 6.5 ms |
| Menorca's south coast, 1200 m | `place=LEMH eye=-7000,-3000,1200 look=55,-10` | 4.8 ms |
| the Inn valley, 700 m | `place=LOWI eye=-1500,-9000,700 look=75,-4` | 7.6 ms |
| Innsbruck, the start | `place=LOWI` | 6.8 ms |
| Barcelona's coast, 900 m | `place=LEBL eye=-6000,-4000,900 look=40,-6` | 6.2 ms |
| Barcelona, 600 m over the city | `place=LEBL eye=1000,3000,600 look=40,-8` | 7.9 ms |

With the satellites' picture (`imagery=off` without it; median frame, the same views):

| view | without | with |
|---|---|---|
| Menorca, the start | 7.38 ms | 7.68 ms |
| Menorca, 300 m | 6.92 ms | 7.02 ms |
| Menorca's south coast, 1200 m | 4.83 ms | 4.86 ms |
| Menorca, 3000 m | 5.71 ms | 5.68 ms |
| the Inn valley, 700 m | 7.95 ms | 8.24 ms |
| the Inn valley, 3000 m | 6.89 ms | 7.19 ms |
| Barcelona's coast, 900 m | 6.25 ms | 6.24 ms |
| Barcelona, 600 m over the city | 8.20 ms | 8.11 ms |

A generated world, which has no picture, draws to the pixel as before (`compare`).

With the picture cut into parcels (`ground_detail`; median frame and the memory the viewer holds once
all is drawn, before and after; each view looking down 6° at 50 m, 15° at 300 m, 20° at 1000 m and
25° at 3000 m; Menorca east of Alaior `eye=7400,-5000`, the Inn valley `eye=-400,-5500`, the
Llobregat's fields `eye=1200,-4200`). The parcels' trees and the shader cost some 0.5 to 1.5 ms;
the memory is the parcels (150 MB on the GPU, 150 MB for the trees) and what was freed after
making them, which the allocator keeps. `low` makes none: only the roads' sharper edge is new there.

| view | 50 m | 300 m | 1000 m | 3000 m |
|---|---|---|---|---|
| Menorca | 6.17 → 6.69 ms | 7.43 → 8.13 ms | 7.78 → 8.92 ms | 8.13 → 8.81 ms |
| Innsbruck | 6.28 → 7.21 ms | 6.96 → 8.31 ms | 7.92 → 9.58 ms | 11.13 → 12.13 ms |
| Barcelona | 7.95 → 8.45 ms | 9.24 → 10.04 ms | 9.79 → 10.70 ms | 11.57 → 12.44 ms |
| memory, Menorca | 0.95 → 0.92 GB (peaks) | 0.72 → 0.96 GB | 0.72 → 0.90 GB | 0.74 → 0.92 GB |
| memory, Innsbruck | 0.71 → 1.29 GB | 0.76 → 1.26 GB | 0.73 → 1.27 GB (peak) | 0.79 → 1.28 GB |
| memory, Barcelona | 0.74 → 1.09 GB | 0.75 → 1.07 GB | 0.78 → 1.12 GB | 0.81 → 1.08 GB |

At 300 m with `low`: Menorca 3.12 → 3.23 ms, Innsbruck 3.71 → 3.90 ms, Barcelona 4.04 → 4.17 ms,
the memory as before (0.61 to 0.73 GB). The generated world draws to the pixel as before (the start,
farmland at 80 m and 300 m, and 300 m in `low`, compared with `compare`).

`WorldRenderer::set_terrain` changes the world drawn while the game runs: the tiles, height map,
road map, trees, houses and aerodromes are made again (2.3 ms of the frame it is called in, with
Barcelona); the pipelines, photographs, sky and meshes stay.

`WorldRenderer::replace_terrain` puts another terrain of the same place in its stead, in the same
axes: the region round a middle further along the way, as the aircraft flies on
(`geodata::follow`). Nothing is dropped: each tile of ground, trees and buildings remembers the
terrain it was made of (a generation), and those of the old one are made again in the background,
the nearest first, each drawn as it was until its new one is there; the height map and the road
map are made again and swapped as when the camera moves; the picture of the ground stays until
the new region's is given. Near the aircraft the new region has the same heights and cover (its
grids' nodes are where the old one had them), so what changes there is only where the old region
was coarser. The viewer's `cruise=<m/s>,<s>` flies the eye straight ahead and swaps the regions
in as the game does; from Barcelona westwards at 150 m/s for 240 s (36 km, two regions):

| | median | 99th percentile | worst frame of the second after a swap |
|---|---|---|---|
| frames | 8.8 ms | 10.5 ms | 13.3 ms, 13.5 ms |

and the picture at its end is that of a viewer started there (`tests/places.rs` checks the frame
of the swap draws the old ground whole and the new one comes after).

## Checking a change

- `cargo test -p world-render`: the air against Koschmieder and the tables of daylight, the sun's
  colour, the exposure (`sky::tests`), the buildings against the solids the simulation runs into, the
  runs of tiles, the qualities, the fire, the meshes; the buildings of the maps' walls facing out and
  their roofs of every shape standing where the simulation's are (`footprints::tests`), an outline
  with a courtyard covered once, a hipped roof's faces, the looks by use and region; the shaders of every quality parsed and
  validated without a GPU (`pipelines::tests`, `particles::tests`, `fire::tests`); on a real device,
  the noise filling its range and repeating, and a fireball played from its crash (its fuel burnt
  in a second and a half, the glowing ball's size, its soot kept and rising where the particles
  above the grid expect it, the grid gone to where the wreck burns, nothing for a soft crash); `tests/pipelines.rs`
  builds every pipeline on a real device and draws sky, sea and clouds to a PNG (`$WORLD_TEST_PNG`);
  `tests/effects.rs` draws a crash half a second after it, with and without the fire's simulation,
  with four samples and one, and finds the fireball in the picture; `tests/sea.rs` draws the
  shader's own `glitter`, `hidden`, `mirror_share` and `surf` as rows of values: the sun mirrored
  where it should be and spread by rougher water, the surf as fronts that come in over the
  shallows, none in a calm, a smooth band from far; `tests/places.rs` moves from a
  generated world to a small region and finds its road (from the road map) in the picture, and its
  ground turned red by a red picture of it, and a region put in the place of another
  (`replace_terrain`) drawn with no hole the frame it comes, and two fields of a picture drawn
  sharp where they meet with their parcels and soft without; `parcels::tests`: two fields of a
  picture meet on a straight edge between the texels, parcels told by their colour, numbered apart
  from every neighbour, the trees of a big tile those of the small ones in it, and none on the
  aerodrome, a lake or a village, the parcels beside them ending with no wall; the trees:
  each species drawn for a crown like the simulation's and fitted to it, by the climate
  (`species::tests`), its meshes unit models as wide as that crown, the simpler cheaper
  (`vegetation::tests`), the cards covering their share of the card at every level, the scans
  too (`foliage::tests`), the views of the pictures from far round the hemisphere and agreeing
  with the shaders (`impostors::tests`), the tufts on land and off the runway (`grass::tests`).
- Anything visible: `viewer -- --shot <dir> [...]` and look at `<dir>/scene.png`; in the game,
  `cargo run -q -p flight -- --shot <dir>`. To prove a change leaves the picture alone, take it
  before and after and `cargo run --release -p engine --example compare -- before.png after.png`:
  trees may move a handful of edge pixels, nothing else may differ.
- Speed: the viewer's and the game's `--shot` print the fastest, quartile and median frame. Compare
  medians of several runs, taken in turn with the other build, with no other game window open: one
  open window doubles the figure. `ENGINE_PROFILE=1` adds where the time went.
  `cargo run -p world-render --example msl` prints the Metal source the GPU gets of `shaders.wgsl`.
