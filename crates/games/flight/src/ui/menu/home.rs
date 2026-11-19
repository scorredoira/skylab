//! The start and pause screen: a column on the left over the waiting flight with everything the
//! game has, each a click away: fly, the aircraft, the place, the weather and the time, the
//! settings, the manual and the replay. The keys that open them in flight still do (they are
//! written beside each). In its head, "Change vehicle" goes back to the first screen (`vehicles`).

use super::theme::{self, icon};
use super::{no_picture, Cursor, Nav, Pictures};
use crate::kinds::Craft;
use crate::settings::Sky;
use engine::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

/// What the player chose on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Fly,
    Aircraft,
    Place,
    Weather,
    Settings,
    Manual,
    Replay,
    About,
    /// The first screen again: planes, drones or cars ("Change vehicle").
    Vehicles,
}

/// What the screen says of the flight that waits behind it.
pub struct Info<'a> {
    pub craft: Craft,
    /// The picture of the place (`Pictures::place`'s key) and its name and what it is.
    pub place_picture: &'a str,
    pub place: &'a str,
    pub place_what: &'a str,
    /// The kind of day, if the sliders are one's, and the day in a line ("10:00 · wind 250° 6 kt").
    pub sky: Option<Sky>,
    pub day: &'a str,
    /// The hour, for the little sky of the weather's row.
    pub hour: f64,
    /// The game has just opened: nothing has been flown yet.
    pub fresh: bool,
}

#[derive(Default)]
pub struct Home {
    cursor: Cursor,
    /// The cursor has been put on "Fly", the first time the screen was shown.
    placed: bool,
}

/// What a row shows at its right end.
enum Aside<'a> {
    Key(&'a str),
    /// A picture (or the icon in its place), and the part of it shown: its focus and how much larger.
    Picture(Option<egui::TextureHandle>, char, (egui::Pos2, f32)),
    Sky(Option<Sky>, f64),
}

impl Home {
    /// Draws the screen, `t` of the way in (0 hidden, 1 whole); it listens only when `active`.
    pub fn show(&mut self, ctx: &egui::Context, t: f32, active: bool, moves: &[Nav], info: &Info, pictures: &mut Pictures) -> Option<Choice> {
        let screen = ctx.content_rect();
        let mut choice = None;
        let shown = theme::ease(t);
        // The sizes shrink together on a short window.
        let k = ((screen.height() - 40.0) / 760.0).clamp(0.62, 1.0);
        let width = 404.0_f32.min(screen.width() - 32.0);
        let panel = Rect::from_min_size(pos2(screen.left() + 20.0 - 36.0 * (1.0 - shown), screen.top() + 20.0), vec2(width, screen.height() - 40.0));
        // The area is the panel's own rectangle: a click beside it is still the cockpit's.
        egui::Area::new(egui::Id::unique("menu-home")).fixed_pos(panel.min).constrain(false).order(egui::Order::Middle).interactable(active).show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_rect(panel, Sense::click());
            let painter = ui.painter().clone();
            // The world darker towards the side the menu is on, so that it reads over a bright sky.
            let shade = Rect::from_min_max(screen.min, pos2(screen.left() + screen.width() * 0.6, screen.bottom()));
            theme::gradient_across(&painter, shade, 12, |x| Color32::from_black_alpha((150.0 * shown * (1.0 - x).powi(2)) as u8));

            theme::glass(&painter, panel, 18.0, theme::GLASS, shown);
            if t < 0.5 {
                return;
            }
            let pad = 20.0;
            let (left, right) = (panel.left() + pad, panel.right() - pad);
            let mut y = panel.top() + 22.0 * k;

            // The name of the game.
            let mark = Rect::from_min_size(pos2(left, y), vec2(44.0, 44.0) * k);
            painter.rect_filled(mark, 12.0 * k, theme::ACCENT_FILL);
            theme::icon(&painter, mark.center(), icon::AIRPLANE, 26.0 * k, Color32::WHITE);
            let name = theme::wide(&painter, pos2(mark.right() + 14.0, mark.top() + 1.0 * k), "SKYLAB", theme::bold(25.0 * k), theme::TEXT, 3.2);
            theme::text(&painter, pos2(mark.right() + 14.0, mark.bottom() + 1.0), Align2::LEFT_BOTTOM, "Flight simulator", theme::regular(13.0 * k.max(0.85)), theme::DIM);
            y = mark.bottom() + 20.0 * k;

            // Where the window is tall enough, the aircraft that waits, big: its picture and where it is.
            let needed = 68.0 * k + 14.0 * k + 6.0 * 78.0 * k + 44.0;
            let spare = panel.bottom() - y - needed;
            if spare >= 120.0 {
                let hero = Rect::from_min_size(pos2(left, y), vec2(right - left, (spare - 16.0).min((right - left) * 0.6)));
                match pictures.aircraft(ui.ctx(), info.craft) {
                    Some(texture) => theme::picture(&painter, hero, 14.0, &texture, Color32::WHITE),
                    None => no_picture(&painter, hero, 14.0, if matches!(info.craft, Craft::Multirotor(_)) { icon::DRONE } else { icon::AIRPLANE }),
                }
                let foot = Rect::from_min_max(pos2(hero.left(), hero.bottom() - hero.height() * 0.6), hero.max);
                theme::rounded_gradient(&painter.with_clip_rect(foot), hero, 14.0, |p| Color32::from_black_alpha((210.0 * ((p.y - foot.top()) / foot.height()).clamp(0.0, 1.0)) as u8));
                theme::text(&painter, pos2(hero.left() + 16.0, hero.bottom() - 30.0), Align2::LEFT_BOTTOM, info.craft.name(), theme::bold(22.0), Color32::WHITE);
                theme::icon(&painter, pos2(hero.left() + 23.0, hero.bottom() - 20.0), icon::PIN, 14.0, theme::ACCENT);
                theme::text_within(&painter, pos2(hero.left() + 34.0, hero.bottom() - 29.0), hero.width() - 50.0, &format!("{} · {}", info.place, info.day), theme::regular(13.0), Color32::from_gray(222));
                painter.rect_stroke(hero, 14.0, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
                y = hero.bottom() + 16.0;
            }

            // The cursor starts on "Fly", under the head's button.
            if !std::mem::replace(&mut self.placed, true) {
                self.cursor.set(1, false);
            }
            self.cursor.begin(ui.ctx(), if active { moves } else { &[] });

            // In the head, at the right: back to the first screen, for planes, drones or cars.
            self.cursor.row();
            let focus = self.cursor.item(false);
            let label = painter.layout_no_wrap("Change vehicle".to_string(), theme::medium(12.5), theme::TEXT);
            let wordy = right - name.right() >= label.size().x + 60.0;
            let size = vec2(if wordy { label.size().x + 42.0 } else { 32.0 }, 32.0);
            let change = Rect::from_min_size(pos2(right - size.x, mark.center().y - size.y / 2.0), size);
            let response = ui.interact(change, ui.scope_id().with("vehicles"), Sense::click());
            let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered() || focus.on, 0.12);
            painter.rect(change, 16.0, theme::mix(theme::CARD, theme::CARD_HOVER, lit), Stroke::new(1.0, theme::mix(theme::LINE, theme::fade(theme::ACCENT, 0.7), lit)), StrokeKind::Inside);
            theme::icon(&painter, pos2(change.left() + 16.0, change.center().y), icon::SWAP, 16.0, theme::ACCENT);
            if wordy {
                painter.galley(pos2(change.left() + 30.0, change.center().y - label.size().y / 2.0), label, theme::TEXT);
            }
            focus.show(ui, change, 16.0);
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.on_hover_text("Planes, drones or cars").clicked() || focus.activate {
                choice = Some(Choice::Vehicles);
            }

            // Fly: the one big button.
            self.cursor.row();
            let focus = self.cursor.item(false);
            let fly = Rect::from_min_size(pos2(left, y), vec2(right - left, 68.0 * k));
            let response = ui.interact(fly, ui.scope_id().with("fly"), Sense::click());
            let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered() || focus.on, 0.12);
            painter.rect_filled(fly, 14.0, theme::mix(theme::ACCENT_FILL, theme::ACCENT, lit * 0.55));
            painter.rect_filled(Rect::from_min_max(pos2(fly.left() + 14.0, fly.top()), pos2(fly.right() - 14.0, fly.top() + 1.0)), 0.0, Color32::from_white_alpha(70));
            theme::icon(&painter, pos2(fly.left() + 36.0, fly.center().y), icon::TAKEOFF, 30.0 * k, Color32::WHITE);
            theme::text(&painter, pos2(fly.left() + 68.0, fly.center().y - 10.0 * k), Align2::LEFT_CENTER, if info.fresh { "Fly" } else { "Resume flight" }, theme::bold(21.0 * k), Color32::WHITE);
            // With the aircraft's picture above saying what and where, the button says what it does.
            let about = if spare >= 120.0 { (if info.fresh { "Take the controls" } else { "Back to the cockpit" }).to_string() } else { format!("{} · {}", info.craft.name(), info.place) };
            theme::text_within(&painter, pos2(fly.left() + 68.0, fly.center().y + 3.0 * k), fly.width() - 150.0, &about, theme::regular(13.0 * k.max(0.85)), Color32::from_white_alpha(215));
            theme::key_cap(&painter, pos2(fly.right() - 16.0, fly.center().y), "Enter");
            focus.show(ui, fly, 14.0);
            if response.clicked() || focus.activate {
                choice = Some(Choice::Fly);
            }
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            y = fly.bottom() + 14.0 * k;

            let class = crate::kinds::card(info.craft).class.as_str();
            let craft_icon = if matches!(info.craft, Craft::Multirotor(_)) { icon::DRONE } else { icon::AIRPLANE };
            let rows = [
                (Choice::Aircraft, craft_icon, "Aircraft", format!("{}{}{}", info.craft.name(), if class.is_empty() { "" } else { " · " }, class), Aside::Picture(pictures.aircraft(ui.ctx(), info.craft), craft_icon, (theme::AIRCRAFT_IN_PICTURE, 1.7))),
                (Choice::Place, icon::PIN, "Place", format!("{} · {}", info.place, info.place_what), Aside::Picture(pictures.place(ui.ctx(), info.place_picture), icon::GLOBE, (pos2(0.5, 0.5), 1.0))),
                (Choice::Weather, icon::CLOUD_SUN, "Weather & time", format!("{}{}", info.sky.map_or(String::new(), |s| format!("{} · ", s.title())), info.day), Aside::Sky(info.sky, info.hour)),
                (Choice::Settings, icon::SLIDERS, "Settings", "Flight aids, graphics, replay".to_string(), Aside::Key("O")),
                (Choice::Manual, icon::BOOK, "Manual", "How to fly, with pictures".to_string(), Aside::Key("F1")),
                (Choice::Replay, icon::REPLAY, "Replay", "Watch the last minutes again".to_string(), Aside::Key("F2")),
            ];
            let height = 70.0 * k;
            for (what, glyph, title, about, aside) in rows {
                self.cursor.row();
                let focus = self.cursor.item(false);
                let row = Rect::from_min_size(pos2(left, y), vec2(right - left, height));
                let response = ui.interact(row, ui.scope_id().with(title), Sense::click());
                let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered() || focus.on, 0.12);
                painter.rect(row, 12.0, theme::mix(theme::CARD, theme::CARD_HOVER, lit), Stroke::new(1.0, theme::mix(theme::LINE, theme::fade(theme::ACCENT, 0.7), lit)), StrokeKind::Inside);
                let tile = Rect::from_center_size(pos2(row.left() + 14.0 + 21.0 * k, row.center().y), vec2(42.0, 42.0) * k);
                painter.rect_filled(tile, 11.0 * k, Color32::from_rgba_premultiplied(20, 40, 70, 150));
                theme::icon(&painter, tile.center(), glyph, 23.0 * k, theme::ACCENT);
                // The right end: a picture, the sky, or the key that opens it in flight.
                let aside_left = match aside {
                    Aside::Key(key) => theme::key_cap(&painter, pos2(row.right() - 16.0, row.center().y), key).left(),
                    Aside::Picture(texture, glyph, (focus, zoom)) => {
                        let frame = Rect::from_min_max(pos2(row.right() - 8.0 - (height - 16.0) * 1.6, row.top() + 8.0), pos2(row.right() - 8.0, row.bottom() - 8.0));
                        match texture {
                            Some(texture) => theme::picture_of(&painter, frame, 8.0, &texture, Color32::WHITE, focus, zoom),
                            None => no_picture(&painter, frame, 8.0, glyph),
                        }
                        painter.rect_stroke(frame, 8.0, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
                        frame.left()
                    }
                    Aside::Sky(sky, hour) => {
                        let frame = Rect::from_min_max(pos2(row.right() - 8.0 - (height - 16.0) * 1.6, row.top() + 8.0), pos2(row.right() - 8.0, row.bottom() - 8.0));
                        super::side::sky_art(&painter, frame, 8.0, sky, hour);
                        painter.rect_stroke(frame, 8.0, Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
                        frame.left()
                    }
                };
                let text_left = tile.right() + 13.0;
                theme::text(&painter, pos2(text_left + 2.0 * lit, row.center().y - 10.0 * k), Align2::LEFT_CENTER, title, theme::medium(17.0 * k.max(0.8)), theme::TEXT);
                theme::text_within(&painter, pos2(text_left + 2.0 * lit, row.center().y + 3.0 * k), aside_left - text_left - 10.0, &about, theme::regular(12.5 * k.max(0.88)), theme::DIM);
                focus.show(ui, row, 12.0);
                if response.clicked() || focus.activate {
                    choice = Some(what);
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                y = row.bottom() + 8.0 * k;
            }

            // The foot: how the keys work it, and the About.
            let foot = panel.bottom() - 18.0;
            let credits = theme::text(&painter, pos2(right, foot), Align2::RIGHT_BOTTOM, "About", theme::regular(12.0), theme::FAINT);
            let response = ui.interact(credits.expand(6.0), ui.scope_id().with("credits"), Sense::click());
            if response.hovered() {
                theme::text(&painter, pos2(right, foot), Align2::RIGHT_BOTTOM, "About", theme::regular(12.0), theme::ACCENT);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                choice = Some(Choice::About);
            }
            if foot - 22.0 > y {
                theme::text(&painter, pos2(left, foot), Align2::LEFT_BOTTOM, "↑ ↓  Enter  ·  Esc flies", theme::regular(12.0), theme::FAINT);
            }
        });
        choice
    }
}
