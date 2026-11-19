//! The small models of the aerodromes, built on the CPU: the windsock and its pole, and the square
//! every light, flame and puff of smoke is drawn on.

use engine::MeshBuilder;
use glam::{Vec3, Vec4};
use std::f32::consts::TAU;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A pole 6 m tall, y up, base at the origin.
pub fn pole() -> MeshBuilder {
    let mut b = MeshBuilder::new();
    b.cylinder(v(0.0, 0.0, 0.0), v(0.0, 6.0, 0.0), 0.07, 0.07, 6, Vec4::new(0.6, 0.6, 0.6, 0.5));
    b
}

/// Windsock pointing along +x from the origin, 3.6 m long.
pub fn windsock() -> MeshBuilder {
    let mut b = MeshBuilder::new();
    let orange = Vec4::new(0.9, 0.25, 0.02, 0.0);
    let white = Vec4::new(0.85, 0.85, 0.85, 0.0);
    for i in 0..4 {
        let x0 = i as f32 * 0.9;
        let x1 = x0 + 0.9;
        let r0 = 0.45 - i as f32 * 0.075;
        let r1 = r0 - 0.075;
        let circle = |x: f32, r: f32| -> Vec<Vec3> {
            (0..8)
                .map(|k| {
                    let t = k as f32 / 8.0 * TAU;
                    v(x, t.cos() * r, t.sin() * r)
                })
                .collect()
        };
        let rings = [circle(x0, r0), circle(x1, r1)];
        let c = if i % 2 == 0 { orange } else { white };
        // Outside, then a darker inside.
        b.loft(&rings, |_, _| c, false, false, false);
        b.loft(&rings, |_, _| c * Vec4::new(0.6, 0.6, 0.6, 1.0), false, false, true);
    }
    b
}

/// A square facing +z, one metre from centre to edge, for lights.
pub fn light() -> MeshBuilder {
    let mut b = MeshBuilder::new();
    b.quad(v(-1.0, -1.0, 0.0), v(1.0, -1.0, 0.0), v(1.0, 1.0, 0.0), v(-1.0, 1.0, 0.0), Vec4::new(1.0, 1.0, 1.0, 0.0));
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenery_is_not_empty() {
        for (name, mesh) in [
            ("pole", pole()),
            ("windsock", windsock()), ("light", light()),
        ] {
            assert!(mesh.count() > 0 && mesh.count() % 3 == 0, "{name}");
            assert!(mesh.data.iter().all(|x| x.is_finite()), "{name} has a non-finite value");
        }
    }
}
