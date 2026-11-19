//! A vector tile of OpenFreeMap made into what the picker's map draws: its areas as one mesh of
//! coloured triangles (land is the map's background; woods, fields, towns, aerodromes and water on
//! it), its lines (rivers, borders, roads) and the names of its places, all in the tile's units (0
//! to 1 east and south from its north-west corner), so that each frame only moves and scales them.
//! The style is flat and dark, as the rest of the game's screens: which layers and classes of
//! OpenMapTiles are drawn, from which zoom, in which colour, is all in this file.

use engine::egui::{epaint::Vertex, pos2, Color32, Mesh};
use geodata::mvt::{self, Feature, Kind};

/// The layers read.
const LAYERS: [&str; 8] = ["water", "landcover", "landuse", "aeroway", "waterway", "boundary", "transportation", "place"];

/// The colour of the land, under everything.
pub const LAND: Color32 = Color32::from_rgb(38, 42, 46);
pub const WATER: Color32 = Color32::from_rgb(24, 42, 66);

/// What a line is, which says how it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LineKind {
    River,
    /// A region's border inside a country (OpenMapTiles' admin level 3 or 4).
    Region,
    Minor,
    Secondary,
    Primary,
    Trunk,
    Motorway,
    /// A country's border on land.
    Country,
}

impl LineKind {
    /// Colour and width in points of a line of a tile of zoom `z` (the width grows as the tile is
    /// drawn bigger). Roads are thin and dim far out, where there are many of them, and full nearer.
    pub fn stroke(self, z: u8) -> (Color32, f32) {
        let near = ((z as f32 - 6.0) / 6.0).clamp(0.0, 1.0);
        let road = |(r, g, b): (u8, u8, u8), width: f32| (Color32::from_rgb(r, g, b).gamma_multiply(0.6 + 0.4 * near), width * (0.5 + 0.5 * near));
        match self {
            LineKind::River => (Color32::from_rgb(34, 60, 92), 1.0),
            LineKind::Region => (Color32::from_rgb(86, 82, 100), 0.8),
            LineKind::Minor => road((70, 72, 76), 0.8),
            LineKind::Secondary => road((96, 94, 88), 1.0),
            LineKind::Primary => road((132, 120, 96), 1.2),
            LineKind::Trunk => road((168, 128, 80), 1.4),
            LineKind::Motorway => road((204, 140, 72), 1.7),
            LineKind::Country => (Color32::from_rgb(160, 140, 184), 1.3),
        }
    }
}

/// What a name is, which says how big it is written and which go first when they would overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LabelKind {
    Country,
    /// A city, by OpenMapTiles' rank (1 the most important).
    City(u8),
    Town,
    Village,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub kind: LabelKind,
    pub text: String,
    pub at: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub kind: LineKind,
    pub points: Vec<[f32; 2]>,
}

/// A tile ready to be drawn.
#[derive(Clone, Debug, Default)]
pub struct Shapes {
    /// The areas, in the tile's units.
    pub mesh: Mesh,
    /// The lines, the most important last (drawn on top).
    pub lines: Vec<Line>,
    pub labels: Vec<Label>,
}

impl Shapes {
    /// Bytes it holds, for the memory kept.
    pub fn bytes(&self) -> usize {
        self.mesh.vertices.len() * std::mem::size_of::<Vertex>() + self.mesh.indices.len() * 4 + self.lines.iter().map(|l| l.points.len() * 8 + 32).sum::<usize>() + self.labels.len() * 64
    }
}

/// The colour an area is filled with at this zoom, or `None` when it is not drawn.
fn fill(layer: &str, f: &Feature, z: u8) -> Option<Color32> {
    let class = f.text("class").unwrap_or("");
    let rgb = |r, g, b| Some(Color32::from_rgb(r, g, b));
    match layer {
        "water" if f.text("brunnel") == Some("tunnel") || class == "swimming_pool" => None,
        "water" => Some(WATER),
        "landcover" => match class {
            "wood" | "forest" => rgb(36, 52, 42),
            "grass" | "farmland" => rgb(42, 48, 42),
            "ice" | "glacier" => rgb(64, 74, 86),
            "sand" => rgb(62, 58, 46),
            "rock" => rgb(54, 54, 58),
            "wetland" => rgb(36, 50, 52),
            _ => None,
        },
        "landuse" if z >= 8 => match class {
            "residential" | "suburb" | "neighbourhood" | "quarter" => rgb(54, 52, 52),
            "commercial" | "industrial" | "retail" | "railway" => rgb(58, 54, 58),
            _ => None,
        },
        "aeroway" => match class {
            "aerodrome" => rgb(56, 58, 70),
            "runway" => rgb(150, 152, 160),
            "taxiway" | "apron" => rgb(88, 90, 100),
            _ => None,
        },
        _ => None,
    }
}

/// What kind of line a feature is at this zoom, or `None` when it is not drawn.
fn line(layer: &str, f: &Feature, z: u8) -> Option<LineKind> {
    let class = f.text("class").unwrap_or("");
    let number = |key: &str| f.properties.get(key).and_then(mvt::Value::number);
    match layer {
        "waterway" if z >= 7 && matches!(class, "river" | "canal") && f.text("brunnel") != Some("tunnel") => Some(LineKind::River),
        "boundary" if number("maritime") == Some(1.0) || number("disputed") == Some(1.0) => None,
        "boundary" => match number("admin_level").map(|l| l as i32) {
            Some(2) => Some(LineKind::Country),
            Some(3 | 4) if z >= 4 => Some(LineKind::Region),
            _ => None,
        },
        "transportation" if f.text("brunnel") == Some("tunnel") => None,
        "transportation" => match class {
            "motorway" => Some(LineKind::Motorway),
            "trunk" => Some(LineKind::Trunk),
            "primary" if z >= 7 => Some(LineKind::Primary),
            "secondary" if z >= 9 => Some(LineKind::Secondary),
            "tertiary" | "minor" if z >= 12 => Some(LineKind::Minor),
            _ => None,
        },
        _ => None,
    }
}

/// What kind of name a place is at this zoom, or `None` when it is not written.
fn label(f: &Feature, z: u8) -> Option<LabelKind> {
    let rank = f.properties.get("rank").and_then(mvt::Value::number).unwrap_or(10.0) as u8;
    match f.text("class").unwrap_or("") {
        "country" => Some(LabelKind::Country),
        "city" if z >= 3 && rank <= z.saturating_mul(2).max(2) => Some(LabelKind::City(rank)),
        "town" if z >= 8 => Some(LabelKind::Town),
        "village" if z >= 11 => Some(LabelKind::Village),
        _ => None,
    }
}

/// The name of a place in English if the map has it, else in Latin letters, or its own when the
/// overlay's font has its letters (Greek, Cyrillic); `None` otherwise (Devanagari or Chinese would
/// be boxes).
fn name(f: &Feature) -> Option<&str> {
    ["name:en", "name_en", "name:latin", "name"].iter().filter_map(|k| f.text(k)).find(|n| !n.is_empty() && drawable(n))
}

/// Whether the overlay's font has every letter of a text: Latin, Greek and Cyrillic.
fn drawable(text: &str) -> bool {
    text.chars().all(|c| matches!(c as u32, 0..=0x052F | 0x1E00..=0x1FFF | 0x2000..=0x206F))
}

/// Reads a tile of zoom `z` (gzipped or not) into what is drawn of it.
pub fn shapes(bytes: &[u8], z: u8) -> Shapes {
    let layers = mvt::decode(bytes, &LAYERS);
    let mut out = Shapes::default();
    // Areas: the layers in the order they cover each other.
    for name in ["landcover", "landuse", "aeroway", "water"] {
        let Some(layer) = layers.get(name) else { continue };
        let scale = 1.0 / layer.extent;
        for f in layer.features.iter().filter(|f| f.kind == Kind::Polygon) {
            if let Some(colour) = fill(name, f, z) {
                add_polygons(&mut out.mesh, &f.parts, scale, colour);
            }
        }
    }
    // Lines.
    for name in ["waterway", "boundary", "transportation"] {
        let Some(layer) = layers.get(name) else { continue };
        let scale = 1.0 / layer.extent;
        for f in layer.features.iter().filter(|f| f.kind == Kind::Line) {
            if let Some(kind) = line(name, f, z) {
                for part in f.parts.iter().filter(|p| p.len() >= 2) {
                    out.lines.push(Line { kind, points: part.iter().map(|p| [p[0] * scale, p[1] * scale]).collect() });
                }
            }
        }
    }
    out.lines.sort_by_key(|l| l.kind);
    // Names of places, only those in the tile itself (not its border), so each is written once.
    if let Some(layer) = layers.get("place") {
        let scale = 1.0 / layer.extent;
        for f in layer.features.iter().filter(|f| f.kind == Kind::Point) {
            let (Some(kind), Some(text), Some(p)) = (label(f, z), name(f), f.parts.first().and_then(|p| p.first())) else { continue };
            let at = [p[0] * scale, p[1] * scale];
            if (0.0..1.0).contains(&at[0]) && (0.0..1.0).contains(&at[1]) {
                out.labels.push(Label { kind, text: text.to_string(), at });
            }
        }
        out.labels.sort_by_key(|l| l.kind);
    }
    out
}

/// Adds a feature's polygons (each an outer ring and the holes after it, as vector tiles write
/// them: outer rings with a positive area, holes negative) to `mesh` as triangles.
fn add_polygons(mesh: &mut Mesh, rings: &[Vec<[f32; 2]>], scale: f32, colour: Color32) {
    let area = |ring: &[[f32; 2]]| ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64).sum::<f64>();
    let mut start = 0;
    while start < rings.len() {
        let mut end = start + 1;
        while end < rings.len() && area(&rings[end]) < 0.0 {
            end += 1;
        }
        if area(&rings[start]) > 0.0 {
            add_polygon(mesh, &rings[start..end], scale, colour);
        }
        start = end;
    }
}

fn add_polygon(mesh: &mut Mesh, rings: &[Vec<[f32; 2]>], scale: f32, colour: Color32) {
    let mut coords: Vec<f64> = Vec::new();
    let mut holes = Vec::new();
    for (k, ring) in rings.iter().enumerate() {
        if ring.len() < 3 {
            continue;
        }
        if k > 0 {
            holes.push(coords.len() / 2);
        }
        coords.extend(ring.iter().flat_map(|p| [p[0] as f64, p[1] as f64]));
    }
    let Ok(triangles) = earcutr::earcut(&coords, &holes, 2) else { return };
    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend(coords.as_chunks::<2>().0.iter().map(|p| Vertex { pos: pos2(p[0] as f32 * scale, p[1] as f32 * scale), uv: engine::egui::epaint::WHITE_UV, color: colour }));
    mesh.indices.extend(triangles.into_iter().map(|i| base + i as u32));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tile of `geodata`'s fixtures: made up (no real data), in OpenMapTiles' layers: an
    /// ocean in the west, a wood with a clearing, a built-up area, a lake, a primary road, a tunnel
    /// and a path (see `geodata/src/fixtures.rs`).
    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../sim/geodata/tests/fixtures/tile.mvt")).unwrap()
    }

    /// Whether a point is covered by a triangle of the mesh, and in which colour.
    fn colour_at(mesh: &Mesh, x: f32, y: f32) -> Option<Color32> {
        let mut found = None;
        for t in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[t[k] as usize].pos);
            let side = |p: engine::egui::Pos2, q: engine::egui::Pos2| (q.x - p.x) * (y - p.y) - (q.y - p.y) * (x - p.x);
            let (d0, d1, d2) = (side(a, b), side(b, c), side(c, a));
            if (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0) {
                found = Some(mesh.vertices[t[0] as usize].color);
            }
        }
        found
    }

    #[test]
    fn a_tile_becomes_areas_lines_and_names_in_its_units() {
        let tile = shapes(&fixture(), 12);
        assert!(!tile.mesh.indices.is_empty() && tile.mesh.indices.len().is_multiple_of(3));
        assert!(tile.mesh.vertices.iter().all(|v| (-0.1..=1.1).contains(&v.pos.x) && (-0.1..=1.1).contains(&v.pos.y)), "in the tile's units, with its border");
        // The ocean in the west is water; the wood is wood, but for its clearing (a hole).
        assert_eq!(colour_at(&tile.mesh, 0.05, 0.5), Some(WATER));
        let wood = fill("landcover", &Feature { kind: Kind::Polygon, parts: Vec::new(), properties: [("class".to_string(), mvt::Value::Text("wood".into()))].into() }, 12);
        assert!(tile.mesh.vertices.iter().any(|v| Some(v.color) == wood));
        // The primary road is a line, the tunnel and the path are not.
        assert_eq!(tile.lines.iter().filter(|l| l.kind == LineKind::Primary).count(), 1, "{:?}", tile.lines.iter().map(|l| l.kind).collect::<Vec<_>>());
        assert!(tile.lines.iter().all(|l| l.kind == LineKind::Primary));
        // Far out the same tile draws no roads of that class.
        assert!(shapes(&fixture(), 5).lines.is_empty());
    }

    #[test]
    fn names_in_english_else_in_letters_the_font_has() {
        let place = |names: &[(&str, &str)]| Feature { kind: Kind::Point, parts: Vec::new(), properties: names.iter().map(|(k, v)| (k.to_string(), mvt::Value::Text(v.to_string()))).collect() };
        assert_eq!(name(&place(&[("name", "北京"), ("name:latin", "Běijīng"), ("name:es", "Pekín"), ("name:en", "Beijing")])), Some("Beijing"));
        assert_eq!(name(&place(&[("name", "München"), ("name_en", "Munich"), ("name:latin", "München")])), Some("Munich"));
        assert_eq!(name(&place(&[("name", "नाम्चे बजार"), ("name:latin", "Namche Bazaar")])), Some("Namche Bazaar"));
        assert_eq!(name(&place(&[("name", "Αθήνα")])), Some("Αθήνα"));
        assert_eq!(name(&place(&[("name", "नाम्चे बजार")])), None);
    }

    #[test]
    fn rings_become_triangles_with_their_holes_left_out() {
        // A square of 4 by 4 with a hole of 2 by 2 in the middle (holes go the other way round).
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let hole = vec![[1.0, 1.0], [1.0, 3.0], [3.0, 3.0], [3.0, 1.0]];
        let mut mesh = Mesh::default();
        add_polygons(&mut mesh, &[outer.clone(), hole], 0.25, Color32::RED);
        assert_eq!(colour_at(&mesh, 0.1, 0.1), Some(Color32::RED));
        assert_eq!(colour_at(&mesh, 0.5, 0.5), None, "the hole");
        let area: f32 = mesh.indices.as_chunks::<3>().0.iter().map(|t| {
            let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[t[k] as usize].pos);
            ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() / 2.0
        }).sum();
        assert!((area - 0.75).abs() < 1e-5, "{area}");
        // An outer ring the wrong way round is not an area.
        let mut none = Mesh::default();
        add_polygons(&mut none, &[outer.into_iter().rev().collect()], 1.0, Color32::RED);
        assert!(none.indices.is_empty());
    }
}
