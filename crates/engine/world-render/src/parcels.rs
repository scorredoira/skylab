//! A real place's ground cut into its fields, woods and scrub, from the satellites' picture of it
//! (`sim_core::Imagery`), so that close up the ground is drawn sharp where the picture is soft.
//!
//! Seen from 300 m a texel of 10 m is tens of pixels wide: the picture is a blur of soft patches.
//! What it shows is still right: where one field ends and the next begins, which is green, which
//! ploughed, where the trees stand. So the picture is cut into the parcels it shows (superpixels
//! by colour and place, SLIC after Achanta et al. 2012, then neighbours alike merged), and each
//! parcel is told by its colour, its texture and the maps' cover what it is (`Kind`). The ground's
//! shader then draws the parcel's own colour up to its edge, sharp at any size, with what its
//! kind has (crop rows along the parcel, furrows, stubble, scrub, stones), a boundary along the
//! edge (a dry-stone wall, a hedge, a track, or none) and the trees where the picture is dark and
//! green (`trees`, drawn but not run into: the simulation never reads the picture).
//!
//! The edge is drawn from a distance field: each texel keeps how far it is from the edge of its
//! own parcel, and a point between four texels is in the parcel whose signed distance (plus inside
//! it, minus in the others) interpolates highest, the edge where two are equal. Interpolated, a
//! distance to a straight line is that line exactly: the edges are straight between texels, not
//! steps of 10 m. `sample` does what the shader's `ground_parcel` does, for the trees and the tests.

use crate::species::{self, Climate, Species};
use glam::Vec2;
use sim_core::scenery::{Instances, Tree};
use sim_core::{Cover, Grid, Region, Terrain};

/// What a parcel of ground is (`P_*` in shaders.wgsl, in this order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Not drawn as parcels: sea, lakes, towns, sand, snow, or what the picture does not know.
    None,
    /// A green crop, in rows.
    Crop,
    /// A pale field, harvested: stubble in rows.
    Stubble,
    /// Bare brown soil: ploughed, in furrows.
    Ploughed,
    /// Dry grass, pasture.
    DryGrass,
    /// Green grass, a meadow.
    Meadow,
    /// Scrub: bushes and low trees on dry ground.
    Scrub,
    /// Woods: trees close together.
    Wood,
    /// Bare rock and stony ground.
    Rock,
}

impl Kind {
    const ALL: [Kind; 9] = [Kind::None, Kind::Crop, Kind::Stubble, Kind::Ploughed, Kind::DryGrass, Kind::Meadow, Kind::Scrub, Kind::Wood, Kind::Rock];

    pub fn from_byte(b: u8) -> Kind {
        Kind::ALL.get(b as usize).copied().unwrap_or(Kind::None)
    }
}

/// Distances to an edge are kept up to this many texels, as a byte that is the square root of
/// the share of it (`distance_byte`): finest near the edge, where the walls stand (some 2 cm a
/// step within a texel of it, at 10 m a texel), so a wall's sides stay straight.
const DISTANCE_MOST: f32 = 255.0 / 32.0;

/// A distance to an edge, texels, as the parcels' map keeps it.
fn distance_byte(d: f32) -> u8 {
    ((d / DISTANCE_MOST).clamp(0.0, 1.0).sqrt() * 255.0).round() as u8
}

/// The distance, texels, a byte of the parcels' map stands for (`distance_byte`).
fn byte_distance(b: u8) -> f64 {
    let s = b as f64 / 255.0;
    s * s * DISTANCE_MOST as f64
}
/// Superpixels are this many metres apart, at least 3 texels and at most 8.
const SUPERPIXEL: f64 = 50.0;
/// How much the place counts against the colour in a superpixel (SLIC's `m`): a texel a superpixel
/// away weighs as a difference of this much in sRGB units (0 to 255).
const COMPACT: f32 = 22.0;
const ITERATIONS: usize = 6;
/// Neighbouring parcels closer in colour than this (sRGB units) are one, up to `MOST_AREA`.
const ALIKE: f32 = 10.0;
/// Texels nearer than this in colour (sRGB units) are smoothed together before the picture is cut.
const GRAIN: f32 = 24.0;
/// The smallest a parcel stands on its own, square metres; smaller ones and slivers (a line of trees
/// along a wall, two texels wide at most) join the neighbour most alike.
const LEAST_AREA: f64 = 1500.0;
/// The largest a parcel is merged to, square metres: a field seldom passes it.
const MOST_AREA: f64 = 30_000.0;

/// The parcels of one grid of a picture, as the GPU takes them: `side` texels a side, rows from
/// the north, the grid in the north-west corner (as `imagery::prepare` lays the picture).
pub struct Parcels {
    pub side: usize,
    /// Per texel: the parcel's number (1 to 255, never the same as a neighbour's; 0 = not drawn as
    /// parcels), the distance to its edge (`distance_byte`), the heading of its rows
    /// (0 to 255 for 0 to π, from east towards south), its `Kind`.
    pub map: Vec<[u8; 4]>,
    /// Per texel: the parcel's colour (sRGB, the mean of its texels in linear light), and how much
    /// the texel itself looks like the crowns of trees (0 to 255).
    pub tone: Vec<[u8; 4]>,
    /// The north-west texel's middle (north, east) and the metres between texels.
    north: f64,
    west: f64,
    pub step: f64,
}

/// What `Parcels::sample` finds at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub kind: Kind,
    /// The parcel's number, and the one across the nearest edge (the same when no other is near).
    pub id: u8,
    pub other: u8,
    pub other_kind: Kind,
    /// Metres to the nearest edge, and the way from it into the parcel (north, east), unit.
    pub edge: f64,
    pub inward: (f64, f64),
    /// How much the texel nearest looks like the crowns of trees, 0 to 1.
    pub tree: f64,
    /// The parcel's colour, sRGB.
    pub colour: [u8; 3],
    /// The colour of the parcel across the edge (its own when none is near).
    pub other_colour: [u8; 3],
    /// The heading of its rows, radians from east towards south (0 to π).
    pub heading: f64,
}

/// sRGB's decoding of a byte, as a table.
pub fn linear_table() -> [f32; 256] {
    std::array::from_fn(|i| {
        let s = i as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn encode(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// `ihash` of shaders.wgsl, bit for bit.
pub fn ihash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// The texels of a grid with what is known of each (sRGB and alpha), rows from the south.
struct Image {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

impl Image {
    /// A grid's picture with its grain smoothed away and its edges kept: each texel the mean of
    /// those round it (3 by 3) that are near it in colour, twice over.
    fn smoothed(grid: &Grid<[u8; 4]>) -> Image {
        let (w, h) = (grid.cols, grid.rows);
        let mut img = Image { w, h, px: grid.data.clone() };
        let mut out = img.px.clone();
        for _ in 0..2 {
            for y in 1..h.saturating_sub(1) {
                for x in 1..w - 1 {
                    let i = y * w + x;
                    if !img.known(i) {
                        continue;
                    }
                    let c = img.rgb(i);
                    let mut sum = [0f32; 4];
                    for j in [i - w - 1, i - w, i - w + 1, i - 1, i, i + 1, i + w - 1, i + w, i + w + 1] {
                        let o = img.rgb(j);
                        if img.known(j) && d2(o, c) < GRAIN * GRAIN {
                            sum = [sum[0] + o[0], sum[1] + o[1], sum[2] + o[2], sum[3] + 1.0];
                        }
                    }
                    out[i] = [(sum[0] / sum[3]).round() as u8, (sum[1] / sum[3]).round() as u8, (sum[2] / sum[3]).round() as u8, 255];
                }
            }
            std::mem::swap(&mut img.px, &mut out);
            out.copy_from_slice(&img.px);
        }
        img
    }

    fn known(&self, i: usize) -> bool {
        self.px[i][3] > 127
    }

    fn rgb(&self, i: usize) -> [f32; 3] {
        let p = self.px[i];
        [p[0] as f32, p[1] as f32, p[2] as f32]
    }
}

fn d2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Superpixels: every known texel to the nearest centre in colour and place, the centres moved to
/// the middles of what they got, `ITERATIONS` times. `u32::MAX` where nothing is known.
fn superpixels(img: &Image, s: usize) -> Vec<u32> {
    struct Centre {
        x: f32,
        y: f32,
        c: [f32; 3],
    }
    let n = img.w * img.h;
    let mut centres = Vec::new();
    for y in (s / 2..img.h).step_by(s) {
        for x in (s / 2..img.w).step_by(s) {
            let i = y * img.w + x;
            if img.known(i) {
                centres.push(Centre { x: x as f32, y: y as f32, c: img.rgb(i) });
            }
        }
    }
    let mut label = vec![u32::MAX; n];
    let mut best = vec![f32::INFINITY; n];
    let weight = (COMPACT / s as f32).powi(2);
    for _ in 0..ITERATIONS {
        best.fill(f32::INFINITY);
        for (k, c) in centres.iter().enumerate() {
            let (cx, cy) = (c.x.round() as isize, c.y.round() as isize);
            let reach = s as isize;
            let (x0, x1) = ((cx - reach).max(0) as usize, ((cx + reach) as usize).min(img.w - 1));
            let (y0, y1) = ((cy - reach).max(0) as usize, ((cy + reach) as usize).min(img.h - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = y * img.w + x;
                    if !img.known(i) {
                        continue;
                    }
                    let d = d2(img.rgb(i), c.c) + ((x as f32 - c.x).powi(2) + (y as f32 - c.y).powi(2)) * weight;
                    if d < best[i] {
                        best[i] = d;
                        label[i] = k as u32;
                    }
                }
            }
        }
        let mut sums = vec![[0f64; 6]; centres.len()];
        for (i, &l) in label.iter().enumerate() {
            if l == u32::MAX {
                continue;
            }
            let c = img.rgb(i);
            let s = &mut sums[l as usize];
            *s = [s[0] + (i % img.w) as f64, s[1] + (i / img.w) as f64, s[2] + c[0] as f64, s[3] + c[1] as f64, s[4] + c[2] as f64, s[5] + 1.0];
        }
        for (c, s) in centres.iter_mut().zip(&sums) {
            if s[5] > 0.0 {
                *c = Centre { x: (s[0] / s[5]) as f32, y: (s[1] / s[5]) as f32, c: [(s[2] / s[5]) as f32, (s[3] / s[5]) as f32, (s[4] / s[5]) as f32] };
            }
        }
    }
    label
}

/// Each connected piece of a superpixel its own segment (a texel far in colour from where the piece
/// began, that the superpixel took for want of a nearer one, starts a piece of its own); a piece smaller than `least` texels joins
/// the segment already made beside it that is most alike in colour, if one is alike enough (a
/// clump of trees or a roof stays a parcel of its own). Returns the segments and how many there are.
fn connected(img: &Image, label: &[u32], least: usize) -> (Vec<u32>, usize) {
    let (w, h) = (img.w, img.h);
    let mut seg = vec![u32::MAX; label.len()];
    // Per segment: the sums of its colour and its texels.
    let mut sums: Vec<[f32; 4]> = Vec::new();
    let (mut stack, mut piece, mut beside) = (Vec::new(), Vec::new(), Vec::new());
    for start in 0..label.len() {
        if seg[start] != u32::MAX || label[start] == u32::MAX {
            continue;
        }
        let count = sums.len() as u32;
        piece.clear();
        beside.clear();
        stack.push(start);
        seg[start] = count;
        let (first, mut sum) = (img.rgb(start), [0f32; 4]);
        while let Some(i) = stack.pop() {
            piece.push(i);
            let c = img.rgb(i);
            sum = [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2], sum[3] + 1.0];
            let (x, y) = (i % w, i / w);
            let around = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)];
            for j in around.into_iter().flatten() {
                if seg[j] == u32::MAX && label[j] == label[start] && d2(img.rgb(j), first) < (4.0 * ALIKE).powi(2) {
                    seg[j] = count;
                    stack.push(j);
                } else if seg[j] != u32::MAX && seg[j] != count && !beside.contains(&seg[j]) {
                    beside.push(seg[j]);
                }
            }
        }
        let mean = |s: [f32; 4]| [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
        let join = beside.iter().map(|&b| (b, d2(mean(sums[b as usize]), mean(sum)))).filter(|b| b.1 < (3.0 * ALIKE).powi(2)).min_by(|a, b| a.1.total_cmp(&b.1));
        match join {
            Some((other, _)) if piece.len() < least => {
                piece.iter().for_each(|&i| seg[i] = other);
                let s = &mut sums[other as usize];
                *s = [s[0] + sum[0], s[1] + sum[1], s[2] + sum[2], s[3] + sum[3]];
            }
            _ => sums.push(sum),
        }
    }
    (seg, sums.len())
}

fn root(parent: &mut [u32], mut a: u32) -> u32 {
    while parent[a as usize] != a {
        parent[a as usize] = parent[parent[a as usize] as usize];
        a = parent[a as usize];
    }
    a
}

/// Neighbouring segments alike in colour joined, the most alike first, none past `most` texels.
fn merged(img: &Image, seg: &mut [u32], count: usize, most: f64) {
    let mut sum = vec![[0f64; 4]; count];
    for (i, &s) in seg.iter().enumerate() {
        if s != u32::MAX {
            let c = img.rgb(i);
            let t = &mut sum[s as usize];
            *t = [t[0] + c[0] as f64, t[1] + c[1] as f64, t[2] + c[2] as f64, t[3] + 1.0];
        }
    }
    let mean = |t: &[f64; 4]| [(t[0] / t[3]) as f32, (t[1] / t[3]) as f32, (t[2] / t[3]) as f32];
    let mut pairs = neighbours(img.w, seg, false);
    pairs.sort_by_cached_key(|&(a, b)| (d2(mean(&sum[a as usize]), mean(&sum[b as usize])) * 16.0) as u32);
    let mut parent: Vec<u32> = (0..count as u32).collect();
    for (a, b) in pairs {
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        if ra == rb || sum[ra as usize][3] + sum[rb as usize][3] > most {
            continue;
        }
        if d2(mean(&sum[ra as usize]), mean(&sum[rb as usize])) > ALIKE * ALIKE {
            continue;
        }
        parent[rb as usize] = ra;
        let t = sum[rb as usize];
        let u = &mut sum[ra as usize];
        *u = [u[0] + t[0], u[1] + t[1], u[2] + t[2], u[3] + t[3]];
    }
    for s in seg.iter_mut().filter(|s| **s != u32::MAX) {
        *s = root(&mut parent, *s);
    }
}

/// Segments too small to stand alone (under `least` texels) and slivers (no texel two from their
/// edge: a line of trees along a wall, a strip of a track) joined to the neighbour most alike in
/// colour; twice, as what is joined makes others.
fn dissolve(img: &Image, seg: &mut [u32], count: usize, least: usize) {
    let (w, h) = (img.w, img.h);
    for _ in 0..2 {
        let raw = distance_field(w, h, &edges(w, h, seg));
        let (mut sum, mut widest) = (vec![[0f32; 4]; count], vec![0f32; count]);
        for (i, &s) in seg.iter().enumerate() {
            if s != u32::MAX {
                let (c, t) = (img.rgb(i), &mut sum[s as usize]);
                *t = [t[0] + c[0], t[1] + c[1], t[2] + c[2], t[3] + 1.0];
                widest[s as usize] = widest[s as usize].max(raw[i]);
            }
        }
        let mean = |t: [f32; 4]| [t[0] / t[3], t[1] / t[3], t[2] / t[3]];
        let mut alike = vec![(f32::INFINITY, u32::MAX); count];
        for (a, b) in neighbours(w, seg, false) {
            let d = d2(mean(sum[a as usize]), mean(sum[b as usize]));
            for (p, q) in [(a, b), (b, a)] {
                if d < alike[p as usize].0 {
                    alike[p as usize] = (d, q);
                }
            }
        }
        let mut parent: Vec<u32> = (0..count as u32).collect();
        let mut joined = false;
        for s in 0..count {
            let (n, to) = (sum[s][3], alike[s].1);
            if n == 0.0 || to == u32::MAX || ((n as usize) >= least && widest[s] >= 1.0) {
                continue;
            }
            let (a, b) = (root(&mut parent, s as u32), root(&mut parent, to));
            if a != b {
                parent[a as usize] = b;
                joined = true;
            }
        }
        if !joined {
            break;
        }
        for s in seg.iter_mut().filter(|s| **s != u32::MAX) {
            *s = root(&mut parent, *s);
        }
    }
}

/// The texels on the edge of their segment: one of their four neighbours is in another.
fn edges(w: usize, h: usize, seg: &[u32]) -> Vec<bool> {
    (0..seg.len())
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let other = |j: usize| seg[j] != seg[i];
            (x > 0 && other(i - 1)) || (x + 1 < w && other(i + 1)) || (y > 0 && other(i - w)) || (y + 1 < h && other(i + w))
        })
        .collect()
}

/// The pairs of different segments that touch (`diagonal`: at a corner too), each once, lower first.
fn neighbours(w: usize, seg: &[u32], diagonal: bool) -> Vec<(u32, u32)> {
    let h = seg.len() / w;
    let mut pairs = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let a = seg[y * w + x];
            if a == u32::MAX {
                continue;
            }
            let mut add = |b: u32| {
                if b != u32::MAX && b != a && pairs.last() != Some(&(a.min(b), a.max(b))) {
                    pairs.push((a.min(b), a.max(b)));
                }
            };
            if x + 1 < w {
                add(seg[y * w + x + 1]);
            }
            if y + 1 < h {
                add(seg[(y + 1) * w + x]);
                if diagonal && x + 1 < w {
                    add(seg[(y + 1) * w + x + 1]);
                }
                if diagonal && x > 0 {
                    add(seg[(y + 1) * w + x - 1]);
                }
            }
        }
        // Kept to the pairs seen so far, not every texel's.
        if pairs.len() > 1 << 22 {
            pairs.sort_unstable();
            pairs.dedup();
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// How far a straightened edge may stray from the steps between the texels it follows, texels.
const TOLERANCE: f64 = 0.8;
/// Texels from an edge within which their distance to it is measured; beyond, they are this far.
const REACH: f32 = 6.0;

/// A run of the edge between two segments `a` and `b`, from one meeting of three or more (or the
/// picture's border) to the next, or round a closed loop: the corners of texels it passes, `a` on
/// its left (the positive side of `cross`).
struct Chain {
    points: Vec<(f64, f64)>,
    a: u32,
    b: u32,
}

fn cross(o: (f64, f64), p: (f64, f64), q: (f64, f64)) -> f64 {
    (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0)
}

/// A chain's points each the mean of the seven round it along the chain (round a loop when `closed`),
/// the first and last kept where they are: the steps between texels become the line they step along.
fn smoothed(points: &[(f64, f64)], closed: bool) -> Vec<(f64, f64)> {
    const HALF: isize = 3;
    let n = points.len() as isize;
    (0..n)
        .map(|i| {
            if !closed && (i == 0 || i == n - 1) {
                return points[i as usize];
            }
            let (mut x, mut y, mut k) = (0.0, 0.0, 0.0);
            for j in i - HALF..=i + HALF {
                // A loop's last point is its first.
                let j = if closed { j.rem_euclid(n - 1) } else { j.clamp(0, n - 1) };
                x += points[j as usize].0;
                y += points[j as usize].1;
                k += 1.0;
            }
            (x / k, y / k)
        })
        .collect()
}

/// The fewest of `points` (the first and the last kept) whose lines stay within `TOLERANCE` of the
/// rest (Douglas and Peucker 1973).
fn simplified(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        let (p, q) = (points[i], points[j]);
        let len = (q.0 - p.0).hypot(q.1 - p.1).max(1e-9);
        let far = (i + 1..j).map(|k| (k, cross(p, q, points[k]).abs() / len)).max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = far.filter(|f| f.1 > TOLERANCE) {
            keep[k] = true;
            stack.push((i, k));
            stack.push((k, j));
        }
    }
    points.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

/// The edges between the segments followed into chains (`Chain`): along the cracks between texels
/// of two segments, corner to corner, from each corner where three or more cracks meet (or one,
/// at the border), and then round what is left (the loops round a segment within another); each
/// given to `each` as it is found.
fn chains(w: usize, h: usize, seg: &[u32], mut each: impl FnMut(&Chain)) {
    // Cracks: `w · h` vertical ones (the west side of texel x, y, for x from 1), then as many
    // horizontal (the south side of texel x, y, for y from 1); corners are (x, y), 0..=w, 0..=h.
    let vertical = |x: usize, y: usize| x >= 1 && x < w && y < h && seg[y * w + x - 1] != seg[y * w + x];
    let horizontal = |x: usize, y: usize| y >= 1 && y < h && x < w && seg[(y - 1) * w + x] != seg[y * w + x];
    // The cracks at a corner: (crack, the corner at its other end).
    let around = |x: usize, y: usize| {
        let mut out = [(usize::MAX, (0, 0)); 4];
        let mut n = 0;
        if y >= 1 && vertical(x, y - 1) {
            out[n] = ((y - 1) * w + x, (x, y - 1));
            n += 1;
        }
        if vertical(x, y) {
            out[n] = (y * w + x, (x, y + 1));
            n += 1;
        }
        if x >= 1 && horizontal(x - 1, y) {
            out[n] = (w * h + y * w + x - 1, (x - 1, y));
            n += 1;
        }
        if horizontal(x, y) {
            out[n] = (w * h + y * w + x, (x + 1, y));
            n += 1;
        }
        (out, n)
    };
    // The segments either side of a crack, the one on the left of going from `from` along it first.
    let sides = |crack: usize, from: (usize, usize), to: (usize, usize)| {
        let (west_or_south, east_or_north) = if crack < w * h {
            let (x, y) = (crack % w, crack / w);
            (seg[y * w + x - 1], seg[y * w + x])
        } else {
            let (x, y) = ((crack - w * h) % w, (crack - w * h) / w);
            (seg[(y - 1) * w + x], seg[y * w + x])
        };
        // Left of a vertical crack going north is its west side; of a horizontal one going east, its north side.
        let forward = if crack < w * h { to.1 > from.1 } else { to.0 > from.0 };
        let left_is_first = if crack < w * h { forward } else { !forward };
        if left_is_first { (west_or_south, east_or_north) } else { (east_or_north, west_or_south) }
    };
    let mut seen = vec![false; 2 * w * h];
    let walk = |start: (usize, usize), first: (usize, (usize, usize)), seen: &mut Vec<bool>| {
        let (a, b) = sides(first.0, start, first.1);
        let mut points = vec![(start.0 as f64, start.1 as f64)];
        let (mut crack, mut at) = first;
        loop {
            seen[crack] = true;
            points.push((at.0 as f64, at.1 as f64));
            let (next, n) = around(at.0, at.1);
            if n != 2 || at == start {
                break;
            }
            match next[..n].iter().find(|c| !seen[c.0]) {
                Some(&(c, to)) => (crack, at) = (c, to),
                None => break,
            }
        }
        Chain { points, a, b }
    };
    for y in 0..=h {
        for x in 0..=w {
            let (cracks, n) = around(x, y);
            if n == 2 || n == 0 {
                continue;
            }
            for &c in &cracks[..n] {
                if !seen[c.0] {
                    each(&walk((x, y), c, &mut seen));
                }
            }
        }
    }
    // What is left are loops: each from a corner of its first crack.
    for crack in 0..2 * w * h {
        let exists = if crack < w * h { vertical(crack % w, crack / w) } else { horizontal((crack - w * h) % w, (crack - w * h) / w) };
        if !exists || seen[crack] {
            continue;
        }
        let start = if crack < w * h { (crack % w, crack / w) } else { ((crack - w * h) % w, (crack - w * h) / w) };
        let to = if crack < w * h { (start.0, start.1 + 1) } else { (start.0 + 1, start.1) };
        each(&walk(start, (crack, to), &mut seen));
    }
}

/// How far each texel's middle is from the edge of its segment, in texels (`REACH` at most), with
/// the edges straightened: each chain of the edge (`chains`) cut to its fewest straight lines
/// (`simplified`; a loop at its first point and the point farthest from it), and each texel
/// measured to the nearest line of an edge of its own segment, signed. A texel that a straightened
/// edge leaves on its other side changes to the segment there.
fn straight_edges(w: usize, h: usize, seg: &mut [u32]) -> Vec<f32> {
    let mut best = vec![REACH; seg.len()];
    let mut own = vec![REACH; seg.len()];
    let mut across = vec![u32::MAX; seg.len()];
    chains(w, h, seg, |chain| {
        let closed = chain.points.len() > 2 && chain.points[0] == chain.points[chain.points.len() - 1];
        let p = &smoothed(&chain.points, closed);
        let lines = if closed {
            let far = (1..p.len() - 1).max_by(|&i, &j| (p[i].0 - p[0].0).hypot(p[i].1 - p[0].1).total_cmp(&(p[j].0 - p[0].0).hypot(p[j].1 - p[0].1))).unwrap_or(1);
            let mut l = simplified(&p[..=far]);
            l.extend_from_slice(&simplified(&p[far..])[1..]);
            l
        } else {
            simplified(p)
        };
        for pair in lines.windows(2) {
            let (s, e) = (pair[0], pair[1]);
            let (dx, dy) = (e.0 - s.0, e.1 - s.1);
            let len2 = (dx * dx + dy * dy).max(1e-12);
            let (x0, x1) = ((s.0.min(e.0) - REACH as f64).floor().max(0.0) as usize, ((s.0.max(e.0) + REACH as f64).ceil().max(0.0) as usize).min(w));
            let (y0, y1) = ((s.1.min(e.1) - REACH as f64).floor().max(0.0) as usize, ((s.1.max(e.1) + REACH as f64).ceil().max(0.0) as usize).min(h));
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = y * w + x;
                    let l = seg[i];
                    if l != chain.a && l != chain.b {
                        continue;
                    }
                    let c = (x as f64 + 0.5, y as f64 + 0.5);
                    let t = (((c.0 - s.0) * dx + (c.1 - s.1) * dy) / len2).clamp(0.0, 1.0);
                    let d = (c.0 - s.0 - dx * t).hypot(c.1 - s.1 - dy * t);
                    let d = d as f32;
                    if d < best[i] {
                        let left = cross(s, e, c) >= 0.0;
                        best[i] = d;
                        own[i] = if left == (l == chain.a) { d } else { -d };
                        across[i] = if l == chain.a { chain.b } else { chain.a };
                    }
                }
            }
        }
    });
    let mut dist = vec![0f32; seg.len()];
    for i in 0..seg.len() {
        let d = if own[i] < 0.0 && seg[i] != u32::MAX && across[i] != u32::MAX {
            seg[i] = across[i];
            -own[i]
        } else {
            own[i].abs()
        };
        dist[i] = d.clamp(0.01, DISTANCE_MOST);
    }
    dist
}

/// Squared distance to the nearest zero along a line of `f` (Felzenszwalb and Huttenlocher 2012):
/// the lower envelope of the parabolas rooted at each sample. `FAR` stands for no zero.
fn distance_line(f: &[f32], out: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let parabola = |q: usize, p: usize| ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * (q - p) as f32);
    let mut k = 0;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..f.len() {
        let mut s = parabola(q, v[k]);
        while s <= z[k] {
            k -= 1;
            s = parabola(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        *o = (q as f32 - v[k] as f32).powi(2) + f[v[k]];
    }
}

/// Stands for no zero in `distance_line`: far beyond any distance in a grid, finite.
const FAR: f32 = 1e20;

/// Every texel's distance in texels to the nearest texel of `seed`, exactly.
fn distance_field(w: usize, h: usize, seed: &[bool]) -> Vec<f32> {
    let n = w.max(h);
    let (mut f, mut out, mut v, mut z) = (vec![0f32; n], vec![0f32; n], vec![0usize; n], vec![0f32; n + 1]);
    let mut d: Vec<f32> = seed.iter().map(|&s| if s { 0.0 } else { FAR }).collect();
    for y in 0..h {
        f[..w].copy_from_slice(&d[y * w..(y + 1) * w]);
        distance_line(&f[..w], &mut out[..w], &mut v, &mut z);
        d[y * w..(y + 1) * w].copy_from_slice(&out[..w]);
    }
    for x in 0..w {
        for y in 0..h {
            f[y] = d[y * w + x];
        }
        distance_line(&f[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            d[y * w + x] = out[y].sqrt();
        }
    }
    d
}

/// How far round a runway the ground is the aerodrome's, metres along it and across it beyond its
/// ends and sides: wholly (its strip, as the ground's shader paints it), and where the maps say it is
/// grass (the maps give the aerodrome's ground inside its fence, its aprons, taxiways and
/// buildings, as mown grass).
const STRIP: (f64, f64) = (70.0, 45.0);
const AERODROME: (f64, f64) = (400.0, 700.0);

/// The texels of a grid (rows from the south) that are kept out of the parcels, so that no field,
/// wall, hedge or track is drawn there: the aerodromes, the buildings and towns, and what the maps
/// cover with water, sea, houses, a railway, sand or snow. Roads are not: walls run along them, and the
/// ground's shader draws none on them.
fn kept_out(grid: &Grid<[u8; 4]>, region: &Region) -> Vec<bool> {
    let (w, h) = (grid.cols, grid.rows);
    let mut out = vec![false; w * h];
    for (i, o) in out.iter_mut().enumerate() {
        let (n, e) = grid.node(i / w, i % w);
        *o = matches!(region.cover_at(n, e), Cover::Sea | Cover::Water | Cover::Built | Cover::Railway | Cover::Sand | Cover::Ice);
    }
    // Every texel within a box (south, west, north, east) for which `inside` holds.
    let mut mark = |s: f64, west: f64, north: f64, east: f64, inside: &dyn Fn(f64, f64) -> bool| {
        let row = |n: f64| ((n - grid.south) / grid.step).clamp(-1.0, h as f64);
        let col = |e: f64| ((e - grid.west) / grid.step).clamp(-1.0, w as f64);
        let (r0, r1) = (row(s).ceil().max(0.0) as usize, (row(north).floor() + 1.0).max(0.0) as usize);
        let (c0, c1) = (col(west).ceil().max(0.0) as usize, (col(east).floor() + 1.0).max(0.0) as usize);
        for r in r0..r1.min(h) {
            for c in c0..c1.min(w) {
                let (n, e) = grid.node(r, c);
                if inside(n, e) {
                    out[r * w + c] = true;
                }
            }
        }
    };
    for a in &region.airports {
        let (cos, sin) = (a.heading.cos(), a.heading.sin());
        let (half_l, half_w) = (a.length / 2.0, a.width / 2.0);
        let reach = (half_l + AERODROME.0).hypot(half_w + AERODROME.1);
        mark(a.n - reach, a.e - reach, a.n + reach, a.e + reach, &|n, e| {
            let (dn, de) = (n - a.n, e - a.e);
            let (along, across) = ((dn * cos + de * sin).abs(), (de * cos - dn * sin).abs());
            if along < half_l + STRIP.0 && across < half_w + STRIP.1 {
                return true;
            }
            along < half_l + AERODROME.0 && across < half_w + AERODROME.1 && region.cover_at(n, e) == Cover::Grass
        });
    }
    // A building, and the texels its outline touches.
    let margin = grid.step * 0.5;
    for b in &region.buildings {
        let (mut s, mut west, mut north, mut east) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &b.ring {
            (s, north) = (s.min(p[0] as f64), north.max(p[0] as f64));
            (west, east) = (west.min(p[1] as f64), east.max(p[1] as f64));
        }
        mark(s - margin, west - margin, north + margin, east + margin, &|_, _| true);
    }
    for t in &region.towns {
        let r = t.radius * 1.15;
        mark(t.n - r, t.e - r, t.n + r, t.e + r, &|n, e| t.weight(n, e) > 0.5);
    }
    out
}

/// What a parcel is, from its mean colour (linear reflectance), how much of it looks like tree
/// crowns, how mottled it is (the spread of its brightness over its mean) and what the maps say
/// covers most of it.
fn classify(c: [f32; 3], tree: f32, mottled: f32, cover: Cover) -> Kind {
    let y = luma(c);
    let green = c[1] / c[0].max(1e-4);
    let grey = c.iter().copied().fold(0.0, f32::max) / c.iter().copied().fold(1.0, f32::min).max(1e-4);
    match cover {
        Cover::Sea | Cover::Water | Cover::Built | Cover::Railway | Cover::Sand | Cover::Ice => return Kind::None,
        _ if y > 0.4 => return Kind::None,
        _ => {}
    }
    if tree > 0.55 || (y < 0.045 && green > 0.9) {
        Kind::Wood
    } else if cover == Cover::Rock || (grey < 1.1 && y > 0.22 && cover != Cover::Farmland) {
        Kind::Rock
    } else if tree > 0.25 || (cover == Cover::Scrub && y < 0.13) || (mottled > 0.3 && y < 0.11 && green < 1.2) {
        Kind::Scrub
    } else if green > 1.12 {
        if y < 0.05 {
            Kind::Scrub
        } else if cover == Cover::Farmland {
            Kind::Crop
        } else {
            Kind::Meadow
        }
    } else if y > 0.16 {
        if cover == Cover::Farmland { Kind::Stubble } else { Kind::DryGrass }
    } else if c[0] / c[1].max(1e-4) > 1.25 && cover != Cover::Grass {
        Kind::Ploughed
    } else {
        Kind::DryGrass
    }
}

/// What the maps say covers a parcel, from how many of its `n` texels each cover has: water, a
/// town, sand or snow if they hold half of it, else the commonest other cover if it holds a third.
fn main_cover(counts: &[u32; Cover::COUNT], n: f64) -> Cover {
    let share = |c: Cover| counts[c as usize] as f64 / n.max(1.0);
    if let Some(c) = [Cover::Sea, Cover::Water, Cover::Built, Cover::Railway, Cover::Sand, Cover::Ice].into_iter().find(|&c| share(c) >= 0.5) {
        return c;
    }
    let land = [Cover::Grass, Cover::Farmland, Cover::Scrub, Cover::Forest, Cover::Rock, Cover::Wetland];
    land.into_iter().max_by(|a, b| share(*a).total_cmp(&share(*b))).filter(|&c| share(c) >= 0.3).unwrap_or(Cover::Unknown)
}

/// How much a texel looks like the crowns of trees: dark and green, or darker and greener than
/// the parcel round it (a line of trees along a wall, a clump in a pasture).
fn tree_look(c: [f32; 3], parcel: [f32; 3]) -> f32 {
    let smooth = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let y = luma(c);
    let green = smooth(0.85, 1.05, c[1] / c[0].max(1e-4));
    let dark = smooth(0.075, 0.04, y);
    let darker = smooth(0.82, 0.55, y / luma(parcel).max(1e-4));
    dark.max(darker) * green
}

impl Parcels {
    /// The parcels of a grid of the picture, laid out in a texture `side` texels a side, with the
    /// region's maps for what covers the ground (none: they are told by their colour alone).
    pub fn new(grid: &Grid<[u8; 4]>, side: usize, region: Option<&Region>) -> Parcels {
        let (w, h) = (grid.cols, grid.rows);
        let mut img = Image::smoothed(grid);
        // What the maps keep out of the parcels is not cut into them: the picture alone is drawn there.
        if let Some(region) = region {
            for (i, out) in kept_out(grid, region).into_iter().enumerate() {
                if out {
                    img.px[i][3] = 0;
                }
            }
        }
        let s = ((SUPERPIXEL / grid.step).round() as usize).clamp(3, 8);
        let label = superpixels(&img, s);
        let (mut seg, count) = connected(&img, &label, s * s / 4);
        drop(label);
        merged(&img, &mut seg, count, MOST_AREA / (grid.step * grid.step));
        dissolve(&img, &mut seg, count, (LEAST_AREA / (grid.step * grid.step)).ceil() as usize);

        // What each parcel is: its mean colour first, then how its texels look against it.
        let linear = linear_table();
        let lin = |i: usize| {
            let p = grid.data[i];
            [linear[p[0] as usize], linear[p[1] as usize], linear[p[2] as usize]]
        };
        #[derive(Clone, Default)]
        struct Stats {
            n: f64,
            c: [f64; 3],
            /// Sums of east, north, their squares and product, of the brightness and its square, of the tree look.
            x: [f64; 5],
            y: [f64; 2],
            tree: f64,
            cover: [u32; Cover::COUNT],
        }
        let mut stats = vec![Stats::default(); count];
        for (i, &sg) in seg.iter().enumerate() {
            if sg == u32::MAX {
                continue;
            }
            let (c, s) = (lin(i), &mut stats[sg as usize]);
            let (x, y) = ((i % w) as f64, (i / w) as f64);
            s.n += 1.0;
            s.c = [s.c[0] + c[0] as f64, s.c[1] + c[1] as f64, s.c[2] + c[2] as f64];
            s.x = [s.x[0] + x, s.x[1] + y, s.x[2] + x * x, s.x[3] + y * y, s.x[4] + x * y];
            let l = luma(c) as f64;
            s.y = [s.y[0] + l, s.y[1] + l * l];
            if let Some(r) = region {
                let (north, east) = grid.node(i / w, i % w);
                s.cover[r.cover_at(north, east) as usize] += 1;
            }
        }
        let mean = |s: &Stats| [(s.c[0] / s.n) as f32, (s.c[1] / s.n) as f32, (s.c[2] / s.n) as f32];
        let mut tree = vec![0u8; seg.len()];
        for (i, &sg) in seg.iter().enumerate() {
            if sg != u32::MAX {
                let s = &mut stats[sg as usize];
                let t = tree_look(lin(i), mean(s));
                s.tree += t as f64;
                tree[i] = (t * 255.0).round() as u8;
            }
        }
        let kinds: Vec<Kind> = stats
            .iter()
            .map(|s| {
                if s.n == 0.0 {
                    return Kind::None;
                }
                let cover = main_cover(&s.cover, s.n);
                let m = s.y[0] / s.n;
                let spread = ((s.y[1] / s.n - m * m).max(0.0)).sqrt() / m.max(1e-4);
                classify(mean(s), (s.tree / s.n) as f32, spread as f32, cover)
            })
            .collect();

        // The heading of each parcel's rows: its longer axis, from east towards south.
        let heading: Vec<u8> = stats
            .iter()
            .map(|s| {
                if s.n < 2.0 {
                    return 0;
                }
                let (mx, my) = (s.x[0] / s.n, s.x[1] / s.n);
                let (xx, yy, xy) = (s.x[2] / s.n - mx * mx, s.x[3] / s.n - my * my, s.x[4] / s.n - mx * my);
                // Grid rows run north: east-north to east-south turns the angle round.
                let north_of_east = 0.5 * (2.0 * xy).atan2(xx - yy);
                let a = (-north_of_east).rem_euclid(std::f64::consts::PI);
                ((a / std::f64::consts::PI * 256.0) as u32).min(255) as u8
            })
            .collect();

        drop(img);
        // How far each texel is from the edge of its parcel, the edges straightened.
        let dist = straight_edges(w, h, &mut seg);

        // Numbers 1 to 255 that no two parcels touching (at a corner too) share; 0 for the ones not drawn.
        let pairs = neighbours(w, &seg, true);
        let mut first = vec![0usize; count + 1];
        for &(a, b) in &pairs {
            first[a as usize + 1] += 1;
            first[b as usize + 1] += 1;
        }
        for i in 0..count {
            first[i + 1] += first[i];
        }
        let mut next = first.clone();
        let mut adjacent = vec![0u32; pairs.len() * 2];
        for &(a, b) in &pairs {
            adjacent[next[a as usize]] = b;
            next[a as usize] += 1;
            adjacent[next[b as usize]] = a;
            next[b as usize] += 1;
        }
        let mut id = vec![0u8; count];
        for p in 0..count {
            if kinds[p] == Kind::None {
                continue;
            }
            let mut used = [false; 256];
            for &q in &adjacent[first[p]..first[p + 1]] {
                used[id[q as usize] as usize] = true;
            }
            let start = (ihash(p as i32, 77) * 255.0) as usize;
            id[p] = (0..255).map(|k| (start + k) % 255 + 1).find(|&v| !used[v]).unwrap_or(1) as u8;
        }

        let srgb: Vec<[u8; 3]> = stats.iter().map(|s| if s.n > 0.0 { mean(s).map(encode) } else { [0; 3] }).collect();
        let mut map = vec![[0u8; 4]; side * side];
        let mut tone = vec![[0u8; 4]; side * side];
        for (r, row) in seg.chunks(w).enumerate() {
            // Row 0 of the grid is the south; of the texture, the north.
            let y = h - 1 - r;
            if y >= side {
                continue;
            }
            for (x, &sg) in row.iter().enumerate().take(side) {
                let i = r * w + x;
                if sg == u32::MAX {
                    // Not a parcel, but how far it is from the parcels' edge, so they end on it.
                    map[y * side + x] = [0, distance_byte(dist[i]), 0, 0];
                    continue;
                }
                let p = sg as usize;
                map[y * side + x] = [id[p], distance_byte(dist[i]), heading[p], kinds[p] as u8];
                tone[y * side + x] = [srgb[p][0], srgb[p][1], srgb[p][2], tree[i]];
            }
        }
        let north = grid.south + (h as f64 - 1.0) * grid.step;
        Parcels { side, map, tone, north, west: grid.west, step: grid.step }
    }

    /// How far a point (north, east) is inside the texture the parcels are laid in, as a share of its
    /// side from the nearest edge (as the shader measures it): negative outside.
    pub fn inside(&self, n: f64, e: f64) -> f64 {
        let side = self.side as f64;
        let (u, v) = (((e - self.west) / self.step + 0.5) / side, ((self.north - n) / self.step + 0.5) / side);
        u.min(1.0 - u).min(v).min(1.0 - v)
    }

    /// The parcel at a point (north, east) as the ground's shader draws it (`ground_parcel`), if
    /// the point is among the texels.
    pub fn sample(&self, n: f64, e: f64) -> Option<Sample> {
        // Texel coordinates: x east, y south, 0 at the north-west texel's middle.
        let (tx, ty) = ((e - self.west) / self.step, (self.north - n) / self.step);
        let (x0, y0) = (tx.floor(), ty.floor());
        if x0 < 0.0 || y0 < 0.0 || x0 + 1.0 >= self.side as f64 || y0 + 1.0 >= self.side as f64 {
            return None;
        }
        let (fx, fy) = (tx - x0, ty - y0);
        let at = |dx: usize, dy: usize| self.map[(y0 as usize + dy) * self.side + x0 as usize + dx];
        let texels = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
        let weights = [(1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy];
        // d/dfx and d/dfy of each weight.
        let gx = [-(1.0 - fy), 1.0 - fy, -fy, fy];
        let gy = [-(1.0 - fx), -fx, 1.0 - fx, fx];
        let signed = |id: u8| {
            let (mut s, mut sx, mut sy) = (0.0, 0.0, 0.0);
            for k in 0..4 {
                let d = byte_distance(texels[k][1]);
                let d = if texels[k][0] == id { d } else { -d };
                s += weights[k] * d;
                sx += gx[k] * d;
                sy += gy[k] * d;
            }
            (s, sx, sy)
        };
        let (mut best, mut second) = ((f64::NEG_INFINITY, 0usize, (0.0, 0.0)), (f64::NEG_INFINITY, 0usize));
        for k in 0..4 {
            if texels[..k].iter().any(|t| t[0] == texels[k][0]) {
                continue;
            }
            let (s, sx, sy) = signed(texels[k][0]);
            if s > best.0 {
                second = (best.0, best.1);
                best = (s, k, (sx, sy));
            } else if s > second.0 {
                second = (s, k);
            }
        }
        let own = texels[best.1];
        let (other, edge) = if second.0 == f64::NEG_INFINITY {
            (own, best.0)
        } else {
            (texels[second.1], (best.0 - second.0) / 2.0)
        };
        let (sx, sy) = best.2;
        let len = sx.hypot(sy).max(1e-9);
        // Into the parcel is up its signed distance: texel x is east, y south.
        let inward = (-sy / len, sx / len);
        let nearest = ((tx.round() as usize).min(self.side - 1), (ty.round() as usize).min(self.side - 1));
        let tone_of = |k: usize| self.tone[(y0 as usize + k / 2) * self.side + x0 as usize + k % 2];
        let tone = tone_of(best.1);
        let other_tone = tone_of(if second.0 == f64::NEG_INFINITY { best.1 } else { second.1 });
        Some(Sample {
            kind: Kind::from_byte(own[3]),
            id: own[0],
            other: other[0],
            other_kind: Kind::from_byte(other[3]),
            edge: edge.max(0.0) * self.step,
            inward,
            tree: self.tone[nearest.1 * self.side + nearest.0][3] as f64 / 255.0,
            colour: [tone[0], tone[1], tone[2]],
            other_colour: [other_tone[0], other_tone[1], other_tone[2]],
            heading: own[2] as f64 / 255.0 * std::f64::consts::PI,
        })
    }
}

/// What stands on the edge between two parcels (`parcel_bounds` in shaders.wgsl decides it alike).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    None,
    Wall,
    Hedge,
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Sample {
    /// What stands on the nearest edge: decided by the pair of parcels, the same on both sides; a
    /// wall where the land is dry, a hedge where it is green, none inside woods, scrub and bare rock.
    pub fn boundary(&self) -> Boundary {
        let rough = |k: Kind| k == Kind::Wood || k == Kind::Scrub;
        let wood = rough(self.kind) as u8 + rough(self.other_kind) as u8;
        let rock = self.kind == Kind::Rock || self.other_kind == Kind::Rock;
        if self.other == self.id || self.other_kind == Kind::None || self.edge > 8.0 || wood == 2 || rock {
            return Boundary::None;
        }
        let (lo, hi) = (self.id.min(self.other) as i32, self.id.max(self.other) as i32);
        let pair = ihash(lo * 7 + 3, hi * 13 + 5) as f64;
        let linear = linear_table();
        let (a, b) = (self.colour.map(|v| linear[v as usize] as f64), self.other_colour.map(|v| linear[v as usize] as f64));
        let dry = 1.0 - smoothstep(0.95, 1.3, (a[1] + b[1]) / (a[0] + b[0]).max(2e-3));
        let alike = smoothstep(0.004, 0.02, ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt());
        let wall = dry * 0.9 * if wood == 1 { 0.35 } else { 1.0 } * (0.5 + 0.5 * alike);
        let hedge = (1.0 - dry) * 0.4 * if wood == 1 { 0.0 } else { 1.0 } * (0.3 + 0.7 * alike);
        if pair < wall {
            Boundary::Wall
        } else if pair < wall + hedge {
            Boundary::Hedge
        } else {
            Boundary::None
        }
    }
}

/// The parcel at a point (north, east) of the grid the shader draws there: the near one well
/// inside it, else the far one.
pub fn sample_at(parcels: &[Parcels], n: f64, e: f64) -> Option<Sample> {
    let near = parcels.first().filter(|p| p.inside(n, e) > 0.0165);
    near.or_else(|| parcels.get(1).filter(|p| p.inside(n, e) > 0.0)).and_then(|p| p.sample(n, e))
}

/// Spots where a tree may stand on the parcels, metres apart: a tile's side is a whole number of them.
const TREE_SPOT: f64 = 6.4;
/// An olive grove's trees: metres between them along its rows and between the rows.
const GROVE: (f64, f64) = (7.5, 8.5);

/// Adds to the instances of a tree tile (`key`, side `size`, by species as `species::assign`
/// made them) the trees the parcels show: woods where the picture sees woods and the maps do not,
/// scrub, the clumps and lines of trees in the picture, the bushes along walls and hedges, and
/// where the summers are dry the olive groves, in rows along their fields. Each spot is decided by
/// its place alone, so tiles of any size hold the same trees. They are drawn, not run into: the
/// simulation never reads the picture.
pub fn trees(parcels: &[Parcels], terrain: &Terrain, key: (i32, i32), size: f64, all: &mut Instances) {
    let (ox, oz) = (key.0 as f64 * size, key.1 as f64 * size);
    let spots = (size / TREE_SPOT).round() as i64;
    let airports = terrain.airports(-(oz + size), -oz, ox, ox + size);
    // The same ground kept clear as for the simulation's trees: roads, railways, buildings.
    let clear = sim_core::scenery::Clear::new(terrain, ox, oz, size);
    let towns = terrain.towns(-(oz + size), -oz, ox, ox + size);
    let climate = Climate::of(terrain);
    let linear = linear_table();
    // A tree at (east, south) if nothing is in the way, of `species` (or as `shape` grows there).
    let mut plant = |x: f64, z: f64, s: &Sample, what: Result<Species, Tree>, r: [f64; 4]| {
        let (n, e) = (-z, x);
        let g = terrain.ground_with(n, e, &airports);
        if g.h < 0.6 || g.river > 0.05 || g.flat > 0.02 || towns.iter().any(|t| t.weight(n, e) > 0.25) || !clear.holds(n, e) {
            return;
        }
        let d = 6.0;
        let sx = (terrain.ground_with(n, e + d, &airports).h - g.h) / d;
        let sz = (terrain.ground_with(n - d, e, &airports).h - g.h) / d;
        // Where the maps' woods have their trees already, none more.
        if terrain.surface(n, e, &g, sx.hypot(sz), 0.0).forest > 0.35 {
            return;
        }
        let c = s.colour.map(|v| linear[v as usize] as f64);
        let dry = c[1] / c[0].max(1e-4) < 1.2;
        let species = what.unwrap_or_else(|shape| species::choose(shape, &climate, terrain, (n, e, g.h), [r[0], r[1], r[2]]));
        // Smaller where it is dry (wild olives, holm oaks, lentisks), and towards the tree line.
        let stunt = (if dry { 0.7 } else { 1.0 }) * (1.0 - 0.5 * smoothstep(1300.0, 1850.0, g.h));
        let (r4, r5) = (r[1], r[2]);
        let (height, width) = match species.shape() {
            Tree::Oak => (9.0 + 8.0 * r4, 0.9 + 0.4 * r5),
            Tree::Ash => (12.0 + 9.0 * r4, 0.85 + 0.35 * r5),
            Tree::HolmOak => (6.0 + 5.0 * r4, 0.95 + 0.45 * r5),
            Tree::Poplar => (15.0 + 11.0 * r4, 0.9 + 0.3 * r5),
            Tree::Spruce => (13.0 + 13.0 * r4, 0.85 + 0.4 * r5),
            Tree::Fir => (11.0 + 11.0 * r4, 0.9 + 0.4 * r5),
            Tree::Pine => (8.0 + 7.0 * r4, 0.9 + 0.45 * r5),
            Tree::Bush => (1.4 + 2.2 * r4, 0.9 + 0.8 * r5),
        };
        let height = match species {
            Species::Maquis | Species::Shrub => height,
            // A grove's olives are pruned low and wide.
            Species::Olive if what.is_ok() => 4.0 + 1.5 * r4,
            _ => height * stunt,
        };
        // Darker or lighter as the picture is there, each a little on its own.
        let brightness = (0.5 + 2.0 * (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])).min(0.9) + 0.2 * r[3];
        let tone = (brightness * 32.0).floor() + r[0] * 0.999;
        all[species as usize].extend_from_slice(&[(x - ox) as f32, g.h as f32 - 0.25, (z - oz) as f32, height as f32, tone as f32, width as f32, sx as f32, sz as f32]);
    };
    for j in 0..spots {
        for i in 0..spots {
            let (gx, gz) = ((key.0 as i64 * spots + i) as i32, (key.1 as i64 * spots + j) as i32);
            let r = |k: i32| ihash(gx.wrapping_mul(8).wrapping_add(k), gz.wrapping_add(31)) as f64;
            let (cx, cz) = (ox + i as f64 * TREE_SPOT, oz + j as f64 * TREE_SPOT);
            let (mut x, mut z) = (cx + r(0) * TREE_SPOT, cz + r(1) * TREE_SPOT);
            let Some(s) = sample_at(parcels, -z, x) else {
                continue;
            };
            if s.kind == Kind::None {
                continue;
            }
            // An olive grove: dry open land where the picture sees scattered crowns, in a
            // Mediterranean place. Its trees stand on a lattice along the field's rows; each spot
            // plants the nodes that fall in its square.
            let open = matches!(s.kind, Kind::DryGrass | Kind::Stubble | Kind::Ploughed);
            if open && s.tree > 0.16 && climate.dry_at(terrain, -z, x) > 0.5 {
                let (cos, sin) = (s.heading.cos() as f32, s.heading.sin() as f32);
                let (along, across) = (Vec2::new(cos, sin), Vec2::new(-sin, cos));
                let middle = Vec2::new((cx + TREE_SPOT / 2.0) as f32, (cz + TREE_SPOT / 2.0) as f32);
                let (a, b) = ((middle.dot(along) as f64 / GROVE.0).round(), (middle.dot(across) as f64 / GROVE.1).round());
                for (da, db) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (0.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
                    let node = along * ((a + da) * GROVE.0) as f32 + across * ((b + db) * GROVE.1) as f32;
                    let (nx, nz) = (node.x as f64, node.y as f64);
                    if nx < cx || nx >= cx + TREE_SPOT || nz < cz || nz >= cz + TREE_SPOT {
                        continue;
                    }
                    let Some(at) = sample_at(parcels, -nz, nx).filter(|t| t.id == s.id && t.edge > 3.0) else {
                        continue;
                    };
                    let h = |k: i32| ihash((nx * 4.0) as i32 + k * 977, (nz * 4.0) as i32) as f64;
                    plant(nx, nz, &at, Ok(Species::Olive), [h(1), h(2), h(3), h(4)]);
                }
                continue;
            }
            let boundary = s.boundary();
            let on_edge = boundary != Boundary::None && s.edge < TREE_SPOT * 0.5;
            let mut chance = match s.kind {
                Kind::Wood => 0.7,
                Kind::Scrub => 0.3,
                _ => 0.0,
            };
            chance = f64::max(chance, 0.85 * s.tree.powf(1.5));
            if on_edge {
                chance = chance.max(if boundary == Boundary::Hedge { 0.8 } else { 0.18 });
            }
            if r(2) >= chance {
                continue;
            }
            if on_edge {
                // On the wall or hedge itself, a little either side of it.
                let along = (r(9) - 0.5) * 0.8;
                x += -s.inward.1 * s.edge + s.inward.1 * along;
                z += s.inward.0 * s.edge - s.inward.0 * along;
            }
            let c = s.colour.map(|v| linear[v as usize] as f64);
            let dry = c[1] / c[0].max(1e-4) < 1.2;
            let height = terrain.height(-z, x);
            let mountain = smoothstep(800.0, 1300.0, height);
            let (r3, r4) = (r(3), r(4));
            let shape = if on_edge {
                match (boundary, r3) {
                    (Boundary::Hedge, v) if v < 0.85 => Tree::Bush,
                    (Boundary::Hedge, _) => Tree::Oak,
                    (_, v) if v < 0.6 => Tree::Bush,
                    _ => Tree::HolmOak,
                }
            } else if s.kind == Kind::Scrub {
                if r3 < 0.65 { Tree::Bush } else if dry || r4 < 0.5 { Tree::HolmOak } else { Tree::Pine }
            } else if dry {
                if r3 < 0.55 { Tree::HolmOak } else if r3 < 0.85 { Tree::Pine } else { Tree::Bush }
            } else if r3 < mountain {
                if r4 < 0.6 { Tree::Spruce } else { Tree::Fir }
            } else if r3 < 0.5 {
                Tree::Oak
            } else if r3 < 0.8 {
                Tree::Ash
            } else if r3 < 0.9 {
                Tree::Poplar
            } else {
                Tree::Bush
            };
            plant(x, z, &s, Err(shape), [r(7), r(4), r(5), r(6)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Airport, Detail, Town};

    /// A grid of `size` texels a side of 10 m, coloured by `paint(east, north)` in metres from its south-west texel.
    fn picture(size: usize, paint: impl Fn(f64, f64) -> [u8; 4]) -> Grid<[u8; 4]> {
        let mut g = Grid::new(0.0, 0.0, 10.0, size, size, [0u8; 4]);
        for r in 0..size {
            for c in 0..size {
                g.set(r, c, paint(c as f64 * 10.0, r as f64 * 10.0));
            }
        }
        g
    }

    const PALE: [u8; 4] = [170, 150, 120, 255];
    const GREEN: [u8; 4] = [90, 120, 60, 255];

    #[test]
    fn two_fields_meet_on_a_straight_edge_between_the_texels() {
        // A slanted line across the picture, at 30° from north: pale to the west, green to the east.
        let (dx, dn) = (30f64.to_radians().cos(), -30f64.to_radians().sin());
        // Two fields small enough to stay a parcel each.
        let line = |e: f64, n: f64| (e - 115.0) * dx + (n - 115.0) * dn;
        let g = picture(24, |e, n| if line(e, n) < 0.0 { PALE } else { GREEN });
        let p = Parcels::new(&g, 24, None);
        // Along the line, away from the picture's edges: the edge found within a sixth of a texel
        // of the true one, though the picture steps a texel at a time.
        for k in 0..11 {
            let t = -50.0 + k as f64 * 10.0;
            let (e, n) = (115.0 - dn * t, 115.0 + dx * t);
            for off in [-9.0, -4.0, 4.0, 9.0] {
                let s = p.sample(n + dn * off, e + dx * off).unwrap();
                assert!((s.edge - off.abs()).abs() < 1.6, "at {off} m from the line, {t} m along, the edge is {} m away", s.edge);
                // The way in points away from the line, into the parcel of the point.
                let into = s.inward.1 * dx + s.inward.0 * dn;
                assert!(into * off > 0.0, "inward {:?} at {off}", s.inward);
            }
        }
        let (west, east) = (p.sample(115.0, 30.0).unwrap(), p.sample(115.0, 200.0).unwrap());
        assert_ne!(west.id, east.id);
        assert!(west.colour[0] > east.colour[0] && east.colour[1] > east.colour[0]);
    }

    #[test]
    fn a_field_is_told_by_its_colour_and_the_maps() {
        let dark = [35, 55, 28, 255];
        let g = picture(48, |e, _| if e < 160.0 { PALE } else if e < 320.0 { GREEN } else { dark });
        let p = Parcels::new(&g, 48, None);
        let kind = |e: f64| p.sample(240.0, e).unwrap().kind;
        assert_eq!(kind(80.0), Kind::DryGrass);
        assert_eq!(kind(240.0), Kind::Meadow);
        assert_eq!(kind(400.0), Kind::Wood);
        assert!(p.sample(240.0, 400.0).unwrap().tree > 0.9);
        assert!(p.sample(240.0, 80.0).unwrap().tree < 0.1);
        assert_eq!(classify([0.3, 0.25, 0.2], 0.0, 0.0, Cover::Built), Kind::None);
        assert_eq!(classify([0.22, 0.2, 0.15], 0.0, 0.0, Cover::Farmland), Kind::Stubble);
        assert_eq!(classify([0.12, 0.08, 0.05], 0.0, 0.0, Cover::Farmland), Kind::Ploughed);
        assert_eq!(classify([0.06, 0.1, 0.04], 0.0, 0.0, Cover::Farmland), Kind::Crop);
    }

    #[test]
    fn what_is_not_known_is_not_a_parcel_and_the_texture_is_north_up() {
        // The south half unknown.
        let g = picture(32, |_, n| if n < 160.0 { [0; 4] } else { PALE });
        let p = Parcels::new(&g, 40, None);
        assert_eq!(p.map.len(), 40 * 40);
        // The texture's first rows are the north: known.
        assert_ne!(p.map[5 * 40 + 5][0], 0);
        assert_eq!(p.map[25 * 40 + 5][0], 0);
        // Outside the grid (the texture is bigger), nothing.
        assert_eq!(p.map[5 * 40 + 35], [0; 4]);
        assert_eq!(p.sample(100.0, 100.0).unwrap().kind, Kind::None);
    }

    #[test]
    fn touching_parcels_never_share_a_number() {
        // A checkerboard of fields of 40 m, in two colours that alternate.
        let g = picture(64, |e, n| if ((e / 40.0) as i32 + (n / 40.0) as i32) % 2 == 0 { PALE } else { GREEN });
        let p = Parcels::new(&g, 64, None);
        // The picture's colour at a texel of the texture (rows from the north).
        let colour = |x: usize, y: usize| g.data[(63 - y) * 64 + x];
        for y in 0..63 {
            for x in 1..63 {
                for (bx, by) in [(x + 1, y), (x, y + 1), (x + 1, y + 1), (x - 1, y + 1)] {
                    if colour(x, y) != colour(bx, by) {
                        assert_ne!(p.map[y * 64 + x][0], p.map[by * 64 + bx][0], "texels {x},{y} and {bx},{by}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_trees_of_a_big_tile_are_those_of_the_small_ones_in_it() {
        // A flat grassy region, its picture a dark wood to the west and a pale field to the east.
        let region = Region {
            name: "Campo".into(),
            latitude: 0.7,
            longitude: 0.0,
            heights: vec![Grid::centred(0.0, 0.0, 4_000.0, 30.0, 20.0f32)],
            cover: Grid::centred(0.0, 0.0, 4_000.0, 30.0, Cover::Grass as u8),
            airports: Vec::new(),
            roads: Vec::new(),
            towns: Vec::new(),
            buildings: Vec::new(),
            bridges: Vec::new(),
        };
        let terrain = Terrain::from_region(1, std::sync::Arc::new(region));
        let mut g = Grid::centred(0.0, 0.0, 1_270.0, 10.0, [0u8; 4]);
        for r in 0..g.rows {
            for c in 0..g.cols {
                let (_, e) = g.node(r, c);
                g.set(r, c, if e < 128.0 { [35, 55, 28, 255] } else { PALE });
            }
        }
        let parcels = vec![Parcels::new(&g, g.cols, terrain.region())];
        let place = |key: (i32, i32), size: f64| {
            let mut all = vec![Vec::new(); crate::species::COUNT];
            trees(&parcels, &terrain, key, size, &mut all);
            let mut spots: Vec<(usize, i64, i64)> = Vec::new();
            for (kind, list) in all.iter().enumerate() {
                for t in list.chunks(8) {
                    spots.push((kind, ((t[0] as f64 + key.0 as f64 * size) * 10.0).round() as i64, ((t[2] as f64 + key.1 as f64 * size) * 10.0).round() as i64));
                }
            }
            spots
        };
        let mut big = place((0, 0), 256.0);
        let mut small: Vec<_> = [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().flat_map(|k| place(k, 128.0)).collect();
        big.sort_unstable();
        small.sort_unstable();
        assert_eq!(big, small);
        // The wood is full of trees; the pale field has few.
        let wood = big.iter().filter(|t| t.1 < 1_280).count();
        assert!(wood > 300 && wood > 5 * (big.len() - wood), "{wood} trees in the wood, {} in the field", big.len() - wood);
    }

    #[test]
    fn no_parcel_on_the_aerodrome_the_towns_and_the_water() {
        // Fields everywhere in the picture, every 60 m another colour; a runway north-south at the
        // middle, the aerodrome's mown grass 300 m either side of it, farmland beyond, a lake and a
        // village to the east.
        let mut cover = Grid::centred(0.0, 0.0, 4_000.0, 24.0, Cover::Farmland as u8);
        for r in 0..cover.rows {
            for c in 0..cover.cols {
                let (n, e) = cover.node(r, c);
                if e.abs() < 300.0 && n.abs() < 900.0 {
                    cover.set(r, c, Cover::Grass as u8);
                }
                if (n - 600.0).hypot(e - 1000.0) < 120.0 {
                    cover.set(r, c, Cover::Water as u8);
                }
            }
        }
        let runway = Airport { id: 1, n: 0.0, e: 0.0, heading: 0.0, elevation: 20.0, length: 1200.0, width: 30.0, detail: Detail::GENERATED };
        let region = Region {
            name: "Campo".into(),
            latitude: 0.7,
            longitude: 0.0,
            heights: vec![Grid::centred(0.0, 0.0, 4_000.0, 30.0, 20.0f32)],
            cover,
            airports: vec![runway],
            roads: Vec::new(),
            towns: vec![Town { n: -600.0, e: 1000.0, radius: 150.0, grid: 0.0 }],
            buildings: Vec::new(),
            bridges: Vec::new(),
        };
        let mut g = Grid::centred(0.0, 0.0, 3_000.0, 10.0, [0u8; 4]);
        for r in 0..g.rows {
            for c in 0..g.cols {
                let (n, e) = g.node(r, c);
                g.set(r, c, if ((n / 60.0).floor() + (e / 60.0).floor()) as i32 % 2 == 0 { PALE } else { GREEN });
            }
        }
        let p = Parcels::new(&g, g.cols, Some(&region));
        let kind = |n: f64, e: f64| p.sample(n, e).unwrap().kind;
        // On the runway, its strip, the aerodrome's grass, the lake and the village: nothing.
        for (n, e) in [(0.0, 0.0), (300.0, 50.0), (-500.0, -200.0), (100.0, 250.0), (600.0, 1000.0), (-600.0, 1000.0)] {
            assert_eq!(kind(n, e), Kind::None, "at {n}, {e}");
        }
        // The fields beyond.
        for (n, e) in [(0.0, 600.0), (0.0, -700.0), (1200.0, 0.0), (-1200.0, 500.0)] {
            assert_ne!(kind(n, e), Kind::None, "at {n}, {e}");
        }
        // The first parcel east of the aerodrome ends on its edge with no wall: nothing stands against it.
        let by = (0..60).map(|k| p.sample(30.0, 280.0 + k as f64).unwrap()).find(|s| s.kind != Kind::None).unwrap();
        assert!(by.edge < 20.0 && by.other_kind == Kind::None && by.boundary() == Boundary::None, "{by:?}");
    }

    #[test]
    fn the_distance_field_is_exact() {
        let mut seed = vec![false; 9 * 7];
        seed[3 * 9 + 4] = true;
        let d = distance_field(9, 7, &seed);
        assert_eq!(d[3 * 9 + 4], 0.0);
        assert!((d[0] - (16.0f32 + 9.0).sqrt()).abs() < 1e-5);
        assert!((d[6 * 9 + 8] - (16.0f32 + 9.0).sqrt()).abs() < 1e-5);
    }
}

