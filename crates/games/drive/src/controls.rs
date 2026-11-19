//! The player's controls: the keyboard, gamepads and joysticks to the car's steering, throttle,
//! brake, clutch, hand brake and shifts, through a mapping the player can change in a text file
//! (`$XDG_CONFIG_HOME/skylab-drive/controls`, or `~/.config/skylab-drive/controls`).
//!
//! Each device has a section, named by the device's name, written with its defaults the first
//! time it is seen:
//!
//! ```text
//! [T.16000M]
//! kind = stick            # gamepad, stick (a flight stick) or wheel (a steering wheel: as it is turned)
//! steer = axis0           # axes by gamepad name (LeftStickX) or by number (axis0...), "-" turns one over
//! pedal = -axis1          # forward the throttle, back the brake (a stick's); or:
//! throttle = RightTrigger2
//! brake = LeftTrigger2
//! shift_up = button1      # buttons by gamepad name (South...) or by number (button1...)
//! shift_down = button2
//! handbrake = button3
//! camera = button4
//! ```
//!
//! The keyboard: ↑ or W throttle, ↓ or S brake, ← → or A D steer (a key steers more slowly the faster
//! the car goes, as a driver's hands do), E and Q shift up and down, Space the hand brake.

use std::path::PathBuf;

use engine::{pad_axis_named, pad_button_named, Controller, Input, KeyCode, PadAxis, PadButton};
use sim_core::{Car, CarControls};

/// What a device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gamepad,
    Stick,
    /// A steering wheel: the front wheels turn as it is turned, nothing shaped, cut or smoothed.
    Wheel,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Gamepad => "gamepad",
            Kind::Stick => "stick",
            Kind::Wheel => "wheel",
        }
    }
}

/// An axis: by gamepad name or by device number, perhaps turned over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    named: Option<PadAxis>,
    number: u32,
    inverted: bool,
}

impl Axis {
    fn parse(text: &str) -> Option<Axis> {
        let (inverted, name) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        if let Some(n) = name.strip_prefix("axis") {
            return n.parse().ok().map(|number| Axis { named: None, number, inverted });
        }
        pad_axis_named(name).map(|a| Axis { named: Some(a), number: 0, inverted })
    }

    fn text(&self) -> String {
        let sign = if self.inverted { "-" } else { "" };
        match self.named {
            Some(a) => format!("{sign}{a:?}"),
            None => format!("{sign}axis{}", self.number),
        }
    }

    /// −1 to 1 (a trigger 0 to 1), if the device has reported it.
    fn value(&self, c: &Controller) -> Option<f64> {
        let v = match self.named {
            Some(a) => c.named_axis(a),
            None => c.axis_raw(self.number),
        }? as f64;
        Some(if self.inverted { -v } else { v })
    }
}

/// A button: by gamepad name or by number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    named: Option<PadButton>,
    number: u32,
}

impl Button {
    fn parse(text: &str) -> Option<Button> {
        if let Some(n) = text.strip_prefix("button") {
            return n.parse().ok().map(|number| Button { named: None, number });
        }
        pad_button_named(text).map(|b| Button { named: Some(b), number: 0 })
    }

    fn text(&self) -> String {
        match self.named {
            Some(b) => format!("{b:?}"),
            None => format!("button{}", self.number),
        }
    }

    fn down(&self, c: &Controller) -> bool {
        match self.named {
            Some(b) => c.is_down(b),
            None => c.button_raw(self.number).is_some_and(|s| s.down),
        }
    }

    fn pressed(&self, c: &Controller) -> bool {
        match self.named {
            Some(b) => c.pressed(b),
            None => c.button_raw(self.number).is_some_and(|s| s.pressed),
        }
    }

    /// 0 to 1: a trigger's pull, a button's 0 or 1.
    fn value(&self, c: &Controller) -> f64 {
        match self.named {
            Some(b) => c.button_value(b) as f64,
            None => c.button_raw(self.number).map_or(0.0, |s| if s.down { s.value.max(1.0) as f64 } else { 0.0 }),
        }
    }
}

/// What one device's axes and buttons do.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    pub kind: Kind,
    steer: Option<Axis>,
    pedal: Option<Axis>,
    throttle: Option<Button>,
    brake: Option<Button>,
    clutch: Option<Button>,
    shift_up: Option<Button>,
    shift_down: Option<Button>,
    handbrake: Option<Button>,
    camera: Option<Button>,
    restart: Option<Button>,
}

impl Mapping {
    /// What a device does unless its section says otherwise: a gamepad by its names, anything else
    /// as a flight stick (the T.16000M: its X axis steers, forward and back are the pedals, the
    /// trigger shifts up and the thumb button down).
    pub fn default_for(standard: bool) -> Mapping {
        if standard {
            Mapping {
                kind: Kind::Gamepad,
                steer: Axis::parse("LeftStickX"),
                pedal: None,
                throttle: Button::parse("RightTrigger2"),
                brake: Button::parse("LeftTrigger2"),
                clutch: None,
                shift_up: Button::parse("RightTrigger"),
                shift_down: Button::parse("LeftTrigger"),
                handbrake: Button::parse("South"),
                camera: Button::parse("North"),
                restart: Button::parse("Select"),
            }
        } else {
            Mapping {
                kind: Kind::Stick,
                steer: Axis::parse("axis0"),
                pedal: Axis::parse("-axis1"),
                throttle: None,
                brake: None,
                clutch: None,
                shift_up: Button::parse("button1"),
                shift_down: Button::parse("button2"),
                handbrake: Button::parse("button3"),
                camera: Button::parse("button4"),
                restart: None,
            }
        }
    }

    /// The section of the file for a device.
    fn text(&self, name: &str) -> String {
        let mut out = format!("[{name}]\nkind = {}\n", self.kind.name());
        let mut line = |key: &str, value: Option<String>| {
            if let Some(v) = value {
                out.push_str(&format!("{key} = {v}\n"));
            }
        };
        line("steer", self.steer.map(|a| a.text()));
        line("pedal", self.pedal.map(|a| a.text()));
        line("throttle", self.throttle.map(|b| b.text()));
        line("brake", self.brake.map(|b| b.text()));
        line("clutch", self.clutch.map(|b| b.text()));
        line("shift_up", self.shift_up.map(|b| b.text()));
        line("shift_down", self.shift_down.map(|b| b.text()));
        line("handbrake", self.handbrake.map(|b| b.text()));
        line("camera", self.camera.map(|b| b.text()));
        line("restart", self.restart.map(|b| b.text()));
        out
    }

    /// A section's lines over the defaults of its kind.
    fn parse(lines: &[&str], standard: bool) -> Mapping {
        let value = |key: &str| lines.iter().find_map(|l| {
            let l = l.split('#').next()?.trim();
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        });
        let kind = match value("kind").as_deref() {
            Some("gamepad") => Kind::Gamepad,
            Some("stick") => Kind::Stick,
            Some("wheel") => Kind::Wheel,
            _ if standard => Kind::Gamepad,
            _ => Kind::Stick,
        };
        let mut m = Mapping::default_for(kind == Kind::Gamepad);
        m.kind = kind;
        let axis = |key: &str, default: Option<Axis>| value(key).map_or(default, |v| Axis::parse(&v));
        let button = |key: &str, default: Option<Button>| value(key).map_or(default, |v| Button::parse(&v));
        m.steer = axis("steer", m.steer);
        m.pedal = axis("pedal", m.pedal);
        m.throttle = button("throttle", m.throttle);
        m.brake = button("brake", m.brake);
        m.clutch = button("clutch", m.clutch);
        m.shift_up = button("shift_up", m.shift_up);
        m.shift_down = button("shift_down", m.shift_down);
        m.handbrake = button("handbrake", m.handbrake);
        m.camera = button("camera", m.camera);
        m.restart = button("restart", m.restart);
        m
    }
}

/// The file of mappings.
fn path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = std::env::var_os("HOME").filter(|v| !v.is_empty()).map(|h| PathBuf::from(h).join(".config"));
    Some(config.or(home)?.join("skylab-drive").join("controls"))
}

/// What the player asked for this frame besides the car's controls.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Asked {
    pub camera: bool,
    pub restart: bool,
}

/// What a driver's feet feel of the tyres.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Feet {
    /// How much the driven wheels spin (their slip along, the more of the two).
    pub spin: f64,
    /// How near a wheel is to locking (the most slip along of the four, braking).
    pub lock: f64,
    /// How far the rear tyres slide across (their slip angle, rad, the more of the two).
    pub slide: f64,
}

impl Feet {
    pub fn of(car: &Car) -> Feet {
        let o = car.observe();
        let driven: &[usize] = match car.p.drive {
            sim_core::car::Drive::Rear => &[2, 3],
            sim_core::car::Drive::Front => &[0, 1],
            sim_core::car::Drive::All => &[0, 1, 2, 3],
        };
        Feet {
            spin: driven.iter().map(|&i| o.slip[i]).fold(0.0, f64::max),
            lock: o.slip.iter().fold(0.0, |m, s| m.max(-s)),
            slide: if o.speed > 5.0 { o.slip_angle[2].abs().max(o.slip_angle[3].abs()) } else { 0.0 },
        }
    }
}

/// The slip of the driven wheels past which a helped throttle key eases, the slip of a braked
/// wheel past which a helped brake key does, and how fast (of the pedal per unit of slip); the
/// rear tyres' slip angle (rad) past which the throttle eases too, and how fast.
const SPIN: f64 = 0.10;
const LOCK: f64 = 0.11;
const EASE: f64 = 6.0;
const SLIDE: f64 = 0.10;
const EASE_SLIDE: f64 = 7.0;

/// The player's hands and feet on the car.
pub struct Driving {
    /// The pedals' keys as feet that feel the tyres (`keys`); `G` switches it.
    pub pedal_help: bool,
    /// Where the keyboard has the steering and the pedals.
    steer: f64,
    throttle: f64,
    brake: f64,
    /// Where a stick has the steering: its travel shaped, cut with speed and smoothed.
    stick: f64,
    /// How long the pedal has been held the other way with the car stopped (`reverse`).
    held_over: f64,
    /// The mappings of the devices seen, by name.
    mappings: Vec<(String, Mapping)>,
    file: Option<PathBuf>,
    /// Whether to write new devices' sections to the file (not in a screenshot).
    pub write: bool,
}

impl Driving {
    pub fn new() -> Driving {
        Driving { pedal_help: true, steer: 0.0, throttle: 0.0, brake: 0.0, stick: 0.0, held_over: 0.0, mappings: Vec::new(), file: path(), write: true }
    }

    fn mapping(&mut self, c: &Controller) -> Mapping {
        if let Some((_, m)) = self.mappings.iter().find(|(n, _)| *n == c.name) {
            return m.clone();
        }
        let text = self.file.as_ref().and_then(|p| engine::files::load_text(p).ok()).unwrap_or_default();
        let header = format!("[{}]", c.name);
        let lines: Vec<&str> = text.lines().skip_while(|l| l.trim() != header).skip(1).take_while(|l| !l.trim_start().starts_with('[')).collect();
        let found = text.lines().any(|l| l.trim() == header);
        let m = if found { Mapping::parse(&lines, c.standard) } else { Mapping::default_for(c.standard) };
        if !found && self.write {
            if let Some(p) = &self.file {
                let joined = format!("{}{}{}", text, if text.is_empty() || text.ends_with("\n\n") { "" } else { "\n" }, m.text(&c.name));
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Err(e) = engine::files::save_text(p, &joined) {
                    eprintln!("controls: could not write {}: {e}", p.display());
                }
            }
        }
        self.mappings.push((c.name.clone(), m.clone()));
        m
    }

    /// What the keyboard has of the steering and the pedals, `dt` later: `steer` −1 (the left key),
    /// 0 or 1, the pedals' keys held or not. The steering moves towards full lock at a rate and
    /// back to the middle faster; full lock is less the faster the car goes (`speed`, m/s).
    ///
    /// A pedal's key is all or nothing and a foot is not: with `pedal_help` a key held is a foot
    /// that feels the tyres (`Feet`). The throttle goes down as far as the driven tyres grip,
    /// eased while they spin past `SPIN` or the tail slides out; the brake as far as no wheel
    /// locks past `LOCK`, as a driver brakes at the threshold. With traction control and anti-lock
    /// brakes on the tyres never get that far and the keys are as they were.
    pub fn keys(&mut self, steer: f64, throttle: bool, brake: bool, dt: f64, speed: f64, feet: &Feet) -> (f64, f64, f64) {
        let reach = 1.0 / (1.0 + (speed / 22.0).powi(2));
        let wanted = steer.clamp(-1.0, 1.0) * reach;
        let rate = if wanted == 0.0 || wanted.signum() != self.steer.signum() { 4.0 } else { 1.6 };
        self.steer += (wanted - self.steer).clamp(-rate * dt, rate * dt);
        let pedal = |held: bool, most: f64, v: &mut f64, up: f64, down: f64| {
            *v = if held && *v <= most { (*v + up * dt).min(most) } else { (*v - down * dt).max(if held { most } else { 0.0 }) };
        };
        let (mut gas, mut stop) = (1.0, 1.0);
        if self.pedal_help {
            gas = (1.0 - (feet.spin - SPIN).max(0.0) * EASE).min(1.0 - (feet.slide - SLIDE).max(0.0) * EASE_SLIDE).clamp(0.0, 1.0);
            stop = (1.0 - (feet.lock - LOCK).max(0.0) * EASE).clamp(0.2, 1.0);
        }
        pedal(throttle, gas, &mut self.throttle, 4.0, 6.0);
        pedal(brake, stop, &mut self.brake, 3.5, 8.0);
        (self.steer, self.throttle, self.brake)
    }

    /// The car's controls from what is held, and what else was asked for. `speed` (m/s) slows
    /// the keyboard's steering.
    /// `speed` is forward (negative backing), `gear` the one engaged (−1 reverse).
    pub fn update(&mut self, input: &Input, dt: f64, speed: f64, gear: i32, feet: &Feet, current: &CarControls) -> (CarControls, Asked) {
        let mut c = *current;
        let mut asked = Asked::default();
        c.shift = 0;
        let key = input.axis(KeyCode::ArrowRight, KeyCode::ArrowLeft) + input.axis(KeyCode::KeyD, KeyCode::KeyA);
        let throttle = input.is_down(KeyCode::ArrowUp) || input.is_down(KeyCode::KeyW);
        let brake = input.is_down(KeyCode::ArrowDown) || input.is_down(KeyCode::KeyS);
        (c.steer, c.throttle, c.brake) = self.keys(key, throttle, brake, dt, speed, feet);
        c.handbrake = if input.is_down(KeyCode::Space) { 1.0 } else { 0.0 };
        c.clutch = 0.0;
        for k in &input.pressed {
            match k {
                KeyCode::KeyE | KeyCode::ShiftRight => c.shift = 1,
                KeyCode::KeyQ | KeyCode::ControlRight => c.shift = -1,
                _ => {}
            }
        }

        // The devices: where they say something, they have it.
        let controllers: Vec<Controller> = input.controllers.clone();
        for dev in &controllers {
            let m = self.mapping(dev);
            if let Some(v) = m.steer.and_then(|a| a.value(dev)) {
                if v.abs() > 0.03 || key == 0.0 {
                    c.steer = self.stick_steer(v, dt, speed, m.kind);
                }
            }
            if let Some(v) = m.pedal.and_then(|a| a.value(dev)) {
                let v = shape(v, 0.05, 0.0);
                if v.abs() > 0.0 || (self.throttle == 0.0 && self.brake == 0.0) {
                    c.throttle = v.max(0.0);
                    c.brake = (-v).max(0.0);
                }
            }
            if let Some(b) = m.throttle {
                let v = b.value(dev);
                if v > 0.02 {
                    c.throttle = v;
                }
            }
            if let Some(b) = m.brake {
                let v = b.value(dev);
                if v > 0.02 {
                    c.brake = v;
                }
            }
            if let Some(b) = m.clutch {
                c.clutch = b.value(dev);
            }
            if m.handbrake.is_some_and(|b| b.down(dev)) {
                c.handbrake = 1.0;
            }
            if m.shift_up.is_some_and(|b| b.pressed(dev)) {
                c.shift = 1;
            }
            if m.shift_down.is_some_and(|b| b.pressed(dev)) {
                c.shift = -1;
            }
            asked.camera |= m.camera.is_some_and(|b| b.pressed(dev));
            asked.restart |= m.restart.is_some_and(|b| b.pressed(dev));
        }
        self.reverse(&mut c, dt, speed, gear);
        (c, asked)
    }

    /// The automatic's reverse, as a driving game's: the car stopped, the brake (the stick back,
    /// `↓`) held on for `OVER_HOLD` seconds puts it in reverse, and in reverse back is the throttle
    /// and forward the brake; stopped again, forward held puts it back in first. The sequential
    /// shifts into reverse with `Q` from first, as the gearbox allows, and its pedals stay as
    /// they are.
    fn reverse(&mut self, c: &mut CarControls, dt: f64, speed: f64, gear: i32) {
        if !c.automatic {
            self.held_over = 0.0;
            return;
        }
        let (on, off) = (c.throttle, c.brake);
        if gear < 0 {
            // Backing: the pedals swapped.
            c.throttle = off;
            c.brake = on;
        }
        // The pedal the other way from the gear: the brake going forward, the throttle backing.
        let other = if gear < 0 { on } else { off };
        let stopped = speed.abs() < STOPPED;
        if stopped && other > 0.5 && c.shift == 0 {
            self.held_over += dt;
            if self.held_over >= OVER_HOLD {
                c.shift = if gear < 0 { 1 } else { -1 };
                self.held_over = 0.0;
            }
        } else {
            self.held_over = 0.0;
        }
    }

    /// A stick's or a pad's steering: a flight stick or a thumbstick has a few centimetres of
    /// travel for a wheel's 900°, so the travel is shaped (a dead zone and a strong expo: the
    /// middle of it fine, the end of it full lock: a quarter of the travel turns the wheels 4 % of
    /// their lock, half of it 16 %), the lock it reaches is cut with speed as the
    /// keyboard's (at 100 km/h about half, as a driver turns the wheel less the faster he goes),
    /// and it follows the stick over `STICK_LAG` seconds, as a wheel turned by hands, not a switch.
    /// A gamepad's thumbstick, shorter still and flicked, is cut harder with speed (`PAD_SPEED`) and
    /// followed more slowly (`PAD_LAG`, `PAD_OUT`, `PAD_BACK`).
    /// A steering wheel (`Kind::Wheel`) turns the front wheels as it is turned.
    fn stick_steer(&mut self, v: f64, dt: f64, speed: f64, kind: Kind) -> f64 {
        if kind == Kind::Wheel {
            return v.clamp(-1.0, 1.0);
        }
        let pad = kind == Kind::Gamepad;
        let expo = if pad { 0.75 } else { 0.85 };
        let reach = 1.0 / (1.0 + (speed / if pad { PAD_SPEED } else { STICK_SPEED }).powi(2));
        let wanted = shape(v, 0.05, expo) * reach;
        let step = (wanted - self.stick) * (dt / if pad { PAD_LAG } else { STICK_LAG }).min(1.0);
        // A thumbstick goes from end to end in a flick: the wheels turn no faster than hands would
        // turn them, and come back towards the middle faster than they go out.
        let rate = if pad { if wanted.abs() < self.stick.abs() || wanted.signum() != self.stick.signum() { PAD_BACK } else { PAD_OUT } } else { f64::INFINITY };
        self.stick += step.clamp(-rate * dt, rate * dt);
        self.stick
    }

    /// Lets go of everything (after a restart).
    pub fn release(&mut self) {
        self.stick = 0.0;
        self.steer = 0.0;
        self.throttle = 0.0;
        self.brake = 0.0;
    }
}

/// Slower than this (m/s) the car counts as stopped, to go into reverse or out of it.
const STOPPED: f64 = 0.5;
/// How long (s) the pedal is held the other way, stopped, to go into reverse or out of it.
const OVER_HOLD: f64 = 0.3;

/// The speed (m/s) at which a stick's full travel turns the wheels half their lock.
const STICK_SPEED: f64 = 27.0;
/// How long (s) the steering takes to follow a stick, as an exponential lag.
const STICK_LAG: f64 = 0.08;
/// A gamepad's thumbstick: its full travel turns the wheels half their lock at this speed (m/s, 72 km/h),
const PAD_SPEED: f64 = 20.0;
/// the steering follows it over this lag (s),
const PAD_LAG: f64 = 0.18;
/// and turns out at most this much of the lock a second, and back towards the middle this much.
const PAD_OUT: f64 = 1.8;
const PAD_BACK: f64 = 3.5;

/// A stick's travel shaped: a dead zone in the middle, then an expo curve (`expo` 0 linear).
pub fn shape(v: f64, dead: f64, expo: f64) -> f64 {
    let a = ((v.abs() - dead) / (1.0 - dead)).clamp(0.0, 1.0);
    v.signum() * (a * (1.0 - expo) + a * a * a * expo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(d: &mut Driving, v: f64, speed: f64, seconds: f64) -> f64 {
        let mut out = 0.0;
        for _ in 0..(seconds * 60.0) as usize {
            out = d.stick_steer(v, 1.0 / 60.0, speed, Kind::Stick);
        }
        out
    }

    /// A flight stick steers finely in the middle of its travel, reaches full lock at its end when
    /// slow and about half at 100 km/h, and the wheels follow it within a fraction of a second.
    #[test]
    fn a_stick_steers_finely_and_less_the_faster() {
        let mut d = Driving::new();
        assert!((held(&mut d, 1.0, 0.0, 1.0) - 1.0).abs() < 1e-3);
        let mut d = Driving::new();
        let half = held(&mut d, 0.5, 0.0, 1.0);
        assert!(half > 0.1 && half < 0.2, "{half}");
        let mut d = Driving::new();
        let quarter = held(&mut d, 0.25, 0.0, 1.0);
        assert!(quarter > 0.01 && quarter < 0.06, "{quarter}");
        let mut d = Driving::new();
        assert!(held(&mut d, 0.03, 0.0, 1.0).abs() < 1e-9, "in the dead zone");
        let mut d = Driving::new();
        let fast = held(&mut d, 1.0, 100.0 / 3.6, 1.0);
        assert!(fast > 0.4 && fast < 0.55, "{fast}");
        let mut d = Driving::new();
        let soon = held(&mut d, 1.0, 0.0, 0.05);
        assert!(soon > 0.3 && soon < 0.8, "it follows the stick, not at once: {soon}");
        let mut d = Driving::new();
        assert_eq!(d.stick_steer(0.5, 1.0 / 60.0, 100.0 / 3.6, Kind::Wheel), 0.5, "a steering wheel as it is turned");
        // A gamepad's thumbstick, flicked to its end: the wheels go out over half a second, and
        // less the faster.
        let pad = |v: f64, speed: f64, seconds: f64| {
            let mut d = Driving::new();
            (0..(seconds * 60.0) as usize).fold(0.0, |_, _| d.stick_steer(v, 1.0 / 60.0, speed, Kind::Gamepad))
        };
        let flick = pad(1.0, 0.0, 0.1);
        assert!(flick > 0.05 && flick < 0.25, "not at once: {flick}");
        assert!((pad(1.0, 0.0, 1.5) - 1.0).abs() < 1e-2);
        let fast = pad(1.0, 100.0 / 3.6, 1.5);
        assert!(fast > 0.25 && fast < 0.4, "{fast}");
        let wheel = Mapping::parse(&["kind = wheel", "steer = axis2"], false);
        assert_eq!(wheel.kind, Kind::Wheel);
        assert!(wheel.text("W").contains("kind = wheel"));
    }
}

#[cfg(test)]
mod reverse_tests {
    use super::*;

    fn auto() -> CarControls {
        CarControls { automatic: true, ..CarControls::default() }
    }

    /// Stopped in first with the brake held, it asks for reverse after a moment; in reverse the
    /// pedals are swapped, and forward held, stopped, asks for first again. Moving, nothing.
    #[test]
    fn back_held_stopped_goes_into_reverse_and_forward_out() {
        let mut d = Driving::new();
        let mut c = auto();
        c.brake = 1.0;
        let mut asked = 0;
        for _ in 0..30 {
            let mut k = c;
            d.reverse(&mut k, 1.0 / 60.0, 0.0, 1);
            asked += k.shift;
        }
        assert_eq!(asked, -1, "reverse asked for once");
        let mut k = c;
        d.reverse(&mut k, 1.0 / 60.0, 0.0, -1);
        assert!(k.throttle == 1.0 && k.brake == 0.0, "back is the throttle in reverse: {k:?}");
        let mut c = auto();
        c.throttle = 1.0;
        let mut asked = 0;
        for _ in 0..30 {
            let mut k = c;
            d.reverse(&mut k, 1.0 / 60.0, 0.0, -1);
            asked += k.shift;
        }
        assert_eq!(asked, 1, "first again");
        let mut d = Driving::new();
        let mut c = auto();
        c.brake = 1.0;
        for _ in 0..60 {
            let mut k = c;
            d.reverse(&mut k, 1.0 / 60.0, 10.0, 1);
            assert_eq!(k.shift, 0, "braking while moving is braking");
        }
    }
}
