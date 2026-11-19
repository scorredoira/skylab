# The gates race: the FPV quad round Pinewood Airfield

A network that races the 5-inch FPV quad round the built-in course, Pinewood Airfield
(`assets/courses/pinewood.ron`: 17 gates, an arch, square gates, a slalom of flags, a dive gate, a
gauntlet of hoops under the pines, a ladder and a hairpin), two laps from the start's pad, as the
game races it. The task is `gates` in `sim-env` (observation, actions and rewards in
[`crates/sim/sim-env/README.md`](../crates/sim/sim-env/README.md#the-multirotors-race-gates)): the
quad's 26 numbers and the next two gates (12 more), the four sticks as actions (direct: the
throttle at `(a + 1) / 2`, the others where `a` is), a decision every 0.05 s. The result is
[`models/gates.fltmodel`](../models/gates.fltmodel) (38 → 512 → 512 → 4, 1.1 MB), and it races
against the player in the game: see "Watching it". The forest course, Deepwood, has its own network
in the race quad: [`DEEPWOOD.md`](DEEPWOOD.md).

**Since then** quads strike trees and gates as they strike each other (`sim_core::multirotor`'s
`Multirotor::hit`: propellers break on them, chip on a graze, the frame bounces off; before, a quad
that touched one slower than 10 m/s was put back where it was a step before) and a crown's twigs
break propellers. The tables below are from before that. The same 400 races since: the network 100 % in calm air (173.6 s, as before) and
87.5 % in wind (93.8 % in all; its failures flips past 85° after a propeller struck a pine or a
gate), the racer 100 % and 75 % (87.5 %), the racer deciding every 0.05 s 100 % and 75 %. A PPO run
from the network under the new contact (8 M steps, a crash costing 60, as Deepwood's last run) did
not make it surer (73 to 94 % of 300 races on other seeds, against its 93 % there): the hoops under the pines are as tight as they were,
and a touch now costs a propeller. It is kept as it was.

## Results

Every competitor flies the same 400 races, none of which training saw: seeds 1000–1199 in calm
air and the same seeds with wind of 0 to 6 m/s from any direction and turbulence 0 to 0.3. A
success is both laps from the pad with no crash and every gate (a missed gate must be flown again,
so a success has flown them all; none of the successes below missed one on the way). Calm air is
the same race every time (no randomness in it). Times are the median of the successes.

| competitor | calm: success, race (laps) | wind: success, race | all 400 |
|---|---|---|---|
| **Network, DAgger + PPO** (`models/gates.fltmodel`) | **100 %, 173.6 s (88.4 + 85.2)** | **99.0 %, 174.3 s** | **99.5 %** |
| Racer (`course::racer::Racer`, deciding every 1/240 s) | 100 %, 177.3 s (90.2 + 87.1) | 86.0 %, 177.5 s | 93.0 % |
| Racer deciding every 0.05 s, as the network does | 100 %, 178.9 s (90.9 + 87.9) | 85.0 %, 178.3 s | 92.5 % |
| Network, imitation + 8 rounds of DAgger | 100 %, 177.7 s | 80.5 %, 177.9 s | 90.2 % |
| Network, imitation only (behaviour cloning) | 0 % (hovers by the dive gate) | 0 % | 0 % |

The network is the quickest and the surest: 3.7 s faster than the racer over the two laps (1.8 s
on the first, 1.9 on the second), and it finishes 99 races in 100 in wind where the racer finishes
86. Of its two failures one is a crash, one hovering by a gate until the time limit. The racer's
failures are all the same: the wind carries it into a pine on the way from the forest's first
square gate to the first hoop.

### From the side pads

In the game the quads start side by side, a pad each, 3 m apart across the start (the player's in
the middle). The network was trained from the pad's middle only, so it was raced from pads off it,
the same 400 races (`examples/gates.rs score ... --pad <m>`, metres to the right; negative is left):

| start | network: calm, wind, all | racer: calm, wind, all |
|---|---|---|
| middle | 100 %, 99.0 %, 99.5 %; 173.6 s | 100 %, 86.0 %, 93.0 %; 177.3 s |
| 2.5 m left | 100 %, 99.0 %, 99.5 %; 173.9 s | 100 %, 86.0 %, 93.0 %; 177.3 s |
| 2.5 m right | 100 %, 99.0 %, 99.5 %; 173.6 s | 100 %, 86.5 %, 93.2 %; 177.3 s |
| 3 m left | 100 %, 99.0 %, 99.5 %; 173.9 s | 100 %, 86.0 %, 93.0 %; 177.3 s |
| 3 m right | 100 %, 98.5 %, 99.2 %; 173.6 s | 100 %, 86.0 %, 93.0 %; 177.3 s |
| 5 m left | 100 %, 99.0 %, 99.5 %; 174.0 s | 100 %, 86.0 %, 93.0 %; 177.4 s |
| 5 m right | 100 %, 99.0 %, 99.5 %; 173.7 s | 100 %, 86.0 %, 93.0 %; 177.4 s |

(times: the median race in calm air.) It does not lose its way from any of them: as sure as from
the middle, 0.3 s slower from the left, as quick from the right. No training was needed for it;
the game's rivals start from the side pads (`game.rs`'s `rival_pad`).

## How

**The expert**: the course's racer (`sim_core::course::racer::Racer`), the pilot that proves a
course can be flown: it aims along the line through the next gate's opening at a speed it can turn
at there, and works out tilt and thrust from the velocity it wants. Held for 0.05 s it flies as well
as deciding every step (0.1 s costs some 10 points of success in wind), so the network decides at
20 Hz.

**Imitation, then DAgger** (`sim-env`'s `examples/gates.rs data`, `flight-train imitate --task
gates`): 400 races of the racer (half from the pad, half in flight 20 m before a gate drawn at
random; every other one windy; both laps), 1.2 M decisions, cloned into a 256 × 256 network. Cloned
alone it flies the first five gates and then hovers by the dive gate for ever: the racer's way
through it (over the gate, then down through it) hangs on a flag of its own the network does not
see. DAgger fixes it: the network flies, the racer watches each decision (it takes over the race
where it is, `Racer::take_over`) and says what it would do, and the network learns again on all of
it. Round 1: 3 % of the races; round 3: 87 %; round 8 (512 × 512, 7.6 M decisions in all): 90 %,
the racer's own level less its wind failures. As for the C172, the stick positions are hidden
from the network (`throttle`, `roll_stick`, `pitch_stick`, `yaw_stick`: with direct actions they
are the last action, and it would copy them).

**PPO fine-tuning** (`flight-train --task gates --init`): from the DAgger network, its critic
fresh, the task's own reward (progress round the course, +5 a gate, +50 at the finish, −20 for a
crash, the clock), both laps from the pad or before a gate, wind 0–6 m/s and turbulence 0–0.3, a
tiny exploration noise (std 0.05) and learning rate (1e-5 falling to 1e-6), KL limit 0.02, 128
quads. Unlike the C172's race (`RACE.md`), where every PPO run made the cloned network worse, here
it helped at once: 10 M steps (7 minutes on the CPU) took it from 80 % to 99 % in wind and 3.7 s
quicker. The race gives a reward at every gate, not one at the end, and the clone's mistakes
(the trees in the wind, braking too early) are small corrections away. Checkpoints were chosen by
racing them on other seeds (5000–5299, windy), not by their training return: the one kept is
`ppo1/checkpoint-009830400` (299 of those 300). A second run from it (lr 5e-6, 25 M steps) went on
getting quicker (172.2 s in calm air, 100 % on the table's races at 10 M steps) but then less sure
(78 % at 25 M), and its quickest checkpoint, in the game, starting 2 m behind the pad's middle,
hovered for two minutes by the first flag: kept out. The one kept finishes from there.

Wall time, all on the CPU but the imitation (MLX on the GPU, 30 s a round): the racer's data 7 s,
each DAgger round about 1 minute, PPO 10 minutes (+ 17 for the second run), the scoring 30 s a
model: under an hour of machine time, 8 threads.

## Watching it

```sh
scripts/run.sh --ai --course pinewood
```

The race at Pinewood with two rivals on the pads beside yours: the network (NET, red) on the left
and the racer (RACER, blue) on the right. 3, 2, 1, GO: they go; fly with them, or press **N** to put the camera on the next
quad (its FPV camera; **V** for the camera behind it), N again for the next. The display shows the
race of the quad watched (WATCHING NET), its position (P1/3) and everyone in order with their lap
and next gate, then their time; each other quad's name over it. **1** starts the race again.
Straight onto the network, from behind:

```sh
scripts/run.sh --ai --course pinewood --watch 1 --ext
```

Any gates model, or the racer alone: `--rival <file>.fltmodel`, `--rival racer` (repeatable), or
`--model <file>.fltmodel` (a gates model is a rival); without `--course`, at Deepwood (the forest
course, `DEEPWOOD.md`), whose network `--ai` takes there.

## Reproducing

```sh
cargo build --release -p sim-env --examples        # also target/release/flight-env, which flight-train runs
(cd trainer && swift build -c release)
G=target/release/examples/gates; FT=trainer/.build/release/flight-train
$G score racer held@0.05 --episodes 200 --first-seed 1000                # the racer's lines of the table
$G data --out d0.bin --dt 0.05 --episodes 400 --threads 8                 # the racer's races
$FT imitate --task gates --data d0.bin --out bc0 --hidden 256,256 --epochs 30 --dt 0.05
for k in 1 2 3 4 5 6 7 8; do                                              # DAgger (512,512 from round 5)
  $G data --out d$k.bin --dt 0.05 --episodes 300 --first-seed $((100000 + 1000 * k)) \
     --model bc$((k - 1))/model.fltmodel --beta 0 --threads 8
  $FT imitate --task gates --data d0.bin,d1.bin,...,d$k.bin --out bc$k --hidden 512,512 --epochs 30 --dt 0.05
done
$FT --task gates --init bc8/model.fltmodel --fresh-critic --critic-warmup 15 \
    --set config.laps=0 --set config.time_limit=300 --set air.wind_max=6 --set air.turbulence_max=0.3 \
    --envs 128 --rollout 256 --minibatches 8 --epochs 4 --lr 1e-5 --lr-end 1e-6 --gamma 0.995 \
    --log-std -3 --target-kl 0.02 --hidden 512,512 --steps 15000000 --threads 8 --device cpu --out ppo1
$G score ppo1/checkpoint-*.fltmodel --episodes 300 --first-seed 5000      # choose a checkpoint
cp ppo1/checkpoint-009830400.fltmodel models/gates.fltmodel
$G score racer models/gates.fltmodel --episodes 200 --first-seed 1000     # the table
```

## What is left

- The forest in wind: most of the remaining failures, the racer's and the network's, are a pine
  on the way from the forest's first square gate to the first hoop. A racer that slows down there
  would give better data; the network might learn it with more PPO in stronger wind.
- Other quads: the network does not see them (its observation is its own quad and the gates). The
  racer gives way to a quad closing on it when flying fast; the two rivals, from their side pads,
  touch nowhere in 4 kt of wind from any of eight ways (`race.rs`'s
  `rivals_never_freeze_in_the_wind`); in 10 kt across the start they sometimes do, and break
  propellers as two quads do (`sim_core::multirotor`). Traffic in the observation would let the
  network give way too.
- Other courses: trained on Pinewood only; the observation is the next two gates, so it may fly
  another course, untested.
- A `hover` model: not trained (the game would also need to place a model's quad; `ai.rs` only flies
  aircraft).
