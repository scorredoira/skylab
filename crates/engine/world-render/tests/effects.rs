//! The effects of a crash on a real device: a wreck half a second after it came apart, seen from
//! 70 m, drawn with the fire's simulation and four samples per pixel (`high`) and with particles
//! alone and one sample (`low`): the second pass and its pipelines build for both kinds of depth,
//! and the fireball is in the picture. The images are written to the test's temporary directory.
//! And the frame a first crash shows in makes no shader or pipeline: they are made before.

use engine::{wgpu, Game, Gpu, Input};
use sim_core::{quaternion, AircraftType, PieceKind, Terrain, Weather, Wreck, DEG};
use glam::DVec3;
use std::sync::Arc;
use std::time::{Duration, Instant};
use world_render::{buildings, to_render, Quality, Scene, ViewPoint, WorldRenderer};

struct Crash {
    world: WorldRenderer,
    wrecks: Vec<Wreck>,
    eye: DVec3,
    /// What the eye looks at: a little over the crash.
    target: DVec3,
    time: f64,
    /// How long the last `update` took (the renderer's `prepare`), and the shaders, render and
    /// compute pipelines the device had made by its end.
    update: Duration,
    counted: Option<[isize; 3]>,
}

impl Game for Crash {
    fn samples(&self) -> u32 {
        self.world.quality().samples
    }

    fn update(&mut self, _input: &Input, _dt: f64, gpu: &Gpu) {
        let started = Instant::now();
        let forward = (self.target - self.eye).normalize();
        let right = DVec3::Z.cross(forward).normalize();
        let view = ViewPoint { position: self.eye, forward: to_render(forward), up: to_render(right.cross(forward)), right: to_render(right), fov_y: 60f32.to_radians(), shift_y: 0.0, near: 0.25 };
        let scene = Scene { view, hour: 11.0, time: self.time, weather: &Weather::default(), height_above_ground: 10.0, body: None, casters: &[], objects: &[], glows: &[], lamps: &[], see_through: &[], wrecks: &self.wrecks };
        self.world.prepare(gpu, &scene);
        self.update = started.elapsed();
        let hal = gpu.device.get_internal_counters().hal;
        self.counted = Some([hal.shader_modules.read(), hal.render_pipelines.read(), hal.compute_pipelines.read()]);
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        self.world.draw_sky(pass);
        self.world.draw(pass);
    }

    fn translucent(&self) -> bool {
        self.world.translucent()
    }

    fn before_translucent(&mut self, _gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        self.world.before_translucent(encoder);
    }

    fn draw_translucent(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        self.world.draw_translucent(pass);
    }
}

/// A light aircraft that went into the ground at 45 m/s with 100 kg of fuel, half a second ago.
fn crash(terrain: &Terrain) -> (Vec<Wreck>, DVec3, f64) {
    let (n, e) = (2000.0, 12000.0);
    let ground = terrain.height(n, e).max(0.0);
    let mut wreck = Wreck::new(AircraftType::c172().pieces, &PieceKind::ALL, 1100.0, DVec3::new(n, e, -(ground + 1.0)), quaternion(0.0, -35.0 * DEG, 0.0), DVec3::new(37.0, 0.0, 26.0), DVec3::ZERO, 0.5, 7, 100.0);
    wreck.spill(100.0, false);
    let mut t = 100.0;
    while t < 100.5 {
        wreck.step(1.0 / 240.0, t, |n, e| (terrain.height(n, e).max(0.0), false));
        t += 1.0 / 240.0;
    }
    (vec![wreck], DVec3::new(n - 70.0, e, -(ground + 12.0)), t)
}

fn draw(quality: Quality, name: &str) -> (Vec<u8>, bool) {
    let terrain = Arc::new(Terrain::new(1));
    let (wrecks, eye, time) = crash(&terrain);
    let path = format!("{}/{name}.png", env!("CARGO_TARGET_TMPDIR"));
    let mut translucent = false;
    engine::render_to_png(
        &path,
        320,
        200,
        1.0,
        |gpu| {
            let mut world = WorldRenderer::new(gpu, terrain.clone(), buildings::stand_in());
            world.set_quality(gpu, quality);
            let target = wrecks[0].impact + DVec3::new(0.0, 0.0, -8.0);
            Crash { world, wrecks, eye, target, time, update: Duration::ZERO, counted: None }
        },
        |crash, frames| {
            translucent |= crash.world.translucent();
            // Once the ground round it is drawn: the fire is told from it, not from the haze.
            frames >= 3 && (crash.world.pending() == 0 || frames > 600)
        },
    );
    println!("image written to {path}");
    let image = image::open(&path).expect("the picture").to_rgb8();
    (image.into_raw(), translucent)
}

/// Pixels that glow orange: fire.
fn fire(pixels: &[u8]) -> usize {
    pixels.chunks(3).filter(|p| p[0] > 150 && p[0] as u32 > p[2] as u32 + 60).count()
}

#[test]
fn a_crash_burns_in_the_picture_with_and_without_the_simulation() {
    for (quality, name) in [(Quality::HIGH, "effects_high"), (Quality::LOW, "effects_low")] {
        let (pixels, translucent) = draw(quality, name);
        println!("{name}: {} pixels of fire", fire(&pixels));
        assert!(translucent, "{name}: no second pass");
        assert!(fire(&pixels) > 200, "{name}: no fireball");
    }
}

/// The first crash of a world shows in the next frame without that frame making a shader or a
/// pipeline: the renderer made them in the background, before `pending` came to 0. Made in that
/// frame, the driver's compiling of the effects' shaders froze it for 1.1 s on an Apple M5 Pro
/// the first time a release build ran.
#[test]
fn the_first_crash_makes_no_pipeline() {
    let terrain = Arc::new(Terrain::new(1));
    let (wrecks, eye, time) = crash(&terrain);
    let target = wrecks[0].impact + DVec3::new(0.0, 0.0, -8.0);
    let path = format!("{}/first_crash.png", env!("CARGO_TARGET_TMPDIR"));
    // Shaders and pipelines the device had made when the wreck was given, and once the frame it
    // showed in was prepared, with how long that took.
    let mut before = None;
    let mut after = None;
    engine::render_to_png(
        &path,
        320,
        200,
        1.0,
        |gpu| {
            let mut world = WorldRenderer::new(gpu, terrain.clone(), buildings::stand_in());
            world.set_quality(gpu, Quality::HIGH);
            Crash { world, wrecks: Vec::new(), eye, target, time, update: Duration::ZERO, counted: None }
        },
        |crash, frames| {
            if crash.wrecks.is_empty() {
                assert!(frames < 3000, "the world never finished loading");
                if crash.world.pending() == 0 && frames >= 5 {
                    crash.wrecks = wrecks.clone();
                    before = crash.counted;
                } else {
                    std::thread::sleep(Duration::from_millis(20));
                }
                return false;
            }
            after = crash.counted.map(|c| (c, crash.update));
            true
        },
    );
    let (before, (after, update)) = (before.expect("counted"), after.expect("the crash was shown"));
    println!("the frame of the first crash prepared in {:.1} ms", update.as_secs_f64() * 1000.0);
    assert_eq!(before, after, "shaders, render and compute pipelines made in the frame of the first crash");
}
