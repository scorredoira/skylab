//! The keyboard pilot, direct and assisted, flying the aircraft through `World`.

mod common;

use common::{assert_close, Sim};
use sim_core::*;

const TAU: f64 = 2.0 * std::f64::consts::PI;

fn pitch(sim: &Sim) -> f64 {
    euler_angles(sim.aircraft().state.q).pitch / DEG
}

fn bank(sim: &Sim) -> f64 {
    euler_angles(sim.aircraft().state.q).roll / DEG
}

/// The keys move the controls and the controls stay where they are left.
mod direct {
    use super::*;

    fn apply(pilot: &mut KeyboardPilot, sim: &mut Sim, roll: f64, pitch: f64, pedals: f64) {
        pilot.assist = false;
        pilot.set_keys(roll, pitch, pedals);
        sim.fly(pilot);
    }

    #[test]
    fn controls_stay_where_the_keys_leave_them() {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        let mut pilot = KeyboardPilot::new();
        for _ in 0..240 {
            apply(&mut pilot, &mut sim, 1.0, 1.0, -1.0);
        }
        let (x, y, p) = (pilot.yoke_x(), pilot.yoke_y(), pilot.pedal());
        assert!(x > 0.1);
        assert!(y > 0.03);
        assert!(p < -0.1);
        for _ in 0..480 {
            apply(&mut pilot, &mut sim, 0.0, 0.0, 0.0);
        }
        assert_eq!(pilot.yoke_x(), x);
        assert_eq!(pilot.yoke_y(), y);
        assert_eq!(pilot.pedal(), p);
        assert_eq!(sim.aircraft().controls.aileron, x);
        pilot.centre();
        apply(&mut pilot, &mut sim, 0.0, 0.0, 0.0);
        assert_eq!(pilot.yoke_x(), 0.0);
        assert_eq!(pilot.yoke_y(), 0.0);
        assert_eq!(pilot.pedal(), 0.0);
    }

    #[test]
    fn a_tap_is_a_small_correction_and_a_hold_is_a_large_one() {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        let mut pilot = KeyboardPilot::new();
        // 0.15 s
        for _ in 0..36 {
            apply(&mut pilot, &mut sim, 0.0, 1.0, 0.0);
        }
        let tap = pilot.yoke_y();
        pilot.centre();
        for _ in 0..240 * 3 {
            apply(&mut pilot, &mut sim, 0.0, 1.0, 0.0);
        }
        println!(
            "KEYS a tap moves the yoke {tap:.3}, three seconds {:.2}",
            pilot.yoke_y()
        );
        assert!(tap < 0.02);
        assert!(pilot.yoke_y() > 0.7);
    }

    #[test]
    fn side_arrows_steer_on_the_ground_and_bank_in_the_air() {
        let mut sim = Sim::new(1);
        let h0 = euler_angles(sim.aircraft().state.q).heading;
        let mut pilot = KeyboardPilot::new();
        sim.aircraft_mut().controls.throttle = 0.3;
        for _ in 0..240 * 5 {
            apply(&mut pilot, &mut sim, 0.0, 0.0, 0.0);
        }
        for _ in 0..240 * 3 {
            apply(&mut pilot, &mut sim, -1.0, 0.0, 0.0);
        }
        let turned = remainder(euler_angles(sim.aircraft().state.q).heading - h0, TAU) / DEG;
        assert!(turned < -5.0);
        assert_eq!(pilot.yoke_x(), 0.0);
        assert!(pilot.pedal() < 0.0);

        sim.reset_in_air(0.0, 0.0, 1.0);
        pilot.centre();
        for _ in 0..240 {
            apply(&mut pilot, &mut sim, 1.0, 0.0, 0.0);
        }
        assert!(pilot.yoke_x() > 0.1);
        assert_eq!(pilot.pedal(), 0.0);
        assert!(euler_angles(sim.aircraft().state.q).roll > 3.0 * DEG);
    }

    #[test]
    fn takeoff_and_climb_can_be_flown_with_the_keys() {
        // A pilot pressing keys by what he sees: feet for the heading, back at 55 knots, wings level.
        let mut sim = Sim::new(1);
        let h0 = euler_angles(sim.aircraft().state.q).heading;
        let p0 = sim.aircraft().state.pos;
        let mut pilot = KeyboardPilot::new();
        sim.aircraft_mut().controls.throttle = 1.0;
        let (mut worst_across, mut worst_roll) = (0.0_f64, 0.0_f64);
        for _ in 0..240 * 70 {
            let s = sim.aircraft().state;
            let e = euler_angles(s.q);
            let yaw_error = remainder(e.heading - h0, TAU) / DEG + s.omega.z / DEG * 1.5;
            let (mut feet, mut side, mut pull) = (0.0, 0.0, 0.0);
            if sim.aircraft().data().on_ground() {
                if yaw_error.abs() > 0.6 {
                    feet = if yaw_error > 0.0 { -1.0 } else { 1.0 };
                }
            } else {
                let lean = e.roll / DEG + s.omega.x / DEG * 0.8;
                if lean.abs() > 2.0 {
                    side = if lean > 0.0 { -1.0 } else { 1.0 };
                }
            }
            let nose = e.pitch / DEG + s.omega.y / DEG * 1.2;
            if sim.vehicle().aircraft().unwrap().indicated_airspeed() > 55.0 * KNOTS {
                if nose < 7.0 {
                    pull = 1.0;
                } else if nose > 9.0 {
                    pull = -1.0;
                }
            }
            apply(&mut pilot, &mut sim, side, pull, feet);
            let d = s.pos - p0;
            if sim.aircraft().data().on_ground() {
                worst_across = worst_across.max((-d.x * h0.sin() + d.y * h0.cos()).abs());
            }
            worst_roll = worst_roll.max((e.roll / DEG).abs());
        }
        println!(
            "KEYS takeoff by hand: off centreline {worst_across:.1} m, roll within {worst_roll:.1}°, after 70 s agl {:.0} m, ias {:.0} kt {}",
            sim.vehicle().agl(),
            sim.vehicle().aircraft().unwrap().indicated_airspeed() / KNOTS,
            sim.aircraft().crashed().unwrap_or("")
        );
        assert!(sim.aircraft().crashed().is_none());
        assert!(worst_across < 10.0);
        assert!(worst_roll < 20.0);
        assert!(sim.vehicle().agl() > 80.0);
    }
}

/// Flying with the assist: the arrows set the attitude and it is held.
mod assist {
    use super::*;

    fn apply(pilot: &mut KeyboardPilot, sim: &mut Sim, roll: f64, pitch: f64) {
        pilot.set_keys(roll, pitch, 0.0);
        sim.fly(pilot);
    }

    #[test]
    fn nose_and_wings_stay_where_the_keys_put_them() {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        let mut pilot = KeyboardPilot::new();
        for _ in 0..240 * 3 {
            apply(&mut pilot, &mut sim, 0.0, 0.0);
        }
        let start = pitch(&sim);
        for _ in 0..240 * 2 {
            apply(&mut pilot, &mut sim, 0.0, -1.0);
        }
        for _ in 0..360 {
            apply(&mut pilot, &mut sim, 0.0, 0.0);
        }
        let pushed = pitch(&sim);
        let mut drift = 0.0_f64;
        for _ in 0..240 * 15 {
            apply(&mut pilot, &mut sim, 0.0, 0.0);
            drift = drift.max((pitch(&sim) - pushed).abs());
        }
        assert_close(
            pushed - start,
            -8.0,
            2.5,
            "nose pushed down by two seconds of key",
        );
        assert!(drift < 1.0);

        let mut held = 0.0;
        while bank(&sim) < 25.0 && held < 6.0 {
            apply(&mut pilot, &mut sim, 1.0, 0.0);
            held += World::DT;
        }
        let (mut lo, mut hi) = (99.0_f64, -99.0_f64);
        for i in 0..240 * 20 {
            apply(&mut pilot, &mut sim, 0.0, 0.0);
            if i > 480 {
                lo = lo.min(bank(&sim));
                hi = hi.max(bank(&sim));
            }
        }
        println!(
            "ASSIST nose {start:.1} → {pushed:.1} (drift {drift:.2}), bank held {lo:.1}..{hi:.1}"
        );
        assert!(lo > 22.0);
        assert!(hi < 32.0);
    }

    /// An attitude given in numbers, with no key touched (a remote pilot's way): it goes there and stays.
    #[test]
    fn an_attitude_asked_for_in_numbers_is_reached_and_held() {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        let mut pilot = KeyboardPilot::new();
        for (nose, wings) in [(6.0, 20.0), (-3.0, -15.0), (2.0, 0.0)] {
            let (mut lo, mut hi) = ((99.0_f64, 99.0_f64), (-99.0_f64, -99.0_f64));
            for i in 0..240 * 20 {
                pilot.hold_attitude(Some(nose * DEG), Some(wings * DEG));
                apply(&mut pilot, &mut sim, 0.0, 0.0);
                if i > 240 * 8 {
                    lo = (lo.0.min(pitch(&sim)), lo.1.min(bank(&sim)));
                    hi = (hi.0.max(pitch(&sim)), hi.1.max(bank(&sim)));
                }
            }
            println!("HOLD nose {nose} wings {wings}: nose {:.1}..{:.1}, bank {:.1}..{:.1}", lo.0, hi.0, lo.1, hi.1);
            assert!(lo.0 > nose - 1.5 && hi.0 < nose + 1.5, "nose {nose}: {:.1}..{:.1}", lo.0, hi.0);
            assert!(lo.1 > wings - 3.0 && hi.1 < wings + 3.0, "bank {wings}: {:.1}..{:.1}", lo.1, hi.1);
        }
    }

    #[test]
    fn takeoff_touching_only_the_down_arrow() {
        let mut sim = Sim::new(1);
        let h0 = euler_angles(sim.aircraft().state.q).heading;
        let p0 = sim.aircraft().state.pos;
        let mut pilot = KeyboardPilot::new();
        sim.aircraft_mut().controls.throttle = 1.0;
        let (mut worst_across, mut worst_roll) = (0.0_f64, 0.0_f64);
        for _ in 0..240 * 60 {
            let pull = sim.vehicle().aircraft().unwrap().indicated_airspeed() > 55.0 * KNOTS && pitch(&sim) < 8.0;
            apply(&mut pilot, &mut sim, 0.0, if pull { 1.0 } else { 0.0 });
            let d = sim.aircraft().state.pos - p0;
            if sim.aircraft().data().on_ground() {
                worst_across = worst_across.max((-d.x * h0.sin() + d.y * h0.cos()).abs());
            }
            worst_roll = worst_roll.max(bank(&sim).abs());
        }
        println!(
            "ASSIST takeoff: off centreline {worst_across:.1} m, roll {worst_roll:.1}°, agl {:.0} m, ias {:.0} kt",
            sim.vehicle().agl(),
            sim.vehicle().aircraft().unwrap().indicated_airspeed() / KNOTS
        );
        assert!(sim.aircraft().crashed().is_none());
        assert!(worst_across < 3.0);
        assert!(worst_roll < 5.0);
        assert!(sim.vehicle().agl() > 100.0);
    }

    #[test]
    fn landing() {
        let mut sim = Sim::new(1);
        let airport = sim.vehicle().home.expect("no airport");
        sim.world.reset(
            sim.id,
            Start::Final {
                airport,
                reversed: false,
                distance: Vehicle::FINAL_DISTANCE,
            },
        );
        let mut pilot = KeyboardPilot::new();
        let glide = (3.0 * DEG).tan();
        let (c, s) = (airport.heading.cos(), airport.heading.sin());
        let (mut touchdown, mut kind, mut across, mut stopped): (
            Option<f64>,
            Option<GroundKind>,
            f64,
            Option<f64>,
        ) = (None, None, 0.0, None);
        let mut t = 0.0;
        while t < 240.0 {
            let pos = sim.aircraft().state.pos;
            let along = (pos.x - airport.n) * c + (pos.y - airport.e) * s;
            let to_aim = -(airport.length / 2.0 - 150.0) - along;
            let wheels = sim.vehicle().agl() - sim.aircraft().p.cg_height;
            let mut wanted = -1.5 - clamp((wheels - to_aim * glide) * 0.25, -4.0, 4.0);
            if wheels < 5.0 {
                wanted = 5.0;
                let throttle = sim.aircraft().controls.throttle;
                sim.aircraft_mut().controls.throttle = (throttle - 0.45 * World::DT).max(0.0);
            }
            let mut key = 0.0;
            if !sim.aircraft().data().on_ground() {
                if pitch(&sim) < wanted - 0.6 {
                    key = 1.0;
                } else if pitch(&sim) > wanted + 0.6 {
                    key = -1.0;
                }
            }
            let offset = -(pos.x - airport.n) * s + (pos.y - airport.e) * c;
            let world_vel = act(sim.aircraft().state.q, sim.aircraft().state.vel);
            let closing = -world_vel.x * s + world_vel.y * c;
            let mut side = 0.0;
            if sim.aircraft().data().on_ground() {
                if (offset + closing * 2.0).abs() > 1.0 {
                    side = if offset + closing * 2.0 > 0.0 {
                        -1.0
                    } else {
                        1.0
                    };
                }
            } else {
                let wanted_bank = clamp(-offset * 0.6 - closing * 4.0, -10.0, 10.0);
                if bank(&sim) < wanted_bank - 2.0 {
                    side = 1.0;
                } else if bank(&sim) > wanted_bank + 2.0 {
                    side = -1.0;
                }
            }
            apply(&mut pilot, &mut sim, side, key);
            t += World::DT;
            if touchdown.is_none() && sim.aircraft().data().on_ground() {
                touchdown = Some(sim.vehicle().aircraft().unwrap().last_touchdown().unwrap_or(0.0));
                kind = Some(sim.vehicle().environment().kind);
            }
            if touchdown.is_some() {
                across = across.max(offset.abs());
                if sim.aircraft().state.vel.length() < 0.5 {
                    stopped = Some(along + airport.length / 2.0);
                    break;
                }
            }
            if sim.aircraft().crashed().is_some() {
                break;
            }
        }
        println!(
            "ASSIST landing: sink {:.1} m/s, stopped {:.0} m past the threshold, {across:.1} m off the centreline {}",
            touchdown.unwrap_or(-1.0),
            stopped.unwrap_or(-1.0),
            sim.aircraft().crashed().unwrap_or("")
        );
        assert!(sim.aircraft().crashed().is_none());
        assert_eq!(kind, Some(GroundKind::Runway));
        assert!(touchdown.unwrap_or(9.0) < 2.5);
        assert!(stopped.unwrap_or(9999.0) < airport.length);
        assert!(across < 5.0);
    }

    /// Body pitch and roll rates integrated over a run: how far the aircraft has turned about
    /// its own axes, degrees.
    struct Turned {
        pitch: f64,
        roll: f64,
        most_g: f64,
        least_g: f64,
    }

    fn run(sim: &mut Sim, pilot: &mut KeyboardPilot, seconds: f64, keys: impl Fn(&Sim, f64) -> (f64, f64), stop: impl Fn(&Turned) -> bool) -> Turned {
        let mut t = Turned { pitch: 0.0, roll: 0.0, most_g: 1.0, least_g: 1.0 };
        let mut time = 0.0;
        while time < seconds && !stop(&t) {
            let (roll, pitch) = keys(sim, time);
            apply(pilot, sim, roll, pitch);
            let a = sim.aircraft();
            t.pitch += a.state.omega.y * World::DT / DEG;
            t.roll += a.state.omega.x * World::DT / DEG;
            t.most_g = t.most_g.max(a.data().load_factor());
            t.least_g = t.least_g.min(a.data().load_factor());
            time += World::DT;
            assert!(a.crashed().is_none(), "{:?}", a.crashed());
        }
        t
    }

    /// With the assist on, holding the down arrow at full power makes a loop: the nose goes on
    /// over the top and the aircraft comes out of it, without overstressing it.
    #[test]
    fn a_loop_with_the_assist() {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        sim.aircraft_mut().controls.throttle = 1.0;
        let mut pilot = KeyboardPilot::new();
        // Nose down to 118 kt first, then pull and keep pulling.
        run(&mut sim, &mut pilot, 5.0, |sim, _| (0.0, if pitch(sim) > -10.0 { -1.0 } else { 0.0 }), |_| false);
        let start = sim.world.time();
        while sim.vehicle().aircraft().unwrap().indicated_airspeed() < 118.0 * KNOTS && sim.world.time() - start < 30.0 {
            apply(&mut pilot, &mut sim, 0.0, 0.0);
        }
        let entry = sim.vehicle().aircraft().unwrap().indicated_airspeed() / KNOTS;
        let t = run(&mut sim, &mut pilot, 40.0, |_, _| (0.0, 1.0), |t| t.pitch > 355.0);
        // Let go as the nose comes back to the horizon: the assist holds it there.
        run(&mut sim, &mut pilot, 10.0, |_, _| (0.0, 0.0), |_| false);
        let e = euler_angles(sim.aircraft().state.q);
        println!(
            "ASSIST loop from {entry:.0} kt: {:.0}° of pitch, {:.0}° of roll, {:.1} to {:.1} g; after it pitch {:.1}°, bank {:.1}°, {:.0} kt",
            t.pitch,
            t.roll,
            t.least_g,
            t.most_g,
            e.pitch / DEG,
            e.roll / DEG,
            sim.vehicle().aircraft().unwrap().indicated_airspeed() / KNOTS
        );
        assert!(t.pitch > 355.0, "the nose went round only {:.0}°", t.pitch);
        assert!(t.roll.abs() < 40.0, "it rolled {:.0}° on the way", t.roll);
        assert!(t.most_g < 3.8 && t.most_g > 2.5, "{:.1} g", t.most_g);
        assert!(e.pitch.abs() < 15.0 * DEG && e.roll.abs() < 10.0 * DEG);
    }

    /// The nose can be pushed down past the 30° the assist holds by itself, to a vertical dive.
    #[test]
    fn a_steep_dive_with_the_assist() {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        sim.aircraft_mut().controls.throttle = 0.0;
        let mut pilot = KeyboardPilot::new();
        let mut steepest = 0.0_f64;
        run(&mut sim, &mut pilot, 12.0, |sim, _| (0.0, if pitch(sim) > -75.0 { -1.0 } else { 0.0 }), |_| false);
        for _ in 0..240 * 3 {
            apply(&mut pilot, &mut sim, 0.0, 0.0);
            steepest = steepest.min(pitch(&sim));
        }
        println!("ASSIST dive: {steepest:.0}°, {:.0} kt", sim.vehicle().aircraft().unwrap().indicated_airspeed() / KNOTS);
        assert!(steepest < -70.0);
    }

    /// Holding a side arrow rolls the aircraft right round: the assist does not stop at 60°.
    #[test]
    fn an_aileron_roll_with_the_assist() {
        let mut sim = Sim::new(1);
        sim.reset_in_air(0.0, 0.0, 1.0);
        sim.aircraft_mut().controls.throttle = 1.0;
        let mut pilot = KeyboardPilot::new();
        // A little nose up first, as for a real one.
        run(&mut sim, &mut pilot, 6.0, |sim, _| (0.0, if pitch(sim) < 15.0 { 1.0 } else { 0.0 }), |_| false);
        let t = run(&mut sim, &mut pilot, 15.0, |_, _| (-1.0, 0.0), |t| t.roll < -350.0);
        run(&mut sim, &mut pilot, 8.0, |_, _| (0.0, 0.0), |_| false);
        println!("ASSIST roll: {:.0}° in all, {:.1} to {:.1} g; after it bank {:.1}°", t.roll, t.least_g, t.most_g, bank(&sim));
        assert!(t.roll < -350.0);
        assert!(bank(&sim).abs() < 10.0);
    }
}

/// The pilot only works the yoke and the pedals: everything else on the controls is the game's.
#[test]
fn the_pilot_leaves_throttle_flaps_and_trim_alone() {
    let mut sim = Sim::new(1);
    sim.reset_in_air(0.0, 0.0, 1.0);
    let before = sim.aircraft().controls;
    for assist in [true, false] {
        let mut pilot = KeyboardPilot::new();
        pilot.assist = assist;
        pilot.set_keys(1.0, -1.0, 1.0);
        let obs = sim.world.observe(sim.id);
        let set = AircraftControls {
            throttle: 0.37,
            flap_notch: 2,
            trim: -0.2,
            parking_brake: true,
            ..before
        };
        let after = pilot.fly_aircraft(&obs, &set, World::DT);
        assert_eq!(
            AircraftControls {
                aileron: set.aileron,
                elevator: set.elevator,
                rudder: set.rudder,
                ..after
            },
            set
        );
        assert_eq!(
            (after.aileron, after.elevator, after.rudder),
            (pilot.yoke_x(), pilot.yoke_y(), pilot.pedal())
        );
        assert!(after.rudder > 0.0);
    }
}
