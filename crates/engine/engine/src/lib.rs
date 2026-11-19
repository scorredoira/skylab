//! Reusable pieces for 3D games: window, input, GPU, meshes, camera, sound, 2D overlay.
//! Knows nothing about any particular game.

pub mod app;
pub mod audio;
pub mod camera;
pub mod canvas;
pub mod clock;
pub mod controller;
pub mod files;
pub mod gltf;
pub mod gpu;
#[cfg(not(target_arch = "wasm32"))]
pub mod icon;
pub mod input;
pub mod mesh;
pub mod occlusion;
pub mod pipeline;
pub mod post;
#[cfg(not(target_arch = "wasm32"))]
pub mod program;
pub mod texture;
pub mod threads;
pub mod uniforms;
pub mod video;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use app::{render_to_png, Offscreen};
pub use app::{run, Game};
pub use camera::Camera;
pub use canvas::Canvas;
pub use controller::{pad_axis_named, pad_button_named, AxisState, ButtonState, Controller, ControllerPort, PadAxis, PadButton};
pub use gpu::Gpu;
#[cfg(not(target_arch = "wasm32"))]
pub use icon::set_icon;
pub use input::Input;
pub use mesh::{Mesh, MeshBuilder, Skin};
pub use occlusion::{ambient_occlusion, SkyVolume};
pub use post::Post;
pub use texture::{Kind, Texture, TextureError, TextureSource};
pub use uniforms::UniformArena;
/// The clock to measure real time with: std's natively, the browser's on the web (std's panics there).
pub use web_time::Instant;

// Games use these directly; re-exported so they need no version of their own.
pub use egui;
pub use wgpu;
pub use winit::keyboard::KeyCode;
