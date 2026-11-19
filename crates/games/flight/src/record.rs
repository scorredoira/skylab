//! Recordings (`--record`): the shots of a script rendered frame by frame without a window, in the
//! game's own time however long each frame takes, to PNG files that a video is then made of.
//!
//! A frame is drawn larger than it is kept (`supersample` times each way) and as the average of
//! `subframes` pictures spread over the time the shutter is open (`shutter`, a fraction of the
//! frame: 0.5 is a film camera's 180°): what moves is blurred as a camera blurs it. The averages are
//! taken in linear light.
//!
//! A shot can have a lens (`lens`: depth of field, the sun's flare, darker corners, the glow, a
//! grade), slow motion (`speed`) and, with the script's `sound: true`, its sound: what its camera
//! hears, to `<dir>/<shot>.wav` (48 kHz stereo), which `scripts/video/assemble.sh` joins and puts
//! under the pictures.
//!
//! The script is RON:
//!
//! ```ron
//! #![enable(implicit_some)]
//! Script(
//!     width: 1920, height: 1080, fps: 60, supersample: 2, subframes: 12, shutter: 0.5,
//!     sound: true,                               // each shot's sound too (see `Sound`)
//!     lens: Lens(vignette: 0.2, warmth: 0.15),   // every shot's lens, unless it has its own (see `Lens`)
//!     shots: [
//!         Shot(
//!             name: "clouds",
//!             // The command line of the game: the place, the hour, the sky, the aircraft...
//!             args: "--place LEMH --hour 18.5 --sky fair --air",
//!             seconds: 2.0,
//!             // Where it starts instead (both optional): over a point, or down the runway.
//!             start: Over(n: 0, e: 0, heading: 90, agl: 1700),
//!             // start: Runway(back: 900, agl: 12),
//!             afcs: ["ap", "hdg", "bug=+40"],   // the GFC 700's keys (a C172), or
//!             keys: [(-4.0, -2.5, "power"), (0.5, 3.0, "right")],  // keys held, from and to (see `Shot::keys`)
//!             hold: true,                        // the game's autopilot holds heading and altitude
//!             before: 2.0,                       // seconds flown before the first frame
//!             // Slow motion: the world's rate at these seconds of the shot, eased between them.
//!             speed: [(0.8, 1.0), (1.1, 0.25), (1.6, 0.25), (1.9, 1.0)],
//!             lens: Lens(blur: 0.012, flare: 0.9, vignette: 0.25, bloom: 0.06, warmth: 0.2),
//!             camera: [
//!                 Chase(t: 0.0, yaw: 20, pitch: 5, distance: 22, fov: 40),
//!                 Chase(t: 2.0, yaw: 70, pitch: 10, distance: 18, fov: 40),
//!             ],
//!         ),
//!     ],
//! )
//! ```
//!
//! The camera's keys, all of one kind in a shot, go through a smooth curve (a cubic through each
//! key, its slope that of its neighbours), straight on before the first and after the last:
//! - `Chase`: round the aircraft, `yaw` degrees from behind it towards its right, `pitch` above,
//!   `distance` metres, looking at it (moved by `look`: metres forward, right, up). The way it
//!   turns round is the aircraft's heading, followed softly: not its bank, nor its pitch.
//! - `Fixed`: standing `eye` metres (forward, right, up) from where the aircraft was at the first
//!   frame, along its heading then, looking at it as it flies on.
//! - `Follow`: a chase drone: it flies for the point `back` metres behind the aircraft along its
//!   path (and `right`, `up` of it), always `lag` seconds late, so it trails farther the faster the
//!   aircraft goes, swings wide when it turns and catches up when it brakes; it looks at the
//!   aircraft (moved by `look`). It may fly as low as 0.25 m.
//! - `At`: standing at a point of the world (`n`, `e` metres north and east of its origin, `agl`
//!   above the ground there), turning to look at the aircraft (moved by `look`): a camera by a
//!   gate with a long lens (a small `fov`, and the lens's `blur` for its depth of field), or one
//!   high above looking down.
//! - `Cockpit`: in the cockpit at the game's spot `spot` (0 the seat, see `kinds::spots`), with any
//!   of its eye (body axes, metres), yaw, pitch and fov (degrees) and shift given instead.

use crate::audio::{Audio, Heard, Mixer, Params};
use crate::game::{Flight, Held};
use crate::kinds;
use crate::render::{to_render, Preset, ViewPoint};
use crate::settings::Store;
use crate::Command;
use engine::{Game, Offscreen};
use serde::Deserialize;
use sim_core::{act, axis_angle, euler_angles, DEG, V3};
use std::path::Path;
use std::time::Instant;

#[derive(Deserialize)]
struct Script {
    #[serde(default = "Script::width")]
    width: u32,
    #[serde(default = "Script::height")]
    height: u32,
    #[serde(default = "Script::fps")]
    fps: f64,
    #[serde(default = "Script::supersample")]
    supersample: u32,
    #[serde(default = "Script::subframes")]
    subframes: u32,
    #[serde(default = "Script::shutter")]
    shutter: f64,
    /// Each shot's sound too, to `<dir>/<shot>.wav` (`Sound`).
    #[serde(default)]
    sound: bool,
    /// The lens of every shot that has none of its own.
    #[serde(default)]
    lens: Option<Lens>,
    shots: Vec<Shot>,
}

/// What a shot's lens adds to the game's picture (`engine::Post`); nothing unless given.
#[derive(Deserialize, Clone, Copy, Debug)]
struct Lens {
    /// Depth of field: the widest blur of what is out of focus, as a fraction of the picture's
    /// height (0.012: a long lens wide open; 0: everything sharp).
    #[serde(default)]
    blur: f32,
    /// The distance it is focused on, metres; not given: on the aircraft, followed.
    #[serde(default)]
    focus: Option<f64>,
    /// How far either side of that stays sharp, metres; not given: 1 m and 4% of the distance.
    #[serde(default)]
    span: Option<f64>,
    /// The sun's flare when it is in the picture or near it (0.9 is the driving game's).
    #[serde(default)]
    flare: f32,
    /// How much darker the corners are, 0...1.
    #[serde(default)]
    vignette: f32,
    /// The glow round what is bright (a gate's LEDs, the sun), instead of the quality's (0.03).
    #[serde(default)]
    bloom: Option<f32>,
    /// The white balance: towards amber (positive) or blue, -1...1.
    #[serde(default)]
    warmth: f32,
    /// Added to the game's contrast, and stops added to its exposure.
    #[serde(default)]
    contrast: f32,
    #[serde(default)]
    exposure: f32,
    /// The game's saturation times this.
    #[serde(default = "Lens::saturation")]
    saturation: f32,
}

impl Lens {
    fn saturation() -> f32 {
        1.0
    }

    /// The game's lens for the picture from `view` (`aspect` wide for one high) at `hour`,
    /// focused on `subject`.
    fn of(&self, view: &ViewPoint, aspect: f32, hour: f64, subject: V3) -> crate::game::Lens {
        let distance = self.focus.unwrap_or_else(|| (subject - view.position).length());
        // Where the sun is in the picture (it may be outside it); none behind the camera or by night.
        let sun = world_render::sun(hour);
        let depth = sun.direction.dot(view.forward);
        let flare_sun = (depth > 1e-4 && sun.day > 0.2).then(|| {
            let tan = (view.fov_y * 0.5).tan();
            let x = sun.direction.dot(view.right) / (depth * tan * aspect);
            let y = sun.direction.dot(view.up) / (depth * tan) + view.shift_y;
            [0.5 + 0.5 * x, 0.5 - 0.5 * y]
        });
        crate::game::Lens {
            vignette: self.vignette,
            flare: if flare_sun.is_some() { self.flare * sun.day } else { 0.0 },
            flare_sun: flare_sun.unwrap_or([0.5, 0.5]),
            focus: if self.blur > 0.0 { distance as f32 } else { 0.0 },
            focus_span: self.span.unwrap_or(1.0 + 0.04 * distance) as f32,
            focus_blur: self.blur,
            near: view.near,
            bloom: self.bloom,
            warmth: self.warmth,
            contrast: self.contrast,
            exposure: self.exposure,
            saturation: self.saturation,
        }
    }
}

impl Script {
    fn width() -> u32 {
        1920
    }
    fn height() -> u32 {
        1080
    }
    fn fps() -> f64 {
        60.0
    }
    fn supersample() -> u32 {
        2
    }
    fn subframes() -> u32 {
        12
    }
    fn shutter() -> f64 {
        0.5
    }
}

#[derive(Deserialize)]
struct Shot {
    name: String,
    #[serde(default)]
    args: String,
    seconds: f64,
    #[serde(default)]
    start: Option<Begin>,
    #[serde(default)]
    afcs: Vec<String>,
    /// The GFC 700's keys worked during the shot: at so many seconds into it, this key.
    #[serde(default)]
    during: Vec<(f64, String)>,
    /// The game's keys held from so many seconds into the shot to so many (negative: in the time
    /// flown before it): "power" and "idle" (the throttle up and down), "left" and "right" (the
    /// pedals; a multirotor's yaw), "brake"; and a stick held part of the way, "roll=0.3",
    /// "pitch=-0.5" (back positive: forward is nose down), "yaw=0.2" (a multirotor's sticks spring
    /// to it).
    #[serde(default)]
    keys: Vec<(f64, f64, String)>,
    /// A multirotor's on-screen display drawn over the picture (`ui::osd`), as its goggles show it
    /// (and a race's, with `--course` in `args`).
    #[serde(default)]
    osd: bool,
    /// With `--course` in `args`: the race's autopilot flies it (from the countdown: `before` 3
    /// seconds or more to see it go).
    #[serde(default)]
    racer: bool,
    #[serde(default)]
    hold: bool,
    #[serde(default = "Shot::before")]
    before: f64,
    /// At the first frame, the aircraft diving at the ground: so many metres above it, the nose so
    /// many degrees down, at so many knots (and nobody pulling out). The camera follows its wreck.
    #[serde(default)]
    dive: Option<(f64, f64, f64)>,
    camera: Vec<Key>,
    /// A hand on the panel: a mouse pointer drawn over the picture that goes to the GFC 700's keys
    /// and knobs and works them (`Hands`).
    #[serde(default)]
    hands: Vec<Hand>,
    /// From so many seconds into the shot, the world runs this many times faster than the camera
    /// (a slow turn shown in less time).
    #[serde(default)]
    fast: Option<(f64, f64)>,
    /// Slow motion: at so many seconds into the shot the world runs at this rate (1 as it is, 0.25
    /// four times slower), eased from each to the next; before the first and after the last, theirs.
    /// The camera's keys and the shot's length stay in the film's time. The sound follows (`Sound`).
    #[serde(default)]
    speed: Vec<(f64, f64)>,
    /// This shot's lens, instead of the script's.
    #[serde(default)]
    lens: Option<Lens>,
    /// The time of the shot its still is taken at (`--storyboard`); the middle if not given.
    #[serde(default)]
    still: Option<f64>,
}

impl Shot {
    fn before() -> f64 {
        2.0
    }

    /// The world's seconds for one of the film's, `t` seconds into the shot (`fast`, `speed`).
    fn rate(&self, t: f64) -> f64 {
        let fast = self.fast.filter(|(from, _)| t >= *from).map_or(1.0, |(_, rate)| rate);
        let slow = match self.speed.iter().position(|(at, _)| t < *at) {
            None => self.speed.last().map_or(1.0, |(_, rate)| *rate),
            Some(0) => self.speed[0].1,
            Some(i) => {
                let ((t0, a), (t1, b)) = (self.speed[i - 1], self.speed[i]);
                let u = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                a + (b - a) * u * u * (3.0 - 2.0 * u)
            }
        };
        fast * slow.max(0.01)
    }

    /// The keys held at `t` seconds into the shot.
    fn held(&self, t: f64) -> Result<Held, String> {
        let mut held = Held::default();
        for (from, to, key) in &self.keys {
            if t < *from || t >= *to {
                continue;
            }
            match key.as_str() {
                "power" => held.throttle_up = true,
                "idle" => held.throttle_down = true,
                "left" => held.pedals = -1.0,
                "right" => held.pedals = 1.0,
                "brake" => held.brake = true,
                other => {
                    let (stick, value) = other.split_once('=').and_then(|(s, v)| Some((s, v.parse::<f64>().ok()?.clamp(-1.0, 1.0)))).ok_or_else(|| format!("{}: unknown key {other}", self.name))?;
                    match stick {
                        "roll" => held.roll = value,
                        "pitch" => held.pitch = value,
                        "yaw" => held.pedals = value,
                        _ => return Err(format!("{}: unknown key {other}", self.name)),
                    }
                }
            }
        }
        Ok(held)
    }
}

#[derive(Deserialize)]
enum Begin {
    /// In flight over this point (metres north and east of the middle of the place), heading this
    /// way (degrees), this high above the ground (metres).
    Over { n: f64, e: f64, heading: f64, agl: f64 },
    /// In flight down the runway it would start on, `back` metres before its middle, `agl` above it.
    Runway { back: f64, agl: f64 },
}

#[derive(Deserialize, Clone)]
enum Key {
    Chase {
        t: f64,
        yaw: f64,
        pitch: f64,
        distance: f64,
        fov: f64,
        #[serde(default)]
        look: [f64; 3],
    },
    Fixed {
        t: f64,
        eye: [f64; 3],
        fov: f64,
        #[serde(default)]
        look: [f64; 3],
    },
    Follow {
        t: f64,
        back: f64,
        #[serde(default)]
        right: f64,
        #[serde(default)]
        up: f64,
        lag: f64,
        fov: f64,
        #[serde(default)]
        look: [f64; 3],
    },
    At {
        t: f64,
        n: f64,
        e: f64,
        agl: f64,
        fov: f64,
        #[serde(default)]
        look: [f64; 3],
    },
    Cockpit {
        t: f64,
        #[serde(default)]
        spot: usize,
        #[serde(default)]
        eye: Option<[f64; 3]>,
        #[serde(default)]
        yaw: Option<f64>,
        #[serde(default)]
        pitch: Option<f64>,
        #[serde(default)]
        fov: Option<f64>,
        #[serde(default)]
        shift: Option<f64>,
    },
}

/// What the hand on the panel does, at so many seconds into the shot.
#[derive(Deserialize, Clone)]
enum Hand {
    /// Clicks a key of the PFD's bezel (ap, fd, hdg, alt, nav, vnv, apr, bc, vs, flc).
    Press { t: f64, key: String },
    /// Turns a knob of the PFD's bezel (hdg, alt, baro) so many clicks (negative: anticlockwise),
    /// one after another from `t` to `until`.
    Turn { t: f64, until: f64, knob: String, clicks: i32 },
    /// Goes away, down off the picture.
    Away { t: f64 },
}

impl Hand {
    fn t(&self) -> f64 {
        match self {
            Hand::Press { t, .. } | Hand::Turn { t, .. } | Hand::Away { t } => *t,
        }
    }
}

/// Where the pointer goes: a point of the panel (body axes), or off the picture.
#[derive(Clone, Copy)]
enum Spot3 {
    Panel(glam::Vec3),
    Off,
}

/// The hand's moves, worked out of its script: where it is when, and the clicks it makes.
struct Hands {
    /// Each stop: when the pointer starts for it, when it gets there, and where it is.
    stops: Vec<(f64, f64, Spot3)>,
    /// Each click: when, what it does, and the key's corners on the panel if it is a key.
    clicks: Vec<(f64, crate::cockpit::Panel, Option<[glam::Vec3; 4]>)>,
    /// The clicks made so far.
    done: usize,
}

/// Seconds a click's ring and light take to fade.
const CLICK_FADE: f64 = 0.35;

impl Hands {
    fn new(script: &[Hand], controls: &[crate::cockpit::PanelControl]) -> Result<Hands, String> {
        use crate::cockpit::{Panel, PanelKnob, PanelShape};
        use sim_core::AfcsKey;

        // The PFD's control (the leftmost of the two displays') that does this.
        let find = |wanted: &dyn Fn(&PanelShape) -> bool| controls.iter().filter(|c| wanted(&c.shape)).min_by(|a, b| a.centre.y.total_cmp(&b.centre.y)).copied();
        let mut stops = Vec::new();
        let mut clicks = Vec::new();
        let mut last = f64::NEG_INFINITY;
        for hand in script {
            let t = hand.t();
            // Off to the next place in at most 0.6 s, a little after the last.
            let leave = (t - 0.6).max(last + 0.12).min(t - 0.1);
            match hand {
                Hand::Press { key, .. } => {
                    let key = match key.as_str() {
                        "ap" => AfcsKey::Ap,
                        "fd" => AfcsKey::Fd,
                        "hdg" => AfcsKey::Hdg,
                        "alt" => AfcsKey::Alt,
                        "nav" => AfcsKey::Nav,
                        "apr" => AfcsKey::Apr,
                        "vs" => AfcsKey::Vs,
                        "flc" => AfcsKey::Flc,
                        other => return Err(format!("hands: unknown key {other}")),
                    };
                    let control = find(&|shape| matches!(shape, PanelShape::Key { does: Panel::Key(k), .. } if *k == key)).ok_or("hands: that key is not on the panel")?;
                    let PanelShape::Key { half, does } = control.shape else { unreachable!() };
                    let c = control.centre;
                    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(y, z)| c + glam::Vec3::new(0.0, y * half.x, z * half.y));
                    stops.push((leave, t - 0.08, Spot3::Panel(c + glam::Vec3::new(0.0, half.x * 0.2, half.y * 0.3))));
                    clicks.push((t, does, Some(corners)));
                    last = t;
                }
                Hand::Turn { until, knob, clicks: n, .. } => {
                    let wanted = match knob.as_str() {
                        "hdg" => PanelKnob::Heading,
                        "alt" => PanelKnob::AltitudeCoarse,
                        "baro" => PanelKnob::Baro,
                        other => return Err(format!("hands: unknown knob {other}")),
                    };
                    let control = find(&|shape| matches!(shape, PanelShape::Knob { outer, .. } if *outer == wanted)).ok_or("hands: that knob is not on the panel")?;
                    let PanelShape::Knob { radius, .. } = control.shape else { unreachable!() };
                    // On its right half to turn it clockwise, on its left half the other way.
                    let side = n.signum() as f32;
                    stops.push((leave, t - 0.08, Spot3::Panel(control.centre + glam::Vec3::new(0.0, side * radius * 0.55, 0.0))));
                    let count = n.unsigned_abs().max(1);
                    for i in 0..count {
                        let at = t + (i as f64 + 0.5) * (until - t) / count as f64;
                        clicks.push((at, Panel::Knob(wanted, n.signum()), None));
                    }
                    last = *until;
                }
                Hand::Away { .. } => {
                    stops.push((leave, t, Spot3::Off));
                    last = t;
                }
            }
        }
        clicks.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Hands { stops, clicks, done: 0 })
    }

    /// Makes the clicks due by `t`.
    fn act(&mut self, flight: &mut Flight, t: f64) {
        while let Some(&(at, does, _)) = self.clicks.get(self.done).filter(|c| c.0 <= t) {
            let _ = at;
            flight.session.panel(does);
            self.done += 1;
        }
    }

    /// The pointer at `t`, on the picture `screen` (points) seen from `view`; none before the first
    /// stop is started for, or once it has gone away.
    fn pointer(&self, flight: &Flight, view: &ViewPoint, screen: engine::egui::Rect, t: f64) -> Option<crate::game::Pointer> {
        use engine::egui::{pos2, Pos2};
        let first = self.stops.first()?;
        if t < first.0 {
            return None;
        }
        let plane = flight.session.vehicle();
        let off = pos2(screen.right() + 40.0, screen.bottom() + 60.0);
        let place = |spot: Spot3| -> Pos2 {
            match spot {
                Spot3::Panel(body) => crate::cockpit::on_screen(view, screen, plane, body).unwrap_or(off),
                Spot3::Off => off,
            }
        };
        let mut from = off;
        let mut at = off;
        for &(leave, arrive, spot) in &self.stops {
            let to = place(spot);
            if t < leave {
                at = from;
                break;
            }
            if t < arrive {
                // Eased, along a slight curve, as a hand moves.
                let u = ((t - leave) / (arrive - leave)) as f32;
                let e = u * u * (3.0 - 2.0 * u);
                let line = to - from;
                let bend = engine::egui::vec2(-line.y, line.x) * 0.08 * (std::f32::consts::PI * e).sin();
                at = from + line * e + bend;
                break;
            }
            at = to;
            from = to;
        }
        if matches!(self.stops.last(), Some(&(_, arrive, Spot3::Off)) if t >= arrive) {
            return None;
        }
        // The last click made, while it fades.
        let click = self.clicks.iter().rev().find(|c| c.0 <= t && t - c.0 < CLICK_FADE).map(|&(when, _, corners)| {
            let age = ((t - when) / CLICK_FADE) as f32;
            (age, corners.map(|c| c.map(|p| place(Spot3::Panel(p)))))
        });
        Some(crate::game::Pointer { at, click })
    }
}

/// The kinds of camera.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Chase,
    Fixed,
    Follow,
    At,
    Cockpit,
}

impl Key {
    fn t(&self) -> f64 {
        match self {
            Key::Chase { t, .. } | Key::Fixed { t, .. } | Key::Follow { t, .. } | Key::At { t, .. } | Key::Cockpit { t, .. } => *t,
        }
    }

    fn kind(&self) -> Kind {
        match self {
            Key::Chase { .. } => Kind::Chase,
            Key::Fixed { .. } => Kind::Fixed,
            Key::Follow { .. } => Kind::Follow,
            Key::At { .. } => Kind::At,
            Key::Cockpit { .. } => Kind::Cockpit,
        }
    }

    /// Its numbers, the ones the curve goes through.
    fn values(&self, flight: &Flight) -> Vec<f64> {
        match self {
            Key::Chase { yaw, pitch, distance, fov, look, .. } => vec![*yaw, *pitch, *distance, *fov, look[0], look[1], look[2]],
            Key::Fixed { eye, fov, look, .. } => vec![eye[0], eye[1], eye[2], *fov, look[0], look[1], look[2]],
            Key::Follow { back, right, up, lag, fov, look, .. } => vec![*back, *right, *up, *fov, look[0], look[1], look[2], *lag],
            Key::At { n, e, agl, fov, look, .. } => vec![*n, *e, *agl, *fov, look[0], look[1], look[2]],
            Key::Cockpit { spot, eye, yaw, pitch, fov, shift, .. } => {
                let spots = crate::multirotor::spots(flight.session.craft());
                let s = spots[(*spot).min(spots.len() - 1)];
                let eye = eye.unwrap_or([s.eye.x as f64, s.eye.y as f64, s.eye.z as f64]);
                vec![eye[0], eye[1], eye[2], yaw.unwrap_or(s.yaw as f64 / DEG), pitch.unwrap_or(s.pitch as f64 / DEG), fov.unwrap_or(s.fov as f64 / DEG), shift.unwrap_or(s.shift as f64)]
            }
        }
    }
}

/// A shot's camera: its keys' numbers along its curve, and what it follows.
struct Director {
    kind: Kind,
    times: Vec<f64>,
    values: Vec<Vec<f64>>,
    /// The heading the chase camera turns with, following the aircraft's softly, radians.
    heading: f64,
    /// Where the aircraft was at the first frame and its heading then (`Fixed`).
    origin: (V3, f64),
    /// Where the chase drone is, and the way the aircraft goes as it follows it (`Follow`).
    drone: V3,
    course: f64,
}

/// Seconds the chase camera's heading takes to follow two thirds of a turn.
const HEADING_LAG: f64 = 0.6;
/// The same for the way the aircraft goes, which the chase drone flies behind.
const COURSE_LAG: f64 = 0.25;
/// How low the chase drone and a standing camera may be above the ground, metres (the other
/// outside cameras keep 1.5 m).
const LOW_FLOOR: f64 = 0.25;

/// The way a craft goes over the ground, radians from north; standing (or going straight up or
/// down), the way it points.
fn course(velocity: V3, heading: f64) -> f64 {
    if velocity.x.hypot(velocity.y) > 2.0 {
        velocity.y.atan2(velocity.x)
    } else {
        heading
    }
}

impl Director {
    fn new(keys: &[Key], flight: &Flight) -> Result<Director, String> {
        let first = keys.first().ok_or("a shot without a camera")?;
        if keys.iter().any(|k| k.kind() != first.kind()) {
            return Err("a shot's camera keys are all of one kind".to_string());
        }
        if keys.windows(2).any(|w| w[1].t() <= w[0].t()) {
            return Err("the camera keys go in order of time".to_string());
        }
        let s = flight.session.body();
        let heading = euler_angles(s.q).heading;
        let mut director = Director { kind: first.kind(), times: keys.iter().map(Key::t).collect(), values: keys.iter().map(|k| k.values(flight)).collect(), heading, origin: (s.pos, heading), drone: s.pos, course: heading };
        if director.kind == Kind::Follow {
            // Where it would be had it followed all along: its lag behind where it flies for.
            let v = director.at(0.0);
            let velocity = s.world_velocity();
            director.course = course(velocity, heading);
            director.drone = director.station(s.pos, &v) - velocity * v[7].max(0.0);
        }
        Ok(director)
    }

    /// Where the chase drone flies for: behind the aircraft at `at` along its course.
    fn station(&self, at: V3, v: &[f64]) -> V3 {
        let (forward, right) = (V3::new(self.course.cos(), self.course.sin(), 0.0), V3::new(-self.course.sin(), self.course.cos(), 0.0));
        at - forward * v[0] + right * v[1] - V3::Z * v[2]
    }

    /// The aircraft flew on `dt` seconds (it is now `t` into the shot): the heading followed, and
    /// the chase drone flown on after it.
    fn follow(&mut self, flight: &Flight, t: f64, dt: f64) {
        let s = flight.session.body();
        let wrapped = |a: f64| (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        let h = euler_angles(s.q).heading;
        self.heading += wrapped(h - self.heading) * (1.0 - (-dt / HEADING_LAG).exp());
        if self.kind == Kind::Follow {
            let v = self.at(t);
            self.course += wrapped(course(s.world_velocity(), h) - self.course) * (1.0 - (-dt / COURSE_LAG).exp());
            let station = self.station(s.pos, &v);
            self.drone += (station - self.drone) * (1.0 - (-dt / v[7].max(1e-3)).exp());
        }
    }

    /// The numbers at `t` seconds into the shot.
    fn at(&self, t: f64) -> Vec<f64> {
        curve(&self.times, &self.values, t)
    }

    fn view(&self, flight: &Flight, t: f64) -> ViewPoint {
        let v = self.at(t);
        let session = &flight.session;
        let s = session.body();
        // What the outside cameras look at: the aircraft, or once it has broken up its wreck.
        let focus = session.player_wreck().map_or(s.pos, |w| w.main().pos);
        // Forward, right and up along a heading, world axes (north, east, down).
        let frame = |h: f64| (V3::new(h.cos(), h.sin(), 0.0), V3::new(-h.sin(), h.cos(), 0.0), V3::new(0.0, 0.0, -1.0));
        let offset = |h: f64, f: f64, r: f64, u: f64| {
            let (fw, rt, up) = frame(h);
            fw * f + rt * r + up * u
        };
        match self.kind {
            Kind::Cockpit => {
                let head = s.q * axis_angle(v[3] * DEG, V3::Z) * axis_angle(v[4] * DEG, V3::Y);
                ViewPoint {
                    position: s.pos + act(s.q, V3::new(v[0], v[1], v[2])),
                    forward: to_render(act(head, V3::X)),
                    up: to_render(act(head, V3::NEG_Z)),
                    right: to_render(act(head, V3::Y)),
                    fov_y: (v[5] * DEG) as f32,
                    shift_y: v[6] as f32,
                    near: 0.05,
                }
            }
            Kind::Chase => {
                let target = focus + offset(self.heading, v[4], v[5], v[6]);
                let azimuth = self.heading + std::f64::consts::PI - v[0] * DEG;
                let pitch = v[1] * DEG;
                let eye = focus + V3::new(azimuth.cos() * pitch.cos(), azimuth.sin() * pitch.cos(), -pitch.sin()) * v[2];
                looking(session, eye, target, v[3], 1.5, self.heading)
            }
            Kind::Fixed => {
                let (origin, h0) = self.origin;
                let eye = origin + offset(h0, v[0], v[1], v[2]);
                let target = focus + offset(self.heading, v[4], v[5], v[6]);
                looking(session, eye, target, v[3], 1.5, self.heading)
            }
            Kind::Follow => {
                let target = focus + offset(self.course, v[4], v[5], v[6]);
                looking(session, self.drone, target, v[3], LOW_FLOOR, self.course)
            }
            Kind::At => {
                let eye = V3::new(v[0], v[1], -(session.world.terrain.height(v[0], v[1]).max(0.0) + v[2]));
                let target = focus + offset(self.heading, v[4], v[5], v[6]);
                looking(session, eye, target, v[3], LOW_FLOOR, self.heading)
            }
        }
    }
}

/// From `eye` (kept `floor` metres above the ground) towards `target`, `fov` degrees high; looking
/// straight down or up, the top of the picture is towards `heading`.
fn looking(session: &crate::game::Session, mut eye: V3, target: V3, fov: f64, floor: f64, heading: f64) -> ViewPoint {
    let floor = session.world.terrain.height(eye.x, eye.y).max(0.0) + floor;
    if -eye.z < floor {
        eye.z = -floor;
    }
    let forward = (target - eye).normalize();
    let level = V3::Z.cross(forward);
    let right = if level.length() > 1e-6 { level.normalize() } else { V3::new(-heading.sin(), heading.cos(), 0.0) };
    ViewPoint { position: eye, forward: to_render(forward), up: to_render(right.cross(forward)), right: to_render(right), fov_y: (fov * DEG) as f32, shift_y: 0.0, near: 0.25 }
}

/// A smooth curve through `values` at `times`: a cubic between each two keys whose slope at a key is
/// that from the key before to the one after; straight on (at the end slopes) outside them.
fn curve(times: &[f64], values: &[Vec<f64>], t: f64) -> Vec<f64> {
    let n = times.len();
    if n == 1 {
        return values[0].clone();
    }
    let slope = |i: usize, j: usize| -> Vec<f64> { values[j].iter().zip(&values[i]).map(|(b, a)| (b - a) / (times[j] - times[i])).collect() };
    // Inside, the slope from the key before to the one after; flat where the curve turns back or
    // stands still on either side (a camera holding, then moving on, does not swing past).
    let tangent = |i: usize| -> Vec<f64> {
        if i == 0 {
            return slope(0, 1);
        }
        if i == n - 1 {
            return slope(n - 2, n - 1);
        }
        let (before, after, across) = (slope(i - 1, i), slope(i, i + 1), slope(i - 1, i + 1));
        (0..across.len()).map(|k| if before[k] * after[k] <= 0.0 { 0.0 } else { across[k] }).collect()
    };
    if t <= times[0] {
        return values[0].iter().zip(tangent(0)).map(|(v, m)| v + m * (t - times[0])).collect();
    }
    if t >= times[n - 1] {
        return values[n - 1].iter().zip(tangent(n - 1)).map(|(v, m)| v + m * (t - times[n - 1])).collect();
    }
    let i = times.windows(2).position(|w| t < w[1]).unwrap_or(n - 2);
    let h = times[i + 1] - times[i];
    let u = (t - times[i]) / h;
    let (m0, m1) = (tangent(i), tangent(i + 1));
    let (h00, h10, h01, h11) = (2.0 * u * u * u - 3.0 * u * u + 1.0, u * u * u - 2.0 * u * u + u, -2.0 * u * u * u + 3.0 * u * u, u * u * u - u * u);
    (0..values[i].len()).map(|k| h00 * values[i][k] + h10 * h * m0[k] + h01 * values[i + 1][k] + h11 * h * m1[k]).collect()
}

/// Samples a second of a shot's sound.
const SOUND_RATE: u32 = 48_000;
/// The speed of sound, m/s.
const SOUND_SPEED: f64 = 343.0;

/// A shot's sound (the script's `sound: true`): what its camera hears, made a frame at a time with
/// the pictures. Every craft of the world (a race's rivals too) sounds of its own rotors or engine
/// (`audio::Params::of`, the game's synthesiser), fainter the farther it is from the camera, to
/// the side it is on, and higher or lower as it comes or goes (Doppler); the air rushes by as fast
/// as the camera moves; the race's chimes are the player's. In slow motion the notes fall with
/// the world's rate (`audio::Mixer`).
struct Sound {
    mixer: Mixer,
    /// Where the camera was at the last frame, and the world's seconds since.
    eye: Option<V3>,
    since: f64,
    velocity: V3,
    /// Frames made so far, for each to have its share of the samples.
    frames: usize,
}

impl Sound {
    fn new() -> Sound {
        Sound { mixer: Mixer::new(SOUND_RATE), eye: None, since: 0.0, velocity: V3::ZERO, frames: 0 }
    }

    /// The world went on `dt` seconds.
    fn passed(&mut self, dt: f64) {
        self.since += dt;
    }

    /// The sound of the frame seen from `view`, the world running at `rate`; `riding`: the camera
    /// goes with the aircraft (it has its speed before a second frame shows its own).
    fn frame(&mut self, flight: &Flight, view: &ViewPoint, riding: bool, rate: f64, fps: f64) {
        let session = &flight.session;
        let eye = view.position;
        self.velocity = match self.eye {
            Some(last) if self.since > 1e-6 => (eye - last) / self.since,
            Some(_) => self.velocity,
            None if riding => session.body().world_velocity(),
            None => V3::ZERO,
        };
        self.eye = Some(eye);
        self.since = 0.0;
        // The picture's right in world axes (render: x east, y up, z south).
        let right = V3::new(-view.right.z as f64, view.right.x as f64, -view.right.y as f64);
        let heard: Vec<Heard> = session
            .craft_ids()
            .into_iter()
            .map(|id| {
                let vehicle = session.world.vehicle(id);
                let body = vehicle.body();
                let to = body.pos - eye;
                let distance = to.length();
                let towards = if distance > 1e-3 { to / distance } else { V3::ZERO };
                // How fast it goes away from the camera.
                let away = (body.world_velocity() - self.velocity).dot(towards);
                Heard { params: Params::of(vehicle, false, 0.0, false), distance: distance as f32, pan: towards.dot(right) as f32, pitch: (SOUND_SPEED / (SOUND_SPEED + away)) as f32 }
            })
            .collect();
        self.frames += 1;
        let count = (self.frames as f64 * SOUND_RATE as f64 / fps).round() as usize - self.mixer.samples.len() / 2;
        self.mixer.frame(&heard, self.velocity.length() as f32, &flight.sound(), rate as f32, count);
    }
}

/// Frames drawn with the world standing still before a shot, once its ground has loaded: for the
/// exposure to settle and the clouds' average over the frames to fill.
const SETTLE_FRAMES: usize = 150;
/// Frames a shot waits for its ground before going on without it.
const LOAD_CAP: usize = 3000;

/// Renders the script's shots (or those named in `only`) to `directory`.
pub fn run(directory: &str, script: &str, only: &[String], storyboard: bool) -> Result<(), String> {
    let text = std::fs::read_to_string(script).map_err(|e| format!("{script}: {e}"))?;
    let script: Script = ron::from_str(&text).map_err(|e| format!("{script}: {e}"))?;
    if let Some(name) = only.iter().find(|name| !script.shots.iter().any(|s| &s.name == *name)) {
        return Err(format!("--only: no shot {name}"));
    }
    std::fs::create_dir_all(directory).map_err(|e| format!("{directory}: {e}"))?;
    let ss = script.supersample.max(1);
    let mut screen = Offscreen::new(script.width * ss, script.height * ss, ss as f32);
    for shot in script.shots.iter().filter(|s| only.is_empty() || only.contains(&s.name)) {
        let started = Instant::now();
        let frames = record_shot(&mut screen, &script, shot, directory, storyboard)?;
        println!("{}: {frames} frames in {:.0} s", shot.name, started.elapsed().as_secs_f64());
    }
    Ok(())
}

/// Renders one shot; returns how many frames it wrote.
fn record_shot(screen: &mut Offscreen, script: &Script, shot: &Shot, directory: &str, storyboard: bool) -> Result<usize, String> {
    let Some(Command { mut options, .. }) = crate::parse(shot.args.split_whitespace().map(str::to_string), false) else {
        return Err(format!("{}: its arguments are not a shot's", shot.name));
    };
    options.quality = options.quality.or(Some(Preset::Ultra));
    // Saved settings and sound stay out, as in a screenshot.
    let mut flight = Flight::new(&screen.gpu, &options, Store::memory(), Audio::silent());
    flight.clean = true;
    flight.clean_osd = shot.osd;
    let session = &mut flight.session;
    if shot.racer {
        let (world, player) = (&session.world, session.player);
        match &mut session.race {
            Some(race) => race.toggle_racer(world, player),
            None => return Err(format!("{}: racer needs --course in its args", shot.name)),
        }
    }
    match shot.start {
        Some(Begin::Over { n, e, heading, agl }) => {
            session.restart_over(n, e, heading * DEG);
            session.set_height(agl);
        }
        Some(Begin::Runway { back, agl }) => session.restart_over_runway(back, agl),
        None => {}
    }
    session.cockpit = shot.camera.first().is_some_and(|k| k.kind() == Kind::Cockpit);
    // Two seconds for the G1000 to see the aircraft, the GFC 700 worked or the hold engaged, then
    // the time before the shot flown with them.
    let step = 1.0 / script.fps;
    let held = Held::default();
    shot.held(0.0)?;
    if !shot.afcs.is_empty() {
        for _ in 0..(2.0 / step) as usize {
            session.advance(step, &held);
        }
        for word in &shot.afcs {
            session.afcs_command(word)?;
        }
    }
    if shot.hold {
        session.engage_hold();
    }
    let before = (shot.before / step).round() as usize;
    for n in 0..before {
        session.advance(step, &shot.held((n as f64 - before as f64) * step)?);
    }
    if let Some((agl, pitch, knots)) = shot.dive {
        flight.session.autopilot = None;
        flight.session.dive(agl, pitch, knots);
    }
    let mut director = Director::new(&shot.camera, &flight)?;
    // The hand on the panel: an aircraft's cockpit's.
    let controls = flight.session.aircraft_kind().map_or(&[][..], |kind| &kinds::cockpit(kind).controls[..]);
    if !shot.hands.is_empty() && controls.is_empty() {
        return Err("a hand on the panel needs an aircraft with a cockpit".to_string());
    }
    let mut hands = Hands::new(&shot.hands, controls)?;
    let screen_rect = engine::egui::Rect::from_min_size(engine::egui::Pos2::ZERO, engine::egui::vec2(script.width as f32, script.height as f32));
    // The world's time for so much of the shot's.
    let rate = |t: f64| shot.rate(t);
    let lens = shot.lens.or(script.lens);
    let aspect = script.width as f32 / script.height as f32;
    // What the picture is taken with at `t`: the camera's view and its lens.
    let look = |flight: &mut Flight, director: &Director, t: f64| -> ViewPoint {
        let view = director.view(flight, t);
        let session = &flight.session;
        let subject = session.player_wreck().map_or(session.body().pos, |w| w.main().pos);
        flight.lens = lens.map(|l| l.of(&view, aspect, session.world.hour, subject));
        flight.session.view_override = Some(view);
        view
    };
    // A still: the shot flown on to it.
    let t0 = if storyboard { shot.still.unwrap_or(shot.seconds / 2.0) } else { 0.0 };
    // The keys of the GFC 700 worked during the shot, in order of time; those before `t0` first.
    let mut keys: Vec<&(f64, String)> = shot.during.iter().collect();
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut keys = keys.into_iter().peekable();
    let mut work = |flight: &mut Flight, t: f64| -> Result<(), String> {
        while let Some((_, word)) = keys.next_if(|(at, _)| *at <= t) {
            flight.session.afcs_command(word)?;
        }
        Ok(())
    };
    for n in 0..(t0 / step).round() as usize {
        let t = n as f64 * step;
        work(&mut flight, t)?;
        hands.act(&mut flight, t);
        flight.session.advance(step * rate(t), &shot.held(t)?);
        let led = flight.session.lead();
        director.follow(&flight, t, step * rate(t));
        flight.session.settle(led);
    }

    if storyboard {
        // Where the still is taken, to place a standing camera (`At`) by what it shows.
        let body = flight.session.body();
        let velocity = body.world_velocity();
        let agl = -body.pos.z - flight.session.world.terrain.height(body.pos.x, body.pos.y).max(0.0);
        println!("{}: at {t0:.2} s the craft is at n {:.1} e {:.1}, {agl:.1} m above the ground, going {:.0} m/s towards {:.0}", shot.name, body.pos.x, body.pos.y, velocity.length(), velocity.y.atan2(velocity.x).rem_euclid(std::f64::consts::TAU) / DEG);
    }
    // The ground loaded and the picture settled, the world standing still.
    flight.session.frozen = true;
    let mut loaded = None;
    for frame in 0.. {
        screen.gpu.set_samples(flight.samples());
        flight.background(&screen.gpu);
        let led = flight.session.lead();
        let view = look(&mut flight, &director, t0);
        flight.pointer = hands.pointer(&flight, &view, screen_rect, t0);
        flight.prepare(&screen.gpu, step);
        screen.draw(&mut flight, step);
        flight.session.settle(led);
        screen.gpu.wait();
        if loaded.is_none() && (flight.pending() == 0 || frame >= LOAD_CAP) {
            loaded = Some(frame);
        }
        if loaded.is_some_and(|at| frame - at >= SETTLE_FRAMES) {
            break;
        }
    }
    flight.session.frozen = false;

    let out = if storyboard { Path::new(directory).to_path_buf() } else { Path::new(directory).join(&shot.name) };
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let count = if storyboard { 1 } else { (shot.seconds * script.fps).round() as usize };
    let (w, h, ss) = (script.width as usize, script.height as usize, script.supersample.max(1) as usize);
    let subframes = script.subframes.max(1) as usize;
    let linear: Vec<f32> = (0..256).map(|i| srgb_to_linear(i as f32 / 255.0)).collect();
    let mut sum = vec![0f32; w * h * 3];
    let mut last = t0;
    let mut timing = Instant::now();
    let mut sound = (script.sound && !storyboard).then(Sound::new);
    let riding = !matches!(director.kind, Kind::Fixed | Kind::At);
    for i in 0..count {
        sum.iter_mut().for_each(|v| *v = 0.0);
        for k in 0..subframes {
            let t = t0 + i as f64 / script.fps + k as f64 * script.shutter / (subframes as f64 * script.fps);
            let dt = t - last;
            last = t;
            work(&mut flight, t)?;
            hands.act(&mut flight, t);
            if dt > 0.0 {
                flight.background(&screen.gpu);
                flight.session.advance(dt * rate(t), &shot.held(t)?);
            }
            // Drawn (and heard) where the craft are now, between the simulation's steps.
            let led = flight.session.lead();
            if dt > 0.0 {
                director.follow(&flight, t, dt * rate(t));
            }
            screen.gpu.set_samples(flight.samples());
            let view = look(&mut flight, &director, t);
            flight.pointer = hands.pointer(&flight, &view, screen_rect, t);
            flight.prepare(&screen.gpu, dt);
            if let Some(sound) = &mut sound {
                sound.passed(dt * rate(t));
                if k == 0 {
                    sound.frame(&flight, &view, riding, rate(t), script.fps);
                }
            }
            screen.draw(&mut flight, dt);
            flight.session.settle(led);
            let pixels = screen.gpu.read_pixels();
            add_down(&mut sum, &pixels, w, h, ss, &linear);
        }
        let scale = 1.0 / (subframes * ss * ss) as f32;
        let picture = to_srgb(&sum, scale, i as u32);
        let path = if storyboard { out.join(format!("{}.png", shot.name)) } else { out.join(format!("{i:05}.png")) };
        image::save_buffer(&path, &picture, w as u32, h as u32, image::ColorType::Rgb8).map_err(|e| format!("{}: {e}", path.display()))?;
        if timing.elapsed().as_secs_f64() > 10.0 || i + 1 == count {
            println!("{}: frame {}/{count}", shot.name, i + 1);
            timing = Instant::now();
        }
    }
    if let Some(sound) = sound {
        let path = Path::new(directory).join(format!("{}.wav", shot.name));
        std::fs::write(&path, sound.mixer.wav()).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(count)
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Adds a picture `ss` times larger each way (RGBA, sRGB) to `sum` (RGB, linear), each block of
/// `ss` x `ss` of its pixels to one of `sum`'s.
fn add_down(sum: &mut [f32], pixels: &[u8], w: usize, h: usize, ss: usize, linear: &[f32]) {
    let big_w = w * ss;
    for y in 0..h {
        for sy in 0..ss {
            let row = &pixels[(y * ss + sy) * big_w * 4..][..big_w * 4];
            let out = &mut sum[y * w * 3..][..w * 3];
            for x in 0..w {
                for sx in 0..ss {
                    let p = &row[(x * ss + sx) * 4..][..3];
                    out[x * 3] += linear[p[0] as usize];
                    out[x * 3 + 1] += linear[p[1] as usize];
                    out[x * 3 + 2] += linear[p[2] as usize];
                }
            }
        }
    }
}

/// `sum` times `scale`, back to sRGB bytes, with a dither of less than a step against banding in
/// the sky's gradients (a different one every frame, `seed`).
fn to_srgb(sum: &[f32], scale: f32, seed: u32) -> Vec<u8> {
    sum.iter()
        .enumerate()
        .map(|(i, v)| {
            let mut x = (i as u32).wrapping_mul(747_796_405).wrapping_add(seed.wrapping_mul(2_891_336_453));
            x ^= x >> 16;
            x = x.wrapping_mul(0x7feb_352d);
            x ^= x >> 15;
            let noise = (x & 0xffff) as f32 / 65535.0 - 0.5;
            (linear_to_srgb(v * scale) * 255.0 + 0.5 + noise).clamp(0.0, 255.0) as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_goes_through_its_keys_and_straight_on_outside_them() {
        let times = [0.0, 1.0, 3.0];
        let values = vec![vec![0.0], vec![10.0], vec![10.0]];
        for (t, v) in times.iter().zip(&values) {
            assert!((curve(&times, &values, *t)[0] - v[0]).abs() < 1e-9);
        }
        // Two keys: a straight line, and on beyond them.
        let two = vec![vec![0.0], vec![4.0]];
        assert!((curve(&[0.0, 2.0], &two, 0.5)[0] - 1.0).abs() < 1e-9);
        assert!((curve(&[0.0, 2.0], &two, 3.0)[0] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn linear_light_and_back() {
        for i in 0..=255 {
            let c = i as f32 / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(c)) - c).abs() < 1e-4);
        }
    }

    #[test]
    fn the_example_scripts_read() {
        for text in [include_str!("../../../../scripts/video/showcase.ron"), include_str!("../../../../scripts/video/course.ron"), include_str!("../../../../scripts/video/deepwood.ron")] {
            let script: Script = ron::from_str(text).unwrap();
            assert!(!script.shots.is_empty());
        }
    }

    #[test]
    fn slow_motion_eases_from_each_rate_to_the_next() {
        let shot: Shot = ron::from_str(r#"Shot(name: "s", seconds: 3.0, camera: [], speed: [(1.0, 1.0), (1.5, 0.25), (2.0, 0.25), (2.5, 1.0)])"#).unwrap();
        for (t, rate) in [(0.0, 1.0), (1.0, 1.0), (1.25, 0.625), (1.5, 0.25), (1.8, 0.25), (2.25, 0.625), (2.9, 1.0)] {
            assert!((shot.rate(t) - rate).abs() < 1e-9, "{t}: {}", shot.rate(t));
        }
        // Never faster than the nearer of its two rates, nor slower than the slower.
        assert!((1..50).all(|i| (0.25..=1.0).contains(&shot.rate(1.0 + i as f64 * 0.03))));
        // None given: the world's own time; `fast` multiplies it from its second on.
        let plain: Shot = ron::from_str(r#"Shot(name: "s", seconds: 3.0, camera: [], fast: Some((1.0, 3.0)))"#).unwrap();
        assert_eq!((plain.rate(0.5), plain.rate(1.5)), (1.0, 3.0));
    }

    #[test]
    fn a_lens_focuses_on_the_subject_and_finds_the_sun() {
        let lens: Lens = ron::from_str("Lens(blur: 0.01, flare: 0.9, vignette: 0.2)").unwrap();
        // Looking north and level from the origin, at noon and at night.
        let view = ViewPoint { position: V3::ZERO, forward: to_render(V3::X), up: to_render(V3::NEG_Z), right: to_render(V3::Y), fov_y: 0.6, shift_y: 0.0, near: 0.25 };
        let made = lens.of(&view, 16.0 / 9.0, 12.0, V3::new(30.0, 0.0, 0.0));
        assert!((made.focus - 30.0).abs() < 1e-4 && (made.focus_span - 2.2).abs() < 1e-4 && made.focus_blur == 0.01 && made.saturation == 1.0 && made.bloom.is_none());
        // The sun is in the south at noon: behind this camera, no flare; before one looking south.
        assert_eq!(made.flare, 0.0);
        let south = ViewPoint { forward: to_render(V3::NEG_X), right: to_render(V3::NEG_Y), ..view };
        let made = lens.of(&south, 16.0 / 9.0, 12.0, V3::new(-30.0, 0.0, 0.0));
        assert!(made.flare > 0.5 && (made.flare_sun[0] - 0.5).abs() < 0.2 && made.flare_sun[1] < 0.5, "{made:?}");
        assert_eq!(lens.of(&south, 16.0 / 9.0, 0.0, V3::ZERO).flare, 0.0);
        // No blur asked for: no depth of field (and its cost) at all.
        let sharp: Lens = ron::from_str("Lens(warmth: 0.2)").unwrap();
        assert_eq!(sharp.of(&view, 1.0, 12.0, V3::X).focus, 0.0);
    }
}
