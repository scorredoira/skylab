//! Pilots: whatever turns an `Observation` into `Controls`. The human at the keyboard (with or
//! without the assist) is one; an AI model would be another.

use std::f64::consts::PI;

use crate::aircraft::AircraftControls;
use crate::math::{clamp, remainder, smoothstep, DEG, GRAVITY, KNOTS};
use crate::vehicle::Controls;
use crate::world::Observation;

/// Something that drives a vehicle: flies an aircraft, today.
pub trait Pilot {
    /// Called once per simulation step, before it. `current` is what the vehicle's controls are
    /// now, of its kind; the result replaces them, of the same kind. A pilot that only works some of
    /// the controls returns the others as they are in `current`; one that cannot drive this kind of
    /// vehicle returns `current`.
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls;
}

/// Moves the yoke and pedals from on/off keys, in one of two ways.
///
/// **Direct** (`assist` off): a key moves its control while it is held, slowly at first and faster the
/// longer it is held, and the control stays where it is left. Nothing is held or corrected.
///
/// **Assisted** (`assist` on): the arrows move the attitude the pilot wants instead. Holding one tilts
/// the nose or the wings at a steady rate and letting go keeps them there; the yoke is then moved, as far
/// as needed and no further, to follow that attitude, and the feet keep the aircraft straight.
/// It holds up to `ASSIST_PITCH` of nose and `ASSIST_BANK` of bank. A key held past them, or the
/// aircraft beyond them, makes the arrows ask for a rate of pitch or roll instead, up to the full
/// travel of the yoke, so loops, rolls, dives and spins can be flown with it too (see `ASSIST_LOAD`).
/// The aircraft's own physics is the same in both.
///
/// The key state (`roll`, `pitch`, `pedals`, `fine`, `mouse_yoke`, `mouse_rudder`) is set from
/// outside before each step. It only moves aileron, elevator and rudder: throttle, flaps, trim
/// and brakes are left as they are.
#[derive(Clone, Debug)]
pub struct KeyboardPilot {
    pub assist: bool,

    /// -1, 0 or 1 from the side keys (+1 = right).
    pub roll: f64,
    /// -1, 0 or 1 (+1 = pull).
    pub pitch: f64,
    /// -1, 0 or 1 (+1 = right pedal).
    pub pedals: f64,
    /// -1, 0 or 1: with the assist on, moves the nose slowly (+1 = up), for small corrections.
    pub fine: f64,
    /// Direct yoke position (x, y) while the mouse holds it.
    pub mouse_yoke: Option<(f64, f64)>,
    /// Direct rudder position while the mouse holds it.
    pub mouse_rudder: Option<f64>,

    /// How fast the nose and the wings move while a key is held with the assist on, radians per second.
    pub pitch_rate: f64,
    pub roll_rate: f64,

    yoke_x: f64,
    yoke_y: f64,
    pedal: f64,

    held_x: f64,
    held_y: f64,
    held_pedal: f64,
    feet: f64,
    ground_heading: Option<f64>,
    /// North and east of the point the runway line was taken at.
    ground_point: (f64, f64),
    pitch_hold: f64,
    pitch_spring: f64,
    attitude: Option<f64>,
    bank_target: f64,
    roll_was_held: bool,
    pitch_was_held: bool,
    was_on_ground: bool,
    /// Beyond the attitudes the assist holds: the arrows ask for a rate of pitch or roll.
    pitch_free: bool,
    roll_free: bool,
    /// How long the pitch and roll keys have been held, seconds.
    pitch_held_for: f64,
    roll_held_for: f64,
    /// How long the pitch key has been held against the limits of `ASSIST_LOAD` and the stall.
    insisted: f64,
    /// The steady part of the yoke while a pitch rate is asked for.
    free_hold: f64,
}

/// The nose attitude and the bank the assist holds by itself. Holding a key past them, or flying
/// beyond them, the arrows ask for a rate of pitch or roll instead: loops, rolls, dives and
/// spins are flown with the assist on too.
pub const ASSIST_PITCH: f64 = 30.0 * DEG;
pub const ASSIST_BANK: f64 = 60.0 * DEG;
/// Load factor the assist pulls to beyond those attitudes, below the limit of 3.8 g, unless the
/// key is held on (see `KeyboardPilot`).
pub const ASSIST_LOAD: f64 = 3.4;
/// Seconds the pitch key must be held against that limit, or the stall, for them to give way.
pub const ASSIST_INSIST: f64 = 3.0;

impl Default for KeyboardPilot {
    fn default() -> KeyboardPilot {
        KeyboardPilot::new()
    }
}

/// A control moved by a key: slowly at first, faster the longer the key is held.
fn moved(value: f64, key: f64, held: &mut f64, dt: f64, slow: f64, fast: f64) -> f64 {
    *held = if key == 0.0 { 0.0 } else { *held + dt };
    let rate = slow + (fast - slow) * (*held / 1.5).min(1.0);
    clamp(value + key * rate * dt, -1.0, 1.0)
}

impl KeyboardPilot {
    pub fn new() -> KeyboardPilot {
        KeyboardPilot {
            assist: true,
            roll: 0.0,
            pitch: 0.0,
            pedals: 0.0,
            fine: 0.0,
            mouse_yoke: None,
            mouse_rudder: None,
            pitch_rate: 4.0 * DEG,
            roll_rate: 18.0 * DEG,
            yoke_x: 0.0,
            yoke_y: 0.0,
            pedal: 0.0,
            held_x: 0.0,
            held_y: 0.0,
            held_pedal: 0.0,
            feet: 0.0,
            ground_heading: None,
            ground_point: (0.0, 0.0),
            pitch_hold: 0.0,
            pitch_spring: 0.0,
            attitude: None,
            bank_target: 0.0,
            roll_was_held: false,
            pitch_was_held: false,
            was_on_ground: true,
            pitch_free: false,
            roll_free: false,
            pitch_held_for: 0.0,
            roll_held_for: 0.0,
            insisted: 0.0,
            free_hold: 0.0,
        }
    }

    /// Where the yoke is sideways, -1...1 (+1 = right).
    pub fn yoke_x(&self) -> f64 {
        self.yoke_x
    }

    /// Where the yoke is fore and aft, -1...1 (+1 = back).
    pub fn yoke_y(&self) -> f64 {
        self.yoke_y
    }

    /// Where the pedals are, -1...1 (+1 = right).
    pub fn pedal(&self) -> f64 {
        self.pedal
    }

    /// Sets the three keys at once.
    pub fn set_keys(&mut self, roll: f64, pitch: f64, pedals: f64) {
        self.roll = roll;
        self.pitch = pitch;
        self.pedals = pedals;
    }

    /// With the assist on, in the air: the nose attitude and the bank it holds from now on, radians
    /// (`None` leaves that one where the keys left it), as if the arrows had brought it there.
    /// Within what the assist holds (`ASSIST_PITCH`, `ASSIST_BANK`). The nose goes there as with
    /// the arrows, never asked for far beyond where it is, so call it before each step for as
    /// long as that attitude is wanted. On the ground it does nothing: the yoke is the keys'.
    pub fn hold_attitude(&mut self, pitch: Option<f64>, bank: Option<f64>) {
        if let (Some(pitch), Some(_)) = (pitch, self.attitude) {
            self.attitude = Some(clamp(pitch, -ASSIST_PITCH, ASSIST_PITCH));
        }
        if let Some(bank) = bank {
            self.bank_target = clamp(bank, -ASSIST_BANK, ASSIST_BANK);
        }
    }

    /// Yoke and pedals back to the middle.
    pub fn centre(&mut self) {
        self.yoke_x = 0.0;
        self.yoke_y = 0.0;
        self.pedal = 0.0;
        self.feet = 0.0;
        self.held_x = 0.0;
        self.held_y = 0.0;
        self.held_pedal = 0.0;
        self.pitch_hold = 0.0;
        self.pitch_spring = 0.0;
        self.attitude = None;
        self.bank_target = 0.0;
        self.pitch_free = false;
        self.roll_free = false;
    }

    /// As `centre`, and forgets what the aircraft was doing. For when it is placed somewhere new.
    pub fn reset(&mut self) {
        self.centre();
        self.ground_heading = None;
        self.roll_was_held = false;
        self.pitch_was_held = false;
        self.was_on_ground = true;
    }

    /// Moves yoke and pedals for one step from the key state and returns (aileron, elevator, rudder).
    pub fn update(&mut self, dt: f64, obs: &Observation) -> (f64, f64, f64) {
        let out = self.moved_controls(dt, obs);
        self.was_on_ground = obs.on_ground;
        self.roll_was_held = self.roll != 0.0;
        self.pitch_was_held = self.pitch != 0.0;
        out
    }

    fn moved_controls(&mut self, dt: f64, obs: &Observation) -> (f64, f64, f64) {
        let (roll, pitch, pedals, fine) = (self.roll, self.pitch, self.pedals, self.fine);
        let on_ground = obs.on_ground;
        let steer = clamp(pedals + if on_ground { roll } else { 0.0 }, -1.0, 1.0);

        if !self.assist {
            if let Some((x, y)) = self.mouse_yoke {
                self.yoke_x = x;
                self.yoke_y = y;
            } else {
                // On the ground the side arrows steer, so they work the pedals instead of the ailerons.
                self.yoke_x = moved(
                    self.yoke_x,
                    if on_ground { 0.0 } else { roll },
                    &mut self.held_x,
                    dt,
                    0.15,
                    0.9,
                );
                self.yoke_y = moved(self.yoke_y, pitch, &mut self.held_y, dt, 0.06, 0.45);
            }
            self.pedal = match self.mouse_rudder {
                Some(r) => r,
                None => moved(self.pedal, steer, &mut self.held_pedal, dt, 0.15, 0.8),
            };
            self.attitude = None;
            self.ground_heading = None;
            self.feet = self.pedal;
            self.pitch_hold = self.yoke_y;
            self.pitch_spring = 0.0;
            return (self.yoke_x, self.yoke_y, self.pedal);
        }

        let omega = obs.angular_rates;
        let ias = (aircraft(obs).indicated_airspeed / KNOTS).max(1.0);
        let nose = obs.pitch;
        let bank = obs.roll;
        // Control power grows with the square of airspeed, so deflections shrink by the same factor.
        let power = clamp((80.0 / ias) * (80.0 / ias), 0.5, 2.5);

        if let Some((x, y)) = self.mouse_yoke {
            self.yoke_x = x;
            self.yoke_y = y;
            self.pitch_hold = y;
            self.pitch_spring = 0.0;
            self.attitude = Some(nose);
            self.bank_target = bank;
            self.pitch_free = false;
            self.roll_free = false;
        } else if on_ground {
            // On the ground the yoke is pushed directly and springs back.
            self.yoke_x -= clamp(self.yoke_x, -2.4 * dt, 2.4 * dt);
            let target = pitch * clamp(power * 0.32, 0.12, 0.6);
            let rate = if pitch != 0.0 {
                if target * self.pitch_spring < 0.0 {
                    2.4
                } else {
                    0.8
                }
            } else {
                1.6
            };
            self.pitch_spring += clamp(target - self.pitch_spring, -rate * dt, rate * dt);
            self.pitch_hold -= clamp(self.pitch_hold, -0.5 * dt, 0.5 * dt);
            self.yoke_y = clamp(self.pitch_hold + self.pitch_spring, -1.0, 1.0);
            self.attitude = None;
            self.bank_target = 0.0;
            self.pitch_free = false;
            self.roll_free = false;
        } else {
            self.pitch_held_for = if pitch != 0.0 { self.pitch_held_for + dt } else { 0.0 };
            self.roll_held_for = if roll != 0.0 { self.roll_held_for + dt } else { 0.0 };
            let attitude = match self.attitude {
                Some(a) if !self.was_on_ground => a,
                _ => {
                    // Just airborne, or just switched on: keep what the yoke was doing and hold what there is.
                    self.pitch_hold = clamp(self.pitch_hold + self.pitch_spring, -1.0, 1.0);
                    self.pitch_spring = 0.0;
                    self.bank_target = if self.was_on_ground || bank.abs() < 5.0 * DEG {
                        0.0
                    } else {
                        bank
                    };
                    nose
                }
            };
            // Beyond the attitudes it holds, or steeply banked, nose up or nose down, the assist
            // stops holding an attitude: the arrows ask for a rate of pitch and roll, and with
            // no key it keeps the aircraft from rotating.
            let upright = bank.abs() < ASSIST_BANK + 10.0 * DEG && nose.abs() < 75.0 * DEG;

            // Pitch: the key moves the wanted attitude; it never runs far ahead of the real nose,
            // so a stalled aircraft is free to drop it.
            // Held for more than two seconds, the nose moves faster and faster, up to four times.
            let faster = 1.0 + 3.0 * ((self.pitch_held_for - 2.0) / 1.5).clamp(0.0, 1.0);
            let mut wanted = attitude + (pitch * self.pitch_rate * faster + fine * 0.8 * DEG) * dt;
            if pitch == 0.0 && self.pitch_was_held {
                wanted = nose + omega[1] * 0.25;
            }
            wanted = clamp(wanted, nose - 6.0 * DEG, nose + 6.0 * DEG);
            if !upright || (pitch != 0.0 && wanted.abs() > ASSIST_PITCH) {
                self.pitch_free = true;
            } else if self.pitch_free && pitch == 0.0 && nose.abs() < ASSIST_PITCH - 5.0 * DEG {
                self.pitch_free = false;
                self.free_hold = 0.0;
                wanted = nose + omega[1] * 0.25;
            }
            if self.pitch_free {
                // The pitch rate asked for grows the longer the key is held. It is kept to what
                // loads the wing to `ASSIST_LOAD` (and to -1 g pushing) and short of the stall;
                // held on against those limits for more than `ASSIST_INSIST` seconds, they give
                // way: the aircraft can then be overstressed or stalled and spun, as with the
                // yoke in the hand.
                let v = aircraft(obs).indicated_airspeed.max(15.0);
                let level = nose.cos() * bank.cos();
                let beyond = (self.insisted - ASSIST_INSIST).max(0.0);
                let (most, least) = (ASSIST_LOAD + beyond * 2.0, -1.0 - beyond);
                let stall = (13.0 - aircraft(obs).flap_deg / 15.0 + beyond * 10.0) * DEG;
                let rate = pitch * (10.0 + 20.0 * (self.pitch_held_for / 1.5).min(1.0)) * DEG
                    + fine * 2.0 * DEG;
                let mut q = clamp(rate, (least - level) * GRAVITY / v, (most - level) * GRAVITY / v);
                q = q.min(omega[1] + (stall - aircraft(obs).alpha) * 3.0);
                q = q.max(omega[1] + (-stall * 0.6 - aircraft(obs).alpha) * 3.0);
                if pitch != 0.0 && (rate - q).abs() > 2.0 * DEG && (rate - q) * pitch > 0.0 {
                    self.insisted += dt;
                } else if pitch == 0.0 {
                    self.insisted = 0.0;
                }
                // What the yoke held before stays as it was, for when the attitude is held again.
                let correction = clamp((q - omega[1]) * 2.5 * power, -2.0, 2.0);
                self.free_hold = clamp(self.free_hold + correction * dt, -2.0, 2.0);
                self.yoke_y = clamp(self.pitch_hold + self.free_hold + correction, -1.0, 1.0);
                self.attitude = Some(nose);
            } else {
                wanted = clamp(wanted, -ASSIST_PITCH, ASSIST_PITCH);
                self.attitude = Some(wanted);
                let limit = 0.28 * power;
                let correction = clamp(
                    ((wanted - nose) * 3.2 - omega[1] * 0.9) * power,
                    -limit,
                    limit,
                );
                self.pitch_hold = clamp(self.pitch_hold + correction * 0.5 * dt, -1.0, 1.0);
                self.yoke_y = clamp(self.pitch_hold + correction, -1.0, 1.0);
            }

            // Roll: the key moves the wanted bank; letting go near level means level.
            let next = self.bank_target + roll * self.roll_rate * dt;
            let mut settled = false;
            if !upright || (roll != 0.0 && next.abs() > ASSIST_BANK) {
                self.roll_free = true;
            } else if self.roll_free
                && roll == 0.0
                && bank.abs() < ASSIST_BANK - 5.0 * DEG
                && nose.abs() < 60.0 * DEG
            {
                // Out of a roll near level means level.
                self.roll_free = false;
                self.bank_target = if bank.abs() < 20.0 * DEG { 0.0 } else { bank };
                settled = true;
            }
            let aileron = if self.roll_free {
                // A roll rate, up to full aileron: about 55°/s at 100 kt.
                let rate = roll * (30.0 + 45.0 * self.roll_held_for.min(1.0)) * DEG;
                let full = 55.0 * DEG * (ias / 100.0).max(0.3);
                self.bank_target = bank;
                clamp(rate / full + (rate - omega[0]) * 1.2, -1.0, 1.0)
            } else {
                if roll != 0.0 {
                    self.bank_target = clamp(
                        clamp(next, bank - 15.0 * DEG, bank + 15.0 * DEG),
                        -ASSIST_BANK,
                        ASSIST_BANK,
                    );
                } else if self.roll_was_held && !settled {
                    let settle = bank + omega[0] * 0.25;
                    self.bank_target = if settle.abs() < 5.0 * DEG {
                        0.0
                    } else {
                        clamp(settle, -ASSIST_BANK, ASSIST_BANK)
                    };
                }
                clamp(
                    ((self.bank_target - bank) * 2.2 - omega[0] * 0.7) * power.min(1.5),
                    -0.7,
                    0.7,
                )
            };
            self.yoke_x += clamp(aileron - self.yoke_x, -4.0 * dt, 4.0 * dt);
        }

        // Feet: the keys push and spring back; the rest keeps the runway line or the ball centred.
        let feet_rate = if steer == 0.0 { 2.5 } else { 1.2 };
        self.feet += clamp(steer - self.feet, -feet_rate * dt, feet_rate * dt);
        let mut rudder = self.feet;
        if let Some(r) = self.mouse_rudder {
            rudder = r;
            self.ground_heading = None;
        } else if on_ground {
            let line = match self.ground_heading {
                Some(line) if steer == 0.0 => line,
                _ => {
                    self.ground_heading = Some(obs.heading);
                    self.ground_point = (obs.north, obs.east);
                    obs.heading
                }
            };
            let across = -(obs.north - self.ground_point.0) * line.sin()
                + (obs.east - self.ground_point.1) * line.cos();
            let error = remainder(
                line + clamp(-across * 0.06, -0.15, 0.15) - obs.heading,
                2.0 * PI,
            );
            rudder +=
                clamp(error * 6.0 - omega[2] * 2.0, -1.0, 1.0) * smoothstep(1.0, 4.0, obs.speed);
        } else {
            self.ground_heading = None;
            // A foot pushed all the way is the pilot's: the ball is left alone (slips, spins).
            rudder += clamp(aircraft(obs).beta * 4.0 - omega[2] * 0.3, -0.6, 0.6) * (1.0 - self.feet.abs());
        }
        self.pedal = clamp(rudder, -1.0, 1.0);
        (self.yoke_x, self.yoke_y, self.pedal)
    }
}

impl KeyboardPilot {
    /// `Pilot::fly` for an aircraft.
    pub fn fly_aircraft(&mut self, obs: &Observation, current: &AircraftControls, dt: f64) -> AircraftControls {
        let (aileron, elevator, rudder) = self.update(dt, obs);
        AircraftControls {
            aileron,
            elevator,
            rudder,
            ..*current
        }
    }
}

impl Pilot for KeyboardPilot {
    fn fly(&mut self, obs: &Observation, current: &Controls, dt: f64) -> Controls {
        match (current, obs.aircraft()) {
            (Controls::Aircraft(c), Some(_)) => Controls::Aircraft(self.fly_aircraft(obs, c, dt)),
            // Not an aircraft: nothing it can fly.
            _ => *current,
        }
    }
}

/// What the aircraft's pilot sees of it: only ever asked through `fly_aircraft`, which `fly` calls
/// for an aircraft alone.
fn aircraft(obs: &Observation) -> &crate::aircraft::AircraftObservation {
    obs.aircraft().expect("an aircraft's pilot flies an aircraft")
}
