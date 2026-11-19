//! Builds the GT coupé's model, `assets/cars/gt/exterior.glb`: an original two-door fastback
//! coupé of no make, of the size of `assets/cars/gt/car.ron` (4.4 m long, 1.9 m wide, 1.28 m high,
//! the wheels where its axles are).
//! `cargo build --release -p drive --example car_model && target/release/examples/car_model assets/cars/gt/exterior.glb`
//!
//! The model's axes are the car's body axes: x forward, y right, z down, metres from its centre
//! of gravity (the ground at z = 0.46). The nodes, which the game draws (`crates/games/drive/src/look.rs`):
//! - `body`: the shell (paint, black trim, lamps, grilles, the wells' liners, the shut lines) and
//!   what is fitted to it (splitter, diffuser, exhausts, mirrors);
//! - `glass`: the windscreen, the side windows and the rear window;
//! - `cabin`: the dashboard, seats, floor, mirror; `steering`: the steering wheel about its own
//!   centre, its column along x; `dial_rev`, `dial_speed`: the dials' faces about their centres;
//!   `hand_left`, `hand_right`: the driver's hands on the rim, in the steering wheel's axes;
//!   `arm`: a forearm, a metre long from its origin along x, stretched from wrist to elbow;
//! - `wheel_front`, `wheel_rear`: a wheel about its hub, the axle along y, its outer face towards
//!   +y (the left ones are drawn mirrored); `brake_front`, `brake_rear`: the disc and caliper
//!   behind it, which do not turn with it.
//!
//! Each triangle has a glTF material whose name says what it is made of (`MATERIALS`): the game's
//! shader draws paint, glass, lamps, rubber and the dials each in its own way, by that name. The
//! vertex colour is how open the point is to the sky (ambient occlusion), a grey.
//!
//! The shapes are fields (signed distances) turned into triangles, `sdf.rs`: the body in `body.rs`,
//! the wheels and brakes in `wheel.rs`, the inside in `cabin.rs`. Everything is made here (MIT).

mod body;
mod cabin;
mod sdf;
mod wheel;

use engine::gltf::{Vertex, Writer, WrittenMaterial};
use glam::{Vec2, Vec3, Vec4};
use sdf::{Corner, Triangle};

/// Height of the centre of gravity: the ground is at z = GROUND in body axes.
const GROUND: f32 = 0.46;

/// A material: name, base colour (linear), metallic, roughness, emissive colour.
type Material = (&'static str, [f32; 3], f32, f32, [f32; 3]);

const MATERIALS: [Material; 37] = [
    ("paint", [0.34, 0.013, 0.012], 0.7, 0.28, [0.0; 3]),
    ("glass", [0.02, 0.026, 0.03], 0.0, 0.03, [0.0; 3]),
    ("trim", [0.012, 0.012, 0.013], 0.0, 0.86, [0.0; 3]),
    ("liner", [0.012, 0.012, 0.012], 0.0, 0.95, [0.0; 3]),
    ("gap", [0.004, 0.004, 0.004], 0.0, 1.0, [0.0; 3]),
    ("grille", [0.014, 0.014, 0.015], 0.0, 0.8, [0.0; 3]),
    ("carbon", [0.012, 0.012, 0.014], 0.0, 0.8, [0.0; 3]),
    ("fin", [0.05, 0.05, 0.055], 0.0, 0.6, [0.0; 3]),
    ("plate", [0.5, 0.5, 0.47], 0.0, 0.4, [0.0; 3]),
    ("chrome", [0.62, 0.62, 0.64], 1.0, 0.16, [0.0; 3]),
    ("burnt", [0.05, 0.04, 0.035], 1.0, 0.55, [0.0; 3]),
    ("soot", [0.008, 0.008, 0.008], 0.0, 1.0, [0.0; 3]),
    ("mirror", [0.92, 0.93, 0.95], 1.0, 0.02, [0.0; 3]),
    ("lamp_dark", [0.02, 0.02, 0.024], 0.0, 0.04, [0.0; 3]),
    ("lamp_ring", [0.85, 0.88, 0.95], 0.0, 0.1, [1.0, 1.0, 1.0]),
    ("lamp_head", [0.75, 0.76, 0.78], 0.0, 0.05, [1.0, 0.96, 0.9]),
    ("lamp_tail", [0.45, 0.012, 0.012], 0.0, 0.08, [1.0, 0.04, 0.02]),
    ("lamp_brake", [0.30, 0.008, 0.008], 0.0, 0.08, [1.0, 0.035, 0.015]),
    ("lamp_lens", [0.085, 0.003, 0.004], 0.0, 0.05, [1.0, 0.03, 0.015]),
    ("lamp_reflex", [0.30, 0.01, 0.008], 0.0, 0.12, [1.0, 0.03, 0.015]),
    ("rubber", [0.02, 0.02, 0.021], 0.0, 0.85, [0.0; 3]),
    ("rim", [0.30, 0.30, 0.32], 1.0, 0.3, [0.0; 3]),
    ("spokes", [0.30, 0.30, 0.32], 1.0, 0.3, [0.0; 3]),
    ("blur", [0.16, 0.16, 0.17], 1.0, 0.45, [0.0; 3]),
    ("disc", [0.42, 0.42, 0.44], 1.0, 0.45, [0.0; 3]),
    ("hub", [0.07, 0.07, 0.075], 1.0, 0.55, [0.0; 3]),
    ("caliper", [0.85, 0.42, 0.015], 0.0, 0.3, [0.0; 3]),
    ("dash", [0.036, 0.036, 0.039], 0.0, 0.75, [0.0; 3]),
    ("seat", [0.045, 0.04, 0.038], 0.0, 0.6, [0.0; 3]),
    ("leather", [0.03, 0.03, 0.032], 0.0, 0.5, [0.0; 3]),
    ("glove", [0.035, 0.035, 0.04], 0.0, 0.7, [0.0; 3]),
    ("glove_band", [0.42, 0.03, 0.025], 0.0, 0.6, [0.0; 3]),
    ("sleeve", [0.16, 0.17, 0.19], 0.0, 0.85, [0.0; 3]),
    ("carpet", [0.012, 0.012, 0.013], 0.0, 1.0, [0.0; 3]),
    ("screen", [0.01, 0.012, 0.016], 0.0, 0.1, [0.2, 0.5, 1.0]),
    ("dial", [0.01, 0.01, 0.012], 0.0, 0.4, [1.0, 1.0, 1.0]),
    ("marker", [0.9, 0.55, 0.03], 0.0, 0.5, [0.0; 3]),
];

/// Triangles in design axes (z up) as a model's vertices in body axes (z down), about `origin`
/// (design axes).
fn vertices(triangles: &[Triangle], origin: Vec3) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(triangles.len() * 3);
    for tri in triangles {
        let mut v = tri.map(|c| Vertex {
            position: Vec3::new(c.p.x - origin.x, c.p.y - origin.y, -(c.p.z - origin.z)),
            normal: Vec3::new(c.n.x, c.n.y, -c.n.z),
            color: Vec4::new(c.open, c.open, c.open, 1.0),
            uv: Vec2::ZERO,
        });
        // Turning z over turns the triangle's face over: wound again to look the way its normals do.
        let face = (v[1].position - v[0].position).cross(v[2].position - v[0].position);
        if face.dot(v[0].normal + v[1].normal + v[2].normal) < 0.0 {
            v.swap(1, 2);
        }
        out.extend(v);
    }
    out
}

/// Triangles made about a centre, moved to where it is.
fn placed(parts: Vec<(&'static str, Vec<Triangle>)>, at: Vec3) -> Vec<(&'static str, Vec<Triangle>)> {
    parts.into_iter().map(|(m, t)| (m, t.into_iter().map(|tri| tri.map(|c| Corner { p: c.p + at, ..c })).collect())).collect()
}

/// Adds a node of these parts. `origin` (design axes, z up from the ground) is where the node's
/// own axes start; `None` for the model's (the centre of gravity).
fn node(w: &mut Writer, name: &str, origin: Option<Vec3>, parts: &[(&'static str, Vec<Triangle>)]) {
    let ground = Vec3::new(0.0, 0.0, GROUND);
    let about = origin.unwrap_or(ground);
    let mut count = 0;
    let mut primitives: Vec<(usize, Vec<Vertex>, bool)> = Vec::new();
    for (material, triangles) in parts {
        let &(m, base, metallic, roughness, emissive) = MATERIALS.iter().find(|m| m.0 == *material).unwrap_or_else(|| panic!("no material {material}"));
        let index = w.material(WrittenMaterial { name: m.into(), base: Vec3::from(base).extend(1.0), metallic, roughness, emissive: Vec3::from(emissive) });
        count += triangles.len();
        match primitives.iter_mut().find(|p| p.0 == index) {
            Some(p) => p.1.extend(vertices(triangles, about)),
            None => primitives.push((index, vertices(triangles, about), false)),
        }
    }
    // The node's place in body axes.
    let place = origin.map_or(Vec3::ZERO, |o| Vec3::new(o.x, o.y, GROUND - o.z));
    w.node(name, None, place, &primitives);
    println!("{name}: {count} triangles");
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "assets/cars/gt/exterior.glb".to_string());
    // The grid the shell is sampled on, metres: finer is smoother and heavier.
    let cell = std::env::args().nth(2).and_then(|c| c.parse::<f32>().ok()).unwrap_or(0.018);
    let mut w = Writer::new();
    let shell = body::Body::new().build(cell);
    let (glass, solid): (Vec<_>, Vec<_>) = shell.into_iter().partition(|p| p.0 == "glass");
    node(&mut w, "body", None, &solid);
    node(&mut w, "glass", None, &glass);
    node(&mut w, "cabin", None, &cabin::cabin());
    node(&mut w, "steering", Some(cabin::STEERING), &placed(cabin::steering(), cabin::STEERING));
    for (name, at) in [("dial_rev", cabin::DIAL_REV), ("dial_speed", cabin::DIAL_SPEED)] {
        node(&mut w, name, Some(at), &placed(vec![("dial", cabin::dial())], at));
    }
    node(&mut w, "hand_left", Some(cabin::STEERING), &placed(cabin::hand(-1.0), cabin::STEERING));
    node(&mut w, "hand_right", Some(cabin::STEERING), &placed(cabin::hand(1.0), cabin::STEERING));
    node(&mut w, "arm", Some(Vec3::ZERO), &cabin::forearm());
    // The wheels and brakes are about their hubs: placed by the game on each axle's end. 245 mm
    // tyres in front on 370 mm discs, 285 mm behind on 350 mm; the calipers behind the front
    // axle and ahead of the rear one.
    let hub = Some(Vec3::ZERO);
    node(&mut w, "wheel_front", hub, &wheel::wheel(0.245));
    node(&mut w, "wheel_rear", hub, &wheel::wheel(0.285));
    node(&mut w, "brake_front", hub, &wheel::brake(0.245, 0.185, 2.7));
    node(&mut w, "brake_rear", hub, &wheel::brake(0.285, 0.175, 0.44));
    let bytes = w.glb();
    std::fs::write(&out, &bytes).unwrap_or_else(|e| panic!("{out}: {e}"));
    println!("wrote {out}: {:.1} MB", bytes.len() as f64 / 1048576.0);
}
