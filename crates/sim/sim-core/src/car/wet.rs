//! Wet roads: how much grip a tyre keeps on a surface under so much water, and how much water
//! there is where on a circuit (`RoadWater`): a film that rain lays and tyres and time dry, and
//! puddles in the asphalt's hollows.
//!
//! **Grip under water** (`wet_grip`), for a road tyre with its tread, from the friction
//! coefficients of the Bosch Automotive Handbook (the table "Coefficients of friction μHF for
//! tyres in various conditions of wear, on various road conditions and at various speeds"; also
//! in K. Reif (ed.), *Brakes, Brake Control and Driver Assistance Systems*, Springer 2014, "Tires"):
//! a new tyre at 50 km/h has 0.85 dry, 0.65 wet (0.2 mm of water), 0.55 in heavy rain (1 mm) and
//! 0.5 in puddles (2 mm); at 90 km/h 0.8, 0.6, 0.3 and 0.05. Shares of the dry friction:
//!
//! - at low speed the water only wets the contact (a thin film the tread's grooves take away, the
//!   rest squeezed out of the patch: "viscous" aquaplaning): 0.76 of dry at 0.2 mm, 0.65 at 1 mm,
//!   0.59 at 2 mm (`LOW_SPEED`). It is the same for any depth up to some 50 km/h (the water's
//!   depth matters only at speed, as SWOV's measurements of tyres on wet roads found: R-76-17);
//! - faster, a wedge of water lifts the front of the patch off the road, more the deeper the water
//!   and the faster the tyre (dynamic aquaplaning): what is left is `1 / (1 + (v / v_c)^n)`, with
//!   `v_c` about 96 km/h in 1 mm of water (Horne and Dreher's speed of full aquaplaning for a car
//!   tyre at 2.4 bar is 9 √p knots, p in psi: some 98 km/h; NASA TN D-2056, 1963) and lower in
//!   deeper water, `n` making it come on sharply. Fitted to the table's 90 km/h row: 0.375 of dry
//!   at 1 mm, 0.06 at 2 mm, while the thin film of 0.2 mm keeps 0.75 at 90 and 0.73 at 130.
//!
//! On paint water lies on a smoother surface: road markings must keep a skid resistance of 45
//! (British pendulum, wet; EN 1436, class S1) where wet asphalt gives 55 to 70, about 0.7 of it;
//! the kerbs are painted all over, the lines along the asphalt's edges and the start's line in
//! part (`Patch::paint`). Wet grass keeps about 0.55 of its grip (Wong, *Theory of Ground
//! Vehicles*, table 1.3, against the dry), earth turns to mud (0.6), gravel hardly changes.
//!
//! The curve of force against slip changes too: the cornering and slip stiffnesses stay (they are
//! the tread's, not the road's), so with less friction the peak comes at a smaller slip, and past
//! it the force falls away faster (a peakier curve: the Magic Formula's curvature lowered by
//! `WET_CURVATURE`).
//!
//! **Where the water is** (`RoadWater`): rain of level 0 (dry) to 1 (a downpour) keeps a film of
//! `film_for(rain)` (0.15 to 0.7 mm) on the asphalt, reached in a minute or so, dried in some ten
//! (it drains off the camber and evaporates); every tyre that rolls over a spot squeezes some of
//! its water away, so with cars going round, the racing line dries first. The puddles stand in the
//! asphalt's hollows, the same hollows `world-render`'s `track.wgsl` draws them in (`puddle`, the
//! same noise), as deep as `PUDDLE_DEPTH` at their deepest in a downpour; they fill and drain
//! slower than the film. Everything is a pure function of the steps: the same rain, the same water.

use std::sync::Arc;

use crate::circuit::{Circuit, Located, STEP};
use crate::math::{interpolate, smoothstep};

use super::ground::Surface;

/// The Magic Formula's curvature (E) is lowered by this on a fully wet road: a peakier curve.
pub const WET_CURVATURE: f64 = 0.3;
/// The deepest puddle in a downpour, m.
pub const PUDDLE_DEPTH: f64 = 0.0025;
/// How far a tyre rolls into deeper water before it rides on all of it, m: some three lengths of
/// its contact, over which the wedge of water forms ahead of it and the tread's grooves fill.
pub const WEDGE: f64 = 0.6;
/// Lanes of `RoadWater` across a circuit, each a metre, centred on its centre line.
const LANES: usize = 36;
/// Seconds between the updates of the whole film.
const EVERY: f64 = 0.5;
/// Share of the film under a tyre that a tyre takes away rolling over a sample (`STEP` metres).
const WIPE: f64 = 0.12;

/// Water depth (mm) and the share of the dry friction kept at low speed, from the handbook's
/// 50 km/h row (0.2, 1 and 2 mm), beginning from dry and going on gently beyond.
const LOW_SPEED: ([f64; 7], [f64; 7]) = ([0.0, 0.03, 0.1, 0.2, 1.0, 2.0, 5.0], [1.0, 0.9, 0.8, 0.765, 0.647, 0.59, 0.55]);

/// The speed a tyre aquaplanes at in `mm` of water (km/h), and how sharply (the exponent).
fn aquaplaning(mm: f64) -> (f64, f64) {
    let mm = mm.max(0.01);
    if mm < 1.0 {
        (96.0 * mm.powf(-0.9), 5.4)
    } else {
        (96.0 * mm.powf(-0.42), (5.4 * mm.powf(0.83)).min(14.0))
    }
}

/// Share of the dry asphalt's friction a tyre keeps on asphalt under `mm` of water at `kmh`.
pub fn wet_asphalt(mm: f64, kmh: f64) -> f64 {
    if mm <= 0.0 {
        return 1.0;
    }
    let low = interpolate(&LOW_SPEED.0, &LOW_SPEED.1, mm);
    let (vc, n) = aquaplaning(mm);
    let left = 1.0 / (1.0 + (kmh.max(0.0) / vc).powf(n));
    (low * left).max(0.03)
}

/// The grip of a tyre (a share of what it has on dry asphalt, as `Surface::grip`) on `surface`
/// with `paint` of it painted (0 to 1), under `water` metres of water, rolling at `speed` m/s; and
/// how wet the contact is (0 dry to 1), for the shape of the curve.
pub fn wet_grip(surface: Surface, paint: f64, water: f64, speed: f64) -> (f64, f64) {
    let dry = surface.grip();
    if water <= 0.0 {
        return (dry, 0.0);
    }
    let mm = water * 1000.0;
    let wet = smoothstep(0.0, 0.1, mm);
    let share = match surface {
        Surface::Asphalt | Surface::Concrete => wet_asphalt(mm, speed * 3.6) * (1.0 - 0.3 * wet * paint.clamp(0.0, 1.0)),
        Surface::Kerb => wet_asphalt(mm, speed * 3.6) * (1.0 - 0.3 * wet),
        Surface::Grass => 1.0 - 0.45 * wet,
        Surface::Dirt => 1.0 - 0.4 * wet,
        Surface::Gravel => 1.0 - 0.1 * wet,
        Surface::Water => 1.0,
    };
    (dry * share, wet)
}

/// The film of water that rain of `rain` (0 to 1) keeps on a cambered asphalt, m: 0.15 mm in a
/// drizzle to 0.7 mm in a downpour (Gallaway et al., *Pavement and geometric design criteria for
/// minimizing hydroplaning*, FHWA-RD-79-31, 1979: some tenths of a millimetre above the texture
/// for a few metres of drainage at 2 % and 5 to 25 mm/h of rain).
pub fn film_for(rain: f64) -> f64 {
    if rain <= 0.0 {
        0.0
    } else {
        (0.15 + 0.55 * rain.min(1.0)) / 1000.0
    }
}

/// How full the puddles are (0 to 1) when rain of `rain` has fallen for long: what the renderer
/// draws as `TrackRenderer::wet`.
pub fn standing_for(rain: f64) -> f64 {
    if rain <= 0.0 {
        0.0
    } else {
        0.3 + 0.7 * rain.min(1.0)
    }
}

/// How much of a puddle is at (`across`, `along`) of a circuit when the puddles are `standing`
/// full (0 to 1): `track.wgsl`'s `puddle`, the same noise at the same place (its `hollow`).
pub fn puddle(across: f64, along: f64, standing: f64, kerb: bool) -> f64 {
    if standing <= 0.0 {
        return 0.0;
    }
    let hollow = value_noise(across / 5.0, along / 5.0) * 0.65 + value_noise(across / 1.7 + 3.1, along / 1.7 + 3.1) * 0.35;
    smoothstep(0.70 - 0.10 * standing, 0.76, hollow) * standing * if kerb { 0.3 } else { 1.0 }
}

/// `ihash` of `world-render`'s `shaders.wgsl`: a number in [0, 1) for a cell.
fn hash(x: i32, y: i32) -> f64 {
    let mut h: u32 = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f64 / 16_777_216.0
}

/// `vnoise` of `shaders.wgsl` with a period of 4096 cells.
fn value_noise(x: f64, y: f64) -> f64 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (mut u, mut v) = (x - fx, y - fy);
    u = u * u * (3.0 - 2.0 * u);
    v = v * v * (3.0 - 2.0 * v);
    let m = 4095;
    let (x0, y0, x1, y1) = (ix & m, iy & m, (ix + 1) & m, (iy + 1) & m);
    let (a, b, c, d) = (hash(x0, y0), hash(x1, y0), hash(x0, y1), hash(x1, y1));
    let top = a + (b - a) * u;
    let bottom = c + (d - c) * u;
    top + (bottom - top) * v
}

/// The water on a circuit: see the module.
#[derive(Clone, Debug)]
pub struct RoadWater {
    pub circuit: Arc<Circuit>,
    /// The film per sample and lane, m (empty while the circuit is dry).
    film: Vec<f32>,
    /// How full the puddles are, 0 to 1 (the renderer's `wet`).
    pub standing: f64,
    /// Seconds since the film was last brought up to date.
    since: f64,
}

impl RoadWater {
    pub fn new(circuit: Arc<Circuit>) -> RoadWater {
        RoadWater { circuit, film: Vec::new(), standing: 0.0, since: 0.0 }
    }

    /// Whether there is no water at all.
    pub fn dry(&self) -> bool {
        self.film.is_empty() && self.standing <= 0.0
    }

    /// As it is after rain of `rain` has fallen for long (or dry, for 0): for a start.
    pub fn settle(&mut self, rain: f64) {
        self.standing = standing_for(rain);
        let film = film_for(rain) as f32;
        if film > 0.0 {
            self.film = vec![film; self.circuit.samples.len() * LANES];
        } else {
            self.film.clear();
        }
    }

    /// `dt` seconds of rain of `rain`.
    pub fn step(&mut self, dt: f64, rain: f64) {
        // The puddles fill in some two minutes and take a quarter of an hour to drain.
        let target = standing_for(rain);
        let tau = if target > self.standing { 120.0 } else { 900.0 };
        self.standing += (target - self.standing) * (1.0 - (-dt / tau).exp());
        if self.standing < 1e-3 && target == 0.0 {
            self.standing = 0.0;
        }
        self.since += dt;
        if self.since < EVERY {
            return;
        }
        let dt = std::mem::take(&mut self.since);
        let film = film_for(rain) as f32;
        if self.film.is_empty() {
            if film <= 0.0 {
                return;
            }
            self.film = vec![0.0; self.circuit.samples.len() * LANES];
        }
        // Wetting in a minute; drying (draining off the camber, evaporating) in some ten.
        let (wetting, drying) = ((1.0 - (-dt / 60.0).exp()) as f32, (1.0 - (-dt / 600.0).exp()) as f32);
        let mut any = false;
        for f in &mut self.film {
            *f += (film - *f) * if film > *f { wetting } else { drying };
            if *f < 2e-6 && film == 0.0 {
                *f = 0.0;
            }
            any |= *f > 0.0;
        }
        if !any {
            self.film.clear();
        }
    }

    /// The cell of the film at a point of the circuit.
    fn cell(&self, along: f64, across: f64) -> Option<usize> {
        let lane = (across + LANES as f64 / 2.0).floor();
        if !(0.0..LANES as f64).contains(&lane) {
            return None;
        }
        let i = (along.rem_euclid(self.circuit.length) / STEP) as usize % self.circuit.samples.len();
        Some(i * LANES + lane as usize)
    }

    /// The film at a point of the circuit, m.
    pub fn film(&self, along: f64, across: f64) -> f64 {
        if self.film.is_empty() {
            return 0.0;
        }
        self.cell(along, across).map_or(0.0, |k| self.film[k] as f64)
    }

    /// The water at a located point, m: the film and any puddle; `kerb` on a kerb (which holds less).
    pub fn depth(&self, at: &Located, kerb: bool) -> f64 {
        let film = self.film(at.along, at.across);
        film + puddle(at.across, at.along, self.standing, kerb) * PUDDLE_DEPTH * self.standing
    }

    /// A tyre has rolled `rolled` metres over the point: it squeezes away some of the film there.
    pub fn wipe(&mut self, along: f64, across: f64, rolled: f64) {
        if self.film.is_empty() {
            return;
        }
        if let Some(k) = self.cell(along, across) {
            self.film[k] *= (1.0 - WIPE * (rolled / STEP).min(1.0)) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handbook_table() {
        // (mm, km/h, the handbook's share of dry: new tyre, 0.85 dry at 50 and 0.8 at 90).
        for (mm, kmh, share) in [(0.2, 50.0, 0.65 / 0.85), (1.0, 50.0, 0.55 / 0.85), (2.0, 50.0, 0.5 / 0.85), (0.2, 90.0, 0.6 / 0.8), (1.0, 90.0, 0.3 / 0.8), (2.0, 90.0, 0.05 / 0.8)] {
            let got = wet_asphalt(mm, kmh);
            println!("{mm} mm at {kmh} km/h: {got:.3} of dry (handbook {share:.3})");
            assert!((got - share).abs() < 0.06, "{mm} mm at {kmh} km/h: {got:.3}, not {share:.3}");
        }
        // A thin film hardly cares for speed; a deep one is lost.
        assert!(wet_asphalt(0.2, 200.0) > 0.6, "{}", wet_asphalt(0.2, 200.0));
        assert!(wet_asphalt(3.0, 120.0) < 0.05);
        assert_eq!(wet_asphalt(0.0, 300.0), 1.0);
    }

    #[test]
    fn noise_is_the_shaders() {
        // Cells hash to [0, 1), the noise stays in it and is continuous across a cell's edge.
        for k in 0..200 {
            let (x, y) = (k as f64 * 0.37 - 30.0, k as f64 * 1.91 - 100.0);
            let v = value_noise(x, y);
            assert!((0.0..1.0).contains(&v));
            assert!((value_noise(x.floor() - 1e-9, y) - value_noise(x.floor() + 1e-9, y)).abs() < 1e-6);
        }
    }
}
