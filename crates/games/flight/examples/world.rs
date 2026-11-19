//! Renders the world to a PNG without a window, to compare it with the Swift version's `--shot`.
//! `cargo run --release -p flight --example world -- <out.png> [ground|air|ext|final] [hour=<h>] [pos=<n>,<e>] [size=<w>x<h>] [planes=<count>] [quality=<ultra|high|medium|low>]`
//!
//! - `ground`: parked on the home runway, seen from the cockpit (the default).
//! - `air`: in flight over the home airport, from the cockpit.
//! - `ext`: in flight, from the camera that follows the aircraft from outside. With `ground` or
//!   `final` as well, that situation from outside.
//! - `final`: on final approach to the home runway, from the cockpit.
//! - `hour=18.2` sets the time of day; `pos=12000,30000` starts in flight over that point (north, east).
//! - `at=<n>,<e>,<agl>` then puts the aircraft at that point, metres north and east, metres above the ground.
//! - `planes=5` draws copies of the aircraft in formation behind it.
//! - `cam=<n>,<e>,<agl>,<heading>,<pitch>` looks from a free camera instead: metres north and east, metres above
//!   the ground, degrees from north and degrees above the horizon. The aircraft stays where the other options put it.
//! - `orbit=<metres>,<bearing>,<elevation>[,<x>,<y>,<z>]` looks at the aircraft (at that point in its body axes,
//!   0.3 m behind its centre of gravity without it) from that far, from that many degrees right of its nose and above it (a close-up); `fov=<degrees>` the camera's vertical field of view.
//! - `quality=low` draws what that quality setting draws (the default is `ultra`, everything).
//! - `textures=off` draws the ground and the buildings without their photographs, as they were before them.
//!
//! The world alone, without an aircraft, and `places` (where the home airport, towns, river and coast
//! are): `cargo run --release -p world-render --example viewer`.
//!
//! Once everything is loaded it prints the average time of 40 frames.

#[path = "../src/models.rs"]
mod models;
// Where the aircraft's files are.
#[allow(dead_code)]
#[path = "../src/fleet.rs"]
mod fleet;
// The game paints its instruments and takes its levers; this example draws the world only.
#[allow(dead_code)]
#[path = "../src/render/mod.rs"]
mod render;
// The cockpit the renderer draws from the seat, and the dials its displays share with the strip.
#[allow(dead_code)]
#[path = "../src/cockpit/mod.rs"]
mod cockpit;
#[allow(dead_code)]
#[path = "../src/g1000/mod.rs"]
mod g1000;
// Each kind's look.
#[allow(dead_code)]
#[path = "../src/kinds.rs"]
mod kinds;
#[allow(dead_code)]
#[path = "../src/ui/panel.rs"]
mod panel;
mod ui {
    pub(crate) use super::panel;
}

use engine::{wgpu, Game, Gpu, Input};
use sim_core::{act, euler_angles, normalize, AircraftKind, Start, Vehicle, VehicleId, World, DEG};
use glam::DVec3;
use render::{to_render, PlaneVisual, Preset, Renderer, Scene, ViewPoint};
use std::time::{Duration, Instant};

struct Shot {
    world: World,
    renderer: Renderer,
    exterior: bool,
    /// A camera on its own: position (north, east, down) and heading and pitch in radians.
    free: Option<(DVec3, f64, f64)>,
    fov_y: f32,
    /// Copies of the aircraft drawn in formation behind it, to see several at once.
    wingmen: usize,
    /// Time spent in `WorldRenderer::prepare` and in `draw` since they were last reset, to report what they cost.
    preparing: Duration,
    drawing: Duration,
}

impl Shot {
    /// The camera of the game: the pilot's eyes, or 17 m behind and above the aircraft.
    fn view(&self) -> ViewPoint {
        let s = &self.world.vehicles[0].aircraft().unwrap().state;
        let fov_y = self.fov_y;
        if let Some((position, heading, pitch)) = self.free {
            let fwd = DVec3::new(heading.cos() * pitch.cos(), heading.sin() * pitch.cos(), -pitch.sin());
            let right = normalize(DVec3::Z.cross(fwd));
            return ViewPoint { position, forward: to_render(fwd), up: to_render(right.cross(fwd)), right: to_render(right), fov_y, shift_y: 0.0, near: 0.25 };
        }
        if !self.exterior {
            let axis = |v: DVec3| to_render(act(s.q, v));
            // The nose points at a spot above the panel, not at the middle of the window.
            return ViewPoint { position: s.pos + act(s.q, DVec3::new(0.10, -0.27, -0.50)), forward: axis(DVec3::X), up: axis(-DVec3::Z), right: axis(DVec3::Y), fov_y, shift_y: 0.34, near: 0.05 };
        }
        let (h, pitch) = (euler_angles(s.q).heading, 0.18f64);
        let dir = DVec3::new(h.cos() * pitch.cos(), h.sin() * pitch.cos(), pitch.sin());
        let mut pos = s.pos - dir * 17.0;
        let floor = self.world.terrain.height(pos.x, pos.y).max(0.0) + 1.5;
        if -pos.z < floor {
            pos.z = -floor;
        }
        let fwd = normalize(s.pos + DVec3::new(0.0, 0.0, -0.6) - pos);
        let right = normalize(DVec3::Z.cross(fwd));
        ViewPoint { position: pos, forward: to_render(fwd), up: to_render(right.cross(fwd)), right: to_render(right), fov_y, shift_y: 0.0, near: 0.25 }
    }
}

impl Game for Shot {
    fn samples(&self) -> u32 {
        self.renderer.samples()
    }

    fn update(&mut self, _input: &Input, _dt: f64, gpu: &Gpu) {
        // The background threads need time while tiles are missing.
        if self.renderer.pending() > 0 {
            std::thread::sleep(Duration::from_millis(20));
        }
        let plane = &self.world.vehicles[0];
        let lead = PlaneVisual::of(plane);
        let planes: Vec<PlaneVisual> = (0..=self.wingmen)
            .map(|k| {
                let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                let offset = DVec3::new(-7.0, 12.0 * side, 0.0) * k.div_ceil(2) as f64;
                PlaneVisual { position: lead.position + act(lead.attitude, offset), ..lead }
            })
            .collect();
        let scene = Scene {
            view: self.view(),
            planes: &planes,
            player: 0,
            inside_player: !self.exterior && self.free.is_none(),
            propeller_inside: true,
            player_agl: plane.agl(),
            hour: self.world.hour,
            time: self.world.time(),
            weather: &self.world.weather,
            wrecks: &self.world.wrecks,
            cockpit: Default::default(),
            dt: 0.016,
            course: None,
        };
        let start = Instant::now();
        self.renderer.prepare(gpu, &scene);
        self.preparing += start.elapsed();
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        let start = Instant::now();
        self.renderer.draw(pass);
        self.drawing += start.elapsed();
    }

    fn translucent(&self) -> bool {
        self.renderer.translucent()
    }

    fn before_translucent(&mut self, _gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        self.renderer.before_translucent(encoder);
    }

    fn draw_translucent(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        self.renderer.draw_translucent(pass);
    }

    fn post(&self) -> engine::Post {
        self.renderer.post()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first().cloned() else {
        eprintln!("usage: world <out.png> [ground|air|ext|final] [hour=<h>] [pos=<n>,<e>] [size=<w>x<h>] [planes=<count>]");
        std::process::exit(2);
    };
    let has = |word: &str| args[1..].iter().any(|a| a == word);
    let value = |name: &str| args[1..].iter().find_map(|a| a.strip_prefix(name)?.strip_prefix('='));
    let pair = |text: &str, separator: char| -> Option<(f64, f64)> {
        let (a, b) = text.split_once(separator)?;
        Some((a.parse().ok()?, b.parse().ok()?))
    };
    let exterior = has("ext");
    let wingmen = value("planes").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).saturating_sub(1);
    let mut quality = value("quality").and_then(Preset::named).unwrap_or(Preset::ALL[0]).quality();
    if value("textures") == Some("off") {
        quality.texture_size = 0;
    }
    let (width, height) = value("size").and_then(|v| pair(v, 'x')).unwrap_or((1600.0, 1000.0));

    let mut world = World::new(1);
    if let Some(hour) = value("hour").and_then(|v| v.parse().ok()) {
        world.hour = hour;
    }
    let id: VehicleId = world.add_aircraft(AircraftKind::C172, Start::Home);
    let s = world.vehicle(id).aircraft().unwrap().state;
    let into_wind = |world: &World| {
        let a = world.terrain.nearest_airport(s.pos.x, s.pos.y).or(world.vehicle(id).home)?;
        Some((a, ((world.weather.wind_from * DEG) - a.heading).cos() < 0.0))
    };
    if let Some((n, e)) = value("pos").and_then(|v| pair(v, ',')) {
        world.reset(id, Start::Air { n, e, heading: 0.6 });
    } else if has("final") {
        if let Some((airport, reversed)) = into_wind(&world) {
            world.reset(id, Start::Final { airport, reversed, distance: Vehicle::FINAL_DISTANCE });
        }
    } else if has("air") || (exterior && !has("ground")) {
        world.reset(id, Start::Air { n: s.pos.x, e: s.pos.y, heading: euler_angles(s.q).heading });
    }
    for _ in 0..480 {
        world.step();
    }
    if let Some(p) = value("at").map(|v| v.split(',').filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<f64>>()).filter(|p| p.len() == 3) {
        let ground = world.terrain.height(p[0], p[1]).max(0.0);
        world.vehicle_mut(id).aircraft_mut().unwrap().state.pos = DVec3::new(p[0], p[1], -(ground + p[2]));
    }
    let free = value("cam").and_then(|v| {
        let p: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        (p.len() == 5).then(|| (DVec3::new(p[0], p[1], -(world.terrain.height(p[0], p[1]).max(0.0) + p[2])), p[3] * DEG, p[4] * DEG))
    });
    let orbit = value("orbit").and_then(|v| {
        let p: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        let s = world.vehicle(id).aircraft().unwrap().state;
        let (bearing, up) = (euler_angles(s.q).heading + p.get(1)? * DEG, p.get(2)? * DEG);
        let at = s.pos + act(s.q, DVec3::new(p.get(3).copied().unwrap_or(-0.3), p.get(4).copied().unwrap_or(0.0), p.get(5).copied().unwrap_or(0.0)));
        let eye = at + DVec3::new(bearing.cos() * up.cos(), bearing.sin() * up.cos(), -up.sin()) * p[0];
        Some((eye, bearing + std::f64::consts::PI, -up))
    });
    let free = orbit.or(free);
    let fov_y = value("fov").and_then(|v| v.parse::<f32>().ok()).unwrap_or(62.0).to_radians();

    // Frames are drawn until nothing is pending, then one more, then 40 to time them (as the Swift `--shot` does,
    // so that the propellers end at the same angle).
    let mut loaded = false;
    let mut timing: Option<(usize, Instant)> = None;
    let loading = Instant::now();
    let shot = engine::render_to_png(
        &path,
        width as u32,
        height as u32,
        1.0,
        |gpu| {
            let mut renderer = Renderer::new(gpu, world.terrain.clone());
            renderer.set_quality(gpu, quality);
            Shot { renderer, world, exterior, free, fov_y, wingmen, preparing: Duration::ZERO, drawing: Duration::ZERO }
        },
        |game, frames| match timing {
            None if loaded => {
                (game.preparing, game.drawing) = (Duration::ZERO, Duration::ZERO);
                timing = Some((frames, Instant::now()));
                false
            }
            None => {
                if (game.renderer.pending() == 0 && frames > 5) || frames > 3000 {
                    println!("world ready in {:.1} s ({frames} frames, {} tiles pending)", loading.elapsed().as_secs_f64(), game.renderer.pending());
                    loaded = true;
                }
                false
            }
            Some((start, since)) => {
                let done = frames - start >= 40;
                if done {
                    println!(
                        "render: {:.1} ms per frame at {width}x{height}, of which {:.2} ms in prepare and {:.2} ms in draw; {} draw calls",
                        since.elapsed().as_secs_f64() / 40.0 * 1000.0,
                        game.preparing.as_secs_f64() / 40.0 * 1000.0,
                        game.drawing.as_secs_f64() / 40.0 * 1000.0,
                        game.renderer.draw_calls()
                    );
                }
                done
            }
        },
    );
    let s = &shot.world.vehicles[0].aircraft().unwrap().state;
    println!("position n {:.0} e {:.0} alt {:.0} m, agl {:.1} m", s.pos.x, s.pos.y, -s.pos.z, shot.world.vehicles[0].agl());
}
