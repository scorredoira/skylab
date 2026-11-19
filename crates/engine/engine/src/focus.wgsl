// A lens's depth of field (`Post::focus`): what is nearer or farther than what the lens is focused
// on is blurred, the more the farther from it, before the bloom and the tone mapping (post.rs). One
// pass gathers, round each pixel, what would spread over it.

// `Params` of post.wgsl: the same fields in the same order.
struct Params {
    look: vec4<f32>,
    more: vec4<f32>,
    grade: vec4<f32>,
    lens: vec4<f32>,
    flare: vec4<f32>,
    // x = the distance in focus, m, y = how far either side of it stays sharp, m, z = the widest
    // blur's radius in pixels, w = the camera's near distance (the depth is near / distance).
    focus: vec4<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var depth: DEPTH_TEXTURE;
@group(0) @binding(3) var linear_clamp: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn fullscreen(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Out(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

// How far what a pixel shows is from the camera, m.
fn distance_at(p: vec2<i32>) -> f32 {
    return params.focus.w / max(textureLoad(depth, p, 0), 1e-7);
}

// The radius, in pixels, of the circle a point `far` metres away is blurred into: nothing within
// the sharp span, then growing with how far off it is for its distance, as with a lens.
fn circle(far: f32) -> f32 {
    let off = max(abs(far - params.focus.x) - params.focus.y, 0.0);
    return min(off / max(min(far, params.focus.x), 0.1) * 1.6, 1.0) * params.focus.z;
}

fn turn_of(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

const TAPS: i32 = 36;

@fragment fn focus(in: Out) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    let here = distance_at(vec2<i32>(in.position.xy));
    let own = circle(here);
    // Each tap counts the less the brighter it is, so that one glint among them does not show as a
    // speck: the average is taken as if after a tone mapping, and undone.
    let centre = textureLoad(source, vec2<i32>(in.position.xy), 0).rgb;
    var weight = 1.0 / (1.0 + max(centre.r, max(centre.g, centre.b)));
    var sum = centre * weight;
    // Points on a spiral over the widest circle, turned at each pixel so that their pattern does not show.
    let turn = turn_of(in.position.xy) * 6.2831853;
    for (var i = 0; i < TAPS; i += 1) {
        let r = sqrt((f32(i) + 0.5) / f32(TAPS)) * params.focus.z;
        let a = f32(i) * 2.3999632 + turn;
        let at = clamp(in.position.xy + vec2<f32>(cos(a), sin(a)) * r, vec2<f32>(0.5), size - 0.5);
        let there = distance_at(vec2<i32>(at));
        var spread = circle(there);
        // What is behind does not spread over what is sharper in front of it.
        if there > here {
            spread = min(spread, own * 2.0);
        }
        let c = textureSampleLevel(source, linear_clamp, at / size, 0.0).rgb;
        let w = saturate(spread - r + 1.0) / (1.0 + max(c.r, max(c.g, c.b)));
        sum += c * w;
        weight += w;
    }
    return vec4<f32>(sum / weight, 1.0);
}
