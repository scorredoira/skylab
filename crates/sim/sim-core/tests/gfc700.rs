//! The C172's GFC 700 through `World::fly`: each mode captures and holds what it is set to, the
//! selected altitude is captured without overshooting, the approach, the protections, how the
//! autopilot is disconnected, and what a blocked pitot does to it.
//!
//! The pilot's hands are the aircraft's controls as the test leaves them: the throttle is set by
//! the test, as the C172 has no autothrottle, and the rudder stays centred (no yaw servo).
//!
//! `cargo test -p sim-core --test gfc700 -- --nocapture` prints what each test measured.

use sim_core::*;

const TAU: f64 = 2.0 * std::f64::consts::PI;
const FPM: f64 = FEET / 60.0;

/// A C172 trimmed at 100 kt, 900 m over the ground, and its GFC 700.
struct Flight {
    world: World,
    id: VehicleId,
    gfc: Gfc700,
}

impl Flight {
    fn new(seed: u64, heading: f64) -> Flight {
        let mut world = World::new(seed);
        let id = world.add_aircraft(AircraftKind::C172, Start::Air { n: 3000.0, e: -2000.0, heading });
        let fit = world.vehicle(id).aircraft().unwrap().p.afcs.expect("the C172 has a GFC 700");
        let mut flight = Flight { world, id, gfc: Gfc700::new(fit) };
        // A second for the G1000 to see the aircraft.
        flight.fly(1.0, |_, _| {});
        flight
    }

    fn fly(&mut self, seconds: f64, mut each: impl FnMut(&mut Flight, f64)) {
        for i in 0..(seconds / World::DT).round() as usize {
            self.world.fly(self.id, &mut self.gfc);
            self.world.step();
            each(self, (i + 1) as f64 * World::DT);
        }
    }

    fn press(&mut self, keys: &[AfcsKey]) {
        for &key in keys {
            assert!(self.gfc.press(key), "{key:?} did nothing");
        }
    }

    fn vehicle(&self) -> &Vehicle {
        self.world.vehicle(self.id)
    }

    fn throttle(&mut self, throttle: f64) {
        self.world.vehicle_mut(self.id).aircraft_mut().unwrap().controls.throttle = throttle;
    }

    /// The true altitude, heading and indicated airspeed (ideal sensors), and the bank.
    fn truth(&self) -> (f64, f64, f64, f64) {
        let p = self.vehicle();
        let e = euler_angles(p.aircraft().unwrap().state.q);
        (-p.aircraft().unwrap().state.pos.z, e.heading, p.aircraft().unwrap().data().ias, e.roll)
    }

    /// What the G1000 reads: altitude, vertical speed, airspeed.
    fn air_data(&self) -> AirData {
        self.vehicle().aircraft().unwrap().instruments.readings().air_data
    }
}

#[test]
fn hdg_turns_at_standard_rate_and_holds_the_bug() {
    for (seed, turn) in [(1, 90.0), (2, -170.0), (3, 30.0)] {
        let mut f = Flight::new(seed, 1.0);
        f.press(&[AfcsKey::Ap, AfcsKey::Alt]);
        let bug = f.gfc.heading_bug() + turn * DEG;
        f.gfc.set_heading_bug(bug);
        f.press(&[AfcsKey::Hdg]);
        assert_eq!(f.gfc.annunciations(), ["HDG", "", "ALT", ""]);
        let (mut bank, mut rate, mut off, mut alt, mut last) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, None::<f64>);
        let start = f.air_data().altitude;
        let seconds = 30.0 + turn.abs() / 3.0;
        f.fly(seconds + 40.0, |f, t| {
            let (_, _, _, roll) = f.truth();
            bank = bank.max(roll.abs() / DEG);
            let heading = f.truth().1;
            if let Some(before) = last {
                rate = rate.max(remainder(heading - before, TAU).abs() / World::DT / DEG);
            }
            last = Some(heading);
            alt = alt.max((f.air_data().altitude - start).abs() / FEET);
            if t > seconds {
                let heading = f.vehicle().aircraft().unwrap().instruments.readings().ahrs[2];
                off = off.max(remainder(heading - bug, TAU).abs() / DEG);
            }
        });
        println!("HDG {turn:+.0}°: largest bank {bank:.1}°, turn rate {rate:.1}°/s, within {off:.2}° of the bug after {seconds:.0} s, altitude within {alt:.0} ft");
        assert!(off < 2.0, "heading {off}");
        assert!(bank < 22.5, "bank {bank}");
        assert!(rate < 3.3, "rate {rate}");
        assert!(alt < 20.0, "altitude {alt}");
    }
}

#[test]
fn rol_levels_the_wings_or_holds_the_bank() {
    let mut f = Flight::new(1, 0.0);
    // Rolled into 15° of bank by hand, then the autopilot.
    let mut s = f.vehicle().aircraft().unwrap().state;
    s.q *= axis_angle(15.0 * DEG, V3::X);
    f.world.place(f.id, s);
    f.fly(0.5, |_, _| {});
    f.press(&[AfcsKey::Ap]);
    assert_eq!(f.gfc.annunciations()[0], "ROL");
    f.fly(20.0, |_, _| {});
    let (_, _, _, roll) = f.truth();
    println!("ROL from 15°: {:.1}° after 20 s", roll / DEG);
    assert!((roll / DEG - 15.0).abs() < 2.0);
    // Nearly level, it levels.
    let mut f = Flight::new(1, 0.0);
    let mut s = f.vehicle().aircraft().unwrap().state;
    s.q *= axis_angle(4.0 * DEG, V3::X);
    f.world.place(f.id, s);
    f.fly(0.5, |_, _| {});
    f.press(&[AfcsKey::Ap]);
    f.fly(20.0, |_, _| {});
    let (_, _, _, roll) = f.truth();
    println!("ROL from 4°: {:.1}° after 20 s", roll / DEG);
    assert!(roll.abs() < 1.0 * DEG);
}

#[test]
fn alt_holds_within_20_ft_in_turbulence_and_turns() {
    let mut f = Flight::new(1, 1.0);
    f.world.weather.wind_speed = 15.0 * KNOTS;
    f.world.weather.turbulence = 0.25;
    f.press(&[AfcsKey::Ap, AfcsKey::Alt]);
    let VerticalMode::Altitude(held) = f.gfc.vertical() else { panic!("not ALT") };
    let (mut off, mut bank) = (0.0_f64, 0.0_f64);
    f.fly(30.0, |_, _| {});
    f.gfc.set_heading_bug(f.gfc.heading_bug() + 120.0 * DEG);
    f.press(&[AfcsKey::Hdg]);
    f.fly(150.0, |f, _| {
        off = off.max((f.air_data().altitude - held).abs() / FEET);
        bank = bank.max(f.truth().3.abs() / DEG);
    });
    println!("ALT in 15 kt and turbulence 0.25, turning 120°: within {off:.1} ft, bank up to {bank:.1}°");
    assert!(off < 20.0, "altitude {off}");
    // NOSE UP moves the reference 10 ft.
    f.press(&[AfcsKey::NoseUp, AfcsKey::NoseUp]);
    assert_eq!(f.gfc.vertical(), VerticalMode::Altitude(held + 20.0 * FEET));
}

#[test]
fn vs_holds_its_rate_and_alts_captures_without_overshooting() {
    for (climb, throttle) in [(500.0_f64, 1.0), (300.0, 1.0), (-700.0, 0.45)] {
        let mut f = Flight::new(2, 0.5);
        let cruise = f.vehicle().aircraft().unwrap().controls.throttle;
        f.throttle(throttle);
        let start = f.air_data().altitude;
        let target = ((start / FEET + climb.signum() * 1000.0) / 100.0).round() * 100.0;
        f.gfc.set_selected_altitude(target * FEET);
        f.press(&[AfcsKey::Ap, AfcsKey::Vs]);
        // The wheel to the rate wanted, 100 ft/min a click.
        let clicks = (climb / 100.0) as i32;
        for _ in 0..clicks.abs() {
            f.press(&[if clicks > 0 { AfcsKey::NoseUp } else { AfcsKey::NoseDown }]);
        }
        assert_eq!(f.gfc.annunciations()[2..], ["VS", "ALTS"]);
        let (mut rate_off, mut overshoot, mut held, mut captured_at) = (0.0_f64, 0.0_f64, 0.0_f64, None);
        f.fly(240.0, |f, t| {
            let air = f.air_data();
            let mode = f.gfc.vertical();
            if matches!(mode, VerticalMode::VerticalSpeed(_)) && t > 25.0 {
                rate_off = rate_off.max((air.vertical_speed / FPM - climb).abs());
            }
            if mode != VerticalMode::Capture && !matches!(mode, VerticalMode::VerticalSpeed(_)) && captured_at.is_none() {
                captured_at = Some(t);
                // Level: the pilot sets cruise power again.
                f.throttle(cruise);
            }
            overshoot = overshoot.max((air.altitude / FEET - target) * climb.signum());
            if captured_at.is_some_and(|c| t > c + 20.0) {
                held = held.max((air.altitude / FEET - target).abs());
            }
        });
        println!("VS {climb:+.0} ft/min: within {rate_off:.0} ft/min after 25 s; ALT at {target:.0} ft from {:.0} s, overshoot {overshoot:.1} ft, then within {held:.1} ft", captured_at.unwrap_or(-1.0));
        assert!(rate_off < 100.0, "rate {rate_off}");
        assert!(matches!(f.gfc.vertical(), VerticalMode::Altitude(a) if (a / FEET - target).abs() < 1.0));
        assert!(overshoot < 50.0, "overshoot {overshoot}");
        assert!(held < 20.0, "held {held}");
    }
}

#[test]
fn flc_climbs_and_descends_at_its_speed_and_levels_at_the_selected_altitude() {
    // (speed kt, throttle, change ft)
    for (speed, throttle, change) in [(75.0_f64, 1.0, 2000.0_f64), (90.0, 0.0, -1500.0), (110.0, 0.55, -1000.0)] {
        let mut f = Flight::new(3, 2.0);
        let cruise = f.vehicle().aircraft().unwrap().controls.throttle;
        f.press(&[AfcsKey::Ap, AfcsKey::Alt]);
        let start = f.air_data().altitude / FEET;
        let target = ((start + change) / 100.0).round() * 100.0;
        f.gfc.set_selected_altitude(target * FEET);
        f.press(&[AfcsKey::Flc]);
        let VerticalMode::Flc(now) = f.gfc.vertical() else { panic!("not FLC") };
        let clicks = ((now / KNOTS) - speed).round() as i32;
        for _ in 0..clicks.abs() {
            f.press(&[if clicks > 0 { AfcsKey::NoseUp } else { AfcsKey::NoseDown }]);
        }
        assert!(matches!(f.gfc.vertical(), VerticalMode::Flc(v) if (v / KNOTS - speed).abs() < 1e-6));
        f.throttle(throttle);
        let (mut off, mut overshoot, mut held, mut captured, mut reached) = (0.0_f64, 0.0_f64, 0.0_f64, None, false);
        f.fly(420.0, |f, t| {
            let air = f.air_data();
            let mode = f.gfc.vertical();
            // From when the airspeed first reaches the reference (it goes there at about 1 kt/s).
            let error = (air.airspeed / KNOTS - speed).abs();
            reached |= error < 1.0;
            if matches!(mode, VerticalMode::Flc(_)) && reached {
                off = off.max(error);
            }
            if matches!(mode, VerticalMode::Altitude(_)) && captured.is_none() {
                captured = Some(t);
                f.throttle(cruise);
            }
            overshoot = overshoot.max((air.altitude / FEET - target) * change.signum());
            if captured.is_some_and(|c| t > c + 20.0) {
                held = held.max((air.altitude / FEET - target).abs());
            }
        });
        println!("FLC {speed:.0} kt, throttle {throttle}, {change:+.0} ft: airspeed within {off:.1} kt once reached; ALT at {:.0} s, overshoot {overshoot:.1} ft, then within {held:.1} ft", captured.unwrap_or(-1.0));
        assert!(off < 3.0, "airspeed {off}");
        assert!(captured.is_some(), "never captured");
        assert!(overshoot < 50.0, "overshoot {overshoot}");
        assert!(held < 20.0, "held {held}");
    }
}

#[test]
fn flc_never_climbs_away_from_a_selected_altitude_below() {
    let mut f = Flight::new(1, 0.0);
    let start = f.air_data().altitude;
    f.gfc.set_selected_altitude(start - 1000.0 * FEET);
    f.press(&[AfcsKey::Ap, AfcsKey::Flc]);
    // Slower asked, with power on: the nose would go up, but the descent asked holds it level at most.
    for _ in 0..20 {
        f.press(&[AfcsKey::NoseUp]);
    }
    f.throttle(0.9);
    let mut highest = 0.0_f64;
    f.fly(60.0, |f, _| highest = highest.max((f.air_data().altitude - start) / FEET));
    println!("FLC 20 kt slower with power, selected altitude 1000 ft below: rose at most {highest:.0} ft");
    assert!(highest < 50.0, "climbed {highest}");
}

#[test]
fn pit_holds_the_pitch_and_the_protections_keep_the_airspeed() {
    let mut f = Flight::new(1, 0.0);
    f.press(&[AfcsKey::Ap]);
    let VerticalMode::Pitch(start) = f.gfc.vertical() else { panic!("not PIT") };
    for _ in 0..4 {
        f.press(&[AfcsKey::NoseUp]);
    }
    f.fly(30.0, |_, _| {});
    let pitch = f.vehicle().aircraft().unwrap().instruments.readings().ahrs[0];
    println!("PIT: {:.1}° asked, {:.1}° held", (start + 2.0 * DEG) / DEG, pitch / DEG);
    assert!((pitch - start - 2.0 * DEG).abs() < 0.5 * DEG);

    // Nose high with little power: MINSPD lowers it (the selected altitude far above, or ALTS would
    // capture it and only warn).
    f.gfc.set_selected_altitude(f.air_data().altitude + 5000.0 * FEET);
    f.throttle(0.3);
    for _ in 0..30 {
        f.press(&[AfcsKey::NoseUp]);
    }
    let mut slowest = f64::INFINITY;
    let mut seen = false;
    f.fly(120.0, |f, _| {
        slowest = slowest.min(f.air_data().airspeed / KNOTS);
        seen |= f.gfc.protection() == Some("MINSPD");
    });
    println!("PIT 17° up at a third of the power: slowest {slowest:.1} kt (MINSPD {:.0} kt)", f.gfc.fit().min_speed / KNOTS);
    assert!(seen && slowest > f.gfc.fit().min_speed / KNOTS - 4.0, "slowest {slowest}");
    assert!(f.vehicle().aircraft().unwrap().data().stall_warning.then_some(()).is_none());

    // Nose low with full power: MAXSPD raises it.
    f.gfc.set_selected_altitude(0.0);
    f.throttle(1.0);
    f.press(&[AfcsKey::Vs]);
    for _ in 0..30 {
        f.press(&[AfcsKey::NoseDown]);
    }
    let mut fastest = 0.0_f64;
    let mut seen = false;
    // Some 1500 ft down: the ground is 3000 ft below.
    f.fly(50.0, |f, _| {
        fastest = fastest.max(f.air_data().airspeed / KNOTS);
        seen |= f.gfc.protection() == Some("MAXSPD");
    });
    println!("VS −2000 ft/min at full power: fastest {fastest:.1} kt (MAXSPD {:.0} kt)", f.gfc.fit().max_speed / KNOTS);
    assert!(seen && fastest < f.gfc.fit().max_speed / KNOTS + 4.0, "fastest {fastest}");
    assert!(f.gfc.engaged());
}

#[test]
fn gps_flies_direct_to_an_aerodrome() {
    let mut f = Flight::new(1, 1.0);
    let airport = f.world.terrain.nearest_airport(30_000.0, 20_000.0).expect("an airport");
    f.press(&[AfcsKey::Ap, AfcsKey::Alt]);
    // NAV needs a plan.
    assert!(!f.gfc.press(AfcsKey::Nav));
    f.gfc.direct_to(&airport);
    f.press(&[AfcsKey::Nav]);
    assert_eq!(f.gfc.annunciations()[0], "GPS");
    let leg = f.gfc.plan().expect("a plan").active_leg().clone();
    let (mut off, mut closest) = (0.0_f64, f64::INFINITY);
    f.fly(900.0, |f, t| {
        let s = f.vehicle().aircraft().unwrap().state;
        let (across, _) = leg.place(s.pos.x, s.pos.y);
        if t > 60.0 {
            off = off.max(across.abs());
        }
        closest = closest.min((s.pos.x - airport.n).hypot(s.pos.y - airport.e));
    });
    println!("GPS D→ {} {:.1} km away: within {off:.0} m of the leg after 60 s, passed {closest:.0} m from it", leg.name, leg.place(leg.from.0, leg.from.1).1 / 1000.0);
    assert!(off < 50.0, "off the leg {off}");
    assert!(closest < 150.0, "missed by {closest}");
}

#[test]
fn nav_arms_away_from_the_leg_and_captures_it() {
    let mut f = Flight::new(1, 0.0);
    let airport = f.world.terrain.nearest_airport(30_000.0, 0.0).expect("an airport");
    f.press(&[AfcsKey::Ap, AfcsKey::Alt]);
    f.gfc.direct_to(&airport);
    let leg = f.gfc.plan().expect("a plan").active_leg().course();
    // Heading 90° off the leg, then 3 km to the side of it.
    f.gfc.set_heading_bug(leg + 90.0 * DEG);
    f.press(&[AfcsKey::Hdg]);
    f.fly(90.0, |_, _| {});
    f.gfc.set_heading_bug(leg - 45.0 * DEG);
    f.press(&[AfcsKey::Nav]);
    assert_eq!(f.gfc.annunciations()[..2], ["HDG", "GPS"]);
    let mut captured = None;
    f.fly(300.0, |f, t| {
        if captured.is_none() && f.gfc.lateral() == LateralMode::Gps {
            captured = Some(t);
        }
    });
    let s = f.vehicle().aircraft().unwrap().state;
    let (across, _) = f.gfc.plan().expect("a plan").active_leg().place(s.pos.x, s.pos.y);
    println!("NAV armed 45° to the leg: captured after {:.0} s, then {across:.0} m off it", captured.unwrap_or(-1.0));
    assert!(captured.is_some() && across.abs() < 50.0);
}

#[test]
fn apr_flies_the_gps_approach_down_the_glide_path() {
    let mut world = World::new(1);
    let airport = world.terrain.nearest_airport(0.0, 0.0).expect("home");
    // 12 NM out on the extended centre line, 45° off it, 2500 ft above the runway.
    let approach = Approach { runway: airport, reversed: false };
    let c = approach.course();
    let (n, e) = approach.point(12.0 * 1852.0);
    let n = n - 3000.0 * (c + 0.5 * std::f64::consts::PI).cos();
    let e = e - 3000.0 * (c + 0.5 * std::f64::consts::PI).sin();
    let id = world.add_aircraft(AircraftKind::C172, Start::Air { n, e, heading: c + 45.0 * DEG });
    let mut s = world.vehicle(id).aircraft().unwrap().state;
    s.pos.z = -(airport.elevation + 2000.0 * FEET + 500.0);
    world.place(id, s);
    let fit = world.vehicle(id).aircraft().unwrap().p.afcs.expect("GFC 700");
    let mut f = Flight { world, id, gfc: Gfc700::new(fit) };
    f.fly(1.0, |_, _| {});
    f.gfc.set_selected_altitude(f.air_data().altitude);
    f.press(&[AfcsKey::Ap, AfcsKey::Alt]);
    // Down to 2000 ft over the runway in VS before the fix.
    f.gfc.set_selected_altitude(f.air_data().altitude - 500.0);
    f.press(&[AfcsKey::Vs]);
    for _ in 0..5 {
        f.press(&[AfcsKey::NoseDown]);
    }
    f.gfc.load_approach(approach);
    f.press(&[AfcsKey::Apr]);
    assert!(f.gfc.glide_armed());
    f.throttle(0.62);
    let (mut gp_at, mut worst_path, mut worst_line, mut at_200) = (None, 0.0_f64, 0.0_f64, None);
    f.fly(900.0, |f, t| {
        let s = f.vehicle().aircraft().unwrap().state;
        let height = -s.pos.z - airport.elevation;
        if f.gfc.vertical() == VerticalMode::GlidePath {
            gp_at.get_or_insert(t);
        }
        if gp_at.is_some() && height > 200.0 * FEET && f.gfc.engaged() {
            let off = -s.pos.z - approach.path_altitude(s.pos.x, s.pos.y);
            let (across, _) = Leg { from: approach.point(5.0 * 1852.0), to: approach.point(0.0), name: String::new() }.place(s.pos.x, s.pos.y);
            if gp_at.is_some_and(|g| t > g + 30.0) {
                worst_path = worst_path.max(off.abs());
                worst_line = worst_line.max(across.abs());
            }
        }
        if at_200.is_none() && gp_at.is_some() && height < 200.0 * FEET {
            at_200 = Some((t, f.vehicle().aircraft().unwrap().data().ias / KNOTS));
        }
    });
    let (t200, speed) = at_200.expect("never down to 200 ft");
    println!("APR: GP captured at {:.0} s; on the path within {worst_path:.1} m and the centre line within {worst_line:.1} m; 200 ft at {t200:.0} s, {speed:.0} kt", gp_at.unwrap_or(-1.0));
    assert!(worst_path < 15.0, "path {worst_path}");
    assert!(worst_line < 30.0, "line {worst_line}");
}

#[test]
fn the_autopilot_is_disconnected_by_the_pilot_and_lets_go_by_itself() {
    let mut f = Flight::new(1, 0.0);
    // Not on the ground.
    let mut ground = World::new(1);
    let id = ground.add_aircraft(AircraftKind::C172, Start::Home);
    let mut on_ground = Gfc700::new(AircraftType::c172().afcs.unwrap());
    for _ in 0..2 {
        ground.fly(id, &mut on_ground);
        ground.step();
    }
    assert!(!on_ground.press(AfcsKey::Ap));

    // The AP key: normal disconnect, two seconds of tone, the flight director stays in its modes.
    f.press(&[AfcsKey::Ap, AfcsKey::Hdg, AfcsKey::Alt]);
    f.fly(5.0, |_, _| {});
    f.press(&[AfcsKey::Ap]);
    assert!(!f.gfc.engaged() && f.gfc.director());
    assert_eq!(f.gfc.annunciations()[0], "HDG");
    assert_eq!(f.gfc.alert().map(|a| a.why), Some(Disconnect::Pilot));
    f.fly(1.0, |f, _| assert!(f.gfc.tone()));
    f.fly(1.5, |_, _| {});
    assert!(!f.gfc.tone());
    f.fly(3.0, |_, _| {});
    assert!(f.gfc.alert().is_none());

    // A firm push on the yoke: a moment is not enough, half a second is.
    f.press(&[AfcsKey::Ap]);
    f.gfc.push_yoke(1.0, 0.3);
    f.gfc.push_yoke(0.0, 0.1);
    f.gfc.push_yoke(1.0, 0.3);
    assert!(f.gfc.engaged());
    f.gfc.push_yoke(0.2, 1.0);
    assert!(f.gfc.engaged());
    f.gfc.push_yoke(1.0, 0.6);
    assert!(!f.gfc.engaged());
    assert_eq!(f.gfc.alert().map(|a| a.why), Some(Disconnect::Overpowered));

    // The manual trim.
    f.press(&[AfcsKey::Ap]);
    f.gfc.manual_trim();
    assert_eq!(f.gfc.alert().map(|a| a.why), Some(Disconnect::ManualTrim));

    // Rolled past 45° (a gust, a wing lost): it lets go, and the tone goes on until AP DISC.
    f.press(&[AfcsKey::Ap]);
    let mut s = f.vehicle().aircraft().unwrap().state;
    s.q *= axis_angle(55.0 * DEG, V3::X);
    f.world.place(f.id, s);
    f.fly(0.5, |_, _| {});
    assert!(!f.gfc.engaged());
    assert_eq!(f.gfc.alert().map(|a| a.why), Some(Disconnect::Attitude));
    f.fly(30.0, |f, _| assert!(f.gfc.tone()));
    f.press(&[AfcsKey::Disconnect]);
    assert!(!f.gfc.tone() && f.gfc.alert().is_none());
    // Not engaged beyond its limits.
    let mut s = f.vehicle().aircraft().unwrap().state;
    s.q = quaternion(euler_angles(s.q).heading, 0.0, 55.0 * DEG);
    f.world.place(f.id, s);
    f.fly(World::DT, |_, _| {});
    assert!(!f.gfc.press(AfcsKey::Ap));

    // The flight director alone moves nothing; FD off is refused while the autopilot flies.
    let mut f = Flight::new(1, 0.0);
    f.press(&[AfcsKey::Fd, AfcsKey::Hdg]);
    let before = f.vehicle().aircraft().unwrap().controls;
    f.gfc.set_heading_bug(f.gfc.heading_bug() + 60.0 * DEG);
    f.fly(5.0, |_, _| {});
    assert_eq!(f.vehicle().aircraft().unwrap().controls, before);
    assert!(f.gfc.command().is_some_and(|(_, bank)| bank > 10.0 * DEG));
    f.press(&[AfcsKey::Ap]);
    assert!(!f.gfc.press(AfcsKey::Fd));
}

#[test]
fn the_altitude_alerter() {
    let mut f = Flight::new(1, 0.0);
    let start = f.air_data().altitude;
    f.gfc.set_selected_altitude(start + 1500.0 * FEET);
    f.throttle(1.0);
    f.press(&[AfcsKey::Ap, AfcsKey::Flc]);
    for _ in 0..25 {
        f.press(&[AfcsKey::NoseUp]);
    }
    let mut seen = Vec::new();
    f.fly(300.0, |f, _| {
        let a = f.gfc.altitude_alert();
        if seen.last() != Some(&a) {
            seen.push(a);
        }
    });
    println!("alerter climbing 1500 ft: {seen:?}");
    assert_eq!(seen, [AltitudeAlert::None, AltitudeAlert::Approaching, AltitudeAlert::None, AltitudeAlert::Within]);
    // 300 ft off it after being there: a deviation.
    f.press(&[AfcsKey::Vs]);
    for _ in 0..5 {
        f.press(&[AfcsKey::NoseDown]);
    }
    f.throttle(0.5);
    let mut deviated = false;
    f.fly(60.0, |f, _| deviated |= f.gfc.altitude_alert() == AltitudeAlert::Deviation);
    assert!(deviated);
}

#[test]
fn a_blocked_pitot_fools_flc_but_not_alt() {
    // ALT and HDG fly on the static ports and the attitude: a pitot blocked at the inlet (the
    // airspeed bleeding away) leaves them holding.
    let mut f = Flight::new(1, 1.0);
    f.press(&[AfcsKey::Ap, AfcsKey::Alt]);
    let VerticalMode::Altitude(held) = f.gfc.vertical() else { panic!("not ALT") };
    f.world.vehicle_mut(f.id).aircraft_mut().unwrap().instruments.failures.pitot = PitotBlock::Inlet;
    let mut off = 0.0_f64;
    f.fly(180.0, |f, _| off = off.max((f.air_data().altitude - held).abs() / FEET));
    println!("ALT with the pitot inlet blocked: within {off:.1} ft for 3 minutes, airspeed reading {:.0} kt", f.air_data().airspeed / KNOTS);
    assert!(off < 20.0 && f.gfc.engaged());

    // FLC climbing with the pitot sealed: the trapped pressure reads faster the higher it goes, so
    // the GFC raises the nose to slow down while the real airspeed falls away.
    let mut f = Flight::new(1, 1.0);
    f.throttle(1.0);
    f.gfc.set_selected_altitude(f.air_data().altitude + 5000.0 * FEET);
    f.press(&[AfcsKey::Ap, AfcsKey::Flc]);
    for _ in 0..25 {
        f.press(&[AfcsKey::NoseUp]);
    }
    f.fly(40.0, |_, _| {});
    f.world.vehicle_mut(f.id).aircraft_mut().unwrap().instruments.failures.pitot = PitotBlock::Sealed;
    let (mut lowest_true, mut read, mut disconnected) = (f64::INFINITY, 0.0, None);
    f.fly(300.0, |f, t| {
        let (_, _, ias, _) = f.truth();
        if f.gfc.engaged() {
            lowest_true = lowest_true.min(ias / KNOTS);
            read = f.air_data().airspeed / KNOTS;
        } else if disconnected.is_none() {
            disconnected = Some((t, f.gfc.alert().map(|a| a.why)));
        }
    });
    println!("FLC 75 kt with the pitot sealed: reads {read:.0} kt while the real airspeed falls to {lowest_true:.0} kt; disconnected {disconnected:?}");
    assert!(lowest_true < 60.0, "the real airspeed only fell to {lowest_true}");
    assert!(read > 70.0, "it read {read}");
}

#[test]
fn the_extra_has_no_gfc_700_and_the_c172_s_flight_is_unchanged() {
    assert!(Aircraft::of(AircraftKind::EXTRA300).p.afcs.is_none());
    // Deterministic: the same keys, the same flight.
    let run = || {
        let mut f = Flight::new(4, 0.3);
        f.press(&[AfcsKey::Ap, AfcsKey::Hdg, AfcsKey::Vs, AfcsKey::NoseUp]);
        f.gfc.set_heading_bug(1.5);
        f.fly(30.0, |_, _| {});
        f.vehicle().aircraft().unwrap().state
    };
    assert_eq!(run(), run());
}
