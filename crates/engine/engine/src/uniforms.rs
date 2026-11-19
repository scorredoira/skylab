//! Per-draw data: many small uniform blocks in one buffer, picked with a dynamic offset.

use crate::Gpu;

/// Collects one uniform block per draw call during `update`, uploads them together, and hands back
/// the offset to pass to `set_bind_group`. The bind group layout must have, at binding 0, a uniform
/// buffer with `has_dynamic_offset: true`.
pub struct UniformArena {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    stride: usize,
    capacity: usize,
    data: Vec<u8>,
    count: usize,
}

impl UniformArena {
    /// Room for `capacity` blocks of `block_size` bytes each.
    pub fn new(gpu: &Gpu, layout: &wgpu::BindGroupLayout, block_size: usize, capacity: usize) -> UniformArena {
        let align = gpu.device.limits().min_uniform_buffer_offset_alignment as usize;
        let stride = block_size.div_ceil(align) * align;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("per-draw uniforms"),
            size: (stride * capacity) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("per-draw uniforms"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buffer, offset: 0, size: wgpu::BufferSize::new(block_size as u64) }),
            }],
        });
        UniformArena { buffer, bind_group, stride, capacity, data: vec![0; stride * capacity], count: 0 }
    }

    /// Forget last frame's blocks.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// Adds a block and returns its dynamic offset. Blocks beyond the capacity reuse the last slot,
    /// which draws wrong rather than crashing; size the arena for the worst case.
    pub fn push<T: bytemuck::Pod>(&mut self, value: &T) -> u32 {
        let index = self.count.min(self.capacity - 1);
        let bytes = bytemuck::bytes_of(value);
        let start = index * self.stride;
        self.data[start..start + bytes.len()].copy_from_slice(bytes);
        self.count = index + 1;
        start as u32
    }

    /// Sends this frame's blocks to the GPU. Call once, after the last `push`.
    pub fn upload(&self, gpu: &Gpu) {
        if self.count > 0 {
            gpu.queue.write_buffer(&self.buffer, 0, &self.data[..self.count * self.stride]);
        }
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}
