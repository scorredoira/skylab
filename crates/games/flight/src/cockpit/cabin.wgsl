// A cockpit seen from the pilot's seat, whichever kind's: the vertex and fragment shaders of the
// `cabin` pipeline and the fragment shader of `cabin_glass`. Appended to the world's shaders
// (`world_render::shader_source`; it uses their `frame`, `object`, `ObjectIn`, `ObjectOut`,
// `sky_seen` and `cut_noise`; `frame.body_*` are the aircraft's axes).

// The cabin's canvas (the instruments) and photographs, group 2 of the interior pipeline.
@group(2) @binding(0) var cabin_canvas: texture_2d<f32>;
@group(2) @binding(1) var cabin_colour: texture_2d_array<f32>;
@group(2) @binding(2) var cabin_detail: texture_2d_array<f32>;
@group(2) @binding(3) var cabin_sampler: sampler;
// How much sky each point sees through the windows (`SkyVolume`): r the open fraction, gba the mean
// open direction (body axes) times 2 plus 0.5. Baked from `cabin.sky.xyz` every `cabin.sky.w`.
@group(2) @binding(4) var cabin_sky: texture_3d<f32>;

// The light a surface of the cabin at `p` facing `n` (body axes) gets through the windows, 0 to 1.
fn cabin_open(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let size = vec3<f32>(textureDimensions(cabin_sky));
    // A little off the surface, so the samples behind it count for little.
    let cell = (p + n * 0.035 - cabin.sky.xyz) / cabin.sky.w + 0.5;
    let s = textureSampleLevel(cabin_sky, cabin_sampler, clamp(cell, vec3<f32>(0.5), size - 0.5) / size, 0.0);
    return clamp(s.r + dot(s.gba - 0.5, n), 0.0, 1.0);
}

// What each material (`cockpit::Material`, by its number) takes of the photographs: which
// (0 plastic, 1 leather, 2 fabric, 3 carpet, 4 rubber, 5 painted metal), metres per repeat, how much
// of its grain shows in the colour, how far its relief bends the light, how much its roughness map
// takes off its gloss, and how much of the light it reflects head on (a metal reflects its colour).
// Plain 0, plastic 1, leather 2, fabric 3, carpet 4, rubber 5, painted metal 6, (screen 7, lamp 8),
// printed 9 (the panel's crinkle paint), vinyl 10 (textured trim), chrome 11, textured 12 (a
// material with its own picture, `cabin_picture`: of the photographs it takes nothing).
const CABIN_LAYER = array<i32, 13>(0, 0, 1, 2, 3, 4, 5, 0, 0, 0, 1, 5, 0);
const CABIN_SCALE = array<f32, 13>(1.0, 0.30, 0.16, 0.14, 0.30, 0.10, 0.25, 1.0, 1.0, 0.05, 0.09, 0.40, 1.0);
const CABIN_GRAIN = array<f32, 13>(0.0, 0.6, 0.45, 0.8, 0.9, 0.2, 0.25, 0.0, 0.0, 0.25, 0.2, 0.0, 0.0);
const CABIN_BUMP = array<f32, 13>(0.0, 0.6, 0.8, 0.8, 0.8, 0.5, 0.5, 0.0, 0.0, 1.1, 0.9, 0.12, 0.0);
const CABIN_ROUGH = array<f32, 13>(0.0, 0.5, 0.6, 0.3, 0.2, 0.6, 0.5, 0.0, 0.0, 0.5, 0.6, 0.3, 0.0);
const CABIN_F0 = array<f32, 13>(0.04, 0.045, 0.04, 0.03, 0.03, 0.035, 0.05, 0.04, 0.04, 0.045, 0.045, 1.0, 0.04);
// Each projection of a photograph by its share, from the axes the surface faces (`CABIN_TRIPLANAR`:
// the three blended by how much it faces each, else only the one it faces most).
const CABIN_TRIPLANAR: bool = true;
// What is printed in the cockpit (`CabinSpec::print`), in one picture of `cabin.print.xy` points:
// the panel's part (y, z from `cabin.panel_print.xy` over `.zw` metres) at its top, laid along x
// on what faces aft; the floor's (y, x, forward up) `cabin.print.z` points down, `cabin.print.w`
// square, laid along z on what faces up.
@group(2) @binding(5) var cabin_print: texture_2d<f32>;

fn panel_print_uv(p: vec3<f32>) -> vec2<f32> {
    return (p.yz - cabin.panel_print.xy) / cabin.panel_print.zw * vec2<f32>(1.0, cabin.panel_print.w / cabin.panel_print.z * cabin.print.x / cabin.print.y);
}

fn floor_print_uv(p: vec3<f32>) -> vec2<f32> {
    let q = vec2<f32>(p.y - cabin.floor_print.x, cabin.floor_print.y + cabin.floor_print.w - p.x) / cabin.floor_print.zw;
    return (vec2<f32>(0.0, cabin.print.z) + q * cabin.print.w) / cabin.print.xy;
}

// What the cabin's shader reads besides the frame (`render::CabinUniforms`), and the sun's shadow
// map of the cabin and the aircraft round it, drawn each frame (`CockpitRenderer::prepare`).
struct Cabin {
    light: mat4x4<f32>,  // body axes to the shadow map
    lights: vec4<f32>,   // x = panel's lights, y = standby instruments', z = 1 with a shadow map, w = its texel
    sky: vec4<f32>,      // the light through the windows' box: xyz its least corner, w its step
    panel_print: vec4<f32>, // the print's panel part: (y, z) from xy over zw metres
    floor_print: vec4<f32>, // its floor part: (y, x) from xy over zw metres
    print: vec4<f32>,    // xy the print in points, z where the floor part starts, w how wide it is
    place: vec4<f32>,    // x: the panel's print only ahead of it
};
@group(2) @binding(6) var<uniform> cabin: Cabin;
@group(2) @binding(7) var cabin_shadow_map: texture_depth_2d;
@group(2) @binding(8) var cabin_shadow_sampler: sampler_comparison;
// The pictures of the cabin's textured materials (`cockpit::Material::Textured`), a layer each:
// their colour, and their normal maps (x and y; +y up the picture).
@group(2) @binding(9) var cabin_picture: texture_2d_array<f32>;
@group(2) @binding(10) var cabin_relief: texture_2d_array<f32>;
// How strongly a picture's normal map bends the light.
const PICTURE_RELIEF = 1.0;

// The normal `n` at a fragment `rel` bent by `mapped`, the x and y of a normal map read at `uv`, in
// the frame the texture coordinates make there (from the derivatives: the mesh has no tangents).
fn cabin_relief_normal(rel: vec3<f32>, uv: vec2<f32>, n: vec3<f32>, v: vec3<f32>, mapped: vec2<f32>) -> vec3<f32> {
    let dp1 = dpdx(rel);
    let dp2 = dpdy(rel);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let scale = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-30));
    let xy = mapped * PICTURE_RELIEF;
    if dot(xy, xy) < 1e-8 {
        return n;
    }
    let z = sqrt(saturate(1.0 - dot(xy, xy)));
    let bent = normalize(t * scale * xy.x - b * scale * xy.y + n * z);
    // Never bent away from the eye: that would light the back of a groove seen edge-on.
    return normalize(bent + n * max(0.0, 0.05 - dot(bent, v)));
}

// The sky through the windows, as a multiple of the sky's light. What the cabin throws back of the
// light that comes in (`cabin_bounce_light`): that share of the sky's and of the ground's below,
// with `BOUNCE_COLOUR` of their colour, and `CABIN_SUN` of the sun's.
const CABIN_SKY = 2.0;
const CABIN_BOUNCE = 0.36;
const BOUNCE_COLOUR = 0.6;
const CABIN_SUN = 0.035;
// A display's white in the scene's light: 2000 cd/m² under the noon's sun. Its photocell keeps it
// as bright to the eye from the day to the end of the dusk, whatever the exposure of the hour
// (`sky::exposure`); at night the PANEL knob sets it, from `DISPLAY_NIGHT.x` of that with the knob
// off to `.y` with it full.
const DISPLAY = 0.128;
// A display's or a dial's glass: what it mirrors head on, how much more where it is smudged, and
// how much of the light that falls on it its dust scatters back.
const SCREEN_F0 = 0.03;
const SCREEN_SMUDGE = 0.12;
const SCREEN_DUST = 0.0006;
const DISPLAY_NIGHT = vec2<f32>(0.2, 1.5);
// The luminance of the panel's backlit legends at night and by day and of the lit standby dials at
// full, cd/m², and the illuminance of the flood light under the glare shield on the panel, lux.
const LEGENDS_NIGHT = 4.0;
const LEGENDS_DAY = 60.0;
const DIALS = 0.6;
const FLOOD = 2.0;

// The eye in a cabin adapts to the shade it looks at, not only to the bright world through the
// windows, as no single exposure does: what is lit less than `LIFT_FROM` of the light of the
// open sky (the panel under the glare shield, the footwell) is drawn brighter, by the share it
// lacks to the power `LIFT_POWER`, up to `LIFT_MOST` times; all of that from the day to the end of
// the dusk, `LIFT_NIGHT` of it at night, when the cabin is dark and the panel's lights are what
// is seen by. What the sun or the windows light, the displays and the world outside are not
// touched, so the view out is not washed out.
const LIFT_FROM = 1.5;
const LIFT_POWER = 0.8;
const LIFT_MOST = 12.0;
const LIFT_NIGHT = 0.5;

// How much brighter the cabin's shade is drawn where it is lit by `light` (`cabin_fragment`).
fn cabin_lift(light: vec3<f32>, sky: vec3<f32>) -> f32 {
    let open_sky = dot(sky, vec3<f32>(0.3, 0.5, 0.2)) * CABIN_SKY;
    let share = dot(light, vec3<f32>(0.3, 0.5, 0.2)) / max(open_sky, 1e-6);
    let lift = min(pow(LIFT_FROM / max(share, 1e-4), LIFT_POWER), LIFT_MOST);
    return mix(1.0, max(lift, 1.0), mix(LIFT_NIGHT, 1.0, smoothstep(0.0, 0.3, frame.sun_color.w)));
}

// How much of the sun reaches `p` (body axes) on a surface facing `n`: 1 lit, 0 in the shadow of
// the cabin, the wing or the cowling. Nine filtered taps of the shadow map, a soft edge some 5 mm wide.
fn cabin_sunlit(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let q = cabin.light * vec4<f32>(p + n * 0.008, 1.0);
    let uv = vec2<f32>(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
    let texel = cabin.lights.w * 1.5;
    var lit = 0.0;
    for (var i = -1; i <= 1; i++) {
        for (var j = -1; j <= 1; j++) {
            lit += textureSampleCompareLevel(cabin_shadow_map, cabin_shadow_sampler, uv + vec2<f32>(f32(i), f32(j)) * texel, q.z);
        }
    }
    let inside = all(uv > vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0)) && q.z < 1.0;
    return select(1.0, lit / 9.0, inside) * cabin.lights.z;
}

// What a photograph of the cabin gives a point: its brightness, how its relief bends the surface
// (body axes), its roughness and its occlusion.
struct CabinPhoto {
    colour: vec3<f32>,
    bend: vec3<f32>,
    rough: f32,
    occlusion: f32,
};

// The photograph `layer` at `p` (body axes) on a surface facing `nb`, `scale` metres a repeat,
// projected along each axis and blended by how much the surface faces it, so that a curved surface
// shows no seam (`CABIN_TRIPLANAR`; else only along the axis it faces most).
fn cabin_photo(p: vec3<f32>, nb: vec3<f32>, layer: i32, scale: f32) -> CabinPhoto {
    let a = abs(nb);
    var w = a * a * a * a;
    if !CABIN_TRIPLANAR {
        w = select(select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), a.y >= a.z), vec3<f32>(1.0, 0.0, 0.0), a.x >= a.y && a.x >= a.z);
    }
    w /= w.x + w.y + w.z;
    let q = p / scale;
    var photo: CabinPhoto;
    if CABIN_TRIPLANAR {
        // Along x the picture lies on (y, z), along y on (x, z), along z on (x, y).
        let dx = textureSample(cabin_detail, cabin_sampler, q.yz, layer);
        let dy = textureSample(cabin_detail, cabin_sampler, q.xz, layer);
        let dz = textureSample(cabin_detail, cabin_sampler, q.xy, layer);
        photo.colour = textureSample(cabin_colour, cabin_sampler, q.yz, layer).rgb * w.x + textureSample(cabin_colour, cabin_sampler, q.xz, layer).rgb * w.y + textureSample(cabin_colour, cabin_sampler, q.xy, layer).rgb * w.z;
        let bx = dx.rg * 2.0 - 1.0;
        let by = dy.rg * 2.0 - 1.0;
        let bz = dz.rg * 2.0 - 1.0;
        photo.bend = vec3<f32>(0.0, bx.x, bx.y) * w.x + vec3<f32>(by.x, 0.0, by.y) * w.y + vec3<f32>(bz.x, bz.y, 0.0) * w.z;
        photo.rough = dx.b * w.x + dy.b * w.y + dz.b * w.z;
        photo.occlusion = dx.a * w.x + dy.a * w.y + dz.a * w.z;
    } else {
        let uv = select(select(q.xy, q.xz, w.y > 0.5), q.yz, w.x > 0.5);
        let d = textureSample(cabin_detail, cabin_sampler, uv, layer);
        photo.colour = textureSample(cabin_colour, cabin_sampler, uv, layer).rgb;
        let b = d.rg * 2.0 - 1.0;
        photo.bend = select(select(vec3<f32>(b.x, b.y, 0.0), vec3<f32>(b.x, 0.0, b.y), w.y > 0.5), vec3<f32>(0.0, b.x, b.y), w.x > 0.5);
        photo.rough = d.b;
        photo.occlusion = d.a;
    }
    return photo;
}

struct CabinOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) open: f32,
};

// The cabin's vertices, with how open each is to the cabin round it (`engine::ambient_occlusion`).
@vertex
fn cabin_vertex(in: ObjectIn, @location(3) open: f32) -> CabinOut {
    var o: CabinOut;
    let rel = object.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.normal = (object.model * vec4<f32>(in.normal, 0.0)).xyz;
    o.color = in.color;
    o.open = open;
    return o;
}

// The point `rel` (relative to the camera) and the direction `d`, in body axes.
fn cabin_body_point(rel: vec3<f32>) -> vec3<f32> {
    let d = rel - frame.body_pos.xyz;
    return vec3<f32>(dot(d, frame.body_fwd.xyz), dot(d, frame.body_right.xyz), dot(d, frame.body_down.xyz));
}

fn cabin_body_direction(d: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(d, frame.body_fwd.xyz), dot(d, frame.body_right.xyz), dot(d, frame.body_down.xyz));
}

// The sky the windows look at: ahead and a little up, and overhead.
fn cabin_sky_seen() -> vec3<f32> {
    return (sky_seen(normalize(frame.body_fwd.xyz - frame.body_down.xyz * 0.3)) + sky_seen(vec3<f32>(0.0, 1.0, 0.0))) * 0.5;
}

// The sky the windows let in, mostly its brightness: the cabin's own colours tint what it lets in.
fn cabin_sky_light() -> vec3<f32> {
    let seen = cabin_sky_seen();
    return mix(vec3<f32>(dot(seen, vec3<f32>(0.3, 0.5, 0.2))), seen, 0.2) * vec3<f32>(1.02, 1.0, 0.96);
}

// What the cabin throws back, from its seats, ceiling and panel, of the light that comes in through
// its windows: the sky's, the lit ground's from below (`sky_light` facing down) and the sun's `s`
// (body axes) where it falls inside, on the floor and the seats when it is high, through one
// window or another when it is low. It reaches what sees no window, a soft half-light with more of
// the colour of what came in than the sky's own light has: blue at noon, warm at dusk.
fn cabin_bounce_light(s: vec3<f32>) -> vec3<f32> {
    let seen = cabin_sky_seen();
    let sky = mix(vec3<f32>(dot(seen, vec3<f32>(0.3, 0.5, 0.2))), seen, BOUNCE_COLOUR);
    let ground = sky_light(vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0));
    let sun = frame.sun_color.rgb * (0.4 + 0.6 * max(-s.z, 0.0));
    return (sky + ground * 0.5) * CABIN_BOUNCE + sun * CABIN_SUN;
}

// The player's cabin from the pilot's seat. The integer part of a vertex colour's alpha is its
// material (`cockpit::Material`), the fraction its gloss. A textured material has its own picture
// and normal map. Photographs are projected along
// the body axis the surface faces most, their grain given to the vertex colour. Daylight comes in
// through the windows: the sky as much as each point sees of it (`cabin_open`) and as open as it is
// to the cabin round it (`in.open`), and what the cabin throws back of it all (`cabin_bounce_light`);
// the sun where its shadow map lets it through. The DIMMING knobs light the panel's legends from
// behind (seen from the dusk on), and at night a flood light under the glare shield and the
// standby dials. Displays shine with their own light.
@fragment
fn cabin_fragment(in: CabinOut) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    let v = normalize(-in.rel);
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let sun = frame.sun_dir.xyz;
    let day = frame.sun_color.w;
    let fwd = frame.body_fwd.xyz;
    let right = frame.body_right.xyz;
    let down = frame.body_down.xyz;
    let p = cabin_body_point(in.rel);
    let s = cabin_body_direction(sun);
    let nb = cabin_body_direction(n);
    let id = i32(floor(in.color.a + 0.001));
    var gloss = fract(in.color.a + 0.001) / 0.9;
    // A candela per square metre in the scene's light.
    let nit = frame.lights[0].x;

    // Every sample is taken whatever the material, so that they stay in uniform control flow.
    let m = clamp(id, 0, 12);
    let layer = CABIN_LAYER[m];
    let photo = cabin_photo(p, nb, layer, CABIN_SCALE[m]);
    let mean = textureSampleLevel(cabin_colour, cabin_sampler, vec2<f32>(0.5), layer, 20.0).rgb;
    // The canvases have every mip level and the sampler is anisotropic: one sample each is sharp.
    let picture = textureSample(cabin_canvas, cabin_sampler, in.color.rg).rgb;
    let on_panel = nb.x < -0.7;
    let pt = select(floor_print_uv(p), panel_print_uv(p), on_panel);
    let print = textureSample(cabin_print, cabin_sampler, pt);
    let picture_layer = i32(in.color.b + 0.5);
    let own = textureSample(cabin_picture, cabin_sampler, in.color.rg, picture_layer).rgb;
    let relief = textureSample(cabin_relief, cabin_sampler, in.color.rg, picture_layer).rg * 2.0 - 1.0;
    let own_normal = cabin_relief_normal(in.rel, in.color.rg, n, v, relief);

    var albedo = in.color.rgb;
    var nn = n;
    var occlusion = 1.0;
    if id == 12 {
        albedo = own;
        nn = own_normal;
    } else if (id >= 1 && id <= 6) || id >= 9 {
        // The photograph's grain, not its colour: its brightness against its average, held within
        // reason (a dark photograph's noise would otherwise turn into speckles).
        let grain = dot(photo.colour, vec3<f32>(0.3, 0.5, 0.2)) / max(dot(mean, vec3<f32>(0.3, 0.5, 0.2)), 0.004);
        albedo = in.color.rgb * clamp(mix(1.0, grain, CABIN_GRAIN[m]), 0.4, 1.8);
        let bent = normalize(nb + photo.bend * CABIN_BUMP[m]);
        nn = normalize(fwd * bent.x + right * bent.y + down * bent.z);
        gloss *= 1.0 - CABIN_ROUGH[m] * photo.rough;
        occlusion = mix(1.0, photo.occlusion, 0.6);
    }
    // The print (premultiplied), where the surface faces aft at the panel or up at the floor, inside
    // that part of the picture; its legends are lit from behind by the panel's lights.
    let split = cabin.print.z / cabin.print.y;
    let panel_part = on_panel && p.x > cabin.place.x && pt.y < split;
    let floor_part = nb.z < -0.7 && pt.y > split && pt.x < cabin.print.w / cabin.print.x;
    var glow = vec3<f32>(0.0);
    if id == 9 && (panel_part || floor_part) && all(pt > vec2<f32>(0.0)) && all(pt < vec2<f32>(1.0)) {
        albedo = albedo * (1.0 - print.a) + print.rgb;
        // The white legends glow (`placards::WHITE`); the placards' paler grey (`PLACARD`) does not.
        // Their lamps are as bright as the knob sets them, more by day than at night as the
        // displays are: against the daylight they do not show, from the dusk on they do.
        let legend = smoothstep(0.72, 0.78, min(print.r, min(print.g, print.b))) * print.a;
        glow = vec3<f32>(1.0, 0.93, 0.82) * legend * mix(LEGENDS_NIGHT, LEGENDS_DAY, day) * cabin.lights.x * cabin.lights.x * nit * f32(panel_part);
    }
    if id == 8 {
        return vec4<f32>(albedo * mix(1.0, 0.6, day), 1.0);
    }

    let sunlit = cabin_sunlit(p, nb) * step(0.0, dot(n, sun));
    let sky = cabin_sky_light();
    // The sky through the windows, as much as this point sees of them, and what the cabin throws
    // back everywhere, less in its corners; both as open as the point is.
    let open = cabin_open(p, nb);
    let around = in.open * in.open;
    let bounce = cabin_bounce_light(s);
    var ambient = (sky * CABIN_SKY * open + bounce * (0.4 + 0.6 * sqrt(open))) * mix(0.25, 1.0, around);
    // The flood light under the glare shield, on the panel's face below it, by night.
    let flood = FLOOD * cabin.lights.x * cabin.lights.x * (1.0 - day) * smoothstep(0.45, 0.6, p.x) * smoothstep(-0.46, -0.30, p.z) * max(-nb.x, 0.0) / 3.14159;
    ambient += vec3<f32>(1.0, 0.82, 0.62) * flood * nit;
    // What the surface reflects: the sun by a highlight as sharp as its gloss (Blinn and Phong's,
    // normalised), the cabin and the sky through the windows the way it mirrors them, both by
    // Schlick's Fresnel from what it reflects head on; a metal (chrome) its own colour.
    let h = normalize(sun + v);
    let metal = id == 11;
    let f0 = select(vec3<f32>(CABIN_F0[m]), albedo, metal);
    let sharp = exp2(3.5 + 7.0 * gloss);
    let to_sun = f0 + (1.0 - f0) * pow(1.0 - max(dot(v, h), 0.0), 5.0);
    let shine = frame.sun_color.rgb * to_sun * (sharp + 8.0) / 25.13 * pow(max(dot(nn, h), 0.0), sharp) * max(dot(nn, sun), 0.0) * sunlit;
    let r = reflect(-v, nn);
    let rb = cabin_body_direction(r);
    let seen = cabin_open(p, rb);
    let mirrored = mix(bounce, mix(sky, sky_seen(r), 0.6) * CABIN_SKY, seen);
    let to_view = f0 + (1.0 - f0) * pow(1.0 - max(dot(nn, v), 0.0), 5.0) * gloss;
    let reflection = mirrored * to_view * gloss * gloss * around + shine;

    if id == 7 {
        // A display shines by itself, as bright as the light it is seen in asks until the night,
        // then as the PANEL knob sets it; a dial is lit as any surface, and from inside by the STBY
        // IND knob. Both behind glass.
        // The glass mirrors what is behind the eye: the windows where its reflection finds them, the
        // dim cabin elsewhere, by Fresnel's share (`SCREEN_F0` head on; more at a slant), a little
        // more where a finger left its grease; the sun, when it is behind, in a sharp glint and a
        // wide soft one (the anti-glare coat), and its dust lit by whatever light falls on it.
        let smudge = smoothstep(0.35, 0.8, cut_noise(p * 28.0)) * (0.5 + 0.5 * cut_noise(p * 190.0));
        let to_glass = SCREEN_F0 * (1.0 + SCREEN_SMUDGE * smudge) + (1.0 - SCREEN_F0) * pow(1.0 - max(dot(n, v), 0.0), 5.0);
        let glint = frame.sun_color.rgb * (pow(max(dot(n, h), 0.0), 400.0) * 1.2 + pow(max(dot(n, h), 0.0), 24.0) * 0.012) * sunlit;
        let dust = (ambient + frame.sun_color.rgb * max(dot(n, sun), 0.0) * sunlit) * SCREEN_DUST * (0.4 + 0.6 * smudge);
        let glass = mirrored * to_glass * mix(0.5, 1.0, around) + glint + dust;
        if in.color.b < 0.5 {
            let level = mix(mix(DISPLAY_NIGHT.x, DISPLAY_NIGHT.y, cabin.lights.x), 1.0, day);
            return vec4<f32>(picture * DISPLAY * level + glass, 1.0);
        }
        let lamp = vec3<f32>(1.0, 0.9, 0.75) * DIALS * cabin.lights.y * cabin.lights.y * (1.0 - day) * nit;
        let light = frame.sun_color.rgb * max(dot(n, sun), 0.0) * sunlit + ambient * 1.3 + lamp;
        return vec4<f32>((picture * light + glass) * cabin_lift(light, sky), 1.0);
    }

    // A metal has no light of its own colour under its reflection but what its roughness scatters.
    let diffuse = select(albedo, albedo * 0.08, metal);
    let light = frame.sun_color.rgb * max(dot(nn, sun), 0.0) * sunlit + ambient * occlusion;
    // The eye that has opened for the shade sees the legends' lamps brighter by as much.
    let col = (diffuse * light + reflection + glow) * cabin_lift(light, sky);
    return vec4<f32>(col, 1.0);
}

// The windows' glass, over the world seen through it: it mirrors a little of the dim cabin, more at
// a slant, and its dust and smears scatter the sun that falls on them,
// mostly forward: a haze over the windscreen when the sun is ahead.
@fragment
fn cabin_glass_fragment(in: ObjectOut) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    let v = normalize(-in.rel);
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let sun = frame.sun_dir.xyz;
    let p = cabin_body_point(in.rel);
    let fresnel = 0.04 + 0.96 * pow(1.0 - abs(dot(n, v)), 5.0);
    // Dust in broad smears and finer grime. No insects: as dark specks they read as spots in the sky.
    let smear = smoothstep(0.45, 0.9, cut_noise(p * 6.0 + vec3<f32>(3.1, 0.0, 0.0)));
    let grime = cut_noise(p * 40.0) * 0.5 + cut_noise(p * 160.0) * 0.5;
    let dirt = clamp(smear * (0.35 + 0.65 * grime), 0.0, 1.0);
    let sunlit = cabin_sunlit(p, cabin_body_direction(n)) * frame.sun_color.w;
    let ahead = max(dot(-v, sun), 0.0);
    let scatter = frame.sun_color.rgb * sunlit * dirt * (0.004 + 0.10 * pow(ahead, 10.0) + 0.02 * pow(ahead, 2.0));
    let sky = cabin_sky_light();
    // The cabin the inside of the glass mirrors: dark, a little of the sky's light.
    let mirrored = sky * 0.05;
    let alpha = clamp(fresnel * 0.5 + dirt * 0.05, 0.0, 1.0);
    return vec4<f32>(mirrored * fresnel + scatter + sky * dirt * 0.006, alpha);
}
