//! What the trees are drawn with on the GPU besides their meshes: the foliage's cards and the
//! barks (made and read on a thread when the renderer starts: `foliage.rs`, `assets/textures`),
//! and each species' picture from far, baked once they are there.
//!
//! **The pictures from far** (impostors): a tree beyond a few hundred metres is drawn as one
//! square facing the eye, painted with what its full mesh looks like from the side the eye is on.
//! Per species, a layer of 8 x 8 views over the upper half of the sky round the tree,
//! hemi-octahedral (each view the direction it is taken from, the grid folded over the hemisphere
//! so that the views are about evenly spread); each view is the unit model drawn unlit,
//! orthographic, within `RADIUS` of its middle: its colour and outline, its normal and how far in
//! front of or behind the view's middle plane each texel is. The scene reads the four views nearest
//! the eye's direction, where the eye's ray crosses each one's middle plane, and lights the result
//! as it lights the meshes (`impostor_sample`, `flora_light` in flora.wgsl): the same colours, the
//! same light through the leaves, the same shade of the crown, so a tree dissolves from its mesh into
//! its picture unnoticed. The view from the sun also gives each tree's shadow on the ground its
//! outline (`tree_shadow_fragment`).
//!
//! The views are drawn at twice the size they are kept at and shrunk (outlines smooth), then halved
//! down to 4 texels a view, colour and normal weighted by what each texel covers. `Quality::
//! tree_picture_size` says the side of a view: 80 texels (high, 15 layers of 640 x 640, 65 MB with
//! their levels), 64 (medium, 42 MB), 32 (low, 10 MB). They are baked once, when the cards are made,
//! and again for another size of view.

use crate::foliage;
use crate::species::{self, BARK_FILES};
use crate::vegetation::{self, Detail, FloraMesh};
use engine::texture::{Kind, Texture, TextureSource};
use engine::{Gpu, Mesh, wgpu};
use glam::Vec3;
use std::sync::mpsc::{Receiver, channel};

/// Views a side of a species' picture (`IMPOSTOR_FRAMES` in flora.wgsl).
pub const FRAMES: u32 = 8;
/// The radius round the middle of the unit model each view holds (`IMPOSTOR_RADIUS`), and its
/// middle (`IMPOSTOR_CENTRE`).
pub const RADIUS: f32 = 0.62;
pub const CENTRE: Vec3 = Vec3::new(0.0, 0.5, 0.0);
/// The smallest a view gets in the smaller levels, texels.
const SMALLEST_VIEW: u32 = 4;

/// The direction (unit, up the hemisphere) the view `(i, j)` of the grid is taken from: the
/// hemi-octahedral folding (`impostor_direction` in flora.wgsl).
pub fn direction(i: u32, j: u32) -> Vec3 {
    let g = |k: u32| (k as f32 + 0.5) / FRAMES as f32 * 2.0 - 1.0;
    let (x, y) = (g(i), g(j));
    let (px, pz) = ((x + y) / 2.0, (x - y) / 2.0);
    Vec3::new(px, 1.0 - px.abs() - pz.abs(), pz).normalize()
}

/// The place (0..1 each way) in the grid of the view a direction is nearest: the inverse of `direction`
/// (`impostor_place` in flora.wgsl).
#[cfg(test)]
pub fn place(d: Vec3) -> (f32, f32) {
    let u = Vec3::new(d.x, d.y.max(0.0), d.z).normalize();
    let sum = u.x.abs() + u.y.abs() + u.z.abs();
    let (px, pz) = (u.x / sum, u.z / sum);
    ((px + pz) * 0.5 + 0.5, (px - pz) * 0.5 + 0.5)
}

/// The levels a picture of views of `size` texels has.
fn levels(size: u32) -> u32 {
    (size / SMALLEST_VIEW).max(1).ilog2() + 1
}

/// What the background thread makes: the foliage's arrays and the barks'.
struct Made {
    foliage: foliage::Prepared,
    bark_colour: engine::texture::PreparedLayers,
    bark_normal: engine::texture::PreparedLayers,
}

/// The trees' textures, meshes and pictures from far.
pub struct Flora {
    /// Each species' meshes, near then far (`mesh`).
    meshes: Vec<Mesh>,
    /// A tuft of grass (`vegetation::tuft`).
    tuft: Mesh,
    foliage: (Texture, Texture),
    bark: (Texture, Texture),
    /// The pictures from far: colour and detail, one layer per species, and the side of a view.
    pictures: (wgpu::Texture, wgpu::Texture),
    view_size: u32,
    /// Whether the pictures are of the textures now bound (they are made again when those come).
    baked: bool,
    making: Option<Receiver<Result<Made, String>>>,
    sampler: wgpu::Sampler,
    picture_sampler: wgpu::Sampler,
    /// Group 2 of the trees' pipelines (`pipelines::flora_layout`).
    pub bind: wgpu::BindGroup,
}

/// A texture array of one texel per layer, for what is not made yet.
fn blank(gpu: &Gpu, label: &str, kind: Kind, layers: usize, rgba: [u8; 4]) -> Texture {
    let sources: Vec<TextureSource> = (0..layers).map(|_| TextureSource::Pixels { width: 1, height: 1, rgba: rgba.to_vec() }).collect();
    gpu.texture_array(label, &sources, kind).expect("a 1 x 1 texture")
}

fn pictures(gpu: &Gpu, view: u32) -> (wgpu::Texture, wgpu::Texture) {
    let make = |label, format| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: view * FRAMES, height: view * FRAMES, depth_or_array_layers: species::COUNT as u32 },
            mip_level_count: levels(view),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    };
    (make("tree pictures", wgpu::TextureFormat::Rgba8UnormSrgb), make("tree pictures' detail", wgpu::TextureFormat::Rgba8Unorm))
}

fn array_view(t: &wgpu::Texture) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() })
}

/// A target cleared to nothing.
fn attachment(view: &wgpu::TextureView) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })
}

/// One level of one layer, to draw into or read from.
fn level_view(t: &wgpu::Texture, layer: u32, level: u32) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_mip_level: level,
        mip_level_count: Some(1),
        base_array_layer: layer,
        array_layer_count: Some(1),
        ..Default::default()
    })
}

impl Flora {
    /// Starts making the foliage and reading the barks (from `textures`, `assets/textures`) on a
    /// thread; until they come the trees are drawn with nothing (`pending`).
    pub fn new(gpu: &Gpu, layout: &wgpu::BindGroupLayout, view_size: u32, textures: Option<std::path::PathBuf>) -> Flora {
        let mut meshes = Vec::with_capacity(species::COUNT * 2);
        for s in species::Species::ALL.iter() {
            meshes.push(vegetation::tree(*s, Detail::Near).build(&gpu.device));
            meshes.push(vegetation::tree(*s, Detail::Far).build(&gpu.device));
        }
        let making = textures.map(|dir| {
            let (send, receive) = channel();
            engine::threads::spawn(move || {
                let made = (|| {
                    let foliage = foliage::load(&dir)?;
                    let files = |suffix: &str| BARK_FILES.iter().map(|name| TextureSource::file(dir.join(format!("{name}{suffix}")))).collect::<Vec<_>>();
                    let bark_colour = engine::texture::prepare_layers("bark", &files("_color.jpg"), Kind::Color).map_err(|e| e.to_string())?;
                    let bark_normal = engine::texture::prepare_layers("bark relief", &files("_normal.png"), Kind::Normal).map_err(|e| e.to_string())?;
                    Ok(Made { foliage, bark_colour, bark_normal })
                })();
                let _ = send.send(made);
            });
            receive
        });
        let foliage = (blank(gpu, "foliage", Kind::Color, 1, [128, 128, 128, 0]), blank(gpu, "foliage detail", Kind::Data, 1, [128, 128, 0, 0]));
        let bark = (blank(gpu, "bark", Kind::Color, 1, [90, 80, 70, 255]), blank(gpu, "bark relief", Kind::Normal, 1, [128, 128, 255, 255]));
        let sampler = gpu.texture_sampler();
        let picture_sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tree pictures"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let pictures = pictures(gpu, view_size);
        let bind = Flora::bind_group(gpu, layout, &foliage, &bark, &pictures, (&sampler, &picture_sampler));
        let tuft = vegetation::tuft().build(&gpu.device);
        Flora { meshes, tuft, foliage, bark, pictures, view_size, baked: false, making, sampler, picture_sampler, bind }
    }

    fn bind_group(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        foliage: &(Texture, Texture),
        bark: &(Texture, Texture),
        pictures: &(wgpu::Texture, wgpu::Texture),
        (sampler, picture_sampler): (&wgpu::Sampler, &wgpu::Sampler),
    ) -> wgpu::BindGroup {
        let (colour, detail) = (array_view(&pictures.0), array_view(&pictures.1));
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flora"),
            layout,
            entries: &[
                foliage.0.entry(2),
                foliage.1.entry(3),
                bark.0.entry(4),
                bark.1.entry(5),
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&colour) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&detail) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::Sampler(picture_sampler) },
            ],
        })
    }

    fn rebind(&mut self, gpu: &Gpu, layout: &wgpu::BindGroupLayout) {
        self.bind = Flora::bind_group(gpu, layout, &self.foliage, &self.bark, &self.pictures, (&self.sampler, &self.picture_sampler));
    }

    /// The mesh of a tuft of grass.
    pub fn tuft(&self) -> &Mesh {
        &self.tuft
    }

    /// The mesh of a species (its place in `Species::ALL`) at a level.
    pub fn mesh(&self, species: usize, detail: Detail) -> &Mesh {
        &self.meshes[species * 2 + usize::from(detail == Detail::Far)]
    }

    /// Takes what the thread made, and bakes the pictures from far once it is there. Call every frame.
    pub fn update(&mut self, gpu: &Gpu, layout: &wgpu::BindGroupLayout) {
        if let Some(receiver) = &self.making {
            match receiver.try_recv() {
                Ok(Ok(made)) => {
                    // With the smallest pictures from far (low), the cards and barks at half their side too.
                    let drop = u32::from(self.view_size < 64);
                    let upload = |label, prepared: &engine::texture::PreparedLayers| gpu.upload_layers(label, prepared, drop).unwrap_or_else(|e| panic!("{e}"));
                    self.foliage = (upload("foliage", &made.foliage.colour), upload("foliage detail", &made.foliage.detail));
                    self.bark = (upload("bark", &made.bark_colour), upload("bark relief", &made.bark_normal));
                    self.making = None;
                    self.baked = false;
                }
                Ok(Err(error)) => {
                    eprintln!("the trees' pictures: {error}");
                    std::process::exit(1);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.making = None,
            }
        }
        if !self.baked && self.making.is_none() {
            self.bake(gpu);
            self.baked = true;
            self.rebind(gpu, layout);
        }
    }

    /// Pictures of views of another side (`Quality::tree_picture_size`), baked again.
    pub fn set_view_size(&mut self, gpu: &Gpu, layout: &wgpu::BindGroupLayout, view_size: u32) {
        if view_size != self.view_size {
            self.view_size = view_size;
            self.pictures = pictures(gpu, view_size);
            self.baked = false;
            self.rebind(gpu, layout);
        }
    }

    /// 1 until the foliage is made and the pictures baked.
    pub fn pending(&self) -> usize {
        usize::from(!self.baked)
    }

    /// Draws every species' views, shrinks them into the pictures and halves those level by level.
    fn bake(&self, gpu: &Gpu) {
        let device = &gpu.device;
        let source = format!("{}{}{}", species::wgsl(), include_str!("flora_surface.wgsl"), include_str!("impostor_bake.wgsl"));
        let shader = gpu.shader("tree pictures", &source);
        let uniform = |binding, dynamic| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: wgpu::BufferSize::new(64) },
            count: None,
        };
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false },
            count: None,
        };
        let views_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("tree views"), entries: &[uniform(0, true)] });
        let empty = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("nothing"), entries: &[] });
        let textures_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tree views' textures"),
            entries: &[
                texture(2, wgpu::TextureViewDimension::D2Array),
                texture(3, wgpu::TextureViewDimension::D2Array),
                texture(4, wgpu::TextureViewDimension::D2Array),
                texture(5, wgpu::TextureViewDimension::D2Array),
                wgpu::BindGroupLayoutEntry { binding: 8, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let shrink_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tree pictures shrunk"),
            entries: &[texture(1, wgpu::TextureViewDimension::D2), texture(2, wgpu::TextureViewDimension::D2)],
        });
        let targets = [Some(wgpu::TextureFormat::Rgba8UnormSrgb.into()), Some(wgpu::TextureFormat::Rgba8Unorm.into())];
        let pipeline = |label, layouts: &[Option<&wgpu::BindGroupLayout>], vertex, fragment, buffers: &[Option<wgpu::VertexBufferLayout>], depth: bool| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: layouts, immediate_size: 0 });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vertex), buffers, compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(fragment), targets: &targets, compilation_options: Default::default() }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: depth.then(|| wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let draw = pipeline("tree views", &[Some(&views_layout), Some(&empty), Some(&textures_layout)], "bake_vertex", "bake_fragment", &[Some(FloraMesh::layout())], true);
        let shrink = pipeline("tree pictures shrunk", &[Some(&shrink_layout)], "shrink_vertex", "shrink_fragment", &[], false);

        // Each view's axes, at offsets the device allows.
        let stride = (device.limits().min_uniform_buffer_offset_alignment as usize).max(64);
        let mut views = vec![0u8; stride * (FRAMES * FRAMES) as usize];
        for j in 0..FRAMES {
            for i in 0..FRAMES {
                let d = direction(i, j);
                let right = if d.y > 0.999 { Vec3::X } else { Vec3::Y.cross(d).normalize() };
                let up = d.cross(right);
                let block: [f32; 16] = [right.x, right.y, right.z, RADIUS, up.x, up.y, up.z, 0.0, d.x, d.y, d.z, 0.0, CENTRE.x, CENTRE.y, CENTRE.z, 0.0];
                let at = (j * FRAMES + i) as usize * stride;
                views[at..at + 64].copy_from_slice(bytemuck::cast_slice(&block));
            }
        }
        let views_buffer = wgpu::util::DeviceExt::create_buffer_init(device, &wgpu::util::BufferInitDescriptor { label: Some("tree views"), contents: &views, usage: wgpu::BufferUsages::UNIFORM });
        let views_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tree views"),
            layout: &views_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &views_buffer, offset: 0, size: wgpu::BufferSize::new(64) }) }],
        });
        let empty_bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("nothing"), layout: &empty, entries: &[] });
        let textures_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tree views' textures"),
            layout: &textures_layout,
            entries: &[
                self.foliage.0.entry(2),
                self.foliage.1.entry(3),
                self.bark.0.entry(4),
                self.bark.1.entry(5),
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });

        // The views drawn at twice their size.
        let big = self.view_size * 2 * FRAMES;
        let target = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: big, height: big, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        };
        let (colour, detail, depth) =
            (target("tree views", wgpu::TextureFormat::Rgba8UnormSrgb), target("tree views' detail", wgpu::TextureFormat::Rgba8Unorm), target("tree views' depth", wgpu::TextureFormat::Depth32Float));
        let (colour_view, detail_view, depth_view) = (colour.create_view(&Default::default()), detail.create_view(&Default::default()), depth.create_view(&Default::default()));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("tree pictures") });
        let shrink_into = |encoder: &mut wgpu::CommandEncoder, from: (&wgpu::TextureView, &wgpu::TextureView), into: (&wgpu::TextureView, &wgpu::TextureView)| {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tree pictures shrunk"),
                layout: &shrink_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(from.0) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(from.1) },
                ],
            });
            let mut pass =
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("tree pictures shrunk"), color_attachments: &[attachment(into.0), attachment(into.1)], ..Default::default() });
            pass.set_pipeline(&shrink);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        };
        for layer in 0..species::COUNT {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("tree views"),
                    color_attachments: &[attachment(&colour_view), attachment(&detail_view)],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&draw);
                pass.set_bind_group(1, &empty_bind, &[]);
                pass.set_bind_group(2, &textures_bind, &[]);
                let m = &self.meshes[layer * 2];
                pass.set_vertex_buffer(0, m.buffer.slice(..));
                if let Some(indices) = &m.indices {
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                }
                let side = (self.view_size * 2) as f32;
                for j in 0..FRAMES {
                    for i in 0..FRAMES {
                        pass.set_viewport(i as f32 * side, j as f32 * side, side, side, 0.0, 1.0);
                        pass.set_bind_group(0, &views_bind, &[((j * FRAMES + i) as usize * stride) as u32]);
                        pass.draw_indexed(0..m.count, 0, 0..1);
                    }
                }
            }
            let first = (level_view(&self.pictures.0, layer as u32, 0), level_view(&self.pictures.1, layer as u32, 0));
            shrink_into(&mut encoder, (&colour_view, &detail_view), (&first.0, &first.1));
            for level in 1..levels(self.view_size) {
                let from = (level_view(&self.pictures.0, layer as u32, level - 1), level_view(&self.pictures.1, layer as u32, level - 1));
                let into = (level_view(&self.pictures.0, layer as u32, level), level_view(&self.pictures.1, layer as u32, level));
                shrink_into(&mut encoder, (&from.0, &from.1), (&into.0, &into.1));
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_views_cover_the_upper_half_of_the_sky() {
        for j in 0..FRAMES {
            for i in 0..FRAMES {
                let d = direction(i, j);
                assert!(d.y >= -1e-6 && (d.length() - 1.0).abs() < 1e-5);
                // A view's direction is found back in its own cell.
                let (u, v) = place(d);
                assert_eq!(((u * FRAMES as f32) as u32, (v * FRAMES as f32) as u32), (i, j));
            }
        }
        // Straight down onto the tree is the middle of the grid; the horizon its rim.
        assert_eq!(place(Vec3::Y), (0.5, 0.5));
        let (u, v) = place(Vec3::X);
        assert!((u - 1.0).abs() < 1e-6 && (v - 1.0).abs() < 1e-6);
        assert_eq!(levels(80), 5);
        assert_eq!(levels(32), 4);
    }

    #[test]
    fn the_shaders_agree() {
        let wgsl = include_str!("flora.wgsl");
        assert!(wgsl.contains(&format!("const IMPOSTOR_FRAMES: f32 = {:?};", FRAMES as f32)));
        assert!(wgsl.contains(&format!("const IMPOSTOR_RADIUS: f32 = {RADIUS:?};")));
        assert!(wgsl.contains(&format!("const GRASS_LIGHT: i32 = {};", species::Species::Shrub as usize)));
        assert!(wgsl.contains(&format!("const IMPOSTOR_CENTRE: vec3<f32> = vec3<f32>({:?}, {:?}, {:?});", CENTRE.x, CENTRE.y, CENTRE.z)));
    }
}
