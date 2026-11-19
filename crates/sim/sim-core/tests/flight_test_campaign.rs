//! Flight-test style measurements of the bare aircraft (no keyboard assist, still air), printed
//! next to the figures of the Cessna 172N handbook and published stability data.
//! Run with `-- --nocapture` to read the report.

mod common;

use common::{assert_close, Sim};
use sim_core::*;

struct Setup {
    a: Aircraft,
    env: Environment,
    trim: Trim,
}

fn setup(
    kt: f64,
    altitude: f64,
    flap: f64,
    throttle: Option<f64>,
    climb_angle: f64,
) -> Option<Setup> {
    let mut a = Aircraft::new();
    let t = a.trim(kt * KNOTS, altitude, flap, throttle, climb_angle)?;
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(kt * KNOTS * t.alpha.cos(), 0.0, kt * KNOTS * t.alpha.sin()),
        q: quaternion(0.0, t.alpha + t.climb_angle, 0.0),
        prop_speed: t.prop_speed,
        ..State::default()
    };
    a.flap_deg = flap;
    a.controls.flap_notch = (flap / 10.0) as i32;
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    let env = Environment {
        rho: atmosphere::density(altitude),
        ..Environment::default()
    };
    // Let the downwash settle.
    for _ in 0..240 {
        a.step(1.0 / 240.0, &env);
    }
    Some(Setup { a, env, trim: t })
}

/// Period and damping ratio of an oscillation from its successive peaks.
fn oscillation(samples: &[f64], dt: f64) -> Option<(f64, f64)> {
    let mean = samples.iter().fold(0.0, |s, x| s + x) / samples.len() as f64;
    let mut peaks: Vec<(f64, f64)> = Vec::new();
    for i in 1..samples.len() - 1 {
        let v = samples[i] - mean;
        if v > 0.0 && samples[i] >= samples[i - 1] && samples[i] > samples[i + 1] {
            peaks.push((i as f64 * dt, v));
        }
    }
    if peaks.len() < 2 {
        return None;
    }
    let period = (peaks[peaks.len() - 1].0 - peaks[0].0) / (peaks.len() - 1) as f64;
    let d = (peaks[0].1 / peaks[1].1).ln();
    Some((
        period,
        d / (4.0 * std::f64::consts::PI * std::f64::consts::PI + d * d).sqrt(),
    ))
}

#[test]
fn campaign() {
    let dt = 1.0 / 240.0;
    let line = |s: String| println!("FT {s}");

    // ---- Performance
    for (ft, reference) in [
        (0.0, 770.0),
        (4000.0, 580.0),
        (8000.0, 390.0),
        (12000.0, 200.0),
    ] {
        let sigma: f64 = atmosphere::density(ft * FEET) / atmosphere::SEA_LEVEL_DENSITY;
        let tas = 73.0 / sigma.sqrt();
        if let Some(s) = setup(tas, ft * FEET, 0.0, Some(1.0), 0.0) {
            let fpm = tas * KNOTS * s.trim.climb_angle.sin() / FEET * 60.0;
            assert_close(
                fpm,
                reference,
                (reference * 0.1_f64).max(30.0),
                &format!("climb at {ft} ft"),
            );
            line(format!("climb {ft:5.0} ft at 73 KIAS: {fpm:4.0} ft/min (handbook {reference:.0}), {:4.0} rpm", s.trim.prop_speed * 9.549));
        }
    }
    let a0 = Aircraft::new();
    for (ft, rpm, reference) in [
        (8000.0, 2700.0, 122.0),
        (8000.0, 2500.0, 111.0),
        (8000.0, 2300.0, 99.0),
        (4000.0, 2500.0, 115.0),
        (4000.0, 2300.0, 105.0),
        (4000.0, 2100.0, 93.0),
        (2000.0, 2400.0, 111.0),
        (2000.0, 2200.0, 100.0),
    ] {
        let (mut lo, mut hi) = (60.0, 135.0);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            match a0.trim_level(mid * KNOTS, ft * FEET) {
                Some(t) if t.prop_speed * 9.549 < rpm => lo = mid,
                _ => hi = mid,
            }
        }
        let t = a0.trim_level(lo * KNOTS, ft * FEET);
        let power = t.map_or(0.0, |t| {
            a0.engine_torque(t.throttle, t.prop_speed, atmosphere::density(ft * FEET))
                * t.prop_speed
                / 119_300.0
                * 100.0
        });
        assert_close(lo, reference, 5.0, &format!("cruise at {ft} ft, {rpm} rpm"));
        line(format!("cruise {ft:5.0} ft {rpm:4.0} rpm: {lo:5.1} KTAS (handbook {reference:.0}), {power:2.0}% power"));
    }
    for (flap, kt) in [(0.0, 65.0), (30.0, 60.0)] {
        if let Some(s) = setup(kt, 600.0, flap, Some(0.0), 0.0) {
            line(format!(
                "glide idle flaps {flap:2.0} at {kt:2.0} kt: {:4.0} ft/min, ratio {:.1}, {:4.0} rpm",
                -kt * KNOTS * s.trim.climb_angle.sin() / FEET * 60.0,
                -1.0 / s.trim.climb_angle.tan(),
                s.trim.prop_speed * 9.549
            ));
        }
    }
    if let Some(s) = setup(65.0, 300.0, 30.0, None, -3.0 * DEG) {
        line(format!(
            "approach 65 kt flaps 30 on 3°: {:4.0} rpm, throttle {:.2}, pitch {:+.1}° (typical 1500-1700 rpm)",
            s.trim.prop_speed * 9.549,
            s.trim.throttle,
            (s.trim.alpha - 3.0 * DEG) / DEG
        ));
    }

    // ---- Longitudinal modes at 100 kt
    if let Some(mut s) = setup(100.0, 1500.0, 0.0, None, 0.0) {
        s.a.state.vel.x += 5.0;
        let mut tas = Vec::new();
        for i in 0..240 * 150 {
            s.a.step(dt, &s.env);
            if i % 24 == 0 {
                tas.push(s.a.data().tas);
            }
        }
        if let Some((period, zeta)) = oscillation(&tas, 0.1) {
            line(format!("phugoid at 100 kt: period {period:.1} s, damping {zeta:.3} (theory 23 s, 172 about 0.10-0.13)"));
        }
    }
    if let Some(mut s) = setup(100.0, 1500.0, 0.0, None, 0.0) {
        let a0 = s.a.data().alpha;
        s.a.controls.elevator += 0.05;
        let (mut peak, mut peak_t, mut at2) = (0.0, 0.0, 0.0);
        let mut alpha = Vec::new();
        for i in 0..240 * 3 {
            s.a.step(dt, &s.env);
            let d = s.a.data().alpha - a0;
            alpha.push(d);
            if d > peak {
                peak = d;
                peak_t = i as f64 * dt;
            }
            if i == 480 {
                at2 = d;
            }
        }
        let t90 = alpha.iter().position(|d| *d > 0.9 * at2).unwrap_or(0) as f64 * dt;
        line(format!(
            "short period at 100 kt: alpha reaches 90% in {t90:.2} s, peak at {peak_t:.2} s, overshoot {:.0}% (172: fast and well damped, about 0.3 s, <10%)",
            (peak / at2 - 1.0) * 100.0
        ));
    }

    // ---- Lateral modes at 100 kt
    if let Some(mut s) = setup(100.0, 1500.0, 0.0, None, 0.0) {
        let (mut beta, mut roll) = (Vec::new(), Vec::new());
        for i in 0..240 * 14 {
            s.a.controls.rudder = if i < 240 { 0.5 } else { 0.0 };
            s.a.step(dt, &s.env);
            if i >= 240 && i % 12 == 0 {
                beta.push(s.a.data().beta / DEG);
                roll.push(euler_angles(s.a.state.q).roll / DEG);
            }
        }
        if let Some((period, zeta)) = oscillation(&beta, 0.05) {
            line(format!(
                "dutch roll at 100 kt: period {period:.2} s, damping {zeta:.2} (172 about 2-3 s, 0.2), first swing {:.1}° of slip",
                beta.iter().fold(0.0_f64, |m, b| m.max(b.abs()))
            ));
        } else {
            line("dutch roll: no oscillation (overdamped)".to_string());
        }
    }
    if let Some(mut s) = setup(100.0, 1500.0, 0.0, None, 0.0) {
        s.a.controls.aileron = 0.3;
        let mut rates = Vec::new();
        for _ in 0..240 * 2 {
            s.a.step(dt, &s.env);
            rates.push(s.a.state.omega.x / DEG);
        }
        let steady = rates[400];
        let p63 = rates.iter().position(|r| *r > 0.63 * steady).unwrap_or(0) as f64 * dt;
        line(format!("roll at 100 kt: {:.0}°/s per full aileron, time constant {p63:.2} s (172 about 45-55°/s, 0.1-0.2 s)", steady / 0.3));
    }
    for kt in [70.0, 100.0] {
        if let Some(mut s) = setup(kt, 1500.0, 0.0, None, 0.0) {
            s.a.state.q *= axis_angle(10.0 * DEG, V3::new(1.0, 0.0, 0.0));
            let mut b20 = 0.0;
            for i in 0..240 * 40 {
                s.a.step(dt, &s.env);
                if i == 240 * 20 {
                    b20 = euler_angles(s.a.state.q).roll / DEG;
                }
            }
            let b40 = euler_angles(s.a.state.q).roll / DEG;
            line(format!("spiral at {kt:3.0} kt: 10° of bank becomes {b20:.1}° after 20 s and {b40:.1}° after 40 s (172: nearly neutral, slow)"));
        }
    }

    // ---- Climb hands-off: how much it pulls
    if let Some(mut s) = setup(73.0, 300.0, 0.0, Some(1.0), 0.0) {
        let mut ball = 0.0;
        for _ in 0..240 * 12 {
            let e = euler_angles(s.a.state.q);
            s.a.controls.aileron = clamp(-e.roll * 3.0 - s.a.state.omega.x, -1.0, 1.0);
            s.a.step(dt, &s.env);
            ball += (s.a.data().specific.y.atan2(-s.a.data().specific.z) / DEG - ball) * dt;
        }
        line(format!(
            "climb at 73 kt, feet off, wings held level: ball {:.1}° off, slip {:.1}°, turning {:.1}°/s, aileron held {:.2} (172: half a ball, slow left turn)",
            -ball,
            s.a.data().beta / DEG,
            s.a.state.omega.z / DEG,
            s.a.controls.aileron
        ));
    }

    // ---- Stall, power off, one knot per second
    for flap in [0.0, 30.0] {
        let Some(mut s) = setup(70.0, 1500.0, flap, Some(0.0), 0.0) else {
            continue;
        };
        let mut horn: Option<f64> = None;
        let (mut min_ias, mut break_pitch, mut max_bank, mut top) = (999.0_f64, 0.0, 0.0_f64, 0.0);
        let mut broke = false;
        for _ in 0..240 * 60 {
            let e = euler_angles(s.a.state.q);
            // Bring the yoke back slowly and steadily until it stalls or runs out of travel.
            if !broke {
                s.a.controls.elevator = (s.a.controls.elevator + 0.03 * dt).min(1.0);
            }
            s.a.controls.aileron = clamp(-e.roll * 2.0 - s.a.state.omega.x, -1.0, 1.0);
            s.a.controls.rudder = clamp(s.a.data().beta * 4.0 - s.a.state.omega.z, -1.0, 1.0);
            s.a.step(dt, &s.env);
            if horn.is_none() && s.a.data().stall_warning {
                horn = Some(s.a.data().ias / KNOTS);
            }
            if !broke {
                min_ias = min_ias.min(s.a.data().ias / KNOTS);
                max_bank = max_bank.max((e.roll / DEG).abs());
                if s.a.controls.elevator >= 0.999
                    || (s.a.data().stalled > 0.5 && s.a.state.omega.y < -3.0 * DEG)
                {
                    broke = true;
                    break_pitch = e.pitch / DEG;
                    top = -s.a.state.pos.z;
                }
            }
        }
        assert!(horn.unwrap_or(0.0) - min_ias > 3.0, "stall horn too late");
        assert!(
            horn.unwrap_or(99.0) - min_ias < 11.0,
            "stall horn too early"
        );
        line(format!(
            "stall flaps {flap:2.0}: horn at {:.0} kt, stall at {min_ias:.0} kt, nose {break_pitch:.0}° up at the break, bank within {max_bank:.0}°, elevator {:.2}, sank to {:.0} m below (handbook {} KCAS; horn 5-10 kt before)",
            horn.unwrap_or(0.0),
            s.a.controls.elevator,
            top + s.a.state.pos.z,
            if flap == 0.0 { "53" } else { "48" }
        ));
    }

    // ---- Landing roll with the automatic brakes
    {
        let mut sim = Sim::new(1);
        sim.aircraft_mut().state.vel = V3::new(47.0 * KNOTS, 0.0, 0.0);
        sim.aircraft_mut().controls.throttle = 0.0;
        let x0 = sim.aircraft().state.pos;
        let mut t = 0.0;
        while sim.aircraft().state.vel.length() > 0.5 && t < 60.0 {
            sim.step();
            t += dt;
        }
        line(format!(
            "landing roll from 47 kt: {:.0} m in {t:.0} s (handbook 160 m with hard braking)",
            (sim.aircraft().state.pos - x0).length()
        ));
    }
}
