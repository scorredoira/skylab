//! The settings (key O): a panel down the right side of the window, the paused world still in view
//! on the left, so that what is changed of the day (the hour, the clouds, the haze) is seen at
//! once. Four sections: the weather and the time, the flight's aids, the graphics, and who the
//! data and the fonts come from. Every control shows what it means: the hour is a sky with its
//! sun, the wind a compass rose, the clouds a little sky that fills up.
//!
//! A change is applied at once and written to the settings file (`settings::Store`), as before.

use super::theme::{self, icon};
use super::{no_picture, Cursor, Nav, Pictures};
use crate::kinds::Craft;
use crate::render::Preset;
use crate::settings::{self, Conditions, Setting, Sky, Store};
use engine::egui::{self, pos2, vec2, Align2, Color32, Painter, Pos2, Rect, Sense, Stroke, StrokeKind};

/// The width of the panel, points.
pub const WIDTH: f32 = 460.0;

/// A section of the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tab {
    #[default]
    Weather,
    Flight,
    Graphics,
    About,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Weather, Tab::Flight, Tab::Graphics, Tab::About];

    fn title(self) -> &'static str {
        match self {
            Tab::Weather => "Weather",
            Tab::Flight => "Flight",
            Tab::Graphics => "Graphics",
            Tab::About => "About",
        }
    }

    fn icon(self) -> char {
        match self {
            Tab::Weather => icon::CLOUD_SUN,
            Tab::Flight => icon::YOKE,
            Tab::Graphics => icon::MONITOR,
            Tab::About => icon::INFO,
        }
    }

    /// The section a word names (`--menu weather`).
    pub fn named(word: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.title().eq_ignore_ascii_case(word))
    }
}

/// What the panel says of the flight, at its top.
pub struct Now<'a> {
    pub craft: Craft,
    pub place_picture: &'a str,
    pub place: &'a str,
    /// How the loading of a place goes, or why it failed.
    pub status: Option<&'a str>,
    /// It was opened from the start and pause screen: closing it goes back there.
    pub from_home: bool,
    pub assist: bool,
}

/// What the player asked for in the panel, besides what it changed itself.
#[derive(Default)]
pub struct Out {
    /// The keyboard assist was switched.
    pub assist: Option<bool>,
    pub close: bool,
    pub aircraft: bool,
    pub place: bool,
    pub manual: bool,
    pub replay: bool,
}

#[derive(Default)]
pub struct Side {
    pub tab: Tab,
    cursor: Cursor,
}

/// The settings' key of the choice to show the start screen when the game opens; on unless taken away.
pub const HOME_AT_START: &str = "menu_at_start";

impl Side {
    /// Shows a section the next time the panel is drawn.
    pub fn turn_to(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.cursor.reset();
        }
    }

    /// Draws the panel, `t` of the way in (0 hidden, 1 whole); it listens only when `active`.
    #[allow(clippy::too_many_arguments)]
    pub fn show(&mut self, ctx: &egui::Context, t: f32, active: bool, moves: &[Nav], c: &mut Conditions, quality: &mut Preset, now: &Now, store: &mut Store, pictures: &mut Pictures) -> Out {
        let screen = ctx.content_rect();
        let mut out = Out::default();
        let shown = theme::ease(t);
        for nav in moves {
            let at = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
            match nav {
                Nav::NextTab => self.turn_to(Tab::ALL[(at + 1) % Tab::ALL.len()]),
                Nav::PrevTab => self.turn_to(Tab::ALL[(at + Tab::ALL.len() - 1) % Tab::ALL.len()]),
                _ => {}
            }
        }
        let width = WIDTH.min(screen.width() - 24.0);
        let panel = Rect::from_min_size(pos2(screen.right() - 14.0 - width + 48.0 * (1.0 - shown), screen.top() + 14.0), vec2(width, screen.height() - 28.0));
        // The area is the panel's own rectangle: it takes the clicks on it, so that none goes through
        // to the cockpit behind, and a click beside it is still the cockpit's.
        egui::Area::new(egui::Id::unique("menu-side")).fixed_pos(panel.min).constrain(false).order(egui::Order::Middle).interactable(active).show(ctx, |ui| {
            ui.set_clip_rect(screen);
            ui.allocate_rect(panel, Sense::click());
            let painter = ui.painter().clone();
            theme::glass(&painter, panel, 18.0, theme::GLASS, shown);
            if t < 0.5 {
                return;
            }
            let pad = 20.0;
            let (left, right) = (panel.left() + pad, panel.right() - pad);
            self.cursor.begin(ui.ctx(), if active { moves } else { &[] });

            // The head: the title and how to leave.
            let head = panel.top() + 30.0;
            theme::text(&painter, pos2(left, head), Align2::LEFT_CENTER, "Settings", theme::bold(23.0), theme::TEXT);
            let close = Rect::from_center_size(pos2(right - 15.0, head), vec2(32.0, 32.0));
            let response = ui.interact(close, ui.scope_id().with("close"), Sense::click());
            painter.rect_filled(close, 9.0, if response.hovered() { theme::CARD_HOVER } else { theme::CARD });
            theme::icon(&painter, close.center(), icon::CLOSE, 16.0, theme::TEXT);
            if response.on_hover_text(if now.from_home { "Back to the menu (Esc)" } else { "Back to the flight (Esc)" }).clicked() {
                out.close = true;
            }
            theme::key_cap(&painter, pos2(close.left() - 10.0, head), "Esc");

            // What is flown and where, each a way to its screen.
            let top = panel.top() + 60.0;
            let half = (right - left - 10.0) / 2.0;
            let craft_icon = if matches!(now.craft, Craft::Multirotor(_)) { icon::DRONE } else { icon::AIRPLANE };
            self.cursor.row();
            // The aircraft's frame shows the aircraft, which is not in the middle of its picture.
            let tiles = [
                ("AIRCRAFT", now.craft.name().to_string(), pictures.aircraft(ui.ctx(), now.craft), craft_icon, left, (theme::AIRCRAFT_IN_PICTURE, 1.7)),
                ("PLACE", now.place.to_string(), pictures.place(ui.ctx(), now.place_picture), icon::GLOBE, left + half + 10.0, (pos2(0.5, 0.5), 1.0)),
            ];
            for (k, (label, name, texture, glyph, x, (part, zoom))) in tiles.into_iter().enumerate() {
                let focus = self.cursor.item(false);
                let tile = Rect::from_min_size(pos2(x, top), vec2(half, 66.0));
                let response = ui.interact(tile, ui.scope_id().with(label), Sense::click());
                theme::card(&painter, tile, 12.0, response.hovered() || focus.on, false);
                let frame = Rect::from_min_size(tile.min + vec2(6.0, 6.0), vec2(82.0, 54.0));
                match texture {
                    Some(texture) => theme::picture_of(&painter, frame, 8.0, &texture, Color32::WHITE, part, zoom),
                    None => no_picture(&painter, frame, 8.0, glyph),
                }
                theme::text(&painter, pos2(frame.right() + 10.0, tile.top() + 17.0), Align2::LEFT_CENTER, label, theme::medium(10.5), theme::FAINT);
                theme::text_within(&painter, pos2(frame.right() + 10.0, tile.top() + 25.0), tile.right() - frame.right() - 16.0, &name, theme::medium(14.5), theme::TEXT);
                theme::text(&painter, pos2(frame.right() + 10.0, tile.bottom() - 13.0), Align2::LEFT_CENTER, "Change ›", theme::regular(12.0), theme::ACCENT);
                focus.show(ui, tile, 12.0);
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() || focus.activate {
                    if k == 0 {
                        out.aircraft = true;
                    } else {
                        out.place = true;
                    }
                }
            }
            let mut y = top + 66.0 + 8.0;
            if let Some(status) = now.status {
                theme::text_within(&painter, pos2(left, y), right - left, status, theme::regular(12.5), theme::WARM);
                y += 20.0;
            }

            // The sections.
            y += 4.0;
            let bar = Rect::from_min_size(pos2(left, y), vec2(right - left, 40.0));
            painter.rect_filled(bar, 11.0, Color32::from_rgba_premultiplied(0, 0, 0, 80));
            let each = bar.width() / Tab::ALL.len() as f32;
            for (k, tab) in Tab::ALL.into_iter().enumerate() {
                let cell = Rect::from_min_size(pos2(bar.left() + each * k as f32, bar.top()), vec2(each, bar.height())).shrink(3.0);
                let response = ui.interact(cell, ui.scope_id().with(("tab", k)), Sense::click());
                let chosen = self.tab == tab;
                if chosen {
                    painter.rect_filled(cell, 9.0, theme::ACCENT_FILL);
                } else if response.hovered() {
                    painter.rect_filled(cell, 9.0, theme::CARD_HOVER);
                }
                let colour = if chosen { Color32::WHITE } else { theme::DIM };
                let label = painter.layout_no_wrap(tab.title().to_string(), theme::medium(13.5), colour);
                let whole = 18.0 + 6.0 + label.size().x;
                let x = cell.center().x - whole / 2.0;
                theme::icon(&painter, pos2(x + 9.0, cell.center().y), tab.icon(), 17.0, colour);
                painter.galley(pos2(x + 24.0, cell.center().y - label.size().y / 2.0), label, colour);
                if response.clicked() {
                    self.turn_to(tab);
                }
            }
            y = bar.bottom() + 12.0;

            // The section shown, scrolled if the window is short.
            let body = Rect::from_min_max(pos2(left, y), pos2(right, panel.bottom() - 14.0));
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body));
            inner.set_clip_rect(body.expand2(vec2(6.0, 0.0)).intersect(screen));
            let tab = self.tab;
            egui::ScrollArea::vertical().id_salt(("menu-side-scroll", tab)).auto_shrink([false, false]).show(&mut inner, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                ui.set_width(body.width());
                let cursor = &mut self.cursor;
                match tab {
                    Tab::Weather => weather(ui, cursor, c, store),
                    Tab::Flight => flight(ui, cursor, now, store, &mut out),
                    Tab::Graphics => graphics(ui, cursor, quality, store),
                    Tab::About => about(ui),
                }
                ui.add_space(22.0);
            });
            // What is below fades into the panel's foot: there is more to scroll to.
            let foot = Rect::from_min_max(pos2(panel.left() + 6.0, panel.bottom() - 40.0), pos2(panel.right() - 6.0, panel.bottom() - 14.0));
            theme::gradient_down(&painter, foot, Color32::TRANSPARENT, theme::GLASS);
            painter.rect_filled(Rect::from_min_max(pos2(foot.left(), foot.bottom()), pos2(foot.right(), panel.bottom() - 6.0)), 0.0, theme::GLASS);
        });
        out
    }
}

// ---------------------------------------------------------------------------------------------
// The sections

/// The weather and the time.
fn weather(ui: &mut egui::Ui, cursor: &mut Cursor, c: &mut Conditions, store: &mut Store) {
    section(ui, icon::CLOUD_SUN, "Kind of day");
    let now = Sky::current(c);
    let width = ui.available_width();
    let (gap, columns) = (8.0, 3);
    let size = vec2((width - gap * (columns as f32 - 1.0)) / columns as f32, 82.0);
    for line in Sky::ALL.chunks(columns) {
        cursor.row();
        let (row, _) = ui.allocate_exact_size(vec2(width, size.y), Sense::hover());
        for (k, sky) in line.iter().enumerate() {
            let focus = cursor.item(false);
            let card = Rect::from_min_size(pos2(row.left() + (size.x + gap) * k as f32, row.top()), size);
            let response = ui.interact(card, ui.scope_id().with(("sky", sky.name())), Sense::click());
            let painter = ui.painter();
            let lit = ui.ctx().animate_bool_with_time(response.id, response.hovered() || focus.on, 0.12);
            sky_art(painter, card, 11.0, Some(*sky), 13.0);
            // The name over the foot of the picture, darkened to read.
            let foot = Rect::from_min_max(pos2(card.left(), card.bottom() - 30.0), card.max);
            theme::rounded_gradient(painter, foot, 0.0, |p| Color32::from_black_alpha((150.0 * ((p.y - foot.top()) / foot.height()).clamp(0.0, 1.0)) as u8));
            theme::text(painter, pos2(card.left() + 10.0, card.bottom() - 8.0), Align2::LEFT_BOTTOM, sky.title(), theme::medium(14.0), Color32::WHITE);
            let chosen = now == Some(*sky);
            let edge = if chosen { Stroke::new(2.0, theme::ACCENT) } else { Stroke::new(1.0, theme::mix(theme::LINE, Color32::from_white_alpha(120), lit)) };
            painter.rect_stroke(card, 11.0, edge, StrokeKind::Inside);
            if chosen {
                painter.circle_filled(card.right_top() + vec2(-14.0, 14.0), 9.0, theme::ACCENT_FILL);
                theme::icon(painter, card.right_top() + vec2(-14.0, 14.0), icon::CHECK, 12.0, Color32::WHITE);
            }
            focus.show(ui, card, 11.0);
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.on_hover_text(sky.summary()).clicked() || focus.activate {
                sky.apply(c, Some(store));
            }
        }
        ui.add_space(gap);
    }

    ui.add_space(6.0);
    time_of_day(ui, cursor, c, store);
    stored_toggle(ui, cursor, store, settings::LOCAL_TIME, true, icon::CLOCK, "Start at the current time", "The hour of your computer's clock when the game opens, wherever you fly. Off, the hour set above.");
    ui.add_space(10.0);
    wind(ui, cursor, c, store);

    section(ui, icon::CLOUD, "Sky");
    slider(ui, cursor, Setting::CloudCover, c, store, None);
    slider(ui, cursor, Setting::CloudBase, c, store, None);
    slider(ui, cursor, Setting::Visibility, c, store, Some(&|t| theme::mix(Color32::from_rgb(214, 208, 190), theme::ACCENT, t)));

    section(ui, icon::THERMOMETER, "Air");
    slider(ui, cursor, Setting::Temperature, c, store, Some(&temperature_colour));
    slider(ui, cursor, Setting::Pressure, c, store, None);
    slider(ui, cursor, Setting::Humidity, c, store, None);
    ui.add_space(6.0);
    note(ui, "Going back to the runway (key 1) lines you up on the end into the wind.");
}

/// The flight's aids, the replay and the manual.
fn flight(ui: &mut egui::Ui, cursor: &mut Cursor, now: &Now, store: &mut Store, out: &mut Out) {
    section(ui, icon::YOKE, "Flying");
    if toggle(ui, cursor, icon::KEYBOARD, "Keyboard assist", "Holds pitch, bank and the heading on the runway, for flying with the keys. Key G.", now.assist) {
        out.assist = Some(!now.assist);
    }
    stored_toggle(ui, cursor, store, settings::FUEL_BURNS, false, icon::FUEL, "Burn fuel", "Off, the tanks never go down.");
    stored_toggle(ui, cursor, store, settings::CONTROLS_SHOWN, false, icon::STICK, "Show where the controls are", "A box for the yoke and a bar for the pedals, at the bottom left.");
    stored_toggle(ui, cursor, store, settings::YOKES_SHOWN, true, icon::YOKE, "Show the yokes in the cockpit", "Without them the panel behind shows whole: the ALT knob, the switches. They still fly.");
    stored_toggle(ui, cursor, store, settings::PROPELLER_SHOWN, true, icon::AIRPLANE, "Show the propeller from the cockpit", "Its blades standing, its blur turning. Outside it always shows.");
    stored_toggle(ui, cursor, store, settings::LOOK_NATURAL, false, icon::MONITOR, "Natural mouse look", "Dragging takes the scene with the pointer: down looks up, right looks left.");

    section(ui, icon::REPLAY, "Replay");
    let (least, most) = crate::replay::MINUTES_RANGE;
    let minutes = store.number(crate::replay::MINUTES_KEY).unwrap_or(crate::replay::DEFAULT_MINUTES).clamp(least, most);
    let look = Look { title: "Minutes kept", value: format!("{minutes:.0} min"), t: ((minutes - least) / (most - least)) as f32, colour: None };
    let art = |p: &Painter, r: Rect| theme::icon(p, r.center(), icon::CLOCK, 22.0, theme::ACCENT);
    if let Some(change) = slider_row(ui, cursor, "replay-minutes", &look, &art) {
        let to = match change {
            Change::To(t) => least + (most - least) * t as f64,
            Change::Step(n) => minutes + n as f64,
        };
        store.set(crate::replay::MINUTES_KEY, to.round().clamp(least, most));
    }
    ui.add_space(8.0);
    cursor.row();
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let half = (row.width() - 8.0) / 2.0;
    if button(ui, cursor, Rect::from_min_size(row.min, vec2(half, 44.0)), icon::PLAY, "Watch replay", "F2") {
        out.replay = true;
    }
    if button(ui, cursor, Rect::from_min_size(pos2(row.left() + half + 8.0, row.top()), vec2(half, 44.0)), icon::BOOK, "Manual", "F1") {
        out.manual = true;
    }

    section(ui, icon::HOME, "When the game opens");
    stored_toggle(ui, cursor, store, HOME_AT_START, true, icon::HOME, "Start on the menu", "Off, the game opens straight in the cockpit. Esc shows the menu at any time.");
    stored_toggle(ui, cursor, store, crate::places::PICKER_AT_START, false, icon::PIN, "Start on the place picker", "Choose where to fly every time the game opens.");
}

/// The graphics.
fn graphics(ui: &mut egui::Ui, cursor: &mut Cursor, quality: &mut Preset, store: &mut Store) {
    section(ui, icon::MONITOR, "Quality");
    for preset in Preset::ALL {
        let (name, what, load) = match preset {
            Preset::Ultra => ("Ultra", "For pictures and video: the clouds worked out at every pixel. Not for flying on most machines.", 4),
            Preset::High => ("High", "Everything, as the game is meant to look: full clouds, trees, grass, reflections in the sea.", 3),
            Preset::Medium => ("Medium", "Lighter clouds and shadows, trees and buildings nearer. For a laptop.", 2),
            Preset::Low => ("Low", "The lightest: simple clouds and sea, little detail on the ground. For an older machine.", 1),
        };
        cursor.row();
        let focus = cursor.item(false);
        let (card, response) = ui.allocate_exact_size(vec2(ui.available_width(), 74.0), Sense::click());
        let painter = ui.painter();
        let chosen = *quality == preset;
        theme::card(painter, card, 12.0, response.hovered() || focus.on, chosen);
        // The mark of the one chosen.
        let mark = pos2(card.left() + 24.0, card.center().y);
        if chosen {
            painter.circle_filled(mark, 10.0, theme::ACCENT_FILL);
            theme::icon(painter, mark, icon::CHECK, 13.0, Color32::WHITE);
        } else {
            painter.circle_stroke(mark, 9.5, Stroke::new(1.5, theme::FAINT));
        }
        // What it costs: bars, as a signal's.
        let bars = pos2(card.right() - 62.0, card.center().y + 9.0);
        for k in 0..4 {
            let tall = 8.0 + 5.0 * k as f32;
            let bar = Rect::from_min_max(pos2(bars.x + 11.0 * k as f32, bars.y - tall), pos2(bars.x + 11.0 * k as f32 + 7.0, bars.y));
            let colour = if k < load { [theme::GOOD, theme::GOOD, theme::WARM, theme::BAD][load as usize - 1] } else { Color32::from_white_alpha(28) };
            painter.rect_filled(bar, 2.0, colour);
        }
        theme::text(painter, pos2(bars.x + 20.0, bars.y + 5.0), Align2::CENTER_TOP, "GPU LOAD", theme::medium(9.0), theme::FAINT);
        theme::text(painter, pos2(card.left() + 46.0, card.top() + 20.0), Align2::LEFT_CENTER, name, theme::bold(16.5), theme::TEXT);
        theme::paragraph(painter, pos2(card.left() + 46.0, card.top() + 31.0), card.width() - 46.0 - 82.0, 2, what, theme::regular(12.5), theme::DIM);
        focus.show(ui, card, 12.0);
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if (response.clicked() || focus.activate) && !chosen {
            *quality = preset;
            store.set("quality", preset.name());
        }
        ui.add_space(8.0);
    }
    note(ui, "A quality is applied in the background: the flight goes on meanwhile.");

    section(ui, icon::GAUGE, "Display");
    stored_toggle(ui, cursor, store, settings::FPS_SHOWN, false, icon::GAUGE, "Show frames per second", "At the end of the status line. Key F3 (on a Mac, fn+F3).");
}

/// What the game is and who made it.
fn about(ui: &mut egui::Ui) {
    section(ui, icon::AIRPLANE, "Skylab");
    note(ui, &format!("Version {}. A flight simulator to fly in, and to train AI pilots with.", env!("CARGO_PKG_VERSION")));
    ui.add_space(4.0);
    credit_row(ui, "Created by", "Santiago Corredoira");
    credit_row(ui, "Source", "github.com/scorredoira/skylab");
    ui.add_space(4.0);
    note(ui, "Real places are downloaded the first time they are flown (20 to 50 MB each) and kept on this computer for flying offline.");
}

// ---------------------------------------------------------------------------------------------
// The pieces

/// The name of a group of controls, with its icon.
fn section(ui: &mut egui::Ui, glyph: char, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
    let painter = ui.painter();
    theme::icon(painter, pos2(rect.left() + 9.0, rect.bottom() - 15.0), glyph, 16.0, theme::ACCENT);
    let job = egui::text::LayoutJob::single_section(title.to_uppercase(), egui::TextFormat { font_id: theme::bold(11.5), color: theme::DIM, extra_letter_spacing: 1.4, ..Default::default() });
    let galley = painter.layout_job(job);
    let at = pos2(rect.left() + 24.0, rect.bottom() - 15.0 - galley.size().y / 2.0);
    let end = at.x + galley.size().x + 10.0;
    painter.galley(at, galley, theme::DIM);
    painter.hline(end..=rect.right(), rect.bottom() - 15.0, Stroke::new(1.0, theme::LINE));
}

/// A line of small print.
fn note(ui: &mut egui::Ui, text: &str) {
    let width = ui.available_width();
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: theme::regular(12.5), color: theme::FAINT, ..Default::default() });
    job.wrap.max_width = width;
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 4.0), Sense::hover());
    ui.painter().galley(rect.min, galley, theme::FAINT);
}

/// What a credit is for, and whom it names.
fn credit_row(ui: &mut egui::Ui, what: &str, who: &str) {
    let width = ui.available_width();
    let mut job = egui::text::LayoutJob::single_section(who.to_string(), egui::TextFormat { font_id: theme::regular(12.5), color: theme::DIM, ..Default::default() });
    job.wrap.max_width = width - 146.0;
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 10.0), Sense::hover());
    theme::text(ui.painter(), rect.min + vec2(0.0, 1.0), Align2::LEFT_TOP, what, theme::medium(12.5), theme::TEXT);
    ui.painter().galley(rect.min + vec2(146.0, 1.0), galley, theme::DIM);
}

/// A switch with its icon, its name and what it does. Returns true when it is clicked.
fn toggle(ui: &mut egui::Ui, cursor: &mut Cursor, glyph: char, title: &str, what: &str, on: bool) -> bool {
    cursor.row();
    let focus = cursor.item(false);
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
    let painter = ui.painter();
    theme::card(painter, row, 12.0, response.hovered() || focus.on, false);
    let tile = Rect::from_center_size(pos2(row.left() + 30.0, row.center().y), vec2(36.0, 36.0));
    painter.rect_filled(tile, 10.0, Color32::from_rgba_premultiplied(20, 40, 70, 150));
    theme::icon(painter, tile.center(), glyph, 20.0, theme::ACCENT);
    theme::text(painter, pos2(row.left() + 60.0, row.top() + 19.0), Align2::LEFT_CENTER, title, theme::medium(15.0), theme::TEXT);
    theme::paragraph(painter, pos2(row.left() + 60.0, row.top() + 29.0), row.width() - 60.0 - 64.0, 2, what, theme::regular(12.0), theme::DIM);
    // The switch: its knob slides and its track lights up.
    let slid = ui.ctx().animate_bool_with_time(response.id.with("switch"), on, 0.14);
    let track = Rect::from_center_size(pos2(row.right() - 36.0, row.center().y), vec2(42.0, 24.0));
    painter.rect_filled(track, 12.0, theme::mix(Color32::from_white_alpha(36), theme::ACCENT_FILL, slid));
    painter.circle_filled(pos2(track.left() + 12.0 + 18.0 * slid, track.center().y), 9.0, Color32::WHITE);
    focus.show(ui, row, 12.0);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.add_space(8.0);
    response.clicked() || focus.activate
}

/// A switch kept in the settings file under `key` (`default` until the player changes it).
#[allow(clippy::too_many_arguments)]
fn stored_toggle(ui: &mut egui::Ui, cursor: &mut Cursor, store: &mut Store, key: &str, default: bool, glyph: char, title: &str, what: &str) {
    let on = store.flag(key).unwrap_or(default);
    if toggle(ui, cursor, glyph, title, what, on) {
        store.set(key, !on);
    }
}

/// A button with an icon and the key that does the same in flight.
fn button(ui: &mut egui::Ui, cursor: &mut Cursor, rect: Rect, glyph: char, label: &str, key: &str) -> bool {
    let focus = cursor.item(false);
    let response = ui.interact(rect, ui.scope_id().with(label), Sense::click());
    let painter = ui.painter();
    theme::card(painter, rect, 11.0, response.hovered() || focus.on, false);
    theme::icon(painter, pos2(rect.left() + 22.0, rect.center().y), glyph, 18.0, theme::ACCENT);
    theme::text(painter, pos2(rect.left() + 40.0, rect.center().y), Align2::LEFT_CENTER, label, theme::medium(14.5), theme::TEXT);
    theme::key_cap(painter, pos2(rect.right() - 12.0, rect.center().y), key);
    focus.show(ui, rect, 11.0);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked() || focus.activate
}

/// What a slider shows: its name, its value as text and as a share of its travel, and the colour
/// of its track along its length if it has one of its own (the accent otherwise).
struct Look<'a> {
    title: &'a str,
    value: String,
    t: f32,
    colour: Option<&'a dyn Fn(f32) -> Color32>,
}

/// What the player did to a slider: dragged it to a share of its travel, or stepped it with the keys.
enum Change {
    To(f32),
    Step(i32),
}

/// A slider in a row: a little picture of what it sets on the left (`art` draws it), its name and
/// its value above the track.
fn slider_row(ui: &mut egui::Ui, cursor: &mut Cursor, id: &str, look: &Look, art: &dyn Fn(&Painter, Rect)) -> Option<Change> {
    cursor.row();
    let focus = cursor.item(true);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
    let response = ui.interact(row, ui.scope_id().with(id), Sense::click_and_drag());
    let painter = ui.painter();
    let frame = Rect::from_min_size(pos2(row.left(), row.top() + 5.0), vec2(48.0, 40.0));
    painter.rect(frame, 9.0, Color32::from_rgba_premultiplied(0, 0, 0, 70), Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
    art(&painter.with_clip_rect(frame.shrink(1.0)), frame);
    let (from, to) = (frame.right() + 14.0, row.right() - 2.0);
    theme::text(painter, pos2(from, row.top() + 15.0), Align2::LEFT_CENTER, look.title, theme::regular(13.5), theme::DIM);
    theme::text(painter, pos2(to, row.top() + 15.0), Align2::RIGHT_CENTER, &look.value, theme::medium(14.5), theme::TEXT);
    let track = Rect::from_min_max(pos2(from, row.top() + 33.0), pos2(to, row.top() + 39.0));
    let knob = track_and_knob(painter, track, look.t, look.colour, response.hovered() || response.dragged() || focus.on);
    focus.show(ui, Rect::from_min_max(pos2(from - 6.0, row.top() + 2.0), pos2(to + 4.0, row.bottom() - 2.0)), 8.0);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let _ = knob;
    if response.is_pointer_button_down_on() {
        if let Some(at) = response.interact_pointer_pos() {
            return Some(Change::To(((at.x - track.left()) / track.width()).clamp(0.0, 1.0)));
        }
    }
    (focus.step != 0).then_some(Change::Step(focus.step))
}

/// A slider's track, filled up to its knob, and the knob. Returns where the knob is.
fn track_and_knob(painter: &Painter, track: Rect, t: f32, colour: Option<&dyn Fn(f32) -> Color32>, lit: bool) -> Pos2 {
    let t = t.clamp(0.0, 1.0);
    painter.rect_filled(track, track.height() / 2.0, Color32::from_white_alpha(30));
    let filled = Rect::from_min_max(track.min, pos2(track.left() + track.width() * t, track.bottom()));
    let knob = pos2(filled.right(), track.center().y);
    let tint = match colour {
        Some(colour) => {
            if filled.width() > 1.0 {
                theme::rounded_gradient_across(painter, filled, track.height() / 2.0, |x| colour(x * t));
            }
            colour(t)
        }
        None => {
            painter.rect_filled(filled, track.height() / 2.0, theme::ACCENT);
            theme::ACCENT
        }
    };
    painter.circle_filled(knob + vec2(0.0, 1.5), 9.5, Color32::from_black_alpha(70));
    painter.circle_filled(knob, if lit { 9.5 } else { 8.5 }, Color32::WHITE);
    painter.circle_filled(knob, 4.0, tint);
    knob
}

/// The slider of one of the day's settings, with the picture of what it sets.
fn slider(ui: &mut egui::Ui, cursor: &mut Cursor, s: Setting, c: &mut Conditions, store: &mut Store, colour: Option<&dyn Fn(f32) -> Color32>) {
    let (min, max) = s.range();
    let v = s.get(c);
    // The visibility's travel is by ratios: as much of it from 1 to 10 km as from 10 to 100.
    let log = s == Setting::Visibility;
    let share = |v: f64| if log { ((v / min).ln() / (max / min).ln()) as f32 } else { ((v - min) / (max - min)) as f32 };
    let t = share(v).clamp(0.0, 1.0);
    let title = match s {
        Setting::CloudCover => "Cloud cover",
        Setting::CloudBase => "Cloud base",
        Setting::Temperature => "Temperature at sea level",
        Setting::WindSpeed => "Speed",
        Setting::Pressure => "Pressure at sea level (QNH)",
        other => other.title(),
    };
    let look = Look { title, value: s.format(v), t, colour };
    let art = |p: &Painter, r: Rect| setting_art(p, r, s, t, v);
    let Some(change) = slider_row(ui, cursor, s.key(), &look, &art) else { return };
    let to = match change {
        Change::To(t) if log => min * (max / min).powf(t as f64),
        Change::To(t) => min + (max - min) * t as f64,
        Change::Step(n) if log => v * 1.12f64.powi(n),
        Change::Step(n) => v + s.step() * n as f64,
    };
    // Far visibilities move in whole kilometres, near ones in tenths.
    let step = if log && to >= 10.0 { 1.0 } else { s.step() };
    let to = ((to / step).round() * step).clamp(min, max);
    if (to - v).abs() > 1e-9 {
        s.set(c, to);
        store.set(s.key(), to);
    }
}

/// The colour of a temperature along its slider: blue for frost, green for a mild day, red for heat.
fn temperature_colour(t: f32) -> Color32 {
    theme::along(&[(0.0, Color32::from_rgb(84, 120, 255)), (0.4, Color32::from_rgb(96, 204, 255)), (0.6, Color32::from_rgb(120, 220, 150)), (0.78, Color32::from_rgb(255, 196, 76)), (1.0, Color32::from_rgb(255, 92, 76))], t)
}

/// The little picture beside a setting's slider: what the value means, `t` of the way along its
/// travel (`v` in its units).
fn setting_art(p: &Painter, r: Rect, s: Setting, t: f32, v: f64) {
    let (c, w, h) = (r.center(), r.width(), r.height());
    let sky = Color32::from_rgb(64, 134, 230);
    match s {
        Setting::CloudCover => {
            // A sky that fills up with cloud.
            theme::rounded_gradient_down(p, r.shrink(1.0), 8.0, theme::mix(sky, Color32::from_rgb(120, 128, 140), t * 0.8), theme::mix(Color32::from_rgb(150, 196, 246), Color32::from_rgb(170, 176, 186), t * 0.8));
            let clouds = [(0.30, 0.50, 0.50), (0.72, 0.72, 0.46), (0.62, 0.36, 0.42), (0.22, 0.86, 0.50), (0.90, 0.40, 0.4)];
            for (k, (x, y, size)) in clouds.iter().enumerate() {
                let grown = ((t * clouds.len() as f32) - k as f32).clamp(0.0, 1.0);
                if grown > 0.0 {
                    theme::cloud(p, pos2(r.left() + w * x, r.top() + h * y), w * size * (0.6 + 0.4 * grown), theme::fade(Color32::WHITE, 0.55 + 0.45 * grown));
                }
            }
        }
        Setting::CloudBase => {
            // A cloud as high over the ground as its base.
            theme::rounded_gradient_down(p, r.shrink(1.0), 8.0, sky, Color32::from_rgb(150, 196, 246));
            p.rect_filled(Rect::from_min_max(pos2(r.left(), r.bottom() - 7.0), r.max), 0.0, Color32::from_rgb(74, 122, 70));
            let base = r.bottom() - 10.0 - (h - 22.0) * t;
            theme::cloud(p, pos2(c.x, base), w * 0.56, Color32::WHITE);
            p.line_segment([pos2(r.left() + 8.0, base + 1.0), pos2(r.left() + 8.0, r.bottom() - 7.0)], Stroke::new(1.0, Color32::from_white_alpha(150)));
        }
        Setting::Visibility => {
            // Hills one behind the other: the haze takes the far ones first.
            let haze = 1.0 - t;
            theme::rounded_gradient_down(p, r.shrink(1.0), 8.0, theme::mix(sky, Color32::from_rgb(206, 200, 184), haze), theme::mix(Color32::from_rgb(170, 206, 246), Color32::from_rgb(222, 214, 196), haze));
            let hills = [(0.70, 0.52, Color32::from_rgb(104, 128, 158), 1.0), (0.34, 0.40, Color32::from_rgb(70, 104, 110), 0.72), (0.62, 0.22, Color32::from_rgb(52, 84, 62), 0.3)];
            for (x, rise, colour, far) in hills {
                let veiled = theme::mix(colour, Color32::from_rgb(214, 208, 192), (haze * (0.35 + far)).clamp(0.0, 0.97));
                let peak = pos2(r.left() + w * x, r.bottom() - h * rise - 6.0);
                p.add(egui::Shape::convex_polygon(vec![peak, pos2(peak.x + w * 0.42, r.bottom()), pos2(peak.x - w * 0.42, r.bottom())], veiled, Stroke::NONE));
            }
        }
        Setting::Humidity => {
            // A drop, as full as the air.
            let drop = |scale: f32, colour: Color32| {
                let (top, low) = (pos2(c.x, c.y - 13.0 * scale), pos2(c.x, c.y + 5.0 * scale));
                p.circle_filled(low, 9.0 * scale, colour);
                p.add(egui::Shape::convex_polygon(vec![top, pos2(low.x + 8.2 * scale, low.y - 3.6 * scale), pos2(low.x - 8.2 * scale, low.y - 3.6 * scale)], colour, Stroke::NONE));
            };
            drop(1.0, Color32::from_white_alpha(40));
            let level = r.bottom() - 6.0 - (h - 12.0) * t;
            let full = p.with_clip_rect(Rect::from_min_max(pos2(r.left(), level), r.max).intersect(p.clip_rect()));
            let (top, low) = (pos2(c.x, c.y - 13.0), pos2(c.x, c.y + 5.0));
            let water = Color32::from_rgb(96, 190, 255);
            full.circle_filled(low, 9.0, water);
            full.add(egui::Shape::convex_polygon(vec![top, pos2(low.x + 8.2, low.y - 3.6), pos2(low.x - 8.2, low.y - 3.6)], water, Stroke::NONE));
        }
        Setting::Pressure => {
            // A barometer: its needle from low on the left to high on the right.
            let centre = pos2(c.x, c.y + 8.0);
            let arc = |from: f32, to: f32, colour: Color32| {
                let points: Vec<Pos2> = (0..=12).map(|k| (from + (to - from) * k as f32 / 12.0).to_radians()).map(|a| centre + 15.0 * vec2(a.cos(), a.sin())).collect();
                p.add(egui::Shape::line(points, Stroke::new(3.0, colour)));
            };
            arc(-180.0, -120.0, Color32::from_rgb(120, 140, 170));
            arc(-120.0, -60.0, theme::GOOD);
            arc(-60.0, 0.0, theme::WARM);
            let a = (-180.0 + 180.0 * t).to_radians();
            p.line_segment([centre, centre + 12.0 * vec2(a.cos(), a.sin())], Stroke::new(2.0, Color32::WHITE));
            p.circle_filled(centre, 2.5, Color32::WHITE);
        }
        Setting::Temperature => {
            // A thermometer, its column as high and as warm as the day.
            let colour = temperature_colour(t);
            let tube = Rect::from_center_size(pos2(c.x, c.y - 4.0), vec2(8.0, h - 18.0));
            p.rect_filled(tube, 4.0, Color32::from_white_alpha(46));
            let bulb = pos2(c.x, r.bottom() - 10.0);
            p.circle_filled(bulb, 7.0, colour);
            let column = Rect::from_min_max(pos2(c.x - 2.0, tube.bottom() - (tube.height() - 3.0) * t), pos2(c.x + 2.0, bulb.y));
            p.rect_filled(column, 2.0, colour);
            if v <= 0.0 {
                theme::text(p, pos2(r.right() - 9.0, r.top() + 10.0), Align2::CENTER_CENTER, "❄", theme::regular(10.0), Color32::from_rgb(170, 214, 255));
            }
        }
        Setting::Turbulence => {
            // The air's path: smooth, or shaken.
            for (k, y) in [-7.0f32, 2.0, 11.0].into_iter().enumerate() {
                let points: Vec<Pos2> = (0..=24).map(|i| i as f32 / 24.0).map(|x| pos2(r.left() + 5.0 + (w - 10.0) * x, c.y + y + (x * 19.0 + k as f32 * 2.1).sin() * (0.6 + 5.0 * t) * (0.6 + 0.4 * (x * 47.0 + k as f32).sin()))).collect();
                p.add(egui::Shape::line(points, Stroke::new(1.6, theme::mix(theme::ACCENT, theme::WARM, t))));
            }
        }
        Setting::WindSpeed => theme::icon(p, c, icon::WIND, 22.0, theme::ACCENT),
        Setting::WindFrom | Setting::Hour => {}
    }
}

/// The colour of the sky with the sun this high, radians above the horizon: night, the glow of
/// dawn and dusk, the day.
fn sky_colour(elevation: f32) -> Color32 {
    theme::along(
        &[(-0.30, Color32::from_rgb(12, 16, 38)), (-0.12, Color32::from_rgb(40, 44, 92)), (-0.03, Color32::from_rgb(214, 108, 84)), (0.05, Color32::from_rgb(255, 176, 96)), (0.2, Color32::from_rgb(132, 186, 240)), (0.5, Color32::from_rgb(66, 140, 236))],
        elevation,
    )
}

/// The hours the sun rises and sets in the game's sky.
fn sunrise_and_sunset() -> (f64, f64) {
    let up = |hour: f64| world_render::sun_elevation(hour) > 0.0;
    let crossing = |from: f64, to: f64| (0..=((to - from) * 60.0) as usize).map(|m| from + m as f64 / 60.0).find(|h| up(*h) != up(from)).unwrap_or(to);
    (crossing(0.0, 12.0), crossing(12.0, 24.0))
}

/// The hour of the day: the sky of the whole day as a band, the sun (or the moon) where the hour
/// is, the sunrise and the sunset marked. A drag along it changes the hour and the world's light.
fn time_of_day(ui: &mut egui::Ui, cursor: &mut Cursor, c: &mut Conditions, store: &mut Store) {
    let s = Setting::Hour;
    let (min, max) = s.range();
    cursor.row();
    let focus = cursor.item(true);
    let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), 122.0), Sense::hover());
    let response = ui.interact(card, ui.scope_id().with("hour"), Sense::click_and_drag());
    let painter = ui.painter();
    theme::card(painter, card, 12.0, false, false);
    let hour = s.get(c);
    let elevation = world_render::sun_elevation(hour) as f32;
    theme::icon(painter, pos2(card.left() + 22.0, card.top() + 23.0), icon::CLOCK, 18.0, theme::ACCENT);
    theme::text(painter, pos2(card.left() + 40.0, card.top() + 23.0), Align2::LEFT_CENTER, "Time of day", theme::medium(15.0), theme::TEXT);
    let part = match (elevation, hour) {
        (e, _) if e < -0.1 => "Night",
        (e, h) if e < 0.12 && h < 12.0 => "Dawn",
        (e, _) if e < 0.12 => "Dusk",
        (_, h) if h < 12.0 => "Morning",
        (_, h) if h < 17.0 => "Afternoon",
        _ => "Evening",
    };
    let time = theme::text(painter, pos2(card.right() - 16.0, card.top() + 24.0), Align2::RIGHT_CENTER, s.format(hour), theme::bold(22.0), theme::TEXT);
    theme::text(painter, pos2(time.left() - 10.0, card.top() + 25.0), Align2::RIGHT_CENTER, part, theme::regular(13.0), theme::DIM);

    // The day as a band of sky.
    let band = Rect::from_min_max(pos2(card.left() + 16.0, card.top() + 48.0), pos2(card.right() - 16.0, card.top() + 80.0));
    theme::rounded_gradient_across(painter, band, 10.0, |x| sky_colour(world_render::sun_elevation(x as f64 * 24.0) as f32));
    painter.rect_stroke(band, 10.0, Stroke::new(1.0, Color32::from_white_alpha(40)), StrokeKind::Inside);
    let x_of = |h: f64| band.left() + band.width() * (h / 24.0) as f32;
    let (rise, set) = sunrise_and_sunset();
    for (h, name) in [(rise, "Sunrise"), (set, "Sunset")] {
        painter.vline(x_of(h), band.y_range(), Stroke::new(1.0, Color32::from_white_alpha(150)));
        theme::text(painter, pos2(x_of(h), band.bottom() + 5.0), Align2::CENTER_TOP, format!("{name} {}", s.format(h)), theme::regular(11.0), theme::DIM);
    }
    for h in [0.0, 12.0, 24.0] {
        let anchor = if h == 0.0 { Align2::LEFT_TOP } else if h == 24.0 { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
        theme::text(painter, pos2(x_of(h), band.bottom() + 5.0), anchor, format!("{h:02.0}"), theme::regular(11.0), theme::FAINT);
    }
    // The sun where the hour is, or the moon by night.
    let at = pos2(x_of(hour), band.center().y);
    painter.circle_filled(at + vec2(0.0, 1.5), 13.0, Color32::from_black_alpha(70));
    painter.circle_filled(at, 12.5, Color32::WHITE);
    if elevation > -0.03 {
        painter.circle_filled(at, 9.5, theme::mix(Color32::from_rgb(255, 120, 60), theme::WARM, (elevation * 5.0).clamp(0.0, 1.0)));
    } else {
        painter.circle_filled(at, 9.5, Color32::from_rgb(40, 48, 92));
        painter.circle_filled(at + vec2(-1.0, 0.0), 6.0, Color32::from_rgb(232, 236, 250));
        painter.circle_filled(at + vec2(2.0, -1.5), 5.0, Color32::from_rgb(40, 48, 92));
    }
    focus.show(ui, card, 12.0);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut to = None;
    if response.is_pointer_button_down_on() {
        if let Some(pointer) = response.interact_pointer_pos() {
            to = Some(((pointer.x - band.left()) / band.width()).clamp(0.0, 1.0) as f64 * 24.0);
        }
    }
    if focus.step != 0 {
        to = Some(hour + s.step() * focus.step as f64);
    }
    if let Some(to) = to {
        let to = ((to / s.step()).round() * s.step()).clamp(min, max);
        if (to - hour).abs() > 1e-9 {
            s.set(c, to);
            store.set(s.key(), to);
        }
    }
}

/// The wind: a compass rose with an arrow blowing from where the wind comes, as long as it is
/// strong. A drag on the rose sets both; the sliders beside it set the speed and the turbulence.
fn wind(ui: &mut egui::Ui, cursor: &mut Cursor, c: &mut Conditions, store: &mut Store) {
    cursor.row();
    let focus = cursor.item(true);
    let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), 158.0), Sense::hover());
    let painter = ui.painter().clone();
    theme::card(&painter, card, 12.0, false, false);
    let (from, speed) = (Setting::WindFrom.get(c), Setting::WindSpeed.get(c));
    let top_speed = Setting::WindSpeed.range().1;
    theme::icon(&painter, pos2(card.left() + 22.0, card.top() + 23.0), icon::WIND, 18.0, theme::ACCENT);
    theme::text(&painter, pos2(card.left() + 40.0, card.top() + 23.0), Align2::LEFT_CENTER, "Wind", theme::medium(15.0), theme::TEXT);

    // The rose.
    let radius = 52.0;
    let centre = pos2(card.left() + 16.0 + radius + 4.0, card.top() + 40.0 + radius + 4.0);
    let rose = Rect::from_center_size(centre, vec2(radius * 2.0 + 16.0, radius * 2.0 + 16.0));
    let response = ui.interact(rose, ui.scope_id().with("wind-rose"), Sense::click_and_drag());
    let polar = |degrees: f64, r: f32| centre + r * vec2(degrees.to_radians().sin() as f32, -degrees.to_radians().cos() as f32);
    painter.circle(centre, radius, Color32::from_rgba_premultiplied(0, 0, 0, 80), Stroke::new(1.0, if response.hovered() || focus.on { theme::ACCENT } else { Color32::from_white_alpha(60) }));
    painter.circle_stroke(centre, radius * 0.55, Stroke::new(1.0, Color32::from_white_alpha(18)));
    for mark in (0..360).step_by(30) {
        let long = mark % 90 == 0;
        painter.line_segment([polar(mark as f64, radius - if long { 7.0 } else { 4.0 }), polar(mark as f64, radius)], Stroke::new(1.0, Color32::from_white_alpha(if long { 170 } else { 80 })));
    }
    for (mark, name) in [(0.0, "N"), (90.0, "E"), (180.0, "S"), (270.0, "W")] {
        theme::text(&painter, polar(mark, radius - 15.0), Align2::CENTER_CENTER, name, theme::medium(11.0), if name == "N" { theme::TEXT } else { theme::DIM });
    }
    // The arrow: from where the wind comes, towards the middle; its tail further out the stronger it blows.
    let reach = |knots: f64| radius * (0.34 + 0.6 * (knots / top_speed) as f32);
    let (tail, tip) = (polar(from, reach(speed)), polar(from, -radius * 0.2));
    if speed > 0.0 {
        let along = (tip - tail).normalized();
        let across = vec2(-along.y, along.x);
        painter.line_segment([tail, tip - along * 6.0], Stroke::new(3.0, theme::ACCENT));
        painter.add(egui::Shape::convex_polygon(vec![tip, tip - along * 11.0 + across * 6.0, tip - along * 11.0 - across * 6.0], theme::ACCENT, Stroke::NONE));
    }
    painter.circle_filled(tail + vec2(0.0, 1.0), 8.5, Color32::from_black_alpha(70));
    painter.circle_filled(tail, 8.0, Color32::WHITE);
    painter.circle_filled(tail, 3.8, theme::ACCENT);
    focus.show(ui, rose, radius + 8.0);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut set = |c: &mut Conditions, s: Setting, v: f64| {
        let (min, max) = s.range();
        let v = ((v / s.step()).round() * s.step()).clamp(min, max);
        if (v - s.get(c)).abs() > 1e-9 {
            s.set(c, v);
            store.set(s.key(), v);
        }
    };
    if response.is_pointer_button_down_on() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let d = pointer - centre;
            if d.length() > 4.0 {
                let degrees = (d.x.atan2(-d.y).to_degrees() as f64 + 360.0) % 360.0;
                set(c, Setting::WindFrom, if degrees > 357.5 { 0.0 } else { degrees });
                set(c, Setting::WindSpeed, ((d.length() / radius - 0.34) / 0.6).clamp(0.0, 1.0) as f64 * top_speed);
            }
        }
    }
    if focus.step != 0 {
        set(c, Setting::WindFrom, (from + 5.0 * focus.step as f64 + 360.0) % 360.0);
    }

    // What it reads, and the two sliders.
    let (from, speed) = (Setting::WindFrom.get(c), Setting::WindSpeed.get(c));
    let reading = if speed < 0.5 { "Calm".to_string() } else { format!("{} at {}", Setting::WindFrom.format(from), Setting::WindSpeed.format(speed)) };
    theme::text(&painter, pos2(card.right() - 16.0, card.top() + 24.0), Align2::RIGHT_CENTER, reading, theme::bold(20.0), theme::TEXT);
    let column = Rect::from_min_max(pos2(rose.right() + 12.0, card.top() + 46.0), pos2(card.right() - 14.0, card.bottom() - 8.0));
    let mut beside = ui.new_child(egui::UiBuilder::new().max_rect(column));
    beside.spacing_mut().item_spacing = vec2(0.0, 0.0);
    slider(&mut beside, cursor, Setting::WindSpeed, c, store, None);
    slider(&mut beside, cursor, Setting::Turbulence, c, store, Some(&|t| theme::mix(theme::ACCENT, theme::WARM, t)));
}

/// A little picture of a kind of day (`None`: the day as the sliders have it, which is none of
/// them), by day or, with the sun down at `hour`, by night.
pub fn sky_art(painter: &Painter, rect: Rect, radius: f32, sky: Option<Sky>, hour: f64) {
    let night = world_render::sun_elevation(hour) < -0.05;
    let (top, bottom) = match sky {
        _ if night => (Color32::from_rgb(10, 14, 36), Color32::from_rgb(30, 40, 74)),
        Some(Sky::Clear) | None => (Color32::from_rgb(34, 104, 220), Color32::from_rgb(126, 184, 246)),
        Some(Sky::Fair) => (Color32::from_rgb(48, 120, 226), Color32::from_rgb(152, 200, 248)),
        Some(Sky::Haze) => (Color32::from_rgb(168, 166, 152), Color32::from_rgb(226, 208, 170)),
        Some(Sky::Overcast) => (Color32::from_rgb(92, 100, 114), Color32::from_rgb(164, 170, 180)),
        Some(Sky::Windy) => (Color32::from_rgb(40, 110, 214), Color32::from_rgb(138, 190, 244)),
        Some(Sky::Storms) => (Color32::from_rgb(30, 34, 50), Color32::from_rgb(92, 96, 116)),
    };
    theme::rounded_gradient_down(painter, rect, radius, top, bottom);
    let p = painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + h * y);
    let white = if night { Color32::from_rgb(120, 130, 160) } else { Color32::WHITE };
    let light = |p: &Painter, x: f32, y: f32, colour: Color32| {
        if night {
            p.circle_filled(at(x, y), h * 0.13, Color32::from_rgb(232, 236, 250));
            p.circle_filled(at(x, y) + vec2(h * 0.06, -h * 0.03), h * 0.11, top);
        } else {
            theme::sun(p, at(x, y), h * 0.14, colour);
        }
    };
    match sky {
        Some(Sky::Clear) => light(&p, 0.74, 0.34, theme::WARM),
        Some(Sky::Fair) | None => {
            light(&p, 0.78, 0.28, theme::WARM);
            theme::cloud(&p, at(0.32, 0.52), w * 0.42, white);
            theme::cloud(&p, at(0.66, 0.74), w * 0.30, theme::fade(white, 0.92));
        }
        Some(Sky::Haze) => {
            light(&p, 0.70, 0.36, Color32::from_rgb(255, 232, 190));
            for (y, alpha) in [(0.50, 50), (0.62, 70), (0.74, 90)] {
                p.rect_filled(Rect::from_min_max(at(0.0, y), at(1.0, y + 0.07)), 0.0, Color32::from_white_alpha(alpha));
            }
        }
        Some(Sky::Overcast) => {
            let grey = if night { Color32::from_rgb(70, 78, 100) } else { Color32::from_rgb(214, 218, 226) };
            theme::cloud(&p, at(0.24, 0.40), w * 0.50, theme::fade(grey, 0.85));
            theme::cloud(&p, at(0.74, 0.46), w * 0.56, grey);
            theme::cloud(&p, at(0.44, 0.62), w * 0.50, theme::mix(grey, Color32::from_rgb(150, 156, 168), 0.5));
        }
        Some(Sky::Windy) => {
            theme::cloud(&p, at(0.70, 0.40), w * 0.34, white);
            for (y, from, to) in [(0.36, 0.08, 0.46), (0.50, 0.16, 0.66), (0.64, 0.06, 0.40)] {
                let points: Vec<Pos2> = (0..=16).map(|k| k as f32 / 16.0).map(|x| at(from + (to - from) * x, y) + vec2(0.0, (x * 6.0).sin() * h * 0.03)).collect();
                p.add(egui::Shape::line(points, Stroke::new(2.0, Color32::from_white_alpha(200))));
            }
        }
        Some(Sky::Storms) => {
            let dark = Color32::from_rgb(70, 74, 94);
            theme::cloud(&p, at(0.50, 0.46), w * 0.62, dark);
            let bolt = [at(0.54, 0.46), at(0.44, 0.64), at(0.52, 0.64), at(0.46, 0.84)];
            p.add(egui::Shape::line(bolt.to_vec(), Stroke::new(2.5, theme::WARM)));
            for x in [0.28, 0.36, 0.66, 0.74] {
                p.line_segment([at(x, 0.54), at(x - 0.04, 0.70)], Stroke::new(1.5, Color32::from_rgb(150, 180, 230)));
            }
        }
    }
}

/// The day in a line, for the start and pause screen: the hour and the wind.
pub fn day_line(c: &Conditions) -> String {
    let speed = Setting::WindSpeed.get(c);
    let wind = if speed < 0.5 { "calm".to_string() } else { format!("wind {} {}", Setting::WindFrom.format(Setting::WindFrom.get(c)), Setting::WindSpeed.format(speed)) };
    format!("{} · {wind}", Setting::Hour.format(Setting::Hour.get(c)))
}
