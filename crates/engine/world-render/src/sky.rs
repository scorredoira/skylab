//! The air the light goes through, as one model for everything: the sky's colour, the sun's and
//! the moon's light where it reaches, the light of the sky on every surface, and the haze between
//! the eye and whatever it sees (Hillaire 2020, "A Scalable and Production Ready Sky and Atmosphere
//! Rendering Technique"). Rayleigh scattering by the air, Mie by the haze or the fog of the day
//! (`Weather::visibility` and its humidity), absorption by the ozone, on a round planet.
//!
//! [`Air`] is the model on the CPU: what the air is made of for a weather, and the light that gets
//! through it (the sun's colour, for the frame and for the tests). [`SkyTables`] makes the tables
//! the shaders read, on the GPU (`sky.wgsl`, with `sky_common.wgsl` shared with `shaders.wgsl`):
//! the transmittance and the multiple scattering when the weather changes, the sky seen from the
//! eye, the aerial perspective and the ambient light when the sun, the moon or the eye's altitude move.
//!
//! **The scene's light.** The shaders count light so that a white surface lit by a light of `E` has
//! a radiance of `E` (no 1 / pi): the sun above the air is [`SUN_SCALE`] by day, and a candela per
//! square metre is `gain` (`lights.rs`). Night is drawn brighter than it is, so that it can be
//! seen: every light of the sky is multiplied by [`exposure`], which draws the darkest night
//! [`DEEPEST`] stops below a sunlit day, where it is twenty.

use crate::sun_position;
use engine::{wgpu, Gpu};
use sim_core::{smoothstep, Weather};
use glam::{DVec3, Vec3};
use std::f64::consts::PI;
use std::mem::size_of;

/// The planet's radius and the top of its air, m (`sky_common.wgsl` has the same).
pub const GROUND_RADIUS: f64 = 6_360_000.0;
pub const TOP_RADIUS: f64 = 6_460_000.0;
/// Rayleigh scattering of the air at sea level, red, green and blue (680, 550 and 440 nm), 1/m,
/// and its scale height, m (Bruneton and Neyret 2008; Hillaire 2020).
const RAYLEIGH: DVec3 = DVec3::new(5.802e-6, 13.558e-6, 33.1e-6);
const RAYLEIGH_HEIGHT: f64 = 8000.0;
/// Absorption of the ozone at the peak of its layer, 25 km up and 30 km thick (Hillaire 2020).
const OZONE: DVec3 = DVec3::new(0.650e-6, 1.881e-6, 0.085e-6);
/// The wavelengths of red, green and blue, nm, for the haze's colour (`Air::new`).
const WAVELENGTHS: [f64; 3] = [680.0, 550.0, 440.0];
/// Koschmieder: a dark object is seen against the horizon until its contrast falls to 2 %, so the
/// visibility is ln 50 over the extinction of green light.
const KOSCHMIEDER: f64 = 3.912;
/// The least haze there is, 1/m: a visibility of about 280 km.
const CLEAREST: f64 = 0.4e-6;

/// The sun's light above the air in the scene's units: a white surface facing it would have this
/// radiance. Its luminance stands for `SUN_LUX`.
pub const SUN_SCALE: f32 = 2.6;
/// The sun's illuminance above the air, lux.
pub const SUN_LUX: f64 = 128_000.0;
/// The moon's light above the air as a share of the sun's: the world's moon is always nearly full,
/// some 0.25 lux overhead (0.1 lux on the ground where it stands).
const MOON: f64 = 0.25 / SUN_LUX;
/// Moonlight is sunlight, a little redder, but the eye sees by it with the rods, bluer and without
/// colour (the Purkinje shift), which a picture on a screen cannot do by itself: its light is
/// given that tint, of luminance 1.
const MOON_TINT: Vec3 = Vec3::new(0.66, 0.97, 1.62);
/// How many stops below a sunlit day the darkest night is drawn (`exposure`), and the illuminance
/// on the ground above which the scene is drawn as it is, lux.
pub const DEEPEST: f64 = 6.5;
const REFERENCE_LUX: f64 = 80_000.0;
/// The ground's albedo for the light it throws back into the air and on to what faces it.
const GROUND_ALBEDO: f32 = 0.15;

/// What the air is made of for a weather, at sea level or where each part is thickest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Air {
    /// Scattering by the haze or fog, red, green, blue, 1/m, where it is thickest, and its extinction.
    pub mie_scattering: DVec3,
    pub mie_extinction: DVec3,
    /// How it thins with height: uniform up to `mie_base` (the aerodrome the visibility is reported
    /// at), then falling by e every `mie_height` metres.
    pub mie_height: f64,
    pub mie_base: f64,
    /// How much of its light it throws forward: Cornette and Shanks's g.
    pub mie_g: f64,
}

impl Air {
    /// The air of a visibility (m) reported at an altitude `base`, with the humidity of `weather`.
    /// The haze's extinction is what the visibility leaves after the air's own (Koschmieder). Haze
    /// lies in the lowest kilometre or two and is a little blue (its droplets are small: an Ångström
    /// exponent of 1.3), and absorbs a tenth of what it takes; humid, it grows whiter and absorbs
    /// less. Fog, below 5 km and most below 1, is water: white, absorbing nothing, more forward,
    /// and lying low.
    pub fn new(weather: &Weather, visibility: f64, base: f64) -> Air {
        let humid = weather.air.relative_humidity.clamp(0.0, 1.0);
        let fog = 1.0 - smoothstep(1000.0, 5000.0, visibility);
        let green = (KOSCHMIEDER / visibility.max(30.0) - RAYLEIGH.y).max(CLEAREST);
        let angstrom = (1.3 - 0.8 * humid) * (1.0 - fog);
        let albedo = (0.9 + 0.09 * humid) * (1.0 - fog) + fog;
        let extinction = DVec3::from(WAVELENGTHS.map(|l| green * (l / 550.0).powf(-angstrom)));
        Air {
            mie_scattering: extinction * albedo,
            mie_extinction: extinction,
            mie_height: 1200.0 * (1.0 - fog) + 300.0 * fog,
            mie_base: base,
            mie_g: 0.76 * (1.0 - fog) + 0.85 * fog,
        }
    }

    /// Rayleigh, haze and ozone at an altitude, each as a share of its own peak (`densities` in sky.wgsl).
    fn densities(&self, altitude: f64) -> DVec3 {
        let h = altitude.max(0.0);
        DVec3::new((-h / RAYLEIGH_HEIGHT).exp(), (-(h - self.mie_base).max(0.0) / self.mie_height).exp(), (1.0 - (h - 25_000.0).abs() / 15_000.0).max(0.0))
    }

    /// Extinction at an altitude, 1/m.
    pub fn extinction(&self, altitude: f64) -> DVec3 {
        let d = self.densities(altitude);
        RAYLEIGH * d.x + self.mie_extinction * d.y + OZONE * d.z
    }

    /// The share of a light above the air that reaches an altitude, coming from `mu` from the
    /// vertical (the cosine): nothing once the planet is in the way, over a little more than the
    /// sun's disc (`light_through` in sky.wgsl, which reads it from its table).
    pub fn transmittance(&self, altitude: f64, mu: f64) -> DVec3 {
        let r = GROUND_RADIUS + altitude.max(0.0);
        let horizon = -(1.0 - (GROUND_RADIUS / r).powi(2)).max(0.0).sqrt();
        let seen = smoothstep(horizon - 0.006, horizon + 0.006, mu);
        if seen <= 0.0 {
            return DVec3::ZERO;
        }
        let distance = (r * r * (mu * mu - 1.0) + TOP_RADIUS * TOP_RADIUS).max(0.0).sqrt() - r * mu;
        let steps = 64;
        let dt = distance / steps as f64;
        let mut depth = DVec3::ZERO;
        for i in 0..steps {
            let t = dt * (i as f64 + 0.5);
            // Height of the point `t` along the ray, over the round planet.
            let h = (r * r + t * t + 2.0 * r * t * mu).sqrt() - GROUND_RADIUS;
            depth += self.extinction(h) * dt;
        }
        (-depth).exp() * seen
    }

    /// The uniforms of the tables for this air (`struct Sky` in sky.wgsl); the lights are set by `SkyTables::update`.
    fn uniforms(&self) -> SkyUniforms {
        let v = |v: DVec3, w: f64| [v.x as f32, v.y as f32, v.z as f32, w as f32];
        SkyUniforms {
            rayleigh: v(RAYLEIGH, RAYLEIGH_HEIGHT),
            mie_scattering: v(self.mie_scattering, self.mie_height),
            mie_extinction: v(self.mie_extinction, self.mie_base),
            ozone: v(OZONE, self.mie_g),
            ..bytemuck::Zeroable::zeroed()
        }
    }
}

/// How much light really falls on level ground in the open with the sun this high (radians),
/// lux: the sun and the sky of a clear day, then the twilight, and the moon's 0.1 lux under it
/// all (the world always has a moon). After the tables of the U.S. Naval Observatory.
pub fn daylight(sun_elevation: f64) -> f64 {
    // (degrees, log10 lux).
    const SKY: [(f64, f64); 14] = [
        (-18.0, -3.2),
        (-12.0, -2.1),
        (-6.0, 0.5),
        (-4.0, 1.2),
        (-2.0, 1.9),
        (0.0, 2.6),
        (2.0, 3.1),
        (5.0, 3.6),
        (10.0, 4.0),
        (20.0, 4.4),
        (30.0, 4.65),
        (45.0, 4.85),
        (60.0, 4.95),
        (90.0, 5.05),
    ];
    let deg = (sun_elevation * 180.0 / PI).clamp(-18.0, 90.0);
    let i = SKY.iter().position(|p| p.0 >= deg).unwrap_or(SKY.len() - 1).max(1);
    let ((x0, y0), (x1, y1)) = (SKY[i - 1], SKY[i]);
    let log = y0 + (y1 - y0) * (deg - x0) / (x1 - x0);
    10f64.powf(log) + 0.1
}

/// How much brighter than it is the scene draws the light of the sky, the sun and the moon with the
/// sun this high (radians): 1 by day; below `REFERENCE_LUX` on the ground, `x` stops darker in the
/// world are drawn `DEEPEST (1 - e^(-x / DEEPEST))` stops darker, the same at first and never more
/// than `DEEPEST`: the sun on the horizon (7.6 stops) 4.5, the end of civil twilight (15) 5.8, a
/// moonlit night (20) 6.2; the engine's exposure makes up a quarter of that. What a camera or the
/// eye would do, before the exposure the engine follows the scene with.
pub fn exposure(sun_elevation: f64) -> f32 {
    let x = (REFERENCE_LUX / daylight(sun_elevation)).max(1.0).log2();
    (x - DEEPEST * (1.0 - (-x / DEEPEST).exp())).exp2() as f32
}

/// Where the moon is when the sun is at `sun` (render axes): across the sky from it, 37° up. The
/// world's moon keeps to the sun's vertical plane, which the sky's tables rely on.
pub fn moon_direction(sun: Vec3) -> Vec3 {
    Vec3::new(-sun.x, 0.6, -sun.z).normalize()
}

/// The lights of the sky at an hour, in the scene's units: the sun's and the moon's above the air
/// (`sun_top`, `moon_top`) and where each is (render axes).
#[derive(Clone, Copy, Debug)]
pub struct SkyLights {
    pub sun: Vec3,
    pub moon: Vec3,
    pub sun_top: Vec3,
    pub moon_top: Vec3,
    /// The scene's value of the sun's light above the air: `SUN_SCALE` times the exposure.
    pub scale: f32,
}

impl SkyLights {
    pub fn at(hour: f64) -> SkyLights {
        let (east, north, up) = sun_position(hour);
        let sun = Vec3::new(east as f32, up as f32, -north as f32).normalize();
        let scale = SUN_SCALE * exposure(up.asin());
        SkyLights { sun, moon: moon_direction(sun), sun_top: Vec3::splat(scale), moon_top: MOON_TINT * (scale * MOON as f32), scale }
    }

    /// The sun's azimuth: the horizontal unit vector towards it (render axes), the tables' +x.
    pub fn azimuth(&self) -> Vec3 {
        Vec3::new(self.sun.x, 0.0, self.sun.z).try_normalize().unwrap_or(Vec3::X)
    }

    /// Whether the moon lights the world directly: once the sun is 4.6° below the horizon. Before
    /// that the sun, set or not, is still the light that counts (the moon's is a thousandth of the
    /// sky's), and what casts the shadows.
    pub fn moonlit(&self) -> bool {
        self.sun.y < -0.08
    }

    /// The light the world is lit by directly, the sun or the moon (`moonlit`): its direction, and
    /// its light above the air.
    pub fn direct(&self) -> (Vec3, Vec3) {
        if !self.moonlit() {
            (self.sun, self.sun_top)
        } else {
            (self.moon, self.moon_top)
        }
    }
}

/// The uniforms of the tables: `struct Sky` in sky.wgsl, field by field.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct SkyUniforms {
    rayleigh: [f32; 4],
    mie_scattering: [f32; 4],
    mie_extinction: [f32; 4],
    ozone: [f32; 4],
    sun: [f32; 4],
    moon: [f32; 4],
    sun_top: [f32; 4],
    moon_top: [f32; 4],
    ground: [f32; 4],
}

const _: () = assert!(size_of::<SkyUniforms>() == 9 * 16);

/// Sizes of the tables (`sky_common.wgsl`).
const TRANSMITTANCE: [u32; 2] = [256, 64];
const MULTIPLE: [u32; 2] = [32, 32];
const SKY_VIEW: [u32; 2] = [192, 108];
const AERIAL: [u32; 3] = [32, 48, 32];
const AMBIENT: [u32; 2] = [8, 16];

/// How far the aerial perspective reaches for a visibility, m: far enough that what lies beyond is
/// lost in the haze, as far as the horizon of the highest flight on a clear day.
pub fn aerial_reach(visibility: f64) -> f64 {
    (visibility * 30.0).clamp(8000.0, 200_000.0)
}

/// What the tables were last made for.
#[derive(Clone, Copy, PartialEq)]
struct Made {
    air: Air,
    uniforms: SkyUniforms,
}

/// The sky's tables on the GPU, and the passes that make them.
pub struct SkyTables {
    uniforms: wgpu::Buffer,
    passes: [(wgpu::ComputePipeline, wgpu::BindGroup, wgpu::BindGroup, [u32; 3]); 5],
    transmittance: wgpu::TextureView,
    sky_view: wgpu::TextureView,
    aerial_scatter: wgpu::TextureView,
    aerial_through: wgpu::TextureView,
    ambient: wgpu::TextureView,
    sampler: wgpu::Sampler,
    made: Option<Made>,
}

/// The source of the tables' passes.
pub fn source() -> String {
    concat!(include_str!("sky_common.wgsl"), include_str!("sky.wgsl")).to_string()
}

impl SkyTables {
    pub fn new(gpu: &Gpu) -> SkyTables {
        let d = &gpu.device;
        let texture = |label: &str, size: [u32; 3], dimension: wgpu::TextureDimension| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: size[2] },
                mip_level_count: 1,
                sample_count: 1,
                dimension,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
        };
        let flat = |label: &str, s: [u32; 2]| texture(label, [s[0], s[1], 1], wgpu::TextureDimension::D2);
        let transmittance = flat("sky transmittance", TRANSMITTANCE);
        let multiple = flat("sky multiple scattering", MULTIPLE);
        let sky_view = flat("sky view", SKY_VIEW);
        let aerial_scatter = texture("aerial scatter", AERIAL, wgpu::TextureDimension::D3);
        let aerial_through = texture("aerial through", AERIAL, wgpu::TextureDimension::D3);
        let ambient = flat("sky ambient", AMBIENT);
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor { label: Some("sky"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let uniforms = d.create_buffer(&wgpu::BufferDescriptor { label: Some("sky"), size: size_of::<SkyUniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let module = gpu.shader_with_ending_loops("sky", &source());
        // Each pass reads what the passes before it made (group 0, as much of it as it uses) and
        // writes its own tables (group 1): the layouts come from the shader.
        let pass = |entry: &str, outputs: &[(u32, &wgpu::TextureView)], size: [u32; 3]| {
            let pipeline = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry), layout: None, module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: None });
            let mut reads = vec![wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }];
            if entry != "transmittance" {
                reads.push(wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) });
                reads.push(wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&transmittance) });
            }
            if entry != "transmittance" && entry != "multiple_scattering" {
                reads.push(wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&multiple) });
            }
            let group = |index: u32, entries: &[wgpu::BindGroupEntry]| d.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(entry), layout: &pipeline.get_bind_group_layout(index), entries });
            let writes: Vec<wgpu::BindGroupEntry> = outputs.iter().map(|(binding, view)| wgpu::BindGroupEntry { binding: *binding, resource: wgpu::BindingResource::TextureView(view) }).collect();
            let (read, write) = (group(0, &reads), group(1, &writes));
            let groups = [size[0].div_ceil(8), size[1].div_ceil(8), 1];
            (pipeline, read, write, groups)
        };
        let passes = [
            pass("transmittance", &[(0, &transmittance)], [TRANSMITTANCE[0], TRANSMITTANCE[1], 1]),
            pass("multiple_scattering", &[(1, &multiple)], [MULTIPLE[0], MULTIPLE[1], 1]),
            pass("sky_view", &[(2, &sky_view)], [SKY_VIEW[0], SKY_VIEW[1], 1]),
            pass("aerial", &[(3, &aerial_scatter), (4, &aerial_through)], [AERIAL[0], AERIAL[1], 1]),
            pass("ambient", &[(5, &ambient)], [AMBIENT[0], AMBIENT[1], 1]),
        ];
        SkyTables { uniforms, passes, transmittance, sky_view, aerial_scatter, aerial_through, ambient, sampler, made: None }
    }

    /// Makes again the tables that changed: all of them for another air; the sky, the aerial
    /// perspective and the ambient light when the lights, the eye's altitude (`eye`), the ground's
    /// under it (`ground`) or the reach of the aerial perspective (`reach`) moved more than can be seen.
    pub fn update(&mut self, gpu: &Gpu, air: &Air, lights: &SkyLights, eye: f64, ground: f64, reach: f64) {
        // The tables' frame: the sun's azimuth along +x.
        let across = lights.azimuth();
        let local = |d: Vec3| [Vec3::new(d.x, 0.0, d.z).dot(across), d.y, across.x * d.z - across.z * d.x, 0.0];
        let mut u = air.uniforms();
        u.sun = local(lights.sun);
        u.sun[3] = GROUND_ALBEDO;
        u.moon = local(lights.moon);
        u.sun_top = [lights.sun_top.x, lights.sun_top.y, lights.sun_top.z, eye as f32];
        u.moon_top = [lights.moon_top.x, lights.moon_top.y, lights.moon_top.z, reach as f32];
        u.ground = [ground as f32, 0.0, 0.0, 0.0];
        let new_air = self.made.is_none_or(|m| m.air != *air);
        let moved = new_air || self.made.is_none_or(|m| moved(&m.uniforms, &u));
        if !moved {
            return;
        }
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sky") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sky"), timestamp_writes: None });
            let first = if new_air { 0 } else { 2 };
            for (pipeline, read, write, groups) in &self.passes[first..] {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, read, &[]);
                pass.set_bind_group(1, write, &[]);
                pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
            }
        }
        gpu.queue.submit([encoder.finish()]);
        self.made = Some(Made { air: *air, uniforms: u });
    }

    /// The frame's bindings 6 to 11 (`pipelines.rs`).
    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 6] {
        let view = |binding, view| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) };
        [
            view(6, &self.transmittance),
            view(7, &self.sky_view),
            view(8, &self.aerial_scatter),
            view(9, &self.aerial_through),
            view(10, &self.ambient),
            wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::Sampler(&self.sampler) },
        ]
    }
}

/// Whether the lights or the eye moved enough since the tables were made to see it: a hundredth
/// of a degree, half a percent of light, the eye 2 m or half a percent of its height, the ground 10 m.
fn moved(was: &SkyUniforms, now: &SkyUniforms) -> bool {
    let apart = |a: [f32; 4], b: [f32; 4], n: usize, tolerance: f32| (0..n).any(|i| (a[i] - b[i]).abs() > tolerance * a[i].abs().max(b[i].abs()).max(1e-30));
    let turned = |a: [f32; 4], b: [f32; 4]| (0..3).any(|i| (a[i] - b[i]).abs() > 2e-4);
    turned(was.sun, now.sun)
        || turned(was.moon, now.moon)
        || apart(was.sun_top, now.sun_top, 3, 0.005)
        || apart(was.moon_top, now.moon_top, 3, 0.005)
        || (was.sun_top[3] - now.sun_top[3]).abs() > (now.sun_top[3].abs() * 0.005).max(2.0)
        || (was.moon_top[3] - now.moon_top[3]).abs() > 0.01 * now.moon_top[3]
        || (was.ground[0] - now.ground[0]).abs() > 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::DEG;

    fn clear() -> Air {
        Air::new(&Weather::default(), 45_000.0, 0.0)
    }

    #[test]
    fn the_visibility_is_where_a_dark_object_keeps_two_percent_of_its_contrast() {
        for visibility in [800.0, 8000.0, 45_000.0] {
            let air = Air::new(&Weather::default(), visibility, 0.0);
            // Along the ground, green light: the haze's extinction and the air's.
            let contrast = (-air.extinction(0.0).y * visibility).exp();
            assert!((contrast - 0.02).abs() < 0.001, "{visibility}: {contrast}");
        }
    }

    #[test]
    fn the_haze_lies_low() {
        let air = clear();
        let haze = |h: f64| air.densities(h).y;
        assert!(haze(3000.0) < 0.1 * haze(0.0));
        let fog = Air::new(&Weather::default(), 600.0, 200.0);
        assert_eq!(fog.densities(100.0).y, 1.0, "uniform below the aerodrome's height");
        assert!(fog.densities(800.0).y < 0.15);
        // Haze is a little blue; fog is white and absorbs nothing.
        assert!(air.mie_extinction.z > air.mie_extinction.x * 1.3);
        assert!((fog.mie_extinction.z - fog.mie_extinction.x).abs() < 1e-9);
        assert_eq!(fog.mie_scattering, fog.mie_extinction);
    }

    #[test]
    fn the_sun_is_white_at_noon_and_red_at_sunset() {
        let air = clear();
        let noon = air.transmittance(0.0, (60.0 * DEG).sin());
        assert!(noon.x > 0.8 && noon.z > 0.55 && noon.z / noon.x > 0.7, "{noon:?}");
        let low = air.transmittance(0.0, (2.0 * DEG).sin());
        assert!(low.x > 4.0 * low.z, "a reddened sun {low:?}");
        assert!(low.x < 0.5 * noon.x);
        // Higher up there is less air above: whiter and brighter.
        let high = air.transmittance(3000.0, (2.0 * DEG).sin());
        assert!(high.x > low.x && high.z / high.x > low.z / low.x, "{high:?} against {low:?}");
        // Under the horizon, nothing.
        assert_eq!(air.transmittance(0.0, -0.05), DVec3::ZERO);
    }

    #[test]
    fn the_scene_keeps_a_part_of_the_range_of_light() {
        let noon = exposure(60.0 * DEG);
        assert_eq!(noon, 1.0);
        let mut last = 0.0;
        for k in -30..=60 {
            let e = exposure(-(k as f64) * DEG);
            assert!(e >= last);
            last = e;
        }
        // A moonlit night against noon: a millionth of the light, drawn some 2^-6 as bright.
        let ratio = daylight(-30.0 * DEG) * exposure(-30.0 * DEG) as f64 / daylight(60.0 * DEG);
        assert!(ratio.log2() > -DEEPEST && ratio.log2() < -6.0, "{}", ratio.log2());
    }

    #[test]
    fn daylight_rises_with_the_sun() {
        let mut last = 0.0;
        for k in -30..=90 {
            let lux = daylight(k as f64 * DEG);
            assert!(lux >= last);
            last = lux;
        }
        assert!((daylight(PI / 2.0) - 112_000.0).abs() < 1000.0);
        assert!((daylight(-0.5) - 0.1).abs() < 0.01);
    }

    #[test]
    fn the_air_reaching_the_ground_agrees_with_the_tables_of_daylight() {
        // The sun's light on level ground (the sky's is left out) against the U.S. Naval
        // Observatory's tables, which hold both: the sun alone is most of it, by day.
        let air = clear();
        for deg in [10.0, 30.0, 60.0] {
            let mu = (deg * DEG).sin();
            let direct = SUN_LUX * mu * air.transmittance(0.0, mu).y;
            let total = daylight(deg * DEG);
            assert!(direct > 0.55 * total && direct < total, "{deg}°: {direct:.0} of {total:.0} lux");
        }
    }

    /// The tables' shaders parse and validate without a GPU, as wgpu checks them.
    #[test]
    fn the_tables_shaders_are_valid_wgsl() {
        use engine::wgpu::naga;
        let source = source();
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
    }

    #[test]
    fn moved_only_when_it_shows() {
        let air = clear();
        let a = SkyLights::at(12.0);
        let mut u = air.uniforms();
        u.sun = [a.sun.x, a.sun.y, a.sun.z, 0.0];
        u.sun_top = [1.0, 1.0, 1.0, 1000.0];
        let mut v = u;
        v.sun_top[3] = 1001.0;
        assert!(!moved(&u, &v));
        v.sun_top[3] = 1010.0;
        assert!(moved(&u, &v));
        let mut w = u;
        w.sun[1] += 1e-3;
        assert!(moved(&u, &w));
    }
}
