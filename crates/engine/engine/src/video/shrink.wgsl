// The window's picture, as it is on the screen, at the size of the video (video/mod.rs).

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var blend: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle over the whole viewport.
@vertex fn fullscreen(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Out(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

// Four blended reads a quarter of the video's pixel from its centre: the mean of what lies under
// the pixel, about, for a picture shrunk up to two times (thin lines and text stay steady).
@fragment fn shrink(in: Out) -> @location(0) vec4<f32> {
    let d = 0.25 * vec2<f32>(dpdx(in.uv.x), dpdy(in.uv.y));
    let sum = textureSample(picture, blend, in.uv + vec2<f32>(-d.x, -d.y))
        + textureSample(picture, blend, in.uv + vec2<f32>(d.x, -d.y))
        + textureSample(picture, blend, in.uv + vec2<f32>(-d.x, d.y))
        + textureSample(picture, blend, in.uv + vec2<f32>(d.x, d.y));
    return vec4<f32>(sum.rgb * 0.25, 1.0);
}
