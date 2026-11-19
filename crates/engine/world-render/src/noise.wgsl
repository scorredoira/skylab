// The noise the effects are carved with (effects.wgsl, fire_sim.wgsl, fire_draw.wgsl): a volume of
// 128 texels a side that repeats in every direction, made once on the GPU (`particles::make_noise`).
//   R = soft lumps: gradient noise in four octaves under round billows, four cells to the volume,
//   G = round billows, eight cells to the volume, in three octaves,
//   B = the same twice as fine,
//   A = wisps: gradient noise in three octaves, sixteen cells to the volume, 0.5 its middle.
// The billows are one minus the distance to the nearest of one point scattered in every cell
// (Worley 1996); the gradient noise is Perlin's (2002), its corners hashed with the PCG of
// Jarzynski and Olano (2020). Every lattice wraps at its period, which is what makes the volume repeat.

@group(0) @binding(0) var volume: texture_storage_3d<rgba8unorm, write>;

const SIDE: f32 = 128.0;

// Three numbers 0..1 of a lattice corner, the lattice repeating every `period` corners.
fn corner(at: vec3<i32>, period: i32, salt: u32) -> vec3<f32> {
    let wrapped = ((at % vec3<i32>(period)) + vec3<i32>(period)) % vec3<i32>(period);
    var v = (vec3<u32>(wrapped) + vec3<u32>(salt, salt * 7u + 3u, salt * 13u + 5u)) * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v = v ^ (v >> vec3<u32>(16u));
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return vec3<f32>(v >> vec3<u32>(8u)) / 16777216.0;
}

// Gradient noise, about -0.7..0.7.
fn gradient(p: vec3<f32>, period: i32, salt: u32) -> f32 {
    let base = vec3<i32>(floor(p));
    let f = fract(p);
    let ease = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    var sum = 0.0;
    for (var c = 0; c < 8; c++) {
        let o = vec3<i32>(c & 1, (c >> 1) & 1, (c >> 2) & 1);
        let slope = normalize(corner(base + o, period, salt) * 2.0 - 1.0 + vec3<f32>(1e-4));
        let share = mix(1.0 - ease, ease, vec3<f32>(o));
        sum += share.x * share.y * share.z * dot(slope, f - vec3<f32>(o));
    }
    return sum;
}

// One minus the distance to the nearest scattered point: round lumps, 1 at their middles.
fn lumps(p: vec3<f32>, period: i32, salt: u32) -> f32 {
    let base = vec3<i32>(floor(p));
    let f = fract(p);
    var nearest = 4.0;
    for (var c = 0; c < 27; c++) {
        let o = vec3<i32>(c % 3, (c / 3) % 3, c / 9) - vec3<i32>(1);
        let to = vec3<f32>(o) + corner(base + o, period, salt) - f;
        nearest = min(nearest, dot(to, to));
    }
    return 1.0 - min(sqrt(nearest), 1.0);
}

// Lumps on lumps: three octaves, `cells` of the coarsest to the volume; `p` is 0..1.
fn billows(p: vec3<f32>, cells: i32, salt: u32) -> f32 {
    return lumps(p * f32(cells), cells, salt) * 0.56 + lumps(p * f32(cells * 2), cells * 2, salt + 1u) * 0.29 + lumps(p * f32(cells * 4), cells * 4, salt + 2u) * 0.15;
}

// Gradient noise in `octaves`, `cells` of the coarsest to the volume, 0..1 with 0.5 its middle.
fn clouds(p: vec3<f32>, cells: i32, octaves: i32, salt: u32) -> f32 {
    var sum = 0.0;
    var weight = 0.5;
    var n = cells;
    for (var i = 0; i < octaves; i++) {
        sum += gradient(p * f32(n), n, salt + u32(i)) * weight;
        weight *= 0.5;
        n *= 2;
    }
    return clamp(0.5 + sum * 1.25, 0.0, 1.0);
}

// Sums of octaves crowd about their middle: spread out from it, so that four fifths of the volume
// lie between 0.1 and 0.9 and what reads the noise has all of 0..1 to cut it at.
fn spread(v: f32, middle: f32) -> f32 {
    return clamp(0.5 + (v - middle) * 2.7, 0.0, 1.0);
}

@compute @workgroup_size(4, 4, 4)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = (vec3<f32>(id) + 0.5) / SIDE;
    let soft = 0.5 * clouds(p, 4, 4, 11u) + 0.62 * billows(p, 4, 23u) - 0.08;
    textureStore(volume, vec3<i32>(id), vec4<f32>(spread(soft, 0.467), spread(billows(p, 8, 37u), 0.48), spread(billows(p, 16, 51u), 0.48), spread(clouds(p, 16, 3, 67u), 0.5)));
}
