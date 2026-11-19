//! Builds the quads' models from shapes:
//!
//! - `fpv5`, the 5-inch freestyle quad, `assets/aircraft/fpv5/exterior.glb`: a true-X carbon frame
//!   (arms, plates, aluminium standoffs), the flight controller's stack, four 2306 motors with
//!   their bells, two-blade 5x3 propellers and the blur of each, the battery strapped on top, the
//!   camera between orange side plates tilted up 25°, the video antenna at the back.
//! - `race5`, the 5-inch racing quad, `assets/aircraft/race5/exterior.glb`: a slimmer true-X frame
//!   of 5 mm arms and a low top plate, four 2207 motors, three-blade 5.1x4.3 race propellers, a
//!   6S 1300 mAh pack (78 x 39 x 36 mm) strapped on top, the camera tilted up 40° between green
//!   side plates, a short whip antenna at the back.
//!
//! `cargo build --release -p flight --example quad_model && target/release/examples/quad_model [fpv5|race5] [out.glb]`
//! (fpv5 and its own file without arguments; each to its own file without the second).
//!
//! Body axes: x forward, y right, z down, metres from the centre of gravity, where its
//! `multirotor.ron` puts its motors (fpv5's hubs at (±0.0795, ±0.0795, −0.022), race5's at
//! (±0.0778, ±0.0778, −0.022)) and its camera. The nodes are those `exterior.ron` names: `frame`,
//! `stack`, `battery`, `camera`, `motors`, `prop_1`...`prop_4` (each at its hub, in the order of
//! the motors) and `disc_1`...`disc_4`. Everything is original, made here (MIT).

use engine::gltf::{Vertex, Writer, WrittenMaterial};
use glam::{Mat3, Vec2, Vec3, Vec4};
use std::f32::consts::{PI, TAU};

/// fpv5's motors' hubs, in `multirotor.ron`'s order: rear right, front right, rear left, front
/// left, and whether each turns clockwise seen from above.
const MOTORS: [(f32, f32, bool); 4] = [(-0.0795, 0.0795, true), (0.0795, 0.0795, false), (-0.0795, -0.0795, false), (0.0795, -0.0795, true)];
const HUB_Z: f32 = -0.022;
const PROP_RADIUS: f32 = 0.0635;
const UPTILT: f32 = 25.0 * PI / 180.0;

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

    /// A triangle with a normal at each corner (smooth surfaces).
    fn tri_n(&mut self, p: [Vec3; 3], n: [Vec3; 3], color: Vec4) {
        for k in 0..3 {
            self.vertices.push(Vertex { position: p[k], normal: n[k].normalize_or_zero(), color, uv: Vec2::ZERO });
        }
    }

    /// The same triangles facing the other way.
    fn flipped(mut self) -> Shape {
        for t in self.vertices.chunks_mut(3) {
            t.swap(1, 2);
            for v in t {
                v.normal = -v.normal;
            }
        }
        self
    }

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec4) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    /// A closed outline (convex, counter-clockwise in x, y) extruded from `top` down to `bottom`
    /// (z down: top < bottom), its faces outwards.
    fn prism(&mut self, outline: &[Vec2], top: f32, bottom: f32, color: Vec4) {
        let middle = outline.iter().copied().sum::<Vec2>() / outline.len() as f32;
        let at = |p: Vec2, z: f32| Vec3::new(p.x, p.y, z);
        for i in 0..outline.len() {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            // Top (facing −z) and bottom (facing +z).
            self.tri(at(middle, top), at(b, top), at(a, top), color);
            self.tri(at(middle, bottom), at(a, bottom), at(b, bottom), color);
            self.quad(at(a, top), at(b, top), at(b, bottom), at(a, bottom), color);
        }
    }

    /// A box between two corners, turned by `turn` about `about`.
    fn cuboid(&mut self, lo: Vec3, hi: Vec3, color: Vec4, turn: Mat3, about: Vec3) {
        let p = |x: f32, y: f32, z: f32| about + turn * (Vec3::new(x, y, z) - about);
        let (a, b) = (lo, hi);
        let c = [
            p(a.x, a.y, a.z),
            p(b.x, a.y, a.z),
            p(b.x, b.y, a.z),
            p(a.x, b.y, a.z),
            p(a.x, a.y, b.z),
            p(b.x, a.y, b.z),
            p(b.x, b.y, b.z),
            p(a.x, b.y, b.z),
        ];
        for [i, j, k, l] in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
            self.quad(c[i], c[j], c[k], c[l], color);
        }
    }

    /// A cylinder of `radius` from `a` to `b`, with its ends closed; smooth sides.
    fn cylinder(&mut self, a: Vec3, b: Vec3, radius: f32, segments: usize, color: Vec4) {
        self.frustum(a, b, radius, radius, segments, color);
    }

    /// A cone cut off: `ra` at `a`, `rb` at `b`, the ends closed.
    fn frustum(&mut self, a: Vec3, b: Vec3, ra: f32, rb: f32, segments: usize, color: Vec4) {
        let axis = (b - a).normalize();
        let u = axis.any_orthonormal_vector();
        let v = axis.cross(u);
        let slope = (ra - rb) / (b - a).length();
        for i in 0..segments {
            let (t0, t1) = (TAU * i as f32 / segments as f32, TAU * (i + 1) as f32 / segments as f32);
            let d0 = u * t0.cos() + v * t0.sin();
            let d1 = u * t1.cos() + v * t1.sin();
            let (n0, n1) = (d0 + axis * slope, d1 + axis * slope);
            self.tri_n([a + d0 * ra, a + d1 * ra, b + d1 * rb], [n0, n1, n1], color);
            self.tri_n([a + d0 * ra, b + d1 * rb, b + d0 * rb], [n0, n1, n0], color);
            if ra > 0.0 {
                self.tri(a, a + d1 * ra, a + d0 * ra, color);
            }
            if rb > 0.0 {
                self.tri(b, b + d0 * rb, b + d1 * rb, color);
            }
        }
    }
}

/// A stadium: round ends of `radius` at `a` and `b`, as an outline counter-clockwise in x, y (the
/// order `Shape::prism` takes).
fn stadium(a: Vec2, b: Vec2, radius: f32, segments: usize) -> Vec<Vec2> {
    let along = (b - a).normalize();
    let side = Vec2::new(-along.y, along.x);
    let mut out = Vec::new();
    for (centre, from) in [(b, -PI / 2.0), (a, PI / 2.0)] {
        for i in 0..=segments {
            let t = from + PI * i as f32 / segments as f32;
            out.push(centre + (along * t.cos() + side * t.sin()) * radius);
        }
    }
    out
}

/// A rectangle with rounded corners, centred, as an outline.
fn rounded(centre: Vec2, half: Vec2, radius: f32, segments: usize) -> Vec<Vec2> {
    let mut out = Vec::new();
    let corners = [(1.0, 1.0, 0.0), (-1.0, 1.0, PI / 2.0), (-1.0, -1.0, PI), (1.0, -1.0, 1.5 * PI)];
    for (sx, sy, from) in corners {
        let c = centre + Vec2::new(sx * (half.x - radius), sy * (half.y - radius));
        for i in 0..=segments {
            let t = from + PI / 2.0 * i as f32 / segments as f32;
            out.push(c + Vec2::new(t.cos(), t.sin()) * radius);
        }
    }
    out
}

/// Linear colour from sRGB bytes.
fn srgb(r: u8, g: u8, b: u8) -> Vec4 {
    let f = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    Vec4::new(f(r), f(g), f(b), 1.0)
}

/// One blade of fpv5's two-blade 5x3 propeller (`blade_of`).
fn blade(s: &mut Shape, dir: Vec3, ahead: Vec3, color: Vec4) {
    blade_of(s, dir, ahead, color, PROP_RADIUS, 0.0762, 1.0);
}

/// One blade of a propeller of `radius` and `pitch` (m), from the hub out along `dir` (in the
/// disc's plane), turning towards `ahead`: chord (times `chord`) and twist as such a blade has
/// them, its leading edge high.
fn blade_of(s: &mut Shape, dir: Vec3, ahead: Vec3, color: Vec4, radius: f32, pitch: f32, chord_scale: f32) {
    let up = Vec3::NEG_Z;
    let stations = 9;
    let section = |k: usize| {
        let t = k as f32 / (stations - 1) as f32;
        let r = 0.007 + (radius - 0.007) * t;
        // Wide near the root, narrowing to a rounded tip.
        let chord = (0.0115 + 0.0065 * (PI * (t * 0.8 + 0.1)).sin() - if t > 0.85 { (t - 0.85) / 0.15 * 0.008 } else { 0.0 }) * chord_scale;
        // The pitch: the angle whose tangent is the pitch over the circle at r.
        let angle = (pitch / (TAU * r)).atan().min(0.6);
        let (lead, trail) = (ahead * angle.cos() + up * angle.sin(), -(ahead * angle.cos() + up * angle.sin()));
        let centre = dir * r;
        let thick = up * 0.0007 * (1.0 - 0.6 * t);
        [centre + lead * chord * 0.42 + thick, centre + trail * chord * 0.58 + thick, centre + lead * chord * 0.42 - thick, centre + trail * chord * 0.58 - thick]
    };
    for k in 0..stations - 1 {
        let (a, b) = (section(k), section(k + 1));
        // Top and bottom faces, the two edges.
        s.quad(a[0], b[0], b[1], a[1], color);
        s.quad(a[2], a[3], b[3], b[2], color);
        s.quad(a[0], a[2], b[2], b[0], color);
        s.quad(a[1], b[1], b[3], a[3], color);
    }
    let tip = section(stations - 1);
    s.quad(tip[0], tip[2], tip[3], tip[1], color);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The first argument names the quad; a file instead is fpv5's model written there.
    let (which, out) = match args.first().map(String::as_str) {
        Some("race5") => ("race5", args.get(1).cloned()),
        Some("fpv5") => ("fpv5", args.get(1).cloned()),
        Some(file) => ("fpv5", Some(file.to_string())),
        None => ("fpv5", None),
    };
    let out = out.unwrap_or_else(|| format!("assets/aircraft/{which}/exterior.glb"));
    let w = if which == "race5" { race5() } else { fpv5() };
    let bytes = w.glb();
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("{out}: {e}"));
    let model = engine::gltf::read(&bytes).expect("it reads back");
    let triangles: usize = model.nodes.iter().map(|n| n.vertices.len() / 3).sum();
    println!("{out}: {} nodes, {triangles} triangles, {} bytes", model.nodes.len(), bytes.len());
}

/// race5's motors' hubs, in its `multirotor.ron`'s order, and whether each turns clockwise.
const RACE_MOTORS: [(f32, f32, bool); 4] = [(-0.0778, 0.0778, true), (0.0778, 0.0778, false), (-0.0778, -0.0778, false), (0.0778, -0.0778, true)];
/// A 5.1-inch propeller's radius and its pitch of 4.3 inches, m.
const RACE_PROP_RADIUS: f32 = 0.06475;
const RACE_PROP_PITCH: f32 = 0.109;
const RACE_UPTILT: f32 = 40.0 * PI / 180.0;

/// The racing quad: as fpv5's model, slimmer and lower, in a race build's colours.
fn race5() -> Writer {
    let mut w = Writer::new();
    let mut material = |name: &str, metallic: f32, roughness: f32| w.material(WrittenMaterial { name: name.into(), base: Vec4::ONE, metallic, roughness, emissive: Vec3::ZERO });
    let carbon_m = material("carbon", 0.0, 0.3);
    let metal_m = material("anodised", 1.0, 0.28);
    let plastic_m = material("plastic", 0.0, 0.55);
    let gloss_m = material("glossy", 0.0, 0.15);
    let rubber_m = material("rubber", 0.0, 0.9);

    let carbon = srgb(24, 25, 28);
    let carbon_edge = srgb(62, 64, 68);
    let neon = srgb(150, 255, 30);
    let teal = srgb(0, 185, 175);
    let stator = srgb(40, 40, 44);
    let copper = srgb(185, 100, 55);
    let bell = srgb(30, 30, 34);
    let pcb = srgb(25, 25, 30);
    let pcb_blue = srgb(30, 50, 140);
    let black = srgb(16, 16, 18);
    let white = srgb(232, 234, 236);
    let yellow = srgb(240, 190, 20);
    let red = srgb(205, 30, 40);
    let lens = srgb(20, 30, 60);
    let silver = srgb(170, 172, 178);

    // The frame: four arms of 5 mm carbon, 13 mm wide, on the diagonals with round pads under the
    // motors; a narrow bottom plate, a low top plate on short standoffs.
    let (arm_top, arm_bottom) = (0.004f32, 0.009f32);
    let mut frame = Shape::default();
    let mut frame_metal = Shape::default();
    let mut frame_edge = Shape::default();
    for &(x, y, _) in &RACE_MOTORS {
        let end = Vec2::new(x, y);
        frame.prism(&stadium(end * 0.15, end, 0.0065, 6), arm_top, arm_bottom, carbon);
        let pad: Vec<Vec2> = (0..20).map(|i| end + Vec2::from_angle(TAU * i as f32 / 20.0) * 0.0135).collect();
        frame_edge.prism(&pad, arm_top - 0.0002, arm_bottom + 0.0002, carbon_edge);
    }
    frame.prism(&rounded(Vec2::new(-0.004, 0.0), Vec2::new(0.050, 0.019), 0.007, 4), arm_top - 0.0005, arm_bottom - 0.002, carbon_edge);
    frame.prism(&rounded(Vec2::new(-0.004, 0.0), Vec2::new(0.044, 0.0185), 0.006, 4), -0.0145, -0.0125, carbon);
    for (x, y) in [(0.028, 0.0135), (0.028, -0.0135), (-0.038, 0.0135), (-0.038, -0.0135)] {
        frame_metal.cylinder(Vec3::new(x, y, -0.0125), Vec3::new(x, y, arm_top), 0.0024, 10, black);
        frame_metal.cylinder(Vec3::new(x, y, -0.0155), Vec3::new(x, y, -0.0145), 0.0030, 6, neon);
    }
    // The camera's side plates (green TPU) and the antenna's mount at the back.
    let mut tpu = Shape::default();
    for side in [-1.0f32, 1.0] {
        let y = side * 0.0130;
        tpu.cuboid(Vec3::new(0.034, y - 0.0015, -0.0145), Vec3::new(0.068, y + 0.0015, arm_top), neon, Mat3::IDENTITY, Vec3::ZERO);
    }
    tpu.cuboid(Vec3::new(-0.058, -0.008, -0.0125), Vec3::new(-0.048, 0.008, -0.003), neon, Mat3::IDENTITY, Vec3::ZERO);
    // A short whip antenna back and up, its tip in a little cap.
    let foot = Vec3::new(-0.054, 0.0, -0.010);
    let tip = foot + Vec3::new(-0.022, 0.0, -0.034);
    let mut antenna = Shape::default();
    antenna.cylinder(foot, tip, 0.0012, 8, black);
    let mut antenna_cap = Shape::default();
    antenna_cap.cylinder(tip, tip + (tip - foot).normalize() * 0.006, 0.0022, 10, teal);
    // The pack's leads and plug.
    let mut plug = Shape::default();
    plug.cuboid(Vec3::new(-0.076, -0.008, -0.034), Vec3::new(-0.062, 0.008, -0.024), yellow, Mat3::IDENTITY, Vec3::ZERO);
    let mut leads = Shape::default();
    leads.cylinder(Vec3::new(-0.062, -0.004, -0.029), Vec3::new(-0.044, -0.004, -0.024), 0.0022, 8, red);
    leads.cylinder(Vec3::new(-0.062, 0.004, -0.029), Vec3::new(-0.044, 0.004, -0.024), 0.0022, 8, black);

    let frame_node = w.node(
        "frame",
        None,
        Vec3::ZERO,
        &[(carbon_m, frame.vertices, false), (carbon_m, frame_edge.vertices, false), (metal_m, frame_metal.vertices, false), (rubber_m, tpu.vertices, false), (plastic_m, antenna.vertices, false), (gloss_m, antenna_cap.vertices, false), (plastic_m, plug.vertices, false), (rubber_m, leads.vertices, false)],
    );

    // The stack: a 4-in-1 ESC and the flight controller, 30 mm boards, and the capacitor.
    let mut stack = Shape::default();
    stack.cuboid(Vec3::new(-0.024, -0.0150, -0.002), Vec3::new(0.010, 0.0150, 0.0005), pcb_blue, Mat3::IDENTITY, Vec3::ZERO);
    stack.cuboid(Vec3::new(-0.022, -0.0140, -0.0085), Vec3::new(0.008, 0.0140, -0.0065), pcb, Mat3::IDENTITY, Vec3::ZERO);
    let mut cap = Shape::default();
    cap.cylinder(Vec3::new(-0.034, -0.011, -0.004), Vec3::new(-0.034, 0.011, -0.004), 0.004, 12, black);
    w.node("stack", Some(frame_node), Vec3::ZERO, &[(plastic_m, stack.vertices, false), (plastic_m, cap.vertices, false)]);

    // The pack: 6S 1300 mAh, 78 x 39 x 36 mm, a white wrap with a teal band and label, a black
    // strap through the top plate, a grip pad under it.
    let mut battery = Shape::default();
    battery.cuboid(Vec3::new(-0.043, -0.0195, -0.0515), Vec3::new(0.035, 0.0195, -0.0155), white, Mat3::IDENTITY, Vec3::ZERO);
    let mut label = Shape::default();
    for side in [-1.0f32, 1.0] {
        label.cuboid(Vec3::new(-0.036, side * 0.0195 - 0.0003, -0.040), Vec3::new(0.028, side * 0.0195 + 0.0003, -0.027), teal, Mat3::IDENTITY, Vec3::ZERO);
    }
    label.cuboid(Vec3::new(-0.038, -0.013, -0.0520), Vec3::new(0.030, 0.013, -0.0514), teal, Mat3::IDENTITY, Vec3::ZERO);
    // Its ends rounded off in a dark band, as the shrink-wrap folds over them.
    for x in [-0.043f32, 0.035] {
        label.cuboid(Vec3::new(x - 0.0006, -0.0198, -0.0518), Vec3::new(x + 0.0006, 0.0198, -0.0152), srgb(50, 52, 56), Mat3::IDENTITY, Vec3::ZERO);
    }
    let mut strap = Shape::default();
    strap.cuboid(Vec3::new(-0.014, -0.0205, -0.0528), Vec3::new(0.006, 0.0205, -0.0515), black, Mat3::IDENTITY, Vec3::ZERO);
    for side in [-1.0f32, 1.0] {
        strap.cuboid(Vec3::new(-0.014, side * 0.0205 - 0.0007, -0.0528), Vec3::new(0.006, side * 0.0205 + 0.0007, -0.0125), black, Mat3::IDENTITY, Vec3::ZERO);
    }
    let mut pad = Shape::default();
    pad.cuboid(Vec3::new(-0.041, -0.018, -0.0155), Vec3::new(0.033, 0.018, -0.0145), srgb(60, 60, 60), Mat3::IDENTITY, Vec3::ZERO);
    w.node("battery", Some(frame_node), Vec3::ZERO, &[(plastic_m, battery.vertices, false), (gloss_m, label.vertices, false), (rubber_m, strap.vertices, false), (rubber_m, pad.vertices, false)]);

    // The camera: a 19 mm cube between the side plates, tilted up 40°, its lens out of the front
    // where `multirotor.ron` puts it.
    let about = Vec3::new(0.050, 0.0, -0.004);
    let turn = Mat3::from_rotation_y(RACE_UPTILT);
    let mut camera = Shape::default();
    camera.cuboid(Vec3::new(0.0405, -0.0095, -0.0135), Vec3::new(0.0595, 0.0095, 0.0055), black, turn, about);
    let mut lens_body = Shape::default();
    let ahead = turn * Vec3::X;
    let face = about + turn * (Vec3::new(0.0595, 0.0, -0.004) - about);
    lens_body.cylinder(face, face + ahead * 0.006, 0.0066, 16, srgb(40, 40, 44));
    let mut glass = Shape::default();
    glass.frustum(face + ahead * 0.006, face + ahead * 0.0078, 0.0058, 0.0038, 16, lens);
    w.node("camera", Some(frame_node), Vec3::ZERO, &[(plastic_m, camera.vertices, false), (metal_m, lens_body.vertices, false), (gloss_m, glass.vertices, false)]);

    // The motors: 2207s, a little wider and taller than fpv5's 2306s; dark bells with a green
    // ring at the top.
    let mut bases = Shape::default();
    let mut bells = Shape::default();
    let mut rings = Shape::default();
    let mut windings = Shape::default();
    let mut shafts = Shape::default();
    for &(x, y, _) in &RACE_MOTORS {
        let at = |z: f32| Vec3::new(x, y, z);
        bases.cylinder(at(arm_top), at(arm_top - 0.004), 0.0140, 24, stator);
        windings.cylinder(at(arm_top - 0.004), at(arm_top - 0.0065), 0.0125, 18, copper);
        bells.cylinder(at(arm_top - 0.0065), at(-0.0160), 0.0140, 24, bell);
        rings.cylinder(at(-0.0160), at(-0.0185), 0.0141, 24, neon);
        bells.frustum(at(-0.0185), at(-0.0200), 0.0141, 0.0095, 24, bell);
        shafts.cylinder(at(-0.0200), at(HUB_Z + 0.002), 0.0025, 10, silver);
    }
    w.node("motors", Some(frame_node), Vec3::ZERO, &[(plastic_m, bases.vertices, false), (metal_m, windings.vertices, false), (metal_m, bells.vertices, false), (metal_m, rings.vertices, false), (metal_m, shafts.vertices, false)]);

    // The propellers, three blades each in a node at its hub, and the blur of each.
    for (i, &(x, y, clockwise)) in RACE_MOTORS.iter().enumerate() {
        let hub = Vec3::new(x, y, HUB_Z);
        let mut blades = Shape::default();
        let mut mirrored = Shape::default();
        let turning = if clockwise { 1.0 } else { -1.0 };
        let phase = (i as f32) * 0.7;
        for k in 0..3 {
            let a = phase + TAU * k as f32 / 3.0;
            let dir = Vec3::new(a.cos(), a.sin(), 0.0);
            let ahead = Vec3::new(-a.sin(), a.cos(), 0.0) * turning;
            blade_of(if clockwise { &mut blades } else { &mut mirrored }, dir, ahead, neon, RACE_PROP_RADIUS, RACE_PROP_PITCH, 0.85);
        }
        blades.vertices.extend(mirrored.flipped().vertices);
        let mut nut = Shape::default();
        nut.cylinder(Vec3::new(0.0, 0.0, 0.0018), Vec3::new(0.0, 0.0, -0.0032), 0.0062, 16, neon);
        nut.frustum(Vec3::new(0.0, 0.0, -0.0032), Vec3::new(0.0, 0.0, -0.0085), 0.0045, 0.0018, 12, black);
        w.node(&format!("prop_{}", i + 1), Some(frame_node), hub, &[(plastic_m, blades.vertices, false), (metal_m, nut.vertices, false)]);
        let mut disc = Shape::default();
        let segments = 40;
        for k in 0..segments {
            let (t0, t1) = (TAU * k as f32 / segments as f32, TAU * (k + 1) as f32 / segments as f32);
            let p0 = Vec3::new(t0.cos(), t0.sin(), 0.0) * RACE_PROP_RADIUS;
            let p1 = Vec3::new(t1.cos(), t1.sin(), 0.0) * RACE_PROP_RADIUS;
            disc.tri(Vec3::ZERO, p1, p0, srgb(215, 240, 190));
            disc.tri(Vec3::ZERO, p0, p1, srgb(215, 240, 190));
        }
        w.node(&format!("disc_{}", i + 1), Some(frame_node), hub, &[(plastic_m, disc.vertices, false)]);
    }
    w
}


/// The freestyle quad.
fn fpv5() -> Writer {
    let mut w = Writer::new();
    let mut material = |name: &str, metallic: f32, roughness: f32| w.material(WrittenMaterial { name: name.into(), base: Vec4::ONE, metallic, roughness, emissive: Vec3::ZERO });
    let carbon_m = material("carbon", 0.0, 0.35);
    let metal_m = material("anodised", 1.0, 0.3);
    let plastic_m = material("plastic", 0.0, 0.6);
    let gloss_m = material("glossy", 0.0, 0.15);
    let rubber_m = material("rubber", 0.0, 0.9);

    let carbon = srgb(32, 33, 36);
    let carbon_edge = srgb(70, 72, 76);
    let purple = srgb(120, 40, 190);
    let red = srgb(205, 30, 40);
    let stator = srgb(45, 45, 48);
    let copper = srgb(180, 95, 50);
    let orange = srgb(255, 95, 10);
    let pcb_green = srgb(20, 90, 50);
    let pcb_blue = srgb(25, 45, 120);
    let black = srgb(18, 18, 20);
    let yellow = srgb(240, 190, 20);
    let white = srgb(225, 225, 225);
    let lens = srgb(20, 30, 60);
    let silver = srgb(170, 172, 178);

    // The frame: four arms of 5 mm carbon on the diagonals with pads under the motors, the bottom
    // plate, the top plate, the standoffs.
    let (arm_top, arm_bottom) = (0.004f32, 0.009f32);
    let mut frame = Shape::default();
    let mut frame_metal = Shape::default();
    let mut frame_edge = Shape::default();
    for &(x, y, _) in &MOTORS {
        let end = Vec2::new(x, y);
        frame.prism(&stadium(end * 0.12, end, 0.0085, 6), arm_top, arm_bottom, carbon);
        // The motor's pad: round, a little wider.
        let pad: Vec<Vec2> = (0..20).map(|i| end + Vec2::from_angle(TAU * i as f32 / 20.0) * 0.0145).collect();
        frame_edge.prism(&pad, arm_top - 0.0002, arm_bottom + 0.0002, carbon_edge);
    }
    frame.prism(&rounded(Vec2::new(-0.004, 0.0), Vec2::new(0.052, 0.024), 0.008, 4), arm_top - 0.0005, arm_bottom - 0.002, carbon_edge);
    frame.prism(&rounded(Vec2::new(-0.006, 0.0), Vec2::new(0.046, 0.021), 0.007, 4), -0.0145, -0.0125, carbon);
    for (x, y) in [(0.030, 0.015), (0.030, -0.015), (-0.040, 0.015), (-0.040, -0.015)] {
        frame_metal.cylinder(Vec3::new(x, y, -0.0125), Vec3::new(x, y, arm_top), 0.0026, 10, purple);
        frame_metal.cylinder(Vec3::new(x, y, -0.0155), Vec3::new(x, y, -0.0145), 0.0032, 6, purple);
    }
    // The camera's side plates (TPU, orange), and a rear guard for the antenna.
    let mut tpu = Shape::default();
    for side in [-1.0f32, 1.0] {
        let y = side * 0.0135;
        tpu.cuboid(Vec3::new(0.036, y - 0.0015, -0.0145), Vec3::new(0.068, y + 0.0015, arm_top), orange, Mat3::IDENTITY, Vec3::ZERO);
    }
    tpu.cuboid(Vec3::new(-0.060, -0.010, -0.0125), Vec3::new(-0.050, 0.010, -0.002), orange, Mat3::IDENTITY, Vec3::ZERO);
    // The video antenna, back and up, its cap red.
    let foot = Vec3::new(-0.056, 0.0, -0.010);
    let tip = foot + Vec3::new(-0.030, 0.0, -0.030);
    let mut antenna = Shape::default();
    antenna.cylinder(foot, tip, 0.0018, 8, black);
    let mut antenna_cap = Shape::default();
    antenna_cap.cylinder(tip, tip + (tip - foot).normalize() * 0.016, 0.0048, 12, red);
    // The battery's leads and plug.
    let mut plug = Shape::default();
    plug.cuboid(Vec3::new(-0.074, -0.008, -0.030), Vec3::new(-0.060, 0.008, -0.020), yellow, Mat3::IDENTITY, Vec3::ZERO);
    let mut leads = Shape::default();
    leads.cylinder(Vec3::new(-0.060, -0.004, -0.025), Vec3::new(-0.042, -0.004, -0.022), 0.0022, 8, red);
    leads.cylinder(Vec3::new(-0.060, 0.004, -0.025), Vec3::new(-0.042, 0.004, -0.022), 0.0022, 8, black);

    let frame_node = w.node(
        "frame",
        None,
        Vec3::ZERO,
        &[(carbon_m, frame.vertices, false), (carbon_m, frame_edge.vertices, false), (metal_m, frame_metal.vertices, false), (rubber_m, tpu.vertices, false), (plastic_m, antenna.vertices, false), (gloss_m, antenna_cap.vertices, false), (plastic_m, plug.vertices, false), (rubber_m, leads.vertices, false)],
    );

    // The stack between the plates: the ESC and the flight controller, with a capacitor.
    let mut stack = Shape::default();
    stack.cuboid(Vec3::new(-0.025, -0.0155, -0.002), Vec3::new(0.011, 0.0155, 0.0005), pcb_blue, Mat3::IDENTITY, Vec3::ZERO);
    stack.cuboid(Vec3::new(-0.023, -0.0145, -0.0085), Vec3::new(0.009, 0.0145, -0.0065), pcb_green, Mat3::IDENTITY, Vec3::ZERO);
    let mut cap = Shape::default();
    cap.cylinder(Vec3::new(-0.036, -0.012, -0.004), Vec3::new(-0.036, 0.012, -0.004), 0.0045, 12, black);
    w.node("stack", Some(frame_node), Vec3::ZERO, &[(plastic_m, stack.vertices, false), (plastic_m, cap.vertices, false)]);

    // The battery: 4S 1300 mAh, black wrap with a yellow label, a red strap round it and the
    // top plate, a grip pad under it.
    let mut battery = Shape::default();
    battery.cuboid(Vec3::new(-0.042, -0.0175, -0.0465), Vec3::new(0.036, 0.0175, -0.0155), black, Mat3::IDENTITY, Vec3::ZERO);
    let mut label = Shape::default();
    for side in [-1.0f32, 1.0] {
        label.cuboid(Vec3::new(-0.030, side * 0.0175 - 0.0003, -0.042), Vec3::new(0.024, side * 0.0175 + 0.0003, -0.020), yellow, Mat3::IDENTITY, Vec3::ZERO);
    }
    label.cuboid(Vec3::new(-0.034, -0.012, -0.0470), Vec3::new(0.028, 0.012, -0.0464), white, Mat3::IDENTITY, Vec3::ZERO);
    let mut strap = Shape::default();
    strap.cuboid(Vec3::new(-0.012, -0.0185, -0.0478), Vec3::new(0.008, 0.0185, -0.0465), red, Mat3::IDENTITY, Vec3::ZERO);
    for side in [-1.0f32, 1.0] {
        strap.cuboid(Vec3::new(-0.012, side * 0.0185 - 0.0007, -0.0478), Vec3::new(0.008, side * 0.0185 + 0.0007, -0.0125), red, Mat3::IDENTITY, Vec3::ZERO);
    }
    let mut pad = Shape::default();
    pad.cuboid(Vec3::new(-0.040, -0.016, -0.0155), Vec3::new(0.034, 0.016, -0.0145), srgb(60, 60, 60), Mat3::IDENTITY, Vec3::ZERO);
    w.node("battery", Some(frame_node), Vec3::ZERO, &[(plastic_m, battery.vertices, false), (gloss_m, label.vertices, false), (rubber_m, strap.vertices, false), (rubber_m, pad.vertices, false)]);

    // The camera: a 19 mm cube between the side plates, tilted up, its lens out of the front.
    let about = Vec3::new(0.052, 0.0, -0.005);
    let turn = Mat3::from_rotation_y(UPTILT);
    let mut camera = Shape::default();
    camera.cuboid(Vec3::new(0.0425, -0.0095, -0.0145), Vec3::new(0.0615, 0.0095, 0.0045), black, turn, about);
    let mut lens_body = Shape::default();
    let ahead = turn * Vec3::X;
    let face = about + turn * (Vec3::new(0.0615, 0.0, -0.005) - about);
    lens_body.cylinder(face, face + ahead * 0.007, 0.0068, 16, srgb(40, 40, 44));
    let mut glass = Shape::default();
    glass.frustum(face + ahead * 0.007, face + ahead * 0.0085, 0.0058, 0.0035, 16, lens);
    w.node("camera", Some(frame_node), Vec3::ZERO, &[(plastic_m, camera.vertices, false), (metal_m, lens_body.vertices, false), (gloss_m, glass.vertices, false)]);

    // The motors: base and stator, the bell, the shaft; on each pad.
    let mut bases = Shape::default();
    let mut bells = Shape::default();
    let mut windings = Shape::default();
    let mut shafts = Shape::default();
    for &(x, y, _) in &MOTORS {
        let at = |z: f32| Vec3::new(x, y, z);
        bases.cylinder(at(arm_top), at(arm_top - 0.004), 0.0145, 24, stator);
        windings.cylinder(at(arm_top - 0.004), at(arm_top - 0.0065), 0.0128, 18, copper);
        bells.cylinder(at(arm_top - 0.0065), at(-0.0175), 0.0145, 24, red);
        bells.frustum(at(-0.0175), at(-0.0195), 0.0145, 0.010, 24, red);
        shafts.cylinder(at(-0.0195), at(HUB_Z + 0.002), 0.0025, 10, silver);
    }
    w.node("motors", Some(frame_node), Vec3::ZERO, &[(plastic_m, bases.vertices, false), (metal_m, windings.vertices, false), (metal_m, bells.vertices, false), (metal_m, shafts.vertices, false)]);

    // The propellers, each in a node at its hub, and the blur of each.
    let prop_color = srgb(255, 70, 15);
    for (i, &(x, y, clockwise)) in MOTORS.iter().enumerate() {
        let hub = Vec3::new(x, y, HUB_Z);
        let mut blades = Shape::default();
        let mut mirrored = Shape::default();
        // Seen from above, clockwise is from x towards y; the blades turn their leading edge that way.
        let turning = if clockwise { 1.0 } else { -1.0 };
        let phase = (i as f32) * 0.7;
        for k in 0..2 {
            let a = phase + PI * k as f32;
            let dir = Vec3::new(a.cos(), a.sin(), 0.0);
            let ahead = Vec3::new(-a.sin(), a.cos(), 0.0) * turning;
            blade(if clockwise { &mut blades } else { &mut mirrored }, dir, ahead, prop_color);
        }
        // Built for a clockwise turn its faces point the right way; mirrored, they are turned over.
        blades.vertices.extend(mirrored.flipped().vertices);
        let mut nut = Shape::default();
        nut.cylinder(Vec3::new(0.0, 0.0, 0.0018), Vec3::new(0.0, 0.0, -0.0032), 0.0062, 16, prop_color);
        nut.frustum(Vec3::new(0.0, 0.0, -0.0032), Vec3::new(0.0, 0.0, -0.0085), 0.0045, 0.0018, 12, silver);
        w.node(&format!("prop_{}", i + 1), Some(frame_node), hub, &[(plastic_m, blades.vertices, false), (metal_m, nut.vertices, false)]);
        // The blur: a thin disc, both faces, drawn see-through by the renderer.
        let mut disc = Shape::default();
        let segments = 40;
        for k in 0..segments {
            let (t0, t1) = (TAU * k as f32 / segments as f32, TAU * (k + 1) as f32 / segments as f32);
            let p0 = Vec3::new(t0.cos(), t0.sin(), 0.0) * PROP_RADIUS;
            let p1 = Vec3::new(t1.cos(), t1.sin(), 0.0) * PROP_RADIUS;
            disc.tri(Vec3::ZERO, p1, p0, srgb(235, 200, 180));
            disc.tri(Vec3::ZERO, p0, p1, srgb(235, 200, 180));
        }
        w.node(&format!("disc_{}", i + 1), Some(frame_node), hub, &[(plastic_m, disc.vertices, false)]);
    }

    w
}
