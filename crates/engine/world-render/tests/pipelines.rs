//! Compiles the shaders and builds every pipeline on a real device, then draws what needs only
//! uniforms and the sky's tables (sky, sea, clouds) to a PNG. Look at the image: a blue sky with the
//! glow of the sun, the sea below the horizon and clouds. It is written to `$WORLD_TEST_PNG`, or to the test's temporary
//! directory, and the path is printed.

use world_render::Quality;

use engine::texture::{Kind, TextureSource};
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Camera, Game, Gpu, Input, MeshBuilder};
use glam::Vec3;
use world_render::pipelines::{self, ChunkUniforms, FrameUniforms, ObjectUniforms, Pipelines};
use world_render::sky::{self, Air, SkyLights, SkyTables};

const SLICES: usize = 24;

struct Scene {
    pipelines: Pipelines,
    frame: wgpu::BindGroup,
    slices: wgpu::BindGroup,
    /// The sea's group 1: no mirror.
    sea: wgpu::BindGroup,
    /// Offset of each cloud slice in the buffer, in drawing order.
    offsets: Vec<u32>,
}

impl Game for Scene {
    fn update(&mut self, _input: &Input, _dt: f64, _gpu: &Gpu) {}

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.frame, &[]);
        pass.set_pipeline(&self.pipelines.sky);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&self.pipelines.sea);
        pass.set_bind_group(1, &self.sea, &[]);
        pass.draw(0..6, 0..1);
        pass.set_pipeline(&self.pipelines.clouds);
        for offset in &self.offsets {
            pass.set_bind_group(1, &self.slices, &[*offset]);
            pass.draw(0..6, 0..1);
        }
    }
}

fn with(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

fn scene(gpu: &Gpu) -> Scene {
    let mut pipelines = Pipelines::new(gpu, &Quality::HIGH);
    // Made again, as when the quality changes: the bind groups below are made with the layouts that stay.
    // Every quality has its own shader source, which must compile: the presets and one without textures.
    for quality in [Quality::MEDIUM, Quality::LOW, Quality { texture_size: 0, ..Quality::LOW }, Quality::HIGH] {
        pipelines.rebuild(gpu, &quality);
    }

    // A camera 600 m above the sea looking north at the horizon, mid-morning sun ahead and to the right.
    let camera = Camera::default();
    let (tan_x, tan_y) = camera.tan_half_fov(gpu.aspect());
    let altitude = 600.0;
    let (cloud_base, cloud_thickness) = (2300.0 - altitude, 650.0);
    let mut frame: FrameUniforms = bytemuck::Zeroable::zeroed();
    frame.view_proj = camera.view_proj(gpu.aspect()).to_cols_array_2d();
    frame.cam_right = with(camera.right, tan_x);
    frame.cam_up = with(camera.up, tan_y);
    frame.cam_fwd = with(camera.forward, camera.shift_y);
    // The sky of a clear morning, its tables made as the renderer makes them.
    let air = Air::new(&sim_core::Weather::default(), 45_000.0, 0.0);
    let lights = SkyLights::at(9.0);
    let reach = sky::aerial_reach(45_000.0);
    let mut tables = SkyTables::new(gpu);
    tables.update(gpu, &air, &lights, altitude as f64, 0.0, reach);
    frame.sun_dir = with(lights.sun, 100.0);
    frame.sun_color = with(lights.sun_top * air.transmittance(altitude as f64, lights.sun.y as f64).as_vec3(), 1.0);
    frame.cam_wrap = [1000.0, altitude, 2000.0, 0.0];
    frame.sky_sun = with(lights.sun, reach as f32);
    frame.sky_moon = with(lights.moon, lights.scale);
    frame.moon_light = with(lights.moon_top, 0.0);
    frame.sky_more = [lights.azimuth().x, lights.azimuth().z, 0.0, 0.0];
    frame.body_pos = [0.0, 0.0, 0.0, 0.75];
    frame.body_fwd = [0.0, 0.0, -1.0, 0.0];
    frame.body_right = [1.0, 0.0, 0.0, 0.0];
    frame.body_down = [0.0, -1.0, 0.0, 0.0];
    frame.misc = [0.0, cloud_base, 0.0, 0.6];
    frame.extra = [cloud_thickness, 0.0, 0.0, 0.0];
    frame.materials = [[0.2, 0.2, 0.2, 0.8]; 20];

    let frame_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("frame"), contents: bytemuck::bytes_of(&frame), usage: wgpu::BufferUsages::UNIFORM });
    let height_map = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("height map"),
            size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(&pipelines::half_floats(&[0.0f32; 16])),
    );
    // The material textures: the shaders take arrays of any number of layers, here one grey texel each.
    let texel = |kind, rgba: [u8; 4]| gpu.texture_array("materials", &[TextureSource::Pixels { width: 1, height: 1, rgba: rgba.to_vec() }], kind).expect("a 1 x 1 texture");
    let (colour, detail) = (texel(Kind::Color, [128, 128, 128, 255]), texel(Kind::NormalPacked, [128, 128, 204, 255]));
    let (height_view, height_sampler, texture_sampler) = (height_map.create_view(&Default::default()), pipelines::height_sampler(gpu), gpu.texture_sampler());
    let mut entries = vec![
        wgpu::BindGroupEntry { binding: 0, resource: frame_buffer.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&height_view) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&height_sampler) },
        colour.entry(3),
        detail.entry(4),
        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&texture_sampler) },
        // No roads: the height map stands in for the road map, which the frame says is not ready.
        wgpu::BindGroupEntry { binding: 12, resource: wgpu::BindingResource::TextureView(&height_view) },
    ];
    // No vehicles' shadows (the frame has no tiles): an empty map and its comparison sampler.
    let casters = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vehicles' shadows"),
        size: wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let casters_view = casters.create_view(&Default::default());
    let compare = gpu.device.create_sampler(&wgpu::SamplerDescriptor { compare: Some(wgpu::CompareFunction::LessEqual), ..Default::default() });
    entries.push(wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::TextureView(&casters_view) });
    entries.push(wgpu::BindGroupEntry { binding: 14, resource: wgpu::BindingResource::Sampler(&compare) });
    entries.extend(tables.entries());
    let frame_bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("frame"), layout: &pipelines.frame_layout, entries: &entries });
    // Nothing mirrored in the sea (the frame says so): the height map stands in for the picture.
    let sea = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sea"), layout: &pipelines.sea_layout, entries: &[wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&height_view) }] });

    // The cloud layer is a stack of slices, drawn from the farthest to the nearest in height.
    let mut heights: Vec<(f32, f32)> = (0..SLICES)
        .map(|i| {
            let level = (i as f32 + 0.5) / SLICES as f32;
            (cloud_base + level * cloud_thickness, level)
        })
        .collect();
    heights.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
    let step = gpu.device.limits().min_uniform_buffer_offset_alignment as usize;
    let mut bytes = vec![0u8; step * SLICES];
    let mut offsets = Vec::new();
    for (i, (y, level)) in heights.into_iter().enumerate() {
        let slice = ChunkUniforms { offset: [0.0, y, 0.0, level], wrap: [0.0; 4] };
        bytes[i * step..i * step + 32].copy_from_slice(bytemuck::bytes_of(&slice));
        offsets.push((i * step) as u32);
    }
    let slice_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("cloud slices"), contents: &bytes, usage: wgpu::BufferUsages::UNIFORM });
    let slices = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cloud slices"),
        layout: &pipelines.chunk_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &slice_buffer, offset: 0, size: wgpu::BufferSize::new(32) }) }],
    });

    Scene { pipelines, frame: frame_bind, slices, sea, offsets }
}

#[test]
fn layouts_match_the_buffers() {
    assert_eq!(pipelines::terrain_vertex_layout().array_stride, 12 * 4);
    assert_eq!(pipelines::instance_layout().array_stride, 8 * 4);
    assert_eq!(pipelines::instance_layout().step_mode, wgpu::VertexStepMode::Instance);
    // Instance attributes follow those of the mesh.
    let last = MeshBuilder::layout().attributes.iter().map(|a| a.shader_location).max().unwrap();
    assert_eq!(pipelines::instance_layout().attributes[0].shader_location, last + 1);
    let object = ObjectUniforms::new(glam::Mat4::IDENTITY.to_cols_array_2d(), [1.0; 4]);
    assert_eq!(bytemuck::bytes_of(&object).len(), 160);
}

#[test]
fn pipelines_build_and_draw() {
    let path = std::env::var("WORLD_TEST_PNG").unwrap_or_else(|_| format!("{}/pipelines.png", env!("CARGO_TARGET_TMPDIR")));
    let scene = engine::render_to_png(&path, 320, 200, 1.0, scene, |_, frames| frames >= 1);
    let p = &scene.pipelines;
    // Terrain, objects, trees, their shadows and houses need geometry: building them without a validation error is the check.
    let _ = [&p.terrain, &p.objects, &p.objects_blended, &p.flora, &p.impostors, &p.grass, &p.tree_shadows, &p.houses, &p.house_shadows, &p.lights];
    let _ = &p.object_layout;
    println!("image written to {path}");
}

/// The source every quality compiles, checked on the CPU alone (no device): parsed and validated
/// as wgpu does before it hands it to the GPU.
#[test]
fn every_quality_has_valid_shaders() {
    use engine::wgpu::naga;
    for quality in [Quality::HIGH, Quality::MEDIUM, Quality::LOW, Quality { texture_size: 0, ..Quality::LOW }] {
        let source = pipelines::shader_source(&quality);
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{quality:?}: {}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{quality:?}: {}", e.emit_to_string(&source)));
    }
}
