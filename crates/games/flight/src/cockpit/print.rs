//! What is printed in a cockpit (the legends of its switches, knobs and keys, the placards, the
//! seams and screws of the panel's plates), as data: shapes in points of the print's canvas
//! (`CabinSpec::print_points`), painted with egui once when the cockpit is put on the GPU.
//! `cabin.wgsl` lays the canvas's panel part over the faces that face aft and its floor part over
//! those that face up (`CabinSpec::panel_print`, `floor_print`). Colours are egui's: RGBA, 8 bits,
//! premultiplied by their alpha.

use engine::egui::{self, epaint, pos2, Color32, CornerRadius, FontId, Painter, Pos2, Stroke};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// How a rectangle's outline lies on its edge (`egui::StrokeKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    Inside,
    Middle,
    Outside,
}

/// A shape of the print.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// One line of text, its top left corner at `at`, its font `size` points (monospace if `mono`).
    Text {
        at: Vec2,
        text: String,
        size: f32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        mono: bool,
        color: [u8; 4],
    },
    /// A rectangle: its corners' radii (top left, top right, bottom left, bottom right), what fills
    /// it and its outline (width and colour, and where it lies).
    Rect {
        min: Vec2,
        max: Vec2,
        #[serde(default)]
        radius: [u8; 4],
        #[serde(default)]
        fill: [u8; 4],
        #[serde(default)]
        stroke: (f32, [u8; 4]),
        #[serde(default = "Shape::middle")]
        edge: Edge,
    },
    Circle {
        centre: Vec2,
        radius: f32,
        #[serde(default)]
        fill: [u8; 4],
        #[serde(default)]
        stroke: (f32, [u8; 4]),
    },
    Line {
        from: Vec2,
        to: Vec2,
        width: f32,
        color: [u8; 4],
    },
}

impl Shape {
    fn middle() -> Edge {
        Edge::Middle
    }
}

fn color([r, g, b, a]: [u8; 4]) -> Color32 {
    Color32::from_rgba_premultiplied(r, g, b, a)
}

fn at(p: Vec2) -> Pos2 {
    pos2(p.x, p.y)
}

/// Paints `shapes` with `painter`, in order.
pub fn paint(painter: &Painter, shapes: &[Shape]) {
    for shape in shapes {
        match shape {
            Shape::Text { at: p, text, size, mono, color: c } => {
                let font = if *mono { FontId::monospace(*size) } else { FontId::proportional(*size) };
                let galley = painter.layout_no_wrap(text.clone(), font, color(*c));
                painter.galley(at(*p), galley, color(*c));
            }
            Shape::Rect { min, max, radius: [nw, ne, sw, se], fill, stroke, edge } => {
                let kind = match edge {
                    Edge::Inside => egui::StrokeKind::Inside,
                    Edge::Middle => egui::StrokeKind::Middle,
                    Edge::Outside => egui::StrokeKind::Outside,
                };
                let rect = egui::Rect::from_min_max(at(*min), at(*max));
                let radius = CornerRadius { nw: *nw, ne: *ne, sw: *sw, se: *se };
                painter.add(epaint::RectShape::new(rect, radius, color(*fill), Stroke::new(stroke.0, color(stroke.1)), kind));
            }
            Shape::Circle { centre, radius, fill, stroke } => {
                painter.add(epaint::CircleShape { center: at(*centre), radius: *radius, fill: color(*fill), stroke: Stroke::new(stroke.0, color(stroke.1)) });
            }
            Shape::Line { from, to, width, color: c } => {
                painter.line_segment([at(*from), at(*to)], Stroke::new(*width, color(*c)));
            }
        }
    }
}

/// The shapes `paint` paints as print (`Shape`): what egui was given in a canvas of `size` points
/// at `pixels_per_point` (text is laid out for it).
/// Text keeps its first section's font and colour; shapes of other kinds are an error.
#[cfg(test)]
pub fn record(size: (f32, f32), pixels_per_point: f32, paint: impl FnMut(&egui::Context)) -> Result<Vec<Shape>, String> {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(pixels_per_point);
    let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(size.0, size.1))), ..Default::default() };
    let mut paint = paint;
    let mut output = ctx.run_ui(raw, |root| paint(root.ctx()));
    // Nothing is drawn here: the fonts' texture it would upload is not needed.
    output.textures_delta.clear();
    let v = |p: Pos2| Vec2::new(p.x, p.y);
    let rgba = |c: Color32| c.to_array();
    output
        .shapes
        .into_iter()
        .map(|clipped| match clipped.shape {
            epaint::Shape::Text(t) => {
                let section = t.galley.job.sections.first().ok_or("text without a section")?;
                let font = &section.format.font_id;
                if t.angle != 0.0 || t.override_text_color.is_some() || t.galley.job.sections.len() > 1 {
                    return Err(format!("text \"{}\" turned or of several colours", t.galley.job.text));
                }
                Ok(Shape::Text { at: v(t.pos), text: t.galley.job.text.clone(), size: font.size, mono: font.family == egui::FontFamily::Monospace, color: rgba(t.fallback_color) })
            }
            epaint::Shape::Rect(r) => {
                let c = r.corner_radius;
                let edge = match r.stroke_kind {
                    egui::StrokeKind::Inside => Edge::Inside,
                    egui::StrokeKind::Middle => Edge::Middle,
                    egui::StrokeKind::Outside => Edge::Outside,
                };
                Ok(Shape::Rect { min: v(r.rect.min), max: v(r.rect.max), radius: [c.nw, c.ne, c.sw, c.se], fill: rgba(r.fill), stroke: (r.stroke.width, rgba(r.stroke.color)), edge })
            }
            epaint::Shape::Circle(c) => Ok(Shape::Circle { centre: v(c.center), radius: c.radius, fill: rgba(c.fill), stroke: (c.stroke.width, rgba(c.stroke.color)) }),
            epaint::Shape::LineSegment { points: [a, b], stroke } => Ok(Shape::Line { from: v(a), to: v(b), width: stroke.width, color: rgba(stroke.color) }),
            other => Err(format!("a shape the print does not keep: {other:?}")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is painted, recorded and painted again is what was painted the first time.
    #[test]
    fn a_print_painted_again_is_the_same() {
        let first = |ctx: &egui::Context| {
            let p = ctx.layer_painter(egui::LayerId::background());
            p.text(pos2(100.0, 50.0), egui::Align2::CENTER_CENTER, "MASTER", FontId::proportional(9.5), Color32::from_gray(232));
            p.text(pos2(10.0, 20.0), egui::Align2::RIGHT_TOP, "122.800", FontId::monospace(7.0), Color32::from_rgb(90, 230, 120));
            p.rect_filled(egui::Rect::from_min_max(pos2(5.0, 5.0), pos2(40.0, 30.0)), 2.0, Color32::from_gray(205));
            p.rect_stroke(egui::Rect::from_min_max(pos2(5.0, 5.0), pos2(40.0, 30.0)), 3.3, Stroke::new(0.5, Color32::from_rgba_premultiplied(0, 0, 0, 200)), egui::StrokeKind::Middle);
            p.circle_filled(pos2(60.0, 60.0), 4.0, Color32::from_gray(125));
            p.circle_stroke(pos2(60.0, 60.0), 4.0, Stroke::new(0.3, Color32::BLACK));
            p.line_segment([pos2(1.0, 2.0), pos2(3.0, 4.0)], Stroke::new(0.6, Color32::from_gray(40)));
        };
        let shapes = record((200.0, 100.0), 4.0, first).unwrap();
        assert_eq!(shapes.len(), 7);
        let again = record((200.0, 100.0), 4.0, |ctx| paint(&ctx.layer_painter(egui::LayerId::background()), &shapes)).unwrap();
        assert_eq!(again, shapes);
        let text = ron::to_string(&shapes).unwrap();
        assert_eq!(ron::from_str::<Vec<Shape>>(&text).unwrap(), shapes);
    }
}
