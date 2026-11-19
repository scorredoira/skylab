//! The graphics device and the targets a frame is drawn into.

use crate::post::{Post, PostProcess, SCENE_FORMAT};
use std::sync::Arc;

/// Device, queue and render targets, for a window or for an off-screen image.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The final image's format (the window's or the off-screen image's): the overlay draws into it.
    pub color_format: wgpu::TextureFormat,
    /// The scene pass's format: linear light in floating point, values above 1 kept (`Rgba16Float`).
    pub scene_format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    /// Samples per pixel of the scene pass: 4, or 1 for no smoothing of edges. Changed with `set_samples`.
    pub sample_count: u32,
    pub width: u32,
    pub height: u32,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    /// Whether the window's picture can be copied from (`set_capture`).
    copyable: bool,
    offscreen: Option<wgpu::Texture>,
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    /// The colour of the first pass, resolved, for the second to read (`keep_scene`, `scene_kept`).
    kept: Option<wgpu::TextureView>,
    keep: bool,
    /// Counts the times the scene's targets were made again (size, samples): a bind group of
    /// `scene_depth` made at another count is stale.
    targets_made: u64,
    post: PostProcess,
}

/// One frame being drawn.
pub struct Frame {
    pub encoder: wgpu::CommandEncoder,
    /// The final image: the window's surface or the off-screen texture.
    pub view: wgpu::TextureView,
    surface: Option<wgpu::SurfaceTexture>,
}

fn targets(device: &wgpu::Device, format: wgpu::TextureFormat, depth_format: wgpu::TextureFormat, samples: u32, w: u32, h: u32) -> (wgpu::TextureView, wgpu::TextureView) {
    let make = |format, label, usage| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    // The depth is also read, by the second pass (`Gpu::translucent_pass`).
    let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
    (make(format, "msaa scene", attachment), make(depth_format, "depth", attachment | wgpu::TextureUsages::TEXTURE_BINDING))
}

impl Gpu {
    pub const SAMPLES: u32 = 4;
    pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    async fn device(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'static>>) -> (wgpu::Adapter, wgpu::Device, wgpu::Queue) {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("no graphics adapter");
        // A shader stage may read more textures than WebGPU's least (16) where the adapter allows
        // it (Metal: 31 and more): the world's shaders read 17 in the clouds' march.
        let required_limits = wgpu::Limits { max_sampled_textures_per_shader_stage: adapter.limits().max_sampled_textures_per_shader_stage.clamp(16, 32), ..Default::default() };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor { label: Some("device"), required_limits, ..Default::default() })
            .await
            .expect("no graphics device");
        (adapter, device, queue)
    }

    /// For a window. `width` and `height` are in physical pixels.
    pub async fn for_window(window: Arc<winit::window::Window>, width: u32, height: u32) -> Gpu {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).expect("no surface");
        let (adapter, device, queue) = Self::device(&instance, Some(&surface)).await;
        let caps = surface.get_capabilities(&adapter);
        let color_format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: color_format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let mut gpu = Self::with_targets(device, queue, color_format, width, height, Some(surface), Some(config), None);
        gpu.copyable = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        gpu
    }

    /// For rendering to an image, without a window.
    pub async fn headless(width: u32, height: u32) -> Gpu {
        let instance = wgpu::Instance::default();
        let (_, device, queue) = Self::device(&instance, None).await;
        let color_format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let offscreen = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("off-screen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self::with_targets(device, queue, color_format, width, height, None, None, Some(offscreen))
    }

    #[allow(clippy::too_many_arguments)]
    fn with_targets(
        device: wgpu::Device,
        queue: wgpu::Queue,
        color_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        surface: Option<wgpu::Surface<'static>>,
        config: Option<wgpu::SurfaceConfiguration>,
        offscreen: Option<wgpu::Texture>,
    ) -> Gpu {
        let (msaa, depth) = targets(&device, SCENE_FORMAT, Self::DEPTH, Self::SAMPLES, width, height);
        let post = PostProcess::new(&device, color_format, width, height);
        Gpu { device, queue, color_format, scene_format: SCENE_FORMAT, depth_format: Self::DEPTH, sample_count: Self::SAMPLES, width, height, surface, config, copyable: true, offscreen, msaa, depth, kept: None, keep: false, targets_made: 0, post }
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width == self.width && height == self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        if let (Some(surface), Some(config)) = (&self.surface, &mut self.config) {
            config.width = width;
            config.height = height;
            surface.configure(&self.device, config);
        }
        let (msaa, depth) = targets(&self.device, self.scene_format, self.depth_format, self.sample_count, width, height);
        self.msaa = msaa;
        self.depth = depth;
        self.kept = None;
        self.targets_made += 1;
        self.post.resize(&self.device, width, height);
    }

    /// Whether the picture of each frame (`picture`) can be copied from: for a recording of the
    /// window (`video`). Off, the window's picture is only drawn into, which may be a little faster.
    /// False when it cannot be.
    pub fn set_capture(&mut self, on: bool) -> bool {
        if !self.copyable {
            return !on;
        }
        if let (Some(surface), Some(config)) = (&self.surface, &mut self.config) {
            let usage = if on { wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC } else { wgpu::TextureUsages::RENDER_ATTACHMENT };
            if config.usage != usage {
                config.usage = usage;
                surface.configure(&self.device, config);
            }
        }
        true
    }

    /// The texture of a frame's final image: the window's, or the off-screen one.
    pub fn picture<'a>(&'a self, frame: &'a Frame) -> &'a wgpu::Texture {
        match &frame.surface {
            Some(s) => &s.texture,
            None => self.offscreen.as_ref().expect("a frame without a window is drawn off-screen"),
        }
    }

    /// Changes the samples per pixel of the scene pass to 4 or 1, the counts every GPU has; anything
    /// else is taken as 4. Pipelines made for the scene pass before must be made again.
    pub fn set_samples(&mut self, samples: u32) {
        let samples = if samples == 1 { 1 } else { Self::SAMPLES };
        if samples == self.sample_count {
            return;
        }
        self.sample_count = samples;
        let (msaa, depth) = targets(&self.device, self.scene_format, self.depth_format, samples, self.width, self.height);
        self.msaa = msaa;
        self.depth = depth;
        self.kept = None;
        self.targets_made += 1;
    }

    /// Whether the scene pass that comes, when a second pass follows it, also leaves its colour
    /// (resolved to one sample per pixel) in a picture the second pass can read: `scene_kept`.
    /// For what is drawn over the scene out of the scene itself (a wet road that mirrors what
    /// stands on it, glass with drops that bend what is behind them). The loop asks the game
    /// (`Game::reads_scene`) every frame. Only with several samples per pixel (the resolve is
    /// what fills the picture); with one there is none.
    pub fn keep_scene(&mut self, keep: bool) {
        self.keep = keep && self.sample_count > 1;
        if self.keep && self.kept.is_none() {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene kept"),
                size: wgpu::Extent3d { width: self.width.max(1), height: self.height.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.scene_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.kept = Some(texture.create_view(&Default::default()));
        }
    }

    /// The first pass's colour, for a bind group read in `translucent_pass`: there when
    /// `keep_scene` asked for it this frame. Made again with the targets (`targets_made`).
    pub fn scene_kept(&self) -> Option<&wgpu::TextureView> {
        self.kept.as_ref().filter(|_| self.keep)
    }

    /// Starts a frame. `None` when the window cannot be drawn to right now (hidden, minimised, behind
    /// other windows, on another desktop). The uploads the game queued for the frame are sent all
    /// the same (see `skip_frame`); the caller should wait a little before trying again.
    pub fn begin(&mut self) -> Option<Frame> {
        let encoder = self.device.create_command_encoder(&Default::default());
        if let Some(surface) = &self.surface {
            let texture = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    self.skip_frame();
                    return None;
                }
                _ => {
                    if let Some(config) = &self.config {
                        surface.configure(&self.device, config);
                    }
                    self.skip_frame();
                    return None;
                }
            };
            let view = texture.texture.create_view(&Default::default());
            Some(Frame { encoder, view, surface: Some(texture) })
        } else {
            let view = self.offscreen.as_ref()?.create_view(&Default::default());
            Some(Frame { encoder, view, surface: None })
        }
    }

    /// For a frame that is not drawn. `queue.write_buffer` and `write_texture` hold their data in a
    /// staging buffer until the next `submit` and free it only when that submission is done. A
    /// window that stays hidden never submitted, so every frame's uploads piled up in GPU memory
    /// (2 GB a second; a 48 GB Mac ended in a kernel panic). An empty submit sends them.
    fn skip_frame(&self) {
        self.queue.submit([]);
    }

    /// The pass the 3D scene is drawn in: linear light in `scene_format`, multisampled unless
    /// `sample_count` is 1, with reversed depth cleared to 0 (infinitely far). `post_pass` makes the
    /// picture from it. With `then_translucent` the colour and the depth are kept for
    /// `translucent_pass`, which must follow; without it nothing is kept but the picture.
    pub fn scene_pass<'a>(&'a self, frame: &'a mut Frame, then_translucent: bool) -> wgpu::RenderPass<'a> {
        self.scene_pass_keeping(frame, then_translucent, false)
    }

    /// `scene_pass`, keeping the depth for `post_pass` when `keep_depth` (a `Post` that reads it:
    /// `Post::wants_depth`).
    pub fn scene_pass_keeping<'a>(&'a self, frame: &'a mut Frame, then_translucent: bool, keep_depth: bool) -> wgpu::RenderPass<'a> {
        // With one sample there is nothing to resolve: the scene is drawn straight into what the post-processing reads.
        let scene = self.post.scene();
        let (view, resolve_target, store) = match (self.sample_count > 1, then_translucent) {
            (true, false) => (&self.msaa, Some(scene), wgpu::StoreOp::Discard),
            (true, true) => (&self.msaa, self.scene_kept(), wgpu::StoreOp::Store),
            (false, _) => (scene, None, wgpu::StoreOp::Store),
        };
        let depth_store = if then_translucent || keep_depth { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard };
        frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: depth_store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// A second pass over the scene, after `scene_pass(frame, true)`: the same colour, and the depth
    /// tested but not written, so that the shaders can read it at the same time (`scene_depth`) for
    /// what is see-through and soft against what it meets (smoke, fire). Pipelines drawn in it must
    /// not write depth (`Depth::Read` or `Depth::Off`).
    pub fn translucent_pass<'a>(&'a self, frame: &'a mut Frame) -> wgpu::RenderPass<'a> {
        let scene = self.post.scene();
        let (view, resolve_target) = if self.sample_count > 1 { (&self.msaa, Some(scene)) } else { (scene, None) };
        frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("translucent"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: if self.sample_count > 1 { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store } },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.depth, depth_ops: None, stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// The scene's depth, for a bind group read in `translucent_pass`: `texture_depth_multisampled_2d`
    /// when `sample_count` is above 1, `texture_depth_2d` otherwise (reversed: 1 at the near plane,
    /// 0 at infinity). It is made again with the targets: remake the bind group when `targets_made`
    /// changes.
    pub fn scene_depth(&self) -> &wgpu::TextureView {
        &self.depth
    }

    /// How many times the scene's targets have been made again since the start (a new size, other samples).
    pub fn targets_made(&self) -> u64 {
        self.targets_made
    }

    /// Makes the final image from the scene, as `post` says, `dt` seconds after the last frame (for
    /// the exposure's adaptation): bloom, exposure, tone mapping.
    pub fn post_pass(&mut self, frame: &mut Frame, post: &Post, dt: f64) {
        self.post.encode(&self.device, &self.queue, &mut frame.encoder, &frame.view, self.color_format.is_srgb(), post, dt, (&self.depth, self.sample_count));
    }

    /// The exposure the last `post_pass` gave the scene, in stops (after its adaptation). Waits for
    /// the GPU: for tests and measurements.
    pub fn exposure(&self) -> f32 {
        self.post.exposure(&self.device, &self.queue)
    }

    /// A pass over the finished image, for the 2D overlay.
    pub fn overlay_pass<'a>(&self, frame: &'a mut Frame) -> wgpu::RenderPass<'a> {
        frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    pub fn finish(&self, frame: Frame) {
        self.queue.submit([frame.encoder.finish()]);
        if let Some(texture) = frame.surface {
            self.queue.present(texture);
        }
    }

    /// The off-screen image as RGBA bytes, row by row from the top.
    pub fn read_pixels(&self) -> Vec<u8> {
        let texture = self.offscreen.as_ref().expect("read_pixels needs a headless Gpu");
        let (w, h) = (self.width, self.height);
        let row = (w * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map failed"));
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("buffer not mapped");
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let start = (y * row) as usize;
            out.extend_from_slice(&data[start..start + (w * 4) as usize]);
        }
        out
    }

    /// Waits until the GPU has finished everything submitted so far.
    pub fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}
