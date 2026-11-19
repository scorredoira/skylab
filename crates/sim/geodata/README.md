# geodata

Real places: downloads free, open map data, keeps it on disk and builds from it a
`sim_core::Region` before the world is made (seam 3 of `docs/ROADMAP.md`). It is the only code
of the project that touches the network. `sim-core` stays offline, pure and deterministic: it
gets plain grids and lists, never waits on a download, and the same region kept on disk always
gives the same world.

Depends on `sim-core` (for `Region` and its parts) and nothing else of ours; the games use it.

## The contract

```rust
let place = geodata::Place::parse("LEMH").unwrap();          // an aerodrome's code, or "39.86,4.22"
let loaded = geodata::load(&place, offline, &|p: Progress| { /* p.text(), p.share() */ })?;
let terrain = Arc::new(Terrain::from_region(seed, Arc::new(loaded.region)));
let world = World::with_terrain(seed, terrain);
```

`load` blocks until the region is whole; call it on a thread of its own to keep a window drawing
(`flight`'s `places::Loading`). `progress` is called from the loading threads with the stage
(`Stage`), how much of it is done and the bytes downloaded. `offline` reads only the cache: what
is not there is an `Error::Offline`. Errors are said to the player in plain words (`Display`).
`PRESETS` are the places the game features (an island, a mountain valley, a big city, then a fjord,
Lofoten, a Caribbean island, mountain strips, Madeira, Hong Kong, San Francisco, Queenstown);
`ATTRIBUTION` is what the data's licences ask to be shown with it.

For the game's place picker, three more things, none of which builds a region:

```rust
let index = geodata::search::Index::load(&fetcher)?;      // OurAirports' aerodromes, from the cache (0.25 s)
let best: Vec<usize> = index.search("barcelona", 20);     // places in index.airfields, the best first (1 to 2 ms)
let need = geodata::cache::need(&place, lat, lon);       // kept already, or about how many bytes to download
let bytes = geodata::basemap::Basemap::new(false).tile(id)?; // a map tile to draw (zoom 0 to 14), cached
```

- `search` (`search.rs`): every aerodrome a region can be built round (small, medium and large, as
  `Tables::parse` keeps them; 48 080), with its codes, town, country (ISO, and its name in Spanish
  and English), size, whether airlines fly from it, and its runways as a region places them (open,
  not on water, 250 m or more) with both ends and their true headings. A code typed whole or its
  start finds it, or every word typed starting one of its words, any case and without accents;
  the best first: a whole code, then the town, then the name, then the bigger, with airlines, the
  longer runway.
- `cache::need` (`cache.rs`): whether the place's region is kept (by this version of `store`), or
  the tiles of its four grids missing from the cache times what such a tile weighs on average,
  and the tables if they are missing. Only names of files are looked at.
- `basemap` (`basemap.rs`): OpenFreeMap's vector tiles by `TileId`, kept in the same place a
  region's are (`openfreemap/z/x/y.pbf`), fetched as politely; the address of the tiles is asked
  once, only when a tile is not in the cache.

## What a region holds

Round the place: the middle of the main runway of an aerodrome (so the world starts on it), or the
point given. North and east are the azimuthal equidistant projection round it (`geo.rs`):
distances and bearings from the origin are exact, the error across 100 km is a few metres, and
runway headings are taken between their projected ends.

| | side | step | from |
|---|---|---|---|
| heights near | 24 km | 12 m | Terrarium, zoom 13 |
| heights | 96 km | 30 m | Terrarium, zoom 12 |
| heights far | 400 km | 400 m | Terrarium, zoom 8 |
| what covers the ground, the coast, roads, towns | 96 km | 24 m | OpenFreeMap, zoom 12 |
| buildings, where the ground is built up | 32 km | | Overture Maps, zoom 14 |
| bridges, where the maps have them | 32 km | | Overture Maps, zoom 13 |
| aerodromes and runways | 96 km | | OurAirports |

- **Heights** (`terrarium.rs`): each node of each grid sampled bilinearly from the tiles of its zoom.
  `Region::height` eases each grid into the next coarser one over its last 12 nodes.
- **Cover** (`build.rs`, `cover_grid`): the polygons of OpenMapTiles' layers `landcover` (wood,
  farmland, grass, scrub, rock, sand, ice, wetland), `landuse` (residential, commercial, industrial,
  retail: built up; railway: a railway's tracks, yards and stations, over the built up), `aeroway`
  (the aerodrome's ground: mown grass) and `water` (ocean: sea; lakes,
  reservoirs, wide rivers: inland water), each layer filled over the one before (even-odd, holes
  kept). Where the maps say nothing the node is `Cover::Unknown` and the ground is the seed's.
- **The coast** (`coast`): the fine heights read 0 or so over the sea, so the maps decide it. The
  sea (OpenMapTiles' ocean and docks) is filled on each grid of heights at its own step, and near
  the coastline the heights go by the exact distance to its edges (`Coastline`), the same slope
  on both sides of the waterline, so the 0 between two nodes falls on the map's coastline rather
  than stepping from node to node. What the land behind a stretch is decides the bed (`Shore`):
  off a beach (sand) a gentle shelf from 0.3 m, the land rising slowly from the water; off a port,
  an industry or shops (`industrial`, `commercial`, `retail`) 12 m deep at once, the land standing
  2.5 m as a quay; off a town's houses 6 m; off rock, woods and fields 2 m; then down 4 % a metre.
  Far out (from 1.5 to 3 km off) the far grid's bathymetry (ETOPO1) takes over where it is
  shallower: nearer the shore its 400 m are too coarse to know it. Out of the coastline's reach
  (64 m) the distance and the kind of shore come from a chamfer over the cover's grid, so a
  harbour is as deep in its middle as by its quays. Land is 0.5 m up at least; lakes keep their heights.
- **Roads** (`transportation`): motorway 22 m, trunk 16, primary 11, secondary 9, tertiary 7.5,
  minor 6 (few at zoom 12), from kerb to kerb; the maps draw a motorway or a trunk road with two
  carriageways as a way each (`oneway`), 12 m and 9 m, and each slip road as a way of its own
  (`ramp`), 6.5 m at most: drawn as wide as the whole road, the dozens of ways of a city's
  junction were one slab of asphalt (Madrid's M-30); not tunnels, tracks, paths or railways, nor bridges (each is its own,
  below: painted on the ground it would cross a river on the water). Each stretch once, from the
  tile it is in.
- **Aerodromes** (`ourairports.rs`): aeroplane aerodromes whose reference point is in the square,
  each open runway of 250 m or more as one `Airport` (they share the aerodrome's id), from its two
  ends when the table has them. Width from the table (30 m paved, 20 m grass if missing); surface
  paved or not; its lights by kind, length and whether it is lit (`ourairports::class`: unlit or
  grass, visual; big aerodromes' long runways category II, the rest category I; medium ones
  category I from 1800 m, non-precision below; small lit ones non-precision from 1200 m). The main
  runway (paved first, then longest) has the apron, hangars, office, windsock and floodlights, on
  the side away from the other runways, or of the reference point (usually the terminal). Every
  runway of an aerodrome is flattened to one height: halfway between the main runway's ends in the
  table, or the aerodrome's, unless that is more than 20 m from the ground's, then the ground's
  (the median along the runway).
- **Buildings** (`build::buildings`, `overture::read_buildings`): Overture Maps' buildings at zoom
  14 (`overture`), over the 32 km round the origin, only the tiles with built-up ground: OpenStreetMap's
  buildings, and where it has none those Microsoft, Google and Esri found in aerial pictures. Each
  building once (from the tile its middle is in: a tile has a margin of its neighbours'), its
  outline and its courtyards (`Footprint::holes`), simplified (corners nearer than 30 cm, or in a
  straight line, dropped), with what the maps say of it: its height (`height`, else its floors at a
  storey for its use, `overture::storey`: 3.1 m for flats, 3.8 for offices and shops, 4.5 for
  sheds, churches and public buildings) and bottom (`min_height`, `min_floor`); its roof's shape
  (`roof_shape`: flat, gabled, hipped, pyramidal, skillion, dome, onion, round, gambrel, mansard, the
  rarer ones as the nearest of these), height, direction (`roof_direction`, `roof_orientation`) and
  colour or material; its use (`class` and `subtype`: a house, flats, offices, shops, industry, a
  church, a public building, an outbuilding); its walls' material and colour. A building drawn by its
  parts (`has_parts`, the parts there: a tower on its podium, the floors of the leaning Puerta de
  Europa) is left out for them; each part is a footprint of its own, on its bottom. Underground
  ones are not read. Then `settle`: a building the maps give no height (a third or more in a city)
  is as tall as the median of the measured blocks of its square of 200 m when it is a block, as its
  use makes it otherwise (a house 6.5 m, a shed 3.5 m); a small house standing on its own whose roof
  they do not describe gets a gabled or hipped one, and a church a gabled one; a house that shares a
  wall with another (a terrace) keeps a flat one. Madrid holds some 310 000 in the 32 km,
  New York a million (most of them Queens' and Brooklyn's houses).
- **Bridges** (`bridges.rs`, `overture::read_bridges`): OpenStreetMap's ways tagged `bridge=*` that
  carry a road, a railway or a path, as Overture's `base` theme has them at zoom 13 (layer
  `infrastructure`, with every tag of the way: `highway`, `railway`, `layer`, `lanes`, `width`,
  `bridge:structure`, `colour`, `wikidata`), read only under the tiles of OpenFreeMap where the maps
  show bridges, within the 32 km. A way is cut into pieces by the tiles; the pieces of each are
  joined again by its id (`overture::join`). Each gets a width (tagged, or by its lanes, or its kind
  of road: 13 m a carriageway of a motorway, 3.5 m a footbridge, 4.5 m a track of railway) and a
  structure (`bridge:structure`: suspension, cable-stayed, arch, truss, else a beam on piers), and a
  deck (`profile`): where an end is on land it meets the ground (or stands up to 2 m over it on its
  abutment); over water it is free (a long bridge is often several ways). In between the deck
  stands over what it crosses by a clearance (5.2 m over a road for a road or a railway, 3 m for a
  path; over water 2.5 m for a path, else 5 m and 2 % of its length up to 25 m, for the ships;
  0.6 m on `layer` 0), never climbing faster than what it carries does (6 % a road, 3 % a railway, 10 %
  a path), smoothed. What is under a point is the lowest ground across the way and some more, as
  the heights the satellites measured may hold the bridge itself. Neither the ground nor the water
  under a bridge is changed. New York has some 7800 (with the elevated railways), Madrid some 2000.
- **Landmarks** (`bridges::LANDMARKS`): famous bridges made by hand from their published
  dimensions (named beside each row), in the place of the maps' ways of them (those tagged with
  the same Wikidata item, named after it, or lying along it): the Golden Gate Bridge (1280 m between
  towers 227 m above the water, the roadway 75 m up, its cables, hangers and International Orange)
  and the Brooklyn Bridge (486 m between stone towers 84 m high, pierced by two arches). A row is
  the line of the deck with its heights, where the towers stand, how high they rise, the width,
  the structure and the colour, and the approach viaducts to the ground; another bridge is another
  row.
- **Towns** (`towns`): circles of 220 m to 1 km, the biggest first, wherever 80 % of the circle is
  built up, away from each other and from the aerodromes; their street grids run along the roads
  round them (the length of road by direction, modulo a right angle). `sim_core::scenery` puts
  houses only on the built-up ground of them.

## Beyond the region: following the aircraft

A region serves 96 km; the aircraft may fly on (`follow.rs`). Every region of a place keeps its
origin and axes, only its middle moves, on a lattice of 24 km (`follow::LATTICE`, a multiple of
the step of every grid of a region and of its picture, so that their nodes fall where the region
before had them: the same tiles, the same heights, the same cover). `Follow::next`, given the
aircraft's position and velocity over the ground, asks for the region round where it will be in
three minutes when it, or that point, is more than 30 km from the middle of its region
(`follow::SERVES`); the game builds it on a thread (`load_around`, kept as
`regions/<place>@<n>_<e>.region` like the place's own: flown again, and offline, it is read back)
and flies on it once it has come (`Follow::arrived`): positions, wrecks and the old picture of the
ground stay where they were, as the axes are the same.

```rust
let mut follow = geodata::follow::Follow::new((0.0, 0.0));
if let Some(centre) = follow.next(n, e, vn, ve) { /* on a thread: geodata::load_around(&place, centre, offline, ..) */ }
// once built: if follow.arrived(n, e) { world.terrain = new; renderer.replace_terrain(new) }
```

`cargo run --release -p geodata --example follow -- LEBL km=300 heading=280` flies a C172 on its
autopilot 300 km in a straight line from Barcelona, 20 times faster than real time, without a
window, as the game does, and says what each region cost, how long the swap took, how much the
ground under the aircraft moved at it, and the peak memory (see `world-render`'s README for what
the swap costs a frame). On this computer, 300 km west of Barcelona at 2500 m: 12 regions, each
built in 10 to 17 s (10 to 20 MB of new tiles; 169 MB in all), swapped in under 1 ms (the old one
freed on a thread; freeing it in place took 5 ms), the ground under the aircraft 0.35 m higher or
lower at a swap at most (where the new region has its finer grid), and the process never above
722 MB. At 20 times real time the aircraft went 8.8 km past what a region serves while the next
was built (still within its maps); at real speed a region is asked for three minutes of flight
before it is needed and comes in a fraction of that.

## The ground from the satellites

After the region, in the background, the colour of its ground as Sentinel-2 saw it (`imagery.rs`,
`sentinel.rs`), for the renderer to draw (`sim_core::Imagery`; the simulation never reads it):

```rust
let loaded = geodata::imagery::load(&region, offline, &|p: imagery::Progress| { /* p.text(), p.share() */ })?;
renderer.set_imagery(gpu, Some(Arc::new(loaded.imagery)));   // world-render
let credit = geodata::imagery::attribution(&loaded.imagery.years); // "Contains modified Copernicus Sentinel data 2024-2025"
```

| | side | step | days | read from |
|---|---|---|---|---|
| near | 30.7 km | 10 m | 2 | the true colour picture at 10 m, the classes at 20 m |
| far | 98 km | 32 m | 3 | the picture's overview at 40 m, the classes' at 40 m |

Round the middle of the region's maps, in its projection, 3072 texels a side each.

- **Which scenes**: Element 84's STAC catalogue (`earth-search`, no key) is asked for the
  Sentinel-2 L2A scenes over the place in the summers of 2023 to 2025 (June to mid September; in
  the south, December to mid March), under 20 % cloud, the least clouded first (`sentinel::search`).
  The best days are taken whole (`sentinel::choose`): one day is one pass of the satellite, cut
  into the MGRS squares of 109.8 km of its scenes, and a day is better the more of the place's
  squares it holds and the less clouded and empty they are. Fixed summers: a place looks the same
  on every computer whenever it is made.
- **What is read of a scene** (`tiff.rs`, `utm.rs`): its files are Cloud-Optimized GeoTIFFs on AWS
  Open Data (`sentinel-cogs.s3.us-west-2.amazonaws.com`, public, free); the first 64 KB give every
  tile of every level, and only the tiles of land the grids touch are fetched, by HTTP range
  requests (`fetch::Fetcher::cached_range`; the open sea is the sea's own to draw, so a coast's
  tiles out at sea are not). From each: the true colour picture (`visual`, 8 bits a channel:
  reflectance times 1020, so it clips at 0.25, a dry field or a roof), its classes (`scl`: ground,
  water, snow, cloud, cloud shadow, cirrus) and, where the picture clipped, the red, green and
  blue bands' own reflectance at 80 m. Each grid's node is projected from the world's axes to the
  scene's UTM zone (Krüger's series), exactly every 32 nodes and bilinearly between.
- **The cloud-free picture**: at each texel, each day's look (two scenes of one day where squares
  overlap blend over 4 km from the edge of each, as the correction for the air is made per square
  and the same ground comes out a little brighter in one than the other); of the looks that see
  the ground, the median brightness (of two, the darker: a missed shadow is darker, haze
  brighter); else of the doubtful ones (unclassified, often the edge of a cloud); else the darkest.
  A near texel no day sees is left unknown, and the far grid is drawn there.
- **Its colour** is the reflectance the sensor measured over the ground after Sen2Cor's
  correction for the air, which is the albedo the renderer lights (forest some 0.03 to 0.06,
  Menorca's dry fields 0.15 to 0.25), kept as sRGB so that the darks keep their steps in 8 bits.

What it takes, the first time (then the picture is read from the cache in 0.1 to 0.2 s), on the
connection this was written on:

| place | downloaded | time | kept |
|---|---|---|---|
| Menorca (LEMH): an island, the sea left out | 86 MB | 26 s | 9.5 MB |
| Barcelona (LEBL): city and coast | 146 MB | 48 s | 18 MB |
| Innsbruck (LOWI): all land, mountains | 194 MB | 67 s | 35 MB |

The loading thread peaks at 750 MB (the decoded tiles of a band of rows of every scene).

## The cache

Under `$FLIGHT_CACHE`, or the user's cache folder (`~/Library/Caches/flight` on macOS,
`$XDG_CACHE_HOME/flight` or `~/.cache/flight` elsewhere), never in the repository:
`terrarium/z/x/y.png`, `openfreemap/z/x/y.pbf` (zoom 12), `ourairports/*.csv` as they came,
`overture/buildings/14/x/y.mvt` and `overture/base/13/x/y.mvt` the tiles of Overture's files as
they came (gzipped vector tiles; one zero byte for a tile the file does not have), under names
without the release, so a place built once reads them again offline whatever the release is by
then; `overture/<release>/<theme>/head` and `.../leaf/<offset>` the parts of a release's file that
say where its tiles are, and
`sentinel2/<scene>/<asset>/<level>/<column>_<row>` the parts of the Sentinel-2 files fetched
(as they came, deflated; with `head`, their first 64 KB), `imagery/<lat>_<lon>_<n>_<e>.sat` the
picture of a region's ground (`imagery::encode`: its grids as PNGs after a magic word and
`imagery`'s `VERSION`), and
`regions/<place>.region`, the region built (`store.rs`: plain little-endian numbers after a magic word
and `store::VERSION`; a file of another version is built again from the tiles). Nothing is ever
deleted: remove the folder to download anew. A file is written under a temporary name and renamed,
so an interrupted download leaves nothing half written.

Requests are polite: a User-Agent that names the program and nothing of the person
(`flight/<version> (open-source flight simulator)`), four at a time to a server and 40 ms
apart, each file once. A first place downloads 20 to 50 MB of tiles and, once for all places, the
17 MB of OurAirports' tables; a region kept is about 80 MB.

On the web (`wasm32`) the same code loads on the game's workers (`engine::threads`), where it may
wait: the network is the browser's synchronous requests (`fetch::http`; every source answers a page
from elsewhere, CORS, and the browser sets the User-Agent), and the cache is the browser's Cache
Storage under `cache/<key>`, kept by the page's service worker (`flight`'s `web/sw.js`) and read
and written with the same requests (`disk.rs`). std cannot start a thread in a worker, so there a
region is built on one thread and its tiles are downloaded one at a time (`threads.rs`): Menorca's
first load took some 55 s for the region and 75 s more for the satellites' picture in Chrome on
this computer (natively about 30 and 26); from the cache, a few seconds. The page's own thread
may not wait, so there nothing is read or kept: the game asks `cache::need` from a worker.

## Sources and licences

| Data | Where | Terms |
|---|---|---|
| Heights | AWS Terrain Tiles, Terrarium (`s3.amazonaws.com/elevation-tiles-prod`), public, no key | Mapzen's mix of open sources, attribution asked: SRTM, GMTED2010, ETOPO1 (public domain), USGS 3DEP and NED (public domain), Copernicus EU-DEM, and national models (Canada, United Kingdom, Australia, New Zealand, Norway, Mexico) under OGL or CC BY |
| Coast, water, woods, fields, towns, roads, aerodrome grounds | OpenFreeMap's vector tiles (`tiles.openfreemap.org`), no key | © OpenStreetMap contributors, ODbL; schema © OpenMapTiles (CC BY) |
| Buildings; bridges with their tags | Overture Maps' PMTiles (`tiles.overturemaps.org/<release>/buildings.pmtiles`, `base.pmtiles`), the release named by its STAC catalogue (`stac.overturemaps.org`), no key, read by HTTP range requests | Open Database License (ODbL 1.0); "Overture Maps Foundation, overturemaps.org"; of the buildings it merges, "© OpenStreetMap contributors", "Licensed by Microsoft under the Open Database License", "Google Open Buildings" (CC BY 4.0), "Esri Community Maps contributors" (CC BY 4.0), and the few from USGS 3DEP, Qian Shi et al. (CC BY 4.0) and Spain's BTN (CC BY 4.0) |
| Aerodromes and runways | OurAirports (`davidmegginson.github.io/ourairports-data`) | Public domain |
| The colour of the ground | Copernicus Sentinel-2 L2A on AWS Open Data (`sentinel-cogs`), found with Element 84's STAC catalogue (`earth-search.aws.element84.com`), no key | Free and open for any use; "Contains modified Copernicus Sentinel data <year>" (the Legal Notice on the use of Copernicus Sentinel Data) |

The data is downloaded on each computer and
kept there; none of it is distributed with the project (the test fixtures are made up, but for a
few rows of OurAirports' public-domain tables). No paid service, nothing that needs a key.

The readers are ours, after the formats' published specifications: the vector tiles (`mvt.rs`)
after the Mapbox Vector Tile specification (version 2.1) and the protobuf wire format, the heights
(`terrarium.rs`) after the Terrain Tiles' documentation of the Terrarium encoding, and the PMTiles
(`pmtiles.rs`) after Protomaps' specification (version 3).

### What a city costs

Overture's tiles of a city, the first time, on the connection this was written on, and the time
to download them and build the region (the terrain and maps already kept; a region already built
reads in a second):

| place | buildings | bridges | in all | time |
|---|---|---|---|---|
| Madrid (`--at 40.4766,-3.6880`) | 237 tiles, 26 MB | 96 tiles, 32 MB | 60 MB | 28 s |
| Manhattan (`--at 40.7549,-73.9840`) | 290 tiles, 78 MB | 90 tiles, 43 MB | 121 MB | 19 s |

Why PMTiles: Overture publishes its data as GeoParquet too (some 200 files of a gigabyte a theme,
sorted in space, with a box per row group), which could be read by range requests as well, but
needs a Parquet reader (a large dependency) and a geometry decoder, and reads a city's row groups
whole, with every column. The PMTiles are the same data cut into vector tiles, which `mvt` already
reads, with every property of each building: a tile is one request.

## Checking a change

`cargo test -p geodata`: the projection there and back and the tiles' numbering, PMTiles' numbering
of tiles along the Hilbert curve and a directory read, Overture's tags made into a building (uses,
roofs, colours), a tile's buildings read once with their parts, the pieces of a bridge cut by the
tiles joined again, a bridge's deck meeting the banks and clearing a river without changing it,
the Golden Gate and the Brooklyn Bridge standing where their rows say, the search (codes,
words without accents, the order of what is found, the runways of an aerodrome), what the cache
holds of a place, the coast (its
waterline on the map's line between the nodes, deep by a quay, shallow off a beach), towns over built ground following its streets, the region written and read back, and on the
small files of `tests/fixtures` (no network; `src/fixtures.rs` says what they are): a Terrarium tile
decoded, a vector tile's layers, geometry and properties, a map tile laid on the cover's grid with
its roads, OurAirports' tables read and runways placed with their heights, lights and apron;
and for the satellites' picture: a GeoTIFF made up in the test read through its tiles (deflate and
the predictor), UTM against Snyder's series, a catalogue's answer (`tests/fixtures/stac.json`, made
up) read into scenes and the best days chosen, a texel's median of the days that see the ground,
two scenes of a day blended, the clipped picture given back from the bands, the picture kept
and read back.

`cargo run --release -p geodata --example search -- <words> [--offline]` searches the aerodromes as
the picker does and says how long it took. `cargo run --release -p geodata --example region -- <ICAO | lat,lon> [--offline]` loads a place
(downloading it the first time) and says what it holds; `--example imagery -- <ICAO | lat,lon>
[--offline] [--png <dir>]` makes the picture of its ground, says what it cost, and writes its
grids as pictures to look at; the viewer draws it
(`world-render`'s `viewer -- place=<ICAO>`), and the game flies it (`flight --place <ICAO>`).
