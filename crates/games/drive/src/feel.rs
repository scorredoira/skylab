//! A player at the keyboard, as a test: someone who holds and taps the arrow keys round the
//! circuit, through `controls::Driving` as the game has it. Keys are all or nothing, so what such
//! a player can do says how the car and its aids take rough hands: stability control that cuts the
//! engine for every tap of a key, or an automatic that shifts up whenever the throttle's key is let
//! go, cost them half a minute a lap.

use sim_core::car::driver::{Pace, RacingLine};
use sim_core::{CarKind, Start, World};

use crate::controls::{Driving, Feet};
use crate::game::circuit_world;

/// What became of some laps at the keyboard.
#[derive(Debug)]
struct Laps {
    /// The flying laps' times, s.
    times: Vec<f64>,
    /// The farthest the car's middle went beyond the asphalt's edge, m.
    widest: f64,
    /// Times it slid past 30° of sideslip, and whether it was wrecked.
    spins: u32,
    crashed: bool,
}

/// Drives `laps` flying laps of Lakeside with the keys, the aids on. The player looks at a point of
/// the racing line ahead and at the speed the line asks for a little further on (a share `pace` of
/// it), and every tenth of a second decides what to hold: the key towards the point while the car
/// turns less than it takes to get there (a tap if it is nearly enough), none while it turns
/// enough, the other while it turns too much; the throttle below the speed, the brake above it.
fn at_the_keyboard(aids: bool, help: bool, pace: f64, rain: f64, laps: usize) -> Laps {
    in_a(CarKind::GT, aids, help, pace, rain, laps)
}

/// The same in a car of `kind`.
fn in_a(kind: CarKind, aids: bool, help: bool, pace: f64, rain: f64, laps: usize) -> Laps {
    let (circuit, terrain) = circuit_world("lakeside").expect("the circuit");
    let mut world = World::with_terrain(1, terrain);
    world.weather.rain = rain;
    world.settle_water();
    let (n, e, heading) = circuit.grid(0);
    let id = world.add_car(kind, Start::Air { n, e, heading });
    let line = RacingLine::new(&circuit, kind.spec(), Pace::default());
    {
        let c = &mut world.vehicle_mut(id).car_mut().expect("a car").controls;
        (c.abs, c.traction_control, c.stability_control) = (aids, aids, aids);
    }
    let mut driving = Driving::new();
    driving.write = false;
    driving.pedal_help = help;
    let mut out = Laps { times: Vec::new(), widest: 0.0, spins: 0, crashed: false };
    let (mut key, mut held, mut throttle, mut brake) = (0.0, 0, false, false);
    let mut last = circuit.from_start(circuit.locate(n, e).expect("on the grid").along);
    let (mut lap, mut lap_start) = (0, 0.0);
    let mut sliding = false;
    let frame = 1.0 / 60.0;
    let mut frames = 0u64;
    while out.times.len() < laps && world.time() < 250.0 * (laps as f64 + 1.0) {
        let car = world.vehicle(id).car().expect("a car");
        let b = car.body;
        let Some(at) = circuit.locate(b.pos.x, b.pos.y).filter(|_| car.crashed().is_none()) else {
            out.crashed = true;
            break;
        };
        let v = b.vel.x.max(0.0);
        if frames.is_multiple_of(6) {
            // Where it wants to be, ahead on the line, and the yaw it takes to get there.
            let ahead = 8.0 + 0.6 * v;
            let ((tn, te), _) = line.at(&circuit, at.along + ahead);
            let heading = sim_core::euler_angles(b.q).heading;
            let (dn, de) = (tn - b.pos.x, te - b.pos.y);
            let forward = dn * heading.cos() + de * heading.sin();
            let right = -dn * heading.sin() + de * heading.cos();
            let wanted = v * 2.0 * right / (forward * forward + right * right).max(1.0);
            let error = wanted - b.omega.z;
            let band = 0.02 + 0.1 * wanted.abs();
            key = if error.abs() > band { error.signum() } else { 0.0 };
            held = (((error.abs() - band) / (0.12 + 0.4 * wanted.abs())).clamp(0.0, 1.0) * 6.0).ceil() as u64;
            let target = line.speed_ahead(&circuit, at.along, 5.0 + 0.3 * v) * pace;
            throttle = v < target - 0.5;
            brake = v > target + 1.5;
        }
        let controls = driving.keys(if frames % 6 < held { key } else { 0.0 }, throttle, brake, frame, b.vel.x, &Feet::of(car));
        out.widest = out.widest.max(at.across.abs() - circuit.sample(at.along).width / 2.0);
        let beta = if b.vel.x.abs() > 3.0 { b.vel.y.atan2(b.vel.x.abs()).to_degrees().abs() } else { 0.0 };
        if beta > 30.0 && !sliding {
            out.spins += 1;
        }
        sliding = beta > 30.0 || (sliding && beta > 10.0);
        let along = circuit.from_start(at.along);
        if along < last - circuit.length / 2.0 {
            if lap > 0 {
                out.times.push(world.time() - lap_start);
            }
            lap += 1;
            lap_start = world.time();
        }
        last = along;
        let c = &mut world.vehicle_mut(id).car_mut().expect("a car").controls;
        (c.steer, c.throttle, c.brake) = controls;
        for _ in 0..4 {
            world.step();
        }
        frames += 1;
    }
    out
}

/// With the aids on, a player at the keyboard laps Lakeside without spinning or leaving the kerbs
/// behind, within ten seconds of the racing driver (115 s): 123 and 118 s as measured (it was 262
/// and 255 s while stability control cut the engine for every tap of a key and the automatic
/// shifted up at every lift); and at nine tenths of the pace, and in the rain at the wet road's.
#[test]
fn a_player_at_the_keyboard_laps_cleanly() {
    for (pace, rain, most) in [(1.0, 0.0, 128.0), (0.9, 0.0, 138.0), (0.72, 0.6, 170.0)] {
        let l = at_the_keyboard(true, true, pace, rain, 2);
        println!("pace {pace}, rain {rain}: laps {:.1?} s, at most {:.1} m beyond the asphalt, {} spins", l.times, l.widest, l.spins);
        assert!(!l.crashed && l.spins == 0, "{l:?}");
        assert_eq!(l.times.len(), 2, "{l:?}");
        assert!(l.times.iter().all(|&t| t < most), "{l:?}");
        assert!(l.widest < sim_core::circuit::KERB_WIDTH + 0.5, "{l:?}");
    }
}

/// Without traction control, anti-lock brakes or stability control, the keys as feet that feel
/// the tyres (`Driving::pedal_help`): the same player laps cleanly, dry (126 and 122 s) and in the
/// rain. With the keys all or nothing they spin a dozen times and do not finish a lap.
#[test]
fn without_the_aids_the_keys_feel_the_tyres() {
    for (pace, rain, most) in [(0.95, 0.0, 132.0), (0.68, 0.6, 180.0)] {
        let l = at_the_keyboard(false, true, pace, rain, 2);
        println!("no aids, pace {pace}, rain {rain}: laps {:.1?} s, at most {:.1} m beyond the asphalt, {} spins", l.times, l.widest, l.spins);
        assert!(!l.crashed && l.spins == 0, "{l:?}");
        assert_eq!(l.times.len(), 2, "{l:?}");
        assert!(l.times.iter().all(|&t| t < most), "{l:?}");
        assert!(l.widest < sim_core::circuit::KERB_WIDTH + 0.5, "{l:?}");
    }
    let l = at_the_keyboard(false, false, 0.95, 0.0, 1);
    assert!(l.spins > 0, "all or nothing, it spins: {l:?}");
}

/// The Model Y at the keyboard, with its aids and without: four driven wheels and a motor's
/// torque that comes at once; it laps cleanly.
#[test]
fn the_model_y_at_the_keyboard() {
    for (aids, pace, most) in [(true, 1.0, 140.0), (false, 0.95, 145.0)] {
        let l = in_a(CarKind::MODEL_Y, aids, true, pace, 0.0, 2);
        println!("Model Y, aids {aids}, pace {pace}: laps {:.1?} s, at most {:.1} m beyond the asphalt, {} spins", l.times, l.widest, l.spins);
        assert!(!l.crashed && l.spins == 0, "{l:?}");
        assert_eq!(l.times.len(), 2, "{l:?}");
        assert!(l.times.iter().all(|&t| t < most), "{l:?}");
        assert!(l.widest < sim_core::circuit::KERB_WIDTH + 0.5, "{l:?}");
    }
}
