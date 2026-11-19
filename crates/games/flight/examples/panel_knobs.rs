//! Makes the knobs of a cockpit's panel that the mouse and the keys turn (the `Knob`s of its
//! `cockpit.ron`'s `controls`: the C172's HDG, ALT and CRS / BARO on both G1000 bezels) parts of
//! their own in its `cockpit.glb`, so each can turn about its axis:
//!
//! - a single knob is the node `knob_<what>_<n>` (`knob_heading_1`: n counts the knobs of that
//!   name from the left); a dual one two nodes, its outer ring and its inner knob
//!   (`knob_altitude_coarse_1`, `knob_altitude_fine_1`);
//! - each is a machined knob: a cylinder with a fine straight knurl round it, a chamfered edge
//!   and a thin pale line on its face, from near the middle to short of the edge (a round knob
//!   without a mark does not show that it turns), where the plain knob was: the `rubber` triangles
//!   of the `cabin` node round the knob's axis are taken out, and the new nodes have their origin
//!   on the axis at the panel's face;
//! - the panel's other knobs (VOL, NAV, COM, RANGE, FMS, the audio panel's), which stay in the
//!   `cabin` node, are given the same smooth dark grey instead of the rubber's rough photograph,
//!   which made stones of things this small;
//! - every other node is written as it was read.
//!
//! `cargo build --release -p flight --example panel_knobs && target/release/examples/panel_knobs [c172] [out.glb]`
//! writes `assets/aircraft/c172/cockpit.glb` (or `out.glb`) and prints the `parts` to have in
//! `cockpit.ron`. Run again on its own result, it makes the same knobs again from where they are.
//!
//! Body axes: x forward, y right, z down, metres; the panel faces aft, so a knob stands out
//! towards −x and turns clockwise, seen from the seat, with a positive angle about +x.
//! Everything is original, made here (MIT).

use engine::gltf::{Model, Vertex, Writer, WrittenMaterial};
use glam::{Vec2, Vec3, Vec4};
use serde::de::IgnoredAny;
use serde::Deserialize;
use std::f32::consts::TAU;

/// What this reads of `cockpit.ron`: the knobs of its `controls` (`cockpit::PanelControl`).
#[derive(Deserialize)]
struct CockpitFile {
    controls: Vec<Control>,
}

#[derive(Deserialize)]
struct Control {
    centre: Vec3,
    shape: Shape,
}

#[derive(Deserialize)]
#[allow(dead_code)]
enum Shape {
    Key { half: IgnoredAny, does: IgnoredAny },
    Knob { radius: f32, outer: Knob, inner: Option<(f32, Knob)> },
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
enum Knob {
    Heading,
    AltitudeFine,
    AltitudeCoarse,
    Baro,
    Course,
}

impl Knob {
    fn name(self) -> &'static str {
        match self {
            Knob::Heading => "heading",
            Knob::AltitudeFine => "altitude_fine",
            Knob::AltitudeCoarse => "altitude_coarse",
            Knob::Baro => "baro",
            Knob::Course => "course",
        }
    }
}

/// A click of a knob, radians: 24 to a turn.
const CLICK: f32 = TAU / 24.0;
/// How deep a grip's knurl is, as a share of its radius, and its chamfer, metres.
const SCALLOP: f32 = 0.03;
const CHAMFER: f32 = 0.0007;
/// The line on the face: half its width, how far it stands proud of the face, how far short of
/// the chamfer it ends, and its colour (the dimming knobs' pointers').
const POINTER_HALF: f32 = 0.0004;
const POINTER_PROUD: f32 = 0.0001;
const POINTER_SHORT: f32 = 0.0006;
const POINTER: Vec4 = Vec4::new(0.85, 0.85, 0.82, 1.0);
/// Sections round a grip for each ridge of its knurl.
const STEPS: usize = 4;
/// The knobs' grey: lighter than the bezel's black, as Garmin's are, so they show from the seat;
/// and how rough they are (smooth, without a photograph's grain: the `plain` material).
const KNOB: Vec4 = Vec4::new(0.11, 0.11, 0.115, 1.0);
const KNOB_ROUGHNESS: f32 = 0.55;

/// Triangles of a grip and of its pointer, in the node's space: its axis is x through the origin,
/// the panel's face at x = 0, the pilot towards −x.
#[derive(Default)]
struct Grip {
    rubber: Vec<Vertex>,
    pointer: Vec<Vertex>,
}

/// A triangle with a normal at each corner, wound to face the way its normals do.
fn tri(out: &mut Vec<Vertex>, p: [Vec3; 3], n: [Vec3; 3], color: Vec4) {
    let facing = (p[1] - p[0]).cross(p[2] - p[0]).dot(n[0] + n[1] + n[2]) >= 0.0;
    for k in if facing { [0, 1, 2] } else { [0, 2, 1] } {
        out.push(Vertex { position: p[k], normal: n[k].normalize_or_zero(), color, uv: Vec2::ZERO });
    }
}

fn quad(out: &mut Vec<Vertex>, p: [Vec3; 4], n: [Vec3; 4], color: Vec4) {
    tri(out, [p[0], p[1], p[2]], [n[0], n[1], n[2]], color);
    tri(out, [p[0], p[2], p[3]], [n[0], n[2], n[3]], color);
}

/// A knurled grip of `radius` from `from` to `to` metres out of the panel, its face open within
/// `hole` (a dual knob's outer ring: the inner knob comes out of it), `scallops` ridges round it,
/// the line on its face pointing up (−z).
fn grip(radius: f32, from: f32, to: f32, hole: f32, scallops: usize, color: Vec4) -> Grip {
    let mut g = Grip::default();
    let sections = scallops * STEPS;
    // The pointer is on a crest: the angle 0 is up.
    let around = |k: usize| k as f32 / sections as f32 * TAU;
    let edge = |a: f32| radius * (1.0 - SCALLOP * (0.5 - 0.5 * (scallops as f32 * a).cos()));
    let slope = |a: f32| -radius * SCALLOP * 0.5 * scallops as f32 * (scallops as f32 * a).sin();
    // A point `r` from the axis at the angle `a` (0 up, clockwise seen from the seat), `out` of the panel.
    let at = |a: f32, r: f32, out: f32| Vec3::new(-out, r * a.sin(), -r * a.cos());
    // The grip's side faces out along the normal of its edge, a curve in polar coordinates.
    let side = |a: f32| {
        let (r, dr) = (edge(a), slope(a));
        Vec3::new(0.0, r * a.sin() - dr * a.cos(), -(r * a.cos() + dr * a.sin())).normalize()
    };
    let aft = Vec3::NEG_X;
    let shoulder = to - CHAMFER;
    for k in 0..sections {
        let (a, b) = (around(k), around(k + 1));
        let (ra, rb) = (edge(a), edge(b));
        let (na, nb) = (side(a), side(b));
        // The side, the chamfer and the face.
        quad(&mut g.rubber, [at(a, ra, from), at(b, rb, from), at(b, rb, shoulder), at(a, ra, shoulder)], [na, nb, nb, na], color);
        let (ca, cb) = ((na + aft).normalize(), (nb + aft).normalize());
        quad(&mut g.rubber, [at(a, ra, shoulder), at(b, rb, shoulder), at(b, rb - CHAMFER, to), at(a, ra - CHAMFER, to)], [ca, cb, cb, ca], color);
        if hole > 0.0 {
            quad(&mut g.rubber, [at(a, ra - CHAMFER, to), at(b, rb - CHAMFER, to), at(b, hole, to), at(a, hole, to)], [aft; 4], color);
        } else {
            tri(&mut g.rubber, [at(a, ra - CHAMFER, to), at(b, rb - CHAMFER, to), at(0.0, 0.0, to)], [aft; 3], color);
        }
    }
    // The line: a strip on the face from near the middle (or the hole) to short of the chamfer.
    let strip = |y: f32, r: f32| Vec3::new(-(to + POINTER_PROUD), y, -r);
    let w = POINTER_HALF;
    let inner = if hole > 0.0 { hole + 0.0012 } else { radius * 0.3 };
    let rim = radius * (1.0 - SCALLOP) - CHAMFER - POINTER_SHORT;
    quad(&mut g.pointer, [strip(-w, inner), strip(w, inner), strip(w, rim), strip(-w, rim)], [aft; 4], POINTER);
    g
}

/// Runs of triangles of one material, in order, as `Writer::node` takes them; `origin` is the
/// node's, taken off each vertex.
fn runs(model: &Model, w: &mut Writer, materials: &mut [Option<usize>], vertices: &[Vertex], of: &[usize], origin: Vec3) -> Vec<(usize, Vec<Vertex>, bool)> {
    let mut out: Vec<(usize, Vec<Vertex>, bool)> = Vec::new();
    let mut last = usize::MAX;
    for (t, triangle) in vertices.chunks(3).enumerate() {
        let read = of[t];
        if read != last {
            let m = &model.materials[read];
            let written = *materials[read].get_or_insert_with(|| w.material(WrittenMaterial { name: m.name.clone(), base: m.base, metallic: m.metallic, roughness: m.roughness, emissive: m.emissive }));
            // The pictures of the instruments' canvas are where their texture coordinates say.
            let name = m.name.split('.').next().unwrap_or("");
            out.push((written, Vec::new(), name == "screen" || name == "dial"));
            last = read;
        }
        let run = &mut out.last_mut().expect("just pushed").1;
        run.extend(triangle.iter().map(|v| Vertex { position: v.position - origin, ..*v }));
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cockpit = args.first().map_or("c172", String::as_str);
    let dir = format!("assets/aircraft/{cockpit}");
    let out = args.get(1).cloned().unwrap_or_else(|| format!("{dir}/cockpit.glb"));
    let text = std::fs::read_to_string(format!("{dir}/cockpit.ron")).unwrap_or_else(|e| panic!("{dir}/cockpit.ron: {e}"));
    let file: CockpitFile = ron::from_str(&text).unwrap_or_else(|e| panic!("{dir}/cockpit.ron: {e}"));
    let bytes = std::fs::read(format!("{dir}/cockpit.glb")).unwrap_or_else(|e| panic!("{dir}/cockpit.glb: {e}"));
    let mut model = engine::gltf::read(&bytes).unwrap_or_else(|e| panic!("{dir}/cockpit.glb: {e}"));
    // What the knobs are made of, also those left in the cabin.
    let knob_material = model.materials.len();
    model.materials.push(engine::gltf::Material { name: "plain".into(), roughness: KNOB_ROUGHNESS, ..Default::default() });
    let model = model;

    // The knobs, from the left, and the nodes each becomes: (name, what turns it, radius, hole, scallops).
    let mut knobs: Vec<(Vec3, f32, Knob, Option<(f32, Knob)>)> = file.controls.iter().filter_map(|c| match c.shape {
        Shape::Knob { radius, outer, inner } => Some((c.centre, radius, outer, inner)),
        Shape::Key { .. } => None,
    }).collect();
    knobs.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    let name = |knob: Knob, centre: Vec3| {
        let n = 1 + knobs.iter().filter(|k| (k.2 == knob || k.3.is_some_and(|(_, inner)| inner == knob)) && k.0.y < centre.y).count();
        format!("knob_{}_{n}", knob.name())
    };
    let made: Vec<String> = knobs.iter().flat_map(|&(centre, _, outer, inner)| std::iter::once(name(outer, centre)).chain(inner.map(|(_, k)| name(k, centre)))).collect();

    // A knob's triangles as they are now: the cabin's rubber round its axis, or the nodes made before.
    let near = |v: &Vertex, centre: Vec3, radius: f32| Vec2::new(v.position.y - centre.y, v.position.z - centre.z).length() < radius + 0.0005 && (centre.x - 0.03..centre.x + 0.011).contains(&v.position.x);
    let rubber = |m: usize| model.materials[m].name.split('.').next() == Some("rubber");
    let smooth = |node: &engine::gltf::Node, t: usize| rubber(node.materials[t]) && knobs.iter().any(|&(centre, radius, ..)| node.vertices[t * 3..t * 3 + 3].iter().all(|v| near(v, centre, radius)));

    // The panel's other knobs: the cabin's rubber on the panel's face, round where these are.
    let span = |of: fn(Vec3) -> f32| knobs.iter().map(|k| of(k.0)).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let ((y0, y1), (z0, z1), (x0, x1)) = (span(|c| c.y), span(|c| c.z), span(|c| c.x));
    let on_panel = |v: &Vertex| (x0 - 0.03..x1 + 0.011).contains(&v.position.x) && (y0 - 0.05..y1 + 0.05).contains(&v.position.y) && (z0 - 0.09..z1 + 0.03).contains(&v.position.z);
    let mut tinted = 0;

    let mut w = Writer::new();
    let mut materials: Vec<Option<usize>> = vec![None; model.materials.len()];
    let mut cabin = None;
    for node in &model.nodes {
        assert!(node.parent.is_none() && (node.local - glam::Mat4::from_translation(node.origin())).abs_diff_eq(glam::Mat4::ZERO, 0.0), "{}: only nodes moved from the model's origin are kept as they are", node.name);
        if made.contains(&node.name) {
            continue;
        }
        let keep: Vec<usize> = (0..node.materials.len()).filter(|&t| node.name != "cabin" || !smooth(node, t)).collect();
        let mut vertices: Vec<Vertex> = keep.iter().flat_map(|&t| node.vertices[t * 3..t * 3 + 3].iter().copied()).collect();
        let mut of: Vec<usize> = keep.iter().map(|&t| node.materials[t]).collect();
        if node.name == "cabin" {
            for (t, material) in of.iter_mut().enumerate() {
                if rubber(*material) && vertices[t * 3..t * 3 + 3].iter().all(on_panel) {
                    *material = knob_material;
                    vertices[t * 3..t * 3 + 3].iter_mut().for_each(|v| v.color = KNOB);
                    tinted += 1;
                }
            }
        }
        let primitives = runs(&model, &mut w, &mut materials, &vertices, &of, node.origin());
        let index = w.node(&node.name, None, node.origin(), &primitives);
        if node.name == "cabin" {
            cabin = Some((index, node.materials.len() - keep.len()));
        }
    }
    let (_, taken) = cabin.expect("a cabin node");

    let rubber_m = w.material(WrittenMaterial { name: "plain".into(), base: Vec4::ONE, metallic: 0.0, roughness: KNOB_ROUGHNESS, emissive: Vec3::ZERO });
    let pointer_m = w.material(WrittenMaterial { name: "plain".into(), base: Vec4::ONE, metallic: 0.0, roughness: 0.9, emissive: Vec3::ZERO });
    let black = KNOB;
    let mut parts = String::new();
    for &(centre, radius, outer, inner) in &knobs {
        // How far the knob stands: from the panel's face to the tip of what is there now.
        let old: Vec<&Vertex> = model
            .nodes
            .iter()
            .filter(|n| n.name == "cabin" || made.contains(&n.name))
            // The cabin's rubber, or a knob made before without its pale line.
            .flat_map(|n| n.vertices.chunks(3).zip(&n.materials).filter(|(_, m)| rubber(**m) || n.name != "cabin").flat_map(|(t, _)| t.iter()).filter(|v| v.color.x < 0.5 && near(v, centre, radius)))
            .collect();
        assert!(!old.is_empty(), "no knob at {centre}");
        let face = old.iter().map(|v| v.position.x).fold(f32::MIN, f32::max);
        let tip = face - old.iter().map(|v| v.position.x).fold(f32::MAX, f32::min);
        let origin = Vec3::new(face, centre.y, centre.z);
        // The outer ring (or the only knob) ends at the control's face.
        let ring = if inner.is_some() { face - centre.x } else { tip };
        let mut nodes = vec![(name(outer, centre), outer, grip(radius, 0.0, ring, inner.map_or(0.0, |(r, _)| r * 0.8), 40, black))];
        if let Some((r, knob)) = inner {
            nodes.push((name(knob, centre), knob, grip(r, ring - 0.0005, tip, 0.0, 28, black)));
        }
        for (name, knob, g) in nodes {
            println!("{name}: {} triangles, {:.1} mm out of the panel at x {face}", (g.rubber.len() + g.pointer.len()) / 3, if knob == outer { ring } else { tip } * 1000.0);
            w.node(&name, None, origin, &[(rubber_m, g.rubber, false), (pointer_m, g.pointer, false)]);
            parts += &format!("        (\n            node: \"{name}\",\n            moves: Turn(by: Knob({knob:?}), angle: {CLICK}),\n            axis: (1.0, 0.0, 0.0),\n        ),\n");
        }
    }

    let bytes = w.glb();
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("{out}: {e}"));
    let again = engine::gltf::read(&bytes).expect("it reads back");
    let triangles: usize = again.nodes.iter().map(|n| n.vertices.len() / 3).sum();
    println!("{out}: {} nodes, {triangles} triangles, {} bytes; {taken} triangles of the plain knobs taken out of the cabin, {tinted} of its other knobs made smooth", again.nodes.len(), bytes.len());
    println!("The parts of cockpit.ron, after the others:\n{parts}");
}
