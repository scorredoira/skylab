//! The GT coupé (`assets/cars/gt/car.ron`) against the figures road tests give for its class
//! (rear-drive sports coupés of some 1400 kg and 350 to 400 hp): 0-100 km/h in 4.3 to 5 s,
//! 100-0 km/h in 33 to 37 m with anti-lock brakes, a top speed of 270 to 295 km/h, 0.95 to 1.05 g
//! on the skidpad; and its stability: still at rest, parked on a slope with the hand brake, no
//! energy from nowhere, walls it bounces off or is wrecked by.
//! `cargo test -p sim-core --test car -- --nocapture` prints the measurements.

use sim_core::aircraft::Environment;
use sim_core::car::{Car, CarKind, Plane, Surface, Wall, WallKind};
use sim_core::{Body, World, GRAVITY, V3};

const DT: f64 = World::DT;

fn air() -> Environment {
    Environment { ground_height: 0.0, ..Environment::default() }
}

fn car_on(plane: &Plane, heading: f64) -> Car {
    let mut car = Car::of(CarKind::GT);
    car.stand(plane, 0.0, 0.0, heading);
    car
}

fn run(car: &mut Car, plane: &Plane, seconds: f64) {
    for _ in 0..(seconds / DT).round() as usize {
        car.step(DT, &air(), plane);
    }
}

/// Sets a car going straight at `speed` (m/s), wheels and engine turning with it.
fn going(car: &mut Car, speed: f64) {
    let b = car.body;
    car.put(Body { vel: V3::new(speed, 0.0, 0.0), ..b });
}

#[test]
fn file_reads_and_stands_level() {
    let car = Car::of(CarKind::GT);
    assert_eq!(car.p.key, "gt");
    let load: f64 = (0..2).map(|a| 2.0 * car.p.static_load(a)).sum();
    assert!((load - car.p.mass * GRAVITY).abs() < 1.0);
    println!("static loads: front {:.0} N, rear {:.0} N a wheel; hubs at z {:.3}, {:.3}", car.p.static_load(0), car.p.static_load(1), car.p.hub_z[0], car.p.hub_z[1]);
    for g in 1..=6 {
        println!("gear {g}: top {:.0} km/h", car.p.top_of_gear(g) * 3.6);
    }
}

#[test]
fn still_at_rest() {
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.3);
    let start = car.body.pos;
    run(&mut car, &plane, 2.0);
    let settled = car.body.pos;
    run(&mut car, &plane, 10.0);
    let b = car.body;
    let drift = (b.pos - settled).length();
    println!("at rest: settled {:.4} m from where it was put; then drifted {:.6} m in 10 s, speed {:.6} m/s, rates {:.6} rad/s", (settled - start).length(), drift, b.vel.length(), b.omega.length());
    assert!(drift < 0.002, "drifted {drift} m");
    assert!(b.vel.length() < 0.001 && b.omega.length() < 0.001, "still moving: {:?} {:?}", b.vel, b.omega);
    // It sits as high as it should, the tyres squashed by the weight.
    assert!((-b.pos.z - car.p.cg_height).abs() < 0.02, "centre of gravity {:.3} m up", -b.pos.z);
}

#[test]
fn parks_on_a_slope_with_the_hand_brake() {
    // 15 % uphill to the north; facing up and facing down.
    let plane = Plane { rise: (0.15, 0.0), ..Plane::level(Surface::Asphalt) };
    for heading in [0.0, std::f64::consts::PI] {
        let mut car = car_on(&plane, heading);
        car.set_gear(0);
        car.controls.handbrake = 1.0;
        run(&mut car, &plane, 2.0);
        let settled = car.body.pos;
        run(&mut car, &plane, 20.0);
        let moved = (car.body.pos - settled).length();
        println!("15 % slope, facing {}: moved {:.4} m in 20 s", if heading == 0.0 { "up" } else { "down" }, moved);
        assert!(moved < 0.01, "slid {moved} m");
    }
    // Without the hand brake, in neutral, it rolls away.
    let mut car = car_on(&plane, 0.0);
    car.set_gear(0);
    run(&mut car, &plane, 5.0);
    assert!(car.body.vel.x < -2.0, "rolls back: {:?}", car.body.vel);
}

/// Seconds from rest to `kmh` at full throttle (the automatic, the clutch by itself).
fn sprint(car: &mut Car, plane: &Plane, kmh: f64) -> f64 {
    car.controls.throttle = 1.0;
    let mut t = 0.0;
    while car.body.vel.x < kmh / 3.6 && t < 60.0 {
        car.step(DT, &air(), plane);
        t += DT;
    }
    t
}

#[test]
fn zero_to_hundred() {
    // With traction control, as the road tests launch (with launch or traction control).
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    car.controls.traction_control = true;
    let t = sprint(&mut car, &plane, 100.0);
    let t200 = t + sprint(&mut car, &plane, 200.0);
    println!("0-100 km/h {t:.2} s (class: 4.3 to 5); 0-200 km/h {t200:.1} s; in gear {}", car.gear());
    assert!((4.2..=5.1).contains(&t), "0-100 in {t:.2} s");
}

#[test]
fn top_speed() {
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    going(&mut car, 60.0);
    car.set_gear(4);
    car.controls.throttle = 1.0;
    let mut best = 0.0f64;
    for _ in 0..(90.0 / DT) as usize {
        car.step(DT, &air(), &plane);
        best = best.max(car.body.vel.x);
    }
    println!("top speed {:.0} km/h in gear {} at {:.0} rpm (class: 270 to 295)", best * 3.6, car.gear(), car.rpm());
    assert!((270.0..=295.0).contains(&(best * 3.6)), "top speed {:.0} km/h", best * 3.6);
}

/// Metres from `kmh` to rest at full brake.
fn stop(abs: bool, kmh: f64) -> f64 {
    stop_on(&Plane::level(Surface::Asphalt), abs, kmh)
}

/// Metres from `kmh` to rest at full brake on a plane.
fn stop_on(plane: &Plane, abs: bool, kmh: f64) -> f64 {
    let plane = plane.clone();
    let mut car = car_on(&plane, 0.0);
    car.controls.abs = abs;
    going(&mut car, kmh / 3.6);
    car.set_gear(0);
    car.controls.brake = 1.0;
    let start = car.body.pos;
    let mut t = 0.0;
    while car.body.vel.x > 0.05 && t < 20.0 {
        car.step(DT, &air(), &plane);
        t += DT;
    }
    (car.body.pos - start).length()
}

#[test]
fn hundred_to_zero() {
    let with = stop(true, 100.0);
    let without = stop(false, 100.0);
    println!("100-0 km/h: {with:.1} m with anti-lock brakes (class: 33 to 37), {without:.1} m without (wheels locked)");
    assert!((32.0..=38.0).contains(&with), "{with:.1} m");
    assert!(without > with, "locked wheels stop sooner: {without:.1} m");
}

#[test]
fn skidpad() {
    let best = skidpad_on(&Plane::level(Surface::Asphalt));
    println!("skidpad: {:.3} g held a second (class: 0.95 to 1.05)", best / GRAVITY);
    assert!((0.93..=1.06).contains(&(best / GRAVITY)), "{:.3} g", best / GRAVITY);
}

/// The most lateral acceleration held a second on the skidpad, m/s².
fn skidpad_on(plane: &Plane) -> f64 {
    // A circle of 300 ft across (45.7 m of radius), as the magazines' skidpad: the speed raised
    // slowly while the steering keeps the car on it; the most lateral acceleration held a second.
    let plane = plane.clone();
    let radius = 45.72;
    let mut car = car_on(&plane, 0.0);
    // On the circle round (0, radius), going clockwise (turning right) from its west point.
    going(&mut car, 12.0);
    let mut target = 12.0;
    let mut best = 0.0f64;
    let mut held = Vec::new();
    for k in 0..(120.0 / DT) as usize {
        let b = car.body;
        let (dn, de) = (b.pos.x, b.pos.y - radius);
        let r = dn.hypot(de);
        let v = b.vel.x;
        // Steer for the circle's curvature, plus the error in radius and heading.
        let e = sim_core::euler_angles(b.q);
        let tangent = de.atan2(dn) + std::f64::consts::FRAC_PI_2;
        let heading_error = sim_core::remainder(tangent - e.heading, std::f64::consts::TAU);
        let wanted = 1.0 / radius + 0.02 * (r - radius) + 0.8 * heading_error / v.max(1.0);
        let steer = (wanted * car.p.wheelbase()).atan() + 0.0 + 0.6 * (wanted * v - b.omega.z) * 0.1;
        car.controls.steer = (steer / car.p.steering.lock_deg.to_radians() * 1.0).clamp(-1.0, 1.0);
        car.controls.automatic = true;
        // The speed raised by 0.15 m/s a second while it holds the line.
        if (r - radius).abs() < 1.0 {
            target += 0.12 * DT;
        } else {
            target -= 0.3 * DT;
        }
        car.controls.throttle = ((target - v) * 0.6 + 0.25).clamp(0.0, 1.0);
        car.step(DT, &air(), &plane);
        let lateral = car.body.vel.x * car.body.omega.z;
        if (r - radius).abs() < 1.0 {
            held.push(lateral);
        } else {
            held.clear();
        }
        if held.len() >= 240 {
            let mean = held[held.len() - 240..].iter().sum::<f64>() / 240.0;
            best = best.max(mean);
        }
        if k % 2400 == 0 {
            println!("  t {:.0} s: v {:.1} m/s, r {:.1} m, lateral {:.2} g, steer {:.2}", k as f64 * DT, v, r, lateral / GRAVITY, car.controls.steer);
        }
    }
    best
}

#[test]
fn wet_braking_and_skidpad() {
    // A film of water as rain leaves on the asphalt: 0.2 mm (a drizzle, the handbook's "wet"),
    // 0.5 mm (steady rain), 1 mm (a downpour). A road tyre keeps some 0.75 of its dry grip in a
    // thin film at any speed and loses more in deeper water the faster it goes
    // (`sim_core::car::wet`): the stop from 100 km/h grows from 35 m to some 45 in the wet (tyre
    // tests of summer tyres give 40 to 55 m), the skidpad's g falls by a quarter; in a downpour's
    // millimetre the tyres partly aquaplane above 70 km/h (the handbook keeps 0.375 of the dry
    // friction at 90 km/h there) and the stop is twice the dry one.
    let dry = stop(true, 100.0);
    let dry_g = skidpad_on(&Plane::level(Surface::Asphalt)) / GRAVITY;
    println!("dry: 100-0 km/h {dry:.1} m, skidpad {dry_g:.3} g");
    let mut last = dry;
    for (mm, stop_range, g_range) in [(0.2, 42.0..=50.0, 0.70..=0.85), (0.5, 44.0..=54.0, 0.68..=0.85), (1.0, 60.0..=85.0, 0.58..=0.75)] {
        let plane = Plane { water: mm / 1000.0, ..Plane::level(Surface::Asphalt) };
        let wet = stop_on(&plane, true, 100.0);
        let locked = stop_on(&plane, false, 100.0);
        let g = skidpad_on(&plane) / GRAVITY;
        println!("{mm} mm of water: 100-0 km/h {wet:.1} m with anti-lock brakes ({:.2} of dry), {locked:.1} m locked; skidpad {g:.3} g ({:.2} of dry)", wet / dry, g / dry_g);
        assert!(stop_range.contains(&wet), "{mm} mm: {wet:.1} m");
        assert!(g_range.contains(&g), "{mm} mm: {g:.3} g");
        assert!(wet >= last - 0.5, "deeper water stops no sooner: {wet:.1} after {last:.1}");
        last = wet;
    }
    // In 3 mm of standing water at 120 km/h the tyres aquaplane: braking does next to nothing at first.
    let plane = Plane { water: 0.003, ..Plane::level(Surface::Asphalt) };
    let mut car = car_on(&plane, 0.0);
    car.controls.abs = true;
    going(&mut car, 120.0 / 3.6);
    car.set_gear(0);
    car.controls.brake = 1.0;
    run(&mut car, &plane, 1.0);
    let kmh = car.body.vel.x * 3.6;
    println!("3 mm of water: from 120 km/h, a second of full braking leaves {kmh:.0} km/h");
    assert!(kmh > 100.0, "{kmh:.0} km/h");
}

#[test]
fn no_energy_from_nowhere() {
    // Coasting in neutral on a level road from 30 m/s, and dropped from 0.3 m: its energy only falls.
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    going(&mut car, 30.0);
    car.set_gear(0);
    let energy = |c: &Car| {
        let b = c.body;
        let i = c.p.inertia;
        let spin: f64 = c.wheels().iter().enumerate().map(|(k, w)| 0.5 * c.p.axles[k / 2].tyre.inertia * w.omega * w.omega).sum();
        0.5 * c.p.mass * b.vel.length_squared() + 0.5 * (i * b.omega * b.omega).element_sum() + c.p.mass * GRAVITY * -b.pos.z + spin
    };
    let mut last = energy(&car);
    let mut rises = 0.0f64;
    for _ in 0..(20.0 / DT) as usize {
        car.step(DT, &air(), &plane);
        let e = energy(&car);
        rises = rises.max(e - last);
        last = e;
    }
    println!("coasting: speed after 20 s {:.1} m/s, the most energy gained in a step {:.2} J", car.body.vel.x, rises);
    assert!(rises < 5.0, "gained {rises} J in a step");
    assert!(car.body.vel.x < 29.0 && car.body.vel.x > 20.0);

    let mut car = car_on(&plane, 0.0);
    car.body.pos.z -= 0.3;
    let first = energy(&car);
    let mut most = first;
    for _ in 0..(5.0 / DT) as usize {
        car.step(DT, &air(), &plane);
        most = most.max(energy(&car));
    }
    println!("dropped 0.3 m: energy {:.0} J at the start, at most {:.0} J after", first, most);
    assert!(most <= first + 10.0);
}

#[test]
fn walls_bounce_and_wreck() {
    // An armco rail along x = 30 m, its face to the south: driven into at 20° and 80 km/h the car
    // scrapes along it; square at 100 km/h, it is wrecked.
    let wall = Wall { a: (30.0, -500.0), b: (30.0, 500.0), base: 0.0, height: 0.75, normal: (-1.0, 0.0), kind: WallKind::Armco };
    let plane = Plane { walls: vec![wall], ..Plane::level(Surface::Asphalt) };
    let mut car = car_on(&plane, 70f64.to_radians());
    going(&mut car, 80.0 / 3.6);
    car.set_gear(0);
    run(&mut car, &plane, 4.0);
    println!("into armco at 20°, 80 km/h: crashed {:?}, now {:.1} m/s, {:.1} m from the rail", car.crashed(), car.data().speed, 30.0 - car.body.pos.x);
    assert!(car.crashed().is_none());
    assert!(car.body.pos.x < 30.0);

    let mut car = car_on(&plane, 0.0);
    going(&mut car, 100.0 / 3.6);
    car.set_gear(0);
    run(&mut car, &plane, 3.0);
    println!("square at 100 km/h: {:?}", car.crashed());
    assert!(car.crashed().is_some());
    assert!(car.body.pos.x < 30.0, "went through the wall");
}

#[test]
fn deterministic() {
    let plane = Plane::level(Surface::Asphalt);
    let drive = || {
        let mut car = car_on(&plane, 0.0);
        car.controls.throttle = 1.0;
        car.controls.steer = 0.3;
        run(&mut car, &plane, 8.0);
        car.body
    };
    assert_eq!(drive(), drive());
}

/// How a step of the steering is driven.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pedal {
    /// The throttle holding the speed it had.
    Hold,
    /// Full throttle from the step on.
    Full,
    /// The throttle holding the speed, then shut at 3 s, in the corner.
    LiftAt3,
}

/// The car going straight at `kmh`, the steering stepped to `steer` at 1 s, for 6 s: the most
/// sideslip of its body (degrees) and whether it spun (past 45°). `aids`: anti-lock, traction
/// control and stability control.
fn step_steer(kmh: f64, steer: f64, pedal: Pedal, aids: bool) -> (f64, bool) {
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    car.controls.abs = aids;
    car.controls.traction_control = aids;
    car.controls.stability_control = aids;
    let v0 = kmh / 3.6;
    let gear = (1..=car.p.gearbox.ratios.len() as i32).find(|&g| car.p.top_of_gear(g) > v0 * 1.15).unwrap_or(6);
    car.set_gear(gear);
    going(&mut car, v0);
    let mut most: f64 = 0.0;
    for k in 0..(6.0 / DT) as usize {
        let t = k as f64 * DT;
        car.controls.steer = if t >= 1.0 { steer } else { 0.0 };
        let v = car.body.vel.length();
        let hold = ((v0 - v) * 0.5 + 0.3).clamp(0.0, 1.0);
        car.controls.throttle = match pedal {
            Pedal::Hold => hold,
            Pedal::Full => {
                if t >= 1.0 {
                    1.0
                } else {
                    hold
                }
            }
            Pedal::LiftAt3 => {
                if t >= 3.0 {
                    0.0
                } else {
                    hold
                }
            }
        };
        car.step(DT, &air(), &plane);
        if t >= 1.0 && car.body.vel.x.abs() > 1.0 {
            let beta = car.body.vel.y.atan2(car.body.vel.x).to_degrees();
            most = most.max(beta.abs());
            if most > 45.0 {
                return (most, true);
            }
        }
    }
    (most, false)
}

#[test]
fn handling_a_step_of_steering() {
    // At 100 km/h, the speed held, the steering stepped (up to 0.4 of its lock, some 13° at the
    // road wheels): a road GT turns in with a few degrees of sideslip and settles, the front
    // giving first at the limit (it understeers).
    for steer in [0.05, 0.1, 0.2, 0.4] {
        let (beta, spun) = step_steer(100.0, steer, Pedal::Hold, true);
        let (beta_off, spun_off) = step_steer(100.0, steer, Pedal::Hold, false);
        println!("100 km/h, steer {steer}: sideslip {beta:.1}° with the aids, {beta_off:.1}° without");
        assert!(!spun && beta < 5.0, "steer {steer}: {beta:.1}°");
        assert!(!spun_off && beta_off < 7.0, "steer {steer}, no aids: {beta_off:.1}°");
    }
}

#[test]
fn handling_power_in_a_corner() {
    // Full throttle in a gentle corner at 50 km/h (3° and 9° at the road wheels): with traction and
    // stability control it does not spin; without them 3° of steering is no trap.
    for steer in [0.1, 0.3] {
        let (beta, spun) = step_steer(50.0, steer, Pedal::Full, true);
        let (beta_off, spun_off) = step_steer(50.0, steer, Pedal::Full, false);
        println!("50 km/h, full throttle, steer {steer}: sideslip {beta:.1}° with the aids; without, {beta_off:.1}° {}", if spun_off { "(spun)" } else { "" });
        assert!(!spun && beta < 8.0, "steer {steer} with the aids: {beta:.1}°");
        if steer < 0.15 {
            assert!(!spun_off, "3° of steering and the throttle spin it: {beta_off:.1}°");
        }
    }
    // From 20 km/h with more lock and no aids, the throttle provokes oversteer: it is there to be
    // had; the aids catch it.
    let (beta, _) = step_steer(20.0, 0.5, Pedal::Full, false);
    println!("20 km/h, full throttle, steer 0.5, no aids: sideslip {beta:.1}°");
    assert!(beta > 8.0, "power oversteer can be provoked: {beta:.1}°");
    let (beta, spun) = step_steer(20.0, 0.5, Pedal::Full, true);
    println!("  with the aids: {beta:.1}°");
    assert!(!spun, "the aids catch it: {beta:.1}°");
}

#[test]
fn handling_lifting_off_in_a_corner() {
    // Cornering at 100 km/h and the throttle shut: the nose tucks in, the tail steps out a little,
    // and it is caught.
    let (beta_off, spun_off) = step_steer(100.0, 0.2, Pedal::LiftAt3, false);
    let (beta, spun) = step_steer(100.0, 0.2, Pedal::LiftAt3, true);
    println!("lift-off at 100 km/h, steer 0.2: sideslip {beta_off:.1}° without the aids, {beta:.1}° with");
    assert!(!spun_off && beta_off < 10.0, "{beta_off:.1}°");
    assert!(!spun && beta < 6.0, "{beta:.1}°");
}

/// A car going straight at `v` m/s in the gear for it, with or without its aids.
fn at_speed(plane: &Plane, v: f64, aids: bool) -> Car {
    let mut car = car_on(plane, 0.0);
    car.controls.abs = aids;
    car.controls.traction_control = aids;
    car.controls.stability_control = aids;
    let gear = (1..=car.p.gearbox.ratios.len() as i32).find(|&g| car.p.top_of_gear(g) > v * 1.15).unwrap_or(6);
    car.set_gear(gear);
    going(&mut car, v);
    car
}

#[test]
fn load_moves_as_statics_say() {
    // Cornering at some 0.7 g and braking at 0.7 g: the load that leaves the inner wheels for the
    // outer ones is the car's weight times the acceleration, times the height of its centre of
    // gravity over its track; braking, over its wheelbase. Whatever the suspension (its springs,
    // bars and roll centres only share it out between the axles).
    let plane = Plane::level(Surface::Asphalt);
    let mut car = at_speed(&plane, 25.0, false);
    let (mut sum, mut lateral, mut n) = ([0.0; 4], 0.0, 0.0);
    for k in 0..(8.0 / DT) as usize {
        let t = k as f64 * DT;
        car.controls.steer = if t > 1.0 { 0.07 } else { 0.0 };
        car.controls.throttle = ((25.0 - car.body.vel.length()) * 0.5 + 0.2).clamp(0.0, 1.0);
        car.step(DT, &air(), &plane);
        if t > 5.0 {
            for (s, w) in sum.iter_mut().zip(car.wheels()) {
                *s += w.load;
            }
            lateral += car.body.vel.x * car.body.omega.z;
            n += 1.0;
        }
    }
    let p = &car.p;
    let track = (p.axles[0].track + p.axles[1].track) / 2.0;
    let moved = ((sum[0] - sum[1]) + (sum[2] - sum[3])) / n / 2.0;
    let statics = p.mass * lateral / n * p.cg_height / track;
    println!("cornering at {:.2} g: {moved:.0} N moved to the outer wheels, statics say {statics:.0} N; {:.0} % of it at the front", lateral / n / GRAVITY, (sum[0] - sum[1]) / ((sum[0] - sum[1]) + (sum[2] - sum[3])) * 100.0);
    assert!((moved / statics - 1.0).abs() < 0.04, "{moved:.0} N against {statics:.0} N");

    let mut car = at_speed(&plane, 40.0, true);
    car.set_gear(0);
    car.controls.brake = 0.45;
    run(&mut car, &plane, 1.0);
    let v0 = car.body.vel.x;
    let (mut front, mut n) = (0.0, 0.0);
    for _ in 0..(1.0 / DT) as usize {
        car.step(DT, &air(), &plane);
        front += car.wheels()[0].load + car.wheels()[1].load;
        n += 1.0;
    }
    let slowing = v0 - car.body.vel.x;
    let moved = front / n - 2.0 * car.p.static_load(0);
    let statics = car.p.mass * slowing * car.p.cg_height / car.p.wheelbase();
    println!("braking at {:.2} g: {moved:.0} N moved to the front wheels, statics say {statics:.0} N", slowing / GRAVITY);
    assert!((moved / statics - 1.0).abs() < 0.05, "{moved:.0} N against {statics:.0} N");
}

#[test]
fn shifting_down_under_braking() {
    // Braking from 200 km/h in a gentle turn, the automatic shifting down four times: the gearbox
    // opens the throttle to match the revs of each gear to come, so the clutch takes up the drive
    // without braking the rear wheels (unmatched, each shift slipped them by a third: a locked axle
    // for a moment, in a corner's entry).
    let plane = Plane::level(Surface::Asphalt);
    let mut car = at_speed(&plane, 55.0, false);
    car.controls.steer = 0.03;
    car.controls.throttle = 0.6;
    run(&mut car, &plane, 0.5);
    car.controls.throttle = 0.0;
    car.controls.brake = 0.5;
    let first = car.gear();
    let mut worst: f64 = 0.0;
    while car.body.vel.x > 12.0 {
        car.step(DT, &air(), &plane);
        worst = worst.min(car.wheels()[2].slip.min(car.wheels()[3].slip));
    }
    println!("braking from 200 km/h, gears {first} to {}: the rear tyres slip {worst:.3} at most", car.gear());
    assert!(car.gear() <= first - 3);
    assert!(worst > -0.11, "a shift down brakes the rear wheels: slip {worst:.3}");

    // A shift down that would take the engine past its limiter is refused.
    let mut car = at_speed(&plane, 55.0, false);
    car.controls.automatic = false;
    car.controls.throttle = 0.5;
    for _ in 0..4 {
        car.controls.shift = -1;
        run(&mut car, &plane, 0.3);
    }
    println!("at {:.0} km/h, four shifts down asked for: in gear {} at {:.0} rpm", car.body.vel.x * 3.6, car.gear(), car.rpm());
    assert_eq!(car.gear(), 4, "third would turn it past its limiter");
    assert!(car.rpm() < car.p.engine.limiter_rpm);
}

#[test]
fn stability_control_leaves_quick_corrections_alone() {
    // Full throttle from 70 km/h with the steering worked quickly from side to side (2.5 times a
    // second, 3° at the road wheels: a driver's corrections, a player's taps of a key): the car's
    // yaw follows the wheel a moment late, which is not a slide, and stability control lets it be.
    // It gains nearly the speed it gains without it (it once cut the engine for every reversal:
    // the car slowed to 52 km/h).
    let gained = |esc: bool| {
        let plane = Plane::level(Surface::Asphalt);
        let mut car = at_speed(&plane, 70.0 / 3.6, true);
        car.controls.stability_control = esc;
        car.controls.throttle = 1.0;
        for k in 0..(5.0 / DT) as usize {
            car.controls.steer = if (k as f64 * DT * 2.5).fract() < 0.5 { 0.1 } else { -0.1 };
            car.step(DT, &air(), &plane);
        }
        car.body.vel.x * 3.6
    };
    let (with, without) = (gained(true), gained(false));
    println!("5 s of full throttle from 70 km/h, the wheel worked from side to side: {with:.0} km/h with stability control, {without:.0} without");
    assert!(with > without - 8.0, "{with:.0} km/h against {without:.0}");
}

#[test]
fn the_automatic_holds_its_gear_through_a_lift() {
    // Hard on the throttle, off it for a corner, on it again: the gear stays, as a sports
    // automatic's does (shifting up at every lift, it had the wrong gear at every corner's exit).
    // Cruising on a little throttle, it shifts up.
    let plane = Plane::level(Surface::Asphalt);
    let mut car = at_speed(&plane, 70.0 / 3.6, true);
    car.set_gear(2);
    going(&mut car, 70.0 / 3.6);
    car.controls.throttle = 1.0;
    run(&mut car, &plane, 1.0);
    let gear = car.gear();
    assert_eq!(gear, 2);
    for throttle in [0.0, 0.1, 0.3, 1.0] {
        car.controls.throttle = throttle;
        run(&mut car, &plane, 0.4);
        assert_eq!(car.gear(), gear, "at a throttle of {throttle}, coming back on it");
    }
    car.controls.throttle = 0.12;
    run(&mut car, &plane, 8.0);
    println!("in gear {gear} through the lift; cruising, in gear {}", car.gear());
    assert!(car.gear() > gear, "cruising, it shifts up");
}
