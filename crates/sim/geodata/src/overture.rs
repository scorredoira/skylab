//! Overture Maps: the buildings of the whole world and OpenStreetMap's bridges with all their tags.
//!
//! Overture publishes each release of its data as PMTiles files on a public server
//! (`tiles.overturemaps.org/<release>/<theme>.pmtiles`, no key), named by its STAC catalogue
//! (`stac.overturemaps.org`). Two are read here, a tile at a time by HTTP range requests
//! (`pmtiles`):
//!
//! - `buildings`, zoom 14: OpenStreetMap's buildings, and where it has none those Microsoft, Google
//!   and Esri found in aerial pictures (with heights where they measured them), each with its
//!   height, floors, roof shape and height, use, colours and materials as the maps give them;
//!   and the parts buildings are drawn by (`building_part`: a tower on its podium, a spire).
//! - `base`, zoom 13, its layer `infrastructure`: OpenStreetMap's bridges (the ways tagged
//!   `bridge=*`, with every tag they have: what they carry, `layer`, `bridge:structure`, `width`,
//!   `colour`, `wikidata`).
//!
//! A release is kept on the server for some two months; the catalogue is asked for the latest only
//! when a tile is missing from the cache, and the tiles are kept under names without the release
//! (`overture/<theme>/<z>/<x>/<y>.mvt`, as they came: gzipped vector tiles), so a place built
//! once is read again offline whatever the release is by then. A tile the file does not have
//! (nothing there) is kept as one zero byte.

use std::collections::HashMap;
use std::sync::OnceLock;

use sim_core::{Carries, Footprint, Material, Roof, Structure, Use};

use crate::fetch::Fetcher;
use crate::geo::{tile_to_earth, Projection, TileId};
use crate::json::Json;
use crate::mvt::{self, Feature, Kind, Value};
use crate::pmtiles::Archive;
use crate::{disk, Error};

/// The catalogue of Overture's releases.
pub const CATALOGUE: &str = "https://stac.overturemaps.org/catalog.json";
/// The zoom of the buildings' tiles (the file's deepest) and of the bridges' (`base`'s deepest).
pub const BUILDINGS_ZOOM: u8 = 14;
pub const BRIDGES_ZOOM: u8 = 13;

/// The PMTiles files of a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub name: String,
    pub buildings: String,
    pub base: String,
}

/// The latest release, from the catalogue.
pub fn latest(fetcher: &Fetcher) -> Result<Release, Error> {
    let read = |url: &str| -> Result<Json, Error> {
        let bytes = fetcher.get(url)?.ok_or_else(|| Error::Http(url.to_string()))?;
        Json::parse(&String::from_utf8_lossy(&bytes)).ok_or_else(|| Error::Data(format!("Overture's catalogue {url}")))
    };
    let root = read(CATALOGUE)?;
    let name = root.get("latest").and_then(Json::text).ok_or_else(|| Error::Data("Overture's catalogue: no latest release".into()))?.to_string();
    let theme = |theme: &str| -> Result<String, Error> {
        let catalogue = read(&format!("https://stac.overturemaps.org/{name}/{theme}/catalog.json"))?;
        catalogue
            .get("links")
            .map(Json::list)
            .unwrap_or_default()
            .iter()
            .find(|l| l.get("rel").and_then(Json::text) == Some("pmtiles"))
            .and_then(|l| l.get("href").and_then(Json::text))
            .map(str::to_string)
            .ok_or_else(|| Error::Data(format!("Overture's catalogue: no PMTiles for {theme}")))
    };
    Ok(Release { buildings: theme("buildings")?, base: theme("base")?, name })
}

/// Reads tiles of one of a release's files, through the cache: the release and the file's head are
/// only asked for when a tile is not kept.
pub struct Tiles<'a> {
    fetcher: &'a Fetcher,
    theme: &'static str,
    release: &'a OnceLock<Result<Release, Error>>,
    archive: OnceLock<Result<Archive<'a>, Error>>,
}

impl<'a> Tiles<'a> {
    pub fn new(fetcher: &'a Fetcher, theme: &'static str, release: &'a OnceLock<Result<Release, Error>>) -> Tiles<'a> {
        Tiles { fetcher, theme, release, archive: OnceLock::new() }
    }

    /// Where a tile is kept.
    pub fn key(&self, id: TileId) -> String {
        format!("overture/{}/{}/{}/{}.mvt", self.theme, id.z, id.x, id.y)
    }

    /// A tile's bytes as stored (gzipped), from the cache or the server; empty where the file has
    /// nothing.
    pub fn get(&self, id: TileId) -> Result<Vec<u8>, Error> {
        let key = self.key(id);
        let path = self.fetcher.root().join(&key);
        if disk::exists(&path) {
            return disk::read(&path).map_err(|e| Error::Io(format!("{}: {e}", path.display())));
        }
        let archive = self
            .archive
            .get_or_init(|| {
                let release = self.release.get_or_init(|| latest(self.fetcher)).clone()?;
                let url = if self.theme == "buildings" { release.buildings } else { release.base };
                Archive::open(self.fetcher, &url, &format!("overture/{}/{}", release.name, self.theme))
            })
            .as_ref()
            .map_err(Clone::clone)?;
        match archive.tile(id, &key)? {
            Some(bytes) => Ok(bytes),
            None => {
                disk::write(&path, &[0]).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
                Ok(Vec::new())
            }
        }
    }
}

/// A building or a part of one as read from a tile, before its height is settled.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub footprint: Footprint,
    /// Overture's id of the building, and whether it is drawn by its parts.
    pub id: String,
    pub has_parts: bool,
    /// For a part, its building's id.
    pub part_of: Option<String>,
    /// Whether the maps say what its roof is like (a shape, or a material of a pitched one).
    pub roof_said: bool,
}

/// The buildings and parts of a tile whose middle is in it, in the world's axes.
pub fn read_buildings(bytes: &[u8], id: TileId, projection: &Projection) -> Vec<Read> {
    let layers = mvt::decode(bytes, &["building", "building_part"]);
    let mut out = Vec::new();
    for (name, layer) in &layers {
        let part = name == "building_part";
        let to_world = |p: [f32; 2]| {
            let (lat, lon) = tile_to_earth(id.x as f64 + p[0] as f64 / layer.extent as f64, id.y as f64 + p[1] as f64 / layer.extent as f64, id.z);
            let (n, e) = projection.to_world(lat, lon);
            [n as f32, e as f32]
        };
        for f in layer.features.iter().filter(|f| f.kind == Kind::Polygon) {
            if flag(f, "is_underground") {
                continue;
            }
            for (outer, holes) in polygons(f) {
                let k = outer.len().max(1) as f32;
                let middle = outer.iter().fold([0.0, 0.0], |m, p| [m[0] + p[0] / k, m[1] + p[1] / k]);
                if middle[0] < 0.0 || middle[1] < 0.0 || middle[0] >= layer.extent || middle[1] >= layer.extent {
                    continue;
                }
                let ring = crate::build::simplify(outer.iter().map(|&p| to_world(p)).collect());
                if ring.len() < 3 {
                    continue;
                }
                let holes: Vec<Vec<[f32; 2]>> = holes.iter().map(|h| crate::build::simplify(h.iter().map(|&p| to_world(p)).collect())).filter(|h| h.len() >= 3).collect();
                let footprint = Footprint { holes, ..describe(f, ring) };
                out.push(Read {
                    footprint,
                    id: f.text("id").unwrap_or("").to_string(),
                    has_parts: flag(f, "has_parts"),
                    part_of: part.then(|| f.text("building_id").unwrap_or("").to_string()),
                    roof_said: f.text("roof_shape").is_some() || f.text("roof_material").is_some(),
                });
            }
        }
    }
    out
}

/// Whether a property is true (a boolean, or the text "true").
fn flag(f: &Feature, key: &str) -> bool {
    match f.properties.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(v) => v.text() == Some("true"),
        None => false,
    }
}

/// The polygons of a feature: each outer ring with the holes after it (they turn the other way).
fn polygons(f: &Feature) -> Vec<(&Vec<[f32; 2]>, Vec<&Vec<[f32; 2]>>)> {
    let signed = |ring: &[[f32; 2]]| ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64).sum::<f64>();
    let Some(first) = f.parts.first().map(|r| signed(r)) else { return Vec::new() };
    let mut out: Vec<(&Vec<[f32; 2]>, Vec<&Vec<[f32; 2]>>)> = Vec::new();
    for ring in &f.parts {
        if signed(ring) * first > 0.0 {
            out.push((ring, Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push(ring);
        }
    }
    out
}

/// A storey's height for a use, metres: what floors are turned into heights with.
pub fn storey(kind: Use) -> f32 {
    match kind {
        Use::Office | Use::Retail => 3.8,
        Use::Industrial | Use::Outbuilding => 4.5,
        Use::Civic | Use::Religious => 4.5,
        _ => 3.1,
    }
}

/// What the tags of a building say of it (its height left at 0 when they say nothing of it).
fn describe(f: &Feature, ring: Vec<[f32; 2]>) -> Footprint {
    let number = |key: &str| f.properties.get(key).and_then(Value::number).filter(|v| v.is_finite());
    let kind = use_of(f.text("class"), f.text("subtype"));
    let floors = number("num_floors").filter(|&v| v >= 1.0).map(|v| v.min(250.0) as u8).unwrap_or(0);
    let mut roof = f.text("roof_shape").map(roof_of).unwrap_or(Roof::Flat);
    let roof_height = number("roof_height").unwrap_or(0.0).clamp(0.0, 400.0) as f32;
    let storey = storey(kind);
    let (height, measured) = match number("height").filter(|&h| h > 0.5) {
        Some(h) => (h.min(900.0) as f32, true),
        None if floors > 0 => (floors as f32 * storey + if roof.pitched() { roof_height.max(storey * 0.7) } else { 0.0 }, true),
        None => (0.0, false),
    };
    let floor = match number("min_height") {
        Some(h) => h.max(0.0) as f32,
        None => number("min_floor").map_or(0.0, |k| (k.max(0.0) as f32) * storey),
    };
    let mut direction = number("roof_direction").map(|d| (d as f32).to_radians());
    if roof == Roof::Skillion {
        // The tag is the way the slope faces; the ridge (its high edge) runs across that.
        direction = direction.map(|d| d + std::f32::consts::FRAC_PI_2);
    }
    let mut footprint = Footprint { ring, height, floor, roof, roof_height, direction, levels: floors, kind, measured, ..Footprint::default() };
    if f.text("roof_orientation") == Some("across") && direction.is_none() {
        let (_, heading, _, _) = footprint.oriented_box();
        footprint.direction = Some(heading as f32 + std::f32::consts::FRAC_PI_2);
    }
    footprint.material = f.text("facade_material").map(material_of).unwrap_or_default();
    footprint.wall_colour = f.text("facade_color").and_then(colour);
    footprint.roof_colour = f.text("roof_color").and_then(colour).or_else(|| f.text("roof_material").and_then(roof_material_colour));
    if roof == Roof::Flat && footprint.roof_colour.is_none() && f.text("roof_material") == Some("roof_tiles") {
        // Tiles are on a pitched roof, whatever the shape says.
        roof = Roof::Hipped;
        footprint.roof = roof;
    }
    footprint
}

/// What a building is for, from Overture's `class` (OpenStreetMap's `building=*`) and `subtype`.
pub fn use_of(class: Option<&str>, subtype: Option<&str>) -> Use {
    let by_class = match class.unwrap_or("") {
        "house" | "detached" | "semidetached_house" | "terrace" | "bungalow" | "farm" | "cabin" | "villa" | "static_caravan" | "houseboat" => Some(Use::House),
        "apartments" | "residential" | "dormitory" => Some(Use::Residential),
        "office" | "commercial" => Some(Use::Office),
        "retail" | "supermarket" | "kiosk" => Some(Use::Retail),
        "industrial" | "warehouse" | "factory" | "manufacture" | "hangar" | "storage_tank" | "digester" => Some(Use::Industrial),
        "church" | "cathedral" | "chapel" | "mosque" | "temple" | "synagogue" | "shrine" | "religious" | "monastery" | "presbytery" => Some(Use::Religious),
        "civic" | "public" | "government" | "school" | "university" | "college" | "hospital" | "train_station" | "transportation" | "museum" | "kindergarten" | "fire_station" | "stadium" | "sports_hall" | "grandstand" | "hotel" => Some(Use::Civic),
        "garage" | "garages" | "shed" | "barn" | "carport" | "hut" | "greenhouse" | "farm_auxiliary" | "roof" | "cowshed" | "stable" | "sty" | "allotment_house" | "boathouse" | "parking" => Some(Use::Outbuilding),
        _ => None,
    };
    by_class.unwrap_or(match subtype.unwrap_or("") {
        "residential" => Use::Residential,
        "commercial" => Use::Office,
        "industrial" => Use::Industrial,
        "religious" => Use::Religious,
        "civic" | "education" | "medical" | "transportation" | "entertainment" | "military" | "service" => Use::Civic,
        "agricultural" | "outbuilding" => Use::Outbuilding,
        _ => Use::Unknown,
    })
}

/// A roof's shape from its tag (`roof:shape`'s values; the rarer ones as the nearest of ours).
pub fn roof_of(shape: &str) -> Roof {
    match shape {
        "gabled" | "saltbox" | "side_half-hipped" | "double_saltbox" | "quadruple_saltbox" => Roof::Gabled,
        "hipped" | "half-hipped" | "side_hipped" | "hipped-and-gabled" | "crosspitched" | "many" => Roof::Hipped,
        "pyramidal" | "cone" | "zakomar" => Roof::Pyramidal,
        "skillion" | "lean_to" | "sawtooth" | "butterfly" => Roof::Skillion,
        "dome" => Roof::Dome,
        "onion" => Roof::Onion,
        "round" | "half_round" | "barrel" => Roof::Round,
        "gambrel" => Roof::Gambrel,
        "mansard" => Roof::Mansard,
        _ => Roof::Flat,
    }
}

fn material_of(m: &str) -> Material {
    match m {
        "glass" => Material::Glass,
        "brick" | "clay" => Material::Brick,
        "stone" | "sandstone" | "limestone" | "granite" | "marble" => Material::Stone,
        "concrete" | "cement_block" => Material::Concrete,
        "metal" | "steel" | "aluminium" | "metal_plates" => Material::Metal,
        "plaster" | "render" => Material::Plaster,
        "wood" | "timber_framing" | "log" => Material::Wood,
        _ => Material::Unknown,
    }
}

/// A colour as the maps write it: `#rrggbb`, `#rgb` or one of the common names.
pub fn colour(text: &str) -> Option<[u8; 3]> {
    let t = text.trim().to_ascii_lowercase();
    let hex = |s: &str| u8::from_str_radix(s, 16).ok();
    if let Some(h) = t.strip_prefix('#') {
        return match h.len() {
            6 => Some([hex(&h[0..2])?, hex(&h[2..4])?, hex(&h[4..6])?]),
            3 => Some([hex(&h[0..1])? * 17, hex(&h[1..2])? * 17, hex(&h[2..3])? * 17]),
            _ => None,
        };
    }
    Some(match t.as_str() {
        "white" => [240, 240, 236],
        "black" => [30, 30, 32],
        "grey" | "gray" => [128, 128, 128],
        "lightgrey" | "lightgray" | "silver" => [192, 192, 192],
        "darkgrey" | "darkgray" => [80, 80, 82],
        "red" => [170, 50, 40],
        "brown" => [120, 80, 50],
        "maroon" => [110, 40, 35],
        "orange" => [220, 120, 50],
        "yellow" => [225, 200, 90],
        "beige" | "tan" => [215, 200, 165],
        "cream" | "ivory" => [235, 228, 200],
        "green" => [80, 120, 70],
        "blue" => [70, 100, 150],
        "pink" => [225, 170, 170],
        _ => return None,
    })
}

/// The colour of a roof of a material, when its colour is not given.
fn roof_material_colour(m: &str) -> Option<[u8; 3]> {
    Some(match m {
        "roof_tiles" | "tile" | "clay" => [168, 86, 58],
        "slate" => [72, 74, 80],
        "metal" | "tin" | "steel" | "zinc" | "aluminium" => [122, 126, 132],
        "copper" => [96, 150, 124],
        "concrete" => [150, 148, 140],
        "glass" => [92, 110, 128],
        "grass" | "plants" => [82, 108, 60],
        "tar_paper" | "asphalt" | "asphalt_shingle" | "bitumen" => [62, 62, 64],
        "stone" => [132, 126, 116],
        "thatch" => [140, 118, 78],
        "wood" => [110, 84, 60],
        _ => return None,
    })
}

/// A piece of a bridge as one tile has it: the ways are cut at the tiles' edges, and joined again
/// by their id (`join`).
#[derive(Clone, Debug, PartialEq)]
pub struct BridgePiece {
    pub id: String,
    pub points: Vec<[f64; 2]>,
    pub carries: Carries,
    /// How wide the way is, metres, as tagged or as its kind of road makes it.
    pub width: f64,
    pub layer: i32,
    pub structure: Structure,
    pub colour: Option<[u8; 3]>,
    pub wikidata: Option<String>,
    pub name: Option<String>,
}

/// The bridges of a tile of `base` (its layer `infrastructure`): the ways carrying a road, a
/// railway or a path, in the world's axes.
pub fn read_bridges(bytes: &[u8], id: TileId, projection: &Projection) -> Vec<BridgePiece> {
    let layers = mvt::decode(bytes, &["infrastructure"]);
    let Some(layer) = layers.get("infrastructure") else { return Vec::new() };
    let mut out = Vec::new();
    for f in layer.features.iter().filter(|f| f.kind == Kind::Line && f.text("subtype") == Some("bridge")) {
        if matches!(f.text("class"), Some("bridge_support" | "aqueduct")) {
            continue;
        }
        let tags = f.text("source_tags").and_then(Json::parse).unwrap_or(Json::Null);
        let tag = |k: &str| tags.get(k).and_then(Json::text);
        let (carries, width) = if let Some(rail) = tag("railway") {
            let tracks = tag("tracks").and_then(|t| t.parse::<f64>().ok()).unwrap_or(1.0).clamp(1.0, 8.0);
            match rail {
                "rail" | "light_rail" | "subway" | "narrow_gauge" | "monorail" | "tram" | "funicular" | "preserved" => (Carries::Rail, 2.0 + 4.5 * tracks),
                _ => continue,
            }
        } else if let Some(highway) = tag("highway") {
            let lanes = tag("lanes").and_then(|t| t.parse::<f64>().ok()).filter(|l| *l >= 1.0);
            let (carries, by_class) = match highway {
                "motorway" | "trunk" => (Carries::Road, 13.0),
                "primary" => (Carries::Road, 11.0),
                "secondary" => (Carries::Road, 9.5),
                "tertiary" | "motorway_link" | "trunk_link" | "primary_link" => (Carries::Road, 8.0),
                "residential" | "unclassified" | "secondary_link" | "tertiary_link" | "living_street" | "road" => (Carries::Road, 7.0),
                "service" | "track" => (Carries::Road, 5.0),
                "footway" | "cycleway" | "path" | "pedestrian" | "bridleway" | "steps" => (Carries::Path, 3.5),
                _ => continue,
            };
            (carries, lanes.map_or(by_class, |l| l * 3.5 + 1.5).max(if carries == Carries::Path { 2.0 } else { 5.0 }))
        } else {
            continue;
        };
        let width = tag("width").and_then(|t| t.trim_end_matches(" m").trim().parse::<f64>().ok()).filter(|w| *w >= 1.0 && *w < 80.0).unwrap_or(width);
        let layer_tag = tag("layer").and_then(|t| t.parse::<i32>().ok()).or_else(|| f.properties.get("level").and_then(Value::number).map(|v| v as i32)).unwrap_or(1);
        let structure = match tag("bridge:structure").unwrap_or("") {
            "suspension" => Structure::Suspension,
            "cable-stayed" | "cable_stayed" => Structure::CableStayed,
            "arch" | "tied-arch" | "tied_arch" => Structure::Arch,
            "truss" => Structure::Truss,
            _ => Structure::Beam,
        };
        let to_world = |p: [f32; 2]| {
            let (lat, lon) = tile_to_earth(id.x as f64 + p[0] as f64 / layer.extent as f64, id.y as f64 + p[1] as f64 / layer.extent as f64, id.z);
            let (n, e) = projection.to_world(lat, lon);
            [n, e]
        };
        for part in &f.parts {
            out.push(BridgePiece {
                id: f.text("id").unwrap_or("").to_string(),
                points: part.iter().map(|&p| to_world(p)).collect(),
                carries,
                width,
                layer: layer_tag,
                structure,
                colour: tag("colour").and_then(colour),
                wikidata: f.text("wikidata").or_else(|| tag("wikidata")).map(str::to_string),
                name: f.text("@name").or_else(|| tag("bridge:name")).or_else(|| tag("name")).map(str::to_string),
            });
        }
    }
    out
}

/// The pieces of each way joined into one line: the pieces the tiles cut a way into overlap in
/// their margins, so their points are put in order along the line between the two farthest apart
/// and those nearer than a metre to the one before are dropped. (A way that bends back on itself,
/// a spiral ramp, is not kept.)
pub fn join(pieces: Vec<BridgePiece>) -> Vec<BridgePiece> {
    let mut by_id: HashMap<String, Vec<BridgePiece>> = HashMap::new();
    let mut order = Vec::new();
    for p in pieces {
        if !by_id.contains_key(&p.id) {
            order.push(p.id.clone());
        }
        by_id.entry(p.id.clone()).or_default().push(p);
    }
    let mut out = Vec::new();
    for id in order {
        let mut list = by_id.remove(&id).unwrap_or_default();
        let points: Vec<[f64; 2]> = list.iter().flat_map(|p| p.points.iter().copied()).collect();
        if points.len() < 2 {
            continue;
        }
        let (mut a, mut b, mut far) = (0, 0, -1.0);
        for i in 0..points.len() {
            for j in i + 1..points.len() {
                let d = (points[i][0] - points[j][0]).hypot(points[i][1] - points[j][1]);
                if d > far {
                    (a, b, far) = (i, j, d);
                }
            }
        }
        let (o, axis) = (points[a], [(points[b][0] - points[a][0]) / far.max(1e-9), (points[b][1] - points[a][1]) / far.max(1e-9)]);
        let mut sorted: Vec<(f64, [f64; 2])> = points.iter().map(|p| ((p[0] - o[0]) * axis[0] + (p[1] - o[1]) * axis[1], *p)).collect();
        sorted.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut line: Vec<[f64; 2]> = Vec::new();
        for (_, p) in sorted {
            if line.last().is_none_or(|q: &[f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) > 1.0) {
                line.push(p);
            }
        }
        // A line that turns back on itself is not a bridge this can draw.
        let length: f64 = line.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        if line.len() < 2 || length > far * 1.6 + 20.0 {
            continue;
        }
        let mut joined = list.swap_remove(0);
        joined.points = line;
        out.push(joined);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_say_what_a_building_is() {
        assert_eq!(use_of(Some("apartments"), Some("residential")), Use::Residential);
        assert_eq!(use_of(Some("cathedral"), None), Use::Religious);
        assert_eq!(use_of(None, Some("commercial")), Use::Office);
        assert_eq!(use_of(Some("shed"), Some("residential")), Use::Outbuilding);
        assert_eq!(use_of(None, None), Use::Unknown);
        assert_eq!(roof_of("half-hipped"), Roof::Hipped);
        assert_eq!(roof_of("onion"), Roof::Onion);
        assert_eq!(roof_of("flat"), Roof::Flat);
        assert_eq!(colour("#C0362C"), Some([192, 54, 44]));
        assert_eq!(colour("#fff"), Some([255, 255, 255]));
        assert_eq!(colour("white"), Some([240, 240, 236]));
        assert_eq!(colour("#12"), None);
    }

    #[test]
    fn the_pieces_of_a_way_cut_by_the_tiles_are_joined_in_order() {
        let piece = |id: &str, points: Vec<[f64; 2]>| BridgePiece { id: id.into(), points, carries: Carries::Road, width: 8.0, layer: 1, structure: Structure::Beam, colour: None, wikidata: None, name: None };
        // One way cut at east 100, each tile with a margin past the cut; another way on its own.
        let joined = join(vec![
            piece("a", vec![[0.0, 0.0], [5.0, 60.0], [8.0, 108.0]]),
            piece("b", vec![[50.0, 0.0], [50.0, 30.0]]),
            piece("a", vec![[7.5, 92.0], [8.0, 108.0], [10.0, 200.0]]),
        ]);
        assert_eq!(joined.len(), 2);
        let a = &joined[0];
        assert_eq!(a.id, "a");
        let east: Vec<f64> = a.points.iter().map(|p| p[1]).collect();
        assert_eq!(east, vec![0.0, 60.0, 92.0, 108.0, 200.0]);
    }
}
