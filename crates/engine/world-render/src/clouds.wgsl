// Ported from takram-design-engineering/three-geospatial (packages/clouds/src/shaders/parameters.glsl,
// clouds.glsl, clouds.frag), MIT, see THIRD_PARTY_NOTICES.md.
//
// Volumetric clouds in up to four layers: where there is cloud (`cloud_weather_at`, `cloud_media`)
// and the light a ray gathers through it (`cloud_march`), the length of a ray that lies in their
// shade (`cloud_shade_length`) and the haze under them (`cloud_haze`).
//
// What is ours and not theirs: the world is flat with the drop of a round one (`cloud_point`,
// `cloud_level`) instead of a sphere, the weather is laid over it by its place instead of round a
// globe, the light of the sun and of the sky comes from our sky's tables, the fourth layer's
// weather is our storm towers, the level of detail is by the ray instead of by the screen's
// derivatives, and, by eye and not by physics: the relief of an overcast sky's base
// (`cloud_weather_at`), the light of a low sun (`cloud_low_sun`, `cloud_golden`, the silver lining
// in `cloud_march`) and the rain that hangs under the storm clouds (`cloud_rain`).
//
// Needs, from whoever includes it: `sky_common.wgsl`; the textures `cloud_weather`, `cloud_shape`,
// `cloud_detail`, `cloud_turbulence` and the sampler `cloud_repeat`; the uniform `clouds`; and, for
// what comes after the line of the march, `cloud_sun_light(altitude)` and `cloud_sky_light()` (the
// light a white surface gets from the sun, facing it, and from the sky, facing up) and
// `cloud_shadow_depth(p, offset)`, the optical depth of the clouds from `p` towards the sun
// farther than `offset`.

struct Clouds {
    sun: vec4<f32>,          // xyz = towards the light, w = time, s
    wind: vec4<f32>,         // xy = how far the wind has carried the clouds (x, z), m, z = coverage, w = how far their shapes have churned, m
    origin: vec4<f32>,       // xy = the eye's place in the world (x, z), m, z = its altitude, w = storm towers
    more: vec4<f32>,         // x = the view's near distance, y = frame, z = pixels of the scene to a texel of the march each way, w = the sun's light on the clouds, as a factor
    map: vec4<f32>,          // the picture of their shade on the ground: xy = its middle (x, z), z = half its side, m, w = the level it is drawn for, m
    before_right: vec4<f32>, // the view of the frame before: xyz = its right, w = tan(half fov x)
    before_up: vec4<f32>,    // xyz = its up, w = tan(half fov y)
    before_fwd: vec4<f32>,   // xyz = its forward, w = its vertical shift
    moved: vec4<f32>,        // xyz = the eye now less the eye then, w = 1 when there is a frame before
    towers: array<vec4<f32>, 4>, // a storm cloud each: xy = its centre (x, z), z = radius of its base, w = altitude of its top
    jitter: vec4<f32>,       // xy = the pixel of each block the march looks through this frame, z = how far a cloud is near, m (`CloudMarched::near`), w = unused
    // The layers, one to a component.
    min_heights: vec4<f32>,
    max_heights: vec4<f32>,
    density_scales: vec4<f32>,
    shape_amounts: vec4<f32>,
    detail_amounts: vec4<f32>,
    weather_exponents: vec4<f32>,
    shape_biases: vec4<f32>,
    coverage_widths: vec4<f32>,
    profile_exp: vec4<f32>,
    profile_exponent: vec4<f32>,
    profile_linear: vec4<f32>,
    profile_constant: vec4<f32>,
    // The gaps between the layers, where there is never cloud: xyz = where each starts and ends.
    interval_min: vec4<f32>,
    interval_max: vec4<f32>,
    heights: vec4<f32>,      // x = the lowest base, y = the highest top, z = the top of the layers that shade, w = their base
    shadow_mask: vec4<f32>,  // 1 for the layers that shade
    bsm: vec4<f32>,          // the shadow map: xy = its middle (x, z) at the level of the top of the shade, z = half its side, m, w = its side in texels (0: there is none)
    bsm_before: vec4<f32>,   // xy = from a place of the map to the same place in the map before, in its picture; z = 1 when there is one
    light: vec4<f32>,        // x = the sky's light, y = the ground's, z = the haze, each as a factor; w = how much of the shadow map's depth shades the clouds themselves
    shade: vec4<f32>,        // xyz = the way the shadow map is marched: towards the light, never lower than `clouds::SHADE_LOW`; w = how much shade there is, 0..1 (none once the light has set)
    rain: vec4<f32>,         // xy = how far the wind carries the rain as it falls (x, z), m to a metre of fall; z = how much falls from the heavy cumulus, w = from the storm towers, 0..1
};

// Sizes in metres of what the textures tile over (theirs: 100 km of weather, 1/0.0003 and 1/0.006 m
// of shape and detail, a twentieth of the weather for the turbulence; rounded so that all divide
// `clouds::WRAP`).
const WEATHER_TILE: f32 = 96000.0;
const SHAPE_TILE: f32 = 3360.0;
const DETAIL_TILE: f32 = 168.0;
const TURBULENCE_TILE: f32 = 4800.0;
const TURBULENCE_DISPLACEMENT: f32 = 350.0;
const EARTH: f32 = 6360000.0;
const CLOUD_PI: f32 = 3.14159265;

// The quality's (`clouds::source` writes them; these are their `high`).
const CLOUD_STEPS: i32 = 500;
const CLOUD_MIN_STEP: f32 = 50.0;
const CLOUD_MAX_STEP: f32 = 1000.0;
const CLOUD_MAX_RAY: f32 = 200000.0;
const CLOUD_STEP_SCALE: f32 = 1.01;
const CLOUD_MIN_DENSITY: f32 = 0.00001;
const CLOUD_MIN_EXTINCTION: f32 = 0.00001;
const CLOUD_MIN_TRANSMITTANCE: f32 = 0.01;
const CLOUD_SUN_STEPS: i32 = 2;
const CLOUD_GROUND_STEPS: i32 = 3;
const CLOUD_SECONDARY_STEP: f32 = 100.0;
const CLOUD_SECONDARY_SCALE: f32 = 2.0;
const CLOUD_OCTAVES: i32 = 8;
const CLOUD_SHAPE_DETAIL: bool = true;
const CLOUD_TURBULENCE: bool = true;
const CLOUD_SHAFT_STEPS: i32 = 500;
const CLOUD_SHAFT_STEP: f32 = 50.0;
const CLOUD_SHAFT_RAY: f32 = 200000.0;

// The shape's noise where `cloud_media` was last asked.
var<private> cloud_shape_here: f32 = 0.5;
// Which layers a density is asked for: all, or those that shade (the shadow map's march sets it).
var<private> cloud_layers: vec4<f32> = vec4<f32>(1.0);

fn remap_clamped(x: vec4<f32>, low: vec4<f32>, high: vec4<f32>) -> vec4<f32> {
    return saturate((x - low) / (high - low));
}

fn cloud_in_gap(height: f32) -> bool {
    let h = vec3<f32>(height);
    return any((h > clouds.interval_min.xyz) & (h < clouds.interval_max.xyz));
}

struct CloudWeather {
    // How high in each layer, 0..1.
    fraction: vec4<f32>,
    density: vec4<f32>,
    // (Ours.) Under an overcast sky, how thin the sheet is here, 0 (thick, its base low) to 1.
    thin: f32,
};

// How much of an overcast sky the coverage is: 0 a sky of cumulus, 1 a closed sheet.
fn cloud_overcast() -> f32 {
    return smoothstep(0.42, 0.66, clouds.wind.z);
}

// (Ours, by eye.) An overcast sky seen from below is not one flat grey: its base rolls (this many
// metres up where the sheet is thin) and the sheet thins in patches that let more light through
// (this much less coverage there). Both from the fourth channel of the weather (`weather_fragment`).
const OVERCAST_RELIEF: f32 = 450.0;
const OVERCAST_THIN: f32 = 0.34;

// A semicircle that rounds the clouds towards the top.
fn cloud_shape_altering(fraction: vec4<f32>, bias: vec4<f32>) -> vec4<f32> {
    let biased = pow(fraction, bias);
    let x = clamp(biased * 2.0 - 1.0, vec4<f32>(-1.0), vec4<f32>(1.0));
    return 1.0 - x * x;
}

// The storm towers as weather (ours): 1 in the middle of each, 0 from its rim outwards. A tower
// narrows as it climbs and spreads into an anvil under its top.
fn cloud_towers(xz: vec2<f32>, height: f32) -> f32 {
    var most = 0.0;
    let base = clouds.min_heights.w;
    for (var i = 0; i < i32(clouds.origin.w); i++) {
        let tower = clouds.towers[i];
        let up = (height - base) / max(tower.w - base, 1.0);
        let anvil = smoothstep(0.7, 0.82, up);
        let radius = tower.z * mix(1.0, 0.7, smoothstep(0.0, 0.35, up)) * (1.0 + 1.5 * anvil);
        most = max(most, (1.0 - smoothstep(0.25, 1.0, length(xz - tower.xy) / radius)) * step(up, 1.0));
    }
    return most;
}

// The weather over a place (`xz`) at a height: how much each layer has there before its shape.
fn cloud_weather_at(xz: vec2<f32>, height: f32, mip: f32) -> CloudWeather {
    var weather: CloudWeather;
    var local = textureSampleLevel(cloud_weather, cloud_repeat, (xz - clouds.wind.xy) / WEATHER_TILE, mip);
    let overcast = cloud_overcast();
    weather.thin = local.a;
    // The bases of the two layers of cumulus, higher where the sheet is thin.
    let lift = vec4<f32>(vec2<f32>(overcast * OVERCAST_RELIEF * local.a), 0.0, 0.0);
    let bases = clouds.min_heights + min(lift, 0.6 * (clouds.max_heights - clouds.min_heights));
    weather.fraction = saturate((vec4<f32>(height) - bases) / max(clouds.max_heights - bases, vec4<f32>(0.001)));
    let towers = cloud_towers(xz, height);
    local.a = towers;
    local = pow(local, clouds.weather_exponents) * cloud_layers;
    let height_scale = cloud_shape_altering(weather.fraction, clouds.shape_biases);
    // How much of the weather shows: what of it is over a level that the coverage lowers, most
    // in the middle of the layer's height, and softly, over a width (the threshold of Schneider's
    // "The real-time volumetric cloudscapes of Horizon Zero Dawn"; ours, not theirs, which is
    // after Skybolt's: the level and the width are set to draw the same curve from their
    // coverage and filter width). A storm tower is all there whatever the sky round it, and only
    // where it stands.
    let sharp = 1.0 - clouds.coverage_widths;
    let cover = clouds.wind.z * (1.0 - overcast * OVERCAST_THIN * smoothstep(0.45, 0.95, local.a));
    let level = 1.0 - vec4<f32>(cover, cover, clouds.wind.z, 0.8) * height_scale / sharp;
    weather.density = saturate((local - level) * sharp / clouds.coverage_widths);
    weather.density.w *= smoothstep(0.0, 0.15, towers);
    return weather;
}

fn cloud_layer_density(fraction: vec4<f32>) -> vec4<f32> {
    return clouds.profile_exp * exp(clouds.profile_exponent * fraction) + clouds.profile_linear * fraction + clouds.profile_constant;
}

struct CloudMedia {
    // The share of each layer in it.
    weight: vec4<f32>,
    scattering: f32,
    extinction: f32,
};

// The cloud at `p` (x, z its place, y its altitude): the weather carved by the shape and its detail.
fn cloud_media(weather: CloudWeather, p: vec3<f32>, mip: f32, jitter: f32) -> CloudMedia {
    var density = weather.density;
    let carried = p - vec3<f32>(clouds.wind.x, 0.0, clouds.wind.y);
    let evolution = vec3<f32>(0.0, -clouds.wind.w, 0.0);
    var turbulence = vec3<f32>(0.0);
    if CLOUD_TURBULENCE {
        // Strongest at the bottom of a cloud.
        turbulence = TURBULENCE_DISPLACEMENT * (textureSampleLevel(cloud_turbulence, cloud_repeat, carried.xz / TURBULENCE_TILE, 0.0).rgb * 2.0 - 1.0) * dot(density, remap_clamped(weather.fraction, vec4<f32>(0.3), vec4<f32>(0.0)));
    }
    let shape = textureSampleLevel(cloud_shape, cloud_repeat, (carried + evolution + turbulence).xzy / SHAPE_TILE, 0.0).r;
    cloud_shape_here = shape;
    density = remap_clamped(density, vec4<f32>(1.0 - shape) * clouds.shape_amounts, vec4<f32>(1.0));
    if CLOUD_SHAPE_DETAIL && mip * 0.5 + (jitter - 0.5) * 0.5 < 0.5 {
        let detail = textureSampleLevel(cloud_detail, cloud_repeat, (carried + turbulence).xzy / DETAIL_TILE, 0.0).r;
        // Fluffy at the top and wispy at the bottom.
        var modifier = mix(vec4<f32>(pow(detail, 6.0)), vec4<f32>(1.0 - detail), remap_clamped(weather.fraction, vec4<f32>(0.2), vec4<f32>(0.4)));
        modifier = modifier * clouds.detail_amounts;
        density = remap_clamped(density * 2.0, modifier * 0.5, vec4<f32>(1.0));
    }
    density = saturate(density * clouds.density_scales * cloud_layer_density(weather.fraction));
    var media: CloudMedia;
    let sum = density.x + density.y + density.z + density.w;
    media.weight = density / max(sum, 1e-9);
    // The scattering coefficient is 1 and the absorption 0.
    media.scattering = sum;
    media.extinction = sum;
    return media;
}

// The distances along a ray from `altitude` at which it crosses the level `level` on a round
// world: `rise` is the vertical part of its unit direction. (first, second), negative if never.
fn cloud_level(altitude: f32, rise: f32, level: f32) -> vec2<f32> {
    let a = (1.0 - rise * rise) / (2.0 * EARTH);
    let c = altitude - level;
    let disc = rise * rise - 4.0 * a * c;
    if disc < 0.0 {
        return vec2<f32>(-1.0);
    }
    let s = sqrt(disc);
    if a < 1e-12 {
        return vec2<f32>(-c / rise, -1.0);
    }
    return vec2<f32>((-rise - s) / (2.0 * a), (-rise + s) / (2.0 * a));
}

// A point `t` along the ray `v` from the eye: its place and altitude.
fn cloud_point(v: vec3<f32>, t: f32) -> vec3<f32> {
    let flat = v.xz * t;
    return vec3<f32>(clouds.origin.x + flat.x, clouds.origin.z + v.y * t + dot(flat, flat) / (2.0 * EARTH), clouds.origin.y + flat.y);
}

// ---- the march

fn cloud_hg(g: vec2<f32>, c: f32) -> vec2<f32> {
    let g2 = g * g;
    return (1.0 - g2) / max(vec2<f32>(1e-7), pow(1.0 + g2 - 2.0 * g * c, vec2<f32>(1.5))) / (4.0 * CLOUD_PI);
}

// Two lobes, forward and back; `attenuation` rounds them for the light scattered more than once.
fn cloud_phase(c: f32, attenuation: f32) -> f32 {
    return dot(cloud_hg(vec2<f32>(0.7, -0.2) * attenuation, c), vec2<f32>(0.5, 0.5));
}

struct CloudDepth {
    depth: f32,
    // How far the last sample was.
    reached: f32,
};

// The optical depth from `origin` along `direction`, in a few growing steps.
fn cloud_march_depth(origin: vec3<f32>, direction: vec3<f32>, most: i32, mip: f32, jitter: f32) -> CloudDepth {
    let count = i32(max(0.0, mix(f32(most + 1), 1.0, mip) - jitter));
    if count <= 0 {
        // Their fudge for the mean optical depth far off.
        return CloudDepth(0.5, 0.0);
    }
    var step = CLOUD_SECONDARY_STEP / f32(count);
    var next = step * jitter;
    var out = CloudDepth(0.0, 0.0);
    for (var i = 0; i < min(count, 8); i++) {
        out.reached = next;
        let p = origin + direction * next;
        let weather = cloud_weather_at(p.xz, p.y, mip);
        let media = cloud_media(weather, p, mip, jitter);
        out.depth += media.extinction * step;
        next += step;
        step *= CLOUD_SECONDARY_SCALE;
    }
    return out;
}

// What the sky's light on the base of an overcast sheet is times, where it is thick and where it thins.
const OVERCAST_THICK: vec3<f32> = vec3<f32>(0.34, 0.42, 0.56);
const OVERCAST_BREAK: vec3<f32> = vec3<f32>(1.6, 1.68, 1.8);

// Ours, by eye: under a sky that is mostly cloud the light that reaches a cloud from all round is
// no longer the blue of the open sky but what the other clouds pass on, grey and a little brighter.
fn cloud_overcast_light(sky: vec3<f32>) -> vec3<f32> {
    return mix(sky, vec3<f32>(dot(sky, vec3<f32>(0.25, 0.6, 0.15))) * 1.5, cloud_overcast());
}

// (Ours, by eye.) How low the light is: 0 from 20 degrees up, 1 at the horizon. The hour of the
// gold: what the clouds do with it below is art direction, not physics.
fn cloud_low_sun() -> f32 {
    return 1.0 - smoothstep(0.03, 0.35, clouds.sun.y);
}

// A colour further from its grey by `more` (0: as it is), as bright as it was.
fn cloud_saturated(colour: vec3<f32>, more: f32) -> vec3<f32> {
    let grey = dot(colour, vec3<f32>(0.25, 0.6, 0.15));
    let vivid = max(vec3<f32>(grey) + (colour - grey) * (1.0 + more), vec3<f32>(0.0));
    return vivid * grey / max(dot(vivid, vec3<f32>(0.25, 0.6, 0.15)), 1e-9);
}

// The light of a low sun on a cloud: more orange than the air leaves it, and pink at the last.
const GOLDEN_SUN: f32 = 0.6;
const GOLDEN_PINK: vec3<f32> = vec3<f32>(1.08, 0.93, 0.96);
fn cloud_golden(sun: vec3<f32>) -> vec3<f32> {
    let low = cloud_low_sun();
    return cloud_saturated(sun, GOLDEN_SUN * low) * mix(vec3<f32>(1.0), GOLDEN_PINK, low * low);
}

// And the sky's on the side the sun does not reach: bluer and less of it, so that the shade is deep.
const GOLDEN_SKY: f32 = 0.6;
const GOLDEN_SKY_DIM: f32 = 0.3;
const GOLDEN_SHADE: vec3<f32> = vec3<f32>(0.7, 0.95, 1.2);
fn cloud_golden_sky(sky: vec3<f32>) -> vec3<f32> {
    let low = cloud_low_sun() * (1.0 - cloud_overcast());
    return cloud_saturated(sky, GOLDEN_SKY * low) * mix(vec3<f32>(1.0), GOLDEN_SHADE, low) * (1.0 - GOLDEN_SKY_DIM * low);
}

// The silver lining: the thin rim of a cloud before the sun passes its light on almost straight,
// far more than the two lobes of `cloud_phase` say. This much of a narrow third lobe, with a low sun
// and with a high one.
const SILVER_LINING: vec2<f32> = vec2<f32>(0.5, 0.15);
fn cloud_silver(depth: f32, c: f32) -> f32 {
    return mix(SILVER_LINING.y, SILVER_LINING.x, cloud_low_sun()) * cloud_hg(vec2<f32>(0.9, 0.0), c).x * exp(-1.5 * depth);
}

// The light scattered more than once, as octaves (Wrenninge, "Oz: the great and volumetric"):
// each reaches twice as deep, is half as bright and comes from more of all round.
fn cloud_multiple_scattering(depth: f32, c: f32) -> f32 {
    var coeffs = vec3<f32>(1.0);
    var scattering = 0.0;
    for (var i = 0; i < CLOUD_OCTAVES; i++) {
        scattering += coeffs.x * exp(-depth * coeffs.y) * cloud_phase(c, coeffs.z);
        coeffs *= 0.5;
    }
    return scattering;
}

struct CloudMarched {
    // The light gathered, and how much of what is behind the clouds hide, 0..1.
    colour: vec4<f32>,
    // How far along the ray the cloud that shows is, m from the eye; negative when there is none.
    front: f32,
    // Where the first of it is.
    first: f32,
    // (Ours.) How much the cloud nearer than `clouds.jitter.z` hides: the cloud right before an
    // eye that moves, which crosses the picture fast.
    near: f32,
};

// The part of the ray `v` between the lowest base and the highest top: (start, end), negative when none.
fn cloud_range(v: vec3<f32>) -> vec2<f32> {
    let eye = clouds.origin.z;
    let ground = cloud_level(eye, v.y, 0.0).x > 0.0;
    let low = cloud_level(eye, v.y, clouds.heights.x);
    let high = cloud_level(eye, v.y, clouds.heights.y);
    if eye < clouds.heights.x {
        if ground || low.y < 0.0 {
            return vec2<f32>(-1.0);
        }
        return vec2<f32>(low.y, min(high.y, CLOUD_MAX_RAY));
    }
    if eye < clouds.heights.y {
        return vec2<f32>(clouds.more.x, select(high.y, low.x, low.x > 0.0));
    }
    return vec2<f32>(high.x, select(high.y, low.x, low.x > 0.0));
}

// The clouds along the ray `v` (unit) from the eye, between the distances `range`. `jitter`: a
// number 0..1 of the pixel and the frame; `texels`: 2 to the level of detail the ray starts at.
fn cloud_march(v: vec3<f32>, range: vec2<f32>, jitter: f32, texels: f32) -> CloudMarched {
    var radiance = vec3<f32>(0.0);
    var through = 1.0;
    var weighted = 0.0;
    var weights = 0.0;
    var first = -1.0;
    var veiled = 0.0;
    let sun = clouds.sun.xyz;
    let toward = dot(sun, v);
    let most = range.y - range.x;
    var step = CLOUD_MIN_STEP + (CLOUD_STEP_SCALE - 1.0) * range.x;
    // (Theirs: "I don't understand why spatial aliasing remains unless doubling the jitter.")
    var t = step * jitter * 2.0;
    // The sky's light on the clouds, the same all along the ray, and the sun's and the sky's on the ground.
    let sky = cloud_golden_sky(cloud_overcast_light(cloud_sky_light())) * (2.0 * CLOUD_PI * CLOUD_PI);
    let ground = 0.3 / CLOUD_PI * (sky + (1.0 - clouds.wind.z) * cloud_golden(cloud_sun_light(0.0)) * CLOUD_PI * max(sun.y, 0.0));
    let overcast = cloud_overcast();
    for (var i = 0; i < CLOUD_STEPS; i++) {
        if t > most {
            break;
        }
        let p = cloud_point(v, range.x + t);
        let mip = log2(max(1.0, texels + t * 1e-5));
        if cloud_in_gap(p.y) {
            step *= CLOUD_STEP_SCALE;
            t += mix(step, CLOUD_MAX_STEP, min(1.0, mip));
            continue;
        }
        let weather = cloud_weather_at(p.xz, p.y, mip);
        if !any(weather.density > vec4<f32>(CLOUD_MIN_DENSITY)) {
            // Longer steps in empty space.
            step *= CLOUD_STEP_SCALE;
            t += mix(step, CLOUD_MAX_STEP, min(1.0, mip));
            continue;
        }
        let media = cloud_media(weather, p, mip, jitter);
        if media.extinction > CLOUD_MIN_EXTINCTION {
            // The optical depth to the sun: marched a little way for the detail the shadow map
            // lacks, and the map's for what is farther.
            let near = cloud_march_depth(p, sun, CLOUD_SUN_STEPS, mip, jitter);
            var depth = near.depth;
            if p.y < clouds.heights.z {
                depth += cloud_shadow_depth(p, near.reached) * clouds.light.w;
            }
            var light = cloud_golden(cloud_sun_light(p.y)) * (CLOUD_PI * clouds.more.w) * (cloud_multiple_scattering(depth, toward) + cloud_silver(depth, toward));
            if CLOUD_GROUND_STEPS > 0 && p.y < clouds.heights.z && mip < 0.5 {
                // The light the ground sends back up, through the cloud under the point.
                let under = cloud_march_depth(p, vec3<f32>(0.0, -1.0, 0.0), CLOUD_GROUND_STEPS, mip, jitter);
                light += ground * exp(-under.depth) / (4.0 * CLOUD_PI) * clouds.light.y;
            }
            // A crude gradient of the sky's light: better than none in the shade.
            let gradient = dot(weather.fraction * 0.5 + 0.5, media.weight);
            // (Ours, by eye: under an overcast sky, where this is all the light there is, the
            // lumps of the shape are lighter and the hollows between them darker, or the base is one grey.)
            // And the sheet is darker and bluer where it is thick, lighter where it thins.
            let lumps = mix(vec3<f32>(1.0), (0.35 + 1.3 * cloud_shape_here) * mix(OVERCAST_THICK, OVERCAST_BREAK, smoothstep(0.2, 0.9, weather.thin)), overcast);
            // (And a storm cloud is so deep that little of the sky's light is left in it, and none
            // of its blue: its sides are grey and its base dark.)
            let deep = mix(sky, vec3<f32>(dot(sky, vec3<f32>(0.1, 0.24, 0.06))), media.weight.w);
            light += deep / (4.0 * CLOUD_PI) * gradient * lumps * clouds.light.x;
            light *= media.scattering;
            // Beer-powder.
            light *= 1.0 - 0.8 * exp(-media.extinction * 150.0);
            // The scattered light integrated over the step with its energy kept (Hillaire 2016, 5.6.3).
            let kept = exp(-media.extinction * step);
            radiance += through * (light - light * kept) / max(media.extinction, 1e-7);
            through *= kept;
            // The mean distance of the cloud, by what each part of it lets through (5.9.1).
            weighted += t * through;
            weights += through;
            if first < 0.0 {
                first = range.x + t;
            }
            if range.x + t < clouds.jitter.z {
                veiled = 1.0 - through;
            }
        }
        if through <= CLOUD_MIN_TRANSMITTANCE {
            break;
        }
        // Shorter steps once in the cloud.
        step *= CLOUD_STEP_SCALE;
        t += step;
    }
    var out: CloudMarched;
    out.front = select(-1.0, range.x + weighted / max(weights, 1e-9), weights > 0.0);
    out.first = select(1e6, first, first >= 0.0);
    out.near = veiled;
    out.colour = vec4<f32>(radiance, saturate((through - 1.0) / (CLOUD_MIN_TRANSMITTANCE - 1.0)));
    return out;
}

// How much of the ray `v` between the distances `range` lies in the shade of the clouds, m.
fn cloud_shade_length(v: vec3<f32>, range: vec2<f32>, jitter: f32) -> f32 {
    var shaded = 0.0;
    let most = range.y - range.x;
    var step = CLOUD_SHAFT_STEP;
    var t = step * jitter;
    for (var i = 0; i < CLOUD_SHAFT_STEPS; i++) {
        if t > most {
            break;
        }
        let depth = cloud_shadow_depth(cloud_point(v, range.x + t), 0.0);
        shaded += (1.0 - exp(-depth)) * step;
        step *= CLOUD_STEP_SCALE;
        t += step;
    }
    return shaded;
}

// The part of the ray `v` under the top of the shade, as far as `reach`: (start, end), negative when none.
fn cloud_shade_range(v: vec3<f32>, reach: f32) -> vec2<f32> {
    let eye = clouds.origin.z;
    let ground = cloud_level(eye, v.y, 0.0);
    let top = cloud_level(eye, v.y, clouds.heights.z);
    var range = vec2<f32>(clouds.more.x, select(top.y, ground.x, ground.x > 0.0));
    if eye >= clouds.heights.z {
        range.x = top.x;
    }
    range.y = min(min(range.y, reach), range.x + CLOUD_SHAFT_RAY);
    return range;
}

const GOLDEN_HAZE: f32 = 0.7;

// The haze under the clouds along the ray `v` as far as `reach`, less where the ray is in their
// shade (`shaded`, m): its light, and how much of what is behind it hides. Its density falls
// with the height by an exponential, whose optical depth along a ray has a closed form
// (Quilez, "Better fog").
fn cloud_haze(v: vec3<f32>, reach: f32, shaded: f32) -> vec4<f32> {
    let eye = clouds.origin.z;
    let modulation = saturate((clouds.wind.z - 0.2) / 0.2) * clouds.light.z;
    let exponent = 0.001;
    // (Ours, by eye: more of it with a low sun, whose light it takes the colour of and whose
    // shafts it shows.)
    let density = modulation * 0.00003 * (1.0 + GOLDEN_HAZE * cloud_low_sun()) * exp(-max(eye, 0.0) * exponent);
    if density < 1e-9 {
        return vec4<f32>(0.0);
    }
    let angle = max(v.y, 1e-5);
    let slope = angle * exponent;
    let column = density / exponent / angle;
    let whole = 1.0 - exp(-reach * slope);
    let dark = 1.0 - exp(-min(reach, shaded) * slope);
    let opacity = saturate(1.0 - exp(-whole * column));
    let lit = saturate(1.0 - exp(-max((whole - dark) * column, 0.0)));
    var inscatter = cloud_golden(cloud_sun_light(eye)) * CLOUD_PI * cloud_phase(dot(clouds.sun.xyz, v), 1.0) * lit;
    inscatter += cloud_overcast_light(cloud_sky_light()) * (2.0 * CLOUD_PI * CLOUD_PI) / (4.0 * CLOUD_PI) * clouds.light.x * opacity;
    inscatter *= 0.9 / (0.5 + 0.9);
    return vec4<f32>(inscatter, opacity);
}

// ---- the rain (ours)

// The rain that hangs under the storm towers and the heaviest cumulus: grey curtains from the
// base towards the ground, in streaks, leaning as the wind carries them, dark in the middle and
// bright where the sun comes through them from behind. A cheat from end to end: it is not
// marched as the clouds are but in a few long steps, its streaks are stripes round the eye (by
// the bearing of the cloud each drop fell from) and not places of the world, so they are the same
// however a step falls, and its light is a grey of the sky's and a glow of the sun's.
const RAIN_STEPS: i32 = 48;
const RAIN_STEP: f32 = 200.0;
const RAIN_STEP_SCALE: f32 = 1.08;
// How far into the cloud over its base it starts, m, and what it takes of the light, 1/m, where it falls hardest.
const RAIN_INTO: f32 = 200.0;
const RAIN_EXTINCTION: f32 = 0.0011;
// What it gives back of the sky's light: little, and grey, so that it is darker than the sky behind it.
const RAIN_GREY: f32 = 0.3;

struct CloudRain {
    // Its light towards the eye, and how much of what is behind it hides.
    colour: vec4<f32>,
    // How far it is, m.
    front: f32,
};

// How much rain there is at `p`, 0..1, for a step of the march `step` long: the weather is read
// no finer than the step, or a shower narrower than it would be there in one frame and not in the next.
fn cloud_rain_at(p: vec3<f32>, step: f32) -> f32 {
    let base = clouds.heights.x;
    let fall = max(base - p.y, 0.0);
    // The place of the cloud it fell from: the further down, the further the wind has carried it.
    let source = p.xz + clouds.rain.xy * fall * (1.0 + 0.5 * fall / max(base, 1.0));
    var storm = 0.0;
    for (var i = 0; i < i32(clouds.origin.w); i++) {
        let tower = clouds.towers[i];
        storm = max(storm, 1.0 - smoothstep(0.35, 1.05, length(source - tower.xy) / tower.z));
    }
    var shower = 0.0;
    if clouds.rain.z > 0.0 {
        shower = smoothstep(0.6, 0.95, textureSampleLevel(cloud_weather, cloud_repeat, (source - clouds.wind.xy) / WEATHER_TILE, log2(max(step / 190.0, 2.0))).g);
    }
    let falls = max(storm * clouds.rain.w, shower * clouds.rain.z);
    if falls <= 0.0 {
        return 0.0;
    }
    // The streaks: stripes by the bearing of that cloud from the eye, others every few kilometres out.
    let away = source - clouds.origin.xy;
    let out = length(away);
    let bearing = atan2(away.x, away.y) / (2.0 * CLOUD_PI);
    let fine = textureSampleLevel(cloud_turbulence, cloud_repeat, vec2<f32>(bearing * 13.0, out / 30000.0), 0.0).r;
    let coarse = textureSampleLevel(cloud_turbulence, cloud_repeat, vec2<f32>(bearing * 4.0 + 0.37, out / 50000.0 + 0.61), 0.0).gb;
    // (Right over the eye every bearing meets: no stripes there.)
    let striped = smoothstep(800.0, 4000.0, out);
    let streak = mix(1.0, 0.45 + 1.1 * smoothstep(0.3, 0.7, 0.5 * fine + 0.5 * coarse.x), striped);
    // How far down each streak comes: to the ground under a storm, part of the way elsewhere (virga).
    let bottom = base * mix(mix(0.15, 0.8, coarse.y), mix(-0.6, 0.35, coarse.y), storm * clouds.rain.w);
    let hangs = smoothstep(bottom, bottom + 0.3 * base, p.y) * (1.0 - smoothstep(base, base + RAIN_INTO, p.y));
    return falls * streak * hangs;
}

// The rain along the ray `v` (unit) from the eye as far as `reach`, under the clouds' base.
fn cloud_rain(v: vec3<f32>, reach: f32, jitter: f32) -> CloudRain {
    var out = CloudRain(vec4<f32>(0.0), 0.0);
    let eye = clouds.origin.z;
    let top = clouds.heights.x + RAIN_INTO;
    let ground = cloud_level(eye, v.y, 0.0).x;
    let level = cloud_level(eye, v.y, top);
    var range = vec2<f32>(clouds.more.x, level.y);
    if eye >= top {
        if level.x <= 0.0 {
            return out;
        }
        range.x = level.x;
    }
    if ground > 0.0 {
        range.y = ground;
    }
    range.y = min(range.y, reach);
    if range.y <= range.x {
        return out;
    }
    let sun = clouds.sun.xyz;
    // Drops pass the sun's light on forwards, in a narrow glow, and a little of it all round.
    let phase = dot(cloud_hg(vec2<f32>(0.85, 0.2), dot(sun, v)), vec2<f32>(0.35, 0.65));
    let sunlight = cloud_golden(cloud_sun_light(0.5 * clouds.heights.x)) * (CLOUD_PI * phase);
    let blue = cloud_overcast_light(cloud_sky_light());
    let sky = mix(blue, vec3<f32>(dot(blue, vec3<f32>(0.25, 0.6, 0.15))), 0.7) * (0.5 * CLOUD_PI * RAIN_GREY);
    var radiance = vec3<f32>(0.0);
    var through = 1.0;
    var weighted = 0.0;
    var weights = 0.0;
    var step = RAIN_STEP + (RAIN_STEP_SCALE - 1.0) * range.x;
    var t = range.x + step * jitter;
    for (var i = 0; i < RAIN_STEPS; i++) {
        if t > range.y || through < 0.02 {
            break;
        }
        let p = cloud_point(v, t);
        let rain = cloud_rain_at(p, step);
        if rain > 0.001 {
            let extinction = rain * RAIN_EXTINCTION;
            // Under its own cloud the rain is in the shade: grey, and darker the harder it falls.
            let shade = exp(-0.5 * cloud_shadow_depth(p, 0.0));
            let light = sunlight * shade + sky * (1.0 - 0.5 * rain);
            let kept = exp(-extinction * step);
            radiance += through * light * (1.0 - kept);
            through *= kept;
            weighted += t * through;
            weights += through;
        }
        step *= RAIN_STEP_SCALE;
        t += step;
    }
    out.colour = vec4<f32>(radiance, 1.0 - through);
    out.front = select(0.0, weighted / max(weights, 1e-9), weights > 0.0);
    return out;
}
