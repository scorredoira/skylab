# Applying the textures to the simulator

The plan the world's renderer (`crates/engine/world-render`, then in `crates/games/flight/src/render`) was wired
from. It is done for the ground, the runway and apron, the roads and the buildings (`src/materials.rs`
and the `[materials]` parts of `src/shaders.wgsl` there); the aircraft is not textured. **Section 9
lists where what was built differs from this plan**: read it with the rest. The engine loads the arrays (`crates/engine/engine/README.md`,
"Textures") and `crates/engine/engine/examples/textured.rs` is a working model of the techniques below
(world-space and triplanar mapping, tangent frames, two scales, tint by the mean colour).

## 1. Arrays and bindings

Two arrays of 20 layers, 1024 x 1024, one bind group for the whole frame:

| Binding | Array | `Kind` | Built from | GPU memory |
|---|---|---|---|---|
| colour | `texture_2d_array<f32>` | `Color` | `<name>_color.jpg` | 106.7 MB |
| detail | `texture_2d_array<f32>` | `NormalPacked` | `TextureSource::Channels { rg: _normal.png, b: _rough.jpg, a: _ao.jpg }` | 106.7 MB |
| sampler | `sampler` | | `Gpu::texture_sampler()` | |

Put them in group 0 (`frame_layout`) after the height map: bindings 3, 4 and 5. 213 MB in all
(5.33 MB per layer and array). If that is too much, the two cheap cuts are loading everything at
512 (53 MB; shrink in `fetch.sh`, `size=512`) or dropping the detail array beyond the objects.

Layer index, fixed (terrain first, objects after):

| # | Material | Tile near (m) | # | Material | Tile (m) |
|---|---|---|---|---|---|
| 0 | `grass_short` | 2.8 | 10 | `snow` | 4 |
| 1 | `grass_meadow` | 4.2 | 11 | `sand` | 3.6 |
| 2 | `grass_dry` | 5 | 12 | `asphalt` | 4 |
| 3 | `soil_ploughed` | 4 | 13 | `concrete` | 2.5 |
| 4 | `crop_stubble` | 4 | 14 | `dirt_road` | 4 |
| 5 | `crop_green` | 4 | 15 | `metal_painted` | 2 |
| 6 | `forest_floor` | 4 | 16 | `metal_corrugated` | 2.7 |
| 7 | `rock_granite` | 3 | 17 | `plaster` | 2 |
| 8 | `rock_layered` | 4 | 18 | `roof_clay` | 2.5 |
| 9 | `scree` | 6 | 19 | `roof_slate` | 1.5 |

The same table is `MATERIALS` in `examples/textured.rs`. Ground tiles are about twice the real size
of the photograph (4 mm per texel instead of 2): from a cockpit 1.2 m above the runway that is still
sharper than the screen, and it halves the repetition.

## 2. Which material where (terrain fragment)

The terrain shader already has everything needed: vertex colour, `cover` (farm, water, forest),
the normal, the height (`in.rel.y + frame.cam_wrap.y`), `w` (wrapped world position) and `dist`.
Compute a weight per candidate, keep **the two largest**, renormalise, and sample only those.

| Weight | Rule (same thresholds the code uses today) | Material |
|---|---|---|
| rock | `steep = smoothstep(0.22, 0.5, 1 - n.y)` | `rock_granite`; `rock_layered` where `1 - n.y > 0.45` or height > 1900 m. Triplanar (section 5) |
| scree | `smoothstep(0.12, 0.22, 1 - n.y) * (1 - steep)`, and height 1900-2300 m on gentle ground | `scree` |
| snow | vertex colour is already snow: `smoothstep(2250, 2650, height)` and not steep | `snow` |
| sand | height < 5 m and not steep (the beach of `terrain.rs`) | `sand` |
| forest | `cover.z` | `forest_floor` (under the trees; from above the trees hide it) |
| farm | `farm = smoothstep(0.3, 0.6, cover.x)`, then by the field's `pick` | `pick < 0.22`: `crop_stubble`; `< 0.65`: `crop_green`; `< 0.82`: `soil_ploughed`; else `grass_meadow` |
| airfield | inside `airport_paint`'s outer box but off the runway and apron | `grass_short` |
| default | what is left; by the moisture the vertex colour encodes (`color.g / color.r`: 1.0 dry, 1.8 lush) | `grass_dry` to `grass_meadow` |
| runway | `rw` of `airport_paint` | `asphalt` |
| apron | `apron` of `airport_paint` | `concrete` |
| roads | `road` | `asphalt` (they are painted dark grey today); `dirt_road` if a road class is ever added |

Runway, apron and roads are not candidates in the blend: they are laid **over** the result with
their existing coverage masks, exactly as `mix(albedo, paint.rgb, paint.a)` does now.

Layers are integers, so two materials are blended by sampling both and mixing, never by
interpolating the index. Hedges stay procedural (a dark multiply), as do crop rows.

## 3. Scale: near, far, and nothing

One scale cannot serve 1.2 m and 3000 m. Per material:

```
near = fetch(w / tile)                       // 4 mm per texel
far  = fetch(ROT * w / (tile * 9) + 0.37)    // 36 mm per texel, rotated 37 degrees
mean = textureSampleLevel(colour, samp, vec2(0.5), layer, 16.0).rgb   // 1 x 1 mip = average colour
k    = smoothstep(tile * 10, tile * 60, dist)                         // 40 m .. 240 m for a 4 m tile
albedo = mix(near * far / mean, far, k)
normal = mix(near_normal, far_normal, k)     // each in its own tangent frame, see the example
```

- Below ~40 m only the near scale gives detail; the far one modulates it, so no two repeats of the
  near tile look the same.
- Beyond ~240 m the near scale is gone and need not be sampled.
- Fade the far scale out too: `far = mix(far, mean, smoothstep(1500, 4000, dist))`. At 3000 m a
  36 m tile is two pixels; the mip chain already returns nearly the mean and the anisotropic filter
  does the rest. Beyond 4000 m sample nothing: use `mean` (a constant per layer, better passed as a
  uniform table than sampled).
- `w` is the wrapped position (period 16384 m). 16384 / tile must be an integer or a seam appears
  every 16 km: use tiles that divide it (4, 2, 8, 3.2 = 16384/5120, ...). Rounded: 2.8 -> 3.2,
  4.2 -> 4, 5 -> 5.12, 3.6 -> 3.2, 6 -> 6.4, 3 -> 3.2; the far scale `tile * 8` instead of 9, and
  the rotation dropped or replaced by a swap of axes (`w.yx`), which also wraps.
- Repetition of the far scale in the mid range (300-1500 m, a tile is 20-100 pixels): the example
  uses `fetch_scattered` (two fetches at offsets chosen by a slow noise). It hides it for grass,
  soil, sand, snow and asphalt. It does **not** hide the cracks of `rock_layered` laid flat; that
  material is for slopes. If the scattered fetch costs too much, a plain `fetch` plus the tint of
  section 4 is nearly as good on the vegetation layers.

Sample with explicit gradients: take `dpdx(w)` and `dpdy(w)` once at the top of the fragment
shader and use `textureSampleGrad` everywhere. Then the samples can sit inside real `if` branches
(near scale only when `k < 1`, second material only when its weight is above 0.02) without breaking
the mip selection, and the per-fragment cost follows the table in section 7 instead of its worst case.

## 4. Keeping the look from altitude

Today the colour of the land is `in.color` (moisture, forest, rock, snow, sand, town from
`sim-core`), the crop colour of each field, and the `detail` / `broad` noise multiplies. Keep
all of it and let the photographs supply only the variation around their mean:

```
albedo = tint * texture / mean        // tint = the colour computed today, before the noise
```

- `tint` is the existing `albedo` just before the line `albedo *= (0.74 + 0.22 * detail) * ...`.
- Keep the `broad` multiply and the warm shift (scales of 256 m and 1024 m: no texture has them).
- Drop `vnoise(w * 0.5)` and `vnoise(w / 16)` from `detail` and the `fine_bump` normal: the
  near and far scales replace them. Keep the 128 m term.
- Clamp `texture / mean` to 0.4..2.2 so a dark crack cannot go black nor a pebble blow out.
- Snow: its texture is slightly blue and darker than the vertex colour; with the division by the
  mean this does not matter.

This way the landscape from 3000 m is pixel for pixel what it is now (the texture term tends to 1),
and the photographs fade in as the aircraft descends.

## 5. Steep rock: triplanar

World-space top-down mapping stretches on slopes. Where `steep > 0`:

- Project along x and z (the two side projections of `object` in the example); the top
  projection is the flat material already sampled. Weights `pow(abs(n), 6)`, renormalised.
- Vertical coordinate is the height above sea level, not `rel.y`, so the strata of `rock_layered`
  stay put as the camera moves. Use the far scale only (tile 16-32 m): cliffs are seen from afar.
- Tangent frames as in `side()` of the example; add the leans to the geometric normal.
- Keep the existing strata darkening (`vnoise(height * 0.09, ...)`) as a multiply: it gives the
  horizontal banding at the 100 m scale that a 4 m texture cannot.
- Skip a projection whose weight is below 0.1 (with explicit gradients): most cliff fragments then
  cost one projection.

## 6. Runway markings, roads and objects

**Markings.** `airport_paint` stays as it is and keeps producing `mark`, `rubber`, `rw`, `apron`.
Change only what it mixes: asphalt = `tint_asphalt * asphalt_texture / mean` instead of the `grain`
noise; paint = `white * (0.75 + 0.5 * texture_luma / mean_luma)`, so the paint shows the grain of
the surface under it, and `mark *= 0.85 + 0.15 * ao`. Use the asphalt normal under the paint at 40 %
strength. Tyre rubber: multiply the albedo as now and lower the roughness by 0.2. The runway is
mapped along its own axes (`across`, `along`), not world axes, so the grain does not cut the edge
lines at an angle. Roads: the same along the segment (`h * length(ba)`, `d`).

**Objects** have 10-float vertices and no texture coordinates. Proposal, cheapest first:

- *Buildings (hangar, houses):* triplanar in **model space**, in the `object` / `house` fragment
  shader, with the layer taken from the vertex. The meshes are boxes and gables: one projection
  dominates almost everywhere, so with the 0.1 cut it is one fetch. Walls `plaster`, house roofs
  `roof_clay` or `roof_slate` (by the instance tint), hangar walls and roof `metal_corrugated`.
  Corrugation must run down the slope: for roofs project along the ridge axis.
- *Aircraft:* generated coordinates per part, written by `models.rs` when it builds each loft:
  u = distance along the loft axis, v = arc length around the ring, in metres; `metal_painted` at
  a 2 m tile, tinted by the paint colour. Triplanar would swim on the control surfaces as they move
  unless done in each part's own space, and on curved skin it blends three projections everywhere.
- *Where the layer goes:* the alpha of the vertex colour is gloss today (0..1). Encode
  `alpha = layer + gloss * 0.5` (gloss in 0..0.99) or, cleaner, add two floats to the vertex
  (layer and one spare) when the UVs are added: `MeshBuilder` then needs a 14-float layout
  (position, normal, colour, uv + layer). That is an engine change for when the aircraft is done.
- The texture multiplies the vertex colour through `texture / mean`, as on the ground, so today's
  colours of walls, roofs and paint are kept.
- Glass, lights, propeller disc, trees: no texture.

## 7. Cost per fragment

Texture samples (each is one anisotropic trilinear lookup), for one material:

| Fragment | Colour | Detail | Total |
|---|---|---|---|
| Ground beyond 4000 m | 0 | 0 | 0 |
| Ground 240-4000 m, far scale | 1 | 1 | 2 (4 with the scattered fetch) |
| Ground under 240 m, near + far | 2 | 2 | 4 (6 scattered) |
| Second material in a transition | the same again | | x 2 |
| Steep rock, one or two projections | 1-2 | 1-2 | 2-4 |
| Building, one projection | 1 | 1 | 2 |
| Aircraft | 1 | 1 | 2 |

The mean colour is a uniform table (20 x vec4), not a sample.

Typical frame at 1000 m: most of the screen is ground beyond 240 m with one material, so about
2-4 samples per pixel, against today's shader which already does over a dozen noise evaluations and
a 22-step height-map march per pixel. The textures are not the expensive part of the terrain shader.

Measured: `examples/textured.rs` fills a 1600 x 900 frame (4x MSAA) with the worst case of this
table everywhere, ground with near + scattered far = 6 samples plus the mean, in **0.7 ms of GPU
time on an Apple M5 Pro**, grazing or from above (`ENGINE_PROFILE=1 ... textured out.png grazing 300`).
That GPU is several times faster than a small integrated one; scaling by 6-8 for an older
integrated GPU (Intel Iris Xe, Apple M1) gives an estimate of 4-6 ms per frame for the worst
case and 2-3 ms for the tiered version above. This is an estimate, not a measurement: measure on the
target machine with the same command before fixing the tiers.

Memory bandwidth, not sample count, is what an integrated GPU runs out of. Two things keep it
down: the mip chain (far ground reads the small levels, which fit in cache) and the anisotropy
clamp. If a machine struggles, lower `anisotropy_clamp` from 16 to 4 first; the grazing view loses
a little sharpness in the distance and nothing else changes.

## 8. Checks when it is wired in

- `cargo run -q -p flight -- --shot <dir>` from the cockpit on the runway, at 300 m and at 3000 m.
  The 3000 m picture should be indistinguishable from today's.
- Fly along x and z across a multiple of 16384 m: no seam (section 3).
- Sun low in the west: furrows of `soil_ploughed`, roof tiles and corrugation lit on the west side.
  The `domes` view of the example is the reference for the sign conventions.
- Bank steeply near the ground: no shimmer on the runway; if there is, a texture is being sampled
  without mips (wrong gradients inside a branch).

## 9. What was built, and where it differs from the plan

Built as planned: the two arrays and the sampler at bindings 3, 4 and 5 of the frame bind group;
the layer order; `texture / mean` multiplying the colour the game already had; two scales with the
big one eight times the small; explicit gradients and real branches; the average colour and
roughness of every layer as a uniform table (`Frame.materials`, taken from the 1 x 1 mip level);
runway, apron and roads laid over the ground along their own axes; triplanar rock with the height
above the sea as its vertical coordinate; the old noise kept beyond the range of the photographs.

Differences:

- **Tiles** (section 1 and 3): the rounded ones, 3.2 / 4 / 5.12 / 4 / 4 / 4 / 4 / 3.2 / 4 / 6.4 /
  4 / 3.2 / 4 / 2.56 m for layers 0 to 13, the big scale 8 times that with the axes swapped. The
  ground's coordinates are metres modulo 1024 (a varying of their own, `TerrainOut.tex`), not
  modulo 16384: at 16384 a float resolves 2 mm, half a texel. 1024 is a multiple of every repeat.
- **Half of the hue only** (section 4): `texture / mean` per channel on a ground of another
  colour than the photograph turned the straw of `crop_green` pink on the green of a field. The
  ratio is mixed half and half with its own average. Under the small scale the big one counts for
  half, or its blotches are too marked from the cockpit.
- **Which material** (section 2): snow, sand, rock-grey and moisture are read from the vertex
  colour itself (with the height for sand), so the textures follow the colour exactly where the
  terrain generator's noise moved a boundary. Inside a town the material is what `town_ground`
  painted there (it writes it to a private variable, `town_at`): gardens `grass_short`, bare yards
  `dirt_road`, paved plots and pavements `concrete`, streets `asphalt`; one material, switched half
  way across the soft edge between street, pavement and plot (blending the three cost 0.15 ms a
  frame). Grey ground that is flat and low outside the painted towns takes `dirt_road`; grey on a
  slope or high up is `scree`; steep is rock. The second material comes in from a twentieth of the
  weight and the two swap at a half.
- **Scattered fetch** (section 3): its bands come from two value noises of 64 m and 32 m cells
  added, not one: with one, the bands follow the grid of the noise and show as a weave. Bands much
  wider than a repeat do not hide the repetition at all. Where a band is not blended only one fetch
  is made. Without the scattered fetch (`low`) the big scale is left at half contrast beyond the
  small one, because its repeats show from the air as a grid.
- **Triplanar** (section 5): the sides use both scales, as the top does, not the big one alone:
  a cliff is flown past at 50 m. A side counts from a weight of 0.1.
- **Markings** (section 6): `airport_paint` still mixes the paint; it now also writes what it found
  (runway, apron, paint, rubber, the airfield box, position across and along) to a private
  variable, and the fragment shader multiplies its colour by the asphalt or concrete. Paint shows
  40 % of the grain (`0.6 + 0.4 * luma`), not `0.75 + 0.5`, which brightened it. The grass inside
  the airfield box is `grass_short`, mown in strips 4 m wide along the runway.
- **Roads**: all asphalt, and so are the streets of the towns.
- **Buildings** (section 6): no triplanar blend. Every face of a hangar or a house is flat, so it
  is mapped in its own plane from model space: u along the horizontal of the face, v down it,
  computed per vertex; the tangent goes to the fragment shader as a varying. One fetch, no
  stretching on walls, gables or roofs, and corrugation and tiles run down the slope. The layer is
  chosen in the vertex shader: houses by the roof mark (gloss 1) and the tint (dark tint = slate,
  which is also greyed), hangars are `metal_corrugated` with a `concrete` footing 0.9 m high.
  This lives in `building_vertex` / `building_fragment` of `scenery.wgsl` (the buildings of towns and
  aerodromes). Walls (`buildings::WALL`) are `plaster` under the windows; roofs (`ROOF`) `roof_clay`,
  or `roof_slate` when the roof tint is dark; the hangar's sheet is marked `buildings::SHEET` (a kind
  of `PLAIN` surface, alpha 0.6) and takes `metal_corrugated`; every other `PLAIN` surface keeps its
  colour. No vertex layout changed.
- **A third scale, from the air** (`mat_air`): the colour photograph of `grass_dry` once more at 64
  times its tile (328 m a repeat), fading in from 20 to 200 m of distance and out between 1.5 and
  2.25 times `TEX_RANGE`. Seen from 50 to 500 m the near scale is gone and the big one is
  mip-averaged into an even colour: grassland looked as flat as before. Enlarged 64 times, the
  shading of the tufts becomes the patches a meadow or a field shows from there. It is read
  blurred, never sharper than the mip level of 64 texels a repeat (`textureSampleLevel`, no
  anisotropy): sharp, single blades of grass showed as strokes tens of metres long across fields
  and the mown strip of the aerodrome. Blurred, the patches of one photograph look like those of
  another, so every ground material takes the same ones (`air_strength`: all but rock and paving,
  mixed between the two materials of the fragment): one fetch instead of one per material, and no
  seam where two materials swap. Its contrast is raised 2.2 times from 300 m, 1.4 times under 60 m
  (a texel of 5 m is several pixels wide there and the patches looked smeared) and 1.3 beyond 3 km,
  where the patches are seen edge on; a fifth less without the scattered fetch (`low`), where the
  big scale is at half contrast and they read as brown smears. Its mapping is bent by the 256 m
  and 1024 m noises of `broad` (0.3 of a repeat), so the repeats do not line up into a grid.
- **The 16 m noise is kept** (only the 0.5 m one gives way to the photographs), so the land from
  altitude keeps its old variation. Once the small scale is gone, the big one has its contrast
  raised by a third on the three grasses only, so the air patches keep a fine grain; raised on the
  crops, it drew their blades as strokes from 150 m.
- **Forest canopy**: beyond the single trees `forest_ground` paints the ground as crowns seen from
  above; the photographs fade out there.
- **Sheen**: roughness drives a small highlight on runway, apron, roads and sand within 1.6 m of
  sea level; nowhere else.
- **Cost** (section 7): with the air scale, up to one more colour fetch per pixel of ground out to
  2.25 `TEX_RANGE`. Measured with `--shot` (median of 240 frames, M5 Pro) against the game before
  the air scale was blurred and the town plots got their materials: the same within 0.03 ms
  (runway 3.56, cockpit at 900 m in `medium` 5.61, outside at 150 m 7.41, the same in `low` 3.20).
  Not measured on an integrated GPU.
