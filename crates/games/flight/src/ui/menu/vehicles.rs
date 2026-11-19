//! The first screen, as the game opens: what to drive. Three pictures that fill the window, each
//! taken in the game (`scripts/menu-shots.sh`): the planes and the drones, which are this game's
//! (the aircraft to choose from are then those of the family, `hangar`), and the cars, which are
//! the driving game's: the program beside this one, started in its place (`engine::program`). On
//! the web there is no other program, and no cars. A click on a picture chooses it; the arrows and
//! Enter, or a pad, do the same. The start and pause screen opens it again ("Change vehicle").

use super::theme::{self, icon};
use super::{no_picture, Cursor, Nav, Pictures};
use crate::kinds::Family;
use engine::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// The driving game's program, beside this one (`crates/games/drive`).
pub const DRIVE: &str = "skylab-drive";

/// What the player chose on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A family of this game's: its aircraft are shown next.
    Fly(Family),
    /// The driving game.
    Cars,
    /// Back to the start and pause screen, with the craft flown.
    Close,
}

/// One of the three: what its picture is, what is written over it.
struct Card {
    choice: Choice,
    /// The picture's name (`Pictures::vehicle`).
    picture: &'static str,
    glyph: char,
    name: &'static str,
    /// What there is of it, by name.
    fleet: String,
    line: &'static str,
    /// What choosing it does, in a word.
    action: &'static str,
}

#[derive(Default)]
pub struct Vehicles {
    cursor: Cursor,
    /// It was shown in the last frame (to know when it opens again).
    shown: bool,
    /// Why the cars did not start, written on their picture.
    pub error: Option<String>,
}

/// The names of some things in a line: "Cessna 172 · Extra 300L".
fn names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names.collect::<Vec<_>>().join(" · ")
}

/// The cards the screen shows: the planes, the drones and, with `cars`, the cars.
fn cards(cars: bool) -> Vec<Card> {
    let mut cards = vec![
        Card {
            choice: Choice::Fly(Family::Planes),
            picture: "planes",
            glyph: icon::AIRPLANE,
            name: "Planes",
            fleet: names(Family::Planes.crafts().map(|c| c.name())),
            line: "Light aircraft with working cockpits, from any airfield in the world.",
            action: "Fly",
        },
        Card {
            choice: Choice::Fly(Family::Drones),
            picture: "drones",
            glyph: icon::DRONE,
            name: "Drones",
            fleet: names(Family::Drones.crafts().map(|c| c.name())),
            line: "Quads to fly anywhere, or to race through the gates of a course.",
            action: "Fly",
        },
    ];
    if cars {
        cards.push(Card {
            choice: Choice::Cars,
            picture: "cars",
            glyph: icon::CAR,
            name: "Cars",
            fleet: names(sim_core::CarKind::all().map(|c| c.name()).chain(sim_core::circuit::file::circuits().iter().map(|c| c.name.as_str()))),
            line: "A circuit to learn and laps to beat, alone or against rivals.",
            action: "Drive",
        });
    }
    cards
}

/// Where the cards go in `body`: side by side, or one above the other in a window taller than wide.
/// Returns their rectangles and whether they are side by side.
fn layout(body: Rect, count: usize, gap: f32) -> (Vec<Rect>, bool) {
    let n = count.max(1) as f32;
    let across = body.width() >= 660.0 && body.width() >= body.height() * 0.9;
    let rects = (0..count)
        .map(|k| {
            if across {
                let width = (body.width() - gap * (n - 1.0)) / n;
                Rect::from_min_size(pos2(body.left() + (width + gap) * k as f32, body.top()), vec2(width, body.height()))
            } else {
                let height = (body.height() - gap * (n - 1.0)) / n;
                Rect::from_min_size(pos2(body.left(), body.top() + (height + gap) * k as f32), vec2(body.width(), height))
            }
        })
        .collect();
    (rects, across)
}

impl Vehicles {
    /// It is not shown this frame.
    pub fn hidden(&mut self) {
        self.shown = false;
    }

    /// Draws the screen, `t` of the way in (0 hidden, 1 whole); it listens only when `active`.
    /// `flown` is the family of the craft that waits (the cursor starts on it), and `cars` whether
    /// the driving game can be offered.
    pub fn show(&mut self, ctx: &egui::Context, t: f32, active: bool, moves: &[Nav], flown: Family, cars: bool, pictures: &mut Pictures) -> Option<Choice> {
        let screen = ctx.content_rect();
        let mut choice = None;
        let shown = theme::ease(t);
        let cards = cards(cars);
        // Opened again: the cursor on the family flown.
        if active && !std::mem::replace(&mut self.shown, true) {
            self.cursor.set(cards.iter().position(|c| c.choice == Choice::Fly(flown)).unwrap_or(0), false);
        }
        egui::Area::new(egui::Id::unique("menu-vehicles")).fixed_pos(screen.min).order(egui::Order::Middle).interactable(active).show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            // The whole window is the screen's: nothing of the flight shows through.
            theme::gradient_down(&painter, screen, theme::fade(Color32::from_rgb(16, 21, 31), shown), theme::fade(Color32::from_rgb(7, 9, 13), shown));
            if t < 0.5 {
                return;
            }
            // The sizes shrink together in a small window.
            let k = (screen.height() / 820.0).min(screen.width() / 1100.0).clamp(0.6, 1.0);
            let pad = (28.0 * k).round();
            let (left, right) = (screen.left() + pad, screen.right() - pad);

            // The head: the name of the game, what the screen asks, and the way on without choosing
            // (the start screen, with the craft flown last).
            let mark = Rect::from_min_size(pos2(left, screen.top() + pad * 0.8), vec2(44.0, 44.0) * k);
            painter.rect_filled(mark, 12.0 * k, theme::ACCENT_FILL);
            theme::icon(&painter, mark.center(), icon::AIRPLANE, 26.0 * k, Color32::WHITE);
            let name = theme::wide(&painter, pos2(mark.right() + 14.0 * k, mark.top() + 1.0 * k), "SKYLAB", theme::bold(25.0 * k), theme::TEXT, 3.2);
            theme::text(&painter, pos2(mark.right() + 14.0 * k, mark.bottom() + 1.0), Align2::LEFT_BOTTOM, "Simulator", theme::regular(13.0 * k.max(0.85)), theme::DIM);
            let close = theme::text(&painter, pos2(right, mark.center().y), Align2::RIGHT_CENTER, "Start screen", theme::regular(13.0), theme::DIM);
            let cap = theme::key_cap(&painter, pos2(close.left() - 8.0, mark.center().y), "Esc");
            let response = ui.interact(cap.union(close).expand(6.0), ui.scope_id().with("close"), Sense::click());
            if response.hovered() {
                theme::text(&painter, pos2(right, mark.center().y), Align2::RIGHT_CENTER, "Start screen", theme::regular(13.0), theme::ACCENT);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                choice = Some(Choice::Close);
            }
            let question = "Choose what to fly or drive";
            let room = cap.left() - name.right() - 60.0;
            if room > 300.0 {
                theme::text(&painter, pos2((name.right() + cap.left()) / 2.0, mark.center().y), Align2::CENTER_CENTER, question, theme::medium(19.0 * k.max(0.8)), theme::TEXT);
            }

            // The pictures: all of the window under the head.
            let gap = (16.0 * k).round();
            let body = Rect::from_min_max(pos2(left, mark.bottom() + pad * 0.7), pos2(right, screen.bottom() - pad));
            let (rects, across) = layout(body, cards.len(), gap);
            self.cursor.begin(ui.ctx(), if active { moves } else { &[] });
            // The cards are of one size: the line about each has the room the longest takes.
            let rows = rects.first().map_or(1, |rect| cards.iter().map(|card| Type::of(*rect).rows(&painter, card)).max().unwrap_or(1));
            for (card, rect) in cards.iter().zip(rects) {
                // Side by side they are one row; one above the other, a row each.
                if !across {
                    self.cursor.row();
                }
                let focus = self.cursor.item(false);
                let response = ui.interact(rect, ui.scope_id().with(card.picture), Sense::click());
                let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered() || focus.on, 0.18);
                let error = if card.choice == Choice::Cars { self.error.as_deref() } else { None };
                draw(&painter, rect, card, lit, rows, error, pictures.vehicle(ui.ctx(), card.picture));
                focus.show(ui, rect, 18.0);
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() || focus.activate {
                    choice = Some(card.choice);
                }
            }
        });
        choice
    }
}

/// How a card of this size is written on: the size of its text against the full one, whether it
/// is wider than tall (what is written is then down its left side, not over its foot), the room
/// its text has across, and the line about it: its font and the height of a row.
struct Type {
    s: f32,
    wide: bool,
    pad: f32,
    width: f32,
    font: egui::FontId,
    row: f32,
}

impl Type {
    fn of(rect: Rect) -> Type {
        let s = (rect.width() / 400.0).min(rect.height() / 300.0).clamp(0.62, 1.0);
        let wide = rect.width() > rect.height() * 1.4;
        let pad = (26.0 * s).round();
        let room = rect.width() - 2.0 * pad;
        let width = if wide { (rect.width() * 0.42).max(210.0).min(room) } else { room.min(440.0) };
        let font = theme::regular((15.5 * s).max(12.5));
        Type { s, wide, pad, width, row: font.size * 1.3, font }
    }

    /// The rows the line about a card takes, two at most.
    fn rows(&self, painter: &egui::Painter, card: &Card) -> usize {
        painter.layout(card.line.to_string(), self.font.clone(), Color32::WHITE, self.width).rows.len().clamp(1, 2)
    }
}

/// Half of a card darkened: from nothing along its middle to black of opacity `strength` (0 to
/// 255) at its foot, or at its left side if `wide`. The half keeps the card's round corners.
fn shade(painter: &egui::Painter, rect: Rect, radius: f32, wide: bool, strength: f32) {
    let centre = rect.center();
    // The card's outline goes round from its top left corner, nine points a corner: the half's is
    // the two corners of its side, between the ends of the middle line.
    let outline = theme::rounded_outline(rect, radius);
    let mut ring = Vec::with_capacity(20);
    if wide {
        ring.push(pos2(centre.x, rect.bottom()));
        ring.extend_from_slice(&outline[27..]);
        ring.extend_from_slice(&outline[..9]);
        ring.push(pos2(centre.x, rect.top()));
    } else {
        ring.push(pos2(rect.right(), centre.y));
        ring.extend_from_slice(&outline[18..]);
        ring.push(pos2(rect.left(), centre.y));
    }
    // How far out a point is from the middle line, 0 to 1: the darkness is even across the
    // triangles, which all start at the centre.
    let out = |p: egui::Pos2| if wide { (centre.x - p.x) / (rect.width() / 2.0) } else { (p.y - centre.y) / (rect.height() / 2.0) };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, Color32::TRANSPARENT);
    for p in &ring {
        mesh.colored_vertex(*p, Color32::from_black_alpha((strength * out(*p).clamp(0.0, 1.0)) as u8));
    }
    for k in 1..ring.len() as u32 {
        mesh.add_triangle(0, k, k + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// One card: its picture over all of it, and over its foot, darkened to read, what it is. `lit`
/// (0 to 1) is how far the pointer or the cursor has lit it up: the picture comes a little nearer
/// and brighter, and what choosing it does shows in the accent. `rows` is the room left for the
/// line about it, the same in every card so that their names stand level.
fn draw(painter: &egui::Painter, rect: Rect, card: &Card, lit: f32, rows: usize, error: Option<&str>, picture: Option<egui::TextureHandle>) {
    let radius = 18.0;
    let Type { s, wide, pad, width, font, row } = Type::of(rect);
    match &picture {
        // The subject is in the middle of its picture.
        Some(texture) => theme::picture_of(painter, rect, radius, texture, Color32::from_gray((205.0 + 50.0 * lit) as u8), pos2(0.5, 0.5), 1.0 + 0.05 * lit),
        None => no_picture(painter, rect, radius, card.glyph),
    }
    // Half the picture darkened, for the text: its lower half, or its left one if it is wider
    // than tall; laid twice, so that it deepens faster towards the edge.
    for _ in 0..2 {
        shade(painter, rect, radius, wide, 200.0);
    }

    let left = rect.left() + pad;
    // What it is, as a badge at the top.
    let badge = Rect::from_min_size(rect.min + vec2(pad, pad) * 0.8, vec2(46.0, 46.0) * s);
    painter.rect(badge, 13.0 * s, theme::mix(Color32::from_black_alpha(170), theme::ACCENT_FILL, lit), Stroke::new(1.0, Color32::from_white_alpha(40)), StrokeKind::Inside);
    theme::icon(painter, badge.center(), card.glyph, 27.0 * s, Color32::WHITE);

    // From the bottom up: what choosing it does (or why it could not be done), the line about it,
    // its name, what there is of it.
    let mut y = rect.bottom() - pad;
    if let Some(error) = error {
        let measure = painter.layout(error.to_string(), theme::regular(12.5), theme::BAD, width);
        let lines = measure.rows.len().min(3) as f32;
        theme::paragraph(painter, pos2(left, y - lines * 16.0), width, 3, error, theme::regular(12.5), theme::BAD);
        y -= lines * 16.0 + 10.0;
    } else if rect.height() >= 250.0 {
        let go = painter.layout_no_wrap(card.action.to_string(), theme::bold(16.0 * s.max(0.85)), Color32::WHITE);
        let pill = Rect::from_min_max(pos2(left, y - 40.0 * s), pos2(left + go.size().x + 62.0 * s, y));
        painter.rect(pill, pill.height() / 2.0, theme::mix(Color32::from_white_alpha(26), theme::ACCENT_FILL, lit), Stroke::new(1.0, Color32::from_white_alpha((70.0 * (1.0 - lit)) as u8)), StrokeKind::Inside);
        painter.galley(pos2(pill.left() + 18.0 * s, pill.center().y - go.size().y / 2.0), go, Color32::WHITE);
        theme::icon(painter, pos2(pill.right() - 20.0 * s + 3.0 * lit, pill.center().y), icon::ARROW_RIGHT, 17.0 * s, Color32::WHITE);
        y = pill.top() - 16.0 * s;
    }
    y -= rows as f32 * row;
    theme::paragraph(painter, pos2(left, y), width, 2, card.line, font, Color32::from_gray(228));
    y -= 4.0 * s;
    let title = theme::text(painter, pos2(left, y), Align2::LEFT_BOTTOM, card.name, theme::bold(46.0 * s), Color32::WHITE);
    // What there is of it: above its name, or beside it in a card too low for both.
    let fleet = card.fleet.to_uppercase();
    let colour = theme::mix(Color32::from_gray(205), theme::ACCENT, lit);
    if title.top() - 14.0 > badge.bottom() + 6.0 {
        theme::text_within(painter, pos2(left + 1.0, title.top() - 12.0), width, &fleet, theme::medium(11.5), colour);
    } else if left + width - title.right() > 120.0 {
        theme::text_within(painter, pos2(title.right() + 16.0, title.bottom() - 15.0 - 11.0 * s), left + width - title.right() - 16.0, &fleet, theme::medium(11.5), colour);
    }

    // Its edge: lighter under the pointer.
    painter.rect_stroke(rect, radius, Stroke::new(1.0 + lit, theme::mix(theme::LINE, Color32::from_white_alpha(200), lit)), StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three, in their order, each with its picture in `assets/menu/vehicles`; without the
    /// driving game, the two that are flown.
    #[test]
    fn the_cards_are_the_planes_the_drones_and_the_cars_each_with_its_picture() {
        let all = cards(true);
        assert_eq!(all.iter().map(|c| c.choice).collect::<Vec<_>>(), [Choice::Fly(Family::Planes), Choice::Fly(Family::Drones), Choice::Cars]);
        assert_eq!(all[0].fleet, "Cessna 172 · Extra 300L · Kestrel tilt-rotor");
        assert!(all[2].fleet.contains("Lakeside Park"), "{}", all[2].fleet);
        for card in &all {
            let bytes = crate::fleet::asset(&format!("menu/vehicles/{}.jpg", card.picture)).unwrap_or_else(|e| panic!("{e}"));
            let (w, h, _) = engine::texture::decode_bytes(&bytes).unwrap_or_else(|e| panic!("{}: {e:?}", card.picture));
            assert!(w >= 800 && h >= 800, "{}: {w}x{h}", card.picture);
        }
        assert_eq!(cards(false).iter().map(|c| c.name).collect::<Vec<_>>(), ["Planes", "Drones"]);
    }

    /// Side by side in a window wider than tall, one above the other in a narrow one; they fill
    /// the room they are given, with the gap between them.
    #[test]
    fn the_cards_fill_the_window_side_by_side_or_one_above_the_other() {
        let body = Rect::from_min_size(pos2(20.0, 80.0), vec2(1400.0, 800.0));
        let (rects, across) = layout(body, 3, 16.0);
        assert!(across);
        assert_eq!((rects[0].left(), rects[2].right(), rects[0].height()), (20.0, 1420.0, 800.0));
        assert!((rects[1].left() - rects[0].right() - 16.0).abs() < 1e-3 && (rects[0].width() - rects[2].width()).abs() < 1e-3);
        let body = Rect::from_min_size(pos2(20.0, 80.0), vec2(500.0, 800.0));
        let (rects, across) = layout(body, 3, 16.0);
        assert!(!across);
        assert_eq!((rects[0].top(), rects[2].bottom(), rects[1].width()), (80.0, 880.0, 500.0));
        // Two cards, on the web: half the width each.
        let (rects, _) = layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(1016.0, 600.0)), 2, 16.0);
        assert_eq!(rects[1].left(), 516.0);
    }
}
