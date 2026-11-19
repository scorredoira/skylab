//! How much of the sky each point inside a shape sees through its openings, baked once on the CPU
//! into a volume a shader reads: a room seen through its windows, a cabin, a cave.
//!
//! The triangles are laid into a grid of small cells; from each point of a coarser grid of samples,
//! rays in fixed directions walk the cells until they hit a triangle or leave the box (the sky).
//! Each sample keeps the first two terms of what it sees (`a`, the fraction of directions open, and
//! `b`, the mean of the open directions), so a surface facing `n` gets the light of its hemisphere
//! as `a + 2 b·n`: 1 in the open, 0 facing a wall, in between at the foot of one.
//!
//! ```ignore
//! let volume = SkyVolume::bake(&triangles, min, max, 0.03);
//! let texture = gpu.volume("sky", volume.size, &volume.texels);  // texture_3d<f32>: rgba = a, b·2 + 0.5
//! // WGSL: let s = textureSample(sky, samp, volume_uvw); let open = s.r + dot(s.gba - 0.5, n);
//! ```

use glam::{IVec3, Vec3};

/// The sky seen from a grid of points: `size` samples `step` apart from `origin`, each 4 bytes,
/// x fastest: red the fraction of directions open to the sky, green, blue and alpha the mean of the
/// open directions times 2, plus 0.5 (so that `red + dot(gba - 0.5, n)` is the light a surface
/// facing `n` gets, 0 to 1).
pub struct SkyVolume {
    pub origin: Vec3,
    pub step: f32,
    pub size: [u32; 3],
    pub texels: Vec<u8>,
}

/// Directions per sample.
const RAYS: usize = 96;

impl SkyVolume {
    /// Bakes the volume of the box `min`..`max` with samples `step` apart, the triangles laid into
    /// cells of half that. Takes a fraction of a second on every core for a box of a few metres at
    /// 3 cm. Samples inside a cell a triangle crosses (on a surface) take the mean of their open
    /// neighbours, so a shader reading near a surface does not see the surface's own darkness.
    pub fn bake(triangles: &[[Vec3; 3]], min: Vec3, max: Vec3, step: f32) -> SkyVolume {
        let grid = Grid::new(triangles, min, max, step / 2.0);
        let size = ((max - min) / step).ceil().as_uvec3() + 1;
        let size = [size.x, size.y, size.z];
        let count = (size[0] * size[1] * size[2]) as usize;
        let directions = fibonacci_sphere(RAYS);
        let index = |i: usize| -> (u32, u32, u32) { ((i as u32) % size[0], (i as u32 / size[0]) % size[1], i as u32 / (size[0] * size[1])) };
        let point = |i: usize| {
            let (x, y, z) = index(i);
            min + Vec3::new(x as f32, y as f32, z as f32) * step
        };

        // (open fraction, mean open direction), or None on a surface.
        let mut seen: Vec<Option<(f32, Vec3)>> = vec![None; count];
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chunk = count.div_ceil(threads);
        crate::threads::scope(|scope| {
            for (k, part) in seen.chunks_mut(chunk).enumerate() {
                let (grid, directions) = (&grid, &directions);
                scope.spawn(move || {
                    for (j, out) in part.iter_mut().enumerate() {
                        let p = point(k * chunk + j);
                        if grid.solid(grid.cell(p)) {
                            continue;
                        }
                        let (mut open, mut mean) = (0.0, Vec3::ZERO);
                        for &d in directions {
                            if grid.escapes(p, d) {
                                open += 1.0;
                                mean += d;
                            }
                        }
                        *out = Some((open / RAYS as f32, mean / RAYS as f32));
                    }
                });
            }
        });

        // Samples on a surface: the mean of their open neighbours, twice over for thick walls.
        for _ in 0..2 {
            let before = seen.clone();
            for (i, out) in seen.iter_mut().enumerate().filter(|(_, s)| s.is_none()) {
                let (x, y, z) = index(i);
                let (mut sum, mut n) = ((0.0, Vec3::ZERO), 0.0);
                for (dx, dy, dz) in neighbours() {
                    let (nx, ny, nz) = (x as i32 + dx, y as i32 + dy, z as i32 + dz);
                    if nx < 0 || ny < 0 || nz < 0 || nx >= size[0] as i32 || ny >= size[1] as i32 || nz >= size[2] as i32 {
                        continue;
                    }
                    if let Some((a, b)) = before[(nx as u32 + size[0] * (ny as u32 + size[1] * nz as u32)) as usize] {
                        sum = (sum.0 + a, sum.1 + b);
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    *out = Some((sum.0 / n, sum.1 / n));
                }
            }
        }

        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let texels = seen
            .iter()
            .flat_map(|s| {
                let (a, b) = s.unwrap_or((0.0, Vec3::ZERO));
                [byte(a), byte(b.x * 2.0 + 0.5), byte(b.y * 2.0 + 0.5), byte(b.z * 2.0 + 0.5)]
            })
            .collect();
        SkyVolume { origin: min, step, size, texels }
    }

    /// The light a surface at `p` facing `n` gets, 0 to 1, from the nearest sample (as the shader
    /// reads it, without blending between samples).
    pub fn light(&self, p: Vec3, n: Vec3) -> f32 {
        let i = ((p - self.origin) / self.step).round().as_ivec3().clamp(IVec3::ZERO, IVec3::new(self.size[0] as i32 - 1, self.size[1] as i32 - 1, self.size[2] as i32 - 1));
        let at = 4 * (i.x as u32 + self.size[0] * (i.y as u32 + self.size[1] * i.z as u32)) as usize;
        let t = |k: usize| self.texels[at + k] as f32 / 255.0;
        (t(0) + (t(1) - 0.5) * n.x + (t(2) - 0.5) * n.y + (t(3) - 0.5) * n.z).clamp(0.0, 1.0)
    }
}

/// How open each of `points` (a point on a surface and the way the surface faces) is to what is
/// round it, 0 to 1: the share of its hemisphere, weighted by the cosine, that reaches `reach`
/// metres without meeting one of `triangles`. Dark in the corners, at the foot of a knob, under a
/// lip: the occlusion of the light that comes from everywhere, which `SkyVolume`'s 3 cm do not
/// see. The triangles are laid into cells of `cell` within `min`..`max`; each point casts 48 rays,
/// from a cell and a half off its surface. A fraction of a second on every core for 100 000 points.
pub fn ambient_occlusion(triangles: &[[Vec3; 3]], points: &[(Vec3, Vec3)], min: Vec3, max: Vec3, cell: f32, reach: f32) -> Vec<f32> {
    let grid = Grid::new(triangles, min, max, cell);
    let directions = fibonacci_sphere(96);
    let mut open = vec![1.0f32; points.len()];
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = points.len().div_ceil(threads).max(1);
    crate::threads::scope(|scope| {
        for (out, points) in open.chunks_mut(chunk).zip(points.chunks(chunk)) {
            let (grid, directions) = (&grid, &directions);
            scope.spawn(move || {
                for (out, &(p, n)) in out.iter_mut().zip(points) {
                    let n = n.normalize_or_zero();
                    let start = p + n * (cell * 1.5);
                    let (mut seen, mut all) = (0.0, 0.0);
                    for &d in directions {
                        let w = d.dot(n);
                        if w <= 0.0 {
                            continue;
                        }
                        all += w;
                        if grid.reaches(start, d, reach) {
                            seen += w;
                        }
                    }
                    *out = if all > 0.0 { seen / all } else { 1.0 };
                }
            });
        }
    });
    open
}

fn neighbours() -> impl Iterator<Item = (i32, i32, i32)> {
    (-1..=1).flat_map(|dz| (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| (dx, dy, dz)))).filter(|&d| d != (0, 0, 0))
}

/// `n` directions spread evenly over the sphere.
fn fibonacci_sphere(n: usize) -> Vec<Vec3> {
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    (0..n)
        .map(|i| {
            let z = 1.0 - (i as f32 + 0.5) / n as f32 * 2.0;
            let r = (1.0 - z * z).sqrt();
            let a = golden * i as f32;
            Vec3::new(r * a.cos(), r * a.sin(), z)
        })
        .collect()
}

/// The cells of the box, and which of them a triangle crosses.
struct Grid {
    min: Vec3,
    cell: f32,
    size: IVec3,
    solid: Vec<bool>,
}

impl Grid {
    fn new(triangles: &[[Vec3; 3]], min: Vec3, max: Vec3, cell: f32) -> Grid {
        let size = ((max - min) / cell).ceil().as_ivec3().max(IVec3::ONE);
        let mut grid = Grid { min, cell, size, solid: vec![false; (size.x * size.y * size.z) as usize] };
        // Points over each triangle closer than half a cell: every cell it crosses gets one.
        for [a, b, c] in triangles {
            let longest = (*b - *a).length().max((*c - *b).length()).max((*a - *c).length());
            let n = (longest / (cell * 0.5)).ceil().max(1.0) as u32;
            for i in 0..=n {
                for j in 0..=n - i {
                    let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                    let cell = grid.cell(*a + (*b - *a) * u + (*c - *a) * v);
                    if let Some(k) = grid.index(cell) {
                        grid.solid[k] = true;
                    }
                }
            }
        }
        grid
    }

    fn cell(&self, p: Vec3) -> IVec3 {
        ((p - self.min) / self.cell).floor().as_ivec3()
    }

    fn index(&self, c: IVec3) -> Option<usize> {
        (c.cmpge(IVec3::ZERO).all() && c.cmplt(self.size).all()).then(|| (c.x + self.size.x * (c.y + self.size.y * c.z)) as usize)
    }

    fn solid(&self, c: IVec3) -> bool {
        self.index(c).is_some_and(|k| self.solid[k])
    }

    /// Whether a ray from `p` along `d` goes `reach` metres, or leaves the box, without crossing a
    /// solid cell.
    fn reaches(&self, p: Vec3, d: Vec3, reach: f32) -> bool {
        let mut cell = self.cell(p);
        if self.solid(cell) {
            return false;
        }
        let step = IVec3::new(d.x.signum() as i32, d.y.signum() as i32, d.z.signum() as i32);
        let local = (p - self.min) / self.cell;
        let next = |i: usize| {
            let (c, l, dir) = (cell[i] as f32, local[i], d[i]);
            if dir > 0.0 { (c + 1.0 - l) / dir } else if dir < 0.0 { (l - c) / -dir } else { f32::INFINITY }
        };
        let mut t_max = Vec3::new(next(0), next(1), next(2));
        let t_delta = Vec3::new(1.0 / d.x.abs(), 1.0 / d.y.abs(), 1.0 / d.z.abs());
        let cells = reach / self.cell;
        loop {
            let axis = if t_max.x < t_max.y && t_max.x < t_max.z { 0 } else if t_max.y < t_max.z { 1 } else { 2 };
            if t_max[axis] > cells {
                return true;
            }
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            match self.index(cell) {
                None => return true,
                Some(k) if self.solid[k] => return false,
                _ => {}
            }
        }
    }

    /// Whether a ray from `p` along `d` leaves the box without crossing a solid cell: the cells are
    /// walked one by one (Amanatides and Woo), so no thin surface is stepped over.
    fn escapes(&self, p: Vec3, d: Vec3) -> bool {
        let mut cell = self.cell(p);
        let step = IVec3::new(d.x.signum() as i32, d.y.signum() as i32, d.z.signum() as i32);
        let local = (p - self.min) / self.cell;
        let next = |i: usize| {
            let (c, l, dir) = (cell[i] as f32, local[i], d[i]);
            if dir > 0.0 { (c + 1.0 - l) / dir } else if dir < 0.0 { (l - c) / -dir } else { f32::INFINITY }
        };
        let mut t_max = Vec3::new(next(0), next(1), next(2));
        let t_delta = Vec3::new(1.0 / d.x.abs(), 1.0 / d.y.abs(), 1.0 / d.z.abs());
        loop {
            let axis = if t_max.x < t_max.y && t_max.x < t_max.z { 0 } else if t_max.y < t_max.z { 1 } else { 2 };
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            match self.index(cell) {
                None => return true,
                Some(k) if self.solid[k] => return false,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The six faces of a box as triangles, inward or outward does not matter, with a square hole
    /// in the top if `hole`.
    fn room(hole: bool) -> Vec<[Vec3; 3]> {
        let mut t = Vec::new();
        let mut quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| {
            t.push([a, b, c]);
            t.push([a, c, d]);
        };
        let v = Vec3::new;
        // Floor at z = 0, walls up to z = 1, x and y from 0 to 1.
        quad(v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.));
        quad(v(0., 0., 0.), v(1., 0., 0.), v(1., 0., 1.), v(0., 0., 1.));
        quad(v(0., 1., 0.), v(1., 1., 0.), v(1., 1., 1.), v(0., 1., 1.));
        quad(v(0., 0., 0.), v(0., 1., 0.), v(0., 1., 1.), v(0., 0., 1.));
        quad(v(1., 0., 0.), v(1., 1., 0.), v(1., 1., 1.), v(1., 0., 1.));
        if hole {
            // The roof round a hole from 0.3 to 0.7.
            quad(v(0., 0., 1.), v(1., 0., 1.), v(1., 0.3, 1.), v(0., 0.3, 1.));
            quad(v(0., 0.7, 1.), v(1., 0.7, 1.), v(1., 1., 1.), v(0., 1., 1.));
            quad(v(0., 0.3, 1.), v(0.3, 0.3, 1.), v(0.3, 0.7, 1.), v(0., 0.7, 1.));
            quad(v(0.7, 0.3, 1.), v(1., 0.3, 1.), v(1., 0.7, 1.), v(0.7, 0.7, 1.));
        } else {
            quad(v(0., 0., 1.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 1.));
        }
        t
    }

    #[test]
    fn the_occlusion_is_dark_in_a_corner_and_open_in_the_middle_of_a_floor() {
        let (min, max) = (Vec3::splat(-0.2), Vec3::splat(1.2));
        let points = [(Vec3::new(0.5, 0.5, 0.0), Vec3::Z), (Vec3::new(0.03, 0.03, 0.0), Vec3::Z)];
        let open = ambient_occlusion(&room(false), &points, min, max, 0.01, 0.3);
        assert!(open[0] > 0.99, "middle {}", open[0]);
        assert!(open[1] < 0.6, "corner {}", open[1]);
    }

    #[test]
    fn a_closed_room_is_dark_and_a_skylight_lights_the_floor_under_it() {
        let (min, max) = (Vec3::splat(-0.2), Vec3::splat(1.2));
        let closed = SkyVolume::bake(&room(false), min, max, 0.05);
        assert!(closed.light(Vec3::new(0.5, 0.5, 0.5), Vec3::Z) < 0.01);
        let open = SkyVolume::bake(&room(true), min, max, 0.05);
        let floor = open.light(Vec3::new(0.5, 0.5, 0.1), Vec3::Z);
        let corner = open.light(Vec3::new(0.1, 0.1, 0.1), Vec3::Z);
        // A 0.4 m hole 0.9 m up is about 6 % of the floor's sky: what a cosine-weighted hemisphere sees of it.
        let facing_down = open.light(Vec3::new(0.5, 0.5, 0.1), -Vec3::Z);
        assert!(floor > 0.04 && floor > 5.0 * corner, "floor {floor}, corner {corner}");
        assert!(facing_down < 0.05, "{facing_down}");
        // Outside the room, the open sky.
        assert!(open.light(Vec3::new(0.5, 0.5, 1.15), Vec3::Z) > 0.9);
    }
}
