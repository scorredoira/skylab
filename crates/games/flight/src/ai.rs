//! Aircraft flown by trained models (`--model <file>[:<task>]`, or a `.pattern` file: the autopilot
//! flying the race with that circuit): where each one starts for its
//! task, the referee that tells when its episode would have ended, and the restart a few seconds
//! later so it keeps demonstrating.

use sim_core::{euler_angles, AircraftKind, Autopilot, Goal, Pattern, Start, VehicleId, World, KNOTS};
use sim_env::{EnvConfig, Guidance, Model, ModelPilot, Reason, Referee, Runway, Task};

/// Seconds an aircraft goes on after its episode has ended, before it starts again.
pub const RESTART_DELAY: f64 = 4.0;
/// Seconds flown hands-off after each start, as the environment does, so the instruments settle.
const SETTLE: f64 = 0.5;
/// A `landing` model starts this far before the aiming point (training: 1.5 to 4 km).
pub const LANDING_DISTANCE: f64 = 2500.0;
/// A runway start closer than this to another aircraft takes the other end of the runway.
const CLEAR_OF: f64 = 150.0;

/// What flies a `ModelFlier`: a trained network, or the autopilot with a circuit of its own (a
/// `.pattern` file, as `sim-env`'s `race search` writes; it flies the `race`).
enum Brain {
    Network(Model),
    Autopilot(Pattern),
}

enum Pilot {
    Network(ModelPilot),
    Autopilot(Box<Autopilot>),
}

/// Seconds between the referee's judgements for the autopilot (a network's are its decisions).
const AUTOPILOT_JUDGE_DT: f64 = 0.1;

/// One aircraft flown by a model.
pub struct ModelFlier {
    brain: Brain,
    task: Task,
    /// The file name, for the status line.
    pub name: String,
    start: Start,
    guidance: Guidance,
    pilot: Pilot,
    referee: Option<Referee>,
    started_at: f64,
    ended_at: Option<f64>,
    /// How its last episode ended.
    pub last_ending: Option<Reason>,
    /// Episodes ended so far.
    pub episodes: u32,
    /// Seconds its last episode lasted, from the end of the settle to its ending.
    pub last_time: Option<f64>,
}

/// `file` or `file:task`: the model file and the task to fly it in (the model's own by default).
pub fn parse_spec(spec: &str) -> (&str, Option<&str>) {
    match spec.rsplit_once(':') {
        Some((file, task)) if Task::parse(task).is_some() => (file, Some(task)),
        _ => (spec, None),
    }
}

/// The task's name as the player reads it.
pub fn task_text(task: &str) -> &'static str {
    match task {
        "hold" => "hold heading and altitude",
        "takeoff" => "takeoff",
        "landing" => "landing",
        "circuit" => "circuit",
        "race" => "race: take off, turn round and land",
        _ => "?",
    }
}

pub fn ending_text(reason: Reason) -> &'static str {
    match reason {
        Reason::Running => "in progress",
        Reason::Success => "success",
        Reason::Crashed => "crash",
        Reason::OutOfEnvelope => "out of limits",
        Reason::Failed => "hard landing",
        Reason::TimeLimit => "time limit",
    }
}

impl ModelFlier {
    /// Reads the model of `spec` and adds its aircraft to the world at the start its task calls
    /// for at the home airport (the one the tasks are trained at), into the wind: `takeoff`,
    /// `circuit` and `race` on the runway, `landing` on final, `hold` in the air beside it. `flying` are the
    /// tasks of the models flying already, to keep those of the same task apart.
    pub fn add(spec: &str, world: &mut World, flying: &[&str]) -> Result<(VehicleId, ModelFlier), String> {
        let (file, task_name) = parse_spec(spec);
        let brain = if file.ends_with(".pattern") {
            let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
            Brain::Autopilot(Pattern::from_text(&text).map_err(|e| format!("{file}: {e}"))?)
        } else {
            Brain::Network(Model::load(file)?)
        };
        let own = match &brain {
            Brain::Network(m) => m.task.as_str(),
            Brain::Autopilot(_) => "race",
        };
        let task_name = task_name.unwrap_or(own).to_string();
        if matches!(brain, Brain::Autopilot(_)) && task_name != "race" {
            return Err(format!("{file}: an autopilot pattern flies the race only"));
        }
        let task = Task::parse(&task_name).ok_or_else(|| format!("{file}: unknown task {task_name}"))?;
        let home = EnvConfig::home(&world.terrain).ok_or("this world has no airport")?;
        let into = world.weather.wind_from * sim_core::DEG;
        let reversed = (into - home.heading).cos() < 0.0;
        let k = flying.iter().filter(|t| **t == task_name).count() as f64;
        let start = match task_name.as_str() {
            "landing" => Start::Final { airport: home, reversed, distance: (LANDING_DISTANCE + 700.0 * k).min(4000.0) },
            "hold" => {
                // Beside the airport, 2 km to the left of the runway and apart from each other.
                let course = home.heading + if reversed { std::f64::consts::PI } else { 0.0 };
                let (c, s) = (course.cos(), course.sin());
                let (along, right) = (-1500.0 * k, -2000.0 - 600.0 * k);
                Start::Air { n: home.n + c * along - s * right, e: home.e + s * along + c * right, heading: course }
            }
            _ => Start::Runway { airport: home, reversed },
        };
        let id = world.add_aircraft(AircraftKind::C172, start);
        // A takeoff does not start on top of another aircraft: then it takes the other end (if
        // both are taken, the one into the wind).
        let mut start = start;
        if let Start::Runway { airport, reversed } = start {
            let clear = |world: &World| {
                let at = world.vehicle(id).body().pos;
                world.vehicles.iter().enumerate().all(|(i, p)| i == id.0 || (p.body().pos - at).length() >= CLEAR_OF)
            };
            if !clear(world) {
                world.reset(id, Start::Runway { airport, reversed: !reversed });
                if clear(world) {
                    start = Start::Runway { airport, reversed: !reversed };
                }
            }
        }
        let name = std::path::Path::new(file).file_name().map_or(file.to_string(), |n| n.to_string_lossy().into_owned());
        if let Brain::Network(m) = &brain {
            ModelPilot::new(m.clone())?;
        }
        let mut flier = ModelFlier {
            brain,
            task,
            name,
            start,
            guidance: Guidance::hold(0.0, 0.0, 0.0),
            pilot: Pilot::Autopilot(Box::new(Autopilot::new(Goal::Hold { heading: None, vertical: sim_core::Vertical::Speed(0.0), airspeed: None }))),
            referee: None,
            started_at: 0.0,
            ended_at: None,
            last_ending: None,
            episodes: 0,
            last_time: None,
        };
        flier.restart(world, id);
        Ok((id, flier))
    }

    pub fn task_name(&self) -> &'static str {
        self.task.name()
    }

    /// Puts the aircraft back at its start with a fresh pilot.
    pub fn restart(&mut self, world: &mut World, id: VehicleId) {
        world.reset(id, self.start);
        let plane = world.vehicle(id);
        let runway = |start: &Start| match *start {
            Start::Runway { airport, reversed } | Start::Final { airport, reversed, .. } => Some(Runway { airport, reversed }),
            _ => None,
        };
        self.guidance = match (self.task.name(), runway(&self.start)) {
            ("takeoff", Some(r)) => Guidance::takeoff(r),
            ("landing", Some(r)) => Guidance::landing(r),
            ("circuit", Some(r)) => Guidance::circuit(r),
            ("race", Some(r)) => Guidance::race(r),
            // `hold`: the heading and altitude it starts at, at 95 knots.
            _ => Guidance::hold(euler_angles(plane.body().q).heading, -plane.body().pos.z, 95.0 * KNOTS),
        };
        self.pilot = match &self.brain {
            Brain::Network(m) => Pilot::Network(ModelPilot::new(m.clone()).expect("checked when loaded").with_guidance(self.guidance.clone())),
            Brain::Autopilot(pattern) => {
                let r = self.guidance.runway().expect("the race has a runway");
                let mut a = Autopilot::new(Goal::Circuit { airport: r.airport, reversed: r.reversed, repeat: false });
                a.pattern = pattern.clone();
                Pilot::Autopilot(Box::new(a))
            }
        };
        self.referee = None;
        self.started_at = world.time();
        self.ended_at = None;
    }

    /// Before each simulation step: the model moves the controls (after the hands-off settle).
    pub fn fly(&mut self, world: &mut World, id: VehicleId) {
        if world.time() - self.started_at < SETTLE - 1e-9 {
            return;
        }
        if self.referee.is_none() && self.ended_at.is_none() {
            let dt = match &self.brain {
                Brain::Network(m) => m.decision_dt,
                Brain::Autopilot(_) => AUTOPILOT_JUDGE_DT,
            };
            self.referee = Some(Referee::new(self.task, self.guidance.clone(), world, id, dt));
        }
        match &mut self.pilot {
            Pilot::Network(p) => world.fly(id, p),
            Pilot::Autopilot(p) => world.fly(id, p.as_mut()),
        }
    }

    /// After each simulation step: judges the episode. A `hold` that reaches the time limit
    /// simply goes on (its next episode starts there); any other ending restarts the aircraft
    /// `RESTART_DELAY` seconds later.
    pub fn after_step(&mut self, world: &mut World, id: VehicleId) {
        if let Some(referee) = &mut self.referee {
            let reason = referee.step(world, id);
            if reason != Reason::Running {
                self.last_ending = Some(reason);
                self.last_time = Some(world.time() - self.started_at - SETTLE);
                self.episodes += 1;
                self.referee = None;
                if !(reason == Reason::TimeLimit && self.task.name() == "hold") {
                    self.ended_at = Some(world.time());
                }
            }
        }
        if self.ended_at.is_some_and(|t| world.time() - t >= RESTART_DELAY) {
            self.restart(world, id);
        }
    }

    /// Seconds since its episode began (after the settle), while it runs.
    pub fn flying_for(&self, now: f64) -> Option<f64> {
        match (&self.referee, self.ended_at) {
            (Some(_), None) => Some(now - self.started_at - SETTLE),
            _ => None,
        }
    }

    /// Seconds until it restarts, if its episode has ended.
    pub fn restarting_in(&self, now: f64) -> Option<f64> {
        self.ended_at.map(|t| (t + RESTART_DELAY - now).max(0.0))
    }
}

