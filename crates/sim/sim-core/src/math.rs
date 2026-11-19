//! Units, small numeric helpers, attitude conversions, the deterministic random generator and the
//! noise the terrain is made of.
//!
//! The hash and noise functions are bit-exact: the world generated from a seed depends on them.

use glam::{DQuat, DVec3};

pub type V3 = DVec3;
pub type Quat = DQuat;

pub const GRAVITY: f64 = 9.80665;
/// m/s per knot.
pub const KNOTS: f64 = 0.514444;
/// m per foot.
pub const FEET: f64 = 0.3048;
/// Radians per degree.
pub const DEG: f64 = std::f64::consts::PI / 180.0;

#[inline(always)]
pub fn clamp(x: f64, a: f64, b: f64) -> f64 {
    x.max(a).min(b)
}

#[inline(always)]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline(always)]
pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear interpolation in a table with evenly or unevenly spaced abscissas.
pub fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if x <= xs[0] {
        return ys[0];
    }
    let n = xs.len();
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let mut i = 1;
    while xs[i] < x {
        i += 1;
    }
    let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
    ys[i - 1] + (ys[i] - ys[i - 1]) * t
}

/// IEEE 754 remainder: `x - n * y` with `n` the integer nearest to `x / y` (ties to even).
/// The result is in `[-|y| / 2, |y| / 2]`.
pub fn remainder(x: f64, y: f64) -> f64 {
    let y = y.abs();
    let r = x % y;
    let half = 0.5 * y;
    let a = r.abs();
    if a > half || (a == half && (x / y).trunc() % 2.0 != 0.0) {
        r - y.copysign(r)
    } else {
        r
    }
}

// MARK: - Vectors and quaternions
//
// Quaternions are `glam::DQuat` (x, y, z, w) and rotate body axes into world axes. The helpers
// below are used instead of glam's operators where the integrator handles quaternions that are
// not exactly unit length, so that the arithmetic is the same as in the reference implementation.

/// Unit vector along `v`.
#[inline(always)]
pub fn normalize(v: V3) -> V3 {
    v * (1.0 / v.length_squared().sqrt())
}

/// Rotation of `angle` radians about the unit vector `axis`.
#[inline(always)]
pub fn axis_angle(angle: f64, axis: V3) -> Quat {
    let half = angle / 2.0;
    let s = half.sin();
    Quat::from_xyzw(axis.x * s, axis.y * s, axis.z * s, half.cos())
}

/// Rotates `v` by `q`.
#[inline(always)]
pub fn act(q: Quat, v: V3) -> V3 {
    let imag = V3::new(q.x, q.y, q.z);
    let t = 2.0 * imag.cross(v);
    v + q.w * t + imag.cross(t)
}

/// Inverse of a quaternion that may not be of unit length.
#[inline(always)]
pub fn inverse(q: Quat) -> Quat {
    let k = 1.0 / (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    Quat::from_xyzw(-q.x * k, -q.y * k, -q.z * k, q.w * k)
}

/// Heading, pitch and roll, in radians.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Euler {
    /// In [0, 2π), from north towards east.
    pub heading: f64,
    /// Positive nose up.
    pub pitch: f64,
    /// Positive right wing down.
    pub roll: f64,
}

/// Attitude as heading, pitch and roll (radians) of a body→NED quaternion.
pub fn euler_angles(q: Quat) -> Euler {
    let f = act(q, V3::new(1.0, 0.0, 0.0));
    let r = act(q, V3::new(0.0, 1.0, 0.0));
    let d = act(q, V3::new(0.0, 0.0, 1.0));
    let pitch = -clamp(f.z, -1.0, 1.0).asin();
    let mut heading = f.y.atan2(f.x);
    if heading < 0.0 {
        heading += 2.0 * std::f64::consts::PI;
    }
    let roll = r.z.atan2(d.z);
    Euler {
        heading,
        pitch,
        roll,
    }
}

/// Body→NED quaternion of an attitude given as heading, pitch and roll (radians).
pub fn quaternion(heading: f64, pitch: f64, roll: f64) -> Quat {
    let qz = axis_angle(heading, V3::new(0.0, 0.0, 1.0));
    let qy = axis_angle(pitch, V3::new(0.0, 1.0, 0.0));
    let qx = axis_angle(roll, V3::new(1.0, 0.0, 0.0));
    qz * qy * qx
}

// MARK: - Deterministic random numbers

#[derive(Clone, Debug)]
pub struct Rng {
    s: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng {
            s: seed.wrapping_add(0x9E3779B97F4A7C15),
        };
        r.next();
        r
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        self.s ^= self.s << 13;
        self.s ^= self.s >> 7;
        self.s ^= self.s << 17;
        self.s
    }

    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }

    /// Standard normal.
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform().max(1e-12);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

// MARK: - Noise

/// Hash of an integer lattice point, uniform in [0, 1).
#[inline(always)]
pub fn hash2(x: i64, y: i64, s: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (y as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ s;
    h ^= h >> 32;
    h = h.wrapping_mul(0xD6E8FEB86659FD93);
    h ^= h >> 32;
    h = h.wrapping_mul(0xD6E8FEB86659FD93);
    h ^= h >> 32;
    (h >> 11) as f64 * (1.0 / 9007199254740992.0)
}

/// Value of the noise and its gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub v: f64,
    pub dx: f64,
    pub dy: f64,
}

/// Value noise in [-1, 1] with its analytic gradient.
#[inline(always)]
pub fn noised(x: f64, y: f64, seed: u64) -> Noise {
    let fx0 = x.floor();
    let fy0 = y.floor();
    let ix = fx0 as i64;
    let iy = fy0 as i64;
    let fx = x - fx0;
    let fy = y - fy0;
    let a = hash2(ix, iy, seed);
    let b = hash2(ix + 1, iy, seed);
    let c = hash2(ix, iy + 1, seed);
    let d = hash2(ix + 1, iy + 1, seed);
    let u = fx * fx * fx * (fx * (fx * 6.0 - 15.0) + 10.0);
    let v = fy * fy * fy * (fy * (fy * 6.0 - 15.0) + 10.0);
    let du = 30.0 * fx * fx * (fx * (fx - 2.0) + 1.0);
    let dv = 30.0 * fy * fy * (fy * (fy - 2.0) + 1.0);
    let k = a - b - c + d;
    let value = a + (b - a) * u + (c - a) * v + k * u * v;
    Noise {
        v: value * 2.0 - 1.0,
        dx: 2.0 * du * ((b - a) + k * v),
        dy: 2.0 * dv * ((c - a) + k * u),
    }
}

/// Fractal sum of value noise, roughly in [-1, 1].
pub fn fbm(x: f64, y: f64, octaves: u32, seed: u64) -> f64 {
    let (mut px, mut py, mut amp, mut sum, mut norm) = (x, y, 0.5, 0.0, 0.0);
    for i in 0..octaves {
        sum += amp * noised(px, py, seed.wrapping_add(i as u64)).v;
        norm += amp;
        let nx = 1.6 * px - 1.2 * py;
        let ny = 1.2 * px + 1.6 * py;
        px = nx;
        py = ny;
        amp *= 0.5;
    }
    sum / norm
}
