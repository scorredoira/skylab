//! The Garmin G1000's displays: the PFD (attitude, airspeed and altitude tapes, vertical speed,
//! HSI) and the MFD (engine strip and moving map), painted with egui where a cockpit's file puts
//! them in its instruments' canvas (`cockpit::instruments`). The PFD shows the air data and
//! attitude-heading computers (`Pfd`), a cockpit's standby dials and compass the pneumatic and
//! vacuum instruments (`PanelData`): a blocked pitot fools both, a failed vacuum pump only the
//! standby horizon.
//! Plain numbers in, no dependency on the simulation, as `panel`.

use crate::ui::panel::PanelData;
use engine::egui::{self, epaint, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};
use std::f32::consts::PI;

/// What the PFD shows, from the air data computer and the attitude and heading reference.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pfd {
    pub airspeed_kt: f32,
    pub altitude_ft: f32,
    pub vertical_speed_fpm: f32,
    /// Radians; the heading magnetic.
    pub pitch: f32,
    pub roll: f32,
    pub heading: f32,
    /// The BARO setting, hPa.
    pub baro_hpa: f32,
    /// The LOW VACUUM annunciation.
    pub low_vacuum: bool,
}

/// What the displays show besides the basic instruments.
#[derive(Clone, Debug, Default)]
pub struct G1000Data {
    /// The standby instruments.
    pub panel: PanelData,
    pub pfd: Pfd,
    pub ground_speed_kt: f32,
    /// Radians from north.
    pub track: f32,
    pub true_airspeed_kt: f32,
    /// Where the wind comes from (radians) and its speed.
    pub wind_from: f32,
    pub wind_kt: f32,
    pub oat_c: f32,
    /// Local time, hours.
    pub hour: f32,
    pub fuel_gal: f32,
    pub fuel_flow_gph: f32,
    /// The vacuum gauge, inHg.
    pub suction_inhg: f32,
    /// The nearest aerodrome: its runway number, distance (NM) and bearing (radians).
    pub nearest: Option<(i32, f32, f32)>,
    pub map: MapData,
    /// The GFC 700 and the flight plan, in an aircraft that has one.
    pub afcs: Option<Afcs>,
}

/// What the moving map draws, relative to the aircraft: metres north and east.
#[derive(Clone, Debug, Default)]
pub struct MapData {
    /// Centre, runway direction (radians from north), runway length, runway number.
    pub airports: Vec<(egui::Vec2, f32, f32, i32)>,
    /// Centre and radius.
    pub towns: Vec<(egui::Vec2, f32)>,
    pub roads: Vec<(egui::Vec2, egui::Vec2)>,
}

/// What the GFC 700 and the flight plan show: the mode annunciator bar, the flight director's
/// command bars, the bugs and references, the altitude alerter, the active leg and the glide path.
#[derive(Clone, Debug, Default)]
pub struct Afcs {
    /// Lateral active and armed, vertical active and armed, as the bar writes them (empty: none).
    pub modes: [&'static str; 4],
    /// The vertical mode's reference as the bar writes it: 4500FT, 500FPM, 75KT.
    pub reference: String,
    pub ap: bool,
    /// The disconnect alert: the AP annunciation flashing, red if the autopilot let go by itself.
    pub alert: Option<bool>,
    /// The lateral and the vertical mode were just captured: they flash.
    pub flash: [bool; 2],
    /// What flashes is shown inverted now (half of every second).
    pub blink: bool,
    /// The flight director's commands, radians: pitch and bank.
    pub command: Option<(f32, f32)>,
    /// Radians, magnetic.
    pub heading_bug: f32,
    pub selected_ft: f32,
    pub alerter: AltitudeAlert,
    /// The references of FLC and VS, for their bugs.
    pub airspeed_ref_kt: Option<f32>,
    pub vertical_speed_ref_fpm: Option<f32>,
    /// MAXSPD or MINSPD.
    pub protection: Option<&'static str>,
    /// The active leg.
    pub leg: Option<LegData>,
    /// The legs of the plan relative to the aircraft (north, east metres), and whether each is the active one.
    pub legs: Vec<(egui::Vec2, egui::Vec2, bool)>,
    /// Above the glide path, feet.
    pub glide_ft: Option<f32>,
}

/// The altitude alerter, as the selected altitude's box shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AltitudeAlert {
    #[default]
    None,
    /// Within 1000 ft: the box flashes.
    Approaching,
    /// Within 200 ft.
    Within,
    /// Off it by more than 200 ft: the box flashes yellow.
    Deviation,
}

/// The active leg of the flight plan, for the navigation status and the HSI.
#[derive(Clone, Debug, Default)]
pub struct LegData {
    pub name: String,
    /// Desired track, radians; distance to go, NM; how far right of the leg the aircraft is, NM;
    /// the CDI's full scale, NM.
    pub course: f32,
    pub to_go_nm: f32,
    pub across_nm: f32,
    pub scale_nm: f32,
}

const CYAN: Color32 = Color32::from_rgb(0, 230, 230);
const MAGENTA: Color32 = Color32::from_rgb(230, 0, 230);
const GREEN: Color32 = Color32::from_rgb(20, 220, 60);
const YELLOW: Color32 = Color32::from_rgb(240, 220, 0);
const RED: Color32 = Color32::from_rgb(230, 20, 20);
const WHITE: Color32 = Color32::WHITE;
const GREY: Color32 = Color32::from_gray(150);
const BAR: Color32 = Color32::from_gray(18);
const SKY_TOP: Color32 = Color32::from_rgb(20, 70, 190);
const SKY: Color32 = Color32::from_rgb(70, 130, 230);
const EARTH: Color32 = Color32::from_rgb(120, 76, 28);
const EARTH_LOW: Color32 = Color32::from_rgb(80, 50, 18);
const TAPE: Color32 = Color32::from_rgba_premultiplied(30, 30, 40, 150);

fn text(p: &Painter, at: Pos2, align: Align2, s: &str, size: f32, color: Color32) {
    p.text(at, align, s, FontId::proportional(size), color);
}

fn mono(p: &Painter, at: Pos2, align: Align2, s: &str, size: f32, color: Color32) {
    p.text(at, align, s, FontId::monospace(size), color);
}

fn degrees(r: f32) -> i32 {
    ((r.to_degrees() % 360.0 + 360.0) % 360.0).round() as i32 % 360
}

/// The bar of frequencies across the top of both displays; on the PFD of an aircraft with a GFC 700,
/// its mode annunciator bar and the active leg between them.
fn top_bar(p: &Painter, r: Rect, data: &G1000Data, pfd: bool) {
    let bar = Rect::from_min_size(r.min, vec2(r.width(), 24.0));
    p.rect_filled(bar, 0.0, BAR);
    p.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, GREY));
    mono(p, pos2(bar.left() + 4.0, bar.top() + 7.0), Align2::LEFT_CENTER, "NAV1 108.00 \u{2194} 113.90", 8.5, WHITE);
    mono(p, pos2(bar.left() + 4.0, bar.top() + 17.0), Align2::LEFT_CENTER, "NAV2 108.00   110.60", 8.5, WHITE);
    mono(p, pos2(bar.right() - 4.0, bar.top() + 7.0), Align2::RIGHT_CENTER, "118.000 \u{2194} 121.500 COM1", 8.5, WHITE);
    mono(p, pos2(bar.right() - 4.0, bar.top() + 17.0), Align2::RIGHT_CENTER, "118.000   124.700 COM2", 8.5, WHITE);
    if let (true, Some(afcs)) = (pfd, &data.afcs) {
        afcs_bar(p, pos2(bar.center().x, bar.top() + 7.0), afcs);
        let (line, color) = match &afcs.leg {
            Some(leg) => (format!("{}  DIS {:>4.1}NM  DTK {:03}\u{b0}  TRK {:03}\u{b0}", leg.name, leg.to_go_nm, degrees(leg.course), degrees(data.track)), MAGENTA),
            None => (format!("GS {:>3.0}KT  TRK {:03}\u{b0}", data.ground_speed_kt, degrees(data.track)), CYAN),
        };
        mono(p, pos2(bar.center().x, bar.top() + 17.0), Align2::CENTER_CENTER, &line, 8.5, color);
        return;
    }
    let fields = format!("GS {:>3.0}KT   TRK {:03}\u{b0}", data.ground_speed_kt, degrees(data.track));
    mono(p, pos2(bar.center().x, bar.top() + 7.0), Align2::CENTER_CENTER, &fields, 8.5, CYAN);
    let near = match data.nearest {
        Some((rwy, nm, brg)) => format!("RWY {:02}  DIS {:>4.1}NM  BRG {:03}\u{b0}", rwy, nm, degrees(brg)),
        None => "DIS ----NM".into(),
    };
    mono(p, pos2(bar.center().x, bar.top() + 17.0), Align2::CENTER_CENTER, &near, 8.5, MAGENTA);
}

/// The softkeys' labels along the bottom.
fn softkeys(p: &Painter, r: Rect, labels: &[&str; 12]) {
    let bar = Rect::from_min_max(pos2(r.left(), r.bottom() - 14.0), r.max);
    p.rect_filled(bar, 0.0, Color32::BLACK);
    let w = r.width() / 12.0;
    for (k, l) in labels.iter().enumerate() {
        let x = r.left() + w * (k as f32 + 0.5);
        if !l.is_empty() {
            p.rect_filled(Rect::from_center_size(pos2(x, bar.center().y), vec2(w - 4.0, 11.0)), 1.5, Color32::from_gray(45));
            text(p, pos2(x, bar.center().y), Align2::CENTER_CENTER, l, 7.0, WHITE);
        }
    }
}

/// The GFC 700's mode annunciator bar, centred at `c` in the top bar: the lateral mode active
/// (green) and armed (white), AP, the vertical mode active with its reference (green) and armed
/// (white). A mode just captured flashes, and so does AP after a disconnect (yellow; red if the
/// autopilot let go by itself).
fn afcs_bar(p: &Painter, c: Pos2, a: &Afcs) {
    let field = |x: f32, s: &str, color: Color32, flash: bool| {
        if s.is_empty() {
            return;
        }
        let at = pos2(c.x + x, c.y);
        if flash && a.blink {
            p.rect_filled(Rect::from_center_size(at, vec2(s.chars().count() as f32 * 5.2 + 6.0, 10.0)), 1.0, color);
            mono(p, at, Align2::CENTER_CENTER, s, 8.5, Color32::BLACK);
        } else {
            mono(p, at, Align2::CENTER_CENTER, s, 8.5, color);
        }
    };
    for x in [-50.0, -6.0] {
        p.line_segment([pos2(c.x + x, c.y - 6.0), pos2(c.x + x, c.y + 6.0)], Stroke::new(1.0, GREY));
    }
    field(-110.0, a.modes[0], GREEN, a.flash[0]);
    field(-72.0, a.modes[1], WHITE, false);
    match a.alert {
        Some(abnormal) => field(-28.0, "AP", if abnormal { RED } else { YELLOW }, true),
        None if a.ap => field(-28.0, "AP", GREEN, false),
        None => {}
    }
    field(14.0, a.modes[2], GREEN, a.flash[1]);
    field(58.0, &a.reference, GREEN, false);
    field(106.0, a.modes[3], WHITE, false);
}

/// The flight director's single cue: a magenta V at the attitude it asks for, as seen against the
/// aircraft symbol: `dy` points above it and turned by the bank still to go.
fn command_bars(p: &Painter, c: Pos2, dy: f32, bank: f32) {
    let (s, k) = bank.sin_cos();
    let middle = pos2(c.x, c.y - dy.clamp(-60.0, 60.0));
    let at = |x: f32, y: f32| pos2(middle.x + x * k - y * s, middle.y + x * s + y * k);
    for side in [-1.0f32, 1.0] {
        p.add(Shape::convex_polygon(vec![at(0.0, 0.0), at(side * 68.0, 22.0), at(side * 68.0, 29.0), at(0.0, 7.0)], MAGENTA, Stroke::new(1.0, Color32::BLACK)));
    }
}

/// A reference's bug at `at` on the edge of a tape: a cyan block with a notch, standing out of the
/// edge towards `into` (+1: to the right).
fn bug(p: &Painter, at: Pos2, into: f32) {
    let w = 6.0 * into;
    p.rect_filled(Rect::from_two_pos(pos2(at.x, at.y - 7.0), pos2(at.x + w, at.y + 7.0)), 0.0, CYAN);
    p.add(Shape::convex_polygon(vec![pos2(at.x + w, at.y - 3.0), pos2(at.x + w, at.y + 3.0), pos2(at.x + w * 0.4, at.y)], Color32::BLACK, Stroke::NONE));
}

/// The glide path indicator beside the altitude tape: four dots, 100 ft each, and a magenta diamond
/// where the path is (below the middle when the aircraft is above it).
fn glide_path(p: &Painter, c: Pos2, above_ft: f32) {
    p.rect_filled(Rect::from_center_size(c, vec2(12.0, 118.0)), 1.0, TAPE);
    text(p, pos2(c.x, c.y - 66.0), Align2::CENTER_CENTER, "G", 8.0, MAGENTA);
    for k in [-2.0f32, -1.0, 1.0, 2.0] {
        p.circle_stroke(pos2(c.x, c.y + k * 25.0), 2.5, Stroke::new(1.0, WHITE));
    }
    p.line_segment([pos2(c.x - 5.0, c.y), pos2(c.x + 5.0, c.y)], Stroke::new(1.0, WHITE));
    let y = c.y + (above_ft / 100.0).clamp(-2.4, 2.4) * 25.0;
    p.add(Shape::convex_polygon(vec![pos2(c.x, y - 6.0), pos2(c.x + 5.0, y), pos2(c.x, y + 6.0), pos2(c.x - 5.0, y)], MAGENTA, Stroke::new(1.0, Color32::BLACK)));
}

// ---------------------------------------------------------------- PFD

/// The PFD, filling `r` (512 x 384 points at the size it was drawn for).
pub fn draw_pfd(p: &Painter, r: Rect, data: &G1000Data) {
    let d = &data.pfd;
    let centre = pos2(r.center().x, r.top() + 150.0);
    let afcs = data.afcs.as_ref();
    attitude(p, r, centre, d.pitch, d.roll, afcs.and_then(|a| a.command));
    top_bar(p, r, data, true);
    airspeed_tape(p, pos2(r.left() + 62.0, centre.y), d.airspeed_kt, data.true_airspeed_kt, afcs);
    altitude_tape(p, pos2(r.right() - 92.0, centre.y), d.altitude_ft, d.baro_hpa, afcs);
    vsi(p, pos2(r.right() - 36.0, centre.y), d.vertical_speed_fpm, afcs.and_then(|a| a.vertical_speed_ref_fpm));
    hsi(p, pos2(centre.x, r.bottom() - 62.0), 76.0, d.heading, data.track, data.panel.turn_rate, afcs);
    if d.low_vacuum {
        // The annunciation, right of the altitude tape.
        let at = pos2(r.right() - 57.0, r.top() + 40.0);
        p.rect_filled(Rect::from_min_size(at, vec2(55.0, 13.0)), 1.0, YELLOW);
        text(p, at + vec2(27.5, 6.5), Align2::CENTER_CENTER, "LOW VACUUM", 7.0, Color32::BLACK);
    }
    // Boxes in the corners: temperature and wind on the left, transponder and time on the right.
    let low = r.bottom() - 30.0;
    p.rect_filled(Rect::from_min_size(pos2(r.left() + 4.0, low - 26.0), vec2(74.0, 40.0)), 2.0, Color32::from_black_alpha(200));
    mono(p, pos2(r.left() + 8.0, low - 18.0), Align2::LEFT_CENTER, &format!("OAT {:>3.0}\u{b0}C", data.oat_c), 8.5, WHITE);
    let wind_to = data.wind_from + PI - d.heading;
    mono(p, pos2(r.left() + 8.0, low - 4.0), Align2::LEFT_CENTER, &format!("{:03}\u{b0}/{:.0}KT", degrees(data.wind_from), data.wind_kt), 8.5, WHITE);
    if data.wind_kt > 0.5 {
        let c = pos2(r.left() + 68.0, low - 4.0);
        let dir = vec2(wind_to.sin(), -wind_to.cos()) * 6.0;
        p.arrow(c - dir, dir * 2.0, Stroke::new(1.5, WHITE));
    }
    p.rect_filled(Rect::from_min_size(pos2(r.right() - 82.0, low - 26.0), vec2(78.0, 40.0)), 2.0, Color32::from_black_alpha(200));
    mono(p, pos2(r.right() - 8.0, low - 18.0), Align2::RIGHT_CENTER, "XPDR 1200 ALT", 8.5, GREEN);
    let (h, m) = (data.hour.floor() as i32 % 24, ((data.hour.fract()) * 60.0).floor() as i32);
    mono(p, pos2(r.right() - 8.0, low - 4.0), Align2::RIGHT_CENTER, &format!("LCL {h:02}:{m:02}"), 8.5, WHITE);
    softkeys(p, r, &["", "INSET", "", "PFD", "", "CDI", "DME", "XPDR", "IDENT", "TMR/REF", "NRST", "ALERTS"]);
}

/// The artificial horizon over the whole display, the pitch ladder, the roll scale and the aircraft.
fn attitude(p: &Painter, r: Rect, c: Pos2, pitch: f32, roll: f32, command: Option<(f32, f32)>) {
    const PX: f32 = 7.0; // points per degree of pitch
    let (sn, cs) = roll.sin_cos();
    // Screen point of (along the horizon, above it) in the horizon's frame.
    let at = |x: f32, y: f32| {
        let y = y - pitch.to_degrees() * PX;
        pos2(c.x + x * cs + y * sn, c.y + x * sn - y * cs)
    };
    let big = 900.0;
    let sky = vec![at(-big, 0.0), at(big, 0.0), at(big, big), at(-big, big)];
    let earth = vec![at(-big, 0.0), at(big, 0.0), at(big, -big), at(-big, -big)];
    // Sky lighter at the horizon, earth darker below it: two bands each.
    p.add(Shape::convex_polygon(sky, SKY_TOP, Stroke::NONE));
    p.add(Shape::convex_polygon(vec![at(-big, 0.0), at(big, 0.0), at(big, 120.0), at(-big, 120.0)], SKY, Stroke::NONE));
    p.add(Shape::convex_polygon(earth, EARTH_LOW, Stroke::NONE));
    p.add(Shape::convex_polygon(vec![at(-big, 0.0), at(big, 0.0), at(big, -120.0), at(-big, -120.0)], EARTH, Stroke::NONE));
    p.line_segment([at(-big, 0.0), at(big, 0.0)], Stroke::new(1.5, WHITE));
    // Pitch ladder, clipped to a window round the centre.
    let ladder = p.with_clip_rect(Rect::from_center_size(c, vec2(160.0, 150.0)).intersect(r));
    let mut deg = -30.0f32;
    while deg <= 30.0 {
        if deg != 0.0 {
            let y = deg * PX;
            let half = if deg % 10.0 == 0.0 { 34.0 } else if deg % 5.0 == 0.0 { 18.0 } else { 9.0 };
            ladder.line_segment([at(-half, y), at(half, y)], Stroke::new(1.3, WHITE));
            if deg % 10.0 == 0.0 {
                for x in [-half - 11.0, half + 11.0] {
                    let q = at(x, y);
                    ladder.text(q, Align2::CENTER_CENTER, format!("{}", deg.abs() as i32), FontId::proportional(9.0), WHITE);
                }
            }
        }
        deg += 2.5;
    }
    // Roll scale: an arc with ticks fixed to the horizon, the pointer fixed to the aircraft.
    let radius = 96.0;
    let arc: Vec<Pos2> = (-60..=60).step_by(4).map(|a| {
        let a = (a as f32).to_radians() + roll;
        pos2(c.x + radius * a.sin(), c.y - radius * a.cos())
    }).collect();
    p.add(Shape::line(arc, Stroke::new(1.5, WHITE)));
    for (a, long) in [(-60, true), (-45, false), (-30, true), (-20, false), (-10, false), (10, false), (20, false), (30, true), (45, false), (60, true)] {
        let a = (a as f32).to_radians() + roll;
        let (s, k) = (a.sin(), a.cos());
        let len = if long { 12.0 } else { 7.0 };
        p.line_segment([pos2(c.x + radius * s, c.y - radius * k), pos2(c.x + (radius + len) * s, c.y - (radius + len) * k)], Stroke::new(1.5, WHITE));
    }
    let top = roll;
    let tip = pos2(c.x + radius * top.sin(), c.y - radius * top.cos());
    let side = vec2(top.cos(), top.sin()) * 6.0;
    let out = vec2(top.sin(), -top.cos()) * 10.0;
    p.add(Shape::convex_polygon(vec![tip, tip + out + side, tip + out - side], WHITE, Stroke::NONE));
    // The pointer at the top of the aircraft's frame.
    let ptr = pos2(c.x, c.y - radius + 1.0);
    p.add(Shape::convex_polygon(vec![ptr, ptr + vec2(7.0, 11.0), ptr + vec2(-7.0, 11.0)], YELLOW, Stroke::new(1.0, Color32::BLACK)));
    if let Some((pitch_cmd, bank_cmd)) = command {
        command_bars(p, c, (pitch_cmd - pitch).to_degrees() * PX, bank_cmd - roll);
    }
    // The aircraft: a yellow chevron and two wing bars.
    let chevron = [c, c + vec2(-42.0, 18.0), c + vec2(-24.0, 18.0), c, c + vec2(24.0, 18.0), c + vec2(42.0, 18.0)];
    p.add(Shape::convex_polygon(vec![chevron[0], chevron[1], chevron[2]], YELLOW, Stroke::new(1.0, Color32::BLACK)));
    p.add(Shape::convex_polygon(vec![chevron[3], chevron[4], chevron[5]], YELLOW, Stroke::new(1.0, Color32::BLACK)));
    for s in [-1.0f32, 1.0] {
        p.rect(Rect::from_min_max(pos2(c.x + s * 62.0 - 14.0, c.y - 2.5), pos2(c.x + s * 62.0 + 14.0, c.y + 2.5)), 0.0, YELLOW, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Outside);
    }
}

fn airspeed_tape(p: &Painter, c: Pos2, kt: f32, tas: f32, afcs: Option<&Afcs>) {
    const PX: f32 = 3.2; // points per knot
    let rect = Rect::from_center_size(c, vec2(56.0, 230.0));
    p.rect_filled(rect, 0.0, TAPE);
    let clip = p.with_clip_rect(rect);
    // Coloured bands on the right edge: white flap range, green, yellow, red line.
    let band = |from: f32, to: f32, x: f32, w: f32, color: Color32| {
        let (y0, y1) = (c.y - (to - kt) * PX, c.y - (from - kt) * PX);
        clip.rect_filled(Rect::from_min_max(pos2(rect.right() - x - w, y0), pos2(rect.right() - x, y1)), 0.0, color);
    };
    band(33.0, 85.0, 6.0, 4.0, WHITE);
    band(48.0, 129.0, 0.0, 6.0, GREEN);
    band(129.0, 163.0, 0.0, 6.0, YELLOW);
    band(163.0, 220.0, 0.0, 6.0, RED);
    let low = ((kt - 40.0) / 5.0).floor() as i32 * 5;
    for k in (low..low + 90).step_by(5) {
        if k < 20 {
            continue;
        }
        let y = c.y - (k as f32 - kt) * PX;
        let long = k % 10 == 0;
        clip.line_segment([pos2(rect.right() - 6.0 - if long { 12.0 } else { 7.0 }, y), pos2(rect.right() - 6.0, y)], Stroke::new(1.3, WHITE));
        if long {
            text(&clip, pos2(rect.right() - 22.0, y), Align2::RIGHT_CENTER, &format!("{k}"), 12.0, WHITE);
        }
    }
    if let Some(reference) = afcs.and_then(|a| a.airspeed_ref_kt) {
        let y = (c.y - (reference - kt) * PX).clamp(rect.top(), rect.bottom());
        bug(p, pos2(rect.right(), y), -1.0);
    }
    readout(p, pos2(rect.right() - 4.0, c.y), 48.0, &format!("{:.0}", kt.max(0.0)), true);
    // The reference above it, and a protection holding the airspeed.
    if let Some(afcs) = afcs {
        let top = Rect::from_min_size(pos2(rect.left(), rect.top() - 16.0), vec2(56.0, 16.0));
        if let Some(protection) = afcs.protection {
            p.rect_filled(top, 0.0, if afcs.blink { YELLOW } else { BAR });
            mono(p, top.center(), Align2::CENTER_CENTER, protection, 8.5, if afcs.blink { Color32::BLACK } else { YELLOW });
        } else if let Some(reference) = afcs.airspeed_ref_kt {
            p.rect_filled(top, 0.0, BAR);
            mono(p, top.center(), Align2::CENTER_CENTER, &format!("{reference:.0}KT"), 9.0, CYAN);
        }
    }
    p.rect_filled(Rect::from_min_size(pos2(rect.left(), rect.bottom()), vec2(56.0, 14.0)), 0.0, BAR);
    mono(p, pos2(rect.center().x, rect.bottom() + 7.0), Align2::CENTER_CENTER, &format!("TAS {:.0}KT", tas), 7.5, WHITE);
}

/// The black box with the present value, its point towards the tape (`right`: the point on the right).
fn readout(p: &Painter, tip: Pos2, width: f32, s: &str, right: bool) {
    let dir = if right { -1.0 } else { 1.0 };
    let h = 13.0;
    let base = tip.x + dir * 8.0;
    let far = base + dir * width;
    let points = vec![tip, pos2(base, tip.y - h), pos2(far, tip.y - h), pos2(far, tip.y + h), pos2(base, tip.y + h)];
    p.add(Shape::Path(epaint::PathShape::closed_line(points.clone(), Stroke::new(1.2, WHITE))));
    p.add(Shape::convex_polygon(if right { points.iter().rev().copied().collect() } else { points }, Color32::BLACK, Stroke::new(1.2, WHITE)));
    let x = (base + far) / 2.0;
    text(p, pos2(x, tip.y), Align2::CENTER_CENTER, s, 17.0, WHITE);
}

fn altitude_tape(p: &Painter, c: Pos2, feet: f32, baro_hpa: f32, afcs: Option<&Afcs>) {
    const PX: f32 = 0.42; // points per foot
    let rect = Rect::from_center_size(c, vec2(64.0, 230.0));
    p.rect_filled(rect, 0.0, TAPE);
    let clip = p.with_clip_rect(rect);
    let low = ((feet - 300.0) / 100.0).floor() as i32 * 100;
    for k in (low..low + 700).step_by(20) {
        let y = c.y - (k as f32 - feet) * PX;
        let long = k % 100 == 0;
        clip.line_segment([pos2(rect.left(), y), pos2(rect.left() + if long { 12.0 } else { 6.0 }, y)], Stroke::new(1.2, WHITE));
        if long {
            text(&clip, pos2(rect.left() + 15.0, y), Align2::LEFT_CENTER, &format!("{k}"), 11.0, WHITE);
        }
    }
    // The selected altitude's bug on the tape, then the readout over it.
    if let Some(afcs) = afcs {
        let y = (c.y - (afcs.selected_ft - feet) * PX).clamp(rect.top(), rect.bottom());
        bug(p, pos2(rect.left(), y), 1.0);
    }
    readout(p, pos2(rect.left() + 2.0, c.y), 54.0, &format!("{:.0}", (feet / 10.0).round() * 10.0), false);
    // Selected altitude above (flashing as the alerter says), barometric setting below.
    let top = Rect::from_min_size(pos2(rect.left(), rect.top() - 16.0), vec2(64.0, 16.0));
    match afcs {
        Some(afcs) => {
            let shown = format!("{:.0}FT", afcs.selected_ft);
            let (fill, ink) = match afcs.alerter {
                AltitudeAlert::Approaching if afcs.blink => (CYAN, Color32::BLACK),
                AltitudeAlert::Deviation if afcs.blink => (YELLOW, Color32::BLACK),
                AltitudeAlert::Deviation => (BAR, YELLOW),
                _ => (BAR, CYAN),
            };
            p.rect_filled(top, 0.0, fill);
            if afcs.alerter == AltitudeAlert::Within {
                p.rect_stroke(top.shrink(1.0), 0.0, Stroke::new(1.0, CYAN), egui::StrokeKind::Inside);
            }
            mono(p, top.center(), Align2::CENTER_CENTER, &shown, 9.0, ink);
            if let Some(off) = afcs.glide_ft {
                glide_path(p, pos2(rect.left() - 8.0, c.y), off);
            }
        }
        None => {
            p.rect_filled(top, 0.0, BAR);
            mono(p, top.center(), Align2::CENTER_CENTER, &format!("{:.0}FT", ((feet / 500.0).round() * 500.0).max(0.0)), 9.0, CYAN);
        }
    }
    p.rect_filled(Rect::from_min_size(pos2(rect.left(), rect.bottom()), vec2(64.0, 14.0)), 0.0, BAR);
    mono(p, pos2(rect.center().x, rect.bottom() + 7.0), Align2::CENTER_CENTER, &format!("{baro_hpa:.0}HPA"), 8.5, CYAN);
}

fn vsi(p: &Painter, c: Pos2, fpm: f32, reference: Option<f32>) {
    let rect = Rect::from_center_size(c, vec2(26.0, 170.0));
    p.rect_filled(rect, 0.0, TAPE);
    let scale = |v: f32| c.y - v.clamp(-2000.0, 2000.0) / 2000.0 * 80.0;
    for k in [-2000.0f32, -1000.0, -500.0, 0.0, 500.0, 1000.0, 2000.0] {
        let y = scale(k);
        p.line_segment([pos2(rect.left(), y), pos2(rect.left() + if k % 1000.0 == 0.0 { 8.0 } else { 5.0 }, y)], Stroke::new(1.2, WHITE));
        if k != 0.0 && k.abs() % 1000.0 == 0.0 {
            text(p, pos2(rect.left() + 10.0, y), Align2::LEFT_CENTER, &format!("{}", (k.abs() / 1000.0) as i32), 9.0, WHITE);
        }
    }
    if let Some(reference) = reference {
        bug(p, pos2(rect.left(), scale(reference)), 1.0);
        mono(p, pos2(rect.center().x + 4.0, rect.top() - 7.0), Align2::CENTER_CENTER, &format!("{reference:.0}"), 8.0, CYAN);
    }
    let y = scale(fpm);
    p.add(Shape::convex_polygon(vec![pos2(rect.left() + 2.0, y), pos2(rect.left() + 12.0, y - 7.0), pos2(rect.right() + 14.0, y - 7.0), pos2(rect.right() + 14.0, y + 7.0), pos2(rect.left() + 12.0, y + 7.0)], Color32::BLACK, Stroke::new(1.0, WHITE)));
    let shown = (fpm / 50.0).round() * 50.0;
    text(p, pos2(rect.right() + 3.0, y), Align2::CENTER_CENTER, &if shown.abs() < 50.0 { String::new() } else { format!("{}", shown as i32) }, 8.0, WHITE);
}

/// The horizontal situation indicator: a compass card turning with the heading, the lubber line and
/// heading box, the track bug, a course needle on the nearest runway's line, the turn rate.
fn hsi(p: &Painter, c: Pos2, radius: f32, heading: f32, track: f32, turn_rate: f32, afcs: Option<&Afcs>) {
    p.circle_filled(c, radius + 2.0, Color32::from_rgba_premultiplied(20, 20, 26, 230));
    p.circle_stroke(c, radius, Stroke::new(1.0, GREY));
    for k in 0..72 {
        let deg = k as f32 * 5.0;
        let a = (deg.to_radians()) - heading;
        let (s, cs) = a.sin_cos();
        let len = if k % 6 == 0 { 10.0 } else if k % 2 == 0 { 7.0 } else { 4.0 };
        p.line_segment([pos2(c.x + s * radius, c.y - cs * radius), pos2(c.x + s * (radius - len), c.y - cs * (radius - len))], Stroke::new(1.2, WHITE));
        if k % 6 == 0 {
            let n = k / 2;
            let label = match n {
                0 => "N".to_string(),
                9 => "E".to_string(),
                18 => "S".to_string(),
                27 => "W".to_string(),
                _ => format!("{}", n),
            };
            let q = pos2(c.x + s * (radius - 19.0), c.y - cs * (radius - 19.0));
            p.text(q, Align2::CENTER_CENTER, label, FontId::proportional(10.0), WHITE);
        }
    }
    // Course needle (magenta, a GPS course): the active leg's, its middle off by the deviation
    // (two dots each side, full scale at the CDI's); without a leg, along the track.
    let leg = afcs.and_then(|a| a.leg.as_ref());
    let a = leg.map_or(track, |l| l.course) - heading;
    let dir = vec2(a.sin(), -a.cos());
    let across = vec2(dir.y, -dir.x);
    p.line_segment([c + dir * (radius - 34.0), c + dir * (radius - 24.0)], Stroke::new(3.0, MAGENTA));
    p.line_segment([c - dir * (radius - 24.0), c - dir * (radius - 34.0)], Stroke::new(3.0, MAGENTA));
    p.add(Shape::convex_polygon(vec![c + dir * (radius - 22.0), c + dir * (radius - 34.0) + across * 6.0, c + dir * (radius - 34.0) - across * 6.0], MAGENTA, Stroke::NONE));
    let dots = 34.0;
    for k in [-2.0f32, -1.0, 1.0, 2.0] {
        p.circle_stroke(c - across * (k * dots / 2.0), 2.5, Stroke::new(1.0, WHITE));
    }
    let off = leg.map_or(0.0, |l| (l.across_nm / l.scale_nm).clamp(-1.0, 1.0)) * dots;
    // Right of the leg, the course is to the left.
    p.line_segment([c + across * off - dir * (radius - 36.0), c + across * off + dir * (radius - 36.0)], Stroke::new(3.0, MAGENTA));
    // The heading bug on the card, and its value beside it.
    if let Some(afcs) = afcs {
        let b = afcs.heading_bug - heading;
        let (s, k) = b.sin_cos();
        let at = |r: f32, side: f32| pos2(c.x + r * s + side * k, c.y - r * k + side * s);
        p.add(Shape::convex_polygon(vec![at(radius + 1.0, -6.0), at(radius + 1.0, 6.0), at(radius - 7.0, 6.0), at(radius - 7.0, 3.0), at(radius - 2.0, 0.0), at(radius - 7.0, -3.0), at(radius - 7.0, -6.0)], CYAN, Stroke::new(0.8, Color32::BLACK)));
        let bx = Rect::from_min_size(pos2(c.x - radius - 50.0, c.y - radius - 6.0), vec2(56.0, 14.0));
        p.rect_filled(bx, 1.0, BAR);
        mono(p, bx.center(), Align2::CENTER_CENTER, &format!("HDG {:03}\u{b0}", degrees(afcs.heading_bug)), 8.5, CYAN);
    }
    // The aircraft in the middle.
    p.line_segment([c + vec2(0.0, -9.0), c + vec2(0.0, 10.0)], Stroke::new(2.0, WHITE));
    p.line_segment([c + vec2(-9.0, -1.0), c + vec2(9.0, -1.0)], Stroke::new(2.0, WHITE));
    p.line_segment([c + vec2(-4.0, 8.0), c + vec2(4.0, 8.0)], Stroke::new(2.0, WHITE));
    // Turn rate: a magenta arc from the lubber line, a standard rate turn to the 9° marks.
    let rate = (turn_rate.to_degrees() * 6.0).clamp(-25.0, 25.0);
    if rate.abs() > 0.5 {
        let pts: Vec<Pos2> = (0..=10).map(|k| {
            let a = (rate * k as f32 / 10.0).to_radians();
            pos2(c.x + (radius + 4.0) * a.sin(), c.y - (radius + 4.0) * a.cos())
        }).collect();
        p.add(Shape::line(pts, Stroke::new(3.0, MAGENTA)));
    }
    // Lubber line and heading box.
    p.add(Shape::convex_polygon(vec![pos2(c.x, c.y - radius + 2.0), pos2(c.x - 5.0, c.y - radius - 6.0), pos2(c.x + 5.0, c.y - radius - 6.0)], WHITE, Stroke::NONE));
    let bx = Rect::from_center_size(pos2(c.x, c.y - radius - 15.0), vec2(40.0, 16.0));
    p.rect(bx, 1.0, Color32::BLACK, Stroke::new(1.0, WHITE), egui::StrokeKind::Inside);
    text(p, bx.center(), Align2::CENTER_CENTER, &format!("{:03}\u{b0}", degrees(heading)), 12.0, WHITE);
}

// ---------------------------------------------------------------- MFD

/// The MFD, filling `r` (as the PFD).
pub fn draw_mfd(p: &Painter, r: Rect, data: &G1000Data) {
    p.rect_filled(r, 0.0, Color32::from_rgb(4, 8, 20));
    let map = Rect::from_min_max(pos2(r.left() + 92.0, r.top() + 24.0), pos2(r.right(), r.bottom() - 14.0));
    moving_map(&p.with_clip_rect(map), map, data);
    engine_strip(p, Rect::from_min_max(pos2(r.left(), r.top() + 24.0), pos2(r.left() + 92.0, r.bottom() - 14.0)), data);
    top_bar(p, r, data, false);
    softkeys(p, r, &["ENGINE", "", "MAP", "", "", "", "", "", "", "DCLTR", "SHW CHRT", "CHKLIST"]);
}

/// An arc gauge with a needle and the value under it.
fn arc_gauge(p: &Painter, c: Pos2, radius: f32, value: f32, (lo, hi): (f32, f32), bands: &[(f32, f32, Color32)], label: &str, shown: &str) {
    let angle = |v: f32| -PI * 0.75 + (v - lo) / (hi - lo) * PI * 1.0;
    for &(a, b, color) in bands {
        let pts: Vec<Pos2> = (0..=12).map(|k| {
            let t = angle(a + (b - a) * k as f32 / 12.0);
            pos2(c.x + radius * t.sin(), c.y - radius * t.cos())
        }).collect();
        p.add(Shape::line(pts, Stroke::new(4.0, color)));
    }
    let t = angle(value.clamp(lo, hi));
    p.add(Shape::convex_polygon(vec![pos2(c.x + (radius + 2.0) * t.sin(), c.y - (radius + 2.0) * t.cos()), c + vec2(t.cos(), t.sin()) * 3.0, c - vec2(t.cos(), t.sin()) * 3.0], WHITE, Stroke::NONE));
    text(p, pos2(c.x, c.y + 8.0), Align2::CENTER_CENTER, shown, 13.0, WHITE);
    text(p, pos2(c.x, c.y - radius - 9.0), Align2::CENTER_CENTER, label, 7.5, WHITE);
}

/// A horizontal bar gauge: label and value above, the scale with its bands and a white pointer.
fn bar_gauge(p: &Painter, top_left: Pos2, width: f32, label: &str, shown: &str, t: f32, bands: &[(f32, f32, Color32)]) {
    text(p, top_left, Align2::LEFT_TOP, label, 7.5, WHITE);
    text(p, top_left + vec2(width, 0.0), Align2::RIGHT_TOP, shown, 8.5, WHITE);
    let y = top_left.y + 14.0;
    for &(a, b, color) in bands {
        p.rect_filled(Rect::from_min_max(pos2(top_left.x + a * width, y), pos2(top_left.x + b * width, y + 3.0)), 0.0, color);
    }
    let x = top_left.x + t.clamp(0.0, 1.0) * width;
    p.add(Shape::convex_polygon(vec![pos2(x, y + 1.0), pos2(x - 3.5, y - 5.0), pos2(x + 3.5, y - 5.0)], WHITE, Stroke::NONE));
}

fn engine_strip(p: &Painter, r: Rect, data: &G1000Data) {
    let d = &data.panel;
    p.rect_filled(r, 0.0, Color32::from_gray(10));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, GREY));
    text(p, pos2(r.center().x, r.top() + 8.0), Align2::CENTER_CENTER, "ENGINE", 8.0, CYAN);
    arc_gauge(p, pos2(r.center().x, r.top() + 50.0), 28.0, d.rpm, (0.0, 3000.0), &[(500.0, 2200.0, WHITE), (2200.0, 2700.0, GREEN), (2700.0, 2760.0, RED)], "RPM", &format!("{:.0}", (d.rpm / 10.0).round() * 10.0));
    let (x, w) = (r.left() + 6.0, r.width() - 12.0);
    let power = (d.rpm / 2700.0).clamp(0.0, 1.0);
    let ff = data.fuel_flow_gph;
    let oil_p = if d.rpm > 300.0 { 25.0 + 45.0 * power } else { 0.0 };
    let oil_t = 120.0 + 75.0 * power;
    let egt = if d.rpm > 300.0 { 0.3 + 0.55 * power } else { 0.0 };
    let gauges: [(&str, String, f32, Vec<(f32, f32, Color32)>); 5] = [
        ("FFLOW GPH", format!("{ff:.1}"), ff / 20.0, vec![(0.0, 0.85, GREEN)]),
        ("OIL PRES", format!("{oil_p:.0}"), oil_p / 115.0, vec![(0.0, 0.18, RED), (0.43, 0.87, GREEN), (0.87, 1.0, RED)]),
        ("OIL TEMP", format!("{oil_t:.0}"), oil_t / 250.0, vec![(0.4, 0.98, GREEN), (0.98, 1.0, RED)]),
        ("EGT \u{b0}F", String::new(), egt, vec![(0.0, 1.0, Color32::from_gray(90))]),
        ("VAC", format!("{:.1}", data.suction_inhg), data.suction_inhg / 10.0, vec![(0.45, 0.55, GREEN)]),
    ];
    let mut y = r.top() + 92.0;
    for (label, shown, t, bands) in gauges.iter() {
        bar_gauge(p, pos2(x, y), w, label, shown, *t, bands);
        y += 30.0;
    }
    // Fuel: both tanks.
    text(p, pos2(x, y), Align2::LEFT_TOP, "FUEL QTY GAL", 7.5, WHITE);
    let half = data.fuel_gal / 2.0;
    for (k, side) in ["L", "R"].iter().enumerate() {
        let yy = y + 13.0 + k as f32 * 13.0;
        text(p, pos2(x, yy), Align2::LEFT_TOP, side, 7.5, WHITE);
        let t = (half / 28.0).clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_max(pos2(x + 9.0, yy + 3.0), pos2(x + w, yy + 6.0)), 0.0, Color32::from_gray(60));
        p.rect_filled(Rect::from_min_max(pos2(x + 9.0, yy + 3.0), pos2(x + 9.0 + (w - 9.0) * t, yy + 6.0)), 0.0, if t < 0.2 { YELLOW } else { GREEN });
    }
    y += 44.0;
    text(p, pos2(x, y), Align2::LEFT_TOP, "ELECTRICAL", 7.5, CYAN);
    mono(p, pos2(x, y + 12.0), Align2::LEFT_TOP, "M BUS 28.0  E 28.0", 7.0, WHITE);
    mono(p, pos2(x, y + 22.0), Align2::LEFT_TOP, "M BATT +2   S +0", 7.0, WHITE);
}

fn moving_map(p: &Painter, r: Rect, data: &G1000Data) {
    let d = &data.panel;
    // Track up, the aircraft two thirds of the way down; 5 NM to the ring.
    let me = pos2(r.center().x, r.top() + r.height() * 0.68);
    let ring_nm = 5.0;
    let scale = (r.height() * 0.5) / (ring_nm * 1852.0); // points per metre
    let up = data.track;
    let (s, c) = up.sin_cos();
    let to_map = |v: egui::Vec2| {
        // v = (north, east) metres; rotate so the track points up the screen.
        let (n, e) = (v.x, v.y);
        let right = e * c - n * s;
        let fwd = n * c + e * s;
        pos2(me.x + right * scale, me.y - fwd * scale)
    };
    for (centre, radius) in &data.map.towns {
        p.circle_filled(to_map(*centre), radius * scale, Color32::from_rgba_premultiplied(90, 80, 10, 120));
    }
    for (a, b) in &data.map.roads {
        p.line_segment([to_map(*a), to_map(*b)], Stroke::new(1.0, Color32::from_gray(110)));
    }
    for (centre, heading, length, number) in &data.map.airports {
        let q = to_map(*centre);
        let dir = vec2((heading - up).sin(), -(heading - up).cos()) * (length / 2.0 * scale).max(5.0);
        p.circle_stroke(q, 7.0, Stroke::new(1.5, MAGENTA));
        p.line_segment([q - dir, q + dir], Stroke::new(2.5, MAGENTA));
        text(p, q + vec2(10.0, -10.0), Align2::LEFT_CENTER, &format!("RWY {:02}", number), 8.0, WHITE);
    }
    if let Some(afcs) = &data.afcs {
        for (a, b, active) in &afcs.legs {
            p.line_segment([to_map(*a), to_map(*b)], Stroke::new(if *active { 2.5 } else { 1.5 }, if *active { MAGENTA } else { WHITE }));
        }
    }
    // The range ring and its label, the track line.
    p.circle_stroke(me, r.height() * 0.5, Stroke::new(1.0, Color32::from_gray(140)));
    text(p, pos2(me.x - r.height() * 0.35, me.y - r.height() * 0.35), Align2::CENTER_CENTER, &format!("{ring_nm:.0}NM"), 8.0, WHITE);
    p.line_segment([me, pos2(me.x, r.top())], Stroke::new(1.0, Color32::from_gray(200)));
    // North arrow and orientation.
    let north = vec2(-s, -c) * 10.0;
    let corner = pos2(r.right() - 20.0, r.top() + 22.0);
    p.arrow(corner - north, north * 2.0, Stroke::new(1.5, WHITE));
    text(p, pos2(r.right() - 6.0, r.top() + 6.0), Align2::RIGHT_TOP, "TRK UP", 7.5, WHITE);
    text(p, pos2(r.center().x, r.top() + 8.0), Align2::CENTER_CENTER, "MAP - NAVIGATION MAP", 8.0, WHITE);
    // The aircraft.
    let plane = [(0.0, -9.0), (2.0, -3.0), (9.0, 1.0), (9.0, 3.0), (2.0, 1.0), (1.5, 6.0), (4.0, 8.0), (-4.0, 8.0), (-1.5, 6.0), (-2.0, 1.0), (-9.0, 3.0), (-9.0, 1.0), (-2.0, -3.0)];
    let pts: Vec<Pos2> = plane.iter().map(|&(x, y)| me + vec2(x, y) * 1.2).collect();
    p.add(Shape::Path(epaint::PathShape::closed_line(pts.clone(), Stroke::new(1.0, Color32::BLACK))));
    for tri in [[0usize, 1, 12], [1, 2, 3], [1, 3, 4], [12, 11, 10], [12, 10, 9], [1, 4, 9], [1, 9, 12], [4, 5, 8], [4, 8, 9], [5, 6, 7], [5, 7, 8]] {
        p.add(Shape::convex_polygon(tri.iter().map(|&k| pts[k]).collect(), WHITE, Stroke::NONE));
    }
    let _ = d;
}
