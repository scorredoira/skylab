//! Race circuits drawn (`sim_core::circuit`): each circuit of the terrain as one mesh of its own,
//! made from the same samples the simulation drives on, so what is seen is what the tyres touch.
//!
//! Across each sample of the centre line, from the middle out on either side: the asphalt (its
//! cross slope, the painted edge lines, the start's chequered line and grid drawn by the shader,
//! `track.wgsl`), the kerb (raised as the simulation has it, red and white), the run-off (grass,
//! gravel or asphalt), the barrier (armco on posts, a belted tyre wall, a concrete wall), and the
//! verge or the pit lane behind it to the end of the circuit's own ground, where the terrain (laid
//! a little lower under the circuit, `Terrain::with_circuits`) takes over at the same height. Then
//! what the circuit's file places beside it (`Prop`): the pits' building, the start's gantry,
//! grandstands, marshals' posts, a footbridge, the floodlights' towers. One draw for all of it,
//! with the world's light, haze and shadows (the relief's, the clouds', the car's).
//!
//! By night the towers light it: what each vertex gets from all of them is worked out when the
//! mesh is made (`Vertex::light`), and the towers nearest the eye reach the shader as they are,
//! for their glints on a wet road. The car the view follows lights it too, by what the game says
//! (`TrackRenderer::beams`, `tail`): its dipped beams, wide and cut off under the horizontal, and
//! the red of its tail lamps on the ground behind it.
//!
//! Wet, the paved ground mirrors the sky in the first pass; what stands on it (the car, its
//! lamps, the barriers, the towers and their lamps, the trees) it mirrors in a second pass over
//! the scene, out of the scene itself (`draw_wet`: rays followed across the picture, which the
//! engine keeps for it, `Gpu::scene_kept`).
//!
//! The game makes a `TrackRenderer` for the terrain it draws and asks it to `prepare` and `draw`
//! every frame, between `WorldRenderer::draw_sky` and `draw`; and, when `mirrors` says so, makes
//! a second pass of the frame (`Game::translucent`, `reads_scene`) and calls `before_wet` and
//! `draw_wet` in it.

use std::sync::Arc;

use engine::pipeline::{Depth, PipelineDesc};
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Gpu, UniformArena};
use glam::{DVec3, Mat4};
use sim_core::circuit::{self, BarrierKind, Circuit, PropKind, Runoff, Side};
use sim_core::Terrain;

use crate::{shader_source, to_render, ObjectUniforms, Quality, WorldRenderer};

/// What a vertex is: `TRACK_*` in `track.wgsl`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Prop = 0,
    Asphalt = 1,
    Kerb = 2,
    Grass = 3,
    Gravel = 4,
    Concrete = 5,
    Runoff = 6,
    Belt = 7,
    /// A row of spectators: a card standing on a step of a stand, people drawn on it.
    Crowd = 8,
    /// An advertising board: its colour and a name lettered on it.
    Board = 9,
    /// A floodlight's lamp: it shines by night.
    Lamp = 10,
}

/// A vertex of a circuit's mesh: `TrackIn` in `track.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Render axes (east, up, south), metres from the mesh's origin.
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Albedo and gloss (half the asphalt's width, on the asphalt).
    pub color: [f32; 4],
    /// Kind, metres across and along the circuit, how open it is to the sky.
    pub surface: [f32; 4],
    /// The floodlights' light there by full night (on level ground), and nothing.
    pub light: [f32; 2],
}

/// A circuit's mesh as built: its vertices and triangles, and where its origin is (north, east, height).
#[derive(Clone, Debug, Default)]
pub struct TrackMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub origin: DVec3,
}

/// The albedos of the circuit's surfaces (linear), the average colour the photographs multiply.
const ASPHALT: [f32; 3] = [0.075, 0.075, 0.08];
const RUNOFF_ASPHALT: [f32; 3] = [0.085, 0.085, 0.088];
const GRASS: [f32; 3] = [0.062, 0.12, 0.035];
const GRAVEL: [f32; 3] = [0.34, 0.30, 0.24];
const CONCRETE: [f32; 3] = [0.38, 0.37, 0.35];
const KERB: [f32; 3] = [0.5, 0.5, 0.5];
/// Galvanised steel, painted steel, rubber, concrete of walls.
const STEEL: [f32; 4] = [0.45, 0.46, 0.47, 0.75];
const RUBBER: [f32; 4] = [0.025, 0.025, 0.027, 0.15];
const WALL: [f32; 4] = [0.52, 0.51, 0.48, 0.1];

/// Builds a mesh, a vertex at a time; positions given in the world (north, east, height).
struct Builder {
    mesh: TrackMesh,
    /// The floodlights, which light every vertex.
    towers: Vec<Tower>,
}

impl Builder {
    fn new(origin: DVec3, towers: Vec<Tower>) -> Builder {
        Builder { mesh: TrackMesh { origin, ..TrackMesh::default() }, towers }
    }

    /// The position in the mesh of a world point (north, east, height).
    fn local(&self, p: DVec3) -> [f32; 3] {
        let o = self.mesh.origin;
        [(p.y - o.y) as f32, (p.z - o.z) as f32, -(p.x - o.x) as f32]
    }

    /// Adds a vertex; `normal` in the world's axes (north, east, up).
    fn vertex(&mut self, p: DVec3, normal: DVec3, color: [f32; 4], surface: [f32; 4]) -> u32 {
        let n = normal.normalize_or_zero();
        let light = [floodlight(&self.towers, p), 0.0];
        self.mesh.vertices.push(Vertex { position: self.local(p), normal: [n.y as f32, n.z as f32, -n.x as f32], color, surface, light });
        (self.mesh.vertices.len() - 1) as u32
    }

    /// Two triangles over four vertices in order round them (either way: faces are drawn both sides).
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.mesh.indices.extend_from_slice(&[a, b, c, a, c, d]);
    }

    /// A flat-shaded quad of a prop, its four corners in order.
    fn face(&mut self, corners: [DVec3; 4], color: [f32; 4], kind: Kind, along: f64) {
        let normal = (corners[1] - corners[0]).cross(corners[3] - corners[0]);
        let normal = if normal.length_squared() > 1e-12 { normal } else { (corners[2] - corners[0]).cross(corners[3] - corners[1]) };
        let s = [kind as u8 as f32, 0.0, along as f32, 1.0];
        let v: Vec<u32> = corners.iter().map(|&c| self.vertex(c, normal, color, s)).collect();
        self.quad(v[0], v[1], v[2], v[3]);
    }

    /// A flat quad with coordinates of its own over it (`surface.y`, `surface.z`: for what the
    /// shader draws on it) and a number for its kind in `surface.w`; its four corners in order.
    fn panel(&mut self, corners: [DVec3; 4], color: [f32; 4], kind: Kind, uv: [[f32; 2]; 4], number: f32) {
        let normal = (corners[1] - corners[0]).cross(corners[3] - corners[0]);
        let v: Vec<u32> = corners.iter().zip(uv).map(|(&c, uv)| self.vertex(c, normal, color, [kind as u8 as f32, uv[0], uv[1], number])).collect();
        self.quad(v[0], v[1], v[2], v[3]);
    }

    /// A box between `lo` and `hi` in a frame (origin, along, right, up: world axes), all six faces.
    fn cuboid(&mut self, f: &Frame, lo: DVec3, hi: DVec3, color: [f32; 4], along: f64) {
        let c = |x: f64, y: f64, z: f64| f.at(x, y, z);
        let (a, b) = (lo, hi);
        let p = [c(a.x, a.y, a.z), c(b.x, a.y, a.z), c(b.x, b.y, a.z), c(a.x, b.y, a.z), c(a.x, a.y, b.z), c(b.x, a.y, b.z), c(b.x, b.y, b.z), c(a.x, b.y, b.z)];
        // Outward winding with x along, y right, z up (a left-handed frame on the ground seen from above).
        for [i, j, k, l] in [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [3, 0, 4, 7]] {
            self.face([p[i], p[j], p[k], p[l]], color, Kind::Prop, along);
        }
    }
}

/// Axes at a point beside the circuit: along it, to its right, up.
#[derive(Clone, Copy, Debug)]
struct Frame {
    origin: DVec3,
    along: DVec3,
    right: DVec3,
}

impl Frame {
    /// At `along` metres round, `across` to the right of the centre line, on the circuit's plane there.
    fn of(c: &Circuit, along: f64, across: f64) -> Frame {
        let (n, e, h) = c.point(along, across);
        let heading = c.at(along).3;
        Frame { origin: DVec3::new(n, e, h), along: DVec3::new(heading.cos(), heading.sin(), 0.0), right: DVec3::new(-heading.sin(), heading.cos(), 0.0) }
    }

    fn at(&self, x: f64, y: f64, z: f64) -> DVec3 {
        self.origin + self.along * x + self.right * y + DVec3::Z * z
    }
}

/// One band across the circuit, from the middle out: its inner and outer distance, its kind and
/// colour, and how open to the sky its outer edge is (the foot of a wall is darker).
struct Band {
    from: f64,
    to: f64,
    kind: Kind,
    color: [f32; 4],
    /// Extra points across (shares of its width) and their height over the plane, for a kerb.
    profile: &'static [(f64, f64)],
    open: (f32, f32),
}

/// The kerb's shape across, as `Circuit::surface_at` has it: shares of its width, metres up.
const KERB_PROFILE: &[(f64, f64)] = &[(0.0, 0.0), (0.12, 0.012), (0.3, 0.036), (0.85, 0.036), (1.0, 0.0)];
const FLAT: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0)];

/// The bands of one side of the circuit at a sample.
fn bands(s: &circuit::Sample, side: usize) -> Vec<Band> {
    let ss = &s.sides[side];
    let edge = s.width / 2.0;
    let kerb_end = edge + if ss.kerb { circuit::KERB_WIDTH } else { 0.0 };
    let barrier = ss.barrier_at(s.width).max(kerb_end);
    let wall = ss.barrier != BarrierKind::None;
    let foot = if wall { 0.6 } else { 1.0 };
    let (runoff_kind, runoff_color) = match ss.runoff {
        Runoff::Grass => (Kind::Grass, GRASS),
        Runoff::Gravel => (Kind::Gravel, GRAVEL),
        Runoff::Asphalt => (Kind::Runoff, RUNOFF_ASPHALT),
    };
    let rgb = |c: [f32; 3], a: f32| [c[0], c[1], c[2], a];
    let mut out = vec![
        Band { from: 0.0, to: edge, kind: Kind::Asphalt, color: rgb(ASPHALT, edge as f32), profile: FLAT, open: (1.0, 1.0) },
        Band { from: edge, to: kerb_end, kind: Kind::Kerb, color: rgb(KERB, 0.0), profile: KERB_PROFILE, open: (1.0, 1.0) },
        // A run-off's colour carries where it begins (its paint is measured from there).
        Band { from: kerb_end, to: barrier, kind: runoff_kind, color: rgb(runoff_color, kerb_end as f32), profile: FLAT, open: (1.0, foot) },
    ];
    if ss.pit {
        out.push(Band { from: barrier, to: barrier + 1.5, kind: Kind::Grass, color: rgb(GRASS, 0.0), profile: FLAT, open: (foot, 1.0) });
        out.push(Band { from: barrier + 1.5, to: ss.reach, kind: Kind::Concrete, color: rgb(CONCRETE, 0.0), profile: FLAT, open: (1.0, 1.0) });
    } else {
        out.push(Band { from: barrier, to: ss.reach.max(barrier + 0.5), kind: Kind::Grass, color: rgb(GRASS, 0.0), profile: FLAT, open: (foot, 1.0) });
    }
    out
}

/// Where the cars run at a sample of a circuit: metres across of their line, and how much
/// rubber they leave there (0 none to 1, the braking zones and the corners).
pub type Line = [f32; 2];

/// How high a floodlight's lamps are over the ground it stands on, and how far apart the towers
/// are along a circuit whose file places none.
pub const TOWER_HEIGHT: f64 = 22.0;
pub const TOWER_SPACING: f64 = 90.0;
/// The light of a tower 20 m from the middle of the track: on level ground `d` metres from its
/// lamps, `h` under them, `TOWER_LIGHT · h / d³` (what multiplies a surface's colour by full night).
pub const TOWER_LIGHT: f64 = 2600.0;

/// A floodlight's tower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tower {
    /// Its lamps: north, east, height above the sea.
    pub lamp: DVec3,
    /// Where it stands: metres along the circuit, and across (left negative).
    pub along: f64,
    pub across: f64,
    /// How much more light it gives than one 20 m from the middle of the track: a tower that
    /// stands far back (behind a gravel trap) has lamps to light the track from there.
    pub power: f64,
}

/// The floodlights of a circuit: where its file places them (`PropKind::Floodlight`), or, when it
/// places none, one every `TOWER_SPACING` metres on one side and the other in turn. Each stands at
/// the end of the circuit's own ground on its side (8 m from the asphalt at least).
pub fn towers(c: &Circuit) -> Vec<Tower> {
    let stand = |along: f64, sign: f64, offset: f64| {
        let s = c.sample(along);
        let out = (s.sides[if sign < 0.0 { 0 } else { 1 }].reach - 1.0).max(s.width / 2.0 + 8.0) + offset;
        let (n, e, h) = c.point(along, sign * out);
        let reach = out.hypot(TOWER_HEIGHT) / 20f64.hypot(TOWER_HEIGHT);
        Tower { lamp: DVec3::new(n, e, h + TOWER_HEIGHT), along, across: sign * out, power: reach.powi(3).clamp(1.0, 5.0) }
    };
    let placed: Vec<Tower> = c.props.iter().filter(|p| p.kind == PropKind::Floodlight).map(|p| stand(p.along.rem_euclid(c.length), if p.side == Side::Left { -1.0 } else { 1.0 }, p.offset)).collect();
    if !placed.is_empty() {
        return placed;
    }
    (0..(c.length / TOWER_SPACING).floor() as i64).map(|k| stand(k as f64 * TOWER_SPACING, if k % 2 == 0 { -1.0 } else { 1.0 }, 0.0)).collect()
}

/// The towers' light on level ground at a point (north, east, height above the sea) by full
/// night: for the circuit's own mesh, and for a game to light its vehicles by.
pub fn floodlight(towers: &[Tower], p: DVec3) -> f32 {
    let mut sum = 0.0;
    for t in towers {
        let d = t.lamp - p;
        let d2 = d.length_squared();
        // Its lamps are turned to the track: its own mast and what is close under them get
        // no more than the ground at its foot.
        if d2 < 250.0 * 250.0 {
            sum += t.power * (d.z.max(4.0) / (d2 * d2.sqrt())).min(1.0 / (TOWER_HEIGHT * TOWER_HEIGHT));
        }
    }
    (sum * TOWER_LIGHT) as f32
}

/// The mesh of a circuit: its ground, barriers and props; the cars' line down its middle.
pub fn mesh(c: &Circuit) -> TrackMesh {
    mesh_with(c, &[])
}

/// The mesh of a circuit with the line the cars run on, a `Line` per sample (none: the middle).
pub fn mesh_with(c: &Circuit, line: &[Line]) -> TrackMesh {
    let first = &c.samples[0];
    let mut b = Builder::new(DVec3::new(first.n, first.e, first.height), towers(c));
    ground(&mut b, c, line);
    barriers(&mut b, c);
    boards(&mut b, c);
    masts(&mut b, c);
    for prop in &c.props {
        match prop.kind {
            PropKind::Pits => pits(&mut b, c, prop),
            PropKind::Gantry => gantry(&mut b, c, prop.along),
            PropKind::Grandstand => grandstand(&mut b, c, prop),
            PropKind::Marshal => marshal(&mut b, c, prop),
            PropKind::Bridge => bridge(&mut b, c, prop.along),
            // The towers are made together (`masts`).
            PropKind::Floodlight => {}
        }
    }
    b.mesh
}

/// The circuit's ground: every band of both sides, sample to sample.
fn ground(b: &mut Builder, c: &Circuit, line: &[Line]) {
    let count = c.samples.len();
    // The vertices of each sample, side by side, band by band: the indices of each band's points.
    let mut rows: Vec<[Vec<Vec<u32>>; 2]> = Vec::with_capacity(count);
    for (i, s) in c.samples.iter().enumerate() {
        let (prev, next) = (&c.samples[(i + count - 1) % count], &c.samples[(i + 1) % count]);
        let grade = (next.height - prev.height) / (2.0 * circuit::STEP);
        let (tn, te) = (s.heading.cos(), s.heading.sin());
        let (rn, re) = (-te, tn);
        let mut sides: [Vec<Vec<u32>>; 2] = [Vec::new(), Vec::new()];
        for (side, list) in sides.iter_mut().enumerate() {
            let sign = if side == 0 { -1.0 } else { 1.0 };
            for mut band in bands(s, side) {
                // The asphalt carries where the cars run instead of a colour (the shader has its own).
                if band.kind == Kind::Asphalt {
                    let l = line.get(i).copied().unwrap_or([0.0, 0.5]);
                    band.color = [l[0], l[1], 0.0, band.color[3]];
                }
                let mut points = Vec::with_capacity(band.profile.len());
                for (k, &(share, rise)) in band.profile.iter().enumerate() {
                    let d = band.from + (band.to - band.from) * share;
                    let across = sign * d;
                    let h = s.height + s.cross * across + rise;
                    // The kerb's slopes lean its normal across.
                    let slope = if band.profile.len() > 2 && k + 1 < band.profile.len() && k > 0 {
                        let (s0, r0) = band.profile[k - 1];
                        let (s1, r1) = band.profile[k + 1];
                        (r1 - r0) / ((s1 - s0) * (band.to - band.from)).max(1e-3) * sign
                    } else {
                        0.0
                    };
                    let cross = s.cross + slope;
                    let up = DVec3::new(-(grade * tn + cross * rn), -(grade * te + cross * re), 1.0);
                    let p = DVec3::new(s.n + rn * across, s.e + re * across, h);
                    let open = if k == 0 { band.open.0 } else if k + 1 == band.profile.len() { band.open.1 } else { 1.0 };
                    points.push(b.vertex(p, up, band.color, [band.kind as u8 as f32, across as f32, s.along as f32, open]));
                }
                list.push(points);
            }
        }
        rows.push(sides);
    }
    // The last sample joins the first, whose `along` starts again at 0: its own copy of the first
    // row at the lap's length, so the coordinates do not jump across the last strip.
    let mut last = rows[0].clone();
    for list in last.iter_mut() {
        for points in list.iter_mut() {
            for v in points.iter_mut() {
                let mut copy = b.mesh.vertices[*v as usize];
                copy.surface[2] = c.length as f32;
                b.mesh.vertices.push(copy);
                *v = (b.mesh.vertices.len() - 1) as u32;
            }
        }
    }
    for i in 0..count {
        let (a, z) = (&rows[i], if i + 1 < count { &rows[i + 1] } else { &last });
        for side in 0..2 {
            for (pa, pz) in a[side].iter().zip(&z[side]) {
                for k in 0..pa.len().min(pz.len()) - 1 {
                    b.quad(pa[k], pa[k + 1], pz[k + 1], pz[k]);
                }
            }
        }
    }
}

/// The barriers along each side: armco rail on posts, tyre walls with their belt, concrete walls.
fn barriers(b: &mut Builder, c: &Circuit) {
    let count = c.samples.len();
    for side in 0..2 {
        let sign = if side == 0 { -1.0 } else { 1.0 };
        for i in 0..count {
            let (s, t) = (&c.samples[i], &c.samples[(i + 1) % count]);
            let kind = s.sides[side].barrier;
            if kind == BarrierKind::None || t.sides[side].barrier == BarrierKind::None {
                continue;
            }
            let (da, db) = (s.sides[side].barrier_at(s.width), t.sides[side].barrier_at(t.width));
            let along = s.along;
            // A point of the wall's face `out` metres behind it, `z` up from the ground there.
            let at = |smp: &circuit::Sample, d: f64, out: f64, z: f64| {
                let across = sign * (d + out);
                let (rn, re) = (-smp.heading.sin(), smp.heading.cos());
                DVec3::new(smp.n + rn * across, smp.e + re * across, smp.height + smp.cross * across + z)
            };
            match kind {
                BarrierKind::Armco => {
                    // A W-shaped rail, 0.45 to 0.75 m up, its ridges towards the track.
                    let profile = [(0.0, 0.45), (-0.05, 0.52), (0.0, 0.6), (-0.05, 0.68), (0.0, 0.75)];
                    for w in profile.windows(2) {
                        let corners = [at(s, da, w[0].0, w[0].1), at(t, db, w[0].0, w[0].1), at(t, db, w[1].0, w[1].1), at(s, da, w[1].0, w[1].1)];
                        b.face(corners, STEEL, Kind::Prop, along);
                    }
                    // A post every other sample, behind the rail.
                    if i % 2 == 0 {
                        let f = Frame::of(c, s.along, sign * (da + 0.12));
                        b.cuboid(&f, DVec3::new(-0.05, -0.06, -0.3), DVec3::new(0.05, 0.06, 0.7), STEEL, along);
                    }
                }
                BarrierKind::Tyres => {
                    // Tyres stacked four high, 0.9 m deep, behind a belt.
                    let (h, deep) = (1.0, 0.9);
                    b.face([at(s, da, 0.0, -0.1), at(t, db, 0.0, -0.1), at(t, db, 0.0, h), at(s, da, 0.0, h)], [0.5, 0.5, 0.5, 0.35], Kind::Belt, along);
                    b.face([at(s, da, 0.0, h), at(t, db, 0.0, h), at(t, db, deep, h), at(s, da, deep, h)], RUBBER, Kind::Prop, along);
                    b.face([at(s, da, deep, -0.1), at(t, db, deep, -0.1), at(t, db, deep, h), at(s, da, deep, h)], RUBBER, Kind::Prop, along);
                }
                BarrierKind::Wall => {
                    let (h, deep) = (1.1, 0.3);
                    b.face([at(s, da, 0.0, -0.1), at(t, db, 0.0, -0.1), at(t, db, 0.0, h), at(s, da, 0.0, h)], WALL, Kind::Prop, along);
                    b.face([at(s, da, 0.0, h), at(t, db, 0.0, h), at(t, db, deep, h), at(s, da, deep, h)], WALL, Kind::Prop, along);
                    b.face([at(s, da, deep, -0.1), at(t, db, deep, -0.1), at(t, db, deep, h), at(s, da, deep, h)], WALL, Kind::Prop, along);
                    // A debris fence on it: a post every 3 samples, a rail along the top.
                    if i % 3 == 0 {
                        let f = Frame::of(c, s.along, sign * (da + 0.15));
                        b.cuboid(&f, DVec3::new(-0.04, -0.04, h), DVec3::new(0.04, 0.04, 3.2), STEEL, along);
                    }
                    b.face([at(s, da, 0.12, 3.1), at(t, db, 0.12, 3.1), at(t, db, 0.18, 3.2), at(s, da, 0.18, 3.2)], STEEL, Kind::Prop, along);
                }
                BarrierKind::None => {}
            }
        }
    }
}

/// Which way across a prop's side is (−1 left, 1 right), and how far out its side's barrier (or
/// the end of the run-off) is at a distance along.
fn side_of(c: &Circuit, side: Side, along: f64) -> (f64, f64, f64) {
    let k = if side == Side::Left { 0 } else { 1 };
    let s = c.sample(along);
    (if k == 0 { -1.0 } else { 1.0 }, s.sides[k].barrier_at(s.width), s.sides[k].reach)
}

/// The pits: a long low building beyond the pit lane, its garages open to it.
fn pits(b: &mut Builder, c: &Circuit, prop: &circuit::Prop) {
    let (sign, _, reach) = side_of(c, prop.side, prop.along);
    let (deep, high) = (14.0, 7.0);
    let step = 6.0;
    let pieces = (prop.length / step).ceil() as usize;
    for k in 0..pieces {
        let along = prop.along - prop.length / 2.0 + k as f64 * step;
        // The building follows the circuit, piece by piece; its front at the end of the pit lane.
        let f = Frame::of(c, along + step / 2.0, sign * (reach + 1.0));
        let (y0, y1) = if sign < 0.0 { (-deep, 0.0) } else { (0.0, deep) };
        let half = step / 2.0 + 0.02;
        b.cuboid(&f, DVec3::new(-half, y0, -2.0), DVec3::new(half, y1, high), [0.62, 0.62, 0.6, 0.1], along);
        // A garage door on its front, dark, and the fascia over it in the circuit's blue.
        let front = if sign < 0.0 { y1 + 0.02 } else { y0 - 0.02 };
        let face = |x0: f64, x1: f64, z0: f64, z1: f64| [f.at(x0, front, z0), f.at(x1, front, z0), f.at(x1, front, z1), f.at(x0, front, z1)];
        b.face(face(-half + 0.6, half - 0.6, 0.0, 4.2), [0.035, 0.037, 0.04, 0.2], Kind::Prop, along);
        b.face(face(-half, half, 5.2, 6.6), [0.02, 0.08, 0.3, 0.3], Kind::Prop, along);
        // The roof's overhang over the pit lane.
        let (o0, o1) = if sign < 0.0 { (y1, y1 + 3.0) } else { (y0 - 3.0, y0) };
        b.cuboid(&f, DVec3::new(-half, o0, high - 0.3), DVec3::new(half, o1, high), [0.7, 0.7, 0.7, 0.2], along);
    }
}

/// The gantry over the start line: two columns, a beam with the start's lights facing the grid.
fn gantry(b: &mut Builder, c: &Circuit, along: f64) {
    let s = c.sample(along);
    let span = s.width / 2.0 + 2.5;
    let f = Frame::of(c, along, 0.0);
    let grey = [0.12, 0.12, 0.13, 0.4];
    for y in [-span, span] {
        b.cuboid(&f, DVec3::new(-0.35, y - 0.35, -0.5), DVec3::new(0.35, y + 0.35, 8.0), grey, along);
    }
    b.cuboid(&f, DVec3::new(-0.6, -span - 0.4, 6.4), DVec3::new(0.6, span + 0.4, 7.8), grey, along);
    // The lights' panel, black, five pairs of red lamps.
    b.cuboid(&f, DVec3::new(-0.75, -2.6, 6.6), DVec3::new(-0.6, 2.6, 7.6), [0.01, 0.01, 0.01, 0.6], along);
    for k in 0..5 {
        let y = -2.0 + k as f64;
        for z in [6.85, 7.3] {
            b.cuboid(&f, DVec3::new(-0.8, y - 0.16, z - 0.16), DVec3::new(-0.74, y + 0.16, z + 0.16), [0.25, 0.01, 0.005, 0.9], along);
        }
    }
    // A white band with the line's chequers on the beam.
    for k in 0..16 {
        let y = -span + (2.0 * span) * k as f64 / 16.0;
        let col = if k % 2 == 0 { [0.7, 0.7, 0.7, 0.3] } else { [0.02, 0.02, 0.02, 0.3] };
        b.face([f.at(-0.62, y, 6.45), f.at(-0.62, y + 2.0 * span / 16.0, 6.45), f.at(-0.62, y + 2.0 * span / 16.0, 6.8), f.at(-0.62, y, 6.8)], col, Kind::Prop, along);
    }
}

/// A grandstand behind the barrier: rows of seats rising away from the track under a roof.
fn grandstand(b: &mut Builder, c: &Circuit, prop: &circuit::Prop) {
    let (sign, barrier, _) = side_of(c, prop.side, prop.along);
    let rows = 14;
    let (tread, rise) = (0.85, 0.45);
    let step = 6.0;
    let pieces = (prop.length / step).ceil() as usize;
    let seats = [[0.03, 0.09, 0.32, 0.4], [0.5, 0.5, 0.52, 0.4]];
    for k in 0..pieces {
        let along = prop.along - prop.length / 2.0 + k as f64 * step;
        let f = Frame::of(c, along + step / 2.0, sign * (barrier + 3.0 + prop.offset));
        let half = step / 2.0 + 0.02;
        let y = |d: f64| sign * d;
        for r in 0..rows {
            let (d0, d1) = (r as f64 * tread, (r + 1) as f64 * tread);
            let z = 0.6 + r as f64 * rise;
            let (a, bb) = (y(d0).min(y(d1)), y(d0).max(y(d1)));
            // The step: concrete, its seats in the stand's colours by blocks.
            b.cuboid(&f, DVec3::new(-half, a, -1.5), DVec3::new(half, bb, z), [0.45, 0.45, 0.43, 0.1], along);
            let colour = seats[(r / 3 + k) % 2];
            b.cuboid(&f, DVec3::new(-half + 0.1, a + 0.1 * sign.max(0.0), z), DVec3::new(half - 0.1, bb - 0.1 * (-sign).max(0.0), z + 0.4), colour, along);
            // The people of the row: a card standing over the seats, facing the track.
            let d = y(d0 + tread * 0.55);
            let (u0, u1) = (along as f32, (along + step) as f32);
            let row = (r * 31 + 7) as f32;
            b.panel([f.at(-half, d, z + 0.25), f.at(half, d, z + 0.25), f.at(half, d, z + 1.2), f.at(-half, d, z + 1.2)], [0.0, 0.0, 0.0, row], Kind::Crowd, [[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]], prop.along as f32);
        }
        // The back wall and the roof on columns.
        let depth = rows as f64 * tread;
        let top = 0.6 + rows as f64 * rise;
        let (a, bb) = (y(depth).min(y(depth + 0.3)), y(depth).max(y(depth + 0.3)));
        b.cuboid(&f, DVec3::new(-half, a, -1.5), DVec3::new(half, bb, top + 3.5), [0.55, 0.55, 0.53, 0.1], along);
        let (r0, r1) = (y(-1.5).min(y(depth + 0.3)), y(-1.5).max(y(depth + 0.3)));
        b.cuboid(&f, DVec3::new(-half, r0, top + 3.5), DVec3::new(half, r1, top + 3.8), [0.75, 0.75, 0.76, 0.5], along);
        if k % 2 == 0 {
            let x = -half + 0.2;
            let (c0, c1) = (y(-1.0).min(y(-1.3)), y(-1.0).max(y(-1.3)));
            b.cuboid(&f, DVec3::new(x, c0, 0.0), DVec3::new(x + 0.3, c1, top + 3.5), [0.6, 0.6, 0.62, 0.5], along);
            // A flag on the roof's edge over each column.
            let pole = y(-1.2);
            b.cuboid(&f, DVec3::new(x, pole - 0.04, top + 3.8), DVec3::new(x + 0.08, pole + 0.04, top + 8.3), STEEL, along);
            let flags = [[0.55, 0.03, 0.03, 0.1], [0.03, 0.10, 0.45, 0.1], [0.6, 0.45, 0.02, 0.1], [0.6, 0.6, 0.6, 0.1], [0.03, 0.3, 0.08, 0.1]];
            b.face([f.at(x + 0.08, pole, top + 6.9), f.at(x + 2.1, pole, top + 6.75), f.at(x + 2.1, pole, top + 8.05), f.at(x + 0.08, pole, top + 8.2)], flags[(k / 2) % flags.len()], Kind::Prop, along);
        }
        // A board along the stand's foot, two pieces long.
        if k % 2 == 0 && k + 1 < pieces {
            let foot = y(-1.55);
            let name = (prop.along as usize / 7 + k / 2) % BRANDS.len();
            let long = 2.0 * step as f32;
            let (x0, x1) = if sign < 0.0 { (-half, half + step) } else { (half + step, -half) };
            b.panel([f.at(x0, foot, 0.1), f.at(x1, foot, 0.1), f.at(x1, foot, 1.1), f.at(x0, foot, 1.1)], board_color(name, along), Kind::Board, [[0.0, 0.0], [long, 0.0], [long, 1.0], [0.0, 1.0]], long);
        }
    }
}

/// A marshals' post: a small white hut with an orange band behind the barrier, a flag on a pole.
fn marshal(b: &mut Builder, c: &Circuit, prop: &circuit::Prop) {
    let (sign, barrier, _) = side_of(c, prop.side, prop.along);
    let f = Frame::of(c, prop.along, sign * (barrier + 1.8 + prop.offset));
    b.cuboid(&f, DVec3::new(-1.2, -1.0, -0.5), DVec3::new(1.2, 1.0, 2.3), [0.65, 0.65, 0.63, 0.2], prop.along);
    b.cuboid(&f, DVec3::new(-1.22, -1.02, 1.8), DVec3::new(1.22, 1.02, 2.1), [0.6, 0.15, 0.01, 0.3], prop.along);
    b.cuboid(&f, DVec3::new(-1.4, -1.2, 2.3), DVec3::new(1.4, 1.2, 2.45), [0.3, 0.3, 0.3, 0.2], prop.along);
    b.cuboid(&f, DVec3::new(1.4, -0.04, 0.0), DVec3::new(1.48, 0.04, 4.5), STEEL, prop.along);
    b.face([f.at(1.48, 0.0, 3.6), f.at(2.5, 0.0, 3.6), f.at(2.5, 0.0, 4.4), f.at(1.48, 0.0, 4.4)], [0.55, 0.45, 0.02, 0.1], Kind::Prop, prop.along);
}

/// A footbridge over the circuit on two towers, its sides a banner.
fn bridge(b: &mut Builder, c: &Circuit, along: f64) {
    let s = c.sample(along);
    let span = s.width / 2.0 + s.sides[0].runoff_width.max(s.sides[1].runoff_width).min(14.0) + 3.0;
    let f = Frame::of(c, along, 0.0);
    let grey = [0.5, 0.5, 0.52, 0.3];
    for y in [-span, span] {
        b.cuboid(&f, DVec3::new(-2.0, y - 2.0, -1.0), DVec3::new(2.0, y + 2.0, 8.5), grey, along);
    }
    b.cuboid(&f, DVec3::new(-1.6, -span, 6.2), DVec3::new(1.6, span, 6.6), grey, along);
    // Its sides: a banner, dark blue with a white stripe.
    for x in [-1.62, 1.62] {
        b.face([f.at(x, -span, 6.6), f.at(x, span, 6.6), f.at(x, span, 7.8), f.at(x, -span, 7.8)], [0.02, 0.05, 0.2, 0.3], Kind::Prop, along);
        b.face([f.at(x * 1.001, -span, 7.05), f.at(x * 1.001, span, 7.05), f.at(x * 1.001, span, 7.3), f.at(x * 1.001, -span, 7.3)], [0.65, 0.65, 0.65, 0.3], Kind::Prop, along);
    }
    b.cuboid(&f, DVec3::new(-1.6, -span, 8.3), DVec3::new(1.6, span, 8.5), [0.7, 0.7, 0.72, 0.4], along);
}

/// The names on the advertising boards: invented, of no real make.
pub const BRANDS: [&str; 8] = ["VANTOR", "KIRUMA", "ZELTIC", "NORDAK", "OLVANE", "TARVEX", "HELQA", "BRIXOL"];

/// A board's colour; in alpha, its name's number and, in eights of a metre times 8, where it is
/// along the circuit (the shader lights it from there by night).
fn board_color(name: usize, along: f64) -> [f32; 4] {
    const COLORS: [[f32; 3]; 8] = [[0.45, 0.02, 0.02], [0.55, 0.55, 0.55], [0.02, 0.07, 0.30], [0.50, 0.36, 0.01], [0.02, 0.02, 0.025], [0.02, 0.22, 0.10], [0.50, 0.16, 0.01], [0.10, 0.30, 0.42]];
    let c = COLORS[name % COLORS.len()];
    [c[0], c[1], c[2], (name % BRANDS.len()) as f32 + 8.0 * (along / 8.0).floor().max(0.0) as f32]
}

/// Advertising boards behind the barriers, where a wall or a rail runs: one every 50 m, 8 m long.
fn boards(b: &mut Builder, c: &Circuit) {
    let (every, long) = (50.0, 8.0);
    let count = (c.length / every).floor() as usize;
    for k in 0..count {
        let along = k as f64 * every + 12.0;
        let s = c.sample(along);
        for side in 0..2 {
            let ss = &s.sides[side];
            // Not where the pits are, nor where nothing holds a board up; one side at a time.
            if ss.pit || ss.barrier == BarrierKind::None || (k + side) % 2 == 0 {
                continue;
            }
            let sign = if side == 0 { -1.0 } else { 1.0 };
            let out = ss.barrier_at(s.width) + if ss.barrier == BarrierKind::Tyres { 1.3 } else { 0.9 };
            if out + 0.5 > ss.reach {
                continue;
            }
            let f = Frame::of(c, along, sign * out);
            let name = (k * 5 + side * 3) % BRANDS.len();
            let (x0, x1) = if sign < 0.0 { (-long / 2.0, long / 2.0) } else { (long / 2.0, -long / 2.0) };
            let (z0, z1) = (1.15, 2.15);
            b.panel([f.at(x0, 0.0, z0), f.at(x1, 0.0, z0), f.at(x1, 0.0, z1), f.at(x0, 0.0, z1)], board_color(name, along), Kind::Board, [[0.0, 0.0], [long as f32, 0.0], [long as f32, 1.0], [0.0, 1.0]], long as f32);
            // Its back and its two legs.
            let back = sign * 0.06;
            b.face([f.at(x1, back, z0), f.at(x0, back, z0), f.at(x0, back, z1), f.at(x1, back, z1)], [0.25, 0.25, 0.26, 0.2], Kind::Prop, along);
            for x in [-long / 2.0 + 0.6, long / 2.0 - 0.6] {
                b.cuboid(&f, DVec3::new(x - 0.04, back.min(0.0) - 0.02, -0.3), DVec3::new(x + 0.04, back.max(0.0) + 0.02, z0), STEEL, along);
            }
        }
    }
}

/// The floodlights' towers (`towers`): a mast with a rack of lamps turned to the track.
fn masts(b: &mut Builder, c: &Circuit) {
    for t in b.towers.clone() {
        let (along, sign) = (t.along, t.across.signum());
        let f = Frame::of(c, along, t.across);
        let h = TOWER_HEIGHT;
        b.cuboid(&f, DVec3::new(-0.18, -0.18, -3.0), DVec3::new(0.18, 0.18, h), STEEL, along);
        // The rack: a frame facing the track with three rows of four lamps.
        let face = -sign * 0.35;
        b.cuboid(&f, DVec3::new(-1.6, face.min(0.0) - 0.05, h - 2.2), DVec3::new(1.6, face.max(0.0) + 0.05, h + 0.3), [0.06, 0.06, 0.065, 0.3], along);
        for row in 0..3 {
            for col in 0..4 {
                let (x, z) = (-1.2 + 0.8 * col as f64, h - 1.8 + 0.75 * row as f64);
                let y = face - sign * 0.08;
                let (x0, x1) = if sign < 0.0 { (x + 0.28, x - 0.28) } else { (x - 0.28, x + 0.28) };
                b.panel([f.at(x0, y, z - 0.25), f.at(x1, y, z - 0.25), f.at(x1, y, z + 0.25), f.at(x0, y, z + 0.25)], [1.0, 0.97, 0.9, 0.0], Kind::Lamp, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], along as f32);
            }
        }
    }
}

/// The letters the boards are written with, for the shader: each of A to Z as 35 dots (five
/// across, seven down, the top row first) in two numbers, and the letters of each name.
fn lettering() -> String {
    const FONT: [[u8; 7]; 26] = [
        [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
    ];
    // A dot's bit: row * 5 + column from the left, rows 0 to 3 in the first number, 4 to 6 in the second.
    let mut font = Vec::new();
    for glyph in FONT {
        let bits = |rows: &[u8]| rows.iter().enumerate().fold(0u32, |acc, (r, row)| acc | (0..5).fold(0u32, |a, col| a | ((((row >> (4 - col)) & 1) as u32) << (r * 5 + col))));
        font.push(format!("{}u", bits(&glyph[..4])));
        font.push(format!("{}u", bits(&glyph[4..])));
    }
    let mut letters = Vec::new();
    for name in BRANDS {
        for k in 0..8 {
            letters.push(name.as_bytes().get(k).map_or("-1".to_string(), |b| (b - b'A').to_string()));
        }
    }
    format!(
        "const TRACK_FONT = array<u32, 52>({});\nconst TRACK_NAMES = array<i32, {}>({});\nconst TRACK_NAME_COUNT: i32 = {};\n",
        font.join(", "),
        BRANDS.len() * 8,
        letters.join(", "),
        BRANDS.len()
    )
}

/// The circuit's shader on the world's, for a quality.
pub fn track_source(quality: &Quality) -> String {
    shader_source(quality) + &lettering() + &format!("const TOWER_LIGHT: f32 = {TOWER_LIGHT:?};\n") + include_str!("track.wgsl")
}

/// A circuit on the GPU.
struct OnGpu {
    origin: DVec3,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    /// The triangles of its paved ground alone (what is a mirror when wet), for the second pass.
    paved: wgpu::Buffer,
    paved_count: u32,
    /// The paint's numbers (`object.tint` in `track.wgsl`): start along, lap length, grid places.
    paint: [f32; 4],
    towers: Vec<Tower>,
}

/// What the second pass over a wet circuit needs: its pipeline, and the scene's colour and depth
/// as it reads them, for the targets of one `Gpu::targets_made`.
struct Mirror {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind: Option<(wgpu::BindGroup, u64)>,
}

/// Draws the circuits of a terrain: see the module.
pub struct TrackRenderer {
    circuits: Vec<OnGpu>,
    pipeline: Option<(wgpu::RenderPipeline, Quality, u32)>,
    mirror: Option<Mirror>,
    uniforms: UniformArena,
    draws: Vec<(usize, u32)>,
    /// How wet the circuits are, 0 dry to 1 soaked with standing water: the game says (it rains).
    pub wet: f32,
    /// The dipped beams of the car the view follows (`Scene::body`), candelas in their middle
    /// (0: not lit), and its tail lamps', as they light the ground behind it: the game says.
    pub beams: f32,
    pub tail: f32,
    /// The hour of the day, 0 to 24: how bright a candela of those lamps is in the scene.
    pub hour: f64,
}

/// The vertex layout of `Vertex`.
fn layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Float32x2];
    wgpu::VertexBufferLayout { array_stride: size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRIBUTES }
}

/// The triangles of a mesh that are paved ground: asphalt, kerbs, concrete.
fn paved(m: &TrackMesh) -> Vec<u32> {
    let is = |i: u32| matches!(m.vertices[i as usize].surface[0] as u8, 1 | 2 | 5 | 6);
    m.indices.chunks_exact(3).filter(|t| is(t[0])).flatten().copied().collect()
}

impl TrackRenderer {
    /// The circuits of `terrain` (`Terrain::circuits`) on the GPU, ready to draw in `world`.
    pub fn new(gpu: &Gpu, world: &WorldRenderer, terrain: &Arc<Terrain>) -> TrackRenderer {
        TrackRenderer::with_lines(gpu, world, terrain, &[])
    }

    /// As `new`, with the line the cars run on round each circuit of the terrain, in their order
    /// (a `Line` per sample; a circuit without one has it down its middle): the asphalt is darker
    /// with rubber along it.
    pub fn with_lines(gpu: &Gpu, world: &WorldRenderer, terrain: &Arc<Terrain>, lines: &[Vec<Line>]) -> TrackRenderer {
        let circuits = terrain
            .circuits()
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let m = mesh_with(c, lines.get(k).map_or(&[][..], |l| l.as_slice()));
                let vertices = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("circuit"), contents: bytemuck::cast_slice(&m.vertices), usage: wgpu::BufferUsages::VERTEX });
                let indices = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("circuit indices"), contents: bytemuck::cast_slice(&m.indices), usage: wgpu::BufferUsages::INDEX });
                let wet = paved(&m);
                let paved = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("circuit paved indices"), contents: bytemuck::cast_slice(&wet), usage: wgpu::BufferUsages::INDEX });
                OnGpu { origin: m.origin, vertices, indices, count: m.indices.len() as u32, paved, paved_count: wet.len() as u32, paint: [0.0, c.start as f32, c.length as f32, 20.0], towers: towers(c) }
            })
            .collect();
        TrackRenderer { circuits, pipeline: None, mirror: None, uniforms: UniformArena::new(gpu, world.object_layout(), size_of::<ObjectUniforms>(), 8), draws: Vec::new(), wet: 0.0, beams: 0.0, tail: 0.0, hour: 12.0 }
    }

    /// The floodlights of the first circuit (`towers`): for a game to light its vehicles by
    /// (`floodlight`) and to give the nearest to the world as lamps.
    pub fn towers(&self) -> &[Tower] {
        self.circuits.first().map_or(&[], |c| c.towers.as_slice())
    }

    /// Whether the circuits mirror the scene this frame (they are wet, and the quality draws
    /// mirrors): the game then makes a second pass that reads the first (`Game::translucent`,
    /// `Game::reads_scene`) and calls `before_wet` and `draw_wet` in it. As of the last `prepare`.
    pub fn mirrors(&self) -> bool {
        self.wet > 0.0 && self.mirror.is_some() && !self.circuits.is_empty()
    }

    /// Before `WorldRenderer::prepare`'s frame is drawn: the pipeline for the world's quality and
    /// samples, and where each circuit is from the eye (north, east, down).
    pub fn prepare(&mut self, gpu: &Gpu, world: &WorldRenderer, eye: DVec3) {
        let quality = world.quality();
        if self.pipeline.as_ref().is_none_or(|(_, q, samples)| *q != quality || *samples != gpu.sample_count) {
            let source = track_source(&quality);
            let shader = gpu.shader_with_ending_loops("circuit", &source);
            let buffers = [layout()];
            let pipeline = gpu.pipeline(&PipelineDesc {
                label: "circuit",
                shader: &shader,
                vertex: "track_vertex",
                fragment: "track_fragment",
                buffers: &buffers,
                bind_groups: &[world.frame_layout(), world.object_layout()],
                depth: Depth::Write,
                blend: false,
            });
            self.pipeline = Some((pipeline, quality, gpu.sample_count));
            // The second pass over a wet circuit reads the scene's depth, which has several
            // samples per pixel: with one sample, or at a quality without mirrors, there is none.
            self.mirror = (gpu.sample_count > 1 && quality.reflection > 0).then(|| {
                let texture = |binding, sample_type, multisampled| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled }, count: None };
                let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("wet circuit"),
                    entries: &[
                        texture(8, wgpu::TextureSampleType::Float { filterable: true }, false),
                        texture(9, wgpu::TextureSampleType::Depth, true),
                        wgpu::BindGroupLayoutEntry { binding: 10, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                    ],
                });
                let desc = PipelineDesc { label: "wet circuit", shader: &shader, vertex: "track_vertex", fragment: "track_wet_fragment", buffers: &buffers, bind_groups: &[world.frame_layout(), world.object_layout(), &layout], depth: Depth::Read, blend: false };
                let pipeline = gpu.pipeline_blended(&desc, Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));
                let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("wet circuit"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
                Mirror { pipeline, layout, sampler, bind: None }
            });
        }
        self.uniforms.clear();
        self.draws.clear();
        for (k, c) in self.circuits.iter().enumerate() {
            let rel = DVec3::new(c.origin.x, c.origin.y, -c.origin.z) - eye;
            let model = Mat4::from_translation(to_render(rel));
            let mut u = ObjectUniforms::new(model.to_cols_array_2d(), [self.wet.clamp(0.0, 1.0), c.paint[1], c.paint[2], c.paint[3]]);
            // The four towers nearest the eye, as they are: where their lamps are from the eye
            // and how strong (`track_glints`). The cuts' numbers are free: nothing cuts a circuit.
            let mut near: Vec<(f64, &Tower)> = c.towers.iter().map(|t| ((DVec3::new(t.lamp.x, t.lamp.y, -t.lamp.z) - eye).length_squared(), t)).collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (slot, (_, t)) in u.cuts.iter_mut().zip(&near) {
                let at = to_render(DVec3::new(t.lamp.x, t.lamp.y, -t.lamp.z) - eye);
                *slot = [at.x, at.y, at.z, t.power as f32];
            }
            // What the followed car's lamps give: a white surface a metre from a lamp of `I`
            // candelas is `I · gain / π` in the scene (as the world's floodlights, `renderer.rs`).
            let gain = crate::lights::gain(self.hour) / std::f32::consts::PI;
            u.keep = [self.beams.max(0.0) * gain, self.tail.max(0.0) * gain, 0.0, 0.0];
            let offset = self.uniforms.push(&u);
            self.draws.push((k, offset));
        }
        self.uniforms.upload(gpu);
    }

    /// Between the two passes of a frame in which the circuits mirror the scene (`mirrors`): what
    /// the second pass reads, the first's colour and depth.
    pub fn before_wet(&mut self, gpu: &Gpu) {
        let Some(mirror) = &mut self.mirror else { return };
        let Some(scene) = gpu.scene_kept() else {
            mirror.bind = None;
            return;
        };
        if mirror.bind.as_ref().is_none_or(|(_, made)| *made != gpu.targets_made()) {
            let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("wet circuit"),
                layout: &mirror.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(scene) },
                    wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(gpu.scene_depth()) },
                    wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::Sampler(&mirror.sampler) },
                ],
            });
            mirror.bind = Some((bind, gpu.targets_made()));
        }
    }

    /// Draws the scene mirrored in the wet circuits over them, in the second pass (group 0
    /// bound, `before_wet` called): before anything see-through that stands over the road.
    pub fn draw_wet(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((mirror, (bind, _))) = self.mirror.as_ref().and_then(|m| m.bind.as_ref().map(|b| (m, b))) else { return };
        if self.wet <= 0.0 {
            return;
        }
        pass.set_pipeline(&mirror.pipeline);
        pass.set_bind_group(2, bind, &[]);
        for &(k, offset) in &self.draws {
            let c = &self.circuits[k];
            pass.set_bind_group(1, self.uniforms.bind_group(), &[offset]);
            pass.set_vertex_buffer(0, c.vertices.slice(..));
            pass.set_index_buffer(c.paved.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..c.paved_count, 0, 0..1);
        }
    }

    /// Draws the circuits, after `WorldRenderer::draw_sky` (group 0 bound) and before `draw`.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((pipeline, ..)) = &self.pipeline else { return };
        pass.set_pipeline(pipeline);
        for &(k, offset) in &self.draws {
            let c = &self.circuits[k];
            pass.set_bind_group(1, self.uniforms.bind_group(), &[offset]);
            pass.set_vertex_buffer(0, c.vertices.slice(..));
            pass.set_index_buffer(c.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..c.count, 0, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::circuit::CircuitFile;

    #[test]
    fn the_boards_are_lettered_and_the_towers_light_their_side() {
        // Every name is of letters the font has, eight at most, and the tables are whole.
        assert!(BRANDS.iter().all(|b| b.len() <= 8 && b.bytes().all(|c| c.is_ascii_uppercase())));
        let tables = lettering();
        let numbers = |name: &str| tables.lines().find(|l| l.starts_with(&format!("const {name} "))).map(|l| l.split(['(', ')']).nth(1).unwrap_or("").split(',').count());
        assert_eq!(numbers("TRACK_FONT"), Some(52));
        assert_eq!(numbers("TRACK_NAMES"), Some(BRANDS.len() * 8));
        // A in the font: its top row is the three middle dots.
        let first: u32 = tables.split('(').nth(1).unwrap().split(',').next().unwrap().trim().trim_end_matches('u').parse().unwrap();
        assert_eq!(first & 0b11111, 0b01110);
        // The towers stand where the circuit's file places them, beyond its ground on their side;
        // the ground under one is brighter than as far out on the other side of the track, and
        // the whole of the asphalt has some of their light.
        let file = CircuitFile::find("lakeside").unwrap();
        let c = Circuit::new(file, &Terrain::new(file.seed()));
        let all = towers(&c);
        assert_eq!(all.len(), c.props.iter().filter(|p| p.kind == PropKind::Floodlight).count());
        assert!(all.len() > 20, "{} towers", all.len());
        let t = all[0];
        let at = |across: f64| {
            let (n, e, h) = c.point(t.along, across);
            floodlight(&all, DVec3::new(n, e, h))
        };
        assert!(at(t.across * 0.8) > at(-t.across * 0.8) && at(-t.across * 0.8) > 0.0);
        let darkest = c.samples.iter().map(|s| floodlight(&all, DVec3::new(s.n, s.e, s.height))).fold(f32::MAX, f32::min);
        assert!(darkest > 0.2, "the darkest of the asphalt has {darkest}");
        // The mesh has the crowd's cards, the boards and the lamps.
        let m = mesh_with(&c, &vec![[1.5, 0.8]; c.samples.len()]);
        for kind in [Kind::Crowd, Kind::Board, Kind::Lamp] {
            assert!(m.vertices.iter().any(|v| v.surface[0] == kind as u8 as f32), "{kind:?}");
        }
        // The asphalt carries the line given.
        assert!(m.vertices.iter().filter(|v| v.surface[0] == Kind::Asphalt as u8 as f32).all(|v| v.color[0] == 1.5 && v.color[1] == 0.8));
    }

    #[test]
    fn a_circuit_makes_a_whole_mesh() {
        let file = CircuitFile::find("lakeside").unwrap();
        let c = Circuit::new(file, &Terrain::new(file.seed()));
        let m = mesh(&c);
        assert!(m.vertices.len() > 40_000 && m.indices.len().is_multiple_of(3));
        assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
        assert!(m.vertices.iter().all(|v| v.position.iter().chain(&v.normal).all(|x| x.is_finite())));
        // The mesh's asphalt is where the simulation's is.
        let s = &c.samples[100];
        let p = c.patch(s.n, s.e).unwrap();
        let near = m.vertices.iter().filter(|v| v.surface[0] == Kind::Asphalt as u8 as f32).map(|v| {
            let o = m.origin;
            let (n, e, h) = (o.x - v.position[2] as f64, o.y + v.position[0] as f64, o.z + v.position[1] as f64);
            ((n - s.n).hypot(e - s.e), h)
        }).fold((f64::MAX, 0.0), |a, b| if b.0 < a.0 { b } else { a });
        assert!((near.1 - p.height).abs() < 0.02, "mesh {:.3} m, simulation {:.3} m", near.1, p.height);
    }
}
