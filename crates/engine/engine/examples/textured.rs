//! Texture arrays in use: a ground of many materials and a few buildings, drawn with two arrays
//! (colour, and normal + roughness + occlusion packed) and one bind group.
//!
//! `cargo run -p engine --example textured -- out.png [grazing|above|objects|domes] [frames]` writes an image
//! (with `ENGINE_PROFILE=1` and a few hundred frames it also prints the time per frame);
//! without arguments it opens a window on the grazing view.
//!
//! What it shows, and what to copy from it:
//! - the material is an integer layer chosen per fragment (ground) or carried in the vertex (boxes);
//! - meshes have no texture coordinates: the ground is mapped from its world position, the boxes
//!   with triplanar mapping, each projection with its own tangent frame for the normal map;
//! - the ground blends a near scale with a far, rotated one, so it neither repeats visibly from
//!   the air nor blurs under the wheels;
//! - the last layer is a made-up map of domes: lit on the side of the sun, they prove the normal
//!   maps are read the right way round (OpenGL convention: green points up the image).

use engine::pipeline::{Depth, PipelineDesc};
use engine::texture::{self, Kind, TextureSource};
use engine::{wgpu, Camera, Game, Gpu, Input, Mesh, MeshBuilder};
use glam::{Vec3, Vec4};

/// The materials in `assets/textures`, in array order, with the metres one repeat covers.
const MATERIALS: [(&str, f32); 20] = [
    ("grass_short", 2.8),
    ("grass_meadow", 4.2),
    ("grass_dry", 5.0),
    ("soil_ploughed", 4.0),
    ("crop_stubble", 4.0),
    ("crop_green", 4.0),
    ("forest_floor", 4.0),
    ("rock_granite", 3.0),
    ("rock_layered", 4.0),
    ("scree", 6.0),
    ("snow", 4.0),
    ("sand", 3.6),
    ("asphalt", 4.0),
    ("concrete", 2.5),
    ("dirt_road", 4.0),
    ("metal_painted", 2.0),
    ("metal_corrugated", 2.7),
    ("plaster", 2.0),
    ("roof_clay", 2.5),
    ("roof_slate", 1.5),
];
/// The layer after the materials: the domes.
const DOMES: f32 = 20.0;

const SHADER: &str = r#"
struct Scene { view_proj: mat4x4<f32>, eye: vec4<f32>, sun: vec4<f32> };
@group(0) @binding(0) var<uniform> scene: Scene;
@group(0) @binding(1) var colour: texture_2d_array<f32>;
// rg = normal x and y (z is rebuilt), b = roughness, a = ambient occlusion.
@group(0) @binding(2) var detail: texture_2d_array<f32>;
@group(0) @binding(3) var samp: sampler;

const SKY = vec3<f32>(0.42, 0.56, 0.78);
const SUN = vec3<f32>(1.75, 1.65, 1.5);
// How many times bigger the far scale of the ground is, and its rotation (a 3-4-5 triangle).
const FAR = 9.0;
const ROT = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex fn vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>) -> Out {
    return Out(scene.view_proj * vec4(position - scene.eye.xyz, 1.0), position, normal, color);
}

struct Surface { color: vec3<f32>, normal: vec3<f32>, rough: f32, ao: f32 };

// Two samples: everything a material knows at `uv`. The normal is in tangent space.
fn fetch(uv: vec2<f32>, layer: i32) -> Surface {
    let c = textureSample(colour, samp, uv, layer).rgb;
    let d = textureSample(detail, samp, uv, layer);
    let xy = d.rg * 2.0 - 1.0;
    return Surface(c, vec3(xy, sqrt(max(1.0 - dot(xy, xy), 0.0))), d.b, d.a);
}

fn shade(s: Surface, world: vec3<f32>) -> vec4<f32> {
    let sun = scene.sun.xyz;
    let to_eye = scene.eye.xyz - world;
    let dist = length(to_eye);
    let v = to_eye / dist;
    let n = s.normal;
    let ndl = max(dot(n, sun), 0.0);
    let ambient = mix(vec3(0.16, 0.15, 0.13), SKY * 0.55, n.y * 0.5 + 0.5) * s.ao;
    var col = s.color * (SUN * ndl + ambient);
    // A highlight as wide as the surface is rough.
    let a = max(s.rough * s.rough, 0.03);
    let power = 2.0 / (a * a) - 2.0;
    col += SUN * pow(max(dot(n, normalize(sun + v)), 0.0), power) * (power + 2.0) / 8.0 * 0.04 * ndl;
    col = mix(col, SKY, 1.0 - exp(-dist / 9000.0));
    return vec4(col, 1.0);
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// Smooth noise, 0..1, one feature per unit.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

// Same as `fetch`, with the screen-space derivatives given: for coordinates that jump.
fn fetch_grad(uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>) -> Surface {
    let c = textureSampleGrad(colour, samp, uv, layer, dx, dy).rgb;
    let d = textureSampleGrad(detail, samp, uv, layer, dx, dy);
    let xy = d.rg * 2.0 - 1.0;
    return Surface(c, vec3(xy, sqrt(max(1.0 - dot(xy, xy), 0.0))), d.b, d.a);
}

// A material without visible repetition (after Inigo Quilez): the plane is divided by a slow
// noise into irregular patches, each reading the texture at its own offset, and neighbouring
// patches are blended. Two fetches instead of one.
fn fetch_scattered(uv: vec2<f32>, layer: i32) -> Surface {
    let k = noise(uv * 0.7) * 8.0;
    let i = floor(k);
    let off_a = sin(vec2(3.0, 7.0) * i);
    let off_b = sin(vec2(3.0, 7.0) * (i + 1.0));
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    let a = fetch_grad(uv + off_a, layer, dx, dy);
    let b = fetch_grad(uv + off_b, layer, dx, dy);
    let d = a.color - b.color;
    let t = smoothstep(0.2, 0.8, k - i - 0.1 * (d.r + d.g + d.b));
    return Surface(mix(a.color, b.color, t), mix(a.normal, b.normal, t), mix(a.rough, b.rough, t), mix(a.ao, b.ao, t));
}

// Which material the ground has at a point: a runway with its apron, a dirt road across, and a
// patchwork of every terrain material around them.
fn ground_layer(p: vec2<f32>) -> i32 {
    var layer = i32(hash(floor(p / 160.0)) * 11.999);
    layer = select(layer, 14, abs(p.y + 220.0) < 4.0);
    layer = select(layer, 13, p.x > 15.0 && p.x < 80.0 && abs(p.y) < 70.0);
    layer = select(layer, 12, abs(p.x) < 15.0 && abs(p.y) < 900.0);
    return layer;
}

@fragment fn ground(in: Out) -> @location(0) vec4<f32> {
    var tiles = TILES;
    let p = in.world.xz;
    let layer = ground_layer(p);
    let tile = tiles[layer];
    let dist = length(in.world - scene.eye.xyz);

    let near = fetch(p / tile, layer);
    let far = fetch_scattered(ROT * p / (tile * FAR) + 0.37, layer);
    // The smallest mip level is the average colour of the material.
    let mean = textureSampleLevel(colour, samp, vec2(0.5), layer, 16.0).rgb;

    // Close up: the near scale, tinted by the far one so no two repeats look alike.
    // Far away: the far scale alone, whose repeats are too big to count.
    let w = smoothstep(tile * 10.0, tile * 60.0, dist);
    var s: Surface;
    s.color = mix(near.color * far.color / max(mean, vec3(0.02)), far.color, w);
    s.rough = mix(near.rough, far.rough, w);
    s.ao = mix(near.ao, far.ao, w);
    // Tangent frames of the two mappings: u runs east (near) or along the rotated axis (far);
    // green points up the image, which is north (-z) for the near one.
    let n_near = vec3(near.normal.x, near.normal.z, -near.normal.y);
    let f = far.normal.x * vec2(0.8, -0.6) - far.normal.y * vec2(0.6, 0.8);
    let n_far = vec3(f.x, far.normal.z, f.y);
    s.normal = normalize(mix(n_near, n_far, w));
    return shade(s, in.world);
}

// One projection of triplanar mapping: `t` and `b` are the directions in which u grows and in
// which the image goes up, `n` the axis it is projected along.
fn side(p: vec3<f32>, t: vec3<f32>, b: vec3<f32>, n: vec3<f32>, layer: i32, tile: f32) -> Surface {
    var s = fetch(vec2(dot(p, t), -dot(p, b)) / tile, layer);
    s.normal = t * s.normal.x + b * s.normal.y + n * s.normal.z;
    return s;
}

@fragment fn object(in: Out) -> @location(0) vec4<f32> {
    var tiles = TILES;
    let layer = i32(in.color.a + 0.5);
    let tile = tiles[layer];
    var n = normalize(in.normal);
    if dot(n, scene.eye.xyz - in.world) < 0.0 {
        n = -n;
    }
    let sg = sign(n + vec3(1e-6));
    var w = pow(abs(n), vec3(6.0));
    w /= w.x + w.y + w.z;
    let p = in.world;
    let x = side(p, vec3(0.0, 0.0, -sg.x), vec3(0.0, 1.0, 0.0), vec3(sg.x, 0.0, 0.0), layer, tile);
    let y = side(p, vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0), vec3(0.0, sg.y, 0.0), layer, tile);
    let z = side(p, vec3(sg.z, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, sg.z), layer, tile);
    var s: Surface;
    s.color = (x.color * w.x + y.color * w.y + z.color * w.z) * in.color.rgb;
    s.rough = x.rough * w.x + y.rough * w.y + z.rough * w.z;
    s.ao = x.ao * w.x + y.ao * w.y + z.ao * w.z;
    // The mapped normals lean away from each axis; add the lean to the real normal.
    s.normal = normalize(n + (x.normal - vec3(sg.x, 0.0, 0.0)) * w.x + (y.normal - vec3(0.0, sg.y, 0.0)) * w.y + (z.normal - vec3(0.0, 0.0, sg.z)) * w.z);
    return shade(s, in.world);
}

@vertex fn sky_vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4(select(-1.0, 3.0, i == 1u), select(-1.0, 3.0, i == 2u), 0.0, 1.0);
}
@fragment fn sky() -> @location(0) vec4<f32> {
    return vec4(SKY, 1.0);
}
"#;

struct Textured {
    sky: wgpu::RenderPipeline,
    ground_pipeline: wgpu::RenderPipeline,
    object_pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    ground: Mesh,
    objects: Mesh,
    eye: Vec3,
    target: Vec3,
}

impl Game for Textured {
    fn update(&mut self, _input: &Input, _dt: f64, gpu: &Gpu) {
        let forward = (self.target - self.eye).normalize();
        let right = forward.cross(Vec3::Y).normalize();
        let camera = Camera { forward, right, up: right.cross(forward), ..Default::default() };
        let sun = Vec3::new(-0.75, 0.42, 0.25).normalize();
        let mut data = camera.view_proj(gpu.aspect()).to_cols_array().to_vec();
        data.extend_from_slice(&[self.eye.x, self.eye.y, self.eye.z, 0.0, sun.x, sun.y, sun.z, 0.0]);
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&data));
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&self.sky);
        pass.draw(0..3, 0..1);
        for (pipeline, mesh) in [(&self.ground_pipeline, &self.ground), (&self.object_pipeline, &self.objects)] {
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, mesh.buffer.slice(..));
            pass.draw(0..mesh.count, 0..1);
        }
    }
}

/// The made-up layer: a grid of domes, as a colour image and as a normal map (OpenGL convention)
/// with roughness in blue and occlusion in alpha.
fn domes(size: u32) -> (TextureSource, TextureSource) {
    let (mut color, mut detail) = (Vec::new(), Vec::new());
    let (cell, radius) = (size as f32 / 8.0, size as f32 / 8.0 * 0.4);
    for y in 0..size {
        for x in 0..size {
            let right = (x as f32 + 0.5) % cell - cell / 2.0;
            let up = cell / 2.0 - (y as f32 + 0.5) % cell;
            let inside = right * right + up * up < radius * radius;
            let n = if inside { Vec3::new(right, up, (radius * radius - right * right - up * up).sqrt()) / radius } else { Vec3::Z };
            color.extend_from_slice(if inside { &[205, 205, 200, 255] } else { &[120, 125, 135, 255] });
            detail.extend_from_slice(&[(n.x * 127.5 + 128.0) as u8, (n.y * 127.5 + 128.0) as u8, 150, 255]);
        }
    }
    (TextureSource::Pixels { width: size, height: size, rgba: color }, TextureSource::Pixels { width: size, height: size, rgba: detail })
}

/// A house: walls of one material, a gabled roof of another.
fn house(b: &mut MeshBuilder, at: Vec3, half: Vec3, wall: f32, roof: f32) {
    let white = |layer| Vec4::new(1.0, 1.0, 1.0, layer);
    b.cuboid(at - Vec3::new(half.x, 0.0, half.z), at + half, white(wall));
    let (top, ridge) = (at.y + half.y, at.y + half.y + half.x * 0.7);
    let corner = |sx: f32, sz: f32| Vec3::new(at.x + sx * (half.x + 0.4), top, at.z + sz * (half.z + 0.4));
    let peak = |sz: f32| Vec3::new(at.x, ridge, at.z + sz * (half.z + 0.4));
    b.quad(corner(-1.0, -1.0), corner(-1.0, 1.0), peak(1.0), peak(-1.0), white(roof));
    b.quad(corner(1.0, 1.0), corner(1.0, -1.0), peak(-1.0), peak(1.0), white(roof));
    for sz in [-1.0, 1.0] {
        b.triangle(corner(-1.0, sz), corner(1.0, sz), peak(sz), white(wall));
    }
}

fn make(gpu: &Gpu, view: &str) -> Textured {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/textures");
    let (dome_color, dome_detail) = domes(1024);
    let mut colours: Vec<TextureSource> = MATERIALS.iter().map(|(name, _)| TextureSource::file(dir.join(format!("{name}_color.jpg")))).collect();
    let mut details: Vec<TextureSource> = MATERIALS
        .iter()
        .map(|(name, _)| {
            let ao = dir.join(format!("{name}_ao.jpg"));
            TextureSource::Channels { rg: dir.join(format!("{name}_normal.png")), b: Some(dir.join(format!("{name}_rough.jpg"))), a: ao.exists().then_some(ao) }
        })
        .collect();
    colours.push(dome_color);
    details.push(dome_detail);
    let colour = gpu.texture_array("colour", &colours, Kind::Color).expect("colour textures");
    let detail = gpu.texture_array("normal, roughness, occlusion", &details, Kind::NormalPacked).expect("detail textures");
    let sampler = gpu.texture_sampler();
    for t in [&colour, &detail] {
        eprintln!("{:?}: {} layers of {} x {}, {} mip levels, {:.1} MB on the GPU", t.kind, t.layers, t.width, t.height, t.mip_levels, t.bytes() as f64 / 1048576.0);
    }

    let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            texture::array_layout_entry(1),
            texture::array_layout_entry(2),
            texture::sampler_layout_entry(3),
        ],
    });
    let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 96, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }, colour.entry(1), detail.entry(2), wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&sampler) }],
    });

    // The metres per repeat of every layer, as a constant of the shader; the domes repeat every 3 m.
    let tiles: Vec<String> = MATERIALS.iter().map(|(_, metres)| format!("{metres:?}")).chain(["3.0".to_string()]).collect();
    let source = format!("const TILES = array<f32, {}>({});\n{SHADER}", tiles.len(), tiles.join(", "));
    let shader = gpu.shader("textured", &source);
    let pipeline = |label, vertex, fragment, buffers: &[wgpu::VertexBufferLayout], depth| gpu.pipeline(&PipelineDesc { label, shader: &shader, vertex, fragment, buffers, bind_groups: &[&layout], depth, blend: false });
    let sky = pipeline("sky", "sky_vertex", "sky", &[], Depth::Off);
    let ground_pipeline = pipeline("ground", "vertex", "ground", &[MeshBuilder::layout()], Depth::Write);
    let object_pipeline = pipeline("objects", "vertex", "object", &[MeshBuilder::layout()], Depth::Write);

    let mut g = MeshBuilder::new();
    let e = 12000.0;
    g.quad(Vec3::new(-e, 0.0, -e), Vec3::new(-e, 0.0, e), Vec3::new(e, 0.0, e), Vec3::new(e, 0.0, -e), Vec4::ONE);

    let mut b = MeshBuilder::new();
    let layer = |l: f32| Vec4::new(1.0, 1.0, 1.0, l);
    house(&mut b, Vec3::new(-34.0, 0.0, -60.0), Vec3::new(4.5, 3.2, 6.0), 17.0, 18.0);
    house(&mut b, Vec3::new(-52.0, 0.0, -84.0), Vec3::new(4.0, 3.0, 5.0), 17.0, 19.0);
    // A hangar of corrugated metal, a painted metal box, two rocks, and the domes standing and lying.
    house(&mut b, Vec3::new(52.0, 0.0, -95.0), Vec3::new(11.0, 6.0, 14.0), 16.0, 16.0);
    b.cuboid(Vec3::new(24.0, 0.0, -44.0), Vec3::new(28.0, 3.0, -38.0), Vec4::new(0.95, 0.95, 1.0, 15.0));
    b.cuboid(Vec3::new(-30.0, 0.0, -30.0), Vec3::new(-24.0, 5.0, -25.0), layer(7.0));
    b.cuboid(Vec3::new(-46.0, 0.0, -40.0), Vec3::new(-40.0, 8.0, -34.0), layer(8.0));
    b.cuboid(Vec3::new(-22.0, 0.0, -42.0), Vec3::new(-16.0, 6.0, -41.5), layer(DOMES));
    b.cuboid(Vec3::new(-22.0, 0.0, -38.0), Vec3::new(-16.0, 0.3, -32.0), layer(DOMES));

    let (eye, target) = match view {
        "above" => (Vec3::new(-120.0, 420.0, 260.0), Vec3::new(10.0, 0.0, -160.0)),
        "domes" => (Vec3::new(-15.0, 4.5, -31.0), Vec3::new(-19.0, 2.0, -40.0)),
        "objects" => (Vec3::new(-6.0, 7.0, -8.0), Vec3::new(-30.0, 2.0, -48.0)),
        _ => (Vec3::new(3.0, 1.5, 40.0), Vec3::new(-4.0, 1.2, -100.0)),
    };
    Textured { sky, ground_pipeline, object_pipeline, bind, uniform, ground: g.build(&gpu.device), objects: b.build(&gpu.device), eye, target }
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next() {
        Some(path) => {
            let view = args.next().unwrap_or_default();
            let count: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(2);
            engine::render_to_png(&path, 1600, 900, 1.0, |gpu| make(gpu, &view), |_, frames| frames >= count);
        }
        None => engine::run("engine textures", 1280, 720, |gpu| make(gpu, "grazing")),
    }
}
