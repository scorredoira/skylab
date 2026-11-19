//! The race in the game: the player against the rivals (`rivals.rs`) from a standing start, kept
//! by `sim_core::circuit::race::Race` (lights, laps, order, gaps, the flag); and what the player
//! sees of it (`overlay`): the start lights, their position and lap, the order with the gaps, and
//! the results; besides, the replay's bar, the ghost's gap and the rain.

use engine::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, Stroke};
use sim_core::circuit::race::{Phase, Race, LIGHTS};

use crate::game::Drive;
use crate::replay::SPEEDS;
use crate::timing::clock;

/// Seconds after the race is made before the first light comes on.
pub const LEAD_IN: f64 = 2.0;

/// Text with a dark shadow, for any background.
fn text(p: &egui::Painter, at: egui::Pos2, align: Align2, s: &str, size: f32, color: Color32) {
    p.text(at + vec2(1.5, 1.5), align, s, FontId::monospace(size), Color32::from_black_alpha(170));
    p.text(at, align, s, FontId::monospace(size), color);
}

/// A gap as the timing screens give it: +1.234, +1 LAP, or the leader's lap.
fn gap_text(race: &Race, ahead: usize, i: usize) -> String {
    let down = race.laps_down(ahead, i);
    if down > 0 && race.entrants[i].finished.is_none() {
        return format!("+{down} LAP{}", if down > 1 { "S" } else { "" });
    }
    match race.gap(ahead, i) {
        Some(g) => format!("+{g:.3}"),
        None => String::new(),
    }
}

/// What is drawn over the picture besides `hud.rs`'s: the race, the replay, the ghost, the rain.
pub fn overlay(ctx: &egui::Context, game: &Drive) {
    let p = ctx.layer_painter(egui::LayerId::background());
    let screen = ctx.content_rect();
    let white = Color32::from_rgb(240, 240, 235);
    let dim = Color32::from_rgb(170, 170, 165);
    if game.rain > 0.0 {
        text(&p, pos2(screen.right() - 30.0, screen.bottom() - 132.0), Align2::RIGHT_BOTTOM, &format!("RAIN {:.0}%", game.rain * 100.0), 13.0, Color32::from_rgb(140, 190, 255));
    }
    // The ghost: its lap, and the time between it and the player here.
    if let Some(best) = game.ghost.as_ref().and_then(|g| g.best.as_ref()) {
        let mut line = format!("ghost {}", clock(best.time));
        if let Some(d) = game.ghost_delta {
            line.push_str(&format!("   {d:+.2}"));
        }
        let color = match game.ghost_delta {
            Some(d) if d < 0.0 => Color32::from_rgb(110, 230, 120),
            Some(_) => Color32::from_rgb(255, 120, 100),
            None => dim,
        };
        text(&p, pos2(28.0, 186.0), Align2::LEFT_TOP, &line, 15.0, color);
    }
    // The replay's bar: where in what is kept, the speed, the keys.
    if let (Some(play), Some((first, last))) = (&game.playback, game.recorder.span()) {
        let bar = Rect::from_min_size(pos2(screen.center().x - 300.0, screen.bottom() - 60.0), vec2(600.0, 8.0));
        p.rect_filled(bar.expand(18.0), 6.0, Color32::from_black_alpha(140));
        p.rect_filled(bar, 3.0, Color32::from_white_alpha(40));
        let share = ((play.time - first) / (last - first).max(1e-6)).clamp(0.0, 1.0) as f32;
        p.rect_filled(Rect::from_min_size(bar.min, vec2(600.0 * share, 8.0)), 3.0, Color32::from_rgb(230, 70, 60));
        let state = if play.paused { "STOPPED" } else { "REPLAY" };
        text(&p, pos2(bar.min.x, bar.min.y - 6.0), Align2::LEFT_BOTTOM, &format!("{state}  {}  ×{}", clock(play.time - first), SPEEDS[play.speed]), 14.0, white);
        text(&p, pos2(bar.max.x, bar.min.y - 6.0), Align2::RIGHT_BOTTOM, "Space  ← →  ↓ ↑  [ ]  C  F2", 13.0, dim);
    }
    draw(ctx, game);
}

/// What is drawn of the race over the picture.
fn draw(ctx: &egui::Context, game: &Drive) {
    // In the replay the race is as it is now, not as it was: only the replay's bar is shown.
    let Some(race) = game.race.as_ref().filter(|_| game.playback.is_none()) else { return };
    let p = ctx.layer_painter(egui::LayerId::background());
    let screen = ctx.content_rect();
    let white = Color32::from_rgb(240, 240, 235);
    let dim = Color32::from_rgb(170, 170, 165);
    let player = 0;

    // The start lights: five pairs of red lamps over the middle of the picture, lit one a second,
    // all out at once for the start.
    if let Phase::Lights(lit) = race.phase() {
        let w = 64.0;
        let left = screen.center().x - w * LIGHTS as f32 / 2.0;
        let top = screen.top() + 70.0;
        let panel = Rect::from_min_size(pos2(left - 10.0, top - 10.0), vec2(w * LIGHTS as f32 + 20.0, 2.0 * w - 8.0));
        p.rect_filled(panel, 8.0, Color32::from_rgb(12, 12, 14));
        for k in 0..LIGHTS {
            for row in 0..2 {
                let c = pos2(left + w * (k as f32 + 0.5), top + w * (row as f32 + 0.4) - 6.0);
                let on = k < lit;
                let color = if on { Color32::from_rgb(255, 30, 20) } else { Color32::from_rgb(45, 8, 8) };
                if on {
                    p.circle_filled(c, 26.0, Color32::from_rgba_unmultiplied(255, 40, 20, 50));
                }
                p.circle_filled(c, 20.0, color);
            }
        }
    } else if race.clock() < 2.0 {
        text(&p, pos2(screen.center().x, screen.top() + 90.0), Align2::CENTER_CENTER, "GO", 54.0, Color32::from_rgb(90, 230, 90));
    }

    // Position and lap, top right.
    let order = race.order();
    let total = race.entrants.len();
    let right = screen.right() - 30.0;
    let position = race.position(player);
    text(&p, pos2(right, 40.0), Align2::RIGHT_TOP, &format!("P{position}/{total}"), 40.0, white);
    let me = &race.entrants[player];
    let lap = if me.finished.is_some() { "FINISHED".to_string() } else { format!("LAP {}/{}", race.lap(player), race.laps) };
    text(&p, pos2(right, 88.0), Align2::RIGHT_TOP, &lap, 18.0, dim);

    // The order, under the lap timing at the left.
    let left = 28.0;
    let mut y = 210.0;
    let row = 19.0;
    let board = Rect::from_min_size(pos2(left - 10.0, y - 8.0), vec2(300.0, row * total as f32 + 14.0));
    p.rect_filled(board, 6.0, Color32::from_black_alpha(110));
    for (place, &i) in order.iter().enumerate() {
        let e = &race.entrants[i];
        let mine = i == player;
        let color = if mine { Color32::from_rgb(255, 210, 90) } else if e.retired { Color32::from_rgb(150, 110, 100) } else { white };
        let gap = if e.retired {
            "OUT".to_string()
        } else if place == 0 {
            match e.finished {
                Some(t) => clock(t),
                None => format!("LAP {}", race.lap(i)),
            }
        } else {
            gap_text(race, order[0], i)
        };
        let flag = if e.finished.is_some() { " ▣" } else { "" };
        p.text(pos2(left, y), Align2::LEFT_TOP, format!("{:>2} {:<9}{flag}", place + 1, e.name), FontId::monospace(15.0), color);
        p.text(pos2(left + 280.0, y), Align2::RIGHT_TOP, gap, FontId::monospace(15.0), color);
        y += row;
    }

    // The results, once the player has the flag.
    if me.finished.is_some() || race.phase() == Phase::Over {
        let w = 520.0;
        let h = 70.0 + row * total as f32;
        let r = Rect::from_center_size(screen.center(), vec2(w, h));
        p.rect_filled(r, 8.0, Color32::from_black_alpha(185));
        p.rect_stroke(r, 8.0, Stroke::new(1.0, Color32::from_white_alpha(60)), egui::StrokeKind::Inside);
        let title = if race.phase() == Phase::Over { "RESULTS" } else { "RESULTS (the others are finishing)" };
        p.text(r.min + vec2(16.0, 12.0), Align2::LEFT_TOP, title, FontId::monospace(18.0), white);
        let mut y = r.min.y + 46.0;
        for (place, &i) in order.iter().enumerate() {
            let e = &race.entrants[i];
            let color = if i == player { Color32::from_rgb(255, 210, 90) } else { white };
            let time = match (e.finished, e.retired) {
                (_, true) => "retired".to_string(),
                (Some(t), _) if place == 0 => clock(t),
                (Some(_), _) => gap_text(race, order[0], i),
                (None, _) => "running".to_string(),
            };
            let best = e.best().map_or(String::new(), clock);
            p.text(pos2(r.min.x + 16.0, y), Align2::LEFT_TOP, format!("{:>2}  {:<10}{:>12}   best {best}", place + 1, e.name, time), FontId::monospace(15.0), color);
            y += row;
        }
        p.text(pos2(r.center().x, r.max.y - 8.0), Align2::CENTER_BOTTOM, "Shift+R: race again   F2: replay", FontId::monospace(13.0), dim);
    }
}
