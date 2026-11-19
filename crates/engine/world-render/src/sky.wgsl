// The sky's tables, made on the GPU by `sky.rs` (after `sky_common.wgsl`): the air's
// transmittance, the light it scatters more than once, the sky seen from the eye, the light
// scattered and let through between the eye and every point round it (the aerial perspective),
// and the ambient light on a surface by the way it faces. Hillaire 2020 ("A scalable and
// production ready sky and atmosphere rendering technique").
//
// Units: the tables hold the light scattered for a light of 1 above the air (the transmittance and
// the multiple scattering) or the scene's light, already multiplied by `sun_top` and `moon_top`
// (the sky, the aerial perspective, the ambient): the radiance a white surface lit by that light
// would have, as the scene's shaders count it (they leave out the 1 / pi of Lambert's law, so a
// radiance is pi times the physical one).

struct Sky {
    rayleigh: vec4<f32>,        // xyz = scattering at sea level, 1/m; w = its scale height, m
    mie_scattering: vec4<f32>,  // xyz = scattering of the haze where it is thickest, 1/m; w = its scale height, m
    mie_extinction: vec4<f32>,  // xyz = its extinction there; w = the altitude below which it is uniform, m
    ozone: vec4<f32>,           // xyz = absorption of the ozone at its peak, 1/m; w = the haze's asymmetry g
    sun: vec4<f32>,             // xyz = towards the sun, in the tables' frame; w = the ground's albedo
    moon: vec4<f32>,            // xyz = towards the moon
    sun_top: vec4<f32>,         // rgb = the sun's light above the air, scene units; w = the eye's altitude, m
    moon_top: vec4<f32>,        // rgb = the moon's; w = how far the aerial perspective reaches, m
    ground: vec4<f32>,          // x = altitude of the ambient table's ground rows, m
};

@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var lut_sampler: sampler;
@group(0) @binding(2) var transmittance_lut: texture_2d<f32>;
@group(0) @binding(3) var multiple_lut: texture_2d<f32>;

@group(1) @binding(0) var transmittance_out: texture_storage_2d<rgba16float, write>;
@group(1) @binding(1) var multiple_out: texture_storage_2d<rgba16float, write>;
@group(1) @binding(2) var sky_view_out: texture_storage_2d<rgba16float, write>;
@group(1) @binding(3) var aerial_scatter_out: texture_storage_3d<rgba16float, write>;
@group(1) @binding(4) var aerial_through_out: texture_storage_3d<rgba16float, write>;
@group(1) @binding(5) var ambient_out: texture_storage_2d<rgba16float, write>;

// Rayleigh, haze and ozone, each as a share of its own peak, at an altitude.
fn densities(altitude: f32) -> vec3<f32> {
    let h = max(altitude, 0.0);
    return vec3<f32>(
        exp(-h / sky.rayleigh.w),
        exp(-max(h - sky.mie_extinction.w, 0.0) / sky.mie_scattering.w),
        max(0.0, 1.0 - abs(h - 25000.0) / 15000.0),
    );
}

fn extinction(d: vec3<f32>) -> vec3<f32> {
    return sky.rayleigh.xyz * d.x + sky.mie_extinction.xyz * d.y + sky.ozone.xyz * d.z;
}

fn rayleigh_phase(c: f32) -> f32 {
    return 3.0 / (16.0 * SKY_PI) * (1.0 + c * c);
}

// Cornette and Shanks's phase function of the haze.
fn mie_phase(c: f32) -> f32 {
    let g = sky.ozone.w;
    let g2 = g * g;
    return 3.0 / (8.0 * SKY_PI) * (1.0 - g2) * (1.0 + c * c) / ((2.0 + g2) * pow(1.0 + g2 - 2.0 * g * c, 1.5));
}

// Distance along a ray from `o` (relative to the planet's centre) to a sphere of radius `r`
// round the centre: the near and far crossings, (-1, -1) for none.
fn ray_sphere(o: vec3<f32>, d: vec3<f32>, r: f32) -> vec2<f32> {
    let b = dot(o, d);
    let closest = length(o - d * b);
    if closest > r {
        return vec2<f32>(-1.0);
    }
    let s = sqrt((r - closest) * (r + closest));
    return vec2<f32>(-b - s, -b + s);
}

// `light_through_table` in this module's table.
fn light_through(altitude: f32, mu: f32) -> vec3<f32> {
    return light_through_table(transmittance_lut, lut_sampler, altitude, mu);
}

// The light scattered more than once at a point (Hillaire's psi_ms), for a light of 1 above the air.
fn multiple(altitude: f32, mu: f32) -> vec3<f32> {
    let uv = vec2<f32>(texel_from_unit(mu * 0.5 + 0.5, MULTIPLE_SIZE.x), texel_from_unit(saturate(altitude / (TOP_RADIUS - GROUND_RADIUS)), MULTIPLE_SIZE.y));
    return textureSampleLevel(multiple_lut, lut_sampler, uv, 0.0).rgb;
}

// ---------------------------------------------------------------- transmittance

@compute @workgroup_size(8, 8)
fn transmittance(@builtin(global_invocation_id) id: vec3<u32>) {
    if f32(id.x) >= TRANSMITTANCE_SIZE.x || f32(id.y) >= TRANSMITTANCE_SIZE.y {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / TRANSMITTANCE_SIZE;
    let x_mu = unit_from_texel(uv.x, TRANSMITTANCE_SIZE.x);
    let x_r = unit_from_texel(uv.y, TRANSMITTANCE_SIZE.y);
    // Bruneton's inverse of `transmittance_uv`.
    let h = sqrt(TOP_RADIUS * TOP_RADIUS - GROUND_RADIUS * GROUND_RADIUS);
    let rho = h * x_r;
    let r = sqrt(rho * rho + GROUND_RADIUS * GROUND_RADIUS);
    let d_min = TOP_RADIUS - r;
    let d_max = rho + h;
    let d = d_min + x_mu * (d_max - d_min);
    var mu = 1.0;
    if d > 0.0 {
        mu = clamp((h * h - rho * rho - d * d) / (2.0 * r * d), -1.0, 1.0);
    }
    let o = vec3<f32>(0.0, r, 0.0);
    let dir = vec3<f32>(sqrt(max(1.0 - mu * mu, 0.0)), mu, 0.0);
    var depth = vec3<f32>(0.0);
    let steps = 40;
    for (var i = 0; i < steps; i++) {
        let p = o + dir * (d * (f32(i) + 0.5) / f32(steps));
        depth += extinction(densities(length(p) - GROUND_RADIUS)) * (d / f32(steps));
    }
    textureStore(transmittance_out, vec2<i32>(id.xy), vec4<f32>(exp(-depth), 1.0));
}

// ---------------------------------------------------------------- multiple scattering

// The directions the light is gathered from at a point: rings of equal area from the zenith to
// the nadir, each with its points over half a turn, from the sun's side to the side away from it
// (the sun is in the plane that divides the sphere in two, so the other half is this one's mirror
// image and the points count twice). And the steps along each.
const GATHER_RINGS: i32 = 8;
const GATHER_SPOKES: i32 = 8;
const GATHER_STEPS: i32 = 20;

// Hillaire 2020, section 5.5: the light that reaches a point of the air having been scattered at
// least twice, taken as coming equally from everywhere. Two means over the directions round the
// point: `once`, the sunlight scattered once along the direction (or thrown back by the ground at
// its end) that arrives at the point, and `again`, the share of the light of air equally bright
// everywhere that would arrive scattered from along it. The light scattered twice is `once`
// through the phase function of the same light everywhere, 1 / (4 pi); each further scattering
// keeps the share `again` of the one before, so all of them add to 1 / (1 - again) times it.
@compute @workgroup_size(8, 8)
fn multiple_scattering(@builtin(global_invocation_id) id: vec3<u32>) {
    if f32(id.x) >= MULTIPLE_SIZE.x || f32(id.y) >= MULTIPLE_SIZE.y {
        return;
    }
    // The table's axes, as `multiple` reads them: the sun's height and the altitude.
    let cell = (vec2<f32>(id.xy) + 0.5) / MULTIPLE_SIZE;
    let sun_height = unit_from_texel(cell.x, MULTIPLE_SIZE.x) * 2.0 - 1.0;
    let altitude = max(unit_from_texel(cell.y, MULTIPLE_SIZE.y) * (TOP_RADIUS - GROUND_RADIUS), 1.0);
    let here = vec3<f32>(0.0, GROUND_RADIUS + altitude, 0.0);
    let sun = vec3<f32>(sqrt(max(1.0 - sun_height * sun_height, 0.0)), sun_height, 0.0);
    var once = vec3<f32>(0.0);
    var again = vec3<f32>(0.0);
    for (var ring = 0; ring < GATHER_RINGS; ring++) {
        let y = 1.0 - (2.0 * f32(ring) + 1.0) / f32(GATHER_RINGS);
        let level = sqrt(1.0 - y * y);
        for (var spoke = 0; spoke < GATHER_SPOKES; spoke++) {
            let turn = SKY_PI * (f32(spoke) + 0.5) / f32(GATHER_SPOKES);
            let dir = vec3<f32>(level * cos(turn), y, level * sin(turn));
            let ground = ray_sphere(here, dir, GROUND_RADIUS);
            let lands = ground.x > 0.0;
            let reach = select(ray_sphere(here, dir, TOP_RADIUS).y, ground.x, lands);
            let dt = reach / f32(GATHER_STEPS);
            // What gets through the air from the step to the point.
            var clear = vec3<f32>(1.0);
            for (var i = 0; i < GATHER_STEPS; i++) {
                let p = here + dir * (dt * (f32(i) + 0.5));
                let r = length(p);
                let d = densities(r - GROUND_RADIUS);
                let lost = extinction(d);
                let beyond = clear * exp(-lost * dt);
                // What the step scatters towards the point, per unit of light in it, dimmed on its
                // way by the air of the step itself: the integral over an even step.
                let sent = (sky.rayleigh.xyz * d.x + sky.mie_scattering.xyz * d.y) * (clear - beyond) / max(lost, vec3<f32>(1e-12));
                once += sent * light_through(r - GROUND_RADIUS, dot(p, sun) / r);
                again += sent;
                clear = beyond;
            }
            if lands {
                // The ground at the end, a matt surface: albedo / pi of what falls on it, and the
                // 4 pi the sum is divided by below.
                let facing = dot(normalize(here + dir * reach), sun);
                once += clear * light_through(0.0, facing) * (max(facing, 0.0) * sky.sun.w * 4.0);
            }
        }
    }
    let directions = f32(GATHER_RINGS * GATHER_SPOKES);
    let twice = once / (4.0 * SKY_PI * directions);
    textureStore(multiple_out, vec2<i32>(id.xy), vec4<f32>(twice / (1.0 - again / directions), 1.0));
}

// ---------------------------------------------------------------- along a ray

struct March {
    light: vec3<f32>,
    through: vec3<f32>,
};

// The light of the sun and the moon scattered towards `o` from the stretch of the ray `t0..t1`
// along `dir`, in `steps` steps, and what of the light from beyond it gets through; `march.through`
// is what got through up to `t0` and `march.light` what was scattered before it.
fn march(o: vec3<f32>, dir: vec3<f32>, t0: f32, t1: f32, steps: i32, start: March) -> March {
    let c_sun = dot(dir, sky.sun.xyz);
    let c_moon = dot(dir, sky.moon.xyz);
    let phase_sun = vec2<f32>(rayleigh_phase(c_sun), mie_phase(c_sun));
    let phase_moon = vec2<f32>(rayleigh_phase(c_moon), mie_phase(c_moon));
    var m = start;
    let dt = (t1 - t0) / f32(steps);
    for (var i = 0; i < steps; i++) {
        let p = o + dir * (t0 + dt * (f32(i) + 0.5));
        let r = length(p);
        let up = p / r;
        let h = r - GROUND_RADIUS;
        let d = densities(h);
        let ext = extinction(d);
        let rayleigh = sky.rayleigh.xyz * d.x;
        let mie = sky.mie_scattering.xyz * d.y;
        let mu_sun = dot(up, sky.sun.xyz);
        let mu_moon = dot(up, sky.moon.xyz);
        let scatter = sky.sun_top.rgb * (light_through(h, mu_sun) * (rayleigh * phase_sun.x + mie * phase_sun.y) + multiple(h, mu_sun) * (rayleigh + mie))
            + sky.moon_top.rgb * (light_through(h, mu_moon) * (rayleigh * phase_moon.x + mie * phase_moon.y) + multiple(h, mu_moon) * (rayleigh + mie));
        let step_through = exp(-ext * dt);
        m.light += m.through * scatter * (1.0 - step_through) / max(ext, vec3<f32>(1e-12));
        m.through *= step_through;
    }
    return m;
}

// The sky seen from an altitude along `dir`: the light the air scatters towards the eye, in scene
// units (times pi: see the top of the file), up to the top of the air or the ground.
fn sky_along(altitude: f32, dir: vec3<f32>, steps: i32) -> vec3<f32> {
    let o = vec3<f32>(0.0, GROUND_RADIUS + max(altitude, 1.0), 0.0);
    let top = ray_sphere(o, dir, TOP_RADIUS);
    let ground = ray_sphere(o, dir, GROUND_RADIUS);
    let t_max = select(top.y, ground.x, ground.x > 0.0);
    // Shorter steps near the eye, where the air is densest: in four stretches of growing length.
    var m = March(vec3<f32>(0.0), vec3<f32>(1.0));
    var t = 0.0;
    for (var k = 1; k <= 4; k++) {
        let next = t_max * f32(k * k) / 16.0;
        m = march(o, dir, t, next, steps / 4, m);
        t = next;
    }
    return m.light * SKY_PI;
}

// ---------------------------------------------------------------- the sky from the eye

@compute @workgroup_size(8, 8)
fn sky_view(@builtin(global_invocation_id) id: vec3<u32>) {
    if f32(id.x) >= SKY_VIEW_SIZE.x || f32(id.y) >= SKY_VIEW_SIZE.y {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / SKY_VIEW_SIZE;
    let dir = sky_direction(vec2<f32>(unit_from_texel(uv.x, SKY_VIEW_SIZE.x), unit_from_texel(uv.y, SKY_VIEW_SIZE.y)));
    textureStore(sky_view_out, vec2<i32>(id.xy), vec4<f32>(sky_along(sky.sun_top.w, dir, 32), 1.0));
}

// ---------------------------------------------------------------- the aerial perspective

// One thread per direction marches out to `moon_top.w` and writes every slice it passes: the light
// scattered towards the eye up to there, and what gets through.
@compute @workgroup_size(8, 8)
fn aerial(@builtin(global_invocation_id) id: vec3<u32>) {
    if f32(id.x) >= AERIAL_SIZE.x || f32(id.y) >= AERIAL_SIZE.y {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / AERIAL_SIZE.xy;
    let dir = sky_direction(vec2<f32>(unit_from_texel(uv.x, AERIAL_SIZE.x), unit_from_texel(uv.y, AERIAL_SIZE.y)));
    let o = vec3<f32>(0.0, GROUND_RADIUS + max(sky.sun_top.w, 1.0), 0.0);
    let far = sky.moon_top.w;
    let slices = i32(AERIAL_SIZE.z);
    var m = March(vec3<f32>(0.0), vec3<f32>(1.0));
    var t = 0.0;
    textureStore(aerial_scatter_out, vec3<i32>(vec2<i32>(id.xy), 0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    textureStore(aerial_through_out, vec3<i32>(vec2<i32>(id.xy), 0), vec4<f32>(1.0));
    for (var k = 1; k < slices; k++) {
        let w = f32(k) / f32(slices - 1);
        let next = far * w * w;
        m = march(o, dir, t, next, 3, m);
        t = next;
        textureStore(aerial_scatter_out, vec3<i32>(vec2<i32>(id.xy), k), vec4<f32>(m.light * SKY_PI, 1.0));
        textureStore(aerial_through_out, vec3<i32>(vec2<i32>(id.xy), k), vec4<f32>(m.through, 1.0));
    }
}

// ---------------------------------------------------------------- the ambient light

// The light of the sky on a surface facing `n` at an altitude, over the directions above the
// horizon, in the scene's units: (1 / pi) times its irradiance.
fn sky_on(n: vec3<f32>, altitude: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let directions = 96;
    for (var k = 0; k < directions; k++) {
        // The upper half of a Fibonacci sphere.
        let z = 1.0 - (f32(k) + 0.5) / f32(directions);
        let ring = sqrt(max(1.0 - z * z, 0.0));
        let angle = f32(k) * 2.399963;
        let dir = vec3<f32>(ring * cos(angle), z, ring * sin(angle));
        let facing = dot(n, dir);
        if facing > 0.0 {
            sum += sky_along(altitude, dir, 12) * facing;
        }
    }
    // Each direction stands for 2 pi / directions of the half sphere.
    return sum * (2.0 / f32(directions));
}

@compute @workgroup_size(8, 8)
fn ambient(@builtin(global_invocation_id) id: vec3<u32>) {
    if f32(id.x) >= AMBIENT_SIZE.x || f32(id.y) >= AMBIENT_SIZE.y {
        return;
    }
    let half = AMBIENT_SIZE.y * 0.5;
    let row = select(0.0, 1.0, f32(id.y) >= half);
    let j = f32(id.y) - row * half;
    // Evenly in the cosine of the azimuth from the sun and in the height (`ambient_uv`).
    let c = 1.0 - 2.0 * f32(id.x) / (AMBIENT_SIZE.x - 1.0);
    let y = 2.0 * j / (half - 1.0) - 1.0;
    let level = sqrt(max(1.0 - y * y, 0.0));
    let n = vec3<f32>(level * c, y, level * sqrt(max(1.0 - c * c, 0.0)));
    let ground_altitude = sky.ground.x;
    let altitude = select(ground_altitude, max(sky.sun_top.w, ground_altitude), row > 0.5);
    // The ground's light: what falls on it from the sun, the moon and the sky, thrown back.
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let direct = sky.sun_top.rgb * light_through(ground_altitude, sky.sun.y) * max(sky.sun.y, 0.0)
        + sky.moon_top.rgb * light_through(ground_altitude, sky.moon.y) * max(sky.moon.y, 0.0);
    let ground = sky.sun.w * (direct + sky_on(up, ground_altitude));
    // Below the horizon a surface facing `n` sees (1 - n.y) / 2 of the ground's light.
    let light = sky_on(n, altitude) + ground * (1.0 - n.y) * 0.5;
    textureStore(ambient_out, vec2<i32>(id.xy), vec4<f32>(light, 1.0));
}
