//! One environment: a world with one vehicle (an aircraft, or a multirotor for the multirotor's
//! task), a task, and the gym-style `reset` / `step`.

use std::sync::Arc;

use sim_core::circuit::Circuit;
use sim_core::Pilot;
use sim_core::{
    euler_angles, Aircraft, AircraftKind, Airport, Car, CarKind, Controls, Dynamics, Multirotor, MultirotorKind, Observation, Rng, Start,
    Terrain, Vehicle, VehicleId, VehicleObservation, WindField, World,
};

use crate::action::{ActionConfig, ActionMapper};
use crate::guidance::{Guidance, Targets};
use crate::observation::{self, Inputs};
use crate::task::{Episode, Reason, Task, Tick};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvConfig {
    /// Seed of the terrain. Environments with the same one can share it.
    pub world_seed: u64,
    /// Seconds between decisions; the action is held in between. Rounded to whole simulation
    /// steps of 1/240 s.
    pub decision_dt: f64,
    pub action: ActionConfig,
    pub task: Task,
    /// What is flown in the aircraft's tasks.
    pub aircraft: AircraftKind,
    /// What is flown in the multirotor's tasks (`hover`, `gates`).
    pub multirotor: MultirotorKind,
    /// What is driven in the car's task (`drive`).
    pub car: CarKind,
}

impl Default for EnvConfig {
    fn default() -> EnvConfig {
        EnvConfig {
            world_seed: 1,
            decision_dt: 0.1,
            action: ActionConfig::default(),
            task: Task::parse("hold").expect("hold exists"),
            aircraft: AircraftKind::C172,
            multirotor: MultirotorKind::FPV5,
            car: CarKind::GT,
        }
    }
}

impl EnvConfig {
    /// The observation's table (`observation::fields`) and the action's length for its task.
    pub fn observation_len(&self) -> usize {
        observation::fields(&self.task).len()
    }

    pub fn action_len(&self) -> usize {
        self.action.len_for(&self.task)
    }

    /// Simulation steps per decision.
    pub fn substeps(&self) -> usize {
        ((self.decision_dt / World::DT).round() as usize).max(1)
    }

    /// The terrain of the world the task is in: the seed's, with the circuit of a `drive` laid on
    /// it (cut and filled to it).
    pub fn terrain(&self) -> Terrain {
        match self.task.circuit() {
            Some(file) => {
                let circuit = Arc::new(Circuit::new(file, &Terrain::new(self.world_seed)));
                Terrain::new(self.world_seed).with_circuits(vec![circuit])
            }
            None => Terrain::new(self.world_seed),
        }
    }

    /// The airport the tasks use: the one nearest to the origin.
    pub fn home(terrain: &Terrain) -> Option<Airport> {
        terrain.nearest_airport(0.0, 0.0)
    }

    /// Whether environments can be built with this configuration on this terrain.
    pub fn validate(&self, terrain: &Terrain) -> Result<(), String> {
        if !(self.decision_dt.is_finite()
            && self.decision_dt >= World::DT
            && self.decision_dt <= 10.0)
        {
            return Err(format!(
                "decision_dt must be between {:.5} and 10 seconds",
                World::DT
            ));
        }
        if let Task::Gates(s) = &self.task {
            let course = self.task.course().ok_or_else(|| format!("there is no course {} (there are {})", s.config.course, sim_core::course::file::courses().len()))?;
            if let sim_core::course::Place::Seed(seed) = course.place {
                if seed != terrain.seed() || terrain.region().is_some() {
                    return Err(format!("the course {} is in the world of seed {seed}, not {}", course.key, terrain.seed()));
                }
            }
        }
        if let Task::Drive(s) = &self.task {
            let file = self.task.circuit().ok_or_else(|| format!("there is no circuit {} (there are {})", s.config.circuit, sim_core::circuit::circuits().len()))?;
            if file.seed() != terrain.seed() || terrain.region().is_some() {
                return Err(format!("the circuit {} is in the world of seed {}, not {}", file.key, file.seed(), terrain.seed()));
            }
        }
        if self.task.needs_runway() && EnvConfig::home(terrain).is_none() {
            return Err(format!(
                "world {} has no airport near the origin, which the task {} needs",
                terrain.seed(),
                self.task.name()
            ));
        }
        Ok(())
    }
}

/// What came with a step besides the observation and the reward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Info {
    /// Why the episode ended, or `Running`.
    pub reason: Reason,
    /// Sum of the rewards of the episode so far.
    pub episode_return: f32,
    /// Decisions taken in the episode so far.
    pub episode_steps: u32,
    /// Simulated seconds since the first observation of the episode.
    pub time: f32,
}

impl Info {
    pub fn success(&self) -> bool {
        self.reason == Reason::Success
    }
}

/// The result of `reset` or `step`.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub observation: Vec<f32>,
    pub reward: f32,
    /// The episode ended by itself: success, crash, failure or out of the task's envelope.
    pub terminated: bool,
    /// The episode was cut at the time limit.
    pub truncated: bool,
    pub info: Info,
}

/// What `World::observe` returns for a world with one vehicle, without its search for the
/// nearest airport (the environment knows its runway) and without its allocations.
/// `tests/env.rs` checks that both give the same observation vector.
pub fn observe(world: &World, id: VehicleId) -> Observation {
    let vehicle = world.vehicle(id);
    let s = vehicle.body();
    let e = euler_angles(s.q);
    // Fields added to `Observation` later keep their default here.
    #[allow(clippy::needless_update)]
    Observation {
        time: world.time(),
        heading: e.heading,
        pitch: e.pitch,
        roll: e.roll,
        angular_rates: s.omega.to_array(),
        velocity: s.world_velocity().to_array(),
        speed: s.vel.length(),
        on_ground: vehicle.dynamics.on_ground(),
        agl: vehicle.agl(),
        crashed: vehicle.dynamics.crashed().map(str::to_string),
        north: s.pos.x,
        east: s.pos.y,
        altitude: -s.pos.z,
        nearest_airport: None,
        traffic: Vec::new(),
        vehicle: match &vehicle.dynamics {
            Dynamics::Aircraft(a) => VehicleObservation::Aircraft(a.observe()),
            Dynamics::Multirotor(m) => VehicleObservation::Multirotor(m.observe()),
            Dynamics::Car(c) => VehicleObservation::Car(sim_core::CarObservation { rain: world.weather.rain, water_ahead: world.water_ahead(&c.body), ..c.observe() }),
        },
        ..Observation::default()
    }
}

pub struct Env {
    config: EnvConfig,
    substeps: usize,
    world: World,
    id: VehicleId,
    /// A freshly built vehicle, copied at every reset so nothing carries over between episodes.
    template: Vehicle,
    home: Option<Airport>,
    mapper: ActionMapper,
    episode: Option<Episode>,
    targets: Targets,
    touchdowns: Vec<(f64, bool)>,
    info: Info,
    done: bool,
    last: Vec<f32>,
    /// A time limit for this episode only, in decisions, shorter than the task's.
    cut_at: Option<u32>,
}

const IDLE: Info = Info {
    reason: Reason::Running,
    episode_return: 0.0,
    episode_steps: 0,
    time: 0.0,
};

impl Env {
    /// An environment with its own terrain.
    ///
    /// Panics if the task needs a runway and the world has no airport (see `EnvConfig::validate`).
    pub fn new(config: EnvConfig) -> Env {
        Env::with_terrain(config, Arc::new(config.terrain()))
    }

    /// An environment on a terrain shared with others. The terrain's seed is the world seed.
    pub fn with_terrain(mut config: EnvConfig, terrain: Arc<Terrain>) -> Env {
        config.world_seed = terrain.seed();
        // A course raced in a quad of its own (`Course::quad`, Deepwood's racing quad) is flown in it.
        if let Some(kind) = config.task.course().and_then(|c| c.quad.as_deref()).and_then(MultirotorKind::parse) {
            config.multirotor = kind;
        }
        let home = EnvConfig::home(&terrain);
        assert!(
            home.is_some() || !config.task.needs_runway(),
            "world {} has no airport for the task {}",
            config.world_seed,
            config.task.name()
        );
        // The world on the terrain given (and the water on its circuits, if any).
        let mut world = World::with_terrain(config.world_seed, terrain);
        let start = Start::Air {
            n: home.map_or(0.0, |a| a.n),
            e: home.map_or(0.0, |a| a.e),
            heading: 0.0,
        };
        let (id, template) = if config.task.car() {
            (world.add_car(config.car, start), Vehicle::new(Dynamics::Car(Car::of(config.car)), 0))
        } else if config.task.multirotor() {
            (world.add_multirotor(config.multirotor, start), Vehicle::new(Dynamics::Multirotor(Multirotor::of(config.multirotor)), 0))
        } else {
            (world.add_aircraft(config.aircraft, start), Vehicle::new(Dynamics::Aircraft(Aircraft::of(config.aircraft)), 0))
        };
        Env {
            config,
            substeps: config.substeps(),
            template,
            world,
            id,
            home,
            mapper: ActionMapper::new(config.action),
            episode: None,
            targets: Targets::default(),
            touchdowns: Vec::new(),
            info: IDLE,
            done: false,
            last: vec![0.0; config.observation_len()],
            cut_at: None,
        }
    }

    pub fn config(&self) -> &EnvConfig {
        &self.config
    }

    /// Seconds of simulated time one `step` advances.
    pub fn decision_dt(&self) -> f64 {
        self.substeps as f64 * World::DT
    }

    pub fn observation_len(&self) -> usize {
        self.config.observation_len()
    }

    pub fn action_len(&self) -> usize {
        self.config.action_len()
    }

    /// The world, to look at.
    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn vehicle_id(&self) -> VehicleId {
        self.id
    }

    pub fn vehicle(&self) -> &Vehicle {
        self.world.vehicle(self.id)
    }

    /// The guidance of the current episode, as it is now.
    pub fn guidance(&self) -> Option<&Guidance> {
        self.episode.as_ref().map(|e| &e.guidance)
    }

    /// The targets the last observation was built with.
    pub fn targets(&self) -> &Targets {
        &self.targets
    }

    /// The last observation returned.
    pub fn observation(&self) -> &[f32] {
        &self.last
    }

    /// Starts a new episode. The same seed always gives the same episode.
    pub fn reset(&mut self, seed: u64) -> Step {
        let mut rng = Rng::new(seed);
        let mut vehicle = self.template.clone();
        vehicle.wind = WindField::new(rng.next());
        self.world.vehicles[self.id.0] = vehicle;
        let mut episode = Episode::start(
            &self.config.task,
            &mut self.world,
            self.id,
            &mut rng,
            self.home,
        );
        if let Some(aircraft) = self.template.aircraft() {
            episode.guidance.set_cg_height(aircraft.p.cg_height);
        }
        for _ in 0..(episode.settle / World::DT).round() as usize {
            self.world.step();
        }
        let controls = self.vehicle().dynamics.controls();
        if let Controls::Aircraft(c) = &controls {
            self.mapper.reset(c);
        }

        let obs = observe(&self.world, self.id);
        self.targets = episode.guidance.update(&obs);
        episode.begin(
            &self.config.task,
            &Tick {
                obs: &obs,
                targets: &self.targets,
                before: &controls,
                after: &controls,
                dt: self.decision_dt(),
                touchdowns: &[],
            },
        );
        observation::write_with(
            observation::fields(&self.config.task),
            &Inputs {
                obs: &obs,
                controls: &controls,
                targets: &self.targets,
            },
            &mut self.last,
        );
        self.episode = Some(episode);
        self.info = IDLE;
        self.done = false;
        self.cut_at = None;
        Step {
            observation: self.last.clone(),
            reward: 0.0,
            terminated: false,
            truncated: false,
            info: self.info,
        }
    }

    /// Truncates the current episode after `decisions` decisions in all, as if its time limit
    /// were that (it ends with `TimeLimit`). The next episode has the task's limit again. Used to
    /// spread the episodes of many environments that all started together.
    pub fn cut_episode_at(&mut self, decisions: u32) {
        self.cut_at = Some(decisions.max(1));
    }

    /// Applies an action (of length `action_len()`, each in [-1, 1]) and advances `decision_dt`.
    ///
    /// Before the first `reset` this starts an episode with seed 0. After an episode has ended it
    /// changes nothing and returns the last observation with no reward: call `reset`.
    pub fn step(&mut self, action: &[f32]) -> Step {
        self.advance(Some(action), None)
    }

    /// Like `step`, but the aircraft is flown by a `Pilot` through `World::fly`, as the game does
    /// it, once per simulation step. The environment only keeps the score.
    pub fn step_with_pilot(&mut self, pilot: &mut dyn Pilot) -> Step {
        self.advance(None, Some(pilot))
    }

    fn advance(&mut self, action: Option<&[f32]>, mut pilot: Option<&mut dyn Pilot>) -> Step {
        if self.episode.is_none() {
            self.reset(0);
        }
        let dt = self.decision_dt();
        if self.done {
            return Step {
                observation: self.last.clone(),
                reward: 0.0,
                terminated: self.info.reason != Reason::TimeLimit,
                truncated: self.info.reason == Reason::TimeLimit,
                info: self.info,
            };
        }
        let before = self.vehicle().dynamics.controls();
        if let Some(action) = action {
            let moved = match &before {
                Controls::Aircraft(c) => Controls::Aircraft(self.mapper.apply(action, c, dt)),
                Controls::Multirotor(c) => Controls::Multirotor(self.config.action.sticks(action, c, dt)),
                Controls::Car(c) => Controls::Car(self.config.action.pedals(action, c, dt)),
            };
            self.world.vehicle_mut(self.id).dynamics.set_controls(moved);
        }

        let runway = self.episode.as_ref().and_then(|e| e.runway);
        self.touchdowns.clear();
        let mut was_on_ground = self.vehicle().dynamics.on_ground();
        for _ in 0..self.substeps {
            if let Some(pilot) = pilot.as_deref_mut() {
                self.world.fly(self.id, pilot);
            }
            self.world.step();
            let vehicle = &self.world.vehicles[self.id.0];
            let on_ground = vehicle.dynamics.on_ground();
            // An aircraft's touchdowns are judged: how hard, and on the runway or not.
            if let (true, false, Some(plane)) = (on_ground, was_on_ground, vehicle.aircraft()) {
                let pos = plane.state.pos;
                self.touchdowns.push((
                    plane.last_touchdown().unwrap_or(0.0),
                    runway.is_some_and(|r| r.on_runway(pos.x, pos.y)),
                ));
            }
            was_on_ground = on_ground;
            if vehicle.dynamics.crashed().is_some() {
                break;
            }
        }

        let after = self.vehicle().dynamics.controls();
        let obs = observe(&self.world, self.id);
        let episode = self.episode.as_mut().expect("an episode is running");
        self.targets = episode.guidance.update(&obs);
        let (reward, mut reason) = episode.judge(
            &self.config.task,
            &Tick {
                obs: &obs,
                targets: &self.targets,
                before: &before,
                after: &after,
                dt,
                touchdowns: &self.touchdowns,
            },
        );
        observation::write_with(
            observation::fields(&self.config.task),
            &Inputs {
                obs: &obs,
                controls: &after,
                targets: &self.targets,
            },
            &mut self.last,
        );

        let reward = reward as f32;
        self.info.episode_steps += 1;
        self.info.episode_return += reward;
        self.info.time = (f64::from(self.info.episode_steps) * dt) as f32;
        if reason == Reason::Running
            && (f64::from(self.info.episode_steps) * dt >= self.config.task.time_limit() - 1e-6
                || self.cut_at.is_some_and(|n| self.info.episode_steps >= n))
        {
            reason = Reason::TimeLimit;
        }
        self.info.reason = reason;
        self.done = reason != Reason::Running;
        Step {
            observation: self.last.clone(),
            reward,
            terminated: self.done && reason != Reason::TimeLimit,
            truncated: reason == Reason::TimeLimit,
            info: self.info,
        }
    }
}
