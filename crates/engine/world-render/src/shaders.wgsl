// The shaders of the world (with scenery.wgsl). Loaded by `pipelines.rs`, which documents the
// bindings and mirrors the uniform structs field by field. A game appends its own (`shader_source`).
//
// Render space: x east, y up, z south, relative to the camera. Colours are linear.

const WRAP: f32 = 16384.0;
const WRAP_I: i32 = 16384;
// Metres a texel of the road map (`roadmap::TEXEL`).
const ROAD_TEXEL: f32 = 3.0;
const PI: f32 = 3.14159265;

// What the quality setting changes: `pipelines.rs` rewrites these three lines for the lower qualities.
const RELIEF_STEPS: i32 = 22;
const RELIEF_GROWTH: f32 = 1.3;
const CLOUD_OPACITY: f32 = 0.42;
// How dark the ground is right under one of the body's solid pieces from what it hides of the sky (`body_contact`).
const CONTACT_DARK: f32 = 0.85;

// Per-frame data. Mirrors `FrameUniforms` in pipelines.rs: same fields, same order, 259 * 16 bytes.

struct Frame {
    view_proj: mat4x4<f32>,
    cam_right: vec4<f32>,   // xyz = camera right, w = tan(half fov x)
    cam_up: vec4<f32>,      // xyz = camera up, w = tan(half fov y)
    cam_fwd: vec4<f32>,     // xyz = camera forward, w = vertical shift of the view
    sun_dir: vec4<f32>,     // xyz = direction towards the sun, w = time
    sun_color: vec4<f32>,   // rgb = sunlight, w = daylight 0..1
    cam_wrap: vec4<f32>,    // xz = camera position modulo WRAP, y = altitude, w = 1 when the clouds are the volumetric ones
    body_pos: vec4<f32>,    // xyz = the body the view belongs to, relative to the camera, w = shadow strength
    body_fwd: vec4<f32>,    // xyz = its forward axis
    body_right: vec4<f32>,  // xyz = its right axis
    body_down: vec4<f32>,   // xyz = its down axis
    shadow_flat: array<vec4<f32>, 4>, // its shadow's pieces lying flat (see `body_shadow`)
    shadow_flat_z: vec4<f32>,         // the heights (body z) they lie at
    shadow_upright: array<vec4<f32>, 3>, // its pieces standing in the plane y = 0
    shadow_solids: array<vec4<f32>, 18>, // its solid pieces, two rows each (see `solid_cover`)
    shadow_reach: vec4<f32>,  // x = the radius round the body that holds them (0: none)
    misc: vec4<f32>,        // x = airports, y = cloud height relative to the camera, z = tree range, w = cloud cover
    airports_a: array<vec4<f32>, 8>, // xy = centre (x, z) relative to the camera, zw = runway direction (x, z)
    airports_b: array<vec4<f32>, 8>, // x = half length, y = half width, z = number, w = height relative to the camera
    hmap: vec4<f32>,        // xy = height map centre (x, z) relative to the camera, z = half extent, w = 1 when ready
    extra: vec4<f32>,       // x = cloud layer thickness, y = roads, z = towns, w = airports near enough to show their mown grass
    roads: array<vec4<f32>, 32>,     // xy = one end, zw = the other, (x, z) relative to the camera
    towns: array<vec4<f32>, 16>,     // xy = centre (x, z) relative to the camera, z = radius, w = direction of the grid
    materials: array<vec4<f32>, 20>, // [materials] per texture layer: rgb = its average colour, w = its average roughness
    fire_count: vec4<f32>,  // x = lights of fires, y = marks on the ground, z = patches of foam (effects.rs)
    fire_pos: array<vec4<f32>, 4>,   // xyz = a light of the ground, relative to the camera, w = what a beam spills all round
    fire_color: array<vec4<f32>, 4>, // rgb = its colour times its strength, the light at 1 m
    fire_axis: array<vec4<f32>, 4>,  // xyz = the axis of its beam, w = the angle off it where it is half as bright (0: all round)
    marks_a: array<vec4<f32>, 8>,    // xyz = centre of a scorched or dug patch of ground, relative to the camera, w = radius
    marks_b: array<vec4<f32>, 8>,    // x = radius of the hole, y = how hot it glows 0..1, z = a number 0..1 of its own
    foam: array<vec4<f32>, 8>,       // xyz = centre of a patch of foam on the water, relative to the camera, w = radius
    foam_b: array<vec4<f32>, 2>,     // the strength of each patch of foam, 0..1, four to a vector
    sea: vec4<f32>,         // x = wind m/s, y = trains of waves, z = significant wave height m, w = how many deviations of the height a crest breaks at
    sea_rough: vec4<f32>,   // xyz = slope covariance (xx, zz, xz) of what the trains leave out, w = part of the sea white with breaking crests
    sea_more: vec4<f32>,    // xy = the way the wind blows, z = where the ground ends under it, w = period of the waves at the shore s
    waves: array<vec4<f32>, 32>,     // per train (sea.rs): xy = the way it runs, z = waves per metre, w = phase at the camera in turns
    wave_rest: array<vec4<f32>, 32>, // xyz = slope covariance (xx, zz, xz) of the trains from this one on, w = this one's steepness
    lights: array<vec4<f32>, 3>,     // the aerodromes' lights: x = brightness per cd/m², y = a pixel's angle, zw = fade; then each system's setting
    sky_sun: vec4<f32>,     // xyz = towards the sun, below the horizon too, w = how far the aerial perspective reaches (sky.rs)
    sky_moon: vec4<f32>,    // xyz = towards the moon, w = the sun's light above the air (the scene's scale)
    moon_light: vec4<f32>,  // rgb = the moon's light above the air, w = 1 when it lights the world directly
    sky_more: vec4<f32>,    // xy = the sun's azimuth (x, z), unit, z = altitude of the ambient table's ground rows
    airports_c: array<vec4<f32>, 8>, // per airport: x = side of the runway its apron is on (1 right, -1 left, 0 none), y = 1 paved, 0 grass
    road_map: vec4<f32>,    // xy = road map centre (x, z) relative to the camera, z = half extent, w = 1 when ready (a region's roads)
    mirror: vec4<f32>,      // the sea's mirror (reflection.rs): y = 1 when the sea reads it, zw = 1 / screen size
    imagery: array<vec4<f32>, 2>,    // a real place's ground from the satellites (imagery.rs), near and far: xy = centre (x, z) relative to the camera, z = side in metres, w = 1 when there is one
    caster_right: vec4<f32>, // the vehicles' shadow map (casters.rs): xyz = its right axis across the sunlight, w = 1 when there is one
    caster_up: vec4<f32>,    // xyz = its up axis, w = the body's ShadowShape: 0 none, 1 it takes over from the first tile far below, 2 it is all the body's shadow
    caster_tiles: array<vec4<f32>, 4>, // per tile: xyz = its middle relative to the camera, w = half its side (0: unused)
};

// Per-draw data of terrain tiles, tree and house tiles and cloud slices. Mirrors `ChunkUniforms`.
struct Chunk {
    offset: vec4<f32>,
    wrap: vec4<f32>,
};

// Per-draw data of objects, lights and puffs. Mirrors `ObjectUniforms`.
struct Object {
    model: mat4x4<f32>,
    tint: vec4<f32>,
    cuts: array<vec4<f32>, 4>, // per cut: its plane (normal, offset) in mesh space, then the plane that bounds where it cuts
    keep: vec4<f32>,           // x, y: the side of each cut kept, 1 inside, -1 outside, 0 no cut
};

@group(0) @binding(0) var<uniform> frame: Frame;
// Heights above sea level around the camera, R16Float: half floats can be filtered by the hardware
// on every backend, and their precision (2 m at 3000 m) is plenty for shadows.
@group(0) @binding(1) var height_map: texture_2d<f32>;
@group(0) @binding(2) var height_sampler: sampler;
// How much of each texel of 3 m round the camera is road, with its mip levels: a region's roads (`roadmap.rs`).
@group(0) @binding(12) var road_map: texture_2d<f32>;
// The vehicles as the sun sees them (casters.rs): an atlas of two by two tiles of depths, 0 on the sun's
// side of a vehicle's sphere and 1 on the other, read with a comparison (`caster_tile`).
@group(0) @binding(13) var caster_map: texture_depth_2d;
@group(0) @binding(14) var caster_compare: sampler_comparison;
// A real place's ground as the satellites saw it (imagery.rs): the near picture (10 m) and the far (32 m), layers 0 and 1.
@group(2) @binding(0) var imagery: texture_2d_array<f32>;
// [parcels] Per grid of the picture, layer 2i: its parcels (number, distance to the edge in texels times
// 32, heading of the rows, kind), layer 2i + 1: their colours (sRGB) and how much each texel looks like trees.
@group(2) @binding(1) var parcels: texture_2d_array<f32>;
// The sky's tables (`sky.rs`): the air's transmittance, the sky seen from the eye, the aerial
// perspective (light scattered, light let through) and the ambient light, and their sampler.
@group(0) @binding(6) var sky_transmittance: texture_2d<f32>;
@group(0) @binding(7) var sky_view: texture_2d<f32>;
@group(0) @binding(8) var aerial_scatter: texture_3d<f32>;
@group(0) @binding(9) var aerial_through: texture_3d<f32>;
@group(0) @binding(10) var sky_ambient: texture_2d<f32>;
@group(0) @binding(11) var sky_sampler: sampler;

// The two kinds of per-draw data share group 1, binding 0; no entry point uses both.
@group(1) @binding(0) var<uniform> chunk: Chunk;
@group(1) @binding(0) var<uniform> object: Object;
// The sea's alone: the land, trees, buildings and lights mirrored in it, premultiplied, at a
// fraction of the screen's size (`reflection.rs`), drawn before the scene.
@group(1) @binding(1) var mirrored: texture_2d<f32>;

// ---------------------------------------------------------------- noise

fn ihash(p: vec2<i32>) -> f32 {
    var h: u32 = (u32(p.x) * 0x9E3779B1u) ^ (u32(p.y) * 0x85EBCA77u);
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    h *= 0x297A2D39u;
    h ^= h >> 15u;
    return f32(h & 0xFFFFFFu) / 16777216.0;
}

// Value noise that repeats every `period` cells, so wrapped coordinates have no seam.
fn vnoise(p: vec2<f32>, period: i32) -> f32 {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    var f = p - fl;
    f = f * f * (3.0 - 2.0 * f);
    let m = vec2<i32>(period - 1);
    let i0 = i & m;
    let i1 = (i + 1) & m;
    let a = ihash(i0);
    let b = ihash(vec2<i32>(i1.x, i0.y));
    let c = ihash(vec2<i32>(i0.x, i1.y));
    let d = ihash(i1);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// `v` modulo `m`, always in 0..m.
fn wrap_mod(v: vec2<f32>, m: f32) -> vec2<f32> {
    return v - m * floor(v / m);
}

// ---------------------------------------------------------------- sky
//
// The sky, the light of the sun, the moon and the sky on everything, and the haze between the eye
// and what it sees, all from one model of the air (`sky.rs`): read from the tables it makes
// (`sky.wgsl`; their layout is `sky_common.wgsl`). In the scene's units, as the rest.

// A direction (unit) as the tables hold it: the cosine of its azimuth from the sun and its height.
fn sky_place(d: vec3<f32>) -> vec2<f32> {
    let level = dot(d.xz, d.xz);
    let c = select(1.0, dot(d.xz, frame.sky_more.xy) * inverseSqrt(level), level > 1e-12);
    return vec2<f32>(c, d.y);
}

// The sky along `d` (unit): the light the air scatters towards the eye, without the sun's disc.
fn sky_color(d: vec3<f32>) -> vec3<f32> {
    let a = sky_place(d);
    return textureSampleLevel(sky_view, sky_sampler, sky_view_uv(a.x, a.y), 0.0).rgb;
}

// The sky along `d`, or its horizon for a direction below it: what a shiny surface mirrors.
fn sky_seen(d: vec3<f32>) -> vec3<f32> {
    return sky_color(normalize(vec3<f32>(d.x, max(d.y, 0.0), d.z)));
}

// The light that lights the world directly, the sun or by night the moon (`frame.sun_dir`), above the air.
fn direct_light_above() -> vec3<f32> {
    return select(vec3<f32>(frame.sky_moon.w), frame.moon_light.rgb, frame.moon_light.w > 0.5);
}

// That light where it reaches a point at `rel`: through the air above it, coloured by the way it
// came (`frame.sun_color` is the same at the eye).
fn light_at(rel: vec3<f32>) -> vec3<f32> {
    return direct_light_above() * light_through_table(sky_transmittance, sky_sampler, frame.cam_wrap.y + rel.y, frame.sun_dir.y);
}

// The sun's light and the moon's where they reach `rel`, whichever lights the world.
fn sun_light_at(rel: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(frame.sky_moon.w) * light_through_table(sky_transmittance, sky_sampler, frame.cam_wrap.y + rel.y, frame.sky_sun.y);
}

fn moon_light_at(rel: vec3<f32>) -> vec3<f32> {
    return frame.moon_light.rgb * light_through_table(sky_transmittance, sky_sampler, frame.cam_wrap.y + rel.y, frame.sky_moon.y);
}

// The light of the whole sky on a surface at `rel` facing `n`: the sky over it and the ground's
// light under it (`ambient` in sky.wgsl), between what it is at the ground and at the eye. Under
// clouds it is greyer.
fn sky_light(n: vec3<f32>, rel: vec3<f32>) -> vec3<f32> {
    let a = sky_place(n);
    let ground = textureSampleLevel(sky_ambient, sky_sampler, ambient_uv(a.x, a.y, 0.0), 0.0).rgb;
    let eye = textureSampleLevel(sky_ambient, sky_sampler, ambient_uv(a.x, a.y, 1.0), 0.0).rgb;
    let up = saturate((frame.cam_wrap.y + rel.y - frame.sky_more.z) / max(frame.cam_wrap.y - frame.sky_more.z, 1.0));
    let light = mix(ground, eye, up);
    return mix(light, vec3<f32>(luma(light)), 0.5 * frame.misc.w);
}

// What lies between the eye and `rel`: the light of the sun and the moon it scatters towards the
// eye, and what it lets through of the light from `rel`, per colour (blue is scattered first, so
// what is far turns blue and a light far off red).
struct Haze {
    through: vec3<f32>,
    light: vec3<f32>,
};

fn aerial_at(rel: vec3<f32>) -> vec3<f32> {
    let dist = length(rel);
    let a = sky_place(rel / max(dist, 1e-3));
    return aerial_uvw(a.x, a.y, dist, frame.sky_sun.w);
}

fn haze_at(rel: vec3<f32>) -> Haze {
    let uvw = aerial_at(rel);
    return Haze(textureSampleLevel(aerial_through, sky_sampler, uvw, 0.0).rgb, textureSampleLevel(aerial_scatter, sky_sampler, uvw, 0.0).rgb);
}

fn haze_through(rel: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(aerial_through, sky_sampler, aerial_at(rel), 0.0).rgb;
}

fn apply_fog(col: vec3<f32>, rel: vec3<f32>) -> vec3<f32> {
    let haze = haze_at(rel);
    return col * haze.through + haze.light;
}

// How much of the sun's disc a strip of half width `half`, `d` away, hides from a point whose
// view of the sun, `t` metres away, is `w` wide: 1 in the umbra, fading through the penumbra,
// and never all of it once the strip is narrower than the sun seen from there.
fn sun_cover(d: f32, half: f32, w: f32) -> f32 {
    return saturate((min(d + half, w * 0.5) - max(d - half, -w * 0.5)) / w);
}

// How much of the sun's disc a solid piece of the body (`ShadowShape::solids`: an ellipsoid or a
// rod, rows `a` and `b`) hides from a point `p` whose way to the sun is `s`, both in body axes: by
// how far the ray passes outside it against the width of the sun seen from there, never all of
// it once the piece is narrower than that. Art-directed: a little softer than the sun's disc
// alone makes it, so that a tyre's shadow is crisp at its foot and blurs up its height.
fn solid_cover(p: vec3<f32>, s: vec3<f32>, a: vec4<f32>, b: vec4<f32>) -> f32 {
    var t = 0.0;
    var miss = 0.0;
    var size = 0.0;
    if a.w == 1.0 {
        // In the ellipsoid's own scale it is a unit sphere: the ray's nearest pass to its centre.
        let o = (p - a.xyz) / b.xyz;
        let dv = s / b.xyz;
        if dot(o, o) < 1.0 {
            return 1.0;
        }
        t = -dot(o, dv) / dot(dv, dv);
        let q = o + t * dv;
        let lq = length(q);
        // Back to metres along the way it misses by.
        let r = length(b.xyz * (q / max(lq, 1e-5)));
        miss = (lq - 1.0) * r;
        size = r;
    } else if a.w == 2.0 {
        // The nearest pass of the ray p + t s to the segment from a to b.
        let e = b.xyz - a.xyz;
        let w0 = p - a.xyz;
        let ee = dot(e, e);
        let se = dot(s, e);
        let d = ee - se * se;
        var u = 0.0;
        if d > 1e-6 {
            u = clamp((dot(e, w0) - se * dot(s, w0)) / d, 0.0, 1.0);
        }
        t = dot(a.xyz + u * e - p, s);
        let q = p + t * s - (a.xyz + u * e);
        miss = length(q) - b.w;
        size = b.w;
    } else {
        return 0.0;
    }
    if t <= 0.0 {
        return 0.0;
    }
    let w = 0.012 * t + 0.01;
    return saturate(0.5 - miss / w) * min(1.0, 2.0 * size / w);
}

// The sky the body's solid pieces hide from the ground at `rel`: a soft darkening under and round
// each ellipsoid that comes down near it (a tyre where it stands, a fairing just above), fading as
// the gap below it grows. Art-directed, not worked out. 1 where none is. Also under an
// overcast, where it is all the shadow there is.
fn body_contact(rel: vec3<f32>) -> f32 {
    let reach = frame.shadow_reach.x;
    let d = rel - frame.body_pos.xyz;
    let far = reach + 2.0;
    if reach <= 0.0 || dot(d, d) > far * far {
        return 1.0;
    }
    let p = vec3<f32>(dot(d, frame.body_fwd.xyz), dot(d, frame.body_right.xyz), dot(d, frame.body_down.xyz));
    // It dies away over the outer half of that distance: cut off there, a wide flat piece (a
    // car's floor) left a circle on the ground round the body.
    let fade = 1.0 - smoothstep(0.5 * far, far, length(d));
    var lit = 1.0;
    for (var i = 0; i < 9; i++) {
        let a = frame.shadow_solids[2 * i];
        if a.w == 1.0 {
            let b = frame.shadow_solids[2 * i + 1];
            // How far out of its footprint (a little wider than the piece), and how far below
            // its lowest point, as a share of its thickness (body z is down).
            let o = (p.xy - a.xy) / (b.xy * 1.8 + 0.1);
            let h2 = dot(o, o);
            let gap = max(p.z - (a.z + b.z), 0.0);
            let under = 1.0 / (1.0 + h2 * h2);
            lit *= 1.0 - CONTACT_DARK * fade * under * exp(-gap / (0.5 * min(b.x, b.y) + 0.08));
        }
    }
    return lit;
}

// Shadow of the body the view belongs to (`ShadowShape`): flat pieces in its axes (x forward, y right,
// z down), each (centre, half) along x and along the other axis of the plane it lies in. Lying flat
// at a height z, `shadow_flat` (y across) at `shadow_flat_z`; standing in the plane y = 0,
// `shadow_upright` (z across). Where the ray to the sun crosses each piece, how much of the sun's
// disc (0.53° across) it hides. Far below the body the shadow is all penumbra and fades by itself.
fn shape_shadow(rel: vec3<f32>) -> f32 {
    let d = rel - frame.body_pos.xyz;
    if dot(d, d) > 640000.0 {
        return 0.0;
    }
    let sun = frame.sun_dir.xyz;
    let p = vec3<f32>(dot(d, frame.body_fwd.xyz), dot(d, frame.body_right.xyz), dot(d, frame.body_down.xyz));
    let s = vec3<f32>(dot(sun, frame.body_fwd.xyz), dot(sun, frame.body_right.xyz), dot(sun, frame.body_down.xyz));
    var shade = 0.0;
    let flat = frame.shadow_flat;
    let flat_z = frame.shadow_flat_z;
    let upright = frame.shadow_upright;
    if abs(s.z) > 1e-3 {
        for (var i = 0; i < 4; i++) {
            let t = (flat_z[i] - p.z) / s.z;
            if t > 0.0 {
                let h = p.xy + t * s.xy;
                let w = 0.0093 * t + 0.02;
                shade = max(shade, sun_cover(flat[i].x - h.x, flat[i].y, w) * sun_cover(flat[i].z - h.y, flat[i].w, w));
            }
        }
    }
    // The solid pieces (the gear), only where the ray to the sun passes near the body.
    let reach = frame.shadow_reach.x;
    if reach > 0.0 {
        let along = -dot(p, s);
        let miss = p + along * s;
        let w_far = 0.012 * max(along + reach, 0.0) + 0.01;
        if along > -reach && dot(miss, miss) < (reach + w_far) * (reach + w_far) {
            for (var i = 0; i < 9; i++) {
                shade = max(shade, solid_cover(p, s, frame.shadow_solids[2 * i], frame.shadow_solids[2 * i + 1]));
            }
        }
    }
    if abs(s.y) > 1e-3 {
        let t = -p.y / s.y;
        if t > 0.0 {
            let h = p.xz + t * s.xz;
            let w = 0.0093 * t + 0.02;
            for (var i = 0; i < 3; i++) {
                shade = max(shade, sun_cover(upright[i].x - h.x, upright[i].y, w) * sun_cover(upright[i].z - h.y, upright[i].w, w));
            }
        }
    }
    return shade;
}

// ---------------------------------------------------------------- the vehicles' shadows from their meshes

// Texels of a tile's side (`casters::TILE`); the atlas is two tiles a side.
const CASTER_TILE: f32 = 1024.0;
// Metres past the far side of a vehicle over which its map gives way: to the body's ShadowShape
// (an aircraft in the air), or to nothing for the others, whose shadow is then too soft to tell.
const CASTER_FADE_FROM: f32 = 10.0;
const CASTER_FADE_TO: f32 = 28.0;
// How much wider than the sun's disc (0.53°) the penumbra is drawn: by eye, a little softer than
// a photograph's, so that it reads as a shadow and not as a cut-out.
const CASTER_SOFT: f32 = 1.6;
// Points in a disc of radius 1, spread evenly: where the edge of a shadow is sampled round a pixel.
const CASTER_DISC = array<vec2<f32>, 12>(
    vec2<f32>(-0.326, -0.406), vec2<f32>(-0.840, -0.074), vec2<f32>(-0.696, 0.457), vec2<f32>(-0.203, 0.621),
    vec2<f32>(0.962, -0.195), vec2<f32>(0.473, -0.480), vec2<f32>(0.519, 0.767), vec2<f32>(0.185, -0.893),
    vec2<f32>(0.507, 0.064), vec2<f32>(0.896, 0.412), vec2<f32>(-0.322, -0.933), vec2<f32>(-0.792, -0.598),
);

// How much of the sun tile `i` of the vehicles' map hides from the point `rel` (relative to the
// camera), pushed `bias` metres towards the sun, at `along` metres past the middle of the tile's
// sphere and at (x, y) across the tile (-1 to 1): 0 to 1.
//
// The point is looked up in the tile along the sunlight. First the vehicle's depth round it, to
// know how far behind what hides the sun the point is; then the penumbra as wide as the sun's
// disc makes it from there (`CASTER_SOFT` times), sampled at twelve points of a disc, each a
// comparison of four texels: sharp where a tyre stands on the road, soft under a wing tip.
fn caster_tile(i: i32, along: f32, x: f32, y: f32) -> f32 {
    let r = frame.caster_tiles[i].w;
    let depth = min(0.5 + along / (2.0 * r), 1.0);
    // The tile in the atlas: the atlas's coordinates per metre, and its corner.
    let per_metre = 0.25 / r;
    let corner = vec2<f32>(f32(i % 2), f32(i / 2)) * 0.5;
    let uv = corner + (vec2<f32>(x, -y) * 0.5 + 0.5) * 0.5;
    let texel = 0.5 / CASTER_TILE;
    let lo = corner + vec2<f32>(texel);
    let hi = corner + vec2<f32>(0.5 - texel);
    // What hides the sun round the point, within 8 cm: how far in front of it, on average.
    let size = vec2<f32>(2.0 * CASTER_TILE);
    var blockers = 0.0;
    var nearer = 0.0;
    for (var k = 0; k < 5; k++) {
        let at = clamp(uv + CASTER_DISC[k * 2] * (0.08 * per_metre * f32(min(k, 1))), lo, hi);
        let z = textureLoad(caster_map, vec2<i32>(at * size), 0);
        if z < depth {
            blockers += 1.0;
            nearer += z;
        }
    }
    if blockers == 0.0 {
        return 0.0;
    }
    let gap = (depth - nearer / blockers) * 2.0 * r;
    // The penumbra's half width, metres: the sun's disc from that far, and never under a texel and
    // a half (the map's own steps).
    let half = max(0.5 * 0.0093 * CASTER_SOFT * gap, 1.5 * r / CASTER_TILE);
    // Deep in the shadow (the vehicle all round the point, its penumbra narrower than where that
    // was looked): no need to look closer. Most of a cabin seen from its seat is so.
    if blockers == 5.0 && half < 0.04 {
        return 1.0;
    }
    let spread = min(half, 0.35) * per_metre;
    var lit = 0.0;
    for (var k = 0; k < 12; k++) {
        let at = clamp(uv + CASTER_DISC[k] * spread, lo, hi);
        lit += textureSampleCompareLevel(caster_map, caster_compare, at, depth);
    }
    return 1.0 - lit / 12.0;
}

// How much of the sun the vehicles' maps hide from `rel`, pushed `bias` metres towards the sun:
// from the first tile whose vehicle stands between the point and the sun (tiles seldom overlap),
// fading 10 to 28 m beyond the far side of its sphere. With `shape`, the body's ShadowShape takes
// over where the first tile fades (`caster_up.w` 1), or is its whole shadow (2).
fn caster_shade(rel: vec3<f32>, bias: f32, shape: bool) -> f32 {
    let sun = frame.sun_dir.xyz;
    var shade = 0.0;
    var beyond = 0.0;
    // The tile found: which, and where in it the point is (along, x, y, how far past its reach).
    var hit = -1;
    var at = vec4<f32>(0.0);
    for (var i = 0; i < 4; i++) {
        let t = frame.caster_tiles[i];
        if t.w <= 0.0 {
            break;
        }
        let d = rel - t.xyz;
        let along = -dot(d, sun) - bias;
        let far = smoothstep(CASTER_FADE_FROM, CASTER_FADE_TO, along - t.w);
        if i == 0 {
            beyond = far;
        }
        let x = dot(d, frame.caster_right.xyz) / t.w;
        let y = dot(d, frame.caster_up.xyz) / t.w;
        if far < 1.0 && along > -t.w && max(abs(x), abs(y)) < 0.995 {
            hit = i;
            at = vec4<f32>(along, x, y, far);
            break;
        }
    }
    if hit >= 0 {
        shade = caster_tile(hit, at.x, at.y, at.z) * (1.0 - at.w);
    }
    if shape {
        let w = frame.caster_up.w;
        let k = select(select(0.0, beyond, w == 1.0), 1.0, w == 2.0);
        if k > 0.0 {
            shade = max(shade, shape_shadow(rel) * k);
        }
    }
    return shade;
}

// Shadow of the vehicles on what is round them (ground, sea, trees, grass, buildings, the circuit):
// from the vehicles' map when there is one (`caster_tile`), else from the body's `ShadowShape`.
// 1 where the sun is not hidden.
fn body_shadow(rel: vec3<f32>) -> f32 {
    let strength = frame.body_pos.w;
    if strength <= 0.0 {
        return 1.0;
    }
    if frame.caster_right.w <= 0.0 {
        return 1.0 - shape_shadow(rel) * strength;
    }
    return 1.0 - caster_shade(rel, 0.0, true) * strength;
}

// Shadow of the vehicles on a vehicle (itself and the others: a wing on its fuselage, a mirror on
// a door), from their map alone, at `rel` on a surface facing `n`: the point is lifted off the
// surface by a centimetre, so that a face does not shade itself.
fn vehicle_shadow(rel: vec3<f32>, n: vec3<f32>) -> f32 {
    let strength = frame.body_pos.w;
    if strength <= 0.0 || frame.caster_right.w <= 0.0 {
        return 1.0;
    }
    return 1.0 - caster_shade(rel + n * 0.01, 0.01, false) * strength;
}

// ---------------------------------------------------------------- the light of fires, scorched ground, foam

// The lights of the ground (`FrameUniforms::fire_pos`: fires, floodlights, vehicles' beams) on a
// surface at `rel` facing `n`, falling off with the square of the distance, and off the axis of a
// beam. Nothing to do when there is none.
fn fire_light(rel: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var l = vec3<f32>(0.0);
    for (var i = 0; i < i32(frame.fire_count.x); i++) {
        let d = frame.fire_pos[i].xyz - rel;
        let r2 = dot(d, d) + 1.0;
        l += frame.fire_color[i].rgb * max(dot(n, d) * inverseSqrt(r2), 0.0) / r2 * in_beam(i, d * inverseSqrt(r2));
    }
    return l;
}

// How bright the light `i` is towards `back` (unit, from what it lights to it): 1 all round, and
// along a beam a Gaussian of the angle off its axis, half at its half width, over what it spills.
fn in_beam(i: i32, back: vec3<f32>) -> f32 {
    let beam = frame.fire_axis[i];
    if beam.w <= 0.0 {
        return 1.0;
    }
    let off = acos(clamp(-dot(back, beam.xyz), -1.0, 1.0)) / beam.w;
    return max(exp(-0.6931 * off * off), frame.fire_pos[i].w);
}

// The same light on gas or spray, which takes it from every side.
fn fire_glow(rel: vec3<f32>) -> vec3<f32> {
    var l = vec3<f32>(0.0);
    for (var i = 0; i < i32(frame.fire_count.x); i++) {
        let d = frame.fire_pos[i].xyz - rel;
        l += frame.fire_color[i].rgb / (dot(d, d) + 4.0) * in_beam(i, d * inverseSqrt(dot(d, d) + 1e-4));
    }
    return l;
}

// The scene's value of the light of a fire as bright as soot at 1900 K (heat.wgsl): nearly twice
// what the sun lights white, by day and by night, so that a deep flame is a bright orange, its
// hottest tongues yellow, and the picture's exposure decides the rest (a fire by night glares).
fn fire_light_scale() -> f32 {
    return 4.8;
}

// What a crash did to the ground at `rel` (`FrameUniforms::marks_a`): x = how scorched, 0..1,
// y = how much it is the earth dug up from a hole, z = how hot it still glows, and in `bend` the
// lean of the ground in and round the hole.
struct Marked {
    scorch: f32,
    earth: f32,
    embers: f32,
    bend: vec3<f32>,
};

fn ground_marks(rel: vec3<f32>) -> Marked {
    var m = Marked(0.0, 0.0, 0.0, vec3<f32>(0.0));
    for (var i = 0; i < i32(frame.fire_count.y); i++) {
        let a = frame.marks_a[i];
        let b = frame.marks_b[i];
        let local = rel.xz - a.xz;
        let d = length(local);
        if d > a.w * 1.3 + b.x * 2.5 || abs(rel.y - a.y) > a.w + 8.0 {
            continue;
        }
        let offset = vec2<f32>(b.z * 371.0, b.z * 113.0);
        // A ragged edge, blotched inside: burnt grass and soot, not a disc.
        let rough = vnoise(local * (2.5 / max(a.w, 1.0)) + offset, 1024) * 0.5 + vnoise(local * 0.9 + offset, 1024) * 0.3 + vnoise(local * 2.7 + offset, 1024) * 0.2;
        let edge = a.w * (0.45 + 0.9 * rough);
        let burnt = (1.0 - smoothstep(edge * 0.35, edge, d)) * (0.6 + 0.4 * vnoise(local * 3.1 + offset, 1024));
        m.scorch = max(m.scorch, burnt);
        let fine = vnoise(local * 6.0 + offset, 1024);
        m.embers = max(m.embers, b.y * burnt * smoothstep(0.8, 0.95, fine));
        if b.x > 0.0 {
            // The hole: a bowl, and the earth thrown out of it in a ring.
            let r = b.x * (0.85 + 0.3 * vnoise(local * (3.0 / b.x) + offset, 1024));
            let bowl = 1.0 - smoothstep(r * 0.7, r * 1.05, d);
            let thrown = (1.0 - smoothstep(r, r * 2.2, d)) * (0.5 + 0.5 * vnoise(local * 1.7 + offset, 1024));
            m.earth = max(m.earth, max(bowl, thrown * 0.8));
            let out = vec3<f32>(local.x, 0.0, local.y) / max(d, 1e-3);
            m.bend += -out * bowl * 0.8 * smoothstep(0.0, r * 0.7, d) + out * (1.0 - smoothstep(r, r * 1.5, d)) * smoothstep(r * 0.8, r, d) * 0.5;
        }
    }
    return m;
}

// How much foam lies on the water at `rel` (`FrameUniforms::foam`), 0..1.
fn foam_at(rel: vec3<f32>) -> f32 {
    var f = 0.0;
    for (var i = 0; i < i32(frame.fire_count.z); i++) {
        let a = frame.foam[i];
        let local = rel.xz - a.xz;
        let d = length(local);
        if d > a.w * 1.2 {
            continue;
        }
        let strength = frame.foam_b[i / 4][i % 4];
        let lace = vnoise(local * 0.7 + a.w, 1024) * 0.6 + vnoise(local * 2.3, 1024) * 0.4;
        // Thick in the middle, broken into lace towards the edge.
        let inside = 1.0 - smoothstep(a.w * 0.3, a.w * 1.1, d + (lace - 0.5) * a.w * 0.5);
        let spot = inside * smoothstep(0.3, 0.6, lace + 0.5 * inside * strength);
        f = max(f, spot * strength);
    }
    return f;
}

// ---------------------------------------------------------------- clouds and shadows

fn cloud_coord(rel_xz: vec2<f32>) -> vec2<f32> {
    return wrap_mod(rel_xz + frame.cam_wrap.xz + WRAP * 32.0 + vec2<f32>(frame.sun_dir.w * 6.0, 0.0), WRAP * 2.0);
}

fn cloud_noise(w: vec2<f32>, dist: f32) -> f32 {
    var n = vnoise(w / 4096.0, 8) * 0.55 + vnoise(w / 1024.0, 32) * 0.27;
    if dist < 40000.0 {
        n += vnoise(w / 256.0, 128) * 0.12 + vnoise(w / 64.0, 512) * 0.06;
    } else {
        n += 0.09;
    }
    return n;
}

// The light on the underside of the cloud layer at `rel`: what the sun and the sky give its base
// through the cloud above it.
fn cloud_underside(rel: vec3<f32>) -> vec3<f32> {
    return light_at(rel) * max(frame.sun_dir.y, 0.0) * 0.22 + sky_light(vec3<f32>(0.0, 1.0, 0.0), rel) * 0.5;
}

// Shade of the cloud layer on a point below it.
fn cloud_shadow(rel: vec3<f32>) -> f32 {
    let sun = frame.sun_dir.xyz;
    if frame.cam_wrap.w > 0.5 {
        // The volumetric clouds: their shade is drawn every frame beside the heights (`clouds.rs`),
        // by where the ray to the sun crosses the level of their bases.
        let rise = frame.misc.y - rel.y;
        if rise <= 0.0 || sun.y < 0.03 || frame.hmap.w < 0.5 {
            return 1.0;
        }
        let uv = (rel.xz + sun.xz / sun.y * rise - frame.hmap.xy) / (2.0 * frame.hmap.z) + 0.5;
        let off = max(abs(uv.x - 0.5), abs(uv.y - 0.5));
        return 1.0 - 0.82 * textureSampleLevel(height_map, height_sampler, uv, 0.0).g * (1.0 - smoothstep(0.44, 0.5, off));
    }
    let up = frame.misc.y + frame.extra.x * 0.4 - rel.y;
    if frame.misc.w < 0.01 || up <= 0.0 || sun.y < 0.03 {
        return 1.0;
    }
    let xz = rel.xz + sun.xz / sun.y * up;
    let th = mix(0.90, 0.36, frame.misc.w);
    let n = cloud_noise(cloud_coord(xz), 50000.0);
    return 1.0 - 0.62 * smoothstep(th + 0.02, th + 0.16, n) * (1.0 - smoothstep(40000.0, 70000.0, length(xz)));
}

// The height map at `uv`, interpolated between texels and clamped at the edges.
fn height_at(uv: vec2<f32>) -> f32 {
    return textureSampleLevel(height_map, height_sampler, uv, 0.0).r;
}

fn outside(uv: vec2<f32>) -> bool {
    return any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0));
}

// Shadow of the relief: walk towards the sun over a height map of the surroundings.
fn terrain_shadow(rel: vec3<f32>) -> f32 {
    return relief_shadow(rel, RELIEF_STEPS, RELIEF_GROWTH);
}

// The same with a third of the steps, three times as long: for the trees, which ask it at every
// pixel of every card and are much smaller than the relief's shadows.
fn coarse_terrain_shadow(rel: vec3<f32>) -> f32 {
    return relief_shadow(rel, (RELIEF_STEPS + 2) / 3, RELIEF_GROWTH * RELIEF_GROWTH * RELIEF_GROWTH);
}

fn relief_shadow(rel: vec3<f32>, steps: i32, growth: f32) -> f32 {
    let sun = frame.sun_dir.xyz;
    if frame.hmap.w < 0.5 || sun.y < 0.01 {
        return 1.0;
    }
    let extent = 2.0 * frame.hmap.z;
    let uv0 = (rel.xz - frame.hmap.xy) / extent + 0.5;
    if outside(uv0) {
        return 1.0;
    }
    // Start from the map's own idea of the ground here, so its coarseness does not shade flat land.
    let ground = max(rel.y, height_at(uv0) - frame.cam_wrap.y) + 6.0;
    var shade = 1.0;
    var t = 90.0;
    for (var i = 0; i < steps; i++) {
        let xz = rel.xz + sun.xz * t;
        let uv = (xz - frame.hmap.xy) / extent + 0.5;
        if outside(uv) {
            break;
        }
        let h = height_at(uv) - frame.cam_wrap.y;
        let clearance = ground + sun.y * t - h;
        shade = min(shade, saturate(0.5 + clearance / (0.07 * t)));
        if shade < 0.02 {
            break;
        }
        t *= growth;
    }
    return shade;
}

// ---------------------------------------------------------------- sky pass

struct SkyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn sky_vertex(@builtin(vertex_index) vid: u32) -> SkyOut {
    let p = vec2<f32>(select(-1.0, 3.0, vid == 1u), select(-1.0, 3.0, vid == 2u));
    var o: SkyOut;
    o.position = vec4<f32>(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

// The discs of the sun and the moon are drawn this many times as bright as a white surface they
// light: their real radiance, some 46 000 times that, would be all glare once bloomed.
const DISC: f32 = 40.0;
// A star of magnitude 0 lights the ground with 2.5 microlux: this share of the sun's light.
const STAR_ZERO: f32 = 1.95e-11;

@fragment
fn sky_fragment(in: SkyOut) -> @location(0) vec4<f32> {
    let d = normalize(frame.cam_fwd.xyz + in.ndc.x * frame.cam_right.w * frame.cam_right.xyz + (in.ndc.y - frame.cam_fwd.w) * frame.cam_up.w * frame.cam_up.xyz);
    // Under the horizon of the flat world the horizon's sky: the ground or the sea is drawn over it.
    var col = sky_seen(d);
    let eye = frame.cam_wrap.y;
    // The discs of the sun and the moon, through the air before them.
    let sun = frame.sky_sun.xyz;
    let moon = frame.sky_moon.xyz;
    col += vec3<f32>(frame.sky_moon.w * DISC) * light_through_table(sky_transmittance, sky_sampler, eye, sun.y) * smoothstep(0.99996, 0.99999, dot(d, sun));
    col += frame.moon_light.rgb * DISC * light_through_table(sky_transmittance, sky_sampler, eye, moon.y) * smoothstep(0.99996, 0.99999, dot(d, moon));
    // The stars, by their magnitudes (a few bright, many faint), through the air: by day the sky
    // drowns them.
    if d.y > 0.0 {
        let sp = d.xz / (d.y + 0.4) * 180.0;
        let h = ihash(vec2<i32>(floor(sp)));
        if h > 0.994 {
            let c = fract(sp) - 0.5;
            let magnitude = 4.5 - 5.5 * pow(fract(h * 977.0), 3.0);
            // The dot covers 0.059 of its cell, which spans this angle.
            let cell = (d.y + 0.4) / 180.0;
            let radiance = PI * frame.sky_moon.w * STAR_ZERO * pow(10.0, -0.4 * magnitude) / (0.059 * cell * cell);
            // The original is smoothstep(0.25, 0.0, length(c)): edges in descending order, written the other way round.
            col += radiance * (1.0 - smoothstep(0.0, 0.25, length(c))) * light_through_table(sky_transmittance, sky_sampler, eye, d.y);
        }
    }
    // Alpha: no foam (the sea beyond the ground is drawn over the sky, `sea_fragment`).
    return vec4<f32>(col, 0.0);
}

// ---------------------------------------------------------------- [materials] photographs of ground and buildings
//
// Two texture arrays with one layer per material (`materials.rs` has the order) give the detail of
// the ground, the runway, the roads and the buildings. They never give the colour: a fetch returns
// the photograph divided by its average, which multiplies the colour the game had before, so the
// landscape keeps its look from the air and gains its grain close up. The method, with the reasons,
// is in assets/textures/APPLYING.md. Everything of this job is marked [materials].

// What the quality setting changes: `pipelines.rs` rewrites these four lines for the lower qualities.
const TEXTURES: bool = true;
const TEX_RANGE: f32 = 4000.0;
const TEX_RELIEF: f32 = 1500.0;
const TEX_SCATTER: bool = true;

@group(0) @binding(3) var mat_colour: texture_2d_array<f32>;
// rg = normal x and y (OpenGL convention, z is not needed), b = roughness, a = ambient occlusion.
@group(0) @binding(4) var mat_detail: texture_2d_array<f32>;
@group(0) @binding(5) var mat_sampler: sampler;

const M_GRASS_SHORT: i32 = 0;
const M_GRASS_MEADOW: i32 = 1;
const M_GRASS_DRY: i32 = 2;
const M_SOIL_PLOUGHED: i32 = 3;
const M_CROP_STUBBLE: i32 = 4;
const M_CROP_GREEN: i32 = 5;
const M_FOREST_FLOOR: i32 = 6;
const M_ROCK_GRANITE: i32 = 7;
const M_ROCK_LAYERED: i32 = 8;
const M_SCREE: i32 = 9;
const M_SNOW: i32 = 10;
const M_SAND: i32 = 11;
const M_ASPHALT: i32 = 12;
const M_CONCRETE: i32 = 13;
const M_DIRT_ROAD: i32 = 14;
const M_METAL_PAINTED: i32 = 15;
const M_METAL_CORRUGATED: i32 = 16;
const M_PLASTER: i32 = 17;
const M_ROOF_CLAY: i32 = 18;
const M_ROOF_SLATE: i32 = 19;

// Metres one repeat covers close up, per layer. On the ground a repeat must fit a whole number of
// times in MAT_PERIOD, and so must MAT_FAR repeats: 2, 2.56, 3.2, 4, 5.12 and 6.4 do.
const MAT_TILE = array<f32, 20>(3.2, 4.0, 5.12, 4.0, 4.0, 4.0, 4.0, 3.2, 4.0, 6.4, 4.0, 3.2, 4.0, 2.56, 4.0, 2.0, 3.2, 2.0, 4.0, 2.0);
// How many times bigger the second scale of the ground is: the one seen from the air.
const MAT_FAR: f32 = 8.0;
// The ground's texture coordinates are metres modulo this, which divides WRAP: small enough for a
// float to resolve a texel (4 mm) and, being a multiple of every repeat, without a seam.
const MAT_PERIOD: f32 = 1024.0;

// What a material is at a point. `color` is the photograph over its average (1 = the average);
// `lean` is how far the normal tilts along the two axes of the mapping.
struct Mat {
    color: vec3<f32>,
    lean: vec2<f32>,
    rough: f32,
    ao: f32,
};

// The same in the world: `lean` is added to the normal of the surface.
struct Surface {
    color: vec3<f32>,
    lean: vec3<f32>,
    rough: f32,
    ao: f32,
};

fn mat_none(layer: i32) -> Mat {
    return Mat(vec3<f32>(1.0), vec2<f32>(0.0), frame.materials[layer].w, 1.0);
}

// One fetch of the colour and, with `relief`, one of the detail. Gradients are explicit so that it
// can be called inside branches without upsetting the choice of mip level.
fn mat_fetch(uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>, relief: bool) -> Mat {
    var m = mat_none(layer);
    let ratio = textureSampleGrad(mat_colour, mat_sampler, uv, layer, dx, dy).rgb / max(frame.materials[layer].rgb, vec3<f32>(0.004));
    // Half of the change of hue is kept: all of it, on a ground of another colour than the
    // photograph, turns straw among green shoots pink.
    m.color = mix(vec3<f32>((ratio.r + ratio.g + ratio.b) / 3.0), ratio, 0.5);
    if relief {
        let d = textureSampleGrad(mat_detail, mat_sampler, uv, layer, dx, dy);
        m.lean = d.rg * 2.0 - 1.0;
        m.rough = d.b;
        m.ao = d.a;
    }
    return m;
}

// A fetch without visible repetition (after Inigo Quilez): `scatter` is a slow noise times 8 that
// divides the ground into irregular bands, each reading the texture at its own offset; where two
// bands meet both are fetched and blended.
fn mat_fetch_scattered(uv: vec2<f32>, layer: i32, dx: vec2<f32>, dy: vec2<f32>, relief: bool, scatter: f32) -> Mat {
    if !TEX_SCATTER {
        return mat_fetch(uv, layer, dx, dy, relief);
    }
    let i = floor(scatter);
    let f = scatter - i;
    let uv_a = uv + sin(vec2<f32>(3.0, 7.0) * i);
    let uv_b = uv + sin(vec2<f32>(3.0, 7.0) * (i + 1.0));
    if f < 0.15 {
        return mat_fetch(uv_a, layer, dx, dy, relief);
    }
    if f > 0.85 {
        return mat_fetch(uv_b, layer, dx, dy, relief);
    }
    let a = mat_fetch(uv_a, layer, dx, dy, relief);
    let b = mat_fetch(uv_b, layer, dx, dy, relief);
    let d = a.color - b.color;
    let t = smoothstep(0.2, 0.8, f - clamp(0.03 * (d.r + d.g + d.b), -0.05, 0.05));
    return Mat(mix(a.color, b.color, t), mix(a.lean, b.lean, t), mix(a.rough, b.rough, t), mix(a.ao, b.ao, t));
}

// A material laid on a plane at two scales. `p` is the position on the plane in metres, `gx` and
// `gy` its change from one pixel to the next. Close up the small scale gives the grain and the big
// one varies it, so no two repeats look alike; from some 200 m on only the big one is left, and by
// TEX_RANGE that has faded into the average. The lean is along the axes of `p`.
fn mat_planar(layer: i32, p: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>, dist: f32, scatter: f32) -> Mat {
    var tiles = MAT_TILE;
    let tile = tiles[layer];
    let far = tile * MAT_FAR;
    var m = mat_none(layer);
    let gone = smoothstep(TEX_RANGE * 0.375, TEX_RANGE, dist);
    if gone >= 1.0 {
        return m;
    }
    let relief = select(0.0, 1.0 - smoothstep(TEX_RELIEF * 0.5, TEX_RELIEF, dist), TEX_RELIEF > 0.0);
    // The big scale has its axes swapped, so its pattern never lines up with the small one's.
    let f = mat_fetch_scattered(p.yx / far, layer, gx.yx / far, gy.yx / far, relief > 0.0, scatter);
    // Without the scattered fetch the repeats of the big scale show from the air as a grid: it is
    // then left at half its contrast once the small scale is gone.
    let faint = select(0.5 * smoothstep(tile * 10.0, tile * 60.0, dist), 0.0, TEX_SCATTER);
    let k = smoothstep(tile * 10.0, tile * 60.0, dist);
    // On grass the big scale is the finest grain left once the small one is gone: a third more of
    // its contrast keeps the air patches from looking smeared. Not on crops: raised there, their
    // blades show from 150 m as strokes.
    let lift = select(0.0, 0.35 * k, layer <= M_GRASS_DRY);
    m.color = mix(vec3<f32>(1.0), f.color, (1.0 + lift) * (1.0 - max(gone, faint)));
    // Red leans along +u and green up the image: with the axes swapped that is +y and -x of `p`.
    m.lean = vec2<f32>(-f.lean.y, f.lean.x) * relief * (0.6 + 0.3 * k);
    m.rough = f.rough;
    m.ao = mix(1.0, f.ao, relief);
    if k < 1.0 {
        let n = mat_fetch(p / tile, layer, gx / tile, gy / tile, TEX_RELIEF > 0.0);
        // Under the small scale the big one counts for half: full, its blotches are too marked from the ground.
        m.color = mix(n.color * mix(vec3<f32>(1.0), m.color, 0.5), m.color, k);
        m.lean += vec2<f32>(n.lean.x, -n.lean.y) * (1.0 - k);
        m.rough = mix(n.rough, m.rough, k);
        m.ao = mix(n.ao * m.ao, m.ao, k);
    }
    // A dark crack must not go black nor a pebble blow out.
    m.color = clamp(m.color, vec3<f32>(0.4), vec3<f32>(2.2));
    return m;
}

// A material on the ground. Rock is mapped from three sides (triplanar), each weighted by how
// much the slope faces it, so that it does not stretch down a cliff; its vertical coordinate is the
// height above the sea. Everything else is mapped from above. `tex` is TerrainOut.tex, `gh` the
// change of the height from one pixel to the next (x: across, y: down the screen).
fn mat_ground(layer: i32, tex: vec2<f32>, height: f32, n: vec3<f32>, gx: vec2<f32>, gy: vec2<f32>, gh: vec2<f32>, dist: f32, scatter: f32) -> Surface {
    if layer != M_ROCK_GRANITE && layer != M_ROCK_LAYERED {
        let m = mat_planar(layer, tex, gx, gy, dist, scatter);
        return Surface(m.color, vec3<f32>(m.lean.x, 0.0, m.lean.y), m.rough, m.ao);
    }
    var wt = pow(abs(n), vec3<f32>(6.0));
    // A side that hardly counts is not fetched.
    wt = max(wt / (wt.x + wt.y + wt.z) - vec3<f32>(0.1), vec3<f32>(0.0));
    wt /= wt.x + wt.y + wt.z;
    var s = Surface(vec3<f32>(0.0), vec3<f32>(0.0), 0.0, 0.0);
    if wt.y > 0.0 {
        let m = mat_planar(layer, tex, gx, gy, dist, scatter);
        s.color += m.color * wt.y;
        s.lean += vec3<f32>(m.lean.x, 0.0, m.lean.y) * wt.y;
        s.rough += m.rough * wt.y;
        s.ao += m.ao * wt.y;
    }
    if wt.x > 0.0 {
        // Seen along x: u runs south, v down.
        let m = mat_planar(layer, vec2<f32>(tex.y, -height), vec2<f32>(gx.y, -gh.x), vec2<f32>(gy.y, -gh.y), dist, scatter);
        s.color += m.color * wt.x;
        s.lean += vec3<f32>(0.0, -m.lean.y, m.lean.x) * wt.x;
        s.rough += m.rough * wt.x;
        s.ao += m.ao * wt.x;
    }
    if wt.z > 0.0 {
        // Seen along z: u runs east, v down.
        let m = mat_planar(layer, vec2<f32>(tex.x, -height), vec2<f32>(gx.x, -gh.x), vec2<f32>(gy.x, -gh.y), dist, scatter);
        s.color += m.color * wt.z;
        s.lean += vec3<f32>(m.lean.x, -m.lean.y, 0.0) * wt.z;
        s.rough += m.rough * wt.z;
        s.ao += m.ao * wt.z;
    }
    return s;
}

// Patches for the ground seen from the air (50 m to a few km): the photograph of dry grass once
// more, 64 times its size (328 m a repeat). Enlarged that much, the shading of its tufts becomes
// patches of a few metres, which is what grassland, a field or a slope shows from there. It is
// read blurred, from the mip level of 64 texels a repeat (5 m a texel) or a coarser one: a sharper
// level shows single blades of grass tens of metres long. Blurred, the patches of one photograph
// look like those of another, so every material takes these: one fetch, and no seam where two
// materials meet. `strength` is how much the ground takes them (`air_strength`). `warp` (0..1,
// slow noises of 256 m and 1024 m) bends the mapping so that the repeats do not line up into a
// grid. Colour only, without anisotropy; 1 = no change. `w` is the wrapped position: 16384 m is
// 50 repeats.
fn mat_air(strength: f32, w: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>, dist: f32, warp: vec2<f32>) -> vec3<f32> {
    let layer = M_GRASS_DRY;
    var tiles = MAT_TILE;
    let tile = tiles[layer] * 64.0;
    let k = strength * smoothstep(tile * 0.06, tile * 0.6, dist) * (1.0 - smoothstep(TEX_RANGE * 1.5, TEX_RANGE * 2.25, dist));
    if k <= 0.0 {
        return vec3<f32>(1.0);
    }
    let size = f32(textureDimensions(mat_colour).x);
    let footprint = max(length(gx), length(gy)) * size / tile;
    let level = max(log2(max(footprint, 1e-6)), log2(size / 64.0));
    let uv = w / tile + 0.31 + (warp - 0.5) * 0.3;
    let ratio = textureSampleLevel(mat_colour, mat_sampler, uv, layer, level).rgb / max(frame.materials[layer].rgb, vec3<f32>(0.004));
    let grey = mix(vec3<f32>((ratio.r + ratio.g + ratio.b) / 3.0), ratio, 0.5);
    // Blurred, the photograph varies little: its contrast is raised, less close up, where a texel
    // of 5 m is several pixels wide and the patches look smeared, and less in the distance, where
    // they are seen edge on and draw lines across the screen.
    // Without the scattered fetch the big scale is at half contrast and gives no grain under the
    // patches: they are then raised a fifth less, or they read as brown smears.
    let gain = (1.4 + 0.8 * smoothstep(60.0, 300.0, dist) - 0.9 * smoothstep(800.0, 3000.0, dist)) * select(0.8, 1.0, TEX_SCATTER);
    return clamp(mix(vec3<f32>(1.0), grey, k * gain), vec3<f32>(0.55), vec3<f32>(1.6));
}

// How much a ground material takes the patches seen from the air: rock is mapped from three
// sides and has its own strata, paving is even.
fn air_strength(layer: i32) -> f32 {
    return select(1.0, 0.0, layer == M_ROCK_GRANITE || layer == M_ROCK_LAYERED || layer >= M_ASPHALT);
}

// The two materials that count most at a point of the ground, and how much each.
struct Pick {
    a: i32,
    b: i32,
    wa: f32,
    wb: f32,
};

// Lays `layer` over what was chosen so far, covering `w` of it.
fn pick_over(p: Pick, layer: i32, w: f32) -> Pick {
    var q = p;
    if w <= 0.0 {
        return q;
    }
    q.wa *= 1.0 - w;
    q.wb *= 1.0 - w;
    if layer == q.a {
        q.wa += w;
    } else if layer == q.b {
        q.wb += w;
    } else if w > q.wb {
        q.b = layer;
        q.wb = w;
    }
    if q.wb > q.wa {
        q = Pick(q.b, q.a, q.wb, q.wa);
    }
    return q;
}

// What `airport_paint` found at the fragment, for the photographs under the paint and the mown
// grass around: coverage of runway, apron, markings and tyre rubber, how much the point is on the
// airfield, its position across and along the runway in metres, and those two axes on the ground.
struct Paved {
    rw: f32,
    apron: f32,
    mark: f32,
    rubber: f32,
    field: f32,
    uv: vec2<f32>,
    right: vec2<f32>,
    dir: vec2<f32>,
};
var<private> paved: Paved;

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// A highlight of the sun as wide as the surface is rough, stronger at a grazing angle.
fn mat_sheen(n: vec3<f32>, v: vec3<f32>, sun: vec3<f32>, rough: f32) -> f32 {
    let h = normalize(sun + v);
    let a = max(rough * rough, 0.05);
    let power = 2.0 / (a * a) - 2.0;
    let fresnel = 0.04 + 0.96 * pow(1.0 - max(dot(v, h), 0.0), 5.0);
    return pow(max(dot(n, h), 0.0), power) * (power + 2.0) / 8.0 * fresnel;
}

// Buildings have no texture coordinates: every face is mapped flat from model space (metres), u
// along the horizontal of the face and v down it, so walls, gables and roofs are never stretched,
// corrugation and roof tiles run down the slope, and one fetch is enough.
// The horizontal of a face in model space.
fn building_tangent(n: vec3<f32>) -> vec3<f32> {
    if abs(n.y) > 0.999 {
        return vec3<f32>(1.0, 0.0, 0.0);
    }
    return normalize(vec3<f32>(n.z, 0.0, -n.x));
}

fn building_uv(p: vec3<f32>, n: vec3<f32>, t: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(dot(p, t), -dot(p, cross(n, t)));
}

// The photograph on a building. `tex` is BuildingOut.tex, `tangent` the horizontal of the face in the
// world, `n` the unit normal of the face, `gx` and `gy` the change of tex.xy between pixels.
fn building_material(tex: vec4<f32>, tangent: vec3<f32>, n: vec3<f32>, gx: vec2<f32>, gy: vec2<f32>, dist: f32) -> Surface {
    var tiles = MAT_TILE;
    var layer = i32(tex.z + 0.5);
    // A hangar stands on a concrete footing.
    if tex.w > 1.5 && layer == M_METAL_CORRUGATED && abs(n.y) < 0.2 && -tex.y < 0.9 {
        layer = M_CONCRETE;
    }
    var s = Surface(vec3<f32>(1.0), vec3<f32>(0.0), frame.materials[layer].w, 1.0);
    let gone = smoothstep(350.0, 900.0, dist);
    if !TEXTURES || gone >= 1.0 {
        return s;
    }
    let tile = tiles[layer];
    let relief = (1.0 - smoothstep(150.0, 300.0, dist)) * select(0.0, 1.0, TEX_RELIEF > 0.0);
    let m = mat_fetch(tex.xy / tile, layer, gx / tile, gy / tile, relief > 0.0);
    s.color = clamp(mix(m.color, vec3<f32>(1.0), gone), vec3<f32>(0.4), vec3<f32>(2.2));
    let t = normalize(tangent);
    var b = cross(n, t);
    // Up the face, whichever way round the model was placed.
    if b.y < 0.0 {
        b = -b;
    }
    s.lean = (t * m.lean.x + b * m.lean.y) * relief;
    s.rough = m.rough;
    s.ao = mix(1.0, m.ao, relief);
    return s;
}

// ---------------------------------------------------------------- terrain

struct TerrainIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) cover: vec3<f32>,   // farmland, water, forest
};

struct TerrainOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) cover: vec3<f32>,
    @location(4) wrapped: vec2<f32>,
    @location(5) tex: vec2<f32>,     // [materials] position (x, z) in metres modulo MAT_PERIOD, for the textures
};

@vertex
fn terrain_vertex(in: TerrainIn) -> TerrainOut {
    var o: TerrainOut;
    var rel = in.position + chunk.offset.xyz;
    rel.y += canopy_height(in.cover.z, length(rel.xz));
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.normal = in.normal;
    o.color = in.color;
    o.cover = in.cover;
    o.wrapped = in.position.xz + chunk.wrap.xy;
    o.tex = in.position.xz + wrap_mod(chunk.wrap.xy, MAT_PERIOD);   // [materials]
    return o;
}

fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, w: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = saturate(dot(pa, ba) / dot(ba, ba));
    return 1.0 - smoothstep(w - 0.15, w + 0.15, length(pa - ba * h));
}

// Which of the seven segments each digit lights.
const DIGIT_MASKS = array<i32, 10>(0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F);

// A painted digit in a box 4 m wide and 16 m tall, centred on x.
fn digit(p: vec2<f32>, n: i32) -> f32 {
    var masks = DIGIT_MASKS;
    let m = masks[clamp(n, 0, 9)];
    let w = 0.55;
    let x = 1.6;
    let y0 = 0.6;
    let y1 = 8.0;
    let y2 = 15.4;
    var v = 0.0;
    if (m & 1) != 0 { v = max(v, segment(p, vec2<f32>(-x, y2), vec2<f32>(x, y2), w)); }
    if (m & 2) != 0 { v = max(v, segment(p, vec2<f32>(x, y1), vec2<f32>(x, y2), w)); }
    if (m & 4) != 0 { v = max(v, segment(p, vec2<f32>(x, y0), vec2<f32>(x, y1), w)); }
    if (m & 8) != 0 { v = max(v, segment(p, vec2<f32>(-x, y0), vec2<f32>(x, y0), w)); }
    if (m & 16) != 0 { v = max(v, segment(p, vec2<f32>(-x, y0), vec2<f32>(-x, y1), w)); }
    if (m & 32) != 0 { v = max(v, segment(p, vec2<f32>(-x, y1), vec2<f32>(-x, y2), w)); }
    if (m & 64) != 0 { v = max(v, segment(p, vec2<f32>(-x, y1), vec2<f32>(x, y1), w)); }
    return v;
}

fn box(p: vec2<f32>, centre: vec2<f32>, ext: vec2<f32>, soft: f32) -> f32 {
    let d = abs(p - centre) - ext;
    return (1.0 - smoothstep(-soft, soft, d.x)) * (1.0 - smoothstep(-soft, soft, d.y));
}

// Runway, apron and markings. Returns paint coverage in a and colour in rgb.
fn airport_paint(xz: vec2<f32>, wrapped: vec2<f32>, dist: f32) -> vec4<f32> {
    var result = vec4<f32>(0.0);
    let count = i32(frame.misc.x);
    for (var i = 0; i < count; i++) {
        let d = xz - frame.airports_a[i].xy;
        let dir = frame.airports_a[i].zw;
        let right = vec2<f32>(-dir.y, dir.x);
        let along = dot(d, dir);
        let across = dot(d, right);
        let hl = frame.airports_b[i].x;
        let hw = frame.airports_b[i].y;
        if abs(along) > hl + 80.0 || abs(across) > hw + 140.0 {
            continue;
        }
        let soft = clamp(dist * 0.0012, 0.08, 6.0);
        let grain = vnoise(wrapped * 2.0, WRAP_I * 2) * 0.5 + vnoise(wrapped * 0.125, WRAP_I / 8) * 0.5;
        let asphalt = vec3<f32>(0.045, 0.046, 0.05) * (0.75 + 0.5 * grain);
        let white = vec3<f32>(0.72);
        let p = vec2<f32>(across, along);
        // [materials] the mown grass of the airfield: the strip `airfield_ground` colours.
        paved.field = max(paved.field, box(p, vec2<f32>(0.0), vec2<f32>(hw + 42.0, hl + 67.0), 6.0));
        paved.uv = p;
        paved.right = right;
        paved.dir = dir;

        // The apron and its taxiway on their side of the runway (none on the other runways of a
        // real aerodrome); a grass runway is the mown strip, unpainted.
        let side = frame.airports_c[i].x;
        let apron = max(box(p, vec2<f32>(side * (hw + 70.0), 0.0), vec2<f32>(32.0, 60.0), soft), box(p, vec2<f32>(side * (hw + 20.0), 0.0), vec2<f32>(22.0, 5.0), soft)) * abs(side);
        var col = vec3<f32>(0.085, 0.085, 0.09) * (0.8 + 0.4 * grain);
        var cover = apron;
        paved.apron = max(paved.apron, apron);   // [materials]

        let rw = box(p, vec2<f32>(0.0), vec2<f32>(hw, hl), soft) * frame.airports_c[i].y;
        if rw > 0.0 {
            let s = hl - abs(along);
            let u = select(-across, across, along < 0.0);
            var mark = 0.0;
            let fine = 1.0 - smoothstep(1500.0, 5000.0, dist);
            mark = max(mark, box(vec2<f32>(abs(across), 0.0), vec2<f32>(hw - 0.75, 0.0), vec2<f32>(0.3, 1.0), soft * 0.5));
            if s > 110.0 {
                mark = max(mark, (1.0 - smoothstep(0.2, 0.3 + soft * 0.3, abs(across))) * step(fract(along / 60.0), 0.5));
            }
            if s > 6.0 && s < 36.0 && abs(across) < hw - 1.6 {
                mark = max(mark, step(fract(across / 2.2 + 0.5), 0.55) * step(0.6, abs(across)));
            }
            if s > 300.0 && s < 345.0 {
                mark = max(mark, box(vec2<f32>(abs(across), 0.0), vec2<f32>(6.0, 0.0), vec2<f32>(2.2, 1.0), soft * 0.5));
            }
            if s > 46.0 && s < 62.5 {
                let n = i32(frame.airports_b[i].z) + select(18, 0, along < 0.0);
                let g = vec2<f32>(u, s - 46.0);
                mark = max(mark, digit(g - vec2<f32>(-3.2, 0.0), n / 10));
                mark = max(mark, digit(g - vec2<f32>(3.2, 0.0), n % 10));
            }
            mark *= mix(0.55, 1.0, fine);
            // Tyre marks where everybody touches down.
            let rubber = (1.0 - smoothstep(1.0, 4.0, abs(abs(across) - 1.3))) * smoothstep(120.0, 250.0, s) * (1.0 - smoothstep(300.0, 450.0, s));
            var rc = asphalt * (1.0 - 0.35 * rubber);
            rc = mix(rc, white * (0.8 + 0.3 * grain), mark);
            col = mix(col, rc, rw);
            cover = max(cover, rw);
            // [materials]
            paved.rw = rw;
            paved.mark = mark;
            paved.rubber = rubber;
        }
        if cover > result.a {
            result = vec4<f32>(col, cover);
        }
    }
    return result;
}

// [imagery] A real place's ground as the satellites saw it (Sentinel-2, `imagery.rs`): rgb its
// albedo, a how much of it is known (0 where nothing is: the sea, no clear view, no picture). The
// near picture over the far one, each read at the level of the ground a pixel spans and fading
// out over the last thirtieth of its side. The pictures hold black where nothing is known, so what
// is read is divided by how much is.
// The albedo above which a picture of the ground is eased (`lit_ground`).
const PHOTO_KNEE: f32 = 0.16;

fn ground_photo(xz: vec2<f32>, gx: vec2<f32>, gy: vec2<f32>) -> vec4<f32> {
    // The mean of the two sides of a pixel's footprint: seen at a grazing angle the longer one would
    // blur the picture along the ground (there is no anisotropic filtering here).
    let span = sqrt(length(gx) * length(gy));
    var sum = vec4<f32>(0.0);
    for (var i = 1; i >= 0; i--) {
        let p = frame.imagery[i];
        if p.w < 0.5 {
            continue;
        }
        let uv = (xz - p.xy) / p.z + 0.5;
        let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
        if edge <= 0.0 {
            continue;
        }
        let c = textureSampleLevel(imagery, height_sampler, uv, i, log2(max(span * f32(textureDimensions(imagery).x) / p.z, 1.0)));
        let k = smoothstep(0.0, 0.033, edge);
        sum = sum * (1.0 - c.a * k) + c * k;
    }
    return vec4<f32>(sum.rgb / max(sum.a, 1e-4), sum.a);
}

// ---------------------------------------------------------------- [parcels]
// A real place's ground cut into its parcels (`parcels.rs`): close up, where a texel of the picture
// spans pixels, each parcel is drawn in its own colour up to its edge, with what its kind has on it
// and a boundary along the edge. Everything of this job is marked [parcels].

const P_CROP: i32 = 1;
const P_STUBBLE: i32 = 2;
const P_PLOUGHED: i32 = 3;
const P_DRY: i32 = 4;
const P_MEADOW: i32 = 5;
const P_SCRUB: i32 = 6;
const P_WOOD: i32 = 7;
const P_ROCK: i32 = 8;
// The distance to the edge in the map, texels, is this times the square of its byte over 255
// (`distance_byte` in parcels.rs).
const P_DISTANCE_MOST: f32 = 255.0 / 32.0;

// The parcel at a point: how much it is drawn (0: the picture alone), its kind and number and those
// of the parcel across the nearest edge (the same when none is near), the metres to that edge and
// the way from it into the parcel (unit, x and z), the way its rows run (unit), and the colours of
// both (linear).
struct Parcel {
    weight: f32,
    kind: i32,
    other_kind: i32,
    id: i32,
    other: i32,
    edge: f32,
    inward: vec2<f32>,
    heading: vec2<f32>,
    colour: vec3<f32>,
    other_colour: vec3<f32>,
};

fn no_parcel() -> Parcel {
    return Parcel(0.0, 0, 0, 0, 0, 1e4, vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec3<f32>(0.0), vec3<f32>(0.0));
}

fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// One of four values by its number (0 to 3), without indexing an array: a shader's private array
// indexed at run time may be kept in slow memory.
fn pick4(a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, d: vec4<f32>, k: i32) -> vec4<f32> {
    return select(select(a, b, k == 1), select(c, d, k == 3), k >= 2);
}

// The parcel of grid `g` at `xz`, from the four texels round it: of the parcels among them, the
// one whose signed distance (plus in it, minus out of it) interpolates highest; the edge is where
// it equals the next one's. `Parcels::sample` in parcels.rs does the same.
fn parcel_of(g: i32, xz: vec2<f32>) -> Parcel {
    let p = frame.imagery[g];
    let side = i32(textureDimensions(parcels).x);
    let t = ((xz - p.xy) / p.z + 0.5) * f32(side) - 0.5;
    let base = floor(t);
    let f = t - base;
    let i0 = clamp(vec2<i32>(base), vec2<i32>(0), vec2<i32>(side - 2));
    let a0 = textureLoad(parcels, i0, 2 * g, 0);
    let a1 = textureLoad(parcels, i0 + vec2<i32>(1, 0), 2 * g, 0);
    let a2 = textureLoad(parcels, i0 + vec2<i32>(0, 1), 2 * g, 0);
    let a3 = textureLoad(parcels, i0 + vec2<i32>(1, 1), 2 * g, 0);
    let ids = vec4<f32>(a0.r, a1.r, a2.r, a3.r);
    let dg = vec4<f32>(a0.g, a1.g, a2.g, a3.g);
    let d = dg * dg * P_DISTANCE_MOST;
    let wt = vec4<f32>((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    // How each weight changes along x and along y.
    let wx = vec4<f32>(f.y - 1.0, 1.0 - f.y, -f.y, f.y);
    let wy = vec4<f32>(f.x - 1.0, -f.x, 1.0 - f.x, f.x);
    // Each parcel among the four, once: texel k unless an earlier texel is of the same one.
    let fresh = vec4<bool>(true, ids.y != ids.x, ids.z != ids.x && ids.z != ids.y, ids.w != ids.x && ids.w != ids.y && ids.w != ids.z);
    var best = -1e9;
    var second = -1e9;
    var bk = 0;
    var sk = 0;
    var grad = vec2<f32>(0.0, 1.0);
    for (var k = 0; k < 4; k++) {
        if !fresh[k] {
            continue;
        }
        let sd = d * select(vec4<f32>(-1.0), vec4<f32>(1.0), ids == vec4<f32>(ids[k]));
        let s = dot(wt, sd);
        if s > best {
            second = best;
            sk = bk;
            best = s;
            bk = k;
            grad = vec2<f32>(dot(wx, sd), dot(wy, sd));
        } else if s > second {
            second = s;
            sk = k;
        }
    }
    let lone = second < -1e8;
    if lone {
        sk = bk;
    }
    let own = pick4(a0, a1, a2, a3, bk);
    let other = pick4(a0, a1, a2, a3, sk);
    let step = p.z / f32(side);
    var o = no_parcel();
    o.kind = i32(round(own.a * 255.0));
    o.other_kind = i32(round(other.a * 255.0));
    o.id = i32(round(own.r * 255.0));
    o.other = i32(round(other.r * 255.0));
    o.edge = max(select((best - second) * 0.5, best, lone), 0.0) * step;
    // Texels run east (x) and south (z), as the render's axes.
    o.inward = grad / max(length(grad), 1e-6);
    let heading = own.b * (255.0 / 256.0) * PI;
    o.heading = vec2<f32>(cos(heading), sin(heading));
    o.colour = srgb_decode(textureLoad(parcels, i0 + vec2<i32>(bk & 1, bk >> 1), 2 * g + 1, 0).rgb);
    o.other_colour = srgb_decode(textureLoad(parcels, i0 + vec2<i32>(sk & 1, sk >> 1), 2 * g + 1, 0).rgb);
    return o;
}


// The parcel at `xz`, drawn as much as a texel of its grid spans pixels (`span` metres a pixel):
// not at all under two pixels, wholly from six. The near grid's where it is, the far one's beyond;
// where the near one gives way to the far one the parcels fade out and back in, so the change is
// not seen.
fn ground_parcel(xz: vec2<f32>, span: f32) -> Parcel {
    var near_k = 0.0;
    let near = frame.imagery[0];
    if near.w > 1.5 {
        let uv = (xz - near.xy) / near.z + 0.5;
        near_k = smoothstep(0.0, 0.033, min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y)));
    }
    let g = select(1, 0, near_k > 0.5);
    let p = frame.imagery[g];
    if p.w < 1.5 {
        return no_parcel();
    }
    let uv = (xz - p.xy) / p.z + 0.5;
    let inside = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    let step = p.z / f32(textureDimensions(parcels).x);
    var weight = smoothstep(2.0, 6.0, step / max(span, 1e-4)) * smoothstep(0.0, 0.01, inside);
    if near_k > 0.0 && near_k < 1.0 {
        weight *= abs(2.0 * near_k - 1.0);
    }
    if weight <= 0.0 {
        return no_parcel();
    }
    var o = parcel_of(g, xz);
    o.weight = select(0.0, weight, o.kind > 0);
    return o;
}

// The ground material a parcel's kind is drawn with.
fn parcel_material(kind: i32) -> i32 {
    switch kind {
        case P_CROP: { return M_CROP_GREEN; }
        case P_STUBBLE: { return M_CROP_STUBBLE; }
        case P_PLOUGHED: { return M_SOIL_PLOUGHED; }
        case P_MEADOW: { return M_GRASS_MEADOW; }
        case P_WOOD: { return M_FOREST_FLOOR; }
        case P_ROCK: { return M_SCREE; }
        default: { return M_GRASS_DRY; }
    }
}

// Stripes across `x` with this period, -1 to 1, faded out as they get finer than some three pixels.
fn stripes(x: f32, period: f32, span: f32) -> f32 {
    return sin(x * (2.0 * PI / period)) * (1.0 - smoothstep(0.2, 0.4, span / period));
}

// Round blobs scattered on a grid of `cell` metres (which divides WRAP), one in a cell or none
// (`density` of the cells have one): how much of a blob covers `w`, its edge eased over `aa` metres.
fn blobs(w: vec2<f32>, cell: f32, density: f32, size: f32, salt: i32, aa: f32) -> f32 {
    let c = floor(w / cell);
    let ci = vec2<i32>(c) + salt;
    if ihash(ci) > density {
        return 0.0;
    }
    let r = cell * size * (0.55 + 0.45 * ihash(ci + 17));
    let centre = (c + 0.5) * cell + (vec2<f32>(ihash(ci + 31), ihash(ci + 47)) - 0.5) * max(cell - 2.0 * r, 0.0);
    return 1.0 - smoothstep(r - aa, r + aa, length(w - centre));
}

// What a parcel's kind has on it, as a factor of its colour about 1 (its colour is already the
// mean of all of it): rows of a crop and its tramlines, the swaths of a harvest, furrows, the
// mown stripes of a meadow, tufts of dry grass, bushes of scrub, crowns of a wood, stones. Close
// to the edge a field's rows follow it (the headland). `sun` is the sun's way (x, z) over its height.
fn parcel_pattern(pc: Parcel, w: vec2<f32>, span: f32, sun: vec2<f32>) -> f32 {
    let r = ihash(vec2<i32>(pc.id, 911));
    let across = dot(w, vec2<f32>(-pc.heading.y, pc.heading.x)) + r * 97.0;
    let field = pc.kind == P_CROP || pc.kind == P_PLOUGHED || pc.kind == P_STUBBLE;
    let rows = select(across, pc.edge, field && pc.edge < 5.0 + 3.0 * r);
    let aa = max(span * 0.7, 0.03);
    var m = 1.0;
    switch pc.kind {
        case P_CROP: {
            let period = 18.0 + 6.0 * r;
            let q = abs(fract(across / period) - 0.5) * period;
            let ruts = (1.0 - smoothstep(0.12, 0.12 + aa, abs(q - 0.9))) * (1.0 - smoothstep(0.3, 0.6, span));
            m = (1.0 + 0.24 * stripes(rows, 0.75 + 0.5 * r, span) + 0.05 * stripes(across, 9.0 + 4.0 * r, span)) * (1.0 - 0.3 * ruts);
        }
        case P_STUBBLE: {
            m = 1.0 + 0.10 * stripes(rows, 5.0 + 3.0 * r, span) + 0.08 * stripes(rows, 0.9, span) + 0.04 * stripes(across, 23.0, span);
        }
        case P_PLOUGHED: {
            m = 1.0 + 0.28 * stripes(rows, 0.5, span) + 0.07 * stripes(rows, 3.6 + r, span);
        }
        case P_MEADOW: {
            let period = 6.0 + 2.0 * r;
            let mown = select(0.0, 0.07, r < 0.5) * clamp(sin(across * (2.0 * PI / period)) * 6.0, -1.0, 1.0) * (1.0 - smoothstep(0.05, 0.15, span / period));
            m = 1.0 + mown + 0.16 * (vnoise(w / 4.0, WRAP_I / 4) - 0.5) * (1.0 - smoothstep(0.8, 1.6, span));
        }
        case P_DRY: {
            let tufts = 1.0 - smoothstep(0.6, 1.2, span);
            m = 1.0 + (0.22 * (vnoise(w / 2.0, WRAP_I / 2) - 0.5) * tufts + 0.12 * (vnoise(w / 8.0 + 3.0, WRAP_I / 8) - 0.5));
        }
        case P_SCRUB, P_WOOD: {
            // Bushes or crowns, lit on the sun's side and casting a short shadow away from it.
            let wood = pc.kind == P_WOOD;
            let cell = select(4.0, 8.0, wood);
            let density = select(0.55, 0.9, wood);
            let size = select(0.32, 0.42, wood);
            let fade = 1.0 - smoothstep(cell * 0.12, cell * 0.3, span);
            let crown = blobs(w, cell, density, size, 401, aa);
            let shade = blobs(w + sun * select(1.2, 4.0, wood), cell, density, size, 401, aa) * (1.0 - crown);
            let mean = density * 3.1416 * size * size * 0.8;
            m = 1.0 + ((1.0 - crown * 0.5 - shade * 0.6) - (1.0 - mean * 0.5 - mean * 0.3)) * fade;
        }
        case P_ROCK: {
            let fade = 1.0 - smoothstep(0.4, 1.0, span);
            let stone = blobs(w, 2.0, 0.3, 0.3, 77, aa);
            let shade = blobs(w + sun * 0.4, 2.0, 0.3, 0.3, 77, aa) * (1.0 - stone);
            m = 1.0 + (0.15 * stone - 0.35 * shade + 0.01) * fade + 0.15 * (vnoise(w / 8.0, WRAP_I / 8) - 0.5);
        }
        default: {}
    }
    return m;
}

// Value noise along a line, 0 to 1, a value every unit of `x` (`salt` picks the line).
fn line_noise(x: f32, salt: i32) -> f32 {
    let i = floor(x);
    let f = x - i;
    let a = ihash(vec2<i32>(i32(i), salt));
    let b = ihash(vec2<i32>(i32(i) + 1, salt));
    return mix(a, b, f * f * (3.0 - 2.0 * f));
}

// What stands on the edge between two parcels: a dry-stone wall where the land is dry, a hedge
// where it is green, a track beside some, or nothing; decided by the pair, the same on both sides.
// `albedo` and `cover`: what is drawn over the ground; `tilt`: how far the normal leans into the
// parcel (the round of a wall's top); `shade`: how much of the sun the wall or hedge hides here.
struct Bound {
    albedo: vec3<f32>,
    cover: f32,
    tilt: f32,
    shade: f32,
    ao: f32,
};

fn parcel_bounds(pc: Parcel, w: vec2<f32>, span: f32) -> Bound {
    var b = Bound(vec3<f32>(0.0), 0.0, 0.0, 0.0, 1.0);
    if pc.other == pc.id || pc.edge > 8.0 || pc.other_kind == 0 {
        return b;
    }
    let lo = min(pc.id, pc.other);
    let hi = max(pc.id, pc.other);
    let pair = ihash(vec2<i32>(lo * 7 + 3, hi * 13 + 5));
    let pair2 = ihash(vec2<i32>(hi * 11 + 1, lo * 5 + 99));
    let c = (pc.colour + pc.other_colour) * 0.5;
    let dry = 1.0 - smoothstep(0.95, 1.3, c.g / max(c.r, 1e-3));
    // Woods, scrub and bare rock have no walls inside them; along an open field, some.
    let rough = i32(pc.kind >= P_SCRUB) + i32(pc.other_kind >= P_SCRUB);
    if rough == 2 || pc.kind == P_ROCK || pc.other_kind == P_ROCK {
        return b;
    }
    let wood = rough == 1;
    // Two parcels much alike are often one field the picture could not tell apart.
    let alike = smoothstep(0.004, 0.02, length(pc.colour - pc.other_colour));
    let wall_p = dry * 0.9 * select(1.0, 0.35, wood) * mix(0.5, 1.0, alike);
    let hedge_p = (1.0 - dry) * 0.4 * select(1.0, 0.0, wood) * mix(0.3, 1.0, alike);
    let sun = frame.sun_dir.xyz;
    // Positive when the sun is on this side of the edge.
    let toward = dot(pc.inward, sun.xz);
    let aa = max(span * 0.7, 0.02);
    let e = pc.edge;
    // Metres along the edge, the same way from both sides of it, and the pair's number for what
    // varies along it.
    var tangent = vec2<f32>(-pc.inward.y, pc.inward.x);
    tangent = select(tangent, -tangent, tangent.x < 0.0 || (tangent.x == 0.0 && tangent.y < 0.0));
    let along = dot(w, tangent);
    let salt = lo * 263 + hi;
    // Far off, where a wall or a hedge is a fraction of a pixel, less of it: a pale thread at most.
    let far = 1.0 - 0.6 * smoothstep(0.15, 0.8, span);
    if pair2 < 0.09 && pc.id < pc.other {
        // A dirt track along it on one side, the grass of its crown between the wheels.
        let x = abs(e - 2.6);
        let track = 1.0 - smoothstep(1.4 - aa, 1.4 + aa, x);
        let crown = (1.0 - smoothstep(0.35 - aa, 0.35 + aa, x)) * (1.0 - smoothstep(0.3, 0.6, span));
        b.albedo = mix(vec3<f32>(0.30, 0.26, 0.20) * (0.9 + 0.2 * vnoise(w / 2.0, WRAP_I / 2)), pc.colour * 0.85, crown);
        b.cover = track;
        b.ao = 1.0;
    }
    if pair < wall_p {
        // A dry-stone wall of pale limestone, some 0.7 m wide and 1 m high, as the walls of the
        // Mediterranean's dry fields: its top a row of stones, lighter and darker, broken here and
        // there by a gap or a stretch fallen to rubble; its foot a few dry weeds, a little darker
        // than the field; its shadow a thin strip on the side away from the sun.
        let intact = line_noise(along / 7.0, salt);
        let present = smoothstep(0.1, 0.15, intact);
        let standing = smoothstep(0.17, 0.32, intact);
        let hw = (0.3 + 0.08 * line_noise(along / 2.5, salt + 7)) * mix(1.3, 1.0, standing);
        let height = present * mix(0.3, 1.0, standing);
        let wall = (1.0 - smoothstep(hw - aa, hw + aa, e)) * present * mix(0.5, 1.0, standing);
        let fine = 1.0 - smoothstep(0.05, 0.2, span);
        let block = vec2<i32>(i32(floor(along / 0.42)), i32(floor(e / 0.22)) + salt);
        let stones = mix(1.0, 0.8 + 0.35 * ihash(block), fine) * (0.9 + 0.2 * vnoise(w / 3.0, WRAP_I / 3));
        let stone = vec3<f32>(0.3, 0.29, 0.26) * stones;
        let weeds = (1.0 - smoothstep(hw, hw + 0.25 + 0.6 * line_noise(along / 1.7, salt + 3), e)) * (1.0 - wall) * 0.6 * present;
        b.albedo = mix(mix(b.albedo, pc.colour * 0.7, weeds * select(1.0, 0.0, b.cover > 0.5)), stone, wall);
        b.cover = max(max(b.cover, weeds), wall) * far;
        b.tilt = smoothstep(0.0, hw, e) * wall * 0.9;
        let reach = hw + height * max(-toward, 0.0) / max(sun.y, 0.06);
        b.shade = (1.0 - smoothstep(reach - aa, reach + aa, e)) * (1.0 - wall) * step(toward, 0.0) * 0.85 * far;
        b.ao = 1.0 - 0.15 * (1.0 - smoothstep(hw, hw + 0.5, e)) * height * far;
    } else if pair < wall_p + hedge_p {
        let hw = 1.0 + 0.5 * vnoise(w / 4.0, WRAP_I / 4);
        let height = 2.4;
        let present = smoothstep(0.08, 0.16, line_noise(along / 9.0, salt));
        let hedge = (1.0 - smoothstep(hw - aa, hw + aa, e)) * present;
        let leaves = 0.7 + 0.6 * vnoise(w, WRAP_I) * (1.0 - smoothstep(0.3, 0.8, span));
        b.albedo = mix(b.albedo, vec3<f32>(0.04, 0.07, 0.03) * leaves, hedge);
        b.cover = max(b.cover, hedge) * far;
        b.tilt = smoothstep(0.0, hw, e) * hedge * 0.8;
        let reach = hw + height * max(-toward, 0.0) / max(sun.y, 0.06);
        b.shade = (1.0 - smoothstep(reach - aa, reach + aa, e)) * (1.0 - hedge) * step(toward, 0.0) * far * present;
        b.ao = 1.0 - 0.3 * (1.0 - smoothstep(hw, hw + 1.2, e)) * far * present;
    }
    return b;
}

// The ground lit, before the water over it and the haze: rgb, and in a how much of it is painted
// runway or apron (no river there). `tex_g*` say how the position changes from one pixel to the next.
// The bed of the sea (`bed`) has no roads, and the water hides the relief's shadows on it.
fn lit_ground(in: TerrainOut, tex_gx: vec2<f32>, tex_gy: vec2<f32>, tex_gh: vec2<f32>, bed: bool) -> vec4<f32> {
    let dist = length(in.rel);
    var n = normalize(in.normal);
    // [imagery] Where a picture of the place knows the ground, it is its colour: from far, all of
    // it; near, the photographs give it its grain, and what the maps say it is (town, field,
    // forest, rock, sand) chooses them. The painted fields, towns and canopy give way to it.
    let photo = select(vec4<f32>(0.0), ground_photo(in.rel.xz, tex_gx, tex_gy), frame.imagery[0].w + frame.imagery[1].w > 0.5 && !bed);
    let seen = photo.a;
    // Bright ground in a picture from above is mostly roofs, concrete and bare rock in the full
    // sun; under the buildings drawn over it, and lit again here, it would glare: its brightness
    // is eased above `PHOTO_KNEE`.
    let bright = luma(photo.rgb);
    let eased = select(photo.rgb, photo.rgb * (PHOTO_KNEE + (bright - PHOTO_KNEE) * 0.45) / max(bright, 1e-4), bright > PHOTO_KNEE);
    var albedo = mix(in.color, eased, seen);
    // [parcels] Close up, the parcels the picture was cut into: each in its own colour up to its
    // edge (eased as the picture is), with a little of the picture's own light and dark inside it
    // (none by the edge, where the picture runs across it), and what its kind has on it.
    let span = sqrt(length(tex_gx) * length(tex_gy));
    var pc = no_parcel();
    if seen > 0.0 && max(frame.imagery[0].w, frame.imagery[1].w) > 1.5 {
        pc = ground_parcel(in.rel.xz, span);
    }
    let pw = pc.weight * seen;
    if pw > 0.0 {
        let pl = luma(pc.colour);
        let pe = select(pc.colour, pc.colour * (PHOTO_KNEE + (pl - PHOTO_KNEE) * 0.45) / max(pl, 1e-4), pl > PHOTO_KNEE);
        let own = clamp(luma(eased) / max(luma(pe), 1e-4), 0.7, 1.4);
        let sun = frame.sun_dir.xz / max(frame.sun_dir.y, 0.15);
        var fill = pe * mix(1.0, own, 0.4 * smoothstep(4.0, 15.0, pc.edge));
        var pattern = parcel_pattern(pc, in.wrapped, span, sun);
        if pc.kind == P_WOOD {
            // Where its trees are drawn a wood's ground is its floor, not the crowns the picture saw.
            let crowns = smoothstep(frame.misc.z * 0.72, frame.misc.z * 0.98, dist);
            fill = mix(vec3<f32>(luma(pe)) * vec3<f32>(1.2, 1.0, 0.7) * 0.8, fill, crowns);
            pattern = mix(1.0, pattern, crowns);
        }
        albedo = mix(albedo, fill * pattern, pw);
    }
    let w = in.wrapped;
    let close = 1.0 - smoothstep(600.0, 2500.0, dist);
    let mid = 1.0 - smoothstep(6000.0, 20000.0, dist);
    // [materials] The material of the field the fragment is in.
    var farm_layer = M_GRASS_MEADOW;
    let height = in.rel.y + frame.cam_wrap.y;

    // Farmland: a patchwork of fields with crop rows.
    let farm = smoothstep(0.3, 0.6, in.cover.x);
    if farm > 0.01 {
        let cell = floor(w / 256.0);
        let ci = vec2<i32>(cell) & vec2<i32>(63);
        let r = ihash(ci);
        let uv = w / 256.0 - cell;
        let along_x = r > 0.5;
        let strips = 2.0 + floor(ihash(ci + 91) * 3.0);
        let t = select(uv.y, uv.x, along_x) * strips;
        let strip = floor(t);
        let pick = ihash(ci * 7 + vec2<i32>(i32(strip), 13));
        var crop = vec3<f32>(0.24, 0.25, 0.11);
        if pick < 0.22 {
            crop = vec3<f32>(0.36, 0.30, 0.12);
        } else if pick < 0.45 {
            crop = vec3<f32>(0.10, 0.19, 0.05);
        } else if pick < 0.65 {
            crop = vec3<f32>(0.06, 0.12, 0.04);
        } else if pick < 0.82 {
            crop = vec3<f32>(0.17, 0.12, 0.075);
        }
        // [materials] stubble, two green crops, ploughed soil, pasture.
        farm_layer = select(select(select(M_GRASS_MEADOW, M_SOIL_PLOUGHED, pick < 0.82), M_CROP_GREEN, pick < 0.65), M_CROP_STUBBLE, pick < 0.22);
        let rows = 0.5 + 0.5 * sin(select(w.x, w.y, along_x) * 2.1);
        crop *= 1.0 + 0.16 * (rows - 0.5) * close;
        let edge = min(min(fract(t), 1.0 - fract(t)) * 256.0 / strips, min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y)) * 256.0);
        let hedge = 1.0 - smoothstep(1.5, 3.5 + dist * 0.002, edge);
        crop = mix(crop, vec3<f32>(0.035, 0.07, 0.03), hedge * 0.8);
        albedo = mix(albedo, mix(vec3<f32>(0.16, 0.18, 0.08), crop, mid), farm * (1.0 - seen));
        // [imagery] A real field: its rows on the photograph's colour, and the photographs of
        // what its colour says it is: green, a crop; pale, stubble; brown, ploughed soil.
        if seen > 0.0 {
            albedo *= 1.0 + 0.16 * (rows - 0.5) * close * farm * seen * (1.0 - pc.weight);   // [parcels] their own rows instead
            let p = photo.rgb;
            let green = p.g / max(p.r, 1e-3) > 1.08;
            farm_layer = select(select(M_SOIL_PLOUGHED, M_CROP_STUBBLE, luma(p) > 0.16), M_CROP_GREEN, green);
        }
    }

    // [imagery] Under the trees and beyond them the photograph is the canopy itself; a town's painted
    // streets stay only close by, where its 10 m are blurred, and fade into it by 1.5 km.
    albedo = mix(forest_ground(albedo, in.cover.z, w, length(in.rel.xz)), albedo, seen);
    albedo = mix(town_ground(albedo, in.rel.xz, dist), albedo, seen * smoothstep(300.0, 1500.0, dist));
    albedo = airfield_ground(albedo, in.rel.xz, dist);

    // Ground texture at several scales so it does not turn flat close up.
    // [materials] Where the photographs are, they are the two finest scales: the noise there is
    // left at its average, and comes back as they fade out (`untextured` goes from 0 to 1).
    let untextured = select(1.0, smoothstep(TEX_RANGE * 0.375, TEX_RANGE, dist), TEXTURES);
    var detail = 0.5 * close + vnoise(w / 16.0, WRAP_I / 16) * mid * 0.8 + vnoise(w / 128.0, WRAP_I / 128) * 0.6;
    if untextured > 0.0 {
        detail += (vnoise(w * 0.5, WRAP_I / 2) - 0.5) * close * untextured;
    }
    let slow = vec2<f32>(vnoise(w / 256.0 + 5.0, WRAP_I / 256), vnoise(w / 1024.0, WRAP_I / 1024));
    let broad = slow.y * 0.6 + slow.x * 0.4;
    // [imagery] A photograph has its own light and dark patches: only the finest noise is kept
    // on it, round 1 (its mean there).
    let shade = (0.74 + 0.22 * detail) * (0.75 + 0.5 * broad);
    albedo *= mix(shade, 1.0 + 0.22 * (detail - 0.5 * close - 0.4 * mid - 0.3) * close, seen);
    albedo = mix(albedo, albedo * vec3<f32>(1.25, 1.1, 0.8), saturate(broad * 1.6 - 0.6) * (1.0 - farm) * 0.6 * (1.0 - seen));
    if untextured > 0.0 {   // [materials] the normal maps do this where there are photographs
        let fine_bump = (vnoise(w * 0.5 + 37.0, WRAP_I / 2) - 0.5) * close;
        n = normalize(n + vec3<f32>(fine_bump * 0.25, 0.0, (detail - 1.2) * 0.1 * close) * untextured);
    }

    // Steep rock shows its strata.
    let steep = smoothstep(0.22, 0.5, 1.0 - in.normal.y / length(in.normal));
    albedo *= mix(1.0, 0.72 + 0.56 * vnoise(vec2<f32>(height * 0.09, (w.x + w.y) / 128.0), 128), steep * mid);

    // Roads between towns.
    let road_count = select(i32(frame.extra.y), 0, bed);
    var road = 0.0;
    var centre = 0.0;
    // [materials] position across and along the road the fragment is on, and its direction.
    var road_uv = vec2<f32>(0.0);
    var road_dir = vec2<f32>(0.0, 1.0);
    for (var i = 0; i < road_count; i++) {
        let a = frame.roads[i].xy;
        let b = frame.roads[i].zw;
        let pa = in.rel.xz - a;
        let ba = b - a;
        let h = saturate(dot(pa, ba) / dot(ba, ba));
        let d = length(pa - ba * h);
        let width = max(3.5, dist * 0.0011);
        let road_before = road;   // [materials]
        road = max(road, 1.0 - smoothstep(width, width + max(0.4, dist * 0.0006), d));
        centre = max(centre, (1.0 - smoothstep(0.12, 0.3, d)) * step(fract(h * length(ba) / 12.0), 0.5) * close);
        if road > road_before {   // [materials]
            road_dir = ba / length(ba);
            road_uv = vec2<f32>(pa.x * road_dir.y - pa.y * road_dir.x, dot(pa, road_dir));
        }
    }
    // A region's roads, from the road map: its mip level for the ground a pixel spans, so a road far
    // off fades into the ground; a little more than its share of the pixel, as the eye picks out a
    // line. They fade out before the map's edge.
    if frame.road_map.w > 0.5 && !bed {
        let uv = (in.rel.xz - frame.road_map.xy) / (2.0 * frame.road_map.z) + 0.5;
        let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
        if edge > 0.0 {
            let texels = max(length(tex_gx), length(tex_gy)) / ROAD_TEXEL;
            let lod = log2(max(texels, 1.0));
            let raw = textureSampleLevel(road_map, height_sampler, uv, lod).r;
            // Close up, where a texel spans more than a pixel, the edge of the road (where the map is a
            // half: its texels fall from 1 to 0 over one) is drawn a pixel wide, not a texel.
            let sharp = select(raw, saturate((raw - 0.5) / max(texels, 0.02) + 0.5), texels < 1.0);
            let mapped = saturate(sharp * (1.0 + 0.6 * lod)) * smoothstep(0.0, 0.05, edge);
            if mapped > road {   // [materials] mapped as the ground is: these roads have no direction of their own
                road = mapped;
                road_uv = w;
                road_dir = vec2<f32>(0.0, 1.0);
            }
        }
    }
    road *= 1.0 - smoothstep(0.35, 0.65, in.cover.y);
    let paint = airport_paint(in.rel.xz, w, dist);

    // [materials] BEGIN: the photographs. `ground` multiplies the colour of the land, `road_tex`
    // that of the road and `paved_tex` that of runway and apron; the leans tilt the normal.
    var ground = Surface(vec3<f32>(1.0), vec3<f32>(0.0), 1.0, 1.0);
    var road_tex = Mat(vec3<f32>(1.0), vec2<f32>(0.0), 1.0, 1.0);
    var paved_tex = Mat(vec3<f32>(1.0), vec2<f32>(0.0), 1.0, 1.0);
    // How much of a sheen the fragment has (asphalt, concrete, wet sand).
    var sheen = 0.0;
    if TEXTURES && dist < TEX_RANGE * 2.25 {
        // Bands some 8 m wide, a fraction of a repeat of the big scale; two noises of different
        // sizes, so that the bands do not follow the grid of either.
        let scatter = (vnoise(w / 64.0, WRAP_I / 64) * 0.6 + vnoise(w / 32.0 + 7.3, WRAP_I / 32) * 0.4) * 8.0;
        let slope = 1.0 - in.normal.y / length(in.normal);
        // What the colour of the vertex says the ground is (see `Terrain::surface`).
        let c = in.color;
        let snow = smoothstep(0.45, 0.75, c.b);
        let sand = smoothstep(0.27, 0.42, c.r) * (1.0 - snow) * (1.0 - smoothstep(6.0, 12.0, height));
        let grey = smoothstep(0.55, 0.75, c.b / max(c.r, 1e-3)) * (1.0 - smoothstep(1.0, 1.3, c.g / max(c.r, 1e-3))) * (1.0 - snow) * (1.0 - sand);
        // Grey on the flat is a town; on a slope or high up it is rock, loose where it is not steep.
        let town = grey * (1.0 - smoothstep(0.04, 0.09, slope)) * (1.0 - smoothstep(1700.0, 1900.0, height));
        let layered = max(smoothstep(0.40, 0.50, slope), smoothstep(1850.0, 1950.0, height));
        // [imagery] How green the photograph is, where there is one.
        let hue = mix(c, photo.rgb, seen);
        let moist = saturate((hue.g / max(hue.r, 1e-3) - 1.0) / 0.76);
        var pick = Pick(M_GRASS_DRY, M_GRASS_MEADOW, 1.0 - moist, moist);
        if pick.wb > pick.wa {
            pick = Pick(pick.b, pick.a, pick.wb, pick.wa);
        }
        pick = pick_over(pick, M_FOREST_FLOOR, in.cover.z);
        pick = pick_over(pick, farm_layer, farm);
        pick = pick_over(pick, parcel_material(pc.kind), pw);   // [parcels]
        // In a town, what `town_ground` painted there.
        // Grey ground beyond the painted towns stays bare earth.
        pick = pick_over(pick, M_DIRT_ROAD, town * (1.0 - town_at.weight));
        pick = pick_over(pick, town_at.layer, town_at.weight);
        pick = pick_over(pick, M_GRASS_SHORT, paved.field);
        pick = pick_over(pick, M_SCREE, (grey - town) * (1.0 - steep));
        pick = pick_over(pick, M_ROCK_GRANITE, steep * (1.0 - layered));
        pick = pick_over(pick, M_ROCK_LAYERED, steep * layered);
        pick = pick_over(pick, M_SNOW, snow);
        pick = pick_over(pick, M_SAND, sand);

        let covered = max(max(paint.a, road), smoothstep(0.35, 0.65, in.cover.y));
        if covered < 0.99 {
            // [materials] the two materials, and the patches seen from the air.
            ground = mat_ground(pick.a, in.tex, height, n, tex_gx, tex_gy, tex_gh, dist, scatter);
            // The second material comes in from a twentieth; at a half the two swap places unseen.
            let second = smoothstep(0.05, 0.5, pick.wb / (pick.wa + pick.wb)) * 0.5;
            // Patches from the air, as much as the two materials take them.
            let air = mat_air(mix(air_strength(pick.a), air_strength(pick.b), second), w, tex_gx, tex_gy, dist, slow);
            if second > 0.0 {
                let other = mat_ground(pick.b, in.tex, height, n, tex_gx, tex_gy, tex_gh, dist, scatter);
                ground = Surface(mix(ground.color, other.color, second), mix(ground.lean, other.lean, second), mix(ground.rough, other.rough, second), mix(ground.ao, other.ao, second));
            }
            // [imagery] A photograph has its real patches: the made-up ones fade under it.
            ground.color *= mix(air, vec3<f32>(1.0), seen * (0.7 + 0.25 * pc.weight));
            // Beyond the single trees the ground is painted as the canopy (`forest_ground`): no photograph there.
            let canopy = in.cover.z * smoothstep(frame.misc.z * 0.72, frame.misc.z * 0.98, dist);
            ground.color = mix(ground.color, vec3<f32>(1.0), canopy);
            ground.lean *= 1.0 - canopy;
            // Sand is wet, and shines, next to the water.
            sheen = sand * (1.0 - smoothstep(0.4, 1.6, height)) * 0.8;
            ground.rough = mix(ground.rough, 0.3, sheen);
        }
        if road > 0.01 {
            // Mapped along the road, so the grain does not cut its edges at an angle.
            let across = vec2<f32>(road_dir.y, -road_dir.x);
            road_tex = mat_planar(M_ASPHALT, road_uv, vec2<f32>(dot(tex_gx, across), dot(tex_gx, road_dir)), vec2<f32>(dot(tex_gy, across), dot(tex_gy, road_dir)), dist, scatter);
            ground.lean = mix(ground.lean, vec3<f32>(across.x * road_tex.lean.x + road_dir.x * road_tex.lean.y, 0.0, across.y * road_tex.lean.x + road_dir.y * road_tex.lean.y) * 0.6, road);
            ground.rough = mix(ground.rough, road_tex.rough, road);
            ground.ao = mix(ground.ao, road_tex.ao, road);
            sheen = mix(sheen, 1.0, road);
        }
        if paint.a > 0.01 {
            // Mapped across and along the runway, for the same reason.
            let gx = vec2<f32>(dot(tex_gx, paved.right), dot(tex_gx, paved.dir));
            let gy = vec2<f32>(dot(tex_gy, paved.right), dot(tex_gy, paved.dir));
            if paved.rw > 0.01 {
                paved_tex = mat_planar(M_ASPHALT, paved.uv, gx, gy, dist, scatter);
            }
            if paved.rw < 0.99 {
                let concrete = mat_planar(M_CONCRETE, paved.uv, gx, gy, dist, scatter);
                paved_tex = Mat(mix(concrete.color, paved_tex.color, paved.rw), mix(concrete.lean, paved_tex.lean, paved.rw), mix(concrete.rough, paved_tex.rough, paved.rw), mix(concrete.ao, paved_tex.ao, paved.rw));
            }
            // Paint lies on the grain and shows less of it; rubber fills it.
            paved_tex.color = mix(paved_tex.color, vec3<f32>(0.6 + 0.4 * luma(paved_tex.color)), paved.mark);
            paved_tex.lean *= 1.0 - 0.6 * paved.mark;
            paved_tex.rough -= 0.2 * paved.rubber * paved.rw;
            ground.rough = mix(ground.rough, paved_tex.rough, paint.a);
            ground.ao = mix(ground.ao, paved_tex.ao, paint.a);
            sheen = mix(sheen, 1.0, paint.a);
        }
        albedo *= ground.color;
    }
    // [parcels] Walls, hedges and tracks along the parcels' edges; none, nor their shadows, on a
    // runway, an apron or the strip round them, a road or a river.
    var bound = Bound(vec3<f32>(0.0), 0.0, 0.0, 0.0, 1.0);
    let bw = pw * (1.0 - paint.a) * (1.0 - paved.field) * (1.0 - road) * (1.0 - smoothstep(0.35, 0.65, in.cover.y));
    if bw > 0.0 {
        bound = parcel_bounds(pc, in.wrapped, span);
        albedo = mix(albedo, bound.albedo, bound.cover * bw);
    }
    albedo = mix(albedo, mix(vec3<f32>(0.07, 0.07, 0.075), vec3<f32>(0.6) * (0.6 + 0.4 * luma(road_tex.color)), centre) * mix(road_tex.color, vec3<f32>(1.0), centre), road);
    albedo = mix(albedo, paint.rgb * paved_tex.color, paint.a);
    if paint.a > 0.5 {
        n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), paint.a));
    }
    let paved_lean = vec3<f32>(paved.right.x * paved_tex.lean.x + paved.dir.x * paved_tex.lean.y, 0.0, paved.right.y * paved_tex.lean.x + paved.dir.y * paved_tex.lean.y) * 0.6;
    n = normalize(n + mix(ground.lean, paved_lean, paint.a));
    if bw > 0.0 {   // [parcels]
        n = normalize(n + vec3<f32>(pc.inward.x, 0.0, pc.inward.y) * bound.tilt * bw);
    }
    // [materials] END

    // What a crash left: scorched ground, a hole and the earth thrown out of it, embers.
    var embers = 0.0;
    if frame.fire_count.y > 0.0 {
        let m = ground_marks(in.rel);
        albedo = mix(albedo, vec3<f32>(0.15, 0.11, 0.075) * ground.color, m.earth);
        albedo = mix(albedo, vec3<f32>(0.02, 0.018, 0.016), m.scorch * 0.94);
        n = normalize(n + m.bend);
        embers = m.embers;
    }

    let sun = frame.sun_dir.xyz;
    var shadow = body_shadow(in.rel) * cloud_shadow(in.rel);
    let contact = body_contact(in.rel);
    var ao = ground.ao * contact;
    shadow *= contact;
    if bw > 0.0 {   // [parcels]
        shadow *= 1.0 - bound.shade * bw;
        ao *= mix(1.0, bound.ao, bw);
    }
    if !bed {
        shadow *= terrain_shadow(in.rel);
    }
    let ndl = max(dot(n, sun), 0.0);
    let sunlight = light_at(in.rel);
    var col = albedo * (sunlight * ndl * shadow + sky_light(n, in.rel) * ao);
    if frame.fire_count.x > 0.0 {
        col += albedo * fire_light(in.rel, n);
        col += vec3<f32>(1.0, 0.28, 0.04) * embers * (2.0 + 1.5 * sin(frame.sun_dir.w * 7.0 + in.rel.x * 3.0));
    }
    if sheen > 0.0 {   // [materials]
        col += sunlight * mat_sheen(n, -in.rel / dist, sun, ground.rough) * ndl * shadow * sheen * 0.35;
    }
    return vec4<f32>(col, paint.a);
}

// The ground at a fragment, as `terrain_fragment` writes it.
fn terrain_shade(in: TerrainOut) -> vec4<f32> {
    // [materials] How the ground position and its height change from one pixel to the next, for
    // every texture fetch, and for the waves on the water.
    let tex_gx = dpdx(in.rel.xz);
    let tex_gy = dpdy(in.rel.xz);
    let tex_gh = vec2<f32>(dpdx(in.rel.y), dpdy(in.rel.y));
    // Where the sea is so deep that its bed is not seen, only the water is. Where little of the bed
    // shows through, or without the sea's detail, the bed is its plain colour.
    // (One call of `lit_ground`, which is big: two would make the shader slower everywhere.)
    let undersea = under_sea(in.rel);
    let seen = !undersea || in.rel.y + frame.cam_wrap.y > -OPAQUE_DEPTH;
    var ground = vec4<f32>(0.0);
    if !undersea || (seen && SEA_DETAIL && bed_seen(in.rel) > 0.3) {
        ground = lit_ground(in, tex_gx, tex_gy, tex_gh, undersea);
    } else if seen {
        let n = normalize(in.normal);
        let sun = frame.sun_dir.xyz;
        let lit = light_at(in.rel) * max(dot(n, sun), 0.0) * cloud_shadow(in.rel) + sky_light(n, in.rel);
        ground = vec4<f32>(in.color * lit, 0.0);
    }
    // Under the sea: what comes up through the water, for the sea's surface to be drawn over it.
    if undersea {
        return under_sea_color(ground.rgb, in.rel, vec3<f32>(tex_gx.x, tex_gh.x, tex_gx.y), vec3<f32>(tex_gy.x, tex_gh.y, tex_gy.y), true);
    }
    // Rivers: shallow at the banks, some three metres deep in the middle.
    var col = ground.rgb;
    let river = smoothstep(0.35, 0.65, in.cover.y) * (1.0 - ground.a);
    if river > 0.0 {
        col = mix(col, river_color(col, in.rel, tex_gx, tex_gy, 0.2 + 2.8 * smoothstep(0.5, 0.95, in.cover.y)), river);
    }
    // Alpha: no foam on any sea over it (`under_sea_color`).
    return vec4<f32>(apply_fog(col, in.rel), 0.0);
}

@fragment
fn terrain_fragment(in: TerrainOut) -> @location(0) vec4<f32> {
    return terrain_shade(in);
}

// The ground mirrored in the sea (`reflection.rs`), covering the sky there: a tile all above it,
// and one the sea reaches, with what is under it left out.
@fragment
fn terrain_mirror_fragment(in: TerrainOut) -> @location(0) vec4<f32> {
    return vec4<f32>(terrain_shade(in).rgb, 1.0);
}

@fragment
fn terrain_coast_fragment(in: TerrainOut) -> @location(0) vec4<f32> {
    if mirror_hidden(in.rel) {
        discard;
    }
    return vec4<f32>(terrain_shade(in).rgb, 1.0);
}

// The ground above the sea as it is, before its mirror is drawn: into the depth alone.
@fragment
fn terrain_depth_fragment(in: TerrainOut) -> @location(0) vec4<f32> {
    if mirror_hidden(in.rel) {
        discard;
    }
    return vec4<f32>(0.0);
}

// What is under the sea is not mirrored in it (`reflection.rs`).
fn mirror_hidden(rel: vec3<f32>) -> bool {
    return rel.y < -frame.cam_wrap.y;
}

// ---------------------------------------------------------------- water
//
// The sea and the rivers, in two parts drawn by different pipelines:
//
// - What comes up through the surface (`water_column`): the water's own colour and the bed under
//   it, dimmed by the depth, is drawn by whatever is under the sea, the ground (`terrain_fragment`)
//   or a thing afloat (`object_fragment`), with `under_sea_color`. It knows the depth exactly, so it
//   also puts down the surf along the shore (`surf`), and the foam where something went in; it writes
//   how much foam in alpha (`under_sea_color`).
// - The surface itself (`sea_fragment`), a plane at sea level blended over that: the slopes of the
//   waves (`sea.rs`, `frame.waves`), the sky and its clouds mirrored (`water_sky`), the sun and
//   the moon glittering (`glitter`) and the whitecaps. It lets through 1 - F (Fresnel) of what is
//   under it and covers what the foam leaves (the blend in `pipelines.rs`). Beyond the ground it
//   is deep water by itself. Kept out of the ground's shader, which it would make slower everywhere.
//
// Each train of waves finer than a pixel can show is left out of the normal and its slopes go into
// the slope covariance the light is spread with, so near and far water shine alike and nothing
// shimmers (the way of Bruneton, Neyret and Holzschuch 2010, "Real-time realistic ocean lighting
// using seamless transitions from geometry to BRDF"). Rivers are calm and drawn by the ground in
// one go (`river_color`).

// What the quality setting changes: `pipelines.rs` rewrites this line for the lower qualities.
const SEA_DETAIL: bool = true;

// Light in clear coastal sea: absorption (pure water, with a little chlorophyll and yellow matter,
// which take blue) and backscattering per metre, red, green and blue.
const SEA_ABSORB = vec3<f32>(0.36, 0.066, 0.034);
const SEA_BACK = vec3<f32>(0.0019, 0.0026, 0.0036);
// A river: brown with humic matter, which takes the blue most, and silt that scatters.
const RIVER_ABSORB = vec3<f32>(0.55, 0.36, 0.55);
const RIVER_BACK = vec3<f32>(0.009, 0.010, 0.007);
// How much more the light seen through the water is dimmed than the light that reaches down
// (forward scattering blurs it; the eye's path is not the sun's).
const VIEW_SCATTER: f32 = 22.0;
// The sun's disc, seen in a mirror, spans slopes of half its radius (0.0046): its variance per axis.
const SUN_SLOPE2: f32 = 5.3e-6;
// The sea's bed as seen (`sea_column`, art-directed): the absorption it is seen through, per metre
// (clearer than the water's own colour, `SEA_ABSORB`), how much its backscattering dims it on the
// way up, the depth it seems to lie at for its true one, the longest path it is seen along in
// depths, the tint of sand seen through it and the turquoise glow of pale sand under it.
const SEA_CLEAR = vec3<f32>(0.55, 0.040, 0.036);
const SEA_SEEN_SCATTER: f32 = 10.0;
const SEA_SEEN_DEPTH: f32 = 0.8;
const SEA_SEEN_PATH: f32 = 2.5;
const SEA_BED_TINT = vec3<f32>(0.8, 1.0, 0.98);
const SEA_GLOW = vec3<f32>(0.0, 0.05, 0.045);
// Below this depth (m) the bed is not seen at all: the water is its own colour.
const OPAQUE_DEPTH: f32 = 45.0;
// How much of the turn the waves give a reflected ray the eye sees in what it meets: all of it for
// what lies far beyond the water, half for what is as far from the water as the eye.
const MIRROR_PARALLAX: f32 = 0.7;
// The light through a crest: the water it crosses, m, and the share of the sun it lets out towards the eye.
const CREST_PATH: f32 = 4.0;
const CREST_THROUGH: f32 = 0.05;
// The height and the folding of a crest, each in standard deviations, add to one of about as many.
const BREAKING: f32 = 0.58;

struct Waves {
    slope: vec2<f32>,   // the slope (dh/dx, dh/dz) of the trains a pixel resolves
    cov: vec3<f32>,     // slope covariance (xx, zz, xz) of those it does not
    height: f32,        // height of the resolved surface over its mean, m
    fold: f32,          // how much the resolved surface is squeezed together here (see `sea_waves`)
    fold_var: f32,      // the variance of `fold` over the sea
};

// The trains of waves at `p` (x, z relative to the camera) moving by `dx`, `dy` from one pixel to
// the next. The short ones are raised and lowered in patches by the gusts (`gust`), and each is
// shifted along its way by `warp` metres, which changes slowly over the sea, so that their crests
// do not repeat in a lattice. A train is faded out of the normal from 0.15 to 0.26 cycles per
// pixel, and what it loses is added to the covariance.
// `fold` is how much the surface is squeezed together where it stands: the water of a wave gathers
// towards its crest (Gerstner's waves), by its steepness times the sine of its height, and a crest
// folds over and breaks where the trains together squeeze it most.
fn sea_waves(p: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, gust: f32, warp: f32) -> Waves {
    var o = Waves(vec2<f32>(0.0), vec3<f32>(0.0), 0.0, 0.0, 0.0);
    // The least a pixel spans in any direction, m: once a train is too fine for that, so are all
    // the shorter ones, and their slopes go into the covariance at once.
    let narrow = abs(dx.x * dy.y - dx.y * dy.x) / max(sqrt(dot(dx, dx) + dot(dy, dy)), 1e-6);
    for (var i = 0; i < i32(frame.sea.y); i++) {
        let wave = frame.waves[i];
        // Waves per metre.
        let kc = wave.z;
        if kc * narrow > 0.26 {
            o.cov += frame.wave_rest[i].xyz * mix(1.0, gust * gust, 0.8);
            break;
        }
        let dir = wave.xy;
        let s = frame.wave_rest[i].w * mix(1.0, gust, smoothstep(0.1, 0.4, kc));
        // Cycles per pixel, squared.
        let along = vec2<f32>(dot(dir, dx), dot(dir, dy)) * kc;
        let keep = 1.0 - smoothstep(0.0225, 0.0676, dot(along, along));
        if keep < 1.0 {
            let lost = 0.5 * s * s * (1.0 - keep * keep);
            o.cov += vec3<f32>(dir.x * dir.x, dir.y * dir.y, dir.x * dir.y) * lost;
        }
        if keep > 0.0 {
            let turn = turn_sin_cos((dot(dir, p) + warp) * kc + wave.w);
            o.slope += dir * (s * keep * turn.y);
            o.height += s / (2.0 * PI * kc) * keep * turn.x;
            o.fold += s * keep * turn.x;
            o.fold_var += 0.5 * s * s * keep * keep;
        }
    }
    return o;
}

// The sine and cosine of `t` turns, to within a thousandth: a parabola, refined. The waves take
// many of them a pixel, and the hardware's exact ones cost several times more.
fn turn_sin_cos(t: f32) -> vec2<f32> {
    let u = fract(vec2<f32>(t + 0.5, t + 0.75)) * 2.0 - 1.0;
    let y = 4.0 * u * (1.0 - abs(u));
    return 0.225 * (y * abs(y) - y) + y;
}

// The reflectance of water facing the light squarely (an index of 1.333).
const WATER_FACING: f32 = 0.02;

// How much of a rough surface is hidden from a direction by the surface itself, as Smith counts
// it: G1 = 1 / (1 + this) is the share seen. For slopes spread as a Gaussian of variance
// `variance` along the direction's azimuth, and the direction at the cosine `cosine` from the
// mean normal, G1 is Walter et al. 2007's rational fit (their equation 27).
fn hidden(cosine: f32, variance: f32) -> f32 {
    let c = clamp(cosine, 1e-3, 0.9999);
    // The direction's own slope over the slopes' deviation, and the square root of two.
    let a = c / sqrt(2.0 * max(variance, 1e-7) * (1.0 - c * c));
    if a >= 1.6 {
        return 0.0;
    }
    return (1.0 + a * (2.276 + 2.577 * a)) / (a * (3.535 + 2.181 * a)) - 1.0;
}

// The variance of the slopes of covariance `cv` (xx, yy, xy in the frame `tx`, `ty`) along the
// azimuth of `d`.
fn variance_towards(d: vec3<f32>, tx: vec3<f32>, ty: vec3<f32>, cv: vec3<f32>) -> f32 {
    let a = vec2<f32>(dot(d, tx), dot(d, ty));
    return (cv.x * a.x * a.x + cv.y * a.y * a.y + 2.0 * cv.z * a.x * a.y) / max(dot(a, a), 1e-8);
}

// The glitter: the light from `l` (1 falling on a surface facing it) that the facets of the water
// mirror to the eye, which looks along `v`. The facets lie round the normal `n` with slopes spread
// as a Gaussian of covariance `cv` in the frame `tx`, `ty`. Only those facing half way between the
// light and the eye mirror one to the other: the light is Fresnel's share times how many facets
// have that slope, over 4 cos(eye) cos^4(facet), less what hides them (Bruneton et al. 2010, the
// reflectance of Ross et al. 2005).
fn glitter(l: vec3<f32>, v: vec3<f32>, n: vec3<f32>, tx: vec3<f32>, ty: vec3<f32>, cv: vec3<f32>) -> f32 {
    let facet = normalize(l - v);
    let facing = dot(facet, n);
    if facing <= 0.01 {
        return 0.0;
    }
    // The facet's slope, and how many have it: exp(-s C^-1 s / 2) / (2 pi sqrt |C|).
    let slope = vec2<f32>(dot(facet, tx), dot(facet, ty)) / facing;
    let det = max(cv.x * cv.y - cv.z * cv.z, 1e-12);
    let solved = vec2<f32>(cv.y * slope.x - cv.z * slope.y, cv.x * slope.y - cv.z * slope.x) / det;
    let share = exp(-0.5 * dot(slope, solved)) / (2.0 * PI * sqrt(det));
    let eye = max(dot(-v, n), 0.01);
    let bounced = WATER_FACING + (1.0 - WATER_FACING) * pow(1.0 - saturate(dot(l, facet)), 5.0);
    let hiding = 1.0 + hidden(max(dot(l, n), 0.01), variance_towards(l, tx, ty, cv)) + hidden(eye, variance_towards(v, tx, ty, cv));
    let f2 = facing * facing;
    return bounced * share / (4.0 * eye * f2 * f2 * hiding);
}

// The sky seen in the water along `r`, with the clouds of the layer where the ray meets it. `p` is
// the point of the water relative to the camera; `spread` how far the waves' slopes spread the
// reflected rays round `r` (radians, about), which blurs the volumetric clouds mirrored.
fn water_sky(r: vec3<f32>, p: vec3<f32>, spread: f32) -> vec3<f32> {
    let lifted = normalize(r + vec3<f32>(0.0, max(0.03 - r.y, 0.0), 0.0));
    var sky = sky_color(lifted);
    if !SEA_DETAIL {
        // Overcast, the mirrored sky is grey.
        return mix(sky, cloud_underside(p + lifted * 2000.0), frame.misc.w * 0.55 * smoothstep(0.02, 0.3, lifted.y));
    }
    if frame.cam_wrap.w > 0.5 {
        // The volumetric clouds: where the reflected ray meets the level of their bases, what their
        // shade on the ground has there (drawn beside the heights) says how much cloud is above.
        let rise = frame.misc.y - p.y;
        if rise > 0.0 && frame.hmap.w > 0.5 {
            let t = rise / lifted.y;
            let uv = (p.xz + lifted.xz * t - frame.hmap.xy) / (2.0 * frame.hmap.z) + 0.5;
            let off = max(abs(uv.x - 0.5), abs(uv.y - 0.5));
            // The rays the waves spread round this one meet the bases over a patch as wide as the
            // spread at that distance: the cover averaged over it (the map's own texels, a few tens
            // of metres, at least), not the sharp edge of the shade one ray would see. Eight taps on
            // two rings, turned by the pixel so that their pattern does not show.
            let texel = 2.0 * frame.hmap.z / f32(textureDimensions(height_map).x);
            let radius = max(t * spread, 1.5 * texel) / (2.0 * frame.hmap.z);
            let turn = fract(sin(dot(p.xz, vec2<f32>(12.9898, 78.233))) * 43758.547) * 6.2831853;
            var cover = textureSampleLevel(height_map, height_sampler, uv, 0.0).g * 0.2;
            for (var k = 0; k < 8; k++) {
                let a = turn + f32(k) * 0.7853982;
                let ring = select(1.0, 0.5, k % 2 == 0);
                cover += textureSampleLevel(height_map, height_sampler, uv + vec2<f32>(cos(a), sin(a)) * radius * ring, 0.0).g * 0.1;
            }
            cover *= 1.0 - smoothstep(0.4, 0.5, off);
            if cover > 0.003 {
                let at = p + lifted * t;
                let lit = light_at(at) * 0.35 + sky_light(vec3<f32>(0.0, -1.0, 0.0), at) * 0.9 + sky_light(vec3<f32>(0.0, 1.0, 0.0), at) * 0.25;
                let haze = haze_at(lifted * t);
                sky = mix(sky, lit * haze.through + haze.light, cover);
            }
        }
        return sky;
    }
    // The clouds: their underside, where the reflected ray meets the middle of the layer.
    let up = frame.misc.y + frame.extra.x * 0.4 - p.y;
    if frame.misc.w > 0.01 && up > 0.0 {
        let t = up / lifted.y;
        let xz = p.xz + lifted.xz * t;
        let reach = length(xz);
        if reach < 70000.0 {
            let th = mix(0.90, 0.36, frame.misc.w);
            let cover = smoothstep(th - 0.01, th + 0.14, cloud_noise(cloud_coord(xz), 50000.0)) * (1.0 - smoothstep(30000.0, 70000.0, reach));
            // The haze between them, as if from the eye.
            let haze = haze_at(lifted * t);
            sky = mix(sky, cloud_underside(p + lifted * t) * haze.through + haze.light, cover);
        }
    }
    return sky;
}

// The light that comes up out of the water to the surface: the water's own (light scattered back
// by it, `back` over `absorb`) and, under `depth` metres seen along `path` metres of water, the bed
// lit as `bed`. `light` is the light falling on the water (sun and sky).
fn water_column(absorb: vec3<f32>, back: vec3<f32>, light: vec3<f32>, depth: f32, path: f32, bed: vec3<f32>) -> vec3<f32> {
    // Deep water's reflectance (Morel and Prieur 1977): a third of backscattering over absorption.
    let own = 0.33 * back / (absorb + back) * light;
    // The sunlight that reaches the bed (refracted, it goes down no flatter than 48°) and what of
    // the bed's light comes back up along the eye's path.
    let down = exp(-(absorb + 4.0 * back) * depth * 1.3);
    let seen = exp(-(absorb + VIEW_SCATTER * back) * path);
    let through = down * seen;
    return own * (1.0 - through) + bed * through;
}

// The sea over its bed, art-directed (looks over physics): its own colour is deep water's
// (`water_column`), but the bed is seen through clearer water than that, along no more than a few
// times the depth however low the eye is, so that clear water over sand is the turquoise of
// Menorca's calas and the Caribbean's banks a metre or two deep, aquamarine where it deepens to
// some 15 m, the navy of deep water beyond; and pale sand under it glows turquoise with the light
// it sends up and the water scatters back to it.
fn sea_through(depth: f32, path: f32) -> vec3<f32> {
    let d = depth * SEA_SEEN_DEPTH;
    let along = min(path * SEA_SEEN_DEPTH, d * SEA_SEEN_PATH);
    // Gone by `OPAQUE_DEPTH`, where the ground stops being drawn under the sea.
    return smoothstep(OPAQUE_DEPTH, OPAQUE_DEPTH * 0.6, depth) * exp(-(SEA_CLEAR + 4.0 * SEA_BACK) * d * 1.3 - (SEA_CLEAR + SEA_SEEN_SCATTER * SEA_BACK) * along);
}

fn sea_column(light: vec3<f32>, depth: f32, path: f32, bed: vec3<f32>) -> vec3<f32> {
    let own = 0.33 * SEA_BACK / (SEA_ABSORB + SEA_BACK) * light;
    let through = sea_through(depth, path);
    // How pale the bed is (sand, not weed or rock), from its light over the light falling on the water.
    let pale = smoothstep(0.12, 0.45, luma(bed) / max(luma(light), 1e-4));
    let glow = SEA_GLOW * light * pale * through.g * (1.0 - through.r);
    return own * (1.0 - through) + bed * through * SEA_BED_TINT + glow;
}

// What a pixel of the bed gets from the light the waves focus: patches and lines that shift. `q` is
// the bed's position (x, z, wrapped), `fp` the pixel's size there, m.
fn caustics(q: vec2<f32>, depth: f32, fp: f32, t: f32) -> f32 {
    if !SEA_DETAIL {
        return 1.0;
    }
    let fine = 1.0 - smoothstep(0.12, 0.45, fp);
    if fine <= 0.0 || depth < 0.15 || depth > 12.0 {
        return 1.0;
    }
    let a = vnoise(q * 1.3 + vec2<f32>(t * 0.45, t * 0.27), WRAP_I);
    let b = vnoise(q * 1.3 - vec2<f32>(t * 0.31, t * 0.42) + 19.0, WRAP_I);
    let lines = pow(1.0 - abs(a - b), 14.0);
    let strength = smoothstep(0.15, 0.8, depth) * exp(-depth * 0.2) * fine * smoothstep(0.05, 0.3, frame.sun_dir.y) * frame.sun_color.w;
    return 1.0 + (lines * 2.2 - 0.25) * strength;
}

// A wave breaks where its height reaches this share of the depth (McCowan's criterion).
const BREAKS_AT: f32 = 0.78;
// Seconds the foam a breaker leaves on the water lasts (to 1 / e), and how white it starts,
// beside the breaker's own front.
const FOAM_LASTS: f32 = 5.0;
const FOAM_LEFT: f32 = 0.55;

// The depth at which the wave number `wave` breaks at `w` along the shore, for waves of the mean
// height `mean`: each wave has its own height, and is taller along some stretches of its crest
// than along others (it breaks there first, further out), and shoaling has raised it by a fifth.
fn breaking_depth(wave: i32, w: vec2<f32>, mean: f32) -> f32 {
    let own = ihash(vec2<i32>(wave, 311));
    let stretch = smoothstep(0.25, 0.75, vnoise(w / 128.0 + own * 53.0, WRAP_I / 128));
    return mean * (0.75 + 0.5 * own) * mix(0.6, 1.15, stretch) * 1.2 / BREAKS_AT;
}

// How strong the bore of a wave that breaks at the depth `breaking` is at `depth`: nothing before
// it breaks, whitest as it plunges, then a bore that loses height with the water under it.
fn bore(depth: f32, breaking: f32) -> f32 {
    let x = depth / breaking;
    return (1.0 - smoothstep(0.82, 1.0, x)) * mix(0.75, 1.0, smoothstep(0.3, 0.8, x));
}

// How white the surf makes the water, 0..1, where the sea is `h` metres deep over a bed that
// rises towards the shore along `shore` (unit, x z) by `s` metres a metre. `w` is the point's
// wrapped position, `fp` the pixel's size there, `t` the time.
//
// The swell comes in at sqrt(g h), ever slower, so its crests close up and turn to face the shore:
// a crest is found by the time it still needs to reach the water's edge over a bed of that slope.
// Each breaks where the water is too shallow for its height (`breaking_depth`) and runs in as a
// bore: a white front a few metres wide and the foam it leaves, which fades in `FOAM_LASTS`. At
// the edge the bore runs up the beach as a sheet of foam that drains away until the next one.
// A wave too fine for the pixel is the mean of all that over its cycle.
fn surf(h: f32, shore: vec2<f32>, s: f32, w: vec2<f32>, fp: f32, t: f32) -> f32 {
    let g = 9.81;
    let period = frame.sea_more.w;
    // The mean height of waves is 0.63 of the significant one; lower where the wind blows off the shore.
    let mean = frame.sea.z * 0.63 * mix(0.65, 1.0, 0.5 + 0.5 * dot(frame.sea_more.xy, shore));
    if mean < 0.02 || h > 1.75 * mean / BREAKS_AT + 0.5 {
        return 0.0;
    }
    // The water a bore still has under it at the very edge.
    let least = 0.25;
    let depth = max(h, 0.0) + least;
    // Seconds from here to the edge, the integral of dx / sqrt(g h) up a bed of slope s, and the
    // length of the waves here.
    let run = 2.0 * (sqrt(depth) - sqrt(least)) / (s * sqrt(g));
    let wavelength = period * sqrt(g * depth);
    let pixel = fp / wavelength;
    // The crests do not arrive all along the shore at once, and a front is ragged: lobes a few
    // metres wide run a metre ahead of the rest, as long as a pixel can show them.
    let late = (vnoise(w / 512.0 + 3.0, WRAP_I / 512) - 0.5) * 1.6;
    let fine = 1.0 - smoothstep(1.0, 5.0, fp);
    let lobes = (vnoise(w / 8.0 + vec2<f32>(t * 0.03, 0.0), WRAP_I / 8) - 0.5) * fine;
    // The foam left is a lace whose holes grow as it ages.
    let lace = mix(0.5, vnoise(w / 2.0 - vec2<f32>(0.0, t * 0.05), WRAP_I / 2), 1.0 - smoothstep(0.4, 2.0, fp));
    // Waves gone by here, counted from the start: the whole ones say which wave went by last and
    // the fraction how long ago, in periods.
    let count = (t + run) / period + late + lobes * 3.0 / wavelength;
    let wave = floor(count);
    let since = count - wave;

    let breaking = mean * 1.2 / BREAKS_AT;
    let typical = bore(h, breaking);
    let last = bore(h, breaking_depth(i32(wave), w, mean));
    let next = bore(h, breaking_depth(i32(wave) + 1, w, mean));
    // The front, as wide as the bore is high and a little more, spread over the pixel; and the
    // foam left by this wave and, ever fainter and more open, by those before it.
    let front = (1.2 + 2.0 * min(h, breaking) * BREAKS_AT) / wavelength;
    let kept = exp(-period / FOAM_LASTS);
    let fade = exp(-since * period / FOAM_LASTS);
    let laced = smoothstep(since - 0.45, since + 0.15, lace);
    let older = typical * FOAM_LEFT * kept / (1.0 - kept) * smoothstep(0.35, 0.75, lace) * 2.0;
    var foam = last * ((1.0 - FOAM_LEFT) * front / (front + pixel) * exp(-since / (front + pixel)) + FOAM_LEFT * fade * laced) + older * fade;
    // The next front, coming: it does not begin sharper than a pixel.
    foam = mix(foam, next + older, smoothstep(1.0 - 1.5 * pixel - 0.004, 1.0, since));
    // Finer than a pixel: the mean over a cycle of the front and of all the foam left.
    let mean_foam = typical * ((1.0 - FOAM_LEFT) * min(front, 1.0) + 0.7 * FOAM_LEFT * FOAM_LASTS / period);
    foam = mix(foam, mean_foam, smoothstep(0.2, 0.6, pixel));

    // The swash. A bore runs up a beach some sqrt(H L) along it, L the swell's length in deep
    // water, g T^2 / 2 pi (Hunt's run-up); here that is how far out from the edge its sheet of
    // foam lies, each bore's its own, with a scalloped rim. It thins and opens as it drains, from
    // the outside in; a line of foam stays at the very edge.
    let arrived = t / period + late;
    let ago = fract(arrived);
    let reach = 0.7 * sqrt(mean * g * period * period / (2.0 * PI)) * (0.7 + 0.6 * ihash(vec2<i32>(i32(floor(arrived)), 97))) * (1.0 + 0.7 * lobes);
    let off = h / s;
    let sheet = exp(-2.5 * ago) * (1.0 - smoothstep(0.35 * reach, reach * (1.0 - 0.5 * ago) + fp, off)) * smoothstep(0.8 * ago - 0.45, 0.8 * ago + 0.15, lace);
    let rim = 0.8 * (1.0 - smoothstep(0.0, 0.6 + fp, off));
    // Narrower than a pixel, an even fringe.
    let swash = mix(max(sheet, rim), 0.45 * (1.0 - smoothstep(0.0, reach + fp, off)), smoothstep(0.4, 1.2, fp / reach));
    return saturate(max(foam, swash)) * smoothstep(0.03, 0.4, mean);
}

// The bed sloping up to the shore at a point (x, z relative to the camera), from the height map:
// xy = the way up (unit), z = how steep. Over 64 m texels, so the crests follow the shore's general line.
fn shore_slope(xz: vec2<f32>) -> vec3<f32> {
    if frame.hmap.w < 0.5 {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    let extent = 2.0 * frame.hmap.z;
    let uv = (xz - frame.hmap.xy) / extent + 0.5;
    if outside(uv) {
        return vec3<f32>(0.0);
    }
    let e = 1.0 / 1024.0;
    let gx = height_at(uv + vec2<f32>(e, 0.0)) - height_at(uv - vec2<f32>(e, 0.0));
    let gz = height_at(uv + vec2<f32>(0.0, e)) - height_at(uv - vec2<f32>(0.0, e));
    let g = vec2<f32>(gx, gz) / (2.0 * e * extent);
    let s = length(g);
    return vec3<f32>(g / max(s, 1e-5), s);
}


// The light falling on the water at `p` (relative to the camera): the sun, shadowed, and the sky.
fn water_light(p: vec3<f32>) -> vec3<f32> {
    let shade = body_shadow(p) * cloud_shadow(p);
    return light_at(p) * max(frame.sun_dir.y, 0.0) * shade + sky_light(vec3<f32>(0.0, 1.0, 0.0), p);
}

// How far on the screen the mirror is read off for a facet facing `n`: the turn of the ray it
// reflects from the flat sea's, as a share of the screen.
fn mirror_turn(v: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    let turn = reflect(v, n) - reflect(v, vec3<f32>(0.0, 1.0, 0.0));
    return vec2<f32>(dot(turn, frame.cam_right.xyz) / frame.cam_right.w, -dot(turn, frame.cam_up.xyz) / frame.cam_up.w) * (0.5 * MIRROR_PARALLAX);
}

// What the sea mirrors of the world (`reflection.rs`, premultiplied: rgb, and in alpha how much it
// covers the sky) at the pixel `pixel`, for the facets round the normal `n` seen along `v`, spread
// by `spread` (a tilt of the normal along the view: one look each way, as for the sky). The mirror
// was drawn for a flat sea, so the ray turned by the waves is found by its turn seen on the screen;
// what it meets lies beyond the water, so the eye sees some of that turn (`MIRROR_PARALLAX`).
fn mirrored_seen(pixel: vec2<f32>, v: vec3<f32>, n: vec3<f32>, spread: vec3<f32>) -> vec4<f32> {
    let uv = pixel * frame.mirror.zw;
    var seen = textureSampleLevel(mirrored, height_sampler, uv + mirror_turn(v, n), 0.0);
    if dot(spread, spread) > 0.0 {
        let a = textureSampleLevel(mirrored, height_sampler, uv + mirror_turn(v, normalize(n - spread)), 0.0);
        let b = textureSampleLevel(mirrored, height_sampler, uv + mirror_turn(v, normalize(n + spread)), 0.0);
        seen = seen * (2.0 / 3.0) + (a + b) / 6.0;
    }
    return seen;
}

// The share of the light water mirrors (Fresnel's), as a mean over facets whose slopes spread by
// `sv` (one deviation, along the view) round a normal at the cosine `ndv` from the eye: the fit of
// Bruneton et al. 2010 to that mean. Rough water mirrors less of the horizon than flat water does.
fn mirror_share(ndv: f32, sv: f32) -> f32 {
    let grazing = pow(1.0 - ndv, 5.0 * exp(-2.69 * sv)) / (1.0 + 22.7 * pow(sv, 1.5));
    return saturate(WATER_FACING + (1.0 - WATER_FACING) * grazing);
}

// Foam lit as what falls on it, a little less white than snow.
fn foam_color(light: vec3<f32>) -> vec3<f32> {
    return light * 0.75;
}

// ---------------------------------------------------------------- under the sea

// Where the ray from the eye to `rel`, below the sea, crosses it, and how that point moves from one
// pixel to the next given how `rel` does (`dx`, `dy`).
struct Crossing {
    at: vec3<f32>,
    dx: vec2<f32>,
    dy: vec2<f32>,
};

fn sea_crossing(rel: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>) -> Crossing {
    let s = frame.cam_wrap.y / max(-rel.y, 1e-3);
    // d(s rel) = s (d rel - rel d(rel.y) / rel.y)
    return Crossing(rel * s, s * (dx.xz - rel.xz * dx.y / rel.y), s * (dy.xz - rel.xz * dy.y / rel.y));
}

// How much of the light of the bed at `rel`, under the sea, reaches the eye (green, the colour
// the water lets through best): `water_column`'s `through`.
fn bed_seen(rel: vec3<f32>) -> f32 {
    let depth = -(rel.y + frame.cam_wrap.y);
    let path = length(rel) * depth / max(-rel.y, 1e-3);
    return sea_through(depth, path).g;
}

// Whether `rel` is under the sea, seen from above it.
fn under_sea(rel: vec3<f32>) -> bool {
    return frame.cam_wrap.y > 0.05 && rel.y + frame.cam_wrap.y < 0.0;
}

// What a fragment under the sea at `rel`, lit as `lit`, sends up through the water, hazed as far
// as the surface, for the sea's surface to be drawn over it (`sea_fragment`); in alpha, how much
// foam lies on the water over it. `bed`: the fragment is the ground, with the light the waves focus
// on it and the surf over it. `dx`, `dy`: how `rel` moves from one pixel to the next.
fn under_sea_color(lit: vec3<f32>, rel: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>, bed: bool) -> vec4<f32> {
    let c = sea_crossing(rel, dx, dy);
    let p = c.at;
    let depth = -(rel.y + frame.cam_wrap.y);
    let t = frame.sun_dir.w;
    let light = water_light(p);
    let fp = max(length(c.dx), length(c.dy));
    var seen = lit;
    var foam = 0.0;
    if bed && depth < OPAQUE_DEPTH {
        seen *= caustics(wrap_mod(rel.xz + frame.cam_wrap.xz + WRAP * 32.0, WRAP), depth, fp, t);
        var shore = vec3<f32>(0.0);
        if depth < 12.0 {
            shore = shore_slope(p.xz);
        }
        if shore.z > 0.0 {
            let w = wrap_mod(p.xz + frame.cam_wrap.xz + WRAP * 32.0, WRAP);
            let white = surf(depth, shore.xy, clamp(shore.z, 0.008, 0.12), w, fp, t);
            if white > 0.0 {
                // Laced close up, an even white far off.
                let far = smoothstep(0.5, 3.0, fp);
                foam = white * (0.55 + 0.6 * vnoise(w / 4.0 + vec2<f32>(t * 0.1, 0.0), WRAP_I / 4) * (1.0 - far) + 0.3 * far);
            }
        }
    }
    if frame.fire_count.z > 0.0 {
        foam = max(foam, foam_at(p));
    }
    let dist = length(p);
    foam = saturate(foam) * (1.0 - smoothstep(4000.0, 14000.0, dist) * 0.5);
    let under = sea_column(light, depth, length(rel - p), seen);
    // The surface lets through 1 - F of this and draws its own light over 1 - foam of it: the foam
    // is put here, brighter by what F will take of it.
    let through = 1.0 - mirror_share(-p.y / dist, 0.12);
    let col = under * (1.0 - foam) + foam_color(light) * foam / max(through, 0.3);
    return vec4<f32>(apply_fog(col, p), foam);
}

// ---------------------------------------------------------------- rivers

// A river over its bed lit as `lit` at `rel`, `depth` metres deep; `dx`, `dy` how `rel` moves from
// one pixel to the next. Calm: ripples that drift with the current and fade into roughness before
// they are finer than a pixel, the sky and the banks mirrored, the sun glittering. Before the haze.
fn river_color(lit: vec3<f32>, rel: vec3<f32>, dx: vec2<f32>, dy: vec2<f32>, depth: f32) -> vec3<f32> {
    let dist = length(rel);
    let v = rel / dist;
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let sun = frame.sun_dir.xyz;
    let t = frame.sun_dir.w;
    let w = wrap_mod(rel.xz + frame.cam_wrap.xz + WRAP * 32.0, WRAP);
    let fp = max(length(dx), length(dy));
    // Ripples of a metre or so: their slope fades out between a fifth and a half of a cell a
    // pixel, and its variance goes into the roughness.
    let keep = 1.0 - smoothstep(0.2, 0.5, fp);
    let ripple = 0.07 * (0.6 + 0.4 * min(frame.sea.x / 6.0, 1.0));
    var slope = vec2<f32>(0.0);
    if keep > 0.0 {
        slope = (vec2<f32>(vnoise(w + vec2<f32>(t * 0.5, 0.0), WRAP_I), vnoise(w + vec2<f32>(19.0, -t * 0.4), WRAP_I)) - 0.5) * ripple * 2.0 * keep;
    }
    let s2 = 0.0015 + ripple * ripple * 0.33 * (1.0 - keep * keep);
    let n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
    let ndv = max(dot(-v, n), 0.01);
    let fres = mirror_share(ndv, sqrt(s2));
    let r = reflect(v, n);
    var refl = sky_color(normalize(r + up * max(0.03 - r.y, 0.0)));
    let sunlight = light_at(rel);
    // A river lies low between its banks and their trees, which the water mirrors low down.
    let banks = vec3<f32>(0.035, 0.06, 0.03) * (sunlight * max(sun.y, 0.0) * 0.5 + sky_light(up, rel));
    refl = mix(refl, banks, (1.0 - smoothstep(0.0, 0.1, r.y)) * 0.6);
    let shade = body_shadow(rel) * cloud_shadow(rel);
    let light = sunlight * max(sun.y, 0.0) * shade + sky_light(up, rel);
    let under = water_column(RIVER_ABSORB, RIVER_BACK, light, depth, depth * 1.25, lit);
    // The sun glittering on the ripples: their slopes, and the sun's disc, spread round `n` as a
    // round Gaussian (`glitter` without what hides the facets, which so gentle slopes do not need).
    let sg = s2 + SUN_SLOPE2;
    let h = normalize(sun - v);
    let zh = max(dot(h, n), 0.01);
    let density = exp(-0.5 * (1.0 - zh * zh) / (zh * zh * sg)) / (2.0 * PI * sg * zh * zh * zh * zh);
    let glint = (0.02 + 0.98 * pow(1.0 - max(dot(-v, h), 0.0), 5.0)) * density / (4.0 * ndv);
    var col = mix(under, refl, fres) + sunlight * glint * shade;
    if frame.fire_count.x > 0.0 {
        col += fire_light(rel, n) * (0.04 + fres);
    }
    if frame.fire_count.z > 0.0 {
        col = mix(col, foam_color(light), foam_at(rel));
    }
    return col;
}

// ---------------------------------------------------------------- the sea's surface

// Two triangles covering a square of side 2.
const CORNERS = array<vec2<f32>, 6>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));

struct SeaOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
};

@vertex
fn sea_vertex(@builtin(vertex_index) vid: u32) -> SeaOut {
    var corners = CORNERS;
    let c = corners[vid];
    let rel = vec3<f32>(c.x * 250000.0, -frame.cam_wrap.y, c.y * 250000.0);
    var o: SeaOut;
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    return o;
}

// The surface of the sea over what is under it (`under_sea_color`), premultiplied: what it reflects,
// glitters and whitens in rgb, how much of what is under it it hides in alpha.
@fragment
fn sea_fragment(in: SeaOut) -> @location(0) vec4<f32> {
    let dx = dpdx(in.rel.xz);
    let dy = dpdy(in.rel.xz);
    let p = in.rel;
    let dist = length(p);
    let v = p / dist;   // from the eye
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let sun = frame.sun_dir.xyz;
    let t = frame.sun_dir.w;
    let w = wrap_mod(p.xz + frame.cam_wrap.xz + WRAP * 32.0, WRAP);
    let fp = max(length(dx), length(dy));

    // Gusts ruffle some patches and leave others glassier (cat's paws); in light air the calm ones
    // are slicks, the ripples damped out of them, where the water mirrors the coast. They drift downwind. The noise goes to its mean before its cells are finer than a pixel.
    let drift = frame.sea_more.xy * (t * frame.sea.x);
    let g1 = mix(vnoise((w - drift * 0.6) / 256.0, WRAP_I / 256), 0.5, smoothstep(25.0, 90.0, fp));
    let calm = 1.0 - smoothstep(2.0, 7.0, frame.sea.x);
    let gust = mix(mix(0.45, 0.12, calm), 1.35, smoothstep(0.25, 0.75, g1 * 1.1 - 0.05 - 0.25 * calm));
    // How far the trains are shifted here (`sea_waves`): still, so that it does not change their speed.
    let warp = (mix(vnoise(w / 128.0 + 11.0, WRAP_I / 128), 0.5, smoothstep(12.0, 45.0, fp)) - 0.5) * 14.0;

    let waves = sea_waves(p.xz, dx, dy, gust, warp);
    let cov = waves.cov + frame.sea_rough.xyz * mix(1.0, gust * gust, 0.6 + 0.35 * calm) + vec3<f32>(SUN_SLOPE2, SUN_SLOPE2, 0.0);
    let n = normalize(vec3<f32>(-waves.slope.x, 1.0, -waves.slope.y));
    let tx = normalize(vec3<f32>(1.0, 0.0, 0.0) - n * n.x);
    let ty = cross(tx, n);
    // Slope variance along the view.
    let vh = normalize(vec2<f32>(v.x, v.z) + vec2<f32>(1e-6, 0.0));
    let sv2 = cov.x * vh.x * vh.x + 2.0 * cov.z * vh.x * vh.y + cov.y * vh.y * vh.y;
    let fres = mirror_share(max(dot(-v, n), 0.0), sqrt(max(sv2, 0.0)));

    // The sky the facets mirror. Their slopes spread most along the view, which draws what is
    // mirrored out into a streak up and down; across it the sky hardly changes. So three looks:
    // the mean facet and those sqrt(3) deviations either side of it along the view, which for a
    // Gaussian count 2/3, 1/6 and 1/6 (Gauss and Hermite's rule), each besides by how much of it
    // the eye sees: a facet tilted towards the eye shows more of itself than one tilted away, and
    // one facing away shows nothing. Near the horizon the sea so mirrors more of the sky higher up.
    var refl = water_sky(reflect(v, n), p, 2.0 * sqrt(max(sv2, 0.0)) + 0.03);
    if SEA_DETAIL {
        let tilt = (tx * vh.x + ty * vh.y) * sqrt(3.0 * max(sv2, 0.0));
        let seen = max(dot(-v, n), 1e-4) * 4.0;
        let seen_near = max(dot(-v, n - tilt), 0.0);
        let seen_far = max(dot(-v, n + tilt), 0.0);
        let r_near = reflect(v, normalize(n - tilt));
        let r_far = reflect(v, normalize(n + tilt));
        refl = (refl * seen + sky_color(normalize(r_near + up * max(0.03 - r_near.y, 0.0))) * seen_near + sky_color(normalize(r_far + up * max(0.03 - r_far.y, 0.0))) * seen_far) / (seen + seen_near + seen_far);
    }
    // The land, trees, buildings and lights mirrored, over the sky where the reflected ray meets them.
    if frame.mirror.y > 0.5 {
        var spread = vec3<f32>(0.0);
        if SEA_DETAIL {
            spread = tx * vh.x + ty * vh.y;
            spread *= sqrt(3.0 * max(sv2, 0.0));
        }
        let world = mirrored_seen(in.position.xy, v, n, spread);
        refl = refl * (1.0 - world.a) + world.rgb;
    }

    // The sun (by night the moon) glittering on the facets.
    let shade = body_shadow(p) * cloud_shadow(p);
    let sunlight = light_at(p);
    var col = refl * fres + sunlight * glitter(sun, v, n, tx, ty, cov) * shade;
    if frame.fire_count.x > 0.0 {
        col += fire_light(p, n) * (0.04 + fres);
    }
    var cover = fres;
    let light = sunlight * max(sun.y, 0.0) * shade + sky_light(up, p);
    // Beyond the ground nothing is drawn under the sea: it is deep water there by itself.
    let beyond = smoothstep(frame.sea_more.z * 0.9, frame.sea_more.z, dist);
    if beyond > 0.0 {
        col += water_column(SEA_ABSORB, SEA_BACK, light, 1000.0, 1000.0, vec3<f32>(0.0)) * (1.0 - fres) * beyond;
        cover = mix(cover, 1.0, beyond);
    }

    // A crest seen against a low sun lets through the light that went into its back, green and blue
    // from the metres of water it crossed: only where the pixel resolves crests that stand high and
    // squeezed thin.
    let sigma = max(frame.sea.z * 0.25, 0.05);
    let folding = waves.fold / sqrt(max(waves.fold_var, 1e-6));
    let against = pow(saturate(dot(vh, normalize(sun.xz + vec2<f32>(1e-6, 0.0)))), 3.0) * smoothstep(-0.02, 0.06, sun.y) * (1.0 - smoothstep(0.2, 0.7, sun.y));
    if against > 0.0 {
        let thin = smoothstep(0.6, 2.2, folding) * smoothstep(-0.3, 1.0, waves.height / sigma) * (1.0 - smoothstep(60.0, 400.0, dist));
        col += sunlight * shade * exp(-SEA_ABSORB * CREST_PATH) * (CREST_THROUGH * against * thin * (1.0 - fres));
    }

    // Whitecaps where the crests stand high and fold over (`fold`): the two together, in standard
    // deviations, past `Sea::whitecap_threshold`, laced and drawn out downwind; far off, the part of
    // the sea that is white, as a mean.
    if frame.sea_rough.w > 1e-4 {
        let lace = mix(vnoise((w - drift * 0.05) / 8.0, WRAP_I / 8) * 0.6 + vnoise(w / 2.0, WRAP_I / 2) * 0.4, 0.5, smoothstep(0.4, 2.0, fp));
        let crest = smoothstep(frame.sea.w, frame.sea.w + 0.5, (waves.height / sigma + folding) * BREAKING + (lace - 0.5) * 1.2);
        let caps = mix(crest, frame.sea_rough.w * 0.8, smoothstep(2.0, 12.0, fp)) * smoothstep(0.0, 0.6, gust) * (1.0 - smoothstep(4000.0, 14000.0, dist) * 0.5);
        col = mix(col, foam_color(light), caps);
        cover = mix(cover, 1.0, caps);
    }

    // The haze, over what the surface covers: what it lets through was hazed under it.
    let haze = haze_at(p);
    return vec4<f32>(col * haze.through + haze.light * cover, cover);
}

// ---------------------------------------------------------------- cloud layer, drawn as stacked slices

struct CloudOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) level: f32,
    // Height between this slice and the next, metres.
    @location(2) spacing: f32,
};

// chunk.offset.y = height of the slice relative to the camera, chunk.offset.w = its level in the layer, 0..1.
@vertex
fn cloud_vertex(@builtin(vertex_index) vid: u32) -> CloudOut {
    var corners = CORNERS;
    let c = corners[vid];
    let rel = vec3<f32>(c.x * 120000.0, chunk.offset.y, c.y * 120000.0);
    var o: CloudOut;
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.level = chunk.offset.w;
    o.spacing = chunk.offset.x;
    return o;
}

@fragment
fn cloud_fragment(in: CloudOut) -> @location(0) vec4<f32> {
    let dist = length(in.rel);
    if dist > 100000.0 {
        discard;
    }
    // A slice stands for the slab of air around it: each pixel looks at the cloud somewhere along its
    // ray through the slab, a different place from its neighbours, so the slices seen edge-on do not
    // show as stripes.
    let ray = in.rel / dist;
    let grain = ihash(vec2<i32>(in.position.xy) + vec2<i32>(i32(in.level * 9770.0), i32(in.level * 19310.0))) - 0.5;
    let along = grain * in.spacing / max(abs(ray.y), 0.03);
    let xz = in.rel.xz + ray.xz * along;
    let n = cloud_noise(cloud_coord(xz), dist);
    let th = mix(0.90, 0.36, frame.misc.w);
    // Cumulus shape: flat base, domed top. A slice shows cloud only where the puff reaches its height.
    let lv = saturate(in.level + grain * in.spacing / max(frame.extra.x, 1.0) * sign(ray.y));

    var need = th;
    if lv < 0.12 {
        need += (0.12 - lv) * 0.5;
    } else {
        need += pow((lv - 0.12) / 0.88, 1.5) * 0.24;
    }
    let density = smoothstep(need - 0.015, need + 0.135, n);
    let alpha = density * CLOUD_OPACITY * (1.0 - smoothstep(40000.0, 100000.0, dist)) * smoothstep(15.0, 220.0, dist) * step(0.01, frame.misc.w);
    if alpha < 0.004 {
        discard;
    }
    let sun = frame.sun_dir.xyz;
    // Bright where the sun reaches: high in the cloud and on its sunward side.
    let toward = normalize(sun.xz + 1e-4) * 260.0;
    let sun_side = n - cloud_noise(cloud_coord(xz + toward), dist);
    let depth = saturate((n - need) / 0.22);
    let lit = saturate(0.40 + 0.60 * lv + sun_side * 3.5 - depth * 0.34 * (1.0 - lv));
    let sky = sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel);
    // The thin edges of a cloud between the eye and the sun shine with the light they let through.
    let lining = pow(max(dot(ray, sun), 0.0), 6.0) * (1.0 - depth) * 0.5;
    // The sun lights a cloud high up for a while after it has set on the ground: both lights, by
    // where each reaches the cloud.
    let light = sun_light_at(in.rel) + moon_light_at(in.rel);
    var col = light * (0.36 * lit + lining) + sky * (0.36 + 0.25 * lv);

    col = apply_fog(col, in.rel);
    return vec4<f32>(col, alpha);
}

// ---------------------------------------------------------------- objects (vehicles, poles, lights)

struct ObjectIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,   // a = gloss, 2 more on a metal
};

struct ObjectOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) alpha: f32,
    @location(4) local: vec3<f32>,   // in mesh space, for the cuts
};

@vertex
fn object_vertex(in: ObjectIn) -> ObjectOut {
    var o: ObjectOut;
    let rel = object.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.normal = (object.model * vec4<f32>(in.normal, 0.0)).xyz;
    o.color = vec4<f32>(in.color.rgb * object.tint.rgb, in.color.a);
    o.alpha = object.tint.a;
    o.local = in.position;
    return o;
}

// Value noise in three dimensions, 0..1.
fn cut_corner(i: vec3<i32>) -> f32 {
    return ihash(vec2<i32>(i.x + i.z * 7919, i.y - i.z * 3571));
}

fn cut_noise(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    var f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    let x0 = mix(mix(cut_corner(i), cut_corner(i + vec3<i32>(1, 0, 0)), f.x), mix(cut_corner(i + vec3<i32>(0, 1, 0)), cut_corner(i + vec3<i32>(1, 1, 0)), f.x), f.y);
    let x1 = mix(mix(cut_corner(i + vec3<i32>(0, 0, 1)), cut_corner(i + vec3<i32>(1, 0, 1)), f.x), mix(cut_corner(i + vec3<i32>(0, 1, 1)), cut_corner(i + vec3<i32>(1, 1, 1)), f.x), f.y);
    return mix(x0, x1, f.z);
}

// A line broken at every `1 / scale` metres along `x`, each corner at a height of its own, -1..1:
// straight runs between sharp corners, as sheet metal parts between its rivets.
fn torn_line(x: f32, scale: f32, salt: i32) -> f32 {
    let at = x * scale;
    let i = i32(floor(at));
    return mix(ihash(vec2<i32>(i, salt)), ihash(vec2<i32>(i + 1, salt)), fract(at)) * 2.0 - 1.0;
}

// How far the torn edge of a skin lies from the flat plane of its cut (unit normal `n`) at a point
// `p` of the mesh, m. Across the plane's two directions: a broken line of long runs (the skin
// parting from rib to rib), a shorter one the other way (the upper and the lower skin do not part
// at the same place), and along the edge small teeth, each as deep as its own number says.
fn torn_edge(p: vec3<f32>, n: vec3<f32>) -> f32 {
    // Two directions in the plane: the first across the axis the normal is least along.
    let m = abs(n);
    var least = vec3<f32>(0.0, 0.0, 1.0);
    if m.x <= m.y && m.x <= m.z {
        least = vec3<f32>(1.0, 0.0, 0.0);
    } else if m.y <= m.z {
        least = vec3<f32>(0.0, 1.0, 0.0);
    }
    let first = normalize(cross(n, least));
    let second = cross(n, first);
    let u = dot(p, first);
    let v = dot(p, second);
    let runs = torn_line(u, 4.5, 17) * 0.085 + torn_line(v, 8.0, 43) * 0.045;
    let tooth = (u + v) * 27.0;
    let teeth = abs(fract(tooth) - 0.5) * 2.0 * ihash(vec2<i32>(i32(floor(tooth)), 71)) * 0.024;
    return runs + teeth;
}

// How far a point of an object (mesh space) lies past the cuts of its draw, m: above 0 it is not
// drawn; -1000 for a draw that is not cut. The edge is torn (`torn_edge`), the same on both sides of
// a cut, so that the piece that came off and the stub left fit.
fn cut_past(p: vec3<f32>) -> f32 {
    var past = -1000.0;
    for (var i = 0; i < 2; i++) {
        let side = object.keep[i];
        if side == 0.0 {
            continue;
        }
        let within = object.cuts[2 * i + 1];
        if dot(within.xyz, p) <= within.w {
            continue;
        }
        let plane = object.cuts[2 * i];
        past = max(past, side * (dot(plane.xyz, p) - plane.w + torn_edge(p, plane.xyz)));
    }
    return past;
}

// An object at a fragment, as `object_fragment` writes it.
fn object_shade(in: ObjectOut) -> vec4<f32> {
    let pixel_x = dpdx(in.rel);
    let pixel_y = dpdy(in.rel);
    let past = cut_past(in.local);
    if past > 0.0 {
        discard;
    }
    var n = normalize(in.normal);
    let sun = frame.sun_dir.xyz;
    // Lights shine by themselves.
    if in.alpha > 1.5 {
        return vec4<f32>(mix(in.color.rgb, apply_fog(in.color.rgb, in.rel), 0.6), 1.0);
    }
    let v = normalize(-in.rel);
    var base = in.color.rgb;
    var gloss = in.color.a;
    // A metal's gloss comes 2 higher (`engine::gltf`): it has no colour of its own under a clear
    // coat, it mirrors the light in its colour.
    var metal = gloss > 1.5;
    if metal {
        gloss -= 2.0;
    }
    if past > -100.0 {
        // Through a tear the inside of the skin shows, in its primer; along the edge, bare crumpled metal.
        if dot(n, v) < 0.0 {
            base = vec3<f32>(0.30, 0.36, 0.24) * (0.35 + 0.25 * cut_noise(in.local * 9.0));
            gloss = 0.0;
            metal = false;
        }
        let rip = smoothstep(-0.05, -0.004, past);
        base = mix(base, vec3<f32>(0.36, 0.37, 0.39) * (0.5 + 0.8 * cut_noise(in.local * 40.0)), rip);
        gloss = mix(gloss, 0.8, rip);
    }
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let ndl = max(dot(n, sun), 0.0);
    // The vehicles' shadows on it (on an aircraft, its wing's on its fuselage).
    let sunlight = light_at(in.rel) * vehicle_shadow(in.rel, n);
    let h = normalize(sun + v);
    let sharp = exp2(4.0 + 7.0 * gloss);
    var col: vec3<f32>;
    if metal {
        // Schlick's with the metal's colour head on, white at grazing; a rough metal scatters a
        // little of the light in its colour.
        let edge = base + (1.0 - base) * pow(1.0 - max(dot(v, h), 0.0), 5.0);
        col = base * (1.0 - gloss) * 0.3 * (sunlight * ndl + sky_light(n, in.rel));
        col += sunlight * pow(max(dot(n, h), 0.0), sharp) * (sharp + 8.0) / 25.13 * edge * ndl;
        let seen = base + (1.0 - base) * pow(1.0 - max(dot(n, v), 0.0), 5.0);
        col += sky_seen(reflect(-v, n)) * seen * (0.25 + 0.75 * gloss);
    } else {
        col = base * (sunlight * ndl + sky_light(n, in.rel));
        if frame.fire_count.x > 0.0 {
            col += base * fire_light(in.rel, n);
        }
        // A clear coat over the colour, as on paint: the sun's highlight as sharp as the surface is
        // glossy (Blinn and Phong's, normalised: (s + 8) / 8π), its strength a varnish's (Schlick's,
        // 4 % head on), and the sky mirrored, more and more towards grazing.
        let coat = 0.04 + 0.96 * pow(1.0 - max(dot(v, h), 0.0), 5.0);
        col += sunlight * pow(max(dot(n, h), 0.0), sharp) * (sharp + 8.0) / 25.13 * coat * gloss * ndl;
        let fres = (0.04 + 0.96 * pow(1.0 - max(dot(n, v), 0.0), 5.0)) * gloss;
        col = mix(col, sky_seen(reflect(-v, n)), fres * 0.5);
    }
    // What is afloat or sunk is seen through the water over it, and the sea's surface is drawn
    // over that (`under_sea_color`); a see-through draw keeps its own alpha.
    if under_sea(in.rel) {
        let under = under_sea_color(col, in.rel, pixel_x, pixel_y, false);
        return vec4<f32>(under.rgb, select(under.a, in.alpha, in.alpha < 1.0));
    }
    col = apply_fog(col, in.rel);
    return vec4<f32>(col, in.alpha);
}

@fragment
fn object_fragment(in: ObjectOut) -> @location(0) vec4<f32> {
    return object_shade(in);
}

// An object mirrored in the sea (`reflection.rs`); what is afloat shows only above it.
@fragment
fn object_mirror_fragment(in: ObjectOut) -> @location(0) vec4<f32> {
    if mirror_hidden(in.rel) {
        discard;
    }
    return vec4<f32>(object_shade(in).rgb, 1.0);
}

// The vehicles drawn into their shadow map (`casters.rs`): `object.model` takes the mesh to the
// tile's clip space; what the draw's cuts leave out casts nothing.
struct CasterOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec3<f32>,
};

@vertex
fn caster_vertex(in: ObjectIn) -> CasterOut {
    var o: CasterOut;
    o.position = object.model * vec4<f32>(in.position, 1.0);
    o.local = in.position;
    return o;
}

@fragment
fn caster_fragment(in: CasterOut) {
    if cut_past(in.local) > 0.0 {
        discard;
    }
}

// ---------------------------------------------------------------- trees

struct TreeIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) place: vec4<f32>,   // xyz in the tile, w = size
    @location(4) tint: vec4<f32>,    // rgb = tint; w = width of a tree, or heading of a house in radians
};

