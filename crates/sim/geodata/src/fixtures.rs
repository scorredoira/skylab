//! The tests of what `geodata` reads, on small files in `tests/fixtures`, so that they need no network:
//!
//! - `airports.csv`, `runways.csv`: a few rows of OurAirports' tables (public domain) as they come:
//!   Menorca (LEMH, with two closed runways), Sant Lluís (LESL), Innsbruck (LOWI, with a grass
//!   runway) and a heliport.
//! - `terrarium.png`: a Terrarium height tile of 16 by 16 pixels made up here (no real data): the
//!   height of pixel (x, y) is `10 x − 5 y − 100.5` metres.
//! - `tile.mvt`: a vector tile made up here (no real data) in OpenMapTiles' layers: an ocean in the
//!   west, a wood with a clearing, a built-up area, a lake, a primary road, a tunnel and a path.
//!
//! The made-up ones are written by `write_fixtures` (`cargo test -p geodata -- --ignored write_fixtures`).

use std::path::PathBuf;

use sim_core::lights::Class;
use sim_core::{Cover, FEET};

use crate::build::{cover_grid, read_map};
use crate::geo::{tile_to_earth, tile_xy, Projection, TileId};
use crate::mvt::{self, Kind, Value};
use crate::ourairports::{self, Tables};
use crate::terrarium::{self, Mosaic};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The made-up tile's place: Menorca's tile at zoom 12.
const TILE: TileId = TileId { z: 12, x: 2095, y: 1552 };

/// The height the made-up Terrarium tile has at a pixel.
fn made_up_height(x: usize, y: usize) -> f32 {
    10.0 * x as f32 - 5.0 * y as f32 - 100.5
}

/// Protobuf as the vector tiles write it, just what the made-up tile needs.
#[derive(Default)]
struct Pb(Vec<u8>);

impl Pb {
    fn varint(&mut self, mut v: u64) -> &mut Pb {
        while v >= 0x80 {
            self.0.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.0.push(v as u8);
        self
    }
    fn key(&mut self, field: u64, wire: u64) -> &mut Pb {
        self.varint(field << 3 | wire)
    }
    fn bytes(&mut self, field: u64, b: &[u8]) -> &mut Pb {
        self.key(field, 2).varint(b.len() as u64);
        self.0.extend_from_slice(b);
        self
    }
    fn packed(&mut self, field: u64, values: &[u32]) -> &mut Pb {
        let mut inner = Pb::default();
        for &v in values {
            inner.varint(v as u64);
        }
        self.bytes(field, &inner.0)
    }
}

/// The commands of a geometry: each part moved to, drawn and, for a polygon, closed.
fn geometry(parts: &[Vec<[i32; 2]>], closed: bool) -> Vec<u32> {
    let zigzag = |v: i32| ((v << 1) ^ (v >> 31)) as u32;
    let (mut out, mut at) = (Vec::new(), [0, 0]);
    for part in parts {
        out.extend([1 | (1 << 3), zigzag(part[0][0] - at[0]), zigzag(part[0][1] - at[1])]);
        at = part[0];
        out.push(2 | ((part.len() as u32 - 1) << 3));
        for p in &part[1..] {
            out.extend([zigzag(p[0] - at[0]), zigzag(p[1] - at[1])]);
            at = *p;
        }
        if closed {
            out.push(7 | (1 << 3));
        }
    }
    out
}

fn square(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<[i32; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// A feature as the made-up tile has it: what it is, its properties (key, text value) and its parts.
type Made<'a> = (Kind, &'a [(&'a str, &'a str)], Vec<Vec<[i32; 2]>>);

/// A layer of features.
fn layer(name: &str, features: &[Made]) -> Vec<u8> {
    let (mut keys, mut values): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    let mut l = Pb::default();
    l.bytes(1, name.as_bytes());
    for (kind, properties, parts) in features {
        let mut tags = Vec::new();
        for (k, v) in properties.iter() {
            let ki = keys.iter().position(|x| x == k).unwrap_or_else(|| {
                keys.push(k);
                keys.len() - 1
            });
            let vi = values.iter().position(|x| x == v).unwrap_or_else(|| {
                values.push(v);
                values.len() - 1
            });
            tags.extend([ki as u32, vi as u32]);
        }
        let mut f = Pb::default();
        f.packed(2, &tags);
        f.key(3, 0).varint(match kind {
            Kind::Point => 1,
            Kind::Line => 2,
            _ => 3,
        });
        f.packed(4, &geometry(parts, *kind == Kind::Polygon));
        l.bytes(2, &f.0);
    }
    for k in keys {
        l.bytes(3, k.as_bytes());
    }
    for v in values {
        let mut value = Pb::default();
        value.bytes(1, v.as_bytes());
        l.bytes(4, &value.0);
    }
    l.key(5, 0).varint(4096);
    l.key(15, 0).varint(2);
    l.0
}

fn made_up_tile() -> Vec<u8> {
    let mut tile = Pb::default();
    let water = layer("water", &[(Kind::Polygon, &[("class", "ocean")], vec![square(-64, -64, 1024, 4160)]), (Kind::Polygon, &[("class", "lake")], vec![square(3600, 200, 4000, 600)])]);
    let wood = layer("landcover", &[(Kind::Polygon, &[("class", "wood"), ("subclass", "forest")], vec![square(1500, 500, 3500, 2000), square(2200, 1000, 2800, 1500)])]);
    let town = layer("landuse", &[(Kind::Polygon, &[("class", "residential")], vec![square(1500, 2500, 3000, 3800)])]);
    let roads = layer(
        "transportation",
        &[
            (Kind::Line, &[("class", "primary")], vec![vec![[1100, 2200], [4000, 2200]]]),
            (Kind::Line, &[("class", "primary"), ("brunnel", "tunnel")], vec![vec![[1100, 2300], [4000, 2300]]]),
            (Kind::Line, &[("class", "path")], vec![vec![[1100, 2400], [4000, 2400]]]),
        ],
    );
    for l in [water, wood, town, roads] {
        tile.bytes(3, &l);
    }
    tile.0
}

fn made_up_terrarium() -> Vec<u8> {
    let mut png_bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_bytes, 16, 16);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut pixels = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            let v = made_up_height(x, y) as f64 + 32768.0;
            pixels.extend([(v / 256.0).floor() as u8, (v.floor() as u32 % 256) as u8, (v.fract() * 256.0) as u8]);
        }
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels).unwrap();
    writer.finish().unwrap();
    png_bytes
}

#[test]
#[ignore]
fn write_fixtures() {
    std::fs::write(fixture("tile.mvt"), made_up_tile()).unwrap();
    std::fs::write(fixture("terrarium.png"), made_up_terrarium()).unwrap();
}

#[test]
fn the_made_up_fixtures_are_as_written() {
    assert_eq!(read("tile.mvt"), made_up_tile());
    assert_eq!(read("terrarium.png"), made_up_terrarium());
}

#[test]
fn terrarium_tiles_decode_to_heights() {
    let tile = terrarium::decode(&read("terrarium.png")).unwrap();
    assert_eq!(tile.size, 16);
    for (y, x) in [(0, 0), (3, 7), (15, 15), (15, 0)] {
        assert_eq!(tile.heights[y * 16 + x], made_up_height(x, y), "{x} {y}");
    }
    assert!(terrarium::decode(b"not a png").is_err());
    // As a mosaic: a pixel's centre is its height, halfway between two pixels the mean, a missing
    // tile the open sea.
    let mosaic = Mosaic::new(4, [((0, 0), tile)].into_iter().collect());
    let at = |px: f64, py: f64| mosaic.sample(px / 16.0, py / 16.0);
    assert_eq!(at(3.5, 4.5), made_up_height(3, 4));
    assert_eq!(at(4.0, 4.5), (made_up_height(3, 4) + made_up_height(4, 4)) / 2.0);
    assert_eq!(mosaic.sample(5.5, 5.5), 0.0);
}

#[test]
fn vector_tiles_decode_their_layers() {
    let layers = mvt::decode(&read("tile.mvt"), &["water", "landcover", "transportation"]);
    assert_eq!(layers.len(), 3, "landuse was not asked for");
    let water = &layers["water"];
    assert_eq!((water.extent, water.features.len()), (4096.0, 2));
    let ocean = &water.features[0];
    assert_eq!(ocean.kind, Kind::Polygon);
    assert_eq!(ocean.properties.get("class"), Some(&Value::Text("ocean".into())));
    assert_eq!(ocean.parts, vec![vec![[-64.0, -64.0], [1024.0, -64.0], [1024.0, 4160.0], [-64.0, 4160.0]]]);
    let wood = &layers["landcover"].features[0];
    assert_eq!((wood.parts.len(), wood.text("subclass")), (2, Some("forest")));
    let roads = &layers["transportation"].features;
    assert_eq!(roads.iter().map(|f| f.kind).collect::<Vec<_>>(), vec![Kind::Line; 3]);
    assert_eq!(roads[1].text("brunnel"), Some("tunnel"));
    assert!(mvt::decode(b"\x1f\x8bnot gzip", &["water"]).is_empty());
}

#[test]
fn a_map_tile_becomes_cover_and_roads() {
    // The origin at the tile's middle.
    let (lat, lon) = tile_to_earth(TILE.x as f64 + 0.5, TILE.y as f64 + 0.5, TILE.z);
    let p = Projection::new(lat, lon);
    let map = read_map(&read("tile.mvt"), TILE, &p);
    // The primary road only: not the tunnel, not the path. One stretch, about 5.3 km of the 7.5 km tile.
    assert_eq!(map.roads.len(), 1);
    let r = map.roads[0];
    assert_eq!(r.width, 11.0);
    assert!(((r.n1 - r.n0).hypot(r.e1 - r.e0) - 7500.0 * 2900.0 / 4096.0).abs() < 300.0);
    assert_eq!(map.areas.len(), 4);

    let cover = cover_grid(&[map], (0.0, 0.0));
    let at = |x: f64, y: f64| {
        let (lat, lon) = tile_to_earth(TILE.x as f64 + x / 4096.0, TILE.y as f64 + y / 4096.0, TILE.z);
        let (n, e) = p.to_world(lat, lon);
        Cover::from_byte(cover.nearest(n, e).unwrap())
    };
    assert_eq!(at(500.0, 2000.0), Cover::Sea);
    assert_eq!(at(1800.0, 800.0), Cover::Forest);
    assert_eq!(at(2500.0, 1250.0), Cover::Unknown, "the clearing");
    assert_eq!(at(2000.0, 3000.0), Cover::Built);
    assert_eq!(at(3800.0, 400.0), Cover::Water);
    // Beyond the tile, nothing.
    let (n, e) = { let (lat, lon) = tile_to_earth(TILE.x as f64 - 0.5, TILE.y as f64, TILE.z); p.to_world(lat, lon) };
    assert_eq!(cover.nearest(n, e), Some(Cover::Unknown as u8));
    assert_eq!(tile_xy(lat, lon, TILE.z), (TILE.x as f64 + 0.5, TILE.y as f64 + 0.5));
}

fn tables() -> Tables {
    let text = |name| String::from_utf8(read(name)).unwrap();
    Tables::parse(&text("airports.csv"), &text("runways.csv"))
}

#[test]
fn ourairports_tables_are_read() {
    let t = tables();
    // The heliport is left out, and the closed runways.
    assert_eq!(t.aerodromes.len(), 3);
    assert_eq!(t.runways.len(), 4);
    let lemh = t.find("lemh").unwrap();
    assert_eq!(t.find("MAH"), Some(lemh), "by its IATA code");
    assert_eq!((lemh.kind.as_str(), lemh.codes[0].as_str()), ("large_airport", "LEMH"));
    assert!((lemh.elevation.unwrap() - 302.0 * FEET).abs() < 1e-9);
    assert!(t.find("XXXX").is_none());
    let lowi: Vec<_> = t.runways_of(t.find("LOWI").unwrap().id).collect();
    assert_eq!(lowi.len(), 2);
    assert!(lowi[0].paved() && !lowi[1].paved());
    assert_eq!(lowi[1].low.elevation, None);

    let fields = ourairports::records("a,\"b, \"\"c\"\"\nd\",e\r\nf\n");
    assert_eq!(fields, vec![vec!["a".to_string(), "b, \"c\"\nd".into(), "e".into()], vec!["f".to_string()]]);
}

#[test]
fn runways_are_placed_from_their_ends() {
    let t = tables();
    let lemh = t.find("LEMH").unwrap();
    let p = Projection::new(lemh.latitude, lemh.longitude);
    let runways: Vec<_> = t.runways_of(lemh.id).collect();
    let placed = ourairports::place(lemh, &runways, &p);
    assert_eq!(placed.len(), 1);
    let a = placed[0].airport;
    assert!(placed[0].surveyed);
    // 8366 ft between its ends, 9.0° east of north as they lie (the table says 8°), halfway between
    // their heights, a category II.
    assert!((a.length - 8366.0 * FEET).abs() < 30.0, "{}", a.length);
    assert!((a.heading.to_degrees() - 8.97).abs() < 0.05, "{}", a.heading.to_degrees());
    assert_eq!(a.number(), 1);
    assert!((a.elevation - (278.0 + 303.0) / 2.0 * FEET).abs() < 1e-9);
    assert_eq!(a.detail.lighting, Some(Class::PrecisionTwo));
    // The terminal (its reference point) is west of the runway: the apron is on the left.
    assert_eq!(a.detail.apron, -1.0);

    let lowi = t.find("LOWI").unwrap();
    let runways: Vec<_> = t.runways_of(lowi.id).collect();
    let placed = ourairports::place(lowi, &runways, &Projection::new(lowi.latitude, lowi.longitude));
    let (main, grass) = (placed[0].airport, placed[1].airport);
    assert!(main.detail.paved && !grass.detail.paved);
    assert_eq!((main.detail.lighting, grass.detail.lighting), (Some(Class::PrecisionOne), Some(Class::Visual)));
    assert_eq!(grass.detail.apron, 0.0);
    // The grass runway is north of the main one: the apron is on the other side, the right of 08.
    assert!(main.local(grass.n, grass.e).1 < 0.0);
    assert_eq!(main.detail.apron, 1.0);
    assert!(!placed[1].surveyed || grass.elevation > 0.0);
}

#[test]
fn a_tile_gives_its_buildings_once_with_what_the_maps_say_of_them() {
    use crate::overture::read_buildings;
    use sim_core::{Roof, Use};
    let mut tile = Pb::default();
    let buildings = layer(
        "building",
        &[
            // A block of flats; a church with its roof; one in the margin, the next tile's; one drawn
            // by its parts; one of floors and no height.
            (Kind::Polygon, &[("id", "flats"), ("height", "24"), ("class", "apartments"), ("facade_color", "#d0c0a0")], vec![square(100, 100, 300, 200)]),
            (Kind::Polygon, &[("id", "church"), ("height", "18"), ("class", "church"), ("roof_shape", "gabled"), ("roof_height", "6")], vec![square(1000, 1000, 1040, 1020)]),
            (Kind::Polygon, &[("id", "margin"), ("height", "8")], vec![square(4090, 100, 4200, 150)]),
            (Kind::Polygon, &[("id", "tower"), ("height", "120"), ("has_parts", "true")], vec![square(2000, 2000, 2100, 2100)]),
            (Kind::Polygon, &[("id", "offices"), ("num_floors", "5"), ("subtype", "commercial")], vec![square(3000, 3000, 3050, 3060)]),
        ],
    );
    let parts = layer("building_part", &[(Kind::Polygon, &[("id", "top"), ("building_id", "tower"), ("height", "120"), ("min_height", "40")], vec![square(2020, 2020, 2080, 2080)])]);
    tile.bytes(3, &buildings);
    tile.bytes(3, &parts);
    let projection = Projection::new(39.86, 4.22);
    let found = read_buildings(&tile.0, TILE, &projection);
    let named = |id: &str| found.iter().find(|r| r.id == id);
    assert_eq!(found.len(), 5, "{found:?}");
    assert!(named("margin").is_none());
    let (block, church, offices) = (&named("flats").unwrap().footprint, &named("church").unwrap().footprint, &named("offices").unwrap().footprint);
    assert_eq!((block.height, block.floor, block.roof, block.ring.len(), block.kind), (24.0, 0.0, Roof::Flat, 4, Use::Residential));
    assert_eq!(block.wall_colour, Some([0xd0, 0xc0, 0xa0]));
    assert_eq!((church.roof, church.roof_height, church.kind), (Roof::Gabled, 6.0, Use::Religious));
    assert!(named("tower").unwrap().has_parts);
    let top = named("top").unwrap();
    assert_eq!((top.part_of.as_deref(), top.footprint.floor, top.footprint.height), (Some("tower"), 40.0, 120.0));
    // Floors make a height, at an office's storey.
    assert!(offices.measured && (offices.height - 5.0 * 3.8).abs() < 1e-4 && offices.levels == 5);
    // 200 x 100 units of a tile of zoom 12 (7.5 km across at this latitude) is some 366 by 183 m.
    let side = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    assert!((side(block.ring[0], block.ring[1]) - 366.0).abs() < 5.0, "{:?}", block.ring);
    assert!((block.area() / (side(block.ring[0], block.ring[1]) as f64 * side(block.ring[1], block.ring[2]) as f64) - 1.0).abs() < 0.01);
}
