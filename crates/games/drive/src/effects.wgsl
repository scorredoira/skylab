// What the tyres leave behind (`effects.rs`): the marks of rubber they lay on the asphalt when
// they slide, and puffs: the smoke of a sliding tyre, the dust of gravel and grass, the spray of
// a wet road. With the world's shaders and the car's (`car.wgsl`), in one module.

struct MarkOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) across: f32,
    @location(2) strength: f32,
};

// A mark's vertex: `normal.x` runs from -1 to 1 across the mark, `color.a` is how dark it is there.
@vertex
fn mark_vertex(in: CarIn) -> MarkOut {
    var o: MarkOut;
    let rel = object.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.across = in.normal.x;
    o.strength = in.color.a;
    return o;
}

@fragment
fn mark_fragment(in: MarkOut) -> @location(0) vec4<f32> {
    // Soft at the edges, streaked along by the tread's ribs.
    let edge = 1.0 - smoothstep(0.65, 1.0, abs(in.across));
    let ribs = 0.92 + 0.08 * cos(in.across * 9.0);
    let a = saturate(in.strength * 1.4) * edge * ribs * 0.85;
    // Rubber: nearly black whatever the light; the haze over it as over the road.
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let lit = vec3<f32>(0.012) * (light_at(in.rel) * max(frame.sun_dir.y, 0.0) + sky_light(up, in.rel));
    return vec4<f32>(apply_fog(lit, in.rel) * a, a);
}

struct PuffOut {
    @builtin(position) position: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) seed: f32,
};

// A puff's vertex: a corner of a square that faces the eye, already placed; `normal.xy` is the
// corner (-1 to 1), `normal.z` a number of the puff's own.
@vertex
fn puff_vertex(in: CarIn) -> PuffOut {
    var o: PuffOut;
    let rel = object.model * vec4<f32>(in.position, 1.0);
    o.position = frame.view_proj * rel;
    o.rel = rel.xyz;
    o.uv = in.normal.xy;
    o.seed = in.normal.z;
    o.color = in.color;
    return o;
}

@fragment
fn puff_fragment(in: PuffOut) -> @location(0) vec4<f32> {
    if in.seed < 0.0 {
        // A drop of rain: a thin streak, brightest down its middle, lit by the sky.
        let a = (1.0 - in.uv.x * in.uv.x) * (1.0 - in.uv.y * in.uv.y) * in.color.a;
        let lit = in.color.rgb * (sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel) * 2.2 + light_at(in.rel) * 0.4);
        return vec4<f32>(apply_fog(lit, in.rel) * a, a);
    }
    let r2 = dot(in.uv, in.uv);
    if r2 >= 1.0 {
        discard;
    }
    // A soft ball, lumpy: two scales of noise turned by the puff's own number.
    let q = in.uv * 1.7 + vec2<f32>(in.seed * 37.0, in.seed * 91.0);
    let lumps = vnoise(q, 4096) * 0.6 + vnoise(q * 2.3 + 5.0, 4096) * 0.4;
    let body = (1.0 - r2) * (1.0 - r2) * (0.55 + 0.9 * lumps);
    let a = saturate(body) * in.color.a;
    // Lit by the sun through it and by the sky: brighter towards the side the sun is on.
    let sun = light_at(in.rel) * cloud_shadow(in.rel) * terrain_shadow(in.rel);
    let side = 0.65 + 0.35 * dot(normalize(vec3<f32>(in.uv.x, in.uv.y, 0.6)), vec3<f32>(dot(frame.sun_dir.xyz, frame.cam_right.xyz), dot(frame.sun_dir.xyz, frame.cam_up.xyz), -dot(frame.sun_dir.xyz, frame.cam_fwd.xyz)));
    let lit = in.color.rgb * (sun * 0.55 * side + sky_light(vec3<f32>(0.0, 1.0, 0.0), in.rel) * 0.9);
    return vec4<f32>(apply_fog(lit, in.rel) * a, a);
}
