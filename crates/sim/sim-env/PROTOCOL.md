# sim-env protocol, version 1

How a trainer in another program drives the environments, and the file a trained model is saved
in. Everything needed to write a client is here; `clients/swift/RandomAgent.swift` is one written
from this document, and `tests/server.rs` speaks it in raw bytes.

## Transport

One client at a time, over a stream socket:

- TCP on `127.0.0.1`, port 7447 unless `--port` says otherwise. Set `TCP_NODELAY` on the client.
- or a Unix-domain socket, with `--unix PATH` (a path must be shorter than 104 bytes on macOS).

The server answers every request with exactly one frame, in order. The client speaks first.
When the client leaves, the server waits for the next one (or exits, with `--once`).

## Framing

Every message, in both directions:

| bytes | type | meaning |
|---|---|---|
| 4 | `u32` little-endian | `length`: number of bytes that follow (type + payload), 1 to 67 108 864 |
| 1 | `u8` | message type |
| `length - 1` | | payload |

All integers are little-endian `u32`; all reals are little-endian IEEE-754 `f32`. JSON payloads
are UTF-8 with no terminator. There is no alignment to respect: read by offset.

| type | name | direction | payload |
|---|---|---|---|
| 1 | `hello` | client → server | JSON |
| 2 | `describe` | server → client | JSON, the answer to `hello` |
| 3 | `reset` | client → server | JSON |
| 4 | `observations` | server → client | binary, the answer to `reset` |
| 5 | `step` | client → server | binary |
| 6 | `step_result` | server → client | binary, the answer to `step` |
| 7 | `close` | client → server | empty |
| 8 | `bye` | server → client | empty, the answer to `close`; the server then closes |
| 9 | `error` | server → client | JSON, instead of the answer |

Below, `N` is the number of environments, `O` the observation length and `A` the action length,
all three given by `describe`.

## hello (1) → describe (2)

`hello` opens a session. `protocol` is required and must be `1`. Everything else is optional and
overrides the defaults the server was started with:

```json
{
  "protocol": 1,
  "client": "my-trainer",
  "task": "landing",
  "num_envs": 384,
  "threads": 6,
  "world_seed": 1,
  "decision_dt": 0.1,
  "action_mode": "incremental",
  "flaps": false,
  "trim": false,
  "params": { "air.wind_max": 5, "rewards.crash": 50, "config.time_limit": 180 }
}
```

| key | meaning |
|---|---|
| `task` | `hold`, `takeoff`, `landing`, `circuit` or `race` (an aircraft), or `hover` or `gates` (a multirotor) |
| `aircraft` | `c172` (default) or `extra` (the Extra 300L) |
| `multirotor` | `fpv5` (default, the 5-inch FPV quad): what `hover` and `gates` fly; `describe` gives `vehicle`, the one flown |
| `num_envs` | 1 to 65 536 |
| `threads` | worker threads; 0 = one per performance core |
| `world_seed` | seed of the terrain (whole number below 2^53) |
| `decision_dt` | seconds between decisions, rounded to a whole number of 1/240 s steps |
| `action_mode` | `incremental` (actions are control rates) or `direct` (actions are control positions) |
| `flaps`, `trim` | add a flap action, an elevator trim action |
| `params` | numbers of the task by name; the names and defaults are listed in `describe` |
| `client` | free text, ignored |

`hello` may be sent again at any time: the environments are thrown away and new ones built (this
is how a curriculum changes the wind or the task). After a `hello`, send `reset` before `step`.

`describe` says what was built:

```json
{
  "protocol": 1,
  "server": "flight-env 0.1.0",
  "num_envs": 384,
  "threads": 6,
  "world_seed": 1,
  "decision_dt": 0.1,
  "substeps": 24,
  "observation": {
    "len": 34,
    "clip": 10,
    "fields": [
      { "name": "airspeed", "scale": 50, "description": "indicated airspeed, m/s" },
      ...
    ],
    "image": null
  },
  "action": {
    "len": 4,
    "mode": "incremental",
    "low": -1,
    "high": 1,
    "fields": [
      { "name": "elevator", "description": "yoke speed: 1 moves it back at 1 per second" },
      ...
    ]
  },
  "task": {
    "name": "landing",
    "time_limit": 240,
    "max_steps": 2400,
    "params": [
      { "name": "config.time_limit", "value": 240, "description": "seconds before the episode is truncated" },
      { "name": "rewards.crash", "value": 30, "description": "taken off once on a crash" },
      { "name": "air.wind_max", "value": 0, "description": "highest wind speed 10 m above the ground, m/s" },
      ...
    ]
  },
  "tasks": ["hold", "takeoff", "landing", "circuit"],
  "reasons": ["running", "success", "crashed", "out_of_envelope", "failed", "time_limit"],
  "stagger": true
}
```

- Observation entry `i` is `raw value / fields[i].scale`, clipped to ±`clip`. The order of
  `fields` is the order in the vector. Save the names with a model: they are its compatibility
  check (see "Model file").
- `observation.image` is `null`. When camera images exist they will be described here and travel
  as a separate array; the vector above will not change because of them.
- Actions are in [-1, 1]; values outside are clamped, NaN counts as 0.
- `reasons` is indexed by the `reason` number of a finished episode.
- New keys may be added to `describe` within version 1: ignore the ones you do not know.

## reset (3) → observations (4)

```json
{ "seed": 1000 }
```
gives environment `i` the seed `1000 + i`. Or one seed each:
```json
{ "seeds": [5, 6, 7] }
```
(exactly `N` of them). Seeds are whole numbers below 2^53.

Optionally, with either form, `"stagger": true` ends the first episodes at spread times:
environment `i` has its first episode truncated (`time_limit`, flag 2) after
`floor(max_steps · (i + 1) / N)` decisions (at least one); every later episode is whole. Without it
all the environments start together and, when most episodes last until the time limit, stay in step
for the whole run, so that each batch a trainer collects comes from the same moment of the episode.
A server that has this says `"stagger": true` in `describe`; one that does not ignores the key.

An episode is a pure function of the session settings, its seed and the actions. When an episode
ends the environment starts the next one by itself with the seed `splitmix64(previous seed)`, so a
whole run is reproducible from the seeds of one `reset`.

`observations` payload, `8 + 4·N·O` bytes:

| offset | type | meaning |
|---|---|---|
| 0 | `u32` | `N` |
| 4 | `u32` | `O` |
| 8 | `f32[N·O]` | first observation of each environment, environment by environment |

## step (5) → step_result (6)

`step` payload, `4·N·A` bytes: `f32[N·A]`, the action of each environment, environment by
environment, in the order of `action.fields`.

Every environment advances `decision_dt` of simulated time holding its action.

`step_result` payload:

| offset | type | meaning |
|---|---|---|
| 0 | `u32` | `N` |
| 4 | `u32` | `O` |
| 8 | `u32` | `F`: episodes that ended in this step |
| 12 | `f32[N·O]` | observations. For an environment whose episode ended: the **first observation of its next episode** |
| 12 + 4·N·O | `f32[N]` | rewards |
| 12 + 4·N·O + 4·N | `u8[N]` | flags: 0 running, 1 terminated, 2 truncated (time limit) |
| ... | `u8[P]` | zeros, `P = (4 - N mod 4) mod 4`, so that what follows starts on a multiple of 4 |
| ... | `F` records | one per ended episode, in environment order |

A record is `16 + 4·O` bytes:

| offset | type | meaning |
|---|---|---|
| 0 | `u32` | index of the environment |
| 4 | `u32` | reason: index into `reasons` |
| 8 | `f32` | return of the episode (sum of its rewards) |
| 12 | `u32` | decisions taken in the episode |
| 16 | `f32[O]` | the **last observation of the ended episode** (to bootstrap the value when it was truncated) |

The reward at a flagged position belongs to the episode that ended. `terminated` covers success,
crash, failure and leaving the envelope; `truncated` is only the time limit.

Total size: `12 + 4·N·O + 4·N + N + P + F·(16 + 4·O)`.

## close (7) → bye (8)

Empty payloads. Closing the socket without `close` is fine too.

## error (9)

```json
{ "error": "step carries 12 bytes; 2 environments of 4 actions need 32", "fatal": false }
```

Sent instead of the answer. With `"fatal": false` nothing changed and the session goes on: an
unknown task or parameter, bad JSON, `step` before `hello` or before `reset`, a wrong number of
seeds or of action bytes, an unknown message type. With `"fatal": true` the server closes the
connection after it: a `hello` whose `protocol` is not `1`, or a frame length of 0 or above the
limit (the framing is lost).

## An annotated session

Two environments of `hold`, so `N = 2`, `O = 34`, `A = 4`.

```
client → 2a 00 00 00                      length 42
         01                               hello
         7b 22 70 72 6f 74 6f 63 6f 6c    {"protocol":1,"task":"hold","num_envs":2}
         22 3a 31 2c 22 74 61 73 6b 22
         3a 22 68 6f 6c 64 22 2c 22 6e
         75 6d 5f 65 6e 76 73 22 3a 32
         7d
server → LL LL 00 00  02  7b ... 7d       describe (JSON)

client → 0b 00 00 00                      length 11
         03                               reset
         7b 22 73 65 65 64 22 3a 37 7d    {"seed":7}
server → 19 01 00 00                      length 281 = 1 + 8 + 4·2·34
         04                               observations
         02 00 00 00                      N = 2
         22 00 00 00                      O = 34
         <272 bytes>                      68 f32

client → 21 00 00 00                      length 33 = 1 + 4·2·4
         05                               step
         00 00 00 3f                      env 0 elevator  0.5
         00 00 80 bf                      env 0 aileron  -1.0
         00 00 00 00                      env 0 rudder    0.0
         00 00 80 3f                      env 0 throttle  1.0
         00 00 00 00 00 00 00 00          env 1: four zeros
         00 00 00 00 00 00 00 00
server → 29 01 00 00                      length 297 = 1 + 12 + 272 + 8 + 2 + 2
         06                               step_result
         02 00 00 00                      N = 2
         22 00 00 00                      O = 34
         00 00 00 00                      F = 0, no episode ended
         <272 bytes>                      observations
         <8 bytes>                        2 rewards
         00 00                            flags: both running
         00 00                            padding

client → 01 00 00 00  07                  close
server → 01 00 00 00  08                  bye
```

Had environment 1 reached its time limit in that step: `F = 1`, flags `00 02`, and after the
padding one record of 152 bytes: `01 00 00 00` (environment 1), `05 00 00 00` (`time_limit`),
the return, the number of steps, and 34 `f32` of last observation; observations 34..67 of the
frame would then be the first of environment 1's next episode.

## Model file

What a trainer writes so that the simulator can fly the policy (`ModelPilot`). A feed-forward
network; the action is its output, with no sampling.

```
offset 0    8 bytes    "FLTMODEL" (ASCII)
offset 8    u32        H: length of the header in bytes
offset 12   H bytes    header: JSON, UTF-8
offset 12+H f32[...]   the numbers below, little-endian, nothing between them
```

Header:

```json
{
  "format": 1,
  "task": "landing",
  "observation": ["airspeed", "ground_speed", "..."],
  "actions": ["elevator", "aileron", "rudder", "throttle"],
  "action_mode": "incremental",
  "decision_dt": 0.1,
  "sizes": [34, 64, 64, 4],
  "activation": "tanh",
  "output": "tanh",
  "normalize": true
}
```

| key | meaning |
|---|---|
| `format` | `1` |
| `task` | the task it was trained on; the pilot derives its targets from it |
| `observation` | the `name` of every observation field, in order, as `describe` gave them. A model whose names differ from the simulator's is refused |
| `actions` | the `name` of every action, in order: the four basic ones, then `flaps` and/or `trim` if trained with them; a multirotor's task (`hover`, `gates`): its four sticks, `throttle`, `roll`, `pitch`, `yaw` |
| `action_mode` | `incremental` or `direct`, as trained |
| `decision_dt` | seconds between decisions, as `describe` gave it |
| `sizes` | width of the input, of each hidden layer, and of the output. `sizes[0]` = number of observation names, the last = number of actions. `[34, 4]` is a single linear layer |
| `activation` | of the hidden layers: `tanh` or `relu` |
| `output` | how the last layer becomes an action in [-1, 1]: `tanh`, or `clip` (clamp) |
| `normalize` | whether a mean and a standard deviation of the observation come first |

Numbers, in this order:

1. if `normalize`: `mean`, `f32[sizes[0]]`, then `std`, `f32[sizes[0]]`
2. for each layer `k = 0 ..`, with `in = sizes[k]`, `out = sizes[k+1]`:
   the weights `W`, `f32[out·in]`, row by row (`W[o][i]` at index `o·in + i`), then the bias
   `f32[out]`

The file ends there: its size is exactly `12 + H + 4·(count of numbers)`. All numbers finite.

What the pilot computes, every `decision_dt`:

```
x = observation                                  (as the environment builds it)
if normalize: x[i] = (x[i] - mean[i]) / max(std[i], 1e-6)
for each layer: y[o] = bias[o] + Σ_i W[o][i] · x[i]
                x = activation(y) for hidden layers, output(y) for the last
action = x
```

For a Gaussian policy save the network that gives the mean; if the trainer squashes its samples
with tanh, say `"output": "tanh"`, if it clips them, `"clip"`.
