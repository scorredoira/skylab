//! Flies episodes headless and prints how they went.
//!
//! ```text
//! fly_model <model file> [--task T] [--episodes N] [--seed S] [--wind M] [--turbulence X]
//! fly_model --random     [--task T] ...          a policy of uniform random actions
//! fly_model --write-example <file>               writes the hand-made wings leveller
//! ```
//! With a model file the aircraft is flown by `ModelPilot` through `World::fly`, as the game
//! would, while an `Env` of the model's task (or `--task`) keeps the score.

use sim_core::Rng;
use sim_env::model::wings_leveller;
use sim_env::{Env, EnvConfig, Model, ModelPilot, Reason, Task};

fn value<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1)?.parse().ok()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(path) = value::<String>(&args, "--write-example") {
        wings_leveller()
            .save(&path)
            .expect("could not write the file");
        println!("wrote {path}");
        return;
    }
    let random = args.iter().any(|a| a == "--random");
    let model = if random {
        None
    } else {
        let Some(path) = args.first().filter(|a| !a.starts_with("--")) else {
            eprintln!("usage: fly_model <model file> | --random | --write-example <file>  [--task T] [--episodes N] [--seed S] [--wind M] [--turbulence X]");
            std::process::exit(2);
        };
        Some(Model::load(path).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        }))
    };
    let task_name: String = value(&args, "--task")
        .or_else(|| model.as_ref().map(|m| m.task.clone()))
        .unwrap_or_else(|| "hold".to_string());
    let mut task = Task::parse(&task_name).expect("task: hold, takeoff, landing or circuit");
    if let Some(wind) = value::<f64>(&args, "--wind") {
        task.air_mut().wind_max = wind;
    }
    if let Some(turbulence) = value::<f64>(&args, "--turbulence") {
        task.air_mut().turbulence_max = turbulence;
    }
    let episodes: u64 = value(&args, "--episodes").unwrap_or(1);
    let seed: u64 = value(&args, "--seed").unwrap_or(1);

    let mut config = EnvConfig {
        task,
        ..EnvConfig::default()
    };
    if let Some(m) = &model {
        config.action = m.action;
        config.decision_dt = m.decision_dt;
    }
    let mut env = Env::new(config);
    let mut rng = Rng::new(seed ^ 0xABCD);
    let mut action = vec![0.0f32; env.action_len()];
    let mut counts = [0u32; Reason::NAMES.len()];
    let (mut total, mut total_time) = (0.0f64, 0.0f64);
    for episode in 0..episodes {
        let mut step = env.reset(seed + episode);
        // The pilot is told what the episode asks for (the targets of `hold` are random).
        let mut pilot = model.as_ref().map(|m| {
            let guidance = env.guidance().expect("an episode is running").clone();
            ModelPilot::new(m.clone())
                .expect("checked when loaded")
                .with_guidance(guidance)
        });
        while !(step.terminated || step.truncated) {
            step = match &mut pilot {
                Some(pilot) => env.step_with_pilot(pilot),
                None => {
                    action
                        .iter_mut()
                        .for_each(|a| *a = (rng.uniform() * 2.0 - 1.0) as f32);
                    env.step(&action)
                }
            };
        }
        let info = step.info;
        counts[info.reason as usize] += 1;
        total += f64::from(info.episode_return);
        total_time += f64::from(info.time);
        if episodes <= 20 {
            let crash = env
                .vehicle()
                .aircraft().unwrap()
                .crashed()
                .map_or(String::new(), |c| format!(" ({c})"));
            let o = sim_env::env::observe(env.world(), env.vehicle_id());
            println!(
                "episode {episode}: seed {}, return {:8.2}, {:6.1} s, {}{crash}; ended at roll {:.0}, pitch {:.0} degrees, {:.0} m above the ground, {:.0} m/s",
                seed + episode,
                info.episode_return,
                info.time,
                info.reason.name(),
                o.roll.to_degrees(),
                o.pitch.to_degrees(),
                o.agl,
                o.aircraft().unwrap().indicated_airspeed
            );
        }
    }
    let n = episodes as f64;
    let ends: Vec<String> = Reason::NAMES
        .iter()
        .zip(counts)
        .filter(|(_, c)| *c > 0)
        .map(|(name, c)| format!("{name} {c}"))
        .collect();
    println!(
        "{task_name}, {}: {episodes} episodes, mean return {:.2}, mean length {:.1} s, success {:.0}% [{}]",
        if random { "random policy" } else { "model" },
        total / n,
        total_time / n,
        100.0 * f64::from(counts[Reason::Success as usize]) / n,
        ends.join(", ")
    );
}
