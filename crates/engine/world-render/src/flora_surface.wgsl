// What the surface of a tree is at a fragment: the foliage's and the barks' pictures, read the same
// way by the scene (flora.wgsl) and by the baking of the trees' pictures from far (impostor_bake.wgsl).
// `species.rs` writes the table of the species (SPECIES_CROWN, SPECIES_LIGHT) before this file.

// Group 2 of the trees' pipelines (`pipelines::flora_layout`); bindings 0 and 1 are the ground's
// pictures of the terrain's pipelines, which never draw a tree.
// The foliage's cards (foliage.rs), a layer per kind: colour scaled to half grey and its outline;
// the leaf's slope across the card (xy), how much light it lets through (z), its outline.
@group(2) @binding(2) var foliage_colour: texture_2d_array<f32>;
@group(2) @binding(3) var foliage_detail: texture_2d_array<f32>;
// The barks (species::BARK_FILES): colour, and the normal map (x and y).
@group(2) @binding(4) var bark_colour: texture_2d_array<f32>;
@group(2) @binding(5) var bark_normal: texture_2d_array<f32>;
// Repeating, filtered between levels, anisotropic.
@group(2) @binding(8) var flora_sampler: sampler;

// Layers from this number on are barks (`vegetation::BARK`).
const BARK_LAYER: f32 = 16.0;

struct FloraSurface {
    albedo: vec3<f32>,
    // How much of the pixel it covers, before the cut-off at a half.
    alpha: f32,
    normal: vec3<f32>,
    // How much light it lets through: 0 for bark.
    through: f32,
};

// Two directions square to `n`.
fn flora_frame(n: vec3<f32>) -> mat2x3<f32> {
    let t = normalize(cross(select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.95), n));
    return mat2x3<f32>(t, cross(n, t));
}

// The surface at `uv` of `layer` (tex.xy, tex.z of a vertex), its colour times `tint`, round the
// normal `n` of the crown's volume (or of the bark).
fn flora_surface(uv: vec2<f32>, layer: f32, tint: vec3<f32>, n: vec3<f32>) -> FloraSurface {
    let is_bark = layer >= BARK_LAYER - 0.5;
    let gx = dpdx(uv);
    let gy = dpdy(uv);
    // Both are read everywhere: a texture read where only some fragments go loses its derivatives.
    let leaf = textureSampleGrad(foliage_colour, flora_sampler, uv, select(i32(layer + 0.5), 0, is_bark), gx, gy);
    let detail = textureSampleGrad(foliage_detail, flora_sampler, uv, select(i32(layer + 0.5), 0, is_bark), gx, gy);
    let bark = textureSampleGrad(bark_colour, flora_sampler, uv, select(0, i32(layer - BARK_LAYER + 0.5), is_bark), gx, gy).rgb;
    let relief = textureSampleGrad(bark_normal, flora_sampler, uv, select(0, i32(layer - BARK_LAYER + 0.5), is_bark), gx, gy).xy * 2.0 - 1.0;
    let frame = flora_frame(n);
    var s: FloraSurface;
    if is_bark {
        s.albedo = bark * tint;
        s.alpha = 1.0;
        // Round the trunk: u goes round it, v up it.
        let up = normalize(vec3<f32>(0.0, 1.0, 0.0) - n * n.y + vec3<f32>(0.0, 1e-4, 0.0));
        let round = cross(up, n);
        s.normal = normalize(n + round * relief.x * 0.8 + up * relief.y * 0.8);
        s.through = 0.0;
    } else {
        s.albedo = leaf.rgb * 2.0 * tint;
        s.alpha = leaf.a;
        let slope = detail.xy * 2.0 - 1.0;
        s.normal = normalize(n + (frame[0] * slope.x + frame[1] * slope.y) * 0.9);
        s.through = detail.z;
    }
    return s;
}

// How open to the sky a point of a crown is, from its place in it (crown radii): the same as
// `vegetation::Builder::openness`, for the pictures from far that keep no openness of their own.
fn crown_open(q: vec3<f32>) -> f32 {
    return clamp(0.2 + 0.55 * min(length(q), 1.2) + 0.3 * clamp(q.y, -1.0, 1.0), 0.08, 1.0);
}
