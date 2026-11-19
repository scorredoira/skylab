//! A cockpit on the GPU, whichever kind's (`CabinSpec`): its mesh with the occlusion of each
//! vertex, the glass of its windows, the canvas its instruments are painted into, what is printed
//! in it, its photographs (plastic, leather, fabric, carpet, rubber, painted metal), the pictures
//! of its textured materials, the light
//! through its windows, the sun's shadow map and where each part that moves with the controls is.
//! `render` draws it with the `cabin` and `cabin_glass` pipelines, this group 2 bound. What takes
//! time and no GPU (the mesh, the light through the windows, the occlusion, the photographs read)
//! is `bake`d on another thread when the game loads (`start_baking`), so entering a cockpit the
//! first time does not stop the game.

use super::{CabinPart, CabinSpec, CockpitControls, Lever, PanelControl};
use engine::texture::{prepare_layers, Kind, PreparedLayers, TextureSource};
use engine::{wgpu, Canvas, Gpu, Mesh, MeshBuilder, SkyVolume};
use glam::{Mat4, Vec3};

use std::ops::Range;
use std::sync::mpsc::{channel, Receiver};
use world_render::materials;

/// The cockpit's photographs, in the order of `cockpit::Material` from `Plastic` on.
const NAMES: [&str; 6] = ["plastic", "leather", "fabric", "carpet", "rubber", "metal_painted"];
/// Canvas pixels per point: the instruments are painted at three times their size in points.
const CANVAS_SCALE: f32 = 3.0;
/// Print pixels per point: a legend 1 mm high is some 15 pixels.
pub(crate) const PRINT_SCALE: f32 = 4.0;
/// Metres between the samples of the light through the windows (`SkyVolume`).
const SKY_STEP: f32 = 0.03;
/// The sun's shadow map: texels a side (some 1.5 mm each over a cabin), and the most draws it takes.
const SHADOW_SIZE: u32 = 2048;
const SHADOW_DRAWS: usize = 32;
/// Bytes between two draws' matrices in the shadow pass's uniform buffer (the alignment of dynamic offsets).
const SHADOW_STRIDE: u64 = 256;
/// How far beyond the cockpit, towards the sun, what casts a shadow into it may be: a wing's tip.
const SHADOW_REACH: f32 = 7.0;

/// What the cockpit's shader reads besides the frame's: `struct Cabin` in `cabin.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CabinUniforms {
    /// Body axes to the sun's shadow map (x, y in -1..1, depth 0..1).
    light: [[f32; 4]; 4],
    /// x: the panel's lights (DIMMING PANEL), 0 to 1; y: the standby instruments' (STBY IND);
    /// z: 1 when the shadow map holds this frame's sun; w: a texel of the shadow map, in its x and y.
    lights: [f32; 4],
    /// The light through the windows' box: its least corner and the samples' step.
    sky: [f32; 4],
    /// The print's panel and floor parts (`Print`: min, size); its canvas in points, where the
    /// floor's part starts and how wide it is; x: the panel's print only ahead of it.
    panel_print: [f32; 4],
    floor_print: [f32; 4],
    print: [f32; 4],
    place: [f32; 4],
}

/// The sun's shadow map of the cabin and the aircraft round it: its depth texture, the pipeline that
/// draws it and the matrices of the draws.
struct Shadow {
    view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    casters: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

const SHADOW_WGSL: &str = "
struct Caster { model: mat4x4<f32> };
@group(0) @binding(0) var<uniform> caster: Caster;
@vertex fn cabin_shadow_vertex(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return caster.model * vec4<f32>(position, 1.0);
}
";

impl Shadow {
    fn new(gpu: &Gpu) -> Shadow {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cabin: sun's shadow"),
            size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let casters = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cabin: casters"),
            size: SHADOW_STRIDE * SHADOW_DRAWS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cabin: casters"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(64) },
                count: None,
            }],
        });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cabin: casters"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &casters, offset: 0, size: wgpu::BufferSize::new(64) }) }],
        });
        let shader = gpu.shader("cabin shadow", SHADOW_WGSL);
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("cabin shadow"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cabin shadow"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("cabin_shadow_vertex"), buffers: &[Some(MeshBuilder::layout())], compilation_options: Default::default() },
            fragment: None,
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 1.5, clamp: 0.0 },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Shadow { view, pipeline, casters, bind }
    }
}

/// The sun's view of the cockpit, body axes to the shadow map: looking along `-sun` at the box the
/// light through the windows is baked in, wide enough for all of it, deep enough for the wing.
fn light_matrix(sun: Vec3, (min, max): (Vec3, Vec3)) -> Mat4 {
    let centre = (min + max) / 2.0;
    let up = if sun.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let view = Mat4::look_at_rh(centre + sun * SHADOW_REACH, centre, up);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for k in 0..8 {
        let corner = Vec3::new(if k & 1 == 0 { min.x } else { max.x }, if k & 2 == 0 { min.y } else { max.y }, if k & 4 == 0 { min.z } else { max.z });
        let p = view.transform_point3(corner);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    // In view space the eye looks down -z: near is the least distance, far the most.
    Mat4::orthographic_rh(lo.x, hi.x, lo.y, hi.y, 0.05, -lo.z + 0.1) * view
}

pub struct CockpitRenderer {
    /// The cockpit's mesh, the ranges of it that move, and each vertex's openness to its
    /// surroundings (`engine::ambient_occlusion`), a vertex buffer of its own.
    pub mesh: Mesh,
    parts: Vec<CabinPart>,
    pub occlusion: wgpu::Buffer,
    /// The windows' glass, drawn over the world.
    pub glass: Mesh,
    /// What the kind's instruments are painted into.
    pub canvas: Canvas,
    /// The knobs a mouse can take (`CabinSpec::levers`).
    pub levers: Vec<(Lever, [Vec3; 2])>,
    /// The keys and knobs it can press and turn (`CabinSpec::controls`).
    pub controls: Vec<PanelControl>,
    shadow: Shadow,
    uniforms: wgpu::Buffer,
    /// The uniforms that stay: where the print and the light's box are.
    place: CabinUniforms,
    sky_box: (Vec3, Vec3),
    /// Group 2 of the cockpit's pipelines (`bind_group`): it holds the photographs, the light through
    /// the windows and the print.
    bind: wgpu::BindGroup,
}

/// A mesh as triangles.
fn triangles(mesh: &MeshBuilder) -> Vec<[Vec3; 3]> {
    mesh.data.as_chunks::<30>().0.iter().map(|t| [0, 10, 20].map(|k| Vec3::new(t[k], t[k + 1], t[k + 2]))).collect()
}

/// What makes a cockpit that takes time and needs no GPU (`bake`): the kind's spec, the light
/// through its windows, how open each vertex is, its photographs read with their mip levels.
pub struct Baked {
    spec: CabinSpec,
    sky: SkyVolume,
    occlusion: Vec<f32>,
    /// Colour, and normal, roughness and occlusion; `None` without the textures' folder.
    photographs: Option<(PreparedLayers, PreparedLayers)>,
    /// The pictures of the cabin's textured materials: colour, and normal maps; `None` without any.
    pictures: Option<(PreparedLayers, PreparedLayers)>,
}

/// Reads the kind's cockpit (`kinds::cockpit_spec`: checked when the game started, so one that
/// does not read now is broken), bakes the light through its windows and the occlusion of every
/// vertex, and reads its photographs: some hundreds of milliseconds on every core.
pub fn bake(cabin: &str) -> Baked {
    let mut spec = crate::kinds::cockpit_spec_in(cabin).unwrap_or_else(|e| panic!("{e}"));
    let pictures = pictures(std::mem::take(&mut spec.pictures));
    let triangles = triangles(&spec.model.mesh);
    let (min, max) = spec.sky_box;
    let sky = SkyVolume::bake(&triangles, min, max, SKY_STEP);
    let points: Vec<(Vec3, Vec3)> = spec.model.mesh.data.as_chunks::<10>().0.iter().map(|v| (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]))).collect();
    let occlusion = occlusion(&triangles, &points, spec.sky_box);
    let photographs = materials::find_textures().ok().and_then(|dir| {
        let colours: Vec<TextureSource> = NAMES.iter().map(|n| TextureSource::file(dir.join(format!("{n}_color.jpg")))).collect();
        let details: Vec<TextureSource> = NAMES
            .iter()
            .map(|n| {
                let ao = dir.join(format!("{n}_ao.jpg"));
                TextureSource::Channels { rg: dir.join(format!("{n}_normal.png")), b: Some(dir.join(format!("{n}_rough.jpg"))), a: engine::files::exists(&ao).then_some(ao) }
            })
            .collect();
        match (prepare_layers("cabin: colour", &colours, Kind::Color), prepare_layers("cabin: detail", &details, Kind::NormalPacked)) {
            (Ok(colour), Ok(detail)) => Some((colour, detail)),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("flight: the cockpit's textures: {e}");
                None
            }
        }
    });
    Baked { spec, sky, occlusion, photographs, pictures }
}

/// The pictures of a cabin's textured materials as two arrays, one layer a material: the colours,
/// and the normal maps (flat for a material without one).
fn pictures(pictures: Vec<super::Picture>) -> Option<(PreparedLayers, PreparedLayers)> {
    if pictures.is_empty() {
        return None;
    }
    let (w, h) = pictures.iter().find_map(|p| p.normal.as_ref().map(|n| (n.width, n.height))).unwrap_or((1, 1));
    let (mut colours, mut normals) = (Vec::new(), Vec::new());
    for picture in pictures {
        colours.push(TextureSource::Pixels { width: picture.colour.width, height: picture.colour.height, rgba: picture.colour.rgba });
        let flat = || [128, 128, 255, 255].repeat((w * h) as usize);
        normals.push(TextureSource::Pixels { width: w, height: h, rgba: picture.normal.map_or_else(flat, |n| n.rgba) });
    }
    match (prepare_layers("cabin: pictures", &colours, Kind::Color), prepare_layers("cabin: pictures' relief", &normals, Kind::Normal)) {
        (Ok(colour), Ok(normal)) => Some((colour, normal)),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("flight: the cockpit's pictures: {e}");
            None
        }
    }
}

/// Starts baking a kind's cockpit (`bake`) on another thread; it comes back through the receiver.
pub fn start_baking(cabin: &'static str) -> Receiver<Baked> {
    let (sender, receiver) = channel();
    engine::threads::spawn(move || {
        // The game may have gone by the time it is done.
        let _ = sender.send(bake(cabin));
    });
    receiver
}

impl CockpitRenderer {
    /// The cockpit on the GPU, from what `bake` made: only copies, and the print painted.
    pub fn new(gpu: &Gpu, layout: &wgpu::BindGroupLayout, baked: Baked) -> CockpitRenderer {
        let Baked { spec: cabin, sky, occlusion, photographs, pictures } = baked;
        let (w, h) = cabin.canvas_points;
        let canvas = Canvas::new(gpu, "instruments", (w * CANVAS_SCALE) as u32, (h * CANVAS_SCALE) as u32, CANVAS_SCALE);
        let (pw, ph) = cabin.print_points;
        let mut print = Canvas::new(gpu, "print", (pw * PRINT_SCALE) as u32, (ph * PRINT_SCALE) as u32, PRINT_SCALE);
        print.paint(gpu, [0.0; 4], |ctx| super::print::paint(&ctx.layer_painter(engine::egui::LayerId::background()), &cabin.print));
        let blank = |label: &str, kind: Kind, rgba: [u8; 4]| {
            let layers: Vec<TextureSource> = NAMES.iter().map(|_| TextureSource::Pixels { width: 1, height: 1, rgba: rgba.to_vec() }).collect();
            gpu.texture_array(label, &layers, kind).expect("1 x 1 textures")
        };
        let uploaded = photographs.and_then(|(colour, detail)| match (gpu.upload_layers("cabin: colour", &colour, 1), gpu.upload_layers("cabin: detail", &detail, 1)) {
            (Ok(colour), Ok(detail)) => Some((colour, detail)),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("flight: the cockpit's textures: {e}");
                None
            }
        });
        let (colour, detail) = uploaded.unwrap_or_else(|| (blank("cabin: colour (none)", Kind::Color, [128, 128, 128, 255]), blank("cabin: detail (none)", Kind::NormalPacked, [128, 128, 204, 255])));
        let one = |label: &str, kind: Kind, rgba: [u8; 4]| gpu.texture_array(label, &[TextureSource::Pixels { width: 1, height: 1, rgba: rgba.to_vec() }], kind).expect("a 1 x 1 texture");
        let pictures = pictures.and_then(|(colour, normal)| match (gpu.upload_layers("cabin: pictures", &colour, 0), gpu.upload_layers("cabin: pictures' relief", &normal, 0)) {
            (Ok(colour), Ok(normal)) => Some((colour, normal)),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("flight: the cockpit's pictures: {e}");
                None
            }
        });
        let (picture, relief) = pictures.unwrap_or_else(|| (one("cabin: pictures (none)", Kind::Color, [128, 128, 128, 255]), one("cabin: pictures' relief (none)", Kind::Normal, [128, 128, 255, 255])));
        let sky = gpu.volume("cabin: sky", sky.size, &sky.texels).unwrap_or_else(|e| {
            eprintln!("flight: the cockpit's light: {e}");
            gpu.volume("cabin: sky (none)", [1, 1, 1], &[60, 128, 128, 128]).expect("a 1 x 1 x 1 volume")
        });
        let sampler = gpu.texture_sampler();
        let shadow_sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cabin: shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let place = CabinUniforms {
            light: Mat4::IDENTITY.to_cols_array_2d(),
            lights: [0.6, 0.6, 0.0, 1.0 / SHADOW_SIZE as f32],
            sky: cabin.sky_box.0.extend(SKY_STEP).to_array(),
            panel_print: [cabin.panel_print.min.0, cabin.panel_print.min.1, cabin.panel_print.size.0, cabin.panel_print.size.1],
            floor_print: [cabin.floor_print.min.0, cabin.floor_print.min.1, cabin.floor_print.size.0, cabin.floor_print.size.1],
            print: [pw, ph, cabin.floor_print.top, cabin.floor_print.points],
            place: [cabin.panel_x, 0.0, 0.0, 0.0],
        };
        let uniforms = wgpu::util::DeviceExt::create_buffer_init(&gpu.device, &wgpu::util::BufferInitDescriptor { label: Some("cabin"), contents: bytemuck::bytes_of(&place), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        let occlusion = wgpu::util::DeviceExt::create_buffer_init(&gpu.device, &wgpu::util::BufferInitDescriptor { label: Some("cabin: occlusion"), contents: bytemuck::cast_slice(&occlusion), usage: wgpu::BufferUsages::VERTEX });
        let shadow = Shadow::new(gpu);
        let bind = bind_group(gpu, layout, [canvas.view(), &colour.view, &detail.view, &sky.view, print.view(), &shadow.view, &picture.view, &relief.view], (&sampler, &shadow_sampler), &uniforms);
        CockpitRenderer {
            mesh: cabin.model.mesh.build(&gpu.device),
            parts: cabin.model.parts,
            occlusion,
            glass: cabin.glass.build(&gpu.device),
            canvas,
            levers: cabin.levers,
            controls: cabin.controls,
            shadow,
            uniforms,
            place,
            sky_box: cabin.sky_box,
            bind,
        }
    }

    /// Group 2 of the cockpit's pipelines.
    pub fn bind(&self) -> &wgpu::BindGroup {
        &self.bind
    }

    /// The vertex buffer layout of the occlusion: one float per vertex at location 3.
    pub fn occlusion_layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 1] = [wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 0, shader_location: 3 }];
        wgpu::VertexBufferLayout { array_stride: 4, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRIBUTES }
    }

    /// Each part of the cockpit to draw: its first vertex, its count, and where it is (`body` moved
    /// as the controls say); the yokes or the stick only if the controls say so.
    pub fn draws(&self, body: Mat4, c: &CockpitControls) -> Vec<(u32, u32, Mat4)> {
        self.parts.iter().filter(|p| c.yokes || !p.is_yoke()).map(|p| (p.first, p.count, p.matrix(body, c))).collect()
    }

    /// Draws the sun's shadow map of the cockpit as the controls have it and of `casters` (ranges of
    /// meshes in body axes: the wing, the cowling), towards `sun` (body axes; `None` with the sun
    /// down), and writes what the cockpit's shader reads this frame. Its own submission: call it
    /// from `Game::update`.
    pub fn prepare(&mut self, gpu: &Gpu, sun: Option<Vec3>, c: &CockpitControls, casters: &[(&wgpu::Buffer, Range<u32>)]) {
        let light = sun.map_or(Mat4::IDENTITY, |sun| light_matrix(sun, self.sky_box));
        let uniforms = CabinUniforms { light: light.to_cols_array_2d(), lights: [c.panel_lights, c.standby_lights, if sun.is_some() { 1.0 } else { 0.0 }, 1.0 / SHADOW_SIZE as f32], ..self.place };
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        if sun.is_none() {
            return;
        }
        let mut draws: Vec<(&wgpu::Buffer, Range<u32>, Mat4)> = self.parts.iter().filter(|p| c.yokes || !p.is_yoke()).map(|p| (&self.mesh.buffer, p.first..p.first + p.count, light * p.matrix(Mat4::IDENTITY, c))).collect();
        draws.extend(casters.iter().map(|(buffer, range)| (*buffer, range.clone(), light)));
        draws.truncate(SHADOW_DRAWS);
        let mut bytes = vec![0u8; SHADOW_STRIDE as usize * draws.len()];
        for (k, (_, _, m)) in draws.iter().enumerate() {
            bytes[k * SHADOW_STRIDE as usize..k * SHADOW_STRIDE as usize + 64].copy_from_slice(bytemuck::bytes_of(&m.to_cols_array()));
        }
        gpu.queue.write_buffer(&self.shadow.casters, 0, &bytes);
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cabin shadow") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cabin shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.shadow.view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow.pipeline);
            for (k, (buffer, range, _)) in draws.iter().enumerate() {
                pass.set_bind_group(0, &self.shadow.bind, &[(k as u64 * SHADOW_STRIDE) as u32]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range.clone(), 0..1);
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

/// Group 2 of the cockpit's pipelines: the instruments' canvas, the photographs' colour and detail,
/// the light through the windows, the print and the sun's shadow map (`views`, in that order), the
/// sampler and the shadow's (`samplers`), the cockpit's uniforms; at bindings 0 to 8.
fn bind_group(gpu: &Gpu, layout: &wgpu::BindGroupLayout, views: [&wgpu::TextureView; 8], samplers: (&wgpu::Sampler, &wgpu::Sampler), uniforms: &wgpu::Buffer) -> wgpu::BindGroup {
    let [canvas, colour, detail, sky, print, shadow, picture, relief] = views;
    let view = |binding: u32, view| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) };
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cabin"),
        layout,
        entries: &[
            view(0, canvas),
            view(1, colour),
            view(2, detail),
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(samplers.0) },
            view(4, sky),
            view(5, print),
            wgpu::BindGroupEntry { binding: 6, resource: uniforms.as_entire_binding() },
            view(7, shadow),
            wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(samplers.1) },
            view(9, picture),
            view(10, relief),
        ],
    })
}

/// How open each vertex is to the cockpit round it, within 30 cm, in cells of 1 cm within `bounds`;
/// vertices that share a place and a direction share the work.
fn occlusion(triangles: &[[Vec3; 3]], points: &[(Vec3, Vec3)], (min, max): (Vec3, Vec3)) -> Vec<f32> {
    let key = |(p, n): &(Vec3, Vec3)| ((*p * 2000.0).round().as_ivec3(), (*n * 50.0).round().as_ivec3());
    let mut unique = std::collections::HashMap::new();
    let mut distinct = Vec::new();
    let slots: Vec<usize> = points
        .iter()
        .map(|point| {
            *unique.entry(key(point)).or_insert_with(|| {
                distinct.push(*point);
                distinct.len() - 1
            })
        })
        .collect();
    let open = engine::ambient_occlusion(triangles, &distinct, min, max, 0.01, 0.30);
    let open = smoothed(&distinct, &open, OCCLUSION_SMOOTHING);
    slots.iter().map(|&k| open[k]).collect()
}

/// How far apart points of a surface share their occlusion (`smoothed`): about two of its cells.
const OCCLUSION_SMOOTHING: f32 = 0.02;

/// Each point's openness averaged with that of the points within `radius` facing about the same
/// way, nearer ones more: the occlusion's cells of 1 cm make neighbours on a smooth surface differ
/// more than the surface does, and the difference shows as blotches along its long faces.
fn smoothed(points: &[(Vec3, Vec3)], open: &[f32], radius: f32) -> Vec<f32> {
    let cell = |p: Vec3| (p / radius).floor().as_ivec3();
    let mut cells: std::collections::HashMap<glam::IVec3, Vec<usize>> = std::collections::HashMap::new();
    for (k, (p, _)) in points.iter().enumerate() {
        cells.entry(cell(*p)).or_default().push(k);
    }
    points
        .iter()
        .map(|&(p, n)| {
            let c = cell(p);
            let (mut sum, mut weight) = (0.0, 0.0);
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        for &k in cells.get(&(c + glam::IVec3::new(dx, dy, dz))).into_iter().flatten() {
                            let (q, m) = points[k];
                            let w = (1.0 - (q - p).length() / radius).max(0.0) * n.dot(m).max(0.0).powi(4);
                            sum += open[k] * w;
                            weight += w;
                        }
                    }
                }
            }
            if weight > 0.0 { sum / weight } else { 1.0 }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sun's view holds the whole cockpit, nearer the sun is nearer in depth.
    #[test]
    fn the_shadow_map_holds_the_cockpit() {
        let (min, max) = (Vec3::new(-1.3, -0.6, -1.0), Vec3::new(1.1, 0.6, 0.6));
        for sun in [Vec3::new(0.3, 0.2, -0.9), Vec3::new(-0.5, 0.6, -0.4), Vec3::new(0.0, 0.0, -1.0)] {
            let sun = sun.normalize();
            let m = light_matrix(sun, (min, max));
            for p in [min, max, Vec3::new(min.x, max.y, min.z)] {
                let q = m.project_point3(p);
                assert!(q.x.abs() <= 1.0001 && q.y.abs() <= 1.0001 && (0.0..=1.0).contains(&q.z), "{sun}: {p} at {q}");
            }
            let (near, far) = (m.project_point3(sun * 0.5).z, m.project_point3(Vec3::ZERO).z);
            assert!(near < far, "{near} {far}");
        }
    }
}
