//! The game flown from another program (`--remote`): what the requests of
//! `crates/games/remote/PROTOCOL.md` mean here.
//!
//! ```text
//! Flight::update ─► serve ─► answer ─┬─ hello    ─► describe      what there is to fly, and with what
//!   (once a frame)                   ├─ state    ─► state         the numbers a pilot flies by
//!                                    ├─ controls ─► set_controls  the yoke, pedals, throttle, flaps, trim, brakes
//!                                    └─ command  ─► command       the orders of `ORDERS`: restart, pause, wind...
//! ```
//!
//! To add:
//! - **a control**: a line in `CONTROLS`. `describe`, `controls` and the state's `controls` come from it.
//!   Two of them, `pitch_deg` and `bank_deg`, are not places for a control but an attitude for the
//!   keyboard's assist to hold (`RemoteHands::fly`): the easy way to fly at a decision every few seconds.
//! - **an order**: a line in `ORDERS` and its function.
//! - **a number of the state**: a line in `state`, and in `PROTOCOL.md` (a test holds them together).
//! - **a kind of vehicle**: its own table of controls beside `CONTROLS` and its hands beside
//!   `RemoteHands`; today a multirotor's state is told but its `controls` are refused.
//!
//! The first `controls` takes the aircraft: from then on `RemoteHands` is its pilot instead of
//! the keyboard's (`Session::pass`), until `release`. The GFC 700 stays behind those hands as it
//! is behind the keyboard's: engaged, its servos have the ailerons, elevator and trim.

use std::collections::VecDeque;

use sim_core::{Aircraft, AircraftControls, Controls, Gfc700, KeyboardPilot, Observation, Pilot, DEG, FEET, GRAVITY, KNOTS};
use sim_env::json::Json;
use sim_env::guidance::AIMING_POINT;
use sim_env::Runway;

use crate::game::Session;

// ---- The controls

/// What a remote pilot holds: every control where the client last put it and, with the assist,
/// the attitude it asked for instead of a place for the yoke.
#[derive(Clone, Copy)]
struct Hold {
    controls: AircraftControls,
    /// The assist flies the yoke and the pedals to `pitch` and `bank` (the keyboard's assist,
    /// `sim_core::KeyboardPilot`); otherwise they are where `controls` says.
    assisted: bool,
    /// The attitude asked of the assist, degrees. Without the assist they follow the aircraft,
    /// so that the assist starts from the attitude there is.
    pitch: f64,
    bank: f64,
}

/// A control a client sets by name.
struct Control {
    name: &'static str,
    low: f64,
    high: f64,
    what: &'static str,
    get: fn(&Hold) -> f64,
    set: fn(&mut Hold, f64),
}

/// An aircraft's controls, as `describe` lists them. `pitch_deg` and `bank_deg` turn the assist
/// on, `elevator` and `aileron` turn it off: whichever was set last has the yoke.
const CONTROLS: [Control; 9] = [
    Control {
        name: "pitch_deg",
        low: -15.0,
        high: 20.0,
        what: "assist: the nose attitude to hold, degrees up. Setting it or bank_deg gives the yoke and the pedals to the assist",
        get: |h| h.pitch,
        set: |h, v| (h.pitch, h.assisted) = (v, true),
    },
    Control { name: "bank_deg", low: -45.0, high: 45.0, what: "assist: the bank to hold, degrees, right wing down positive (0: wings level)", get: |h| h.bank, set: |h, v| (h.bank, h.assisted) = (v, true) },
    Control {
        name: "elevator",
        low: -1.0,
        high: 1.0,
        what: "direct: the yoke back and forward, 1 full back (nose up). Setting it or aileron turns the assist off",
        get: |h| h.controls.elevator,
        set: |h, v| (h.controls.elevator, h.assisted) = (v, false),
    },
    Control { name: "aileron", low: -1.0, high: 1.0, what: "direct: the yoke to the sides, 1 full right (rolls right)", get: |h| h.controls.aileron, set: |h, v| (h.controls.aileron, h.assisted) = (v, false) },
    Control {
        name: "rudder",
        low: -1.0,
        high: 1.0,
        what: "the pedals, 1 full right (nose right; on the ground it steers). With the assist it is a push over what the assist's feet do: leave it at 0",
        get: |h| h.controls.rudder,
        set: |h, v| h.controls.rudder = v,
    },
    Control { name: "throttle", low: 0.0, high: 1.0, what: "0 idle, 1 full", get: |h| h.controls.throttle, set: |h, v| h.controls.throttle = v },
    Control { name: "flaps", low: 0.0, high: 3.0, what: "the lever's notch: 0 up, 1, 2, 3 for 10°, 20°, 30°", get: |h| h.controls.flap_notch as f64, set: |h, v| h.controls.flap_notch = v.round() as i32 },
    Control { name: "trim", low: -1.0, high: 1.0, what: "elevator trim, 1 full nose up (with the assist it does not matter: the assist holds the yoke)", get: |h| h.controls.trim, set: |h, v| h.controls.trim = v },
    Control {
        name: "brakes",
        low: 0.0,
        high: 1.0,
        what: "both wheel brakes: 1 full",
        get: |h| h.controls.brake_left.max(h.controls.brake_right),
        set: |h, v| (h.controls.brake_left, h.controls.brake_right) = (v, v),
    },
];

/// The pedals among `CONTROLS`: the assist's start lets them go.
const RUDDER: usize = 4;

/// Controls set for a while, over the ones held: when it ends they are the held ones again.
struct Pulse {
    /// Which of `CONTROLS`, and at what.
    set: Vec<(usize, f64)>,
    /// The world's time it ends at.
    until: f64,
}

/// The pilot a client is. It holds every control where the client last put it; with the assist
/// on, the keyboard's assisted pilot moves the yoke and the pedals to the attitude asked for.
pub struct RemoteHands {
    held: Hold,
    pulse: Option<Pulse>,
    /// The keyboard's pilot with its assist on, for `Hold::assisted`.
    assist: KeyboardPilot,
    /// The speed the nose is lifted at on the takeoff run, m/s (the type's).
    rotate: f64,
}

impl RemoteHands {
    /// Hands that hold the controls as they are now, the yoke where it is.
    fn taking(aircraft: &Aircraft) -> RemoteHands {
        let held = Hold { controls: aircraft.controls, assisted: false, pitch: 0.0, bank: 0.0 };
        RemoteHands { held, pulse: None, assist: KeyboardPilot::new(), rotate: aircraft.p.speeds.rotate }
    }

    /// What is held at this time of the world: the held controls, under the pulse while it lasts.
    fn at(&mut self, time: f64) -> Hold {
        if self.pulse.as_ref().is_some_and(|p| time >= p.until) {
            self.pulse = None;
        }
        let mut hold = self.held;
        for &(control, value) in self.pulse.iter().flat_map(|p| &p.set) {
            (CONTROLS[control].set)(&mut hold, value);
        }
        hold
    }

    /// Seconds of the pulse still to go; 0 with none.
    fn pulse_left(&self, time: f64) -> f64 {
        self.pulse.as_ref().map_or(0.0, |p| (p.until - time).max(0.0))
    }
}

impl Pilot for RemoteHands {
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls {
        // Not an aircraft: nothing these hands can fly.
        let (Controls::Aircraft(_), Some(aircraft)) = (current, obs.aircraft()) else { return *current };
        let hold = self.at(obs.time);
        if !hold.assisted {
            // The assist, when it is asked for, starts afresh from the attitude there is: the
            // nose where it is, the wings where they are (level, if they nearly are).
            self.assist.reset();
            self.held.pitch = (obs.pitch / DEG).round();
            self.held.bank = if obs.roll.abs() < 5.0 * DEG { 0.0 } else { (obs.roll / DEG).round() };
            return Controls::Aircraft(hold.controls);
        }
        // On the ground the assist's yoke is the down arrow's: held from the rotation speed if a
        // nose-up attitude is asked for, as a player holds it to take off. In the air the
        // attitude is given to it as it is.
        let rotating = obs.on_ground && aircraft.indicated_airspeed >= self.rotate && hold.pitch * DEG > obs.pitch + DEG;
        self.assist.assist = true;
        self.assist.set_keys(0.0, if rotating { 1.0 } else { 0.0 }, hold.controls.rudder);
        self.assist.hold_attitude(Some(hold.pitch * DEG), Some(hold.bank * DEG));
        Controls::Aircraft(self.assist.fly_aircraft(obs, &hold.controls, dt))
    }
}

/// How long the state's means look back, seconds: about one swing of the nose (the phugoid), so
/// that they say where the speed and the climb are settling while they swing.
const MEAN_WINDOW: f64 = 30.0;
/// The seconds between two samples kept for them.
const SAMPLE_EVERY: f64 = 0.25;
/// How far back the speed's trend looks, seconds.
const TREND_WINDOW: f64 = 2.0;

/// What the session keeps of the remote: the hands, once a client has taken the aircraft, the
/// runway its state is told against, and the last half minute of the flight for the means.
#[derive(Default)]
pub struct Remote {
    pub hands: Option<RemoteHands>,
    runway: Option<Runway>,
    /// (the world's time, indicated airspeed m/s, vertical speed m/s), oldest first.
    samples: VecDeque<(f64, f64, f64)>,
    /// The aircraft has left the ground since it was last put somewhere.
    flew: bool,
    /// It was on the ground when last looked at.
    on_ground: bool,
    /// The landing of this flight, from its first touch.
    landing: Option<Landing>,
}

/// Where and how hard the aircraft came down.
#[derive(Clone, Copy)]
struct Landing {
    /// The hardest touch of it (a bounce touches again), m/s of sink.
    sink: f64,
    /// Where the first touch was from the runway in use: metres past its threshold, metres
    /// right of its centreline, and whether on its surface. `None` if no runway was known then.
    place: Option<(f64, f64, bool)>,
}

/// What the remote calls a landing by its hardest touch, m/s of sink: up to `FIRM` good, up to
/// `HARD` firm, beyond it hard. The remote's own grades, not the airframe's limits (the gear took
/// 560 ft/min with a bounce when it was measured).
const FIRM: f64 = 300.0 * FEET / 60.0;
const HARD: f64 = 500.0 * FEET / 60.0;
/// A landing further from the centreline than this share of the runway's width is said to be so.
const OFF_CENTRE: f64 = 0.25;
/// Above this height over the runway after a touch, the aircraft is flying again: the landing
/// is forgotten (a go-around, a touch and go), m.
const FLYING_AGAIN: f64 = 30.0;

impl Remote {
    /// A client flies the player's aircraft.
    pub fn flying(&self) -> bool {
        self.hands.is_some()
    }

    /// The aircraft was put somewhere else (a restart): the hands hold the controls it was left
    /// with there, not the ones of the flight before; the runway is looked for again and the
    /// means start again.
    pub fn restarted(&mut self, aircraft: Option<&Aircraft>) {
        self.runway = None;
        self.samples.clear();
        (self.flew, self.on_ground, self.landing) = (false, true, None);
        match (&mut self.hands, aircraft) {
            (Some(hands), Some(aircraft)) => *hands = RemoteHands::taking(aircraft),
            // No aircraft any more (another kind of vehicle was chosen): nothing to hold.
            (hands, None) => *hands = None,
            (None, Some(_)) => {}
        }
    }

    /// Keeps the flight as it is now for the means, and sees it touch down: called every frame,
    /// whoever flies.
    pub fn record(&mut self, time: f64, aircraft: &Aircraft) {
        let on_ground = aircraft.data().on_ground();
        let (north, east, altitude) = (aircraft.state.pos.x, aircraft.state.pos.y, -aircraft.state.pos.z);
        if on_ground && self.flew {
            let sink = aircraft.last_touchdown().unwrap_or(0.0);
            let place = self.runway.map(|runway| {
                let (along, across) = runway.local(north, east);
                (along, across, runway.on_runway(north, east))
            });
            // The first touch says where; every touch may be the hardest.
            let landing = self.landing.get_or_insert(Landing { sink, place });
            landing.sink = landing.sink.max(sink);
        } else if !on_ground {
            self.flew = true;
            if self.runway.is_some_and(|runway| altitude - runway.airport.elevation > FLYING_AGAIN) {
                self.landing = None;
            }
        }
        self.on_ground = on_ground;
        if self.samples.back().is_some_and(|&(last, ..)| time < last + SAMPLE_EVERY) {
            return;
        }
        self.samples.push_back((time, aircraft.indicated_airspeed(), aircraft.vertical_speed()));
        while self.samples.front().is_some_and(|&(first, ..)| first < time - MEAN_WINDOW) {
            self.samples.pop_front();
        }
    }

    /// The means of the airspeed and the vertical speed over the last `MEAN_WINDOW` (m/s), and
    /// how fast the airspeed changes now (m/s²). `None` before anything was kept.
    fn trends(&self) -> Option<(f64, f64, f64)> {
        let &(now, speed, _) = self.samples.back()?;
        let n = self.samples.len() as f64;
        let (mean_speed, mean_climb) = self.samples.iter().fold((0.0, 0.0), |(s, c), &(_, speed, climb)| (s + speed / n, c + climb / n));
        let &(then, before, _) = self.samples.iter().find(|&&(time, ..)| time >= now - TREND_WINDOW)?;
        Some((mean_speed, mean_climb, if now > then { (speed - before) / (now - then) } else { 0.0 }))
    }
}

/// `controls`: sets the controls named, for good or, with `for`, for that many seconds of the
/// world's time. The first one takes the aircraft from the keyboard.
fn set_controls(session: &mut Session, body: &Json) -> Result<(), String> {
    let names = || CONTROLS.iter().map(|c| c.name).collect::<Vec<_>>().join(", ");
    let pairs = body.as_object().filter(|pairs| !pairs.is_empty()).ok_or_else(|| format!("controls needs an object of controls by name: {}", names()))?;
    if session.aircraft().is_none() {
        return Err(format!("the remote flies aircraft only, so far: this is the {}", session.craft().name()));
    }
    let mut set = Vec::new();
    let mut seconds = None;
    for (name, value) in pairs {
        let number = value.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("{name} needs a number, not {value}"))?;
        if name == "for" {
            if number <= 0.0 {
                return Err("for needs seconds, more than 0".to_string());
            }
            seconds = Some(number);
            continue;
        }
        let control = CONTROLS.iter().position(|c| c.name == name).ok_or_else(|| format!("no control {name}: {}, and for", names()))?;
        set.push((control, number.clamp(CONTROLS[control].low, CONTROLS[control].high)));
    }
    if set.is_empty() {
        return Err(format!("controls names no control: {}", names()));
    }
    if session.remote.hands.is_none() {
        // The game's own autopilot (L, or P without a GFC 700) lets go, as for any other pilot.
        session.autopilot = None;
    }
    let time = session.world.time();
    let taken = session.aircraft().map(RemoteHands::taking).expect("checked above");
    let hands = session.remote.hands.get_or_insert(taken);
    match seconds {
        Some(seconds) => hands.pulse = Some(Pulse { set, until: time + seconds }),
        None => {
            let was_assisted = hands.held.assisted;
            for &(control, value) in &set {
                (CONTROLS[control].set)(&mut hands.held, value);
            }
            // The assist's feet keep the aircraft straight by themselves: the pedals as they were
            // held by hand are let go, unless this very message says where they go.
            if hands.held.assisted && !was_assisted && !set.iter().any(|&(control, _)| control == RUDDER) {
                hands.held.controls.rudder = 0.0;
            }
        }
    }
    Ok(())
}

// ---- The orders

/// An order a client gives the game (`command`'s `do`).
struct Order {
    name: &'static str,
    what: &'static str,
    run: fn(&mut Session, &Json) -> Result<(), String>,
}

const ORDERS: [Order; 9] = [
    Order { name: "restart", what: "start again; where: runway (default), final or air", run: restart },
    Order { name: "pause", what: "the game waits", run: pause },
    Order { name: "resume", what: "the game goes on, with no menu over it", run: resume },
    Order { name: "release", what: "the aircraft goes back to the keyboard", run: release },
    Order { name: "runway", what: "the state's runway is from now the nearest one, in the direction nearest the heading", run: runway },
    Order { name: "view", what: "what the window shows; which: cockpit or outside", run: view },
    Order { name: "wind", what: "the surface wind, for this run: from_deg (true) and speed_kt, either or both", run: wind },
    Order { name: "time", what: "how many times faster than real time the world goes; times: 1, 2, 4, 8 or 16", run: time },
    Order { name: "autopilot", what: "works the GFC 700; keys: its keys and settings in order, with commas (ap,hdg,bug=270,sel=3000,alt,vs,flc,apr,dto,disc)", run: autopilot },
];

fn restart(session: &mut Session, body: &Json) -> Result<(), String> {
    match body.get("where").map_or(Some("runway"), Json::as_str) {
        Some("runway") => session.restart_on_runway(),
        Some("final") => session.restart_on_final(),
        Some("air") => session.restart_in_air(),
        _ => return Err("restart's where is runway, final or air".to_string()),
    }
    Ok(())
}

fn pause(session: &mut Session, _: &Json) -> Result<(), String> {
    session.pause();
    Ok(())
}

fn resume(session: &mut Session, _: &Json) -> Result<(), String> {
    session.resume();
    Ok(())
}

fn time(session: &mut Session, body: &Json) -> Result<(), String> {
    let times = body.get("times").and_then(Json::as_f64).ok_or("time needs times: 1, 2, 4, 8 or 16")?;
    if !crate::game::TIME_RATES.iter().any(|r| *r as f64 == times) {
        return Err("time's times is 1, 2, 4, 8 or 16".to_string());
    }
    session.set_time_rate(times as u32);
    Ok(())
}

fn runway(session: &mut Session, _: &Json) -> Result<(), String> {
    session.remote.runway = None;
    Ok(())
}

fn release(session: &mut Session, _: &Json) -> Result<(), String> {
    if session.remote.hands.take().is_some() {
        // The keyboard's pilot takes over from the attitude there is.
        session.pilot.reset();
    }
    Ok(())
}

fn view(session: &mut Session, body: &Json) -> Result<(), String> {
    session.cockpit = match body.get("which").and_then(Json::as_str) {
        Some("cockpit") => true,
        Some("outside") => false,
        _ => return Err("view's which is cockpit or outside".to_string()),
    };
    Ok(())
}

fn wind(session: &mut Session, body: &Json) -> Result<(), String> {
    let (from, speed) = (body.get("from_deg").and_then(Json::as_f64), body.get("speed_kt").and_then(Json::as_f64));
    if from.is_none() && speed.is_none() {
        return Err("wind needs from_deg, speed_kt or both".to_string());
    }
    let weather = &mut session.world.weather;
    if let Some(from) = from {
        weather.wind_from = from.rem_euclid(360.0);
    }
    if let Some(speed) = speed {
        weather.wind_speed = speed.clamp(0.0, 60.0) * KNOTS;
    }
    Ok(())
}

/// The GFC 700's keys and settings, as the command line's `--afcs` takes them (`Session::afcs_command`).
fn autopilot(session: &mut Session, body: &Json) -> Result<(), String> {
    let keys = body.get("keys").and_then(Json::as_str).ok_or("autopilot needs keys: ap,hdg,bug=270,sel=3000...")?;
    for word in keys.split(',').map(str::trim).filter(|word| !word.is_empty()) {
        session.afcs_command(word).map_err(|error| error.trim_start_matches("--afcs: ").to_string())?;
    }
    Ok(())
}

/// `command`: runs the order named by `do`; the answer names it and the world's time.
fn command(session: &mut Session, body: &Json) -> Result<Json, String> {
    let names = || ORDERS.iter().map(|o| o.name).collect::<Vec<_>>().join(", ");
    let name = body.get("do").and_then(Json::as_str).ok_or_else(|| format!("command needs \"do\": {}", names()))?;
    let order = ORDERS.iter().find(|o| o.name == name).ok_or_else(|| format!("no order {name}: {}", names()))?;
    (order.run)(session, body)?;
    Ok(Json::obj(vec![("done", Json::str(order.name)), ("time_s", number(session.world.time(), 2))]))
}

// ---- What the game says

/// A number with no more decimals than mean anything.
fn number(x: f64, decimals: i32) -> Json {
    let k = 10f64.powi(decimals);
    Json::Num((x * k).round() / k)
}

/// An angle in degrees, 0 to 360.
fn compass(radians: f64) -> f64 {
    (radians / DEG).rem_euclid(360.0)
}

/// `describe`: what there is to fly here, and with what.
fn describe(session: &Session) -> Json {
    let controls = CONTROLS.iter().map(|c| Json::obj(vec![("name", Json::str(c.name)), ("low", Json::Num(c.low)), ("high", Json::Num(c.high)), ("description", Json::str(c.what))]));
    let orders = ORDERS.iter().map(|o| Json::obj(vec![("do", Json::str(o.name)), ("description", Json::str(o.what))]));
    // The type's own speeds, knots: what its handbook flies each part of a circuit at.
    let speeds = session.aircraft().map_or(Json::Null, |a| {
        let s = &a.p.speeds;
        let knots = |speed: f64| number(speed / KNOTS, 0);
        Json::obj(vec![
            ("stall_clean_kt", knots(s.stall_clean)),
            ("stall_full_kt", knots(s.stall_full)),
            ("rotate_kt", knots(s.rotate)),
            ("climb_kt", knots(s.climb)),
            ("pattern_kt", knots(s.pattern)),
            ("base_kt", knots(s.base)),
            ("final_kt", knots(s.final_approach)),
            ("flaps_max_kt", knots(s.flaps)),
        ])
    });
    Json::obj(vec![
        ("protocol", Json::Num(1.0)),
        ("server", Json::str(concat!("skylab ", env!("CARGO_PKG_VERSION")))),
        ("clock", Json::str("real")),
        ("vehicle", Json::str(session.craft().key())),
        ("speeds", speeds),
        ("controls", Json::Arr(controls.collect())),
        ("commands", Json::Arr(orders.collect())),
    ])
}

/// `hello`: only the clock that runs by itself can be asked for, so far.
fn hello(session: &Session, body: &Json) -> Result<Json, String> {
    match body.get("clock").map(Json::as_str) {
        None | Some(Some("real")) => Ok(describe(session)),
        _ => Err("the only clock is \"real\": the game runs by itself (a stepped clock is not built yet)".to_string()),
    }
}

/// The runway the state is told against: the nearest aerodrome's, in the direction the aircraft
/// faces while it stands on the ground, and kept from then on (a circuit flies its downwind leg
/// the other way and it is still the same runway).
fn runway_in_use(session: &mut Session, obs: &Observation) -> Option<Runway> {
    let standing = obs.on_ground && obs.velocity[0].hypot(obs.velocity[1]) < 2.0;
    if standing || session.remote.runway.is_none() {
        session.remote.runway = Runway::nearest(obs);
    }
    session.remote.runway
}

/// `state`: the numbers a pilot flies by, in a cockpit's units. Every name is in `PROTOCOL.md`.
fn state(session: &mut Session) -> Json {
    let obs = session.world.observe(session.player);
    let time = obs.time;
    let [north, east, down] = obs.velocity;
    let ground_speed = north.hypot(east);
    let flown_by = match (&session.remote.hands, &session.autopilot) {
        (Some(_), _) => "remote",
        (None, Some(_)) => "autopilot",
        (None, None) => "player",
    };
    let mut fields = vec![
        ("brief", Json::Null),
        ("time_s", number(time, 2)),
        ("paused", Json::Bool(session.paused)),
        ("time_rate", Json::Num(session.time_rate() as f64)),
        ("time_rate_achieved", number(session.achieved_rate(), 1)),
        ("flown_by", Json::str(flown_by)),
        ("vehicle", Json::str(session.craft().key())),
        ("message", Json::str(session.message_text().replace('\n', " "))),
        ("on_ground", Json::Bool(obs.on_ground)),
        ("crashed", obs.crashed.as_deref().map_or(Json::Null, Json::str)),
        ("ground_speed_kt", number(ground_speed / KNOTS, 1)),
        ("altitude_ft", number(obs.altitude / FEET, 0)),
        ("heading_deg", number(compass(obs.heading), 1)),
        ("track_deg", number(if ground_speed > 1.0 { compass(east.atan2(north)) } else { compass(obs.heading) }, 1)),
        ("pitch_deg", number(obs.pitch / DEG, 1)),
        ("roll_deg", number(obs.roll / DEG, 1)),
        ("roll_rate_deg_s", number(obs.angular_rates[0] / DEG, 1)),
        ("pitch_rate_deg_s", number(obs.angular_rates[1] / DEG, 1)),
        ("wind", {
            let w = session.world.weather;
            Json::obj(vec![("from_deg", number(w.wind_from, 0)), ("speed_kt", number(w.wind_speed / KNOTS, 1))])
        }),
    ];
    let Some((a, p, controls, touchdown)) = session.aircraft().map(|a| (obs.aircraft().cloned(), a.p.cg_height, a.controls, a.last_touchdown())) else {
        // Not an aircraft: what any vehicle has.
        fields.push(("height_ft", number(obs.agl / FEET, 0)));
        fields.push(("vertical_speed_fpm", number(-down / FEET * 60.0, 0)));
        return Json::obj(fields);
    };
    let a = a.unwrap_or_default();
    // What the remote holds: its attitude with the assist, its controls without. The yoke and
    // the pedals are told where they are (the assist moves them, or the GFC 700's servos).
    let engaged = session.gfc.as_ref().is_some_and(Gfc700::engaged);
    let hold = session.remote.hands.as_mut().map(|hands| hands.at(time));
    let shown = match hold {
        Some(hold) if hold.assisted || engaged => Hold { controls: AircraftControls { throttle: hold.controls.throttle, flap_notch: hold.controls.flap_notch, ..controls }, ..hold },
        Some(hold) => hold,
        None => Hold { controls, assisted: false, pitch: (obs.pitch / DEG).round(), bank: (obs.roll / DEG).round() },
    };
    let assisted = hold.is_some_and(|hold| hold.assisted);
    let (mean_speed, mean_climb, speed_trend) = session.remote.trends().unwrap_or((a.indicated_airspeed, a.vertical_speed, 0.0));
    fields.extend([
        ("stall_warning", Json::Bool(a.stall_warning)),
        ("airspeed_kt", number(a.indicated_airspeed / KNOTS, 1)),
        ("airspeed_avg_kt", number(mean_speed / KNOTS, 0)),
        ("airspeed_trend_kt_s", number(speed_trend / KNOTS, 1)),
        ("height_ft", number((obs.agl - p).max(0.0) / FEET, 0)),
        ("vertical_speed_fpm", number(a.vertical_speed / FEET * 60.0, 0)),
        ("vertical_speed_avg_fpm", number(mean_climb / FEET * 60.0, 0)),
        ("turn_rate_deg_s", number(a.turn_rate / DEG, 1)),
        // The circle it flies at this bank and speed; none with the wings near level.
        ("turn_radius_m", if obs.roll.abs() > 3.0 * DEG { number(ground_speed * ground_speed / (GRAVITY * obs.roll.abs().tan()), 0) } else { Json::Null }),
        ("alpha_deg", number(a.alpha / DEG, 1)),
        ("slip_deg", number(a.slip_ball / DEG, 1)),
        ("rpm", number(a.rpm, 0)),
        ("flaps_deg", number(a.flap_deg, 0)),
        ("assist", Json::Bool(assisted)),
        ("controls", Json::obj(CONTROLS.iter().map(|c| (c.name, number((c.get)(&shown), 3))).collect())),
        // Rounded up: 0 only once it is over (`skylab-ctl --wait` waits for that).
        ("pulse_s", Json::Num((session.remote.hands.as_ref().map_or(0.0, |h| h.pulse_left(time)) * 100.0).ceil() / 100.0)),
        ("autopilot", Json::str(session.gfc.as_ref().filter(|g| g.engaged()).map_or("off".to_string(), modes))),
        ("touchdown_fpm", touchdown.map_or(Json::Null, |sink| number(sink / FEET * 60.0, 0))),
    ]);
    let mut brief = format!(
        "{:.0} kt ({:+.1}/s, mean {:.0}) · {:.0} ft up, {:+.0} fpm (mean {:+.0}) · nose {:+.0}° · {} · heading {:03.0}°{}",
        a.indicated_airspeed / KNOTS,
        speed_trend / KNOTS,
        mean_speed / KNOTS,
        (obs.agl - p).max(0.0) / FEET,
        a.vertical_speed / FEET * 60.0,
        mean_climb / FEET * 60.0,
        obs.pitch / DEG,
        sided(obs.roll / DEG, "wings level", "° bank right", "° bank left", 1.5),
        compass(obs.heading),
        match sided(a.turn_rate / DEG, "", "°/s", "°/s", 0.3).as_str() {
            "" => String::new(),
            rate => format!(", turning {} {rate}", if a.turn_rate > 0.0 { "right" } else { "left" }),
        },
    );
    if obs.on_ground {
        brief = format!("ON THE GROUND · {brief}");
    }
    if let Some(runway) = runway_in_use(session, &obs) {
        let (along, across) = runway.local(obs.north, obs.east);
        // Where it will be a second from now, for how fast it leaves or comes to the centreline.
        let across_rate = runway.local(obs.north + north, obs.east + east).1 - across;
        let height = obs.altitude - runway.airport.elevation - p;
        let path_error = height - runway.path_height(along);
        let heading_error = (compass(runway.heading()) - compass(obs.heading) + 180.0).rem_euclid(360.0) - 180.0;
        let number_painted = ((runway.heading() / DEG / 10.0).round() as i64 - 1).rem_euclid(36) + 1;
        // The vertical speed that takes it from here straight to the aiming point at this speed
        // over the ground: what to fly to be on the path there, wherever it is now.
        let to_aim = (AIMING_POINT - along).hypot(across);
        let needed = (along < AIMING_POINT - 50.0 && !obs.on_ground && ground_speed > 5.0).then(|| -height / (to_aim / ground_speed));
        let closing = if across.abs() < 3.0 || across_rate.abs() < 0.5 {
            String::new()
        } else if across * across_rate < 0.0 {
            format!(", closing {:.0} m/s", across_rate.abs())
        } else {
            format!(", GOING AWAY {:.0} m/s", across_rate.abs())
        };
        brief += &format!(
            " · runway {number_painted:02}: {}, {}{closing}, nose {}",
            if along < 0.0 { format!("{:.0} m before the threshold", -along) } else { format!("{along:.0} m past the threshold") },
            sided(across, "on the centreline", " m RIGHT of the centreline", " m LEFT of the centreline", 3.0),
            sided(-heading_error, "along it", "° right of it", "° left of it", 1.5),
        );
        if along < 0.0 && !obs.on_ground {
            brief += &format!(", {}", sided(path_error / FEET, "on the path", " ft above the path", " ft below the path", 15.0));
            if let Some(needed) = needed {
                brief += &format!(" ({:+.0} fpm goes to the touchdown point)", needed / FEET * 60.0);
            }
        }
        fields.push((
            "runway",
            Json::obj(vec![
                ("number", Json::Num(number_painted as f64)),
                ("heading_deg", number(compass(runway.heading()), 1)),
                ("elevation_ft", number(runway.airport.elevation / FEET, 0)),
                ("length_m", number(runway.airport.length, 0)),
                ("width_m", number(runway.airport.width, 0)),
                ("along_m", number(along, 0)),
                ("across_m", number(across, 1)),
                ("across_rate_m_s", number(across_rate, 1)),
                ("height_ft", number(height / FEET, 0)),
                ("path_error_ft", number(path_error / FEET, 0)),
                ("needed_fpm", needed.map_or(Json::Null, |needed| number(needed / FEET * 60.0, 0))),
                ("heading_error_deg", number(heading_error, 1)),
                ("nose_right_deg", number(-heading_error, 1)),
                ("over", Json::Bool(runway.on_runway(obs.north, obs.east))),
            ]),
        ));
    }
    // The landing: where the first touch was, how hard the hardest, and what to call it.
    if let Some(landing) = session.remote.landing {
        let over_now = session.remote.runway.is_none_or(|runway| runway.on_runway(obs.north, obs.east));
        let width = session.remote.runway.map_or(f64::INFINITY, |runway| runway.airport.width);
        let verdict = match landing.place {
            _ if obs.crashed.is_some() => "crashed",
            Some((_, _, false)) => "off the runway",
            _ if obs.on_ground && !over_now => "ran off the runway",
            _ if landing.sink > HARD => "hard",
            _ if landing.sink > FIRM => "firm",
            Some((_, across, _)) if across.abs() > width * OFF_CENTRE => "good, but far from the centreline",
            _ => "good",
        };
        let mut told = vec![("verdict", Json::str(verdict)), ("sink_fpm", number(landing.sink / FEET * 60.0, 0))];
        let mut said = format!("LANDED ({verdict}): {:.0} fpm", landing.sink / FEET * 60.0);
        if let Some((along, across, on_runway)) = landing.place {
            told.extend([("along_m", number(along, 0)), ("across_m", number(across, 1)), ("on_runway", Json::Bool(on_runway))]);
            said += &format!(", {along:.0} m past the threshold, {}", sided(across, "on the centreline", " m RIGHT of the centreline", " m LEFT of the centreline", 1.0));
        }
        fields.push(("landing", Json::obj(told)));
        brief = format!("{said} · {brief}");
    } else {
        fields.push(("landing", Json::Null));
    }
    let lever = shown.controls.flap_notch as f64 * 10.0;
    brief += &format!(
        " · throttle {:.2}, flaps {:.0}°{}, {}",
        shown.controls.throttle,
        a.flap_deg,
        if (lever - a.flap_deg).abs() > 0.5 { format!(" going to {lever:.0}°") } else { String::new() },
        if assisted { format!("assist holding nose {:+.0}° bank {:+.0}°", shown.pitch, shown.bank) } else { "no assist".to_string() }
    );
    if a.stall_warning {
        brief = format!("STALL WARNING · {brief}");
    }
    if let Some(why) = &obs.crashed {
        brief = format!("CRASHED: {why} · {brief}");
    }
    fields[0].1 = Json::str(brief);
    Json::obj(fields)
}

/// A number said with its side in words instead of its sign: `12° bank left`, or `level` within `dead`.
fn sided(value: f64, zero: &str, positive: &str, negative: &str, dead: f64) -> String {
    if value.abs() < dead {
        zero.to_string()
    } else {
        format!("{:.0}{}", value.abs(), if value > 0.0 { positive } else { negative })
    }
}

/// The modes the GFC 700 is in, as its status bar names them: `HDG ALT`.
fn modes(gfc: &Gfc700) -> String {
    let [lateral, _, vertical, _] = gfc.annunciations();
    format!("{lateral} {vertical}")
}

// ---- The socket

#[cfg(not(target_arch = "wasm32"))]
pub use socket::{listen, serve};

#[cfg(not(target_arch = "wasm32"))]
mod socket {
    use super::*;
    use ::remote::{message, Server};

    /// Opens the game to clients on this port, and says so (or why not).
    pub fn listen(port: u16) -> Option<Server> {
        match Server::listen(port) {
            Ok(server) => {
                eprintln!("skylab: remote control on 127.0.0.1:{} (skylab-ctl state)", server.port());
                Some(server)
            }
            Err(error) => {
                eprintln!("skylab: --remote cannot listen on 127.0.0.1:{port}: {error}");
                None
            }
        }
    }

    /// Answers every request that came in since the last frame.
    pub fn serve(session: &mut Session, server: &Server) {
        while let Some(request) = server.next() {
            match answer(session, request.kind, &request.body) {
                Ok((kind, body)) => request.answer(kind, body),
                Err(why) => request.refuse(&why),
            }
        }
    }

    /// The answer to one message: its type and payload, or why it is refused.
    pub fn answer(session: &mut Session, kind: u8, body: &Json) -> Result<(u8, Json), String> {
        match kind {
            message::HELLO => Ok((message::DESCRIBE, hello(session, body)?)),
            message::STATE => Ok((message::STATE_RESULT, state(session))),
            message::CONTROLS => {
                set_controls(session, body)?;
                Ok((message::STATE_RESULT, state(session)))
            }
            message::COMMAND => Ok((message::DONE, command(session, body)?)),
            message::FRAME | message::ADVANCE => Err(format!("{} is not built yet", message::name(kind))),
            _ => Err(format!("unknown message type {kind}")),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::socket::answer;
    use super::*;
    use crate::game::Held;
    use crate::settings::Store;
    use ::remote::message;

    const FRAME: f64 = 1.0 / 60.0;

    fn session() -> Session {
        Session::new(1, Store::memory())
    }

    fn fly(s: &mut Session, held: Held, seconds: f64) {
        for _ in 0..(seconds / FRAME).round() as usize {
            s.advance(FRAME, &held);
        }
    }

    fn json(text: &str) -> Json {
        Json::parse(text).unwrap()
    }

    /// Sends `controls` and returns the state answered.
    fn set(s: &mut Session, text: &str) -> Json {
        answer(s, message::CONTROLS, &json(text)).unwrap().1
    }

    fn state_of(s: &mut Session) -> Json {
        answer(s, message::STATE, &Json::Null).unwrap().1
    }

    fn at(state: &Json, path: &str) -> f64 {
        path.split('.').try_fold(state, |inside, name| inside.get(name)).and_then(Json::as_f64).unwrap_or_else(|| panic!("no number {path} in {state}"))
    }

    #[test]
    fn hello_describes_the_game_and_only_knows_the_real_clock() {
        let mut s = session();
        let (kind, description) = answer(&mut s, message::HELLO, &json(r#"{"protocol":1}"#)).unwrap();
        assert_eq!(kind, message::DESCRIBE);
        assert_eq!(description.get("vehicle").and_then(Json::as_str), Some("c172"));
        let names: Vec<&str> = description.get("controls").and_then(Json::as_array).unwrap().iter().filter_map(|c| c.get("name")?.as_str()).collect();
        assert_eq!(names, ["pitch_deg", "bank_deg", "elevator", "aileron", "rudder", "throttle", "flaps", "trim", "brakes"]);
        assert_eq!(description.get("speeds").and_then(|s| s.get("rotate_kt")).and_then(Json::as_f64), Some(55.0));
        assert!(answer(&mut s, message::HELLO, &json(r#"{"protocol":1,"clock":"stepped"}"#)).is_err());
        assert!(answer(&mut s, message::FRAME, &Json::Null).unwrap_err().contains("not built yet"));
    }

    #[test]
    fn the_first_controls_take_the_aircraft_from_the_keyboard_and_release_gives_it_back() {
        let mut s = session();
        assert_eq!(state_of(&mut s).get("flown_by").and_then(Json::as_str), Some("player"));
        let state = set(&mut s, r#"{"throttle":0.4,"flaps":1}"#);
        assert_eq!(state.get("flown_by").and_then(Json::as_str), Some("remote"));
        // The keys fly nothing now: W held does not open the throttle.
        fly(&mut s, Held { throttle_up: true, roll: 1.0, ..Held::default() }, 1.0);
        let controls = s.aircraft().unwrap().controls;
        assert_eq!((controls.throttle, controls.flap_notch, controls.aileron), (0.4, 1, 0.0));
        assert!(s.status_text().contains("remote control"));

        answer(&mut s, message::COMMAND, &json(r#"{"do":"release"}"#)).unwrap();
        fly(&mut s, Held { throttle_up: true, ..Held::default() }, 1.0);
        assert!(s.aircraft().unwrap().controls.throttle > 0.4);
        assert_eq!(state_of(&mut s).get("flown_by").and_then(Json::as_str), Some("player"));
    }

    #[test]
    fn a_pulse_lasts_its_seconds_of_the_world_and_leaves_what_was_held() {
        let mut s = session();
        set(&mut s, r#"{"aileron":-0.1}"#);
        let state = set(&mut s, r#"{"aileron":0.5,"rudder":0.2,"for":0.5}"#);
        assert!((at(&state, "pulse_s") - 0.5).abs() < 1e-9);
        fly(&mut s, Held::default(), 0.25);
        let controls = s.aircraft().unwrap().controls;
        assert_eq!((controls.aileron, controls.rudder), (0.5, 0.2));
        // Paused, the world's time stands still and so does the pulse.
        s.pause();
        fly(&mut s, Held::default(), 2.0);
        assert!((at(&state_of(&mut s), "pulse_s") - 0.25).abs() < 0.03);
        s.resume();
        fly(&mut s, Held::default(), 0.35);
        let controls = s.aircraft().unwrap().controls;
        assert_eq!((controls.aileron, controls.rudder), (-0.1, 0.0));
        assert_eq!(at(&state_of(&mut s), "pulse_s"), 0.0);
    }

    #[test]
    fn the_time_order_sets_how_fast_the_world_goes_and_the_state_says_it() {
        let mut s = session();
        s.restart_in_air();
        s.step_budget = std::time::Duration::from_secs(10);
        answer(&mut s, message::COMMAND, &json(r#"{"do":"time","times":4}"#)).unwrap();
        let time = s.world.time();
        fly(&mut s, Held::default(), 2.0);
        assert!((s.world.time() - time - 8.0).abs() < 0.02, "{}", s.world.time() - time);
        let state = state_of(&mut s);
        assert_eq!((at(&state, "time_rate"), at(&state, "time_rate_achieved")), (4.0, 4.0));
        assert!(answer(&mut s, message::COMMAND, &json(r#"{"do":"time","times":3}"#)).unwrap_err().contains("1, 2, 4, 8 or 16"));
        assert!(answer(&mut s, message::COMMAND, &json(r#"{"do":"time"}"#)).unwrap_err().contains("times"));
        answer(&mut s, message::COMMAND, &json(r#"{"do":"time","times":1}"#)).unwrap();
        assert_eq!(at(&state_of(&mut s), "time_rate"), 1.0);
    }

    #[test]
    fn what_is_wrong_with_controls_and_commands_is_said() {
        let mut s = session();
        let refused = |s: &mut Session, kind, text: &str| answer(s, kind, &json(text)).unwrap_err();
        assert!(refused(&mut s, message::CONTROLS, r#"{"wheel":1}"#).contains("no control wheel: pitch_deg"));
        assert!(refused(&mut s, message::CONTROLS, r#"{"for":1}"#).contains("names no control"));
        assert!(refused(&mut s, message::CONTROLS, r#"{"throttle":"full"}"#).contains("needs a number"));
        assert!(refused(&mut s, message::COMMAND, r#"{"do":"land"}"#).contains("no order land: restart"));
        assert!(refused(&mut s, message::COMMAND, r#"{"do":"restart","where":"moon"}"#).contains("runway, final or air"));
        assert!(refused(&mut s, message::COMMAND, r#"{"do":"wind"}"#).contains("from_deg, speed_kt"));
        assert_eq!(refused(&mut s, message::COMMAND, r#"{"do":"autopilot","keys":"warp"}"#), "unknown warp");
        answer(&mut s, message::COMMAND, &json(r#"{"do":"wind","from_deg":-90,"speed_kt":8}"#)).unwrap();
        assert_eq!((at(&state_of(&mut s), "wind.from_deg"), at(&state_of(&mut s), "wind.speed_kt")), (270.0, 8.0));
        // Nothing of that took the aircraft; a value beyond a control's range is brought into it.
        assert!(!s.remote.flying());
        assert_eq!(at(&set(&mut s, r#"{"throttle":7}"#), "controls.throttle"), 1.0);
    }

    #[test]
    fn a_takeoff_by_remote_and_a_restart_that_leaves_the_controls_of_the_new_start() {
        let mut s = session();
        fly(&mut s, Held::default(), 0.5);
        let start = state_of(&mut s);
        assert_eq!(start.get("on_ground").and_then(Json::as_bool), Some(true));
        // On the runway it starts on: over it, along it, at its height.
        assert_eq!(start.get("runway").and_then(|r| r.get("over")).and_then(Json::as_bool), Some(true));
        assert!(at(&start, "runway.heading_error_deg").abs() < 1.0 && at(&start, "runway.across_m").abs() < 2.0 && at(&start, "runway.height_ft").abs() < 2.0);
        assert!((at(&start, "runway.number") * 10.0 - at(&start, "runway.heading_deg")).abs() <= 5.0);

        // Flown as a client flies it: the state read ten times a second, the pedals for the
        // runway's heading (the propeller pulls the nose left), the wings level, the nose to 8°
        // from 55 knots.
        set(&mut s, r#"{"throttle":1}"#);
        for _ in 0..600 {
            fly(&mut s, Held::default(), 0.1);
            let state = state_of(&mut s);
            if at(&state, "height_ft") > 100.0 {
                break;
            }
            let rudder = (at(&state, "runway.heading_error_deg") * 0.05).clamp(-1.0, 1.0);
            let aileron = (-at(&state, "roll_deg") * 0.03).clamp(-1.0, 1.0);
            let elevator = if at(&state, "airspeed_kt") > 55.0 { ((8.0 - at(&state, "pitch_deg")) * 0.05).clamp(-0.2, 0.4) } else { 0.0 };
            set(&mut s, &format!(r#"{{"rudder":{rudder},"aileron":{aileron},"elevator":{elevator}}}"#));
        }
        let climbing = state_of(&mut s);
        assert_eq!(climbing.get("crashed"), Some(&Json::Null), "{climbing}");
        assert!(at(&climbing, "height_ft") > 100.0 && at(&climbing, "vertical_speed_fpm") > 200.0, "{climbing}");
        // Still the runway it left, and it is behind and below.
        assert!(at(&climbing, "runway.along_m") > at(&start, "runway.along_m") + 300.0 && at(&climbing, "runway.height_ft") > 90.0, "{climbing}");
        assert!(at(&climbing, "runway.across_m").abs() < 60.0 && at(&climbing, "runway.heading_error_deg").abs() < 10.0, "{climbing}");

        answer(&mut s, message::COMMAND, &json(r#"{"do":"restart"}"#)).unwrap();
        fly(&mut s, Held::default(), 1.0);
        let again = state_of(&mut s);
        assert_eq!(again.get("flown_by").and_then(Json::as_str), Some("remote"));
        assert_eq!((at(&again, "controls.throttle"), at(&again, "controls.elevator")), (0.0, 0.0));
        assert!(at(&again, "height_ft") < 3.0 && at(&again, "ground_speed_kt") < 1.0);
    }

    /// The whole of a takeoff and a climbing turn with three messages: the assist keeps the
    /// runway's line, lifts the nose at the rotation speed and holds the attitude asked for.
    #[test]
    fn with_the_assist_an_attitude_asked_for_is_flown_and_held() {
        let mut s = session();
        fly(&mut s, Held::default(), 0.5);
        let state = set(&mut s, r#"{"pitch_deg":8,"bank_deg":0,"throttle":1}"#);
        assert_eq!(state.get("assist").and_then(Json::as_bool), Some(true));
        fly(&mut s, Held::default(), 45.0);
        let climbing = state_of(&mut s);
        assert_eq!(climbing.get("crashed"), Some(&Json::Null), "{climbing}");
        assert!(at(&climbing, "height_ft") > 150.0, "{climbing}");
        assert!((at(&climbing, "pitch_deg") - 8.0).abs() < 1.5 && at(&climbing, "roll_deg").abs() < 2.0, "{climbing}");
        // Out along the runway with nobody on the pedals; wings level is not a heading held, and
        // the propeller takes it a few degrees left in the climb.
        assert!(at(&climbing, "runway.heading_error_deg").abs() < 12.0 && at(&climbing, "runway.across_m").abs() < 120.0, "{climbing}");
        // The means and the trend are of the climb.
        assert!(at(&climbing, "vertical_speed_avg_fpm") > 0.0 && at(&climbing, "airspeed_avg_kt") > 20.0 && at(&climbing, "airspeed_trend_kt_s").abs() < 3.0, "{climbing}");

        // A bank for a while: it turns, and comes back to the wings level it holds.
        set(&mut s, r#"{"bank_deg":-20,"for":10}"#);
        fly(&mut s, Held::default(), 8.0);
        let turning = state_of(&mut s);
        assert!((at(&turning, "roll_deg") + 20.0).abs() < 3.0 && at(&turning, "turn_rate_deg_s") < -3.0, "{turning}");
        assert!(turning.get("brief").and_then(Json::as_str).unwrap().contains("bank left"), "{turning}");
        fly(&mut s, Held::default(), 6.0);
        let level = state_of(&mut s);
        assert!(at(&level, "roll_deg").abs() < 2.0 && at(&level, "runway.heading_error_deg") > 30.0, "{level}");

        // The yoke by hand again: the assist lets go, and the attitude it would start from follows the aircraft.
        let direct = set(&mut s, r#"{"elevator":0,"aileron":0}"#);
        assert_eq!(direct.get("assist").and_then(Json::as_bool), Some(false));
    }

    #[test]
    fn the_brief_says_where_the_runway_is_in_words() {
        let mut s = session();
        s.restart_on_final();
        fly(&mut s, Held::default(), 1.0);
        let brief = state_of(&mut s).get("brief").and_then(Json::as_str).unwrap().to_string();
        assert!(brief.contains("m before the threshold") && brief.contains("on the centreline") && brief.contains("no assist"), "{brief}");
    }

    #[test]
    fn the_autopilot_order_works_the_gfc_700_and_its_servos_then_have_the_yoke() {
        let mut s = session();
        s.restart_in_air();
        set(&mut s, r#"{"throttle":0.65,"aileron":0.4}"#);
        fly(&mut s, Held::default(), 1.0);
        answer(&mut s, message::COMMAND, &json(r#"{"do":"autopilot","keys":"ap, hdg, alt"}"#)).unwrap();
        fly(&mut s, Held::default(), 20.0);
        let state = state_of(&mut s);
        assert_eq!(state.get("autopilot").and_then(Json::as_str), Some("HDG ALT"), "{state}");
        // The remote still holds the aileron over, and the servos fly wings near level all the same.
        assert!(at(&state, "roll_deg").abs() < 25.0 && at(&state, "vertical_speed_fpm").abs() < 300.0, "{state}");
        answer(&mut s, message::COMMAND, &json(r#"{"do":"autopilot","keys":"disc"}"#)).unwrap();
        assert_eq!(state_of(&mut s).get("autopilot").and_then(Json::as_str), Some("off"));
    }

    #[test]
    fn a_multirotor_s_state_is_told_and_its_controls_refused() {
        let mut s = session();
        s.set_craft(crate::kinds::Craft::parse("fpv").unwrap());
        let state = state_of(&mut s);
        assert!(state.get("height_ft").is_some() && state.get("airspeed_kt").is_none());
        assert!(answer(&mut s, message::CONTROLS, &json(r#"{"throttle":1}"#)).unwrap_err().contains("aircraft only"));
    }

    /// Every field the state and `describe` carry is named in the protocol's document.
    #[test]
    fn the_protocol_document_names_every_field() {
        let document = include_str!("../../remote/PROTOCOL.md");
        let mut s = session();
        set(&mut s, r#"{"throttle":0}"#);
        fn names(value: &Json, into: &mut Vec<String>) {
            for (name, inside) in value.as_object().unwrap_or(&[]) {
                into.push(name.clone());
                names(inside, into);
            }
        }
        let mut fields = Vec::new();
        names(&state_of(&mut s), &mut fields);
        names(&describe(&s), &mut fields);
        fields.extend(ORDERS.iter().map(|o| o.name.to_string()));
        for field in fields {
            assert!(document.contains(&format!("`{field}`")), "PROTOCOL.md does not name `{field}`");
        }
    }

    /// Flies until the state is as asked, looking thirty times a second as `skylab-ctl --when`
    /// does, then sends the controls; the state then. Panics if it never is.
    fn when(s: &mut Session, what: &str, met: impl Fn(&Json) -> bool, controls: &str) -> Json {
        for _ in 0..30 * 400 {
            if met(&state_of(s)) {
                return if controls.is_empty() { state_of(s) } else { set(s, controls) };
            }
            fly(s, Held::default(), 1.0 / 30.0);
        }
        panic!("never {what}: {}", state_of(s));
    }

    /// The circuit of `docs/AGENT-PILOT.md`, command by command, as an agent sends them: every
    /// number here is one the guide gives. It ends stopped on the runway it left, a good landing.
    #[test]
    fn the_guide_s_circuit_takes_off_and_lands_on_the_same_runway() {
        let mut s = session();
        fly(&mut s, Held::default(), 0.5);
        let near = |state: &Json, field: &str, value: f64, margin: f64| ((at(state, field) - value + 180.0).rem_euclid(360.0) - 180.0).abs() <= margin;

        // Takeoff and climb.
        set(&mut s, r#"{"pitch_deg":8,"bank_deg":1,"throttle":1}"#);
        // Crosswind at 500 feet.
        when(&mut s, "500 ft", |st| at(st, "runway.height_ft") > 500.0, r#"{"bank_deg":-20}"#);
        when(&mut s, "crosswind", |st| near(st, "runway.nose_right_deg", -83.0, 5.0), r#"{"bank_deg":1}"#);
        // Downwind, levelling off in the turn.
        when(&mut s, "900 m out", |st| at(st, "runway.across_m") < -900.0, r#"{"bank_deg":-20,"pitch_deg":3,"throttle":0.65}"#);
        let downwind = when(&mut s, "downwind", |st| near(st, "runway.nose_right_deg", -172.0, 5.0), r#"{"bank_deg":0}"#);
        println!("downwind: {}", downwind.get("brief").unwrap());
        // Base.
        let base = when(&mut s, "base point", |st| at(st, "runway.along_m") < -2500.0, r#"{"bank_deg":-20,"flaps":2,"throttle":0.5,"pitch_deg":-3}"#);
        println!("base turn: {}", base.get("brief").unwrap());
        when(&mut s, "base", |st| near(st, "runway.nose_right_deg", 98.0, 5.0), r#"{"bank_deg":0}"#);
        // Final.
        when(&mut s, "final turn", |st| at(st, "runway.across_m") > -500.0, r#"{"bank_deg":-20}"#);
        let rolled = when(&mut s, "final", |st| near(st, "runway.nose_right_deg", 8.0, 5.0), r#"{"bank_deg":0,"flaps":3}"#);
        println!("final: {}", rolled.get("brief").unwrap());
        set(&mut s, r#"{"throttle":0.62}"#);
        // Lined up as the guide says, a look and a small bank for a few seconds every six: the
        // nose a tenth of the metres off towards the centreline, the throttle for the path.
        loop {
            let st = state_of(&mut s);
            if at(&st, "runway.height_ft") < 150.0 {
                println!("at 150 ft: {}", st.get("brief").unwrap());
                break;
            }
            let wanted = (-at(&st, "runway.across_m") / 10.0).clamp(-10.0, 10.0);
            let turn = wanted - at(&st, "runway.nose_right_deg");
            if turn.abs() > 1.0 {
                set(&mut s, &format!(r#"{{"bank_deg":{},"for":{}}}"#, 8.0 * turn.signum(), (turn.abs() / 1.6).clamp(1.0, 5.0)));
            }
            let throttle = at(&st, "controls.throttle");
            let path = at(&st, "runway.path_error_ft");
            if path.abs() > 20.0 {
                set(&mut s, &format!(r#"{{"throttle":{}}}"#, (throttle - 0.05 * path.signum()).clamp(0.5, 0.7)));
            }
            fly(&mut s, Held::default(), 6.0);
        }
        // Touchdown and stop.
        when(&mut s, "30 ft", |st| at(st, "runway.height_ft") < 30.0, r#"{"pitch_deg":3,"throttle":0.45}"#);
        when(&mut s, "wheels down", |st| at(st, "runway.height_ft") < 1.0, r#"{"throttle":0,"brakes":1,"pitch_deg":0}"#);
        fly(&mut s, Held::default(), 20.0);
        let end = state_of(&mut s);
        println!("end: {}", end.get("brief").unwrap());
        assert_eq!(end.get("landing").and_then(|l| l.get("verdict")).and_then(Json::as_str), Some("good"), "{end}");
        assert!(at(&end, "ground_speed_kt") < 1.0 && end.get("runway").and_then(|r| r.get("over")) == Some(&Json::Bool(true)), "{end}");
    }
}
