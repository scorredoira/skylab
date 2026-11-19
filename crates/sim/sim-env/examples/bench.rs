//! Measures how fast the environment runs. Release build:
//! `cargo run --release -p sim-env --example bench -- [task] [seconds]`

use std::time::Instant;

use sim_core::Rng;
use sim_env::{performance_cores, Env, EnvConfig, Task, VecEnv};

fn random(rng: &mut Rng, out: &mut [f32]) {
    for a in out {
        *a = (rng.uniform() * 2.0 - 1.0) as f32;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().map_or("hold", String::as_str);
    let seconds: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let config = EnvConfig {
        task: Task::parse(task).expect("task: hold, takeoff, landing or circuit"),
        ..EnvConfig::default()
    };
    let cores = performance_cores();
    println!(
        "task {task}, decision every {:.3} s ({} simulation steps), random actions, {seconds} s per row",
        config.decision_dt,
        config.substeps()
    );
    println!(
        "{:<34}{:>14}{:>22}",
        "", "env steps/s", "simulated s / wall s"
    );

    let mut rng = Rng::new(7);
    let mut env = Env::new(config);
    let mut action = vec![0.0f32; env.action_len()];
    env.reset(1);
    let (mut steps, mut episode) = (0u64, 1u64);
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        for _ in 0..200 {
            random(&mut rng, &mut action);
            let s = env.step(&action);
            steps += 1;
            if s.terminated || s.truncated {
                episode += 1;
                env.reset(episode);
            }
        }
    }
    let rate = steps as f64 / start.elapsed().as_secs_f64();
    println!(
        "{:<34}{:>14.0}{:>22.0}",
        "single environment, one thread",
        rate,
        rate * env.decision_dt()
    );

    for (threads, per_thread) in [(1, 16), (cores, 16), (cores, 64)] {
        let n = threads * per_thread;
        let mut envs = VecEnv::new(config, n, threads);
        let mut actions = vec![0.0f32; n * envs.action_len()];
        envs.reset_from(1);
        let mut steps = 0u64;
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < seconds {
            random(&mut rng, &mut actions);
            envs.step(&actions);
            steps += n as u64;
        }
        let rate = steps as f64 / start.elapsed().as_secs_f64();
        println!(
            "{:<34}{:>14.0}{:>22.0}",
            format!("{n} environments, {threads} threads"),
            rate,
            rate * config.substeps() as f64 / 240.0
        );
    }
    println!("performance cores: {cores}");
}
