//! The world mirrored in the sea: the land, the trees, the buildings, the objects and the
//! aerodromes' lights, drawn before the scene from under the water into a picture of their own at a
//! fraction of the screen's size (`Quality::reflection`), which the sea's surface reads
//! (`mirrored_seen` in shaders.wgsl).
//!
//! The sea is flat at height 0 (the simulation floats on it), so its mirror is exact: the same
//! draws as the scene's, through the view mirrored in the plane of the sea, leave in each pixel
//! what a flat sea there reflects. The waves only turn the ray, which the sea follows by reading
//! the picture a little off the pixel. What is under the sea is left out (`*_mirror_fragment`);
//! where nothing is, the picture keeps nothing and the sea mirrors its own sky and clouds. Each
//! surface is lit and hazed as seen from the eye, not along the way through the mirror: a little
//! less haze than the reflected path holds.

use crate::pipelines::FrameUniforms;
use engine::{wgpu, Gpu};
use glam::Mat4;
use std::mem::size_of;

pub struct Reflection {
    /// The mirror's frame uniforms: the scene's, with the view mirrored.
    pub buffer: wgpu::Buffer,
    /// Group 1 of the sea (`Pipelines::sea_layout`): the picture, or a texel of nothing.
    pub sea_bind: wgpu::BindGroup,
    view: Option<wgpu::TextureView>,
    blank: wgpu::TextureView,
    depth: Option<wgpu::TextureView>,
    /// The size of the picture, pixels; (0, 0) when there is none.
    size: (u32, u32),
}

impl Reflection {
    pub fn new(gpu: &Gpu, sea_layout: &wgpu::BindGroupLayout) -> Reflection {
        let blank = texture(gpu, "no mirror", gpu.scene_format, (1, 1), wgpu::TextureUsages::TEXTURE_BINDING);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mirror frame"),
            size: size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Reflection { buffer, sea_bind: sea_bind(gpu, sea_layout, &blank), view: None, blank, depth: None, size: (0, 0) }
    }

    /// Makes the picture for a screen of `width` by `height` drawn at one `divisor`th of it (0: no
    /// picture).
    pub fn resize(&mut self, gpu: &Gpu, sea_layout: &wgpu::BindGroupLayout, width: u32, height: u32, divisor: u32) {
        let size = width.checked_div(divisor).zip(height.checked_div(divisor)).map_or((0, 0), |(w, h)| (w.max(1), h.max(1)));
        if size == self.size {
            return;
        }
        self.size = size;
        if divisor == 0 {
            (self.view, self.depth) = (None, None);
        } else {
            let usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
            self.view = Some(texture(gpu, "mirror", gpu.scene_format, size, usage | wgpu::TextureUsages::TEXTURE_BINDING));
            self.depth = Some(texture(gpu, "mirror depth", gpu.depth_format, size, usage));
        }
        self.sea_bind = sea_bind(gpu, sea_layout, self.view.as_ref().unwrap_or(&self.blank));
    }

    /// Whether there is a picture to draw into.
    pub fn drawn(&self) -> bool {
        self.depth.is_some()
    }

    /// The view-projection of the mirror, for a scene's `view_proj` (camera-relative, render axes) and
    /// the camera's height above the sea: everything reflected in the plane y = -height.
    pub fn view_proj(view_proj: Mat4, height: f32) -> Mat4 {
        let mirror = Mat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -2.0 * height, 0.0, 1.0]);
        view_proj * mirror
    }

    /// A pass that draws into the picture, cleared to nothing; `None` without one.
    pub fn pass<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder) -> Option<wgpu::RenderPass<'a>> {
        let (view, depth) = (self.view.as_ref()?, self.depth.as_ref()?);
        Some(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mirror"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }))
    }
}

fn sea_bind(gpu: &Gpu, layout: &wgpu::BindGroupLayout, view: &wgpu::TextureView) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sea"), layout, entries: &[wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) }] })
}

fn texture(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, (width, height): (u32, u32), usage: wgpu::TextureUsages) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn the_mirror_reflects_in_the_sea() {
        // A camera 100 m up: a point 30 m above the sea is seen in the mirror where the point 30 m
        // below it is seen directly.
        let vp = Mat4::perspective_rh(1.0, 1.6, 0.25, 10_000.0) * Mat4::look_to_rh(Vec3::ZERO, Vec3::new(0.0, -0.2, -1.0).normalize(), Vec3::Y);
        let m = Reflection::view_proj(vp, 100.0);
        let above = Vec3::new(12.0, -70.0, -900.0);
        let below = Vec3::new(12.0, -130.0, -900.0);
        let (a, b) = (m.project_point3(above), vp.project_point3(below));
        assert!((a - b).length() < 1e-5, "{a} against {b}");
    }
}
