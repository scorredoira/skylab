//! The instrument panel of the cockpits that have no panel of their own in 3D (the Extra 300L's, under
//! the bottom 42 % of the screen), and the row of instruments of the outside view: the six basic
//! instruments, tachometer and flap, trim and throttle sliders, the compass and the vacuum gauge;
//! the airspeed, attitude and altimeter dials and the compass are also the C172's standby
//! instruments (`cockpit::instruments`). Every dial shows what the instrument reads (`sim_core::Readings`),
//! not the true state of the flight. Everything is drawn from scratch every frame with egui's
//! painter and scales with the given rectangle.
//!
//! Angles on a dial are in degrees clockwise from the top, as seen on screen (y points down).

use engine::egui::{self, epaint, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use sim_core::{AircraftKind, AircraftType, KNOTS};

/// What the panel shows. Plain numbers so it has no dependency on the simulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct PanelData {
    pub airspeed_kt: f32,
    pub altitude_ft: f32,
    pub vertical_speed_fpm: f32,
    /// Radians.
    pub pitch: f32,
    /// Radians, positive to the right.
    pub roll: f32,
    /// The heading indicator, radians; `None` if the aircraft has none (a compass card takes its place).
    pub heading: Option<f32>,
    /// The magnetic compass, radians.
    pub compass: f32,
    /// The altimeter's Kollsman window, hPa.
    pub altimeter_hpa: f32,
    /// The vacuum gauge, inHg; `None` without a vacuum system.
    pub suction_inhg: Option<f32>,
    /// rad/s, from the turn coordinator's gyro.
    pub turn_rate: f32,
    /// rad, slip ball angle as computed by the simulation; the ball is drawn at `-slip`.
    pub slip: f32,
    pub rpm: f32,
    pub flap_deg: f32,
    /// -1..1, +1 nose up.
    pub trim: f32,
    /// 0..1.
    pub throttle: f32,
    pub stall_warning: bool,
    /// What the airspeed indicator marks and whether there are flaps, for the aircraft flown.
    pub airframe: Airframe,
    /// Load factor now, and the least and most since the start, g: the accelerometer.
    pub load_factor: f32,
    pub g_felt: (f32, f32),
}

/// The marks of the airspeed indicator, in knots, and what takes the place of the flap slider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Airframe {
    /// White arc (flaps), green arc, and the never-exceed speed (the yellow arc runs from the green to it).
    pub white: Option<(f32, f32)>,
    pub green: (f32, f32),
    pub vne: f32,
    /// The highest speed on the dial.
    pub dial_top: f32,
    /// Without flaps an accelerometer stands where their slider is, marked at these limits, g.
    pub flaps: bool,
    pub g_limits: (f32, f32),
    /// A jet's engines, in place of the tachometer: the fan's speed at N1 = 1 (rpm), and N1 at
    /// idle and at its red line.
    pub fan: Option<(f32, f32, f32)>,
}

impl Airframe {
    /// A type's: the marks of its airspeed indicator (`InstrumentFit::airspeed_marks`), and its load limits.
    pub fn of(t: &AircraftType) -> Airframe {
        let marks = t.instruments.airspeed_marks;
        let kt = |v: f64| (v / KNOTS) as f32;
        Airframe {
            white: marks.white.map(|(from, to)| (kt(from), kt(to))),
            green: (kt(marks.green.0), kt(marks.green.1)),
            vne: kt(marks.red_line),
            dial_top: kt(marks.dial_top),
            flaps: t.max_flap_notch() > 0,
            g_limits: (t.load_limit_negative as f32, t.load_limit as f32),
            fan: t.jets.as_ref().map(|jets| (jets.n1_rpm as f32, jets.idle as f32, jets.redline as f32)),
        }
    }
}

impl Default for Airframe {
    /// The C172's.
    fn default() -> Airframe {
        Airframe::of(AircraftKind::C172.spec())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Lever {
    Flaps,
    Trim,
    Throttle,
}

const WHITE: Color32 = Color32::WHITE;
const GREY: Color32 = Color32::from_gray(191);
const GREEN: Color32 = Color32::from_rgb(26, 179, 51);
const YELLOW: Color32 = Color32::from_rgb(242, 204, 26);
const RED: Color32 = Color32::from_rgb(255, 0, 0);
const ORANGE: Color32 = Color32::from_rgb(255, 153, 13);
const SKY: Color32 = Color32::from_rgb(41, 115, 217);
const GROUND: Color32 = Color32::from_rgb(107, 69, 31);

/// Where the instruments go inside the panel rectangle.
struct Layout {
    gap: f32,
    /// Side of one instrument.
    size: f32,
    x0: f32,
    top: f32,
}

impl Layout {
    fn frame(&self, col: usize, row: usize) -> Rect {
        let step = self.size + self.gap;
        Rect::from_min_size(pos2(self.x0 + col as f32 * step, self.top + row as f32 * step), Vec2::splat(self.size))
    }
}

/// The same instruments in one row along the bottom of `area`, centred in it, over the outside view:
/// no panel behind them, only a dark backing for the sliders so their marks read against the sky.
/// They shrink to fit its width.
pub fn draw_strip(painter: &Painter, area: Rect, data: &PanelData) {
    // The six, the controls and the tachometer; then the accelerometer of an aerobatic aircraft,
    // the compass (when a heading indicator holds the sixth place) and the vacuum gauge, if the
    // aircraft has them.
    let compass = data.heading.is_some();
    let g_meter = !data.airframe.flaps;
    let count = 8 + g_meter as usize + compass as usize + data.suction_inhg.is_some() as usize;
    let (w, h) = (area.width(), area.height());
    let gap = h * 0.012;
    let size = (h * 0.15).min((w - gap * (count + 1) as f32) / count as f32);
    if size < 10.0 {
        return;
    }
    let block = size * count as f32 + gap * (count - 1) as f32;
    let row = Layout { gap, size, x0: area.left() + (w - block) / 2.0, top: area.bottom() - size - gap * 1.5 };
    let at = |col: usize| row.frame(col, 0);
    airspeed(painter, at(0), data.airspeed_kt, &data.airframe);
    attitude(painter, at(1), data.pitch, data.roll);
    altimeter(painter, at(2), data.altitude_ft, data.altimeter_hpa);
    controls_dial(painter, at(3), data);
    turn_coordinator(painter, at(4), data.turn_rate, data.slip);
    heading_or_compass(painter, at(5), data);
    vertical_speed(painter, at(6), data.vertical_speed_fpm);
    match data.airframe.fan {
        Some((full, idle, redline)) => share_gauge(painter, at(7), data.rpm / full, 1.2, 0.2, (idle, 1.0), redline, "N1 %"),
        None => tachometer(painter, at(7), data.rpm),
    }
    let mut next = 8;
    if g_meter {
        self::g_meter(painter, at(next), data);
        next += 1;
    }
    if compass {
        compass_window(painter, at(next), data.compass);
        next += 1;
    }
    if let Some(inhg) = data.suction_inhg {
        suction(painter, at(next), inhg);
    }
}

// ---------------------------------------------------------------------------------------------
// Scales

/// Angle of a speed on a dial that reaches `top` knots at 330°.
fn airspeed_angle(kt: f32, top: f32) -> f32 {
    let kt = kt.max(0.0);
    if kt < 40.0 { kt / 40.0 * 30.0 } else { 30.0 + (kt.min(top + 10.0) - 40.0) * 300.0 / (top - 40.0) }
}

fn vertical_speed_angle(fpm: f32) -> f32 {
    -90.0 + fpm.clamp(-2000.0, 2000.0) / 2000.0 * 170.0
}

fn tachometer_angle(rpm: f32) -> f32 {
    -135.0 + rpm.clamp(0.0, 3500.0) / 3500.0 * 270.0
}

// ---------------------------------------------------------------------------------------------
// Drawing helpers

/// Point on a dial: `angle` in degrees clockwise from the top.
fn polar(c: Pos2, r: f32, angle: f32) -> Pos2 {
    let a = angle.to_radians();
    pos2(c.x + r * a.sin(), c.y - r * a.cos())
}

/// A point given as (right, up) from `c`, turned `angle` degrees clockwise about `c`.
fn turned(c: Pos2, right: f32, up: f32, angle: f32) -> Pos2 {
    let (sin, cos) = angle.to_radians().sin_cos();
    pos2(c.x + right * cos + up * sin, c.y - (up * cos - right * sin))
}

fn tick(p: &Painter, c: Pos2, r0: f32, r1: f32, angle: f32, width: f32, color: Color32) {
    p.line_segment([polar(c, r0, angle), polar(c, r1, angle)], Stroke::new(width, color));
}

fn arc(p: &Painter, c: Pos2, r: f32, from: f32, to: f32, width: f32, color: Color32) {
    let steps = (((to - from).abs() / 3.0).ceil() as usize).max(1);
    let points = (0..=steps).map(|i| polar(c, r, from + (to - from) * i as f32 / steps as f32)).collect();
    p.add(Shape::line(points, Stroke::new(width, color)));
}

/// Semibold text centred on `at`, turned `rotation` degrees clockwise about its centre.
fn text(p: &Painter, s: &str, at: Pos2, size: f32, color: Color32, rotation: f32) {
    // The overlay has a single light typeface: a second copy a whole number of pixels along the
    // baseline thickens it (text is snapped to pixels, so a fraction of one would come and go).
    let pixel = 1.0 / p.pixels_per_point();
    let (sin, cos) = rotation.to_radians().sin_cos();
    let shift = vec2(cos, sin) * (size * 0.05 / pixel).round().max(1.0) * pixel;
    label(p, s, at - shift / 2.0, size, color, rotation);
    label(p, s, at + shift / 2.0, size, color, rotation);
}

/// The same in the regular weight.
fn label(p: &Painter, s: &str, at: Pos2, size: f32, color: Color32, rotation: f32) {
    let galley = p.layout_no_wrap(s.to_owned(), FontId::proportional(size), color);
    let pos = at - galley.rect.center().to_vec2();
    p.add(epaint::TextShape::new(pos, galley, color).with_angle_and_anchor(rotation.to_radians(), Align2::CENTER_CENTER));
}

/// A line with round ends.
fn round_line(p: &Painter, points: &[Pos2], width: f32, color: Color32) {
    for pair in points.windows(2) {
        p.line_segment([pair[0], pair[1]], Stroke::new(width, color));
    }
    for point in points {
        p.circle_filled(*point, width / 2.0, color);
    }
}

/// Rim and black face of a round instrument. Returns its centre and side.
fn bezel(p: &Painter, frame: Rect) -> (Pos2, f32) {
    let (c, s) = (frame.center(), frame.width());
    p.circle_filled(c, s * 0.49, Color32::from_gray(56));
    p.circle_filled(c, s * 0.455, Color32::from_gray(8));
    (c, s)
}

/// A pointer from the centre of the dial; `length`, `tail` and `width` are fractions of the dial's side.
fn needle(p: &Painter, c: Pos2, s: f32, angle: f32, length: f32, tail: f32, width: f32, pointed: bool) {
    let (w, len, t) = (width * s / 2.0, length * s, tail * s);
    let shoulder = if pointed { len * 0.85 } else { len };
    let mut outline = vec![(-w, -t), (-w, shoulder)];
    if pointed {
        outline.push((0.0, len));
    }
    outline.extend([(w, shoulder), (w, -t)]);
    let points: Vec<Pos2> = outline.iter().map(|&(x, y)| turned(c, x, y, angle)).collect();
    let shadow = points.iter().map(|&q| q + vec2(s * 0.008, s * 0.008)).collect();
    p.add(Shape::convex_polygon(shadow, Color32::from_black_alpha(110), Stroke::NONE));
    p.add(Shape::convex_polygon(points, WHITE, Stroke::NONE));
}

fn hub(p: &Painter, c: Pos2, s: f32) {
    p.circle(c, s * 0.05, Color32::from_gray(31), Stroke::new(1.0, Color32::from_gray(89)));
}

/// The part of the segment `a`-`b` inside the circle of centre `c` and radius `r`.
fn clip_to_circle(c: Pos2, r: f32, a: Pos2, b: Pos2) -> Option<[Pos2; 2]> {
    let d = b - a;
    let f = a - c;
    let qa = d.length_sq();
    if qa <= 0.0 {
        return None;
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.length_sq() - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc <= 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let t0 = ((-qb - root) / (2.0 * qa)).max(0.0);
    let t1 = ((-qb + root) / (2.0 * qa)).min(1.0);
    (t0 < t1).then(|| [a + d * t0, a + d * t1])
}

// ---------------------------------------------------------------------------------------------
// Instruments

pub(crate) fn airspeed(p: &Painter, frame: Rect, kt: f32, marks: &Airframe) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    let top = marks.dial_top;
    let airspeed_angle = |kt: f32| airspeed_angle(kt, top);
    if let Some((from, to)) = marks.white {
        arc(p, c, r * 0.80, airspeed_angle(from), airspeed_angle(to), s * 0.03, WHITE);
    }
    arc(p, c, r * 0.87, airspeed_angle(marks.green.0), airspeed_angle(marks.green.1), s * 0.035, GREEN);
    arc(p, c, r * 0.87, airspeed_angle(marks.green.1), airspeed_angle(marks.vne), s * 0.035, YELLOW);
    tick(p, c, r * 0.78, r, airspeed_angle(marks.vne), s * 0.02, RED);
    // Every 5 kt up to 200, then every 10; numbers every 20, or 40 on a faster dial.
    let numbered = if top > 220.0 { 40 } else { 20 };
    for mark in (40..=top as i32).step_by(5).filter(|m| *m <= 200 || m % 10 == 0) {
        let major = mark % numbered == 0;
        let angle = airspeed_angle(mark as f32);
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle, s * if major { 0.012 } else { 0.006 }, WHITE);
        if major {
            text(p, &mark.to_string(), polar(c, r * 0.68, angle), s * 0.075, WHITE, 0.0);
        }
    }
    label(p, "KNOTS", c + vec2(0.0, s * 0.16), s * 0.05, GREY, 0.0);
    needle(p, c, s, airspeed_angle(kt), 0.40, 0.1, 0.03, true);
    hub(p, c, s);
}

pub(crate) fn attitude(p: &Painter, frame: Rect, pitch: f32, roll: f32) {
    let (c, s) = (frame.center(), frame.width());
    let radius = s * 0.455;
    let per_degree = s * 0.016;
    let roll_deg = roll.to_degrees();
    let pitch_deg = pitch.to_degrees();
    // The card turns counter-clockwise when rolling right and moves down when pitching up.
    // A point of the card: `right` of its centre line and `above` the horizon line, in points.
    let card = |right: f32, above: f32| turned(c, right, above - pitch_deg * per_degree, -roll_deg);

    // Sky over the whole disc, then the ground as the circular segment below the horizon line.
    p.circle_filled(c, radius, SKY);
    let drop = pitch_deg * per_degree; // Distance from the centre down to the horizon, along the card.
    if drop <= -radius {
        p.circle_filled(c, radius, GROUND);
    } else if drop < radius {
        let half = (drop / radius).acos().to_degrees();
        let steps = ((half / 3.0).ceil() as usize).max(2);
        // The card's "down" is at 180 degrees on the dial, turned with the card.
        let points = (0..=steps).map(|i| polar(c, radius, 180.0 - roll_deg - half + 2.0 * half * i as f32 / steps as f32)).collect();
        p.add(Shape::convex_polygon(points, GROUND, Stroke::NONE));
    }
    let clipped_line = |a: Pos2, b: Pos2, width: f32| {
        if let Some(ends) = clip_to_circle(c, radius, a, b) {
            p.line_segment(ends, Stroke::new(width, WHITE));
        }
    };
    clipped_line(card(-s * 1.5, 0.0), card(s * 1.5, 0.0), s * 0.012);
    for mark in (-40_i32..=40).step_by(5).filter(|m| *m != 0) {
        let above = mark as f32 * per_degree;
        let major = mark % 10 == 0;
        let half = s * if major { 0.13 } else { 0.06 };
        clipped_line(card(-half, above), card(half, above), s * 0.008);
        if major {
            for side in [-1.0, 1.0] {
                // Numerals cannot be cut by the circle, so they are left out when they would cross the rim.
                let at = card(side * (half + s * 0.05), above);
                if (at - c).length() < radius - s * 0.05 {
                    text(p, &mark.abs().to_string(), at, s * 0.05, WHITE, -roll_deg);
                }
            }
        }
    }
    // Bank scale, turning with the horizon.
    for mark in [-60, -30, -20, -10, 0, 10, 20, 30, 60] {
        let major = mark % 30 == 0;
        tick(p, c, radius * if major { 0.80 } else { 0.87 }, radius, mark as f32 - roll_deg, s * if major { 0.015 } else { 0.008 }, WHITE);
    }
    // Rim, fixed pointer and miniature aircraft.
    p.circle_stroke(c, s * 0.47, Stroke::new(s * 0.045, Color32::from_gray(56)));
    let tip = polar(c, s * 0.365, 0.0);
    p.add(Shape::convex_polygon(vec![tip, tip + vec2(s * 0.03, s * 0.055), tip + vec2(-s * 0.03, s * 0.055)], ORANGE, Stroke::NONE));
    for side in [-1.0, 1.0] {
        let wing = [c + vec2(side * s * 0.30, 0.0), c + vec2(side * s * 0.12, 0.0), c + vec2(side * s * 0.12, s * 0.035)];
        round_line(p, &wing, s * 0.022, ORANGE);
    }
    p.circle_filled(c, s * 0.018, ORANGE);
}

pub(crate) fn altimeter(p: &Painter, frame: Rect, feet: f32, hpa: f32) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    // The Kollsman window at three o'clock, under the needles.
    let window = Rect::from_center_size(c + vec2(s * 0.22, 0.0), vec2(s * 0.17, s * 0.075));
    p.rect_filled(window, s * 0.01, Color32::from_gray(225));
    p.rect_stroke(window, s * 0.01, Stroke::new(s * 0.008, Color32::from_gray(90)), egui::StrokeKind::Outside);
    label(p, &format!("{hpa:.0}"), window.center(), s * 0.055, Color32::from_gray(10), 0.0);
    label(p, "hPa", window.center() + vec2(0.0, s * 0.07), s * 0.035, GREY, 0.0);
    for i in 0..50 {
        let major = i % 5 == 0;
        let angle = i as f32 * 7.2;
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle, s * if major { 0.012 } else { 0.006 }, WHITE);
        if major {
            text(p, &(i / 5).to_string(), polar(c, r * 0.72, angle), s * 0.10, WHITE, 0.0);
        }
    }
    label(p, "ALT", c - vec2(0.0, s * 0.13), s * 0.055, GREY, 0.0);
    label(p, "FEET ×100", c + vec2(0.0, s * 0.15), s * 0.042, GREY, 0.0);
    needle(p, c, s, feet / 100_000.0 * 360.0, 0.42, 0.0, 0.012, false);
    needle(p, c, s, feet % 10_000.0 / 10_000.0 * 360.0, 0.24, 0.1, 0.05, true);
    needle(p, c, s, feet % 1_000.0 / 1_000.0 * 360.0, 0.39, 0.1, 0.025, true);
    hub(p, c, s);
}

pub(crate) fn turn_coordinator(p: &Painter, frame: Rect, turn_rate: f32, slip: f32) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    for angle in [90.0, 110.0, 270.0, 250.0] {
        tick(p, c, r * 0.84, r, angle, s * 0.022, WHITE);
    }
    text(p, "I", polar(c, r * 0.70, 252.0), s * 0.075, WHITE, 0.0);
    text(p, "D", polar(c, r * 0.70, 108.0), s * 0.075, WHITE, 0.0);
    label(p, "2 MIN", c + vec2(0.0, s * 0.33), s * 0.045, GREY, 0.0);
    // Tube of the slip ball.
    let tube = Rect::from_min_size(pos2(c.x - s * 0.22, c.y + s * 0.175), vec2(s * 0.44, s * 0.085));
    p.rect_filled(tube, tube.height() / 2.0, Color32::from_gray(217));
    for side in [-1.0, 1.0] {
        p.vline(c.x + side * s * 0.05, tube.y_range(), Stroke::new(s * 0.008, Color32::BLACK));
    }
    let x = (-slip / 9.0_f32.to_radians()).clamp(-1.0, 1.0);
    let travel = s * 0.18;
    p.circle_filled(pos2(c.x + x * travel, c.y + s * 0.2175 + x * x * travel * 0.06), s * 0.038, Color32::from_gray(13));
    // Miniature aircraft seen from behind: banks 20 degrees at a standard-rate turn.
    let bank = (turn_rate / 3.0_f32.to_radians() * 20.0).clamp(-40.0, 40.0);
    let quad = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let points = vec![turned(c, x0, y0, bank), turned(c, x0, y1, bank), turned(c, x1, y1, bank), turned(c, x1, y0, bank)];
        p.add(Shape::convex_polygon(points, WHITE, Stroke::NONE));
    };
    quad(-s * 0.33, -s * 0.012, s * 0.33, s * 0.012);
    quad(-s * 0.008, 0.0, s * 0.008, s * 0.10);
    p.circle_filled(c, s * 0.045, WHITE);
}

/// The heading indicator where the aircraft has one, else a vertical-card compass in its place.
fn heading_or_compass(p: &Painter, frame: Rect, data: &PanelData) {
    match data.heading {
        Some(heading) => heading_indicator(p, frame, heading, false),
        None => heading_indicator(p, frame, data.compass, true),
    }
}

/// A card turning so that the heading is at the top, as the heading indicator or, `magnetic`,
/// the vertical-card compass draw it.
fn heading_indicator(p: &Painter, frame: Rect, heading: f32, magnetic: bool) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    let heading_deg = heading.to_degrees();
    // The card turns so that the current heading is at the top.
    let names = ["N", "3", "6", "E", "12", "15", "S", "21", "24", "W", "30", "33"];
    for mark in (0..360).step_by(5) {
        let major = mark % 10 == 0;
        let angle = mark as f32 - heading_deg;
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle, s * if major { 0.010 } else { 0.006 }, WHITE);
        if mark % 30 == 0 {
            text(p, names[mark / 30], polar(c, r * 0.70, angle), s * 0.085, WHITE, angle);
        }
    }
    // Aircraft outline and lubber line.
    let width = s * 0.014;
    round_line(p, &[c - vec2(0.0, s * 0.16), c + vec2(0.0, s * 0.14)], width, ORANGE);
    round_line(p, &[c + vec2(-s * 0.14, -s * 0.03), c + vec2(s * 0.14, -s * 0.03)], width, ORANGE);
    round_line(p, &[c + vec2(-s * 0.05, s * 0.11), c + vec2(s * 0.05, s * 0.11)], width, ORANGE);
    round_line(p, &[polar(c, s * 0.36, 0.0), polar(c, s * 0.45, 0.0)], s * 0.018, ORANGE);
    if magnetic {
        label(p, "MAG", c + vec2(0.0, s * 0.24), s * 0.045, GREY, 0.0);
    }
}

/// The magnetic compass as it stands on the glare shield: its card seen through a window, the
/// lubber line down the middle. The numbers are on the card's near side, so a higher heading is to
/// the left: the card stays put as the aircraft turns under it.
pub(crate) fn compass_window(p: &Painter, frame: Rect, heading: f32) {
    let (c, s) = (frame.center(), frame.width());
    let case = Rect::from_center_size(c, vec2(s * 0.96, s * 0.62));
    p.rect_filled(case, s * 0.09, Color32::from_gray(30));
    let window = case.shrink2(vec2(s * 0.06, s * 0.08));
    p.rect_filled(window, s * 0.05, Color32::from_gray(12));
    let clip = p.with_clip_rect(window);
    // 35° of the card across the half-width of the window.
    let per_degree = window.width() / 2.0 / 35.0;
    let heading_deg = heading.to_degrees();
    let names = ["N", "3", "6", "E", "12", "15", "S", "21", "24", "W", "30", "33"];
    let base = window.bottom() - window.height() * 0.25;
    for mark in (0..360).step_by(5) {
        let off = ((mark as f32 - heading_deg + 540.0) % 360.0) - 180.0;
        if off.abs() > 40.0 {
            continue;
        }
        let x = c.x - off * per_degree;
        let long = mark % 30 == 0;
        let tall = window.height() * if long { 0.2 } else if mark % 10 == 0 { 0.14 } else { 0.08 };
        clip.line_segment([pos2(x, base), pos2(x, base - tall)], Stroke::new(s * 0.008, WHITE));
        if long {
            text(&clip, names[mark / 30], pos2(x, window.top() + window.height() * 0.3), s * 0.09, WHITE, 0.0);
        }
    }
    p.line_segment([pos2(c.x, window.top() - s * 0.02), pos2(c.x, window.bottom() + s * 0.02)], Stroke::new(s * 0.012, ORANGE));
    label(p, "COMPASS", pos2(c.x, case.bottom() + s * 0.07), s * 0.045, GREY, 0.0);
}

/// How far a 3D cockpit's compass card turns each side of the lubber line across its window,
/// degrees (`compass_card`'s `arc`, `cockpit::parts::compass`'s drum).
pub(crate) const COMPASS_ARC: f32 = 54.0;

/// A magnetic compass's card as a 3D cockpit lays it round its drum: `arc` degrees of it each side
/// of the middle across `frame`'s half width, evenly (the drum's curve does the rest), black with
/// white marks every 5° and figures every 30°, read the way a compass is (east to the left of
/// north). The lubber line is the cockpit's, on the window.
pub(crate) fn compass_card(p: &Painter, frame: Rect, heading: f32, arc: f32) {
    p.rect_filled(frame, 0.0, Color32::from_gray(14));
    let (c, h) = (frame.center(), frame.height());
    let per_degree = frame.width() / 2.0 / arc;
    let names = ["N", "3", "6", "E", "12", "15", "S", "21", "24", "W", "30", "33"];
    let base = frame.top() + h * 0.62;
    for mark in (0..360).step_by(5) {
        let off = ((mark as f32 - heading.to_degrees() + 540.0) % 360.0) - 180.0;
        if off.abs() > arc + 5.0 {
            continue;
        }
        let x = c.x - off * per_degree;
        let (tall, width) = if mark % 30 == 0 { (0.20, 0.022) } else if mark % 10 == 0 { (0.15, 0.016) } else { (0.09, 0.012) };
        p.line_segment([pos2(x, base), pos2(x, base + h * tall)], Stroke::new(h * width, Color32::from_gray(235)));
        if mark % 30 == 0 {
            let letter = mark % 90 == 0;
            text(p, names[mark / 30], pos2(x, frame.top() + h * 0.36), h * if letter { 0.36 } else { 0.28 }, Color32::from_gray(240), 0.0);
        }
    }
    // The card's rim along its foot.
    p.line_segment([pos2(frame.left(), frame.top() + h * 0.88), pos2(frame.right(), frame.top() + h * 0.88)], Stroke::new(h * 0.012, Color32::from_gray(120)));
}

/// A compass's deviation card, as its holder shows it under the compass: what to steer for each
/// heading wanted, with the radios on.
pub(crate) fn deviation_card(p: &Painter, frame: Rect) {
    let ink = Color32::from_gray(25);
    p.rect_filled(frame, frame.height() * 0.04, Color32::from_rgb(232, 229, 218));
    let (w, h) = (frame.width(), frame.height());
    let rows: [(&str, [&str; 6]); 4] = [
        ("FOR", ["N", "30", "60", "E", "120", "150"]),
        ("STEER", ["359", "28", "61", "92", "121", "149"]),
        ("FOR", ["S", "210", "240", "W", "300", "330"]),
        ("STEER", ["182", "211", "239", "268", "298", "332"]),
    ];
    let row_height = h * 0.19;
    let top = frame.top() + h * 0.06;
    for (r, (name, cells)) in rows.iter().enumerate() {
        let y = top + row_height * (r as f32 + 0.5);
        p.text(pos2(frame.left() + w * 0.03, y), Align2::LEFT_CENTER, *name, FontId::proportional(row_height * 0.62), ink);
        for (k, cell) in cells.iter().enumerate() {
            p.text(pos2(frame.left() + w * (0.27 + k as f32 * 0.123), y), Align2::CENTER_CENTER, *cell, FontId::proportional(row_height * 0.66), ink);
        }
    }
    let line = Stroke::new(h * 0.012, ink);
    for r in [2.0, 4.0] {
        let y = top + row_height * r;
        p.line_segment([pos2(frame.left() + w * 0.02, y), pos2(frame.right() - w * 0.02, y)], line);
    }
    p.line_segment([pos2(frame.left() + w * 0.21, top), pos2(frame.left() + w * 0.21, top + row_height * 4.0)], line);
    p.text(pos2(frame.center().x, frame.bottom() - h * 0.09), Align2::CENTER_CENTER, "CALIBRATED WITH RADIOS ON", FontId::proportional(h * 0.10), ink);
}

/// The vacuum gauge: inches of mercury, the green arc 4.5 to 5.5.
fn suction(p: &Painter, frame: Rect, inhg: f32) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    let angle = |v: f32| -120.0 + v.clamp(0.0, 10.0) / 10.0 * 240.0;
    arc(p, c, r * 0.87, angle(4.5), angle(5.5), s * 0.035, GREEN);
    for mark in 0..=20 {
        let major = mark % 4 == 0;
        let v = mark as f32 / 2.0;
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle(v), s * if major { 0.012 } else { 0.006 }, WHITE);
        if major {
            text(p, &format!("{v:.0}"), polar(c, r * 0.70, angle(v)), s * 0.08, WHITE, 0.0);
        }
    }
    label(p, "SUCTION", c + vec2(0.0, s * 0.17), s * 0.05, GREY, 0.0);
    label(p, "inHg", c + vec2(0.0, s * 0.25), s * 0.04, GREY, 0.0);
    needle(p, c, s, angle(inhg), 0.40, 0.1, 0.03, true);
    hub(p, c, s);
}

pub(crate) fn vertical_speed(p: &Painter, frame: Rect, fpm: f32) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    for mark in (-2000_i32..=2000).step_by(100) {
        let major = mark % 500 == 0;
        let angle = vertical_speed_angle(mark as f32);
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle, s * if major { 0.012 } else { 0.006 }, WHITE);
        if major {
            text(p, &(mark.abs() / 100).to_string(), polar(c, r * 0.70, angle), s * 0.08, WHITE, 0.0);
        }
    }
    label(p, "UP", c + vec2(-s * 0.10, -s * 0.12), s * 0.045, GREY, 0.0);
    label(p, "DOWN", c + vec2(-s * 0.10, s * 0.12), s * 0.045, GREY, 0.0);
    label(p, "×100 FT/MIN", c + vec2(s * 0.17, s * 0.07), s * 0.036, GREY, 0.0);
    needle(p, c, s, vertical_speed_angle(fpm), 0.40, 0.1, 0.03, true);
    hub(p, c, s);
}

pub(crate) fn tachometer(p: &Painter, frame: Rect, rpm: f32) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    arc(p, c, r * 0.87, tachometer_angle(2100.0), tachometer_angle(2700.0), s * 0.035, GREEN);
    tick(p, c, r * 0.78, r, tachometer_angle(2700.0), s * 0.02, RED);
    for mark in (0..=3500).step_by(100) {
        let major = mark % 500 == 0;
        let angle = tachometer_angle(mark as f32);
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle, s * if major { 0.012 } else { 0.006 }, WHITE);
        if major {
            text(p, &(mark / 100).to_string(), polar(c, r * 0.70, angle), s * 0.075, WHITE, 0.0);
        }
    }
    label(p, "RPM ×100", c + vec2(0.0, s * 0.22), s * 0.05, GREY, 0.0);
    needle(p, c, s, tachometer_angle(rpm), 0.40, 0.1, 0.03, true);
    hub(p, c, s);
}

/// A round gauge of a share: from nothing at the lower left round the top to `top` at the lower
/// right, numbered every `step` in hundredths, a green arc and a red line (with a yellow arc from
/// the green to it, if it is past the green), its name and what it reads now in figures.
#[allow(clippy::too_many_arguments)]
pub(crate) fn share_gauge(p: &Painter, frame: Rect, value: f32, top: f32, step: f32, green: (f32, f32), red: f32, name: &str) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    let angle = |v: f32| -135.0 + 270.0 * (v / top).clamp(0.0, 1.0);
    arc(p, c, r * 0.87, angle(green.0), angle(green.1), s * 0.035, GREEN);
    if red > green.1 {
        arc(p, c, r * 0.87, angle(green.1), angle(red), s * 0.035, YELLOW);
    }
    tick(p, c, r * 0.76, r, angle(red), s * 0.022, RED);
    let marks = (top / step).round() as usize;
    for k in 0..=marks * 2 {
        let v = k as f32 * step / 2.0;
        let major = k % 2 == 0;
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle(v), s * if major { 0.012 } else { 0.006 }, WHITE);
        if major {
            text(p, &format!("{:.0}", v * 100.0), polar(c, r * 0.68, angle(v)), s * 0.07, WHITE, 0.0);
        }
    }
    label(p, name, c + vec2(0.0, s * 0.15), s * 0.055, GREY, 0.0);
    text(p, &format!("{:.0}", value * 100.0), c + vec2(0.0, s * 0.29), s * 0.1, WHITE, 0.0);
    needle(p, c, s, angle(value), 0.40, 0.1, 0.03, true);
    hub(p, c, s);
}

/// A tilt-rotor's nacelles: a dial with a pointer as they stand, upright at the top and forward
/// to the left as seen from the left side, from `back` degrees behind upright to `forward`, and
/// the angle in figures.
pub(crate) fn nacelle_gauge(p: &Painter, frame: Rect, degrees: f32, back: f32, forward: f32) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    arc(p, c, r * 0.87, back, -forward, s * 0.03, GREEN);
    for mark in (-(forward as i32)..=back as i32).filter(|m| m % 15 == 0) {
        tick(p, c, r * 0.84, r, mark as f32, s * 0.012, WHITE);
        if mark % 30 == 0 {
            text(p, &(-mark).to_string(), polar(c, r * 0.66, mark as f32), s * 0.07, WHITE, 0.0);
        }
    }
    label(p, "NACELLES", c + vec2(0.0, s * 0.16), s * 0.055, GREY, 0.0);
    text(p, &format!("{degrees:.0}°"), c + vec2(0.0, s * 0.28), s * 0.1, WHITE, 0.0);
    needle(p, c, s, -degrees, 0.40, 0.08, 0.035, true);
    hub(p, c, s);
}

/// The heading indicator: a card that turns under a lubber line.
pub(crate) fn heading_dial(p: &Painter, frame: Rect, heading: f32) {
    heading_indicator(p, frame, heading, false);
}

/// A row of lamps with their legends, each lit or not, in its colour.
pub(crate) fn annunciators(p: &Painter, frame: Rect, lamps: &[(&str, bool, Color32)]) {
    p.rect_filled(frame, 0.0, Color32::from_gray(10));
    let w = frame.width() / lamps.len().max(1) as f32;
    for (k, (legend, lit, color)) in lamps.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(frame.left() + k as f32 * w, frame.top()), vec2(w, frame.height())).shrink(frame.height() * 0.08);
        p.rect_filled(cell, cell.height() * 0.12, if *lit { *color } else { Color32::from_gray(26) });
        let ink = if *lit { Color32::from_gray(8) } else { Color32::from_gray(70) };
        text(p, legend, cell.center(), (cell.height() * 0.42).min(cell.width() * 0.16), ink, 0.0);
    }
}

/// The lamps' colours: a warning's, a caution's, an advice's.
pub(crate) const LAMP_RED: Color32 = RED;
pub(crate) const LAMP_AMBER: Color32 = ORANGE;
pub(crate) const LAMP_GREEN: Color32 = GREEN;

/// A round accelerometer, as aerobatic aircraft carry: the load factor from the negative limit to the
/// positive one round the dial, 0 at the top, with a red pointer each for the least and the most
/// felt (`PanelData::g_felt`), the limits in red.
pub(crate) fn g_meter(p: &Painter, frame: Rect, data: &PanelData) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    let (low, high) = data.airframe.g_limits;
    let span = high.max(-low);
    let angle = |g: f32| g.clamp(-span * 1.05, span * 1.05) / span * 150.0;
    arc(p, c, r * 0.87, angle(low * 0.67), angle(high * 0.67), s * 0.03, GREEN);
    arc(p, c, r * 0.87, angle(high * 0.67), angle(high), s * 0.03, YELLOW);
    arc(p, c, r * 0.87, angle(low), angle(low * 0.67), s * 0.03, YELLOW);
    for g in [low, high] {
        tick(p, c, r * 0.76, r, angle(g), s * 0.022, RED);
    }
    for g in (-(span as i32)..=span as i32).filter(|g| *g as f32 >= low && *g as f32 <= high) {
        let major = g % 2 == 0;
        tick(p, c, r * if major { 0.86 } else { 0.92 }, r, angle(g as f32), s * if major { 0.012 } else { 0.006 }, WHITE);
        if major {
            text(p, &g.to_string(), polar(c, r * 0.68, angle(g as f32)), s * 0.08, WHITE, 0.0);
        }
    }
    label(p, "G", c + vec2(0.0, s * 0.2), s * 0.07, GREY, 0.0);
    for g in [data.g_felt.0, data.g_felt.1] {
        round_line(p, &[c, polar(c, r * 0.8, angle(g))], s * 0.014, RED);
    }
    needle(p, c, s, angle(data.load_factor), 0.40, 0.1, 0.03, true);
    hub(p, c, s);
}

/// Throttle, flaps and trim in one round instrument, as a panel's cluster gauge: the throttle round
/// the left from idle to full, the flaps round the right (without flaps, nothing there), the trim
/// along the bottom, the stall warning's lamp at the top.
fn controls_dial(p: &Painter, frame: Rect, data: &PanelData) {
    let (c, s) = bezel(p, frame);
    let r = s * 0.44;
    let pointer = |angle: f32| round_line(p, &[polar(c, r * 0.52, angle), polar(c, r * 0.96, angle)], s * 0.028, WHITE);
    // The throttle: -150° idle to -30° full.
    let throttle = |t: f32| -150.0 + t.clamp(0.0, 1.0) * 120.0;
    arc(p, c, r, throttle(0.0), throttle(1.0), s * 0.008, GREY);
    for k in 0..=4 {
        tick(p, c, r * 0.86, r, throttle(k as f32 / 4.0), s * if k % 2 == 0 { 0.014 } else { 0.008 }, WHITE);
    }
    label(p, "IDLE", polar(c, r * 0.70, throttle(0.0) + 8.0), s * 0.05, WHITE, 0.0);
    label(p, "MAX", polar(c, r * 0.70, throttle(1.0) - 8.0), s * 0.05, WHITE, 0.0);
    text(p, "THR", c + vec2(-s * 0.19, -s * 0.02), s * 0.055, GREY, 0.0);
    pointer(throttle(data.throttle));
    // The flaps: 30° up to 150° at 30 degrees of flap.
    if data.airframe.flaps {
        let flaps = |deg: f32| 30.0 + deg.clamp(0.0, 30.0) * 4.0;
        arc(p, c, r, flaps(0.0), flaps(30.0), s * 0.008, GREY);
        for deg in [0.0, 10.0, 20.0, 30.0] {
            tick(p, c, r * 0.86, r, flaps(deg), s * 0.014, WHITE);
            label(p, &format!("{deg:.0}"), polar(c, r * 0.72, flaps(deg)), s * 0.05, WHITE, 0.0);
        }
        text(p, "FLAPS", c + vec2(s * 0.19, -s * 0.02), s * 0.055, GREY, 0.0);
        pointer(flaps(data.flap_deg));
    }
    // The trim: a scale along the bottom, nose down to the left, nose up to the right.
    let (y, half) = (c.y + s * 0.2, s * 0.16);
    p.line_segment([pos2(c.x - half, y), pos2(c.x + half, y)], Stroke::new(s * 0.008, GREY));
    for k in -2..=2 {
        let x = c.x + half * k as f32 / 2.0;
        p.line_segment([pos2(x, y - s * 0.02), pos2(x, y + s * 0.02)], Stroke::new(s * if k == 0 { 0.012 } else { 0.006 }, WHITE));
    }
    let at = pos2(c.x + half * data.trim.clamp(-1.0, 1.0), y);
    p.add(Shape::convex_polygon(vec![at + vec2(0.0, -s * 0.012), at + vec2(-s * 0.03, -s * 0.06), at + vec2(s * 0.03, -s * 0.06)], WHITE, Stroke::NONE));
    label(p, "TRIM", pos2(c.x, y + s * 0.07), s * 0.045, GREY, 0.0);
    // The stall warning's lamp.
    let lamp = if data.stall_warning { Color32::from_rgb(255, 64, 38) } else { Color32::from_gray(48) };
    p.circle_filled(c + vec2(0.0, -s * 0.24), s * 0.03, lamp);
    label(p, "STALL", c + vec2(0.0, -s * 0.17), s * 0.045, if data.stall_warning { lamp } else { GREY }, 0.0);
    hub(p, c, s);
}

