//! The meshes of the trees and bushes, one per species (`species.rs`) and level of detail, made
//! when the renderer starts. Every one is a unit model: its foot at the origin, 1 m tall, y up; the
//! renderer scales it by the height of each tree and widens it by the tree's own factor.
//!
//! A tree is a trunk and branches, tubes with the photograph of a bark, and its foliage: cards with
//! the pictures of `foliage.rs`, set in clusters round the ends of the branches and over the
//! crown, two crossed cards a cluster (a spruce's and a fir's lie along their branches in a V,
//! a palm's fronds arch out of its top). Every vertex knows where it is in its crown, so that the
//! shaders light the foliage as one volume: its normal leans out of the crown's middle, its colour's
//! alpha says how open to the sky it is, and `crown` is its place in the crown's ellipsoid
//! (`species::Look::crown`), through which the sun's light is followed to shade the crown by itself.
//!
//! Three levels: `Detail::Near`, the full tree (some 300 to 700 triangles); `Detail::Far`, its
//! trunk and main limbs with fewer and bigger cards (80 to 200); and beyond, a picture of the full
//! tree from the side it is seen from (`impostors.rs`).
//!
//! A vertex is `FLOATS` floats (`FloraMesh::layout`): position; normal; colour (rgb) and openness;
//! the texture's u, v, its layer (a `foliage::Layer`, or `BARK` plus a `species::Bark`) and how much
//! the wind moves it; its place in the crown (x, y, z in crown radii) and a phase of its own.

use crate::foliage::Layer;
use crate::species::{Look, Species};
use engine::wgpu::util::DeviceExt;
use engine::{Mesh, MeshBuilder, wgpu};
use glam::{Vec2, Vec3, Vec4};
use std::f32::consts::{PI, TAU};

pub const FLOATS: usize = 18;
/// Layers from this number on are barks (`BARK + species::Bark as u32`).
pub const BARK: f32 = 16.0;
/// Repeats of a bark's photograph per unit of the model, along the trunk.
const BARK_REPEAT: f32 = 7.0;

/// The levels of detail a species has a mesh for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Near,
    Far,
}

/// A mesh being built: vertices and the triangles between them.
#[derive(Default, Clone)]
pub struct FloraMesh {
    pub data: Vec<f32>,
    pub indices: Vec<u32>,
}

struct Vertex {
    p: Vec3,
    n: Vec3,
    colour: Vec3,
    open: f32,
    uv: Vec2,
    layer: f32,
    sway: f32,
    crown: Vec3,
    phase: f32,
}

impl FloraMesh {
    fn push(&mut self, v: Vertex) -> u32 {
        let index = (self.data.len() / FLOATS) as u32;
        self.data.extend_from_slice(&[v.p.x, v.p.y, v.p.z, v.n.x, v.n.y, v.n.z, v.colour.x, v.colour.y, v.colour.z, v.open, v.uv.x, v.uv.y, v.layer, v.sway, v.crown.x, v.crown.y, v.crown.z, v.phase]);
        index
    }

    pub fn vertices(&self) -> usize {
        self.data.len() / FLOATS
    }

    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Vertex and index buffers on the GPU.
    pub fn build(&self, device: &wgpu::Device) -> Mesh {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("flora"), contents: bytemuck::cast_slice(&self.data), usage: wgpu::BufferUsages::VERTEX });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("flora indices"), contents: bytemuck::cast_slice(&self.indices), usage: wgpu::BufferUsages::INDEX });
        Mesh { buffer, count: self.indices.len() as u32, indices: Some(indices) }
    }

    /// Slot 0 of the trees' pipelines: locations 0, 1, 2, 5 and 6 (3 and 4 are the instance's).
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 5 => Float32x4, 6 => Float32x4];
        wgpu::VertexBufferLayout { array_stride: (FLOATS * 4) as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRS }
    }
}

/// A small generator of numbers, so that a species is the same tree every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }

    /// A direction, unit, up to `low` below the horizontal (sin of the angle).
    fn direction(&mut self, low: f32) -> Vec3 {
        let y = self.range(-low, 1.0);
        let a = self.range(0.0, TAU);
        let r = (1.0 - y * y).max(0.0).sqrt();
        Vec3::new(r * a.cos(), y, r * a.sin())
    }
}

/// Two directions square to `axis` and to each other.
fn square_to(axis: Vec3) -> (Vec3, Vec3) {
    let helper = if axis.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let a = axis.cross(helper).normalize();
    (a, axis.cross(a))
}

/// A tree being built: its mesh, its look, its crown.
struct Builder {
    mesh: FloraMesh,
    look: &'static Look,
    centre: Vec3,
    radii: Vec3,
    rng: Rng,
    /// How far out of the crown's middle a cluster may be, in crown radii: the cards reach on
    /// beyond, and the tree should not outgrow the crown the simulation runs into.
    reach: f32,
}

impl Builder {
    fn new(species: Species) -> Builder {
        let look = species.look();
        let (cy, rx, ry) = look.crown;
        Builder {
            mesh: FloraMesh::default(),
            look,
            centre: Vec3::new(0.0, cy, 0.0),
            radii: Vec3::new(rx, ry, rx),
            reach: f32::INFINITY,
            rng: Rng(0x2545_F491_4F6C_DD1D ^ (species as u64 + 7).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
        }
    }

    /// A point's place in the crown, in crown radii.
    fn in_crown(&self, p: Vec3) -> Vec3 {
        (p - self.centre) / self.radii
    }

    /// The normal of the crown's volume at a point: out of its middle, a little up.
    fn volume_normal(&self, p: Vec3) -> Vec3 {
        let q = self.in_crown(p);
        let out = (q / self.radii).normalize_or(Vec3::Y);
        (out + Vec3::Y * 0.3).normalize()
    }

    /// How open to the sky a point of the crown is: dark deep inside and underneath, open at the
    /// top and the outside.
    fn openness(&self, p: Vec3) -> f32 {
        let q = self.in_crown(p);
        (0.2 + 0.55 * q.length().min(1.2) + 0.3 * q.y.clamp(-1.0, 1.0)).clamp(0.08, 1.0)
    }

    /// How much the wind moves a point: nothing at the foot, most at the top and the ends of the branches.
    fn sway(&self, p: Vec3) -> f32 {
        (p.y.max(0.0).powf(1.5) * 0.7 + Vec2::new(p.x, p.z).length() * 1.2).min(1.5)
    }

    /// A tube along `path` (points and radii), `sides` round, with the bark of the species.
    fn tube(&mut self, path: &[(Vec3, f32)], sides: usize) {
        if path.len() < 2 {
            return;
        }
        let layer = BARK + self.look.bark as u32 as f32;
        let colour = self.look.bark_tint;
        let phase = self.rng.next();
        // Frames carried along the path, so the tube does not twist.
        let mut along = (path[1].0 - path[0].0).normalize_or(Vec3::Y);
        let (mut a, _) = square_to(along);
        let mut length = 0.0;
        let first = self.mesh.vertices() as u32;
        for (k, &(p, r)) in path.iter().enumerate() {
            if k > 0 {
                length += (p - path[k - 1].0).length();
                let next = if k + 1 < path.len() { (path[k + 1].0 - p).normalize_or(along) } else { (p - path[k - 1].0).normalize_or(along) };
                let turned = (along + next).normalize_or(next);
                a = (a - turned * a.dot(turned)).normalize_or(square_to(turned).0);
                along = turned;
            }
            let b = along.cross(a);
            let around = (TAU * r * BARK_REPEAT).round().max(1.0);
            for i in 0..=sides {
                let angle = i as f32 / sides as f32 * TAU;
                let n = a * angle.cos() + b * angle.sin();
                let point = p + n * r;
                let open = (0.45 + 0.4 * (point.y / self.centre.y).min(1.0)).min(0.9) * if point.y > self.centre.y - self.radii.y { self.openness(point).max(0.3) } else { 1.0 };
                let v = Vertex { p: point, n, colour, open, uv: Vec2::new(i as f32 / sides as f32 * around, length * BARK_REPEAT), layer, sway: self.sway(p), crown: self.in_crown(point), phase };
                self.mesh.push(v);
            }
        }
        let ring = (sides + 1) as u32;
        for k in 0..path.len() as u32 - 1 {
            for i in 0..sides as u32 {
                let (a, b, c, d) = (first + k * ring + i, first + k * ring + i + 1, first + (k + 1) * ring + i, first + (k + 1) * ring + i + 1);
                self.mesh.indices.extend_from_slice(&[a, c, d, a, d, b]);
            }
        }
    }

    /// One card of foliage: its base, the way it grows, the way across it, its length and width.
    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, base: Vec3, up: Vec3, across: Vec3, length: f32, width: f32, layer: Layer, colour: Vec3, phase: f32) {
        let flip = self.rng.next() < 0.5;
        let corners = [(-0.5f32, 0.0f32), (0.5, 0.0), (-0.5, 1.0), (0.5, 1.0)];
        let first = self.mesh.vertices() as u32;
        for (u, v) in corners {
            let p = base + across * (u * width) + up * (v * length);
            let uv = Vec2::new(if flip { 0.5 - u } else { u + 0.5 }, 1.0 - v);
            let vertex = Vertex { p, n: self.volume_normal(p), colour, open: self.openness(p), uv, layer: layer as u32 as f32, sway: self.sway(p), crown: self.in_crown(p), phase };
            self.mesh.push(vertex);
        }
        self.mesh.indices.extend_from_slice(&[first, first + 2, first + 3, first, first + 3, first + 1]);
    }

    /// The colour of a cluster of leaves: the species', a little lighter or darker, warmer or cooler.
    fn leaf_colour(&mut self) -> Vec3 {
        let b = self.rng.range(0.82, 1.18);
        let warm = self.rng.range(-0.06, 0.06);
        self.look.leaf * Vec3::new(b * (1.0 + warm), b, b * (1.0 - warm))
    }

    /// A cluster of leaves round `at`, facing `facing` (the way it is seen from best, out of the
    /// crown): cards of `size` whose twigs grow up along the crown's surface, each turned about its
    /// twig a little the other way, so that the cluster has depth from the side too.
    fn cluster(&mut self, at: Vec3, facing: Vec3, size: f32, cards: usize) {
        let q = self.in_crown(at);
        let at = if q.length() > self.reach { self.centre + (at - self.centre) * (self.reach / q.length()) } else { at };
        let colour = self.leaf_colour();
        let phase = self.rng.next();
        let facing = facing.normalize();
        let up = (Vec3::Y - facing * facing.y).normalize_or(square_to(facing).0);
        let spin = self.rng.range(-0.8, 0.8);
        let up = up * spin.cos() + facing.cross(up) * spin.sin();
        let across = up.cross(facing);
        for k in 0..cards {
            let angle = (k as f32 - (cards - 1) as f32 / 2.0) * 1.05 + self.rng.range(-0.2, 0.2);
            let across = across * angle.cos() + facing * angle.sin();
            let length = size * self.rng.range(0.85, 1.15);
            self.card(at - up * (length * 0.5), up, across, length, length, self.look.foliage, colour, phase);
        }
    }

    /// Clusters over the crown's ellipsoid, out to its shell: `count` of them, `size` each, none
    /// in `gaps` (directions from the middle, cosines), facing out.
    fn fill(&mut self, count: usize, size: f32, low: f32, gaps: &[(Vec3, f32)], cards: usize) {
        let mut placed = 0;
        let mut tries = 0;
        while placed < count && tries < count * 20 {
            tries += 1;
            let d = self.rng.direction(low);
            if gaps.iter().any(|(g, cos)| d.dot(*g) > *cos) {
                continue;
            }
            let r = self.rng.range(0.2, 0.85).powf(0.6);
            let at = self.centre + d * self.radii * r;
            let facing = d + Vec3::Y * 0.25 + self.rng.direction(1.0) * 0.3;
            self.cluster(at, facing, size, cards);
            placed += 1;
        }
    }

    /// A branch from `from` along `dir` for `length`, bending up by `rise` and wandering by `gnarl`:
    /// its points and radii, from `radius` to a third of it.
    #[allow(clippy::too_many_arguments)]
    fn branch(&mut self, from: Vec3, dir: Vec3, length: f32, radius: f32, rise: f32, gnarl: f32, steps: usize) -> Vec<(Vec3, f32)> {
        let mut points = vec![(from, radius)];
        let mut d = dir.normalize();
        let mut p = from;
        for k in 1..=steps {
            let t = k as f32 / steps as f32;
            d = (d + Vec3::Y * rise / steps as f32 + self.rng.direction(1.0) * gnarl).normalize();
            p += d * (length / steps as f32);
            points.push((p, radius * (1.0 - 0.66 * t)));
        }
        points
    }

    fn finish(self) -> FloraMesh {
        self.mesh
    }
}

/// How a broadleaf is made.
struct Broad {
    /// Where the trunk forks, and its radius at the foot.
    fork: f32,
    trunk: f32,
    /// The trunk's lean and wander.
    lean: Vec3,
    limbs: usize,
    /// Branches off each limb.
    twigs: usize,
    gnarl: f32,
    /// Clusters of leaves near and far, and their size.
    clusters: usize,
    size: f32,
    /// Directions from the crown's middle left bare (cosines of their half angles).
    gaps: usize,
    /// Several stems from the ground instead of a trunk.
    stems: bool,
}

fn broad(species: Species, detail: Detail, b: Broad) -> FloraMesh {
    let mut t = Builder::new(species);
    let near = detail == Detail::Near;
    t.reach = if near { 0.85 } else { 0.72 };
    let gaps: Vec<(Vec3, f32)> = (0..b.gaps).map(|_| (t.rng.direction(0.3), t.rng.range(0.82, 0.9))).collect();
    let (sides, limb_sides) = if near { (7, 5) } else { (5, 3) };
    let top = if b.stems {
        Vec3::ZERO
    } else {
        let trunk = t.branch(Vec3::ZERO, Vec3::Y + b.lean, b.fork, b.trunk, 0.0, 0.05, 4);
        // The foot spreads into its roots.
        let mut path = trunk.clone();
        path[0].1 *= 1.6;
        t.tube(&path, sides);
        trunk.last().map_or(Vec3::Y * b.fork, |p| p.0)
    };
    let mut tips = Vec::new();
    for k in 0..b.limbs {
        let a = (k as f32 + t.rng.range(0.0, 0.7)) / b.limbs as f32 * TAU;
        // Each limb reaches out towards a point of the crown.
        let aim = t.centre + Vec3::new(a.cos() * t.radii.x * 0.65, t.rng.range(-0.1, 0.45) * t.radii.y, a.sin() * t.radii.z * 0.65);
        let dir = aim - top;
        let radius = b.trunk * 0.6;
        let limb = t.branch(top, dir, dir.length(), radius, 0.15, b.gnarl, 3);
        // A bush's stems are hidden by its leaves from far.
        if near || !b.stems {
            t.tube(&limb, limb_sides);
        }
        if near {
            for j in 0..b.twigs {
                let (from, r) = limb[1 + j % (limb.len() - 1)];
                let out = (from - Vec3::new(0.0, from.y, 0.0)).normalize_or(Vec3::X);
                let dir = (out + Vec3::Y * t.rng.range(0.2, 0.9) + t.rng.direction(1.0) * 0.4).normalize();
                let reach = (t.radii.x * t.rng.range(0.35, 0.6)).max(0.05);
                let twig = t.branch(from, dir, reach, r * 0.6, 0.2, b.gnarl, 2);
                t.tube(&twig, 3);
                tips.push(twig[twig.len() - 1].0);
            }
        }
        tips.push(limb[limb.len() - 1].0);
    }
    let (clusters, size, cards) = if near { (b.clusters, b.size, 2) } else { (b.clusters / 2, b.size * 1.45, 2) };
    // Leaves round the ends of the branches, and over the crown.
    for tip in tips.iter().take(clusters / 3) {
        let up = (t.volume_normal(*tip) + Vec3::Y * 0.4).normalize();
        t.cluster(*tip, up, size, cards);
    }
    let rest = clusters.saturating_sub(tips.len().min(clusters / 3));
    t.fill(rest, size, 0.55, &gaps, cards);
    t.finish()
}

/// How a conifer of tiers of branches is made.
struct Tiered {
    tiers: usize,
    branches: usize,
    /// Where the lowest branches are, and how far out they reach.
    low: f32,
    reach: f32,
    /// How much the branches hang (the tangent of their angle below the horizontal).
    droop: f32,
    /// The angle between the two cards of a branch, radians: a V that opens up.
    vee: f32,
    width: f32,
}

fn tiered(species: Species, detail: Detail, c: Tiered) -> FloraMesh {
    let mut t = Builder::new(species);
    let near = detail == Detail::Near;
    let trunk = [(Vec3::ZERO, 0.026), (Vec3::Y * 0.05, 0.017), (Vec3::Y * 0.5, 0.01), (Vec3::Y * 0.95, 0.002)];
    t.tube(&trunk, if near { 6 } else { 4 });
    // Near, each branch is a V of two sprays and a third hanging from it (a spruce's curtain of
    // branchlets); far, the V alone, on fewer and wider branches.
    let (tiers, branches, angles): (usize, usize, &[f32]) = if near { (c.tiers, c.branches, &[0.5, -0.5, std::f32::consts::FRAC_PI_2]) } else { (c.tiers / 2 + 1, (c.branches * 2 / 3).max(4), &[0.5, -0.5]) };
    let layer = t.look.foliage;
    for k in 0..tiers {
        let f = k as f32 / tiers as f32;
        let y = c.low + (0.95 - c.low) * f + t.rng.range(-0.01, 0.01);
        let reach = c.reach * (1.0 - f).powf(0.9) + 0.025;
        let turn = t.rng.range(0.0, TAU);
        for j in 0..branches {
            let a = turn + j as f32 / branches as f32 * TAU + t.rng.range(-0.25, 0.25);
            let out = Vec3::new(a.cos(), 0.0, a.sin());
            let dir = (out - Vec3::Y * c.droop * t.rng.range(0.7, 1.3)).normalize();
            let length = reach * t.rng.range(0.85, 1.12);
            let colour = t.leaf_colour();
            let phase = t.rng.next();
            let base = Vec3::Y * y;
            // Across the branch, level, and square to both.
            let flat = dir.cross(Vec3::Y).normalize();
            let lift = flat.cross(dir).normalize();
            for (s, angle) in angles.iter().enumerate() {
                let angle = angle * if s < 2 { c.vee } else { 1.0 };
                let across = flat * angle.cos() + lift * angle.sin();
                let width = length * c.width * if near { 1.0 } else { 1.3 };
                // The hanging spray from half the branch on, down from it.
                let (from, length) = if s == 2 { (base + dir * length * 0.35 - lift * width * 0.3, length * 0.65) } else { (base, length) };
                t.card(from, dir, across, length, width, layer, colour, phase);
            }
        }
    }
    // The leader at the top.
    let top = Vec3::Y * (0.97 - 0.1);
    let (a, b) = square_to(Vec3::Y);
    let colour = t.leaf_colour();
    for across in [a, b] {
        t.card(top, Vec3::Y, across, 0.12, 0.07, layer, colour, 0.5);
    }
    t.finish()
}

/// A pine of the woods (Aleppo, Scots): a leaning trunk bare to the half, a few long branches with
/// clumps of needles at their ends, the sky through the crown.
fn pine(detail: Detail) -> FloraMesh {
    let mut t = Builder::new(Species::Pine);
    let near = detail == Detail::Near;
    let trunk = t.branch(Vec3::ZERO, Vec3::new(0.08, 1.0, 0.03), 0.84, 0.022, 0.0, 0.06, 6);
    t.tube(&trunk, if near { 6 } else { 4 });
    let (branches, per_clump, size) = if near { (9, 7, 0.14) } else { (6, 3, 0.2) };
    for k in 0..branches {
        let at = trunk[2 + k % (trunk.len() - 2)];
        let a = k as f32 * 2.4 + t.rng.range(-0.3, 0.3);
        let dir = Vec3::new(a.cos(), t.rng.range(0.25, 0.8), a.sin());
        let length = t.rng.range(0.16, 0.3) * (1.1 - at.0.y * 0.5);
        let limb = t.branch(at.0, dir, length, at.1 * 0.5, 0.3, 0.12, 3);
        t.tube(&limb, if near { 4 } else { 3 });
        let end = limb[limb.len() - 1].0;
        for _ in 0..per_clump {
            let at = end + t.rng.direction(0.5) * 0.06;
            let up = (t.volume_normal(at) + Vec3::Y * 0.6 + t.rng.direction(1.0) * 0.3).normalize();
            t.cluster(at, up, size, 2);
        }
    }
    let top = trunk[trunk.len() - 1].0;
    for _ in 0..if near { 6 } else { 2 } {
        let at = top + t.rng.direction(0.2) * 0.05;
        let up = (Vec3::Y + t.rng.direction(1.0) * 0.4).normalize();
        t.cluster(at, up, size, 2);
    }
    t.finish()
}

/// An umbrella pine: a bare trunk forking into limbs that hold a flat dome of needles high up.
fn stone_pine(detail: Detail) -> FloraMesh {
    let mut t = Builder::new(Species::StonePine);
    let near = detail == Detail::Near;
    let trunk = t.branch(Vec3::ZERO, Vec3::new(0.1, 1.0, 0.04), 0.6, 0.026, 0.0, 0.05, 4);
    t.tube(&trunk, if near { 7 } else { 4 });
    let top = trunk[trunk.len() - 1].0;
    for k in 0..if near { 5 } else { 3 } {
        let a = k as f32 / 5.0 * TAU + t.rng.range(0.0, 0.8);
        let aim = Vec3::new(a.cos() * t.radii.x * 0.6, t.centre.y - 0.03, a.sin() * t.radii.z * 0.6);
        let limb = t.branch(top, aim - top, (aim - top).length(), 0.016, 0.1, 0.08, 3);
        t.tube(&limb, if near { 4 } else { 3 });
    }
    // The dome: over its top and round its rim, the cards facing up and out.
    let (count, size) = if near { (110, 0.15) } else { (34, 0.24) };
    for k in 0..count {
        // A fifth over the flat top, where few directions of the sphere fall.
        let d = if k % 5 == 0 {
            let (r, a) = (t.rng.next().sqrt() * 0.7, t.rng.range(0.0, TAU));
            Vec3::new(r * a.cos(), 0.8, r * a.sin()).normalize()
        } else {
            t.rng.direction(0.35)
        };
        let at = t.centre + d * t.radii * t.rng.range(0.7, 1.0);
        let up = (d * Vec3::new(1.0, 0.6, 1.0) + Vec3::Y * 0.9).normalize();
        t.cluster(at, up, size, 2);
    }
    t.finish()
}

/// A palm: a slender ringed trunk, curving a little, and a crown of arching fronds, the old ones
/// hanging dry under it.
fn palm(detail: Detail) -> FloraMesh {
    let mut t = Builder::new(Species::Palm);
    let near = detail == Detail::Near;
    let trunk = t.branch(Vec3::ZERO, Vec3::new(0.05, 1.0, 0.0), 0.83, 0.028, 0.02, 0.025, 6);
    let mut path = trunk.clone();
    for (k, p) in path.iter_mut().enumerate() {
        p.1 = 0.028 - 0.006 * k as f32 / 6.0 + if k == 0 { 0.012 } else { 0.0 };
    }
    t.tube(&path, if near { 8 } else { 5 });
    let top = trunk[trunk.len() - 1].0;
    let fronds = if near { 20 } else { 10 };
    let segments = if near { 5 } else { 3 };
    for k in 0..fronds + if near { 5 } else { 2 } {
        let dead = k >= fronds;
        let a = k as f32 * 2.399 + t.rng.range(-0.2, 0.2);
        let mut elevation = if dead { -1.25 } else { t.rng.range(-0.25, 1.15) };
        let length = if dead { 0.2 } else { t.rng.range(0.3, 0.36) };
        let colour = if dead { Vec3::new(0.10, 0.075, 0.04) } else { t.leaf_colour() };
        let phase = t.rng.next();
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let side = Vec3::new(-a.sin(), 0.0, a.cos());
        let mut p = top;
        let first = t.mesh.vertices() as u32;
        for s in 0..=segments {
            let v = s as f32 / segments as f32;
            let along = out * elevation.cos() + Vec3::Y * elevation.sin();
            let lift = along.cross(side).normalize();
            let lift = if lift.y < 0.0 { -lift } else { lift };
            // The leaflets rise from the rachis in a V, narrower towards the tip.
            let half = 0.065 * (1.0 - 0.6 * v) + 0.01;
            for u in [-1.0f32, 0.0, 1.0] {
                let q = p + side * (u * half * 0.8) + lift * (u.abs() * half * 0.55);
                let vertex = Vertex {
                    p: q,
                    n: t.volume_normal(q),
                    colour,
                    open: t.openness(q),
                    uv: Vec2::new(0.5 + u * 0.5, 1.0 - v),
                    layer: Layer::Palm as u32 as f32,
                    sway: t.sway(q),
                    crown: t.in_crown(q),
                    phase,
                };
                t.mesh.push(vertex);
            }
            p += along * (length / segments as f32);
            // Each frond arches down towards its tip.
            elevation -= if dead { 0.05 } else { 0.32 };
        }
        for s in 0..segments as u32 {
            for u in 0..2u32 {
                let (a, b, c, d) = (first + s * 3 + u, first + s * 3 + u + 1, first + (s + 1) * 3 + u, first + (s + 1) * 3 + u + 1);
                t.mesh.indices.extend_from_slice(&[a, c, d, a, d, b]);
            }
        }
    }
    t.finish()
}

/// A cypress: a dark flame of scale-leaved sprays from the foot to the point.
fn cypress(detail: Detail) -> FloraMesh {
    let mut t = Builder::new(Species::Cypress);
    let near = detail == Detail::Near;
    t.reach = if near { 0.95 } else { 0.8 };
    t.tube(&[(Vec3::ZERO, 0.02), (Vec3::Y * 0.9, 0.006)], if near { 5 } else { 3 });
    let (count, size) = if near { (80, 0.13) } else { (26, 0.2) };
    for _ in 0..count {
        let y = t.rng.range(0.06, 0.95);
        // Widest a third of the way up, to a point at the top.
        let profile = ((y - 0.02) / 0.35).min(1.0).powf(0.5) * (1.0 - ((y - 0.37) / 0.63).max(0.0).powf(1.4));
        let a = t.rng.range(0.0, TAU);
        let r = t.radii.x * profile * t.rng.range(0.4, 1.0);
        let at = Vec3::new(a.cos() * r, y, a.sin() * r);
        t.cluster(at, Vec3::new(a.cos(), 0.35, a.sin()), size, 2);
    }
    t.finish()
}

/// A Lombardy poplar: a column of upturned branches.
fn poplar(detail: Detail) -> FloraMesh {
    let mut t = Builder::new(Species::Poplar);
    let near = detail == Detail::Near;
    t.reach = if near { 0.9 } else { 0.75 };
    t.tube(&[(Vec3::ZERO, 0.024), (Vec3::Y * 0.05, 0.018), (Vec3::Y * 0.92, 0.005)], if near { 6 } else { 4 });
    if near {
        for k in 0..10 {
            let y = 0.15 + 0.07 * k as f32;
            let a = k as f32 * 2.4;
            let branch = t.branch(Vec3::Y * y, Vec3::new(a.cos() * 0.4, 1.0, a.sin() * 0.4), 0.22, 0.007, 0.0, 0.05, 2);
            t.tube(&branch, 3);
        }
    }
    let (count, size) = if near { (80, 0.12) } else { (26, 0.19) };
    for _ in 0..count {
        let d = t.rng.direction(1.0);
        let at = t.centre + d * t.radii * t.rng.range(0.5, 1.0);
        t.cluster(at, d + Vec3::Y * 0.2, size, 2);
    }
    t.finish()
}

/// The mesh of a species at a level of detail.
pub fn tree(species: Species, detail: Detail) -> FloraMesh {
    let b = |fork, trunk, limbs, twigs, gnarl, clusters, size, gaps| Broad { fork, trunk, lean: Vec3::ZERO, limbs, twigs, gnarl, clusters, size, gaps, stems: false };
    match species {
        Species::Oak => broad(species, detail, Broad { lean: Vec3::new(0.05, 0.0, 0.02), ..b(0.33, 0.026, 5, 3, 0.22, 85, 0.17, 2) }),
        Species::Beech => broad(species, detail, b(0.32, 0.022, 5, 3, 0.08, 95, 0.16, 0)),
        Species::Ash => broad(species, detail, b(0.42, 0.02, 4, 3, 0.12, 60, 0.19, 3)),
        Species::HolmOak => broad(species, detail, b(0.27, 0.03, 5, 3, 0.18, 105, 0.15, 0)),
        Species::Olive => broad(species, detail, Broad { lean: Vec3::new(0.12, 0.0, 0.05), ..b(0.26, 0.045, 3, 3, 0.3, 80, 0.15, 3) }),
        Species::Maquis => broad(species, detail, Broad { stems: true, ..b(0.0, 0.012, 4, 0, 0.25, 55, 0.24, 0) }),
        Species::Shrub => broad(species, detail, Broad { stems: true, ..b(0.0, 0.012, 4, 0, 0.25, 50, 0.25, 1) }),
        Species::Poplar => poplar(detail),
        Species::Cypress => cypress(detail),
        Species::Spruce => tiered(species, detail, Tiered { tiers: 14, branches: 7, low: 0.14, reach: 0.18, droop: 0.4, vee: 1.6, width: 0.9 }),
        Species::Fir => tiered(species, detail, Tiered { tiers: 12, branches: 7, low: 0.12, reach: 0.22, droop: 0.12, vee: 1.2, width: 0.9 }),
        Species::Larch => tiered(species, detail, Tiered { tiers: 11, branches: 6, low: 0.14, reach: 0.2, droop: 0.05, vee: 1.0, width: 1.0 }),
        Species::StonePine => stone_pine(detail),
        Species::Pine => pine(detail),
        Species::Palm => palm(detail),
    }
}

/// The names of the species for tools (`flight`'s `models` example), in the order of `Species::ALL`.
pub const NAMES: [&str; crate::species::COUNT] = ["oak", "beech", "ash", "holm-oak", "olive", "poplar", "cypress", "spruce", "fir", "larch", "stone-pine", "pine", "palm", "maquis", "shrub"];

/// The mesh of a species by its name in `NAMES`, `far-` before it for the simpler one.
pub fn named(name: &str) -> Option<FloraMesh> {
    let (detail, name) = match name.strip_prefix("far-") {
        Some(rest) => (Detail::Far, rest),
        None => (Detail::Near, name),
    };
    NAMES.iter().position(|n| *n == name).map(|k| tree(Species::ALL[k], detail))
}

impl FloraMesh {
    /// As a plain mesh of `MeshBuilder`'s vertices (position, normal, colour), for a viewer that
    /// knows none of the pictures: each card a solid square of its leaves' colour, the bark its tint.
    pub fn plain(&self) -> MeshBuilder {
        let mut b = MeshBuilder::new();
        for &i in &self.indices {
            let v = &self.data[i as usize * FLOATS..(i as usize + 1) * FLOATS];
            b.vertex(Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]), Vec4::new(v[6], v[7], v[8], 1.0) * if v[12] >= BARK { 0.3 } else { 6.0 });
        }
        b
    }
}

/// A tuft of grass (`grass.rs`): three cards of the grass picture crossed round its middle, 1 m
/// tall, lit as the ground under it is (their normals up), darker at the foot.
pub fn tuft() -> FloraMesh {
    let mut mesh = FloraMesh::default();
    for k in 0..3 {
        let a = k as f32 * PI / 3.0;
        let across = Vec3::new(a.cos(), 0.0, a.sin());
        let first = mesh.vertices() as u32;
        for (u, v) in [(-0.5f32, 0.0f32), (0.5, 0.0), (-0.5, 1.0), (0.5, 1.0)] {
            let p = across * (u * 1.4) + Vec3::Y * v;
            let n = (Vec3::Y + across.cross(Vec3::Y) * 0.25).normalize();
            mesh.push(Vertex {
                p,
                n,
                colour: Vec3::ONE,
                open: 0.45 + 0.55 * v,
                uv: Vec2::new(u + 0.5, 1.0 - v),
                layer: Layer::Grass as u32 as f32,
                sway: v,
                crown: Vec3::new(0.0, 5.0, 0.0),
                phase: k as f32 / 3.0,
            });
        }
        mesh.indices.extend_from_slice(&[first, first + 2, first + 3, first, first + 3, first + 1]);
    }
    mesh
}

/// A square on the ground, one metre from centre to edge: what a tree's shadow and its picture far
/// away (`impostors.rs`) are drawn on.
pub fn square() -> MeshBuilder {
    let mut b = MeshBuilder::new();
    let c = Vec4::ZERO;
    b.quad(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(-1.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 1.0), Vec3::new(1.0, 0.0, -1.0), c);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::species::COUNT;
    use sim_core::scenery::CROWNS;

    fn extent(mesh: &FloraMesh) -> (f32, f32, f32) {
        let (mut low, mut high, mut wide) = (f32::INFINITY, f32::NEG_INFINITY, 0f32);
        for v in mesh.data.chunks(FLOATS) {
            low = low.min(v[1]);
            high = high.max(v[1]);
            wide = wide.max(v[0].hypot(v[2]));
        }
        (low, high, wide)
    }

    #[test]
    fn trees_are_unit_models_that_fit_what_the_simulation_runs_into() {
        for species in Species::ALL {
            for detail in [Detail::Near, Detail::Far] {
                let mesh = tree(species, detail);
                assert!(mesh.triangles() > 0, "{species:?}");
                assert!(mesh.data.iter().all(|x| x.is_finite()), "{species:?} has a non-finite value");
                assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.vertices()));
                let (low, high, wide) = extent(&mesh);
                assert!(low > -0.12 && low < 0.1, "{species:?} starts at {low}");
                assert!(high > 0.85 && high < 1.12, "{species:?} {detail:?} ends at {high}");
                // As wide as the crown the simulation runs into, give or take the leaves at its edge.
                let (_, radius, _) = CROWNS[species.shape() as usize];
                assert!(wide > radius as f32 * 0.6 && wide < radius as f32 * 1.45, "{species:?} {detail:?} is {wide} wide, its crown {radius}");
                for v in mesh.data.chunks(FLOATS) {
                    assert!((Vec3::new(v[3], v[4], v[5]).length() - 1.0).abs() < 1e-3, "{species:?}: a normal is not unit");
                    assert!((0.0..=1.0).contains(&v[9]), "{species:?} openness {}", v[9]);
                    assert!(v[12] < crate::foliage::LAYERS as f32 || v[12] >= BARK, "{species:?} layer {}", v[12]);
                }
            }
        }
        assert_eq!(Species::ALL.len(), COUNT);
    }

    #[test]
    fn the_far_trees_are_cheaper() {
        for species in Species::ALL {
            let (near, far) = (tree(species, Detail::Near).triangles(), tree(species, Detail::Far).triangles());
            assert!(far * 2 <= near, "{species:?}: {far} triangles far, {near} near");
            assert!(near <= 1000, "{species:?}: {near} triangles");
        }
    }

    #[test]
    fn a_species_is_the_same_tree_every_time() {
        for species in Species::ALL {
            assert_eq!(tree(species, Detail::Near).data, tree(species, Detail::Near).data);
        }
    }
}
