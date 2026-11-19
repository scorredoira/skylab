
// ---------------------------------------------------------------- the ghost (`ghost.rs`)
//
// The best lap's car, drawn as `car_shade` has it but see-through: its faces towards the eye only,
// at a third of their opacity, a little blue, premultiplied.
@fragment
fn car_ghost_fragment(in: CarOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    if front_facing == (car.surface.w < 0.0) {
        discard;
    }
    let s = car_shade(in, front_facing);
    let a = 0.32 * s.alpha;
    return vec4<f32>((s.color + vec3<f32>(0.02, 0.05, 0.12)) * a, a);
}
