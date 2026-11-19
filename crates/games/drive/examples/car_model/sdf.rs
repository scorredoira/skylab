//! The tools the car is modelled with: shapes as signed distances (negative inside), joined, cut
//! and rounded as fields; the surface of a field as triangles (naive surface nets: a vertex in every
//! cell of a grid the surface crosses, moved on to the surface, with the field's gradient as its
//! normal, so the shading is as smooth as the field); triangles split along a line of the surface
//! (where another field crosses zero), which gives crisp edges to glass, lamps and shut lines
//! whatever the grid; and how open each point is to the sky (ambient occlusion from the field).

use glam::{Vec2, Vec3};

pub fn sq(x: f32) -> f32 {
    x * x
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The union of two shapes, rounded where they meet over about `k` metres.
pub fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// What two shapes share, its edge rounded over about `k` metres. `smax(a, -b, k)` cuts `b` out of `a`.
pub fn smax(a: f32, b: f32, k: f32) -> f32 {
    -smin(-a, -b, k)
}

/// A box of half sizes `half` round the origin, its edges rounded by `r`.
pub fn round_box(p: Vec3, half: Vec3, r: f32) -> f32 {
    let q = p.abs() - (half - Vec3::splat(r));
    q.max(Vec3::ZERO).length() + q.max_element().min(0.0) - r
}

/// The same in two dimensions.
pub fn round_box2(p: Vec2, half: Vec2, r: f32) -> f32 {
    let q = p.abs() - (half - Vec2::splat(r));
    q.max(Vec2::ZERO).length() + q.max_element().min(0.0) - r
}

/// An ellipsoid of radii `r` round the origin (not a true distance away from its surface, near enough).
pub fn ellipsoid(p: Vec3, r: Vec3) -> f32 {
    let k0 = (p / r).length();
    let k1 = (p / (r * r)).length();
    if k1 < 1e-9 { -r.min_element() } else { k0 * (k0 - 1.0) / k1 }
}

/// A rod from `a` to `b` of radius `r`, its ends round.
pub fn capsule(p: Vec3, a: Vec3, b: Vec3, r: f32) -> f32 {
    let (pa, ba) = (p - a, b - a);
    let h = (pa.dot(ba) / ba.length_squared()).clamp(0.0, 1.0);
    (pa - ba * h).length() - r
}

/// A closed outline in a plane, as a signed distance: drawn through control points and rounded
/// (Chaikin's corner cutting: the curve stays inside the control polygon and touches the middle
/// of each of its sides; a control point given twice makes a tighter corner).
#[derive(Clone, Debug)]
pub struct Shape {
    pub points: Vec<Vec2>,
}

impl Shape {
    pub fn closed(control: &[(f32, f32)], rounds: usize) -> Shape {
        let mut points: Vec<Vec2> = control.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        for _ in 0..rounds {
            let n = points.len();
            let mut next = Vec::with_capacity(n * 2);
            for i in 0..n {
                let (a, b) = (points[i], points[(i + 1) % n]);
                next.push(a * 0.75 + b * 0.25);
                next.push(a * 0.25 + b * 0.75);
            }
            points = next;
        }
        Shape { points }
    }

    /// An outline symmetric in its second coordinate: `half` runs from a point on the axis of
    /// symmetry round the positive side to another point on it (second coordinates 0 at both ends).
    pub fn mirrored_second(half: &[(f32, f32)], rounds: usize) -> Shape {
        let mut all = half.to_vec();
        all.extend(half[1..half.len() - 1].iter().rev().map(|&(x, y)| (x, -y)));
        Shape::closed(&all, rounds)
    }

    /// The same for an outline symmetric in its first coordinate (first coordinates 0 at both ends).
    pub fn mirrored_first(half: &[(f32, f32)], rounds: usize) -> Shape {
        let mut all = half.to_vec();
        all.extend(half[1..half.len() - 1].iter().rev().map(|&(x, y)| (-x, y)));
        Shape::closed(&all, rounds)
    }

    /// Metres from the outline, negative inside.
    pub fn sd(&self, x: f32, y: f32) -> f32 {
        let p = Vec2::new(x, y);
        let n = self.points.len();
        let mut d2 = f32::MAX;
        let mut inside = false;
        for i in 0..n {
            let (a, b) = (self.points[i], self.points[(i + 1) % n]);
            let (e, w) = (b - a, p - a);
            let t = (w.dot(e) / e.length_squared().max(1e-12)).clamp(0.0, 1.0);
            d2 = d2.min((w - e * t).length_squared());
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        d2.sqrt() * if inside { -1.0 } else { 1.0 }
    }
}

/// The gradient of a field at a point, by differences.
pub fn gradient<F: Fn(Vec3) -> f32>(f: &F, p: Vec3) -> Vec3 {
    let e = 0.0015;
    Vec3::new(f(p + Vec3::X * e) - f(p - Vec3::X * e), f(p + Vec3::Y * e) - f(p - Vec3::Y * e), f(p + Vec3::Z * e) - f(p - Vec3::Z * e)) / (2.0 * e)
}

/// A corner of a triangle: where, its normal, and how open it is to the sky (1 open, 0 shut in).
#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub p: Vec3,
    pub n: Vec3,
    pub open: f32,
}

pub type Triangle = [Corner; 3];

/// Runs `job(k)` for k in 0..count on 8 threads, the results in order.
fn parallel<T: Send, J: Fn(usize) -> T + Sync>(count: usize, job: J) -> Vec<T> {
    let threads = 8.min(count.max(1));
    let mut out: Vec<Option<T>> = (0..count).map(|_| None).collect();
    let chunk = count.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (c, slots) in out.chunks_mut(chunk).enumerate() {
            let job = &job;
            scope.spawn(move || {
                for (k, slot) in slots.iter_mut().enumerate() {
                    *slot = Some(job(c * chunk + k));
                }
            });
        }
    });
    out.into_iter().map(|v| v.expect("a job")).collect()
}

/// The surface of `field` (where it is zero, the negative side inside) within the box from `lo`
/// to `hi`, sampled on a grid of `cell` metres. `open` gives each vertex's openness to the sky
/// from its place and normal.
pub fn surface<F: Fn(Vec3) -> f32 + Sync, O: Fn(Vec3, Vec3) -> f32 + Sync>(field: &F, lo: Vec3, hi: Vec3, cell: f32, open: &O) -> Vec<Triangle> {
    let size = ((hi - lo) / cell).ceil();
    let (nx, ny, nz) = (size.x as usize + 1, size.y as usize + 1, size.z as usize + 1);
    let at = |i: usize, j: usize, k: usize| lo + Vec3::new(i as f32, j as f32, k as f32) * cell;
    let values: Vec<f32> = parallel(nx, |i| {
        let mut slab = Vec::with_capacity(ny * nz);
        for j in 0..ny {
            for k in 0..nz {
                slab.push(field(at(i, j, k)));
            }
        }
        slab
    })
    .concat();
    let value = |i: usize, j: usize, k: usize| values[(i * ny + j) * nz + k];
    // A vertex in each cell the surface crosses: the middle of where it crosses the cell's edges.
    let cells = |i: usize, j: usize, k: usize| (i * (ny - 1) + j) * (nz - 1) + k;
    let mut vertex_of = vec![u32::MAX; (nx - 1) * (ny - 1) * (nz - 1)];
    let mut points: Vec<Vec3> = Vec::new();
    const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    for i in 0..nx - 1 {
        for j in 0..ny - 1 {
            for k in 0..nz - 1 {
                let corner = |c: usize| (i + (c & 1), j + ((c >> 1) & 1), k + ((c >> 2) & 1));
                let v: [f32; 8] = std::array::from_fn(|c| {
                    let (a, b, d) = corner(c);
                    value(a, b, d)
                });
                let inside = v.iter().filter(|x| **x < 0.0).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                let (mut sum, mut count) = (Vec3::ZERO, 0.0);
                for (a, b) in EDGES {
                    if (v[a] < 0.0) != (v[b] < 0.0) {
                        let (pa, pb) = (corner(a), corner(b));
                        let t = v[a] / (v[a] - v[b]);
                        sum += at(pa.0, pa.1, pa.2).lerp(at(pb.0, pb.1, pb.2), t);
                        count += 1.0;
                    }
                }
                vertex_of[cells(i, j, k)] = points.len() as u32;
                points.push(sum / count);
            }
        }
    }
    // On to the surface, with its normal and its openness.
    let corners: Vec<Corner> = parallel(points.len(), |v| {
        let mut p = points[v];
        for _ in 0..4 {
            let g = gradient(field, p);
            let step = g * (field(p) / g.length_squared().max(1e-6));
            p -= step.clamp_length_max(cell * 0.6);
        }
        let n = gradient(field, p).normalize_or(Vec3::Z);
        Corner { p, n, open: open(p, n) }
    });
    // A quad round every edge of the grid the surface crosses, between the four cells that share it.
    let mut out = Vec::new();
    let mut quad = |cells4: [u32; 4]| {
        if cells4.contains(&u32::MAX) {
            return;
        }
        let c = cells4.map(|v| corners[v as usize]);
        // Split along the shorter diagonal; wound so that the face looks the way its normals do.
        let pairs = if (c[0].p - c[2].p).length_squared() <= (c[1].p - c[3].p).length_squared() { [[0, 1, 2], [0, 2, 3]] } else { [[0, 1, 3], [1, 2, 3]] };
        for t in pairs {
            let mut tri = [c[t[0]], c[t[1]], c[t[2]]];
            let face = (tri[1].p - tri[0].p).cross(tri[2].p - tri[0].p);
            if face.length_squared() < 1e-14 {
                continue;
            }
            if face.dot(tri[0].n + tri[1].n + tri[2].n) < 0.0 {
                tri.swap(1, 2);
            }
            out.push(tri);
        }
    };
    for i in 0..nx {
        for j in 0..ny {
            for k in 0..nz {
                let v = value(i, j, k) < 0.0;
                if i + 1 < nx && j > 0 && k > 0 && j < ny - 1 && k < nz - 1 && (value(i + 1, j, k) < 0.0) != v {
                    quad([vertex_of[cells(i, j - 1, k - 1)], vertex_of[cells(i, j, k - 1)], vertex_of[cells(i, j, k)], vertex_of[cells(i, j - 1, k)]]);
                }
                if j + 1 < ny && i > 0 && k > 0 && i < nx - 1 && k < nz - 1 && (value(i, j + 1, k) < 0.0) != v {
                    quad([vertex_of[cells(i - 1, j, k - 1)], vertex_of[cells(i, j, k - 1)], vertex_of[cells(i, j, k)], vertex_of[cells(i - 1, j, k)]]);
                }
                if k + 1 < nz && i > 0 && j > 0 && i < nx - 1 && j < ny - 1 && (value(i, j, k + 1) < 0.0) != v {
                    quad([vertex_of[cells(i - 1, j - 1, k)], vertex_of[cells(i, j - 1, k)], vertex_of[cells(i, j, k)], vertex_of[cells(i - 1, j, k)]]);
                }
            }
        }
    }
    out
}

/// How open to the sky a point of a surface is, from the field round it: how much nearer the
/// surface is, a little way out along the normal, than it would be over a flat one.
pub fn openness<F: Fn(Vec3) -> f32>(field: &F, p: Vec3, n: Vec3) -> f32 {
    let (mut shut, mut weight) = (0.0, 1.0);
    for i in 1..=5 {
        let h = 0.015 + 0.05 * i as f32;
        shut += ((h - field(p + n * h)) / h).clamp(0.0, 1.0) * weight;
        weight *= 0.75;
    }
    (1.0 - shut * 0.42).clamp(0.08, 1.0)
}

/// Splits triangles where `d` (a field over the surface, read at the corners) crosses zero:
/// those on its negative side, and the rest.
pub fn split<D: Fn(&Corner) -> f32 + Sync>(triangles: &[Triangle], d: &D) -> (Vec<Triangle>, Vec<Triangle>) {
    let values: Vec<[f32; 3]> = parallel(triangles.len(), |t| [d(&triangles[t][0]), d(&triangles[t][1]), d(&triangles[t][2])]);
    let (mut inside, mut outside) = (Vec::new(), Vec::new());
    // Where the field is zero between two corners; the same point whichever triangle asks.
    let cut = |a: &Corner, da: f32, b: &Corner, db: f32| -> Corner {
        let key = |c: &Corner| [c.p.x.to_bits(), c.p.y.to_bits(), c.p.z.to_bits()];
        let (a, da, b, db) = if key(a) <= key(b) { (a, da, b, db) } else { (b, db, a, da) };
        let t = (da / (da - db)).clamp(0.0, 1.0);
        Corner { p: a.p.lerp(b.p, t), n: a.n.lerp(b.n, t).normalize_or(a.n), open: a.open + (b.open - a.open) * t }
    };
    for (tri, v) in triangles.iter().zip(&values) {
        let negative = v.iter().filter(|x| **x < 0.0).count();
        if negative == 3 {
            inside.push(*tri);
        } else if negative == 0 {
            outside.push(*tri);
        } else {
            // The corner alone on its side first, the order round the triangle kept.
            let lone = (0..3).find(|&i| (v[i] < 0.0) == (negative == 1)).unwrap_or(0);
            let (a, b, c) = (lone, (lone + 1) % 3, (lone + 2) % 3);
            let ab = cut(&tri[a], v[a], &tri[b], v[b]);
            let ac = cut(&tri[a], v[a], &tri[c], v[c]);
            let (alone, rest) = if negative == 1 { (&mut inside, &mut outside) } else { (&mut outside, &mut inside) };
            alone.push([tri[a], ab, ac]);
            rest.push([ab, tri[b], tri[c]]);
            rest.push([ab, tri[c], ac]);
        }
    }
    let whole = |t: &Triangle| (t[1].p - t[0].p).cross(t[2].p - t[0].p).length_squared() > 1e-16;
    inside.retain(whole);
    outside.retain(whole);
    (inside, outside)
}

/// The triangles within `half(corner)` of where `d` is zero (a line drawn on the surface, as wide
/// as that on either side), and the rest.
pub fn band<D: Fn(&Corner) -> f32 + Sync, W: Fn(&Corner) -> f32 + Sync>(triangles: &[Triangle], d: &D, half: &W) -> (Vec<Triangle>, Vec<Triangle>) {
    let (within, mut rest) = split(triangles, &|c: &Corner| d(c) - half(c));
    let (beyond, band) = split(&within, &|c: &Corner| d(c) + half(c));
    rest.extend(beyond);
    (band, rest)
}

/// The triangles whose middle passes `keep`, and the rest.
pub fn sift<K: Fn(&Corner) -> bool>(triangles: Vec<Triangle>, keep: &K) -> (Vec<Triangle>, Vec<Triangle>) {
    triangles.into_iter().partition(|t| {
        let n = (t[0].n + t[1].n + t[2].n).normalize_or(t[0].n);
        keep(&Corner { p: (t[0].p + t[1].p + t[2].p) / 3.0, n, open: 1.0 })
    })
}

/// Wound so that the face looks the way its normals do; `None` for a triangle without area.
fn wound(mut tri: Triangle) -> Option<Triangle> {
    let face = (tri[1].p - tri[0].p).cross(tri[2].p - tri[0].p);
    if face.length_squared() < 1e-14 {
        return None;
    }
    if face.dot(tri[0].n + tri[1].n + tri[2].n) < 0.0 {
        tri.swap(1, 2);
    }
    Some(tri)
}

/// A shape turned about the y axis: `profile` as (y, radius) points, joined in order; the normals
/// to the left of the way it runs (`flip`: to the right), smooth but where it folds sharply.
pub fn lathe(profile: &[(f32, f32)], segments: usize, flip: bool) -> Vec<Triangle> {
    let sign = if flip { -1.0 } else { 1.0 };
    let normal = |a: (f32, f32), b: (f32, f32)| Vec2::new(-(b.1 - a.1), b.0 - a.0).normalize_or_zero() * sign;
    let faces: Vec<Vec2> = profile.windows(2).map(|w| normal(w[0], w[1])).collect();
    // Each stretch's normal at its two ends: shared with its neighbour unless they fold.
    let at = |s: usize, end: usize| -> Vec2 {
        let other = if end == 0 { s.checked_sub(1) } else { (s + 1 < faces.len()).then_some(s + 1) };
        match other {
            Some(o) if faces[o].dot(faces[s]) > 0.7 => (faces[o] + faces[s]).normalize_or(faces[s]),
            _ => faces[s],
        }
    };
    let mut out = Vec::new();
    for s in 0..faces.len() {
        let (p0, p1) = (profile[s], profile[s + 1]);
        let (n0, n1) = (at(s, 0), at(s, 1));
        for k in 0..segments {
            let turn = |i: usize, p: (f32, f32), n: Vec2| {
                let a = std::f32::consts::TAU * i as f32 / segments as f32;
                Corner { p: Vec3::new(a.cos() * p.1, p.0, a.sin() * p.1), n: Vec3::new(a.cos() * n.y, n.x, a.sin() * n.y), open: 1.0 }
            };
            let (a, b, c, d) = (turn(k, p0, n0), turn(k + 1, p0, n0), turn(k + 1, p1, n1), turn(k, p1, n1));
            out.extend([[a, b, c], [a, c, d]].into_iter().filter_map(wound));
        }
    }
    out
}

/// A box between two corners, flat faced.
pub fn cuboid(lo: Vec3, hi: Vec3) -> Vec<Triangle> {
    let mut out = Vec::new();
    for axis in 0..3 {
        for side in [-1.0f32, 1.0] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut n = Vec3::ZERO;
            n[axis] = side;
            let corner = |a: f32, b: f32| {
                let mut p = Vec3::ZERO;
                p[axis] = if side > 0.0 { hi[axis] } else { lo[axis] };
                p[u] = a;
                p[v] = b;
                Corner { p, n, open: 1.0 }
            };
            let q = [corner(lo[u], lo[v]), corner(hi[u], lo[v]), corner(hi[u], hi[v]), corner(lo[u], hi[v])];
            out.extend([[q[0], q[1], q[2]], [q[0], q[2], q[3]]].into_iter().filter_map(wound));
        }
    }
    out
}

/// Moves and turns triangles: each point through `to`, each normal through its turning part.
pub fn moved(triangles: &[Triangle], to: glam::Mat4) -> Vec<Triangle> {
    triangles.iter().map(|t| t.map(|c| Corner { p: to.transform_point3(c.p), n: to.transform_vector3(c.n).normalize_or(c.n), open: c.open })).collect()
}
