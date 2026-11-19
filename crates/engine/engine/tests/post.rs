//! The post-processing on a real device: a scene of one known light, drawn with `render_to_png`,
//! and the pixels of the picture read back from the PNG.

use engine::pipeline::{Depth, PipelineDesc};
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Game, Gpu, Input, Post};

const SHADER: &str = r#"
struct Light { colour: vec4<f32>, spot: vec4<f32> };
@group(0) @binding(0) var<uniform> light: Light;
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4(select(-1.0, 3.0, i == 1u), select(-1.0, 3.0, i == 2u), 0.0, 1.0);
}
// The whole frame at `colour`, but for a square of `spot.x` pixels in the middle at `spot.y`.
@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let d = abs(p.xy - vec2(f32(SIZE) * 0.5));
    if max(d.x, d.y) < light.spot.x * 0.5 {
        return vec4(vec3(light.spot.y), 1.0);
    }
    return vec4(light.colour.rgb, 1.0);
}
"#;
const SIZE: u32 = 128;
/// The light of the frame, by the number of frames drawn before it.
type Light = fn(usize) -> [f32; 8];

struct Scene {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    buffer: wgpu::Buffer,
    post: Post,
    light: Light,
    frames: usize,
}

impl Game for Scene {
    fn update(&mut self, _input: &Input, _dt: f64, gpu: &Gpu) {
        gpu.queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&(self.light)(self.frames)));
        self.frames += 1;
    }
    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
    fn post(&self) -> Post {
        self.post
    }
}

/// Draws `frames` frames of `light` with `post` and gives the picture as 8-bit RGBA rows.
fn picture(name: &str, post: Post, light: Light, frames: usize) -> image::RgbaImage {
    let path = format!("{}/post-{name}.png", env!("CARGO_TARGET_TMPDIR"));
    let make = |gpu: &Gpu| {
        let shader = gpu.shader("light", &SHADER.replace("SIZE", &SIZE.to_string()));
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &[0; 32], usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }] });
        let pipeline = gpu.pipeline(&PipelineDesc { label: "light", shader: &shader, vertex: "vertex", fragment: "fragment", buffers: &[], bind_groups: &[&layout], depth: Depth::Off, blend: false });
        Scene { pipeline, bind, buffer, post, light, frames: 0 }
    };
    engine::render_to_png(&path, SIZE, SIZE, 1.0, make, |_, n| n >= frames);
    image::open(&path).expect("the picture").to_rgba8()
}

fn grey(level: f32) -> [f32; 8] {
    [level, level, level, 1.0, 0.0, 0.0, 0.0, 0.0]
}

/// The red channel of a pixel.
fn at(image: &image::RgbaImage, x: u32, y: u32) -> i32 {
    image.get_pixel(x, y)[0] as i32
}

const FIXED: Post = Post { exposure: 0.0, adaptation: 0.0, spot: 0.0, glare: 0.0, bloom: 0.0, contrast: 0.0, saturation: 1.0, warmth: 0.0, vignette: 0.0, blur: 0.0, blur_centre: [0.5, 0.5], blur_above: 1.0, flare: 0.0, flare_sun: [0.5, 0.5], focus: 0.0, focus_span: 0.0, focus_blur: 0.0, near: 0.25 };

#[test]
fn mid_grey_is_the_middle_of_the_picture_and_exposure_is_in_stops() {
    // AgX puts mid grey (0.18) at half of the display's range.
    let mid = at(&picture("grey", FIXED, |_| grey(0.18), 1), 10, 10);
    assert!((mid - 128).abs() <= 3, "{mid}");
    // One stop more of exposure is the same as twice the light.
    let brighter = at(&picture("stop", Post { exposure: 1.0, ..FIXED }, |_| grey(0.18), 1), 10, 10);
    let twice = at(&picture("twice", FIXED, |_| grey(0.36), 1), 10, 10);
    assert!(brighter > mid + 20 && (brighter - twice).abs() <= 1, "{brighter} {twice}");
    // Far above 1 the light rolls off towards white without clipping at once.
    let (bright, brighter) = (at(&picture("bright", FIXED, |_| grey(4.0), 1), 10, 10), at(&picture("very bright", FIXED, |_| grey(8.0), 1), 10, 10));
    assert!(bright < brighter && brighter < 255, "{bright} {brighter}");
}

#[test]
fn the_exposure_adapts_to_the_scene_smoothly() {
    let adapting = Post { adaptation: 1.0, ..FIXED };
    // Fully adapted, a dark and a bright scene both come out mid grey; the first frame takes its exposure at once.
    let scenes: [(&str, Light); 2] = [("dark", |_| grey(0.01)), ("light", |_| grey(3.0))];
    for (name, level) in scenes {
        let mid = at(&picture(name, adapting, level, 1), 10, 10);
        assert!((mid - 128).abs() <= 4, "{name}: {mid}");
    }
    // The light drops eight times at frame 30: the frame after, the eye has barely opened up; two
    // seconds later it has made up most of the way to mid grey (three quarters of the stops).
    let drop: Light = |n| grey(if n < 30 { 0.18 } else { 0.0225 });
    let (just, later) = (at(&picture("drop", adapting, drop, 31), 10, 10), at(&picture("later", adapting, drop, 150), 10, 10));
    let unadapted = at(&picture("unadapted", FIXED, |_| grey(0.0225), 1), 10, 10);
    assert!((just - unadapted).abs() <= 3 && later > (just + 128) / 2 && later < 125, "{unadapted} {just} {later}");
}

#[test]
fn the_exposure_opens_for_a_dark_middle_and_not_for_a_bright_one() {
    // A square six stops under mid grey over most of the frame (the exposure is measured on a few
    // texels: a small one is lost in them), a rim of mid grey round it.
    let shade: Light = |_| [0.18, 0.18, 0.18, 1.0, 120.0, 0.0028, 0.0, 0.0];
    let (middle, rim) = (SIZE / 2, 1);
    let plain = picture("spot none", FIXED, shade, 1);
    let half = picture("spot half", Post { spot: 0.5, ..FIXED }, shade, 1);
    let whole = picture("spot whole", Post { spot: 1.0, ..FIXED }, shade, 1);
    // The middle is seen better the more the eye looks at it, and what is round it glares more.
    assert!(at(&plain, middle, middle) < at(&whole, middle, middle), "{} {}", at(&plain, middle, middle), at(&whole, middle, middle));
    assert!((at(&plain, rim, rim) - 128).abs() <= 3 && at(&plain, rim, rim) + 8 < at(&half, rim, rim) && at(&half, rim, rim) + 8 < at(&whole, rim, rim), "{} {} {}", at(&plain, rim, rim), at(&half, rim, rim), at(&whole, rim, rim));
    // Never by more than three stops, however dark what it looks at.
    let dark: Light = |_| grey(0.0028);
    let most = at(&picture("spot most", Post { spot: 1.0, ..FIXED }, dark, 1), middle, middle);
    let three = at(&picture("three stops", Post { exposure: 3.0, ..FIXED }, dark, 1), middle, middle);
    let none = at(&picture("no stops", FIXED, dark, 1), middle, middle);
    assert!((most - three).abs() <= 1 && most > none + 20, "{most} {three} {none}");
    // A middle a stop under mid grey, or brighter than the rest, leaves the exposure alone.
    for (name, level) in [("spot dim", 0.09), ("spot lamp", 2.88)] {
        let scene: Light = if level < 1.0 { |_| [0.18, 0.18, 0.18, 1.0, 120.0, 0.09, 0.0, 0.0] } else { |_| [0.18, 0.18, 0.18, 1.0, 120.0, 2.88, 0.0, 0.0] };
        let (without, with) = (picture(&format!("{name} none"), FIXED, scene, 1), picture(name, Post { spot: 1.0, ..FIXED }, scene, 1));
        assert_eq!(at(&without, rim, rim), at(&with, rim, rim), "{name}");
    }
}

#[test]
fn bloom_spreads_a_bright_light_and_only_when_asked() {
    // A square of 8 pixels at 200 times mid grey on black.
    let spot: Light = |_| [0.0, 0.0, 0.0, 1.0, 8.0, 36.0, 0.0, 0.0];
    let without = picture("no bloom", FIXED, spot, 1);
    let with = picture("bloom", Post { bloom: 0.03, ..FIXED }, spot, 1);
    let (centre, beside) = (SIZE / 2, SIZE / 2 + 12);
    assert!(at(&without, centre, centre) > 240 && at(&with, centre, centre) > 240);
    assert!(at(&without, beside, centre) <= 1, "{}", at(&without, beside, centre));
    // A glow beside it, fading with the distance.
    assert!(at(&with, beside, centre) > 8 && at(&with, beside, centre) > at(&with, beside + 20, centre), "{} {}", at(&with, beside, centre), at(&with, beside + 20, centre));
}

#[test]
fn contrast_keeps_mid_grey_and_spreads_the_rest_and_saturation_adds_colour() {
    let firm = Post { contrast: 0.4, ..FIXED };
    let mid = at(&picture("firm grey", firm, |_| grey(0.18), 1), 10, 10);
    assert!((mid - 128).abs() <= 3, "{mid}");
    let soft = (at(&picture("soft dark", FIXED, |_| grey(0.045), 1), 10, 10), at(&picture("soft light", FIXED, |_| grey(0.72), 1), 10, 10));
    let firm = (at(&picture("firm dark", firm, |_| grey(0.045), 1), 10, 10), at(&picture("firm light", firm, |_| grey(0.72), 1), 10, 10));
    assert!(firm.0 < soft.0 && firm.1 > soft.1, "{soft:?} {firm:?}");
    // A reddish grey: the gap between red and green grows with the saturation.
    let reddish: Light = |_| [0.3, 0.15, 0.15, 1.0, 0.0, 0.0, 0.0, 0.0];
    let gap = |post: Post, name: &str| {
        let p = picture(name, post, reddish, 1);
        let px = p.get_pixel(10, 10);
        px[0] as i32 - px[1] as i32
    };
    assert!(gap(Post { saturation: 1.3, ..FIXED }, "rich") > gap(FIXED, "plain") + 5);
}

#[test]
fn a_warmer_white_balance_reddens_a_grey_and_a_cooler_one_blues_it() {
    let of = |name: &str, warmth: f32| {
        let p = picture(name, Post { warmth, ..FIXED }, |_| grey(0.18), 1);
        let px = p.get_pixel(10, 10);
        (px[0] as i32, px[1] as i32, px[2] as i32)
    };
    let (plain, warm, cool) = (of("neutral", 0.0), of("warm", 0.5), of("cool", -0.5));
    assert!(plain.0 == plain.2, "{plain:?}");
    assert!(warm.0 > warm.2 + 10 && cool.2 > cool.0 + 10, "{warm:?} {cool:?}");
    // Its brightness stays about where it was.
    assert!((warm.1 - plain.1).abs() <= 6 && (cool.1 - plain.1).abs() <= 6, "{plain:?} {warm:?} {cool:?}");
}

#[test]
fn the_lens_darkens_the_corners_smears_towards_its_centre_and_blurs_what_is_out_of_focus() {
    // A vignette: the corner of an even grey darker than its middle, which stays mid grey.
    let v = picture("vignette", Post { vignette: 0.5, ..FIXED }, |_| grey(0.18), 1);
    let middle = at(&v, SIZE / 2, SIZE / 2);
    assert!((middle - 128).abs() <= 3 && at(&v, 2, 2) < middle - 20, "{middle} {}", at(&v, 2, 2));
    // A square of 8 pixels, bright on black, in the middle.
    let spot: Light = |_| [0.0, 0.0, 0.0, 1.0, 8.0, 4.0, 0.0, 0.0];
    let plain = picture("no lens", FIXED, spot, 1);
    // The blur of speed, round the top left corner: the square is smeared away from it, so a
    // pixel beyond the square shows some of it; the square's own middle stays lit.
    let smeared = picture("smeared", Post { blur: 0.3, blur_centre: [0.0, 0.0], ..FIXED }, spot, 1);
    let beyond = SIZE / 2 + 8;
    assert!(at(&plain, beyond, beyond) <= 1 && at(&smeared, beyond, beyond) > 10, "{} {}", at(&plain, beyond, beyond), at(&smeared, beyond, beyond));
    assert!(at(&smeared, SIZE / 2, SIZE / 2) > 60);
    // The depth of field: nothing here has a depth (all of it is infinitely far), so a lens focused
    // at 5 m blurs the whole picture: the square spreads beyond its edge. Not without the focus.
    let focused = picture("focused", Post { focus: 5.0, focus_span: 1.0, focus_blur: 0.05, ..FIXED }, spot, 1);
    let beside = SIZE / 2 + 7;
    assert!(at(&plain, beside, SIZE / 2) <= 1 && at(&focused, beside, SIZE / 2) > 5, "{} {}", at(&plain, beside, SIZE / 2), at(&focused, beside, SIZE / 2));
}

#[test]
fn the_tone_curve_rises_all_the_way_from_black_to_white_and_a_bright_colour_pales() {
    // From nothing to 6.5 stops above mid grey, a stop and a half at a time (the light of frame k
    // is level k): never down, and never a jump of a third of the range.
    let ramp: Light = |k| if k == 0 { grey(0.0) } else { grey(0.18 * 2f32.powf(-11.5 + 1.5 * k as f32)) };
    let levels: Vec<i32> = (0..=12).map(|k| at(&picture("ramp", FIXED, ramp, k + 1), 10, 10)).collect();
    assert!(levels[0] <= 1 && levels[12] >= 254, "{levels:?}");
    assert!(levels.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] < 85), "{levels:?}");
    // The curve is steepest at mid grey: the levels either side of it are further apart than
    // those at the ends.
    let (low, mid, high) = (levels[2] - levels[1], levels[8] - levels[7], levels[12] - levels[11]);
    assert!(mid > 2 * low && mid > 2 * high, "{levels:?}");
    // A red far brighter than the display can show goes towards white instead of staying a flat red.
    let red = picture("bright red", FIXED, |_| [40.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0], 1);
    let px = red.get_pixel(10, 10);
    assert!(px[0] >= 250 && px[1] > 120 && px[1] == px[2], "{px:?}");
}

#[test]
fn the_glow_is_round_and_a_blinding_point_does_not_flood_the_picture() {
    let spot: Light = |_| [0.0, 0.0, 0.0, 1.0, 8.0, 36.0, 0.0, 0.0];
    let glow = picture("round glow", Post { bloom: 0.03, ..FIXED }, spot, 1);
    let c = SIZE / 2;
    // As bright 20 pixels to the side as 20 above or below, and about as bright along a diagonal.
    let (side, above, below) = (at(&glow, c + 20, c), at(&glow, c, c - 20), at(&glow, c, c + 20));
    let diagonal = at(&glow, c + 14, c + 14);
    assert!(side > 3 && (side - above).abs() <= 2 && (side - below).abs() <= 2 && (side - diagonal).abs() <= side / 3 + 2, "{side} {above} {below} {diagonal}");
    // Four pixels a million times mid grey: their glow fades out before the corner of the picture.
    let blinding = picture("blinding", Post { bloom: 0.03, ..FIXED }, |_| [0.0, 0.0, 0.0, 1.0, 2.0, 180000.0, 0.0, 0.0], 1);
    assert!(at(&blinding, c + 6, c) > at(&blinding, 4, 4) + 10 && at(&blinding, 4, 4) < 128, "{} {}", at(&blinding, c + 6, c), at(&blinding, 4, 4));
}
