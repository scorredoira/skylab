//! Prints the Boeing 747's figures as the flight model gives them, next to the published ones:
//! stability derivatives, trims, stalls, climb and cruise. For tuning its file
//! (`wip/b747/aircraft.ron`); `tests/b747.rs` keeps them where they should be.
//!
//! `cargo run --release -p sim-core --example b747`

use sim_core::*;

const FPM: f64 = 60.0 / FEET;

/// Force and moment coefficients (body axes: X, Y, Z, l, m, n) in a flight condition.
#[allow(clippy::too_many_arguments)]
fn coefficients(a: &Aircraft, tas: f64, altitude: f64, alpha: f64, beta: f64, omega: V3, c: &AircraftControls, flap_deg: f64) -> [f64; 6] {
    let env = Environment::standard(altitude);
    let mut s = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(tas * alpha.cos() * beta.cos(), tas * beta.sin(), tas * alpha.sin() * beta.cos()),
        q: quaternion(0.0, alpha, 0.0),
        omega,
        ..State::default()
    };
    // Engines at idle thrust would still push: leave them out by putting them at the speed that gives none.
    s.n1 = [0.0; MOST_ENGINES];
    let d = a.forces_settled(&s, c, &env, flap_deg);
    let q = 0.5 * env.rho * tas * tas;
    let (area, span, chord) = (a.p.wing_area, a.p.span, a.p.chord);
    let f = d.specific * a.mass();
    [f.x / (q * area), f.y / (q * area), f.z / (q * area), d.moment.x / (q * area * span), d.moment.y / (q * area * chord), d.moment.z / (q * area * span)]
}

fn derivatives(a: &Aircraft, name: &str, mach: f64, altitude: f64, flap_deg: f64, alpha0: f64) {
    let tas = mach * atmosphere::Atmosphere::STANDARD.speed_of_sound(altitude);
    let c0 = AircraftControls::default();
    let at = |alpha: f64, beta: f64, omega: V3, c: &AircraftControls| coefficients(a, tas, altitude, alpha, beta, omega, c, flap_deg);
    let h = 0.5 * DEG;
    let base = at(alpha0, 0.0, V3::ZERO, &c0);
    let d = |plus: [f64; 6], minus: [f64; 6], step: f64| -> [f64; 6] { std::array::from_fn(|i| (plus[i] - minus[i]) / (2.0 * step)) };
    let da = d(at(alpha0 + h, 0.0, V3::ZERO, &c0), at(alpha0 - h, 0.0, V3::ZERO, &c0), h);
    let db = d(at(alpha0, h, V3::ZERO, &c0), at(alpha0, -h, V3::ZERO, &c0), h);
    let rate = 0.02;
    let (b2v, c2v) = (a.p.span / (2.0 * tas), a.p.chord / (2.0 * tas));
    let dp = d(at(alpha0, 0.0, V3::new(rate, 0.0, 0.0), &c0), at(alpha0, 0.0, V3::new(-rate, 0.0, 0.0), &c0), rate * b2v);
    let dq = d(at(alpha0, 0.0, V3::new(0.0, rate, 0.0), &c0), at(alpha0, 0.0, V3::new(0.0, -rate, 0.0), &c0), rate * c2v);
    let dr = d(at(alpha0, 0.0, V3::new(0.0, 0.0, rate), &c0), at(alpha0, 0.0, V3::new(0.0, 0.0, -rate), &c0), rate * b2v);
    let with = |f: &dyn Fn(&mut AircraftControls)| {
        let mut c = c0;
        f(&mut c);
        at(alpha0, 0.0, V3::ZERO, &c)
    };
    let aileron = with(&|c| c.aileron = 1.0);
    let elevator = d(with(&|c| c.elevator = 0.2), with(&|c| c.elevator = -0.2), 0.2);
    let rudder = with(&|c| c.rudder = 1.0);
    println!("{name}: Mach {mach} at {altitude:.0} m, flaps {flap_deg}, alpha {:.1}°", alpha0 / DEG);
    println!("  CL {:.3}  CD {:.4}  Cm {:.3}", -base[2] * alpha0.cos() + base[0] * alpha0.sin(), -base[0] * alpha0.cos() - base[2] * alpha0.sin(), base[4]);
    println!("  CL_alpha {:.2}  Cm_alpha {:.2}  Cm_q {:.1}  CL_q {:.1}", -da[2], da[4], dq[4], -dq[2]);
    println!("  CY_beta {:.2}  Cl_beta {:.3}  Cn_beta {:.3}", db[1], db[3], db[5]);
    println!("  Cl_p {:.3}  Cn_p {:.3}  Cl_r {:.3}  Cn_r {:.3}", dp[3], dp[5], dr[3], dr[5]);
    println!("  full aileron Cl {:.4} Cn {:.4}   elevator Cm per travel {:.3}   full rudder Cn {:.4} Cl {:.4}", aileron[3] - base[3], aileron[5] - base[5], elevator[4], rudder[5] - base[5], rudder[3] - base[3]);
}

fn main() {
    let file = ("wip/b747/aircraft.ron".to_string(), include_str!("../../../../wip/b747/aircraft.ron").to_string());
    sim_core::aircraft::file::set_fleet(sim_core::aircraft::file::fleet_with(&[file]).unwrap()).unwrap();
    let mut a = Aircraft::of(AircraftKind::parse("b747").expect("the fleet has the Boeing 747"));
    // The bare airframe: the yaw damper off.
    let damper = a.p.yaw_damper.take();
    println!("mass {:.0} kg, flapped share {:.3}, strips {:?}", a.mass(), a.strips().iter().filter(|s| s.flapped).map(|s| s.weight).sum::<f64>(), a.strips().iter().map(|s| (s.y, s.weight, s.x)).collect::<Vec<_>>());
    println!("\nPublished (CR-2144): Mach 0.25 at sea level, landing: CL_alpha 5.70, Cm_alpha -1.26, Cm_q -20.8, CY_beta -0.96, Cl_beta -0.221, Cn_beta 0.150, Cl_p -0.45, Cn_p -0.121, Cl_r 0.101, Cn_r -0.30, full wheel Cl 0.064, Cm_de -1.34/rad, Cn_dr -0.109/rad");
    derivatives(&a, "approach", 0.25, 0.0, 30.0, 5.5 * DEG);
    derivatives(&a, "clean", 0.35, 0.0, 0.0, 4.0 * DEG);
    println!("\nPublished: Mach 0.8 at 40,000 ft: CL 0.654, CD 0.043, CL_alpha 4.92, Cm_alpha -1.03, Cm_q -23.9, CY_beta -0.88, Cl_beta -0.28, Cn_beta 0.195, Cl_p -0.33, Cn_p -0.04, Cl_r 0.30, Cn_r -0.27");
    derivatives(&a, "cruise", 0.8, 12192.0, 0.0, 3.0 * DEG);
    println!("\nPublished: Mach 0.9 at 40,000 ft: CL 0.5, CD 0.042, CL_alpha 5.5, Cm_alpha -1.6, Cm_q -25, CY_beta -0.85, Cl_beta -0.10, Cn_beta 0.20, Cl_p -0.30, Cn_r -0.325");
    derivatives(&a, "fast", 0.9, 12192.0, 0.0, 1.5 * DEG);
    a.p.yaw_damper = damper;

    println!("\nStalls at {:.0} kg (published: about 170 kt clean, 108 kt with flaps 30)", a.mass());
    for &flaps in a.p.flap_notches {
        let s = a.stall(flaps, 0.0);
        println!("  flaps {flaps:>4}: CLmax {:.2}, {:.0} kt", s.cl_max, s.speed / KNOTS);
    }

    println!("\nLevel flight");
    let level = |a: &mut Aircraft, name: &str, kt_or_mach: f64, altitude: f64, flaps: f64, gear: f64| {
        a.gear = gear;
        let sound = atmosphere::Atmosphere::STANDARD.speed_of_sound(altitude);
        let tas = if kt_or_mach < 2.0 { kt_or_mach * sound } else { kt_or_mach * KNOTS / (atmosphere::density(altitude) / atmosphere::SEA_LEVEL_DENSITY).sqrt() };
        match a.trim(tas, altitude, flaps, None, 0.0) {
            Some(t) => {
                let (d, _) = a.steady_state(tas, altitude, t.alpha, 0.0, if a.p.stabiliser_trim.is_some() { t.trim } else { t.elevator }, t.throttle, flaps);
                let weight = a.mass() * GRAVITY;
                println!(
                    "  {name}: pitch {:.1}°, throttle {:.2}, N1 {:.0}%, stabiliser trim {:.2}, thrust {:.0} kN, L/D {:.1}, fuel {:.1} t/h, Mach {:.2}",
                    t.alpha / DEG,
                    t.throttle,
                    t.n1[0] * 100.0,
                    t.trim,
                    d.thrust / 1000.0,
                    weight / d.thrust,
                    d.fuel_flow * 3.6,
                    d.mach
                );
            }
            None => println!("  {name}: cannot be trimmed"),
        }
    };
    level(&mut a, "250 kt at 3000 m, clean", 250.0, 3000.0, 0.0, 0.0);
    level(&mut a, "Mach 0.84 at 35,000 ft", 0.84, 10668.0, 0.0, 0.0);
    level(&mut a, "Mach 0.86 at 39,000 ft", 0.86, 11887.0, 0.0, 0.0);
    level(&mut a, "200 kt flaps 5", 200.0, 600.0, 5.0, 0.0);
    level(&mut a, "170 kt flaps 20, gear down", 170.0, 600.0, 20.0, 1.0);
    level(&mut a, "146 kt flaps 30, gear down", 146.0, 300.0, 30.0, 1.0);
    a.gear = 1.0;
    if let Some(t) = a.trim(146.0 * KNOTS, 300.0, 30.0, None, -3.0 * DEG) {
        println!("  on the glide path at 146 kt, flaps 30: pitch {:.1}°, N1 {:.0}%, trim {:.2}", (t.alpha - 3.0 * DEG) / DEG, t.n1[0] * 100.0, t.trim);
    }

    println!("\nClimb at full thrust");
    for (name, kt, altitude, flaps, gear) in [("flaps 10 at 165 kt, gear up", 165.0, 300.0, 10.0, 0.0), ("clean at 250 kt, 1500 m", 250.0, 1500.0, 0.0, 0.0), ("clean at 300 kt, 6000 m", 300.0, 6000.0, 0.0, 0.0), ("Mach 0.84 at 10,000 m", 0.0, 10000.0, 0.0, 0.0)] {
        a.gear = gear;
        let tas = if kt == 0.0 { 0.84 * 299.5 } else { kt * KNOTS / (atmosphere::density(altitude) / atmosphere::SEA_LEVEL_DENSITY).sqrt() };
        match a.trim(tas, altitude, flaps, Some(1.0), 0.0) {
            Some(t) => println!("  {name}: {:.0} ft/min, pitch {:.1}°, N1 {:.0}%", tas * t.climb_angle.sin() * FPM, (t.alpha + t.climb_angle) / DEG, t.n1[0] * 100.0),
            None => println!("  {name}: cannot be trimmed"),
        }
    }
}
