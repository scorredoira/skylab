//! A race course's gates drawn into the world (`sim_core::course`): each gate's frame from
//! `assets/courses/gates.glb` (made by `examples/gate_models.rs` from the same shapes the
//! simulation runs into) painted its colour, its LED strips shining in its colour, brighter at the gate to fly
//! through next (pulsing) and dim at those flown this lap, its number on its plate, and the start's
//! pad. The strips and digits are lights: drawn by themselves (`ObjectDraw` with a tint's alpha
//! over 1.5), as bright as LEDs are (some 15 000 cd/m², the next gate 45 000), so by day they are
//! coloured tubes and at dusk and at night they glow, the bloom round them.

use engine::MeshBuilder;
use glam::{Mat4, Vec3};
use sim_core::course::{GateKind, Track};
use world_render::{to_render, MeshId, ObjectDraw, WorldRenderer};

/// The model file, built into the program.
const GATES: &[u8] = include_bytes!("../../../../../assets/courses/gates.glb");

/// The strips' luminance, cd/m²: a gate's, the next one's at its brightest, a gate flown.
const LED: f32 = 15_000.0;
const NEXT: f32 = 45_000.0;
const FLOWN: f32 = 3_000.0;
/// Gates farther than this from the eye are not drawn, m.
const FARTHEST: f64 = 3000.0;

/// The colours of the gates' strips, in turn round the course (linear, of luminance about 1).
const COLOURS: [[f32; 3]; 6] = [[0.1, 0.9, 1.6], [1.9, 0.15, 1.3], [0.35, 1.3, 0.1], [2.2, 0.75, 0.05], [0.7, 0.3, 2.6], [2.0, 0.25, 0.2]];

/// What the renderer keeps of the gates' models: one mesh and each node's range in it.
pub struct GateModels {
    mesh: MeshId,
    /// Per kind (`GateKind::ALL`'s order): the frame's range and the strips'.
    frames: Vec<(u32, u32)>,
    leds: Vec<(u32, u32)>,
    digits: Vec<(u32, u32)>,
    pad: (u32, u32),
}

/// What the course looks like this frame.
#[derive(Clone, Copy, Debug)]
pub struct CourseLook<'a> {
    pub track: &'a Track,
    /// The gate to fly through next, while racing (none before the start and after the finish).
    pub next: Option<usize>,
    /// Gates flown through this lap (the next's index is past them).
    pub lap_started: bool,
    /// The pads of the start, one for each quad racing: metres to the right of its middle.
    pub pads: &'a [f64],
}

impl GateModels {
    /// Reads the models and gives them to the world.
    pub fn new(gpu: &engine::Gpu, world: &mut WorldRenderer) -> Result<GateModels, String> {
        let model = engine::gltf::read(GATES).map_err(|e| format!("assets/courses/gates.glb: {e}"))?;
        let mut mesh = MeshBuilder::new();
        let mut add = |name: &str| -> Result<(u32, u32), String> {
            let node = model.node(name).ok_or_else(|| format!("assets/courses/gates.glb: no node {name}"))?;
            let first = mesh.count();
            mesh.data.extend(model.mesh(node).data);
            Ok((first, mesh.count() - first))
        };
        let frames = GateKind::ALL.iter().map(|k| add(k.word())).collect::<Result<Vec<_>, _>>()?;
        let leds = GateKind::ALL.iter().map(|k| add(&format!("{}_led", k.word()))).collect::<Result<Vec<_>, _>>()?;
        let digits = (0..10).map(|d| add(&format!("digit_{d}"))).collect::<Result<Vec<_>, _>>()?;
        let pad = add("pad")?;
        Ok(GateModels { mesh: world.add_mesh(gpu, &mesh), frames, leds, digits, pad })
    }

    /// The draws of a course seen from `eye` (world axes) at `hour` and `time` (s).
    pub fn draws(&self, look: &CourseLook, eye: sim_core::V3, hour: f64, time: f64, objects: &mut Vec<ObjectDraw>) {
        let gain = world_render::light_gain(hour);
        let track = look.track;
        let draw = |range: (u32, u32), model: Mat4, tint: [f32; 4], objects: &mut Vec<ObjectDraw>| {
            if range.1 > 0 {
                objects.push(ObjectDraw { mesh: self.mesh, first: range.0, count: range.1, model, tint, cuts: [None; 2] });
            }
        };
        for (i, g) in track.gates.iter().enumerate() {
            if (g.foot - eye).length() > FARTHEST {
                continue;
            }
            let (f, r, u) = g.axes();
            let s = g.scale as f32;
            let at = to_render(g.foot - eye);
            let model = Mat4::from_cols((to_render(f) * s).extend(0.0), (to_render(r) * s).extend(0.0), (to_render(u) * s).extend(0.0), at.extend(1.0));
            let kind = g.kind.index();
            // The frame painted the gate's colour (the arch and the log keep their own); the strips the same,
            // the next gate's pulsing bright, those flown this lap dim.
            let colour = Vec3::from(COLOURS[i % COLOURS.len()]);
            let paint = if g.kind == GateKind::Arch || g.kind == GateKind::Log { Vec3::ONE } else { Vec3::splat(0.2) + colour / colour.max_element() * 0.8 };
            draw(self.frames[kind], model, [paint.x, paint.y, paint.z, 1.0], objects);
            let luminance = match look.next {
                Some(next) if next == i => NEXT * (0.75 + 0.25 * (time * 2.0 * std::f64::consts::TAU).sin() as f32),
                Some(next) if look.lap_started && i < next => FLOWN,
                _ => LED,
            };
            let light = colour * luminance * gain;
            draw(self.leds[kind], model, [light.x, light.y, light.z, 2.0], objects);
            // Its number on the plate, facing the racers coming; the start and finish has its banner.
            if g.kind == GateKind::Arch {
                continue;
            }
            let number = sim_core::course::shape(g.kind).number;
            let text = i.to_string();
            let height = 0.42f32;
            let advance = 0.62 * height;
            let white = Vec3::splat(1.0) * LED * 1.4 * gain;
            for (k, ch) in text.chars().enumerate() {
                let d = ch.to_digit(10).unwrap_or(0) as usize;
                let y = number.1 as f32 + (k as f32 - (text.len() as f32 - 1.0) / 2.0) * advance;
                let place = Mat4::from_translation(Vec3::new(number.0 as f32 - 0.025, y, number.2 as f32)) * Mat4::from_scale(Vec3::splat(height));
                draw(self.digits[d], model * place, [white.x, white.y, white.z, 2.0], objects);
            }
        }
        // The start's pads, side by side.
        let (f, r, u) = (sim_core::V3::new(track.start_heading.cos(), track.start_heading.sin(), 0.0), sim_core::V3::new(-track.start_heading.sin(), track.start_heading.cos(), 0.0), sim_core::V3::new(0.0, 0.0, -1.0));
        for &across in look.pads {
            let at = track.start + r * across;
            if (at - eye).length() < FARTHEST {
                let model = Mat4::from_cols(to_render(f).extend(0.0), to_render(r).extend(0.0), to_render(u).extend(0.0), to_render(at - eye).extend(1.0));
                draw(self.pad, model, [1.0; 4], objects);
            }
        }
    }
}
