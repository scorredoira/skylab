// The cloud lab's picture: the sky of the tables, a plain ground and the clouds of `clouds.wgsl`,
// one sample of each per frame, averaged over the frames by the blend. Without the shadow map:
// the shade of the clouds is marched towards the sun at every sample.

struct Lab {
    right: vec4<f32>,  // xyz = camera right, w = tan(half fov x)
    up: vec4<f32>,     // xyz = camera up, w = tan(half fov y)
    fwd: vec4<f32>,    // xyz = camera forward
    sky: vec4<f32>,    // xy = the sun's azimuth (x, z), z = unused, w = how far the aerial perspective reaches
    light: vec4<f32>,  // x = the sun's light above the air, y = frame, zw = size of the picture
    sun: vec4<f32>,    // xyz = towards the sun
};

@group(0) @binding(0) var<uniform> clouds: Clouds;
@group(0) @binding(1) var cloud_weather: texture_2d<f32>;
@group(0) @binding(2) var cloud_shape: texture_3d<f32>;
@group(0) @binding(3) var cloud_detail: texture_3d<f32>;
@group(0) @binding(4) var cloud_turbulence: texture_2d<f32>;
@group(0) @binding(5) var cloud_repeat: sampler;
@group(0) @binding(6) var sky_transmittance: texture_2d<f32>;
@group(0) @binding(7) var sky_view: texture_2d<f32>;
@group(0) @binding(8) var aerial_scatter: texture_3d<f32>;
@group(0) @binding(9) var aerial_through: texture_3d<f32>;
@group(0) @binding(10) var sky_ambient: texture_2d<f32>;
@group(0) @binding(11) var sky_sampler: sampler;
@group(0) @binding(12) var<uniform> lab: Lab;

fn cloud_sun_light(altitude: f32) -> vec3<f32> {
    return vec3<f32>(lab.light.x) * light_through_table(sky_transmittance, sky_sampler, altitude, lab.sun.y);
}

// The sky on a cloud: from above, and a little from the horizon away from the sun.
fn cloud_sky_light() -> vec3<f32> {
    let up = textureSampleLevel(sky_ambient, sky_sampler, ambient_uv(1.0, 1.0, 1.0), 0.0).rgb;
    let away = textureSampleLevel(sky_ambient, sky_sampler, ambient_uv(-1.0, 0.0, 1.0), 0.0).rgb;
    return up * 0.8 + away * 0.2;
}

var<private> lab_jitter: f32 = 0.5;

// The optical depth of the clouds from `p` towards the sun, less what is nearer than `offset`:
// marched to the top of the layers that shade, in steps that grow.
fn cloud_shadow_depth(p: vec3<f32>, offset: f32) -> f32 {
    let sun = clouds.sun.xyz;
    let to_top = (clouds.heights.z - p.y) / max(sun.y, 0.03);
    var depth = 0.0;
    var step = 120.0;
    var t = offset + step * lab_jitter;
    for (var i = 0; i < 14; i++) {
        if t > to_top {
            break;
        }
        let q = p + sun * t;
        if !cloud_in_gap(q.y) {
            let weather = cloud_weather_at(q.xz, q.y, 0.5);
            if any(weather.density > vec4<f32>(CLOUD_MIN_DENSITY)) {
                depth += cloud_media(weather, q, 0.5, 0.5).extinction * step;
            }
        }
        t += step;
        step *= 1.35;
    }
    return depth;
}

fn sky_place(d: vec3<f32>) -> vec2<f32> {
    let level = dot(d.xz, d.xz);
    let c = select(1.0, dot(d.xz, lab.sky.xy) * inverseSqrt(level), level > 1e-12);
    return vec2<f32>(c, d.y);
}

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn random(seed: ptr<function, u32>) -> f32 {
    *seed = pcg(*seed);
    return f32(*seed >> 8u) / 16777216.0;
}

fn plot(p: vec2<f32>) -> f32 {
    let c = vec2<u32>(vec2<i32>(floor(p)) + 100000);
    return f32(pcg(c.x + pcg(c.y)) >> 8u) / 16777216.0;
}

// Fields, woods and a few pale patches: enough to read the gaps between the clouds and their shadows.
fn ground_colour(xz: vec2<f32>) -> vec3<f32> {
    let field = plot(xz / 420.0 + vec2<f32>(plot(xz / 2600.0), plot(xz / 1900.0 + 7.0)));
    let wood = plot(xz / 3100.0 + 31.0);
    var c = mix(vec3<f32>(0.075, 0.115, 0.035), vec3<f32>(0.16, 0.17, 0.06), field);
    c = mix(c, vec3<f32>(0.03, 0.06, 0.022), step(0.72, wood));
    return mix(c, vec3<f32>(0.22, 0.19, 0.11), step(0.9, field));
}

@vertex
fn lab_vertex(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(select(-1.0, 3.0, vid == 1u), select(-1.0, 3.0, vid == 2u), 0.0, 1.0);
}

@fragment
fn lab_fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let frame = u32(lab.light.y);
    var seed = pcg(u32(at.x) + pcg(u32(at.y) + pcg(frame)));
    let pixel = at.xy + vec2<f32>(random(&seed), random(&seed)) - 0.5;
    let uv = pixel / lab.light.zw;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let v = normalize(lab.fwd.xyz + ndc.x * lab.right.w * lab.right.xyz + ndc.y * lab.up.w * lab.up.xyz);
    let eye = clouds.origin.z;
    let sun = lab.sun.xyz;
    let jitter = random(&seed);
    lab_jitter = random(&seed);

    let a = sky_place(v);
    var behind = textureSampleLevel(sky_view, sky_sampler, sky_view_uv(a.x, max(a.y, horizon_mu(eye))), 0.0).rgb;
    var reach = 1e9;
    let down = cloud_level(eye, v.y, 0.0);
    if down.x > 0.0 {
        reach = down.x;
        let p = cloud_point(v, reach);
        let direct = cloud_sun_light(0.0) * max(sun.y, 0.0) * exp(-cloud_shadow_depth(vec3<f32>(p.x, 0.0, p.z), 0.0));
        let lit = ground_colour(p.xz) * (direct + cloud_sky_light());
        let uvw = aerial_uvw(a.x, a.y, reach, lab.sky.w);
        behind = lit * textureSampleLevel(aerial_through, sky_sampler, uvw, 0.0).rgb + textureSampleLevel(aerial_scatter, sky_sampler, uvw, 0.0).rgb;
    } else {
        behind += vec3<f32>(lab.light.x * 40.0) * light_through_table(sky_transmittance, sky_sampler, eye, sun.y) * smoothstep(0.99996, 0.99999, dot(v, sun));
    }
    // The rain: behind the clouds for an eye over their base, before them for one under it.
    var rain = vec4<f32>(0.0);
    if clouds.rain.z + clouds.rain.w > 0.0 {
        let fallen = cloud_rain(v, reach, jitter);
        if fallen.colour.a > 0.0 {
            let uvw = aerial_uvw(a.x, a.y, fallen.front, lab.sky.w);
            rain = vec4<f32>(fallen.colour.rgb * textureSampleLevel(aerial_through, sky_sampler, uvw, 0.0).rgb + textureSampleLevel(aerial_scatter, sky_sampler, uvw, 0.0).rgb * fallen.colour.a, fallen.colour.a);
        }
    }
    let under = eye < clouds.heights.x + RAIN_INTO;
    if !under {
        behind = behind * (1.0 - rain.a) + rain.rgb;
    }
    var range = cloud_range(v);
    range.y = min(range.y, reach);
    var colour = behind;
    var hazy = min(select(cloud_level(eye, v.y, clouds.heights.y).y, down.x, down.x > 0.0), reach);
    if range.x >= 0.0 && range.y > range.x {
        let m = cloud_march(v, range, jitter, 1.0);
        if m.front >= 0.0 {
            let uvw = aerial_uvw(a.x, a.y, m.front, lab.sky.w);
            let through = textureSampleLevel(aerial_through, sky_sampler, uvw, 0.0).rgb;
            let haze = textureSampleLevel(aerial_scatter, sky_sampler, uvw, 0.0).rgb;
            colour = behind * (1.0 - m.colour.a) + m.colour.rgb * through + haze * m.colour.a;
            hazy = mix(hazy, min(m.front, hazy), m.colour.a);
        }
    }
    if under {
        colour = colour * (1.0 - rain.a) + rain.rgb;
    }
    if hazy > 0.0 {
        let haze = cloud_haze(v, hazy, 0.0);
        colour = mix(colour, haze.rgb, haze.a);
    }
    return vec4<f32>(colour, 1.0);
}
