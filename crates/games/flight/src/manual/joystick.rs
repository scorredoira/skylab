//! The T.16000M drawn as a diagram, with what each of its controls does by default: the buttons'
//! numbers and their jobs are read from `controls`' defaults, so the drawing follows them.

use super::inputs::JOYSTICK;
use crate::controls::{Action, ButtonRef, Mapping};
use engine::egui::{self, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke};

/// The size of the drawing, in points.
pub const SIZE: egui::Vec2 = vec2(760.0, 400.0);

const BODY: Color32 = Color32::from_gray(58);
const EDGE: Color32 = Color32::from_gray(120);
const BUTTON: Color32 = Color32::from_rgb(255, 204, 0);
const TEXT: Color32 = Color32::from_gray(225);

/// What a button does, briefly.
fn job(action: Action) -> &'static str {
    match action {
        Action::ThrottleUp => "more throttle",
        Action::ThrottleDown => "less throttle",
        Action::TrimNoseUp => "trim up",
        Action::TrimNoseDown => "trim down",
        Action::FlapsDown => "flaps down",
        Action::FlapsUp => "flaps up",
        Action::LookLeft => "look left",
        Action::LookRight => "look right",
        Action::View => "cockpit or outside",
        Action::Closer => "zoom in",
        Action::Further => "zoom out",
        Action::PanelView => "normal ↔ close",
        Action::PanelLights => "panel lights",
        Action::Centre => "centre",
        Action::Pause => "pause",
        Action::Autopilot => "AP",
        Action::Heading => "HDG",
        Action::Nav => "NAV",
        Action::Approach => "APR",
        Action::Altitude => "ALT",
        Action::VerticalSpeed => "VS",
        Action::LevelChange => "FLC",
        Action::NoseUp => "nose up",
        Action::NoseDown => "nose down",
        Action::HeadingBugLeft => "heading bug ←",
        Action::HeadingBugRight => "heading bug →",
        Action::DirectTo => "D→",
        Action::Arm => "arm (a multirotor)",
        Action::TimeSlower => "time slower",
        Action::TimeFaster => "time faster",
    }
}

/// The numbered buttons of the default mapping and what each does, by number.
pub fn buttons() -> Vec<(u32, Action)> {
    let mapping = Mapping::default_for(JOYSTICK, false);
    let mut buttons: Vec<(u32, Action)> = mapping
        .buttons
        .iter()
        .flat_map(|(action, refs)| refs.iter().filter_map(move |b| if let ButtonRef::Number(n) = b { Some((*n, *action)) } else { None }))
        .collect();
    buttons.sort();
    buttons
}

/// Where a button of the stick's head is, from the head's middle: the trigger in front, 2 under the
/// hat, 3 and 4 left and right of it.
fn on_head(number: u32) -> Option<egui::Vec2> {
    match number {
        1 => Some(vec2(-58.0, 52.0)),
        2 => Some(vec2(0.0, 30.0)),
        3 => Some(vec2(-32.0, 0.0)),
        4 => Some(vec2(32.0, 0.0)),
        _ => None,
    }
}

/// Draws it in the next `SIZE` of `ui`.
pub fn draw(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(SIZE, Sense::hover());
    let p = ui.painter_at(rect);
    let at = |x: f32, y: f32| rect.min + vec2(x, y);
    let font = FontId::proportional(15.0);
    let line = Stroke::new(1.2, Color32::from_gray(170));

    // The base, its lever on the left and its buttons on the right.
    p.rect(Rect::from_min_max(at(20.0, 280.0), at(320.0, 340.0)), 14.0, BODY, Stroke::new(1.5, EDGE), egui::StrokeKind::Inside);
    let lever = Rect::from_min_max(at(36.0, 236.0), at(56.0, 300.0));
    p.rect(lever, 6.0, Color32::from_gray(80), Stroke::new(1.5, EDGE), egui::StrokeKind::Inside);
    let base_buttons: Vec<Pos2> = (0..6).map(|k| at(222.0 + (k % 3) as f32 * 26.0, 297.0 + (k / 3) as f32 * 24.0)).collect();
    for &b in &base_buttons {
        p.circle(b, 7.0, Color32::from_gray(30), Stroke::new(1.5, BUTTON));
    }
    // The column and the grip, which turns for the pedals.
    p.rect_filled(Rect::from_min_max(at(150.0, 230.0), at(170.0, 282.0)), 3.0, Color32::from_gray(45));
    p.rect(Rect::from_min_max(at(126.0, 120.0), at(194.0, 240.0)), 22.0, BODY, Stroke::new(1.5, EDGE), egui::StrokeKind::Inside);
    let twist = at(160.0, 200.0);
    let arc: Vec<Pos2> = (0..=16).map(|k| std::f32::consts::PI * (0.15 + 0.7 * k as f32 / 16.0)).map(|a| twist + vec2(-a.cos() * 52.0, a.sin() * 14.0)).collect();
    p.add(Shape::line(arc.clone(), Stroke::new(2.0, BUTTON)));
    for (end, before) in [(arc[0], arc[1]), (arc[16], arc[15])] {
        let d = (end - before).normalized();
        let n = vec2(-d.y, d.x);
        p.add(Shape::convex_polygon(vec![end + d * 7.0, end + n * 5.0, end - n * 5.0], BUTTON, Stroke::NONE));
    }
    // The head: the hat and the buttons round it.
    let head = at(160.0, 80.0);
    p.rect(Rect::from_center_size(head + vec2(0.0, 8.0), vec2(116.0, 84.0)), 30.0, BODY, Stroke::new(1.5, EDGE), egui::StrokeKind::Inside);
    p.circle(head, 12.0, Color32::from_gray(30), Stroke::new(1.5, BUTTON));
    for d in [vec2(0.0, -1.0), vec2(0.0, 1.0), vec2(-1.0, 0.0), vec2(1.0, 0.0)] {
        let n = vec2(-d.y, d.x);
        p.add(Shape::convex_polygon(vec![head + d * 9.0, head + d * 4.0 + n * 3.5, head + d * 4.0 - n * 3.5], BUTTON, Stroke::NONE));
    }

    // What each does, in a column on the right, from the top of the drawing down so that the
    // lines to them do not cross.
    let buttons = buttons();
    let mut labels: Vec<(String, Pos2)> = Vec::new();
    for &(n, action) in buttons.iter().filter(|(n, _)| on_head(*n).is_some()) {
        let spot = head + on_head(n).unwrap_or_default();
        p.circle(spot, 8.0, Color32::from_gray(30), Stroke::new(1.5, BUTTON));
        p.text(spot, Align2::CENTER_CENTER, n.to_string(), FontId::proportional(10.0), TEXT);
        labels.push((format!("{n}{}: {}", if n == 1 { " (trigger)" } else { "" }, job(action)), spot + vec2(8.0, 0.0)));
    }
    labels.push(("hat ↕: trim · ↔: look".to_string(), head + vec2(9.0, -9.0)));
    labels.push(("forward and back, to the sides: yoke".to_string(), at(194.0, 160.0)));
    labels.push(("twist the stick: pedals".to_string(), twist + vec2(52.0, -4.0)));
    labels.push(("lever on the base: throttle".to_string(), lever.right_center()));
    labels.push(("buttons on the base:".to_string(), base_buttons[2] + vec2(7.0, 0.0)));
    labels.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    let column = 360.0;
    let mut y = 24.0;
    for (text, target) in &labels {
        let anchor = at(column, y);
        p.text(anchor, Align2::LEFT_CENTER, text, font.clone(), TEXT);
        p.line_segment([anchor - vec2(8.0, 0.0), *target], line);
        y += 29.0;
    }
    // The base's buttons in two columns under the last.
    let base = buttons.iter().filter(|(n, _)| on_head(*n).is_none()).map(|&(n, action)| format!("{n} {}", job(action)));
    for (k, text) in base.enumerate() {
        let (x, row) = (column + 16.0 + (k % 2) as f32 * 190.0, y - 3.0 + (k / 2) as f32 * 24.0);
        p.text(at(x, row), Align2::LEFT_CENTER, text, FontId::proportional(14.0), TEXT);
    }
}
