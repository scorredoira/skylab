//! The menus over the flight: the first screen, as the game opens (`vehicles`: planes, drones or
//! cars), the start and pause screen (`home`: Fly, Aircraft, Place, Weather, Settings, Manual,
//! Replay), the settings as a panel down the right side with the world still in view (`side`), and
//! the aircraft to choose from as pictures (`hangar`). The place picker (`crate::picker`) wears
//! the same look (`theme`).
//!
//! The mouse works them as it works any widget. The keyboard and the game controllers reach them
//! as moves (`Nav`): the game hands over the keys pressed while a menu is shown
//! (`Session::menu_keys`), and each screen keeps a cursor (`Cursor`) over the things it shows that
//! can be chosen or adjusted, in rows.

pub mod hangar;
pub mod home;
pub mod side;
pub mod theme;
pub mod vehicles;

use crate::kinds::Craft;
use engine::egui::{self, Color32, Rect};
use engine::{Controller, KeyCode, PadAxis, PadButton};
use std::collections::HashMap;

/// A move in a menu, from the keyboard or a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// Enter, the space bar, a pad's A: choose what the cursor is on.
    Activate,
    /// Tab and a pad's right shoulder, and back with shift or the left one: the next section.
    NextTab,
    PrevTab,
}

impl Nav {
    /// The move a key makes in a menu, if it makes one.
    pub fn of_key(key: KeyCode, shift: bool) -> Option<Nav> {
        Some(match key {
            KeyCode::ArrowUp => Nav::Up,
            KeyCode::ArrowDown => Nav::Down,
            KeyCode::ArrowLeft => Nav::Left,
            KeyCode::ArrowRight => Nav::Right,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => Nav::Activate,
            KeyCode::Tab if shift => Nav::PrevTab,
            KeyCode::Tab => Nav::NextTab,
            _ => return None,
        })
    }
}

/// The moves the controllers make in a menu, and whether one asks to go back (B): the direction
/// pad and the buttons act once a press; a stick pushed past half way acts once, and again when it
/// has been back to the middle.
#[derive(Default)]
pub struct PadNav {
    /// Where each controller's stick was last frame, as -1, 0 or 1 on each axis.
    sticks: HashMap<usize, (i8, i8)>,
}

impl PadNav {
    pub fn read(&mut self, controllers: &[Controller]) -> (Vec<Nav>, bool) {
        let mut moves = Vec::new();
        let mut back = false;
        self.sticks.retain(|id, _| controllers.iter().any(|c| c.id == *id));
        for c in controllers {
            for (button, nav) in [(PadButton::DPadUp, Nav::Up), (PadButton::DPadDown, Nav::Down), (PadButton::DPadLeft, Nav::Left), (PadButton::DPadRight, Nav::Right), (PadButton::South, Nav::Activate), (PadButton::RightTrigger, Nav::NextTab), (PadButton::LeftTrigger, Nav::PrevTab)] {
                if c.pressed(button) {
                    moves.push(nav);
                }
            }
            back |= c.pressed(PadButton::East);
            // Only a gamepad's left stick: a flight stick is rarely at rest in the middle.
            if !c.standard {
                continue;
            }
            let side = |v: f32| if v > 0.6 { 1 } else if v < -0.6 { -1 } else { 0 };
            let now = (side(c.axis(PadAxis::LeftStickX)), side(c.axis(PadAxis::LeftStickY)));
            let was = self.sticks.insert(c.id, now).unwrap_or((0, 0));
            if now.0 != was.0 && now.0 != 0 {
                moves.push(if now.0 > 0 { Nav::Right } else { Nav::Left });
            }
            if now.1 != was.1 && now.1 != 0 {
                moves.push(if now.1 > 0 { Nav::Up } else { Nav::Down });
            }
        }
        (moves, back)
    }
}

/// One thing of a screen the cursor can be on: the row it is in, and whether left and right
/// change its value (a slider) instead of moving along the row.
#[derive(Clone, Copy)]
struct Item {
    row: u32,
    adjusts: bool,
}

/// What a thing of a screen learns when it takes its place under the cursor (`Cursor::item`).
#[derive(Clone, Copy, Default)]
pub struct Focus {
    /// The cursor is on it and is shown: it draws the ring.
    pub on: bool,
    /// It was chosen (Enter, A).
    pub activate: bool,
    /// Left (-1) or right (+1) was pressed on it, if it adjusts.
    pub step: i32,
    /// The cursor has just come to it: it scrolls into view.
    pub arrived: bool,
}

impl Focus {
    /// Draws the ring round `rect` if the cursor is here, and brings it into view if it just came.
    pub fn show(&self, ui: &egui::Ui, rect: Rect, radius: f32) {
        if self.on {
            theme::focus_ring(ui.painter(), rect, radius);
        }
        if self.arrived {
            ui.scroll_to_rect(rect.expand(24.0), None);
        }
    }
}

/// The keyboard's and the pad's cursor over a screen. The screen calls `begin` with the moves of
/// the frame, then `row` and `item` as it draws, in order; the moves are worked out on the things
/// the frame before drew.
#[derive(Default)]
pub struct Cursor {
    at: usize,
    /// The ring is drawn: the keys were used last, not the mouse.
    shown: bool,
    items: Vec<Item>,
    last: Vec<Item>,
    row: u32,
    activate: bool,
    step: i32,
    arrived: bool,
}

impl Cursor {
    /// Starts a frame: moves as `moves` say over what the last frame drew.
    pub fn begin(&mut self, ctx: &egui::Context, moves: &[Nav]) {
        self.last = std::mem::take(&mut self.items);
        self.row = 0;
        (self.activate, self.step, self.arrived) = (false, 0, false);
        // The mouse moved: the pointer says where the player is, not the ring.
        if ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_pressed()) {
            self.shown = false;
        }
        if self.last.is_empty() {
            return;
        }
        self.at = self.at.min(self.last.len() - 1);
        for nav in moves {
            // The sections' keys are the screen's, not the cursor's.
            if matches!(nav, Nav::NextTab | Nav::PrevTab) {
                continue;
            }
            // The first key only shows where the cursor is.
            if !std::mem::replace(&mut self.shown, true) && *nav != Nav::Activate {
                self.arrived = true;
                continue;
            }
            let here = self.last[self.at];
            let before = self.at;
            match nav {
                Nav::Up | Nav::Down => {
                    let row = if *nav == Nav::Up { here.row.checked_sub(1) } else { Some(here.row + 1) };
                    let column = self.last[..self.at].iter().filter(|i| i.row == here.row).count();
                    let target: Vec<usize> = row.map_or(Vec::new(), |row| (0..self.last.len()).filter(|k| self.last[*k].row == row).collect());
                    if let Some(to) = target.get(column.min(target.len().saturating_sub(1))) {
                        self.at = *to;
                    }
                }
                Nav::Left | Nav::Right if here.adjusts => self.step += if *nav == Nav::Left { -1 } else { 1 },
                Nav::Left if self.at > 0 && self.last[self.at - 1].row == here.row => self.at -= 1,
                Nav::Right if self.at + 1 < self.last.len() && self.last[self.at + 1].row == here.row => self.at += 1,
                Nav::Activate => self.activate = true,
                _ => {}
            }
            self.arrived |= self.at != before;
        }
    }

    /// The things that follow are in a new row.
    pub fn row(&mut self) {
        if self.items.last().is_some_and(|i| i.row == self.row) {
            self.row += 1;
        }
    }

    /// The next thing of the screen takes its place: what the cursor says to it.
    pub fn item(&mut self, adjusts: bool) -> Focus {
        let index = self.items.len();
        self.items.push(Item { row: self.row, adjusts });
        let on = self.shown && index == self.at;
        Focus { on, activate: on && self.activate, step: if on && adjusts { self.step } else { 0 }, arrived: on && self.arrived }
    }

    /// Puts the cursor on the thing that will be drawn `index`-th, shown or not.
    pub fn set(&mut self, index: usize, shown: bool) {
        (self.at, self.shown) = (index, shown);
    }

    /// Back to the first thing, as when the screen shows something else.
    pub fn reset(&mut self) {
        self.at = 0;
    }
}

/// The pictures the menus show, on the GPU once they have been asked for: each aircraft's beauty
/// shot (`assets/aircraft/<model>/picture.jpg`), the featured places' and the courses'
/// (`assets/menu/places/<key>.jpg`) and the first screen's (`assets/menu/vehicles/<name>.jpg`),
/// all taken from the games themselves (`scripts/menu-shots.sh`).
/// A picture that is not there is not an error: the menu draws something in its place. Reading
/// one takes a few milliseconds, so only a couple are read in a frame (`PER_FRAME`): a screen with
/// many shows them as they come, and no frame waits for all of them.
#[derive(Default)]
pub struct Pictures {
    textures: HashMap<String, Option<egui::TextureHandle>>,
    /// How many more may be read in this frame.
    left: usize,
}

/// The pictures read in one frame at most.
const PER_FRAME: usize = 2;

impl Pictures {
    fn load(&mut self, ctx: &egui::Context, name: String, read: impl FnOnce() -> Result<Vec<u8>, String>) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(&name) {
            return texture.clone();
        }
        // Not this frame: what stands in for it is drawn meanwhile.
        self.left = self.left.checked_sub(1)?;
        let texture = read().ok().and_then(|bytes| engine::texture::decode_bytes(&bytes).ok()).map(|(w, h, rgba)| {
            ctx.load_texture(format!("menu-{name}"), egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), egui::TextureOptions::LINEAR)
        });
        self.textures.insert(name, texture.clone());
        texture
    }

    /// The picture of a craft.
    pub fn aircraft(&mut self, ctx: &egui::Context, craft: Craft) -> Option<egui::TextureHandle> {
        self.load(ctx, format!("aircraft-{}", craft.model()), || crate::fleet::file(craft.model(), "picture.jpg"))
    }

    /// A picture of the first screen: `planes`, `drones` or `cars`.
    pub fn vehicle(&mut self, ctx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
        self.load(ctx, format!("vehicle-{name}"), || crate::fleet::asset(&format!("menu/vehicles/{name}.jpg")))
    }

    /// The picture of a place by its key: a featured place's code (`LEMH`), a course's key
    /// (`deepwood`), or `generated` for the seed's world.
    pub fn place(&mut self, ctx: &egui::Context, key: &str) -> Option<egui::TextureHandle> {
        // Only names made of letters, figures and dashes are files of the folder: a point's key is not.
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        self.load(ctx, format!("place-{key}"), || crate::fleet::asset(&format!("menu/places/{key}.jpg")))
    }
}

/// What stands in for a picture that is not there: a dusk sky with an icon.
pub fn no_picture(painter: &egui::Painter, rect: Rect, radius: f32, icon: char) {
    theme::rounded_gradient_down(painter, rect, radius, Color32::from_rgb(34, 58, 96), Color32::from_rgb(20, 26, 38));
    theme::icon(painter, rect.center(), icon, (rect.height() * 0.42).min(64.0), Color32::from_white_alpha(70));
}

/// The menus' state: the four screens and the pictures they share with the place picker.
#[derive(Default)]
pub struct Menus {
    installed: bool,
    pub pictures: Pictures,
    pub home: home::Home,
    pub side: side::Side,
    pub hangar: hangar::Hangar,
    pub vehicles: vehicles::Vehicles,
}

impl Menus {
    /// At the start of every frame of the overlay: the menus' fonts, the first time.
    pub fn begin(&mut self, ctx: &egui::Context) {
        if !std::mem::replace(&mut self.installed, true) {
            theme::install(ctx);
        }
        self.pictures.left = PER_FRAME;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor_over(rows: &[(usize, bool)]) -> Cursor {
        let mut cursor = Cursor::default();
        for (count, adjusts) in rows {
            cursor.row();
            for _ in 0..*count {
                cursor.item(*adjusts);
            }
        }
        cursor
    }

    /// One frame: the moves, then the same things drawn again. Returns what each thing was told.
    fn frame(cursor: &mut Cursor, rows: &[(usize, bool)], moves: &[Nav]) -> Vec<Focus> {
        cursor.begin(&egui::Context::default(), moves);
        let mut told = Vec::new();
        for (count, adjusts) in rows {
            cursor.row();
            for _ in 0..*count {
                told.push(cursor.item(*adjusts));
            }
        }
        told
    }

    /// Every featured place, every race course and the seed's world has its picture in
    /// `assets/menu/places` (`scripts/menu-shots.sh` takes them).
    #[test]
    fn every_featured_place_and_course_has_its_picture() {
        let places = geodata::PRESETS.iter().map(|p| p.code.to_string());
        let courses = sim_core::course::file::courses().iter().map(|c| c.key.clone());
        for key in places.chain(courses).chain(["generated".to_string()]) {
            let bytes = crate::fleet::asset(&format!("menu/places/{key}.jpg")).unwrap_or_else(|e| panic!("{e}"));
            let (w, h, _) = engine::texture::decode_bytes(&bytes).unwrap_or_else(|e| panic!("{key}: {e:?}"));
            assert!(w >= 480 && h >= 280, "{key}: {w}x{h}");
        }
    }

    #[test]
    fn the_cursor_moves_by_rows_and_along_them_and_adjusts_sliders() {
        // Three cards in a row, a slider, two cards.
        let rows = [(3, false), (1, true), (2, false)];
        let mut cursor = cursor_over(&rows);
        // The first key only shows the ring, on the first thing.
        let told = frame(&mut cursor, &rows, &[Nav::Down]);
        assert!(told[0].on && told[0].arrived && !told[0].activate);
        // Right twice along the row, and no further than its end.
        let told = frame(&mut cursor, &rows, &[Nav::Right, Nav::Right, Nav::Right]);
        assert!(told[2].on);
        // Down to the slider: left and right adjust it and do not move.
        let told = frame(&mut cursor, &rows, &[Nav::Down, Nav::Right, Nav::Right, Nav::Left]);
        assert!(told[3].on && told[3].step == 1);
        // Down to the last row, on its first card (the slider's column), and Enter chooses it.
        let told = frame(&mut cursor, &rows, &[Nav::Down, Nav::Activate]);
        assert!(told[4].on && told[4].activate && told[4].step == 0);
        // Up twice, back in the first row; up again stays.
        let told = frame(&mut cursor, &rows, &[Nav::Up, Nav::Up, Nav::Up]);
        assert!(told[0].on);
    }

    #[test]
    fn keys_and_pads_make_the_same_moves() {
        assert_eq!(Nav::of_key(KeyCode::ArrowDown, false), Some(Nav::Down));
        assert_eq!(Nav::of_key(KeyCode::Space, false), Some(Nav::Activate));
        assert_eq!(Nav::of_key(KeyCode::Tab, true), Some(Nav::PrevTab));
        assert_eq!(Nav::of_key(KeyCode::KeyW, false), None);
        let mut pad = Controller::new(3, "Pad", true);
        pad.set_button(0, Some(PadButton::DPadDown), Some(true), None);
        pad.set_button(1, Some(PadButton::South), Some(true), None);
        pad.set_button(2, Some(PadButton::East), Some(true), None);
        pad.set_axis(0, Some(PadAxis::LeftStickX), 0.9);
        let mut nav = PadNav::default();
        let (moves, back) = nav.read(std::slice::from_ref(&pad));
        assert_eq!(moves, [Nav::Down, Nav::Activate, Nav::Right]);
        assert!(back);
        // Held: nothing more until the stick has been back and the buttons pressed again.
        pad.end_frame();
        assert_eq!(nav.read(std::slice::from_ref(&pad)), (Vec::new(), false));
    }
}
