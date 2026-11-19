// What the sky's tables (`sky.wgsl`, made by `sky.rs`) and the shaders that read them
// (`shaders.wgsl`) agree on: the planet, the sizes of the tables and how a direction or a distance
// is laid out in them. Hillaire 2020, "A Scalable and Production Ready Sky and Atmosphere Rendering
// Technique"; the transmittance's layout is Bruneton's (2017).
//
// The tables' frame: the planet's centre below the eye, y up through the eye, the sun in the plane
// x y on the side of +x. The sky is the same on both sides of that plane (the moon is in it too, on
// the other side: `sky::moon_direction`), so a table holds the azimuth from the sun's side only,
// 0 to pi.

const GROUND_RADIUS: f32 = 6360000.0;
const TOP_RADIUS: f32 = 6460000.0;
const SKY_PI: f32 = 3.14159265;

const TRANSMITTANCE_SIZE = vec2<f32>(256.0, 64.0);
const MULTIPLE_SIZE = vec2<f32>(32.0, 32.0);
const SKY_VIEW_SIZE = vec2<f32>(192.0, 108.0);
const AERIAL_SIZE = vec3<f32>(32.0, 48.0, 32.0);
// The ambient light by the direction a surface faces: 8 cosines of its azimuth from the sun by 8
// heights, at the ground's altitude (the first 8 rows) and at the eye's (the next 8).
const AMBIENT_SIZE = vec2<f32>(8.0, 16.0);

// A coordinate 0..1 to the middle of the first texel..the middle of the last, and back.
fn texel_from_unit(x: f32, size: f32) -> f32 {
    return 0.5 / size + x * (1.0 - 1.0 / size);
}

fn unit_from_texel(u: f32, size: f32) -> f32 {
    return (u - 0.5 / size) / (1.0 - 1.0 / size);
}

// Distance from a point at radius `r` along `mu` (the cosine from the vertical) to the top of the air.
fn to_top(r: f32, mu: f32) -> f32 {
    let disc = r * r * (mu * mu - 1.0) + TOP_RADIUS * TOP_RADIUS;
    return max(0.0, -r * mu + sqrt(max(disc, 0.0)));
}

// The transmittance table's place for an altitude and the cosine of a direction from the vertical.
fn transmittance_uv(altitude: f32, mu: f32) -> vec2<f32> {
    let r = GROUND_RADIUS + clamp(altitude, 0.0, TOP_RADIUS - GROUND_RADIUS);
    let h = sqrt(TOP_RADIUS * TOP_RADIUS - GROUND_RADIUS * GROUND_RADIUS);
    let rho = sqrt(max(r * r - GROUND_RADIUS * GROUND_RADIUS, 0.0));
    let d = to_top(r, mu);
    let d_min = TOP_RADIUS - r;
    let d_max = rho + h;
    let x_mu = (d - d_min) / max(d_max - d_min, 1.0);
    return vec2<f32>(texel_from_unit(saturate(x_mu), TRANSMITTANCE_SIZE.x), texel_from_unit(rho / h, TRANSMITTANCE_SIZE.y));
}

// The cosine from the vertical of the horizon seen from an altitude: below it a ray meets the ground.
fn horizon_mu(altitude: f32) -> f32 {
    let r = GROUND_RADIUS + max(altitude, 0.0);
    return -sqrt(max(1.0 - (GROUND_RADIUS / r) * (GROUND_RADIUS / r), 0.0));
}

// Where a direction lies in the sky-view table and the aerial perspective, 0..1 each way, from
// the cosine `c` of its azimuth from the sun and its height `y` (its vertical component): most of
// their texels near the sun's side and near the horizon, where the sky changes fastest. No angle
// is taken, so that the shaders that read it per pixel pay for none.
fn sky_unit(c: f32, y: f32) -> vec2<f32> {
    return vec2<f32>(sqrt(sqrt(saturate((1.0 - c) * 0.5))), 0.5 + 0.5 * sign(y) * sqrt(min(abs(y), 1.0)));
}

// The direction in the tables' frame at a place of `sky_unit`.
fn sky_direction(unit: vec2<f32>) -> vec3<f32> {
    let s = unit.x * unit.x;
    let c = 1.0 - 2.0 * s * s;
    let t = unit.y * 2.0 - 1.0;
    let y = sign(t) * t * t;
    let level = sqrt(max(1.0 - y * y, 0.0));
    return vec3<f32>(level * c, y, level * sqrt(max(1.0 - c * c, 0.0)));
}

// The sky-view table's place for a direction (`sky_unit`).
fn sky_view_uv(c: f32, y: f32) -> vec2<f32> {
    let unit = sky_unit(c, y);
    return vec2<f32>(texel_from_unit(unit.x, SKY_VIEW_SIZE.x), texel_from_unit(unit.y, SKY_VIEW_SIZE.y));
}

// The aerial perspective's place for a direction and a distance up to `far`: the distance by its
// square root, so the near slices are thin.
fn aerial_uvw(c: f32, y: f32, distance: f32, far: f32) -> vec3<f32> {
    let unit = sky_unit(c, y);
    let w = sqrt(saturate(distance / far));
    return vec3<f32>(texel_from_unit(unit.x, AERIAL_SIZE.x), texel_from_unit(unit.y, AERIAL_SIZE.y), texel_from_unit(w, AERIAL_SIZE.z));
}

// The ambient table's place for the direction a surface faces, evenly in the cosine of its azimuth
// from the sun and in its height, in the rows of the ground (`row` 0) or of the eye (1).
fn ambient_uv(c: f32, y: f32, row: f32) -> vec2<f32> {
    let u = texel_from_unit(saturate((1.0 - c) * 0.5), AMBIENT_SIZE.x);
    let v = texel_from_unit(saturate(y * 0.5 + 0.5), AMBIENT_SIZE.y * 0.5);
    return vec2<f32>(u, (v + row) * 0.5);
}

// The share of a light of 1 above the air that reaches an altitude, coming from `mu` from the
// vertical: none once the planet is in the way, which it crosses over a little more than the
// sun's disc (`Air::transmittance`, on the CPU, is the same).
fn light_through_table(table: texture_2d<f32>, s: sampler, altitude: f32, mu: f32) -> vec3<f32> {
    let horizon = horizon_mu(altitude);
    let seen = smoothstep(horizon - 0.006, horizon + 0.006, mu);
    return textureSampleLevel(table, s, transmittance_uv(altitude, mu), 0.0).rgb * seen;
}
