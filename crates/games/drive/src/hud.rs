//! What is written over the picture: the speed, the gear and a rev counter at the bottom right, the
//! lap, its time and sectors, the last and best laps at the top left, the driving aids, a line said
//! for a moment, and the keys (H).

use engine::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Stroke};

use crate::game::Drive;
use crate::timing::clock;

const KEYS: &str = "\
↑ / W      throttle
↓ / S      brake
← → / A D  steer
E  Q       shift up, down (sequential: M)
Space      hand brake
C          camera: chase, bonnet, cockpit, trackside
R          back on the track   Shift+R  to the grid
X          the racing driver drives / you drive
M          automatic / sequential gearbox
B  T  Y    anti-lock brakes, traction control, stability control
G          pedal help: the keys ease as the tyres spin or lock
L          driving line: whole, only where to slow down, off
N          later in the day (Shift: earlier)
F2 / Tab   replay: Space stop, ← → 5 s, ↓ ↑ speed, F2 back
[  ]       watch the car before, after
F6  F7     more rain, less (stopped, the road dries)
F12        record a video with its sound; again: saved in Movies/Skylab
P          pause      H  these keys      Esc  menu";

/// Text with a dark shadow, for any background.
fn text(p: &egui::Painter, at: egui::Pos2, align: Align2, s: &str, size: f32, color: Color32) {
    p.text(at + vec2(1.5, 1.5), align, s, FontId::monospace(size), Color32::from_black_alpha(170));
    p.text(at, align, s, FontId::monospace(size), color);
}

pub fn draw(ctx: &egui::Context, game: &Drive) {
    let p = ctx.layer_painter(egui::LayerId::background());
    let screen = ctx.content_rect();
    let Some(car) = game.world.vehicle(game.id).car() else { return };
    let white = Color32::from_rgb(240, 240, 235);
    let dim = Color32::from_rgb(170, 170, 165);
    let amber = Color32::from_rgb(255, 190, 60);

    // Speed, gear, revs.
    let kmh = car.data().forward.abs() * 3.6;
    let right = screen.right() - 30.0;
    let bottom = screen.bottom() - 30.0;
    text(&p, pos2(right, bottom - 52.0), Align2::RIGHT_BOTTOM, &format!("{kmh:.0}"), 54.0, white);
    text(&p, pos2(right, bottom - 34.0), Align2::RIGHT_BOTTOM, "km/h", 14.0, dim);
    let electric = car.p.electric.is_some();
    let gear = match car.gear() {
        -1 => "R".to_string(),
        0 => "N".to_string(),
        _ if electric => "D".to_string(),
        g => g.to_string(),
    };
    let gear_color = if car.shifting() { dim } else { white };
    text(&p, pos2(right - 150.0, bottom - 52.0), Align2::RIGHT_BOTTOM, &gear, 54.0, gear_color);
    if !electric {
        let mode = if car.controls.automatic { "AUTO" } else { "SEQ" };
        text(&p, pos2(right - 150.0, bottom - 34.0), Align2::RIGHT_BOTTOM, mode, 14.0, dim);
    }
    let bar = Rect::from_min_size(pos2(right - 300.0, bottom - 14.0), vec2(300.0, 10.0));
    p.rect_filled(bar, 2.0, Color32::from_black_alpha(140));
    if let Some(e) = &car.p.electric {
        // The power meter, as the car's screen has it: what the motors draw to the right of a mark
        // a quarter of the way along, what they give back (green) to its left.
        let kw = car.data().power / 1000.0;
        let zero = bar.min.x + 75.0;
        let (from, to, color) = if kw >= 0.0 {
            (zero, zero + 225.0 * (kw / (e.battery_power / 1000.0)).min(1.0) as f32, Color32::from_rgb(230, 230, 225))
        } else {
            (zero - 75.0 * (-kw / (e.regen_power / 1000.0)).min(1.0) as f32, zero, Color32::from_rgb(90, 200, 90))
        };
        p.rect_filled(Rect::from_x_y_ranges(from..=to, bar.min.y..=bar.max.y), 2.0, color);
        p.line_segment([pos2(zero, bar.min.y - 3.0), pos2(zero, bar.max.y + 3.0)], Stroke::new(2.0, dim));
        text(&p, pos2(bar.min.x, bar.min.y - 6.0), Align2::LEFT_BOTTOM, &format!("{kw:.0} kW"), 13.0, dim);
    } else {
        // The rev counter: a bar, amber from the shift light, red at the limiter.
        let e = &car.p.engine;
        let share = (car.rpm() / e.limiter_rpm).clamp(0.0, 1.0) as f32;
        let color = if car.data().limiter {
            Color32::from_rgb(255, 60, 40)
        } else if car.rpm() > e.redline_rpm {
            amber
        } else {
            Color32::from_rgb(230, 230, 225)
        };
        p.rect_filled(Rect::from_min_size(bar.min, vec2(300.0 * share, 10.0)), 2.0, color);
        let red = (e.redline_rpm / e.limiter_rpm) as f32;
        p.line_segment([pos2(bar.min.x + 300.0 * red, bar.min.y - 3.0), pos2(bar.min.x + 300.0 * red, bar.max.y + 3.0)], Stroke::new(2.0, Color32::from_rgb(220, 40, 30)));
        text(&p, pos2(bar.min.x, bar.min.y - 6.0), Align2::LEFT_BOTTOM, &format!("{:.0} rpm", car.rpm()), 13.0, dim);
    }
    let mut aids = Vec::new();
    if car.controls.abs {
        aids.push("ABS");
    }
    if car.controls.traction_control {
        aids.push("TC");
    }
    if car.controls.stability_control {
        aids.push("ESC");
    }
    if game.driver.is_some() {
        aids.push("AI DRIVING");
    }
    text(&p, pos2(right, bar.min.y - 6.0), Align2::RIGHT_BOTTOM, &aids.join("  "), 13.0, amber);

    // The lap.
    let t = &game.timing;
    let mut y = 24.0;
    let left = 28.0;
    let line = |s: &str, size: f32, color: Color32, y: &mut f32| {
        text(&p, pos2(left, *y), Align2::LEFT_TOP, s, size, color);
        *y += size + 6.0;
    };
    let time = game.world.time();
    match t.current(time) {
        Some(now) => {
            line(&format!("LAP {}", t.lap), 16.0, dim, &mut y);
            let color = if t.invalid { Color32::from_rgb(255, 110, 90) } else { white };
            line(&clock(now), 30.0, color, &mut y);
            let sectors: Vec<String> = t.sectors.iter().map(|s| format!("{s:.2}")).collect();
            if !sectors.is_empty() {
                line(&format!("sectors {}", sectors.join("  ")), 14.0, dim, &mut y);
            }
        }
        None => line(&format!("{}  ·  to the line", game.circuit.name), 16.0, dim, &mut y),
    }
    if let Some(last) = t.last {
        let mark = if t.last_invalid { "  (track limits)" } else { "" };
        line(&format!("last {}{mark}", clock(last)), 15.0, dim, &mut y);
    }
    if let Some(best) = t.best {
        line(&format!("best {}", clock(best)), 15.0, Color32::from_rgb(170, 120, 255), &mut y);
    }

    if let Some(why) = car.crashed() {
        text(&p, pos2(screen.center().x, screen.center().y - 60.0), Align2::CENTER_CENTER, why, 30.0, Color32::from_rgb(255, 90, 70));
        text(&p, pos2(screen.center().x, screen.center().y - 25.0), Align2::CENTER_CENTER, "R: back on the track", 16.0, white);
    }
    if game.paused && game.menu.is_none() {
        text(&p, screen.center(), Align2::CENTER_CENTER, "PAUSED", 30.0, white);
    }
    if let Some((m, _)) = &game.message {
        text(&p, pos2(screen.center().x, screen.top() + 30.0), Align2::CENTER_TOP, m, 18.0, white);
    }
    crate::race::overlay(ctx, game);
    if game.help {
        let r = Rect::from_min_size(pos2(left - 10.0, y + 10.0), vec2(470.0, 346.0));
        p.rect_filled(r, 6.0, Color32::from_black_alpha(150));
        p.text(r.min + vec2(12.0, 12.0), Align2::LEFT_TOP, KEYS, FontId::monospace(14.0), white);
    } else {
        text(&p, pos2(screen.right() - 20.0, screen.top() + 16.0), Align2::RIGHT_TOP, "H: keys", 13.0, dim);
    }
}
