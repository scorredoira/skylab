//! Renders one of the models to a PNG, to check it by eye.
//! `cargo run -p flight --example models -- <model> <view> <out.png> [disc] [flap=30] [aileron=15] [elevator=-10] [rudder=10]`
//! Models: an aircraft's look by its folder in `assets/aircraft` (c172, extra300, fpv5), pole, windsock, light; the buildings: cottage, hip-house, corner-house, town-house, shed,
//! church, hangar, clubhouse, parked; and the trees (`world_render::vegetation::NAMES`): oak, beech,
//! ash, holm-oak, olive, poplar, cypress, spruce, fir, larch, stone-pine, pine, palm, maquis, shrub,
//! each also with `far-` before it for its simpler mesh; their cards are drawn as solid squares.
//! Views: side, front, rear, top, bottom, front-quarter, rear-quarter.
//! The aircraft's propeller disc hides the nose, so it is drawn only when `disc` is given. The control
//! surfaces turn about their hinges as the renderer turns them, by the angles given in degrees: flap
//! and elevator positive trailing edge down, aileron positive left one down, rudder positive to the right.

// The lists the renderer picks the trees from, and the aircraft's lights, are not used here.
#[allow(dead_code, unused_imports)]
#[path = "../src/models.rs"]
mod models;

use engine::pipeline::{Depth, PipelineDesc};
use engine::{wgpu, Camera, Game, Gpu, Input, Mesh, MeshBuilder};
use glam::{Mat4, Vec3};
use std::path::Path;

// Two-sided: the meshes are drawn without culling, so a normal facing away from the viewer is flipped.
const SHADER: &str = r#"
struct Scene { view_proj: mat4x4<f32>, eye: vec4<f32> };
@group(0) @binding(0) var<uniform> scene: Scene;
struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) world: vec3<f32> };
@vertex fn vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>) -> Out {
    return Out(scene.view_proj * vec4(position - scene.eye.xyz, 1.0), color.rgb, normal, position);
}
@fragment fn fragment(in: Out) -> @location(0) vec4<f32> {
    let to_eye = normalize(scene.eye.xyz - in.world);
    var n = normalize(in.normal);
    if dot(n, to_eye) < 0.0 { n = -n; }
    let light = 0.25 + 0.5 * max(dot(n, normalize(vec3(0.4, 0.8, 0.5))), 0.0) + 0.25 * max(dot(n, to_eye), 0.0);
    return vec4(in.color * light, 1.0);
}
"#;

struct Viewer {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    mesh: Mesh,
    /// Vertices to draw, from the start of the mesh.
    count: u32,
    /// Position of each vertex drawn, to frame the picture.
    points: Vec<Vec3>,
    centre: Vec3,
    /// From the model towards the camera.
    direction: Vec3,
}

impl Game for Viewer {
    fn update(&mut self, _input: &Input, _dt: f64, gpu: &Gpu) {
        let forward = -self.direction;
        let right = forward.cross(Vec3::Y).normalize();
        let camera = Camera { forward, right, up: right.cross(forward), fov_y: 20f32.to_radians(), ..Default::default() };
        let (tan_x, tan_y) = camera.tan_half_fov(gpu.aspect());
        // The nearest distance from which every vertex is inside the picture, plus a margin.
        let distance = self.points.iter().fold(0f32, |distance, &p| {
            let d = p - self.centre;
            let depth = d.dot(self.direction);
            distance.max(depth + d.dot(camera.right).abs() / tan_x).max(depth + d.dot(camera.up).abs() / tan_y)
        });
        let eye = self.centre + self.direction * (distance * 1.08);
        let mut data = camera.view_proj(gpu.aspect()).to_cols_array().to_vec();
        data.extend_from_slice(&[eye.x, eye.y, eye.z, 0.0]);
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&data));
    }
    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_vertex_buffer(0, self.mesh.buffer.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

/// Body axes (x forward, y right, z down) to a y-up frame with the nose towards +z: a proper rotation.
fn body_to_render(mesh: &mut MeshBuilder) {
    let rotation = Mat4::from_cols_array_2d(&[[0.0, 0.0, 1.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    for vertex in mesh.data.chunks_exact_mut(10) {
        for offset in [0, 3] {
            let p = rotation.transform_vector3(Vec3::new(vertex[offset], vertex[offset + 1], vertex[offset + 2]));
            vertex[offset..offset + 3].copy_from_slice(&p.to_array());
        }
    }
}

/// The folder of an aircraft's look in the repository.
fn folder(name: &str) -> String {
    format!("{}/../../../assets/aircraft/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// An aircraft's look read from its files (`assets/aircraft/<name>/exterior.*`).
fn aircraft(name: &str) -> (models::AircraftModel, models::Exterior) {
    let dir = folder(name);
    let exterior = models::Exterior::parse(&std::fs::read_to_string(format!("{dir}/exterior.ron")).expect("exterior.ron")).expect("exterior.ron reads");
    let glb = std::fs::read(format!("{dir}/exterior.glb")).expect("exterior.glb");
    (models::build(&exterior, &glb).expect("exterior.glb reads"), exterior)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(name), Some(view), Some(path)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("usage: models <model> <view> <out.png> [disc] [flap=deg] [aileron=deg] [elevator=deg] [rudder=deg]");
        std::process::exit(2);
    };
    let with_disc = args.iter().skip(3).any(|a| a == "disc");
    let angle = |name: &str| -> f32 {
        let prefix = format!("{name}=");
        args.iter().skip(3).find_map(|a| a.strip_prefix(&prefix)).map_or(0.0, |v| v.parse::<f32>().expect("an angle in degrees").to_radians())
    };
    let (flap, aileron, elevator, rudder) = (angle("flap"), angle("aileron"), angle("elevator"), angle("rudder"));

    let mut count = None;
    let mesh = match name.as_str() {
        "parked" => {
            let (model, exterior) = aircraft("c172");
            models::parked(&model, exterior.parked.as_ref()).mesh
        }
        name if Path::new(&folder(name)).join("exterior.glb").is_file() => {
            let (mut model, _) = aircraft(name);
            for part in &model.parts {
                println!("{:?}: first {} count {} hinge {}", part.moves, part.first, part.count, part.hinge);
                if part.blur() && !with_disc && count.is_none() {
                    count = Some(part.first);
                }
                use models::Surface::*;
                let models::Moves::Surface { by, scale } = part.moves else { continue };
                let angle = match by {
                    FlapLeft | FlapRight => flap,
                    AileronLeft | AileronRight => aileron,
                    Elevator => elevator,
                    Rudder => rudder,
                    Trim => 0.0,
                } * scale;
                // As `render::rotation`: about the hinge line.
                let turn = Mat4::from_translation(part.hinge) * Mat4::from_axis_angle(part.axis, angle) * Mat4::from_translation(-part.hinge);
                let range = part.first as usize * 10..(part.first + part.count) as usize * 10;
                for vertex in model.mesh.data[range].chunks_exact_mut(10) {
                    let p = turn.transform_point3(Vec3::new(vertex[0], vertex[1], vertex[2]));
                    let n = turn.transform_vector3(Vec3::new(vertex[3], vertex[4], vertex[5]));
                    vertex[..6].copy_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z]);
                }
            }
            body_to_render(&mut model.mesh);
            model.mesh
        }
        "cottage" => world_render::buildings::cottage().mesh,
        "hip-house" => world_render::buildings::hip_house().mesh,
        "corner-house" => world_render::buildings::corner_house().mesh,
        "town-house" => world_render::buildings::town_house().mesh,
        "shed" => world_render::buildings::shed().mesh,
        "church" => world_render::buildings::church().mesh,
        "hangar" => world_render::buildings::hangar().mesh,
        "clubhouse" => world_render::buildings::clubhouse().mesh,
        "pole" => world_render::models::pole(),
        "windsock" => world_render::models::windsock(),
        "light" => world_render::models::light(),
        other => match world_render::vegetation::named(other) {
            Some(tree) => tree.plain(),
            None => {
                eprintln!("unknown model {other}");
                std::process::exit(2);
            }
        },
    };
    let direction = match view.as_str() {
        "side" => Vec3::new(1.0, 0.0, 0.0),
        "front" => Vec3::new(0.0, 0.08, 1.0),
        "rear" => Vec3::new(0.0, 0.25, -1.0),
        "top" => Vec3::new(0.0, 1.0, -0.02),
        "bottom" => Vec3::new(0.0, -1.0, -0.02),
        "front-quarter" => Vec3::new(0.7, 0.35, 1.0),
        "rear-quarter" => Vec3::new(-0.6, 0.35, -1.0),
        other => {
            eprintln!("unknown view {other}");
            std::process::exit(2);
        }
    }
    .normalize();
    println!("{name}: {} vertices", mesh.count());

    let count = count.unwrap_or(mesh.count());
    let points: Vec<Vec3> = mesh.data[..count as usize * 10].chunks_exact(10).map(|vertex| Vec3::new(vertex[0], vertex[1], vertex[2])).collect();
    let low = points.iter().fold(Vec3::splat(f32::MAX), |low, &p| low.min(p));
    let high = points.iter().fold(Vec3::splat(f32::MIN), |high, &p| high.max(p));
    let centre = (low + high) / 2.0;

    engine::render_to_png(
        path,
        1000,
        700,
        1.0,
        |gpu| {
            let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                }],
            });
            let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 80, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }] });
            let shader = gpu.shader("models", SHADER);
            let pipeline = gpu.pipeline(&PipelineDesc { label: "models", shader: &shader, vertex: "vertex", fragment: "fragment", buffers: &[MeshBuilder::layout()], bind_groups: &[&layout], depth: Depth::Write, blend: false });
            Viewer { pipeline, bind, uniform, mesh: mesh.build(&gpu.device), count, points, centre, direction }
        },
        |_, frames| frames >= 2,
    );
}
