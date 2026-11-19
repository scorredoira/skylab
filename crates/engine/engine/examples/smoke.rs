//! Smallest use of the engine: one mesh, one pipeline, some overlay text, rendered to a PNG or a window.
//! `cargo run -p engine --example smoke -- out.png` writes an image; without arguments it opens a window.

use engine::pipeline::{Depth, PipelineDesc};
use engine::{egui, wgpu, Camera, Game, Gpu, Input, Mesh, MeshBuilder};
use glam::{Vec3, Vec4};

const SHADER: &str = r#"
struct Scene { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> scene: Scene;
struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32>, @location(1) normal: vec3<f32> };
@vertex fn vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) color: vec4<f32>) -> Out {
    return Out(scene.view_proj * vec4(position, 1.0), color.rgb, normal);
}
@fragment fn fragment(in: Out) -> @location(0) vec4<f32> {
    let light = 0.3 + 0.7 * max(dot(normalize(in.normal), normalize(vec3(0.4, 0.8, 0.5))), 0.0);
    return vec4(in.color * light, 1.0);
}
"#;

struct Smoke { pipeline: wgpu::RenderPipeline, bind: wgpu::BindGroup, uniform: wgpu::Buffer, mesh: Mesh, angle: f32 }

impl Game for Smoke {
    fn update(&mut self, _input: &Input, dt: f64, gpu: &Gpu) {
        self.angle += dt as f32 * 0.6;
        let dir = Vec3::new(self.angle.sin(), -0.35, -self.angle.cos()).normalize();
        let right = dir.cross(Vec3::Y).normalize();
        let camera = Camera { forward: dir, right, up: right.cross(dir), ..Default::default() };
        // The mesh sits 6 m in front of where the camera started; the camera orbits by moving the mesh instead.
        let m = camera.view_proj(gpu.aspect()) * glam::Mat4::from_translation(dir * 6.0);
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&m.to_cols_array()));
    }
    fn ui(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::unique("label")).fixed_pos(egui::pos2(12.0, 12.0)).show(ctx, |ui| {
            ui.label(egui::RichText::new("engine: mesh, camera and text").size(18.0).color(egui::Color32::WHITE));
        });
    }
    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_vertex_buffer(0, self.mesh.buffer.slice(..));
        pass.draw(0..self.mesh.count, 0..1);
    }
}

fn make(gpu: &Gpu) -> Smoke {
    let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
    });
    let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }] });
    let shader = gpu.shader("smoke", SHADER);
    let pipeline = gpu.pipeline(&PipelineDesc { label: "smoke", shader: &shader, vertex: "vertex", fragment: "fragment", buffers: &[MeshBuilder::layout()], bind_groups: &[&layout], depth: Depth::Write, blend: false });
    let mut b = MeshBuilder::new();
    b.cuboid(Vec3::splat(-1.0), Vec3::splat(1.0), Vec4::new(0.8, 0.5, 0.2, 0.0));
    b.cylinder(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 2.6, 0.0), 0.6, 0.0, 12, Vec4::new(0.2, 0.6, 0.3, 0.0));
    Smoke { pipeline, bind, uniform, mesh: b.build(&gpu.device), angle: 0.5 }
}

fn main() {
    match std::env::args().nth(1) {
        Some(path) => { engine::render_to_png(&path, 800, 500, 2.0, make, |_, frames| frames >= 3); }
        None => engine::run("engine smoke test", 800, 500, make),
    }
}
