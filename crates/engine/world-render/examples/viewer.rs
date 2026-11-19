//! The world alone, with a free camera: no vehicle, nothing simulated but the clock and, if asked
//! for (`crash=`), a crash.
//! `cargo run --release -p world-render --example viewer -- [--shot <dir>] [options]`
//!
//! In a window (1440×900): W S forward and back, A D sideways, R F up and down, ← → turn, ↑ ↓ look
//! up and down, Shift ten times faster; the mouse dragged looks around. T and Shift+T move the hour
//! (held, an hour every second), 1 2 3 the quality (high, medium, low), C and Shift+C the cloud
//! cover, P prints the options that take this picture again. Esc closes it.
//!
//! With `--shot <dir>` it writes `<dir>/scene.png` (1600×1000, or `size=`) once everything is
//! loaded, and prints the milliseconds of a frame as the game's `--shot` does: the fastest, quartile
//! and median of 240 frames drawn after 60 of warm-up.
//!
//! Options, all `name=value`:
//! - `eye=<n>,<e>,<agl>`: where the eye is, metres north and east and metres above the ground
//!   (default: 2 km short of the home aerodrome's runway, 150 m up);
//! - `look=<heading>,<pitch>`: degrees from north and above the horizon (default: along the runway, 4° down);
//! - `hour=<h>` (default 10.5); `seed=<n>`, the world (default 1);
//! - weather: `visibility=<km>` (150, a clear day), `humidity=<%>` (0, the standard atmosphere's dry air),
//!   `cover=<%>` of cloud (63), `base=<m>` of the clouds above sea level (2300), `storms=<n>` storm clouds about (0), `thickness=<m>` of
//!   their layer (650), `wind=<from>,<kt>` (250, 0);
//! - `quality=<ultra|high|medium|low>`, `textures=off` (the ground without its photographs), `size=<w>x<h>`;
//! - `crash=<s>[,<m>]`: a light aircraft flown into the ground `m` metres ahead of the eye (120), with
//!   full tanks, at 90 kt and 35° down, shown `s` seconds after it came apart (its fire, smoke,
//!   fragments and marks; the pieces are the game's to draw). In the window the time then runs on.
//! - `aerodrome=<visual|nonprecision|cat1|cat2>`: instead of the home aerodrome, the one of that
//!   class nearest the origin (`sim_core::lights::Class`), where the eye starts by default;
//! - `final=<m>`: the eye on the 3° path of that aerodrome's main approach, `m` metres before its
//!   aiming point, 2 m over the wheels, looking at the aiming point (`eye` and `look` aside);
//! - `place=<ICAO>`: a real place round that aerodrome instead of the generated world, the eye short
//!   of its main runway; `at=<lat>,<lon>`: round that point, the eye over it. The region is downloaded
//!   the first time and kept (`geodata`); `offline=1` reads only what is kept. Its ground is coloured by
//!   the satellites' picture of it (Sentinel-2, `geodata::imagery`, made the first time too);
//!   `imagery=off` leaves the ground its own colours.
//! - `cruise=<m/s>,<s>`: the eye flown straight ahead at that speed (a sixtieth of a second a frame)
//!   for that many seconds of frames; with `--shot`, the frames are timed, and over a real place the
//!   regions further along are built and swapped in under it as in the game (`geodata::follow`):
//!   the worst frame of the second after each swap is said.
//!
//! `places` as the only argument prints the home aerodrome and the towns, lakes, river and coast nearest
//! to it, to point the eye at.

use engine::egui::{self, pos2, Align2, Color32, FontId};
use engine::{wgpu, Game, Gpu, Input, KeyCode};
use sim_core::lights::{self, Class};
use sim_core::{normalize, quaternion, AircraftKind, Airport, Atmosphere, Imagery, Region, Start, Terrain, Weather, World, DEG, KNOTS, V3};
use glam::DVec3;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use world_render::{buildings, to_render, Preset, Quality, Scene, ViewPoint, WorldRenderer};

/// The frames a picture waits for the world before giving up, and those drawn before and while timing.
const FRAME_CAP: usize = 3000;
const WARM_UP: usize = 60;
const TIMED: usize = 240;

/// What the command line says.
struct Options {
    seed: u64,
    eye: Option<(f64, f64, f64)>,
    look: Option<(f64, f64)>,
    hour: f64,
    cover: f64,
    base: f64,
    thickness: f64,
    /// Storm clouds about, 0 to 3.
    storms: u32,
    weather: Weather,
    quality: Quality,
    size: (u32, u32),
    crash: Option<(f64, f64)>,
    /// Metres before the aiming point of the main approach of `aerodrome`, on its 3° path.
    on_final: Option<f64>,
    /// The aerodrome nearest the origin of this class (`sim_core::lights::Class`); the home one if none.
    aerodrome: Option<Class>,
    /// A real place given as a point (`at=`): the eye starts over it, not at an aerodrome.
    over_point: bool,
    /// A real place's ground as the satellites saw it.
    imagery: Option<Arc<Imagery>>,
    /// The eye flown straight ahead at this speed, m/s, for this many seconds of frames (`cruise=`).
    cruise: Option<(f64, f64)>,
    /// The real place flown, to follow it beyond its region.
    place: Option<geodata::Place>,
    offline: bool,
}

/// A real place followed beyond its region as the eye flies on, as the game does
/// (`geodata::follow`): the next region built on a thread, swapped in under the eye with
/// `WorldRenderer::replace_terrain`, and its ground's picture after it.
struct Following {
    place: geodata::Place,
    offline: bool,
    follow: geodata::follow::Follow,
    building: Option<Receiver<Option<Arc<Terrain>>>>,
    photos: Option<Receiver<Option<Arc<Imagery>>>>,
}

struct Viewer {
    world: WorldRenderer,
    terrain: Arc<Terrain>,
    weather: Weather,
    /// The eye in world axes (north, east, down), and where it looks: heading and pitch, radians.
    eye: DVec3,
    heading: f64,
    pitch: f64,
    hour: f64,
    /// Seconds since the start: what moves by itself (clouds, sea, fire) goes by it.
    time: f64,
    quality: Quality,
    /// Seconds per frame, smoothed, for the overlay; no overlay in a picture (`--shot`).
    frame: f64,
    overlay: bool,
    /// The simulation of a crash (`crash=`), and the seconds of its time the clock was at when it was made.
    crash: Option<World>,
    /// Flown straight ahead at this speed, m/s (`cruise=`).
    cruise: Option<f64>,
    following: Option<Following>,
    /// Frames drawn, and those at which another region was swapped in.
    frames: usize,
    swaps: Vec<usize>,
}

impl Viewer {
    fn new(gpu: &Gpu, terrain: Arc<Terrain>, options: &Options) -> Viewer {
        let mut world = WorldRenderer::new(gpu, terrain.clone(), buildings::stand_in());
        world.cloud_cover = options.cover;
        world.cloud_altitude = options.base;
        world.cloud_thickness = options.thickness;
        world.cloud_storms = options.storms;
        world.set_quality(gpu, options.quality);
        world.set_imagery(gpu, options.imagery.clone());
        // By default, short of the home aerodrome's runway, looking along it.
        let home = match options.aerodrome {
            Some(class) => of_class(&terrain, class),
            None => terrain.nearest_airport(0.0, 0.0),
        }
        .filter(|_| !options.over_point);
        let (n, e, agl) = options.eye.unwrap_or_else(|| home.map_or((0.0, 0.0, 150.0), |a| (a.n - a.heading.cos() * 2000.0, a.e - a.heading.sin() * 2000.0, 150.0)));
        let (heading, pitch) = options.look.map_or_else(|| (home.map_or(0.0, |a| a.heading), -4.0 * DEG), |(h, p)| (h * DEG, p * DEG));
        let ground = terrain.height(n, e).max(0.0);
        let crash = options.crash.map(|(seconds, distance)| crash(options.seed, &terrain, n, e, heading, distance, seconds));
        let mut eye = DVec3::new(n, e, -(ground + agl));
        let (mut heading, mut pitch) = (heading, pitch);
        if let (Some(out), Some(a)) = (options.on_final, home) {
            (eye, heading, pitch) = on_final(&a, out);
        }
        let following = options.place.clone().map(|place| Following { place, offline: options.offline, follow: geodata::follow::Follow::new((0.0, 0.0)), building: None, photos: None });
        Viewer {
            world,
            terrain,
            weather: options.weather,
            eye,
            heading,
            pitch,
            hour: options.hour,
            time: 0.0,
            quality: options.quality,
            frame: 0.0,
            overlay: true,
            crash,
            cruise: options.cruise.map(|c| c.0),
            following,
            frames: 0,
            swaps: Vec::new(),
        }
    }

    /// Builds the region further along when the eye needs it, flies on it once it has come, and
    /// draws its ground's picture once that has.
    fn follow_place(&mut self, gpu: &Gpu) {
        let Some(f) = &mut self.following else { return };
        let (n, e) = (self.eye.x, self.eye.y);
        let speed = self.cruise.unwrap_or(0.0);
        if let Some(centre) = f.follow.next(n, e, speed * self.heading.cos(), speed * self.heading.sin()) {
            let (sender, done) = channel();
            let (place, offline) = (f.place.clone(), f.offline);
            std::thread::spawn(move || {
                let built = geodata::load_around(&place, centre, offline, &|_| {});
                let _ = sender.send(built.ok().map(|l| Arc::new(Terrain::from_region(1, Arc::new(l.region)))));
            });
            f.building = Some(done);
        }
        if let Some(built) = f.building.as_ref().and_then(|r| r.try_recv().ok()) {
            f.building = None;
            match built {
                Some(terrain) if f.follow.arrived(n, e) => {
                    self.world.replace_terrain(terrain.clone());
                    let old = std::mem::replace(&mut self.terrain, terrain.clone());
                    std::thread::spawn(move || drop(old));
                    self.swaps.push(self.frames);
                    let (sender, done) = channel();
                    let offline = f.offline;
                    std::thread::spawn(move || {
                        let picture = terrain.region().and_then(|r| geodata::imagery::load(r, offline, &|_| {}).ok());
                        let _ = sender.send(picture.map(|l| Arc::new(l.imagery)));
                    });
                    f.photos = Some(done);
                }
                Some(_) => {}
                None => f.follow.failed(),
            }
        }
        if let Some(picture) = f.photos.as_ref().and_then(|r| r.try_recv().ok()) {
            f.photos = None;
            if picture.is_some() {
                self.world.set_imagery(gpu, picture);
            }
        }
    }

    fn ground(&self) -> f64 {
        self.terrain.height(self.eye.x, self.eye.y).max(0.0)
    }

    fn view(&self) -> ViewPoint {
        let (h, p) = (self.heading, self.pitch);
        let forward = DVec3::new(h.cos() * p.cos(), h.sin() * p.cos(), -p.sin());
        let right = normalize(DVec3::Z.cross(forward));
        ViewPoint { position: self.eye, forward: to_render(forward), up: to_render(right.cross(forward)), right: to_render(right), fov_y: 62f32.to_radians(), shift_y: 0.0, near: 0.25 }
    }

    /// The options that take this picture again.
    fn options(&self) -> String {
        let agl = -self.eye.z - self.ground();
        let preset = Preset::ALL.into_iter().find(|p| p.quality() == self.quality).map_or("high", Preset::name);
        format!(
            "eye={:.0},{:.0},{:.0} look={:.1},{:.1} hour={:.2} cover={:.0} quality={preset}",
            self.eye.x,
            self.eye.y,
            agl,
            self.heading.rem_euclid(std::f64::consts::TAU) / DEG,
            self.pitch / DEG,
            self.hour,
            self.world.cloud_cover * 100.0
        )
    }

    /// Moves the eye and the clock by the keys held.
    fn fly(&mut self, input: &Input, dt: f64, gpu: &Gpu) {
        let fast = if input.shift { 10.0 } else { 1.0 };
        let speed = 60.0 * fast;
        self.heading += input.axis(KeyCode::ArrowRight, KeyCode::ArrowLeft) * 1.2 * dt;
        self.pitch = (self.pitch + input.axis(KeyCode::ArrowUp, KeyCode::ArrowDown) * 0.8 * dt).clamp(-1.55, 1.55);
        if input.left_button && !input.pointer_over_ui {
            self.heading += input.mouse_delta.0 as f64 * 0.004;
            self.pitch = (self.pitch - input.mouse_delta.1 as f64 * 0.004).clamp(-1.55, 1.55);
        }
        let ahead = DVec3::new(self.heading.cos(), self.heading.sin(), 0.0);
        let side = DVec3::new(-self.heading.sin(), self.heading.cos(), 0.0);
        let move_by = ahead * input.axis(KeyCode::KeyW, KeyCode::KeyS) + side * input.axis(KeyCode::KeyD, KeyCode::KeyA) + DVec3::NEG_Z * input.axis(KeyCode::KeyR, KeyCode::KeyF);
        self.eye += move_by * speed * dt;
        // Never under the ground.
        self.eye.z = self.eye.z.min(-(self.ground() + 1.5));
        if input.is_down(KeyCode::KeyT) {
            self.hour = (self.hour + if input.shift { -dt } else { dt }).rem_euclid(24.0);
        }
        for key in &input.pressed {
            match key {
                KeyCode::Digit1 => self.quality = Quality::HIGH,
                KeyCode::Digit2 => self.quality = Quality::MEDIUM,
                KeyCode::Digit3 => self.quality = Quality::LOW,
                KeyCode::KeyC => self.world.cloud_cover = (self.world.cloud_cover + if input.shift { -0.1 } else { 0.1 }).clamp(0.0, 1.0),
                KeyCode::KeyP => println!("{}", self.options()),
                KeyCode::Escape => std::process::exit(0),
                _ => {}
            }
        }
        self.world.set_quality(gpu, self.quality);
    }
}

impl Game for Viewer {
    fn samples(&self) -> u32 {
        self.world.quality().samples
    }

    fn update(&mut self, input: &Input, dt: f64, gpu: &Gpu) {
        self.fly(input, dt, gpu);
        self.frames += 1;
        if let Some(speed) = self.cruise {
            // A sixtieth of a second a frame, whatever it took to draw.
            self.eye += DVec3::new(self.heading.cos(), self.heading.sin(), 0.0) * speed / 60.0;
        }
        self.follow_place(gpu);
        self.time += dt;
        // The crash goes on in the window; a picture shows the moment asked for.
        if let Some(world) = self.crash.as_mut().filter(|_| self.overlay) {
            for _ in 0..(dt * 240.0).round() as usize {
                world.step();
            }
        }
        self.frame += (dt - self.frame) * 0.05;
        let agl = -self.eye.z - self.ground();
        let scene = Scene {
            view: self.view(),
            hour: self.hour,
            time: self.crash.as_ref().map_or(self.time, World::time),
            weather: &self.weather,
            height_above_ground: agl,
            body: None,
            casters: &[],
            objects: &[],
            glows: &[],
            lamps: &[],
            see_through: &[],
            wrecks: self.crash.as_ref().map_or(&[], |w| &w.wrecks),
        };
        self.world.prepare(gpu, &scene);
    }

    fn ui(&mut self, ctx: &egui::Context) {
        if !self.overlay {
            return;
        }
        let text = format!("{} · {:.1} ms · {} draw calls", self.options(), self.frame * 1000.0, self.world.draw_calls());
        let painter = ctx.layer_painter(egui::LayerId::background());
        for (offset, color) in [(1.0, Color32::from_black_alpha(160)), (0.0, Color32::WHITE)] {
            painter.text(pos2(12.0 + offset, 10.0 + offset), Align2::LEFT_TOP, &text, FontId::monospace(13.0), color);
        }
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

    fn post(&self) -> engine::Post {
        self.world.post()
    }
}

/// A light aircraft flown into the ground `distance` metres ahead of (n, e) along `heading`, and the
/// world stepped until `seconds` after it came apart.
fn crash(seed: u64, terrain: &Arc<Terrain>, n: f64, e: f64, heading: f64, distance: f64, seconds: f64) -> World {
    let mut world = World::with_terrain(seed, terrain.clone());
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n, e, heading });
    // From 40 m up at 35° down, it hits the ground about `distance` ahead.
    let (dn, de) = (heading.cos(), heading.sin());
    let run = distance - 40.0 / 35f64.to_radians().tan();
    let (pn, pe) = (n + dn * run, e + de * run);
    let ground = world.terrain.height(pn, pe).max(0.0);
    let s = &mut world.vehicle_mut(id).aircraft_mut().unwrap().state;
    s.pos = V3::new(pn, pe, -(ground + 40.0));
    s.q = quaternion(heading, -35.0 * DEG, 0.0);
    s.vel = V3::new(90.0 * KNOTS, 0.0, 0.0);
    s.omega = V3::ZERO;
    let mut broke = None;
    for _ in 0..240 * 600 {
        world.step();
        if broke.is_none() && world.vehicle(id).aircraft().unwrap().damage().broken_up {
            broke = Some(world.time());
        }
        if broke.is_some_and(|t| world.time() - t >= seconds) {
            break;
        }
    }
    world
}

/// The aerodrome of a class nearest to the origin, within 100 km.
fn of_class(t: &Terrain, class: Class) -> Option<Airport> {
    let r = 100_000.0;
    let mut all = t.airports(-r, r, -r, r);
    all.sort_by(|a, b| a.n.hypot(a.e).total_cmp(&b.n.hypot(b.e)));
    all.into_iter().find(|a| Class::of(a) == class)
}

/// The eye on the 3° path of an aerodrome's main approach, `out` metres before the aiming point,
/// as the pilot's (2 m over the wheels), looking at the aiming point: eye, heading and pitch.
fn on_final(a: &Airport, out: f64) -> (DVec3, f64, f64) {
    let heading = a.heading + if lights::main_end_reversed(a) { std::f64::consts::PI } else { 0.0 };
    let back = a.length / 2.0 - lights::AIM + out;
    let height = a.elevation + out * lights::GLIDE_PATH.tan() + 2.0;
    (DVec3::new(a.n - heading.cos() * back, a.e - heading.sin() * back, -height), heading, -lights::GLIDE_PATH)
}

/// Prints where to point the eye: the home aerodrome and the towns, river and coast nearest to it,
/// and the class of the aerodromes near it.
fn places(t: &Terrain) {
    let Some(home) = t.nearest_airport(0.0, 0.0) else {
        return println!("no aerodrome near the origin");
    };
    println!("home aerodrome n {:.0} e {:.0} heading {:.0} elevation {:.0} m, {:?}", home.n, home.e, home.heading / DEG, home.elevation, Class::of(&home));
    for class in [Class::Visual, Class::NonPrecision, Class::PrecisionOne, Class::PrecisionTwo] {
        if let Some(a) = of_class(t, class) {
            println!("{class:?}: aerodrome n {:.0} e {:.0}, {:.1} km from the origin", a.n, a.e, a.n.hypot(a.e) / 1000.0);
        }
    }
    let r = 30_000.0;
    let mut towns = t.towns(home.n - r, home.n + r, home.e - r, home.e + r);
    towns.sort_by(|a, b| (a.n - home.n).hypot(a.e - home.e).total_cmp(&(b.n - home.n).hypot(b.e - home.e)));
    for town in towns.iter().take(8) {
        println!("town n {:.0} e {:.0} radius {:.0} m, {:.1} km from home", town.n, town.e, town.radius, (town.n - home.n).hypot(town.e - home.e) / 1000.0);
    }
    let mut lakes = t.lakes(home.n - 2.0 * r, home.n + 2.0 * r, home.e - 2.0 * r, home.e + 2.0 * r);
    lakes.sort_by(|a, b| (a.n - home.n).hypot(a.e - home.e).total_cmp(&(b.n - home.n).hypot(b.e - home.e)));
    for lake in lakes.iter().take(5) {
        println!("lake n {:.0} e {:.0} at {:.0} m, {:.0} by {:.0} m, {:.1} km from home", lake.n, lake.e, lake.level, 2.0 * lake.length, 2.0 * lake.width, (lake.n - home.n).hypot(lake.e - home.e) / 1000.0);
    }
    // The nearest river and sea, looking outwards ring by ring.
    let (mut river, mut coast) = (None, None);
    for ring in 1..400 {
        let d = ring as f64 * 250.0;
        for k in 0..(ring * 8) {
            let a = k as f64 / (ring * 8) as f64 * std::f64::consts::TAU;
            let (n, e) = (home.n + d * a.cos(), home.e + d * a.sin());
            let raw = t.raw(n, e);
            if river.is_none() && raw.river > 0.9 {
                river = Some((n, e));
            }
            if coast.is_none() && raw.h < 0.0 {
                coast = Some((n, e));
            }
        }
    }
    if let Some((n, e)) = river {
        println!("river n {n:.0} e {e:.0}");
    }
    if let Some((n, e)) = coast {
        println!("coast n {n:.0} e {e:.0}");
    }
    // The densest wood within 8 km, by the trees of its 256 m tiles.
    let size = 256.0;
    let densest = (-32..32)
        .flat_map(|i| (-32..32).map(move |j| (i, j)))
        .map(|(i, j)| {
            let key = (((home.e / size).floor() as i32) + i, ((-home.n / size).floor() as i32) + j);
            let count: usize = sim_core::scenery::trees(t, key, size).iter().map(|k| k.len() / 8).sum();
            (count, key)
        })
        .max_by_key(|&(count, _)| count);
    if let Some((count, key)) = densest {
        let (n, e) = (-(key.1 as f64 + 0.5) * size, (key.0 as f64 + 0.5) * size);
        println!("wood n {n:.0} e {e:.0}, {count} trees in {size:.0} m square, {:.1} km from home", (n - home.n).hypot(e - home.e) / 1000.0);
    }
}

/// The region of a real place, downloaded the first time, saying how it goes; the program ends if it
/// cannot be had.
fn region(place: &geodata::Place, offline: bool) -> sim_core::Region {
    let start = Instant::now();
    let shown = std::sync::Mutex::new(Instant::now());
    let loaded = geodata::load(place, offline, &|p: geodata::Progress| {
        let mut shown = shown.lock().unwrap_or_else(|e| e.into_inner());
        if shown.elapsed() > Duration::from_secs(1) {
            *shown = Instant::now();
            eprintln!("{:3.0} % {}", p.share() * 100.0, p.text());
        }
    });
    match loaded {
        Ok(loaded) => {
            println!("region {} in {:.1} s ({:.1} MB downloaded)", loaded.region.name, start.elapsed().as_secs_f64(), loaded.downloaded as f64 / 1e6);
            loaded.region
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

/// The satellites' picture of a region's ground, made the first time, saying how it goes; none if
/// it cannot be had (the ground keeps its own colours).
fn imagery(region: &Region, offline: bool) -> Option<Arc<Imagery>> {
    let start = Instant::now();
    let shown = std::sync::Mutex::new(Instant::now());
    let loaded = geodata::imagery::load(region, offline, &|p| {
        let mut shown = shown.lock().unwrap_or_else(|e| e.into_inner());
        if shown.elapsed() > Duration::from_secs(1) {
            *shown = Instant::now();
            eprintln!("{:3.0} % {}", p.share() * 100.0, p.text());
        }
    });
    match loaded {
        Ok(loaded) => {
            println!("imagery in {:.1} s ({:.1} MB downloaded), {:?}", start.elapsed().as_secs_f64(), loaded.downloaded as f64 / 1e6, loaded.imagery.years);
            Some(Arc::new(loaded.imagery))
        }
        Err(error) => {
            eprintln!("{error}: no satellite picture");
            None
        }
    }
}

/// Draws one picture to `<dir>/scene.png` once nothing is loading, and times frames after it.
fn shot(dir: &str, terrain: Arc<Terrain>, options: &Options) {
    if let Err(error) = std::fs::create_dir_all(dir) {
        eprintln!("cannot make {dir}: {error}");
        std::process::exit(1);
    }
    let path = format!("{dir}/scene.png");
    let start = Instant::now();
    // Once loaded: the frame it was at and, after the warm-up, when timing began.
    let mut measuring: Option<(usize, Option<Instant>)> = None;
    let mut times: Vec<f64> = Vec::with_capacity(TIMED);
    let mut last = Instant::now();
    let (width, height) = options.size;
    let viewer = engine::render_to_png(
        &path,
        width,
        height,
        1.0,
        |gpu| Viewer { overlay: false, ..Viewer::new(gpu, terrain, options) },
        |viewer, frames| match measuring {
            None => {
                if (viewer.world.pending() > 0 || frames < 5) && frames < FRAME_CAP {
                    std::thread::sleep(Duration::from_millis(20));
                    return false;
                }
                println!("world ready in {:.1} s ({frames} frames)", start.elapsed().as_secs_f64());
                measuring = Some((frames, None));
                false
            }
            Some((from, None)) => {
                if frames - from == WARM_UP {
                    measuring = Some((frames, Some(Instant::now())));
                }
                false
            }
            Some((from, Some(_))) if options.cruise.is_some() => {
                let now = Instant::now();
                if frames > from + 1 {
                    times.push((now - last).as_secs_f64() * 1000.0);
                }
                last = now;
                if (frames - from) as f64 / 60.0 < options.cruise.map_or(0.0, |c| c.1) {
                    return false;
                }
                cruised(&times, &viewer.swaps, from);
                true
            }
            Some((from, Some(_))) => {
                let now = Instant::now();
                if frames > from {
                    times.push((now - last).as_secs_f64() * 1000.0);
                }
                last = now;
                if frames - from < TIMED {
                    return false;
                }
                times.sort_by(f64::total_cmp);
                println!("frames: min {:.2} p25 {:.2} median {:.2} ms at {width}x{height}, {} draw calls", times[0], times[times.len() / 4], times[times.len() / 2], viewer.world.draw_calls());
                true
            }
        },
    );
    println!("{}", viewer.options());
}

/// What a cruise cost: every frame's time, the worst, and the worst within a second after each
/// region was swapped in (frame numbers of the viewer's, `from` the first timed).
fn cruised(times: &[f64], swaps: &[usize], from: usize) {
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
    let worst = times.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |w| w.0 + from + 2);
    println!("cruise: {} frames, median {:.2} p99 {:.2} worst {:.2} ms (frame {worst})", times.len(), at(0.5), at(0.99), at(1.0));
    for &swap in swaps {
        // `times[i]` is the frame `from + 2 + i` took (the first timed one waited since the start).
        let first = swap.saturating_sub(from + 2);
        let around: Vec<f64> = times.iter().skip(first.saturating_sub(2)).take(62).copied().collect();
        let worst = around.iter().copied().fold(0.0, f64::max);
        println!("  swap at frame {swap}: the worst frame of the second after it {worst:.2} ms");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().find_map(|a| a.strip_prefix(name)?.strip_prefix('='));
    let numbers = |name: &str| value(name).map(|v| v.split(',').filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<f64>>());
    let number = |name: &str, default: f64| value(name).and_then(|v| v.parse::<f64>().ok()).unwrap_or(default);
    let known = ["eye", "look", "hour", "seed", "visibility", "humidity", "cover", "base", "thickness", "storms", "wind", "quality", "textures", "size", "crash", "final", "aerodrome", "place", "at", "offline", "imagery", "cruise"];
    let mut shot_dir = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--shot" => shot_dir = rest.next().cloned(),
            "places" => {}
            other if known.iter().any(|k| other.strip_prefix(k).is_some_and(|v| v.starts_with('='))) => {}
            other => eprintln!("unknown option: {other}"),
        }
    }
    let mut quality = value("quality").and_then(Preset::named).unwrap_or(Preset::High).quality();
    if value("textures") == Some("off") {
        quality.texture_size = 0;
    }
    let wind = numbers("wind").unwrap_or_default();
    let options = Options {
        seed: number("seed", 1.0) as u64,
        eye: numbers("eye").filter(|p| p.len() == 3).map(|p| (p[0], p[1], p[2])),
        look: numbers("look").filter(|p| p.len() == 2).map(|p| (p[0], p[1])),
        hour: number("hour", 10.5).rem_euclid(24.0),
        cover: (number("cover", 63.0) / 100.0).clamp(0.0, 1.0),
        base: number("base", 2300.0),
        thickness: number("thickness", 650.0),
        storms: number("storms", 0.0).clamp(0.0, 3.0) as u32,
        weather: Weather {
            wind_from: wind.first().copied().unwrap_or(250.0),
            wind_speed: wind.get(1).copied().unwrap_or(0.0) * KNOTS,
            visibility: number("visibility", Weather::default().visibility / 1000.0) * 1000.0,
            air: Atmosphere { relative_humidity: (number("humidity", 0.0) / 100.0).clamp(0.0, 1.0), ..Atmosphere::STANDARD },
            ..Weather::default()
        },
        quality,
        size: value("size").and_then(|v| v.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).unwrap_or((1600, 1000)),
        crash: numbers("crash").filter(|c| !c.is_empty()).map(|c| (c[0], c.get(1).copied().unwrap_or(120.0))),
        on_final: value("final").and_then(|v| v.parse::<f64>().ok()),
        aerodrome: value("aerodrome").and_then(|v| match v {
            "visual" => Some(Class::Visual),
            "nonprecision" => Some(Class::NonPrecision),
            "cat1" => Some(Class::PrecisionOne),
            "cat2" => Some(Class::PrecisionTwo),
            other => {
                eprintln!("aerodrome: visual, nonprecision, cat1 or cat2, not {other}");
                None
            }
        }),
        over_point: value("at").is_some(),
        imagery: None,
        cruise: numbers("cruise").filter(|c| c.len() == 2).map(|c| (c[0], c[1])),
        place: None,
        offline: value("offline") == Some("1"),
    };
    let place = value("place").or(value("at")).map(|v| geodata::Place::parse(v).unwrap_or_else(|| {
        eprintln!("place=<ICAO> or at=<lat>,<lon>, not {v}");
        std::process::exit(2);
    }));
    let offline = value("offline") == Some("1");
    let region = place.as_ref().map(|place| Arc::new(region(place, offline)));
    let options = Options { place: place.clone(), imagery: region.as_ref().filter(|_| value("imagery") != Some("off")).and_then(|r| imagery(r, offline)), ..options };
    let terrain = match region {
        Some(region) => Arc::new(Terrain::from_region(options.seed, region)),
        None => Arc::new(Terrain::new(options.seed)),
    };
    if args.iter().any(|a| a == "places") {
        return places(&terrain);
    }
    match shot_dir {
        Some(dir) => shot(&dir, terrain, &options),
        None => engine::run("World", 1440, 900, |gpu| Viewer::new(gpu, terrain, &options)),
    }
}
