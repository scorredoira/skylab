//! The lights of the aerodromes (`sim_core::lights`) as the GPU draws them: a buffer of instances
//! per aerodrome near the camera, made the first time it is near, and what turns candelas into the
//! light of the scene (`lights.wgsl`).
//!
//! A light is a point. What reaches the eye is its illuminance, `I / d²` dimmed by the haze, and a
//! pixel it falls in shines with that illuminance over the pixel's solid angle; the sprite spreads
//! it over a blob of at least a pixel's radius, or the lens's own size when that is bigger, so
//! the same light is drawn over the same area of the screen however it moves. Seen near, it is its
//! lens; far, a pixel whose brightness falls as it should, and never a shimmer.
//!
//! The scene is not in candelas per square metre: by day a white surface in the sun is about 2,
//! where a real one is 30 000 cd/m², and the night is drawn much brighter than a real night, so
//! that it can be seen (`sky::exposure`). The lights are put into it by the same measure as the
//! sun, the moon and the sky: `gain` is the scene's brightness of a candela per square metre at
//! that hour. A runway light at night is then as much brighter than the moonlit grass as it really
//! is, and the bloom and the tone mapping make of it what a camera would.

use crate::sky::{exposure, SUN_LUX, SUN_SCALE};
use crate::{sun_elevation, Lamp};
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Gpu};
use sim_core::lights::{self, Color, Light, System};
use sim_core::{Airport, Terrain};
use glam::{DVec3, Vec3};
use std::collections::HashMap;
use std::f64::consts::PI;

/// Floats per light in an instance buffer: five `vec4`s (`LightIn` in lights.wgsl).
const FLOATS: usize = 20;

/// Beyond this distance the lights fade out, metres, and at this one they are gone: before the end
/// of the airports looked through (30 km), so that none pops in or out.
pub const FADE: (f32, f32) = (18_000.0, 27_000.0);

/// The lights of one aerodrome, on the GPU.
pub struct Set {
    /// The point their positions are measured from: the middle of the runway (north, east, down).
    pub origin: DVec3,
    pub buffer: wgpu::Buffer,
    pub count: u32,
    /// The floodlights: where they are, their intensity (candelas) and their beam (the axis in render
    /// axes, a round half width, and the share of its middle it spills all round), to light the ground
    /// around them.
    pub floods: Vec<(DVec3, f64, Vec3, f32, f32)>,
}

/// The lights of the aerodromes near the camera.
#[derive(Default)]
pub struct AerodromeLights {
    sets: HashMap<u64, Set>,
}

impl AerodromeLights {
    /// Keeps the lights of these aerodromes, making those not made yet, and lets the others go.
    pub fn keep(&mut self, gpu: &Gpu, terrain: &Terrain, near: &[Airport]) {
        self.sets.retain(|id, _| near.iter().any(|a| a.id == *id));
        for a in near {
            self.sets.entry(a.id).or_insert_with(|| {
                let lights = lights::aerodrome(terrain, a);
                let origin = DVec3::new(a.n, a.e, -a.elevation);
                let data = instances(&lights, origin);
                let buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("aerodrome lights"), contents: bytemuck::cast_slice(&data), usage: wgpu::BufferUsages::VERTEX });
                let floods = lights.iter().filter(|l| l.system == System::Apron).filter_map(|l| l.beam.map(|b| (l.at, l.intensity, axis(b.heading, b.elevation), ((b.half_width + b.half_height) / 2.0) as f32, (l.omni / l.intensity) as f32))).collect();
                Set { origin, buffer, count: lights.len() as u32, floods }
            });
        }
    }

    pub fn get(&self, a: &Airport) -> Option<&Set> {
        self.sets.get(&a.id)
    }
}

/// The instances of `lights` for `light_vertex`, positions relative to `origin`.
pub fn instances(lights: &[Light], origin: DVec3) -> Vec<f32> {
    let mut data = Vec::with_capacity(lights.len() * FLOATS);
    for l in lights {
        let at = crate::to_render(l.at - origin);
        let color = rgb(l.color);
        let beam = l.beam.map_or([0.0; 4], |b| [b.heading as f32, b.elevation as f32, b.half_width as f32, b.half_height as f32]);
        let spin = l.beam.map_or(0.0, |b| b.spin as f32);
        let system = System::ALL.iter().position(|s| *s == l.system).unwrap_or(0) as f32;
        let (slope, half) = l.slope.map_or((0.0, 0.0), |s| (s.angle as f32, (s.transition / 2.0) as f32));
        let (period, offset, duration) = l.flash.map_or((0.0, 0.0, 0.0), |f| (f.period as f32, f.offset as f32, f.duration as f32));
        data.extend_from_slice(&[at.x, at.y, at.z, l.radius as f32]);
        data.extend_from_slice(&[color.x, color.y, color.z, l.intensity as f32]);
        data.extend_from_slice(&beam);
        data.extend_from_slice(&[l.omni as f32, spin, system, offset]);
        data.extend_from_slice(&[slope, half, period, duration]);
    }
    data
}

/// The system of a vehicle's lamp in an instance: past those of `System::ALL`, always lit as it is
/// given (the game says when it is on).
const LAMP_SYSTEM: f32 = System::ALL.len() as f32;

/// The instances of a vehicle's lamps, positions relative to the camera.
pub fn lamp_instances(lamps: &[Lamp]) -> Vec<f32> {
    let mut data = Vec::with_capacity(lamps.len() * FLOATS);
    for l in lamps {
        let color = rgb(l.color);
        // The beam's axis as the shader takes it: heading from north and elevation.
        let beam = l.beam.map_or([0.0; 4], |(a, w, h)| [a.x.atan2(-a.z), a.y.clamp(-1.0, 1.0).asin(), w, h]);
        data.extend_from_slice(&[l.at.x, l.at.y, l.at.z, l.radius]);
        data.extend_from_slice(&[color.x, color.y, color.z, l.intensity]);
        data.extend_from_slice(&beam);
        data.extend_from_slice(&[l.omni, 0.0, LAMP_SYSTEM, 0.0]);
        data.extend_from_slice(&[0.0; 4]);
    }
    data
}

/// A beam's axis in render axes, from its heading (from north) and elevation.
pub fn axis(heading: f64, elevation: f64) -> Vec3 {
    let (h, e) = (heading as f32, elevation as f32);
    Vec3::new(h.sin() * e.cos(), e.sin(), -h.cos() * e.cos())
}

/// The instance buffer's layout: five `vec4`s at locations 0 to 4.
pub fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
    wgpu::VertexBufferLayout { array_stride: (FLOATS * 4) as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &ATTRS }
}

/// The colour of a light in linear sRGB, of luminance 1, so that its intensity in candelas is its
/// brightness. From the chromaticities of Annex 14, Appendix 1; the red is `AVIATION_RED` in lights.wgsl.
pub fn rgb(color: Color) -> Vec3 {
    let c = match color {
        Color::White => Vec3::new(1.0, 0.66, 0.36),
        Color::Strobe => Vec3::new(0.94, 0.97, 1.0),
        Color::Yellow => Vec3::new(1.0, 0.55, 0.02),
        Color::Green => Vec3::new(0.0, 1.0, 0.5),
        Color::Red => Vec3::new(1.0, 0.02, 0.0),
        Color::Blue => Vec3::new(0.05, 0.12, 1.0),
        Color::Flood => Vec3::new(1.0, 0.82, 0.64),
    };
    c / luminance(c)
}

fn luminance(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// The scene's brightness per candela per square metre at an hour: what the lights are multiplied
/// by to stand in the scene as they stand in the world. A white surface under `E` lux shines with
/// `E / pi` cd/m², and the scene draws it as the sun's `SUN_SCALE` for `SUN_LUX`, times the exposure.
pub fn gain(hour: f64) -> f32 {
    (PI * SUN_SCALE as f64 / SUN_LUX) as f32 * exposure(sun_elevation(hour))
}

/// How much of each system is lit (`sim_core::lights::setting`), in the order of `System::ALL`.
pub fn settings(hour: f64, visibility: f64) -> [f32; 8] {
    System::ALL.map(|s| lights::setting(s, sun_elevation(hour), visibility) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_have_luminance_one_and_the_red_is_the_shaders() {
        for c in [Color::White, Color::Strobe, Color::Yellow, Color::Green, Color::Red, Color::Blue, Color::Flood] {
            assert!((luminance(rgb(c)) - 1.0).abs() < 1e-5);
        }
        let shader = include_str!("lights.wgsl");
        let red = rgb(Color::Red);
        assert!(shader.contains(&format!("const AVIATION_RED = vec3<f32>({:.3}, {:.3}, {:.3});", red.x, red.y, red.z)));
    }

    #[test]
    fn lights_stand_out_more_the_darker_it_is() {
        // By day the scene is near the real thing's brightness over 15 000; at night it is drawn far brighter.
        assert!((5e-5..1e-4).contains(&gain(12.0)), "{}", gain(12.0));
        assert!(gain(19.5) > gain(12.0) * 10.0);
        assert!(gain(23.0) > gain(19.5));
    }

    #[test]
    fn every_light_is_one_instance() {
        let t = Terrain::new(1);
        let a = t.nearest_airport(0.0, 0.0).unwrap();
        let lights = lights::aerodrome(&t, &a);
        let data = instances(&lights, DVec3::new(a.n, a.e, -a.elevation));
        assert_eq!(data.len(), lights.len() * FLOATS);
        assert!(data.iter().all(|x| x.is_finite()));
    }
}
