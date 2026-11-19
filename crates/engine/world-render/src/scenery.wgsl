// Shaders of the scenery: the forest beyond the trees, buildings and their shadows, towns,
// aerodromes. Appended to `shaders.wgsl` by `pipelines.rs`, so everything declared there (the
// frame, the noise, the sky, the fog) is in reach here.

// ---------------------------------------------------------------- forest on the ground
//
// Trees are drawn one by one up to `frame.misc.z` metres over the ground. Under them the ground is in their shade;
// beyond them the ground itself stands for the forest: it is lifted to the height of the crowns and
// painted as a canopy seen from above.

// How much the ground is lifted where it stands for the forest.
fn canopy_height(forest: f32, dist: f32) -> f32 {
    return forest * 9.0 * smoothstep(frame.misc.z * 0.72, frame.misc.z * 0.98, dist);
}

// Bumps the size of crowns, 0..1.
fn crowns(w: vec2<f32>) -> f32 {
    return vnoise(w / 8.0, WRAP_I / 8) * 0.62 + vnoise(w / 4.0 + 3.7, WRAP_I / 4) * 0.38;
}

// The colour of the ground where there is forest: `albedo` is the colour without it.
fn forest_ground(albedo: vec3<f32>, forest: f32, w: vec2<f32>, dist: f32) -> vec3<f32> {
    if forest < 0.02 {
        return albedo;
    }
    let range = frame.misc.z;
    let beyond = smoothstep(range * 0.72, range * 0.98, dist);
    // Under the trees: leaf litter in the shade of the crowns, darker the denser the forest.
    var under = albedo * vec3<f32>(0.62, 0.50, 0.55) * (0.55 + 0.25 * vnoise(w / 4.0, WRAP_I / 4));
    if beyond <= 0.0 {
        return mix(albedo, under, forest);
    }
    // Beyond them: crowns lit from the side of the sun, with dark gaps between them.
    var canopy = vec3<f32>(0.030, 0.056, 0.020);
    let fine = 1.0 - smoothstep(range, range * 3.0, dist);
    if fine > 0.0 {
        let c = crowns(w);
        let sun = frame.sun_dir.xyz;
        let lit = c - crowns(w + sun.xz * 3.0);
        canopy *= 1.0 + ((c - 0.5) * 1.1 + lit * 2.2) * fine;
    }
    // Stands of different trees, as the single trees have.
    canopy *= 0.82 + 0.36 * vnoise(w / 256.0 + 9.0, WRAP_I / 256);
    return mix(albedo, mix(under, canopy, beyond), forest);
}

// ---------------------------------------------------------------- shadows
//
// The trees' are in flora.wgsl. A building's is its hull flattened on the ground (below), drawn
// after everything solid and blended over it.

// Distances beyond which trees and buildings cast no shadow (`TREE_SHADOW_RANGE` and
// `HOUSE_SHADOW_RANGE` in renderer.rs say the same, to leave out whole tiles).
const TREE_SHADOW_RANGE: f32 = 1000.0;
const HOUSE_SHADOW_RANGE: f32 = 5000.0;

struct ShadowOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) strength: f32,
};

@fragment
fn house_shadow_fragment(in: ShadowOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.004, 0.012, in.strength);
}

// ---------------------------------------------------------------- buildings
//
// An instance: xyz relative to the batch and scale | roof tint, heading. The alpha of a vertex colour
// says what the surface is: 0 a wall (takes the building's wall colour, has windows), 1 the roof
// (takes the roof tint), 0.5 anything that keeps its own colour.

struct BuildingOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // rgb = colour, a = kind of surface.
    @location(2) color: vec4<f32>,
    // x = distance along the wall, y = height above the ground, metres; z = a number of the building's own, 0..1.
    @location(3) wall: vec3<f32>,
    // [materials] xy = position on the face in metres, z = texture layer, w = 0 for a surface without
    // a photograph, 1 for one with it, 2 for metal sheet; and the horizontal of the face in the world.
    @location(4) tex: vec4<f32>,
    @location(5) tangent: vec3<f32>,
    // A building of the maps' facade (`footprints.rs`): x = its style (`STYLE_*`, -1 for the
    // instanced houses, which draw their windows the old way), y = a storey's height, z = the column
    // of windows along the wall, w = the height of a ground floor of shops (0: none).
    @location(6) facade: vec4<f32>,
};

// A number of the building's own, the same wherever the batch it is drawn in begins (batches begin on multiples of 512 m).
fn building_seed(place: vec3<f32>) -> f32 {
    return fract(sin(dot(fract(place.xz / 64.0), vec2<f32>(127.1, 311.7))) * 43758.5453);
}

@vertex
fn building_vertex(in: TreeIn) -> BuildingOut {
    var o: BuildingOut;
    let cs = cos(in.tint.w);
    let sn = sin(in.tint.w);
    let p = in.position * in.place.w;
    let rel = in.place.xyz + chunk.offset.xyz + vec3<f32>(p.x * cs - p.z * sn, p.y, p.x * sn + p.z * cs);
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.normal = vec3<f32>(in.normal.x * cs - in.normal.z * sn, in.normal.y, in.normal.x * sn + in.normal.z * cs);
    let seed = building_seed(in.place.xyz);
    // Whitewash, cream, ochre and stone.
    var walls = vec3<f32>(1.30, 1.32, 1.36);
    if seed > 0.45 { walls = vec3<f32>(1.22, 1.15, 0.98); }
    if seed > 0.70 { walls = vec3<f32>(1.10, 0.92, 0.66); }
    if seed > 0.88 { walls = vec3<f32>(0.80, 0.76, 0.72); }
    var color = in.color.rgb;
    if in.color.a < 0.25 {
        color *= walls;
    } else if in.color.a > 0.75 {
        color *= in.tint.rgb * (0.85 + 0.3 * fract(seed * 7.3));
    }
    o.color = vec4<f32>(color, in.color.a);
    o.wall = vec3<f32>(p.z * in.normal.x - p.x * in.normal.z, p.y, seed);
    // [materials] Plaster on walls and gables; clay tiles on the roof, or slate where its tint is
    // dark; corrugated sheet where the mesh says so (`buildings::SHEET`). The rest has no photograph.
    let t = building_tangent(in.normal);
    var material = vec2<f32>(f32(M_PLASTER), 1.0);
    if in.color.a > 0.75 && abs(in.normal.y) > 0.1 {
        material.x = f32(select(M_ROOF_CLAY, M_ROOF_SLATE, in.tint.r < 0.6));
    } else if abs(in.color.a - 0.6) < 0.05 {
        material = vec2<f32>(f32(M_METAL_CORRUGATED), 2.0);
    } else if in.color.a > 0.25 && in.color.a < 0.75 {
        material.y = 0.0;
    }
    o.tex = vec4<f32>(building_uv(p, in.normal, t), material);
    o.tangent = vec3<f32>(t.x * cs - t.z * sn, t.y, t.x * sn + t.z * cs);
    o.facade = vec4<f32>(-1.0, 0.0, 0.0, 0.0);
    return o;
}

// The buildings and bridges of a real place's maps (`footprints.rs`, `bridges.rs`): a tile's
// mesh, positions relative to its corner (`chunk.offset`), each vertex with its colour, what surface
// it is (`SURFACE_*`), how high above the building's foot, the building's own number and its facade.
struct FootprintIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) more: vec3<f32>,   // x = the surface, y = above the foot, m; z = the building's own number
    @location(4) facade: vec4<f32>, // style, storey, column along the wall, ground floor
};

// The surfaces and styles of `footprints.rs`.
const SURFACE_PLAIN: f32 = 0.3;
const SURFACE_STEEL: f32 = 0.6;
const SURFACE_DECK: f32 = 0.75;
const SURFACE_PITCHED: f32 = 0.9;
const STYLE_PLASTER: i32 = 0;
const STYLE_BRICK: i32 = 1;
const STYLE_STONE: i32 = 2;
const STYLE_GLASS: i32 = 3;
const STYLE_RIBBON: i32 = 4;
const STYLE_INDUSTRIAL: i32 = 5;
const STYLE_HOUSE: i32 = 6;
const STYLE_CHURCH: i32 = 7;
const STYLE_PLAIN: i32 = 8;

@vertex
fn footprint_vertex(in: FootprintIn) -> BuildingOut {
    var o: BuildingOut;
    let rel = in.position + chunk.offset.xyz;
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.normal = in.normal;
    let surface = in.more.x;
    let roof = surface > 0.8;
    let wall = surface < 0.15;
    // Walls take the facade (alpha 0); the rest keeps its colour (alpha 0.5) or is a roof (1).
    o.color = vec4<f32>(in.color, select(select(0.5, 1.0, roof), 0.0, wall));
    o.wall = vec3<f32>(in.position.z * in.normal.x - in.position.x * in.normal.z, in.more.y, in.more.z);
    let t = building_tangent(in.normal);
    let style = i32(in.facade.x + 0.5);
    var material = vec2<f32>(f32(M_PLASTER), 1.0);
    if surface > 0.95 {
        material.x = f32(select(M_CONCRETE, M_ROOF_CLAY, in.color.r > in.color.g * 1.45));
    } else if roof {
        material.x = f32(select(M_ROOF_SLATE, M_ROOF_CLAY, in.color.r > in.color.g * 1.45));
    } else if abs(surface - SURFACE_DECK) < 0.05 {
        material.x = f32(M_ASPHALT);
    } else if abs(surface - SURFACE_STEEL) < 0.05 {
        material.x = f32(M_METAL_PAINTED);
    } else if abs(surface - SURFACE_PLAIN) < 0.05 {
        material.x = f32(M_CONCRETE);
    } else if style == STYLE_GLASS {
        material.y = 0.0;
    } else if style == STYLE_RIBBON || style == STYLE_STONE || style == STYLE_CHURCH {
        material.x = f32(M_CONCRETE);
    } else if style == STYLE_INDUSTRIAL {
        material = vec2<f32>(f32(M_METAL_CORRUGATED), 2.0);
    }
    o.tex = vec4<f32>(building_uv(in.position, in.normal, t), material);
    o.tangent = t;
    o.facade = select(vec4<f32>(f32(STYLE_PLAIN), in.facade.yzw), in.facade, wall);
    return o;
}

// What a facade has at a point of a wall: the colour there, how much of it is a window's glass,
// and the light of a lit window after dark.
struct FacadeAt {
    albedo: vec3<f32>,
    glass: f32,
    glow: vec3<f32>,
};

// The windows of a facade (`style`) at `u` columns along the wall and `y` metres above the
// building's foot: one window to each column and storey, shaped by the style; shop fronts on a
// ground floor; brick coursing close up. `wall` is the wall's colour. Each window's edges are
// softened over the pixel (`fwidth`), so far away a facade is the mean of its windows and wall.
fn facade(style: i32, storey: f32, ground: f32, u: f32, y: f32, wall: vec3<f32>, seed: f32, dist: f32) -> FacadeAt {
    var out = FacadeAt(wall, 0.0, vec3<f32>(0.0));
    if style == STYLE_PLAIN || y < 0.0 {
        return out;
    }
    // Width of the window in its column, and its sill and head as shares of the storey.
    var width = 0.42;
    var sill = 0.28;
    var head = 0.80;
    var glass = vec3<f32>(0.018, 0.024, 0.030);
    var lit_share = 0.35;
    var warm = vec3<f32>(0.75, 0.47, 0.22);
    var levels = storey;
    var yy = y;
    var shop = false;
    if ground > 0.0 && y < ground {
        // A shop front: glass from knee height to under the fascia.
        shop = true;
        width = 0.84;
        sill = 0.12;
        head = 0.80;
        levels = ground;
        glass = vec3<f32>(0.040, 0.042, 0.045);
        lit_share = 0.7;
    } else {
        yy = y - ground;
        if style == STYLE_BRICK { width = 0.38; sill = 0.26; head = 0.80; }
        if style == STYLE_STONE { width = 0.36; sill = 0.20; head = 0.84; }
        if style == STYLE_GLASS {
            width = 0.95;
            sill = 0.26;
            head = 1.0;
            glass = wall * 0.6;
            lit_share = 0.45;
            // A lit floor of offices behind a curtain of glass: dimmer than a window, as most of
            // the light is ceilings seen through tinted glass.
            warm = vec3<f32>(0.045, 0.047, 0.055);
        }
        if style == STYLE_RIBBON { width = 1.0; sill = 0.34; head = 0.86; lit_share = 0.45; warm = vec3<f32>(0.09, 0.09, 0.10); }
        if style == STYLE_INDUSTRIAL { width = 0.55; sill = 0.72; head = 0.90; lit_share = 0.15; }
        if style == STYLE_HOUSE { width = 0.34; sill = 0.32; head = 0.76; }
        if style == STYLE_CHURCH { width = 0.20; sill = 0.12; head = 0.86; levels = max(storey * 2.2, 8.0); lit_share = 0.1; }
    }
    let row = floor(yy / levels);
    let fy = yy / levels - row;
    let fu = fract(u);
    let column = floor(u);
    // Industrial sheds have one row of high windows, churches one tall one.
    if (style == STYLE_INDUSTRIAL || style == STYLE_CHURCH) && !shop && row > 0.5 {
        return out;
    }
    let du = max(fwidth(u), 1e-4);
    let dy = max(fwidth(yy / levels), 1e-4);
    if max(du, dy) > 0.9 {
        // Far: the windows are smaller than a pixel, the mean of them and the wall.
        let share = width * (head - sill);
        out.albedo = mix(wall, glass, share);
        out.glass = share;
        out.glow = warm * share * lit_share * 1.1 * 0.9;
        return out;
    }
    let half = width * 0.5;
    let across = smoothstep(0.5 - half - du, 0.5 - half + du, fu) * (1.0 - smoothstep(0.5 + half - du, 0.5 + half + du, fu));
    let up = smoothstep(sill - dy, sill + dy, fy) * (1.0 - smoothstep(head - dy, head + dy, fy));
    var pane = across * up;
    // Where a window is smaller than a pixel, the mean of the wall and its windows.
    let tiny = smoothstep(0.35, 0.9, max(du, dy));
    pane = mix(pane, width * (head - sill), tiny);
    var albedo = wall;
    // A storey's slab or balcony shading the wall under it, and the sills.
    if style == STYLE_PLASTER || style == STYLE_STONE || style == STYLE_BRICK {
        let band = 1.0 - smoothstep(0.0, 0.05 + dy, fy);
        albedo *= 1.0 - 0.25 * band * (1.0 - tiny);
        if fract(seed * 13.7) < 0.5 && style == STYLE_PLASTER && !shop {
            // Balconies: a deeper shade under the slab across the window's column.
            let balcony = (1.0 - smoothstep(0.0, 0.12 + dy, fy)) * smoothstep(0.5 - half - 0.12 - du, 0.5 - half - 0.08, fu) * (1.0 - smoothstep(0.5 + half + 0.08, 0.5 + half + 0.12 + du, fu));
            albedo *= 1.0 - 0.35 * balcony * (1.0 - tiny);
        }
    }
    if style == STYLE_GLASS && !shop {
        // Mullions and the opaque spandrels at the slabs.
        albedo = wall * 0.75;
    } else if !shop {
        // A window's frame round it, and the shade of the lintel at the top of the glass.
        let frame_w = 0.05;
        let around = smoothstep(0.5 - half - frame_w - du, 0.5 - half - frame_w + du, fu) * (1.0 - smoothstep(0.5 + half + frame_w - du, 0.5 + half + frame_w + du, fu))
            * smoothstep(sill - frame_w - dy, sill - frame_w + dy, fy) * (1.0 - smoothstep(head + frame_w * 0.5 - dy, head + frame_w * 0.5 + dy, fy));
        albedo = mix(albedo, albedo * 1.2 + vec3<f32>(0.05), max(around - pane, 0.0) * (1.0 - tiny));
        glass = glass * (1.0 - 0.6 * smoothstep(head - 0.12, head, fy));
    }
    if style == STYLE_BRICK && dist < 160.0 && !shop {
        // Courses of brick, a joint every 7.5 cm, the bricks staggered.
        let course = y / 0.075;
        let rowb = floor(course);
        let along = u * 3.3 / 0.24 + 0.5 * fract(rowb * 0.5) * 2.0;
        let joint = max(1.0 - smoothstep(0.0, 0.15, fract(course)), 1.0 - smoothstep(0.0, 0.06, fract(along)));
        albedo = mix(albedo, albedo * 1.35 + vec3<f32>(0.04), joint * 0.6 * (1.0 - smoothstep(60.0, 160.0, dist)));
        albedo *= 0.9 + 0.2 * ihash(vec2<i32>(i32(floor(along)), i32(rowb)));
    }
    // Shutters on the windows of warm plaster, the colour of the building's own.
    if style == STYLE_PLASTER && !shop && fract(seed * 7.1) < 0.45 {
        let outside = abs(fu - 0.5) - half;
        let shutter = smoothstep(-du, du, outside) * (1.0 - smoothstep(0.13 - du, 0.13 + du, outside)) * up;
        let colours = array<vec3<f32>, 3>(vec3<f32>(0.10, 0.20, 0.12), vec3<f32>(0.32, 0.18, 0.10), vec3<f32>(0.42, 0.42, 0.40));
        albedo = mix(albedo, colours[i32(fract(seed * 3.3) * 2.99)], shutter * (1.0 - tiny));
    }
    // A shop's fascia over its window.
    if shop {
        let fascia = smoothstep(0.80, 0.82, fy) * (1.0 - smoothstep(0.92, 0.94, fy));
        albedo = mix(albedo, vec3<f32>(0.08, 0.08, 0.09), fascia * (1.0 - tiny));
    }
    out.albedo = mix(albedo, glass, pane);
    out.glass = pane;
    // After dark some windows are lit, by their window's own number.
    let lit = step(ihash(vec2<i32>(i32(column) + i32(seed * 9173.0), i32(row) * 31 + select(0, 977, shop))), lit_share);
    let bright = 0.6 + 0.8 * ihash(vec2<i32>(i32(row), i32(column) + 17));
    out.glow = warm * pane * mix(lit, lit_share, tiny) * bright * 0.9;
    return out;
}

@fragment
fn building_fragment(in: BuildingOut) -> @location(0) vec4<f32> {
    return building_shade(in);
}

// A building mirrored in the sea (`reflection.rs`): they stand on land too.
@fragment
fn building_mirror_fragment(in: BuildingOut) -> @location(0) vec4<f32> {
    return building_shade(in);
}

// A building at a fragment, as `building_fragment` writes it.
fn building_shade(in: BuildingOut) -> vec4<f32> {
    var n = normalize(in.normal);
    let dist = length(in.rel);
    // [materials] the photograph of the wall, the roof or the sheet.
    let tex_gx = dpdx(in.tex.xy);
    let tex_gy = dpdy(in.tex.xy);
    var surface = Surface(vec3<f32>(1.0), vec3<f32>(0.0), 1.0, 1.0);
    if in.tex.w > 0.5 {
        surface = building_material(in.tex, in.tangent, n, tex_gx, tex_gy, dist);
    }
    if dot(n, -in.rel) < 0.0 {
        n = -n;
    }
    n = normalize(n + surface.lean);   // [materials]
    let sun = frame.sun_dir.xyz;
    let day = frame.sun_color.w;
    var albedo = in.color.rgb * surface.color;   // [materials] surface
    // [materials] Slate is grey: most of the red of the roof it was is taken out.
    if in.tex.w > 0.5 && i32(in.tex.z + 0.5) == M_ROOF_SLATE {
        albedo = mix(albedo, vec3<f32>(luma(albedo)) * vec3<f32>(0.95, 1.0, 1.08), 0.85);
    }
    var glow = vec3<f32>(0.0);
    var gloss = 0.0;
    var glossy_far = 0.0;
    if in.facade.x >= -0.5 && in.color.a < 0.25 && abs(n.y) < 0.5 {
        // A building of the maps: its facade.
        let f = facade(i32(in.facade.x + 0.5), in.facade.y, in.facade.w, in.facade.z, in.wall.y, albedo, in.wall.z, dist);
        albedo = f.albedo;
        gloss = f.glass;
        glossy_far = select(0.0, f.glass, i32(in.facade.x + 0.5) == STYLE_GLASS);
        glow = f.glow * (1.0 - smoothstep(0.25, 0.6, day));
    } else if in.color.a < 0.25 && abs(n.y) < 0.5 {
        // Windows: one to each 3.2 m of wall and 2.9 m of height. Far away they are a shade on the wall.
        let cell = vec2<f32>(in.wall.x / 3.2 + 0.5, in.wall.y / 2.9);
        let f = fract(cell);
        let soft = clamp(dist * 0.0006, 0.01, 0.2);
        let pane = smoothstep(0.30 - soft, 0.30 + soft, f.x) * (1.0 - smoothstep(0.70 - soft, 0.70 + soft, f.x)) * smoothstep(0.36 - soft, 0.36 + soft, f.y) * (1.0 - smoothstep(0.80 - soft, 0.80 + soft, f.y));
        let window = mix(pane, 0.18, smoothstep(500.0, 1600.0, dist));
        albedo = mix(albedo, vec3<f32>(0.035, 0.045, 0.055), window);
        gloss = window;
        // After dark some are lit.
        let lit = step(ihash(vec2<i32>(floor(cell)) + vec2<i32>(i32(in.wall.z * 9173.0), 7)), 0.38);
        glow = vec3<f32>(1.0, 0.62, 0.28) * window * lit * (1.0 - smoothstep(0.25, 0.6, day)) * 0.9;
    }
    let ndl = max(dot(n, sun), 0.0) * body_shadow(in.rel) * cloud_shadow(in.rel) * terrain_shadow(in.rel);
    // A wall in the shade is lit by the whole sky and by the sunlit ground in front of it.
    var col = albedo * (light_at(in.rel) * ndl + sky_light(n, in.rel) * surface.ao);   // [materials] ao
    if frame.fire_count.x > 0.0 {
        col += albedo * fire_light(in.rel, n);
    }
    // Glass mirrors the sky; a curtain wall of it, from as far as it is seen.
    let v = -in.rel / dist;
    let fresnel = 0.25 + 0.5 * pow(1.0 - max(dot(v, n), 0.0), 3.0);
    // (`sky_seen` is the sky by day: after dark the glass mirrors a dark sky.)
    let mirrored = sky_seen(reflect(-v, n)) * 0.9 * mix(0.02, 1.0, smoothstep(0.05, 0.5, day));
    col = mix(col, mirrored, max(gloss * 0.14 * (1.0 - smoothstep(300.0, 900.0, dist)), glossy_far * fresnel));
    col = apply_fog(col + glow, in.rel);
    return vec4<f32>(col, 1.0);
}

// The shadow of a building: its hull flattened on to the ground along the sunlight. Only the faces
// turned away from the sun are drawn, so each convex part of the hull darkens the ground once.
@vertex
fn building_shadow_vertex(in: TreeIn) -> ShadowOut {
    var o: ShadowOut;
    let cs = cos(in.tint.w);
    let sn = sin(in.tint.w);
    let p = in.position * in.place.w;
    let base = in.place.xyz + chunk.offset.xyz;
    let n = vec3<f32>(in.normal.x * cs - in.normal.z * sn, in.normal.y, in.normal.x * sn + in.normal.z * cs);
    let sun = frame.sun_dir.xyz;
    let dist = length(base);
    var strength = 0.5 * (1.0 - smoothstep(HOUSE_SHADOW_RANGE * 0.6, HOUSE_SHADOW_RANGE, dist)) * smoothstep(0.25, 0.6, frame.sun_color.w) * smoothstep(0.04, 0.2, sun.y);
    if dot(n, sun) > -0.0001 || strength < 0.01 {
        o.position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return o;
    }
    strength *= smoothstep(0.42, 0.95, cloud_shadow(base)) * luma(haze_through(base));
    // A low sun would throw the shadow to the horizon; it is cut short.
    let height = max(p.y - 0.4, 0.0);
    let reach = height / max(sun.y, 0.24);
    let rel = base + vec3<f32>(p.x * cs - p.z * sn - sun.x * reach, 0.4 + dist * 0.0008, p.x * sn + p.z * cs - sun.z * reach);
    o.position = frame.view_proj * vec4<f32>(rel, 1.0);
    o.uv = vec2<f32>(0.0);
    o.strength = strength;
    return o;
}

// ---------------------------------------------------------------- the ground of a town
//
// Streets, pavements and plots with their gardens and yards, on the grid the houses stand on
// (`sim_core::scenery::PLOT`: plots of 24 m by 19 m, every fourth row and fifth column a street).

// [materials] What `town_ground` painted at the fragment, for the photographs: how much of it is
// town, and its material: the street asphalt, the pavement concrete, the plot grass, bare earth or
// paving. One material, switched half way across the soft edges: the material only gives the grain,
// and blending the three cost 0.15 ms a frame over the whole ground.
struct TownAt {
    weight: f32,
    layer: i32,
};
var<private> town_at: TownAt;

fn town_ground(albedo: vec3<f32>, xz: vec2<f32>, dist: f32) -> vec3<f32> {
    var result = albedo;
    let count = i32(frame.extra.z);
    // A point is in the town whose middle is nearest of those that reach it: a region's may
    // overlap (`sim_core::scenery` lays its houses the same way); a generated world's never do.
    var nearest = -1;
    var least = 1e9;
    for (var i = 0; i < count; i++) {
        let r = length(xz - frame.towns[i].xy);
        if r <= frame.towns[i].z * 1.15 && r < least {
            nearest = i;
            least = r;
        }
    }
    if nearest >= 0 {
        let i = nearest;
        let town = frame.towns[i];
        let d = xz - town.xy;
        let r = length(d);
        let weight = (1.0 - smoothstep(town.z * 0.75, town.z * 1.15, r)) * (1.0 - smoothstep(3500.0, 8000.0, dist));
        // Along the two directions of the grid: north is -z.
        let cg = cos(town.w);
        let sg = sin(town.w);
        let g = vec2<f32>(-d.y * cg + d.x * sg, d.y * sg + d.x * cg);
        let plot = floor(g / vec2<f32>(24.0, 19.0) + 0.5);
        let within = g - plot * vec2<f32>(24.0, 19.0);
        let street = vec2<f32>(96.0, 95.0);
        let to_street = abs(g - street * floor(g / street + 0.5));
        let soft = clamp(dist * 0.0012, 0.15, 8.0);
        let lane = min(to_street.x, to_street.y);
        let h = ihash(vec2<i32>(plot) + vec2<i32>(i * 131, 17));
        // Gardens, bare yards and paving; the middle of the town is paved.
        var ground = vec3<f32>(0.070, 0.090, 0.042);
        if h > 0.42 { ground = vec3<f32>(0.20, 0.17, 0.115); }
        if h > 0.74 || r < town.z * 0.16 { ground = vec3<f32>(0.20, 0.195, 0.185); }
        var plot_layer = M_GRASS_SHORT;   // [materials]
        if h > 0.42 { plot_layer = M_DIRT_ROAD; }
        if h > 0.74 || r < town.z * 0.16 { plot_layer = M_CONCRETE; }
        ground *= 0.85 + 0.3 * fract(h * 13.0);
        // Walls between the plots.
        let edge = min(12.0 - abs(within.x), 9.5 - abs(within.y));
        ground = mix(ground, vec3<f32>(0.11, 0.10, 0.09), (1.0 - smoothstep(0.25, 0.5 + soft, edge)) * 0.8);
        ground = mix(ground, vec3<f32>(0.20, 0.195, 0.185), 1.0 - smoothstep(5.6, 5.8 + soft, lane));
        ground = mix(ground, vec3<f32>(0.065, 0.065, 0.07), 1.0 - smoothstep(3.6, 3.8 + soft, lane));
        result = mix(result, ground, weight);
        if weight > town_at.weight {   // [materials]
            town_at = TownAt(weight, select(select(plot_layer, M_CONCRETE, lane < 5.7 + soft * 0.5), M_ASPHALT, lane < 3.7 + soft * 0.5));
        }
    }
    return result;
}

// ---------------------------------------------------------------- aerodromes
//
// The grass beside a runway is kept short and shows the passes of the mower.
fn airfield_ground(albedo: vec3<f32>, xz: vec2<f32>, dist: f32) -> vec3<f32> {
    var result = albedo;
    let count = i32(frame.extra.w);
    for (var i = 0; i < count; i++) {
        let d = xz - frame.airports_a[i].xy;
        let dir = frame.airports_a[i].zw;
        let along = dot(d, dir);
        let across = dot(d, vec2<f32>(-dir.y, dir.x));
        let hl = frame.airports_b[i].x;
        let hw = frame.airports_b[i].y;
        if abs(along) > hl + 90.0 || abs(across) > hw + 60.0 {
            continue;
        }
        let soft = clamp(dist * 0.0012, 0.2, 6.0);
        let mown = (1.0 - smoothstep(hl + 60.0, hl + 75.0, abs(along))) * (1.0 - smoothstep(hw + 38.0, hw + 46.0, abs(across)));
        let pass_ = smoothstep(0.5 - soft / 14.0, 0.5 + soft / 14.0, abs(fract(across / 14.0) - 0.5) * 2.0);
        let stripes = mix(0.5, pass_, 1.0 - smoothstep(1500.0, 5000.0, dist));
        result = mix(result, result * vec3<f32>(0.80, 0.98, 0.72) * (0.92 + 0.16 * stripes), mown);
    }
    return result;
}

