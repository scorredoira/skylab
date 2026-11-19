//! A 2D picture painted with egui into a texture, for a game to show on a surface of its scene: a
//! screen, a sign, a page. The same painter as the overlay, drawn off screen, with every mip level
//! made after each paint, so that the picture stays sharp and steady however small or slanted the
//! surface is seen (sample it with `Gpu::texture_sampler`: trilinear and anisotropic).

use crate::texture::mip_count;
use crate::Gpu;

/// An egui context that paints into its own texture instead of the window.
pub struct Canvas {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    /// Every mip level, to sample.
    view: wgpu::TextureView,
    /// Each mip level on its own: the first is painted, each other one drawn from the one before.
    levels: Vec<wgpu::TextureView>,
    /// Draws a level from the one before (`MIPS_WGSL`), and the bind group of each source level.
    mips: wgpu::RenderPipeline,
    sources: Vec<wgpu::BindGroup>,
    width: u32,
    height: u32,
    pixels_per_point: f32,
}

/// Half the size of a texture: each pixel the average of the four below it, as the bilinear sample
/// at their shared corner reads them (in linear light: the view is sRGB).
const MIPS_WGSL: &str = "
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn mip_vertex(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}
@fragment fn mip_fragment(in: Out) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, source_sampler, in.uv, 0.0);
}
";

impl Canvas {
    /// The texture's format: what shaders sample, with the colour already in sRGB.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// A canvas of `width` x `height` pixels; `pixels_per_point` sets the size of egui's points
    /// (its text, its strokes) in those pixels.
    pub fn new(gpu: &Gpu, label: &str, width: u32, height: u32, pixels_per_point: f32) -> Canvas {
        let mip_level_count = mip_count(width, height);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let levels: Vec<wgpu::TextureView> = (0..mip_level_count)
            .map(|level| texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() }))
            .collect();
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas mips"),
            entries: &[crate::texture::layout_entry(0), crate::texture::sampler_layout_entry(1)],
        });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("canvas mips"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sources = levels[..levels.len() - 1]
            .iter()
            .map(|level| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("canvas mips"),
                    layout: &layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(level) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }],
                })
            })
            .collect();
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("canvas mips"), source: wgpu::ShaderSource::Wgsl(MIPS_WGSL.into()) });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("canvas mips"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let mips = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas mips"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("mip_vertex"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("mip_fragment"),
                targets: &[Some(wgpu::ColorTargetState { format: Self::FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(pixels_per_point);
        let renderer = egui_wgpu::Renderer::new(&gpu.device, Self::FORMAT, egui_wgpu::RendererOptions::default());
        Canvas { ctx, renderer, view, levels, mips, sources, width, height, pixels_per_point }
    }

    /// The texture with all its mip levels, to bind as a `texture_2d<f32>`.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Size in egui points: what `paint` lays out in.
    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.width as f32, self.height as f32) / self.pixels_per_point
    }

    /// Clears the texture to `clear` (linear RGBA), runs `ui`, draws what it paints and makes the
    /// smaller mip levels from it, all in one submission of its own: call it from `Game::update`,
    /// before the frame that shows it.
    pub fn paint(&mut self, gpu: &Gpu, clear: [f64; 4], mut ui: impl FnMut(&egui::Context)) {
        let size = self.size();
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
        let output = self.ctx.run_ui(raw, |root| ui(root.ctx()));
        let jobs = self.ctx.tessellate(output.shapes, self.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [self.width, self.height], pixels_per_point: self.pixels_per_point };
        // Taken out of the output as they are applied: what is left unapplied in it when it is
        // dropped is an error to egui.
        let mut textures = output.textures_delta;
        for (id, deltas) in std::mem::take(&mut textures.set) {
            for delta in deltas.iter() {
                self.renderer.update_texture(&gpu.device, &gpu.queue, id, delta);
            }
        }
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("canvas") });
        self.renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.levels[0],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: clear[0], g: clear[1], b: clear[2], a: clear[3] }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        for (source, target) in self.sources.iter().zip(&self.levels[1..]) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas mip"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.mips);
            pass.set_bind_group(0, source, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        for id in std::mem::take(&mut textures.free) {
            self.renderer.free_texture(&id);
        }
    }
}
