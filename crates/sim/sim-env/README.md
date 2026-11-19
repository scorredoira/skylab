# sim-env

The training environment for AI pilots: tasks with rewards over `sim-core`, stepped in bulk and
served over a local socket to a trainer in another program (Swift + MLX), and `ModelPilot`, which
flies a trained network inside the simulator in plain Rust. Depends on `sim-core` only; no
graphics, no ML library, no JSON crate. Its server binary is still called `flight-env` (`target/release/flight-env`):
the Swift trainer runs it by that name.

| Module | What it is for |
|---|---|
| `env` | `Env`, `EnvConfig`, `Step`, `Info`: one environment, `reset(seed)` / `step(action)` |
| `vec_env` | `VecEnv`: N environments on worker threads, auto-reset, one shared terrain |
| `task` | The tasks: starts, rewards, endings. Every number is a named parameter |
| `guidance` | `Guidance`, `Runway`, `Targets`: what the pilot is asked to do, from the `Observation` |
| `observation` | `FIELDS`, `MULTIROTOR_FIELDS` and `GATE_FIELDS`: the tables that build and describe the observation vector (`fields(task)` picks one) |
| `action` | `ActionConfig`, `ActionMapper`: the action vector and how it becomes `AircraftControls`, or a multirotor's sticks (`ActionConfig::sticks`) |
| `model` | `Model` (the weight file), `ModelPilot` (a `Pilot`), `wings_leveller()` |
| `referee` | `Referee`: the score and ending of a task for an aircraft flown outside an `Env` (the game) |
| `server` | The protocol of `PROTOCOL.md` over any byte stream |
| `json` | The little JSON the protocol and the model file need |

```rust
let mut env = Env::new(EnvConfig { task: Task::parse("landing").unwrap(), ..EnvConfig::default() });
let mut step = env.reset(seed);                 // same seed, same episode
while !(step.terminated || step.truncated) {
    step = env.step(&action);                   // 0.1 s of flight: 24 simulation steps
}
```

An episode is a pure function of (config, seed, actions). The simulation runs at 240 Hz; the policy
decides every `decision_dt` (0.1 s) and its action is held in between.

## Running it

```sh
cargo run --release -p sim-env -- --task landing --num-envs 384      # 127.0.0.1:7447
cargo run --release -p sim-env -- --unix /tmp/flight.sock --set air.wind_max=5
cargo run --release -p sim-env -- --aircraft extra --task hold       # the Extra 300L
cargo run --release -p sim-env -- --task hover                       # the FPV quad (--multirotor fpv5)
cargo run --release -p sim-env -- --task gates                       # the FPV quad round a race course
cargo run --release -p sim-env -- --task drive                       # the GT coupé round Lakeside Park
cargo run --release -p sim-env -- --help
```

The flags are only defaults: a client can choose the task, the aircraft, the number of environments, the action
mode and any task number in its `hello`. The protocol, complete, is in [PROTOCOL.md](PROTOCOL.md).

The Swift client (one file, Foundation only) plays random actions and prints returns and speed:

```sh
swiftc -O crates/sim/sim-env/clients/swift/RandomAgent.swift -o random-agent
./random-agent --port 7447 --task hold --envs 64 --steps 20000
```

```
server: flight-env 0.1.0, protocol 1
task: hold, time limit 60 s (600 steps), 64 environments on 6 threads, a decision every 0.1 s
observation (34): airspeed ground_speed agl vertical_speed pitch roll heading_sin heading_cos ...
action (4, incremental): elevator aileron rudder throttle
episode 0: environment 7, return -20.36, 32 steps (3.2 s), out_of_envelope
...
20032 environment steps, 202 episodes finished: mean return -19.46, mean length 80.5 steps [out_of_envelope 202]
69399 environment steps per second, 6940 simulated seconds per second (0.29 s)
```

## Observation

34 numbers, each `raw / scale` clipped to ±10. The table lives once, in `observation::FIELDS`;
the vector, the `describe` message and the model file's compatibility check all come from it.

| # | name | scale | raw value |
|---|---|---|---|
| 0 | `airspeed` | 50 | indicated airspeed, m/s |
| 1 | `ground_speed` | 50 | speed over the ground, m/s |
| 2 | `agl` | 1000 | height above the ground below, m |
| 3 | `vertical_speed` | 10 | m/s, positive up |
| 4 | `pitch` | 0.5 | rad, positive nose up |
| 5 | `roll` | 1 | rad, positive right wing down |
| 6, 7 | `heading_sin`, `heading_cos` | 1 | of the heading |
| 8, 9, 10 | `roll_rate`, `pitch_rate`, `yaw_rate` | 1 | body rates, rad/s |
| 11, 12 | `alpha`, `beta` | 0.25 | angle of attack, sideslip, rad |
| 13 | `slip_ball` | 0.2 | rad |
| 14 | `rpm` | 2700 | engine speed |
| 15, 16, 17 | `elevator`, `aileron`, `rudder` | 1 | where the yoke and pedals are |
| 18 | `throttle` | 1 | 0 to 1 |
| 19 | `flap` | 30 | actual flap deflection, degrees |
| 20 | `trim` | 1 | elevator trim |
| 21 | `on_ground` | 1 | 0 or 1 |
| 22 | `stall_warning` | 1 | 0 or 1 |
| 23 | `heading_error` | 1 | target heading − heading, rad in (−π, π] |
| 24 | `altitude_error` | 100 | target altitude − altitude, m |
| 25 | `speed_error` | 10 | target airspeed − indicated airspeed, m/s |
| 26 | `has_runway` | 1 | 1 when the task has a runway (all but `hold`) |
| 27 | `runway_along` | 1000 | metres past the threshold (negative on the approach) |
| 28 | `runway_across` | 100 | metres right of the centreline |
| 29 | `runway_height` | 100 | height of the wheels above the runway, m |
| 30 | `path_error` | 50 | height above the 3° approach path, m |
| 31 | `runway_heading_error` | 1 | runway direction − heading, rad |
| 32 | `runway_distance` | 5000 | horizontal distance to the aiming point, m |
| 33 | `leg` | 5 | circuit leg: 1 upwind, 2 crosswind, 3 downwind, 4 base, 5 final |

Every task gives targets for heading, altitude and airspeed (`Guidance`), so entries 23–25 always
mean something. No pixels yet: an image will be a second array (`observation.image` in `describe`,
today `null`), leaving this vector as it is.

## Actions

Each in [−1, 1]: `elevator`, `aileron`, `rudder`, `throttle`, and optionally `flaps` and `trim`.

| | `incremental` (default) | `direct` |
|---|---|---|
| elevator, aileron, rudder | speed of the control: 1 = 1 per second | position |
| throttle | 1 opens it at 0.5 per second | `clamp(0.5 + 0.55 a, 0, 1)` |
| flaps (optional) | lever speed, 1 notch per second | notch `round(1.5 (a + 1))` |
| trim (optional) | 0.1 per second | position |

There is no brake control in the simulation: on the ground the wheels brake when the throttle is
closed. Without the flap action the flaps stay as the task set them (20° on `landing`, up
elsewhere), without the trim action the trim stays as the start left it.

## The multirotor: `hover`

The tasks `hover` and `gates` (below) fly a multirotor (`EnvConfig::multirotor`, the FPV quad `fpv5` by default; `hello`'s
`multirotor`), every other task an aircraft (`EnvConfig::aircraft`). Its observation is the
multirotor's table, `observation::MULTIROTOR_FIELDS` (26 numbers, clipped as the other), its action
the radio's four sticks, and `describe` says so (`vehicle`, the observation's fields, the actions):

| # | name | scale | raw value |
|---|---|---|---|
| 0 | `agl` | 20 | height above the ground below, m |
| 1 | `vertical_speed` | 5 | m/s, positive up |
| 2, 3 | `forward_speed`, `right_speed` | 10 | speed over the ground along and across the heading, m/s |
| 4, 5 | `pitch`, `roll` | 0.5 | rad |
| 6, 7 | `heading_sin`, `heading_cos` | 1 | of the heading |
| 8, 9, 10 | `roll_rate`, `pitch_rate`, `yaw_rate` | 5 | body rates, rad/s |
| 11–14 | `motor_1`…`motor_4` | 3000 | each rotor's speed, rad/s, in the flight controller's order (rear right, front right, rear left, front left) |
| 15 | `battery` | 16.8 | battery voltage, V |
| 16 | `throttle` | 1 | the throttle stick, 0 to 1 |
| 17, 18, 19 | `roll_stick`, `pitch_stick`, `yaw_stick` | 1 | the sticks, −1 to 1 (pitch: back, nose up, positive) |
| 20, 21 | `armed`, `on_ground` | 1 | 0 or 1 |
| 22 | `heading_error` | 1 | target heading − heading, rad |
| 23 | `altitude_error` | 10 | target altitude − altitude, m |
| 24, 25 | `point_ahead`, `point_right` | 10 | where the point to hover over is, metres ahead and to the right along the heading |

Actions, each in [−1, 1]: `throttle`, `roll`, `pitch`, `yaw`. `direct`: the throttle stick at
`(a + 1) / 2`, the others where `a` is. `incremental`: the throttle stick moves at 1 per second,
the others at 2. The arm switch stays on; the quad levels itself (a stick is a tilt, up to 55°; it has no
flight modes): what is learnt is the hover, not the rates.

**`hover`**: starts hovering 15 m above the ground within 200 m of the home airport, armed, facing
anywhere, pushed up to 2 m/s each way and turning up to 30°/s. Asked to hover over a point within
10 m of the start, 10 ± 4 m above the ground there, facing a heading within 90° of the start's.

| reward | default | |
|---|---|---|
| `position` | 0.4 /s | × exp(−distance from the point / 2 m) |
| `height` | 0.4 /s | × exp(−\|height error\| / 1 m) |
| `heading` | 0.2 /s | × exp(−\|heading error\| / 0.3 rad) |
| `rates` | −0.02 /s | × squared angular rate, at most 20 |
| `smooth` | −0.01 /s | × mean squared speed of the sticks |
| `crash`, `envelope` | −20 | once |

Ends: crash (a hit faster than 10 m/s, water); envelope (over 40 m from the point, over 15 m off the
height, tilted over 80°); time limit 30 s, no success: the best episode is worth 30.
`tests/env.rs` flies it with a hover controller on the observation alone (direct actions: the
throttle from the height's error and the climb, the sticks towards the point against the drift):
within 0.3 m of the height and a return of 24–25 in wind up to 4 m/s and turbulence 0.3.

## The multirotor's race: `gates`

A race round a course's gates (`sim_core::course`; `config.course`, 0 the built-in Pinewood
Airfield, 1 Deepwood Forest, both in the world of seed 1: the environment's `world_seed` must be
the course's). A course that names its quad (`Course::quad`: Deepwood's race quad, `race5`) is
flown in it, whatever `multirotor` says; Pinewood in `multirotor` (the FPV quad). Its
observation is the multirotor's table and then `observation::GATE_FIELDS` (38 numbers in all), its
action the four sticks as in `hover`. The gates are in the world (`World::obstacles`): a frame hit
too fast is a crash.

| # | name | scale | raw value |
|---|---|---|---|
| 0–25 | as `hover` | | (`heading_error`, `altitude_error`, `point_ahead`, `point_right` are 0) |
| 26, 27, 28 | `gate_ahead`, `gate_right`, `gate_up` | 50, 50, 20 | the middle of the next gate's opening from here, metres along the heading, to its right, above |
| 29, 30, 31 | `gate_through_ahead`, `_right`, `_up` | 1 | the way through it, a unit vector in the same axes (`_up` −1 for a dive gate) |
| 32–37 | `gate2_...` | | the same of the gate after it |

Starts: half the episodes (`gate_starts`) in flight 20 m before a gate drawn at random
(`gate_lead`), on the line through it, at 6 m/s towards it (`gate_speed`; above a dive gate); the
rest on the start's pad, armed, the throttle down, `pad_offset` metres right of its middle (0) and up to
`pad_spread` either side of that at random (0): the side pads of the game's race (`examples/gates.rs`
`--pad`, `--pad-spread`). An episode is `laps` laps (1; 0 the course's).

| reward | default | |
|---|---|---|
| `progress` | 2 | per gate's worth of the way round (`Race::progress`: gates flown and the share of the way to the next) |
| `gate` | +5 | each gate flown through |
| `finish` | +50 | once, the laps done: success |
| `missed` | −3 | each gate missed (past it outside the opening, or another flown instead) |
| `time` | −0.2 /s | the clock |
| `rates`, `smooth` | −0.005 /s | × squared angular rate (at most 20), mean squared speed of the sticks |
| `crash`, `envelope` | −20 | once |

Ends: success; crash; envelope (over 350 m from the next gate, tilted over 85°); time limit 150 s.
`tests/env.rs` runs it with random sticks (the same seed, the same episode) and flies it with the
race's own pilot (`course::racer::Racer`, through `step_with_pilot`): a lap from the pad in 89.5 s,
every gate, a return of 158: the baseline a trained model has to beat. Deepwood's trees are in
its world, cut for its trail (`sim_core::course::cuts`): a quad that leaves the trail meets them.

`cargo run --release -p sim-env --example gates -- score <competitor>... [--course deepwood]` races competitors (the
racer, the racer deciding every `dt` like a network, model files) from the pad for the course's two
laps on the same episodes, in calm air and in wind of 0 to 6 m/s with turbulence up to 0.3, and prints
success, gates missed and times; `data` writes the records `flight-train imitate --task gates` learns
from (the racer's flights, or a network's that the racer labels: DAgger); `fly` prints one race.
See the comment at the top of `examples/gates.rs`, `trainer/GATES.md` and `trainer/DEEPWOOD.md` for
the networks trained this way (`models/gates.fltmodel` at Pinewood, `models/gates-deepwood.fltmodel`
at Deepwood), which race in the game against the player (`--ai`).

## The car: `drive`

A car (`EnvConfig::car`, the GT coupé `gt` by default) laps a circuit (`config.circuit`, 0 Lakeside
Park, in the world of seed 1: `EnvConfig::terrain` lays the circuit on the seed's terrain, and the
environment's `world_seed` must be the circuit's). Its observation is `observation::DRIVE_FIELDS`
(36 numbers, clipped as the others), its action two numbers, `steer` and `pedal` (above 0 the
throttle, below the brake; `direct`: where they are; `incremental`: the steering moves at 2 a second,
the pedal at 4); the gearbox is the automatic.

| # | name | scale | raw value |
|---|---|---|---|
| 0, 1, 2 | `speed`, `lateral_speed`, `yaw_rate` | 50, 10, 2 | along the car, m/s; across the heading, m/s; rad/s |
| 3, 4 | `rpm`, `gear` | 8000, 6 | the engine's speed; −1 reverse to 6 |
| 5, 6, 7 | `steer`, `throttle`, `brake` | 1 | where they are |
| 8–11 | `slip_fl` … `slip_rr` | 1 | each tyre's longitudinal slip |
| 12–15 | `slip_angle_fl` … `slip_angle_rr` | 0.3 | each tyre's slip angle, rad |
| 16 | `off_asphalt` | 4 | wheels on grass, gravel or earth |
| 17, 18 | `across`, `half_width` | 10 | metres right of the centre line; half the asphalt's width |
| 19 | `track_heading_error` | 1 | the circuit's direction minus the heading, rad |
| 20–24 | `curvature_0` … `curvature_150` | 0.05 | the circuit's curvature here and 20, 50, 100, 150 m ahead, 1/m |
| 25–27 | `line_10`, `line_30`, `line_60` | 10 | the racing line 10, 30, 60 m ahead, metres right of the centre line |
| 28–30 | `line_speed_0`, `_50`, `_100` | 50 | the racing line's speed here and 50, 100 m ahead, m/s (for the episode's rain) |
| 31 | `rain` | 1 | how hard it rains, 0 to 1 |
| 32 | `water` | 2 | water under the tyres, mm (their mean) |
| 33–35 | `water_20`, `water_40`, `water_70` | 2 | water on the road 20, 40 and 70 m ahead along the way the car goes, mm (a puddle) |

Starts: `grid_starts` (0.3) of the episodes on the grid at rest; the rest on the racing line anywhere
round the lap, up to `start_spread` (1.5 m) off it, at 0.5 to 0.9 of the line's speed there.
`wet_share` (0) of the episodes are in the rain, of `rain_min` to `rain_max` (0.2 to 0.8), the
circuit's water as long rain leaves it (`sim_core::car::wet`). The car's aids (anti-lock, traction
and stability control) are on, as in the road car. `examples/drive.rs` scores competitors (the racing
driver, a network) dry and wet and writes imitation data for DAgger: `trainer/DRIVE.md`.

| reward | default | |
|---|---|---|
| `progress` | 0.01 | per metre round the circuit (backwards takes it off) |
| `lap`, `finish` | +10, +30 | each lap done; the laps (`laps`, 1) done: success |
| `time` | −0.05 /s | the clock |
| `off_track` | −0.5 /s | while the car's middle is beyond the kerbs |
| `smooth` | −0.005 /s | × mean squared speed of the steering and pedals |
| `crash`, `envelope` | −20, −10 | once |

Ends: success; crash (a wall or the ground struck harder than the car takes, water, upside down);
envelope (over `max_off`, 15 m, beyond the asphalt, or going the wrong way round for `max_reverse`, 2 s);
failed (standing still for `max_stopped`, 8 s); time limit 200 s. `tests/env.rs` runs it with random
actions (the same seed, the same episode) and drives it with the racing driver
(`sim_core::car::driver::Driver`, through `step_with_pilot`): a lap from the grid in 122 s, a return
of 79: the teacher a model learns from. `ModelPilot::driving(model, circuit, car)` drives a car with
a `drive` model outside an `Env` (the game's rival).

## Tasks

Every number below is a parameter (`config.*`, `rewards.*`, `air.*`), listed with its default and
description by `describe` and changed with `--set` or `hello.params`. Rewards "per second" are
multiplied by the decision interval. `air.*` is the wind and turbulence range of an episode:
calm by default, to be widened as a curriculum (`air.wind_max`, `air.turbulence_max`,
`air.crosswind_angle`).

Common endings: **crashed** (the aircraft is wrecked), **out_of_envelope**, **success**,
**failed**, and **time_limit**, the only one that truncates instead of terminating.

### `hold` — HoldHeadingAltitude
Starts in the air 900 ± 150 m above the ground near the home airport, at 38–54 m/s, banked up to
30°, pitched up to 8° off trim, with small rates; trimmed for its speed. Targets: a heading within
90° of the present one, an altitude within 100 m, an airspeed of 38–54 m/s.

| reward | default | |
|---|---|---|
| `heading` | 0.35 /s | × exp(−\|heading error\| / 0.15 rad) |
| `altitude` | 0.35 /s | × exp(−\|altitude error\| / 20 m) |
| `speed` | 0.2 /s | × exp(−\|airspeed error\| / 3 m/s) |
| `rates` | −0.1 /s | × squared angular rate, at most 5 |
| `slip` | −0.05 /s | × (sideslip / 0.1 rad)², at most 5 |
| `smooth` | −0.02 /s | × mean squared speed of yoke and pedals |
| `crash`, `envelope` | −20 | once |

Ends: crash; envelope (altitude error over 400 m, under 150 m above the ground, bank over 100°,
pitch over 60°); time limit 60 s. There is no success: the best episode lasts the 60 s, worth at
most 54.

### `takeoff`
Starts standing 40 m down the runway, within 4 m of the centreline and 3° of its direction, either
runway direction.

| reward | default | |
|---|---|---|
| `speed` | +10 in all | for airspeed gained up to 74 knots |
| `climb` | +20 in all | for height gained up to 150 m |
| `centreline` | −0.2 /s | × min(\|across\| / 50 m, 1) |
| `heading` | −0.2 /s | × \|runway heading error\| in rad |
| `smooth` | −0.02 /s | as above |
| `time` | −0.05 /s | |
| `success` | +20 | once |
| `crash` / `envelope` | −20 / −10 | once |

Success: 150 m above the runway, within 20° of its heading and 200 m of its centreline. Envelope:
on the ground off the runway, over 400 m from the centreline, over 90° off the heading, bank over
70°. Time limit 120 s.

### `landing`
Starts on final 1.5–4 km before the aiming point (150 m past the threshold), off the centreline by
up to 5 % of the distance, above or below the 3° path by up to 25 % of its height, up to 5° off the
runway heading, 68 knots ± 7 %, flaps 20, trimmed.

| reward | default | |
|---|---|---|
| `path` | 0.3 /s | in the air: × exp(−\|height above the path\| / 10 m) |
| `centreline` | 0.3 /s | in the air: × exp(−\|across\| / 15 m) |
| `speed` | 0.2 /s | in the air: × exp(−\|airspeed − 65 knots\| / 3 m/s) |
| `progress` | +5 | per 1000 m flown towards the aiming point |
| `smooth` | −0.02 /s | as above |
| `touchdown` | +20 | at the first touchdown on the runway × quality (1 under 1 m/s of sink, 0.4 at 2.5, 0 from 3.5) |
| `roll` | +10 | per 30 m/s lost on the runway after touchdown |
| `success` | +30 | once |
| `crash` / `envelope` | −30 / −15 | once |

Success: stopped (under 1 m/s) on the runway with every touchdown under 2.5 m/s of sink.
Failed: stopped on the runway after a harder touchdown. Envelope: touching down off the runway or
leaving it, over 150 m above the path, off the centreline by more than 300 m (or 30 % of the
distance to go), over 90° off the heading, bank over 70°, flying past the far end. Time limit
240 s.

### `circuit`
Starts like `takeoff`. `Guidance` leads round a left-hand pattern and tells the leg in the
observation: upwind to 150 m, crosswind, downwind 1000 m to the left at 200 m and 85 knots, base
2500 m before the threshold, final.

| reward | default | |
|---|---|---|
| `track` | 0.2 /s | × (0.4 exp(−\|heading error\| / 0.2) + 0.4 exp(−\|altitude error\| / 20 m) + 0.2 exp(−\|airspeed error\| / 3 m/s)) for the leg's targets |
| `speed`, `climb` | +5, +10 in all | on the upwind leg, as in `takeoff` |
| `leg` | +10 | each leg completed |
| `smooth` | −0.02 /s | as above |
| `touchdown`, `roll` | +20, +10 | on final, as in `landing` |
| `success` | +50 | once |
| `crash` / `envelope` | −30 / −15 | once |

Success and failed as in `landing`, after the whole circuit. Envelope: on the ground off the
runway or before final, over 500 m above the runway, over 6 km from the aiming point, bank over
70°. Time limit 480 s.

### `race`
From standstill on the runway (as `takeoff`): take off, turn back and land on the **same runway**,
in either direction, until stopped, in the least time. The time is the episode's, from the first
observation to the stop. The rules of a valid flight, all of them parameters:

| rule | default | |
|---|---|---|
| `min_height` | 120 m (400 ft) | the wheels must have been this high above the runway before landing |
| `turn_back` | 150° | the heading must at some time have been this far from the takeoff direction |
| `max_sink` | 2.5 m/s (about 500 ft/min) | at every touchdown |
| `max_touchdown_roll` | 10° | bank at a touchdown (a wing tip on the ground is a crash anyway) |
| `max_touchdown_crab` | 15° | between the nose and the runway, either way, at a touchdown |
| `stop_speed` | 1 m/s | below this on the runway it has stopped: success |

A touchdown before the flight has begun (the wheels `airborne_height`, 10 m, up) is part of the
takeoff. Any later touchdown before both the height and the turn, or breaking a touchdown rule, is
**failed**; off the runway, on the ground at any time or at a touchdown, is **out_of_envelope**.
The aircraft's own limits make a crash (a wing, the propeller or the tail on the ground, the gear
broken by more than 6 g). Envelope in the air: bank over 60°, pitch over 30°, over 600 m above
the runway, over 5 km from it. Time limit 480 s.

| reward | default | |
|---|---|---|
| `time` | 0 /s | always (off: the clock is paid by `fast`, so an early crash is never cheaper than flying on) |
| `speed`, `climb` | +5, +10 in all | before `min_height`, as in `takeoff` |
| `climbed`, `turned` | +5, +10 | once each |
| `approach` | +5 | per 1000 m nearer the runway, after both |
| `touchdown`, `roll` | +20, +10 | as in `landing` |
| `success` | +100 | once |
| `fast`, `par` | +2, 300 s | on success, per second under `par`: the race is against the clock |
| `crash` / `envelope` (and failed) | −50 / −30 | once |

`Guidance::race` gives `leg` 1 (`UPWIND`) until 120 m, 2 (`TURNING`) until the heading is 150°
from the takeoff direction, then 3 (`RETURNING`); the targets are the centreline and 150 m at
74 knots, then the reverse heading, then the centreline and 3° path of the runway direction nearer
the heading at 65 knots. The runway terms of the observation stay in the takeoff direction.

`cargo run --release -p sim-env --example race -- eval <competitor>...` flies competitors on
the same 100 seeds in calm air and 100 with wind (up to 8 m/s from any direction, turbulence up to
0.3) and prints success and times; `search` looks for the quickest autopilot circuit (`Pattern`),
`data` writes the records `flight-train imitate` learns from, `fly` prints one flight. See the
comment at the top of `examples/race.rs` and `trainer/RACE.md` for the results.

## Flying a trained model in the simulator

The trainer saves the policy in the model file of [PROTOCOL.md](PROTOCOL.md#model-file): a JSON
header and raw `f32`. `ModelPilot` loads it and is a `sim_core::Pilot` like any other. It builds
its observation with the same `observation::write` and `Guidance` as the environment and moves the
controls with the same `ActionMapper` at the same decision rate; `tests/env.rs` checks that a
pilot in a plain `World` flies bit for bit the flight the environment flies with the same model.
A file trained on another observation layout is refused, naming the first field that differs.

A model of `hover` or `gates` flies a multirotor: its sticks, from the multirotor's observation
(`Model::fields`, `ModelPilot::fly_multirotor`); it needs its `Guidance` given (the point, or the
course with `Guidance::gates`), and leaves an aircraft alone, as an aircraft's model leaves a
multirotor. The game races `gates` models against the player (`--rival`, `crates/games/flight/src/race.rs`).

What the game does with an aircraft's (`flight --model <file>[:<task>]`, `crates/games/flight/src/ai.rs`):

```rust
use sim_env::ModelPilot;

let mut pilot = ModelPilot::load("models/landing.fltmodel")?;   // once, per AI aircraft
// every simulation step, before world.step():
world.fly(plane_id, &mut pilot);
// whenever that aircraft is placed somewhere new:
pilot.reset();
```

The pilot works out what to do from the model's task and what it sees: the runway is the nearest
airport's, in the direction the aircraft points; `hold` keeps the present heading and altitude at
95 knots. To choose, give it a `Guidance`:
`ModelPilot::load(path)?.with_guidance(Guidance::landing(Runway { airport, reversed }))`.
It moves yoke, pedals and throttle (and flaps or trim if trained with them).

To know when its episode would have ended, the game keeps a `Referee` beside the pilot: built from
the task, the same `Guidance` and the aircraft as it is, and told about every `World::step`, it
judges each decision interval exactly as `Env` does (`tests/env.rs` checks the score and the
ending against the environment's).

Headless, with the score an environment would give:

```sh
cargo run --release -p sim-env --example fly_model -- --write-example leveller.fltmodel
cargo run --release -p sim-env --example fly_model -- leveller.fltmodel --episodes 5
cargo run --release -p sim-env --example fly_model -- --random --task landing --episodes 200
```

`wings_leveller()` is a model written by hand, before any training exists: one linear layer
(34 → 4, direct actions) that levels the wings, turns to the target heading and holds altitude and
speed. It goes through the same file and the same pilot as a trained one.

## Baselines (200 episodes each, calm air)

| task | random actions | hand-made leveller |
|---|---|---|
| `hold` | −19.5, lasts 8.7 s, all out of envelope | **+28.2**, all reach the 60 s limit |
| `takeoff` | −12.4, 28.6 s, all out of envelope | −7.0, 9.7 s, runs off the runway |
| `landing` | −14.5, 7.6 s, 14 % crashed, rest out of envelope | +32.8, 107 s, flies the approach and overflies the runway |
| `circuit` | −13.6, 28.2 s, all out of envelope | −12.7, 9.7 s |

The scripted pilots of `tests/env.rs` (simple rules over the observation, direct actions) complete
`takeoff` (return 41) and `landing` (114 to 148 on three seeds), so both are solvable with what a
model sees and can do.

## Speed

`cargo run --release -p sim-env --example bench -- hold 2`, random actions, M-series Mac with 6
performance cores (other builds were running on the machine at the time):

| | env steps/s | simulated s per wall s |
|---|---|---|
| one environment, one thread | 24 300 | 2 430 |
| 96 environments, 6 threads | 79 600 | 7 960 |
| 384 environments, 6 threads | 104 000 | 10 400 |
| through the socket, Swift client, 384 environments | 99 400 | 9 940 |

The Unix socket and TCP measure the same. `World::observe` is not on the hot path: the
environment builds the `Observation` itself once per decision, without the nearest-airport search.

## Tests

`cargo test -p sim-env` (about 4 s): determinism per task with wind and turbulence, the
observation, starts and endings, a scripted takeoff and landing, the race round a course's gates, `VecEnv` against single
environments, the model file against a reference forward pass, the hand-made model flying, pilot
against environment, and the server in the raw bytes of `PROTOCOL.md`, over TCP and a Unix socket.
