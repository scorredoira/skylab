//! A cockpit read from files: a folder of `assets/aircraft/` (the one a type's `visual.cockpit`
//! names) with
//! - `cockpit.glb`: the cabin's mesh in body axes (x forward, y right, z down, metres from the
//!   centre of gravity), in named nodes (glTF, `engine::gltf`), a node's origin the point it moves
//!   about, and its windows' glass (`examples/panel_knobs.rs` makes the panel's knobs that turn
//!   nodes of their own);
//! - `cockpit.ron`: what moves each node with the controls, where the eye can be, the instruments
//!   and where they are painted, what is printed, and what the mouse takes and clicks
//!   ([`CockpitFile`]).
//!
//! A triangle's material says what it is made of by its name ([`Material`]'s, in lower case:
//! `leather`, `chrome`...; a suffix after a dot, as `leather.001`, is left out), and its gloss is
//! `1 − roughness`. Two names are pictures of the instruments' canvas, at the triangle's texture
//! coordinates (fractions of the canvas): `screen`, a display that shines by itself, and `dial`, a
//! dial lit as any surface. A material of another name is plastic, or metal or a lamp by its PBR
//! values; one with a base colour texture is drawn with it, per pixel, and with its normal map
//! (`Material::Textured`: a model made in a modelling program, with its pictures; all of them of
//! one size, and the normal maps of one size). The colour of the others is the base colour times
//! the vertex colour.

use super::instruments::Fitted;
use super::print::Shape;
use super::{CabinModel, CabinPart, CabinSpec, Lever, Material, Motion, PanelControl, Picture, Print, Spot};
use engine::MeshBuilder;
use glam::Vec3;
use serde::{Deserialize, Serialize};

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// A node of `cockpit.glb` and how it moves (`Motion`): about `axis` through the node's origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartFile {
    pub node: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub moves: Motion,
    #[serde(default, skip_serializing_if = "is_default")]
    pub axis: Vec3,
}

/// A place the eye can be (`Spot`), by its name for the status line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SpotFile {
    /// The pilot's own view, along the nose: the view's height in degrees, and how far its middle
    /// is below where it looks (a fraction of half the screen).
    Seat { name: String, eye: Vec3, fov: f32, shift: f32 },
    /// From `eye` looking at `at`, `height` metres of it filling the view's height there.
    LookingAt { name: String, eye: Vec3, at: Vec3, height: f32 },
}

impl SpotFile {
    /// The spot; its name kept for the game's life.
    pub fn spot(&self) -> Spot {
        let keep = |name: &str| -> &'static str { Box::leak(name.to_string().into_boxed_str()) };
        match self {
            SpotFile::Seat { name, eye, fov, shift } => Spot::seat(keep(name), *eye, *fov, *shift),
            SpotFile::LookingAt { name, eye, at, height } => Spot::looking_at(keep(name), *eye, *at, *height),
        }
    }
}

/// What is printed in the cabin (`print`): its canvas in points, the parts of it laid on the
/// panel and on the floor (`Print`), how far forward the panel's part is laid at least (x, metres),
/// and its shapes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintFile {
    pub size: (f32, f32),
    pub panel: Print,
    pub floor: Print,
    pub panel_x: f32,
    pub shapes: Vec<Shape>,
}

/// `cockpit.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CockpitFile {
    /// The nodes that are the cabin, in the order they are drawn; a node not here is not drawn.
    pub parts: Vec<PartFile>,
    /// The node of the windows' glass, drawn over the world.
    pub glass: String,
    /// Where the eye can be, as the mouse wheel and the + and − keys go through them: the pilot's
    /// seat first; the one named "panel" shows the whole panel.
    pub spots: Vec<SpotFile>,
    /// The box the light through the windows and the occlusion are baked in (least and most corner).
    pub sky_box: (Vec3, Vec3),
    /// The instruments' canvas, in points, and the instruments painted into it.
    pub canvas: (f32, f32),
    pub instruments: Vec<Fitted>,
    pub print: PrintFile,
    /// The knobs the mouse can take: each lever's knob at its values 0 and 1 (flaps 1 is up, trim 1
    /// full nose down), body axes.
    #[serde(default)]
    pub levers: Vec<(Lever, [Vec3; 2])>,
    /// The keys and knobs the mouse can press and turn.
    #[serde(default)]
    pub controls: Vec<PanelControl>,
}

/// The cabin's material of a glTF material, and for a picture of the canvas whether it is lit as a dial.
fn material(m: Option<&engine::gltf::Material>) -> (Material, bool) {
    let Some(m) = m else { return (Material::Plastic, false) };
    let name = m.name.split('.').next().unwrap_or("").to_ascii_lowercase();
    let named = match name.as_str() {
        "plain" => Material::Plain,
        "plastic" => Material::Plastic,
        "leather" => Material::Leather,
        "fabric" => Material::Fabric,
        "carpet" => Material::Carpet,
        "rubber" => Material::Rubber,
        "metal" => Material::Metal,
        "screen" => Material::Screen,
        "dial" => return (Material::Screen, true),
        "lamp" => Material::Lamp,
        "printed" => Material::Printed,
        "vinyl" => Material::Vinyl,
        "chrome" => Material::Chrome,
        _ if m.texture.is_some() => Material::Textured,
        _ if m.emissive.max_element() > 0.0 => Material::Lamp,
        _ if m.metallic >= 0.5 && m.roughness < 0.2 => Material::Chrome,
        _ if m.metallic >= 0.5 => Material::Metal,
        _ => Material::Plastic,
    };
    (named, false)
}

/// The name of a cabin's material in a file, as `material` reads it.
#[cfg(test)]
pub fn material_name(m: Material, lit: bool) -> &'static str {
    match m {
        Material::Plain => "plain",
        Material::Plastic => "plastic",
        Material::Leather => "leather",
        Material::Fabric => "fabric",
        Material::Carpet => "carpet",
        Material::Rubber => "rubber",
        Material::Metal => "metal",
        Material::Screen if lit => "dial",
        Material::Screen => "screen",
        Material::Lamp => "lamp",
        Material::Printed => "printed",
        Material::Vinyl => "vinyl",
        Material::Chrome => "chrome",
        // By its picture, whatever its name.
        Material::Textured => "textured",
    }
}

impl CockpitFile {
    pub fn parse(text: &str) -> Result<CockpitFile, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    pub fn spots(&self) -> Vec<Spot> {
        self.spots.iter().map(SpotFile::spot).collect()
    }

    /// Everything the cockpit's renderer needs (`CabinSpec`), with the bytes of `cockpit.glb`.
    pub fn spec(&self, glb: &[u8]) -> Result<CabinSpec, String> {
        let model = engine::gltf::read(glb)?;
        let node = |name: &str| model.node(name).ok_or_else(|| format!("cockpit.glb has no node \"{name}\""));
        let mut cabin = CabinModel::new();
        // The textured materials' pictures, by the material's place in the file.
        let mut pictures: Vec<(usize, Picture)> = Vec::new();
        for part in &self.parts {
            let n = node(&part.node)?;
            let first = cabin.mesh.count();
            for (k, v) in n.vertices.iter().enumerate() {
                let m = n.materials[k / 3];
                let found = model.materials.get(m);
                let (kind, lit) = material(found);
                let gloss = 1.0 - found.map_or(1.0, |m| m.roughness).clamp(0.0, 1.0);
                let rgb = match kind {
                    Material::Screen => Vec3::new(v.uv.x, v.uv.y, if lit { 1.0 } else { 0.0 }),
                    Material::Textured => {
                        let layer = pictures.iter().position(|(of, _)| *of == m).unwrap_or_else(|| {
                            let image = |k: Option<usize>| k.and_then(|k| model.images.get(k)).cloned();
                            let colour = image(found.and_then(|f| f.texture)).unwrap_or(engine::gltf::Image { width: 1, height: 1, rgba: vec![255; 4] });
                            pictures.push((m, Picture { colour, normal: image(found.and_then(|f| f.normal)) }));
                            pictures.len() - 1
                        });
                        Vec3::new(v.uv.x, v.uv.y, layer as f32)
                    }
                    _ => model.color(v, m).truncate(),
                };
                cabin.mesh.vertex(v.position, v.normal, rgb.extend(kind as u8 as f32 + gloss * 0.9));
            }
            let count = cabin.mesh.count() - first;
            if count == 0 {
                return Err(format!("the node \"{}\" of cockpit.glb has no triangles", part.node));
            }
            cabin.parts.push(CabinPart { motion: part.moves, first, count, hinge: n.origin(), axis: part.axis });
        }
        let glass: MeshBuilder = model.mesh(node(&self.glass)?);
        let pictures: Vec<Picture> = pictures.into_iter().map(|(_, picture)| picture).collect();
        let same = |sizes: Vec<(u32, u32)>| sizes.windows(2).all(|w| w[0] == w[1]);
        if !same(pictures.iter().map(|p| (p.colour.width, p.colour.height)).collect()) || !same(pictures.iter().filter_map(|p| p.normal.as_ref()).map(|n| (n.width, n.height)).collect()) {
            return Err("cockpit.glb: its materials' pictures are not all of one size (nor their normal maps)".to_string());
        }
        Ok(CabinSpec {
            model: cabin,
            pictures,
            glass,
            sky_box: self.sky_box,
            canvas_points: self.canvas,
            print_points: self.print.size,
            panel_print: self.print.panel,
            floor_print: self.print.floor,
            panel_x: self.print.panel_x,
            print: self.print.shapes.clone(),
            levers: self.levers.clone(),
            controls: self.controls.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cockpit::material_of;
    use sim_core::AircraftKind;

    /// Each material's name in a file reads back as that material.
    #[test]
    fn material_names_read_back() {
        use Material::*;
        for m in [Plain, Plastic, Leather, Fabric, Carpet, Rubber, Metal, Screen, Lamp, Printed, Vinyl, Chrome] {
            for lit in [false, true] {
                let lit = lit && m == Screen;
                let name = format!("{}.001", material_name(m, lit));
                let file = engine::gltf::Material { name, ..Default::default() };
                assert_eq!(material(Some(&file)), (m, lit));
            }
        }
    }

    /// Every vertex of each cockpit says a material there is and is sound, a display's or dial's
    /// vertices point inside an instrument of the canvas, the parts follow each other, and the
    /// levers' knobs are on their parts.
    #[test]
    fn the_cockpits_are_sound() {
        // Every craft's that has one: the aircraft's, and a multirotor's.
        let mut cabins: Vec<&str> = crate::kinds::Craft::all().filter_map(crate::kinds::cabin).collect();
        cabins.dedup();
        assert!(cabins.len() > AircraftKind::all().map(|k| k.spec().visual.cockpit).collect::<std::collections::BTreeSet<_>>().len());
        for kind in cabins {
            let file = crate::kinds::cockpit_in(kind);
            let spec = crate::kinds::cockpit_spec_in(kind).unwrap();
            let mesh = &spec.model.mesh;
            assert!(mesh.count() <= 450_000, "{kind:?}: {} vertices", mesh.count());
            let (w, h) = file.canvas;
            let mut screens = 0;
            for v in mesh.data.as_chunks::<10>().0 {
                assert!(v.iter().all(|x| x.is_finite()));
                assert!((Vec3::new(v[3], v[4], v[5]).length() - 1.0).abs() < 1e-3, "{kind:?}: normal at {:?}", &v[..3]);
                let (material, gloss) = material_of(glam::Vec4::new(v[6], v[7], v[8], v[9]));
                assert!(material <= Material::Textured as u8 && (0.0..=1.0001).contains(&gloss), "{kind:?}: material {material}, gloss {gloss}");
                if material == Material::Screen as u8 {
                    screens += 1;
                    let (x, y) = (v[6] * w, v[7] * h);
                    let inside = file.instruments.iter().any(|f| (f.at[0] - 0.1..=f.at[0] + f.at[2] + 0.1).contains(&x) && (f.at[1] - 0.1..=f.at[1] + f.at[3] + 0.1).contains(&y));
                    assert!(inside, "{kind:?}: a picture's corner at {x}, {y} on no instrument");
                }
            }
            assert!(screens > 0, "{kind:?}");
            let mut next = 0;
            for part in &spec.model.parts {
                assert_eq!(part.first, next, "{kind:?}: {:?}", part.motion);
                assert!(part.count > 0 && part.count % 3 == 0, "{kind:?}: {:?}", part.motion);
                next += part.count;
            }
            assert_eq!(next, mesh.count());
            for (lever, [knob, _]) in &spec.levers {
                let near = |part: &CabinPart| mesh.data[part.first as usize * 10..(part.first + part.count) as usize * 10].as_chunks::<10>().0.iter().map(|v| (Vec3::new(v[0], v[1], v[2]) - *knob).length()).fold(f32::MAX, f32::min);
                let nearest = spec.model.parts.iter().filter(|p| p.motion != Motion::Fixed).map(near).fold(f32::MAX, f32::min);
                assert!(nearest < 0.03 || *lever == Lever::Trim, "{kind:?} {lever:?}: knob {nearest} m from its part");
            }
            assert!(spec.glass.count() > 0);
            assert!(file.spots.len() >= 2 && file.spots().iter().all(|s| s.fov > 0.1 && s.fov < 2.0));
        }
    }
}
