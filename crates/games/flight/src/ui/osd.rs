//! A multirotor's on-screen display, as an FPV pilot's goggles show what its flight controller
//! writes over the camera's picture: white capitals edged in black on a grid of characters, the
//! battery at the top left (voltage, a cell's, current, what has been used), the heading at the
//! top, the time armed at the top right, a crosshair and, through the camera, the
//! horizon in the middle, DISARMED and why it does not arm, warnings that blink, the height and
//! the climb at the bottom left, the throttle at the bottom and the speed at the bottom right.
//! Plain numbers in (`Osd`), so it knows nothing of the simulation.

use engine::egui::{pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

/// What the display shows: plain numbers, SI units unless the name says otherwise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Osd {
    pub armed: bool,
    /// Why it does not arm while the switch is on.
    pub blocked: Option<&'static str>,
    /// The battery: volts, a cell's volts, amps, mAh used.
    pub volts: f32,
    pub cell: f32,
    pub amps: f32,
    pub used_mah: f32,
    /// Height above where it was armed and above the ground, m; climb, m/s; speed, km/h.
    pub altitude: f32,
    pub agl: f32,
    pub climb: f32,
    pub speed_kmh: f32,
    /// The throttle stick, 0...1.
    pub throttle: f32,
    /// Seconds armed.
    pub time: f32,
    /// Attitude, radians: the horizon is drawn from it through a camera tilted up `uptilt`.
    pub pitch: f32,
    pub roll: f32,
    pub heading: f32,
    pub uptilt: f32,
    /// The camera's field of view corner to corner, radians.
    pub fov: f32,
    /// A tilt-rotor's nacelles, radians forward of upright.
    pub nacelles: Option<f32>,
    /// Turbines, shown in the battery's place: the rotors' speed and the engines' torque (1 is
    /// all of each), and the fuel left, kg.
    pub engines: Option<(f32, f32, f32)>,
    /// The help the flight controller gives, if it is the pilot's to choose: LEVEL, RATES, OFF.
    pub stability: Option<&'static str>,
    /// Whether the view is the camera's: then the horizon line is drawn.
    pub horizon: bool,
    /// A warning that blinks: LOW BATTERY, CRASH.
    pub warning: Option<&'static str>,
    /// The hour of the day, for the blinking.
    pub clock: f32,
}

const WHITE: Color32 = Color32::from_rgb(250, 250, 250);
const EDGE: Color32 = Color32::from_rgb(0, 0, 0);

/// Draws the display over the whole of `screen`.
pub fn draw(painter: &Painter, screen: Rect, osd: &Osd) {
    let painter = &painter.with_clip_rect(screen);
    // A character of the grid: 30 columns by 16 rows, as the analogue displays had them.
    let size = (screen.height() / 24.0).clamp(12.0, 30.0);
    let font = FontId::monospace(size);
    let (left, right) = (screen.left() + screen.width() * 0.06, screen.right() - screen.width() * 0.06);
    let (top, bottom) = (screen.top() + screen.height() * 0.13, screen.bottom() - screen.height() * 0.08);
    let line = size * 1.15;
    let text = |at: Pos2, align: Align2, s: &str| edged(painter, at, align, s, &font);

    // The battery, or the engines.
    if let Some((rotors, torque, fuel)) = osd.engines {
        text(pos2(left, top), Align2::LEFT_TOP, &format!("NR {:.0}%", rotors * 100.0));
        text(pos2(left, top + line), Align2::LEFT_TOP, &format!("TQ {:.0}%", torque * 100.0));
        text(pos2(left, top + 2.0 * line), Align2::LEFT_TOP, &format!("FUEL {:.0}KG", fuel));
    } else {
        battery_icon(painter, pos2(left, top), size, osd.cell);
        text(pos2(left + size * 1.2, top), Align2::LEFT_TOP, &format!("{:.1}V", osd.volts));
        text(pos2(left, top + line), Align2::LEFT_TOP, &format!("{:.2}V", osd.cell));
        text(pos2(left, top + 2.0 * line), Align2::LEFT_TOP, &format!("{:.1}A", osd.amps));
        text(pos2(left, top + 3.0 * line), Align2::LEFT_TOP, &format!("{:.0}MAH", osd.used_mah));
    }
    if let Some(stability) = osd.stability {
        text(pos2(right, top + line), Align2::RIGHT_TOP, &format!("SAS {stability}"));
    }

    // The heading; the time armed.
    let middle = screen.center().x;
    text(pos2(middle, top), Align2::CENTER_TOP, &format!("{:03.0}", osd.heading.to_degrees().rem_euclid(360.0)));
    let seconds = osd.time.max(0.0) as u32;
    text(pos2(right, top), Align2::RIGHT_TOP, &format!("{:02}:{:02}", seconds / 60, seconds % 60));

    // Through the camera, the crosshair and the horizon.
    let centre = screen.center();
    for (width, colour) in [(size * 0.22, EDGE), (size * 0.09, WHITE)].into_iter().filter(|_| osd.horizon) {
        let s = Stroke::new(width, colour);
        painter.line_segment([centre - vec2(size * 0.9, 0.0), centre - vec2(size * 0.3, 0.0)], s);
        painter.line_segment([centre + vec2(size * 0.3, 0.0), centre + vec2(size * 0.9, 0.0)], s);
        painter.line_segment([centre - vec2(0.0, size * 0.25), centre - vec2(0.0, size * 0.55)], s);
    }
    if osd.horizon && osd.armed {
        horizon(painter, screen, size, osd);
    }

    // Disarmed, and why; warnings, blinking. Behind it, clear of it in the middle of the picture.
    let below = if osd.horizon { centre.y } else { screen.top() + screen.height() * 0.62 };
    if !osd.armed {
        text(pos2(middle, below + size * 2.0), Align2::CENTER_CENTER, if osd.engines.is_some() { "ENGINES OFF" } else { "DISARMED" });
        if let Some(why) = osd.blocked {
            let what = if osd.engines.is_some() { "START BLOCKED" } else { "ARMING BLOCKED" };
            text(pos2(middle, below + size * 3.3), Align2::CENTER_CENTER, &format!("{what}: {why}"));
        }
    }
    let blink = (osd.clock * 3600.0 * 2.0).fract() < 0.6;
    if let (Some(warning), true) = (osd.warning, blink) {
        text(pos2(middle, below + size * 4.6), Align2::CENTER_CENTER, warning);
    }

    // Height and climb, throttle, speed.
    text(pos2(left, bottom - line), Align2::LEFT_BOTTOM, &format!("ALT {:.1}M", osd.altitude));
    text(pos2(left, bottom), Align2::LEFT_BOTTOM, &format!("VS {:+.1}", osd.climb));
    text(pos2(middle, bottom), Align2::CENTER_BOTTOM, &format!("{} {:.0}", if osd.engines.is_some() { "COL" } else { "THR" }, osd.throttle * 100.0));
    if let Some(nacelles) = osd.nacelles {
        text(pos2(middle, bottom - line), Align2::CENTER_BOTTOM, &format!("NAC {:.0}", nacelles.to_degrees()));
    }
    text(pos2(right, bottom), Align2::RIGHT_BOTTOM, &format!("{:.0}KM/H", osd.speed_kmh));
}

/// The horizon as the camera sees it: a broken line through where the level is, turned by the bank.
fn horizon(painter: &Painter, screen: Rect, size: f32, osd: &Osd) {
    // The camera's field of view top to bottom, of its own across 16:9 (`multirotor::spots`).
    let fov_y = 2.0 * ((osd.fov / 2.0).tan() * 9.0 / 16.0).atan();
    let below = -(osd.pitch + osd.uptilt);
    let focal = screen.height() / 2.0 / (fov_y / 2.0).tan();
    let offset = (focal * below.tan()).clamp(-screen.height() * 0.3, screen.height() * 0.3);
    let along = vec2(osd.roll.cos(), -osd.roll.sin());
    let down = vec2(osd.roll.sin(), osd.roll.cos());
    let middle = screen.center() - down * offset;
    let half = screen.width() * 0.16;
    for (width, colour) in [(size * 0.2, EDGE), (size * 0.08, WHITE)] {
        let mut k = -half;
        while k < half {
            let a = middle + along * k;
            let b = middle + along * (k + size * 0.5).min(half);
            // Leave the crosshair clear.
            if (a - screen.center()).length() > size * 1.2 || (b - screen.center()).length() > size * 1.2 {
                painter.line_segment([a, b], Stroke::new(width, colour));
            }
            k += size * 0.8;
        }
    }
}

/// A battery, filled as much as the cell's voltage says (3.3 V empty to 4.2 V full).
fn battery_icon(painter: &Painter, at: Pos2, size: f32, cell: f32) {
    let body = Rect::from_min_size(at + vec2(0.0, size * 0.18), vec2(size * 0.95, size * 0.62));
    let cap = Rect::from_min_size(pos2(body.right(), body.center().y - size * 0.12), vec2(size * 0.1, size * 0.24));
    let full = ((cell - 3.3) / 0.9).clamp(0.0, 1.0);
    painter.rect_filled(body.expand(size * 0.07), 1.0, EDGE);
    painter.rect_filled(cap.expand(size * 0.05), 0.0, EDGE);
    painter.add(Shape::rect_stroke(body, 1.0, Stroke::new(size * 0.08, WHITE), engine::egui::StrokeKind::Inside));
    painter.rect_filled(cap, 0.0, WHITE);
    let inner = body.shrink(size * 0.14);
    painter.rect_filled(Rect::from_min_size(inner.min, vec2(inner.width() * full, inner.height())), 0.0, WHITE);
}

/// White text edged in black, as the displays write it.
fn edged(painter: &Painter, at: Pos2, align: Align2, text: &str, font: &FontId) {
    let edge = (font.size * 0.08).max(1.0);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7)] {
        painter.text(at + vec2(dx, dy) * edge, align, text, font.clone(), EDGE);
    }
    painter.text(at, align, text, font.clone(), WHITE);
}

/// Where the next gate is on the picture: x and y from −1 to 1 (right, up), on the picture or on
/// its edge the way to turn, and how far, m.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Marker {
    pub x: f32,
    pub y: f32,
    pub on_screen: bool,
    pub distance: f32,
}

/// What a race shows over the display (`race::RaceSession::osd`): plain numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaceOsd {
    pub name: String,
    /// 3, 2, 1, then 0 for GO for a second.
    pub countdown: Option<u32>,
    /// The race's time, s; the lap's, the lap flown (1 the first) of so many.
    pub time: f32,
    pub lap_time: f32,
    pub lap: u32,
    pub laps: u32,
    /// The gate to fly through next: `3/15`, START or FINISH.
    pub gate: String,
    pub last_lap: Option<f32>,
    pub best: Option<f32>,
    pub new_best: bool,
    /// The last gate's time into the lap and its difference from the last lap's.
    pub split: Option<(f32, Option<f32>)>,
    /// A gate missed, while it is said.
    pub missed: Option<usize>,
    pub marker: Option<Marker>,
    /// At the finish: each lap's time and the race's.
    pub results: Option<(Vec<f32>, f32)>,
    pub crashed: bool,
    /// The autopilot flies the race.
    pub racer: bool,
    /// Seconds of the world, for the blinking.
    pub clock: f32,
    /// With quads flown by the game in the race: the place of the quad watched, of so many (P1/3).
    pub position: Option<(usize, usize)>,
    /// Everyone in the race, the first first.
    pub standings: Vec<Standing>,
    /// The name of each other quad seen, over it.
    pub labels: Vec<Label>,
    /// The rival whose race is shown (the camera is on its quad), if it is not the player's.
    pub watching: Option<String>,
}

/// One line of the standings: who, in its colour, and how far it is (LAP 2 7/15, its time, CRASHED).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Standing {
    pub name: String,
    pub colour: [u8; 3],
    pub detail: String,
    /// The camera is on it.
    pub watched: bool,
}

/// A quad's name over it on the picture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Label {
    pub marker: Marker,
    pub name: String,
    pub colour: [u8; 3],
}

const GREEN: Color32 = Color32::from_rgb(90, 255, 120);
const RED: Color32 = Color32::from_rgb(255, 80, 70);
const YELLOW: Color32 = Color32::from_rgb(255, 220, 60);

/// A time as minutes, seconds and hundredths: 1:23.45.
pub fn lap_time(t: f32) -> String {
    let hundredths = (t.max(0.0) * 100.0).round() as u32;
    format!("{}:{:02}.{:02}", hundredths / 6000, hundredths / 100 % 60, hundredths % 100)
}

/// Draws a race's display over the whole of `screen`, in the style of the flight controller's.
pub fn draw_race(painter: &Painter, screen: Rect, race: &RaceOsd) {
    let painter = &painter.with_clip_rect(screen);
    let size = (screen.height() / 24.0).clamp(12.0, 30.0);
    let font = FontId::monospace(size);
    let big = FontId::monospace(size * 1.6);
    let huge = FontId::monospace(size * 5.0);
    let (left, right) = (screen.left() + screen.width() * 0.06, screen.right() - screen.width() * 0.06);
    let top = screen.top() + screen.height() * 0.13;
    let line = size * 1.15;
    let middle = screen.center().x;
    let text = |at: Pos2, align: Align2, s: &str, font: &FontId, colour: Color32| tinted(painter, at, align, s, font, colour);

    // The race's clock and the lap, under the heading; the gate to go; at the right, the laps.
    text(pos2(middle, top + line * 1.3), Align2::CENTER_TOP, &lap_time(race.time), &big, WHITE);
    text(pos2(middle, top + line * 3.0), Align2::CENTER_TOP, &format!("LAP {}/{}  GATE {}", race.lap, race.laps, race.gate), &font, WHITE);
    let mut y = top + 1.4 * line;
    text(pos2(right, y), Align2::RIGHT_TOP, &format!("LAP {}", lap_time(race.lap_time)), &font, WHITE);
    y += line;
    if let Some(last) = race.last_lap {
        text(pos2(right, y), Align2::RIGHT_TOP, &format!("LAST {}", lap_time(last)), &font, WHITE);
        y += line;
    }
    if let Some(best) = race.best {
        text(pos2(right, y), Align2::RIGHT_TOP, &format!("BEST {}", lap_time(best)), &font, if race.new_best { GREEN } else { WHITE });
    }
    if race.racer {
        text(pos2(left, top + 4.6 * line), Align2::LEFT_TOP, "AUTOPILOT (L)", &font, YELLOW);
    }
    if let Some(name) = &race.watching {
        text(pos2(middle, top), Align2::CENTER_BOTTOM, &format!("WATCHING {name} - N: NEXT"), &font, YELLOW);
    }

    // The position, and everyone in order: the quad watched brighter, each in its colour.
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    if let Some((p, of)) = race.position {
        text(pos2(left, top + 6.0 * line), Align2::LEFT_TOP, &format!("P{p}/{of}"), &big, WHITE);
        let mut y = top + 7.8 * line;
        for (i, s) in race.standings.iter().enumerate() {
            let small = FontId::monospace(size * 0.75);
            let colour = rgb(s.colour);
            let mark = if s.watched { ">" } else { " " };
            text(pos2(left, y), Align2::LEFT_TOP, &format!("{mark}{} {:<6} {}", i + 1, s.name, s.detail), &small, colour);
            y += line * 0.8;
        }
    }
    for l in &race.labels {
        let at = screen.center() + vec2(l.marker.x * screen.width() / 2.0, -l.marker.y * screen.height() / 2.0);
        let small = FontId::monospace(size * 0.7);
        text(at - vec2(0.0, size * 0.6), Align2::CENTER_BOTTOM, &format!("{} {:.0}M", l.name, l.marker.distance), &small, rgb(l.colour));
    }

    // The last gate's split: green when quicker than the last lap there.
    if let Some((split, delta)) = race.split {
        let (s, colour) = match delta {
            Some(d) => (format!("{} {:+.2}", lap_time(split), d), if d <= 0.0 { GREEN } else { RED }),
            None => (lap_time(split), WHITE),
        };
        text(pos2(middle, top + line * 4.3), Align2::CENTER_TOP, &s, &font, colour);
    }

    // Where the next gate is: a diamond on it, or an arrow on the edge the way to turn.
    if let Some(m) = race.marker {
        let at = screen.center() + vec2(m.x * screen.width() / 2.0, -m.y * screen.height() / 2.0);
        let colour = Color32::from_rgba_unmultiplied(120, 255, 160, 210);
        if m.on_screen {
            let r = size * 0.45;
            let diamond = vec![at + vec2(0.0, -r), at + vec2(r, 0.0), at + vec2(0.0, r), at + vec2(-r, 0.0)];
            painter.add(Shape::closed_line(diamond.clone(), Stroke::new(size * 0.18, EDGE)));
            painter.add(Shape::closed_line(diamond, Stroke::new(size * 0.08, colour)));
            text(at + vec2(0.0, r * 1.4), Align2::CENTER_TOP, &format!("{:.0}M", m.distance), &FontId::monospace(size * 0.7), colour);
        } else {
            let way = vec2(m.x * screen.width(), -m.y * screen.height()).normalized();
            let side = vec2(-way.y, way.x);
            let arrow = vec![at, at - way * size * 1.1 + side * size * 0.6, at - way * size * 0.7, at - way * size * 1.1 - side * size * 0.6];
            painter.add(Shape::convex_polygon(arrow, colour, Stroke::new(size * 0.08, EDGE)));
        }
    }

    // In the middle: the countdown, a gate missed, the crash, the results.
    let centre = screen.center();
    let blink = (race.clock * 2.5).fract() < 0.6;
    if let Some(n) = race.countdown {
        let (s, colour) = if n == 0 { ("GO!".to_string(), GREEN) } else { (n.to_string(), YELLOW) };
        text(centre - vec2(0.0, screen.height() * 0.12), Align2::CENTER_CENTER, &s, &huge, colour);
        if n > 0 {
            text(centre + vec2(0.0, screen.height() * 0.04), Align2::CENTER_CENTER, &race.name.to_uppercase(), &font, WHITE);
        }
    }
    if let (Some(gate), true) = (race.missed, blink) {
        let which = if gate == 0 { "START".to_string() } else { gate.to_string() };
        text(centre - vec2(0.0, screen.height() * 0.1), Align2::CENTER_CENTER, &format!("MISSED GATE {which}"), &big, RED);
    }
    if race.crashed {
        text(centre - vec2(0.0, screen.height() * 0.1), Align2::CENTER_CENTER, "CRASHED - 1: RACE AGAIN", &big, RED);
    }
    if let Some((laps, total)) = &race.results {
        let width = size * 16.0;
        let height = line * (laps.len() as f32 + 5.5);
        let panel = Rect::from_center_size(centre, vec2(width, height));
        painter.rect_filled(panel, 6.0, Color32::from_black_alpha(170));
        let mut y = panel.top() + line * 0.6;
        text(pos2(centre.x, y), Align2::CENTER_TOP, "FINISHED", &big, GREEN);
        y += line * 1.8;
        let best = laps.iter().copied().fold(f32::INFINITY, f32::min);
        for (i, t) in laps.iter().enumerate() {
            let colour = if *t == best { GREEN } else { WHITE };
            text(pos2(panel.left() + size, y), Align2::LEFT_TOP, &format!("LAP {}", i + 1), &font, colour);
            text(pos2(panel.right() - size, y), Align2::RIGHT_TOP, &lap_time(*t), &font, colour);
            y += line;
        }
        text(pos2(panel.left() + size, y), Align2::LEFT_TOP, "TOTAL", &font, WHITE);
        text(pos2(panel.right() - size, y), Align2::RIGHT_TOP, &lap_time(*total), &font, WHITE);
        y += line * 1.2;
        let note = if race.new_best { "NEW BEST LAP!  1: RACE AGAIN" } else { "1: RACE AGAIN" };
        text(pos2(centre.x, y), Align2::CENTER_TOP, note, &font, if race.new_best { GREEN } else { WHITE });
    }
}

/// Text of a colour edged in black, as the displays write it.
fn tinted(painter: &Painter, at: Pos2, align: Align2, text: &str, font: &FontId, colour: Color32) {
    let edge = (font.size * 0.08).max(1.0);
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7)] {
        painter.text(at + vec2(dx, dy) * edge, align, text, font.clone(), EDGE);
    }
    painter.text(at, align, text, font.clone(), colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_minutes_seconds_and_hundredths() {
        assert_eq!(lap_time(83.456), "1:23.46");
        assert_eq!(lap_time(9.0), "0:09.00");
        assert_eq!(lap_time(59.999), "1:00.00");
    }
}
