//! The effects' pass on the GPU: the particles of `effects.rs` in one storage buffer, drawn in one
//! call in the second pass over the scene (`Gpu::translucent_pass`), reading the scene's depth so
//! that smoke meets the ground without a seam; and the 3D noise they and the fire's simulation are
//! carved with. Their pipelines and the noise are made in the background (`effect_pipelines.rs`);
//! the buffers grow at the first wreck.

use crate::effect_pipelines::{Drawing, Target};
use crate::effects::{Kind, Particle};
use crate::{shader_source, to_render, Quality};
use engine::{wgpu, Gpu};
use glam::DVec3;
use std::mem::size_of;
use std::sync::Arc;

/// One particle as `effects.wgsl` reads it (`struct Particle`): positions relative to the camera,
/// render axes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuParticle {
    pub pos: [f32; 3],
    pub size: f32,
    pub vel: [f32; 3],
    pub kind: f32,
    pub color: [f32; 4],
    /// x = temperature, y = seed, z = age, w = shade.
    pub params: [f32; 4],
}

const _: () = assert!(size_of::<GpuParticle>() == 64);

/// The effects' own uniforms (`struct Effects` in effects.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EffectUniforms {
    /// x = near distance of the view, y = pixels per unit of the tangent of the angle from the
    /// centre of the view (how big a thing looks), z = time, w unused.
    pub view: [f32; 4],
}

/// Side of the noise volume, texels.
const NOISE: u32 = 128;

/// The particles to draw this frame, the farthest first, relative to the camera at `eye`.
pub fn sorted(particles: &[Particle], eye: DVec3, out: &mut Vec<GpuParticle>) {
    out.clear();
    out.extend(particles.iter().map(|p| GpuParticle {
        pos: to_render(p.at - eye).to_array(),
        size: p.size,
        vel: to_render(p.vel).to_array(),
        kind: p.kind as u32 as f32,
        color: p.color,
        params: [p.temp, p.seed, p.age, p.shade],
    }));
    let key = |p: &GpuParticle| {
        let d = glam::Vec3::from(p.pos).length();
        // What adds light can go in any order; the rest from far to near.
        if p.kind == Kind::Spark as u32 as f32 || p.kind == Kind::Flash as u32 as f32 {
            d
        } else {
            d - p.size * 0.5
        }
    };
    out.sort_by(|a, b| key(b).total_cmp(&key(a)));
}

/// The pipeline of the particles for a quality and a target, and the layout of its group 1.
pub struct Pipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl Pipeline {
    pub fn new(device: &wgpu::Device, target: Target, frame_layout: &wgpu::BindGroupLayout, quality: &Quality) -> Pipeline {
        // The depth's binding depends on the samples.
        let layout = effect_layout(device, target.samples);
        let source = effects_source(quality, target.samples);
        let shader = engine::pipeline::shader_with_ending_loops(device, "effects", &source);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("effects"), bind_group_layouts: &[Some(frame_layout), Some(&layout)], immediate_size: 0 });
        let pipeline = blended_pipeline(device, target, "effects", &pipeline_layout, &shader, "particle_vertex", "particle_fragment");
        Pipeline { layout, pipeline }
    }
}

pub struct ParticlePass {
    uniforms: wgpu::Buffer,
    buffer: wgpu::Buffer,
    capacity: usize,
    sampler: wgpu::Sampler,
    /// The pipelines this frame draws with (`EffectPipelines`).
    drawing: Option<Arc<Drawing>>,
    /// The bind group, and the `Gpu::targets_made`, capacity and pipelines it was made for.
    bind: Option<(wgpu::BindGroup, u64, usize, Arc<Drawing>)>,
    count: u32,
}

impl ParticlePass {
    pub fn new(gpu: &Gpu) -> ParticlePass {
        let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("effects"), size: size_of::<EffectUniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        ParticlePass { uniforms, buffer: particle_buffer(gpu, 1), capacity: 1, sampler: gpu.texture_sampler(), drawing: None, bind: None, count: 0 }
    }

    /// Uploads this frame's particles (`sorted`) and makes what drawing them needs, with the
    /// pipelines of `drawing` and the noise of `EffectPipelines`.
    pub fn prepare(&mut self, gpu: &Gpu, drawing: &Arc<Drawing>, noise: &wgpu::TextureView, uniforms: &EffectUniforms, particles: &[GpuParticle]) {
        self.count = particles.len() as u32;
        if particles.is_empty() {
            return;
        }
        if particles.len() > self.capacity {
            self.capacity = particles.len().next_power_of_two();
            self.buffer = particle_buffer(gpu, self.capacity);
        }
        self.drawing = Some(drawing.clone());
        if self.bind.as_ref().is_none_or(|(_, made, capacity, d)| *made != gpu.targets_made() || *capacity != self.capacity || !Arc::ptr_eq(d, drawing)) {
            let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effects"),
                layout: &drawing.particles.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(gpu.scene_depth()) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(noise) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.bind = Some((bind, gpu.targets_made(), self.capacity, drawing.clone()));
        }
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
        gpu.queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(particles));
    }

    /// Draws a range of the particles (in the order given to `prepare`); group 0 must be bound.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, range: std::ops::Range<u32>) {
        let (Some(drawing), Some((bind, ..))) = (&self.drawing, &self.bind) else { return };
        let range = range.start..range.end.min(self.count);
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&drawing.particles.pipeline);
        pass.set_bind_group(1, bind, &[]);
        pass.draw(0..6, range);
    }
}

/// The world's shaders with heat.wgsl and the effects' after them, for `samples` per pixel.
pub fn effects_source(quality: &Quality, samples: u32) -> String {
    let source = shader_source(quality) + include_str!("heat.wgsl") + include_str!("effects.wgsl");
    if samples > 1 {
        source
    } else {
        source.replace("texture_depth_multisampled_2d", "texture_depth_2d")
    }
}

/// A pipeline of the second pass: tested against the scene's depth, not writing it, blended
/// premultiplied (what glows adds, what is dark covers), no culling.
pub fn blended_pipeline(device: &wgpu::Device, target: Target, label: &str, layout: &wgpu::PipelineLayout, shader: &wgpu::ShaderModule, vertex: &str, fragment: &str) -> wgpu::RenderPipeline {
    let premultiplied = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState { module: shader, entry_point: Some(vertex), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState { format: target.scene, blend: Some(wgpu::BlendState { color: premultiplied, alpha: premultiplied }), write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: target.depth,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: target.samples, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

/// The binding of the scene's depth, for `samples` per pixel.
pub fn depth_entry(binding: u32, samples: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: samples > 1 },
        count: None,
    }
}

/// Group 1 of the particles: their uniforms and buffer, the scene's depth, the noise and its sampler.
fn effect_layout(device: &wgpu::Device, samples: u32) -> wgpu::BindGroupLayout {
    let stages = wgpu::ShaderStages::VERTEX_FRAGMENT;
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("effects"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: stages,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(size_of::<EffectUniforms>() as u64) },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: stages, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None },
            depth_entry(2, samples),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: stages,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 4, visibility: stages, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ],
    })
}

fn particle_buffer(gpu: &Gpu, capacity: usize) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("particles"), size: (capacity * size_of::<GpuParticle>()) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

/// The noise volume, made on the GPU by `noise.wgsl`.
pub fn make_noise(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    noise_texture(device, queue).create_view(&Default::default())
}

fn noise_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("effects noise"),
        size: wgpu::Extent3d { width: NOISE, height: NOISE, depth_or_array_layers: NOISE },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("effects noise"), source: wgpu::ShaderSource::Wgsl(include_str!("noise.wgsl").into()) });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("effects noise"), layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("effects noise"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(NOISE / 4, NOISE / 4, NOISE / 4);
    }
    queue.submit([encoder.finish()]);
    texture
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::wgpu::naga;

    /// The effects' shaders, on the world's of every quality and both sample counts, parse and validate without a GPU.
    #[test]
    fn effects_shaders_are_valid_wgsl() {
        for quality in [Quality::HIGH, Quality::LOW] {
            for samples in [1, 4] {
                let source = effects_source(&quality, samples);
                let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
                naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
                    .validate(&module)
                    .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            }
        }
        let source = include_str!("noise.wgsl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default()).validate(&module).unwrap();
    }

    /// The noise made on a real device: every channel uses all of its range about its middle,
    /// changes smoothly from texel to texel, and goes on across the volume's faces as smoothly as
    /// inside it (it repeats).
    #[test]
    fn the_noise_fills_its_range_and_repeats() {
        let gpu = engine::Offscreen::new(16, 16, 1.0).gpu;
        let texture = noise_texture(&gpu.device, &gpu.queue);
        let n = NOISE as usize;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (n * n * n * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(NOISE * 4), rows_per_image: Some(NOISE) } },
            wgpu::Extent3d { width: NOISE, height: NOISE, depth_or_array_layers: NOISE },
        );
        gpu.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map failed"));
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("buffer not mapped");
        let at = |x: usize, y: usize, z: usize, c: usize| data[((z % n * n + y % n) * n + x % n) * 4 + c] as f64 / 255.0;
        for c in 0..4 {
            let values: Vec<f64> = (0..n * n * n).map(|i| data[i * 4 + c] as f64 / 255.0).collect();
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            let mut sorted = values.clone();
            sorted.sort_by(|a, b| a.total_cmp(b));
            let (low, high) = values.iter().fold((1.0f64, 0.0f64), |(l, h), &v| (l.min(v), h.max(v)));
            assert!((0.42..0.58).contains(&mean) && low < 0.02 && high > 0.98 && sorted[sorted.len() / 10] < 0.2 && sorted[sorted.len() * 9 / 10] > 0.8, "channel {c}: mean {mean:.2}, {low:.2} to {high:.2}");
            // The mean step between neighbours along x, inside the volume and across its face.
            let step = |x: usize| (0..n * n).map(|i| (at(x + 1, i % n, i / n, c) - at(x, i % n, i / n, c)).abs()).sum::<f64>() / (n * n) as f64;
            let (inside, across) = (step(n / 2), step(n - 1));
            assert!(inside < 0.2 && across < inside * 1.6 + 0.01, "channel {c}: steps of {inside:.3} inside, {across:.3} across the face");
        }
    }
}
