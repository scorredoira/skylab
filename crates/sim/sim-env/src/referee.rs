//! `Referee`: the score and the ending of a task for an aircraft flown somewhere else (in the
//! game, by a `ModelPilot` or anyone), judged exactly as the environment judges an episode.

use sim_core::{Controls, VehicleId, World};

use crate::env::observe;
use crate::guidance::{Guidance, Targets};
use crate::task::{Episode, Reason, Task, Tick};

/// Watches one aircraft of a `World` and tells when its episode of `task` would have ended.
/// Call `step` after every `World::step`; it judges every decision interval, as `Env` does.
pub struct Referee {
    task: Task,
    episode: Episode,
    substeps: usize,
    count: usize,
    dt: f64,
    before: Controls,
    was_on_ground: bool,
    touchdowns: Vec<(f64, bool)>,
    decisions: u32,
    total: f64,
    reason: Reason,
}

impl Referee {
    /// Starts an episode of `task` from where aircraft `id` is now, asked for what `guidance`
    /// says, with a decision every `decision_dt` seconds.
    pub fn new(
        task: Task,
        guidance: Guidance,
        world: &World,
        id: VehicleId,
        decision_dt: f64,
    ) -> Referee {
        let substeps = ((decision_dt / World::DT).round() as usize).max(1);
        let dt = substeps as f64 * World::DT;
        let mut episode = Episode::new(guidance, 0.0);
        let obs = observe(world, id);
        let targets = episode.guidance.update(&obs);
        let controls = world.vehicle(id).dynamics.controls();
        episode.begin(
            &task,
            &Tick {
                obs: &obs,
                targets: &targets,
                before: &controls,
                after: &controls,
                dt,
                touchdowns: &[],
            },
        );
        Referee {
            task,
            episode,
            substeps,
            count: 0,
            dt,
            before: controls,
            was_on_ground: obs.on_ground,
            touchdowns: Vec::new(),
            decisions: 0,
            total: 0.0,
            reason: Reason::Running,
        }
    }

    /// Call after every simulation step. Returns how the episode stands: `Running` until it
    /// ends, then the reason it ended, from then on.
    pub fn step(&mut self, world: &World, id: VehicleId) -> Reason {
        if self.reason != Reason::Running {
            return self.reason;
        }
        let vehicle = world.vehicle(id);
        let on_ground = vehicle.dynamics.on_ground();
        if let (true, false, Some(plane)) = (on_ground, self.was_on_ground, vehicle.aircraft()) {
            let pos = plane.state.pos;
            let runway = self.episode.runway;
            self.touchdowns.push((
                plane.last_touchdown().unwrap_or(0.0),
                runway.is_some_and(|r| r.on_runway(pos.x, pos.y)),
            ));
        }
        self.was_on_ground = on_ground;
        self.count += 1;
        // A crash is judged at once, as the environment does.
        if self.count < self.substeps && vehicle.dynamics.crashed().is_none() {
            return Reason::Running;
        }
        self.count = 0;
        let after = vehicle.dynamics.controls();
        let obs = observe(world, id);
        let targets: Targets = self.episode.guidance.update(&obs);
        let (reward, mut reason) = self.episode.judge(
            &self.task,
            &Tick {
                obs: &obs,
                targets: &targets,
                before: &self.before,
                after: &after,
                dt: self.dt,
                touchdowns: &self.touchdowns,
            },
        );
        self.touchdowns.clear();
        self.before = after;
        self.decisions += 1;
        self.total += reward;
        if reason == Reason::Running
            && f64::from(self.decisions) * self.dt >= self.task.time_limit() - 1e-6
        {
            reason = Reason::TimeLimit;
        }
        self.reason = reason;
        reason
    }

    /// How the episode stands.
    pub fn reason(&self) -> Reason {
        self.reason
    }

    /// The sum of the rewards so far.
    pub fn episode_return(&self) -> f64 {
        self.total
    }

    /// Simulated seconds judged so far.
    pub fn time(&self) -> f64 {
        f64::from(self.decisions) * self.dt
    }
}
