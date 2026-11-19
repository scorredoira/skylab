//! Text drawn over the view: status at the top left, tags of what flies for the player at the top
//! right, a message in the middle, the help in a column on the right, and (a setting) the yoke box
//! and the pedal bar at the bottom left.
//!
//! Everything is in logical points with the origin at the top left of `screen`.

use engine::egui::{pos2, text::LayoutJob, vec2, Align, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, TextFormat};

/// Side of the yoke box.
const BOX: f32 = 96.0;
/// How far the dots travel from the centre.
const TRAVEL: f32 = BOX / 2.0 - 6.0;

/// What the overlay shows this frame.
pub struct Hud<'a> {
    pub status: &'a str,
    pub message: &'a str,
    /// Empty hides the help column.
    pub help: &'a str,
    /// Where the controls are, -1..1: aileron +1 right, elevator +1 back, rudder +1 right.
    pub aileron: f32,
    pub elevator: f32,
    pub rudder: f32,
    /// Whether to draw the yoke box and the pedal bar (a setting, off by default).
    pub controls: bool,
    /// Small tags at the top right: what flies for the player (`Session::annunciators`).
    pub tags: &'a [String],
    /// How far down the tags start, below anything already at the top right.
    pub tags_top: f32,
    /// How far in from the right edge they are: beside the settings' panel while it is open over
    /// the flight, which goes on (a faster time's tag must stay in sight).
    pub tags_right: f32,
}

/// The square that shows where the yoke is.
pub fn yoke_box(screen: Rect) -> Rect {
    Rect::from_min_size(pos2(screen.left() + 16.0, screen.bottom() - 36.0 - BOX), vec2(BOX, BOX))
}

/// The bar under it that shows where the pedals are.
pub fn pedal_bar(screen: Rect) -> Rect {
    Rect::from_min_size(pos2(screen.left() + 16.0, screen.bottom() - 16.0 - 14.0), vec2(BOX, 14.0))
}

/// True if a click at `pos` is on the yoke box or the pedal bar (it centres the controls).
pub fn on_controls(screen: Rect, pos: Pos2) -> bool {
    yoke_box(screen).expand(6.0).contains(pos) || pedal_bar(screen).expand2(vec2(6.0, 5.0)).contains(pos)
}

/// The help column.
fn help_box(screen: Rect) -> Rect {
    let size = vec2((screen.width() * 0.4).min(410.0), (screen.height() * 0.62).min(520.0));
    Rect::from_min_size(pos2(screen.right() - size.x - 14.0, screen.top() + 64.0), size)
}

/// White text with a dark shadow so it reads over any background.
fn shadowed(painter: &Painter, at: Pos2, job: LayoutJob) {
    let galley = painter.layout_job(job);
    for offset in [vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0), vec2(0.0, 2.0)] {
        painter.galley_with_override_text_color(at + offset, galley.clone(), Color32::from_black_alpha(150));
    }
    painter.galley(at, galley, Color32::WHITE);
}

fn job(text: &str, font: FontId, wrap: f32, align: Align) -> LayoutJob {
    let mut job = LayoutJob::single_section(text.to_string(), TextFormat { font_id: font, color: Color32::WHITE, ..Default::default() });
    job.wrap.max_width = wrap.max(1.0);
    job.halign = align;
    job
}

pub fn draw(painter: &Painter, screen: Rect, hud: &Hud) {
    let painter = &painter.with_clip_rect(screen);

    if hud.controls {
        draw_controls(painter, screen, hud);
    }
    draw_tags(painter, screen, hud);

    if !hud.status.is_empty() {
        let at = pos2(screen.left() + 14.0, screen.top() + 12.0);
        shadowed(painter, at, job(hud.status, FontId::monospace(13.0), screen.width() - 28.0, Align::LEFT));
    }

    if !hud.message.is_empty() {
        // Centred text is laid out around its x position.
        let at = pos2(screen.center().x, screen.top() + screen.height() * 0.08);
        shadowed(painter, at, job(hud.message, FontId::proportional(20.0), screen.width() * 0.8, Align::Center));
    }

    if !hud.help.is_empty() {
        let frame = help_box(screen);
        let mut galley = painter.layout_job(job(hud.help, FontId::monospace(12.0), frame.width() - 28.0, Align::LEFT));
        // Taller than the screen leaves it: a smaller letter, down to 9 points.
        let room = screen.bottom() - frame.top() - 32.0;
        if galley.size().y > room {
            let size = (12.0 * room / galley.size().y).max(9.0);
            galley = painter.layout_job(job(hud.help, FontId::monospace(size), frame.width() - 28.0, Align::LEFT));
        }
        // The box is as tall as in the original, or as tall as the text needs with this font.
        let frame = Rect::from_min_size(frame.min, vec2(frame.width(), frame.height().max(galley.size().y + 24.0)));
        painter.rect_filled(frame, 8.0, Color32::from_black_alpha(158));
        painter.galley(frame.min + vec2(14.0, 12.0), galley, Color32::WHITE);
    }
}

/// A key or a knob of a cockpit's panel to name: the one under the pointer, or a knob being
/// turned. Its edge on the screen (drawn if `lit`: a knob's; a key is not lit, as the real ones
/// are not) and its middle, its name with what it is set to, and what is said under it (or nothing).
pub struct Pointed {
    pub lit: bool,
    pub outline: Vec<Pos2>,
    /// The edge of the whole key or knob (a dual knob's ring): the tag goes beside it, not over it.
    pub whole: Vec<Pos2>,
    pub at: Pos2,
    pub name: String,
    pub hint: String,
}

/// Where the tag of something on the screen (`bounds`) goes, `size` points: beside it on the
/// right, on the left if it does not fit there, and inside the screen wherever the thing is.
fn tag_place(screen: Rect, bounds: Rect, size: engine::egui::Vec2) -> Rect {
    let room = screen.shrink(8.0);
    let left = if bounds.right() + 10.0 + size.x <= room.right() { bounds.right() + 10.0 } else { bounds.left() - 10.0 - size.x };
    let min = pos2(left.clamp(room.left(), (room.right() - size.x).max(room.left())), (bounds.center().y - size.y / 2.0).clamp(room.top(), (room.bottom() - size.y).max(room.top())));
    Rect::from_min_size(min, size)
}

/// Lights the edge of a knob of the panel and writes a key's or a knob's tag beside it; out of the
/// picture (the C172's ALT knob from the seat), the tag is at the screen's edge with an arrow.
pub fn draw_pointed(painter: &Painter, screen: Rect, pointed: &Pointed) {
    let painter = &painter.with_clip_rect(screen);
    let light = Color32::from_rgb(120, 220, 255);
    if pointed.lit {
        painter.add(engine::egui::Shape::convex_polygon(pointed.outline.clone(), Color32::from_white_alpha(26), Stroke::new(1.5, light)));
    }
    let arrow = match pointed.at {
        at if at.y > screen.bottom() => " ↓",
        at if at.y < screen.top() => " ↑",
        at if at.x < screen.left() => " ←",
        at if at.x > screen.right() => " →",
        _ => "",
    };
    let name = painter.layout_job(job(&format!("{}{arrow}", pointed.name), FontId::monospace(13.0), screen.width() * 0.5, Align::LEFT));
    let hint = (!pointed.hint.is_empty()).then(|| painter.layout_job(job(&pointed.hint, FontId::monospace(11.0), screen.width() * 0.5, Align::LEFT)));
    let text = vec2(name.size().x.max(hint.as_ref().map_or(0.0, |h| h.size().x)), name.size().y + hint.as_ref().map_or(0.0, |h| h.size().y + 2.0));
    let bounds = Rect::from_points(&pointed.whole);
    let frame = tag_place(screen, bounds, text + vec2(16.0, 10.0));
    painter.rect(frame, 5.0, Color32::from_rgba_unmultiplied(10, 14, 20, 240), Stroke::new(1.0, light.gamma_multiply(0.7)), StrokeKind::Inside);
    let top = frame.min + vec2(8.0, 5.0);
    let below = top + vec2(0.0, name.size().y + 2.0);
    painter.galley(top, name, Color32::WHITE);
    if let Some(hint) = hint {
        painter.galley_with_override_text_color(below, hint, Color32::from_gray(190));
    }
}

/// Where the yoke and the pedals are: a dot in a square and a dot on a bar.
fn draw_controls(painter: &Painter, screen: Rect, hud: &Hud) {
    let frame = Stroke::new(1.0, Color32::from_white_alpha(128));
    let dot = Color32::from_rgb(255, 191, 26);
    let yoke = yoke_box(screen);
    painter.rect(yoke, 4.0, Color32::from_black_alpha(115), frame, StrokeKind::Inside);
    let mark = Stroke::new(1.0, Color32::from_white_alpha(64));
    painter.hline(yoke.x_range(), yoke.center().y, mark);
    painter.vline(yoke.center().x, yoke.y_range(), mark);
    painter.circle_filled(yoke.center() + vec2(hud.aileron, hud.elevator) * TRAVEL, 5.0, dot);
    let bar = pedal_bar(screen);
    painter.rect(bar, 4.0, Color32::from_black_alpha(115), frame, StrokeKind::Inside);
    painter.circle_filled(bar.center() + vec2(hud.rudder * TRAVEL, 0.0), 5.0, dot);
}

/// The tags at the top right, one under the other: green text on a dark rounded box.
fn draw_tags(painter: &Painter, screen: Rect, hud: &Hud) {
    let mut top = screen.top() + hud.tags_top;
    for tag in hud.tags {
        let galley = painter.layout_job(job(tag, FontId::monospace(13.0), screen.width() * 0.5, Align::LEFT));
        let size = galley.size() + vec2(16.0, 8.0);
        let frame = Rect::from_min_size(pos2(screen.right() - hud.tags_right - size.x, top), size);
        painter.rect(frame, 5.0, Color32::from_black_alpha(150), Stroke::new(1.0, Color32::from_rgb(70, 200, 110)), StrokeKind::Inside);
        painter.galley(frame.min + vec2(8.0, 4.0), galley, Color32::from_rgb(110, 235, 140));
        top += size.y + 6.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag is beside what it names, on the right while it fits there, and always in the screen:
    /// at its edge when what it names is out of the picture.
    #[test]
    fn a_tag_stays_in_the_screen_beside_what_it_names() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 1000.0));
        let size = vec2(160.0, 40.0);
        let knob = |x: f32, y: f32| Rect::from_center_size(pos2(x, y), vec2(40.0, 40.0));
        let tag = tag_place(screen, knob(400.0, 500.0), size);
        assert_eq!((tag.left(), tag.center().y, tag.size()), (430.0, 500.0, size));
        let tag = tag_place(screen, knob(1500.0, 500.0), size);
        assert_eq!(tag.right(), 1470.0, "on the left near the right edge");
        // The ALT knob below the picture: the tag at the bottom edge, beside where it is.
        let tag = tag_place(screen, knob(400.0, 1040.0), size);
        assert_eq!((tag.left(), tag.bottom()), (430.0, 992.0));
        for (x, y) in [(-300.0, -300.0), (2000.0, 500.0), (800.0, 3000.0), (5.0, 5.0)] {
            let tag = tag_place(screen, knob(x, y), size);
            assert!(screen.contains_rect(tag), "{x}, {y}: {tag:?}");
        }
    }
}
