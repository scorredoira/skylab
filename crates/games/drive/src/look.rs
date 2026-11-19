//! How a car looks: its model (`assets/cars/<model>/exterior.glb`, made by
//! `examples/car_model`), drawn into the world as the car is now (the body where it is, each
//! wheel turned, steered and on its spring, its brakes glowing when hot, the steering wheel
//! turned, the dials' needles where the revs and the speed are, the lamps lit; from the seat, the
//! driver's hands on the wheel, which turn with it as far as hands do and reach for a paddle
//! when a gear changes, and the forearms behind them), its shadow and the light of its lamps.
//!
//! The model's materials are drawn by the game's own shader (`car.wgsl`, on the world's: the same
//! sun, sky, clouds, shadows and haze), each kind in its own way: paint under a clear coat, glass,
//! lamps, rubber, alloy, the inside. A material's name says its kind (`Kind::of`); its glTF colour,
//! metallic and roughness are the shader's. The solid parts are drawn before the world, the glass
//! after it (it is see-through: what is behind it must be there already). In the rain, from the
//! seat, the glass is drawn in the frame's second pass with the scene the first left
//! (`before_wet`): the drops on it bend what is behind them, and the wipers clear their part.

use engine::gltf::Model;
use engine::pipeline::{Depth, PipelineDesc};
use engine::{wgpu, Gpu, Mesh, MeshBuilder, UniformArena};
use glam::{DQuat, DVec3, Mat4, Vec3};
use sim_core::lights::Color;
use sim_core::{act, Car};
use world_render::{to_render, CasterDraw, CasterMesh, Lamp, ShadowShape, WorldRenderer};

/// How the shader draws a material: `CAR_*` in `car.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Kind {
    Plain = 0,
    Paint = 1,
    Glass = 2,
    Lamp = 3,
    Rubber = 4,
    Dial = 5,
    Disc = 6,
    Inside = 7,
    Grille = 8,
    Carbon = 9,
    Mirror = 10,
    Blur = 11,
    Plate = 12,
}

impl Kind {
    /// The kind of the material of this name.
    fn of(name: &str) -> Kind {
        match name {
            "paint" => Kind::Paint,
            "glass" => Kind::Glass,
            "rubber" => Kind::Rubber,
            "dial" => Kind::Dial,
            "disc" => Kind::Disc,
            "dash" | "seat" | "leather" | "carpet" | "marker" | "screen" | "glove" | "glove_band" | "sleeve" => Kind::Inside,
            "grille" => Kind::Grille,
            "carbon" => Kind::Carbon,
            "mirror" => Kind::Mirror,
            "blur" => Kind::Blur,
            "plate" => Kind::Plate,
            n if n.starts_with("lamp_") => Kind::Lamp,
            _ => Kind::Plain,
        }
    }
}

/// A run of a part's triangles of one material.
#[derive(Clone, Debug)]
struct Piece {
    first: u32,
    count: u32,
    name: String,
    kind: Kind,
    base: [f32; 3],
    roughness: f32,
    metallic: f32,
    emissive: [f32; 3],
}

/// A node of the model on the GPU, about its own origin.
struct Part {
    mesh: Mesh,
    pieces: Vec<Piece>,
    /// Where its origin is in the body's axes.
    origin: Vec3,
    /// How far from the body's origin its farthest point is, metres.
    reach: f32,
}

/// Per-draw data: `CarDraw` in `car.wgsl`. The size of the world's `ObjectUniforms`, whose layout it uses.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CarDraw {
    model: [[f32; 4]; 4],
    base: [f32; 4],
    surface: [f32; 4],
    glow: [f32; 4],
    state: [f32; 4],
    more: [f32; 4],
    spare: [f32; 4],
}

const _: () = assert!(size_of::<CarDraw>() == size_of::<world_render::ObjectUniforms>());

/// One draw: which part and piece, where its uniforms are.
#[derive(Clone, Copy)]
struct Draw {
    part: usize,
    piece: usize,
    offset: u32,
}

/// What the car is doing that shows, besides where it is: given each frame.
#[derive(Clone, Copy, Debug)]
pub struct Showing {
    /// The eye is inside the car.
    pub inside: bool,
    /// The lamps are lit (dusk to dawn).
    pub lights: bool,
    /// 0 night to 1 full day.
    pub daylight: f32,
    /// Seconds since the last frame, for what warms and cools.
    pub dt: f32,
    /// The light of the circuit's floodlights where the car is (0 by day), and the way to the
    /// tower that gives most of it (render axes, unit).
    pub flood: f32,
    pub flood_from: Vec3,
    /// Where the car is on the circuit, for what its mirrors show of the road behind: metres
    /// across (right of the middle), the circuit's heading there (radians from north), its
    /// curvature (1/m, a right turn positive), half the asphalt's width.
    pub road: [f32; 4],
    /// How wet it is, 0 to 1 (drops on the glass), and seconds of the game so far (they form,
    /// run and are wiped).
    pub wet: f32,
    pub time: f32,
    /// How much of the sky the clouds cover, 0 to 1: the paint mirrors them.
    pub cloud_cover: f32,
}

const BODY: usize = 0;
const GLASS: usize = 1;
const CABIN: usize = 2;
const STEERING: usize = 3;
const DIAL_REV: usize = 4;
const DIAL_SPEED: usize = 5;
const WHEEL_FRONT: usize = 6;
const WHEEL_REAR: usize = 7;
const BRAKE_FRONT: usize = 8;
const BRAKE_REAR: usize = 9;
const HAND_LEFT: usize = 10;
const HAND_RIGHT: usize = 11;
const ARM: usize = 12;
const NODES: [&str; 13] = ["body", "glass", "cabin", "steering", "dial_rev", "dial_speed", "wheel_front", "wheel_rear", "brake_front", "brake_rear", "hand_left", "hand_right", "arm"];

/// How far the hands turn with the steering wheel, radians either way: beyond, the rim slides
/// through them (a driver's arms cross no further).
const HANDS_TURN: f32 = 1.75;
/// Where a hand holds the rim and where its wrist is, in the steering wheel's axes (the right
/// hand's; the left's are mirrored), as `examples/car_model/cabin.rs` makes them.
const GRIP: Vec3 = Vec3::new(0.0, 0.172, -0.03);
const WRIST: Vec3 = Vec3::new(-0.05, 0.228, 0.065);
/// Where the driver's elbows are in the body's axes: left, right.
const ELBOWS: [Vec3; 2] = [Vec3::new(-0.33, -0.665, -0.150), Vec3::new(-0.33, -0.095, -0.150)];
/// Seconds a hand takes to pull a paddle and come back.
const PADDLE_TIME: f32 = 0.32;

/// A wheel turning faster than this (rad/s) is drawn with its spokes blurred into a disc: at 60
/// frames a second they would jump a quarter of the way to the next pair every frame.
const BLUR_SPIN: f64 = 16.0;

/// What draws the glass seen from the seat in the rain: its pipeline, and the scene the first pass
/// left as it reads it, for the targets of one `Gpu::targets_made`.
struct WetGlass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind: Option<(wgpu::BindGroup, u64)>,
}

/// A car's model, on the GPU, and what draws it.
pub struct CarLook {
    /// Shared by every car drawn with the same model (`CarLook::share`).
    parts: std::rc::Rc<Vec<Part>>,
    pipelines: Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>,
    wet_glass: Option<WetGlass>,
    /// The eye was inside the car at the last `prepare`.
    inside: bool,
    uniforms: UniformArena,
    solid: Vec<Draw>,
    clear: Vec<Draw>,
    /// The paint's colour, linear.
    pub paint: [f32; 3],
    /// How hot each brake is, 0 cold to 1 glowing orange.
    heat: [f32; 4],
    /// The gear at the last frame, and the paddle being pulled: which hand (0 left, down a gear;
    /// 1 right, up) and for how long so far.
    gear: i32,
    paddle: Option<(usize, f32)>,
    /// The radius of a sphere round the body's origin that holds the whole car, metres.
    reach: f32,
}

/// The folder of the game's assets (`assets`), found as the world finds its textures.
pub fn assets() -> Result<std::path::PathBuf, String> {
    let textures = world_render::materials::find_textures()?;
    Ok(textures.parent().map(|p| p.to_path_buf()).unwrap_or(textures))
}

impl CarLook {
    /// Reads the model of `name` (a folder of `assets/cars`) and puts its parts on the GPU.
    pub fn load(gpu: &Gpu, world: &WorldRenderer, name: &str) -> Result<CarLook, String> {
        let path = assets()?.join("cars").join(name).join("exterior.glb");
        let bytes = engine::files::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let model = engine::gltf::read(&bytes)?;
        let parts = NODES.iter().map(|node| part(gpu, &model, node).map_err(|e| format!("{}: {e}", path.display()))).collect::<Result<Vec<Part>, String>>()?;
        let paint = parts[BODY].pieces.iter().find(|p| p.kind == Kind::Paint).map_or([0.4, 0.02, 0.02], |p| p.base);
        // The body holds the rest, but for the wheels' springs and steering: a little room for them.
        let reach = parts[BODY].reach + 0.15;
        Ok(CarLook { parts: std::rc::Rc::new(parts), pipelines: None, wet_glass: None, inside: false, uniforms: UniformArena::new(gpu, world.object_layout(), size_of::<CarDraw>(), 160), solid: Vec::new(), clear: Vec::new(), paint, heat: [0.0; 4], gear: 1, paddle: None, reach })
    }

    /// Another car of the same model (its parts shared on the GPU), with its own paint and state;
    /// its pipelines are made by `pipelines` as the first's are.
    pub fn share(&self, gpu: &Gpu, world: &WorldRenderer, paint: [f32; 3]) -> CarLook {
        CarLook { parts: self.parts.clone(), pipelines: None, wet_glass: None, inside: false, uniforms: UniformArena::new(gpu, world.object_layout(), size_of::<CarDraw>(), 160), solid: Vec::new(), clear: Vec::new(), paint, heat: [0.0; 4], gear: 1, paddle: None, reach: self.reach }
    }

    /// Draws every part, solid and see-through, with another pipeline on the same shader inputs
    /// (the ghost's: `ghost.rs`), after `WorldRenderer::draw`.
    pub fn draw_all_with(&self, pass: &mut wgpu::RenderPass<'_>, world: &WorldRenderer, pipeline: &wgpu::RenderPipeline) {
        world.bind_frame(pass);
        pass.set_pipeline(pipeline);
        self.run(pass, &self.solid);
        self.run(pass, &self.clear);
    }

    /// The pipelines, from the game's shader module (the world's shaders with `car.wgsl`): made
    /// again when the quality or the samples change.
    pub fn pipelines(&mut self, gpu: &Gpu, world: &WorldRenderer, shader: &wgpu::ShaderModule) {
        let buffers = [MeshBuilder::layout()];
        let groups = [world.frame_layout(), world.object_layout()];
        let desc = |fragment: &'static str, depth: Depth| PipelineDesc { label: "car", shader, vertex: "car_vertex", fragment, buffers: &buffers, bind_groups: &groups, depth, blend: false };
        let solid = gpu.pipeline(&desc("car_fragment", Depth::Write));
        let clear = gpu.pipeline_blended(&desc("car_glass_fragment", Depth::Read), Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
        self.pipelines = Some((solid, clear));
        // The glass with drops reads the scene the first pass kept, which the engine has only
        // with several samples per pixel (`Gpu::keep_scene`).
        self.wet_glass = (gpu.sample_count > 1).then(|| {
            let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("wet glass"),
                entries: &[
                    wgpu::BindGroupLayoutEntry { binding: 8, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                    wgpu::BindGroupLayoutEntry { binding: 10, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                ],
            });
            let groups = [world.frame_layout(), world.object_layout(), &layout];
            let desc = PipelineDesc { label: "wet glass", shader, vertex: "car_vertex", fragment: "car_wet_glass_fragment", buffers: &buffers, bind_groups: &groups, depth: Depth::Read, blend: false };
            let pipeline = gpu.pipeline_blended(&desc, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
            let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("wet glass"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
            WetGlass { pipeline, layout, sampler, bind: None }
        });
    }

    /// Between the two passes of a frame whose second reads the first (`Gpu::scene_kept`): what
    /// the glass with drops reads.
    pub fn before_wet(&mut self, gpu: &Gpu) {
        let Some(wet) = &mut self.wet_glass else { return };
        let Some(scene) = gpu.scene_kept() else {
            wet.bind = None;
            return;
        };
        if wet.bind.as_ref().is_none_or(|(_, made)| *made != gpu.targets_made()) {
            let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("wet glass"),
                layout: &wet.layout,
                entries: &[wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(scene) }, wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::Sampler(&wet.sampler) }],
            });
            wet.bind = Some((bind, gpu.targets_made()));
        }
    }

    /// Before the frame is drawn: every draw of the car as it is, seen from `eye` (world axes).
    pub fn prepare(&mut self, gpu: &Gpu, car: &Car, eye: DVec3, showing: Showing) {
        self.warm(car, showing.dt);
        self.inside = showing.inside;
        self.uniforms.clear();
        self.solid.clear();
        self.clear.clear();
        let b = car.body;
        let body = body_matrix(b.q, b.pos - eye);
        let place = |origin: Vec3| body * Mat4::from_translation(origin);
        let plain = Extra::default();
        self.push(BODY, body, 1.0, car, showing, plain);
        self.push(GLASS, body, 1.0, car, showing, plain);
        self.push(CABIN, body, 1.0, car, showing, plain);
        // The steering wheel turned as the front wheels are, by the steering's ratio, about its
        // column, which leans back.
        let turn = (car.controls.steer * car.p.steering.wheel_turns * std::f64::consts::PI) as f32;
        let lean = Mat4::from_translation(self.parts[STEERING].origin) * Mat4::from_rotation_y(-0.38);
        self.push(STEERING, body * lean * Mat4::from_rotation_x(turn), 1.0, car, showing, plain);
        // A gear changed: the hand on that side pulls its paddle.
        let gear = car.gear();
        if gear != self.gear {
            self.paddle = Some((usize::from(gear > self.gear), 0.0));
            self.gear = gear;
        }
        if let Some((_, time)) = &mut self.paddle {
            *time += showing.dt;
            if *time > PADDLE_TIME {
                self.paddle = None;
            }
        }
        // The driver's hands and forearms, seen from the seat: the hands go round with the rim
        // as far as they can; the one that pulls a paddle slides in along its spoke and bends
        // its fingers behind the wheel; each forearm from its wrist to an elbow that stays put.
        if showing.inside {
            let held = lean * Mat4::from_rotation_x(turn.clamp(-HANDS_TURN, HANDS_TURN));
            for (k, part) in [HAND_LEFT, HAND_RIGHT].into_iter().enumerate() {
                let side = if k == 0 { -1.0 } else { 1.0 };
                let grip = Vec3::new(GRIP.x, side * GRIP.y, GRIP.z);
                let pull = self.paddle.filter(|p| p.0 == k).map_or(0.0, |(_, t)| (std::f32::consts::PI * t / PADDLE_TIME).sin());
                let reach = Mat4::from_translation(grip + Vec3::new(0.0, -side * 0.022 * pull, 0.0)) * Mat4::from_rotation_z(-side * 0.4 * pull) * Mat4::from_translation(-grip);
                let hand = held * reach;
                self.push(part, body * hand, 1.0, car, showing, plain);
                let wrist = hand.transform_point3(Vec3::new(WRIST.x, side * WRIST.y, WRIST.z));
                let along = ELBOWS[k] - wrist;
                let x = along.normalize_or(Vec3::NEG_X);
                let y = Vec3::Z.cross(x).normalize_or(Vec3::Y);
                let arm = Mat4::from_cols((x * along.length()).extend(0.0), y.extend(0.0), x.cross(y).extend(0.0), wrist.extend(1.0));
                self.push(ARM, body * arm, 1.0, car, showing, plain);
            }
        }
        // The dials: the revs up to 8000 a minute (and the gear, in its face), the speed up to 320 km/h.
        let revs = (car.rpm() / 8000.0) as f32;
        let speed = (b.vel.x.abs() * 3.6 / 320.0) as f32;
        let red = (car.p.engine.redline_rpm / 8000.0) as f32;
        self.push(DIAL_REV, place(self.parts[DIAL_REV].origin), 1.0, car, showing, Extra { value: revs, more: red, third: gear as f32 });
        self.push(DIAL_SPEED, place(self.parts[DIAL_SPEED].origin), 1.0, car, showing, Extra { value: speed, more: 2.0, third: -10.0 });
        // The wheels: on their springs, steered, turning; the left ones mirrored. The brakes
        // behind them steer and ride with them and do not turn.
        for (i, w) in car.wheels().iter().enumerate() {
            let hub = car.p.hub(i) + DVec3::new(0.0, 0.0, w.travel);
            let mirror = if i % 2 == 0 { -1.0 } else { 1.0 };
            let upright = body * Mat4::from_translation(hub.as_vec3()) * Mat4::from_rotation_z(w.steer as f32);
            let side = Mat4::from_scale(Vec3::new(1.0, mirror, 1.0));
            let (wheel, brake) = if i < 2 { (WHEEL_FRONT, BRAKE_FRONT) } else { (WHEEL_REAR, BRAKE_REAR) };
            let blurred = if w.omega.abs() > BLUR_SPIN { 1.0 } else { 0.0 };
            self.push(wheel, upright * Mat4::from_rotation_y(-w.angle as f32) * side, mirror, car, showing, Extra { value: blurred, more: 10.0, third: 0.0 });
            self.push(brake, upright * side, mirror, car, showing, Extra { value: self.heat[i], more: 0.0, third: 0.0 });
        }
        self.uniforms.upload(gpu);
    }

    /// What casts the car's shadow, seen from `eye` (world axes), for the world's map of the
    /// vehicles' shadows (`world_render::Caster`): the body, the cabin and the steering wheel, each
    /// wheel on its spring and steered, its brake. The glass lets the sun through (the cabin's
    /// seats show in the shadow through the windows, and the sun comes in on them from the seat);
    /// a wheel turning fast casts its rim and tyre, not its spokes, as it is drawn.
    pub fn casters(&self, car: &Car, eye: DVec3) -> Vec<CasterDraw<'_>> {
        let mut out = Vec::new();
        let b = car.body;
        let body = body_matrix(b.q, b.pos - eye);
        let mut add = |part: usize, model: Mat4, spinning: bool| {
            let p = &self.parts[part];
            for piece in &p.pieces {
                let shown = match piece.kind {
                    Kind::Glass | Kind::Blur => false,
                    Kind::Plain => !(spinning && piece.name == "spokes"),
                    _ => true,
                };
                if shown {
                    out.push(CasterDraw { mesh: CasterMesh::Game(&p.mesh), first: piece.first, count: piece.count, model, cuts: [None; 2] });
                }
            }
        };
        add(BODY, body, false);
        add(CABIN, body, false);
        let turn = (car.controls.steer * car.p.steering.wheel_turns * std::f64::consts::PI) as f32;
        let lean = Mat4::from_translation(self.parts[STEERING].origin) * Mat4::from_rotation_y(-0.38);
        add(STEERING, body * lean * Mat4::from_rotation_x(turn), false);
        for (i, w) in car.wheels().iter().enumerate() {
            let hub = car.p.hub(i) + DVec3::new(0.0, 0.0, w.travel);
            let mirror = if i % 2 == 0 { -1.0 } else { 1.0 };
            let upright = body * Mat4::from_translation(hub.as_vec3()) * Mat4::from_rotation_z(w.steer as f32);
            let side = Mat4::from_scale(Vec3::new(1.0, mirror, 1.0));
            let (wheel, brake) = if i < 2 { (WHEEL_FRONT, BRAKE_FRONT) } else { (WHEEL_REAR, BRAKE_REAR) };
            add(wheel, upright * Mat4::from_rotation_y(-w.angle as f32) * side, w.omega.abs() > BLUR_SPIN);
            add(brake, upright * side, false);
        }
        out
    }

    /// The radius of a sphere round the car's centre of gravity that holds all of it, metres.
    pub fn reach(&self) -> f32 {
        self.reach
    }

    /// The brakes warm with the work they do (the pedal times how fast the wheel turns) and cool in the air.
    fn warm(&mut self, car: &Car, dt: f32) {
        let front = car.p.brakes.front_share as f32;
        for (i, w) in car.wheels().iter().enumerate() {
            let share = if i < 2 { front } else { 1.0 - front };
            let work = car.controls.brake as f32 * share * (w.omega.abs() as f32 / 150.0).min(1.5);
            let heat = &mut self.heat[i];
            *heat += (work * 0.55 - *heat * (0.10 + 0.05 * (w.omega.abs() as f32 / 100.0))) * dt;
            *heat = heat.clamp(0.0, 1.0);
        }
    }

    /// Adds the draws of a part, each piece with its material as the car's state makes it.
    fn push(&mut self, part: usize, model: Mat4, mirror: f32, car: &Car, showing: Showing, extra: Extra) {
        let braking = car.controls.brake > 0.05;
        let night = 1.0 - showing.daylight;
        for (k, piece) in self.parts[part].pieces.iter().enumerate() {
            let mut base = piece.base;
            // What a lamp or a lit mark gives off, by what it is and what the car does.
            let lit = match piece.name.as_str() {
                "lamp_ring" => 1.6 + 5.0 * night,
                "lamp_head" => if showing.lights { 70.0 } else { 0.0 },
                // The tail light's line is always lit (more by night); the brake lights' cells
                // glow a little with the lamps on and shine braking; the lens round them takes
                // some of their light.
                "lamp_tail" => if braking { 14.0 } else if showing.lights { 6.0 } else { 1.6 },
                "lamp_brake" => if braking { 42.0 } else if showing.lights { 1.6 } else { 0.0 },
                "lamp_lens" => if braking { 1.6 } else if showing.lights { 0.35 } else { 0.0 },
                "screen" => 0.02 + 0.03 * showing.daylight,
                // The dials are lit from behind: enough to read by day, a glow by night.
                "dial" => 0.22 + 0.4 * showing.daylight,
                _ => 0.0,
            };
            let mut opacity = 1.0;
            match piece.kind {
                Kind::Paint => base = self.paint,
                // The tint lets this much light through.
                Kind::Glass => opacity = 0.32,
                // Standing or slow, the spokes; fast, the disc they blur into.
                Kind::Blur if extra.value < 0.5 => continue,
                Kind::Plain if piece.name == "spokes" && extra.value > 0.5 => continue,
                _ => {}
            }
            // What the kind wants besides (`CarDraw::more`, `spare` in `car.wgsl`): a mirror, the
            // road behind; the glass, the rain on it; anything else, where the floodlights are.
            let (more, spare) = match piece.kind {
                Kind::Mirror => ([extra.more, 0.0, showing.road[3], 0.0], [showing.cloud_cover, showing.road[0], showing.road[1], showing.road[2]]),
                Kind::Glass => ([extra.more, 0.0, showing.time, showing.wet], [showing.cloud_cover, car.body.vel.x as f32, 0.0, 0.0]),
                _ => {
                    // What a part of the inside is made of, for what the shader draws on it.
                    let made = match piece.name.as_str() {
                        "lamp_brake" => 1.0,
                        "dash" => 1.0,
                        "seat" => 2.0,
                        "sleeve" => 3.0,
                        _ => 0.0,
                    };
                    ([extra.more, made, extra.third, 0.0], [showing.cloud_cover, showing.flood_from.x, showing.flood_from.y, showing.flood_from.z])
                }
            };
            let draw = CarDraw {
                model: model.to_cols_array_2d(),
                base: [base[0], base[1], base[2], opacity],
                surface: [piece.kind as u8 as f32, piece.roughness, piece.metallic, mirror],
                glow: [piece.emissive[0] * lit, piece.emissive[1] * lit, piece.emissive[2] * lit, extra.value],
                state: [if showing.inside { 1.0 } else { 0.0 }, showing.daylight, showing.flood, if matches!(part, CABIN | STEERING | DIAL_REV | DIAL_SPEED | HAND_LEFT | HAND_RIGHT | ARM) { 1.0 } else { 0.0 }],
                more,
                spare,
            };
            let offset = self.uniforms.push(&draw);
            let draw = Draw { part, piece: k, offset };
            if matches!(piece.kind, Kind::Glass | Kind::Blur) {
                self.clear.push(draw);
            } else {
                self.solid.push(draw);
            }
        }
    }

    fn run(&self, pass: &mut wgpu::RenderPass<'_>, draws: &[Draw]) {
        let mut bound = usize::MAX;
        for d in draws {
            let part = &self.parts[d.part];
            if bound != d.part {
                pass.set_vertex_buffer(0, part.mesh.buffer.slice(..));
                if let Some(indices) = &part.mesh.indices {
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                }
                bound = d.part;
            }
            let piece = &part.pieces[d.piece];
            pass.set_bind_group(1, self.uniforms.bind_group(), &[d.offset]);
            pass.draw_indexed(piece.first..piece.first + piece.count, 0, 0..1);
        }
    }

    /// Draws the car's solid parts, after `WorldRenderer::draw_sky` (group 0 bound) and before `draw`.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((solid, _)) = &self.pipelines else { return };
        pass.set_pipeline(solid);
        self.run(pass, &self.solid);
    }

    /// Draws its glass, after `WorldRenderer::draw`: over the world seen through it. `second`: in
    /// the frame's second pass, after `before_wet`; from the seat the glass then has the rain's
    /// drops on it, which bend the scene the first pass left.
    pub fn draw_glass(&self, pass: &mut wgpu::RenderPass<'_>, world: &WorldRenderer, second: bool) {
        let Some((_, clear)) = &self.pipelines else { return };
        world.bind_frame(pass);
        let wet = self.wet_glass.as_ref().filter(|_| second && self.inside).and_then(|w| w.bind.as_ref().map(|(bind, _)| (&w.pipeline, bind)));
        let Some((pipeline, bind)) = wet else {
            pass.set_pipeline(clear);
            return self.run(pass, &self.clear);
        };
        // The glass with its drops, then the rest (a turning wheel's blur) as ever.
        let (glass, rest): (Vec<Draw>, Vec<Draw>) = self.clear.iter().partition(|d| self.parts[d.part].pieces[d.piece].kind == Kind::Glass);
        pass.set_pipeline(clear);
        self.run(pass, &rest);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(2, bind, &[]);
        self.run(pass, &glass);
    }

    /// The car's headlights as they are seen at night: two dipped beams, some 30 000 cd each,
    /// aimed a little down. (What they light is the circuit's to draw, `TrackRenderer::beams`;
    /// its tail lamps are lit lenses of the model, which the bloom makes glow.)
    pub fn lamps(&self, car: &Car, eye: DVec3, out: &mut Vec<Lamp>) {
        let b = car.body;
        let forward = to_render(act(b.q, DVec3::X));
        let down = to_render(act(b.q, DVec3::Z));
        let beam = (forward - down * 0.035).normalize();
        for y in [-0.68, 0.68] {
            let at = to_render(b.pos + act(b.q, DVec3::new(2.05, y, -0.18)) - eye);
            out.push(Lamp { at, color: Color::White, intensity: 30_000.0, omni: 0.0, beam: Some((beam, 24f32.to_radians(), 5f32.to_radians())), radius: 0.04 });
        }
    }
}

/// What a part's draw carries besides its material: a number for the kind (a needle's place, a
/// brake's heat, whether a wheel's spokes are blurred), another (a dial's red line) and a third
/// (the gear a dial shows).
#[derive(Clone, Copy, Default)]
struct Extra {
    value: f32,
    more: f32,
    third: f32,
}

/// A node of the model as a part: its triangles about its origin, in runs of one material each.
fn part(gpu: &Gpu, model: &Model, name: &str) -> Result<Part, String> {
    let node = model.node(name).ok_or_else(|| format!("no node \"{name}\""))?;
    let origin = node.origin();
    let mut b = MeshBuilder::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let reach = node.vertices.iter().map(|v| v.position.length()).fold(0.0, f32::max);
    // The triangles of each material together, in the order the materials first come.
    let mut order: Vec<usize> = Vec::new();
    for &m in &node.materials {
        if !order.contains(&m) {
            order.push(m);
        }
    }
    for m in order {
        let material = model.materials.get(m).cloned().unwrap_or_default();
        let first = b.count();
        let scale = material.base.truncate().max_element().max(1e-6);
        for (k, v) in node.vertices.iter().enumerate() {
            if node.materials[k / 3] != m {
                continue;
            }
            // The vertex colour is how open the point is to the sky; the model read has the material's in it.
            let open = (v.color.truncate().max_element() / scale).clamp(0.0, 1.0);
            b.vertex(v.position - origin, v.normal, glam::Vec4::new(open, 0.0, 0.0, 1.0));
        }
        pieces.push(Piece {
            first,
            count: b.count() - first,
            kind: Kind::of(&material.name),
            name: material.name,
            base: material.base.truncate().to_array(),
            roughness: material.roughness,
            metallic: material.metallic,
            emissive: material.emissive.to_array(),
        });
    }
    Ok(Part { mesh: b.build_indexed(&gpu.device), pieces, origin, reach })
}

/// Body axes to render axes relative to the camera, for a body at `rel` (world axes) turned by `q`.
pub fn body_matrix(q: DQuat, rel: DVec3) -> Mat4 {
    let axis = |v: DVec3| to_render(act(q, v));
    Mat4::from_cols(axis(DVec3::X).extend(0.0), axis(DVec3::Y).extend(0.0), axis(DVec3::Z).extend(0.0), to_render(rel).extend(1.0))
}

/// The shadow of a car: its body and cabin lying flat, their sides standing, its tyres as solids.
pub fn shadow(car: &Car) -> ShadowShape {
    let g = car.p.cg_height as f32;
    let mut s = ShadowShape {
        // The body, its nose and tail narrower, and the cabin.
        flat: [[0.0, 1.95, 0.0, 0.92], [-0.45, 1.05, 0.0, 0.62], [2.07, 0.14, 0.0, 0.72], [-2.08, 0.13, 0.0, 0.76]],
        flat_z: [g - 0.40, g - 1.20, g - 0.45, g - 0.60],
        upright: [[0.0, 2.15, g - 0.55, 0.36], [-0.45, 1.05, g - 1.08, 0.2], [0.0; 4]],
        ..ShadowShape::default()
    };
    for (i, w) in car.wheels().iter().enumerate() {
        let hub = car.p.hub(i) + DVec3::new(0.0, 0.0, w.travel);
        let r = car.p.axles[i / 2].tyre.radius as f32;
        let half = car.p.axles[i / 2].tyre.width as f32 / 2.0;
        s.solids[i] = [[hub.x as f32, hub.y as f32, hub.z as f32, 1.0], [r, half, r, 0.0]];
    }
    // The floor, as a flat solid: it darkens the road under the car, by day and by night.
    s.solids[4] = [[0.1, 0.0, g - 0.22, 1.0], [1.85, 0.72, 0.07, 0.0]];
    s
}
