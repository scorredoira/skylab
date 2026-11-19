//! The inside of the car: the dashboard with the hood over the dials, the centre stack and the
//! tunnel, two bucket seats, the floor and the shelf behind them, the mirror; the steering wheel
//! and the dials' faces as parts of their own, and the driver's gloved hands on the wheel (each a
//! part, in the wheel's axes: the game turns them with it) and a forearm (one, drawn twice from
//! elbow to wrist). The shell's inner side (pillars, roof, doors) is the shell itself seen from
//! within. Design axes as the body's; the driver sits on the left.

use glam::{Vec2, Vec3};

use crate::sdf::{capsule, cuboid, openness, round_box, smax, smin, smoothstep, split, surface, Corner, Triangle};

/// Where the driver sits across the car, and where the steering wheel's and the dials' centres are.
pub const DRIVER_Y: f32 = -0.37;
pub const STEERING: Vec3 = Vec3::new(0.24, DRIVER_Y, 0.795);
pub const DIAL_RADIUS: f32 = 0.062;
pub const DIAL_REV: Vec3 = Vec3::new(0.425, DRIVER_Y - 0.072, 0.808);
pub const DIAL_SPEED: Vec3 = Vec3::new(0.425, DRIVER_Y + 0.072, 0.808);

fn dash(p: Vec3) -> f32 {
    let main = round_box(p - Vec3::new(0.60, 0.0, 0.735), Vec3::new(0.20, 0.82, 0.115), 0.05);
    let hood = round_box(p - Vec3::new(0.44, DRIVER_Y, 0.848), Vec3::new(0.11, 0.19, 0.04), 0.035);
    let stack = round_box(p - Vec3::new(0.37, 0.0, 0.60), Vec3::new(0.07, 0.14, 0.16), 0.03);
    let tunnel = round_box(p - Vec3::new(-0.15, 0.0, 0.36), Vec3::new(0.60, 0.10, 0.13), 0.035);
    let d = smin(smin(smin(main, hood, 0.04), stack, 0.04), tunnel, 0.05);
    // The hollow the dials sit in, under the hood.
    smax(d, -round_box(p - Vec3::new(0.37, DRIVER_Y, 0.808), Vec3::new(0.06, 0.155, 0.052), 0.02), 0.01)
}

fn seat(p: Vec3, y: f32) -> f32 {
    let cushion = round_box(p - Vec3::new(-0.33, y, 0.35), Vec3::new(0.27, 0.25, 0.07), 0.05);
    // The back, leaning 14° about the cushion's rear edge, narrowing to the head.
    let (s, c) = 0.245f32.sin_cos();
    let r = p - Vec3::new(-0.58, y, 0.37);
    let q = Vec3::new(r.x * c + r.z * s, r.y, -r.x * s + r.z * c);
    let half = 0.25 - 0.09 * smoothstep(0.42, 0.62, q.z);
    let back = round_box(q - Vec3::new(0.0, 0.0, 0.37), Vec3::new(0.06, half, 0.39), 0.05);
    let mut d = smin(cushion, back, 0.06);
    for side in [-1.0f32, 1.0] {
        d = smin(d, capsule(p, Vec3::new(-0.55, y + side * 0.215, 0.43), Vec3::new(-0.10, y + side * 0.215, 0.42), 0.045), 0.05);
        d = smin(d, capsule(q, Vec3::new(0.05, side * 0.20, 0.10), Vec3::new(0.06, side * 0.20, 0.50), 0.05), 0.05);
    }
    d
}

fn floor(p: Vec3) -> f32 {
    let floor = round_box(p - Vec3::new(-0.10, 0.0, 0.21), Vec3::new(1.0, 0.86, 0.03), 0.005);
    let shelf = round_box(p - Vec3::new(-1.30, 0.0, 0.60), Vec3::new(0.40, 0.80, 0.31), 0.04);
    floor.min(shelf)
}

fn mirror(p: Vec3) -> f32 {
    let glass = round_box(p - Vec3::new(0.10, 0.0, 1.172), Vec3::new(0.016, 0.105, 0.03), 0.012);
    smin(glass, capsule(p, Vec3::new(0.11, 0.0, 1.19), Vec3::new(0.07, 0.0, 1.26), 0.012), 0.01)
}

/// The cabin's triangles by material.
pub fn cabin() -> Vec<(&'static str, Vec<Triangle>)> {
    let seats = |p: Vec3| seat(p, DRIVER_Y).min(seat(p, -DRIVER_Y));
    let field = |p: Vec3| dash(p).min(seats(p)).min(floor(p)).min(mirror(p));
    let open = |p: Vec3, n: Vec3| openness(&field, p, n);
    let all = surface(&field, Vec3::new(-1.75, -0.88, 0.17), Vec3::new(0.86, 0.88, 1.28), 0.024, &open);
    let (seat, rest) = split(&all, &|c: &Corner| seats(c.p) - dash(c.p).min(floor(c.p)).min(mirror(c.p)));
    let (carpet, rest) = split(&rest, &|c: &Corner| floor(c.p) - dash(c.p).min(mirror(c.p)));
    let (mirror_all, dash) = split(&rest, &|c: &Corner| mirror(c.p) - dash(c.p));
    let (glass, housing) = split(&mirror_all, &|c: &Corner| (c.n.x + 0.9) * 0.1 + (c.p.z - 1.21).max(0.0));
    // A screen on the centre stack, facing the seats.
    let face = |y: f32, z: f32| Corner { p: Vec3::new(0.2985, y, z), n: Vec3::NEG_X, open: 1.0 };
    let q = [face(-0.10, 0.60), face(0.10, 0.60), face(0.10, 0.715), face(-0.10, 0.715)];
    let screen = vec![[q[0], q[1], q[2]], [q[0], q[2], q[3]]];
    vec![("seat", seat), ("carpet", carpet), ("mirror", glass), ("trim", housing), ("dash", dash), ("screen", screen)]
}

/// The steering wheel about its centre, its rim in the plane x = 0 and its column along +x: the
/// rim, three spokes, the hub's pad, a mark at twelve o'clock, the shift paddles behind it.
pub fn steering() -> Vec<(&'static str, Vec<Triangle>)> {
    let field = |p: Vec3| {
        let rim = Vec2::new(p.y.hypot(p.z) - 0.175, p.x).length() - 0.0165;
        let mut d = rim;
        for side in [-1.0f32, 1.0] {
            d = smin(d, round_box(p - Vec3::new(0.012, side * 0.095, -0.004), Vec3::new(0.008, 0.085, 0.021), 0.008), 0.014);
        }
        d = smin(d, round_box(p - Vec3::new(0.012, 0.0, -0.095), Vec3::new(0.008, 0.019, 0.085), 0.008), 0.014);
        d = smin(d, round_box(p - Vec3::new(0.016, 0.0, -0.004), Vec3::new(0.028, 0.068, 0.05), 0.024), 0.014);
        d.min(capsule(p, Vec3::new(0.05, 0.0, 0.0), Vec3::new(0.34, 0.0, 0.0), 0.027))
    };
    let open = |p: Vec3, n: Vec3| openness(&field, p, n).max(0.5);
    let all = surface(&field, Vec3::new(-0.03, -0.20, -0.20), Vec3::new(0.40, 0.20, 0.20), 0.007, &open);
    let (mark, wheel) = split(&all, &|c: &Corner| (c.p.y.abs() - 0.011).max(0.15 - c.p.z));
    let mut paddles = Vec::new();
    for side in [-1.0f32, 1.0] {
        let (a, b) = (side * 0.075, side * 0.155);
        paddles.extend(cuboid(Vec3::new(0.034, a.min(b), -0.03), Vec3::new(0.039, a.max(b), 0.05)));
    }
    vec![("leather", wheel), ("marker", mark), ("carbon", paddles)]
}

/// Where a hand holds the rim, in the wheel's axes: at a quarter to three, a little above its middle.
pub const GRIP: Vec3 = Vec3::new(0.0, 0.172, 0.03);

/// A gloved hand on the wheel's rim, in the wheel's axes (the rim in the plane x = 0, the driver
/// at −x): `side` −1 the left one, 1 the right. The back of the hand outside the rim, four
/// fingers closed round it from the driver's side, the thumb lying up along it, a cuff towards
/// the forearm.
pub fn hand(side: f32) -> Vec<(&'static str, Vec<Triangle>)> {
    let at = Vec3::new(GRIP.x, side * GRIP.y, GRIP.z);
    let field = |p: Vec3| {
        // In the hand's own axes: y out from the wheel's middle.
        let q = Vec3::new(p.x - at.x, side * (p.y - at.y), p.z - at.z);
        let back = round_box(q - Vec3::new(-0.004, 0.031, -0.004), Vec3::new(0.026, 0.017, 0.046), 0.015);
        // The fingers: a roll round the rim's near side, parted in four.
        let roll = round_box(q - Vec3::new(-0.026, 0.004, -0.002), Vec3::new(0.014, 0.031, 0.043), 0.013);
        let parted = roll + 0.004 * (1.0 - ((q.z + 0.002) / 0.0215 * std::f32::consts::PI).cos().abs()) * smoothstep(-0.02, -0.03, q.x);
        let tips = round_box(q - Vec3::new(0.004, -0.024, -0.002), Vec3::new(0.022, 0.011, 0.041), 0.010);
        let thumb = capsule(q, Vec3::new(-0.018, 0.012, 0.042), Vec3::new(-0.022, -0.002, 0.082), 0.0125);
        let cuff = capsule(q, Vec3::new(-0.016, 0.038, -0.045), Vec3::new(-0.052, 0.060, -0.100), 0.031);
        smin(smin(smin(smin(back, parted, 0.012), tips, 0.010), thumb, 0.012), cuff, 0.02)
    };
    let open = |p: Vec3, n: Vec3| openness(&field, p, n).max(0.45);
    let all = surface(&field, at - Vec3::new(0.11, 0.12, 0.15), at + Vec3::new(0.07, 0.12, 0.12), 0.0045, &open);
    // The cuff is the suit's sleeve; a band of the glove's colour across the knuckles.
    let (sleeve, glove) = split(&all, &|c: &Corner| c.p.z - at.z + 0.058 + 0.5 * (c.p.x - at.x));
    let (band, glove) = split(&glove, &|c: &Corner| ((c.p.z - at.z - 0.012).abs() - 0.012).max(0.036 - side * (c.p.y - at.y)));
    vec![("glove", glove), ("glove_band", band), ("sleeve", sleeve)]
}

/// A forearm: a sleeve a metre long along +x from the origin, which the game stretches from the
/// wrist to the elbow.
pub fn forearm() -> Vec<(&'static str, Vec<Triangle>)> {
    let profile: Vec<(f32, f32)> = vec![(0.0, 0.0), (0.0, 0.034), (0.25, 0.040), (0.7, 0.047), (1.0, 0.05), (1.0, 0.0)];
    // `lathe` turns about y: laid along x.
    let along_x = glam::Mat4::from_rotation_z(-std::f32::consts::FRAC_PI_2);
    vec![("sleeve", crate::sdf::moved(&crate::sdf::lathe(&profile, 20, false), along_x))]
}

/// A dial's face about its centre: a disc in the plane x = 0, facing the driver (−x). The game's
/// shader draws its marks and its needle on it.
pub fn dial() -> Vec<Triangle> {
    let n = Vec3::NEG_X;
    let rim = |k: usize| {
        let a = std::f32::consts::TAU * k as f32 / 48.0;
        Corner { p: Vec3::new(0.0, a.cos() * DIAL_RADIUS, a.sin() * DIAL_RADIUS), n, open: 1.0 }
    };
    (0..48).map(|k| [Corner { p: Vec3::ZERO, n, open: 1.0 }, rim(k), rim(k + 1)]).collect()
}
