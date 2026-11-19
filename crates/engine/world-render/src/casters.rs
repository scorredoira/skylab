//! The shadows of the vehicles drawn from their meshes (`Scene::casters`).
//!
//! Each frame the vehicles near the camera are drawn as the sun sees them, depth only, into a map of
//! their own: one square tile of `TILE` texels a side per vehicle, of an atlas of two by two tiles.
//! A tile is an orthographic view along the sunlight fitted round the sphere that holds its vehicle,
//! its middle moved by whole texels as the vehicle goes (the edges of the shadow do not crawl). The
//! world's shaders read it wherever the sun lights something (`caster_shadow` in shaders.wgsl): the
//! ground, the circuit, the grass, the buildings, the vehicles themselves. What a tile is, the
//! frame's uniforms say (`FrameUniforms::caster_*`).

use crate::{to_render, Caster};
use engine::{wgpu, Gpu};
use glam::{DVec3, Mat4, Vec3, Vec4};

/// Vehicles with a tile of their own; the body followed is the first.
pub const TILES: usize = 4;
/// Texels of a tile's side: some 6 mm for a car, 12 mm for a light aircraft.
pub const TILE: u32 = 1024;
/// Vehicles farther from the camera than this cast only through their `ShadowShape`, if any: their
/// shadow would be a few pixels.
pub const RANGE: f64 = 250.0;
/// The depth format of the map.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The atlas the vehicles are drawn into, and how the shaders read it.
pub struct CasterMap {
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl CasterMap {
    pub fn new(gpu: &Gpu) -> CasterMap {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vehicles' shadows"),
            size: wgpu::Extent3d { width: 2 * TILE, height: 2 * TILE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        // Each read compares four texels with the depth asked for and blends the answers: a
        // shadow's edge is smooth between texels.
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vehicles' shadows"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        CasterMap { view: texture.create_view(&Default::default()), sampler }
    }

    /// The pass that draws the tiles: cleared to nothing (the far side of every tile).
    pub fn pass<'e>(&self, encoder: &'e mut wgpu::CommandEncoder) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vehicles' shadows"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }
}

/// The axes of the map across the sunlight, right and up (render axes, unit), for the way to the sun
/// `sun`: right is level, so the tiles keep their texels' grid as the sun moves.
pub fn axes(sun: Vec3) -> (Vec3, Vec3) {
    let right = sun.cross(Vec3::Y).normalize_or(Vec3::X);
    (right, right.cross(sun).normalize_or(Vec3::Z))
}

/// A vehicle's tile this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    /// The middle of what it sees, render axes relative to the camera, and the half side, metres.
    pub centre: Vec3,
    pub radius: f32,
}

impl Tile {
    /// The tile of a vehicle seen from `cam` (world axes) with the sun along `sun` (render axes),
    /// its middle on the texels' grid of the world (not of the camera, which moves).
    pub fn of(caster: &Caster, cam: DVec3, sun: Vec3) -> Tile {
        // A little room round the sphere, in steps of 5 cm: the size of a texel stays put.
        let radius = ((caster.radius.max(0.1) * 1.04 / 0.05).ceil() * 0.05) as f64;
        let texel = 2.0 * radius / TILE as f64;
        let (right, up) = axes(sun);
        let (right, up) = (right.as_dvec3(), up.as_dvec3());
        // Render axes, but from the world's origin.
        let at = DVec3::new(caster.centre.y, -caster.centre.z, -caster.centre.x);
        let (a, b) = (at.dot(right), at.dot(up));
        let snap = |v: f64| (v / texel).round() * texel - v;
        let moved = right * snap(a) + up * snap(b);
        Tile { centre: to_render(caster.centre - cam) + moved.as_vec3(), radius: radius as f32 }
    }

    /// Render axes relative to the camera to the tile's clip space: x and y across the sunlight,
    /// -1 to 1 over the tile; depth along it, 0 on the sun's side of the sphere, 1 on the other.
    /// `caster_shadow` (shaders.wgsl) works out the same from the frame's uniforms.
    pub fn light(&self, sun: Vec3) -> Mat4 {
        let (right, up) = axes(sun);
        let r = self.radius;
        let c = self.centre;
        let row = |v: Vec3, scale: f32, offset: f32| Vec4::new(v.x * scale, v.y * scale, v.z * scale, -v.dot(c) * scale + offset);
        Mat4::from_cols(row(right, 1.0 / r, 0.0), row(up, 1.0 / r, 0.0), row(-sun, 0.5 / r, 0.5), Vec4::W).transpose()
    }

    /// Where in the atlas tile `i` is: x, y, side, in texels.
    pub fn viewport(i: usize) -> (f32, f32, f32) {
        ((i % 2) as f32 * TILE as f32, (i / 2) as f32 * TILE as f32, TILE as f32)
    }
}

/// Which of the casters get a tile: the first (the body followed) if near enough, then the nearest
/// others, up to `TILES`. Their indices.
pub fn chosen(casters: &[Caster], cam: DVec3) -> Vec<usize> {
    let near = |c: &Caster| (c.centre - cam).length() - c.radius as f64;
    let mut others: Vec<(f64, usize)> = casters.iter().enumerate().skip(1).map(|(i, c)| (near(c), i)).filter(|(d, _)| *d < RANGE).collect();
    others.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = casters.first().filter(|c| near(c) < RANGE).map(|_| 0);
    first.into_iter().chain(others.into_iter().map(|(_, i)| i)).take(TILES).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caster(centre: DVec3, radius: f32) -> Caster<'static> {
        Caster { centre, radius, draws: &[] }
    }

    /// The sphere of the vehicle lies within its tile, and the depth runs from the sun's side.
    #[test]
    fn a_tile_holds_its_vehicle() {
        let sun = Vec3::new(0.4, 0.5, -0.6).normalize();
        let c = caster(DVec3::new(120.3, -40.7, -1.2), 2.6);
        let cam = DVec3::new(110.0, -45.0, -3.0);
        let tile = Tile::of(&c, cam, sun);
        let m = tile.light(sun);
        let centre = to_render(c.centre - cam);
        let (right, up) = axes(sun);
        for v in [right, -right, up, -up, sun, -sun] {
            let p = m.project_point3(centre + v * c.radius);
            assert!(p.x.abs() < 1.0 && p.y.abs() < 1.0 && (0.0..=1.0).contains(&p.z), "{v} {p}");
        }
        let near = m.project_point3(centre + sun * c.radius);
        let far = m.project_point3(centre - sun * c.radius);
        assert!(near.z < far.z);
    }

    /// As the camera moves the tile stays on the world's grid of texels: its middle moves by whole texels.
    #[test]
    fn tiles_keep_to_their_texels() {
        let sun = Vec3::new(-0.3, 0.6, 0.5).normalize();
        let c = caster(DVec3::new(10.0, 20.0, -1.0), 2.5);
        let a = Tile::of(&c, DVec3::ZERO, sun);
        let b = Tile::of(&c, DVec3::new(3.3, -1.7, 0.4), sun);
        let (right, up) = axes(sun);
        let texel = 2.0 * a.radius / TILE as f32;
        let shift = (b.centre + to_render(DVec3::new(3.3, -1.7, 0.4))) - a.centre;
        for along in [shift.dot(right), shift.dot(up)] {
            let steps = along / texel;
            assert!((steps - steps.round()).abs() < 0.02, "{steps}");
        }
    }

    #[test]
    fn the_body_followed_comes_first_then_the_nearest() {
        let far = caster(DVec3::new(1000.0, 0.0, 0.0), 2.0);
        let list = [caster(DVec3::ZERO, 2.0), caster(DVec3::new(80.0, 0.0, 0.0), 2.0), far, caster(DVec3::new(20.0, 0.0, 0.0), 2.0)];
        assert_eq!(chosen(&list, DVec3::ZERO), vec![0, 3, 1]);
    }
}
