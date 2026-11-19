// The fire's grid (fire_sim.wgsl) drawn: after shaders.wgsl, the grid's numbers, heat.wgsl and
// fire_common.wgsl, and after the two numbers of the quality that `fire.rs` writes: MARCH_STEP
// (cells a step of the march) and MARCH_MOST (steps at most).
//
// `fire_march` marches the view's rays through the grid into a picture of its own, between the
// scene's two passes; `fire_over` puts that picture over the scene in the second pass. The picture
// may be smaller than the scene (half each way, but for pictures and video): every other of its
// pixels stops at the nearest of the scene's pixels it covers and the rest at the farthest, and
// `fire_over` takes for each of the scene's pixels those marched to about its own distance, so
// smoke neither leaks over what stands in front of it nor leaves a rim round it.
//
// What a place in the smoke sends to the eye: the sun's light that reaches it through the smoke
// (`lighting`'s depth towards the sun) scattered forwards and back, and again as light that has
// been scattered many times (Wrenninge, Kulla and Lundqvist 2013); the sky's from above; the
// fire's own, from a coarse level of its light; and what it gives off by its heat (fire_common.wgsl
// says what of the gas is smoke and what is flame). Each step's light is added as Hillaire (2015)
// does, whole whatever the step's length; a noise frays the gas (`fire_fray`).

// Mirrors `ViewUniforms` in fire.rs.
struct FireView {
    corner: vec4<f32>,  // xyz = the grid's corner relative to the camera, render axes, w = near distance of the view
    place: vec4<f32>,   // xyz = what keeps the noise still in the world as the grid and the wind move, cells, w = time, s
    size: vec4<f32>,    // xy = the size of the march's picture, pixels, z = the scene's pixels to one of its, w = how far below the grid's top the smoke fades out, cells
};

@group(1) @binding(0) var<uniform> fire: FireView;
@group(1) @binding(1) var grid_gas: texture_3d<f32>;
@group(1) @binding(3) var grid_light: texture_3d<f32>;
@group(1) @binding(4) var grid_own: texture_3d<f32>;
@group(1) @binding(5) var grid_noise: texture_3d<f32>;
@group(1) @binding(6) var grid_clamp: sampler;
@group(1) @binding(7) var grid_repeat: sampler;
// `fire.rs` makes this a texture_depth_2d when the scene has one sample per pixel.
@group(1) @binding(8) var grid_depth: texture_depth_multisampled_2d;
@group(1) @binding(9) var grid_marched: texture_2d<f32>;
@group(1) @binding(10) var grid_reached: texture_2d<f32>;

// What soot, dust and unburnt mist send back of the light that falls on them.
const SOOT_COLOUR: vec3<f32> = vec3<f32>(0.12, 0.115, 0.11);
const DUST_COLOUR: vec3<f32> = vec3<f32>(0.36, 0.32, 0.27);
const MIST_COLOUR: vec3<f32> = vec3<f32>(0.6, 0.6, 0.6);
// How far the noise pushes the place the gas is read at, cells, and how many times further its
// widest bulges do.
const WARP: f32 = 3.6;
const BULGE: f32 = 1.1;
// What smoke sends on of the fire's own light, by eye.
const FIRE_ON_SMOKE: f32 = 0.08;

@vertex
fn fire_screen(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(select(-1.0, 3.0, vid == 1u), select(-1.0, 3.0, vid == 2u), 1.0, 1.0);
}

// How far along the ray `rd` the scene's pixel `at` is, m (1e9: the sky).
fn fire_scene_distance(at: vec2<i32>, rd: vec3<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(grid_depth));
    let z = textureLoad(grid_depth, clamp(at, vec2<i32>(0), size - vec2<i32>(1)), 0);
    if z <= 0.0 {
        return 1e9;
    }
    return fire.corner.w / z / max(dot(rd, frame.cam_fwd.xyz), 0.05);
}

// The ray of the view through a place of the picture, 0..1 each way.
fn fire_ray(uv: vec2<f32>) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return normalize(frame.cam_fwd.xyz + ndc.x * frame.cam_right.w * frame.cam_right.xyz + (ndc.y - frame.cam_fwd.w) * frame.cam_up.w * frame.cam_up.xyz);
}

// A number 0..1 of its own for each pixel, with no pattern (a hash of its place, Jarzynski and
// Olano 2020), to start each ray's march at its own depth: what is left of it is grain.
fn fire_dither(at: vec2<f32>) -> f32 {
    var v = vec2<u32>(at) * vec2<u32>(1664525u, 22695477u) + vec2<u32>(1013904223u, 1u);
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    v.x += v.y * 1664525u;
    return f32(v.x >> 8u) / 16777216.0;
}

// Henyey and Greenstein's lobe: how much of the light goes on at the cosine `c` from where it was going.
fn fire_lobe(c: f32, g: f32) -> f32 {
    return (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * c, 1.5));
}

// The noise that frays the gas at a place (cells from the grid's corner): x, y, z = three numbers
// about 0, which push the place read about (a few cells over a few metres, more over the width of
// a fireball), w = lumps and hollows down to a metre across, 0..1. The noise stays where it is in
// the air and drifts up, as the gas does.
fn fire_fray(p: vec3<f32>) -> vec4<f32> {
    let world = (p + fire.place.xyz) * CELL;
    let n = textureSampleLevel(grid_noise, grid_repeat, world / 19.0 + vec3<f32>(0.0, -fire.place.w * 0.08, 0.0), 0.0);
    let lumps = smoothstep(0.28, 0.72, n.r * 0.36 + n.g * 0.3 + n.b * 0.12 + n.a * 0.22);
    // In its first second the fireball is small: pushed about less, it is a ball from the start.
    var push = (n.gba - 0.5) * (0.4 + 0.6 * smoothstep(0.2, 1.2, fire.place.w));
    // And bulges many metres across, which stay where they are as the gas goes through them: the
    // fireball's, in its first seconds. The fire that burns on after it is too small for them.
    let bulging = BULGE * (1.0 - smoothstep(1.2, 4.0, fire.place.w)) * smoothstep(0.4, 1.2, fire.place.w);
    if bulging > 0.01 {
        push += (textureSampleLevel(grid_noise, grid_repeat, world / 57.0 + vec3<f32>(0.37, fire.place.w * 0.013, 0.11), 0.0).rga - 0.5) * bulging;
    }
    return vec4<f32>(push, lumps);
}

struct Marched {
    @location(0) colour: vec4<f32>,
    @location(1) reached: vec4<f32>,
};

@fragment
fn fire_march(@builtin(position) at: vec4<f32>) -> Marched {
    let pixel = vec2<i32>(at.xy);
    let rd = fire_ray(at.xy / fire.size.xy);
    // The scene's pixels under this one: the nearest of them, or the farthest, by turns.
    let scale = i32(fire.size.z);
    var nearest = 1e9;
    var farthest = 0.0;
    for (var j = 0; j < scale; j++) {
        for (var i = 0; i < scale; i++) {
            let d = fire_scene_distance(pixel * scale + vec2<i32>(i, j), rd);
            nearest = min(nearest, d);
            farthest = max(farthest, d);
        }
    }
    let reached = select(farthest, nearest, ((pixel.x + pixel.y) & 1) == 0);
    var out: Marched;
    out.colour = vec4<f32>(0.0);
    out.reached = vec4<f32>(reached, 0.0, 0.0, 0.0);

    // Where the ray is inside the grid.
    let low = fire.corner.xyz;
    let high = low + SIZE * CELL;
    let inv = 1.0 / (rd + vec3<f32>(1e-6) * (step(vec3<f32>(0.0), rd) * 2.0 - 1.0));
    let ta = (low - vec3<f32>(0.0)) * inv;
    let tb = high * inv;
    let t_in = max(max(max(min(ta.x, tb.x), min(ta.y, tb.y)), min(ta.z, tb.z)), 0.0);
    let t_out = min(min(min(max(ta.x, tb.x), max(ta.y, tb.y)), max(ta.z, tb.z)), reached);
    if t_out <= t_in {
        return out;
    }
    var stride = CELL * MARCH_STEP;
    stride = max(stride, (t_out - t_in) / f32(MARCH_MOST));
    var t = t_in + stride * fire_dither(at.xy);

    let sun = frame.sun_dir.xyz;
    let towards = dot(rd, sun);
    // Smoke sends most of the sun's light on, a little back.
    let direct = mix(fire_lobe(towards, 0.45), fire_lobe(towards, -0.2), 0.5) * 4.0 * PI;
    let sky_above = sky_light(vec3<f32>(0.0, 1.0, 0.0), low);
    let sky_below = sky_light(vec3<f32>(0.0, -1.0, 0.0), low);

    let bright = fire_light_scale();
    var light = vec3<f32>(0.0);
    var through = 1.0;
    var seen_at = 0.0;


    var weight = 0.0;
    for (var i = 0; i < MARCH_MOST; i++) {
        if t >= t_out || through < 0.02 {
            break;
        }
        let rel = rd * t;
        var pace = stride;
        let p = (rel - low) / CELL;
        let uvw = p / SIZE;
        // Nothing within the texels of a coarse level of the fire's light (which also keeps how
        // much gas there is, 12 m to a texel): on by half of one.
        if textureSampleLevel(grid_own, grid_clamp, uvw, 3.0).a < 1e-4 {
            t += 6.0;
            continue;
        }
        let lit = textureSampleLevel(grid_light, grid_clamp, uvw, 0.0);
        if lit.w < 0.003 {
            // Nothing here nor next to here.
            t += stride * 3.0;
            continue;
        }
        // The grid's sides and top are not the smoke's: it thins out towards them.
        let edge = min(min(p.x, SIZE.x - p.x), min(p.z, SIZE.z - p.z));
        let fade = saturate(edge / 6.0) * saturate((SIZE.y - p.y) / fire.size.w);
        // The gas is read a little way off, as the noise says: the grid's smooth outlines are
        // pushed into billows and rags finer than its cells.
        let frayed = fire_fray(p);
        var gas = textureSampleLevel(grid_gas, grid_clamp, (p + frayed.xyz * WARP) / SIZE, 0.0);
        let whole = thickness(gas);
        if whole * fade < 0.002 {
            t += stride;
            continue;
        }
        // The noise makes the heat uneven before anything is made of it: where it is low the
        // soot is a cold black crust over the fire, where it is high a tongue of flame, hotter
        // in its middle. Thin smoke is torn into wisps; thick smoke gets lumps and hollows.
        let lumps = frayed.w;
        gas.y *= 0.42 + 0.72 * lumps;
        let heat = gas.y;
        let parts = smoke_parts(gas);
        let dense = parts.x + parts.y + parts.z;
        let thin = 1.0 - saturate(whole * 6.0);
        let dark = dense * saturate(0.25 + 1.4 * lumps - thin * (1.0 - lumps) * 1.3) * fade;
        let flame = flame_part(gas) * (0.5 + lumps) * 1.6 * fade;
        let thick = dark + flame;
        if thick > 1e-4 {
            let colour = (SOOT_COLOUR * parts.x + DUST_COLOUR * parts.y + MIST_COLOUR * parts.z) / max(dense, 1e-5);
            let from_sun = frame.sun_color.rgb * (exp(-lit.x) * direct + 0.5 * exp(-lit.x * 0.22));
            let from_sky = mix(sky_below * 0.35, sky_above, saturate(0.5 + 0.5 * exp(-lit.y * 0.7))) * exp(-lit.y * 0.45);
            var from_fire = vec3<f32>(0.0);
            if dark > 0.004 {
                // The fire's light from as far as the texels of one of its levels are wide, 12 m.
                let own = textureSampleLevel(grid_own, grid_clamp, uvw, 3.0).rgb * 30.0;
                from_fire = own * (bright / (4.0 * PI)) * FIRE_ON_SMOKE;
            }
            // Light that a pale gas (dust) sends on is sent on again inside it; soot's is not.
            let again = 1.0 / (1.0 - 0.8 * colour.r);
            let sent = dark * (colour * again * (from_sun / (4.0 * PI) * 3.0 + from_sky * 0.32) + from_fire) + incandescence(heat) * (bright * flame);
            // Shorter steps where the gas is thick, so that no step hides much by itself: the
            // rays of neighbouring pixels start at different depths, and must agree.
            pace = stride * clamp(0.16 / (thick * stride), 0.45, 1.6);
            let part = exp(-thick * pace);
            let hides = through * (1.0 - part);

            light += hides * sent / thick;
            seen_at += hides * t;
            weight += hides;
            through *= part;
        }
        t += pace;
    }
    let cover = 1.0 - through;
    if cover < 0.002 && max(light.r, light.g) < 0.002 {
        return out;
    }
    // The haze between the eye and the fire, as over everything else.
    let middle = rd * select(t_in, seen_at / weight, weight > 1e-5);
    let haze = haze_at(middle);
    out.colour = vec4<f32>(light * haze.through + haze.light * cover, cover);


    return out;
}

@fragment
fn fire_over(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = fire.size.xy;
    let uv = at.xy / (size * fire.size.z);
    let mine = min(fire_scene_distance(vec2<i32>(at.xy), fire_ray(uv)), 20000.0);
    // The nine of the march's pixels round this one, each counted by how near it is (a tent a
    // little wider than a pixel of the march, which also smooths the grain of the rays' different
    // starts) and by how close to this pixel's distance its ray stopped.
    let spot = at.xy / fire.size.z - 0.5;
    let middle = vec2<i32>(round(spot));
    let most = vec2<i32>(size) - vec2<i32>(1);
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var k = 0; k < 9; k++) {
        let o = vec2<i32>(k % 3, k / 3) - vec2<i32>(1);
        let texel = clamp(middle + o, vec2<i32>(0), most);
        let reached = min(textureLoad(grid_reached, texel, 0).r, 20000.0);
        let side = max(vec2<f32>(0.0), 1.0 - abs(vec2<f32>(middle + o) - spot) / 1.15);
        let w = (side.x * side.y + 0.005) / (0.02 + abs(reached - mine) / mine);
        sum += textureLoad(grid_marched, texel, 0) * w;
        total += w;
    }
    let c = sum / total;
    if c.a < 0.002 && max(c.r, c.g) < 0.002 {
        discard;
    }
    return c;
}
