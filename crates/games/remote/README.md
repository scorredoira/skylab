# remote

A running game flown by another program: the socket, its messages, a client, and `skylab-ctl`,
the client at the command line. It knows no game and no vehicle; a game answers the requests
with what it has. Depends on `sim-env` only, for its frames and its JSON; sockets, threads and
channels are the standard library's. Native only (no sockets on the web).

| To read | For |
|---|---|
| [PROTOCOL.md](PROTOCOL.md) | every message and every field: all a client in any language needs |
| [docs/AGENT-PILOT.md](../../../docs/AGENT-PILOT.md) | how an agent flies the Cessna 172 with `skylab-ctl`: its numbers and a circuit, step by step |
| `src/wire.rs` | the message types and how one is written and read |
| `src/server.rs` | the game's end: `Server`, `Request` |
| `src/client.rs` | a program's end: `Client` |
| `src/bin/skylab-ctl.rs` | the command line |
| `crates/games/flight/src/remote.rs` | what the requests mean in the flight game: the controls, the state, the orders |

```text
your program ──► Client ──┐                                  ┌──► the game's loop, once a frame:
skylab-ctl ─────► Client ──┼─ TCP 127.0.0.1:7448 ─► Server ───┤     while let Some(request) = server.next()
anything that writes       │   (a thread per client)          │         request.answer(kind, json)
the frames of PROTOCOL.md ─┘                                  └──► nothing of the game is shared with a thread
```

## Trying it

```sh
cargo build --release -p flight -p remote
target/release/skylab --remote                 # the game, listening on 127.0.0.1:7448
target/release/skylab-ctl state                # in another terminal
```

```
brief: "ON THE GROUND · 0 kt (+0.0/s, mean 0) · 0 ft up, +0 fpm (mean +0) · nose -0° · wings level · heading 189° · runway 19: 40 m past the threshold, on the centreline, nose along it · throttle 0.00, flaps 0°, no assist"
time_s: 12.4
paused: false
flown_by: "player"
...
airspeed_kt: 0
runway: {"number":19,"heading_deg":189,"elevation_ft":291,"length_m":2550,...,"along_m":40,"across_m":0,...}
```

```sh
skylab-ctl describe                                   # the controls, the orders and the speeds this game has
skylab-ctl state brief                                # the flight in one line of words
skylab-ctl state airspeed_kt height_ft runway.along_m # only these fields
skylab-ctl controls pitch_deg=8 bank_deg=0 throttle=1 brief   # the assist: an attitude asked for, and held
skylab-ctl controls bank_deg=-20 for=10 --wait brief  # a bank for ten seconds, then the state
skylab-ctl controls bank_deg=0 --when 'runway.heading_error_deg~90:5' brief   # sent when the state is there
skylab-ctl controls throttle=1                        # set, and held until set again
skylab-ctl controls aileron=0.3 for=0.5 --wait roll_deg   # the yoke itself: half a second of aileron
skylab-ctl command restart where=final                # an order
skylab-ctl command wind speed_kt=0                    # still air for this run
skylab-ctl command release                            # the aircraft back to the keyboard
```

`skylab-ctl` connects, asks once and leaves; what it set stays set. It prints a field per line,
`name: value`, and after `controls` or `state` the names of the fields wanted can follow. It
exits with 1 and the reason on the error output when the game refuses, or when there is no
game. `--port <n>` or `$SKYLAB_PORT` for another port. It passes `name=value` on as it is: the
names are the game's (`describe`), so it needs no change when a game gains a control or an order.

`--when <field><op><value>` waits for a number of the state before sending (`>`, `<`, or `~` for
near: `heading_deg~270:5`), polling the game thirty times a second: a command lands when the
aircraft is there, not when the caller gets round to it. While it waits it says every 5 seconds,
on the error output, where the number is and what it waits for. `--timeout <s>` bounds the wait
(300); if it runs out, or the wait is stopped, nothing is sent (exit code 2 for the timeout).

`--after <s>` prints the state that many seconds of the game's time after a `controls` or a
`command`, once it has acted: the answer by itself is the state at the moment of sending.

## From a program

In Rust, `Client` (`src/client.rs`):

```rust
use remote::{Client, Json};
let mut game = Client::connect(remote::DEFAULT_PORT, "my-program")?;
let state = game.state()?;
if state.get("airspeed_kt").and_then(Json::as_f64) > Some(55.0) {
    game.controls(&Json::obj(vec![("elevator", Json::num(0.2))]))?;
}
```

In any other language: a TCP socket and the frames of [PROTOCOL.md](PROTOCOL.md) (four bytes of
length, one of type, JSON). There is no client library to install.

## In a game

A game opens the socket once and answers what came in every frame, in its own loop:

```rust
let server = remote::Server::listen(port)?;          // at start
while let Some(request) = server.next() {            // every frame, before the world steps
    match request.kind {
        remote::message::STATE => request.answer(remote::message::STATE_RESULT, state_as_json()),
        _ => request.refuse("unknown message"),
    }
}
```

`Server` checks what the protocol itself asks (`hello` first and with the right version,
`close`, a payload that is JSON) and hands the rest to the game. The game's side is one file,
`crates/games/flight/src/remote.rs`; its header says where a control, an order, a number of the
state or another kind of vehicle goes.

To open **the driving game** the same way: a `remote.rs` there with the car's controls (steer,
throttle, brake), its state (speed, the circuit's distance and offset, the lap) and its orders,
answered from its loop, and `--remote` on its command line. Nothing here changes: `skylab-ctl`
already passes any name on.

## What is not built

`PROTOCOL.md` keeps their message types, and the game answers them with an `error`:

- **The window's picture** (`frame` → `image`): a PNG of what the game shows, for models that see.
- **A stepped clock** (`advance`, and `hello`'s `"clock": "stepped"`): the game waits for the
  client instead of running by itself, for models that take seconds to decide.
- **`reset` and `step`** as `sim-env` has them, with a task's reward, so that a trainer made for
  `flight-env` can connect to the game.
- **Multirotors and cars**: a multirotor's state is told, its controls are refused.

The plan they belong to is [docs/AI-CONTROL.md](../../../docs/AI-CONTROL.md), step 6.

## Checking a change

```sh
cargo test -p remote            # the frames and the protocol's own rules over a real socket
cargo test -p flight remote     # what the flight game answers: controls, pulses, state, orders
```

The second also checks that `PROTOCOL.md` names every field the game sends and every order it
takes: a field added to the state without its line there fails.
