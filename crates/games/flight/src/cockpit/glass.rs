//! The displays of a tilt-rotor's cockpit: three screens of thin lines on black, drawn from
//! scratch each frame with egui's painter, each in the rectangle its cockpit's file gives it
//! (`instruments::Instrument::FlightDisplay`, `SystemsDisplay`, `SituationDisplay`).
//!
//! - **Flight**: the horizon across the whole screen with its pitch ladder and the mark of
//!   where it is going, the speed on a tape at the left, the height at the right with the
//!   height over the ground and the climb, the heading along the top, and at the foot the rings
//!   seen from the side, tilted as the nacelles are.
//! - **Systems**: the engines' torque and the rotors' speed as bars against their limits, the
//!   fuel with how long it lasts, the collective, the flight controller's help, and what is wrong.
//! - **Situation**: the ground from above, the nose up: rings of distance, the way it is going,
//!   and the aerodrome it started from with its runway.
//!
//! Plain numbers in (`PanelData`, `Rotorcraft`): nothing here knows the simulation.

use super::instruments::Rotorcraft;
use crate::ui::panel::PanelData;
use engine::egui::{pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

const BACK: Color32 = Color32::from_rgb(2, 6, 9);
const LINE: Color32 = Color32::from_rgb(70, 225, 235);
const DIM: Color32 = Color32::from_rgb(30, 95, 105);
const FAINT: Color32 = Color32::from_rgb(14, 40, 46);
const WHITE: Color32 = Color32::from_rgb(235, 245, 248);
const AMBER: Color32 = Color32::from_rgb(255, 176, 30);
const RED: Color32 = Color32::from_rgb(255, 60, 50);
const GREEN: Color32 = Color32::from_rgb(70, 235, 120);
const SKY: Color32 = Color32::from_rgb(6, 26, 44);
const GROUND: Color32 = Color32::from_rgb(16, 22, 18);

fn text(p: &Painter, at: Pos2, align: Align2, s: &str, size: f32, color: Color32) {
    p.text(at, align, s, FontId::monospace(size), color);
}

fn line(p: &Painter, a: Pos2, b: Pos2, width: f32, color: Color32) {
    p.line_segment([a, b], Stroke::new(width, color));
}

/// A screen's ground: black, a thin frame, its name at the top left.
fn screen(p: &Painter, r: Rect, name: &str) -> f32 {
    let u = r.height() / 100.0;
    p.rect_filled(r, 0.0, BACK);
    text(p, r.left_top() + vec2(2.0 * u, 1.5 * u), Align2::LEFT_TOP, name, 3.2 * u, DIM);
    u
}

/// The flight display.
pub fn flight(p: &Painter, r: Rect, d: &PanelData, k: &Rotorcraft) {
    let p = &p.with_clip_rect(r);
    let u = r.height() / 100.0;
    let c = r.center();
    // The horizon: the sky a little lighter than the ground, a bright line between, turned by
    // the bank and moved by the pitch (2.2 units a degree).
    let per_degree = 2.2 * u;
    let (sin, cos) = d.roll.sin_cos();
    let along = vec2(cos, sin);
    let down = vec2(-sin, cos);
    let level = c + down * (d.pitch.to_degrees() * per_degree);
    let far = r.width() + r.height();
    p.rect_filled(r, 0.0, GROUND);
    p.add(Shape::convex_polygon(vec![level - along * far, level + along * far, level + along * far - down * far, level - along * far - down * far], SKY, Stroke::NONE));
    line(p, level - along * far, level + along * far, 0.5 * u, LINE);
    // The pitch ladder, every 5°, the tens numbered.
    for step in (-90i32..=90).step_by(5).filter(|s| *s != 0) {
        let at = level - down * (step as f32 * per_degree);
        if (at - c).length() > 34.0 * u {
            continue;
        }
        let half = if step % 10 == 0 { 9.0 * u } else { 4.5 * u };
        line(p, at - along * half, at - along * 2.5 * u, 0.35 * u, if step > 0 { LINE } else { DIM });
        line(p, at + along * 2.5 * u, at + along * half, 0.35 * u, if step > 0 { LINE } else { DIM });
        if step % 10 == 0 {
            text(p, at + along * (half + 3.0 * u), Align2::CENTER_CENTER, &step.abs().to_string(), 3.0 * u, DIM);
        }
    }
    // The bank: marks round the top and a pointer.
    for mark in [-60, -45, -30, -20, -10, 0, 10, 20, 30, 45, 60] {
        let a = (mark as f32).to_radians();
        let dir = vec2(a.sin(), -a.cos());
        line(p, c + dir * 36.0 * u, c + dir * (if mark % 30 == 0 { 39.5 } else { 38.0 }) * u, 0.35 * u, DIM);
    }
    let dir = vec2(-sin, -cos);
    let tip = c + dir * 35.5 * u;
    p.add(Shape::convex_polygon(vec![tip, tip - dir * 3.0 * u + vec2(dir.y, -dir.x) * 1.6 * u, tip - dir * 3.0 * u - vec2(dir.y, -dir.x) * 1.6 * u], LINE, Stroke::NONE));
    // Itself: wings either side of the middle; and where it is going, a ring with three ticks,
    // as far below the nose as it sinks.
    for side in [-1.0, 1.0] {
        line(p, c + vec2(side * 4.0 * u, 0.0), c + vec2(side * 12.0 * u, 0.0), 0.8 * u, AMBER);
        line(p, c + vec2(side * 4.0 * u, 0.0), c + vec2(side * 4.0 * u, 2.0 * u), 0.8 * u, AMBER);
    }
    p.circle_filled(c, 0.7 * u, AMBER);
    let speed = d.airspeed_kt.max(8.0) * 101.27;
    let path = (d.vertical_speed_fpm / speed).atan().to_degrees().clamp(-20.0, 20.0);
    let going = level - down * (path * per_degree);
    p.circle_stroke(going, 1.8 * u, Stroke::new(0.45 * u, GREEN));
    for tick in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0)] {
        line(p, going + tick * 1.8 * u, going + tick * 4.0 * u, 0.45 * u, GREEN);
    }

    // The tapes: the speed at the left, the height at the right, each a strip of marks moving
    // past a box with the figure.
    let tape = |x: f32, value: f32, step: f32, per: f32, right: bool, unit: &str| {
        let strip = Rect::from_center_size(pos2(x, c.y), vec2(13.0 * u, 56.0 * u));
        p.rect_filled(strip, 0.0, Color32::from_rgba_unmultiplied(2, 6, 9, 170));
        let pc = p.with_clip_rect(strip.intersect(r));
        let first = ((value - 28.0 * u / per) / step).floor() as i32;
        for n in first..first + (56.0 * u / per / step) as i32 + 3 {
            let v = n as f32 * step;
            if v < 0.0 && !right {
                continue;
            }
            let y = c.y - (v - value) * per;
            let edge = if right { strip.left() } else { strip.right() };
            let into = if right { 1.0 } else { -1.0 };
            let major = n % 2 == 0;
            line(&pc, pos2(edge, y), pos2(edge + into * if major { 2.6 } else { 1.4 } * u, y), 0.3 * u, DIM);
            if major {
                text(&pc, pos2(edge + into * 3.4 * u, y), if right { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER }, &format!("{v:.0}"), 3.0 * u, DIM);
            }
        }
        let frame = Rect::from_center_size(pos2(x, c.y), vec2(15.0 * u, 7.0 * u));
        p.rect_filled(frame, 0.6 * u, BACK);
        p.rect_stroke(frame, 0.6 * u, Stroke::new(0.4 * u, LINE), engine::egui::StrokeKind::Inside);
        text(p, frame.center(), Align2::CENTER_CENTER, &format!("{value:.0}"), 4.6 * u, WHITE);
        text(p, pos2(x, strip.top() - 2.5 * u), Align2::CENTER_CENTER, unit, 2.8 * u, DIM);
    };
    tape(r.left() + 10.0 * u, d.airspeed_kt.max(0.0), 10.0, 0.9 * u, false, "KT");
    tape(r.right() - 11.0 * u, d.altitude_ft, 100.0, 0.09 * u, true, "FT");
    // Over the ground, and the climb.
    let low = k.agl_ft < 50.0;
    text(p, pos2(r.right() - 11.0 * u, c.y + 33.0 * u), Align2::CENTER_CENTER, &format!("RA {:.0}", k.agl_ft.max(0.0)), 3.6 * u, if low { AMBER } else { GREEN });
    text(p, pos2(r.right() - 11.0 * u, c.y + 38.0 * u), Align2::CENTER_CENTER, &format!("VS {:+.0}", (d.vertical_speed_fpm / 10.0).round() * 10.0), 3.2 * u, WHITE);

    // The heading along the top: a strip of marks, the figure in a box.
    let heading = d.heading.unwrap_or(d.compass).to_degrees().rem_euclid(360.0);
    let strip = Rect::from_center_size(pos2(c.x, r.top() + 7.0 * u), vec2(70.0 * u, 7.0 * u));
    let pc = p.with_clip_rect(strip.intersect(r));
    for n in ((heading - 40.0) / 5.0).floor() as i32..((heading + 40.0) / 5.0).ceil() as i32 {
        let v = n as f32 * 5.0;
        let x = c.x + (v - heading) * 0.9 * u;
        let major = n % 2 == 0;
        line(&pc, pos2(x, strip.bottom()), pos2(x, strip.bottom() - if major { 2.4 } else { 1.2 } * u), 0.3 * u, DIM);
        if n % 6 == 0 {
            let name = match (v.rem_euclid(360.0) / 30.0).round() as i32 % 12 {
                0 => "N".to_string(),
                3 => "E".to_string(),
                6 => "S".to_string(),
                9 => "W".to_string(),
                other => (other * 3).to_string(),
            };
            text(&pc, pos2(x, strip.top() + 1.6 * u), Align2::CENTER_CENTER, &name, 3.0 * u, DIM);
        }
    }
    let frame = Rect::from_center_size(pos2(c.x, r.top() + 13.5 * u), vec2(13.0 * u, 5.6 * u));
    p.rect_filled(frame, 0.6 * u, BACK);
    p.rect_stroke(frame, 0.6 * u, Stroke::new(0.4 * u, LINE), engine::egui::StrokeKind::Inside);
    text(p, frame.center(), Align2::CENTER_CENTER, &format!("{:03.0}", heading.round() % 360.0), 4.0 * u, WHITE);

    // The rings from the side: the frame level, a ring each end of its axis tilted as they are.
    let at = pos2(c.x, r.bottom() - 9.0 * u);
    let a = k.nacelles.to_radians();
    let axis = vec2(a.sin(), -a.cos());
    let across = vec2(axis.y, -axis.x);
    line(p, at - vec2(9.0 * u, 0.0), at + vec2(9.0 * u, 0.0), 0.4 * u, DIM);
    line(p, at, at + axis * 3.0 * u, 0.4 * u, DIM);
    line(p, at + axis * 3.0 * u - across * 7.0 * u, at + axis * 3.0 * u + across * 7.0 * u, 0.9 * u, LINE);
    text(p, at + vec2(14.0 * u, 0.0), Align2::LEFT_CENTER, &format!("NAC {:.0}\u{b0}", k.nacelles), 3.4 * u, WHITE);
    text(p, at - vec2(14.0 * u, 0.0), Align2::RIGHT_CENTER, &format!("TQ {:.0}", k.torque * 100.0), 3.4 * u, if k.torque > 0.95 { AMBER } else { WHITE });
    if k.rotors < 0.9 {
        text(p, pos2(c.x, c.y + 22.0 * u), Align2::CENTER_CENTER, "LOW ROTOR RPM", 5.0 * u, RED);
    }
}

/// A bar of a share against its limits: its name over it, the figure under it, the green band
/// lit as far as it reads and amber past `caution`, red past `limit`.
#[allow(clippy::too_many_arguments)]
fn bar(p: &Painter, r: Rect, name: &str, value: f32, top: f32, caution: f32, limit: f32, figure: &str) {
    let u = r.width() / 10.0;
    let well = Rect::from_min_max(pos2(r.left() + 2.5 * u, r.top() + 4.5 * u), pos2(r.right() - 2.5 * u, r.bottom() - 6.0 * u));
    p.rect_filled(well, 0.0, FAINT);
    let y = |v: f32| well.bottom() - well.height() * (v / top).clamp(0.0, 1.0);
    let color = if value >= limit { RED } else if value >= caution { AMBER } else { LINE };
    p.rect_filled(Rect::from_min_max(pos2(well.left(), y(value)), well.max), 0.0, color);
    for (v, c) in [(caution, AMBER), (limit, RED)].into_iter().filter(|(v, _)| *v > 0.0 && *v < top) {
        line(p, pos2(well.left() - 1.2 * u, y(v)), pos2(well.right() + 1.2 * u, y(v)), 0.25 * u, c);
    }
    text(p, pos2(r.center().x, r.top() + 2.0 * u), Align2::CENTER_CENTER, name, 2.6 * u, DIM);
    text(p, pos2(r.center().x, r.bottom() - 3.0 * u), Align2::CENTER_CENTER, figure, 3.6 * u, WHITE);
}

/// The systems display.
pub fn systems(p: &Painter, r: Rect, d: &PanelData, k: &Rotorcraft) {
    let p = &p.with_clip_rect(r);
    let u = screen(p, r, "SYSTEMS");
    let w = r.width();
    let column = |i: usize| Rect::from_min_size(pos2(r.left() + w * (0.04 + 0.235 * i as f32), r.top() + 8.0 * u), vec2(w * 0.215, 50.0 * u));
    bar(p, column(0), "TQ %", k.torque, 1.2, 0.9, 1.0, &format!("{:.0}", k.torque * 100.0));
    // The rotors: low is what is wrong (drawn the other way: a limit under which it is red).
    let low = k.rotors < 0.9;
    bar(p, column(1), "NR %", k.rotors, 1.2, if low { 0.0 } else { 1.04 }, if low { 0.0 } else { 1.1 }, &format!("{:.0}", k.rotors * 100.0));
    bar(p, column(2), "COL %", d.throttle, 1.0, 2.0, 2.0, &format!("{:.0}", d.throttle * 100.0));
    let fuel_low = k.fuel < 0.1;
    bar(p, column(3), "FUEL", k.fuel, 1.0, if fuel_low { 0.0 } else { 2.0 }, 2.0, &format!("{:.0}", k.fuel_kg));

    // Under them: how long the fuel lasts, the engines, the help, and what is wrong.
    let left = r.left() + w * 0.05;
    let mut y = r.top() + 63.0 * u;
    let mut row = |label: &str, value: String, color: Color32| {
        text(p, pos2(left, y), Align2::LEFT_CENTER, label, 3.2 * u, DIM);
        text(p, pos2(r.right() - w * 0.05, y), Align2::RIGHT_CENTER, &value, 3.6 * u, color);
        y += 5.4 * u;
    };
    let (hours, minutes) = ((k.endurance_min / 60.0).floor(), k.endurance_min % 60.0);
    row("ENDURANCE", if k.engines { format!("{hours:.0}:{minutes:02.0}") } else { "--:--".to_string() }, if fuel_low { AMBER } else { WHITE });
    row("ENGINES", if k.engines { "RUN".to_string() } else { "OFF".to_string() }, if k.engines { GREEN } else { RED });
    row("SAS", ["RATES", "LEVEL", "OFF", "HOLD"][k.stability.min(3) as usize].to_string(), if k.stability == 2 { AMBER } else { GREEN });
    row("NACELLES", format!("{:.0}\u{b0}", k.nacelles), WHITE);
    let wrong: Vec<(&str, Color32)> = [("ENG OUT", !k.engines, RED), ("LOW ROTOR RPM", low, RED), ("LOW FUEL", fuel_low, AMBER), ("TORQUE", k.torque > 1.0, AMBER)].iter().filter(|w| w.1).map(|w| (w.0, w.2)).collect();
    for (i, (what, color)) in wrong.iter().enumerate() {
        text(p, pos2(r.center().x, r.bottom() - (4.0 + 5.0 * i as f32) * u), Align2::CENTER_CENTER, what, 4.0 * u, *color);
    }
}

/// The situation display.
pub fn situation(p: &Painter, r: Rect, d: &PanelData, k: &Rotorcraft) {
    let p = &p.with_clip_rect(r);
    let u = screen(p, r, "SITUATION");
    let c = pos2(r.center().x, r.top() + r.height() * 0.6);
    let heading = d.heading.unwrap_or(d.compass);
    // The scale: the outer ring is the first of 1, 2, 5, 10... nautical miles past the aerodrome.
    let away_nm = k.home.map_or(1.0, |h| h.1 / 1852.0);
    let range = [1.0f32, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0].into_iter().find(|s| *s > away_nm * 1.15).unwrap_or(200.0);
    let outer = r.height() * 0.34;
    for share in [0.5, 1.0] {
        p.circle_stroke(c, outer * share, Stroke::new(0.3 * u, DIM));
        text(p, c + vec2(outer * share * 0.72, outer * share * 0.72), Align2::LEFT_TOP, &format!("{:.0}", range * share), 2.8 * u, DIM);
    }
    // North, east, south and west round the outer ring, the nose up.
    for (name, bearing) in [("N", 0.0f32), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
        let a = bearing.to_radians() - heading;
        text(p, c + vec2(a.sin(), -a.cos()) * (outer + 4.0 * u), Align2::CENTER_CENTER, name, 3.4 * u, LINE);
    }
    // The way it goes over the ground, as far as it gets in a minute.
    let track = k.track - heading;
    let minute = (k.ground_speed_kt / 60.0 / range * outer).min(outer);
    line(p, c, c + vec2(track.sin(), -track.cos()) * minute, 0.4 * u, GREEN);
    // The aerodrome: a circle with its runway through it.
    if let Some((bearing, metres, runway)) = k.home {
        let a = bearing - heading;
        let at = c + vec2(a.sin(), -a.cos()) * (metres / 1852.0 / range * outer);
        let along = runway - heading;
        let dir = vec2(along.sin(), -along.cos());
        p.circle_stroke(at, 2.2 * u, Stroke::new(0.4 * u, WHITE));
        line(p, at - dir * 4.0 * u, at + dir * 4.0 * u, 0.7 * u, WHITE);
        text(p, pos2(r.left() + 2.0 * u, r.bottom() - 3.0 * u), Align2::LEFT_CENTER, &format!("HOME {:03.0}\u{b0} {:.1} NM", bearing.to_degrees().rem_euclid(360.0), metres / 1852.0), 3.2 * u, WHITE);
    }
    // Itself, in the middle.
    p.add(Shape::convex_polygon(vec![c + vec2(0.0, -3.0 * u), c + vec2(2.0 * u, 2.4 * u), c + vec2(0.0, 1.2 * u), c + vec2(-2.0 * u, 2.4 * u)], AMBER, Stroke::NONE));
    text(p, pos2(r.right() - 2.0 * u, r.top() + 1.5 * u), Align2::RIGHT_TOP, &format!("GS {:.0} KT", k.ground_speed_kt), 3.2 * u, WHITE);
    text(p, pos2(r.right() - 2.0 * u, r.bottom() - 3.0 * u), Align2::RIGHT_CENTER, &format!("TRK {:03.0}\u{b0}", k.track.to_degrees().rem_euclid(360.0)), 3.2 * u, WHITE);
}
