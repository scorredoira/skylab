//! The atmosphere: the International Standard Atmosphere (troposphere and the lower stratosphere)
//! and the air of the day, which may be warmer or colder, of higher or lower pressure and more or
//! less humid. Altitudes in metres, SI units.
//!
//! `Atmosphere` is the single source of pressure, temperature and density in the world
//! (`Weather::air`): the aircraft fly in its density, the engine breathes its dry air and the
//! instruments sense its pressure and temperature. The free functions are the standard day, which
//! `Atmosphere::default()` is.

use crate::math::clamp;

pub const SEA_LEVEL_DENSITY: f64 = 1.225;
pub const SEA_LEVEL_PRESSURE: f64 = 101_325.0;
pub const SEA_LEVEL_TEMPERATURE: f64 = 288.15;
/// Temperature lapse rate of the troposphere, K/m.
pub const LAPSE_RATE: f64 = 0.0065;
/// g / (R L): the exponent of pressure against temperature in the troposphere.
pub const PRESSURE_EXPONENT: f64 = 5.25588;
/// Gas constant of dry air, J/(kg K).
pub const GAS_CONSTANT: f64 = 287.05;
/// Gas constant of water vapour, J/(kg K).
pub const VAPOUR_GAS_CONSTANT: f64 = 461.5;
/// Ratio of the specific heats of air.
pub const GAMMA: f64 = 1.4;
/// Where the troposphere ends, m: above it, up to 20 km, the temperature stays as it was there and
/// the pressure falls exponentially.
pub const TROPOPAUSE: f64 = 11_000.0;
/// The lowest altitude the standard atmosphere is taken down to, m (the shore of the Dead Sea is at -430).
const LOWEST: f64 = -500.0;

/// Kelvin, on the standard day.
pub fn temperature(altitude: f64) -> f64 {
    Atmosphere::STANDARD.temperature(altitude)
}

/// Pascal, on the standard day.
pub fn pressure(altitude: f64) -> f64 {
    Atmosphere::STANDARD.pressure(altitude)
}

/// kg/m³, on the standard day.
pub fn density(altitude: f64) -> f64 {
    Atmosphere::STANDARD.density(altitude)
}

/// Pressure of water vapour that saturates air at a temperature (K), Pa: over water above
/// freezing and over ice below it (Buck 1996).
pub fn saturation_vapour_pressure(temperature: f64) -> f64 {
    let t = temperature - 273.15;
    if t >= 0.0 {
        611.21 * ((18.678 - t / 234.5) * (t / (257.14 + t))).exp()
    } else {
        611.15 * ((23.036 - t / 333.7) * (t / (279.82 + t))).exp()
    }
}

/// The air of the day: the standard atmosphere's lapse rate and tropopause from a sea-level
/// pressure and temperature of its own (a day `ISA + 10` is 10 K warmer at every height), and its
/// humidity. Uniform over the world for now; a later atmosphere that varies from place to place
/// keeps these questions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// Pressure at sea level, Pa (the QNH, on a standard-temperature day).
    pub sea_level_pressure: f64,
    /// Temperature at sea level, K.
    pub sea_level_temperature: f64,
    /// Relative humidity, 0 (the standard atmosphere's dry air) to 1 (saturated), the same at
    /// every height of the troposphere. Moist air is lighter, and the engine gets less of the air
    /// it burns; the haze grows with it (`Weather::visibility` is what it gives).
    pub relative_humidity: f64,
}

impl Default for Atmosphere {
    fn default() -> Atmosphere {
        Atmosphere::STANDARD
    }
}

impl Atmosphere {
    /// The International Standard Atmosphere: dry air.
    pub const STANDARD: Atmosphere = Atmosphere {
        sea_level_pressure: SEA_LEVEL_PRESSURE,
        sea_level_temperature: SEA_LEVEL_TEMPERATURE,
        relative_humidity: 0.0,
    };

    /// Kelvin: falling by the lapse rate up to the tropopause, then constant.
    pub fn temperature(&self, altitude: f64) -> f64 {
        self.sea_level_temperature - LAPSE_RATE * clamp(altitude, LOWEST, TROPOPAUSE)
    }

    /// Pascal: the weight of the air above, which falls faster with height on a cold day.
    pub fn pressure(&self, altitude: f64) -> f64 {
        let troposphere = self.sea_level_pressure
            * (self.temperature(altitude) / self.sea_level_temperature).powf(PRESSURE_EXPONENT);
        if altitude <= TROPOPAUSE {
            return troposphere;
        }
        // Isothermal: dp/dh = -g p / (R T), with g / R = PRESSURE_EXPONENT * LAPSE_RATE.
        let t = self.temperature(TROPOPAUSE);
        troposphere * (-PRESSURE_EXPONENT * LAPSE_RATE * (altitude - TROPOPAUSE) / t).exp()
    }

    /// Partial pressure of the water vapour in the air, Pa.
    pub fn vapour_pressure(&self, altitude: f64) -> f64 {
        if self.relative_humidity <= 0.0 {
            return 0.0;
        }
        let saturated = saturation_vapour_pressure(self.temperature(altitude));
        (clamp(self.relative_humidity, 0.0, 1.0) * saturated).min(0.1 * self.pressure(altitude))
    }

    /// kg/m³: the dry air and the vapour, each by its own gas constant. Water vapour is lighter
    /// than the dry air it takes the place of, so humid air is less dense.
    pub fn density(&self, altitude: f64) -> f64 {
        let t = self.temperature(altitude);
        let p = self.pressure(altitude);
        let e = self.vapour_pressure(altitude);
        (p - e) / (GAS_CONSTANT * t) + e / (VAPOUR_GAS_CONSTANT * t)
    }

    /// The share of the density that is water vapour, 0 in dry air: what an engine cannot burn.
    pub fn vapour_share(&self, altitude: f64) -> f64 {
        let e = self.vapour_pressure(altitude);
        if e <= 0.0 {
            return 0.0;
        }
        e / (VAPOUR_GAS_CONSTANT * self.temperature(altitude)) / self.density(altitude)
    }

    /// Speed of sound, m/s.
    pub fn speed_of_sound(&self, altitude: f64) -> f64 {
        (GAMMA * GAS_CONSTANT * self.temperature(altitude)).sqrt()
    }

    /// The QNH an aerodrome at `elevation` reports: its pressure brought down to sea level through
    /// the standard atmosphere, so that an altimeter set to it reads the elevation on the ground.
    /// On a day of standard temperature it is the sea-level pressure.
    pub fn qnh(&self, elevation: f64) -> f64 {
        self.pressure(elevation) / (pressure(elevation) / SEA_LEVEL_PRESSURE)
    }
}

/// What an altimeter set to `setting` (Pa) reads at a static pressure `static_pressure` (Pa): the
/// height of that pressure above the setting's in the standard atmosphere, m.
pub fn pressure_altitude(static_pressure: f64, setting: f64) -> f64 {
    SEA_LEVEL_TEMPERATURE / LAPSE_RATE
        * (1.0 - (static_pressure / setting).powf(1.0 / PRESSURE_EXPONENT))
}

/// Impact pressure (total minus static) of a flow at Mach `mach` over a static pressure, Pa
/// (isentropic, subsonic).
pub fn impact_pressure(static_pressure: f64, mach: f64) -> f64 {
    static_pressure * ((1.0 + 0.2 * mach * mach).powf(3.5) - 1.0)
}

/// Calibrated airspeed, m/s: what an airspeed indicator calibrated for the standard sea level
/// shows for an impact pressure (Pa). Zero for none or less.
pub fn calibrated_airspeed(impact: f64) -> f64 {
    if impact <= 0.0 {
        return 0.0;
    }
    let a0 = (GAMMA * GAS_CONSTANT * SEA_LEVEL_TEMPERATURE).sqrt();
    a0 * (5.0 * ((impact / SEA_LEVEL_PRESSURE + 1.0).powf(2.0 / 7.0) - 1.0)).sqrt()
}

/// Mach number from an impact pressure and the static pressure it was measured against.
pub fn mach(impact: f64, static_pressure: f64) -> f64 {
    if impact <= 0.0 || static_pressure <= 0.0 {
        return 0.0;
    }
    (5.0 * ((impact / static_pressure + 1.0).powf(2.0 / 7.0) - 1.0)).sqrt()
}
