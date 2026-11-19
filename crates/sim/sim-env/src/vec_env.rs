//! Many environments stepped together on worker threads.
//!
//! The environments share one terrain. Each has its own chain of episode seeds: the seed given to
//! `reset`, then `next_seed` of it, and so on, so what an environment does is a pure function of
//! its first seed and the actions it is given, however many threads there are.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;


use crate::env::{Env, EnvConfig};
use crate::task::Reason;

/// Bit of `VecStep::flags`: the episode ended by itself.
pub const TERMINATED: u8 = 1;
/// Bit of `VecStep::flags`: the episode was cut at the time limit.
pub const TRUNCATED: u8 = 2;

/// The seed of the episode after the one with seed `seed` (splitmix64).
pub fn next_seed(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// An episode that ended during a step.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    /// Index of the environment.
    pub env: u32,
    pub reason: Reason,
    pub episode_return: f32,
    /// Decisions taken in the episode.
    pub steps: u32,
    /// The last observation of the episode (the batch holds the first of the next one).
    pub final_observation: Vec<f32>,
}

/// The result of stepping every environment once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VecStep {
    /// `num_envs * observation_len` values, environment by environment. For an environment whose
    /// episode ended, this is the first observation of its next episode.
    pub observations: Vec<f32>,
    pub rewards: Vec<f32>,
    /// `TERMINATED` and `TRUNCATED` bits, one byte per environment.
    pub flags: Vec<u8>,
    /// The episodes that ended in this step, in environment order.
    pub finished: Vec<Finished>,
}

enum Command {
    /// Seeds, and for each environment a first episode cut short to that many decisions (0: not cut).
    Reset(Vec<u64>, Vec<u32>),
    Step(Vec<f32>),
}

struct Worker {
    commands: Sender<Command>,
    results: Receiver<VecStep>,
    handle: Option<JoinHandle<()>>,
    /// Index of its first environment and how many it has.
    first: usize,
    count: usize,
}

fn work(mut envs: Vec<Env>, first: usize, commands: Receiver<Command>, results: Sender<VecStep>) {
    let obs_len = envs[0].observation_len();
    let act_len = envs[0].action_len();
    let mut seeds = vec![0u64; envs.len()];
    while let Ok(command) = commands.recv() {
        let mut out = VecStep {
            observations: Vec::with_capacity(envs.len() * obs_len),
            rewards: vec![0.0; envs.len()],
            flags: vec![0; envs.len()],
            finished: Vec::new(),
        };
        match command {
            Command::Reset(new, cuts) => {
                seeds = new;
                for ((env, seed), cut) in envs.iter_mut().zip(&seeds).zip(&cuts) {
                    env.reset(*seed);
                    if *cut > 0 {
                        env.cut_episode_at(*cut);
                    }
                    out.observations.extend_from_slice(env.observation());
                }
            }
            Command::Step(actions) => {
                for (i, env) in envs.iter_mut().enumerate() {
                    let step = env.step(&actions[i * act_len..(i + 1) * act_len]);
                    out.rewards[i] = step.reward;
                    if step.terminated || step.truncated {
                        out.flags[i] = if step.truncated {
                            TRUNCATED
                        } else {
                            TERMINATED
                        };
                        out.finished.push(Finished {
                            env: (first + i) as u32,
                            reason: step.info.reason,
                            episode_return: step.info.episode_return,
                            steps: step.info.episode_steps,
                            final_observation: step.observation,
                        });
                        seeds[i] = next_seed(seeds[i]);
                        env.reset(seeds[i]);
                    }
                    out.observations.extend_from_slice(env.observation());
                }
            }
        }
        if results.send(out).is_err() {
            break;
        }
    }
}

pub struct VecEnv {
    config: EnvConfig,
    num_envs: usize,
    workers: Vec<Worker>,
    out: VecStep,
    started: bool,
}

impl VecEnv {
    /// `num_envs` environments on `threads` worker threads (at most one per environment).
    ///
    /// Panics if the task needs a runway and the world has no airport (see `EnvConfig::validate`).
    pub fn new(config: EnvConfig, num_envs: usize, threads: usize) -> VecEnv {
        assert!(num_envs > 0, "a VecEnv needs at least one environment");
        let terrain = Arc::new(config.terrain());
        let threads = threads.clamp(1, num_envs);
        let mut workers = Vec::with_capacity(threads);
        let mut first = 0;
        for t in 0..threads {
            // Spread the environments as evenly as they go.
            let count = num_envs / threads + usize::from(t < num_envs % threads);
            let envs: Vec<Env> = (0..count)
                .map(|_| Env::with_terrain(config, terrain.clone()))
                .collect();
            let (command_tx, command_rx) = channel();
            let (result_tx, result_rx) = channel();
            let handle = std::thread::Builder::new()
                .name(format!("flight-env-{t}"))
                .spawn(move || work(envs, first, command_rx, result_tx))
                .expect("could not start a worker thread");
            workers.push(Worker {
                commands: command_tx,
                results: result_rx,
                handle: Some(handle),
                first,
                count,
            });
            first += count;
        }
        VecEnv {
            config,
            num_envs,
            workers,
            out: VecStep::default(),
            started: false,
        }
    }

    pub fn config(&self) -> &EnvConfig {
        &self.config
    }

    pub fn num_envs(&self) -> usize {
        self.num_envs
    }

    pub fn threads(&self) -> usize {
        self.workers.len()
    }

    pub fn observation_len(&self) -> usize {
        self.config.observation_len()
    }

    pub fn action_len(&self) -> usize {
        self.config.action_len()
    }

    /// Whether `reset` has been called.
    pub fn started(&self) -> bool {
        self.started
    }

    fn collect(&mut self) -> &VecStep {
        self.out = VecStep::default();
        for w in &self.workers {
            let part = w.results.recv().expect("a worker thread died");
            self.out.observations.extend(part.observations);
            self.out.rewards.extend(part.rewards);
            self.out.flags.extend(part.flags);
            self.out.finished.extend(part.finished);
        }
        &self.out
    }

    /// Starts a new episode in every environment, with one seed each. Returns the observations,
    /// `num_envs * observation_len` values.
    pub fn reset(&mut self, seeds: &[u64]) -> &[f32] {
        self.start(seeds, false)
    }

    /// Like `reset`, but the first episodes end at spread times: environment `i` of `n` has its
    /// first episode truncated after `max_steps * (i + 1) / n` decisions (`TimeLimit`). After
    /// that every episode is a whole one. Environments that all start together otherwise stay
    /// in step for as long as their episodes last, and a trainer sees every batch from the same
    /// moment of the episode.
    pub fn reset_staggered(&mut self, seeds: &[u64]) -> &[f32] {
        self.start(seeds, true)
    }

    fn start(&mut self, seeds: &[u64], stagger: bool) -> &[f32] {
        assert_eq!(seeds.len(), self.num_envs, "one seed per environment");
        let n = self.num_envs as u64;
        let max_steps = (self.config.task.time_limit()
            / (self.config.substeps() as f64 * sim_core::World::DT)
            - 1e-6)
            .ceil() as u64;
        let cuts: Vec<u32> = (0..n)
            .map(|i| {
                if stagger {
                    (max_steps * (i + 1) / n).max(1) as u32
                } else {
                    0
                }
            })
            .collect();
        for w in &self.workers {
            let part = seeds[w.first..w.first + w.count].to_vec();
            let cut = cuts[w.first..w.first + w.count].to_vec();
            w.commands
                .send(Command::Reset(part, cut))
                .expect("a worker thread died");
        }
        self.started = true;
        &self.collect().observations
    }

    /// Starts every environment with seeds `seed`, `seed + 1`, ...
    pub fn reset_from(&mut self, seed: u64) -> &[f32] {
        let seeds: Vec<u64> = (0..self.num_envs as u64)
            .map(|i| seed.wrapping_add(i))
            .collect();
        self.reset(&seeds)
    }

    /// Steps every environment with its action: `num_envs * action_len` values, environment by
    /// environment. An environment whose episode ends starts the next one by itself.
    pub fn step(&mut self, actions: &[f32]) -> &VecStep {
        let act_len = self.action_len();
        assert_eq!(
            actions.len(),
            self.num_envs * act_len,
            "one action per environment"
        );
        if !self.started {
            self.reset_from(0);
        }
        for w in &self.workers {
            let part = actions[w.first * act_len..(w.first + w.count) * act_len].to_vec();
            w.commands
                .send(Command::Step(part))
                .expect("a worker thread died");
        }
        self.collect()
    }
}

impl Drop for VecEnv {
    fn drop(&mut self) {
        for w in &mut self.workers {
            // Closing the channel ends the worker's loop.
            let (dead, _) = channel();
            w.commands = dead;
            if let Some(handle) = w.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

/// How many performance cores this machine has: what to run the environments on. On Apple
/// silicon the efficiency cores are left out; elsewhere it is every core.
pub fn performance_cores() -> usize {
    let all = std::thread::available_parallelism().map_or(1, |n| n.get());
    if cfg!(target_os = "macos") {
        let asked = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.perflevel0.physicalcpu"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<usize>().ok());
        if let Some(n) = asked.filter(|n| *n > 0) {
            return n.min(all);
        }
    }
    all
}
