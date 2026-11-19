//! The fire of a crash as a gas: the fireball, the flames of the fuel that burns where the wreck
//! lies and along its slide, the smoke, the dust the impact throws up. A grid of cells round the
//! fire, each holding the air's velocity, fuel, heat, soot and dust, stepped on the GPU
//! (`fire_sim.wgsl`: what a step is and whose methods it uses) and ray marched into the picture
//! (`fire_draw.wgsl`).
//!
//! The simulation (`sim_core::Wreck`) says what burns; `emitters` says, as a pure function of the
//! wreck and the time since the crash, what goes into the grid at that moment: fuel thrown out and
//! lit, the pool's vapour, dust. What the gas then does is the grid's own, and is the one thing
//! the drawing of a wreck keeps from frame to frame: seen late (a view that arrives, a picture
//! taken some seconds after the crash) it is played from the crash, or from `REPLAY` seconds back,
//! at `SLOTS` steps a frame until it has caught up.
//!
//! The grid follows the fire: it starts round the impact and goes, a cell a step at most, to where
//! the main piece burns (`Grid::follow`). The ground, the buildings and the trees under it come
//! from a map made once (`Land`), wide enough for the grid to move over.

use crate::effect_pipelines::{Drawing, Target};
use crate::particles::{blended_pipeline, depth_entry, EffectUniforms};
use crate::{shader_source, to_render, Quality};
use bytemuck::Zeroable;
use engine::{wgpu, Gpu};
use glam::{DVec3, Vec3};
use sim_core::scenery::{self, Shape};
use sim_core::{smoothstep, Terrain, Wreck};
use std::mem::size_of;
use std::sync::Arc;

/// Cells of the grid: east, up, south (the render axes).
pub const CELLS: [u32; 3] = [80, 112, 80];
/// A cell's side, m: the grid is 68 m wide and 95 m high.
pub const CELL: f64 = 0.85;
/// A step of the simulation, s.
pub const STEP: f64 = 1.0 / 60.0;
/// Fires farther than this from the camera are not simulated, m.
pub const RANGE: f64 = 4000.0;
/// Sweeps of the pressure in a step, each over both colours of the chessboard.
const SWEEPS: u32 = 5;
/// Emitters at most at a time.
const MOST_EMITTERS: usize = 48;
/// Steps at most in a frame: while it catches up. Once it has, `STEADY`.
const SLOTS: usize = 30;
const STEADY: i64 = 4;
/// Seconds played at most to show a fire first seen late.
const REPLAY: f64 = 45.0;
/// The fireball's smoke is simulated this long, and a fire's this long after it is out, s.
const FIREBALL_LASTS: f64 = 50.0;
const SMOKES_AFTER: f64 = 60.0;
/// The map of the ground: texels a side, and cells to a texel (1.7 m; 218 m a side).
const MAP: u32 = 128;
const MAP_CELLS: u32 = 2;
/// The smoke fades out over this many cells under the grid's top, where the column of particles takes over.
const TOP_FADE: f64 = 16.0;

const UP: DVec3 = DVec3::new(0.0, 0.0, -1.0);

/// Something that feeds the grid at a moment, in world axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitter {
    pub at: DVec3,
    /// Radius, m.
    pub radius: f64,
    /// The velocity it gives the gas, m/s, and the speed outwards from its middle at its rim.
    pub push: DVec3,
    pub burst: f64,
    /// Fuel, soot and dust a second (1: a cell filled with a rich mist in a second).
    pub fuel: f32,
    pub soot: f32,
    pub dust: f32,
    /// The heat it lights the gas to, thousands of kelvin above the air (0: cold).
    pub heat: f32,
    /// How ragged its outline is, 0..1, and how fast the gas takes its velocity, 1/s.
    pub ragged: f32,
    pub grip: f32,
    /// A number 0..1 of its own.
    pub seed: f32,
}

/// The fuel of a fireball's widest ball, m: 5.8 m across per kg^⅓ (the fit for hydrocarbon
/// fireballs, Roberts 1982), no wider than the grid can hold.
pub fn fireball_radius(fuel: f64) -> f64 {
    (2.9 * fuel.cbrt()).min(16.0)
}

fn hash(i: u32, k: u32) -> f64 {
    crate::effects::hash(i, k) as f64
}

/// What feeds the fire of `wreck` `since` seconds after its crash; `ground` is the height of the
/// ground where it came apart.
pub fn emitters(wreck: &Wreck, since: f64, ground: f64, out: &mut Vec<Emitter>) {
    out.clear();
    if since < 0.0 {
        return;
    }
    let seed = (wreck.time * 240.0) as u32 ^ (wreck.impact.y.to_bits() as u32).rotate_left(9);
    let in_water = wreck.splashes.first().is_some_and(|s| s.time == 0.0);
    let along = DVec3::new(wreck.impact_vel.x, wreck.impact_vel.y, 0.0);
    let ahead = along.normalize_or_zero();
    // Nothing is fed from under the ground.
    let low = -(ground + 0.6);
    let lifted = |p: DVec3| DVec3::new(p.x, p.y, p.z.min(low));

    // The fireball: the fuel bursts out as a mist in a few lobes, upwards and the way the aircraft
    // was going, already alight; its burning then blows it up to its size and lifts it.
    const BURST: f64 = 0.42;
    if wreck.fireball > 0.0 && since < BURST {
        let radius = fireball_radius(wreck.fireball);
        let k = since / BURST;
        let centre = lifted(wreck.impact) + UP * radius * 0.28;
        let rate = (1.0 - k * k) as f32;
        out.push(Emitter { at: centre, radius: radius * 0.5, push: UP * radius * 0.4 + along * 0.12, burst: radius * 1.1 * (1.0 - k), fuel: 19.0 * rate, soot: 0.6, dust: 0.0, heat: 2.0, ragged: 0.4, grip: 30.0, seed: 0.13 });
    }
    // And in many smaller bursts round it, one after another over a quarter of a second, each
    // thrown its own way: the ball is lumps on lumps, not one round cloud.
    if wreck.fireball > 0.0 && since < 0.6 {
        let radius = fireball_radius(wreck.fireball);
        let centre = lifted(wreck.impact) + UP * radius * 0.28;
        for i in 0..14u32 {
            let from = i as f64 * 0.016 + 0.05 * hash(seed, i);
            let lasts = 0.16 + 0.14 * hash(seed, i + 80);
            if since < from || since >= from + lasts {
                continue;
            }
            let k = (since - from) / lasts;
            let a = hash(seed, i + 90) * std::f64::consts::TAU;
            let throw = (DVec3::new(a.cos(), a.sin(), 0.0) * (0.3 + 0.7 * hash(seed, i + 10)) + UP * (0.2 + 0.9 * hash(seed, i + 20)) + ahead * 0.45).normalize();
            let reach = radius * (0.2 + 0.4 * hash(seed, i + 30));
            out.push(Emitter {
                at: lifted(centre + throw * reach * (0.7 + 0.6 * k)),
                radius: radius * (0.2 + 0.12 * hash(seed, i + 40)),
                push: throw * radius * (0.6 + 0.7 * hash(seed, i + 50)) * (1.0 - 0.6 * k) + along * 0.2,
                burst: radius * 0.7 * (1.0 - k),
                fuel: (13.0 + 9.0 * hash(seed, i + 60) as f32) * (1.0 - k * k) as f32,
                soot: 0.5,
                dust: 0.0,
                heat: 2.0,
                ragged: 0.6,
                grip: 24.0,
                seed: hash(seed, i + 70) as f32,
            });
        }
    }

    // What of the fuel fell back burns on the ground for a few seconds: the fireball's foot, and
    // the stem of smoke it leaves under it as it rises.
    const STEM: f64 = 5.0;
    if wreck.fireball > 0.0 && (BURST..STEM).contains(&since) {
        let radius = fireball_radius(wreck.fireball);
        let left = (1.0 - (since - BURST) / (STEM - BURST)) as f32;
        out.push(Emitter { at: lifted(wreck.impact) + UP * 0.5 + along * 0.05, radius: radius * 0.3, push: UP * 2.5, burst: 0.0, fuel: 1.1 * left, soot: 2.2 * left, dust: if in_water { 0.0 } else { 6.0 * left }, heat: 1.6, ragged: 0.8, grip: 2.5, seed: 0.47 });
    }

    // The earth the impact throws up: a skirt blown out along the ground and a cloud over the hole.
    if wreck.severity > 12.0 && !in_water && -wreck.impact.z - ground < 8.0 && since < 1.0 {
        let p = (wreck.severity / 40.0).clamp(0.4, 1.6);
        let k = since;
        let foot = DVec3::new(wreck.impact.x, wreck.impact.y, -ground - 0.8);
        out.push(Emitter { at: foot + UP * 2.5, radius: 4.5 * p, push: UP * 7.0 * p + along * 0.15, burst: 5.0, fuel: 0.0, soot: 0.0, dust: 16.0 * (1.0 - k) as f32, heat: 0.0, ragged: 0.5, grip: 12.0, seed: 0.71 });
        for i in 0..6u32 {
            let a = (i as f64 + hash(seed, i + 100)) * std::f64::consts::TAU / 6.0;
            let away = DVec3::new(a.cos(), a.sin(), 0.0);
            out.push(Emitter {
                at: foot + UP * 1.0 + away * (3.0 + 5.0 * k) * p,
                radius: 3.2 * p,
                push: away * (6.0 + 5.0 * hash(seed, i + 110)) * p + UP * 4.0 + along * 0.2,
                burst: 2.0,
                fuel: 0.0,
                soot: 0.0,
                dust: 12.0 * (1.0 - k) as f32,
                heat: 0.0,
                ragged: 0.6,
                grip: 10.0,
                seed: hash(seed, i + 120) as f32,
            });
        }
    }

    // The fuel that burns where the main piece lies: its vapour rising off the pool, lit, from a
    // few places that wander about it; once it is out, the smoke of what smoulders.
    if let Some(fire) = wreck.fire {
        let main = wreck.main();
        let pool = lifted(main.pos + UP * 0.2);
        let burning = match fire.out {
            None => (since / 2.0).min(1.0) * (0.35 + 0.65 * (fire.fuel / 20.0).min(1.0)),
            Some(_) => 0.0,
        };
        if burning > 0.0 && !main.sunk {
            for i in 0..4u32 {
                let turn = since * (0.5 + 0.23 * i as f64) + i as f64 * 2.1;
                let off = DVec3::new(turn.cos(), (turn * 1.3).sin(), 0.0) * (0.4 + 0.45 * i as f64);
                out.push(Emitter {
                    at: pool + off,
                    radius: 1.1 + 0.4 * burning + 0.1 * i as f64,
                    push: UP * (1.5 + burning) + main.vel * 0.5,
                    burst: 0.0,
                    fuel: (0.55 + 0.35 * (turn * 3.7).sin() as f32) * burning as f32,
                    soot: 6.0 * burning as f32,
                    dust: 0.0,
                    heat: 1.7,
                    ragged: 0.9,
                    grip: 3.0,
                    seed: 0.2 + 0.19 * i as f32,
                });
            }
        }
        if let Some(out_at) = fire.out {
            let smoulders = (1.0 - (since + wreck.time - out_at) / SMOKES_AFTER).clamp(0.0, 1.0);
            if smoulders > 0.0 && !main.sunk {
                out.push(Emitter { at: pool, radius: 1.6, push: UP * 1.2, burst: 0.0, fuel: 0.0, soot: 0.5 * smoulders as f32, dust: 0.0, heat: 0.3 * smoulders as f32, ragged: 0.7, grip: 2.0, seed: 0.9 });
            }
        }
    }

    // Fuel spilt along the slide burns where it lies.
    for (i, scar) in wreck.scars.iter().enumerate() {
        if scar.crater > 0.0 || since < scar.made || since >= scar.burns_until || out.len() >= MOST_EMITTERS {
            continue;
        }
        let strength = (((scar.burns_until - since) / 6.0).min(1.0) * ((since - scar.made) / 0.5).min(1.0)) as f32;
        let flicker = 0.75 + 0.25 * (since * 5.3 + i as f64 * 1.9).sin() as f32;
        out.push(Emitter { at: scar.pos + UP * 0.3, radius: (scar.radius * 0.5).clamp(1.0, 2.5), push: UP * 1.5, burst: 0.0, fuel: 1.1 * strength * flicker, soot: 0.1 * strength, dust: 0.0, heat: 1.6, ragged: 0.75, grip: 3.0, seed: hash(seed, i as u32 + 200) as f32 });
    }
    out.truncate(MOST_EMITTERS);
}

/// The ground under and round a fire, as the grid needs it: per texel of `MAP_CELLS` cells, the top
/// of what is solid (the ground, the water, a building), the foot and the top of the trees' crowns
/// and whether there is one, heights in metres above `zero`.
struct Land {
    /// World east and south (minus north) of the map's corner, and the height its heights are from.
    east: f64,
    south: f64,
    zero: f64,
    texels: Vec<[f32; 4]>,
}

impl Land {
    const STEP: f64 = MAP_CELLS as f64 * CELL;
    const SIDE: f64 = MAP as f64 * Land::STEP;

    /// The map round `middle` (north, east).
    fn new(terrain: &Terrain, middle: (f64, f64), zero: f64) -> Land {
        let (east, south) = (middle.1 - Land::SIDE / 2.0, -middle.0 - Land::SIDE / 2.0);
        let mut land = Land { east, south, zero, texels: vec![[0.0; 4]; (MAP * MAP) as usize] };
        for j in 0..MAP {
            for i in 0..MAP {
                let (n, e) = land.place(i, j);
                let mut top = terrain.height(n, e).max(0.0);
                if let Some(roof) = terrain.solid_top(n, e) {
                    top = top.max(roof);
                }
                land.texels[(j * MAP + i) as usize] = [(top - zero) as f32, 0.0, 0.0, 0.0];
            }
        }
        let tiles = |size: f64| {
            let (i0, i1) = ((east / size).floor() as i32, ((east + Land::SIDE) / size).floor() as i32);
            let (j0, j1) = ((south / size).floor() as i32, ((south + Land::SIDE) / size).floor() as i32);
            (i0..=i1).flat_map(move |i| (j0..=j1).map(move |j| (i, j)))
        };
        let mut standing = Vec::new();
        for key in tiles(scenery::TREE_TILE) {
            standing.extend(scenery::tree_obstacles(terrain, key, scenery::TREE_TILE));
        }
        for key in tiles(scenery::HOUSE_TILE) {
            standing.extend(scenery::house_obstacles(terrain, key, scenery::HOUSE_TILE));
        }
        for o in &standing {
            let (reach, height) = o.extent();
            if reach <= 0.0 {
                continue;
            }
            let span = |from: f64, to: f64| ((from / Land::STEP).floor().max(0.0) as u32)..(((to / Land::STEP).ceil().max(0.0) as u32).min(MAP));
            for j in span(-o.n - reach - south, -o.n + reach - south) {
                for i in span(o.e - reach - east, o.e + reach - east) {
                    let (n, e) = land.place(i, j);
                    let texel = &mut land.texels[(j * MAP + i) as usize];
                    match o.shape {
                        Shape::Tree { crown, radius, top, .. } => {
                            // Air goes through a crown, slowly.
                            if (n - o.n).hypot(e - o.e) < radius {
                                *texel = [texel[0], (o.base + crown - zero) as f32, (o.base + top - zero) as f32, 1.0];
                            }
                        }
                        Shape::Block { .. } => {
                            // The highest of a few heights that are inside it.
                            if let Some(k) = (1..=8).rev().find(|&k| o.contains(n, e, o.base + height * k as f64 / 8.0 - 0.05)) {
                                texel[0] = texel[0].max((o.base + height * k as f64 / 8.0 - zero) as f32);
                            }
                        }
                        Shape::Bar { .. } | Shape::Mapped => {}
                    }
                }
            }
        }
        land
    }

    /// North and east of a texel's middle.
    fn place(&self, i: u32, j: u32) -> (f64, f64) {
        (-(self.south + (j as f64 + 0.5) * Land::STEP), self.east + (i as f64 + 0.5) * Land::STEP)
    }

    /// The lowest solid top under a square of the grid's size whose corner is at these cells
    /// (east, south) of the map's, m above `zero`.
    fn lowest(&self, corner: (i32, i32)) -> f64 {
        let span = |from: i32, cells: u32| (from.max(0) as u32 / MAP_CELLS)..((from.max(0) as u32 + cells).div_ceil(MAP_CELLS).min(MAP));
        let mut lowest = f32::MAX;
        for j in span(corner.1, CELLS[2]) {
            for i in span(corner.0, CELLS[0]) {
                lowest = lowest.min(self.texels[(j * MAP + i) as usize][0]);
            }
        }
        lowest as f64
    }
}

/// Where the grid is: its corner in cells east, up and south of the map's corner (and of its zero).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grid {
    corner: [i32; 3],
}

impl Grid {
    /// The point the grid should be round `since` seconds after the crash: the fireball at first,
    /// then where the main piece burns.
    fn focus(wreck: &Wreck, since: f64) -> DVec3 {
        match wreck.fire {
            // (a dozen metres towards the main piece from the start: the fireball still fits, and
            // the fire that starts where the piece has slid to is in the grid too)
            Some(_) if wreck.fireball > 0.0 => {
                let main = wreck.main().pos;
                (wreck.impact + (main - wreck.impact).clamp_length_max(12.0)).lerp(main, smoothstep(3.5, 9.0, since))
            }
            Some(_) => wreck.main().pos,
            None => wreck.impact,
        }
    }

    /// The grid round a point, as far as the map lets it.
    fn round(land: &Land, focus: DVec3) -> Grid {
        let most = (MAP * MAP_CELLS) as i32;
        let x = (((focus.y - land.east) / CELL).round() as i32 - CELLS[0] as i32 / 2).clamp(0, most - CELLS[0] as i32);
        let z = (((-focus.x - land.south) / CELL).round() as i32 - CELLS[2] as i32 / 2).clamp(0, most - CELLS[2] as i32);
        // Its floor a cell under the lowest ground it stands on.
        let y = (land.lowest((x, z)) / CELL).floor() as i32 - 1;
        Grid { corner: [x, y, z] }
    }

    /// A step towards `to`, a cell at most each way; and how many cells it moved.
    fn follow(&mut self, to: Grid) -> [i32; 3] {
        let by = std::array::from_fn(|i| (to.corner[i] - self.corner[i]).clamp(-1, 1));
        for (corner, by) in self.corner.iter_mut().zip(by) {
            *corner += by;
        }
        by
    }

    /// A world point in cells from the grid's corner (east, up, south).
    fn cells(&self, land: &Land, p: DVec3) -> Vec3 {
        Vec3::new(((p.y - land.east) / CELL - self.corner[0] as f64) as f32, ((-p.z - land.zero) / CELL - self.corner[1] as f64) as f32, ((-p.x - land.south) / CELL - self.corner[2] as f64) as f32)
    }

    /// The grid's corner in the world.
    fn origin(&self, land: &Land) -> DVec3 {
        DVec3::new(-(land.south + self.corner[2] as f64 * CELL), land.east + self.corner[0] as f64 * CELL, -(land.zero + self.corner[1] as f64 * CELL))
    }
}

/// `struct Step` in fire_sim.wgsl. A slot of the steps' buffer holds one, padded to the alignment
/// a uniform's offset must have.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StepUniforms {
    shift: [i32; 4],
    map: [f32; 4],
    wind: [f32; 4],
    place: [f32; 4],
    pad: [f32; 48],
}

const _: () = assert!(size_of::<StepUniforms>() == 256);

/// `struct Emitter` in fire_sim.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuEmitter {
    place: [f32; 4],
    push: [f32; 4],
    gives: [f32; 4],
    more: [f32; 4],
}

/// A slot of the emitters' buffer, bytes: a multiple of the alignment a storage buffer's offset must have.
const EMITTER_SLOT: u64 = (MOST_EMITTERS * size_of::<GpuEmitter>()) as u64;
const _: () = assert!(EMITTER_SLOT.is_multiple_of(256));

/// `struct Lit` in fire_sim.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LightUniforms {
    sun: [f32; 4],
}

/// `struct FireView` in fire_draw.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniforms {
    corner: [f32; 4],
    place: [f32; 4],
    size: [f32; 4],
}

/// The grid's numbers as the shaders read them, heat.wgsl and fire_common.wgsl.
fn shader_prefix() -> String {
    format!(
        "const NX: i32 = {};\nconst NY: i32 = {};\nconst NZ: i32 = {};\nconst CELL: f32 = {CELL:?};\nconst DT: f32 = {STEP:?};\nconst MAP: f32 = {MAP}.0;\nconst MAP_CELLS: f32 = {MAP_CELLS}.0;\n{}\n{}\n",
        CELLS[0],
        CELLS[1],
        CELLS[2],
        include_str!("heat.wgsl"),
        include_str!("fire_common.wgsl")
    )
}

/// The simulation's shader.
pub fn sim_source() -> String {
    shader_prefix() + include_str!("fire_sim.wgsl")
}

/// How the grid is marched for a quality: cells a step, steps at most, and the scene's pixels to
/// one of the march's each way.
fn march(quality: &Quality) -> (f32, u32, u32) {
    match quality.fire_detail {
        0 | 1 => (1.5, 120, 2),
        _ => (1.0, 200, 2),
    }
}

/// The drawing's shader on the world's, for a quality and `samples` per pixel of the scene.
pub fn draw_source(quality: &Quality, samples: u32) -> String {
    let (step, most, _) = march(quality);
    let source = shader_source(quality) + &shader_prefix() + &format!("const MARCH_STEP: f32 = {step:?};\nconst MARCH_MOST: i32 = {most};\n") + include_str!("fire_draw.wgsl");
    if samples > 1 {
        source
    } else {
        source.replace("texture_depth_multisampled_2d", "texture_depth_2d")
    }
}

/// The simulation's pipelines: made once, in the background (`effect_pipelines.rs`).
pub struct Simulation {
    whirl: wgpu::ComputePipeline,
    carry: wgpu::ComputePipeline,
    spread: wgpu::ComputePipeline,
    settle: [wgpu::ComputePipeline; 2],
    release: wgpu::ComputePipeline,
    lighting: wgpu::ComputePipeline,
    glow_spread: wgpu::ComputePipeline,
}

impl Simulation {
    pub fn new(device: &wgpu::Device) -> Simulation {
        let module = engine::pipeline::shader_with_ending_loops(device, "fire", &sim_source());
        let make = |entry: &str| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry), layout: None, module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: None });
        Simulation { whirl: make("whirl"), carry: make("carry"), spread: make("spread"), settle: [make("settle_white"), make("settle_black")], release: make("release"), lighting: make("lighting"), glow_spread: make("glow_spread") }
    }
}

const GRID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Levels of the fire's light: 48 x 64 x 48 texels down to 3 x 4 x 3.
const GLOW_LEVELS: u32 = 5;

fn volume(device: &wgpu::Device, label: &str, size: [u32; 3], levels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: size[2] },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: GRID_FORMAT,
        // Copied from only by the tests, which read the gas back.
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn tex(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) }
}

fn buf(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
}

fn smp(binding: u32, sampler: &wgpu::Sampler) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::Sampler(sampler) }
}

/// The bind groups of a step, but `carry`'s second (one per slot).
struct StepBinds {
    whirl: wgpu::BindGroup,
    /// By which of the two textures of gas holds the step before.
    carry: [wgpu::BindGroup; 2],
    spread: wgpu::BindGroup,
    settle: [wgpu::BindGroup; 2],
    release: wgpu::BindGroup,
    lighting: [wgpu::BindGroup; 2],
    lighting_more: wgpu::BindGroup,
    glow_spread: Vec<(wgpu::BindGroup, [u32; 3])>,
    glow_more: wgpu::BindGroup,
}

/// What the drawing has made for one size of picture and set of pipelines.
struct DrawBinds {
    targets_made: u64,
    drawing: Arc<Drawing>,
    /// Which texture of gas `march` reads.
    gas: usize,
    colour: wgpu::TextureView,
    reached: wgpu::TextureView,
    march: wgpu::BindGroup,
    over: wgpu::BindGroup,
}

/// The fire of one wreck.
pub struct Fire {
    simulation: Arc<Simulation>,
    /// The wreck it is the fire of: the time and the place of its crash.
    wreck: (f64, DVec3),
    land: Land,
    grid: Grid,
    /// Steps done since the crash, and whether any was.
    done: i64,
    started: bool,
    caught_up: bool,
    /// Which texture of gas holds the last step.
    gas_now: usize,
    gas: [wgpu::TextureView; 2],
    light: wgpu::TextureView,
    glow: wgpu::TextureView,
    noise: wgpu::TextureView,
    steps: wgpu::Buffer,
    emitters: wgpu::Buffer,
    light_uniforms: wgpu::Buffer,
    view_uniforms: wgpu::Buffer,
    binds: StepBinds,
    slots: Vec<wgpu::BindGroup>,
    clamp: wgpu::Sampler,
    draw: Option<DrawBinds>,
    /// The wind, world axes, and the camera, as of the last `advance`.
    wind: DVec3,
    eye: DVec3,
    time: f64,
    scratch: Vec<Emitter>,
    /// Height above the fire's foot up to which the grid draws the smoke, m: the column of
    /// particles goes on from there (`effects.rs`).
    pub top: f64,
}

impl Fire {
    /// The fire of `wreck`, nothing of it simulated yet. `ground` is the height of the ground where it came apart.
    pub fn new(gpu: &Gpu, simulation: &Arc<Simulation>, noise: &wgpu::TextureView, wreck: &Wreck, ground: f64, terrain: &Terrain) -> Fire {
        let d = &gpu.device;
        // The map reaches further the way the wreck slides.
        let slide = DVec3::new(wreck.impact_vel.x, wreck.impact_vel.y, 0.0);
        let middle = wreck.impact + slide.normalize_or_zero() * (slide.length() * 0.8).min(45.0);
        let land = Land::new(terrain, (middle.x, middle.y), ground);
        let grid = Grid::round(&land, wreck.impact);

        let map = d.create_buffer(&wgpu::BufferDescriptor { label: Some("fire ground"), size: (land.texels.len() * 16) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        gpu.queue.write_buffer(&map, 0, bytemuck::cast_slice(&land.texels));

        let half = [CELLS[0] / 2, CELLS[1] / 2, CELLS[2] / 2];
        let view = |t: wgpu::Texture| t.create_view(&Default::default());
        let flow = [view(volume(d, "fire flow", CELLS, 1)), view(volume(d, "fire flow carried", CELLS, 1))];
        let gas = [view(volume(d, "fire gas", CELLS, 1)), view(volume(d, "fire gas", CELLS, 1))];
        let whirl = view(volume(d, "fire whirl", CELLS, 1));
        let light = view(volume(d, "fire light", half, 1));
        let glow_texture = volume(d, "fire glow", half, GLOW_LEVELS);
        let glow_level = |level: u32| glow_texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() });
        let cells = (CELLS[0] * CELLS[1] * CELLS[2]) as u64;
        let numbers = |label: &str| d.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: cells * 4, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let (pressure, spreading) = (numbers("fire pressure"), numbers("fire spreading"));
        let steps = d.create_buffer(&wgpu::BufferDescriptor { label: Some("fire steps"), size: (SLOTS * size_of::<StepUniforms>()) as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let emitters = d.create_buffer(&wgpu::BufferDescriptor { label: Some("fire emitters"), size: SLOTS as u64 * EMITTER_SLOT, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let uniforms = |label: &str, size: usize| d.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let light_uniforms = uniforms("fire light", size_of::<LightUniforms>());
        let view_uniforms = uniforms("fire view", size_of::<ViewUniforms>());
        let sampler = |mode: wgpu::AddressMode| {
            d.create_sampler(&wgpu::SamplerDescriptor { label: Some("fire"), address_mode_u: mode, address_mode_v: mode, address_mode_w: mode, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, ..Default::default() })
        };
        let (clamp, repeat) = (sampler(wgpu::AddressMode::ClampToEdge), sampler(wgpu::AddressMode::Repeat));

        let s = simulation;
        let group = |pipeline: &wgpu::ComputePipeline, index: u32, entries: &[wgpu::BindGroupEntry]| d.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("fire"), layout: &pipeline.get_bind_group_layout(index), entries });
        let smooth = wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&clamp) };
        let binds = StepBinds {
            whirl: group(&s.whirl, 0, &[tex(0, &flow[0]), tex(5, &whirl)]),
            carry: std::array::from_fn(|was| group(&s.carry, 0, &[tex(0, &flow[0]), tex(1, &gas[was]), tex(2, &whirl), tex(3, &flow[1]), tex(4, &gas[1 - was])])),
            spread: group(&s.spread, 0, &[tex(0, &flow[1]), buf(7, &spreading)]),
            settle: std::array::from_fn(|colour| group(&s.settle[colour], 0, &[buf(6, &pressure), buf(7, &spreading)])),
            release: group(&s.release, 0, &[tex(0, &flow[1]), tex(3, &flow[0]), buf(6, &pressure), buf(7, &spreading)]),
            lighting: std::array::from_fn(|now| group(&s.lighting, 0, &[tex(1, &gas[now]), tex(8, &light), tex(9, &glow_level(0))])),
            lighting_more: group(&s.lighting, 1, &[smooth.clone(), buf(6, &light_uniforms)]),
            glow_spread: (1..GLOW_LEVELS).map(|level| (group(&s.glow_spread, 0, &[tex(10, &glow_level(level - 1)), tex(9, &glow_level(level))]), half.map(|n| (n >> level).max(1)))).collect(),
            glow_more: group(&s.glow_spread, 1, std::slice::from_ref(&smooth)),
        };
        let slots = (0..SLOTS as u64)
            .map(|slot| {
                group(
                    &s.carry,
                    1,
                    &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &steps, offset: slot * size_of::<StepUniforms>() as u64, size: wgpu::BufferSize::new(64) }) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &emitters, offset: slot * EMITTER_SLOT, size: wgpu::BufferSize::new(EMITTER_SLOT) }) },
                        buf(2, &map),
                        tex(3, noise),
                        smooth.clone(),
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&repeat) },
                    ],
                )
            })
            .collect();
        Fire {
            simulation: simulation.clone(),
            wreck: (wreck.time, wreck.impact),
            land,
            grid,
            done: 0,
            started: false,
            caught_up: false,
            gas_now: 0,
            gas,
            light,
            glow: glow_texture.create_view(&Default::default()),
            noise: noise.clone(),
            steps,
            emitters,
            light_uniforms,
            view_uniforms,
            binds,
            slots,
            clamp,
            draw: None,
            wind: DVec3::ZERO,
            eye: DVec3::ZERO,
            time: 0.0,
            scratch: Vec::new(),
            top: 0.0,
        }
    }

    /// Whether this is the fire of `wreck`.
    pub fn is_for(&self, wreck: &Wreck) -> bool {
        self.wreck == (wreck.time, wreck.impact)
    }

    /// Whether `wreck` has a fire to simulate `since` seconds after its crash: a fireball's smoke
    /// still near, fuel burning, or the smoke of a fire not long out.
    pub fn alive(wreck: &Wreck, since: f64) -> bool {
        if since < 0.0 {
            return false;
        }
        let fireball = wreck.fireball > 0.0 && since < FIREBALL_LASTS;
        let fire = wreck.fire.is_some_and(|f| f.out.is_none_or(|out| since + wreck.time < out + SMOKES_AFTER));
        fireball || fire
    }

    /// The middle of the grid, world axes.
    pub fn centre(&self) -> DVec3 {
        self.grid.origin(&self.land) + DVec3::new(-(CELLS[2] as f64), CELLS[0] as f64, -(CELLS[1] as f64)) * (CELL / 2.0)
    }

    /// Plays the fire up to `since` seconds after the crash of `wreck` (as many steps as a frame
    /// may), and makes its light ready for the drawing. `ground` is the height of the ground where
    /// it came apart; `wind` in world axes, m/s; `sun` towards the sun or the moon, render axes.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(&mut self, gpu: &Gpu, wreck: &Wreck, since: f64, ground: f64, wind: DVec3, sun: Vec3, eye: DVec3) {
        (self.wind, self.eye, self.time) = (wind, eye, since);
        let due = (since / STEP).floor() as i64;
        let earliest = due - (REPLAY / STEP) as i64;
        // Shown from an earlier moment (a replay), or for the first time long after: from nothing.
        let mut fresh = !self.started || due < self.done || earliest > self.done;
        if fresh {
            self.done = earliest.max(0);
            self.grid = Grid::round(&self.land, Grid::focus(wreck, self.done as f64 * STEP));
            self.caught_up = false;
        }
        let behind = due - self.done;
        let steps = behind.min(if self.caught_up { STEADY } else { SLOTS as i64 }).max(0);
        // Far behind though it had caught up (the game stood still a while): go on from here.
        if self.caught_up && behind > STEADY {
            self.done = due - steps;
        }
        self.caught_up |= steps == behind;

        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("fire") });
        let mut uniforms = Vec::with_capacity(steps as usize);
        let mut fed = vec![GpuEmitter::zeroed(); steps as usize * MOST_EMITTERS];
        let breeze = to_render(wind);
        for k in 0..steps as usize {
            let t = (self.done + 1) as f64 * STEP;
            let shift = if fresh { [0; 3] } else { self.grid.follow(Grid::round(&self.land, Grid::focus(wreck, t))) };
            emitters(wreck, t, ground, &mut self.scratch);
            for (e, slot) in self.scratch.iter().zip(&mut fed[k * MOST_EMITTERS..]) {
                let at = self.grid.cells(&self.land, e.at);
                let push = to_render(e.push);
                *slot = GpuEmitter { place: [at.x, at.y, at.z, (e.radius / CELL) as f32], push: [push.x, push.y, push.z, e.burst as f32], gives: [e.fuel, e.heat, e.soot, e.dust], more: [e.ragged, e.seed, e.grip, 0.0] };
            }
            let c = self.grid.corner;
            uniforms.push(StepUniforms {
                shift: [shift[0], shift[1], shift[2], i32::from(fresh)],
                map: [c[0] as f32 / MAP_CELLS as f32, c[2] as f32 / MAP_CELLS as f32, c[1] as f32 * CELL as f32, self.scratch.len() as f32],
                wind: [breeze.x, breeze.y, breeze.z, t as f32],
                place: self.still(t),
                pad: [0.0; 48],
            });
            self.done += 1;
            (fresh, self.started) = (false, true);
        }
        if steps > 0 {
            gpu.queue.write_buffer(&self.steps, 0, bytemuck::cast_slice(&uniforms));
            gpu.queue.write_buffer(&self.emitters, 0, bytemuck::cast_slice(&fed));
        }
        gpu.queue.write_buffer(&self.light_uniforms, 0, bytemuck::bytes_of(&LightUniforms { sun: [sun.x, sun.y, sun.z, 0.0] }));
        {
            let s = &self.simulation;
            let b = &self.binds;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("fire"), timestamp_writes: None });
            let all = CELLS.map(|n| n.div_ceil(4));
            for slot in 0..steps as usize {
                pass.set_pipeline(&s.whirl);
                pass.set_bind_group(0, &b.whirl, &[]);
                pass.dispatch_workgroups(all[0], all[1], all[2]);
                pass.set_pipeline(&s.carry);
                pass.set_bind_group(0, &b.carry[self.gas_now], &[]);
                pass.set_bind_group(1, &self.slots[slot], &[]);
                pass.dispatch_workgroups(all[0], all[1], all[2]);
                self.gas_now = 1 - self.gas_now;
                pass.set_pipeline(&s.spread);
                pass.set_bind_group(0, &b.spread, &[]);
                pass.dispatch_workgroups(all[0], all[1], all[2]);
                for _ in 0..SWEEPS {
                    for colour in 0..2 {
                        pass.set_pipeline(&s.settle[colour]);
                        pass.set_bind_group(0, &b.settle[colour], &[]);
                        pass.dispatch_workgroups(all[0].div_ceil(2), all[1], all[2]);
                    }
                }
                pass.set_pipeline(&s.release);
                pass.set_bind_group(0, &b.release, &[]);
                pass.dispatch_workgroups(all[0], all[1], all[2]);
            }
            pass.set_pipeline(&s.lighting);
            pass.set_bind_group(0, &b.lighting[self.gas_now], &[]);
            pass.set_bind_group(1, &b.lighting_more, &[]);
            pass.dispatch_workgroups(all[0].div_ceil(2), all[1].div_ceil(2), all[2].div_ceil(2));
            pass.set_pipeline(&s.glow_spread);
            pass.set_bind_group(1, &b.glow_more, &[]);
            for (bind, size) in &b.glow_spread {
                pass.set_bind_group(0, bind, &[]);
                pass.dispatch_workgroups(size[0].div_ceil(4), size[1].div_ceil(4), size[2].div_ceil(4));
            }
        }
        gpu.queue.submit([encoder.finish()]);

        let foot = wreck.fire.map_or(wreck.impact, |_| wreck.main().pos + UP * 0.4);
        let roof = self.land.zero + (self.grid.corner[1] as f64 + CELLS[1] as f64) * CELL;
        self.top = roof - TOP_FADE * CELL * 0.6 - (-foot.z);
    }

    /// What keeps the noise still in the world at time `t`: the grid's corner and what the wind has
    /// carried the air, in cells, each wrapped where the noise repeats whatever it is scaled by.
    fn still(&self, t: f64) -> [f32; 4] {
        // The noise is read at scales of 19 and 57 m, 40 and 56 cells to its side: a common period in cells.
        const PERIOD: f64 = 15_960.0;
        let carried = to_render(self.wind).as_dvec3() * t / CELL;
        let c = self.grid.corner;
        [(c[0] as f64 - carried.x).rem_euclid(PERIOD) as f32, (c[1] as f64 - carried.y).rem_euclid(PERIOD) as f32, (c[2] as f64 - carried.z).rem_euclid(PERIOD) as f32, t as f32]
    }

    /// Makes what drawing it this frame needs: the march's picture for the size of the scene's and
    /// the pipelines of `drawing`, and the view's uniforms. After `advance`.
    pub fn prepare_draw(&mut self, gpu: &Gpu, drawing: &Arc<Drawing>, quality: &Quality, uniforms: &EffectUniforms) {
        let Some(pipelines) = &drawing.fire else { return };
        let scale = march(quality).2;
        let size = [gpu.width.div_ceil(scale).max(1), gpu.height.div_ceil(scale).max(1)];
        if self.draw.as_ref().is_none_or(|b| b.targets_made != gpu.targets_made() || !Arc::ptr_eq(&b.drawing, drawing) || b.gas != self.gas_now) {
            let d = &gpu.device;
            // The march's picture is kept while only the texture of gas changes.
            let kept = self.draw.take().filter(|b| b.targets_made == gpu.targets_made() && Arc::ptr_eq(&b.drawing, drawing));
            let (colour, reached) = match kept {
                Some(b) => (b.colour, b.reached),
                None => {
                    let target = |label: &str, format: wgpu::TextureFormat| {
                        d.create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default())
                    };
                    (target("fire marched", MARCH_COLOUR), target("fire reached", MARCH_REACHED))
                }
            };
            let repeat = gpu.texture_sampler();
            let march = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fire march"),
                layout: &pipelines.march_layout,
                entries: &[buf(0, &self.view_uniforms), tex(1, &self.gas[self.gas_now]), tex(3, &self.light), tex(4, &self.glow), tex(5, &self.noise), smp(6, &self.clamp), smp(7, &repeat), tex(8, gpu.scene_depth())],
            });
            let over = d.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("fire over"), layout: &pipelines.over_layout, entries: &[buf(0, &self.view_uniforms), tex(8, gpu.scene_depth()), tex(9, &colour), tex(10, &reached)] });
            self.draw = Some(DrawBinds { targets_made: gpu.targets_made(), drawing: drawing.clone(), gas: self.gas_now, colour, reached, march, over });
        }
        let corner = to_render(self.grid.origin(&self.land) - self.eye);
        // The grid's corner in the render axes is its lowest east, lowest height, and lowest south.
        let view = ViewUniforms { corner: [corner.x, corner.y, corner.z, uniforms.view[0]], place: self.still(self.time), size: [size[0] as f32, size[1] as f32, scale as f32, TOP_FADE as f32] };
        gpu.queue.write_buffer(&self.view_uniforms, 0, bytemuck::bytes_of(&view));
    }

    /// Marches the grid into its picture, between the scene's two passes; `frame_bind` is group 0.
    pub fn march(&self, encoder: &mut wgpu::CommandEncoder, frame_bind: &wgpu::BindGroup) {
        let Some(b) = &self.draw else { return };
        let Some(pipelines) = &b.drawing.fire else { return };
        let attachment = |view| Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("fire march"), color_attachments: &[attachment(&b.colour), attachment(&b.reached)], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
        pass.set_pipeline(&pipelines.march);
        pass.set_bind_group(0, frame_bind, &[]);
        pass.set_bind_group(1, &b.march, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Puts the march's picture over the scene, in the second pass; group 0 must be bound.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(b) = &self.draw else { return };
        let Some(pipelines) = &b.drawing.fire else { return };
        pass.set_pipeline(&pipelines.over);
        pass.set_bind_group(1, &b.over, &[]);
        pass.draw(0..3, 0..1);
    }
}

const MARCH_COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const MARCH_REACHED: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// The drawing's pipelines for a quality and a target: made in the background (`effect_pipelines.rs`).
pub struct DrawPipelines {
    march_layout: wgpu::BindGroupLayout,
    over_layout: wgpu::BindGroupLayout,
    march: wgpu::RenderPipeline,
    over: wgpu::RenderPipeline,
}

impl DrawPipelines {
    pub fn new(device: &wgpu::Device, target: Target, frame_layout: &wgpu::BindGroupLayout, quality: &Quality) -> DrawPipelines {
        let shader = engine::pipeline::shader_with_ending_loops(device, "fire draw", &draw_source(quality, target.samples));
        let stage = wgpu::ShaderStages::FRAGMENT;
        let uniform = wgpu::BindGroupLayoutEntry { binding: 0, visibility: stage, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(size_of::<ViewUniforms>() as u64) }, count: None };
        let texture = |binding: u32, view_dimension: wgpu::TextureViewDimension, filterable: bool| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension, multisampled: false }, count: None };
        let sampler = |binding: u32| wgpu::BindGroupLayoutEntry { binding, visibility: stage, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let d3 = wgpu::TextureViewDimension::D3;
        let d2 = wgpu::TextureViewDimension::D2;
        let march_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fire march"),
            entries: &[uniform, texture(1, d3, true), texture(3, d3, true), texture(4, d3, true), texture(5, d3, true), sampler(6), sampler(7), depth_entry(8, target.samples)],
        });
        let over_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("fire over"), entries: &[uniform, depth_entry(8, target.samples), texture(9, d2, true), texture(10, d2, false)] });
        let layout = |group: &wgpu::BindGroupLayout| device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("fire draw"), bind_group_layouts: &[Some(frame_layout), Some(group)], immediate_size: 0 });
        let march = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fire march"),
            layout: Some(&layout(&march_layout)),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("fire_screen"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fire_march"),
                targets: &[Some(wgpu::ColorTargetState { format: MARCH_COLOUR, blend: None, write_mask: wgpu::ColorWrites::ALL }), Some(wgpu::ColorTargetState { format: MARCH_REACHED, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let over = blended_pipeline(device, target, "fire over", &layout(&over_layout), &shader, "fire_screen", "fire_over");
        DrawPipelines { march_layout, over_layout, march, over }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::wgpu::naga;
    use sim_core::{AircraftType, PieceKind};

    fn crash(fuel: f64, speed: f64) -> Wreck {
        let mut wreck = Wreck::new(AircraftType::c172().pieces, &PieceKind::ALL, 1000.0, DVec3::new(0.0, 0.0, -1.0), sim_core::quaternion(0.0, -0.5, 0.0), DVec3::new(speed, 0.0, speed * 0.5), DVec3::ZERO, 0.5, 7, 100.0);
        wreck.spill(fuel, false);
        let mut t = 100.0;
        while t < 104.0 {
            wreck.step(1.0 / 240.0, t, |_, _| (0.0, false));
            t += 1.0 / 240.0;
        }
        wreck
    }

    fn fed(wreck: &Wreck, since: f64) -> Vec<Emitter> {
        let mut out = Vec::new();
        emitters(wreck, since, 0.0, &mut out);
        out
    }

    fn validate(source: &str) {
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default()).validate(&module).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    }

    #[test]
    fn the_shaders_are_valid_wgsl() {
        validate(&sim_source());
        for quality in [Quality::ULTRA, Quality::HIGH, Quality::MEDIUM] {
            for samples in [1, 4] {
                validate(&draw_source(&quality, samples));
            }
        }
    }

    #[test]
    fn a_hard_crash_bursts_then_burns_then_smoulders() {
        let wreck = crash(100.0, 40.0);
        assert!(wreck.fireball > 0.0 && wreck.fire.is_some());
        // The burst: lit fuel thrown outwards, and dust; over in half a second.
        let burst = fed(&wreck, 0.15);
        assert!(burst.iter().filter(|e| e.fuel > 4.0 && e.heat > 1.0 && e.burst > 0.0).count() >= 5);
        assert!(burst.iter().any(|e| e.dust > 0.0 && e.fuel == 0.0));
        let fuel = |list: &[Emitter]| list.iter().map(|e| e.fuel).sum::<f32>();
        assert!(fuel(&fed(&wreck, 0.15)) > fuel(&fed(&wreck, 0.5)));
        // Then the pool by the main piece, and no more dust.
        let pool = fed(&wreck, 20.0);
        assert!(!pool.is_empty() && pool.iter().all(|e| e.dust == 0.0 && e.burst == 0.0));
        assert!(pool.iter().any(|e| e.fuel > 0.0 && (e.at - wreck.main().pos).length() < 5.0));
        assert!(fuel(&pool) < fuel(&burst) / 2.0);
        // Out: smoke alone, thinning, then nothing.
        let mut cold = wreck.clone();
        cold.fire = Some(sim_core::Fire { fuel: 0.0, started: 100.0, out: Some(300.0) });
        cold.scars.clear();
        let smoke = fed(&cold, 210.0);
        assert!(smoke.len() == 1 && smoke[0].fuel == 0.0 && smoke[0].soot > 0.0);
        assert!(fed(&cold, 240.0)[0].soot < smoke[0].soot);
        assert!(fed(&cold, 200.0 + SMOKES_AFTER + 1.0).is_empty());
        assert!(Fire::alive(&cold, 200.0 + SMOKES_AFTER - 1.0) && !Fire::alive(&cold, 200.0 + SMOKES_AFTER + 1.0));
    }

    #[test]
    fn a_soft_crash_feeds_no_fire() {
        let wreck = crash(100.0, 14.0);
        assert!(wreck.fireball == 0.0 && wreck.fire.is_none());
        assert!(fed(&wreck, 0.1).iter().all(|e| e.fuel == 0.0 && e.heat == 0.0));
        assert!(!Fire::alive(&wreck, 1.0));
    }

    #[test]
    fn the_same_moment_is_fed_the_same_and_from_above_the_ground() {
        let wreck = crash(100.0, 40.0);
        for since in [0.0, 0.2, 0.41, 0.55, 3.0, 30.0] {
            assert_eq!(fed(&wreck, since), fed(&wreck, since));
            assert!(fed(&wreck, since).iter().all(|e| -e.at.z > 0.0 && e.radius > 0.5 && e.fuel >= 0.0), "{since}");
            assert!(fed(&wreck, since).len() <= MOST_EMITTERS);
        }
    }

    #[test]
    fn a_bigger_fireball_is_wider_up_to_what_the_grid_holds() {
        assert!((fireball_radius(100.0) - 13.46).abs() < 0.01);
        // The noise repeats at the period its place is wrapped at, at every scale it is read at.
        for cells in [19.0 / CELL, 57.0 / CELL, 40.0, 56.0] {
            assert!(((15_960.0 / cells).round() - 15_960.0 / cells).abs() < 1e-9, "{cells}");
        }
        assert!(fireball_radius(20.0) < fireball_radius(100.0));
        assert!(fireball_radius(1e5) * 2.0 < CELLS[0] as f64 * CELL / 2.0);
    }

    fn flat(zero: f64) -> Land {
        Land { east: -Land::SIDE / 2.0, south: -Land::SIDE / 2.0, zero, texels: vec![[0.0; 4]; (MAP * MAP) as usize] }
    }

    #[test]
    fn the_grid_follows_the_fire_a_cell_a_step_and_stays_on_the_map() {
        let land = flat(0.0);
        let mut grid = Grid::round(&land, DVec3::ZERO);
        // Round the point, its floor a cell under the ground.
        let middle = grid.cells(&land, DVec3::new(0.0, 0.0, 0.0));
        let half = CELLS[0] as f32 / 2.0;
        assert!((middle.x - half).abs() < 1.0 && (middle.z - half).abs() < 1.0 && (middle.y - 1.0).abs() < 1e-3, "{middle:?}");
        // Sixteen cells north-east: one a step.
        let to = Grid::round(&land, DVec3::new(16.0 * CELL, 16.0 * CELL, 0.0));
        let mut steps = 0;
        while grid != to {
            let by = grid.follow(to);
            assert!(by.iter().all(|b| b.abs() <= 1) && by != [0; 3]);
            steps += 1;
        }
        assert_eq!(steps, 16);
        // North is less south: the grid went east and back along its third axis.
        assert_eq!(grid.corner, [Grid::round(&land, DVec3::ZERO).corner[0] + 16, -1, Grid::round(&land, DVec3::ZERO).corner[2] - 16]);
        // A point is where its cells say, and the corner where the cells are none.
        let p = DVec3::new(20.0, 5.0, -30.0);
        let cells = grid.cells(&land, p).as_dvec3();
        let back = grid.origin(&land) + DVec3::new(-cells.z, cells.x, -cells.y) * CELL;
        assert!((back - p).length() < 1e-3, "{back:?}");
        // Never off the map.
        let far = Grid::round(&land, DVec3::new(5000.0, -5000.0, 0.0));
        assert_eq!((far.corner[0], far.corner[2]), (0, 0));
    }

    #[test]
    fn the_grid_stands_on_the_lowest_ground_under_it() {
        let mut land = flat(100.0);
        // A hollow 6 m deep under one corner of where the grid will be.
        for texel in land.texels.iter_mut().take(MAP as usize * 20) {
            texel[0] = -6.0;
        }
        let low = Grid::round(&land, DVec3::new(60.0, 0.0, -100.0));
        let high = Grid::round(&land, DVec3::new(-60.0, 0.0, -100.0));
        assert_eq!(high.corner[1], -1);
        assert_eq!(low.corner[1], -9);
        assert!((low.origin(&land).z - -(100.0 - 9.0 * CELL)).abs() < 1e-9);
    }

    /// A half float's value.
    fn half(bits: u16) -> f32 {
        let (sign, exponent, fraction) = (if bits >> 15 == 1 { -1.0f32 } else { 1.0 }, ((bits >> 10) & 31) as i32, (bits & 1023) as f32);
        match exponent {
            0 => sign * fraction * 2f32.powi(-24),
            31 => sign * f32::INFINITY,
            e => sign * (1.0 + fraction / 1024.0) * 2f32.powi(e - 15),
        }
    }

    /// The gas of every cell (fuel, heat, soot, dust), x fastest, then y, then z.
    fn gas_of(fire: &Fire, gpu: &Gpu) -> Vec<[f32; 4]> {
        // A row of the copy is as long as the copy asks: a multiple of 256 bytes.
        let row = (CELLS[0] * 8).div_ceil(256) * 256;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * CELLS[1] * CELLS[2]) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            fire.gas[fire.gas_now].texture().as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(CELLS[1]) } },
            wgpu::Extent3d { width: CELLS[0], height: CELLS[1], depth_or_array_layers: CELLS[2] },
        );
        gpu.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map failed"));
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("buffer not mapped");
        let numbers: &[u16] = bytemuck::cast_slice(&data);
        numbers.chunks(row as usize / 2).flat_map(|r| r[..CELLS[0] as usize * 4].as_chunks::<4>().0.iter().map(|c| c.map(half))).collect()
    }

    /// What the gas is like: its fuel and soot in all (a cell's unit times m³), the hottest cell,
    /// the room hotter than 1 (1300 K: what glows), m³, and the soot's middle: its height above
    /// the grid's floor and how far from it the soot lies at that height, m.
    #[derive(Debug)]
    struct Seen {
        fuel: f64,
        soot: f64,
        hottest: f32,
        glowing: f64,
        soot_height: f64,
        half_width: f64,
    }

    fn seen(gas: &[[f32; 4]]) -> Seen {
        let (nx, ny, nz) = (CELLS[0] as usize, CELLS[1] as usize, CELLS[2] as usize);
        let volume = CELL * CELL * CELL;
        let at = |x: usize, y: usize, z: usize| gas[(z * ny + y) * nx + x];
        let (mut fuel, mut soot, mut hottest, mut glowing) = (0.0, 0.0, 0.0f32, 0.0);
        let (mut cx, mut cy, mut cz) = (0.0, 0.0, 0.0);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let g = at(x, y, z);
                    assert!(g.iter().all(|v| v.is_finite() && *v >= 0.0), "{g:?} at {x} {y} {z}");
                    fuel += g[0] as f64 * volume;
                    soot += g[2] as f64 * volume;
                    hottest = hottest.max(g[1]);
                    if g[1] > 1.0 {
                        glowing += volume;
                    }
                    cx += g[2] as f64 * x as f64;
                    cy += g[2] as f64 * y as f64;
                    cz += g[2] as f64 * z as f64;
                }
            }
        }
        let total = (soot / volume).max(1e-9);
        let (cx, cy, cz) = (cx / total, cy / total, cz / total);
        let level = (cy as usize).min(ny - 1);
        let (mut spread, mut there) = (0.0, 0.0);
        for z in 0..nz {
            for x in 0..nx {
                let s = at(x, level, z)[2] as f64;
                there += s;
                spread += s * ((x as f64 - cx).powi(2) + (z as f64 - cz).powi(2)) * CELL * CELL;
            }
        }
        Seen { fuel, soot, hottest, glowing, soot_height: (cy + 0.5) * CELL, half_width: if there > 0.0 { (spread / there).sqrt() } else { 0.0 } }
    }

    fn crashed(terrain: &Terrain, speed: f64) -> (Wreck, f64) {
        let (n, e) = (2000.0, 12000.0);
        let ground = terrain.height(n, e).max(0.0);
        let mut wreck = Wreck::new(AircraftType::c172().pieces, &PieceKind::ALL, 1100.0, DVec3::new(n, e, -(ground + 1.0)), sim_core::quaternion(0.0, -35.0 * sim_core::DEG, 0.0), DVec3::new(speed, 0.0, speed * 0.7), DVec3::ZERO, 0.5, 7, 100.0);
        wreck.spill(100.0, false);
        let mut t = 100.0;
        while t < 104.0 {
            wreck.step(1.0 / 240.0, t, |n, e| (terrain.height(n, e).max(0.0), false));
            t += 1.0 / 240.0;
        }
        (wreck, ground)
    }

    /// The fire of a hard crash on a real device, played from the crash: the fuel thrown out
    /// burns away in a second and a half into a ball of glowing gas some 12 m in radius, which
    /// cools into soot that is kept and rises, where `effects::cap_height` says; the fire then
    /// burns where the main piece lies, and the grid goes there. A soft crash leaves the grid empty.
    #[test]
    fn a_fireball_on_a_real_device() {
        let gpu = engine::Offscreen::new(16, 16, 1.0).gpu;
        let terrain = Terrain::new(1);
        let simulation = Arc::new(Simulation::new(&gpu.device));
        let noise = crate::particles::make_noise(&gpu.device, &gpu.queue);
        let (wind, sun) = (DVec3::new(1.5, 0.0, 0.0), Vec3::new(0.4, 0.8, 0.3).normalize());

        let (wreck, ground) = crashed(&terrain, 37.0);
        assert!(wreck.fireball > 0.0 && wreck.fire.is_some());
        let mut fire = Fire::new(&gpu, &simulation, &noise, &wreck, ground, &terrain);
        let first = fire.grid;
        let mut since = 0.0;
        let mut at = |until: f64, fire: &mut Fire| {
            while since < until {
                since += 4.0 * STEP;
                fire.advance(&gpu, &wreck, since, ground, wind, sun, wreck.impact);
            }
            let s = seen(&gas_of(fire, &gpu));
            // The soot's height above where the fireball burst, a metre over the ground.
            let above = s.soot_height + fire.grid.corner[1] as f64 * CELL - 1.0;
            if std::env::var_os("FIRE_TEST_PRINT").is_some() {
                println!("{until:5.2} s: {s:?}, soot {above:.1} m up, the cap at {:.1}", crate::effects::cap_height(until));
            }
            (s, above)
        };
        let (burst, _) = at(0.42, &mut fire);
        assert!(burst.fuel > 800.0 && burst.hottest > 1.7 && burst.hottest < 2.1, "{burst:?}");
        let (one, up_one) = at(1.0, &mut fire);
        let (burnt, _) = at(1.5, &mut fire);
        assert!(burnt.fuel < burst.fuel * 0.05, "{burnt:?}");
        assert!(burnt.glowing > 6000.0 && burnt.glowing < 22_000.0, "{burnt:?}");
        assert!(burnt.half_width > 8.0 && burnt.half_width < 16.0, "{burnt:?}");
        let (three, up_three) = at(3.0, &mut fire);
        // Soot is neither made nor lost by the flow, however the gas grows: only the pool adds to it.
        assert!(three.soot > burnt.soot * 0.95 && three.soot < burnt.soot * 1.4, "{burnt:?} {three:?}");
        let (six, up_six) = at(6.0, &mut fire);
        assert!(six.glowing < burnt.glowing * 0.1, "{six:?}");
        assert!(up_six > up_one + 3.0, "{up_one} {up_six}");
        for (t, up) in [(1.0, up_one), (3.0, up_three), (6.0, up_six)] {
            assert!((crate::effects::cap_height(t) - up).abs() < 5.0, "{t} s: {up:.1} m, the cap at {:.1}", crate::effects::cap_height(t));
        }
        assert!(one.soot > 0.0);
        // By now the grid has gone to where the main piece burns, and stays over the ground there.
        at(14.0, &mut fire);
        let to = Grid::round(&fire.land, wreck.main().pos);
        assert!(fire.grid != first && fire.grid == to, "{first:?} {:?} {to:?}", fire.grid);
        assert!(fire.top > 60.0 && fire.top < CELLS[1] as f64 * CELL);
        if std::env::var_os("FIRE_TEST_PRINT").is_some() {
            // How long a step takes: 240 of them, the GPU waited for.
            let _ = gas_of(&fire, &gpu);
            let started = std::time::Instant::now();
            for k in 1..=60 {
                fire.advance(&gpu, &wreck, 14.0 + k as f64 * 4.0 * STEP + 0.001, ground, wind, sun, wreck.impact);
            }
            let _ = gas_of(&fire, &gpu);
            println!("a step: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0 / 240.0);
        }

        let (soft, ground) = crashed(&terrain, 10.0);
        assert!(soft.fireball == 0.0 && soft.fire.is_none());
        let mut fire = Fire::new(&gpu, &simulation, &noise, &soft, ground, &terrain);
        for step in 1..=20 {
            fire.advance(&gpu, &soft, step as f64 * 4.0 * STEP, ground, wind, sun, soft.impact);
        }
        let nothing = seen(&gas_of(&fire, &gpu));
        assert!(nothing.fuel == 0.0 && nothing.soot == 0.0 && nothing.hottest == 0.0, "{nothing:?}");
    }

    #[test]
    fn the_uniforms_are_as_the_shaders_read_them() {
        assert_eq!(size_of::<GpuEmitter>(), 64);
        assert_eq!(size_of::<ViewUniforms>(), 48);
        assert_eq!(GpuEmitter::zeroed().place, [0.0; 4]);
        // Every quality that simulates the fire marches it, the better the finer.
        let (high, ultra) = (march(&Quality::HIGH), march(&Quality::ULTRA));
        assert!(ultra == high && march(&Quality::MEDIUM).0 > high.0);
        assert!(draw_source(&Quality::HIGH, 1).contains("texture_depth_2d") && !draw_source(&Quality::HIGH, 1).contains("multisampled"));
    }
}
