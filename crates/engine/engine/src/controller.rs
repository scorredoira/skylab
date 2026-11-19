//! Game controllers: gamepads, flight sticks, wheels, anything the operating system calls a joystick.
//!
//! A [`Controller`] is plain data: its axes and buttons as they are this frame. Each axis and button
//! has a number that depends only on the device (the same on macOS and Linux for an ordinary HID
//! device) and, where gilrs knows the layout, a gamepad name. A flight stick has numbers and a few
//! names that mean little; a gamepad in the layout database has the names every gamepad shares.
//! [`ControllerPort`] is what reads the devices.

use gilrs::{EventType, Gilrs, GilrsBuilder, MappingSource};

pub use gilrs::{Axis as PadAxis, Button as PadButton};

const AXIS_NAMES: [PadAxis; 8] =
    [PadAxis::LeftStickX, PadAxis::LeftStickY, PadAxis::LeftZ, PadAxis::RightStickX, PadAxis::RightStickY, PadAxis::RightZ, PadAxis::DPadX, PadAxis::DPadY];

const BUTTON_NAMES: [PadButton; 19] = [
    PadButton::South,
    PadButton::East,
    PadButton::North,
    PadButton::West,
    PadButton::C,
    PadButton::Z,
    PadButton::LeftTrigger,
    PadButton::LeftTrigger2,
    PadButton::RightTrigger,
    PadButton::RightTrigger2,
    PadButton::Select,
    PadButton::Start,
    PadButton::Mode,
    PadButton::LeftThumb,
    PadButton::RightThumb,
    PadButton::DPadUp,
    PadButton::DPadDown,
    PadButton::DPadLeft,
    PadButton::DPadRight,
];

/// The axis written `LeftStickX`, `RightZ`, `DPadY`...: the names `{:?}` prints.
pub fn pad_axis_named(name: &str) -> Option<PadAxis> {
    AXIS_NAMES.into_iter().find(|a| format!("{a:?}") == name)
}

/// The button written `South`, `LeftTrigger2`, `DPadUp`...: the names `{:?}` prints.
pub fn pad_button_named(name: &str) -> Option<PadButton> {
    BUTTON_NAMES.into_iter().find(|b| format!("{b:?}") == name)
}

/// One axis of a controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisState {
    /// The device's own number for it: 0 X, 1 Y, 2 Z, 3 Rx, 4 Ry, 5 Rz, 6 slider, 7 dial, 8 wheel,
    /// 16 and 17 the hat.
    pub index: u32,
    /// What it is on a gamepad, if gilrs knows.
    pub name: Option<PadAxis>,
    /// -1...1 in the gamepad convention: right and **up** are positive.
    pub value: f32,
}

impl AxisState {
    /// -1...1 as the device sends it: on a stick, right and **back** (towards the player) are
    /// positive. gilrs turns over the axes it names as vertical; this turns them back.
    pub fn raw(&self) -> f32 {
        match self.name {
            Some(PadAxis::LeftStickY | PadAxis::RightStickY | PadAxis::DPadY) => -self.value,
            _ => self.value,
        }
    }
}

/// One button of a controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonState {
    /// The device's own number for it, from 1 as printed on a flight stick; 0 for the four
    /// directions made from a hat, which have no number.
    pub index: u32,
    pub name: Option<PadButton>,
    pub down: bool,
    /// Went down since the last frame.
    pub pressed: bool,
    /// How far it is pressed, 0...1: an analogue trigger gives everything in between.
    pub value: f32,
}

/// A connected controller as it is this frame. Axes and buttons appear in the lists the first time
/// the device reports them, which for an axis is the first time it moves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Controller {
    /// Stays the same while the device is connected.
    pub id: usize,
    pub name: String,
    /// gilrs has this device in its layout database, so the names are the usual gamepad ones.
    /// False for flight sticks and other joysticks: go by number.
    pub standard: bool,
    pub axes: Vec<AxisState>,
    pub buttons: Vec<ButtonState>,
}

impl Controller {
    /// A controller with nothing reported yet. Also how tests make one: fill it with `set_axis`
    /// and `set_button`.
    pub fn new(id: usize, name: &str, standard: bool) -> Controller {
        Controller { id, name: name.to_string(), standard, axes: Vec::new(), buttons: Vec::new() }
    }

    /// The named axis in the gamepad convention (up is positive); 0 if the device has not reported it.
    pub fn axis(&self, name: PadAxis) -> f32 {
        self.named_axis(name).unwrap_or(0.0)
    }

    /// As `axis`, or nothing if the device has not reported it.
    pub fn named_axis(&self, name: PadAxis) -> Option<f32> {
        self.axes.iter().find(|a| a.name == Some(name)).map(|a| a.value)
    }

    /// The axis with this number as the device sends it (back is positive), if it has reported it.
    pub fn axis_raw(&self, index: u32) -> Option<f32> {
        self.axes.iter().find(|a| a.index == index).map(AxisState::raw)
    }

    pub fn is_down(&self, name: PadButton) -> bool {
        self.buttons.iter().any(|b| b.name == Some(name) && b.down)
    }

    /// True on the frame the button went down.
    pub fn pressed(&self, name: PadButton) -> bool {
        self.buttons.iter().any(|b| b.name == Some(name) && b.pressed)
    }

    /// How far the button is pressed, 0...1.
    pub fn button_value(&self, name: PadButton) -> f32 {
        self.buttons.iter().filter(|b| b.name == Some(name)).map(|b| b.value).fold(0.0, f32::max)
    }

    /// The button with this number (from 1), if the device has reported it.
    pub fn button_raw(&self, index: u32) -> Option<&ButtonState> {
        self.buttons.iter().find(|b| b.index == index && index != 0)
    }

    /// An axis moved. A hat (`DPadX`, `DPadY`) also holds down the direction buttons `DPadLeft`...
    /// `DPadUp`, so a game finds the directions in one place whether the device has a hat or four buttons.
    pub fn set_axis(&mut self, index: u32, name: Option<PadAxis>, value: f32) {
        match self.axes.iter_mut().find(|a| a.index == index && a.name == name) {
            Some(axis) => axis.value = value,
            None => self.axes.push(AxisState { index, name, value }),
        }
        let directions = match name {
            Some(PadAxis::DPadX) => Some((PadButton::DPadLeft, PadButton::DPadRight)),
            Some(PadAxis::DPadY) => Some((PadButton::DPadDown, PadButton::DPadUp)),
            _ => None,
        };
        if let Some((minus, plus)) = directions {
            self.set_button(0, Some(minus), Some(value < -0.5), None);
            self.set_button(0, Some(plus), Some(value > 0.5), None);
        }
    }

    /// A button changed: whether it is down, how far it is pressed, or both. Without a `value` it
    /// is 1 when down and 0 when up.
    pub fn set_button(&mut self, index: u32, name: Option<PadButton>, down: Option<bool>, value: Option<f32>) {
        let at = match self.buttons.iter().position(|b| b.index == index && b.name == name) {
            Some(at) => at,
            None => {
                self.buttons.push(ButtonState { index, name, down: false, pressed: false, value: 0.0 });
                self.buttons.len() - 1
            }
        };
        let button = &mut self.buttons[at];
        if let Some(down) = down {
            button.pressed |= down && !button.down;
            button.down = down;
            button.value = if down { 1.0 } else { 0.0 };
        }
        if let Some(value) = value {
            button.value = value;
        }
    }

    /// Forgets which buttons went down this frame.
    pub fn end_frame(&mut self) {
        for button in &mut self.buttons {
            button.pressed = false;
        }
    }
}

/// The device's number for an axis from the code of the operating system. HID numbers its axes X, Y,
/// Z, Rx, Ry, Rz, slider, dial, wheel; Linux keeps that order in its ABS codes, so both give 0...8.
fn axis_index(code: u32) -> u32 {
    let low = code & 0xffff;
    if cfg!(target_os = "macos") {
        // Usage page in the high half, usage in the low one.
        match (code >> 16, low) {
            (1, 0x30..=0x38) => low - 0x30,
            // The hat, where Linux has ABS_HAT0X and ABS_HAT0Y.
            (1, 0x39..=0x3a) => low - 0x39 + 16,
            // Simulation controls, numbered as Linux does: throttle, rudder, accelerator, brake.
            (2, 0xbb) => 6,
            (2, 0xba) => 7,
            (2, 0xc4) => 9,
            (2, 0xc5) => 10,
            _ => 0x100 + low,
        }
    } else {
        low
    }
}

/// The device's number for a button, from 1. HID numbers them from 1; Linux gives a joystick's
/// first button the code 0x120 (and a gamepad's the code 0x130, so those start at 17).
fn button_index(code: u32) -> u32 {
    let low = code & 0xffff;
    if cfg!(target_os = "linux") {
        if low >= 0x120 {
            low - 0x11f
        } else {
            0x1000 + low
        }
    } else if cfg!(target_os = "macos") && code >> 16 != 9 {
        // Not on the button page: Start, Select and the like on some devices.
        0x1000 + low
    } else {
        low
    }
}

/// Reads the controllers of the machine. Devices can come and go at any time.
pub struct ControllerPort {
    gilrs: Option<Gilrs>,
}

impl Default for ControllerPort {
    fn default() -> ControllerPort {
        ControllerPort::new()
    }
}

impl ControllerPort {
    /// Starts listening. If the system has no controller support this is a port where nothing is
    /// ever connected.
    pub fn new() -> ControllerPort {
        // No filters: gilrs's own dead zone and jitter filters would hide the small movements a game
        // wants to shape itself, and its hat filter behaves differently on each system.
        let gilrs = match GilrsBuilder::new().with_default_filters(false).build() {
            Ok(gilrs) => Some(gilrs),
            Err(gilrs::Error::NotImplemented(gilrs)) => Some(gilrs),
            Err(error) => {
                eprintln!("controllers: {error}");
                None
            }
        };
        ControllerPort { gilrs }
    }

    /// Brings `list` up to date: new devices are added, gone ones removed, axes and buttons moved.
    /// `pressed` flags accumulate until `Controller::end_frame`.
    pub fn poll(&mut self, list: &mut Vec<Controller>) {
        let Some(gilrs) = &mut self.gilrs else { return };
        while let Some(event) = gilrs.next_event() {
            let id = usize::from(event.id);
            if matches!(event.event, EventType::Disconnected) {
                list.retain(|c| c.id != id);
                continue;
            }
            let at = match list.iter().position(|c| c.id == id) {
                Some(at) => at,
                None => {
                    let pad = gilrs.gamepad(event.id);
                    list.push(Controller::new(id, pad.name(), pad.mapping_source() == MappingSource::SdlMappings));
                    list.len() - 1
                }
            };
            let controller = &mut list[at];
            let axis_name = |a: PadAxis| Some(a).filter(|a| *a != PadAxis::Unknown);
            let button_name = |b: PadButton| Some(b).filter(|b| *b != PadButton::Unknown);
            match event.event {
                EventType::AxisChanged(axis, value, code) => controller.set_axis(axis_index(code.into_u32()), axis_name(axis), value),
                EventType::ButtonPressed(button, code) => controller.set_button(button_index(code.into_u32()), button_name(button), Some(true), None),
                EventType::ButtonReleased(button, code) => controller.set_button(button_index(code.into_u32()), button_name(button), Some(false), None),
                EventType::ButtonChanged(button, value, code) => controller.set_button(button_index(code.into_u32()), button_name(button), None, Some(value)),
                _ => {}
            }
        }
        // Devices that were there before the port was opened send no event.
        list.retain(|c| gilrs.gamepads().any(|(id, _)| usize::from(id) == c.id));
        for (id, pad) in gilrs.gamepads() {
            if !list.iter().any(|c| c.id == usize::from(id)) {
                list.push(Controller::new(usize::from(id), pad.name(), pad.mapping_source() == MappingSource::SdlMappings));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_read_as_they_are_printed() {
        assert_eq!(pad_axis_named("LeftStickX"), Some(PadAxis::LeftStickX));
        assert_eq!(pad_axis_named("DPadY"), Some(PadAxis::DPadY));
        assert_eq!(pad_button_named("RightTrigger2"), Some(PadButton::RightTrigger2));
        assert_eq!(pad_axis_named("South"), None);
        assert_eq!(pad_button_named("Unknown"), None);
    }

    #[test]
    fn axes_by_name_point_up_and_by_number_as_the_device_sends_them() {
        let mut c = Controller::new(0, "pad", true);
        assert_eq!((c.axis(PadAxis::LeftStickY), c.axis_raw(1)), (0.0, None));
        c.set_axis(0, Some(PadAxis::LeftStickX), 0.25);
        c.set_axis(1, Some(PadAxis::LeftStickY), 0.5);
        c.set_axis(6, None, -0.75);
        c.set_axis(1, Some(PadAxis::LeftStickY), 0.6);
        assert_eq!(c.axes.len(), 3);
        assert_eq!((c.axis(PadAxis::LeftStickX), c.axis_raw(0)), (0.25, Some(0.25)));
        assert_eq!((c.axis(PadAxis::LeftStickY), c.axis_raw(1)), (0.6, Some(-0.6)));
        assert_eq!((c.named_axis(PadAxis::RightZ), c.axis_raw(6)), (None, Some(-0.75)));
    }

    #[test]
    fn buttons_are_pressed_for_one_frame_and_triggers_have_a_value() {
        let mut c = Controller::new(0, "pad", true);
        c.set_button(1, Some(PadButton::South), Some(true), None);
        assert!(c.is_down(PadButton::South) && c.pressed(PadButton::South));
        assert_eq!(c.button_value(PadButton::South), 1.0);
        c.end_frame();
        assert!(c.is_down(PadButton::South) && !c.pressed(PadButton::South));
        // Pressed and released within a frame still counts as pressed.
        c.set_button(2, None, Some(true), None);
        c.set_button(2, None, Some(false), None);
        let b = c.button_raw(2).expect("reported");
        assert!(b.pressed && !b.down && b.value == 0.0);
        // An analogue trigger: a value first, down only when gilrs says so.
        c.set_button(8, Some(PadButton::RightTrigger2), None, Some(0.4));
        assert!(!c.is_down(PadButton::RightTrigger2));
        assert_eq!(c.button_value(PadButton::RightTrigger2), 0.4);
        assert!(c.button_raw(0).is_none() && c.button_raw(9).is_none());
    }

    #[test]
    fn a_hat_holds_the_direction_buttons() {
        let mut c = Controller::new(0, "stick", false);
        c.set_axis(17, Some(PadAxis::DPadY), 1.0);
        assert!(c.pressed(PadButton::DPadUp) && c.is_down(PadButton::DPadUp) && !c.is_down(PadButton::DPadDown));
        c.end_frame();
        c.set_axis(17, Some(PadAxis::DPadY), -1.0);
        assert!(!c.is_down(PadButton::DPadUp) && c.pressed(PadButton::DPadDown));
        c.set_axis(16, Some(PadAxis::DPadX), -1.0);
        assert!(c.is_down(PadButton::DPadLeft) && !c.is_down(PadButton::DPadRight));
        c.set_axis(17, Some(PadAxis::DPadY), 0.0);
        assert!(!c.is_down(PadButton::DPadDown) && c.is_down(PadButton::DPadLeft));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hid_usages_become_device_numbers() {
        let desktop = |usage: u32| (1 << 16) | usage;
        assert_eq!([0x30, 0x31, 0x35, 0x36].map(|u| axis_index(desktop(u))), [0, 1, 5, 6]);
        assert_eq!((axis_index(desktop(0x39)), axis_index(desktop(0x3a))), (16, 17));
        assert_eq!((axis_index((2 << 16) | 0xbb), axis_index((2 << 16) | 0xba)), (6, 7));
        assert_eq!((button_index((9 << 16) | 1), button_index((9 << 16) | 16)), (1, 16));
        assert_eq!(button_index((1 << 16) | 0x3d), 0x103d);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_codes_become_device_numbers() {
        assert_eq!([0, 1, 5, 6, 16].map(|abs| axis_index((3 << 16) | abs)), [0, 1, 5, 6, 16]);
        assert_eq!((button_index((1 << 16) | 0x120), button_index((1 << 16) | 0x12f), button_index((1 << 16) | 0x130)), (1, 16, 17));
    }

    #[test]
    fn a_port_with_nothing_connected_polls_without_devices_or_panics() {
        let mut port = ControllerPort::new();
        let mut list = vec![Controller::new(99, "gone", true)];
        port.poll(&mut list);
        assert!(list.iter().all(|c| c.id != 99));
    }
}
