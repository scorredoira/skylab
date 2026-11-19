# Flying Skylab as an agent

For an agent (or any program) that flies the Cessna 172 of a running game through
`skylab-ctl`, in real time: what to type, how the aircraft answers, and a circuit from takeoff
to landing on the same runway. The messages behind it are in
[crates/games/remote/PROTOCOL.md](../crates/games/remote/PROTOCOL.md).

Where it stands (18 Nov 2026): an agent flew an earlier version of this circuit in real time,
some 22 commands, and landed on the runway it left (217 feet per minute, 7 m from the
centreline). A second one landed 30 m from the centreline, off the paved surface, and said what
was unclear: the circuit below is that one corrected. Command by command it is flown by a test,
`cargo test -p flight the_guide_s_circuit`, which ends in a `good` landing. The tables are
measured in the simulation, in still air.

## Starting

```sh
cargo build --release -p flight -p remote
target/release/skylab --remote --aircraft c172      # the window; it starts flying, no menu
target/release/skylab-ctl command wind speed_kt=0   # still air, for a first flight
target/release/skylab-ctl state brief
```

The game runs in real time whether you look or not.

## The rules of the exercise

- Real time: `command pause` exists, and using it is not flying the exercise.
- By hand means the assist or the yoke. The autopilot (`command autopilot keys=...`) is there,
  but when the exercise is to fly, leave it: `autopilot` in the state must read `off`.
- Do not write a program that flies in your place unless you are asked to. One command at a
  time, decided by you, `--when` included.
- `crashed` ends the flight; `skylab-ctl command restart where=runway` starts another.
- **A landing is good** when `landing.verdict` says `good`: the hardest touch under 300 feet per
  minute, on the runway's surface, within a quarter of its width of the centreline, and stopped
  on it. `firm` is 300 to 500, `hard` beyond. `off the runway` and `ran off the runway` are
  what they say, however soft the touch was.

## The tools that make it possible

### `brief`: the flight in one line of words

```sh
skylab-ctl state brief
```

```
65 kt (-0.0/s, mean 65) · 380 ft up, -288 fpm (mean -313) · nose -3° · wings level · heading 193° ·
runway 19: 1832 m before the threshold, 22 m RIGHT of the centreline, GOING AWAY 3 m/s, nose 4° right
of it, 36 ft above the path (-395 fpm goes to the touchdown point) · throttle 0.62, flaps 30°,
assist holding nose -3° bank +0°
```

Sides are words, not signs: you are RIGHT or LEFT of the centreline, closing or GOING AWAY,
above or below the path. The speed comes with how fast it changes and its mean over the last
half minute. After a landing it starts with `LANDED (good): 217 fpm, 359 m past the threshold,
7 m LEFT of the centreline`.

### The assist: ask for an attitude, not for a place of the yoke

```sh
skylab-ctl controls pitch_deg=8 bank_deg=0     # nose 8° up, wings level: it goes there and stays
skylab-ctl controls bank_deg=-20               # 20° of bank to the left, held
skylab-ctl controls bank_deg=-8 for=5          # 8° left for 5 seconds, then back to what was held
```

The assist moves the yoke and the pedals to hold the nose and the bank you ask for. It is the
one a player has at the keyboard, not the autopilot: it holds no heading, no height and no
speed. Those are yours, through the attitude and the throttle. On the ground it keeps the
aircraft on its line and lifts the nose at the rotation speed.

- **What a message does not name stays as it was.** `controls throttle=0.62` leaves the
  attitude the assist holds, the flaps and everything else alone.
- **`for` is for the whole message.** `controls throttle=0.62 bank_deg=-10 for=5` sets the
  throttle for 5 seconds too, and then it goes back. What is to stay goes in a message of its
  own, without `for`.

### `--when`: act at the moment, not seconds later

```sh
skylab-ctl controls bank_deg=0 --when 'runway.nose_right_deg~-83:5' brief
```

It waits until that number of the state is there and sends the command at once; you get the
answer then. `>` and `<` compare, `~value:margin` is "near" (angles the short way round).
Several `--when` must all be true.

- While it waits it prints every 5 seconds, on the error output, where the number is and what
  it waits for. It is not hung: a downwind leg takes two minutes.
- If the number is already there, it sends at once. That is not an error: you were past the point.
- If it does not happen in `--timeout` seconds (300) nothing is sent and the exit code is 2. If
  the wait is stopped from outside, nothing was sent either.

### `--after`: read once the command has acted

The answer to `controls` is the state at the moment of sending: the bank you just asked for is
not there yet. `--after <s>` prints the state that many seconds later instead.

```sh
skylab-ctl controls bank_deg=-20 --after 4 brief     # the bank is there in 3 seconds
skylab-ctl controls bank_deg=-8 for=5 --wait brief   # a pulse: the state when it is over
```

### Every number has a name

`skylab-ctl state` prints all of them; any can go after a command to be printed, or in a
`--when`. The ones a circuit is flown by:

| Field | What it is |
|---|---|
| `runway.height_ft` | wheels above the runway. Use it for every height of the circuit; `height_ft` is above the ground below, which rises and falls |
| `runway.along_m` | metres past the threshold along the runway; negative before it |
| `runway.across_m` | metres from the centreline: positive right of it, negative left |
| `runway.across_rate_m_s` | how fast `across_m` changes (the `brief`'s closing and GOING AWAY) |
| `runway.nose_right_deg` | where the nose points from the runway's heading: positive right of it, negative left, 0 lined up, ±180 the other way. It is not the heading: the heading is a compass number, `heading_deg` |
| `runway.path_error_ft` | above (positive) or below the 3° path to the touchdown point |
| `runway.needed_fpm` | the vertical speed that takes you from here to the touchdown point |
| `turn_radius_m` | the radius of the turn you are in: a 90° turn ends that far to the side |
| `airspeed_kt`, `airspeed_avg_kt`, `airspeed_trend_kt_s` | the speed, its mean of half a minute, how fast it changes |
| `vertical_speed_fpm`, `vertical_speed_avg_fpm` | the climb and its mean |
| `roll_deg`, `pitch_deg` | right wing down positive; nose up positive |
| `flaps_deg` | where the flaps are; they take seconds to follow the lever (`brief` says `going to 30°`) |
| `landing.verdict`, `landing.sink_fpm`, `landing.across_m` | the landing, once the wheels have touched |

## How the aircraft answers, with the assist

The type's own speeds are in `skylab-ctl describe speeds`: it stalls at 53 knots clean and 48
with full flaps, rotates at 55, climbs at 74, flies the final at 66.

### The nose and the throttle give the speed and the climb

Knots and feet per minute, settled, wings level:

| Flaps, throttle | nose −3° | nose 0° | nose +3° | nose +6° | nose +8° to +9° |
|---|---|---|---|---|---|
| up, 1.0 | | | 97, +410 | 82, +520 | 77, +610 (the climb) |
| up, 0.65 | 110, −540 | 94, −195 | **79, level** | 67, level | 58, level |
| up, 0.5 | 101, −620 | 86, −340 | 73, −220 | 62, −210 | stalls |
| flaps 2, 0.5 | **68, −410** | 60, −355 | 53, −340 | stalls | |
| flaps 3, 0.65 | **65, −330** | 58, −250 | 53, −220 | stalls | |
| flaps 3, 0.5 | 61, −460 | 55, −420 | 50, −400, stall warning | stalls | |

The assist does not keep you from stalling: a nose higher than the power can carry slows the
aircraft until it does. Keep the speed above 60, and never ask for the nose up with the
throttle low and the flaps down, except for the touchdown.

### The bank gives the turn

At 80 knots; slower, the turn is a little quicker and tighter.

| `bank_deg` | Turn | Radius |
|---|---|---|
| 6 | 1.2° a second | 1600 m |
| 10 | 1.8° a second | 980 m |
| 20 | 3.6° a second: 90° in 25 seconds | 470 m at 80 knots, 340 m at 68 |
| 30 | 5.2° a second, and it loses height | 300 m |

- The bank is there 3 seconds after it is asked for, and takes as long to come off. In those
  seconds the aircraft flies on: a 90° turn at 20° needs **500 m** from where you ask for it to
  where it ends, not the radius alone.
- A turn goes on for 5° to 7° after `bank_deg=0` at 20° of bank: stop it that much early. The
  commands below already do.
- Small heading changes with a bank for a time: `bank_deg=-6 for=3.5` is about 4° to the left,
  `bank_deg=10 for=4.5` about 8° to the right.
- Wings level is not a heading held: at high power and low speed the aircraft drifts left a
  fifth of a degree a second. `bank_deg=1` flies straight in the climb.

## A circuit, step by step

Left-hand, 1000 feet above the runway. Each line is one call; `C` is `skylab-ctl`. Every
`--when` that is already true fires at once, so a point you have passed costs nothing: go on
to the next line.

**Takeoff and climb.** One message takes off: the assist keeps the line, lifts the nose at 55
knots and climbs at 77.

```sh
C command restart where=runway
C controls pitch_deg=8 bank_deg=1 throttle=1 brief
```

**Crosswind**, at 500 feet: a left turn of 90°. The nose ends 83° to 90° left of the runway
(the compass heading is another number: with runway 19 it reads about 105°).

```sh
C controls bank_deg=-20 --when 'runway.height_ft>500' brief
C controls bank_deg=1 --when 'runway.nose_right_deg~-83:5' brief
```

**Downwind, levelling off in the turn.** The climb is still on: the turn begins 900 m to the
side and ends 1400 to 1500 m out, at about 1000 feet. Nose +3° and 0.65 is level at 79 knots.

```sh
C controls bank_deg=-20 pitch_deg=3 throttle=0.65 --when 'runway.across_m<-900' brief
C controls bank_deg=0 --when 'runway.nose_right_deg~-172:5' brief
```

Look at `brief` once on the way. If the nose is not within a few degrees of 180° from the
runway, or it says GOING AWAY, a small bank for a few seconds puts it right. Expect to be 1500
to 2000 m to the side: that is fine, the base leg is just longer.

**Base**, 2500 m before the threshold: turn left, flaps 2, and descend at 68 knots.

```sh
C controls bank_deg=-20 flaps=2 throttle=0.5 pitch_deg=-3 --when 'runway.along_m<-2500' brief
C controls bank_deg=0 --when 'runway.nose_right_deg~98:5' brief
```

**Final.** Ask for the turn 500 m before the centreline, and roll out near the runway's
heading. Then full flaps and the approach power, each to stay.

```sh
C controls bank_deg=-20 --when 'runway.across_m>-500' brief
C controls bank_deg=0 flaps=3 --when 'runway.nose_right_deg~8:5' brief
C controls throttle=0.62 brief
```

**Line up.** One rule, a look every few seconds:

> The nose points towards the centreline by a tenth of the metres you are off it, 10° at most.
> 60 m RIGHT of it: nose 6° left (`nose_right_deg` −6). 15 m LEFT: nose 1.5° right. On it: nose
> along it.

Turn to that with a bank of 8° for a few seconds: 1.6° of heading for each second
(`C controls bank_deg=-8 for=3 --wait brief` is 5° to the left). As the centreline arrives, the
nose comes back along the runway: `C controls bank_deg=8 for=4 --when 'runway.across_m<10' --wait brief`
coming from the right with the nose 6° left. GOING AWAY in `brief` is what to fix first.

**The path.** `brief` says how far above or below it you are and the feet per minute that go to
the touchdown point. Above it, 0.05 less throttle; below it, 0.05 more; between 0.4 and 0.7.

**Go around** if at 100 feet you are more than 10 m from the centreline, more than 3° off its
heading or GOING AWAY: `C controls pitch_deg=8 bank_deg=1 throttle=1 flaps=1`, and fly the
circuit again. A runway is 24 to 45 m wide: 15 m off its centreline can be off its surface.

**Touchdown and stop.** Arm the two when you are lined up, at 150 feet: the nose comes up at
30 feet, and the throttle closes and the brakes go on as the wheels touch.

```sh
C controls pitch_deg=3 throttle=0.45 --when 'runway.height_ft<30' brief
C controls throttle=0 brakes=1 pitch_deg=0 --when 'runway.height_ft<1' --timeout 40 brief
C state --when 'ground_speed_kt<1' brief landing
```

From 30 feet it floats 350 to 600 m before it touches, and stops 130 m further on. `landing`
says how it went.

## When it goes wrong

| What you see | What to do |
|---|---|
| STALL WARNING, or the speed under 58 and falling | `pitch_deg` 3° lower and `throttle=1`; never ask for more nose |
| the bank is not what you asked for | the answer is from before it acted: read again, or use `--after 4`. If `brief` says `no assist`, a message with `elevator` or `aileron` turned it off: any `bank_deg` turns it on again |
| `--when` prints the same number for a long time | it is waiting, not hung: read what it says. If the number moves away from what it waits for, stop it (nothing was sent) and decide again |
| `--when` timed out (exit code 2) | nothing was sent: read `brief` and decide again |
| far wider or longer than the circuit says | fly it as it is: turn base and final by the same numbers. The final is just longer |
| lost | `brief` says where the runway is and which way the nose is from it |
| CRASHED | `skylab-ctl command restart where=runway` |

## Without the assist

`elevator`, `aileron` and `rudder` move the yoke and the pedals themselves (−1 to 1), `trim`
sets the speed the aircraft settles at, and nothing holds anything. It is much harder at one
decision every few seconds, and nobody has landed it that way yet: the first attempt never got
lined up with the runway. What was measured, with the yoke centred:

- Trim gives the speed (0: 83 knots; +0.1: 78; +0.3: 72; full flaps and +0.4: 63) and the
  throttle the climb (flaps up, trim 0: 0.65 level, 1.0 +650 feet per minute, 0.35 −500).
- After any change the nose swings for minutes, by 10 knots and hundreds of feet per minute,
  every half minute. `airspeed_avg_kt` and `vertical_speed_avg_fpm` say where it is settling.
- `aileron=0.3 for=1` leaves 10° of bank, `aileron=0.5 for=1` 18°. At cruise power the bank
  stays; under full power the aircraft rolls left by itself, a degree a second or more.
- On the takeoff run it needs right pedal, from 0.06 to 0.14 as it accelerates, and it does not
  lift off with trim 0 and the yoke centred.
- A touchdown flown on without lifting the nose is taken up to 560 feet per minute, with a bounce.
