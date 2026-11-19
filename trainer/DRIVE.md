# The driving network: the GT coupé round Lakeside Park, dry and wet

A network that drives the GT coupé (`assets/cars/gt`, its anti-lock brakes, traction and
stability control on, as in the road car) round Lakeside Park (4.5 km), from a standing start or
anywhere round the lap at speed, dry or in the rain. The task is `drive` in `sim-env` (observation,
actions and rewards in
[`crates/sim/sim-env/README.md`](../crates/sim/sim-env/README.md#the-car-drive)): the car's 36
numbers (how it moves, its engine and pedals, its tyres' slips, where it is on the circuit, the
circuit's bends and the racing line ahead with its speeds, the rain, the water under its tyres and
on the road 20, 40 and 70 m ahead), the steering and the pedal as actions (direct), a decision every
0.05 s. The result is [`models/drive.fltmodel`](../models/drive.fltmodel) (36 → 512 → 512 → 2,
1.1 MB). It races in the game as a rival: `scripts/drive.sh --race --rival net`.

## Results

Every competitor drives the same 200 drives, none of which training saw: seeds 1000–1099 dry and
the same seeds in rain of 0.2 to 0.8 (drawn by the seed), the circuit's water as long rain leaves
it (a film of 0.26 to 0.59 mm, puddles in the hollows; `sim_core::car::wet`). Two laps from the
grid; a success is both laps with no crash and never 15 m off the asphalt, the wrong way round or
standing still. Times are medians of the successes; the flying lap is the second.
(`examples/drive.rs score`, on bill.)

| competitor | dry: success, first + flying lap | wet: success, first + flying lap (best) | all 200 |
|---|---|---|---|
| **Network, imitation + 8 rounds of DAgger** (`models/drive.fltmodel`) | **100 %, 122.10 + 115.65 s** | **100 %, 158.10 + 154.40 s (133.50)** | **100 %** |
| Racing driver (`car::driver::Driver`, deciding every 1/240 s) | 100 %, 121.75 + 115.35 s | 100 %, 159.20 + 154.95 s (134.20) | 100 % |
| Racing driver deciding every 0.05 s, as the network does | 100 %, 121.80 + 115.45 s | 100 %, 158.30 + 154.10 s (133.80) | 100 % |

The network drives as its teacher does: dry, 0.3 s a lap slower than the driver deciding every
step; in the rain as quick (half a second quicker in the median, 0.7 s on the best). The wet laps
are slower with the rain: 134 s in a drizzle (0.2), 152 s in steady rain (0.5), 169 s in a downpour
(0.8), against 115.4 s dry, from the grip a road tyre keeps on a wet road and the puddles it must
slow for.

In the game, from the pole against two racing drivers over two laps (`rivals.rs`'s test
`the_network_races_the_racing_drivers`): the network wins in 237.5 s (best lap 115.68 s), the
quickest racing driver 0.3 s behind, both the other cars finishing. The network does not see the
other cars; the racing driver watches the traffic for it and its pedals take over while a car is in
its way.

### Before it

- Imitation alone (`bc0`): 0 % dry, 20 % wet (40 drives, seeds 5000–5019): it runs off within the
  first corners.
- One round of DAgger: 97.5 % (100 % dry, 95 % wet); from round 2 on, 100 % of every scoring
  (seeds 5000–5019, dry and wet), the laps within 0.5 s of the teacher's.
- PPO from round 8 (`flight-train --task drive --init bc8 ...`, 10 M steps, 128 cars, the task's
  own rewards, half of them wet, lr 1e-5 to 1e-6, exploration std 0.05): every checkpoint, scored on
  seeds 6000–6019, was slower than its start (116.8 to 119.2 s dry against 115.6 s) and some less
  sure in the rain (95 %). As the C172's race (`RACE.md`), the clone of a good teacher is not made
  better by PPO with these rewards; it is kept out.

PPO found a fault in the car first: with the exploration's noise on the steering (std 0.05 at
20 Hz) the stability control braked wheels all the time and the car lost 40 s a lap. Its yaw
reference now lags the steering as the car's own yaw does, with wider bands (`car.rs`,
`stability`): the same noise costs 3 s a lap.

## How

**The teacher**: the racing driver (`sim_core::car::driver::Driver`): pure pursuit of the
minimum-curvature line at its speeds, threshold braking, and on a wet road the line's speeds for the
film it sees under its wheels (`RacingLine::respeed` with the wet grip at each speed) and slower for
the puddles it sees ahead. Held for 0.05 s it drives as well as deciding every step.

**Imitation, then DAgger** (`examples/drive.rs data`, `flight-train imitate --task drive`; the
script is `trainer/drive-pipeline.sh`): 400 drives of the driver (30 % from the grid, the rest on
the line anywhere round the lap at 0.5 to 0.9 of its speed; half of them in rain of 0.2 to 0.8; one
lap), 1.1 M decisions, cloned into a 256 × 256 network, the steering and the pedals hidden from it
(with direct actions they are its last action). Then eight rounds of DAgger: 300 drives of the
network, the driver saying what it would do at each decision, the network learning again on all of
it (512 × 512 from round 3), some 6 M decisions in all.

Wall time, on the Mac mini M4 Pro (12 threads): the driver's data 1 minute, each DAgger round 2 to
4 minutes (data and imitation, MLX on the GPU), 25 minutes for all; PPO 19 minutes.

## Reproducing

```sh
cargo build --release -p sim-env --examples --bins     # also target/release/flight-env
(cd trainer && swift build -c release)
trainer/drive-pipeline.sh runs/drive 12 8              # data, imitation, 8 rounds of DAgger, each scored
cp runs/drive/bc8/model.fltmodel models/drive.fltmodel
D=target/release/examples/drive
$D score driver held@0.05 models/drive.fltmodel --episodes 100 --first-seed 1000 --threads 12   # the table
$D fly models/drive.fltmodel --rain 0.5 --seed 7       # one drive, a line a second
trainer/drive-ppo.sh runs/drive runs/drive/bc8/model.fltmodel 12 10000000   # the PPO run (not kept)
```

## What is left

- Traffic in the observation: the network does not see the other cars, so the racing driver
  brakes for it behind them and it never passes by itself.
- PPO with a reward that pays for lap time (a time per lap rather than a small clock) might make it
  quicker than its teacher, as it did the quads (`GATES.md`).
- Other circuits and cars: trained on Lakeside Park in the GT coupé only.
