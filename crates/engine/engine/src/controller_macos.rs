//! macOS: gamepads read through the system's GameController framework. It speaks to each pad as
//! the pad wants (a Switch Pro over Bluetooth sends nothing until it is told to send its full
//! reports, which the framework does and gilrs does not), and gives every one the same layout.
//! Flight sticks, throttles and wheels are not gamepads to it: those stay with gilrs.

use objc2::rc::Retained;
use objc2_game_controller::{GCController, GCControllerButtonInput, GCDevice, GCExtendedGamepad, GCSystemGestureState};

use super::{Controller, PadAxis, PadButton};

/// The ids of these pads start here, above any gilrs gives.
pub const FIRST_ID: usize = 1 << 20;

/// The pads seen, each with its id while it stays connected.
pub struct Pads {
    known: Vec<(Retained<GCController>, usize)>,
    next: usize,
}

impl Pads {
    pub fn new() -> Pads {
        // Also when the game is not the frontmost app: `--controllers` runs in a terminal.
        unsafe { GCController::setShouldMonitorBackgroundEvents(true) };
        Pads { known: Vec::new(), next: FIRST_ID }
    }

    /// Brings the pads of `list` (those with an id from `FIRST_ID`) up to date. Whether there is one.
    pub fn poll(&mut self, list: &mut Vec<Controller>) -> bool {
        let all = unsafe { GCController::controllers() };
        let pads: Vec<(Retained<GCController>, Retained<GCExtendedGamepad>)> =
            (0..all.count()).map(|i| all.objectAtIndex(i)).filter_map(|c| unsafe { c.extendedGamepad() }.map(|g| (c, g))).collect();
        let here = |c: &Retained<GCController>| pads.iter().any(|(p, _)| Retained::as_ptr(p) == Retained::as_ptr(c));
        self.known.retain(|(c, _)| here(c));
        list.retain(|c| c.id < FIRST_ID || self.known.iter().any(|(_, id)| *id == c.id));
        for (pad, gamepad) in &pads {
            let id = match self.known.iter().find(|(c, _)| Retained::as_ptr(c) == Retained::as_ptr(pad)) {
                Some((_, id)) => *id,
                None => {
                    let id = self.next;
                    self.next += 1;
                    self.known.push((pad.clone(), id));
                    // Its Home button is the game's while it runs, not the system's (which opens Games).
                    if let Some(home) = unsafe { gamepad.buttonHome() } {
                        unsafe { home.setPreferredSystemGestureState(GCSystemGestureState::Disabled) };
                    }
                    id
                }
            };
            let at = match list.iter().position(|c| c.id == id) {
                Some(at) => at,
                None => {
                    // Its vendor's name, or what it is when that is blank (a Switch Pro's is spaces).
                    let vendor = unsafe { pad.vendorName() }.map(|n| n.to_string().trim().to_string()).unwrap_or_default();
                    let name = if vendor.is_empty() { unsafe { pad.productCategory() }.to_string() } else { vendor };
                    list.push(Controller::new(id, &name, true));
                    list.len() - 1
                }
            };
            read(&mut list[at], gamepad);
        }
        !pads.is_empty()
    }
}

/// The pad as it is now, numbered as an ordinary HID gamepad: the sticks X, Y, Rx, Ry; the buttons
/// from 1.
fn read(c: &mut Controller, g: &GCExtendedGamepad) {
    unsafe {
        let (left, right) = (g.leftThumbstick(), g.rightThumbstick());
        c.set_axis(0, Some(PadAxis::LeftStickX), left.xAxis().value());
        c.set_axis(1, Some(PadAxis::LeftStickY), left.yAxis().value());
        c.set_axis(3, Some(PadAxis::RightStickX), right.xAxis().value());
        c.set_axis(4, Some(PadAxis::RightStickY), right.yAxis().value());
        let dpad = g.dpad();
        let buttons: [(u32, PadButton, Option<Retained<GCControllerButtonInput>>); 17] = [
            (1, PadButton::South, Some(g.buttonA())),
            (2, PadButton::East, Some(g.buttonB())),
            (3, PadButton::West, Some(g.buttonX())),
            (4, PadButton::North, Some(g.buttonY())),
            (5, PadButton::LeftTrigger, Some(g.leftShoulder())),
            (6, PadButton::RightTrigger, Some(g.rightShoulder())),
            (7, PadButton::LeftTrigger2, Some(g.leftTrigger())),
            (8, PadButton::RightTrigger2, Some(g.rightTrigger())),
            (9, PadButton::Select, g.buttonOptions()),
            (10, PadButton::Start, Some(g.buttonMenu())),
            (11, PadButton::LeftThumb, g.leftThumbstickButton()),
            (12, PadButton::RightThumb, g.rightThumbstickButton()),
            (13, PadButton::Mode, g.buttonHome()),
            (14, PadButton::DPadUp, Some(dpad.up())),
            (15, PadButton::DPadDown, Some(dpad.down())),
            (16, PadButton::DPadLeft, Some(dpad.left())),
            (17, PadButton::DPadRight, Some(dpad.right())),
        ];
        for (index, name, button) in buttons {
            if let Some(button) = button {
                c.set_button(index, Some(name), Some(button.isPressed()), Some(button.value()));
            }
        }
    }
}
