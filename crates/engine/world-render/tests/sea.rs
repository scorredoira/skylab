//! The sea's light and surf as the shader works them out, on a real device: the functions are
//! taken from the shader's own source (`glitter`, `hidden`, `mirror_share`, `surf` and what they
//! call) and drawn as rows of a small picture, one value per pixel, which the tests read back.
//! The picture goes through the tone mapping, which keeps the order of the values: the tests
//! compare pixels with each other, not with numbers.

use engine::pipeline::{Depth, PipelineDesc};
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Game, Gpu, Input, Post};
use world_render::{pipelines, Quality};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 64;

/// What the rows of the picture hold, from the top; `x` runs 0...1 along a row.
const ROWS: &str = r#"
struct Frame { sea: vec4<f32>, sea_more: vec4<f32>, test: vec4<f32> };
@group(0) @binding(0) var<uniform> frame: Frame;

@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4(select(-1.0, 3.0, i == 1u), select(-1.0, 3.0, i == 2u), 0.0, 1.0);
}

// The glitter of a sun 30 degrees up, ahead, for an eye looking down at the water at x * 90 degrees.
fn glitter_at(x: f32, variance: f32) -> f32 {
    let sun = vec3<f32>(0.0, 0.5, 0.8660254);
    let a = x * 1.5707963;
    let v = vec3<f32>(0.0, -sin(a), cos(a));
    return glitter(sun, v, vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(variance, variance, 0.0));
}

@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let x = p.x / f32(WIDTH);
    let row = i32(p.y) / 4;
    var v = 0.0;
    switch row {
        // Nearly smooth water and rough water; the rough again, thirty times brighter, to see its faint skirts.
        case 0 { v = glitter_at(x, 0.0005) * 0.1; }
        case 1 { v = glitter_at(x, 0.04) * 0.1; }
        case 2 { v = glitter_at(x, 0.04) * 3.0; }
        case 3 { v = hidden(x, 0.02); }
        case 4 { v = mirror_share(x, 0.0); }
        case 5 { v = mirror_share(x, 0.3); }
        case 6 { v = 0.02; }
        // The surf along lines from 4 m of water (left) to the edge (right) up a bed rising 3 cm a
        // metre, 3 m apart along the shore; `test.x` is the pixel's size and `test.y` the time.
        default { v = surf(4.0 * (1.0 - x), vec2<f32>(1.0, 0.0), 0.03, vec2<f32>(4000.0 + x * 133.0, 3.0 * f32(row)), frame.test.x, frame.test.y); }
    }
    return vec4(vec3(v), 1.0);
}
"#;

/// A function of the shader's source, whole.
fn function<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source.find(&format!("\nfn {name}(")).unwrap_or_else(|| panic!("no fn {name} in the shader"));
    let end = start + source[start..].find("\n}\n").expect("the function's end") + 3;
    &source[start..end]
}

/// A constant of the shader's source: its line.
fn constant<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source.find(&format!("\nconst {name}")).unwrap_or_else(|| panic!("no const {name} in the shader")) + 1;
    &source[start..start + source[start..].find('\n').expect("the line's end")]
}

fn shader() -> String {
    let source = pipelines::shader_source(&Quality::HIGH);
    let mut shader = ROWS.replace("WIDTH", &WIDTH.to_string());
    for name in ["PI", "WRAP_I", "WATER_FACING", "BREAKS_AT", "FOAM_LASTS", "FOAM_LEFT"] {
        shader.push_str(constant(&source, name));
        shader.push('\n');
    }
    for name in ["ihash", "vnoise", "hidden", "variance_towards", "glitter", "mirror_share", "breaking_depth", "bore", "surf"] {
        shader.push_str(function(&source, name));
    }
    shader
}

struct Rows {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
}

impl Game for Rows {
    fn update(&mut self, _input: &Input, _dt: f64, _gpu: &Gpu) {}
    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
    fn post(&self) -> Post {
        Post { adaptation: 0.0, glare: 0.0, bloom: 0.0, ..Post::default() }
    }
}

/// The rows drawn for a sea of this significant height, m, with waves of 8 s and the wind blowing
/// onshore, at pixels of `pixel` metres and the time `time`: the red of each row's pixels, by row.
fn rows(name: &str, height: f32, pixel: f32, time: f32) -> Vec<Vec<i32>> {
    let path = format!("{}/sea-{name}.png", env!("CARGO_TARGET_TMPDIR"));
    let frame: [f32; 12] = [8.0, 0.0, height, 0.0, 1.0, 0.0, 0.0, 8.0, pixel, time, 0.0, 0.0];
    let make = |gpu: &Gpu| {
        let shader = gpu.shader("sea rows", &shader());
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&frame), usage: wgpu::BufferUsages::UNIFORM });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }] });
        let pipeline = gpu.pipeline(&PipelineDesc { label: "sea rows", shader: &shader, vertex: "vertex", fragment: "fragment", buffers: &[], bind_groups: &[&layout], depth: Depth::Off, blend: false });
        Rows { pipeline, bind }
    };
    engine::render_to_png(&path, WIDTH, HEIGHT, 1.0, make, |_, frames| frames >= 1);
    let image = image::open(&path).expect("the picture").to_rgba8();
    (0..HEIGHT / 4).map(|row| (0..WIDTH).map(|x| image.get_pixel(x, row * 4 + 1)[0] as i32).collect()).collect()
}

fn brightest(row: &[i32]) -> usize {
    (0..row.len()).max_by_key(|&x| row[x]).unwrap()
}

#[test]
fn the_sun_glitters_where_the_water_mirrors_it_and_rougher_water_spreads_it() {
    let rows = rows("light", 1.0, 0.2, 0.0);
    let (smooth, rough, skirts) = (&rows[0], &rows[1], &rows[2]);
    // The sun is 30 degrees up: nearly smooth water mirrors it to an eye that looks 30 degrees
    // down, a third of the way along the row, and to no other.
    let (peak, mirror) = (brightest(smooth), WIDTH as usize / 3);
    assert!(peak.abs_diff(mirror) <= 6, "{peak}");
    assert!(smooth[peak] > 150 && smooth[mirror - 40] <= 1 && smooth[mirror + 40] <= 1, "{} {} {}", smooth[peak], smooth[mirror - 40], smooth[mirror + 40]);
    // Rough water: far less light there, more nearer the horizon (a facet seen at a low angle
    // mirrors more and looks larger), and light a long way from it.
    assert!(rough[mirror] < smooth[mirror] / 2 && rough[mirror - 40] > rough[mirror] + 5, "{} {} {}", rough[mirror], smooth[mirror], rough[mirror - 40]);
    assert!(skirts[mirror + 100] > 15 && skirts[WIDTH as usize - 1] < skirts[mirror + 100], "{} {}", skirts[mirror + 100], skirts[WIDTH as usize - 1]);
}

#[test]
fn rough_water_hides_itself_from_a_low_eye_and_mirrors_less_of_the_horizon() {
    let rows = rows("mirror", 1.0, 0.2, 0.0);
    // What hides the facets: more the lower the direction, nothing from high above.
    let hidden = &rows[3];
    assert!(hidden[4] > hidden[20] && hidden[20] > hidden[60] && hidden[60] > 0, "{} {} {}", hidden[4], hidden[20], hidden[60]);
    assert!(hidden[128..].iter().all(|&v| v == 0));
    // Flat water mirrors nearly all at a grazing angle and 2 % straight down, ever less between.
    let (flat, rough, facing) = (&rows[4], &rows[5], &rows[6]);
    assert!(flat.windows(2).all(|w| w[1] <= w[0] + 1), "{flat:?}");
    assert!((flat[WIDTH as usize - 1] - facing[0]).abs() <= 2 && flat[1] > facing[0] + 100, "{} {} {}", flat[1], flat[WIDTH as usize - 1], facing[0]);
    // Rough water mirrors less of what is low: its facets face the eye more squarely.
    assert!(rough[3] < flat[3] - 20 && (rough[WIDTH as usize - 1] - facing[0]).abs() <= 2, "{} {} {}", rough[3], flat[3], rough[WIDTH as usize - 1]);
}

#[test]
fn the_surf_breaks_in_the_shallows_as_fronts_that_come_in() {
    let lines = |name: &str, height: f32, pixel: f32, time: f32| rows(name, height, pixel, time).split_off(7);
    let now = lines("surf", 1.0, 0.2, 100.0);
    // Nothing where the water is too deep for the waves to break (the left third: over 2.6 m).
    assert!(now.iter().all(|line| line[..80].iter().all(|&v| v == 0)), "{:?}", now[0]);
    // In the shallows, fronts: white somewhere along every line, and open water between them.
    for line in &now {
        let shallows = &line[130..];
        assert!(shallows.iter().any(|&v| v > 150) && shallows.iter().any(|&v| v < 60), "{shallows:?}");
    }
    // A front is one line along the shore: where a line is whitest, its neighbour 3 m along is
    // whitest nearby.
    let fronts: Vec<usize> = now.iter().map(|line| 130 + brightest(&line[130..230])).collect();
    assert!(fronts.windows(2).filter(|w| w[0].abs_diff(w[1]) <= 12).count() >= fronts.len() * 2 / 3, "{fronts:?}");
    // Two seconds later they are nearer the shore (further right), or gone in and others have come.
    let later = lines("surf later", 1.0, 0.2, 102.0);
    assert!(now != later);
    let white = |lines: &[Vec<i32>]| lines.iter().flatten().map(|&v| v as i64).sum::<i64>();
    // A calm sea has no surf, and a lower sea less.
    assert_eq!(white(&lines("calm", 0.0, 0.2, 100.0)), 0);
    assert!(white(&lines("low", 0.4, 0.2, 100.0)) < white(&now) / 2);
    // From far (pixels of 60 m, more than the waves' length), the fronts are a smooth band, as white as they are on average near.
    let far = lines("surf far", 1.0, 60.0, 100.0);
    assert!(far.iter().all(|line| line.windows(2).all(|w| (w[0] - w[1]).abs() <= 12)), "{:?}", far[0]);
    let (near, far) = (white(&now) as f64, white(&far) as f64);
    assert!(far > near * 0.4 && far < near * 2.5, "{near} {far}");
}
