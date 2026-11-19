// What the fire's simulation (fire_sim.wgsl) and its drawing (fire_draw.wgsl) agree on: the grid,
// what a cell holds and what that looks like. `fire.rs` writes the grid's numbers before this:
// NX, NY, NZ (cells east, up and south), CELL (a cell's side, m), DT (a step, s), MAP (texels a side
// of the map of the ground) and MAP_CELLS (cells to one of its texels), and heat.wgsl.
//
// A cell's gas is four numbers: x = fuel not yet burnt (1: a rich mist of it), y = heat, thousands
// of kelvin above the air, z = soot, w = dust.

const DIM: vec3<i32> = vec3<i32>(NX, NY, NZ);
const SIZE: vec3<f32> = vec3<f32>(f32(NX), f32(NY), f32(NZ));

// How much of the light a metre of each takes away (1/m per unit): soot, dust, the mist of fuel
// not alight, and fuel burning.
const SOOT_DARK: f32 = 0.8;
const DUST_DARK: f32 = 0.3;
const MIST_DARK: f32 = 0.012;
const FLAME_DARK: f32 = 0.5;

// How far a cell's gas is flame, 0..1, by its heat: soot that hot is burning itself, bright and
// easier to see through; cold, it is black smoke.
fn alight(heat: f32) -> f32 {
    return smoothstep(0.45, 1.25, heat);
}

// What a metre of a cell's gas takes of the light through it without glowing, 1/m: x = its smoke
// (the soot that is not alight), y = its dust (which does not veil a flame it is in), z = its
// mist of fuel.
fn smoke_parts(gas: vec4<f32>) -> vec3<f32> {
    let cold = 1.0 - alight(gas.y);
    return vec3<f32>(gas.z * SOOT_DARK * (0.2 + 0.8 * cold), gas.w * DUST_DARK * (0.08 + 0.92 * cold), gas.x * MIST_DARK);
}

// What a metre of it takes of the light through it and gives back as its own, by its heat, 1/m:
// its soot and its fuel alight. A flame is as bright as its heat makes it where it is deep enough
// to see no further into, and no brighter.
fn flame_part(gas: vec4<f32>) -> f32 {
    return (gas.z * SOOT_DARK * 0.5 + gas.x * FLAME_DARK) * alight(gas.y);
}

// All that a metre of it takes of the light through it, 1/m.
fn thickness(gas: vec4<f32>) -> f32 {
    let smoke = smoke_parts(gas);
    return smoke.x + smoke.y + smoke.z + flame_part(gas);
}
