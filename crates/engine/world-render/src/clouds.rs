//! Ported from takram-design-engineering/three-geospatial (packages/clouds/src/CloudLayer.ts,
//! CloudLayers.ts, DensityProfile.ts, uniforms.ts, bayer.ts, CloudsPass.ts, ShadowPass.ts,
//! CloudsEffect.ts), MIT, see THIRD_PARTY_NOTICES.md.
//!
//! The volumetric clouds in the scene: their layers, the textures they are carved from, their
//! shadow map marched from the sun and averaged over the frames, the march of the view at a
//! sixteenth of the pixels between the scene's two passes, the pass that brings it to the scene's
//! size with the frames before and the one that puts it over the scene (`clouds.wgsl`,
//! `clouds_shadow.wgsl`, `clouds_draw.wgsl`). The lab (`examples/cloud_lab.rs`) draws the same
//! clouds alone.

use crate::cloud_noise::{self, Noise};
use crate::effect_pipelines::Target;
use crate::particles::depth_entry;
use crate::quality::{CloudMarch, Quality};
use engine::{wgpu, Gpu};
use glam::{DVec3, Vec3};

/// `struct Clouds` in clouds.wgsl, field by field.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CloudUniforms {
    /// xyz = towards the light, w = time, s.
    pub sun: [f32; 4],
    /// xy = how far the wind has carried the clouds (x, z), m, z = coverage, w = how far their shapes have churned, m.
    pub wind: [f32; 4],
    /// xy = the eye's place in the world (x, z), m, z = its altitude, w = storm towers.
    pub origin: [f32; 4],
    /// x = the view's near distance, y = frame, z = pixels of the scene to a texel of the march each way, w = the sun's light on the clouds, as a factor.
    pub more: [f32; 4],
    /// The picture of the shade on the ground: xy = its middle (x, z) in the world, z = half its side, m, w = the level it is drawn for, m.
    pub map: [f32; 4],
    /// The view of the frame before (`Seen`), and xyz = the eye now less the eye then, w = 1 when there is one.
    pub before_right: [f32; 4],
    pub before_up: [f32; 4],
    pub before_fwd: [f32; 4],
    pub moved: [f32; 4],
    /// A storm cloud each: xy = its centre (x, z), z = radius of its base, w = altitude of its top.
    pub towers: [[f32; 4]; 4],
    /// xy = the pixel of each block the march looks through this frame, z = how far a cloud is near, m (`NEAR_PIXELS`).
    pub jitter: [f32; 4],
    /// The layers (`Layers::pack`).
    pub layers: LayerUniforms,
    /// The shadow map: xy = its middle (x, z) at the level of the top of the shade, z = half its side, m, w = its side in texels.
    pub bsm: [f32; 4],
    /// xy = from a place of the map to the same place in the map before, in its picture; z = 1 when there is one.
    pub bsm_before: [f32; 4],
    /// x = the sky's light, y = the ground's, z = the haze, each as a factor; w = how much of the shadow map's depth shades the clouds themselves.
    pub light: [f32; 4],
    /// The way the shadow map is marched and how much shade there is (`shade`); `prepare` sets it.
    pub shade: [f32; 4],
    /// The rain under the clouds (`rain`).
    pub rain: [f32; 4],
}

/// The layers as the shaders have them: a layer to a component.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LayerUniforms {
    pub min_heights: [f32; 4],
    pub max_heights: [f32; 4],
    pub density_scales: [f32; 4],
    pub shape_amounts: [f32; 4],
    pub detail_amounts: [f32; 4],
    pub weather_exponents: [f32; 4],
    pub shape_biases: [f32; 4],
    pub coverage_widths: [f32; 4],
    pub profile_exp: [f32; 4],
    pub profile_exponent: [f32; 4],
    pub profile_linear: [f32; 4],
    pub profile_constant: [f32; 4],
    /// The gaps between the layers: xyz = where each starts and ends.
    pub interval_min: [f32; 4],
    pub interval_max: [f32; 4],
    /// x = the lowest base, y = the highest top, z = the top of the layers that shade, w = their base.
    pub heights: [f32; 4],
    pub shadow_mask: [f32; 4],
}

/// A layer of clouds. Its weather is a channel of the weather texture by its place among the four
/// (red, green, blue), and our storm towers for the fourth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    /// Its base and how tall it is, m.
    pub altitude: f32,
    pub height: f32,
    pub density_scale: f32,
    pub shape_amount: f32,
    pub shape_detail_amount: f32,
    pub weather_exponent: f32,
    /// How the layer rounds towards its top.
    pub shape_altering_bias: f32,
    pub coverage_filter_width: f32,
    /// The density by the height 0..1 in the layer: a·e^(b·h) + c·h + d, as (a, b, c, d).
    pub density_profile: [f32; 4],
    /// Whether it shades what is under it.
    pub shadow: bool,
}

impl Layer {
    /// A layer of no height: none.
    pub const NONE: Layer = Layer { altitude: 0.0, height: 0.0, density_scale: 0.2, shape_amount: 1.0, shape_detail_amount: 1.0, weather_exponent: 1.0, shape_altering_bias: 0.35, coverage_filter_width: 0.6, density_profile: [0.0, 0.0, 0.75, 0.25], shadow: false };
}

/// The four layers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layers(pub [Layer; 4]);

impl Layers {
    /// Theirs: low cumulus from 750 m, taller ones from 1000 m and cirrus at 7500 m.
    pub const DEFAULT: Layers = Layers([
        Layer { altitude: 750.0, height: 650.0, shadow: true, ..Layer::NONE },
        Layer { altitude: 1000.0, height: 1200.0, shadow: true, ..Layer::NONE },
        Layer { altitude: 7500.0, height: 500.0, density_scale: 0.003, shape_amount: 0.4, shape_detail_amount: 0.0, coverage_filter_width: 0.5, ..Layer::NONE },
        Layer::NONE,
    ]);

    /// The layers of a sky of ours: cumulus with their bases at `base` m and `thickness` m tall,
    /// taller ones among them, cirrus high above, and the storm clouds, as tall as `storm_top`,
    /// if there is one.
    pub fn of(base: f32, thickness: f32, storm_top: Option<f32>) -> Layers {
        let mut layers = Layers::DEFAULT;
        layers.0[0].altitude = base;
        layers.0[0].height = thickness;
        layers.0[1].altitude = base + 250.0;
        layers.0[1].height = thickness * 1200.0 / 650.0;
        layers.0[2].altitude = (base + 3500.0).max(7500.0);
        if let Some(top) = storm_top {
            layers.0[3] = Layer { altitude: base, height: (top - base).max(1000.0), density_scale: 0.25, shape_amount: 0.7, shape_altering_bias: 0.6, density_profile: [0.0, 0.0, 0.5, 0.5], shadow: true, ..Layer::NONE };
        }
        layers
    }

    /// The gaps between the layers, at most three: (where each starts, where it ends), zeros for none.
    fn intervals(&self) -> ([f32; 4], [f32; 4]) {
        // The bases (false) and the tops (true) in order of height, a base before a top at the same.
        let mut entries: Vec<(f32, bool)> = self.0.iter().flat_map(|l| [(l.altitude, false), (l.altitude + l.height, true)]).collect();
        entries.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let (mut low, mut high) = ([0.0; 4], [0.0; 4]);
        let (mut found, mut inside) = (0, 0);
        for i in 0..entries.len() {
            if inside == 0 && i > 0 && found < 3 {
                (low[found], high[found]) = (entries[i - 1].0, entries[i].0);
                found += 1;
            }
            inside += if entries[i].1 { -1 } else { 1 };
        }
        (low, high)
    }

    pub fn pack(&self) -> LayerUniforms {
        let each = |f: &dyn Fn(&Layer) -> f32| std::array::from_fn(|i| f(&self.0[i]));
        let (interval_min, interval_max) = self.intervals();
        let (mut low, mut high, mut shade_low, mut shade_high) = (f32::MAX, 0.0f32, f32::MAX, 0.0f32);
        for layer in self.0.iter().filter(|l| l.height > 0.0) {
            low = low.min(layer.altitude);
            high = high.max(layer.altitude + layer.height);
            if layer.shadow {
                shade_low = shade_low.min(layer.altitude);
                shade_high = shade_high.max(layer.altitude + layer.height);
            }
        }
        if low == f32::MAX {
            low = 0.0;
        }
        if shade_low == f32::MAX {
            shade_low = 0.0;
        }
        LayerUniforms {
            min_heights: each(&|l| l.altitude),
            max_heights: each(&|l| l.altitude + l.height),
            density_scales: each(&|l| l.density_scale),
            shape_amounts: each(&|l| l.shape_amount),
            detail_amounts: each(&|l| l.shape_detail_amount),
            weather_exponents: each(&|l| l.weather_exponent),
            shape_biases: each(&|l| l.shape_altering_bias),
            coverage_widths: each(&|l| l.coverage_filter_width),
            profile_exp: each(&|l| l.density_profile[0]),
            profile_exponent: each(&|l| l.density_profile[1]),
            profile_linear: each(&|l| l.density_profile[2]),
            profile_constant: each(&|l| l.density_profile[3]),
            interval_min,
            interval_max,
            heights: [low, high, shade_high, shade_low],
            shadow_mask: each(&|l| if l.shadow && l.height > 0.0 { 1.0 } else { 0.0 }),
        }
    }
}

/// Their coverage (0.3 a sky of scattered cumulus, 0.6 overcast) for a share of the sky of ours, 0..1.
pub fn coverage(cover: f64) -> f32 {
    if cover <= 0.0 {
        0.0
    } else {
        (0.08 + 0.5 * cover) as f32
    }
}

/// How much of the shadow map's depth shades the clouds themselves, by the height of the light (the
/// vertical part of its direction). Ours, by eye: with the sun low its light crosses so many clouds
/// on its way that all would be in the shade of the rest, grey; with less of it they take the gold.
pub fn own_shade(sun_y: f32) -> f32 {
    let t = ((sun_y - 0.05) / 0.3).clamp(0.0, 1.0);
    0.3 + 0.7 * t * t * (3.0 - 2.0 * t)
}

/// The sun's light on the clouds as a factor, by the height of the light. Ours, by eye: a low sun
/// reaches the clouds so dim through the air that the blue of the sky wins on them; with more of
/// it they are gold.
pub fn sun_gain(sun_y: f32) -> f32 {
    2.0 - (own_shade(sun_y) - 0.3) / 0.7
}

/// How much of the hour of the gold it is, 0..1, by the height of the sun (the vertical part of its
/// direction): all of it from the horizon to some 4 degrees up, none from 20 degrees up nor once
/// the sun is 6 degrees under the horizon. Ours, by eye: the picture is graded by it
/// (`WorldRenderer::post`); the clouds take their light by `cloud_low_sun` in `clouds.wgsl`.
pub fn golden_hour(sun_y: f32) -> f32 {
    let smooth = |t: f32| t.clamp(0.0, 1.0).powi(2) * (3.0 - 2.0 * t.clamp(0.0, 1.0));
    (1.0 - smooth((sun_y - 0.03) / 0.32)) * smooth((sun_y + 0.1) / 0.1)
}

/// The lowest the shadow map is marched from, as the vertical part of its direction (4 degrees).
/// Ours, by eye: the shade of a cloud under a sun lower than this would fall out of the map, and
/// the clouds and the air would be without shade and shafts for the last half hour of the day.
pub const SHADE_LOW: f32 = 0.07;

/// The way the shadow map is marched for a light towards `sun`: the light's, never lower than
/// `SHADE_LOW`; and how much shade there is, 0..1: it fades as the light sets.
pub fn shade(sun: Vec3) -> [f32; 4] {
    let level = sun.x.hypot(sun.z);
    let up = sun.y.max(SHADE_LOW);
    let flat = (1.0 - up * up).sqrt();
    let along = if level > 1e-6 { [sun.x / level * flat, sun.z / level * flat] } else { [0.0, 0.0] };
    let t = ((sun.y + 0.02) / 0.04).clamp(0.0, 1.0);
    [along[0], if level > 1e-6 { up } else { 1.0 }, along[1], t * t * (3.0 - 2.0 * t)]
}

/// The rain under the clouds, for `CloudUniforms::rain`: how far the wind (blowing from `wind_from`
/// degrees at `wind_speed` m/s) carries it as it falls, and how much of it there is under the
/// heavy cumulus and under the storm towers. Ours, by eye: it rains under every storm tower, a
/// little under the cumulus round them, and here and there from a sky that is all cloud.
pub fn rain(wind_from: f64, wind_speed: f64, cover: f64, storms: usize) -> [f32; 4] {
    // The cloud goes with the wind of its height and the drops fall into slower air: the curtain
    // trails behind the cloud. A drop falls at some 7 m/s.
    let lean = (0.06 + 0.3 * wind_speed / 7.0).min(0.5);
    let from = wind_from.to_radians();
    let closed = ((cover - 0.75) / 0.25).clamp(0.0, 1.0);
    let showers = if storms > 0 { 0.4 } else { 0.4 * closed };
    [(-from.sin() * lean) as f32, (from.cos() * lean) as f32, showers as f32, if storms > 0 { 1.0 } else { 0.0 }]
}

/// The places of the eye repeat every this many metres: a multiple of all the textures' tiles and
/// of the shadow map's texels, so that the clouds are the same and a float keeps the place to half a metre.
pub const WRAP: f64 = 4_032_000.0;

/// Half the side of the shadow map, m: its texels are 96 m at 1024 to a side.
pub const SHADOW_HALF: f64 = 49_152.0;

/// A cloud is near, for the pass that puts the frames together, when the eye's own movement takes
/// it more than this many pixels across the picture in a frame (`clouds_draw.wgsl`, `CLOUD_VEIL`).
const NEAR_PIXELS: f32 = 2.0;

/// The order in which the pixels of a block of four by four are marched, a frame each.
const BAYER: [u32; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// The pixel of its block the march looks through in a frame.
fn bayer_offset(frame: u64) -> [f32; 2] {
    let at = BAYER.iter().position(|v| *v as u64 == frame % 16).unwrap_or(0);
    [(at % 4) as f32, (at / 4) as f32]
}

/// A line `const NAME: type = value;` of a shader with another value.
fn set(source: &mut String, name: &str, value: String) {
    let start = source.find(&format!("const {name}:")).unwrap_or_else(|| panic!("the clouds' shader has no constant {name}"));
    let equals = start + source[start..].find('=').unwrap();
    let end = start + source[start..].find(';').unwrap();
    source.replace_range(equals + 1..end, &format!(" {value}"));
}

/// `clouds.wgsl` marched as a quality says.
pub fn marched(source: &str, march: &CloudMarch) -> String {
    let mut source = source.to_string();
    set(&mut source, "CLOUD_STEPS", march.steps.to_string());
    set(&mut source, "CLOUD_MIN_STEP", format!("{:?}", march.min_step));
    set(&mut source, "CLOUD_MAX_RAY", format!("{:?}", march.max_ray));
    set(&mut source, "CLOUD_MIN_DENSITY", format!("{:?}", march.least));
    set(&mut source, "CLOUD_MIN_EXTINCTION", format!("{:?}", march.least));
    set(&mut source, "CLOUD_MIN_TRANSMITTANCE", format!("{:?}", march.min_transmittance));
    set(&mut source, "CLOUD_SUN_STEPS", march.sun_steps.to_string());
    set(&mut source, "CLOUD_GROUND_STEPS", march.ground_steps.to_string());
    set(&mut source, "CLOUD_SHAPE_DETAIL", march.shape_detail.to_string());
    set(&mut source, "CLOUD_TURBULENCE", march.turbulence.to_string());
    set(&mut source, "CLOUD_SHAFT_STEPS", if march.shafts { "500".to_string() } else { "0".to_string() });
    source
}

/// The part of `clouds.wgsl` that says where there is cloud, without the march of the view.
fn density(source: &str) -> &str {
    source.split("// ---- the march").next().unwrap()
}

/// The shaders of the shadow map for a quality.
pub fn shadow_source(march: &CloudMarch) -> String {
    let mut source = density(&marched(include_str!("clouds.wgsl"), march)).to_string() + include_str!("clouds_shadow.wgsl");
    set(&mut source, "SHADOW_STEPS", march.shadow_steps.to_string());
    set(&mut source, "SHADOW_MIN_DENSITY", format!("{:?}", march.shadow_least));
    set(&mut source, "SHADOW_MIN_EXTINCTION", format!("{:?}", march.shadow_least));
    set(&mut source, "SHADOW_MIN_TRANSMITTANCE", format!("{:?}", march.shadow_min_transmittance));
    source
}

/// The world's shaders, the clouds and their drawing in the scene, for a quality and `samples` per pixel.
pub fn source(quality: &Quality, march: &CloudMarch, samples: u32) -> String {
    let source = crate::pipelines::shader_source(quality) + &marched(include_str!("clouds.wgsl"), march) + include_str!("clouds_draw.wgsl");
    if samples > 1 {
        source
    } else {
        source.replace("texture_depth_multisampled_2d", "texture_depth_2d")
    }
}

/// What depends on the scene's targets and the quality: the pipelines, the targets of the marches
/// and of their averages over the frames, and the bind groups. Those in pairs are by the frame's
/// parity: the average written in a frame is read in the next.
struct Made {
    targets: u64,
    samples: u32,
    march: CloudMarch,
    full: bool,
    shadow_march: wgpu::RenderPipeline,
    shadow_resolve: wgpu::RenderPipeline,
    shadow_ground: wgpu::RenderPipeline,
    shadow_march_bind: wgpu::BindGroup,
    shadow_resolve_bind: [wgpu::BindGroup; 2],
    shadow_ground_bind: [wgpu::BindGroup; 2],
    shadow_now: wgpu::TextureView,
    shadow_history: [wgpu::TextureView; 2],
    cloud_march: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    over: wgpu::RenderPipeline,
    march_bind: [wgpu::BindGroup; 2],
    resolve_bind: [wgpu::BindGroup; 2],
    over_bind: [wgpu::BindGroup; 2],
    low: wgpu::TextureView,
    /// How far the march's clouds are: this frame's and the one before's, by the frame's parity.
    far: [wgpu::TextureView; 2],
    history: [wgpu::TextureView; 2],
    /// How long each pixel of `history` has been in it.
    settled: [wgpu::TextureView; 2],
}

/// A view the clouds were drawn from: its axes as the frame's uniforms have them (right with
/// the tangent of half the field across, up with that of half the field up, forward with the
/// vertical shift) and the eye, in render axes.
#[derive(Clone, Copy)]
pub struct Seen {
    pub right: [f32; 4],
    pub up: [f32; 4],
    pub forward: [f32; 4],
    pub eye: DVec3,
}

/// The shadow map of a frame: its middle and the light it was marched along.
#[derive(Clone, Copy)]
struct Shadow {
    middle: [f64; 2],
    sun: Vec3,
}

pub struct Clouds {
    /// Pipelines made beforehand for the quality the frames are about to draw with.
    adopted: Option<CloudPipelines>,
    uniforms: wgpu::Buffer,
    noise: Noise,
    repeat: wgpu::Sampler,
    clamp: wgpu::Sampler,
    made: Option<Made>,
    /// Frames drawn, and the view and the shadow map of the last: the averages are carried from them.
    frames: u64,
    before: Option<Seen>,
    shadow_before: Option<Shadow>,
}

const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const SETTLED: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

impl Clouds {
    pub fn new(gpu: &Gpu) -> Clouds {
        let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("clouds"), size: size_of::<CloudUniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let clamp = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("cloud shadow"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Clouds { adopted: None, uniforms, noise: cloud_noise::textures(gpu), repeat: cloud_noise::sampler(gpu), clamp, made: None, frames: 0, before: None, shadow_before: None }
    }

    /// Before the frame: the uniforms (the frame, the view before and the shadow map are set
    /// here), and the pipelines and targets if the scene's or the quality changed.
    pub fn prepare(&mut self, gpu: &Gpu, frame_layout: &wgpu::BindGroupLayout, quality: &Quality, mut uniforms: CloudUniforms, seen: Seen) {
        let Some(march) = quality.cloud_march else { return };
        let target = Target::of(gpu);
        let kept = self.made.as_ref().is_some_and(|m| m.targets == gpu.targets_made() && m.samples == target.samples && m.march == march && m.full == quality.cloud_full);
        let scale: u32 = if quality.cloud_full { 1 } else { 4 };
        self.frames += 1;
        uniforms.more[1] = (self.frames % 4096) as f32;
        uniforms.more[2] = scale as f32;
        if scale > 1 {
            let [x, y] = bayer_offset(self.frames);
            uniforms.jitter = [x, y, 0.0, 0.0];
        }
        if let (Some(before), true) = (self.before, kept) {
            let moved = (seen.eye - before.eye).as_vec3();
            uniforms.before_right = before.right;
            uniforms.before_up = before.up;
            uniforms.before_fwd = before.forward;
            // A jump (another place, another view) starts the average again.
            uniforms.moved = [moved.x, moved.y, moved.z, if moved.length() < 2000.0 { 1.0 } else { 0.0 }];
            // What is this far crosses `NEAR_PIXELS` of the picture in the frame, at most.
            uniforms.jitter[2] = moved.length() * 0.5 * gpu.height as f32 / seen.up[3].max(1e-3) / NEAR_PIXELS;
        }
        self.before = Some(seen);

        // The shadow map: over the eye, moved towards the sun by half of what the shade of the top
        // falls away from it on the ground, at whole texels of the world so that it does not swim.
        // (Along `shade`, which is the light's way but for a light at the horizon.)
        uniforms.shade = shade(Vec3::new(uniforms.sun[0], uniforms.sun[1], uniforms.sun[2]));
        let sun = Vec3::new(uniforms.shade[0], uniforms.shade[1], uniforms.shade[2]);
        let texel = 2.0 * SHADOW_HALF / march.shadow_size as f64;
        let lean = (0.5 * uniforms.layers.heights[2] as f64 / sun.y as f64).min(0.5 * SHADOW_HALF);
        let level = sun.x.hypot(sun.z).max(1e-6) as f64;
        let middle = [uniforms.origin[0] as f64 + sun.x as f64 / level * lean, uniforms.origin[1] as f64 + sun.z as f64 / level * lean].map(|v| (v / texel).round() * texel);
        uniforms.bsm = [middle[0] as f32, middle[1] as f32, SHADOW_HALF as f32, march.shadow_size as f32];
        if let (Some(before), true) = (self.shadow_before, kept) {
            let shift = [(middle[0] - before.middle[0]) / (2.0 * SHADOW_HALF), (middle[1] - before.middle[1]) / (2.0 * SHADOW_HALF)];
            // The light a fifth of a degree elsewhere is another map.
            let same = before.sun.dot(sun) > 0.999_994 && shift[0].abs() < 1.0 && shift[1].abs() < 1.0;
            uniforms.bsm_before = [shift[0] as f32, shift[1] as f32, if same { 1.0 } else { 0.0 }, 0.0];
        }
        self.shadow_before = Some(Shadow { middle, sun });
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        if kept {
            return;
        }

        let d = &gpu.device;
        let pipelines = match self.adopted.take().filter(|p| p.quality == *quality && p.target == target) {
            Some(pipelines) => pipelines,
            None => match CloudPipelines::new(d, frame_layout, quality, target) {
                Some(pipelines) => pipelines,
                None => return,
            },
        };
        let CloudPipelines { shadow_layout, march_layout, picture_layout, shadow_march, shadow_resolve, shadow_ground, cloud_march, resolve, over, .. } = pipelines;
        let plane = |label, format, w: u32, h: u32| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
        };
        fn view(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) }
        }
        let none = plane("cloud none", COLOUR, 1, 1);
        let side = march.shadow_size;
        let shadow_now = plane("cloud shadow", COLOUR, side, side);
        let shadow_history = [plane("cloud shadow average", COLOUR, side, side), plane("cloud shadow average", COLOUR, side, side)];
        let shadow_bind = |now: &wgpu::TextureView, history: &wgpu::TextureView| {
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cloud shadow"),
                layout: &shadow_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                    view(1, &self.noise.weather),
                    view(2, &self.noise.shape),
                    view(3, &self.noise.detail),
                    view(4, &self.noise.turbulence),
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.repeat) },
                    view(6, &self.noise.blue),
                    view(7, now),
                    view(8, history),
                    wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::Sampler(&self.clamp) },
                ],
            })
        };

        let (w, h) = (gpu.width.div_ceil(scale), gpu.height.div_ceil(scale));
        let low = plane("cloud march", COLOUR, w, h);
        let far = [plane("cloud far", COLOUR, w, h), plane("cloud far", COLOUR, w, h)];
        let history = [plane("cloud picture", COLOUR, gpu.width, gpu.height), plane("cloud picture", COLOUR, gpu.width, gpu.height)];
        let settled = [plane("cloud picture's age", SETTLED, gpu.width, gpu.height), plane("cloud picture's age", SETTLED, gpu.width, gpu.height)];
        let march_bind = |shadow: &wgpu::TextureView| {
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cloud march"),
                layout: &march_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                    view(1, &self.noise.weather),
                    view(2, &self.noise.shape),
                    view(3, &self.noise.detail),
                    view(4, &self.noise.turbulence),
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.repeat) },
                    view(6, &self.noise.blue),
                    view(7, gpu.scene_depth()),
                    view(11, shadow),
                ],
            })
        };
        let bind = |low: &wgpu::TextureView, far: &wgpu::TextureView, history: &wgpu::TextureView, far_before: &wgpu::TextureView, settled: &wgpu::TextureView| {
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cloud picture"),
                layout: &picture_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() }, view(7, gpu.scene_depth()), view(8, low), view(9, far), view(10, history), view(12, far_before), view(13, settled)],
            })
        };
        self.made = Some(Made {
            targets: gpu.targets_made(),
            samples: target.samples,
            march,
            full: quality.cloud_full,
            shadow_march,
            shadow_resolve,
            shadow_ground,
            shadow_march_bind: shadow_bind(&none, &none),
            shadow_resolve_bind: [shadow_bind(&shadow_now, &shadow_history[1]), shadow_bind(&shadow_now, &shadow_history[0])],
            shadow_ground_bind: [shadow_bind(&none, &shadow_history[0]), shadow_bind(&none, &shadow_history[1])],
            cloud_march,
            resolve,
            over,
            march_bind: [march_bind(&shadow_history[0]), march_bind(&shadow_history[1])],
            resolve_bind: [bind(&low, &far[0], &history[1], &far[1], &settled[1]), bind(&low, &far[1], &history[0], &far[0], &settled[0])],
            over_bind: [bind(&none, &far[0], &history[0], &none, &none), bind(&none, &far[1], &history[1], &none, &none)],
            shadow_now,
            shadow_history,
            low,
            far,
            history,
            settled,
        });
    }

    /// Takes pipelines made beforehand, to be drawn with when the quality they are for comes.
    pub fn adopt(&mut self, pipelines: CloudPipelines) {
        self.adopted = Some(pipelines);
    }

    /// Between the scene's two passes (`Game::before_translucent`): marches the shadow map and
    /// averages it with the frames before, draws the clouds' shade on the ground into the second
    /// channel of `shade` (the height map), marches the view and brings it to the scene's size.
    pub fn march(&self, encoder: &mut wgpu::CommandEncoder, frame_bind: &wgpu::BindGroup, shade: &wgpu::TextureView) {
        let Some(made) = &self.made else { return };
        let which = (self.frames % 2) as usize;
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        // (label, targets, whether they are cleared, pipeline, the frame's group if it takes it, its own)
        type Pass<'a> = (&'a str, &'a [&'a wgpu::TextureView], wgpu::LoadOp<wgpu::Color>, &'a wgpu::RenderPipeline, bool, &'a wgpu::BindGroup);
        let passes: [Pass; 5] = [
            ("cloud shadow march", &[&made.shadow_now], clear, &made.shadow_march, false, &made.shadow_march_bind),
            ("cloud shadow average", &[&made.shadow_history[which]], clear, &made.shadow_resolve, false, &made.shadow_resolve_bind[which]),
            ("cloud shade", &[shade], wgpu::LoadOp::Load, &made.shadow_ground, false, &made.shadow_ground_bind[which]),
            ("cloud march", &[&made.low, &made.far[which]], clear, &made.cloud_march, true, &made.march_bind[which]),
            ("cloud upscale", &[&made.history[which], &made.settled[which]], clear, &made.resolve, true, &made.resolve_bind[which]),
        ];
        for (label, targets, load, pipeline, framed, bind) in passes {
            let attachments: Vec<_> = targets.iter().map(|view| Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })).collect();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: &attachments, depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
            pass.set_pipeline(pipeline);
            if framed {
                pass.set_bind_group(0, frame_bind, &[]);
                pass.set_bind_group(1, bind, &[]);
            } else {
                pass.set_bind_group(0, bind, &[]);
            }
            pass.draw(0..3, 0..1);
        }
    }

    /// Puts the clouds over the scene, in the second pass; group 0 must be bound.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(made) = &self.made else { return };
        pass.set_pipeline(&made.over);
        pass.set_bind_group(1, &made.over_bind[(self.frames % 2) as usize], &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The clouds' pipelines and layouts for a quality and what the scene draws into: the driver
/// compiles their shader (the world's and the clouds'), seconds the first time a build runs one,
/// so the renderer makes them on another thread when the quality changes (`Clouds::adopt`).
pub struct CloudPipelines {
    quality: Quality,
    target: Target,
    shadow_layout: wgpu::BindGroupLayout,
    march_layout: wgpu::BindGroupLayout,
    picture_layout: wgpu::BindGroupLayout,
    shadow_march: wgpu::RenderPipeline,
    shadow_resolve: wgpu::RenderPipeline,
    shadow_ground: wgpu::RenderPipeline,
    cloud_march: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    over: wgpu::RenderPipeline,
}

impl CloudPipelines {
    /// `None` for a quality without volumetric clouds.
    pub fn new(d: &wgpu::Device, frame_layout: &wgpu::BindGroupLayout, quality: &Quality, target: Target) -> Option<CloudPipelines> {
        let march = quality.cloud_march?;
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry { binding, visibility: fragment, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false }, count: None };
        let sampler = |binding| wgpu::BindGroupLayoutEntry { binding, visibility: fragment, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let (d2, d3) = (wgpu::TextureViewDimension::D2, wgpu::TextureViewDimension::D3);
        let uniform = wgpu::BindGroupLayoutEntry { binding: 0, visibility: fragment, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let plain = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        // The shadow map: the clouds alone, without the world's shaders.
        let shadow_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cloud shadow"),
            entries: &[uniform, texture(1, d2), texture(2, d3), texture(3, d3), texture(4, d2), sampler(5), texture(6, d2), texture(7, d2), texture(8, d2), sampler(9)],
        });
        let shadow_shader = engine::pipeline::shader_with_ending_loops(d, "cloud shadow", &shadow_source(&march));
        let shadow_pipeline_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("cloud shadow"), bind_group_layouts: &[Some(&shadow_layout)], immediate_size: 0 });
        let shadow_pass = |label, entry, target: wgpu::ColorTargetState| {
            d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&shadow_pipeline_layout),
                vertex: wgpu::VertexState { module: &shadow_shader, entry_point: Some("shadow_vertex"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState { module: &shadow_shader, entry_point: Some(entry), targets: &[Some(target)], compilation_options: Default::default() }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let shadow_march = shadow_pass("cloud shadow march", "shadow_march", plain(COLOUR).unwrap());
        let shadow_resolve = shadow_pass("cloud shadow average", "shadow_resolve", plain(COLOUR).unwrap());
        // The height map's second channel: the first is the heights.
        let shadow_ground = shadow_pass("cloud shade", "shadow_ground", wgpu::ColorTargetState { format: wgpu::TextureFormat::Rg16Float, blend: None, write_mask: wgpu::ColorWrites::GREEN });

        // The clouds in the scene: after the world's shaders, whose group 0 they read. The march
        // and the passes after it have a layout each: a stage may read 16 textures in WebGPU (more on
        // Metal, `engine::Gpu`), the frame's group has 10 and the march needs 7 more.
        let march_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cloud march"),
            entries: &[uniform, texture(1, d2), texture(2, d3), texture(3, d3), texture(4, d2), sampler(5), texture(6, d2), depth_entry(7, target.samples), texture(11, d2)],
        });
        let picture_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("cloud picture"), entries: &[uniform, depth_entry(7, target.samples), texture(8, d2), texture(9, d2), texture(10, d2), texture(12, d2), texture(13, d2)] });
        let shader = engine::pipeline::shader_with_ending_loops(d, "clouds", &source(quality, &march, target.samples));
        let march_pipeline_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("cloud march"), bind_group_layouts: &[Some(frame_layout), Some(&march_layout)], immediate_size: 0 });
        let pipeline_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("cloud picture"), bind_group_layouts: &[Some(frame_layout), Some(&picture_layout)], immediate_size: 0 });
        let vertex = wgpu::VertexState { module: &shader, entry_point: Some("cloud_volume_vertex"), buffers: &[], compilation_options: Default::default() };
        let pass = |label, entry, layout: &wgpu::PipelineLayout, targets: &[Option<wgpu::ColorTargetState>]| {
            d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: vertex.clone(),
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(entry), targets, compilation_options: Default::default() }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let cloud_march = pass("cloud march", "cloud_volume_march", &march_pipeline_layout, &[plain(COLOUR), plain(COLOUR)]);
        let resolve = pass("cloud upscale", "cloud_volume_resolve", &pipeline_layout, &[plain(COLOUR), plain(SETTLED)]);
        // The scene times what the clouds let through (the alpha), plus their light.
        let through = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::SrcAlpha, operation: wgpu::BlendOperation::Add };
        let over = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clouds over the scene"),
            layout: Some(&pipeline_layout),
            vertex: vertex.clone(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("cloud_volume_fragment"), targets: &[Some(wgpu::ColorTargetState { format: target.scene, blend: Some(wgpu::BlendState { color: through, alpha: through }), write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: target.depth, depth_write_enabled: Some(false), depth_compare: Some(wgpu::CompareFunction::Always), stencil: Default::default(), bias: Default::default() }),
            multisample: wgpu::MultisampleState { count: target.samples, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        Some(CloudPipelines { quality: *quality, target, shadow_layout, march_layout, picture_layout, shadow_march, shadow_resolve, shadow_ground, cloud_march, resolve, over })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gaps_between_the_layers() {
        // Theirs: 750..1400 and 1000..2200 overlap, the cirrus are apart, the fourth is at the ground.
        let (low, high) = Layers::DEFAULT.intervals();
        assert_eq!((low, high), ([0.0, 2200.0, 0.0, 0.0], [750.0, 7500.0, 0.0, 0.0]));
        let packed = Layers::DEFAULT.pack();
        assert_eq!(packed.heights, [750.0, 8000.0, 2200.0, 750.0]);
        assert_eq!(packed.shadow_mask, [1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_storm_is_a_fourth_layer_that_shades() {
        let packed = Layers::of(1200.0, 650.0, Some(10_000.0)).pack();
        assert_eq!(packed.heights, [1200.0, 10_000.0, 10_000.0, 1200.0]);
        assert_eq!(packed.shadow_mask, [1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn the_shade_is_never_cast_from_lower_than_four_degrees() {
        let high = Vec3::new(0.6, 0.8, 0.0);
        let cast = shade(high);
        assert!((Vec3::new(cast[0], cast[1], cast[2]) - high).length() < 1e-6 && cast[3] == 1.0);
        let low = Vec3::new(-0.6, 0.01, 0.8).normalize();
        let cast = shade(low);
        let way = Vec3::new(cast[0], cast[1], cast[2]);
        assert!((way.length() - 1.0).abs() < 1e-6 && way.y == SHADE_LOW);
        // The same bearing as the light.
        assert!((way.x / way.z - low.x / low.z).abs() < 1e-5 && way.x < 0.0);
        assert!(cast[3] > 0.5 && cast[3] < 1.0);
        assert_eq!(shade(Vec3::new(0.0, -0.05, 1.0).normalize())[3], 0.0);
    }

    #[test]
    fn the_hour_of_the_gold_is_round_the_sunset() {
        assert_eq!(golden_hour(0.8), 0.0);
        assert_eq!(golden_hour(0.02), 1.0);
        assert_eq!(golden_hour(-0.2), 0.0);
        assert!(golden_hour(0.15) > 0.3 && golden_hour(0.15) < golden_hour(0.08));
    }

    #[test]
    fn it_rains_under_storms_and_a_closed_sky_and_leans_downwind() {
        assert_eq!(rain(250.0, 3.0, 0.55, 0)[2..], [0.0, 0.0]);
        assert_eq!(rain(250.0, 7.0, 0.7, 3)[2..], [0.4, 1.0]);
        assert_eq!(rain(250.0, 5.0, 1.0, 0)[2..], [0.4, 0.0]);
        // A wind from the west: the cloud a drop fell from is east of it (+x), by more in more wind.
        let (calm, windy) = (rain(270.0, 1.0, 1.0, 0), rain(270.0, 12.0, 1.0, 0));
        assert!(calm[0] > 0.0 && windy[0] > calm[0] && windy[0] <= 0.5 && calm[1].abs() < 1e-6);
    }

    #[test]
    fn every_pixel_of_a_block_is_marched_once_in_sixteen_frames() {
        let mut seen = [false; 16];
        for frame in 0..16 {
            let [x, y] = bayer_offset(frame);
            seen[(y * 4.0 + x) as usize] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn the_shaders_take_every_quality() {
        for march in [CloudMarch::LOW, CloudMarch::MEDIUM, CloudMarch::HIGH, CloudMarch::ULTRA] {
            let source = shadow_source(&march);
            assert!(source.contains(&format!("const SHADOW_STEPS: i32 = {};", march.shadow_steps)));
            assert!(!source.contains("fn cloud_march("));
            let source = marched(include_str!("clouds.wgsl"), &march);
            assert!(source.contains(&format!("const CLOUD_MIN_STEP: f32 = {:?};", march.min_step)));
        }
    }

    /// Every shader of the clouds is valid WGSL at every quality, for a scene of four samples and of one.
    #[test]
    fn the_shaders_are_valid() {
        use engine::wgpu::naga;
        let check = |label: &str, source: &str| {
            let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|error| panic!("{label}: {}", error.emit_to_string(source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap_or_else(|error| panic!("{label}: {error:?}"));
        };
        check("noise", include_str!("cloud_noise.wgsl"));
        for (quality, name) in [(Quality::LOW, "low"), (Quality::MEDIUM, "medium"), (Quality::HIGH, "high"), (Quality::ULTRA, "ultra")] {
            let march = quality.cloud_march.unwrap();
            check(&format!("{name} shadow"), &shadow_source(&march));
            check(&format!("{name} scene"), &source(&quality, &march, quality.samples));
        }
    }

    #[test]
    fn the_tiles_divide_the_wrap() {
        for tile in [96_000.0, 3360.0, 168.0, 4800.0] {
            assert_eq!(WRAP % tile, 0.0);
        }
        for size in [256.0, 512.0, 1024.0, 2048.0] {
            assert_eq!(WRAP % (2.0 * SHADOW_HALF / size), 0.0);
        }
    }
}
