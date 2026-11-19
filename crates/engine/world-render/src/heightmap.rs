//! A coarse picture of the ground around the camera that the shaders walk over to find what is in
//! the shade of the relief. It is rebuilt in the background when the camera has moved far enough.

use engine::{wgpu, Gpu};
use sim_core::Terrain;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

const SIZE: usize = 1024;
const TEXEL: f64 = 64.0;
/// Threads that share the work of one picture.
const THREADS: usize = 4;

pub struct HeightMap {
    pub texture: wgpu::Texture,
    terrain: Arc<Terrain>,
    /// Centre of the picture in render space (east, south), metres.
    pub centre_x: f64,
    pub centre_z: f64,
    pub ready: bool,
    /// The terrain was replaced (`replace`): the picture is made again, the old one drawn meanwhile.
    stale: bool,
    /// The picture being made: heights, and its centre.
    busy: Option<Receiver<(Vec<f32>, f64, f64)>>,
}

impl HeightMap {
    pub const HALF_EXTENT: f64 = SIZE as f64 * TEXEL / 2.0;

    pub fn new(gpu: &Gpu, terrain: Arc<Terrain>) -> HeightMap {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("height map"),
            size: wgpu::Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // The heights, and in the second channel the shade of the clouds over the same ground,
            // which `clouds.rs` draws there every frame (a texture of its own would be one more than
            // the 16 a fragment shader may read).
            format: wgpu::TextureFormat::Rg16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        HeightMap { texture, terrain, centre_x: 0.0, centre_z: 0.0, ready: false, stale: false, busy: None }
    }

    /// Another terrain of the same place (a region round another middle): the picture is made
    /// again from it, the one there drawn until it is.
    pub fn replace(&mut self, terrain: Arc<Terrain>) {
        self.terrain = terrain;
        self.stale = true;
    }

    /// Takes a finished picture, and starts a new one when the camera (east, south) is far from the centre.
    pub fn update(&mut self, gpu: &Gpu, cam_x: f64, cam_z: f64) {
        if let Some(Ok((data, x, z))) = self.busy.as_ref().map(|r| r.try_recv()) {
            gpu.queue.write_texture(
                self.texture.as_image_copy(),
                bytemuck::cast_slice(&crate::pipelines::half_floats(&data.iter().flat_map(|h| [*h, 0.0]).collect::<Vec<f32>>())),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE as u32 * 4), rows_per_image: None },
                self.texture.size(),
            );
            (self.centre_x, self.centre_z) = (x, z);
            self.ready = true;
            self.busy = None;
        }
        if self.busy.is_some() || (self.ready && !self.stale && (cam_x - self.centre_x).hypot(cam_z - self.centre_z) <= 6000.0) {
            return;
        }
        // Snap to the grid so the map does not shimmer when it is rebuilt.
        let (cx, cz) = ((cam_x / TEXEL).round() * TEXEL, (cam_z / TEXEL).round() * TEXEL);
        self.stale = false;
        let (sender, receiver) = channel();
        self.busy = Some(receiver);
        let terrain = self.terrain.clone();
        engine::threads::spawn(move || {
            let mut data = vec![0f32; SIZE * SIZE];
            let rows_each = SIZE.div_ceil(THREADS);
            engine::threads::scope(|scope| {
                for (part, rows) in data.chunks_mut(rows_each * SIZE).enumerate() {
                    let terrain = &terrain;
                    scope.spawn(move || {
                        for (k, row) in rows.chunks_mut(SIZE).enumerate() {
                            let j = part * rows_each + k;
                            let z = cz + (j as f64 + 0.5 - SIZE as f64 / 2.0) * TEXEL;
                            for (i, h) in row.iter_mut().enumerate() {
                                let x = cx + (i as f64 + 0.5 - SIZE as f64 / 2.0) * TEXEL;
                                // Below sea level too: the sea reads its depth from here.
                                *h = terrain.raw(-z, x).h as f32;
                            }
                        }
                    });
                }
            });
            let _ = sender.send((data, cx, cz));
        });
    }
}
