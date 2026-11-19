//! What the player sees while something takes a while: a card in the middle of the screen while
//! another place loads (the flight held where it was) or after its loading failed, and a small
//! note at the bottom while something lighter is made in the background (a graphics quality, the
//! satellites' picture of the ground) and the flight goes on.

use engine::egui::{self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Stroke, StrokeKind};
use std::f32::consts::TAU;

/// The colours of the other panels over the picture (`ui::replay`, the place picker).
const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(12, 14, 18, 250);
const ACCENT: Color32 = Color32::from_rgb(110, 180, 255);
const FAILED: Color32 = Color32::from_rgb(255, 130, 100);
const FLY: Color32 = Color32::from_rgb(38, 110, 190);

/// Where the place being gone to stands.
pub enum Place<'a> {
    /// Loading: how far it has got (share 0 to 1, what it is doing), if it has said, and the
    /// seconds since it started.
    Loading { progress: Option<(f64, String)>, seconds: f64 },
    /// Its loading failed, for this reason.
    Failed { reason: &'a str },
}

/// What the player chose on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Not to go: the flight goes on where it was.
    Stay,
    /// Load it again.
    Retry,
    /// Choose another place (the picker).
    Another,
}

/// A spinner: an arc going round, `time` in seconds.
fn spinner(painter: &egui::Painter, centre: egui::Pos2, radius: f32, time: f64, colour: Color32) {
    let start = (time * 1.6).fract() as f32 * TAU;
    let points: Vec<egui::Pos2> = (0..=24).map(|k| start + k as f32 / 24.0 * TAU * 0.7).map(|a| centre + radius * vec2(a.cos(), a.sin())).collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.5, colour)));
}

/// The card over the picture, dimmed and held, while the place `name` (`describe`d below it)
/// loads or after its loading failed. Returns what the player chose, if anything.
pub fn place_card(ctx: &egui::Context, name: &str, describe: &str, place: &Place, time: f64) -> Option<Choice> {
    let screen = ctx.content_rect();
    // The flight stands still behind it: the picture dimmed.
    ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("place-loading-shade"))).rect_filled(screen, 0.0, Color32::from_black_alpha(120));
    let mut choice = None;
    let frame = egui::Frame::new().fill(BACKGROUND).corner_radius(12.0).inner_margin(egui::Margin::symmetric(26, 22)).stroke(Stroke::new(1.0, Color32::from_white_alpha(28)));
    egui::Area::new(egui::Id::unique("place-loading")).anchor(Align2::CENTER_CENTER, vec2(0.0, -20.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(420.0);
            match place {
                Place::Loading { progress, seconds } => {
                    ui.label(RichText::new("GOING TO").size(11.0).color(ACCENT).strong());
                    ui.label(RichText::new(name).size(24.0).strong().color(Color32::WHITE));
                    ui.label(RichText::new(describe).size(13.0).weak());
                    ui.add_space(16.0);
                    let (share, doing) = match progress {
                        Some((share, doing)) => (Some(*share), format!("Loading {doing}")),
                        None => (None, "Starting…".to_string()),
                    };
                    // The bar: filled as far as it has got, or a band going along it before it says.
                    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), egui::Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(bar, 4.0, Color32::from_white_alpha(22));
                    let filled = match share {
                        Some(share) => Rect::from_min_size(bar.min, vec2(bar.width() * share.clamp(0.02, 1.0) as f32, bar.height())),
                        None => {
                            let at = (time * 0.7).fract() as f32;
                            let x = bar.left() + (bar.width() * 1.25) * at - bar.width() * 0.25;
                            Rect::from_x_y_ranges(x.max(bar.left())..=(x + bar.width() * 0.25).min(bar.right()), bar.y_range())
                        }
                    };
                    painter.rect_filled(filled, 4.0, ACCENT);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (spot, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                        spinner(ui.painter(), spot.center(), 6.0, time, ACCENT);
                        ui.label(RichText::new(doing).size(13.0).color(Color32::from_gray(220)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(share) = share {
                                ui.label(RichText::new(format!("{:.0} %", share * 100.0)).monospace().color(Color32::from_gray(200)));
                            }
                        });
                    });
                    ui.add_space(10.0);
                    let note = if *seconds > 8.0 { "The first visit downloads the place (20 to 50 MB); later ones read it from the disk." } else { "Your flight here is paused until the new place is ready." };
                    ui.label(RichText::new(note).size(12.0).weak());
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel: stay here (Esc)").clicked() {
                            choice = Some(Choice::Stay);
                        }
                    });
                }
                Place::Failed { reason } => {
                    ui.label(RichText::new("COULD NOT LOAD").size(11.0).color(FAILED).strong());
                    ui.label(RichText::new(name).size(24.0).strong().color(Color32::WHITE));
                    ui.add_space(10.0);
                    egui::Frame::new().fill(Color32::from_rgba_unmultiplied(255, 130, 100, 18)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(*reason).size(13.0).color(Color32::from_gray(230)));
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Your flight is still where it was, paused.").size(12.0).weak());
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new("Back to the flight (Esc)").strong()).fill(FLY)).clicked() {
                            choice = Some(Choice::Stay);
                        }
                        if ui.button("Try again").clicked() {
                            choice = Some(Choice::Retry);
                        }
                        if ui.button("Choose another place…").clicked() {
                            choice = Some(Choice::Another);
                        }
                    });
                }
            }
        });
    });
    choice
}

/// A small note at the bottom of the screen with a spinner, `lift` points above the bottom: what is
/// being made in the background while the flight goes on.
pub fn busy(painter: &egui::Painter, screen: Rect, text: &str, lift: f32, time: f64) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(13.0), Color32::from_gray(230));
    let size = vec2(galley.size().x + 44.0, 28.0);
    let frame = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - lift - size.y / 2.0), size);
    painter.rect(frame, 14.0, BACKGROUND, Stroke::new(1.0, Color32::from_white_alpha(28)), StrokeKind::Inside);
    spinner(painter, pos2(frame.left() + 18.0, frame.center().y), 6.0, time, ACCENT);
    painter.galley(pos2(frame.left() + 32.0, frame.center().y - galley.size().y / 2.0), galley, Color32::from_gray(230));
}
