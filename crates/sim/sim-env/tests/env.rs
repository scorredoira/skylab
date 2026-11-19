//! The environment: determinism, the tasks, the vectorised form, and the hand-made model flying.

use sim_core::{AircraftKind, Pilot, Rng, Start, World};
use sim_env::env::observe;
use sim_env::guidance::{leg, Guidance};
use sim_env::model::{wings_leveller, Scratch};
use sim_env::observation::{self, Inputs};
use sim_env::vec_env::{next_seed, TERMINATED, TRUNCATED};
use sim_env::{
    ActionConfig, ActionMode, Env, EnvConfig, Model, ModelPilot, Reason, Referee, Step, Task,
    VecEnv,
};

fn config(task: &str) -> EnvConfig {
    EnvConfig {
        task: Task::parse(task).unwrap(),
        ..EnvConfig::default()
    }
}

fn random(rng: &mut Rng, n: usize, size: f64) -> Vec<f32> {
    (0..n)
        .map(|_| ((rng.uniform() * 2.0 - 1.0) * size) as f32)
        .collect()
}

/// Runs an episode of small random actions; returns every step.
fn rollout(env: &mut Env, seed: u64, action_seed: u64, most: usize) -> Vec<Step> {
    let mut rng = Rng::new(action_seed);
    let mut out = vec![env.reset(seed)];
    for _ in 0..most {
        let step = env.step(&random(&mut rng, env.action_len(), 0.3));
        let done = step.terminated || step.truncated;
        out.push(step);
        if done {
            break;
        }
    }
    out
}

#[test]
fn an_episode_is_a_pure_function_of_config_seed_and_actions() {
    for task in Task::NAMES {
        let mut cfg = config(task);
        // Turbulence has its own random numbers: they must come from the seed too.
        cfg.task.air_mut().wind_max = 6.0;
        cfg.task.air_mut().turbulence_max = 1.0;
        let mut a = Env::new(cfg);
        let first = rollout(&mut a, 11, 5, 150);
        // The same environment after flying something else, and a new one.
        rollout(&mut a, 99, 6, 40);
        let again = rollout(&mut a, 11, 5, 150);
        let fresh = rollout(&mut Env::new(cfg), 11, 5, 150);
        assert_eq!(
            first, again,
            "{task}: an old environment remembers something"
        );
        assert_eq!(first, fresh, "{task}");
        let other_seed = rollout(&mut a, 12, 5, 150);
        assert_ne!(first[0].observation, other_seed[0].observation, "{task}");
        let other_actions = rollout(&mut a, 11, 7, 150);
        assert_eq!(first[0], other_actions[0], "{task}");
        assert_ne!(first[5].observation, other_actions[5].observation, "{task}");
    }
}

#[test]
fn observations_are_finite_bounded_and_of_the_described_length() {
    for task in Task::NAMES {
        let mut env = Env::new(config(task));
        let fields = observation::fields(&env.config().task);
        let mut rng = Rng::new(3);
        for seed in 0..3 {
            let mut step = env.reset(seed);
            for _ in 0..300 {
                assert_eq!(step.observation.len(), fields.len());
                assert_eq!(env.observation_len(), fields.len());
                for (v, f) in step.observation.iter().zip(fields) {
                    assert!(
                        v.is_finite() && v.abs() <= observation::CLIP,
                        "{task} {}: {v}",
                        f.name
                    );
                }
                assert!(step.reward.is_finite());
                if step.terminated || step.truncated {
                    break;
                }
                step = env.step(&random(&mut rng, env.action_len(), 1.0));
            }
        }
    }
    for table in [observation::FIELDS, observation::MULTIROTOR_FIELDS, observation::DRIVE_FIELDS] {
        let names: Vec<&str> = table.iter().map(|f| f.name).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "field names repeat");
    }
}

/// The multirotor's task: it flies the quad, its observation is the multirotor's table, its
/// actions the four sticks.
#[test]
fn hover_flies_a_multirotor() {
    let mut env = Env::new(config("hover"));
    assert_eq!(env.action_len(), 4);
    assert_eq!(env.observation_len(), observation::MULTIROTOR_FIELDS.len());
    let step = env.reset(1);
    assert!(env.vehicle().multirotor().is_some_and(|m| m.armed()));
    let at = |name: &str| observation::index_in(observation::MULTIROTOR_FIELDS, name).unwrap();
    assert_eq!(step.observation[at("armed")], 1.0);
    assert!(step.observation[at("agl")] > 0.5, "starts in the air");
    // Full throttle, direct: up it goes.
    let mut c = config("hover");
    c.action.mode = ActionMode::Direct;
    let mut env = Env::new(c);
    env.reset(2);
    let before = env.vehicle().body().pos.z;
    for _ in 0..5 {
        env.step(&[1.0, 0.0, 0.0, 0.0]);
    }
    assert!(env.vehicle().body().pos.z < before - 2.0);
}

/// A hover controller on the observation vector alone (direct actions: the throttle from the
/// height's error and the climb, the sticks from where the point is and how fast it drifts)
/// holds the height within half a metre and stays over the point, in wind and turbulence, to
/// the end of the episode.
#[test]
fn a_hover_controller_holds_altitude() {
    let mut c = config("hover");
    c.action.mode = ActionMode::Direct;
    c.task.set("air.wind_max", 4.0);
    c.task.set("air.turbulence_max", 0.3);
    let mut env = Env::new(c);
    let at = |name: &str| observation::index_in(observation::MULTIROTOR_FIELDS, name).unwrap();
    let raw = |obs: &[f32], name: &str| f64::from(obs[at(name)]) * f64::from(observation::MULTIROTOR_FIELDS[at(name)].scale);
    for seed in 0..4 {
        let mut step = env.reset(seed);
        let mut worst: f64 = 0.0;
        let mut integral = 0.0;
        let mut steps = 0;
        while !(step.terminated || step.truncated) {
            let o = &step.observation;
            let height_error = raw(o, "altitude_error");
            let climb = (0.8 * height_error).clamp(-3.0, 3.0) - raw(o, "vertical_speed");
            integral = (integral + 0.04 * climb * 0.1f64).clamp(-0.3, 0.3);
            let throttle = (0.40 + integral + 0.06 * climb).clamp(0.0, 1.0);
            // Self-levelling: a stick is a tilt; lean towards the point, against the drift.
            let pitch = (-(0.08 * raw(o, "point_ahead") - 0.25 * raw(o, "forward_speed"))).clamp(-0.4, 0.4);
            let roll = (0.08 * raw(o, "point_right") - 0.25 * raw(o, "right_speed")).clamp(-0.4, 0.4);
            let yaw = (1.5 * raw(o, "heading_error")).clamp(-1.0, 1.0);
            step = env.step(&[(2.0 * throttle - 1.0) as f32, roll as f32, pitch as f32, yaw as f32]);
            steps += 1;
            if steps > 50 {
                worst = worst.max(raw(&step.observation, "altitude_error").abs());
            }
        }
        println!("seed {seed}: {:?} after {steps} decisions, height within {worst:.2} m, return {:.1}", step.info.reason, step.info.episode_return);
        assert_eq!(step.info.reason, Reason::TimeLimit, "seed {seed}");
        assert!(worst < 0.5, "seed {seed}: {worst}");
        assert!(step.info.episode_return > 15.0, "seed {seed}: {}", step.info.episode_return);
    }
}

/// The environment's cheap `observe` and `World::observe` give the same observation vector.
#[test]
fn the_cheap_observation_matches_the_worlds() {
    let mut env = Env::new(config("landing"));
    let mut rng = Rng::new(1);
    env.reset(4);
    for _ in 0..50 {
        env.step(&random(&mut rng, 4, 0.3));
    }
    let (world, id) = (env.world(), env.vehicle_id());
    let controls = world.vehicle(id).aircraft().unwrap().controls;
    let mut vectors = Vec::new();
    for obs in [observe(world, id), world.observe(id)] {
        let mut guidance = env.guidance().unwrap().clone();
        let targets = guidance.update(&obs);
        let mut v = vec![0.0; observation::len()];
        observation::write(
            &Inputs {
                obs: &obs,
                controls: &sim_core::Controls::Aircraft(controls),
                targets: &targets,
            },
            &mut v,
        );
        vectors.push(v);
    }
    assert_eq!(vectors[0], vectors[1]);
    assert!(vectors[0].iter().filter(|x| **x != 0.0).count() > 20);
}

#[test]
fn hold_starts_near_trim_and_ends_at_the_time_limit() {
    let mut env = Env::new(config("hold"));
    let at = |name: &str| observation::index(name).unwrap();
    for seed in 0..6 {
        let first = env.reset(seed);
        let o = &first.observation;
        assert!(o[at("airspeed")] * 50.0 > 30.0 && o[at("airspeed")] * 50.0 < 56.0);
        assert!(o[at("agl")] * 1000.0 > 500.0);
        assert!(o[at("roll")].abs() < 0.6);
        assert!(o[at("heading_error")].abs() < 1.7);
        assert!(o[at("altitude_error")].abs() * 100.0 < 130.0);
        assert_eq!(o[at("has_runway")], 0.0);
    }
    // Hands off, the aircraft flies on: the episode is truncated, not terminated.
    env.reset(2);
    let mut last = env.step(&[0.0; 4]);
    let mut steps = 1;
    while !(last.terminated || last.truncated) {
        last = env.step(&[0.0; 4]);
        steps += 1;
    }
    assert!(last.truncated && !last.terminated);
    assert_eq!(last.info.reason, Reason::TimeLimit);
    assert_eq!(steps, 600);
    assert_eq!(last.info.episode_steps, 600);
    // After the end nothing moves until the next reset.
    let after = env.step(&[1.0; 4]);
    assert_eq!(after.observation, last.observation);
    assert_eq!(after.reward, 0.0);
    assert!(after.truncated);
}

#[test]
fn full_back_stick_ends_in_the_ground_or_out_of_the_envelope() {
    let mut cfg = config("hold");
    cfg.action.mode = ActionMode::Direct;
    let mut env = Env::new(cfg);
    env.reset(1);
    // Nose down, full left, power on.
    let mut last = env.step(&[-1.0, -1.0, 0.0, 1.0]);
    while !(last.terminated || last.truncated) {
        last = env.step(&[-1.0, -1.0, 0.0, 1.0]);
    }
    assert!(last.terminated);
    assert!(matches!(
        last.info.reason,
        Reason::OutOfEnvelope | Reason::Crashed
    ));
    assert!(last.reward < -10.0);
}

/// Flies with the aircraft's own trim solution and simple rules: enough to see that the takeoff
/// succeeds and pays, with the actions a model would use.
#[test]
fn a_scripted_takeoff_succeeds() {
    let mut cfg = config("takeoff");
    cfg.action.mode = ActionMode::Direct;
    let mut env = Env::new(cfg);
    let at = |name: &str| observation::index(name).unwrap();
    for seed in [1, 2] {
        let mut step = env.reset(seed);
        assert_eq!(step.observation[at("on_ground")], 1.0);
        assert_eq!(step.observation[at("has_runway")], 1.0);
        while !(step.terminated || step.truncated) {
            let o = &step.observation;
            let fast = o[at("airspeed")] * 50.0 > 28.0;
            let pitch_wanted = if fast { 0.14 } else { 0.0 };
            let elevator = 3.0 * (pitch_wanted - o[at("pitch")] * 0.5) - 0.5 * o[at("pitch_rate")];
            let rudder = if o[at("on_ground")] > 0.5 {
                2.0 * o[at("heading_error")]
            } else {
                0.0
            };
            let aileron = 0.5 * o[at("heading_error")] - o[at("roll")] - 0.3 * o[at("roll_rate")];
            step = env.step(&[elevator, aileron, rudder, 1.0]);
        }
        println!("takeoff seed {seed}: {:?}", step.info);
        assert_eq!(step.info.reason, Reason::Success);
        assert!(step.info.episode_return > 40.0);
        assert!(step.observation[at("runway_height")] * 100.0 >= 150.0);
    }
}

#[test]
fn landing_starts_on_final_with_errors_and_judges_the_touchdown() {
    let mut env = Env::new(config("landing"));
    let at = |name: &str| observation::index(name).unwrap();
    let (mut far, mut near) = (0.0f32, f32::MAX);
    for seed in 0..12 {
        let o = env.reset(seed).observation;
        let distance = o[at("runway_distance")] * 5000.0;
        far = far.max(distance);
        near = near.min(distance);
        assert!((1400.0..4100.0).contains(&distance), "{distance}");
        assert!(o[at("runway_along")] < 0.0);
        assert!(o[at("runway_across")].abs() * 100.0 <= 0.06 * distance + 20.0);
        assert!(o[at("path_error")].abs() * 50.0 <= 0.26 * distance * 0.0525 + 10.0);
        assert!(o[at("runway_heading_error")].abs() < 0.12);
        assert_eq!(o[at("flap")], 20.0 / 30.0);
        assert_eq!(o[at("leg")], 1.0);
    }
    assert!(far - near > 800.0, "starts do not vary: {near}..{far}");
    // Throttle closed and hands off: it comes down short of the runway, which ends the episode.
    let mut cfg = config("landing");
    cfg.action.mode = ActionMode::Direct;
    let mut env = Env::new(cfg);
    env.reset(3);
    let mut last = env.step(&[0.0, 0.0, 0.0, -1.0]);
    while !(last.terminated || last.truncated) {
        last = env.step(&[0.0, 0.0, 0.0, -1.0]);
    }
    println!("landing, power off: {:?}", last.info);
    assert!(last.terminated);
    assert!(matches!(
        last.info.reason,
        Reason::OutOfEnvelope | Reason::Crashed
    ));
}

#[test]
fn the_circuit_guidance_goes_round_the_legs() {
    let world = World::new(1);
    let airport = world.terrain.nearest_airport(0.0, 0.0).unwrap();
    let runway = sim_env::Runway {
        airport,
        reversed: false,
    };
    let mut guidance = Guidance::circuit(runway);
    let h = runway.heading();
    let (c, s) = (h.cos(), h.sin());
    let half = airport.length / 2.0;
    // (metres past the threshold, metres right of the centreline, height above the runway)
    let place = |along: f64, across: f64, height: f64| sim_core::Observation {
        north: airport.n + (along - half) * c - across * s,
        east: airport.e + (along - half) * s + across * c,
        altitude: airport.elevation + height + 1.0,
        heading: h,
        ..Default::default()
    };
    let mut legs = Vec::new();
    for (along, across, height) in [
        (40.0, 0.0, 0.0),
        (900.0, 0.0, 100.0),
        (1500.0, 0.0, 160.0),
        (1900.0, -800.0, 200.0),
        (0.0, -1000.0, 200.0),
        (-2600.0, -1000.0, 200.0),
        (-2600.0, -250.0, 140.0),
        (-1000.0, 0.0, 60.0),
    ] {
        let t = guidance.update(&place(along, across, height));
        let v = t.runway.unwrap();
        assert!((v.along - along).abs() < 1e-6 && (v.across - across).abs() < 1e-6);
        legs.push(t.leg);
    }
    use leg::*;
    assert_eq!(
        legs,
        [UPWIND, UPWIND, CROSSWIND, DOWNWIND, DOWNWIND, BASE, FINAL, FINAL]
    );
}

#[test]
fn every_task_number_can_be_listed_and_set() {
    for name in Task::NAMES {
        let mut task = Task::parse(name).unwrap();
        assert_eq!(task.name(), name);
        for (section, params) in task.params() {
            assert!(!params.is_empty());
            for p in params {
                assert!(!p.description.is_empty() && !p.description.starts_with(' '));
                let key = format!("{section}.{}", p.name);
                assert!(task.set(&key, p.value + 1.0), "{key}");
            }
        }
        assert!(!task.set("rewards.nothing", 1.0));
        assert_eq!(
            task.time_limit(),
            Task::parse(name).unwrap().time_limit() + 1.0
        );
    }
}

#[test]
fn actions_map_to_controls() {
    use sim_core::AircraftControls;
    use sim_env::ActionMapper;
    let all = ActionConfig {
        mode: ActionMode::Direct,
        flaps: true,
        trim: true,
    };
    assert_eq!(all.len(), 6);
    let mut m = ActionMapper::new(all);
    let c = m.apply(
        &[0.5, -0.25, 1.0, 1.0, 1.0, -0.5],
        &AircraftControls::default(),
        0.1,
    );
    assert_eq!((c.elevator, c.aileron, c.rudder), (0.5, -0.25, 1.0));
    assert_eq!((c.throttle, c.flap_notch, c.trim), (1.0, 3, -0.5));
    let c = m.apply(&[9.0, f32::NAN, 0.0, -1.0, -1.0, 0.0], &c, 0.1);
    assert_eq!(
        (c.elevator, c.aileron, c.throttle, c.flap_notch),
        (1.0, 0.0, 0.0, 0)
    );
    let c = m.apply(&[0.0, 0.0, 0.0, 0.0, -0.2, 0.0], &c, 0.1);
    assert_eq!((c.throttle, c.flap_notch), (0.5, 1));

    let mut m = ActionMapper::new(ActionConfig {
        mode: ActionMode::Incremental,
        flaps: true,
        trim: false,
    });
    let start = AircraftControls {
        throttle: 0.4,
        flap_notch: 2,
        trim: 0.3,
        ..AircraftControls::default()
    };
    m.reset(&start);
    let mut c = start;
    for _ in 0..5 {
        c = m.apply(&[1.0, -1.0, 0.0, 1.0, 1.0], &c, 0.1);
    }
    assert!((c.elevator - 0.5).abs() < 1e-9 && (c.aileron + 0.5).abs() < 1e-9);
    assert!((c.throttle - 0.65).abs() < 1e-9);
    assert_eq!((c.flap_notch, c.trim), (3, 0.3));
    for _ in 0..30 {
        c = m.apply(&[1.0, -1.0, 0.0, -1.0, -1.0], &c, 0.1);
    }
    assert_eq!(
        (c.elevator, c.aileron, c.throttle, c.flap_notch),
        (1.0, -1.0, 0.0, 0)
    );
}

#[test]
fn the_vectorised_environment_matches_single_ones_and_resets_by_itself() {
    let mut cfg = config("hold");
    cfg.task.set("config.time_limit", 3.0);
    let n = 5;
    let mut rng = Rng::new(8);
    let actions: Vec<Vec<f32>> = (0..45).map(|_| random(&mut rng, n * 4, 0.5)).collect();

    let mut vec = VecEnv::new(cfg, n, 3);
    assert_eq!(vec.threads(), 3);
    let seeds: Vec<u64> = (0..n as u64).map(|i| 100 + i).collect();
    let first = vec.reset(&seeds).to_vec();
    let steps: Vec<_> = actions.iter().map(|a| vec.step(a).clone()).collect();

    let len = observation::len();
    let mut finished = 0;
    for i in 0..n {
        let mut env = Env::new(cfg);
        let mut seed = seeds[i];
        assert_eq!(env.reset(seed).observation, first[i * len..(i + 1) * len]);
        for (k, a) in actions.iter().enumerate() {
            let single = env.step(&a[i * 4..(i + 1) * 4]);
            let batch = &steps[k];
            assert_eq!(single.reward, batch.rewards[i]);
            let flag = if single.truncated {
                TRUNCATED
            } else if single.terminated {
                TERMINATED
            } else {
                0
            };
            assert_eq!(flag, batch.flags[i]);
            let shown = &batch.observations[i * len..(i + 1) * len];
            if flag == 0 {
                assert_eq!(single.observation, shown);
            } else {
                // The batch shows the next episode; the finished one is reported apart.
                let f = batch.finished.iter().find(|f| f.env == i as u32).unwrap();
                assert_eq!(f.final_observation, single.observation);
                assert_eq!(f.episode_return, single.info.episode_return);
                assert_eq!(
                    (f.steps, f.reason),
                    (single.info.episode_steps, single.info.reason)
                );
                seed = next_seed(seed);
                assert_eq!(env.reset(seed).observation, shown);
                finished += 1;
            }
        }
    }
    // 45 steps of a 30-step time limit: every environment finished one episode at least.
    assert!(finished >= n);
    // One thread or three, the same numbers.
    let mut one = VecEnv::new(cfg, n, 1);
    one.reset(&seeds);
    let again: Vec<_> = actions.iter().map(|a| one.step(a).clone()).collect();
    assert_eq!(steps, again);
}

// MARK: The model

fn random_model(
    seed: u64,
    hidden: &[usize],
    activation: sim_env::Activation,
    normalize: bool,
) -> Model {
    let mut rng = Rng::new(seed);
    let action = ActionConfig {
        mode: ActionMode::Incremental,
        flaps: true,
        trim: false,
    };
    let mut m = Model::new("landing", action, hidden, activation);
    for l in &mut m.layers {
        l.weights
            .iter_mut()
            .for_each(|w| *w = (rng.normal() * 0.3) as f32);
        l.bias
            .iter_mut()
            .for_each(|b| *b = (rng.normal() * 0.1) as f32);
    }
    if normalize {
        let n = observation::len();
        m.normalization = Some((
            (0..n).map(|_| rng.normal() as f32).collect(),
            (0..n).map(|_| (0.5 + rng.uniform()) as f32).collect(),
        ));
    }
    m.decision_dt = 0.05;
    m
}

/// The forward pass written out plainly, in `f64`, straight from the definition in PROTOCOL.md.
#[allow(clippy::needless_range_loop)] // plain indices, as the document writes it
fn reference(m: &Model, obs: &[f32]) -> Vec<f64> {
    let mut x: Vec<f64> = obs.iter().map(|v| f64::from(*v)).collect();
    if let Some((mean, std)) = &m.normalization {
        for i in 0..x.len() {
            x[i] = (x[i] - f64::from(mean[i])) / f64::from(std[i]);
        }
    }
    for (k, l) in m.layers.iter().enumerate() {
        let mut y = vec![0.0; l.outputs];
        for o in 0..l.outputs {
            y[o] = f64::from(l.bias[o]);
            for i in 0..l.inputs {
                y[o] += f64::from(l.weights[o * l.inputs + i]) * x[i];
            }
            y[o] = if k + 1 == m.layers.len() {
                match m.squash {
                    sim_env::Squash::Tanh => y[o].tanh(),
                    sim_env::Squash::Clip => y[o].clamp(-1.0, 1.0),
                }
            } else {
                match m.activation {
                    sim_env::Activation::Tanh => y[o].tanh(),
                    sim_env::Activation::Relu => y[o].max(0.0),
                }
            };
        }
        x = y;
    }
    x
}

#[test]
fn a_model_file_round_trips_and_computes_what_the_reference_does() {
    use sim_env::{Activation, Squash};
    let dir = std::env::temp_dir().join(format!("sim-env-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut rng = Rng::new(21);
    for (k, (hidden, activation, normalize)) in [
        (vec![32, 16], Activation::Tanh, true),
        (vec![64], Activation::Relu, false),
        (vec![], Activation::Tanh, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut model = random_model(k as u64, &hidden, activation, normalize);
        if k == 1 {
            model.squash = Squash::Clip;
        }
        let path = dir.join(format!("m{k}.fltmodel"));
        model.save(&path).unwrap();
        let loaded = Model::load(&path).unwrap();
        assert_eq!(loaded, model);
        let mut scratch = Scratch::default();
        for _ in 0..20 {
            let obs = random(&mut rng, observation::len(), 3.0);
            let mut out = vec![0.0f32; 5];
            loaded.act(&obs, &mut scratch, &mut out);
            for (a, b) in out.iter().zip(reference(&model, &obs)) {
                assert!((f64::from(*a) - b).abs() < 1e-4, "{a} vs {b}");
                assert!(a.abs() <= 1.0);
            }
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_bad_model_file_is_refused_with_a_reason() {
    let model = random_model(1, &[8], sim_env::Activation::Tanh, true);
    let bytes = model.to_bytes();
    assert!(Model::from_bytes(&bytes).is_ok());
    assert!(Model::from_bytes(&bytes[..bytes.len() - 4])
        .unwrap_err()
        .contains("truncated"));
    let mut longer = bytes.clone();
    longer.extend_from_slice(&[0; 4]);
    assert!(Model::from_bytes(&longer).unwrap_err().contains("expected"));
    assert!(Model::from_bytes(b"not a model at all").is_err());
    // Trained on another layout: it loads as a file but no pilot will fly it.
    let mut other = model.clone();
    other.observation[3] = "something_else".to_string();
    let read = Model::from_bytes(&other.to_bytes()).unwrap();
    let error = read.check_layout().unwrap_err();
    assert!(
        error.contains("field 3") && error.contains("something_else"),
        "{error}"
    );
    assert!(ModelPilot::new(read).is_err());
}

/// The whole path before any training exists: weights written by hand, saved to a file, loaded,
/// and flown by `ModelPilot` through `World::fly` as the game will do it.
#[test]
fn the_hand_made_model_flies_to_its_targets_and_holds_them() {
    let path = std::env::temp_dir().join(format!(
        "sim-env-leveller-{}.fltmodel",
        std::process::id()
    ));
    wings_leveller().save(&path).unwrap();
    let model = Model::load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    let mut cfg = config("hold");
    cfg.action = model.action;
    let mut env = Env::new(cfg);
    let at = |name: &str| observation::index(name).unwrap();
    for seed in [1, 2, 3] {
        let mut step = env.reset(seed);
        let start = step.observation.clone();
        let mut pilot = ModelPilot::new(model.clone())
            .unwrap()
            .with_guidance(env.guidance().unwrap().clone());
        while !(step.terminated || step.truncated) {
            step = env.step_with_pilot(&mut pilot);
        }
        let o = &step.observation;
        println!(
            "leveller seed {seed}: return {:.1}; heading error {:.2} -> {:.2} rad, altitude error {:.0} -> {:.0} m, roll {:.2} rad",
            step.info.episode_return,
            start[at("heading_error")], o[at("heading_error")],
            start[at("altitude_error")] * 100.0, o[at("altitude_error")] * 100.0,
            o[at("roll")]
        );
        assert_eq!(
            step.info.reason,
            Reason::TimeLimit,
            "it did not stay in the air"
        );
        assert!(o[at("heading_error")].abs() < 0.1);
        assert!(o[at("altitude_error")].abs() * 100.0 < 40.0);
        assert!(o[at("roll")].abs() < 0.1);
        assert!(step.info.episode_return > 15.0);
    }
}

/// `ModelPilot` in a plain world does exactly what the environment does with the same model.
#[test]
fn the_pilot_and_the_environment_fly_the_same_flight() {
    for mode in [ActionMode::Direct, ActionMode::Incremental] {
        let mut model = wings_leveller();
        model.action.mode = mode;
        let mut cfg = config("hold");
        cfg.action = model.action;

        // The environment, fed the model's actions.
        let mut env = Env::new(cfg);
        let mut step = env.reset(5);
        let guidance = env.guidance().unwrap().clone();
        // A plain world brought to the same start, flown by the pilot.
        let mut world = World::new(cfg.world_seed);
        let id = world.add_aircraft(AircraftKind::C172, Start::Home);
        world.weather = env.world().weather;
        world.vehicles[id.0] = env.vehicle().clone();
        let mut pilot = ModelPilot::new(model.clone())
            .unwrap()
            .with_guidance(guidance);

        let mut scratch = Scratch::default();
        let mut action = vec![0.0f32; 4];
        let mut compared = 0;
        for k in 0..100 {
            model.act(&step.observation, &mut scratch, &mut action);
            step = env.step(&action);
            for _ in 0..24 {
                world.fly(id, &mut pilot);
                world.step();
            }
            assert_eq!(
                world.vehicle(id).aircraft().unwrap().state,
                env.vehicle().aircraft().unwrap().state,
                "{mode:?} step {k}"
            );
            compared += 1;
            // As rates the hand-made gains are unstable: the comparison ends with the episode.
            if step.terminated {
                break;
            }
        }
        assert!(
            compared >= 50,
            "{mode:?}: only {compared} decisions compared"
        );
    }
}

/// The network trained on `gates` (`models/gates.fltmodel`, `trainer/GATES.md`): its file names the
/// quad's sticks and reads back as written, and `ModelPilot` flying a quad in a plain world, given
/// the course, does exactly what the environment does with the same model: off the pad and through
/// the first gates.
#[test]
fn a_gates_model_flies_the_quad_as_the_environment_does() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../models/gates.fltmodel");
    let model = Model::load(path).unwrap();
    assert_eq!(model.task, "gates");
    assert!(model.multirotor());
    assert_eq!(Model::from_bytes(&model.to_bytes()).unwrap(), model);
    assert!(String::from_utf8_lossy(&model.to_bytes()).contains(r#""actions":["throttle","roll","pitch","yaw"]"#));

    let mut cfg = config("gates");
    cfg.task.set("config.gate_starts", 0.0);
    cfg.action = model.action;
    cfg.decision_dt = model.decision_dt;
    let mut env = Env::new(cfg);
    let mut step = env.reset(3);
    let guidance = env.guidance().unwrap().clone();
    let mut world = World::new(cfg.world_seed);
    let id = world.add_multirotor(sim_core::MultirotorKind::FPV5, Start::Home);
    world.weather = env.world().weather;
    world.obstacles = env.world().obstacles.clone();
    world.vehicles[id.0] = env.vehicle().clone();
    let mut pilot = ModelPilot::new(model.clone()).unwrap().with_guidance(guidance);
    // An aircraft's model leaves a quad alone, and this one an aircraft.
    let before = world.vehicle(id).dynamics.controls();
    assert_eq!(ModelPilot::new(wings_leveller()).unwrap().fly(&world.observe(id), &before, World::DT), before);

    let mut scratch = Scratch::default();
    let mut action = vec![0.0f32; 4];
    for k in 0..300 {
        let seen = step.observation.clone();
        model.act(&seen, &mut scratch, &mut action);
        step = env.step(&action);
        for _ in 0..env.config().substeps() {
            world.fly(id, &mut pilot);
            world.step();
        }
        assert_eq!(world.vehicle(id).body(), env.vehicle().body(), "step {k}");
        // What the pilot saw at its decision, the environment gave before the step.
        assert_eq!(pilot.observation(), &seen[..], "step {k}");
        assert!(!step.terminated, "step {k}: {:?}", step.info.reason);
    }
    assert!(env.targets().race.unwrap().passed >= 3, "{:?}", env.targets().race);
}

/// Without being told, the pilot works out the runway from what it sees.
#[test]
fn the_pilot_finds_its_runway() {
    let mut model = wings_leveller();
    model.task = "takeoff".to_string();
    let mut world = World::new(1);
    let id = world.add_aircraft(AircraftKind::C172, Start::Home);
    let mut pilot = ModelPilot::new(model).unwrap();
    world.fly(id, &mut pilot);
    let view = pilot.targets().runway.expect("a runway");
    assert!(
        (view.along - 40.0).abs() < 1.0 && view.across.abs() < 1.0,
        "{view:?}"
    );
    assert!(view.heading_error.abs() < 1e-6);
    assert_eq!(
        pilot.observation()[observation::index("has_runway").unwrap()],
        1.0
    );
}

/// A landing flown by simple rules with the actions a model has: the task can be completed, and
/// completing it pays far more than anything else.
#[test]
fn a_scripted_landing_succeeds() {
    let mut cfg = config("landing");
    cfg.action.mode = ActionMode::Direct;
    let mut env = Env::new(cfg);
    let at = |name: &str| observation::index(name).unwrap();
    for seed in [1, 2, 3] {
        let mut step = env.reset(seed);
        while !(step.terminated || step.truncated) {
            let o = &step.observation;
            let height = f64::from(o[at("runway_height")]) * 100.0;
            let across = f64::from(o[at("runway_across")]) * 100.0;
            let turn = f64::from(o[at("runway_heading_error")]);
            let pitch = f64::from(o[at("pitch")]) * 0.5;
            let throttle = f64::from(o[at("throttle")]);
            let action = if o[at("on_ground")] > 0.5 {
                [
                    0.3,
                    0.0,
                    (2.0 * turn - 0.03 * across).clamp(-1.0, 1.0),
                    -1.0,
                ]
            } else {
                let path = f64::from(o[at("path_error")]) * 50.0;
                let flare = height < 5.0;
                let wanted = if flare {
                    5.0
                } else {
                    -1.5 - (path * 0.25).clamp(-4.0, 4.0)
                };
                let elevator =
                    4.0 * (wanted.to_radians() - pitch) - 0.5 * f64::from(o[at("pitch_rate")]);
                let bank = (-0.6 * across + 130.0 * turn)
                    .clamp(-10.0, 10.0)
                    .to_radians();
                let aileron =
                    2.0 * (bank - f64::from(o[at("roll")])) - 0.3 * f64::from(o[at("roll_rate")]);
                let power = if flare {
                    throttle - 0.045
                } else {
                    throttle + 0.01 * f64::from(o[at("speed_error")]) * 10.0
                };
                [elevator, aileron, 0.0, (power - 0.5) / 0.55]
            };
            step = env.step(&action.map(|a| a as f32));
        }
        println!("landing seed {seed}: {:?}", step.info);
        assert_eq!(step.info.reason, Reason::Success);
        assert!(step.info.episode_return > 80.0);
    }
}

/// A `Referee` watching an aircraft in a plain world, flown by the same pilot, gives the score and
/// the ending the environment gives.
#[test]
fn the_referee_judges_as_the_environment_does() {
    for (task, seed) in [("hold", 2), ("landing", 3), ("takeoff", 4)] {
        let model = wings_leveller();
        let mut cfg = config(task);
        cfg.action = model.action;
        let mut env = Env::new(cfg);
        let mut step = env.reset(seed);
        let guidance = env.guidance().unwrap().clone();
        let mut world = World::new(cfg.world_seed);
        let id = world.add_aircraft(AircraftKind::C172, Start::Home);
        world.weather = env.world().weather;
        world.vehicles[id.0] = env.vehicle().clone();
        let mut referee = Referee::new(cfg.task, guidance.clone(), &world, id, cfg.decision_dt);
        let mut a = ModelPilot::new(model.clone())
            .unwrap()
            .with_guidance(guidance.clone());
        let mut b = ModelPilot::new(model.clone())
            .unwrap()
            .with_guidance(guidance);
        while !(step.terminated || step.truncated) {
            step = env.step_with_pilot(&mut a);
        }
        let mut reason = Reason::Running;
        while reason == Reason::Running {
            world.fly(id, &mut b);
            world.step();
            reason = referee.step(&world, id);
        }
        println!(
            "{task}: environment {:?} {:.3} in {:.1} s, referee {reason:?} {:.3} in {:.1} s",
            step.info.reason,
            step.info.episode_return,
            step.info.time,
            referee.episode_return(),
            referee.time()
        );
        assert_eq!(reason, step.info.reason, "{task}");
        assert!(
            (referee.time() - f64::from(step.info.time)).abs() < 1e-3,
            "{task}"
        );
        let r = f64::from(step.info.episode_return);
        assert!(
            (referee.episode_return() - r).abs() < 1e-3 * r.abs().max(1.0),
            "{task}"
        );
    }
}

/// A staggered reset ends the first episodes at spread times, as time limits, and leaves the
/// later ones whole.
#[test]
fn a_staggered_reset_spreads_the_first_episodes() {
    let mut cfg = config("hold");
    cfg.task.set("config.time_limit", 2.0); // 20 decisions
    let n = 4;
    let mut vec = VecEnv::new(cfg, n, 2);
    vec.reset_staggered(&[1, 2, 3, 4]);
    let zeros = vec![0.0f32; n * 4];
    let mut ends: Vec<Vec<(u32, Reason)>> = vec![Vec::new(); n];
    for _ in 0..45 {
        for f in &vec.step(&zeros).finished {
            ends[f.env as usize].push((f.steps, f.reason));
        }
    }
    for (i, e) in ends.iter().enumerate() {
        // Environment i of 4: first episode cut at 20 * (i + 1) / 4 decisions, then whole ones.
        let first = 20 * (i as u32 + 1) / 4;
        assert_eq!(e[0], (first, Reason::TimeLimit), "environment {i}: {e:?}");
        assert_eq!(e[1], (20, Reason::TimeLimit), "environment {i}: {e:?}");
    }
    // A plain reset cuts nothing.
    vec.reset(&[1, 2, 3, 4]);
    let mut first = Vec::new();
    for _ in 0..20 {
        first.extend(vec.step(&zeros).finished.iter().map(|f| f.steps));
    }
    assert_eq!(first, vec![20; n]);
}

/// The autopilot's standard circuit is a valid race: it climbs, turns back and lands. A taller
/// minimum height than its circuit makes the same flight invalid.
#[test]
fn the_autopilot_circuit_completes_the_race() {
    use sim_core::{Autopilot, Goal};
    for (seed, min_height, expected) in [(3, 120.0, Reason::Success), (3, 400.0, Reason::Failed)] {
        let mut cfg = config("race");
        assert!(cfg.task.set("config.min_height", min_height));
        let mut env = Env::new(cfg);
        let mut step = env.reset(seed);
        let runway = env.guidance().unwrap().runway().unwrap();
        let mut pilot = Autopilot::new(Goal::Circuit {
            airport: runway.airport,
            reversed: runway.reversed,
            repeat: false,
        });
        let mut legs = vec![step_leg(&env)];
        while !(step.terminated || step.truncated) {
            step = env.step_with_pilot(&mut pilot);
            if legs.last() != Some(&step_leg(&env)) {
                legs.push(step_leg(&env));
            }
        }
        println!("race, minimum height {min_height}: {:?}, legs {legs:?}", step.info);
        assert_eq!(step.info.reason, expected);
        if expected == Reason::Success {
            assert_eq!(legs, [leg::UPWIND, leg::TURNING, leg::RETURNING]);
            assert!(step.info.time > 200.0 && step.info.time < 400.0);
        }
    }
}

fn step_leg(env: &Env) -> u8 {
    env.targets().leg
}

/// The race round a course's gates: its observation is the multirotor's table and the next two
/// gates, it runs with random sticks (crashing or leaving the course, never panicking), the same
/// seed gives the same episode, and the race's own autopilot (`course::racer`) flies a whole lap
/// through every gate from the start's pad: a success, with a return of more than the gates' worth.
#[test]
fn gates_races_round_a_course() {
    let mut c = config("gates");
    let mut env = Env::new(c);
    assert_eq!(env.action_len(), 4);
    assert_eq!(env.observation_len(), observation::MULTIROTOR_FIELDS.len() + observation::GATE_FIELDS.len());
    let a = rollout(&mut env, 3, 4, 2000);
    let b = rollout(&mut env, 3, 4, 2000);
    assert_eq!(a, b, "the same seed, the same episode");
    assert!(a.last().is_some_and(|s| s.terminated || s.truncated));
    // The next gate is some metres ahead of where it starts (or under it, for a dive gate), on the
    // pad or in flight before a gate.
    let fields = observation::gates_fields();
    let at = |name: &str| observation::index_in(fields, name).unwrap();
    let raw = |o: &[f32], name: &str| f64::from(o[at(name)]) * f64::from(fields[at(name)].scale);
    for seed in 0..20 {
        let o = env.reset(seed).observation;
        let (ahead, right, up) = (raw(&o, "gate_ahead"), raw(&o, "gate_right"), raw(&o, "gate_up"));
        let distance = (ahead * ahead + right * right + up * up).sqrt();
        assert!((15.0..40.0).contains(&distance) && (ahead > 10.0 || up < -10.0), "{ahead} {right} {up}");
    }

    c.task.set("config.gate_starts", 0.0);
    let mut env = Env::new(c);
    env.reset(7);
    let mut course = env.config().task.course().unwrap().clone();
    course.laps = 1;
    let track = std::sync::Arc::new(sim_core::course::Track::new(&course, &env.world().terrain));
    let p = env.vehicle().multirotor().unwrap().p.clone();
    let mut racer = sim_core::course::racer::Racer::new(track.clone(), &p);
    racer.go(env.world().time(), env.vehicle().body().pos);
    let mut step = env.step_with_pilot(&mut racer);
    let mut total = step.reward;
    while !(step.terminated || step.truncated) {
        step = env.step_with_pilot(&mut racer);
        total += step.reward;
    }
    println!("gates: {:?} after {:.1} s, return {total:.1}", step.info.reason, step.info.time);
    assert_eq!(step.info.reason, Reason::Success);
    assert!(total > 5.0 * track.gates.len() as f32, "{total}");
}

/// `drive`: the car's observation (`DRIVE_FIELDS`) and two actions (steering and pedals); with
/// random actions it ends (off the circuit, crashed, stopped or at the time limit) and the same
/// seed gives the same episode; a start on the racing line is on the asphalt and moving; and the
/// racing driver (`car::driver::Driver`, the teacher) drives a whole lap from the grid: a success,
/// with a return of more than the lap's progress.
#[test]
fn drive_laps_a_circuit() {
    let mut c = config("drive");
    let mut env = Env::new(c);
    assert_eq!(env.action_len(), 2);
    assert_eq!(env.observation_len(), observation::DRIVE_FIELDS.len());
    let a = rollout(&mut env, 5, 2, 400);
    let b = rollout(&mut env, 5, 2, 400);
    assert_eq!(a, b, "the same seed, the same episode");
    let fields = observation::DRIVE_FIELDS;
    let at = |name: &str| observation::index_in(fields, name).unwrap();
    let raw = |o: &[f32], name: &str| f64::from(o[at(name)]) * f64::from(fields[at(name)].scale);
    c.task.set("config.grid_starts", 0.0);
    let mut env = Env::new(c);
    for seed in 0..10 {
        let o = env.reset(seed).observation;
        assert!(raw(&o, "across").abs() < raw(&o, "half_width"), "seed {seed}: {:.1} m across", raw(&o, "across"));
        assert!(raw(&o, "speed") > 5.0, "seed {seed}: {:.1} m/s", raw(&o, "speed"));
    }

    c.task.set("config.grid_starts", 1.0);
    let mut env = Env::new(c);
    env.reset(3);
    let circuit = env.guidance().and_then(|g| g.race_circuit()).cloned().expect("a drive's circuit");
    let car = env.vehicle().car().unwrap().p.clone();
    let mut driver = sim_core::car::driver::Driver::new(circuit, &car);
    let mut step = env.step_with_pilot(&mut driver);
    let mut total = step.reward;
    while !(step.terminated || step.truncated) {
        step = env.step_with_pilot(&mut driver);
        total += step.reward;
    }
    println!("drive: {:?} after {:.1} s, return {total:.1}", step.info.reason, step.info.time);
    assert_eq!(step.info.reason, Reason::Success);
    assert!(total > 40.0, "{total}");
}
