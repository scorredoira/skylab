//! What the tyres leave behind, from what the physics says each wheel is doing (`WheelState`: how
//! much it slips, on what, under what load): the marks of rubber laid on the asphalt, which stay;
//! the smoke of a tyre that spins, locks or slides; the dust of gravel and grass; the spray of a
//! wet road. Nothing here changes how the car drives.
//!
//! The marks are ribbons in one buffer that goes round (the oldest give way to the newest), drawn
//! over the circuit; the puffs are squares that face the eye, sorted far to near, drawn after the
//! world. Their shaders are `effects.wgsl`.

use engine::pipeline::{Depth, PipelineDesc};
use engine::{wgpu, Gpu, MeshBuilder, UniformArena};
use glam::{DVec3, Mat4, Vec3};
use sim_core::car::Surface;
use sim_core::{act, Car};
use world_render::{to_render, ObjectUniforms, WorldRenderer};

/// Quads of marks kept: at 0.25 m each, some 2.5 km of them.
const MARKS: usize = 10_000;
/// The most puffs alive at once.
const PUFFS: usize = 700;
/// Drops of rain drawn round the eye, within this many metres of it.
const DROPS: usize = 3000;
const RAIN_REACH: f64 = 9.0;
/// How fast a drop falls, m/s.
const RAIN_FALL: f64 = 8.5;
/// A quad's vertices and the bytes of one (`MeshBuilder::layout`).
const QUAD: usize = 6;
const VERTEX: usize = 40;

/// The end of a mark being laid by a wheel: its two edges and how dark it is there.
#[derive(Clone, Copy)]
struct End {
    left: DVec3,
    right: DVec3,
    strength: f32,
}

/// A puff of smoke, dust or spray.
#[derive(Clone, Copy)]
struct Puff {
    at: DVec3,
    velocity: DVec3,
    age: f32,
    life: f32,
    /// Its radius when made and when it is gone, m.
    size: (f32, f32),
    color: [f32; 3],
    opacity: f32,
    seed: f32,
}

pub struct Effects {
    /// Where the marks' vertices are measured from (world axes).
    origin: DVec3,
    marks: wgpu::Buffer,
    /// The next quad to write, and how many have been written (up to `MARKS`).
    next: usize,
    laid: usize,
    ends: [Option<End>; 4],
    /// Quads made since the last upload: (slot, vertices).
    fresh: Vec<(usize, [f32; QUAD * 10])>,
    puffs: Vec<Puff>,
    puff_buffer: wgpu::Buffer,
    puff_count: u32,
    /// How hard it rains, 0 to 1, the air's velocity, and seconds of rain so far.
    rain: f32,
    wind: DVec3,
    rained: f64,
    /// How fast the eye moves (the drops streak past it).
    eye_velocity: DVec3,
    /// What each wheel still owes in puffs (a fraction of one).
    owed: [f32; 4],
    made: u32,
    pipelines: Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>,
    /// Whether this one draws the rain's drops (one of the cars' effects does).
    pub drops: bool,
    uniforms: UniformArena,
    offsets: (u32, u32),
}

impl Effects {
    pub fn new(gpu: &Gpu, world: &WorldRenderer, origin: DVec3) -> Effects {
        let buffer = |label, bytes: usize| gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: bytes as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Effects {
            origin,
            marks: buffer("tyre marks", MARKS * QUAD * VERTEX),
            next: 0,
            laid: 0,
            ends: [None; 4],
            fresh: Vec::new(),
            puffs: Vec::new(),
            puff_buffer: buffer("puffs", (PUFFS + DROPS) * QUAD * VERTEX),
            rain: 0.0,
            wind: DVec3::ZERO,
            rained: 0.0,
            eye_velocity: DVec3::ZERO,
            puff_count: 0,
            owed: [0.0; 4],
            made: 0,
            pipelines: None,
            drops: true,
            uniforms: UniformArena::new(gpu, world.object_layout(), size_of::<ObjectUniforms>(), 2),
            offsets: (0, 0),
        }
    }

    /// The pipelines, from the game's shader module (the world's shaders with `car.wgsl` and
    /// `effects.wgsl`): made again when the quality or the samples change.
    pub fn pipelines(&mut self, gpu: &Gpu, world: &WorldRenderer, shader: &wgpu::ShaderModule) {
        let buffers = [MeshBuilder::layout()];
        let groups = [world.frame_layout(), world.object_layout()];
        let desc = |vertex: &'static str, fragment: &'static str| PipelineDesc { label: "tyre effects", shader, vertex, fragment, buffers: &buffers, bind_groups: &groups, depth: Depth::Read, blend: false };
        let blend = Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        self.pipelines = Some((gpu.pipeline_blended(&desc("mark_vertex", "mark_fragment"), blend), gpu.pipeline_blended(&desc("puff_vertex", "puff_fragment"), blend)));
    }

    /// Forgets the marks being laid (the car was moved): the next ones start afresh.
    pub fn lift(&mut self) {
        self.ends = [None; 4];
    }

    /// `dt` seconds of the car as it is: marks and puffs made, puffs aged. `wet`: how wet the
    /// road is, 0 to 1; `wind`: the air's velocity (world axes).
    pub fn update(&mut self, car: &Car, dt: f64, wet: f32, wind: DVec3) {
        self.rain = wet;
        self.wind = wind;
        self.rained += dt;
        let b = car.body;
        let up = DVec3::NEG_Z;
        for (i, w) in car.wheels().iter().enumerate() {
            let tyre = &car.p.axles[i / 2].tyre;
            let hub = car.p.hub(i) + DVec3::new(0.0, 0.0, w.travel);
            // Where the tyre touches, and the way it points and lies across.
            let foot = b.pos + act(b.q, hub + DVec3::new(0.0, 0.0, tyre.radius));
            let (s, c) = w.steer.sin_cos();
            let along = act(b.q, DVec3::new(c, s, 0.0));
            let across = act(b.q, DVec3::new(-s, c, 0.0));
            let ground_speed = (b.world_velocity() + act(b.q, b.omega.cross(hub))).length();
            let sliding = (w.omega * tyre.radius - ground_speed).abs().max(w.slip_angle.abs() * ground_speed);
            // How hard the tyre is being rubbed: its slip beyond what grips, times its load.
            let slip = ((w.slip.abs() - 0.10) / 0.25).max((w.slip_angle.abs() - 0.10) / 0.22).clamp(0.0, 1.0);
            let load = (w.load / tyre.nominal_load).clamp(0.0, 1.6) as f32;
            let rub = if w.contact && ground_speed > 1.5 || w.contact && sliding > 2.0 { slip as f32 * load.min(1.0) } else { 0.0 };
            let paved = matches!(w.surface, Surface::Asphalt | Surface::Kerb | Surface::Concrete);

            // The mark: a ribbon as wide as the tread, from where the last piece ended.
            let strength = if paved { rub * (1.0 - 0.6 * wet) } else { 0.0 };
            if strength > 0.04 {
                let half = across * (tyre.width * 0.42);
                let lifted = foot + up * 0.035;
                let end = End { left: lifted - half, right: lifted + half, strength };
                match self.ends[i] {
                    Some(last) if (last.left - end.left).length() < 0.25 => {}
                    Some(last) if (last.left - end.left).length() < 3.0 => {
                        self.quad(last, end);
                        self.ends[i] = Some(end);
                    }
                    // The first of a mark starts from nothing.
                    _ => self.ends[i] = Some(End { strength: 0.0, ..end }),
                }
            } else if let Some(last) = self.ends[i].take() {
                // The mark fades out over its last piece.
                let lifted = foot + up * 0.035;
                let half = across * (tyre.width * 0.42);
                if (last.left - (lifted - half)).length() < 3.0 && last.strength > 0.0 {
                    self.quad(last, End { left: lifted - half, right: lifted + half, strength: 0.0 });
                }
            }

            // Puffs, thrown back from the tyre: smoke, dust or spray.
            let back = -along * (w.omega * tyre.radius).clamp(-12.0, 12.0) * 0.25 + b.world_velocity() * 0.25;
            let (rate, puff) = match w.surface {
                _ if !w.contact => (0.0, None),
                Surface::Asphalt | Surface::Kerb | Surface::Concrete => {
                    let smoke = ((rub - 0.3) / 0.7).max(0.0) * (sliding as f32 / 6.0).min(1.0);
                    let spray = wet * ((ground_speed as f32 - 8.0) / 40.0).clamp(0.0, 1.0);
                    if smoke * (1.0 - wet) > spray * 0.5 {
                        (70.0 * smoke * (1.0 - wet), Some(([0.80, 0.80, 0.82], 0.30, 1.9, (0.28, 1.7), 0.7)))
                    } else {
                        (if i < 2 { 5.0 } else { 16.0 } * spray, Some(([0.80, 0.84, 0.88], 0.055, 1.4, (0.3, 2.8), 1.1)))
                    }
                }
                Surface::Gravel | Surface::Dirt => {
                    let stirred = ((ground_speed as f32 - 3.0) / 25.0).clamp(0.0, 1.0).max(rub);
                    (55.0 * stirred, Some(([0.55, 0.47, 0.36], 0.34, 2.6, (0.35, 2.4), 0.5)))
                }
                Surface::Grass => {
                    let stirred = ((ground_speed as f32 - 6.0) / 30.0).clamp(0.0, 1.0) * (0.3 + 0.7 * rub);
                    (30.0 * stirred, Some(([0.40, 0.38, 0.26], 0.22, 1.6, (0.3, 1.5), 0.4)))
                }
                Surface::Water => (0.0, None),
            };
            if let Some((color, opacity, life, size, rise)) = puff {
                self.owed[i] += rate * dt as f32;
                while self.owed[i] >= 1.0 {
                    self.owed[i] -= 1.0;
                    self.made = self.made.wrapping_add(1);
                    let r = |k: u32| hash(self.made, k);
                    let spread = across * ((r(1) - 0.5) as f64 * tyre.width) + along * ((r(2) - 0.5) as f64 * 0.3);
                    let velocity = back * (0.6 + 0.8 * r(3) as f64) + up * (rise * (0.4 + r(4)) as f64) + across * ((r(5) - 0.5) as f64 * 1.2);
                    if self.puffs.len() >= PUFFS {
                        self.puffs.remove(0);
                    }
                    self.puffs.push(Puff { at: foot + spread + up * 0.12, velocity, age: 0.0, life: life * (0.7 + 0.6 * r(6)), size, color, opacity, seed: r(7) });
                }
            } else {
                self.owed[i] = 0.0;
            }
        }
        // The puffs drift with the air, slow down, rise a little and die.
        for p in &mut self.puffs {
            p.age += dt as f32;
            let drag = (-2.2 * dt).exp();
            p.velocity = wind + (p.velocity - wind) * drag;
            p.at += p.velocity * dt;
        }
        self.puffs.retain(|p| p.age < p.life);
    }

    /// A piece of a mark between two ends.
    fn quad(&mut self, a: End, b: End) {
        let v = |p: DVec3, across: f32, strength: f32| -> [f32; 10] {
            let r = to_render(p - self.origin);
            [r.x, r.y, r.z, across, 0.0, 0.0, 0.0, 0.0, 0.0, strength]
        };
        let corners = [v(a.left, -1.0, a.strength), v(a.right, 1.0, a.strength), v(b.right, 1.0, b.strength), v(a.left, -1.0, a.strength), v(b.right, 1.0, b.strength), v(b.left, -1.0, b.strength)];
        let mut data = [0.0f32; QUAD * 10];
        for (k, c) in corners.iter().enumerate() {
            data[k * 10..k * 10 + 10].copy_from_slice(c);
        }
        self.fresh.push((self.next, data));
        self.next = (self.next + 1) % MARKS;
        self.laid = (self.laid + 1).min(MARKS);
    }

    /// Before the frame is drawn, seen from `eye` (world axes) by a camera whose right and up
    /// are these (render axes).
    pub fn prepare(&mut self, gpu: &Gpu, eye: DVec3, right: Vec3, up: Vec3, eye_velocity: DVec3) {
        self.eye_velocity = eye_velocity;
        for (slot, data) in self.fresh.drain(..) {
            gpu.queue.write_buffer(&self.marks, (slot * QUAD * VERTEX) as u64, bytemuck::cast_slice(&data));
        }
        // The puffs as squares facing the eye, the farthest first.
        let mut order: Vec<(f32, &Puff)> = self.puffs.iter().map(|p| ((p.at - eye).length_squared() as f32, p)).collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut data: Vec<f32> = Vec::with_capacity(order.len() * QUAD * 10);
        for (_, p) in &order {
            let t = p.age / p.life;
            let size = p.size.0 + (p.size.1 - p.size.0) * (1.0 - (1.0 - t) * (1.0 - t));
            // It comes up quickly and thins away.
            let opacity = p.opacity * (t / 0.08).min(1.0) * (1.0 - t) * (1.0 - t);
            let centre = to_render(p.at - eye) + Vec3::Y * size * 0.35;
            for (x, y) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let at = centre + (right * x + up * y) * size;
                data.extend_from_slice(&[at.x, at.y, at.z, x, y, p.seed, p.color[0], p.color[1], p.color[2], opacity]);
            }
        }
        // The rain: each drop falls down a column of its own in a box that goes with the eye, and
        // is drawn as the streak it leaves in a fiftieth of a second as the eye sees it pass.
        let mut drops = 0;
        if self.rain > 0.0 && self.drops {
            let fall = DVec3::new(self.wind.x, self.wind.y, RAIN_FALL);
            let streak = to_render((fall - self.eye_velocity) * 0.012);
            let toward = Vec3::ZERO;
            let count = (DROPS as f32 * self.rain) as u32;
            for k in 0..count {
                // Its place in the box: where it began, moved by its fall, wrapped round the eye.
                let cell = |seed: u32, moved: f64, centre: f64| -> f64 {
                    let at = hash(k, seed) as f64 * 2.0 * RAIN_REACH + moved;
                    (at - centre).rem_euclid(2.0 * RAIN_REACH) - RAIN_REACH
                };
                let rel = DVec3::new(cell(11, fall.x * self.rained, eye.x), cell(12, fall.y * self.rained, eye.y), cell(13, fall.z * self.rained, eye.z));
                let at = to_render(rel);
                let along = streak;
                let side = along.cross(at - toward).normalize_or_zero() * (0.003 + 0.0010 * at.length());
                let fade = ((1.0 - rel.length() / RAIN_REACH).clamp(0.0, 1.0) * ((rel.length() - 0.8) / 1.7).clamp(0.0, 1.0)) as f32;
                if fade <= 0.0 || side == Vec3::ZERO {
                    continue;
                }
                for (x, y) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    let p = at + side * x + along * (y * 0.5);
                    // A number below zero says it is a drop.
                    data.extend_from_slice(&[p.x, p.y, p.z, x, y, -1.0, 0.75, 0.8, 0.85, 0.22 * fade]);
                }
                drops += 1;
            }
        }
        self.puff_count = ((order.len() + drops) * QUAD) as u32;
        if !data.is_empty() {
            gpu.queue.write_buffer(&self.puff_buffer, 0, bytemuck::cast_slice(&data));
        }
        self.uniforms.clear();
        let marks = Mat4::from_translation(to_render(self.origin - eye));
        let white = [1.0; 4];
        self.offsets = (self.uniforms.push(&ObjectUniforms::new(marks.to_cols_array_2d(), white)), self.uniforms.push(&ObjectUniforms::new(Mat4::IDENTITY.to_cols_array_2d(), white)));
        self.uniforms.upload(gpu);
    }

    /// Draws the marks, over the circuit (after `TrackRenderer::draw`, group 0 bound).
    pub fn draw_marks(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((marks, _)) = &self.pipelines else { return };
        if self.laid == 0 {
            return;
        }
        pass.set_pipeline(marks);
        pass.set_bind_group(1, self.uniforms.bind_group(), &[self.offsets.0]);
        pass.set_vertex_buffer(0, self.marks.slice(..));
        pass.draw(0..(self.laid * QUAD) as u32, 0..1);
    }

    /// Draws the puffs, after the world (group 0 bound).
    pub fn draw_puffs(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((_, puffs)) = &self.pipelines else { return };
        if self.puff_count == 0 {
            return;
        }
        pass.set_pipeline(puffs);
        pass.set_bind_group(1, self.uniforms.bind_group(), &[self.offsets.1]);
        pass.set_vertex_buffer(0, self.puff_buffer.slice(..));
        pass.draw(0..self.puff_count, 0..1);
    }
}

/// A number from 0 to 1 for a puff and one of its properties.
fn hash(i: u32, k: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / 16_777_216.0
}
