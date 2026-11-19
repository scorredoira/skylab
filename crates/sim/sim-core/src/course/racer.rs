//! A pilot that races a multirotor round a track with the sticks a player has (self-levelling:
//! roll and pitch ask for a bank and a pitch, yaw a rate, throttle the power), seeing only the
//! `Observation`, the track and its own count of the gates (`Race`).
//!
//! How it flies: it aims at a point on the line through the next gate's opening, some metres
//! ahead of where it is along that line (so it comes to the gate square to it), at a speed it can
//! still turn at the gate (by the angle the course turns there, and the room it has to slow down),
//! a little faster than the gate... From the velocity it wants it works out the acceleration, from
//! that the tilt and the thrust (the motors' model gives the throttle for a thrust off the
//! battery's voltage), and it turns the nose to where it goes. An integral of the climb's error
//! makes up for what the model does not know (the air through the discs at speed, the wash).
//! A flag is flown round a few metres off its pole; a dive gate from above, dropping through it.
//! Flying fast it gives way to the other vehicles it sees (`Observation::traffic`): from one that
//! would pass within `GIVE_WAY` metres of it in the next second it moves aside, as a pilot does from
//! a rival closing on it; slow, round a gate, it keeps its line (aside there is the gate's frame).

use std::sync::Arc;

use super::{Opening, Race, Track};
use crate::math::{clamp, remainder, GRAVITY, V3};
use crate::multirotor::{MultirotorControls, MultirotorType};
use crate::pilot::Pilot;
use crate::vehicle::Controls;
use crate::world::Observation;

/// Closer than this (m) to another vehicle within the next `LOOK_AHEAD` seconds, it moves away from
/// it: a 5-inch quad's propellers reach 0.18 m from its middle, and it holds its line within half
/// a metre or so in gusts.
pub const GIVE_WAY: f64 = 1.5;
const LOOK_AHEAD: f64 = 1.0;
/// The speed it moves aside at when they would meet, m/s (less the further they would pass).
const AWAY: f64 = 4.0;
/// Slower than this (m/s) it does not give way.
const GIVING_WAY: f64 = 8.0;

/// What it flies at: the speeds and accelerations it asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    /// The fastest it flies, m/s.
    pub top: f64,
    /// Through a gate where the course goes straight on, and where it turns back, m/s.
    pub straight: f64,
    pub hairpin: f64,
    /// How hard it slows down for a gate, m/s².
    pub braking: f64,
    /// The tilt it keeps within, radians.
    pub tilt: f64,
    /// The least height it keeps over the ground, m.
    pub floor: f64,
    /// Whether it follows the ground between the gates (a forest's trail, under the canopy): at
    /// the height over the ground of the last gate's opening, going over to the next one's, its
    /// climb set by how fast the ground rises under it. Otherwise it flies the straight lines
    /// between the openings (an airfield's).
    pub follow_ground: bool,
    /// The fastest it turns its nose, rad/s.
    pub yaw_rate: f64,
}

impl Default for Pace {
    fn default() -> Pace {
        Pace { top: 29.0, straight: 24.0, hairpin: 9.0, braking: 7.0, tilt: 54.0 * crate::math::DEG, floor: 2.5, follow_ground: false, yaw_rate: 5.0 }
    }
}

/// A racer: see the module.
#[derive(Clone, Debug)]
pub struct Racer {
    track: Arc<Track>,
    p: MultirotorType,
    pub pace: Pace,
    /// Its own count of the race (gates flown through, the lap).
    pub race: Race,
    /// Added to the thrust it works out, a share of it: what the model misses.
    lift: f64,
    /// Whether it may go (the start signal has been given).
    going: bool,
    /// Over a dive gate, dropping through it.
    diving: std::cell::Cell<bool>,
    /// Following the ground: when it last looked and its height over the ground then, and how fast
    /// the ground rises under it (m/s, smoothed).
    last_ground: Option<(f64, f64)>,
    ground_rate: f64,
}

impl Racer {
    pub fn new(track: Arc<Track>, p: &MultirotorType) -> Racer {
        let laps = track.course.laps;
        let pace = track.course.pace;
        Racer { track, p: p.clone(), pace, race: Race::new(laps), lift: 0.0, going: false, diving: std::cell::Cell::new(false), last_ground: None, ground_rate: 0.0 }
    }

    /// The start signal: it goes from where it is now (`time`, `pos`).
    pub fn go(&mut self, time: f64, pos: V3) {
        self.race.go(time, pos);
        self.going = true;
        self.lift = 0.0;
    }

    /// Takes over a race already going, where it is: it flies on from there.
    pub fn take_over(&mut self, race: &Race) {
        self.race = race.clone();
        self.going = true;
        self.lift = 0.0;
        self.diving.set(false);
        self.last_ground = None;
        self.ground_rate = 0.0;
    }

    /// The velocity it wants now, for the next gate, from `pos`.
    fn aim(&self, pos: V3) -> V3 {
        let track = &self.track;
        let n = track.gates.len();
        let i = self.race.next;
        let (middle, through) = track.target(i);
        // Along the line through the opening: how far past it, and how far off it.
        let along = (pos - middle).dot(through);
        let off = (pos - middle) - through * along;
        let flat = matches!(track.shape(i).opening, Opening::Flat { .. });

        // The speed it can still turn at the gate: by the angle between the way through it and
        // the way on to the next one, and the room to the next.
        let (next_middle, _) = track.target((i + 1) % n);
        let on = (next_middle - middle).normalize_or_zero();
        let level = |v: V3| V3::new(v.x, v.y, 0.0).normalize_or_zero();
        let turn = level(through).dot(level(on)).clamp(-1.0, 1.0).acos();
        let pace = &self.pace;
        let mut at_gate = pace.straight + (pace.hairpin - pace.straight) * (turn / std::f64::consts::PI).powf(0.7);
        let room = (next_middle - middle).length();
        if room < 60.0 {
            at_gate = at_gate.min(2.0 + 0.2 * room);
        }
        let distance = (middle - pos).length();
        let speed = (at_gate * at_gate + 2.0 * pace.braking * (distance - 3.0).max(0.0)).sqrt().min(pace.top);

        if flat {
            // A dive: to a point over the gate, lower as it nears it, then down through it; from
            // under it or level with it, out to the side and up first.
            let h = off.length();
            let away = if h > 0.1 { off / h } else { -level(through) };
            let point = if self.diving.get() {
                if along > 0.5 && h > 2.0 {
                    self.diving.set(false);
                }
                middle + through * 6.0
            } else if along > -0.5 && h < 8.0 {
                middle + away * 10.0 - through * 6.0
            } else if h > 1.5 {
                middle - through * (h * 0.6).clamp(2.5, 14.0)
            } else {
                self.diving.set(true);
                middle + through * 6.0
            };
            return (point - pos).normalize_or_zero() * speed.min(4.0 + 0.35 * h);
        }
        if along > -2.0 {
            // At the gate's plane or past it, and not through: round behind it and again.
            let back = middle - through * 15.0 + off.normalize_or_zero() * 4.0;
            return (back - pos).normalize_or_zero() * 8.0;
        }
        // Along the line through it, coming onto it so as to be on it some metres before it.
        let to_go = (-along - 2.0).max(1.0);
        // Coming in from the side: slower, the more so the closer.
        let speed = if to_go < 50.0 { speed.min(8.0 + 3.0 * to_go / off.length().max(0.5)) } else { speed };
        let lateral = -off * (1.6 * speed / to_go).clamp(0.6, 1.2);
        let most = 0.85 * speed;
        let lateral = if lateral.length() > most { lateral * (most / lateral.length()) } else { lateral };
        let ahead = (speed * speed - lateral.length_squared()).max(0.0).sqrt();
        through * ahead + lateral
    }

    fn fly_sticks(&mut self, obs: &Observation, c: &MultirotorControls, dt: f64) -> MultirotorControls {
        let mut out = MultirotorControls { arm: true, ..*c };
        let pos = V3::new(obs.north, obs.east, -obs.altitude);
        let v = V3::from_array(obs.velocity);
        if !self.going || obs.crashed.is_some() {
            out.throttle = 0.0;
            out.roll = 0.0;
            out.pitch = 0.0;
            out.yaw = 0.0;
            return out;
        }
        let before = self.race.next;
        self.race.update(&self.track, pos, obs.time);
        if self.race.next != before {
            self.diving.set(false);
        }
        if self.race.finished.is_some() {
            // Over the line: slow down and hover.
            return self.steer(obs, v, V3::ZERO, out, dt);
        }
        let mut want = self.aim(pos) + give_way(obs, v);
        if self.pace.follow_ground {
            want.z = -self.climb(obs, pos, v);
        }
        self.steer(obs, v, want, out, dt)
    }

    /// Following the ground (`Pace::follow_ground`): the climb it wants (m/s, up). The height
    /// over the ground it holds goes from the last gate's opening's (over its foot) to the next
    /// one's as it goes from one to the other; the ground's rise under it, worked out from how its
    /// height over it changes against its own climb, is added, so that it climbs with a slope
    /// before it is low.
    fn climb(&mut self, obs: &Observation, pos: V3, v: V3) -> f64 {
        let track = &self.track;
        let n = track.gates.len();
        let height = |i: usize| track.gates[i].foot.z - track.target(i).0.z;
        let next = self.race.next;
        let (from, from_height) = if self.race.passed == 0 { (track.start, height(next)) } else { let i = (next + n - 1) % n; (track.target(i).0, height(i)) };
        let (to, to_height) = (track.target(next).0, height(next));
        let flat = |a: V3| (a.x - pos.x).hypot(a.y - pos.y);
        let share = flat(from) / (flat(from) + flat(to)).max(1.0);
        let wanted = from_height + (to_height - from_height) * share;
        let up = -v.z;
        if let Some((time, agl)) = self.last_ground {
            let dt = obs.time - time;
            if dt > 1e-6 {
                let rise = up - (obs.agl - agl) / dt;
                self.ground_rate += (rise - self.ground_rate) * (dt / 0.25).min(1.0);
            }
        }
        self.last_ground = Some((obs.time, obs.agl));
        clamp(self.ground_rate + 2.0 * (wanted - obs.agl), -12.0, 15.0)
    }

    /// Flies at the velocity it wants.
    fn steer(&mut self, obs: &Observation, v: V3, mut want: V3, mut out: MultirotorControls, dt: f64) -> MultirotorControls {
        // Never into the ground: keep `floor` over it, and climb back over it.
        let floor = self.pace.floor;
        if obs.agl < floor {
            want.z = want.z.min(-(floor - obs.agl) * 1.5);
        }
        // The acceleration that brings the velocity to it.
        let gain = 2.5;
        let mut accel = (want - v) * gain;
        accel.z = clamp(accel.z, -14.0, 9.0);
        // Slowing down, no harder than its braking (a racer keeps its camera on the way ahead).
        let level_v = V3::new(v.x, v.y, 0.0);
        let back = -V3::new(accel.x, accel.y, 0.0).dot(level_v.normalize_or_zero());
        if back > self.pace.braking {
            let along = level_v.normalize_or_zero();
            accel += along * (back - self.pace.braking);
        }
        // The force the thrust must give (per unit mass), world axes, z down.
        let mut force = V3::new(accel.x, accel.y, accel.z - GRAVITY);
        // Within the tilt it keeps, the level part shortened.
        let up = (-force.z).max(2.0);
        let level = force.x.hypot(force.y);
        let most = up * self.pace.tilt.tan();
        if level > most {
            force.x *= most / level;
            force.y *= most / level;
        }
        force.z = -up;
        let heading = obs.heading;
        let (ch, sh) = (heading.cos(), heading.sin());
        let forward = force.x * ch + force.y * sh;
        let right = -force.x * sh + force.y * ch;
        let pitch = (-forward).atan2(up);
        let roll = right.atan2(forward.hypot(up));
        let max = self.p.controller.angle_max;
        out.pitch = clamp(pitch / max, -1.0, 1.0);
        out.roll = clamp(roll / max, -1.0, 1.0);

        // The thrust: what the force asks, and what the model misses, learnt from the climb.
        let climb_error = want.z - v.z;
        self.lift = clamp(self.lift - 0.4 * climb_error * dt, -0.3, 1.0);
        let thrust = self.p.mass * force.length() * (1.0 + self.lift - 0.12 * climb_error.clamp(-5.0, 5.0));
        // Each rotor's speed for it: the air coming through the discs as it flies along the thrust
        // takes some of it (the model's `1 + v_z / pitch speed`).
        let p = &self.p;
        let rho = crate::atmosphere::density(obs.altitude) / crate::atmosphere::SEA_LEVEL_DENSITY;
        let along = v.dot(force.normalize_or_zero());
        let per_rotor = thrust / p.motors.len() as f64;
        let mut w = (per_rotor / (p.thrust_coefficient * rho)).sqrt();
        for _ in 0..4 {
            let pitch_speed = 1.1 * p.prop_pitch * w / std::f64::consts::TAU + 0.5;
            let f_in = clamp(1.0 - along / pitch_speed, 0.15, 1.25);
            w = (per_rotor / (p.thrust_coefficient * rho * f_in)).sqrt();
        }
        let volts = obs.multirotor().map_or(16.0, |m| m.battery_voltage);
        let duty = p.duty_for(p.thrust_coefficient * w * w, volts);
        out.throttle = clamp((duty - p.idle) / (1.0 - p.idle), 0.0, 1.0);

        // The nose where it goes (where it wants to go when slow).
        let ahead = want + v * 0.5;
        out.yaw = if ahead.x.hypot(ahead.y) > 2.5 {
            let error = remainder(ahead.y.atan2(ahead.x) - heading, 2.0 * std::f64::consts::PI);
            self.stick_for_rate(clamp(3.0 * error, -self.pace.yaw_rate, self.pace.yaw_rate))
        } else {
            0.0
        };
        out
    }

    /// The yaw stick that asks for `rate` (rad/s), by the type's rates.
    fn stick_for_rate(&self, rate: f64) -> f64 {
        let rates = &self.p.controller.rates;
        let (mut lo, mut hi) = (-1.0, 1.0);
        for _ in 0..30 {
            let mid = 0.5 * (lo + hi);
            if rates.rate(2, mid) < rate {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }
}

impl Pilot for Racer {
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls {
        match current {
            Controls::Multirotor(c) => Controls::Multirotor(self.fly_sticks(obs, c, dt)),
            // It flies multirotors only.
            other => *other,
        }
    }
}

/// The velocity to add to keep clear of the other vehicles (`GIVE_WAY`), flying faster than
/// `GIVING_WAY`: from each that would come within it in the next `LOOK_AHEAD` seconds (as both move
/// now), aside (level and across its own way) from where it would be nearest, the faster the
/// nearer; from one dead ahead or behind on its line, to the right. And from one ahead that it
/// would come within `GIVE_WAY` of it holds back (`hold_back`), as a pilot waits for a rival
/// through a gate: the one behind waits. Slower, round a gate, where aside is the gate's frame,
/// it only holds back.
fn give_way(obs: &Observation, v: V3) -> V3 {
    let ahead = V3::new(v.x, v.y, 0.0);
    if ahead.length() < GIVING_WAY {
        return hold_back(obs, v);
    }
    let ahead = ahead.normalize();
    let right = V3::new(-ahead.y, ahead.x, 0.0);
    let mut aside = V3::ZERO;
    for t in &obs.traffic {
        let rel = V3::from_array(t.relative);
        if rel.length() > 20.0 {
            continue;
        }
        let closing = V3::from_array(t.velocity) - v;
        let when = if closing.length_squared() > 1e-6 { (-rel.dot(closing) / closing.length_squared()).clamp(0.0, LOOK_AHEAD) } else { 0.0 };
        // From it to here, when they are nearest.
        let apart = -(rel + closing * when);
        let gap = apart.length();
        if gap >= GIVE_WAY {
            continue;
        }
        let across = apart.dot(right);
        let side = if across.abs() > 0.05 { across.signum() } else { 1.0 };
        aside += right * side * AWAY * (1.0 - gap / GIVE_WAY);
    }
    // Aside may not be enough by a gate (its pull is stronger): one behind holds back too.
    aside + hold_back(obs, v)
}

/// Slow (`give_way`): the velocity to take off its own to stay `GIVE_WAY` behind another vehicle
/// ahead that it would come within that of in the next `LOOK_AHEAD` seconds, the more the nearer.
fn hold_back(obs: &Observation, v: V3) -> V3 {
    if v.length() < 1.0 {
        return V3::ZERO;
    }
    let way = v.normalize();
    let mut back = 0.0f64;
    for t in &obs.traffic {
        let rel = V3::from_array(t.relative);
        if rel.length() > 20.0 || rel.dot(way) <= 0.0 {
            continue;
        }
        let closing = V3::from_array(t.velocity) - v;
        let when = if closing.length_squared() > 1e-6 { (-rel.dot(closing) / closing.length_squared()).clamp(0.0, LOOK_AHEAD) } else { 0.0 };
        let gap = (rel + closing * when).length();
        if gap < GIVE_WAY {
            back = back.max(AWAY * (1.0 - gap / GIVE_WAY));
        }
    }
    -way * back.min(v.length())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Traffic;

    fn seeing(relative: [f64; 3], velocity: [f64; 3]) -> Observation {
        Observation { traffic: vec![Traffic { id: 1, relative, velocity }], ..Observation::default() }
    }

    /// Slow round a gate, catching up with a quad 1.2 m ahead on its line, it holds back (the
    /// velocity it asks for is less along its way, not aside); with that quad behind it, or well
    /// clear, it keeps its line.
    #[test]
    fn the_one_behind_holds_back() {
        let v = V3::new(7.0, 0.0, 0.0);
        let back = give_way(&seeing([1.2, 0.1, 0.0], [6.0, 0.0, 0.0]), v);
        assert!(back.x < -1.0 && back.y.abs() < 1e-9, "{back:?}");
        assert_eq!(give_way(&seeing([-1.2, 0.1, 0.0], [8.0, 0.0, 0.0]), v), V3::ZERO, "it is the one ahead");
        assert_eq!(give_way(&seeing([1.2, 4.0, 0.0], [6.0, 0.0, 0.0]), v), V3::ZERO, "well clear aside");
        // Fast, it also moves aside from one it would pass close to.
        let fast = give_way(&seeing([3.0, 0.3, 0.0], [12.0, 0.0, 0.0]), V3::new(20.0, 0.0, 0.0));
        assert!(fast.x < 0.0 && fast.y.abs() > 0.5, "{fast:?}");
    }
}
