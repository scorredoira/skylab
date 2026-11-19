//! The render pipelines of the world and the data they are fed with.
//!
//! # Bindings
//!
//! Group 0, the same for every pipeline (`frame_layout`):
//! - binding 0: a uniform buffer holding one [`FrameUniforms`], seen by vertex and fragment shaders.
//! - binding 1: the height map, a 2D `R16Float` texture of heights above sea level around the camera
//!   (seen by vertex shaders too: a tree's shade by the relief is worked out once per vertex)
//!   (make its data with `half_floats`), and binding 2: a linear, clamp-to-edge sampler for it.
//! - bindings 3 and 4: the photographs of ground and buildings, two `texture_2d_array` of one layer
//!   per material (colour; normal + roughness + occlusion), and binding 5: their sampler
//!   (`Gpu::texture_sampler`). `materials.rs` loads them; any array of at least one layer does.
//! - bindings 6 to 10: the sky's tables (`sky.rs`): the air's transmittance and the sky seen from
//!   the eye (2D), the aerial perspective's light and transmittance (3D), the ambient light (2D),
//!   seen by vertex and fragment shaders; binding 11: their linear, clamp-to-edge sampler.
//! - binding 12: the road map (`roadmap.rs`), a 2D `R8Unorm` texture with its mip levels of how much
//!   of each texel round the camera is road, read with the sampler of binding 2.
//! - binding 13: the vehicles' shadow map (`casters.rs`), a 2D `Depth32Float` atlas of their
//!   tiles, and binding 14: its comparison sampler (fragment shaders).
//!
//! Group 1, per draw, a uniform buffer bound with a **dynamic offset** (binding 0, vertex shaders;
//! the objects' fragment shaders too, for their cuts):
//! - `chunk_layout`: one [`ChunkUniforms`]. Terrain, clouds, trees and houses.
//! - `object_layout`: one [`ObjectUniforms`]. Objects, blended objects, and whatever the
//!   game draws with pipelines of its own (`WorldRenderer::frame_layout`).
//!
//! Bind the buffer with a size of one struct; offsets must be multiples of the device's
//! `min_uniform_buffer_offset_alignment`. The sky uses group 0 only; the sea, as group 1,
//! `sea_layout`: binding 1, its mirror (`reflection.rs`), a 2D texture of the scene's format read
//! with the sampler of group 0, binding 2 (a texel of nothing when there is none).
//!
//! Group 2 of the terrain and its mirror, `imagery_layout`: binding 0, a real place's ground as the
//! satellites saw it (`imagery.rs`), the near and the far picture as two layers of a 2D
//! `Rgba8UnormSrgb` array with its mip levels, read with the sampler of group 0, binding 2. Binding 1,
//! its parcels (`parcels.rs`): a 2D `Rgba8Unorm` array of two layers a grid (the map, the colours),
//! one level, read texel by texel (one texel of nothing when there are none).
//!
//! Group 2 of the trees and their shadows, `flora_layout` (`impostors.rs`), bindings 2 to 9 (0 and 1
//! are the terrain's, which share the shaders' source): the foliage's cards (colour, detail) and the
//! barks (colour, normal map), 2D arrays with their mip levels read with binding 8, a repeating
//! anisotropic sampler; the species' pictures from far (colour, detail), 2D arrays read with binding
//! 9, clamped.
//!
//! # Vertex buffers
//!
//! - terrain: slot 0 = [`terrain_vertex_layout`]. Drawn indexed.
//! - objects, objects_blended: slot 0 = `MeshBuilder::layout()`.
//! - flora: slot 0 = `vegetation::FloraMesh::layout()`, slot 1 = [`instance_layout`], one per instance. Drawn indexed.
//! - impostors, tree_shadows, houses, house_shadows: slot 0 = `MeshBuilder::layout()`, slot 1 = [`instance_layout`], one per instance.
//! - footprints: slot 0 = [`footprint_vertex_layout`], drawn indexed.
//! - lights: slot 0 = `lights::instance_layout`, one per light, 6 vertices each.
//! - sky (3 vertices), sea (6) and clouds (6 per slice): none.

use crate::Quality;
use engine::pipeline::{Depth, PipelineDesc, PipelineMaker};
use engine::texture;
use engine::{wgpu, Gpu, MeshBuilder};
use std::mem::size_of;

/// Per-frame data, as the shaders see it (`struct Frame` in shaders.wgsl: same fields, same order).
/// Positions and directions are in render space (x east, y up, z south), relative to the camera.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrameUniforms {
    /// View-projection for camera-relative positions (columns), reversed depth.
    pub view_proj: [[f32; 4]; 4],
    /// xyz = camera right, w = tan(half fov x).
    pub cam_right: [f32; 4],
    /// xyz = camera up, w = tan(half fov y).
    pub cam_up: [f32; 4],
    /// xyz = camera forward, w = vertical shift of the view (`Camera::shift_y`).
    pub cam_fwd: [f32; 4],
    /// xyz = direction towards the sun (unit), w = time in seconds, wrapped every hour.
    pub sun_dir: [f32; 4],
    /// rgb = colour of the sunlight, w = daylight 0..1.
    pub sun_color: [f32; 4],
    /// x, z = camera position modulo 16384 m, y = camera altitude above sea level, w unused.
    pub cam_wrap: [f32; 4],
    /// xyz = the body the view belongs to (`Scene::body`), relative to the camera, w = strength of its shadow (0 = none).
    pub body_pos: [f32; 4],
    /// xyz = its forward axis, w unused.
    pub body_fwd: [f32; 4],
    /// xyz = its right axis, w unused.
    pub body_right: [f32; 4],
    /// xyz = its down axis, w unused.
    pub body_down: [f32; 4],
    /// Its shadow (`ShadowShape`): the pieces lying flat, the heights they lie at, the pieces standing.
    pub shadow_flat: [[f32; 4]; 4],
    pub shadow_flat_z: [f32; 4],
    pub shadow_upright: [[f32; 4]; 3],
    /// Its solid pieces (`ShadowShape::solids`), two rows each, and in `shadow_reach.x` the radius
    /// round the body that holds them all (0: none).
    pub shadow_solids: [[f32; 4]; 2 * crate::SHADOW_SOLIDS],
    pub shadow_reach: [f32; 4],
    /// x = number of airports in `airports_a`/`airports_b`, y = height of the cloud base relative to the camera,
    /// z = distance at which trees fade out, w = cloud cover 0..1.
    pub misc: [f32; 4],
    /// Per airport: xy = centre (x, z) relative to the camera, zw = runway direction (x, z), unit.
    pub airports_a: [[f32; 4]; 8],
    /// Per airport: x = half length, y = half width, z = runway number, w = height relative to the camera.
    pub airports_b: [[f32; 4]; 8],
    /// xy = centre (x, z) of the height map relative to the camera, z = half its extent in metres, w = 1 when it is ready.
    pub hmap: [f32; 4],
    /// x = thickness of the cloud layer, y = number of roads in `roads`, z = number of towns in `towns`,
    /// w = how many of the first airports are near enough for their mown grass to show.
    pub extra: [f32; 4],
    /// Per road: xy = one end, zw = the other end, both (x, z) relative to the camera.
    pub roads: [[f32; 4]; 32],
    /// Per town: xy = centre (x, z) relative to the camera, z = radius, w = direction of its street
    /// grid, radians (`sim_core::Town::grid`).
    pub towns: [[f32; 4]; 16],
    /// Per layer of the texture arrays: rgb = its average colour (linear), w = its average roughness.
    pub materials: [[f32; 4]; 20],
    /// x = number of fire lights in `fire_pos`/`fire_color`, y = of marks in `marks_a`/`marks_b`, z = of
    /// patches of foam in `foam`/`foam_b` (`effects.rs`). With none, the shaders do nothing more.
    pub fire_count: [f32; 4],
    /// Per light of the ground (a fire's, a floodlight's, a vehicle's beam): xyz = where, relative to the
    /// camera, w = the share of a beam's middle it spills all round; rgb = its colour times its
    /// strength (the light at 1 m).
    pub fire_pos: [[f32; 4]; 4],
    pub fire_color: [[f32; 4]; 4],
    /// Per light: xyz = the axis of its beam (render axes, unit), w = the angle off it where it is half
    /// as bright, radians; 0 = it shines all round.
    pub fire_axis: [[f32; 4]; 4],
    /// Per mark on the ground: xyz = its centre relative to the camera, w = radius of the scorch;
    /// x = radius of the hole dug, y = how hot it glows 0..1, z = a number 0..1 of its own.
    pub marks_a: [[f32; 4]; 8],
    pub marks_b: [[f32; 4]; 8],
    /// Per patch of foam on the water: xyz = its centre relative to the camera, w = radius; its
    /// strength 0..1 in `foam_b`, four to a vector.
    pub foam: [[f32; 4]; 8],
    pub foam_b: [[f32; 4]; 2],
    /// The water (`sea.rs`): x = wind speed (m/s), y = trains of waves in `waves`, z = significant
    /// wave height (m), w = how many standard deviations above the mean a crest stands to break.
    pub sea: [f32; 4],
    /// xyz = slope covariance (xx, zz, xz) of what the trains leave out, w = part of the sea white with breaking crests.
    pub sea_rough: [f32; 4],
    /// xy = the way the wind blows (x, z), unit, z = distance from the camera from which nothing is
    /// drawn under the sea (the end of the ground), w = period of the waves at the shore, s.
    pub sea_more: [f32; 4],
    /// Per train of waves: xy = the way it runs (x, z), unit, z = waves per metre, w = phase at the camera, turns.
    pub waves: [[f32; 4]; crate::sea::MOST_WAVES],
    /// Per train: xyz = slope covariance (xx, zz, xz) of it and every shorter one, for a pixel too
    /// coarse for them all; w = its steepness (amplitude of its slope).
    pub wave_rest: [[f32; 4]; crate::sea::MOST_WAVES],
    /// The aerodromes' lights (`lights.rs`): x = the scene's brightness per cd/m² (`lights::gain`), y = the
    /// angle a pixel spans, radians, z and w = the distances over which they fade out (`lights::FADE`);
    /// then how much of each system is lit, in the order of `sim_core::lights::System::ALL`.
    pub lights: [[f32; 4]; 3],
    /// The sky (`sky.rs`): xyz = towards the sun (below the horizon too), w = how far the aerial perspective reaches, m.
    pub sky_sun: [f32; 4],
    /// xyz = towards the moon, w = the sun's light above the air (`SkyLights::scale`).
    pub sky_moon: [f32; 4],
    /// rgb = the moon's light above the air, w = 1 when it lights the world directly (`SkyLights::moonlit`).
    pub moon_light: [f32; 4],
    /// xy = the sun's azimuth (x, z), unit, z = the altitude of the ambient table's ground rows, w unused.
    pub sky_more: [f32; 4],
    /// Per airport (as `airports_a`): x = the side of the runway its apron is on (1 right, -1 left, 0 none,
    /// `sim_core::Detail::apron`), y = 1 when it is paved (otherwise grass, unpainted).
    pub airports_c: [[f32; 4]; 8],
    /// The road map (`roadmap.rs`, a region's roads): xy = its centre (x, z) relative to the camera, z = half
    /// its extent in metres, w = 1 when it is ready (0 for a generated world).
    pub road_map: [f32; 4],
    /// The sea's mirror (`reflection.rs`): x unused, y = 1 when the sea reads it, zw = one over the
    /// screen's size in pixels.
    pub mirror: [f32; 4],
    /// A real place's ground as the satellites saw it (`imagery.rs`), the near picture and the far: xy =
    /// its middle (x, z) relative to the camera, z = the metres its side spans, w = 1 when there is one.
    pub imagery: [[f32; 4]; crate::imagery::LEVELS],
    /// The vehicles' shadow map (`casters.rs`): xyz = its axes across the sunlight, right and up
    /// (unit); `caster_right.w` = 1 when there is one; `caster_up.w` = what the body's `ShadowShape`
    /// does: 0 nothing (it has none), 1 it takes over from the first tile far below the body,
    /// 2 it is the body's whole shadow (the body has no tile).
    pub caster_right: [f32; 4],
    pub caster_up: [f32; 4],
    /// Per tile of the map: xyz = its middle relative to the camera, w = half its side, metres (0: unused).
    pub caster_tiles: [[f32; 4]; crate::casters::TILES],
}

const _: () = assert!(size_of::<FrameUniforms>() == 259 * 16);

/// Per-draw data of terrain tiles, tree and house tiles and cloud slices (`struct Chunk`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ChunkUniforms {
    /// Terrain, trees, houses: xyz = origin of the tile relative to the camera (y = minus the camera
    /// altitude), w unused. Cloud slice: y = height of the slice relative to the camera, w = its level
    /// in the layer, 0 at the base and 1 at the top, x = height between slices, metres.
    pub offset: [f32; 4],
    /// Terrain: xy = origin (x, z) of the tile modulo 16384 m. Trees: x = 0 for the detailed models,
    /// 1 for the simple ones and 2 for the specks, y and z = the distances over the ground at which
    /// the first give way to the second and the second to the third. Unused by the others.
    pub wrap: [f32; 4],
}

const _: () = assert!(size_of::<ChunkUniforms>() == 32);

/// Per-draw data of objects, lights and puffs (`struct Object`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ObjectUniforms {
    /// Model space to camera-relative render space (columns).
    pub model: [[f32; 4]; 4],
    /// rgb multiplies the colour of the mesh. a = opacity (used by `objects_blended`); above 1.5 the
    /// object is a light: it shines with its own colour, unlit.
    pub tint: [f32; 4],
    /// Up to two planes the draw is cut along, in mesh space (`sim_core::Cut`): for each, the
    /// plane (normal, offset) and the plane that bounds where it cuts (normal, offset; zero: none).
    pub cuts: [[f32; 4]; 4],
    /// x, y: which side of each cut is kept: 1 inside, -1 outside, 0 no cut.
    pub keep: [f32; 4],
}

const _: () = assert!(size_of::<ObjectUniforms>() == 160);

impl ObjectUniforms {
    /// A whole draw.
    pub fn new(model: [[f32; 4]; 4], tint: [f32; 4]) -> ObjectUniforms {
        ObjectUniforms { model, tint, cuts: [[0.0; 4]; 4], keep: [0.0; 4] }
    }

    /// A draw cut along these planes.
    pub fn cut(mut self, cuts: &[Option<sim_core::Cut>; 2]) -> ObjectUniforms {
        for (i, cut) in cuts.iter().enumerate() {
            let Some(c) = cut else { continue };
            self.cuts[2 * i] = [c.normal.x as f32, c.normal.y as f32, c.normal.z as f32, c.offset as f32];
            // Bounded by nothing: a plane every point is in front of.
            let (n, o) = c.within.unwrap_or((sim_core::V3::ZERO, -1.0));
            self.cuts[2 * i + 1] = [n.x as f32, n.y as f32, n.z as f32, o as f32];
            self.keep[i] = if c.inside { 1.0 } else { -1.0 };
        }
        self
    }
}

pub struct Pipelines {
    /// Group 0 of every pipeline: the frame uniforms, the height map and the material textures.
    pub frame_layout: wgpu::BindGroupLayout,
    /// Group 1 of terrain, clouds, trees and houses: a `ChunkUniforms` at a dynamic offset.
    pub chunk_layout: wgpu::BindGroupLayout,
    /// Group 1 of objects and objects_blended: an `ObjectUniforms` at a dynamic offset.
    pub object_layout: wgpu::BindGroupLayout,
    /// Group 1 of the sea: its mirror.
    pub sea_layout: wgpu::BindGroupLayout,
    /// Group 2 of the terrain (and of its mirror): a real place's ground as the satellites saw it.
    pub imagery_layout: wgpu::BindGroupLayout,
    /// Group 2 of the trees and their shadows: their textures and pictures from far.
    pub flora_layout: wgpu::BindGroupLayout,
    /// Group 2 of the textured objects: a material and its pictures (`looks.rs`).
    pub look_layout: wgpu::BindGroupLayout,
    /// The sky, as one triangle over the whole screen: `draw(0..3, 0..1)`. No depth.
    pub sky: wgpu::RenderPipeline,
    pub terrain: wgpu::RenderPipeline,
    /// The sea's surface, as one big square at sea level: `draw(0..6, 0..1)`, group 1 =
    /// `sea_layout`. Draw it after everything solid: it is blended over what is under the sea ([`sea_blend`]).
    pub sea: wgpu::RenderPipeline,
    /// One horizontal slice of the cloud layer: `draw(0..6, 0..1)` per slice, farthest in height first. Blended.
    pub clouds: wgpu::RenderPipeline,
    pub objects: wgpu::RenderPipeline,
    /// Objects that are see-through (the blur of a spinning rotor): depth tested, not written, blended.
    pub objects_blended: wgpu::RenderPipeline,
    /// The game's meshes drawn with their materials' textures (`textured.wgsl`), group 2 =
    /// `look_layout`, their texture coordinates in vertex slot 1; and their glass, blended.
    pub textured: wgpu::RenderPipeline,
    pub glass: wgpu::RenderPipeline,
    /// The same by instances, as the houses (group 1 = `chunk_layout`, the instances in slot 1, the
    /// texture coordinates in slot 2): the vehicles parked on the aprons, and their glass.
    pub parked: wgpu::RenderPipeline,
    pub parked_glass: wgpu::RenderPipeline,
    /// Trees of one species, as their full or their simpler meshes (`ChunkUniforms::wrap` says which,
    /// and the species; see `flora.wgsl`), and as their pictures from far: a square per instance.
    /// Group 2 = `flora_layout`.
    pub flora: wgpu::RenderPipeline,
    pub impostors: wgpu::RenderPipeline,
    /// Tufts of grass (`grass.rs`): a tuft's mesh and its instances, as the trees;
    /// `ChunkUniforms::wrap.x` is how far they reach. Group 2 = `flora_layout`.
    pub grass: wgpu::RenderPipeline,
    /// The shadow of each tree on the ground: a square per instance, blended, group 2 = `flora_layout`.
    /// Draw after everything solid.
    pub tree_shadows: wgpu::RenderPipeline,
    /// Buildings and what is parked, and their shadows: the hull of each (`models::buildings`)
    /// flattened on the ground, blended. Draw the shadows after everything solid.
    pub houses: wgpu::RenderPipeline,
    pub house_shadows: wgpu::RenderPipeline,
    /// The buildings of a real place's maps: slot 0 = [`footprint_vertex_layout`], indexed, group 1 =
    /// `chunk_layout` with the tile's corner in `offset`.
    pub footprints: wgpu::RenderPipeline,
    /// The aerodromes' lights: slot 0 = `lights::instance_layout`, `draw(0..6, lights)`, group 1 =
    /// `chunk_layout` with the aerodrome's origin in `offset`. Added to the scene; draw after everything solid.
    pub lights: wgpu::RenderPipeline,
    /// The world mirrored in the sea (`reflection.rs`), when the quality draws it.
    pub mirror: Option<MirrorPipelines>,
    /// The vehicles as the sun sees them, depth only, into their shadow map (`casters.rs`): slot 0
    /// = `MeshBuilder::layout()`, group 1 = `object_layout` with the draw's matrix to the tile's clip
    /// space and its cuts (group 0 is not used). Both faces; biased by the slope.
    pub casters: wgpu::RenderPipeline,
}

/// What is drawn mirrored in the sea, one sample per pixel into the mirror's picture: the ground,
/// the trees (all solid), the buildings and the objects, with what is under the sea left out
/// (`terrain_coast_fragment`, `object_mirror_fragment`), and the aerodromes' lights as they are.
pub struct MirrorPipelines {
    pub terrain: wgpu::RenderPipeline,
    /// The ground of a tile the sea reaches, what is under it left out.
    pub coast: wgpu::RenderPipeline,
    /// The ground above the sea as it is, into the depth alone, drawn first with the scene's view:
    /// what is mirrored behind it is never seen, and is not shaded.
    pub land: wgpu::RenderPipeline,
    pub flora: wgpu::RenderPipeline,
    pub impostors: wgpu::RenderPipeline,
    pub houses: wgpu::RenderPipeline,
    pub footprints: wgpu::RenderPipeline,
    pub objects: wgpu::RenderPipeline,
    pub lights: wgpu::RenderPipeline,
}

/// How the sea's surface goes over what is under it (`sea_fragment`, `under_sea_color` in
/// shaders.wgsl). Under the sea the scene holds the light that comes up through the water and, in
/// alpha, how much foam lies on it; the surface's fragment is premultiplied, with in alpha how much
/// it hides of what is under it (Fresnel, its own foam). The surface then shows where the foam
/// below leaves room, `src · (1 - dst.a) + dst · (1 - src.a)`. Over the sky (alpha 0) it is the sea
/// beyond the ground, which hides everything.
pub fn sea_blend() -> wgpu::BlendState {
    let over = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::OneMinusDstAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add };
    wgpu::BlendState { color: over, alpha: wgpu::BlendComponent::REPLACE }
}

/// Terrain vertices: position, normal, colour and cover (farmland, water, forest), 12 floats.
/// Heights as the half floats an `R16Float` texture holds. Values are truncated, which is at most
/// 2 m at 3000 m; anything below the smallest normal half float becomes 0.
pub fn half_floats(values: &[f32]) -> Vec<u16> {
    values
        .iter()
        .map(|v| {
            let bits = v.to_bits();
            let sign = ((bits >> 16) & 0x8000) as u16;
            let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
            if exponent <= 0 {
                sign
            } else if exponent >= 31 {
                sign | 0x7c00
            } else {
                sign | ((exponent as u16) << 10) | ((bits & 0x7f_ffff) >> 13) as u16
            }
        })
        .collect()
}

/// The sampler that goes with the height map at group 0, binding 2, and with the road map.
pub fn height_sampler(gpu: &engine::Gpu) -> wgpu::Sampler {
    gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("height map"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, ..Default::default() })
}

pub fn terrain_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3];
    wgpu::VertexBufferLayout { array_stride: 48, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRS }
}

/// A vertex of the buildings of a real place's maps (`footprints.rs`): position, normal, colour,
/// what surface it is, how high above the building's foot and the building's own number, and its
/// facade (style, storey, column, ground floor).
pub fn footprint_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Float32x4];
    wgpu::VertexBufferLayout { array_stride: (crate::footprints::FLOATS * 4) as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRS }
}

/// One tree or house, 8 floats: place (xyz in the tile, w = size) and four more that trees and
/// houses read differently (see `placement`).
pub fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4];
    wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Instance, attributes: &ATTRS }
}

/// One of the sky's tables (`sky.rs`), read by vertex shaders too (the lights' haze).
fn sky_entry(binding: u32, dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: dimension, multisampled: false },
        count: None,
    }
}

fn uniform_entry(visibility: wgpu::ShaderStages, dynamic: bool, size: usize) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: wgpu::BufferSize::new(size as u64) },
        count: None,
    }
}

/// Group 2 of the trees (`flora_layout`): six 2D arrays and two samplers, bindings 2 to 9.
fn flora_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
    let mut entries: Vec<wgpu::BindGroupLayoutEntry> = (2..8)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false },
            count: None,
        })
        .collect();
    entries.extend((8..10).map(|binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }));
    entries
}

/// The shaders as a quality draws them: the full one is the files as they are.
pub fn shader_source(quality: &Quality) -> String {
    let mut source = concat!(include_str!("sky_common.wgsl"), include_str!("shaders.wgsl"), include_str!("textured.wgsl"), include_str!("scenery.wgsl"), include_str!("lights.wgsl")).to_string();
    source.push_str(&crate::species::wgsl());
    source.push_str(include_str!("flora_surface.wgsl"));
    source.push_str(include_str!("flora.wgsl"));
    let mut set = |line: &str, value: String| {
        assert!(source.contains(line), "shaders.wgsl has no line `{line}`");
        source = source.replace(line, &value);
    };
    if *quality != Quality::HIGH {
        set("const RELIEF_STEPS: i32 = 22;", format!("const RELIEF_STEPS: i32 = {};", quality.relief_steps));
        set("const RELIEF_GROWTH: f32 = 1.3;", format!("const RELIEF_GROWTH: f32 = {:?};", quality.relief_growth()));
        set("const CLOUD_OPACITY: f32 = 0.42;", format!("const CLOUD_OPACITY: f32 = {:?};", quality.cloud_opacity()));
        set("const TEXTURES: bool = true;", format!("const TEXTURES: bool = {};", quality.texture_size > 0));
        set("const TEX_RANGE: f32 = 4000.0;", format!("const TEX_RANGE: f32 = {:?};", quality.texture_range.max(1.0)));
        set("const TEX_RELIEF: f32 = 1500.0;", format!("const TEX_RELIEF: f32 = {:?};", quality.texture_relief.max(0.0)));
        set("const TEX_SCATTER: bool = true;", format!("const TEX_SCATTER: bool = {};", quality.texture_scatter));
        set("const SEA_DETAIL: bool = true;", format!("const SEA_DETAIL: bool = {};", quality.sea_detail));
        // The halos of the lights in mist go with the bloom.
        set("const LIGHT_HALOS: bool = true;", format!("const LIGHT_HALOS: bool = {};", quality.bloom > 0.0));
    }
    source
}

impl Pipelines {
    pub fn new(gpu: &Gpu, quality: &Quality) -> Pipelines {
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries });
        let frame_layout = layout(
            "frame",
            &[
                uniform_entry(wgpu::ShaderStages::VERTEX_FRAGMENT, false, size_of::<FrameUniforms>()),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                texture::array_layout_entry(3),
                texture::array_layout_entry(4),
                texture::sampler_layout_entry(5),
                sky_entry(6, wgpu::TextureViewDimension::D2),
                sky_entry(7, wgpu::TextureViewDimension::D2),
                sky_entry(8, wgpu::TextureViewDimension::D3),
                sky_entry(9, wgpu::TextureViewDimension::D3),
                sky_entry(10, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry { binding: 11, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 12,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 13,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 14, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
            ],
        );
        // Its own group, not the frame's: the frame's textures and the fire's drawing's already
        // take the 16 a fragment shader may read (WebGPU's least).
        let imagery_layout = layout(
            "ground imagery",
            &[0, 1].map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false },
                count: None,
            }),
        );
        let chunk_layout = layout("chunk", &[uniform_entry(wgpu::ShaderStages::VERTEX, true, size_of::<ChunkUniforms>())]);
        let object_layout = layout("object", &[uniform_entry(wgpu::ShaderStages::VERTEX_FRAGMENT, true, size_of::<ObjectUniforms>())]);
        let sea_layout = layout(
            "sea",
            &[wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        );
        let flora_layout = layout("flora", &flora_entries());
        let look_layout = crate::looks::layout(gpu);
        Pipelines::with_layouts(&gpu.pipeline_maker(gpu.sample_count), quality, [frame_layout, chunk_layout, object_layout, sea_layout, imagery_layout, flora_layout, look_layout])
    }

    /// Makes the pipelines again, for another quality or another `Gpu::sample_count`. Bind groups made
    /// with the layouts stay good.
    pub fn rebuild(&mut self, gpu: &Gpu, quality: &Quality) {
        *self = Pipelines::with_layouts(&gpu.pipeline_maker(gpu.sample_count), quality, self.layouts());
    }

    /// The layouts, which every quality shares.
    pub(crate) fn layouts(&self) -> [wgpu::BindGroupLayout; 7] {
        [self.frame_layout.clone(), self.chunk_layout.clone(), self.object_layout.clone(), self.sea_layout.clone(), self.imagery_layout.clone(), self.flora_layout.clone(), self.look_layout.clone()]
    }

    /// The pipelines of `quality` with these layouts, for the samples of `gpu`, which may be another
    /// thread's (`WorldRenderer::set_quality` makes them in the background).
    pub(crate) fn with_layouts(gpu: &PipelineMaker, quality: &Quality, [frame_layout, chunk_layout, object_layout, sea_layout, imagery_layout, flora_layout, look_layout]: [wgpu::BindGroupLayout; 7]) -> Pipelines {
        // Every loop in shaders.wgsl and scenery.wgsl counts up to a constant or to a count from the frame uniforms.
        let shader = gpu.shader_with_ending_loops("world", &shader_source(quality));
        let make = |label, vertex, fragment, buffers: &[wgpu::VertexBufferLayout], per_draw: Option<&wgpu::BindGroupLayout>, depth, blend| {
            let groups: Vec<&wgpu::BindGroupLayout> = std::iter::once(&frame_layout).chain(per_draw).collect();
            gpu.pipeline(&PipelineDesc { label, shader: &shader, vertex, fragment, buffers, bind_groups: &groups, depth, blend })
        };
        let mesh = [MeshBuilder::layout()];
        let instanced = [MeshBuilder::layout(), instance_layout()];
        let sky = make("sky", "sky_vertex", "sky_fragment", &[], None, Depth::Off, false);
        let ground = [&frame_layout, &chunk_layout, &imagery_layout];
        let terrain = gpu.pipeline(&PipelineDesc { label: "terrain", shader: &shader, vertex: "terrain_vertex", fragment: "terrain_fragment", buffers: &[terrain_vertex_layout()], bind_groups: &ground, depth: Depth::Write, blend: false });
        let sea = gpu.pipeline_blended(&PipelineDesc { label: "sea", shader: &shader, vertex: "sea_vertex", fragment: "sea_fragment", buffers: &[], bind_groups: &[&frame_layout, &sea_layout], depth: Depth::Write, blend: true }, Some(sea_blend()));
        let clouds = make("clouds", "cloud_vertex", "cloud_fragment", &[], Some(&chunk_layout), Depth::Read, true);
        let objects = make("objects", "object_vertex", "object_fragment", &mesh, Some(&object_layout), Depth::Write, false);
        let objects_blended = make("objects blended", "object_vertex", "object_fragment", &mesh, Some(&object_layout), Depth::Read, true);
        let surfaced = [MeshBuilder::layout(), crate::looks::uv_layout()];
        let looked = [&frame_layout, &object_layout, &look_layout];
        let textured = gpu.pipeline(&PipelineDesc { label: "textured objects", shader: &shader, vertex: "textured_vertex", fragment: "textured_fragment", buffers: &surfaced, bind_groups: &looked, depth: Depth::Write, blend: false });
        let glass = gpu.pipeline(&PipelineDesc { label: "textured objects' glass", shader: &shader, vertex: "textured_vertex", fragment: "glass_fragment", buffers: &surfaced, bind_groups: &looked, depth: Depth::Read, blend: true });
        let placed = [MeshBuilder::layout(), instance_layout(), crate::looks::uv_layout()];
        let looked_chunks = [&frame_layout, &chunk_layout, &look_layout];
        let parked = gpu.pipeline(&PipelineDesc { label: "parked vehicles", shader: &shader, vertex: "parked_vertex", fragment: "parked_fragment", buffers: &placed, bind_groups: &looked_chunks, depth: Depth::Write, blend: false });
        let parked_glass = gpu.pipeline(&PipelineDesc { label: "parked vehicles' glass", shader: &shader, vertex: "parked_vertex", fragment: "parked_glass_fragment", buffers: &placed, bind_groups: &looked_chunks, depth: Depth::Read, blend: true });
        let trees = [&frame_layout, &chunk_layout, &flora_layout];
        let flora_buffers = [crate::vegetation::FloraMesh::layout(), instance_layout()];
        let flora = gpu.pipeline(&PipelineDesc {
            label: "trees",
            shader: &shader,
            vertex: "flora_vertex",
            fragment: "flora_fragment",
            buffers: &flora_buffers,
            bind_groups: &trees,
            depth: Depth::Write,
            blend: false,
        });
        let impostors = gpu.pipeline(&PipelineDesc {
            label: "trees' pictures",
            shader: &shader,
            vertex: "impostor_vertex",
            fragment: "impostor_fragment",
            buffers: &instanced,
            bind_groups: &trees,
            depth: Depth::Write,
            blend: false,
        });
        let grass = gpu.pipeline(&PipelineDesc {
            label: "grass",
            shader: &shader,
            vertex: "grass_vertex",
            fragment: "grass_fragment",
            buffers: &flora_buffers,
            bind_groups: &trees,
            depth: Depth::Write,
            blend: false,
        });
        let tree_shadows = gpu.pipeline(&PipelineDesc {
            label: "tree shadows",
            shader: &shader,
            vertex: "tree_shadow_vertex",
            fragment: "tree_shadow_fragment",
            buffers: &instanced,
            bind_groups: &trees,
            depth: Depth::Read,
            blend: true,
        });
        let houses = make("houses", "building_vertex", "building_fragment", &instanced, Some(&chunk_layout), Depth::Write, false);
        let footprints = make("buildings of the maps", "footprint_vertex", "building_fragment", &[footprint_vertex_layout()], Some(&chunk_layout), Depth::Write, false);
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let keep = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let lights = gpu.pipeline_blended(
            &PipelineDesc { label: "aerodrome lights", shader: &shader, vertex: "aerodrome_light_vertex", fragment: "aerodrome_light_fragment", buffers: &[crate::lights::instance_layout()], bind_groups: &[&frame_layout, &chunk_layout], depth: Depth::Read, blend: true },
            Some(wgpu::BlendState { color: add, alpha: keep }),
        );
        let house_shadows = make("house shadows", "building_shadow_vertex", "house_shadow_fragment", &instanced, Some(&chunk_layout), Depth::Read, true);
        let mirror = (quality.reflection > 0).then(|| {
            let make = |label, vertex, fragment, buffers: &[wgpu::VertexBufferLayout], per_draw: &wgpu::BindGroupLayout, depth, blend: Option<wgpu::BlendState>| {
                let groups: &[&wgpu::BindGroupLayout] = if vertex == "terrain_vertex" {
                    &ground
                } else if fragment == "flora_mirror_fragment" || fragment == "impostor_mirror_fragment" {
                    &trees
                } else {
                    &[&frame_layout, per_draw]
                };
                gpu.pipeline_with_samples(&PipelineDesc { label, shader: &shader, vertex, fragment, buffers, bind_groups: groups, depth, blend: false }, blend, 1)
            };
            let unseen = wgpu::BlendState { color: keep, alpha: keep };
            MirrorPipelines {
                terrain: make("mirrored terrain", "terrain_vertex", "terrain_mirror_fragment", &[terrain_vertex_layout()], &chunk_layout, Depth::Write, None),
                coast: make("mirrored coast", "terrain_vertex", "terrain_coast_fragment", &[terrain_vertex_layout()], &chunk_layout, Depth::Write, None),
                land: make("land before the mirror", "terrain_vertex", "terrain_depth_fragment", &[terrain_vertex_layout()], &chunk_layout, Depth::Write, Some(unseen)),
                flora: make("mirrored trees", "flora_vertex", "flora_mirror_fragment", &flora_buffers, &chunk_layout, Depth::Write, None),
                impostors: make("mirrored trees' pictures", "impostor_vertex", "impostor_mirror_fragment", &instanced, &chunk_layout, Depth::Write, None),
                houses: make("mirrored houses", "building_vertex", "building_mirror_fragment", &instanced, &chunk_layout, Depth::Write, None),
                footprints: make("mirrored buildings of the maps", "footprint_vertex", "building_mirror_fragment", &[footprint_vertex_layout()], &chunk_layout, Depth::Write, None),
                objects: make("mirrored objects", "object_vertex", "object_mirror_fragment", &mesh, &object_layout, Depth::Write, None),
                lights: make("mirrored lights", "aerodrome_light_vertex", "aerodrome_light_fragment", &[crate::lights::instance_layout()], &chunk_layout, Depth::Read, Some(wgpu::BlendState { color: add, alpha: keep })),
            }
        });

        let caster_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("vehicles' shadows"), bind_group_layouts: &[None, Some(&object_layout)], immediate_size: 0 });
        let casters = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vehicles' shadows"),
            layout: Some(&caster_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("caster_vertex"), buffers: &[Some(MeshBuilder::layout())], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("caster_fragment"), targets: &[], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::casters::FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                // Pushed away from the sun by a little and more where a face is slanted to it:
                // a face does not shade itself in stripes.
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        Pipelines { frame_layout, chunk_layout, object_layout, sea_layout, imagery_layout, flora_layout, look_layout, sky, terrain, sea, clouds, objects, objects_blended, textured, glass, parked, parked_glass, flora, impostors, grass, tree_shadows, houses, house_shadows, footprints, lights, mirror, casters }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::wgpu::naga;

    /// The shaders of every quality parse and validate, without a GPU: naga is what wgpu checks them with.
    #[test]
    fn shaders_are_valid_wgsl() {
        for quality in [Quality::HIGH, Quality::MEDIUM, Quality::LOW] {
            let source = shader_source(&quality);
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        }
    }
}
