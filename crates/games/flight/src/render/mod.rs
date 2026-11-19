//! Draws the game's aircraft and multirotors into the world of `world_render`: each kind's mesh
//! with its moving parts and lights, the propellers' blur, their shadows, and the
//! cockpit seen from the seat (each kind of aircraft's, `kinds::cockpit`, drawn by `cockpit::render`). It also gives the
//! world what it draws of the aircraft without knowing them: the parked aircraft of the aprons and
//! the meshes of the wrecks.
//!
//! The game describes what to draw each frame with a [`Scene`]; nothing here reads the keyboard or
//! advances the simulation. Everything else on the screen is `world_render`'s.

mod course;
mod wreck;
pub use course::CourseLook;

use crate::cockpit::render::{bake, start_baking, Baked, CockpitRenderer};
use crate::cockpit::{CockpitControls, Lever, PanelControl};
use crate::kinds::{self, Craft};
use crate::models::{self, LightKind, Moves, Part, Surface};
use engine::pipeline::{Depth, PipelineDesc};
use engine::texture;
use engine::{wgpu, Gpu, MeshBuilder, UniformArena};
use sim_core::{act, clamp, smoothstep, AircraftKind, Damage, Dynamics, PieceKind, Pieces, SurfaceState, Terrain, DEG, MOST_MOTORS};
use glam::{DQuat, DVec3, Mat3, Mat4, Vec3};
use std::f32::consts::TAU;
use std::mem::size_of;
use std::ops::Range;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use world_render::{Body, Caster, CasterDraw, CasterMesh, Lamp, MeshId, ObjectDraw, ObjectUniforms, WorldRenderer};
pub use world_render::{to_render, Preset, Quality, ViewPoint};

/// Aircraft drawn at most; the player's is always one of them.
const MOST_PLANES: usize = 8;
/// The most legs a gear has.
const LEGS: usize = sim_core::MOST_WHEELS;
/// Stops the exposure opens beyond the world's in a cockpit, and how much it opens for what is in
/// the middle of the view there (`engine::Post::spot`), by day and at night: the panel, when the
/// pilot looks at it.
const CABIN_EXPOSURE: f32 = 0.9;
const CABIN_SPOT: f32 = 0.7;
const CABIN_SPOT_NIGHT: f32 = 0.2;

/// How the world's picture is made from a cockpit, `day` of the sun up (`world_render::Sun::day`):
/// the eye opens up for the cabin's shade a little more than the world's exposure follows by
/// itself, as it does looking from a room through its window, and more for what it looks at when
/// that is in the shade: the panel seen from near or at dusk, the world through the windows then
/// left to glare. At night it hardly does: the night stays dark, and the panel is read by its lights.
fn cabin_post(post: engine::Post, day: f32) -> engine::Post {
    let twilight = (day / 0.3).clamp(0.0, 1.0);
    let spot = CABIN_SPOT_NIGHT + (CABIN_SPOT - CABIN_SPOT_NIGHT) * twilight * twilight * (3.0 - 2.0 * twilight);
    engine::Post { exposure: post.exposure + CABIN_EXPOSURE, spot, ..post }
}

/// One aircraft or multirotor as it should be drawn.
#[derive(Clone, Copy, Debug)]
pub struct PlaneVisual {
    /// Which model is drawn.
    pub kind: Craft,
    /// Centre of gravity in world axes.
    pub position: DVec3,
    /// Body axes to world axes.
    pub attitude: DQuat,
    /// Control surface angles, radians: aileron positive rolls right, elevator positive is trailing
    /// edge down, rudder positive is right pedal, flap positive is down.
    pub aileron: f32,
    pub elevator: f32,
    pub rudder: f32,
    pub flap: f32,
    /// The trim's angle, radians, as the simulation adds it to the elevator's.
    pub trim: f32,
    pub rpm: f32,
    /// How far each leg's spring is compressed, metres, in the order of `AircraftType::gear`: nose
    /// (or tail), left, right; a transport's five.
    pub gear: [f32; LEGS],
    /// How far each wheel is steered, radians, its front to the right positive (`AircraftType::steer_angle`).
    pub steer: [f32; LEGS],
    /// How fast each wheel rolls over the ground, m/s, forward positive; `None` off the ground.
    pub roll: [Option<f32>; LEGS],
    /// How hard each wheel is braked, 0...1.
    pub brake: [f32; LEGS],
    /// Where a gear that retracts is: 1 down, 0 up (`Aircraft::gear`).
    pub gear_down: f32,
    /// What has given way: wings and tailplane torn short, surfaces loose or gone, legs collapsed.
    /// An aircraft in pieces (`Damage::broken_up`) is not drawn: its wreck is.
    pub damage: Damage,
    /// A multirotor's rotors' speeds, rad/s, in the order of its motors.
    pub rotors: [f32; MOST_MOTORS],
    /// How far each of its propellers reaches, a share of its radius (its longer blade's,
    /// `Multirotor::blades`): 1 whole, a stub when broken.
    pub props: [f32; MOST_MOTORS],
    /// A tilt-rotor's nacelles (left, right) and the nacelle each rotor is on, as far forward of
    /// upright as they are, radians, about the line across the frame through `tilt_pivot`.
    pub nacelles: [f32; 2],
    pub rotor_tilt: [f32; MOST_MOTORS],
    pub tilt_pivot: Vec3,
    /// Multiplies the colour of its fixed parts (white: as it is): a rival's quad in a race.
    pub tint: [f32; 4],
}

/// The tint that shows a rival's quad in its colour (`PlaneVisual::tint`): the colour, made
/// brighter, since it multiplies a frame's dark colours.
pub fn rival_tint(colour: [u8; 3]) -> [f32; 4] {
    let c = colour.map(|x| 0.4 + 2.6 * f32::from(x) / 255.0);
    [c[0], c[1], c[2], 1.0]
}

impl PlaneVisual {
    /// The visual state of a simulated aircraft or multirotor.
    pub fn of(plane: &sim_core::Vehicle) -> PlaneVisual {
        let aircraft = match &plane.dynamics {
            Dynamics::Aircraft(a) => a,
            Dynamics::Car(_) => unreachable!("the flight game has no cars: they are the driving game's"),
            Dynamics::Multirotor(m) => {
                let body = m.body;
                let mut rotors = [0.0; MOST_MOTORS];
                for (r, w) in rotors.iter_mut().zip(m.rotor_speeds()) {
                    *r = *w as f32;
                }
                let mut props = [1.0; MOST_MOTORS];
                for (p, b) in props.iter_mut().zip(m.blades()) {
                    *p = b[0].max(b[1]) as f32;
                }
                let nacelles = m.nacelles().map(|a| a as f32);
                let rotor_tilt = std::array::from_fn(|i| m.p.motors.get(i).and_then(|motor| motor.nacelle).map_or(0.0, |k| nacelles[k]));
                let tilt_pivot = m.p.tilt.as_ref().map_or(Vec3::ZERO, |t| t.pivot.as_vec3());
                return PlaneVisual {
                    kind: Craft::Multirotor(m.p.kind),
                    position: body.pos,
                    attitude: body.q,
                    aileron: 0.0,
                    elevator: m.elevator() as f32,
                    rudder: 0.0,
                    flap: 0.0,
                    trim: 0.0,
                    rpm: 0.0,
                    gear: [0.0; LEGS],
                    steer: [0.0; LEGS],
                    roll: [None; LEGS],
                    brake: [0.0; LEGS],
                    gear_down: 1.0,
                    // Sunk, it is not drawn.
                    damage: Damage { broken_up: m.sunk(), ..Damage::default() },
                    rotors,
                    props,
                    nacelles,
                    rotor_tilt,
                    tilt_pivot,
                    tint: [1.0; 4],
                };
            }
        };
        let (c, p, d) = (&aircraft.controls, &aircraft.p, aircraft.data());
        let yoke = if c.elevator >= 0.0 { -c.elevator * p.elevator_up } else { -c.elevator * p.elevator_down };
        PlaneVisual {
            kind: Craft::Aircraft(aircraft.p.kind),
            position: aircraft.state.pos,
            attitude: aircraft.state.q,
            aileron: (c.aileron * p.aileron_max) as f32,
            elevator: clamp(yoke - c.trim * p.trim_range, -p.elevator_up, p.elevator_down) as f32,
            rudder: (c.rudder * p.rudder_max) as f32,
            flap: (aircraft.flap_deg * DEG) as f32,
            trim: (-c.trim * p.trim_range) as f32,
            rpm: aircraft.rpm() as f32,
            gear: std::array::from_fn(|i| if i < p.gear.len() { d.wheel_compression[i].min(p.gear_travel) as f32 } else { 0.0 }),
            steer: std::array::from_fn(|i| p.gear.get(i).map_or(0.0, |w| p.steer_angle(w, c.rudder, aircraft.state.vel.x)) as f32),
            roll: std::array::from_fn(|i| (d.wheel_compression[i] > 0.0).then_some(d.wheel_roll[i] as f32)),
            brake: std::array::from_fn(|i| p.gear.get(i).map_or(0.0, |w| c.brake(w.brake)) as f32),
            gear_down: aircraft.gear as f32,
            damage: *aircraft.damage(),
            rotors: [0.0; MOST_MOTORS],
            props: [1.0; MOST_MOTORS],
            nacelles: [0.0; 2],
            rotor_tilt: [0.0; MOST_MOTORS],
            tilt_pivot: Vec3::ZERO,
            tint: [1.0; 4],
        }
    }
}

/// Everything the renderer needs to know about one frame.
pub struct Scene<'a> {
    pub view: ViewPoint,
    pub planes: &'a [PlaneVisual],
    /// Index in `planes` of the aircraft whose shadow is drawn and whose cockpit the view may be in.
    pub player: usize,
    /// True in the cockpit view: the player's fuselage and propeller are not drawn.
    pub inside_player: bool,
    /// In the cockpit view, whether the player's propeller is drawn (a setting): its blades
    /// standing or turning slowly, its blur turning fast.
    pub propeller_inside: bool,
    /// Height of the player's aircraft above the ground, metres (trees and houses are skipped when far above).
    pub player_agl: f64,
    /// Hour of the day, 0..24, and simulation time in seconds.
    pub hour: f64,
    pub time: f64,
    /// The weather: the wind for the windsocks, the visibility and humidity for the haze.
    pub weather: &'a sim_core::Weather,
    /// What is left of the aircraft that have come apart (`World::wrecks`).
    pub wrecks: &'a [sim_core::Wreck],
    /// Seconds since the last frame, for things that only animate (the propeller).
    pub dt: f32,
    /// The player's yokes, levers and pedals, for the cabin.
    pub cockpit: CockpitControls,
    /// A race course's gates, when one is flown.
    pub course: Option<CourseLook<'a>>,
}

/// A craft's cockpit (in the order of `Craft::all`: the aircraft's first, as the fleet has them):
/// being baked on another thread, or on the GPU; a craft without one has none.
enum Cockpit {
    Baking(Receiver<Baked>),
    Ready(Box<CockpitRenderer>),
    None,
}

/// A draw of the cockpit: a range of it before the world, or its glass after it.
enum Inside {
    Cabin { first: u32, count: u32, uniforms: u32 },
    Glass { uniforms: u32 },
}

/// The cockpits' pipelines: the world's shaders with the cockpits' appended, its groups 0 and 1.
struct CockpitPipelines {
    /// A cockpit (`cabin_vertex` and `cabin_fragment` in `cockpit/cabin.wgsl`, with `cabin_layout`
    /// and the occlusion of each vertex in a second buffer).
    cabin: wgpu::RenderPipeline,
    /// Its windows' glass, over the world (`cabin_glass_fragment`).
    glass: wgpu::RenderPipeline,
    /// The quality and the `Gpu::sample_count` they were made for.
    made_for: (Quality, u32),
}

/// The world's shaders with the cockpits' appended, as `quality` draws them: the cabin's photographs
/// blended from three projections where the world's are fetched twice (`Quality::texture_scatter`),
/// else from one.
fn cockpit_source(quality: &Quality) -> String {
    let cabin = include_str!("../cockpit/cabin.wgsl");
    const TRIPLANAR: &str = "const CABIN_TRIPLANAR: bool = true;";
    assert!(cabin.contains(TRIPLANAR), "cabin.wgsl has no line `{TRIPLANAR}`");
    world_render::shader_source(quality) + &cabin.replace(TRIPLANAR, &format!("const CABIN_TRIPLANAR: bool = {};", quality.texture_scatter))
}

impl CockpitPipelines {
    fn new(gpu: &Gpu, world: &WorldRenderer, cabin_layout: &wgpu::BindGroupLayout) -> CockpitPipelines {
        let quality = world.quality();
        let shader = gpu.shader_with_ending_loops("flight", &cockpit_source(&quality));
        let mesh = [MeshBuilder::layout()];
        let (frame, object) = (world.frame_layout(), world.object_layout());
        let buffers = [MeshBuilder::layout(), CockpitRenderer::occlusion_layout()];
        let cabin = gpu.pipeline(&PipelineDesc { label: "cabin", shader: &shader, vertex: "cabin_vertex", fragment: "cabin_fragment", buffers: &buffers, bind_groups: &[frame, object, cabin_layout], depth: Depth::Write, blend: false });
        // Premultiplied: the glass dims what is behind it a little and adds what it reflects and scatters.
        let premultiplied = wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent::OVER };
        let glass = PipelineDesc { label: "cabin glass", shader: &shader, vertex: "object_vertex", fragment: "cabin_glass_fragment", buffers: &mesh, bind_groups: &[frame, object, cabin_layout], depth: Depth::Read, blend: true };
        let glass = gpu.pipeline_blended(&glass, Some(premultiplied));
        CockpitPipelines { cabin, glass, made_for: (quality, gpu.sample_count) }
    }
}

pub struct Renderer {
    /// The world: terrain, sky, scenery, weather (`visibility`, `cloud_*`), wrecks and fire.
    pub world: WorldRenderer,
    /// Each kind's cockpit (`kinds::cockpit`), in the order of the fleet (`AircraftKind::all`), baked
    /// on other threads from the start (`cockpit::render::start_baking`).
    cockpits: Vec<Cockpit>,
    /// Group 2 of the cabin pipelines: the instruments' canvas, photographs (colour; normal +
    /// roughness + occlusion), their sampler, the light through the windows (a volume), the print,
    /// the cockpit's uniforms, the sun's shadow map and its sampler.
    cabin_layout: wgpu::BindGroupLayout,
    pipelines: CockpitPipelines,
    /// The cockpit's per-draw blocks (`ObjectUniforms`), its draws, and which kind's it is.
    inside_uniforms: UniformArena,
    inside: Vec<Inside>,
    inside_kind: usize,
    /// How much of the day the last frame prepared was in (`world_render::Sun::day`).
    day: f32,
    /// The mesh of each craft in the world, in the order of `Craft::all` (the aircraft of the
    /// fleet, then the multirotors).
    meshes: Vec<MeshId>,
    /// The parts of each craft, in the order of `Craft::all`.
    parts: Vec<Vec<Part>>,
    /// Each part's vertices piece by piece (`wreck::regions`), to draw what a tear left of each.
    regions: Vec<Vec<[Range<u32>; 4]>>,
    /// How each kind's legs of the gear move, in the order of `AircraftType::gear`.
    legs: Vec<[Leg; LEGS]>,
    /// How each kind comes apart (an aircraft; a multirotor stays whole).
    shapes: Vec<Option<&'static Pieces>>,
    /// The lights of each kind of aircraft, in body axes.
    lights: Vec<Vec<models::Light>>,
    /// Where the propeller of each aircraft is in its turn, radians; each rotor of a multirotor.
    prop_angles: Vec<f32>,
    rotor_angles: Vec<[f32; MOST_MOTORS]>,
    /// Each aircraft's wheels: where each is in its turn, radians rolled forward, and how fast it turns, rad/s.
    wheels: Vec<[(f32, f32); LEGS]>,
    /// The radius of a sphere round each kind's centre of gravity that holds it all, metres.
    reach: Vec<f32>,
    /// This frame's draws for the world.
    objects: Vec<ObjectDraw>,
    /// This frame's draws of what casts a shadow (`world_render::Caster`): each aircraft's whole
    /// outside, its range of them, where it is.
    caster_draws: Vec<CasterDraw<'static>>,
    caster_ranges: Vec<(DVec3, f32, Range<usize>)>,
    lamps: Vec<Lamp>,
    discs: Vec<ObjectDraw>,
    /// The race gates' models (`course`); none if they could not be read.
    gates: Option<course::GateModels>,
}

/// A light of an aircraft as it shines, for the world to draw (`body`: body axes to render axes,
/// relative to the camera). Intensities after FAR 23.1391 to 23.1401 (position lights 40 cd ahead
/// and 20 cd astern, an anti-collision light of 400 cd) and a PAR-36 landing lamp (110 000 cd in a
/// beam some 10° across, a little down).
fn lamp(light: &models::Light, body: &Mat4) -> Lamp {
    use sim_core::lights::Color;
    let along = |x: f32, y: f32, z: f32| body.transform_vector3(Vec3::new(x, y, z)).normalize();
    let at = body.transform_point3(light.position);
    let c = light.color;
    match light.kind {
        // Red to the left and green to the right, ahead and out to the side; white astern.
        LightKind::Navigation if c.x > 2.0 * c.y || c.y > 2.0 * c.x => {
            let side = if c.x > c.y { -1.0 } else { 1.0 };
            let color = if side < 0.0 { Color::Red } else { Color::Green };
            Lamp { at, color, intensity: 40.0, omni: 5.0, beam: Some((along(0.94, 0.34 * side, 0.0), 0.35, 0.6)), radius: 0.03 }
        }
        LightKind::Navigation => Lamp { at, color: Color::White, intensity: 20.0, omni: 1.0, beam: Some((along(-1.0, 0.0, 0.0), 1.0, 1.0)), radius: 0.03 },
        LightKind::Beacon => Lamp { at, color: Color::Red, intensity: 0.0, omni: 400.0, beam: None, radius: 0.04 },
        LightKind::Landing => Lamp { at, color: Color::White, intensity: 110_000.0, omni: 0.0, beam: Some((along(1.0, 0.0, 0.05), 0.09, 0.08)), radius: 0.05 },
    }
}

/// Bind group layout entries of the cabin's uniforms, its shadow map and the shadow map's sampler.
fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }
}

fn depth_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }
}

fn comparison_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None }
}

/// Place of a kind of aircraft in the fleet (`AircraftKind::all`): of its cockpit.
fn kind_index(kind: AircraftKind) -> usize {
    AircraftKind::all().position(|k| k == kind).unwrap_or(0)
}

fn matrix(c0: Vec3, c1: Vec3, c2: Vec3, translation: Vec3) -> Mat4 {
    Mat4::from_cols(c0.extend(0.0), c1.extend(0.0), c2.extend(0.0), translation.extend(1.0))
}

/// How rotor `index`'s nacelle has turned it: forward about the line across the frame through
/// the nacelles' pivot (nothing, off a nacelle).
fn tilted(plane: &PlaneVisual, index: usize) -> Mat4 {
    rotation(plane.rotor_tilt.get(index).copied().unwrap_or(0.0), Vec3::NEG_Y, plane.tilt_pivot)
}

fn rotation(angle: f32, axis: Vec3, pivot: Vec3) -> Mat4 {
    Mat4::from_translation(pivot) * Mat4::from_axis_angle(axis, angle) * Mat4::from_translation(-pivot)
}

/// Shrinks what is round `axis` through `pivot` towards it, to `share` of its reach across it (a
/// broken propeller drawn as the stubs left at its hub); along the axis, as it was.
fn shortened(share: f32, axis: Vec3, pivot: Vec3) -> Mat4 {
    let a = axis.normalize_or_zero();
    let across = Mat3::IDENTITY - Mat3::from_cols(a * a.x, a * a.y, a * a.z);
    let scale = Mat3::from_cols(a * a.x, a * a.y, a * a.z) + across * share;
    Mat4::from_translation(pivot) * Mat4::from_mat3(scale) * Mat4::from_translation(-pivot)
}

/// One leg of a model's gear, as it moves (`models::Moves::Strut`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Leg {
    /// Where its strut is fixed, and how it reaches from there to its foot (zero if rigid).
    root: Vec3,
    reach: Vec3,
    /// A point of the line its fork steers about, and the line.
    steer_at: Vec3,
    steer_axis: Vec3,
    /// Its wheel's centre and radius.
    axle: Vec3,
    radius: f32,
    /// Where it is fixed to the airframe (its highest point), the pivot it folds about when it collapses.
    fold: Vec3,
    /// The solids its shadow is cast by, standing uncompressed (`world_render::ShadowShape::solids`):
    /// the strut as a rod, the fairing round the wheel and the tyre as ellipsoids.
    solids: [[[f32; 4]; 2]; 3],
}

/// An ellipsoid along the body's axes round the box from `lo` to `hi`, a little bigger than the
/// box's own (which would leave out its rounded corners' share of a shadow).
fn ellipsoid(lo: Vec3, hi: Vec3) -> [[f32; 4]; 2] {
    let (centre, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0 * 1.08);
    [centre.extend(1.0).to_array(), half.max(Vec3::splat(0.01)).extend(0.0).to_array()]
}

/// A rod through `points`: between the two that lie furthest apart, as thick as they lie off that
/// line on average.
fn rod(points: &[Vec3]) -> [[f32; 4]; 2] {
    let centroid = points.iter().copied().sum::<Vec3>() / points.len() as f32;
    let far = |from: Vec3| points.iter().copied().max_by(|a, b| a.distance_squared(from).total_cmp(&b.distance_squared(from))).unwrap_or(from);
    let a = far(centroid);
    let b = far(a);
    let along = (b - a).normalize_or_zero();
    let radius = (points.iter().map(|p| (*p - a).reject_from_normalized(along).length()).sum::<f32>() / points.len() as f32).max(0.015);
    // The rod's rounded ends reach out by its radius.
    let inset = along * radius.min((b - a).length() / 2.0);
    [(a + inset).extend(2.0).to_array(), (b - inset).extend(radius).to_array()]
}

/// The bounds of `points`.
fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), &p| (lo.min(p), hi.max(p)))
}

/// The solids of a craft's gear as they stand this frame (`world_render::ShadowShape::solids`):
/// each leg's raised by its spring's compression, as `leg_matrices` draws it (a bending strut's foot
/// with it, its root staying); none of a leg that has collapsed.
fn gear_solids(legs: &[Leg; LEGS], plane: &PlaneVisual) -> [[[f32; 4]; 2]; world_render::SHADOW_SOLIDS] {
    let mut solids = [[[0.0; 4]; 2]; world_render::SHADOW_SOLIDS];
    // The shadow has solids for three legs: a transport's others are under its fuselage and wing.
    for (leg, l) in legs.iter().enumerate().take(world_render::SHADOW_SOLIDS / 3) {
        if plane.damage.gear_broken[leg] || plane.gear_down < 0.5 {
            continue;
        }
        let lift = plane.gear[leg];
        let [mut strut, mut fairing, mut tyre] = l.solids;
        if l.reach.with_z(0.0).length_squared() > 1e-4 {
            // The shear raises each end by how far it reaches out along the strut.
            let out = l.reach.with_z(0.0);
            for end in strut.iter_mut() {
                let k = (Vec3::new(end[0], end[1], end[2]) - l.root).with_z(0.0).dot(out) / out.length_squared();
                end[2] -= lift * k.clamp(0.0, 1.0);
            }
        }
        fairing[0][2] -= lift;
        tyre[0][2] -= lift;
        solids[3 * leg..3 * leg + 3].copy_from_slice(&[strut, fairing, tyre]);
    }
    solids
}

/// The legs of a model's gear, from its parts.
fn legs(model: &models::AircraftModel) -> [Leg; LEGS] {
    let points = |part: &Part| -> Vec<Vec3> {
        model.mesh.data[part.first as usize * 10..(part.first + part.count) as usize * 10].as_chunks::<10>().0.iter().map(|v| Vec3::new(v[0], v[1], v[2])).collect()
    };
    std::array::from_fn(|leg| {
        let mut l = Leg::default();
        let mut highest: Option<Vec3> = None;
        let mut fork = Vec::new();
        // The tyres of every axle of the leg: one solid round them all.
        let mut tyres = Vec::new();
        for part in model.parts.iter().filter(|p| p.leg() == Some(leg)) {
            let points = points(part);
            if let Some(top) = points.iter().copied().min_by(|a, b| a.z.total_cmp(&b.z)) {
                highest = Some(highest.map_or(top, |h| if top.z < h.z { top } else { h }));
            }
            match part.moves {
                Moves::Strut { .. } => {
                    (l.root, l.reach) = (part.hinge, part.axis);
                    l.solids[0] = rod(&points);
                }
                Moves::Fork { .. } => {
                    (l.steer_at, l.steer_axis) = (part.hinge, part.axis);
                    fork = points;
                }
                _ => {
                    l.axle = part.hinge;
                    l.radius = points.iter().map(|p| p.z - part.hinge.z).fold(0.0, f32::max);
                    tyres.extend(points);
                }
            }
        }
        if !tyres.is_empty() {
            let (lo, hi) = bounds(&tyres);
            l.solids[2] = ellipsoid(lo, hi);
        }
        // The fairing round the wheel: what of the fork comes down beside the tyre (not the
        // piston above it, which the strut's rod stands for).
        let beside: Vec<Vec3> = fork.into_iter().filter(|p| p.z > l.axle.z - 1.2 * l.radius).collect();
        if !beside.is_empty() && l.radius > 0.0 {
            let (lo, hi) = bounds(&beside);
            l.solids[1] = ellipsoid(lo, hi);
        }
        l.fold = highest.unwrap_or(Vec3::ZERO);
        l
    })
}

/// Where a leg's strut, its fork and its wheel are drawn (body axes), with its spring `compressed`
/// (metres), steered `steer` (radians, the front to the right) and its wheel turned `rolled`
/// (radians, forward): what the spring raises rises by the compression, the wheel's contact point
/// on the ground with it; a bending strut's foot rises with it and its root stays (a shear along
/// how it reaches out, which keeps the foot over the axle), a rigid one stays whole.
fn leg_matrices(leg: &Leg, compressed: f32, steer: f32, rolled: f32) -> [Mat4; 3] {
    let lift = Mat4::from_translation(Vec3::new(0.0, 0.0, -compressed));
    let out = leg.reach.with_z(0.0);
    let strut = if out.length_squared() > 1e-4 {
        let k = -compressed / out.length_squared();
        matrix(Vec3::new(1.0, 0.0, k * out.x), Vec3::new(0.0, 1.0, k * out.y), Vec3::Z, Vec3::new(0.0, 0.0, -k * out.dot(leg.root)))
    } else {
        Mat4::IDENTITY
    };
    let fork = if leg.steer_axis.length_squared() > 0.5 { lift * rotation(steer, leg.steer_axis.normalize(), leg.steer_at) } else { lift };
    // About y a turn takes the top of the wheel back: rolling forward is a turn the other way.
    [strut, fork, fork * rotation(-rolled, Vec3::Y, leg.axle)]
}

/// How fast a wheel of `radius` turns after `dt` seconds, rad/s, rolling forward positive, from
/// `rate`: on the ground with it, rolling at `roll` m/s; in the air it runs down, slowly on its
/// bearings, fast `brake`d.
fn wheel_rate(rate: f32, roll: Option<f32>, brake: f32, radius: f32, dt: f32) -> f32 {
    match roll {
        Some(speed) if radius > 0.0 => speed / radius,
        _ => {
            let slowing = (rate.abs() / 20.0 + 1.0 + 60.0 * brake) * dt;
            rate - rate.signum() * slowing.min(rate.abs())
        }
    }
}

/// A collapsed leg, folded about where it is fixed: a main leg splayed out and up, a nose or tail
/// wheel swept back and up.
fn folded(pivot: Vec3) -> Mat4 {
    if pivot.y.abs() > 0.1 {
        rotation(-pivot.y.signum() * 40.0_f32.to_radians(), Vec3::X, pivot)
    } else {
        rotation(-65.0_f32.to_radians(), Vec3::Y, pivot)
    }
}

/// A leg of a gear that retracts on its way up (`down` 1 to 0), turned about where it is fixed: a
/// leg under the wing swings in towards the fuselage, one under the fuselage forward.
fn retracting(pivot: Vec3, down: f32) -> Mat4 {
    let up = (1.0 - down).clamp(0.0, 1.0);
    if up == 0.0 {
        return Mat4::IDENTITY;
    }
    let angle = up * 88.0_f32.to_radians();
    if pivot.y.abs() > 3.0 {
        rotation(pivot.y.signum() * angle, Vec3::X, pivot)
    } else {
        rotation(angle, Vec3::Y, pivot)
    }
}

/// How a loose surface flutters as it trails, radians: a few degrees, a few times a second.
fn trailing(time: f64, seed: f32) -> f32 {
    ((time * 7.3) as f32 + seed).sin() * 3.0_f32.to_radians()
}


impl Renderer {
    pub fn new(gpu: &Gpu, terrain: Arc<Terrain>) -> Renderer {
        // The files were checked when the game started (`kinds::check`): one that does not read now is broken.
        let mut world = WorldRenderer::new(gpu, terrain, kinds::parked_aircraft().unwrap_or_else(|e| panic!("the parked aircraft: {e}")));
        let mut fleet: Vec<models::AircraftModel> = Craft::all().map(|craft| kinds::model_of(craft).unwrap_or_else(|e| panic!("{e}"))).collect();
        let (mut meshes, mut regions, mut shapes) = (Vec::new(), Vec::new(), Vec::new());
        let parked_kind = kinds::parked_kind();
        for (model, craft) in fleet.iter_mut().zip(Craft::all()) {
            // A model with textures is drawn with them, per pixel; the others by their vertices' colours.
            let look = model.look.as_ref().map(|look| world.add_look(gpu, &look.materials, &look.images).unwrap_or_else(|e| panic!("{}: {e}", craft.model())));
            // The aircraft parked on the aprons too, if its model has textures.
            if let (Some(look), Craft::Aircraft(kind)) = (look, craft) {
                if Some(kind) == parked_kind {
                    let building = models::parked(model, kinds::exterior(kind).parked.as_ref());
                    let mesh = world.add_textured_mesh(gpu, &building.mesh, &models::parked_surface(model), look);
                    world.set_parked_look(mesh);
                }
            }
            let Craft::Aircraft(kind) = craft else {
                meshes.push(match look {
                    Some(look) => world.add_textured_mesh(gpu, &model.mesh, &model.surface, look),
                    None => world.add_mesh(gpu, &model.mesh),
                });
                // A multirotor stays whole: each part is one piece.
                regions.push(model.parts.iter().map(|p| [p.first..p.first + p.count, 0..0, 0..0, 0..0]).collect());
                shapes.push(None);
                continue;
            };
            let shape = kind.spec().pieces;
            regions.push(wreck::regions(model, shape));
            meshes.push(match look {
                Some(look) => world.add_textured_mesh(gpu, &model.mesh, &model.surface, look),
                None => world.add_mesh(gpu, &model.mesh),
            });
            let (wreck_mesh, wreck_surface, pieces) = wreck::pieces(model, shape);
            let wreck_mesh = match look {
                Some(look) => world.add_textured_mesh(gpu, &wreck_mesh, &wreck_surface, look),
                None => world.add_mesh(gpu, &wreck_mesh),
            };
            world.add_wreck_shape(shape, wreck_mesh, pieces);
            shapes.push(Some(shape));
        }
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries });
        let cabin_layout = layout("cabin", &[texture::layout_entry(0), texture::array_layout_entry(1), texture::array_layout_entry(2), texture::sampler_layout_entry(3), texture::volume_layout_entry(4), texture::layout_entry(5), uniform_entry(6), depth_entry(7), comparison_entry(8), texture::array_layout_entry(9), texture::array_layout_entry(10)]);
        // The parts of a cockpit, and its glass.
        let most_inside = 24;
        let gates = course::GateModels::new(gpu, &mut world).map_err(|e| eprintln!("{e}: the race gates are not drawn")).ok();
        Renderer {
            gates,
            cockpits: Craft::all().map(|craft| kinds::cabin(craft).map_or(Cockpit::None, |cabin| Cockpit::Baking(start_baking(cabin)))).collect(),
            inside_kind: 0,
            day: 1.0,
            pipelines: CockpitPipelines::new(gpu, &world, &cabin_layout),
            inside_uniforms: UniformArena::new(gpu, world.object_layout(), size_of::<ObjectUniforms>(), most_inside),
            cabin_layout,
            inside: Vec::new(),
            world,
            meshes,
            parts: fleet.iter().map(|a| a.parts.clone()).collect(),
            regions,
            legs: fleet.iter().map(legs).collect(),
            shapes,
            lights: fleet.iter().map(|a| a.lights.clone()).collect(),
            // The farthest vertex, and some room for the gear's springs and the surfaces' throw.
            reach: fleet.iter().map(|a| a.mesh.data.chunks_exact(10).map(|v| Vec3::new(v[0], v[1], v[2]).length()).fold(0.0, f32::max) + 0.25).collect(),
            caster_draws: Vec::new(),
            caster_ranges: Vec::new(),
            prop_angles: Vec::new(),
            rotor_angles: Vec::new(),
            wheels: Vec::new(),
            objects: Vec::new(),
            lamps: Vec::new(),
            discs: Vec::new(),
        }
    }

    /// Changes how much is drawn from the next frame on (`WorldRenderer::set_quality`).
    pub fn set_quality(&mut self, gpu: &Gpu, quality: Quality) {
        self.world.set_quality(gpu, quality);
    }

    /// Changes how much is drawn once it has been made ready in the background, the frames going on
    /// meanwhile (`WorldRenderer::change_quality`).
    pub fn change_quality(&mut self, gpu: &Gpu, quality: Quality) {
        self.world.change_quality(gpu, quality);
    }

    /// A quality asked for is still being made ready.
    pub fn changing_quality(&self) -> bool {
        self.world.changing_quality()
    }

    /// The samples per pixel to draw the next frame with (`Game::samples`).
    pub fn samples(&self) -> u32 {
        self.world.samples()
    }

    /// How the engine turns the drawn light into the picture (`WorldRenderer::post`).
    /// From a cabin the eye opens up for its shade a little more than the world's exposure follows
    /// by itself and for what it looks at (`cabin_post`).
    pub fn post(&self) -> engine::Post {
        let post = self.world.post();
        if self.inside.is_empty() { post } else { cabin_post(post, self.day) }
    }

    /// Chooses what to draw and uploads it. Call once per frame from `Game::update`.
    pub fn prepare(&mut self, gpu: &Gpu, scene: &Scene) {
        const WHITE: [f32; 4] = [1.0; 4];
        if self.pipelines.made_for != (self.world.quality(), gpu.sample_count) {
            self.pipelines = CockpitPipelines::new(gpu, &self.world, &self.cabin_layout);
        }
        let cam = scene.view.position;
        let day = world_render::sun(scene.hour).day;
        self.day = day;
        self.receive_cockpits(gpu);
        self.inside.clear();
        self.inside_uniforms.clear();
        self.objects.clear();
        self.lamps.clear();
        self.discs.clear();
        self.caster_draws.clear();
        self.caster_ranges.clear();

        // Each aircraft in render axes, relative to the camera: position, then forward, right and down.
        let axes = |p: &PlaneVisual| {
            let axis = |v: DVec3| to_render(act(p.attitude, v));
            (to_render(p.position - cam), axis(DVec3::X), axis(DVec3::Y), axis(DVec3::Z))
        };
        let player = scene.planes.get(scene.player);

        // The cabin round the pilot, drawn before the world: what it covers (the glare shield and
        // the panel under the instruments, the roof, the walls) is then not shaded for the world behind it.
        // A craft without a cockpit (a quad) has no cabin: its view is its camera's.
        if let Some(p) = player.filter(|p| scene.inside_player && kinds::cabin(p.kind).is_some()) {
            let (rel, fwd, right, down) = axes(p);
            let kind = p.kind.index();
            let model = p.kind.index();
            self.inside_kind = kind;
            self.wait_for_cockpit(gpu, kind);
            let Cockpit::Ready(cockpit) = &mut self.cockpits[kind] else { unreachable!("waited for") };
            for (first, count, m) in cockpit.draws(matrix(fwd, right, down, rel), &scene.cockpit) {
                let uniforms = self.inside_uniforms.push(&ObjectUniforms::new(m.to_cols_array_2d(), WHITE));
                self.inside.push(Inside::Cabin { first, count, uniforms });
            }
            let uniforms = self.inside_uniforms.push(&ObjectUniforms::new(matrix(fwd, right, down, rel).to_cols_array_2d(), WHITE));
            self.inside.push(Inside::Glass { uniforms });
            // The sun (or the moon) in body axes, if it is up; the wings and the cowling shade the cockpit too.
            let light = world_render::sun(scene.hour).direction;
            let sun = (light.y > 0.0).then(|| Vec3::new(light.dot(fwd), light.dot(right), light.dot(down)));
            let buffer = &self.world.mesh(self.meshes[model]).buffer;
            let casters: Vec<(&wgpu::Buffer, Range<u32>)> = self.parts[model].iter().filter(|part| part.shades_cabin).map(|part| (buffer, part.first..part.first + part.count)).collect();
            cockpit.prepare(gpu, sun, &scene.cockpit, &casters);
        }

        // Aircraft: the player's and as many of the others as fit.
        self.prop_angles.resize(scene.planes.len(), 0.0);
        self.rotor_angles.resize(scene.planes.len(), [0.0; MOST_MOTORS]);
        self.wheels.resize(scene.planes.len(), [(0.0, 0.0); LEGS]);
        let mut others = MOST_PLANES - 1;
        for (i, plane) in scene.planes.iter().enumerate() {
            let is_player = i == scene.player;
            if !is_player {
                if others == 0 {
                    continue;
                }
                others -= 1;
            }
            // In pieces: the wreck is drawn instead.
            let damage = &plane.damage;
            if damage.broken_up {
                continue;
            }
            let hidden = is_player && scene.inside_player;
            let kind = plane.kind.index();
            let first_caster = self.caster_draws.len();
            let (rel, fwd, right, down) = axes(plane);
            let body = matrix(fwd, right, down, rel);
            self.prop_angles[i] = (self.prop_angles[i] + plane.rpm.min(420.0) / 60.0 * TAU * scene.dt) % TAU;
            // A rotor's blades are drawn turning no faster than they can be seen to; the blur shows the rest.
            for (angle, speed) in self.rotor_angles[i].iter_mut().zip(plane.rotors) {
                *angle = (*angle + speed.min(30.0) * scene.dt) % TAU;
            }
            // Each leg: what its spring raises and steers, its wheel as it has rolled; collapsed, folded.
            // The third matrix is the fork's again: each wheel turns about its own axle (`rolled`),
            // so a bogie's two axles each turn about theirs.
            let mut rolled = [0.0; LEGS];
            let legs: [[Mat4; 3]; LEGS] = std::array::from_fn(|leg| {
                let shape = &self.legs[kind][leg];
                let (angle, rate) = &mut self.wheels[i][leg];
                *rate = wheel_rate(*rate, plane.roll[leg], plane.brake[leg], shape.radius, scene.dt);
                *angle = (*angle + *rate * scene.dt) % TAU;
                rolled[leg] = *angle;
                if damage.gear_broken[leg] {
                    [body * folded(shape.fold); 3]
                } else {
                    let [strut, fork, _] = leg_matrices(shape, plane.gear[leg], plane.steer[leg], 0.0);
                    let stowing = retracting(shape.fold, plane.gear_down);
                    [strut, fork, fork].map(|m| body * stowing * m)
                }
            });
            // A gear that is up is in its wells, behind doors: not drawn.
            let gear_up = plane.gear_down < 0.02;
            // What is left of each piece: a wing or the tailplane torn short is drawn with its cut.
            let left = match self.shapes[kind] {
                Some(shape) => PieceKind::ALL.map(|piece| damage.left_of(shape, piece)),
                None => [Some([None, None]), None, None, None],
            };
            // A surface the pilot no longer moves trails with the air.
            let moved = |state: SurfaceState, angle: f32, seed: f32| if state == SurfaceState::Working { angle } else { trailing(scene.time, seed + i as f32) };
            for k in 0..self.parts[kind].len() {
                let part = self.parts[kind][k];
                let turn = |angle: f32| body * rotation(angle, part.axis, part.hinge);
                // From the cockpit the cabin is its lining, drawn before the world; from outside
                // there is none. It casts its shadow all the same.
                let drawn = !(hidden && part.inside == models::Inside::Hidden);
                // A propeller turning fast casts no shadow of its blades: it would flicker.
                let mut casts = true;
                let m = match part.moves {
                    Moves::Fixed => body,
                    Moves::Surface { by, scale } => {
                        let state = match by {
                            Surface::AileronLeft => damage.ailerons[0],
                            Surface::AileronRight => damage.ailerons[1],
                            Surface::FlapLeft => damage.flaps[0],
                            Surface::FlapRight => damage.flaps[1],
                            Surface::Elevator | Surface::Trim => damage.elevator,
                            Surface::Rudder => damage.rudder,
                        };
                        // What has been torn off is not drawn.
                        if state == SurfaceState::Gone {
                            continue;
                        }
                        let angle = match by {
                            // A flap no longer held is blown back up.
                            Surface::FlapLeft | Surface::FlapRight => if state == SurfaceState::Working { plane.flap } else { 0.0 },
                            Surface::AileronLeft | Surface::AileronRight => plane.aileron,
                            Surface::Elevator => plane.elevator,
                            Surface::Rudder => plane.rudder,
                            Surface::Trim => plane.trim,
                        };
                        // A surface the pilot no longer moves trails with the air, each with a flutter of its own.
                        let seed = match by {
                            Surface::AileronLeft | Surface::FlapLeft => 0.0,
                            Surface::AileronRight | Surface::FlapRight => 1.7,
                            Surface::Elevator | Surface::Trim => 3.1,
                            Surface::Rudder => 4.6,
                        };
                        let flaps = matches!(by, Surface::FlapLeft | Surface::FlapRight);
                        turn(if flaps { angle * scale } else { moved(state, angle * scale, seed) })
                    }
                    Moves::Strut { .. } | Moves::Fork { .. } | Moves::Wheel { .. } | Moves::GearOut if gear_up => continue,
                    Moves::GearUp if !gear_up => continue,
                    Moves::GearOut | Moves::GearUp => body,
                    Moves::Strut { leg } => legs[leg.min(LEGS - 1)][0],
                    Moves::Fork { leg } => legs[leg.min(LEGS - 1)][1],
                    Moves::Wheel { leg } if damage.gear_broken[leg.min(LEGS - 1)] => legs[leg.min(LEGS - 1)][2],
                    // About y a turn takes the top of the wheel back: rolling forward is a turn the other way.
                    Moves::Wheel { leg } => legs[leg.min(LEGS - 1)][2] * rotation(-rolled[leg.min(LEGS - 1)], Vec3::Y, part.hinge),
                    // From the cockpit the propeller is not drawn at all if the settings say so.
                    Moves::Propeller | Moves::Disc if hidden && !scene.propeller_inside => continue,
                    // From the cockpit turning blades are not drawn, only their blur; standing, they are blades.
                    Moves::Propeller if hidden && plane.rpm > 300.0 => continue,
                    Moves::Propeller => {
                        casts = plane.rpm < 300.0;
                        turn(self.prop_angles[i])
                    }
                    Moves::Disc => {
                        // From the cockpit, right behind it, the blur is faint: a full disc's edge would be a line across the view.
                        let alpha = smoothstep(300.0, 1200.0, plane.rpm as f64) as f32 * if hidden { 0.07 } else { 0.22 };
                        self.discs.push(ObjectDraw { mesh: self.meshes[kind], first: part.first, count: part.count, model: body, tint: [1.0, 1.0, 1.0, alpha], cuts: [None; 2] });
                        continue;
                    }
                    // A broken propeller is drawn as what is left at its hub, its blur as small.
                    Moves::Nacelle { index } => body * rotation(plane.nacelles[index.min(1)], Vec3::NEG_Y, part.hinge),
                    Moves::Rotor { index } => {
                        casts = plane.rotors.get(index).is_none_or(|&speed| speed < 60.0);
                        let left = plane.props.get(index).copied().unwrap_or(1.0);
                        let spun = rotation(self.rotor_angles[i].get(index).copied().unwrap_or(0.0), part.axis, part.hinge) * shortened(left, part.axis, part.hinge);
                        body * tilted(plane, index) * spun
                    }
                    Moves::RotorDisc { index } => {
                        let speed = plane.rotors.get(index).copied().unwrap_or(0.0);
                        let left = plane.props.get(index).copied().unwrap_or(1.0);
                        let alpha = smoothstep(60.0, 600.0, speed as f64) as f32 * 0.14;
                        if alpha > 0.0 {
                            let model = body * tilted(plane, index) * shortened(left, Vec3::Z, part.hinge);
                            self.discs.push(ObjectDraw { mesh: self.meshes[kind], first: part.first, count: part.count, model, tint: [1.0, 1.0, 1.0, alpha], cuts: [None; 2] });
                        }
                        continue;
                    }
                };
                // Piece by piece, what is left of each.
                for (range, cuts) in self.regions[kind][k].iter().zip(&left) {
                    if let (false, Some(cuts)) = (range.is_empty(), cuts) {
                        if drawn {
                            self.objects.push(ObjectDraw { mesh: self.meshes[kind], first: range.start, count: range.len() as u32, model: m, tint: plane.tint, cuts: *cuts });
                        }
                        if casts {
                            self.caster_draws.push(CasterDraw { mesh: CasterMesh::World(self.meshes[kind]), first: range.start, count: range.len() as u32, model: m, cuts: *cuts });
                        }
                    }
                }
            }
            // The player's first: its shadow is the sharpest, and its ShadowShape takes over far below it.
            let range = (plane.position, self.reach[kind], first_caster..self.caster_draws.len());
            if is_player {
                self.caster_ranges.insert(0, range);
            } else {
                self.caster_ranges.push(range);
            }
            // Navigation lights and the landing light from dusk to dawn, the beacon always, flashing;
            // not those on what has been torn off.
            let flash = ((scene.time + i as f64 * 0.37) / 1.3).fract() < 0.1;
            for light in &self.lights[kind] {
                let on = match light.kind {
                    LightKind::Navigation => day < 0.75,
                    // From the cockpit too: its beam lights the runway ahead and its lens faces away.
                    LightKind::Landing => day < 0.75,
                    LightKind::Beacon => flash && !hidden,
                };
                let at = light.position.as_dvec3();
                let piece = self.shapes[kind].map_or(PieceKind::Fuselage, |shape| shape.of_point(at));
                let kept = left[PieceKind::ALL.iter().position(|k| *k == piece).unwrap_or(0)].is_some_and(|cuts| cuts.iter().flatten().all(|c| (c.normal.dot(at) < c.offset) == c.inside));
                if on && kept {
                    self.lamps.push(lamp(light, &body));
                }
            }
        }
        self.inside_uniforms.upload(gpu);
        if let (Some(gates), Some(look)) = (&self.gates, &scene.course) {
            gates.draws(look, cam, scene.hour, scene.time, &mut self.objects);
        }

        // The player's aircraft is the body the view belongs to: its shadow, its cockpit's light.
        let shadow = |p: &PlaneVisual| {
            let mut shape = match p.kind {
                Craft::Aircraft(kind) => kinds::damaged_shadow(kind, &p.damage),
                craft => kinds::shadow_of(craft),
            };
            shape.solids = gear_solids(&self.legs[p.kind.index()], p);
            shape
        };
        let body = player.map(|p| Body { position: p.position, attitude: p.attitude, shadow: (!p.damage.broken_up).then(|| shadow(p)) });
        let casters: Vec<Caster> = self.caster_ranges.iter().map(|(centre, radius, range)| Caster { centre: *centre, radius: *radius, draws: &self.caster_draws[range.clone()] }).collect();
        let world_scene = world_render::Scene {
            view: scene.view,
            hour: scene.hour,
            time: scene.time,
            weather: scene.weather,
            height_above_ground: scene.player_agl,
            body,
            casters: &casters,
            objects: &self.objects,
            glows: &[],
            lamps: &self.lamps,
            see_through: &self.discs,
            wrecks: scene.wrecks,
        };
        self.world.prepare(gpu, &world_scene);
    }

    /// Draws what `prepare` chose. Call from `Game::draw`.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.world.draw_sky(pass);
        let cockpit = match &self.cockpits[self.inside_kind] {
            Cockpit::Ready(cockpit) if !self.inside.is_empty() => Some(cockpit),
            _ => None,
        };
        if let Some(cockpit) = cockpit {
            pass.set_pipeline(&self.pipelines.cabin);
            pass.set_bind_group(2, cockpit.bind(), &[]);
            pass.set_vertex_buffer(0, cockpit.mesh.buffer.slice(..));
            pass.set_vertex_buffer(1, cockpit.occlusion.slice(..));
            for draw in &self.inside {
                if let Inside::Cabin { first, count, uniforms } = *draw {
                    pass.set_bind_group(1, self.inside_uniforms.bind_group(), &[uniforms]);
                    pass.draw(first..first + count, 0..1);
                }
            }
        }
        self.world.draw(pass);
        // The glass over what is seen through it.
        if let Some(cockpit) = cockpit {
            for draw in &self.inside {
                if let Inside::Glass { uniforms } = *draw {
                    pass.set_pipeline(&self.pipelines.glass);
                    pass.set_bind_group(2, cockpit.bind(), &[]);
                    pass.set_vertex_buffer(0, cockpit.glass.buffer.slice(..));
                    pass.set_bind_group(1, self.inside_uniforms.bind_group(), &[uniforms]);
                    pass.draw(0..cockpit.glass.count, 0..1);
                }
            }
        }
    }

    /// The canvas the instruments of `kind`'s cockpit are painted into (`kinds::paint_instruments`).
    pub fn instruments_canvas(&mut self, gpu: &Gpu, kind: AircraftKind) -> &mut engine::Canvas {
        self.instruments_canvas_of(gpu, Craft::Aircraft(kind))
    }

    /// The same of any craft that has a cockpit (`kinds::cabin`).
    pub fn instruments_canvas_of(&mut self, gpu: &Gpu, craft: Craft) -> &mut engine::Canvas {
        let kind = craft.index();
        self.wait_for_cockpit(gpu, kind);
        let Cockpit::Ready(cockpit) = &mut self.cockpits[kind] else { unreachable!("waited for") };
        &mut cockpit.canvas
    }

    /// The cockpits whose bake has come back, put on the GPU. Call once a frame.
    fn receive_cockpits(&mut self, gpu: &Gpu) {
        for cockpit in &mut self.cockpits {
            let Cockpit::Baking(receiver) = cockpit else { continue };
            if let Ok(baked) = receiver.try_recv() {
                *cockpit = Cockpit::Ready(Box::new(CockpitRenderer::new(gpu, &self.cabin_layout, baked)));
            }
        }
    }

    /// Puts the cockpit of the fleet's kind `kind` on the GPU if it is not yet, waiting for its bake:
    /// only when the game starts in it before the bake is done. Baked here if its thread went away.
    fn wait_for_cockpit(&mut self, gpu: &Gpu, kind: usize) {
        if let Cockpit::Baking(receiver) = &self.cockpits[kind] {
            let cabin = Craft::all().nth(kind).and_then(kinds::cabin).expect("a craft with a cockpit");
            let baked = engine::threads::wait(receiver).unwrap_or_else(|_| bake(cabin));
            self.cockpits[kind] = Cockpit::Ready(Box::new(CockpitRenderer::new(gpu, &self.cabin_layout, baked)));
        }
    }

    /// The knobs a mouse can take in `kind`'s cockpit (`CabinSpec::levers`), once it is on the GPU.
    pub fn levers(&self, kind: AircraftKind) -> &[(Lever, [Vec3; 2])] {
        match &self.cockpits[kind_index(kind)] {
            Cockpit::Ready(c) => &c.levers,
            Cockpit::Baking(_) | Cockpit::None => &[],
        }
    }

    /// The keys and knobs a mouse can press and turn in `kind`'s cockpit (`CabinSpec::controls`), once
    /// it is on the GPU.
    pub fn panel_controls(&self, kind: AircraftKind) -> &[PanelControl] {
        match &self.cockpits[kind_index(kind)] {
            Cockpit::Ready(c) => &c.controls,
            Cockpit::Baking(_) | Cockpit::None => &[],
        }
    }

    /// Whether the frame has a second pass, and what goes in it: the effects of the wrecks and what
    /// is see-through (`WorldRenderer::translucent`). Call from `Game::translucent` and `Game::draw_translucent`.
    pub fn translucent(&self) -> bool {
        self.world.translucent()
    }

    pub fn before_translucent(&self, encoder: &mut wgpu::CommandEncoder) {
        self.world.before_translucent(encoder);
    }

    pub fn draw_translucent(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.world.draw_translucent(pass);
    }

    /// The propellers' blurs of the frame last prepared, and the cockpit's parts and glass.
    #[cfg(test)]
    pub fn discs(&self) -> usize {
        self.discs.len()
    }

    #[cfg(test)]
    pub fn cabin_parts(&self) -> usize {
        self.inside.len()
    }

    /// Draw calls of the frame last prepared.
    pub fn draw_calls(&self) -> usize {
        self.world.draw_calls() + self.inside.len()
    }

    /// Tiles still being generated in the background and cockpits still being baked; 0 when the
    /// picture is complete.
    pub fn pending(&self) -> usize {
        self.world.pending() + self.cockpits.iter().filter(|c| matches!(c, Cockpit::Baking(_))).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::wgpu::naga;

    /// From a cockpit the exposure opens for what the eye looks at, all through the dusk as by day,
    /// and at night much less; the rest of the picture is the world's.
    #[test]
    fn the_eye_opens_for_the_panel_until_the_night() {
        let world = engine::Post { exposure: 0.5, adaptation: 0.25, contrast: 0.4, ..Default::default() };
        let spots: Vec<f32> = [1.0, 0.3, 0.15, 0.0].iter().map(|&day| cabin_post(world, day).spot).collect();
        assert_eq!((spots[0], spots[1], spots[3]), (CABIN_SPOT, CABIN_SPOT, CABIN_SPOT_NIGHT));
        assert!(spots[2] < spots[1] && spots[2] > spots[3], "{spots:?}");
        let post = cabin_post(world, 1.0);
        assert_eq!(post, engine::Post { exposure: 0.5 + CABIN_EXPOSURE, spot: CABIN_SPOT, ..world });
    }

    /// A wheel turns at its rolling speed over its radius; off the ground it runs down by itself in
    /// some tens of seconds and braked in a couple, never turning back.
    #[test]
    fn wheels_turn_as_they_roll_and_run_down_in_the_air() {
        assert!((wheel_rate(0.0, Some(30.0), 0.0, 0.2, 0.01) - 150.0).abs() < 1e-4);
        assert!((wheel_rate(150.0, Some(-1.0), 1.0, 0.2, 0.01) + 5.0).abs() < 1e-4);
        let run_down = |brake: f32| {
            let (mut rate, mut t) = (150.0f32, 0.0f32);
            while rate > 0.0 {
                let next = wheel_rate(rate, None, brake, 0.2, 1.0 / 60.0);
                assert!((0.0..rate).contains(&next), "{rate} to {next}");
                (rate, t) = (next, t + 1.0 / 60.0);
            }
            t
        };
        let (free, braked) = (run_down(0.0), run_down(1.0));
        assert!((20.0..60.0).contains(&free) && (1.0..4.0).contains(&braked), "free {free} s, braked {braked} s");
    }

    /// On every kind: with the spring compressed the tyre's contact point and the strut's foot rise
    /// by as much and its root stays; steered, the wheel heads where the simulation's does; rolling
    /// forward its top goes forward; the radius is the tyre's.
    #[test]
    fn legs_compress_steer_and_roll_where_the_simulation_does() {
        for kind in AircraftKind::all() {
            let model = kinds::model(kind).unwrap();
            let spec = kind.spec();
            for (i, (leg, wheel)) in legs(&model).iter().zip(&spec.gear).enumerate() {
                let contact = Vec3::new(wheel.contact.x as f32, wheel.contact.y as f32, wheel.contact.z as f32);
                assert!((leg.axle.z + leg.radius - contact.z).abs() < 0.01, "{kind:?} leg {i}: radius {}", leg.radius);
                assert!(leg.fold.z < leg.axle.z - leg.radius, "{kind:?} leg {i}");
                let [strut, fork, tyre] = leg_matrices(leg, 0.08, 0.0, 0.0);
                assert!((tyre.transform_point3(contact) - (contact - Vec3::Z * 0.08)).length() < 1e-4, "{kind:?} leg {i}");
                assert!((fork.transform_point3(leg.axle) - (leg.axle - Vec3::Z * 0.08)).length() < 1e-4, "{kind:?} leg {i}");
                assert!((strut.transform_point3(leg.root) - leg.root).length() < 1e-4, "{kind:?} leg {i}");
                if leg.reach != Vec3::ZERO {
                    let foot = leg.root + leg.reach;
                    assert!((strut.transform_point3(foot) - (foot - Vec3::Z * 0.08)).length() < 1e-4, "{kind:?} leg {i}");
                }
                if wheel.steers {
                    let steer = spec.steer_angle(wheel, 1.0, 0.0) as f32;
                    let heading = leg_matrices(leg, 0.0, steer, 0.0)[2].transform_vector3(Vec3::X);
                    assert!((heading.y.atan2(heading.x) - steer).abs() < 0.03, "{kind:?}: heads {heading} steered {steer}");
                }
                let top = leg.axle - Vec3::Z * leg.radius;
                assert!(leg_matrices(leg, 0.0, 0.0, 0.5)[2].transform_point3(top).x > top.x + 0.4 * leg.radius, "{kind:?} leg {i}");
            }
        }
    }

    /// Each leg casts its shadow from a rod and two ellipsoids: the tyre's standing on the contact
    /// point and the fairing's round it, both raised with the spring; a collapsed leg casts none.
    #[test]
    fn the_gear_casts_its_shadow_where_it_stands() {
        for kind in AircraftKind::all() {
            let model = kinds::model(kind).unwrap();
            let legs = legs(&model);
            let mut plane = PlaneVisual {
                kind: Craft::Aircraft(kind),
                position: DVec3::ZERO,
                attitude: DQuat::IDENTITY,
                aileron: 0.0,
                elevator: 0.0,
                rudder: 0.0,
                flap: 0.0,
                trim: 0.0,
                rpm: 0.0,
                gear: [0.08; LEGS],
                steer: [0.0; LEGS],
                roll: [None; LEGS],
                brake: [0.0; LEGS],
                gear_down: 1.0,
                damage: Damage::default(),
                rotors: [0.0; MOST_MOTORS],
                props: [1.0; MOST_MOTORS],
                nacelles: [0.0; 2],
                rotor_tilt: [0.0; MOST_MOTORS],
                tilt_pivot: Vec3::ZERO,
                tint: [1.0; 4],
            };
            let solids = gear_solids(&legs, &plane);
            // The first three legs cast one (`gear_solids`); a leg without a strut of its own, no rod.
            let rigid = model.parts.iter().all(|p| !matches!(p.moves, Moves::Strut { .. }));
            for (i, wheel) in kind.spec().gear.iter().enumerate().take(3) {
                let [strut, fairing, tyre] = [solids[3 * i], solids[3 * i + 1], solids[3 * i + 2]];
                assert_eq!((fairing[0][3], tyre[0][3]), (1.0, 1.0), "{kind:?} leg {i}");
                if rigid {
                    continue;
                }
                assert_eq!(strut[0][3], 2.0, "{kind:?} leg {i}");
                let bottom = tyre[0][2] + tyre[1][2];
                assert!((bottom - (wheel.contact.z as f32 - 0.08)).abs() < 0.03, "{kind:?} leg {i}: tyre's bottom at {bottom}");
                assert!((tyre[0][0] - wheel.contact.x as f32).abs() < 0.02 && (tyre[0][1] - wheel.contact.y as f32).abs() < 0.02, "{kind:?} leg {i}");
                assert!(strut[1][3] > 0.0 && strut[1][3] < 0.1, "{kind:?} leg {i}: a rod {} thick", strut[1][3]);
                assert!(fairing[1].iter().take(3).all(|r| *r > 0.0 && *r < 1.0), "{kind:?} leg {i}: {:?}", fairing[1]);
            }
            plane.damage.gear_broken[1] = true;
            assert!(gear_solids(&legs, &plane)[3..6].iter().all(|s| s[0][3] == 0.0), "{kind:?}");
        }
    }

    /// The cockpits' shaders, on the world's of every quality, parse and validate without a GPU:
    /// naga is what wgpu checks them with.
    #[test]
    fn the_cockpits_shaders_are_valid_wgsl() {
        for quality in [Quality::HIGH, Quality::MEDIUM, Quality::LOW, Quality { texture_size: 0, ..Quality::LOW }] {
            let source = cockpit_source(&quality);
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{quality:?}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{quality:?}: {}", e.emit_to_string(&source)));
        }
    }

    /// Each kind casts a shadow of its own.
    #[test]
    fn every_kind_has_a_shadow() {
        for kind in AircraftKind::all() {
            let shape = kinds::shadow(kind);
            assert!(shape.flat.iter().chain(shape.upright.iter()).all(|p| p[1] > 0.0 && p[3] > 0.0), "{kind:?}");
        }
    }
}
