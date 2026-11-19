//! Builds the race gates' models, `assets/courses/gates.glb`, from the kinds of gate of
//! `assets/courses/gates.ron` (`sim_core::course::shapes`): every tube of a frame where the
//! simulation has its bar, so what is drawn is what is hit.
//! `cargo build --release -p flight --example gate_models && target/release/examples/gate_models assets/courses/gates.glb`
//!
//! Gate axes: x the way through, y right, z up, metres from the gate's foot. The nodes, which
//! `src/course.rs` draws (`course::GateModels`):
//! - `<kind>` (`arch`, `square`, `round`, `ladder`, `flag`, `dive`, `log`, `high`, `hanging`): the
//!   frame, lit as anything is: light tubes (the game paints them the gate's colour) with aluminium
//!   joints and feet, the number's dark plate, the arch's checkered banner and blue padding, the
//!   flag's fabric, the log's bark (its own colour, not painted) and the cut rings of its ends;
//! - `<kind>_led`: the LED strips along the inside of the opening, white: the game gives them their
//!   colour and makes them shine;
//! - `digit_0`...`digit_9`: seven-segment digits a metre tall, centred, facing −x (the racers
//!   coming), shining as the strips;
//! - `pad`: the start's pad, 1.6 m square, checkered, its top at z = 0.02.
//!
//! Everything is original, made here (MIT).

use engine::gltf::{Vertex, Writer, WrittenMaterial};
use glam::{Vec2, Vec3, Vec4};
use sim_core::course::{self, GateKind, Opening, Part};
use std::f32::consts::TAU;

/// Triangles of one material, built up.
#[derive(Default)]
struct Shape {
    vertices: Vec<Vertex>,
}

impl Shape {
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec4) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        for p in [a, b, c] {
            self.vertices.push(Vertex { position: p, normal: n, color, uv: Vec2::ZERO });
        }
    }

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec4) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    /// A box between two corners, its faces outwards.
    fn cuboid(&mut self, lo: Vec3, hi: Vec3, color: Vec4) {
        let (a, b) = (lo, hi);
        let c = [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(b.x, b.y, b.z),
            Vec3::new(a.x, b.y, b.z),
        ];
        for [i, j, k, l] in [[0, 1, 2, 3], [4, 7, 6, 5], [0, 4, 5, 1], [1, 5, 6, 2], [2, 6, 7, 3], [3, 7, 4, 0]] {
            self.quad(c[i], c[j], c[k], c[l], color);
        }
    }

    /// A tube of `radius` from `a` to `b`, its ends closed; smooth sides.
    fn cylinder(&mut self, a: Vec3, b: Vec3, radius: f32, segments: usize, color: Vec4) {
        let axis = (b - a).normalize();
        let u = axis.any_orthonormal_vector();
        let v = axis.cross(u);
        for i in 0..segments {
            let (t0, t1) = (TAU * i as f32 / segments as f32, TAU * (i + 1) as f32 / segments as f32);
            let d0 = u * t0.cos() + v * t0.sin();
            let d1 = u * t1.cos() + v * t1.sin();
            for (p, n) in [(a + d0 * radius, d0), (a + d1 * radius, d1), (b + d1 * radius, d1), (a + d0 * radius, d0), (b + d1 * radius, d1), (b + d0 * radius, d0)] {
                self.vertices.push(Vertex { position: p, normal: n, color, uv: Vec2::ZERO });
            }
            self.tri(a, a + d0 * radius, a + d1 * radius, color);
            self.tri(b, b + d1 * radius, b + d0 * radius, color);
        }
    }

    /// A ball (the joints of the frame's tubes).
    fn ball(&mut self, at: Vec3, radius: f32, color: Vec4) {
        let (rings, segments) = (6, 10);
        let point = |i: usize, j: usize| {
            let (t, p) = (std::f32::consts::PI * i as f32 / rings as f32, TAU * j as f32 / segments as f32);
            Vec3::new(t.sin() * p.cos(), t.sin() * p.sin(), t.cos())
        };
        for i in 0..rings {
            for j in 0..segments {
                let (a, b, c, d) = (point(i, j), point(i + 1, j), point(i + 1, j + 1), point(i, j + 1));
                for n in [a, b, c, a, c, d] {
                    self.vertices.push(Vertex { position: at + n * radius, normal: n, color, uv: Vec2::ZERO });
                }
            }
        }
    }

    /// A flat polygon (convex, in order), seen from both sides.
    fn both_sides(&mut self, ring: &[Vec3], color: Vec4) {
        for i in 1..ring.len() - 1 {
            self.tri(ring[0], ring[i], ring[i + 1], color);
            self.tri(ring[0], ring[i + 1], ring[i], color);
        }
    }
}

fn srgb(r: u8, g: u8, b: u8) -> Vec4 {
    let f = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    Vec4::new(f(r), f(g), f(b), 1.0)
}

fn v(p: sim_core::V3) -> Vec3 {
    Vec3::new(p.x as f32, p.y as f32, p.z as f32)
}

/// The seven segments of a digit, as which of a, b, c, d, e, f, g are lit (top, top right,
/// bottom right, bottom, bottom left, top left, middle).
const SEGMENTS: [[bool; 7]; 10] = [
    [true, true, true, true, true, true, false],
    [false, true, true, false, false, false, false],
    [true, true, false, true, true, false, true],
    [true, true, true, true, false, false, true],
    [false, true, true, false, false, true, true],
    [true, false, true, true, false, true, true],
    [true, false, true, true, true, true, true],
    [true, true, true, false, false, false, false],
    [true, true, true, true, true, true, true],
    [true, true, true, true, false, true, true],
];

/// A digit a metre tall and 0.55 wide, centred, in the plane x = 0 facing −x.
fn digit(d: usize, color: Vec4) -> Shape {
    let mut s = Shape::default();
    let (w, h, t) = (0.55f32, 1.0f32, 0.14f32);
    // Each segment from (y0, z0) to (y1, z1) as a bar `t` thick.
    let (l, r, top, mid, bottom) = (-w / 2.0, w / 2.0, h / 2.0, 0.0, -h / 2.0);
    let bars = [((l, top), (r, top)), ((r, top), (r, mid)), ((r, mid), (r, bottom)), ((l, bottom), (r, bottom)), ((l, mid), (l, bottom)), ((l, top), (l, mid)), ((l, mid), (r, mid))];
    for (k, ((y0, z0), (y1, z1))) in bars.iter().enumerate() {
        if !SEGMENTS[d][k] {
            continue;
        }
        let along = Vec2::new(y1 - y0, z1 - z0).normalize();
        let side = Vec2::new(-along.y, along.x) * (t / 2.0);
        // A small gap at each end, as the segments of a display have.
        let (a, b) = (Vec2::new(*y0, *z0) + along * (t * 0.6), Vec2::new(*y1, *z1) - along * (t * 0.6));
        let p = |q: Vec2| Vec3::new(0.0, q.x, q.y);
        // Facing −x: seen from −x, y to the right and z up, counter-clockwise.
        s.quad(p(a - side), p(a + side), p(b + side), p(b - side), color);
    }
    s
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "assets/courses/gates.glb".to_string());
    let mut w = Writer::new();
    let mut material = |name: &str, metallic: f32, roughness: f32| w.material(WrittenMaterial { name: name.into(), base: Vec4::ONE, metallic, roughness, emissive: Vec3::ZERO });
    let tube_m = material("powder coat", 0.0, 0.35);
    let joint_m = material("aluminium", 1.0, 0.35);
    let fabric_m = material("fabric", 0.0, 0.85);
    let led_m = material("led", 0.0, 0.2);

    let tube_color = srgb(200, 202, 206);
    let joint_color = srgb(150, 152, 158);
    let white = Vec4::ONE;
    let plate_color = srgb(14, 14, 16);
    let padding = srgb(18, 60, 170);
    let bark = srgb(78, 62, 48);
    let cut_wood = srgb(196, 160, 112);

    for shape in course::shapes() {
        let kind = shape.kind;
        let tube = shape.tube as f32;
        let mut frame = Shape::default();
        let mut joints = Shape::default();
        let mut fabric = Shape::default();
        let mut leds = Shape::default();
        let color = match kind {
            GateKind::Arch => padding,
            GateKind::Log => bark,
            _ => tube_color,
        };
        // The tubes, with a joint where tubes meet, and the feet's pads.
        let bars = shape.bars();
        let segments = if kind == GateKind::Arch || kind == GateKind::Log { 20 } else { 12 };
        for (a, b) in &bars {
            frame.cylinder(v(*a), v(*b), tube, segments, color);
            if kind == GateKind::Log {
                // A log and its stumps: rough bark in rings, a pale cut face at each end.
                let (a, b) = (v(*a), v(*b));
                let along = (b - a).normalize();
                let rings = ((b - a).length() / 0.45) as usize;
                for k in 1..rings {
                    let at = a + (b - a) * (k as f32 / rings as f32);
                    let r = tube * (1.04 + 0.03 * ((k * 7) % 3) as f32);
                    frame.cylinder(at - along * 0.06, at + along * 0.06, r, segments, srgb(64, 50, 38));
                }
                for (end, out) in [(a, -along), (b, along)] {
                    joints.cylinder(end + out * 0.002, end + out * 0.012, tube * 0.97, segments, cut_wood);
                }
                continue;
            }
            for end in [a, b] {
                if kind != GateKind::Arch && kind != GateKind::Round && kind != GateKind::High && kind != GateKind::Hanging {
                    joints.ball(v(*end), tube * 1.25, joint_color);
                } else {
                    frame.ball(v(*end), tube, color);
                }
            }
        }
        // The LED strips: along each tube round the opening, on its inner side (towards the
        // opening's middle), standing out of it a little; on the flag, three along its pole.
        let opening_middle = match shape.opening {
            Opening::Rect { middle: (y, z), .. } | Opening::Circle { middle: (y, z), .. } => Vec3::new(0.0, y as f32, z as f32),
            Opening::Flat { z, .. } => Vec3::new(0.0, 0.0, z as f32),
            Opening::Beside { .. } => Vec3::ZERO,
        };
        for part in &shape.parts {
            let (middle, lit) = match *part {
                Part::Rect { middle: (y, z), .. } | Part::Ring { middle: (y, z), .. } => (Vec3::new(0.0, y as f32, z as f32), true),
                Part::FlatRect { z, .. } => (Vec3::new(0.0, 0.0, z as f32), true),
                // The arch's legs and top are its opening's edge; the flag's pole carries strips.
                Part::Bar { .. } => (opening_middle, kind == GateKind::Arch || kind == GateKind::Flag || kind == GateKind::Log),
            };
            if !lit {
                continue;
            }
            let one = course::GateShape { parts: vec![*part], ..shape.clone() };
            for (a, b) in one.bars() {
                let (a, b) = (v(a), v(b));
                if kind == GateKind::Flag {
                    for k in 0..3 {
                        let t = TAU * k as f32 / 3.0;
                        let off = Vec3::new(t.cos(), t.sin(), 0.0) * tube * 0.9;
                        leds.cylinder(a + off + Vec3::Z * 0.3, b + off - Vec3::Z * 0.1, tube * 0.32, 6, white);
                    }
                    continue;
                }
                let mid = (a + b) / 2.0;
                let along = (b - a).normalize();
                let to_middle = middle - mid;
                let inward = (to_middle - along * to_middle.dot(along)).normalize_or_zero();
                let off = inward * tube;
                leds.cylinder(a + off, b + off, tube * 0.45, 6, white);
            }
        }
        // The number's plate (the arch has its banner instead).
        let n = v(course::shape(kind).number.into_v3());
        if kind == GateKind::Arch {
            // A checkered banner over the top, both faces, 6.4 m by 0.9.
            let (cols, rows) = (16, 2);
            let (width, height) = (6.4f32, 0.9f32);
            for i in 0..cols {
                for j in 0..rows {
                    let c = if (i + j) % 2 == 0 { srgb(240, 240, 240) } else { srgb(12, 12, 12) };
                    let y0 = -width / 2.0 + width * i as f32 / cols as f32;
                    let y1 = y0 + width / cols as f32;
                    let z0 = n.z - height / 2.0 + height * j as f32 / rows as f32;
                    let z1 = z0 + height / rows as f32;
                    for x in [-0.03f32, 0.03] {
                        let p = |y: f32, z: f32| Vec3::new(x, y, z);
                        if x < 0.0 {
                            fabric.quad(p(y0, z0), p(y0, z1), p(y1, z1), p(y1, z0), c);
                        } else {
                            fabric.quad(p(y0, z0), p(y1, z0), p(y1, z1), p(y0, z1), c);
                        }
                    }
                }
            }
            // Its posts down to the top tube.
            for y in [-2.9f32, 2.9] {
                joints.cylinder(Vec3::new(0.0, y, 5.6), Vec3::new(0.0, y, n.z - 0.45), 0.03, 8, joint_color);
            }
        } else {
            frame.cuboid(n + Vec3::new(-0.02, -0.42, -0.3), n + Vec3::new(0.02, 0.42, 0.3), plate_color);
            // On its post.
            let below = match kind {
                GateKind::Flag => n.z - 0.3,
                _ => n.z - 0.3,
            };
            let top_of_frame = bars.iter().map(|(a, b)| a.z.max(b.z) as f32).fold(0.0f32, f32::max);
            if kind != GateKind::Dive && kind != GateKind::Flag {
                joints.cylinder(Vec3::new(0.0, 0.0, top_of_frame), Vec3::new(0.0, 0.0, below), 0.025, 8, joint_color);
            }
            if kind == GateKind::Dive {
                joints.cylinder(Vec3::new(n.x, 0.0, 7.0), Vec3::new(n.x, 0.0, below), 0.025, 8, joint_color);
            }
        }
        if kind == GateKind::Flag {
            // A feather flag: orange with a white edge, along the pole and trailing behind it (+x).
            let (top, bottom, width) = (4.2f32, 1.3f32, 0.75f32);
            let ring: Vec<Vec3> = (0..=10)
                .map(|i| {
                    let t = i as f32 / 10.0;
                    let z = bottom + (top - bottom) * t;
                    // The feather's curve: wider at the top, rounded over it.
                    let out = width * (0.55 + 0.45 * (t * std::f32::consts::PI * 0.9).sin());
                    Vec3::new(out + 0.04, 0.0, z)
                })
                .collect();
            let mut outline = vec![Vec3::new(0.04, 0.0, bottom)];
            outline.extend(ring.iter().copied());
            outline.push(Vec3::new(0.04, 0.0, top + 0.1));
            let edge = srgb(240, 240, 235);
            let inner: Vec<Vec3> = outline.iter().map(|p| Vec3::new(0.04 + (p.x - 0.04) * 0.86, 0.0, bottom + 0.1 + (p.z - bottom) * 0.92)).collect();
            fabric.both_sides(&outline.iter().map(|p| *p + Vec3::Y * 0.004).collect::<Vec<_>>(), edge);
            fabric.both_sides(&inner, srgb(255, 92, 8));
            // Its base, a ground stake's plate.
            joints.cuboid(Vec3::new(-0.25, -0.25, 0.0), Vec3::new(0.25, 0.25, 0.04), joint_color);
        }
        let word = kind.word();
        w.node(word, None, Vec3::ZERO, &[(tube_m, frame.vertices, false), (joint_m, joints.vertices, false), (fabric_m, fabric.vertices, false)]);
        w.node(&format!("{word}_led"), None, Vec3::ZERO, &[(led_m, leds.vertices, false)]);
    }
    for d in 0..10 {
        w.node(&format!("digit_{d}"), None, Vec3::ZERO, &[(led_m, digit(d, white).vertices, false)]);
    }
    // The start's pad: black and white squares in an orange border.
    let mut pad = Shape::default();
    let half = 0.8f32;
    pad.cuboid(Vec3::new(-half, -half, 0.0), Vec3::new(half, half, 0.015), srgb(255, 92, 8));
    let cells = 6;
    let inner = half * 0.85;
    for i in 0..cells {
        for j in 0..cells {
            let c = if (i + j) % 2 == 0 { srgb(235, 235, 235) } else { srgb(15, 15, 15) };
            let (x0, y0) = (-inner + 2.0 * inner * i as f32 / cells as f32, -inner + 2.0 * inner * j as f32 / cells as f32);
            let (x1, y1) = (x0 + 2.0 * inner / cells as f32, y0 + 2.0 * inner / cells as f32);
            pad.quad(Vec3::new(x0, y0, 0.02), Vec3::new(x1, y0, 0.02), Vec3::new(x1, y1, 0.02), Vec3::new(x0, y1, 0.02), c);
        }
    }
    w.node("pad", None, Vec3::ZERO, &[(fabric_m, pad.vertices, false)]);
    std::fs::write(&out, w.glb()).unwrap_or_else(|e| panic!("{out}: {e}"));
    println!("{out}");
}

trait IntoV3 {
    fn into_v3(self) -> sim_core::V3;
}

impl IntoV3 for (f64, f64, f64) {
    fn into_v3(self) -> sim_core::V3 {
        sim_core::V3::new(self.0, self.1, self.2)
    }
}
