# AI control: pixels and the game driven from outside

The plan to let any program, in any language, drive the simulator and see it: a camera image in
the training environment, a network trained on pixels only, and the game itself answering on a
socket.

## Where it starts

- `sim-env` (`flight-env`) already serves the simulation without a window over TCP or a Unix
  socket: `hello`, `reset`, `step`, binary arrays, N environments at once (`PROTOCOL.md`). The
  observation is a vector of numbers. `describe` has `observation.image`, today `null`, kept for
  this.
- `engine::Gpu::headless` draws without a window and `read_pixels` reads the picture back; the
  games' `--shot` uses it.
- The games fly trained networks through `sim_env::ModelPilot` (`flight`'s `ai.rs` and `race.rs`,
  `drive`'s `rivals.rs`). Nothing outside the process can drive them.
- The trainer (`trainer/`, Swift + MLX) has PPO, imitation and DAgger on vectors, and trained
  networks for `gates` (Pinewood, Deepwood) and `drive` (Lakeside Park).

## What stays as it is

- `sim-env` depends on `sim-core` only: no graphics. The pictures come from a new crate above it.
- The reward comes from the simulation's true state (`task.rs`, `Referee`), whatever the model
  sees. A pixel network on `gates` or `drive` is scored by the rewards those tasks have today.
- The vector observation and protocol version 1 do not change: images are a second array, and
  only for the client that asks for them.

## Steps, in order

### 1. Measure the picture's cost

The whole plan hangs on how many small pictures per second the renderer gives. Before anything
else: `world-render` headless at 96×96 and 160×120 on Deepwood Forest and Lakeside Park, at the
lowest quality and at a "model" quality (no HUD, no cockpit, no post-processing, no MSAA), one
camera and then 64 cameras a frame. Written down in this file.

- Under about 2 000 pictures a second in total: RL from pixels is out, imitation still works
  (step 4 needs far fewer pictures).
- What costs per camera and what is shared (shadows, clouds, the sky's tables) decides how step 2
  draws many cameras.

### 2. Pictures in the environment

A new crate, `crates/sim/sim-view`, on `sim-env` and `world-render`, with its own server binary
(`flight-view`): the same protocol, plus images.

- `hello` takes `"image": { "width": 96, "height": 96, "channels": ["rgb"], "camera": "fpv" }`.
  `describe.observation.image` gives back the size, the channels, the bytes per pixel and the
  camera. `observations` and `step_result` carry the pictures as one more array after the ones
  they have, `u8`, environment by environment.
- Cameras: the vehicle's own (the quad's FPV camera with its tilt, the car's bonnet, the pilot's
  eyes without the cockpit). One per task by default.
- All environments share one world, as `VecEnv` does. Their cameras are drawn into one texture,
  a tile each, and read back once per step.
- The hour and the weather are per episode, from its seed, within ranges given in `params`
  (`view.hour_min`, `view.hour_max`, `view.visibility_min`...), so a network does not learn one
  light.
- The same seed and actions give the same pictures on the same machine; a test holds one picture
  to the pixel, as the `--shot` tests do.
- `sim-env` gets only what it needs to be stepped from outside with its states in view; if
  `VecEnv` already gives that, nothing.

### 3. Depth and segmentation

Two more channels to ask for in `channels`: `depth` (metres, one `f32` or a `u16` in a stated
range) and `class` (one byte: sky, ground, track, gate, tree, building, water, vehicle). They
come from the passes the renderer already draws. For training and for telling what a network
lacks: one that fails on `rgb` and passes on `class` has a seeing problem, not a flying one.

### 4. A network that only sees

In `trainer/`: a small convolutional network (the picture, the last few pictures stacked, and the
last action; no other number) taught by the network that already flies the task on vectors.

- `flight-train imitate --pixels`: the teacher flies, the server sends vector and picture, the
  student learns the teacher's action from the picture. Then DAgger, as on `gates` today: the
  student flies, the teacher says what it would have done.
- First `drive` on Lakeside Park (the road fills the picture, the task is the simplest to see),
  then `gates` on Pinewood, then Deepwood.
- Done when the student laps without crashing in 9 of 10 episodes with new seeds, hours and
  weather, and its lap time is within 15 % of the teacher's. Written up as `trainer/PIXELS.md`.
- RL from pixels with no teacher (PPO on the picture) is tried only after this works, and only if
  step 1 gave the speed.

### 5. The pixel network in the game

- The model file gains convolution layers and says which camera and size it wants; `Model` and
  `ModelPilot` run them in plain Rust, as they run the dense ones.
- The game draws that small camera for each such pilot each decision and hands it the picture:
  `--rival net` and `--ai` with a pixel model. A key shows what the network sees in a corner.

### 6. The game driven from outside

**Begun (18 Nov 2026).** `skylab --remote [port]` opens a socket and any program flies the
player's aircraft in real time: `controls` (the yoke itself, or an attitude for the keyboard's
assist to hold; for good, or for some seconds), `state` (the flight in a cockpit's units with
its trends and means, where the aircraft is from its runway, and all of it in a line of words),
`command` (restart, pause, resume, release, runway, view, wind, autopilot). An agent flew a
whole circuit with it in real time and landed on the runway it left. The crate is `crates/games/remote` (the socket,
the client, `skylab-ctl`; its `PROTOCOL.md`), the game's side `crates/games/flight/src/remote.rs`,
and `docs/AGENT-PILOT.md` tells an agent how to fly a circuit with it. Left of what follows:
the stepped clock, the window's picture, `reset` and `step` with a task, multirotors, the driving
game, the client in Python.

The whole of it:

`skylab --remote [port]` and `skylab-drive --remote [port]`: the game opens a socket and one
client flies the player's vehicle. Same framing as `PROTOCOL.md`, one environment.

- `hello` chooses real time (the game runs, the last action holds) or stepped (the game waits;
  `step` advances a given time and answers).
- `step_result` gives the vehicle's vector, as `sim-env` builds it. A `frame` request gives the
  window's picture as PNG at any size, and `state` a JSON of what the HUD shows (speed, height,
  heading, lap, the next gate, the autopilot's modes).
- Besides the sticks, orders one level up for slow models: the C172's autopilot (heading,
  altitude, speed, approach), "back to the runway", the menu's choices (aircraft, place, hour,
  weather). A model that answers in seconds flies with these and stepped time.
- `skylab-ctl`, a command-line client: `skylab-ctl step 0.5 --frame out.png`, `skylab-ctl state`,
  `skylab-ctl autopilot --heading 270 --altitude 3000`. Whatever can run a command can fly.
- A client in Python with the standard library only, beside the Swift one, written from the
  protocol document.
- Local only (`127.0.0.1` or a Unix socket), off unless asked for.

Step 6 does not need steps 1 to 5 and can go first.

## Not in this plan

- One program with a menu for both games (`flight` and `drive` as libraries).
- The web version: sockets and the trainer are native only.
- Networks that see the other vehicles; multi-agent training.
