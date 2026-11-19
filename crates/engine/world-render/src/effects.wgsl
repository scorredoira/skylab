// The effects of the wrecks, drawn in the second pass over the scene (`Gpu::translucent_pass`),
// appended to shaders.wgsl and heat.wgsl: smoke, flames, dust and mist as balls of gas marched
// through a 3D noise (noise.wgsl), sparks, the flash, thrown water and fragments. Every particle is
// one instance of one draw, its data in a storage buffer (`effects.rs`, `particles.rs`), blended
// premultiplied: what glows adds its light, what is dark covers. The fire itself, where the
// quality simulates it, is fire_draw.wgsl's.

// Mirrors `EffectUniforms` in particles.rs.
struct Effects {
    view: vec4<f32>,   // x = near distance, y = pixels per radian of view (height / fov), z = time, w unused
};

// Mirrors `GpuParticle` in particles.rs: positions relative to the camera, render axes.
struct Particle {
    pos: vec3<f32>,
    size: f32,
    vel: vec3<f32>,
    kind: f32,
    color: vec4<f32>,
    params: vec4<f32>,   // x = temperature, y = seed, z = age, w = shade
};

@group(1) @binding(0) var<uniform> fx: Effects;
@group(1) @binding(1) var<storage, read> particles: array<Particle>;
// `pipelines.rs` makes this a texture_depth_2d when the scene has one sample per pixel.
@group(1) @binding(2) var scene_depth: texture_depth_multisampled_2d;
@group(1) @binding(3) var noise: texture_3d<f32>;
@group(1) @binding(4) var noise_sampler: sampler;

// Distance along the ray `rd` (unit, from the eye) to what the scene has drawn at this pixel.
fn scene_distance(pixel: vec2<f32>, rd: vec3<f32>) -> f32 {
    let z = textureLoad(scene_depth, vec2<i32>(pixel), 0);
    if z <= 0.0 {
        return 1e9;
    }
    return fx.view.x / z / max(dot(rd, frame.cam_fwd.xyz), 0.05);
}

// A number 0..1 that differs from pixel to pixel (Roberts' R2 sequence over the plane): where
// each ray's march starts.
fn pixel_dither(at: vec2<f32>) -> f32 {
    return fract(dot(at, vec2<f32>(0.7548776662, 0.5698402910)));
}

// Henyey and Greenstein's lobe: how much of the light goes on at the cosine `c` from where it was going.
fn lobe(c: f32, g: f32) -> f32 {
    return (1.0 - g * g) / (4.0 * PI * pow(1.0 + g * g - 2.0 * g * c, 1.5));
}

struct ParticleOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) index: u32,
    // For a ball of gas, the light it is in, the same all over it: the sky's from above, the
    // fires', and the haze between the eye and it (what it lets through, what it adds).
    @location(3) @interpolate(flat) sky: vec3<f32>,
    @location(4) @interpolate(flat) fires: vec3<f32>,
    @location(5) @interpolate(flat) haze_through: vec3<f32>,
    @location(6) @interpolate(flat) haze_light: vec3<f32>,
};

@vertex
fn particle_vertex(@builtin(vertex_index) vid: u32, @builtin(instance_index) iid: u32) -> ParticleOut {
    var corners = CORNERS;
    let c = corners[vid];
    let p = particles[iid];
    let kind = i32(p.kind + 0.5);
    let right = frame.cam_right.xyz;
    let up = frame.cam_up.xyz;
    var w: vec3<f32>;
    if kind == 1 || kind == 3 {
        // A streak along its velocity, as wide as it is.
        let sp = length(p.vel);
        let axis = select(up, p.vel / max(sp, 1e-3), sp > 0.01);
        let side = normalize(cross(axis, normalize(p.pos)) + vec3<f32>(1e-5, 0.0, 0.0));
        let long = select(p.size + sp * 0.03, p.size * 1.6 + sp * 0.045, kind == 1);
        w = p.pos + axis * c.y * long + side * c.x * p.size;
    } else if kind == 0 {
        // A ball of gas, marched: a square that just hides the ball from the eye, facing the eye,
        // set where the ball begins, so that what the scene has in front of the ball's middle does
        // not hide its near side. From a distance d a ball of radius r is a cone of half-angle
        // asin(r / d).
        let d = length(p.pos);
        let near = fx.view.x * 4.0;
        if d > p.size * 1.05 {
            let towards = p.pos / d;
            let side = normalize(cross(up, towards));
            let over = cross(towards, side);
            let front = max(d - p.size, near);
            let wide = front * p.size * inverseSqrt(d * d - p.size * p.size) * 1.1;
            w = towards * front + (side * c.x + over * c.y) * wide;
        } else {
            // The eye is in it, or all but: the whole picture.
            w = frame.cam_fwd.xyz * near + (right * c.x + up * c.y) * (near * 40.0);
        }
    } else {
        // A square facing the eye, turned by its own angle (a tumbling fragment, the flash).
        let a = p.params.y * 6.283 + p.params.z * (3.0 + 9.0 * p.params.y) * f32(kind == 2);
        let r = right * cos(a) + up * sin(a);
        let u = -right * sin(a) + up * cos(a);
        w = p.pos + (r * c.x + u * c.y) * p.size;
    }
    var o: ParticleOut;
    o.position = frame.view_proj * vec4<f32>(w, 1.0);
    o.rel = w;
    o.uv = c;
    o.index = iid;
    if kind == 0 {
        o.sky = sky_light(vec3<f32>(0.0, 1.0, 0.0), p.pos);
        // The fires' lights are points (they are made to light the ground): a ball round one is
        // not lit without end.
        let fires = fire_glow(p.pos);
        o.fires = fires * min(1.0, 2.0 / max(fires.r, 1e-3));
        let haze = haze_at(p.pos);
        o.haze_through = haze.through;
        o.haze_light = haze.light;
    }
    return o;
}

// Steps of the march through a ball of gas with lumps.
const BALL_STEPS: i32 = 6;

// How much gas there is at `q` of a ball (its middle 0, its rim at length 1), in x: a round body
// eaten into by two scales of noise, more towards the rim, so the outline is lumps and hollows;
// and that noise, 0..1, in y. The coarse noise turns slowly about the ball's upright axis and the
// fine one drifts up through it, each ball at its own rate (`seed`), so the billows churn as it
// ages. `rough`: how much of that eating there is, 0..1 (none: no noise is read).
fn ball_gas(q: vec3<f32>, seed: f32, age: f32, rough: f32) -> vec2<f32> {
    let r2 = dot(q, q);
    if r2 >= 1.0 {
        return vec2<f32>(0.0, 0.5);
    }
    if rough <= 0.0 {
        return vec2<f32>(1.0 - r2, 0.5);
    }
    let turn = age * (0.1 + 0.25 * seed) * select(-1.0, 1.0, fract(seed * 7.0) > 0.5);
    let turned = vec3<f32>(q.x * cos(turn) - q.z * sin(turn), q.y, q.x * sin(turn) + q.z * cos(turn));
    let own = vec3<f32>(seed * 17.3, seed * 5.9, seed * 11.7);
    let coarse = textureSampleLevel(noise, noise_sampler, turned * 0.21 + own, 0.0).r;
    let fine = textureSampleLevel(noise, noise_sampler, q * 0.47 + own.zxy + vec3<f32>(0.0, -age * 0.035, 0.0), 0.0);
    let lumps = smoothstep(0.25, 0.8, coarse * 0.5 + fine.g * 0.32 + fine.b * 0.18);
    return vec2<f32>(saturate((1.0 - r2) * (1.0 + rough) - (1.0 - lumps) * (0.7 + 1.3 * r2) * rough), lumps);
}

// A ball of gas: smoke, dust, mist, or burning gas. Marched from where the view's ray enters it to
// where it leaves it or meets the scene, so it lies on the ground and round the wreck without an
// edge. Each step is lit by the sun through the rest of the ball on the sun's side (the way out
// along the sun's direction, the hollows darker than the lumps), by the sky from above, and by
// the fires near it; and gives off light by its heat, most where it is thickest, in tongues.
fn gas_ball(in: ParticleOut, p: Particle) -> vec4<f32> {
    let rd = normalize(in.rel);
    // The chord of the ray through the ball: its middle at `along`, half of it `reach`.
    let along = dot(p.pos, rd);
    let miss = dot(p.pos, p.pos) - along * along;
    let r = p.size;
    if miss >= r * r {
        discard;
    }
    let reach = sqrt(r * r - miss);
    let enters = max(along - reach, 0.0);
    let leaves = min(along + reach, scene_distance(in.position.xy, rd));
    if leaves <= enters {
        discard;
    }
    // A small ball (a wisp of a trail) is smooth: lumps only show on a big one.
    let rough = smoothstep(1.5, 3.2, r);
    let stride = (leaves - enters) / f32(BALL_STEPS);
    let seed = p.params.y;
    let age = p.params.z;
    let shaded = 1.0 - 0.7 * p.params.w;
    // Its optical depth through the middle is `color.a`: per metre, over what the ray through
    // the middle of a ball with its lumps meets of it.
    let thick = p.color.a / r * 0.62;

    let sun = frame.sun_dir.xyz;
    let towards = dot(rd, sun);
    let direct = 0.5 * (lobe(towards, 0.45) + lobe(towards, -0.2)) * 4.0 * PI;
    let sky_above = in.sky;
    let sky_below = sky_above * vec3<f32>(0.3, 0.32, 0.28);
    let fires = in.fires;
    let bright = fire_light_scale();
    // Light that a pale gas sends on is sent on again and again inside it (each time what its
    // colour keeps of it): mist and spray are white right through, soot stays black.
    let again = 1.0 / (1.0 - 0.8 * max(p.color.r, max(p.color.g, p.color.b)));

    if rough <= 0.0 {
        // A wisp, smooth: how much of it the ray meets has a closed form (its gas thins from
        // the middle as 1 - d^2 / r^2), and one place, the middle of the ray's way through it,
        // stands for its light. A trail is hundreds of these one over another.
        let a = enters - along;
        let b = leaves - along;
        let met = (1.0 - miss / (r * r)) * (b - a) - (b * b * b - a * a * a) / (3.0 * r * r);
        let hides = 1.0 - exp(-thick * max(met, 0.0));
        if hides < 0.002 {
            discard;
        }
        let x = rd * (0.5 * (enters + leaves)) - p.pos;
        let gas = 1.0 - min(dot(x, x) / (r * r), 1.0);
        let c = dot(x, sun);
        let way = -c + sqrt(max(c * c + r * r - dot(x, x), 0.0));
        let to_sun = thick * way * (0.2 + 0.5 * gas);
        let from_sun = frame.sun_color.rgb * (exp(-to_sun) * direct + 0.2 * again * exp(-to_sun * 0.22 / sqrt(again)));
        let from_sky = mix(sky_below, sky_above, 0.5 + 0.5 * x.y / r);
        var sent = p.color.rgb * ((from_sun / (4.0 * PI) * 3.0 + from_sky * (0.32 + 0.5 * (again - 1.0))) * shaded + fires * 0.25);
        if p.params.x > 0.0 {
            sent += incandescence(p.params.x * (0.45 + 0.55 * gas)) * bright;
        }
        return vec4<f32>(sent * hides * in.haze_through + in.haze_light * hides, hides);
    }

    var light = vec3<f32>(0.0);
    var through = 1.0;
    var t = enters + stride * pixel_dither(in.position.xy + vec2<f32>(seed * 91.0, seed * 37.0));
    for (var i = 0; i < BALL_STEPS; i++) {
        let x = rd * t - p.pos;
        let q = x / r;
        let gas = ball_gas(q, seed, age, rough);
        if gas.x > 0.003 {
            // Thinner right before the eye: from inside a ball one sees out of it, not a wall.
            let dark = thick * gas.x * saturate(t / (0.6 * r));
            // Out of the ball towards the sun.
            let b = dot(x, sun);
            let way = -b + sqrt(max(b * b + r * r - dot(x, x), 0.0));
            let to_sun = thick * way * (0.2 + 0.5 * gas.x) * (1.35 - 0.7 * gas.y);
            let from_sun = frame.sun_color.rgb * (exp(-to_sun) * direct + 0.2 * again * exp(-to_sun * 0.22 / sqrt(again)));
            let to_sky = thick * r * (1.0 - q.y) * 0.3;
            let from_sky = mix(sky_below, sky_above, 0.5 + 0.5 * q.y) * exp(-to_sky);
            var sent = p.color.rgb * dark * ((from_sun / (4.0 * PI) * 3.0 + from_sky * (0.32 + 0.5 * (again - 1.0))) * shaded + fires * 0.25);
            // Burning gas: hottest in its thick middle, in tongues.
            if p.params.x > 0.0 {
                let heat = p.params.x * saturate(gas.x * 4.0) * (0.75 + 0.5 * gas.y);
                sent += incandescence(heat) * (bright * dark);
            }
            let part = exp(-dark * stride);
            light += through * sent * (1.0 - part) / max(dark, 1e-4);
            through *= part;
            if through < 0.02 {
                break;
            }
        }
        t += stride;
    }
    let cover = 1.0 - through;
    if cover < 0.002 && max(light.r, light.g) < 0.002 {
        discard;
    }
    // The haze between the eye and the ball, as over everything else.
    return vec4<f32>(light * in.haze_through + in.haze_light * cover, cover);
}

@fragment
fn particle_fragment(in: ParticleOut) -> @location(0) vec4<f32> {
    let p = particles[in.index];
    let kind = i32(p.kind + 0.5);
    if kind == 0 {
        return gas_ball(in, p);
    }
    let r = length(in.uv);
    if r > 1.0 {
        discard;
    }
    let dist = length(in.rel);
    // Nothing of a flat particle shows behind what the scene has drawn, and it fades out as it meets it.
    let ahead = scene_distance(in.position.xy, in.rel / dist) - dist;
    let soft = clamp(ahead / (p.size * 0.6 + 0.05), 0.0, 1.0);
    let sun = frame.sun_dir.xyz;
    let sky = sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel);
    if kind == 3 {
        // A spark: a streak that cools from white-yellow to red.
        let heat = p.params.x;
        let f = (1.0 - abs(in.uv.x)) * (1.0 - in.uv.y * in.uv.y);
        let c = mix(vec3<f32>(1.0, 0.25, 0.03), vec3<f32>(1.0, 0.85, 0.55), heat * heat) * f * heat * 40.0 * p.color.a;
        return vec4<f32>(c * soft, 0.0);
    }
    if kind == 4 {
        // The flash: a glow, bright in the middle.
        let f = pow(1.0 - r, 3.0);
        return vec4<f32>(p.color.rgb * f * p.color.a * 30.0 * soft, 0.0);
    }
    if kind == 1 {
        // Thrown water: a broken streak of drops, white where the sun is on it, glowing when seen
        // towards the sun (drops scatter forwards).
        let nz = textureSampleLevel(noise, noise_sampler, vec3<f32>(in.uv * vec2<f32>(0.35, 0.12) + p.params.y * 7.0, p.params.y + p.params.z * 0.2), 0.0);
        let f = (1.0 - in.uv.x * in.uv.x) * (1.0 - r * r) * smoothstep(0.25, 0.7, nz.r * 0.7 + nz.g * 0.5);
        let cs = max(dot(in.rel / dist, sun), 0.0);
        let forward = 0.35 + 2.6 * pow(cs, 6.0);
        // Spray is white in the sun: much of the light is scattered from it whichever way it comes.
        let c = p.color.rgb * (frame.sun_color.rgb * (0.45 + forward * 0.5) + sky * 0.9 + fire_glow(p.pos) * 0.3);
        let a = clamp(f * p.color.a * soft, 0.0, 1.0);
        if a < 0.003 {
            discard;
        }
        return vec4<f32>(apply_fog(c, in.rel) * a, a);
    }
    // A fragment: a dark sliver, catching the sun now and then as it turns.
    if abs(in.uv.x) * 1.6 + abs(in.uv.y) > 1.0 {
        discard;
    }
    let flash = pow(max(sin(p.params.y * 61.0 + p.params.z * 9.0), 0.0), 12.0);
    let c = p.color.rgb * (frame.sun_color.rgb * (0.3 + 1.5 * flash) + sky * 0.8 + fire_glow(p.pos));
    return vec4<f32>(apply_fog(c, in.rel), 1.0);
}
