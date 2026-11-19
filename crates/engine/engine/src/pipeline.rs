//! Shorthand for the render pipelines a game needs.

use crate::Gpu;

#[derive(Clone, Copy, PartialEq)]
pub enum Depth {
    /// Tested and written: solid geometry.
    Write,
    /// Tested, not written: transparent things over the scene.
    Read,
    /// Ignored: backgrounds.
    Off,
}

pub struct PipelineDesc<'a> {
    pub label: &'a str,
    pub shader: &'a wgpu::ShaderModule,
    pub vertex: &'a str,
    pub fragment: &'a str,
    pub buffers: &'a [wgpu::VertexBufferLayout<'a>],
    pub bind_groups: &'a [&'a wgpu::BindGroupLayout],
    pub depth: Depth,
    pub blend: bool,
}

impl Gpu {
    pub fn shader(&self, label: &str, source: &str) -> wgpu::ShaderModule {
        self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) })
    }

    /// As `shader`, for a source in which **every loop ends by itself** (a counter against a constant or
    /// a number from a uniform; never `loop {}` or a condition that may stay true). wgpu otherwise adds a
    /// second counter to every loop so that none can run for ever, and the GPU pays for it at every
    /// turn: in the flight simulator, whose ground shader loops over roads, airports and shadow steps
    /// per pixel, that was 1.2 ms of a 13.8 ms frame at 2880x1800 on an Apple M5 Pro. The other checks
    /// (array bounds, integer division) stay; they measured nothing.
    pub fn shader_with_ending_loops(&self, label: &str, source: &str) -> wgpu::ShaderModule {
        shader_with_ending_loops(&self.device, label, source)
    }

    /// What makes the scene pass's pipelines, for `samples` per pixel (the scene pass's now, or the
    /// count it will have): it can be sent to another thread, so that the driver's compiling of a
    /// big shader does not hold up the frames.
    pub fn pipeline_maker(&self, samples: u32) -> PipelineMaker {
        PipelineMaker { device: self.device.clone(), scene_format: self.scene_format, depth_format: self.depth_format, sample_count: samples }
    }

    /// A pipeline that draws into the scene pass (linear HDR colour, multisampled, reversed depth, no culling).
    pub fn pipeline(&self, d: &PipelineDesc) -> wgpu::RenderPipeline {
        self.pipeline_maker(self.sample_count).pipeline(d)
    }

    /// As [`Gpu::pipeline`], with a blend of its own instead of `d.blend`'s: for what the usual
    /// "over" cannot say (a surface that lets through part of what is behind it and covers another part).
    pub fn pipeline_blended(&self, d: &PipelineDesc, blend: Option<wgpu::BlendState>) -> wgpu::RenderPipeline {
        self.pipeline_maker(self.sample_count).pipeline_blended(d, blend)
    }

    /// As [`Gpu::pipeline_blended`], for a target of the scene's formats with `samples` per pixel
    /// of its own instead of the scene pass's: a picture the game draws before the scene and reads
    /// in it (a reflection).
    pub fn pipeline_with_samples(&self, d: &PipelineDesc, blend: Option<wgpu::BlendState>, samples: u32) -> wgpu::RenderPipeline {
        self.pipeline_maker(samples).pipeline_blended(d, blend)
    }
}

/// The device and the scene pass's formats and samples: [`Gpu`]'s pipeline functions without the
/// rest of it, to be used on another thread (`Gpu::pipeline_maker`).
#[derive(Clone)]
pub struct PipelineMaker {
    pub device: wgpu::Device,
    pub scene_format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    pub sample_count: u32,
}

impl PipelineMaker {
    /// `Gpu::shader_with_ending_loops`.
    pub fn shader_with_ending_loops(&self, label: &str, source: &str) -> wgpu::ShaderModule {
        shader_with_ending_loops(&self.device, label, source)
    }

    /// `Gpu::pipeline`.
    pub fn pipeline(&self, d: &PipelineDesc) -> wgpu::RenderPipeline {
        self.pipeline_blended(d, d.blend.then_some(wgpu::BlendState::ALPHA_BLENDING))
    }

    /// `Gpu::pipeline_blended`.
    pub fn pipeline_blended(&self, d: &PipelineDesc, blend: Option<wgpu::BlendState>) -> wgpu::RenderPipeline {
        self.pipeline_with_samples(d, blend, self.sample_count)
    }

    /// `Gpu::pipeline_with_samples`.
    pub fn pipeline_with_samples(&self, d: &PipelineDesc, blend: Option<wgpu::BlendState>, samples: u32) -> wgpu::RenderPipeline {
        let groups: Vec<Option<&wgpu::BindGroupLayout>> = d.bind_groups.iter().map(|g| Some(*g)).collect();
        let buffers: Vec<Option<wgpu::VertexBufferLayout>> = d.buffers.iter().cloned().map(Some).collect();
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(d.label), bind_group_layouts: &groups, immediate_size: 0 });
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(d.label),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: d.shader, entry_point: Some(d.vertex), buffers: &buffers, compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: d.shader,
                entry_point: Some(d.fragment),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.scene_format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: self.depth_format,
                depth_write_enabled: Some(d.depth == Depth::Write),
                depth_compare: Some(if d.depth == Depth::Off { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::GreaterEqual }),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        })
    }
}

/// `Gpu::shader_with_ending_loops` with only the device, for a pipeline made on another thread (the
/// driver's compiling of a big shader can take a long time, which a frame should not wait for).
pub fn shader_with_ending_loops(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    let mut checks = wgpu::ShaderRuntimeChecks::checked();
    checks.force_loop_bounding = false;
    let desc = wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) };
    // SAFETY: the only check turned off is the forced bound on loops, whose absence is undefined
    // behaviour only for a loop that never ends; the caller gives a source without such loops,
    // which is this function's contract. The source is the game's own, never from outside.
    unsafe { device.create_shader_module_trusted(desc, checks) }
}
