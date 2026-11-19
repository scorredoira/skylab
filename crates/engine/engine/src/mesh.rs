use glam::{Vec3, Vec4};
use wgpu::util::DeviceExt;

/// The most a smooth skin strays from the curve through its points, metres: `skin` divides each
/// stretch between two of them until its pieces keep this close (or turn by `STEP` at most).
pub const SAG: f32 = 0.0006;
/// A piece of a skin's curve need not turn by less than this, radians (8°), however big it is.
const STEP: f32 = 8.0 * std::f32::consts::PI / 180.0;
/// How many times longer than the one before it a stretch must be to be taken as straight there.
const UNEVEN: f32 = 3.0;
/// Pieces a stretch is divided into at most.
const MOST_PIECES: usize = 6;
/// Faces of a skin that meet at more than this (50°) meet at an edge: the curve is not rounded
/// there and each side keeps its own normal. A hexagon stays a hexagon; an octagon is a circle.
pub const CREASE_ANGLE: f32 = 50.0 * std::f32::consts::PI / 180.0;

/// Builds triangle meshes: position, normal, colour with gloss in alpha (10 floats per vertex). Boxes
/// and lofts are flat-shaded; `skin` and `cylinder` shade smoothly.
#[derive(Default, Clone)]
pub struct MeshBuilder {
    pub data: Vec<f32>,
}

impl MeshBuilder {
    pub const STRIDE: u64 = 40;

    pub fn new() -> Self {
        Self::default()
    }

    /// Number of vertices so far.
    pub fn count(&self) -> u32 {
        (self.data.len() / 10) as u32
    }

    fn push(&mut self, p: Vec3, n: Vec3, c: Vec4) {
        self.data.extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z, c.x, c.y, c.z, c.w]);
    }

    pub fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec4) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        for p in [a, b, c] {
            self.push(p, n, color);
        }
    }

    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Vec4) {
        self.triangle(a, b, c, color);
        self.triangle(a, c, d, color);
    }

    fn tri_safe(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Vec4) {
        if (b - a).cross(c - a).length() > 1e-9 {
            self.triangle(a, b, c, color);
        }
    }

    /// Skin between closed rings of equal point count, with optional end caps.
    /// `color(ring, point)` gives the colour of each face.
    pub fn loft(&mut self, rings: &[Vec<Vec3>], color: impl Fn(usize, usize) -> Vec4, cap_start: bool, cap_end: bool, flip: bool) {
        let n = rings[0].len();
        for r in 0..rings.len() - 1 {
            for i in 0..n {
                let j = (i + 1) % n;
                let (a, b, c, d) = (rings[r][i], rings[r][j], rings[r + 1][j], rings[r + 1][i]);
                let col = color(r, i);
                if flip {
                    self.tri_safe(a, d, c, col);
                    self.tri_safe(a, c, b, col);
                } else {
                    self.tri_safe(a, b, c, col);
                    self.tri_safe(a, c, d, col);
                }
            }
        }
        let cap = |ring: &Vec<Vec3>, col: Vec4, reversed: bool, this: &mut Self| {
            let centre = ring.iter().copied().sum::<Vec3>() / n as f32;
            for i in 0..n {
                let j = (i + 1) % n;
                if reversed {
                    this.tri_safe(centre, ring[j], ring[i], col);
                } else {
                    this.tri_safe(centre, ring[i], ring[j], col);
                }
            }
        };
        if cap_start {
            cap(&rings[0], color(0, 0), !flip, self);
        }
        if cap_end {
            cap(&rings[rings.len() - 1], color(rings.len() - 2, 0), flip, self);
        }
    }

    /// One vertex with a normal of its own, for surfaces shaded smoothly (see `skin`).
    pub fn vertex(&mut self, p: Vec3, n: Vec3, c: Vec4) {
        self.push(p, n, c);
    }

    /// A flat polygon, as a fan of triangles from its centre: a cap for a `skin`.
    pub fn fan(&mut self, ring: &[Vec3], color: Vec4) {
        let centre = ring.iter().copied().sum::<Vec3>() / ring.len() as f32;
        for i in 0..ring.len() {
            self.tri_safe(centre, ring[i], ring[(i + 1) % ring.len()], color);
        }
    }

    /// Smooth-shaded skin through rings of equal point count. The rings are points of a smooth
    /// surface: between them it follows the curves through them within `SAG` (round each ring and
    /// from ring to ring), except where they turn by more than `CREASE_ANGLE` or at a crease, which
    /// stay sharp. Each vertex has the area-weighted average of the normals of the faces around its
    /// point, except across a crease or such an edge, where the faces on either side keep their
    /// own: curved skins look smooth, folds stay sharp.
    /// `color(ring, point)` gives the colour of each face, from `rings[ring][point]` to the next
    /// ring and the next point (of the rings given: the pieces between take its colour).
    /// Returns the rings as drawn, each with the points added round it: for caps (`fan`) that fit.
    pub fn skin(&mut self, rings: &[Vec<Vec3>], options: Skin, color: impl Fn(usize, usize) -> Vec4) -> Vec<Vec<Vec3>> {
        debug_assert!(rings.iter().all(|r| r.len() == rings[0].len()), "rings of different lengths");
        let fine = refine(rings, options);
        let creases: Vec<usize> = options.creases.iter().map(|&c| fine.first_of_point[c % rings[0].len()]).collect();
        let options = Skin { creases: &creases, ..options };
        let rings = &fine.grid;
        let (rows, n) = (rings.len(), rings[0].len());
        let columns = if options.closed { n } else { n - 1 };
        let row_count = if options.wrap { rows } else { rows - 1 };
        let at = |r: usize, i: usize| rings[r % rows][i % n];
        // Normal of each face, twice its area long.
        let face = |r: usize, i: usize| (at(r + 1, i + 1) - at(r, i)).cross(at(r + 1, i) - at(r, i + 1));
        let faces: Vec<Vec3> = (0..row_count).flat_map(|r| (0..columns).map(move |i| (r, i))).map(|(r, i)| face(r, i)).collect();
        let face_at = |r: isize, i: isize| -> Option<Vec3> {
            let r = if options.wrap { r.rem_euclid(rows as isize) } else if r < 0 || r >= row_count as isize { return None } else { r };
            let i = if options.closed { i.rem_euclid(n as isize) } else if i < 0 || i >= columns as isize { return None } else { i };
            Some(faces[r as usize * columns + i as usize])
        };
        // Normal of point (r, i) as seen from the face in row `row` and column `column`.
        let normal = |r: usize, i: usize, row: usize, column: usize| -> Vec3 {
            let crease = options.creases.contains(&(i % n));
            let own = face_at(row as isize, column as isize).unwrap_or(Vec3::ZERO);
            let mut sum = Vec3::ZERO;
            for dr in [-1isize, 0] {
                for di in [-1isize, 0] {
                    let c = i as isize + di;
                    if crease && c.rem_euclid(n as isize) != column as isize {
                        continue;
                    }
                    if let Some(f) = face_at(r as isize + dr, c).filter(|&f| !beyond_edge(f, own)) {
                        sum += f;
                    }
                }
            }
            let sum = if sum.length_squared() > 1e-20 { sum } else { own };
            let n = sum.normalize_or_zero();
            if options.flip { -n } else { n }
        };
        for r in 0..row_count {
            for i in 0..columns {
                let col = color(fine.row_of[r], fine.column_of[i]);
                let corner = |dr: usize, di: usize| (at(r + dr, i + di), normal((r + dr) % rows, (i + di) % n, r, i));
                let (a, b, c, d) = (corner(0, 0), corner(0, 1), corner(1, 1), corner(1, 0));
                let tris = if options.flip { [[a, d, c], [a, c, b]] } else { [[a, b, c], [a, c, d]] };
                for [p, q, s] in tris {
                    if (q.0 - p.0).cross(s.0 - p.0).length() > 1e-9 {
                        for (pos, nor) in [p, q, s] {
                            self.push(pos, nor, col);
                        }
                    }
                }
            }
        }
        fine.around
    }

    /// Axis-aligned box.
    pub fn cuboid(&mut self, a: Vec3, b: Vec3, color: Vec4) {
        let p = [
            Vec3::new(a.x, a.y, a.z), Vec3::new(b.x, a.y, a.z), Vec3::new(b.x, b.y, a.z), Vec3::new(a.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z), Vec3::new(b.x, a.y, b.z), Vec3::new(b.x, b.y, b.z), Vec3::new(a.x, b.y, b.z),
        ];
        self.quad(p[0], p[3], p[2], p[1], color);
        self.quad(p[4], p[5], p[6], p[7], color);
        self.quad(p[0], p[1], p[5], p[4], color);
        self.quad(p[2], p[3], p[7], p[6], color);
        self.quad(p[1], p[2], p[6], p[5], color);
        self.quad(p[0], p[4], p[7], p[3], color);
    }

    /// Cylinder or cone between two points, `sides` round it at least: a smooth skin, round within
    /// `SAG`, or a prism with sharp edges if it has six sides or fewer; flat ends.
    pub fn cylinder(&mut self, a: Vec3, b: Vec3, radius: f32, end_radius: f32, sides: usize, color: Vec4) {
        let axis = (b - a).normalize();
        let helper = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let u = axis.cross(helper).normalize();
        let v = axis.cross(u);
        let mut r0 = Vec::with_capacity(sides);
        let mut r1 = Vec::with_capacity(sides);
        for i in 0..sides {
            let t = i as f32 / sides as f32 * std::f32::consts::TAU;
            let d = t.cos() * u + t.sin() * v;
            r0.push(a + d * radius);
            r1.push(b + d * end_radius);
        }
        let rings = self.skin(&[r0, r1], Skin { closed: true, ..Skin::default() }, |_, _| color);
        self.fan(&rings[0].iter().rev().copied().collect::<Vec<_>>(), color);
        self.fan(&rings[1], color);
    }

    pub fn build(&self, device: &wgpu::Device) -> Mesh {
        Mesh { buffer: vertex_buffer(device, &self.data), count: self.count(), indices: None }
    }

    /// The same mesh with each distinct vertex stored once and the triangles as indices into them.
    /// A smooth skin shares each vertex among several triangles, and the GPU then runs the vertex
    /// shader about once per vertex instead of once per corner: worth it for meshes drawn many times
    /// over, like trees. The triangles are the same, in the same order.
    pub fn indexed(&self) -> (Vec<f32>, Vec<u32>) {
        let mut seen: std::collections::HashMap<[u32; 10], u32> = std::collections::HashMap::new();
        let (mut data, mut indices) = (Vec::new(), Vec::with_capacity(self.count() as usize));
        for v in self.data.as_chunks::<10>().0 {
            let key = v.map(f32::to_bits);
            let index = *seen.entry(key).or_insert_with(|| {
                data.extend_from_slice(v);
                (data.len() / 10 - 1) as u32
            });
            indices.push(index);
        }
        (data, indices)
    }

    /// As `build`, from `indexed`: draw it with `draw_indexed` and the index buffer in `Mesh::indices`.
    pub fn build_indexed(&self, device: &wgpu::Device) -> Mesh {
        let (data, indices) = self.indexed();
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Mesh { buffer: vertex_buffer(device, &data), count: indices.len() as u32, indices: Some(index_buffer) }
    }

    /// Vertex layout of these meshes, for a pipeline: position, normal, colour at locations 0, 1, 2.
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
        wgpu::VertexBufferLayout { array_stride: Self::STRIDE, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRS }
    }
}

/// How `MeshBuilder::skin` joins its rings.
#[derive(Clone, Copy, Debug, Default)]
pub struct Skin<'a> {
    /// The last point of each ring joins its first.
    pub closed: bool,
    /// The last ring joins the first, as in a torus.
    pub wrap: bool,
    /// Points of the rings, by index, where the surface folds along the rings' direction.
    pub creases: &'a [usize],
    /// Turns the faces and normals over.
    pub flip: bool,
}

/// Whether a face of normal `f` meets one of normal `own` at an edge (`CREASE_ANGLE`).
fn beyond_edge(f: Vec3, own: Vec3) -> bool {
    let (lf, lo) = (f.length(), own.length());
    lf > 1e-12 && lo > 1e-12 && f.dot(own) < lf * lo * CREASE_ANGLE.cos()
}

/// Angle between two directions of any length; 0 when either is none.
fn angle(a: Vec3, b: Vec3) -> f32 {
    let l = a.length() * b.length();
    if l < 1e-20 { 0.0 } else { (a.dot(b) / l).clamp(-1.0, 1.0).acos() }
}

/// The direction of a curve through `points` (the last joins the first if `closed`) coming into
/// each point and leaving it: the same where it is smooth, each chord's where it turns by more
/// than `CREASE_ANGLE` or `corner` says so; at an open end the next point's mirrored in the chord,
/// as a circle through the three would go.
fn tangents(points: &[Vec3], closed: bool, corner: impl Fn(usize) -> bool) -> Vec<(Vec3, Vec3)> {
    let n = points.len();
    let chord = |i: usize| -> Option<Vec3> { (closed || i + 1 < n).then(|| points[(i + 1) % n] - points[i]) };
    let before = |i: usize| if i > 0 { chord(i - 1) } else if closed { chord(n - 1) } else { None };
    let mut t: Vec<(Vec3, Vec3)> = (0..n)
        .map(|i| match (before(i), chord(i)) {
            (Some(a), Some(b)) => {
                let (ha, hb) = (a.length(), b.length());
                let (ua, ub) = (a.normalize_or_zero(), b.normalize_or_zero());
                if ha < 1e-9 || hb < 1e-9 {
                    let d = (ua + ub).normalize_or_zero();
                    (d, d)
                } else if corner(i) || angle(ua, ub) > CREASE_ANGLE {
                    (ua, ub)
                } else if hb > ha * UNEVEN {
                    // A long stretch after short ones is most likely straight, as where a rounded
                    // corner meets a side: the curve meets it along it, not bulging out over it.
                    (ub, ub)
                } else if ha > hb * UNEVEN {
                    (ua, ua)
                } else {
                    // Bessel's: the slope of the parabola through the three points.
                    let m = (ua * hb + ub * ha).normalize_or_zero();
                    (m, m)
                }
            }
            (Some(a), None) | (None, Some(a)) => {
                let d = a.normalize_or_zero();
                (d, d)
            }
            (None, None) => (Vec3::ZERO, Vec3::ZERO),
        })
        .collect();
    if !closed && n > 2 {
        let mirror = |c: Vec3, m: Vec3| (2.0 * c * c.dot(m) - m).normalize_or_zero();
        let first = mirror(t[0].1, t[1].0);
        let last = mirror(t[n - 1].0, t[n - 2].1);
        t[0] = (first, first);
        t[n - 1] = (last, last);
    }
    t
}

/// Pieces the stretch from `p0` (leaving along `t0`) to `p1` (arriving along `t1`) takes for each
/// to keep within `SAG` of the curve, or to turn by `STEP` at most.
fn pieces(p0: Vec3, p1: Vec3, t0: Vec3, t1: Vec3) -> usize {
    let d = p1 - p0;
    let h = d.length();
    let turn = angle(t0, d) + angle(d, t1);
    if h < 1e-7 || turn < 1e-4 {
        return 1;
    }
    // Less a hair, so that a stretch that needs just so many pieces is not given one more.
    let by_sag = ((h * turn / (8.0 * SAG)).sqrt() - 1e-3).ceil();
    let by_step = (turn / STEP - 1e-3).ceil();
    by_sag.min(by_step).clamp(1.0, MOST_PIECES as f32) as usize
}

/// The point `u` (0 to 1) of the way along the curve from `p0` to `p1`: a cubic leaving along `t0`
/// and arriving along `t1`, its handles as long as a circle's would be.
fn along(p0: Vec3, p1: Vec3, t0: Vec3, t1: Vec3, u: f32) -> Vec3 {
    let turn = angle(t0, t1).min(std::f32::consts::PI * 0.9);
    let handle = (p1 - p0).length() / 3.0 / (turn / 4.0).cos().powi(2);
    let (b1, b2) = (p0 + t0 * handle, p1 - t1 * handle);
    let v = 1.0 - u;
    p0 * (v * v * v) + b1 * (3.0 * v * v * u) + b2 * (3.0 * v * u * u) + p1 * (u * u * u)
}

/// `points` with points of the curve through them added, `counts[i]` pieces from point `i` to the
/// next; the last point too unless `closed`.
fn divide(points: &[Vec3], closed: bool, corner: impl Fn(usize) -> bool, counts: &[usize]) -> Vec<Vec3> {
    let n = points.len();
    let t = tangents(points, closed, corner);
    let mut out = Vec::with_capacity(counts.iter().sum::<usize>() + 1);
    for (i, &k) in counts.iter().enumerate() {
        let j = (i + 1) % n;
        out.push(points[i]);
        for s in 1..k {
            out.push(along(points[i], points[j], t[i].1, t[j].0, s as f32 / k as f32));
        }
    }
    if !closed {
        out.push(points[n - 1]);
    }
    out
}

/// The pieces each stretch of the curves through `lines` (all as long) takes: the most any needs.
fn counts(lines: &[Vec<Vec3>], closed: bool, corner: impl Fn(usize) -> bool + Copy) -> Vec<usize> {
    let n = lines[0].len();
    let stretches = if closed { n } else { n.saturating_sub(1) };
    let mut k = vec![1; stretches];
    for line in lines {
        let t = tangents(line, closed, corner);
        for (i, k) in k.iter_mut().enumerate() {
            let j = (i + 1) % n;
            *k = (*k).max(pieces(line[i], line[j], t[i].1, t[j].0));
        }
    }
    k
}

/// A skin's rings made finer: each ring round (`around`, as many as given), then rings added
/// between them (`grid`).
struct Fine {
    around: Vec<Vec<Vec3>>,
    grid: Vec<Vec<Vec3>>,
    /// The ring given that each row of faces of `grid` starts from, and the point each column does.
    row_of: Vec<usize>,
    column_of: Vec<usize>,
    /// Where each point given is in the finer rings.
    first_of_point: Vec<usize>,
}

fn refine(rings: &[Vec<Vec3>], options: Skin) -> Fine {
    let corner = |i: usize| options.creases.contains(&i);
    let around_counts = counts(rings, options.closed, corner);
    let around: Vec<Vec<Vec3>> = rings.iter().map(|r| divide(r, options.closed, corner, &around_counts)).collect();
    let column_of: Vec<usize> = around_counts.iter().enumerate().flat_map(|(i, &k)| std::iter::repeat_n(i, k)).collect();
    let first_of_point: Vec<usize> = (0..rings[0].len()).map(|i| around_counts[..i.min(around_counts.len())].iter().sum()).collect();
    // From ring to ring, along each line of points across them.
    let lines: Vec<Vec<Vec3>> = (0..around[0].len()).map(|j| around.iter().map(|r| r[j]).collect()).collect();
    let row_counts = counts(&lines, options.wrap, |_| false);
    let divided: Vec<Vec<Vec3>> = lines.iter().map(|l| divide(l, options.wrap, |_| false, &row_counts)).collect();
    let grid: Vec<Vec<Vec3>> = (0..divided[0].len()).map(|r| divided.iter().map(|l| l[r]).collect()).collect();
    let row_of = row_counts.iter().enumerate().flat_map(|(r, &k)| std::iter::repeat_n(r, k)).collect();
    Fine { around, grid, row_of, column_of, first_of_point }
}

fn vertex_buffer(device: &wgpu::Device, data: &[f32]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("mesh"), contents: bytemuck::cast_slice(data), usage: wgpu::BufferUsages::VERTEX })
}

/// A mesh on the GPU.
pub struct Mesh {
    pub buffer: wgpu::Buffer,
    /// Vertices to draw, or indices when `indices` is there.
    pub count: u32,
    /// `MeshBuilder::build_indexed`: the triangles as `u32` indices into `buffer`.
    pub indices: Option<wgpu::Buffer>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    fn vertices(b: &MeshBuilder) -> Vec<(Vec3, Vec3)> {
        b.data.chunks_exact(10).map(|v| (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]))).collect()
    }

    fn circle(z: f32, n: usize) -> Vec<Vec3> {
        (0..n).map(|i| i as f32 / n as f32 * std::f32::consts::TAU).map(|a| Vec3::new(a.cos(), a.sin(), z)).collect()
    }

    #[test]
    fn indexed_keeps_every_corner() {
        let mut b = MeshBuilder::new();
        b.skin(&[circle(0.0, 72), circle(1.0, 72), circle(2.0, 72)], Skin { closed: true, ..Skin::default() }, |_, _| Vec4::ONE);
        b.cuboid(Vec3::ZERO, Vec3::ONE, Vec4::ONE);
        let (data, indices) = b.indexed();
        assert_eq!(indices.len() as u32, b.count());
        // About the skin's 216 points (a circle as fine as `SAG` asks already; rounding can tell a
        // seam's two copies apart) and the box's 24 corners (each face has its own normal), out of
        // 900 corners.
        assert!((240..=244).contains(&(data.len() / 10)), "{} vertices", data.len() / 10);
        for (k, &i) in indices.iter().enumerate() {
            assert_eq!(&data[i as usize * 10..i as usize * 10 + 10], &b.data[k * 10..k * 10 + 10]);
        }
    }

    #[test]
    fn a_smooth_cylinder_has_radial_normals() {
        let mut b = MeshBuilder::new();
        b.skin(&[circle(0.0, 12), circle(1.0, 12), circle(2.0, 12)], Skin { closed: true, ..Skin::default() }, |_, _| Vec4::ONE);
        // Four pieces between each two points given (30° in steps of 8° at most), none between
        // the rings.
        assert_eq!(b.count(), 2 * 48 * 6);
        for (p, n) in vertices(&b) {
            let radial = Vec3::new(p.x, p.y, 0.0).normalize();
            assert!(n.dot(radial).abs() > 0.9999, "{p} has normal {n}");
        }
    }

    /// A wheel fairing's size: eight points round a circle of 16 cm come out round within `SAG`,
    /// through the points given, and the rings returned are the rings drawn.
    #[test]
    fn a_coarse_circle_comes_out_round() {
        let ring = |z: f32| circle(z, 8).iter().map(|p| Vec3::new(p.x * 0.16, p.y * 0.16, p.z)).collect::<Vec<_>>();
        let mut b = MeshBuilder::new();
        let drawn = b.skin(&[ring(0.0), ring(0.5)], Skin { closed: true, ..Skin::default() }, |_, _| Vec4::ONE);
        assert!(drawn[0].len() >= 24, "{} points round", drawn[0].len());
        assert!(ring(0.0).iter().all(|p| drawn[0].contains(p)), "the points given are kept");
        for (p, _) in vertices(&b) {
            let r = Vec2::new(p.x, p.y).length();
            assert!((r - 0.16).abs() < SAG, "{p} is {} m off the circle", r - 0.16);
        }
        for p in &drawn[1] {
            assert!(vertices(&b).iter().any(|(q, _)| q.distance(*p) < 1e-6), "{p} of the rings returned is not drawn");
        }
    }

    /// A rounded rectangle (a padded block's section): its straight sides stay straight where the
    /// rounded corners meet them, nothing bulging out past them.
    #[test]
    fn a_rounded_rectangle_keeps_its_sides_flat() {
        let (w, h, r) = (0.046f32, 0.0055f32, 0.003f32);
        let ring = |z: f32| -> Vec<Vec3> {
            [(1.0f32, 1.0f32), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
                .iter()
                .enumerate()
                .flat_map(|(k, &(sx, sy))| {
                    (0..=4).map(move |j| {
                        let a = (k as f32 + j as f32 / 4.0) * std::f32::consts::FRAC_PI_2;
                        Vec3::new(sx * (w - r) + a.cos() * r, sy * (h - r) + a.sin() * r, z)
                    })
                })
                .collect()
        };
        let mut b = MeshBuilder::new();
        b.skin(&[ring(0.0), ring(0.1)], Skin { closed: true, ..Skin::default() }, |_, _| Vec4::ONE);
        for (p, _) in vertices(&b) {
            assert!(p.x.abs() <= w + 1e-6 && p.y.abs() <= h + 1e-6, "{p} bulges out");
        }
    }

    /// A hexagonal prism keeps its six flat faces and sharp edges: it turns by more than
    /// `CREASE_ANGLE` at each corner.
    #[test]
    fn a_hexagon_stays_a_hexagon() {
        let mut b = MeshBuilder::new();
        b.skin(&[circle(0.0, 6), circle(1.0, 6)], Skin { closed: true, ..Skin::default() }, |_, _| Vec4::ONE);
        assert_eq!(b.count(), 6 * 6);
        for t in vertices(&b).chunks(3) {
            let face = (t[1].0 - t[0].0).cross(t[2].0 - t[0].0).normalize();
            assert!(t.iter().all(|(_, n)| n.dot(face) > 0.9999), "a rounded edge");
        }
    }

    /// A smooth cylinder of few sides comes out round; one of five stays a prism.
    #[test]
    fn cylinders_are_round_unless_they_have_few_sides() {
        let mut b = MeshBuilder::new();
        b.cylinder(Vec3::ZERO, Vec3::Z, 0.1, 0.1, 8, Vec4::ONE);
        let sides = vertices(&b).iter().filter(|(_, n)| n.z.abs() < 1e-3).count();
        assert!(sides >= 2 * 3 * 16, "{sides} corners round the side");
        let mut b = MeshBuilder::new();
        b.cylinder(Vec3::ZERO, Vec3::Z, 0.1, 0.1, 5, Vec4::ONE);
        assert_eq!(b.count(), 5 * 6 + 2 * 5 * 3);
    }

    #[test]
    fn a_crease_keeps_the_faces_on_either_side_flat() {
        // A square tube with creases at its four corners: every normal is a face's normal.
        let square = |z: f32| vec![Vec3::new(1.0, 1.0, z), Vec3::new(-1.0, 1.0, z), Vec3::new(-1.0, -1.0, z), Vec3::new(1.0, -1.0, z)];
        let mut b = MeshBuilder::new();
        b.skin(&[square(0.0), square(1.0)], Skin { closed: true, creases: &[0, 1, 2, 3], ..Skin::default() }, |_, _| Vec4::ONE);
        for (_, n) in vertices(&b) {
            assert!((n.x.abs() - 1.0).abs() < 1e-6 || (n.y.abs() - 1.0).abs() < 1e-6, "{n}");
        }
    }

    #[test]
    fn a_torus_wraps_without_a_seam() {
        let rings: Vec<Vec<Vec3>> = (0..16)
            .map(|k| {
                let a = k as f32 / 16.0 * std::f32::consts::TAU;
                circle(0.0, 8).iter().map(|p| {
                    let r = 2.0 + p.x * 0.5;
                    Vec3::new(r * a.cos(), r * a.sin(), p.y * 0.5)
                }).collect()
            })
            .collect();
        let mut b = MeshBuilder::new();
        b.skin(&rings, Skin { closed: true, wrap: true, ..Skin::default() }, |_, _| Vec4::ONE);
        assert!(b.count() > 16 * 8 * 6, "made finer");
        // All the normals point the same way, outwards or inwards as the winding says.
        let dots: Vec<f32> = vertices(&b).iter().map(|(p, n)| n.dot((*p - Vec3::new(p.x, p.y, 0.0).normalize() * 2.0).normalize())).collect();
        for d in &dots {
            assert!(d * dots[0].signum() > 0.97, "{d}");
        }
    }

    /// FNV-1a over the bytes of the vertex data.
    fn digest(b: &MeshBuilder) -> u64 {
        bytemuck::cast_slice::<f32, u8>(&b.data).iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x0100_0000_01b3))
    }

    /// Every flat shape, with the options the game's models use.
    fn flat_shapes() -> MeshBuilder {
        let mut b = MeshBuilder::new();
        let c = Vec4::new(0.3, 0.5, 0.7, 0.9);
        b.triangle(Vec3::ZERO, Vec3::X, Vec3::Y, c);
        b.quad(Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.2), Vec3::Y, c);
        b.cuboid(Vec3::new(-0.3, 0.1, -2.0), Vec3::new(1.7, 0.9, 0.4), c);
        let rings = [circle(0.0, 9), circle(1.0, 9), circle(2.5, 9).iter().map(|p| *p * 0.5).collect()];
        b.loft(&rings, |r, i| c * (r + i) as f32, true, true, false);
        b.loft(&rings, |_, _| c, true, false, true);
        b
    }

    #[test]
    fn the_flat_shapes_are_as_before() {
        // A box and a cylinder still give one normal per triangle.
        let mut b = MeshBuilder::new();
        b.cuboid(Vec3::ZERO, Vec3::ONE, Vec4::ONE);
        b.cylinder(Vec3::ZERO, Vec3::X, 0.5, 0.3, 6, Vec4::ONE);
        for t in vertices(&b).chunks(3) {
            assert!(t[0].1 == t[1].1 && t[1].1 == t[2].1);
        }
        // Byte for byte what the builder gave before smooth skins were added (the digest was taken
        // with the mesh.rs of commit 3235c25): every existing model is unchanged.
        let b = flat_shapes();
        assert_eq!((b.count(), digest(&b)), (342, 0xfe7a_94b1_daa0_9154));
    }
}
