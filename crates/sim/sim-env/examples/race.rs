//! The `race`: from standstill on the runway, take off, turn back and land on the same runway
//! until stopped, in the least time. Runs competitors over many seeds and winds, searches for the
//! quickest autopilot circuit, and writes imitation data for training a network.
//!
//! ```text
//! race eval <competitor>... [--episodes N] [--threads T] [--verbose]
//! race search [--out pattern.txt] [--from pattern.txt] [--sigma S] [--generations G] [--population P] [--episodes N] [--threads T]
//! race data <expert pattern> --out data.bin [--model net.fltmodel] [--beta B] [--episodes N] [--dt S] [--threads T]
//! race fly <competitor> [--seed S] [--wind M]     one episode, a line a second
//! ```
//! A competitor is `autopilot` (the standard circuit of `Goal::Circuit`), `autopilot:<pattern
//! file>` (the same with another `Pattern`), `held:<pattern file>[@dt]` (that autopilot deciding
//! only every `dt` seconds, as a network does) or a model file (`.fltmodel`).
//!
//! Every competitor flies the same episodes: seeds 1..=N in calm air, and seeds 1..=N with wind
//! (up to 8 m/s from any direction, turbulence up to 0.3).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sim_core::{
    AircraftControls, Autopilot, Controls, Goal, Observation, Pattern, Pilot, Rng, Terrain, World, DEG, KNOTS,
};
use sim_env::model::Scratch;
use sim_env::{ActionConfig, ActionMode, Env, EnvConfig, Model, Reason, Task};

fn value<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1)?.parse().ok()
}

fn fail(message: &str) -> ! {
    eprintln!("race: {message}");
    std::process::exit(2)
}

// ---- Patterns: `Pattern::from_text` / `to_text`, angles in degrees, speeds in knots.

fn get(p: &Pattern, name: &str) -> f64 {
    p.get(name).unwrap_or_else(|| fail(&format!("no pattern field {name}")))
}

fn set(p: &mut Pattern, name: &str, x: f64) {
    if !p.set(name, x) {
        fail(&format!("no pattern field {name}"));
    }
}

fn pattern_text(p: &Pattern) -> String {
    p.to_text()
}

fn load_pattern(path: &str) -> Pattern {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    Pattern::from_text(&text).unwrap_or_else(|e| fail(&format!("{path}: {e}")))
}

// ---- Competitors

#[derive(Clone)]
enum Competitor {
    /// The autopilot deciding every simulation step (`None`) or every so many seconds.
    Autopilot(Pattern, Option<f64>),
    Network(Arc<Model>),
}

impl Competitor {
    fn parse(spec: &str) -> Competitor {
        if spec == "autopilot" {
            return Competitor::Autopilot(Pattern::default(), None);
        }
        if let Some(file) = spec.strip_prefix("autopilot:") {
            return Competitor::Autopilot(load_pattern(file), None);
        }
        if let Some(rest) = spec.strip_prefix("held:") {
            let (file, dt) = rest.split_once('@').unwrap_or((rest, "0.1"));
            let dt: f64 = dt.parse().unwrap_or_else(|_| fail("held:<file>@<seconds>"));
            let pattern = if file == "default" { Pattern::default() } else { load_pattern(file) };
            return Competitor::Autopilot(pattern, Some(dt));
        }
        let model = Model::load(spec).unwrap_or_else(|e| fail(&format!("{spec}: {e}")));
        if model.task != "race" {
            eprintln!("race: {spec} was trained for {}, flying it in the race anyway", model.task);
        }
        Competitor::Network(Arc::new(model))
    }
}

/// An autopilot that decides only every `every` simulation steps and holds its controls in
/// between, as a policy with a decision interval does.
struct Held {
    inner: Autopilot,
    every: usize,
    count: usize,
    last: Option<Controls>,
}

impl Pilot for Held {
    fn fly(&mut self, obs: &Observation, current: &Controls, _dt: f64) -> Controls {
        if self.count.is_multiple_of(self.every) || self.last.is_none() {
            self.last = Some(self.inner.fly(obs, current, self.every as f64 * World::DT));
        }
        self.count += 1;
        self.last.expect("set above")
    }
}

/// The conditions of an episode.
#[derive(Clone, Copy)]
struct Case {
    seed: u64,
    wind: bool,
}

fn race_task(wind: bool) -> Task {
    let mut task = Task::parse("race").expect("race exists");
    if wind {
        let air = task.air_mut();
        air.wind_max = 8.0;
        air.turbulence_max = 0.3;
        air.crosswind_angle = 180.0;
    }
    task
}

/// What one episode gave.
#[derive(Clone, Copy, Debug)]
struct Outcome {
    reason: Reason,
    /// Simulated seconds from the first observation to the end.
    time: f64,
    ret: f64,
}

fn goal(env: &Env) -> Goal {
    let runway = env
        .guidance()
        .and_then(|g| g.runway())
        .expect("the race has a runway");
    Goal::Circuit {
        airport: runway.airport,
        reversed: runway.reversed,
        repeat: false,
    }
}

fn autopilot(pattern: &Pattern, env: &Env) -> Autopilot {
    let mut a = Autopilot::new(goal(env));
    a.pattern = pattern.clone();
    a
}

fn env_for(competitor: &Competitor, case: Case, terrain: &Arc<Terrain>) -> Env {
    let mut config = EnvConfig {
        task: race_task(case.wind),
        ..EnvConfig::default()
    };
    if let Competitor::Network(m) = competitor {
        config.action = m.action;
        config.decision_dt = m.decision_dt;
    }
    Env::with_terrain(config, terrain.clone())
}

fn run(
    competitor: &Competitor,
    case: Case,
    terrain: &Arc<Terrain>,
    mut log: Option<&mut dyn FnMut(&Env)>,
) -> Outcome {
    let mut env = env_for(competitor, case, terrain);
    let mut step = env.reset(case.seed);
    match competitor {
        Competitor::Autopilot(pattern, every) => {
            let a = autopilot(pattern, &env);
            let mut pilot: Box<dyn Pilot> = match every {
                None => Box::new(a),
                Some(dt) => Box::new(Held {
                    inner: a,
                    every: ((dt / World::DT).round() as usize).max(1),
                    count: 0,
                    last: None,
                }),
            };
            while !(step.terminated || step.truncated) {
                step = env.step_with_pilot(pilot.as_mut());
                if let Some(log) = log.as_deref_mut() {
                    log(&env);
                }
            }
        }
        Competitor::Network(model) => {
            let mut scratch = Scratch::default();
            let mut action = vec![0.0f32; env.action_len()];
            while !(step.terminated || step.truncated) {
                model.act(&step.observation, &mut scratch, &mut action);
                step = env.step(&action);
                if let Some(log) = log.as_deref_mut() {
                    log(&env);
                }
            }
        }
    }
    Outcome {
        reason: step.info.reason,
        time: f64::from(step.info.time),
        ret: f64::from(step.info.episode_return),
    }
}

/// Runs `cases` on `threads` threads; the results come back in the order of `cases`.
fn run_all(competitor: &Competitor, cases: &[Case], terrain: &Arc<Terrain>, threads: usize) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(vec![None; cases.len()]);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            // A terrain per thread: its caches are behind a lock that threads would queue on.
            let terrain = Arc::new(Terrain::new(terrain.seed()));
            let (next, results) = (&next, &results);
            s.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= cases.len() {
                    break;
                }
                let o = run(competitor, cases[i], &terrain, None);
                results.lock().expect("not poisoned")[i] = Some(o);
            });
        }
    });
    results
        .into_inner()
        .expect("not poisoned")
        .into_iter()
        .map(|o| o.expect("every case ran"))
        .collect()
}

fn cases(episodes: u64, first_seed: u64) -> Vec<Case> {
    let mut v = Vec::new();
    for wind in [false, true] {
        for seed in first_seed..first_seed + episodes {
            v.push(Case { seed, wind });
        }
    }
    v
}

struct Summary {
    successes: usize,
    total: usize,
    best: f64,
    median: f64,
    mean: f64,
    reasons: [usize; 6],
}

fn summarize(outcomes: &[Outcome]) -> Summary {
    let mut times: Vec<f64> = outcomes
        .iter()
        .filter(|o| o.reason == Reason::Success)
        .map(|o| o.time)
        .collect();
    times.sort_by(f64::total_cmp);
    let mut reasons = [0; 6];
    for o in outcomes {
        reasons[o.reason as usize] += 1;
    }
    Summary {
        successes: times.len(),
        total: outcomes.len(),
        best: times.first().copied().unwrap_or(f64::NAN),
        median: times.get(times.len() / 2).copied().unwrap_or(f64::NAN),
        mean: if times.is_empty() { f64::NAN } else { times.iter().sum::<f64>() / times.len() as f64 },
        reasons,
    }
}

fn print_summary(label: &str, s: &Summary) {
    let others: Vec<String> = Reason::NAMES
        .iter()
        .zip(s.reasons)
        .filter(|(n, c)| *c > 0 && **n != "success")
        .map(|(n, c)| format!("{n} {c}"))
        .collect();
    println!(
        "  {label:<6} success {:>5.1}% ({}/{})  best {:>6.1} s  median {:>6.1} s  mean {:>6.1} s  {}",
        100.0 * s.successes as f64 / s.total.max(1) as f64,
        s.successes,
        s.total,
        s.best,
        s.median,
        s.mean,
        others.join(", ")
    );
}

/// What a search minimises: the mean time, a failed episode counting as `FAILED_TIME`.
const FAILED_TIME: f64 = 400.0;

fn score(outcomes: &[Outcome]) -> f64 {
    outcomes
        .iter()
        .map(|o| if o.reason == Reason::Success { o.time } else { FAILED_TIME })
        .sum::<f64>()
        / outcomes.len().max(1) as f64
}

// ---- Commands

fn eval(args: &[String], terrain: &Arc<Terrain>, threads: usize) {
    let episodes: u64 = value(args, "--episodes").unwrap_or(100);
    let first: u64 = value(args, "--first-seed").unwrap_or(1);
    let specs: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--")))
        .map(|(_, a)| a)
        .collect();
    if specs.is_empty() {
        fail("eval needs at least one competitor");
    }
    let cs = cases(episodes, first);
    for spec in specs {
        let competitor = Competitor::parse(spec);
        let outcomes = run_all(&competitor, &cs, terrain, threads);
        let n = episodes as usize;
        println!("{spec}: {} episodes, seeds {first}..{}", cs.len(), first + episodes - 1);
        print_summary("calm", &summarize(&outcomes[..n]));
        print_summary("wind", &summarize(&outcomes[n..]));
        print_summary("all", &summarize(&outcomes));
        if args.iter().any(|a| a == "--verbose") {
            for (c, o) in cs.iter().zip(&outcomes) {
                println!("    seed {:>4} {} {:<16} {:>6.1} s return {:>7.1}", c.seed, if c.wind { "wind" } else { "calm" }, o.reason.name(), o.time, o.ret);
            }
        }
    }
}

/// The searched fields with their range.
const RANGES: [(&str, f64, f64); 18] = [
    ("height", 125.0, 300.0),
    ("width", 250.0, 1500.0),
    ("final_length", 150.0, 2500.0),
    ("aim", 30.0, 300.0),
    ("glide", 3.0, 10.0),
    ("turn_height", 20.0, 200.0),
    ("upwind", -1500.0, 300.0),
    ("bank", 15.0, 45.0),
    ("rotate_speed", 48.0, 62.0),
    ("climb_speed", 60.0, 85.0),
    ("pattern_speed", 65.0, 110.0),
    ("descent_speed", 60.0, 100.0),
    ("base_speed", 58.0, 85.0),
    ("final_speed", 55.0, 72.0),
    ("landing_flaps", 0.6, 3.4),
    ("land_reversed", 0.0, 1.0),
    ("takeoff_flaps", -0.4, 1.4),
    ("flare_height", 2.5, 12.0),
];

fn normal(rng: &mut Rng) -> f64 {
    let (u, v) = (rng.uniform().max(1e-12), rng.uniform());
    (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
}

/// Cross-entropy search over the pattern, in coordinates scaled to [0, 1] per field.
fn search(args: &[String], terrain: &Arc<Terrain>, threads: usize) {
    let generations: usize = value(args, "--generations").unwrap_or(40);
    let population: usize = value(args, "--population").unwrap_or(64);
    let episodes: u64 = value(args, "--episodes").unwrap_or(16);
    let out: String = value(args, "--out").unwrap_or_else(|| "pattern.txt".to_string());
    let seed: u64 = value(args, "--seed").unwrap_or(7);
    let start = value::<String>(args, "--from").map_or(Pattern::default(), |f| load_pattern(&f));
    let dims = RANGES.len();
    let to_unit = |p: &Pattern| -> Vec<f64> {
        RANGES.iter().map(|(n, lo, hi)| ((get(p, n) - lo) / (hi - lo)).clamp(0.0, 1.0)).collect()
    };
    let from_unit = |u: &[f64]| -> Pattern {
        let mut p = Pattern::default();
        for ((n, lo, hi), x) in RANGES.iter().zip(u) {
            set(&mut p, n, lo + (hi - lo) * x.clamp(0.0, 1.0));
        }
        p
    };
    let mut mean = to_unit(&start);
    let mut sigma = vec![value(args, "--sigma").unwrap_or(0.25); dims];
    let mut rng = Rng::new(seed);
    let elite = (population / 8).max(2);
    let mut best: Option<(f64, Pattern)> = None;
    for gen in 0..generations {
        // Fresh episodes each generation, so the search does not fit a few seeds.
        let cs = cases(episodes / 2, 1000 + (gen as u64) * episodes);
        let mut scored: Vec<(f64, Vec<f64>)> = Vec::new();
        for k in 0..population {
            let u: Vec<f64> = if k == 0 {
                mean.clone()
            } else {
                mean.iter().zip(&sigma).map(|(m, s)| (m + s * normal(&mut rng)).clamp(0.0, 1.0)).collect()
            };
            let c = Competitor::Autopilot(from_unit(&u), None);
            let s = score(&run_all(&c, &cs, terrain, threads));
            scored.push((s, u));
        }
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        let elites = &scored[..elite];
        for d in 0..dims {
            let m = elites.iter().map(|e| e.1[d]).sum::<f64>() / elite as f64;
            let v = elites.iter().map(|e| (e.1[d] - m).powi(2)).sum::<f64>() / elite as f64;
            mean[d] = m;
            sigma[d] = v.sqrt().max(0.03);
        }
        let top = from_unit(&scored[0].1);
        // The best of a generation is checked again on the same larger set before it is kept.
        let check = cases(48, 50_000);
        let checked = score(&run_all(&Competitor::Autopilot(top.clone(), None), &check, terrain, threads));
        let m = score(&run_all(&Competitor::Autopilot(from_unit(&mean), None), &check, terrain, threads));
        let (cand, cand_score) = if m < checked { (from_unit(&mean), m) } else { (top, checked) };
        if best.as_ref().is_none_or(|b| cand_score < b.0) {
            best = Some((cand_score, cand.clone()));
            std::fs::write(&out, pattern_text(&cand)).expect("could not write the pattern");
        }
        println!(
            "gen {gen:>3}: generation best {:>6.1}, its check {checked:>6.1}, mean's check {m:>6.1}, kept {:>6.1}, sigma {:.3}",
            scored[0].0,
            best.as_ref().map_or(f64::NAN, |b| b.0),
            sigma.iter().sum::<f64>() / dims as f64
        );
    }
    if let Some((s, p)) = best {
        println!("best score {s:.1} written to {out}:\n{}", pattern_text(&p));
    }
}

/// The action of `ActionMode::Direct` with flaps and trim that gives these controls.
fn direct_action(c: &AircraftControls) -> [f32; 6] {
    let throttle = if c.throttle <= 0.001 {
        -1.0
    } else if c.throttle >= 0.999 {
        1.0
    } else {
        ((c.throttle - 0.5) / sim_env::action::THROTTLE_GAIN).clamp(-1.0, 1.0)
    };
    [
        c.elevator as f32,
        c.aileron as f32,
        c.rudder as f32,
        throttle as f32,
        (f64::from(c.flap_notch) / 1.5 - 1.0) as f32,
        c.trim as f32,
    ]
}

const DIRECT: ActionConfig = ActionConfig {
    mode: ActionMode::Direct,
    flaps: true,
    trim: true,
};

/// Imitation data: the expert autopilot's action for every decision of episodes flown by the
/// expert, or (DAgger) by a network that the expert watches and labels, mixed: each decision is
/// the expert's with probability `beta`. File: "FLTDATA1", u32 observation length, u32 action
/// length, then f32 records of observation and action.
fn data(args: &[String], terrain: &Arc<Terrain>, threads: usize) {
    let Some(expert) = args.first().filter(|a| !a.starts_with("--")) else {
        fail("data <expert pattern file | default> --out <file>")
    };
    let pattern = if expert == "default" { Pattern::default() } else { load_pattern(expert) };
    let out: String = value(args, "--out").unwrap_or_else(|| fail("data needs --out"));
    let episodes: u64 = value(args, "--episodes").unwrap_or(200);
    let first: u64 = value(args, "--first-seed").unwrap_or(100_000);
    let beta: f64 = value(args, "--beta").unwrap_or(1.0);
    let dt: f64 = value(args, "--dt").unwrap_or(0.1);
    let model = value::<String>(args, "--model").map(|m| Arc::new(Model::load(&m).unwrap_or_else(|e| fail(&e))));
    if let Some(m) = &model {
        if m.action != DIRECT || (m.decision_dt - dt).abs() > 1e-9 {
            fail("the model must have direct actions with flaps and trim, and the same --dt");
        }
    }
    let cs = cases(episodes / 2, first);
    let next = AtomicUsize::new(0);
    let records = Mutex::new(Vec::<f32>::new());
    let outcomes = Mutex::new(Vec::<Outcome>::new());
    let mut obs_len = 0;
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            let terrain = Arc::new(Terrain::new(terrain.seed()));
            let (next, records, outcomes, cs, pattern, model) = (&next, &records, &outcomes, &cs, &pattern, &model);
            s.spawn(move || {
                let mut scratch = Scratch::default();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= cs.len() {
                        break;
                    }
                    let case = cs[i];
                    let mut env = Env::with_terrain(
                        EnvConfig { task: race_task(case.wind), action: DIRECT, decision_dt: dt, ..EnvConfig::default() },
                        terrain.clone(),
                    );
                    let mut step = env.reset(case.seed);
                    let mut expert = autopilot(pattern, &env);
                    let mut rng = Rng::new(case.seed ^ 0x5eed);
                    let mut net = vec![0.0f32; 6];
                    let mut mine = Vec::new();
                    while !(step.terminated || step.truncated) {
                        let obs = env.world().observe(env.vehicle_id());
                        let current = env.vehicle().aircraft().unwrap().controls;
                        // The expert takes the controls as they are, whoever moved them.
                        expert.follow(&current);
                        let label = direct_action(&expert.fly_aircraft(&obs, &current, env.decision_dt()));
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
                    outcomes.lock().expect("not poisoned").push(Outcome {
                        reason: step.info.reason,
                        time: f64::from(step.info.time),
                        ret: f64::from(step.info.episode_return),
                    });
                    records.lock().expect("not poisoned").extend(mine);
                }
            });
        }
    });
    obs_len += sim_env::Env::new(EnvConfig { task: race_task(false), ..EnvConfig::default() }).observation_len();
    let records = records.into_inner().expect("not poisoned");
    let mut bytes = Vec::with_capacity(16 + records.len() * 4);
    bytes.extend_from_slice(b"FLTDATA1");
    bytes.extend_from_slice(&(obs_len as u32).to_le_bytes());
    bytes.extend_from_slice(&6u32.to_le_bytes());
    for x in &records {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    std::fs::write(&out, bytes).expect("could not write the data");
    let o = outcomes.into_inner().expect("not poisoned");
    println!("{} decisions written to {out}; the flights:", records.len() / (obs_len + 6));
    print_summary("all", &summarize(&o));
}

/// One episode, a line a second.
fn fly(args: &[String], terrain: &Arc<Terrain>) {
    let Some(spec) = args.first() else { fail("fly <competitor>") };
    let competitor = Competitor::parse(spec);
    let case = Case { seed: value(args, "--seed").unwrap_or(1), wind: args.iter().any(|a| a == "--wind") };
    let mut last = -1.0;
    let mut log = |env: &Env| {
        let p = env.vehicle();
        let t = env.world().time();
        if t - last >= 1.0 {
            last = t;
            let v = env.targets().runway.unwrap_or_default();
            println!(
                "t {:>6.1}  along {:>7.0} across {:>6.0} height {:>6.1}  ias {:>5.1} kt  hdg err {:>5.0}  roll {:>4.0}  vs {:>5.1}  thr {:.2} flap {}  leg {}",
                t,
                v.along,
                v.across,
                v.height,
                p.aircraft().unwrap().indicated_airspeed() / KNOTS,
                v.heading_error / DEG,
                sim_core::euler_angles(p.aircraft().unwrap().state.q).roll / DEG,
                p.aircraft().unwrap().vertical_speed(),
                p.aircraft().unwrap().controls.throttle,
                p.aircraft().unwrap().controls.flap_notch,
                env.targets().leg
            );
        }
    };
    let o = run(&competitor, case, terrain, Some(&mut log));
    println!("{}: {:.1} s, return {:.1}", o.reason.name(), o.time, o.ret);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let threads: usize = value(&args, "--threads").unwrap_or(8);
    let terrain = Arc::new(Terrain::new(1));
    let home = EnvConfig::home(&terrain).expect("world 1 has an airport");
    eprintln!(
        "runway: {:.0} m long, {:.0} m wide, heading {:.0} deg, elevation {:.0} m",
        home.length,
        home.width,
        home.heading / DEG,
        home.elevation
    );
    let rest = &args[1.min(args.len())..];
    match args.first().map(String::as_str) {
        Some("eval") => eval(rest, &terrain, threads),
        Some("search") => search(rest, &terrain, threads),
        Some("data") => data(rest, &terrain, threads),
        Some("fly") => fly(rest, &terrain),
        _ => fail("race eval|search|data|fly ... (see the comment at the top of examples/race.rs)"),
    }
}
