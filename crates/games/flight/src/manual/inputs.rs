//! What the manual tells the player to do, named by what it does (`Act`), and the keys and the
//! joystick's controls that do it. The G1000's keys and the joystick's buttons are looked up in the
//! game's own tables (`controls::panel_key`, the T.16000M's defaults), so the manual cannot name
//! one the game does not use; the rest are checked against the game by `game.rs`'s tests.

use crate::cockpit::{Panel, PanelKnob};
use crate::controls::{Action, ButtonRef, Mapping};
use engine::KeyCode;
use sim_core::AfcsKey;

/// The device the joystick's column of the manual is about, as it names itself.
pub const JOYSTICK: &str = "Thrustmaster T.16000M";

/// Something the player does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Held: more or less throttle.
    Throttle,
    /// Held: the yoke back (nose up) or forward.
    NoseUp,
    NoseDown,
    /// Held: the wings to one side or the other.
    Bank,
    Pedals,
    Brakes,
    /// Held: the trim, without the keyboard's assist.
    Trim,
    FlapsDown,
    FlapsUp,
    /// The keyboard's assist on or off.
    Assist,
    /// Cockpit or outside.
    View,
    /// The two views in turn: the seat and leaning in.
    Views,
    /// The view zoomed in, or out.
    Closer,
    Further,
    /// Held: the head turned left or right.
    LookSide,
    /// The head ahead again.
    LookAhead,
    Runway,
    Air,
    Final,
    Picker,
    Settings,
    /// Held: the clock forwards (with shift, back).
    Hour,
    /// Time slower or faster: ×1 to ×16.
    Time,
    Help,
    Manual,
    Pause,
    /// The game's autopilot lands at the nearest aerodrome.
    Land,
    /// The altimeter to the nearest aerodrome's QNH.
    Qnh,
    /// The altimeter setting a hectopascal up or down.
    Baro,
    /// AP DISC.
    Disconnect,
    /// The replay on or off.
    Replay,
    /// In the replay: play or stop; jump back or on (10 s, with shift 30 s); slower or faster.
    ReplayPlay,
    ReplayJump,
    ReplaySpeed,
    /// A video of the game, with its sound, started or saved (the engine's key).
    Record,
    /// A key or a knob of the G1000.
    Panel(Panel),
    /// A multirotor's arm switch.
    Arm,
}

/// Keys the G1000's acts are looked for among (`controls::panel_key`), each without and with shift.
const PANEL_KEYS: [KeyCode; 15] = [
    KeyCode::KeyP,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::BracketLeft,
    KeyCode::BracketRight,
    KeyCode::Backslash,
    KeyCode::Semicolon,
    KeyCode::Quote,
    KeyCode::Slash,
];

impl Act {
    /// The keys that do it, with shift or not, in the order they are shown.
    pub fn keys(self) -> Vec<(KeyCode, bool)> {
        use KeyCode::*;
        let plain = |keys: &[KeyCode]| keys.iter().map(|&k| (k, false)).collect();
        match self {
            Act::Throttle => plain(&[KeyW, KeyS]),
            Act::NoseUp => plain(&[ArrowDown]),
            Act::NoseDown => plain(&[ArrowUp]),
            Act::Bank => plain(&[ArrowLeft, ArrowRight]),
            Act::Pedals => plain(&[KeyA, KeyD]),
            Act::Brakes => plain(&[KeyB]),
            Act::Trim => plain(&[KeyQ, KeyE]),
            Act::FlapsDown => plain(&[KeyF]),
            Act::FlapsUp => plain(&[KeyR]),
            Act::Assist => plain(&[KeyG]),
            Act::View => plain(&[KeyV]),
            Act::Views => plain(&[Space]),
            Act::Closer => plain(&[Equal]),
            Act::Further => plain(&[Minus]),
            Act::LookSide => plain(&[KeyZ, KeyX]),
            Act::LookAhead => plain(&[Digit0]),
            Act::Runway => plain(&[Digit1]),
            Act::Air => plain(&[Digit2]),
            Act::Final => plain(&[Digit3]),
            Act::Picker => plain(&[KeyM]),
            Act::Settings => plain(&[KeyO]),
            Act::Hour => vec![(KeyT, false), (KeyT, true)],
            Act::Time => plain(&[F9, F10]),
            Act::Help => plain(&[KeyH]),
            Act::Manual => plain(&[F1]),
            Act::Pause => plain(&[Escape]),
            Act::Land => plain(&[KeyL]),
            Act::Qnh => plain(&[KeyY]),
            Act::Baro => vec![(KeyK, false), (KeyK, true)],
            Act::Disconnect => plain(&[KeyC]),
            Act::Replay => plain(&[F2]),
            Act::ReplayPlay => plain(&[Space]),
            Act::ReplayJump => vec![(ArrowLeft, false), (ArrowRight, false), (ArrowLeft, true)],
            Act::ReplaySpeed => plain(&[ArrowDown, ArrowUp]),
            Act::Arm => plain(&[KeyR]),
            Act::Record => plain(&[engine::video::KEY]),
            Act::Panel(panel) => [false, true]
                .into_iter()
                .flat_map(|shift| PANEL_KEYS.iter().map(move |&k| (k, shift)))
                .filter(|&(k, shift)| crate::controls::panel_key(k, shift) == Some(panel))
                .take(1)
                .collect(),
        }
    }

    /// The keys as the manual writes them: `W`, `⇧;`.
    pub fn key_labels(self) -> Vec<String> {
        self.keys().into_iter().map(|(k, shift)| format!("{}{}", if shift { "⇧" } else { "" }, key_name(k))).collect()
    }

    /// What does it on the T.16000M with its default mapping, if anything does.
    pub fn stick(self) -> Option<String> {
        let axis = match self {
            Act::Throttle => Some("lever on the base"),
            Act::NoseUp => Some("stick back"),
            Act::NoseDown => Some("stick forward"),
            Act::Bank => Some("stick to one side"),
            Act::Pedals => Some("twist the stick"),
            Act::Trim => Some("hat ↕"),
            Act::LookSide => Some("hat ↔"),
            _ => None,
        };
        if let Some(text) = axis {
            return Some(text.to_string());
        }
        // A button: the action whose key this is, unshifted, and the button mapped to it.
        let (key, shift) = *self.keys().first()?;
        let action = Action::ALL.into_iter().find(|a| a.key_code() == Some(key)).filter(|_| !shift)?;
        stick_button(action).map(|n| if n == 1 { "trigger (1)".to_string() } else { format!("button {n}") })
    }
}

/// The button of the T.16000M an action has by default.
pub fn stick_button(action: Action) -> Option<u32> {
    let mapping = Mapping::default_for(JOYSTICK, false);
    mapping.buttons.get(&action)?.iter().find_map(|b| match b {
        ButtonRef::Number(n) => Some(*n),
        _ => None,
    })
}

/// A key's name on the keyboard, as the help writes it.
pub fn key_name(key: KeyCode) -> &'static str {
    use KeyCode::*;
    match key {
        KeyA => "A",
        KeyB => "B",
        KeyC => "C",
        KeyD => "D",
        KeyE => "E",
        KeyF => "F",
        KeyG => "G",
        KeyH => "H",
        KeyK => "K",
        KeyL => "L",
        KeyM => "M",
        KeyO => "O",
        KeyP => "P",
        KeyQ => "Q",
        KeyR => "R",
        KeyS => "S",
        KeyT => "T",
        KeyV => "V",
        KeyW => "W",
        KeyX => "X",
        KeyY => "Y",
        KeyZ => "Z",
        Digit0 => "0",
        Digit1 => "1",
        Digit2 => "2",
        Digit3 => "3",
        Digit4 => "4",
        Digit5 => "5",
        Digit6 => "6",
        Digit7 => "7",
        Digit8 => "8",
        Digit9 => "9",
        ArrowUp => "↑",
        ArrowDown => "↓",
        ArrowLeft => "←",
        ArrowRight => "→",
        Space => "Space",
        Equal => "+",
        Minus => "−",
        Comma => ",",
        Period => ".",
        BracketLeft => "[",
        BracketRight => "]",
        Backslash => "\\",
        Semicolon => ";",
        Quote => "'",
        Slash => "/",
        Escape => "Esc",
        F1 => "F1",
        F2 => "F2",
        F9 => "F9",
        F10 => "F10",
        F12 => "F12",
        _ => "?",
    }
}

/// The G1000's: a key, or a knob a click one way.
pub const fn key(key: AfcsKey) -> Act {
    Act::Panel(Panel::Key(key))
}

pub const fn knob(knob: PanelKnob, clicks: i32) -> Act {
    Act::Panel(Panel::Knob(knob, clicks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::{AxisSource, Role};
    use engine::PadAxis;

    #[test]
    fn the_g1000s_acts_are_found_among_its_keys() {
        assert_eq!(key(AfcsKey::Ap).key_labels(), ["P"]);
        assert_eq!(key(AfcsKey::Fd).key_labels(), ["⇧P"]);
        assert_eq!(knob(PanelKnob::AltitudeCoarse, 1).key_labels(), ["⇧'"]);
        assert_eq!(Act::Panel(Panel::DirectTo).key_labels(), ["/"]);
        assert!(knob(PanelKnob::Course, 1).keys().is_empty(), "no key turns the CRS knob");
    }

    #[test]
    fn the_joystick_is_described_as_its_defaults_map_it() {
        let m = Mapping::default_for(JOYSTICK, false);
        for role in [Role::Throttle, Role::Pitch, Role::Roll, Role::Rudder] {
            assert!(m.axes.contains_key(&role), "{role:?}");
        }
        // The hat: fore and aft the trim, sideways the head.
        assert_eq!(m.axes[&Role::Look].source, AxisSource::Named(PadAxis::DPadX));
        for trim in [Action::TrimNoseUp, Action::TrimNoseDown] {
            assert!(matches!(m.buttons[&trim][..], [ButtonRef::Axis(AxisSource::Named(PadAxis::DPadY), _)]), "{trim:?}");
        }
        assert_eq!(Act::View.stick().as_deref(), Some("trigger (1)"));
        assert_eq!(Act::Views.stick().as_deref(), Some("button 4"));
        assert_eq!(key(AfcsKey::Ap).stick().as_deref(), Some("button 7"));
        assert_eq!(key(AfcsKey::Fd).stick(), None, "shifted keys are not a button's");
        assert_eq!(Act::Manual.stick(), None);
    }

    #[test]
    fn every_key_has_a_name() {
        for act in super::super::pages::acts() {
            for (key, _) in act.keys() {
                assert_ne!(key_name(key), "?", "{act:?}: {key:?}");
            }
        }
    }
}
