//! The instruments from their physics: the pitot-static system against the C172's airspeed
//! calibration, the standard atmosphere and the lag of a calibrated leak; blocked pitot and static,
//! the alternate source, pitot ice; the vacuum system and its gyros (erection, drift, running
//! down, tumbling); the compass's deviation, turning and acceleration errors and its swing; the
//! G1000's air data and attitude computers beside them. `-- --nocapture` prints the measurements.

mod common;

use common::Sim;
use sim_core::instruments::{ERECTION, ICING_BELOW, LOW_VACUUM, SUCTION_RATED, VSI_LEAK};
use sim_core::*;

const DT: f64 = World::DT;

/// One set of instruments driven by a flight written by hand.
struct Rig {
    instruments: Instruments,
    fit: InstrumentFit,
    air: Atmosphere,
    earth: Earth,
}

/// Steady flight: altitude (m), true airspeed (m/s), heading, pitch of the airflow (angle of
/// attack), bank in a coordinated turn, engine rpm.
#[derive(Clone, Copy)]
struct Flight {
    altitude: f64,
    tas: f64,
    heading: f64,
    alpha: f64,
    bank: f64,
    rpm: f64,
    /// Along the flight path, m/s².
    accel: f64,
    climb: f64,
    flap_deg: f64,
}

impl Default for Flight {
    fn default() -> Flight {
        Flight { altitude: 1000.0, tas: 50.0, heading: 0.0, alpha: 3.0 * DEG, bank: 0.0, rpm: 2300.0, accel: 0.0, climb: 0.0, flap_deg: 0.0 }
    }
}

impl Flight {
    /// The turn rate a coordinated turn at this bank has.
    fn turn_rate(&self) -> f64 {
        GRAVITY * self.bank.tan() / self.tas
    }

    fn sense(&self) -> Sense {
        let q = quaternion(self.heading, self.alpha, self.bank);
        let rates = act(inverse(q), V3::new(0.0, 0.0, self.turn_rate()));
        // Body axes: the airflow at the angle of attack; the climb as a vertical wind would give it.
        let climb_body = act(inverse(q), V3::new(0.0, 0.0, -self.climb));
        let airflow = V3::new(self.tas * self.alpha.cos(), 0.0, self.tas * self.alpha.sin()) + climb_body;
        // Accelerometer: along the path, towards the centre of the turn, and holding up the weight.
        let (along, right) = (V3::new(self.heading.cos(), self.heading.sin(), 0.0), V3::new(-self.heading.sin(), self.heading.cos(), 0.0));
        let specific = along * self.accel + right * (self.tas * self.turn_rate()) - V3::new(0.0, 0.0, GRAVITY);
        Sense { altitude: self.altitude, attitude: q, rates, airflow, specific: act(inverse(q), specific), rpm: self.rpm, flap_deg: self.flap_deg }
    }
}

impl Rig {
    fn new(fit: InstrumentFit, start: &Flight) -> Rig {
        let air = Atmosphere::STANDARD;
        let mut rig = Rig { instruments: Instruments::new(7), fit, air, earth: Earth::default() };
        rig.settle(start);
        rig
    }

    fn c172(start: &Flight) -> Rig {
        Rig::new(AircraftType::c172().instruments, start)
    }

    fn settle(&mut self, f: &Flight) {
        self.instruments.settle(&f.sense(), &self.fit, &self.air, &self.earth, self.air.qnh(0.0));
    }

    /// Flies `seconds`, the flight changing as `change` says after each step (it gets the time).
    fn fly(&mut self, f: &mut Flight, seconds: f64, mut change: impl FnMut(&mut Flight, f64)) {
        let steps = (seconds / DT).round() as usize;
        for i in 0..steps {
            change(f, i as f64 * DT);
            f.altitude += f.climb * DT;
            f.tas += f.accel * DT;
            f.heading = (f.heading + f.turn_rate() * DT).rem_euclid(360.0 * DEG);
            self.instruments.step(DT, &f.sense(), &self.fit, &self.air, &self.earth);
        }
    }

    fn r(&self) -> &Readings {
        self.instruments.readings()
    }
}

/// Signed difference of two angles, in (−π, π].
fn diff(a: f64, b: f64) -> f64 {
    remainder(a - b, 360.0 * DEG)
}

fn kt(v: f64) -> f64 {
    v / KNOTS
}

fn ft(v: f64) -> f64 {
    v / FEET
}

// ---- Pitot-static

/// Trimmed level at each KCAS of the C172S handbook's airspeed calibration, the airspeed
/// indicator reads its KIAS.
#[test]
fn the_c172_s_airspeed_reads_as_the_handbook_calibrates_it() {
    let aircraft = Aircraft::new();
    let rows: [(f64, &[(f64, f64)]); 3] = [
        (0.0, &[(50.0, 56.0), (60.0, 62.0), (70.0, 70.0), (80.0, 78.0), (90.0, 87.0), (100.0, 97.0), (110.0, 107.0), (120.0, 117.0), (130.0, 127.0)]),
        (10.0, &[(40.0, 51.0), (50.0, 57.0), (60.0, 63.0), (70.0, 71.0), (80.0, 80.0), (90.0, 89.0), (100.0, 99.0)]),
        (30.0, &[(40.0, 50.0), (50.0, 56.0), (60.0, 63.0), (70.0, 72.0), (80.0, 81.0)]),
    ];
    for (flaps, points) in rows {
        for &(kias, kcas) in points {
            let trim = aircraft.trim(kcas * KNOTS, 0.0, flaps, None, 0.0).expect("trims");
            let f = Flight { altitude: 0.0, tas: kcas * KNOTS, alpha: trim.alpha, flap_deg: flaps, ..Flight::default() };
            let rig = Rig::c172(&f);
            let shown = kt(rig.r().airspeed);
            println!("flaps {flaps:2}: {kcas:3} KCAS, alpha {:5.1}°: reads {shown:5.1} KIAS (handbook {kias})", trim.alpha / DEG);
            assert!((shown - kias).abs() < 1.0, "flaps {flaps}, {kcas} KCAS reads {shown} KIAS, not {kias}");
        }
    }
    // The Extra's ports, both sides and uncalibrated, read true: KIAS is KCAS.
    let f = Flight { altitude: 0.0, tas: 100.0 * KNOTS, alpha: 3.0 * DEG, ..Flight::default() };
    let rig = Rig::new(AircraftType::extra300().instruments, &f);
    assert!((kt(rig.r().airspeed) - 100.0).abs() < 0.2, "{}", kt(rig.r().airspeed));
}

/// Indicated is calibrated airspeed: at altitude the true airspeed is higher by the density, and
/// the air data computer works it out again from the pressures and the outside air temperature.
#[test]
fn indicated_airspeed_falls_behind_true_with_altitude() {
    let f = Flight { altitude: 3048.0, tas: 120.0 * KNOTS, alpha: 5.38 * DEG, ..Flight::default() };
    let rig = Rig::c172(&f);
    let sigma = atmosphere::density(3048.0) / atmosphere::SEA_LEVEL_DENSITY;
    let equivalent = 120.0 * sigma.sqrt();
    let (ias, tas) = (kt(rig.r().airspeed), kt(rig.r().air_data.true_airspeed));
    println!("10 000 ft, 120 KTAS: ASI {ias:.1} kt (EAS {equivalent:.1}), ADC TAS {tas:.1}, OAT {:.1} °C", rig.r().air_data.temperature - 273.15);
    // Compressibility adds a little to CAS over EAS (about 0.5 kt here).
    assert!(ias > equivalent && ias < equivalent + 1.5, "{ias} against EAS {equivalent}");
    assert!((tas - 120.0).abs() < 0.5, "{tas}");
}

/// Set to QNH on a standard day the altimeter reads the true altitude; set 10 hPa too high it
/// reads some 265 ft high (26 ft a hectopascal there); on a day 15 °C colder than standard it reads high by the rule of
/// thumb, 4 % per 10 °C of the height above the station.
#[test]
fn the_altimeter_reads_the_static_pressure_against_its_setting() {
    let f = Flight { altitude: 1500.0, ..Flight::default() };
    let mut rig = Rig::new(AircraftType::extra300().instruments, &f);
    assert!((rig.r().altitude - 1500.0).abs() < 0.5, "{}", rig.r().altitude);
    rig.instruments.altimeter_setting += 10.0 * HPA;
    rig.fly(&mut f.clone(), 2.0, |_, _| {});
    let high = ft(rig.r().altitude - 1500.0);
    println!("10 hPa high: {high:.0} ft high");
    assert!((high - 265.0).abs() < 8.0, "{high}");

    let cold = Atmosphere { sea_level_temperature: atmosphere::SEA_LEVEL_TEMPERATURE - 15.0, ..Atmosphere::STANDARD };
    let mut rig = Rig::new(AircraftType::extra300().instruments, &f);
    rig.air = cold;
    rig.settle(&f);
    let error = rig.r().altitude - 1500.0;
    let rule = 1500.0 * 15.0 / cold.sea_level_temperature;
    println!("15 °C cold, 1500 m true: reads {:.0} m ({error:.0} m high; rule {rule:.0})", rig.r().altitude);
    assert!(error > 0.8 * rule && error < 1.2 * rule, "{error} against {rule}");
    // And the aircraft flies in that air: denser.
    assert!(cold.density(1500.0) > atmosphere::density(1500.0));
}

/// An aerodrome reports a QNH with which the altimeter reads its elevation on the ground, warm day
/// or cold.
#[test]
fn qnh_makes_the_altimeter_read_the_elevation() {
    for air in [
        Atmosphere::STANDARD,
        Atmosphere { sea_level_pressure: 99_800.0, sea_level_temperature: 268.0, ..Atmosphere::STANDARD },
        Atmosphere { sea_level_pressure: 102_900.0, sea_level_temperature: 306.0, ..Atmosphere::STANDARD },
    ] {
        let elevation = 650.0;
        let reads = atmosphere::pressure_altitude(air.pressure(elevation), air.qnh(elevation));
        assert!((reads - elevation).abs() < 0.01, "{air:?}: {reads}");
    }
}

/// The vertical speed indicator lags as its leak makes it: 63 % of a new rate after its time
/// constant, about 95 % after 7.5 s; the air data computer within 2.5 s. Higher up the capillary
/// leaks slower (its lag grows as the pressure falls) but holds its calibration within a few percent.
#[test]
fn the_vertical_speed_lags_by_its_calibrated_leak() {
    for altitude in [300.0, 3000.0] {
        let mut f = Flight { altitude, ..Flight::default() };
        let mut rig = Rig::new(AircraftType::extra300().instruments, &f);
        let rate = 1000.0 * FEET / 60.0;
        let mut at = Vec::new();
        let mut adc_90 = None;
        for k in 0..60 {
            rig.fly(&mut f, 0.5, |f, _| f.climb = rate);
            let t = (k + 1) as f64 * 0.5;
            at.push((t, rig.r().vertical_speed / rate));
            if adc_90.is_none() && rig.r().air_data.vertical_speed > 0.9 * rate {
                adc_90 = Some(t);
            }
        }
        let share = |t: f64| at.iter().find(|p| (p.0 - t).abs() < 1e-9).unwrap().1;
        println!(
            "{altitude:4} m: VSI {:.0} % at {VSI_LEAK} s, {:.0} % at 5 s, {:.0} % at 7.5 s, {:.1} % at 30 s; ADC 90 % at {:?} s",
            share(2.5) * 100.0,
            share(5.0) * 100.0,
            share(7.5) * 100.0,
            share(30.0) * 100.0,
            adc_90
        );
        if altitude < 1000.0 {
            assert!(share(2.5) > 0.5 && share(2.5) < 0.65, "{}", share(2.5));
            assert!(share(7.5) > 0.9 && share(7.5) < 0.97, "{}", share(7.5));
        }
        assert!((share(30.0) - 1.0).abs() < 0.03, "{}", share(30.0));
        if altitude > 1000.0 {
            assert!(share(7.5) < 0.92, "the lag grows with height: {}", share(7.5));
        }
        assert!(adc_90.is_some_and(|t| t <= 2.5), "{adc_90:?}");
    }
}

/// A sharp pull-up raises the angle of attack and with it the pressure at the static port: for a
/// moment the vertical speed indicator shows a descent and the altimeter a little loss.
#[test]
fn a_pull_up_first_shows_a_descent() {
    let mut f = Flight { tas: 100.0 * KNOTS, alpha: 1.0 * DEG, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    let mut least = 0.0f64;
    rig.fly(&mut f, 1.0, |f, t| f.alpha = (1.0 + 8.0 * (t / 0.3).min(1.0)) * DEG);
    least = least.min(rig.r().vertical_speed);
    println!("pull-up: VSI {:.0} ft/min, altimeter {:+.0} ft", rig.r().vertical_speed / FEET * 60.0, ft(rig.r().altitude - 1000.0));
    assert!(least < -50.0 * FEET / 60.0, "{least}");
    assert!(rig.r().altitude < 1000.0 - 5.0 * FEET);
}

/// Ice seals the pitot (inlet and drain): what is trapped stays, so the airspeed indicator reads
/// higher as the aircraft climbs and the static falls, lower as it descends, and does not see a
/// change of speed. The air data computer reads the same lines.
#[test]
fn a_sealed_pitot_turns_the_airspeed_into_an_altimeter() {
    let mut f = Flight { tas: 100.0 * KNOTS, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    let before = kt(rig.r().airspeed);
    rig.instruments.failures.pitot = PitotBlock::Sealed;
    rig.fly(&mut f, 30.0, |f, _| f.tas = 80.0 * KNOTS);
    let slower = kt(rig.r().airspeed);
    rig.fly(&mut f, 60.0, |f, _| f.climb = 5.0);
    let climbed = kt(rig.r().airspeed);
    rig.fly(&mut f, 120.0, |f, _| f.climb = -5.0);
    let descended = kt(rig.r().airspeed);
    println!("sealed pitot at {before:.0} kt: slowed to 80 kt reads {slower:.0}; 300 m higher {climbed:.0}; 300 m lower than at first {descended:.0}");
    // Only the static port's error moves it, with the change of speed.
    assert!((slower - before).abs() < 2.0);
    assert!(climbed > before + 30.0, "{climbed}");
    assert!(descended < before - 20.0, "{descended}");
    assert!((kt(rig.r().air_data.airspeed) - descended).abs() < 0.5);
}

/// With only the inlet blocked the drain bleeds the pitot line down to static: the airspeed falls
/// to nothing in some seconds.
#[test]
fn a_blocked_inlet_drains_the_airspeed_away() {
    let mut f = Flight { tas: 100.0 * KNOTS, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    rig.instruments.failures.pitot = PitotBlock::Inlet;
    rig.fly(&mut f, 5.0, |_, _| {});
    let five = kt(rig.r().airspeed);
    rig.fly(&mut f, 15.0, |_, _| {});
    let twenty = kt(rig.r().airspeed);
    println!("inlet blocked: {five:.0} kt after 5 s, {twenty:.0} kt after 20 s");
    // What is left is the static port's own error against the ambient static the drain bleeds to.
    assert!(five < 70.0 && twenty < 30.0, "{five} {twenty}");
    let mut f = Flight { tas: 100.0 * KNOTS, ..Flight::default() };
    let mut rig = Rig::new(AircraftType::extra300().instruments, &f);
    rig.instruments.failures.pitot = PitotBlock::Inlet;
    rig.fly(&mut f, 20.0, |_, _| {});
    assert!(kt(rig.r().airspeed) < 10.0, "{}", kt(rig.r().airspeed));
}

/// Blocked static ports: altimeter frozen, vertical speed back to zero, the airspeed low in a
/// climb and high in a descent. The alternate static source brings them back, reading a little
/// high from the cabin's lower pressure.
#[test]
fn blocked_static_freezes_the_altimeter_and_the_alternate_source_frees_it() {
    let mut f = Flight { tas: 90.0 * KNOTS, alpha: 3.5 * DEG, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    let (altitude, airspeed) = (rig.r().altitude, kt(rig.r().airspeed));
    rig.instruments.failures.static_blocked = true;
    rig.fly(&mut f, 60.0, |f, _| f.climb = 4.0);
    let (frozen, vsi, climbing) = (rig.r().altitude, rig.r().vertical_speed, kt(rig.r().airspeed));
    println!("static blocked: climbed 240 m, altimeter {:+.1} m, VSI {:.0} ft/min, ASI {climbing:.0} kt (was {airspeed:.0})", frozen - altitude, vsi / FEET * 60.0);
    assert!((frozen - altitude).abs() < 0.1 && vsi.abs() < 0.05);
    assert!(climbing < airspeed - 5.0, "{climbing}");
    rig.fly(&mut f, 120.0, |f, _| f.climb = -4.0);
    let descending = kt(rig.r().airspeed);
    assert!(descending > airspeed + 5.0, "{descending}");
    // Alternate static: the cabin's pressure.
    rig.instruments.alternate_static = true;
    rig.fly(&mut f, 20.0, |f, _| f.climb = 0.0);
    let (high, fast) = (ft(rig.r().altitude - f.altitude), kt(rig.r().airspeed) - airspeed);
    println!("alternate static: altimeter {high:+.0} ft, airspeed {fast:+.1} kt");
    assert!(high > 10.0 && high < 60.0, "{high}");
    assert!(fast > 1.0 && fast < 6.0, "{fast}");
}

/// Below freezing in cloud, ice seals an unheated pitot within a minute; the heat clears it; above
/// freezing nothing forms.
#[test]
fn pitot_ice_and_pitot_heat() {
    let freezing = 3500.0; // −7.75 °C on the standard day.
    assert!(atmosphere::temperature(freezing) < ICING_BELOW);
    let mut f = Flight { altitude: freezing, tas: 100.0 * KNOTS, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    rig.instruments.failures.icing = true;
    rig.fly(&mut f, 50.0, |_, _| {});
    assert!(rig.instruments.pitot_ice() >= 1.0);
    let sealed = kt(rig.r().airspeed);
    rig.fly(&mut f, 20.0, |f, _| f.climb = 5.0);
    assert!(kt(rig.r().airspeed) > sealed + 5.0, "the sealed pitot does not act as an altimeter");
    rig.instruments.pitot_heat = true;
    rig.fly(&mut f, 25.0, |f, _| f.climb = 0.0);
    assert_eq!(rig.instruments.pitot_ice(), 0.0);
    assert!((kt(rig.r().airspeed) - kt(Rig::c172(&f).r().airspeed)).abs() < 0.5);

    let mut warm = Flight { altitude: 500.0, ..Flight::default() };
    let mut rig = Rig::c172(&warm);
    rig.instruments.failures.icing = true;
    rig.fly(&mut warm, 60.0, |_, _| {});
    assert_eq!(rig.instruments.pitot_ice(), 0.0);
}

/// The pitot reads the total pressure to 20° off its axis and nothing past 65°: in a spin or a
/// tail slide the airspeed reads little or nothing.
#[test]
fn the_pitot_stalls_far_off_its_axis() {
    for (alpha, low, high) in [(15.0, 99.5, 100.5), (40.0, 30.0, 85.0), (70.0, 0.0, 0.5)] {
        let f = Flight { altitude: 0.0, tas: 100.0 * KNOTS, alpha: alpha * DEG, ..Flight::default() };
        let rig = Rig::new(AircraftType::extra300().instruments, &f);
        let shown = kt(rig.r().airspeed);
        println!("{alpha}° off the pitot: {shown:.1} kt");
        assert!(shown >= low && shown <= high, "{alpha}: {shown}");
    }
}

// ---- Vacuum and gyros

/// About 4.4 inHg at idle, the regulated 5 from some 1000 rpm; the pump gone, nothing, and the
/// LOW VACUUM annunciation. Without a pump (the Extra), no gauge.
#[test]
fn the_vacuum_pump_s_suction_follows_the_rpm() {
    let inhg = |rpm: f64| {
        let f = Flight { rpm, ..Flight::default() };
        Rig::c172(&f).r().suction.unwrap() / INHG
    };
    println!("suction: {:.2} inHg at 700 rpm, {:.2} at 1000, {:.2} at 2300", inhg(700.0), inhg(1000.0), inhg(2300.0));
    assert!((inhg(700.0) - 4.4).abs() < 0.3);
    assert!((inhg(1000.0) - 5.0).abs() < 0.05 && (inhg(2300.0) - 5.0).abs() < 1e-9);
    let mut f = Flight::default();
    let mut rig = Rig::c172(&f);
    assert!(!rig.r().low_vacuum);
    rig.instruments.failures.vacuum_pump = true;
    rig.fly(&mut f, 5.0, |_, _| {});
    assert!(rig.r().suction.unwrap() < 0.1 * INHG && rig.r().low_vacuum && LOW_VACUUM < SUCTION_RATED);
    assert!(Rig::new(AircraftType::extra300().instruments, &f).r().suction.is_none());
}

/// After a full standard-rate turn the erection, working towards a vertical that is tilted all the
/// way round, leaves the horizon only a little off; an acceleration tilts it at the erection rate.
#[test]
fn the_attitude_gyro_erects_to_the_felt_vertical() {
    let mut f = Flight { tas: 95.0 * KNOTS, ..Flight::default() };
    let bank = (3.0 * DEG * f.tas / GRAVITY).atan();
    let mut rig = Rig::c172(&f);
    rig.fly(&mut f, 5.0, |f, t| f.bank = bank * (t / 5.0));
    rig.fly(&mut f, 120.0, |f, _| f.bank = bank);
    rig.fly(&mut f, 5.0, |f, t| f.bank = bank * (1.0 - t / 5.0));
    rig.fly(&mut f, 1.0, |f, _| f.bank = 0.0);
    let [pitch, roll] = rig.r().attitude.unwrap();
    println!("after a 360° turn at {:.0}° of bank: horizon {:+.1}° pitch {:+.1}° roll", bank / DEG, (pitch - f.alpha) / DEG, roll / DEG);
    assert!((pitch - f.alpha).abs() < 3.0 * DEG && roll.abs() < 3.0 * DEG);

    // Accelerating, the felt vertical leans back: the horizon shows a climb, growing at the
    // erection rate; steady again, it comes back at the same rate.
    let mut f = Flight { tas: 40.0, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    rig.fly(&mut f, 60.0, |f, _| f.accel = 1.0);
    let climb = rig.r().attitude.unwrap()[0] - f.alpha;
    rig.fly(&mut f, 60.0, |f, _| f.accel = 0.0);
    let back = rig.r().attitude.unwrap()[0] - f.alpha;
    println!("a minute at 1 m/s² of acceleration: horizon {:+.1}° high; a minute steady: {:+.1}°", climb / DEG, back / DEG);
    assert!((climb - ERECTION * 60.0).abs() < 0.3 * DEG && back.abs() < 0.3 * DEG);
}

/// The heading gyro drifts by its own few degrees an hour and is set again to the compass.
#[test]
fn the_heading_gyro_drifts_and_is_set_to_the_compass() {
    let mut f = Flight { heading: 70.0 * DEG, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    let start = rig.r().heading_indicator.unwrap();
    assert!(diff(start, rig.r().compass).abs() < 0.01 * DEG);
    rig.fly(&mut f, 900.0, |_, _| {});
    let drift = diff(rig.r().heading_indicator.unwrap(), start);
    println!("heading gyro: {:+.2}° in 15 min", drift / DEG);
    assert!(drift.abs() > 0.9 * DEG && drift.abs() < 2.6 * DEG, "{}", drift / DEG);
    rig.instruments.align_heading_indicator(f.sense().attitude);
    assert!(diff(rig.instruments.readings().heading_indicator.unwrap(), rig.r().compass).abs() < 0.01 * DEG);
    // At another latitude the Earth's rotation, which the latitude nut only takes off at its own.
    let mut f = Flight { heading: 70.0 * DEG, ..Flight::default() };
    let mut north = Rig::c172(&f);
    north.earth.latitude = 60.0 * DEG;
    north.settle(&f);
    let start = north.r().heading_indicator.unwrap();
    north.fly(&mut f, 900.0, |_, _| {});
    let more = diff(north.r().heading_indicator.unwrap(), start) - drift;
    let earth = -7.2921e-5 * (60.0f64.to_radians().sin() - 40.0f64.to_radians().sin()) * 900.0;
    println!("at 60° N: {:+.2}° more ({:+.2}° from the Earth)", more / DEG, earth / DEG);
    assert!((more - earth).abs() < 0.05 * DEG);
}

/// The vacuum pump fails: the suction goes at once, the gyros run down over minutes. Then the
/// horizon leans off and the heading gyro is dragged round with the aircraft, while the G1000's
/// attitude and heading, electric, stay exact.
#[test]
fn without_vacuum_the_gyros_run_down_and_the_ahrs_keeps_working() {
    let mut f = Flight::default();
    let mut rig = Rig::c172(&f);
    rig.instruments.failures.vacuum_pump = true;
    rig.fly(&mut f, 60.0, |_, _| {});
    let [ai, hi] = rig.instruments.gyro_spin();
    let roll_one = rig.r().attitude.unwrap()[1];
    println!("1 min without vacuum: rotors at {:.0} % and {:.0} %, horizon roll {:+.1}°", ai * 100.0, hi * 100.0, roll_one / DEG);
    assert!(ai > 0.7 && roll_one.abs() < 1.0 * DEG, "still usable after a minute");
    rig.fly(&mut f, 540.0, |_, _| {});
    let [ai, _] = rig.instruments.gyro_spin();
    let [pitch, roll] = rig.r().attitude.unwrap();
    println!("10 min: rotor at {:.0} %, horizon {:+.0}° pitch {:+.0}° roll", ai * 100.0, (pitch - f.alpha) / DEG, roll / DEG);
    assert!(ai < 0.2 && ((pitch - f.alpha).abs() > 10.0 * DEG || roll.abs() > 10.0 * DEG));
    // A 90° turn: the heading gyro hardly follows; the AHRS and the compass do.
    let before = rig.r().heading_indicator.unwrap();
    let bank = 20.0 * DEG;
    rig.fly(&mut f, 200.0, |f, _| f.bank = if diff(f.heading, 0.0) < 90.0 * DEG && diff(f.heading, 0.0) >= 0.0 { bank } else { 0.0 });
    rig.fly(&mut f, 10.0, |f, _| f.bank = 0.0);
    let gyro_turned = diff(rig.r().heading_indicator.unwrap(), before);
    let ahrs = rig.r().ahrs;
    println!("then a 90° turn: heading gyro {:+.0}°, AHRS {:.0}°, compass {:.0}°", gyro_turned / DEG, ahrs[2] / DEG, rig.r().compass / DEG);
    assert!(diff(f.heading, 90.0 * DEG).abs() < 3.0 * DEG);
    assert!(gyro_turned.abs() < 70.0 * DEG && diff(ahrs[2], f.heading).abs() < 1e-9);
    assert!((ahrs[0] - f.alpha).abs() < 1e-9 && ahrs[1].abs() < 1e-9);
}

/// Pitching past its stops (85°) the attitude gyro tumbles and is left far off, to erect again
/// at a few degrees a minute: the Extra's electric one in a vertical line.
#[test]
fn past_its_stops_the_attitude_gyro_tumbles() {
    let mut f = Flight { tas: 120.0 * KNOTS, alpha: 0.0, ..Flight::default() };
    let mut rig = Rig::new(AircraftType::extra300().instruments, &f);
    rig.fly(&mut f, 3.0, |f, t| f.alpha = (90.0 * (t / 2.0).min(1.0)) * DEG);
    rig.fly(&mut f, 3.0, |f, t| f.alpha = (90.0 * (1.0 - t / 2.0).max(0.0)) * DEG);
    let [pitch, roll] = rig.r().attitude.unwrap();
    println!("after a vertical line: horizon {:+.0}° pitch, {:+.0}° roll", pitch / DEG, roll / DEG);
    assert!(pitch.abs() > 20.0 * DEG || roll.abs() > 20.0 * DEG);
    // Up to 80° it does not.
    let mut f = Flight { tas: 120.0 * KNOTS, alpha: 0.0, ..Flight::default() };
    let mut rig = Rig::new(AircraftType::extra300().instruments, &f);
    rig.fly(&mut f, 3.0, |f, t| f.alpha = (80.0 * (t / 2.0).min(1.0)) * DEG);
    rig.fly(&mut f, 3.0, |f, t| f.alpha = (80.0 * (1.0 - t / 2.0).max(0.0)) * DEG);
    let [pitch, roll] = rig.r().attitude.unwrap();
    assert!(pitch.abs() < 1.0 * DEG && roll.abs() < 1.0 * DEG, "{pitch} {roll}");
}

// ---- Compass

/// Level and steady the compass reads the magnetic heading but for its deviation, a few degrees
/// that change with the heading.
#[test]
fn the_compass_s_deviation_is_a_few_degrees() {
    let mut worst = 0.0f64;
    let mut card = Vec::new();
    for k in 0..12 {
        let heading = k as f64 * 30.0 * DEG;
        let rig = Rig::c172(&Flight { heading, alpha: 0.0, ..Flight::default() });
        let deviation = diff(rig.r().compass, heading);
        card.push(format!("{:03.0}:{:+.1}", heading / DEG, deviation / DEG));
        worst = worst.max(deviation.abs());
    }
    println!("deviation card: {}", card.join(" "));
    assert!(worst > 0.5 * DEG && worst < 4.0 * DEG, "{}", worst / DEG);
}

/// The northerly turning error: in a turn the card tilts with the aircraft and the dip of the field
/// swings it; turning through north it shows a heading well short of north (it lags), through
/// south well past it (it leads).
#[test]
fn the_compass_lags_through_north_and_leads_through_south() {
    let bank = 15.0 * DEG;
    let mut errors = Vec::new();
    for through in [0.0, 180.0 * DEG] {
        let mut f = Flight { heading: through - 60.0 * DEG, alpha: 0.0, ..Flight::default() };
        let mut rig = Rig::c172(&f);
        rig.fly(&mut f, 3.0, |f, t| f.bank = bank * (t / 3.0));
        // On round the turn until the true heading is the cardinal point: read the compass there.
        let mut seen = None;
        for _ in 0..(40.0 / DT) as usize {
            rig.fly(&mut f, DT, |f, _| f.bank = bank);
            if seen.is_none() && diff(f.heading, through).abs() < 0.2 * DEG {
                seen = Some(diff(rig.r().compass, through));
            }
        }
        let error = seen.expect("passes the heading");
        println!("turning right through {:03.0}° at 15° of bank: compass {:+.0}°", through / DEG, error / DEG);
        errors.push(error);
    }
    // Lags (reads behind, to the left) through north, leads through south, by tens of degrees.
    assert!(errors[0] < -10.0 * DEG && errors[1] > 10.0 * DEG, "{errors:?}");
}

/// The acceleration error (ANDS): heading east, accelerating swings the card towards north,
/// decelerating towards south; on north it does nothing.
#[test]
fn accelerating_on_east_the_compass_shows_a_turn_north() {
    let read = |heading: f64, accel: f64| {
        let mut f = Flight { heading, alpha: 0.0, ..Flight::default() };
        let mut rig = Rig::c172(&f);
        let steady = rig.r().compass;
        rig.fly(&mut f, 4.0, |f, _| f.accel = accel);
        diff(rig.r().compass, steady)
    };
    let (faster, slower, north) = (read(90.0 * DEG, 1.5), read(90.0 * DEG, -1.5), read(0.0, 1.5));
    println!("heading east: {:+.1}° accelerating, {:+.1}° slowing; heading north {:+.1}°", faster / DEG, slower / DEG, north / DEG);
    assert!(faster < -5.0 * DEG && slower > 5.0 * DEG && north.abs() < 1.0 * DEG);
}

/// The card swings past and is damped by its fluid: after a sudden 30° change it overshoots and
/// has settled within a few seconds.
#[test]
fn the_compass_card_swings_and_settles() {
    let mut f = Flight { heading: 0.0, alpha: 0.0, ..Flight::default() };
    let mut rig = Rig::c172(&f);
    let start = rig.r().compass;
    f.heading = 30.0 * DEG;
    let mut most = 0.0f64;
    let mut settled = None;
    for k in 0..(10.0 / DT) as usize {
        rig.fly(&mut f, DT, |_, _| {});
        let moved = diff(rig.r().compass, start);
        most = most.max(moved);
        if settled.is_none() && k as f64 * DT > 1.0 && (moved - 30.0 * DEG).abs() < 0.5 * DEG {
            settled = Some(k as f64 * DT);
        }
    }
    println!("30° step: overshoot {:.1}°, within half a degree after {settled:?} s", (most - 30.0 * DEG) / DEG);
    assert!(most > 33.0 * DEG && most < 45.0 * DEG && settled.is_some_and(|t| t < 6.0));
}

// ---- In the world

/// The world's aircraft carry their instruments: settled at each placement (altimeter on the
/// aerodrome's QNH, heading gyro on the compass), seen in the observation, the same for the same
/// seed. The air of the day is the world's: a colder day, a denser air to fly in.
#[test]
fn the_world_s_aircraft_read_their_instruments() {
    let mut sim = Sim::new(3);
    let home = sim.vehicle().home.expect("an aerodrome");
    let r = *sim.vehicle().aircraft().unwrap().instruments.readings();
    assert!((r.altitude - sim.vehicle().aircraft().unwrap().state.pos.z.abs()).abs() < 1.0, "on the ground the altimeter reads the elevation");
    assert!((r.altimeter_setting - sim.world.weather.air.qnh(home.elevation)).abs() < 1e-6);
    sim.reset_in_air(0.0, 0.0, 1.0);
    for _ in 0..(20.0 / DT) as usize {
        sim.step();
    }
    let obs = sim.world.observe(sim.id);
    let r = obs.aircraft().unwrap().instruments;
    println!("in flight: ASI {:.1} kt (ideal {:.1}), altimeter {:.0} ft (true {:.0}), VSI {:+.0} ft/min", kt(r.airspeed), kt(obs.aircraft().unwrap().indicated_airspeed), ft(r.altitude), ft(obs.altitude), r.vertical_speed / FEET * 60.0);
    assert!((kt(r.airspeed) - kt(obs.aircraft().unwrap().indicated_airspeed)).abs() < 4.0);
    assert!((r.altitude - obs.altitude).abs() < 15.0);
    let [pitch, roll] = r.attitude.expect("an attitude indicator");
    println!("horizon {:+.2}° {:+.2}° against {:+.2}° {:+.2}°", pitch / DEG, roll / DEG, obs.pitch / DEG, obs.roll / DEG);
    // Off by no more than the erection can have pulled it towards the felt vertical in a slip.
    let most = ERECTION * 20.0;
    assert!((pitch - obs.pitch).abs() < most && (roll - obs.roll).abs() < most);
    assert!(r.heading_indicator.is_some() && r.suction.is_some());
    let mut again = Sim::new(3);
    again.reset_in_air(0.0, 0.0, 1.0);
    for _ in 0..(20.0 / DT) as usize {
        again.step();
    }
    assert_eq!(again.world.observe(again.id).aircraft().unwrap().instruments, r);

    let mut cold = Sim::new(3);
    cold.world.weather.air.sea_level_temperature -= 20.0;
    cold.step();
    let colder = cold.vehicle().environment().rho;
    assert!(colder > Sim::new(3).vehicle().environment().rho * 1.05);
}
