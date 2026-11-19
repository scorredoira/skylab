//! The flight model against JSBSim's `c172p` (aircraft/c172p/c172p.xml, engine/eng_io320.xml,
//! engine/prop_75in2f.xml of https://github.com/JSBSim-Team/jsbsim), the model FlightGear flies.
//!
//! The handbook is the truth for what it gives (speeds, climb, cruise, distances: see
//! `flight_model.rs` and `flight_test_campaign.rs`). JSBSim is the reference for what the handbook
//! does not give: stability and control derivatives, response magnitudes, trim changes.
//!
//! The reference numbers are constants here so the tests run without JSBSim. Those marked
//! "table" are read from the XML; those marked "run" were measured by flying the JSBSim
//! executable loaded to 2300 lb (CG at 44.3 in, 1 in aft of its aerodynamic reference point) with
//! its total inertia set to the published 948 / 1346 / 1967 slug·ft².
//! Run with `-- --nocapture` to read the comparison.

mod common;

use sim_core::*;

const S: f64 = 16.17;
const B: f64 = 10.91;
const C: f64 = 1.49;

/// A flight condition to take derivatives at.
struct Point {
    a: Aircraft,
    tas: f64,
    altitude: f64,
    flap: f64,
    trim: Trim,
}

/// Aerodynamic state for one evaluation.
#[derive(Clone, Copy)]
struct Probe {
    alpha: f64,
    beta: f64,
    omega: V3,
    controls: AircraftControls,
}

/// CL, CD, CY, Cl, Cm, Cn.
type Coefficients = [f64; 6];

impl Point {
    fn new(kt: f64, altitude: f64, flap: f64) -> Point {
        let a = Aircraft::new();
        let trim = a.trim(kt * KNOTS, altitude, flap, None, 0.0).expect("trim");
        Point {
            a,
            tas: kt * KNOTS,
            altitude,
            flap,
            trim,
        }
    }

    fn probe(&self) -> Probe {
        Probe {
            alpha: self.trim.alpha,
            beta: 0.0,
            omega: V3::ZERO,
            controls: AircraftControls {
                elevator: self.trim.elevator,
                throttle: self.trim.throttle,
                ..AircraftControls::default()
            },
        }
    }

    /// Coefficients with the downwash settled, thrust removed from lift and drag.
    fn coefficients(&self, p: &Probe) -> Coefficients {
        let rho = atmosphere::density(self.altitude);
        let env = Environment {
            rho,
            ..Environment::default()
        };
        let v = self.tas;
        let s = State {
            pos: V3::new(0.0, 0.0, -self.altitude),
            vel: V3::new(
                v * p.alpha.cos() * p.beta.cos(),
                v * p.beta.sin(),
                v * p.alpha.sin() * p.beta.cos(),
            ),
            q: quaternion(0.0, p.alpha, 0.0),
            omega: p.omega,
            prop_speed: self.trim.prop_speed,
            ..State::default()
        };
        let d = self.a.forces_settled(&s, &p.controls, &env, self.flap);
        let f = d.specific * self.a.mass() - V3::new(d.thrust, 0.0, 0.0);
        let q = 0.5 * rho * v * v;
        let (sa, ca) = p.alpha.sin_cos();
        [
            (-f.z * ca + f.x * sa) / (q * S),
            -(f.x * ca + f.z * sa) / (q * S),
            f.y / (q * S),
            d.moment.x / (q * S * B),
            d.moment.y / (q * S * C),
            d.moment.z / (q * S * B),
        ]
    }

    /// Central difference of the coefficients along `change`, per unit of `step`.
    fn derivative(&self, step: f64, change: impl Fn(&mut Probe, f64)) -> Coefficients {
        let (mut lo, mut hi) = (self.probe(), self.probe());
        change(&mut lo, -step);
        change(&mut hi, step);
        let (a, b) = (self.coefficients(&lo), self.coefficients(&hi));
        std::array::from_fn(|i| (b[i] - a[i]) / (2.0 * step))
    }
}

/// One line of the comparison, and the check that ours is within `tolerance` (a fraction of the
/// reference; see `tolerance`).
#[track_caller]
fn compare(what: &str, ours: f64, reference: f64, source: &str) {
    let tolerance = tolerance(what);
    let error = if reference.abs() > 1e-9 {
        (ours - reference) / reference.abs() * 100.0
    } else {
        0.0
    };
    println!("REF {what:34} ours {ours:9.4}  ref {reference:9.4}  {error:+6.0}%  ({source})");
    assert!(
        (ours - reference).abs() <= tolerance * reference.abs().max(1e-9),
        "{what}: {ours} is not within {:.0}% of {reference} ({source})",
        tolerance * 100.0
    );
}

#[test]
fn derivatives_in_cruise() {
    // 100 KCAS at 1000 m, as in the JSBSim runs.
    let sigma = atmosphere::density(1000.0) / atmosphere::SEA_LEVEL_DENSITY;
    let p = Point::new(100.0 / sigma.sqrt(), 1000.0, 0.0);
    let c0 = p.coefficients(&p.probe());
    let v = p.tas;
    println!(
        "REF trim: alpha {:.2}°, elevator {:.3}, throttle {:.2}, rpm {:.0}, CL {:.3}, CD {:.4}",
        p.trim.alpha / DEG,
        p.trim.elevator,
        p.trim.throttle,
        p.trim.prop_speed * 9.549,
        c0[0],
        c0[1]
    );
    let da = p.derivative(0.5 * DEG, |x, h| x.alpha += h);
    let db = p.derivative(1.0 * DEG, |x, h| x.beta += h);
    let dp = p.derivative(0.05, |x, h| x.omega.x += h);
    let dq = p.derivative(0.05, |x, h| x.omega.y += h);
    let dr = p.derivative(0.05, |x, h| x.omega.z += h);
    // AircraftControls, per radian of surface. Elevator: trailing edge down positive, as in JSBSim.
    let de = p.derivative(0.05, |x, h| x.controls.elevator += h);
    let elevator_rad = -p.a.p.elevator_down; // around a trim with the yoke slightly forward
    let dail = p.derivative(0.1, |x, h| x.controls.aileron += h);
    let drud = p.derivative(0.1, |x, h| x.controls.rudder += h);
    let (b2v, c2v) = (B / (2.0 * v), C / (2.0 * v));
    compare("CL_alpha /rad", da[0], 5.33, "table CLwbh, 0..0.09 rad");
    compare(
        "Cm_alpha /rad (CG 44.3 in)",
        da[4],
        -1.70,
        "table -1.8 at 43.2 in",
    );
    compare("Cm_q", dq[4] / c2v, -12.4, "table");
    compare("CL_q", dq[0] / c2v, 3.9, "table");
    compare("Cm_de /rad", de[4] / elevator_rad, -1.122, "table");
    compare("CL_de /rad", de[0] / elevator_rad, 0.43, "table");
    compare("CY_beta /rad", db[2], -0.393, "table");
    compare("Cl_beta /rad", db[3], -0.0923, "table");
    compare("Cn_beta /rad", db[5], 0.0587, "table");
    compare("Cl_p", dp[3] / b2v, -0.484, "table");
    compare("Cn_p", dp[5] / b2v, -0.0278, "table");
    compare("CY_p", dp[2] / b2v, -0.09, "table at alpha 1.2°");
    compare("Cl_r", dr[3] / b2v, 0.104, "table at alpha 1.2°");
    compare("Cn_r", dr[5] / b2v, -0.0937, "table");
    compare("CY_r", dr[2] / b2v, 0.226, "table at alpha 1.2°");
    compare("Cl full aileron", dail[3], 0.070, "table 0.229/rad x 17.5°");
    compare(
        "Cn full aileron",
        dail[5],
        -0.0016,
        "table -0.0053/rad x 17.5°",
    );
    compare(
        "Cn full rudder",
        drud[5],
        0.0130,
        "table 0.043/rad x 16° x 1.08 slipstream",
    );
    compare("Cl full rudder", drud[3], -0.0041, "table 0.0147/rad x 16°");
    compare("CY full rudder", drud[2], -0.0522, "table 0.187/rad x 16°");
}

const DT: f64 = 1.0 / 240.0;

/// An aircraft flying trimmed and level at a calibrated airspeed, with the downwash settled.
fn flying(kcas: f64, altitude: f64, flap: f64) -> (Aircraft, Environment) {
    let sigma = atmosphere::density(altitude) / atmosphere::SEA_LEVEL_DENSITY;
    let tas = kcas * KNOTS / sigma.sqrt();
    let mut a = Aircraft::new();
    let t = a.trim(tas, altitude, flap, None, 0.0).expect("trim");
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(tas * t.alpha.cos(), 0.0, tas * t.alpha.sin()),
        q: quaternion(0.0, t.alpha, 0.0),
        prop_speed: t.prop_speed,
        ..State::default()
    };
    a.flap_deg = flap;
    a.controls.flap_notch = (flap / 10.0).round() as i32;
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    let env = Environment {
        rho: atmosphere::density(altitude),
        ..Environment::default()
    };
    for _ in 0..240 {
        a.step(DT, &env);
    }
    (a, env)
}

/// Yoke movement that moves the elevator by `deg` (trailing edge up positive) from where it is.
fn elevator_for(a: &Aircraft, deg: f64) -> f64 {
    let e = a.controls.elevator;
    let surface = if e >= 0.0 {
        e * a.p.elevator_up
    } else {
        e * a.p.elevator_down
    } + deg * DEG;
    if surface >= 0.0 {
        surface / a.p.elevator_up - e
    } else {
        surface / a.p.elevator_down - e
    }
}

/// Successive extremes (time, value) of a signal sampled every `dt`.
fn extremes(samples: &[f64], dt: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for i in 1..samples.len() - 1 {
        let (a, b, c) = (samples[i - 1], samples[i], samples[i + 1]);
        if (b > a && b >= c) || (b < a && b <= c) {
            out.push((i as f64 * dt, b));
        }
    }
    out
}

/// Period and damping ratio from the first extremes of a decaying oscillation about `mean`.
fn mode(ex: &[(f64, f64)], mean: f64, count: usize) -> (f64, f64) {
    let n = count.min(ex.len());
    let period = 2.0 * (ex[n - 1].0 - ex[0].0) / (n - 1) as f64;
    // Ratio of swings a whole period apart.
    let d = ((ex[0].1 - mean).abs() / (ex[2].1 - mean).abs()).ln();
    let pi = std::f64::consts::PI;
    (period, d / (4.0 * pi * pi + d * d).sqrt())
}

#[test]
fn responses_at_100_kt() {
    // Elevator step of 1.15° up, held.
    {
        let (mut a, env) = flying(100.0, 1000.0, 0.0);
        let alpha0 = a.data().alpha;
        a.controls.elevator += elevator_for(&a, 1.15);
        let (mut alpha, mut nz_peak, mut q15) = (Vec::new(), 0.0_f64, 0.0);
        for i in 0..240 * 2 {
            a.step(DT, &env);
            alpha.push((a.data().alpha - alpha0) / DEG);
            nz_peak = nz_peak.max(a.data().load_factor() - 1.0);
            if i == 360 {
                q15 = a.state.omega.y / DEG;
            }
        }
        let settled = alpha[359];
        let t90 = alpha.iter().position(|x| *x > 0.9 * settled).unwrap() as f64 * DT;
        let peak = alpha.iter().fold(0.0_f64, |m, x| m.max(*x));
        compare("elevator 1.15°: alpha change, °", settled, 0.735, "run");
        compare("elevator 1.15°: pitch rate at 1.5 s, °/s", q15, 1.53, "run");
        compare(
            "elevator 1.15°: peak load factor - 1",
            nz_peak,
            0.170,
            "run",
        );
        compare("short period: alpha to 90%, s", t90, 0.36, "run");
        compare(
            "short period: overshoot, %",
            (peak / settled - 1.0) * 100.0,
            7.0,
            "run",
        );
    }
    // Phugoid after an elevator pulse.
    {
        let (mut a, env) = flying(100.0, 1000.0, 0.0);
        let e0 = a.controls.elevator;
        let mut ias = Vec::new();
        for i in 0..240 * 150 {
            a.controls.elevator = if i < 480 { e0 + 0.1 } else { e0 };
            a.step(DT, &env);
            if i >= 480 && i % 24 == 0 {
                ias.push(a.data().ias / KNOTS);
            }
        }
        let ex = extremes(&ias, 0.1);
        let (period, zeta) = mode(&ex, 100.0, 7);
        compare("phugoid period, s", period, 25.7, "run");
        compare("phugoid damping ratio", zeta, 0.098, "run");
    }
    // Aileron step of 4.24° and full aileron.
    {
        let (mut a, env) = flying(100.0, 1000.0, 0.0);
        a.controls.aileron = 0.3;
        let mut p = Vec::new();
        let mut beta1 = 0.0;
        for i in 0..240 * 2 {
            a.step(DT, &env);
            p.push(a.state.omega.x / DEG);
            if i == 240 {
                beta1 = a.data().beta / DEG;
            }
        }
        let peak = p.iter().fold(0.0_f64, |m, x| m.max(*x));
        let t63 = p.iter().position(|x| *x > 0.63 * peak).unwrap() as f64 * DT;
        compare("aileron 0.3: peak roll rate, °/s", peak, 18.2, "run");
        compare("roll: time to 63% of peak, s", t63, 0.085, "run");
        compare("aileron 0.3: sideslip after 1 s, °", beta1, 2.51, "run");
        let (mut a, env) = flying(100.0, 1000.0, 0.0);
        a.controls.aileron = 1.0;
        let mut peak = 0.0_f64;
        for _ in 0..240 {
            a.step(DT, &env);
            peak = peak.max(a.state.omega.x / DEG);
        }
        compare(
            "full aileron: peak roll rate, °/s",
            peak,
            62.4,
            "run, 15° of aileron",
        );
    }
    // Rudder: 8° for one second, then released; and 8° held.
    {
        let (mut a, env) = flying(100.0, 1000.0, 0.0);
        let mut beta = Vec::new();
        for i in 0..240 * 12 {
            a.controls.rudder = if i < 240 { 0.5 } else { 0.0 };
            a.step(DT, &env);
            if i % 4 == 0 {
                beta.push(-a.data().beta / DEG);
            }
        }
        let ex = extremes(&beta, 1.0 / 60.0);
        let (period, zeta) = mode(&ex, 0.0, 4);
        compare(
            "rudder 8° for 1 s: first swing of slip, °",
            ex[0].1,
            9.03,
            "run",
        );
        compare("dutch roll period, s", period, 2.39, "run");
        compare("dutch roll damping ratio", zeta, 0.21, "run");
        let (mut a, env) = flying(100.0, 1000.0, 0.0);
        a.controls.rudder = 0.5;
        let mut mean = 0.0;
        for i in 0..240 * 4 {
            a.step(DT, &env);
            if i >= 480 {
                mean += -a.data().beta / DEG / 480.0;
            }
        }
        compare("rudder 8° held: slip over 2..4 s, °", mean, 5.4, "run");
    }
}

/// Flies hands-off (ailerons only keeping the wings level) for three minutes after `change` and
/// returns the mean calibrated airspeed (kt), pitch (°) and climb rate (ft/min) of the last two,
/// and the lowest and highest airspeed of the first 12 s.
fn hands_off(kcas: f64, flap: f64, change: impl Fn(&mut Aircraft)) -> [f64; 5] {
    let (mut a, env) = flying(kcas, 1000.0, flap);
    change(&mut a);
    let (mut v, mut pitch, mut n) = (0.0, 0.0, 0.0);
    let (mut lo, mut hi) = (f64::INFINITY, 0.0_f64);
    let mut h0 = 0.0;
    for i in 0..240 * 180 {
        let e = euler_angles(a.state.q);
        a.controls.aileron = clamp(-e.roll * 2.0 - a.state.omega.x, -1.0, 1.0);
        a.step(DT, &env);
        let ias = a.data().ias / KNOTS;
        if i < 240 * 12 {
            lo = lo.min(ias);
            hi = hi.max(ias);
        }
        if i == 240 * 60 {
            h0 = -a.state.pos.z;
        }
        if i >= 240 * 60 {
            v += ias;
            pitch += e.pitch / DEG;
            n += 1.0;
        }
    }
    let climb = (-a.state.pos.z - h0) / 120.0 / FEET * 60.0;
    [v / n, pitch / n, climb, lo, hi]
}

#[test]
fn trim_changes_hands_off() {
    let r = hands_off(80.0, 0.0, |a| a.controls.flap_notch = 3);
    compare("flaps 0 to 30 at 80 kt: new speed, kt", r[0], 75.4, "run");
    compare("flaps 0 to 30: pitch, °", r[1], -0.47, "run");
    compare("flaps 0 to 30: climb, ft/min", r[2], -197.0, "run");
    compare("flaps 0 to 30: lowest speed in 12 s", r[3], 70.8, "run");
    let r = hands_off(80.0, 0.0, |a| a.controls.flap_notch = 1);
    compare("flaps 0 to 10 at 80 kt: new speed, kt", r[0], 77.6, "run");
    let r = hands_off(65.0, 30.0, |a| a.controls.flap_notch = 0);
    compare(
        "flaps 30 to 0 at 65 kt: new speed, kt",
        r[0],
        67.4,
        "run, first cycles",
    );
    let r = hands_off(90.0, 0.0, |a| a.controls.throttle = 0.0);
    compare("idle from 90 kt: new speed, kt", r[0], 92.5, "run");
    compare("idle from 90 kt: climb, ft/min", r[2], -970.0, "run");
    let r = hands_off(90.0, 0.0, |a| a.controls.throttle = 1.0);
    compare(
        "full throttle from 90 kt: new speed, kt",
        r[0],
        126.3,
        "run",
    );
    compare(
        "full throttle from 90 kt: climb, ft/min",
        r[2],
        -198.0,
        "run",
    );
}

#[test]
fn ground_effect() {
    // Lift and drag at 5° of alpha and 65 kt, flaps up, with the wing at a height above flat
    // ground given as a share of the span, against the same far from it.
    let a = Aircraft::new();
    let at = |wing_height: f64| {
        let env = Environment {
            ground_height: 0.0,
            ..Environment::default()
        };
        let (v, alpha) = (65.0 * KNOTS, 5.0 * DEG);
        let s = State {
            pos: V3::new(0.0, 0.0, -(wing_height + a.p.wing_pos.z)),
            vel: V3::new(v * alpha.cos(), 0.0, v * alpha.sin()),
            q: quaternion(0.0, alpha, 0.0),
            prop_speed: 200.0,
            ..State::default()
        };
        let d = a.forces_settled(&s, &AircraftControls::default(), &env, 0.0);
        let f = d.specific * a.mass() - V3::new(d.thrust, 0.0, 0.0);
        let q = 0.5 * env.rho * v * v * S;
        let (sa, ca) = alpha.sin_cos();
        ((-f.z * ca + f.x * sa) / q, -(f.x * ca + f.z * sa) / q)
    };
    let (cl_far, cd_far) = at(500.0);
    // JSBSim: kCLge and kCDge (Wieselsberger). Ours applies the drag factor to the induced drag
    // at the lift the wing actually has, so it is compared as drag per lift squared.
    for (hb, k_cl, k_cd) in [
        (0.19, 1.076, 0.693),
        (0.2, 1.073, 0.709),
        (0.3, 1.046, 0.815),
        (0.5, 1.019, 0.928),
    ] {
        let (cl, cd) = at(hb * B);
        compare(
            &format!("wing at {hb} span: lift factor"),
            cl / cl_far,
            k_cl,
            "table kCLge",
        );
        let cd0 = cd_far - a.p.induced_k * cl_far * cl_far;
        let induced = (cd - cd0) / (cl * cl) / a.p.induced_k;
        compare(
            &format!("wing at {hb} span: induced drag factor"),
            induced,
            k_cd,
            "table kCDge",
        );
    }
}

#[test]
fn trim_elevator() {
    // Elevator angle (trailing edge down positive) for level flight, power for level flight.
    let a = Aircraft::new();
    let sigma = (atmosphere::density(1000.0) / atmosphere::SEA_LEVEL_DENSITY).sqrt();
    let elevator = |kcas: f64, flap: f64| {
        let tr = a
            .trim(kcas * KNOTS / sigma, 1000.0, flap, None, 0.0)
            .or_else(|| a.trim(kcas * KNOTS / sigma, 1000.0, flap, None, -3.0 * DEG))
            .expect("trim");
        let e = tr.elevator;
        (
            -(if e >= 0.0 {
                e * a.p.elevator_up
            } else {
                e * a.p.elevator_down
            }) / DEG,
            tr.alpha / DEG,
        )
    };
    let (e60, a60) = elevator(60.0, 0.0);
    let (e120, a120) = elevator(120.0, 0.0);
    compare(
        "elevator from 60 to 120 kt, °",
        e120 - e60,
        10.64,
        "run: -5.00 to +5.64",
    );
    compare(
        "alpha from 60 to 120 kt, °",
        a120 - a60,
        -9.60,
        "run: 9.37 to -0.24",
    );
    for (kcas, clean, f10, f30) in [
        (60.0, -5.00, -3.62, -3.15),
        (70.0, -1.61, -1.31, -0.70),
        (80.0, 0.36, f64::NAN, 1.46),
    ] {
        let e0 = elevator(kcas, 0.0).0;
        if f10.is_finite() {
            compare(
                &format!("flaps 10 at {kcas:.0} kt: elevator change, °"),
                elevator(kcas, 10.0).0 - e0,
                f10 - clean,
                "run",
            );
        }
        compare(
            &format!("flaps 30 at {kcas:.0} kt: elevator change, °"),
            elevator(kcas, 30.0).0 - e0,
            f30 - clean,
            "run",
        );
        compare(
            &format!("flaps 30 at {kcas:.0} kt: alpha change, °"),
            elevator(kcas, 30.0).1 - elevator(kcas, 0.0).1,
            if kcas == 60.0 {
                -4.14
            } else if kcas == 70.0 {
                -3.44
            } else {
                -3.81
            },
            "run",
        );
    }
}

/// How far each quantity may be from the reference, as a fraction of it. Quantities that are
/// not listed are printed but not checked: see `REPORT_ONLY`.
fn tolerance(what: &str) -> f64 {
    const TOLERANCES: &[(&str, f64)] = &[
        ("CL_alpha", 0.10),
        // Ours is between JSBSim's -1.8 and the -0.89 of Roskam's Cessna 172 data.
        ("Cm_alpha", 0.40),
        ("Cm_q", 0.10),
        // The lift of the tail alone; JSBSim's figures include the wing's share.
        ("CL_q", 0.45),
        ("Cm_de", 0.25),
        ("CL_de", 0.35),
        ("CY_beta", 0.25),
        ("Cl_beta", 0.15),
        ("Cn_beta", 0.10),
        ("Cl_p", 0.10),
        ("Cn_p", 0.60),
        ("CY_p", 0.60),
        ("Cl_r", 0.15),
        ("Cn_r", 0.15),
        ("CY_r", 0.15),
        ("Cl full aileron", 0.30),
        ("Cn full aileron", 0.60),
        ("Cn full rudder", 0.15),
        ("Cl full rudder", 0.45),
        ("CY full rudder", 0.50),
        ("elevator 1.15°", 0.10),
        ("short period: alpha", 0.25),
        ("short period: overshoot", 0.30),
        ("phugoid period", 0.20),
        ("phugoid damping", 0.15),
        ("aileron 0.3: peak", 0.10),
        ("roll: time", 0.15),
        ("aileron 0.3: sideslip", 0.40),
        ("full aileron", 0.10),
        ("rudder 8° for 1 s", 0.15),
        ("dutch roll period", 0.10),
        ("dutch roll damping", 0.25),
        ("rudder 8° held", 0.15),
        ("flaps 0 to 30 at 80 kt: new speed", 0.05),
        ("flaps 0 to 30: lowest", 0.08),
        ("flaps 0 to 10", 0.05),
        ("flaps 30 to 0", 0.10),
        ("idle from 90 kt: new", 0.05),
        ("idle from 90 kt: climb", 0.20),
        ("elevator from 60", 0.20),
        ("alpha from 60", 0.10),
        ("wing at", 0.03),
    ];
    // Printed for the record, not checked:
    // - flaps 0 to 30, pitch and climb: our flaps make more lift and drag for the same angle
    //   (the handbook's 48 kt stall and steep full-flap glide need them), so the nose sits lower.
    // - flaps at a fixed speed, elevator and alpha change: same reason.
    // - full throttle hands-off: JSBSim's elevator gains power from the slipstream and pushes the
    //   nose down until it does 126 kt at 2930 rpm, with an engine that gives 220 hp at sea
    //   level. A 172 raises its nose and climbs near its trimmed speed, as ours does.
    const REPORT_ONLY: &[&str] = &[
        "flaps 0 to 30: pitch",
        "flaps 0 to 30: climb",
        "flaps 10 at",
        "flaps 30 at",
        "full throttle",
    ];
    if let Some((_, t)) = TOLERANCES.iter().find(|(k, _)| what.trim().starts_with(k)) {
        return *t;
    }
    assert!(
        REPORT_ONLY.iter().any(|k| what.starts_with(k)),
        "no tolerance for {what}"
    );
    f64::INFINITY
}

#[test]
fn power_change_hands_off() {
    // What a 172 does (and JSBSim's does not, see `tolerance`): with full power from level
    // flight at 90 kt it climbs, a little slower than it was trimmed for.
    let r = hands_off(90.0, 0.0, |a| a.controls.throttle = 1.0);
    assert!(r[0] > 80.0 && r[0] < 92.0, "speed {}", r[0]);
    assert!(r[2] > 300.0, "climb {}", r[2]);
}
