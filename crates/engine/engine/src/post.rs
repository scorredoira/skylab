//! What turns the scene, drawn in linear light without limit (high dynamic range), into the picture:
//! a glow round what is bright (bloom), an exposure that follows the scene, and one tone mapping
//! (AgX) for the whole frame. The passes are in `post.wgsl`.

/// The format the scene is drawn in: linear light, values above 1 kept.
pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Levels of the bloom chain, the first at half the size of the frame, each half the one before.
const BLOOM_LEVELS: u32 = 6;
const EXPOSURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// Seconds the exposure takes to cover two thirds of the way to a darker scene, and to a brighter one.
const OPENING_TIME: f64 = 1.0;
const CLOSING_TIME: f64 = 0.5;

/// How the scene becomes the picture; a game says it with `Game::post`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Post {
    /// Exposure in stops (each doubles the light) of a scene whose average is mid grey (0.18).
    pub exposure: f32,
    /// How much of the difference between the scene's average and mid grey the exposure makes up
    /// for: 0 a fixed exposure, 1 every scene brought to mid grey; between, a dark scene stays darker,
    /// as the eye sees it. It moves there smoothly: in some 1 s to a darker scene, 0.5 s to a brighter.
    pub adaptation: f32,
    /// How much the exposure opens for what the eye looks at, 0...1: this share of the stops the
    /// middle of the picture (a soft spot a third of it wide) would be left more than 1.5 under mid
    /// grey is made up for, up to 3 stops, as the eye in a room opens for the wall it looks at and
    /// lets the window beside it glare; the glare then counts by how near the middle it is. A
    /// middle that is bright enough changes nothing. 0: the whole picture alone.
    pub spot: f32,
    /// How much the exposure stops down for a glare: the brightest patch of the scene (big glows, not
    /// points: it is measured on the blurred bloom chain) is kept within `GLARE_STOPS` above mid grey
    /// by this share, 0...1, as the eye closes down for a bright field near where it looks (a lit
    /// fog, a floodlit apron). 0 leaves the exposure to the average alone.
    pub glare: f32,
    /// Part of the light spread into a glow round it, 0...1; 0 draws none and saves its passes.
    pub bloom: f32,
    /// Contrast, 0...1: AgX spans 16.5 stops round mid grey (10 below, 6.5 above), which leaves the
    /// middle soft; the span is divided by 1 + `contrast`. Mid grey stays where it is.
    pub contrast: f32,
    /// Colour on the tone-mapped picture: 1 is AgX's, which pales colours on the way to white.
    pub saturation: f32,
    /// The white balance, warmer (positive) or cooler (negative) than the scene's, −1...1: the light
    /// is tinted towards amber or blue before the tone mapping, its brightness kept. 0 none.
    pub warmth: f32,
    /// How much darker the corners of the picture are than its middle, 0...1 (a lens's vignette); 0 none.
    pub vignette: f32,
    /// A blur of speed: each point of the picture smeared along the line from `blur_centre` (in
    /// the picture, 0...1 from its top left) by this fraction of its distance from it, growing from
    /// nothing near the centre, so what the camera follows stays sharp. 0 none, and no cost.
    pub blur: f32,
    pub blur_centre: [f32; 2],
    /// How much of that blur what is above the centre in the picture gets, 0...1: the sky and
    /// what is far, which do not rush by as the ground near the camera does. 1: all of it.
    pub blur_above: f32,
    /// The flare of a lens looking towards the sun: how strong (0 none) and where the sun is in
    /// the picture (0...1 from its top left; it may be outside). It shows as much as the sun does
    /// (measured on the bloom: hidden behind a hill or a cloud, it goes), so it needs the bloom.
    pub flare: f32,
    pub flare_sun: [f32; 2],
    /// A lens's depth of field: the distance it is focused on, metres (0: everything sharp, and
    /// no cost), how far either side of that stays sharp, the radius of the widest blur as a
    /// fraction of the picture's height (0.01: a long lens wide open), and the camera's near
    /// distance (`Camera::near`), which the scene's depth is read with.
    pub focus: f32,
    pub focus_span: f32,
    pub focus_blur: f32,
    pub near: f32,
}

impl Post {
    /// Whether making the picture reads the scene's depth (a depth of field): the scene pass keeps it then.
    pub fn wants_depth(&self) -> bool {
        self.focus > 0.0 && self.focus_blur > 0.0
    }
}

impl Default for Post {
    fn default() -> Post {
        Post { exposure: 0.0, adaptation: 0.0, spot: 0.0, glare: 0.0, bloom: 0.03, contrast: 0.0, saturation: 1.0, warmth: 0.0, vignette: 0.0, blur: 0.0, blur_centre: [0.5, 0.5], blur_above: 1.0, flare: 0.0, flare_sun: [0.5, 0.5], focus: 0.0, focus_span: 0.0, focus_blur: 0.0, near: 0.25 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    look: [f32; 4],
    more: [f32; 4],
    grade: [f32; 4],
    lens: [f32; 4],
    flare: [f32; 4],
    focus: [f32; 4],
}

/// What depends on the size of the frame.
struct Targets {
    scene: wgpu::TextureView,
    /// The scene through the lens's depth of field, when there is one (`Post::focus`), and the
    /// bind groups that read it instead of the scene.
    focused: wgpu::TextureView,
    down_focused: wgpu::BindGroup,
    finishes_focused: [wgpu::BindGroup; 2],
    bloom: Vec<wgpu::TextureView>,
    /// Bind groups reading the scene, then each level but the last, for the passes down the chain.
    downs: Vec<wgpu::BindGroup>,
    /// Bind groups reading each level from the second, for the passes up the chain.
    ups: Vec<wgpu::BindGroup>,
    /// Reading the exposure kept in each of the two textures.
    meters: [wgpu::BindGroup; 2],
    finishes: [wgpu::BindGroup; 2],
}

/// The targets and passes of `Post`.
pub(crate) struct PostProcess {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    meter: wgpu::RenderPipeline,
    finish: wgpu::RenderPipeline,
    /// The depth of field's pass, for a depth of one sample per pixel and of several.
    focus: [(wgpu::BindGroupLayout, wgpu::RenderPipeline); 2],
    /// The exposure in stops is kept in one of two 1 x 1 textures and written into the other, in turns.
    exposure: [wgpu::TextureView; 2],
    exposure_textures: [wgpu::Texture; 2],
    turn: usize,
    /// Until the first frame the exposure is not known: that frame takes the one it measures.
    measured: bool,
    targets: Targets,
}

fn texture(device: &wgpu::Device, label: &str, size: (u32, u32), levels: u32, format: wgpu::TextureFormat, extra: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | extra,
        view_formats: &[],
    })
}

impl PostProcess {
    /// For a frame of `width` x `height` drawn into a target of format `target`.
    pub(crate) fn new(device: &wgpu::Device, target: wgpu::TextureFormat, width: u32, height: u32) -> PostProcess {
        let texture_entry = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                texture_entry(1, true),
                texture_entry(2, true),
                texture_entry(3, false),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("post"), source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()) });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("post"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = |entry: &str, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("fullscreen"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let focus = [false, true].map(|multisampled| {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("focus"),
                entries: &[
                    wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                    texture_entry(1, true),
                    wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled }, count: None },
                    wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                ],
            });
            let source = include_str!("focus.wgsl").replace("DEPTH_TEXTURE", if multisampled { "texture_depth_multisampled_2d" } else { "texture_depth_2d" });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("focus"), source: wgpu::ShaderSource::Wgsl(source.into()) });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("focus"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("focus"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("fullscreen"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("focus"),
                    targets: &[Some(wgpu::ColorTargetState { format: SCENE_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            (layout, pipeline)
        });
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("post"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let params = device.create_buffer(&wgpu::BufferDescriptor { label: Some("post"), size: size_of::<Params>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let exposure_textures = [0, 1].map(|_| texture(device, "exposure", (1, 1), 1, EXPOSURE_FORMAT, wgpu::TextureUsages::COPY_SRC));
        let exposure = exposure_textures.each_ref().map(|t| t.create_view(&Default::default()));
        let targets = Self::targets(device, &layout, &params, &sampler, &exposure, width, height);
        PostProcess {
            down: pipeline("down", SCENE_FORMAT, None),
            up: pipeline("up", SCENE_FORMAT, Some(wgpu::BlendState { color: add, alpha: add })),
            meter: pipeline("meter", EXPOSURE_FORMAT, None),
            finish: pipeline("finish", target, None),
            focus,
            layout,
            sampler,
            params,
            exposure,
            exposure_textures,
            turn: 0,
            measured: false,
            targets,
        }
    }

    fn targets(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, params: &wgpu::Buffer, sampler: &wgpu::Sampler, exposure: &[wgpu::TextureView; 2], width: u32, height: u32) -> Targets {
        let scene = texture(device, "scene", (width, height), 1, SCENE_FORMAT, wgpu::TextureUsages::empty()).create_view(&Default::default());
        let focused = texture(device, "scene in focus", (width, height), 1, SCENE_FORMAT, wgpu::TextureUsages::empty()).create_view(&Default::default());
        let half = ((width / 2).max(1), (height / 2).max(1));
        let bloom = texture(device, "bloom", half, BLOOM_LEVELS.min(half.0.max(half.1).ilog2() + 1), SCENE_FORMAT, wgpu::TextureUsages::empty());
        let bloom: Vec<_> = (0..bloom.mip_level_count()).map(|i| bloom.create_view(&wgpu::TextureViewDescriptor { base_mip_level: i, mip_level_count: Some(1), ..Default::default() })).collect();
        let group = |source: &wgpu::TextureView, glow: &wgpu::TextureView, exposure: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(source) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(glow) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(exposure) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };
        // A pass may not read what it draws into: the slots a pass does not use hold the scene and
        // an exposure texture, which no pass of the chain draws into.
        let last = bloom.len() - 1;
        Targets {
            downs: std::iter::once(&scene).chain(&bloom[..last]).map(|source| group(source, &scene, &exposure[0])).collect(),
            ups: bloom[1..].iter().map(|source| group(source, &scene, &exposure[0])).collect(),
            meters: [0, 1].map(|i| group(&bloom[last], &scene, &exposure[i])),
            finishes: [0, 1].map(|i| group(&scene, &bloom[0], &exposure[i])),
            down_focused: group(&focused, &scene, &exposure[0]),
            finishes_focused: [0, 1].map(|i| group(&focused, &bloom[0], &exposure[i])),
            focused,
            scene,
            bloom,
        }
    }

    /// The scene, at one sample per pixel: the multisampled scene is resolved into it, or drawn
    /// straight into it with one sample.
    pub(crate) fn scene(&self) -> &wgpu::TextureView {
        &self.targets.scene
    }

    /// Makes the targets again for a frame of this size. The exposure stays.
    pub(crate) fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.targets = Self::targets(device, &self.layout, &self.params, &self.sampler, &self.exposure, width, height);
    }

    /// Records the passes that make the picture from the scene into `target`, `dt` seconds after the
    /// last frame. `srgb_target`: the target encodes the linear colour it is given.
    /// `depth`: the scene's depth and its samples per pixel, kept when `post.focus` is set (`wants_depth`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, srgb_target: bool, post: &Post, dt: f64, depth: (&wgpu::TextureView, u32)) {
        let step = |time: f64| if self.measured { (1.0 - (-dt / time).exp()) as f32 } else { 1.0 };
        let t = &self.targets;
        let levels = t.bloom.len();
        let params = Params {
            look: [post.exposure, post.adaptation.clamp(0.0, 1.0), post.bloom.clamp(0.0, 1.0), step(OPENING_TIME)],
            more: [step(CLOSING_TIME), if srgb_target { 1.0 } else { 0.0 }, post.glare.clamp(0.0, 1.0), levels as f32],
            grade: [post.contrast.clamp(0.0, 1.0), post.saturation.max(0.0), post.vignette.clamp(0.0, 1.0), post.warmth.clamp(-1.0, 1.0)],
            lens: [post.blur.clamp(0.0, 0.5), post.blur_centre[0], post.blur_centre[1], post.blur_above.clamp(0.0, 1.0)],
            focus: [post.focus.max(0.0), post.focus_span.max(0.0), (post.focus_blur.clamp(0.0, 0.05) * t.scene.texture().height() as f32).max(1.0), post.near.max(1e-4)],
            flare: [post.flare_sun[0], post.flare_sun[1], if post.bloom > 0.0 { post.flare.max(0.0) } else { 0.0 }, post.spot.clamp(0.0, 1.0)],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));

        let mut pass = |label, view: &wgpu::TextureView, load, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        // The lens's depth of field first: the rest is made of the scene seen through it.
        let focused = post.wants_depth();
        if focused {
            let (layout, pipeline) = &self.focus[if depth.1 > 1 { 1 } else { 0 }];
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("focus"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&t.scene) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth.0) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            pass("focus", &t.focused, clear, pipeline, &group);
        }
        // The way down is also what the exposure is measured on; with neither, the exposure does not
        // read it (`meter` multiplies what it measures by the adaptation, 0).
        if post.bloom > 0.0 || post.adaptation > 0.0 || post.spot > 0.0 {
            for (k, (view, group)) in t.bloom.iter().zip(&t.downs).enumerate() {
                pass("bloom down", view, clear, &self.down, if k == 0 && focused { &t.down_focused } else { group });
            }
        }
        let (kept, written) = (self.turn, 1 - self.turn);
        pass("exposure", &self.exposure[written], clear, &self.meter, &t.meters[kept]);
        if post.bloom > 0.0 {
            for i in (0..levels - 1).rev() {
                pass("bloom up", &t.bloom[i], wgpu::LoadOp::Load, &self.up, &t.ups[i]);
            }
        }
        pass("tone map", target, clear, &self.finish, if focused { &t.finishes_focused[written] } else { &t.finishes[written] });
        self.turn = written;
        self.measured = true;
    }
}

impl PostProcess {
    /// The exposure of the last frame, in stops; waits for the GPU.
    pub(crate) fn exposure(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> f32 {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("exposure"), size: 4, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.exposure_textures[self.turn].as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: None, rows_per_image: None } },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map failed"));
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("buffer not mapped");
        f32::from_ne_bytes([data[0], data[1], data[2], data[3]])
    }
}
