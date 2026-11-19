// From the scene in linear light to the picture: bloom, exposure, AgX tone mapping (post.rs).

struct Params {
    // Exposure in stops of a scene whose average is mid grey, how much of the difference from mid
    // grey the exposure makes up for (0...1), the part of the light spread into the glow, and how
    // far towards the measured exposure this frame moves when the scene got darker.
    look: vec4<f32>,
    // The same step when the scene got brighter, 1 if the target is sRGB (it encodes what is
    // written) or 0 if what is written must be encoded here, how much the exposure stops down for a
    // glare (`Post::glare`), the levels of the bloom chain.
    more: vec4<f32>,
    // The contrast (0 plain AgX) and the saturation applied to the tone-mapped picture, and how
    // dark the corners are (`Post::vignette`).
    grade: vec4<f32>,
    // The blur of speed: how far each point is smeared towards yz, the centre (`Post::blur`), and
    // how much of it what is above the centre gets (`Post::blur_above`).
    lens: vec4<f32>,
    // The lens flare: xy = where the sun is in the picture, z = its strength (`Post::flare`); w = how
    // much the exposure opens for the middle of the picture (`Post::spot`).
    flare: vec4<f32>,
    // The depth of field (`Post::focus`), read by focus.wgsl.
    focus: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var exposure: texture_2d<f32>;
@group(0) @binding(4) var linear_clamp: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle over the whole target.
@vertex fn fullscreen(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Out(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The bloom is a chain of ever smaller pictures of the scene: `down` makes each from the one before
// and `up` adds each, enlarged, to the one before it, so that the largest holds the light spread
// over every width the chain has (`finish` divides it by the levels).

// Above this luminance a patch counts ever less: a single pixel a thousand times brighter than the
// rest (a glint, or a NaN, which comes out black) must not flood the picture with its glow.
const GLOW_KNEE: f32 = 500.0;

// The mean of the four texels round a point `x`, `y` texels from `at`, as much of it as may spread.
fn glow(at: vec2<f32>, texel: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    let c = max(textureSampleLevel(source, linear_clamp, at + texel * vec2<f32>(x, y), 0.0).rgb, vec3<f32>(0.0));
    return c * (GLOW_KNEE / (GLOW_KNEE + luminance(c)));
}

// Half the size of `source`, through the filter of Jimenez 2014 ("Next generation post processing
// in Call of Duty: Advanced Warfare"): thirteen bilinear taps that make five overlapping boxes of
// 4 x 4 texels, the one in the middle counting a half and the four round it an eighth each. It
// lets through nothing fine enough to flicker when a small light moves from texel to texel.
@fragment fn down(in: Out) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let inner = glow(in.uv, t, -1.0, -1.0) + glow(in.uv, t, 1.0, -1.0) + glow(in.uv, t, -1.0, 1.0) + glow(in.uv, t, 1.0, 1.0);
    let corners = glow(in.uv, t, -2.0, -2.0) + glow(in.uv, t, 2.0, -2.0) + glow(in.uv, t, -2.0, 2.0) + glow(in.uv, t, 2.0, 2.0);
    let sides = glow(in.uv, t, -2.0, 0.0) + glow(in.uv, t, 2.0, 0.0) + glow(in.uv, t, 0.0, -2.0) + glow(in.uv, t, 0.0, 2.0);
    return vec4<f32>((inner + glow(in.uv, t, 0.0, 0.0)) * 0.125 + sides * 0.0625 + corners * 0.03125, 1.0);
}

fn spread(at: vec2<f32>, texel: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    return textureSampleLevel(source, linear_clamp, at + texel * vec2<f32>(x, y), 0.0).rgb;
}

// `source` (the next smaller level) at twice its size, blended by adding: the upsampling of the
// dual filter (Bjorge 2015, "Bandwidth-efficient rendering"), eight bilinear taps on a diamond,
// the four nearer ones counting double. Each texel becomes a round, smooth spot, with no square showing.
@fragment fn up(in: Out) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(source));
    let near = spread(in.uv, t, -0.5, -0.5) + spread(in.uv, t, 0.5, -0.5) + spread(in.uv, t, -0.5, 0.5) + spread(in.uv, t, 0.5, 0.5);
    let far = spread(in.uv, t, -1.0, 0.0) + spread(in.uv, t, 1.0, 0.0) + spread(in.uv, t, 0.0, -1.0) + spread(in.uv, t, 0.0, 1.0);
    return vec4<f32>((near * 2.0 + far) / 12.0, 1.0);
}

// The exposure, in stops, into a 1 x 1 target: the average brightness of the scene (the mean of its
// logarithm, over a grid on the smallest level of the bloom chain, the middle counting more than the
// edges); opened for the middle of the picture (`SPOT_TIGHT`: a soft spot a third of it wide) by
// `Post::spot` of the stops that exposure would leave it more than SPOT_BELOW under mid grey, up to
// SPOT_STOPS; kept from leaving its brightest patch more than GLARE_STOPS above mid grey by
// `Post::glare` (a patch counts by its weight: with a spot, by how near the middle it is); and a
// step from the last frame's exposure (in `exposure`) towards the one it asks for.
const GRID: vec2<i32> = vec2<i32>(16, 10);
const GLARE_STOPS: f32 = 4.0;
const SPOT_TIGHT: f32 = 5.0;
const SPOT_BELOW: f32 = 1.5;
const SPOT_STOPS: f32 = 3.0;
@fragment fn meter(in: Out) -> @location(0) vec4<f32> {
    var sum = 0.0;
    var weights = 0.0;
    var spot_sum = 0.0;
    var spot_weights = 0.0;
    var brightest = -20.0;
    let spot = params.flare.w;
    for (var y = 0; y < GRID.y; y += 1) {
        for (var x = 0; x < GRID.x; x += 1) {
            let uv = (vec2<f32>(f32(x), f32(y)) + 0.5) / vec2<f32>(GRID);
            let d = (uv - 0.5) * 2.0;
            let w = 1.0 - 0.6 * min(dot(d, d), 1.0);
            let ws = exp(-SPOT_TIGHT * dot(d, d));
            let l = log2(max(luminance(textureSampleLevel(source, linear_clamp, uv, 0.0).rgb), 1e-5));
            sum += w * l;
            weights += w;
            spot_sum += ws * l;
            spot_weights += ws;
            brightest = max(brightest, l + log2(mix(w, ws, spot)));
        }
    }
    var wanted = params.look.x + params.look.y * (log2(0.18) - sum / weights);
    wanted += spot * clamp(log2(0.18) - SPOT_BELOW - (spot_sum / spot_weights + wanted), 0.0, SPOT_STOPS);
    let glare = params.look.x + log2(0.18) + GLARE_STOPS - brightest;
    if glare < wanted {
        wanted = mix(wanted, glare, params.more.z);
    }
    wanted = clamp(wanted, -12.0, 12.0);
    let last = textureLoad(exposure, vec2<i32>(0), 0).r;
    // Opening up (the scene got darker) takes longer than closing down, as with the eye.
    let step = select(params.more.x, params.look.w, wanted > last);
    return vec4<f32>(mix(last, wanted, step), 0.0, 0.0, 1.0);
}

// AgX (Troy Sobotka's picture formation): the light goes into primaries drawn in towards white,
// each channel through the logarithm of its stops and an S-shaped curve, and back out. A bright
// colour then pales towards white as on film instead of turning to another hue, and the shadows
// fall off slowly. The result is encoded for the display.

// The primaries drawn in and turned (AgX's inset, for Rec. 709), and the way back (its inverse).
const AGX_IN = mat3x3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051,
                           0.0784335999999992, 0.878468636469772, 0.0784336,
                           0.0792237451477643, 0.0791661274605434, 0.879142973793104);
const AGX_OUT = mat3x3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438,
                            -0.0980208811401368, 1.15190312990417, -0.0980434501171241,
                            -0.0990297440797205, -0.0989611768448433, 1.15107367264116);
// The stops the curve spans below and above mid grey, and log2 of mid grey (0.18).
const AGX_BELOW: f32 = 10.0;
const AGX_ABOVE: f32 = 6.5;
const AGX_MID: f32 = -2.473931;
// The curve, over the span as 0...1: through (AGX_PIVOT, 0.5) with the slope AGX_SLOPE, and from
// there bending to 0 and to 1 as t / (1 + t^3)^(1/3) bends to 1. AGX_TOE and AGX_SHOULDER are the
// sizes of its two halves for which they are 0 and 1 exactly at the ends of the span:
// m x / ((m x / y)^3 - 1)^(1/3), for the slope m and the half's width x and height y.
const AGX_PIVOT: f32 = 0.6060606;
const AGX_SLOPE: f32 = 2.0;
const AGX_TOE: f32 = 0.5122774;
const AGX_SHOULDER: f32 = 0.5516935;

fn agx_curve(x: vec3<f32>) -> vec3<f32> {
    let above = x >= vec3<f32>(AGX_PIVOT);
    let size = select(vec3<f32>(AGX_TOE), vec3<f32>(AGX_SHOULDER), above);
    let t = AGX_SLOPE * abs(x - AGX_PIVOT) / size;
    let bent = size * t / pow(1.0 + t * t * t, vec3<f32>(1.0 / 3.0));
    return 0.5 + select(-bent, bent, above);
}

fn agx(x: vec3<f32>) -> vec3<f32> {
    // The contrast narrows the span round mid grey, which stays at the pivot.
    let firm = 1.0 + params.grade.x;
    let stops = log2(max(AGX_IN * max(x, vec3<f32>(0.0)), vec3<f32>(1e-10))) - AGX_MID;
    var v = agx_curve(saturate((stops * firm + AGX_BELOW) / (AGX_BELOW + AGX_ABOVE)));
    // The saturation: each channel away from the luminance of the three, or towards it.
    v = mix(vec3<f32>(luminance(v)), v, params.grade.y);
    return saturate(AGX_OUT * v);
}

// Interleaved gradient noise: a different fraction at each pixel, in a pattern the eye does not pick out.
fn dither(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// The scene at a pixel, smeared along the line to the blur's centre (`Post::blur`): eight taps, the
// first on the pixel, dithered along the line so that they do not show as steps.
fn smeared(pixel: vec2<f32>, uv: vec2<f32>) -> vec3<f32> {
    let to = params.lens.yz - uv;
    // Nothing near the centre, all of it from a third of the way out.
    let reach = params.lens.x * smoothstep(0.08, 0.45, length(to)) * mix(params.lens.w, 1.0, smoothstep(-0.04, 0.12, -to.y));
    if reach <= 0.0005 {
        return textureLoad(source, vec2<i32>(pixel), 0).rgb;
    }
    let jitter = dither(pixel);
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < 8; i += 1) {
        let t = (f32(i) + jitter) / 8.0;
        sum += textureSampleLevel(source, linear_clamp, uv + to * reach * t, 0.0).rgb;
    }
    return sum / 8.0;
}

// The flare of the lens at `uv` for a sun at `sun` (both 0...1): a few faint ghosts along the line
// through the middle of the picture, each a soft disc or ring of its own colour, and a short streak.
fn lens_flare(uv: vec2<f32>, sun: vec2<f32>, aspect: f32) -> vec3<f32> {
    let axis = vec2<f32>(0.5) - sun;
    var sum = vec3<f32>(0.0);
    let places = array<f32, 5>(0.35, 0.62, 1.0, 1.45, 1.9);
    let sizes = array<f32, 5>(0.035, 0.06, 0.02, 0.10, 0.17);
    let tints = array<vec3<f32>, 5>(vec3<f32>(1.0, 0.6, 0.3), vec3<f32>(0.4, 0.8, 0.5), vec3<f32>(1.0, 0.9, 0.7), vec3<f32>(0.4, 0.5, 1.0), vec3<f32>(0.9, 0.5, 0.8));
    for (var i = 0; i < 5; i += 1) {
        let d = (uv - (sun + axis * places[i])) * vec2<f32>(aspect, 1.0);
        let r = length(d) / sizes[i];
        // A disc with a brighter edge.
        sum += tints[i] * (1.0 - smoothstep(0.85, 1.0, r)) * (0.35 + 0.65 * r * r) * 0.4;
    }
    // A glow round the sun itself, and a faint streak across the picture through it.
    let d = (uv - sun) * vec2<f32>(aspect, 1.0);
    sum += vec3<f32>(1.0, 0.85, 0.6) * (0.04 / (dot(d, d) * 30.0 + 0.04));
    sum += vec3<f32>(0.5, 0.65, 1.0) * exp(-abs(d.y) * 160.0) * exp(-abs(d.x) * 2.2) * 0.5;
    return sum;
}

// The picture: the scene with its glow, exposed and tone-mapped, dithered against banding.
@fragment fn finish(in: Out) -> @location(0) vec4<f32> {
    var c: vec3<f32>;
    if params.lens.x > 0.0 {
        c = smeared(in.position.xy, in.uv);
    } else {
        c = textureLoad(source, vec2<i32>(in.position.xy), 0).rgb;
    }
    if params.look.z > 0.0 {
        // Each level of the chain has added the whole light once.
        c = mix(c, textureSampleLevel(bloom, linear_clamp, in.uv, 0.0).rgb / params.more.w, params.look.z);
    }
    let exposed = exp2(textureLoad(exposure, vec2<i32>(0), 0).r);
    if params.flare.z > 0.0 {
        // As bright as the sun shows through what is in front of it: its glow where it is, exposed.
        let size = vec2<f32>(textureDimensions(source));
        let sun = params.flare.xy;
        let seen = luminance(textureSampleLevel(bloom, linear_clamp, clamp(sun, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb) / params.more.w * exposed;
        let inside = 1.0 - smoothstep(0.0, 0.25, max(max(-sun.x, sun.x - 1.0), max(-sun.y, sun.y - 1.0)));
        let shown = smoothstep(1.5, 12.0, seen) * inside * params.flare.z;
        if shown > 0.0 {
            c += lens_flare(in.uv, sun, size.x / size.y) * shown * 0.12 / exposed;
        }
    }
    if params.grade.w != 0.0 {
        // The white balance (`Post::warmth`): towards amber or blue, the luminance kept.
        let tint = vec3<f32>(1.0 + 0.30 * params.grade.w, 1.0 + 0.04 * params.grade.w, 1.0 - 0.38 * params.grade.w);
        c *= tint / luminance(tint);
    }
    c = agx(c * exposed);
    if params.grade.z > 0.0 {
        // The corners darker, as through a lens: by the square of the distance from the middle.
        let d = (in.uv - 0.5) * 2.0;
        c *= 1.0 - params.grade.z * smoothstep(0.35, 1.9, dot(d, d));
    }
    c += (dither(in.position.xy) - 0.5) / 255.0;
    // AgX's display encoding is a 2.2 power: undone for an sRGB target, which encodes what it is given.
    return vec4<f32>(select(c, pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2)), params.more.y > 0.5), 1.0);
}
