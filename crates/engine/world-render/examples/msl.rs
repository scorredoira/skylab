//! Prints the Metal shader source that naga makes of the world's WGSL, as wgpu compiles it on a Mac, to
//! read what the GPU really runs. `cargo run -p world-render --example msl [unchecked] > shaders.metal`;
//! `unchecked` leaves out the bounds checks and loop guards, as `ShaderRuntimeChecks::unchecked()` does.
use engine::wgpu::naga;

fn main() {
    let source = include_str!("../src/shaders.wgsl");
    let module = naga::front::wgsl::parse_str(source).expect("parse");
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).expect("validate");
    let unchecked = std::env::args().any(|a| a == "unchecked");
    let policy = if unchecked { naga::proc::BoundsCheckPolicy::Unchecked } else { naga::proc::BoundsCheckPolicy::ReadZeroSkipWrite };
    let options = naga::back::msl::Options {
        lang_version: (3, 0),
        fake_missing_bindings: true,
        bounds_check_policies: naga::proc::BoundsCheckPolicies { index: policy, buffer: policy, image_load: policy, binding_array: policy },
        zero_initialize_workgroup_memory: false,
        force_loop_bounding: !unchecked,
        emit_int_div_checks: !unchecked,
        ..Default::default()
    };
    let pipeline = naga::back::msl::PipelineOptions::default();
    let (text, _) = naga::back::msl::write_string(&module, &info, &options, &pipeline).expect("msl");
    println!("{text}");
}
