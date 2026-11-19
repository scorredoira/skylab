# Third-party notices

Skylab is under the MIT licence (`LICENSE`). Parts of it are ported from the work of others, under
the licences below. Each ported file says so in its first lines. Recordings of others are in its assets, as
the sections below say.

## three-geospatial (the volumetric clouds)

The volumetric clouds of `crates/engine/world-render` are a port (GLSL to WGSL, TypeScript to Rust)
of the clouds of [three-geospatial](https://github.com/takram-design-engineering/three-geospatial)
by Shota Matsuda (Takram), its package `packages/clouds`. Nothing of `packages/atmosphere` was
ported: the light of the sun and of the sky on the clouds and the haze of the air come from our own
sky.

Our files that derive from theirs:

| Ours (`crates/engine/world-render/`) | Theirs (`packages/clouds/src/`) |
|---|---|
| `src/clouds.wgsl` | `shaders/parameters.glsl`, `shaders/clouds.glsl`, `shaders/clouds.frag` |
| `src/clouds_draw.wgsl` | `shaders/clouds.frag`, `shaders/cloudsResolve.frag`, `shaders/varianceClipping.glsl` |
| `src/clouds_shadow.wgsl` | `shaders/shadow.frag`, `shaders/shadowResolve.frag`, `shaders/varianceClipping.glsl` |
| `src/cloud_noise.wgsl` | `shaders/perlin.glsl`, `shaders/tileableNoise.glsl`, `shaders/cloudShape.frag`, `shaders/cloudShapeDetail.frag`, `shaders/turbulence.frag`, `shaders/localWeather.frag` |
| `src/cloud_noise.rs` | `CloudShape.ts`, `CloudShapeDetail.ts`, `Turbulence.ts`, `LocalWeather.ts`, `Procedural3DTexture.ts`, `ProceduralTexture.ts` |
| `src/clouds.rs` | `CloudLayer.ts`, `CloudLayers.ts`, `DensityProfile.ts`, `uniforms.ts`, `bayer.ts`, `CloudsPass.ts`, `ShadowPass.ts`, `CloudsEffect.ts` |
| `src/quality.rs` (`CloudMarch`) | `qualityPresets.ts` |
| `examples/cloud_lab.wgsl`, `examples/cloud_lab.rs` | use the files above |

```
The MIT License (MIT)

Copyright (c) 2024 Shota Matsuda

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

What their package took from others, and whether it is in ours:

- **Tileable volume noise** (their `cloudShape.frag`, `cloudShapeDetail.frag`, `tileableNoise.glsl`,
  after [TileableVolumeNoise](https://github.com/sebh/TileableVolumeNoise) by Sébastien Hillaire):
  ported, in `src/cloud_noise.wgsl`.

```
The MIT License (MIT)

Copyright(c) 2017 Sébastien Hillaire

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- **Periodic Perlin noise** (their `perlin.glsl`, ported from
  [GLM](https://github.com/g-truc/glm), `glm/gtc/noise.inl`): ported, in `src/cloud_noise.wgsl`.
  GLM is under either of two licences; we take it under the MIT one.

```
OpenGL Mathematics (GLM)

GLM is licensed under The Happy Bunny License or MIT License

The Happy Bunny License (Modified MIT License)

Copyright (c) 2005 - G-Truc Creation

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

Restrictions:
By making use of the Software for military purposes, you choose to make a
Bunny unhappy.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.

The MIT License

Copyright (c) 2005 - G-Truc Creation

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

- **Clipping the history to the box of the neighbours' colours** (`clipAABB` in their
  `varianceClipping.glsl`, after [Playdead's temporal reprojection](https://github.com/playdeadgames/temporal),
  whose files say "Copyright (c) <2015> <Playdead>" and "subject to the MIT License"): ported, in
  `src/clouds_draw.wgsl` and `src/clouds_shadow.wgsl`.

```
The MIT License (MIT)

Copyright (c) <2015> <Playdead>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

- **How the coverage shows the weather** (their `sampleWeather`, which cites
  [Skybolt](https://github.com/Prograda/Skybolt)'s `Clouds.h`, under the Mozilla Public License 2.0):
  **not ported**. Ours (`cloud_weather_at` in `src/clouds.wgsl`) is the threshold of Schneider's
  "The real-time volumetric cloudscapes of Horizon Zero Dawn" (2015), the weather over a level that
  the coverage lowers, with its level and width set so that it draws the same curve.
- **Structured volume sampling** (their `structuredSampling.glsl`, after
  [volsample](https://github.com/huwb/volsample) and a Shadertoy): not ported; the shadow map is
  marched in plain steps with a jitter.
- **Cascaded shadow maps** (their `CascadedShadowMaps.ts`, `helpers/FrustumCorners.ts`, after
  three-csm): not ported; ours is one map over the flat world.
- **Catmull-Rom sampling** (their `catmullRomSampling.glsl`, after MJP): not ported.
- **Spatiotemporal blue noise** (the texture `packages/core/assets/stbn.bin`, NVIDIA's): not used.
  Its licence could not be established; the blue noise of `src/cloud_noise.rs` is made by our own
  code (Ulichney's void and cluster) and moved between frames by the golden ratio.
- Their other textures (`packages/clouds/assets`) are not used either: ours are drawn at the start
  by the ported generators.

## Sound recordings (the driving game's car)

The GT's sound (`assets/cars/gt/sound/`) is made from recordings published on Freesound under
**CC0 1.0** (public domain dedication): no conditions, credited anyway. Its `CREDITS.md` lists each
file with its title, author, page and what was done to it.

- The engine and the starter: the pack "The Dyno Sessions: 190E" by Chippy569 (recorded by David of
  Track Time Audio), <https://freesound.org/people/Chippy569/packs/9658/>: a Mercedes-Benz
  190E 2.3-16V on a dynamometer.
- The tyres: "Chrysler LHS tire squeal 04" and "Nissan Maxima burnout" by audible-edge.
- The road: "Tire noise on the highway" by ViveleVent; the kerb: "rumble strip exit 41" by
  bsumusictech; gravel: "Tires on Gravel Road 1" by OBXJohn; the wind: "Wind-inside car-091007" by
  ra_gun.
- Impacts and scraping: "Car Crash" by squareal, "MetalScrape_4" by deerlord.

## Fonts (the flight game's menus)

In `assets/fonts`, built into the program, used by the menus of the flight game
(`crates/games/flight/src/ui/menu/theme.rs`) and named in their "About" section:

- **Barlow** (Regular, Medium, SemiBold), copyright 2017 The Barlow Project Authors
  (<https://github.com/jpt/barlow>), under the SIL Open Font License 1.1: its text is
  `assets/fonts/Barlow-OFL.txt`.
- **Phosphor Icons** (the regular weight's icon font), copyright 2020-2021 Phosphor Icons
  (<https://phosphoricons.com>), under the MIT licence: its text is
  `assets/fonts/Phosphor-LICENSE.txt`.

The pictures of the menus (`assets/aircraft/*/picture.jpg`, `assets/menu/places/*.jpg`) are taken
in the game itself (`scripts/menu-shots.sh`); those of real places are a produced work of the data
below, credited as the game credits it.

## Data of real places

None of it is in the repository: `crates/sim/geodata` downloads it on each computer and keeps it in
that computer's cache (its README says what, from where and under which terms). What the game
draws from it is a produced work of those databases; the game names them on screen while a real
place is flown (`flight`'s `places::CREDIT`) and in its settings (`geodata::ATTRIBUTION`).

- **Overture Maps** (buildings, and OpenStreetMap's bridges), `tiles.overturemaps.org`, found with
  its STAC catalogue: the buildings theme and the base theme are under the
  [Open Database License](https://opendatacommons.org/licenses/odbl/1-0/) (ODbL 1.0). Overture's
  attribution: "Overture Maps Foundation, overturemaps.org", and for the buildings, of the sources
  it merges: "© OpenStreetMap contributors. Available under the Open Database License."; "Licensed
  by Microsoft under the Open Database License."; "Google Open Buildings" (CC BY 4.0); "Esri
  Community Maps contributors" (CC BY 4.0); "USGS 3D Elevation Program"; Qian Shi et al. (Zenodo,
  2023, CC BY 4.0); "Work derived from BTN 2024 ign.es." (CC BY 4.0).
- **OpenStreetMap** through **OpenFreeMap** (coast, water, cover, roads): © OpenStreetMap
  contributors, ODbL; the schema © OpenMapTiles (CC BY 4.0).
- **AWS Terrain Tiles** (Mapzen's Terrarium heights), **OurAirports** (public domain) and
  **Copernicus Sentinel-2** ("Contains modified Copernicus Sentinel data"), as `geodata`'s README
  lists them.
- The bridges made by hand (`geodata::bridges::LANDMARKS`) are our own work from published
  dimensions (the sources are named beside each), placed where OpenStreetMap draws them.
