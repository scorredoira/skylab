//! The cloud lab: the sky, a plain ground and the volumetric clouds alone, from fixed cameras,
//! each picture the average of many frames. For working on the clouds' shapes and light without
//! the rest of the world.
//!
//! ```text
//! cargo run --release -p world-render --example cloud_lab -- <dir> [prefix] [camera ...]
//! ```
//!
//! writes `<dir>/<prefix><camera>.png` (1600×1000) for every camera, or for those named.

use engine::pipeline::{Depth, PipelineDesc};
use engine::{wgpu, Game, Gpu, Input};
use glam::Vec3;
use sim_core::Weather;
use std::time::Instant;
use world_render::clouds::{CloudUniforms, Layers};
use world_render::CloudMarch;
use world_render::sky::{aerial_reach, Air, SkyLights, SkyTables};

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1000;
/// Frames averaged into a picture, and frames shown after them while the exposure settles.
const GATHERED: usize = 96;
const SETTLED: usize = 70;

/// A picture the lab takes.
struct Camera {
    name: &'static str,
    /// Altitude of the eye, m.
    eye: f32,
    /// Where it looks: degrees clockwise from the sun's azimuth, and degrees above the horizon.
    off_sun: f32,
    pitch: f32,
    hour: f64,
    /// The share of the sky the clouds take, 0..1, and the altitude of their bases, m.
    cover: f64,
    base: f32,
    /// Storm clouds: (distance m, degrees clockwise from where the camera looks, radius of the base m, altitude of the top m).
    towers: &'static [(f32, f32, f32, f32)],
    /// The eye's place over the weather (x, z), m: which clouds are near.
    place: (f32, f32),
}

const CAMERAS: &[Camera] = &[
    Camera { name: "noon", eye: 150.0, off_sun: 70.0, pitch: 20.0, hour: 12.5, cover: 0.55, base: 1100.0, towers: &[], place: (0.0, 0.0) },
    Camera { name: "golden", eye: 150.0, off_sun: 70.0, pitch: 20.0, hour: 18.2, cover: 0.55, base: 1100.0, towers: &[], place: (0.0, 0.0) },
    Camera { name: "close", eye: 1500.0, off_sun: 150.0, pitch: 4.0, hour: 15.5, cover: 0.6, base: 1100.0, towers: &[], place: (0.0, 0.0) },
    // Over the clouds at cruise, the sun low.
    Camera { name: "above", eye: 3500.0, off_sun: 100.0, pitch: -8.0, hour: 18.45, cover: 0.7, base: 1100.0, towers: &[], place: (0.0, 0.0) },
    Camera { name: "sunward", eye: 400.0, off_sun: 8.0, pitch: 14.0, hour: 17.6, cover: 0.6, base: 1100.0, towers: &[], place: (0.0, 0.0) },
    Camera { name: "overcast", eye: 150.0, off_sun: 70.0, pitch: 15.0, hour: 12.5, cover: 1.0, base: 900.0, towers: &[], place: (0.0, 0.0) },
    // The rain under a storm from the ground: lit from the side, with the sun behind it, and at the end of the day.
    Camera { name: "rain", eye: 200.0, off_sun: 75.0, pitch: 5.0, hour: 16.0, cover: 0.7, base: 1200.0, towers: &[(13_000.0, 0.0, 4500.0, 10_000.0), (40_000.0, -32.0, 5500.0, 12_000.0)], place: (0.0, 0.0) },
    Camera { name: "rain-backlit", eye: 200.0, off_sun: 12.0, pitch: 5.0, hour: 17.5, cover: 0.7, base: 1200.0, towers: &[(15_000.0, 0.0, 4500.0, 10_000.0)], place: (0.0, 0.0) },
    Camera { name: "rain-above", eye: 2800.0, off_sun: 110.0, pitch: -6.0, hour: 15.0, cover: 0.7, base: 1200.0, towers: &[(14_000.0, 0.0, 4500.0, 10_000.0)], place: (0.0, 0.0) },
    Camera { name: "storm", eye: 600.0, off_sun: 60.0, pitch: 14.0, hour: 16.6, cover: 0.7, base: 1200.0, towers: &[(16_000.0, 0.0, 4500.0, 10_000.0), (38_000.0, 35.0, 5500.0, 12_000.0)], place: (0.0, 0.0) },
];

/// `struct Lab` in cloud_lab.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LabUniforms {
    right: [f32; 4],
    up: [f32; 4],
    fwd: [f32; 4],
    sky: [f32; 4],
    light: [f32; 4],
    sun: [f32; 4],
}

struct Lab {
    lab_uniforms: wgpu::Buffer,
    lab: LabUniforms,
    gather: wgpu::RenderPipeline,
    gather_group: wgpu::BindGroup,
    gathered: wgpu::TextureView,
    show: wgpu::RenderPipeline,
    show_group: wgpu::BindGroup,
    frames: usize,
}

const SHOW: &str = "
@group(0) @binding(0) var gathered: texture_2d<f32>;
@vertex
fn show_vertex(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(select(-1.0, 3.0, vid == 1u), select(-1.0, 3.0, vid == 2u), 0.0, 1.0);
}
@fragment
fn show_fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(textureLoad(gathered, vec2<i32>(at.xy), 0).rgb, 1.0);
}
";

impl Lab {
    fn new(gpu: &Gpu, camera: &Camera) -> Lab {
        let d = &gpu.device;
        let lights = SkyLights::at(camera.hour);
        let weather = Weather::default();
        let air = Air::new(&weather, weather.visibility, 0.0);
        let reach = aerial_reach(weather.visibility);
        let mut tables = SkyTables::new(gpu);
        tables.update(gpu, &air, &lights, camera.eye as f64, 0.0, reach);

        // The camera: its heading from the sun's azimuth, north being -z.
        let azimuth = lights.azimuth();
        let heading = azimuth.x.atan2(-azimuth.z) + camera.off_sun.to_radians();
        let level = Vec3::new(heading.sin(), 0.0, -heading.cos());
        let pitch = camera.pitch.to_radians();
        let fwd = level * pitch.cos() + Vec3::Y * pitch.sin();
        let right = fwd.cross(Vec3::Y).normalize();
        let up = right.cross(fwd);
        let tan_y = (60f32.to_radians() * 0.5).tan();
        let tan_x = tan_y * WIDTH as f32 / HEIGHT as f32;
        let (sun, _) = lights.direct();
        // LAB_EYE: another altitude for the eye, to tell what depends on it.
        let eye = std::env::var("LAB_EYE").ok().and_then(|v| v.parse().ok()).unwrap_or(camera.eye);
        let storm_top = camera.towers.iter().map(|t| t.3).reduce(f32::max);
        let mut clouds = CloudUniforms {
            sun: [sun.x, sun.y, sun.z, 0.0],
            wind: [0.0, 0.0, world_render::clouds::coverage(camera.cover), 0.0],
            origin: [camera.place.0, camera.place.1, eye, camera.towers.len() as f32],
            more: [1.0, 0.0, 1.0, world_render::clouds::sun_gain(sun.y)],
            layers: Layers::of(camera.base, 650.0, storm_top).pack(),
            light: [1.0, 1.0, 1.0, world_render::clouds::own_shade(sun.y)],
            // The wind across the picture, from the left.
            rain: {
                let rain = world_render::clouds::rain(0.0, 7.0, camera.cover, camera.towers.len());
                [right.x * rain[1], right.z * rain[1], rain[2], rain[3]]
            },
            ..bytemuck::Zeroable::zeroed()
        };
        for (tower, (distance, bearing, radius, top)) in clouds.towers.iter_mut().zip(camera.towers) {
            let a = heading + bearing.to_radians();
            *tower = [camera.place.0 + a.sin() * distance, camera.place.1 - a.cos() * distance, *radius, *top];
        }
        let lab = LabUniforms {
            right: [right.x, right.y, right.z, tan_x],
            up: [up.x, up.y, up.z, tan_y],
            fwd: [fwd.x, fwd.y, fwd.z, 0.0],
            sky: [azimuth.x, azimuth.z, 0.0, reach as f32],
            light: [lights.scale, 0.0, WIDTH as f32, HEIGHT as f32],
            sun: [lights.sun.x, lights.sun.y, lights.sun.z, 0.0],
        };
        let uniform = |label, bytes: &[u8]| {
            let buffer = d.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: bytes.len() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            gpu.queue.write_buffer(&buffer, 0, bytes);
            buffer
        };
        let cloud_uniforms = uniform("clouds", bytemuck::bytes_of(&clouds));
        let lab_uniforms = uniform("lab", bytemuck::bytes_of(&lab));

        let noise = world_render::cloud_noise::textures(gpu);
        let repeat = world_render::cloud_noise::sampler(gpu);

        let format = wgpu::TextureFormat::Rgba16Float;
        let gathered = d
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("gathered"),
                size: wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());

        // The march as fine as their finest: steps of 10 m.
        let source = world_render::cloud_shader_source(&CloudMarch { steps: 2000, ..CloudMarch::ULTRA }) + include_str!("cloud_lab.wgsl");
        let module = gpu.shader_with_ending_loops("cloud lab", &source);
        // Each frame is blended into the average by its share of it.
        let average = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Constant, dst_factor: wgpu::BlendFactor::OneMinusConstant, operation: wgpu::BlendOperation::Add };
        let gather = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cloud lab"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("lab_vertex"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("lab_fragment"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState { color: average, alpha: average }), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let view = |binding, view| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) };
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: cloud_uniforms.as_entire_binding() },
            view(1, &noise.weather),
            view(2, &noise.shape),
            view(3, &noise.detail),
            view(4, &noise.turbulence),
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&repeat) },
            wgpu::BindGroupEntry { binding: 12, resource: lab_uniforms.as_entire_binding() },
        ];
        entries.extend(tables.entries());
        let gather_group = d.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("cloud lab"), layout: &gather.get_bind_group_layout(0), entries: &entries });

        let show_module = gpu.shader("show", SHOW);
        let show_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("show"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        });
        let show = gpu.pipeline_with_samples(&PipelineDesc { label: "show", shader: &show_module, vertex: "show_vertex", fragment: "show_fragment", buffers: &[], bind_groups: &[&show_layout], depth: Depth::Off, blend: false }, None, 1);
        let show_group = d.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("show"), layout: &show_layout, entries: &[view(0, &gathered)] });
        Lab { lab_uniforms, lab, gather, gather_group, gathered, show, show_group, frames: 0 }
    }
}

impl Game for Lab {
    fn samples(&self) -> u32 {
        1
    }

    fn update(&mut self, _input: &Input, _dt: f64, gpu: &Gpu) {
        if self.frames >= GATHERED {
            return;
        }
        self.lab.light[1] = self.frames as f32;
        gpu.queue.write_buffer(&self.lab_uniforms, 0, bytemuck::bytes_of(&self.lab));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cloud lab") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cloud lab"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.gathered, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let share = 1.0 / (self.frames as f64 + 1.0);
            pass.set_blend_constant(wgpu::Color { r: share, g: share, b: share, a: share });
            pass.set_pipeline(&self.gather);
            pass.set_bind_group(0, &self.gather_group, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        self.frames += 1;
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.show);
        pass.set_bind_group(0, &self.show_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn post(&self) -> engine::Post {
        // The world's (`WorldRenderer::post`, quality `high`).
        engine::Post { exposure: 0.0, adaptation: 0.25, glare: 1.0, bloom: 0.03, contrast: 0.4, saturation: 1.1, ..Default::default() }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: cloud_lab <dir> [prefix] [camera ...]");
    // `cloud_lab pair <left.png> <right.png> <out.png>`: the two pictures side by side, the right one as tall as the left.
    if dir == "pair" {
        let paths: Vec<String> = args.collect();
        let left = image::open(&paths[0]).expect("no left picture").to_rgb8();
        let right = image::open(&paths[1]).expect("no right picture").to_rgb8();
        let width = (right.width() as f32 * left.height() as f32 / right.height() as f32) as u32;
        let right = image::imageops::resize(&right, width, left.height(), image::imageops::FilterType::Triangle);
        let mut both = image::RgbImage::new(left.width() + width, left.height());
        image::imageops::replace(&mut both, &left, 0, 0);
        image::imageops::replace(&mut both, &right, left.width() as i64, 0);
        both.save(&paths[2]).expect("could not write the picture");
        return;
    }
    // `cloud_lab sheet <out.png> <columns> <width> <in.png>...`: the pictures in a grid, each `width` wide.
    if dir == "sheet" {
        let out = args.next().expect("no picture to write");
        let columns: u32 = args.next().and_then(|v| v.parse().ok()).expect("no columns");
        let width: u32 = args.next().and_then(|v| v.parse().ok()).expect("no width");
        let cells: Vec<image::RgbImage> = args
            .map(|path| {
                let picture = image::open(&path).unwrap_or_else(|_| panic!("no picture {path}")).to_rgb8();
                image::imageops::resize(&picture, width, picture.height() * width / picture.width(), image::imageops::FilterType::Triangle)
            })
            .collect();
        let height = cells.iter().map(|c| c.height()).max().unwrap_or(1);
        let mut sheet = image::RgbImage::new(columns * width, (cells.len() as u32).div_ceil(columns) * height);
        for (i, cell) in cells.iter().enumerate() {
            image::imageops::replace(&mut sheet, cell, (i as u32 % columns * width) as i64, (i as u32 / columns * height) as i64);
        }
        sheet.save(&out).expect("could not write the picture");
        return;
    }
    let prefix = args.next().unwrap_or_default();
    let named: Vec<String> = args.collect();
    std::fs::create_dir_all(&dir).expect("could not make the directory");
    for camera in CAMERAS.iter().filter(|c| named.is_empty() || named.iter().any(|n| n == c.name)) {
        let started = Instant::now();
        let path = format!("{dir}/{prefix}{}.png", camera.name);
        engine::render_to_png(&path, WIDTH, HEIGHT, 1.0, |gpu| Lab::new(gpu, camera), |_, frames| frames >= GATHERED + SETTLED);
        eprintln!("{path}: {:.1} s, sun {:.1}° up", started.elapsed().as_secs_f64(), world_render::sun_elevation(camera.hour).to_degrees());
    }
}
