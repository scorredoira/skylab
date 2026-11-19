//! The flight model against the figures of the Cessna 172 handbook.

mod common;

use common::assert_close;
use sim_core::*;

const DT: f64 = 1.0 / 240.0;
const TAU: f64 = 2.0 * std::f64::consts::PI;

#[test]
fn stall_speeds() {
    let a = Aircraft::new();
    let (clean, full) = (a.stall(0.0, 0.0), a.stall(30.0, 0.0));
    println!(
        "stall clean {} kt CLmax {}; flaps {} kt CLmax {}",
        clean.speed / KNOTS,
        clean.cl_max,
        full.speed / KNOTS,
        full.cl_max
    );
    assert_close(clean.speed / KNOTS, 53.0, 2.0, "clean stall speed");
    assert_close(full.speed / KNOTS, 48.0, 2.0, "full-flap stall speed");
}

#[test]
fn cruise_speed() {
    let a = Aircraft::new();
    let altitude = 8000.0 * FEET;
    let (mut lo, mut hi) = (45.0, 80.0);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if a.trim_level(mid, altitude).is_some() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let t = a.trim_level(lo - 0.05, altitude).unwrap();
    println!(
        "max level speed at 8000 ft: {} kt, throttle {}, rpm {}, elevator {}",
        lo / KNOTS,
        t.throttle,
        t.prop_speed * 60.0 / TAU,
        t.elevator
    );
    assert_close(lo / KNOTS, 122.0, 5.0, "max level speed");
    assert_close(
        t.prop_speed * 60.0 / TAU,
        2600.0,
        150.0,
        "rpm at max level speed",
    );
}

#[test]
fn climb_rate() {
    let a = Aircraft::new();
    let v = 73.0 * KNOTS;
    let t = a.trim(v, 0.0, 0.0, Some(1.0), 0.0).unwrap();
    let fpm = v * t.climb_angle.sin() / FEET * 60.0;
    println!(
        "climb at 73 kt: {fpm} ft/min, rpm {}, alpha {}, elevator {}",
        t.prop_speed * 60.0 / TAU,
        t.alpha / DEG,
        t.elevator
    );
    assert_close(fpm, 770.0, 50.0, "climb rate");
}

#[test]
fn static_rpm() {
    let a = Aircraft::new();
    let (r, r_prop) = a.steady(0.1, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0);
    let (idle, idle_prop) = a.steady(0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    println!(
        "static rpm {} thrust {} N; idle rpm {} thrust {}",
        r_prop * 60.0 / TAU,
        r.thrust,
        idle_prop * 60.0 / TAU,
        idle.thrust
    );
    assert_close(r_prop * 60.0 / TAU, 2350.0, 100.0, "static rpm");
    assert_close(idle_prop * 60.0 / TAU, 650.0, 120.0, "idle rpm");
}

#[test]
fn glide() {
    // The handbook gives 9:1 with the engine dead; idling it is a little better.
    let a = Aircraft::new();
    let (mut best, mut at) = (0.0, 0.0);
    for i in 0..=12 {
        let kt = 55.0 + i as f64 * 2.5;
        let Some(t) = a.trim(kt * KNOTS, 1000.0, 0.0, Some(0.0), 0.0) else {
            continue;
        };
        let ratio = -1.0 / t.climb_angle.tan();
        if ratio > best {
            best = ratio;
            at = kt;
        }
    }
    println!("best glide {best}:1 at {at} kt");
    assert_close(best, 10.0, 1.0, "best glide ratio");
}

#[test]
fn trim_across_speeds() {
    let a = Aircraft::new();
    for kt in [60.0, 80.0, 100.0, 120.0] {
        let t = a.trim_level(kt * KNOTS, 1000.0);
        assert!(t.is_some(), "no trim at {kt} kt");
        if let Some(t) = t {
            println!(
                "trim {kt} kt: alpha {}° elevator {} throttle {} rpm {}",
                t.alpha / DEG,
                t.elevator,
                t.throttle,
                t.prop_speed * 60.0 / TAU
            );
        }
    }
    // Slower flight needs more back elevator: positive stick-fixed stability.
    let slow = a.trim_level(65.0 * KNOTS, 1000.0).unwrap();
    let fast = a.trim_level(110.0 * KNOTS, 1000.0).unwrap();
    assert!(slow.elevator > fast.elevator);
}

fn fly_trimmed(a: &mut Aircraft, kt: f64, altitude: f64) -> Environment {
    let t = a.trim_level(kt * KNOTS, altitude).unwrap();
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(kt * KNOTS * t.alpha.cos(), 0.0, kt * KNOTS * t.alpha.sin()),
        q: quaternion(0.0, t.alpha, 0.0),
        prop_speed: t.prop_speed,
        ..State::default()
    };
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    Environment {
        rho: atmosphere::density(altitude),
        ..Environment::default()
    }
}

#[test]
fn hands_off_stability() {
    let mut a = Aircraft::new();
    let env = fly_trimmed(&mut a, 100.0, 1000.0);
    // Disturb it and let go: the phugoid and short period must not diverge.
    a.state.omega.y = 0.1;
    let (mut min_alt, mut max_alt) = (1000.0_f64, 1000.0_f64);
    for _ in 0..240 * 90 {
        a.step(DT, &env);
        min_alt = min_alt.min(-a.state.pos.z);
        max_alt = max_alt.max(-a.state.pos.z);
    }
    let e = euler_angles(a.state.q);
    println!(
        "after 90 s: altitude {min_alt}...{max_alt}, pitch {}, roll {}, tas {}",
        e.pitch / DEG,
        e.roll / DEG,
        a.data().tas / KNOTS
    );
    assert!(max_alt - min_alt < 150.0);
    assert!(e.pitch.abs() < 12.0 * DEG);
    assert!(e.roll.abs() < 60.0 * DEG);
    assert_close(a.data().tas / KNOTS, 100.0, 12.0, "airspeed after 90 s");
}

#[test]
fn roll_rate() {
    let mut a = Aircraft::new();
    let env = fly_trimmed(&mut a, 100.0, 1000.0);
    a.controls.aileron = 1.0;
    for _ in 0..240 {
        a.step(DT, &env);
    }
    println!(
        "roll rate at 100 kt, full aileron: {}°/s",
        a.state.omega.x / DEG
    );
    assert!(a.state.omega.x / DEG > 35.0);
    assert!(a.state.omega.x / DEG < 80.0);
}

#[test]
fn rudder_yaws_and_elevator_pitches() {
    let mut a = Aircraft::new();
    let mut env = fly_trimmed(&mut a, 100.0, 1000.0);
    a.controls.rudder = 0.5;
    for _ in 0..120 {
        a.step(DT, &env);
    }
    assert!(a.state.omega.z > 0.02);

    env = fly_trimmed(&mut a, 100.0, 1000.0);
    a.controls.rudder = 0.0;
    a.controls.elevator += 0.2;
    for _ in 0..120 {
        a.step(DT, &env);
    }
    assert!(a.state.omega.y > 0.02);
}

#[test]
fn spin_entry() {
    // Power off, stick full back, full rudder: it must depart and autorotate.
    let mut a = Aircraft::new();
    let env = fly_trimmed(&mut a, 60.0, 2000.0);
    a.controls.throttle = 0.0;
    a.controls.elevator = 1.0;
    a.controls.rudder = -1.0;
    let mut turned = 0.0;
    for _ in 0..240 * 12 {
        a.step(DT, &env);
        turned += act(a.state.q, a.state.omega).z / 240.0;
    }
    println!(
        "spin: turned {}° in 12 s, alpha {}, lost {} m, ias {}",
        turned / DEG,
        a.data().alpha / DEG,
        2000.0 + a.state.pos.z,
        a.data().ias / KNOTS
    );
    assert!(turned < -360.0 * DEG);
    assert!(a.data().alpha > 18.0 * DEG);

    // Recovery: opposite rudder, stick forward.
    a.controls.rudder = 1.0;
    a.controls.elevator = -0.3;
    for _ in 0..240 * 4 {
        a.step(DT, &env);
    }
    a.controls.rudder = 0.0;
    for _ in 0..240 * 2 {
        a.step(DT, &env);
    }
    println!(
        "recovery: alpha {}, yaw rate {}°/s",
        a.data().alpha / DEG,
        a.state.omega.z / DEG
    );
    assert!(a.data().alpha < 14.0 * DEG);
}

fn ground_env() -> Environment {
    Environment {
        ground_height: 0.0,
        kind: GroundKind::Runway,
        ..Environment::default()
    }
}

#[test]
fn rests_on_its_wheels() {
    let mut a = Aircraft::new();
    let env = ground_env();
    a.state.pos = V3::new(0.0, 0.0, -a.p.cg_height);
    a.controls.parking_brake = true;
    for _ in 0..240 * 10 {
        a.step(DT, &env);
    }
    let load: f64 = a.data().wheel_load.iter().sum();
    println!(
        "at rest: loads {:?}, height {}, moved {}, pitch {}",
        a.data().wheel_load,
        -a.state.pos.z,
        a.state.pos.x,
        euler_angles(a.state.q).pitch / DEG
    );
    assert!(a.crashed().is_none());
    assert_close(load, a.p.mass * GRAVITY, 300.0, "weight on the wheels");
    assert!(a.state.pos.x.abs() < 0.5);
    assert!(a.state.vel.length() < 0.05);
}

#[test]
fn takeoff_roll() {
    // Handbook ground roll at sea level and maximum weight is about 270 m.
    let mut a = Aircraft::new();
    let env = ground_env();
    a.state.pos = V3::new(0.0, 0.0, -a.p.cg_height);
    a.controls.throttle = 1.0;
    a.controls.flap_notch = 1;
    a.flap_deg = 10.0;
    let mut liftoff: Option<f64> = None;
    let mut t = 0.0;
    while t < 60.0 {
        let e = euler_angles(a.state.q);
        if a.data().ias > 45.0 * KNOTS {
            a.controls.elevator = clamp(
                (9.0 * DEG - e.pitch) * 6.0 - a.state.omega.y * 2.0,
                -0.3,
                1.0,
            );
        }
        a.controls.rudder = clamp(
            -remainder(e.heading, TAU) * 6.0 - a.state.omega.z * 2.0 - a.state.pos.y * 0.3,
            -1.0,
            1.0,
        );
        a.controls.aileron = clamp(-e.roll * 2.0 - a.state.omega.x, -1.0, 1.0);
        a.step(DT, &env);
        t += DT;
        if liftoff.is_none() && !a.data().on_ground() && -a.state.pos.z > a.p.cg_height + 0.3 {
            liftoff = Some(a.state.pos.x);
            break;
        }
    }
    println!(
        "takeoff roll {} m in {t} s, ias {} kt, drift {} m",
        liftoff.unwrap_or(-1.0),
        a.data().ias / KNOTS,
        a.state.pos.y
    );
    assert!(a.crashed().is_none());
    assert!(liftoff.is_some());
    assert_close(liftoff.unwrap_or(0.0), 270.0, 70.0, "takeoff roll");
    assert!(a.state.pos.y.abs() < 8.0);
}

#[test]
fn brakes_stop_it() {
    let mut a = Aircraft::new();
    let env = ground_env();
    a.state.pos = V3::new(0.0, 0.0, -a.p.cg_height);
    a.state.vel = V3::new(25.0, 0.0, 0.0);
    a.controls.brake_left = 1.0;
    a.controls.brake_right = 1.0;
    let mut t = 0.0;
    while t < 30.0 && a.state.vel.x > 0.2 {
        a.step(DT, &env);
        t += DT;
    }
    println!("braked from 25 m/s in {} m, {t} s", a.state.pos.x);
    assert!(a.crashed().is_none());
    assert!(a.state.pos.x < 160.0);
    assert!(a.state.pos.x > 50.0);
}

#[test]
fn hard_landing_breaks_the_gear() {
    let mut a = Aircraft::new();
    let env = ground_env();
    a.state.pos = V3::new(0.0, 0.0, -a.p.cg_height - 0.5);
    a.state.vel = V3::new(30.0, 0.0, 8.0);
    for _ in 0..240 {
        a.step(DT, &env);
    }
    assert!(a.crashed().is_some());
}

#[test]
fn stall_hysteresis() {
    // A wing that has stalled keeps its flow separated until the angle of attack has come down
    // about 3° below the angle it stalled at; lower than that it flies as if nothing had happened.
    let a = Aircraft::new();
    let env = Environment::default();
    let lift = |alpha: f64, separation: f64| {
        let v = 30.0;
        let s = State {
            vel: V3::new(v * alpha.cos(), 0.0, v * alpha.sin()),
            q: quaternion(0.0, alpha, 0.0),
            ..State::default()
        };
        let d = a.forces_separated(&s, &AircraftControls::default(), &env, 0.0, 0.0, &[separation; 8]);
        -d.specific.z * a.mass() / (0.5 * env.rho * v * v * a.p.wing_area)
    };
    let (attached, separated) = (lift(15.0 * DEG, 0.0), lift(15.0 * DEG, 1.0));
    let (low_attached, low_separated) = (lift(7.0 * DEG, 0.0), lift(7.0 * DEG, 1.0));
    println!(
        "normal force coefficient at 15°: {attached:.2} attached, {separated:.2} after a stall; at 7°: {low_attached:.2} and {low_separated:.2}"
    );
    assert!(separated < 0.85 * attached);
    assert_close(low_separated, low_attached, 0.01, "reattached at 7°");
}
