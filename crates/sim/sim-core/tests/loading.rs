//! Weight, centre of gravity and fuel: what changes when the aircraft is loaded differently.

mod common;

use common::assert_close;
use sim_core::*;

/// Fuselage station of the centre of gravity, inches (the reference loading is at 43.6).
fn station(a: &Aircraft) -> f64 {
    43.6 - a.cg().x / 0.0254
}

#[test]
fn reference_loading_is_the_handbook_gross_weight() {
    let a = Aircraft::new();
    assert_close(a.mass() / 0.45359, 2300.0, 1.0, "gross weight, lb");
    assert_close(station(&a), 43.6, 1e-9, "centre of gravity, in");
    // JSBSim c172p: 948, 1346 and 1967 slug·ft².
    let slug_ft2 = 1.35582;
    assert_close(a.inertia().x / slug_ft2, 948.0, 1.0, "Ixx");
    assert_close(a.inertia().y / slug_ft2, 1346.0, 1.0, "Iyy");
    assert_close(a.inertia().z / slug_ft2, 1967.0, 1.0, "Izz");
}

#[test]
fn lighter_stalls_slower_and_climbs_faster() {
    let heavy = Aircraft::new();
    let mut light = Aircraft::new();
    // Pilot alone with half tanks: about 1755 lb, with the CG 4.6 in further forward.
    light.set_load(Load {
        front: 77.0,
        rear: 0.0,
        baggage: 0.0,
        fuel: 54.0,
    });
    let ratio = (light.mass() / heavy.mass()).sqrt();
    println!(
        "light: {:.0} lb, CG at {:.1} in, inertia {:?}",
        light.mass() / 0.45359,
        station(&light),
        light.inertia()
    );
    // The handbook's envelope at this weight runs from 35 to 47.3 in.
    assert!(station(&light) > 35.0 && station(&light) < 47.3);
    let (vh, vl) = (heavy.stall(0.0, 0.0).speed, light.stall(0.0, 0.0).speed);
    println!(
        "stall {:.1} kt at gross, {:.1} kt light",
        vh / KNOTS,
        vl / KNOTS
    );
    // Stall speed goes with the square root of the weight; the forward CG costs a little.
    assert_close(vl / vh, ratio, 0.025, "stall speed ratio");
    let climb = |a: &Aircraft| {
        let v = 73.0 * KNOTS;
        v * a
            .trim(v, 0.0, 0.0, Some(1.0), 0.0)
            .unwrap()
            .climb_angle
            .sin()
            / FEET
            * 60.0
    };
    let (ch, cl) = (climb(&heavy), climb(&light));
    println!("climb {ch:.0} ft/min at gross, {cl:.0} ft/min light");
    // Excess power is nearly the same, so the rate goes roughly as 1/weight, and better.
    assert!(cl > ch * heavy.mass() / light.mass());
    assert!(cl < ch * 1.6);
}

#[test]
fn centre_of_gravity_moves_stability_and_trim() {
    // Same weight, the load moved: forward limit 38.5 in and aft limit 47.3 in at 2300 lb.
    let load_at = |rear: f64, baggage: f64| {
        let mut a = Aircraft::new();
        let r = AircraftType::c172().reference_load;
        a.set_load(Load {
            front: r.front + (r.rear - rear) + (r.baggage - baggage),
            rear,
            baggage,
            fuel: r.fuel,
        });
        a
    };
    let forward = load_at(0.0, 0.0);
    let aft = load_at(120.0, 54.0);
    println!(
        "forward {:.1} in, aft {:.1} in",
        station(&forward),
        station(&aft)
    );
    assert_close(forward.mass(), aft.mass(), 1e-9, "same weight");
    assert!(station(&forward) < 40.5 && station(&aft) > 46.0);
    // Pitch stiffness: moment change for a degree of alpha at 100 kt, elevator fixed.
    let stiffness = |a: &Aircraft| {
        let t = a.trim_level(100.0 * KNOTS, 1000.0).unwrap();
        let m = |alpha: f64| {
            a.steady(
                100.0 * KNOTS,
                1000.0,
                alpha,
                0.0,
                t.elevator,
                t.throttle,
                0.0,
            )
            .0
            .moment
            .y
        };
        let q = 0.5 * atmosphere::density(1000.0) * (100.0 * KNOTS).powi(2);
        (m(t.alpha + 0.01) - m(t.alpha - 0.01)) / 0.02 / (q * a.p.wing_area * a.p.chord)
    };
    let (cf, cr, ca) = (
        stiffness(&forward),
        stiffness(&Aircraft::new()),
        stiffness(&aft),
    );
    println!("Cm_alpha forward {cf:.2}, reference {cr:.2}, aft {ca:.2} /rad");
    // Stable everywhere in the envelope, less so aft. The shift per inch is the lift slope times
    // the distance over the chord (JSBSim transfers its moments to the CG the same way):
    // 5.1 x 0.0254 / 1.49 = 0.087 /rad per inch.
    assert!(cf < cr && cr < ca && ca < -0.4);
    let per_inch = (ca - cf) / (station(&aft) - station(&forward));
    assert_close(per_inch, 0.087, 0.02, "Cm_alpha per inch of CG");
    // Nose-heavy needs more back elevator to fly slowly.
    let slow = |a: &Aircraft| a.trim_level(60.0 * KNOTS, 1000.0).unwrap().elevator;
    assert!(slow(&forward) > slow(&aft) + 0.03);
    // And the stall is a little faster: the tail pushes down harder.
    assert!(forward.stall(0.0, 0.0).speed > aft.stall(0.0, 0.0).speed);
}

#[test]
fn fuel_burns_at_the_handbook_rate_and_runs_out() {
    let mut a = Aircraft::new();
    // 75% power is 8.4 gal/h and 55% is 6.3 gal/h in the handbook's cruise table.
    let gph = |power: f64| a.fuel_flow(power * 119_300.0) * 3600.0 / 0.45359 / 6.0;
    assert_close(gph(0.75), 8.4, 0.1, "fuel flow at 75%");
    assert_close(gph(0.55), 6.3, 0.1, "fuel flow at 55%");

    // Ten minutes of cruise at 8000 ft and about 2500 rpm.
    let altitude = 8000.0 * FEET;
    let t = a.trim_level(111.0 * KNOTS, altitude).unwrap();
    a.state = State {
        pos: V3::new(0.0, 0.0, -altitude),
        vel: V3::new(
            111.0 * KNOTS * t.alpha.cos(),
            0.0,
            111.0 * KNOTS * t.alpha.sin(),
        ),
        q: quaternion(0.0, t.alpha, 0.0),
        prop_speed: t.prop_speed,
        ..State::default()
    };
    a.controls.elevator = t.elevator;
    a.controls.throttle = t.throttle;
    let env = Environment {
        rho: atmosphere::density(altitude),
        ..Environment::default()
    };
    let (fuel0, mass0) = (a.load().fuel, a.mass());
    for _ in 0..240 * 600 {
        let e = euler_angles(a.state.q);
        a.controls.aileron = clamp(-e.roll * 2.0 - a.state.omega.x, -1.0, 1.0);
        a.step(1.0 / 240.0, &env);
    }
    let burnt = fuel0 - a.load().fuel;
    let rate = burnt * 6.0 / 0.45359 / 6.0;
    println!("cruise at 8000 ft, {:.0} rpm: {rate:.1} gal/h", a.rpm());
    // Handbook, 8000 ft and 2500 rpm: 7.1 gal/h.
    assert_close(rate, 7.1, 0.6, "cruise fuel flow");
    assert_close(mass0 - a.mass(), burnt, 1e-9, "the aircraft gets lighter");

    // The last drops: the engine stops and the propeller windmills.
    let mut load = a.load();
    load.fuel = 0.01;
    a.set_load(load);
    for _ in 0..240 * 30 {
        let e = euler_angles(a.state.q);
        a.controls.aileron = clamp(-e.roll * 2.0 - a.state.omega.x, -1.0, 1.0);
        a.step(1.0 / 240.0, &env);
    }
    println!(
        "out of fuel: {:.0} rpm, thrust {:.0} N",
        a.rpm(),
        a.data().thrust
    );
    assert_eq!(a.load().fuel, 0.0);
    assert!(a.data().thrust < 0.0 && a.data().engine_torque < 0.0);
    assert!(a.rpm() < 1500.0);
}
