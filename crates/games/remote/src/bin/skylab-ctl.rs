//! `skylab-ctl`: one request to a running game (`skylab --remote`) per call, from the command
//! line. It knows no vehicle: the controls and the commands are whatever the game says it has
//! (`skylab-ctl describe`), passed on as `name=value`.

use std::time::{Duration, Instant};

use remote::{Client, Json, DEFAULT_PORT};

const USAGE: &str = "\
skylab-ctl [--port <n>] <request>

  describe                       what the game is: its vehicle, its controls, its commands
  state [<field>...]             where the vehicle is and how it flies; with fields, only those
                                 (runway.along_m: a field inside another)
  controls <name>=<value>... [for=<seconds>] [--wait] [<field>...]
                                 set controls: for good, or for some seconds of the game's time,
                                 after which they go back to what they were. --wait: come back
                                 when those seconds have passed. Prints the state then
  command <do> [<name>=<value>...]
                                 an order to the game: restart where=runway, pause, resume...

  --when <field><op><value>
               with state, controls or command: first wait until that number of the state is
               there, then do it at once. The ops: > and < (airspeed_kt>55, height_ft<50), and
               ~ for near, within a margin given after a colon (heading_deg~270:5; angles, the
               fields that end in _deg, the short way round). Several --when: all of them
               While it waits it says what it is watching, every 5 seconds, on the error
               output. Stopped while it waits, nothing was sent
  --timeout <s> how long --when waits, in real seconds (default 300). If the time runs out
               nothing is sent: the state is printed and the exit code is 2
  --after <s>  with controls or command: print the state as it is that many seconds of the
               game's time later, once what was sent has acted (the answer by itself is the
               state at the moment of sending)
  --port <n>   the game's port (default 7448, or $SKYLAB_PORT)

Examples:
  skylab-ctl state brief
  skylab-ctl controls pitch_deg=8 bank_deg=0 throttle=1 brief
  skylab-ctl controls bank_deg=-20 --after 4 brief
  skylab-ctl controls bank_deg=0 --when 'runway.heading_error_deg~90:8' brief
  skylab-ctl controls aileron=0.3 for=0.5 --wait roll_deg heading_deg
  skylab-ctl command restart where=runway";

/// After a pulse's seconds have passed in real time, how much longer `--wait` waits for the game
/// to have flown them (a slow frame rate, a pause) before giving up.
const WAIT_MARGIN: f64 = 10.0;
const WAIT_POLL: Duration = Duration::from_millis(20);
/// How often `--when` looks at the state, and how long it waits unless `--timeout` says.
const WHEN_POLL: Duration = Duration::from_millis(30);
const WHEN_TIMEOUT: f64 = 300.0;
/// How often `--when` says what it is waiting for.
const WHEN_SAYS: f64 = 5.0;

/// A number of the state to wait for (`--when`).
struct When {
    field: String,
    test: Test,
}

enum Test {
    Above(f64),
    Below(f64),
    /// Within the margin of the value.
    Near(f64, f64),
}

impl When {
    /// `airspeed_kt>55`, `height_ft<50`, `heading_deg~270:5`.
    fn parse(text: &str) -> Option<When> {
        let at = text.find(['>', '<', '~'])?;
        let (field, rest) = (text[..at].trim().to_string(), &text[at + 1..]);
        let test = match &text[at..at + 1] {
            ">" => Test::Above(rest.trim().parse().ok()?),
            "<" => Test::Below(rest.trim().parse().ok()?),
            _ => {
                let (value, margin) = rest.split_once(':').unwrap_or((rest, "1"));
                Test::Near(value.trim().parse().ok()?, margin.trim().parse().ok()?)
            }
        };
        (!field.is_empty()).then_some(When { field, test })
    }

    /// The field as the state has it now, if it is a number.
    fn now(&self, state: &Json) -> Option<f64> {
        self.field.split('.').try_fold(state, |inside, name| inside.get(name)).and_then(Json::as_f64)
    }

    /// What it waits for and where the number is now: `runway.along_m is -2100, waiting for < -3000`.
    fn says(&self, state: &Json) -> String {
        let wanted = match self.test {
            Test::Above(value) => format!("> {value}"),
            Test::Below(value) => format!("< {value}"),
            Test::Near(value, margin) => format!("within {margin} of {value}"),
        };
        match self.now(state) {
            Some(now) => format!("{} is {now}, waiting for {wanted}", self.field),
            None => format!("{} is not a number of the state (see `skylab-ctl state`)", self.field),
        }
    }

    /// Whether the state is there. A field that is missing or not a number never is.
    fn met(&self, state: &Json) -> bool {
        let Some(now) = self.now(state) else { return false };
        match self.test {
            Test::Above(value) => now > value,
            Test::Below(value) => now < value,
            // An angle is near another the short way round: 359 is 2 from 1.
            Test::Near(value, margin) if self.field.ends_with("_deg") => ((now - value + 180.0).rem_euclid(360.0) - 180.0).abs() <= margin,
            Test::Near(value, margin) => (now - value).abs() <= margin,
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut port = std::env::var("SKYLAB_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    if let Some(i) = args.iter().position(|a| a == "--port") {
        match args.get(i + 1).and_then(|p| p.parse().ok()) {
            Some(p) => port = p,
            None => fail("--port needs a number"),
        }
        args.drain(i..i + 2);
    }
    let wait = args.iter().position(|a| a == "--wait").map(|i| args.remove(i)).is_some();
    let mut after = None;
    if let Some(i) = args.iter().position(|a| a == "--after") {
        match args.get(i + 1).and_then(|t| t.parse::<f64>().ok()) {
            Some(t) => after = Some(t),
            None => fail("--after needs seconds"),
        }
        args.drain(i..i + 2);
    }
    let mut timeout = WHEN_TIMEOUT;
    if let Some(i) = args.iter().position(|a| a == "--timeout") {
        match args.get(i + 1).and_then(|t| t.parse().ok()) {
            Some(t) => timeout = t,
            None => fail("--timeout needs seconds"),
        }
        args.drain(i..i + 2);
    }
    let mut whens = Vec::new();
    while let Some(i) = args.iter().position(|a| a == "--when") {
        match args.get(i + 1).map(|text| When::parse(text)) {
            Some(Some(when)) => whens.push(when),
            _ => fail("--when needs <field><op><value>: airspeed_kt>55, height_ft<50, heading_deg~270:5"),
        }
        args.drain(i..i + 2);
    }
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    let request = args.remove(0);
    // What is left: `name=value` pairs for the game, and bare words (fields to print, or the order).
    let (pairs, words): (Vec<&String>, Vec<&String>) = args.iter().partition(|a| a.contains('='));
    let mut body: Vec<(String, Json)> = pairs.iter().filter_map(|pair| pair.split_once('=')).map(|(name, value)| (name.to_string(), value_of(value))).collect();

    let mut game = Client::connect(port, "skylab-ctl").unwrap_or_else(|e| fail(&e));
    if !whens.is_empty() {
        // Wait for the state to be there; if it never is, nothing is sent.
        let start = Instant::now();
        let mut said = 0.0;
        loop {
            let state = game.state().unwrap_or_else(|e| fail(&e));
            if whens.iter().all(|when| when.met(&state)) {
                break;
            }
            let waited = start.elapsed().as_secs_f64();
            if waited >= said + WHEN_SAYS || waited > timeout {
                said = waited;
                let watching: Vec<String> = whens.iter().filter(|when| !when.met(&state)).map(|when| when.says(&state)).collect();
                eprintln!("skylab-ctl: {:.0} s: {}", waited, watching.join("; "));
            }
            if waited > timeout {
                game.close();
                let fields: Vec<&String> = words.iter().skip((request == "command") as usize).copied().collect();
                print(&state, &fields);
                eprintln!("skylab-ctl: --when was not met in {timeout} s: nothing sent");
                std::process::exit(2);
            }
            std::thread::sleep(WHEN_POLL);
        }
    }
    let (answer, fields) = match request.as_str() {
        "describe" => (game.description.clone(), words),
        "state" => (game.state().unwrap_or_else(|e| fail(&e)), words),
        "controls" => {
            if body.is_empty() {
                fail("controls needs name=value: see `skylab-ctl describe` for the names");
            }
            let pulse = body.iter().find(|(name, _)| name == "for").and_then(|(_, v)| v.as_f64());
            let mut state = game.controls(&Json::Obj(body)).unwrap_or_else(|e| fail(&e));
            if let (true, Some(seconds)) = (wait, pulse) {
                state = after_pulse(&mut game, seconds).unwrap_or_else(|e| fail(&e));
            }
            if let Some(seconds) = after {
                state = later(&mut game, &state, seconds).unwrap_or_else(|e| fail(&e));
            }
            (state, words)
        }
        "command" => {
            let Some((order, fields)) = words.split_first() else { fail("command needs an order: see `skylab-ctl describe` for them") };
            body.insert(0, ("do".to_string(), Json::str(order.as_str())));
            let done = game.command(&Json::Obj(body)).unwrap_or_else(|e| fail(&e));
            match after {
                // The state some seconds after the order, instead of the order's own answer.
                Some(seconds) => (later(&mut game, &done, seconds).unwrap_or_else(|e| fail(&e)), fields.to_vec()),
                None => (done, fields.to_vec()),
            }
        }
        other => fail(&format!("unknown request {other} (see --help)")),
    };
    game.close();
    print(&answer, &fields);
}

/// A value written at the command line: a number, `true`, `false`, or else a text.
fn value_of(text: &str) -> Json {
    match text {
        "true" => Json::Bool(true),
        "false" => Json::Bool(false),
        _ => text.parse::<f64>().map_or_else(|_| Json::str(text), Json::Num),
    }
}

/// The state once the game has flown a pulse to its end: the game says how much of it is left
/// (`pulse_s`), in its own time.
fn after_pulse(game: &mut Client, seconds: f64) -> Result<Json, String> {
    let start = Instant::now();
    loop {
        let state = game.state()?;
        let left = state.get("pulse_s").and_then(Json::as_f64).unwrap_or(0.0);
        if left <= 0.0 || start.elapsed().as_secs_f64() > seconds.max(0.0) + WAIT_MARGIN {
            return Ok(state);
        }
        std::thread::sleep(WAIT_POLL.max(Duration::from_secs_f64((left * 0.5).min(0.2))));
    }
}

/// The state `seconds` of the game's time after the answer `from` (which says its `time_s`).
fn later(game: &mut Client, from: &Json, seconds: f64) -> Result<Json, String> {
    let start = Instant::now();
    let then = from.get("time_s").and_then(Json::as_f64);
    loop {
        let state = game.state()?;
        let now = state.get("time_s").and_then(Json::as_f64);
        let passed = match (then, now) {
            (Some(then), Some(now)) => now - then,
            // A game that tells no time: the caller's clock.
            _ => start.elapsed().as_secs_f64(),
        };
        if passed >= seconds || start.elapsed().as_secs_f64() > seconds.max(0.0) + WAIT_MARGIN {
            return Ok(state);
        }
        std::thread::sleep(WAIT_POLL.max(Duration::from_secs_f64(((seconds - passed) * 0.5).clamp(0.0, 0.2))));
    }
}

/// Prints an answer: a line per field, `name: value`, the whole of it or only the fields asked for.
fn print(answer: &Json, fields: &[&String]) {
    if fields.is_empty() {
        match answer.as_object() {
            Some(pairs) => pairs.iter().for_each(|(name, value)| println!("{name}: {value}")),
            None => println!("{answer}"),
        }
        return;
    }
    for field in fields {
        match field.split('.').try_fold(answer, |inside, name| inside.get(name)) {
            Some(value) => println!("{field}: {value}"),
            None => println!("{field}: (no such field)"),
        }
    }
}

fn fail(why: &str) -> ! {
    eprintln!("skylab-ctl: {why}");
    std::process::exit(1);
}
