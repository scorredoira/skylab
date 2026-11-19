//! Game controllers as flight controls: which axis is the yoke, which button the flaps, how a
//! small stick is shaped, and the text file where the player changes any of it.
//!
//! [`Pads`] is what the game holds. Once per frame `read` looks at the connected controllers and
//! says which one-shot keys their buttons stand for; during the simulation `fly` moves the yoke and
//! pedals through `KeyboardPilot::mouse_yoke` and `mouse_rudder`, and the throttle and trim directly.
//!
//! Four kinds of device:
//! - **gamepad**: its stick is small and springs back, so it pushes the yoke away from where it was
//!   when the stick left the centre, less far the faster the aircraft flies; back in the centre, the
//!   keyboard (and its assist) has the yoke again.
//! - **stick**: a flight stick. The yoke is where the stick is, always, at full travel.
//! - **throttle**: a separate throttle unit; only what its section of the file maps.
//! - **radio**: an RC transmitter plugged in as a joystick (EdgeTX, OpenTX...), its two sticks in
//!   mode 2: throttle and yaw on the left, pitch and roll on the right; its arm switch a button
//!   held down (`arm`). It flies a multirotor as its own radio would, and an aircraft as a stick.
//!
//! A multirotor (`sticks`) takes the same axes as its sticks: roll, pitch (back is nose up), the
//! rudder axis as its yaw and the throttle as its throttle stick, wherever they are, and `arm` as
//! its arm switch; R (on a gamepad its flaps-up button) arms it.

use engine::{pad_axis_named, pad_button_named, Controller, ControllerPort, KeyCode, PadAxis, PadButton};
use crate::cockpit::{Panel, PanelKnob};
use sim_core::{clamp, AfcsKey, AircraftControls, KeyboardPilot, MultirotorControls};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// What a device is, which decides how its stick is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gamepad,
    Stick,
    Throttle,
    Radio,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Gamepad => "gamepad",
            Kind::Stick => "stick",
            Kind::Throttle => "throttle",
            Kind::Radio => "radio",
        }
    }

    fn parse(word: Option<&str>) -> Kind {
        match word {
            Some("stick") => Kind::Stick,
            Some("throttle") => Kind::Throttle,
            Some("radio") => Kind::Radio,
            _ => Kind::Gamepad,
        }
    }

    /// From the name where it says so; otherwise a device gilrs knows as a gamepad is one, and
    /// anything else is taken for a flight stick.
    pub fn of(name: &str, standard: bool) -> Kind {
        let name = name.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        if has(&["edgetx", "opentx", "radiomaster", "frsky", "taranis", "jumper", "tx16", "tx12", "zorro", "boxer", "pocket", "betafpv", "tbs tango", "tango 2", "mambo", "flysky", "rc transmitter", "rc joystick"]) {
            Kind::Radio
        } else if has(&["twcs", "throttle", "quadrant"]) {
            Kind::Throttle
        } else if has(&["16000", "joystick", "stick", "hotas", "flight"]) || !standard {
            Kind::Stick
        } else {
            Kind::Gamepad
        }
    }
}

/// An axis of a controller: by gamepad name or by device number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisSource {
    Named(PadAxis),
    Number(u32),
}

impl AxisSource {
    fn parse(text: &str) -> Option<AxisSource> {
        match text.strip_prefix("axis") {
            Some(number) => number.parse().ok().map(AxisSource::Number),
            None => pad_axis_named(text).map(AxisSource::Named),
        }
    }

    fn text(self) -> String {
        match self {
            AxisSource::Named(name) => format!("{name:?}"),
            AxisSource::Number(number) => format!("axis{number}"),
        }
    }

    /// By name in the gamepad convention (up positive), by number as the device sends it (back positive).
    fn value(self, c: &Controller) -> Option<f64> {
        match self {
            AxisSource::Named(name) => c.named_axis(name),
            AxisSource::Number(number) => c.axis_raw(number),
        }
        .map(f64::from)
    }
}

/// An axis and whether it is turned over: `LeftStickX`, `-LeftStickY`, `axis5`, `-axis6`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisRef {
    pub source: AxisSource,
    pub inverted: bool,
}

impl AxisRef {
    fn parse(text: &str) -> Option<AxisRef> {
        let (inverted, name) = match text.strip_prefix('-') {
            Some(name) => (true, name),
            None => (false, text),
        };
        Some(AxisRef { source: AxisSource::parse(name)?, inverted })
    }

    fn text(self) -> String {
        format!("{}{}", if self.inverted { "-" } else { "" }, self.source.text())
    }

    fn value(self, c: &Controller) -> Option<f64> {
        self.source.value(c).map(|v| if self.inverted { -v } else { v })
    }
}

/// Something that is pressed: a button by gamepad name (`South`) or device number (`button3`), or
/// one direction of an axis (`RightZ+`, `axis17-`), which counts as pressed past half its travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ButtonRef {
    Named(PadButton),
    Number(u32),
    Axis(AxisSource, bool),
}

impl ButtonRef {
    fn parse(text: &str) -> Option<ButtonRef> {
        if let Some(axis) = text.strip_suffix('+') {
            return AxisSource::parse(axis).map(|a| ButtonRef::Axis(a, true));
        }
        if let Some(axis) = text.strip_suffix('-') {
            return AxisSource::parse(axis).map(|a| ButtonRef::Axis(a, false));
        }
        match text.strip_prefix("button") {
            Some(number) => number.parse().ok().filter(|n| *n > 0).map(ButtonRef::Number),
            None => pad_button_named(text).map(ButtonRef::Named),
        }
    }

    fn text(self) -> String {
        match self {
            ButtonRef::Named(name) => format!("{name:?}"),
            ButtonRef::Number(number) => format!("button{number}"),
            ButtonRef::Axis(axis, positive) => format!("{}{}", axis.text(), if positive { "+" } else { "-" }),
        }
    }

    /// How far it is pressed, 0...1: an analogue trigger gives everything in between.
    fn level(self, c: &Controller) -> f64 {
        match self {
            ButtonRef::Named(name) => f64::from(c.button_value(name)),
            ButtonRef::Number(number) => c.button_raw(number).map_or(0.0, |b| f64::from(b.value)),
            ButtonRef::Axis(axis, positive) => axis.value(c).map_or(0.0, |v| if positive { v } else { -v }).max(0.0),
        }
    }

    fn is_down(self, c: &Controller) -> bool {
        match self {
            ButtonRef::Named(name) => c.is_down(name),
            ButtonRef::Number(number) => c.button_raw(number).is_some_and(|b| b.down),
            ButtonRef::Axis(..) => self.level(c) > 0.5,
        }
    }

    /// Went down since the last frame, even if it is up again. An axis never says so.
    fn was_pressed(self, c: &Controller) -> bool {
        match self {
            ButtonRef::Named(name) => c.pressed(name),
            ButtonRef::Number(number) => c.button_raw(number).is_some_and(|b| b.pressed),
            ButtonRef::Axis(..) => false,
        }
    }
}

/// What an axis of a controller moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// The yoke sideways, right positive.
    Roll,
    /// The yoke fore and aft, **back** (nose up) positive.
    Pitch,
    /// The pedals, right positive.
    Rudder,
    /// The throttle lever as a position: the axis at +1 is full throttle.
    Throttle,
    /// The head or the outside camera, right positive.
    Look,
}

impl Role {
    pub const ALL: [Role; 5] = [Role::Roll, Role::Pitch, Role::Rudder, Role::Throttle, Role::Look];

    fn key(self) -> &'static str {
        match self {
            Role::Roll => "roll",
            Role::Pitch => "pitch",
            Role::Rudder => "rudder",
            Role::Throttle => "throttle",
            Role::Look => "look",
        }
    }
}

/// What a button of a controller does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    ThrottleUp,
    ThrottleDown,
    TrimNoseUp,
    TrimNoseDown,
    FlapsDown,
    FlapsUp,
    LookLeft,
    LookRight,
    View,
    /// The cockpit's view zoomed in, or out (the mouse wheel; outside, the camera nearer or further).
    Closer,
    Further,
    /// In the cockpit the eye to the whole panel, or back to the seat from any spot; outside, into the cockpit at the panel.
    PanelView,
    /// The panel's lights a step brighter.
    PanelLights,
    Centre,
    Pause,
    /// The GFC 700's keys and knobs (`Panel`), of an aircraft that has one.
    Autopilot,
    Heading,
    Nav,
    Approach,
    Altitude,
    VerticalSpeed,
    LevelChange,
    NoseUp,
    NoseDown,
    HeadingBugLeft,
    HeadingBugRight,
    DirectTo,
    /// A multirotor's arm switch: armed while it is held (a radio's switch is a button held down).
    Arm,
    /// Time a step slower or faster (`game::TIME_RATES`).
    TimeSlower,
    TimeFaster,
}

impl Action {
    pub const ALL: [Action; 30] = [
        Action::ThrottleUp,
        Action::ThrottleDown,
        Action::TrimNoseUp,
        Action::TrimNoseDown,
        Action::FlapsDown,
        Action::FlapsUp,
        Action::LookLeft,
        Action::LookRight,
        Action::View,
        Action::Closer,
        Action::Further,
        Action::PanelView,
        Action::PanelLights,
        Action::Centre,
        Action::Pause,
        Action::Autopilot,
        Action::Heading,
        Action::Nav,
        Action::Approach,
        Action::Altitude,
        Action::VerticalSpeed,
        Action::LevelChange,
        Action::NoseUp,
        Action::NoseDown,
        Action::HeadingBugLeft,
        Action::HeadingBugRight,
        Action::DirectTo,
        Action::Arm,
        Action::TimeSlower,
        Action::TimeFaster,
    ];

    fn key(self) -> &'static str {
        match self {
            Action::ThrottleUp => "throttle_up",
            Action::ThrottleDown => "throttle_down",
            Action::TrimNoseUp => "trim_nose_up",
            Action::TrimNoseDown => "trim_nose_down",
            Action::FlapsDown => "flaps_down",
            Action::FlapsUp => "flaps_up",
            Action::LookLeft => "look_left",
            Action::LookRight => "look_right",
            Action::View => "view",
            Action::Closer => "closer",
            Action::Further => "further",
            Action::PanelView => "panel",
            Action::PanelLights => "panel_lights",
            Action::Centre => "centre",
            Action::Pause => "pause",
            Action::Autopilot => "autopilot",
            Action::Heading => "heading",
            Action::Nav => "nav",
            Action::Approach => "approach",
            Action::Altitude => "altitude",
            Action::VerticalSpeed => "vertical_speed",
            Action::LevelChange => "level_change",
            Action::NoseUp => "nose_up",
            Action::NoseDown => "nose_down",
            Action::HeadingBugLeft => "heading_bug_left",
            Action::HeadingBugRight => "heading_bug_right",
            Action::DirectTo => "direct_to",
            Action::Arm => "arm",
            Action::TimeSlower => "time_slower",
            Action::TimeFaster => "time_faster",
        }
    }

    /// The key that does the same, for the actions that happen once per press.
    pub(crate) fn key_code(self) -> Option<KeyCode> {
        match self {
            Action::FlapsDown => Some(KeyCode::KeyF),
            Action::FlapsUp => Some(KeyCode::KeyR),
            Action::View => Some(KeyCode::KeyV),
            Action::Closer => Some(KeyCode::Equal),
            Action::Further => Some(KeyCode::Minus),
            Action::PanelView => Some(KeyCode::Space),
            Action::PanelLights => Some(KeyCode::KeyI),
            Action::Centre => Some(KeyCode::KeyC),
            Action::Pause => Some(KeyCode::Escape),
            Action::Autopilot => Some(KeyCode::KeyP),
            Action::Heading => Some(KeyCode::Digit4),
            Action::Nav => Some(KeyCode::Digit5),
            Action::Approach => Some(KeyCode::Digit6),
            Action::Altitude => Some(KeyCode::Digit7),
            Action::VerticalSpeed => Some(KeyCode::Digit8),
            Action::LevelChange => Some(KeyCode::Digit9),
            Action::NoseUp => Some(KeyCode::Period),
            Action::NoseDown => Some(KeyCode::Comma),
            Action::HeadingBugLeft => Some(KeyCode::BracketLeft),
            Action::HeadingBugRight => Some(KeyCode::BracketRight),
            Action::DirectTo => Some(KeyCode::Slash),
            Action::TimeSlower => Some(KeyCode::F9),
            Action::TimeFaster => Some(KeyCode::F10),
            _ => None,
        }
    }
}

/// The keyboard's keys for the G1000 of an aircraft with a GFC 700:
///
/// | Key | Does |
/// |---|---|
/// | P, Shift+P | AP, FD |
/// | 4 5 6 | HDG, NAV, APR |
/// | 7 8 9 | ALT, VS, FLC |
/// | , . | NOSE DN, NOSE UP (the UP/DN wheel) |
/// | [ ] | the heading bug 1° (Shift: 10°) |
/// | \ | the HDG knob pushed: the bug to the heading |
/// | ; ' | the selected altitude 100 ft (Shift: 1000 ft) |
/// | / | D→ the nearest aerodrome |
///
/// (AP DISC is C, which centres the yoke when nothing is engaged; BARO is K, as for every altimeter.)
pub fn panel_key(key: KeyCode, shift: bool) -> Option<Panel> {
    let ten = if shift { 10 } else { 1 };
    let altitude = if shift { PanelKnob::AltitudeCoarse } else { PanelKnob::AltitudeFine };
    Some(match key {
        KeyCode::KeyP if shift => Panel::Key(AfcsKey::Fd),
        KeyCode::KeyP => Panel::Key(AfcsKey::Ap),
        KeyCode::Digit4 => Panel::Key(AfcsKey::Hdg),
        KeyCode::Digit5 => Panel::Key(AfcsKey::Nav),
        KeyCode::Digit6 => Panel::Key(AfcsKey::Apr),
        KeyCode::Digit7 => Panel::Key(AfcsKey::Alt),
        KeyCode::Digit8 => Panel::Key(AfcsKey::Vs),
        KeyCode::Digit9 => Panel::Key(AfcsKey::Flc),
        KeyCode::Period => Panel::Key(AfcsKey::NoseUp),
        KeyCode::Comma => Panel::Key(AfcsKey::NoseDown),
        KeyCode::BracketLeft => Panel::Knob(PanelKnob::Heading, -ten),
        KeyCode::BracketRight => Panel::Knob(PanelKnob::Heading, ten),
        KeyCode::Backslash => Panel::Key(AfcsKey::HeadingSync),
        KeyCode::Semicolon => Panel::Knob(altitude, -1),
        KeyCode::Quote => Panel::Knob(altitude, 1),
        KeyCode::Slash => Panel::DirectTo,
        _ => return None,
    })
}

/// Whether a key of `panel_key` goes on while it is held, as a knob turned on: the knobs and the wheel.
pub fn panel_key_repeats(key: KeyCode) -> bool {
    matches!(panel_key(key, false), Some(Panel::Knob(..) | Panel::Key(AfcsKey::NoseUp | AfcsKey::NoseDown)))
}

/// The axes and the buttons of a kind of device, as they are written in the file.
type Defaults = (&'static [(Role, &'static str)], &'static [(Action, &'static str)]);

/// The version of the defaults (`Mapping::default_for`), written in each section as `defaults=`;
/// a section without it was written before the first version that added something (`added_in`).
const DEFAULTS: u32 = 4;

/// The buttons a version of the defaults added for a kind of device. A section written before it
/// gets them when the file is read (`upgraded`), where it leaves them free.
fn added_in(version: u32, kind: Kind) -> &'static [(Action, &'static str)] {
    match (version, kind) {
        // The T.16000M: the fourth button of the head (right of the hat) to the panel and back,
        // the first two of the base the view zoomed in and out.
        (2, Kind::Stick) => &[(Action::PanelView, "button4"), (Action::Closer, "button5"), (Action::Further, "button6")],
        // The rest of the base's left group: the GFC 700 (AP, HDG, ALT, FLC) and the heading bug.
        (3, Kind::Stick) => &[
            (Action::Autopilot, "button7"),
            (Action::Heading, "button8"),
            (Action::Altitude, "button9"),
            (Action::LevelChange, "button10"),
            (Action::HeadingBugLeft, "button11"),
            (Action::HeadingBugRight, "button12"),
        ],
        // A gamepad's direction pad sideways, free until then: time slower and faster.
        (4, Kind::Gamepad) => &[(Action::TimeSlower, "DPadLeft"), (Action::TimeFaster, "DPadRight")],
        _ => &[],
    }
}

/// Whether a line of the file starts a section.
fn is_section(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('[') && line.ends_with(']')
}

/// A file with what the defaults added since each of its sections was written (`added_in`): a key
/// missing or left empty gets its default if no other key of the section uses those buttons, and the
/// section is marked with this version. The rest, the player's lines and comments, is kept as it is.
/// `None` when every section is up to date.
fn upgraded(text: &str) -> Option<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| is_section(&lines[i])).collect();
    let mut changed = false;
    // From the last section up, so the lines added leave the earlier sections where they were.
    for (k, &start) in starts.iter().enumerate().rev() {
        let end = starts.get(k + 1).copied().unwrap_or(lines.len());
        let keys: Vec<(usize, String, String)> = (start + 1..end)
            .filter(|&i| !lines[i].trim().starts_with('#'))
            .filter_map(|i| lines[i].split_once('=').map(|(key, value)| (i, key.trim().to_string(), value.trim().to_string())))
            .collect();
        let get = |key: &str| keys.iter().rev().find(|(_, k, _)| k == key);
        let version = get("defaults").and_then(|(_, _, v)| v.parse::<u32>().ok()).unwrap_or(1);
        if version >= DEFAULTS {
            continue;
        }
        let kind = Kind::parse(get("kind").map(|(_, _, v)| v.as_str()));
        let mut used: Vec<String> = Action::ALL.iter().filter_map(|a| get(a.key())).flat_map(|(_, _, v)| v.split_whitespace().map(str::to_string)).collect();
        let mut more = Vec::new();
        for &(action, buttons) in (version + 1..=DEFAULTS).flat_map(|v| added_in(v, kind)) {
            let line = get(action.key());
            if line.is_some_and(|(_, _, v)| !v.is_empty()) || buttons.split_whitespace().any(|b| used.iter().any(|u| u == b)) {
                continue;
            }
            used.extend(buttons.split_whitespace().map(str::to_string));
            match line {
                Some((i, key, _)) => lines[*i] = format!("{key}={buttons}"),
                None => more.push(format!("{}={buttons}", action.key())),
            }
        }
        let last = keys.last().map_or(start, |(i, _, _)| *i);
        lines.splice(last + 1..last + 1, more);
        match get("defaults") {
            Some((i, _, _)) => lines[*i] = format!("defaults={DEFAULTS}"),
            None => lines.insert(get("kind").map_or(start, |(i, _, _)| *i) + 1, format!("defaults={DEFAULTS}")),
        }
        changed = true;
    }
    changed.then(|| lines.join("\n") + if text.ends_with('\n') { "\n" } else { "" })
}

/// How one device flies the aircraft: a section of the mapping file.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    pub kind: Kind,
    /// Travel around the centre of the yoke axes that does nothing, 0...1.
    pub dead_zone: f64,
    /// The same for the rudder axis: a twist grip has more play than a stick.
    pub rudder_dead_zone: f64,
    /// 0 is a straight response; towards 1 the centre gets finer and the ends steeper.
    pub expo: f64,
    pub axes: BTreeMap<Role, AxisRef>,
    pub buttons: BTreeMap<Action, Vec<ButtonRef>>,
}

impl Mapping {
    /// A device of this kind with nothing mapped.
    fn empty(kind: Kind) -> Mapping {
        let (dead_zone, rudder_dead_zone, expo) = match kind {
            Kind::Gamepad => (0.08, 0.08, 0.5),
            Kind::Stick | Kind::Throttle => (0.02, 0.06, 0.2),
            // A radio's gimbals are precise, and the flight controller has its own rates.
            Kind::Radio => (0.01, 0.01, 0.0),
        };
        Mapping { kind, dead_zone, rudder_dead_zone, expo, axes: BTreeMap::new(), buttons: BTreeMap::new() }
    }

    /// What a device does before anyone has edited the file.
    pub fn default_for(name: &str, standard: bool) -> Mapping {
        let kind = Kind::of(name, standard);
        let mut m = Mapping::empty(kind);
        let axis = |text: &str| AxisRef::parse(text).expect("a default axis");
        let buttons = |text: &str| text.split_whitespace().map(|b| ButtonRef::parse(b).expect("a default button")).collect::<Vec<_>>();
        let (axes, actions): Defaults = match kind {
            Kind::Gamepad => (
                // The right trigger and the left trigger are buttons with a value on most systems;
                // where they are axes instead (`RightZ+`), the player says so in the file.
                &[(Role::Roll, "LeftStickX"), (Role::Pitch, "-LeftStickY"), (Role::Rudder, "RightStickX")],
                &[
                    (Action::ThrottleUp, "RightTrigger2"),
                    (Action::ThrottleDown, "LeftTrigger2"),
                    // As the stick: down (towards the player) is nose up.
                    (Action::TrimNoseUp, "DPadDown"),
                    (Action::TrimNoseDown, "DPadUp"),
                    (Action::FlapsDown, "South"),
                    (Action::FlapsUp, "East"),
                    (Action::LookLeft, "LeftTrigger"),
                    (Action::LookRight, "RightTrigger"),
                    (Action::View, "North"),
                    (Action::Centre, "West"),
                    (Action::Pause, "Start"),
                ],
            ),
            Kind::Stick => (
                // X, Y, the twist (Rz) and the slider on the base, pushed forward for full throttle.
                // The hat looks sideways and trims fore and aft, as on a real stick.
                &[(Role::Roll, "axis0"), (Role::Pitch, "axis1"), (Role::Rudder, "axis5"), (Role::Throttle, "-axis6"), (Role::Look, "DPadX")],
                &[(Action::TrimNoseUp, "DPadY-"), (Action::TrimNoseDown, "DPadY+"), (Action::View, "button1"), (Action::FlapsDown, "button2"), (Action::FlapsUp, "button3")],
            ),
            Kind::Throttle => (&[(Role::Throttle, "-axis2")], &[]),
            // Mode 2, as EdgeTX's joystick sends its first four channels: roll, pitch, throttle,
            // yaw; the first switch arms, the second works an aircraft's flaps, the third the view.
            Kind::Radio => (
                &[(Role::Roll, "axis0"), (Role::Pitch, "axis1"), (Role::Throttle, "axis2"), (Role::Rudder, "axis3")],
                &[(Action::Arm, "button1"), (Action::FlapsDown, "button2"), (Action::View, "button3")],
            ),
        };
        m.axes = axes.iter().map(|(role, text)| (*role, axis(text))).collect();
        let added = (2..=DEFAULTS).flat_map(|version| added_in(version, kind));
        m.buttons = actions.iter().chain(added).map(|(action, text)| (*action, buttons(text))).collect();
        m
    }

    /// The section of the file for a device with this name: every key, empty where nothing is mapped.
    pub fn text(&self, name: &str) -> String {
        let mut out = format!(
            "[{name}]\nkind={}\ndefaults={DEFAULTS}\ndead_zone={}\nrudder_dead_zone={}\nexpo={}\n",
            self.kind.key(),
            self.dead_zone,
            self.rudder_dead_zone,
            self.expo
        );
        for role in Role::ALL {
            out += &format!("{}={}\n", role.key(), self.axes.get(&role).map(|a| a.text()).unwrap_or_default());
        }
        for action in Action::ALL {
            let buttons: Vec<String> = self.buttons.get(&action).into_iter().flatten().map(|b| b.text()).collect();
            out += &format!("{}={}\n", action.key(), buttons.join(" "));
        }
        out
    }

    /// A section from its `key=value` lines. What is missing or cannot be read is not mapped;
    /// numbers that are missing are the ones of the kind.
    fn from_lines(lines: &[(String, String)]) -> Mapping {
        let get = |key: &str| lines.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let kind = Kind::parse(get("kind"));
        let mut m = Mapping::empty(kind);
        let number = |key: &str, default: f64, max: f64| get(key).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).map_or(default, |v| v.clamp(0.0, max));
        m.dead_zone = number("dead_zone", m.dead_zone, 0.9);
        m.rudder_dead_zone = number("rudder_dead_zone", m.rudder_dead_zone, 0.9);
        m.expo = number("expo", m.expo, 1.0);
        m.axes = Role::ALL.into_iter().filter_map(|role| Some((role, AxisRef::parse(get(role.key())?)?))).collect();
        for action in Action::ALL {
            let buttons: Vec<ButtonRef> = get(action.key()).unwrap_or("").split_whitespace().filter_map(ButtonRef::parse).collect();
            if !buttons.is_empty() {
                m.buttons.insert(action, buttons);
            }
        }
        m
    }

    fn axis(&self, role: Role, c: &Controller) -> Option<f64> {
        self.axes.get(&role)?.value(c)
    }

    /// How far the buttons of an action are pressed, 0...1.
    fn level(&self, action: Action, c: &Controller) -> f64 {
        self.buttons.get(&action).into_iter().flatten().map(|b| b.level(c)).fold(0.0, f64::max).min(1.0)
    }
}

/// An axis value shaped for flying: nothing inside the dead zone, then from 0 to full travel along
/// a curve that is finer near the centre the larger `expo` is. Sign and full travel are kept.
pub fn shape(value: f64, dead_zone: f64, expo: f64) -> f64 {
    let size = value.abs().min(1.0);
    if size <= dead_zone {
        return 0.0;
    }
    let t = (size - dead_zone) / (1.0 - dead_zone);
    value.signum() * ((1.0 - expo) * t + expo * t * t * t)
}

/// How far a gamepad's stick at full travel moves the yoke, sideways and fore and aft, at this
/// indicated airspeed in knots. Control power grows with the square of airspeed, so the travel
/// shrinks by the same factor, as the keyboard assist does. At a 95 knot cruise the whole stick is
/// 0.4 of the ailerons (about 22° of bank per second) and 0.22 of the elevator (about 8° of nose
/// per second, under 2 g); all the ailerons are there below 60 knots and all the elevator below 45.
pub fn gamepad_authority(ias_kt: f64) -> (f64, f64) {
    let ias = ias_kt.max(1.0);
    let power = |full: f64| (full / ias) * (full / ias);
    (clamp(power(60.0), 0.3, 1.0), clamp(power(45.0), 0.15, 1.0))
}

const HEADER: &str = "\
# Flight: what each game controller does. One section per device, named as the device calls itself.
# A section is written with the defaults the first time a device is seen; edit it and start again.
# `flight --controllers` shows the names and numbers a device sends while you move it.
#
# kind              gamepad | stick | throttle | radio (an RC transmitter as a joystick)
# defaults          the version of the defaults the section was written with: a newer one adds its
#                   buttons to it where the section leaves them free
# dead_zone         travel around the centre that does nothing (yoke axes); rudder_dead_zone for the rudder
# expo              0 = straight response, 1 = very fine near the centre
# roll pitch rudder throttle look
#                   an axis: a gamepad name (LeftStickX LeftStickY LeftZ RightStickX RightStickY RightZ
#                   DPadX DPadY; up is positive) or a device number (axis0, axis1...; back is positive).
#                   A leading - turns it over. pitch: positive is nose up. throttle: +1 is full.
#                   A multirotor takes roll, pitch, rudder (its yaw) and throttle as its sticks.
# arm               a multirotor's arm switch: armed while the button is held (a radio's switch)
# the other keys    buttons, separated by spaces: a gamepad name (South East North West LeftTrigger
#                   RightTrigger LeftTrigger2 RightTrigger2 Select Start Mode LeftThumb RightThumb DPadUp
#                   DPadDown DPadLeft DPadRight C Z), a device number (button1...), or an axis pushed one
#                   way (RightZ+, axis17-).
# Leave a value empty for nothing.
";

/// The mapping file: a section per device name.
pub struct Mappings {
    path: Option<PathBuf>,
    devices: Vec<(String, Mapping)>,
}

impl Mappings {
    /// Remembers nothing between runs: for tests.
    pub fn memory() -> Mappings {
        Mappings { path: None, devices: Vec::new() }
    }

    /// Reads the file at `path` if there is one, with what newer defaults added to the sections
    /// written before them (`upgraded`, saved at once); devices seen for the first time are added to it.
    pub fn open(path: PathBuf) -> Mappings {
        let text = engine::files::load_text(&path).ok().map(|text| match upgraded(&text) {
            Some(newer) => {
                if let Err(error) = engine::files::save_text(&path, &newer) {
                    eprintln!("controllers: could not write {}: {error}", path.display());
                }
                newer
            }
            None => text,
        });
        let devices = text.map(|text| Mappings::parse(&text)).unwrap_or_default();
        Mappings { path: Some(path), devices }
    }

    /// The player's file, next to the settings.
    pub fn user() -> Mappings {
        match user_path() {
            Some(path) => Mappings::open(path),
            None => Mappings::memory(),
        }
    }

    /// The sections of a file. Lines starting with `#`, blank lines and lines without `=` are skipped.
    pub fn parse(text: &str) -> Vec<(String, Mapping)> {
        let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.starts_with('#')) {
            if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
                sections.push((name.to_string(), Vec::new()));
            } else if let (Some((key, value)), Some((_, lines))) = (line.split_once('='), sections.last_mut()) {
                lines.push((key.trim().to_string(), value.trim().to_string()));
            }
        }
        sections.into_iter().map(|(name, lines)| (name, Mapping::from_lines(&lines))).collect()
    }

    /// The whole file as it is written when every device is new.
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.devices.iter().fold(HEADER.to_string(), |text, (name, mapping)| text + "\n" + &mapping.text(name))
    }

    /// The mapping of a device; the defaults, added to the file, the first time it is seen.
    pub fn of(&mut self, name: &str, standard: bool) -> &Mapping {
        let at = match self.devices.iter().position(|(n, _)| n == name) {
            Some(at) => at,
            None => {
                let mapping = Mapping::default_for(name, standard);
                self.append(&mapping.text(name));
                self.devices.push((name.to_string(), mapping));
                self.devices.len() - 1
            }
        };
        &self.devices[at].1
    }

    /// Adds a section at the end of the file, leaving what the player wrote as it is.
    fn append(&self, section: &str) {
        let Some(path) = &self.path else { return };
        let before = engine::files::load_text(path).unwrap_or_else(|_| HEADER.to_string());
        let written = engine::files::save_text(path, &format!("{}\n\n{section}", before.trim_end()));
        if let Err(error) = written {
            eprintln!("controllers: could not write {}: {error}", path.display());
        }
    }
}

fn user_path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("controls"))
}

/// Where the controllers want the yoke.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Yoke {
    /// A flight stick: the yoke is here.
    Absolute(f64, f64),
    /// A gamepad: this far from where the yoke was when the stick left the centre, at full authority.
    Offset(f64, f64),
}

/// The controllers in the game: read once per frame, applied in every frame that flies.
pub struct Pads {
    pub mappings: Mappings,
    /// The devices in use, for the status line.
    names: Vec<String>,
    yoke: Option<Yoke>,
    rudder: Option<f64>,
    /// A throttle lever that moved: its new position.
    throttle: Option<f64>,
    throttle_rate: f64,
    trim_rate: f64,
    look: f64,
    /// Where the yoke was when the gamepad's stick left the centre.
    base: Option<(f64, f64)>,
    /// A multirotor's arm switch, where a device maps one: on while it is held.
    arm: Option<bool>,
    /// Where each device's throttle lever was last seen.
    levers: HashMap<usize, f64>,
    held: HashSet<(usize, Action)>,
}

impl Pads {
    pub fn new(mappings: Mappings) -> Pads {
        Pads {
            mappings,
            names: Vec::new(),
            yoke: None,
            rudder: None,
            throttle: None,
            throttle_rate: 0.0,
            trim_rate: 0.0,
            look: 0.0,
            base: None,
            arm: None,
            levers: HashMap::new(),
            held: HashSet::new(),
        }
    }

    /// Looks at the controllers as they are this frame. Returns the keys their buttons stand for:
    /// flaps, view, centre and pause act once per press, exactly as their keys do.
    pub fn read(&mut self, controllers: &[Controller]) -> Vec<KeyCode> {
        self.names = controllers.iter().map(|c| c.name.clone()).collect();
        self.yoke = None;
        self.rudder = None;
        self.throttle = None;
        self.throttle_rate = 0.0;
        self.trim_rate = 0.0;
        self.look = 0.0;
        self.arm = None;
        self.levers.retain(|id, _| controllers.iter().any(|c| c.id == *id));
        self.held.retain(|(id, _)| controllers.iter().any(|c| c.id == *id));
        let mut keys = Vec::new();
        for c in controllers {
            let m = self.mappings.of(&c.name, c.standard).clone();
            let (x, y) = (m.axis(Role::Roll, c), m.axis(Role::Pitch, c));
            let shaped = (shape(x.unwrap_or(0.0), m.dead_zone, m.expo), shape(y.unwrap_or(0.0), m.dead_zone, m.expo));
            let rudder = m.axis(Role::Rudder, c).map(|v| shape(v, m.rudder_dead_zone, m.expo));
            if m.kind == Kind::Gamepad {
                // Only while the stick is out of the centre; a flight stick has the last word.
                if shaped != (0.0, 0.0) && self.yoke.is_none() {
                    self.yoke = Some(Yoke::Offset(shaped.0, shaped.1));
                }
                self.rudder = self.rudder.or(rudder.filter(|r| *r != 0.0));
            } else {
                if (x.is_some() || y.is_some()) && !matches!(self.yoke, Some(Yoke::Absolute(..))) {
                    self.yoke = Some(Yoke::Absolute(shaped.0, shaped.1));
                }
                self.rudder = rudder.or(self.rudder);
            }
            if let Some(v) = m.axis(Role::Throttle, c) {
                // The lever sets the throttle when it moves, so the keys and the panel still can.
                let position = clamp((v + 1.0) / 2.0, 0.0, 1.0);
                if self.levers.get(&c.id).is_none_or(|last| (last - position).abs() > 0.01) {
                    self.levers.insert(c.id, position);
                    self.throttle = Some(position);
                }
            }
            if m.buttons.contains_key(&Action::Arm) {
                self.arm = Some(self.arm.unwrap_or(false) || m.level(Action::Arm, c) > 0.5);
            }
            self.throttle_rate += m.level(Action::ThrottleUp, c) - m.level(Action::ThrottleDown, c);
            self.trim_rate += m.level(Action::TrimNoseUp, c) - m.level(Action::TrimNoseDown, c);
            self.look += m.axis(Role::Look, c).map_or(0.0, |v| shape(v, m.dead_zone, 0.0)) + m.level(Action::LookRight, c) - m.level(Action::LookLeft, c);
            for action in Action::ALL {
                let Some(key) = action.key_code() else { continue };
                let buttons = m.buttons.get(&action).map_or(&[][..], Vec::as_slice);
                let down = buttons.iter().any(|b| b.is_down(c));
                let was_held = if down { !self.held.insert((c.id, action)) } else { self.held.remove(&(c.id, action)) };
                if buttons.iter().any(|b| b.was_pressed(c)) || (down && !was_held) {
                    keys.push(key);
                }
            }
        }
        keys
    }

    /// Sideways look with the controllers added to the keys', -1...1; nothing while paused.
    pub fn look(&self, keys: f64, paused: bool) -> f64 {
        if paused {
            keys
        } else {
            clamp(keys + self.look, -1.0, 1.0)
        }
    }

    /// Moves the aircraft's controls for `dt` seconds of what `read` found. `ias_kt` is the
    /// indicated airspeed in knots; the rates are those of the throttle and trim keys.
    pub fn fly(&mut self, pilot: &mut KeyboardPilot, controls: &mut AircraftControls, ias_kt: f64, dt: f64) {
        pilot.mouse_yoke = match self.yoke {
            Some(Yoke::Absolute(x, y)) => {
                self.base = None;
                Some((x, y))
            }
            Some(Yoke::Offset(x, y)) => {
                let base = *self.base.get_or_insert((pilot.yoke_x(), pilot.yoke_y()));
                let (sideways, fore_aft) = gamepad_authority(ias_kt);
                Some((clamp(base.0 + x * sideways, -1.0, 1.0), clamp(base.1 + y * fore_aft, -1.0, 1.0)))
            }
            // The stick is back in the centre: the yoke goes back to where it was taken from, and
            // from the next frame on the keyboard has it.
            None => self.base.take(),
        };
        pilot.mouse_rudder = self.rudder;
        if let Some(position) = self.throttle.take() {
            controls.throttle = position;
        }
        controls.throttle = clamp(controls.throttle + clamp(self.throttle_rate, -1.0, 1.0) * crate::game::THROTTLE_RATE * dt, 0.0, 1.0);
        controls.trim = clamp(controls.trim + clamp(self.trim_rate, -1.0, 1.0) * crate::game::TRIM_RATE * dt, -1.0, 1.0);
    }

    /// Moves a multirotor's sticks for `dt` seconds of what `read` found, over what the keys did:
    /// any stick out of its centre is where it is (a gamepad's as a radio's, at full travel), the
    /// throttle lever where it was moved, the throttle buttons at the keys' rate; the arm switch
    /// as it is, where a device has one.
    pub fn sticks(&mut self, c: &mut MultirotorControls, dt: f64) {
        match self.yoke {
            Some(Yoke::Absolute(x, y)) | Some(Yoke::Offset(x, y)) => (c.roll, c.pitch) = (x, y),
            None => {}
        }
        self.base = None;
        if let Some(yaw) = self.rudder {
            c.yaw = yaw;
        }
        if let Some(position) = self.throttle.take() {
            c.throttle = position;
        }
        c.throttle = clamp(c.throttle + clamp(self.throttle_rate, -1.0, 1.0) * crate::game::THROTTLE_RATE * dt, 0.0, 1.0);
        // The trim's hat: the pitch a tilt-rotor flown level holds.
        c.trim = clamp(c.trim + clamp(self.trim_rate, -1.0, 1.0) * 0.25 * dt, -1.0, 1.0);
        if let Some(on) = self.arm {
            c.arm = on;
        }
    }

    /// For the end of the status line: the devices in use, or nothing.
    pub fn status(&self) -> String {
        if self.names.is_empty() {
            String::new()
        } else {
            format!(" · Controller: {}", self.names.join(" + "))
        }
    }
}

/// One line with everything a controller has reported: its axes as the device sends them and the
/// buttons that are down.
fn describe(c: &Controller) -> String {
    let mut axes: Vec<_> = c.axes.iter().collect();
    axes.sort_by_key(|a| a.index);
    let axes: Vec<String> = axes
        .iter()
        .map(|a| match a.name {
            Some(name) => format!("axis{}({name:?})={:+.2}", a.index, a.raw()),
            None => format!("axis{}={:+.2}", a.index, a.raw()),
        })
        .collect();
    let buttons: Vec<String> = c
        .buttons
        .iter()
        .filter(|b| b.down || b.value > 0.02)
        .map(|b| {
            let label = match (b.index, b.name) {
                (0, Some(name)) => format!("{name:?}"),
                (index, Some(name)) => format!("button{index}({name:?})"),
                (index, None) => format!("button{index}"),
            };
            if b.value > 0.02 && b.value < 0.98 {
                format!("{label}={:.2}", b.value)
            } else {
                label
            }
        })
        .collect();
    format!("axes: {} | pressed: {}", if axes.is_empty() { "-".to_string() } else { axes.join(" ") }, if buttons.is_empty() { "-".to_string() } else { buttons.join(" ") })
}

/// `--controllers`: prints the connected devices and, for `seconds`, a line each time one of them
/// changes, then everything each device was seen to have. For telling a new device's axes and
/// buttons apart; it writes nothing to the mapping file.
pub fn list_live(seconds: f64) {
    let mut port = ControllerPort::new();
    let mut list: Vec<Controller> = Vec::new();
    let mut known: Vec<usize> = Vec::new();
    let mut shown: HashMap<usize, String> = HashMap::new();
    // Per device and axis number: lowest and highest value seen. Per device: the buttons seen.
    let mut ranges: BTreeMap<(usize, u32), (String, f32, f32)> = BTreeMap::new();
    let mut buttons: BTreeMap<(usize, u32, String), ()> = BTreeMap::new();
    let mut names: BTreeMap<usize, String> = BTreeMap::new();
    println!("Controllers: move each axis from end to end and press each button within {seconds:.0} s.");
    println!("The axes are as the device sends them: stick right and back = positive.");
    if let Some(path) = user_path() {
        println!("Mappings file: {}", path.display());
    }
    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        port.poll(&mut list);
        for c in &list {
            if !known.contains(&c.id) {
                known.push(c.id);
                names.insert(c.id, c.name.clone());
                let kind = Kind::of(&c.name, c.standard);
                let layout = if c.standard { "known gamepad, with names" } else { "no reliable names: by number" };
                println!("[{}] \"{}\" ({layout}) -> treated as {}", c.id, c.name, kind.key());
            }
            for a in &c.axes {
                let label = a.name.map_or(String::new(), |n| format!("({n:?})"));
                let range = ranges.entry((c.id, a.index)).or_insert((label, a.raw(), a.raw()));
                range.1 = range.1.min(a.raw());
                range.2 = range.2.max(a.raw());
            }
            for b in c.buttons.iter().filter(|b| b.down || b.pressed || b.value > 0.02) {
                buttons.insert((c.id, b.index, b.name.map_or(String::new(), |n| format!("{n:?}"))), ());
            }
            let line = describe(c);
            if shown.get(&c.id) != Some(&line) {
                println!("[{}] {line}", c.id);
                shown.insert(c.id, line);
            }
        }
        for id in known.clone() {
            if !list.iter().any(|c| c.id == id) {
                println!("[{id}] disconnected");
                known.retain(|k| *k != id);
                shown.remove(&id);
            }
        }
        list.iter_mut().for_each(Controller::end_frame);
        port.wait(0.05);
    }
    if names.is_empty() {
        println!("No controller connected.");
        return;
    }
    println!("\nSummary:");
    for (id, name) in &names {
        println!("[{id}] \"{name}\"");
        for ((_, index), (label, min, max)) in ranges.iter().filter(|((device, _), _)| device == id) {
            println!("    axis{index}{label}: from {min:+.2} to {max:+.2}");
        }
        let seen: Vec<String> = buttons
            .keys()
            .filter(|(device, _, _)| device == id)
            .map(|(_, index, name)| match (*index, name.is_empty()) {
                (0, _) => name.clone(),
                (index, true) => format!("button{index}"),
                (index, false) => format!("button{index}({name})"),
            })
            .collect();
        println!("    buttons seen: {}", if seen.is_empty() { "-".to_string() } else { seen.join(" ") });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gamepad() -> Controller {
        Controller::new(0, "Test Pad", true)
    }

    fn stick() -> Controller {
        Controller::new(1, "Thrustmaster T.16000M", false)
    }

    #[test]
    fn shaping_has_a_dead_zone_keeps_sign_and_reaches_full_travel() {
        assert_eq!(shape(0.0, 0.08, 0.5), 0.0);
        assert_eq!(shape(0.08, 0.08, 0.5), 0.0);
        assert_eq!(shape(-0.05, 0.08, 0.5), 0.0);
        assert_eq!(shape(1.0, 0.08, 0.5), 1.0);
        assert_eq!(shape(-1.0, 0.08, 0.5), -1.0);
        assert_eq!(shape(1.3, 0.08, 0.5), 1.0);
        // Continuous at the edge of the dead zone, odd, and always rising.
        assert!(shape(0.081, 0.08, 0.5) > 0.0 && shape(0.081, 0.08, 0.5) < 0.001);
        assert_eq!(shape(-0.4, 0.08, 0.5), -shape(0.4, 0.08, 0.5));
        let mut last = 0.0;
        for i in 9..=100 {
            let v = shape(i as f64 / 100.0, 0.08, 0.5);
            assert!(v > last, "{i}");
            last = v;
        }
    }

    #[test]
    fn expo_makes_the_centre_finer() {
        // Without expo the response is a straight line from the dead zone to the end.
        assert!((shape(0.54, 0.08, 0.0) - 0.5).abs() < 1e-12);
        assert!((shape(0.5, 0.0, 0.0) - 0.5).abs() < 1e-12);
        // Half travel with expo 0.5: 0.5 * 0.5 + 0.5 * 0.125.
        assert!((shape(0.5, 0.0, 0.5) - 0.3125).abs() < 1e-12);
        assert!((shape(0.5, 0.0, 1.0) - 0.125).abs() < 1e-12);
        assert!(shape(0.3, 0.08, 0.5) < shape(0.3, 0.08, 0.2));
    }

    #[test]
    fn the_gamepad_has_all_its_travel_slow_and_little_of_it_fast() {
        assert_eq!(gamepad_authority(0.0), (1.0, 1.0));
        assert_eq!(gamepad_authority(45.0), (1.0, 1.0));
        assert!((gamepad_authority(55.0).1 - 0.669).abs() < 0.001 && gamepad_authority(55.0).0 == 1.0);
        let (roll, pitch) = gamepad_authority(95.0);
        assert!((roll - 0.399).abs() < 0.001, "{roll}");
        assert!((pitch - 0.224).abs() < 0.001, "{pitch}");
        assert_eq!(gamepad_authority(160.0), (0.3, 0.15));
    }

    #[test]
    fn a_device_is_a_stick_by_name_or_by_not_being_a_known_gamepad() {
        assert_eq!(Kind::of("Xbox 360 Controller", true), Kind::Gamepad);
        assert_eq!(Kind::of("Pro Controller", true), Kind::Gamepad);
        assert_eq!(Kind::of("Thrustmaster T.16000M", false), Kind::Stick);
        assert_eq!(Kind::of("T.16000M", true), Kind::Stick);
        assert_eq!(Kind::of("Some unknown HID thing", false), Kind::Stick);
        assert_eq!(Kind::of("Thrustmaster TWCS Throttle", false), Kind::Throttle);
    }

    #[test]
    fn sources_are_written_as_they_are_read() {
        for text in ["LeftStickX", "-LeftStickY", "axis5", "-axis6", "DPadX"] {
            assert_eq!(AxisRef::parse(text).expect(text).text(), text);
        }
        for text in ["South", "button3", "RightZ+", "axis17-", "DPadY-", "RightTrigger2"] {
            assert_eq!(ButtonRef::parse(text).expect(text).text(), text);
        }
        for text in ["", "axis", "axisx", "South", "--axis1"] {
            assert_eq!(AxisRef::parse(text), None, "{text}");
        }
        for text in ["", "button0", "button", "LeftStickX", "Unknown"] {
            assert_eq!(ButtonRef::parse(text), None, "{text}");
        }
    }

    #[test]
    fn the_file_reads_back_what_was_written() {
        let pad = Mapping::default_for("Test Pad", true);
        let mut flight_stick = Mapping::default_for("Thrustmaster T.16000M", false);
        flight_stick.dead_zone = 0.035;
        flight_stick.buttons.insert(Action::Pause, vec![ButtonRef::Number(4), ButtonRef::Axis(AxisSource::Number(17), false)]);
        let throttle = Mapping::default_for("TWCS Throttle", false);
        let file = Mappings { path: None, devices: vec![("Test Pad".into(), pad.clone()), ("Thrustmaster T.16000M".into(), flight_stick.clone()), ("TWCS Throttle".into(), throttle.clone())] };
        let text = file.text();
        assert!(text.starts_with("# Flight: what each game controller does."));
        assert!(text.contains("\n[Test Pad]\nkind=gamepad\ndefaults=4\ndead_zone=0.08\nrudder_dead_zone=0.08\nexpo=0.5\nroll=LeftStickX\npitch=-LeftStickY\nrudder=RightStickX\nthrottle=\nlook=\nthrottle_up=RightTrigger2\n"), "{text}");
        assert!(text.contains("\n[Thrustmaster T.16000M]\nkind=stick\ndefaults=4\ndead_zone=0.035\nrudder_dead_zone=0.06\nexpo=0.2\nroll=axis0\npitch=axis1\nrudder=axis5\nthrottle=-axis6\nlook=DPadX\n"), "{text}");
        assert!(text.contains("pause=button4 axis17-\n"));
        let read = Mappings::parse(&text);
        assert_eq!(read, vec![("Test Pad".to_string(), pad), ("Thrustmaster T.16000M".to_string(), flight_stick), ("TWCS Throttle".to_string(), throttle)]);
    }

    #[test]
    fn a_hand_written_section_maps_only_what_it_says() {
        let read = Mappings::parse("junk before any section=1\n# a comment\n[My Pad]\n kind = gamepad \nexpo=7\nroll=-RightStickX\npitch=nonsense\nflaps_down=South button9 what\n\n[Other]\nkind=stick\n");
        assert_eq!(read.len(), 2);
        let (name, m) = &read[0];
        assert_eq!(name, "My Pad");
        assert_eq!((m.kind, m.dead_zone, m.expo), (Kind::Gamepad, 0.08, 1.0));
        assert_eq!(m.axes.len(), 1);
        assert_eq!(m.axes[&Role::Roll], AxisRef { source: AxisSource::Named(PadAxis::RightStickX), inverted: true });
        assert_eq!(m.buttons.len(), 1);
        assert_eq!(m.buttons[&Action::FlapsDown], vec![ButtonRef::Named(PadButton::South), ButtonRef::Number(9)]);
        assert_eq!((read[1].1.kind, read[1].1.dead_zone, read[1].1.axes.len()), (Kind::Stick, 0.02, 0));
    }

    #[test]
    fn a_new_device_is_added_to_the_file_once_and_edits_are_kept() {
        let dir = std::env::temp_dir().join(format!("flight-controls-test-{}", std::process::id()));
        let path = dir.join("controls");
        let _ = std::fs::remove_dir_all(&dir);
        let mut file = Mappings::open(path.clone());
        assert_eq!(file.of("Test Pad", true).kind, Kind::Gamepad);
        file.of("Test Pad", true);
        let text = std::fs::read_to_string(&path).expect("written");
        assert_eq!(text, Mappings { path: None, devices: vec![("Test Pad".into(), Mapping::default_for("Test Pad", true))] }.text());

        // The player's edit and comment survive a second device being added.
        let edited = text.replace("expo=0.5", "# mine\nexpo=0.3");
        std::fs::write(&path, &edited).expect("edited");
        let mut file = Mappings::open(path.clone());
        assert_eq!(file.of("Test Pad", true).expo, 0.3);
        file.of("Thrustmaster T.16000M", false);
        let text = std::fs::read_to_string(&path).expect("written");
        assert!(text.starts_with(&edited) && text.matches("[Test Pad]").count() == 1);
        let read = Mappings::parse(&text);
        assert_eq!((read.len(), read[0].1.expo, read[1].1.kind), (2, 0.3, Kind::Stick));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_section_written_before_new_defaults_gets_them_where_it_leaves_them_free() {
        // A stick's section as the first defaults wrote it, edited by the player: the trigger is
        // pause, button 5 the centre, `further` is left out; a comment; a gamepad after it.
        let old = "# mine\n[Thrustmaster T.16000M]\nkind=stick\nexpo=0.3\nroll=axis0\nview=button2\ncloser=\nflaps_down=\npause=button1\ncentre=button5\n\n[Test Pad]\nkind=gamepad\nview=North\n";
        let new = upgraded(old).expect("upgraded");
        // Its own lines kept; the fourth button to the panel, `closer` taken by button 5 is not
        // given it, `further` added on button 6; the gamepad gets the time on the direction pad.
        assert_eq!(
            new,
            "# mine\n[Thrustmaster T.16000M]\nkind=stick\ndefaults=4\nexpo=0.3\nroll=axis0\nview=button2\ncloser=\nflaps_down=\npause=button1\ncentre=button5\npanel=button4\nfurther=button6\nautopilot=button7\nheading=button8\naltitude=button9\nlevel_change=button10\nheading_bug_left=button11\nheading_bug_right=button12\n\n[Test Pad]\nkind=gamepad\ndefaults=4\nview=North\ntime_slower=DPadLeft\ntime_faster=DPadRight\n"
        );
        assert_eq!(upgraded(&new), None, "once is enough");
        let stick = &Mappings::parse(&new)[0].1;
        assert_eq!((stick.expo, stick.buttons.get(&Action::FlapsDown), stick.buttons.get(&Action::Closer)), (0.3, None, None));
        assert_eq!(stick.buttons[&Action::PanelView], vec![ButtonRef::Number(4)]);
        assert_eq!(stick.buttons[&Action::Further], vec![ButtonRef::Number(6)]);
        // A section the defaults wrote now is up to date, and has them.
        let fresh = Mapping::default_for("Thrustmaster T.16000M", false);
        assert_eq!(upgraded(&fresh.text("Thrustmaster T.16000M")), None);
        assert_eq!(fresh.buttons[&Action::Closer], vec![ButtonRef::Number(5)]);
        assert_eq!(Mappings::parse(&fresh.text("Thrustmaster T.16000M"))[0].1, fresh);
    }

    #[test]
    fn gamepad_sign_conventions() {
        let mut pads = Pads::new(Mappings::memory());
        let mut c = gamepad();
        // Stick right and back: gilrs calls up positive, so back is negative.
        c.set_axis(0, Some(PadAxis::LeftStickX), 1.0);
        c.set_axis(1, Some(PadAxis::LeftStickY), -1.0);
        c.set_axis(3, Some(PadAxis::RightStickX), -1.0);
        assert!(pads.read(std::slice::from_ref(&c)).is_empty());
        assert_eq!(pads.yoke, Some(Yoke::Offset(1.0, 1.0)), "right, and back is nose up");
        assert_eq!(pads.rudder, Some(-1.0));
        assert_eq!(pads.status(), " · Controller: Test Pad");

        // Inside the dead zone the keyboard has the controls.
        c.set_axis(0, Some(PadAxis::LeftStickX), 0.05);
        c.set_axis(1, Some(PadAxis::LeftStickY), -0.05);
        c.set_axis(3, Some(PadAxis::RightStickX), 0.07);
        pads.read(std::slice::from_ref(&c));
        assert_eq!((pads.yoke, pads.rudder), (None, None));
        assert_eq!(Pads::new(Mappings::memory()).status(), "");
    }

    #[test]
    fn gamepad_buttons() {
        let mut pads = Pads::new(Mappings::memory());
        let mut c = gamepad();
        c.set_button(1, Some(PadButton::South), Some(true), None);
        c.set_button(4, Some(PadButton::North), Some(true), None);
        c.set_button(8, Some(PadButton::RightTrigger2), None, Some(0.5));
        c.set_button(5, Some(PadButton::RightTrigger), Some(true), None);
        c.set_axis(17, Some(PadAxis::DPadY), -1.0);
        assert_eq!(pads.read(std::slice::from_ref(&c)), vec![KeyCode::KeyF, KeyCode::KeyV]);
        assert_eq!((pads.throttle_rate, pads.trim_rate, pads.look), (0.5, 1.0, 1.0));
        assert_eq!(pads.look(0.0, false), 1.0);
        assert_eq!(pads.look(-1.0, false), 0.0);
        assert_eq!(pads.look(0.0, true), 0.0);
        // Held: nothing more until it is let go and pressed again.
        c.end_frame();
        assert!(pads.read(std::slice::from_ref(&c)).is_empty());
        c.set_button(1, Some(PadButton::South), Some(false), None);
        c.set_button(1, Some(PadButton::South), Some(true), None);
        assert_eq!(pads.read(std::slice::from_ref(&c)), vec![KeyCode::KeyF]);
        c.end_frame();
        // Pressed and let go between two frames still counts, once.
        c.set_button(2, Some(PadButton::East), Some(true), None);
        c.set_button(2, Some(PadButton::East), Some(false), None);
        assert_eq!(pads.read(std::slice::from_ref(&c)), vec![KeyCode::KeyR]);
        c.end_frame();
        assert!(pads.read(std::slice::from_ref(&c)).is_empty());
    }

    #[test]
    fn a_flight_stick_is_read_by_number_and_its_lever_only_when_it_moves() {
        let mut pads = Pads::new(Mappings::memory());
        let mut c = stick();
        // Nothing reported yet: nothing taken over.
        pads.read(std::slice::from_ref(&c));
        assert_eq!((pads.yoke, pads.rudder, pads.throttle), (None, None, None));

        // Right and back, twisted left, lever fully forward (the device's -1).
        c.set_axis(0, Some(PadAxis::LeftStickX), 1.0);
        c.set_axis(1, Some(PadAxis::LeftStickY), -1.0);
        c.set_axis(5, None, -1.0);
        c.set_axis(6, None, -1.0);
        pads.read(std::slice::from_ref(&c));
        assert_eq!(pads.yoke, Some(Yoke::Absolute(1.0, 1.0)), "the device's own Y: back is positive");
        assert_eq!((pads.rudder, pads.throttle), (Some(-1.0), Some(1.0)));

        // Centred it still holds the yoke, in the middle; the lever says nothing until it moves.
        c.set_axis(0, Some(PadAxis::LeftStickX), 0.01);
        c.set_axis(1, Some(PadAxis::LeftStickY), 0.0);
        c.set_axis(5, None, 0.03);
        pads.read(std::slice::from_ref(&c));
        assert_eq!((pads.yoke, pads.rudder, pads.throttle), (Some(Yoke::Absolute(0.0, 0.0)), Some(0.0), None));
        c.set_axis(6, None, 0.0);
        pads.read(std::slice::from_ref(&c));
        assert_eq!(pads.throttle, Some(0.5));

        // The hat looks sideways and trims; the trigger changes the view.
        c.set_axis(16, Some(PadAxis::DPadX), -1.0);
        c.set_axis(17, Some(PadAxis::DPadY), 1.0);
        c.set_button(1, Some(PadButton::South), Some(true), None);
        c.set_button(2, None, Some(true), None);
        assert_eq!(pads.read(std::slice::from_ref(&c)), vec![KeyCode::KeyF, KeyCode::KeyV]);
        assert_eq!((pads.look, pads.trim_rate), (-1.0, -1.0));
    }

    #[test]
    fn a_stick_has_the_yoke_over_a_gamepad_and_a_throttle_unit_only_the_throttle() {
        let mut pads = Pads::new(Mappings::memory());
        let (mut pad, mut flight_stick, mut unit) = (gamepad(), stick(), Controller::new(2, "TWCS Throttle", false));
        pad.set_axis(0, Some(PadAxis::LeftStickX), -1.0);
        flight_stick.set_axis(0, None, 0.5);
        unit.set_axis(0, None, 1.0);
        unit.set_axis(2, None, 1.0);
        pads.read(&[pad, flight_stick, unit]);
        assert_eq!(pads.yoke, Some(Yoke::Absolute(shape(0.5, 0.02, 0.2), 0.0)));
        assert_eq!(pads.throttle, Some(0.0));
        assert_eq!(pads.status(), " · Controller: Test Pad + Thrustmaster T.16000M + TWCS Throttle");
    }
}

/// Controllers through the game: a synthetic device, frames of simulated time, the aircraft's controls.
#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::game::{Held, Session};
    use crate::settings::Store;
    use sim_core::{euler_angles, DEG, KNOTS};

    const FRAME: f64 = 1.0 / 60.0;

    /// Flies `seconds` of frames with this controller connected and these keys held.
    fn fly(s: &mut Session, c: &mut Controller, held: Held, seconds: f64) {
        for _ in 0..(seconds / FRAME).round() as usize {
            s.controllers(std::slice::from_ref(c));
            c.end_frame();
            s.advance(FRAME, &held);
        }
    }

    /// In level flight at the cruise the restart gives, about 95 knots.
    fn cruising(assist: bool) -> Session {
        let mut s = Session::new(1, Store::memory());
        s.pilot.assist = assist;
        s.restart_in_air();
        for _ in 0..60 * 20 {
            s.advance(FRAME, &Held::default());
        }
        let ias = s.aircraft().unwrap().indicated_airspeed() / KNOTS;
        assert!((90.0..100.0).contains(&ias), "{ias}");
        s
    }

    fn attitude(s: &Session) -> (f64, f64) {
        let e = euler_angles(s.aircraft().unwrap().state.q);
        (e.roll / DEG, e.pitch / DEG)
    }

    fn pad() -> Controller {
        Controller::new(0, "Test Pad", true)
    }

    #[test]
    fn a_full_gamepad_stick_at_cruise_is_a_gentle_turn_and_the_assist_holds_it_after() {
        let mut s = cruising(true);
        let mut c = pad();
        c.set_axis(0, Some(PadAxis::LeftStickX), -1.0);
        fly(&mut s, &mut c, Held::default(), 1.0);
        let k = s.aircraft().unwrap().controls;
        assert!((k.aileron + 0.4).abs() < 0.02 && k.elevator.abs() < 0.05, "{k:?}");
        let (bank, _) = attitude(&s);
        assert!((-30.0..-12.0).contains(&bank), "a second of full left stick banked {bank}°");
        assert!(s.aircraft().unwrap().crashed().is_none());

        // Stick let go: the keyboard assist has the yoke again and keeps the bank.
        c.set_axis(0, Some(PadAxis::LeftStickX), 0.0);
        fly(&mut s, &mut c, Held::default(), 3.0);
        assert_eq!(s.pilot.mouse_yoke, None);
        let (after, pitch) = attitude(&s);
        assert!((after - bank).abs() < 8.0, "bank {bank}° became {after}°");
        assert!(pitch.abs() < 6.0, "{pitch}");
    }

    #[test]
    fn a_full_pull_at_cruise_raises_the_nose_without_stalling_and_stops_when_let_go() {
        let mut s = cruising(true);
        let (_, level) = attitude(&s);
        let mut c = pad();
        // Back on the stick: gilrs's up is positive, so this is -1.
        c.set_axis(1, Some(PadAxis::LeftStickY), -1.0);
        fly(&mut s, &mut c, Held::default(), 1.0);
        let k = s.aircraft().unwrap().controls;
        assert!(k.elevator > 0.15 && k.elevator < 0.3, "{k:?}");
        let (_, pulled) = attitude(&s);
        assert!((6.0..16.0).contains(&(pulled - level)), "a second of full pull raised the nose {}°", pulled - level);
        assert!(!s.aircraft().unwrap().data().stall_warning);

        c.set_axis(1, Some(PadAxis::LeftStickY), 0.0);
        fly(&mut s, &mut c, Held::default(), 3.0);
        let (_, held) = attitude(&s);
        assert!((held - pulled).abs() < 5.0, "nose at {pulled}° went to {held}°");

        // Forward is nose down.
        c.set_axis(1, Some(PadAxis::LeftStickY), 1.0);
        fly(&mut s, &mut c, Held::default(), 1.0);
        assert!(s.aircraft().unwrap().controls.elevator < 0.0);
        assert!(attitude(&s).1 < held - 3.0);
    }

    #[test]
    fn without_the_assist_the_yoke_returns_to_where_the_keys_left_it() {
        let mut s = cruising(false);
        let mut c = pad();
        fly(&mut s, &mut c, Held { pitch: 1.0, roll: 1.0, ..Held::default() }, 0.5);
        let left = (s.pilot.yoke_x(), s.pilot.yoke_y());
        assert!(left.0 > 0.02 && left.1 > 0.01, "{left:?}");

        c.set_axis(0, Some(PadAxis::LeftStickX), 0.6);
        c.set_axis(1, Some(PadAxis::LeftStickY), 0.6);
        fly(&mut s, &mut c, Held::default(), 0.5);
        let ias = s.aircraft().unwrap().indicated_airspeed() / KNOTS;
        let (sideways, fore_aft) = gamepad_authority(ias);
        let moved = shape(0.6, 0.08, 0.5);
        assert!((s.pilot.yoke_x() - (left.0 + moved * sideways)).abs() < 0.01, "{}", s.pilot.yoke_x());
        assert!((s.pilot.yoke_y() - (left.1 - moved * fore_aft)).abs() < 0.01, "{}", s.pilot.yoke_y());
        let k = s.aircraft().unwrap().controls;
        assert_eq!((k.aileron, k.elevator), (s.pilot.yoke_x(), s.pilot.yoke_y()));

        c.set_axis(0, Some(PadAxis::LeftStickX), 0.0);
        c.set_axis(1, Some(PadAxis::LeftStickY), 0.0);
        fly(&mut s, &mut c, Held::default(), 0.2);
        assert_eq!((s.pilot.yoke_x(), s.pilot.yoke_y()), left);
        assert_eq!(s.pilot.mouse_yoke, None);
    }

    #[test]
    fn the_right_stick_is_the_pedals_while_it_is_out_of_the_centre() {
        let mut s = cruising(false);
        let mut c = pad();
        c.set_axis(3, Some(PadAxis::RightStickX), 1.0);
        fly(&mut s, &mut c, Held::default(), 0.1);
        assert_eq!(s.aircraft().unwrap().controls.rudder, 1.0);
        c.set_axis(3, Some(PadAxis::RightStickX), -0.5);
        fly(&mut s, &mut c, Held::default(), 0.1);
        assert_eq!(s.aircraft().unwrap().controls.rudder, shape(-0.5, 0.08, 0.5));
        c.set_axis(3, Some(PadAxis::RightStickX), 0.02);
        fly(&mut s, &mut c, Held::default(), 0.1);
        assert_eq!(s.pilot.mouse_rudder, None);
    }

    #[test]
    fn gamepad_buttons_do_what_their_keys_do() {
        let mut s = Session::new(1, Store::memory());
        let mut c = pad();
        assert!(s.status_text().ends_with(" hPa"));

        // A, twice, and B: flaps. Y: the view.
        for (button, index, flaps) in [(PadButton::South, 1, 1), (PadButton::South, 1, 2), (PadButton::East, 2, 1)] {
            c.set_button(index, Some(button), Some(true), None);
            fly(&mut s, &mut c, Held::default(), 0.1);
            c.set_button(index, Some(button), Some(false), None);
            fly(&mut s, &mut c, Held::default(), 0.1);
            assert_eq!(s.aircraft().unwrap().controls.flap_notch, flaps);
        }
        c.set_button(4, Some(PadButton::North), Some(true), None);
        fly(&mut s, &mut c, Held::default(), 0.5);
        assert!(!s.cockpit);
        assert!(s.status_text().lines().next().expect("a line").ends_with(" · Controller: Test Pad"));
        c.set_button(4, Some(PadButton::North), Some(false), None);

        // The right trigger half pressed for two seconds, then the left one fully for one.
        c.set_button(8, Some(PadButton::RightTrigger2), None, Some(0.5));
        fly(&mut s, &mut c, Held::default(), 2.0);
        assert!((s.aircraft().unwrap().controls.throttle - 0.45).abs() < 1e-9);
        c.set_button(8, Some(PadButton::RightTrigger2), None, Some(0.0));
        c.set_button(7, Some(PadButton::LeftTrigger2), Some(true), None);
        fly(&mut s, &mut c, Held::default(), 1.0);
        assert!(s.aircraft().unwrap().controls.throttle.abs() < 1e-9);
        c.set_button(7, Some(PadButton::LeftTrigger2), Some(false), None);

        // The D-pad towards the player trims nose up (positive), with the assist on or off.
        c.set_axis(17, Some(PadAxis::DPadY), -1.0);
        fly(&mut s, &mut c, Held::default(), 1.0);
        assert!((s.aircraft().unwrap().controls.trim - 0.12).abs() < 1e-9);
        c.set_axis(17, Some(PadAxis::DPadY), 1.0);
        fly(&mut s, &mut c, Held::default(), 2.0);
        assert!((s.aircraft().unwrap().controls.trim + 0.12).abs() < 1e-9);
        c.set_axis(17, Some(PadAxis::DPadY), 0.0);

        // The right bumper swings the outside camera to the right, as X does.
        let before = s.view().position;
        c.set_button(6, Some(PadButton::RightTrigger), Some(true), None);
        fly(&mut s, &mut c, Held::default(), 0.5);
        assert!((s.view().position - before).length() > 5.0);
        c.set_button(6, Some(PadButton::RightTrigger), Some(false), None);
    }

    #[test]
    fn start_shows_the_menu_and_the_pad_is_the_menu_s_until_start_flies_on() {
        let mut s = Session::new(1, Store::memory());
        let mut c = pad();
        c.set_button(12, Some(PadButton::Start), Some(true), None);
        c.set_button(8, Some(PadButton::RightTrigger2), Some(true), None);
        c.set_axis(0, Some(PadAxis::LeftStickX), 1.0);
        fly(&mut s, &mut c, Held::default(), 1.0);
        assert!(s.paused && s.home_open);
        assert_eq!((s.world.time(), s.aircraft().unwrap().controls.throttle), (0.0, 0.0));
        // The menu has the pad: A chooses in it (kept for it as a move), and does not move the flaps.
        c.set_button(12, Some(PadButton::Start), Some(false), None);
        c.set_button(1, Some(PadButton::South), Some(true), None);
        fly(&mut s, &mut c, Held::default(), 1.0);
        assert!(s.paused && s.menu_keys.contains(&crate::ui::menu::Nav::Activate));
        assert_eq!(s.aircraft().unwrap().controls.flap_notch, 0);
        // Start again flies on, and the trigger held is the throttle's once more.
        c.set_button(1, Some(PadButton::South), Some(false), None);
        c.set_button(12, Some(PadButton::Start), Some(true), None);
        fly(&mut s, &mut c, Held::default(), 1.0);
        assert!(!s.paused && !s.home_open);
        assert_eq!(s.aircraft().unwrap().controls.flap_notch, 0);
        assert!(s.aircraft().unwrap().controls.throttle > 0.4);
    }

    #[test]
    fn a_flight_stick_is_the_yoke_whatever_the_assist_says() {
        for assist in [true, false] {
            let mut s = cruising(assist);
            let mut c = Controller::new(0, "Thrustmaster T.16000M", false);
            c.set_axis(0, None, 0.3);
            c.set_axis(1, None, -0.2);
            c.set_axis(5, None, 0.5);
            fly(&mut s, &mut c, Held { roll: -1.0, pitch: 1.0, ..Held::default() }, 0.5);
            let k = s.aircraft().unwrap().controls;
            let expected = (shape(f64::from(0.3f32), 0.02, 0.2), shape(f64::from(-0.2f32), 0.02, 0.2), shape(0.5, 0.06, 0.2));
            assert_eq!((k.aileron, k.elevator, k.rudder), expected, "assist {assist}");
            assert!(expected.0 > 0.2 && expected.1 < -0.1 && expected.2 > 0.3);

            // Centred, the yoke is centred: nothing holds an attitude for the pilot.
            c.set_axis(0, None, 0.0);
            c.set_axis(1, None, 0.0);
            c.set_axis(5, None, 0.0);
            fly(&mut s, &mut c, Held::default(), 0.2);
            let k = s.aircraft().unwrap().controls;
            assert_eq!((k.aileron, k.elevator, k.rudder), (0.0, 0.0, 0.0));
        }
    }

    #[test]
    fn the_lever_of_a_flight_stick_sets_the_throttle_and_the_keys_still_move_it() {
        let mut s = Session::new(1, Store::memory());
        let mut c = Controller::new(0, "Thrustmaster T.16000M", false);
        // Three quarters of the way forward; the device sends forward as negative.
        c.set_axis(6, None, -0.5);
        fly(&mut s, &mut c, Held::default(), 0.1);
        assert_eq!(s.aircraft().unwrap().controls.throttle, 0.75);
        fly(&mut s, &mut c, Held { throttle_down: true, ..Held::default() }, 1.0);
        assert!((s.aircraft().unwrap().controls.throttle - 0.30).abs() < 1e-9);
        c.set_axis(6, None, 1.0);
        fly(&mut s, &mut c, Held::default(), 0.1);
        assert_eq!(s.aircraft().unwrap().controls.throttle, 0.0);

        // Trigger: the view. Second button: flaps down.
        c.set_button(1, None, Some(true), None);
        c.set_button(2, None, Some(true), None);
        fly(&mut s, &mut c, Held::default(), 0.1);
        assert!(!s.cockpit);
        assert_eq!(s.aircraft().unwrap().controls.flap_notch, 1);
    }

    #[test]
    fn a_controller_unplugged_in_flight_gives_the_yoke_back() {
        let mut s = cruising(true);
        let mut c = pad();
        c.set_axis(0, Some(PadAxis::LeftStickX), 1.0);
        fly(&mut s, &mut c, Held::default(), 0.3);
        assert!(s.pilot.mouse_yoke.is_some());
        s.controllers(&[]);
        s.advance(FRAME, &Held::default());
        s.controllers(&[]);
        s.advance(FRAME, &Held::default());
        assert_eq!((s.pilot.mouse_yoke, s.pilot.mouse_rudder), (None, None));
        assert!(s.status_text().ends_with(" hPa"));
    }
}
