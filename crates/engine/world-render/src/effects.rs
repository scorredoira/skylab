//! What a wreck looks like besides its pieces: the flash and the fireball, the flames and the smoke,
//! the fragments and their trails, sparks, dust, the splash and the foam, the light of the fire and
//! the marks on the ground. The simulation (`sim_core::Wreck`) says what happened and how hard:
//! where it came apart, how much fuel went up at once and how much burns, where the fragments flew,
//! the scars and the splashes. This module says how that looks at a moment, as a pure function of
//! the wreck and the time: nothing is kept from one frame to the next, so a picture taken at any
//! moment is the same as one reached by playing up to it.
//!
//! Each puff, spark or drop follows a closed-form path (a linear drag towards the air's velocity, a
//! lift that dies away as hot gas cools, gravity for what is heavy) from where and when it was born.

use sim_core::{smoothstep, Scar, Wreck};
use glam::{DVec3, Vec3};

/// The kinds of particle, as `effects.wgsl` tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A ball of gas, ray marched: smoke, dust, mist, or burning gas that glows by its temperature.
    Puff = 0,
    /// Thrown water: a streak of drops along its velocity, lit.
    Spray = 1,
    /// A fragment of the wreck: a small dark sliver that tumbles.
    Shard = 2,
    /// A spark: a glowing streak along its velocity, added to the light.
    Spark = 3,
    /// The flash of the explosion, added to the light.
    Flash = 4,
}

/// One particle at the moment drawn, in world axes.
#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub at: DVec3,
    /// World axes, m/s: what streaks are drawn along.
    pub vel: DVec3,
    /// Radius, metres.
    pub size: f32,
    pub kind: Kind,
    /// Albedo (puffs, spray, shards) or light (sparks, flash), linear; `a` is the opacity: for a
    /// puff, its optical depth through the middle.
    pub color: [f32; 4],
    /// Temperature of burning gas, thousands of kelvin above the air (0: cold).
    pub temp: f32,
    /// A number 0..1 of its own (its noise, its spin).
    pub seed: f32,
    /// Seconds since it was born.
    pub age: f32,
    /// How much other smoke shades it from the sky and the sun, 0..1.
    pub shade: f32,
}

/// A light that lights the ground, the trees, the buildings, the wrecks and the smoke round it,
/// falling off with the square of the distance: a fire's, and the floodlights' and vehicles' beams.
#[derive(Clone, Copy, Debug)]
pub struct FireLight {
    pub at: DVec3,
    /// Its colour times its strength: the light at 1 m.
    pub color: Vec3,
    /// A beam it shines in, if not all round: the axis (render axes, unit), the angle off it where
    /// it is half as bright (radians), and the share of its middle it spills all round.
    pub beam: Option<(Vec3, f32, f32)>,
}

/// A patch of foam on the water where something went in.
#[derive(Clone, Copy, Debug)]
pub struct Foam {
    pub at: DVec3,
    pub radius: f64,
    /// 0..1.
    pub strength: f32,
}

/// A mark on the ground, and how hot it still glows (0..1).
#[derive(Clone, Copy, Debug)]
pub struct Mark {
    pub scar: Scar,
    pub glow: f32,
}

/// What the wrecks look like at one moment.
#[derive(Default)]
pub struct Effects {
    pub particles: Vec<Particle>,
    pub lights: Vec<FireLight>,
    pub marks: Vec<Mark>,
    pub foam: Vec<Foam>,
}

impl Effects {
    pub fn clear(&mut self) {
        self.particles.clear();
        self.lights.clear();
        self.marks.clear();
        self.foam.clear();
    }
}

/// A number 0..1 of its own for each particle and property.
pub fn hash(i: u32, k: u32) -> f32 {
    let mut h = i.wrapping_mul(0x9E37_79B1) ^ k.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_216.0
}

fn h(i: u32, k: u32) -> f64 {
    hash(i, k) as f64
}

/// A direction at random, uniform over the sphere.
fn direction(i: u32, k: u32) -> DVec3 {
    let z = h(i, k) * 2.0 - 1.0;
    let a = h(i, k + 1) * std::f64::consts::TAU;
    let r = (1.0 - z * z).sqrt();
    DVec3::new(r * a.cos(), r * a.sin(), z)
}

const UP: DVec3 = DVec3::new(0.0, 0.0, -1.0);
const GRAVITY: DVec3 = DVec3::new(0.0, 0.0, 9.81);

/// Where something is `t` seconds after it left `x0` at `v0`, when a drag of rate `k` (1/s) pulls
/// it towards the air's velocity `air` and a lift of `lift` (m/s², world axes) dies away at the
/// rate `cool` (1/s), as the buoyancy of hot gas does. `air` may include a steady fall or rise.
fn drift(x0: DVec3, v0: DVec3, air: DVec3, k: f64, lift: DVec3, cool: f64, t: f64) -> DVec3 {
    let ek = (-k * t).exp();
    let gone = (1.0 - ek) / k;
    let c = if (k - cool).abs() < 1e-3 { cool + 1e-3 } else { cool };
    let lifted = if c > 0.0 { lift / (k - c) * ((1.0 - (-c * t).exp()) / c - gone) } else { lift / k * (t - gone) };
    x0 + v0 * gone + lifted + air * (t - gone)
}

/// The velocity along the same path.
fn drift_velocity(v0: DVec3, air: DVec3, k: f64, lift: DVec3, cool: f64, t: f64) -> DVec3 {
    let ek = (-k * t).exp();
    let c = if (k - cool).abs() < 1e-3 { cool + 1e-3 } else { cool };
    v0 * ek + lift / (k - c) * ((-c * t).exp() - ek) + air * (1.0 - ek)
}

/// Smoke stays this long after the fire is out, seconds.
const SMOKE_AFTER: f64 = 90.0;
/// The plume of the fire: puffs, how long each is seen and how fast the column rises, m/s.
const PLUME_PUFFS: u32 = 90;
const PLUME_LIFE: f64 = 30.0;
const PLUME_RISE: f64 = 3.0;
/// Flames over the pool of fuel, and over each patch burning along the slide.
const FLAMES: u32 = 18;
const TRAIL_FLAMES: u32 = 5;
/// The trail of a burning fragment: a puff every this many seconds, each seen this long.
const TRAIL_EVERY: f64 = 0.025;
const TRAIL_LIFE: f64 = 3.0;
/// Fragments are drawn up to this far, metres.
const SHARD_RANGE: f64 = 2500.0;

/// Adds what `wreck` looks like at world time `time`, seen from `eye`, with the wind (world axes,
/// m/s) carrying the smoke. `ground(n, e)` is the height of the ground there. With `grid` (the
/// height above the fire's foot up to which the fire's simulation draws, `fire.rs`) the fireball,
/// the flames and the smoke up to there are the simulation's; the column above is still drawn here.
pub fn wreck(wreck: &Wreck, time: f64, wind: DVec3, eye: DVec3, grid: Option<f64>, ground: &dyn Fn(f64, f64) -> f64, out: &mut Effects) {
    let since = time - wreck.time;
    if since < 0.0 {
        return;
    }
    let seed = (wreck.time * 240.0) as u32 ^ (wreck.impact.x.to_bits() as u32).rotate_left(7);
    let ground_at_impact = ground(wreck.impact.x, wreck.impact.y);
    let in_water = wreck.splashes.first().is_some_and(|s| s.time == 0.0);
    let low = -wreck.impact.z - ground_at_impact < 8.0;
    let near = (wreck.impact - eye).length() < 20_000.0;

    if wreck.fireball > 0.0 && near {
        fireball(wreck, since, seed, wind, grid, out);
    }
    if wreck.severity > 12.0 && !in_water && near {
        sparks(wreck, since, seed, out);
        if low {
            dust(wreck, since, seed, ground_at_impact, wind, grid.is_some(), out);
        }
    }
    sliding(wreck, since, seed, ground, out);
    fragments(wreck, since, seed, wind, eye, out);
    fire(wreck, time, since, seed, wind, grid, out);
    for splash in &wreck.splashes {
        splashing(wreck, splash, since, seed, out);
    }
    for scar in &wreck.scars {
        if since < scar.made {
            continue;
        }
        // Scorched ground glows for a while where fuel burnt on it.
        let glow = if scar.burns_until > since { 1.0 } else { (1.0 - (since - scar.burns_until.max(scar.made)) / 40.0).max(0.0) as f32 * 0.5 };
        out.marks.push(Mark { scar: *scar, glow });
        // Fuel spilt along the slide burns where it lies.
        if scar.burns_until > since && scar.crater == 0.0 {
            let strength = (((scar.burns_until - since) / 6.0).min(1.0) * ((since - scar.made) / 0.5).min(1.0)) as f32;
            if grid.is_none() {
                flames(scar.pos, scar.radius * 0.5, TRAIL_FLAMES, time, seed ^ (scar.made * 97.0) as u32, strength * 0.7, wind, out);
            }
            out.lights.push(FireLight { at: scar.pos + UP * 1.5, color: Vec3::new(1.0, 0.42, 0.12) * 18.0 * strength, beam: None });
        }
    }
}

/// The flash and the fireball: the fuel thrown out as a mist, lit, going up at once. Its size
/// follows the mass of fuel (5.8 m across per kg^⅓, the fit for hydrocarbon fireballs); it rises
/// and rolls into a mushroom as the gas cools, turns from yellow-orange to black soot, and the soot
/// thins to grey and drifts with the wind.
fn fireball(wreck: &Wreck, since: f64, seed: u32, wind: DVec3, grid: Option<f64>, out: &mut Effects) {
    let fuel = wreck.fireball;
    let radius = 2.9 * fuel.cbrt();
    let p = (radius / 12.0).clamp(0.3, 2.0);
    let at = wreck.impact + UP * 1.0;
    let carry = DVec3::new(wreck.impact_vel.x, wreck.impact_vel.y, 0.0) * 0.25;
    // It burns for about 0.45 s per kg^⅓ (the same fit), then cools into soot.
    let burns = (0.45 * fuel.cbrt()).max(0.8);
    let cool = 0.9 / burns;
    // The flash: the first fraction of a second, as bright as the sun round it.
    if since < 0.35 {
        let k = since / 0.35;
        out.particles.push(Particle { at: at + UP * radius * 0.3, vel: DVec3::ZERO, size: (radius * (0.8 + 1.6 * k)) as f32, kind: Kind::Flash, color: [1.0, 0.75, 0.45, (1.0 - k) as f32 * 1.6], temp: 0.0, seed: 0.5, age: since as f32, shade: 0.0 });
    }
    // Its light on everything round it: the flash, then the burning gas as it rises and dies.
    let rise = radius * 0.6 + since * 6.0 * p;
    let glow = if since < 0.3 { 1.0 } else { (-(since - 0.3) * cool).exp() };
    if glow > 0.01 {
        out.lights.push(FireLight { at: at + UP * rise, color: Vec3::new(1.0, 0.5, 0.18) * (250.0 * fuel.sqrt() * glow) as f32, beam: None });
    }
    if let Some(top) = grid {
        cap(wreck, since, seed, wind, radius.min(17.0), top, out);
        return;
    }
    let count = (40.0 + 3.0 * fuel).min(130.0) as u32;
    for i in 0..count {
        let s = seed.wrapping_add(i * 7919);
        let born = h(s, 1) * 0.12;
        let age = since - born;
        let life = 5.0 + 4.0 * h(s, 2);
        if !(0.0..life * 2.2).contains(&age) {
            continue;
        }
        let d = direction(s, 3);
        let f = h(s, 5).cbrt();
        let x0 = at + d * f * 3.2 * p;
        let v0 = (d * f * 27.0 * p + UP * (4.0 + 5.0 * h(s, 6)) * p) + carry;
        let temp0 = 1.9 + 0.4 * h(s, 7);
        let drag = 1.9 + 0.5 * h(s, 8);
        let buoy = 7.0 + 3.0 * h(s, 9);
        // Hot gas rises while it is hot; what is left is light smoke that drifts with the wind.
        let pos = drift(x0, v0, wind + UP * buoy * 0.15 / drag, drag, UP * buoy * temp0, cool, age);
        let vel = drift_velocity(v0, wind + UP * buoy * 0.15 / drag, drag, UP * buoy * temp0, cool, age);
        let temp = (temp0 * (-cool * age).exp()) as f32;
        let k = (age / (life * 2.2)) as f32;
        let size0 = (2.8 + 1.4 * h(s, 10)) * p;
        let size1 = (10.0 + 4.5 * h(s, 11)) * p;
        let grow = 1.0 - (1.0 - (age / life).min(1.0)).powi(2);
        let size = size0 + (size1 * (1.0 + 0.6 * (age / life - 1.0).max(0.0)) - size0) * grow;
        let fade = smoothstep(0.0, 0.03, k as f64) as f32 * (1.0 - smoothstep(0.5, 1.0, k as f64) as f32);
        // Fresh soot is nearly black; as it thins and cools it greys.
        let grey = 0.17 + 0.25 * smoothstep(0.15, 0.8, k as f64) as f32;
        out.particles.push(Particle {
            at: pos,
            vel,
            size: size as f32,
            kind: Kind::Puff,
            color: [grey, grey * 0.97, grey * 0.94, (0.8 + 0.15 * hash(s, 12)) * 3.2 * fade],
            temp,
            seed: hash(s, 13),
            age: age as f32,
            shade: 0.35 * (1.0 - f) as f32,
        });
    }
}

/// How long the smoke of a fireball is seen above the fire's simulation, s.
const CAP_LIFE: f64 = 75.0;

/// The middle of a fireball's smoke `since` seconds after the crash, metres above where it burst:
/// what the fire's simulation gives for a fireball of an aircraft's fuel (`fire::tests`), so that
/// the smoke goes on above the grid where the grid leaves it.
pub fn cap_height(since: f64) -> f64 {
    5.0 + 10.0 * (1.0 - (-since / 2.2).exp())
}

/// The smoke of the fireball above the fire's simulation: once the cloud has risen through the
/// grid's top (`top` metres above the fire's foot) it goes on here as balls of smoke about its
/// middle, wider and greyer with the years, drifting with the wind.
fn cap(wreck: &Wreck, since: f64, seed: u32, wind: DVec3, radius: f64, top: f64, out: &mut Effects) {
    if since > CAP_LIFE {
        return;
    }
    let foot = wreck.fire.map_or(wreck.impact, |_| wreck.main().pos + UP * 0.4);
    let middle = wreck.impact + UP * cap_height(since) + wind * since * 0.9;
    let k = since / CAP_LIFE;
    let spread = radius * (0.75 + 0.02 * since);
    for i in 0..26u32 {
        let s = seed.wrapping_add(i * 4201 + 61);
        let d = direction(s, 1);
        let at = middle + DVec3::new(d.x, d.y, d.z * 0.7) * spread * h(s, 3).cbrt();
        let shown = smoothstep(top - 10.0, top + 5.0, -(at.z - foot.z));
        if shown <= 0.0 {
            continue;
        }
        let fade = (shown * (1.0 - smoothstep(0.45, 1.0, k))) as f32;
        let grey = 0.06 + 0.2 * smoothstep(0.1, 0.9, k) as f32;
        out.particles.push(Particle {
            at,
            vel: DVec3::ZERO,
            size: (radius * (0.5 + 0.012 * since) * (0.8 + 0.4 * h(s, 4))) as f32,
            kind: Kind::Puff,
            color: [grey, grey * 0.98, grey * 0.96, 2.4 * fade],
            temp: 0.0,
            seed: hash(s, 5),
            age: since as f32,
            shade: 0.4 * (1.0 - h(s, 3)) as f32,
        });
    }
}

/// Sparks thrown from the impact: hot metal and grit, streaks that fall and cool.
fn sparks(wreck: &Wreck, since: f64, seed: u32, out: &mut Effects) {
    if since > 2.5 {
        return;
    }
    let count = (70.0 * smoothstep(12.0, 40.0, wreck.severity)) as u32;
    let speed = (wreck.severity / 40.0).sqrt();
    for i in 0..count {
        let s = seed.wrapping_add(i * 104_729 + 3);
        let born = h(s, 1) * 0.15;
        let age = since - born;
        let life = 0.6 + 1.4 * h(s, 2);
        if !(0.0..life).contains(&age) {
            continue;
        }
        let d = (direction(s, 3) + UP * 0.8).normalize();
        let v0 = d * (15.0 + 45.0 * h(s, 5)) * speed + wreck.impact_vel * 0.2;
        let tau = 1.0 / (0.3 + 0.6 * h(s, 6));
        let pos = drift(wreck.impact, v0, GRAVITY * tau, 1.0 / tau, DVec3::ZERO, 0.0, age);
        let vel = drift_velocity(v0, GRAVITY * tau, 1.0 / tau, DVec3::ZERO, 0.0, age);
        let heat = (1.0 - age / life) as f32;
        out.particles.push(Particle { at: pos, vel, size: 0.05 + 0.05 * hash(s, 7), kind: Kind::Spark, color: [1.0, 0.6, 0.25, heat * (0.5 + 0.5 * hash(s, 8))], temp: heat, seed: hash(s, 9), age: age as f32, shade: 0.0 });
    }
}

/// Earth thrown up where it went in: a skirt of dust blown out along the ground and a cloud over
/// the hole.
fn dust(wreck: &Wreck, since: f64, seed: u32, ground: f64, wind: DVec3, simulated: bool, out: &mut Effects) {
    if since > 14.0 || simulated {
        return;
    }
    let p = (wreck.severity / 40.0).clamp(0.4, 1.6);
    let at = DVec3::new(wreck.impact.x, wreck.impact.y, -ground);
    for i in 0..22u32 {
        let s = seed.wrapping_add(i * 31 + 900);
        let skirt = i < 16;
        let a = (i as f64 + h(s, 1) * 0.6) * std::f64::consts::TAU / 16.0;
        let out_dir = DVec3::new(a.cos(), a.sin(), 0.0);
        let v0 = if skirt { out_dir * (12.0 + 12.0 * h(s, 2)) * p + UP * (1.0 + 2.0 * h(s, 3)) } else { direction(s, 4) * 4.0 * p + UP * (6.0 + 8.0 * h(s, 5)) * p };
        let age = since - h(s, 6) * 0.08;
        let life = 9.0 + 4.0 * h(s, 7);
        if !(0.0..life).contains(&age) {
            continue;
        }
        let pos = drift(at + UP * 1.0, v0, wind * 0.8 + UP * 0.25, 1.3, DVec3::ZERO, 0.0, age);
        let k = age / life;
        let size = (1.5 + 6.5 * (1.0 - (-age / 2.5).exp())) * p * (0.8 + 0.4 * h(s, 8));
        let fade = (smoothstep(0.0, 0.05, k) * (1.0 - smoothstep(0.35, 1.0, k))) as f32;
        out.particles.push(Particle {
            at: pos,
            vel: DVec3::ZERO,
            size: size as f32,
            kind: Kind::Puff,
            color: [0.46, 0.39, 0.31, 1.6 * fade],
            temp: 0.0,
            seed: hash(s, 9),
            age: age as f32,
            shade: 0.1,
        });
    }
}

/// Sparks where a piece slides fast on the ground.
fn sliding(wreck: &Wreck, since: f64, seed: u32, ground: &dyn Fn(f64, f64) -> f64, out: &mut Effects) {
    for (j, piece) in wreck.pieces.iter().enumerate() {
        let speed = piece.vel.length();
        if piece.at_rest || piece.sunk || speed < 6.0 || since > 30.0 {
            continue;
        }
        let g = ground(piece.pos.x, piece.pos.y);
        if piece.pos.z < -g - 1.5 {
            continue;
        }
        let contact = DVec3::new(piece.pos.x, piece.pos.y, -g - 0.1);
        for i in 0..14u32 {
            let s = seed.wrapping_add(j as u32 * 7777 + i * 13 + 5);
            let age = ((since * 3.0 + i as f64 / 14.0 + h(s, 1) * 0.1).fract()) * 0.45;
            let v0 = piece.vel * (0.5 + 0.3 * h(s, 2)) + (direction(s, 3) * 0.6 + UP) * (3.0 + 6.0 * h(s, 4));
            let from = contact - piece.vel * age;
            let pos = drift(from, v0, GRAVITY * 1.5, 1.0 / 1.5, DVec3::ZERO, 0.0, age);
            let heat = (1.0 - age / 0.45) as f32;
            out.particles.push(Particle { at: pos, vel: v0 + GRAVITY * age, size: 0.04, kind: Kind::Spark, color: [1.0, 0.65, 0.3, heat * ((speed - 6.0) / 10.0).min(1.0) as f32], temp: heat, seed: hash(s, 5), age: age as f32, shade: 0.0 });
        }
    }
}

/// The fragments: each a tumbling sliver where it flies or lies, and those set alight by the
/// fireball burning as they fly, trailing smoke.
fn fragments(wreck: &Wreck, since: f64, seed: u32, wind: DVec3, eye: DVec3, out: &mut Effects) {
    for (j, f) in wreck.fragments.iter().enumerate() {
        let at = f.at(since);
        if (at - eye).length() > SHARD_RANGE || (f.water && f.landed.is_some_and(|l| since > l + 1.0)) {
            continue;
        }
        let s = seed.wrapping_add(j as u32 * 9973 + 11);
        out.particles.push(Particle { at, vel: f.velocity(since), size: (f.size * 0.5) as f32, kind: Kind::Shard, color: [0.09, 0.085, 0.08, 1.0], temp: 0.0, seed: hash(s, 1), age: since as f32, shade: 0.0 });
        if f.burns <= 0.0 {
            continue;
        }
        let end = since.min(f.burns).min(f.landed.unwrap_or(f64::MAX));
        // Its flame, while it burns in the air.
        if since < f.burns && f.landed.is_none_or(|l| since < l) {
            out.particles.push(Particle { at, vel: DVec3::ZERO, size: (0.35 + f.size) as f32, kind: Kind::Puff, color: [0.1, 0.09, 0.08, 1.6], temp: 1.8, seed: hash(s, 2), age: since as f32, shade: 0.0 });
        }
        // Its trail: a puff left every `TRAIL_EVERY` seconds, each spreading, rising a little and drifting.
        let first = ((since - TRAIL_LIFE).max(0.0) / TRAIL_EVERY).ceil() as i64;
        let last = (end / TRAIL_EVERY).floor() as i64;
        for n in first..=last {
            let born = n as f64 * TRAIL_EVERY;
            let age = since - born;
            if age < 0.0 {
                continue;
            }
            let from = f.at(born);
            let v0 = f.velocity(born) * 0.15;
            let pos = drift(from, v0, wind + UP * 0.6, 1.2, DVec3::ZERO, 0.0, age);
            let k = age / TRAIL_LIFE;
            let ns = s.wrapping_add(n as u32 * 17);
            let thick = (1.0 - k).powf(1.5) * (f.size / 0.3).clamp(0.6, 1.4);
            out.particles.push(Particle {
                at: pos,
                vel: DVec3::ZERO,
                // Each puff overlaps the next: the trail is one ribbon, not a string of beads.
                size: ((0.2 + 0.5 * age.sqrt()) * (0.8 + 0.4 * h(ns, 1))).max(f.velocity(born).length() * TRAIL_EVERY * 1.1) as f32,
                kind: Kind::Puff,
                color: [0.07, 0.068, 0.065, (1.4 * thick) as f32],
                temp: (1.2 * (-age * 7.0).exp()) as f32,
                seed: hash(ns, 2),
                age: age as f32,
                shade: 0.0,
            });
        }
    }
}

/// Flames over a patch of burning fuel at `base`, `spread` metres round: tongues that rise and shrink, flickering.
#[allow(clippy::too_many_arguments)]
fn flames(base: DVec3, spread: f64, count: u32, time: f64, seed: u32, strength: f32, wind: DVec3, out: &mut Effects) {
    if strength <= 0.0 {
        return;
    }
    for i in 0..count {
        let s = seed.wrapping_add(i * 389 + 77);
        let rate = 1.1 + 0.6 * h(s, 1);
        let phase = (time * rate + h(s, 2)).fract();
        let (a, b) = (h(s, 3) - 0.5, h(s, 4) - 0.5);
        let height = (2.0 + 2.5 * h(s, 5)) * strength as f64 * (spread / 2.0).clamp(0.6, 1.6);
        let at = base + DVec3::new(a, b, 0.0) * spread * 2.0 + UP * (0.3 + phase * height) + wind * phase * 0.4;
        let heat = (1.0 - phase) as f32;
        out.particles.push(Particle {
            at,
            vel: DVec3::ZERO,
            size: (0.7 + 0.9 * heat) * (0.6 + 0.4 * strength) * (spread / 2.0).clamp(0.7, 1.5) as f32,
            kind: Kind::Puff,
            color: [0.2, 0.19, 0.18, 1.4 * heat.sqrt() * strength],
            temp: (1.2 + 0.8 * heat) * strength.sqrt(),
            seed: hash(s, 6),
            age: (time * rate) as f32,
            shade: 0.0,
        });
    }
}

/// The pool of fuel burning by the main piece: flames, their light, and the plume of black smoke
/// that leans with the wind and spreads as it rises; grey and thinning once the fire is out.
fn fire(wreck: &Wreck, time: f64, since: f64, seed: u32, wind: DVec3, grid: Option<f64>, out: &mut Effects) {
    let Some(fire) = wreck.fire else { return };
    let main = wreck.main();
    let base = main.pos + UP * 0.4;
    let burning = match fire.out {
        None => ((since / 2.0).min(1.0) * (0.35 + 0.65 * (fire.fuel / 20.0).min(1.0))) as f32,
        Some(_) => 0.0,
    };
    let smoking = match fire.out {
        None => 1.0,
        Some(out) => (1.0 - (time - out) / SMOKE_AFTER).max(0.0),
    } as f32;
    if burning > 0.0 {
        let flicker = 0.8 + 0.2 * ((time * 11.0).sin() * (time * 17.3).sin()) as f32;
        out.lights.push(FireLight { at: base + UP * 2.0, color: Vec3::new(1.0, 0.45, 0.14) * 40.0 * burning * flicker, beam: None });
        if grid.is_none() {
            flames(base, 2.2, FLAMES, time, seed, burning, wind, out);
        }
    }
    if smoking <= 0.0 {
        return;
    }
    // Where the fire's simulation stops (its top), the column here takes over.
    let above = grid.unwrap_or(f64::NEG_INFINITY);
    for i in 0..PLUME_PUFFS {
        let s = seed.wrapping_add(i * 6151 + 21);
        let age = (since - i as f64 * PLUME_LIFE / PLUME_PUFFS as f64 - h(s, 1) * 0.2).rem_euclid(PLUME_LIFE);
        let born = since - age;
        if born < 0.3 || fire.out.is_some_and(|out| born > out - wreck.time) {
            continue;
        }
        let v0 = UP * 6.0 + direction(s, 2) * 1.5;
        let pos = drift(base + DVec3::new(h(s, 4) - 0.5, h(s, 5) - 0.5, 0.0) * 3.0, v0, wind * 0.95 + UP * PLUME_RISE, 0.5, UP * 4.0, 0.4, age);
        let height = -(pos.z - base.z);
        let k = age / PLUME_LIFE;
        // Seen only above the fire's simulation, coming in over 15 m.
        let shown = if grid.is_some() { smoothstep(above - 10.0, above + 5.0, height) } else { 1.0 };
        if shown <= 0.0 {
            continue;
        }
        let fade = (smoothstep(0.0, 0.04, k) * (1.0 - smoothstep(0.45, 1.0, k)) * shown) as f32;
        let strength = smoking * (0.4 + 0.6 * burning.max(0.25));
        let grey = 0.05 + 0.12 * smoothstep(0.0, 0.8, k) as f32 + 0.2 * (1.0 - smoking);
        out.particles.push(Particle {
            at: pos,
            vel: DVec3::ZERO,
            size: (2.0 + 1.4 * age * (0.85 + 0.3 * h(s, 6))) as f32,
            kind: Kind::Puff,
            color: [grey, grey * 0.98, grey * 0.96, 3.2 * fade * strength],
            temp: (0.9 * (-age * 2.5).exp()) as f32 * burning,
            seed: hash(s, 7),
            age: age as f32,
            shade: (0.75 - 0.35 * k) as f32,
        });
    }
}

/// Water thrown up where something went in: a sheet of drops all round, more of it ahead the flatter
/// it went in, a column straight up the steeper it went in, a mist that hangs and drifts, and the
/// foam it leaves.
fn splashing(wreck: &Wreck, splash: &sim_core::Splash, since: f64, seed: u32, out: &mut Effects) {
    let age = since - splash.time;
    if age < 0.0 {
        return;
    }
    let speed = splash.vel.length();
    let energy = 0.5 * splash.mass * speed * speed;
    // 1 for an aircraft going in at 40 m/s.
    let p = (energy / 8.0e5).cbrt().clamp(0.1, 2.0);
    let flat = DVec3::new(splash.vel.x, splash.vel.y, 0.0);
    let steep = (splash.vel.z / speed.max(1e-3)).clamp(0.0, 1.0);
    let ahead = flat.normalize_or_zero();
    // The foam spreads quickly where it went in, then slowly as it drifts apart and fades.
    out.foam.push(Foam { at: splash.pos + ahead * 4.0 * p, radius: (4.0 + 14.0 * p) * (1.0 - (-age / 4.0).exp()) + age * 0.08, strength: ((1.0 - age / 150.0).max(0.0) * p.min(1.0).sqrt()) as f32 });
    if age > 9.0 {
        return;
    }
    let s0 = seed ^ (splash.time * 1000.0) as u32 ^ (splash.mass as u32);
    let drops = (40.0 + 900.0 * p.min(1.2)) as u32;
    for i in 0..drops {
        let s = s0.wrapping_add(i * 211 + 1);
        let column = h(s, 1) < 0.25 * steep;
        let a = h(s, 2) * std::f64::consts::TAU;
        let around = DVec3::new(a.cos(), a.sin(), 0.0);
        let v0 = if column {
            UP * (18.0 + 22.0 * h(s, 3)) * p.sqrt() + around * 2.0
        } else {
            // A crown thrown out and up, most of it ahead, and ahead the more the flatter it went in.
            let lean = (around + ahead * (1.0 - steep) * 1.2).normalize_or_zero();
            lean * (6.0 + 14.0 * h(s, 4)) * p.sqrt() + UP * (8.0 + 16.0 * h(s, 5)) * p.sqrt() + flat * 0.25 * h(s, 6)
        };
        let born = h(s, 7) * 0.15;
        let t = age - born;
        if t < 0.0 {
            continue;
        }
        let tau = 2.5;
        let pos = drift(splash.pos + around * h(s, 8) * 2.0 * p, v0, GRAVITY * tau, 1.0 / tau, DVec3::ZERO, 0.0, t);
        if pos.z > splash.pos.z + 0.2 {
            continue;
        }
        let vel = drift_velocity(v0, GRAVITY * tau, 1.0 / tau, DVec3::ZERO, 0.0, t);
        out.particles.push(Particle { at: pos, vel, size: ((0.35 + 0.8 * h(s, 9)) * p.sqrt()) as f32, kind: Kind::Spray, color: [0.85, 0.9, 0.93, 0.9], temp: 0.0, seed: hash(s, 10), age: t as f32, shade: 0.0 });
    }
    // The mist: spray hanging in puffs about the same size whatever went in, more of them for more.
    let mists = (6.0 + 40.0 * p.min(1.2)) as u32;
    for i in 0..mists {
        let s = s0.wrapping_add(i * 977 + 5);
        let t = age - 0.1 - h(s, 1) * 0.4;
        let life = 6.0 + 4.0 * h(s, 2);
        if !(0.0..life).contains(&t) {
            continue;
        }
        let a = h(s, 3) * std::f64::consts::TAU;
        let around = DVec3::new(a.cos(), a.sin(), 0.0);
        let v0 = around * (4.0 + 12.0 * h(s, 4)) * p.sqrt() + UP * (3.0 + 14.0 * h(s, 5)) * p.sqrt() + ahead * 4.0 * (1.0 - steep);
        let pos = drift(splash.pos + UP * 0.5, v0, UP * 0.2, 0.9, DVec3::ZERO, 0.0, t);
        let k = t / life;
        let fade = (smoothstep(0.0, 0.08, k) * (1.0 - smoothstep(0.3, 1.0, k))) as f32;
        out.particles.push(Particle {
            at: pos,
            vel: DVec3::ZERO,
            size: ((2.5 + 9.0 * (1.0 - (-t / 2.5).exp())) * (0.7 + 0.6 * p.min(1.0))) as f32,
            kind: Kind::Puff,
            color: [0.9, 0.92, 0.95, 0.9 * fade],
            temp: 0.0,
            seed: hash(s, 6),
            age: t as f32,
            shade: 0.0,
        });
    }
    let _ = wreck;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{AircraftType, PieceKind};

    fn crash(fuel: f64, speed: f64) -> Wreck {
        let mut wreck = Wreck::new(AircraftType::c172().pieces, &PieceKind::ALL, 1000.0, DVec3::new(0.0, 0.0, -1.0), sim_core::quaternion(0.0, -0.5, 0.0), DVec3::new(speed, 0.0, speed * 0.5), DVec3::ZERO, 0.5, 7, 100.0);
        wreck.spill(fuel, false);
        let mut t = 100.0;
        while t < 103.0 {
            wreck.step(1.0 / 240.0, t, |_, _| (0.0, false));
            t += 1.0 / 240.0;
        }
        wreck
    }

    fn effects(wreck: &Wreck, time: f64, grid: Option<f64>) -> Effects {
        let mut out = Effects::default();
        super::wreck(wreck, time, DVec3::new(2.0, 1.0, 0.0), DVec3::new(-200.0, 0.0, -20.0), grid, &|_, _| 0.0, &mut out);
        out
    }

    #[test]
    fn a_hard_crash_flashes_burns_and_smokes_and_a_cold_wreck_does_not() {
        let wreck = crash(100.0, 40.0);
        assert!(wreck.fireball > 0.0 && wreck.fire.is_some());
        let early = effects(&wreck, 100.1, None);
        let count = |e: &Effects, k: Kind| e.particles.iter().filter(|p| p.kind == k).count();
        assert_eq!(count(&early, Kind::Flash), 1);
        assert!(count(&early, Kind::Puff) > 40 && count(&early, Kind::Spark) > 20 && count(&early, Kind::Shard) > 10);
        assert!(early.particles.iter().any(|p| p.temp > 1.0));
        assert!(!early.lights.is_empty() && !early.marks.is_empty());
        // A minute on: flames, a plume, the fragments lying round it.
        let later = effects(&wreck, 160.0, None);
        assert_eq!(count(&later, Kind::Flash), 0);
        assert!(count(&later, Kind::Puff) > 60 && count(&later, Kind::Shard) > 10);
        // With the fire's simulation, the fireball and the flames are its own, and the plume starts above it.
        let (simulated, alone) = (effects(&wreck, 100.5, Some(40.0)), effects(&wreck, 100.5, None));
        assert!(count(&simulated, Kind::Puff) + 60 < count(&alone, Kind::Puff));
        // Long after the fire is out, what is left is the fragments and the marks.
        let mut cold = wreck.clone();
        cold.fire = Some(sim_core::Fire { fuel: 0.0, started: 100.0, out: Some(300.0) });
        let gone = effects(&cold, 300.0 + SMOKE_AFTER + 60.0, None);
        assert_eq!(count(&gone, Kind::Puff), 0);
        assert!(gone.lights.is_empty());
    }

    #[test]
    fn a_soft_crash_does_not_explode() {
        let wreck = crash(100.0, 14.0);
        assert!(wreck.fireball == 0.0 && wreck.fire.is_none());
        let e = effects(&wreck, 100.2, None);
        assert!(e.particles.iter().all(|p| p.kind != Kind::Flash && p.temp == 0.0));
        assert!(e.lights.is_empty());
    }

    #[test]
    fn the_same_moment_looks_the_same() {
        let wreck = crash(100.0, 40.0);
        let (a, b) = (effects(&wreck, 101.3, None), effects(&wreck, 101.3, None));
        assert_eq!(a.particles.len(), b.particles.len());
        assert!(a.particles.iter().zip(&b.particles).all(|(p, q)| p.at == q.at && p.size == q.size));
    }

    #[test]
    fn drift_follows_its_equation() {
        // Integrate v' = k (air - v) + lift e^(-c t) by small steps and compare.
        let (x0, v0, air, k, lift, c): (DVec3, DVec3, DVec3, f64, DVec3, f64) = (DVec3::ZERO, DVec3::new(10.0, 0.0, -5.0), DVec3::new(2.0, 0.0, -3.0), 1.7, DVec3::new(0.0, 0.0, -8.0), 1.25);
        let (mut x, mut v, dt) = (x0, v0, 1e-4_f64);
        let mut t = 0.0_f64;
        while t < 3.0 {
            v += (k * (air - v) + lift * (-c * t).exp()) * dt;
            x += v * dt;
            t += dt;
        }
        assert!((drift(x0, v0, air, k, lift, c, t) - x).length() < 0.01, "{:?} {:?}", drift(x0, v0, air, k, lift, c, t), x);
        assert!((drift_velocity(v0, air, k, lift, c, t) - v).length() < 0.01);
    }
}

