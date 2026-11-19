
// ---------------------------------------------------------------- the aerodromes' lights
//
// Every light of `sim_core::lights` is one instance (`lights.rs`, `LightIn` below) of a square
// that faces the camera, added over the scene. Its intensity towards the eye comes from its beam,
// its flash and how much of its system is lit; the illuminance it gives the eye, dimmed by the
// same haze as the ground, is drawn as a blob as wide as its lens or a pixel, whichever is more,
// with the scene's brightness per candela per square metre (`frame.lights[0].x`, see lights.rs).
// Its brightness is that of the light spread over its lens or over what the eye resolves
// (`ACUITY`), whichever is more: a pixel is coarser than the eye, and a light smaller than both
// looks to the eye as bright as the eye's resolution makes it, not as dim as a pixel would. So a
// PAPI shows by day as the dot it is, and a light looks the same at any size of the picture.
// In mist and fog the light the air scatters out of the way and on towards the eye shows as a glow
// round the light, as wide as the fog is thick (see `halo_of`).

// The red of a slope indicator's filter, luminance 1 (`lights::rgb(Color::Red)`), and the share of
// the white it lets through (Annex 14, Appendix 2).
const AVIATION_RED = vec3<f32>(4.407, 0.088, 0.000);
const RED_FILTER: f32 = 0.15;
// The radius of the smallest spot the eye sees a point of light as, radians: two minutes of arc.
const ACUITY: f32 = 0.0006;
// The glow in mist and fog. The droplets of a fog scatter mostly forwards (an asymmetry of 0.85),
// into a lobe some degrees wide: of the light taken out of the way, about half goes on towards the
// eye within it. Seen through a thin mist that is a tight halo; through more of it the light is
// scattered again and again and the glow widens and dims, as much light spread wider (the
// atmospheric point spread function, Narasimhan and Nayar 2003: some degrees across through an
// optical depth of two, a fifth of the sky through four). `HALO_SHARE` is the part of what was
// taken out that is seen in the glow, spread as a Gaussian of radius `HALO_RADIUS` in fog
// (`HALO_HAZE` in haze, whose finer aerosol scatters wider) times 1 + 2.5 τ + τ² for an optical
// depth τ. Only its middle is drawn, as its peak falling off within `HALO_EXTENT`: drawn whole, each
// light would be a square of half the screen. Off in `low` (`pipelines.rs`).
const LIGHT_HALOS: bool = true;
const HALO_SHARE: f32 = 0.5;
const HALO_RADIUS: f32 = 0.012;
const HALO_HAZE: f32 = 0.3;
const HALO_EXTENT: f32 = 0.12;
// The brightest a light is drawn, so that a lamp seen from a metre stays within a half float.
const BRIGHTEST: f32 = 30000.0;
// How far the square reaches, in radii of the core and of the halo: each blob is let fall to
// nothing at its edge, so that no square shows round a bright light.
const CORE_REACH: f32 = 2.5;
const HALO_REACH: f32 = 2.0;

struct LightIn {
    @location(0) place: vec4<f32>,  // xyz = where, from the aerodrome's origin (`chunk.offset`), w = radius of the lens
    @location(1) color: vec4<f32>,  // rgb = colour of luminance 1, w = intensity of the main beam, cd
    @location(2) beam: vec4<f32>,   // its axis, heading and elevation, and its half width and height, radians; no beam: z = 0
    @location(3) more: vec4<f32>,   // x = intensity all round, cd, y = turn of the beam, rad/s, z = its system, w = offset of its flash, s
    @location(4) slope: vec4<f32>,  // x = a slope indicator's angle (0: none), y = half its transition, z = period of the flash (0: steady), w = its length, s
};

struct LightOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,                             // in radii of the core
    @location(1) @interpolate(flat) core: vec3<f32>,        // its brightness in the middle
    @location(2) @interpolate(flat) halo: vec4<f32>,        // rgb = the halo's in the middle, w = its radius in radii of the core
    @location(3) @interpolate(flat) edge: f32,              // how far it is drawn, in radii of the core
};

// Modulo that keeps the sign of the divisor.
fn wrap_to(x: f32, m: f32) -> f32 {
    return x - floor(x / m) * m;
}

@vertex
fn aerodrome_light_vertex(@builtin(vertex_index) vid: u32, l: LightIn) -> LightOut {
    var o: LightOut;
    o.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);   // off the screen
    let rel = l.place.xyz + chunk.offset.xyz;
    let d = max(length(rel), 0.1);
    let to_eye = -rel / d;
    // A vehicle's lamp (past the eight systems) is lit as the game gives it.
    let system = u32(l.more.z);
    var setting = 1.0;
    if system < 8u {
        setting = frame.lights[1u + system / 4u][system % 4u];
    }
    let fade = 1.0 - smoothstep(frame.lights[0].z, frame.lights[0].w, d);
    if setting <= 0.0 || fade <= 0.0 {
        return o;
    }
    let time = frame.sun_dir.w;
    let elevation = asin(clamp(to_eye.y, -1.0, 1.0));
    var cd = l.more.x * smoothstep(-0.1, 0.0, to_eye.y);
    if l.beam.z > 0.0 {
        let heading = l.beam.x + l.more.y * time;
        let azimuth = atan2(to_eye.x, -to_eye.z);
        let off = vec2<f32>((wrap_to(azimuth - heading + PI, 2.0 * PI) - PI) / l.beam.z, (elevation - l.beam.y) / l.beam.w);
        cd += l.color.w * exp(-0.6931 * dot(off, off));
    }
    if l.slope.z > 0.0 && wrap_to(time - l.more.w, l.slope.z) > l.slope.w {
        cd = 0.0;
    }
    var color = l.color.rgb;
    if l.slope.x > 0.0 {
        color = mix(AVIATION_RED * RED_FILTER, color, smoothstep(l.slope.x - l.slope.y, l.slope.x + l.slope.y, elevation));
    }
    // The illuminance at the eye, lux, and what is left of it through the haze.
    let lux = cd * setting * fade / (d * d);
    let through = haze_through(rel);
    let gain = frame.lights[0].x;
    let theta = max(l.place.w / d, frame.lights[0].y);
    let seen = max(l.place.w / d, ACUITY);
    let core = min(color * through * (lux * gain / (PI * seen * seen)), vec3<f32>(BRIGHTEST));
    var reach = CORE_REACH;
    var halo = vec4<f32>(0.0);
    let kept = dot(through, vec3<f32>(0.2126, 0.7152, 0.0722));
    let tau = -log(max(kept, 1e-6));
    // The droplets of a fog (the air seen less than 2 km through) throw the light into a narrow lobe;
    // the fine aerosol of a haze (more than 10 km) spreads it over tens of degrees, a glow too faint
    // and wide to draw.
    let visibility = 3.912 * d / max(tau, 1e-6);
    let radius = mix(HALO_RADIUS, HALO_HAZE, smoothstep(2000.0, 10000.0, visibility)) * (1.0 + 2.5 * tau + tau * tau);
    if LIGHT_HALOS && radius > theta {
        // The glow spread over its disc: a Gaussian of that radius holds pi r² of its middle.
        let h = lux * gain * (1.0 - kept) * HALO_SHARE / (PI * radius * radius);
        // Only where it shows: a halo is a big square.
        if h > 0.003 {
            // Drawn as its middle, narrower: its peak, falling off within the square.
            halo = vec4<f32>(min(color * h, vec3<f32>(BRIGHTEST)), min(radius, HALO_EXTENT / HALO_REACH) / theta);
            reach = HALO_REACH * halo.w;
        }
    }
    if max(core.r, max(core.g, core.b)) < 1e-4 && halo.w == 0.0 {
        return o;
    }
    var corners = CORNERS;
    let c = corners[vid];
    let p = rel + (frame.cam_right.xyz * c.x + frame.cam_up.xyz * c.y) * (reach * theta * d);
    o.position = frame.view_proj * vec4<f32>(p, 1.0);
    // Its depth is that of a point nearer the eye by the blob's size, so that the ground or the post
    // it stands on does not hide it; a halo's, near the eye, as it is light scattered all along the
    // way, some of it in front of whatever stands between (cut at the middle of the way, the ground
    // there would cut a wide glow in a straight line).
    let pull = select(min(reach * theta * d + 0.5, d * 0.5), d * 0.85, halo.w > 0.0);
    let near = frame.view_proj * vec4<f32>(rel + to_eye * pull, 1.0);
    o.position.z = near.z / near.w * o.position.w;
    o.uv = c * reach;
    o.core = core;
    o.halo = halo;
    o.edge = reach;
    return o;
}

@fragment
fn aerodrome_light_fragment(in: LightOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.uv, in.uv);
    var light = in.core * max(exp(-r2) - exp(-CORE_REACH * CORE_REACH), 0.0);
    if in.halo.w > 0.0 {
        light += in.halo.rgb * max(exp(-r2 / (in.halo.w * in.halo.w)) - exp(-in.edge * in.edge / (in.halo.w * in.halo.w)), 0.0);
    }
    return vec4<f32>(light, 0.0);
}
