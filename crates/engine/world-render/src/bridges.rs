//! The bridges of a real place (`sim_core::Bridge`), drawn into the tiles of the buildings of the
//! maps (`footprints.rs`): each part `sim_core::bridges::parts` makes (the boxes the simulation runs
//! into: deck, piers, towers, girders; the bars of cables, stays and hangers) whose middle is in the
//! tile, in the bridge's colour or concrete, the deck's top asphalt, with a parapet or a rail along
//! its edges (only drawn: nothing to run into that the deck does not already have). Bars thinner than
//! a few pixels at a hundred metres are drawn thicker than they are, so that they do not flicker
//! away (a hanger of the Golden Gate is 7 cm across; drawn, 30 cm).

use crate::footprints::{normal, MeshData, STYLE_PLAIN, SURFACE_DECK, SURFACE_PLAIN, SURFACE_STEEL};
use sim_core::bridges::{self, PartKind, Piece};
use sim_core::{Bridge, Structure, Terrain};

/// The parts of the region's bridges whose middle is in the tile `key` (east, south) of `size`.
pub(crate) fn tile(mesh: &mut MeshData, terrain: &Terrain, key: (i32, i32), size: f64, far: bool) {
    let Some(region) = terrain.region() else { return };
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    // The tile in the world's axes: north from -(oz + size) to -oz, east from ox to ox + size.
    let (south, north, west, east) = (-(oz + size), -oz, ox, ox + size);
    let ground = |n: f64, e: f64| region.height(n, e);
    for b in &region.bridges {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &b.line {
            lo = [lo[0].min(p[0] as f64), lo[1].min(p[1] as f64)];
            hi = [hi[0].max(p[0] as f64), hi[1].max(p[1] as f64)];
        }
        let pad = b.width as f64 + 40.0;
        if hi[0] + pad < south || lo[0] - pad > north || hi[1] + pad < west || lo[1] - pad > east {
            continue;
        }
        let steel = b.colour.map(albedo).unwrap_or(match b.structure {
            Structure::Beam => [0.50, 0.49, 0.46],
            Structure::Truss | Structure::Arch => [0.30, 0.33, 0.32],
            _ => [0.55, 0.56, 0.56],
        });
        let concrete = [0.50, 0.49, 0.46];
        for part in bridges::parts(b, &ground) {
            let bounds = part.piece.bounds();
            let (n, e) = ((bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0);
            if n < south || n >= north || e < west || e >= east {
                continue;
            }
            let colour = match part.kind {
                PartKind::Pier if b.structure != Structure::Beam || b.colour.is_none() => concrete,
                _ => steel,
            };
            match part.piece {
                Piece::Prism { ring, .. } => {
                    let deck = part.kind == PartKind::Deck;
                    prism(mesh, &part.piece, &ring, colour, if deck { SURFACE_DECK } else if b.colour.is_some() { SURFACE_STEEL } else { SURFACE_PLAIN });
                    if deck && !far {
                        edges(mesh, b, &part.piece, &ring, steel);
                    }
                }
                Piece::Bar { a, b: c, radius } => {
                    if far && part.kind == PartKind::Hanger {
                        continue;
                    }
                    let shown = match part.kind {
                        PartKind::Hanger => radius.max(0.25),
                        // A main cable stands out against the sky from kilometres away.
                        PartKind::Cable if radius > 0.3 => radius * 2.2,
                        PartKind::Cable => radius.max(0.2),
                        _ => radius,
                    };
                    bar(mesh, a, c, shown, colour);
                }
            }
        }
    }
}

/// A colour of the maps (sRGB) as the albedo the shader lights.
fn albedo(c: [u8; 3]) -> [f32; 3] {
    // Paint in the open is a little less saturated than its swatch: a quarter of the way to grey.
    let lin = c.map(|v| (v as f32 / 255.0).powf(2.2));
    let grey = 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
    lin.map(|v| (v * 0.75 + grey * 0.25).clamp(0.03, 0.8))
}

/// A box of a part: its sides, its top (the road, on a deck) and its underside.
fn prism(mesh: &mut MeshData, piece: &Piece, ring: &[[f32; 2]; 4], colour: [f32; 3], top_surface: f32) {
    let corner = |i: usize| [ring[i][0] as f64, ring[i][1] as f64];
    let heights: Vec<(f64, f64)> = (0..4).map(|i| piece.heights(corner(i)[0], corner(i)[1])).collect();
    let low: Vec<[f32; 3]> = (0..4).map(|i| mesh.at(corner(i), heights[i].0 as f32)).collect();
    let high: Vec<[f32; 3]> = (0..4).map(|i| mesh.at(corner(i), heights[i].1 as f32)).collect();
    let side_surface = if top_surface == SURFACE_DECK { SURFACE_PLAIN } else { top_surface };
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad(mesh, [low[i], low[j], high[j], high[i]], colour, side_surface);
    }
    quad(mesh, [high[0], high[1], high[2], high[3]], if top_surface == SURFACE_DECK { [0.11, 0.11, 0.12] } else { colour }, top_surface);
    quad(mesh, [low[0], low[1], low[2], low[3]], colour.map(|c| c * 0.8), side_surface);
}

/// A flat four-cornered face, its normal from its corners (the shader turns it to the eye).
fn quad(mesh: &mut MeshData, p: [[f32; 3]; 4], colour: [f32; 3], surface: f32) {
    let (u, v) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]]);
    let mut n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if length < 1e-6 {
        n = normal(p[0], p[1], p[2]);
    } else {
        n = n.map(|x| x / length);
    }
    let v: Vec<u32> = p.iter().map(|&q| mesh.vertex(q, n, colour, surface, 0.0, 0.0, [STYLE_PLAIN, 3.0, 0.0, 0.0])).collect();
    mesh.indices.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
}

/// A round bar drawn as a square one, `radius` from its axis.
fn bar(mesh: &mut MeshData, a: [f32; 3], b: [f32; 3], radius: f32, colour: [f32; 3]) {
    let (pa, pb) = (mesh.at([a[0] as f64, a[1] as f64], a[2]), mesh.at([b[0] as f64, b[1] as f64], b[2]));
    let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length < 1e-4 {
        return;
    }
    let d = d.map(|x| x / length);
    // Two directions across the bar.
    let helper = if d[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let cross = |x: [f32; 3], y: [f32; 3]| [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let unit = |x: [f32; 3]| {
        let l = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt().max(1e-6);
        x.map(|v| v / l)
    };
    let s = unit(cross(d, helper));
    let t = cross(d, s);
    let off = |p: [f32; 3], k: usize| {
        let (cs, ct) = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)][k];
        [p[0] + (s[0] * cs + t[0] * ct) * radius, p[1] + (s[1] * cs + t[1] * ct) * radius, p[2] + (s[2] * cs + t[2] * ct) * radius]
    };
    for k in 0..4 {
        let j = (k + 1) % 4;
        quad(mesh, [off(pa, k), off(pa, j), off(pb, j), off(pb, k)], colour, SURFACE_STEEL);
    }
}

/// A parapet (a concrete bridge) or a rail (a steel one) along both edges of a box of deck.
fn edges(mesh: &mut MeshData, b: &Bridge, piece: &Piece, ring: &[[f32; 2]; 4], colour: [f32; 3]) {
    let rail = b.structure != Structure::Beam || b.colour.is_some();
    let height = if rail { 1.3 } else { 0.9 };
    // The long sides of the box: corners 0-1 and 3-2 (`bridges::slab`).
    for (i, j) in [(0, 1), (3, 2)] {
        let (p, q) = ([ring[i][0] as f64, ring[i][1] as f64], [ring[j][0] as f64, ring[j][1] as f64]);
        let (hp, hq) = (piece.heights(p[0], p[1]).1 as f32, piece.heights(q[0], q[1]).1 as f32);
        let a = mesh.at(p, hp);
        let c = mesh.at(q, hq);
        let lift = |x: [f32; 3], up: f32| [x[0], x[1] + up, x[2]];
        let col = if rail { colour } else { [0.52, 0.51, 0.48] };
        quad(mesh, [a, c, lift(c, height), lift(a, height)], col, SURFACE_PLAIN);
    }
}
