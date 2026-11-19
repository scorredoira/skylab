//! The ghost of the best lap, in practice: the player's quickest lap within the track's limits
//! driven again, as a see-through car, beside the player as they drive the next ones, and the time
//! between them as they go (ahead or behind, at each point of the lap).
//!
//! Each lap is recorded from line to line twenty times a second (`GhostFrame`: where the car is,
//! its attitude, how fast it goes, its wheels on their springs, steered and turning). A lap that is
//! the best for the circuit and the car, ever, becomes the ghost and is kept in the game's settings
//! folder (`$XDG_CONFIG_HOME/skylab-drive/ghosts/<circuit>-<car>.ghost`, or under `~/.config`),
//! so it is there next time. It is drawn by the car's own shader (`car_shade`, `car.wgsl`), at a
//! third of its opacity (`ghost.wgsl`).

use std::path::PathBuf;

use engine::pipeline::{Depth, PipelineDesc};
use engine::{wgpu, Gpu, MeshBuilder};
use glam::DVec3;
use sim_core::circuit::Circuit;
use sim_core::{Body, Car, CarLook, Quat, World};
use world_render::WorldRenderer;

/// Seconds between the frames of a lap.
pub const EVERY: f64 = 1.0 / 20.0;
/// The first bytes of a ghost's file.
const MAGIC: &[u8; 8] = b"SKGHOST1";

/// The car at a moment of a lap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GhostFrame {
    pub pos: [f64; 3],
    pub q: [f64; 4],
    /// Speed along the body, m/s.
    pub speed: f32,
    /// Per wheel: travel, steer, angle, spin.
    pub wheels: [[f32; 4]; 4],
    pub steer: f32,
    pub brake: f32,
}

impl GhostFrame {
    fn of(car: &Car) -> GhostFrame {
        let b = car.body;
        let w = car.wheels();
        GhostFrame {
            pos: b.pos.to_array(),
            q: [b.q.x, b.q.y, b.q.z, b.q.w],
            speed: b.vel.x as f32,
            wheels: std::array::from_fn(|i| [w[i].travel as f32, w[i].steer as f32, w[i].angle as f32, w[i].omega as f32]),
            steer: car.controls.steer as f32,
            brake: car.controls.brake as f32,
        }
    }

    /// A car's look with this frame's body and wheels (the rest from `base`).
    fn look(&self, base: &CarLook) -> CarLook {
        let mut look = *base;
        look.body = Body { pos: DVec3::from_array(self.pos), vel: DVec3::new(self.speed as f64, 0.0, 0.0), q: Quat::from_xyzw(self.q[0], self.q[1], self.q[2], self.q[3]), omega: DVec3::ZERO };
        for (w, f) in look.wheels.iter_mut().zip(&self.wheels) {
            (w.travel, w.steer, w.angle, w.omega) = (f[0] as f64, f[1] as f64, f[2] as f64, f[3] as f64);
            w.contact = true;
        }
        look.controls.steer = self.steer as f64;
        look.controls.brake = self.brake as f64;
        look
    }

    fn blend(&self, next: &GhostFrame, t: f64) -> GhostFrame {
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        let qa = Quat::from_xyzw(self.q[0], self.q[1], self.q[2], self.q[3]);
        let qb = Quat::from_xyzw(next.q[0], next.q[1], next.q[2], next.q[3]);
        let q = qa.slerp(qb, t);
        let mut out = *self;
        out.pos = std::array::from_fn(|i| lerp(self.pos[i], next.pos[i]));
        out.q = [q.x, q.y, q.z, q.w];
        out.speed = lerp(self.speed as f64, next.speed as f64) as f32;
        for (w, n) in out.wheels.iter_mut().zip(&next.wheels) {
            w[0] += (n[0] - w[0]) * t as f32;
            w[1] += (n[1] - w[1]) * t as f32;
        }
        out
    }
}

/// A lap, frame by frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lap {
    pub time: f64,
    pub frames: Vec<GhostFrame>,
}

impl Lap {
    /// The car as it was `t` seconds into the lap.
    pub fn at(&self, t: f64) -> Option<GhostFrame> {
        let s = t / EVERY;
        let i = s.floor().max(0.0) as usize;
        let a = self.frames.get(i)?;
        Some(match self.frames.get(i + 1) {
            Some(b) => a.blend(b, s - s.floor()),
            None => *a,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20 + self.frames.len() * 128);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.frames.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.time.to_le_bytes());
        for f in &self.frames {
            for v in f.pos.iter().chain(&f.q) {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for v in std::iter::once(&f.speed).chain(f.wheels.iter().flatten()).chain([&f.steer, &f.brake]) {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Lap> {
        if bytes.get(..8)? != MAGIC {
            return None;
        }
        let count = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
        let time = f64::from_le_bytes(bytes.get(12..20)?.try_into().ok()?);
        let size = 7 * 8 + 19 * 4;
        if bytes.len() != 20 + count * size || !time.is_finite() {
            return None;
        }
        let mut frames = Vec::with_capacity(count);
        for k in 0..count {
            let at = 20 + k * size;
            let d = |i: usize| f64::from_le_bytes(bytes[at + i * 8..at + i * 8 + 8].try_into().unwrap());
            let f = |i: usize| f32::from_le_bytes(bytes[at + 56 + i * 4..at + 56 + i * 4 + 4].try_into().unwrap());
            frames.push(GhostFrame {
                pos: [d(0), d(1), d(2)],
                q: [d(3), d(4), d(5), d(6)],
                speed: f(0),
                wheels: std::array::from_fn(|w| std::array::from_fn(|j| f(1 + w * 4 + j))),
                steer: f(17),
                brake: f(18),
            });
        }
        Some(Lap { time, frames })
    }
}

/// The game's settings folder.
pub fn settings_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".config"));
    Some(config.or(home)?.join("skylab-drive"))
}

/// Where the ghost of a circuit and a car is kept.
fn file(circuit: &str, car: &str) -> Option<PathBuf> {
    Some(settings_dir()?.join("ghosts").join(format!("{circuit}-{car}.ghost")))
}

/// The ghost: the best lap, the lap being recorded, and the car it is shown as.
pub struct Ghost {
    pub best: Option<Lap>,
    recording: Lap,
    /// The lap being recorded began at this world time; the lap number it is.
    began: Option<(f64, u32)>,
    circuit: String,
    car: String,
    /// The car the ghost is shown as (`Car::show`) and its look (when there is a model).
    pub shown: Car,
    /// Whether it is written to the settings folder (not in a screenshot).
    pub save: bool,
    /// Metres from the start line of each of the best lap's frames (worked out when first asked).
    alongs: Vec<f64>,
    pipeline: Option<wgpu::RenderPipeline>,
}

impl Ghost {
    /// The ghost kept for a circuit and a car, if any.
    pub fn load(circuit: &str, car: &Car) -> Ghost {
        let best = file(circuit, &car.p.key).and_then(|f| std::fs::read(f).ok()).and_then(|b| Lap::from_bytes(&b));
        Ghost { best, recording: Lap::default(), began: None, circuit: circuit.into(), car: car.p.key.clone(), shown: car.clone(), save: true, alongs: Vec::new(), pipeline: None }
    }

    /// After a step: the lap recorded as it goes; a lap done that beats the ghost becomes it.
    /// `lap`, `last`, `valid`: the timing's lap under way, the last lap's time and whether it counted.
    pub fn update(&mut self, world: &World, car: &Car, lap: u32, lap_time: Option<f64>, last: Option<f64>, last_valid: bool) {
        let time = world.time();
        match self.began {
            Some((_, k)) if k == lap => {}
            Some((_, k)) if lap == k + 1 => {
                // A lap done: the ghost if it is the best ever.
                if let Some(t) = last.filter(|_| last_valid) {
                    if self.best.as_ref().is_none_or(|b| t < b.time) && !self.recording.frames.is_empty() {
                        self.recording.time = t;
                        self.best = Some(std::mem::take(&mut self.recording));
                        self.alongs.clear();
                        self.write();
                    }
                }
                self.recording = Lap::default();
                self.began = lap_time.map(|t| (time - t, lap));
            }
            _ => {
                self.recording = Lap::default();
                self.began = lap_time.map(|t| (time - t, lap));
            }
        }
        if let Some((start, _)) = self.began {
            if lap > 0 && (time - start) >= self.recording.frames.len() as f64 * EVERY - 1e-9 {
                self.recording.frames.push(GhostFrame::of(car));
            }
        }
    }

    /// Writes the best lap to the settings folder.
    fn write(&self) {
        let (Some(best), true) = (&self.best, self.save) else { return };
        let Some(path) = file(&self.circuit, &self.car) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, best.to_bytes()) {
            eprintln!("{}: {e}", path.display());
        }
    }

    /// The ghost's car placed `t` seconds into its lap; whether it is there to be shown.
    pub fn place(&mut self, t: Option<f64>) -> bool {
        let Some(frame) = t.and_then(|t| self.best.as_ref()?.at(t)) else { return false };
        let look = frame.look(&self.shown.look());
        self.shown.show(&look);
        true
    }

    /// Seconds the player is behind the ghost (negative: ahead), at `along` on the circuit `t`
    /// seconds into the lap: when the ghost was where the player is.
    pub fn delta(&mut self, circuit: &Circuit, along_from_start: f64, t: f64) -> Option<f64> {
        let best = self.best.as_ref()?;
        if self.alongs.len() != best.frames.len() {
            self.alongs = best
                .frames
                .iter()
                .enumerate()
                .map(|(k, f)| {
                    let d = circuit.locate(f.pos[0], f.pos[1]).map_or(0.0, |at| circuit.from_start(at.along));
                    // Just after the line the distance may read as nearly a lap.
                    if k < 40 && d > circuit.length / 2.0 { d - circuit.length } else { d }
                })
                .collect();
        }
        let k = self.alongs.partition_point(|&d| d < along_from_start);
        if k == 0 || k >= self.alongs.len() {
            return None;
        }
        let (d0, d1) = (self.alongs[k - 1], self.alongs[k]);
        let share = if d1 > d0 { (along_from_start - d0) / (d1 - d0) } else { 0.0 };
        Some(t - (k as f64 - 1.0 + share) * EVERY)
    }

    /// The pipeline that draws it, from the game's shader module (with `ghost.wgsl`).
    pub fn pipeline(&mut self, gpu: &Gpu, world: &WorldRenderer, shader: &wgpu::ShaderModule) {
        let buffers = [MeshBuilder::layout()];
        let groups = [world.frame_layout(), world.object_layout()];
        let desc = PipelineDesc { label: "ghost", shader, vertex: "car_vertex", fragment: "car_ghost_fragment", buffers: &buffers, bind_groups: &groups, depth: Depth::Read, blend: false };
        self.pipeline = Some(gpu.pipeline_blended(&desc, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)));
    }

    pub fn drawing(&self) -> Option<&wgpu::RenderPipeline> {
        self.pipeline.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lap_reads_back_as_it_was_written() {
        let frame = |k: f64| GhostFrame { pos: [k, 2.0 * k, -3.0], q: [0.0, 0.0, 0.0, 1.0], speed: 40.0, wheels: [[0.01, 0.02, k as f32, 100.0]; 4], steer: 0.1, brake: 0.0 };
        let lap = Lap { time: 117.25, frames: (0..50).map(|k| frame(k as f64)).collect() };
        let back = Lap::from_bytes(&lap.to_bytes()).expect("reads");
        assert_eq!(back, lap);
        assert!(Lap::from_bytes(&lap.to_bytes()[..100]).is_none());
        // Between two frames, halfway.
        let mid = lap.at(10.5 * EVERY).unwrap();
        assert!((mid.pos[0] - 10.5).abs() < 1e-9);
    }
}
