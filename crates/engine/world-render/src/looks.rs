//! What the game's textured meshes are painted with (`WorldRenderer::add_look`): a model's materials
//! (glTF's PBR metallic-roughness, `engine::gltf::Material`) and their pictures on the GPU, each with
//! its whole mip chain and sampled with anisotropic filtering, and a bind group per material
//! (group 2 of the textured pipelines, `textured.wgsl`). A mesh drawn with them
//! (`WorldRenderer::add_textured_mesh`) keeps beside its vertices their texture coordinates, and the
//! runs of its vertices of one material each.
//!
//! Per material: its base colour (sRGB), a picture of its occlusion, roughness and metallic in red,
//! green and blue (glTF's metallic-roughness texture, with the occlusion's red put in when it is
//! another picture), its normal map (x and y), its emissive picture. What it lacks is a texel that
//! changes nothing (white; a flat normal). The occlusion-roughness-metallic pictures are kept at
//! most `SURFACE_SIDE` texels wide: they change slowly over a surface.

use engine::gltf::{Image, Material};
use engine::texture::{self, Kind, Texture, TextureSource};
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Gpu};
use std::collections::HashMap;
use std::ops::Range;

/// The widest a material's occlusion-roughness-metallic picture is kept.
const SURFACE_SIDE: u32 = 1024;

/// A look added with `WorldRenderer::add_look`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LookId(pub(crate) usize);

/// A material's numbers as `textured.wgsl` reads them (`SurfaceMaterial`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniforms {
    base: [f32; 4],
    emissive: [f32; 4],
    pbr: [f32; 4],
}

/// A material on the GPU: its bind group, and whether it is glass (drawn blended, after the world).
pub(crate) struct GpuMaterial {
    pub bind: wgpu::BindGroup,
    pub glass: bool,
}

pub(crate) struct Look {
    pub materials: Vec<GpuMaterial>,
    /// What it holds on the GPU, bytes.
    pub bytes: u64,
}

/// A mesh added with its look: its texture coordinates (a second vertex buffer) and its runs of
/// vertices of one material each, in order.
pub(crate) struct TexturedMesh {
    pub look: usize,
    pub uv: wgpu::Buffer,
    pub runs: Vec<(Range<u32>, usize)>,
}

impl TexturedMesh {
    /// The runs of `first .. first + count`, each cut to it: (vertices, material).
    pub fn runs(&self, first: u32, count: u32) -> impl Iterator<Item = (Range<u32>, usize)> + '_ {
        cut(&self.runs, first, count)
    }
}

fn cut(runs: &[(Range<u32>, usize)], first: u32, count: u32) -> impl Iterator<Item = (Range<u32>, usize)> + '_ {
    let end = first + count;
    runs.iter().filter(move |(r, _)| r.start < end && r.end > first).map(move |(r, m)| (r.start.max(first)..r.end.min(end), *m))
}

/// The layout of group 2 of the textured pipelines.
pub(crate) fn layout(gpu: &Gpu) -> wgpu::BindGroupLayout {
    let mut entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 20,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(size_of::<MaterialUniforms>() as u64) },
        count: None,
    }];
    entries.extend((21..25).map(texture::layout_entry));
    entries.push(texture::sampler_layout_entry(25));
    gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("textured materials"), entries: &entries })
}

/// The texture coordinates of a mesh's vertices, the second vertex buffer of the textured pipelines.
/// (Its slot is its place in a pipeline's buffers: 1 for an object, 2 after a house's instances.)
pub(crate) fn uv_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![5 => Float32x2];
    wgpu::VertexBufferLayout { array_stride: 8, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRS }
}

/// The runs of vertices of one material of a mesh's `surface` (texture coordinates and material
/// of each vertex): materials past `materials` are drawn with the first.
pub(crate) fn runs(surface: &[[f32; 3]], materials: usize) -> Vec<(Range<u32>, usize)> {
    let mut runs: Vec<(Range<u32>, usize)> = Vec::new();
    for (k, s) in surface.iter().enumerate() {
        let material = if (s[2] as usize) < materials { s[2] as usize } else { 0 };
        match runs.last_mut() {
            Some((r, m)) if *m == material && r.end == k as u32 => r.end += 1,
            _ => runs.push((k as u32..k as u32 + 1, material)),
        }
    }
    runs
}

fn pixels(image: &Image) -> TextureSource {
    TextureSource::Pixels { width: image.width, height: image.height, rgba: image.rgba.clone() }
}

/// The occlusion-roughness-metallic picture of a material: its metallic-roughness picture with the
/// occlusion's red in red (nearest texel, when the sizes differ), at most `SURFACE_SIDE` wide.
fn surface_image(metallic_roughness: Option<&Image>, occlusion: Option<&Image>) -> Option<Image> {
    let mut out = match (metallic_roughness, occlusion) {
        (None, None) => return None,
        (Some(mr), None) => {
            // Without an occlusion picture red is no occlusion.
            let mut out = mr.clone();
            out.rgba.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[0] = 255);
            out
        }
        (None, Some(o)) => Image { width: o.width, height: o.height, rgba: o.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], 255, 255, 255]).collect() },
        (Some(mr), Some(o)) => {
            let mut out = mr.clone();
            for y in 0..out.height {
                for x in 0..out.width {
                    let (ox, oy) = ((x as u64 * o.width as u64 / out.width as u64) as u32, (y as u64 * o.height as u64 / out.height as u64) as u32);
                    out.rgba[((y * out.width + x) * 4) as usize] = o.rgba[((oy * o.width + ox) * 4) as usize];
                }
            }
            out
        }
    };
    while out.width.max(out.height) > SURFACE_SIDE {
        out = out.half();
    }
    Some(out)
}

/// A model's materials on the GPU, with their pictures: each picture uploaded once for each way it
/// is read.
pub(crate) fn upload(gpu: &Gpu, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, materials: &[Material], images: &[Image]) -> Result<Look, String> {
    let white = Image { width: 1, height: 1, rgba: vec![255; 4] };
    let flat = Image { width: 1, height: 1, rgba: vec![128, 128, 255, 255] };
    let mut textures: HashMap<(Option<usize>, Option<usize>, u8), Texture> = HashMap::new();
    let image = |k: Option<usize>| -> Result<Option<&Image>, String> {
        match k {
            None => Ok(None),
            Some(k) => images.get(k).map(Some).ok_or_else(|| format!("a material names picture {k}, of {}", images.len())),
        }
    };
    let mut bytes = 0;
    let mut get = |key: (Option<usize>, Option<usize>, u8), source: &dyn Fn() -> Result<Image, String>, kind: Kind| -> Result<(), String> {
        if let std::collections::hash_map::Entry::Vacant(entry) = textures.entry(key) {
            let texture = gpu.texture("a model's material", &pixels(&source()?), kind).map_err(|e| e.to_string())?;
            bytes += texture.bytes();
            entry.insert(texture);
        }
        Ok(())
    };
    let mut keys = Vec::new();
    for m in materials {
        let base = (m.texture, None, 0);
        get(base, &|| Ok(image(m.texture)?.unwrap_or(&white).clone()), Kind::Color)?;
        let surface_key = (m.metallic_roughness, m.occlusion, 1);
        get(surface_key, &|| Ok(surface_image(image(m.metallic_roughness)?, image(m.occlusion)?).unwrap_or_else(|| white.clone())), Kind::Data)?;
        let normal = (m.normal, None, 2);
        get(normal, &|| Ok(image(m.normal)?.unwrap_or(&flat).clone()), Kind::Normal)?;
        let glow = (m.emissive_texture, None, 3);
        get(glow, &|| Ok(image(m.emissive_texture)?.unwrap_or(&white).clone()), Kind::Color)?;
        keys.push([base, surface_key, normal, glow]);
    }
    let materials = materials
        .iter()
        .zip(&keys)
        .map(|(m, keys)| {
            let uniforms = MaterialUniforms {
                base: m.base.to_array(),
                emissive: [m.emissive.x, m.emissive.y, m.emissive.z, if m.normal.is_some() { m.normal_scale } else { 0.0 }],
                pbr: [m.metallic, m.roughness, if m.occlusion.is_some() { m.occlusion_strength } else { 0.0 }, if m.blend { 1.0 } else { -m.mask.unwrap_or(0.0) }],
            };
            let buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("a model's material"), contents: bytemuck::bytes_of(&uniforms), usage: wgpu::BufferUsages::UNIFORM });
            let mut entries = vec![wgpu::BindGroupEntry { binding: 20, resource: buffer.as_entire_binding() }];
            entries.extend(keys.iter().enumerate().map(|(i, key)| textures[key].entry(21 + i as u32)));
            entries.push(wgpu::BindGroupEntry { binding: 25, resource: wgpu::BindingResource::Sampler(sampler) });
            GpuMaterial { bind: gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("a model's material"), layout, entries: &entries }), glass: m.blend }
        })
        .collect();
    Ok(Look { materials, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_follow_the_materials_and_draws_are_cut_to_them() {
        let surface: Vec<[f32; 3]> = [0, 0, 0, 1, 1, 1, 0, 0, 0, 9, 9, 9].iter().map(|&m| [0.0, 0.0, m as f32]).collect();
        let runs = runs(&surface, 2);
        assert_eq!(runs, vec![(0..3, 0), (3..6, 1), (6..12, 0)]);
        assert_eq!(cut(&runs, 4, 4).collect::<Vec<_>>(), vec![(4..6, 1), (6..8, 0)]);
    }

    #[test]
    fn the_surface_picture_takes_the_occlusion_in_red() {
        let mr = Image { width: 2, height: 1, rgba: vec![9, 100, 200, 255, 9, 50, 0, 255] };
        let o = Image { width: 1, height: 1, rgba: vec![77, 0, 0, 255] };
        let s = surface_image(Some(&mr), Some(&o)).unwrap();
        assert_eq!(s.rgba, vec![77, 100, 200, 255, 77, 50, 0, 255]);
        assert_eq!(surface_image(Some(&mr), None).unwrap().rgba[0], 255);
        assert!(surface_image(None, None).is_none());
    }
}
