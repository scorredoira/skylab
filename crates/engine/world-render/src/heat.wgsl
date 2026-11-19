// The light of what is hot, for the fire's grid (fire_common.wgsl) and the balls of burning gas
// (effects.wgsl).

// The colour and brightness of soot glowing at `heat` thousand kelvin above the air: Planck's law
// at 610, 550 and 465 nm, relative to the red of 1900 K, its range pressed together the way a
// picture exposed for the fire holds it (red at 1000 K is a ten-thousandth of red at 1900 K; here
// a fortieth). No hotter than 2100 K: the soot of a fuel fire in the open is not, and what a
// picture shows of a flame is orange and yellow.
fn incandescence(heat: f32) -> vec3<f32> {
    let kelvin = 300.0 + 1000.0 * min(heat, 1.8);
    if kelvin < 780.0 {
        return vec3<f32>(0.0);
    }
    // h c / (k lambda) for the three colours, and lambda_red^5 / lambda^5.
    let spectral = vec3<f32>(1.0, 1.678, 3.885) * 246000.0 / (exp(vec3<f32>(23587.0, 26160.0, 30942.0) / kelvin) - 1.0);
    return spectral / spectral.r * pow(spectral.r, 0.4) * smoothstep(780.0, 1000.0, kelvin);
}
