//! The driving line: arrows on the asphalt ahead of the car along the racing line
//! (`sim_core::car::driver::RacingLine`, the racing driver's), coloured by the car's speed against
//! the speed the line asks for where each arrow lies: green where the car can go faster, yellow
//! where it is about as fast as the line, red where it must be slower by then than it is now, so
//! the red begins where the braking must. On a wet road the line's speeds are the wet road's.
//! `L` goes through its modes: the whole line, only where to slow down, off.

use engine::pipeline::{Depth, PipelineDesc};
use engine::{wgpu, Gpu, MeshBuilder, UniformArena};
use glam::{DVec3, Mat4};
use sim_core::car::driver::{Pace, RacingLine};
use sim_core::circuit::Circuit;
use sim_core::Car;
use world_render::{to_render, ObjectUniforms, WorldRenderer};

/// What of the line is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// All of it ahead of the car.
    Full,
    /// Only where the car must slow down or is as fast as it can be.
    Braking,
    Off,
}

impl Mode {
    pub fn parse(word: &str) -> Option<Mode> {
        match word {
            "full" | "on" => Some(Mode::Full),
            "braking" | "brake" => Some(Mode::Braking),
            "off" | "none" => Some(Mode::Off),
            _ => None,
        }
    }

    pub fn next(self) -> Mode {
        match self {
            Mode::Full => Mode::Braking,
            Mode::Braking => Mode::Off,
            Mode::Off => Mode::Full,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Full => "the whole line",
            Mode::Braking => "only where to slow down",
            Mode::Off => "off",
        }
    }
}

/// Metres between arrows, how far ahead of the car the first lies, and how many there are.
const SPACING: f64 = 6.0;
const FIRST: f64 = 7.0;
const ARROWS: usize = 40;
/// An arrow's half width, its length and the width of its arms, m; how far over the road it lies.
const HALF: f64 = 1.1;
const LENGTH: f64 = 3.2;
const ARM: f64 = 1.5;
const LIFT: f64 = 0.04;
/// Vertices of an arrow (two arms, a quad each) and the floats of a vertex (`MeshBuilder::layout`).
const VERTICES: usize = 12;
const FLOATS: usize = 10;

/// How much faster (m/s) than the line asks the car is where an arrow lies, as a colour (linear)
/// and whether it is one of slowing down: green below, through yellow about the line's speed, to
/// red from 5 m/s over.
pub fn colour(over: f64) -> ([f32; 3], bool) {
    const GREEN: [f32; 3] = [0.05, 0.85, 0.20];
    const YELLOW: [f32; 3] = [1.0, 0.72, 0.03];
    const RED: [f32; 3] = [1.0, 0.06, 0.04];
    let mix = |a: [f32; 3], b: [f32; 3], t: f64| std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t.clamp(0.0, 1.0) as f32);
    if over < 0.0 {
        (mix(GREEN, YELLOW, (over + 3.0) / 3.0), over > -1.5)
    } else {
        (mix(YELLOW, RED, over / 5.0), true)
    }
}

/// An arrow: where it lies (world axes, on the road), the way it points (north, east), its
/// colour and how much of it shows (0 to 1: they come in from the car and fade out far ahead).
pub struct Arrow {
    pub at: DVec3,
    pub way: (f64, f64),
    pub colour: [f32; 3],
    pub shows: f32,
}

pub struct Guide {
    pub mode: Mode,
    line: RacingLine,
    /// The water (mm) the line's speeds are for.
    film: f64,
    buffer: wgpu::Buffer,
    count: u32,
    pipeline: Option<wgpu::RenderPipeline>,
    uniforms: UniformArena,
    offset: u32,
}

impl Guide {
    pub fn new(gpu: &Gpu, world: &WorldRenderer, circuit: &Circuit, car: &Car, mode: Mode) -> Guide {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("driving line"), size: (ARROWS * VERTICES * FLOATS * 4) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Guide { mode, line: RacingLine::new(circuit, &car.p, Pace::default()), film: 0.0, buffer, count: 0, pipeline: None, uniforms: UniformArena::new(gpu, world.object_layout(), size_of::<ObjectUniforms>(), 1), offset: 0 }
    }

    /// The pipeline, from the game's shader module (with `guide.wgsl`).
    pub fn pipeline(&mut self, gpu: &Gpu, world: &WorldRenderer, shader: &wgpu::ShaderModule) {
        let buffers = [MeshBuilder::layout()];
        let groups = [world.frame_layout(), world.object_layout()];
        let desc = PipelineDesc { label: "driving line", shader, vertex: "guide_vertex", fragment: "guide_fragment", buffers: &buffers, bind_groups: &groups, depth: Depth::Read, blend: false };
        self.pipeline = Some(gpu.pipeline_blended(&desc, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)));
    }

    /// The arrows ahead of `car`. None off the circuit, backing or with the line off.
    pub fn arrows(&mut self, circuit: &Circuit, car: &Car) -> Vec<Arrow> {
        let b = &car.body;
        let Some(at) = circuit.locate(b.pos.x, b.pos.y).filter(|_| self.mode != Mode::Off && b.vel.x > -1.0) else { return Vec::new() };
        // The line's speeds for the water under the tyres (the least of the four: a puddle is not
        // the road), as the racing driver has them.
        let under = car.wheels().iter().map(|w| w.water).fold(f64::INFINITY, f64::min).max(0.0) * 1000.0;
        let film = if under < 0.02 { 0.0 } else { (under * 10.0).round() / 10.0 };
        if (film - self.film).abs() > 1e-9 {
            self.film = film;
            if film <= 0.0 {
                self.line.respeed(&car.p, Pace::default(), &|_| 1.0);
            } else {
                self.line.respeed(&car.p, Pace::default(), &|v| sim_core::car::wet::wet_asphalt(film, v * 3.6));
            }
        }
        let v = b.vel.x.max(0.0);
        let count = self.line.across.len();
        let across = |along: f64| {
            let s = along.rem_euclid(circuit.length) / sim_core::circuit::STEP;
            let (i, t) = (s.floor() as usize % count, s - s.floor());
            self.line.across[i] + (self.line.across[(i + 1) % count] - self.line.across[i]) * t
        };
        // The arrows stay where they are on the road as the car comes: every `SPACING` metres along.
        let first = ((at.along + FIRST) / SPACING).ceil() * SPACING;
        let mut out = Vec::with_capacity(ARROWS);
        for k in 0..ARROWS {
            let along = first + k as f64 * SPACING;
            let ahead = along - at.along;
            let (_, speed) = self.line.at(circuit, along);
            let (rgb, slowing) = colour(v - speed);
            if self.mode == Mode::Braking && !slowing {
                continue;
            }
            let (n, e, h) = circuit.point(along, across(along));
            let (n2, e2, _) = circuit.point(along + 1.0, across(along + 1.0));
            let length = (n2 - n).hypot(e2 - e).max(1e-6);
            let reach = FIRST + ARROWS as f64 * SPACING;
            let shows = ((ahead - FIRST + 2.0) / 6.0).clamp(0.0, 1.0) * ((reach - ahead) / 50.0).clamp(0.0, 1.0);
            out.push(Arrow { at: DVec3::new(n, e, -(h + LIFT)), way: ((n2 - n) / length, (e2 - e) / length), colour: rgb, shows: shows as f32 });
        }
        out
    }

    /// Before the frame is drawn, seen from `eye` (world axes).
    pub fn prepare(&mut self, gpu: &Gpu, circuit: &Circuit, car: &Car, eye: DVec3) {
        let arrows = self.arrows(circuit, car);
        let mut data: Vec<f32> = Vec::with_capacity(arrows.len() * VERTICES * FLOATS);
        for Arrow { at, way: (fn_, fe), colour: rgb, shows } in &arrows {
            // A chevron pointing the way: two arms from its tip back to each side. `across` runs
            // over each arm's width (-1 to 1), for its soft edges.
            let forward = DVec3::new(*fn_, *fe, 0.0);
            let side = DVec3::new(-fe, *fn_, 0.0);
            let tip = *at + forward * (LENGTH / 2.0);
            for s in [-1.0, 1.0] {
                let wing = *at + side * (s * HALF) - forward * (LENGTH / 2.0);
                let back = forward * ARM;
                let corners = [(tip, -1.0f32), (wing, -1.0), (wing - back, 1.0), (tip, -1.0), (wing - back, 1.0), (tip - back, 1.0)];
                for (p, across) in corners {
                    let r = to_render(p - eye);
                    data.extend_from_slice(&[r.x, r.y, r.z, across, 0.0, 0.0, rgb[0], rgb[1], rgb[2], *shows]);
                }
            }
        }
        self.count = (data.len() / FLOATS) as u32;
        if !data.is_empty() {
            gpu.queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&data));
        }
        self.uniforms.clear();
        self.offset = self.uniforms.push(&ObjectUniforms::new(Mat4::IDENTITY.to_cols_array_2d(), [1.0; 4]));
        self.uniforms.upload(gpu);
    }

    /// Nothing of it this frame.
    pub fn hide(&mut self) {
        self.count = 0;
    }

    /// Draws the arrows, over the circuit (after `TrackRenderer::draw`, group 0 bound).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(pipeline) = &self.pipeline else { return };
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, self.uniforms.bind_group(), &[self.offset]);
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Green well under the line's speed, yellow about it, red well over; slowing from a little
    /// under it on.
    #[test]
    fn the_colour_says_faster_or_slower() {
        let (green, slowing) = colour(-10.0);
        assert!(green[1] > 0.8 && green[0] < 0.1 && !slowing);
        let (yellow, slowing) = colour(0.0);
        assert!(yellow[0] > 0.9 && yellow[1] > 0.6 && slowing);
        let (red, slowing) = colour(8.0);
        assert!(red[0] > 0.9 && red[1] < 0.1 && slowing);
        assert!(!colour(-2.0).1 && colour(-1.0).1);
        for mode in [Mode::Full, Mode::Braking, Mode::Off] {
            assert_eq!(mode.next().next().next(), mode);
        }
        assert_eq!(Mode::parse("braking"), Some(Mode::Braking));
    }
}
