//! The roofs of the buildings of the maps as shapes: how high the roof stands over each point of a
//! building's outline. The simulation runs into them (`solids`) and the world draws them
//! (`world_render`'s `footprints`) from the same numbers, so what is seen is what is hit.
//!
//! A roof is laid in its `Frame`: the box round the outline along its ridge (`u` along, `v`
//! across), the eaves where the walls end and how much higher it rises. Each shape is then a
//! height over the eaves at a point:
//!
//! - gabled, round (a barrel vault) and gambrel: a profile across the ridge, the same all along it
//!   (`profile`), so on a building of any outline the walls rise to meet it, as gables do;
//! - skillion: one plane rising across the building;
//! - hipped, pyramidal and mansard: a plane rising from every side of the outline, the lowest of
//!   them at each point (the straight skeleton of a convex outline: hips where two meet, a ridge
//!   where the long sides do); on an outline that is not convex, a gabled roof instead;
//! - dome and onion: round over the middle of the outline, on a flat roof round it.

use crate::region::{Footprint, Roof};

/// The axes and heights of a roof.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The middle of the box round the outline, north and east.
    pub centre: (f64, f64),
    /// The direction of the ridge (north, east), a unit vector: `u` runs along it, `v` across it
    /// (to the east of a ridge that runs north).
    pub along: (f64, f64),
    pub half_along: f64,
    pub half_across: f64,
    /// Where the walls end and the roof begins, metres above the building's foot.
    pub eaves: f64,
    /// How much higher than the eaves the roof rises.
    pub rise: f64,
    pub shape: Shape,
}

/// How a roof's height is worked out (`Frame::height`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Flat,
    /// A profile across the ridge: gabled, round, gambrel.
    Profile(Roof),
    /// One plane rising from `v = -half_across` to `v = half_across`.
    Skillion,
    /// The lowest of the planes rising from every side, capped at the rise (mansard: steep, with a
    /// flat top); `pyramid`: each plane steep enough to reach the rise at the middle.
    Planes { pyramid: bool, mansard: bool },
    /// Round over a circle of `half_across` round the centre: a dome, or an onion.
    Dome { onion: bool },
}

impl Frame {
    /// The roof of a building, from what the maps say of it and its outline.
    pub fn of(b: &Footprint) -> Frame {
        let (centre, heading, _, _) = b.oriented_box();
        // The ridge runs along the direction given, else along the longest side.
        let ridge = b.direction.map_or(heading, |d| d as f64);
        let (c, s) = (ridge.cos(), ridge.sin());
        // The half sizes along and across that direction.
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &b.ring {
            let (dn, de) = (p[0] as f64 - centre.0, p[1] as f64 - centre.1);
            let (u, v) = (dn * c + de * s, -dn * s + de * c);
            lo = [lo[0].min(u), lo[1].min(v)];
            hi = [hi[0].max(u), hi[1].max(v)];
        }
        let (mid_u, mid_v) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
        let centre = (centre.0 + mid_u * c - mid_v * s, centre.1 + mid_u * s + mid_v * c);
        let (half_along, half_across) = (((hi[0] - lo[0]) / 2.0).max(0.1), ((hi[1] - lo[1]) / 2.0).max(0.1));
        let shape = match b.roof {
            Roof::Flat => Shape::Flat,
            Roof::Gabled | Roof::Round | Roof::Gambrel => Shape::Profile(b.roof),
            Roof::Skillion => Shape::Skillion,
            Roof::Hipped | Roof::Pyramidal | Roof::Mansard if b.holes.is_empty() && convex(&b.ring) => Shape::Planes { pyramid: b.roof == Roof::Pyramidal, mansard: b.roof == Roof::Mansard },
            Roof::Hipped | Roof::Pyramidal | Roof::Mansard => Shape::Profile(Roof::Gabled),
            Roof::Dome | Roof::Onion => Shape::Dome { onion: b.roof == Roof::Onion },
        };
        let half_across = if let Shape::Dome { .. } = shape { half_across.min(half_along) } else { half_across };
        let height = b.height as f64 - b.floor as f64;
        let rise = if shape == Shape::Flat {
            0.0
        } else if b.roof_height > 0.0 {
            b.roof_height as f64
        } else {
            match shape {
                Shape::Dome { onion: false } => half_across,
                Shape::Dome { onion: true } => 1.7 * half_across,
                Shape::Skillion => (0.35 * half_across).clamp(0.8, 4.0),
                Shape::Profile(Roof::Round) => half_across.min(10.0),
                Shape::Planes { pyramid: true, .. } => (0.75 * half_across).clamp(1.5, 12.0),
                Shape::Planes { mansard: true, .. } => (0.5 * half_across).clamp(2.5, 5.0),
                _ => (0.6 * half_across).clamp(1.2, 8.0),
            }
        };
        // A roof never takes more than most of the building: it keeps a storey of walls under it.
        let rise = rise.min((height - 2.5).max(height * 0.5)).max(0.0);
        Frame { centre, along: (c, s), half_along, half_across, eaves: height - rise + b.floor as f64, rise, shape }
    }

    /// A point (north, east) in the roof's axes: along the ridge and across it.
    pub fn local(&self, n: f64, e: f64) -> (f64, f64) {
        let (dn, de) = (n - self.centre.0, e - self.centre.1);
        let (c, s) = self.along;
        (dn * c + de * s, -dn * s + de * c)
    }

    /// The point (north, east) at `u` along the ridge and `v` across it.
    pub fn world(&self, u: f64, v: f64) -> (f64, f64) {
        let (c, s) = self.along;
        (self.centre.0 + u * c - v * s, self.centre.1 + u * s + v * c)
    }

    /// How high the roof stands over a point of the outline (north, east), metres above the
    /// building's foot: the eaves and what rises above them there. `ring` is the outline (for the
    /// planes of a hipped roof).
    pub fn height(&self, ring: &[[f32; 2]], n: f64, e: f64) -> f64 {
        self.eaves + self.rise_at(ring, n, e)
    }

    /// How much the roof rises over the eaves at a point.
    pub fn rise_at(&self, ring: &[[f32; 2]], n: f64, e: f64) -> f64 {
        if self.rise <= 0.0 {
            return 0.0;
        }
        let (_, v) = self.local(n, e);
        match self.shape {
            Shape::Flat => 0.0,
            Shape::Profile(roof) => self.rise * profile(roof, v / self.half_across),
            Shape::Skillion => self.rise * ((v / self.half_across + 1.0) / 2.0).clamp(0.0, 1.0),
            Shape::Planes { pyramid, mansard } => {
                let (cn, ce) = self.centre;
                let mut least = f64::INFINITY;
                for (i, a) in ring.iter().enumerate() {
                    let b = ring[(i + 1) % ring.len()];
                    let d = edge_distance(*a, b, n, e);
                    // How steep the plane of this side is: all alike, steep enough to reach the
                    // rise across the short half; for a pyramid each reaches it at the middle.
                    let reach = if pyramid { edge_distance(*a, b, cn, ce).max(0.5) } else if mansard { self.half_across.min(self.half_along) * 0.3 } else { self.half_across };
                    least = least.min(d / reach);
                }
                self.rise * least.clamp(0.0, 1.0)
            }
            Shape::Dome { onion } => {
                let (u, v) = self.local(n, e);
                let r = u.hypot(v) / self.half_across;
                if r >= 1.0 {
                    0.0
                } else if onion {
                    self.rise * onion_height(r)
                } else {
                    self.rise * (1.0 - r * r).sqrt()
                }
            }
        }
    }

    /// The values of `v` (across the ridge) where a profile roof bends: its faces are flat between
    /// them, so a mesh splits the roof there.
    pub fn bends(&self) -> Vec<f64> {
        let h = self.half_across;
        match self.shape {
            Shape::Profile(Roof::Gambrel) => vec![-h * GAMBREL_KNEE, 0.0, h * GAMBREL_KNEE],
            Shape::Profile(Roof::Round) => (1..ROUND_FACES).map(|k| -h * (std::f64::consts::PI * k as f64 / ROUND_FACES as f64).cos()).collect(),
            Shape::Profile(_) => vec![0.0],
            _ => Vec::new(),
        }
    }
}

/// Faces of a round roof across its width.
pub const ROUND_FACES: usize = 8;
/// Where a gambrel roof's slopes bend, as a share of the half width out from the ridge, and how
/// much of the rise the upper slope takes.
pub const GAMBREL_KNEE: f64 = 0.55;
const GAMBREL_UPPER: f64 = 0.3;

/// The height of a profile roof (as a share of its rise) at `t` across it, -1 and 1 its eaves.
pub fn profile(roof: Roof, t: f64) -> f64 {
    let t = t.abs().min(1.0);
    match roof {
        Roof::Round => {
            // Flat between the bends, on the circle at them.
            let angle = t.clamp(-1.0, 1.0).acos();
            let step = std::f64::consts::PI / ROUND_FACES as f64;
            let k = (angle / step).floor();
            let (a0, a1) = (k * step, (k + 1.0) * step);
            let (t0, t1) = (a0.cos(), a1.cos());
            let (h0, h1) = (a0.sin(), a1.sin());
            if (t0 - t1).abs() < 1e-9 {
                h0
            } else {
                h0 + (h1 - h0) * (t0 - t) / (t0 - t1)
            }
        }
        Roof::Gambrel => {
            if t > GAMBREL_KNEE {
                (1.0 - GAMBREL_UPPER) * (1.0 - t) / (1.0 - GAMBREL_KNEE)
            } else {
                1.0 - GAMBREL_UPPER * t / GAMBREL_KNEE
            }
        }
        _ => 1.0 - t,
    }
}

/// An onion dome's height at `r` out from its middle (0 to 1), as a share of its rise: the top of
/// its profile (`onion_radius`) that reaches out that far.
pub fn onion_height(r: f64) -> f64 {
    (0..=64).map(|k| 1.0 - k as f64 / 64.0).find(|&s| onion_radius(s) >= r).unwrap_or(0.0)
}

/// The radius of an onion dome, a share of its drum's, at `s` up its height (0 the drum, 1 the
/// point): it swells to some 1.17 a third of the way up, then narrows to the point.
pub fn onion_radius(s: f64) -> f64 {
    (1.0 - s.clamp(0.0, 1.0).powf(1.5)) * (1.0 + 0.5 * (std::f64::consts::PI * s).sin())
}

/// Distance from a point to the line through a side of an outline (inside it: the side's own plane).
fn edge_distance(a: [f32; 2], b: [f32; 2], n: f64, e: f64) -> f64 {
    let (an, ae, bn, be) = (a[0] as f64, a[1] as f64, b[0] as f64, b[1] as f64);
    let (dn, de) = (bn - an, be - ae);
    let length = dn.hypot(de).max(1e-9);
    ((n - an) * de - (e - ae) * dn).abs() / length
}

/// Whether a ring is convex (corners all turning the same way, a little slack for nearly straight ones).
pub fn convex(ring: &[[f32; 2]]) -> bool {
    let k = ring.len();
    if k < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..k {
        let (a, b, c) = (ring[i], ring[(i + 1) % k], ring[(i + 2) % k]);
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        let scale = (b[0] - a[0]).hypot(b[1] - a[1]) * (c[0] - b[0]).hypot(c[1] - b[1]);
        if cross.abs() < 0.02 * scale {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn house(roof: Roof) -> Footprint {
        // 20 m along north, 10 m across east, 10 m to the ridge.
        Footprint::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0]], 10.0, 0.0, roof)
    }

    #[test]
    fn a_gabled_roof_rises_from_the_long_sides_to_a_ridge_along_the_middle() {
        let b = house(Roof::Gabled);
        let f = Frame::of(&b);
        assert!((f.half_along - 10.0).abs() < 1e-6 && (f.half_across - 5.0).abs() < 1e-6, "{f:?}");
        assert!((f.eaves + f.rise - 10.0).abs() < 1e-6);
        // At the eaves on both long sides, at the top all along the ridge.
        assert!((f.height(&b.ring, 10.0, 0.0) - f.eaves).abs() < 1e-6);
        assert!((f.height(&b.ring, 10.0, 10.0) - f.eaves).abs() < 1e-6);
        assert!((f.height(&b.ring, 2.0, 5.0) - 10.0).abs() < 1e-6);
        assert!((f.height(&b.ring, 18.0, 5.0) - 10.0).abs() < 1e-6);
        assert_eq!(f.bends(), vec![0.0]);
    }

    #[test]
    fn a_hipped_roof_slopes_from_every_side_and_a_pyramid_to_its_middle() {
        let b = house(Roof::Hipped);
        let f = Frame::of(&b);
        let h = |n, e| f.height(&b.ring, n, e);
        // The ends slope too: at the middle of a short side it is at the eaves.
        assert!((h(0.0, 5.0) - f.eaves).abs() < 1e-6);
        assert!((h(10.0, 5.0) - 10.0).abs() < 1e-6, "the ridge");
        assert!(h(2.0, 5.0) < h(5.0, 5.0));
        let p = house(Roof::Pyramidal);
        let f = Frame::of(&p);
        // Only the middle reaches the top.
        assert!((f.height(&p.ring, 10.0, 5.0) - 10.0).abs() < 1e-6);
        assert!(f.height(&p.ring, 6.0, 5.0) < 9.9);
    }

    #[test]
    fn a_dome_is_round_and_an_l_shaped_hipped_house_gets_gables() {
        let mut b = house(Roof::Dome);
        b.ring = (0..16).map(|k| {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            [10.0 * a.cos(), 10.0 * a.sin()]
        }).collect();
        b.height = 20.0;
        let f = Frame::of(&b);
        assert!((f.rise - f.half_across).abs() < 1e-6 && f.half_across > 9.0);
        assert!((f.height(&b.ring, 0.0, 0.0) - 20.0).abs() < 0.01);
        assert!(f.height(&b.ring, 0.0, 9.9) < f.eaves + 2.0);
        let l = Footprint::new(vec![[0.0, 0.0], [20.0, 0.0], [20.0, 6.0], [6.0, 6.0], [6.0, 14.0], [0.0, 14.0]], 9.0, 0.0, Roof::Hipped);
        assert!(!convex(&l.ring) && convex(&b.ring));
        assert_eq!(Frame::of(&l).shape, Shape::Profile(Roof::Gabled));
    }

    #[test]
    fn profiles_meet_their_eaves_and_ridge() {
        for roof in [Roof::Gabled, Roof::Round, Roof::Gambrel] {
            assert!((profile(roof, 0.0) - 1.0).abs() < 1e-9, "{roof:?}");
            assert!(profile(roof, 1.0).abs() < 1e-9, "{roof:?}");
            assert!(profile(roof, -0.5) == profile(roof, 0.5));
            let mut last = 2.0;
            for k in 0..=20 {
                let h = profile(roof, k as f64 / 20.0);
                assert!(h <= last + 1e-9, "{roof:?} falls outwards");
                last = h;
            }
        }
        // An onion swells out beyond its drum, then narrows to its point.
        assert!(onion_height(0.0) > 0.95 && onion_height(1.1) > 0.0 && onion_height(1.1) < onion_height(0.5) && onion_height(1.25) == 0.0);
    }
}
