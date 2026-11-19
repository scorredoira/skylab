//! A real place's ground as the satellites saw it (`sim_core::Imagery`, Sentinel-2 by `geodata`):
//! its grids as the layers of one texture the ground's shader reads, the near one (10 m) and the
//! far one (32 m), sRGB with their smaller copies; a grid smaller than the texture fills a corner.
//! A new picture is made ready off the frame's thread (its smaller copies) and sent to the GPU a
//! slice at a time over some frames, into a texture of its own, so that no frame waits on it; the
//! one drawn is changed only when the new one is whole.
//!
//! When the quality draws the ground's detail (`Quality::ground_detail`), each grid is also cut
//! into its parcels (`parcels.rs`, a few seconds on a thread) and those are sent the same way, as
//! a second texture of two layers a grid (the parcels' map and their colours), drawn from when
//! they are whole; the picture is drawn without them meanwhile.

use crate::parcels::Parcels;
use engine::{wgpu, Gpu};
use sim_core::{Grid, Imagery, Terrain};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// Bytes sent to the GPU in one frame at most (some 1.5 ms of copying on an Apple M5 Pro).
const BUDGET: usize = 6 << 20;
/// The grids drawn: the near and the far.
pub const LEVELS: usize = 2;

/// One grid as the GPU takes it: its levels of detail, finest first (rows from the north), the
/// side in texels and where it lies.
struct Prepared {
    mips: Vec<Vec<u8>>,
    side: u32,
    /// Its middle (east, south in render axes) and the metres its side spans.
    centre: (f64, f64),
    extent: f64,
}

/// What a texture going to the GPU is made of: layers of levels of detail (finest first), or a
/// picture's parcels (each grid's map and colours, read where they are kept, not copied).
enum Layers {
    Mips(Vec<Vec<Vec<u8>>>),
    Parcels(Arc<Vec<Parcels>>),
}

impl Layers {
    fn count(&self) -> usize {
        match self {
            Layers::Mips(l) => l.len(),
            Layers::Parcels(p) => 2 * p.len(),
        }
    }

    fn mips(&self) -> usize {
        match self {
            Layers::Mips(l) => l.first().map_or(1, Vec::len),
            Layers::Parcels(_) => 1,
        }
    }

    fn bytes(&self, layer: usize, mip: usize) -> &[u8] {
        match self {
            Layers::Mips(l) => &l[layer][mip],
            Layers::Parcels(p) => {
                let grid = &p[layer / 2];
                bytemuck::cast_slice(if layer.is_multiple_of(2) { &grid.map } else { &grid.tone })
            }
        }
    }
}

/// A texture going to the GPU, and how far it has got (layer, level, row).
struct Sending {
    layers: Layers,
    texture: wgpu::Texture,
    at: (usize, usize, u32),
}

impl Sending {
    fn new(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, side: u32, layers: Layers) -> Sending {
        Sending { texture: texture(gpu, label, format, side, layers.count() as u32, layers.mips() as u32), layers, at: (0, 0, 0) }
    }

    /// Sends up to `budget` bytes more; whether it is whole.
    fn send(&mut self, gpu: &Gpu, budget: usize) -> bool {
        let side = self.texture.width();
        let mut sent = 0;
        while sent < budget {
            let (layer, mip, row) = self.at;
            if layer >= self.layers.count() {
                break;
            }
            let side = (side >> mip).max(1);
            let rows = (((budget - sent) / (side as usize * 4)).max(1) as u32).min(side - row);
            let start = (row * side * 4) as usize;
            let bytes = &self.layers.bytes(layer, mip)[start..start + (rows * side * 4) as usize];
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: mip as u32, origin: wgpu::Origin3d { x: 0, y: row, z: layer as u32 }, aspect: wgpu::TextureAspect::All },
                bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side * 4), rows_per_image: None },
                wgpu::Extent3d { width: side, height: rows, depth_or_array_layers: 1 },
            );
            sent += bytes.len();
            self.at = if row + rows < side {
                (layer, mip, row + rows)
            } else if mip + 1 < self.layers.mips() {
                (layer, mip + 1, 0)
            } else {
                (layer + 1, 0, 0)
            };
        }
        self.at.0 >= self.layers.count()
    }
}

/// A picture's parcels being made or sent, and those of the picture drawn.
enum Detail {
    None,
    Making(Receiver<Arc<Vec<Parcels>>>),
    /// Made before the picture they belong to was drawn: kept until it is.
    Made(Arc<Vec<Parcels>>),
    Sending(Sending),
    Drawn(Arc<Vec<Parcels>>),
}

/// Per grid: its middle (east, south) and side in metres, and whether there is one.
type Placing = [(f64, f64, f64, bool); LEVELS];

pub struct GroundImagery {
    /// The picture drawn, a layer a grid (texels of nothing when there is none), and its parcels
    /// (`Parcels::map` and `tone` of each grid, one texel of nothing when there are none), bound
    /// as group 2 of the terrain's pipelines.
    pub bind: wgpu::BindGroup,
    picture: wgpu::Texture,
    parcels: wgpu::Texture,
    /// Per grid: its middle (east, south) and side in metres, and whether there is one.
    placing: Placing,
    /// The picture given last, and the terrain it was given on (for what its maps say covers the ground).
    source: Option<(Arc<Imagery>, Arc<Terrain>)>,
    busy: Option<Receiver<Vec<Prepared>>>,
    /// A picture going to the GPU, and where its grids lie.
    sending: Option<(Sending, Placing)>,
    /// Whether the parcels are made and drawn (`Quality::ground_detail`).
    detailed: bool,
    detail: Detail,
}

fn texture(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, side: u32, layers: u32, mips: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: layers },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

const PICTURE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PARCELS: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl GroundImagery {
    /// No picture: the ground keeps its own colours.
    pub fn none(gpu: &Gpu, layout: &wgpu::BindGroupLayout, detailed: bool) -> GroundImagery {
        let picture = texture(gpu, "ground imagery", PICTURE, 1, LEVELS as u32, 1);
        let parcels = texture(gpu, "ground parcels", PARCELS, 1, 2 * LEVELS as u32, 1);
        GroundImagery { bind: bind(gpu, layout, &picture, &parcels), picture, parcels, placing: [(0.0, 0.0, 1.0, false); LEVELS], source: None, busy: None, sending: None, detailed, detail: Detail::None }
    }

    /// Starts making a picture ready, on the terrain it was made for; the one drawn stays until it
    /// is whole. `None` takes the picture away at once.
    pub fn set(&mut self, gpu: &Gpu, layout: &wgpu::BindGroupLayout, imagery: Option<(Arc<Imagery>, Arc<Terrain>)>) {
        self.sending = None;
        let Some((imagery, terrain)) = imagery else {
            *self = GroundImagery::none(gpu, layout, self.detailed);
            return;
        };
        let (sender, receiver) = channel();
        self.busy = Some(receiver);
        let picture = imagery.clone();
        engine::threads::spawn(move || {
            let side = side(&picture);
            let _ = sender.send(picture.levels.iter().take(LEVELS).map(|g| prepare(g, side)).collect());
        });
        self.source = Some((imagery, terrain));
        self.detail = Detail::None;
        self.make_detail();
    }

    /// Makes and draws the parcels of the picture, or stops drawing them.
    pub fn set_detailed(&mut self, gpu: &Gpu, layout: &wgpu::BindGroupLayout, detailed: bool) {
        if detailed == self.detailed {
            return;
        }
        self.detailed = detailed;
        if detailed {
            self.make_detail();
        } else {
            self.detail = Detail::None;
            self.parcels = texture(gpu, "ground parcels", PARCELS, 1, 2 * LEVELS as u32, 1);
            self.bind = bind(gpu, layout, &self.picture, &self.parcels);
        }
    }

    /// Starts cutting the picture given last into its parcels, on a thread.
    fn make_detail(&mut self) {
        let Some((imagery, terrain)) = self.source.clone().filter(|_| self.detailed) else { return };
        let (sender, receiver) = channel();
        engine::threads::spawn(move || {
            let side = side(&imagery) as usize;
            let _ = sender.send(Arc::new(imagery.levels.iter().take(LEVELS).map(|g| Parcels::new(g, side, terrain.region())).collect()));
        });
        self.detail = Detail::Making(receiver);
    }

    /// Whether a picture or its parcels are being made ready or sent.
    pub fn pending(&self) -> bool {
        self.busy.is_some() || self.sending.is_some() || matches!(self.detail, Detail::Making(_) | Detail::Made(_) | Detail::Sending(_))
    }

    /// The parcels drawn, for what stands on them (`parcels::trees`).
    pub fn parcels(&self) -> Option<&Arc<Vec<Parcels>>> {
        match &self.detail {
            Detail::Drawn(p) => Some(p),
            _ => None,
        }
    }

    /// Sends the next slice of a picture or its parcels to the GPU, and draws it once it is whole.
    pub fn update(&mut self, gpu: &Gpu, layout: &wgpu::BindGroupLayout) {
        if let Some(Ok(grids)) = self.busy.as_ref().map(|r| r.try_recv()) {
            self.busy = None;
            let side = grids.first().map_or(1, |g| g.side);
            let mut placing = [(0.0, 0.0, 1.0, false); LEVELS];
            for (i, g) in grids.iter().enumerate() {
                placing[i] = (g.centre.0, g.centre.1, g.extent, true);
            }
            self.sending = Some((Sending::new(gpu, "ground imagery", PICTURE, side, Layers::Mips(grids.into_iter().map(|g| g.mips).collect())), placing));
        }
        if let Detail::Making(receiver) = &self.detail {
            if let Ok(parcels) = receiver.try_recv() {
                self.detail = Detail::Made(parcels);
            }
        }
        if let Some((sending, _)) = &mut self.sending {
            if !sending.send(gpu, BUDGET) {
                return;
            }
            let Some((done, placing)) = self.sending.take() else { return };
            // The parcels drawn were of the picture before.
            if let Detail::Drawn(_) | Detail::Sending(_) = self.detail {
                self.detail = Detail::None;
            }
            self.parcels = texture(gpu, "ground parcels", PARCELS, 1, 2 * LEVELS as u32, 1);
            self.picture = done.texture;
            self.placing = placing;
            self.bind = bind(gpu, layout, &self.picture, &self.parcels);
            return;
        }
        // The parcels are sent once their picture is drawn.
        if self.busy.is_none() {
            if let Detail::Made(parcels) = &self.detail {
                let side = parcels.first().map_or(1, |p| p.side) as u32;
                self.detail = Detail::Sending(Sending::new(gpu, "ground parcels", PARCELS, side, Layers::Parcels(parcels.clone())));
            }
        }
        if let Detail::Sending(sending) = &mut self.detail {
            if let (true, Layers::Parcels(parcels)) = (sending.send(gpu, BUDGET), &sending.layers) {
                let parcels = parcels.clone();
                let Detail::Sending(done) = std::mem::replace(&mut self.detail, Detail::Drawn(parcels)) else { return };
                self.parcels = done.texture;
                self.bind = bind(gpu, layout, &self.picture, &self.parcels);
            }
        }
    }

    /// The frame's uniforms of the grids for a camera at (east, south): xy their middle relative
    /// to it, z the metres their side spans, w 1 when there is one, 2 when its parcels are drawn too.
    pub fn uniforms(&self, cam_x: f64, cam_z: f64) -> [[f32; 4]; LEVELS] {
        let detail = if matches!(self.detail, Detail::Drawn(_)) { 2.0 } else { 1.0 };
        self.placing.map(|(x, z, extent, there)| [(x - cam_x) as f32, (z - cam_z) as f32, extent as f32, if there { detail } else { 0.0 }])
    }
}

/// The side of the texture a picture's grids are laid in.
fn side(imagery: &Imagery) -> u32 {
    imagery.levels.iter().take(LEVELS).map(|g| g.cols.max(g.rows)).max().unwrap_or(1) as u32
}

fn bind(gpu: &Gpu, layout: &wgpu::BindGroupLayout, picture: &wgpu::Texture, parcels: &wgpu::Texture) -> wgpu::BindGroup {
    let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
    let (picture, parcels) = (view(picture), view(parcels));
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ground imagery"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&picture) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&parcels) }],
    })
}

/// sRGB's decoding of a byte, as a table.
fn linear_table() -> [f32; 256] {
    std::array::from_fn(|i| {
        let s = i as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn encode(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// A grid as the GPU takes it, in a texture `side` texels a side: rows from the north (a smaller
/// grid in the north-west corner), and each smaller copy the mean of four texels
/// of the one before in linear light (the colour where nothing is known is black, so the mean of a
/// coast keeps the land's colour once divided by how much is known, as the shader does).
fn prepare(grid: &Grid<[u8; 4]>, side: u32) -> Prepared {
    let mut base = vec![0u8; (side * side * 4) as usize];
    for (r, row) in grid.data.chunks(grid.cols).enumerate() {
        let y = grid.rows - 1 - r;
        let row = &row[..row.len().min(side as usize)];
        if y >= side as usize {
            continue;
        }
        let at = y * side as usize * 4;
        base[at..at + row.len() * 4].copy_from_slice(row.as_flattened());
    }
    let linear = linear_table();
    let mut mips = vec![base];
    let mut s = side as usize;
    while s > 1 {
        let half = (s / 2).max(1);
        let prev = mips.last().map(Vec::as_slice).unwrap_or_default();
        let mut next = vec![0u8; half * half * 4];
        for y in 0..half {
            for x in 0..half {
                for k in 0..4 {
                    let texel = |i: usize, j: usize| prev[(((2 * y + j).min(s - 1)) * s + (2 * x + i).min(s - 1)) * 4 + k];
                    let four = [texel(0, 0), texel(1, 0), texel(0, 1), texel(1, 1)];
                    next[(y * half + x) * 4 + k] = if k == 3 {
                        ((four.iter().map(|&v| v as u32).sum::<u32>() + 2) / 4) as u8
                    } else {
                        encode(four.iter().map(|&v| linear[v as usize]).sum::<f32>() / 4.0)
                    };
                }
            }
        }
        mips.push(next);
        s = half;
    }
    let extent = side as f64 * grid.step;
    let north = grid.south + (grid.rows as f64 - 1.0) * grid.step;
    let centre = (grid.west + (side as f64 - 1.0) * grid.step / 2.0, -(north - (side as f64 - 1.0) * grid.step / 2.0));
    Prepared { mips, side, centre, extent }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_is_turned_north_up_and_its_copies_keep_the_mean_of_what_is_known() {
        let mut g = Grid::new(-15.0, -15.0, 10.0, 4, 4, [0u8; 4]);
        // The south-west texel known and white, its three neighbours known and black.
        g.set(0, 0, [255, 255, 255, 255]);
        g.set(0, 1, [0, 0, 0, 255]);
        g.set(1, 0, [0, 0, 0, 255]);
        g.set(1, 1, [0, 0, 0, 255]);
        let p = prepare(&g, 4);
        assert_eq!(p.mips.len(), 3);
        // Row 0 of the texture is the north: the white texel is the first of its last row.
        assert_eq!(&p.mips[0][12 * 4..12 * 4 + 4], &[255, 255, 255, 255]);
        // A quarter of white in linear light, all of it known.
        assert_eq!(&p.mips[1][8..12], &[encode(0.25), encode(0.25), encode(0.25), 255]);
        assert_eq!(p.mips[1][3], 0);
        assert_eq!((p.centre, p.extent), ((0.0, 0.0), 40.0));
    }
}
