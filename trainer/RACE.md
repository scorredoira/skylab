# The race: take off, turn back, land on the same runway

Who takes off, turns back and lands on the same runway until stopped, in the least time, any way
it likes. The task is `race` in `sim-env` (rules and rewards in
[`crates/sim/sim-env/README.md`](../crates/sim/sim-env/README.md#race)); in short:

- Start standing 40 m into the home runway of world 1 (1000 × 24 m, elevation 257 m), either
  direction, throttle closed.
- Valid flight: the wheels reach **120 m (400 ft)** above the runway, the heading at some time is
  **150° or more** from the takeoff direction, and it lands on the **same runway, either way**.
- Valid touchdown, every one: on the runway, sink under **2.5 m/s** (≈ 500 ft/min), bank under
  **10°**, nose within **15°** of the runway; nothing broken (the simulator's own crash rules:
  wing, propeller or tail on the ground, gear over 6 g). Then it stays on the runway until it is
  **stopped** (under 1 m/s). Leaving the runway, bank over 60°, pitch over 30° or 5 km away ends it.
- Time: from the first observation (after 0.5 s standing) to the stop. Limit 480 s.

## Results

Every competitor flies the same 200 episodes, none of which any search or training saw: seeds
1–100 in calm air and seeds 1–100 with wind up to 8 m/s from any direction and turbulence up to
0.3. Times are of the successful flights.

| competitor | calm: success, time | wind: success, median time (best) | all 200 |
|---|---|---|---|
| **Tuned autopilot** (`models/race.pattern`) | **100 %, 148.5 s** | 90 %, 150.9 s (146.2) | **95 %** |
| Network, imitation + DAgger | 100 %, 151.0 s | 60 %, 151.9 s (149.1) | 80 % |
| Network, DAgger + PPO fine-tuning (its best checkpoint) | 100 %, 167.2 s | 18 %, 170.5 s | 59 % |
| Standard autopilot (circuit 300 m × 1200 m) | 100 %, 316.1 s | 100 %, 318.0 s (287.4) | 100 % |
| PPO from scratch, 40 M steps | 0 % (never takes off) | 0 % | 0 % |

The winner flies the race in 148.5 s in calm air (touchdown at about 135 s), less than half the
standard circuit. In wind it fails one flight in ten, all the same way: the wind carries it wide
in the turn onto final, it is still turning 40 m up and crosses the centreline, and it touches
down 25–60 m beside the runway. It always lands the other way, whatever the wind.

## The competitors

**Standard autopilot** (`Goal::Circuit`, key L in the game): a textbook circuit, 300 m high and
1200 m wide, crosswind turn 200 m past the runway, 2200 m final at 66 kt.

**Tuned autopilot**: the same autopilot with its circuit as parameters (`sim_core::Pattern`:
heights, widths, final length, aiming point, glide angle, when to turn, bank, every speed, flaps,
flare height, and landing the other way in a teardrop). A cross-entropy search
(`race search`, three runs) tried some 2000 patterns, each on 16 to 32 fresh episodes half of
them windy, scoring the mean time with a failure counted as 400 s. What it found: take off,
turn 45° off the climb at once (from 20 m), keep climbing at 75 kt while drifting 600 m to the
side, turn back in a teardrop at 33° of bank and land **in the opposite direction**, on a
1260 m, 6° final at 60 kt with flaps 20, aiming 50 m past the threshold. The file is
[`models/race.pattern`](../models/race.pattern).

**Network (imitation + DAgger)**: a 34 → 512 → 512 → 6 network (direct yoke, pedals, throttle,
flaps and trim, a decision every 0.1 s) cloned from the tuned autopilot with `flight-train
imitate`, then DAgger: it flies, the autopilot watches and says what it would have done, and it
learns again on everything. Two things were needed: hiding the positions of yoke, pedals and
throttle from the network (cloned from flights where the action nearly equals them, it learnt to
copy them and never corrected anything), and an autopilot that takes the controls as they are
before each label (`Autopilot::follow`), or its integrators drift away from the flight it watches.

**Network + PPO fine-tuning**: PPO from the DAgger network with the race reward (the clock is
paid at the finish: +2 per second under 300 s), small learning rate and noise, KL limit.

**PPO from scratch**: PPO on the race reward alone, incremental actions with flaps.

## What did not work, and why

- **PPO from scratch** learnt to stay on the runway: any attempt ends in a crash or off the runway
  long before the first success, so standing still with the throttle closed until the time limit
  is the best it finds. The race is a long chain of skills with one reward at the end; random
  exploration does not get there.
- **Plain behaviour cloning** of the autopilot failed every flight: with the yoke positions in its
  input the network copied them (copycat), and without DAgger it never saw how to recover.
  Hiding them and DAgger (about ten rounds, 6 M decisions in all) gave 79–80 %.
- **PPO fine-tuning** of the cloned network made it worse at every learning rate tried (3e-5 and
  1e-5, noise 0.08 and 0.05, KL limit): the clone is on a knife edge, the exploration noise alone
  takes its success from 80 % to some 25 %, and the updates follow that noisy signal. Its best
  checkpoint is slower (167 s) and fails more in wind.
- **The search** is what worked: 18 numbers of an autopilot that already flies well, scored on
  fresh windy episodes, converge in about 20 generations. The network only matches the
  autopilot it copies (151.0 s against its 150.8 s) and is less robust in wind.

What is left: the turn onto final in wind (the wind failures), choosing the landing direction
from the wind the takeoff roll measures (today it always comes back the other way), stopping the
climb at 120 m instead of going on to about 135 m before turning back, and a network trained on
the final pattern with more DAgger rounds in wind before trying RL again with much less noise on
trim and flaps.

## Watching it

From the worktree (`.worktrees/trainer`), with the memory cap:

```sh
scripts/run.sh --model models/race.pattern --ext
```

The race aircraft is the second one: press **N** to put the outside camera on it (Z and X swing the
camera, V goes back to your own cockpit). The status line says who flies it, the seconds of the flight running,
and how the last one ended with its time ("last flight: success in 148.5 s"); it starts again
4 s after each ending. It takes off into the wind, from the other end of the runway if your own
aircraft stands at this one. Any network flies the same way:
`--model <file>.fltmodel` (its task is in the file).

## Reproducing

```sh
cargo build --release -p sim-env --examples
R=target/release/examples/race
$R eval autopilot autopilot:models/race.pattern --episodes 100          # the table's two autopilots
$R search --from models/race.pattern --sigma 0.08 --out new.pattern     # search again from it
$R data models/race.pattern --out d0.bin --episodes 600                 # the autopilot's flights
flight-train imitate --data d0.bin --out bc0 --hidden 512,512 --epochs 40
$R data models/race.pattern --model bc0/model.fltmodel --beta 0 --out d1.bin --episodes 400
flight-train imitate --data d0.bin,d1.bin --out bc1 --hidden 512,512 --epochs 40   # and so on
$R eval bc1/model.fltmodel --episodes 100
flight-train --task race --init bc1/model.fltmodel --gamma 0.999 --lr 1e-5 --log-std -3 ...
```
