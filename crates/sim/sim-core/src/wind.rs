//! The air: mean wind by height, terrain-induced lift and continuous turbulence.
//!
//! `Weather` is shared by the whole world. Each aircraft has its own `WindField`, which holds the
//! state of the turbulence it is flying through.

use crate::atmosphere::Atmosphere;
use crate::math::{clamp, smoothstep, Rng, DEG, V3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weather {
    /// Direction the surface wind blows from, degrees true.
    pub wind_from: f64,
    /// Wind speed 10 m above the ground, m/s.
    pub wind_speed: f64,
    /// 0 = still air, 1 = rough.
    pub turbulence: f64,
    /// Pressure, temperature and humidity of the day: what the aircraft fly in and the instruments sense.
    pub air: Atmosphere,
    /// How far a dark object stands out against the horizon sky by day (the meteorological
    /// visibility, m): the aerosols of the haze, or the droplets of a fog, near the ground. It is
    /// what the towers set their lights by and what the haze and fog are drawn with. By default 150 km: a clear day, the mountains 60 km off plain.
    pub visibility: f64,
    /// How hard it rains, 0 dry to 1 a downpour: the water on a circuit's asphalt follows it
    /// (`car::wet::RoadWater`), and with it the tyres' grip.
    pub rain: f64,
}

impl Default for Weather {
    fn default() -> Weather {
        Weather {
            wind_from: 250.0,
            wind_speed: 0.0,
            turbulence: 0.0,
            air: Atmosphere::STANDARD,
            visibility: 150_000.0,
            rain: 0.0,
        }
    }
}

impl Weather {
    /// Mean wind (NED, horizontal) at a height above ground.
    pub fn mean_wind(&self, agl: f64) -> V3 {
        let z = agl.max(2.0);
        let profile = ((z / 0.05).ln() / (10.0_f64 / 0.05).ln()).min(1.7);
        let speed = self.wind_speed * profile;
        let veer = 20.0 * DEG * clamp(agl / 1000.0, 0.0, 1.0);
        let from = self.wind_from * DEG + veer;
        V3::new(-from.cos(), -from.sin(), 0.0) * speed
    }
}

/// The air around one aircraft: mean wind, lift over slopes and its own gusts.
#[derive(Clone, Debug)]
pub struct WindField {
    rng: Rng,
    gust: V3,
    span_gust: f64,
    air_velocity: V3,
    roll_gust: f64,
}

impl WindField {
    pub fn new(seed: u64) -> WindField {
        WindField {
            rng: Rng::new(seed),
            gust: V3::ZERO,
            span_gust: 0.0,
            air_velocity: V3::ZERO,
            roll_gust: 0.0,
        }
    }

    /// Velocity of the air mass at the aircraft, NED.
    pub fn air_velocity(&self) -> V3 {
        self.air_velocity
    }

    /// Vertical gust gradient across the span, as an equivalent roll rate (rad/s).
    pub fn roll_gust(&self) -> f64 {
        self.roll_gust
    }

    /// Advances the gusts by `dt`. `slope_n` and `slope_e` are the ground height gradient
    /// (per metre) towards north and east.
    pub fn update(
        &mut self,
        weather: &Weather,
        dt: f64,
        agl: f64,
        airspeed: f64,
        slope_n: f64,
        slope_e: f64,
    ) {
        let mean = weather.mean_wind(agl);
        // Air forced up or down by the terrain it flows over.
        let lift = clamp(
            (mean.x * slope_n + mean.y * slope_e) * (-agl.max(0.0) / 250.0).exp(),
            -6.0,
            6.0,
        );

        let length = clamp(agl, 30.0, 300.0);
        let tau = length / airspeed.max(15.0);
        let low = 1.0 - 0.6 * smoothstep(300.0, 1500.0, agl);
        let sigma = weather.turbulence * (0.25 + 0.12 * mean.length()) * low;
        let k = (2.0 * dt / tau).sqrt();
        self.gust.x += -self.gust.x * dt / tau + sigma * k * self.rng.normal();
        self.gust.y += -self.gust.y * dt / tau + sigma * k * self.rng.normal();
        self.gust.z += -self.gust.z * dt / tau + sigma * k * self.rng.normal();
        let tau_s = tau * 0.4;
        let k_s = (2.0 * dt / tau_s).sqrt();
        self.span_gust += -self.span_gust * dt / tau_s + 0.5 * sigma * k_s * self.rng.normal();

        self.air_velocity = mean + self.gust + V3::new(0.0, 0.0, -lift);
        self.roll_gust = self.span_gust / 5.45;
    }
}
