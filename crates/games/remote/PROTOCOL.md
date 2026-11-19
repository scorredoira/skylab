# Remote protocol, version 1

How another program flies a running game (`skylab --remote`): it reads the aircraft's state and
sets its controls, while the game runs in its window as ever. Everything needed to write a
client is here. `src/client.rs` is one, and `skylab-ctl` uses it from the command line.

It is `sim-env`'s protocol (`crates/sim/sim-env/PROTOCOL.md`) with other messages: the same
socket, the same frames, the same `hello`, `close` and `error`. A training environment serves
many environments at full speed to a trainer; a game serves the one flight on its screen, in
real time, to anything that wants to fly it.

## Transport

TCP on `127.0.0.1`, port 7448 unless `--remote <port>` says otherwise (`flight-env` has 7447).
Set `TCP_NODELAY` on the client. Local only: the game does not listen on the network.

Any number of clients at once. The client speaks first and the game answers every request with
exactly one frame, in order. The game answers once per frame it draws, so an answer takes up to
a frame (some 16 ms); a game that draws nothing (its window hidden) answers with an `error`
after 5 seconds.

A connection means nothing by itself: connecting, asking and leaving, once per request, is fine
(`skylab-ctl` does that). What a client set stays set after it leaves.

## Framing

Every message, in both directions:

| bytes | type | meaning |
|---|---|---|
| 4 | `u32` little-endian | `length`: number of bytes that follow (type + payload) |
| 1 | `u8` | message type |
| `length - 1` | | payload: JSON in UTF-8, or nothing |

| type | name | direction | payload |
|---|---|---|---|
| 1 | `hello` | client → game | JSON |
| 2 | `describe` | game → client | JSON, the answer to `hello` |
| 7 | `close` | client → game | empty |
| 8 | `bye` | game → client | empty, the answer to `close`; the game then closes the connection |
| 9 | `error` | game → client | JSON, instead of the answer |
| 12 | `state` | client → game | empty |
| 13 | `state_result` | game → client | JSON, the answer to `state` and to `controls` |
| 14 | `command` | client → game | JSON |
| 15 | `done` | game → client | JSON, the answer to `command` |
| 17 | `controls` | client → game | JSON |

Types 3 to 6 are `sim-env`'s `reset` and `step`, which a game does not answer yet. Types 10
(`frame`, the window's picture, answered by 11 `image`) and 16 (`advance`, for a clock the client
steps) are kept for what is not built: the game answers them with an `error`.

## hello (1) → describe (2)

`hello` must come first, and `protocol` must be `1`:

```json
{ "protocol": 1, "client": "my-program" }
```

| key | meaning |
|---|---|
| `protocol` | `1` |
| `client` | free text, ignored |
| `clock` | `"real"` (the default, and the only one): the game runs by itself |

`describe` says what there is to fly and with what:

```json
{
  "protocol": 1,
  "server": "skylab 0.1.0",
  "clock": "real",
  "vehicle": "c172",
  "speeds": { "stall_clean_kt": 53, "stall_full_kt": 48, "rotate_kt": 55, "climb_kt": 74, "pattern_kt": 85,
              "base_kt": 71, "final_kt": 66, "flaps_max_kt": 85 },
  "controls": [
    { "name": "pitch_deg", "low": -15, "high": 20, "description": "assist: the nose attitude to hold, degrees up. ..." },
    ...
  ],
  "commands": [
    { "do": "restart", "description": "start again; where: runway (default), final or air" },
    ...
  ]
}
```

- `server` is the game and its version.
- `vehicle` is the key of what the player flies (`c172`, `extra`, `fpv`...).
- `speeds` are the type's own, in knots (`null` for what is not an aircraft): where it stalls
  with the flaps up (`stall_clean_kt`) and fully down (`stall_full_kt`), where the nose is lifted
  on the takeoff run (`rotate_kt`), what the climb (`climb_kt`), the circuit (`pattern_kt`), the
  base leg (`base_kt`) and the final approach (`final_kt`) are flown at, and the most the flaps
  are lowered at (`flaps_max_kt`).
- `controls` are the names `controls` takes, each with its range (`low`, `high`) and its `description`.
- `commands` are the orders `command` takes.
- New keys may be added within version 1: ignore the ones you do not know.

## state (12) → state_result (13)

`state` has no payload. `state_result` is the flight as a pilot reads it, in a cockpit's units:

```json
{
  "brief": "77 kt (+0.2/s, mean 68) · 181 ft up, +612 fpm (mean +341) · nose +8° · wings level · heading 153° · runway 16: 1285 m past the threshold, 80 m LEFT of the centreline, GOING AWAY 6 m/s, nose 8° left of it · throttle 1.00, flaps 0°, assist holding nose +8° bank +0°",
  "time_s": 45.5, "paused": false, "time_rate": 1, "time_rate_achieved": 1, "flown_by": "remote", "vehicle": "c172", "message": "",
  "on_ground": false, "crashed": null,
  "ground_speed_kt": 78.4, "altitude_ft": 1030, "heading_deg": 153.2, "track_deg": 153.7,
  "pitch_deg": 8.1, "roll_deg": -0.6, "roll_rate_deg_s": 0, "pitch_rate_deg_s": 0,
  "wind": { "from_deg": 250, "speed_kt": 0 },
  "stall_warning": false, "airspeed_kt": 77.2, "airspeed_avg_kt": 68, "airspeed_trend_kt_s": 0.2,
  "height_ft": 181, "vertical_speed_fpm": 612, "vertical_speed_avg_fpm": 341,
  "turn_rate_deg_s": -0.2, "turn_radius_m": null, "alpha_deg": 3.6, "slip_deg": -0.3, "rpm": 2429, "flaps_deg": 0,
  "assist": true,
  "controls": { "pitch_deg": 8, "bank_deg": 0, "elevator": 0.022, "aileron": 0.023, "rudder": 0.034,
                "throttle": 1, "flaps": 0, "trim": 0, "brakes": 0 },
  "pulse_s": 0, "autopilot": "off", "touchdown_fpm": null,
  "runway": { "number": 16, "heading_deg": 161.7, "elevation_ft": 843, "length_m": 1000, "width_m": 24,
              "along_m": 1285, "across_m": -80.4, "across_rate_m_s": -5.6, "height_ft": 183,
              "path_error_ft": 183, "needed_fpm": null, "heading_error_deg": 8.5, "nose_right_deg": -8.5,
              "over": false },
  "landing": null
}
```

| field | meaning |
|---|---|
| `brief` | the whole of it in one line of words, for a reader in a hurry: the speed with its trend and mean, the height and the climb, the attitude, the turn, where the runway is (how far before or past the threshold, how far LEFT or RIGHT of the centreline and whether closing or GOING AWAY, where the nose points from it, above or below the path), the power, the flaps and what the assist holds. Sides are said in words, never by a sign |
| `time_s` | the world's time, seconds; it stands still while the game is paused |
| `paused` | the game waits (a menu, Esc, the `pause` order) |
| `time_rate` | how many times faster than real time the world is to go: 1, 2, 4, 8 or 16 (keys F9 F10, the `time` order). The steps are as long as ever: only more of them each frame. It goes back to 1 on a crash, a touchdown, a pause or a replay |
| `time_rate_achieved` | how many times faster than real time it has gone over the last second or so: below `time_rate` when the computer cannot keep up. The answers come once a frame, so at 16 each is 16 times further apart in the world's time |
| `flown_by` | who has the aircraft: `remote` (a client took it with `controls`), `player` (the keyboard) or `autopilot` (the game's own, key L) |
| `vehicle` | as in `describe` |
| `message` | what the game says in the middle of the screen, or `""`: an overload, an overspeed, what broke, the touchdown |
| `on_ground` | a wheel is on the ground |
| `crashed` | `null`, or why the aircraft is a wreck (then only `restart` flies again) |
| `ground_speed_kt` | speed over the ground |
| `altitude_ft` | above sea level, the true one |
| `heading_deg`, `track_deg` | where the nose points and where the aircraft goes over the ground, degrees true, 0 to 360 |
| `pitch_deg`, `roll_deg` | nose up positive; right wing down positive |
| `roll_rate_deg_s`, `pitch_rate_deg_s` | how fast the bank and the nose are moving now (about the aircraft's own axes): positive, rolling right and pitching up |
| `wind` | the surface wind: where it blows from (`from_deg`, true) and its speed (`speed_kt`) |

An aircraft has these besides (a multirotor has only `height_ft` and `vertical_speed_fpm` of them):

| field | meaning |
|---|---|
| `stall_warning` | the stall horn sounds |
| `airspeed_kt` | indicated airspeed |
| `airspeed_avg_kt`, `vertical_speed_avg_fpm` | their means over the last 30 seconds (since the last restart, if less): where they are settling while the nose swings |
| `airspeed_trend_kt_s` | how fast the airspeed changes now, knots per second: negative, slowing down |
| `height_ft` | of the wheels above the ground below |
| `vertical_speed_fpm` | feet per minute, climbing positive |
| `turn_rate_deg_s` | right positive; 3 is a standard turn |
| `turn_radius_m` | the radius of the circle it flies at this bank and speed; `null` with the wings within 3° of level. A turn of 90° ends that far to the side, and as far ahead, of where it began |
| `alpha_deg` | angle of attack |
| `slip_deg` | the slip ball: positive, it is to the right (right pedal brings it back) |
| `rpm` | engine speed |
| `flaps_deg` | where the flaps are now (they take seconds to follow the lever) |
| `assist` | the assist has the yoke and the pedals, holding the attitude asked for (`controls`, below) |
| `controls` | where every control is now, by the names of `describe`: with the assist, `pitch_deg` and `bank_deg` are what it was asked to hold and `elevator`, `aileron` and `rudder` where it has them; without it, they are where the client put them, and `pitch_deg` and `bank_deg` the attitude there is. With the autopilot engaged, where its servos hold the ailerons, the elevator and the trim |
| `pulse_s` | seconds left of the controls set with `for`, rounded up; 0 when none are |
| `autopilot` | `off`, or the modes the GFC 700 flies in (`HDG ALT`): engaged, its servos have the ailerons, the elevator and the trim whatever `controls` says |
| `touchdown_fpm` | the sink rate at the last touchdown, or `null` before one |
| `landing` | `null` in flight; from the first touch after a flight, the landing (below) |
| `runway` | the runway in use and where the aircraft is from it (below) |

`landing` is the remote's own word on how the aircraft came down. It is forgotten when the
aircraft climbs 30 m above the runway again (a go-around) and at a restart.

| field of `landing` | meaning |
|---|---|
| `verdict` | `good` (the hardest touch under 300 feet per minute, on the runway), `good, but far from the centreline` (further from it than a quarter of the runway's width), `firm` (300 to 500), `hard` (over 500), `off the runway` (the first touch was not on its surface), `ran off the runway` (it touched on it and is on the ground off it), `crashed` |
| `sink_fpm` | the hardest touch of the landing, a bounce included |
| `along_m`, `across_m` | where the first touch was: metres past the threshold, metres right of the centreline |
| `on_runway` | the first touch was on the runway's surface |

The grades are for telling one landing from another, not the airframe's limits.

The runway in use is the nearest aerodrome's, in the direction the aircraft faces while it
stands on the ground. Once the aircraft moves it stays the same, whichever way the aircraft
turns, so a whole circuit is told against the runway it took off from. The `runway` order picks
it again in flight.

| field of `runway` | meaning |
|---|---|
| `number` | as painted: 25 for a heading of 250° |
| `heading_deg` | the direction of takeoff and landing, degrees true |
| `elevation_ft` | above sea level |
| `length_m`, `width_m` | of the runway |
| `along_m` | metres past the threshold, along the runway: negative on the approach, 0 to `length_m` over it |
| `across_m` | metres to the right of the centreline (negative: to its left) |
| `across_rate_m_s` | how fast `across_m` changes, metres per second: with the sign of `across_m`, going away from the centreline; against it, closing |
| `height_ft` | of the wheels above the runway |
| `path_error_ft` | height above a 3° approach path to the aiming point, 150 m past the threshold: positive, too high |
| `needed_fpm` | the vertical speed that takes the aircraft from here straight to the aiming point at its speed over the ground; `null` on the ground and past that point. Fly it and you arrive there, whatever the path error is now |
| `nose_right_deg` | where the nose points from the runway's heading, −180 to 180: positive, to its right; 0 lined up; ±180 flying the other way. The same as the `brief`'s "nose 4° right of it" |
| `heading_error_deg` | `nose_right_deg` with the other sign, as `sim-env`'s observation has it: the runway's heading less the aircraft's |
| `over` | the aircraft is over the runway's surface |

## controls (17) → state_result (13)

Sets controls by name. Only the ones named change:

```json
{ "throttle": 1 }
{ "elevator": 0.2, "aileron": -0.1 }
```

With `for`, the ones named take their values for that many seconds of the world's time and
then go back to what they were held at: a pulse.

```json
{ "aileron": 0.3, "for": 0.5 }
```

- The names and ranges are `describe`'s. For an aircraft: `pitch_deg` and `bank_deg` (the
  assist, below), `elevator`, `aileron`, `rudder` (−1 to 1), `throttle`, `brakes` (0 to 1), `flaps`
  (the notch, 0 to 3), `trim` (−1 to 1). A value beyond its range is brought into it.
- A control stays where it was last set until it is set again. Nothing centres by itself: after
  `{"elevator": 0.2}` the yoke is held back until `{"elevator": 0}`.
- A new pulse replaces the one running. Controls set without `for` during a pulse change what the
  pulse will go back to.
- The first `controls` takes the aircraft: from then on the keyboard and the game's own
  autopilot fly nothing, and `flown_by` says `remote`, until the `release` order. A restart leaves
  the remote flying, holding the controls of the new start (throttle closed, yoke centred).
- The answer is the state at that moment: its `controls` are the new ones, the rest is the
  flight before they have acted.
- Refused (an `error`, nothing changed): a name that is no control, a value that is not a
  number, no control named, a vehicle that is not an aircraft.

### The assist: an attitude instead of a place for the yoke

`pitch_deg` (−15 to 20, nose up) and `bank_deg` (−45 to 45, right wing down) ask for an
attitude, and the assist moves the yoke and the pedals to reach it and hold it. It is the
assist a player has at the keyboard, where the arrows move that same attitude
(`sim_core::KeyboardPilot`); it is not the autopilot, and holds no heading, height or speed.

```json
{ "pitch_deg": 8, "bank_deg": 0, "throttle": 1 }
{ "bank_deg": -20 }
{ "bank_deg": 20, "for": 12 }
```

- Setting either gives the yoke and the pedals to the assist (`assist: true`); the one not
  named starts as the aircraft is (the nose where it is; the wings as they are, or level if
  they nearly are). Setting `elevator` or `aileron` takes the yoke back (`assist: false`).
- In the air it holds the nose and the bank, and its feet keep the slip ball centred.
- On the ground its feet keep the aircraft on the line it is rolling along, and the yoke stays
  centred until the type's rotation speed (`speeds.rotate_kt`); from there a `pitch_deg` above
  the nose lifts it. One message is a takeoff: `{"pitch_deg": 8, "bank_deg": 0, "throttle": 1}`.
- `rudder` with the assist is a push on the pedals over what its feet do: leave it at 0 (the
  first message that turns the assist on lets the pedals go unless it names them).
- It does not keep the aircraft from stalling: a nose held higher than the power can carry
  slows it down until it does. `throttle`, `flaps`, `trim` and `brakes` are the client's as ever.

## command (14) → done (15)

An order to the game, named by `do`:

```json
{ "do": "restart", "where": "final" }
```

| `do` | what it does |
|---|---|
| `restart` | starts again at the nearest aerodrome, into the wind: `"where": "runway"` (stopped on the threshold; the default), `"final"` (in flight on final approach) or `"air"` (in flight over where it is) |
| `pause` | the game waits |
| `resume` | the game goes on, and any menu over it closes |
| `release` | the aircraft goes back to the keyboard |
| `runway` | the state's runway is picked again: the nearest one, in the direction nearest the heading now |
| `time` | how many times faster than real time the world goes: `"times"` 1, 2, 4, 8 or 16 |
| `view` | what the window shows: `"which": "cockpit"` or `"outside"` |
| `autopilot` | works the C172's GFC 700: `"keys"` are its keys and settings in order, with commas: `ap`, `fd`, `hdg`, `nav`, `apr`, `alt`, `vs`, `flc`, `up`, `dn`, `dto`, `sync`, `disc`, and `bug=<degrees>`, `sel=<feet>` (with a sign: from the heading or altitude of now). It does not engage on the ground |
| `wind` | the surface wind, for this run only (the settings keep theirs): `"from_deg"` (true) and `"speed_kt"`, either or both |

`done` names the order and the world's time after it:

```json
{ "done": "restart", "time_s": 201.35 }
```

## close (7) → bye (8)

Empty payloads. Closing the socket without `close` is fine too.

## error (9)

```json
{ "error": "no control wheel: elevator, aileron, rudder, throttle, flaps, trim, brakes, and for", "fatal": false }
```

Sent instead of the answer. With `"fatal": false` nothing changed and the session goes on: an
unknown control or order, a payload that is not JSON, a request before `hello`, a message that
is not built yet. With `"fatal": true` the game closes the connection after it: a `hello` whose
`protocol` is not `1`, or a frame whose length is 0 or beyond the limit (the framing is lost).

## An annotated session

```
client → 0f 00 00 00                      length 15
         01                               hello
         7b 22 70 72 6f 74 6f 63 6f 6c    {"protocol":1}
         22 3a 31 7d
game   → LL LL 00 00  02  7b ... 7d       describe (JSON)

client → 01 00 00 00                      length 1
         0c                               state, no payload
game   → LL LL 00 00  0d  7b ... 7d       state_result (JSON)

client → 0f 00 00 00                      length 15
         11                               controls
         7b 22 74 68 72 6f 74 74 6c 65    {"throttle":1}
         22 3a 31 7d
game   → LL LL 00 00  0d  7b ... 7d       state_result (JSON)

client → 01 00 00 00  07                  close
game   → 01 00 00 00  08                  bye
```
