//! The roads of a real place round the camera, as a picture the ground's shader reads: how much of
//! each texel of 3 m is road, with its smaller copies for the distance (so a road far off fades
//! into the ground as it would, instead of shimmering). A region has thousands of roads, far more
//! than the frame's uniforms hold; the generated world's few go there instead and need no picture.
//! It is drawn again in the background when the camera has moved far enough.

use engine::{wgpu, Gpu};
use sim_core::Terrain;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

const SIZE: usize = 4096;
/// Metres a texel.
pub const TEXEL: f64 = 3.0;
const LEVELS: u32 = SIZE.ilog2() + 1;

/// A picture drawn in the background: its levels, finest first, and its centre (east, south).
type Picture = (Vec<Vec<u8>>, f64, f64);

pub struct RoadMap {
    pub texture: wgpu::Texture,
    terrain: Arc<Terrain>,
    /// Centre of the picture in render space (east, south), metres.
    pub centre_x: f64,
    pub centre_z: f64,
    pub ready: bool,
    /// The terrain was replaced (`replace`): the picture is made again, the old one drawn meanwhile.
    stale: bool,
    /// The picture being drawn.
    busy: Option<Receiver<Picture>>,
}

impl RoadMap {
    pub const HALF_EXTENT: f64 = SIZE as f64 * TEXEL / 2.0;

    /// A picture for the roads of a region; for a generated world, one texel that is never drawn on.
    pub fn new(gpu: &Gpu, terrain: Arc<Terrain>) -> RoadMap {
        let side = if terrain.region().is_some() { SIZE as u32 } else { 1 };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("road map"),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
            mip_level_count: if side > 1 { LEVELS } else { 1 },
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        RoadMap { texture, terrain, centre_x: 0.0, centre_z: 0.0, ready: false, stale: false, busy: None }
    }

    /// Whether there is anything to draw: only a region's roads are drawn from the picture.
    pub fn used(&self) -> bool {
        self.terrain.region().is_some()
    }

    /// Another terrain of the same place (a region round another middle): the picture is made
    /// again from it, the one there drawn until it is.
    pub fn replace(&mut self, terrain: Arc<Terrain>) {
        self.terrain = terrain;
        self.stale = true;
    }

    /// Takes a finished picture, and starts a new one when the camera (east, south) is far from the centre.
    pub fn update(&mut self, gpu: &Gpu, cam_x: f64, cam_z: f64) {
        if !self.used() {
            return;
        }
        if let Some(Ok((levels, x, z))) = self.busy.as_ref().map(|r| r.try_recv()) {
            for (level, data) in levels.iter().enumerate() {
                let side = (SIZE >> level) as u32;
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &self.texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    data,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(side), rows_per_image: None },
                    wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                );
            }
            (self.centre_x, self.centre_z) = (x, z);
            self.ready = true;
            self.busy = None;
        }
        if self.busy.is_some() || (self.ready && !self.stale && (cam_x - self.centre_x).hypot(cam_z - self.centre_z) <= Self::HALF_EXTENT / 2.0) {
            return;
        }
        // On the grid of texels, so the roads do not crawl when it is drawn again.
        let (cx, cz) = ((cam_x / TEXEL).round() * TEXEL, (cam_z / TEXEL).round() * TEXEL);
        self.stale = false;
        let (sender, receiver) = channel();
        self.busy = Some(receiver);
        let terrain = self.terrain.clone();
        engine::threads::spawn(move || {
            let base = draw(&terrain, cx, cz);
            let _ = sender.send((levels(base), cx, cz));
        });
    }
}

/// How much of each texel round (east, south) = (cx, cz) is road: 1 inside it, falling to 0 over a
/// texel at its edge. Row 0 is the north (least z).
fn draw(terrain: &Terrain, cx: f64, cz: f64) -> Vec<u8> {
    let mut data = vec![0u8; SIZE * SIZE];
    let half = RoadMap::HALF_EXTENT;
    let (west, north) = (cx - half, cz - half);
    for r in terrain.roads(-cz - half, -cz + half, cx - half, cx + half) {
        // Its ends in texels: x east from the west edge, y south from the north edge.
        let (ax, ay) = ((r.e0 - west) / TEXEL, (-r.n0 - north) / TEXEL);
        let (bx, by) = ((r.e1 - west) / TEXEL, (-r.n1 - north) / TEXEL);
        let w = r.width / 2.0 / TEXEL;
        let pad = w + 1.0;
        let (x0, x1) = ((ax.min(bx) - pad).floor().max(0.0) as usize, ((ax.max(bx) + pad).ceil().max(0.0) as usize).min(SIZE));
        let (y0, y1) = ((ay.min(by) - pad).floor().max(0.0) as usize, ((ay.max(by) + pad).ceil().max(0.0) as usize).min(SIZE));
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = (dx * dx + dy * dy).max(1e-9);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f64 + 0.5 - ax, y as f64 + 0.5 - ay);
                let t = ((px * dx + py * dy) / len2).clamp(0.0, 1.0);
                let d = (px - dx * t).hypot(py - dy * t);
                let cover = (w - d + 0.5).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let v = &mut data[y * SIZE + x];
                    *v = (*v).max((cover * 255.0).round() as u8);
                }
            }
        }
    }
    data
}

/// The picture and its smaller copies, each the average of four texels of the one before.
fn levels(base: Vec<u8>) -> Vec<Vec<u8>> {
    let mut all = vec![base];
    let mut side = SIZE;
    while side > 1 {
        let prev = all.last().map(Vec::as_slice).unwrap_or_default();
        let half = side / 2;
        let mut next = vec![0u8; half * half];
        for y in 0..half {
            for x in 0..half {
                let at = |i: usize, j: usize| prev[(2 * y + j) * side + 2 * x + i] as u32;
                next[y * half + x] = ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
            }
        }
        all.push(next);
        side = half;
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_halve_down_to_one_texel() {
        let mut base = vec![0u8; SIZE * SIZE];
        base[..SIZE * SIZE / 2].fill(255);
        let all = levels(base);
        assert_eq!(all.len(), LEVELS as usize);
        assert_eq!(all.last().unwrap(), &vec![128u8]);
        assert_eq!(all[1].len(), SIZE * SIZE / 4);
    }
}
