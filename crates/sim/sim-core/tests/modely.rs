//! The Tesla Model Y Long Range AWD (`assets/cars/modely/car.ron`) against the figures Tesla gives
//! and road tests measured: 0-100 km/h in 4.8 s (tests: 4.6 to 5.1), a top speed of 201 km/h held
//! by its software, 100-0 km/h in 34 to 38 m, 0.84 to 0.90 g on the skidpad; and what an electric
//! car does that the engine's does not: it regenerates when the pedal is let go, holds itself
//! stopped, goes into reverse with no gears, and gets no energy from nowhere.
//! `cargo test -p sim-core --test modely -- --nocapture` prints the measurements.

use sim_core::aircraft::Environment;
use sim_core::car::{Car, CarKind, Drive, Plane, Surface};
use sim_core::{Body, World, GRAVITY, V3};

const DT: f64 = World::DT;

fn air() -> Environment {
    Environment { ground_height: 0.0, ..Environment::default() }
}

fn car_on(plane: &Plane, heading: f64) -> Car {
    let mut car = Car::of(CarKind::MODEL_Y);
    car.stand(plane, 0.0, 0.0, heading);
    car
}

fn run(car: &mut Car, plane: &Plane, seconds: f64) {
    for _ in 0..(seconds / DT).round() as usize {
        car.step(DT, &air(), plane);
    }
}

fn going(car: &mut Car, speed: f64) {
    let b = car.body;
    car.put(Body { vel: V3::new(speed, 0.0, 0.0), ..b });
}

#[test]
fn file_reads_as_an_electric_car() {
    let car = Car::of(CarKind::MODEL_Y);
    assert_eq!(car.p.key, "modely");
    assert_eq!(car.p.drive, Drive::All);
    assert_eq!(CarKind::parse("tesla"), Some(CarKind::MODEL_Y));
    let e = car.p.electric.as_ref().expect("electric");
    assert_eq!(e.motors.len(), 2);
    let load: f64 = (0..2).map(|a| 2.0 * car.p.static_load(a)).sum();
    assert!((load - car.p.mass * GRAVITY).abs() < 1.0);
    println!("static loads: front {:.0} N, rear {:.0} N a wheel; {:.0} kW at the wheels; top {:.0} km/h", car.p.static_load(0), car.p.static_load(1), car.p.power() / 1000.0, car.p.top_of_gear(1) * 3.6);
    assert!((car.p.top_of_gear(1) * 3.6 - 201.0).abs() < 0.5);
}

#[test]
fn still_at_rest_and_held() {
    // Stopped in drive with no pedal it holds itself, level and on a 15 % slope either way.
    for (rise, heading) in [(0.0, 0.0), (0.15, 0.0), (0.15, std::f64::consts::PI)] {
        let plane = Plane { rise: (rise, 0.0), ..Plane::level(Surface::Asphalt) };
        let mut car = car_on(&plane, heading);
        run(&mut car, &plane, 2.0);
        let settled = car.body.pos;
        run(&mut car, &plane, 10.0);
        let moved = (car.body.pos - settled).length();
        println!("held, slope {rise}, heading {heading:.1}: moved {moved:.5} m in 10 s");
        assert!(moved < 0.01, "moved {moved} m");
        if rise == 0.0 {
            assert!((-car.body.pos.z - car.p.cg_height).abs() < 0.02, "centre of gravity {:.3} m up", -car.body.pos.z);
        }
    }
    // In neutral on the slope it rolls away.
    let plane = Plane { rise: (0.15, 0.0), ..Plane::level(Surface::Asphalt) };
    let mut car = car_on(&plane, 0.0);
    car.set_gear(0);
    run(&mut car, &plane, 5.0);
    assert!(car.body.vel.x < -2.0, "rolls back: {:?}", car.body.vel);
}

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
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    let t = sprint(&mut car, &plane, 100.0);
    let t160 = t + sprint(&mut car, &plane, 160.0);
    println!("0-100 km/h {t:.2} s (Tesla: 4.8; tests 4.6 to 5.1); 0-160 km/h {t160:.1} s");
    assert!((4.6..=5.2).contains(&t), "0-100 in {t:.2} s");
    // With no traction control it spins its tyres no more than a little: four driven wheels.
    let mut car = car_on(&plane, 0.0);
    car.controls.traction_control = false;
    let loose = sprint(&mut car, &plane, 100.0);
    println!("without traction control: 0-100 km/h {loose:.2} s");
    assert!(loose < t + 0.6);
}

#[test]
fn top_speed() {
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    going(&mut car, 50.0);
    car.controls.throttle = 1.0;
    let mut best = 0.0f64;
    for _ in 0..(60.0 / DT) as usize {
        car.step(DT, &air(), &plane);
        best = best.max(car.body.vel.x);
    }
    println!("top speed {:.1} km/h at {:.0} rpm of the rear motor (Tesla: 201)", best * 3.6, car.rpm());
    assert!((197.0..=202.0).contains(&(best * 3.6)), "top speed {:.1} km/h", best * 3.6);
}

fn stop(abs: bool, kmh: f64) -> f64 {
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    car.controls.abs = abs;
    going(&mut car, kmh / 3.6);
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
    println!("100-0 km/h: {with:.1} m with anti-lock brakes (tests: 34 to 38), {without:.1} m without");
    assert!((34.0..=38.5).contains(&with), "{with:.1} m");
    assert!(without > with);
}

#[test]
fn lifting_off_regenerates() {
    // The pedal let go at 100 km/h: the motors slow it at some 0.15 to 0.25 g (the battery takes
    // back 75 kW), more slowly, it comes to rest by itself and stays there.
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    going(&mut car, 100.0 / 3.6);
    run(&mut car, &plane, 0.5);
    let v0 = car.body.vel.x;
    run(&mut car, &plane, 1.0);
    let at_speed = (v0 - car.body.vel.x) / GRAVITY;
    going(&mut car, 40.0 / 3.6);
    run(&mut car, &plane, 0.5);
    let v0 = car.body.vel.x;
    run(&mut car, &plane, 1.0);
    let slow = (v0 - car.body.vel.x) / GRAVITY;
    run(&mut car, &plane, 10.0);
    println!("lifting off: {at_speed:.2} g at 100 km/h, {slow:.2} g at 40 km/h; then {:.3} m/s", car.body.vel.x);
    assert!((0.12..=0.25).contains(&at_speed), "{at_speed:.2} g");
    assert!((0.15..=0.32).contains(&slow), "{slow:.2} g");
    assert!(car.body.vel.x.abs() < 0.05);
    // In neutral it coasts.
    going(&mut car, 100.0 / 3.6);
    car.set_gear(0);
    run(&mut car, &plane, 1.0);
    assert!(car.body.vel.x > 100.0 / 3.6 - 0.8, "coasting: {:.1} m/s", car.body.vel.x);
}

#[test]
fn backs_up_in_reverse() {
    let plane = Plane::level(Surface::Asphalt);
    let mut car = car_on(&plane, 0.0);
    car.controls.shift = -1;
    run(&mut car, &plane, 0.5);
    assert_eq!(car.gear(), -1);
    car.controls.throttle = 0.3;
    run(&mut car, &plane, 3.0);
    println!("reverse, 0.3 of the pedal for 3 s: {:.1} m/s", car.body.vel.x);
    assert!(car.body.vel.x < -2.0);
    car.controls.throttle = 0.0;
    run(&mut car, &plane, 6.0);
    car.controls.shift = 1;
    run(&mut car, &plane, 0.5);
    assert_eq!(car.gear(), 1);
}

#[test]
fn skidpad() {
    let plane = Plane::level(Surface::Asphalt);
    let radius = 45.72;
    let mut car = car_on(&plane, 0.0);
    going(&mut car, 12.0);
    let mut target = 12.0;
    let mut best = 0.0f64;
    let mut held = Vec::new();
    for _ in 0..(120.0 / DT) as usize {
        let b = car.body;
        let (dn, de) = (b.pos.x, b.pos.y - radius);
        let r = dn.hypot(de);
        let v = b.vel.x;
        let e = sim_core::euler_angles(b.q);
        let tangent = de.atan2(dn) + std::f64::consts::FRAC_PI_2;
        let heading_error = sim_core::remainder(tangent - e.heading, std::f64::consts::TAU);
        let wanted = 1.0 / radius + 0.02 * (r - radius) + 0.8 * heading_error / v.max(1.0);
        let steer = (wanted * car.p.wheelbase()).atan() + 0.06 * (wanted * v - b.omega.z);
        car.controls.steer = (steer / car.p.steering.lock_deg.to_radians()).clamp(-1.0, 1.0);
        if (r - radius).abs() < 1.0 {
            target += 0.12 * DT;
        } else {
            target -= 0.3 * DT;
        }
        // Holding speed takes some pedal: let go, it regenerates.
        car.controls.throttle = ((target - v) * 0.6 + 0.3).clamp(0.0, 1.0);
        car.step(DT, &air(), &plane);
        let lateral = car.body.vel.x * car.body.omega.z;
        if (r - radius).abs() < 1.0 {
            held.push(lateral);
        } else {
            held.clear();
        }
        if held.len() >= 240 {
            best = best.max(held[held.len() - 240..].iter().sum::<f64>() / 240.0);
        }
    }
    println!("skidpad: {:.3} g held a second (tests: 0.84 to 0.90)", best / GRAVITY);
    assert!((0.82..=0.92).contains(&(best / GRAVITY)), "{:.3} g", best / GRAVITY);
}

#[test]
fn no_energy_from_nowhere() {
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
    println!("coasting: {:.1} m/s after 20 s, the most energy gained in a step {rises:.2} J", car.body.vel.x);
    assert!(rises < 8.0, "gained {rises} J");
    assert!(car.body.vel.x < 29.5 && car.body.vel.x > 22.0);
}

#[test]
fn steady_in_a_step_of_steering() {
    for aids in [true, false] {
        for steer in [0.05, 0.1, 0.2, 0.4] {
            let plane = Plane::level(Surface::Asphalt);
            let mut car = car_on(&plane, 0.0);
            car.controls.abs = aids;
            car.controls.traction_control = aids;
            car.controls.stability_control = aids;
            let v0 = 100.0 / 3.6;
            going(&mut car, v0);
            let mut most: f64 = 0.0;
            for k in 0..(6.0 / DT) as usize {
                let t = k as f64 * DT;
                car.controls.steer = if t >= 1.0 { steer } else { 0.0 };
                car.controls.throttle = ((v0 - car.body.vel.length()) * 0.5 + 0.35).clamp(0.0, 1.0);
                car.step(DT, &air(), &plane);
                if t >= 1.0 {
                    most = most.max(car.body.vel.y.atan2(car.body.vel.x).to_degrees().abs());
                }
            }
            println!("100 km/h, steer {steer}, aids {aids}: sideslip {most:.1}°");
            assert!(most < 6.0, "steer {steer}, aids {aids}: {most:.1}°");
        }
    }
}

#[test]
fn the_racing_driver_laps_lakeside_in_it() {
    use sim_core::car::driver::Driver;
    use sim_core::circuit::{Circuit, CircuitFile};
    use sim_core::{Start, Terrain};
    use std::sync::Arc;
    let file = CircuitFile::find("lakeside").expect("the built-in circuit");
    let circuit = Arc::new(Circuit::new(file, &Terrain::new(file.seed())));
    let terrain = Arc::new(Terrain::new(file.seed()).with_circuits(vec![circuit.clone()]));
    let mut world = World::with_terrain(1, terrain);
    let (n, e, heading) = circuit.grid(0);
    let id = world.add_car(CarKind::MODEL_Y, Start::Air { n, e, heading });
    let mut driver = Driver::new(circuit.clone(), CarKind::MODEL_Y.spec());
    let mut last = circuit.from_start(circuit.locate(n, e).unwrap().along);
    let (mut laps, mut lap_start) = (0, 0.0);
    let mut times = Vec::new();
    let mut widest: f64 = 0.0;
    while laps < 3 && world.time() < 500.0 {
        world.fly(id, &mut driver);
        world.step();
        let car = world.vehicle(id).car().unwrap();
        assert!(car.crashed().is_none(), "{:?} at {:.0} s", car.crashed(), world.time());
        let at = circuit.locate(car.body.pos.x, car.body.pos.y).expect("on the circuit");
        widest = widest.max(at.across.abs() - circuit.sample(at.along).width / 2.0);
        let along = circuit.from_start(at.along);
        if along < last - circuit.length / 2.0 {
            if laps > 0 {
                times.push(world.time() - lap_start);
            }
            laps += 1;
            lap_start = world.time();
        }
        last = along;
    }
    println!("Model Y at Lakeside: laps {times:.2?} s (the GT: 115); at most {widest:.2} m beyond the asphalt");
    assert_eq!(times.len(), 2);
    assert!(widest < sim_core::circuit::KERB_WIDTH + 0.5, "{widest:.2} m off");
}
