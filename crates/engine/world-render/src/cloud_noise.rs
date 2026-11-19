//! Ported from takram-design-engineering/three-geospatial (packages/clouds/src/CloudShape.ts,
//! CloudShapeDetail.ts, Turbulence.ts, LocalWeather.ts, Procedural3DTexture.ts,
//! ProceduralTexture.ts), MIT, see THIRD_PARTY_NOTICES.md.
//!
//! The textures the volumetric clouds are made of, drawn once on the GPU by `cloud_noise.wgsl`:
//! the weather (which layer has cloud where), the shape and its detail (volumes that tile) and the
//! turbulence. And the blue noise that places the samples of the marches, which is ours: theirs is
//! NVIDIA's spatiotemporal blue noise, a file whose licence we could not establish.

use engine::{wgpu, Gpu};

/// Sides of the textures, texels.
pub const SHAPE: u32 = 128;
pub const DETAIL: u32 = 32;
pub const TURBULENCE: u32 = 128;
pub const WEATHER: u32 = 512;
pub const BLUE: usize = 64;

/// The clouds' textures.
pub struct Noise {
    /// r, g, b = how much cloud the low, the middle and the high layer have over a place; with its smaller levels.
    pub weather: wgpu::TextureView,
    pub shape: wgpu::TextureView,
    pub detail: wgpu::TextureView,
    /// A direction (0.5 + 0.5 of it) the shapes are pushed along.
    pub turbulence: wgpu::TextureView,
    pub blue: wgpu::TextureView,
}

fn made(gpu: &Gpu, label: &str, size: [u32; 3], levels: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: size[2] },
        mip_level_count: levels,
        sample_count: 1,
        dimension: if size[2] > 1 { wgpu::TextureDimension::D3 } else { wgpu::TextureDimension::D2 },
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Draws the textures: a draw to a layer of each volume, one each for the turbulence and the
/// weather, and one to each smaller level of the weather.
pub fn textures(gpu: &Gpu) -> Noise {
    let d = &gpu.device;
    let module = gpu.shader("cloud noise", include_str!("cloud_noise.wgsl"));
    let (grey, colour) = (wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rgba8Unorm);
    let pipeline = |entry: &str, format| {
        d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("noise_vertex"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let levels = WEATHER.ilog2() + 1;
    let weather = made(gpu, "cloud weather", [WEATHER, WEATHER, 1], levels, colour);
    let shape = made(gpu, "cloud shape", [SHAPE; 3], 1, grey);
    let detail = made(gpu, "cloud detail", [DETAIL; 3], 1, grey);
    let turbulence = made(gpu, "cloud turbulence", [TURBULENCE, TURBULENCE, 1], 1, colour);

    let mut encoder = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cloud noise") });
    let mut draw = |view: &wgpu::TextureView, layer: Option<u32>, pipeline: &wgpu::RenderPipeline, group: Option<&wgpu::BindGroup>| {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cloud noise"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: layer, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        if let Some(group) = group {
            pass.set_bind_group(0, group, &[]);
        }
        let layer = layer.unwrap_or(0);
        pass.draw(0..3, layer..layer + 1);
    };
    for (texture, side, entry) in [(&shape, SHAPE, "shape_fragment"), (&detail, DETAIL, "detail_fragment")] {
        let pipeline = pipeline(entry, grey);
        let view = texture.create_view(&Default::default());
        for layer in 0..side {
            draw(&view, Some(layer), &pipeline, None);
        }
    }
    draw(&turbulence.create_view(&Default::default()), None, &pipeline("turbulence_fragment", colour), None);
    let level = |level: u32| weather.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() });
    draw(&level(0), None, &pipeline("weather_fragment", colour), None);
    let halve = pipeline("halve_fragment", colour);
    let sampler = d.create_sampler(&wgpu::SamplerDescriptor { label: Some("cloud weather levels"), address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
    for to in 1..levels {
        let from = level(to - 1);
        let group = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cloud weather levels"),
            layout: &halve.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&from) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }],
        });
        draw(&level(to), None, &halve, Some(&group));
    }
    gpu.queue.submit([encoder.finish()]);

    let blue = made(gpu, "cloud blue noise", [BLUE as u32, BLUE as u32, 1], 1, grey);
    let extent = wgpu::Extent3d { width: BLUE as u32, height: BLUE as u32, depth_or_array_layers: 1 };
    gpu.queue.write_texture(blue.as_image_copy(), &blue_noise(BLUE), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(BLUE as u32), rows_per_image: Some(BLUE as u32) }, extent);
    let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
    Noise { weather: view(&weather), shape: view(&shape), detail: view(&detail), turbulence: view(&turbulence), blue: view(&blue) }
}

/// The sampler of the clouds' textures: they repeat, and the weather has its smaller levels.
pub fn sampler(gpu: &Gpu) -> wgpu::Sampler {
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("cloud noise"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    })
}

/// Blue noise, `side` texels each way, that tiles: every value 0..=255 as often as any other, and
/// neighbours as different as can be. By Ulichney's void and cluster ("The void-and-cluster method
/// for dither array generation", 1993): the texels are ranked one by one, each the one farthest
/// from those ranked already (where a blur of them is least).
pub fn blue_noise(side: usize) -> Vec<u8> {
    let n = side * side;
    // The blur of one texel over the others, round the edges.
    let kernel: Vec<f32> = (0..n)
        .map(|i| {
            let away = |v: usize| v.min(side - v) as f32;
            let (dx, dy) = (away(i % side), away(i / side));
            (-(dx * dx + dy * dy) / (2.0 * 1.9 * 1.9)).exp()
        })
        .collect();
    let spread = |energy: &mut [f32], at: usize, sign: f32| {
        let (ax, ay) = (at % side, at / side);
        for (i, e) in energy.iter_mut().enumerate() {
            let (x, y) = (i % side, i / side);
            *e += sign * kernel[(y + side - ay) % side * side + (x + side - ax) % side];
        }
    };
    let extreme = |energy: &[f32], on: &[bool], of_on: bool| {
        let mut best = usize::MAX;
        for i in 0..n {
            if on[i] == of_on && (best == usize::MAX || if of_on { energy[i] > energy[best] } else { energy[i] < energy[best] }) {
                best = i;
            }
        }
        best
    };
    // A tenth of the texels at random, then moved apart: the tightest cluster to the largest void, until they are the same.
    let mut on = vec![false; n];
    let mut energy = vec![0.0f32; n];
    let mut state = 0x9e37_79b9u32;
    let mut count = 0;
    while count < n / 10 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let i = (state >> 8) as usize % n;
        if !on[i] {
            on[i] = true;
            spread(&mut energy, i, 1.0);
            count += 1;
        }
    }
    loop {
        let cluster = extreme(&energy, &on, true);
        on[cluster] = false;
        spread(&mut energy, cluster, -1.0);
        let void = extreme(&energy, &on, false);
        on[void] = true;
        spread(&mut energy, void, 1.0);
        if void == cluster {
            break;
        }
    }
    let mut rank = vec![0usize; n];
    // The first ranks: those texels, from the most crowded down.
    let (mut left, mut left_energy) = (on.clone(), energy.clone());
    for r in (0..count).rev() {
        let cluster = extreme(&left_energy, &left, true);
        left[cluster] = false;
        spread(&mut left_energy, cluster, -1.0);
        rank[cluster] = r;
    }
    // The rest: the largest void each time.
    for r in count..n {
        let void = extreme(&energy, &on, false);
        on[void] = true;
        spread(&mut energy, void, 1.0);
        rank[void] = r;
    }
    rank.iter().map(|r| (r * 256 / n) as u8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blue_noise_has_every_value_as_often() {
        let noise = blue_noise(32);
        let mut counts = [0; 256];
        for v in &noise {
            counts[*v as usize] += 1;
        }
        assert!(counts.iter().all(|c| *c == 4));
    }

    /// Neighbours differ more than those of white noise, whose mean difference is a third of the range (85).
    #[test]
    fn blue_noise_is_blue() {
        let side = 32;
        let noise = blue_noise(side);
        let mut sum = 0.0;
        for y in 0..side {
            for x in 0..side {
                let at = |x: usize, y: usize| noise[y % side * side + x % side] as f64;
                sum += (at(x, y) - at(x + 1, y)).abs() + (at(x, y) - at(x, y + 1)).abs();
            }
        }
        let mean = sum / (2 * side * side) as f64;
        assert!(mean > 95.0, "{mean}");
    }
}
