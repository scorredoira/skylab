//! A wheel and its brake, about the hub: x forward, y out along the axle, z up. The tyre is a
//! turned profile (bead, bulging sidewall, round shoulder, a tread with four grooves); the rim a
//! field (barrel, lips, five pairs of spokes dished in to the hub, the hub and its cap); the brake
//! a ventilated disc with its bell and a caliper gripping it.

use std::f32::consts::{PI, TAU};

use glam::{Vec2, Vec3};

use crate::body::TYRE_RADIUS;
use crate::sdf::{lathe, round_box, round_box2, smin, split, sq, surface, Corner, Triangle};

/// The rim's radius at the bead (19 inches), and how far in from the tyre's side its outer lip is.
const RIM: f32 = 0.2445;
const LIP_IN: f32 = 0.012;

/// The tyre's profile from the inner bead over the tread to the outer bead: (y, radius).
fn tyre_profile(width: f32) -> Vec<(f32, f32)> {
    let half = width / 2.0;
    // The outer half, from the bead up the sidewall and round the shoulder to the tread's edge.
    let mut outer: Vec<(f32, f32)> = vec![(half - 0.022, RIM - 0.006), (half - 0.014, RIM)];
    let top = TYRE_RADIUS - 0.022;
    for i in 1..=8 {
        let s = i as f32 / 8.0;
        outer.push((half - 0.014 + 0.014 * (PI * s).sin().powf(0.7), RIM + (top - RIM) * s));
    }
    for i in 1..=6 {
        let a = PI / 2.0 * i as f32 / 6.0;
        outer.push((half - 0.036 + 0.022 * a.cos(), top + 0.022 * a.sin()));
    }
    // The tread from its edge to the middle, with two grooves on the way.
    let mut tread: Vec<(f32, f32)> = Vec::new();
    for g in [half * 0.56, 0.036] {
        tread.extend([(g + 0.0055, TYRE_RADIUS), (g + 0.0045, TYRE_RADIUS - 0.007), (g - 0.0045, TYRE_RADIUS - 0.007), (g - 0.0055, TYRE_RADIUS)]);
    }
    outer.extend(tread);
    // The inner half is the outer one mirrored: from the inner bead to the middle, then back down.
    let mut profile: Vec<(f32, f32)> = outer.iter().map(|&(y, r)| (-y, r)).collect();
    profile.extend(outer.iter().rev());
    profile
}

/// How far out (y) the spokes' face is at a radius: level with the lip at the rim, dished in to the hub.
fn face(width: f32, r: f32) -> f32 {
    width / 2.0 - LIP_IN - 0.010 - 0.052 * sq(1.0 - (r / 0.235).min(1.0))
}

fn rim(width: f32, p: Vec3) -> f32 {
    let half = width / 2.0;
    let r = p.x.hypot(p.z);
    let lip = half - LIP_IN;
    // The barrel between the lips, and the two lips.
    let barrel = ((r - 0.228).abs() - 0.006).max(p.y.abs() - lip);
    let outer_lip = (r - (RIM - 0.004)).hypot(p.y - lip) - 0.011;
    let inner_lip = (r - (RIM - 0.004)).hypot(p.y + lip) - 0.009;
    let mut d = smin(smin(barrel, outer_lip, 0.008), inner_lip, 0.008);
    // Five pairs of spokes, each pair spreading from the hub to the rim.
    let sector = TAU / 5.0;
    let a = (p.z.atan2(p.x) + sector / 2.0).rem_euclid(sector) - sector / 2.0;
    let off = (a.abs() - (0.10 + 0.24 * (r / 0.235).min(1.0))) * r;
    let spoke = round_box2(Vec2::new(off, p.y - (face(width, r) - 0.015)), Vec2::new(0.0115 + 0.004 * (1.0 - r / 0.235).max(0.0), 0.017), 0.007).max(r - 0.232).max(0.045 - r);
    d = smin(d, spoke, 0.014);
    // The hub, its cap, and five nuts round it.
    let hub_face = face(width, 0.0);
    let hub = (r - 0.068).max((p.y - (hub_face - 0.017)).abs() - 0.022);
    d = smin(d, hub, 0.016);
    let cap = (r - 0.027).max((p.y - hub_face - 0.004).abs() - 0.006);
    d = smin(d, cap, 0.004);
    let nut_angle = (p.z.atan2(p.x) + sector).rem_euclid(sector) - sector / 2.0;
    let nut = Vec2::new(r * nut_angle.cos() - 0.048, r * nut_angle.sin()).length().max((p.y - hub_face - 0.002).abs() - 0.008) - 0.008;
    d.min(nut)
}

/// A wheel of this tyre width: its rubber and its alloy.
pub fn wheel(width: f32) -> Vec<(&'static str, Vec<Triangle>)> {
    let tyre = lathe(&tyre_profile(width), 96, false);
    let field = |p: Vec3| rim(width, p);
    // Deep between the spokes the barrel is in shade.
    let open = |p: Vec3, _: Vec3| {
        let r = p.x.hypot(p.z);
        let depth = (face(width, r) - p.y).max(0.0);
        (1.0 - depth / 0.16).clamp(0.25, 1.0) * if r < 0.2 { 1.0 } else { 0.9 }
    };
    let alloy = surface(&field, Vec3::new(-0.26, -width / 2.0 - 0.01, -0.26), Vec3::new(0.26, width / 2.0 + 0.01, 0.26), 0.0075, &open);
    // The spokes apart from what looks the same turning (hub, barrel, lips), and a disc that stands
    // for them when they turn too fast to be seen one by one: the game draws one or the other.
    let (spokes, rim) = split(&alloy, &|c: &Corner| {
        let r = c.p.x.hypot(c.p.z);
        (0.072 - r).max(r - 0.214)
    });
    let dish: Vec<(f32, f32)> = (0..=10).map(|i| 0.072 + (0.214 - 0.072) * i as f32 / 10.0).map(|r| (face(width, r) - 0.006, r)).collect();
    let blur = lathe(&dish, 48, true);
    vec![("rubber", tyre), ("rim", rim), ("spokes", spokes), ("blur", blur)]
}

/// The brake behind a wheel of this tyre width: the disc and its bell, and the caliper, which
/// sits behind the axle (`caliper_angle` radians from forward, up positive).
pub fn brake(width: f32, disc_radius: f32, caliper_angle: f32) -> Vec<(&'static str, Vec<Triangle>)> {
    let y = face(width, 0.0) - 0.075;
    let disc = lathe(&[(y - 0.016, 0.10), (y - 0.016, disc_radius), (y + 0.016, disc_radius), (y + 0.016, 0.10)], 64, false);
    let bell = lathe(&[(y + 0.016, 0.10), (y + 0.045, 0.085), (y + 0.045, 0.0)], 40, false);
    let field = |p: Vec3| {
        let r = p.x.hypot(p.z);
        let mut a = p.z.atan2(p.x) - caliper_angle;
        a = (a + PI).rem_euclid(TAU) - PI;
        let mid = disc_radius - 0.042;
        let block = round_box(Vec3::new(r - mid, a * mid, p.y - y), Vec3::new(0.062, 0.095, 0.058), 0.016);
        // The slot the disc runs through.
        block.max(-((p.y - y).abs() - 0.019).max(r - disc_radius - 0.004))
    };
    let open = |_: Vec3, _: Vec3| 0.8;
    let caliper = surface(&field, Vec3::new(-0.25, y - 0.08, -0.25), Vec3::new(0.25, y + 0.08, 0.25), 0.006, &open);
    vec![("disc", disc), ("hub", bell), ("caliper", caliper)]
}
