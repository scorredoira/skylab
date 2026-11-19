// Ported from takram-design-engineering/three-geospatial (packages/clouds/src/shaders/shadow.frag,
// shadowResolve.frag, varianceClipping.glsl), MIT, see THIRD_PARTY_NOTICES.md.
//
// The clouds' shadow map (a "beer shadow map"): the clouds marched from the sun, each texel what
// is needed for the optical depth from any point under it towards the sun (how far in the first
// cloud is, its mean extinction, the optical depth of the whole column and of its tail), averaged
// over the frames. And the shade on the ground the world's shaders read, drawn from it.
//
// What is ours and not theirs: one map on the flat world, laid over the level of the top of the
// layers that shade and seen along the sun's rays, instead of cascades fitted to the view; a
// plain march with a jitter instead of the structured sampling; the last pass; and the way it is
// marched, which is the light's but never lower than a few degrees (`clouds.shade`): a sun at the
// horizon would cast every cloud's shade out of the map, and the clouds and the air under them
// would lose their shade just when it shows most.
// Appended to the first part of `clouds.wgsl` (up to its march).

@group(0) @binding(0) var<uniform> clouds: Clouds;
@group(0) @binding(1) var cloud_weather: texture_2d<f32>;
@group(0) @binding(2) var cloud_shape: texture_3d<f32>;
@group(0) @binding(3) var cloud_detail: texture_3d<f32>;
@group(0) @binding(4) var cloud_turbulence: texture_2d<f32>;
@group(0) @binding(5) var cloud_repeat: sampler;
@group(0) @binding(6) var cloud_blue: texture_2d<f32>;
// The map marched this frame, and its average over the frames before.
@group(0) @binding(7) var shadow_now: texture_2d<f32>;
@group(0) @binding(8) var shadow_history: texture_2d<f32>;
@group(0) @binding(9) var shadow_sampler: sampler;

// The quality's (`clouds::source` writes them; these are their `high`).
const SHADOW_STEPS: i32 = 50;
const SHADOW_MIN_STEP: f32 = 100.0;
const SHADOW_MAX_STEP: f32 = 1000.0;
const SHADOW_MIN_DENSITY: f32 = 0.00001;
const SHADOW_MIN_EXTINCTION: f32 = 0.00001;
const SHADOW_MIN_TRANSMITTANCE: f32 = 0.0001;
const SHADOW_TAIL_SCALE: f32 = 2.0;
const SHADOW_MIP: f32 = 0.5;
const SHADOW_VARIANCE_GAMMA: f32 = 1.0;
const SHADOW_TEMPORAL_ALPHA: f32 = 0.01;

@vertex
fn shadow_vertex(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(select(-1.0, 3.0, vid == 1u), select(-1.0, 3.0, vid == 2u), 0.0, 1.0);
}

// r = how far from the top the first cloud is along the ray, g = its mean extinction, b = the
// optical depth as far as the march went, a = that of the tail it left out.
@fragment
fn shadow_march(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    cloud_layers = clouds.shadow_mask;
    let sun = clouds.shade.xyz;
    let size = vec2<f32>(clouds.bsm.w);
    let place = clouds.bsm.xy + (at.xy / size * 2.0 - 1.0) * clouds.bsm.z;
    let origin = vec3<f32>(place.x, clouds.heights.z, place.y);
    let most = (clouds.heights.z - clouds.heights.w) / sun.y;
    if clouds.shade.w <= 0.0 {
        // The light has set: there is no shade to find.
        return vec4<f32>(most, 0.0, 0.0, 0.0);
    }
    let step = clamp(most / f32(SHADOW_STEPS), SHADOW_MIN_STEP, SHADOW_MAX_STEP);
    let jitter = fract(textureLoad(cloud_blue, vec2<i32>(at.xy) % vec2<i32>(textureDimensions(cloud_blue)), 0).r + clouds.more.y * 0.61803399);
    var t = step * jitter;

    var extinctions = 0.0;
    var depth = 0.0;
    var tail = 0.0;
    var through = 1.0;
    var weighted = 0.0;
    var weights = 0.0;
    var samples = 0;
    for (var i = 0; i < SHADOW_STEPS; i++) {
        if t > most {
            break;
        }
        let p = origin - sun * t;
        if cloud_in_gap(p.y) {
            t += step;
            continue;
        }
        let weather = cloud_weather_at(p.xz, p.y, SHADOW_MIP);
        if any(weather.density > vec4<f32>(SHADOW_MIN_DENSITY)) {
            // (This takes the cloud for an even medium.)
            let media = cloud_media(weather, p, SHADOW_MIP, jitter);
            if media.extinction > SHADOW_MIN_EXTINCTION {
                extinctions += media.extinction;
                depth += media.extinction * step;
                through *= exp(-media.extinction * step);
                weighted += t * through;
                weights += through;
                samples += 1;
            }
        }
        if through <= SHADOW_MIN_TRANSMITTANCE {
            // Much optical depth is left in the tail, past where nothing gets through; what to
            // expect of it falls with the samples taken to get there.
            tail = min(SHADOW_TAIL_SCALE * step * exp(f32(1 - samples)), step * 0.5);
            break;
        }
        t += step;
    }
    if samples == 0 {
        return vec4<f32>(most, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(min(weighted / weights, most), extinctions / f32(samples), depth, tail);
}

// After Playdead's temporal reprojection: the history brought into the box of the colours round the texel.
fn clip_aabb(current: vec4<f32>, history: vec4<f32>, low: vec4<f32>, high: vec4<f32>) -> vec4<f32> {
    let p_clip = 0.5 * (high.rgb + low.rgb);
    let e_clip = 0.5 * (high.rgb - low.rgb) + 1e-7;
    let v_clip = history - vec4<f32>(p_clip, current.a);
    let a_unit = abs(v_clip.xyz / e_clip);
    let most = max(a_unit.x, max(a_unit.y, a_unit.z));
    if most > 1.0 {
        return vec4<f32>(p_clip, current.a) + v_clip / most;
    }
    return history;
}

// The map averaged over the frames: the history where this texel was, kept within the mean and
// deviation of the nine texels round it now (Salvi's variance clipping), and a hundredth of the new.
@fragment
fn shadow_resolve(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(shadow_now));
    let coord = vec2<i32>(at.xy);
    let current = textureLoad(shadow_now, coord, 0);
    let before = at.xy / vec2<f32>(size) + clouds.bsm_before.xy;
    if clouds.bsm_before.z < 0.5 || any(before < vec2<f32>(0.0)) || any(before > vec2<f32>(1.0)) {
        return current;
    }
    var moment1 = vec4<f32>(0.0);
    var moment2 = vec4<f32>(0.0);
    for (var k = 0; k < 9; k++) {
        let c = textureLoad(shadow_now, clamp(coord + vec2<i32>(k % 3 - 1, k / 3 - 1), vec2<i32>(0), size - 1), 0);
        moment1 += c;
        moment2 += c * c;
    }
    let mean = moment1 / 9.0;
    let spread = sqrt(max(moment2 / 9.0 - mean * mean, vec4<f32>(0.0))) * SHADOW_VARIANCE_GAMMA;
    let history = textureSampleLevel(shadow_history, shadow_sampler, before, 0.0);
    let clipped = clip_aabb(mean, history, mean - spread, mean + spread);
    return mix(clipped, current, SHADOW_TEMPORAL_ALPHA);
}

// The optical depth of the clouds from `p` towards the sun, less what is nearer than `offset`.
fn shadow_depth(p: vec3<f32>, offset: f32) -> f32 {
    let sun = clouds.shade.xyz;
    let to_top = (clouds.heights.z - p.y) / sun.y;
    if to_top <= 0.0 {
        return 0.0;
    }
    let uv = (p.xz + sun.xz * to_top - clouds.bsm.xy) / (2.0 * clouds.bsm.z) + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.0;
    }
    let shadow = textureSampleLevel(shadow_history, shadow_sampler, uv, 0.0);
    let rim = 1.0 - smoothstep(0.4, 0.5, max(abs(uv.x - 0.5), abs(uv.y - 0.5)));
    return min(shadow.b + shadow.a, shadow.g * max(0.0, to_top - offset - shadow.r)) * rim * clouds.shade.w;
}

// The shade on the ground, into the second channel of the height map: for each place of it at
// the level `clouds.map.w`, how much of the sun's light the clouds over it take.
@fragment
fn shadow_ground(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let q = clouds.map.xy + (at.xy / 1024.0 * 2.0 - 1.0) * clouds.map.z;
    let depth = shadow_depth(vec3<f32>(q.x, clouds.map.w, q.y), 0.0);
    return vec4<f32>(0.0, 1.0 - exp(-depth), 0.0, 0.0);
}
