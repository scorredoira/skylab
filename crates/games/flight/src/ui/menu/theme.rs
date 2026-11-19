//! The look of the menus: their typeface (Barlow, three weights), their icons (the Phosphor icon
//! font), their colours, and the few shapes every screen is made of (a dark glass panel, a card, a
//! gradient, a button). The fonts are in `assets/fonts` with their licences (OFL and MIT) and are
//! used only by the menus: the instruments and the display over the flight keep the overlay's own.

use engine::egui::epaint::{self, RectShape};
use engine::egui::{self, pos2, vec2, Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, StrokeKind};

/// The one accent colour: what is chosen, what is pressed to fly.
pub const ACCENT: Color32 = Color32::from_rgb(86, 170, 255);
/// The accent as the fill of a big button.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(34, 116, 224);
/// The dark glass of a panel over the world.
pub const GLASS: Color32 = Color32::from_rgba_premultiplied(12, 15, 22, 247);
/// The same where nothing of the world should show through (a sheet over a dimmed picture).
pub const SHEET: Color32 = Color32::from_rgba_premultiplied(13, 16, 23, 253);
/// A card on the glass, and under the pointer.
pub const CARD: Color32 = Color32::from_rgba_premultiplied(18, 19, 22, 18);
pub const CARD_HOVER: Color32 = Color32::from_rgba_premultiplied(34, 36, 42, 34);
/// A hairline round a panel or a card.
pub const LINE: Color32 = Color32::from_rgba_premultiplied(26, 27, 30, 26);
pub const TEXT: Color32 = Color32::from_rgb(240, 243, 248);
pub const DIM: Color32 = Color32::from_rgb(164, 173, 189);
pub const FAINT: Color32 = Color32::from_rgb(112, 121, 138);
/// The sun, a star, a best time.
pub const WARM: Color32 = Color32::from_rgb(255, 190, 70);
pub const GOOD: Color32 = Color32::from_rgb(108, 214, 142);
pub const BAD: Color32 = Color32::from_rgb(255, 128, 104);

/// The icons used, as their characters in the Phosphor font.
pub mod icon {
    pub const AIRPLANE: char = '\u{e5d6}';
    pub const TAKEOFF: char = '\u{e504}';
    pub const IN_FLIGHT: char = '\u{e4fe}';
    pub const DRONE: char = '\u{ed74}';
    pub const PIN: char = '\u{e316}';
    pub const GLOBE: char = '\u{e28c}';
    pub const CLOUD_SUN: char = '\u{e540}';
    pub const CLOUD: char = '\u{e1aa}';
    pub const WIND: char = '\u{e5d2}';
    pub const THERMOMETER: char = '\u{e5c6}';
    pub const SLIDERS: char = '\u{e434}';
    pub const BOOK: char = '\u{e0e6}';
    pub const REPLAY: char = '\u{e1a0}';
    pub const MONITOR: char = '\u{e32e}';
    pub const INFO: char = '\u{e2ce}';
    pub const FLAG: char = '\u{ea38}';
    pub const KEYBOARD: char = '\u{e2d8}';
    pub const FUEL: char = '\u{e768}';
    pub const SEARCH: char = '\u{e30c}';
    pub const CLOSE: char = '\u{e4f6}';
    pub const CHECK: char = '\u{e182}';
    pub const PLAY: char = '\u{e3d0}';
    pub const CLOCK: char = '\u{e19a}';
    pub const YOKE: char = '\u{e9ac}';
    pub const STICK: char = '\u{ea5e}';
    pub const GAUGE: char = '\u{e628}';
    pub const HOME: char = '\u{e2c2}';
    pub const CAR: char = '\u{e8cc}';
    pub const ARROW_RIGHT: char = '\u{e06c}';
    pub const SWAP: char = '\u{e83c}';
}

/// The menus' typeface in its three weights, and the icons.
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("menu".into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("menu-medium".into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("menu-bold".into()))
}

pub fn icons(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("menu-icons".into()))
}

macro_rules! font {
    ($file:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../assets/fonts/", $file))
    };
}

/// Adds the menus' fonts to the overlay, once, before any text of the frame is laid out. What
/// Barlow lacks (arrows, a few signs) comes from the overlay's own fonts, put behind it.
pub fn install(ctx: &egui::Context) {
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    let families = |names: &[&str], priority: FontPriority| names.iter().map(|name| InsertFontFamily { family: FontFamily::Name((*name).into()), priority }).collect::<Vec<_>>();
    let first = FontPriority::Highest;
    ctx.add_font(FontInsert::new("Barlow-Regular", egui::FontData::from_static(font!("Barlow-Regular.ttf")), families(&["menu"], first)));
    ctx.add_font(FontInsert::new("Barlow-Medium", egui::FontData::from_static(font!("Barlow-Medium.ttf")), families(&["menu-medium"], first)));
    ctx.add_font(FontInsert::new("Barlow-SemiBold", egui::FontData::from_static(font!("Barlow-SemiBold.ttf")), families(&["menu-bold"], first)));
    ctx.add_font(FontInsert::new("Phosphor", egui::FontData::from_static(font!("Phosphor.ttf")), families(&["menu-icons"], first)));
    let own = egui::FontDefinitions::default();
    for (from, name) in [("Ubuntu-Light", "menu-fallback"), ("Hack", "menu-fallback-signs")] {
        if let Some(data) = own.font_data.get(from) {
            ctx.add_font(FontInsert::new(name, (**data).clone(), families(&["menu", "menu-medium", "menu-bold", "menu-icons"], FontPriority::Lowest)));
        }
    }
}

/// The widgets of egui used inside a menu (a text box, a drop-down) in the menus' typeface and colours.
pub fn style(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    for (text, size) in [(egui::TextStyle::Body, 14.5), (egui::TextStyle::Button, 14.5), (egui::TextStyle::Small, 11.5), (egui::TextStyle::Heading, 22.0)] {
        style.text_styles.insert(text, regular(size));
    }
    style.text_styles.insert(egui::TextStyle::Monospace, medium(14.0));
    let v = &mut style.visuals;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgb(22, 52, 96);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.extreme_bg_color = Color32::from_rgba_premultiplied(0, 0, 0, 90);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = 7.0.into();
    }
    v.widgets.inactive.weak_bg_fill = Color32::from_rgba_premultiplied(30, 32, 38, 30);
    v.widgets.inactive.bg_fill = Color32::from_rgba_premultiplied(40, 43, 50, 60);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgba_premultiplied(48, 52, 60, 60);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
}

/// A colour with its opacity multiplied by `t` (0 gone, 1 as it is).
pub fn fade(colour: Color32, t: f32) -> Color32 {
    colour.gamma_multiply(t.clamp(0.0, 1.0))
}

/// Between two colours, `t` of the way from `a` to `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// The colour `t` (0 to 1) of the way along some stops (their place, their colour), in order.
pub fn along(stops: &[(f32, Color32)], t: f32) -> Color32 {
    let Some(first) = stops.first() else { return Color32::TRANSPARENT };
    let mut before = *first;
    for stop in stops {
        if t <= stop.0 {
            let span = (stop.0 - before.0).max(1e-6);
            return mix(before.1, stop.1, (t - before.0) / span);
        }
        before = *stop;
    }
    before.1
}

/// The soft shadow under a panel.
pub fn shadow(painter: &Painter, rect: Rect, radius: f32, strength: f32) {
    let shadow = epaint::Shadow { offset: [0, 10], blur: 36, spread: 2, color: Color32::from_black_alpha((120.0 * strength) as u8) };
    painter.add(shadow.as_shape(rect, radius));
}

/// A panel of dark glass over the world: its shadow, its fill, a hairline round it and a lighter
/// one along its top, as light catches an edge.
pub fn glass(painter: &Painter, rect: Rect, radius: f32, fill: Color32, t: f32) {
    shadow(painter, rect, radius, t);
    painter.rect(rect, radius, fade(fill, t), Stroke::new(1.0, fade(LINE, t)), StrokeKind::Inside);
    let shine = Rect::from_min_max(pos2(rect.left() + radius, rect.top()), pos2(rect.right() - radius, rect.top() + 1.0));
    painter.rect_filled(shine, 0.0, fade(Color32::from_white_alpha(22), t));
}

/// A card: lighter than the glass it lies on, lighter still under the pointer, edged in the accent when chosen.
pub fn card(painter: &Painter, rect: Rect, radius: f32, hovered: bool, chosen: bool) {
    let fill = if chosen { Color32::from_rgba_premultiplied(16, 34, 60, 120) } else if hovered { CARD_HOVER } else { CARD };
    let stroke = if chosen { Stroke::new(1.5, ACCENT) } else { Stroke::new(1.0, LINE) };
    painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);
}

/// The ring round what the keyboard or the pad is on.
pub fn focus_ring(painter: &Painter, rect: Rect, radius: f32) {
    painter.rect_stroke(rect.expand(2.0), radius + 2.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Outside);
}

/// A rectangle filled from one colour at the top to another at the bottom.
pub fn gradient_down(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// A rectangle filled from left to right with `colour(t)`, t from 0 to 1, in `steps` bands.
pub fn gradient_across(painter: &Painter, rect: Rect, steps: usize, colour: impl Fn(f32) -> Color32) {
    let mut mesh = egui::Mesh::default();
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let x = rect.left() + rect.width() * t;
        let c = colour(t);
        mesh.colored_vertex(pos2(x, rect.top()), c);
        mesh.colored_vertex(pos2(x, rect.bottom()), c);
        if k > 0 {
            let i = (k as u32) * 2;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// A picture filling `rect` with round corners, cut at its sides or its top and bottom to keep
/// its shape (as a photograph in a frame), tinted by `tint` (white: as it is).
pub fn picture(painter: &Painter, rect: Rect, radius: impl Into<egui::CornerRadius>, texture: &egui::TextureHandle, tint: Color32) {
    picture_of(painter, rect, radius, texture, tint, pos2(0.5, 0.5), 1.0);
}

/// Where the aircraft is in its picture, as shares of its width and height (`scripts/menu-shots.sh`
/// cuts the pictures so): a small frame shows that part of the picture, larger.
pub const AIRCRAFT_IN_PICTURE: Pos2 = pos2(0.7, 0.27);

/// As `picture`, showing the part of it round `focus` (shares of its width and height), `zoom`
/// times larger: a small frame for a picture whose subject is not in its middle.
pub fn picture_of(painter: &Painter, rect: Rect, radius: impl Into<egui::CornerRadius>, texture: &egui::TextureHandle, tint: Color32, focus: Pos2, zoom: f32) {
    let [w, h] = texture.size();
    let (wanted, has) = (rect.width() / rect.height().max(1.0), w as f32 / (h as f32).max(1.0));
    // The share of the picture shown along each axis.
    let (sx, sy) = if has > wanted { (wanted / has, 1.0) } else { (1.0, has / wanted) };
    let size = vec2(sx, sy) / zoom.max(1.0);
    // As near the focus as the picture's edges let it be.
    let centre = pos2(focus.x.clamp(size.x / 2.0, 1.0 - size.x / 2.0), focus.y.clamp(size.y / 2.0, 1.0 - size.y / 2.0));
    painter.add(RectShape::filled(rect, radius, tint).with_texture(texture.id(), Rect::from_center_size(centre, size)));
}

/// Text at a point; returns the rectangle it took.
pub fn text(painter: &Painter, at: Pos2, anchor: Align2, text: impl ToString, font: FontId, colour: Color32) -> Rect {
    painter.text(at, anchor, text, font, colour)
}

/// An icon centred on a point.
pub fn icon(painter: &Painter, centre: Pos2, icon: char, size: f32, colour: Color32) {
    painter.text(centre, Align2::CENTER_CENTER, icon, icons(size), colour);
}

/// Text with its letters set apart, as a name on a fuselage. Returns its rectangle.
pub fn wide(painter: &Painter, at: Pos2, text: &str, font: FontId, colour: Color32, spacing: f32) -> Rect {
    let job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color: colour, extra_letter_spacing: spacing, ..Default::default() });
    let galley = painter.layout_job(job);
    let rect = Rect::from_min_size(at, galley.size());
    painter.galley(at, galley, colour);
    rect
}

/// Text that fits `width`: cut with an ellipsis if it is longer. Returns its rectangle.
pub fn text_within(painter: &Painter, at: Pos2, width: f32, text: &str, font: FontId, colour: Color32) -> Rect {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color: colour, ..Default::default() });
    job.wrap = egui::text::TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: false, overflow_character: Some('…') };
    let galley = painter.layout_job(job);
    let rect = Rect::from_min_size(at, galley.size());
    painter.galley(at, galley, colour);
    rect
}

/// Text wrapped to `width`, at most `rows` rows. Returns its rectangle.
pub fn paragraph(painter: &Painter, at: Pos2, width: f32, rows: usize, text: &str, font: FontId, colour: Color32) -> Rect {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color: colour, ..Default::default() });
    job.wrap = egui::text::TextWrapping { max_width: width.max(1.0), max_rows: rows, break_anywhere: false, overflow_character: Some('…') };
    let galley = painter.layout_job(job);
    let rect = Rect::from_min_size(at, galley.size());
    painter.galley(at, galley, colour);
    rect
}

/// A key's name as a small cap (`F1`, `Esc`), its right edge at `right`. Returns its rectangle.
pub fn key_cap(painter: &Painter, right: Pos2, name: &str) -> Rect {
    let galley = painter.layout_no_wrap(name.to_string(), medium(11.5), DIM);
    let rect = Rect::from_min_size(pos2(right.x - galley.size().x - 12.0, right.y - 10.0), vec2(galley.size().x + 12.0, 20.0));
    painter.rect(rect, 5.0, Color32::from_rgba_premultiplied(0, 0, 0, 70), Stroke::new(1.0, LINE), StrokeKind::Inside);
    painter.galley(pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0), galley, DIM);
    rect
}

/// A small tag over a picture: text on a dark pill (or, `filled`, white on a pill of `colour`),
/// with an icon before it if one is given; its left edge at `at`, or its right edge if
/// `from_right`. Returns its rectangle.
pub fn chip(painter: &Painter, at: Pos2, glyph: Option<char>, label: &str, colour: Color32, filled: bool, from_right: bool) -> Rect {
    let ink = if filled { Color32::WHITE } else { colour };
    let galley = painter.layout_no_wrap(label.to_string(), medium(11.5), ink);
    let lead = if glyph.is_some() { 18.0 } else { 0.0 };
    let size = vec2(galley.size().x + 16.0 + lead, 22.0);
    let rect = Rect::from_min_size(if from_right { pos2(at.x - size.x, at.y) } else { at }, size);
    painter.rect_filled(rect, 11.0, if filled { colour } else { Color32::from_black_alpha(205) });
    if !filled {
        painter.rect_stroke(rect, 11.0, Stroke::new(1.0, Color32::from_white_alpha(46)), StrokeKind::Inside);
    }
    if let Some(glyph) = glyph {
        icon(painter, pos2(rect.left() + 15.0, rect.center().y), glyph, 13.5, ink);
    }
    painter.galley(pos2(rect.left() + 8.0 + lead, rect.center().y - galley.size().y / 2.0), galley, ink);
    rect
}

/// A cloud: a flat base with three bumps, `width` across, its base's middle at `base`.
pub fn cloud(painter: &Painter, base: Pos2, width: f32, colour: Color32) {
    let r = width * 0.2;
    painter.rect_filled(Rect::from_min_max(pos2(base.x - width * 0.5 + r, base.y - r * 1.25), pos2(base.x + width * 0.5 - r, base.y)), 0.0, colour);
    painter.circle_filled(pos2(base.x - width * 0.5 + r, base.y - r), r, colour);
    painter.circle_filled(pos2(base.x + width * 0.5 - r * 1.1, base.y - r * 1.1), r * 1.1, colour);
    painter.circle_filled(pos2(base.x - width * 0.08, base.y - r * 1.75), r * 1.45, colour);
    painter.circle_filled(pos2(base.x + width * 0.18, base.y - r * 1.5), r * 1.15, colour);
}

/// The sun: a disc with a glow.
pub fn sun(painter: &Painter, centre: Pos2, radius: f32, colour: Color32) {
    painter.circle_filled(centre, radius * 1.9, fade(colour, 0.14));
    painter.circle_filled(centre, radius * 1.4, fade(colour, 0.22));
    painter.circle_filled(centre, radius, colour);
}

/// How far an animation has got, eased: quick at first, settling at the end.
pub fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// The outline of a rectangle with round corners, clockwise from its top left: nine points a corner.
pub fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    let corners = [(rect.left() + r, rect.top() + r, 180.0), (rect.right() - r, rect.top() + r, 270.0), (rect.right() - r, rect.bottom() - r, 0.0), (rect.left() + r, rect.bottom() - r, 90.0)];
    let mut points = Vec::with_capacity(36);
    for (x, y, from) in corners {
        for k in 0..=8 {
            let a = (from + k as f32 * 11.25_f32).to_radians();
            points.push(pos2(x + r * a.cos(), y + r * a.sin()));
        }
    }
    points
}

/// A rectangle with round corners filled with `colour(point)`: right for a colour that changes
/// evenly from one side to the other (it is given at the corners' points and spread between them).
pub fn rounded_gradient(painter: &Painter, rect: Rect, radius: f32, colour: impl Fn(Pos2) -> Color32) {
    let outline = rounded_outline(rect, radius);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), colour(rect.center()));
    for p in &outline {
        mesh.colored_vertex(*p, colour(*p));
    }
    let n = outline.len() as u32;
    for k in 0..n {
        mesh.add_triangle(0, 1 + k, 1 + (k + 1) % n);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// A rectangle with round corners filled from top to bottom.
pub fn rounded_gradient_down(painter: &Painter, rect: Rect, radius: f32, top: Color32, bottom: Color32) {
    rounded_gradient(painter, rect, radius, |p| mix(top, bottom, (p.y - rect.top()) / rect.height().max(1.0)));
}

/// A bar with round ends filled from left to right with `colour(t)`, t from 0 to 1.
pub fn rounded_gradient_across(painter: &Painter, rect: Rect, radius: f32, colour: impl Fn(f32) -> Color32) {
    let r = radius.min(rect.height() / 2.0).min(rect.width() / 2.0).max(0.0);
    let steps = (rect.width() / 2.0).ceil().max(2.0) as u32;
    let mut mesh = egui::Mesh::default();
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let x = rect.left() + rect.width() * t;
        // How far the round end cuts into the bar's height here.
        let into = (rect.left() + r - x).max(x - (rect.right() - r)).max(0.0);
        let cut = r - (r * r - into * into).max(0.0).sqrt();
        let c = colour(t);
        mesh.colored_vertex(pos2(x, rect.top() + cut), c);
        mesh.colored_vertex(pos2(x, rect.bottom() - cut), c);
        if k > 0 {
            let i = k * 2;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}
