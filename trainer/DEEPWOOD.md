# The forest race: the race quad round Deepwood Forest

A network that races the 5-inch race quad (`assets/aircraft/race5`: 6S, 500 g, 13.6:1, 175 km/h
flat out) round Deepwood Forest (`assets/courses/deepwood.ron`), two laps from the start's pads,
as the game races it. Same task (`gates`, `config.course=1`; the course names its quad, so the
environment flies the race quad), same observation, actions and rewards as Pinewood's network
(`GATES.md`), the same pipeline: imitation of the course's racer, DAgger, PPO. The result is
[`models/gates-deepwood.fltmodel`](../models/gates-deepwood.fltmodel) (38 → 512 → 512 → 4), the
course's network: `--ai` finds it by the course's key (`models/gates-<course>.fltmodel`).

## The course

A forest track 6 km west of the home aerodrome of the seed-1 world, on a slope that falls some
80 m to the south-east, 1338 m a lap along the gates. 16 gates you fly through: the start and
finish arch at the forest's edge; across the meadow (a hoop, a gate, a hoop hung from a beam);
into the wood and up the slope (a gate, a hoop, a log to fly under, a hung hoop, a hoop, a gate);
up to a high hoop (7.4 m) in a clearing open to the sky; a split-S (a 145° turn dropping 5 m to a
gate behind and below it); and the fast run home down the slope (a hoop, a hung hoop, a log, a
hoop) to the arch. No flags, no dive gate, nothing virtual.

The trail is cut through the world's own forest (`sim_core::course::Trail`): no trunk within 4.5 m
of its line (18 m across in the clearing), no crown lower than 5 m reaching over it (taller trees
keep theirs: the canopy), and the wood three times as thick within 40 m of it (more trees to a
cell of the world's 12.8 m grid). The trees are grown so in `scenery::trees`: what is drawn is
what is hit. A quad that leaves the trail meets trunks and crowns: its propellers break on them,
chip on a graze, its frame bounces off (`sim_core::multirotor`, `Multirotor::hit`).

## The racer, the teacher

The course's racer (`course::racer::Racer`) with the course's pace: top 45 m/s, 33 through a
straight gate, 13 at a hairpin, braking 12 m/s², tilting 55° at most, never under 1.2 m, and
following the ground under the canopy (`follow_ground`: the height of the last gate's opening
going over to the next one's, climbing with the ground's rise). Faster paces were tried: at 65 to
70° of tilt it flipped past 85° when braking into the high hoop and the hard turns (out of the
task's envelope) and, flying more than 4 m over the ground on the climbs, met the crowns.

## Results

Every competitor flies the same 400 races, none of which training or the choosing saw: seeds
1000–1199 in calm air and the same seeds with wind of 0 to 6 m/s from any direction and turbulence
0 to 0.3, from the start's middle pad. A success is both laps with no crash, no gate missed, and
never tilted past 85° (the task's envelope: a race quad that far over has lost a propeller or is
about to). Times are the median of the successes.

| competitor | calm: success, race (laps) | wind: success, race | all 400 |
|---|---|---|---|
| **Network, DAgger + PPO** (`models/gates-deepwood.fltmodel`) | **100 %, 121.2 s (62.3 + 58.9)** | **100 %, 121.5 s** | **100 %** |
| Racer (`course::racer::Racer`, deciding every 1/240 s) | 100 %, 130.2 s (67.0 + 63.2) | 96.0 %, 131.0 s | 98.0 % |
| Racer deciding every 0.05 s, as the network does | 100 %, 130.8 s (67.4 + 63.4) | 93.0 %, 131.5 s | 96.5 % |
| Network, PPO with the task's own rewards (`ppo1`, 10.6 M steps) | 100 %, 121.8 s | 97.0 %, 122.1 s | 98.5 % |
| Network, imitation + 10 rounds of DAgger (`bc10`) | 100 %, 130.6 s | 91.5 %, 131.4 s | 95.8 % |

From the side pads, as the game's rivals start (3 m left and right of the middle):

| start | network: calm, wind, all; race | racer: calm, wind, all; race |
|---|---|---|
| middle | 100 %, 100 %, 100 %; 121.2 s | 100 %, 96.0 %, 98.0 %; 130.2 s |
| 3 m left | 100 %, 99.0 %, 99.5 %; 121.2 s | 100 %, 95.5 %, 97.8 %; 130.2 s |
| 3 m right | 100 %, 99.5 %, 99.8 %; 121.2 s | 100 %, 97.5 %, 98.8 %; 129.8 s |

Over the 1200 races of the three pads: 99.75 % (3 failures, all in wind: a crash on a gate's
frame and a flip after a propeller struck one). It is 9 s quicker than the racer over the two laps
(4.7 s on the first, 4.3 on the second) and surer in wind. Its race in calm air (`gates fly`): 2789
m in 121.2 s, **23.0 m/s on average (83 km/h), 32 m/s at most (115 km/h)**, against the racer's
21.4 m/s and 30.1 (108 km/h); a lap is 62 s from the pad, 59 flying. Its quickest stretches are the
run home down the slope and the meadow; the slowest the climb to the high hoop and the split-S.

In the game (`race.rs`'s tests): NET and RACER from the side pads, in 4 kt of wind from eight
ways, both finish every race, NET in 120.6 to 121.7 s, RACER in 129.5 to 131.3 s; they never touch
(the closest, 0.52 m, just after the start).

## How

**Imitation, then DAgger** (`examples/gates.rs data --course deepwood`, `flight-train imitate
--task gates`): 400 races of the racer deciding every 0.05 s (half from the pads, anywhere 3 m
either side of the middle, half in flight before a gate; every other one windy), 818 000
decisions, cloned into a 256 × 256 network; then ten rounds of DAgger (300 races each of the
network, the racer labelling, 512 × 512 from round 4). Cloned alone it crashes or flips at once;
round 3 already finishes 89 % of its races; round 10 (`bc10`), on 200 races of other seeds: 100 %
calm, 91 % in wind (97 % once the crowns' fringe was thinned, below), 130.6 s.

**PPO** (`flight-train --task gates --init bc10 --set config.course=1 --set config.pad_spread=3`,
otherwise as Pinewood's: 128 quads, lr 1e-5 to 1e-6, KL limit 0.02, exploration std 0.05, 15 M
steps, 8.5 minutes on the M4's CPU). Success dipped at 2 to 6 M steps and came back; checkpoints
raced on other seeds (5000–5149, calm and windy) to choose: the run's checkpoints were near 98 % (`ppo1/checkpoint-010649600`, 99.3 % on those
300, 98.5 % on the table's), then less sure and quicker (to 118.9 s at 15 M, 97 %): with the
task's rewards (a crash costs 20, the clock 0.2 a second) it trades safety for time. A second run
from that checkpoint with a fresh critic collapsed (3 % of its races at 4 M steps); one with its own
critic at a lower rate (3e-6) stayed at 96 to 98 %. What did it was the price of a crash: a third
run from `ppo1/checkpoint-010649600` with its critic (15 updates of it alone first), a crash and the
envelope costing 60 instead of 20 (`--set rewards.crash=60 --set rewards.envelope=60`), lr 5e-6 to
1e-6, a checkpoint every 10 updates (`ppo4`). Its checkpoints were raced on seeds 5000–5149 and the
best four again on 6000–6299 (600 races): `ppo4/checkpoint-001310720` (1.3 M steps), 100 % and
99.7 %, is the one kept. Past 2 M steps it got quicker again (116.4 s at 8 M) and less sure (97 %).

Wall time, all on a Mac mini M4 Pro (12 threads): the racer's data 10 s; imitation 13 s to a
minute a round (MLX on the GPU); DAgger 15 minutes; PPO 8.5 minutes; the scoring 30 s a model.

## Reproducing

```sh
cargo build --release -p sim-env --examples        # also target/release/flight-env
(cd trainer && swift build -c release)
G=target/release/examples/gates; FT=trainer/.build/release/flight-train; C="--course deepwood"
$G data $C --out d0.bin --dt 0.05 --episodes 400 --threads 12 --pad-spread 3
$FT imitate --task gates --data d0.bin --out bc0 --hidden 256,256 --epochs 30 --dt 0.05
for k in 1 2 3 4 5 6 7 8 9 10; do                                         # 512,512 from round 4
  $G data $C --out d$k.bin --dt 0.05 --episodes 300 --first-seed $((100000 + 1000 * k)) \
     --model bc$((k - 1))/model.fltmodel --beta 0 --threads 12 --pad-spread 3
  $FT imitate --task gates --data d0.bin,...,d$k.bin --out bc$k --hidden 512,512 --epochs 30 --dt 0.05
done
P="--set config.course=1 --set config.pad_spread=3 --set config.laps=0 --set config.time_limit=300 \
   --set air.wind_max=6 --set air.turbulence_max=0.3 --envs 128 --rollout 256 --minibatches 8 --epochs 4 \
   --lr-end 1e-6 --gamma 0.995 --log-std -3 --target-kl 0.02 --hidden 512,512 --threads 12 --device cpu"
$FT --task gates --init bc10/model.fltmodel --fresh-critic --critic-warmup 15 $P --lr 1e-5 --steps 15000000 --out ppo1
$FT --task gates --init ppo1/checkpoint-010649600.fltmodel --critic-warmup 15 $P --lr 5e-6 --steps 8000000 \
    --set rewards.crash=60 --set rewards.envelope=60 --checkpoint-every 10 --seed 2 --out ppo4
$G score ppo4/checkpoint-*.fltmodel $C --episodes 150 --first-seed 5000 --pad-spread 3   # choose (then 6000, 300)
cp ppo4/checkpoint-001310720.fltmodel models/gates-deepwood.fltmodel
$G score racer held@0.05 models/gates-deepwood.fltmodel $C --episodes 200 --first-seed 1000 --pad 0   # the table (and --pad -3, 3)
$G fly models/gates-deepwood.fltmodel $C --seed 1000                     # one race, its speeds
```

## Watching it

```sh
scripts/run.sh --ai                       # Deepwood, NET and RACER on the pads beside yours
scripts/run.sh --ai --watch 1 --ext       # the camera behind the network
```

## What is left

- Faster lines: the network flies the racer's lines a little quicker; 32 m/s is its top, where the
  quad does 49. A racer with a faster pace flipped or met the crowns; PPO with the task's own
  rewards got quicker (116 s) but less sure. A reward for speed that does not pay for crashes, or a
  racer that plans its line through the trail, would give faster data.
- Other quads: the network does not see them (its observation is its own quad and the gates); in
  the game, from the side pads, the rivals come within half a metre of each other after the start.
- Other courses: trained on Deepwood only; Pinewood keeps its own network (`GATES.md`).
