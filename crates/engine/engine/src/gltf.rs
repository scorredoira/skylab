//! glTF 2.0 models: read from a `.glb` (or a `.gltf` whose buffers and images are embedded as
//! `data:` URIs) into named nodes of triangles in the model's space, and written as `.glb`.
//!
//! What is read: the default scene's node hierarchy (each node's transform, translation, rotation
//! and scale or a matrix, applied down the tree), its meshes as triangles (lists, strips and fans,
//! indexed or not), their normals (when a mesh has none, smooth ones: averaged over the faces round
//! each point, except across an edge sharper than `mesh::CREASE_ANGLE`), vertex colours, texture
//! coordinates, and their PBR metallic-roughness materials: base colour (factor, vertex colour and
//! texture), metallic and roughness (factors and texture), the normal map, the occlusion map,
//! emissive (factor and texture), whether it is see-through (`alphaMode: BLEND`) or cut out
//! (`MASK`). Cameras, skins,
//! morph targets and animations are not.
//!
//! The engine's meshes (`MeshBuilder`) carry a colour and a gloss per vertex; [`Model::mesh`] turns a
//! node into one: the base colour times the vertex colour times the texture at the vertex (sampled
//! there, linear, from a level of the texture blurred to about the triangles' size), and the gloss
//! `1 − roughness`, plus 2 on a metal (`metallic` ≥ 0.5): the objects' shader (`world-render`'s
//! `object_shade`) reads both. [`Model::surface`] gives what a mesh drawn with its textures adds to
//! each of those vertices (`world-render`'s textured objects): its texture coordinates and material.
//!
//! [`Writer`] writes nodes of triangles with a material each as a `.glb`, each vertex stored once
//! (indexed): position, normal, colour (`COLOR_0`) and, where given, texture coordinates. The floats
//! are written as they are, so a model read back has the same vertices.

use crate::MeshBuilder;
use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};
use std::collections::HashMap;

/// A vertex of a model's triangles, in the model's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    /// The material's base colour times the vertex colour (`COLOR_0`), linear; the texture's is not in it.
    pub color: Vec4,
    /// Texture coordinates (`TEXCOORD_0`, or the set the base colour's texture names); zero without.
    pub uv: Vec2,
}

/// A material: PBR metallic-roughness, as glTF has it. The textures are indices into
/// `Model::images`; each multiplies its factor.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub base: Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: Vec3,
    /// The base colour's texture (sRGB colour, alpha the opacity).
    pub texture: Option<usize>,
    /// Roughness in green, metallic in blue (linear).
    pub metallic_roughness: Option<usize>,
    /// The tangent-space normal map, and how strongly its x and y bend the normal.
    pub normal: Option<usize>,
    pub normal_scale: f32,
    /// The ambient occlusion in red, and how much of it applies.
    pub occlusion: Option<usize>,
    pub occlusion_strength: f32,
    pub emissive_texture: Option<usize>,
    /// See-through, blended over what is behind (`alphaMode: BLEND`): glass.
    pub blend: bool,
    /// Cut out where the base colour's alpha is below this (`alphaMode: MASK`, its `alphaCutoff`): a decal.
    pub mask: Option<f32>,
}

impl Default for Material {
    /// What a triangle without a material is: white, fully rough, not metal (glTF's default
    /// material is metal; a model without materials is not meant to look like one).
    fn default() -> Material {
        Material {
            name: String::new(),
            base: Vec4::ONE,
            metallic: 0.0,
            roughness: 1.0,
            emissive: Vec3::ZERO,
            texture: None,
            metallic_roughness: None,
            normal: None,
            normal_scale: 1.0,
            occlusion: None,
            occlusion_strength: 1.0,
            emissive_texture: None,
            blend: false,
            mask: None,
        }
    }
}

impl Material {
    /// Whether it has a texture of any kind.
    pub fn textured(&self) -> bool {
        self.texture.or(self.metallic_roughness).or(self.normal).or(self.occlusion).or(self.emissive_texture).is_some()
    }
}

/// An image, decoded: RGBA, 8 bits a channel, sRGB.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// Half its size, each pixel the mean of four (in sRGB, as stored): one level of a mip chain.
    pub fn half(&self) -> Image {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                for c in 0..4 {
                    let at = |i: u32, j: u32| self.rgba[((j.min(self.height - 1) * self.width + i.min(self.width - 1)) * 4 + c) as usize] as u32;
                    let sum = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                    rgba[((y * w + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        Image { width: w, height: h, rgba }
    }

    /// Its colour at `uv`, linear: bilinear, repeating.
    pub fn sample(&self, uv: Vec2) -> Vec4 {
        if self.width == 0 || self.height == 0 {
            return Vec4::ONE;
        }
        let x = uv.x * self.width as f32 - 0.5;
        let y = uv.y * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let texel = |i: f32, j: f32| -> Vec4 {
            let i = (i as i64).rem_euclid(self.width as i64) as usize;
            let j = (j as i64).rem_euclid(self.height as i64) as usize;
            let k = (j * self.width as usize + i) * 4;
            let p = &self.rgba[k..k + 4];
            Vec4::new(srgb_to_linear(p[0]), srgb_to_linear(p[1]), srgb_to_linear(p[2]), p[3] as f32 / 255.0)
        };
        let top = texel(x0, y0).lerp(texel(x0 + 1.0, y0), fx);
        let bottom = texel(x0, y0 + 1.0).lerp(texel(x0 + 1.0, y0 + 1.0), fx);
        top.lerp(bottom, fy)
    }
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// A node of a model: its name, its parent, where it is and its triangles.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    /// Index in `Model::nodes`; parents come before their children.
    pub parent: Option<usize>,
    /// Its space to its parent's, and to the model's.
    pub local: Mat4,
    pub world: Mat4,
    /// Its mesh's triangles in the model's space, three vertices each, and each triangle's material
    /// (an index into `Model::materials`).
    pub vertices: Vec<Vertex>,
    pub materials: Vec<usize>,
}

impl Node {
    /// Its origin in the model's space.
    pub fn origin(&self) -> Vec3 {
        self.world.w_axis.truncate()
    }
}

/// A glTF model read: its nodes, materials and images.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub materials: Vec<Material>,
    pub images: Vec<Image>,
}

impl Model {
    /// The node of this name, if there is one.
    pub fn node(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// The colour a vertex of a triangle of `material` has: its own times the texture's there.
    pub fn color(&self, vertex: &Vertex, material: usize) -> Vec4 {
        let texture = self.materials.get(material).and_then(|m| m.texture).and_then(|t| self.images.get(t));
        texture.map_or(vertex.color, |image| vertex.color * image.sample(vertex.uv))
    }

    /// The model with each base colour's texture shrunk to a level of its mip chain `levels` down:
    /// what [`Model::mesh`] samples at each vertex, a texel about as large as a triangle, so that
    /// a texture's fine print does not land as blotches on the corners it happens to fall on.
    pub fn blurred(&self, levels: u32) -> Model {
        let mut model = self.clone();
        let used: std::collections::HashSet<usize> = self.materials.iter().filter_map(|m| m.texture).collect();
        for (k, image) in model.images.iter_mut().enumerate() {
            if !used.contains(&k) {
                *image = Image { width: 1, height: 1, rgba: vec![255; 4] };
                continue;
            }
            for _ in 0..levels {
                if image.width.min(image.height) <= 16 {
                    break;
                }
                *image = image.half();
            }
        }
        model
    }

    /// What a mesh drawn with its materials' textures adds to each vertex of [`Model::mesh`] of
    /// `node`, in the same order: its texture coordinates and its material (an index into
    /// `materials`; `u32::MAX as f32` without one).
    pub fn surface(&self, node: &Node) -> Vec<[f32; 3]> {
        node.vertices.iter().enumerate().map(|(k, v)| [v.uv.x, v.uv.y, node.materials[k / 3] as f32]).collect()
    }

    /// The same node with its triangles in the order of their materials (each material's in the
    /// order they had): one run of vertices per material, to draw each with its own textures.
    pub fn sorted(&self, node: &Node) -> Node {
        let mut order: Vec<usize> = (0..node.materials.len()).collect();
        order.sort_by_key(|&t| node.materials[t]);
        Node {
            vertices: order.iter().flat_map(|&t| node.vertices[t * 3..t * 3 + 3].iter().copied()).collect(),
            materials: order.iter().map(|&t| node.materials[t]).collect(),
            ..node.clone()
        }
    }

    /// The gloss of a material for the engine's meshes: `1 − roughness`, plus 2 on a metal.
    pub fn gloss(&self, material: usize) -> f32 {
        let m = self.materials.get(material).cloned().unwrap_or_default();
        let gloss = 1.0 - m.roughness.clamp(0.0, 1.0);
        if m.metallic >= 0.5 { gloss + 2.0 } else { gloss }
    }

    /// A node's triangles as one of the engine's meshes (position, normal, colour with gloss in
    /// alpha: see the module's documentation), in the model's space.
    pub fn mesh(&self, node: &Node) -> MeshBuilder {
        let mut b = MeshBuilder::new();
        for (k, v) in node.vertices.iter().enumerate() {
            let material = node.materials[k / 3];
            b.vertex(v.position, v.normal, self.color(v, material).truncate().extend(self.gloss(material)));
        }
        b
    }
}

/// Reads a `.glb`, or a `.gltf` with everything embedded.
pub fn read(bytes: &[u8]) -> Result<Model, String> {
    let ::gltf::Gltf { document, blob } = ::gltf::Gltf::from_slice(bytes).map_err(|e| format!("glTF: {e}"))?;
    let buffers: Vec<Vec<u8>> = document
        .buffers()
        .map(|buffer| match buffer.source() {
            ::gltf::buffer::Source::Bin => blob.clone().ok_or_else(|| "glTF: no binary chunk".to_string()),
            ::gltf::buffer::Source::Uri(uri) => data_uri(uri),
        })
        .collect::<Result<_, _>>()?;
    let images = document
        .images()
        .map(|image| {
            let bytes = match image.source() {
                ::gltf::image::Source::View { view, .. } => buffers[view.buffer().index()][view.offset()..view.offset() + view.length()].to_vec(),
                ::gltf::image::Source::Uri { uri, .. } => data_uri(uri)?,
            };
            let decoded = image::load_from_memory(&bytes).map_err(|e| format!("glTF: image {}: {e}", image.index()))?.to_rgba8();
            Ok(Image { width: decoded.width(), height: decoded.height(), rgba: decoded.into_raw() })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let materials: Vec<Material> = document
        .materials()
        .map(|m| {
            let pbr = m.pbr_metallic_roughness();
            Material {
                name: m.name().unwrap_or("").to_string(),
                base: Vec4::from(pbr.base_color_factor()),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: Vec3::from(m.emissive_factor()),
                texture: pbr.base_color_texture().map(|info| info.texture().source().index()),
                metallic_roughness: pbr.metallic_roughness_texture().map(|info| info.texture().source().index()),
                normal: m.normal_texture().map(|info| info.texture().source().index()),
                normal_scale: m.normal_texture().map_or(1.0, |info| info.scale()),
                occlusion: m.occlusion_texture().map(|info| info.texture().source().index()),
                occlusion_strength: m.occlusion_texture().map_or(1.0, |info| info.strength()),
                emissive_texture: m.emissive_texture().map(|info| info.texture().source().index()),
                blend: m.alpha_mode() == ::gltf::material::AlphaMode::Blend,
                mask: (m.alpha_mode() == ::gltf::material::AlphaMode::Mask).then(|| m.alpha_cutoff().unwrap_or(0.5)),
            }
        })
        .collect();
    let mut model = Model { nodes: Vec::new(), materials, images };
    let roots: Vec<::gltf::Node> = match document.default_scene().or_else(|| document.scenes().next()) {
        Some(scene) => scene.nodes().collect(),
        None => {
            let children: std::collections::HashSet<usize> = document.nodes().flat_map(|n| n.children().map(|c| c.index()).collect::<Vec<_>>()).collect();
            document.nodes().filter(|n| !children.contains(&n.index())).collect()
        }
    };
    let mut stack: Vec<(::gltf::Node, Option<usize>)> = roots.into_iter().rev().map(|n| (n, None)).collect();
    while let Some((node, parent)) = stack.pop() {
        let local = Mat4::from_cols_array_2d(&node.transform().matrix());
        let world = parent.map_or(Mat4::IDENTITY, |p| model.nodes[p].world) * local;
        let mut entry = Node { name: node.name().unwrap_or("").to_string(), parent, local, world, vertices: Vec::new(), materials: Vec::new() };
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                read_primitive(&primitive, &buffers, world, &model.materials, &mut entry)?;
            }
        }
        let index = model.nodes.len();
        model.nodes.push(entry);
        let children: Vec<::gltf::Node> = node.children().collect();
        stack.extend(children.into_iter().rev().map(|c| (c, Some(index))));
    }
    Ok(model)
}

/// A primitive's triangles, into `node`, moved by `world`.
fn read_primitive(primitive: &::gltf::Primitive, buffers: &[Vec<u8>], world: Mat4, materials: &[Material], node: &mut Node) -> Result<(), String> {
    use ::gltf::mesh::Mode;
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|b| &b[..]));
    let Some(positions) = reader.read_positions() else { return Ok(()) };
    let positions: Vec<Vec3> = positions.map(Vec3::from).collect();
    let n = positions.len();
    let normals: Option<Vec<Vec3>> = reader.read_normals().map(|r| r.map(Vec3::from).collect());
    let colors: Vec<Vec4> = reader.read_colors(0).map_or_else(|| vec![Vec4::ONE; n], |r| r.into_rgba_f32().map(Vec4::from).collect());
    let material = primitive.material().index();
    let set = primitive.material().pbr_metallic_roughness().base_color_texture().map_or(0, |t| t.tex_coord());
    let uvs: Vec<Vec2> = reader.read_tex_coords(set).map_or_else(|| vec![Vec2::ZERO; n], |r| r.into_f32().map(Vec2::from).collect());
    let indices: Vec<u32> = reader.read_indices().map_or_else(|| (0..n as u32).collect(), |r| r.into_u32().collect());
    if indices.iter().any(|&i| i as usize >= n) || colors.len() < n || uvs.len() < n || normals.as_ref().is_some_and(|v| v.len() < n) {
        return Err("glTF: a primitive's attributes are shorter than its indices".to_string());
    }
    let triangles: Vec<[u32; 3]> = match primitive.mode() {
        Mode::Triangles => indices.as_chunks::<3>().0.iter().map(|t| [t[0], t[1], t[2]]).collect(),
        Mode::TriangleStrip => (2..indices.len()).map(|k| if k % 2 == 0 { [indices[k - 2], indices[k - 1], indices[k]] } else { [indices[k - 1], indices[k - 2], indices[k]] }).collect(),
        Mode::TriangleFan => (2..indices.len()).map(|k| [indices[0], indices[k - 1], indices[k]]).collect(),
        _ => return Ok(()),
    };
    let base = material.and_then(|m| materials.get(m)).map_or(Vec4::ONE, |m| m.base);
    let flip = world.determinant() < 0.0;
    let turn = Mat3::from_mat4(world).inverse().transpose();
    let first = node.vertices.len();
    for t in &triangles {
        let corners = if flip { [t[0], t[2], t[1]] } else { *t };
        for i in corners {
            let i = i as usize;
            let normal = normals.as_ref().map_or(Vec3::ZERO, |v| (turn * v[i]).normalize_or_zero());
            node.vertices.push(Vertex { position: world.transform_point3(positions[i]), normal, color: base * colors[i], uv: uvs[i] });
        }
        node.materials.push(material.unwrap_or(usize::MAX));
    }
    if normals.is_none() {
        smooth_normals(&mut node.vertices[first..]);
    }
    Ok(())
}

/// Normals for triangles that have none: at each corner the area-weighted average of the faces
/// that share its point and meet its own face at less than `CREASE_ANGLE`.
fn smooth_normals(vertices: &mut [Vertex]) {
    let faces: Vec<Vec3> = vertices.as_chunks::<3>().0.iter().map(|t| (t[1].position - t[0].position).cross(t[2].position - t[0].position)).collect();
    let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut around: HashMap<[u32; 3], Vec<usize>> = HashMap::new();
    for (k, v) in vertices.iter().enumerate() {
        around.entry(key(v.position)).or_default().push(k / 3);
    }
    let limit = crate::mesh::CREASE_ANGLE.cos();
    for k in 0..vertices.len() {
        let own = faces[k / 3];
        let mut sum = Vec3::ZERO;
        for &f in &around[&key(vertices[k].position)] {
            let other = faces[f];
            if f == k / 3 || other.dot(own) >= limit * other.length() * own.length() {
                sum += other;
            }
        }
        vertices[k].normal = if sum.length_squared() > 0.0 { sum.normalize() } else { own.normalize_or_zero() };
    }
}

/// The bytes of a `data:` URI in base64.
fn data_uri(uri: &str) -> Result<Vec<u8>, String> {
    let data = uri.strip_prefix("data:").and_then(|rest| rest.split_once(";base64,")).map(|(_, data)| data).ok_or_else(|| format!("glTF: {uri:.40}: only embedded data (a .glb, or data: URIs) is read"))?;
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let digits: Vec<u32> = data.bytes().filter(|&c| c != b'=' && !c.is_ascii_whitespace()).map(|c| value(c).ok_or("glTF: a data URI that is not base64")).collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (k, d)| n | d << (18 - 6 * k));
        out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    Ok(out)
}

/// A material to write: its name, base colour, metallic, roughness and emissive colour.
#[derive(Clone, Debug, PartialEq)]
pub struct WrittenMaterial {
    pub name: String,
    pub base: Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: Vec3,
}

/// Writes nodes of triangles as a `.glb`.
#[derive(Default)]
pub struct Writer {
    nodes: Vec<serde_json::Value>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
    meshes: Vec<serde_json::Value>,
    materials: Vec<WrittenMaterial>,
    accessors: Vec<serde_json::Value>,
    views: Vec<serde_json::Value>,
    bin: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    /// The index of a material, added if it is not there yet.
    pub fn material(&mut self, material: WrittenMaterial) -> usize {
        if let Some(k) = self.materials.iter().position(|m| *m == material) {
            return k;
        }
        self.materials.push(material);
        self.materials.len() - 1
    }

    /// Adds a node named `name` under `parent`, moved by `translation`, with `primitives`: runs of
    /// triangles (three vertices each, in the node's space) of one material each, `uv` whether
    /// their texture coordinates are written. Its index, for its children.
    pub fn node(&mut self, name: &str, parent: Option<usize>, translation: Vec3, primitives: &[(usize, Vec<Vertex>, bool)]) -> usize {
        let index = self.nodes.len();
        let mut node = serde_json::json!({ "name": name });
        if translation != Vec3::ZERO {
            node["translation"] = serde_json::json!(translation.to_array());
        }
        let written: Vec<serde_json::Value> = primitives.iter().filter(|(_, v, _)| !v.is_empty()).map(|(material, vertices, uv)| self.primitive(*material, vertices, *uv)).collect();
        if !written.is_empty() {
            node["mesh"] = serde_json::json!(self.meshes.len());
            self.meshes.push(serde_json::json!({ "name": name, "primitives": written }));
        }
        self.nodes.push(node);
        self.children.push(Vec::new());
        match parent {
            Some(p) => self.children[p].push(index),
            None => self.roots.push(index),
        }
        index
    }

    fn primitive(&mut self, material: usize, vertices: &[Vertex], uv: bool) -> serde_json::Value {
        let mut seen: HashMap<[u32; 11], u32> = HashMap::new();
        let mut unique: Vec<Vertex> = Vec::new();
        let indices: Vec<u32> = vertices
            .iter()
            .map(|v| {
                let key = [v.position.x, v.position.y, v.position.z, v.normal.x, v.normal.y, v.normal.z, v.color.x, v.color.y, v.color.z, v.uv.x, v.uv.y].map(f32::to_bits);
                *seen.entry(key).or_insert_with(|| {
                    unique.push(*v);
                    (unique.len() - 1) as u32
                })
            })
            .collect();
        let (lo, hi) = unique.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| (lo.min(v.position), hi.max(v.position)));
        let position = self.accessor(&unique.iter().flat_map(|v| v.position.to_array()).collect::<Vec<f32>>(), "VEC3", Some((lo, hi)));
        let normal = self.accessor(&unique.iter().flat_map(|v| v.normal.to_array()).collect::<Vec<f32>>(), "VEC3", None);
        let color = self.accessor(&unique.iter().flat_map(|v| v.color.truncate().to_array()).collect::<Vec<f32>>(), "VEC3", None);
        let mut attributes = serde_json::json!({ "POSITION": position, "NORMAL": normal, "COLOR_0": color });
        if uv {
            attributes["TEXCOORD_0"] = serde_json::json!(self.accessor(&unique.iter().flat_map(|v| v.uv.to_array()).collect::<Vec<f32>>(), "VEC2", None));
        }
        let view = self.view(bytemuck::cast_slice(&indices), Some(34963));
        self.accessors.push(serde_json::json!({ "bufferView": view, "componentType": 5125, "count": indices.len(), "type": "SCALAR" }));
        serde_json::json!({ "attributes": attributes, "indices": self.accessors.len() - 1, "material": material })
    }

    fn accessor(&mut self, floats: &[f32], kind: &str, bounds: Option<(Vec3, Vec3)>) -> usize {
        let width = if kind == "VEC2" { 2 } else { 3 };
        let view = self.view(bytemuck::cast_slice(floats), Some(34962));
        let mut accessor = serde_json::json!({ "bufferView": view, "componentType": 5126, "count": floats.len() / width, "type": kind });
        if let Some((lo, hi)) = bounds {
            accessor["min"] = serde_json::json!(lo.to_array());
            accessor["max"] = serde_json::json!(hi.to_array());
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut view = serde_json::json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() });
        if let Some(target) = target {
            view["target"] = serde_json::json!(target);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    /// The model as the bytes of a `.glb`.
    pub fn glb(mut self) -> Vec<u8> {
        for (node, children) in self.nodes.iter_mut().zip(&self.children) {
            if !children.is_empty() {
                node["children"] = serde_json::json!(children);
            }
        }
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let materials: Vec<serde_json::Value> = self
            .materials
            .iter()
            .map(|m| {
                let mut value = serde_json::json!({
                    "name": m.name,
                    "pbrMetallicRoughness": { "baseColorFactor": m.base.to_array(), "metallicFactor": m.metallic, "roughnessFactor": m.roughness },
                });
                if m.emissive != Vec3::ZERO {
                    value["emissiveFactor"] = serde_json::json!(m.emissive.to_array());
                }
                value
            })
            .collect();
        let mut json = serde_json::json!({
            "asset": { "version": "2.0", "generator": "Skylab (engine::gltf)" },
            "scene": 0,
            "scenes": [{ "nodes": self.roots }],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": materials,
            "accessors": self.accessors,
            "bufferViews": self.views,
        });
        if !self.bin.is_empty() {
            json["buffers"] = serde_json::json!([{ "byteLength": self.bin.len() }]);
        }
        let mut text = serde_json::to_vec(&json).expect("JSON of plain values");
        while !text.len().is_multiple_of(4) {
            text.push(b' ');
        }
        let total = 12 + 8 + text.len() + if self.bin.is_empty() { 0 } else { 8 + self.bin.len() };
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&text);
        if !self.bin.is_empty() {
            out.extend_from_slice(&(self.bin.len() as u32).to_le_bytes());
            out.extend_from_slice(b"BIN\0");
            out.extend_from_slice(&self.bin);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: f32, y: f32, z: f32, color: Vec4) -> Vertex {
        Vertex { position: Vec3::new(x, y, z), normal: Vec3::Z, color, uv: Vec2::new(x, y) }
    }

    /// A model written and read back has its nodes, their hierarchy and origins, its materials and
    /// every vertex as it was, in order.
    #[test]
    fn a_model_comes_back_as_it_was_written() {
        let mut w = Writer::new();
        let red = w.material(WrittenMaterial { name: "red".into(), base: Vec4::ONE, metallic: 0.0, roughness: 0.3, emissive: Vec3::ZERO });
        let chrome = w.material(WrittenMaterial { name: "chrome".into(), base: Vec4::ONE, metallic: 1.0, roughness: 0.05, emissive: Vec3::ZERO });
        let c = Vec4::new(0.8, 0.1, 0.05, 1.0);
        let square = vec![vertex(0.0, 0.0, 0.0, c), vertex(1.0, 0.0, 0.0, c), vertex(1.0, 1.0, 0.0, c), vertex(0.0, 0.0, 0.0, c), vertex(1.0, 1.0, 0.0, c), vertex(0.0, 1.0, 0.0, c)];
        let root = w.node("body", None, Vec3::ZERO, &[(red, square.clone(), false)]);
        let tri = vec![vertex(0.1, 0.2, 0.3, Vec4::ONE), vertex(0.4, 0.5, 0.6, Vec4::ONE), vertex(0.7, 0.8, 0.95, Vec4::ONE)];
        w.node("aileron", Some(root), Vec3::new(-1.0, 2.0, 0.5), &[(chrome, tri.clone(), true), (red, tri.clone(), false)]);
        w.node("light", Some(root), Vec3::new(0.0, 5.0, 0.0), &[]);
        let model = read(&w.glb()).unwrap();
        assert_eq!(model.nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["body", "aileron", "light"]);
        assert_eq!(model.nodes[1].parent, Some(0));
        assert_eq!(model.nodes[2].origin(), Vec3::new(0.0, 5.0, 0.0));
        assert_eq!(model.nodes[0].vertices.iter().map(|v| (v.position, v.color)).collect::<Vec<_>>(), square.iter().map(|v| (v.position, v.color)).collect::<Vec<_>>());
        let moved: Vec<Vec3> = tri.iter().map(|v| v.position + Vec3::new(-1.0, 2.0, 0.5)).collect();
        assert_eq!(model.nodes[1].vertices[..3].iter().map(|v| v.position).collect::<Vec<_>>(), moved);
        assert_eq!(model.nodes[1].vertices[1].uv, Vec2::new(0.4, 0.5));
        assert_eq!(model.nodes[1].materials, [chrome, red]);
        assert_eq!(model.materials[chrome].name, "chrome");
        // A metal's gloss is 2 more.
        let mesh = model.mesh(&model.nodes[1]);
        assert!((mesh.data[9] - (2.0 + 0.95)).abs() < 1e-6 && (mesh.data[39] - 0.7).abs() < 1e-6, "{} {}", mesh.data[9], mesh.data[39]);
        assert_eq!(mesh.count(), 6);
    }

    /// A mesh without normals is given smooth ones, sharp across a fold.
    #[test]
    fn normals_are_made_smooth_but_keep_folds() {
        // Two triangles meeting at 10° along x, and one folded down at 90°.
        let (a, b) = (Vec3::ZERO, Vec3::X);
        let mut vertices: Vec<Vertex> = [[a, b, Vec3::Y], [b, a, Vec3::new(0.0, -1.0, -0.17)], [a, b, Vec3::new(0.0, 0.0, -1.0)]]
            .iter()
            .flat_map(|t| t.iter().map(|&p| Vertex { position: p, normal: Vec3::ZERO, color: Vec4::ONE, uv: Vec2::ZERO }))
            .collect();
        smooth_normals(&mut vertices);
        let flat = (vertices[1].position - vertices[0].position).cross(vertices[2].position - vertices[0].position).normalize();
        assert!(vertices[0].normal.dot(flat) > 0.99 && vertices[0].normal.dot(flat) < 0.9999, "{}", vertices[0].normal);
        assert!((vertices[2].normal - flat).length() < 1e-6, "a corner of its own keeps its face's normal");
        let fold = (vertices[7].position - vertices[6].position).cross(vertices[8].position - vertices[6].position).normalize();
        assert!((vertices[6].normal - fold).length() < 1e-6, "{}", vertices[6].normal);
    }

    #[test]
    fn base64_data_uris_are_read() {
        assert_eq!(data_uri("data:application/octet-stream;base64,SGVsbG8sIGdsVEYh").unwrap(), b"Hello, glTF!");
        assert_eq!(data_uri("data:;base64,YWI=").unwrap(), b"ab");
        assert!(data_uri("model.bin").is_err());
    }

    #[test]
    fn a_texture_is_sampled_in_linear_light() {
        let image = Image { width: 2, height: 1, rgba: vec![255, 255, 255, 255, 0, 0, 0, 255] };
        assert!((image.sample(Vec2::new(0.25, 0.5)) - Vec4::ONE).length() < 1e-6);
        assert!(image.sample(Vec2::new(0.75, 0.5)).truncate().length() < 1e-6);
        let half = image.sample(Vec2::new(0.5, 0.5));
        assert!((half.x - 0.5).abs() < 1e-6, "{half}");
    }
}
