//! Things scattered over the ground near the camera, one instance each: trees and the buildings of
//! the towns. They are generated per square tile in the background and drawn in batches: the
//! instances of every tile in range, one buffer per kind, made again only when the tiles in range change.
//!
//! An instance is 8 floats (`pipelines::instance_layout`): position relative to the batch's origin
//! and size, then four more that each kind of thing reads its own way (see `placement`).

use crate::jobs::Jobs;
use engine::{wgpu, Gpu};
use sim_core::Terrain;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

/// Tiles generated at once, of each kind.
const IN_FLIGHT: usize = 4;

/// The instances of one tile, by kind (which tree, which building). Positions are relative to the
/// tile's corner.
pub use sim_core::scenery::Instances;

/// What makes the instances of the tile `key` of side `size`: one list per kind, always as many lists.
pub type Build = Arc<dyn Fn(&Terrain, (i32, i32), f64) -> Instances + Send + Sync>;

struct Tile {
    origin_x: f64,
    origin_z: f64,
    kinds: Instances,
    last_used: u64,
    /// The terrain it was made of (`Scatter::generation`).
    generation: u64,
}

/// The instances of one kind of every tile in range, in one buffer.
#[derive(Default)]
pub struct Batch {
    /// `None` until there is something to draw.
    pub buffer: Option<wgpu::Buffer>,
    /// Instances the buffer has room for.
    pub(super) capacity: usize,
    /// Where each tile's instances are in the buffer, with the tile's corner (east, south) in the world.
    pub tiles: Vec<(f64, f64, Range<u32>)>,
}

pub struct Scatter {
    terrain: Arc<Terrain>,
    pub tile_size: f64,
    /// Distance from the camera up to which tiles are drawn, metres.
    pub range: f64,
    /// Tiles kept before the ones not drawn for a while are dropped.
    kept: usize,
    build: Build,
    tiles: HashMap<(i32, i32), Tile>,
    jobs: Jobs<(i32, i32), (Instances, u64)>,
    /// How many times the terrain was replaced: older tiles are drawn until made again.
    generation: u64,
    frame: u64,
    selected: Vec<(i32, i32)>,
    /// The tiles the batches were made of.
    batched: Vec<(i32, i32)>,
    /// One batch per kind.
    pub batches: Vec<Batch>,
    /// What the positions in the batches are relative to (east, south): a tile corner near the
    /// camera, so that `f32` stays precise however far from the world's origin.
    pub origin: (f64, f64),
    pub pending: usize,
}

impl Scatter {
    /// Tiles of side `tile_size` up to `range` metres from the camera, with the `kinds` of thing `build` makes.
    pub fn new(terrain: Arc<Terrain>, tile_size: f64, range: f64, kept: usize, kinds: usize, build: Build) -> Scatter {
        let batches = (0..kinds).map(|_| Batch::default()).collect();
        Scatter { terrain, tile_size, range, kept, build, tiles: HashMap::new(), jobs: Jobs::new(), generation: 0, frame: 0, selected: Vec::new(), batched: Vec::new(), batches, origin: (0.0, 0.0), pending: 0 }
    }

    /// Another terrain of the same place (a region round another middle): the tiles stay and are
    /// made again, the nearest first, each drawn until its new one is there.
    pub fn replace(&mut self, terrain: Arc<Terrain>) {
        self.terrain = terrain;
        self.generation += 1;
    }

    /// Makes the instances another way from now on (what stands on a picture of the ground come or
    /// gone): the tiles are made again as for `replace`.
    pub fn rebuild(&mut self, build: Build) {
        self.build = build;
        self.generation += 1;
    }

    /// Chooses the tiles to draw for a camera at (east, south) and schedules what is missing.
    pub fn select(&mut self, gpu: &Gpu, cx: f64, cz: f64, height_above_ground: f64) {
        self.frame += 1;
        for (key, (kinds, generation)) in self.jobs.collect() {
            self.tiles.insert(key, Tile { origin_x: key.0 as f64 * self.tile_size, origin_z: key.1 as f64 * self.tile_size, kinds, last_used: self.frame, generation });
            // What is drawn of a tile made again must be batched again.
            self.batched.retain(|k| *k != key);
        }

        self.selected.clear();
        let (range, size) = (self.range, self.tile_size);
        if height_above_ground > range {
            self.pending = self.jobs.busy();
            self.rebatch(gpu, cx, cz);
            return;
        }
        let (x0, x1) = (((cx - range) / size).floor() as i32, ((cx + range) / size).floor() as i32);
        let (z0, z1) = (((cz - range) / size).floor() as i32, ((cz + range) / size).floor() as i32);
        let mut wanted: Vec<((i32, i32), f64)> = Vec::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                let d = ((x as f64 + 0.5) * size - cx).hypot((z as f64 + 0.5) * size - cz);
                if d > range + size {
                    continue;
                }
                let key = (x, z);
                if let Some(t) = self.tiles.get_mut(&key) {
                    t.last_used = self.frame;
                    if t.kinds.iter().any(|k| !k.is_empty()) {
                        self.selected.push(key);
                    }
                    if t.generation < self.generation && !self.jobs.contains(&key) {
                        wanted.push((key, d));
                    }
                } else if !self.jobs.contains(&key) {
                    wanted.push((key, d));
                }
            }
        }
        wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
        self.pending = wanted.len() + self.jobs.busy();
        for (key, _) in wanted {
            if self.jobs.busy() >= IN_FLIGHT {
                break;
            }
            let (terrain, build, generation) = (self.terrain.clone(), self.build.clone(), self.generation);
            self.jobs.start(key, move || (build(&terrain, key, size), generation));
        }
        if self.tiles.len() > self.kept {
            let frame = self.frame;
            self.tiles.retain(|_, t| frame - t.last_used <= 600);
        }
        self.rebatch(gpu, cx, cz);
    }

    /// Makes the batches again if the tiles chosen are not the ones they were made of.
    fn rebatch(&mut self, gpu: &Gpu, cx: f64, cz: f64) {
        if self.selected == self.batched {
            return;
        }
        let size = self.tile_size;
        self.origin = ((cx / size).floor() * size, (cz / size).floor() * size);
        for (kind, batch) in self.batches.iter_mut().enumerate() {
            let mut data: Vec<f32> = Vec::new();
            batch.tiles.clear();
            for key in &self.selected {
                let tile = &self.tiles[key];
                let Some(instances) = tile.kinds.get(kind).filter(|k| !k.is_empty()) else { continue };
                let first = (data.len() / 8) as u32;
                // Whole tiles apart: the sum is exact as long as the tile's own part is.
                let (dx, dz) = ((tile.origin_x - self.origin.0) as f32, (tile.origin_z - self.origin.1) as f32);
                for instance in instances.chunks_exact(8) {
                    data.extend_from_slice(&[instance[0] + dx, instance[1], instance[2] + dz]);
                    data.extend_from_slice(&instance[3..]);
                }
                batch.tiles.push((tile.origin_x, tile.origin_z, first..(data.len() / 8) as u32));
            }
            let count = data.len() / 8;
            if count > batch.capacity {
                // Room to spare, so that flying on does not make a new buffer at every tile crossed.
                batch.capacity = count + count / 2;
                batch.buffer = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instances"),
                    size: (batch.capacity * 32) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let (Some(buffer), false) = (&batch.buffer, data.is_empty()) {
                gpu.queue.write_buffer(buffer, 0, bytemuck::cast_slice(&data));
            }
        }
        self.batched.clone_from(&self.selected);
    }
}
