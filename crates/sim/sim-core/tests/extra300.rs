//! The Extra 300L against its published figures (sources in `AircraftType::extra300`), and what a
//! tailwheel does on the ground: three-point attitude, the tail coming up in the take-off run,
//! and the ground loop a tricycle does not have.

mod common;

use common::assert_close;
use sim_core::*;

const DT: f64 = 1.0 / 240.0;
const RPM: f64 = 60.0 / (2.0 * std::f64::consts::PI);
const FPM: f64 = 60.0 / FEET;

/// An Extra loaded to `mass` kg: the pilot in the rear seat, then fuel, then a passenger in front.
fn extra(mass: f64) -> Aircraft {
    let mut a = Aircraft::of(AircraftKind::EXTRA300);
    let mut load = a.load();
    let extra = mass - a.mass();
    let fuel = (load.fuel + extra).clamp(12.0, 123.0);
    let left = extra - (fuel - load.fuel);
    if left > 0.0 {
        load.front += left;
    } else {
        load.rear += left;
    }
    load.fuel = fuel;
    a.set_load(load);
    assert_close(a.mass(), mass, 0.01, "mass");
    a
}

/// Level flight, trimmed, at `kt` true and `altitude` m.
fn flying(a: &mut Aircraft, kt: f64, altitude: f64) -> Environment {
    let t = a.trim_level(kt * KNOTS, altitude).unwrap();
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(kt * KNOTS * t.alpha.cos(), 0.0, kt * KNOTS * t.alpha.sin()),
        q: quaternion(0.0, t.alpha, 0.0),
        prop_speed: t.prop_speed,
        blade_pitch: t.blade_pitch,
        ..State::default()
    };
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    Environment { rho: atmosphere::density(altitude), ..Environment::default() }
}

fn runway() -> Environment {
    Environment { ground_height: 0.0, kind: GroundKind::Runway, ..Environment::default() }
}

/// Standing on its three wheels, settled.
fn on_ground(a: &mut Aircraft) -> Environment {
    let env = runway();
    a.state.pos = V3::new(0.0, 0.0, -a.p.cg_height);
    a.state.q = quaternion(0.0, a.p.ground_pitch, 0.0);
    a.controls.parking_brake = true;
    for _ in 0..240 * 3 {
        a.step(DT, &env);
    }
    a.controls.parking_brake = false;
    env
}

#[test]
fn stall_speeds() {
    for (mass, kt) in [(820.0, 55.0), (950.0, 60.0)] {
        let s = extra(mass).stall(0.0, 0.0);
        println!("stall at {mass} kg: {:.1} kt, CLmax {:.2}", s.speed / KNOTS, s.cl_max);
        assert_close(s.speed / KNOTS, kt, 2.5, "stall speed");
    }
}

/// Best rate of climb at full throttle, ft/min, and the speed it is flown at, kt.
fn best_climb(a: &Aircraft, altitude: f64) -> (f64, f64) {
    let mut best = (0.0, 0.0);
    for kt in (80..=140).step_by(5) {
        let v = kt as f64 * KNOTS;
        if let Some(t) = a.trim(v, altitude, 0.0, Some(1.0), 0.0) {
            let fpm = v * t.climb_angle.sin() * FPM;
            if fpm > best.0 {
                best = (fpm, kt as f64);
            }
        }
    }
    best
}

#[test]
fn climb() {
    let (fpm, kt) = best_climb(&extra(750.0), 0.0);
    println!("best climb at 750 kg, sea level: {fpm:.0} ft/min at {kt} kt");
    assert_close(fpm, 3200.0, 250.0, "climb at 750 kg");
    let (fpm, kt) = best_climb(&extra(870.0), 2000.0 * FEET);
    println!("best climb at 870 kg, 2000 ft: {fpm:.0} ft/min at {kt} kt");
    assert_close(fpm, 2320.0, 250.0, "climb at 870 kg");
}

#[test]
fn cruise_at_65_percent() {
    let a = extra(820.0);
    let altitude = 8000.0 * FEET;
    let rho = atmosphere::density(altitude);
    let power = |kt: f64| {
        let t = a.trim_level(kt * KNOTS, altitude).unwrap();
        a.engine_torque(t.throttle, t.prop_speed, rho) * t.prop_speed / (300.0 * 745.7)
    };
    let (mut lo, mut hi) = (120.0, 200.0);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if power(mid) < 0.65 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    println!("65% power at 8000 ft: {lo:.1} kt true");
    assert_close(lo, 169.0, 6.0, "cruise speed");
}

#[test]
fn the_governor_holds_2700_rpm() {
    let a = extra(820.0);
    let (_, still) = a.steady(0.1, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0);
    println!("static full throttle: {:.0} rpm", still * RPM);
    assert!(still * RPM > 2450.0 && still * RPM < 2710.0);
    for kt in [90.0, 130.0, 180.0] {
        let t = a.trim(kt * KNOTS, 1000.0, 0.0, Some(1.0), 0.0).unwrap();
        println!("full throttle at {kt} kt: {:.0} rpm, blade {:.1}°", t.prop_speed * RPM, t.blade_pitch / DEG);
        assert_close(t.prop_speed * RPM, 2700.0, 1.0, "governed rpm");
    }
    // Flown: the throttle slammed open and shut, the governor keeps the speed.
    let mut a = extra(820.0);
    let env = flying(&mut a, 140.0, 1000.0);
    let (mut lo, mut hi) = (f64::MAX, 0.0_f64);
    for i in 0..240 * 12 {
        a.controls.throttle = if (i / (240 * 3)) % 2 == 0 { 1.0 } else { 0.5 };
        a.step(DT, &env);
        if i > 240 {
            lo = lo.min(a.rpm());
            hi = hi.max(a.rpm());
        }
    }
    println!("throttle 0.5 - 1: {lo:.0} to {hi:.0} rpm");
    assert!(lo > 2550.0 && hi < 2850.0);
}

#[test]
fn rolls_at_400_degrees_a_second() {
    let mut a = extra(820.0);
    let env = flying(&mut a, 160.0, 1000.0);
    a.controls.aileron = 1.0;
    let mut most = 0.0_f64;
    for _ in 0..240 {
        a.step(DT, &env);
        most = most.max(a.state.omega.x);
    }
    println!("full aileron at 160 kt: {:.0}°/s", most / DEG);
    assert_close(most / DEG, 400.0, 40.0, "roll rate");
}

/// Faster than it flies level: trimmed at 160 kt, then given `kt`.
fn diving(a: &mut Aircraft, kt: f64, altitude: f64) -> Environment {
    let env = flying(a, 160.0, altitude);
    a.state.vel *= kt / 160.0;
    // The stick that held 160 kt, eased forward for the speed.
    a.controls.elevator *= (160.0 / kt).powi(2);
    env
}

/// Pulled (or pushed) to `g` at `kt` and held there for three seconds by a pilot watching the
/// accelerometer.
fn pull(kt: f64, g: f64) -> Aircraft {
    let mut a = extra(820.0);
    let env = diving(&mut a, kt, 1500.0);
    a.controls.throttle = 1.0;
    let mut stick = a.controls.elevator;
    for _ in 0..240 * 3 {
        stick = clamp(stick + (g - a.data().aero_load) * 0.15 * DT, -1.0, 1.0);
        a.controls.elevator = stick;
        a.step(DT, &env);
    }
    a
}

#[test]
fn ten_g_and_what_breaks_it() {
    // At 200 kt it pulls 9 g and pushes −9 g, and nothing gives way.
    for g in [9.0, -9.0] {
        let a = pull(200.0, g);
        let dmg = a.damage();
        println!("{g} g at 200 kt: felt {:.1} to {:.1} g, bent {:.2}, {:?}", dmg.least_g, dmg.most_g, dmg.bent, a.crashed());
        assert!(if g > 0.0 { dmg.most_g > 8.5 } else { dmg.least_g < -8.5 });
        assert!(dmg.bent == 0.0 && a.crashed().is_none());
    }
    // Past the limit of 10 g the wings take a set; past the ultimate 15 g, beyond Vne, one comes off.
    let a = pull(215.0, 12.0);
    println!("12 g at 215 kt: up to {:.1} g, bent {:.2}", a.damage().most_g, a.damage().bent);
    assert!(a.damage().bent > 0.0 && !(a.damage().wing_lost(0) || a.damage().wing_lost(1)));
    let mut a = extra(820.0);
    let env = diving(&mut a, 235.0, 300.0);
    a.controls.elevator = 1.0;
    for _ in 0..240 * 2 {
        a.step(DT, &env);
    }
    println!("full back at 235 kt, low: {:.1} g, {:?}", a.damage().most_g, a.crashed());
    assert!((a.damage().wing_lost(0) || a.damage().wing_lost(1)) && a.damage().most_g >= 15.0);
}

#[test]
fn flies_upside_down() {
    // Symmetric sections: inverted it stalls at nearly the same speed, with the stick forward.
    let mut a = extra(820.0);
    let env = flying(&mut a, 130.0, 1500.0);
    let e = euler_angles(a.state.q);
    a.state.q = quaternion(e.heading, -e.pitch, PI_);
    let mut pitch = 0.0;
    for _ in 0..240 * 10 {
        // A pitch hold with the stick forward.
        let e = euler_angles(a.state.q);
        pitch += (e.pitch - 2.0 * DEG) * DT;
        a.controls.elevator = clamp(4.0 * (e.pitch - 2.0 * DEG) + 1.0 * pitch - 0.5 * a.state.omega.y, -1.0, 1.0);
        a.controls.aileron = clamp(-2.0 * (e.roll.abs() - PI_) * e.roll.signum() - 0.2 * a.state.omega.x, -1.0, 1.0);
        a.step(DT, &env);
    }
    let e = euler_angles(a.state.q);
    println!("inverted 10 s: pitch {:.1}° roll {:.1}° elevator {:.2} {:.0} kt", e.pitch / DEG, e.roll / DEG, a.controls.elevator, a.data().ias / KNOTS);
    assert!(e.roll.abs() > 150.0 * DEG && a.controls.elevator < 0.0);
    assert!(a.data().stalled < 0.05);
}

const PI_: f64 = std::f64::consts::PI;

#[test]
fn sits_on_its_tail() {
    let mut a = extra(791.0);
    on_ground(&mut a);
    let d = *a.data();
    let pitch = euler_angles(a.state.q).pitch / DEG;
    let tail = d.wheel_load[0] / (d.wheel_load.iter().sum::<f64>());
    println!("on the ground: pitch {pitch:.1}°, tail carries {:.0}%, wheels {:?}", tail * 100.0, d.wheel_load);
    assert_close(pitch, 11.0, 1.0, "three-point attitude");
    assert!(tail > 0.06 && tail < 0.14);
    assert!(a.crashed().is_none());
}

#[test]
fn take_off_tail_first() {
    let mut a = extra(820.0);
    let env = on_ground(&mut a);
    a.controls.throttle = 1.0;
    let (mut tail_up, mut liftoff) = (None, None);
    let mut run = 0.0;
    for i in 0..240 * 30 {
        let e = euler_angles(a.state.q);
        // Stick forward to lift the tail, then eased back to hold a level attitude, and back at 60 kt.
        let ias = a.data().ias / KNOTS;
        let want = if ias > 55.0 { 12.0 * DEG } else { 1.0 * DEG };
        a.controls.elevator = clamp(8.0 * (want - e.pitch) - 0.8 * a.state.omega.y, -1.0, 1.0);
        // Pedals keep it straight.
        a.controls.rudder = clamp(-4.0 * e.heading.sin() - 1.5 * a.state.omega.z, -1.0, 1.0);
        a.step(DT, &env);
        if tail_up.is_none() && a.data().wheel_load[0] < 1.0 && a.data().on_ground() {
            tail_up = Some((i as f64 * DT, ias));
        }
        if liftoff.is_none() && !a.data().on_ground() && -a.state.pos.z > a.p.cg_height + 0.5 {
            liftoff = Some((i as f64 * DT, ias));
            run = a.state.pos.x;
            break;
        }
    }
    println!("tail up {tail_up:?}, lift-off {liftoff:?} after {run:.0} m, {:?}", a.crashed());
    let (t_up, kt_up) = tail_up.expect("the tail comes up");
    let (t_off, kt_off) = liftoff.expect("it flies");
    assert!(t_up < t_off && kt_up < 45.0);
    assert!(kt_off > 55.0 && kt_off < 75.0);
    assert!(run < 350.0);
    assert!(a.crashed().is_none());
}

/// Rolling straight at `kt` with the pedals and stick left alone, nudged into a yaw: how far
/// the heading has swung after `seconds`.
fn swing(mut a: Aircraft, kt: f64, seconds: f64) -> f64 {
    let env = on_ground(&mut a);
    let q = a.state.q;
    a.state.vel = act(inverse(q), V3::new(kt * KNOTS, 0.0, 0.0));
    a.state.omega = V3::new(0.0, 0.0, 6.0 * DEG);
    // Enough power to hold the speed.
    a.controls.throttle = 0.25;
    for _ in 0..(seconds / DT) as usize {
        a.step(DT, &env);
    }
    euler_angles(a.state.q).heading
}

#[test]
fn ground_loops_where_a_tricycle_runs_straight() {
    let wrap = |h: f64| remainder(h, 2.0 * PI_).abs() / DEG;
    let tricycle = wrap(swing(Aircraft::new(), 30.0, 4.0));
    let tailwheel = wrap(swing(extra(791.0), 30.0, 4.0));
    println!("nudged at 30 kt, hands off, 4 s later: C172 {tricycle:.1}°, Extra {tailwheel:.1}°");
    assert!(tricycle < 10.0);
    assert!(tailwheel > 3.0 * tricycle && tailwheel > 30.0);
}

#[test]
fn the_gear_takes_a_firm_landing_and_breaks_on_a_hard_one() {
    let touchdown = |sink: f64| {
        let mut a = extra(820.0);
        let env = runway();
        a.state.pos = V3::new(0.0, 0.0, -a.p.cg_height - 0.3);
        a.state.q = quaternion(0.0, a.p.ground_pitch, 0.0);
        a.state.vel = act(inverse(a.state.q), V3::new(28.0, 0.0, sink));
        for _ in 0..480 {
            a.step(DT, &env);
        }
        a
    };
    let firm = touchdown(2.7);
    println!("three-point at 2.7 m/s: {:?}", firm.crashed());
    assert!(firm.crashed().is_none());
    let hard = touchdown(4.0);
    println!("three-point at 4 m/s: {:?} {:?}", hard.damage().gear_broken, hard.crashed());
    assert!(hard.damage().gear_broken[1] || hard.damage().gear_broken[2]);
}


#[test]
fn placed_in_the_world() {
    let mut w = World::new(1);
    let parked = w.add_aircraft(AircraftKind::EXTRA300, Start::Home);
    let flying = w.add_aircraft(AircraftKind::EXTRA300, Start::Air { n: 0.0, e: 3000.0, heading: 0.0 });
    let start = w.vehicle(flying).aircraft().unwrap().state.pos.z;
    let airport = w.vehicle(parked).home.unwrap();
    let approach = w.add_aircraft(AircraftKind::EXTRA300, Start::Final { airport, reversed: false, distance: 2500.0 });
    for _ in 0..240 * 10 {
        w.step();
    }
    let (p, f, a) = (w.vehicle(parked), w.vehicle(flying), w.vehicle(approach));
    let pitch = euler_angles(p.aircraft().unwrap().state.q).pitch / DEG;
    println!("parked: pitch {pitch:.1}°, {:?}; flying: {:.0} m lost, {:.0} kt; final: {:.0} kt {:?}",
        p.aircraft().unwrap().crashed(), f.aircraft().unwrap().state.pos.z - start, f.aircraft().unwrap().indicated_airspeed() / KNOTS, a.aircraft().unwrap().indicated_airspeed() / KNOTS, a.aircraft().unwrap().crashed());
    assert!(p.aircraft().unwrap().data().on_ground() && p.aircraft().unwrap().crashed().is_none());
    assert_close(pitch, 11.0, 1.5, "parked on its tail");
    assert!((f.aircraft().unwrap().state.pos.z - start).abs() < 40.0 && f.aircraft().unwrap().crashed().is_none());
    assert_close(f.aircraft().unwrap().indicated_airspeed() / KNOTS, 135.0, 10.0, "speed of a start in the air");
    assert!(a.aircraft().unwrap().crashed().is_none() && a.aircraft().unwrap().flap_deg == 0.0);
}

/// The autopilot flies the Extra at its own speeds: a heading, altitude and speed beyond the C172's
/// 120 kt, and a whole circuit to a landing, holding the tail down on the rollout.
#[test]
fn the_autopilot_flies_it() {
    let mut w = World::new(1);
    let id = w.add_aircraft(AircraftKind::EXTRA300, Start::Air { n: 0.0, e: 3000.0, heading: 0.0 });
    let alt = -w.vehicle(id).aircraft().unwrap().state.pos.z;
    let goal = Goal::Hold { heading: Some(90.0 * DEG), vertical: Vertical::Altitude(alt + 100.0), airspeed: Some(150.0 * KNOTS) };
    let mut ap = Autopilot::for_type(goal, &AircraftType::extra300());
    for i in 0..240 * 120 {
        w.fly(id, &mut ap);
        w.step();
        if i % (240 * 10) == 0 {
            let p = w.vehicle(id);
            let e = euler_angles(p.aircraft().unwrap().state.q);
            println!("t {} hdg {:.0} alt {:.0} ias {:.0} roll {:.0} {:?}", i / 240, e.heading / DEG, -p.aircraft().unwrap().state.pos.z - alt, p.aircraft().unwrap().indicated_airspeed() / KNOTS, e.roll / DEG, p.aircraft().unwrap().crashed());
        }
    }
    let p = w.vehicle(id);
    assert!((euler_angles(p.aircraft().unwrap().state.q).heading / DEG - 90.0).abs() < 2.0);
    assert!((-p.aircraft().unwrap().state.pos.z - alt - 100.0).abs() < 10.0 && p.aircraft().unwrap().crashed().is_none());
    assert!((p.aircraft().unwrap().indicated_airspeed() / KNOTS - 150.0).abs() < 3.0, "{:.0} kt", p.aircraft().unwrap().indicated_airspeed() / KNOTS);
    let mut w = World::new(1);
    let id = w.add_aircraft(AircraftKind::EXTRA300, Start::Home);
    let airport = w.vehicle(id).home.unwrap();
    let mut ap = Autopilot::for_type(Goal::Land { airport, reversed: false }, &AircraftType::extra300());
    for i in 0..240 * 600 {
        w.fly(id, &mut ap);
        w.step();
        if i % (240 * 20) == 0 {
            let p = w.vehicle(id);
            let e = euler_angles(p.aircraft().unwrap().state.q);
            println!("t {} {:?} agl {:.0} ias {:.0} pitch {:.0} roll {:.0} {:?}", i / 240, ap.phase(), p.agl(), p.aircraft().unwrap().indicated_airspeed() / KNOTS, e.pitch / DEG, e.roll / DEG, p.aircraft().unwrap().crashed());
        }
        if ap.finished() || w.vehicle(id).aircraft().unwrap().crashed().is_some() { break; }
    }
    println!("end {:?} {:?}", ap.phase(), w.vehicle(id).aircraft().unwrap().crashed());
    assert_eq!(ap.phase(), Phase::Stopped);
    assert!(w.vehicle(id).aircraft().unwrap().crashed().is_none());
}

/// Reference, not truth: JSBSim's `propC10v.xml` (a Clark Y variable-pitch propeller, the numbers
/// only) gives the advance ratio of zero thrust at five blade angles. A coarser blade moves it as
/// the governor's slide along the tables does here (0.75 π per unit of the blade angle's tangent).
#[test]
fn the_blade_angle_moves_the_propeller_as_jsbsims_variable_pitch_one() {
    // Blade angle (degrees) and J of zero thrust, from the C_THRUST table of propC10v.xml.
    let reference = [(11.0, 0.6455), (15.0, 0.7957), (19.0, 0.8979), (23.0, 1.1939), (27.0, 1.3924)];
    let n = reference.len() as f64;
    let x: Vec<f64> = reference.iter().map(|&(b, _)| (b * DEG).tan()).collect();
    let (mx, my) = (x.iter().sum::<f64>() / n, reference.iter().map(|r| r.1).sum::<f64>() / n);
    let slope = x.iter().zip(&reference).map(|(x, r)| (x - mx) * (r.1 - my)).sum::<f64>()
        / x.iter().map(|x| (x - mx).powi(2)).sum::<f64>();
    // Ours: the J at which the table's thrust crosses zero, at its own blade angle and coarser.
    let p = AircraftType::extra300();
    let zero = |shift: f64| {
        let ct = |j: f64| interpolate(p.prop_j, p.prop_ct, j - shift);
        let (mut lo, mut hi) = (0.0, 3.0);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if ct(mid) > 0.0 { lo = mid } else { hi = mid }
        }
        lo
    };
    let shift = |b: f64| 0.75 * std::f64::consts::PI * ((b * DEG).tan() - p.prop_pitch.tan());
    let ours = (zero(shift(27.0)) - zero(shift(11.0))) / ((27.0 * DEG).tan() - (11.0 * DEG).tan());
    println!("zero thrust moves {ours:.2} per unit of tan(blade), propC10v {slope:.2}; at 18.5°: ours J {:.2}", zero(0.0));
    assert_close(ours, slope, 0.10, "zero-thrust J per tan(blade angle)");
}

/// Right pedal turns the steering wheel so the aircraft turns right, on a nose wheel and on a
/// tailwheel alike: the nose wheel's front to the right, the tailwheel's to the left, both by
/// `steer_max` at walking pace and less the faster they roll.
#[test]
fn right_pedal_turns_right_on_the_ground() {
    for kind in [AircraftKind::C172, AircraftKind::EXTRA300] {
        let mut a = Aircraft::of(kind);
        let p = a.p.clone();
        let steering = p.gear.iter().find(|w| w.steers).unwrap();
        let front = if steering.contact.x > 0.0 { 1.0 } else { -1.0 };
        assert_close(p.steer_angle(steering, 1.0, 0.0), front * p.steer_max, 1e-9, "full pedal standing");
        assert_close(p.steer_angle(steering, -0.5, 12.0), -front * p.steer_max / 4.0, 1e-9, "half pedal at 12 m/s");
        assert_eq!(p.steer_angle(&p.gear[1], 1.0, 0.0), 0.0);

        let env = on_ground(&mut a);
        a.state.vel.x = 4.0;
        a.controls.rudder = 1.0;
        for _ in 0..240 {
            a.step(DT, &env);
        }
        assert!(a.state.omega.z > 0.2, "{kind:?}: yaw rate {:.2} rad/s with right pedal", a.state.omega.z);
        assert!(a.data().wheel_roll[1] > 2.0 && a.data().wheel_compression[1] > 0.02, "{kind:?}: {:?}", a.data());
    }
}
