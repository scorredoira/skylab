// How a car is drawn (`look.rs`): its materials, each in its own way. Appended to the world's
// shaders (`world_render::shader_source`), whose `frame`, sky, clouds, shadows and haze it uses, so
// the car is lit by the same sun and sky as the ground it stands on and mirrors them.
//
// - Paint: a metallic base (colour, flakes that glint close up, a broad highlight) under a clear
//   coat (a mirror with Fresnel's strength: the sky with its clouds above, the horizon's dark band,
//   the road below, the sun as a small hot highlight).
// - Glass: the same mirror over a dark tint that lets the inside show through.
// - Lamps: lenses that shine by themselves, as bright as their state says (they feed the bloom).
// - Rubber, trim, alloy, chrome, carbon, grilles, brake discs that glow when hot.
// - The inside: lit by the sky through the glass; the dials' marks and needles drawn here.

struct CarDraw {
    model: mat4x4<f32>,
    base: vec4<f32>,     // rgb = base colour, a = opacity of a see-through part
    surface: vec4<f32>,  // x = kind (CAR_*), y = roughness, z = metallic, w = -1 when drawn mirrored
    glow: vec4<f32>,     // rgb = the light it gives off, w = what the kind wants (a needle's place, a disc's heat)
    state: vec4<f32>,    // x = 1 seen from inside the car, y = daylight 0..1, z = the floodlights' light where the car is, w = 1 for a part of the cabin
    more: vec4<f32>,     // per kind: x = a dial's red line, a wheel's blur; y = 1 for a lamp of cells (the brake lights); a mirror: z = half the asphalt's width; glass: z = seconds, w = how wet 0..1
    spare: vec4<f32>,    // x = how much of the sky the clouds cover, 0..1; yzw = the way to the floodlights' brightest tower (a mirror: the car's metres across the circuit, the circuit's heading and curvature; glass: y = the car's speed, m/s)
};

@group(1) @binding(0) var<uniform> car: CarDraw;
// The scene as the frame's first pass left it, for the glass with drops (`car_wet_glass_fragment`).
@group(2) @binding(8) var car_scene: texture_2d<f32>;
@group(2) @binding(10) var car_scene_sampler: sampler;

const CAR_PLAIN: i32 = 0;
const CAR_PAINT: i32 = 1;
const CAR_GLASS: i32 = 2;
const CAR_LAMP: i32 = 3;
const CAR_RUBBER: i32 = 4;
const CAR_DIAL: i32 = 5;
const CAR_DISC: i32 = 6;
const CAR_INSIDE: i32 = 7;
const CAR_GRILLE: i32 = 8;
const CAR_CARBON: i32 = 9;
const CAR_MIRROR: i32 = 10;
const CAR_BLUR: i32 = 11;
const CAR_PLATE: i32 = 12;

// Where the number plate's middle is in the body's axes (z down), and its letters: SKYLAB, each
// seven rows of five dots, the top row first, the leftmost dot the highest bit.
const CAR_PLATE_Z: f32 = -0.145;
const CAR_PLATE_ROWS = array<u32, 42>(
    15u, 16u, 16u, 14u, 1u, 1u, 30u,
    17u, 18u, 20u, 24u, 20u, 18u, 17u,
    17u, 17u, 10u, 4u, 4u, 4u, 4u,
    16u, 16u, 16u, 16u, 16u, 16u, 31u,
    14u, 17u, 17u, 31u, 17u, 17u, 17u,
    30u, 17u, 17u, 30u, 17u, 17u, 30u,
);

struct CarIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,   // r = how open to the sky (ambient occlusion)
};

struct CarOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) local: vec3<f32>,
    @location(3) open: f32,
};

@vertex
fn car_vertex(in: CarIn) -> CarOut {
    var o: CarOut;
    let rel = car.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.normal = (car.model * vec4<f32>(in.normal, 0.0)).xyz;
    o.local = in.position;
    o.open = in.color.r;
    return o;
}

fn car_hash3(p: vec3<f32>) -> vec3<f32> {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.xxy + q.yxx) * q.zyx);
}

// The sun (or the moon) where the car is: through the clouds and the relief.
fn car_sun(rel: vec3<f32>) -> vec3<f32> {
    return light_at(rel) * cloud_shadow(rel) * terrain_shadow(rel);
}

// The sky a polished surface of the car mirrors along `up`: as the water mirrors it
// (`water_sky`), but the clouds as they look from below by day, white where the sun lights their
// sides and grey under their middles, with firmer edges than their shade on the ground has: on a
// bonnet at midday it is their shapes that say the paint is a mirror. (The volumetric clouds,
// from the map of their shade drawn beside the heights; other clouds, as the water has them.)
fn car_sky(up: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    let rise = frame.misc.y - p.y;
    if frame.cam_wrap.w < 0.5 || rise <= 0.0 || frame.hmap.w < 0.5 {
        return water_sky(up, p, 0.02);
    }
    var sky = sky_color(up);
    let t = rise / up.y;
    let uv = (p.xz + up.xz * t - frame.hmap.xy) / (2.0 * frame.hmap.z) + 0.5;
    let off = max(abs(uv.x - 0.5), abs(uv.y - 0.5));
    let texel = 0.8 / f32(textureDimensions(height_map).x);
    var cover = textureSampleLevel(height_map, height_sampler, uv, 0.0).g * 0.4;
    cover += textureSampleLevel(height_map, height_sampler, uv + vec2<f32>(texel, texel), 0.0).g * 0.15;
    cover += textureSampleLevel(height_map, height_sampler, uv + vec2<f32>(-texel, texel), 0.0).g * 0.15;
    cover += textureSampleLevel(height_map, height_sampler, uv + vec2<f32>(texel, -texel), 0.0).g * 0.15;
    cover += textureSampleLevel(height_map, height_sampler, uv + vec2<f32>(-texel, -texel), 0.0).g * 0.15;
    let mapped = 1.0 - smoothstep(0.38, 0.48, off);
    cover = smoothstep(0.05, 0.6, cover) * mapped;
    // Beyond the map (low over the horizon, where a bonnet looks) clouds of the same cover, made
    // of noise: nobody can tell which clouds a bonnet mirrors.
    let xz = p.xz + up.xz * t;
    let reach = length(xz);
    if mapped < 1.0 && reach < 60000.0 && car.spare.x > 0.01 {
        let th = mix(0.72, 0.30, car.spare.x);
        let far = smoothstep(th, th + 0.07, cloud_noise(cloud_coord(xz * 2.4), 50000.0)) * (1.0 - smoothstep(25000.0, 60000.0, reach));
        cover = max(cover, far * (1.0 - mapped));
    }
    if cover > 0.003 {
        let at = p + up * min(t, 20000.0);
        let above = sky_light(vec3<f32>(0.0, 1.0, 0.0), at);
        let base = light_at(at) * 0.30 + sky_light(vec3<f32>(0.0, -1.0, 0.0), at) * 0.9 + above * 0.25;
        let side = light_at(at) * (0.25 + 0.6 * max(frame.sun_dir.y, 0.0)) + above * 0.7;
        let lit = mix(side, base, 0.65 * smoothstep(0.7, 1.0, cover));
        let haze = haze_at(up * t);
        sky = mix(sky, lit * haze.through + haze.light, cover);
    }
    return sky;
}

// What a mirror on the car shows along `r`: the sky and its clouds above, a dark band of land and
// trees at the horizon, the road below. `rough` blurs it.
fn car_mirrored(r: vec3<f32>, rel: vec3<f32>, rough: f32, sun: vec3<f32>) -> vec3<f32> {
    let blur = 0.012 + rough * 0.6;
    var sky: vec3<f32>;
    let up = normalize(vec3<f32>(r.x, max(r.y, 0.03), r.z));
    if rough < 0.05 {
        sky = car_sky(up, rel);
    } else if rough < 0.3 {
        sky = water_sky(up, rel, 0.02 + rough);
    } else {
        sky = sky_seen(up);
    }
    sky = mix(sky, sky_light(up, rel), saturate(rough * 1.2));
    let above = vec3<f32>(0.0, 1.0, 0.0);
    let day = sun * max(frame.sun_dir.y, 0.0);
    let ambient = sky_light(above, rel);
    // The land round a circuit: grass and trees, far enough to be hazed a little; below it the
    // ground beside the car, its far part paler with the haze and the glare off it.
    let land = vec3<f32>(0.035, 0.055, 0.022) * (day * 0.7 + ambient);
    let road = vec3<f32>(0.058, 0.057, 0.056) * (day + ambient) * (1.0 + 0.6 * smoothstep(-0.35, -0.02, r.y));
    var seen = mix(sky, land, smoothstep(0.075 + blur, 0.02 - blur, r.y));
    seen = mix(seen, road, smoothstep(blur, -0.03 - blur, r.y));
    return seen;
}

// The GGX distribution of microfacets for a roughness `a` (alpha).
fn car_ggx(ndh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / (3.14159265 * d * d);
}

fn car_fresnel(f0: vec3<f32>, c: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - saturate(c), 5.0);
}

// A highlight of the sun on a surface of roughness `rough` and reflectance `f0`.
fn car_highlight(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32, f0: vec3<f32>) -> vec3<f32> {
    let h = normalize(l + v);
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 0.05);
    let a = max(rough * rough, 0.0025);
    let k = a * 0.5;
    let vis = 0.25 / ((ndl * (1.0 - k) + k) * (ndv * (1.0 - k) + k) + 1e-4);
    return car_fresnel(f0, dot(v, h)) * car_ggx(max(dot(n, h), 0.0), a) * vis * ndl;
}

// The light inside the cabin: the sky's through the glass, dimmer the less of it a point sees.
fn car_inside_light(n: vec3<f32>, rel: vec3<f32>, open: f32, sun: vec3<f32>) -> vec3<f32> {
    let above = sky_light(vec3<f32>(0.0, 1.0, 0.0), rel);
    let through = sky_light(normalize(n + vec3<f32>(0.0, 0.6, 0.0)), rel);
    // A little of the sun reaches in through the glass where a surface faces it.
    let sunlit = sun * max(dot(n, frame.sun_dir.xyz), 0.0) * 0.3;
    // What comes in has lost most of the sky's blue on the way round the cabin.
    var light = above * 2.0 + through * 2.4;
    light = mix(light, vec3<f32>(luma(light)), 0.65);
    // By night, what the circuit's floodlights send in through the glass.
    light += vec3<f32>(1.0, 0.97, 0.90) * car.state.z * (0.22 + 0.4 * max(n.y, 0.0));
    return (light + sunlit) * (0.3 + 0.7 * open);
}

// The glow of the lit dials on what is round them by night (the wheel, the hands, the hood over
// them): from where they are, falling off with the square of the distance.
fn car_cluster_light(rel: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let night = 1.0 - car.state.y;
    if night <= 0.0 {
        return vec3<f32>(0.0);
    }
    let d = frame.body_pos.xyz + frame.body_fwd.xyz * 0.40 - frame.body_right.xyz * 0.37 - frame.body_down.xyz * 0.35 - rel;
    let d2 = dot(d, d);
    return vec3<f32>(0.62, 0.78, 1.0) * night * 0.009 * (0.3 + 0.7 * max(dot(n, d) * inverseSqrt(d2), 0.0)) / (d2 + 0.004);
}

// The lining of the shell seen from within, by where it is in the body's axes: the roof's and
// the pillars' grey cloth; under the belt line the door cards, of dark leather with an insert of
// red cloth between two seams and a pull of brushed aluminium; carpet under them.
fn car_lining(local: vec3<f32>) -> vec3<f32> {
    let h = 0.46 - local.z;
    var c = vec3<f32>(0.055, 0.055, 0.06) * (0.95 + 0.1 * vnoise(local.xy * 600.0 + local.z * 350.0, 4096));
    if h < 0.93 {
        c = vec3<f32>(0.03, 0.03, 0.033);
        let between = step(-0.72, local.x) * step(local.x, 0.72);
        let insert = smoothstep(0.585, 0.59, h) * (1.0 - smoothstep(0.765, 0.77, h)) * between;
        c = mix(c, vec3<f32>(0.19, 0.022, 0.02) * (0.85 + 0.3 * vnoise(local.xz * 400.0, 4096)), insert);
        let seam = (1.0 - smoothstep(0.0012, 0.0025, min(abs(h - 0.78), abs(h - 0.575)))) * step(0.45, fract(local.x * 95.0)) * between;
        c = mix(c, vec3<f32>(0.34, 0.32, 0.28), seam);
        let grip = (1.0 - smoothstep(0.011, 0.013, abs(h - 0.69))) * step(-0.22, local.x) * step(local.x, 0.3);
        c = mix(c, vec3<f32>(0.3, 0.3, 0.31) * (0.8 + 0.4 * vnoise(vec2<f32>(local.x * 30.0, h * 900.0), 4096)), grip);
        c = mix(c, vec3<f32>(0.014), 1.0 - smoothstep(0.40, 0.43, h));
    }
    return c;
}

// What is drawn on a part of the inside over its colour (`car.more.y`): the dashboard's seam
// along its top and the strip of aluminium across its face; a seat's quilted middle with a red
// stripe down it; the stripes of the suit's sleeve.
fn car_inside_colour(base: vec3<f32>, local: vec3<f32>) -> vec3<f32> {
    let what = i32(car.more.y + 0.5);
    let h = 0.46 - local.z;
    var c = base;
    if what == 1 {
        c *= 0.88 + 0.24 * vnoise(local.xy * 700.0 + h * 300.0, 4096);
        let seam = (1.0 - smoothstep(0.001, 0.0022, abs(local.x - 0.47))) * step(0.5, fract(local.y * 95.0)) * step(0.84, h);
        c = mix(c, vec3<f32>(0.3, 0.05, 0.04), seam);
        let strip = (1.0 - smoothstep(0.007, 0.009, abs(h - 0.742))) * (1.0 - smoothstep(0.415, 0.43, local.x));
        c = mix(c, vec3<f32>(0.3, 0.3, 0.31) * (0.8 + 0.4 * vnoise(vec2<f32>(local.y * 40.0, h * 1200.0), 4096)), strip);
    } else if what == 2 {
        let across = abs(abs(local.y) - 0.37);
        let middle = 1.0 - smoothstep(0.13, 0.14, across);
        let quilt = min(abs(fract((local.y + h + local.x) * 11.0) - 0.5), abs(fract((local.y - h - local.x) * 11.0) - 0.5));
        c = mix(c, c * 1.5 * (0.55 + 0.45 * smoothstep(0.0, 0.07, quilt)), middle);
        c = mix(c, vec3<f32>(0.22, 0.02, 0.018), 1.0 - smoothstep(0.016, 0.02, across));
    } else if what == 3 {
        // Along the forearm (a metre of it in its own axes): two red bands near the cuff.
        c = mix(c, vec3<f32>(0.3, 0.025, 0.02), step(0.08, local.x) * step(local.x, 0.2) * step(0.5, fract(local.x * 14.0)));
    }
    return c;
}

// A dial's face: marks round three quarters of a circle, a red line, the needle. `p`: across and
// up from its centre in radii. Returns rgb = what it gives off (per unit of its light), a = how
// much of the face is mark or needle.
fn car_dial(p: vec2<f32>, value: f32, red_from: f32, gear: i32) -> vec4<f32> {
    let r = length(p);
    let aa = fwidth(r) * 1.2;
    let sweep = 4.712389;
    let angle = atan2(p.x, p.y);
    let t = (angle + sweep * 0.5) / sweep;
    var mark = 0.0;
    var color = vec3<f32>(1.0);
    if t > -0.01 && t < 1.01 {
        let major = abs(fract(t * 8.0 + 0.5) - 0.5) / 8.0 * sweep * r;
        let minor = abs(fract(t * 40.0 + 0.5) - 0.5) / 40.0 * sweep * r;
        mark = (1.0 - smoothstep(0.016, 0.016 + aa, major)) * smoothstep(0.70 - aa, 0.70, r) * (1.0 - smoothstep(0.90, 0.90 + aa, r));
        mark = max(mark, (1.0 - smoothstep(0.007, 0.007 + aa, minor)) * smoothstep(0.80 - aa, 0.80, r) * (1.0 - smoothstep(0.90, 0.90 + aa, r)));
        if t > red_from {
            color = vec3<f32>(1.0, 0.08, 0.04);
            mark = max(mark, smoothstep(0.92 - aa, 0.92, r) * (1.0 - smoothstep(0.96, 0.96 + aa, r)));
        }
    }
    // A number at the long marks: the thousands of revs at each, 0 to 8, or (a red line beyond
    // the dial: the speed's) the km/h at every other, 0 to 320.
    let speed = red_from > 1.0;
    var k = clamp(i32(round(t * 8.0)), 0, 8);
    if speed {
        k = clamp(i32(round(t * 4.0)), 0, 4) * 2;
    }
    let at = (f32(k) / 8.0 - 0.5) * sweep;
    let number = select(k, k * 40, speed);
    let digits = select(select(3, 2, number < 100), 1, number < 10);
    let unit = select(0.20, 0.15, speed) / 16.0;
    let q = (p - 0.47 * vec2<f32>(sin(at), cos(at))) / unit;
    var tens = 1;
    for (var j = 0; j < digits; j++) {
        let x = q.x + (f32(j) - 0.5 * f32(digits - 1)) * 5.6;
        if abs(x) < 2.8 && abs(q.y) < 9.0 {
            mark = max(mark, digit(vec2<f32>(x, q.y + 8.0), (number / tens) % 10) * 0.9);
        }
        tens *= 10;
    }
    // The gear, large, under the middle of the revs' dial.
    if gear >= 1 {
        let g = (p - vec2<f32>(0.0, -0.52)) / (0.3 / 16.0);
        if abs(g.x) < 3.0 && abs(g.y) < 9.0 {
            let lit = digit(vec2<f32>(g.x, g.y + 8.0), gear);
            color = mix(color, vec3<f32>(1.0, 0.75, 0.3), lit);
            mark = max(mark, lit);
        }
    }
    // The needle, from just short of the centre to the marks.
    let a = (saturate(value) - 0.5) * sweep;
    let dir = vec2<f32>(sin(a), cos(a));
    let along = dot(p, dir);
    let off = abs(dot(p, vec2<f32>(dir.y, -dir.x)));
    let needle = (1.0 - smoothstep(0.022, 0.022 + aa, off)) * smoothstep(-0.12, -0.10, along) * (1.0 - smoothstep(0.86, 0.88, along));
    color = mix(color, vec3<f32>(1.0, 0.25, 0.04), needle);
    mark = max(mark, needle);
    return vec4<f32>(color, mark);
}

// The ink of the number plate at `p` (metres across, to the car's right, and up from its middle):
// six letters of dots and a line round its edge.
fn car_plate_ink(p: vec2<f32>) -> f32 {
    let dot = 0.062 / 7.0;
    let x = (p.x + 0.5 * (6.0 * 6.0 - 1.0) * dot) / dot;
    let y = (0.031 - p.y) / dot;
    var ink = 0.0;
    if x >= 0.0 && y >= 0.0 && y < 7.0 {
        let column = i32(x);
        let letter = column / 6;
        let across = column - letter * 6;
        if letter < 6 && across < 5 {
            var rows = CAR_PLATE_ROWS;
            ink = f32((rows[letter * 7 + i32(y)] >> u32(4 - across)) & 1u);
        }
    }
    let edge = max(abs(p.x) - 0.248, abs(p.y) - 0.0435);
    return max(ink, smoothstep(0.0, 0.002, edge) * (1.0 - smoothstep(0.004, 0.006, edge)));
}

// What a mirror shows the driver: the road behind. A mirror is turned so that the driver sees
// straight back in its middle (the door mirrors a little outwards); along the ray it sends the
// eye's look, the sky and its clouds, the line of the trees, and the circuit's own ground as it
// is where the car is: the asphalt with its edge lines, the kerbs where it bends, the grass
// beyond, bending away as the circuit does (`car.spare`: the car's place across it, its heading
// and curvature there; `car.more.z`: half its width). `local`: the point of the mirror in the
// body's axes; `rel`: from the eye.
fn car_behind(local: vec3<f32>, rel: vec3<f32>, sun: vec3<f32>) -> vec3<f32> {
    let fwd = frame.body_fwd.xyz;
    let right = frame.body_right.xyz;
    let down = frame.body_down.xyz;
    var centre = vec3<f32>(0.085, 0.0, -0.712);
    var aim = normalize(-fwd + down * 0.03);
    // A mirror is a little convex: it shows more than a flat one of its size would.
    var wide = 2.4;
    if abs(local.y) > 0.4 {
        let side = sign(local.y);
        centre = vec3<f32>(0.573, side * 0.965, -0.512);
        aim = normalize(-fwd + right * side * 0.20 + down * 0.07);
        wide = 2.6;
    }
    let c = normalize(frame.body_pos.xyz + fwd * centre.x + right * centre.y + down * centre.z);
    let d = reflect(normalize(mix(c, normalize(rel), wide)), normalize(aim - c));
    let above = vec3<f32>(0.0, 1.0, 0.0);
    let day = sun * max(frame.sun_dir.y, 0.0);
    let ambient = sky_light(above, rel);
    // The sky, and the trees and land round the circuit low over the horizon.
    var seen = car_sky(normalize(vec3<f32>(d.x, max(d.y, 0.03), d.z)), rel);
    let land = vec3<f32>(0.035, 0.055, 0.022) * (day * 0.7 + ambient);
    seen = mix(seen, land, smoothstep(0.05, 0.012, d.y));
    // The ground: the plane the car stands on.
    let up = -down;
    let fall = dot(d, up);
    if fall < -0.004 {
        let t = dot(frame.body_pos.xyz + down * 0.46 - rel, up) / fall;
        let p = rel + d * t;
        let o = p - frame.body_pos.xyz;
        let heading = car.spare.z;
        let s = dot(o, vec3<f32>(sin(heading), 0.0, -cos(heading)));
        let q = abs(car.spare.y + dot(o, vec3<f32>(cos(heading), 0.0, sin(heading))) - 0.5 * car.spare.w * s * s);
        let half = car.more.z;
        // What a pixel of the mirror covers of the ground there: its lines soften with distance.
        let aa = 0.02 + t * 0.004;
        var ground = vec3<f32>(0.046, 0.046, 0.049) * (0.8 + 0.4 * vnoise(vec2<f32>(q * 0.9, s * 0.12), 4096));
        let line = smoothstep(half - 0.2 - aa, half - 0.2 + aa, q) * (1.0 - smoothstep(half - 0.1 - aa, half - 0.1 + aa, q));
        ground = mix(ground, vec3<f32>(0.5), line * (1.0 - smoothstep(60.0, 220.0, t)));
        // Kerbs where the circuit bends: red and white, a metre each.
        let kerbed = smoothstep(0.0025, 0.004, abs(car.spare.w));
        let stripe = smoothstep(0.5 - aa, 0.5 + aa, abs(fract(s * 0.5) - 0.5) * 2.0);
        let kerb = mix(mix(vec3<f32>(0.42, 0.035, 0.03), vec3<f32>(0.6), stripe), vec3<f32>(0.5, 0.3, 0.3), smoothstep(40.0, 120.0, t));
        ground = mix(ground, kerb, kerbed * smoothstep(half - aa, half + aa, q));
        let grass = vec3<f32>(0.036, 0.07, 0.02) * (0.92 + 0.16 * step(0.5, fract(s / 10.0)));
        ground = mix(ground, grass, smoothstep(half + 1.2 * kerbed - aa, half + 1.2 * kerbed + aa, q));
        // Lit as the ground is: the sun and the sky, the floodlights by night.
        let lit = ground * (day * cloud_shadow(p) + ambient + vec3<f32>(1.0, 0.97, 0.90) * car.state.z * 0.8);
        seen = mix(apply_fog(lit, p), seen, smoothstep(220.0, 480.0, t));
    }
    return seen;
}

// A layer of the rain's drops on a pane, at `q` (metres on the pane): one drop to a cell of
// `cell` metres at most, each forming, standing a few seconds and going as another forms
// elsewhere in its cell. `amount`: how many of the cells have one, 0..1. xy = the way the drop's
// surface leans there (it is a lens), z = how much of the point it covers.
fn car_drops(q: vec2<f32>, cell: f32, time: f32, amount: f32, salt: f32) -> vec3<f32> {
    let g = q / cell;
    let id = floor(g);
    let h = car_hash3(vec3<f32>(id, salt));
    let turn = time / (2.5 + 5.0 * h.z) + h.x * 7.0;
    let age = fract(turn);
    let now = car_hash3(vec3<f32>(id + floor(turn) * 17.0, salt + 3.0));
    if now.z > amount {
        return vec3<f32>(0.0);
    }
    let radius = (0.10 + 0.30 * now.x * now.x) * smoothstep(0.0, 0.08, age) * (1.0 - smoothstep(0.8, 1.0, age));
    let d = (fract(g) - (0.3 + 0.4 * now.xy)) / max(radius, 1e-4);
    let l = length(d);
    if l >= 1.0 {
        return vec3<f32>(0.0);
    }
    return vec3<f32>(d, 1.0 - smoothstep(0.7, 1.0, l));
}

// A wiper of the windscreen at `q` (metres to the right of the car's middle, metres up from the
// ground): its arm turns about `pivot` from lying along the screen's foot up to standing, and
// back, every 1.4 s; its blade is `reach` (from, to) metres from the pivot. x = how much of the
// point the blade or the arm covers, y = seconds since the blade last wiped the point (a large
// number where it never does).
fn car_wiper(q: vec2<f32>, pivot: vec2<f32>, reach: vec2<f32>, sweep: f32, time: f32) -> vec2<f32> {
    let period = 1.4;
    let rest = 0.07;
    let u = fract(time / period);
    let angle = rest + sweep * (0.5 - 0.5 * cos(6.2831853 * u));
    let o = q - pivot;
    let r = length(o);
    let dir = vec2<f32>(cos(angle), sin(angle));
    let along = dot(o, dir);
    let off = abs(dot(o, vec2<f32>(-dir.y, dir.x)));
    let blade = (1.0 - smoothstep(0.006, 0.009, off)) * step(reach.x, along) * step(along, reach.y);
    let arm = (1.0 - smoothstep(0.003, 0.005, off)) * step(0.0, along) * step(along, reach.x + 0.12);
    var since = 1000.0;
    let at = atan2(o.y, o.x);
    if r > reach.x && r < reach.y && at > rest && at < rest + sweep {
        // When the blade is at this angle: twice a turn, going up and coming back.
        let w = acos(clamp(1.0 - 2.0 * (at - rest) / sweep, -1.0, 1.0)) / 6.2831853;
        since = min(fract(u - w), fract(u + w)) * period;
    }
    return vec2<f32>(max(blade, arm), since);
}

struct CarShaded {
    color: vec3<f32>,
    alpha: f32,
};

fn car_shade(in: CarOut, front_facing: bool) -> CarShaded {
    let kind = i32(car.surface.x + 0.5);
    let front = front_facing != (car.surface.w < 0.0);
    let inside_view = car.state.x > 0.5;
    var n = normalize(in.normal);
    let v = normalize(-in.rel);
    var base = car.base.rgb;
    var rough = car.surface.y;
    var metal = car.surface.z;
    let open = in.open;
    let l = frame.sun_dir.xyz;
    // The sun, and where the car shades itself (the mirrors and the spoiler on the body, the roof
    // on the cabin): the point looked up off the face it is on, on the side it is seen from.
    let sun = car_sun(in.rel) * vehicle_shadow(in.rel, select(-n, n, front));
    var out: CarShaded;
    out.alpha = 1.0;

    if kind == CAR_GLASS {
        // From outside only the faces towards the eye; from inside only their backs, nearly clear.
        if front == inside_view {
            discard;
        }
        if inside_view {
            n = -n;
            let f = 0.02 + 0.5 * pow(1.0 - saturate(dot(n, v)), 5.0);
            out.color = apply_fog(vec3<f32>(0.01, 0.012, 0.012) * sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel) * f, in.rel);
            out.alpha = 0.05 + f * 0.3;
            return out;
        }
        let ndv = saturate(dot(n, v));
        let f = 0.06 + 0.94 * pow(1.0 - ndv, 5.0);
        let r = reflect(-v, n);
        var col = car_mirrored(r, in.rel, 0.0, sun) * f;
        col += sun * car_highlight(n, v, l, 0.03, vec3<f32>(0.06));
        // The tint lets a third of the light through, less at a slant.
        let through = (1.0 - f) * car.base.a;
        col += base * (sun * max(dot(n, l), 0.0) + sky_light(n, in.rel)) * (1.0 - through);
        out.color = apply_fog(col, in.rel);
        out.alpha = 1.0 - through;
        return out;
    }

    // The shell seen from within is the cabin's lining: pillars, roof, doors.
    if !front {
        n = -n;
        if kind == CAR_PAINT || kind == CAR_PLAIN || kind == CAR_LAMP || kind == CAR_GRILLE || kind == CAR_CARBON {
            out.color = apply_fog(car_lining(in.local) * (car_inside_light(n, in.rel, 0.7, sun) * (0.4 + 0.6 * car.state.y) + car_cluster_light(in.rel, n) * 0.5), in.rel);
            return out;
        }
    }
    let ndv = saturate(dot(n, v));
    let ndl = max(dot(n, l), 0.0);
    let r = reflect(-v, n);
    // A part of the cabin is in the cabin's light, and mirrors little of the world outside.
    let cabin = car.state.w > 0.5;
    var ambient = sky_light(n, in.rel) * open;
    // What a mirror sees is hidden where the surface is shut in.
    var seen = saturate(open * 1.6 - 0.25);
    var sun_here = sun;
    if cabin {
        ambient = car_inside_light(n, in.rel, open, sun) * (0.4 + 0.6 * car.state.y) + car_cluster_light(in.rel, n);
        seen *= 0.2;
        sun_here = sun * 0.15;
    }
    var col: vec3<f32>;

    if kind == CAR_PAINT {
        // The flakes: each a little mirror turned its own way, seen one by one only from close.
        let grain = 1.0 / 0.0007;
        let size = length(fwidth(in.local)) * grain;
        let near = 1.0 - smoothstep(0.6, 2.5, size);
        let turned = car_hash3(floor(in.local * grain)) - 0.5;
        let nf = normalize(n + turned * 0.5 * near);
        // The colour is deepest seen head on and darkens to the flanks, as a metallic paint does
        // (its flakes lie flat: seen at a slant they send the light elsewhere).
        let colour = base * (0.34 + 0.72 * ndv);
        // The base coat: pigment lit by the sun and the sky, and the flakes' own highlight in the
        // paint's colour, a tight one in a wide one. Kept under what the tone mapping turns
        // pale: a red stays red in the midday sun.
        col = colour * (0.50 * (sun * ndl + ambient));
        let flakes = car_highlight(nf, v, l, 0.20, colour) * 0.25 + car_highlight(nf, v, l, 0.46, colour) * 0.35;
        col += sun * min(flakes, colour * 2.2);
        // Flakes that happen to face the eye glint one by one round the highlight.
        let glint = pow(max(dot(normalize(n + turned * 1.6), normalize(l + v)), 0.0), 220.0) * near;
        col += sun * (colour + 0.25) * glint * 1.6 * ndl;
        col += car_mirrored(reflect(-v, nf), in.rel, 0.55, sun) * colour * seen * 0.5;
        // The clear coat: a thick one, more of a mirror than glass is, sharp: the sky with its
        // clouds, the line of the horizon and the ground run along the body.
        let f = 0.08 + 0.92 * pow(1.0 - ndv, 5.0);
        col = col * (1.0 - f) + car_mirrored(r, in.rel, 0.0, sun) * f * seen * 1.3;
        // The sun in it: a small hot highlight (the bloom catches it), with a faint wider one round it.
        col += sun * (car_highlight(n, v, l, 0.028, vec3<f32>(0.05)) + car_highlight(n, v, l, 0.11, vec3<f32>(0.05)) * 0.25);
    } else if kind == CAR_LAMP {
        // A lens: dark and glossy unlit, with what it gives off when lit.
        let f = 0.05 + 0.95 * pow(1.0 - ndv, 5.0);
        col = base * (sun * ndl + ambient) * 0.6;
        col = col * (1.0 - f) + car_mirrored(r, in.rel, 0.0, sun) * f * seen;
        col += sun * car_highlight(n, v, l, 0.05, vec3<f32>(0.05));
        // The brake lights are a row of cells, each a little apart from the next and brightest
        // in its middle; far away they run together.
        var lit = 1.0;
        if car.more.y > 0.5 {
            let pitch = 0.042;
            let across = abs(fract(abs(in.local.y) / pitch) - 0.5) * pitch;
            let far = saturate(fwidth(in.local.y) / 0.012);
            lit = mix(smoothstep(0.003, 0.0055, across) * (0.55 + 0.45 * smoothstep(0.004, 0.019, across)), 0.72, far);
            col *= 0.35 + 0.65 * lit;
        }
        // Brightest seen along its axis.
        col += car.glow.rgb * lit * (0.45 + 0.55 * ndv);
    } else if kind == CAR_RUBBER {
        col = base * (sun * ndl + ambient);
        col += sun * car_highlight(n, v, l, 0.6, vec3<f32>(0.04)) + car_mirrored(r, in.rel, 0.8, sun) * 0.03 * seen;
    } else if kind == CAR_DISC {
        // Brushed iron, turned: the light streaks round it. Hot, it glows from dull red to orange.
        let f0 = base;
        col = sun * car_highlight(n, v, l, rough, f0) + car_mirrored(r, in.rel, rough, sun) * f0 * seen * 0.8;
        let radius = length(in.local.xz);
        let ring = smoothstep(0.10, 0.125, radius) * (0.75 + 0.25 * sin(radius * 900.0));
        let heat = car.glow.w;
        col += vec3<f32>(1.0, 0.16 + 0.3 * heat, 0.02 + 0.08 * heat) * heat * heat * 9.0 * ring;
    } else if kind == CAR_DIAL {
        let radius = 0.062;
        let p = vec2<f32>(in.local.y, -in.local.z) / radius;
        let dial = car_dial(p, car.glow.w, car.more.x, i32(round(car.more.z)));
        col = base * car_inside_light(n, in.rel, 1.0, sun) * (0.3 + 0.7 * car.state.y);
        // The marks are lit from behind: white by day, a cool glow by night.
        col = mix(col, dial.rgb * car.glow.rgb * mix(vec3<f32>(0.7, 0.85, 1.0), vec3<f32>(1.0), car.state.y), dial.a);
        // A bright ring round the face.
        let bezel = smoothstep(0.955, 0.97, length(p));
        col = mix(col, vec3<f32>(0.5) * car_inside_light(n, in.rel, 1.0, sun), bezel);
    } else if kind == CAR_INSIDE {
        let colour = car_inside_colour(base, in.local);
        col = colour * (car_inside_light(n, in.rel, open, sun) * (0.4 + 0.6 * car.state.y) + car_cluster_light(in.rel, n));
        col += car.glow.rgb;
    } else if kind == CAR_MIRROR {
        if inside_view {
            col = car_behind(in.local, in.rel, sun) * base * 0.85;
        } else {
            col = car_mirrored(r, in.rel, 0.0, sun) * base * 0.45;
        }
    } else {
        if kind == CAR_PLATE {
            // Four samples within the pixel: the dots' edges without stairs; far away, a grey.
            let p = vec2<f32>(in.local.y, CAR_PLATE_Z - in.local.z);
            let dx = dpdx(p) * 0.25;
            let dy = dpdy(p) * 0.25;
            let ink = 0.25 * (car_plate_ink(p + dx + dy) + car_plate_ink(p - dx + dy) + car_plate_ink(p + dx - dy) + car_plate_ink(p - dx - dy));
            base = mix(base, vec3<f32>(0.012), ink);
        }
        if kind == CAR_GRILLE {
            // A mesh of small hexagons: its holes black.
            let q = vec2<f32>(in.local.y, in.local.z) * 70.0;
            let cell = abs(fract(vec2<f32>(q.x + 0.5 * floor(q.y), q.y)) - 0.5);
            let wire = smoothstep(0.30, 0.42, max(cell.x, cell.y));
            let far = saturate(length(fwidth(q)) * 1.2);
            base *= mix(0.2 + 0.9 * wire, 0.45, far);
        } else if kind == CAR_CARBON {
            // A twill weave: the tows catch the light in turn.
            let q = floor(vec2<f32>(in.local.x + in.local.z, in.local.y) * 260.0);
            let weave = f32((i32(q.x) + i32(q.y) * 2) & 3) / 3.0;
            let far = saturate(length(fwidth(in.local)) * 300.0);
            base *= mix(0.75 + 0.5 * weave, 1.0, far);
        }
        // Metal mirrors in its own colour; anything else is its colour under a sheen.
        let f0 = mix(vec3<f32>(0.04), base, metal);
        // The sun does not reach what the body shuts in (the valance under the tail, the
        // diffuser's hollow, the wells), finer than its shadow map tells.
        sun_here *= smoothstep(0.30, 0.75, open);
        col = base * (1.0 - metal) * (sun_here * ndl + ambient);
        col += sun_here * car_highlight(n, v, l, rough, f0);
        // A rough surface mirrors little: what it would show is spread into its own colour.
        col += car_mirrored(r, in.rel, rough, sun) * car_fresnel(f0, ndv) * seen * mix((1.0 - rough) * (1.0 - rough), 1.0 - 0.6 * rough, metal);
        col += car.glow.rgb;
    }
    // The circuit's floodlights, by night: most of their light from the brightest tower, where
    // it stands, the rest from the others all round and above.
    if car.state.z > 0.0 && !cabin && kind != CAR_DIAL && kind != CAR_INSIDE {
        let lamp = vec3<f32>(1.0, 0.97, 0.90) * car.state.z;
        var lamp_dir = vec3<f32>(0.3, 1.0, 0.2);
        if kind != CAR_MIRROR && kind != CAR_GLASS && dot(car.spare.yzw, car.spare.yzw) > 0.5 {
            lamp_dir = car.spare.yzw;
        }
        lamp_dir = normalize(lamp_dir);
        let f0 = mix(vec3<f32>(0.04), base, metal);
        col += base * (1.0 - metal) * lamp * (0.16 * (0.6 + 0.4 * n.y) + 0.75 * max(dot(n, lamp_dir), 0.0)) * open;
        col += lamp * car_highlight(n, v, lamp_dir, max(rough, 0.1), f0) * seen;
        if kind == CAR_PAINT {
            // The paint's colour under them, and the rack of lamps itself in the clear coat: a
            // bright patch a few degrees wide, with a wider sheen round it.
            col += base * lamp * (0.02 + 0.42 * max(dot(n, lamp_dir), 0.0)) * (0.4 + 0.6 * ndv) * open;
            col += lamp * (car_highlight(n, v, lamp_dir, 0.09, vec3<f32>(0.05)) * 0.6 + car_highlight(n, v, lamp_dir, 0.3, vec3<f32>(0.05)) * 0.6) * seen;
        }
    }
    if kind == CAR_BLUR {
        // A turning wheel's spokes, blurred into a disc: the more of the circle they fill, the more it covers.
        let radius = length(in.local.xz);
        out.alpha = saturate(car.more.x * 0.026 / (6.2832 * max(radius, 0.03))) * (0.82 + 0.18 * sin(radius * 420.0));
        col *= out.alpha;
    }
    out.color = apply_fog(col, in.rel);
    return out;
}

@fragment
fn car_fragment(in: CarOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let s = car_shade(in, front_facing);
    return vec4<f32>(s.color, 1.0);
}

// The see-through parts (glass, a turning wheel's blur): premultiplied.
@fragment
fn car_glass_fragment(in: CarOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let s = car_shade(in, front_facing);
    return vec4<f32>(s.color, s.alpha);
}

// The glass from the seat in the rain, in the frame's second pass: over the glass as it is, a
// mist of fine drops that blurs what is behind it a little, the drops (each a small lens: the
// scene the first pass left, turned over and squeezed in it), and on the windscreen the wipers,
// which leave the part they sweep clear until drops form on it again.
@fragment
fn car_wet_glass_fragment(in: CarOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let s = car_shade(in, front_facing);
    var out = vec4<f32>(s.color, s.alpha);
    let wet = car.more.w;
    if wet <= 0.0 || car.state.x < 0.5 {
        return out;
    }
    let time = car.more.z;
    let size = vec2<f32>(textureDimensions(car_scene));
    let uv = in.position.xy / size;
    // The pane's own coordinates: a side window is seen from the side, the others from ahead.
    let facing = abs(normalize(cross(dpdx(in.local), dpdy(in.local))));
    var q = vec2<f32>(in.local.y, 0.46 - in.local.z);
    var amount = wet * (0.5 + 0.3 * smoothstep(5.0, 45.0, abs(car.spare.y)));
    var blade = 0.0;
    if facing.y > 0.65 {
        q = vec2<f32>(in.local.x, 0.46 - in.local.z);
    } else if in.local.x > 0.0 {
        // The windscreen: two wipers, the driver's and the passenger's.
        let a = car_wiper(q, vec2<f32>(-0.66, 0.862), vec2<f32>(0.11, 0.60), 1.62, time);
        let b = car_wiper(q, vec2<f32>(-0.06, 0.862), vec2<f32>(0.11, 0.62), 2.05, time);
        blade = max(a.x, b.x);
        amount *= 0.04 + 0.96 * smoothstep(0.0, 3.2, min(a.y, b.y));
    }
    // The mist: what is behind, blurred over a few pixels.
    let mist = 0.55 * amount;
    if mist > 0.01 {
        let turn = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715)))) * 6.2831853;
        let spread = vec2<f32>(cos(turn), sin(turn)) * 5.0 / size;
        let blurred = 0.25 * (textureSampleLevel(car_scene, car_scene_sampler, uv + spread, 0.0).rgb + textureSampleLevel(car_scene, car_scene_sampler, uv - spread, 0.0).rgb + textureSampleLevel(car_scene, car_scene_sampler, uv + vec2<f32>(-spread.y, spread.x), 0.0).rgb + textureSampleLevel(car_scene, car_scene_sampler, uv - vec2<f32>(-spread.y, spread.x), 0.0).rgb);
        out = vec4<f32>(blurred * mist + out.rgb * (1.0 - mist), mist + out.a * (1.0 - mist));
    }
    // The drops: small ones that stand, larger ones that run down slowly, each column at its own pace.
    let column = floor(q.x / 0.03);
    let run = time * (0.004 + 0.02 * car_hash3(vec3<f32>(column, 3.0, 9.0)).x);
    var drop = car_drops(q, 0.009, time, amount, 1.0);
    let large = car_drops(q + vec2<f32>(0.0, run), 0.02, time * 0.6, amount * 0.4, 5.0);
    if large.z > drop.z {
        drop = large;
    }
    if drop.z > 0.0 {
        let bent = clamp(uv - drop.xy * vec2<f32>(1.0, -1.0) * 0.05, vec2<f32>(0.002), vec2<f32>(0.998));
        var lens = textureSampleLevel(car_scene, car_scene_sampler, bent, 0.0).rgb;
        // Darker at its rim, a glint of the sky on its upper side.
        lens *= 1.0 - 0.45 * dot(drop.xy, drop.xy) * dot(drop.xy, drop.xy);
        lens += vec3<f32>(luma(sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel))) * 0.07 * smoothstep(0.35, 0.8, drop.y) * (1.0 - smoothstep(0.8, 1.0, length(drop.xy)));
        out = vec4<f32>(lens * drop.z + out.rgb * (1.0 - drop.z), drop.z + out.a * (1.0 - drop.z));
    }
    // The blades, of black rubber.
    if blade > 0.0 {
        let rubber = vec3<f32>(0.01) * sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel);
        out = vec4<f32>(rubber * blade + out.rgb * (1.0 - blade), blade + out.a * (1.0 - blade));
    }
    return out;
}
