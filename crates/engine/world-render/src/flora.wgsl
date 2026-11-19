// The trees in the scene: their meshes near (vegetation.rs), their pictures from far (impostors.rs)
// and their shadows on the ground. Appended to the world's shaders after flora_surface.wgsl, so the
// frame, the sky, the haze, the noise and the trees' surfaces are in reach.
//
// A draw is the instances of one species at one level of detail (`chunk.wrap`: x the level, 0 the
// full mesh, 1 the simpler one, 2 the picture; y and z the distances over the ground at which the
// first gives way to the second and the second to the third; w the species). Each tree changes
// level at distances of its own, over a band in which both levels draw it, each on its share of the
// pixels and of their samples (`lod_share`, `coverage`): it dissolves from one into the other
// instead of popping.

// The trees' pictures from far (impostors.rs): per species a layer of IMPOSTOR_FRAMES x
// IMPOSTOR_FRAMES views over the upper half of the sky round it (hemi-octahedral), each the unit
// model seen from that side within IMPOSTOR_RADIUS of its middle. Colour and outline; the normal
// (model axes, as 0..1) and how far in front of or behind the view's middle plane it is.
@group(2) @binding(6) var impostor_colour: texture_2d_array<f32>;
@group(2) @binding(7) var impostor_detail: texture_2d_array<f32>;
@group(2) @binding(9) var impostor_sampler: sampler;

// `impostors.rs` writes these two.
const IMPOSTOR_FRAMES: f32 = 8.0;
const IMPOSTOR_RADIUS: f32 = 0.62;
// The middle of the views, in the unit model.
const IMPOSTOR_CENTRE: vec3<f32> = vec3<f32>(0.0, 0.5, 0.0);
// The share of the distance at which a tree changes level over which it dissolves.
const LOD_BAND: f32 = 0.12;

struct FloraIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,   // rgb = colour (the leaves' or the bark's tint), a = openness
    @location(3) place: vec4<f32>,   // the instance: xyz in the tile, w = height
    @location(4) tint: vec4<f32>,    // the instance: tone, width, slope along x and z
    @location(5) tex: vec4<f32>,     // u, v, layer, how much the wind moves it
    @location(6) crown: vec4<f32>,   // place in the crown (crown radii), phase
};

struct FloraOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) tex: vec3<f32>,
    @location(4) crown: vec3<f32>,
    // The sun's way in crown radii (model axes).
    @location(5) sun_q: vec3<f32>,
    // The share of the pixels this level draws: from x to y of 0..1; z = the species.
    @location(6) @interpolate(flat) share: vec3<f32>,
};

// What every level does with an instance: where it is, how it is turned and sized, its tone.
struct Tree {
    base: vec3<f32>,
    dist: f32,
    turn: vec2<f32>,      // cos, sin
    scale: vec3<f32>,     // width * height, height, width * height
    hue: f32,
    brightness: f32,
    share: vec2<f32>,
    // How much it has shrunk into the canopy at the end of the trees' range.
    fade: f32,
};

fn tree_of(place: vec4<f32>, tint: vec4<f32>) -> Tree {
    var t: Tree;
    t.base = place.xyz + chunk.offset.xyz;
    t.dist = length(t.base.xz);
    t.hue = fract(tint.x);
    t.brightness = floor(tint.x) / 32.0;
    let angle = fract(tint.x * 13.7) * 6.2831853;
    t.turn = vec2<f32>(cos(angle), sin(angle));
    t.scale = vec3<f32>(tint.y * place.w, place.w, tint.y * place.w);
    t.share = lod_share(t.dist, t.hue, fract(tint.x * 5.3));
    t.fade = 1.0 - smoothstep(frame.misc.z * 0.75, frame.misc.z, t.dist);
    return t;
}

// The share of the pixels the level of this draw gives a tree at `dist`: an interval of 0..1, the
// levels' end to end. Each tree changes at distances of its own (`a`, `b` in 0..1).
fn lod_share(dist: f32, a: f32, b: f32) -> vec2<f32> {
    let first = chunk.wrap.y * (0.82 + 0.36 * a);
    let second = chunk.wrap.z * (0.9 + 0.2 * b);
    let t1 = saturate((dist - first * (1.0 - LOD_BAND / 2.0)) / (first * LOD_BAND));
    let t2 = saturate((dist - second * (1.0 - LOD_BAND / 2.0)) / (second * LOD_BAND));
    let level = i32(chunk.wrap.x + 0.5);
    if level == 0 {
        return vec2<f32>(0.0, 1.0 - t1);
    } else if level == 1 {
        return vec2<f32>(1.0 - t1, 1.0 - t1 * t2);
    }
    return vec2<f32>(1.0 - t1 * t2, 1.0);
}

// Model axes to the world's (render axes, relative to the tree's foot).
fn tree_to_world(t: Tree, p: vec3<f32>) -> vec3<f32> {
    let s = p * t.scale;
    return vec3<f32>(s.x * t.turn.x - s.z * t.turn.y, s.y, s.x * t.turn.y + s.z * t.turn.x);
}

// The world's axes to the model's, for a point relative to the tree's foot.
fn world_to_tree(t: Tree, w: vec3<f32>) -> vec3<f32> {
    let r = vec3<f32>(w.x * t.turn.x + w.z * t.turn.y, w.y, -w.x * t.turn.y + w.z * t.turn.x);
    return r / t.scale;
}

// A direction of the world in model axes, as a direction (unit).
fn direction_to_tree(t: Tree, w: vec3<f32>) -> vec3<f32> {
    return normalize(world_to_tree(t, w));
}

// A normal of the model in the world's axes.
fn normal_to_world(t: Tree, n: vec3<f32>) -> vec3<f32> {
    let s = n / t.scale * t.scale.y;
    return normalize(vec3<f32>(s.x * t.turn.x - s.z * t.turn.y, s.y, s.x * t.turn.y + s.z * t.turn.x));
}

// The sun's way through the crown of a species, in crown radii (model axes): what the crown's own
// shade is worked out along.
fn sun_in_crown(t: Tree, species: i32) -> vec3<f32> {
    let crown = SPECIES_CROWN[species];
    return normalize(direction_to_tree(t, frame.sun_dir.xyz) / vec3<f32>(crown.y, crown.z, crown.y));
}

// How the wind moves a point of a tree `sway` (0 at its foot, 1 or so at its top) with `phase`, metres.
fn wind_at(t: Tree, sway: f32, phase: f32) -> vec3<f32> {
    let speed = max(frame.sea.x, 1.0);
    let time = frame.sun_dir.w;
    let way = vec3<f32>(frame.sea_more.x, 0.0, frame.sea_more.y);
    let own = t.hue * 23.0;
    // The whole tree leans with the gusts, and its branches and leaves flutter.
    let lean = 0.0016 * pow(speed, 1.5) * (0.65 + 0.35 * sin(time * (0.9 + 0.3 * t.hue) + own) + 0.15 * sin(time * 2.3 + own * 1.7));
    let flutter = 0.0012 * speed * sin(time * (6.0 + 3.0 * phase) + phase * 40.0 + own);
    let side = vec3<f32>(-way.z, 0.4, way.x);
    return (way * lean * sway * sway + side * flutter * sway) * t.scale.y;
}

@vertex
fn flora_vertex(in: FloraIn) -> FloraOut {
    var o: FloraOut;
    let t = tree_of(in.place, in.tint);
    let species = i32(chunk.wrap.w + 0.5);
    let shown = select(1.0, 0.0, t.share.y <= t.share.x);
    let rel = t.base + (tree_to_world(t, in.position) + wind_at(t, in.tex.w, in.crown.w)) * shown;
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.normal = normal_to_world(t, in.normal);
    let tone = mix(vec3<f32>(1.16, 1.05, 0.78), vec3<f32>(0.84, 0.97, 1.08), t.hue) * t.brightness * 1.25;
    // Bark keeps its colour; leaves take the tree's tone.
    let leafy = select(1.0, 0.0, in.tex.z >= BARK_LAYER - 0.5);
    o.color = vec4<f32>(in.color.rgb * mix(vec3<f32>(1.0), tone, leafy), in.color.a);
    o.tex = in.tex.xyz;
    o.crown = in.crown.xyz;
    o.sun_q = sun_in_crown(t, species);
    o.share = vec3<f32>(t.share, f32(species));
    return o;
}

// A number in 0..1 for a pixel, spread evenly (interleaved gradient noise, Jimenez 2014).
fn pixel_noise(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(floor(p), vec2<f32>(0.06711056, 0.00583715))));
}

// The samples of a pixel a tree covers: `cover` of them, and only those whose number falls in
// its level's share (the next level takes the others). With one sample per pixel the pixels are
// shared out instead.
fn coverage(p: vec2<f32>, cover: f32, share: vec2<f32>) -> u32 {
    let n = pixel_noise(p);
    let m = pixel_noise(p + vec2<f32>(37.0, 17.0));
    var mask = 0u;
    for (var i = 0u; i < 4u; i++) {
        let s = fract(n + f32(i) * 0.25);
        let c = fract(m + f32(i) * 0.25);
        if c < cover && s >= share.x && s < share.y {
            mask |= 1u << i;
        }
    }
    return mask;
}

// The light of a point of a tree: the sun through the air and through the crown (its own shade),
// what its leaves let through towards the eye, their sheen, the sky's light on it as open as it
// is, and the fires'. Hazed. `sunlit` is how much of the sun the clouds and the relief let reach
// the tree (`sun_on_tree`).
fn flora_light(rel: vec3<f32>, s: FloraSurface, open: f32, q: vec3<f32>, sun_q: vec3<f32>, species: i32, sunlit: f32) -> vec3<f32> {
    let dist = length(rel);
    let v = -rel / dist;
    let sun = frame.sun_dir.xyz;
    let n = s.normal;
    // The sun's way through the crown's ellipsoid from this point: its own shade.
    let b = dot(q, sun_q);
    let c = dot(q, q) - 1.0;
    let disc = b * b - c;
    var inside = 0.0;
    if disc > 0.0 {
        let r = sqrt(disc);
        inside = max(-b + r, 0.0) - max(-b - r, 0.0);
    }
    let crown = SPECIES_CROWN[species];
    let own = exp(-crown.w * inside);
    let lit = light_at(rel) * body_shadow(rel) * sunlit;
    let ndl = dot(n, sun);
    // Leaves wrap the light round the crown; bark does not.
    let wrap = mix(0.0, 0.35, step(0.001, s.through));
    let diffuse = saturate((ndl + wrap) / (1.0 + wrap)) * own;
    // Light through the leaves: towards the eye when the sun is behind them, a little all round.
    let light = SPECIES_LIGHT[species];
    let behind = pow(saturate(dot(-v, sun)), 4.0) * 0.8 + 0.25 * saturate(-ndl);
    let through = light.rgb * s.through * behind * sqrt(own);
    // The waxy sheen of the leaves' upper side.
    let h = normalize(sun + v);
    let sheen = light.w * pow(saturate(dot(n, h)), 20.0) * 0.25 * own * step(0.001, s.through);
    let ambient = sky_light(n, rel) * (0.12 + 0.88 * open);
    var col = s.albedo * (lit * (diffuse + through) + ambient) + lit * sheen * 0.04;
    if frame.fire_count.x > 0.0 {
        col += s.albedo * fire_light(rel, n) * (0.4 + 0.6 * open);
    }
    return apply_fog(col, rel);
}

struct FloraFragment {
    @location(0) color: vec4<f32>,
    @builtin(sample_mask) mask: u32,
};

// The leaves' outline sharpened to a pixel at any distance (alpha to coverage, Golus 2017), and the
// tree's share of its level.
@fragment
fn flora_fragment(in: FloraOut) -> FloraFragment {
    let s = flora_surface(in.tex.xy, in.tex.z, in.color.rgb, normalize(in.normal));
    let cover = saturate((s.alpha - 0.5) / max(fwidth(s.alpha), 1e-4) + 0.5);
    var o: FloraFragment;
    o.mask = coverage(in.position.xy, cover, in.share.xy);
    if o.mask == 0u {
        return o;
    }
    o.color = vec4<f32>(flora_light(in.rel, s, in.color.a, in.crown, in.sun_q, i32(in.share.z + 0.5), cloud_shadow(in.rel) * coarse_terrain_shadow(in.rel)), 1.0);
    return o;
}

// A tree mirrored in the sea (`reflection.rs`): cut where its leaves are, one sample.
@fragment
fn flora_mirror_fragment(in: FloraOut) -> @location(0) vec4<f32> {
    let s = flora_surface(in.tex.xy, in.tex.z, in.color.rgb, normalize(in.normal));
    if (coverage(in.position.xy, step(0.5, s.alpha), in.share.xy) & 1u) == 0u {
        discard;
    }
    return vec4<f32>(flora_light(in.rel, s, in.color.a, in.crown, in.sun_q, i32(in.share.z + 0.5), cloud_shadow(in.rel) * coarse_terrain_shadow(in.rel)), 1.0);
}

// ---------------------------------------------------------------- the pictures from far

// The view (0..1 each way) of the hemi-octahedral grid a direction from the middle (y up) is
// seen from; under the horizon, the horizon's.
fn impostor_place(d: vec3<f32>) -> vec2<f32> {
    let u = normalize(vec3<f32>(d.x, max(d.y, 0.0), d.z));
    let p = u.xz / (abs(u.x) + abs(u.y) + abs(u.z));
    return vec2<f32>(p.x + p.y, p.x - p.y) * 0.5 + 0.5;
}

// The direction a view of the grid was taken from.
fn impostor_direction(cell: vec2<f32>) -> vec3<f32> {
    let g = (cell + 0.5) / IMPOSTOR_FRAMES * 2.0 - 1.0;
    let p = vec2<f32>(g.x + g.y, g.x - g.y) * 0.5;
    return normalize(vec3<f32>(p.x, 1.0 - abs(p.x) - abs(p.y), p.y));
}

// Right and up of the picture taken from `d`.
fn impostor_axes(d: vec3<f32>) -> mat2x3<f32> {
    let right = select(normalize(cross(vec3<f32>(0.0, 1.0, 0.0), d)), vec3<f32>(1.0, 0.0, 0.0), d.y > 0.999);
    return mat2x3<f32>(right, cross(d, right));
}

struct ImpostorOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    // The point of the square, model axes, and the way to the eye from the views' middle.
    @location(1) point: vec3<f32>,
    @location(2) @interpolate(flat) view: vec3<f32>,
    // Where the eye's ray crosses each of the four views round the one it is seen from (their
    // pictures' place, 0..1): worked out at the corners and interpolated, near enough for views 20°
    // apart.
    @location(3) uv01: vec4<f32>,
    @location(4) uv23: vec4<f32>,
    // The first of the four views in the grid (xy) and how far towards the others (zw).
    @location(5) @interpolate(flat) cells: vec4<f32>,
    // The tree's turn (cos, sin), its width times height and height.
    @location(6) @interpolate(flat) turn: vec4<f32>,
    // Its tone (brightness, hue), how much of the sun reaches it (the clouds', the relief's shade).
    @location(7) @interpolate(flat) tone: vec3<f32>,
    @location(8) @interpolate(flat) sun_q: vec3<f32>,
    @location(9) @interpolate(flat) share: vec3<f32>,
};

// How much of the sun reaches a tree, by the clouds and the relief, at its middle: for the pictures
// from far, once per corner rather than at every pixel (a mesh, with hundreds of vertices, pays
// less for it at its pixels).
fn sun_on_tree(t: Tree) -> f32 {
    let middle = t.base + vec3<f32>(0.0, t.scale.y * 0.6, 0.0);
    return cloud_shadow(middle) * coarse_terrain_shadow(middle);
}

// A square facing the eye round the middle of the tree, as big as the views. `in.position.xz` is
// its corner (`vegetation::square`).
@vertex
fn impostor_vertex(in: TreeIn) -> ImpostorOut {
    var o: ImpostorOut;
    let t = tree_of(in.place, in.tint);
    let species = i32(chunk.wrap.w + 0.5);
    let shown = select(1.0, 0.0, t.share.y <= t.share.x) * t.fade;
    let middle = t.base + tree_to_world(t, IMPOSTOR_CENTRE) + wind_at(t, 0.35, 0.0);
    let to_eye = normalize(-middle);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), to_eye) + vec3<f32>(1e-5, 0.0, 0.0));
    let up = cross(to_eye, right);
    let radius = IMPOSTOR_RADIUS * max(t.scale.x, t.scale.y) * shown;
    // Brought a radius towards the eye, so that the ground before the tree does not cut it.
    let rel = middle + (right * in.position.x - up * in.position.z) * radius + to_eye * radius * 0.5;
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    let point = world_to_tree(t, rel - t.base);
    let eye = world_to_tree(t, -t.base);
    o.point = point;
    o.view = normalize(eye - IMPOSTOR_CENTRE);
    let g = clamp(impostor_place(o.view) * IMPOSTOR_FRAMES - 0.5, vec2<f32>(0.0), vec2<f32>(IMPOSTOR_FRAMES - 1.0));
    let first = floor(g);
    o.cells = vec4<f32>(first, g - first);
    var uv: array<vec2<f32>, 4>;
    for (var k = 0; k < 4; k++) {
        let cell = min(first + vec2<f32>(f32(k & 1), f32(k >> 1)), vec2<f32>(IMPOSTOR_FRAMES - 1.0));
        let d = impostor_direction(cell);
        let axes = impostor_axes(d);
        // Where the ray from the eye through this corner crosses the view's middle plane.
        let ray = point - eye;
        let x = eye + ray * (dot(IMPOSTOR_CENTRE - eye, d) / min(dot(ray, d), -1e-4)) - IMPOSTOR_CENTRE;
        uv[k] = vec2<f32>(dot(x, axes[0]), -dot(x, axes[1])) / (2.0 * IMPOSTOR_RADIUS) + 0.5;
    }
    o.uv01 = vec4<f32>(uv[0], uv[1]);
    o.uv23 = vec4<f32>(uv[2], uv[3]);
    o.turn = vec4<f32>(t.turn, t.scale.x, t.scale.y);
    o.tone = vec3<f32>(t.brightness, t.hue, sun_on_tree(t));
    o.sun_q = sun_in_crown(t, species);
    o.share = vec3<f32>(t.share, f32(species));
    return o;
}

// What the eye sees of the tree: the four views blended by how near each is (colour and outline,
// normal in model axes, how far before or behind the views' middle plane).
fn impostor_sample(in: ImpostorOut, species: i32) -> array<vec4<f32>, 2> {
    // The level to read at, by how many texels of a view a pixel spans.
    let side = f32(textureDimensions(impostor_colour).x) / IMPOSTOR_FRAMES;
    let level = log2(max(max(length(dpdx(in.uv01.xy)), length(dpdy(in.uv01.xy))) * side, 1.0));
    let f = in.cells.zw;
    let uvs = array<vec2<f32>, 4>(in.uv01.xy, in.uv01.zw, in.uv23.xy, in.uv23.zw);
    var colour = vec4<f32>(0.0);
    var detail = vec4<f32>(0.0);
    for (var k = 0; k < 4; k++) {
        let o = vec2<f32>(f32(k & 1), f32(k >> 1));
        let w = mix(1.0 - f.x, f.x, o.x) * mix(1.0 - f.y, f.y, o.y);
        let uv = uvs[k];
        if w < 0.01 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
            continue;
        }
        let cell = min(in.cells.xy + o, vec2<f32>(IMPOSTOR_FRAMES - 1.0));
        let at = (cell + clamp(uv, vec2<f32>(0.5 / side), vec2<f32>(1.0 - 0.5 / side))) / IMPOSTOR_FRAMES;
        let c = textureSampleLevel(impostor_colour, impostor_sampler, at, species, level);
        let n = textureSampleLevel(impostor_detail, impostor_sampler, at, species, level);
        colour += vec4<f32>(c.rgb * c.a, c.a) * w;
        detail += vec4<f32>(n.xyz * 2.0 - 1.0, n.w) * c.a * w;
    }
    return array<vec4<f32>, 2>(colour, detail);
}

// The light of the tree seen at a pixel, and how much of the pixel it covers.
fn impostor_shade(in: ImpostorOut, seen: array<vec4<f32>, 2>, species: i32) -> vec3<f32> {
    let colour = seen[0].rgb / max(seen[0].a, 1e-4);
    var t: Tree;
    t.turn = in.turn.xy;
    t.scale = vec3<f32>(in.turn.z, in.turn.w, in.turn.z);
    let tone = mix(vec3<f32>(1.16, 1.05, 0.78), vec3<f32>(0.84, 0.97, 1.08), in.tone.y) * in.tone.x * 1.25;
    var s: FloraSurface;
    s.albedo = colour * tone;
    s.alpha = seen[0].a;
    s.normal = normal_to_world(t, normalize(seen[1].xyz + vec3<f32>(0.0, 1e-4, 0.0)));
    s.through = 0.6;
    // The point seen, in the crown: before or behind the square along the way to the eye.
    let depth = seen[1].w / max(seen[0].a, 1e-4);
    let at = in.point + in.view * (depth * 2.0 - 1.0) * IMPOSTOR_RADIUS;
    let crown = SPECIES_CROWN[species];
    let q = (at - vec3<f32>(0.0, crown.x, 0.0)) / vec3<f32>(crown.y, crown.z, crown.y);
    return flora_light(in.rel, s, crown_open(q), q, in.sun_q, species, in.tone.z);
}

@fragment
fn impostor_fragment(in: ImpostorOut) -> FloraFragment {
    let species = i32(in.share.z + 0.5);
    let seen = impostor_sample(in, species);
    var o: FloraFragment;
    // Far off a tree is less than a pixel: its outline is its share of the pixel's samples.
    o.mask = coverage(in.position.xy, saturate(seen[0].a * 1.15 - 0.05), in.share.xy);
    // Most of the square is the sky round the tree: nothing to light there.
    if o.mask == 0u {
        return o;
    }
    o.color = vec4<f32>(impostor_shade(in, seen, species), 1.0);
    return o;
}

@fragment
fn impostor_mirror_fragment(in: ImpostorOut) -> @location(0) vec4<f32> {
    let species = i32(in.share.z + 0.5);
    let seen = impostor_sample(in, species);
    if (coverage(in.position.xy, step(0.5, seen[0].a), in.share.xy) & 1u) == 0u {
        discard;
    }
    return vec4<f32>(impostor_shade(in, seen, species), 1.0);
}

// ---------------------------------------------------------------- shadows of the trees
//
// Each tree's shadow is its own outline seen from the sun (the view of its picture nearest the
// sun's way), laid on the ground along the sunlight: a patch round where the shadow of the crown
// falls, drawn after everything solid and blended over it. It assumes the ground under the tree is
// the plane through its foot, with the slope of the instance.

struct TreeShadowOut {
    @builtin(position) position: vec4<f32>,
    // The point of the ground, model axes, and the sun's way there.
    @location(0) point: vec3<f32>,
    @location(1) @interpolate(flat) sun: vec3<f32>,
    // x = how dark, y = the species, zw = the view of the picture nearest the sun.
    @location(2) @interpolate(flat) more: vec4<f32>,
};

@vertex
fn tree_shadow_vertex(in: TreeIn) -> TreeShadowOut {
    var o: TreeShadowOut;
    let t = tree_of(in.place, in.tint);
    let sun = frame.sun_dir.xyz;
    let range = min(frame.misc.z, TREE_SHADOW_RANGE);
    var strength = 0.72 * (1.0 - smoothstep(range * 0.6, range, length(t.base))) * smoothstep(0.25, 0.6, frame.sun_color.w) * smoothstep(0.04, 0.2, sun.y);
    // Most of the trees of a run are too far for a shadow: they skip the clouds.
    if strength < 0.01 {
        o.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return o;
    }
    strength *= smoothstep(0.42, 0.95, cloud_shadow(t.base)) * luma(haze_through(t.base));
    let level = max(length(sun.xz), 1e-4);
    let away = -sun.xz / level;
    let across = vec2<f32>(-away.y, away.x);
    // The shadow of the sphere round the views: as far away from the foot as the middle's height
    // reaches along the sunlight, longer by how low the sun is (cut short when it is very low).
    let rise = max(sun.y, 0.3);
    let reach = min(level / rise, 3.2);
    let radius = IMPOSTOR_RADIUS * max(t.scale.x, t.scale.y) * step(0.01, strength);
    let middle = away * (t.scale.y * IMPOSTOR_CENTRE.y * reach);
    let xz = middle + away * in.position.x * radius * min(1.0 / rise, 3.4) + across * in.position.z * radius;
    let y = dot(in.tint.zw, xz) + 0.3 + length(t.base) * 0.0008;
    let rel = t.base + vec3<f32>(xz.x, y, xz.y);
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    // The point of the ground the sun's ray through it would come from, in model axes.
    o.point = world_to_tree(t, rel - t.base);
    o.sun = direction_to_tree(t, sun);
    o.more = vec4<f32>(strength, chunk.wrap.w, impostor_place(o.sun));
    return o;
}

@fragment
fn tree_shadow_fragment(in: TreeShadowOut) -> @location(0) vec4<f32> {
    let species = i32(in.more.y + 0.5);
    let cell = clamp(floor(in.more.zw * IMPOSTOR_FRAMES), vec2<f32>(0.0), vec2<f32>(IMPOSTOR_FRAMES - 1.0));
    let d = impostor_direction(cell);
    let axes = impostor_axes(d);
    // Up the sunlight from the ground to the view's middle plane.
    let along = dot(IMPOSTOR_CENTRE - in.point, d) / max(dot(in.sun, d), 1e-3);
    let x = in.point + in.sun * along - IMPOSTOR_CENTRE;
    let uv = vec2<f32>(dot(x, axes[0]), -dot(x, axes[1])) / (2.0 * IMPOSTOR_RADIUS) + 0.5;
    let inside = all(uv > vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0));
    // Blurred a little: a crown's shadow has the sun's penumbra and the light through its gaps.
    let a = textureSampleLevel(impostor_colour, impostor_sampler, (cell + clamp(uv, vec2<f32>(0.01), vec2<f32>(0.99))) / IMPOSTOR_FRAMES, species, 1.5).a;
    return vec4<f32>(0.0, 0.004, 0.012, in.more.x * a * select(0.0, 1.0, inside));
}

// ---------------------------------------------------------------- grass
//
// Tufts round the camera (grass.rs): `chunk.wrap.x` is the distance they reach. Each shrinks into
// the ground towards it, so the ring they make has no edge. Coloured between a green and a straw
// by how dry the ground is (`tint.z`), lit as the ground is.

// The species whose light the grass takes (`species::Species::Shrub`): what it lets through.
const GRASS_LIGHT: i32 = 14;

@vertex
fn grass_vertex(in: FloraIn) -> FloraOut {
    var o: FloraOut;
    let t = tree_of(in.place, vec4<f32>(in.tint.xy, 0.0, 0.0));
    let shrink = 1.0 - smoothstep(chunk.wrap.x * 0.6, chunk.wrap.x, length(t.base));
    let rel = t.base + (tree_to_world(t, in.position) + wind_at(t, in.tex.w * 6.0, in.crown.w + t.hue)) * shrink;
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.normal = normal_to_world(t, in.normal);
    let green = vec3<f32>(0.045, 0.075, 0.022);
    let straw = vec3<f32>(0.15, 0.12, 0.065);
    o.color = vec4<f32>(mix(green, straw, in.tint.z) * t.brightness, in.color.a);
    o.tex = in.tex.xyz;
    o.crown = in.crown.xyz;
    o.sun_q = vec3<f32>(0.0, 1.0, 0.0);
    o.share = vec3<f32>(0.0, 1.0, f32(GRASS_LIGHT));
    return o;
}

@fragment
fn grass_fragment(in: FloraOut) -> FloraFragment {
    let s = flora_surface(in.tex.xy, in.tex.z, in.color.rgb, normalize(in.normal));
    let cover = saturate((s.alpha - 0.5) / max(fwidth(s.alpha), 1e-4) + 0.5);
    var o: FloraFragment;
    o.mask = coverage(in.position.xy, cover, in.share.xy);
    if o.mask == 0u {
        return o;
    }
    o.color = vec4<f32>(flora_light(in.rel, s, in.color.a, in.crown, in.sun_q, GRASS_LIGHT, cloud_shadow(in.rel) * coarse_terrain_shadow(in.rel)), 1.0);
    return o;
}
