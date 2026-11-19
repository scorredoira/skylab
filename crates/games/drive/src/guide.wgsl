
// ---------------------------------------------------------------- the driving line (`guide.rs`)
//
// Arrows painted on the road ahead: `normal.x` runs from -1 to 1 across an arm, `color` is the
// arrow's colour and how much of it shows.
struct GuideOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) across: f32,
    @location(2) color: vec4<f32>,
};

@vertex
fn guide_vertex(in: CarIn) -> GuideOut {
    var o: GuideOut;
    let rel = object.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.across = in.normal.x;
    o.color = in.color;
    return o;
}

@fragment
fn guide_fragment(in: GuideOut) -> @location(0) vec4<f32> {
    // Soft at the arms' edges; as bright as paint in the open is by day, and no darker than
    // `GUIDE_GLOW` in the shade or by night: it is a sign to the driver, not a thing of the road.
    let edge = 1.0 - smoothstep(0.7, 1.0, abs(in.across));
    let a = in.color.a * edge * 0.9;
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let light = light_at(in.rel) * max(frame.sun_dir.y, 0.0) + sky_light(up, in.rel);
    let level = max(max(light.r, light.g), light.b);
    let lit = in.color.rgb * max(0.8 * level, GUIDE_GLOW);
    return vec4<f32>(apply_fog(lit, in.rel) * a, a);
}

// The least the arrows shine, whatever the light (scene units, after the exposure).
const GUIDE_GLOW: f32 = 1.6;
