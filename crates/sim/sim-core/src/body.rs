//! The rigid body every vehicle is: where it is, how it is turned, how fast it moves and turns; and
//! the fourth-order Runge-Kutta steps that move it under a force and a moment. A vehicle's dynamics
//! (`vehicle::Dynamics`) works out the force and moment from its own state and controls and the
//! environment; this is the part they all share.
//!
//! Body axes: x forward, y right, z down. World axes: north, east, down.

use glam::DVec4;

use crate::math::{act, Quat, V3};

/// A rigid body's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// World position: north, east, down.
    pub pos: V3,
    /// Inertial velocity in body axes.
    pub vel: V3,
    /// Body → world.
    pub q: Quat,
    /// Angular velocity in body axes, rad/s.
    pub omega: V3,
}

/// How fast a body's state changes: one evaluation of a Runge-Kutta step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub pos: V3,
    pub vel: V3,
    pub omega: V3,
    pub q: DVec4,
}

impl Rates {
    /// The rates of a body of `mass` and principal moments of `inertia` under a total `force` and
    /// `moment` in body axes (gravity included): Newton and Euler in the body's turning axes.
    pub fn of(body: &Body, mass: f64, inertia: V3, force: V3, moment: V3) -> Rates {
        let w = body.omega;
        Rates {
            pos: act(body.q, body.vel),
            vel: force / mass - w.cross(body.vel),
            omega: (moment - w.cross(inertia * w)) / inertia,
            q: DVec4::from(body.q * Quat::from_xyzw(w.x, w.y, w.z, 0.0)) * 0.5,
        }
    }

    /// The weighted mean of the four evaluations of a Runge-Kutta step.
    pub fn rk4(k1: &Rates, k2: &Rates, k3: &Rates, k4: &Rates) -> Rates {
        Rates {
            pos: (k1.pos + 2.0 * k2.pos + 2.0 * k3.pos + k4.pos) / 6.0,
            vel: (k1.vel + 2.0 * k2.vel + 2.0 * k3.vel + k4.vel) / 6.0,
            omega: (k1.omega + 2.0 * k2.omega + 2.0 * k3.omega + k4.omega) / 6.0,
            q: (k1.q + 2.0 * k2.q + 2.0 * k3.q + k4.q) / 6.0,
        }
    }
}

impl Body {
    /// The body `h` seconds on at these rates. The attitude is not renormalised: `normalised` does
    /// that once a whole step is done.
    pub fn advance(&self, rates: &Rates, h: f64) -> Body {
        Body {
            pos: self.pos + rates.pos * h,
            vel: self.vel + rates.vel * h,
            omega: self.omega + rates.omega * h,
            q: Quat::from_vec4(DVec4::from(self.q) + rates.q * h),
        }
    }

    /// With its attitude a unit quaternion again.
    pub fn normalised(mut self) -> Body {
        let qv = DVec4::from(self.q);
        self.q = Quat::from_vec4(qv * (1.0 / qv.length_squared().sqrt()));
        self
    }

    /// The body `k` of the way (0...1) from this one to `next`, `dt` seconds later, for a replay: the
    /// position along the curve both velocities draw, the attitude turned the short way, the rest
    /// in between.
    pub fn blend(&self, next: &Body, k: f64, dt: f64) -> Body {
        let (k2, k3) = (k * k, k * k * k);
        let (m0, m1) = (self.world_velocity() * dt, next.world_velocity() * dt);
        let pos = self.pos * (2.0 * k3 - 3.0 * k2 + 1.0) + m0 * (k3 - 2.0 * k2 + k) + next.pos * (-2.0 * k3 + 3.0 * k2) + m1 * (k3 - k2);
        Body { pos, vel: self.vel.lerp(next.vel, k), q: self.q.slerp(next.q, k), omega: self.omega.lerp(next.omega, k) }
    }

    /// Velocity over the ground, world axes.
    pub fn world_velocity(&self) -> V3 {
        act(self.q, self.vel)
    }

    /// Where a point of the body (body axes, from its centre of mass) is and how fast it moves,
    /// world axes.
    pub fn point(&self, r: V3) -> (V3, V3) {
        (self.pos + act(self.q, r), act(self.q, self.vel + self.omega.cross(r)))
    }
}
