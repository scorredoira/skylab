//! `WorldRenderer`: chooses what of the world to draw each frame, uploads it, and draws it.
//! The contract (what it is given, what it gives) is in the crate's documentation and README.

use crate::buildings::{self, Building};
use crate::casters::{self, CasterMap, Tile};
use crate::effect_pipelines::{EffectPipelines, Slot, Target};
use crate::effects::{self, Effects};
use crate::clouds::{self, CloudPipelines, CloudUniforms, Clouds, Seen};
use crate::fire::{self, Fire};
use crate::footprints::{self, Footprints};
use crate::grass;
use crate::heightmap::HeightMap;
use crate::imagery::GroundImagery;
use crate::impostors::Flora;
use crate::parcels::{self, Parcels};
use crate::lights::{self, AerodromeLights};
use crate::materials::{self, Materials, ReadMaterials};
use crate::models;
use crate::particles::{self, EffectUniforms, GpuParticle, ParticlePass};
use crate::pipelines::{self, ChunkUniforms, FrameUniforms, MirrorPipelines, ObjectUniforms, Pipelines};
use crate::reflection::Reflection;
use crate::roadmap::RoadMap;
use crate::scatter::{self, Scatter};
use crate::sea::Sea;
use crate::sky::{self, Air, SkyLights, SkyTables};
use crate::species;
use crate::terrain::{self, TerrainTiles};
use crate::vegetation::{self, Detail};
use crate::looks::{self, Look, LookId, TexturedMesh};
use crate::{from_render, sun_elevation, to_render, CasterMesh, MeshId, Quality, Scene};
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Camera, Gpu, Mesh, MeshBuilder, UniformArena};
use sim_core::scenery as placement;
use sim_core::{act, smoothstep, Airport, Cut, Imagery, PieceKind, Pieces, Road, Terrain, Town};
use glam::{DVec3, Mat4, Vec3};
use std::collections::HashMap;
use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;

/// Airports with windsock and floodlight masts at once.
const AIRPORTS_WITH_SCENERY: usize = 3;
/// Distance up to which trees are drawn in detail in a picture 1000 pixels tall, metres: the
/// qualities' distances at which a tree gives up its full mesh and its simpler one
/// (`Quality::tree_detail`, `tree_pictures`) are times this. It grows with the height of the
/// picture, up to `MOST_DETAIL` times.
const TREE_DETAIL: f32 = 220.0;
const MOST_DETAIL: f32 = 1.35;
/// Each tree changes detail at up to this much more than the distance, so no line shows (`flora.wgsl`),
/// over a band of `LOD_BAND` of it in which it dissolves from one level into the next.
const DETAIL_SPREAD: f32 = 1.18;
const LOD_BAND: f32 = 0.12;
/// Distances beyond which trees and buildings cast no shadow (the same constants in scenery.wgsl).
const TREE_SHADOW_RANGE: f64 = 1000.0;
const HOUSE_SHADOW_RANGE: f64 = 5000.0;
/// Side of the tiles the simpler trees and the pictures come from, metres. Small enough that few tiles
/// straddle the change from one to the other, which draws their trees twice (`flora_vertex` gives
/// each its share of the pixels of each): with 512 m tiles that ring held half the trees in view.
const FAR_TILE: f64 = 256.0;
/// Per-draw blocks besides the terrain's: each species of tree at each of its three levels, the
/// grass, the houses, the cloud slices of the full quality, the aerodromes' lights, the vehicles'
/// lamps and the tiles of the buildings of the maps.
const OTHER_CHUNKS: usize = 3 + 3 * species::COUNT + Quality::HIGH.cloud_slices + MOST_AIRPORTS + footprints::MOST_TILES;
/// The airports near the camera the ground is painted for and whose lights are drawn (`airports_a` in shaders.wgsl).
const MOST_AIRPORTS: usize = 8;
/// Towns the ground's shader paints the streets of (`FrameUniforms::towns`).
const MOST_TOWNS: usize = 16;
/// A vehicle's lamp lights the ground when its beam is this strong (cd) and no wider (radians).
const BEAM_LIGHTS: (f32, f32) = (5000.0, 0.35);
/// Per-object blocks of the world's own: per airport a pole, a sock and two masts; per wreck its pieces.
const WORLD_OBJECTS: usize = AIRPORTS_WITH_SCENERY * 4 + sim_core::World::MOST_WRECKS * 4;

#[derive(Clone, Copy)]
enum Pipeline {
    Terrain,
    Sea,
    Clouds,
    Objects,
    ObjectsBlended,
    Textured,
    Glass,
    Parked,
    ParkedGlass,
    Flora,
    Impostors,
    Grass,
    TreeShadows,
    Houses,
    HouseShadows,
    Footprints,
    Lights,
}

/// Where on the screen a box from `low` to `high` (camera-relative, render axes) may be seen, as a
/// rectangle in normalised device coordinates (x0, y0, x1, y1) for `view_proj`, or none when it is
/// all behind the eye. What is behind is cut off at a metre in front of it.
fn on_screen(view_proj: Mat4, low: Vec3, high: Vec3) -> Option<[f32; 4]> {
    const NEAR: f32 = 1.0;
    let corners: [glam::Vec4; 8] = std::array::from_fn(|i| view_proj * Vec3::new([low.x, high.x][i & 1], [low.y, high.y][(i >> 1) & 1], [low.z, high.z][i >> 2]).extend(1.0));
    let mut rect: Option<[f32; 4]> = None;
    let mut add = |p: glam::Vec4| {
        let (x, y) = (p.x / p.w, p.y / p.w);
        rect = Some(rect.map_or([x, y, x, y], |r| [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)]));
    };
    for (i, a) in corners.iter().enumerate() {
        if a.w >= NEAR {
            add(*a);
        }
        // Where the edges to its neighbours cross the plane a metre ahead of the eye.
        for bit in [1, 2, 4] {
            let b = corners[i ^ bit];
            if i & bit == 0 && (a.w - NEAR) * (b.w - NEAR) < 0.0 {
                add(a.lerp(b, (NEAR - a.w) / (b.w - a.w)));
            }
        }
    }
    rect
}

/// The smallest rectangle round both, the first possibly none.
fn union(a: Option<[f32; 4]>, b: [f32; 4]) -> Option<[f32; 4]> {
    Some(a.map_or(b, |a| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]))
}

/// Whether two rectangles share some of the screen.
fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// What `WorldRenderer::run` replays the commands for.
#[derive(Clone, Copy)]
enum Replay<'p> {
    Scene,
    /// The ground above the sea into the depth, before the sea's mirror.
    Land,
    /// The sea's mirror, with these pipelines.
    Mirror(&'p MirrorPipelines),
}

/// The pipeline of the scene for each step.
fn scene_pipeline(p: &Pipelines, which: Pipeline) -> &wgpu::RenderPipeline {
    match which {
        Pipeline::Terrain => &p.terrain,
        Pipeline::Sea => &p.sea,
        Pipeline::Clouds => &p.clouds,
        Pipeline::Objects => &p.objects,
        Pipeline::ObjectsBlended => &p.objects_blended,
        Pipeline::Textured => &p.textured,
        Pipeline::Glass => &p.glass,
        Pipeline::Parked => &p.parked,
        Pipeline::ParkedGlass => &p.parked_glass,
        Pipeline::Flora => &p.flora,
        Pipeline::Impostors => &p.impostors,
        Pipeline::Grass => &p.grass,
        Pipeline::TreeShadows => &p.tree_shadows,
        Pipeline::Houses => &p.houses,
        Pipeline::HouseShadows => &p.house_shadows,
        Pipeline::Footprints => &p.footprints,
        Pipeline::Lights => &p.lights,
    }
}

/// The pipeline of the sea's mirror for each step of the scene before the sea: none for the
/// shadows, which the mirror leaves out. A tile of ground the sea reaches takes `coast` instead.
fn mirrored(p: &MirrorPipelines, which: Pipeline) -> Option<&wgpu::RenderPipeline> {
    match which {
        Pipeline::Terrain => Some(&p.terrain),
        Pipeline::Flora => Some(&p.flora),
        Pipeline::Impostors => Some(&p.impostors),
        // The parked vehicles are mirrored as the houses are, with their vertices' colours.
        Pipeline::Houses | Pipeline::Parked => Some(&p.houses),
        Pipeline::Footprints => Some(&p.footprints),
        // A textured mesh is mirrored with the colours its vertices keep (`Model::mesh`).
        Pipeline::Objects | Pipeline::Textured => Some(&p.objects),
        Pipeline::Lights => Some(&p.lights),
        Pipeline::Sea | Pipeline::Clouds | Pipeline::ObjectsBlended | Pipeline::Glass | Pipeline::ParkedGlass | Pipeline::Grass | Pipeline::TreeShadows | Pipeline::HouseShadows => None,
    }
}

/// The meshes of the world, and those the game adds.
#[derive(Clone, Copy)]
enum Model {
    Pole,
    Sock,
    Light,
    /// A building, by its kind in `sim_core::scenery::houses`, and the hull its shadow is drawn with.
    Building(usize),
    Hull(usize),
    /// The square a tree's shadow and its picture from far are drawn on (`vegetation::square`).
    Square,
    /// A tree, by its place in `Species::ALL`, near or far (`Flora::mesh`; not in `meshes`).
    Tree(usize, Detail),
    /// A tuft of grass (`Flora::tuft`; not in `meshes`).
    Tuft,
    /// A mesh the game added (`WorldRenderer::add_mesh`).
    Added(usize),
}

impl Model {
    /// Place of the mesh in `WorldRenderer::meshes`; a tree's past their end (`Flora::mesh` has them).
    fn index(self) -> usize {
        let first = 4;
        match self {
            Model::Pole => 0,
            Model::Sock => 1,
            Model::Light => 2,
            Model::Square => 3,
            Model::Building(k) => first + 2 * k,
            Model::Hull(k) => first + 1 + 2 * k,
            Model::Added(k) => first + 2 * placement::HOUSE_KINDS + k,
            Model::Tree(k, detail) => usize::MAX - 2 * k - usize::from(detail == Detail::Far),
            Model::Tuft => usize::MAX - 2 * species::COUNT,
        }
    }
}

/// One step of drawing the frame, recorded by `prepare` and replayed by `draw`. `uniforms` is the
/// dynamic offset of the draw's block in the arena of its kind.
enum Command {
    Use(Pipeline),
    /// The sea's surface, with its mirror.
    Sea,
    /// The lights of an aerodrome (`lights.rs`), `count` instances of `instances`.
    Lights { instances: wgpu::Buffer, count: u32, uniforms: u32 },
    /// A tile of ground; `mirrored` when some of it stands above the sea and its image in it may be
    /// seen, `wet` when some of it is under the sea.
    Terrain { vertices: wgpu::Buffer, uniforms: u32, mirrored: bool, wet: bool },
    /// A run of instances of a batch of trees or houses.
    Instanced { model: Model, instances: wgpu::Buffer, range: Range<u32>, uniforms: u32 },
    Object { model: Model, first: u32, count: u32, uniforms: u32 },
    /// A run of one material of a textured mesh (`looks.rs`): its look and material.
    Textured { model: Model, first: u32, count: u32, uniforms: u32, look: usize, material: usize },
    /// A run of one material of the textured parked vehicle (`set_parked_look`), by instances.
    Parked { model: Model, first: u32, count: u32, instances: wgpu::Buffer, range: Range<u32>, uniforms: u32, look: usize, material: usize },
    /// A tile of the buildings of the maps (`footprints.rs`).
    Footprints { key: (i32, i32), uniforms: u32 },
    CloudSlice { uniforms: u32 },
    /// The volumetric clouds (`clouds.rs`), in the second pass.
    Clouds,
    /// The effects of the wrecks (`particles.rs`), at the start of the second pass.
    Effects,
}

/// The pieces of the wrecks of one shape (`sim_core::Pieces`): the mesh, and the range of it
/// each piece is, in the order of `PieceKind::ALL`.
struct WreckShape {
    shape: &'static Pieces,
    mesh: MeshId,
    pieces: [Range<u32>; 4],
}

/// A draw into the vehicles' shadow map: which tile, which mesh (the world's, or the index of a
/// game's mesh in this frame's `Scene::casters`), the range, the uniforms.
struct CasterCommand {
    tile: usize,
    mesh: CasterSource,
    first: u32,
    count: u32,
    uniforms: u32,
}

#[derive(Clone, Copy)]
enum CasterSource {
    World(usize),
    /// The caster and the draw of it in `Scene::casters`.
    Game(usize, usize),
}

pub struct WorldRenderer {
    /// Height of the cloud base above sea level, and the depth of the layer, metres.
    pub cloud_altitude: f64,
    pub cloud_thickness: f64,
    /// 0 = clear sky, 1 = overcast.
    pub cloud_cover: f64,
    /// Storm clouds (cumulonimbus) about, 0 to 3: each stands at a place of its own (`STORMS`).
    pub cloud_storms: u32,

    world: Arc<Terrain>,
    quality: Quality,
    pipelines: Pipelines,
    /// The `Gpu::sample_count` the pipelines were made for.
    samples: u32,
    frame_buffer: wgpu::Buffer,
    frame_bind: wgpu::BindGroup,
    /// The sea's mirror (`reflection.rs`), and group 0 for drawing it.
    reflection: Reflection,
    mirror_bind: wgpu::BindGroup,
    /// The sky's tables: the light of the sky and the haze for the weather, the hour and the eye.
    sky: SkyTables,
    /// The vehicles' shadow map (`casters.rs`), and this frame's draws into it.
    caster_map: CasterMap,
    caster_draws: Vec<CasterCommand>,
    /// The photographs of ground and buildings: blank until the first frame, then what the quality asks for.
    materials: Materials,
    chunks: UniformArena,
    objects: UniformArena,
    /// Blocks `objects` has room for.
    objects_room: usize,
    terrain: TerrainTiles,
    /// The trees near the camera, in small tiles, drawn as their full meshes, and all of them in big
    /// tiles, drawn as their simpler meshes and their pictures. Both come from `placement::trees`
    /// (by species, `species::assign`), so they are the same trees.
    near_trees: Scatter,
    far_trees: Scatter,
    /// The parcels their trees were made with (`parcels::trees`).
    trees_on: Option<Arc<Vec<Parcels>>>,
    houses: Scatter,
    /// Tufts of grass round the camera when it is low (`grass.rs`).
    grass: Scatter,
    /// The trees' meshes, textures and pictures from far.
    flora: Flora,
    /// The buildings of a real place's maps.
    footprints: Footprints,
    height_map: HeightMap,
    /// A region's roads round the camera (a generated world's go in the frame's uniforms).
    road_map: RoadMap,
    /// A real place's ground as the satellites saw it, when the game has given it.
    imagery: GroundImagery,
    meshes: Vec<Mesh>,
    /// The meshes added with a look, by their place in `meshes`; the looks; their sampler.
    textured: HashMap<usize, TexturedMesh>,
    looks: Vec<Look>,
    look_sampler: wgpu::Sampler,
    /// This frame's draws of textured meshes, solid and glass, until their place among the commands.
    textured_draws: Vec<Command>,
    glass_draws: Vec<Command>,
    /// The parked vehicle drawn with its look instead of `Model::Building(PARKED)`, if the game gave it.
    parked_look: Option<MeshId>,
    /// This frame's draws of its glass, for after the world.
    parked_glass: Vec<Command>,
    wreck_shapes: Vec<WreckShape>,
    near_airports: Vec<Airport>,
    /// The lights of the aerodromes in `near_airports`, and this frame's floodlights as lights of the ground.
    lights: AerodromeLights,
    floods: Vec<effects::FireLight>,
    near_roads: Vec<Road>,
    near_towns: Vec<Town>,
    /// Frames until the airports and roads near the camera are looked up again.
    airport_refresh: u32,
    commands: Vec<Command>,
    /// Where in `commands` the second pass begins (`Gpu::translucent_pass`): only when there are effects to draw.
    split: Option<usize>,
    /// What the wrecks look like this frame, and its particles as the GPU takes them.
    effects: Effects,
    effect_particles: Vec<GpuParticle>,
    effect_pass: ParticlePass,
    /// The pipelines of the effects and their noise, made in the background before any crash.
    effect_pipelines: EffectPipelines,
    /// The fire of the newest crash near the camera as a simulated gas, and how many of the
    /// particles (the farthest) are drawn before it.
    fire: Option<Fire>,
    behind_fire: u32,
    /// The volumetric clouds, once a quality that draws them has been drawn.
    clouds: Option<Clouds>,
    /// A quality asked for and being made ready in the background (`set_quality`).
    next_quality: Option<NextQuality>,
    /// How much of the hour of the gold the last frame drawn was in (`clouds::golden_hour`).
    golden: f32,
}

/// What a quality draws with that takes long to make, made on other threads while the frames go
/// on with the quality there was: the world's pipelines and the clouds' (the driver compiles their
/// big shaders: 1 to 5 s on an Apple M5 Pro the first time a build runs one, some 0.1 s after),
/// and the photographs of the ground if they differ (0.1 to 0.25 s to read).
struct NextQuality {
    quality: Quality,
    /// The `Gpu::sample_count` it draws with.
    samples: u32,
    pipelines: Slot<(Pipelines, Option<CloudPipelines>)>,
    materials: Option<Slot<Result<ReadMaterials, String>>>,
    /// Everything was made, as of the last `prepare`.
    made: bool,
}

/// Where the storm clouds stand: north and east in the world, the radius of the base and the
/// altitude of the top, m. The first is 16 km ahead of where a flight from the home runway starts.
const STORMS: [(f64, f64, f64, f64); 3] = [(-12500.0, 17200.0, 4500.0, 10000.0), (26000.0, -9000.0, 5500.0, 12000.0), (-32000.0, 41000.0, 6000.0, 11500.0)];

/// Group 0 of every pipeline: the frame uniforms, the height map, the material textures and the sky's tables.
fn frame_bind(gpu: &Gpu, pipelines: &Pipelines, frame_buffer: &wgpu::Buffer, (height_map, road_map): (&HeightMap, &RoadMap), materials: &Materials, (sky, casters): (&SkyTables, &CasterMap)) -> wgpu::BindGroup {
    let height_view = height_map.texture.create_view(&Default::default());
    let road_view = road_map.texture.create_view(&Default::default());
    let height_sampler = pipelines::height_sampler(gpu);
    let mut entries = vec![
        wgpu::BindGroupEntry { binding: 0, resource: frame_buffer.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&height_view) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&height_sampler) },
        materials.colour.entry(3),
        materials.detail.entry(4),
        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&materials.sampler) },
        wgpu::BindGroupEntry { binding: 12, resource: wgpu::BindingResource::TextureView(&road_view) },
        wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::TextureView(&casters.view) },
        wgpu::BindGroupEntry { binding: 14, resource: wgpu::BindingResource::Sampler(&casters.sampler) },
    ];
    entries.extend(sky.entries());
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("frame"), layout: &pipelines.frame_layout, entries: &entries })
}

/// The trees near the camera (in small tiles, in detail), all of them (in big tiles, as simple
/// shapes), the buildings and the tufts of grass, of a terrain, for a quality.
fn scatters(terrain: &Arc<Terrain>, quality: &Quality) -> (Scatter, Scatter, Scatter, Scatter) {
    (
        Scatter::new(terrain.clone(), 128.0, (TREE_DETAIL * MOST_DETAIL * DETAIL_SPREAD) as f64, 600, species::COUNT, tree_build(None)),
        Scatter::new(terrain.clone(), FAR_TILE, quality.tree_range, 1200, species::COUNT, tree_build(None)),
        Scatter::new(terrain.clone(), 512.0, 7000.0, 4000, placement::HOUSE_KINDS, Arc::new(placement::houses)),
        Scatter::new(terrain.clone(), grass::TILE, quality.grass_range, 300, 1, grass_build(None)),
    )
}

/// How the trees of a tile are made, by species: `sim_core::scenery`'s, and those a real place's parcels show.
fn tree_build(parcels: Option<Arc<Vec<Parcels>>>) -> scatter::Build {
    Arc::new(move |terrain: &Terrain, key, size| {
        let mut all = species::assign(terrain, key, size, &placement::trees(terrain, key, size));
        if let Some(parcels) = &parcels {
            parcels::trees(parcels, terrain, key, size, &mut all);
        }
        all
    })
}

/// How the tufts of a tile are made: on the parcels of a real place's picture where it has them.
fn grass_build(parcels: Option<Arc<Vec<Parcels>>>) -> scatter::Build {
    Arc::new(move |terrain: &Terrain, key, size| grass::tufts(terrain, parcels.as_deref().map(Vec::as_slice), key, size))
}

fn matrix(c0: Vec3, c1: Vec3, c2: Vec3, translation: Vec3) -> Mat4 {
    Mat4::from_cols(c0.extend(0.0), c1.extend(0.0), c2.extend(0.0), translation.extend(1.0))
}

fn with(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

/// A coordinate modulo 16384 m, so that patterns drawn from it keep their precision far from the origin.
fn wrapped(v: f64) -> f32 {
    let wrap = 16384.0;
    (v - (v / wrap).floor() * wrap) as f32
}

/// The runs of consecutive instances of a batch whose tiles `visible` (given the tile's corner) accepts.
fn runs(batch: &scatter::Batch, visible: impl Fn(f64, f64) -> bool) -> Vec<Range<u32>> {
    let mut runs: Vec<Range<u32>> = Vec::new();
    for (x, z, range) in &batch.tiles {
        if !visible(*x, *z) {
            continue;
        }
        match runs.last_mut() {
            Some(last) if last.end == range.start => last.end = range.end,
            _ => runs.push(range.clone()),
        }
    }
    runs
}

impl WorldRenderer {
    /// The world of `terrain`. `parked` is what stands on the aprons of the aerodromes
    /// (`buildings::PARKED`): the game's own vehicle, or `buildings::stand_in()`.
    pub fn new(gpu: &Gpu, terrain: Arc<Terrain>, parked: Building) -> WorldRenderer {
        let quality = Quality::HIGH;
        let pipelines = Pipelines::new(gpu, &quality);
        let frame_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let height_map = HeightMap::new(gpu, terrain.clone());
        let road_map = RoadMap::new(gpu, terrain.clone());
        let materials = Materials::blank(gpu);
        let imagery = GroundImagery::none(gpu, &pipelines.imagery_layout, quality.ground_detail);
        let sky = SkyTables::new(gpu);
        let reflection = Reflection::new(gpu, &pipelines.sea_layout);
        let caster_map = CasterMap::new(gpu);
        let mirror_bind = frame_bind(gpu, &pipelines, &reflection.buffer, (&height_map, &road_map), &materials, (&sky, &caster_map));
        let frame_bind = frame_bind(gpu, &pipelines, &frame_buffer, (&height_map, &road_map), &materials, (&sky, &caster_map));
        let effect_pipelines = EffectPipelines::new(gpu, &pipelines.frame_layout);
        let (near_trees, far_trees, houses, grass) = scatters(&terrain, &quality);
        // Worst case: every terrain tile, and the rest.
        let most_chunks = terrain::MOST_DRAWN + OTHER_CHUNKS;
        // In the order of `Model::index`.
        let mut meshes = vec![models::pole(), models::windsock(), models::light(), vegetation::square()];
        let mut parked = Some(parked);
        meshes.extend((0..placement::HOUSE_KINDS).flat_map(|k| {
            let building = buildings::KINDS.get(k).map_or_else(|| parked.take().unwrap_or_else(buildings::stand_in), |make| make());
            [building.mesh, building.hull]
        }));
        // Buildings are drawn by the hundred and thousand: their meshes are indexed (see
        // `MeshBuilder::indexed`). The pole, the sock, the light and the square are drawn whole.
        let scenery = Model::Building(0).index();
        let meshes = meshes.iter().enumerate().map(|(i, mesh)| if i >= scenery { mesh.build_indexed(&gpu.device) } else { mesh.build(&gpu.device) }).collect();
        WorldRenderer {
            // A fair day: cumulus over half the sky, their bases at 5000 ft.
            cloud_altitude: 1500.0,
            cloud_thickness: 650.0,
            cloud_cover: 0.55,
            cloud_storms: 0,
            chunks: UniformArena::new(gpu, &pipelines.chunk_layout, size_of::<ChunkUniforms>(), most_chunks),
            objects: UniformArena::new(gpu, &pipelines.object_layout, size_of::<ObjectUniforms>(), WORLD_OBJECTS),
            objects_room: WORLD_OBJECTS,
            terrain: TerrainTiles::new(gpu, terrain.clone()),
            near_trees,
            far_trees,
            trees_on: None,
            houses,
            grass,
            flora: Flora::new(gpu, &pipelines.flora_layout, quality.tree_picture_size, crate::materials::find_textures().ok()),
            footprints: Footprints::new(terrain.clone(), quality.building_range),
            height_map,
            road_map,
            imagery,
            world: terrain,
            quality,
            samples: gpu.sample_count,
            pipelines,
            frame_buffer,
            frame_bind,
            reflection,
            mirror_bind,
            sky,
            caster_map,
            caster_draws: Vec::new(),
            materials,
            meshes,
            textured: HashMap::new(),
            looks: Vec::new(),
            look_sampler: gpu.texture_sampler(),
            textured_draws: Vec::new(),
            glass_draws: Vec::new(),
            parked_look: None,
            parked_glass: Vec::new(),
            wreck_shapes: Vec::new(),
            near_airports: Vec::new(),
            lights: AerodromeLights::default(),
            floods: Vec::new(),
            near_roads: Vec::new(),
            near_towns: Vec::new(),
            airport_refresh: 0,
            commands: Vec::new(),
            split: None,
            effects: Effects::default(),
            effect_particles: Vec::new(),
            effect_pass: ParticlePass::new(gpu),
            effect_pipelines,
            fire: None,
            behind_fire: 0,
            clouds: None,
            next_quality: None,
            golden: 0.0,
        }
    }

    /// Draws another world from the next frame on (a region loaded while the game runs): its
    /// terrain, height map, roads, trees, buildings and aerodromes are made again as for a new
    /// renderer; the pipelines, the photographs, the sky, the meshes and the weather stay.
    pub fn set_terrain(&mut self, gpu: &Gpu, terrain: Arc<Terrain>) {
        self.terrain = TerrainTiles::new(gpu, terrain.clone());
        self.terrain.view_distance = self.quality.view_distance;
        self.height_map = HeightMap::new(gpu, terrain.clone());
        self.road_map = RoadMap::new(gpu, terrain.clone());
        self.imagery = GroundImagery::none(gpu, &self.pipelines.imagery_layout, self.quality.ground_detail);
        (self.near_trees, self.far_trees, self.houses, self.grass) = scatters(&terrain, &self.quality);
        self.trees_on = None;
        self.footprints = Footprints::new(terrain.clone(), self.quality.building_range);
        self.near_airports.clear();
        self.near_roads.clear();
        self.near_towns.clear();
        self.lights = AerodromeLights::default();
        self.airport_refresh = 0;
        self.fire = None;
        self.world = terrain;
        self.bind_frames(gpu);
    }

    /// Draws another terrain of the same place, in the same axes: a region round another middle
    /// further along the way (`geodata::follow`). Nothing is dropped: every tile of ground, trees
    /// and buildings, the height map and the road map are made again in the background, the
    /// nearest first, and each is drawn as it was until its new one is there; the picture of the
    /// ground stays until the game gives the new region's (`set_imagery`).
    pub fn replace_terrain(&mut self, terrain: Arc<Terrain>) {
        self.terrain.replace(terrain.clone());
        self.near_trees.replace(terrain.clone());
        self.far_trees.replace(terrain.clone());
        self.grass.replace(terrain.clone());
        self.houses.replace(terrain.clone());
        self.footprints.replace(terrain.clone());
        self.height_map.replace(terrain.clone());
        self.road_map.replace(terrain.clone());
        self.airport_refresh = 0;
        self.world = terrain;
    }

    /// Colours a real place's ground as the satellites saw it (`geodata::imagery`), from when it has
    /// reached the GPU (a slice a frame, the old picture drawn meanwhile); `None` takes it away.
    /// Another terrain (`set_terrain`) takes it away too.
    pub fn set_imagery(&mut self, gpu: &Gpu, imagery: Option<Arc<Imagery>>) {
        let source = imagery.map(|i| (i, self.world.clone()));
        self.imagery.set(gpu, &self.pipelines.imagery_layout, source);
    }

    /// The terrain drawn.
    pub fn terrain(&self) -> &Arc<Terrain> {
        &self.world
    }

    /// Keeps a mesh of the game's (a vehicle, its parts, its wreck) on the GPU, to be drawn with
    /// `Scene::objects` and `Scene::see_through`. Its vertices are drawn as ranges, not indexed.
    pub fn add_mesh(&mut self, gpu: &Gpu, mesh: &MeshBuilder) -> MeshId {
        self.meshes.push(mesh.build(&gpu.device));
        MeshId(self.meshes.len() - 1 - Model::Added(0).index())
    }

    /// Keeps a model's materials and their pictures on the GPU (`looks.rs`), for meshes drawn with
    /// them (`add_textured_mesh`). The images are the model's (`engine::gltf::Model::images`), the
    /// materials' textures indices into them.
    pub fn add_look(&mut self, gpu: &Gpu, materials: &[engine::gltf::Material], images: &[engine::gltf::Image]) -> Result<LookId, String> {
        let look = looks::upload(gpu, &self.pipelines.look_layout, &self.look_sampler, materials, images)?;
        if std::env::var_os("ENGINE_PROFILE").is_some() {
            println!("a look of {} materials: {:.1} MB on the GPU", look.materials.len(), look.bytes as f64 / 1e6);
        }
        self.looks.push(look);
        Ok(LookId(self.looks.len() - 1))
    }

    /// `add_mesh`, drawn per pixel with the materials and pictures of `look`: `surface` has, for
    /// each vertex of `mesh`, its texture coordinates and its material (an index into the look's).
    /// Glass (a material blended) is drawn after the world, see-through. Its shadow, and its mirror
    /// in the sea, are drawn from `mesh` as any other's.
    pub fn add_textured_mesh(&mut self, gpu: &Gpu, mesh: &MeshBuilder, surface: &[[f32; 3]], look: LookId) -> MeshId {
        assert_eq!(surface.len() as u32, mesh.count(), "a surface for every vertex");
        let id = self.add_mesh(gpu, mesh);
        let uv: Vec<f32> = surface.iter().flat_map(|s| [s[0], s[1]]).collect();
        let uv = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("texture coordinates"), contents: bytemuck::cast_slice(&uv), usage: wgpu::BufferUsages::VERTEX });
        let runs = looks::runs(surface, self.looks[look.0].materials.len());
        self.textured.insert(Model::Added(id.0).index(), TexturedMesh { look: look.0, uv, runs });
        id
    }

    /// Draws the vehicle parked on the aprons (`buildings::PARKED`, given to `new`) as this mesh
    /// added with its look (`add_textured_mesh`, in the building's axes), per pixel, instead of by
    /// its vertices' colours. Its shadow and its mirror stay the building's.
    pub fn set_parked_look(&mut self, mesh: MeshId) {
        assert!(self.textured.contains_key(&Model::Added(mesh.0).index()), "a mesh added with a look");
        self.parked_look = Some(mesh);
    }

    /// GPU memory the looks take, bytes.
    pub fn looks_bytes(&self) -> u64 {
        self.looks.iter().map(|l| l.bytes).sum()
    }

    /// How the pieces of a wreck of this shape look: `mesh` (added with `add_mesh`) and the range
    /// of its vertices that is each piece, in the order of `PieceKind::ALL`. Pieces of a shape
    /// never given are not drawn; their fire is.
    /// A mesh added with `add_mesh`, to draw it with a pipeline of the game's own.
    pub fn mesh(&self, id: MeshId) -> &Mesh {
        &self.meshes[Model::Added(id.0).index()]
    }

    pub fn add_wreck_shape(&mut self, shape: &'static Pieces, mesh: MeshId, pieces: [Range<u32>; 4]) {
        self.wreck_shapes.push(WreckShape { shape, mesh, pieces });
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }

    /// How the engine turns the drawn light into the picture. Give it from `Game::post`. The contrast and
    /// colour bring AgX to the look the scene had with the tone mapping it was tuned with (measured on
    /// the shots: same spread of brightness and colour by day); the exposure follows a quarter of the
    /// way to the scene's brightness, so the night is seen as an eye used to it sees it, still dark.
    /// With the sun low the picture has more colour and more contrast (by eye: AgX pales the
    /// oranges of a sunset towards white, and the hour is the one a photograph is graded for).
    pub fn post(&self) -> engine::Post {
        engine::Post { exposure: 0.0, adaptation: 0.25, glare: 1.0, bloom: self.quality.bloom, contrast: 0.4 + 0.06 * self.golden, saturation: 1.1 + 0.16 * self.golden, ..Default::default() }
    }

    /// Group 0 of every pipeline, for a game's own pipelines drawn between `draw_sky` and `draw`:
    /// the frame uniforms, the height map and the photographs (see `pipelines.rs`).
    pub fn frame_layout(&self) -> &wgpu::BindGroupLayout {
        &self.pipelines.frame_layout
    }

    /// Binds group 0 again, for a game's own pipelines drawn after `draw` (what is see-through and
    /// must come after the world: a vehicle's glass).
    pub fn bind_frame(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.frame_bind, &[]);
    }

    /// Group 1 of the objects' pipelines: one `ObjectUniforms` at a dynamic offset.
    pub fn object_layout(&self) -> &wgpu::BindGroupLayout {
        &self.pipelines.object_layout
    }

    /// Makes group 0 again, of the scene and of the sea's mirror, after what it holds was made again.
    fn bind_frames(&mut self, gpu: &Gpu) {
        let maps = (&self.height_map, &self.road_map);
        self.frame_bind = frame_bind(gpu, &self.pipelines, &self.frame_buffer, maps, &self.materials, (&self.sky, &self.caster_map));
        self.mirror_bind = frame_bind(gpu, &self.pipelines, &self.reflection.buffer, maps, &self.materials, (&self.sky, &self.caster_map));
    }

    /// Loads the photographs the quality draws with, if they are not the ones in memory: at the
    /// first frame, and when the quality changes to one with other textures. Without the files the
    /// world cannot draw its ground: it says where it looked and ends the program. With
    /// `ENGINE_PROFILE` set it prints what was loaded.
    fn load_materials(&mut self, gpu: &Gpu) {
        if self.materials.serves(&self.quality) {
            return;
        }
        let start = engine::Instant::now();
        let dir = materials::find_textures();
        match dir.and_then(|dir| Materials::load(gpu, &dir, &self.quality).map(|materials| (dir, materials))) {
            Ok((dir, materials)) => {
                if std::env::var_os("ENGINE_PROFILE").is_some() {
                    eprintln!("textures: {:.1} MB on the GPU, read in {:.2} s from {}", materials.bytes() as f64 / 1048576.0, start.elapsed().as_secs_f64(), dir.display());
                }
                self.materials = materials;
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        self.bind_frames(gpu);
    }

    /// Changes how much is drawn from the next frame on, making what it draws with now: a frame
    /// waits for it (seconds, the first time a build compiles the quality's shaders). For a start or
    /// a picture; while the game runs, `change_quality`. The game must also hand `samples` to the
    /// engine (`Game::samples`).
    pub fn set_quality(&mut self, gpu: &Gpu, quality: Quality) {
        self.next_quality = None;
        if quality == self.quality {
            return;
        }
        self.quality = quality;
        self.far_trees.range = quality.tree_range;
        self.grass.range = quality.grass_range;
        self.flora.set_view_size(gpu, &self.pipelines.flora_layout, quality.tree_picture_size);
        self.footprints.range = quality.building_range;
        self.terrain.view_distance = quality.view_distance;
        self.pipelines.rebuild(gpu, &quality);
        self.imagery.set_detailed(gpu, &self.pipelines.imagery_layout, quality.ground_detail);
        self.samples = gpu.sample_count;
    }

    /// Changes how much is drawn as soon as what the quality draws with has been made in the
    /// background: until then the frames go on with the quality there was (`quality`), and
    /// `changing_quality` says so. The game must hand `samples` to the engine (`Game::samples`).
    pub fn change_quality(&mut self, gpu: &Gpu, quality: Quality) {
        if self.next_quality.as_ref().map_or(self.quality, |next| next.quality) == quality {
            return;
        }
        if quality == self.quality {
            self.next_quality = None;
            return;
        }
        let samples = if quality.samples == 1 { 1 } else { Gpu::SAMPLES };
        let (maker, layouts) = (gpu.pipeline_maker(samples), self.pipelines.layouts());
        let target = Target { scene: gpu.scene_format, depth: gpu.depth_format, samples };
        let pipelines = Slot::start(move || {
            let clouds = CloudPipelines::new(&maker.device, &layouts[0], &quality, target);
            (Pipelines::with_layouts(&maker, &quality, layouts), clouds)
        });
        let materials = (!self.materials.serves(&quality)).then(|| Slot::start(move || materials::find_textures().and_then(|dir| Materials::read(&dir, &quality))));
        self.next_quality = Some(NextQuality { quality, samples, pipelines, materials, made: false });
    }

    /// The samples per pixel the frames should be drawn with (`Gpu::set_samples`): the next
    /// quality's once it is ready to be drawn.
    pub fn samples(&self) -> u32 {
        match &self.next_quality {
            Some(next) if next.made => next.samples,
            _ => self.samples_now(),
        }
    }

    fn samples_now(&self) -> u32 {
        if self.quality.samples == 1 { 1 } else { Gpu::SAMPLES }
    }

    /// Whether a quality asked for is still being made ready (`set_quality`).
    pub fn changing_quality(&self) -> bool {
        self.next_quality.is_some()
    }

    /// Draws with the next quality once it is made and the frame has its samples.
    fn take_next_quality(&mut self, gpu: &Gpu) {
        let Some(next) = &mut self.next_quality else { return };
        next.made = next.pipelines.ready().is_some() && next.materials.as_mut().is_none_or(|m| m.ready().is_some());
        if !next.made || gpu.sample_count != next.samples {
            return;
        }
        let Some(next) = self.next_quality.take() else { return };
        let quality = next.quality;
        self.quality = quality;
        self.far_trees.range = quality.tree_range;
        self.grass.range = quality.grass_range;
        self.flora.set_view_size(gpu, &self.pipelines.flora_layout, quality.tree_picture_size);
        self.footprints.range = quality.building_range;
        self.terrain.view_distance = quality.view_distance;
        if let Some((pipelines, clouds)) = next.pipelines.take() {
            self.pipelines = pipelines;
            self.samples = next.samples;
            if let (Some(made), Some(clouds)) = (&mut self.clouds, clouds) {
                made.adopt(clouds);
            }
        }
        self.imagery.set_detailed(gpu, &self.pipelines.imagery_layout, quality.ground_detail);
        // The photographs read; if they could not be, `load_materials` tries again and says why.
        if let Some(Ok(read)) = next.materials.and_then(Slot::take) {
            if Materials::read_serves(&read, &quality) {
                if let Ok(materials) = Materials::upload(gpu, &read) {
                    self.materials = materials;
                    self.bind_frames(gpu);
                }
            }
        }
    }

    /// Airports and roads near a point (north, east), nearest first, as many as the shaders take.
    fn look_around(&mut self, n: f64, e: f64) {
        let r = 30_000.0;
        let mut airports = self.world.airports(n - r, n + r, e - r, e + r);
        airports.sort_by(|a, b| (a.n - n).hypot(a.e - e).total_cmp(&(b.n - n).hypot(b.e - e)));
        airports.truncate(MOST_AIRPORTS);
        self.near_airports = airports;
        // A region's roads are drawn from the road map instead.
        let mut roads = if self.road_map.used() { Vec::new() } else { self.world.roads(n - r, n + r, e - r, e + r) };
        roads.sort_by(|a, b| a.distance(n, e).total_cmp(&b.distance(n, e)));
        roads.truncate(32);
        self.near_roads = roads;
        let mut towns = self.world.towns(n - r, n + r, e - r, e + r);
        towns.sort_by(|a, b| (a.n - n).hypot(a.e - e).total_cmp(&(b.n - n).hypot(b.e - e)));
        towns.truncate(MOST_TOWNS);
        self.near_towns = towns;
    }

    fn object(&mut self, model: Model, first: u32, count: u32, matrix: Mat4, tint: [f32; 4]) {
        self.object_cut(model, first, count, matrix, tint, &[None; 2]);
    }

    /// `object`, cut along planes in its mesh's axes (`sim_core::Cut`).
    fn object_cut(&mut self, model: Model, first: u32, count: u32, matrix: Mat4, tint: [f32; 4], cuts: &[Option<Cut>; 2]) {
        let uniforms = self.objects.push(&ObjectUniforms::new(matrix.to_cols_array_2d(), tint).cut(cuts));
        self.commands.push(Command::Object { model, first, count, uniforms });
    }

    /// A draw of a mesh of the game's: as `object_cut`, or, if it has a look, its runs of one
    /// material each, kept for after the world's objects (the glass for the end).
    fn game_object(&mut self, mesh: MeshId, first: u32, count: u32, matrix: Mat4, tint: [f32; 4], cuts: &[Option<Cut>; 2]) {
        let model = Model::Added(mesh.0);
        let Some(textured) = self.textured.get(&model.index()) else {
            self.object_cut(model, first, count, matrix, tint, cuts);
            return;
        };
        let uniforms = self.objects.push(&ObjectUniforms::new(matrix.to_cols_array_2d(), tint).cut(cuts));
        let look = &self.looks[textured.look];
        for (range, material) in textured.runs(first, count) {
            let command = Command::Textured { model, first: range.start, count: range.len() as u32, uniforms, look: textured.look, material };
            if look.materials[material].glass {
                self.glass_draws.push(command);
            } else {
                self.textured_draws.push(command);
            }
        }
    }

    fn whole(&mut self, model: Model, matrix: Mat4, tint: [f32; 4]) {
        let count = self.meshes[model.index()].count;
        self.object(model, 0, count, matrix, tint);
    }

    /// Chooses what to draw and uploads it. Call once per frame from `Game::update`.
    pub fn prepare(&mut self, gpu: &Gpu, scene: &Scene) {
        const WHITE: [f32; 4] = [1.0; 4];
        let view = &scene.view;
        let cam = view.position;
        let (cam_x, cam_y, cam_z) = (cam.y, -cam.z, -cam.x);
        let camera = Camera { position: cam, forward: view.forward, up: view.up, right: view.right, fov_y: view.fov_y, shift_y: view.shift_y, near: view.near };
        let aspect = gpu.aspect();
        self.take_next_quality(gpu);
        if gpu.sample_count != self.samples {
            self.pipelines.rebuild(gpu, &self.quality);
            self.samples = gpu.sample_count;
        }
        self.load_materials(gpu);
        self.reflection.resize(gpu, &self.pipelines.sea_layout, gpu.width, gpu.height, self.quality.reflection);
        self.effect_pipelines.prepare(gpu, &self.quality);
        let room = WORLD_OBJECTS + scene.objects.len() + scene.glows.len() + scene.see_through.len() + scene.casters.iter().map(|c| c.draws.len()).sum::<usize>();
        if room > self.objects_room {
            self.objects = UniformArena::new(gpu, &self.pipelines.object_layout, size_of::<ObjectUniforms>(), room);
            self.objects_room = room;
        }
        self.commands.clear();
        self.chunks.clear();
        self.objects.clear();

        // Airports and roads near the camera, refreshed about once a second.
        if self.airport_refresh == 0 {
            self.airport_refresh = 60;
            self.look_around(cam.x, cam.y);
            self.lights.keep(gpu, &self.world, &self.near_airports);
        }
        self.airport_refresh -= 1;

        // The sky: the air of the weather, the sun and the moon of the hour, the tables for the eye.
        // The haze is uniform below the nearest aerodrome, where its visibility is reported.
        let day = crate::sun(scene.hour).day;
        let sky_lights = SkyLights::at(scene.hour);
        let (sun_dir, above) = sky_lights.direct();
        self.golden = clouds::golden_hour(sky_lights.sun.y);
        let visibility = scene.weather.visibility;
        let haze = self.quality.haze_visibility(visibility);
        let base = self.near_airports.first().map_or(0.0, |a| a.elevation);
        let air = Air::new(scene.weather, haze, base);
        let reach = sky::aerial_reach(haze);
        self.sky.update(gpu, &air, &sky_lights, cam_y, base, reach);
        let through = air.transmittance(cam_y, sun_dir.y as f64).as_vec3();
        let (tan_x, tan_y) = camera.tan_half_fov(aspect);
        let mut u: FrameUniforms = bytemuck::Zeroable::zeroed();
        u.view_proj = camera.view_proj(aspect).to_cols_array_2d();
        u.cam_right = with(view.right, tan_x);
        u.cam_up = with(view.up, tan_y);
        u.cam_fwd = with(view.forward, view.shift_y);
        u.sun_dir = with(sun_dir, (scene.time % 3600.0) as f32);
        u.sun_color = with(above * through, day);
        u.cam_wrap = [wrapped(cam_x), cam_y as f32, wrapped(cam_z), if self.quality.cloud_march.is_some() { 1.0 } else { 0.0 }];
        let azimuth = sky_lights.azimuth();
        u.sky_sun = with(sky_lights.sun, reach as f32);
        u.sky_moon = with(sky_lights.moon, sky_lights.scale);
        u.moon_light = with(sky_lights.moon_top, if sky_lights.moonlit() { 1.0 } else { 0.0 });
        u.sky_more = [azimuth.x, azimuth.z, base as f32, 0.0];
        // The vehicles' shadows from their meshes: a tile of the map for each of the nearest.
        let sunny = day > 0.3 && sun_dir.y > 0.03;
        let tiles = if sunny && self.quality.vehicle_shadows { casters::chosen(scene.casters, cam) } else { Vec::new() };
        self.caster_draws.clear();
        if !tiles.is_empty() {
            let (right, up) = casters::axes(sun_dir);
            u.caster_right = with(right, 1.0);
            u.caster_up = with(up, 0.0);
            for (t, &i) in tiles.iter().enumerate() {
                let caster = &scene.casters[i];
                let tile = Tile::of(caster, cam, sun_dir);
                u.caster_tiles[t] = with(tile.centre, tile.radius);
                let light = tile.light(sun_dir);
                for (k, draw) in caster.draws.iter().enumerate() {
                    let uniforms = self.objects.push(&ObjectUniforms::new((light * draw.model).to_cols_array_2d(), [1.0; 4]).cut(&draw.cuts));
                    let mesh = match draw.mesh {
                        CasterMesh::World(id) => CasterSource::World(Model::Added(id.0).index()),
                        CasterMesh::Game(_) => CasterSource::Game(i, k),
                    };
                    self.caster_draws.push(CasterCommand { tile: t, mesh, first: draw.first, count: draw.count, uniforms });
                }
            }
            // The body's ShadowShape takes over from its tile far below it; without a tile it is its whole shadow.
            if scene.body.is_some_and(|b| b.shadow.is_some()) {
                u.caster_up[3] = if tiles.first() == Some(&0) { 1.0 } else { 2.0 };
            }
        }
        if let Some(body) = &scene.body {
            // The body in render axes, relative to the camera: position, then forward, right and down.
            let axis = |v: DVec3| to_render(act(body.attitude, v));
            let casts = (body.shadow.is_some() || !tiles.is_empty()) && sunny;
            u.body_pos = with(to_render(body.position - cam), if casts { 1.0 } else { 0.0 });
            (u.body_fwd, u.body_right, u.body_down) = (with(axis(DVec3::X), 0.0), with(axis(DVec3::Y), 0.0), with(axis(DVec3::Z), 0.0));
            if let Some(shape) = &body.shadow {
                (u.shadow_flat, u.shadow_flat_z, u.shadow_upright) = (shape.flat, shape.flat_z, shape.upright);
                let mut reach = 0.0_f32;
                for (i, [a, b]) in shape.solids.iter().enumerate() {
                    (u.shadow_solids[2 * i], u.shadow_solids[2 * i + 1]) = (*a, *b);
                    let length = |v: &[f32; 4]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    if a[3] == 1.0 {
                        reach = reach.max(length(a) + b[0].max(b[1]).max(b[2]));
                    } else if a[3] == 2.0 {
                        reach = reach.max(length(a).max(length(b)) + b[3]);
                    }
                }
                u.shadow_reach = [reach, 0.0, 0.0, 0.0];
            }
        }
        // The flat layers' cover: none when the clouds are the volumetric ones, which the ground's shade and the sea's mirror do not know yet.
        let flat_cover = if self.quality.cloud_march.is_some() { 0.0 } else { self.cloud_cover as f32 };
        u.misc = [self.near_airports.len() as f32, (self.cloud_altitude - cam_y) as f32, self.far_trees.range as f32, flat_cover];
        for (i, a) in self.near_airports.iter().enumerate() {
            u.airports_a[i] = [(a.e - cam_x) as f32, (-a.n - cam_z) as f32, a.heading.sin() as f32, -a.heading.cos() as f32];
            u.airports_b[i] = [(a.length / 2.0) as f32, (a.width / 2.0) as f32, a.number() as f32, (a.elevation - cam_y) as f32];
            u.airports_c[i] = [a.detail.apron as f32, if a.detail.paved { 1.0 } else { 0.0 }, 0.0, 0.0];
        }
        self.height_map.update(gpu, cam_x, cam_z);
        self.road_map.update(gpu, cam_x, cam_z);
        u.road_map = [(self.road_map.centre_x - cam_x) as f32, (self.road_map.centre_z - cam_z) as f32, RoadMap::HALF_EXTENT as f32, if self.road_map.ready { 1.0 } else { 0.0 }];
        self.imagery.update(gpu, &self.pipelines.imagery_layout);
        // The trees stand on the parcels drawn: made again when those come or go.
        let same = match (self.imagery.parcels(), &self.trees_on) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.trees_on = self.imagery.parcels().cloned();
            let build = tree_build(self.trees_on.clone());
            self.near_trees.rebuild(build.clone());
            self.far_trees.rebuild(build);
            self.grass.rebuild(grass_build(self.trees_on.clone()));
        }
        u.imagery = self.imagery.uniforms(cam_x, cam_z);
        u.hmap = [(self.height_map.centre_x - cam_x) as f32, (self.height_map.centre_z - cam_z) as f32, HeightMap::HALF_EXTENT as f32, if self.height_map.ready { 1.0 } else { 0.0 }];
        // Only the towns and airfields whose ground can be told apart from here: the ground shader
        // looks through them at every pixel.
        let mut towns = 0;
        for t in self.near_towns.iter().filter(|t| (t.n - cam.x).hypot(t.e - cam.y) < 8000.0 + t.radius * 1.15) {
            u.towns[towns] = [(t.e - cam_x) as f32, (-t.n - cam_z) as f32, t.radius as f32, t.grid as f32];
            towns += 1;
        }
        let airfields = self.near_airports.iter().take_while(|a| (a.n - cam.x).hypot(a.e - cam.y) < 5000.0 + a.length).count();
        u.extra = [self.cloud_thickness as f32, self.near_roads.len() as f32, towns as f32, airfields as f32];
        for (i, r) in self.near_roads.iter().enumerate() {
            u.roads[i] = [(r.e0 - cam_x) as f32, (-r.n0 - cam_z) as f32, (r.e1 - cam_x) as f32, (-r.n1 - cam_z) as f32];
        }
        u.materials = self.materials.means();
        // The water: its waves for the wind (a few microseconds), and from where nothing is drawn
        // under the sea (the end of the ground): beyond, it is deep water by itself.
        let wind = scene.weather.mean_wind(6.0);
        let sea = Sea::new(wind, self.quality.sea_waves);
        let rough = sea.residual.map(|v| v as f32);
        u.sea = [sea.wind as f32, sea.waves.len() as f32, sea.height as f32, sea.whitecap_threshold() as f32];
        u.sea_rough = [rough[0], rough[1], rough[2], sea.whitecaps as f32];
        u.sea_more = [sea.downwind.x as f32, sea.downwind.y as f32, self.terrain.view_distance as f32, sea.period as f32];
        u.waves = sea.uniforms(scene.time, cam_x, cam_z);
        u.wave_rest = sea.rest();
        // The aerodromes' lights: how bright a candela is in this scene, a pixel's angle, how much of
        // each system is lit; and the floodlights of the aprons and the vehicles' beams as lights of
        // the ground.
        let gain = lights::gain(scene.hour);
        let settings = lights::settings(scene.hour, visibility);
        u.lights = [[gain, 2.0 * tan_y / gpu.height.max(1) as f32, lights::FADE.0, lights::FADE.1], [settings[0], settings[1], settings[2], settings[3]], [settings[4], settings[5], settings[6], settings[7]]];
        let floodlit = sim_core::lights::setting(sim_core::lights::System::Apron, sun_elevation(scene.hour), visibility) as f32;
        self.floods.clear();
        if floodlit > 0.0 {
            for a in self.near_airports.iter().take(AIRPORTS_WITH_SCENERY) {
                // Farther than 5 km the apron they light is a few pixels: they fade from 3.5 km, and the
                // ground's shaders need not look at them beyond.
                let far = 1.0 - smoothstep(3500.0, 5000.0, (a.n - cam.x).hypot(a.e - cam.y)) as f32;
                let Some(set) = self.lights.get(a).filter(|_| far > 0.0) else {
                    continue;
                };
                // A white surface under `E` lux is `E · gain / π` in the scene; the light at 1 m is `I` lux.
                let color = lights::rgb(sim_core::lights::Color::Flood) * (gain / std::f32::consts::PI * floodlit * far);
                self.floods.extend(set.floods.iter().map(|&(at, cd, axis, half, spill)| effects::FireLight { at, color: color * cd as f32, beam: Some((axis, half, spill)) }));
            }
        }
        // A landing or taxi light: a narrow, strong beam lights the ground ahead.
        for lamp in scene.lamps {
            let Some((axis, w, h)) = lamp.beam.filter(|&(_, w, h)| lamp.intensity >= BEAM_LIGHTS.0 && w.max(h) <= BEAM_LIGHTS.1) else {
                continue;
            };
            let color = lights::rgb(lamp.color) * (gain / std::f32::consts::PI * lamp.intensity);
            self.floods.push(effects::FireLight { at: cam + from_render(lamp.at), color, beam: Some((axis, (w + h) / 2.0, lamp.omni / lamp.intensity)) });
        }
        self.wreck_effects(gpu, scene, &mut u);
        // Terrain.
        self.commands.push(Command::Use(Pipeline::Terrain));
        let fwd = view.forward;
        self.terrain.select(gpu, cam_x, cam_y, cam_z);
        // The tiles in view, and where on the screen the sea may be (the tiles it is in): a tile of
        // land is mirrored only where its image falls on that (see `on_screen`).
        let view_proj = camera.view_proj(aspect);
        let mut tiles = Vec::new();
        let mut sea_seen: Option<[f32; 4]> = None;
        for &key in self.terrain.selected() {
            let c = self.terrain.tile(key);
            let half = (c.size / 2.0) as f32;
            let centre = Vec3::new((c.origin_x - cam_x) as f32 + half, (c.min_y + c.max_y) / 2.0 - cam_y as f32, (c.origin_z - cam_z) as f32 + half);
            let radius = half * 1.5 + (c.max_y - c.min_y);
            if centre.dot(fwd) < -radius {
                continue;
            }
            // The sea in it lies at sea level.
            let (low, high) = (Vec3::new((c.origin_x - cam_x) as f32, c.min_y - cam_y as f32, (c.origin_z - cam_z) as f32), Vec3::new((c.origin_x - cam_x) as f32 + 2.0 * half, c.max_y - cam_y as f32, (c.origin_z - cam_z) as f32 + 2.0 * half));
            if c.min_y < 0.0 {
                let sea = -cam_y as f32;
                if let Some(seen) = on_screen(view_proj, Vec3::new(low.x, sea, low.z), Vec3::new(high.x, sea, high.z)) {
                    sea_seen = union(sea_seen, seen);
                }
            }
            tiles.push((key, low, high));
        }
        let sea_seen = sea_seen.map(|s| [s[0].max(-1.0), s[1].max(-1.0), s[2].min(1.0), s[3].min(1.0)]).filter(|s| s[0] < s[2] && s[1] < s[3]);
        for (key, low, high) in tiles {
            let c = self.terrain.tile(key);
            let uniforms = self.chunks.push(&ChunkUniforms {
                offset: [(c.origin_x - cam_x) as f32, -cam_y as f32, (c.origin_z - cam_z) as f32, 0.0],
                wrap: [wrapped(c.origin_x), wrapped(c.origin_z), 0.0, 0.0],
            });
            // Its image in the sea: the tile mirrored in the plane of the sea, under it.
            let mirror = |y: f32| -y - 2.0 * cam_y as f32;
            let image = on_screen(view_proj, Vec3::new(low.x, mirror(high.y), low.z), Vec3::new(high.x, mirror(low.y), high.z));
            let mirrored = c.max_y > 0.0 && sea_seen.zip(image).is_some_and(|(sea, image)| overlap(sea, image));
            self.commands.push(Command::Terrain { vertices: c.buffer.clone(), uniforms, mirrored, wet: c.min_y < 0.0 });
        }

        // The sea mirrors the world when some of it may be seen and there is a picture to draw it in;
        // beyond the ground (`sea_more.z`) the sea mirrors the sky alone.
        let mirroring = self.reflection.drawn() && sea_seen.is_some();
        u.mirror = [0.0, if mirroring { 1.0 } else { 0.0 }, 1.0 / gpu.width.max(1) as f32, 1.0 / gpu.height.max(1) as f32];
        gpu.queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&u));

        // Trees and houses: each kind is one batch, drawn as the runs of its tiles that may be seen.
        // The trees near the camera are drawn as their full meshes and all of them as their simpler
        // meshes and their pictures; each tree is drawn by one level, or by two on its share of the
        // pixels while it changes (see `flora_vertex`).
        self.flora.update(gpu, &self.pipelines.flora_layout);
        let above = (scene.height_above_ground - 50.0).max(0.0);
        let batch_offset = |s: &Scatter, wrap: [f32; 4]| ChunkUniforms { offset: [(s.origin.0 - cam_x) as f32, -cam_y as f32, (s.origin.1 - cam_z) as f32, 0.0], wrap };
        // A tile is left out when it is wholly outside a cone around the view, unless the camera
        // looks steeply up or down.
        let level = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
        let in_view = |x: f64, z: f64, size: f64| {
            let half = (size / 2.0) as f32;
            let centre = Vec3::new((x - cam_x) as f32 + half, 0.0, (z - cam_z) as f32 + half);
            let (distance, radius) = (centre.length(), half * 1.5);
            fwd.y.abs() > 0.5 || distance < radius * 1.5 || (centre.dot(level) / distance).clamp(-1.0, 1.0).acos() < 1.05 + (radius / distance).asin()
        };
        let detail = TREE_DETAIL * (gpu.height as f32 / 1000.0).clamp(1.0, MOST_DETAIL);
        // Where a tree changes from its full mesh to its simpler one, and from that to its picture;
        // the most each tree's own distance and its band reach beyond.
        let (first, second) = (detail * self.quality.tree_detail, detail * self.quality.tree_pictures);
        let beyond = DETAIL_SPREAD * (1.0 + LOD_BAND / 2.0);
        self.near_trees.range = ((first * beyond) as f64).min(self.quality.tree_range);
        self.near_trees.select(gpu, cam_x, cam_z, above);
        self.far_trees.select(gpu, cam_x, cam_z, above);
        // From high up no tree is near enough for the full mesh, or for the simpler one (a
        // centimetre, not nothing: the shaders divide by it).
        let first = if above > self.near_trees.range { 0.01 } else { first };
        let second = if above > (second * beyond) as f64 { 0.01 } else { second };
        // The nearest and the farthest a tree of a tile can be from the camera, over the ground.
        let reach = |x: f64, z: f64, size: f64| {
            let (half, d) = (size / 2.0, (x + size / 2.0 - cam_x).hypot(z + size / 2.0 - cam_z));
            (((d - half * 1.42).max(0.0)) as f32, (d + half * 1.42) as f32)
        };
        // The runs of trees and buildings whose shadows are drawn later: only of the tiles that can
        // be near enough for one (over the ground, and with the height of the eye above it).
        let mut shadows: Vec<(wgpu::Buffer, Range<u32>, u32)> = Vec::new();
        let shadow_range = TREE_SHADOW_RANGE.min(self.quality.tree_range);
        let casts = |x: f64, z: f64, size: f64, range: f64| (reach(x, z, size).0 as f64).hypot(above) < range && in_view(x, z, size);
        let nearer = (2.0 - DETAIL_SPREAD) * (1.0 - LOD_BAND / 2.0);
        for level in 0..3 {
            let scatter = if level == 0 { &self.near_trees } else { &self.far_trees };
            let size = scatter.tile_size;
            self.commands.push(Command::Use(if level < 2 { Pipeline::Flora } else { Pipeline::Impostors }));
            for (kind, batch) in scatter.batches.iter().enumerate() {
                let Some(instances) = &batch.buffer else {
                    continue;
                };
                let model = match level {
                    0 => Model::Tree(kind, Detail::Near),
                    1 => Model::Tree(kind, Detail::Far),
                    _ => Model::Square,
                };
                // Only the tiles that can hold a tree drawn at this level (the margins are those of `flora_vertex`).
                let holds = |x: f64, z: f64| {
                    let (nearest, farthest) = reach(x, z, size);
                    match level {
                        0 => nearest < first * beyond,
                        1 => farthest >= first * nearer && nearest < second * beyond,
                        _ => farthest >= second * nearer,
                    }
                };
                let seen = runs(batch, |x, z| holds(x, z) && in_view(x, z, size));
                // Every tree is in the batches of the simpler ones: their shadows are drawn from there.
                let cast = if level == 1 { runs(batch, |x, z| casts(x, z, size, shadow_range)) } else { Vec::new() };
                if seen.is_empty() && cast.is_empty() {
                    continue;
                }
                let uniforms = self.chunks.push(&batch_offset(scatter, [level as f32, first, second, kind as f32]));
                for range in seen {
                    self.commands.push(Command::Instanced { model, instances: instances.clone(), range, uniforms });
                }
                shadows.extend(cast.into_iter().map(|range| (instances.clone(), range, uniforms)));
            }
        }

        // Tufts of grass round the camera, when it is low.
        if self.quality.grass_range > 0.0 && scene.height_above_ground < self.quality.grass_range * 0.5 {
            self.grass.select(gpu, cam_x, cam_z, 0.0);
            if let Some(instances) = &self.grass.batches[0].buffer {
                let size = self.grass.tile_size;
                let seen = runs(&self.grass.batches[0], |x, z| (reach(x, z, size).0 as f64) < self.quality.grass_range && in_view(x, z, size));
                if !seen.is_empty() {
                    self.commands.push(Command::Use(Pipeline::Grass));
                    let uniforms = self.chunks.push(&batch_offset(&self.grass, [self.quality.grass_range as f32, 0.0, 0.0, 0.0]));
                    for range in seen {
                        self.commands.push(Command::Instanced { model: Model::Tuft, instances: instances.clone(), range, uniforms });
                    }
                }
            }
        } else {
            self.grass.pending = 0;
        }
        // Houses.
        self.commands.push(Command::Use(Pipeline::Houses));
        self.houses.select(gpu, cam_x, cam_z, above);
        let uniforms = self.chunks.push(&batch_offset(&self.houses, [0.0; 4]));
        // The runs of buildings, whose shadows are drawn later.
        let mut house_shadows: Vec<(usize, wgpu::Buffer, Range<u32>, u32)> = Vec::new();
        let mut parked: Vec<Command> = Vec::new();
        let mut parked_glass: Vec<Command> = Vec::new();
        for (kind, batch) in self.houses.batches.iter().enumerate() {
            let Some(instances) = &batch.buffer else {
                continue;
            };
            for range in runs(batch, |x, z| in_view(x, z, self.houses.tile_size)) {
                match self.parked_look.map(|m| Model::Added(m.0)).filter(|_| kind == buildings::PARKED) {
                    Some(model) => {
                        let textured = &self.textured[&model.index()];
                        let count = self.meshes[model.index()].count;
                        for (run, material) in textured.runs(0, count) {
                            let command = Command::Parked { model, first: run.start, count: run.len() as u32, instances: instances.clone(), range: range.clone(), uniforms, look: textured.look, material };
                            if self.looks[textured.look].materials[material].glass {
                                parked_glass.push(command);
                            } else {
                                parked.push(command);
                            }
                        }
                    }
                    None => self.commands.push(Command::Instanced { model: Model::Building(kind), instances: instances.clone(), range, uniforms }),
                }
            }
            for range in runs(batch, |x, z| casts(x, z, self.houses.tile_size, HOUSE_SHADOW_RANGE)) {
                house_shadows.push((kind, instances.clone(), range, uniforms));
            }
        }

        if !parked.is_empty() {
            self.commands.push(Command::Use(Pipeline::Parked));
            self.commands.append(&mut parked);
        }
        self.parked_glass = parked_glass;

        // The buildings of a real place's maps, a mesh to a tile.
        self.footprints.select(gpu, cam_x, cam_z);
        if !self.footprints.selected.is_empty() {
            self.commands.push(Command::Use(Pipeline::Footprints));
            for &(key, (x, z)) in &self.footprints.selected {
                if in_view(x, z, footprints::TILE) {
                    let uniforms = self.chunks.push(&ChunkUniforms { offset: [(x - cam_x) as f32, -cam_y as f32, (z - cam_z) as f32, 0.0], wrap: [0.0; 4] });
                    self.commands.push(Command::Footprints { key, uniforms });
                }
            }
        }

        // Objects.
        self.commands.push(Command::Use(Pipeline::Objects));
        let wind_speed = wind.length() as f32;
        // After dusk the windsock is lit by its lamps, some 30 lux: it shines with that light alone.
        let lamps = sim_core::lights::setting(sim_core::lights::System::Obstacle, sun_elevation(scene.hour), visibility) as f32;
        let lit = 30.0 * gain / std::f32::consts::PI;
        let sock = if lamps > 0.0 { [lit, lit, lit, 2.0] } else { WHITE };
        for i in 0..self.near_airports.len().min(AIRPORTS_WITH_SCENERY) {
            let a = self.near_airports[i];
            let Some((n, e)) = sim_core::lights::windsock(&a) else {
                continue;
            };
            let wp = to_render(DVec3::new(n, e, -a.elevation) - cam);
            self.whole(Model::Pole, Mat4::from_translation(wp), WHITE);
            // The sock streams downwind and droops when the wind is light.
            let down = to_render(wind);
            let dir = Vec3::new(a.heading.sin() as f32, 0.0, -a.heading.cos() as f32);
            let down = if wind_speed > 0.3 { Vec3::new(down.x, 0.0, down.z).normalize() } else { dir };
            let droop = (1.0 - (wind_speed / 8.0).min(1.0)) * 1.25;
            let axis = (down * droop.cos() - Vec3::Y * droop.sin()).normalize();
            let side = Vec3::Y.cross(axis).normalize();
            self.whole(Model::Sock, matrix(axis, axis.cross(side), side, wp + Vec3::new(0.0, 5.6, 0.0)), sock);
            // The masts of the floodlights: the pole, taller.
            let height = (sim_core::lights::FLOOD_MAST / 6.0) as f32;
            let masts: Vec<DVec3> = self.lights.get(&a).map_or(Vec::new(), |s| s.floods.iter().map(|f| f.0).collect());
            for at in masts {
                let foot = to_render(DVec3::new(at.x, at.y, -a.elevation) - cam);
                self.whole(Model::Pole, matrix(Vec3::X * 2.0, Vec3::Y * height, Vec3::Z * 2.0, foot), WHITE);
            }
        }

        // The game's objects: its vehicles.
        self.textured_draws.clear();
        self.glass_draws.clear();
        for draw in scene.objects {
            self.game_object(draw.mesh, draw.first, draw.count, draw.model, draw.tint, &draw.cuts);
        }

        // Wrecks: each piece where it lies, blackened as it burns.
        for w in scene.wrecks {
            let burnt = w.fire.map_or(0.0, |f| (((f.out.unwrap_or(scene.time) - f.started) / 15.0).min(1.0)) as f32);
            for piece in &w.pieces {
                let at = to_render(piece.origin() - cam);
                if piece.sunk || at.length() > 6000.0 {
                    continue;
                }
                let Some(shape) = self.wreck_shapes.iter().find(|s| s.shape.kind == piece.shape.kind) else {
                    continue;
                };
                let axis = |v: DVec3| to_render(act(piece.q, v));
                let k = PieceKind::ALL.iter().position(|k| *k == piece.kind).unwrap_or(0);
                let (mesh, range) = (shape.mesh, shape.pieces[k].clone());
                let char = if piece.kind == PieceKind::Fuselage { 0.82 } else { 0.5 } * burnt;
                let tint = [1.0 - char, 1.0 - char * 1.02, 1.0 - char * 1.05, 1.0];
                self.game_object(mesh, range.start, range.len() as u32, matrix(axis(DVec3::X), axis(DVec3::Y), axis(DVec3::Z), at), tint, &piece.cuts);
            }
        }
        for glow in scene.glows {
            let size = (glow.at.length() * 0.0012).max(0.035);
            self.object(Model::Light, 0, 6, matrix(view.right * size, view.up * size, view.forward, glow.at), with(glow.color, 2.0));
        }
        if !self.textured_draws.is_empty() {
            self.commands.push(Command::Use(Pipeline::Textured));
            self.commands.append(&mut self.textured_draws);
        }

        // The shadows of the trees and the buildings, over the ground and whatever stands on it.
        if sun_dir.y > 0.04 && day > 0.25 {
            self.commands.push(Command::Use(Pipeline::HouseShadows));
            for (kind, instances, range, uniforms) in house_shadows {
                self.commands.push(Command::Instanced { model: Model::Hull(kind), instances, range, uniforms });
            }

            self.commands.push(Command::Use(Pipeline::TreeShadows));
            for (instances, range, uniforms) in shadows {
                self.commands.push(Command::Instanced { model: Model::Square, instances, range, uniforms });
            }
        }

        // The aerodromes' lights, over everything solid.
        self.commands.push(Command::Use(Pipeline::Lights));
        for a in &self.near_airports {
            let Some(set) = self.lights.get(a) else {
                continue;
            };
            let at = to_render(set.origin - cam);
            let uniforms = self.chunks.push(&ChunkUniforms { offset: [at.x, at.y, at.z, 0.0], wrap: [0.0; 4] });
            self.commands.push(Command::Lights { instances: set.buffer.clone(), count: set.count, uniforms });
        }
        // The vehicles' lamps, made again each frame (a handful).
        if !scene.lamps.is_empty() {
            let data = lights::lamp_instances(scene.lamps);
            let instances = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("lamps"), contents: bytemuck::cast_slice(&data), usage: wgpu::BufferUsages::VERTEX });
            let uniforms = self.chunks.push(&ChunkUniforms { offset: [0.0; 4], wrap: [0.0; 4] });
            self.commands.push(Command::Lights { instances, count: scene.lamps.len() as u32, uniforms });
        }

        // The sea's surface, over what is under it.
        self.commands.push(Command::Use(Pipeline::Sea));
        self.commands.push(Command::Sea);

        // The effects of the wrecks, in a second pass that reads the depth of this one; what is
        // see-through after them goes there too, to cover them as it should. Seen from under the
        // clouds the effects are in front of them: then the clouds go first, in this pass.
        self.split = None;
        let effects = !self.effect_particles.is_empty() || self.fire.is_some();
        let under_clouds = cam_y < self.cloud_altitude + self.cloud_thickness * 0.5;
        if effects && !under_clouds {
            self.split = Some(self.commands.len());
            self.commands.push(Command::Effects);
        }
        if effects {
            let uniforms = EffectUniforms { view: [view.near, gpu.height as f32 / (2.0 * tan_y), scene.time as f32, 0.0] };
            let drawing = self.effect_pipelines.drawing(gpu, &self.quality);
            let noise = self.effect_pipelines.noise();
            self.effect_pass.prepare(gpu, &drawing, noise, &uniforms, &self.effect_particles);
            if let Some(fire) = &mut self.fire {
                fire.prepare_draw(gpu, &drawing, &self.quality, &uniforms);
            }
        }

        // Clouds: the volumetric ones in the second pass, which there is then every frame; or the
        // layer as a stack of slices, drawn from the farthest to the nearest in height.
        let volumetric = self.quality.cloud_march.is_some();
        if volumetric {
            let from = scene.weather.wind_from.to_radians();
            let carried = scene.weather.wind_speed * 1.5 * scene.time;
            let storms = (self.cloud_storms as usize).min(STORMS.len());
            let storm_top = STORMS[..storms].iter().map(|s| s.3 as f32).reduce(f32::max);
            let uniforms = CloudUniforms {
                sun: with(sun_dir, scene.time as f32),
                // The shapes churn as the wind carries them: a fifth as far, upwards.
                wind: [(-from.sin() * carried).rem_euclid(clouds::WRAP) as f32, (from.cos() * carried).rem_euclid(clouds::WRAP) as f32, clouds::coverage(self.cloud_cover), (carried * 0.2).rem_euclid(3360.0) as f32],
                origin: [cam_x.rem_euclid(clouds::WRAP) as f32, cam_z.rem_euclid(clouds::WRAP) as f32, cam_y as f32, storms as f32],
                more: [view.near, 0.0, 0.0, clouds::sun_gain(sun_dir.y)],
                map: [(cam_x.rem_euclid(clouds::WRAP) + self.height_map.centre_x - cam_x) as f32, (cam_z.rem_euclid(clouds::WRAP) + self.height_map.centre_z - cam_z) as f32, HeightMap::HALF_EXTENT as f32, self.cloud_altitude as f32],
                towers: std::array::from_fn(|i| {
                    let (north, east, radius, top) = STORMS.get(i).copied().unwrap_or_default();
                    [(cam_x.rem_euclid(clouds::WRAP) + east - cam_x) as f32, (cam_z.rem_euclid(clouds::WRAP) - north - cam_z) as f32, radius as f32, top as f32]
                }),
                layers: clouds::Layers::of(self.cloud_altitude as f32, self.cloud_thickness as f32, storm_top).pack(),
                light: [1.0, 1.0, 1.0, clouds::own_shade(sun_dir.y)],
                rain: clouds::rain(scene.weather.wind_from, scene.weather.wind_speed, self.cloud_cover, storms),
                ..bytemuck::Zeroable::zeroed()
            };
            let seen = Seen { right: u.cam_right, up: u.cam_up, forward: u.cam_fwd, eye: DVec3::new(cam_x, cam_y, cam_z) };
            self.clouds.get_or_insert_with(|| Clouds::new(gpu)).prepare(gpu, &self.pipelines.frame_layout, &self.quality, uniforms, seen);
            self.split.get_or_insert(self.commands.len());
            self.commands.push(Command::Clouds);
        }
        self.commands.push(Command::Use(Pipeline::Clouds));
        let slices = if volumetric { 0 } else { self.quality.cloud_slices.min(Quality::HIGH.cloud_slices) };
        let mut heights: Vec<(f32, f32)> = (0..slices)
            .map(|i| {
                let level = (i as f32 + 0.5) / slices as f32;
                ((self.cloud_altitude - cam_y) as f32 + level * self.cloud_thickness as f32, level)
            })
            .collect();
        heights.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
        for (y, level) in heights {
            let uniforms = self.chunks.push(&ChunkUniforms { offset: [self.cloud_thickness as f32 / slices as f32, y, 0.0, level], wrap: [0.0; 4] });
            self.commands.push(Command::CloudSlice { uniforms });
        }
        if effects && under_clouds {
            self.split.get_or_insert(self.commands.len());
            self.commands.push(Command::Effects);
        }

        // The glass of the textured meshes, then what the game draws see-through, last (the blur
        // of a spinning rotor, say).
        if !self.glass_draws.is_empty() {
            self.commands.push(Command::Use(Pipeline::Glass));
            self.commands.append(&mut self.glass_draws);
        }
        if !self.parked_glass.is_empty() {
            self.commands.push(Command::Use(Pipeline::ParkedGlass));
            self.commands.append(&mut self.parked_glass);
        }
        if !scene.see_through.is_empty() {
            self.commands.push(Command::Use(Pipeline::ObjectsBlended));
            for draw in scene.see_through {
                self.object_cut(Model::Added(draw.mesh.0), draw.first, draw.count, draw.model, draw.tint, &draw.cuts);
            }
        }

        self.chunks.upload(gpu);
        self.objects.upload(gpu);
        self.draw_casters(gpu, scene);
        if mirroring {
            self.draw_mirror(gpu, &u, cam_y as f32);
        }
    }

    /// Draws the vehicles into their shadow map (`casters.rs`), each into its tile: before the
    /// frame, which reads it.
    fn draw_casters(&self, gpu: &Gpu, scene: &Scene) {
        if self.caster_draws.is_empty() {
            return;
        }
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vehicles' shadows") });
        {
            let mut pass = self.caster_map.pass(&mut encoder);
            pass.set_pipeline(&self.pipelines.casters);
            let mut tile = usize::MAX;
            for c in &self.caster_draws {
                if c.tile != tile {
                    tile = c.tile;
                    let (x, y, side) = Tile::viewport(tile);
                    pass.set_viewport(x, y, side, side, 0.0, 1.0);
                }
                let mesh: &Mesh = match c.mesh {
                    CasterSource::World(index) => &self.meshes[index],
                    CasterSource::Game(i, k) => match scene.casters[i].draws[k].mesh {
                        CasterMesh::Game(mesh) => mesh,
                        CasterMesh::World(_) => continue,
                    },
                };
                pass.set_bind_group(1, self.objects.bind_group(), &[c.uniforms]);
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                match &mesh.indices {
                    Some(indices) => {
                        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(c.first..c.first + c.count, 0, 0..1);
                    }
                    None => pass.draw(c.first..c.first + c.count, 0..1),
                }
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }

    /// Draws the world mirrored in the sea (`reflection.rs`) as the scene will draw it, with the view
    /// mirrored in the sea's plane, `height` under the camera: what goes before the sea's surface,
    /// without the shadows.
    fn draw_mirror(&self, gpu: &Gpu, scene: &FrameUniforms, height: f32) {
        let Some(pipelines) = &self.pipelines.mirror else {
            return;
        };
        let mut u = *scene;
        u.view_proj = Reflection::view_proj(Mat4::from_cols_array_2d(&scene.view_proj), height).to_cols_array_2d();
        u.mirror[1] = 0.0;
        gpu.queue.write_buffer(&self.reflection.buffer, 0, bytemuck::bytes_of(&u));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mirror") });
        if let Some(mut pass) = self.reflection.pass(&mut encoder) {
            let sea = self.commands.iter().position(|c| matches!(c, Command::Use(Pipeline::Sea))).unwrap_or(self.commands.len());
            // First the land as it is, which hides what is mirrored behind it: only what is seen
            // through the sea is shaded.
            pass.set_bind_group(0, &self.frame_bind, &[]);
            self.run(&mut pass, &self.commands[..sea], Replay::Land, |p| matches!(p, Pipeline::Terrain).then_some(&pipelines.land));
            pass.set_bind_group(0, &self.mirror_bind, &[]);
            self.run(&mut pass, &self.commands[..sea], Replay::Mirror(pipelines), |p| mirrored(pipelines, p));
        }
        gpu.queue.submit([encoder.finish()]);
    }

    /// What the wrecks look like this frame (`effects.rs`): their particles, sorted for the GPU, and
    /// in the frame's uniforms the lights of their fires, the marks on the ground and the foam on
    /// the water, those that matter most from here.
    fn wreck_effects(&mut self, gpu: &Gpu, scene: &Scene, u: &mut FrameUniforms) {
        let cam = scene.view.position;
        let wind = scene.weather.mean_wind(6.0);
        self.effects.clear();
        let terrain = self.world.clone();
        let ground = move |n: f64, e: f64| terrain.height(n, e).max(0.0);
        // The fire of the newest crash near the camera that still burns or smokes is simulated.
        let simulated = scene
            .wrecks
            .iter()
            .filter(|w| self.quality.fire_simulation && (w.impact - cam).length() < fire::RANGE && Fire::alive(w, scene.time - w.time))
            .max_by(|a, b| a.time.total_cmp(&b.time));
        match simulated {
            Some(w) => {
                let g = ground(w.impact.x, w.impact.y);
                if self.fire.as_ref().is_none_or(|f| !f.is_for(w)) {
                    let (simulation, noise) = self.effect_pipelines.simulation();
                    self.fire = Some(Fire::new(gpu, simulation, noise, w, g, &self.world));
                }
                if let Some(f) = &mut self.fire {
                    f.advance(gpu, w, scene.time - w.time, g, wind, crate::sun(scene.hour).direction, cam);
                }
            }
            None => self.fire = None,
        }
        for w in scene.wrecks {
            let top = self.fire.as_ref().filter(|f| f.is_for(w)).map(|f| f.top);
            effects::wreck(w, scene.time, wind, cam, top, &ground, &mut self.effects);
        }
        particles::sorted(&self.effects.particles, cam, &mut self.effect_particles);
        self.behind_fire = match &self.fire {
            Some(f) => {
                let middle = (f.centre() - cam).length() as f32;
                self.effect_particles.iter().take_while(|p| glam::Vec3::from(p.pos).length() > middle).count() as u32
            }
            None => 0,
        };
        let at = |p: DVec3| to_render(p - cam);
        // The lights that light most of what is seen: bright, and near the eye or not far from it.
        let mut lights = self.effects.lights.clone();
        lights.extend_from_slice(&self.floods);
        lights.sort_by(|a, b| {
            let weight = |l: &effects::FireLight| l.color.length() as f64 / ((l.at - cam).length_squared() + 2500.0);
            weight(b).total_cmp(&weight(a))
        });
        for (i, l) in lights.iter().take(u.fire_pos.len()).enumerate() {
            u.fire_pos[i] = with(at(l.at), l.beam.map_or(0.0, |b| b.2));
            u.fire_color[i] = with(l.color, 0.0);
            u.fire_axis[i] = l.beam.map_or([0.0; 4], |(axis, half, _)| with(axis, half));
            u.fire_count[0] = (i + 1) as f32;
        }
        // The marks and the foam nearest the eye.
        let mut marks = self.effects.marks.clone();
        marks.sort_by(|a, b| ((a.scar.pos - cam).length() - a.scar.radius).total_cmp(&((b.scar.pos - cam).length() - b.scar.radius)));
        for (i, m) in marks.iter().take(u.marks_a.len()).enumerate() {
            u.marks_a[i] = with(at(m.scar.pos), m.scar.radius as f32);
            u.marks_b[i] = [m.scar.crater as f32, m.glow, effects::hash(i as u32, (m.scar.made * 64.0) as u32), 0.0];
            u.fire_count[1] = (i + 1) as f32;
        }
        let mut foam = self.effects.foam.clone();
        foam.sort_by(|a, b| (a.at - cam).length().total_cmp(&(b.at - cam).length()));
        for (i, f) in foam.iter().filter(|f| f.strength > 0.0).take(u.foam.len()).enumerate() {
            u.foam[i] = with(at(f.at), f.radius as f32);
            u.foam_b[i / 4][i % 4] = f.strength;
            u.fire_count[2] = (i + 1) as f32;
        }
    }

    /// Draws the sky over the whole picture, and binds group 0. Call first from `Game::draw`; what
    /// the game draws after it and before `draw` (a cabin round the eye) covers the world.
    pub fn draw_sky(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.frame_bind, &[]);
        pass.set_pipeline(&self.pipelines.sky);
        pass.draw(0..3, 0..1);
    }

    /// Draws what `prepare` chose, after `draw_sky`: all of it, or what goes before the second
    /// pass when there is one (`translucent`).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.frame_bind, &[]);
        self.run(pass, &self.commands[..self.split.unwrap_or(self.commands.len())], Replay::Scene, |p| Some(scene_pipeline(&self.pipelines, p)));
    }

    /// Whether this frame has a second pass (`Game::translucent`): when there are effects of
    /// wrecks to draw, which read the depth of the first.
    pub fn translucent(&self) -> bool {
        self.split.is_some()
    }

    /// Work between the two passes (`Game::before_translucent`): the fire's simulation drawn at half
    /// resolution, to be put over the scene in the second pass.
    pub fn before_translucent(&self, encoder: &mut wgpu::CommandEncoder) {
        if let (Some(fire), Some(_)) = (&self.fire, self.split) {
            fire.march(encoder, &self.frame_bind);
        }
        if let (Some(clouds), true) = (&self.clouds, self.quality.cloud_march.is_some() && self.split.is_some()) {
            clouds.march(encoder, &self.frame_bind, &self.height_map.texture.create_view(&Default::default()));
        }
    }

    /// Draws the second pass (`Game::draw_translucent`): the effects of the wrecks and what is
    /// see-through after them, the clouds and the game's `see_through`.
    pub fn draw_translucent(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(split) = self.split {
            pass.set_bind_group(0, &self.frame_bind, &[]);
            self.run(pass, &self.commands[split..], Replay::Scene, |p| Some(scene_pipeline(&self.pipelines, p)));
        }
    }

    /// Replays `commands` in `pass` for `replay`, each with the pipeline `pick` gives for it; the draws
    /// of a pipeline it gives none for are left out.
    fn run<'p>(&self, pass: &mut wgpu::RenderPass<'_>, commands: &[Command], replay: Replay<'p>, pick: impl Fn(Pipeline) -> Option<&'p wgpu::RenderPipeline>) {
        let mut skipping = false;
        // The mesh in vertex slot 0 and its indices, to bind each once for a run of draws; and
        // whether the index buffer bound is the terrain's.
        let mut bound: Option<usize> = None;
        let mut terrain_indices = false;
        let mesh = |pass: &mut wgpu::RenderPass<'_>, bound: &mut Option<usize>, terrain_indices: &mut bool, model: Model| {
            let mesh = match model {
                Model::Tree(species, detail) => self.flora.mesh(species, detail),
                Model::Tuft => self.flora.tuft(),
                _ => &self.meshes[model.index()],
            };
            if *bound != Some(model.index()) {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                if let Some(indices) = &mesh.indices {
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    *terrain_indices = false;
                }
                *bound = Some(model.index());
            }
            mesh
        };
        for command in commands {
            match command {
                Command::Effects => {
                    // The particles beyond the middle of the fire's grid, the grid, the rest.
                    let count = self.effect_particles.len() as u32;
                    match &self.fire {
                        Some(fire) => {
                            self.effect_pass.draw(pass, 0..self.behind_fire);
                            pass.set_bind_group(0, &self.frame_bind, &[]);
                            fire.draw(pass);
                            self.effect_pass.draw(pass, self.behind_fire..count);
                        }
                        None => self.effect_pass.draw(pass, 0..count),
                    }
                }
                Command::Use(pipeline) => match pick(*pipeline) {
                    Some(p) => {
                        pass.set_pipeline(p);
                        if matches!(pipeline, Pipeline::Flora | Pipeline::Impostors | Pipeline::Grass | Pipeline::TreeShadows) {
                            pass.set_bind_group(2, &self.flora.bind, &[]);
                        }
                        skipping = false;
                    }
                    None => skipping = true,
                },
                _ if skipping => {}
                Command::Sea => {
                    pass.set_bind_group(1, &self.reflection.sea_bind, &[]);
                    pass.draw(0..6, 0..1);
                }
                Command::Lights { instances, count, uniforms } => {
                    pass.set_bind_group(1, self.chunks.bind_group(), &[*uniforms]);
                    pass.set_vertex_buffer(0, instances.slice(..));
                    bound = None;
                    pass.draw(0..6, 0..*count);
                }
                Command::Terrain { mirrored: false, .. } if !matches!(replay, Replay::Scene) => {}
                Command::Terrain { vertices, uniforms, wet, .. } => {
                    // Only a tile the sea reaches pays for leaving out what is under it: a
                    // shader that may discard is shaded before the depth test.
                    if let Replay::Mirror(m) = replay {
                        pass.set_pipeline(if *wet { &m.coast } else { &m.terrain });
                    }
                    pass.set_bind_group(1, self.chunks.bind_group(), &[*uniforms]);
                    pass.set_bind_group(2, &self.imagery.bind, &[]);
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    if !terrain_indices {
                        pass.set_index_buffer(self.terrain.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                        terrain_indices = true;
                    }
                    bound = None;
                    pass.draw_indexed(0..self.terrain.index_count, 0, 0..1);
                }
                Command::Instanced { model, instances, range, uniforms } => {
                    let mesh = mesh(pass, &mut bound, &mut terrain_indices, *model);
                    pass.set_bind_group(1, self.chunks.bind_group(), &[*uniforms]);
                    pass.set_vertex_buffer(1, instances.slice(..));
                    if mesh.indices.is_some() {
                        pass.draw_indexed(0..mesh.count, 0, range.clone());
                    } else {
                        pass.draw(0..mesh.count, range.clone());
                    }
                }
                Command::Footprints { key, uniforms } => {
                    let Some(m) = self.footprints.mesh(*key) else {
                        continue;
                    };
                    pass.set_bind_group(1, self.chunks.bind_group(), &[*uniforms]);
                    pass.set_vertex_buffer(0, m.vertices.slice(..));
                    pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
                    (bound, terrain_indices) = (None, false);
                    pass.draw_indexed(0..m.count, 0, 0..1);
                }
                Command::Object { model, first, count, uniforms } => {
                    mesh(pass, &mut bound, &mut terrain_indices, *model);
                    pass.set_bind_group(1, self.objects.bind_group(), &[*uniforms]);
                    pass.draw(*first..*first + *count, 0..1);
                }
                Command::Parked { model, first, count, instances, range, uniforms, look, material } => {
                    mesh(pass, &mut bound, &mut terrain_indices, *model);
                    pass.set_bind_group(1, self.chunks.bind_group(), &[*uniforms]);
                    pass.set_vertex_buffer(1, instances.slice(..));
                    if let Some(textured) = self.textured.get(&model.index()) {
                        pass.set_vertex_buffer(2, textured.uv.slice(..));
                    }
                    pass.set_bind_group(2, &self.looks[*look].materials[*material].bind, &[]);
                    pass.draw(*first..*first + *count, range.clone());
                }
                Command::Textured { model, first, count, uniforms, look, material } => {
                    mesh(pass, &mut bound, &mut terrain_indices, *model);
                    if let Some(textured) = self.textured.get(&model.index()) {
                        pass.set_vertex_buffer(1, textured.uv.slice(..));
                    }
                    pass.set_bind_group(1, self.objects.bind_group(), &[*uniforms]);
                    pass.set_bind_group(2, &self.looks[*look].materials[*material].bind, &[]);
                    pass.draw(*first..*first + *count, 0..1);
                }
                Command::Clouds => {
                    if let (Some(clouds), Replay::Scene) = (&self.clouds, &replay) {
                        pass.set_bind_group(0, &self.frame_bind, &[]);
                        clouds.draw(pass);
                    }
                }
                Command::CloudSlice { uniforms } => {
                    pass.set_bind_group(1, self.chunks.bind_group(), &[*uniforms]);
                    pass.draw(0..6, 0..1);
                }
            }
        }
    }

    /// Draw calls of the frame last prepared, the sky's included.
    pub fn draw_calls(&self) -> usize {
        1 + self.commands.iter().filter(|c| !matches!(c, Command::Use(_))).count()
    }

    /// Tiles still being generated in the background, and what a crash would need that is still
    /// being made (`effect_pipelines.rs`); 0 when the picture is complete.
    pub fn pending(&self) -> usize {
        self.effect_pipelines.pending() + self.terrain.pending + self.near_trees.pending + self.far_trees.pending + self.grass.pending + self.flora.pending() + self.houses.pending + self.footprints.pending + usize::from(!self.height_map.ready) + usize::from(self.road_map.used() && !self.road_map.ready) + usize::from(self.imagery.pending())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_visible_tiles_are_one_run() {
        let batch = scatter::Batch { tiles: vec![(0.0, 0.0, 0..5), (1.0, 0.0, 5..9), (2.0, 0.0, 9..10), (3.0, 0.0, 10..14)], ..Default::default() };
        assert_eq!(runs(&batch, |_, _| true), vec![0..14]);
        assert_eq!(runs(&batch, |x, _| x != 2.0), vec![0..9, 10..14]);
        assert_eq!(runs(&batch, |_, _| false), Vec::<Range<u32>>::new());
    }

    #[test]
    fn the_meshes_are_in_the_order_of_their_models() {
        assert_eq!(Model::Building(0).index(), 4);
        assert_eq!(Model::Hull(placement::HOUSE_KINDS - 1).index() + 1, Model::Added(0).index());
    }
}
