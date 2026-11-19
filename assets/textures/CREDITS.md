# Textures: sources and licence

Every file in this folder is derived from a material published under **CC0 1.0** (public domain
dedication) by [Poly Haven](https://polyhaven.com/license) or
[ambientCG](https://docs.ambientcg.com/license/). Both sites publish their whole catalogue under
CC0; nothing here comes from anywhere else. No credit is required by the licence; it is given
anyway.

The originals were downloaded at 2K and shrunk to 1024 x 1024 (the barks to 512) by `fetch.sh` (in this folder) with
`crates/engine/engine/examples/prepare_textures.rs`. Running `assets/textures/fetch.sh <cache_dir>` from the
repository root rebuilds the folder.

## Files per material

| File | Content | Format |
|---|---|---|
| `<name>_color.jpg` | albedo, sRGB | JPEG, quality 90 |
| `<name>_normal.png` | tangent-space normal, **OpenGL convention** (red = +u, green = up the image, blue = out) | PNG, RGB 8 bit |
| `<name>_rough.jpg` | roughness, linear, 0 = mirror, 1 = matt | JPEG grey, quality 80 |
| `<name>_ao.jpg` | ambient occlusion, linear, 1 = open | JPEG grey, quality 80 |

Both sites ship the OpenGL normal map (`nor_gl`, `NormalGL`); nothing had to be converted.
`metal_painted` has no occlusion map: its source has none.

## Materials

"Metres" is the real size of one repeat as its author measured it; "-" where the site does not say.
Method: photo = photogrammetry or photographs; procedural = authored in a material editor.

| Name | Source | Author | Method | Metres | Page |
|---|---|---|---|---|---|
| `grass_short` | ambientCG, Grass004 | ambientCG.com | procedural | 1.4 | https://ambientcg.com/a/Grass004 |
| `grass_meadow` | ambientCG, Ground037 | ambientCG.com | photo | 2.1 | https://ambientcg.com/a/Ground037 |
| `grass_dry` | Poly Haven, grass_ground | Charlotte Baglioni | photo | 2.5 | https://polyhaven.com/a/grass_ground |
| `soil_ploughed` | Poly Haven, farm_soil | Amal Kumar | photo | 2.0 | https://polyhaven.com/a/farm_soil |
| `crop_stubble` | Poly Haven, withered_grass | Charlotte Baglioni | photo | 2.0 | https://polyhaven.com/a/withered_grass |
| `crop_green` | Poly Haven, sparse_grass | Amal Kumar | photo | 2.0 | https://polyhaven.com/a/sparse_grass |
| `forest_floor` | Poly Haven, forrest_ground_03 | Rob Tuytel | photo | 2.0 | https://polyhaven.com/a/forrest_ground_03 |
| `rock_granite` | Poly Haven, rock_04 | Rob Tuytel | photo | 1.5 | https://polyhaven.com/a/rock_04 |
| `rock_layered` | ambientCG, Rock023 | ambientCG.com | photo | - | https://ambientcg.com/a/Rock023 |
| `scree` | Poly Haven, rocks_ground_09 | Rob Tuytel | photo | 3.0 | https://polyhaven.com/a/rocks_ground_09 |
| `snow` | ambientCG, Snow010A | ambientCG.com | procedural | - | https://ambientcg.com/a/Snow010A |
| `sand` | Poly Haven, dense_sand | Dimitrios Savva | photo | 1.8 | https://polyhaven.com/a/dense_sand |
| `asphalt` | Poly Haven, asphalt_04 | Jenelle van Heerden, Sergej Majboroda | photo | 4.0 | https://polyhaven.com/a/asphalt_04 |
| `concrete` | Poly Haven, brushed_concrete | Dario Barresi, Dimitrios Savva | photo | 2.5 | https://polyhaven.com/a/brushed_concrete |
| `dirt_road` | Poly Haven, dirt_floor | eye-candy.xyz | photo | 2.1 | https://polyhaven.com/a/dirt_floor |
| `metal_painted` | ambientCG, Metal016 | ambientCG.com | procedural | - | https://ambientcg.com/a/Metal016 |
| `metal_corrugated` | Poly Haven, corrugated_iron_02 | Jenelle van Heerden, Sergej Majboroda | photo | 2.7 | https://polyhaven.com/a/corrugated_iron_02 |
| `plaster` | Poly Haven, painted_plaster_wall | Amal Kumar | photo | 2.0 | https://polyhaven.com/a/painted_plaster_wall |
| `roof_clay` | Poly Haven, clay_roof_tiles_02 | Amal Kumar | photo | 2.5 | https://polyhaven.com/a/clay_roof_tiles_02 |
| `roof_slate` | Poly Haven, grey_roof_tiles_02 | Rob Tuytel | photo | 1.5 | https://polyhaven.com/a/grey_roof_tiles_02 |
| `plastic` | ambientCG, Plastic010 | ambientCG.com | procedural | - | https://ambientcg.com/a/Plastic010 |
| `leather` | ambientCG, Leather026 | ambientCG.com | procedural | - | https://ambientcg.com/a/Leather026 |
| `fabric` | ambientCG, Fabric036 | ambientCG.com | procedural | - | https://ambientcg.com/a/Fabric036 |
| `carpet` | ambientCG, Carpet012 | ambientCG.com | procedural | - | https://ambientcg.com/a/Carpet012 |
| `rubber` | ambientCG, Rubber004 | ambientCG.com | procedural | 0.75 | https://ambientcg.com/a/Rubber004 |
| `bark_furrowed` | Poly Haven, bark_brown_02 | Rob Tuytel | photo | - | https://polyhaven.com/a/bark_brown_02 |
| `bark_pine` | Poly Haven, pine_bark | Dimitrios Savva | photo | - | https://polyhaven.com/a/pine_bark |
| `bark_palm` | Poly Haven, palm_bark | Charlotte Baglioni | photo | - | https://polyhaven.com/a/palm_bark |
| `bark_smooth` | Poly Haven, chinese_hackberry_bark | Charlotte Baglioni | photo | - | https://polyhaven.com/a/chinese_hackberry_bark |

The last five (and `metal_painted`) are the cockpit's: plastic, leather, cloth, carpet and rubber.
`plastic` and `leather` have no occlusion map: their sources have none.

The four barks are the trees' (`world-render`'s `species.rs`): colour and normal map only, at 512 x 512.
The originals of `bark_palm` and `bark_smooth` are twice as tall as wide: they are squeezed into the
square, so a repeat of them is twice as tall as wide.

## Leaves, twigs and grass

Cut out of the textures of Poly Haven's plant models (CC0, the same licence), each the 1K colour map
with its opacity map as alpha, in a rectangle of it, at half size (`fetch.sh`, `cutouts`; the
`cutout` of `prepare_textures`). The renderer finds the separate leaves, twigs and tufts in them and
composes the cards the trees are made of (`world-render`'s `foliage.rs`).

| File | Model | Its maps | Authors | Page |
|---|---|---|---|---|
| `foliage_leaves.png` | Island Tree 01 | `leaves_diff`, `leaves_alpha` | Rico Cilliers, Rob Tuytel | https://polyhaven.com/a/island_tree_01 |
| `foliage_spruce.png` | Fir Tree 01 | `twig_diff`, `twig_alpha` | Rico Cilliers, Rob Tuytel | https://polyhaven.com/a/fir_tree_01 |
| `foliage_pine_1.png`, `foliage_pine_2.png` | Pine Tree 01 | `twig_diff`, `twig_alpha` | Rico Cilliers, Rob Tuytel | https://polyhaven.com/a/pine_tree_01 |
| `foliage_grass.png` | Grass Medium 01 | `Diffuse`, `Alpha` | Rico Cilliers, Rob Tuytel | https://polyhaven.com/a/grass_medium_01 |

The file of each map comes from the site's own download of that material: for Poly Haven the 2K
`Diffuse`, `nor_gl`, `Rough` and `AO` files listed by `https://api.polyhaven.com/files/<id>`; for
ambientCG the archive `https://ambientcg.com/get?file=<id>_2K-PNG.zip`.

## Stand-ins and gaps

- **Crops.** Neither site has a photograph of a cereal or row crop seen from above. `crop_stubble`
  is withered grass (reads as stubble or ripe cereal) and `crop_green` is sparse grass on dark soil
  (reads as a young crop). The rows come from the shader, as they do today.
- **Short grass, snow, painted metal** are procedural materials, not photographs: the photographed
  lawns have weeds and bare patches that repeat visibly, the photographed snow is dirty or
  trampled, and there is no photographed clean painted sheet.
- `rock_layered` has large cracks that show as a regular lattice when it is laid flat over a wide
  area. It is meant for steep slopes, mapped triplanar, where it covers little ground.
