//! The state of the water for the wind: the trains of waves its surface is drawn with, and how
//! rough it is below the shortest of them.
//!
//! The surface is a sum of [`MOST_WAVES`] trains: three of a swell of 156 m that runs in from far off,
//! and the wind's, from two and a half times the length of its peak (at most 220 m) down to ripples
//! of 25 cm, so that a light wind has many trains to each octave and its crests do not line up. Each
//! is a sine with the deep-water speed of its length (ω² = g k) running downwind in a spread of
//! directions. What each band of wavelength holds of the slopes is cut below the peak of the sea
//! the wind raises (Pierson and Moskowitz: 88 m at 10 m/s), rises as the square root of the wave
//! number just above it (Toba's equilibrium range) and is the same for every octave from nine times
//! it on (Phillips' saturation range); a fully developed sea comes out as tall as Pierson and
//! Moskowitz measured, within a fifth. The whole is scaled so that the mean square slope is Cox and
//! Munk's (1954) for the wind, measured from the sun's glitter on the sea: what the trains leave out
//! (ripples under 25 cm, and the trains a quality drops) is given as a slope covariance, the
//! roughness the shaders spread the light with. A shader drops each train it cannot resolve at a
//! pixel and adds its slopes to that roughness instead, so the water looks the same near and far
//! and does not shimmer (`sea_waves` in shaders.wgsl). Still air leaves the sea of a light breeze,
//! and a swell of 70 cm runs under whatever the wind raises. Near the peak the trains run within
//! some 35° of the wind, the short ones wider.
//!
//! The water's height is not moved: the simulation floats and ditches on a flat sea, and so is
//! what is drawn. Only the slopes, and the foam of the crests, come from these waves.

use glam::{DVec2, DVec3};
use std::f64::consts::TAU;

/// Trains of waves at most (`FrameUniforms::waves`).
pub const MOST_WAVES: usize = 32;
/// The longest train of the wind's and the shortest of all, metres.
const LONGEST: f64 = 220.0;
/// The wind's longest train is this many times its peak's length: longer ones hold nothing.
const PEAK_REACH: f64 = 2.5;
/// Trains that carry the swell, the first ones.
const SWELLS: usize = 3;
const SHORTEST: f64 = 0.25;
/// Below this length the slopes of capillary waves are left out: they hold little.
const CAPILLARY: f64 = 0.01;
const G: f64 = 9.81;
/// The swell that runs in from far off whatever the wind here: its height (m) and period (s).
const SWELL_HEIGHT: f64 = 0.7;
const SWELL_PERIOD: f64 = 10.0;
/// How much more slope a band of short waves holds than one at the peak of the sea.
const SATURATED: f64 = 3.0;
/// The sea is never stiller than a light breeze leaves it (m/s): it keeps the waves of the wind
/// that blew before. Whitecaps go by the wind there is.
const LEAST_WIND: f64 = 3.0;

/// One train of waves: height `slope / |k|` times `sin(k·x - ωt + phase)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    /// Wave vector in render axes (x east, z south), radians per metre; it runs along it.
    pub k: DVec2,
    /// Steepness: the amplitude of its slope, a·|k|.
    pub slope: f64,
    /// Phase at the world's origin at time 0, radians.
    pub phase: f64,
}

impl Wave {
    pub fn length(&self) -> f64 {
        TAU / self.k.length()
    }

    /// Angular frequency of a deep-water wave of this length.
    pub fn omega(&self) -> f64 {
        (G * self.k.length()).sqrt()
    }
}

/// The sea for one wind, with the trains a quality draws.
#[derive(Clone, Debug, PartialEq)]
pub struct Sea {
    /// Wind speed 10 m up, m/s.
    pub wind: f64,
    /// The way the wind blows to, render axes (x east, z south), unit.
    pub downwind: DVec2,
    /// From the longest to the shortest.
    pub waves: Vec<Wave>,
    /// Slope covariance of what the trains leave out, render axes: (xx, zz, xz).
    pub residual: [f64; 3],
    /// Significant wave height (four standard deviations of the surface's height), m.
    pub height: f64,
    /// The period of the waves that reach the shore, s: the wind's sea or the swell, the taller.
    pub period: f64,
    /// Part of the sea white with breaking crests (Monahan and O'Muircheartaigh 1980), 0..1.
    pub whitecaps: f64,
}

impl Sea {
    /// The sea for a mean wind near the surface (world axes: north, east, down, m/s), drawn with
    /// `count` trains (at most [`MOST_WAVES`]; the shortest are left out first).
    pub fn new(wind: DVec3, count: usize) -> Sea {
        let count = count.clamp(SWELLS, MOST_WAVES);
        let speed = (wind.x * wind.x + wind.y * wind.y).sqrt();
        // Without wind the swell comes from the west-south-west, the weather's own default.
        let downwind = if speed > 0.2 { DVec2::new(wind.y, -wind.x) / speed } else { DVec2::new(70f64.to_radians().sin(), -70f64.to_radians().cos()) };

        // The peak of the wind's sea (Pierson and Moskowitz 1964): ω = 0.855 g / U.
        let u = speed.max(LEAST_WIND);
        let k_peak = (0.855 * G / u).powi(2) / G;
        // The wind's trains share out the lengths from a little longer than the peak down to the
        // shortest: in a light wind they are many to each octave, and their crests do not line up.
        let longest = (PEAK_REACH * TAU / k_peak).min(LONGEST);
        let trains = MOST_WAVES - SWELLS;
        let ratio = (longest / SHORTEST).powf(1.0 / (trains - 1) as f64);
        let band = ratio.ln();
        // Slope per band of wavelength: cut below the peak, rising as the square root of k just
        // above it (Toba's equilibrium range) and level from nine times its k on (saturation).
        let share = |k: f64| (-1.25 * (k_peak / k).powi(2)).exp() * (k / k_peak).sqrt().clamp(1.0, SATURATED);
        let lengths: Vec<f64> = (0..trains).map(|i| longest / ratio.powi(i as i32)).collect();
        // Slope variance of each train, and of everything shorter than the last, in bands.
        let weights: Vec<f64> = lengths.iter().map(|&l| share(TAU / l)).collect();
        let k_last = TAU / SHORTEST;
        let below = ((SHORTEST / CAPILLARY).ln() / band - 0.5).max(0.0) * share(k_last * 4.0);
        let left_out: f64 = weights[count - SWELLS..].iter().sum::<f64>() + below;
        let total = weights.iter().sum::<f64>() + below;

        // Cox and Munk: mean square slope along and across the wind (3.16e-3 U and 0.003 + 1.92e-3 U).
        // The 0.003 of a calm sea is split between the two, so that it is round.
        let (along, across) = (0.0015 + 3.16e-3 * u, 0.0015 + 1.92e-3 * u);
        let per_weight = (along + across) / total;

        // The swell, first: three trains about its period from 20° off the wind, with amplitudes
        // whose four standard deviations make its height.
        let swell_length = G * SWELL_PERIOD * SWELL_PERIOD / TAU;
        let swell_dir = rotate(downwind, 0.35);
        let swell = [(1.0, 1.0, 0.0), (1.22, 0.75, 0.3), (0.84, 0.6, -0.25)];
        let norm = (0.5 * swell.iter().map(|(_, a, _)| a * a).sum::<f64>()).sqrt();
        let mut waves: Vec<Wave> = swell
            .iter()
            .enumerate()
            .map(|(i, &(length, part, turn))| {
                let k = TAU / (swell_length * length);
                Wave { k: rotate(swell_dir, turn) * k, slope: SWELL_HEIGHT / 4.0 * part / norm * k, phase: fract(i as f64 * 0.754_877 + 0.17) * TAU }
            })
            .collect();

        let mut height_variance = 0.0;
        for (i, (&length, &weight)) in lengths.iter().zip(&weights).enumerate() {
            let k = TAU / length;
            let slope = (2.0 * weight * per_weight).sqrt();
            height_variance += 0.5 * (slope / k).powi(2);
            // Near the peak the waves run within some 35° of the wind, short ones spread wider; a
            // golden-ratio sequence keeps the directions apart.
            let spread = 0.6 + 0.5 * ((k / k_peak).ln() / 4.0).clamp(0.0, 1.0);
            let turn = (fract(i as f64 * 0.618_034 + 0.31) * 2.0 - 1.0) * spread;
            let phase = fract((i + SWELLS) as f64 * 0.754_877 + 0.17) * TAU;
            waves.push(Wave { k: rotate(downwind, turn) * k, slope, phase });
        }
        // Longest first, as the shaders take them; the shortest are left out.
        waves.sort_by(|a, b| a.k.length().total_cmp(&b.k.length()));
        waves.truncate(count);
        let wind_height = 4.0 * height_variance.sqrt();

        let rotate_cov = |a: f64, c: f64| {
            let (x, z) = (downwind.x, downwind.y);
            [a * x * x + c * z * z, a * z * z + c * x * x, (a - c) * x * z]
        };
        let lost = left_out / total;
        let height = (wind_height * wind_height + SWELL_HEIGHT * SWELL_HEIGHT).sqrt();
        let wind_period = TAU / (0.855 * G / u);
        Sea {
            wind: speed,
            downwind,
            residual: rotate_cov(along * lost, across * lost),
            height,
            period: if wind_height > SWELL_HEIGHT { wind_period } else { SWELL_PERIOD },
            whitecaps: (3.84e-6 * speed.powf(3.41)).min(0.3),
            waves,
        }
    }

    /// The mean square slope of the whole surface, along x and z: what the trains hold and the residual.
    #[cfg(test)]
    fn slope_variance(&self) -> (f64, f64) {
        self.waves.iter().fold((self.residual[0], self.residual[1]), |(x, z), w| {
            let d = w.k.normalize();
            (x + 0.5 * w.slope * w.slope * d.x * d.x, z + 0.5 * w.slope * w.slope * d.y * d.y)
        })
    }

    /// The trains for the shaders at a time (s) and for a camera at (x east, z south) in render
    /// axes: per train xy = the way it runs (unit), z = waves per metre, w = its phase at the camera
    /// in turns (0..1), so that the shaders work in coordinates relative to it and need no wrapping.
    pub fn uniforms(&self, time: f64, cam_x: f64, cam_z: f64) -> [[f32; 4]; MOST_WAVES] {
        let mut out = [[0.0; 4]; MOST_WAVES];
        for (o, w) in out.iter_mut().zip(&self.waves) {
            let phase = (w.phase + w.k.x * cam_x + w.k.y * cam_z - w.omega() * time).rem_euclid(TAU);
            let d = w.k.normalize();
            *o = [d.x as f32, d.y as f32, (1.0 / w.length()) as f32, (phase / TAU) as f32];
        }
        out
    }

    /// Per train: xyz = the slope covariance (xx, zz, xz) of it and of every shorter one, what a
    /// pixel too coarse for all of them spreads the light with; w = its steepness.
    pub fn rest(&self) -> [[f32; 4]; MOST_WAVES] {
        let mut out = [[0.0; 4]; MOST_WAVES];
        let mut sum = [0.0f64; 3];
        for (o, w) in out.iter_mut().zip(&self.waves).rev() {
            let (d, v) = (w.k.normalize(), 0.5 * w.slope * w.slope);
            sum = [sum[0] + v * d.x * d.x, sum[1] + v * d.y * d.y, sum[2] + v * d.x * d.y];
            *o = [sum[0] as f32, sum[1] as f32, sum[2] as f32, w.slope as f32];
        }
        out
    }

    /// How many standard deviations of the height above the mean a crest must stand to break, so
    /// that the part of the sea that does is `whitecaps`.
    pub fn whitecap_threshold(&self) -> f64 {
        normal_quantile(1.0 - self.whitecaps.max(1e-6))
    }
}

fn fract(x: f64) -> f64 {
    x - x.floor()
}

fn rotate(v: DVec2, angle: f64) -> DVec2 {
    let (s, c) = angle.sin_cos();
    DVec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// The value a standard normal variable stays under with probability `p` (Abramowitz and Stegun
/// 26.2.23, within 4.5e-4).
fn normal_quantile(p: f64) -> f64 {
    let p = p.clamp(1e-9, 1.0 - 1e-9);
    let (q, sign) = if p < 0.5 { (p, -1.0) } else { (1.0 - p, 1.0) };
    let t = (-2.0 * q.ln()).sqrt();
    sign * (t - (2.515_517 + 0.802_853 * t + 0.010_328 * t * t) / (1.0 + 1.432_788 * t + 0.189_269 * t * t + 0.001_308 * t * t * t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wind(knots: f64, from_deg: f64) -> DVec3 {
        let from = from_deg.to_radians();
        DVec3::new(-from.cos(), -from.sin(), 0.0) * knots * 0.514_444
    }

    #[test]
    fn the_slopes_are_cox_and_munks() {
        for speed in [0.0, 3.0, 7.0, 12.0, 20.0] {
            for count in [MOST_WAVES, 24, 12] {
                let sea = Sea::new(wind(speed / 0.514_444, 250.0), count);
                let (x, z) = sea.slope_variance();
                // Still air leaves the sea of a light breeze.
                let expected = 0.003 + 5.08e-3 * speed.max(LEAST_WIND);
                // What a pixel too coarse for every train is left with is the same.
                let rest = sea.rest()[0];
                assert!(((rest[0] + rest[1]) as f64 + sea.residual[0] + sea.residual[1] - (x + z)).abs() < 1e-6);
                // The swell adds a little on top.
                assert!((x + z) >= expected * 0.999 && (x + z) < expected + 0.002, "{speed} m/s, {count}: {} against {expected}", x + z);
            }
        }
    }

    #[test]
    fn the_slopes_lean_along_the_wind() {
        // Wind from the north blows to the south: +z in render axes.
        let sea = Sea::new(wind(20.0, 0.0), MOST_WAVES);
        assert!((sea.downwind - DVec2::new(0.0, 1.0)).length() < 1e-9);
        let (x, z) = sea.slope_variance();
        assert!(z > x * 1.2, "along {z}, across {x}");
        // Most of the waves run downwind.
        assert!(sea.waves.iter().filter(|w| w.k.normalize().dot(sea.downwind) > 0.0).count() == sea.waves.len());
    }

    #[test]
    fn the_wind_raises_the_sea_as_observed() {
        // Pierson–Moskowitz: a fully developed sea is 0.21 U² / g tall; the swell is added to it.
        for speed in [8.0, 12.0, 16.0] {
            let sea = Sea::new(wind(speed / 0.514_444, 250.0), MOST_WAVES);
            let pm = 0.21 * speed * speed / G;
            let expected = (pm * pm + SWELL_HEIGHT * SWELL_HEIGHT).sqrt();
            assert!((sea.height / expected - 1.0).abs() < 0.25, "{speed} m/s: {} m against {expected} m", sea.height);
        }
        let calm = Sea::new(DVec3::ZERO, MOST_WAVES);
        assert!(calm.height > SWELL_HEIGHT && calm.height < SWELL_HEIGHT + 0.1, "{}", calm.height);
        assert!(calm.whitecaps < 1e-5);
        let gale = Sea::new(wind(40.0, 250.0), MOST_WAVES);
        assert!(gale.whitecaps > 0.05 && gale.whitecap_threshold() < 1.7);
    }

    #[test]
    fn fewer_trains_move_their_slopes_into_the_residual() {
        let full = Sea::new(wind(15.0, 90.0), MOST_WAVES);
        let few = Sea::new(wind(15.0, 90.0), 12);
        assert_eq!(few.waves.len(), 12);
        assert_eq!(few.waves[..], full.waves[..12]);
        assert!(few.residual[0] + few.residual[1] > full.residual[0] + full.residual[1]);
    }

    #[test]
    fn the_phases_follow_the_camera_without_a_seam() {
        // The same point seen from two cameras has the same phase: the shaders add k · (point - camera).
        let sea = Sea::new(wind(10.0, 200.0), MOST_WAVES);
        let (time, p) = (1234.5, DVec2::new(5000.3, -2700.8));
        let (a, b) = (DVec2::new(100.0, 20.0), DVec2::new(4100.0, -900.0));
        let (ua, ub) = (sea.uniforms(time, a.x, a.y), sea.uniforms(time, b.x, b.y));
        for (i, w) in sea.waves.iter().enumerate() {
            let at = |u: &[[f32; 4]; MOST_WAVES], cam: DVec2| (u[i][3] as f64 * TAU + w.k.dot(p - cam)).rem_euclid(TAU);
            let d = (at(&ua, a) - at(&ub, b)).abs();
            assert!(d.min(TAU - d) < 2e-3, "wave {i}: {d}");
        }
    }

    #[test]
    fn quantiles() {
        assert!(normal_quantile(0.5).abs() < 1e-3);
        assert!((normal_quantile(0.975) - 1.96).abs() < 1e-3);
        assert!((normal_quantile(0.01) + 2.326).abs() < 1e-3);
    }
}
