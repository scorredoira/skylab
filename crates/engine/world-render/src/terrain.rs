//! Quadtree of terrain tiles: fine near the camera, coarse far away, generated in the background.
//!
//! Render space: x east, y up, z south. A world point (north, east) is (x: east, z: -north).

use crate::jobs::Jobs;
use engine::wgpu::util::DeviceExt;
use engine::{wgpu, Gpu};
use sim_core::Terrain;
use glam::Vec3;
use std::collections::HashMap;
use std::sync::Arc;

const CELLS: usize = 64;
const ROOT_SIZE: f64 = 32768.0;
const MAX_LEVEL: i32 = 6;
const SPLIT_FACTOR: f64 = 2.2;
/// Tiles generated at once.
const IN_FLIGHT: usize = 6;
/// Tiles kept before the ones not drawn for a while are dropped.
const KEPT: usize = 1100;
/// Most tiles that one frame can draw (every loaded tile).
pub const MOST_DRAWN: usize = KEPT;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    level: i32,
    x: i32,
    z: i32,
}

pub struct Tile {
    pub buffer: wgpu::Buffer,
    pub origin_x: f64,
    pub origin_z: f64,
    pub size: f64,
    pub min_y: f32,
    pub max_y: f32,
    last_used: u64,
    /// The terrain it was made of (`TerrainTiles::generation`).
    generation: u64,
}

/// A tile as the background thread leaves it: the vertices, not yet on the GPU.
struct Built {
    data: Vec<f32>,
    min_y: f32,
    max_y: f32,
    generation: u64,
}

pub struct TerrainTiles {
    terrain: Arc<Terrain>,
    /// Indices of one tile and its skirts, the same for all.
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    chunks: HashMap<Key, Tile>,
    jobs: Jobs<Key, Built>,
    frame: u64,
    wanted: Vec<(Key, f64)>,
    draw_list: Vec<Key>,
    /// Tiles being generated or still needed.
    pub pending: usize,
    /// Distance from the camera up to which there is terrain, metres.
    pub view_distance: f64,
    /// How many times the terrain was replaced (`replace`): tiles of an older one are drawn until
    /// they are made again.
    generation: u64,
}

fn size(level: i32) -> f64 {
    ROOT_SIZE / (1u32 << level) as f64
}

/// Vertex index of the i-th point of each edge of a tile.
fn edge(e: usize, i: usize) -> usize {
    let (n, row) = (CELLS, CELLS + 1);
    match e {
        0 => i,
        1 => n * row + i,
        2 => i * row,
        _ => i * row + n,
    }
}

fn build(key: Key, terrain: &Terrain) -> Built {
    let (n, row) = (CELLS, CELLS + 1);
    let size = size(key.level);
    let step = size / n as f64;
    let (ox, oz) = (key.x as f64 * size, key.z as f64 * size);
    let airports = terrain.airports(-(oz + size) - step, -oz + step, ox - step, ox + size + step);
    let towns = terrain.towns(-(oz + size) - step, -oz + step, ox - step, ox + size + step);

    // Heights on a grid one cell wider than the tile, for the normals.
    let g = n + 3;
    let mut grounds = Vec::with_capacity(g * g);
    for j in 0..g {
        for i in 0..g {
            let (x, z) = (ox + (i as f64 - 1.0) * step, oz + (j as f64 - 1.0) * step);
            grounds.push(terrain.ground_with(-z, x, &airports));
        }
    }

    let mut data = vec![0f32; (row * row + 4 * row) * 12];
    let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
    // Far tiles are too coarse to light from their own grid, so their slopes are measured over 32 m.
    let fine = step > 32.0;
    let d = 32.0;
    for j in 0..row {
        for i in 0..row {
            let gi = (j + 1) * g + (i + 1);
            let gr = grounds[gi];
            let (x, z) = (ox + i as f64 * step, oz + j as f64 * step);
            let (dhdx, dhdz) = if fine {
                (
                    (terrain.ground_with(-z, x + d, &airports).h - terrain.ground_with(-z, x - d, &airports).h) / (2.0 * d),
                    (terrain.ground_with(-(z + d), x, &airports).h - terrain.ground_with(-(z - d), x, &airports).h) / (2.0 * d),
                )
            } else {
                ((grounds[gi + 1].h - grounds[gi - 1].h) / (2.0 * step), (grounds[gi + g].h - grounds[gi - g].h) / (2.0 * step))
            };
            let slope = (dhdx * dhdx + dhdz * dhdz).sqrt();
            let built = towns.iter().fold(0.0f64, |built, t| built.max(t.weight(-z, x)));
            let s = terrain.surface(-z, x, &gr, slope, built);
            let nrm = Vec3::new(-dhdx as f32, 1.0, -dhdz as f32).normalize();
            let y = gr.h as f32;
            let o = (j * row + i) * 12;
            data[o..o + 12].copy_from_slice(&[
                (i as f64 * step) as f32, y, (j as f64 * step) as f32,
                nrm.x, nrm.y, nrm.z,
                s.color.x, s.color.y, s.color.z,
                s.farm, s.water, s.forest,
            ]);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    // Skirts hide the cracks between tiles of different detail.
    let drop = (step * 1.5 + 3.0) as f32;
    for e in 0..4 {
        for i in 0..row {
            let (src, dst) = (edge(e, i) * 12, (row * row + e * row + i) * 12);
            data.copy_within(src..src + 12, dst);
            data[dst + 1] -= drop;
        }
    }
    Built { data, min_y, max_y, generation: 0 }
}

impl TerrainTiles {
    pub fn new(gpu: &Gpu, terrain: Arc<Terrain>) -> TerrainTiles {
        let (n, row) = (CELLS, CELLS + 1);
        let mut idx: Vec<u16> = Vec::with_capacity(n * n * 6 + 4 * n * 6);
        for j in 0..n {
            for i in 0..n {
                let a = (j * row + i) as u16;
                let (b, c) = (a + 1, ((j + 1) * row + i) as u16);
                let d = c + 1;
                idx.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
        let base = row * row;
        for e in 0..4 {
            for i in 0..n {
                let (a, b) = (edge(e, i) as u16, edge(e, i + 1) as u16);
                let sa = (base + e * row + i) as u16;
                let sb = sa + 1;
                idx.extend_from_slice(&[a, sa, b, b, sa, sb]);
            }
        }
        let index_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("terrain indices"), contents: bytemuck::cast_slice(&idx), usage: wgpu::BufferUsages::INDEX });
        TerrainTiles {
            terrain,
            index_buffer,
            index_count: idx.len() as u32,
            chunks: HashMap::new(),
            jobs: Jobs::new(),
            frame: 0,
            wanted: Vec::new(),
            draw_list: Vec::new(),
            pending: 0,
            view_distance: crate::Quality::HIGH.view_distance,
            generation: 0,
        }
    }

    /// Another terrain of the same place (a region round another middle): the tiles drawn stay
    /// and are made again, the nearest first, each drawn until its new one is there.
    pub fn replace(&mut self, terrain: Arc<Terrain>) {
        self.terrain = terrain;
        self.generation += 1;
    }

    pub fn tile(&self, key: Key) -> &Tile {
        &self.chunks[&key]
    }

    fn request(&mut self, key: Key, distance: f64) {
        if !self.jobs.contains(&key) {
            self.wanted.push((key, distance));
        }
    }

    fn visit(&mut self, key: Key, cx: f64, cy: f64, cz: f64) {
        let frame = self.frame;
        let generation = self.generation;
        let Some(c) = self.chunks.get_mut(&key) else { return };
        c.last_used = frame;
        let stale = c.generation < generation;
        let dx = (c.origin_x - cx).max(0.0).max(cx - (c.origin_x + c.size));
        let dz = (c.origin_z - cz).max(0.0).max(cz - (c.origin_z + c.size));
        let dy = (c.min_y as f64 - cy).max(0.0).max(cy - c.max_y as f64);
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if stale {
            self.request(key, d);
        }
        let Some(c) = self.chunks.get(&key) else { return };
        if key.level < MAX_LEVEL && d < c.size * SPLIT_FACTOR {
            let l = key.level + 1;
            let kids = [
                Key { level: l, x: key.x * 2, z: key.z * 2 },
                Key { level: l, x: key.x * 2 + 1, z: key.z * 2 },
                Key { level: l, x: key.x * 2, z: key.z * 2 + 1 },
                Key { level: l, x: key.x * 2 + 1, z: key.z * 2 + 1 },
            ];
            if kids.iter().all(|k| self.chunks.contains_key(k)) {
                for k in kids {
                    self.visit(k, cx, cy, cz);
                }
                return;
            }
            for k in kids {
                if !self.chunks.contains_key(&k) {
                    self.request(k, d);
                }
            }
        }
        self.draw_list.push(key);
    }

    /// Chooses the tiles to draw this frame for a camera at (east, altitude, south) and schedules what is missing.
    pub fn select(&mut self, gpu: &Gpu, cx: f64, cy: f64, cz: f64) {
        self.frame += 1;
        for (key, built) in self.jobs.collect() {
            let size = size(key.level);
            let buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("terrain tile"), contents: bytemuck::cast_slice(&built.data), usage: wgpu::BufferUsages::VERTEX });
            let tile = Tile { buffer, origin_x: key.x as f64 * size, origin_z: key.z as f64 * size, size, min_y: built.min_y, max_y: built.max_y, last_used: self.frame, generation: built.generation };
            self.chunks.insert(key, tile);
        }

        self.wanted.clear();
        self.draw_list.clear();
        let (rs, view) = (ROOT_SIZE, self.view_distance);
        let (x0, x1) = (((cx - view) / rs).floor() as i32, ((cx + view) / rs).floor() as i32);
        let (z0, z1) = (((cz - view) / rs).floor() as i32, ((cz + view) / rs).floor() as i32);
        for x in x0..=x1 {
            for z in z0..=z1 {
                let key = Key { level: 0, x, z };
                if self.chunks.contains_key(&key) {
                    self.visit(key, cx, cy, cz);
                } else {
                    let dx = (x as f64 * rs - cx).max(0.0).max(cx - (x + 1) as f64 * rs);
                    let dz = (z as f64 * rs - cz).max(0.0).max(cz - (z + 1) as f64 * rs);
                    self.request(key, dx.hypot(dz));
                }
            }
        }

        self.wanted.sort_by(|a, b| a.1.total_cmp(&b.1));
        self.pending = self.wanted.len() + self.jobs.busy();
        for &(key, _) in &self.wanted {
            if self.jobs.busy() >= IN_FLIGHT {
                break;
            }
            let (terrain, generation) = (self.terrain.clone(), self.generation);
            self.jobs.start(key, move || Built { generation, ..build(key, &terrain) });
        }

        if self.chunks.len() > KEPT {
            let frame = self.frame;
            self.chunks.retain(|_, c| frame - c.last_used <= 240);
        }
    }

    /// The tiles chosen by the last `select`.
    pub fn selected(&self) -> &[Key] {
        &self.draw_list
    }
}
