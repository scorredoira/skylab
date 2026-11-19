//! Textures from image files: one image or an array of same-sized images, with the whole mip
//! chain built on the CPU, plus the sampler and the bind group layout entries that go with them.
//!
//! ```ignore
//! let files: Vec<TextureSource> = names.iter().map(|n| TextureSource::file(format!("assets/{n}_color.jpg"))).collect();
//! let colour = gpu.texture_array("ground colour", &files, Kind::Color)?;
//! let layout = [texture::array_layout_entry(1), texture::sampler_layout_entry(2)];   // in a bind group layout
//! let entries = [colour.entry(1), wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) }];
//! ```
//!
//! In WGSL an array is `texture_2d_array<f32>` and is sampled with the layer as an integer:
//! `textureSample(colour, samp, uv, layer)`. One array and one bind group serve every material.

use crate::Gpu;
use std::path::{Path, PathBuf};

/// What the pixels mean. It decides the GPU format and how the smaller mip levels are averaged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Colour (albedo), `Rgba8UnormSrgb`: the shader reads linear values. Averaged in linear light,
    /// so a fine pattern of dark and bright keeps its brightness in the distance.
    Color,
    /// Tangent-space normal map, `Rg8Unorm`: x and y only. The shader rebuilds z
    /// (`sqrt(1 - x*x - y*y)` after mapping 0..1 to -1..1), so every mip level is unit length.
    Normal,
    /// A normal map with two more maps in its spare channels, `Rgba8Unorm`: rg = normal x and y,
    /// b and a = any linear data (roughness and ambient occlusion). One sample instead of three.
    NormalPacked,
    /// One linear channel (roughness, ambient occlusion, a mask), `R8Unorm`. Taken from red.
    Gray,
    /// Four linear channels, `Rgba8Unorm`.
    Data,
}

impl Kind {
    pub fn format(self) -> wgpu::TextureFormat {
        match self {
            Kind::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
            Kind::Normal => wgpu::TextureFormat::Rg8Unorm,
            Kind::NormalPacked | Kind::Data => wgpu::TextureFormat::Rgba8Unorm,
            Kind::Gray => wgpu::TextureFormat::R8Unorm,
        }
    }

    /// Bytes per texel on the GPU.
    pub fn channels(self) -> usize {
        match self {
            Kind::Gray => 1,
            Kind::Normal => 2,
            _ => 4,
        }
    }
}

/// Where the pixels of one image (or one layer of an array) come from.
#[derive(Clone, Debug)]
pub enum TextureSource {
    /// A PNG or JPEG file.
    File(PathBuf),
    /// Three files packed into one image, for [`Kind::NormalPacked`]: red and green from `rg` (a
    /// normal map), blue from the red channel of `b`, alpha from the red channel of `a`. A missing
    /// file leaves its channel at 1.
    Channels { rg: PathBuf, b: Option<PathBuf>, a: Option<PathBuf> },
    /// Pixels already in memory: RGBA, 8 bits each, row by row from the top.
    Pixels { width: u32, height: u32, rgba: Vec<u8> },
}

impl TextureSource {
    pub fn file(path: impl Into<PathBuf>) -> TextureSource {
        TextureSource::File(path.into())
    }
}

/// A file that could not be read, or images of different sizes in one array.
#[derive(Debug)]
pub struct TextureError(pub String);

impl std::fmt::Display for TextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TextureError {}

/// A texture on the GPU with all its mip levels: one image (`layers == 1`, a 2D view) or an array.
pub struct Texture {
    pub texture: wgpu::Texture,
    /// `D2` for [`Gpu::texture`], `D2Array` for [`Gpu::texture_array`].
    pub view: wgpu::TextureView,
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub mip_levels: u32,
    /// The average of each layer (its 1 x 1 mip level) as the shader reads it: colour in linear light,
    /// data as stored, channels the format lacks at 0.
    pub means: Vec<[f32; 4]>,
}

impl Texture {
    /// GPU memory it takes, in bytes, with every layer and mip level.
    pub fn bytes(&self) -> u64 {
        memory(self.width, self.height, self.layers, self.kind)
    }

    /// Its entry in a bind group, for a layout made with [`layout_entry`] or [`array_layout_entry`].
    pub fn entry(&self, binding: u32) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(&self.view) }
    }
}

/// Bytes of a texture of this size with its full mip chain.
pub fn memory(width: u32, height: u32, layers: u32, kind: Kind) -> u64 {
    let mut total = 0;
    for level in 0..mip_count(width, height) {
        let (w, h) = ((width >> level).max(1) as u64, (height >> level).max(1) as u64);
        total += w * h * kind.channels() as u64 * layers as u64;
    }
    total
}

/// Number of levels down to 1 x 1.
pub fn mip_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

fn entry(binding: u32, view_dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false },
        count: None,
    }
}

/// Bind group layout entry of a single texture, `texture_2d<f32>` in WGSL, for fragment shaders.
pub fn layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    entry(binding, wgpu::TextureViewDimension::D2)
}

/// Bind group layout entry of a texture array, `texture_2d_array<f32>` in WGSL, for fragment shaders.
pub fn array_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    entry(binding, wgpu::TextureViewDimension::D2Array)
}

/// Bind group layout entry of a volume, `texture_3d<f32>` in WGSL, for fragment shaders.
pub fn volume_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    entry(binding, wgpu::TextureViewDimension::D3)
}

/// Bind group layout entry of the sampler of [`Gpu::texture_sampler`], `sampler` in WGSL.
pub fn sampler_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }
}

// ---------------------------------------------------------------- pixels

struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn read(path: &Path) -> Result<Pixels, TextureError> {
    let failed = |e: &dyn std::fmt::Display| TextureError(format!("{}: {e}", path.display()));
    // By what the bytes are, not the name: the web build may keep a picture in another format.
    let image = image::load_from_memory(&crate::files::read(path).map_err(|e| failed(&e))?).map_err(|e| failed(&e))?.to_rgba8();
    Ok(Pixels { width: image.width(), height: image.height(), rgba: image.into_raw() })
}

fn load(source: &TextureSource) -> Result<Pixels, TextureError> {
    match source {
        TextureSource::File(path) => read(path),
        TextureSource::Pixels { width, height, rgba } => {
            if rgba.len() != *width as usize * *height as usize * 4 {
                return Err(TextureError(format!("{} bytes given for {width} x {height} RGBA pixels", rgba.len())));
            }
            Ok(Pixels { width: *width, height: *height, rgba: rgba.clone() })
        }
        TextureSource::Channels { rg, b, a } => {
            let mut out = read(rg)?;
            for (channel, path) in [(2, b), (3, a)] {
                let Some(path) = path else {
                    out.rgba.iter_mut().skip(channel).step_by(4).for_each(|v| *v = 255);
                    continue;
                };
                let extra = read(path)?;
                if (extra.width, extra.height) != (out.width, out.height) {
                    return Err(TextureError(format!("{} is {} x {}, but {} is {} x {}", path.display(), extra.width, extra.height, rg.display(), out.width, out.height)));
                }
                for (dst, src) in out.rgba.as_chunks_mut::<4>().0.iter_mut().zip(extra.rgba.as_chunks::<4>().0) {
                    dst[channel] = src[0];
                }
            }
            Ok(out)
        }
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// For each pixel of a row shrunk from `src` to `dst` pixels: the source pixels it covers and how
/// much of each. A box filter that is exact for any pair of sizes, not only for halving.
fn spans(src: u32, dst: u32) -> Vec<Vec<(usize, f32)>> {
    let ratio = src as f32 / dst as f32;
    (0..dst)
        .map(|i| {
            let (start, end) = (i as f32 * ratio, (i + 1) as f32 * ratio);
            let last = (end.ceil() as u32).clamp(1, src) - 1;
            (start.floor() as u32..=last).map(|s| (s as usize, ((s as f32 + 1.0).min(end) - (s as f32).max(start)).max(0.0) / ratio)).collect()
        })
        .collect()
}

/// Halves an image of four float channels with a box filter.
fn halve(src: &[f32], width: u32, height: u32) -> (Vec<f32>, u32, u32) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let (columns, rows) = (spans(width, w), spans(height, h));
    let mut out = vec![0.0; (w * h * 4) as usize];
    for (y, row) in rows.iter().enumerate() {
        for (x, column) in columns.iter().enumerate() {
            let mut sum = [0.0f32; 4];
            for &(sy, wy) in row {
                for &(sx, wx) in column {
                    let at = (sy * width as usize + sx) * 4;
                    for (s, v) in sum.iter_mut().zip(&src[at..at + 4]) {
                        *s += v * wx * wy;
                    }
                }
            }
            let at = (y * w as usize + x) * 4;
            out[at..at + 4].copy_from_slice(&sum);
        }
    }
    (out, w, h)
}

/// Every mip level of an RGBA image, largest first, as the bytes the GPU format of `kind` holds.
/// Colour is averaged in linear light; everything else as stored.
pub fn mip_chain(rgba: &[u8], width: u32, height: u32, kind: Kind) -> Vec<Vec<u8>> {
    let table: Vec<f32> = (0..256).map(|i| srgb_to_linear(i as f32 / 255.0)).collect();
    let decode = |channel: usize, v: u8| if kind == Kind::Color && channel < 3 { table[v as usize] } else { v as f32 / 255.0 };
    let encode = |channel: usize, v: f32| {
        let v = if kind == Kind::Color && channel < 3 { linear_to_srgb(v.clamp(0.0, 1.0)) } else { v.clamp(0.0, 1.0) };
        (v * 255.0 + 0.5) as u8
    };
    let pack = |level: &[f32]| -> Vec<u8> { level.as_chunks::<4>().0.iter().flat_map(|p| (0..kind.channels()).map(move |c| encode(c, p[c]))).collect() };

    let mut level: Vec<f32> = rgba.iter().enumerate().map(|(i, v)| decode(i % 4, *v)).collect();
    let (mut w, mut h) = (width, height);
    // The first level is the image itself, untouched by the round trip through floats.
    let mut chain = vec![rgba.as_chunks::<4>().0.iter().flat_map(|p| p[..kind.channels()].to_vec()).collect::<Vec<u8>>()];
    while w > 1 || h > 1 {
        (level, w, h) = halve(&level, w, h);
        chain.push(pack(&level));
    }
    chain
}

/// The values a shader reads from one texel of a mip level.
fn mean(texel: &[u8], kind: Kind) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (channel, v) in texel.iter().take(4).enumerate() {
        let v = *v as f32 / 255.0;
        out[channel] = if kind == Kind::Color && channel < 3 { srgb_to_linear(v) } else { v };
    }
    out
}

// ---------------------------------------------------------------- prepared off the GPU

/// Images read and their mip chains built, as the layers of one texture, for
/// [`Gpu::upload_layers`]: the slow part of making a texture, which needs no GPU and can run on
/// another thread.
pub struct PreparedLayers {
    kind: Kind,
    /// Each layer's width, height and mip levels, largest first.
    layers: Vec<(u32, u32, Vec<Vec<u8>>)>,
}

/// A PNG or JPEG file as RGBA, 8 bits a channel, row by row from the top: width, height, pixels.
/// For pictures a game composes into textures of its own.
pub fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), TextureError> {
    read(path).map(|p| (p.width, p.height, p.rgba))
}

/// `decode`, of a picture already in memory (one built into the program).
pub fn decode_bytes(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), TextureError> {
    let image = image::load_from_memory(bytes).map_err(|e| TextureError(e.to_string()))?.to_rgba8();
    Ok((image.width(), image.height(), image.into_raw()))
}

impl PreparedLayers {
    /// Layers whose mip chains were made elsewhere (by a filter of the game's own): each layer's
    /// width, height and levels, largest first, as the GPU format of `kind` holds them.
    pub fn from_levels(kind: Kind, layers: Vec<(u32, u32, Vec<Vec<u8>>)>) -> PreparedLayers {
        PreparedLayers { kind, layers }
    }
}

/// Reads `sources` and builds their mip chains as `kind` stores them, on every core (each thread
/// takes the next layer). `label` names them in errors.
pub fn prepare_layers(label: &str, sources: &[TextureSource], kind: Kind) -> Result<PreparedLayers, TextureError> {
    if sources.is_empty() {
        return Err(TextureError(format!("{label}: no images")));
    }
    // A 1024 x 1024 layer is 16 MB as floats while its mips are built.
    type Prepared = Result<(u32, u32, Vec<Vec<u8>>), TextureError>;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done: std::sync::Mutex<Vec<Option<Prepared>>> = std::sync::Mutex::new((0..sources.len()).map(|_| None).collect());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(sources.len());
    crate::threads::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let layer = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(source) = sources.get(layer) else { break };
                let prepared = load(source).map(|p| (p.width, p.height, mip_chain(&p.rgba, p.width, p.height, kind)));
                done.lock().unwrap_or_else(|e| e.into_inner())[layer] = Some(prepared);
            });
        }
    });
    let mut layers = Vec::with_capacity(sources.len());
    for prepared in done.into_inner().unwrap_or_else(|e| e.into_inner()) {
        layers.push(prepared.ok_or_else(|| TextureError(format!("{label}: a layer was not read")))??);
    }
    Ok(PreparedLayers { kind, layers })
}

// ---------------------------------------------------------------- GPU

impl Gpu {
    /// One image as a texture with its mip chain. `texture_2d<f32>` in WGSL.
    pub fn texture(&self, label: &str, source: &TextureSource, kind: Kind) -> Result<Texture, TextureError> {
        self.upload(label, std::slice::from_ref(source), kind, wgpu::TextureViewDimension::D2, 0)
    }

    /// Several images of the same size as the layers of one texture, in the order given.
    /// `texture_2d_array<f32>` in WGSL: the shader picks the layer, so one bind group serves every
    /// material.
    pub fn texture_array(&self, label: &str, sources: &[TextureSource], kind: Kind) -> Result<Texture, TextureError> {
        self.upload(label, sources, kind, wgpu::TextureViewDimension::D2Array, 0)
    }

    /// As [`Gpu::texture_array`], leaving out the `drop_levels` largest mip levels: 1 gives an
    /// array of half the side and a quarter of the memory, from the same files.
    pub fn texture_array_reduced(&self, label: &str, sources: &[TextureSource], kind: Kind, drop_levels: u32) -> Result<Texture, TextureError> {
        self.upload(label, sources, kind, wgpu::TextureViewDimension::D2Array, drop_levels)
    }

    /// As [`Gpu::texture_array_reduced`], from layers [`prepare_layers`] read and prepared on another
    /// thread: only the copy to the GPU is left.
    pub fn upload_layers(&self, label: &str, prepared: &PreparedLayers, drop_levels: u32) -> Result<Texture, TextureError> {
        self.upload_prepared(label, prepared, wgpu::TextureViewDimension::D2Array, drop_levels)
    }

    fn upload(&self, label: &str, sources: &[TextureSource], kind: Kind, dimension: wgpu::TextureViewDimension, drop_levels: u32) -> Result<Texture, TextureError> {
        self.upload_prepared(label, &prepare_layers(label, sources, kind)?, dimension, drop_levels)
    }

    fn upload_prepared(&self, label: &str, prepared: &PreparedLayers, dimension: wgpu::TextureViewDimension, drop_levels: u32) -> Result<Texture, TextureError> {
        let (layers, kind) = (&prepared.layers, prepared.kind);
        let (full_width, full_height) = (layers[0].0, layers[0].1);
        let drop_levels = drop_levels.min(mip_count(full_width, full_height) - 1);
        let (width, height) = ((full_width >> drop_levels).max(1), (full_height >> drop_levels).max(1));
        let count = layers.len() as u32;
        let limits = self.device.limits();
        if width.max(height) > limits.max_texture_dimension_2d || count > limits.max_texture_array_layers {
            return Err(TextureError(format!("{label}: {width} x {height} x {count} layers is more than this GPU takes")));
        }
        let mip_levels = mip_count(width, height);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: count },
            mip_level_count: mip_levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: kind.format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut means = Vec::with_capacity(layers.len());
        for (layer, (w, h, chain)) in layers.iter().enumerate() {
            if (*w, *h) != (full_width, full_height) {
                return Err(TextureError(format!("{label}: layer {layer} is {w} x {h}, the first one is {full_width} x {full_height}")));
            }
            for (level, data) in chain.iter().skip(drop_levels as usize).enumerate() {
                let (w, h) = ((width >> level).max(1), (height >> level).max(1));
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 }, aspect: wgpu::TextureAspect::All },
                    data,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * kind.channels() as u32), rows_per_image: Some(h) },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            }
            means.push(mean(chain.last().map_or(&[][..], |level| level), kind));
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor { label: Some(label), dimension: Some(dimension), ..Default::default() });
        Ok(Texture { texture, view, kind, width, height, layers: count, mip_levels, means })
    }

    /// A volume of `size` RGBA texels, linear data (`Kind::Data`), x fastest, no mip levels:
    /// `texture_3d<f32>` in WGSL. Sample it with coordinates kept half a texel inside its faces when
    /// the sampler repeats.
    pub fn volume(&self, label: &str, size: [u32; 3], rgba: &[u8]) -> Result<Texture, TextureError> {
        let [width, height, depth] = size;
        if rgba.len() != (width * height * depth * 4) as usize {
            return Err(TextureError(format!("{label}: {} bytes given for {width} x {height} x {depth} RGBA texels", rgba.len())));
        }
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: depth };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: Kind::Data.format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            extent,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor { label: Some(label), dimension: Some(wgpu::TextureViewDimension::D3), ..Default::default() });
        Ok(Texture { texture, view, kind: Kind::Data, width, height, layers: depth, mip_levels: 1, means: Vec::new() })
    }

    /// The sampler for tiling materials: repeats, blends between texels and between mip levels,
    /// and filters anisotropically (16 samples) so ground seen at a grazing angle stays sharp
    /// without shimmering. Where the GPU has no anisotropic filtering, wgpu drops it to plain
    /// trilinear by itself.
    pub fn texture_sampler(&self) -> wgpu::Sampler {
        self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tiling textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_is_averaged_in_linear_light() {
        // A black and white checker: half the light is sRGB 188, not the 128 a naive average gives.
        let rgba: Vec<u8> = (0..16).flat_map(|i| if (i % 4 + i / 4) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] }).collect();
        let chain = mip_chain(&rgba, 4, 4, Kind::Color);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0], rgba);
        assert_eq!(chain[2], [188, 188, 188, 255]);
        let data = mip_chain(&rgba, 4, 4, Kind::Data);
        assert_eq!(data[2], [128, 128, 128, 255]);
    }

    #[test]
    fn formats_keep_only_their_channels() {
        let rgba = [10, 20, 30, 40, 50, 60, 70, 80];
        assert_eq!(mip_chain(&rgba, 2, 1, Kind::Normal), [vec![10, 20, 50, 60], vec![30, 40]]);
        assert_eq!(mip_chain(&rgba, 2, 1, Kind::Gray), [vec![10, 50], vec![30]]);
        assert_eq!(mip_chain(&rgba, 2, 1, Kind::NormalPacked)[1], [30, 40, 50, 60]);
    }

    #[test]
    fn odd_sizes_lose_no_pixel() {
        // 5 wide to 2 wide: each output covers two and a half pixels.
        let weights = spans(5, 2);
        assert_eq!(weights[0].len(), 3);
        for span in &weights {
            assert!((span.iter().map(|(_, w)| w).sum::<f32>() - 1.0).abs() < 1e-6);
        }
        let rgba: Vec<u8> = [0, 0, 100, 200, 200].iter().flat_map(|v| [*v, 0, 0, 0]).collect();
        let chain = mip_chain(&rgba, 5, 1, Kind::Gray);
        assert_eq!(chain, [vec![0, 0, 100, 200, 200], vec![20, 180], vec![100]]);
    }

    #[test]
    fn memory_counts_every_level_and_layer() {
        assert_eq!(mip_count(1024, 1024), 11);
        assert_eq!(mip_count(5, 3), 3);
        assert_eq!(memory(4, 4, 1, Kind::Gray), 16 + 4 + 1);
        assert_eq!(memory(1024, 1024, 15, Kind::Color), 15 * 4 * 1_398_101);
    }

    #[test]
    fn channels_must_match_in_size() {
        let source = TextureSource::Pixels { width: 2, height: 2, rgba: vec![0; 12] };
        assert!(load(&source).is_err());
    }
}
