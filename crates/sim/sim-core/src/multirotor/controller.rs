//! A multirotor's flight controller: what turns the radio's sticks into the motors' duties, the
//! way the FPV quads' firmware does it, written from the published descriptions (no firmware's
//! code): arming, self-levelling, a rate loop per axis, and a mixer that keeps control at either
//! end of the throttle ("air mode"). It has no flight modes: it always levels itself.
//!
//! - **Arming.** The arm switch arms it only with the throttle down, the frame within
//!   `ARMING_ANGLE` of level and nothing crashed; otherwise it says why (`blocked`). The switch
//!   off, or a crash, disarms it.
//! - **Self-levelling.** Roll and pitch sticks ask for a bank and a pitch, up to `angle_max`, and
//!   it levels when they are let go; the rates that bring it there come from the attitude's error,
//!   `angle_gain` per radian. The yaw stick asks for a rate of turn (`StickRates::rate`). The
//!   throttle is plain power: more climbs, less sinks.
//! - **Rate loop.** Per axis, an angular acceleration from the rate's error (P), its integral (I,
//!   held at nothing while it rests on the ground with the throttle down, and not gathered while
//!   the stick moves fast, "I-term relax"), the rate asked for as it changes (feed-forward) and the
//!   gyro's change (D); times the inertia, the moments to make.
//! - **Mixer.** The collective thrust (the throttle stick's, through the motors'
//!   model off the battery's voltage, so that its middle stays where it hovers as the battery runs
//!   down... as far as the motors can) and the moments are split between the motors
//!   (`MultirotorType::mixer`). What the motors cannot give is taken from the yaw first, then roll
//!   and pitch shrink together, and the whole is moved up or down to fit between idle and full:
//!   control is kept at zero throttle and at full. Each motor's thrust becomes a duty by the
//!   same model.
//!
//! A tilt-rotor (`Tilt`: two nacelles either side, wings and a tail) is flown by the same arming,
//! levelling and rate loop, and by another split of the moments (`tilted`), since two nacelles
//! side by side cannot pitch it with their thrust, and what each of their means does turns with
//! the nacelles:
//!
//! - **Roll and yaw.** One nacelle pushing harder than the other rolls it with the rotors up and
//!   yaws it with them ahead; one tilted further than the other yaws it with the rotors up and
//!   rolls it with them ahead. At an angle δ between, the two means give the roll and yaw moments
//!   through a rotation by δ, which is undone exactly: the difference of thrust is
//!   `−(M_roll cos δ + M_yaw sin δ) / y` and the difference of tilt gives
//!   `(M_roll sin δ − M_yaw cos δ) / y`, `y` the nacelles' distance from the middle.
//! - **Pitch.** The rotors' cyclic pitch gives a moment at each hub, as much as its thrust lets
//!   it; the elevator does it with speed. The moment is split between the two by what each can
//!   give now (least squares, each weighted by its travel).
//! - **Turns.** Above a speed it turns with its bank as an aeroplane does: the rate of turn a
//!   balanced turn at that bank and speed has is added to what the yaw stick asks.
//!
//! How much it helps is the pilot's to say (`Stability`, a switch of the controls). `Level` is
//! the self-levelling above. With `Rates` the roll and pitch sticks ask for rates of turn about
//! its own axes, as the yaw stick does (`StickRates`), and let go it stays as it is: the rate
//! loop holds no turn, and its integral the attitude. With `Off` there is no loop: the sticks ask
//! for angular accelerations (`Tilt::direct` at full stick), and nothing damps it but the air.
//!
//! A tilt-rotor whose stick tilts its nacelles (`Tilt::stick_tilt`) is flown level: the pitch
//! stick moves both nacelles forward or back of where the lever has them, at once, and the frame
//! holds the pitch the pilot trims (`MultirotorControls::trim`), with either help. With none the
//! stick pitches the frame, as the others'.
//!
//! `Hold` (a turbine tilt-rotor's, `HoldFit`) holds it still. The collective lever asks for a rate
//! of climb, none round its middle: the thrust is what carries its weight and gives the
//! acceleration that takes the climb it has to the one asked for (and the integral of that, for
//! what the thrust's sum does not know), over how much of the thrust points up; at the lever's
//! foot the blades are flat, to stand on the ground. With the nacelles' lever upright the stick
//! asks for a speed over the ground: forward, by the nacelles' tilt (the acceleration that takes
//! the speed it has to the one asked for is `g tan` of it), and sideways by its bank, each with
//! its integral, which is what holds the wind off. With the lever forward the stick is `Level`'s.
//!
//! With turbines (`Turbine`) what it orders of each rotor is its blades' pitch, not a duty: the
//! throttle stick is the collective lever, from the least pitch to the most, and a difference of
//! thrust is a difference of pitch at the governed speed. The arm switch is the engines': it
//! starts them as it arms a quad (the collective down, near level, not crashed) and stops them,
//! and with them stopped it goes on flying the rotors while they turn.

use super::{Battery, MultirotorControls, MultirotorData, MultirotorType, Stability, Tilt, ARMING_ANGLE, MOST_MOTORS, NACELLES};
use crate::body::Body;
use crate::math::{act, clamp, euler_angles, quaternion, smoothstep, GRAVITY, V3};

/// The foot of the collective lever, a share of its travel: under it, holding itself, the blades
/// are flat (it stands on the ground, and the engines start).
const LEVER_FOOT: f64 = 0.06;

/// How far the rate asked for can run ahead of itself followed slowly before the integral stops
/// gathering, rad/s (40°/s).
const ITERM_RELAX: f64 = 0.7;

/// What the flight controller orders for a step: each motor's duty (0 disarmed; idle to 1
/// armed; with turbines each rotor's blades' pitch), and of a tilt-rotor where each nacelle should
/// be (radians forward of upright; left, right), its elevator (radians, trailing edge down) and
/// its rotors' cyclic (−1 to 1, nose up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Orders {
    pub duties: [f64; MOST_MOTORS],
    pub nacelles: [f64; NACELLES],
    pub elevator: f64,
    pub cyclic: f64,
}

/// The flight controller's state between steps.
#[derive(Clone, Debug, Default)]
pub struct Controller {
    pub(crate) armed: bool,
    pub(crate) blocked: Option<&'static str>,
    integral: V3,
    /// The rate asked for, followed slowly: how far it is from it says the stick is moving.
    slow_setpoint: V3,
    last_setpoint: V3,
    feed: V3,
    last_omega: V3,
    /// Seconds armed since it was last armed, and the height it was armed at: what its display
    /// shows as the flight's time and the altitude.
    pub(crate) armed_time: f64,
    pub(crate) armed_altitude: f64,
    /// Where the flight controller has a tilt-rotor's nacelles on their way to where the lever
    /// asks, radians forward of upright.
    lever: f64,
    /// `Hold`'s integrals: of the forward and sideways speeds' errors (accelerations, m/s²) and
    /// of the climb's.
    held: V3,
}

impl Controller {
    /// Starts from the body as it is: no jump in its rates to chase.
    pub(crate) fn settle(&mut self, body: &Body) {
        self.last_omega = body.omega;
        self.last_setpoint = body.omega;
        self.slow_setpoint = body.omega;
        self.feed = V3::ZERO;
        self.integral = V3::ZERO;
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
        self.integral = V3::ZERO;
    }

    /// Armed at once, flying from the body as it is (a start in the air).
    pub(crate) fn arm_now(&mut self, body: &Body, ground: f64) {
        self.lever = 0.0;
        self.armed = true;
        self.blocked = None;
        self.armed_time = 0.0;
        self.armed_altitude = ground;
        self.settle(body);
    }

    /// What it orders for these sticks, this step. `nacelles` is where a tilt-rotor's nacelles
    /// are now; disarmed they stay there.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn control(&mut self, p: &MultirotorType, c: &MultirotorControls, body: &Body, battery: &Battery, data: &MultirotorData, nacelles: [f64; NACELLES], crashed: bool, dt: f64) -> Orders {
        let n = p.motors.len();
        let level = act(body.q, V3::Z).z;
        if !c.arm {
            self.armed = false;
            self.blocked = None;
        } else if !self.armed {
            let low = c.throttle <= 0.05;
            self.blocked = if crashed {
                Some("CRASH")
            } else if !low {
                Some("THROTTLE")
            } else if level < ARMING_ANGLE.cos() {
                Some("ANGLE")
            } else {
                None
            };
            if self.blocked.is_none() {
                self.armed = true;
                self.armed_time = 0.0;
                self.armed_altitude = -body.pos.z;
                self.settle(body);
            }
        }
        if crashed {
            self.armed = false;
        }
        // With turbines the switch is the engines': stopped, the rotors still turn and it still
        // flies them (down, on the air through them). Crashed, nothing.
        if crashed || (!self.armed && p.turbine.is_none()) {
            self.integral = V3::ZERO;
            return Orders { duties: [0.0; MOST_MOTORS], nacelles, elevator: 0.0, cyclic: 0.0 };
        }
        if self.armed {
            self.armed_time += dt;
        }

        let fit = &p.controller;
        let volts = battery.voltage.max(1.0);
        // A rotor's thrust at the bottom and the top of what it can be ordered, and all of them
        // at the throttle stick: off the battery, or by the blades' pitch at the governed speed.
        let stick = clamp(c.throttle, 0.0, 1.0);
        // Holding itself: what `Hold` holds, for a type that has it and its turbines.
        let hold = p.tilt.as_ref().and_then(|t| t.hold).filter(|_| c.stability == Stability::Hold && p.turbine.is_some());
        let velocity = body.world_velocity();
        let (sin_h, cos_h) = euler_angles(body.q).heading.sin_cos();
        // Its speed over the ground forward and to the right of where it faces.
        let over_ground = (velocity.x * cos_h + velocity.y * sin_h, velocity.y * cos_h - velocity.x * sin_h);
        // With turbines: the blades' pitch at the collective lever, the thrust that gives a rotor
        // in the hover, and the thrust a unit of pitch more gives.
        let blades = p.turbine.map(|t| {
            let mut pitch = t.pitch.0 + stick * (t.pitch.1 - t.pitch.0);
            if let Some(h) = hold.filter(|_| stick > LEVER_FOOT) {
                // The climb asked for, and the thrust that goes after it, as far as it points up.
                let off = (stick - 0.5) / 0.5;
                let asked = if off.abs() < h.dead_band { 0.0 } else { (off - h.dead_band * off.signum()) / (1.0 - h.dead_band) * if off > 0.0 { h.climb.0 } else { h.climb.1 } };
                let error = asked + velocity.z;
                self.held.z = clamp(self.held.z + 0.12 * h.climb_gain * error * dt, -0.4 * GRAVITY, 0.4 * GRAVITY);
                let up = act(body.q, V3::new(self.lever.sin(), 0.0, -self.lever.cos())).z;
                let thrust = p.mass * (GRAVITY + h.climb_gain * error + self.held.z) / clamp(-up, 0.5, 1.0);
                pitch = clamp(p.static_pitch(&t, thrust / n as f64, crate::atmosphere::SEA_LEVEL_DENSITY), t.pitch.0, t.pitch.1);
            } else {
                self.held.z = 0.0;
            }
            (t, pitch, p.static_thrust(&t, pitch, crate::atmosphere::SEA_LEVEL_DENSITY), p.thrust_coefficient * t.rotor_speed * t.rotor_speed)
        });
        let (least, most, collective) = match blades {
            Some((t, pitch, thrust, full)) => (thrust + (t.pitch.0 - pitch) * full, thrust + (t.pitch.1 - pitch) * full, n as f64 * thrust),
            None => (p.steady_thrust(p.idle, volts), p.steady_thrust(1.0, volts), n as f64 * p.steady_thrust(p.idle + (1.0 - p.idle) * stick, volts)),
        };
        let omega = body.omega;
        let heading = euler_angles(body.q).heading;
        let max = fit.angle_max;
        // Holding a speed over the ground (the nacelles' lever upright): the stick's ends are the
        // speeds, and what goes after them is the nacelles' tilt and the bank.
        let hovering = hold.filter(|_| c.tilt <= 0.0 && !(data.on_ground && stick <= LEVER_FOOT));
        let mut held_tilt = None;
        let mut held_bank = None;
        if let (Some(h), Some(tilt)) = (hovering, &p.tilt) {
            let asked = (-clamp(c.pitch, -1.0, 1.0) * h.speed.0, clamp(c.roll, -1.0, 1.0) * h.speed.1);
            let error = (asked.0 - over_ground.0, asked.1 - over_ground.1);
            let most = 0.25 * GRAVITY;
            self.held.x = clamp(self.held.x + 0.25 * h.speed_gain * error.0 * dt, -most, most);
            self.held.y = clamp(self.held.y + 0.25 * h.speed_gain * error.1 * dt, -most, most);
            held_tilt = Some(clamp(((h.speed_gain * error.0 + self.held.x) / GRAVITY).atan(), -tilt.back, tilt.stick_tilt));
            held_bank = Some(clamp(((h.speed_gain * error.1 + self.held.y) / GRAVITY).atan(), -h.bank, h.bank));
        } else {
            (self.held.x, self.held.y) = (0.0, 0.0);
        }
        // The stick tilts the nacelles: the pitch asked for is the trim's.
        let by_nacelles = p.tilt.as_ref().is_some_and(|t| t.stick_tilt > 0.0) && c.stability != Stability::Off;
        let pitch_asked = if by_nacelles { c.trim } else { c.pitch };
        let bank_asked = match c.stability {
            Stability::Hold if held_bank.is_some() => held_bank.unwrap_or(0.0),
            Stability::Level | Stability::Hold => clamp(c.roll, -1.0, 1.0) * max,
            // The bank as it is: only the pitch is brought to the trim's.
            _ => euler_angles(body.q).roll,
        };
        let target = quaternion(heading, clamp(pitch_asked, -1.0, 1.0) * max, bank_asked);
        // The turn from where it is to where it should be, body axes.
        let mut turn = body.q.conjugate() * target;
        if turn.w < 0.0 {
            turn = -turn;
        }
        // The yaw stick turns it about the vertical, not about its own axis (as the firmwares' angle
        // mode does, "earth-referenced yaw"): banked, the heading turns flat and the bank is kept,
        // the turn split between the body's yaw and pitch rates by where the vertical lies in it.
        let vertical = act(body.q.conjugate(), V3::NEG_Z);
        let level_rates = V3::new(2.0 * fit.angle_gain * turn.x, 2.0 * fit.angle_gain * turn.y, 0.0);
        let mut turn_rate = fit.rates.rate(2, c.yaw);
        if let Some(tilt) = &p.tilt {
            // A balanced turn at this bank and speed: `g tan φ / V`.
            let bank = euler_angles(body.q).roll;
            let flying = smoothstep(tilt.turn_from, tilt.turn_full, data.airspeed);
            turn_rate += flying * GRAVITY * clamp(bank, -1.2, 1.2).tan() / data.airspeed.max(tilt.turn_from);
        }
        let setpoint = match c.stability {
            Stability::Level | Stability::Hold => level_rates - vertical * turn_rate,
            // The sticks' rates about its own axes, and the balanced turn's about the vertical.
            _ => V3::new(fit.rates.rate(0, c.roll), if by_nacelles { level_rates.y } else { fit.rates.rate(1, c.pitch) }, fit.rates.rate(2, c.yaw)) - vertical * (turn_rate - fit.rates.rate(2, c.yaw)),
        };

        // The rate loop.
        let error = setpoint - omega;
        let change = (setpoint - self.last_setpoint) / dt;
        self.last_setpoint = setpoint;
        self.feed += (change - self.feed) * (dt / 0.02).min(1.0);
        let turning = (omega - self.last_omega) / dt;
        self.last_omega = omega;
        // The integral is held while the stick moves (relaxed by how far the rate asked for runs
        // ahead of itself followed over a tenth of a second), so that it does not wind up in the
        // turn's start and bounce back at its end; and at rest on the ground.
        self.slow_setpoint += (setpoint - self.slow_setpoint) * (dt / 0.1).min(1.0);
        let relax = (setpoint - self.slow_setpoint).abs().map(|x| (1.0 - x / ITERM_RELAX).max(0.0));
        let resting = data.on_ground && collective < if p.turbine.is_some() { 0.5 * p.mass * GRAVITY } else { 1.3 * n as f64 * least };
        self.integral = if resting { V3::ZERO } else { (self.integral + fit.i * error * relax * dt).clamp(V3::splat(-fit.i_limit), V3::splat(fit.i_limit)) };
        let accel = fit.p * error + self.integral + fit.feed_forward * self.feed - fit.d * turning;
        let mut moment = p.inertia * accel;
        if let (Stability::Off, Some(tilt)) = (c.stability, &p.tilt) {
            // No loop: the sticks ask for accelerations.
            self.integral = V3::ZERO;
            moment = p.inertia * tilt.direct * V3::new(clamp(c.roll, -1.0, 1.0), clamp(c.pitch, -1.0, 1.0), clamp(c.yaw, -1.0, 1.0));
        }
        if let Some(tilt) = &p.tilt {
            // The conversion: the nacelles follow the lever no faster than it lets them.
            let asked = clamp(c.tilt, 0.0, 1.0) * tilt.forward;
            self.lever += clamp(asked - self.lever, -tilt.conversion_rate * dt, tilt.conversion_rate * dt);
            // A rotor's thrust as what is ordered of it: its blades' pitch, or its motor's duty.
            let order = |thrust: f64| match blades {
                Some((_, pitch, at_lever, full)) => pitch + (thrust - at_lever) / full,
                None => clamp(p.duty_for(thrust, volts), p.idle, 1.0),
            };
            // The stick's share of the nacelles' tilt, at once: forward stick (nose down), forward.
            let lever = match held_tilt {
                Some(held) => held,
                None if by_nacelles => clamp(self.lever - clamp(c.pitch, -1.0, 1.0) * tilt.stick_tilt, -tilt.back, tilt.forward),
                None => self.lever,
            };
            return tilted(p, tilt, lever, collective, moment, nacelles, data.airspeed, (least, most), &order);
        }

        // The mixer, keeping control at both ends of the throttle.
        let mut roll_pitch = [0.0; MOST_MOTORS];
        let mut yaw = [0.0; MOST_MOTORS];
        let mut base = [0.0; MOST_MOTORS];
        for i in 0..n {
            let m = p.mixer[i];
            base[i] = m[0] * collective;
            roll_pitch[i] = m[1] * moment.x + m[2] * moment.y;
            yaw[i] = m[3] * moment.z;
        }
        let spread = |k_rp: f64, k_y: f64| {
            let (lo, hi) = (0..n).map(|i| k_rp * roll_pitch[i] + k_y * yaw[i]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
            hi - lo
        };
        let range = most - least;
        let (k_rp, k_y) = if spread(1.0, 0.0) > range {
            (range / spread(1.0, 0.0), 0.0)
        } else if spread(1.0, 1.0) > range {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..12 {
                let mid = (lo + hi) / 2.0;
                if spread(1.0, mid) <= range {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (1.0, lo)
        } else {
            (1.0, 1.0)
        };
        let thrusts: Vec<f64> = (0..n).map(|i| base[i] + k_rp * roll_pitch[i] + k_y * yaw[i]).collect();
        let (lo, hi) = thrusts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        let shift = if least - lo > 0.0 { least - lo } else if hi - most > 0.0 { (most - hi).max(least - lo) } else { 0.0 };
        let mut duties = [0.0; MOST_MOTORS];
        for i in 0..n {
            duties[i] = clamp(p.duty_for(thrusts[i] + shift, volts), p.idle, 1.0);
        }
        Orders { duties, nacelles, elevator: 0.0, cyclic: 0.0 }
    }
}

/// A tilt-rotor's orders for a `collective` thrust (N) and the moments to make (N m, body axes),
/// its nacelles where they are and the `lever` asking for them at an angle, at an `airspeed`: see
/// the module. A motor's thrust is kept between
/// `least` and `most`, and ordered of it as `order` says.
#[allow(clippy::too_many_arguments)]
fn tilted(p: &MultirotorType, tilt: &Tilt, lever: f64, collective: f64, moment: V3, nacelles: [f64; NACELLES], airspeed: f64, (least, most): (f64, f64), order: &dyn Fn(f64) -> f64) -> Orders {
    let n = p.motors.len();
    let (sin, cos) = ((nacelles[0] + nacelles[1]) / 2.0).sin_cos();
    // How far out the nacelles' motors are, and how many each side has.
    let side = |k: usize| p.motors.iter().filter(move |m| m.nacelle == Some(k));
    let out = p.motors.iter().filter(|m| m.nacelle.is_some()).map(|m| m.position.y.abs()).sum::<f64>() / n as f64;
    // A nacelle's thrust for the tilt's sums: never taken as less than a tenth of the weight, so
    // that with the throttle down the nacelles are not thrown about for nothing.
    let each = (collective / 2.0).max(0.1 * p.mass * GRAVITY);

    // Roll and yaw: the difference of thrust (right less left) and of tilt.
    let more = -(moment.x * cos + moment.z * sin) / out;
    let split = clamp((moment.x * sin - moment.z * cos) / out / (2.0 * each), -tilt.split_authority.sin(), tilt.split_authority.sin()).asin();

    // Pitch: the cyclic's (the moment all of it gives: the thrust times how far it moves off the
    // hubs) and the elevator's (a radian of it: the tailplane's lift it adds, times its arm).
    let by_cyclic = 2.0 * each * tilt.cyclic_arm;
    let (elevator_gain, travel) = tilt
        .surfaces
        .iter()
        .find_map(|s| {
            let (effect, travel) = s.elevator?;
            let slope = std::f64::consts::TAU * s.aspect / (s.aspect + 2.0);
            let pressure = 0.5 * crate::atmosphere::SEA_LEVEL_DENSITY * airspeed * airspeed;
            Some((pressure * s.area * slope * effect * s.position.x, travel))
        })
        .unwrap_or((0.0, 0.0));
    let by_elevator = elevator_gain * travel;
    let both = by_cyclic * by_cyclic + by_elevator * by_elevator;
    let (cyclic, elevator) = if both > 1e-9 {
        (clamp(moment.y * by_cyclic / both, -1.0, 1.0), clamp(moment.y * by_elevator / both, -1.0, 1.0) * travel)
    } else {
        (0.0, 0.0)
    };

    let mut duties = [0.0; MOST_MOTORS];
    for (k, sign) in [(0, -1.0), (1, 1.0)] {
        let motors = side(k).count().max(1) as f64;
        let thrust = clamp((collective / 2.0 + sign * more / 2.0) / motors, least, most);
        for (i, m) in p.motors.iter().enumerate() {
            if m.nacelle == Some(k) {
                duties[i] = order(thrust);
            }
        }
    }
    Orders { duties, nacelles: [lever - split, lever + split], elevator, cyclic }
}
