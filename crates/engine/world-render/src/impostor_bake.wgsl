// The trees' pictures from far (impostors.rs): each species' full mesh drawn from every view of a
// hemi-octahedral grid, unlit, into a picture twice the size it is kept at; then that picture
// shrunk into the kept one and each of its smaller levels. Appended to flora_surface.wgsl.
//
// Group 0: one `Bake` per view, at a dynamic offset. Group 2: the foliage's and the barks' pictures
// (flora_surface.wgsl's bindings). The shrinking: group 0 binding 1 and 2, the level it shrinks.

struct Bake {
    // The unit model to the view's clip space: orthographic, along `dir`, within `radius` of `centre`.
    right: vec4<f32>,  // xyz = the view's right, w = radius
    up: vec4<f32>,     // xyz = its up
    dir: vec4<f32>,    // xyz = towards the eye
    centre: vec4<f32>, // xyz = the middle of the views
};

@group(0) @binding(0) var<uniform> bake: Bake;

struct BakeIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(5) tex: vec4<f32>,
    @location(6) crown: vec4<f32>,
};

struct BakeOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) tex: vec3<f32>,
    // How far in front of the views' middle plane, as 0..1 over two radii.
    @location(3) depth: f32,
};

@vertex
fn bake_vertex(in: BakeIn) -> BakeOut {
    var o: BakeOut;
    let p = in.position - bake.centre.xyz;
    let r = bake.right.w;
    let ahead = dot(p, bake.dir.xyz) / r;
    // Reversed depth, as the scene's: nearer the eye is nearer 1.
    o.position = vec4<f32>(dot(p, bake.right.xyz) / r, dot(p, bake.up.xyz) / r, 0.5 + 0.5 * ahead, 1.0);
    o.normal = in.normal;
    o.color = in.color.rgb;
    o.tex = in.tex.xyz;
    o.depth = 0.5 + 0.5 * ahead;
    return o;
}

struct BakeTargets {
    @location(0) colour: vec4<f32>,
    @location(1) detail: vec4<f32>,
};

@fragment
fn bake_fragment(in: BakeOut) -> BakeTargets {
    let s = flora_surface(in.tex.xy, in.tex.z, in.color, normalize(in.normal));
    if s.alpha < 0.5 {
        discard;
    }
    var o: BakeTargets;
    o.colour = vec4<f32>(s.albedo, 1.0);
    o.detail = vec4<f32>(s.normal * 0.5 + 0.5, in.depth);
    return o;
}

// ---------------------------------------------------------------- shrinking

@group(0) @binding(1) var shrink_colour: texture_2d<f32>;
@group(0) @binding(2) var shrink_detail: texture_2d<f32>;

@vertex
fn shrink_vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// A texel of the level as the mean of the four under it: colour and normal weighted by how much
// of each is covered, so the empty sky round a tree brings in nothing.
@fragment
fn shrink_fragment(@builtin(position) at: vec4<f32>) -> BakeTargets {
    let base = vec2<i32>(at.xy) * 2;
    var colour = vec4<f32>(0.0);
    var normal = vec3<f32>(0.0);
    var depth = 0.0;
    for (var k = 0; k < 4; k++) {
        let p = base + vec2<i32>(k & 1, k >> 1);
        let c = textureLoad(shrink_colour, p, 0);
        let d = textureLoad(shrink_detail, p, 0);
        colour += vec4<f32>(c.rgb * c.a, c.a);
        normal += (d.xyz * 2.0 - 1.0) * c.a;
        depth += d.w * c.a;
    }
    var o: BakeTargets;
    let a = max(colour.a, 1e-4);
    o.colour = vec4<f32>(colour.rgb / a, colour.a / 4.0);
    o.detail = vec4<f32>(normalize(normal + vec3<f32>(0.0, 1e-4, 0.0)) * 0.5 + 0.5, select(0.5, depth / a, colour.a > 1e-4));
    return o;
}
