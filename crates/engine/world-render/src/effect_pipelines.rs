//! What a crash's effects need on the GPU that takes long to make: the pipelines of the fire's
//! simulation (`fire.rs`) and of the second pass (`particles.rs`, the drawing of the grid), the
//! 3D noise they are carved with. The shaders are big and the driver compiles them when a pipeline
//! is made: made at the first crash, they froze that frame for 1.1 s on an Apple M5 Pro the first
//! time a build ran (the driver keeps them on disk: 20 ms more than the frames round it after).
//! So they are made on background threads: the simulation and the noise when the renderer starts,
//! the drawing for each quality and number of samples as soon as the frame has them, long before
//! anything can crash. A frame that needs them before they are made waits for them.

use crate::fire::{self, Simulation};
use crate::particles;
use crate::Quality;
use engine::{wgpu, Gpu};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// What the pipelines of the second pass draw into.
#[derive(Clone, Copy, PartialEq)]
pub struct Target {
    pub scene: wgpu::TextureFormat,
    pub depth: wgpu::TextureFormat,
    pub samples: u32,
}

impl Target {
    pub fn of(gpu: &Gpu) -> Target {
        Target { scene: gpu.scene_format, depth: gpu.depth_format, samples: gpu.sample_count }
    }
}

/// What does not depend on the quality: the simulation's pipelines and the noise.
struct Common {
    simulation: Arc<Simulation>,
    noise: wgpu::TextureView,
}

/// The pipelines of the second pass for one quality and target.
pub struct Drawing {
    pub particles: particles::Pipeline,
    /// The march of the fire's grid and its putting over the scene, if the quality simulates the fire.
    pub fire: Option<fire::DrawPipelines>,
}

/// Something being made on another thread, or made.
pub(crate) enum Slot<T> {
    // Never on the web (`Slot::start`).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Making(Receiver<T>),
    Made(T),
}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + 'static> Slot<T> {
    pub(crate) fn start(work: impl FnOnce() -> T + Send + 'static) -> Slot<T> {
        let (sender, receiver) = std::sync::mpsc::channel();
        engine::threads::spawn(move || {
            // Nobody waits any more when the quality changed meanwhile or the renderer was dropped.
            let _ = sender.send(work());
        });
        Slot::Making(receiver)
    }
}

impl<T> Slot<T> {
    /// Web: made at once. wgpu's objects stay on the thread that made them there, and the browser
    /// compiles a pipeline's shaders in the background on its own.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn start(work: impl FnOnce() -> T) -> Slot<T> {
        Slot::Made(work())
    }

    /// The thing if it is made, without waiting.
    pub(crate) fn ready(&mut self) -> Option<&mut T> {
        if let Slot::Making(receiver) = self {
            *self = Slot::Made(receiver.try_recv().ok()?);
        }
        match self {
            Slot::Made(made) => Some(made),
            Slot::Making(_) => None,
        }
    }

    /// The thing, if it is made (`ready` said so).
    pub(crate) fn take(self) -> Option<T> {
        match self {
            Slot::Made(made) => Some(made),
            Slot::Making(receiver) => receiver.try_recv().ok(),
        }
    }

    /// The thing, waiting for it if it is not made yet.
    fn get(&mut self) -> &mut T {
        if let Slot::Making(receiver) = self {
            *self = Slot::Made(receiver.recv().expect("the thread making a crash's pipelines died"));
        }
        match self {
            Slot::Made(made) => made,
            Slot::Making(_) => unreachable!(),
        }
    }
}

pub struct EffectPipelines {
    device: wgpu::Device,
    frame_layout: wgpu::BindGroupLayout,
    common: Slot<Common>,
    drawing: Option<Slot<Arc<Drawing>>>,
    /// The quality and target of `drawing`.
    drawing_for: Option<(Quality, Target)>,
    /// Whether `common` and `drawing` were made, as of the last `prepare`.
    made: bool,
}

impl EffectPipelines {
    /// Starts making the simulation and the noise. `frame_layout` is group 0 of the second pass.
    pub fn new(gpu: &Gpu, frame_layout: &wgpu::BindGroupLayout) -> EffectPipelines {
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        let common = Slot::start(move || Common { simulation: Arc::new(Simulation::new(&device)), noise: particles::make_noise(&device, &queue) });
        EffectPipelines { device: gpu.device.clone(), frame_layout: frame_layout.clone(), common, drawing: None, drawing_for: None, made: false }
    }

    /// Starts making the drawing for `quality` and what the frame draws into, if it is not made or
    /// being made already. Called every frame; it costs nothing when nothing changed.
    pub fn prepare(&mut self, gpu: &Gpu, quality: &Quality) {
        let key = (*quality, Target::of(gpu));
        if self.drawing_for != Some(key) {
            let (device, frame_layout) = (self.device.clone(), self.frame_layout.clone());
            let (quality, target) = key;
            self.drawing = Some(Slot::start(move || {
                Arc::new(Drawing {
                    particles: particles::Pipeline::new(&device, target, &frame_layout, &quality),
                    fire: quality.fire_simulation.then(|| fire::DrawPipelines::new(&device, target, &frame_layout, &quality)),
                })
            }));
            self.drawing_for = Some(key);
        }
        self.made = self.common.ready().is_some() && self.drawing.as_mut().is_some_and(|d| d.ready().is_some());
    }

    /// Whether something is still being made, as of the last `prepare`: 0 when a crash would find it all.
    pub fn pending(&self) -> usize {
        usize::from(!self.made)
    }

    /// The drawing for `quality` and the frame's target, made now if `prepare` was not called for them.
    pub fn drawing(&mut self, gpu: &Gpu, quality: &Quality) -> Arc<Drawing> {
        self.prepare(gpu, quality);
        self.drawing.as_mut().expect("asked for above").get().clone()
    }

    /// The fire's simulation and the noise.
    pub fn simulation(&mut self) -> (&Arc<Simulation>, &wgpu::TextureView) {
        let common = self.common.get();
        (&common.simulation, &common.noise)
    }

    pub fn noise(&mut self) -> &wgpu::TextureView {
        &self.common.get().noise
    }
}
