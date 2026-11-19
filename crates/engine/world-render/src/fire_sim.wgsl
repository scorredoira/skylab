// The fire of a crash as a gas on a grid, simulated on the GPU (`fire.rs` says where the grid is,
// what feeds it and when; fire_common.wgsl what a cell holds). After the grid's numbers and
// fire_common.wgsl. One step of DT seconds is these passes, in order:
//
//   whirl       the curl of the velocity, for the confinement, and how much it spreads
//   carry       everything carried along the flow (Stam 1999: followed back along the velocity
//               and read there), then fed, burnt, cooled, lifted and stirred
//   spread      how much each cell's flow spreads, less what burning there makes the gas grow
//   settle_*    the pressure that takes that spreading away, relaxed in place: the cells of one
//               colour of a chessboard from those of the other, then the other way, over-relaxed
//   release     the velocity less the pressure's slope: what flows in flows out, but where it burns
//
// and once a frame, for the drawing: `lighting` (how much smoke lies between each place and the
// sun, and above it) and `glow_spread` (the fire's own light, coarser and coarser).
//
// The velocity is kept relative to the wind, so a grid that has just been made (all zeros) is air
// moving with it, and so is what comes in over the grid's sides. The grid follows the fire: when
// `fire.rs` moves it by whole cells, `carry` reads the step before that many cells away.
//
// The vorticity confinement is Fedkiw, Stam and Jensen's (2001), the growth of burning gas Feldman,
// O'Brien and Arikan's (2003), the stirring a curl of noise (Bridson, Houriham and Nordenstam 2007).

// Mirrors `StepUniforms` in fire.rs.
struct Step {
    shift: vec4<i32>,   // xyz = cells the grid moved since the step before, w = 1 for the first step of a grid: nothing before it
    map: vec4<f32>,     // xy = the grid's corner on the map of the ground, texels, z = the height of the grid's floor above the map's zero, m, w = emitters
    wind: vec4<f32>,    // xyz = the wind, m/s, grid axes, w = the fire's time, s
    place: vec4<f32>,   // xyz = the grid's corner in cells from the map's corner, modulo the noise's period: what keeps the noise still in the world
};

// Mirrors `GpuEmitter` in fire.rs.
struct Emitter {
    place: vec4<f32>,   // xyz = its middle, cells from the grid's corner, w = radius, cells
    push: vec4<f32>,    // xyz = the velocity it gives the gas, m/s, w = and outwards from its middle at its rim, m/s
    gives: vec4<f32>,   // per second: x = fuel, z = soot, w = dust; y = the heat it lights the gas to
    more: vec4<f32>,    // x = how ragged its outline is, 0..1, y = a number of its own, z = how fast the gas takes its velocity, 1/s
};

// Mirrors `LightUniforms` in fire.rs.
struct Lit {
    sun: vec4<f32>,     // xyz = towards the sun or the moon, grid axes
};

@group(0) @binding(0) var flow_in: texture_3d<f32>;
@group(0) @binding(1) var gas_in: texture_3d<f32>;
@group(0) @binding(2) var whirl_in: texture_3d<f32>;
@group(0) @binding(3) var flow_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(4) var gas_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(5) var whirl_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(6) var<storage, read_write> pressure: array<f32>;
@group(0) @binding(7) var<storage, read_write> spreading: array<f32>;
@group(0) @binding(8) var light_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(9) var glow_out: texture_storage_3d<rgba16float, write>;
@group(0) @binding(10) var glow_in: texture_3d<f32>;

@group(1) @binding(0) var<uniform> now: Step;
@group(1) @binding(1) var<storage, read> emitters: array<Emitter>;
@group(1) @binding(2) var<storage, read> ground: array<vec4<f32>>;
@group(1) @binding(3) var noise: texture_3d<f32>;
@group(1) @binding(4) var smooth_clamp: sampler;
@group(1) @binding(5) var smooth_repeat: sampler;
@group(1) @binding(6) var<uniform> lit: Lit;

// In `flow`'s fourth number: this cell is solid (ground, a building).
const SOLID: f32 = 30000.0;
// In `spreading`: the same.
const WALL: f32 = 1e30;

// Burning: the heat fuel lights at (from, fully), how much of it burns a second where there is
// little of it and where there is much (a rich cloud burns as fast as air gets into it), the heat
// a unit of it gives and the hottest it gets, the soot it leaves, how much the gas grows (volumes
// per unit burnt).
const LIGHTS_AT: vec2<f32> = vec2<f32>(0.22, 0.5);
const BURNS: vec2<f32> = vec2<f32>(1.7, 0.9);
const HEAT: f32 = 3.2;
const HOTTEST: f32 = 1.85;
const SOOT: f32 = 1.1;
const GROWS: f32 = 4.8;
// Cooling: by its own glow (with the fourth power of the heat) and into the air round it, 1/s.
const GLOW_COOLS: f32 = 0.07;
const AIR_COOLS: f32 = 0.1;
// Hot gas rises (m/s² per thousand kelvin); soot and dust weigh it down.
const LIFT: f32 = 2.6;
// How fast the air holds the fireball's cold smoke back, 1/s.
const HOLDS: f32 = 2.0;
const WEIGHT: f32 = 0.5;
// The confinement's strength, and the stirring's (m/s² at full heat).
const CONFINES: f32 = 1.6;
const STIRS: f32 = 4.0;
// Seconds in which soot thins away by itself, dust settles and fuel mist that never lit is gone.
const SOOT_STAYS: f32 = 140.0;
const DUST_STAYS: f32 = 50.0;
const MIST_STAYS: f32 = 6.0;
// The pressure's over-relaxation.
const OVER: f32 = 1.7;

fn inside(c: vec3<i32>) -> bool {
    return all(c >= vec3<i32>(0)) && all(c < DIM);
}

fn index(c: vec3<i32>) -> i32 {
    return c.x + NX * (c.y + NY * c.z);
}

// 1 well inside the grid, 0 half a cell outside it: what is read beyond the grid is the still air.
fn within(p: vec3<f32>) -> f32 {
    let low = saturate(p + 0.5);
    let high = saturate(SIZE - p + 0.5);
    return low.x * low.y * low.z * high.x * high.y * high.z;
}

fn ground_texel(at: vec2<i32>) -> vec4<f32> {
    let t = clamp(at, vec2<i32>(0), vec2<i32>(i32(MAP) - 1));
    return ground[t.x + i32(MAP) * t.y];
}

// The ground under a place (cells east and south of the grid's corner): x = the top of what is
// solid there, y, z = the foot and the top of the trees' crowns, all in cells above the grid's
// floor, w = how much of it is crown. Read between the map's texels.
fn ground_under(at: vec2<f32>) -> vec4<f32> {
    let texel = now.map.xy + at / MAP_CELLS - 0.5;
    let corner = vec2<i32>(floor(texel));
    let f = fract(texel);
    let a = ground_texel(corner);
    let b = ground_texel(corner + vec2<i32>(1, 0));
    let c = ground_texel(corner + vec2<i32>(0, 1));
    let d = ground_texel(corner + vec2<i32>(1, 1));
    let g = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    return vec4<f32>((g.xyz - now.map.z) / CELL, g.w);
}

@compute @workgroup_size(4, 4, 4)
fn whirl(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec3<i32>(id);
    if !inside(c) {
        return;
    }
    let most = DIM - vec3<i32>(1);
    let xa = textureLoad(flow_in, clamp(c - vec3<i32>(1, 0, 0), vec3<i32>(0), most), 0).xyz;
    let xb = textureLoad(flow_in, clamp(c + vec3<i32>(1, 0, 0), vec3<i32>(0), most), 0).xyz;
    let ya = textureLoad(flow_in, clamp(c - vec3<i32>(0, 1, 0), vec3<i32>(0), most), 0).xyz;
    let yb = textureLoad(flow_in, clamp(c + vec3<i32>(0, 1, 0), vec3<i32>(0), most), 0).xyz;
    let za = textureLoad(flow_in, clamp(c - vec3<i32>(0, 0, 1), vec3<i32>(0), most), 0).xyz;
    let zb = textureLoad(flow_in, clamp(c + vec3<i32>(0, 0, 1), vec3<i32>(0), most), 0).xyz;
    let w = vec3<f32>((yb.z - ya.z) - (zb.y - za.y), (zb.x - za.x) - (xb.z - xa.z), (xb.y - xa.y) - (yb.x - ya.x)) / (2.0 * CELL);
    // And how much the flow there spreads, 1/s: what `carry` thins the gas by.
    let opens = ((xb.x - xa.x) + (yb.y - ya.y) + (zb.z - za.z)) / (2.0 * CELL);
    textureStore(whirl_out, c, vec4<f32>(w, opens));
}

// The velocity (relative to the wind, m/s) at a place of the grid of the step before, in cells.
fn flow_at(p: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(flow_in, smooth_clamp, p / SIZE, 0.0).xyz * within(p);
}

// A flow without sources or sinks that changes from place to place and with time: the curl of
// three noises, m/s per unit. `p` is in cells of the world.
fn eddies(p: vec3<f32>, time: f32) -> vec3<f32> {
    let at = p / 56.0 + vec3<f32>(0.0, -time * 0.05, 0.0);
    let e = 1.5 / 128.0;
    let xa = textureSampleLevel(noise, smooth_repeat, at - vec3<f32>(e, 0.0, 0.0), 0.0).xyz;
    let xb = textureSampleLevel(noise, smooth_repeat, at + vec3<f32>(e, 0.0, 0.0), 0.0).xyz;
    let ya = textureSampleLevel(noise, smooth_repeat, at - vec3<f32>(0.0, e, 0.0), 0.0).xyz;
    let yb = textureSampleLevel(noise, smooth_repeat, at + vec3<f32>(0.0, e, 0.0), 0.0).xyz;
    let za = textureSampleLevel(noise, smooth_repeat, at - vec3<f32>(0.0, 0.0, e), 0.0).xyz;
    let zb = textureSampleLevel(noise, smooth_repeat, at + vec3<f32>(0.0, 0.0, e), 0.0).xyz;
    return vec3<f32>((yb.z - ya.z) - (zb.y - za.y), (zb.x - za.x) - (xb.z - xa.z), (xb.y - xa.y) - (yb.x - ya.x));
}

@compute @workgroup_size(4, 4, 4)
fn carry(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec3<i32>(id);
    if !inside(c) {
        return;
    }
    let here = vec3<f32>(c) + 0.5;
    let land = ground_under(here.xz);
    if here.y < land.x {
        textureStore(flow_out, c, vec4<f32>(0.0, 0.0, 0.0, SOLID));
        textureStore(gas_out, c, vec4<f32>(0.0));
        return;
    }
    var v = vec3<f32>(0.0);
    var gas = vec4<f32>(0.0);
    if now.shift.w == 0 {
        // Where what is here now was a step ago, in the grid as it lay then: back along the
        // velocity here, then again along the velocity found half-way.
        let was = here + vec3<f32>(now.shift.xyz);
        let half = was - flow_at(was) * (0.5 * DT / CELL);
        let source = was - flow_at(half) * (DT / CELL);
        let kept = within(source);
        v = flow_at(source);
        gas = textureSampleLevel(gas_in, smooth_clamp, source / SIZE, 0.0) * kept;

        // The whirls kept from smearing away: a push round each, towards where there is more of it.
        let old = c + now.shift.xyz;
        let most = DIM - vec3<i32>(1);
        if all(old >= vec3<i32>(1)) && all(old < most) {
            let w = textureLoad(whirl_in, old, 0);
            let rise = vec3<f32>(
                length(textureLoad(whirl_in, old + vec3<i32>(1, 0, 0), 0).xyz) - length(textureLoad(whirl_in, old - vec3<i32>(1, 0, 0), 0).xyz),
                length(textureLoad(whirl_in, old + vec3<i32>(0, 1, 0), 0).xyz) - length(textureLoad(whirl_in, old - vec3<i32>(0, 1, 0), 0).xyz),
                length(textureLoad(whirl_in, old + vec3<i32>(0, 0, 1), 0).xyz) - length(textureLoad(whirl_in, old - vec3<i32>(0, 0, 1), 0).xyz),
            );
            let size = length(rise);
            if size > 1e-4 {
                v += cross(rise / size, w.xyz) * (CONFINES * CELL * DT);
            }
            // Following values back along a flow that spreads keeps them as they were, though the
            // same gas now fills more room: thinned here by as much as the flow really spread
            // (the pressure never takes all of the growth in one step), so none is made or lost.
            let thinner = 1.0 / clamp(1.0 + w.w * DT, 0.6, 1.6);
            gas = vec4<f32>(gas.x * thinner, gas.y, gas.z * thinner, gas.w * thinner);
        }
    }

    // What feeds it.
    let world = here + now.place.xyz;
    for (var i = 0; i < i32(now.map.w); i++) {
        let e = emitters[i];
        let to = here - e.place.xyz;
        let far = dot(to, to) / (e.place.w * e.place.w);
        if far > 3.0 {
            continue;
        }
        let rough = textureSampleLevel(noise, smooth_repeat, world / 40.0 + vec3<f32>(e.more.y * 3.1, now.wind.w * 0.23 + e.more.y, e.more.y * 7.7), 0.0);
        let reach = 1.0 + e.more.x * (rough.r + rough.g - 1.0) * 1.4;
        let share = 1.0 - smoothstep(0.45, 1.0, sqrt(far) / max(reach, 0.2));
        if share <= 0.0 {
            continue;
        }
        gas += vec4<f32>(e.gives.x, 0.0, e.gives.z, e.gives.w) * (share * DT);
        gas.y = max(gas.y, e.gives.y * share);
        let wanted = e.push.xyz + to * inverseSqrt(dot(to, to) + 0.01) * e.push.w * min(sqrt(far), 1.0) - now.wind.xyz;
        v = mix(v, wanted, saturate(e.more.z * share * DT));
    }

    // Fuel burns where it is hot: heat, soot, and the gas takes up more room.
    let lit_up = smoothstep(LIGHTS_AT.x, LIGHTS_AT.y, gas.y);
    let burnt = min(gas.x, mix(BURNS.x, BURNS.y, saturate(gas.x / 1.5)) * DT * lit_up);
    gas.x -= burnt;
    gas.y = max(gas.y, min(gas.y + HEAT * burnt, HOTTEST));
    gas.z += SOOT * burnt;
    let growth = GROWS * burnt / DT;

    // It cools, rises, and thins.
    let heat = gas.y;
    // In its first seconds the fireball's cloud takes in so much air that what of it has stopped
    // glowing cools at once and is held where it is: it hangs over the place and drifts, while
    // the fire that burns on sends its smoke up.
    let early = 1.0 - smoothstep(6.0, 14.0, now.wind.w);
    // (from its second second what has burnt out goes dark quickly: a mass of smoke with fire inside)
    let dims = mix(0.9, 1.25, smoothstep(1.2, 2.5, now.wind.w));
    let into_air = mix(AIR_COOLS, AIR_COOLS * 4.0 * (1.0 - smoothstep(dims - 0.45, dims, heat)) + AIR_COOLS, early);
    gas.y = max(heat - DT * (GLOW_COOLS * heat * heat * heat * heat + into_air * heat), 0.0);
    v.y += DT * (LIFT * gas.y - WEIGHT * (gas.z + gas.w) * 0.3);
    v *= 1.0 - DT * HOLDS * early * saturate((gas.z + gas.w) * 5.0) * (1.0 - smoothstep(0.25, 0.9, gas.y));
    gas.x *= 1.0 - DT / MIST_STAYS * (1.0 - lit_up);
    // Soot thins away slowly; the last trace of it, at once (the march skips what is empty).
    gas.z *= 1.0 - DT / select(SOOT_STAYS, 0.4, gas.z < 0.003);
    gas.w *= 1.0 - DT / DUST_STAYS;

    // Hot gas churns.
    let stir = gas.y * 0.5 + min(gas.z, 1.0) * 0.06;
    if stir > 0.01 {
        v += eddies(world, now.wind.w) * (STIRS * stir * DT);
    }
    // The trees' crowns hold the air back.
    if land.w > 0.0 && here.y > land.y && here.y < land.z {
        v = mix(v, -now.wind.xyz, saturate(2.5 * land.w * DT));
    }
    // Nothing goes faster than the grid can follow.
    let speed = length(v);
    if speed > 70.0 {
        v *= 70.0 / speed;
    }
    textureStore(flow_out, c, vec4<f32>(v, growth));
    textureStore(gas_out, c, min(max(gas, vec4<f32>(0.0)), vec4<f32>(60.0)));
}

// The velocity across the face between a cell (its flow `mine`) and its neighbour at `to`, along
// `axis`: none into a solid, its own out of the grid.
fn across(mine: vec4<f32>, to: vec3<i32>, axis: i32) -> f32 {
    if to.y < 0 {
        return -mine[axis];
    }
    if !inside(to) {
        return mine[axis];
    }
    let other = textureLoad(flow_in, to, 0);
    if other.w > SOLID * 0.5 {
        return -mine[axis];
    }
    return other[axis];
}

@compute @workgroup_size(4, 4, 4)
fn spread(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec3<i32>(id);
    if !inside(c) {
        return;
    }
    let mine = textureLoad(flow_in, c, 0);
    if mine.w > SOLID * 0.5 {
        spreading[index(c)] = WALL;
        return;
    }
    var out = 0.0;
    for (var axis = 0; axis < 3; axis++) {
        var along = vec3<i32>(0);
        along[axis] = 1;
        out += 0.5 * (across(mine, c + along, axis) - across(mine, c - along, axis));
    }
    spreading[index(c)] = out - mine.w * CELL;
}

// The pressure of the neighbour at `to` of a cell whose own is `mine`: a solid pushes back as hard
// as it is pushed, the open air beyond the grid is at rest.
fn pressure_of(to: vec3<i32>, mine: f32) -> f32 {
    if to.y < 0 {
        return mine;
    }
    if !inside(to) {
        return 0.0;
    }
    let i = index(to);
    if spreading[i] > WALL * 0.5 {
        return mine;
    }
    return pressure[i];
}

fn settle(id: vec3<u32>, colour: i32) {
    // Half the cells: those of one colour of the chessboard.
    let c = vec3<i32>(i32(id.x) * 2 + ((i32(id.y) + i32(id.z) + colour) & 1), i32(id.y), i32(id.z));
    if !inside(c) {
        return;
    }
    let i = index(c);
    let s = spreading[i];
    if s > WALL * 0.5 {
        return;
    }
    let mine = pressure[i];
    let about = pressure_of(c + vec3<i32>(1, 0, 0), mine) + pressure_of(c - vec3<i32>(1, 0, 0), mine) + pressure_of(c + vec3<i32>(0, 1, 0), mine) + pressure_of(c - vec3<i32>(0, 1, 0), mine) + pressure_of(c + vec3<i32>(0, 0, 1), mine) + pressure_of(c - vec3<i32>(0, 0, 1), mine);
    pressure[i] = mine + OVER * ((about - s) / 6.0 - mine);
}

@compute @workgroup_size(4, 4, 4)
fn settle_white(@builtin(global_invocation_id) id: vec3<u32>) {
    settle(id, 0);
}

@compute @workgroup_size(4, 4, 4)
fn settle_black(@builtin(global_invocation_id) id: vec3<u32>) {
    settle(id, 1);
}

@compute @workgroup_size(4, 4, 4)
fn release(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec3<i32>(id);
    if !inside(c) {
        return;
    }
    let mine = textureLoad(flow_in, c, 0);
    if mine.w > SOLID * 0.5 {
        textureStore(flow_out, c, vec4<f32>(0.0));
        return;
    }
    let p = pressure[index(c)];
    var v = mine.xyz;
    for (var axis = 0; axis < 3; axis++) {
        var along = vec3<i32>(0);
        along[axis] = 1;
        let ahead = pressure_of(c + along, p);
        let behind = pressure_of(c - along, p);
        v[axis] -= 0.5 * (ahead - behind);
        // Not into a solid.
        let up = c + along;
        let down = c - along;
        if inside(up) && spreading[index(up)] > WALL * 0.5 {
            v[axis] = min(v[axis], 0.0);
        }
        if down.y < 0 || (inside(down) && spreading[index(down)] > WALL * 0.5) {
            v[axis] = max(v[axis], 0.0);
        }
    }
    textureStore(flow_out, c, vec4<f32>(v, 0.0));
}

// For the drawing, on a grid of half the cells each way: x = how much smoke lies between a place
// and the sun (its optical depth), y = and between it and the sky above, w = how much there is to
// draw round it (0: nothing, the march skips it); and in `glow_out` the light given off there.
@compute @workgroup_size(4, 4, 4)
fn lighting(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec3<i32>(id);
    if any(c * 2 >= DIM) {
        return;
    }
    let here = vec3<f32>(c) * 2.0 + 1.0;
    let gas = textureSampleLevel(gas_in, smooth_clamp, here / SIZE, 0.0);
    var to_sun = 0.0;
    var reach = 1.5;
    var p = here;
    for (var i = 0; i < 22; i++) {
        p += lit.sun.xyz * reach;
        if within(p) <= 0.0 {
            break;
        }
        to_sun += thickness(textureSampleLevel(gas_in, smooth_clamp, p / SIZE, 0.0)) * reach * CELL;
        reach *= 1.09;
    }
    var to_sky = 0.0;
    reach = 2.0;
    p = here;
    for (var i = 0; i < 10; i++) {
        // Up, leaning a little to one side and then the other: the sky is wide.
        p += vec3<f32>(0.35 * f32((i & 1) * 2 - 1), 1.0, 0.35 * f32((i & 2) - 1)) * reach;
        if within(p) <= 0.0 {
            break;
        }
        to_sky += thickness(textureSampleLevel(gas_in, smooth_clamp, p / SIZE, 0.0)) * reach * CELL;
        reach *= 1.2;
    }
    let seen = thickness(gas);
    textureStore(light_out, c, vec4<f32>(to_sun, to_sky, 0.0, seen));
    // With how much gas there is: its coarser levels tell the march where there is none.
    textureStore(glow_out, c, vec4<f32>(incandescence(gas.y) * flame_part(gas), seen));
}

// The fire's light, each level the average of eight texels of the level before.
@compute @workgroup_size(4, 4, 4)
fn glow_spread(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec3<i32>(textureDimensions(glow_out));
    let c = vec3<i32>(id);
    if any(c >= size) {
        return;
    }
    textureStore(glow_out, c, textureSampleLevel(glow_in, smooth_clamp, (vec3<f32>(c) + 0.5) / vec3<f32>(size), 0.0));
}
