// ---------------------------------------------------------------- textured objects (`looks.rs`)
//
// A game's mesh drawn with its materials' textures, per pixel: base colour, metallic and roughness,
// the normal map, ambient occlusion, emissive (PBR metallic-roughness, as glTF has them). Lit as the
// world lights its objects (`object_shade`): the sun through the air, the vehicles' shadows, the
// clouds' and the relief's, the sky, the lights of the ground; a GGX highlight of the sun and the
// sky and the ground mirrored as rough as the surface is. Glass is a blended pass of its own
// (`glass_fragment`): the inside seen through its tint, the world mirrored on it.

struct SurfaceMaterial {
    base: vec4<f32>,
    emissive: vec4<f32>,   // rgb = emissive factor, a = the normal map's scale
    pbr: vec4<f32>,        // x = metallic, y = roughness, z = occlusion's strength, w = 1 for glass, minus the cutoff of a mask
};

@group(2) @binding(20) var<uniform> surface: SurfaceMaterial;
@group(2) @binding(21) var surface_base: texture_2d<f32>;
@group(2) @binding(22) var surface_orm: texture_2d<f32>;
@group(2) @binding(23) var surface_normal: texture_2d<f32>;
@group(2) @binding(24) var surface_glow: texture_2d<f32>;
@group(2) @binding(25) var surface_sampler: sampler;

struct TexturedIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(5) uv: vec2<f32>,
};

struct TexturedOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) local: vec3<f32>,
};

@vertex
fn textured_vertex(in: TexturedIn) -> TexturedOut {
    var o: TexturedOut;
    let rel = object.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.normal = (object.model * vec4<f32>(in.normal, 0.0)).xyz;
    o.uv = in.uv;
    o.local = in.position;
    return o;
}

// The GGX distribution of microfacets for a roughness `a` (alpha).
fn surface_ggx(ndh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = ndh * ndh * (a2 - 1.0) + 1.0;
    return a2 / (3.14159265 * d * d);
}

// The sun's highlight on a surface of roughness `rough` and reflectance `f0`, as the world's light
// is given (a white surface facing the sun is as bright as the light: the diffuse has no 1 / π).
fn surface_highlight(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32, f0: vec3<f32>) -> vec3<f32> {
    let h = normalize(l + v);
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 0.05);
    let a = max(rough * rough, 0.002);
    let k = a * 0.5;
    let vis = 0.25 / ((ndl * (1.0 - k) + k) * (ndv * (1.0 - k) + k) + 1e-4);
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - saturate(dot(v, h)), 5.0);
    return fresnel * surface_ggx(max(dot(n, h), 0.0), a) * vis * ndl * 3.14159265;
}

// What a surface mirrors along `r` (unit): the sky with its clouds above, blurred as the surface is
// rough, and the ground below it, lit as the ground is lit.
fn surface_mirrored(r: vec3<f32>, rel: vec3<f32>, rough: f32) -> vec3<f32> {
    let up = normalize(vec3<f32>(r.x, max(r.y, 0.02), r.z));
    var sky: vec3<f32>;
    if rough < 0.35 {
        sky = water_sky(up, rel, 0.02 + rough * 0.5);
    } else {
        sky = sky_seen(up);
    }
    sky = mix(sky, sky_light(up, rel), saturate(rough * 1.3));
    // The ground: what lights it from above, at an albedo of fields and asphalt; darker under the
    // vehicle's own shade, which a shiny belly mirrors.
    let day = light_at(rel) * max(frame.sun_dir.y, 0.0);
    let ground = vec3<f32>(0.085, 0.09, 0.07) * (day + luma(sky_light(vec3<f32>(0.0, 1.0, 0.0), rel)));
    let blur = 0.02 + rough * 0.25;
    return mix(sky, ground, smoothstep(blur, -0.02 - blur, r.y));
}

// The share of the light mirrored by a surface seen at `ndv`, over every way the rough microfacets
// send it (Karis's fit of the integrated BRDF): what the sky's reflection is weighted by.
fn surface_env_brdf(f0: vec3<f32>, rough: f32, ndv: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = rough * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * ndv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

// The normal at a fragment: the mesh's (turned towards the eye: the meshes are two-sided), bent by
// the normal map in the frame its texture coordinates make there (from the derivatives, so the
// mesh needs no tangents).
fn surface_normal_at(in: TexturedOut, mapped: vec2<f32>, v: vec3<f32>) -> vec3<f32> {
    var n = normalize(in.normal);
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let dp1 = dpdx(in.rel);
    let dp2 = dpdy(in.rel);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let scale = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-30));
    let xy = mapped * surface.emissive.a;
    if dot(xy, xy) < 1e-8 {
        return n;
    }
    // glTF's normal maps: +y up the picture, which is towards -v.
    let z = sqrt(saturate(1.0 - dot(xy, xy)));
    let bent = normalize(t * scale * xy.x - b * scale * xy.y + n * z);
    // Never bent away from the eye: that would light the back of a groove seen edge-on.
    return normalize(bent + n * max(0.0, 0.05 - dot(bent, v)));
}

struct Surfaced {
    base: vec4<f32>,
    orm: vec3<f32>,
    normal: vec2<f32>,
    glow: vec3<f32>,
};

// The textures at a fragment: all sampled before anything is discarded.
fn surface_sample(uv: vec2<f32>) -> Surfaced {
    var s: Surfaced;
    s.base = textureSample(surface_base, surface_sampler, uv) * surface.base;
    s.orm = textureSample(surface_orm, surface_sampler, uv).rgb;
    s.normal = textureSample(surface_normal, surface_sampler, uv).rg * 2.0 - 1.0;
    s.glow = textureSample(surface_glow, surface_sampler, uv).rgb * surface.emissive.rgb;
    return s;
}

// A textured fragment lit, its colour times `tint`; `past` how far it is past a cut of its draw
// (`cut_past`: -1000 for a whole one).
fn textured_lit(in: TexturedOut, s: Surfaced, normal: vec3<f32>, tint: vec3<f32>, past: f32, pixel_x: vec3<f32>, pixel_y: vec3<f32>) -> vec4<f32> {
    let v = normalize(-in.rel);
    var n = normal;
    var base = s.base.rgb * tint;
    let ao = mix(1.0, s.orm.r, surface.pbr.z);
    var rough = clamp(surface.pbr.y * s.orm.g, 0.03, 1.0);
    var metal = saturate(surface.pbr.x * s.orm.b);
    if past > -100.0 {
        // Through a tear the inside of the skin shows, in its primer; along the edge, bare crumpled metal.
        if dot(normalize(in.normal), v) < 0.0 {
            base = vec3<f32>(0.30, 0.36, 0.24) * (0.35 + 0.25 * cut_noise(in.local * 9.0));
            rough = 0.9;
            metal = 0.0;
        }
        let rip = smoothstep(-0.05, -0.004, past);
        base = mix(base, vec3<f32>(0.36, 0.37, 0.39) * (0.5 + 0.8 * cut_noise(in.local * 40.0)), rip);
        rough = mix(rough, 0.35, rip);
        metal = mix(metal, 1.0, rip);
    }
    let geometric = normalize(in.normal) * select(1.0, -1.0, dot(in.normal, v) < 0.0);
    let sun = frame.sun_dir.xyz;
    let sunlight = light_at(in.rel) * vehicle_shadow(in.rel, geometric) * cloud_shadow(in.rel) * terrain_shadow(in.rel);
    // What the mesh's own face lets of the sun reach the bent normal: no light on a groove turned
    // to the sun on a face turned away from it.
    let ndl = max(dot(n, sun), 0.0) * smoothstep(-0.05, 0.1, dot(geometric, sun));
    let ndv = max(dot(n, v), 1e-3);
    let f0 = mix(vec3<f32>(0.04), base, metal);
    let diffuse = base * (1.0 - metal);
    var col = diffuse * (sunlight * ndl + sky_light(n, in.rel) * ao);
    if frame.fire_count.x > 0.0 {
        col += diffuse * fire_light(in.rel, n) * ao;
    }
    col += sunlight * surface_highlight(n, v, sun, rough, f0) * smoothstep(-0.05, 0.1, dot(geometric, sun));
    // The sky and the ground mirrored, less in the creases the occlusion knows.
    let r = reflect(-v, n);
    let seen = saturate(pow(ndv + ao, exp2(-16.0 * rough - 1.0)) - 1.0 + ao);
    // What a bent normal mirrors from behind the mesh's own face is not seen (the horizon's fade);
    // and a face turned to the ground (a wing's underside) mirrors a ground in its own shade, and
    // less of it at grazing than the sky gives a face turned up (by eye: never a mirror of the sky).
    let horizon = saturate(1.0 + 1.2 * dot(r, geometric));
    let below = mix(0.45, 1.0, smoothstep(-0.6, 0.1, geometric.y));
    col += surface_mirrored(r, in.rel, rough) * surface_env_brdf(f0, rough, ndv) * seen * horizon * horizon * below;
    // What glows by itself (a lit display, a lamp's lens): by night more than by day, as the eye sees it.
    col += s.glow * 0.5;
    if under_sea(in.rel) {
        return under_sea_color(col, in.rel, pixel_x, pixel_y, false);
    }
    return vec4<f32>(apply_fog(col, in.rel), 1.0);
}

fn textured_shade(in: TexturedOut) -> vec4<f32> {
    let pixel_x = dpdx(in.rel);
    let pixel_y = dpdy(in.rel);
    let s = surface_sample(in.uv);
    let n = surface_normal_at(in, s.normal, normalize(-in.rel));
    let past = cut_past(in.local);
    if past > 0.0 || s.base.a < -surface.pbr.w {
        discard;
    }
    return textured_lit(in, s, n, object.tint.rgb, past, pixel_x, pixel_y);
}

@fragment
fn textured_fragment(in: TexturedOut) -> @location(0) vec4<f32> {
    return textured_shade(in);
}

// The same mesh drawn by instances, as the houses are (`building_vertex`: an instance's place
// relative to its batch, scale and heading; the batch's offset in `chunk`): the vehicles parked on
// the aprons. Never cut.
struct ParkedIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) place: vec4<f32>,
    @location(4) tint: vec4<f32>,
    @location(5) uv: vec2<f32>,
};

@vertex
fn parked_vertex(in: ParkedIn) -> TexturedOut {
    var o: TexturedOut;
    let cs = cos(in.tint.w);
    let sn = sin(in.tint.w);
    let p = in.position * in.place.w;
    let rel = in.place.xyz + chunk.offset.xyz + vec3<f32>(p.x * cs - p.z * sn, p.y, p.x * sn + p.z * cs);
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.normal = vec3<f32>(in.normal.x * cs - in.normal.z * sn, in.normal.y, in.normal.x * sn + in.normal.z * cs);
    o.uv = in.uv;
    o.local = in.position;
    return o;
}

@fragment
fn parked_fragment(in: TexturedOut) -> @location(0) vec4<f32> {
    let pixel_x = dpdx(in.rel);
    let pixel_y = dpdy(in.rel);
    let s = surface_sample(in.uv);
    let n = surface_normal_at(in, s.normal, normalize(-in.rel));
    if s.base.a < -surface.pbr.w {
        discard;
    }
    return textured_lit(in, s, n, vec3<f32>(1.0), -1000.0, pixel_x, pixel_y);
}

// Glass of what is parked, which has no draw's cuts.
@fragment
fn parked_glass_fragment(in: TexturedOut) -> @location(0) vec4<f32> {
    return glass_shade(in, -1000.0);
}

// Glass: what is behind it seen through its tint (the blend keeps `1 − alpha` of it), the sky and
// the ground mirrored on it with Fresnel's strength (4 % head on, all of it at grazing), the sun's
// highlight sharp. `surface.base.a` is how much of the light the glass takes.
@fragment
fn glass_fragment(in: TexturedOut) -> @location(0) vec4<f32> {
    let past = cut_past(in.local);
    return glass_shade(in, past);
}

fn glass_shade(in: TexturedOut, past: f32) -> vec4<f32> {
    let s = surface_sample(in.uv);
    let v = normalize(-in.rel);
    let n = surface_normal_at(in, s.normal, v);
    if past > 0.0 {
        discard;
    }
    let ndv = max(dot(n, v), 1e-3);
    let fresnel = 0.04 + 0.96 * pow(1.0 - ndv, 5.0);
    let sun = frame.sun_dir.xyz;
    let sunlight = light_at(in.rel) * vehicle_shadow(in.rel, n) * cloud_shadow(in.rel);
    let rough = clamp(surface.pbr.y, 0.02, 1.0);
    var mirrored = surface_mirrored(reflect(-v, n), in.rel, rough * 0.3) * fresnel;
    mirrored += sunlight * surface_highlight(n, v, sun, max(rough * 0.3, 0.03), vec3<f32>(0.04));
    // The tint, as the base colour says, over what comes through.
    let take = saturate(s.base.a);
    let through = (1.0 - take) * (1.0 - fresnel);
    let alpha = clamp(1.0 - through, 0.02, 1.0);
    let tint = s.base.rgb * take * (sky_light(n, in.rel) * 0.15);
    let col = (mirrored + tint) * haze_through(in.rel);
    return vec4<f32>(col / alpha, alpha);
}
