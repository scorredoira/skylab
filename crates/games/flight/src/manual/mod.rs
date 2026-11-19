//! The manual (F1, or the settings' button): a few short pages with pictures of the game and boxes
//! and labels over them. It covers the window and the game waits, as with the place
//! picker; Esc or F1 closes it, ← → turn the pages.
//!
//! `pages.rs` is what it says (the pictures, where each mark goes, the steps), `inputs.rs` which key
//! and which joystick control does each thing it names, `joystick.rs` the T.16000M drawn. The marks
//! are drawn here over the pictures each frame, so they stay sharp at any size.

pub mod inputs;
mod joystick;
pub mod pages;

use engine::egui::{self, pos2, vec2, Align, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke};
use inputs::Act;
use pages::{Block, Control, Image, Mark, Step, Tone, PAGES};
use std::collections::HashMap;

/// The column of the pages' names.
const SIDE: f32 = 230.0;
/// The widest a page's text and pictures are drawn.
const PAGE_WIDTH: f32 = 1100.0;
const BACKGROUND: Color32 = Color32::from_rgb(24, 26, 30);
const TEXT: Color32 = Color32::from_gray(225);
/// The width of the keyboard's column of the tables.
const KEYS_WIDTH: f32 = 150.0;

/// The colour of a mark: yellow what is pressed, green what is read, pink what was chosen.
fn tone_colour(tone: Tone) -> Color32 {
    match tone {
        Tone::Press => Color32::from_rgb(255, 204, 0),
        Tone::Read => Color32::from_rgb(62, 240, 122),
        Tone::Chosen => Color32::from_rgb(255, 90, 217),
    }
}

#[derive(Default)]
pub struct Manual {
    /// The page shown, an index into `PAGES`.
    pub page: usize,
    /// The pictures already on the GPU, by name: read the first time a page shows them.
    textures: HashMap<&'static str, egui::TextureHandle>,
    /// The page was turned: its top is shown.
    turned: bool,
    /// It was shown in the last frame: the F1 that opens it does not close it.
    shown: bool,
    /// The arrows' glyphs were added to the text's font.
    arrows: bool,
}

impl Manual {
    /// Shows page `page` (0 the first) next time.
    pub fn turn_to(&mut self, page: usize) {
        self.page = page.min(PAGES.len() - 1);
        self.turned = true;
    }

    /// Draws it over the whole window. `open` turns false when it is closed.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) {
        if !std::mem::replace(&mut self.arrows, true) {
            add_arrows(ctx);
        }
        let shown = std::mem::replace(&mut self.shown, true);
        let (close, back, on) = ctx.input(|i| {
            let pressed = |keys: &[egui::Key]| keys.iter().any(|k| i.key_pressed(*k));
            (pressed(&[egui::Key::Escape]) || (shown && pressed(&[egui::Key::F1])),pressed(&[egui::Key::ArrowLeft, egui::Key::PageUp]), pressed(&[egui::Key::ArrowRight, egui::Key::PageDown]))
        });
        if close {
            *open = false;
        }
        if back && self.page > 0 {
            self.turn_to(self.page - 1);
        }
        if on {
            self.turn_to(self.page + 1);
        }
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("manual")).fixed_pos(screen.min).movable(false).order(egui::Order::Middle).show(ctx, |ui| {
            let mut whole = ui.new_child(egui::UiBuilder::new().max_rect(screen));
            whole.set_clip_rect(screen);
            whole.painter().rect_filled(screen, 0.0, BACKGROUND);
            egui::Panel::left("manual_index").exact_size(SIDE).resizable(false).frame(egui::Frame::new().fill(Color32::from_rgb(18, 19, 22)).inner_margin(16.0)).show(&mut whole, |ui| self.index(ui, open));
            egui::CentralPanel::no_frame().show(&mut whole, |ui| self.contents(ui));
        });
    }

    /// It is not shown this frame.
    pub fn hidden(&mut self) {
        self.shown = false;
    }

    /// The column on the left: the pages, and how to close it.
    fn index(&mut self, ui: &mut egui::Ui, open: &mut bool) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Manual").size(24.0).strong().color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(RichText::new("×").size(18.0)).on_hover_text("Close (Esc)").clicked() {
                    *open = false;
                }
            });
        });
        ui.add_space(14.0);
        for (k, page) in PAGES.iter().enumerate() {
            let text = RichText::new(format!("{}  {}", k + 1, page.title)).size(17.0);
            if ui.add_sized([SIDE - 32.0, 34.0], egui::Button::selectable(self.page == k, text)).clicked() && self.page != k {
                self.turn_to(k);
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.label(RichText::new("Esc or F1: close\n← →: page\nThe game waits").size(13.0).weak());
        });
    }

    /// The page: its title and its blocks, down a scrolled column.
    fn contents(&mut self, ui: &mut egui::Ui) {
        let page = &PAGES[self.page];
        let mut scroll = egui::ScrollArea::vertical().id_salt(("manual_page", self.page)).auto_shrink([false, false]);
        if std::mem::take(&mut self.turned) {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show(ui, |ui| {
            egui::Frame::new().inner_margin(Margin { left: 32, right: 24, top: 20, bottom: 40 }).show(ui, |ui| {
                ui.set_max_width(PAGE_WIDTH);
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(RichText::new(page.title).size(28.0).strong().color(TEXT));
                ui.add_space(4.0);
                for block in page.blocks {
                    self.block(ui, block);
                }
            });
        });
    }

    fn block(&mut self, ui: &mut egui::Ui, block: &Block) {
        match block {
            Block::Title(text) => {
                ui.add_space(10.0);
                ui.label(RichText::new(*text).size(21.0).strong().color(TEXT));
            }
            Block::Lines(lines) => {
                for line in *lines {
                    ui.label(RichText::new(*line).size(16.0).color(TEXT));
                }
            }
            Block::Picture(image, marks) => {
                let width = ui.available_width().min(PAGE_WIDTH);
                self.picture(ui, *image, marks, width);
            }
            Block::Views(views) => {
                // As many in a row as fit; the cockpit's numbered as the wheel goes through them.
                let (width, gap) = (250.0, 14.0);
                let per_row = (((ui.available_width().min(PAGE_WIDTH) + gap) / (width + gap)) as usize).max(1);
                for (row, chunk) in views.chunks(per_row).enumerate() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (k, (image, caption)) in chunk.iter().enumerate() {
                            let k = row * per_row + k;
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                self.picture(ui, *image, &[], width);
                                let number = if k + 1 < views.len() { format!("{} · ", k + 1) } else { String::new() };
                                ui.label(RichText::new(format!("{number}{caption}")).size(15.0).color(TEXT));
                            });
                        }
                    });
                }
            }
            Block::Steps(steps) => steps_table(ui, steps),
            Block::Controls(rows) => controls_table(ui, rows),
            Block::Joystick => joystick::draw(ui),
        }
    }

    /// A picture `width` points wide with its marks over it.
    fn picture(&mut self, ui: &mut egui::Ui, image: Image, marks: &[Mark], width: f32) {
        let texture = self.textures.entry(image.name).or_insert_with(|| {
            let (w, h, rgba) = engine::texture::decode_bytes(image.bytes).expect("the manual's pictures are built in and read in its tests");
            ui.ctx().load_texture(format!("manual-{}", image.name), egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), egui::TextureOptions::LINEAR)
        });
        let size = texture.size_vec2();
        let (rect, _) = ui.allocate_exact_size(vec2(width, width * size.y / size.x), Sense::hover());
        let p = ui.painter_at(rect);
        p.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        let at = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
        for m in marks {
            let colour = tone_colour(m.tone);
            let area = Rect::from_min_max(at(m.area[0], m.area[1]), at(m.area[2], m.area[3]));
            p.rect_stroke(area, 5.0, Stroke::new(5.0, Color32::from_black_alpha(200)), egui::StrokeKind::Middle);
            p.rect_stroke(area, 5.0, Stroke::new(2.5, colour), egui::StrokeKind::Middle);
        }
        // The labels over every box, kept inside the picture.
        for m in marks {
            let galley = p.layout_no_wrap(m.label.to_string(), FontId::proportional(15.0), Color32::BLACK);
            let pad = vec2(7.0, 3.0);
            let size = galley.size() + pad * 2.0;
            let mut min = at(m.at[0], m.at[1]);
            min.x = min.x.min(rect.right() - size.x).max(rect.left());
            min.y = min.y.min(rect.bottom() - size.y).max(rect.top());
            let label = Rect::from_min_size(min, size);
            p.rect_filled(label.translate(vec2(0.0, 2.0)), 5.0, Color32::from_black_alpha(140));
            p.rect_filled(label, 5.0, tone_colour(m.tone));
            p.galley(min + pad, galley, Color32::BLACK);
        }
    }
}

/// The text's font has no arrows (← → ↕): the monospaced font's are put behind it, for what it lacks.
fn add_arrows(ctx: &egui::Context) {
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    let Some(hack) = egui::FontDefinitions::default().font_data.get("Hack").cloned() else { return };
    let families = vec![InsertFontFamily { family: egui::FontFamily::Proportional, priority: FontPriority::Lowest }];
    ctx.add_font(FontInsert::new("manual-arrows", (*hack).clone(), families));
}

/// A key as a small cap: `W`, `Espacio`.
fn key_cap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new().fill(Color32::from_gray(62)).stroke(Stroke::new(1.0, Color32::from_gray(95))).corner_radius(4.0).inner_margin(Margin::symmetric(6, 1)).show(ui, |ui| {
        ui.label(RichText::new(text).monospace().size(15.0).color(Color32::WHITE));
    });
}

/// The keys of some acts, as caps, each once.
fn keys(ui: &mut egui::Ui, acts: &[Act]) {
    let mut shown: Vec<String> = Vec::new();
    for label in acts.iter().flat_map(|a| a.key_labels()) {
        if !shown.contains(&label) {
            shown.push(label);
        }
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for label in &shown {
            key_cap(ui, label);
        }
    });
}

/// What does some acts on the joystick, each once; a dash if nothing does.
fn stick(ui: &mut egui::Ui, acts: &[Act]) {
    let mut shown: Vec<String> = Vec::new();
    for text in acts.iter().filter_map(|a| a.stick()) {
        if !shown.contains(&text) {
            shown.push(text);
        }
    }
    let text = if shown.is_empty() { "—".to_string() } else { shown.join(" · ") };
    ui.label(RichText::new(text).size(15.0).color(Color32::from_gray(190)));
}

/// A column's heading, at least `width` wide: the tables of a page line up.
fn header(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.horizontal(|ui| {
        ui.set_min_width(width);
        ui.label(RichText::new(text).size(13.0).weak());
    });
}

/// Numbered steps: the number, what to do, the keys and the joystick.
fn steps_table(ui: &mut egui::Ui, steps: &[Step]) {
    let joystick = steps.iter().any(|s| s.1.iter().any(|a| a.stick().is_some()));
    egui::Grid::new(ui.next_auto_id()).num_columns(4).spacing([18.0, 8.0]).show(ui, |ui| {
        header(ui, "", 24.0);
        header(ui, "", 540.0);
        header(ui, "keyboard", KEYS_WIDTH);
        header(ui, if joystick { "joystick" } else { "" }, 0.0);
        ui.end_row();
        for (k, Step(what, acts)) in steps.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 12.0, Color32::from_gray(70));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, (k + 1).to_string(), FontId::proportional(14.0), Color32::WHITE);
            ui.label(RichText::new(*what).size(16.0).color(TEXT));
            keys(ui, acts);
            if joystick && !acts.is_empty() {
                stick(ui, acts);
            } else {
                ui.label("");
            }
            ui.end_row();
        }
    });
}

/// Rows of keys, the joystick and what they do.
fn controls_table(ui: &mut egui::Ui, rows: &[Control]) {
    egui::Grid::new(ui.next_auto_id()).num_columns(3).spacing([24.0, 8.0]).show(ui, |ui| {
        header(ui, "keyboard", KEYS_WIDTH);
        header(ui, "joystick", 200.0);
        header(ui, "", 0.0);
        ui.end_row();
        for Control(acts, does) in rows {
            keys(ui, acts);
            stick(ui, acts);
            ui.label(RichText::new(*does).size(16.0).color(TEXT));
            ui.end_row();
        }
    });
}
