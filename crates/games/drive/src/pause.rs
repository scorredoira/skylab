//! The pause menu (Esc, a pad's Start): the game waits, and the player drives on, goes back to
//! Skylab's first screen to choose another vehicle (the flight simulator is the program beside this
//! one: `engine::program`), or quits. The arrows and Enter work it, a pad's direction pad or left
//! stick and A, or the mouse; Esc or B drives on.

use engine::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind};
use engine::{Input, KeyCode, PadAxis, PadButton};

/// The flight simulator's program, beside this one, and what opens it on its first screen.
pub const SKYLAB: &str = "skylab";
pub const FIRST_SCREEN: [&str; 2] = ["--menu", "vehicles"];

/// What the player chose in the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Resume,
    /// Skylab's first screen: planes, drones or cars.
    Vehicles,
    Quit,
}

impl Choice {
    fn title(self) -> &'static str {
        match self {
            Choice::Resume => "Drive on",
            Choice::Vehicles => "Change vehicle",
            Choice::Quit => "Quit",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Choice::Resume => "Back to the car",
            Choice::Vehicles => "Planes, drones or cars",
            Choice::Quit => "Close the game",
        }
    }
}

pub struct Menu {
    items: Vec<Choice>,
    at: usize,
    /// Where the pads' sticks were last frame, up (1), down (-1) or in the middle: a push moves once.
    stick: i8,
    /// Why Skylab did not start, written under the rows.
    pub error: Option<String>,
}

impl Menu {
    /// The menu, with "Change vehicle" if `skylab` (the flight simulator is beside this program).
    pub fn new(skylab: bool) -> Menu {
        let items = [Choice::Resume, Choice::Vehicles, Choice::Quit].into_iter().filter(|c| skylab || *c != Choice::Vehicles).collect();
        Menu { items, at: 0, stick: 0, error: None }
    }

    fn step(&mut self, down: bool) {
        self.at = if down { (self.at + 1).min(self.items.len() - 1) } else { self.at.saturating_sub(1) };
    }

    /// The keys and the pads' buttons of this frame: what was chosen, if anything was.
    pub fn keys(&mut self, input: &Input) -> Option<Choice> {
        let mut chosen = None;
        for key in &input.pressed {
            match key {
                KeyCode::ArrowUp | KeyCode::KeyW => self.step(false),
                KeyCode::ArrowDown | KeyCode::KeyS => self.step(true),
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => chosen = Some(self.items[self.at]),
                KeyCode::Escape => chosen = Some(Choice::Resume),
                _ => {}
            }
        }
        let mut stick = 0;
        for pad in &input.controllers {
            if pad.pressed(PadButton::DPadUp) {
                self.step(false);
            }
            if pad.pressed(PadButton::DPadDown) {
                self.step(true);
            }
            if pad.pressed(PadButton::South) {
                chosen = Some(self.items[self.at]);
            }
            if pad.pressed(PadButton::East) || pad.pressed(PadButton::Start) {
                chosen = Some(Choice::Resume);
            }
            // Only a gamepad's stick: a wheel's axes are not a direction.
            if pad.standard {
                let y = pad.axis(PadAxis::LeftStickY);
                stick = if y > 0.6 { 1 } else if y < -0.6 { -1 } else { stick };
            }
        }
        if stick != self.stick && stick != 0 {
            self.step(stick < 0);
        }
        self.stick = stick;
        chosen
    }

    /// Draws the menu over the picture; a row clicked is chosen.
    pub fn show(&mut self, ctx: &egui::Context, circuit: &str) -> Option<Choice> {
        let screen = ctx.content_rect();
        let mut chosen = None;
        let text = Color32::from_rgb(240, 243, 248);
        let dim = Color32::from_rgb(164, 173, 189);
        egui::Area::new(egui::Id::unique("pause-menu")).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(150));
            let row = 64.0;
            let width = 380.0_f32.min(screen.width() - 32.0);
            let height = 92.0 + (row + 8.0) * self.items.len() as f32 + if self.error.is_some() { 58.0 } else { 14.0 };
            let panel = Rect::from_center_size(screen.center(), vec2(width, height));
            painter.rect(panel, 18.0, Color32::from_rgba_premultiplied(12, 15, 22, 247), Stroke::new(1.0, Color32::from_white_alpha(26)), StrokeKind::Inside);
            let (left, right) = (panel.left() + 20.0, panel.right() - 20.0);
            painter.text(pos2(left, panel.top() + 20.0), Align2::LEFT_TOP, "Paused", FontId::proportional(26.0), text);
            painter.text(pos2(left, panel.top() + 54.0), Align2::LEFT_TOP, circuit, FontId::proportional(14.0), dim);
            let mut y = panel.top() + 92.0;
            for (k, item) in self.items.clone().into_iter().enumerate() {
                let rect = Rect::from_min_max(pos2(left, y), pos2(right, y + row));
                let response = ui.interact(rect, ui.scope_id().with(("pause", k)), Sense::click());
                // The pointer over a row puts the cursor on it.
                if response.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                    self.at = k;
                }
                let on = self.at == k;
                let fill = if on { Color32::from_rgb(34, 116, 224) } else { Color32::from_white_alpha(14) };
                painter.rect(rect, 12.0, fill, Stroke::new(1.0, Color32::from_white_alpha(if on { 70 } else { 22 })), StrokeKind::Inside);
                painter.text(pos2(rect.left() + 18.0, rect.center().y - 10.0), Align2::LEFT_CENTER, item.title(), FontId::proportional(18.0), text);
                painter.text(pos2(rect.left() + 18.0, rect.center().y + 12.0), Align2::LEFT_CENTER, item.about(), FontId::proportional(13.0), if on { Color32::from_white_alpha(215) } else { dim });
                if on {
                    painter.text(pos2(rect.right() - 18.0, rect.center().y), Align2::RIGHT_CENTER, "Enter", FontId::proportional(12.0), Color32::from_white_alpha(200));
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    chosen = Some(item);
                }
                y += row + 8.0;
            }
            if let Some(error) = &self.error {
                let bad = Color32::from_rgb(255, 128, 104);
                let galley = painter.layout(error.clone(), FontId::proportional(12.5), bad, right - left);
                painter.galley(pos2(left, y + 2.0), galley, bad);
            }
        });
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(menu: &mut Menu, keys: &[KeyCode]) -> Option<Choice> {
        let mut input = Input::default();
        input.pressed = keys.to_vec();
        menu.keys(&input)
    }

    #[test]
    fn the_keys_move_in_the_menu_and_choose() {
        let mut menu = Menu::new(true);
        // Enter on the first row drives on; so does Esc, wherever the cursor is.
        assert_eq!(press(&mut menu, &[KeyCode::Enter]), Some(Choice::Resume));
        assert_eq!(press(&mut menu, &[KeyCode::ArrowDown]), None);
        assert_eq!(press(&mut menu, &[KeyCode::Enter]), Some(Choice::Vehicles));
        assert_eq!(press(&mut menu, &[KeyCode::Escape]), Some(Choice::Resume));
        // Down stops at the last row, up at the first.
        assert_eq!(press(&mut menu, &[KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::ArrowDown, KeyCode::Enter]), Some(Choice::Quit));
        assert_eq!(press(&mut menu, &[KeyCode::ArrowUp, KeyCode::ArrowUp, KeyCode::ArrowUp, KeyCode::Space]), Some(Choice::Resume));
    }

    #[test]
    fn without_skylab_beside_it_there_is_no_other_vehicle_to_change_to() {
        let mut menu = Menu::new(false);
        assert_eq!(press(&mut menu, &[KeyCode::ArrowDown, KeyCode::Enter]), Some(Choice::Quit));
    }

    #[test]
    fn a_pad_works_the_menu() {
        let mut menu = Menu::new(true);
        let mut pad = engine::Controller::new(1, "Pad", true);
        pad.set_button(0, Some(PadButton::DPadDown), Some(true), None);
        pad.set_button(1, Some(PadButton::South), Some(true), None);
        let mut input = Input::default();
        input.controllers = vec![pad];
        assert_eq!(menu.keys(&input), Some(Choice::Vehicles));
        // The stick pushed down moves once, however long it is held; B drives on.
        let mut pad = engine::Controller::new(1, "Pad", true);
        pad.set_axis(1, Some(PadAxis::LeftStickY), -0.9);
        input.controllers = vec![pad.clone()];
        assert_eq!(menu.keys(&input), None);
        assert_eq!(menu.keys(&input), None);
        pad.set_button(2, Some(PadButton::South), Some(true), None);
        input.controllers = vec![pad];
        assert_eq!(menu.keys(&input), Some(Choice::Quit));
    }
}
