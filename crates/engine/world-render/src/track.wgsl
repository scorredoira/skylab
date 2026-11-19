// A race circuit's own ground and what stands on it (`track.rs`): the asphalt with its painted
// lines, the rubber the cars leave on their line, its patches and sealed cracks, the start's grid
// and its chequered line, the kerbs, the run-off (grass, gravel, asphalt), the verges, the pit
// lane, and the barriers, stands with their crowd, advertising boards, floodlights, pits and
// gantry. Wet, the paved ground darkens and mirrors the sky, with standing water in its hollows,
// and in a second pass what stands on it (`track_wet_fragment`); by night the floodlights' towers
// light it (`TrackIn::light`, worked out with the mesh), and the car the view follows: its
// dipped beams ahead and its tail lamps' red behind. Appended to the world's shaders
// (`track_source`: the boards' letters come before it); it uses their `frame`, `object`, the
// photographs (`mat_planar`), the light of the sky, the shadows of the relief, the clouds and the
// body, and the haze.
//
// `object` here: `tint` = how wet (x), the start's metres along (y), the lap's length (z), the
// grid's places (w); `cuts` = the four towers nearest the eye (xyz = their lamps from the eye,
// w = how strong, 0 none); `keep` = the light of the followed car's dipped beams (x) and of its
// tail lamps (y) at a metre, in the scene's units.

struct TrackIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,   // rgb = albedo, a = gloss (props); on the asphalt: r = the cars' line across, g = its rubber, a = half its width
    @location(3) surface: vec4<f32>, // x = kind (TRACK_*), y = metres across, z = metres along, w = how open to the sky (a panel: its own coordinates and number)
    @location(4) light: vec2<f32>,   // x = the floodlights' light there by full night
};

struct TrackOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) surface: vec4<f32>,
    @location(4) light: vec2<f32>,
};

// The scene as the first pass left it, for the second (`track_wet_fragment`): its colour, its
// depth (several samples per pixel), a sampler. Group 2 of that pipeline alone.
@group(2) @binding(8) var wet_scene: texture_2d<f32>;
@group(2) @binding(9) var wet_depth: texture_depth_multisampled_2d;
@group(2) @binding(10) var wet_sampler: sampler;

// What a vertex is: the kinds of `track.rs` (`Kind`).
const TRACK_PROP: i32 = 0;
const TRACK_ASPHALT: i32 = 1;
const TRACK_KERB: i32 = 2;
const TRACK_GRASS: i32 = 3;
const TRACK_GRAVEL: i32 = 4;
const TRACK_CONCRETE: i32 = 5;
const TRACK_RUNOFF: i32 = 6;   // asphalt of a run-off: no lines on it
const TRACK_BELT: i32 = 7;     // the belt over a tyre wall: stripes along it
const TRACK_CROWD: i32 = 8;    // a row of spectators: y = metres along, z = 0 to 1 up the card, color.a = the row's number
const TRACK_BOARD: i32 = 9;    // an advertising board: y = metres along it, z = 0 to 1 up it, w = its length, color.a = its name + 8 x (where it is along the circuit, in eights of a metre)
const TRACK_LAMP: i32 = 10;    // a floodlight's lamp

// The asphalt's own colour (the photograph multiplies it).
const TRACK_ASPHALT_COLOR = vec3<f32>(0.075, 0.075, 0.08);
// The colour of the floodlights' light.
const TRACK_FLOOD = vec3<f32>(1.0, 0.97, 0.90);

// How much night it is for the floodlights: they come on as the sun goes down (by where it is,
// not by how dark a cloudy day is).
fn track_night() -> f32 {
    return 1.0 - smoothstep(-0.10, 0.02, frame.sky_sun.y);
}

// The followed car's lamps on a surface at `rel` facing `n` (`object.keep`): its two dipped
// beams, each wide (half its light 17 degrees to the side, a skirt beyond), brightest just under
// the horizontal and cut off above it (a little spills over, which is what shows a board), dimmer
// towards the ground at the car's nose so that the road is lit evenly far and near; and its tail
// lamps, a red glow on what is behind it. Both fall off with the square of the distance.
fn track_car_lamps(rel: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    if object.keep.x + object.keep.y <= 0.0 || frame.body_pos.w <= -1.0 {
        return vec3<f32>(0.0);
    }
    let o = rel - frame.body_pos.xyz;
    let fwd = frame.body_fwd.xyz;
    let right = frame.body_right.xyz;
    let down = frame.body_down.xyz;
    var lit = vec3<f32>(0.0);
    if object.keep.x > 0.0 && dot(o, fwd) > 1.5 {
        var sum = 0.0;
        for (var i = 0; i < 2; i++) {
            let d = o - (fwd * 2.05 + right * select(-0.68, 0.68, i == 1) - down * 0.18);
            let ahead = dot(d, fwd);
            if ahead > 0.05 {
                let d2 = dot(d, d);
                let tx = dot(d, right) / ahead;
                let ty = -dot(d, down) / ahead;
                let wide = 0.55 * exp(-tx * tx / 0.19) + 0.45 * exp(-tx * tx / 0.018);
                let cut = mix(0.035, 1.0, 1.0 - smoothstep(-0.028, 0.006, ty));
                let low = exp(-max(-ty - 0.02, 0.0) * 11.0);
                sum += wide * cut * low * max(dot(n, -d) * inverseSqrt(d2), 0.0) / (d2 + 0.5);
            }
        }
        lit += vec3<f32>(1.0, 0.96, 0.88) * sum * object.keep.x;
    }
    if object.keep.y > 0.0 {
        let d = o - (-fwd * 2.3 - down * 0.40);
        let d2 = dot(d, d);
        let behind = smoothstep(-0.2, 0.5, -dot(d, fwd) * inverseSqrt(d2));
        lit += vec3<f32>(1.0, 0.045, 0.02) * object.keep.y * behind * max(dot(n, -d) * inverseSqrt(d2), 0.0) / (d2 + 0.3);
    }
    return lit;
}

// The glint of the towers nearest the eye (`object.cuts`) in a film of water whose surface faces
// `nw`, seen along `v`: each lamp's own highlight, drawn out towards the eye as a wet road draws it.
fn track_glints(rel: vec3<f32>, nw: vec3<f32>, v: vec3<f32>, rough: f32) -> vec3<f32> {
    let night = track_night();
    if night <= 0.0 {
        return vec3<f32>(0.0);
    }
    var sum = 0.0;
    for (var i = 0; i < 4; i++) {
        let lamp = object.cuts[i];
        if lamp.w > 0.0 {
            // A lamp that is in the picture is mirrored out of the picture, in the second pass
            // (`track_mirrored`): this is for those over the picture's edge.
            let clip = frame.view_proj * vec4<f32>(lamp.xyz, 1.0);
            var outside = 1.0;
            if clip.w > 0.0 {
                let at = abs(clip.xy / clip.w);
                outside = smoothstep(0.85, 1.05, max(at.x, at.y));
            }
            let d = lamp.xyz - rel;
            let d2 = dot(d, d);
            let l = d * inverseSqrt(d2);
            sum += outside * lamp.w * mat_sheen(nw, v, l, rough) * max(dot(nw, l), 0.0) / d2;
        }
    }
    return TRACK_FLOOD * sum * TOWER_LIGHT * night;
}

// The film of water on a point of the paved ground: how much of it there is (`film`: a damp
// surface to standing water, `puddle`), the way its surface faces and how much of a mirror it is
// seen along `v`.
struct TrackWater {
    film: f32,
    puddle: f32,
    n: vec3<f32>,
    mirror: f32,
};

fn track_water(uv: vec2<f32>, kind: i32, n: vec3<f32>, level: vec3<f32>, v: vec3<f32>) -> TrackWater {
    var w: TrackWater;
    let wet = object.tint.x;
    let hollow = vnoise(uv / 5.0, 4096) * 0.65 + vnoise(uv / 1.7 + 3.1, 4096) * 0.35;
    w.puddle = smoothstep(0.70 - 0.10 * wet, 0.76, hollow) * wet * select(0.3, 1.0, kind != TRACK_KERB);
    w.film = wet * (0.28 + 0.72 * w.puddle);
    // The water lies level whatever the grain under it; in a puddle the rain's rings stir it a little.
    let ripple = (vec2<f32>(vnoise(uv * 9.0 + frame.sun_dir.w * 3.0, 4096), vnoise(uv * 9.0 + 5.0 - frame.sun_dir.w * 3.0, 4096)) - 0.5) * 0.03 * w.puddle;
    w.n = normalize(mix(n, level, 0.72 + 0.28 * w.puddle) + vec3<f32>(ripple.x, 0.0, ripple.y));
    // More of a mirror than still water seen from above: the film lies on a grain that turns
    // part of it towards the eye.
    w.mirror = (0.035 + 0.965 * pow(1.0 - max(dot(w.n, v), 0.0), 5.0)) * w.film;
    return w;
}

// What the scene has along the ray from `rel` towards `away`, found in the picture of the first
// pass: the ray is followed across the picture a few pixels at a time (a different start at each
// pixel, so that the steps do not show), until the scene at a pixel is nearer than the ray is
// there by no more than the ray went in that step; then between the last two steps. rgb = the
// scene there, a = how much of it is to be believed: nothing when the ray leaves the picture,
// meets nothing or only the sky (which the first pass has mirrored already), fading at the
// picture's edges.
fn track_mirrored(rel: vec3<f32>, away: vec3<f32>, pixel: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(wet_depth));
    let begin = frame.view_proj * vec4<f32>(rel + away * 0.3, 1.0);
    // As far as 160 m, or to just before the eye's plane for a ray that comes back.
    let fwd = dot(away, frame.cam_fwd.xyz);
    var reach = 160.0;
    if fwd < 0.0 {
        reach = min(reach, (begin.w - 0.3) / -fwd);
    }
    if begin.w <= 0.3 || reach <= 0.5 {
        return vec4<f32>(0.0);
    }
    let to = frame.view_proj * vec4<f32>(rel + away * reach, 1.0);
    // In the picture's own space what is linear along the ray is 1 / distance: the reversed depth.
    let a = vec3<f32>(begin.xy / begin.w, begin.z / begin.w);
    let b = vec3<f32>(to.xy / to.w, to.z / to.w);
    let pixels = length((b.xy - a.xy) * 0.5 * size);
    let steps = clamp(i32(pixels / 9.0) + 1, 4, 44);
    let jitter = fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
    let near = begin.z;
    var last = 0.0;
    var found = -1.0;
    for (var i = 0; i < steps; i++) {
        let t = (f32(i) + 0.3 + jitter) / f32(steps);
        let p = mix(a, b, t);
        if abs(p.x) >= 1.0 || abs(p.y) >= 1.0 {
            break;
        }
        let scene = textureLoad(wet_depth, vec2<i32>((p.xy * vec2<f32>(0.5, -0.5) + 0.5) * size), 0);
        // Nearer than the ray there (reversed depth: more is nearer), by less than the step's own depth.
        if scene > p.z {
            let before = mix(a.z, b.z, last);
            let behind = near / p.z - near / scene;
            if behind < abs(near / p.z - near / before) * 1.5 + 0.35 {
                found = t;
                break;
            }
        }
        last = t;
    }
    if found < 0.0 {
        return vec4<f32>(0.0);
    }
    // Between the last step in front and this one, four times halved.
    var lo = last;
    var hi = found;
    for (var i = 0; i < 4; i++) {
        let mid = 0.5 * (lo + hi);
        let p = mix(a, b, mid);
        let scene = textureLoad(wet_depth, vec2<i32>((p.xy * vec2<f32>(0.5, -0.5) + 0.5) * size), 0);
        if scene > p.z {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let p = mix(a, b, hi);
    let at = p.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let edge = min(min(at.x, 1.0 - at.x), min(at.y, 1.0 - at.y));
    // Three looks side by side: the film's grain spreads what it mirrors a little to the sides
    // too, and a lamp's streak is as wide as its glare.
    let aside = vec2<f32>(3.0 / size.x, 0.0);
    let colour = textureSampleLevel(wet_scene, wet_sampler, at, 0.0).rgb * 0.5 + textureSampleLevel(wet_scene, wet_sampler, at - aside, 0.0).rgb * 0.25 + textureSampleLevel(wet_scene, wet_sampler, at + aside, 0.0).rgb * 0.25;
    return vec4<f32>(colour, smoothstep(0.0, 0.06, edge));
}

// One dot of a board's lettering: whether the name's letters have ink at `x` dots from the
// name's left and `y` dots down from its top.
fn track_ink(name: i32, x: f32, y: f32) -> f32 {
    if x < 0.0 || y < 0.0 || y >= 7.0 {
        return 0.0;
    }
    let column = i32(x);
    let letter = column / 6;
    let dot = column - letter * 6;
    if letter >= 8 || dot >= 5 {
        return 0.0;
    }
    var names = TRACK_NAMES;
    let glyph = names[name * 8 + letter];
    if glyph < 0 {
        return 0.0;
    }
    var font = TRACK_FONT;
    let row = i32(y);
    let low = row < 4;
    let word = font[glyph * 2 + select(1, 0, low)];
    let bit = u32(select(row - 4, row, low) * 5 + dot);
    return f32((word >> bit) & 1u);
}

@vertex
fn track_vertex(in: TrackIn) -> TrackOut {
    var o: TrackOut;
    let rel = object.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.normal = (object.model * vec4<f32>(in.normal, 0.0)).xyz;
    o.color = in.color;
    o.surface = in.surface;
    o.light = in.light;
    return o;
}

// A soft-edged band 1 inside [a, b], antialiased over `aa`.
fn band(x: f32, a: f32, b: f32, aa: f32) -> f32 {
    return smoothstep(a - aa, a + aa, x) * (1.0 - smoothstep(b - aa, b + aa, x));
}

// The paint on the asphalt at (across, along): the white lines along its edges, the start's
// chequered line and the grid's boxes. `half`: half the asphalt's width; `object.tint`: y the
// start's metres along, z the lap's length, w the grid's places.
fn track_paint(across: f32, along: f32, half: f32, aa: vec2<f32>) -> f32 {
    let a = abs(across);
    // The edge lines, 10 cm wide, 10 cm in from the edge.
    var paint = band(a, half - 0.2, half - 0.1, aa.x);
    // The start and finish line: a band of chequers 1.2 m long, eight across.
    let from_start = (along - object.tint.y + object.tint.z * 0.5) % object.tint.z - object.tint.z * 0.5;
    if abs(from_start) < 1.0 {
        let cell = floor(across / (half / 4.0)) + floor(from_start / 0.6);
        let chequer = select(0.0, 1.0, (i32(cell) & 1) == 0);
        paint = max(paint, chequer * band(from_start, -0.6, 0.6, aa.y));
    }
    // The grid: an open box ahead of each place, two by two behind the line, staggered.
    let back = -from_start - 10.0;
    if back > -2.0 && back < 8.0 * object.tint.w {
        let slot = floor((back + 2.0) / 8.0);
        let side = select(1.0, -1.0, (i32(slot) & 1) == 0);
        let x = across * side - half * 0.5;
        let y = back - slot * 8.0 + 2.0;
        let bar = band(y, 0.0, 0.12, aa.y) * band(abs(x), 0.0, 1.1, aa.x);
        let legs = band(abs(x), 1.0, 1.1, aa.x) * band(y, 0.0, 1.0, aa.y);
        paint = max(paint, max(bar, legs));
    }
    return paint;
}

fn track_shade(in: TrackOut) -> vec4<f32> {
    let dist = length(in.rel);
    let kind = i32(in.surface.x + 0.5);
    let across = in.surface.y;
    let along = in.surface.z;
    let uv = vec2<f32>(across, along);
    let gx = dpdx(uv);
    let gy = dpdy(uv);
    let aa = max(abs(gx) + abs(gy), vec2<f32>(0.004));
    var n = normalize(in.normal);
    let v = normalize(-in.rel);
    if dot(n, v) < 0.0 && (kind == TRACK_PROP || kind >= TRACK_BELT) {
        n = -n;
    }
    let level = n;
    let scatter = (vnoise(uv / 16.0, 4096) * 0.6 + vnoise(uv / 9.0 + 7.3, 4096) * 0.4) * 8.0;
    var albedo = in.color.rgb;
    var gloss = select(0.0, min(in.color.a, 1.0), kind == TRACK_PROP);
    var sheen = 0.0;
    var rough = 0.6;
    var lean = vec2<f32>(0.0);
    var tex = Mat(vec3<f32>(1.0), vec2<f32>(0.0), 0.8, 1.0);
    if kind == TRACK_ASPHALT || kind == TRACK_RUNOFF {
        tex = mat_planar(M_ASPHALT, uv, gx, gy, dist, scatter);
        albedo *= 0.92 + 0.16 * vnoise(uv / 40.0, 4096);
        if kind == TRACK_ASPHALT {
            // Rubber where the cars run: a dark band along their line, darkest where they brake
            // and turn; off it the asphalt is greyer with dust and marbles.
            let off = across - in.color.r;
            let wear = (0.10 + 0.32 * in.color.g) * exp(-off * off / 3.0);
            albedo = TRACK_ASPHALT_COLOR * (0.92 + 0.16 * vnoise(uv / 40.0, 4096)) * (1.0 - wear) * (1.0 + 0.10 * smoothstep(1.5, 4.0, abs(off)));
            // Patches of newer asphalt, darker and smoother, here and there.
            let stretch = floor(along / 43.0);
            let cell = vec2<i32>(i32(stretch), 17);
            if ihash(cell) > 0.5 {
                let middle = (ihash(cell + vec2<i32>(0, 3)) - 0.5) * in.color.a * 1.3;
                let wide = 1.0 + 2.2 * ihash(cell + vec2<i32>(0, 5));
                let begins = stretch * 43.0 + 4.0 + 12.0 * ihash(cell + vec2<i32>(0, 7));
                let patched = band(across, middle - wide, middle + wide, aa.x) * band(along, begins, begins + 7.0 + 16.0 * ihash(cell + vec2<i32>(0, 9)), aa.y);
                albedo *= 1.0 - 0.2 * patched;
                tex.lean *= 1.0 - 0.5 * patched;
            }
            // Cracks sealed with tar: thin dark lines wandering along, on some stretches.
            let seam = abs(across - in.color.a * 0.45 * sin(along * 0.021 + 1.3) - 1.4 * (vnoise(vec2<f32>(along * 0.13, 4.0), 4096) - 0.5));
            let sealed = (1.0 - smoothstep(0.012, 0.022 + aa.x, seam)) * smoothstep(0.45, 0.55, vnoise(vec2<f32>(along / 70.0, 9.0), 4096));
            albedo *= 1.0 - 0.45 * sealed;
            let paint = track_paint(across, along, in.color.a, aa);
            albedo = mix(albedo, vec3<f32>(0.62), paint);
            tex.color = mix(tex.color, vec3<f32>(0.75 + 0.25 * luma(tex.color)), paint);
            tex.lean *= 1.0 - 0.6 * paint;
        } else {
            // An asphalt run-off is painted green for its first three metres, as many are.
            let out = abs(across) - in.color.a;
            let green = band(out, -0.1, 3.0, aa.x);
            albedo = mix(albedo, vec3<f32>(0.035, 0.11, 0.05), green);
            tex.lean *= 1.0 - 0.5 * green;
        }
        sheen = 1.0;
    } else if kind == TRACK_KERB {
        // Red and white, a metre each, across the kerb's width; ridges across it every 0.7 m.
        let stripe = select(0.0, 1.0, (i32(floor(along / 1.0)) & 1) == 0);
        let red = vec3<f32>(0.42, 0.035, 0.03);
        albedo = mix(red, vec3<f32>(0.62), stripe);
        let edge = band(fract(along / 1.0), 0.0, 0.02, aa.y);
        albedo *= 1.0 - 0.3 * edge;
        tex = mat_planar(M_CONCRETE, uv, gx, gy, dist, scatter);
        tex.color = mix(vec3<f32>(1.0), tex.color, 0.4);
        let ridge = cos(along * 6.2831853 / 0.7);
        lean = vec2<f32>(0.0, ridge * 0.25 * (1.0 - smoothstep(10.0, 40.0, dist)));
        sheen = 0.7;
    } else if kind == TRACK_GRASS {
        tex = mat_planar(M_GRASS_SHORT, uv, gx, gy, dist, scatter);
        // Mown in stripes 5 m wide along the circuit.
        let stripe = select(0.92, 1.08, (i32(floor(along / 5.0)) & 1) == 0);
        albedo *= stripe;
    } else if kind == TRACK_GRAVEL {
        tex = mat_planar(M_SCREE, uv * 1.6, gx * 1.6, gy * 1.6, dist, scatter);
        // Raked into shallow furrows.
        albedo *= 0.94 + 0.06 * sin(across * 4.0 + vnoise(uv * 0.5, 4096) * 2.0);
    } else if kind == TRACK_CONCRETE {
        tex = mat_planar(M_CONCRETE, uv, gx, gy, dist, scatter);
        // Slabs 5 m square.
        let joint = max(band(fract(along / 5.0), 0.0, 0.004, aa.y / 5.0), band(fract(across / 5.0), 0.0, 0.004, aa.x / 5.0));
        albedo *= 1.0 - 0.35 * joint;
        sheen = 0.6;
    } else if kind == TRACK_BELT {
        // The belt over a tyre wall: red and white panels 2 m long.
        let stripe = select(0.0, 1.0, (i32(floor(along / 2.0)) & 1) == 0);
        albedo = mix(vec3<f32>(0.40, 0.03, 0.03), vec3<f32>(0.6), stripe);
        gloss = 0.35;
    } else if kind == TRACK_CROWD {
        // People sitting in a row, half a metre each: a head over shoulders, in whatever they wear;
        // some seats empty. Nothing is drawn between them.
        let seat = across / 0.52;
        let who = vec2<i32>(i32(floor(seat)), i32(in.color.a) + i32(in.surface.w));
        if ihash(who) < 0.2 {
            discard;
        }
        let x = (fract(seat) - 0.5) * 0.52;
        let tall = (ihash(who + vec2<i32>(3, 1)) - 0.5) * 0.14;
        let y = along * 0.95 - tall;
        let head = length(vec2<f32>(x, y - 0.74)) - 0.105;
        let shoulders = max(abs(x) - 0.215 + 0.10 * smoothstep(0.42, 0.62, y), y - 0.60);
        if min(head, shoulders) > 0.0 {
            discard;
        }
        let c = ihash(who + vec2<i32>(7, 2));
        let c2 = ihash(who + vec2<i32>(11, 5));
        let clothes = mix(mix(vec3<f32>(0.5, 0.05, 0.04), vec3<f32>(0.04, 0.12, 0.4), step(0.33, c)), mix(vec3<f32>(0.55, 0.55, 0.55), vec3<f32>(0.03, 0.03, 0.035), step(0.75, c)), step(0.55, c)) * (0.6 + 0.8 * c2);
        let skin = mix(vec3<f32>(0.55, 0.36, 0.26), vec3<f32>(0.16, 0.09, 0.06), smoothstep(0.5, 1.0, c2));
        let hat = step(0.7, ihash(who + vec2<i32>(5, 9))) * step(0.80, y);
        albedo = select(clothes, mix(skin, clothes, hat), head < 0.0);
    } else if kind == TRACK_BOARD {
        // The board's name in the middle, six tenths of its height, and a line round its edge.
        let name = i32(in.color.a + 0.5) % TRACK_NAME_COUNT;
        var names = TRACK_NAMES;
        var letters = 0.0;
        for (var k = 0; k < 8; k++) {
            letters += select(0.0, 1.0, names[name * 8 + k] >= 0);
        }
        let dot = 0.6 / 7.0;
        let width = (letters * 6.0 - 1.0) * dot;
        let x = (across - (in.surface.w - width) * 0.5) / dot;
        let y = (0.8 - along) / dot;
        // Four samples within the pixel: the dots' edges without stairs.
        let dx = dpdx(vec2<f32>(x, y)) * 0.25;
        let dy = dpdy(vec2<f32>(x, y)) * 0.25;
        var ink = 0.0;
        ink += track_ink(name, x + dx.x + dy.x, y + dx.y + dy.y);
        ink += track_ink(name, x - dx.x + dy.x, y - dx.y + dy.y);
        ink += track_ink(name, x + dx.x - dy.x, y + dx.y - dy.y);
        ink += track_ink(name, x - dx.x - dy.x, y - dx.y - dy.y);
        ink *= 0.25 * (1.0 - smoothstep(0.35, 0.7, length(dx + dy) * 4.0) * 0.5);
        let edge = 1.0 - band(along, 0.05, 0.95, 0.005) * band(across, 0.05, in.surface.w - 0.05, 0.005);
        let text = select(vec3<f32>(0.7), vec3<f32>(0.02), luma(albedo) > 0.18);
        albedo = mix(albedo, text, max(ink, edge));
        gloss = 0.3;
    } else if kind == TRACK_LAMP {
        // A lamp of a floodlight: lit from dusk, a pale lens by day.
        let lit = albedo * 60.0 * track_night();
        if track_night() > 0.0 {
            return vec4<f32>(mix(lit, apply_fog(lit, in.rel), 0.6), 1.0);
        }
        albedo = vec3<f32>(0.5);
        gloss = 0.6;
    }
    albedo *= tex.color;
    rough = tex.rough;
    // The lean of the photographs: along the circuit's across and along, as they are mapped.
    let ground = kind >= TRACK_ASPHALT && kind <= TRACK_RUNOFF;
    if ground {
        // Directions in render axes of growing `across` and `along`, from the screen's derivatives.
        let dx = dpdx(in.rel);
        let dy = dpdy(in.rel);
        let det = gx.x * gy.y - gx.y * gy.x;
        if abs(det) > 1e-12 {
            let d_across = normalize((dx * gy.y - dy * gx.y) / det);
            let d_along = normalize((dy * gx.x - dx * gy.x) / det);
            let l = tex.lean + lean;
            n = normalize(n + (d_across * l.x + d_along * l.y) * 0.6);
        }
    }

    let sun = frame.sun_dir.xyz;
    let contact = body_contact(in.rel);
    var shadow = body_shadow(in.rel) * cloud_shadow(in.rel) * terrain_shadow(in.rel) * contact;
    // A panel's fourth number is its own (a length, a place), not how open it is.
    let ao = tex.ao * contact * select(in.surface.w, 1.0, kind >= TRACK_CROWD);
    let ndl = max(dot(n, sun), 0.0);
    let sunlight = light_at(in.rel);
    var col = albedo * (sunlight * ndl * shadow + sky_light(n, in.rel) * ao);
    if frame.fire_count.x > 0.0 {
        col += albedo * fire_light(in.rel, n);
    }
    if sheen > 0.0 {
        col += sunlight * mat_sheen(n, v, sun, rough) * ndl * shadow * sheen * 0.35;
    }
    // The floodlights, by night (what all the towers give this point, worked out with the mesh),
    // and the lamps of the car the view follows.
    let flood = TRACK_FLOOD * in.light.x * track_night();
    col += albedo * flood * select(0.7, max(n.y, 0.0), ground) * select(1.0, 0.6, kind == TRACK_GRASS);
    col += albedo * track_car_lamps(in.rel, n);
    // Wet: the paved ground darker (water fills its grain), a film of water over it that mirrors
    // the sky (most at a glance, as water does), standing in the hollows; the sun's glare in it,
    // and the floodlights'.
    let wet = object.tint.x * select(0.0, 1.0, kind == TRACK_ASPHALT || kind == TRACK_RUNOFF || kind == TRACK_KERB || kind == TRACK_CONCRETE);
    if wet > 0.0 {
        let water = track_water(uv, kind, n, level, v);
        let puddle = water.puddle;
        let film = water.film;
        col *= 1.0 - 0.72 * wet;
        let nw = water.n;
        let away = reflect(-v, nw);
        var sky_in_water = water_sky(away, in.rel, 0.03 + 0.10 * (1.0 - puddle));
        // Low over the horizon the water mirrors the land and its trees, not the sky: dark.
        let land = vec3<f32>(0.03, 0.045, 0.02) * (sunlight * max(sun.y, 0.0) * 0.6 + sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel));
        sky_in_water = mix(sky_in_water, land, smoothstep(0.11, 0.035, away.y));
        // And the body the view follows, where the mirrored ray meets it: its dark underside.
        if frame.body_pos.w > -1.0 {
            let o = in.rel - frame.body_pos.xyz;
            let lo = vec3<f32>(dot(o, frame.body_fwd.xyz), dot(o, frame.body_right.xyz), dot(o, frame.body_down.xyz) + 0.25);
            let ld = vec3<f32>(dot(away, frame.body_fwd.xyz), dot(away, frame.body_right.xyz), dot(away, frame.body_down.xyz));
            // As a rounded solid of its size: the longer the ray's way through it, the darker.
            let half = vec3<f32>(2.3, 1.0, 0.62);
            let q = lo / half;
            let d = ld / half;
            let a = dot(d, d);
            let b = dot(q, d);
            let disc = b * b - a * (dot(q, q) - 1.0);
            if disc > 0.0 && b < 0.0 {
                let chord = 2.0 * sqrt(disc) / a;
                sky_in_water *= 1.0 - 0.88 * smoothstep(0.0, 1.6, chord);
            }
        }
        sky_in_water = sky_in_water * ao + flood * 0.08;
        col = mix(col, sky_in_water, water.mirror);
        col += sunlight * mat_sheen(nw, v, sun, 0.22 - 0.12 * puddle) * max(dot(nw, sun), 0.0) * shadow * film * 2.0;
        col += track_glints(in.rel, nw, v, 0.26 - 0.14 * puddle) * film * 1.6;
    }
    if kind == TRACK_PROP || kind == TRACK_BELT || kind == TRACK_BOARD || kind == TRACK_LAMP {
        // Paint and metal: a clear coat's highlight and the sky in it, as gloss says.
        let h = normalize(sun + v);
        let sharp = exp2(4.0 + 7.0 * min(gloss, 1.0));
        let coat = 0.04 + 0.96 * pow(1.0 - max(dot(v, h), 0.0), 5.0);
        col += sunlight * pow(max(dot(n, h), 0.0), sharp) * (sharp + 8.0) / 25.13 * coat * gloss * ndl * shadow;
        let fres = (0.04 + 0.96 * pow(1.0 - max(dot(n, v), 0.0), 5.0)) * gloss;
        col = mix(col, sky_seen(reflect(-v, n)) * ao, fres * 0.4);
    }
    return vec4<f32>(apply_fog(col, in.rel), 1.0);
}

@fragment
fn track_fragment(in: TrackOut) -> @location(0) vec4<f32> {
    return track_shade(in);
}

// The second pass over the wet paved ground: what the scene has where the film of water mirrors
// it, laid over the sky the first pass put there (premultiplied by how much of a mirror the film
// is and how much the ray found).
@fragment
fn track_wet_fragment(in: TrackOut) -> @location(0) vec4<f32> {
    let kind = i32(in.surface.x + 0.5);
    let uv = vec2<f32>(in.surface.y, in.surface.z);
    let level = normalize(in.normal);
    let v = normalize(-in.rel);
    // The grain of the ground under the film, as the first pass has it: it is what breaks a
    // lamp's image into a streak.
    var n = level;
    let gx = dpdx(uv);
    let gy = dpdy(uv);
    let dx = dpdx(in.rel);
    let dy = dpdy(in.rel);
    let det = gx.x * gy.y - gx.y * gy.x;
    if abs(det) > 1e-12 {
        let dist = length(in.rel);
        let scatter = (vnoise(uv / 16.0, 4096) * 0.6 + vnoise(uv / 9.0 + 7.3, 4096) * 0.4) * 8.0;
        let tex = mat_planar(select(M_ASPHALT, M_CONCRETE, kind == TRACK_KERB || kind == TRACK_CONCRETE), uv, gx, gy, dist, scatter);
        let d_across = normalize((dx * gy.y - dy * gx.y) / det);
        let d_along = normalize((dy * gx.x - dx * gy.x) / det);
        n = normalize(n + (d_across * tex.lean.x + d_along * tex.lean.y) * 0.6);
    }
    let water = track_water(uv, kind, n, level, v);
    if water.mirror < 0.004 {
        discard;
    }
    let seen = track_mirrored(in.rel, reflect(-v, water.n), in.position.xy);
    let a = water.mirror * seen.a;
    if a < 0.002 {
        discard;
    }
    return vec4<f32>(seen.rgb * a, a);
}
