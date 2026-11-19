// Ported from takram-design-engineering/three-geospatial (packages/clouds/src/shaders/clouds.frag,
// cloudsResolve.frag, varianceClipping.glsl), MIT, see THIRD_PARTY_NOTICES.md.
//
// The volumetric clouds (`clouds.wgsl`) in the scene: marched between the scene's two passes at a
// pixel of every block of four by four, a different one each frame, as far as what the scene drew;
// brought to the scene's size with the frames before (a temporal upscale); and put over the scene
// in the second pass. Appended to the world's shaders and `clouds.wgsl`; `clouds.rs` makes the
// pipelines.
//
// What is ours and not theirs: the light and the haze of the air come from our sky's tables, what
// the scene has near the eye (the cockpit, the aircraft) is not the march's business but the last
// pass's, the place a texel had in the frame before is found from its distance when the frames are
// put together instead of being kept as a velocity, the shade in the air (the shafts) dims
// our own haze here instead of in a pass of the atmosphere's, what the pass that puts the
// frames together does where the frames before are of no use (`cloud_volume_resolve`), and the
// rain under the clouds (`cloud_rain`).

@group(1) @binding(0) var<uniform> clouds: Clouds;
@group(1) @binding(1) var cloud_weather: texture_2d<f32>;
@group(1) @binding(2) var cloud_shape: texture_3d<f32>;
@group(1) @binding(3) var cloud_detail: texture_3d<f32>;
@group(1) @binding(4) var cloud_turbulence: texture_2d<f32>;
@group(1) @binding(5) var cloud_repeat: sampler;
@group(1) @binding(6) var cloud_blue: texture_2d<f32>;
// `clouds.rs` makes this a texture_depth_2d when the scene has one sample per pixel.
@group(1) @binding(7) var cloud_scene_depth: texture_depth_multisampled_2d;
// This frame's march and how far its clouds are, for the pass that puts the frames together; and
// the picture at the scene's size: of the frames before for that pass, with this one for the pass
// over the scene.
@group(1) @binding(8) var cloud_low: texture_2d<f32>;
@group(1) @binding(9) var cloud_far: texture_2d<f32>;
@group(1) @binding(10) var cloud_history: texture_2d<f32>;
// The shadow map (`clouds_shadow.wgsl`).
@group(1) @binding(11) var cloud_bsm: texture_2d<f32>;
// How far the clouds of the march of the frame before are (`cloud_far` then).
@group(1) @binding(12) var cloud_far_before: texture_2d<f32>;
// How long each pixel of the picture of the frames before has been in it (`CloudPicture::settled`).
@group(1) @binding(13) var cloud_settled: texture_2d<f32>;

// How much of the pixels made of one frame's march alone is the mean of the texels round them.
const CLOUD_ALONE_BLUR: f32 = 0.5;
// A veil: cloud right before an eye that moves (`CloudMarched::near`) that hides neither all nor
// nothing (`cloud_veil`), by so much on average round a pixel, from none of the veil's blur to
// all of it. The march's steps are as long as such a cloud is deep, so every texel finds
// another, and it crosses the picture in a few frames: the frames before cannot average it. It
// is drawn as the blur it is (`cloud_volume_resolve`).
const CLOUD_VEIL: vec2<f32> = vec2<f32>(0.02, 0.2);
// How much of a veil is this frame's, the rest the frames' before.
const CLOUD_VEIL_RATE: f32 = 0.5;

// How much of a veil a near cloud that hides `near` is: 1 for a half, 0 for nothing and for all.
fn cloud_veil(near: f32) -> f32 {
    return 4.0 * near * (1.0 - near);
}

fn cloud_sun_light(altitude: f32) -> vec3<f32> {
    return direct_light_above() * light_through_table(sky_transmittance, sky_sampler, altitude, frame.sun_dir.y);
}

// The sky on a cloud: from above, and a little from the horizon away from the sun.
fn cloud_sky_light() -> vec3<f32> {
    let up = textureSampleLevel(sky_ambient, sky_sampler, ambient_uv(1.0, 1.0, 1.0), 0.0).rgb;
    let away = textureSampleLevel(sky_ambient, sky_sampler, ambient_uv(-1.0, 0.0, 1.0), 0.0).rgb;
    return up * 0.8 + away * 0.2;
}

// The optical depth of the clouds from `p` towards the sun, less what is nearer than `offset`:
// read from the shadow map, where the ray to the sun leaves the top of the shade.
fn cloud_shadow_depth(p: vec3<f32>, offset: f32) -> f32 {
    let sun = clouds.shade.xyz;
    if clouds.bsm.w < 0.5 || clouds.shade.w <= 0.0 {
        return 0.0;
    }
    let to_top = (clouds.heights.z - p.y) / sun.y;
    if to_top <= 0.0 {
        return 0.0;
    }
    let uv = (p.xz + sun.xz * to_top - clouds.bsm.xy) / (2.0 * clouds.bsm.z) + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.0;
    }
    // r = how far in the first cloud is, g = its mean extinction, b + a = the optical depth of the whole column.
    let shadow = textureSampleLevel(cloud_bsm, sky_sampler, uv, 0.0);
    // (Ours: it fades towards the rim of the map, which would otherwise show in the haze far off as a wall.)
    let rim = 1.0 - smoothstep(0.4, 0.5, max(abs(uv.x - 0.5), abs(uv.y - 0.5)));
    return min(shadow.b + shadow.a, shadow.g * max(0.0, to_top - offset - shadow.r)) * rim * clouds.shade.w;
}

@vertex
fn cloud_volume_vertex(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(select(-1.0, 3.0, vid == 1u), select(-1.0, 3.0, vid == 2u), 1.0, 1.0);
}

// A number 0..1 per texel that differs most between neighbours (blue noise, `cloud_noise.rs`),
// moved on every frame by the golden ratio: the numbers of any run of frames are spread evenly
// over 0..1 at every texel.
fn cloud_jitter(p: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(cloud_blue));
    return fract(textureLoad(cloud_blue, vec2<i32>(p) % size, 0).r + clouds.more.y * 0.61803399);
}

// The ray of the view through a place of the picture, 0..1 each way.
fn cloud_ray(uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return normalize(frame.cam_fwd.xyz + ndc.x * frame.cam_right.w * frame.cam_right.xyz + (ndc.y - frame.cam_fwd.w) * frame.cam_up.w * frame.cam_up.xyz);
}

// How far along a ray what the scene drew at a pixel is, m (1e9 for the sky).
fn cloud_scene_reach(px: vec2<i32>, v: vec3<f32>) -> f32 {
    let z = textureLoad(cloud_scene_depth, px, 0);
    return select(1e9, clouds.more.x / max(z, 1e-9) / max(dot(v, frame.cam_fwd.xyz), 0.05), z > 0.0);
}

// What is nearer than this is not the march's business: the pass that puts the clouds over the
// scene hides them behind it, pixel by pixel (the cockpit, the aircraft seen from outside), and
// the march and its history do not see it move.
const CLOUD_NEAR: f32 = 250.0;

struct CloudMarch {
    // The clouds' light towards the eye through the air between, and how much of the scene they hide.
    @location(0) light: vec4<f32>,
    // r = one over how far what shows is (the cloud, or the scene, or nothing; between them by
    // how much the cloud hides), 1/km; g = where the first of the cloud is, b = how much of the ray
    // is in the clouds' shade, km; a = how much of a veil is right before the eye (`cloud_veil`).
    @location(1) far: vec4<f32>,
};

// A texel of the march: one pixel of its block, one sample of it.
@fragment
fn cloud_volume_march(@builtin(position) at: vec4<f32>) -> CloudMarch {
    let full = vec2<i32>(textureDimensions(cloud_scene_depth));
    let px = min(vec2<i32>(at.xy) * i32(clouds.more.z) + vec2<i32>(clouds.jitter.xy), full - 1);
    let v = cloud_ray((vec2<f32>(px) + 0.5) / vec2<f32>(full));
    let scene = cloud_scene_reach(px, v);
    let reach = select(scene, 1e9, scene < CLOUD_NEAR);
    let jitter = cloud_jitter(at.xy);
    var range = cloud_range(v);
    range.y = min(range.y, reach);

    var colour = vec4<f32>(0.0);
    var front = min(reach, 1e6);
    var first = 1e6;
    var near = 0.0;
    var hit = false;
    if range.x >= 0.0 && range.y > range.x {
        // The level of detail the ray starts at: by how much of the weather a pixel takes in where
        // the ray enters the clouds, and none seen from the ground.
        let delta = range.x * 2.0 * frame.cam_up.w / max(abs(v.y), 0.02) / WEATHER_TILE;
        let mip = max(0.0, 0.5 * log2(max(1.0, delta * delta * 0.1))) * min(1.0, 0.2 * clouds.origin.z / clouds.heights.y);
        let m = cloud_march(v, range, jitter, exp2(mip));
        if m.front >= 0.0 {
            hit = true;
            colour = m.colour;
            front = m.front;
            first = m.first;
            near = m.near;
        }
    }
    // (Ours: a thin cloud moves across the picture as what is behind it does, a thick one as
    // itself, and they move by one over the distance. A wisp at the eye that hides a fiftieth is
    // not what the pixel shows, and a fog nearer than a step of the march has no shape to follow.)
    let behind = min(reach, 1e6);

    // How much of the ray lies in the clouds' shade, as far as the scene or the cloud.
    var shaded = 0.0;
    var shade = cloud_shade_range(v, reach);
    if hit {
        shade.y = mix(shade.y, min(front, shade.y), colour.a);
    }
    if CLOUD_SHAFT_STEPS > 0 && clouds.bsm.w > 0.5 && clouds.shade.w > 0.0 && shade.x >= 0.0 && shade.y > shade.x {
        shaded = cloud_shade_length(v, shade, jitter);
    }
    // (Ours, by eye: with a low sun the shafts take more of the air's light away, to show.)
    let dark = mix(0.8, 0.95, cloud_low_sun()) * saturate(shaded / max(shade.y, 1.0));

    // The air between the eye and the cloud, darker by its part in the shade.
    if hit {
        let air = haze_at(v * front);
        colour = vec4<f32>(colour.rgb * air.through + air.light * (1.0 - dark) * colour.a, colour.a);
    }
    // And the air before what is behind the cloud, which the scene has already drawn lit: the
    // shafts are what its shaded part takes away.
    if dark > 0.0 {
        colour = vec4<f32>(colour.rgb - haze_at(v * min(reach, shade.y)).light * dark * (1.0 - colour.a), colour.a);
    }

    // The rain under the clouds, through the air before it: before the clouds for an eye under
    // their base, behind them for one over it.
    var nearness = mix(1.0 / behind, 1.0 / max(front, CLOUD_MIN_STEP), smoothstep(0.25, 0.75, colour.a));
    if clouds.rain.z + clouds.rain.w > 0.0 {
        let fallen = cloud_rain(v, reach, jitter);
        if fallen.colour.a > 0.0 {
            let air = haze_at(v * fallen.front);
            let rain = vec4<f32>(fallen.colour.rgb * air.through + air.light * (1.0 - dark) * fallen.colour.a, fallen.colour.a);
            if clouds.origin.z < clouds.heights.x + RAIN_INTO {
                colour = vec4<f32>(rain.rgb + (1.0 - rain.a) * colour.rgb, rain.a + (1.0 - rain.a) * colour.a);
                // What shows moves across the picture as the rain does, by how much of it there is.
                nearness = mix(nearness, 1.0 / max(fallen.front, CLOUD_MIN_STEP), smoothstep(0.25, 0.75, rain.a));
            } else {
                colour = vec4<f32>(colour.rgb + (1.0 - colour.a) * rain.rgb, colour.a + (1.0 - colour.a) * rain.a);
            }
        }
    }

    // The haze under the clouds, as far as the scene, the ground or the top of the clouds.
    let ground = cloud_level(clouds.origin.z, v.y, 0.0);
    let top = cloud_level(clouds.origin.z, v.y, clouds.heights.y);
    var hazy = min(select(top.y, ground.x, ground.x > 0.0), reach);
    if hit {
        hazy = mix(hazy, min(front, hazy), colour.a);
    }
    if hazy > 0.0 {
        let haze = cloud_haze(v, hazy, shaded);
        colour = vec4<f32>(mix(colour.rgb, haze.rgb, haze.a), colour.a * (1.0 - haze.a) + haze.a);
    }
    return CloudMarch(colour, vec4<f32>(nearness * 1000.0, first * 0.001, shaded * 0.001, cloud_veil(near)));
}

// After Playdead's temporal reprojection: the history brought into the box of the colours round the texel.
fn cloud_clip(current: vec4<f32>, history: vec4<f32>, low: vec4<f32>, high: vec4<f32>) -> vec4<f32> {
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

// Where in the picture of the frame before the point `distance` along the ray through `uv` was:
// xy, and z = 1 if it was in it.
fn cloud_before(uv: vec2<f32>, distance: f32) -> vec3<f32> {
    let was = cloud_ray(uv) * distance + clouds.moved.xyz;
    let ahead = dot(was, clouds.before_fwd.xyz);
    let ndc = vec2<f32>(dot(was, clouds.before_right.xyz) / (ahead * clouds.before_right.w), dot(was, clouds.before_up.xyz) / (ahead * clouds.before_up.w) + clouds.before_fwd.w);
    let before = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    return vec3<f32>(before, select(1.0, 0.0, ahead <= 0.0 || any(before < vec2<f32>(0.0)) || any(before > vec2<f32>(1.0))));
}

// How much of the picture before is not to be believed where its clouds were this many times as
// far as those of now were then: all of it when they were much nearer (what was before the eye has
// gone and shows what was behind it: out of a cloud, round the side of one).
fn cloud_uncovered(ratio: f32) -> f32 {
    return 1.0 - smoothstep(0.15, 0.4, ratio);
}

// The march put together with the frames before, at the scene's size. Every pixel is what the
// picture before had where its cloud was then (found by the nearest of what is round it), kept
// within the mean and deviation of the march's texels round it now (Salvi's variance clipping;
// twice the deviation, which ghosts more and is hard to see on clouds); the pixel the march
// looked through this frame takes what it found into it. When the march is at the scene's size
// (`clouds.more.z` = 1) it is averaged over the frames instead, a tenth of each.
//
// (Ours, for the march at a sixteenth, which theirs leaves as blocks and grain for as long as the
// march takes to come round: where the picture before has nothing to say, the pixel is made of
// this frame's march alone, its texels joined smoothly and half blurred with those round them:
// soft, but without blocks. That is where the place was not in the picture before (the side the
// view turns towards) and where what was there was much nearer than what is there now
// (`cloud_uncovered`: out of a cloud, and there the blur is that of a veil). A pixel so made is
// new (`CloudPicture::settled` = 0), and for the sixteen frames the march takes to look through
// it, it is the mean of what those frames find round it: their grain, another every frame,
// averages away, and what the march finds at the pixel itself counts as the pixel settles. And a
// veil before the eye (`CLOUD_VEIL`) is the mean of the texels six by six round the pixel, half
// of it every frame: a fog without grain, and what is seen through it as soft as through a fog.)
struct CloudPicture {
    @location(0) colour: vec4<f32>,
    // r = how long the pixel has been in the picture: 0 new, 1 from sixteen frames on.
    @location(1) settled: vec4<f32>,
};

@fragment
fn cloud_volume_resolve(@builtin(position) at: vec4<f32>) -> CloudPicture {
    let scale = i32(clouds.more.z);
    let size = vec2<i32>(textureDimensions(cloud_low));
    let coord = vec2<i32>(at.xy);
    let low = min(coord / scale, size - 1);
    let current = textureLoad(cloud_low, low, 0);
    let uv = at.xy / vec2<f32>(textureDimensions(cloud_history));
    // Where the pixel is among the march's texels: each is the pixel of its block it looked through.
    var among = uv;
    if scale > 1 {
        among = (at.xy - clouds.jitter.xy + 0.5 * f32(scale) - 0.5) / vec2<f32>(size * scale);
    }
    var smooth_now = textureSampleLevel(cloud_low, sky_sampler, among, 0.0);
    var moment1 = smooth_now;
    var moment2 = smooth_now * smooth_now;
    if scale == 1 {
        moment1 = current;
        moment2 = current * current;
    }
    // One over how far the nearest of what shows round the pixel is, 1/km.
    var nearness = 0.0;
    for (var k = 0; k < 9; k++) {
        let offset = vec2<i32>(k % 3 - 1, k / 3 - 1);
        if scale == 1 {
            nearness = max(nearness, textureLoad(cloud_far, clamp(low + offset, vec2<i32>(0), size - 1), 0).r);
        }
        if k == 4 {
            continue;
        }
        var c: vec4<f32>;
        if scale > 1 {
            c = textureSampleLevel(cloud_low, sky_sampler, among + vec2<f32>(offset) / vec2<f32>(size), 0.0);
        } else {
            c = textureLoad(cloud_low, clamp(low + offset, vec2<i32>(0), size - 1), 0);
        }
        moment1 += c;
        moment2 += c * c;
    }
    let mean = moment1 / 9.0;
    let deviation = sqrt(max(moment2 / 9.0 - mean * mean, vec4<f32>(0.0)));
    // This frame alone.
    let alone = mix(smooth_now, mean, CLOUD_ALONE_BLUR);
    if clouds.moved.w < 0.5 {
        return CloudPicture(alone, vec4<f32>(0.0));
    }
    // What is round the pixel, between the march's texels: how near it is, and how much of a veil.
    let step = 1.0 / vec2<f32>(size);
    var veil = 0.0;
    if scale > 1 {
        let a = textureSampleLevel(cloud_far, sky_sampler, among + step * vec2<f32>(0.75, 0.75), 0.0).ra;
        let b = textureSampleLevel(cloud_far, sky_sampler, among + step * vec2<f32>(-0.75, 0.75), 0.0).ra;
        let c = textureSampleLevel(cloud_far, sky_sampler, among + step * vec2<f32>(0.75, -0.75), 0.0).ra;
        let d = textureSampleLevel(cloud_far, sky_sampler, among - step * vec2<f32>(0.75, 0.75), 0.0).ra;
        veil = smoothstep(CLOUD_VEIL.x, CLOUD_VEIL.y, 0.25 * (a.y + b.y + c.y + d.y));
        // (Ours: the nearest of them, so that the rim of a cloud goes with the cloud, but taken
        // between the texels: the place a pixel had before goes smoothly from a pixel to the
        // next, where the nearest of whole texels tears the picture into squares when the eye
        // moves fast by a cloud. In a veil, where the nearest is another at every texel, their mean.)
        nearness = mix(max(max(a.x, b.x), max(c.x, d.x)), 0.25 * (a.x + b.x + c.x + d.x), veil);
    }
    let shows = 1000.0 / max(nearness, 1e-6);
    let before = cloud_before(uv, shows);
    if before.z < 0.5 {
        return CloudPicture(alone, vec4<f32>(0.0));
    }
    let history = textureSampleLevel(cloud_history, sky_sampler, before.xy, 0.0);
    if scale == 1 {
        return CloudPicture(mix(cloud_clip(mean, history, mean - deviation, mean + deviation), current, 0.1), vec4<f32>(1.0));
    }
    // How near what was there in the frame before was, against how far what is here now was
    // then. (The farthest of what was round the place, between the texels as above: a texel alone
    // that found a wisp is not a cloud that was there.)
    let half = 0.5 * step;
    let was = min(min(textureSampleLevel(cloud_far_before, sky_sampler, before.xy + half, 0.0).r, textureSampleLevel(cloud_far_before, sky_sampler, before.xy - half, 0.0).r), min(textureSampleLevel(cloud_far_before, sky_sampler, before.xy + vec2<f32>(half.x, -half.y), 0.0).r, textureSampleLevel(cloud_far_before, sky_sampler, before.xy + vec2<f32>(-half.x, half.y), 0.0).r));
    let then = length(cloud_ray(uv) * shows + clouds.moved.xyz) * 0.001;
    let doubt = cloud_uncovered(1.0 / max(was * then, 1e-9));
    let clipped = cloud_clip(mean, history, mean - 2.0 * deviation, mean + 2.0 * deviation);
    // This frame alone, blurred more in a veil and where a cloud has just gone from before the eye.
    let blur = max(veil, doubt);
    var now = alone;
    if blur > 0.0 {
        var wide = moment1;
        for (var k = 0; k < 9; k++) {
            if k != 4 {
                wide += textureSampleLevel(cloud_low, sky_sampler, among + 2.0 * step * vec2<f32>(vec2<i32>(k % 3 - 1, k / 3 - 1)), 0.0);
            }
        }
        now = mix(alone, wide / 17.0, blur);
    }
    let settled = textureSampleLevel(cloud_settled, sky_sampler, before.xy, 0.0).r * (1.0 - doubt);
    // A new pixel is the mean of what the frames since it came have found round it: of the
    // n-th, 1/n (and nothing of the seventeenth).
    let fresh = max(max(doubt, veil * CLOUD_VEIL_RATE), (1.0 / (1.0 + 16.0 * settled) - 1.0 / 17.0) * 17.0 / 16.0);
    let marched = all(coord % scale == vec2<i32>(clouds.jitter.xy));
    // (Ours: theirs takes the new sample as it is, and its grain stays in the picture until the
    // pixel is marched again; a third of it over what the pixel had averages the grain away.)
    return CloudPicture(mix(mix(clipped, now, fresh), current, select(0.0, 0.3 * settled * (1.0 - veil), marched)), vec4<f32>(min(settled + 1.0 / 16.0, 1.0)));
}

// Over the scene: its colour times what the clouds let through, plus their light. Not where the
// scene is near and before the clouds.
@fragment
fn cloud_volume_fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(cloud_scene_depth));
    let uv = at.xy / size;
    let scene = cloud_scene_reach(vec2<i32>(at.xy), cloud_ray(uv));
    if scene < CLOUD_NEAR && scene < textureSampleLevel(cloud_far, sky_sampler, uv, 0.0).g * 1000.0 {
        discard;
    }
    // (Ours: the mean of the pixel and those round it, in four taps between them. Each pixel of
    // the picture was marched in a frame of its own, with a grain of its own.)
    let texel = 0.5 / size;
    let cloud = 0.25 * (textureSampleLevel(cloud_history, sky_sampler, uv + vec2<f32>(texel.x, texel.y), 0.0) + textureSampleLevel(cloud_history, sky_sampler, uv + vec2<f32>(-texel.x, texel.y), 0.0) + textureSampleLevel(cloud_history, sky_sampler, uv + vec2<f32>(texel.x, -texel.y), 0.0) + textureSampleLevel(cloud_history, sky_sampler, uv - vec2<f32>(texel.x, texel.y), 0.0));
    return vec4<f32>(cloud.rgb, 1.0 - saturate(cloud.a));
}
