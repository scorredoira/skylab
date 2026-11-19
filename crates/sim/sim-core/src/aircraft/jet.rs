//! Turbofan engines: what a jet aircraft has in place of the piston engine and its propeller
//! (`AircraftType::jets`). Each engine is its fan's speed, N1, as a share of the take-off rating's
//! (`State::n1`); the throttle asks for one, the engine spools to it at the rate a big fan does
//! (slowly from idle, briskly near the top), and its thrust, its airflow and its fuel come from
//! that speed and the air it breathes.
//!
//! The thrust is the momentum the engine gives the air: what it takes in at the flight speed
//! (the ram drag) leaves faster through the nozzle (the gross thrust). The two streams of the real
//! engine, the fan's cold one and the core's hot one, are one here: an airflow and a pressure
//! ratio that grow with the fan's corrected speed, a little warmer than the fan alone would leave
//! it for the core's share. That is enough for the thrust to fall as it does with speed on the
//! runway (the ram drag grows, the nozzle gains little) and with height (the air thins; colder, it
//! lets the fan turn faster in corrected terms), for idle thrust to become drag at high speed, and
//! for a dead engine to drag as it windmills. Fuel is the jet's power over the engine's thermal
//! efficiency, which is better in cold air, plus what keeps it turning at idle.

use crate::atmosphere::{GAMMA, GAS_CONSTANT, SEA_LEVEL_PRESSURE, SEA_LEVEL_TEMPERATURE};
use crate::math::{clamp, V3};

/// The most engines a jet aircraft has.
pub const MOST_ENGINES: usize = 4;

/// Specific heat of air at constant pressure, J/(kg K).
const CP: f64 = 1005.0;
/// (γ − 1) / γ.
const KAPPA: f64 = (GAMMA - 1.0) / GAMMA;
/// Efficiency of the compression the equivalent stream goes through.
const COMPRESSION: f64 = 0.88;
/// Heat of combustion of jet fuel, J/kg.
const FUEL_HEAT: f64 = 43.0e6;
/// Share of its speed the air keeps through an engine that windmills.
const WINDMILL_RECOVERY: f64 = 0.4;

/// Where an engine hangs: the point its thrust acts at and the lowest point of its nacelle, which
/// may touch the ground (body axes, from the reference centre of gravity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mount {
    pub at: V3,
    pub pod: V3,
}

/// The thrust reverser: it turns part of the gross thrust forward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reverser {
    /// Share of the gross thrust that pushes backwards once it is open.
    pub share: f64,
    /// Seconds it takes to open or close.
    pub time: f64,
    /// The highest N1 with it open.
    pub limit: f64,
}

/// The engines of a jet aircraft: all of one kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Jets {
    /// One per engine, left to right: at most `MOST_ENGINES`.
    pub mounts: &'static [Mount],
    /// Thrust of one engine standing still at sea level on a standard day, at N1 = 1, N.
    pub static_thrust: f64,
    /// Air it takes in there, kg/s.
    pub airflow: f64,
    /// N1 at idle on the ground and, higher, in the air (so it spools up in time for a go-around).
    pub idle: f64,
    pub flight_idle: f64,
    /// The highest N1 it is ever allowed.
    pub redline: f64,
    /// How much more corrected speed the ratings allow in thin air: full throttle is a corrected
    /// N1 of 1 at sea level and of 1 + this where there is no air.
    pub altitude_rating: f64,
    /// How the pressure ratio and the airflow grow with the corrected speed (their exponents), and
    /// how much warmer than the fan leaves it the jet is, at a corrected speed of 1, for the core.
    pub pressure_exponent: f64,
    pub flow_exponent: f64,
    pub heat: f64,
    /// How fast it spools up, as N1 per second: this with the fan stopped, and this more per N1².
    pub spool: (f64, f64),
    /// Fuel that keeps it turning at idle at sea level, kg/s; thermal efficiency at sea level on a
    /// standard day, and how much it gains per unit the total temperature falls below that.
    pub fuel_idle: f64,
    pub efficiency: f64,
    pub efficiency_cold: f64,
    pub reverser: Option<Reverser>,
    /// The fan: its diameter, m, and its speed at N1 = 1, rpm (what it sounds like, what the gauge shows).
    pub fan_diameter: f64,
    pub n1_rpm: f64,
    /// Pressure ratio of the equivalent stream at a corrected speed of 1: what gives `static_thrust`
    /// with `airflow`. Worked out by `Jets::rated`.
    pub pressure_ratio: f64,
}

/// The air an engine breathes: static pressure (Pa) and temperature (K), and its speed along the
/// engine's axis, m/s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intake {
    pub pressure: f64,
    pub temperature: f64,
    pub speed: f64,
}

impl Intake {
    /// The air of an altitude of the standard day, at a true airspeed.
    pub fn standard(altitude: f64, speed: f64) -> Intake {
        Intake { pressure: crate::atmosphere::pressure(altitude), temperature: crate::atmosphere::temperature(altitude), speed }
    }

    fn mach(&self) -> f64 {
        self.speed.max(0.0) / (GAMMA * GAS_CONSTANT * self.temperature).sqrt()
    }

    /// Total temperature and pressure at the fan's face, as ratios to the standard sea level's.
    fn total(&self) -> (f64, f64) {
        let m = self.mach();
        let rise = 1.0 + 0.5 * (GAMMA - 1.0) * m * m;
        (self.temperature / SEA_LEVEL_TEMPERATURE * rise, self.pressure / SEA_LEVEL_PRESSURE * rise.powf(1.0 / KAPPA))
    }
}

/// What an engine does at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Running {
    /// Net thrust along the engine's axis, N: negative with the reverser open, or windmilling.
    pub thrust: f64,
    /// kg/s.
    pub fuel_flow: f64,
    /// Air through it, kg/s.
    pub airflow: f64,
}

impl Jets {
    /// The engines with `pressure_ratio` worked out from the static thrust and the airflow.
    pub fn rated(mut self) -> Jets {
        let (mut lo, mut hi) = (1.0001, 4.0);
        for _ in 0..60 {
            self.pressure_ratio = (lo + hi) / 2.0;
            if self.airflow * self.jet_speed(1.0, 1.0, 1.0) > self.static_thrust {
                hi = self.pressure_ratio;
            } else {
                lo = self.pressure_ratio;
            }
        }
        self
    }

    /// Speed of the jet, m/s, fully expanded: the stream compressed to the equivalent pressure
    /// ratio at a corrected speed `nc` and let out to the outside pressure. `theta` is the total
    /// temperature at the fan's face over the standard sea level's and `ram` the total pressure
    /// there over the static one outside.
    fn jet_speed(&self, nc: f64, theta: f64, ram: f64) -> f64 {
        let pressure = 1.0 + (self.pressure_ratio - 1.0) * nc.powf(self.pressure_exponent);
        let temperature = SEA_LEVEL_TEMPERATURE * theta * pressure.powf(KAPPA / COMPRESSION) * (1.0 + self.heat * nc * nc);
        let nozzle = pressure * ram;
        if nozzle <= 1.0 {
            return 0.0;
        }
        (2.0 * CP * temperature * (1.0 - nozzle.powf(-KAPPA))).sqrt()
    }

    /// Thrust, fuel and airflow of one engine turning at `n1` in the air `intake`, with its reverser
    /// `reverse` open (0 closed to 1); `burning` false for one that has flamed out or has no fuel.
    pub fn run(&self, n1: f64, intake: &Intake, reverse: f64, burning: bool) -> Running {
        let (theta, delta) = intake.total();
        let ram = delta / (intake.pressure / SEA_LEVEL_PRESSURE);
        let nc = n1.max(0.0) / theta.sqrt();
        let airflow = self.airflow * delta / theta.sqrt() * nc.powf(self.flow_exponent);
        let speed = intake.speed.max(0.0);
        // A dead engine adds nothing to the air it lets through, and its turning fan takes from it.
        let jet = if burning { self.jet_speed(nc, theta, ram) } else { WINDMILL_RECOVERY * speed };
        let share = self.reverser.map_or(0.0, |r| r.share);
        // The reverser turns the jet: closed, all of it pushes forwards; open, `share` of it backwards.
        let gross = airflow * jet * (1.0 - (1.0 + share) * clamp(reverse, 0.0, 1.0));
        let thrust = gross - airflow * speed;
        let fuel_flow = if burning {
            let power = 0.5 * airflow * (jet * jet - speed * speed).max(0.0);
            let efficiency = self.efficiency * (1.0 + self.efficiency_cold * (1.0 - theta));
            self.fuel_idle * delta * theta.sqrt() + power / (efficiency.max(0.05) * FUEL_HEAT)
        } else {
            0.0
        };
        Running { thrust, fuel_flow, airflow }
    }

    /// The highest N1 the ratings allow in that air: full throttle.
    pub fn rating(&self, intake: &Intake) -> f64 {
        let (theta, _) = intake.total();
        let thin = 1.0 - clamp(intake.pressure / SEA_LEVEL_PRESSURE, 0.0, 1.0);
        ((1.0 + self.altitude_rating * thin) * theta.sqrt()).min(self.redline)
    }

    /// The N1 a throttle setting (0...1) asks for: from idle (the ground's, or the higher one of
    /// the air) to the rating; with the reverser selected, to its limit.
    pub fn command(&self, throttle: f64, intake: &Intake, airborne: bool, reverse: bool) -> f64 {
        let idle = if airborne { self.flight_idle } else { self.idle };
        let top = match self.reverser {
            Some(r) if reverse => r.limit.min(self.rating(intake)),
            _ => self.rating(intake),
        };
        idle + (top - idle).max(0.0) * clamp(throttle, 0.0, 1.0)
    }

    /// The N1 the air turns a dead engine at: nothing standing still, a tenth or two at flying speeds.
    pub fn windmill(&self, intake: &Intake) -> f64 {
        (0.45 * intake.mach()).min(0.3)
    }

    /// N1 after `dt` seconds on its way to `target`: up no faster than the engine spools (slower
    /// in thin air), down at about a quarter a second; `burning` false lets it run down to `target`
    /// (the windmilling speed) at the rate the air brakes it.
    pub fn spool(&self, n1: f64, target: f64, intake: &Intake, burning: bool, dt: f64) -> f64 {
        let error = target - n1;
        if !burning {
            return n1 + clamp(error / 4.0, -0.12, 0.05) * dt;
        }
        let (_, delta) = intake.total();
        let up = (self.spool.0 + self.spool.1 * n1 * n1) * delta.clamp(0.3, 1.0).sqrt();
        n1 + clamp(error / 0.5, -0.25, up) * dt
    }

    /// The engine speed for the tachometer and the sound, rpm.
    pub fn rpm(&self, n1: f64) -> f64 {
        self.n1_rpm * n1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Pratt & Whitney JT9D-3A, as the Boeing 747's file has it.
    fn jt9d() -> Jets {
        Jets {
            mounts: &[],
            static_thrust: 193_500.0,
            airflow: 685.0,
            idle: 0.27,
            flight_idle: 0.38,
            redline: 1.06,
            altitude_rating: 0.165,
            pressure_exponent: 2.2,
            flow_exponent: 1.3,
            heat: 0.15,
            spool: (0.02, 0.25),
            fuel_idle: 0.2,
            efficiency: 0.37,
            efficiency_cold: 1.7,
            reverser: Some(Reverser { share: 0.45, time: 2.0, limit: 0.9 }),
            fan_diameter: 2.34,
            n1_rpm: 3650.0,
            pressure_ratio: 0.0,
        }
        .rated()
    }

    const LBF: f64 = 4.448;

    #[test]
    fn the_static_thrust_is_the_rating() {
        let e = jt9d();
        let still = Intake::standard(0.0, 0.0);
        let r = e.run(1.0, &still, 0.0, true);
        assert!((r.thrust - 193_500.0).abs() < 100.0, "{}", r.thrust);
        assert!((e.rating(&still) - 1.0).abs() < 1e-9);
        // Published: 0.346 lb/h per lbf at take-off.
        let sfc = r.fuel_flow / r.thrust * 3600.0 * 9.80665;
        assert!((0.32..0.38).contains(&sfc), "take-off consumption {sfc} lb/h/lbf");
    }

    #[test]
    fn thrust_falls_along_the_runway_and_with_height() {
        let e = jt9d();
        // At 165 kt on the runway a big fan has lost about a quarter of its static thrust.
        let rolling = e.run(1.0, &Intake::standard(0.0, 85.0), 0.0, true).thrust / 193_500.0;
        assert!((0.74..0.80).contains(&rolling), "{rolling}");
        // Published: 10,200 lbf of maximum cruise thrust at 35,000 ft and Mach 0.85.
        let cruise = Intake::standard(10_668.0, 0.85 * 296.5);
        let top = e.rating(&cruise);
        assert!((1.03..=1.06).contains(&top), "N1 at altitude {top}");
        let thrust = e.run(top, &cruise, 0.0, true).thrust / LBF;
        assert!((10_200.0..12_000.0).contains(&thrust), "{thrust} lbf");
        // And about 0.63 lb/h per lbf at cruise thrust.
        let r = e.run(0.97, &cruise, 0.0, true);
        let sfc = r.fuel_flow / r.thrust * 3600.0 * 9.80665;
        assert!((8_000.0..9_500.0).contains(&(r.thrust / LBF)), "{}", r.thrust / LBF);
        assert!((0.58..0.68).contains(&sfc), "cruise consumption {sfc} lb/h/lbf");
    }

    #[test]
    fn idle_pushes_a_little_on_the_ground_and_drags_at_speed() {
        let e = jt9d();
        let idle = e.run(e.idle, &Intake::standard(0.0, 0.0), 0.0, true);
        assert!((0.03..0.06).contains(&(idle.thrust / 193_500.0)), "{}", idle.thrust);
        // About 1,700 lb/h at ground idle.
        assert!((0.18..0.28).contains(&idle.fuel_flow), "{}", idle.fuel_flow);
        let fast = Intake::standard(3000.0, 150.0);
        let descending = e.run(e.flight_idle, &fast, 0.0, true).thrust;
        assert!(descending < 0.03 * 193_500.0, "{descending}");
        // A dead engine windmills and drags.
        let dead = e.run(e.windmill(&fast), &fast, 0.0, false);
        assert!(dead.thrust < -1000.0 && dead.thrust > -15_000.0 && dead.fuel_flow == 0.0, "{dead:?}");
    }

    #[test]
    fn the_reverser_pushes_backwards() {
        let e = jt9d();
        let rolling = Intake::standard(0.0, 60.0);
        let n1 = e.command(1.0, &rolling, false, true);
        assert!((n1 - 0.9).abs() < 1e-9);
        let forward = e.run(n1, &rolling, 0.0, true).thrust;
        let back = e.run(n1, &rolling, 1.0, true).thrust;
        assert!(forward > 0.0 && back < -0.4 * forward, "{forward} {back}");
    }

    #[test]
    fn it_spools_up_slowly_from_idle() {
        let e = jt9d();
        let still = Intake::standard(0.0, 0.0);
        let time_to = |from: f64| {
            let (mut n1, mut t) = (from, 0.0);
            while n1 < 0.95 && t < 30.0 {
                n1 = e.spool(n1, 1.0, &still, true, 0.01);
                t += 0.01;
            }
            t
        };
        // Some eight seconds from ground idle; under six from the flight idle.
        assert!((6.0..10.0).contains(&time_to(e.idle)), "{}", time_to(e.idle));
        assert!((3.0..6.0).contains(&time_to(e.flight_idle)), "{}", time_to(e.flight_idle));
        // Down in a few seconds.
        let (mut n1, mut t) = (1.0, 0.0);
        while n1 > 0.4 && t < 30.0 {
            n1 = e.spool(n1, e.idle, &still, true, 0.01);
            t += 0.01;
        }
        assert!((2.0..5.0).contains(&t), "{t}");
    }
}
