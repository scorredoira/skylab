// Ported from takram-design-engineering/three-geospatial (packages/clouds/src/shaders/perlin.glsl,
// tileableNoise.glsl, cloudShape.frag, cloudShapeDetail.frag, turbulence.frag, localWeather.frag),
// MIT, see THIRD_PARTY_NOTICES.md. Theirs in turn: the periodic Perlin noise is GLM's
// (glm/gtc/noise.inl, G-Truc Creation, MIT) and the tileable volume noise Sébastien Hillaire's
// (TileableVolumeNoise, MIT); both notices are in THIRD_PARTY_NOTICES.md.
//
// The textures the clouds are made of, drawn once (`cloud_noise.rs`): the shape and its detail (a
// layer of the volume to a draw), the turbulence and the weather.

fn modulo4(x: vec4<f32>, y: vec4<f32>) -> vec4<f32> {
    return x - y * floor(x / y);
}

fn modulo3(x: vec3<f32>, y: f32) -> vec3<f32> {
    return x - y * floor(x / y);
}

fn mod289(x: vec4<f32>) -> vec4<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}

fn permute(v: vec4<f32>) -> vec4<f32> {
    return mod289((v * 34.0 + 1.0) * v);
}

fn taylor_inv_sqrt(r: vec4<f32>) -> vec4<f32> {
    return 1.79284291400159 - 0.85373472095314 * r;
}

fn fade(v: vec4<f32>) -> vec4<f32> {
    return v * v * v * (v * (v * 6.0 - 15.0) + 10.0);
}

// The gradients at the four corners `ixy` of a cell: (x, y, z, w) of each as columns.
fn gradients(ixy: vec4<f32>) -> mat4x4<f32> {
    var gx = ixy / 7.0;
    var gy = floor(gx) / 7.0;
    var gz = floor(gy) / 6.0;
    gx = fract(gx) - 0.5;
    gy = fract(gy) - 0.5;
    gz = fract(gz) - 0.5;
    let gw = vec4<f32>(0.75) - abs(gx) - abs(gy) - abs(gz);
    let sw = step(gw, vec4<f32>(0.0));
    gx -= sw * (step(vec4<f32>(0.0), gx) - 0.5);
    gy -= sw * (step(vec4<f32>(0.0), gy) - 0.5);
    return mat4x4<f32>(gx, gy, gz, gw);
}

// Classic Perlin noise in four dimensions, periodic every `rep`.
fn perlin(position: vec4<f32>, rep: vec4<f32>) -> f32 {
    let pi0 = modulo4(floor(position), rep);
    let pi1 = modulo4(pi0 + 1.0, rep);
    let pf0 = fract(position);
    let pf1 = pf0 - 1.0;
    let ix = vec4<f32>(pi0.x, pi1.x, pi0.x, pi1.x);
    let iy = vec4<f32>(pi0.y, pi0.y, pi1.y, pi1.y);
    let iz0 = vec4<f32>(pi0.z);
    let iz1 = vec4<f32>(pi1.z);
    let iw0 = vec4<f32>(pi0.w);
    let iw1 = vec4<f32>(pi1.w);
    let ixy = permute(permute(ix) + iy);
    let ixy0 = permute(ixy + iz0);
    let ixy1 = permute(ixy + iz1);
    let g00 = gradients(permute(ixy0 + iw0));
    let g01 = gradients(permute(ixy0 + iw1));
    let g10 = gradients(permute(ixy1 + iw0));
    let g11 = gradients(permute(ixy1 + iw1));

    var g0000 = vec4<f32>(g00[0].x, g00[1].x, g00[2].x, g00[3].x);
    var g1000 = vec4<f32>(g00[0].y, g00[1].y, g00[2].y, g00[3].y);
    var g0100 = vec4<f32>(g00[0].z, g00[1].z, g00[2].z, g00[3].z);
    var g1100 = vec4<f32>(g00[0].w, g00[1].w, g00[2].w, g00[3].w);
    var g0010 = vec4<f32>(g10[0].x, g10[1].x, g10[2].x, g10[3].x);
    var g1010 = vec4<f32>(g10[0].y, g10[1].y, g10[2].y, g10[3].y);
    var g0110 = vec4<f32>(g10[0].z, g10[1].z, g10[2].z, g10[3].z);
    var g1110 = vec4<f32>(g10[0].w, g10[1].w, g10[2].w, g10[3].w);
    var g0001 = vec4<f32>(g01[0].x, g01[1].x, g01[2].x, g01[3].x);
    var g1001 = vec4<f32>(g01[0].y, g01[1].y, g01[2].y, g01[3].y);
    var g0101 = vec4<f32>(g01[0].z, g01[1].z, g01[2].z, g01[3].z);
    var g1101 = vec4<f32>(g01[0].w, g01[1].w, g01[2].w, g01[3].w);
    var g0011 = vec4<f32>(g11[0].x, g11[1].x, g11[2].x, g11[3].x);
    var g1011 = vec4<f32>(g11[0].y, g11[1].y, g11[2].y, g11[3].y);
    var g0111 = vec4<f32>(g11[0].z, g11[1].z, g11[2].z, g11[3].z);
    var g1111 = vec4<f32>(g11[0].w, g11[1].w, g11[2].w, g11[3].w);

    let norm00 = taylor_inv_sqrt(vec4<f32>(dot(g0000, g0000), dot(g0100, g0100), dot(g1000, g1000), dot(g1100, g1100)));
    g0000 *= norm00.x;
    g0100 *= norm00.y;
    g1000 *= norm00.z;
    g1100 *= norm00.w;
    let norm01 = taylor_inv_sqrt(vec4<f32>(dot(g0001, g0001), dot(g0101, g0101), dot(g1001, g1001), dot(g1101, g1101)));
    g0001 *= norm01.x;
    g0101 *= norm01.y;
    g1001 *= norm01.z;
    g1101 *= norm01.w;
    let norm10 = taylor_inv_sqrt(vec4<f32>(dot(g0010, g0010), dot(g0110, g0110), dot(g1010, g1010), dot(g1110, g1110)));
    g0010 *= norm10.x;
    g0110 *= norm10.y;
    g1010 *= norm10.z;
    g1110 *= norm10.w;
    let norm11 = taylor_inv_sqrt(vec4<f32>(dot(g0011, g0011), dot(g0111, g0111), dot(g1011, g1011), dot(g1111, g1111)));
    g0011 *= norm11.x;
    g0111 *= norm11.y;
    g1011 *= norm11.z;
    g1111 *= norm11.w;

    let n0000 = dot(g0000, pf0);
    let n1000 = dot(g1000, vec4<f32>(pf1.x, pf0.y, pf0.z, pf0.w));
    let n0100 = dot(g0100, vec4<f32>(pf0.x, pf1.y, pf0.z, pf0.w));
    let n1100 = dot(g1100, vec4<f32>(pf1.x, pf1.y, pf0.z, pf0.w));
    let n0010 = dot(g0010, vec4<f32>(pf0.x, pf0.y, pf1.z, pf0.w));
    let n1010 = dot(g1010, vec4<f32>(pf1.x, pf0.y, pf1.z, pf0.w));
    let n0110 = dot(g0110, vec4<f32>(pf0.x, pf1.y, pf1.z, pf0.w));
    let n1110 = dot(g1110, vec4<f32>(pf1.x, pf1.y, pf1.z, pf0.w));
    let n0001 = dot(g0001, vec4<f32>(pf0.x, pf0.y, pf0.z, pf1.w));
    let n1001 = dot(g1001, vec4<f32>(pf1.x, pf0.y, pf0.z, pf1.w));
    let n0101 = dot(g0101, vec4<f32>(pf0.x, pf1.y, pf0.z, pf1.w));
    let n1101 = dot(g1101, vec4<f32>(pf1.x, pf1.y, pf0.z, pf1.w));
    let n0011 = dot(g0011, vec4<f32>(pf0.x, pf0.y, pf1.z, pf1.w));
    let n1011 = dot(g1011, vec4<f32>(pf1.x, pf0.y, pf1.z, pf1.w));
    let n0111 = dot(g0111, vec4<f32>(pf0.x, pf1.y, pf1.z, pf1.w));
    let n1111 = dot(g1111, pf1);

    let f = fade(pf0);
    let n_0w = mix(vec4<f32>(n0000, n1000, n0100, n1100), vec4<f32>(n0001, n1001, n0101, n1101), f.w);
    let n_1w = mix(vec4<f32>(n0010, n1010, n0110, n1110), vec4<f32>(n0011, n1011, n0111, n1111), f.w);
    let n_zw = mix(n_0w, n_1w, f.z);
    let n_yzw = mix(n_zw.xy, n_zw.zw, f.y);
    return 2.2 * mix(n_yzw.x, n_yzw.y, f.x);
}

fn hash(n: f32) -> f32 {
    return fract(sin(n + 1.951) * 43758.5453);
}

fn value_noise(x: vec3<f32>) -> f32 {
    let p = floor(x);
    var f = fract(x);
    f = f * f * (3.0 - 2.0 * f);
    let n = p.x + p.y * 57.0 + 113.0 * p.z;
    return mix(
        mix(mix(hash(n + 0.0), hash(n + 1.0), f.x), mix(hash(n + 57.0), hash(n + 58.0), f.x), f.y),
        mix(mix(hash(n + 113.0), hash(n + 114.0), f.x), mix(hash(n + 170.0), hash(n + 171.0), f.x), f.y),
        f.z,
    );
}

// The square of the distance to the nearest point of a lattice of `cells` to a side, which tiles.
fn worley(p: vec3<f32>, cells: f32) -> f32 {
    let cell = p * cells;
    var d = 1.0e10;
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            for (var z = -1; z <= 1; z++) {
                var tp = floor(cell) + vec3<f32>(f32(x), f32(y), f32(z));
                tp = cell - tp - value_noise(modulo3(tp, cells));
                d = min(d, dot(tp, tp));
            }
        }
    }
    return saturate(d);
}

// Octaves of the Perlin noise, each of twice the frequency and half the weight; not clamped.
fn perlin_octaves(point: vec3<f32>, frequency: vec3<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var weights = 0.0;
    var weight = 1.0;
    var next = frequency;
    for (var i = 0; i < octaves; i++) {
        sum += perlin(vec4<f32>(point * next, 0.0), vec4<f32>(next, 1.0)) * weight;
        weights += weight;
        weight *= 0.5;
        next *= 2.0;
    }
    return sum / weights;
}

struct NoiseOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Which layer of the volume, counted from 0.
    @location(1) @interpolate(flat) layer: u32,
};

@vertex
fn noise_vertex(@builtin(vertex_index) vid: u32, @builtin(instance_index) layer: u32) -> NoiseOut {
    let p = vec2<f32>(select(-1.0, 3.0, vid == 1u), select(-1.0, 3.0, vid == 2u));
    return NoiseOut(vec4<f32>(p, 0.0, 1.0), p * 0.5 + 0.5, layer);
}

fn perlin_worley(point: vec3<f32>) -> f32 {
    let p = saturate(perlin_octaves(point, vec3<f32>(8.0), 3));
    let cells = 4.0;
    let noise = vec3<f32>(1.0 - worley(point, cells * 2.0), 1.0 - worley(point, cells * 8.0), 1.0 - worley(point, cells * 14.0));
    let fbm = dot(noise, vec3<f32>(0.625, 0.25, 0.125));
    return fbm + p * (1.0 - fbm);
}

fn worley_fbm(point: vec3<f32>, cells: f32) -> f32 {
    let noise = vec4<f32>(1.0 - worley(point, cells), 1.0 - worley(point, cells * 2.0), 1.0 - worley(point, cells * 4.0), 1.0 - worley(point, cells * 8.0));
    let fbm = vec3<f32>(dot(noise.xyz, vec3<f32>(0.625, 0.25, 0.125)), dot(noise.yzw, vec3<f32>(0.625, 0.25, 0.125)), dot(noise.zw, vec2<f32>(0.75, 0.25)));
    return dot(fbm, vec3<f32>(0.625, 0.25, 0.125));
}

// The shape, 128 to a side: Perlin-Worley worn by Worley.
@fragment
fn shape_fragment(in: NoiseOut) -> @location(0) vec4<f32> {
    let point = vec3<f32>(in.uv, f32(in.layer) / 128.0);
    let low = worley_fbm(point, 8.0) - 1.0;
    return vec4<f32>((perlin_worley(point) - low) / (1.0 - low));
}

// The detail of the shape, 32 to a side.
@fragment
fn detail_fragment(in: NoiseOut) -> @location(0) vec4<f32> {
    return vec4<f32>(worley_fbm(vec3<f32>(in.uv, f32(in.layer) / 32.0), 2.0));
}

fn turbulence_perlin(point: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        perlin_octaves(point, vec3<f32>(12.0), 3),
        perlin_octaves(point.yzx + vec3<f32>(-19.1, 33.4, 47.2), vec3<f32>(12.0), 3),
        perlin_octaves(point.zxy + vec3<f32>(74.2, -124.5, 99.4), vec3<f32>(12.0), 3),
    );
}

// The turbulence: the curl of three Perlin noises, as a direction.
@fragment
fn turbulence_fragment(in: NoiseOut) -> @location(0) vec4<f32> {
    let point = vec3<f32>(in.uv, 0.0);
    let delta = 0.1;
    let dx = vec3<f32>(delta, 0.0, 0.0);
    let dy = vec3<f32>(0.0, delta, 0.0);
    let dz = vec3<f32>(0.0, 0.0, delta);
    let px0 = turbulence_perlin(point - dx);
    let px1 = turbulence_perlin(point + dx);
    let py0 = turbulence_perlin(point - dy);
    let py1 = turbulence_perlin(point + dy);
    let pz0 = turbulence_perlin(point - dz);
    let pz1 = turbulence_perlin(point + dz);
    let x = py1.z - py0.z - pz1.y + pz0.y;
    let y = pz1.x - pz0.x - px1.z + px0.z;
    let z = px1.y - px0.y - py1.x + py0.x;
    return vec4<f32>(0.5 * normalize(vec3<f32>(x, y, z) / (2.0 * delta)) + 0.5, 1.0);
}

fn weather_worley(point: vec3<f32>, frequency: f32) -> f32 {
    var noise = 0.0;
    var f = frequency;
    var amplitude = 0.4;
    for (var i = 0; i < 4; i++) {
        noise += amplitude * (1.0 - worley(point, f));
        f *= 2.0;
        amplitude *= 0.95;
    }
    return noise;
}

// The weather: r = the low clouds, g = the middle ones, b = the high ones, and (ours) a = the
// relief of an overcast sheet: rolls a few kilometres long in patches of some fifteen, 0 where the
// sheet is thick and its base low to 1 where it thins (`cloud_weather_at`).
@fragment
fn weather_fragment(in: NoiseOut) -> @location(0) vec4<f32> {
    let point = vec3<f32>(in.uv, 0.0);
    let middle = smoothstep(1.0, 1.4, weather_worley(point + vec3<f32>(0.5), 8.0));
    let low = saturate(smoothstep(0.8, 1.4, weather_worley(point, 16.0)) - middle);
    let high = smoothstep(-0.5, 0.5, perlin_octaves(point, vec3<f32>(6.0, 12.0, 1.0), 8));
    let rolls = perlin_octaves(point + vec3<f32>(0.31, 0.72, 0.0), vec3<f32>(26.0, 44.0, 1.0), 4);
    let patches = perlin_octaves(point + vec3<f32>(0.83, 0.17, 0.0), vec3<f32>(7.0, 6.0, 1.0), 3);
    return vec4<f32>(low, middle, high, smoothstep(-0.32, 0.32, 0.6 * rolls + 0.5 * patches));
}

// A level of a picture from the one above it: each texel the mean of four.
@group(0) @binding(0) var halved: texture_2d<f32>;
@group(0) @binding(1) var halved_sampler: sampler;

@fragment
fn halve_fragment(in: NoiseOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(halved, halved_sampler, vec2<f32>(in.uv.x, 1.0 - in.uv.y), 0.0);
}
