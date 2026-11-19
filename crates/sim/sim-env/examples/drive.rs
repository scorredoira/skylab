//! The `drive` task, a car round Lakeside Park: runs competitors over many seeds, dry and wet, and
//! writes imitation data from the racing driver (`car::driver::Driver`) for training a network,
//! with DAgger.
//!
//! ```text
//! drive score <competitor>... [--episodes N] [--first-seed S] [--laps L] [--threads T] [--verbose]
//! drive data --out data.bin [--model net.fltmodel] [--beta B] [--episodes N] [--first-seed S] [--dt S] [--threads T]
//! drive fly <competitor> [--seed S] [--rain R] [--laps L]      one drive, a line a second
//! ```
//! A competitor is `driver` (the racing driver, deciding every simulation step), `held[@dt]` (the
//! racing driver deciding only every `dt` seconds, 0.05 by default, as a network does) or a model
//! file (`.fltmodel`, trained on `drive`).
//!
//! `score` drives every competitor from the grid for `L` laps (2 by default: the first from
//! rest, the second flying) on the same episodes: seeds `S..S+N` dry and the same seeds in rain of
//! 0.2 to 0.8 (drawn by the seed), the circuit's water as long rain leaves it. A success is every
//! lap driven with no crash and never beyond the envelope (15 m off the asphalt, the wrong way
//! round, standing still). The time of the second lap is the flying lap.
//!
//! `data` drives episodes as the training does (30 % from the grid, the rest on the line anywhere
//! round at 0.5 to 0.9 of its speed; every other one wet; one lap) by the racing driver, or (DAgger)
//! by a network the driver watches: every decision is recorded with the driver's direct action
//! (steering, and the pedal: the throttle less the brake). File: "FLTDATA1", u32 observation
//! length, u32 action length (2), then f32 records of observation and action.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sim_core::car::driver::Driver;
use sim_core::{CarControls, Controls, Pilot, Rng, Terrain};
use sim_env::env::observe;
use sim_env::model::Scratch;
use sim_env::{ActionConfig, ActionMode, Env, EnvConfig, Model, Reason, Task};

fn value<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1)?.parse().ok()
}

fn fail(message: &str) -> ! {
    eprintln!("drive: {message}");
    std::process::exit(2)
}

#[derive(Clone)]
enum Competitor {
    /// The racing driver deciding every simulation step (`None`) or every so many seconds.
    Driver(Option<f64>),
    Network(Arc<Model>),
}

impl Competitor {
    fn parse(spec: &str) -> Competitor {
        if spec == "driver" {
            return Competitor::Driver(None);
        }
        if let Some(rest) = spec.strip_prefix("held") {
            let dt = rest.strip_prefix('@').map_or(Some(0.05), |d| d.parse().ok()).unwrap_or_else(|| fail("held@<seconds>"));
            return Competitor::Driver(Some(dt));
        }
        let model = Model::load(spec).unwrap_or_else(|e| fail(&format!("{spec}: {e}")));
        if model.task != "drive" {
            fail(&format!("{spec} was trained for {}, not drive", model.task));
        }
        Competitor::Network(Arc::new(model))
    }
}

/// The conditions of an episode.
#[derive(Clone, Copy)]
struct Case {
    seed: u64,
    /// Rain (0 dry); `None`: drawn as the training draws it.
    rain: Option<f64>,
    /// Share of the starts on the grid (the rest on the line, at speed).
    grid: f64,
    laps: f64,
}

fn task(case: Case) -> Task {
    let mut task = Task::parse("drive").expect("drive exists");
    assert!(task.set("config.grid_starts", case.grid));
    assert!(task.set("config.laps", case.laps));
    assert!(task.set("config.time_limit", 160.0 * case.laps + 40.0));
    match case.rain {
        Some(r) if r > 0.0 => {
            assert!(task.set("config.wet_share", 1.0));
            assert!(task.set("config.rain_min", r));
            assert!(task.set("config.rain_max", r));
        }
        Some(_) => assert!(task.set("config.wet_share", 0.0)),
        None => assert!(task.set("config.wet_share", 0.5)),
    }
    task
}

const DIRECT: ActionConfig = ActionConfig { mode: ActionMode::Direct, flaps: false, trim: false };

fn env_for(case: Case, terrain: &Arc<Terrain>, dt: f64, action: ActionConfig) -> Env {
    Env::with_terrain(EnvConfig { task: task(case), action, decision_dt: dt, ..EnvConfig::default() }, terrain.clone())
}

/// The racing driver, for the car of the episode just begun.
fn driver(env: &Env) -> Driver {
    let circuit = env.guidance().and_then(|g| g.race_circuit()).cloned().expect("a drive's circuit");
    let car = env.vehicle().car().expect("a car").p.clone();
    Driver::new(circuit, &car)
}

/// The direct action that gives these controls.
fn direct_action(c: &CarControls) -> [f32; 2] {
    [c.steer as f32, (c.throttle - c.brake) as f32]
}

/// What the driver would do now, as a direct action, for a decision of `dt` seconds.
fn label(expert: &mut Driver, env: &Env, dt: f64) -> [f32; 2] {
    let obs = observe(env.world(), env.vehicle_id());
    let current = env.vehicle().dynamics.controls();
    match expert.fly(&obs, &current, dt) {
        Controls::Car(c) => direct_action(&c),
        _ => unreachable!("drive drives a car"),
    }
}

/// What one episode gave.
#[derive(Clone, Copy, Debug)]
struct Outcome {
    reason: Reason,
    /// Simulated seconds from the start to the end.
    time: f64,
    ret: f64,
    /// The laps' times (the first two), the rain.
    laps: [f64; 2],
    rain: f64,
}

/// Follows the laps of an episode by its progress (a lap: the circuit's length of it).
struct Laps {
    marks: Vec<f64>,
}

impl Laps {
    fn see(&mut self, env: &Env) {
        let d = env.targets().drive.unwrap_or_default();
        let done = (d.progress / d.length.max(1.0)).floor().max(0.0) as usize;
        while self.marks.len() < done + 1 {
            self.marks.push(env.world().time());
        }
    }

    fn times(&self) -> [f64; 2] {
        let lap = |k: usize| match (self.marks.get(k), self.marks.get(k + 1)) {
            (Some(a), Some(b)) => b - a,
            _ => f64::NAN,
        };
        [lap(0), lap(1)]
    }
}

fn run(competitor: &Competitor, case: Case, terrain: &Arc<Terrain>, mut log: Option<&mut dyn FnMut(&Env)>) -> Outcome {
    let (dt, action) = match competitor {
        Competitor::Driver(every) => (every.unwrap_or(0.05), DIRECT),
        Competitor::Network(m) => (m.decision_dt, m.action),
    };
    let mut env = env_for(case, terrain, dt, action);
    let mut step = env.reset(case.seed);
    let start = env.world().time();
    let mut laps = Laps { marks: vec![start] };
    let mut d = driver(&env);
    let mut scratch = Scratch::default();
    let mut act = vec![0.0f32; env.action_len()];
    let mut noise = Rng::new(case.seed ^ 0x6e6f_6973);
    while !(step.terminated || step.truncated) {
        step = match competitor {
            Competitor::Driver(None) => env.step_with_pilot(&mut d),
            Competitor::Driver(Some(dt)) => {
                let a = label(&mut d, &env, *dt);
                env.step(&a)
            }
            Competitor::Network(model) => {
                model.act(&step.observation, &mut scratch, &mut act);
                // Exploration's noise, as PPO samples its actions (`NOISE`, a standard deviation).
                if *NOISE.get().unwrap_or(&0.0) > 0.0 {
                    let only: Option<usize> = std::env::var("NOISE_ONLY").ok().and_then(|x| x.parse().ok());
                    for (k, a) in act.iter_mut().enumerate() {
                        let n = noise.normal() * NOISE.get().unwrap();
                        if only.is_none_or(|o| o == k) {
                            *a = (*a + n as f32).clamp(-1.0, 1.0);
                        }
                    }
                }
                env.step(&act)
            }
        };
        laps.see(&env);
        if let Some(log) = log.as_deref_mut() {
            log(&env);
        }
    }
    let _ = start;
    Outcome { reason: step.info.reason, time: f64::from(step.info.time), ret: f64::from(step.info.episode_return), laps: laps.times(), rain: env.world().weather.rain }
}

/// Runs `f` over `cases` on `threads` threads; the results come back in the order of `cases`.
fn par<T: Send, F: Fn(Case, &Arc<Terrain>) -> T + Sync>(cases: &[Case], terrain: &Arc<Terrain>, threads: usize, f: F) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<T>>> = Mutex::new((0..cases.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            let terrain = terrain.clone();
            let (next, results, f) = (&next, &results, &f);
            s.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= cases.len() {
                    break;
                }
                let o = f(cases[i], &terrain);
                results.lock().expect("not poisoned")[i] = Some(o);
            });
        }
    });
    results.into_inner().expect("not poisoned").into_iter().map(|o| o.expect("every case ran")).collect()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.retain(|x| x.is_finite());
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

fn print_summary(label: &str, outcomes: &[Outcome]) {
    let ok: Vec<&Outcome> = outcomes.iter().filter(|o| o.reason == Reason::Success).collect();
    let mut reasons = [0usize; 6];
    for o in outcomes {
        reasons[o.reason as usize] += 1;
    }
    let others: Vec<String> = Reason::NAMES.iter().zip(reasons).filter(|(n, c)| *c > 0 && **n != "success").map(|(n, c)| format!("{n} {c}")).collect();
    let best = ok.iter().map(|o| o.laps[1]).filter(|x| x.is_finite()).fold(f64::NAN, f64::min);
    println!(
        "  {label:<5} success {:>5.1}% ({}/{})  race median {:>6.1} s  laps (median) {:>6.2} + {:>6.2} s, best flying {:>6.2} s  {}",
        100.0 * ok.len() as f64 / outcomes.len().max(1) as f64,
        ok.len(),
        outcomes.len(),
        median(ok.iter().map(|o| o.time).collect()),
        median(ok.iter().map(|o| o.laps[0]).collect()),
        median(ok.iter().map(|o| o.laps[1]).collect()),
        best,
        others.join(", ")
    );
}

fn score(args: &[String], terrain: &Arc<Terrain>, threads: usize) {
    let episodes: u64 = value(args, "--episodes").unwrap_or(50);
    let first: u64 = value(args, "--first-seed").unwrap_or(1000);
    let laps: f64 = value(args, "--laps").unwrap_or(2.0);
    let specs: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--"))).map(|(_, a)| a).collect();
    if specs.is_empty() {
        fail("score needs at least one competitor");
    }
    let mut cases = Vec::new();
    for wet in [false, true] {
        for seed in first..first + episodes {
            // The rain of a wet case, from its seed: 0.2 to 0.8.
            let rain = if wet { 0.2 + 0.6 * Rng::new(seed ^ 0x7261_696e).uniform() } else { 0.0 };
            cases.push(Case { seed, rain: Some(rain), grid: 1.0, laps });
        }
    }
    for spec in specs {
        let competitor = Competitor::parse(spec);
        let started = std::time::Instant::now();
        let outcomes = par(&cases, terrain, threads, |c, t| run(&competitor, c, t, None));
        let n = episodes as usize;
        println!("{spec}: {} drives of {laps} laps from the grid, seeds {first}..{} ({:.0} s)", cases.len(), first + episodes - 1, started.elapsed().as_secs_f64());
        print_summary("dry", &outcomes[..n]);
        print_summary("wet", &outcomes[n..]);
        print_summary("all", &outcomes);
        if args.iter().any(|a| a == "--verbose") {
            for (c, o) in cases.iter().zip(&outcomes) {
                println!("    seed {:>5} rain {:.2} {:<16} {:>6.1} s  laps {:>6.2} {:>6.2}  return {:>7.1}", c.seed, o.rain, o.reason.name(), o.time, o.laps[0], o.laps[1], o.ret);
            }
        }
    }
}

/// Imitation data: see the module.
fn data(args: &[String], terrain: &Arc<Terrain>, threads: usize) {
    let out: String = value(args, "--out").unwrap_or_else(|| fail("data needs --out"));
    let episodes: u64 = value(args, "--episodes").unwrap_or(200);
    let first: u64 = value(args, "--first-seed").unwrap_or(100_000);
    let beta: f64 = value(args, "--beta").unwrap_or(1.0);
    let dt: f64 = value(args, "--dt").unwrap_or(0.05);
    let model = value::<String>(args, "--model").map(|m| Arc::new(Model::load(&m).unwrap_or_else(|e| fail(&e))));
    if let Some(m) = &model {
        if m.action.mode != ActionMode::Direct || (m.decision_dt - dt).abs() > 1e-9 || m.task != "drive" {
            fail("the model must be a drive model with direct actions and the same --dt");
        }
    }
    let cases: Vec<Case> = (first..first + episodes).map(|seed| Case { seed, rain: None, grid: 0.3, laps: 1.0 }).collect();
    let results = par(&cases, terrain, threads, |case, terrain| {
        let mut env = env_for(case, terrain, dt, DIRECT);
        let mut step = env.reset(case.seed);
        let mut expert = driver(&env);
        let mut rng = Rng::new(case.seed ^ 0x5eed);
        let mut scratch = Scratch::default();
        let mut net = [0.0f32; 2];
        let mut mine = Vec::new();
        while !(step.terminated || step.truncated) {
            let label = label(&mut expert, &env, dt);
            mine.extend_from_slice(&step.observation);
            mine.extend_from_slice(&label);
            let act: &[f32] = match &model {
                Some(m) if rng.uniform() >= beta => {
                    m.act(&step.observation, &mut scratch, &mut net);
                    &net
                }
                _ => &label,
            };
            step = env.step(act);
        }
        let o = Outcome { reason: step.info.reason, time: f64::from(step.info.time), ret: f64::from(step.info.episode_return), laps: [f64::NAN; 2], rain: env.world().weather.rain };
        (o, mine)
    });
    let obs_len = env_for(cases[0], terrain, dt, DIRECT).observation_len();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"FLTDATA1");
    bytes.extend_from_slice(&(obs_len as u32).to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let mut outcomes = Vec::new();
    let mut count = 0;
    for (o, records) in results {
        outcomes.push(o);
        count += records.len();
        for x in &records {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
    }
    std::fs::write(&out, bytes).expect("could not write the data");
    println!("{} decisions written to {out}; the drives:", count / (obs_len + 2));
    print_summary("all", &outcomes);
}

/// One drive, a line a second.
fn fly(args: &[String], terrain: &Arc<Terrain>) {
    let Some(spec) = args.first() else { fail("fly <competitor>") };
    let competitor = Competitor::parse(spec);
    let case = Case { seed: value(args, "--seed").unwrap_or(1), rain: Some(value(args, "--rain").unwrap_or(0.0)), grid: 1.0, laps: value(args, "--laps").unwrap_or(2.0) };
    let mut last = -1.0;
    let mut log = |env: &Env| {
        let t = env.world().time();
        if t - last >= 1.0 {
            last = t;
            let d = env.targets().drive.unwrap_or_default();
            let car = env.vehicle().car().unwrap();
            println!(
                "t {:>6.1}  progress {:>7.1} m  speed {:>5.1} km/h  across {:>5.1} m  gear {}  steer {:>5.2} throttle {:.2} brake {:.2}  water {:.2} mm",
                t,
                d.progress,
                car.data().forward * 3.6,
                d.across,
                car.gear(),
                car.controls.steer,
                car.controls.throttle,
                car.controls.brake,
                car.wheels().iter().map(|w| w.water).sum::<f64>() / 4.0 * 1000.0
            );
        }
        if let Some(why) = env.vehicle().dynamics.crashed() {
            println!("crashed: {why}");
        }
    };
    let o = run(&competitor, case, terrain, Some(&mut log));
    println!("{}: {:.1} s, laps {:.2} {:.2}, rain {:.2}, return {:.1}", o.reason.name(), o.time, o.laps[0], o.laps[1], o.rain, o.ret);
}

/// Noise added to a network's actions (`--noise <std>`).
static NOISE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(n) = value::<f64>(&args, "--noise") {
        NOISE.set(n).expect("set once");
    }
    let threads: usize = value(&args, "--threads").unwrap_or(8);
    let file = sim_core::circuit::CircuitFile::find("lakeside").expect("the built-in circuit");
    let terrain = Arc::new(EnvConfig { task: Task::parse("drive").expect("drive"), world_seed: file.seed(), ..EnvConfig::default() }.terrain());
    let rest = &args[1.min(args.len())..];
    match args.first().map(String::as_str) {
        Some("score") => score(rest, &terrain, threads),
        Some("data") => data(rest, &terrain, threads),
        Some("fly") => fly(rest, &terrain),
        _ => fail("drive score|data|fly ... (see the comment at the top of examples/drive.rs)"),
    }
}
