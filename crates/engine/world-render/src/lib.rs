//! Draws the world of `sim-core` with `engine`: terrain, sky, clouds, sun, moon and stars, haze,
//! sea and rivers, the ground's photographs, trees, hedges, towns, aerodromes and their lights, the
//! shadows of the landscape, and the wrecks and their fire. It knows nothing about the vehicles of
//! the game that uses it: they reach it as meshes and draws ([`Scene::objects`]), as the meshes
//! whose shadows it casts ([`Scene::casters`]) and as the body the view belongs to ([`Scene::body`]).
//!
//! Each frame the game gives a [`Scene`] to [`WorldRenderer::prepare`] (in `Game::update`), then
//! calls [`WorldRenderer::draw_sky`] and [`WorldRenderer::draw`] (in `Game::draw`), and gives
//! [`WorldRenderer::post`] to the engine (`Game::post`). The contract is in the crate's README.
//!
//! Render axes: x east, y up, z south, relative to the camera, `f32`. World axes are
//! `sim-core`'s: north, east, down, `f64`; [`to_render`] turns one into the other.

pub mod buildings;
mod casters;
pub mod cloud_noise;
pub mod clouds;
mod effect_pipelines;
mod effects;
mod fire;
mod bridges;
mod footprints;
mod grass;
mod heightmap;
mod imagery;
mod jobs;
mod lights;
mod looks;
pub mod materials;
pub mod models;
pub mod pipelines;
mod particles;
mod quality;
mod reflection;
mod renderer;
mod parcels;
mod roadmap;
mod scatter;
mod sea;
pub mod sky;
mod terrain;
pub mod track;
mod foliage;
mod impostors;
mod species;
pub mod vegetation;

pub use lights::gain as light_gain;
pub use looks::LookId;
pub use pipelines::{shader_source, ObjectUniforms};
pub use quality::{CloudMarch, Preset, Quality};
pub use renderer::WorldRenderer;
pub use track::TrackRenderer;

use sim_core::{smoothstep, Cut, Weather, Wreck, DEG};
use glam::{DQuat, DVec3, Mat4, Vec3};

/// World axes (north, east, down) to render axes (east, up, south).
pub fn to_render(v: DVec3) -> Vec3 {
    Vec3::new(v.y as f32, -v.z as f32, -v.x as f32)
}

/// World axes from render axes: the other way of [`to_render`].
pub fn from_render(v: Vec3) -> DVec3 {
    DVec3::new(-v.z as f64, v.x as f64, -v.y as f64)
}

/// Where the picture is taken from.
#[derive(Clone, Copy, Debug)]
pub struct ViewPoint {
    /// Eye position in world axes (north, east, down), metres.
    pub position: DVec3,
    /// Orientation in render axes.
    pub forward: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    /// Moves the point looked at up the screen, as a fraction of half its height (see `engine::Camera`).
    pub shift_y: f32,
    /// Nearest distance drawn, metres: 0.25 outside, less with walls beside the eye.
    pub near: f32,
}

/// The sun, or by night the moon: the direction towards it in render axes and how much day it is
/// (0 night, 1 full day). The colour of its light depends on the air it comes through (`sky.rs`).
#[derive(Clone, Copy, Debug)]
pub struct Sun {
    pub direction: Vec3,
    pub day: f32,
}

/// Where the sun is at an hour of the day (0..24), at 40° north with a declination of 12°: towards
/// the east, the north and up, a unit vector.
pub(crate) fn sun_position(hour: f64) -> (f64, f64, f64) {
    let (lat, dec) = (40.0 * DEG, 12.0 * DEG);
    let h = (hour - 12.0) * 15.0 * DEG;
    let east = -dec.cos() * h.sin();
    let north = lat.cos() * dec.sin() - lat.sin() * dec.cos() * h.cos();
    let up = lat.sin() * dec.sin() + lat.cos() * dec.cos() * h.cos();
    (east, north, up)
}

/// How high the sun is at an hour of the day, radians above the horizon (below it, negative).
pub fn sun_elevation(hour: f64) -> f64 {
    sun_position(hour).2.asin()
}

/// What lights the world at an hour of the day (0..24), at 40° north with a declination of 12°:
/// the sun while it is up, then the moon (`sky::moon_direction`).
pub fn sun(hour: f64) -> Sun {
    let (direction, _) = sky::SkyLights::at(hour).direct();
    Sun { direction, day: smoothstep(-0.12, 0.12, sun_position(hour).2) as f32 }
}

/// The shadow a body casts on the ground, the sea, the trees and the buildings, as a few flat
/// pieces in its own axes (x forward, y right, z down, metres from its origin); each piece is
/// (centre, half size) along x and along the other axis of the plane it lies in. Every piece hides
/// as much of the sun's disc as it covers, so the shadow is sharp near the ground and a faint
/// penumbra far above it. Unused pieces are left at zero size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShadowShape {
    /// Pieces lying flat: (centre x, half length along x, centre y, half width along y), each at the
    /// height `flat_z` of the same place.
    pub flat: [[f32; 4]; 4],
    pub flat_z: [f32; 4],
    /// Pieces standing in the plane y = 0: (centre x, half length along x, centre z, half height along z).
    pub upright: [[f32; 4]; 3],
    /// Solid pieces, for what is too round or too slanted to lie in those planes (the gear's
    /// wheels, fairings and legs), each as two rows: an ellipsoid along the body's axes is
    /// `[[centre x, y, z, 1], [radius along x, y, z, 0]]`; a rod (a capsule) is `[[one end x, y, z,
    /// 2], [the other end x, y, z, radius]]`; `[[.., 0], ..]` is none. Their shadows are sharp where
    /// they touch the ground, and the ellipsoids darken the ground under and round them as they
    /// hide the sky from it (the soft shadow where a tyre stands on the runway). The game moves
    /// them with the gear's springs each frame.
    pub solids: [[[f32; 4]; 2]; SHADOW_SOLIDS],
}

/// How many solid pieces a [`ShadowShape`] has room for: three legs of a strut, a fairing and a wheel.
pub const SHADOW_SOLIDS: usize = 9;

/// The body the view belongs to: the one followed or ridden in. Its shadow is drawn, and its axes
/// reach the shaders (`frame.body_*`) for a game's own pipelines, such as a cabin lit by its windows.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    /// Origin in world axes, metres.
    pub position: DVec3,
    /// Its axes (x forward, y right, z down) to world axes.
    pub attitude: DQuat,
    /// What shadow it casts; `None` for none.
    pub shadow: Option<ShadowShape>,
}

/// A vehicle whose shadow is drawn from its own meshes: they are drawn as the sun sees them into a
/// map of their own (`casters.rs`), and that map darkens everything near: the ground, the circuit,
/// the grass, the buildings, the other vehicles and the vehicle itself. The first of
/// [`Scene::casters`] should be the body followed ([`Scene::body`]): far below it (an aircraft in
/// the air) its shadow is its `ShadowShape`'s instead. On the lowest quality only that is drawn.
#[derive(Clone, Copy)]
pub struct Caster<'a> {
    /// The middle of a sphere that holds the whole vehicle, world axes, and its radius, metres:
    /// the map is fitted round it.
    pub centre: DVec3,
    pub radius: f32,
    /// What casts the shadow: the vehicle's draws as they are now (its outside, all of it, also
    /// what is not drawn from a seat inside it).
    pub draws: &'a [CasterDraw<'a>],
}

/// A range of a mesh that casts a shadow, where it is now.
#[derive(Clone, Copy)]
pub struct CasterDraw<'a> {
    pub mesh: CasterMesh<'a>,
    /// The vertices drawn, or the indices if the mesh has them.
    pub first: u32,
    pub count: u32,
    /// Mesh space to render axes relative to the camera (columns), as for drawing it.
    pub model: Mat4,
    /// Planes it is cut along, as `ObjectDraw::cuts`.
    pub cuts: [Option<Cut>; 2],
}

/// Which mesh a [`CasterDraw`] draws: one the world keeps ([`WorldRenderer::add_mesh`]) or one of
/// the game's own, its vertices as `MeshBuilder::layout()` has them.
#[derive(Clone, Copy)]
pub enum CasterMesh<'a> {
    World(MeshId),
    Game(&'a engine::Mesh),
}

/// A mesh the game keeps on the GPU with [`WorldRenderer::add_mesh`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshId(usize);

/// A range of the vertices of a mesh of the game's, drawn lit by the sun and the sky, hazed with distance.
#[derive(Clone, Copy, Debug)]
pub struct ObjectDraw {
    pub mesh: MeshId,
    pub first: u32,
    pub count: u32,
    /// Mesh space to render axes relative to the camera (columns).
    pub model: Mat4,
    /// Multiplies the mesh's colour; `a` is the opacity of a see-through draw.
    pub tint: [f32; 4],
    /// Planes in the mesh's axes it is cut along, with a ragged edge (what a tear left of a vehicle,
    /// `sim_core::Cut`); `[None; 2]`: whole.
    pub cuts: [Option<Cut>; 2],
}

/// A light seen as a glowing square that faces the camera: a vehicle's navigation light, say.
#[derive(Clone, Copy, Debug)]
pub struct Glow {
    /// Where, in render axes relative to the camera.
    pub at: Vec3,
    /// Its colour and brightness, linear.
    pub color: Vec3,
}

/// A vehicle's light as it shines, drawn as the aerodromes' are (`lights.wgsl`): a point of so many
/// candelas through the haze, with its glow in mist, mirrored in the sea; and, when its beam is
/// narrow and strong (a landing or taxi light), lighting what it points at as the floodlights do.
#[derive(Clone, Copy, Debug)]
pub struct Lamp {
    /// Where, in render axes relative to the camera.
    pub at: Vec3,
    /// Its aviation colour.
    pub color: sim_core::lights::Color,
    /// Candelas in the middle of its beam, and all round.
    pub intensity: f32,
    pub omni: f32,
    /// Its beam, if it has one: the axis (render axes, unit), and its half width and half height
    /// (radians off the axis where it is half as bright).
    pub beam: Option<(Vec3, f32, f32)>,
    /// The radius of its lens, metres.
    pub radius: f32,
}

/// Everything the world needs to know about one frame.
pub struct Scene<'a> {
    pub view: ViewPoint,
    /// Hour of the day, 0..24, and the simulation's time in seconds (what moves: clouds, sea, fire).
    pub hour: f64,
    pub time: f64,
    /// The weather of the world: its wind near the ground (`Weather::mean_wind` at 6 m: the windsocks,
    /// the smoke and the waves, `sea.rs`), its visibility and humidity (the haze and the fog, `sky.rs`).
    pub weather: &'a Weather,
    /// Height of the eye (or of the body it follows) above the ground, metres: trees and houses are
    /// left out from high up.
    pub height_above_ground: f64,
    /// The body the view belongs to, if any.
    pub body: Option<Body>,
    /// The vehicles whose shadows are drawn from their meshes, the body followed first; empty, only
    /// the body's `ShadowShape` is cast.
    pub casters: &'a [Caster<'a>],
    /// The game's solid draws, after the world's scenery (its vehicles).
    pub objects: &'a [ObjectDraw],
    /// The game's lights, after its objects.
    pub glows: &'a [Glow],
    /// The game's vehicles' lights, as they shine: navigation, beacon, strobes, landing and taxi lights.
    pub lamps: &'a [Lamp],
    /// The game's see-through draws, blended over everything, last.
    pub see_through: &'a [ObjectDraw],
    /// What is left of the vehicles that came apart (`World::wrecks`): their pieces, if their shape
    /// was given (`WorldRenderer::add_wreck_shape`), and what their crash looks like: fire, smoke,
    /// fragments, sparks, splash and foam, the fire's light, the marks on the ground.
    pub wrecks: &'a [Wreck],
}

/// The source of the volumetric clouds (`clouds.wgsl`) after the sky tables' layout it reads
/// (`sky_common.wgsl`): whoever includes it adds the bindings and the lights it names.
pub fn cloud_shader_source(march: &quality::CloudMarch) -> String {
    include_str!("sky_common.wgsl").to_string() + &clouds::marched(include_str!("clouds.wgsl"), march)
}
