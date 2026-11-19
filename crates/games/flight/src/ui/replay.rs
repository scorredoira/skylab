//! The replay's controls (a bar at the bottom: play, stop, jumps, speed, the scrub bar, leave) and
//! its debrief (top right: the last landing and a chart of altitude and airspeed over what is kept).

use crate::replay::{self, Landing, Playback, Recorder, SPEEDS};
use engine::egui::{self, pos2, vec2, Color32, FontId, RichText, Sense, Stroke};

/// Colours of the chart's lines: altitude and airspeed.
const ALTITUDE: Color32 = Color32::from_rgb(110, 190, 255);
const AIRSPEED: Color32 = Color32::from_rgb(255, 190, 90);
const TOUCHDOWN: Color32 = Color32::from_rgb(120, 230, 120);
const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(12, 14, 18, 215);

/// Draws the bar, `lift` points above the bottom (clear of the row of instruments outside), and the
/// debrief; returns whether the player asked to leave the replay.
pub fn show(ctx: &egui::Context, p: &mut Playback, recorder: &Recorder, lift: f32) -> bool {
    let Some(span) = recorder.span() else { return true };
    let mut leave = false;
    let frame = egui::Frame::new().fill(BACKGROUND).corner_radius(8.0).inner_margin(egui::Margin::symmetric(14, 10));
    egui::Area::new(egui::Id::unique("replay-bar")).anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -12.0 - lift)).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("REPLAY").strong().color(Color32::from_rgb(255, 110, 90)));
                ui.add_space(6.0);
                if ui.button("−30 s").on_hover_text("⇧←").clicked() {
                    p.jump(-30.0, span);
                }
                if ui.button("−10 s").on_hover_text("←").clicked() {
                    p.jump(-10.0, span);
                }
                let play = if p.playing { "Pause" } else { "Play" };
                if ui.add(egui::Button::new(RichText::new(play).strong()).min_size(vec2(64.0, 0.0))).on_hover_text("Space").clicked() {
                    if !p.playing && p.time >= span.1 {
                        p.time = span.0;
                    }
                    p.playing = !p.playing;
                }
                if ui.button("+10 s").on_hover_text("→").clicked() {
                    p.jump(10.0, span);
                }
                ui.add_space(6.0);
                for speed in SPEEDS {
                    let name = if speed < 1.0 { format!("{speed}×") } else { format!("{speed:.0}×") };
                    if ui.selectable_label((p.speed - speed).abs() < 1e-9, name).on_hover_text("↑ ↓").clicked() {
                        p.speed = speed;
                    }
                }
                ui.add_space(6.0);
                let mut at = p.time - span.0;
                let length = (span.1 - span.0).max(0.1);
                ui.spacing_mut().slider_width = (ctx.content_rect().width() * 0.3).clamp(160.0, 520.0);
                let slider = ui.add(egui::Slider::new(&mut at, 0.0..=length).show_value(false));
                if slider.changed() {
                    p.time = span.0 + at;
                }
                ui.label(RichText::new(format!("−{} / {}", replay::clock(span.1 - p.time), replay::clock(length))).monospace());
                ui.add_space(6.0);
                if ui.button("Back to the flight (F2)").clicked() {
                    leave = true;
                }
            });
        });
    });
    egui::Area::new(egui::Id::unique("replay-debrief")).anchor(egui::Align2::RIGHT_TOP, vec2(-16.0, 64.0)).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(300.0);
            debrief(ui, recorder.last_landing());
            ui.add_space(6.0);
            chart(ui, p, recorder, span);
        });
    });
    leave
}

/// The last landing, in numbers.
fn debrief(ui: &mut egui::Ui, landing: Option<&Landing>) {
    let Some(l) = landing else {
        ui.label(RichText::new("No landings in these minutes").weak());
        return;
    };
    ui.label(RichText::new("Last landing").strong());
    let row = |ui: &mut egui::Ui, name: &str, value: String| {
        ui.label(RichText::new(name).color(Color32::from_gray(175)));
        ui.label(RichText::new(value).monospace());
        ui.end_row();
    };
    egui::Grid::new("shot").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
        row(ui, "Vertical speed", format!("{:.0} ft/min", -l.sink_fpm));
        row(ui, "Load", replay::g_text(l.g));
        row(ui, "Velocidad", format!("{:.0} kt; ground, {:.0} kt", l.airspeed_kt, l.ground_speed_kt));
        row(ui, "Bank, pitch", format!("{:+.1}°, {:+.1}°", l.bank_deg, l.pitch_deg));
        match l.runway {
            Some((number, from, right)) => {
                row(ui, "Runway", format!("{number:02}"));
                row(ui, "From threshold", format!("{from:.0} m"));
                let side = if right.abs() < 0.5 { "on the centreline".to_string() } else { format!("{:.1} m {}", right.abs(), if right > 0.0 { "right" } else { "left" }) };
                row(ui, "From centreline", side);
            }
            None => row(ui, "Runway", "off the runway".to_string()),
        }
    });
}

/// Altitude and airspeed over what is kept, the moment shown and the last landing marked; a click
/// or a drag on it goes there.
fn chart(ui: &mut egui::Ui, p: &mut Playback, recorder: &Recorder, span: (f64, f64)) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 90.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_black_alpha(90));
    let profile = recorder.profile(300);
    let length = (span.1 - span.0).max(0.1);
    let x = |t: f64| rect.left() + ((t - span.0) / length).clamp(0.0, 1.0) as f32 * rect.width();
    let inner = rect.shrink2(vec2(0.0, 6.0));
    let line = |values: Vec<(f64, f64)>, colour: Color32| {
        let (low, high) = values.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, v)| (lo.min(*v), hi.max(*v)));
        let range = (high - low).max(1.0);
        let points: Vec<egui::Pos2> = values.iter().map(|(t, v)| pos2(x(*t), inner.bottom() - ((v - low) / range) as f32 * inner.height())).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, colour)));
        (low, high)
    };
    let (_, top) = line(profile.iter().map(|(t, a, _)| (*t, *a)).collect(), ALTITUDE);
    let (_, fastest) = line(profile.iter().map(|(t, _, s)| (*t, *s)).collect(), AIRSPEED);
    ui.horizontal(|ui| {
        let font = FontId::proportional(11.0);
        ui.label(RichText::new(format!("altitude, up to {top:.0} ft")).font(font.clone()).color(ALTITUDE));
        ui.label(RichText::new(format!("speed, up to {fastest:.0} kt")).font(font.clone()).color(AIRSPEED));
        ui.label(RichText::new("touchdown").font(font).color(TOUCHDOWN));
    });
    if let Some(l) = recorder.last_landing() {
        let at = x(l.time);
        painter.line_segment([pos2(at, rect.top()), pos2(at, rect.bottom())], Stroke::new(1.0, TOUCHDOWN));
    }
    let now = x(p.time);
    painter.line_segment([pos2(now, rect.top()), pos2(now, rect.bottom())], Stroke::new(2.0, Color32::WHITE));
    if let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked() || response.dragged()) {
        p.time = span.0 + ((at.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * length;
    }
}
