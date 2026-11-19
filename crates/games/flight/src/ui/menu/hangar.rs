//! The aircraft to choose from, those of the family flown (the planes or the drones: the first
//! screen, `vehicles`, changes it): a sheet over most of the window with a big picture of each (taken
//! by the game itself, `scripts/menu-shots.sh`), its name, what it is, a few figures, how hard it
//! is to fly and how it starts (`kinds::card`, the `card.ron` of its folder). One click on a
//! picture flies it; the arrows and Enter, or a pad, do the same.

use super::theme::{self, icon};
use super::{no_picture, Cursor, Nav, Pictures};
use crate::kinds::{self, Craft, Family};
use engine::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// What the player did on the sheet.
#[derive(Default)]
pub struct Out {
    /// The craft clicked: it is flown from now on.
    pub chosen: Option<Craft>,
    pub close: bool,
}

#[derive(Default)]
pub struct Hangar {
    cursor: Cursor,
    /// It was shown in the last frame (to know when it opens again).
    shown: bool,
}

/// How hard a craft is to fly, in a word.
fn difficulty(level: u8) -> &'static str {
    match level {
        0 => "",
        1 => "Easy",
        2 => "Gentle",
        3 => "Takes practice",
        4 => "Demanding",
        _ => "Expert",
    }
}

impl Hangar {
    /// It is not shown this frame.
    pub fn hidden(&mut self) {
        self.shown = false;
    }

    /// Draws the sheet, `t` of the way in (0 hidden, 1 whole); it listens only when `active`.
    pub fn show(&mut self, ctx: &egui::Context, t: f32, active: bool, moves: &[Nav], current: Craft, pictures: &mut Pictures) -> Out {
        let screen = ctx.content_rect();
        let mut out = Out::default();
        let shown = theme::ease(t);
        let family = current.family();
        let crafts: Vec<Craft> = family.crafts().collect();
        // Opened again: the cursor on the craft flown.
        if active && !std::mem::replace(&mut self.shown, true) {
            self.cursor.set(crafts.iter().position(|c| *c == current).unwrap_or(0), false);
        }
        egui::Area::new(egui::Id::unique("menu-hangar")).fixed_pos(screen.min).order(egui::Order::Middle).interactable(active).show(ctx, |ui| {
            ui.set_clip_rect(screen);
            let painter = ui.painter().clone();
            // The flight dimmed behind; a click on it closes the sheet.
            let behind = ui.allocate_rect(screen, Sense::click());
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha((165.0 * shown) as u8));
            let size = vec2((screen.width() - 72.0).min(1480.0), (screen.height() - 72.0).min(940.0));
            let sheet = Rect::from_center_size(screen.center() + vec2(0.0, 26.0 * (1.0 - shown)), size);
            let on_sheet = ui.interact(sheet, ui.scope_id().with("sheet"), Sense::click());
            if behind.clicked() && !on_sheet.hovered() {
                out.close = true;
            }
            theme::glass(&painter, sheet, 20.0, theme::SHEET, shown);
            if t < 0.5 {
                return;
            }
            let pad = 26.0;
            let (left, right) = (sheet.left() + pad, sheet.right() - pad);

            // The head.
            let head = sheet.top() + 38.0;
            let (glyph, words) = match family {
                Family::Planes => (icon::AIRPLANE, "Choose your plane"),
                Family::Drones => (icon::DRONE, "Choose your drone"),
            };
            theme::icon(&painter, pos2(left + 13.0, head), glyph, 26.0, theme::ACCENT);
            let title = theme::text(&painter, pos2(left + 36.0, head), Align2::LEFT_CENTER, words, theme::bold(26.0), theme::TEXT);
            if right - title.right() > 520.0 {
                theme::text(&painter, pos2(title.right() + 18.0, head + 3.0), Align2::LEFT_CENTER, "Click one to fly it: it starts on the runway of the place you are in.", theme::regular(14.0), theme::DIM);
            }
            let close = Rect::from_center_size(pos2(right - 16.0, head), vec2(34.0, 34.0));
            let response = ui.interact(close, ui.scope_id().with("close"), Sense::click());
            painter.rect_filled(close, 10.0, if response.hovered() { theme::CARD_HOVER } else { theme::CARD });
            theme::icon(&painter, close.center(), icon::CLOSE, 17.0, theme::TEXT);
            if response.on_hover_text("Close (Esc)").clicked() {
                out.close = true;
            }
            theme::key_cap(&painter, pos2(close.left() - 10.0, head), "Esc");

            // The pictures: two across, three with many aircraft, as tall as the sheet leaves them.
            let body = Rect::from_min_max(pos2(left, sheet.top() + 74.0), pos2(right, sheet.bottom() - pad));
            let columns = if crafts.len() > 4 || body.width() > body.height() * 2.6 { 3 } else if body.width() < 760.0 { 1 } else { 2 };
            let rows = crafts.len().div_ceil(columns);
            let gap = 16.0;
            let width = (body.width() - gap * (columns as f32 - 1.0)) / columns as f32;
            let height = ((body.height() - gap * (rows as f32 - 1.0)) / rows as f32).max((width * 0.42).max(250.0));
            self.cursor.begin(ui.ctx(), if active { moves } else { &[] });
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body));
            inner.set_clip_rect(body.expand(4.0).intersect(screen));
            egui::ScrollArea::vertical().id_salt("menu-hangar-scroll").auto_shrink([false, false]).show(&mut inner, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                for line in crafts.chunks(columns) {
                    self.cursor.row();
                    let (row, _) = ui.allocate_exact_size(vec2(body.width(), height), Sense::hover());
                    for (k, craft) in line.iter().enumerate() {
                        let focus = self.cursor.item(false);
                        let card = Rect::from_min_size(pos2(row.left() + (width + gap) * k as f32, row.top()), vec2(width, height));
                        let response = ui.interact(card, ui.scope_id().with(("craft", craft.key())), Sense::click());
                        let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered() || focus.on, 0.15);
                        craft_card(ui.painter(), card, *craft, *craft == current, lit, pictures.aircraft(ui.ctx(), *craft));
                        focus.show(ui, card, 16.0);
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if response.clicked() || focus.activate {
                            out.chosen = Some(*craft);
                        }
                    }
                    ui.add_space(gap);
                }
            });
        });
        out
    }
}

/// One aircraft: its picture over the whole card, and over its foot, darkened to read, its name,
/// what it is and its figures. `lit` (0 to 1) is how far the pointer or the cursor has lit it up.
fn craft_card(painter: &egui::Painter, card: Rect, craft: Craft, flown: bool, lit: f32, picture: Option<egui::TextureHandle>) {
    let radius = 16.0;
    let about = kinds::card(craft);
    let quad = matches!(craft, Craft::Multirotor(_));
    match &picture {
        // A little dimmer until it is pointed at. A card taller than the picture shows the part
        // of it the aircraft is in, across.
        Some(texture) => theme::picture_of(painter, card, radius, texture, Color32::from_gray((218.0 + 37.0 * lit) as u8), pos2(theme::AIRCRAFT_IN_PICTURE.x, 0.5), 1.0),
        None => no_picture(painter, card, radius, if quad { icon::DRONE } else { icon::AIRPLANE }),
    }
    // The foot of the picture darkened, for the text.
    let foot = Rect::from_min_max(pos2(card.left(), card.bottom() - (card.height() * 0.66).min(230.0)), card.max);
    let clip = painter.with_clip_rect(foot.intersect(painter.clip_rect()));
    theme::rounded_gradient(&clip, Rect::from_min_max(pos2(card.left(), card.top()), card.max), radius, |p| Color32::from_black_alpha((235.0 * ((p.y - foot.top()) / foot.height()).clamp(0.0, 1.0).powf(0.8)) as u8));

    let pad = 22.0;
    let (left, right) = (card.left() + pad, card.right() - pad);
    // The tags at the top: what it is, and whether it is the one flown.
    let mut x = left;
    if flown {
        x = theme::chip(painter, pos2(x, card.top() + 16.0), Some(icon::CHECK), "FLYING NOW", theme::ACCENT_FILL, true, false).right() + 8.0;
    }
    if !about.class.is_empty() {
        theme::chip(painter, pos2(x, card.top() + 16.0), None, &about.class.to_uppercase(), Color32::from_gray(236), false, false);
    }
    // How it starts, at the top right.
    let start = if quad { "STARTS ON THE GROUND · ON THE PAD IN A RACE" } else { "STARTS ON THE RUNWAY, INTO THE WIND" };
    if card.width() > 560.0 {
        theme::chip(painter, pos2(right, card.top() + 16.0), Some(icon::TAKEOFF), start, Color32::from_gray(236), false, true);
    }

    // From the bottom up: the figures, what it is good for, the line about it, its name.
    let mut y = card.bottom() - 18.0;
    let facts = &about.facts;
    if !facts.is_empty() {
        let each = ((right - left) * 0.62 / facts.len() as f32).min(132.0);
        for (k, (name, value)) in facts.iter().enumerate() {
            let at = pos2(left + each * k as f32, y);
            theme::text(painter, at, Align2::LEFT_BOTTOM, name.to_uppercase(), theme::medium(10.5), Color32::from_gray(170));
            theme::text(painter, at - vec2(0.0, 14.0), Align2::LEFT_BOTTOM, value, theme::bold(17.0), Color32::WHITE);
        }
        // How hard it is: five dots and a word, at the right.
        if about.difficulty > 0 {
            let colour = [theme::GOOD, theme::GOOD, theme::WARM, theme::WARM, theme::BAD][(about.difficulty.clamp(1, 5) - 1) as usize];
            theme::text(painter, pos2(right, y), Align2::RIGHT_BOTTOM, "DIFFICULTY", theme::medium(10.5), Color32::from_gray(170));
            for k in 0..5u8 {
                let dot = pos2(right - 5.0 - 13.0 * (4 - k) as f32, y - 23.0);
                painter.circle_filled(dot, 4.5, if k < about.difficulty { colour } else { Color32::from_white_alpha(50) });
            }
            theme::text(painter, pos2(right - 13.0 * 5.0 - 6.0, y - 16.0), Align2::RIGHT_BOTTOM, difficulty(about.difficulty), theme::medium(13.0), colour);
        }
        y -= 50.0;
        painter.hline(left..=right, y + 4.0, Stroke::new(1.0, Color32::from_white_alpha(36)));
        y -= 6.0;
    }
    // What it is good for.
    if !about.good_for.is_empty() {
        theme::icon(painter, pos2(left + 8.0, y - 8.0), icon::CHECK, 15.0, theme::ACCENT);
        theme::text_within(painter, pos2(left + 22.0, y - 16.0), right - left - 22.0, &format!("Good for: {}", about.good_for), theme::regular(12.5), Color32::from_gray(200));
        y -= 26.0;
    }
    if !about.tagline.is_empty() {
        let width = (right - left).min(620.0);
        // Laid out once to know its height, then drawn above what is below it.
        let measure = painter.layout(about.tagline.clone(), theme::regular(14.5), Color32::WHITE, width);
        let rows = measure.rows.len().min(2) as f32;
        theme::paragraph(painter, pos2(left, y - rows * 19.0), width, 2, &about.tagline, theme::regular(14.5), Color32::from_gray(236));
        y -= rows * 19.0 + 6.0;
    }
    theme::text(painter, pos2(left, y), Align2::LEFT_BOTTOM, craft.name(), theme::bold(31.0), Color32::WHITE);

    // Its edge: the accent on the one flown, lighter under the pointer.
    let edge = if flown { Stroke::new(2.0, theme::ACCENT) } else { Stroke::new(1.0 + lit, theme::mix(theme::LINE, Color32::from_white_alpha(190), lit)) };
    painter.rect_stroke(card, radius, edge, StrokeKind::Inside);
}
