use crate::controller::Controller;
use std::collections::HashSet;
use winit::keyboard::KeyCode;

/// Keyboard, mouse and controller state for the current frame.
#[derive(Default)]
pub struct Input {
    down: HashSet<KeyCode>,
    /// Keys that went down since the last frame, in order.
    pub pressed: Vec<KeyCode>,
    pub shift: bool,
    /// Pointer position in logical points, origin top-left.
    pub mouse: (f32, f32),
    /// Pointer movement since the last frame, in logical points.
    pub mouse_delta: (f32, f32),
    /// Lines the wheel turned since the last frame: positive away from the user (up), unless the
    /// overlay took them. With Shift held, a scroll across counts too (`wheel_lines`); a notch of a
    /// mouse's wheel is a line at least (`notch_lines`).
    pub scroll: f32,
    pub left_button: bool,
    pub right_button: bool,
    /// True on the frame the left button went down, unless the overlay took the click.
    pub clicked: bool,
    /// The 2D overlay is using the pointer; the game should ignore it.
    pub pointer_over_ui: bool,
    /// False while another window has the keyboard.
    pub unfocused: bool,
    /// The game controllers connected now, as they are this frame.
    pub controllers: Vec<Controller>,
}

/// The lines a turn of the wheel counts for (`Input::scroll`), of those it sent `across` and `up`:
/// with Shift held macOS sends a mouse's wheel as a scroll across, and it is the wheel all the same.
pub(crate) fn wheel_lines(across: f32, up: f32, shift: bool) -> f32 {
    if shift && up == 0.0 { across } else { up }
}

/// Points of a trackpad's scroll to a line.
const POINTS_PER_LINE: f32 = 40.0;

/// The lines a notch of a mouse's wheel counts for, of those the system says. macOS (`by_speed`)
/// says how far to scroll, by how fast the wheel turns: a tenth of a line for a notch turned
/// slowly, several for one of a quick spin. A game that counts whole lines (a knob's clicks, a
/// camera's steps) would do nothing for ten slow notches: there a notch is a line at least.
pub(crate) fn notch_lines(lines: f32, by_speed: bool) -> f32 {
    if by_speed && lines != 0.0 { lines.signum() * lines.abs().max(1.0) } else { lines }
}

impl Input {
    pub fn is_down(&self, key: KeyCode) -> bool {
        self.down.contains(&key)
    }

    /// -1, 0 or 1 from a pair of keys.
    pub fn axis(&self, plus: KeyCode, minus: KeyCode) -> f64 {
        (self.is_down(plus) as i32 - self.is_down(minus) as i32) as f64
    }

    /// The wheel turned, as the system says it: `across` and `up` in lines for a mouse's notch, or
    /// in points (`points`) for a trackpad's or a touch mouse's scroll. Adds to `scroll`.
    pub fn wheel(&mut self, across: f32, up: f32, points: bool) {
        let lines = wheel_lines(across, up, self.shift);
        self.scroll += if points { lines / POINTS_PER_LINE } else { notch_lines(lines, cfg!(target_os = "macos")) };
    }

    pub(crate) fn key(&mut self, key: KeyCode, pressed: bool) {
        if pressed {
            if self.down.insert(key) {
                self.pressed.push(key);
            }
        } else {
            self.down.remove(&key);
        }
    }

    pub(crate) fn release_all(&mut self) {
        self.down.clear();
        self.left_button = false;
        self.right_button = false;
    }

    pub(crate) fn end_frame(&mut self) {
        self.pressed.clear();
        self.mouse_delta = (0.0, 0.0);
        self.scroll = 0.0;
        self.clicked = false;
        self.controllers.iter_mut().for_each(Controller::end_frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_shift_a_scroll_across_is_the_wheel_s() {
        assert_eq!(wheel_lines(0.0, 1.0, false), 1.0);
        assert_eq!(wheel_lines(0.0, -2.0, true), -2.0);
        assert_eq!(wheel_lines(1.0, 0.0, true), 1.0, "macOS with Shift held");
        assert_eq!(wheel_lines(-3.0, 0.0, false), 0.0, "a swipe across is not the wheel");
        assert_eq!(wheel_lines(0.5, 1.0, true), 1.0);
    }

    /// A notch of a mouse's wheel is a line at least where the system gives less for a slow one.
    #[test]
    fn a_slow_notch_of_the_wheel_is_a_whole_line() {
        assert_eq!(notch_lines(0.1, true), 1.0);
        assert_eq!(notch_lines(-0.1, true), -1.0);
        assert_eq!(notch_lines(3.5, true), 3.5, "a quick spin keeps its lines");
        assert_eq!(notch_lines(0.0, true), 0.0);
        assert_eq!(notch_lines(0.25, false), 0.25, "a fine wheel's quarter notch elsewhere");
        let mut input = Input::default();
        input.wheel(0.0, 0.1, false);
        assert_eq!(input.scroll, if cfg!(target_os = "macos") { 1.0 } else { 0.1 });
        input.scroll = 0.0;
        input.wheel(0.0, -20.0, true);
        assert_eq!(input.scroll, -0.5, "a trackpad's points");
        // Shift held: macOS sends the notch across.
        (input.scroll, input.shift) = (0.0, true);
        input.wheel(-1.0, 0.0, false);
        assert_eq!(input.scroll, -1.0);
    }
}
