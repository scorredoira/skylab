//! Makes a car's `exterior.glb` from a model made elsewhere (in Blender, downloaded), as the
//! game draws it (`src/look.rs`): `import_car assets/cars/<key>/import.ron`.
//!
//! The import file says where the model is, which way it faces, its materials as the game's
//! (`paint`, `glass`, `rubber`, `lamp_head`, ...; any other name is drawn plain in its own colour)
//! and which of its materials make the wheels. The program:
//!
//! - turns the model into the car's body axes (x forward, y right, z down), its origin at the
//!   centre of gravity: between the four wheels, as far from each axle as the weight on the other
//!   (`front_share` on the front wheels), `cg_height` over the ground (the bottom of the tyres);
//! - finds the four wheels by their tyres' material, and their hubs, radius and width; the right
//!   ones (whatever of the model lies within their round) become `wheel_front` and `wheel_rear`,
//!   about their hubs, which the game turns, steers and mirrors for the left side; the left ones
//!   are left out;
//! - puts the glass in `glass`, the rest in `body`; writes the nodes it has nothing for (the
//!   dials, the driver's hands) empty;
//! - prints where the axles are, their track and the tyres' radius, for the car's `car.ron`.

use std::collections::HashMap;

use engine::gltf::{Vertex, Writer, WrittenMaterial};
use glam::{Vec2, Vec3, Vec4};
use serde::Deserialize;

#[derive(Deserialize)]
struct Import {
    /// The model, beside the import file.
    source: String,
    /// The model's axes that point forward and up, as "+x", "-z" ...
    forward: String,
    up: String,
    /// Metres per unit of the model.
    scale: f32,
    /// Height of the centre of gravity, m, and the share of the weight on the front wheels.
    cg_height: f32,
    front_share: f32,
    /// The model's materials as the game's: (theirs, ours). Unlisted ones keep their names.
    materials: Vec<(String, String)>,
    /// The materials of the tyres: the wheels are found by them.
    tyres: Vec<String>,
    /// Others of the wheels (rims, nuts, discs): with the tyres, what turns.
    wheels: Vec<String>,
    /// A colour to give a material in place of its own (linear), and its gloss: (ours, (r, g, b), metallic, roughness).
    #[serde(default)]
    looks: Vec<(String, [f32; 3], f32, f32)>,
    /// A box of the body's axes (lower and upper corners, after the move to the centre of gravity)
    /// that is the cabin: there the materials listed become the one named (the shell's material
    /// that also lines the inside, as the dashboard's).
    #[serde(default)]
    cabin: Option<Cabin>,
}

/// A point of the body's axes, as the import file gives it.
type Point = (f32, f32, f32);
/// The cabin's box, the materials in it to change, and to which.
type Cabin = (Point, Point, Vec<String>, String);

fn axis(s: &str) -> Vec3 {
    let v = match &s[1..] {
        "x" => Vec3::X,
        "y" => Vec3::Y,
        "z" => Vec3::Z,
        _ => panic!("an axis is +x, -y ...: {s}"),
    };
    if s.starts_with('-') { -v } else { v }
}

/// A triangle of the model in body axes, with its material (ours) and the source's.
struct Tri {
    v: [Vertex; 3],
    material: String,
    source: String,
}

fn main() {
    let file = std::env::args().nth(1).expect("import_car <import.ron>");
    let dir = std::path::Path::new(&file).parent().unwrap().to_path_buf();
    let import: Import = ron::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let model = engine::gltf::read(&std::fs::read(dir.join(&import.source)).unwrap()).unwrap();
    let ours: HashMap<&str, &str> = import.materials.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();

    // The model's axes to the body's, before the origin is known.
    let fwd = axis(&import.forward);
    let up = axis(&import.up);
    let right = fwd.cross(up);
    let turn = |p: Vec3| Vec3::new(p.dot(fwd), p.dot(right), -p.dot(up)) * import.scale;
    let turn_n = |n: Vec3| Vec3::new(n.dot(fwd), n.dot(right), -n.dot(up)).normalize_or_zero();

    // Each material's colour: its own, times its texture's mean where it has one.
    let mean_texture: Vec<Vec4> = model
        .materials
        .iter()
        .map(|m| match m.texture.and_then(|t| model.images.get(t)) {
            Some(image) => {
                let n = (image.rgba.len() / 4).max(1);
                let step = (n / 4096).max(1);
                let mut sum = Vec4::ZERO;
                let mut count = 0.0;
                for k in (0..n).step_by(step) {
                    let p = &image.rgba[k * 4..k * 4 + 4];
                    let lin = |c: u8| {
                        let c = c as f32 / 255.0;
                        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
                    };
                    sum += Vec4::new(lin(p[0]), lin(p[1]), lin(p[2]), 1.0);
                    count += 1.0;
                }
                sum / count
            }
            None => Vec4::ONE,
        })
        .collect();

    let mut tris: Vec<Tri> = Vec::new();
    let mut colours: HashMap<String, (Vec4, f32, f32, Vec3)> = HashMap::new();
    for node in &model.nodes {
        for (k, t) in node.vertices.chunks(3).enumerate() {
            let m = node.materials[k];
            let mat = model.materials.get(m).cloned().unwrap_or_default();
            let source = mat.name.clone();
            let material = ours.get(source.as_str()).map_or(source.clone(), |s| s.to_string());
            colours.entry(material.clone()).or_insert((mat.base * mean_texture.get(m).copied().unwrap_or(Vec4::ONE), mat.metallic, mat.roughness, mat.emissive));
            let v = [0, 1, 2].map(|i| Vertex { position: turn(t[i].position), normal: turn_n(t[i].normal), color: Vec4::ONE, uv: Vec2::ZERO });
            tris.push(Tri { v, material, source });
        }
    }

    // The wheels: the tyres' triangles in four groups, by end and side.
    let centre = |t: &Tri| (t.v[0].position + t.v[1].position + t.v[2].position) / 3.0;
    let tyres: Vec<&Tri> = tris.iter().filter(|t| import.tyres.contains(&t.source)).collect();
    let (lo, hi) = tyres.iter().flat_map(|t| t.v.iter()).fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(l, h), v| (l.min(v.position), h.max(v.position)));
    let mid = (lo + hi) / 2.0;
    let mut wheels = [(Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)); 4];
    for t in &tyres {
        let c = centre(t);
        let i = usize::from(c.x < mid.x) * 2 + usize::from(c.y > mid.y);
        for v in &t.v {
            wheels[i].0 = wheels[i].0.min(v.position);
            wheels[i].1 = wheels[i].1.max(v.position);
        }
    }
    let hubs: [Vec3; 4] = wheels.map(|(l, h)| (l + h) / 2.0);
    let radius = wheels.iter().map(|(l, h)| (h.x - l.x).max(h.z - l.z) / 2.0).sum::<f32>() / 4.0;
    let width = wheels.iter().map(|(l, h)| h.y - l.y).sum::<f32>() / 4.0;
    let ground = wheels.iter().map(|(_, h)| h.z).fold(f32::MIN, f32::max);
    let front_x = (hubs[0].x + hubs[1].x) / 2.0;
    let rear_x = (hubs[2].x + hubs[3].x) / 2.0;
    let wheelbase = front_x - rear_x;
    // The centre of gravity: between the wheels, by the weight on each axle, `cg_height` up.
    let cg = Vec3::new(front_x - (1.0 - import.front_share) * wheelbase, (hubs[0].y + hubs[1].y + hubs[2].y + hubs[3].y) / 4.0, ground - import.cg_height);
    for t in &mut tris {
        for v in &mut t.v {
            v.position -= cg;
        }
    }
    let hubs = hubs.map(|h| h - cg);
    if let Some((lo, hi, from, to)) = &import.cabin {
        let (lo, hi) = (Vec3::new(lo.0, lo.1, lo.2), Vec3::new(hi.0, hi.1, hi.2));
        for t in &mut tris {
            let c = centre(t);
            if from.contains(&t.source) && c.cmpge(lo).all() && c.cmple(hi).all() {
                t.material = to.clone();
            }
        }
    }
    println!("front axle x {:.3}, rear axle x {:.3} (wheelbase {:.3}); tracks {:.3}, {:.3}; tyre radius {:.3}, width {:.3}", hubs[0].x, hubs[2].x, wheelbase, hubs[1].y - hubs[0].y, hubs[3].y - hubs[2].y, radius, width);
    println!("hubs at z {:.3} (front) {:.3} (rear); the ground at z {:.3}", hubs[0].z, hubs[2].z, import.cg_height);

    // What lies within each right wheel's round goes with it, about its hub; the left wheels' is left out.
    let in_wheel = |c: Vec3, h: Vec3| (c.x - h.x).hypot(c.z - h.z) < radius + 0.01 && (c.y - h.y).abs() < width * 0.5 + 0.12;
    let mut parts: HashMap<&str, Vec<(String, [Vertex; 3])>> = HashMap::new();
    for t in tris {
        let c = centre(&t);
        let wheel_part = import.tyres.contains(&t.source) || import.wheels.contains(&t.source);
        let which = (0..4).find(|&i| in_wheel(c, hubs[i]));
        let part = match which {
            Some(i) if wheel_part => {
                if i % 2 == 0 {
                    continue;
                }
                let hub = hubs[i];
                let mut v = t.v;
                for x in &mut v {
                    x.position -= hub;
                }
                parts.entry(if i < 2 { "wheel_front" } else { "wheel_rear" }).or_default().push((t.material, v));
                continue;
            }
            _ if t.material == "glass" => "glass",
            _ => "body",
        };
        parts.entry(part).or_default().push((t.material, t.v));
    }

    let mut w = Writer::new();
    let looks: HashMap<&str, ([f32; 3], f32, f32)> = import.looks.iter().map(|(n, c, m, r)| (n.as_str(), (*c, *m, *r))).collect();
    for name in ["body", "glass", "cabin", "steering", "dial_rev", "dial_speed", "wheel_front", "wheel_rear", "brake_front", "brake_rear", "hand_left", "hand_right", "arm"] {
        let mut primitives: Vec<(usize, Vec<Vertex>, bool)> = Vec::new();
        let triangles = parts.remove(name).unwrap_or_default();
        let count = triangles.len();
        for (material, v) in triangles {
            let (base, metallic, roughness, emissive) = colours.get(&material).copied().unwrap_or((Vec4::new(0.03, 0.03, 0.03, 1.0), 0.0, 0.8, Vec3::ZERO));
            let (base, metallic, roughness) = match looks.get(material.as_str()) {
                Some((c, m, r)) => (Vec3::from(*c).extend(1.0), *m, *r),
                None => (base, metallic, roughness),
            };
            let emissive = match material.as_str() {
                "lamp_head" | "lamp_ring" => Vec3::new(1.0, 0.96, 0.9),
                "lamp_tail" | "lamp_brake" | "lamp_lens" => Vec3::new(1.0, 0.04, 0.02),
                _ => emissive,
            };
            let index = w.material(WrittenMaterial { name: material.clone(), base, metallic, roughness, emissive });
            match primitives.iter_mut().find(|p| p.0 == index) {
                Some(p) => p.1.extend(v),
                None => primitives.push((index, v.to_vec(), false)),
            }
        }
        // A node with nothing in it gets one point-sized triangle, so that every part has a mesh.
        if primitives.is_empty() {
            let index = w.material(WrittenMaterial { name: "trim".into(), base: Vec4::new(0.01, 0.01, 0.01, 1.0), metallic: 0.0, roughness: 1.0, emissive: Vec3::ZERO });
            let v = Vertex { position: Vec3::ZERO, normal: Vec3::Z, color: Vec4::ONE, uv: Vec2::ZERO };
            primitives.push((index, vec![v; 3], false));
        }
        w.node(name, None, Vec3::ZERO, &primitives);
        println!("{name}: {count} triangles");
    }
    let out = dir.join("exterior.glb");
    let bytes = w.glb();
    std::fs::write(&out, &bytes).unwrap();
    println!("wrote {}: {:.1} MB", out.display(), bytes.len() as f64 / 1048576.0);
}
