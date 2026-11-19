//! The aircraft as they are seen from outside, read from files: a model is a folder of
//! `assets/aircraft/` (the one a type's `visual.model` names) with
//! - `exterior.glb`: the mesh in body axes (x forward, y right, z down, metres from the centre of
//!   gravity, so the points the simulation uses are where they are drawn), in named nodes (glTF,
//!   `engine::gltf`); a node's origin is the point it moves about;
//! - `exterior.ron`: what moves each node and how ([`Exterior`]), the lights (at nodes' origins),
//!   the shadow it casts and, for the one parked at the aerodromes, how it stands there.
//!
//! What the renderer draws of an aircraft is its [`AircraftModel`]: the mesh with each node a
//! [`Part`], a range of its vertices (`kinds::model` reads a type's). Nothing here touches the GPU. The scenery's models are the
//! world's (`world_render::vegetation`, `world_render::buildings`).

use engine::MeshBuilder;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use world_render::buildings::{self, Building};
use world_render::ShadowShape;

/// What a node of a model is and how it moves: the files' word for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Moves {
    /// With the airframe.
    #[default]
    Fixed,
    /// A control surface: it turns about `axis` through its origin by the surface's angle (radians,
    /// as the simulation has it: a flap's, an aileron's or the elevator's trailing edge down, the
    /// rudder's to the right) times `scale`. Torn off it is not drawn; broken loose it trails.
    Surface { by: Surface, scale: f32 },
    /// A leg of the gear (0 the nose or tail wheel's, 1 the left, 2 the right: `AircraftType::gear`),
    /// in three parts. The strut is fixed to the airframe at its origin and `axis` reaches from there
    /// to its foot, which the spring's compression raises, bending it; a zero `axis` is a rigid
    /// one (an oleo's cylinder).
    Strut { leg: usize },
    /// What the compression raises and the pedals steer: piston, fork, axle, fairing. It steers
    /// about `axis` through its origin.
    Fork { leg: usize },
    /// The tyre and its hub, turning as they roll about the axle: its origin, along y.
    Wheel { leg: usize },
    /// Turns about `axis` through its origin at the engine's speed; from the cockpit, turning, it is
    /// not drawn.
    Propeller,
    /// The propeller's blur, drawn see-through as it turns.
    Disc,
    /// A multirotor's propeller `index` (its motor's, in the type's order): turns about `axis`
    /// through its origin at that rotor's speed, the axis pointing the way it turns.
    Rotor { index: usize },
    /// That propeller's blur, drawn see-through as it turns.
    RotorDisc { index: usize },
    /// Of a gear that retracts, what is seen only while it is out (the doors hanging open, the
    /// wells' linings), and what only once it is up (the doors closed over the wells).
    GearOut,
    GearUp,
    /// A tilt-rotor's nacelle (0 the left, 1 the right) and what is fixed to it: it turns about
    /// the line across the frame through its origin by the nacelle's angle, forward. The rotors
    /// on it (`Rotor`, `RotorDisc`, given with the nacelle upright) turn with it.
    Nacelle { index: usize },
}

/// The control surfaces, as the simulation and its damage name them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface {
    AileronLeft,
    AileronRight,
    FlapLeft,
    FlapRight,
    Elevator,
    Rudder,
    /// The elevator's trim tab: its angle is the trim's, as the simulation adds it to the elevator.
    Trim,
}

/// Whether a part is drawn when the view is in the aircraft's cockpit (whose own lining is drawn then).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Inside {
    #[default]
    Shown,
    Hidden,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// A node of `exterior.glb` and what it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartFile {
    pub node: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub moves: Moves,
    /// The line it turns about (with its origin), or how a strut reaches to its foot; body axes.
    #[serde(default, skip_serializing_if = "is_default")]
    pub axis: Vec3,
    #[serde(default, skip_serializing_if = "is_default")]
    pub inside: Inside,
    /// Whether it shades the cockpit from the sun (the wing, the cowling: what is near it).
    #[serde(default, skip_serializing_if = "is_default")]
    pub shades_cabin: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightKind {
    /// Red on the left tip, green on the right, white at the tail: steady, from dusk to dawn.
    Navigation,
    /// Red: flashes, day and night.
    Beacon,
    /// White, ahead: from dusk to dawn.
    Landing,
}

/// A light, at the origin of a node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightFile {
    pub node: String,
    pub kind: LightKind,
    /// Linear colour, brighter than 1. A navigation light's says its side: redder left, greener right.
    pub color: Vec3,
}

/// The shadow a model casts (`world_render::ShadowShape`, which says what each number is).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub flat: [[f32; 4]; 4],
    pub flat_z: [f32; 4],
    pub upright: [[f32; 4]; 3],
}

/// How the model stands parked at the aerodromes: how far its wheels are below its centre of
/// gravity, and the slabs it is run into as (base in the building's axes: x forward, y up, z right;
/// half length along x and half width along z; height).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parked {
    pub lift: f32,
    pub hull: Vec<(Vec3, (f32, f32), f32)>,
}

/// `exterior.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exterior {
    /// The nodes that are parts, in the order they are drawn; a node not here is not drawn.
    pub parts: Vec<PartFile>,
    #[serde(default)]
    pub lights: Vec<LightFile>,
    pub shadow: Shadow,
    #[serde(default)]
    pub parked: Option<Parked>,
}

impl Exterior {
    /// The file's text read.
    pub fn parse(text: &str) -> Result<Exterior, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    pub fn shadow(&self) -> ShadowShape {
        ShadowShape { flat: self.shadow.flat, flat_z: self.shadow.flat_z, upright: self.shadow.upright, ..ShadowShape::default() }
    }
}

/// A range of the aircraft's vertices that moves as one piece, as its file says: about the line
/// through `hinge` along `axis` (`Moves`).
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub moves: Moves,
    pub inside: Inside,
    pub shades_cabin: bool,
    pub first: u32,
    pub count: u32,
    pub hinge: Vec3,
    pub axis: Vec3,
}

impl Part {
    /// Whether it is a propeller's blur, drawn only as it turns, or what is drawn only with the
    /// gear up: not of the aircraft as it stands.
    pub fn blur(&self) -> bool {
        matches!(self.moves, Moves::Disc | Moves::RotorDisc { .. } | Moves::GearUp)
    }

    /// The leg of the gear it belongs to, if it is one's.
    pub fn leg(&self) -> Option<usize> {
        match self.moves {
            Moves::Strut { leg } | Moves::Fork { leg } | Moves::Wheel { leg } => Some(leg),
            _ => None,
        }
    }
}

/// A light of the aircraft, drawn by the renderer as a small glowing square.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub position: Vec3,
    pub color: Vec3,
    pub kind: LightKind,
}

/// An aircraft in body axes (x forward, y right, z down), metres from the centre of gravity.
pub struct AircraftModel {
    /// Each vertex with its colour: the material's, its texture's blurred to the size of a
    /// triangle (`engine::gltf::Model::mesh`), for what is drawn without the textures.
    pub mesh: MeshBuilder,
    /// For each vertex of `mesh`, its texture coordinates and material (`engine::gltf::Model::surface`),
    /// each part's triangles in the order of their materials; drawn with `look`'s textures.
    pub surface: Vec<[f32; 3]>,
    /// The materials and their pictures, if the model has textures (an imported one).
    pub look: Option<Look>,
    pub parts: Vec<Part>,
    pub lights: Vec<Light>,
}

/// What a textured model is painted with: its glTF's materials and images (`world_render::WorldRenderer::add_look`).
pub struct Look {
    pub materials: Vec<engine::gltf::Material>,
    pub images: Vec<engine::gltf::Image>,
}

impl AircraftModel {
    /// The surface of the vertices `first .. first + count` of the mesh.
    pub fn surface_of(&self, first: u32, count: u32) -> &[[f32; 3]] {
        &self.surface[first as usize..(first + count) as usize]
    }
}

/// How many levels of its mip chain a texture is shrunk for the colour each vertex keeps
/// (`engine::gltf::Model::blurred`): a 2048 picture to 128, some 8 cm a texel on a light aircraft.
const VERTEX_BLUR: u32 = 4;

/// Builds a model from its files: `exterior` read, and the bytes of its glTF.
pub fn build(exterior: &Exterior, glb: &[u8]) -> Result<AircraftModel, String> {
    let model = engine::gltf::read(glb)?;
    let textured = model.materials.iter().any(|m| m.textured());
    let blurred;
    let soft = if textured {
        blurred = model.blurred(VERTEX_BLUR);
        &blurred
    } else {
        &model
    };
    let mut mesh = MeshBuilder::new();
    let mut surface = Vec::new();
    let mut parts = Vec::new();
    for part in &exterior.parts {
        let node = model.node(&part.node).ok_or_else(|| format!("exterior.glb has no node \"{}\"", part.node))?;
        let node = model.sorted(node);
        let first = mesh.count();
        mesh.data.extend_from_slice(&soft.mesh(&node).data);
        surface.extend(model.surface(&node));
        let count = mesh.count() - first;
        if count == 0 {
            return Err(format!("the node \"{}\" of exterior.glb has no triangles", part.node));
        }
        parts.push(Part { moves: part.moves, inside: part.inside, shades_cabin: part.shades_cabin, first, count, hinge: node.origin(), axis: part.axis });
    }
    let lights = exterior
        .lights
        .iter()
        .map(|light| match model.node(&light.node) {
            Some(node) => Ok(Light { position: node.origin(), color: light.color, kind: light.kind }),
            None => Err(format!("exterior.glb has no node \"{}\" for a light", light.node)),
        })
        .collect::<Result<_, _>>()?;
    let look = textured.then_some(Look { materials: model.materials, images: model.images });
    Ok(AircraftModel { mesh, surface, look, parts, lights })
}

/// The aircraft standing on its wheels, engine stopped, nose towards +x, as a building of the world
/// (`buildings::PARKED`: y up, origin on the ground at its middle), as `parked` says; without it on
/// its lowest point, its bounds for a hull.
pub fn parked(model: &AircraftModel, parked: Option<&Parked>) -> Building {
    let points = || model.parts.iter().filter(|p| !p.blur()).flat_map(|p| model.mesh.data[p.first as usize * 10..(p.first + p.count) as usize * 10].as_chunks::<10>().0.iter().map(|v| Vec3::new(v[0], v[1], v[2])));
    let (lo, hi) = points().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let bounds = Parked {
        lift: hi.z,
        hull: vec![(Vec3::new((lo.x + hi.x) / 2.0, 0.0, (lo.y + hi.y) / 2.0), ((hi.x - lo.x) / 2.0, (hi.y - lo.y) / 2.0), hi.z - lo.z)],
    };
    let parked = parked.unwrap_or(&bounds);
    let mut mesh = MeshBuilder::new();
    for part in model.parts.iter().filter(|p| !p.blur()) {
        for vertex in model.mesh.data[part.first as usize * 10..(part.first + part.count) as usize * 10].as_chunks::<10>().0 {
            // Body axes (x forward, y right, z down) to x forward, y up, z right; wheels on the ground.
            mesh.data.extend_from_slice(&[vertex[0], -vertex[2] + parked.lift, vertex[1], vertex[3], -vertex[5], vertex[4], vertex[6], vertex[7], vertex[8], buildings::PLAIN]);
        }
    }
    let mut hull = MeshBuilder::new();
    for &(base, half, height) in &parked.hull {
        buildings::slab(&mut hull, base, half, height);
    }
    Building { mesh, hull }
}

/// The surface of each vertex of `parked`'s building (`AircraftModel::surface`), in its order: to
/// draw it with the model's look.
pub fn parked_surface(model: &AircraftModel) -> Vec<[f32; 3]> {
    model.parts.iter().filter(|p| !p.blur()).flat_map(|p| model.surface_of(p.first, p.count).iter().copied()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{AircraftKind, AircraftType};

    fn points(model: &AircraftModel, part: &Part) -> Vec<Vec3> {
        model.mesh.data[part.first as usize * 10..(part.first + part.count) as usize * 10].as_chunks::<10>().0.iter().map(|v| Vec3::new(v[0], v[1], v[2])).collect()
    }

    fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
        points.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), &p| (lo.min(p), hi.max(p)))
    }

    fn every_model() -> Vec<(AircraftType, AircraftModel)> {
        AircraftKind::all().map(|kind| (kind.spec().clone(), crate::kinds::model(kind).unwrap())).collect()
    }

    /// Each model's parts follow each other over the whole mesh, every vertex is sound, and it has
    /// what the renderer moves: each leg in its parts (a strut at most, what the spring raises,
    /// a wheel for each axle), and a propeller and its blur unless it is a jet.
    #[test]
    fn the_models_are_sound() {
        for (spec, model) in every_model() {
            let mut next = 0;
            for part in &model.parts {
                assert_eq!(part.first, next, "{:?}: {:?}", spec.kind, part.moves);
                assert!(part.count > 0 && part.count % 3 == 0, "{:?}: {:?} has {} vertices", spec.kind, part.moves, part.count);
                next += part.count;
            }
            assert_eq!(next, model.mesh.count());
            // A textured model's surface: a pair of texture coordinates and a material of its look for every vertex.
            assert_eq!(model.surface.len() as u32, model.mesh.count());
            if let Some(look) = &model.look {
                assert!(model.surface.iter().all(|s| s[0].is_finite() && s[1].is_finite() && (s[2] as usize) < look.materials.len()), "{:?}: a vertex without its material", spec.kind);
                for m in &look.materials {
                    for image in [m.texture, m.metallic_roughness, m.normal, m.occlusion, m.emissive_texture].into_iter().flatten() {
                        let image = &look.images[image];
                        assert!(image.width <= 2048 && image.height <= 2048, "{:?}: {} has a {}x{} picture", spec.kind, m.name, image.width, image.height);
                    }
                }
            }
            assert!(model.mesh.count() <= 150_000, "{:?}: {} vertices", spec.kind, model.mesh.count());
            for v in model.mesh.data.as_chunks::<10>().0 {
                assert!(v.iter().all(|x| x.is_finite()));
                let n = Vec3::new(v[3], v[4], v[5]).length();
                assert!((n - 1.0).abs() < 1e-3, "{:?}: normal of length {n} at {:?}", spec.kind, &v[..3]);
                // The gloss, 2 more on a metal (`engine::gltf::Model::gloss`).
                assert!((0.0..=1.0).contains(&v[9]) || (2.0..=3.0).contains(&v[9]), "{:?}: gloss {}", spec.kind, v[9]);
            }
            for leg in 0..spec.gear.len() {
                let count = |moves: Moves| model.parts.iter().filter(|p| p.moves == moves).count();
                assert!(count(Moves::Strut { leg }) <= 1, "{:?}: leg {leg}'s struts", spec.kind);
                assert_eq!(count(Moves::Fork { leg }), 1, "{:?}: leg {leg}'s fork", spec.kind);
                assert!((1..=2).contains(&count(Moves::Wheel { leg })), "{:?}: leg {leg}'s wheels", spec.kind);
            }
            assert_eq!(model.parts.iter().filter(|p| p.moves == Moves::Propeller).count(), usize::from(spec.jets.is_none()));
            assert!(model.lights.iter().any(|l| l.kind == LightKind::Navigation) && model.lights.iter().any(|l| l.kind == LightKind::Beacon));
        }
    }

    /// Each wheel stands on its contact point and turns about its centre, right above it (a
    /// bogie's two axles, as far before it as behind); the steering wheel steers about a line
    /// through its fork near upright; a bending leg reaches out.
    #[test]
    fn the_wheels_stand_on_the_simulations_contact_points() {
        for (spec, model) in every_model() {
            for (leg, wheel) in spec.gear.iter().enumerate() {
                let of = |moves: Moves| *model.parts.iter().find(|p| p.moves == moves).unwrap();
                let contact = Vec3::new(wheel.contact.x as f32, wheel.contact.y as f32, wheel.contact.z as f32);
                let tyres: Vec<Part> = model.parts.iter().filter(|p| p.moves == Moves::Wheel { leg }).copied().collect();
                for tyre in &tyres {
                    let (_, hi) = bounds(&points(&model, tyre));
                    assert!((hi.z - contact.z).abs() < 0.02, "{:?} leg {leg}: lowest point {}, contact point {contact}", spec.kind, hi.z);
                }
                let centre = tyres.iter().map(|t| t.hinge).sum::<Vec3>() / tyres.len() as f32;
                assert!((centre.truncate() - contact.truncate()).length() < 2e-3, "{:?} leg {leg}: wheels' centre {centre}", spec.kind);
                let fork = of(Moves::Fork { leg });
                if wheel.steers {
                    assert!(fork.axis.z > 0.95 && (fork.axis.length() - 1.0).abs() < 1e-5, "{:?} leg {leg}: steers about {}", spec.kind, fork.axis);
                }
            }
        }
    }

    /// The span and the propeller are the simulation's: the tips where it has them, the propeller
    /// turning on its thrust line, its blades reaching its radius, the blur with it.
    #[test]
    fn they_are_drawn_where_the_simulation_flies_them() {
        for (spec, model) in every_model() {
            let all: Vec<Vec3> = model.parts.iter().filter(|p| !p.blur()).flat_map(|p| points(&model, p)).collect();
            let (lo, hi) = bounds(&all);
            assert!((hi.y - lo.y - spec.span as f32).abs() < 0.12, "{:?}: span {}", spec.kind, hi.y - lo.y);
            if spec.jets.is_some() {
                continue;
            }
            let prop = *model.parts.iter().find(|p| p.moves == Moves::Propeller).unwrap();
            assert!((prop.hinge.z - spec.prop_pos.z as f32).abs() < 1e-4, "{:?}: hub at {}", spec.kind, prop.hinge);
            assert_eq!(prop.axis, Vec3::X);
            let reach = points(&model, &prop).iter().map(|p| (*p - prop.hinge).length()).fold(0.0, f32::max);
            assert!((reach - spec.prop_diameter as f32 / 2.0).abs() < 0.02, "{:?}: blade tips at {reach}", spec.kind);
            // The blades turn true: their tips, all round, in one plane square to the shaft.
            let mut sectors = [(0.0f32, 0usize); 12];
            for p in points(&model, &prop).iter().map(|p| *p - prop.hinge).filter(|p| p.length() > reach * 0.9) {
                let k = ((p.z.atan2(p.y) / std::f32::consts::TAU + 0.5) * 12.0) as usize % 12;
                sectors[k] = (sectors[k].0 + p.x, sectors[k].1 + 1);
            }
            let tips: Vec<f32> = sectors.iter().filter(|s| s.1 > 0).map(|s| s.0 / s.1 as f32).collect();
            let wobble = tips.iter().fold(f32::MIN, |a, &b| a.max(b)) - tips.iter().fold(f32::MAX, |a, &b| a.min(b));
            assert!(tips.len() >= 2 && wobble < 0.015, "{:?}: the blade tips are {wobble} m apart along the shaft", spec.kind);
            let disc = model.parts.iter().find(|p| p.moves == Moves::Disc).unwrap();
            assert_eq!(disc.hinge, prop.hinge);
        }
    }

    /// Each control surface but the flaps (which hinge below the wing) turns about a line through itself.
    #[test]
    fn hinges_are_inside_their_surfaces() {
        for (spec, model) in every_model() {
            for part in model.parts.iter().filter(|p| matches!(p.moves, Moves::Surface { by, .. } if !matches!(by, Surface::FlapLeft | Surface::FlapRight))) {
                let (lo, hi) = bounds(&points(&model, part));
                let h = part.hinge;
                assert!(h.cmpge(lo - 0.01).all() && h.cmple(hi + 0.01).all(), "{:?} {:?}: hinge {h} outside {lo}..{hi}", spec.kind, part.moves);
                assert!((part.axis.length() - 1.0).abs() < 1e-4, "{:?} {:?}: axis {}", spec.kind, part.moves, part.axis);
            }
        }
    }

    /// The parked aircraft has a surface for every vertex, to be drawn with its model's look.
    #[test]
    fn the_parked_aircraft_has_its_surface() {
        let kind = crate::kinds::parked_kind().unwrap();
        let model = crate::kinds::model(kind).unwrap();
        let building = parked(&model, crate::kinds::exterior(kind).parked.as_ref());
        assert_eq!(parked_surface(&model).len() as u32, building.mesh.count());
    }

    /// The parked aircraft is what the simulation runs into on the aprons.
    #[test]
    fn the_parked_aircraft_is_a_sound_building() {
        buildings::check(buildings::PARKED, &crate::kinds::parked_aircraft().unwrap(), 1.0).unwrap();
    }
}
