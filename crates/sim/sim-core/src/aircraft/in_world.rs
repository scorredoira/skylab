//! An aircraft as one of the world's vehicles (`vehicle::Dynamics::Aircraft`): what its instruments
//! sense and show, how the world places it, the points of it that run into things and what running
//! into them does to it, and what its pilot observes of it. The world calls these; the flight model
//! itself (`Aircraft::step`) knows nothing of them.

use super::{Aircraft, AircraftControls, Damage, Environment, FlightData, State};
use std::f64::consts::{PI, TAU};
use crate::atmosphere::{self, Atmosphere};
use crate::instruments::{AirData, Earth, Readings, Sense};
use crate::math::{act, clamp, inverse, quaternion, DEG, GRAVITY, V3};
use crate::scenery::ObstacleKind;
use crate::vehicle::{ContactPoint, On, Trunk};
use crate::wreck::PieceKind;

/// The air data an ideal sensor gives, with a lag: what the pilots and the trained models fly by
/// (the instruments, `Aircraft::instruments`, are what the dials show). And the last touchdown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Gauges {
    indicated_airspeed: f64,
    indicated_altitude: f64,
    vertical_speed: f64,
    turn_rate: f64,
    slip_ball: f64,
    last_touchdown: Option<f64>,
    was_on_ground: bool,
}

/// What a pilot observes of an aircraft, besides what it observes of any vehicle
/// (`world::Observation`): plain numbers, SI units, angles in radians.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AircraftObservation {
    // Instruments (with their lag).
    pub indicated_airspeed: f64,
    pub indicated_altitude: f64,
    /// Positive up.
    pub vertical_speed: f64,
    pub turn_rate: f64,
    pub slip_ball: f64,
    /// What every dial and display shows, from the pressures, gyros and compass that drive them
    /// (the five above are the air data an ideal sensor gives, with a lag).
    pub instruments: Readings,
    pub alpha: f64,
    pub beta: f64,

    // Engine and configuration.
    pub rpm: f64,
    pub throttle: f64,
    /// Actual flap deflection, degrees.
    pub flap_deg: f64,
    pub trim: f64,
    pub stall_warning: bool,
}

/// Spacing of the points along the leading edges that run into trees and buildings, m: less than the
/// thinnest trunk is across, so none slips between two (and less than an aircraft moves in a step at 120 kt).
const EDGE_STEP: f64 = 0.25;
/// What crushes the nose and the cabin against a post, N: the engine and its mount, the cabin's
/// frame. Estimated: four times what cuts a wing.
const FUSELAGE_CRUSH: f64 = 80_000.0;
/// How far a leg is pushed back as it is torn off, m.
const LEG_TRAVEL: f64 = 0.3;

impl Aircraft {
    /// Indicated airspeed, m/s, as an ideal sensor gives it with a lag.
    pub fn indicated_airspeed(&self) -> f64 {
        self.gauges.indicated_airspeed
    }

    /// m.
    pub fn indicated_altitude(&self) -> f64 {
        self.gauges.indicated_altitude
    }

    /// m/s, positive up.
    pub fn vertical_speed(&self) -> f64 {
        self.gauges.vertical_speed
    }

    /// What the turn coordinator shows, rad/s.
    pub fn turn_rate(&self) -> f64 {
        self.gauges.turn_rate
    }

    /// Angle of the slip ball, radians.
    pub fn slip_ball(&self) -> f64 {
        self.gauges.slip_ball
    }

    /// Sink rate at the last touchdown, m/s.
    pub fn last_touchdown(&self) -> Option<f64> {
        self.gauges.last_touchdown
    }

    /// The gauges from where the aircraft is now, as at a placement; on the ground or not.
    fn clear_gauges(&mut self, on_ground: bool) {
        self.gauges = Gauges { indicated_altitude: -self.state.pos.z, was_on_ground: on_ground, ..Gauges::default() };
    }

    /// At rest on the ground at (`n`, `e`), on its wheels on ground `elevation` high, facing `heading`,
    /// engine idling, nothing broken.
    pub(crate) fn stand(&mut self, n: f64, e: f64, elevation: f64, heading: f64) {
        self.state = State {
            pos: V3::new(n, e, -(elevation + self.p.cg_height)),
            // A tailwheel aircraft sits nose up on its three wheels.
            q: quaternion(heading, self.p.ground_pitch, 0.0),
            prop_speed: 70.0,
            n1: self.idling(),
            ..State::default()
        };
        self.clear_crash();
        self.refuel();
        self.flap_deg = 0.0;
        self.configure(true);
        self.downwash = 0.0;
        self.controls = AircraftControls::default();
        self.clear_gauges(true);
    }

    /// In steady flight at `pos` towards `heading`, at true airspeed `tas`, `flap_deg` of flap on
    /// `flap_notch`, along a path `climb` (radians, negative down); `fallback` (pitch, propeller
    /// speed, throttle) if it cannot be trimmed there. The airspeed indicator reads it already.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fly_steady(&mut self, pos: V3, heading: f64, tas: f64, flap_notch: i32, climb: f64, air: &Atmosphere, fallback: (f64, f64, f64)) {
        let altitude = -pos.z;
        let flaps = self.p.flap_notches[flap_notch as usize];
        let mut s = State::default();
        let mut c = AircraftControls::default();
        s.pos = pos;
        c.flap_notch = flap_notch;
        if let Some(t) = self.trim(tas, altitude, flaps, None, climb) {
            s.vel = V3::new(tas * t.alpha.cos(), 0.0, tas * t.alpha.sin());
            s.q = quaternion(heading, t.alpha + climb, 0.0);
            s.prop_speed = t.prop_speed;
            s.blade_pitch = t.blade_pitch;
            s.n1 = t.n1;
            c.throttle = t.throttle;
            // The yoke starts centred; trim carries the elevator.
            c.trim = if self.p.stabiliser_trim.is_some() { t.trim } else { self.trim_for_elevator(t.elevator) };
        } else {
            s.vel = V3::new(tas, 0.0, 0.0);
            s.q = quaternion(heading, fallback.0, 0.0);
            s.prop_speed = fallback.1;
            s.n1 = self.idling();
            c.throttle = fallback.2;
        }
        // A retractable gear is up in the air, unless it comes with the flaps of the approach.
        let gear_down = self.p.retract.is_none() || flap_notch >= self.p.final_flap_notch.max(1);
        c.gear_up = !gear_down;
        self.state = s;
        self.controls = c;
        self.clear_crash();
        self.refuel();
        self.flap_deg = flaps;
        self.configure(gear_down);
        self.downwash = 0.0;
        self.clear_gauges(false);
        self.gauges.indicated_airspeed = tas * (air.density(altitude) / atmosphere::SEA_LEVEL_DENSITY).sqrt();
    }

    /// N1 of each jet engine idling on the ground.
    fn idling(&self) -> [f64; super::MOST_ENGINES] {
        let mut n1 = [0.0; super::MOST_ENGINES];
        if let Some(jets) = &self.p.jets {
            n1[..jets.mounts.len()].fill(jets.idle);
        }
        n1
    }

    /// At a placement: the gear down or up, the spoilers down, the reversers closed.
    fn configure(&mut self, gear_down: bool) {
        self.gear = if gear_down { 1.0 } else { 0.0 };
        self.spoilers = 0.0;
        self.reversers = [0.0; super::MOST_ENGINES];
    }

    /// A new flight starts with the tanks as the type is loaded for it: a crash spills the fuel
    /// (all of it when the airframe breaks up, a wing's tank with a torn wing) and a long flight
    /// burns it, and without this a restart would leave the engine with nothing to run on.
    fn refuel(&mut self) {
        let mut load = self.load();
        load.fuel = self.p.reference_load.fuel;
        self.set_load(load);
    }

    /// Put in a state set by hand, its gauges reading it already.
    pub(crate) fn put(&mut self, state: State, air: &Atmosphere) {
        self.state = state;
        self.clear_gauges(false);
        let altitude = -state.pos.z;
        self.gauges.indicated_airspeed = state.vel.length() * (air.density(altitude) / atmosphere::SEA_LEVEL_DENSITY).sqrt();
    }

    /// Trim setting that does the work of a yoke deflection.
    fn trim_for_elevator(&self, elevator: f64) -> f64 {
        let p = &self.p;
        clamp(elevator * (if elevator >= 0.0 { p.elevator_up } else { p.elevator_down }) / p.trim_range, -1.0, 1.0)
    }

    /// What the instruments sense of the aircraft now, with an accelerometer reading `specific`,
    /// in air moving at `air_velocity`.
    fn sense(&self, specific: V3, air_velocity: V3) -> Sense {
        let s = &self.state;
        Sense {
            altitude: -s.pos.z,
            attitude: s.q,
            rates: s.omega,
            airflow: s.vel - act(inverse(s.q), air_velocity),
            specific,
            rpm: self.rpm(),
            flap_deg: self.flap_deg,
        }
    }

    /// The instruments as after taxiing out, the altimeter set to `qnh`.
    pub(crate) fn settle_instruments(&mut self, env: &Environment, air: &Atmosphere, earth: &Earth, qnh: f64) {
        let steady = act(inverse(self.state.q), V3::new(0.0, 0.0, -GRAVITY));
        let sense = self.sense(steady, env.air_velocity);
        let fit = self.p.instruments;
        self.instruments.settle(&sense, &fit, air, earth, qnh);
    }

    /// The gauges and the instruments after a step of the flight model, and the touchdown.
    pub(crate) fn step_instruments(&mut self, dt: f64, env: &Environment, air: &Atmosphere, earth: &Earth) {
        let s = self.state;
        let d = *self.data();
        let world_vel = act(s.q, s.vel);
        let g = &mut self.gauges;
        g.indicated_airspeed += (d.ias - g.indicated_airspeed) * (dt / 0.25).min(1.0);
        g.indicated_altitude += (-s.pos.z - g.indicated_altitude) * (dt / 0.15).min(1.0);
        g.vertical_speed += (-world_vel.z - g.vertical_speed) * (dt / 1.5).min(1.0);
        // The turn coordinator's gyro is canted 30°, so it feels roll as well as yaw.
        let rate = s.omega.z * (30.0 * DEG).cos() + s.omega.x * (30.0 * DEG).sin();
        g.turn_rate += (rate - g.turn_rate) * (dt / 0.3).min(1.0);
        let ball = d.specific.y.atan2((-d.specific.z).max(1.0));
        g.slip_ball += (ball - g.slip_ball) * (dt / 0.25).min(1.0);
        let sense = self.sense(d.specific, env.air_velocity);
        let fit = self.p.instruments;
        self.instruments.step(dt, &sense, &fit, air, earth);

        let g = &mut self.gauges;
        if d.on_ground() && !g.was_on_ground {
            g.last_touchdown = Some(if g.vertical_speed < 0.0 { world_vel.z } else { 0.0 });
        }
        g.was_on_ground = d.on_ground();
    }

    /// What its pilot observes of it.
    pub fn observe(&self) -> AircraftObservation {
        let (g, d) = (&self.gauges, self.data());
        AircraftObservation {
            indicated_airspeed: g.indicated_airspeed,
            indicated_altitude: g.indicated_altitude,
            vertical_speed: g.vertical_speed,
            turn_rate: g.turn_rate,
            slip_ball: g.slip_ball,
            instruments: *self.instruments.readings(),
            alpha: d.alpha,
            beta: d.beta,
            rpm: self.rpm(),
            throttle: self.controls.throttle,
            flap_deg: self.flap_deg,
            trim: self.controls.trim,
            stall_warning: d.stall_warning,
        }
    }

    /// Points of the airframe: every point the fuselage and tail have (`Pieces::points`), the
    /// wheels, and the leading edges of the wings and of the tailplane every `EDGE_STEP`, out to where
    /// each ends now.
    pub(crate) fn contact_points(&self) -> Vec<ContactPoint> {
        let s = self.state.body();
        let damage = self.damage();
        let pieces = self.p.pieces;
        let mut list = Vec::with_capacity(96);
        let mut add = |body: V3, on: On, length: f64| {
            let (at, vel) = s.point(body - self.cg());
            list.push(ContactPoint { at, vel, body, on, length });
        };
        for &p in pieces.points(PieceKind::Fuselage) {
            add(p, On::Body, 0.5);
        }
        for &p in pieces.points(PieceKind::Tail).iter().filter(|p| p.y.abs() < pieces.stabiliser.root) {
            add(p, On::Body, 0.5);
        }
        for (i, wheel) in self.p.gear.iter().enumerate() {
            if !damage.gear_broken[i] && self.gear > 0.5 {
                add(wheel.contact, On::Wheel(i), 0.3);
            }
        }
        for side in 0..2 {
            let sign = [-1.0, 1.0][side];
            for (span, torn, wing) in [(&pieces.wing, damage.wing_torn[side], true), (&pieces.stabiliser, damage.stabiliser_torn[side], false)] {
                if torn.is_some_and(|end| end <= span.root) {
                    continue;
                }
                let end = torn.map_or(span.tip, |end| end.min(span.tip));
                let steps = ((end - span.root) / EDGE_STEP).ceil().max(1.0) as usize;
                for k in 0..=steps {
                    let station = span.root + (end - span.root) * k as f64 / steps as f64;
                    let on = if wing { On::Wing { side, station } } else { On::Tailplane { side, station } };
                    add(span.leading_edge(sign, station), on, (end - span.root) / steps as f64);
                }
                add(span.trailing_edge(sign, end), if wing { On::Wing { side, station: end } } else { On::Tailplane { side, station: end } }, 0.0);
            }
        }
        list
    }

    /// A point of the airframe has run into an obstacle: a trunk (what fells it, `trunk`) or a
    /// building. The weaker gives way: a trunk that breaks before the airframe does is felled and
    /// takes its energy out of the aircraft, which goes on; otherwise a wing or half of the tailplane
    /// is cut through where it struck, if the point brings enough energy to cut it, and the aircraft
    /// goes on, slower and turned; what has not enough is stopped against it. The fuselage against
    /// what does not give way: stopped, or broken up if faster than `COLLISION_BREAKUP`. Whether it
    /// felled the tree.
    pub(crate) fn strike(&mut self, point: &ContactPoint, kind: ObstacleKind, trunk: Option<Trunk>) -> bool {
        let (what, keep) = match kind {
            ObstacleKind::Tree => ("Crashed into a tree", 0.5),
            ObstacleKind::Building => ("Crashed into a building", 0.15),
            ObstacleKind::Hangar => ("Crashed into a hangar", 0.15),
            ObstacleKind::Aircraft => ("Crashed into a parked aircraft", 0.3),
            ObstacleKind::Gate => ("Crashed into a race gate", 0.3),
            ObstacleKind::Bridge => ("Crashed into a bridge", 0.15),
        };
        let speed = point.vel.length();
        let energy = self.energy_at(point.body);
        let (fells_at, felling) = trunk.map_or((f64::INFINITY, f64::INFINITY), |t| (t.fells_at, t.felling));
        let strength = match point.on {
            On::Wing { .. } => self.p.wing_crush,
            On::Tailplane { .. } => 0.5 * self.p.wing_crush,
            On::Wheel(leg) => self.p.gear[leg].strength,
            On::Body => FUSELAGE_CRUSH,
        };
        if fells_at < strength {
            if energy > felling {
                self.absorb(point.body, felling);
                match point.on {
                    On::Wing { side, station } if station > self.p.strip_edges[4 - self.p.aileron_strips] => {
                        self.loosen_aileron(side, ["Tree snapped: left aileron bent", "Tree snapped: right aileron bent"][side]);
                    }
                    // The nose: the propeller hit it.
                    On::Body if point.body.x > 1.0 => self.strike_propeller(),
                    _ => {}
                }
                return true;
            }
            self.collide(what, speed, keep);
            return false;
        }
        let pieces = self.p.pieces;
        match point.on {
            On::Wing { side, station } | On::Tailplane { side, station } => {
                let wing = matches!(point.on, On::Wing { .. });
                let span = if wing { &pieces.wing } else { &pieces.stabiliser };
                // Cut through over its chord; just inboard of what it struck, and not a sliver.
                let cutting = strength * span.chord(station);
                let width = trunk.map_or(0.1, |t| t.radius);
                let torn = if wing { self.damage().wing_torn[side] } else { self.damage().stabiliser_torn[side] };
                let end = torn.map_or(span.tip, |end| end.min(span.tip));
                let at = (station - width).min(end - 0.5);
                if energy > cutting {
                    self.absorb(point.body, cutting);
                    if wing {
                        self.tear_wing(side, at, keep, [["Left wing cut off by a tree", "Right wing cut off by a tree"], ["Left wing torn off by a building", "Right wing torn off by a building"]][usize::from(trunk.is_none())][side]);
                    } else {
                        self.tear_stabiliser(side, at, keep, ["Left stabiliser snapped", "Right stabiliser snapped"][side]);
                    }
                } else {
                    self.collide(what, speed, keep);
                }
            }
            On::Wheel(leg) if trunk.is_some() && energy > self.p.gear[leg].strength * LEG_TRAVEL => {
                self.absorb(point.body, self.p.gear[leg].strength * LEG_TRAVEL);
                self.break_leg(leg, ["Wheel torn off by a tree", "Left gear torn off by a tree", "Right gear torn off by a tree"][leg.min(2)]);
            }
            _ => self.collide(what, speed, keep),
        }
        false
    }
}

/// What a replay keeps of an aircraft many times a second: what moves and what is shown from one
/// step to the next (`Aircraft::look`). The rest, its type and what changes slowly, comes from a
/// whole copy of the aircraft kept now and then, which `Aircraft::show` puts this over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub state: State,
    pub controls: AircraftControls,
    pub flap_deg: f64,
    /// The gear (1 down), the speed brakes and each engine's reverser (0...1 each).
    pub gear: f64,
    pub spoilers: f64,
    pub reversers: [f64; super::MOST_ENGINES],
    pub data: FlightData,
    pub readings: Readings,
    pub damage: Damage,
    pub crashed: Option<&'static str>,
    gauges: Gauges,
}

impl Aircraft {
    /// What is seen of it now, for a replay.
    pub fn look(&self) -> Look {
        Look {
            state: self.state,
            controls: self.controls,
            flap_deg: self.flap_deg,
            gear: self.gear,
            spoilers: self.spoilers,
            reversers: self.reversers,
            data: self.data,
            readings: *self.instruments.readings(),
            damage: self.damage,
            crashed: self.crashed,
            gauges: self.gauges,
        }
    }

    /// For a replay: looks as it did when `look` was taken. Only to be drawn: its flight model's
    /// memory (the stall's, the strips of a wing torn since) is the copy's, not that moment's.
    pub fn show(&mut self, look: &Look) {
        self.state = look.state;
        self.controls = look.controls;
        self.flap_deg = look.flap_deg;
        self.gear = look.gear;
        self.spoilers = look.spoilers;
        self.reversers = look.reversers;
        self.data = look.data;
        self.instruments.show(look.readings);
        self.damage = look.damage;
        self.crashed = look.crashed;
        self.gauges = look.gauges;
    }
}

impl Look {
    /// The look `k` of the way (0...1) from this one to `next`, taken `dt` seconds later: the
    /// position along the curve both velocities draw, the attitude turned the short way, the
    /// controls, the dials and the gauges in between; what breaks or happens once (the damage, a
    /// crash, the flight model's flags) as in the nearer of the two.
    pub fn blend(&self, next: &Look, k: f64, dt: f64) -> Look {
        // At the ends, exactly the looks kept.
        if k <= 0.0 {
            return *self;
        }
        if k >= 1.0 {
            return *next;
        }
        let (a, b) = (&self.state, &next.state);
        let state = State {
            pos: hermite(a.pos, act(a.q, a.vel) * dt, b.pos, act(b.q, b.vel) * dt, k),
            vel: a.vel.lerp(b.vel, k),
            q: a.q.slerp(b.q, k),
            omega: a.omega.lerp(b.omega, k),
            prop_speed: mix(a.prop_speed, b.prop_speed, k),
            blade_pitch: mix(a.blade_pitch, b.blade_pitch, k),
            n1: std::array::from_fn(|i| mix(a.n1[i], b.n1[i], k)),
        };
        let near = if k < 0.5 { self } else { next };
        let (c, d) = (&self.controls, &next.controls);
        let controls = AircraftControls {
            aileron: mix(c.aileron, d.aileron, k),
            elevator: mix(c.elevator, d.elevator, k),
            rudder: mix(c.rudder, d.rudder, k),
            throttle: mix(c.throttle, d.throttle, k),
            trim: mix(c.trim, d.trim, k),
            brake_left: mix(c.brake_left, d.brake_left, k),
            brake_right: mix(c.brake_right, d.brake_right, k),
            spoilers: mix(c.spoilers, d.spoilers, k),
            ..near.controls
        };
        let (r, s) = (&self.readings, &next.readings);
        let readings = Readings {
            airspeed: mix(r.airspeed, s.airspeed, k),
            altitude: mix(r.altitude, s.altitude, k),
            vertical_speed: mix(r.vertical_speed, s.vertical_speed, k),
            attitude: r.attitude.zip(s.attitude).map(|(x, y)| [mix(x[0], y[0], k), turn(x[1], y[1], k)]).or(near.readings.attitude),
            heading_indicator: r.heading_indicator.zip(s.heading_indicator).map(|(x, y)| turn(x, y, k)).or(near.readings.heading_indicator),
            suction: r.suction.zip(s.suction).map(|(x, y)| mix(x, y, k)).or(near.readings.suction),
            compass: turn(r.compass, s.compass, k),
            air_data: AirData {
                airspeed: mix(r.air_data.airspeed, s.air_data.airspeed, k),
                true_airspeed: mix(r.air_data.true_airspeed, s.air_data.true_airspeed, k),
                altitude: mix(r.air_data.altitude, s.air_data.altitude, k),
                vertical_speed: mix(r.air_data.vertical_speed, s.air_data.vertical_speed, k),
                temperature: mix(r.air_data.temperature, s.air_data.temperature, k),
            },
            ahrs: [mix(r.ahrs[0], s.ahrs[0], k), turn(r.ahrs[1], s.ahrs[1], k), turn(r.ahrs[2], s.ahrs[2], k)],
            ..near.readings
        };
        let (g, h) = (&self.gauges, &next.gauges);
        let gauges = Gauges {
            indicated_airspeed: mix(g.indicated_airspeed, h.indicated_airspeed, k),
            indicated_altitude: mix(g.indicated_altitude, h.indicated_altitude, k),
            vertical_speed: mix(g.vertical_speed, h.vertical_speed, k),
            turn_rate: mix(g.turn_rate, h.turn_rate, k),
            slip_ball: mix(g.slip_ball, h.slip_ball, k),
            ..near.gauges
        };
        Look {
            state,
            controls,
            flap_deg: mix(self.flap_deg, next.flap_deg, k),
            gear: mix(self.gear, next.gear, k),
            spoilers: mix(self.spoilers, next.spoilers, k),
            reversers: std::array::from_fn(|i| mix(self.reversers[i], next.reversers[i], k)),
            readings,
            gauges,
            ..*near
        }
    }
}

fn mix(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

/// From angle `a` towards `b` the short way round, `k` of the way: in [0, 2π) if both were, else
/// in [-π, π).
fn turn(a: f64, b: f64, k: f64) -> f64 {
    let x = a + ((b - a + PI).rem_euclid(TAU) - PI) * k;
    if a >= 0.0 && b >= 0.0 { x.rem_euclid(TAU) } else { (x + PI).rem_euclid(TAU) - PI }
}

/// The cubic from `p0` to `p1` leaving with `m0` and arriving with `m1` (velocities times the span).
fn hermite(p0: V3, m0: V3, p1: V3, m1: V3, k: f64) -> V3 {
    let (k2, k3) = (k * k, k * k * k);
    p0 * (2.0 * k3 - 3.0 * k2 + 1.0) + m0 * (k3 - 2.0 * k2 + k) + p1 * (-2.0 * k3 + 3.0 * k2) + m1 * (k3 - k2)
}

#[cfg(test)]
mod look_tests {
    use super::*;
    use crate::aircraft::AircraftKind;

    #[test]
    fn a_look_shown_on_another_copy_gives_it_back() {
        let mut a = Aircraft::of(AircraftKind::default());
        a.state.pos = V3::new(100.0, -40.0, -300.0);
        a.state.vel = V3::new(50.0, 0.0, 2.0);
        a.controls.throttle = 0.7;
        a.flap_deg = 10.0;
        let look = a.look();
        let mut b = Aircraft::of(AircraftKind::default());
        b.show(&look);
        assert_eq!(b.look(), look);
    }

    #[test]
    fn a_blend_runs_from_one_look_to_the_next() {
        let mut a = Aircraft::of(AircraftKind::default());
        a.state.vel = V3::new(50.0, 0.0, 0.0);
        let first = a.look();
        a.state.pos = V3::new(5.0, 0.0, 0.0);
        a.controls.aileron = 1.0;
        let second = a.look();
        assert_eq!(first.blend(&second, 0.0, 0.1), first);
        assert_eq!(first.blend(&second, 1.0, 0.1).state.pos, second.state.pos);
        let half = first.blend(&second, 0.5, 0.1);
        assert!((half.state.pos.x - 2.5).abs() < 1e-9 && (half.controls.aileron - 0.5).abs() < 1e-12);
        // Angles go the short way round.
        assert!(turn(0.1, TAU - 0.1, 0.5).min(TAU - turn(0.1, TAU - 0.1, 0.5)) < 1e-9);
        assert!((turn(PI - 0.1, -PI + 0.1, 0.5).abs() - PI).abs() < 1e-9);
    }
}
