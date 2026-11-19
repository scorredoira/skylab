//! The `gates` race of a quad round a course (Pinewood by default, `--course <key>` another, in
//! the course's own quad: Deepwood's racing quad): runs competitors over many seeds and winds, and
//! writes imitation data from the race's own pilot (`course::racer`) for training a network, with
//! DAgger.
//!
//! ```text
//! gates score <competitor>... [--course K] [--episodes N] [--first-seed S] [--laps L] [--pad M] [--pad-spread M] [--threads T] [--verbose]
//! gates data --out data.bin [--course K] [--model net.fltmodel] [--beta B] [--episodes N] [--first-seed S] [--dt S] [--pad-spread M] [--threads T]
//! gates fly <competitor> [--course K] [--seed S] [--wind] [--pad M]   one race, a line a second (EVERY=<s> another interval), and its speeds
//! ```
//! A competitor is `racer` (the course's racer, deciding every simulation step), `held[@dt]`
//! (the racer deciding only every `dt` seconds, 0.1 by default, as a network does) or a model
//! file (`.fltmodel`, trained on `gates`).
//!
//! `score` races every competitor from the start's pad for the course's laps (two at Pinewood) on
//! the same episodes: seeds `S..S+N` in calm air and the same seeds with wind of 0 to 6 m/s from
//! any direction and turbulence 0 to 0.3. A success is every lap flown, no crash, no envelope.
//! `--pad M` starts every race M metres to the right of the pad's middle (across the start, as
//! the side pads of the game's race: negative is left), `--pad-spread M` anywhere up to M either
//! side of that, at random.
//!
//! `data` flies episodes as the training does (half from the pad, half in flight before a gate
//! drawn at random; wind as above on every other one; the course's laps) by the racer, or (DAgger)
//! by a network the racer watches; every decision is recorded with the racer's direct sticks for
//! it. File: "FLTDATA1", u32 observation length, u32 action length (4), then f32 records of
//! observation and action.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sim_core::course::racer::Racer;
use sim_core::{Controls, MultirotorControls, Pilot, Rng, Terrain};
use sim_env::env::observe;
use sim_env::model::Scratch;
use sim_env::{ActionConfig, ActionMode, Env, EnvConfig, Model, Reason, Task};

fn value<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1)?.parse().ok()
}

fn fail(message: &str) -> ! {
    eprintln!("gates: {message}");
    std::process::exit(2)
}

/// Seconds an episode may last: two laps of some 86 s and room to spare.
const TIME_LIMIT: f64 = 300.0;
/// The wind of a windy episode: up to this, m/s, and turbulence up to `TURBULENCE`.
const WIND: f64 = 6.0;
const TURBULENCE: f64 = 0.3;

#[derive(Clone)]
enum Competitor {
    /// The racer deciding every simulation step (`None`) or every so many seconds.
    Racer(Option<f64>),
    Network(Arc<Model>),
}

impl Competitor {
    fn parse(spec: &str) -> Competitor {
        if spec == "racer" {
            return Competitor::Racer(None);
        }
        if let Some(rest) = spec.strip_prefix("held") {
            let dt = rest.strip_prefix('@').map_or(Some(0.1), |d| d.parse().ok()).unwrap_or_else(|| fail("held@<seconds>"));
            return Competitor::Racer(Some(dt));
        }
        let model = Model::load(spec).unwrap_or_else(|e| fail(&format!("{spec}: {e}")));
        if model.task != "gates" {
            fail(&format!("{spec} was trained for {}, not gates", model.task));
        }
        Competitor::Network(Arc::new(model))
    }
}

/// The conditions of an episode.
#[derive(Clone, Copy)]
struct Case {
    seed: u64,
    wind: bool,
    /// Share of the starts in flight before a gate (0: always from the pad).
    gate_starts: f64,
    /// Laps (0: the course's).
    laps: f64,
    /// A start on the pad: metres right of its middle, and the random spread about that.
    pad: f64,
    pad_spread: f64,
}

/// The course raced (`--course`, by its key): its place in the list of courses.
static COURSE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

fn task(case: Case) -> Task {
    let mut task = Task::parse("gates").expect("gates exists");
    assert!(task.set("config.course", *COURSE.get().unwrap_or(&0.0)));
    assert!(task.set("config.time_limit", TIME_LIMIT));
    assert!(task.set("config.laps", case.laps));
    assert!(task.set("config.gate_starts", case.gate_starts));
    assert!(task.set("config.pad_offset", case.pad));
    assert!(task.set("config.pad_spread", case.pad_spread));
    if case.wind {
        assert!(task.set("air.wind_max", WIND));
        assert!(task.set("air.turbulence_max", TURBULENCE));
    }
    task
}

const DIRECT: ActionConfig = ActionConfig { mode: ActionMode::Direct, flaps: false, trim: false };

fn env_for(case: Case, terrain: &Arc<Terrain>, dt: f64) -> Env {
    Env::with_terrain(EnvConfig { task: task(case), action: DIRECT, decision_dt: dt, ..EnvConfig::default() }, terrain.clone())
}

/// The racer, taking over the race the environment has just started.
fn racer(env: &Env) -> Racer {
    let (track, race) = env.guidance().and_then(|g| g.course()).expect("gates has a course");
    let p = env.vehicle().multirotor().expect("a multirotor").p.clone();
    let mut r = Racer::new(Arc::new(track.clone()), &p);
    r.take_over(race);
    r
}

/// The direct action that gives these sticks.
fn direct_action(c: &MultirotorControls) -> [f32; 4] {
    [(2.0 * c.throttle - 1.0) as f32, c.roll as f32, c.pitch as f32, c.yaw as f32]
}

/// What the racer would do now, as a direct action, for a decision of `dt` seconds.
fn label(expert: &mut Racer, env: &Env, dt: f64) -> [f32; 4] {
    let obs = observe(env.world(), env.vehicle_id());
    let current = env.vehicle().dynamics.controls();
    match expert.fly(&obs, &current, dt) {
        Controls::Multirotor(c) => direct_action(&c),
        _ => unreachable!("gates flies a multirotor"),
    }
}

/// What one episode gave.
#[derive(Clone, Copy, Debug)]
struct Outcome {
    reason: Reason,
    /// Simulated seconds from the start to the end.
    time: f64,
    ret: f64,
    /// Gates missed, and the laps' times (the first two).
    missed: u32,
    laps: [f64; 2],
    /// Gates flown through, the start's too.
    passed: u32,
}

fn outcome(env: &Env, step: &sim_env::Step) -> Outcome {
    let p = env.targets().race.unwrap_or_default();
    let mut laps = [f64::NAN; 2];
    if let Some((_, race)) = env.guidance().and_then(|g| g.course()) {
        for (l, t) in laps.iter_mut().zip(&race.lap_times) {
            *l = *t;
        }
    }
    Outcome { reason: step.info.reason, time: f64::from(step.info.time), ret: f64::from(step.info.episode_return), missed: p.missed, laps, passed: p.passed }
}

fn run(competitor: &Competitor, case: Case, terrain: &Arc<Terrain>, mut log: Option<&mut dyn FnMut(&Env)>) -> Outcome {
    let dt = match competitor {
        Competitor::Racer(every) => every.unwrap_or(0.1),
        Competitor::Network(m) => m.decision_dt,
    };
    let mut env = match competitor {
        Competitor::Network(m) => Env::with_terrain(EnvConfig { task: task(case), action: m.action, decision_dt: dt, ..EnvConfig::default() }, terrain.clone()),
        _ => env_for(case, terrain, dt),
    };
    let mut step = env.reset(case.seed);
    match competitor {
        Competitor::Racer(None) => {
            let mut r = racer(&env);
            while !(step.terminated || step.truncated) {
                step = env.step_with_pilot(&mut r);
                if let Some(log) = log.as_deref_mut() {
                    log(&env);
                }
            }
        }
        Competitor::Racer(Some(dt)) => {
            let mut r = racer(&env);
            while !(step.terminated || step.truncated) {
                let a = label(&mut r, &env, *dt);
                step = env.step(&a);
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
    outcome(&env, &step)
}

/// Runs `f` over `cases` on `threads` threads; the results come back in the order of `cases`.
fn par<T: Send, F: Fn(Case, &Arc<Terrain>) -> T + Sync>(cases: &[Case], terrain: &Arc<Terrain>, threads: usize, f: F) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<T>>> = Mutex::new((0..cases.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            // A terrain per thread: its caches are behind a lock that threads would queue on.
            let terrain = Arc::new(Terrain::new(terrain.seed()));
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

struct Summary {
    successes: usize,
    clean: usize,
    total: usize,
    best: f64,
    median: f64,
    mean: f64,
    lap1: f64,
    lap2: f64,
    reasons: [usize; 6],
}

fn median(mut v: Vec<f64>) -> f64 {
    v.retain(|x| x.is_finite());
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

fn summarize(outcomes: &[Outcome]) -> Summary {
    let ok: Vec<&Outcome> = outcomes.iter().filter(|o| o.reason == Reason::Success).collect();
    let times: Vec<f64> = ok.iter().map(|o| o.time).collect();
    let mut reasons = [0; 6];
    for o in outcomes {
        reasons[o.reason as usize] += 1;
    }
    Summary {
        successes: ok.len(),
        clean: ok.iter().filter(|o| o.missed == 0).count(),
        total: outcomes.len(),
        best: times.iter().copied().fold(f64::NAN, f64::min),
        median: median(times.clone()),
        mean: if times.is_empty() { f64::NAN } else { times.iter().sum::<f64>() / times.len() as f64 },
        lap1: median(ok.iter().map(|o| o.laps[0]).collect()),
        lap2: median(ok.iter().map(|o| o.laps[1]).collect()),
        reasons,
    }
}

fn print_summary(label: &str, s: &Summary) {
    let others: Vec<String> =
        Reason::NAMES.iter().zip(s.reasons).filter(|(n, c)| *c > 0 && **n != "success").map(|(n, c)| format!("{n} {c}")).collect();
    println!(
        "  {label:<5} success {:>5.1}% ({}/{}), {} with no gate missed  race: best {:>6.1} s median {:>6.1} s mean {:>6.1} s  laps (median) {:>5.1} + {:>5.1} s  {}",
        100.0 * s.successes as f64 / s.total.max(1) as f64,
        s.successes,
        s.total,
        s.clean,
        s.best,
        s.median,
        s.mean,
        s.lap1,
        s.lap2,
        others.join(", ")
    );
}

fn eval_cases(episodes: u64, first: u64, laps: f64, pad: f64, pad_spread: f64) -> Vec<Case> {
    let mut v = Vec::new();
    for wind in [false, true] {
        for seed in first..first + episodes {
            v.push(Case { seed, wind, gate_starts: 0.0, laps, pad, pad_spread });
        }
    }
    v
}

fn score(args: &[String], terrain: &Arc<Terrain>, threads: usize) {
    let episodes: u64 = value(args, "--episodes").unwrap_or(100);
    let first: u64 = value(args, "--first-seed").unwrap_or(1);
    let laps: f64 = value(args, "--laps").unwrap_or(0.0);
    let pad: f64 = value(args, "--pad").unwrap_or(0.0);
    let pad_spread: f64 = value(args, "--pad-spread").unwrap_or(0.0);
    let specs: Vec<&String> =
        args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--"))).map(|(_, a)| a).collect();
    if specs.is_empty() {
        fail("score needs at least one competitor");
    }
    let cs = eval_cases(episodes, first, laps, pad, pad_spread);
    for spec in specs {
        let competitor = Competitor::parse(spec);
        let started = std::time::Instant::now();
        let outcomes = par(&cs, terrain, threads, |c, t| run(&competitor, c, t, None));
        let n = episodes as usize;
        println!("{spec}: {} races from the pad ({pad:+.1} m, ±{pad_spread:.1}), seeds {first}..{} ({:.0} s)", cs.len(), first + episodes - 1, started.elapsed().as_secs_f64());
        print_summary("calm", &summarize(&outcomes[..n]));
        print_summary("wind", &summarize(&outcomes[n..]));
        print_summary("all", &summarize(&outcomes));
        if args.iter().any(|a| a == "--verbose") {
            for (c, o) in cs.iter().zip(&outcomes) {
                println!(
                    "    seed {:>4} {} {:<16} {:>6.1} s  laps {:>5.1} {:>5.1}  passed {:>3}  missed {}  return {:>7.1}",
                    c.seed,
                    if c.wind { "wind" } else { "calm" },
                    o.reason.name(),
                    o.time,
                    o.laps[0],
                    o.laps[1],
                    o.passed,
                    o.missed,
                    o.ret
                );
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
    let dt: f64 = value(args, "--dt").unwrap_or(0.1);
    let gate_starts: f64 = value(args, "--gate-starts").unwrap_or(0.5);
    let pad_spread: f64 = value(args, "--pad-spread").unwrap_or(0.0);
    let model = value::<String>(args, "--model").map(|m| Arc::new(Model::load(&m).unwrap_or_else(|e| fail(&e))));
    if let Some(m) = &model {
        if m.action != DIRECT || (m.decision_dt - dt).abs() > 1e-9 || m.task != "gates" {
            fail("the model must be a gates model with direct actions and the same --dt");
        }
    }
    let cs: Vec<Case> = (first..first + episodes).map(|seed| Case { seed, wind: seed % 2 == 1, gate_starts, laps: 0.0, pad: 0.0, pad_spread }).collect();
    let results = par(&cs, terrain, threads, |case, terrain| {
        let mut env = env_for(case, terrain, dt);
        let mut step = env.reset(case.seed);
        let mut expert = racer(&env);
        let mut rng = Rng::new(case.seed ^ 0x5eed);
        let mut scratch = Scratch::default();
        let mut net = [0.0f32; 4];
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
        (outcome(&env, &step), mine)
    });
    let obs_len = env_for(cs[0], terrain, dt).observation_len();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"FLTDATA1");
    bytes.extend_from_slice(&(obs_len as u32).to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
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
    println!("{} decisions written to {out}; the flights:", count / (obs_len + 4));
    print_summary("all", &summarize(&outcomes));
}

/// One race, a line a second.
fn fly(args: &[String], terrain: &Arc<Terrain>) {
    let Some(spec) = args.first() else { fail("fly <competitor>") };
    let competitor = Competitor::parse(spec);
    let case = Case { seed: value(args, "--seed").unwrap_or(1), wind: args.iter().any(|a| a == "--wind"), gate_starts: 0.0, laps: 0.0, pad: value(args, "--pad").unwrap_or(0.0), pad_spread: 0.0 };
    let mut last = -1.0;
    // The way flown from the start signal and the fastest, for its speeds.
    let (mut flown, mut top, mut from) = (0.0, 0.0f64, None::<sim_core::V3>);
    let mut log = |env: &Env| {
        let t = env.world().time();
        let at = env.vehicle().body().pos;
        if let Some(p) = from {
            flown += (at - p).length();
        }
        from = Some(at);
        top = top.max(env.vehicle().body().vel.length());
        let crashed = env.vehicle().dynamics.crashed();
        if let Some(why) = crashed {
            println!("crashed: {why}");
        }
        if t - last >= std::env::var("EVERY").ok().and_then(|x| x.parse().ok()).unwrap_or(1.0) || crashed.is_some() {
            last = t;
            let b = env.vehicle().body();
            let p = env.targets().race.unwrap_or_default();
            println!(
                "t {:>6.1}  pos {:>7.1} {:>7.1} alt {:>6.1}  speed {:>5.1}  tilt {:>3.0}°  passed {:>2} missed {}  progress {:>5.2}  blades {:?}",
                t,
                b.pos.x,
                b.pos.y,
                -b.pos.z,
                b.vel.length(),
                {
                    let e = sim_core::euler_angles(b.q);
                    (e.pitch.cos() * e.roll.cos()).clamp(-1.0, 1.0).acos().to_degrees()
                },
                p.passed,
                p.missed,
                p.progress,
                env.vehicle().multirotor().map(|m| m.blades().iter().map(|b| (b[0].min(b[1]) * 100.0).round() as i32).collect::<Vec<_>>()).unwrap_or_default()
            );
        }
    };
    let o = run(&competitor, case, terrain, Some(&mut log));
    println!("{}: {:.1} s, laps {:.1} {:.1}, missed {}, return {:.1}", o.reason.name(), o.time, o.laps[0], o.laps[1], o.missed, o.ret);
    println!("{flown:.0} m flown: {:.1} m/s on average ({:.0} km/h), {top:.1} m/s at most ({:.0} km/h)", flown / o.time, flown / o.time * 3.6, top * 3.6);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let threads: usize = value(&args, "--threads").unwrap_or(8);
    if let Some(key) = value::<String>(&args, "--course") {
        let index = sim_core::course::file::courses().iter().position(|c| c.key == key).unwrap_or_else(|| fail(&format!("no course {key}")));
        COURSE.set(index as f64).expect("set once");
    }
    let terrain = Arc::new(Terrain::new(1));
    let rest = &args[1.min(args.len())..];
    match args.first().map(String::as_str) {
        Some("score") => score(rest, &terrain, threads),
        Some("data") => data(rest, &terrain, threads),
        Some("fly") => fly(rest, &terrain),
        _ => fail("gates score|data|fly ... (see the comment at the top of examples/gates.rs)"),
    }
}
