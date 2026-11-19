//! The driving game: a car on a circuit of the generated world, driven by the player or by the
//! racing driver (`sim_core::car::driver::Driver`), drawn into the world with its shadow, timed lap
//! by lap, heard.

use std::sync::Arc;

use engine::egui;
use engine::{wgpu, Game, Gpu, Input, KeyCode};
use glam::DVec3;
use sim_core::car::driver::Driver;
use sim_core::circuit::{Circuit, CircuitFile};
use sim_core::circuit::race::Race;
use sim_core::{Body, CarKind, Start, Terrain, VehicleId, World, DEG};
use world_render::{buildings, to_render, Body as ShadowBody, Caster, Lamp, ObjectDraw, Quality, Scene, TrackRenderer, WorldRenderer};

use crate::audio::{Audio, Listener, Params};
use crate::camera::{Cameras, View};
use crate::controls::{Driving, Feet};
use crate::effects::Effects;
use crate::ghost::Ghost;
use crate::guide::{self, Guide};
use crate::hud;
use crate::look::{self, CarLook};
use crate::race::LEAD_IN;
use crate::replay::{Playback, Recorder, SPEEDS};
use crate::rivals::{self, Rival};
use crate::timing::Timing;

/// What the command line chose.
#[derive(Clone, Debug)]
pub struct Options {
    pub circuit: String,
    pub car: String,
    pub hour: f64,
    pub view: View,
    /// The racing driver drives (the player watches), from the start.
    pub driver: bool,
    /// Start on the racing line this many metres past the start line, at the line's speed.
    pub at: Option<f64>,
    pub quality: Quality,
    /// Cloud cover, 0 to 1.
    pub cover: f64,
    pub automatic: bool,
    pub abs: bool,
    pub traction_control: bool,
    pub stability_control: bool,
    /// The chase camera turned round the car, degrees.
    pub orbit: (f64, f64),
    /// How far the chase camera sits from the car, m (`None`: where it usually does).
    pub distance: Option<f64>,
    /// The paint's colour (linear), instead of the model's.
    pub paint: Option<[f32; 3]>,
    /// How hard it rains, 0 dry to 1 a downpour: the sky closed, the road wet, its grip less
    /// (`sim_core::car::wet`).
    pub rain: f64,
    /// Rivals on the grid (0: free practice, alone with the ghost of the best lap).
    pub rivals: usize,
    /// What drives the first rivals (the racing driver for the rest).
    pub rival_kinds: Vec<rivals::Kind>,
    /// Laps of a race.
    pub laps: u32,
    /// The player's place on the grid (0 the pole; `None`: at the back).
    pub grid: Option<usize>,
    /// The rivals' characters and the start's lights are drawn from this.
    pub seed: u64,
    /// For a picture: the replay, stopped this many seconds before the end of what is kept.
    pub replay: Option<f64>,
    /// For pictures: what the car does by itself while `--wait` runs (`Stunt`).
    pub stunt: Option<Stunt>,
    /// Without what the lens adds (vignette, blur of speed, flare, depth of field): to compare.
    pub plain: bool,
    /// The pause menu shown from the start (`--menu`).
    pub menu: bool,
    /// The driving line on the road (`guide`): the whole of it unless said (off in a picture).
    pub line: Option<guide::Mode>,
}

/// Something the car does by itself for a picture of what its tyres leave: nobody drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stunt {
    /// Full throttle from where it stands: the rear tyres spin.
    Launch,
    /// The brakes hard on, no anti-lock: the tyres lock.
    Lock,
    /// A turn on the hand brake with the throttle open: the tail slides.
    Slide,
    /// For the sound, some twenty seconds from the grid: idling, full throttle up through the
    /// gears, lifting off, the brakes locked, a slide on the hand brake.
    Run,
}

impl Stunt {
    pub fn parse(word: &str) -> Option<Stunt> {
        match word {
            "launch" => Some(Stunt::Launch),
            "lock" => Some(Stunt::Lock),
            "slide" => Some(Stunt::Slide),
            "run" => Some(Stunt::Run),
            _ => None,
        }
    }
}

impl Default for Options {
    fn default() -> Options {
        Options {
            circuit: "lakeside".into(),
            car: "gt".into(),
            hour: 17.5,
            view: View::Chase,
            driver: false,
            at: None,
            quality: Quality::HIGH,
            cover: 0.35,
            automatic: true,
            abs: true,
            traction_control: true,
            stability_control: true,
            orbit: (0.0, 0.0),
            distance: None,
            paint: None,
            rain: 0.0,
            rivals: 0,
            rival_kinds: Vec::new(),
            laps: 3,
            grid: None,
            seed: 1,
            replay: None,
            stunt: None,
            plain: false,
            menu: false,
            line: None,
        }
    }
}

/// The world of a circuit: the circuit laid out on its natural ground, and the terrain cut and filled to it.
pub fn circuit_world(name: &str) -> Result<(Arc<Circuit>, Arc<Terrain>), String> {
    let file = CircuitFile::find(name).ok_or_else(|| format!("no circuit \"{name}\" (there is: {})", sim_core::circuit::circuits().iter().map(|c| c.key.as_str()).collect::<Vec<_>>().join(", ")))?;
    let natural = Terrain::new(file.seed());
    let circuit = Arc::new(Circuit::new(file, &natural));
    let terrain = Arc::new(Terrain::new(file.seed()).with_circuits(vec![circuit.clone()]));
    Ok((circuit, terrain))
}

pub struct Drive {
    pub world: World,
    pub id: VehicleId,
    pub circuit: Arc<Circuit>,
    pub renderer: WorldRenderer,
    track: TrackRenderer,
    look: Option<CarLook>,
    effects: Effects,
    /// The driving line on the road.
    pub guide: Guide,
    /// The quality and samples the game's own pipelines were made for.
    made_for: Option<(Quality, u32)>,
    /// How wet the road is, 0 to 1 (it rains under a closed sky).
    pub wet: f32,
    /// Steps of the simulation since the tyres' effects were last brought up to date.
    steps: u32,
    /// Without what the lens adds (`Options::plain`).
    plain: bool,
    /// What the car does by itself (`Options::stunt`), and for how long it has.
    pub stunt: Option<(Stunt, f64)>,
    pub cameras: Cameras,
    driving: Driving,
    /// The racing driver, when it drives.
    pub driver: Option<Driver>,
    pub timing: Timing,
    audio: Audio,
    /// What the sound was last told (`--sound` renders it off line), and the hardest impact
    /// since then, m/s.
    pub sound: Params,
    knock: f64,
    pub hour: f64,
    /// Simulation time owed to real time, s.
    lag: f64,
    /// The cars' bodies as the simulation has them, while they are drawn a little further on
    /// (`ahead`).
    stepped: Vec<(VehicleId, Body)>,
    pub paused: bool,
    /// The pause menu is shown (Esc): the game waits.
    pub menu: Option<crate::pause::Menu>,
    pub help: bool,
    /// The HUD and the help are drawn (not in a screenshot unless asked).
    pub overlay: bool,
    objects: Vec<ObjectDraw>,
    see_through: Vec<ObjectDraw>,
    lamps: Vec<Lamp>,
    /// A line said for a moment: (text, seconds left).
    pub message: Option<(String, f64)>,
    /// The rivals, their looks and what their tyres leave (one each, in the order of `rivals`).
    pub rivals: Vec<Rival>,
    rival_looks: Vec<Option<CarLook>>,
    rival_effects: Vec<Effects>,
    /// The race, when there are rivals.
    pub race: Option<Race>,
    laps: u32,
    seed: u64,
    /// What is kept for the replay, and the replay being shown.
    pub recorder: Recorder,
    pub playback: Option<Playback>,
    /// The ghost of the best lap, in practice, and its look.
    pub ghost: Option<Ghost>,
    ghost_look: Option<CarLook>,
    ghost_shown: bool,
    /// The car the cameras follow: 0 the player's, k the kth rival's.
    pub watch: usize,
    /// The player's place on the grid.
    player_place: usize,
    /// How hard it rains, 0 to 1 (the world's weather).
    pub rain: f64,
    /// Seconds behind the ghost here (negative ahead).
    pub ghost_delta: Option<f64>,
    /// Seconds since the start, for what moves by itself (clouds, water).
    clock: f64,
    car_kind: CarKind,
    /// The view of the frame being drawn and the picture's width over its height, for what the
    /// lens does (`post`).
    view: Option<world_render::ViewPoint>,
    aspect: f32,
}

impl Drive {
    pub fn new(gpu: &Gpu, options: &Options, audio: Audio) -> Drive {
        let (circuit, terrain) = circuit_world(&options.circuit).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        });
        let car_kind = CarKind::parse(&options.car).unwrap_or_else(|| {
            eprintln!("no car \"{}\"", options.car);
            std::process::exit(2);
        });
        let mut world = World::with_terrain(1, terrain.clone());
        world.hour = options.hour;
        // The rain, and the water it has left on the circuit by now.
        world.weather.rain = options.rain.clamp(0.0, 1.0);
        world.settle_water();
        let racing = options.rivals > 0;
        let player_place = if racing { options.grid.unwrap_or(options.rivals).min(options.rivals) } else { 0 };
        let (n, e, heading) = circuit.grid(player_place);
        let id = world.add_car(car_kind, Start::Air { n, e, heading });
        if let Some(car) = world.vehicle_mut(id).car_mut() {
            car.controls.automatic = options.automatic;
            car.controls.abs = options.abs;
            car.controls.traction_control = options.traction_control;
            car.controls.stability_control = options.stability_control;
        }
        let mut renderer = WorldRenderer::new(gpu, terrain.clone(), buildings::stand_in());
        renderer.set_quality(gpu, options.quality);
        renderer.cloud_cover = if options.rain > 0.0 { (0.85 + 0.3 * options.rain).min(1.0) } else { options.cover };
        let look = match CarLook::load(gpu, &renderer, &car_kind.spec().visual.model) {
            Ok(mut l) => {
                if let Some(paint) = options.paint {
                    l.paint = paint;
                }
                Some(l)
            }
            Err(e) => {
                eprintln!("the car's model: {e}");
                None
            }
        };
        // The asphalt darker with rubber along the racing line, most where the cars brake and turn.
        let line = sim_core::car::driver::RacingLine::new(&circuit, car_kind.spec(), Default::default());
        let fastest = line.speed.iter().copied().fold(1.0, f64::max);
        let rubber: Vec<world_render::track::Line> = line.across.iter().zip(&line.speed).map(|(&a, &v)| [a as f32, (1.25 - v / fastest).clamp(0.15, 1.0) as f32]).collect();
        let mut track = TrackRenderer::with_lines(gpu, &renderer, &terrain, &[rubber]);
        track.wet = world.water.first().map_or(0.0, |w| w.standing as f32);
        // The rivals, on the grid's other places, and the race.
        let places: Vec<usize> = (0..=options.rivals).filter(|&k| k != player_place).take(options.rivals).collect();
        let rivals = rivals::grid(&mut world, &circuit, car_kind, &places, &options.rival_kinds, options.seed, true).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        });
        let rival_looks: Vec<Option<CarLook>> = rivals.iter().map(|r| look.as_ref().map(|l| l.share(gpu, &renderer, r.paint))).collect();
        let rival_effects: Vec<Effects> = rivals
            .iter()
            .map(|_| {
                let mut fx = Effects::new(gpu, &renderer, DVec3::new(n, e, -terrain.height(n, e)));
                fx.drops = false;
                fx
            })
            .collect();
        let race = racing.then(|| {
            let mut names = vec![(id, "You".to_string())];
            names.extend(rivals.iter().map(|r| (r.id, r.name.clone())));
            Race::new(circuit.clone(), options.laps, &names, world.time() + LEAD_IN, options.seed)
        });
        let ghost = (!racing).then(|| Ghost::load(&circuit.key, world.vehicle(id).car().expect("the player's car")));
        let ghost_look = if racing { None } else { look.as_ref().map(|l| l.share(gpu, &renderer, l.paint)) };
        let effects = Effects::new(gpu, &renderer, DVec3::new(n, e, -terrain.height(n, e)));
        let guide = Guide::new(gpu, &renderer, &circuit, world.vehicle(id).car().expect("the player's car"), options.line.unwrap_or(guide::Mode::Full));
        let driver = options.driver.then(|| Driver::new(circuit.clone(), car_kind.spec()));
        let mut cameras = Cameras::new(options.view);
        cameras.orbit = (options.orbit.0 * DEG, options.orbit.1 * DEG);
        if let Some(d) = options.distance {
            cameras.distance = d.clamp(2.0, 60.0);
        }
        let wet = track.wet;
        let mut game = Drive {
            world,
            id,
            circuit,
            renderer,
            track,
            look,
            effects,
            guide,
            made_for: None,
            wet,
            steps: 0,
            plain: options.plain,
            stunt: options.stunt.map(|s| (s, 0.0)),
            cameras,
            driving: Driving::new(),
            driver,
            timing: Timing::default(),
            audio,
            sound: Params::default(),
            knock: 0.0,
            hour: options.hour,
            lag: 0.0,
            stepped: Vec::new(),
            paused: false,
            menu: None,
            help: false,
            overlay: true,
            objects: Vec::new(),
            see_through: Vec::new(),
            lamps: Vec::new(),
            message: None,
            rivals,
            rival_looks,
            rival_effects,
            race,
            laps: options.laps,
            seed: options.seed,
            recorder: Recorder::default(),
            playback: None,
            ghost,
            ghost_look,
            ghost_shown: false,
            watch: 0,
            player_place,
            ghost_delta: None,
            rain: options.rain.clamp(0.0, 1.0),
            clock: 0.0,
            car_kind,
            view: None,
            aspect: 1.6,
        };
        if let Some(along) = options.at {
            game.place_on_line(along);
        }
        if options.menu {
            game.open_menu();
        }
        game
    }

    /// The car on the racing line at `along` metres past the start line, going at the line's speed.
    pub fn place_on_line(&mut self, along: f64) {
        let c = self.circuit.clone();
        let line = self.driver.as_ref().map(|d| d.line.clone()).unwrap_or_else(|| sim_core::car::driver::RacingLine::new(&c, self.car_kind.spec(), Default::default()));
        let at = c.start + along;
        let ((n, e), speed) = line.at(&c, at);
        let ((n2, e2), _) = line.at(&c, at + 2.0);
        let heading = (e2 - e).atan2(n2 - n);
        self.world.stand(self.id, n, e, heading);
        if let Some(car) = self.world.vehicle_mut(self.id).car_mut() {
            let b = car.body;
            let gear = (1..=car.p.gearbox.ratios.len() as i32).find(|&g| car.p.top_of_gear(g) > speed * 1.15).unwrap_or(car.p.gearbox.ratios.len() as i32);
            car.set_gear(gear);
            car.put(Body { vel: DVec3::new(speed, 0.0, 0.0), ..b });
        }
        if let Some(d) = &mut self.driver {
            d.reset();
        }
        self.timing.restart();
        self.cameras.reset();
        self.effects.lift();
    }

    /// Back to the grid, at rest; in a race, everyone, and the race again.
    pub fn restart(&mut self) {
        if let Some(p) = self.playback.take() {
            p.end(&mut self.world);
        }
        if self.race.is_some() {
            // Each car back to the place it started from.
            for id in self.ids() {
                let (n, e, heading) = self.circuit.grid(self.grid_place(id).unwrap_or(0));
                self.world.stand(id, n, e, heading);
            }
            for r in &mut self.rivals {
                r.reset();
            }
            for fx in &mut self.rival_effects {
                fx.lift();
            }
            let race = self.race.as_ref().expect("a race");
            let names: Vec<(VehicleId, String)> = race.entrants.iter().map(|e| (e.id, e.name.clone())).collect();
            self.race = Some(Race::new(self.circuit.clone(), self.laps, &names, self.world.time() + LEAD_IN, self.seed));
            self.recorder.clear();
        }
        let place = self.grid_place(self.id).unwrap_or(0);
        let (n, e, heading) = self.circuit.grid(place);
        self.world.stand(self.id, n, e, heading);
        self.effects.lift();
        self.timing.restart();
        self.cameras.reset();
        self.driving.release();
        if let Some(d) = &mut self.driver {
            d.reset();
        }
    }

    /// The grid's place a car started from (the one nearest to where the grid's places are, kept
    /// in the order the cars were added).
    fn grid_place(&self, id: VehicleId) -> Option<usize> {
        if self.race.is_none() {
            return Some(0);
        }
        // The player and the rivals took the places in order, the player's left out of theirs.
        let n = self.rivals.len() + 1;
        let mut free: Vec<usize> = (0..n).collect();
        let player = self.player_place;
        free.retain(|&k| k != player);
        if id == self.id {
            return Some(player);
        }
        self.rivals.iter().position(|r| r.id == id).map(|k| free[k])
    }

    /// The cars, the player's first, then the rivals'.
    pub fn ids(&self) -> Vec<VehicleId> {
        std::iter::once(self.id).chain(self.rivals.iter().map(|r| r.id)).collect()
    }

    /// The car the cameras follow.
    fn watched(&self) -> VehicleId {
        if self.watch == 0 { self.id } else { self.rivals.get(self.watch - 1).map_or(self.id, |r| r.id) }
    }

    /// Back on the circuit where the car is, stopped, facing the way round.
    pub fn recover(&mut self) {
        let b = self.world.vehicle(self.id).body();
        let Some(at) = self.circuit.locate(b.pos.x, b.pos.y) else { return self.restart() };
        let (n, e, _, heading) = self.circuit.at(at.along);
        self.world.stand(self.id, n, e, heading);
        self.effects.lift();
        self.cameras.reset();
        self.driving.release();
        self.timing.invalid = true;
    }

    /// Simulates `seconds` with the racing driver (or the controls as they are), as the game would.
    pub fn run(&mut self, seconds: f64) {
        for _ in 0..(seconds / World::DT).round() as usize {
            self.step();
        }
    }

    fn step(&mut self) {
        self.back();
        // On the grid until the lights go out: everyone stands with the brakes on.
        let holding = self.race.as_ref().is_some_and(Race::holding);
        if holding {
            for id in self.ids() {
                if let Some(car) = self.world.vehicle_mut(id).car_mut() {
                    (car.controls.brake, car.controls.throttle) = (1.0, 0.0);
                }
            }
        } else {
            for r in &mut self.rivals {
                r.drive(&mut self.world);
            }
        }
        if let Some(d) = self.driver.as_mut().filter(|_| !holding) {
            self.world.fly(self.id, d);
        }
        if let Some((stunt, time)) = &mut self.stunt {
            *time += World::DT;
            let t = *time;
            if let Some(car) = self.world.vehicle_mut(self.id).car_mut() {
                let c = &mut car.controls;
                (c.abs, c.traction_control, c.stability_control) = (false, false, false);
                match stunt {
                    Stunt::Launch => (c.throttle, c.brake, c.steer) = (1.0, 0.0, 0.0),
                    Stunt::Lock => (c.throttle, c.brake, c.steer) = (0.0, if t > 0.4 { 1.0 } else { 0.0 }, 0.0),
                    Stunt::Run => {
                        c.handbrake = 0.0;
                        (c.throttle, c.brake, c.steer) = match t {
                            t if t < 4.0 => (0.0, 0.0, 0.0),
                            t if t < 13.5 => (1.0, 0.0, 0.0),
                            t if t < 15.5 => (0.0, 0.0, 0.0),
                            t if t < 17.5 => (0.0, 1.0, 0.0),
                            t if t < 21.5 => {
                                c.handbrake = if (17.6..18.3).contains(&t) { 1.0 } else { 0.0 };
                                (if t > 18.0 { 0.7 } else { 0.3 }, 0.0, if t < 19.6 { 0.6 } else { -0.3 })
                            }
                            _ => (0.0, 0.3, 0.0),
                        };
                    }
                    Stunt::Slide => {
                        c.steer = if t < 1.6 { 0.55 } else { -0.25 };
                        c.handbrake = if (0.25..0.9).contains(&t) { 1.0 } else { 0.0 };
                        (c.throttle, c.brake) = (if t > 0.8 { 1.0 } else { 0.2 }, 0.0);
                    }
                }
            }
        }
        self.world.step();
        if let Some(car) = self.world.vehicle(self.id).car() {
            self.knock = self.knock.max(car.data().impact);
        }
        // What the tyres leave, 60 times a second.
        self.steps += 1;
        if self.steps >= 4 {
            let w = &self.world.weather;
            let from = w.wind_from * DEG;
            let wind = DVec3::new(-from.cos(), -from.sin(), 0.0) * w.wind_speed;
            if let Some(car) = self.world.vehicle(self.id).car() {
                self.effects.update(car, World::DT * self.steps as f64, self.wet, wind);
            }
            for (r, fx) in self.rivals.iter().zip(&mut self.rival_effects) {
                if let Some(car) = self.world.vehicle(r.id).car() {
                    fx.update(car, World::DT * self.steps as f64, self.wet, wind);
                }
            }
            self.steps = 0;
        }
        let b = self.world.vehicle(self.id).body();
        self.timing.update(&self.circuit, b.pos.x, b.pos.y, self.world.time());
        if let Some(race) = &mut self.race {
            race.update(&self.world);
        }
        if let (Some(ghost), Some(car)) = (&mut self.ghost, self.world.vehicle(self.id).car()) {
            let t = &self.timing;
            ghost.update(&self.world, car, t.lap, t.current(self.world.time()), t.last, !t.last_invalid);
        }
        let ids = self.ids();
        self.recorder.record(&self.world, &ids);
    }

    /// The rain from now on (the water on the circuit follows it, over minutes).
    pub fn set_rain(&mut self, rain: f64) {
        self.rain = (rain * 10.0).round() / 10.0;
        self.world.weather.rain = self.rain;
        self.renderer.cloud_cover = if self.rain > 0.0 { (0.85 + 0.3 * self.rain).min(1.0) } else { self.renderer.cloud_cover.min(0.6) };
        let text = if self.rain <= 0.0 { "The rain stops: the road dries, the racing line first".to_string() } else { format!("Rain {:.0} %", self.rain * 100.0) };
        self.say(text);
    }

    /// The cameras on the next car (or the one before).
    fn watch_next(&mut self, forward: bool) {
        let n = self.rivals.len() + 1;
        self.watch = (self.watch + if forward { 1 } else { n - 1 }) % n;
        self.cameras.reset();
        let name = if self.watch == 0 { "your car".to_string() } else { self.rivals[self.watch - 1].name.clone() };
        self.say(format!("Watching {name}"));
    }

    /// Shows the replay from `before` seconds before the end of what is kept, or from the start of
    /// the player's last lap (a minute back if there is none).
    pub fn begin_replay(&mut self, before: Option<f64>) {
        self.back();
        let Some((first, last)) = self.recorder.span() else { return };
        let now = self.world.time();
        let from = match before {
            Some(b) => last - b,
            None => match (self.timing.current(now), self.timing.last) {
                (Some(t), Some(lap)) => now - t - lap,
                (Some(t), None) if t > 10.0 => now - t,
                _ => now - 60.0,
            },
        };
        let ids = self.ids();
        self.playback = Some(Playback::begin(&self.world, &ids, from.clamp(first, last)));
        self.effects.lift();
        for fx in &mut self.rival_effects {
            fx.lift();
        }
        self.cameras.reset();
        self.say("Replay: Space stop, ← → 5 s, ↓ ↑ speed, [ ] car, F2 back");
    }

    /// Back from the replay to the session, as it was.
    pub fn end_replay(&mut self) {
        if let Some(p) = self.playback.take() {
            p.end(&mut self.world);
        }
        self.effects.lift();
        for fx in &mut self.rival_effects {
            fx.lift();
        }
        self.cameras.reset();
        self.lag = 0.0;
    }

    /// The keys of the replay.
    fn replay_keys(&mut self, input: &Input) {
        for key in input.pressed.clone() {
            let step = if input.shift { 30.0 } else { 5.0 };
            let Some(p) = self.playback.as_mut() else { return };
            match key {
                KeyCode::F2 | KeyCode::Tab | KeyCode::Escape => return self.end_replay(),
                KeyCode::Space | KeyCode::KeyP => {
                    if p.paused && self.recorder.span().is_some_and(|(_, last)| p.time >= last) {
                        p.time = self.recorder.span().map_or(p.time, |(first, _)| first);
                    }
                    p.paused = !p.paused;
                }
                KeyCode::ArrowLeft => p.seek(-step, &self.recorder),
                KeyCode::ArrowRight => p.seek(step, &self.recorder),
                KeyCode::ArrowUp => p.speed = (p.speed + 1).min(SPEEDS.len() - 1),
                KeyCode::ArrowDown => p.speed = p.speed.saturating_sub(1),
                KeyCode::Home => p.seek(-1e9, &self.recorder),
                KeyCode::End => p.seek(1e9, &self.recorder),
                KeyCode::BracketRight | KeyCode::BracketLeft => self.watch_next(key == KeyCode::BracketRight),
                KeyCode::KeyC => {
                    self.cameras.view = self.cameras.view.next();
                    self.cameras.reset();
                }
                _ => {}
            }
        }
    }

    /// Shows the pause menu: the game waits. "Change vehicle" is offered if Skylab is beside this program.
    fn open_menu(&mut self) {
        self.menu = Some(crate::pause::Menu::new(engine::program::beside(crate::pause::SKYLAB).is_some()));
        self.paused = true;
    }

    /// What was chosen in the pause menu, done.
    fn pause_choice(&mut self, choice: Option<crate::pause::Choice>) {
        use crate::pause::Choice;
        match choice {
            Some(Choice::Resume) => {
                self.menu = None;
                self.paused = false;
            }
            // Skylab takes this game's place, on its first screen; if it cannot, the menu says why.
            Some(Choice::Vehicles) => {
                let error = engine::program::hand_over(crate::pause::SKYLAB, &crate::pause::FIRST_SCREEN);
                if let Some(menu) = &mut self.menu {
                    menu.error = Some(error);
                }
            }
            Some(Choice::Quit) => std::process::exit(0),
            None => {}
        }
    }

    fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), 2.5));
    }

    /// The keys that are not driving.
    fn keys(&mut self, input: &Input) {
        fn car_controls(g: &mut Drive) -> Option<&mut sim_core::CarControls> {
            g.world.vehicle_mut(g.id).car_mut().map(|c| &mut c.controls)
        }
        if let Some(menu) = &mut self.menu {
            let choice = menu.keys(input);
            return self.pause_choice(choice);
        }
        if self.playback.is_some() {
            return self.replay_keys(input);
        }
        if input.controllers.iter().any(|c| c.pressed(engine::PadButton::Start)) {
            return self.open_menu();
        }
        for key in input.pressed.clone() {
            match key {
                KeyCode::F2 | KeyCode::Tab => self.begin_replay(None),
                KeyCode::BracketRight | KeyCode::BracketLeft => self.watch_next(key == KeyCode::BracketRight),
                KeyCode::F6 | KeyCode::F7 => {
                    let rain = (self.rain + if key == KeyCode::F6 { 0.2 } else { -0.2 }).clamp(0.0, 1.0);
                    self.set_rain(rain);
                }
                // The rest of the frame's keys are not the menu's.
                KeyCode::Escape => return self.open_menu(),
                KeyCode::KeyC => {
                    self.cameras.view = self.cameras.view.next();
                    self.cameras.reset();
                    let name = self.cameras.view.name();
                    self.say(format!("Camera: {name}"));
                }
                KeyCode::KeyR if input.shift => self.restart(),
                KeyCode::KeyR => self.recover(),
                KeyCode::KeyP => self.paused = !self.paused,
                KeyCode::KeyH | KeyCode::F1 => self.help = !self.help,
                KeyCode::KeyM => {
                    if let Some(c) = car_controls(self) {
                        c.automatic = !c.automatic;
                        let text = if c.automatic { "Gearbox: automatic" } else { "Gearbox: sequential (E up, Q down)" };
                        self.say(text);
                    }
                }
                KeyCode::KeyB => {
                    if let Some(c) = car_controls(self) {
                        c.abs = !c.abs;
                        let text = if c.abs { "Anti-lock brakes on" } else { "Anti-lock brakes off" };
                        self.say(text);
                    }
                }
                KeyCode::KeyT => {
                    if let Some(c) = car_controls(self) {
                        c.traction_control = !c.traction_control;
                        let text = if c.traction_control { "Traction control on" } else { "Traction control off" };
                        self.say(text);
                    }
                }
                KeyCode::KeyY => {
                    if let Some(c) = car_controls(self) {
                        c.stability_control = !c.stability_control;
                        let text = if c.stability_control { "Stability control on" } else { "Stability control off" };
                        self.say(text);
                    }
                }
                KeyCode::KeyG => {
                    self.driving.pedal_help = !self.driving.pedal_help;
                    self.say(if self.driving.pedal_help { "Pedal help on: the keys ease as the tyres spin or lock" } else { "Pedal help off: the pedals' keys are all or nothing" });
                }
                KeyCode::KeyL => {
                    self.guide.mode = self.guide.mode.next();
                    let name = self.guide.mode.name();
                    self.say(format!("Driving line: {name}"));
                }
                KeyCode::KeyX => {
                    if self.driver.take().is_none() {
                        self.driver = Some(Driver::new(self.circuit.clone(), self.car_kind.spec()));
                        self.say("The racing driver drives (X to take over)");
                    } else {
                        self.say("You drive");
                    }
                }
                KeyCode::KeyN => {
                    self.hour = (self.hour + if input.shift { -0.5 } else { 0.5 }).rem_euclid(24.0);
                    self.world.hour = self.hour;
                }
                _ => {}
            }
        }
    }
}

impl Drive {
    /// The cars drawn where they are now, not where the last step left them. The simulation goes
    /// in steps of `World::DT` and a frame takes however long it takes: one frame holds a step
    /// more than the next, and a car drawn at its last step jumps back and forth before a camera
    /// that moves with the frame's own time (20 cm at 180 km/h). So for the picture each car is
    /// moved on by the time the simulation still owes (`lag`), at the velocity it has; `back` puts
    /// them as the simulation has them before anything else looks at them.
    fn ahead(&mut self) {
        let lag = self.lag.clamp(0.0, World::DT);
        for id in self.ids() {
            let Some(car) = self.world.vehicle_mut(id).car_mut() else { continue };
            let b = car.body;
            self.stepped.push((id, b));
            car.body.pos = b.pos + b.world_velocity() * lag;
            car.body.q = (b.q * sim_core::Quat::from_scaled_axis(b.omega * lag)).normalize();
        }
    }

    fn back(&mut self) {
        for (id, body) in self.stepped.drain(..) {
            if let Some(car) = self.world.vehicle_mut(id).car_mut() {
                car.body = body;
            }
        }
    }

    /// A frame of the replay: the moment shown moves on, the cars shown as they were then, and
    /// what their tyres left made again.
    fn replaying(&mut self, dt: f64) {
        let ids = self.ids();
        let Some(p) = self.playback.as_mut() else { return };
        let moved = p.advance(dt, &self.recorder);
        p.show(&mut self.world, &ids, &self.recorder);
        if moved > 0.0 {
            let w = &self.world.weather;
            let from = w.wind_from * DEG;
            let wind = DVec3::new(-from.cos(), -from.sin(), 0.0) * w.wind_speed;
            if let Some(car) = self.world.vehicle(self.id).car() {
                self.effects.update(car, moved, self.wet, wind);
            }
            for (r, fx) in self.rivals.iter().zip(&mut self.rival_effects) {
                if let Some(car) = self.world.vehicle(r.id).car() {
                    fx.update(car, moved, self.wet, wind);
                }
            }
        }
    }

}

impl Drive {
    /// What is see-through, over what is seen through it: smoke, dust and spray, then the glass
    /// of every car, then the ghost. `second`: in the frame's second pass (the wet road's).
    fn see_through(&self, pass: &mut wgpu::RenderPass<'_>, second: bool) {
        self.renderer.bind_frame(pass);
        self.effects.draw_puffs(pass);
        for fx in &self.rival_effects {
            fx.draw_puffs(pass);
        }
        if let Some(look) = &self.look {
            look.draw_glass(pass, &self.renderer, second);
        }
        for look in self.rival_looks.iter().flatten() {
            look.draw_glass(pass, &self.renderer, second);
        }
        if let (true, Some(look), Some(pipeline)) = (self.ghost_shown, &self.ghost_look, self.ghost.as_ref().and_then(Ghost::drawing)) {
            look.draw_all_with(pass, &self.renderer, pipeline);
        }
    }

    /// How a car other than the player's is shown (a rival, the ghost): lit by the floodlights
    /// where it is, its mirrors with the road behind it, the rain on it.
    fn showing(&self, car: &sim_core::Car, inside: bool, lights: bool, daylight: f32, dt: f32, night: f32) -> look::Showing {
        let towers = self.track.towers();
        let here = DVec3::new(car.body.pos.x, car.body.pos.y, -car.body.pos.z);
        let flood = world_render::track::floodlight(towers, here) * night;
        let brightest = towers.iter().map(|t| (t.power / (t.lamp - here).length_squared(), t)).max_by(|a, b| a.0.total_cmp(&b.0));
        let flood_from = brightest.map_or(glam::Vec3::Y, |(_, t)| to_render(DVec3::new(t.lamp.x - here.x, t.lamp.y - here.y, here.z - t.lamp.z).normalize_or(DVec3::NEG_Z)));
        let road = self.circuit.locate(car.body.pos.x, car.body.pos.y).map_or([0.0, 0.0, 0.0, 0.0], |at| {
            let s = self.circuit.sample(at.along);
            [at.across as f32, s.heading as f32, s.curvature as f32, (s.width / 2.0) as f32]
        });
        look::Showing { inside, lights, daylight, dt, flood, flood_from, cloud_cover: self.renderer.cloud_cover as f32, road, wet: self.wet, time: self.clock as f32 }
    }
}

impl Game for Drive {
    fn samples(&self) -> u32 {
        self.renderer.quality().samples
    }

    fn update(&mut self, input: &Input, dt: f64, gpu: &Gpu) {
        self.back();
        self.keys(input);
        let dt = dt.min(0.1);
        self.clock += dt;
        if let Some((_, left)) = &mut self.message {
            *left -= dt;
            if *left <= 0.0 {
                self.message = None;
            }
        }
        // The player's hands on the car, once a frame (a shift asked for is taken once).
        let speed = self.world.vehicle(self.id).body().vel.x;
        if self.playback.is_some() {
            self.replaying(dt);
        } else if self.driver.is_none() && self.menu.is_none() {
            if let Some(car) = self.world.vehicle_mut(self.id).car_mut() {
                self.driving.car(car.p.wheelbase(), car.p.steering.lock_deg.to_radians());
                let (controls, asked) = self.driving.update(input, dt, speed, car.gear(), &Feet::of(car), &car.controls);
                car.controls = controls;
                if asked.camera {
                    self.cameras.view = self.cameras.view.next();
                    self.cameras.reset();
                }
                if asked.restart {
                    self.recover();
                }
            }
        }
        if !self.paused && self.playback.is_none() {
            self.lag += dt;
            while self.lag >= World::DT {
                self.step();
                self.lag -= World::DT;
            }
        }

        if self.playback.is_none() {
            self.ahead();
        }

        // The water on the circuit, as it is drawn.
        self.wet = self.world.water.first().map_or(0.0, |w| w.standing as f32);
        self.track.wet = self.wet;
        let watched = self.world.vehicle(self.watched()).car().expect("a car");
        // The cameras stand still while the world does (paused, a replay stopped): the chase
        // camera follows the car's velocity, which a car frozen in place still has.
        let still = self.paused || self.playback.as_ref().is_some_and(|p| p.paused);
        let view = self.cameras.view_point(watched, &self.circuit, if still { 0.0 } else { dt });
        let car = self.world.vehicle(self.id).car().expect("the player's car");
        let eye = view.position;
        self.view = Some(view);
        self.aspect = gpu.width as f32 / gpu.height.max(1) as f32;
        let sun = world_render::sun(self.hour);
        let lights = sun.day < 0.35;
        self.lamps.clear();
        if let Some(look) = &self.look {
            if lights {
                look.lamps(car, eye, &mut self.lamps);
            }
        }
        // The floodlights come on at dusk (as `track.wgsl` has them).
        let night = 1.0 - ((world_render::sun_elevation(self.hour).sin() as f32 + 0.10) / 0.12).clamp(0.0, 1.0);
        let night = night * night * (3.0 - 2.0 * night);
        // The circuit lights its own ground; the two towers nearest the car are also the world's
        // lamps, so that the land and the trees beyond the circuit have a little of their light
        // (and their glare shows): each fades in as the car comes within 220 m of it.
        if night > 0.0 {
            let towers = self.track.towers();
            let mut near: Vec<(f64, &world_render::track::Tower)> = towers.iter().map(|t| ((DVec3::new(t.lamp.x, t.lamp.y, -t.lamp.z) - car.body.pos).length(), t)).collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (distance, t) in near.into_iter().take(2) {
                let fade = (1.0 - ((distance - 130.0) / 90.0).clamp(0.0, 1.0)) as f32 * night;
                if fade <= 0.0 {
                    continue;
                }
                // Its lamps are turned to the middle of the track.
                let (n, e, h) = self.circuit.point(t.along, 0.0);
                let lamp = DVec3::new(t.lamp.x, t.lamp.y, -t.lamp.z);
                let axis = to_render((DVec3::new(n, e, -h) - lamp).normalize_or(DVec3::Z));
                let candelas = (world_render::track::TOWER_LIGHT * t.power) as f32 * std::f32::consts::PI / world_render::light_gain(self.hour).max(1e-9);
                self.lamps.push(Lamp { at: to_render(lamp - eye), color: sim_core::lights::Color::Flood, intensity: candelas * 0.4 * fade, omni: candelas * 0.16 * fade, beam: Some((axis, 0.34, 0.34)), radius: 0.5 });
            }
        }
        // The car's own lamps on the circuit's ground: its dipped beams, and the red of its tail
        // lamps behind it (more of it braking).
        let braking = car.controls.brake > 0.05;
        self.track.hour = self.hour;
        self.track.beams = if lights { 30_000.0 } else { 0.0 };
        self.track.tail = if braking { 300.0 } else if lights { 40.0 } else { 0.0 };
        let ground = self.world.terrain.height(car.body.pos.x, car.body.pos.y);
        let body = ShadowBody { position: car.body.pos, attitude: car.body.q, shadow: Some(look::shadow(car)) };
        // The car's shadow from its model, as it is now; its ShadowShape stays for its tyres'
        // contact with the road and for the lowest quality.
        // The rivals cast theirs the same way; the renderer keeps the nearest few.
        let draws = self.look.as_ref().map(|look| look.casters(car, eye)).unwrap_or_default();
        let rival_draws: Vec<(DVec3, f32, Vec<world_render::CasterDraw>)> = self
            .rivals
            .iter()
            .zip(&self.rival_looks)
            .filter_map(|(r, look)| {
                let (c, look) = (self.world.vehicle(r.id).car()?, look.as_ref()?);
                Some((c.body.pos, look.reach(), look.casters(c, eye)))
            })
            .collect();
        let mut casters: Vec<Caster> = Vec::new();
        if let Some(look) = &self.look {
            casters.push(Caster { centre: car.body.pos, radius: look.reach(), draws: &draws });
        }
        casters.extend(rival_draws.iter().map(|(centre, radius, draws)| Caster { centre: *centre, radius: *radius, draws }));
        let scene = Scene {
            view,
            hour: self.hour,
            time: self.world.time().max(self.clock),
            weather: &self.world.weather,
            height_above_ground: (-eye.z - ground).max(0.5),
            body: Some(body),
            casters: &casters,
            objects: &self.objects,
            glows: &[],
            lamps: &self.lamps,
            see_through: &self.see_through,
            wrecks: &self.world.wrecks,
        };
        self.renderer.prepare(gpu, &scene);
        self.track.prepare(gpu, &self.renderer, eye);
        // The game's own pipelines, on the world's shaders: one module for the car and the tyres' effects.
        let made_for = (self.renderer.quality(), gpu.sample_count);
        if self.made_for != Some(made_for) {
            let source = world_render::shader_source(&made_for.0) + include_str!("car.wgsl") + include_str!("effects.wgsl") + include_str!("ghost.wgsl") + include_str!("guide.wgsl");
            let shader = gpu.shader_with_ending_loops("drive", &source);
            if let Some(look) = &mut self.look {
                look.pipelines(gpu, &self.renderer, &shader);
            }
            self.effects.pipelines(gpu, &self.renderer, &shader);
            self.guide.pipeline(gpu, &self.renderer, &shader);
            for look in self.rival_looks.iter_mut().flatten() {
                look.pipelines(gpu, &self.renderer, &shader);
            }
            for fx in &mut self.rival_effects {
                fx.pipelines(gpu, &self.renderer, &shader);
            }
            if let Some(ghost) = &mut self.ghost {
                ghost.pipeline(gpu, &self.renderer, &shader);
            }
            self.made_for = Some(made_for);
        }
        // The eye goes with the car but beside the track.
        let eye_velocity = if self.cameras.view == View::Trackside || self.playback.is_some() { DVec3::ZERO } else { self.world.vehicle(self.watched()).body().world_velocity() };
        self.effects.prepare(gpu, eye, view.right, view.up, eye_velocity);
        // The driving line ahead of the player's car (not in a replay, nor watching another).
        if self.playback.is_none() && self.watch == 0 {
            self.guide.prepare(gpu, &self.circuit, car, eye);
        } else {
            self.guide.hide();
        }
        for fx in &mut self.rival_effects {
            fx.prepare(gpu, eye, view.right, view.up, eye_velocity);
        }
        if let Some(look) = &mut self.look {
            // The floodlights where the car is, and the tower that gives it most of them.
            let towers = self.track.towers();
            let here = DVec3::new(car.body.pos.x, car.body.pos.y, -car.body.pos.z);
            let flood = world_render::track::floodlight(towers, here) * night;
            let brightest = towers.iter().map(|t| (t.power / (t.lamp - here).length_squared(), t)).max_by(|a, b| a.0.total_cmp(&b.0));
            let flood_from = brightest.map_or(glam::Vec3::Y, |(_, t)| to_render(DVec3::new(t.lamp.x - here.x, t.lamp.y - here.y, here.z - t.lamp.z).normalize_or(DVec3::NEG_Z)));
            // Where the car is on the circuit, for what its mirrors show of the road behind:
            // metres across, the circuit's heading and curvature there, half its asphalt's width.
            let road = self.circuit.locate(car.body.pos.x, car.body.pos.y).map_or([0.0, 0.0, 0.0, 0.0], |at| {
                let s = self.circuit.sample(at.along);
                [at.across as f32, s.heading as f32, s.curvature as f32, (s.width / 2.0) as f32]
            });
            let showing = look::Showing { inside: self.cameras.view.inside(), lights, daylight: sun.day, dt: if self.paused { 0.0 } else { dt as f32 }, flood, flood_from, cloud_cover: self.renderer.cloud_cover as f32, road, wet: self.wet, time: self.clock as f32 };
            look.prepare(gpu, car, eye, showing);
        }
        // The rivals, each in its own paint, lit as the player's car is.
        for k in 0..self.rivals.len() {
            let Some(c) = self.world.vehicle(self.rivals[k].id).car() else { continue };
            let showing = self.showing(c, false, lights, sun.day, if self.paused { 0.0 } else { dt as f32 }, night);
            if let Some(look) = &mut self.rival_looks[k] {
                look.prepare(gpu, c, eye, showing);
                if lights {
                    look.lamps(c, eye, &mut self.lamps);
                }
            }
        }
        // The ghost, where the best lap was this far into it (not when it is on the player's own
        // car, nor in a replay).
        self.ghost_shown = false;
        if self.playback.is_none() {
            let t = self.timing.current(self.world.time());
            let b = self.world.vehicle(self.id).body();
            let located = self.circuit.locate(b.pos.x, b.pos.y);
            if let Some(ghost) = &mut self.ghost {
                self.ghost_delta = match (t, located) {
                    (Some(t), Some(at)) if t > 2.0 => ghost.delta(&self.circuit, self.circuit.from_start(at.along), t),
                    _ => None,
                };
                if ghost.place(t) && (ghost.shown.body.pos - b.pos).length() > 5.0 {
                    self.ghost_shown = true;
                }
            }
            if self.ghost_shown {
                let shown = self.ghost.as_ref().map(|g| g.shown.clone()).expect("a ghost");
                let showing = self.showing(&shown, false, false, sun.day, 0.0, night);
                if let Some(look) = &mut self.ghost_look {
                    look.prepare(gpu, &shown, eye, showing);
                }
            }
        }
        let listener = Listener::of(self.cameras.view, eye, view.right, car.body.pos, car.body.world_velocity(), eye_velocity);
        self.sound = Params::of(car, self.paused, listener, self.knock);
        self.knock = 0.0;
        self.audio.update(&self.sound);
    }

    fn ui(&mut self, ctx: &egui::Context) {
        if self.overlay {
            hud::draw(ctx, self);
        }
        if let Some(menu) = &mut self.menu {
            let choice = menu.show(ctx, &self.circuit.name);
            self.pause_choice(choice);
        }
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        self.renderer.draw_sky(pass);
        self.track.draw(pass);
        self.effects.draw_marks(pass);
        self.guide.draw(pass);
        for fx in &self.rival_effects {
            fx.draw_marks(pass);
        }
        if let Some(look) = &self.look {
            look.draw(pass);
        }
        for look in self.rival_looks.iter().flatten() {
            look.draw(pass);
        }
        self.renderer.draw(pass);
        // What is see-through last, over what is seen through it: smoke and dust, then the glass.
        // (When the wet road mirrors the scene, in the second pass, over its mirror.)
        if !self.track.mirrors() {
            self.see_through(pass, false);
        }
    }

    fn translucent(&self) -> bool {
        self.renderer.translucent() || self.track.mirrors()
    }

    /// A wet road mirrors what the first pass drew, and the drops on the glass bend it.
    fn reads_scene(&self) -> bool {
        self.track.mirrors()
    }

    fn before_translucent(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        self.renderer.before_translucent(encoder);
        if self.track.mirrors() {
            self.track.before_wet(gpu);
            if let Some(look) = &mut self.look {
                look.before_wet(gpu);
            }
        }
    }

    fn draw_translucent(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        self.renderer.draw_translucent(pass);
        if self.track.mirrors() {
            // The scene in the wet road, then what is see-through over it: spray, the glass.
            self.renderer.bind_frame(pass);
            self.track.draw_wet(pass);
            self.see_through(pass, true);
        }
    }

    /// Its recordings (F12) are Skylab's, in the same folder, whatever the window's title.
    fn clip_name(&self) -> Option<&str> {
        Some("Skylab")
    }

    fn post(&self) -> engine::Post {
        // From the seat the eye is set for the road outside, not for the dark cabin round it.
        let mut post = self.renderer.post();
        if self.cameras.view.inside() {
            post.adaptation *= 0.4;
        }
        // What the lens adds, when the quality has the bloom (it draws from it): darker corners,
        // the sun's flare, the depth of field of the trackside cameras.
        let Some(view) = self.view.filter(|_| post.bloom > 0.0 && !self.plain) else { return post };
        post.vignette = 0.22;
        post.blur_above = 0.2;
        // No blur of speed round the car: pulsing with the speed, it read as a zoom.
        match self.cameras.view {
            View::Chase | View::Bonnet => {}
            // From the seat the cabin fills half the picture and stands still: no blur.
            View::Cockpit => {}
            // A long lens on the car: what is before and behind it out of focus.
            View::Trackside => {
                post.focus = self.cameras.focus as f32;
                post.focus_span = 2.5 + post.focus * 0.03;
                post.focus_blur = 0.011;
                post.near = view.near;
            }
        }
        let sun = world_render::sun(self.hour);
        if sun.day > 0.2 {
            if let Some(at) = self.on_screen(&view, sun.direction) {
                post.flare = 0.9 * sun.day;
                post.flare_sun = at;
            }
        }
        post
    }
}

impl Drive {
    /// Where a direction from the eye (render axes) falls in the picture, 0...1 from its top
    /// left (it may be outside); `None` behind the eye.
    fn on_screen(&self, view: &world_render::ViewPoint, direction: glam::Vec3) -> Option<[f32; 2]> {
        let depth = direction.dot(view.forward);
        if depth <= 1e-4 {
            return None;
        }
        let tan = (view.fov_y * 0.5).tan();
        let x = direction.dot(view.right) / (depth * tan * self.aspect);
        let y = direction.dot(view.up) / (depth * tan) + view.shift_y;
        Some([0.5 + 0.5 * x, 0.5 - 0.5 * y])
    }
}
